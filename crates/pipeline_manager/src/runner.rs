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
    ) -> Result<(), ManagerError> {
        self.commit_revision(tenant_id, pipeline_id).await?;
        self.set_desired_state(tenant_id, pipeline_id, PipelineStatus::Paused)?;
        self.notify_pipeline_automaton(tenant_id, pipeline_id)
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
    ) -> Result<(), ManagerError> {
        self.set_desired_state(tenant_id, pipeline_id, PipelineStatus::Shutdown)?;
        self.notify_pipeline_automaton(tenant_id, pipeline_id)
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
    ) -> Result<(), ManagerError> {
        // Make sure that the pipeline is in a `Shutdown` state.
        let db = self.db.lock().await;
        
        let status = txn.get_pipeline_by_id(tenant_id, pipeline_id).await?.status;
        if status != PipelineStatus::Shutdown {
            return Err(ManagerError);
        };
        txn.delete_pipeline(tenant_id, pipeline_id).await?;
        txn.commit()
        drop(db);
        // No need to do anything else since the pipeline was in the `Shutdown` state.  
    }

    pub(crate) async fn pause_pipeline(
        &self,
        tenant_id: TenantId,
        pipeline_id: PipelineId,
    ) -> Result<(), ManagerError> {
        self.set_desired_state(tenant_id, pipeline_id, PipelineStatus::Paused)?;
        self.notify_pipeline_automaton(tenant_id, pipeline_id)
    }

    pub(crate) async fn start_pipeline(
        &self,
        tenant_id: TenantId,
        pipeline_id: PipelineId,
    ) -> Result<(), ManagerError> {
        self.set_desired_state(tenant_id, pipeline_id, PipelineStatus::Running)?;
        self.notify_pipeline_automaton(tenant_id, pipeline_id)
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

    pub(crate) async fn update_pipeline_status (
        &self,
        tenant_id: TenantId,
        pipeline_id: PipelineId,
    ) -> Result<HttpResponse, ManagerError> {
        match self {
            Self::Local(local) => {
                local
                    .update_pipeline_status(tenant_id, pipeline_id)
                    .await
            }
        }
    }

    /// Retrieves the last revision for a pipeline.
    ///
    /// Tries to create a new revision if this pipeline never had a revision
    /// created before.
    async fn commit_revision(
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
            Ok(_revision) => Ok(()),
            Err(DBError::RevisionNotChanged) => Ok(()),
            Err(e) => Err(e)?,
        }
    }
}

impl LocalRunner {
    pub(crate) fn new(
        db: Arc<Mutex<ProjectDB>>,
        config: &ManagerConfig,
    ) -> Result<Self, ManagerError> {
        let mut result = Self {
            db,
            config: config.clone(),
        }

        // TODO: kill stale pipelines.

        Ok()
    }

    async fn notify_pipeline_automaton(&self, tenant_id: TenantId, pipeline_id: PipelineId) {
        let (notifier, _handle) = self.pipelines.entry(pipeline_id).or_insert_with(|| {
            notifier = Notify::new();
            handle = spawn(|| Self::pipeline_automaton(db.clone(), notifier.clone(), tenant_id, pipeline_id));
            (notifier, handle)
        });
        notifier.notify();
    }
    
