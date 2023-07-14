//! DBSP Pipeline Manager provides an HTTP API to catalog, compile, and execute
//! SQL programs.
//!
//! The API is currently single-tenant: there is no concept of users or
//! permissions.  Multi-tenancy can be implemented by creating a manager
//! instance per tenant, which enables better separation of concerns,
//! resource isolation and fault isolation compared to buiding multitenancy
//! into the manager.
//!
//! # Architecture
//!
//! * Project database.  Programs (including SQL source code), configs, and
//!   pipelines are stored in a Postgres database.  The database is the only
//!   state that is expected to survive across server restarts.  Intermediate
//!   artifacts stored in the file system (see below) can be safely deleted.
//!
//! * Compiler.  The compiler generates a binary crate for each program and adds
//!   it to a cargo workspace that also includes libraries that come with the
//!   SQL libraries.  This way, all precompiled dependencies of the main crate
//!   are reused across programs, thus speeding up compilation.
//!
//! * Runner.  The runner component is responsible for starting and killing
//!   compiled pipelines and for interacting with them at runtime.

use actix_web::dev::Service;
use actix_web::{
    delete,
    dev::{ServiceFactory, ServiceRequest},
    get,
    http::{
        header::{CacheControl, CacheDirective},
        Method,
    },
    middleware::{Condition, Logger},
    patch, post, rt,
    web::Data as WebData,
    web::{self, ReqData},
    App, Error as ActixError, HttpRequest, HttpResponse, HttpServer, ResponseError,
};
use actix_web_httpauth::middleware::HttpAuthentication;
use actix_web_static_files::ResourceFiles;
use anyhow::{Error as AnyError, Result as AnyResult};
use auth::JwkCache;
use clap::Parser;
use colored::Colorize;
#[cfg(unix)]
use daemonize::Daemonize;
use dbsp_adapters::{ControllerError, ErrorResponse};
use env_logger::Env;
use log::debug;
use serde::{Deserialize, Serialize};
use std::{
    env, io::Write,
    fs::{read, write},
    net::TcpListener,
    sync::Arc,
    time::Duration,
};
use tokio::sync::Mutex;
use utoipa::{openapi::OpenApi as OpenApiDoc, OpenApi, ToSchema};
use utoipa_swagger_ui::SwaggerUi;
use uuid::{uuid, Uuid};

mod auth;
mod compiler;
mod config;
mod db;
mod error;
#[cfg(test)]
#[cfg(feature = "integration-test")]
mod integration_test;
mod runner;

pub(crate) use compiler::{Compiler, ProgramStatus};
pub(crate) use config::ManagerConfig;
use db::{
    storage::Storage, AttachedConnector, AttachedConnectorId, ConnectorId, DBError, PipelineId,
    PipelineRevision, ProgramDescr, ProgramId, ProjectDB, Version,
};
pub use error::ManagerError;
use runner::{Runner, RunnerError};

use crate::auth::TenantId;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "DBSP API",
        description = r"API to catalog, compile, and execute SQL programs.

# API concepts

* *Program*.  A program is a SQL script with a unique name and a unique ID
  attached to it.  The client can add, remove, modify, and compile programs.
  Compilation includes running the SQL-to-DBSP compiler followed by the Rust
  compiler.

* *Configuration*.  A program can have multiple configurations associated with
  it.  Similar to programs, one can add, remove, and modify configs.

* *Pipeline*.  A pipeline is a running instance of a compiled program based on
  one of the configs.  Clients can start multiple pipelines for a program with
  the same or different configs.

# Concurrency

The API prevents race conditions due to multiple users accessing the same
program or configuration concurrently.  An example is user 1 modifying the program,
while user 2 is starting a pipeline for the same program.  The pipeline
may end up running the old or the new version, potentially leading to
unexpected behaviors.  The API prevents such situations by associating a
monotonically increasing version number with each program and configuration.
Every request to compile the program or start a pipeline must include program
id _and_ version number. If the version number isn't equal to the current
version in the database, this means that the last version of the program
observed by the user is outdated, so the request is rejected."
    ),
    paths(
        list_programs,
        program_code,
        program_status,
        new_program,
        update_program,
        compile_program,
        cancel_program,
        delete_program,
        new_pipeline,
        update_pipeline,
        list_pipelines,
        pipeline_stats,
        pipeline_status,
        pipeline_validate,
        pipeline_action,
        pipeline_committed,
        pipeline_delete,
        list_connectors,
        new_connector,
        update_connector,
        connector_status,
        delete_connector,
        http_input,
        http_output,
    ),
    components(schemas(
        compiler::SqlCompilerMessage,
        db::AttachedConnector,
        db::ProgramDescr,
        db::ProgramSchema,
        db::Relation,
        db::Field,
        db::ConnectorDescr,
        db::Pipeline,
        db::PipelineDescr,
        db::PipelineRevision,
        db::Revision,
        db::PipelineStatus,
        dbsp_adapters::EgressMode,
        dbsp_adapters::PipelineConfig,
        dbsp_adapters::InputEndpointConfig,
        dbsp_adapters::NeighborhoodQuery,
        dbsp_adapters::OutputEndpointConfig,
        dbsp_adapters::OutputQuery,
        dbsp_adapters::TransportConfig,
        dbsp_adapters::FormatConfig,
        dbsp_adapters::transport::FileInputConfig,
        dbsp_adapters::transport::FileOutputConfig,
        dbsp_adapters::transport::KafkaInputConfig,
        dbsp_adapters::transport::KafkaOutputConfig,
        dbsp_adapters::transport::KafkaLogLevel,
        dbsp_adapters::transport::http::Chunk,
        dbsp_adapters::format::CsvEncoderConfig,
        dbsp_adapters::format::CsvParserConfig,
        TenantId,
        ProgramId,
        PipelineId,
        ConnectorId,
        AttachedConnectorId,
        Version,
        ProgramStatus,
        ErrorResponse,
        ProgramCodeResponse,
        NewProgramRequest,
        NewProgramResponse,
        UpdateProgramRequest,
        UpdateProgramResponse,
        CompileProgramRequest,
        CancelProgramRequest,
        NewPipelineRequest,
        NewPipelineResponse,
        UpdatePipelineRequest,
        UpdatePipelineResponse,
        NewConnectorRequest,
        NewConnectorResponse,
        UpdateConnectorRequest,
        UpdateConnectorResponse,
    ),),
    tags(
        (name = "Program", description = "Manage programs"),
        (name = "Pipeline", description = "Manage pipelines"),
        (name = "Connector", description = "Manage data connectors"),
    ),
)]
pub struct ApiDoc;

fn main() -> AnyResult<()> {
    // Stay in single-threaded mode (no tokio) until calling `daemonize`.

    // Create env logger.
    let name = "[manager]".cyan();
    env_logger::Builder::from_env(Env::default().default_filter_or("info"))
        .format(move |buf, record| {
            let t = chrono::Utc::now();
            let t = format!("{}", t.format("%Y-%m-%d %H:%M:%S"));
            writeln!(
                buf,
                "{} {} {} {}",
                t,
                buf.default_styled_level(record.level()),
                name,
                record.args()
            )
        })
        .init();

    let mut config = ManagerConfig::try_parse()?;

    if config.dump_openapi {
        let openapi_json = ApiDoc::openapi().to_json()?;
        write("openapi.json", openapi_json.as_bytes())?;
        return Ok(());
    }

    if config.precompile {
        rt::System::new().block_on(Compiler::precompile_dependencies(&config))?;
        return Ok(());
    }

    if let Some(config_file) = &config.config_file {
        let config_yaml = read(config_file).map_err(|e| {
            AnyError::msg(format!("error reading config file '{config_file}': {e}"))
        })?;
        let config_yaml = String::from_utf8_lossy(&config_yaml);
        config = serde_yaml::from_str(&config_yaml).map_err(|e| {
            AnyError::msg(format!("error parsing config file '{config_file}': {e}"))
        })?;
    }

    let config = config.canonicalize()?;

    run(config)
}

