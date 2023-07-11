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
    PipelineProvisioningTimeout {
        pipeline_id: PipelineId,
        timeout: Duration,
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
    pipelines: Mutex<BTreeMap<PipelineId, Notify>>,
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
        self.set_desired_status(tenant_id, pipeline_id, PipelineStatus::Paused)?;
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
        self.set_desired_status(tenant_id, pipeline_id, PipelineStatus::Shutdown)?;
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

        // TODO: this function should run in a transaction to avoid conflicts with
        // another manager instance.
        let db = self.db.lock().await;
        
        let pipeline_state = txn.get_pipeline_runtime_state(tenant_id, pipeline_id).await?;
        Self::validate_desired_state_request(pipeline_state, None)?;

        db.delete_pipeline(tenant_id, pipeline_id).await?;
        self.notify_pipeline_automaton(tenant_id, pipeline_id);

        // No need to do anything else since the pipeline was in the `Shutdown` state.  
        // The pipeline tokio task will self-destruct when it polls pipeline
        // state and discovers it has been deleted.
    }

    fn validate_desired_state_request(pipeline_state: &PipelineRuntimeState, request: Option<PipelineStatus>) -> Result<(), ManagerError> {
        match request {
            None => {
                if pipeline_state.current_status != PipelineStatus::Shutdown ||
                   pipeline_state.desired_status != PipelineStatus::Shutdown
                {
                    return Err(RunnerError::IllegalPipelineStateTransition {
                        error: "".to_string(),
                        current_status: pipeline_state.current_status,
                        desired_status: pipeline_state.desired_status,
                        requested_status: None,
                    });
                };
            }
            Some(new_desired_status) => {
                if new_desired_status == PipelineStatus::Paused || new_desired_status == PipelineStatus::Running {
                    // Refuse to restart a pipeline that has not completed shutting down.
                    if pipeline_state.desired_status == PipelineStatus::Shutdown && pipeline_state.current_status != PipelineStatus::Shutdown {
                        Err(RunnerError::IllegalPipelineStateTransition {
                            error: "".to_string(),
                            current_status: pipeline_state.current_status,
                            desired_status: pipeline_state.desired_status,
                            requested_status: Some(new_desired_status),
                        })?;
                    };

                    // Refuse to restart failed pipeline until it's in the shutdown state.
                    if pipeline_state.desired_status != PipelineStatus::Shutdown && 
                      (pipeline_state.current_status == PipelineStatus::ShuttingDown || pipeline_state.current_status == PipelineStatus::Failed)
                    {
                        Err(RunnerError::IllegalPipelineStateTransition {
                            error: "".to_string(),
                            current_status: pipeline_state.current_status,
                            desired_status: pipeline_state.desired_status,
                            requested_status: Some(new_desired_status),
                        })?;
                    }
                }
            }
        }
    }

    pub(crate) async fn pause_pipeline(
        &self,
        tenant_id: TenantId,
        pipeline_id: PipelineId,
    ) -> Result<(), ManagerError> {
        self.set_desired_status(tenant_id, pipeline_id, PipelineStatus::Paused)?;
        self.notify_pipeline_automaton(tenant_id, pipeline_id)
    }

    pub(crate) async fn start_pipeline(
        &self,
        tenant_id: TenantId,
        pipeline_id: PipelineId,
    ) -> Result<(), ManagerError> {
        self.set_desired_status(tenant_id, pipeline_id, PipelineStatus::Running)?;
        self.notify_pipeline_automaton(tenant_id, pipeline_id)
    }

    async fn set_desired_status(&self, tenant_id: TenantId, pipeline_id: PipelineId, new_desired_status: PipelineStatus) -> Result<(), ManagerError> {
        // TODO: this function should run in a transaction to avoid conflicts with
        // another manager instance.

        let db = self.db.lock().await;
        let pipeline_state = db.get_pipeline_runtime_state(tenant_id, pipeline_id).await?;

        Self::validate_desired_state_request(pipeline_state, Some(new_desired_status))?;

        self.db.lock().await.set_pipeline_desired_status(tenant_id, pipeline_id, desired_status).await?;
        Ok(())
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

    pub(crate) async fn forward_to_pipeline(
        &self,
        tenant_id: TenantId,
        pipeline_id: PipelineId,
        method: Method,
        endpoint: &str,
    ) -> Result<HttpResponse, ManagerError> {
        let pipeline_state = self
            .db
            .lock()
            .await
            .get_pipeline_runtime_state(tenant_id, pipeline_id)
            .await?;

        match pipeline_state.current_status {
            PipelineStatus::Shutdown |
            PipelineStatus::Failed |
            PipelineStatus::Provisionining => {
                Err(RunnerError::PipelineShutdown { pipeline_id })?
            }
            _ => {}
        }

        Self::do_forward_to_pipeline(pipeline_id, method, endpoint, pipeline_state.port).await
    }

    /// Forward HTTP request to pipeline.  Assumes that the pipeline is running.
    /// Takes pipeline port as an argument instead of reading it from the database.
    async fn do_forward_to_pipeline(
        pipeline_id: PipelineId,
        method: Method,
        endpoint: &str,
        port: u16,
    ) -> Result<HttpResponse, ManagerError> {
        let mut response = pipeline_http_request(pipeline_id, method, endpoint, port)
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

    fn pipeline_http_request(
        pipeline_id: PipelineId,
        method: Method,
        endpoint: &str,
        port: u16,
    ) -> SendClientRequest {
        let client = Client::default();
        let request = client.request(method, &format!("http://localhost:{port}/{endpoint}",));

        request.send()
    }

    async fn pipeline_http_request_json_response(
        pipeline_id: PipelineId,
        method: Method,
        endpoint: &str,
        port: u16
    ) -> Result<(JsonValue), RunnerError> {
        let mut response = pipeline_http_request(pipeline_id, method, endpoint, port)
            .await
            .map_err(|e| RunnerError::HttpForwardError {
                pipeline_id,
                error: e.to_string(),
            })?;

        let value = response.json::<JsonValue>()
            .await
            .map_err(|e| HttpForwardError {
                pipeline_id,
                error: e.to_string(),
            })?;

        (response.status(), value)
    }

    pub(crate) async fn forward_to_pipeline_as_stream(
        &self,
        tenant_id: TenantId,
        pipeline_id: PipelineId,
        endpoint: &str,
        req: HttpRequest,
        body: Payload,
    ) -> Result<HttpResponse, ManagerError> {
        let pipeline_state = self
            .db
            .lock()
            .await
            .get_pipeline_runtime_state(tenant_id, pipeline_id)
            .await?;

        match pipeline_state.current_status {
            PipelineStatus::Shutdown |
            PipelineStatus::Failed |
            PipelineStatus::Provisionining => {
                Err(RunnerError::PipelineShutdown { pipeline_id })?
            }
            _ => {}
        }
        let port = pipeline_state.port;

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
}

struct PipelineHandle {
    pipeline_process: Child,
}

impl Drop for PipelineHandle {
    fn drop(&mut self) {
        self.pipelone_process.kill();
    }
}

struct PipelineAutomaton {
    pipeline_id: PipelineId,
    tenant_id: TenantId,
    pipeline_process: Option<PipelineHandle>,
    config: Arc<ManagerConfig>,
    db: Arc<Mutex<ProjectDB>>,
    notifier: Notify,
}

impl PipelineAutomaton {
    /// The frequency of polling the pipeline during normal operation
    /// when we don't normally expect its state to change.
    const DEFAULT_PIPELINE_POLL_PERIOD = Duration::from_millis(10_000);

    /// Max time to wait for the pipeline process to initialize its
    /// HTTP server.
    const PROVISIONING_TIMEOUT = Duration::from_millis(10_000);

    /// How often to check for the pipeline port file during the
    /// provisioning phase.
    const PROVISIONING_POLL_PERIOD = Duration::from_millis(300);

    /// Max time to wait for the pipeline to initialize all connectors.
    const INITIALIZATION_TIMEOUT = Duration::from_millis(20_000);
    
    /// How often to poll for the pipeline initialization status.
    const INITIALIZATION_POLL_PERIOD = Duration::from_millis(300);

    /// Max time to wait for the pipeline process to exit.
    // TODO: It seems that Actix often takes a while to shutdown.  This
    // is something to investigate.
    const SHUTDOWN_TIMEOUT = Duration::from_millis(3_000);

    /// How often to poll for the pipeline process to exit.
    const SHUTDOWN_POLL_PERIOD = Duration::from_millis(300);

    fn new() -> Self {
        Self {
            pipeline_id,
            tenant_id,
            pipeline_process: None,
            config,
            db,
            notifier,
        }
    }

    async fn run(self) -> Result<(), ManagerError> {
        do_run(self).map_err(|e| {
            error!("Pipeline automaton '{pipeline_id}' terminated with error: '{e}'");
            e
        }
    }

    async fn do_run(mut self) -> Result<(), ManagerError> {
        let mut poll_timeout = DEFAULT_PIPELINE_POLL_PERIOD;
    
        loop {
            timeout(poll_timeout, notifier.notified());
            poll_timeout = DEFAULT_PIPELINE_POLL_PERIOD;

            // TODO: use transactional API when ready to avoid races with a parallel
            // `/deploy /shutdown` sequence.

            // txn = db.transaction();
            let mut pipeline = self.db.get_pipeline_runtime_state(pipeline_id).await?;

            if pipeline.current_status == PipelineStatus::Shutdown &&
               pipeline.desired_status != PipelineStatus::Shutdown
            {
                pipeline.set_current_status(PipelineStatus::Provisioning, None)?;
                let revision = self.db.get_last_committed_pipeline_revision(pipeline_id)?;
                self.db.set_pipeline_runtime_state(pipeline)?;
                // txn.commit();

                match self.start(revision) {
                    Ok(child) => {
                        pipeline_process = Some(child);
                        self.db.set_pipeline_runtime_state(pipeline)?;
                    }
                    Err(e) {
                        self.force_kill_pipeline(pipeline, Some(e));
                    }
                }
                continue;
            } else {
                // txn.commit();
            }

            match (pipeline.current_status, pipeline.desired_status) {
                (PipelineStatus::Provisioning, PipelineStatus::Running) |
                (PipelineStatus::Provisioning, PipelineStatus::Paused) => {
                    match Self::read_pipeline_port_file(pipeline_id) {
                        Ok(Some(port)) => {
                            pipeline.set_current_status(PipelineStatus::Initializing);
                            pipeline.set_port(port);
                            pipeline.set_created();
                            self.db.update_pipeline_runtime_state(pipeline)?;
                        }
                        Ok(None) => {
                            if pipeline.status_since.elapsed() > PROVISIONING_TIMEOUT {
                                self.force_kill_pipeline(pipeline, Some(RunnerError::PipelineProvisioningTimeout)) {
                                    pipeline_id,
                                    timeout: PROVISIONING_TIMEOUT,
                                });
                            } else {
                                poll_timeout = PROVISIONING_POLL_PERIOD;
                            }
                        }
                        Err(e) => {
                            self.force_kill_pipeline(pipeline, Some(e));
                        }
                    }
                }
                (PipelineStatus::Provisioning, PipelineStatus::Shutdown) => {
                    selfforce_kill_pipeline(pipeline, None);
                }
                (PipelineStatus::Initializing, PipelineStartup::Running) |
                (PipelineStatus::Initializing, PipelineStartup::Paused) => {
                    match Runner::pipeline_http_request_json_response(pipeline_id, Method::GET, "stats", pipeline.port) {
                        Err(e) => {
                            self.force_kill_pipeline(pipeline, Some(e));
                        }
                        Ok((status, response)) => {
                            if status.is_success() {
                                pipeline.set_current_status(PipelineStatus::Paused, None);
                                self.db.update_pipeline_runtime_state(pipeline)?;
                            } else if status == StatusCode::SERVICE_UNAVAILABLE {
                                if pipeline.status_since.elapsed > INITIALIZATION_TIMEOUT {
                                    self.force_kill_pipeline(pipeline, RunnerError::PipelineInitializationTimeout {
                                        pipeline_id,
                                        timeout: INITIALIZATION_TIMEOUT,
                                    });
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
                    self.force_kill_pipeline(pipeline, None);
                }
                (PipelineStatus::Paused, PipelineStatus::Running) => {
                    match Runner::pipeline_http_request_json_response(pipeline_id, Method::GET, "start", pipeline.port) {
                        Err(e) => {
                            self.force_kill_pipeline(pipeline, Some(e));
                        }
                        Ok((status, body)) => {
                            if status.is_success() {
                                pipeline.set_current_status(PipelineStatus::Running, None);
                                self.db.update_pipeline_runtime_state(pipeline)?;
                            } else {
                                self.force_kill_pipeline(pipeline, Some(error_response_from_json(pipeline_id, status, body)));
                            }
                        }
                    }
                }
                (PipelineStatus::Running, PipelineStatus::Paused) => {
                    match Runner::pipeline_http_request_json_response(pipeline_id, Method::GET, "pause", pipeline.port) {
                        Err(e) => {
                            selfforce_kill_pipeline(pipeline, Some(r));
                        }
                        Ok((status, body)) => {
                            if status.is_success() {
                                pipeline.set_current_status(PipelineStatus::Paused, None);
                                self.db.update_pipeline_runtime_state(pipeline)?;
                            } else {
                                self.force_kill_pipeline(pipeline, Some(error_response_from_json(pipeline_id, status, body)));
                            }
                        }
                    }
                }
                (PipelineStatus::Running, PipelineStatus::Shutdown) |
                (PipelineStatus::Paused, PipelineStatus::Shutdown) => {
                    match Runner::pipeline_http_request_json_response(pipeline_id, Method::GET, "shutdown", pipeline.port) {
                        Err(e) => {
                            self.force_kill_pipeline(pipeline, Some(e));
                        }
                        Ok((status, body)) => {
                            if status.is_success() {
                                pipeline.set_current_status(PipelineStatus::ShuttingDown, None);
                                self.db.update_pipeline_runtime_state(pipeline)?;
                            } else {
                                self.force_kill_pipeline(pipeline, Some(error_response_from_json(pipeline_id, status, body)));
                            }
                        }
                    }
                }
                (PipelineStatus::ShuttingDown, _) => {
                    if pipeline_process.map(|p| p.pipeline_process.try_wait().is_ok()).unwrap_or(true) {
                        self.pipeline_process = None;
                        pipeline.set_current_status(PipelineStatus::Shutdown, None);
                        self.db.update_pipeline_runtime_state(pipeline)?;
                    } else if pipeline.status_since.elapsed > SHUTDOWN_TIMEOUT {
                        self.force_kill_pipeline(pipeline, RunnerError::PipelineShutdownTimeout {
                            pipeline_id,
                            timeout: SHUTDOWN_TIMEOUT,
                        });
                    } else {
                        poll_timeout = SHUTDOWN_POLL_PERIOD;
                    }
                }

                (PipelineStatus::Running, _) |
                (PipelineStatus::Paused, _) => {
                    match Runner::pipeline_http_request_json_response(pipeline_id, Method::GET, "stats", pipeline.port) {
                        Err(e) => {
                            self.force_kill_pipeline(pipeline, Some(e));
                        }
                        Ok((status, json_body)) => {
                            if !status.is_success() {
                                self.force_kill_pipeline(pipeline, Some(error_response_from_json(pipeline_id, status, body));
                            } else {
                                let state = json_body.get("global_metrics").get("state").as_str();
                                if state == "Paused" {
                                    pipeline.set_current_status(PipelineStatus::Paused, None);
                                    self.db.update_pipeline_runtime_state(pipeline)?;
                                } else if state == "Running" {
                                    pipeline.set_current_status(PipelineStatus::Running, None);
                                    self.db.update_pipeline_runtime_state(pipeline)?;
                                } else {
                                    self.force_kill_pipeline(pipeline, );
                                }
                            }
                        }
                    }
                }
                (PipelineStatus::Failed, PipelineStatus::Shutdown) => {
                    pipeline.set_current_status(PipelineStatus::Shutdown, pipeline.error);
                    self.db.update_pipeline_runtime_state(pipeline_id, pipeline);
                }
                (PipelineStatus::Failed, _) => { }
            }
        }
    }

    fn force_kill_pipeline(&mut self, mut pipeline: PipelineRutimeStatus, error: ErrorResponse) {
        self.pipeline_process = None;

        if pipeline.desired_status == PipelineStatus::Shutdown {
            pipeline.set_current_status(PipelineStatus::Shutdown, Some(error));
        } else {
            pipeline.set_current_status(PipelineStatus::Failed, Some(error));
        }
        self.db.set_pipeline_runtime_status(pipeline_id, pipeline);
    }

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

    async fn start(&self, pr: PipelineRevision) -> Result<PipelineHandle, ManagerError> {
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

    fn error_response_from_json(pipeline_id: PipelineId, status: StatusCode, json: &JsonValue) -> ErrorResponse {
        ErrorResponse::deserialize(json).unwrap_or_else(|e| {
            ErrorResponse::from(RunnerError::HttpForwardError {
                pipeline_id,
                error: format!("Pipeline returned HTTP status {status}, response body:{json:#}"),
            })
        })
    }
}

impl LocalRunner {
    pub(crate) fn new(
        db: Arc<Mutex<ProjectDB>>,
        config: &ManagerConfig,
    ) -> Self {
        Self {
            db,
            config: config.clone(),
            pipelines: Mutex::new(BTreeMap::new()),
        }
    }

    async fn notify_pipeline_automaton(&self, tenant_id: TenantId, pipeline_id: PipelineId) {
        self.pipelines.lock().await.entry(pipeline_id).or_insert_with(|| {
            notifier = Notify::new();
            spawn(|| PipelineAutomaton::new(db.clone(), notifier.clone(), tenant_id, pipeline_id).run().await);
            notifier
        }).notify();
    }
}