    async fn pipeline_automaton(db: Arc<Mutex<ProjectDB>>, notifier: Notify, tenant_id: TenantId, pipeline_id: PipelineId) -> Result<(), ManagerError>{
        do_pipeline_automaton(db, notifier, pipeline_id).map_err(|e| {
            error!("Pipeline automaton '{pipeline_id}' terminated with error: '{e}'");
            e
        }
    }

    async fn do_pipeline_automaton(db: Arc<Mutex<ProjectDB>>, notifier: Notify, tenant_id: TenantId, pipeline_id: PipelineId) -> Result<(), ManagerError> {
        let mut poll_timeout = Duration::DEFAULT_PIPELINE_POLL_PERIOD;
    
        loop {
            timeout(poll_timeout, notifier.notified());
            poll_timeout = Duration::DEFAULT_PIPELINE_POLL_PERIOD;

            txn = db.transaction();
            let mut pipeline = db.get_pipeline_runtime_state(txn, pipeline_id).await?;

            if pipeline.current_status == PipelineStatus::Shutdown &&
               pipeline.desired_status != PipelineStatus::Shutdown
            {
                pipeline.set_current_status(PipelineStatus::Provisioning, None)?;
                let revision = db.set_pipeline_runtime_status(txn, pipeline_id)?;
                txn.commit();

                match self.start(revision) {
                    Ok(child) => {
                        pipeline.set_pid(child.id());
                        db.set_pipeline_runtime_status(pipeline)?;
                    }
                    Err(e) {
                        pipeline.set_current_status(PipelineStatus::Shutdown, Some(e));
                        pipeline.set_desired_status(PipelineStatus::Shutdown);
                        db.set_pipeline_runtime_status(pipeline)?;
                    }
                }
                continue;
            } else {
                txn.commit();
            }

            match (pipeline.current_status, pipeline.desired_status) {
                (PipelineStatus::Provisioning, PipelineStatus::Running) |
                (PipelineStatus::Provisioning, PipelineStatus::Paused) => {
                    match Self::read_pipeline_port_file(pipeline_id) {
                        Ok(Some(port)) => {
                            pipeline.set_current_status(PipelineStatus::Initializing);
                            pipeline.set_port(port);
                            db.set_pipeline_runtime_status(pipeline)?;
                        }
                        Ok(None) => {
                            if pipeline.status_since.elapsed() > PROVISIONING_TIMEOUT {
                                Self::force_kill_pipeline(pipeline, ErrorResponse);
                            } else {
                                poll_timeout = PROVISIONING_POLL_PERIOD;
                            }
                        }
                        Err(e) => {
                            Self::force_kill_pipeline(pipeline, ErrorResponse);
                        }
                    }
                }
                (PipelineStatus::Provisioning, PipelineStatus::Shutdown) => {
                    Self::kill(pipeline.pid);
                    pipeline.set_current_status(PipelineStatus::Shutdown, None);
                    db.set_pipeline_runtime_status(pipeline)?;
                }
                (PipelineStatus::Initializing, PipelineStartup::Running) |
                (PipelineStatus::Initializing, PipelineStartup::Paused) => {
                    match self.forward_to_pipeline(pipeline_id, pipeline.port, Method::GET, "stats") {
                        Err(e) => {
                            Self::force_kill_pipeline(pipeline, ErrorResponse);
                        }
                        Ok(response) => {
                            if response.status().is_success() {
                                pipeline.set_current_status(PipelineStatus::Paused, None);
                                db.set_pipeline_runtime_status(pipeline)?;
                            } else if response.status == StatusCode::SERVICE_UNAVAILABLE {
                                if pipeline.status_since.elapsed > INITIALIZATION_TIMEOUT {
                                    Self::force_kill_pipeline(pipeline, ErrorResponse);
                                } else {
                                    poll_timeout = INITIALIZATION_POLL_PERIOD;
                                }
                            } else {
                                Self::force_kill_pipeline(pipeline, ErrorResponse);
                            }
                        }
                    }
                }
                (PipelineStatus::Initializing, PipelineStatus::Shutdown) => {
                    Self::kill(pipeline.pid);
                    pipeline.set_current_status(PipelineStatus::Shutdown, None);
                    db.set_pipeline_runtime_status(pipeline)?;
                }
                (PipelineStatus::Paused, PipelineStatus::Running) => {
                    match self.forward_to_pipeline(pipeline_id, pipeline.port, Method::GET, "start") {
                        Err(e) => {
                            Self::force_kill_pipeline(pipeline, ErrorResponse);
                        }
                        Ok(response) => {
                            if response.status().is_success() {
                                pipeline.set_current_status(PipelineStatus::Running, None);
                                db.set_pipeline_runtime_status(pipeline)?;
                            } else {
                                Self::force_kill_pipeline(pipeline, ErrorResponse);
                            }
                        }
                    }
                }
                (PipelineStatus::Running, PipelineStatus::Paused) => {
                    match self.forward_to_pipeline(pipeline_id, pipeline.port, Method::GET, "pause") {
                        Err(e) => {
                            Self::force_kill_pipeline(pipeline, ErrorResponse);
                        }
                        Ok(response) => {
                            if response.status().is_success() {
                                pipeline.set_current_status(PipelineStatus::Paused, None);
                                db.set_pipeline_runtime_status(pipeline)?;
                            } else {
                                Self::force_kill_pipeline(pipeline, ErrorResponse);
                            }
                        }
                    }
                }
                (PipelineStatus::Running, PipelineStatus::Shutdown) |
                (PipelineStatus::Paused, PipelineStatus::Shutdown) => {
                    match self.forward_to_pipeline(pipeline_id, pipeline.port, Method::GET, "shutdown") {
                        Err(e) => {
                            Self::force_kill_pipeline(pipeline, ErrorResponse);
                        }
                        Ok(response) => {
                            if response.status().is_success() {
                                pipeline.set_current_status(PipelineStatus::ShuttingDown, None);
                                db.set_pipeline_runtime_status(pipeline)?;
                            } else {
                                Self::force_kill_pipeline(pipeline, ErrorResponse);
                            }
                        }
                    }
                }
                (PipelineStatus::ShuttingDown, _) => {
                    if !process_exists(pipeline.pid) {
                        pipeline.set_current_status(PipelineStatus::Shutdown, None);
                        db.set_pipeline_runtime_status(pipeline)?;
                    } else if pipeline.status_since.elapsed > SHUTDOWN_TIMEOUT {
                        Self::force_kill_pipeline(pipeline, ErrorResponse);
                    } else {
                        poll_timeout = SHUTDOWN_POLL_PERIOD;
                    }
                }

                (PipelineStatus::Running, _) |
                (PipelineStatus::Paused, _) => {
                    match self.forward_to_pipeline(pipeline_id, pipeline.port, Method::GET, "stats") {
                        Err(e) => {
                            Self::force_kill_pipeline(pipeline, ErrorResponse);
                        }
                        Ok(response) => {
                            if !response.status().is_success() {
                                Self::force_kill_pipeline(pipeline, ErrorResponse);
                            }
                        }
                    }

                }
            }
        }
    }

    fn force_kill_pipeline(mut pipeline: PipelineRutimeStatus, error: ErrorResponse) {
        Self::kill(pipeline.pid);
        pipeline.set_current_status(PipelineStatus::Shutdown, Some(error));
        pipeline.set_desired_status(PipelineStatus::Shutdown);
        db.set_pipeline_runtime_status(pipeline_id, pipeline);
    }

    /*
    pub(crate) async fn provision_pipeline(
        &self,
        pipeline_revision,
    ) -> Result<String, ManagerError> {
        let mut pipeline_process = self.start(pipeline_revision).await.map_err(|e| {
            let _ = self.set_pipeline_status(PipelineStatus::Shutdown);
            e
        })?;

        Ok(pipeline_process.id().to_string())
    }

    pub async fn update_pipeline_status(&self, pipeline_id) {
        let (status, status_since, handle) = self.get_pipeline_by_id(pipeline_id);
        match status {
            PipelineStatus::Shutdown => {
                // do nothing.
            }
            PipelineStatus::Provisioning => {
                match Self::read_pipeline_port_file(pipeline_id) {
                    Ok(Some(port_number)) => {
                        self.set_pipeline_status_and_port(PipelineStatus::Initializing, port_number)?;
                        self.update_pipeline_status(pipeline_id)?;
                    }
                    Ok(None) => {
                        if status_since.elapsed() > PORT_FILE_TIMEOUT {
                            self.set_pipeline_status(PipelineStatus::Unreachable)?;
                        }
                    };
                    Err(e) => {
                        self.set_pipeline_status(???)
                    }
                }
            }
            PipelineStatus::ShuttingDown => {
                match self.poll_pipeline_status(pipeline_id) {
                    PipelineStatus::Unreachable => {
                        self.set_pipeline_status(PipelineStatus::Shutdown)?;
                    }
                    _ => {}
                }
            }
            PipelineStatus::Initializing => {
                match self.poll_pipeline_status(pipeline_id)? {
                    PipelineStatus::Initializing => {
                        if status_since.elapsed() > STARTUP_TIMEOUT => {
                            self.set_pipeline_status(PipelineStatus::Unreachable)?;
                        }
                    }
                    status => self.set_pipeline_status(status)?,
                }
            }
            PipelineStatus::Unreachable => {
                // Remember the original reason the pipeline became unreachable.
                match self.poll_pipeline_status(pipeline_id)? {
                    PipelineStatus::Unreachable => {}
                    status => self.set_pipeline_status(new_status)?
                }
            }
            PipelineStatus::Running |
            PipelineStatus::Paused |
            PipelineStatus::Failed => {
                let new_status = self.poll_pipeline_status(pipeline_id);
                if new_status != status {
                    self.set_pipeline_status(new_status)?
                };
            }
        }
    }
    */

    fn read_pipeline_port_file(&self, pipeline_id: PipelineId) -> Result<Option<u16>, ManagerError>
    {
        let port_file_path = self.config.port_file_path(pipeline_id);

        match fs::read_to_string(port_file_path).await {
            Ok(port) => {
                let parse = port.trim().parse::<u16>();
                match parse {
                    Ok(port) => {
                        Ok(Some(port))
                    }
                    Err(e) => Err(ManagerError::from(RunnerError::PortFileParseError {
                        pipeline_id,
                        error: e.to_string(),
                    }))
                };
            }
            Err(e) => {
                Ok(None)
            }
        }
    }

    /*pub(crate) async fn shutdown_pipeline(
        &self,
        tenant_id: TenantId,
        pipeline_id: PipelineId,
    ) -> Result<HttpResponse, ManagerError> {
        let db = self.db.lock().await;
        self.do_shutdown_pipeline(tenant_id, &db, pipeline_id).await
    }*/

    /*pub(crate) async fn pause_pipeline(
        &self,
        tenant_id: TenantId,
        pipeline_id: PipelineId,
    ) -> Result<bool, ManagerError> {
        let db = self.db.lock().await;
        Ok(db
            .set_pipeline_status(tenant_id, pipeline_id, PipelineStatus::Paused)
            .await?)
    }*/

    /*pub(crate) async fn start_pipeline(
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
    }*/

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

    async fn poll_pipeline_status(&self, pipeline_id: PipelineId) -> Result<PipelineStatus, ManagerError> {
        /*
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
         */
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

    /*
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
    */
}