struct ServerState {
    // Serialize DB access with a lock, so we don't need to deal with
    // transaction conflicts.  The server must avoid holding this lock
    // for a long time to avoid blocking concurrent requests.
    db: Arc<Mutex<ProjectDB>>,
    // Dropping this handle kills the compiler task.
    _compiler: Option<Compiler>,
    runner: Runner,
    _config: ManagerConfig,
    pub jwk_cache: Arc<Mutex<JwkCache>>,
}

impl ServerState {
    async fn new(
        config: ManagerConfig,
        db: Arc<Mutex<ProjectDB>>,
        compiler: Option<Compiler>,
    ) -> AnyResult<Self> {
        let runner = Runner::local(db.clone(), &config);

        Ok(Self {
            db,
            _compiler: compiler,
            runner,
            _config: config,
            jwk_cache: Arc::new(Mutex::new(JwkCache::new())),
        })
    }
}

fn run(config: ManagerConfig) -> AnyResult<()> {
    // Check that the port is available before turning into a daemon, so we can fail
    // early if the port is taken.
    let listener = TcpListener::bind((config.bind_address.clone(), config.port)).map_err(|e| {
        AnyError::msg(format!(
            "failed to bind port '{}:{}': {e}",
            &config.bind_address, config.port
        ))
    })?;

    #[cfg(unix)]
    if config.unix_daemon {
        let logfile = std::fs::File::create(config.logfile.as_ref().unwrap()).map_err(|e| {
            AnyError::msg(format!(
                "failed to create log file '{}': {e}",
                &config.logfile.as_ref().unwrap()
            ))
        })?;

        let logfile_clone = logfile.try_clone().unwrap();

        let daemonize = Daemonize::new()
            .pid_file(config.manager_pid_file_path())
            .working_directory(&config.working_directory)
            .stdout(logfile_clone)
            .stderr(logfile);

        daemonize.start().map_err(|e| {
            AnyError::msg(format!(
                "failed to detach server process from terminal: '{e}'",
            ))
        })?;
    }

    let dev_mode = config.dev_mode;
    let use_auth = config.use_auth;
    rt::System::new().block_on(async {
        let db = ProjectDB::connect(&config).await?;
        let db = Arc::new(Mutex::new(db));
        let compiler = Compiler::new(&config, db.clone()).await?;

        // Since we don't trust any file system state after restart,
        // reset all programs to `ProgramStatus::None`, which will force
        // us to recompile programs before running them.
        db.lock().await.reset_program_status().await?;
        let openapi = ApiDoc::openapi();

        let state = WebData::new(ServerState::new(config, db, Some(compiler)).await?);

        if use_auth {
            let server = HttpServer::new(move || {
                let auth_middleware = HttpAuthentication::with_fn(auth::auth_validator);
                let auth_configuration = auth::aws_auth_config();

                let app = App::new()
                    .app_data(state.clone())
                    .app_data(auth_configuration)
                    .wrap(Logger::default())
                    .wrap(Condition::new(dev_mode, actix_cors::Cors::permissive()))
                    .wrap(auth_middleware);
                build_app(app, openapi.clone())
            });
            server.listen(listener)?.run().await?;
        } else {
            let server = HttpServer::new(move || {
                let app = App::new()
                    .app_data(state.clone())
                    .wrap(Logger::default())
                    .wrap(Condition::new(dev_mode, actix_cors::Cors::permissive()))
                    .wrap_fn(|req, srv| {
                        let req = auth::tag_with_default_tenant_id(req);
                        srv.call(req)
                    });
                build_app(app, openapi.clone())
            });
            server.listen(listener)?.run().await?;
        }
        Ok(())
    })
}

// `static_files` magic.
include!(concat!(env!("OUT_DIR"), "/generated.rs"));

fn build_app<T>(app: App<T>, openapi: OpenApiDoc) -> App<T>
where
    T: ServiceFactory<ServiceRequest, Config = (), Error = ActixError, InitError = ()>,
{
    // Creates a dictionary of static files indexed by file name.
    let generated = generate();

    app.service(list_programs)
        .service(program_code)
        .service(program_status)
        .service(new_program)
        .service(update_program)
        .service(compile_program)
        .service(delete_program)
        .service(new_pipeline)
        .service(update_pipeline)
        .service(list_pipelines)
        .service(pipeline_stats)
        .service(pipeline_status)
        .service(pipeline_action)
        .service(pipeline_validate)
        .service(pipeline_committed)
        .service(pipeline_delete)
        .service(list_connectors)
        .service(new_connector)
        .service(update_connector)
        .service(connector_status)
        .service(delete_connector)
        .service(http_input)
        .service(http_output)
        .service(SwaggerUi::new("/swagger-ui/{_:.*}").url("/api-doc/openapi.json", openapi))
        .service(ResourceFiles::new("/", generated))
}

// Example errors for use in OpenApi docs.

