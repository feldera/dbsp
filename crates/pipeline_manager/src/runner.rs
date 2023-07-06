use crate::{
    auth::TenantId,
    db::{storage::Storage, DBError, PipelineRevision, PipelineStatus},
    ErrorResponse, ManagerConfig, ManagerError, PipelineId, ProjectDB, ResponseError,
};
use actix_web::{
    body::BoxBody,
    http::{Method, StatusCode},
    web::Payload,
    HttpRequest, HttpResponse, HttpResponseBuilder,
};
use awc::Client;
use dbsp_adapters::DetailedError;
use either::Either;
use serde::Serialize;
use std::{
    borrow::Cow, error::Error as StdError, fmt, fmt::Display, path::Path, process::Stdio, sync::Arc,
};
use tokio::{
    fs,
    fs::{create_dir_all, remove_dir_all},
    process::{Child, Command},
    sync::Mutex,
    time::{sleep, Duration, Instant},
};
use uuid::Uuid;

/// Timeout waiting for pipeline port file.
pub(crate) const PORT_FILE_TIMEOUT: Duration = Duration::from_millis(10_000);

const PORT_FILE_LOG_QUIET_PERIOD: Duration = Duration::from_millis(2_000);

/// Pipeline port file polling period.
const PORT_FILE_POLL_PERIOD: Duration = Duration::from_millis(100);

/// Timeout waiting for the pipeline to initialize.
pub(crate) const STARTUP_TIMEOUT: Duration = Duration::from_millis(20_000);
const STARTUP_QUIET_PERIOD: Duration = Duration::from_millis(2_000);

/// Pipeline initialization polling period.
const STARTUP_POLL_PERIOD: Duration = Duration::from_millis(500);

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum RunnerError {
    PipelineShutdown {
        pipeline_id: PipelineId,
    },
    HttpForwardError {
        pipeline_id: PipelineId,
        error: String,
    },
    PortFileParseError {
        pipeline_id: PipelineId,
        error: String,
    },
    PipelineInitializationTimeout {
        pipeline_id: PipelineId,
        timeout: Duration,
    },
    PipelineStartupError {
        pipeline_id: PipelineId,
        // TODO: This should be IOError, so we can serialize the error code
        // similar to `DBSPError::IO`.
        error: String,
    },
}

impl DetailedError for RunnerError {
    fn error_code(&self) -> Cow<'static, str> {
        match self {
            Self::PipelineShutdown { .. } => Cow::from("PipelineShutdown"),
            Self::HttpForwardError { .. } => Cow::from("HttpForwardError"),
            Self::PortFileParseError { .. } => Cow::from("PortFileParseError"),
            Self::PipelineInitializationTimeout { .. } => {
                Cow::from("PipelineInitializationTimeout")
            }
            Self::PipelineStartupError { .. } => Cow::from("PipelineStartupError"),
        }
    }
}

impl Display for RunnerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PipelineShutdown { pipeline_id } => {
                write!(f, "Pipeline '{pipeline_id}' is not currently running.")
            }
            Self::HttpForwardError { pipeline_id, error } => {
                write!(
                    f,
                    "Error forwarding HTTP request to pipeline '{pipeline_id}': '{error}'"
                )
            }
            Self::PipelineInitializationTimeout {
                pipeline_id,
                timeout,
            } => {
                write!(f, "Waiting for pipeline '{pipeline_id}' initialization status timed out after {timeout:?}")
            }
            Self::PortFileParseError { pipeline_id, error } => {
                write!(
                    f,
                    "Could not parse port for pipeline '{pipeline_id}' from port file: '{error}'"
                )
            }
            Self::PipelineStartupError { pipeline_id, error } => {
                write!(f, "Failed to start pipeline '{pipeline_id}': '{error}'")
            }
        }
    }
}

impl StdError for RunnerError {}