fn example_pipeline_toml() -> String {
    let input_connector = db::ConnectorDescr {
        connector_id: ConnectorId(uuid!("01890c99-376f-743e-ac30-87b6c0ce74ef")),
        name: "Input".into(),
        description: "My Input Connector".into(),
        config: "format: csv\n".into(),
    };
    let input = db::AttachedConnector {
        name: "Input-To-Table".into(),
        is_input: true,
        connector_id: input_connector.connector_id,
        config: "my_input_table".into(),
    };
    let output_connector = db::ConnectorDescr {
        connector_id: ConnectorId(uuid!("01890c99-3734-7052-9e97-55c0679a5adb")),
        name: "Output ".into(),
        description: "My Output Connector".into(),
        config: "format: csv\n".into(),
    };
    let output = db::AttachedConnector {
        name: "Output-To-View".into(),
        is_input: false,
        connector_id: output_connector.connector_id,
        config: "my_output_view".into(),
    };
    let pipeline = db::PipelineDescr {
        pipeline_id: PipelineId(uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8")),
        program_id: Some(ProgramId(uuid!("2e79afe1-ff4d-44d3-af5f-9397de7746c0"))),
        name: "My Pipeline".into(),
        description: "My Description".into(),
        config: "workers: 8\n".into(),
        attached_connectors: vec![input, output],
        version: Version(1),
    };

    let connectors = vec![input_connector, output_connector];
    PipelineRevision::generate_toml_config(&pipeline, &connectors).unwrap()
}

fn example_unknown_program() -> ErrorResponse {
    ErrorResponse::from_error_nolog(&DBError::UnknownProgram {
        program_id: ProgramId(uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8")),
    })
}

fn example_duplicate_name() -> ErrorResponse {
    ErrorResponse::from_error_nolog(&DBError::DuplicateName)
}

fn example_outdated_program_version() -> ErrorResponse {
    ErrorResponse::from_error_nolog(&DBError::OutdatedProgramVersion {
        expected_version: Version(5),
    })
}

fn example_unknown_pipeline() -> ErrorResponse {
    ErrorResponse::from_error_nolog(&DBError::UnknownPipeline {
        pipeline_id: PipelineId(uuid!("2e79afe1-ff4d-44d3-af5f-9397de7746c0")),
    })
}

fn example_unknown_connector() -> ErrorResponse {
    ErrorResponse::from_error_nolog(&DBError::UnknownConnector {
        connector_id: ConnectorId(uuid!("d764b9e2-19f2-4572-ba20-8b42641b07c4")),
    })
}

fn example_unknown_name() -> ErrorResponse {
    ErrorResponse::from_error_nolog(&DBError::UnknownName {
        name: "unknown_name".to_string(),
    })
}

fn example_unknown_input_table(table: &str) -> ErrorResponse {
    ErrorResponse::from_error_nolog(&ControllerError::unknown_input_stream(table))
}

fn example_unknown_output_table(table: &str) -> ErrorResponse {
    ErrorResponse::from_error_nolog(&ControllerError::unknown_output_stream(table))
}

fn example_unknown_input_format() -> ErrorResponse {
    ErrorResponse::from_error_nolog(&ControllerError::unknown_input_format("xml"))
}

fn example_parse_error() -> ErrorResponse {
    ErrorResponse::from_error_nolog(&ControllerError::parse_error(
        "api-ingress-my_table-d24e60a3-9058-4751-aa6b-b88f4ddfd7bd",
        anyhow::Error::msg("missing field 'column_name'"),
    ))
}

fn example_unknown_output_format() -> ErrorResponse {
    ErrorResponse::from_error_nolog(&ControllerError::unknown_output_format("xml"))
}

fn example_pipeline_shutdown() -> ErrorResponse {
    ErrorResponse::from_error_nolog(&RunnerError::PipelineShutdown {
        pipeline_id: PipelineId(uuid!("2e79afe1-ff4d-44d3-af5f-9397de7746c0")),
    })
}

fn example_program_not_set() -> ErrorResponse {
    ErrorResponse::from_error_nolog(&DBError::ProgramNotSet)
}

fn example_program_not_compiled() -> ErrorResponse {
    ErrorResponse::from_error_nolog(&DBError::ProgramNotCompiled)
}

fn example_program_has_errors() -> ErrorResponse {
    ErrorResponse::from_error_nolog(&DBError::ProgramFailedToCompile)
}

fn example_pipline_invalid_input_ac() -> ErrorResponse {
    ErrorResponse::from_error_nolog(&DBError::TablesNotInSchema {
        missing: vec![("ac_name".to_string(), "my_table".to_string())],
    })
}

fn example_pipline_invalid_output_ac() -> ErrorResponse {
    ErrorResponse::from_error_nolog(&DBError::ViewsNotInSchema {
        missing: vec![("ac_name".to_string(), "my_view".to_string())],
    })
}

fn example_pipeline_timeout() -> ErrorResponse {
    ErrorResponse::from_error_nolog(&RunnerError::PipelineInitializationTimeout {
        pipeline_id: PipelineId(uuid!("2e79afe1-ff4d-44d3-af5f-9397de7746c0")),
        timeout: Duration::from_millis(10_000),
    })
}

fn example_invalid_uuid_param() -> ErrorResponse {
    ErrorResponse::from_error_nolog(&ManagerError::InvalidUuidParam{value: "not_a_uuid".to_string(), error: "invalid character: expected an optional prefix of `urn:uuid:` followed by [0-9a-fA-F-], found `n` at 1".to_string()})
}

fn example_program_not_specified() -> ErrorResponse {
    ErrorResponse::from_error_nolog(&ManagerError::ProgramNotSpecified)
}

fn example_pipeline_not_specified() -> ErrorResponse {
    ErrorResponse::from_error_nolog(&ManagerError::PipelineNotSpecified)
}

fn example_connector_not_specified() -> ErrorResponse {
    ErrorResponse::from_error_nolog(&ManagerError::ConnectorNotSpecified)
}

fn example_invalid_pipeline_action() -> ErrorResponse {
    ErrorResponse::from_error_nolog(&ManagerError::InvalidPipelineAction {
        action: "my_action".to_string(),
    })
}

fn parse_uuid_param(req: &HttpRequest, param_name: &'static str) -> Result<Uuid, ManagerError> {
    match req.match_info().get(param_name) {
        None => Err(ManagerError::MissingUrlEncodedParam { param: param_name }),
        Some(id) => match id.parse::<Uuid>() {
            Err(e) => Err(ManagerError::InvalidUuidParam {
                value: id.to_string(),
                error: e.to_string(),
            }),
            Ok(uuid) => Ok(uuid),
        },
    }
}

fn parse_pipeline_action(req: &HttpRequest) -> Result<&str, ManagerError> {
    match req.match_info().get("action") {
        None => Err(ManagerError::MissingUrlEncodedParam { param: "action" }),
        Some(action) => Ok(action),
    }
}

/// Enumerate the program database.
#[utoipa::path(
    responses(
        (status = OK, description = "List of programs retrieved successfully", body = [ProgramDescr]),
    ),
    tag = "Program"
)]
#[get("/v0/programs")]
async fn list_programs(
    state: WebData<ServerState>,
    tenant_id: ReqData<TenantId>,
) -> Result<HttpResponse, DBError> {
    state
        .db
        .lock()
        .await
        .list_programs(*tenant_id)
        .await
        .map(|programs| {
            HttpResponse::Ok()
                .insert_header(CacheControl(vec![CacheDirective::NoCache]))
                .json(programs)
        })
}

/// Response to a program code request.
#[derive(Serialize, ToSchema)]
struct ProgramCodeResponse {
    /// Current program meta-data.
    program: ProgramDescr,
    /// Program code.
    code: String,
}

/// Returns the latest SQL source code of the program along with its meta-data.
#[utoipa::path(
    responses(
        (status = OK, description = "Program data and code retrieved successfully.", body = ProgramCodeResponse),
        (status = BAD_REQUEST
            , description = "Specified program id is not a valid uuid."
            , body = ErrorResponse
            , example = json!(example_invalid_uuid_param())),
        (status = NOT_FOUND
            , description = "Specified program id does not exist in the database."
            , body = ErrorResponse
            , example = json!(example_unknown_program())),
    ),
    params(
        ("program_id" = Uuid, Path, description = "Unique program identifier")
    ),
    tag = "Program"
)]
#[get("/v0/program/{program_id}/code")]
async fn program_code(
    state: WebData<ServerState>,
    tenant_id: ReqData<TenantId>,
    req: HttpRequest,
) -> Result<HttpResponse, ManagerError> {
    let program_id = ProgramId(parse_uuid_param(&req, "program_id")?);

    Ok(state
        .db
        .lock()
        .await
        .program_code(*tenant_id, program_id)
        .await
        .map(|(program, code)| {
            HttpResponse::Ok()
                .insert_header(CacheControl(vec![CacheDirective::NoCache]))
                .json(&ProgramCodeResponse { program, code })
        })?)
}

/// Returns program descriptor, including current program version and
/// compilation status.
#[utoipa::path(
    responses(
        (status = OK, description = "Program status retrieved successfully.", body = ProgramDescr),
        (status = BAD_REQUEST
            , description = "Program not specified. Use ?id or ?name query strings in the URL."
            , body = ErrorResponse
            , example = json!(example_program_not_specified())),
        (status = NOT_FOUND
            , description = "Specified program name does not exist in the database."
            , body = ErrorResponse
            , example = json!(example_unknown_name())),
        (status = NOT_FOUND
            , description = "Specified program id does not exist in the database."
            , body = ErrorResponse
            , example = json!(example_unknown_program())),
    ),
    params(
        ("id" = Option<Uuid>, Query, description = "Unique connector identifier"),
        ("name" = Option<String>, Query, description = "Unique connector name")
    ),
    tag = "Program"
)]
#[get("/v0/program")]
async fn program_status(
    state: WebData<ServerState>,
    tenant_id: ReqData<TenantId>,
    req: web::Query<IdOrNameQuery>,
) -> Result<HttpResponse, ManagerError> {
    let descr = if let Some(id) = req.id {
        state
            .db
            .lock()
            .await
            .get_program_by_id(*tenant_id, ProgramId(id))
            .await?
    } else if let Some(name) = req.name.clone() {
        state
            .db
            .lock()
            .await
            .get_program_by_name(*tenant_id, &name)
            .await?
    } else {
        return Err(ManagerError::ProgramNotSpecified);
    };

    Ok(HttpResponse::Ok()
        .insert_header(CacheControl(vec![CacheDirective::NoCache]))
        .json(&descr))
}

/// Request to create a new DBSP program.
#[derive(Debug, Deserialize, ToSchema)]
struct NewProgramRequest {
    /// Program name.
    #[schema(example = "Example program")]
    name: String,
    /// Overwrite existing program with the same name, if any.
    #[serde(default)]
    overwrite_existing: bool,
    /// Program description.
    #[schema(example = "Example description")]
    description: String,
    /// SQL code of the program.
    #[schema(example = "CREATE TABLE Example(name varchar);")]
    code: String,
}

/// Response to a new program request.
#[derive(Serialize, ToSchema)]
struct NewProgramResponse {
    /// Id of the newly created program.
    #[schema(example = 42)]
    program_id: ProgramId,
    /// Initial program version (this field is always set to 1).
    #[schema(example = 1)]
    version: Version,
}

/// Create a new program.
///
/// If the `overwrite_existing` flag is set in the request and a program with
/// the same name already exists, all pipelines associated with that program and
/// the program itself will be deleted.
#[utoipa::path(
    request_body = NewProgramRequest,
    responses(
        (status = CREATED, description = "Program created successfully", body = NewProgramResponse),
        (status = CONFLICT
            , description = "A program with this name already exists in the database."
            , body = ErrorResponse
            , example = json!(example_duplicate_name())),
    ),
    tag = "Program"
)]
#[post("/v0/programs")]
async fn new_program(
    state: WebData<ServerState>,
    tenant_id: ReqData<TenantId>,
    request: web::Json<NewProgramRequest>,
) -> Result<HttpResponse, DBError> {
    do_new_program(state, tenant_id, request).await
}

async fn do_new_program(
    state: WebData<ServerState>,
    tenant_id: ReqData<TenantId>,
    request: web::Json<NewProgramRequest>,
) -> Result<HttpResponse, DBError> {
    if request.overwrite_existing {
        let descr = {
            let db = state.db.lock().await;
            let descr = db.lookup_program(*tenant_id, &request.name).await?;
            drop(db);
            descr
        };
        if let Some(program_descr) = descr {
            do_delete_program(state.clone(), *tenant_id, program_descr.program_id).await?;
        }
    }

    state
        .db
        .lock()
        .await
        .new_program(
            *tenant_id,
            Uuid::now_v7(),
            &request.name,
            &request.description,
            &request.code,
        )
        .await
        .map(|(program_id, version)| {
            HttpResponse::Created()
                .insert_header(CacheControl(vec![CacheDirective::NoCache]))
                .json(&NewProgramResponse {
                    program_id,
                    version,
                })
        })
}

/// Update program request.
#[derive(Deserialize, ToSchema)]
struct UpdateProgramRequest {
    /// Id of the program.
    program_id: ProgramId,
    /// New name for the program.
    name: String,
    /// New description for the program.
    #[serde(default)]
    description: String,
    /// New SQL code for the program or `None` to keep existing program
    /// code unmodified.
    code: Option<String>,
}

/// Response to a program update request.
#[derive(Serialize, ToSchema)]
struct UpdateProgramResponse {
    /// New program version.  Equals the previous version if program code
    /// doesn't change or previous version +1 if it does.
    version: Version,
}

/// Change program code and/or name.
///
/// If program code changes, any ongoing compilation gets cancelled,
/// program status is reset to `None`, and program version
/// is incremented by 1.  Changing program name only doesn't affect its
/// version or the compilation process.
#[utoipa::path(
    request_body = UpdateProgramRequest,
    responses(
        (status = OK, description = "Program updated successfully.", body = UpdateProgramResponse),
        (status = NOT_FOUND
            , description = "Specified program id does not exist in the database."
            , body = ErrorResponse
            , example = json!(example_unknown_program())),
        (status = CONFLICT
            , description = "A program with this name already exists in the database."
            , body = ErrorResponse
            , example = json!(example_duplicate_name())),
    ),
    tag = "Program"
)]
#[patch("/v0/programs")]
async fn update_program(
    state: WebData<ServerState>,
    tenant_id: ReqData<TenantId>,
    request: web::Json<UpdateProgramRequest>,
) -> Result<HttpResponse, DBError> {
    let version = state
        .db
        .lock()
        .await
        .update_program(
            *tenant_id,
            request.program_id,
            &request.name,
            &request.description,
            &request.code,
        )
        .await?;

    Ok(HttpResponse::Ok()
        .insert_header(CacheControl(vec![CacheDirective::NoCache]))
        .json(&UpdateProgramResponse { version }))
}

/// Request to queue a program for compilation.
#[derive(Deserialize, ToSchema)]
struct CompileProgramRequest {
    /// Program id.
    program_id: ProgramId,
    /// Latest program version known to the client.
    version: Version,
}

/// Queue program for compilation.
///
/// The client should poll the `/program_status` endpoint
/// for compilation results.
#[utoipa::path(
    request_body = CompileProgramRequest,
    responses(
        (status = ACCEPTED, description = "Compilation request submitted."),
        (status = NOT_FOUND
            , description = "Specified program id does not exist in the database."
            , body = ErrorResponse
            , example = json!(example_unknown_program())),
        (status = CONFLICT
            , description = "Program version specified in the request doesn't match the latest program version in the database."
            , body = ErrorResponse
            , example = json!(example_outdated_program_version())),
    ),
    tag = "Program"
)]
#[post("/v0/programs/compile")]
async fn compile_program(
    state: WebData<ServerState>,
    tenant_id: ReqData<TenantId>,
    request: web::Json<CompileProgramRequest>,
) -> Result<HttpResponse, DBError> {
    state
        .db
        .lock()
        .await
        .set_program_pending(*tenant_id, request.program_id, request.version)
        .await?;

    Ok(HttpResponse::Accepted().finish())
}

/// Request to cancel ongoing program compilation.
#[derive(Deserialize, ToSchema)]
struct CancelProgramRequest {
    /// Program id.
    program_id: ProgramId,
    /// Latest program version known to the client.
    version: Version,
}

/// Cancel outstanding compilation request.
///
/// The client should poll the `/program_status` endpoint
/// to determine when the cancelation request completes.
#[utoipa::path(
    request_body = CancelProgramRequest,
    responses(
        (status = ACCEPTED, description = "Cancelation request submitted."),
        (status = NOT_FOUND
            , description = "Specified program id does not exist in the database."
            , body = ErrorResponse
            , example = json!(example_unknown_program())),
        (status = CONFLICT
            , description = "Program version specified in the request doesn't match the latest program version in the database."
            , body = ErrorResponse
            , example = json!(example_outdated_program_version())),
    ),
    tag = "Program"
)]
#[delete("/v0/programs/compile")]
async fn cancel_program(
    state: WebData<ServerState>,
    tenant_id: ReqData<TenantId>,
    request: web::Json<CancelProgramRequest>,
) -> Result<HttpResponse, DBError> {
    state
        .db
        .lock()
        .await
        .cancel_program(*tenant_id, request.program_id, request.version)
        .await?;

    Ok(HttpResponse::Accepted().finish())
}