impl ResponseError for RunnerError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::PipelineShutdown { .. } => StatusCode::NOT_FOUND,
            Self::HttpForwardError { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            Self::PortFileParseError { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            Self::PipelineInitializationTimeout { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            Self::PipelineStartupError { .. } => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_response(&self) -> HttpResponse<BoxBody> {
        HttpResponseBuilder::new(self.status_code()).json(ErrorResponse::from_error(self))
    }
}

/// A runner component responsible for running and interacting with
/// pipelines at runtime.
pub(crate) enum Runner {
    Local(LocalRunner),
}

/// A runner that executes pipelines locally
///
/// # Starting a pipeline
///
/// Starting a pipeline amounts to running the compiled executable with
/// selected config, and monitoring the pipeline log file for either
/// "Started HTTP server on port XXXXX" or "Failed to create server
/// [detailed error message]".  In the former case, the port number is
/// recorded in the database.  In the latter case, the error message is
/// returned to the client.
///
/// # Shutting down a pipeline
///
/// To shutdown the pipeline, the runner sends a `/shutdown` HTTP request to the
/// pipeline.  This request is asynchronous: the pipeline may continue running
/// for a few seconds after the request succeeds.
pub struct LocalRunner {
    db: Arc<Mutex<ProjectDB>>,
    config: ManagerConfig,
}

impl Runner {
    /// Start a new pipeline.
    ///
    /// Starts the pipeline executable and waits for the pipeline to initialize,
    /// returning pipeline id and port number.
    pub(crate) async fn deploy_pipeline(
        &self,
        tenant_id: TenantId,
        pipeline_id: PipelineId,
    ) -> Result<HttpResponse, ManagerError> {
        match self {
            Self::Local(local) => local.deploy_pipeline(tenant_id, pipeline_id).await,
        }
    }

    /// Send a `/shutdown` request to the pipeline process, but keep the
    /// pipeline state in the database and file system.
    ///
    /// After calling this method, the user can still do post-mortem analysis
    /// of the pipeline, e.g., access its logs.
    ///
    /// Use the [`delete_pipeline`](`Self::delete_pipeline`) method to remove
    /// all traces of the pipeline from the manager.
    pub(crate) async fn shutdown_pipeline(
        &self,
        tenant_id: TenantId,
        pipeline_id: PipelineId,
    ) -> Result<HttpResponse, ManagerError> {
        match self {
            Self::Local(local) => local.shutdown_pipeline(tenant_id, pipeline_id).await,
        }
    }

    /// Delete the pipeline from the database. Shuts down the pipeline first if
    /// it is already running.
    /// Takes a reference to an already locked DB instance, since this function
    /// is invoked in contexts where the client already holds the lock.
    pub(crate) async fn delete_pipeline(
        &self,
        tenant_id: TenantId,
        db: &ProjectDB,
        pipeline_id: PipelineId,
    ) -> Result<HttpResponse, ManagerError> {
        match self {
            Self::Local(local) => local.delete_pipeline(tenant_id, db, pipeline_id).await,
        }
    }

    pub(crate) async fn pause_pipeline(
        &self,
        tenant_id: TenantId,
        pipeline_id: PipelineId,
    ) -> Result<HttpResponse, ManagerError> {
        /*match self {
            Self::Local(local) => local.pause_pipeline(tenant_id, pipeline_id).await?,
        };*/
        let result = self.forward_to_pipeline(tenant_id, pipeline_id, Method::GET, "pause").await;
        self.update_pipeline_status();
        result
    }

    pub(crate) async fn start_pipeline(
        &self,
        tenant_id: TenantId,
        pipeline_id: PipelineId,
    ) -> Result<HttpResponse, ManagerError> {
        /*match self {
            Self::Local(local) => local.start_pipeline(tenant_id, pipeline_id).await?,
        };*/
        let result = self.forward_to_pipeline(tenant_id, pipeline_id, Method::GET, "start").await;
        self.update_pipeline_status();
        result
    }

    pub(crate) async fn forward_to_pipeline(
        &self,
        tenant_id: TenantId,
        pipeline_id: PipelineId,
        method: Method,
        endpoint: &str,
    ) -> Result<HttpResponse, ManagerError> {
        match self {
            Self::Local(local) => {
                local
                    .forward_to_pipeline(tenant_id, pipeline_id, method, endpoint)
                    .await
            }
        }
    }

    pub(crate) async fn forward_to_pipeline_as_stream(
        &self,
        tenant_id: TenantId,
        pipeline_id: PipelineId,
        endpoint: &str,
        req: HttpRequest,
        body: Payload,
    ) -> Result<HttpResponse, ManagerError> {
        match self {
            Self::Local(r) => {
                r.forward_to_pipeline_as_stream(tenant_id, pipeline_id, endpoint, req, body)
                    .await
            }
        }
    }
}

impl LocalRunner {
    pub(crate) fn new(
        db: Arc<Mutex<ProjectDB>>,
        config: &ManagerConfig,
    ) -> Result<Self, ManagerError> {
        Ok(Self {
            db,
            config: config.clone(),
        })
    }

    /// Retrieves the last revision for a pipeline.
    ///
    /// Tries to create a new revision if this pipeline never had a revision
    /// created before.
    async fn commit_and_fetch_revision(
        &self,
        tenant_id: TenantId,
        pipeline_id: PipelineId,
    ) -> Result<PipelineRevision, ManagerError> {
        let db = self.db.lock().await;

        // Make sure we create a revision by updating to latest config state
        match db
            .create_pipeline_revision(Uuid::now_v7(), tenant_id, pipeline_id)
            .await
        {
            Ok(_revision) => (),
            Err(DBError::RevisionNotChanged) => (),
            Err(e) => return Err(e.into()),
        };

        // This should normally succeed (because we just created a revision)
        Ok(db
            .get_last_committed_pipeline_revision(tenant_id, pipeline_id)
            .await?)
    }

    pub(crate) async fn deploy_pipeline(
        &self,
        tenant_id: TenantId,
        pipeline_id: PipelineId,
    ) -> Result<HttpResponse, ManagerError> {
        self.compare_and_set_pipeline_status(PipelineStatus::Shutdown, PipelineStatus::Deploying)?;

        let pipeline_revision = self
            .commit_and_fetch_revision(tenant_id, pipeline_id)
            .await
            .map_err(|e| {
                let _ = self.set_pipeline_status(PipelineStatus::Shutdown);
                e
            })?;
 
        let mut pipeline_process = self.start(pipeline_revision).await.map_err(|e| {
            let _ = self.set_pipeline_status(PipelineStatus::Shutdown);
            e
        })?;

        poll_pipeline_status()?;

        Ok(())
    }

    pub fn update_pipeline_status() {
        let (status, port) = self.get_pipeline_status();
        match status {
            PipelineStatus::Shutdown => {
                // do nothing.
            }
            PipelineStatus::Deploying if port == 0 => {
                // check port file
                // set port file
                // call self recursively.
            }
            PipelineStatus::ShuttingDown => {
                // poll_pipeline_status, change unreachable to Shutdown.
            }
            PipelineStatus::Deploying | PipelineStatus::Running | PipelineStatus::Paused | PipelineStatus::Failed | PipelineStatus::Unreachable => {
                // poll_pipeline_status
            }
        }

        match Self::wait_for_startup(pipeline_id, &self.config.port_file_path(pipeline_id)).await {
            Ok(port) => {
                // Store pipeline in the database.
                if let Err(e) = self
                    .db
                    .lock()
                    .await
                    .set_pipeline_port_number(tenant_id, pipeline_id, port)
                    .await
                {
                    let _ = pipeline_process.kill().await;
                    Err(e)?
                };
                Ok(HttpResponse::Ok().json("Pipeline successfully deployed."))
            }
            Err(e) => {
                let _ = pipeline_process.kill().await;
                self.db
                    .lock()
                    .await
                    .set_pipeline_status(tenant_id, pipeline_id, PipelineStatus::Shutdown)
                    .await?;
                match e {
                    Either::Left(manager_error) => Err(manager_error),
                    Either::Right(http_response) => Ok(http_response),
                }
            }
        }
    }

    pub(crate) async fn shutdown_pipeline(
        &self,
        tenant_id: TenantId,
        pipeline_id: PipelineId,
    ) -> Result<HttpResponse, ManagerError> {
        let db = self.db.lock().await;
        self.do_shutdown_pipeline(tenant_id, &db, pipeline_id).await
    }

    pub(crate) async fn pause_pipeline(
        &self,
        tenant_id: TenantId,
        pipeline_id: PipelineId,
    ) -> Result<bool, ManagerError> {
        let db = self.db.lock().await;
        Ok(db
            .set_pipeline_status(tenant_id, pipeline_id, PipelineStatus::Paused)
            .await?)
    }

    pub(crate) async fn start_pipeline(
        &self,
        tenant_id: TenantId,
        pipeline_id: PipelineId,
    ) -> Result<bool, ManagerError> {
        let db = self.db.lock().await;
        Ok(db
            .set_pipeline_status(tenant_id, pipeline_id, PipelineStatus::Running)
            .await?)
    }

    pub(crate) async fn delete_pipeline(
        &self,
        tenant_id: TenantId,
        db: &ProjectDB,
        pipeline_id: PipelineId,
    ) -> Result<HttpResponse, ManagerError> {
        // Kill pipeline.
        let response = self
            .do_shutdown_pipeline(tenant_id, db, pipeline_id)
            .await?;
        if !response.status().is_success() {
            return Ok(response);
        }

        // Delete pipeline directory.
        match remove_dir_all(self.config.pipeline_dir(pipeline_id)).await {
            Ok(_) => (),
            Err(e) => {
                log::warn!(
                    "Failed to delete pipeline directory for pipeline {}: {}",
                    pipeline_id,
                    e
                );
            }
        }
        db.delete_pipeline(tenant_id, pipeline_id).await?;

        Ok(HttpResponse::Ok().json("Pipeline successfully deleted."))
    }

    pub(crate) async fn forward_to_pipeline(
        &self,
        tenant_id: TenantId,
        pipeline_id: PipelineId,
        method: Method,
        endpoint: &str,
    ) -> Result<HttpResponse, ManagerError> {
        let pipeline_descr = self
            .db
            .lock()
            .await
            .get_pipeline_by_id(tenant_id, pipeline_id)
            .await?;

        if pipeline_descr.status == PipelineStatus::Shutdown {
            Err(RunnerError::PipelineShutdown { pipeline_id })?
        }

        Self::do_forward_to_pipeline(pipeline_id, method, endpoint, pipeline_descr.port).await
    }

    /// Forward HTTP request to pipeline.  Assumes that the pipeline is running.
    /// Takes pipeline port as an argument instead of reading it from the database.
    async fn do_forward_to_pipeline(
        pipeline_id: PipelineId,
        method: Method,
        endpoint: &str,
        port: u16,
    ) -> Result<HttpResponse, ManagerError> {
        let client = Client::default();
        let request = client.request(method, &format!("http://localhost:{port}/{endpoint}",));

        let mut response = request
            .send()
            .await
            .map_err(|e| RunnerError::HttpForwardError {
                pipeline_id,
                error: e.to_string(),
            })?;

        let response_body = response
            .body()
            .await
            .map_err(|e| RunnerError::HttpForwardError {
                pipeline_id,
                error: e.to_string(),
            })?;

        let mut response_builder = HttpResponse::build(response.status());
        // Remove `Connection` as per
        // https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Connection#Directives
        for (header_name, header_value) in response
            .headers()
            .iter()
            .filter(|(h, _)| *h != "connection")
        {
            response_builder.insert_header((header_name.clone(), header_value.clone()));
        }

        Ok(response_builder.body(response_body))
    }

    pub(crate) async fn forward_to_pipeline_as_stream(
        &self,
        tenant_id: TenantId,
        pipeline_id: PipelineId,
        endpoint: &str,
        req: HttpRequest,
        body: Payload,
    ) -> Result<HttpResponse, ManagerError> {
        let pipeline_descr = self
            .db
            .lock()
            .await
            .get_pipeline_by_id(tenant_id, pipeline_id)
            .await?;
        if pipeline_descr.status == PipelineStatus::Shutdown {
            Err(RunnerError::PipelineShutdown { pipeline_id })?
        }
        let port = pipeline_descr.port;

        // TODO: it might be better to have ?name={}, otherwise we have to
        // restrict name format
        let url = format!("http://localhost:{port}/{endpoint}?{}", req.query_string());

        let client = awc::Client::new();

        let mut request = client.request(req.method().clone(), url);

        for header in req
            .headers()
            .into_iter()
            .filter(|(h, _)| *h != "connection")
        {
            request = request.append_header(header);
        }

        let response =
            request
                .send_stream(body)
                .await
                .map_err(|e| RunnerError::HttpForwardError {
                    pipeline_id,
                    error: e.to_string(),
                })?;

        let mut builder = HttpResponseBuilder::new(response.status());
        for header in response.headers().into_iter() {
            builder.append_header(header);
        }
        Ok(builder.streaming(response))
    }

    async fn start(&self, pr: PipelineRevision) -> Result<Child, ManagerError> {
        let pipeline_id = pr.pipeline.pipeline_id;
        let program_id = pr.pipeline.program_id.unwrap();

        log::debug!("Pipeline config is '{}'", pr.config);

        // Create pipeline directory (delete old directory if exists); write metadata
        // and config files to it.
        let pipeline_dir = self.config.pipeline_dir(pipeline_id);
        create_dir_all(&pipeline_dir).await.map_err(|e| {
            ManagerError::io_error(
                format!("creating pipeline directory '{}'", pipeline_dir.display()),
                e,
            )
        })?;
        let config_file_path = self.config.config_file_path(pipeline_id);
        fs::write(&config_file_path, &pr.config)
            .await
            .map_err(|e| {
                ManagerError::io_error(
                    format!("writing config file '{}'", config_file_path.display()),
                    e,
                )
            })?;
        let metadata_file_path = self.config.metadata_file_path(pipeline_id);
        fs::write(&metadata_file_path, serde_json::to_string(&pr).unwrap())
            .await
            .map_err(|e| {
                ManagerError::io_error(
                    format!("writing metadata file '{}'", metadata_file_path.display()),
                    e,
                )
            })?;

        // Locate project executable.
        let executable = self
            .config
            .versioned_executable(program_id, pr.program.version);

        // Run executable, set current directory to pipeline directory, pass metadata
        // file and config as arguments.
        let pipeline_process = Command::new(executable)
            .current_dir(self.config.pipeline_dir(pipeline_id))
            .arg("--config-file")
            .arg(&config_file_path)
            .arg("--metadata-file")
            .arg(&metadata_file_path)
            .stdin(Stdio::null())
            .spawn()
            .map_err(|e| RunnerError::PipelineStartupError {
                pipeline_id,
                error: e.to_string(),
            })?;

        Ok(pipeline_process)
    }

    async fn wait_for_startup(
        pipeline_id: PipelineId,
        port_file_path: &Path,
    ) -> Result<u16, Either<ManagerError, HttpResponse>> {
        // Wait for the pipeline server to start and create a port number file.
        let start = Instant::now();
        let mut count = 0;
        let port_number;
        loop {
            let res: Result<String, std::io::Error> = fs::read_to_string(port_file_path).await;
            match res {
                Ok(port) => {
                    let parse = port.trim().parse::<u16>();
                    match parse {
                        Ok(port) => {
                            port_number = port;
                            break;
                        }
                        Err(e) => Err(ManagerError::from(RunnerError::PortFileParseError {
                            pipeline_id,
                            error: e.to_string(),
                        }))
                        .map_err(Either::Left)?,
                    };
                }
                Err(e) => {
                    if start.elapsed() > PORT_FILE_TIMEOUT {
                        Err(ManagerError::from(
                            RunnerError::PipelineInitializationTimeout {
                                pipeline_id,
                                timeout: PORT_FILE_TIMEOUT,
                            },
                        ))
                        .map_err(Either::Left)?
                    }
                    if start.elapsed() > PORT_FILE_LOG_QUIET_PERIOD && (count % 10) == 0 {
                        log::info!("Could not read runner port file yet. Retrying\n{}", e);
                    }
                    count += 1;
                }
            }
            sleep(PORT_FILE_POLL_PERIOD).await;
        }

        // Now wait for the pipeline to initialize.
        let start = Instant::now();
        let mut count = 0;

        loop {
            let response =
                Self::do_forward_to_pipeline(pipeline_id, Method::GET, "stats", port_number)
                    .await
                    .map_err(Either::Left)?;
            if response.status().is_success() {
                // Status Ok, pipeline is running.
                break;
            } else if response.status() == StatusCode::SERVICE_UNAVAILABLE {
                // SERVICE_UNAVAILABLE - pipeline still initializing.
                if start.elapsed() > STARTUP_TIMEOUT {
                    Err(ManagerError::from(
                        RunnerError::PipelineInitializationTimeout {
                            pipeline_id,
                            timeout: STARTUP_TIMEOUT,
                        },
                    ))
                    .map_err(Either::Left)?
                }
                if start.elapsed() > STARTUP_QUIET_PERIOD && (count % 10) == 0 {
                    log::info!("Waiting for pipeline initialization.");
                }
                count += 1;
            } else {
                // Any other error indicates startup failure.
                return Err(Either::Right(response));
            }

            sleep(STARTUP_POLL_PERIOD).await;
        }

        Ok(port_number)
    }

    async fn update_pipeline_status(&self, pipeline_id: PipelineId) -> {
        let response =
            Self::do_forward_to_pipeline(pipeline_id, Method::GET, "stats", port_number)
            .await
            .map_err(Either::Left)?;
        // pipeline unreachable.

        if response.status().is_success() {
            // Status Ok, pipeline is running.
            parse PipelineStatus body -> PipelineState::invalid_response.
            pause/running/shutting down.
            break;
        } else {
            parse error response -> PipelineState::invalid_response.
            Initializing/Failed to initialize
        } else {
            // Any other error indicates startup failure.
            return Err(Either::Right(response));
        }
    }

    async fn do_shutdown_pipeline(
        &self,
        tenant_id: TenantId,
        db: &ProjectDB,
        pipeline_id: PipelineId,
    ) -> Result<HttpResponse, ManagerError> {
        let pipeline_descr = db.get_pipeline_by_id(tenant_id, pipeline_id).await?;

        if pipeline_descr.status == PipelineStatus::Shutdown {
            return Ok(HttpResponse::Ok().json("Pipeline already shut down."));
        };

        let url = format!("http://localhost:{}/shutdown", pipeline_descr.port);

        let client = awc::Client::new();

        let mut response = match client.get(&url).send().await {
            Ok(response) => response,
            Err(_) => {
                db.set_pipeline_status(tenant_id, pipeline_id, PipelineStatus::Shutdown)
                    .await?;
                // We failed to reach the pipeline, which likely means
                // that it crashed or was killed manually by the user.
                return Ok(
                    HttpResponse::Ok().json(&format!("Pipeline at '{url}' already shut down."))
                );
            }
        };

        if response.status().is_success() {
            db.set_pipeline_status(tenant_id, pipeline_id, PipelineStatus::Shutdown)
                .await?;
            Ok(HttpResponse::Ok().json("Pipeline successfully terminated."))
        } else {
            let mut builder = HttpResponseBuilder::new(response.status());
            for header in response.headers().into_iter() {
                builder.append_header(header);
            }
            Ok(builder.body(
                response
                    .body()
                    .await
                    .map_err(|e| RunnerError::HttpForwardError {
                        pipeline_id,
                        error: e.to_string(),
                    })?,
            ))
        }
    }
}