/// Delete a program.
///
/// Deletes all pipelines and configs associated with the program.
#[utoipa::path(
    responses(
        (status = OK, description = "Program successfully deleted."),
        (status = BAD_REQUEST
            , description = "Specified program id is not a valid uuid."
            , body = ErrorResponse
            , example = json!(example_invalid_uuid_param())),
        (status = NOT_FOUND
            , description = "Specified program id does not exist in the database."
            , body = ErrorResponse
            , example = json!(example_unknown_program())),
    ),
    params(
        ("program_id" = Uuid, Path, description = "Unique program identifier")
    ),
    tag = "Program"
)]
#[delete("/v0/programs/{program_id}")]
async fn delete_program(
    state: WebData<ServerState>,
    tenant_id: ReqData<TenantId>,
    req: HttpRequest,
) -> Result<HttpResponse, ManagerError> {
    let program_id = ProgramId(parse_uuid_param(&req, "program_id")?);

    Ok(do_delete_program(state, *tenant_id, program_id).await?)
}

async fn do_delete_program(
    state: WebData<ServerState>,
    tenant_id: TenantId,
    program_id: ProgramId,
) -> Result<HttpResponse, DBError> {
    let db = state.db.lock().await;
    db.delete_program(tenant_id, program_id)
        .await
        .map(|_| HttpResponse::Ok().finish())
}

/// Request to create a new program configuration.
#[derive(Debug, Deserialize, ToSchema)]
struct NewPipelineRequest {
    /// Config name.
    name: String,
    /// Config description.
    description: String,
    /// Program to create config for.
    program_id: Option<ProgramId>,
    /// YAML code for the config.
    config: String,
    /// Attached connectors.
    connectors: Option<Vec<AttachedConnector>>,
}

/// Response to a config creation request.
#[derive(Serialize, ToSchema)]
struct NewPipelineResponse {
    /// Id of the newly created config.
    pipeline_id: PipelineId,
    /// Initial config version (this field is always set to 1).
    version: Version,
}

/// Create a new program configuration.
#[utoipa::path(
    request_body = NewPipelineRequest,
    responses(
        (status = OK, description = "Configuration successfully created.", body = NewPipelineResponse),
        (status = NOT_FOUND
            , description = "Specified program id does not exist in the database."
            , body = ErrorResponse
            , example = json!(example_unknown_program())),
    ),
    tag = "Pipeline"
)]
#[post("/v0/pipelines")]
async fn new_pipeline(
    state: WebData<ServerState>,
    tenant_id: ReqData<TenantId>,
    request: web::Json<NewPipelineRequest>,
) -> Result<HttpResponse, DBError> {
    debug!("Received new-pipeline request: {request:?}");
    let (pipeline_id, version) = state
        .db
        .lock()
        .await
        .new_pipeline(
            *tenant_id,
            Uuid::now_v7(),
            request.program_id,
            &request.name,
            &request.description,
            &request.config,
            &request.connectors,
        )
        .await?;

    Ok(HttpResponse::Ok()
        .insert_header(CacheControl(vec![CacheDirective::NoCache]))
        .json(&NewPipelineResponse {
            pipeline_id,
            version,
        }))
}

/// Request to update an existing program configuration.
#[derive(Deserialize, ToSchema)]
struct UpdatePipelineRequest {
    /// Config id.
    pipeline_id: PipelineId,
    /// New config name.
    name: String,
    /// New config description.
    description: String,
    /// New program to create config for. If absent, program will be set to
    /// NULL.
    program_id: Option<ProgramId>,
    /// New config YAML. If absent, existing YAML will be kept unmodified.
    config: Option<String>,
    /// Attached connectors.
    ///
    /// - If absent, existing connectors will be kept unmodified.
    ///
    /// - If present all existing connectors will be replaced with the new
    /// specified list.
    connectors: Option<Vec<AttachedConnector>>,
}

/// Response to a config update request.
#[derive(Serialize, ToSchema)]
struct UpdatePipelineResponse {
    /// New config version. Equals the previous version +1.
    version: Version,
}

/// Update existing program configuration.
///
/// Updates program config name, description and code and, optionally, config
/// and connectors. On success, increments config version by 1.
#[utoipa::path(
    request_body = UpdatePipelineRequest,
    responses(
        (status = OK, description = "Configuration successfully updated.", body = UpdatePipelineResponse),
        (status = NOT_FOUND
            , description = "Specified pipeline id does not exist in the database."
            , body = ErrorResponse
            , example = json!(example_unknown_pipeline())),
        (status = NOT_FOUND
            , description = "Specified connector id does not exist in the database."
            , body = ErrorResponse
            , example = json!(example_unknown_connector())),
    ),
    tag = "Pipeline"
)]
#[patch("/v0/pipelines")]
async fn update_pipeline(
    state: WebData<ServerState>,
    tenant_id: ReqData<TenantId>,
    request: web::Json<UpdatePipelineRequest>,
) -> Result<HttpResponse, DBError> {
    let version = state
        .db
        .lock()
        .await
        .update_pipeline(
            *tenant_id,
            request.pipeline_id,
            request.program_id,
            &request.name,
            &request.description,
            &request.config,
            &request.connectors,
        )
        .await?;

    Ok(HttpResponse::Ok()
        .insert_header(CacheControl(vec![CacheDirective::NoCache]))
        .json(&UpdatePipelineResponse { version }))
}

/// List pipelines.
#[utoipa::path(
    responses(
        (status = OK, description = "Pipeline list retrieved successfully.", body = [Pipeline])
    ),
    tag = "Pipeline"
)]
#[get("/v0/pipelines")]
async fn list_pipelines(
    state: WebData<ServerState>,
    tenant_id: ReqData<TenantId>,
) -> Result<HttpResponse, DBError> {
    let pipelines = state.db.lock().await.list_pipelines(*tenant_id).await?;

    Ok(HttpResponse::Ok()
        .insert_header(CacheControl(vec![CacheDirective::NoCache]))
        .json(pipelines))
}

/// Return the last committed (and running, if pipeline is started)
/// configuration of the pipeline.
#[utoipa::path(
    responses(
        (status = OK, description = "Last committed configuration of the pipeline retrieved successfully (returns null if pipeline was never deployed yet).", body = Option<PipelineRevision>),
        (status = NOT_FOUND
            , description = "Specified `pipeline_id` does not exist in the database."
            , body = ErrorResponse
            , example = json!(example_unknown_pipeline())),
    ),
    params(
        ("pipeline_id" = Uuid, Path, description = "Unique pipeline identifier")
    ),
    tag = "Pipeline"
)]
#[get("/v0/pipelines/{pipeline_id}/committed")]
async fn pipeline_committed(
    state: WebData<ServerState>,
    tenant_id: ReqData<TenantId>,
    req: HttpRequest,
) -> Result<HttpResponse, ManagerError> {
    let pipeline_id = PipelineId(parse_uuid_param(&req, "pipeline_id")?);

    let descr: Option<db::PipelineRevision> = match state
        .db
        .lock()
        .await
        .get_last_committed_pipeline_revision(*tenant_id, pipeline_id)
        .await
    {
        Ok(revision) => Some(revision),
        Err(e) => {
            if matches!(e, DBError::NoRevisionAvailable { .. }) {
                None
            } else {
                Err(e)?
            }
        }
    };

    Ok(HttpResponse::Ok()
        .insert_header(CacheControl(vec![CacheDirective::NoCache]))
        .json(&descr))
}

/// Retrieve pipeline metrics and performance counters.
#[utoipa::path(
    responses(
        // TODO: Implement `ToSchema` for `ControllerStatus`, which is the
        // actual type returned by this endpoint.
        (status = OK, description = "Pipeline metrics retrieved successfully.", body = Object),
        (status = BAD_REQUEST
            , description = "Specified pipeline id is not a valid uuid."
            , body = ErrorResponse
            , example = json!(example_invalid_uuid_param())),
        (status = NOT_FOUND
            , description = "Specified pipeline id does not exist in the database."
            , body = ErrorResponse
            , example = json!(example_unknown_pipeline())),
    ),
    params(
        ("pipeline_id" = Uuid, Path, description = "Unique pipeline identifier")
    ),
    tag = "Pipeline"
)]
#[get("/v0/pipelines/{pipeline_id}/stats")]
async fn pipeline_stats(
    state: WebData<ServerState>,
    tenant_id: ReqData<TenantId>,
    req: HttpRequest,
) -> Result<HttpResponse, ManagerError> {
    let pipeline_id = PipelineId(parse_uuid_param(&req, "pipeline_id")?);

    state
        .runner
        .forward_to_pipeline(*tenant_id, pipeline_id, Method::GET, "stats")
        .await
}

/// Retrieve pipeline metadata.
#[utoipa::path(
    responses(
        (status = OK, description = "Pipeline descriptor retrieved successfully.",content(
            ("text/plain" = String, example = json!(example_pipeline_toml())),
            ("application/json" = Pipeline),
        )),
        (status = BAD_REQUEST
            , description = "Pipeline not specified. Use ?id or ?name query strings in the URL."
            , body = ErrorResponse
            , example = json!(example_pipeline_not_specified())),
        (status = NOT_FOUND
            , description = "Specified pipeline name does not exist in the database."
            , body = ErrorResponse
            , example = json!(example_unknown_name())),
        (status = NOT_FOUND
            , description = "Specified pipeline id does not exist in the database."
            , body = ErrorResponse
            , example = json!(example_unknown_pipeline())),
        (status = NOT_FOUND
            , description = "Specified pipeline name does not exist in the database."
            , body = ErrorResponse
            , example = json!(example_unknown_name())),
    ),
    params(
        ("id" = Option<Uuid>, Query, description = "Unique pipeline identifier"),
        ("name" = Option<String>, Query, description = "Unique pipeline name"),
        ("toml" = Option<bool>, Query, description = "Set to true to request the configuration of the pipeline as a toml file."),
    ),
    tag = "Pipeline"
)]
#[get("/v0/pipeline")]
async fn pipeline_status(
    state: WebData<ServerState>,
    tenant_id: ReqData<TenantId>,
    req: web::Query<IdOrNameTomlQuery>,
) -> Result<HttpResponse, ManagerError> {
    if req.toml.unwrap_or(false) {
        let toml: String = if let Some(id) = req.id {
            state
                .db
                .lock()
                .await
                .pipeline_to_toml(*tenant_id, PipelineId(id))
                .await?
        } else if let Some(name) = req.name.clone() {
            let db: tokio::sync::MutexGuard<'_, ProjectDB> = state.db.lock().await;
            let pipeline = db.get_pipeline_descr_by_name(*tenant_id, name).await?;
            db.pipeline_to_toml(*tenant_id, pipeline.pipeline_id)
                .await?
        } else {
            Err(ManagerError::PipelineNotSpecified)?
        };

        Ok(HttpResponse::Ok()
            .content_type("text/plain")
            .insert_header(CacheControl(vec![CacheDirective::NoCache]))
            .body(toml))
    } else {
        let pipeline: db::Pipeline = if let Some(id) = req.id {
            state
                .db
                .lock()
                .await
                .get_pipeline_by_id(*tenant_id, PipelineId(id))
                .await?
        } else if let Some(name) = req.name.clone() {
            state
                .db
                .lock()
                .await
                .get_pipeline_by_name(*tenant_id, name)
                .await?
        } else {
            Err(ManagerError::PipelineNotSpecified)?
        };

        Ok(HttpResponse::Ok()
            .insert_header(CacheControl(vec![CacheDirective::NoCache]))
            .json(&pipeline))
    }
}

/// Validate the configuration of a  a pipeline.
///
/// Validate configuration, usable as a pre-cursor for deploy to
/// check if pipeline configuration is valid and can be deployed.
#[utoipa::path(
    responses(
        (status = OK
            , description = "Validate a Pipeline config."
            , content_type = "application/json"
            , body = String),
        (status = BAD_REQUEST
            , description = "Specified pipeline id is not a valid uuid."
            , body = ErrorResponse
            , example = json!(example_invalid_uuid_param())),
        (status = NOT_FOUND
            , description = "Specified pipeline id does not exist in the database."
            , body = ErrorResponse
            , example = json!(example_unknown_pipeline())),
        (status = BAD_REQUEST
            , description = "Pipeline does not have a program set."
            , body = ErrorResponse
            , example = json!(example_program_not_set())),
        (status = SERVICE_UNAVAILABLE
            , description = "Unable to start the pipeline before its program has been compiled."
            , body = ErrorResponse
            , example = json!(example_program_not_compiled())),
        (status = BAD_REQUEST
            , description = "Unable to start the pipeline before its program has been compiled successfully."
            , body = ErrorResponse
            , example = json!(example_program_has_errors())),
        (status = BAD_REQUEST
            , description = "The connectors in the config referenced a table that doesn't exist."
            , body = ErrorResponse
            , example = json!(example_pipline_invalid_input_ac())),
        (status = BAD_REQUEST
            , description = "The connectors in the config referenced a view that doesn't exist."
            , body = ErrorResponse
            , example = json!(example_pipline_invalid_output_ac())),
    ),
    params(
        ("pipeline_id" = Uuid, Path, description = "Unique pipeline identifier"),
    ),
    tag = "Pipeline"
)]
#[get("/v0/pipelines/{pipeline_id}/validate")]
async fn pipeline_validate(
    state: WebData<ServerState>,
    tenant_id: ReqData<TenantId>,
    req: HttpRequest,
) -> Result<HttpResponse, ManagerError> {
    let pipeline_id = PipelineId(parse_uuid_param(&req, "pipeline_id")?);

    let db = state.db.lock().await;
    Ok(db
        .pipeline_is_committable(*tenant_id, pipeline_id)
        .await
        .map(|_| HttpResponse::Ok().json("Pipeline successfully validated."))?)
}

/// Perform action on a pipeline.
///
/// - 'deploy': Deploy a pipeline for the specified program and configuration.
/// This is a synchronous endpoint, which sends a response once the pipeline has
/// been initialized.
/// - 'start': Start a pipeline.
/// - 'pause': Pause the pipeline.
/// - 'shutdown': Terminate the execution of a pipeline. Sends a termination
/// request to the pipeline process. Returns immediately, without waiting for
/// the pipeline to terminate (which can take several seconds). The pipeline is
/// not deleted from the database, but its `status` is set to `shutdown`.
#[utoipa::path(
    responses(
        (status = ACCEPTED
            , description = "Performed a Pipeline action."),
        (status = BAD_REQUEST
            , description = "Invalid pipeline action specified."
            , body = ErrorResponse
            , example = json!(example_invalid_pipeline_action())),
        (status = BAD_REQUEST
            , description = "Specified pipeline id is not a valid uuid."
            , body = ErrorResponse
            , example = json!(example_invalid_uuid_param())),
        (status = NOT_FOUND
            , description = "Specified pipeline id does not exist in the database."
            , body = ErrorResponse
            , example = json!(example_unknown_pipeline())),
        (status = BAD_REQUEST
            , description = "Pipeline does not have a program set."
            , body = ErrorResponse
            , example = json!(example_program_not_set())),
        (status = SERVICE_UNAVAILABLE
            , description = "Unable to start the pipeline before its program has been compiled."
            , body = ErrorResponse
            , example = json!(example_program_not_compiled())),
        (status = BAD_REQUEST
            , description = "Unable to start the pipeline before its program has been compiled successfully."
            , body = ErrorResponse
            , example = json!(example_program_has_errors())),
        (status = BAD_REQUEST
            , description = "The connectors in the config referenced a table that doesn't exist."
            , body = ErrorResponse
            , example = json!(example_pipline_invalid_input_ac())),
        (status = BAD_REQUEST
            , description = "The connectors in the config referenced a view that doesn't exist."
            , body = ErrorResponse
            , example = json!(example_pipline_invalid_output_ac())),
        (status = INTERNAL_SERVER_ERROR
            , description = "Timeout waiting for the pipeline to initialize. Indicates an internal system error."
            , body = ErrorResponse
            , example = json!(example_pipeline_timeout())),
    ),
    params(
        ("pipeline_id" = Uuid, Path, description = "Unique pipeline identifier"),
        ("action" = String, Path, description = "Pipeline action [deploy, start, pause, shutdown]")
    ),
    tag = "Pipeline"
)]
#[post("/v0/pipelines/{pipeline_id}/{action}")]
async fn pipeline_action(
    state: WebData<ServerState>,
    tenant_id: ReqData<TenantId>,
    req: HttpRequest,
) -> Result<HttpResponse, ManagerError> {
    let pipeline_id = PipelineId(parse_uuid_param(&req, "pipeline_id")?);
    let action = parse_pipeline_action(&req)?;

    match action {
        "deploy" => state.runner.deploy_pipeline(*tenant_id, pipeline_id).await?,
        "start" => state.runner.start_pipeline(*tenant_id, pipeline_id).await?,
        "pause" => state.runner.pause_pipeline(*tenant_id, pipeline_id).await?,
        "shutdown" => {
            state
                .runner
                .shutdown_pipeline(*tenant_id, pipeline_id)
                .await?
        }
        _ => Err(ManagerError::InvalidPipelineAction {
            action: action.to_string(),
        })?,
    }

    Ok(HttpResponse::Accepted().finish())
}

/// Terminate and delete a pipeline.
///
/// Shut down the pipeline if it is still running and delete it from
/// the database.
#[utoipa::path(
    responses(
        (status = ACCEPTED
            , description = "Pipeline successfully deleted."),
        (status = BAD_REQUEST
            , description = "Specified pipeline id is not a valid uuid."
            , body = ErrorResponse
            , example = json!(example_invalid_uuid_param())),
        (status = NOT_FOUND
            , description = "Specified pipeline id does not exist in the database."
            , body = ErrorResponse
            , example = json!(example_unknown_pipeline())),
    ),
    params(
        ("pipeline_id" = Uuid, Path, description = "Unique pipeline identifier")
    ),
    tag = "Pipeline"
)]
#[delete("/v0/pipelines/{pipeline_id}")]
async fn pipeline_delete(
    state: WebData<ServerState>,
    tenant_id: ReqData<TenantId>,
    req: HttpRequest,
) -> Result<HttpResponse, ManagerError> {
    let pipeline_id = PipelineId(parse_uuid_param(&req, "pipeline_id")?);

    state
        .runner
        .delete_pipeline(*tenant_id, pipeline_id)
        .await?;

    Ok(HttpResponse::Accepted().finish())
}

/// Enumerate the connector database.
#[utoipa::path(
    responses(
        (status = OK, description = "List of connectors retrieved successfully", body = [ConnectorDescr]),
    ),
    tag = "Connector"
)]
#[get("/v0/connectors")]
async fn list_connectors(
    state: WebData<ServerState>,
    tenant_id: ReqData<TenantId>,
) -> Result<HttpResponse, DBError> {
    let connectors = state.db.lock().await.list_connectors(*tenant_id).await?;

    Ok(HttpResponse::Ok()
        .insert_header(CacheControl(vec![CacheDirective::NoCache]))
        .json(connectors))
}

/// Request to create a new connector.
#[derive(Deserialize, ToSchema)]
pub(self) struct NewConnectorRequest {
    /// connector name.
    name: String,
    /// connector description.
    description: String,
    /// connector config.
    config: String,
}

/// Response to a connector creation request.
#[derive(Serialize, ToSchema)]
struct NewConnectorResponse {
    /// Unique id assigned to the new connector.
    connector_id: ConnectorId,
}

/// Create a new connector configuration.
#[utoipa::path(
    request_body = NewConnectorRequest,
    responses(
        (status = OK, description = "connector successfully created.", body = NewConnectorResponse),
    ),
    tag = "Connector"
)]
#[post("/v0/connectors")]
async fn new_connector(
    state: WebData<ServerState>,
    tenant_id: ReqData<TenantId>,
    request: web::Json<NewConnectorRequest>,
) -> Result<HttpResponse, DBError> {
    let connector_id = state
        .db
        .lock()
        .await
        .new_connector(
            *tenant_id,
            Uuid::now_v7(),
            &request.name,
            &request.description,
            &request.config,
        )
        .await?;

    Ok(HttpResponse::Ok()
        .insert_header(CacheControl(vec![CacheDirective::NoCache]))
        .json(&NewConnectorResponse { connector_id }))
}

/// Request to update an existing data-connector.
#[derive(Deserialize, ToSchema)]
struct UpdateConnectorRequest {
    /// connector id.
    connector_id: ConnectorId,
    /// New connector name.
    name: String,
    /// New connector description.
    description: String,
    /// New config YAML. If absent, existing YAML will be kept unmodified.
    config: Option<String>,
}

/// Response to a config update request.
#[derive(Serialize, ToSchema)]
struct UpdateConnectorResponse {}

/// Update existing connector.
///
/// Updates config name and, optionally, code.
/// On success, increments config version by 1.
#[utoipa::path(
    request_body = UpdateConnectorRequest,
    responses(
        (status = OK, description = "connector successfully updated.", body = UpdateConnectorResponse),
        (status = NOT_FOUND
            , description = "Specified connector id does not exist in the database."
            , body = ErrorResponse
            , example = json!(example_unknown_connector())),
    ),
    tag = "Connector"
)]
#[patch("/v0/connectors")]
async fn update_connector(
    state: WebData<ServerState>,
    tenant_id: ReqData<TenantId>,
    request: web::Json<UpdateConnectorRequest>,
) -> Result<HttpResponse, DBError> {
    state
        .db
        .lock()
        .await
        .update_connector(
            *tenant_id,
            request.connector_id,
            &request.name,
            &request.description,
            &request.config,
        )
        .await?;

    Ok(HttpResponse::Ok()
        .insert_header(CacheControl(vec![CacheDirective::NoCache]))
        .json(&UpdateConnectorResponse {}))
}

/// Delete existing connector.
#[utoipa::path(
    responses(
        (status = OK, description = "connector successfully deleted."),
        (status = BAD_REQUEST
            , description = "Specified connector id is not a valid uuid."
            , body = ErrorResponse
            , example = json!(example_invalid_uuid_param())),
        (status = NOT_FOUND
            , description = "Specified connector id does not exist in the database."
            , body = ErrorResponse
            , example = json!(example_unknown_connector())),
    ),
    params(
        ("connector_id" = Uuid, Path, description = "Unique connector identifier")
    ),
    tag = "Connector"
)]
#[delete("/v0/connectors/{connector_id}")]
async fn delete_connector(
    state: WebData<ServerState>,
    tenant_id: ReqData<TenantId>,
    req: HttpRequest,
) -> Result<HttpResponse, ManagerError> {
    let connector_id = ConnectorId(parse_uuid_param(&req, "connector_id")?);

    state
        .db
        .lock()
        .await
        .delete_connector(*tenant_id, connector_id)
        .await?;

    Ok(HttpResponse::Ok().finish())
}

#[derive(Debug, Deserialize)]
pub struct IdOrNameQuery {
    id: Option<Uuid>,
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct IdOrNameTomlQuery {
    id: Option<Uuid>,
    name: Option<String>,
    toml: Option<bool>,
}

/// Returns connector descriptor.
#[utoipa::path(
    responses(
        (status = OK, description = "connector status retrieved successfully.", body = ConnectorDescr),
        (status = BAD_REQUEST
            , description = "Connector not specified. Use ?id or ?name query strings in the URL."
            , body = ErrorResponse
            , example = json!(example_connector_not_specified())),
        (status = NOT_FOUND
            , description = "Specified connector name does not exist in the database."
            , body = ErrorResponse
            , example = json!(example_unknown_name())),
        (status = NOT_FOUND
            , description = "Specified connector id does not exist in the database."
            , body = ErrorResponse
            , example = json!(example_unknown_connector())),
        (status = NOT_FOUND
            , description = "Specified connector name does not exist in the database."
            , body = ErrorResponse
            , example = json!(example_unknown_name())),
    ),
    params(
        ("id" = Option<Uuid>, Query, description = "Unique connector identifier"),
        ("name" = Option<String>, Query, description = "Unique connector name")
    ),
    tag = "Connector"
)]
#[get("/v0/connector")]
async fn connector_status(
    state: WebData<ServerState>,
    tenant_id: ReqData<TenantId>,
    req: web::Query<IdOrNameQuery>,
) -> Result<HttpResponse, ManagerError> {
    let descr: db::ConnectorDescr = if let Some(id) = req.id {
        state
            .db
            .lock()
            .await
            .get_connector_by_id(*tenant_id, ConnectorId(id))
            .await?
    } else if let Some(name) = req.name.clone() {
        state
            .db
            .lock()
            .await
            .get_connector_by_name(*tenant_id, name)
            .await?
    } else {
        Err(ManagerError::ConnectorNotSpecified)?
    };

    Ok(HttpResponse::Ok()
        .insert_header(CacheControl(vec![CacheDirective::NoCache]))
        .json(&descr))
}

/// Push data to a SQL table.
///
/// The client sends data encoded using the format specified in the `?format=`
/// parameter as a body of the request.  The contents of the data must match
/// the SQL table schema specified in `table_name`
///
/// The pipeline ingests data as it arrives without waiting for the end of
/// the request.  Successful HTTP response indicates that all data has been
/// ingested successfully.
// TODO: implement chunked and batch modes.
#[utoipa::path(
    responses(
        (status = OK
            , description = "Data successfully delivered to the pipeline."
            , content_type = "application/json"),
        (status = BAD_REQUEST
            , description = "Specified pipeline id is not a valid uuid."
            , body = ErrorResponse
            , example = json!(example_invalid_uuid_param())),
        (status = NOT_FOUND
            , description = "Specified pipeline id does not exist in the database."
            , body = ErrorResponse
            , example = json!(example_unknown_pipeline())),
        (status = NOT_FOUND
            , description = "Specified table does not exist."
            , body = ErrorResponse
            , example = json!(example_unknown_input_table("MyTable"))),
        (status = NOT_FOUND
            , description = "Pipeline is not currently running because it has been shutdown or not yet started."
            , body = ErrorResponse
            , example = json!(example_pipeline_shutdown())),
        (status = BAD_REQUEST
            , description = "Unknown data format specified in the '?format=' argument."
            , body = ErrorResponse
            , example = json!(example_unknown_input_format())),
        (status = UNPROCESSABLE_ENTITY
            , description = "Error parsing input data."
            , body = ErrorResponse
            , example = json!(example_parse_error())),
        (status = INTERNAL_SERVER_ERROR
            , description = "Request failed."
            , body = ErrorResponse),
    ),
    params(
        ("pipeline_id" = Uuid, Path, description = "Unique pipeline identifier."),
        ("table_name" = String, Path, description = "SQL table name."),
        ("format" = String, Query, description = "Input data format, e.g., 'csv' or 'json'."),
    ),
    tag = "Pipeline"
)]
#[post("/v0/pipelines/{pipeline_id}/ingress/{table_name}")]
async fn http_input(
    state: WebData<ServerState>,
    tenant_id: ReqData<TenantId>,
    req: HttpRequest,
    body: web::Payload,
) -> Result<HttpResponse, ManagerError> {
    debug!("Received {req:?}");

    let pipeline_id = PipelineId(parse_uuid_param(&req, "pipeline_id")?);
    debug!("Pipeline_id {:?}", pipeline_id);

    let table_name = match req.match_info().get("table_name") {
        None => {
            return Err(ManagerError::MissingUrlEncodedParam {
                param: "table_name",
            });
        }
        Some(table_name) => table_name,
    };
    debug!("Table name {table_name:?}");

    let endpoint = format!("ingress/{table_name}");

    state
        .runner
        .forward_to_pipeline_as_stream(*tenant_id, pipeline_id, &endpoint, req, body)
        .await
}

/// Subscribe to a stream of updates to a SQL view or table.
///
/// The pipeline responds with a continuous stream of changes to the specified
/// table or view, encoded using the format specified in the `?format=`
/// parameter. Updates are split into `Chunk`'s.
///
/// The pipeline continuous sending updates until the client closes the
/// connection or the pipeline is shut down.
#[utoipa::path(
    responses(
        (status = OK
            , description = "Connection to the endpoint successfully established. The body of the response contains a stream of data chunks."
            , content_type = "application/json"
            , body = Chunk),
        (status = BAD_REQUEST
            , description = "Specified pipeline id is not a valid uuid."
            , body = ErrorResponse
            , example = json!(example_invalid_uuid_param())),
        (status = NOT_FOUND
            , description = "Specified pipeline id does not exist in the database."
            , body = ErrorResponse
            , example = json!(example_unknown_pipeline())),
        (status = NOT_FOUND
            , description = "Specified table or view does not exist."
            , body = ErrorResponse
            , example = json!(example_unknown_output_table("MyTable"))),
        (status = GONE
            , description = "Pipeline is not currently running because it has been shutdown or not yet started."
            , body = ErrorResponse
            , example = json!(example_pipeline_shutdown())),
        (status = BAD_REQUEST
            , description = "Unknown data format specified in the '?format=' argument."
            , body = ErrorResponse
            , example = json!(example_unknown_output_format())),
        (status = INTERNAL_SERVER_ERROR
            , description = "Request failed."
            , body = ErrorResponse),
    ),
    params(
        ("pipeline_id" = Uuid, Path, description = "Unique pipeline identifier."),
        ("table_name" = String, Path, description = "SQL table or view name."),
        ("format" = String, Query, description = "Output data format, e.g., 'csv' or 'json'."),
        ("query" = Option<OutputQuery>, Query, description = "Query to execute on the table. Must be one of 'table', 'neighborhood', or 'quantiles'. The default value is 'table'"),
        ("mode" = Option<EgressMode>, Query, description = "Output mode. Must be one of 'watch' or 'snapshot'. The default value is 'watch'"),
    ),
    request_body(
        content = Option<NeighborhoodQuery>,
        description = "When the `query` parameter is set to 'neighborhood', the body of the request must contain a neighborhood specification.",
        content_type = "application/json",
    ),
    tag = "Pipeline"
)]
#[get("/v0/pipelines/{pipeline_id}/egress/{table_name}")]
async fn http_output(
    state: WebData<ServerState>,
    tenant_id: ReqData<TenantId>,
    req: HttpRequest,
    body: web::Payload,
) -> Result<HttpResponse, ManagerError> {
    debug!("Received {req:?}");

    let pipeline_id = PipelineId(parse_uuid_param(&req, "pipeline_id")?);
    debug!("Pipeline_id {:?}", pipeline_id);

    let table_name = match req.match_info().get("table_name") {
        None => {
            return Err(ManagerError::MissingUrlEncodedParam {
                param: "table_name",
            });
        }
        Some(table_name) => table_name,
    };
    debug!("Table name {table_name:?}");

    let endpoint = format!("egress/{table_name}");

    state
        .runner
        .forward_to_pipeline_as_stream(*tenant_id, pipeline_id, &endpoint, req, body)
        .await
}
