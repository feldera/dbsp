use crate::{
    auth::TenantId,
    db::{storage::Storage, DBError, Pipeline, PipelineRevision, PipelineStatus, PipelineRuntimeState},
    ErrorResponse, ManagerConfig, ManagerError, PipelineId, ProjectDB, ResponseError,
};
use actix_web::{
    body::BoxBody,
    http::{Method, StatusCode},
    web::Payload,
    HttpRequest, HttpResponse, HttpResponseBuilder,
};
use awc::{Client, SendClientRequest};
use dbsp_adapters::DetailedError;
use either::Either;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::{
    borrow::Cow, collections::BTreeMap, error::Error as StdError, fmt, fmt::Display, path::Path, process::Stdio, sync::Arc,
};
use tokio::{
    fs, spawn,
    fs::{create_dir_all, remove_dir_all},
    process::{Child, Command},
    sync::Mutex,
    time::{sleep, Duration, Instant},
};
use uuid::Uuid;
use log::error;
use tokio::{
    time::timeout,
    sync::Notify
};
use chrono::{DateTime, Utc};

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
    PipelineShutdownTimeout {
        pipeline_id: PipelineId,
        timeout: Duration,
    },
    PipelineStartupError {
        pipeline_id: PipelineId,
        // TODO: This should be IOError, so we can serialize the error code
        // similar to `DBSPError::IO`.
        error: String,
    },
    IllegalPipelineStateTransition {
        pipeline_id: PipelineId,
        error: String,
        current_status: PipelineStatus,
        desired_status: PipelineStatus,
        requested_status: Option<PipelineStatus>,
    }
}

impl DetailedError for RunnerError {
    fn error_code(&self) -> Cow<'static, str> {
        match self {
            Self::PipelineShutdown { .. } => Cow::from("PipelineShutdown"),
            Self::HttpForwardError { .. } => Cow::from("HttpForwardError"),
            Self::PortFileParseError { .. } => Cow::from("PortFileParseError"),
            Self::PipelineProvisioningTimeout { .. } => {
                Cow::from("PipelineProvisioningTimeout")
            }
            Self::PipelineInitializationTimeout { .. } => {
                Cow::from("PipelineInitializationTimeout")
            }
            Self::PipelineShutdownTimeout { .. } => {
                Cow::from("PipelineShutdownTimeout")
            }
            Self::PipelineStartupError { .. } => Cow::from("PipelineStartupError"),
            Self::IllegalPipelineStateTransition { .. } => Cow::from("IllegalPipelineStateTransition"),
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
            Self::PipelineProvisioningTimeout {
                pipeline_id,
                timeout,
            } => {
                write!(f, "Waiting for pipeline '{pipeline_id}' to start timed out after {timeout:?}")
            }
            Self::PipelineInitializationTimeout {
                pipeline_id,
                timeout,
            } => {
                write!(f, "Waiting for pipeline '{pipeline_id}' initialization timed out after {timeout:?}")
            }
            Self::PipelineShutdownTimeout {
                pipeline_id,
                timeout,
            } => {
                write!(f, "Waiting for pipeline '{pipeline_id}' to shutdown timed out after {timeout:?}")
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
            Self::IllegalPipelineStateTransition { pipeline_id, error, .. } => {
                write!(f, "Action is not applicable in the current state of the pipeline: {error}")
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
            Self::PipelineProvisioningTimeout { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            Self::PipelineInitializationTimeout { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            Self::PipelineShutdownTimeout { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            Self::PipelineStartupError { .. } => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_response(&self) -> HttpResponse<BoxBody> {
        HttpResponseBuilder::new(self.status_code()).json(ErrorResponse::from_error(self))
    }
}

/// A runner component responsible for running and interacting with
/// pipelines at runtime.
pub(crate) struct Runner {
    db: Arc<Mutex<ProjectDB>>,
    inner: RunnerInner,
}

enum RunnerInner {
    Local(LocalRunner)
}

impl Runner {
    pub(crate) fn local(db: Arc<Mutex<ProjectDB>>, config: &ManagerConfig) -> Self {
        Self {
            db,
            inner: RunnerInner::Local(LocalRunner::new(db.clone(), config))
        }
    }

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
        self.set_desired_status(tenant_id, pipeline_id, PipelineStatus::Paused).await?;
        self.notify_pipeline_automaton(tenant_id, pipeline_id).await;
        Ok(())
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
        self.set_desired_status(tenant_id, pipeline_id, PipelineStatus::Shutdown).await?;
        self.notify_pipeline_automaton(tenant_id, pipeline_id).await;
        Ok(())
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
        
        let pipeline_state = db.get_pipeline_runtime_state(tenant_id, pipeline_id).await?;
        Self::validate_desired_state_request(pipeline_id, &pipeline_state, None)?;

        db.delete_pipeline(tenant_id, pipeline_id).await?;
        self.notify_pipeline_automaton(tenant_id, pipeline_id).await;

        // No need to do anything else since the pipeline was in the `Shutdown` state.  
        // The pipeline tokio task will self-destruct when it polls pipeline
        // state and discovers it has been deleted.

        Ok(())
    }

    fn validate_desired_state_request(pipeline_id: PipelineId, pipeline_state: &PipelineRuntimeState, request: Option<PipelineStatus>) -> Result<(), ManagerError> {
        match request {
            None => {
                if pipeline_state.current_status != PipelineStatus::Shutdown ||
                   pipeline_state.desired_status != PipelineStatus::Shutdown
                {
                    Err(RunnerError::IllegalPipelineStateTransition {
                        pipeline_id,
                        error: "Cannot delete a running pipeline. Shutdown the pipeline first by invoking the '/shutdown' endpoint.".to_string(),
                        current_status: pipeline_state.current_status,
                        desired_status: pipeline_state.desired_status,
                        requested_status: None,
                    })?
                };
            }
            Some(new_desired_status) => {
                if new_desired_status == PipelineStatus::Paused || new_desired_status == PipelineStatus::Running {
                    // Refuse to restart a pipeline that has not completed shutting down.
                    if pipeline_state.desired_status == PipelineStatus::Shutdown && pipeline_state.current_status != PipelineStatus::Shutdown {
                        Err(RunnerError::IllegalPipelineStateTransition {
                            pipeline_id,
                            error: "Cannot restart the pipeline while it is shutting down. Wait for the shutdown to complete before starting a new instance of the pipeline.".to_string(),
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
                            pipeline_id,
                            error: "Cannot restart a failed pipeline. Clear the error state first by invoking the '/shutdown' endpoint.".to_string(),
                            current_status: pipeline_state.current_status,
                            desired_status: pipeline_state.desired_status,
                            requested_status: Some(new_desired_status),
                        })?;
                    }
                }
            }
        }

        Ok(())
    }

    pub(crate) async fn pause_pipeline(
        &self,
        tenant_id: TenantId,
        pipeline_id: PipelineId,
    ) -> Result<(), ManagerError> {
        self.set_desired_status(tenant_id, pipeline_id, PipelineStatus::Paused).await?;
        self.notify_pipeline_automaton(tenant_id, pipeline_id).await;

        Ok(())
    }

    pub(crate) async fn start_pipeline(
        &self,
        tenant_id: TenantId,
        pipeline_id: PipelineId,
    ) -> Result<(), ManagerError> {
        self.set_desired_status(tenant_id, pipeline_id, PipelineStatus::Running).await?;
        self.notify_pipeline_automaton(tenant_id, pipeline_id).await;

        Ok(())
    }

    async fn set_desired_status(&self, tenant_id: TenantId, pipeline_id: PipelineId, new_desired_status: PipelineStatus) -> Result<(), ManagerError> {
        // TODO: this function should run in a transaction to avoid conflicts with
        // another manager instance.

        let db = self.db.lock().await;
        let pipeline_state = db.get_pipeline_runtime_state(tenant_id, pipeline_id).await?;

        Self::validate_desired_state_request(pipeline_id, &pipeline_state, Some(new_desired_status))?;

        db.set_pipeline_desired_status(tenant_id, pipeline_id, new_desired_status).await?;
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
    ) -> Result<(), ManagerError> {
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
            PipelineStatus::Provisioning => {
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
        let mut response = Self::pipeline_http_request(pipeline_id, method, endpoint, port)
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
    ) -> Result<(StatusCode, JsonValue), RunnerError> {
        let mut response = Self::pipeline_http_request(pipeline_id, method, endpoint, port)
            .await
            .map_err(|e| RunnerError::HttpForwardError {
                pipeline_id,
                error: e.to_string(),
            })?;

        let value = response.json::<JsonValue>()
            .await
            .map_err(|e| RunnerError::HttpForwardError {
                pipeline_id,
                error: e.to_string(),
            })?;

        Ok((response.status(), value))
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
            PipelineStatus::Provisioning => {
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

    async fn notify_pipeline_automaton(&self, tenant_id: TenantId, pipeline_id: PipelineId) {
        match &self.inner {
            RunnerInner::Local(runner) => runner.notify_pipeline_automaton(tenant_id, pipeline_id).await,
        }
    }
}

struct PipelineHandle {
    pipeline_process: Child,
}

impl Drop for PipelineHandle {
    fn drop(&mut self) {
        self.pipeline_process.kill();
    }
}

struct PipelineAutomaton {
    pipeline_id: PipelineId,
    tenant_id: TenantId,
    pipeline_process: Option<PipelineHandle>,
    config: Arc<ManagerConfig>,
    db: Arc<Mutex<ProjectDB>>,
    notifier: Arc<Notify>,
}

impl PipelineAutomaton {
    /// The frequency of polling the pipeline during normal operation
    /// when we don't normally expect its state to change.
    const DEFAULT_PIPELINE_POLL_PERIOD: Duration = Duration::from_millis(10_000);

    /// Max time to wait for the pipeline process to initialize its
    /// HTTP server.
    const PROVISIONING_TIMEOUT: Duration = Duration::from_millis(10_000);

    /// How often to check for the pipeline port file during the
    /// provisioning phase.
    const PROVISIONING_POLL_PERIOD: Duration = Duration::from_millis(300);

    /// Max time to wait for the pipeline to initialize all connectors.
    const INITIALIZATION_TIMEOUT: Duration = Duration::from_millis(20_000);
    
    /// How often to poll for the pipeline initialization status.
    const INITIALIZATION_POLL_PERIOD: Duration = Duration::from_millis(300);

    /// Max time to wait for the pipeline process to exit.
    // TODO: It seems that Actix often takes a while to shutdown.  This
    // is something to investigate.
    const SHUTDOWN_TIMEOUT: Duration = Duration::from_millis(3_000);

    /// How often to poll for the pipeline process to exit.
    const SHUTDOWN_POLL_PERIOD: Duration = Duration::from_millis(300);

    fn new(pipeline_id: PipelineId, tenant_id: TenantId, config: &Arc<ManagerConfig>, db: Arc<Mutex<ProjectDB>>, notifier: Arc<Notify>) -> Self {
        Self {
            pipeline_id,
            tenant_id,
            pipeline_process: None,
            config: config.clone(),
            db,
            notifier,
        }
    }

    async fn run(self) -> Result<(), ManagerError> {
        let pipeline_id = self.pipeline_id;

        self.do_run().await.map_err(|e| {
            error!("Pipeline automaton '{}' terminated with error: '{e}'", pipeline_id);
            e
        })
    }

    async fn do_run(mut self) -> Result<(), ManagerError> {
        let mut poll_timeout = Self::DEFAULT_PIPELINE_POLL_PERIOD;
    
        loop {
            timeout(poll_timeout, self.notifier.notified());
            poll_timeout = Self::DEFAULT_PIPELINE_POLL_PERIOD;

            // TODO: use transactional API when ready to avoid races with a parallel
            // `/deploy /shutdown` sequence.

            // txn = db.transaction();
            let db = self.db.lock().await;
            let mut pipeline = db.get_pipeline_runtime_state(self.tenant_id, self.pipeline_id).await?;

            if pipeline.current_status == PipelineStatus::Shutdown &&
               pipeline.desired_status != PipelineStatus::Shutdown
            {
                pipeline.set_current_status(PipelineStatus::Provisioning, None);
                let revision = db.get_last_committed_pipeline_revision(self.tenant_id, self.pipeline_id).await?;
                db.update_pipeline_runtime_state(self.pipeline_id, &pipeline).await?;
                // txn.commit();
                drop(db);

                match self.start(revision).await {
                    Ok(child) => {
                        self.pipeline_process = Some(child);
                    }
                    Err(e) => {
                        self.force_kill_pipeline(&pipeline, Some(e)).await?;
                    }
                }
                continue;
            } else {
                // txn.commit();
                drop(db);
            }

            match (pipeline.current_status, pipeline.desired_status) {
                (PipelineStatus::Provisioning, PipelineStatus::Running) |
                (PipelineStatus::Provisioning, PipelineStatus::Paused) => {
                    match self.read_pipeline_port_file().await {
                        Ok(Some(port)) => {
                            pipeline.set_current_status(PipelineStatus::Initializing, None);
                            pipeline.set_port(port);
                            pipeline.set_created();
                            self.update_pipeline_runtime_state(&pipeline).await?;
                        }
                        Ok(None) => {
                            if Self::timeout_expired(pipeline.status_since, Self::PROVISIONING_TIMEOUT) {
                                self.force_kill_pipeline(&pipeline, Some(RunnerError::PipelineProvisioningTimeout {
                                    pipeline_id: self.pipeline_id,
                                    timeout: Self::PROVISIONING_TIMEOUT,
                                })).await?;
                            } else {
                                poll_timeout = Self::PROVISIONING_POLL_PERIOD;
                            }
                        }
                        Err(e) => {
                            self.force_kill_pipeline(&pipeline, Some(e)).await?;
                        }
                    }
                }
                (PipelineStatus::Provisioning, PipelineStatus::Shutdown) => {
                    self.force_kill_pipeline(&pipeline, None).await?;
                }
                (PipelineStatus::Initializing, PipelineStatus::Running) |
                (PipelineStatus::Initializing, PipelineStatus::Paused) => {
                    match Runner::pipeline_http_request_json_response(self.pipeline_id, Method::GET, "stats", pipeline.port).await {
                        Err(e) => {
                            self.force_kill_pipeline(&pipeline, Some(e)).await?;
                        }
                        Ok((status, body)) => {
                            if status.is_success() {
                                pipeline.set_current_status(PipelineStatus::Paused, None);
                                self.update_pipeline_runtime_state(&pipeline).await?;
                            } else if status == StatusCode::SERVICE_UNAVAILABLE {
                                if Self::timeout_expired(pipeline.status_since, Self::INITIALIZATION_TIMEOUT) {
                                    self.force_kill_pipeline(&pipeline, Some(RunnerError::PipelineInitializationTimeout {
                                        pipeline_id: self.pipeline_id,
                                        timeout: Self::INITIALIZATION_TIMEOUT,
                                    })).await?;
                                } else {
                                    poll_timeout = Self::INITIALIZATION_POLL_PERIOD;
                                }
                            } else {
                                self.force_kill_pipeline(&pipeline, Some(Self::error_response_from_json(self.pipeline_id, status, &body))).await?;
                            }
                        }
                    }
                }
                (PipelineStatus::Initializing, PipelineStatus::Shutdown) => {
                    self.force_kill_pipeline(&pipeline, None).await?;
                }
                (PipelineStatus::Paused, PipelineStatus::Running) => {
                    match Runner::pipeline_http_request_json_response(self.pipeline_id, Method::GET, "start", pipeline.port).await {
                        Err(e) => {
                            self.force_kill_pipeline(&pipeline, Some(e)).await?;
                        }
                        Ok((status, body)) => {
                            if status.is_success() {
                                pipeline.set_current_status(PipelineStatus::Running, None);
                                self.update_pipeline_runtime_state(&pipeline).await?;
                            } else {
                                self.force_kill_pipeline(&pipeline, Some(Self::error_response_from_json(self.pipeline_id, status, &body))).await?;
                            }
                        }
                    }
                }
                (PipelineStatus::Running, PipelineStatus::Paused) => {
                    match Runner::pipeline_http_request_json_response(self.pipeline_id, Method::GET, "pause", pipeline.port).await {
                        Err(e) => {
                            self.force_kill_pipeline(&pipeline, Some(e)).await?;
                        }
                        Ok((status, body)) => {
                            if status.is_success() {
                                pipeline.set_current_status(PipelineStatus::Paused, None);
                                self.update_pipeline_runtime_state(&pipeline).await?;
                            } else {
                                self.force_kill_pipeline(&pipeline, Some(Self::error_response_from_json(self.pipeline_id, status, &body))).await?;
                            }
                        }
                    }
                }
                (PipelineStatus::Running, PipelineStatus::Shutdown) |
                (PipelineStatus::Paused, PipelineStatus::Shutdown) => {
                    match Runner::pipeline_http_request_json_response(self.pipeline_id, Method::GET, "shutdown", pipeline.port).await {
                        Err(e) => {
                            self.force_kill_pipeline(&pipeline, Some(e)).await?;
                        }
                        Ok((status, body)) => {
                            if status.is_success() {
                                pipeline.set_current_status(PipelineStatus::ShuttingDown, None);
                                self.update_pipeline_runtime_state(&pipeline).await?;
                            } else {
                                self.force_kill_pipeline(&pipeline, Some(Self::error_response_from_json(self.pipeline_id, status, &body))).await?;
                            }
                        }
                    }
                }
                (PipelineStatus::ShuttingDown, _) => {
                    if self.pipeline_process.map(|p| p.pipeline_process.try_wait().is_ok()).unwrap_or(true) {
                        self.pipeline_process = None;
                        pipeline.set_current_status(PipelineStatus::Shutdown, None);
                        self.update_pipeline_runtime_state(&pipeline).await?;
                    } else if Self::timeout_expired(pipeline.status_since, Self::SHUTDOWN_TIMEOUT) {
                        self.force_kill_pipeline(&pipeline, Some(RunnerError::PipelineShutdownTimeout {
                            pipeline_id: self.pipeline_id,
                            timeout: Self::SHUTDOWN_TIMEOUT,
                        })).await?;
                    } else {
                        poll_timeout = Self::SHUTDOWN_POLL_PERIOD;
                    }
                }

                (PipelineStatus::Running, _) |
                (PipelineStatus::Paused, _) => {
                    match Runner::pipeline_http_request_json_response(self.pipeline_id, Method::GET, "stats", pipeline.port).await {
                        Err(e) => {
                            self.force_kill_pipeline(&pipeline, Some(e)).await?;
                        }
                        Ok((status, body)) => {
                            if !status.is_success() {
                                self.force_kill_pipeline(&pipeline, Some(Self::error_response_from_json(self.pipeline_id, status, &body))).await?;
                            } else {
                                let state = body.get("global_metrics").get("state").as_str();
                                if state == "Paused" {
                                    pipeline.set_current_status(PipelineStatus::Paused, None);
                                    self.update_pipeline_runtime_state(&pipeline).await?;
                                } else if state == "Running" {
                                    pipeline.set_current_status(PipelineStatus::Running, None);
                                    self.update_pipeline_runtime_state(&pipeline).await?;
                                } else {
                                    self.force_kill_pipeline(&pipeline, Some(RunnerError::HttpForwardError {
                                        pipeline_id: self.pipeline_id,
                                        error: format!("Pipeline reported unexpected status '{state}', expected 'Paused' or 'Running'")
                                    })).await?;
                                }
                            }
                        }
                    }
                }
                (PipelineStatus::Failed, PipelineStatus::Shutdown) => {
                    pipeline.set_current_status(PipelineStatus::Shutdown, pipeline.error);
                    self.update_pipeline_runtime_state(&pipeline).await?;
                }
                (PipelineStatus::Failed, _) => { }
            }
        }
    }

    async fn update_pipeline_runtime_state(
        &self,
        state: &PipelineRuntimeState,
    ) -> Result<(), DBError> {
        self.db.lock().await.update_pipeline_runtime_state(self.pipeline_id, state).await
    }

    fn timeout_expired(since: DateTime<Utc>, timeout: Duration) -> bool {
        Utc::now().timestamp_millis() - since.timestamp_millis() > timeout.as_millis() as i64
    }
 
    async fn force_kill_pipeline<E>(&mut self, mut pipeline: &PipelineRuntimeState, error: Option<E>) -> Result<(), DBError>
    where
        ErrorResponse: for<'a> From<&'a E>,
    {
        self.pipeline_process = None;

        if pipeline.desired_status == PipelineStatus::Shutdown {
            pipeline.set_current_status(PipelineStatus::Shutdown, error);
        } else {
            pipeline.set_current_status(PipelineStatus::Failed, error);
        }
        self.update_pipeline_runtime_state(pipeline).await
    }

    async fn read_pipeline_port_file(&self) -> Result<Option<u16>, ManagerError>
    {
        let port_file_path = self.config.port_file_path(self.pipeline_id);

        match fs::read_to_string(port_file_path).await {
            Ok(port) => {
                let parse = port.trim().parse::<u16>();
                match parse {
                    Ok(port) => {
                        Ok(Some(port))
                    }
                    Err(e) => Err(ManagerError::from(RunnerError::PortFileParseError {
                        pipeline_id: self.pipeline_id,
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
            ErrorResponse::from(&RunnerError::HttpForwardError {
                pipeline_id,
                error: format!("Pipeline returned HTTP status {status}, response body:{json:#}"),
            })
        })
    }
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
    config: Arc<ManagerConfig>,
    pipelines: Mutex<BTreeMap<PipelineId, Arc<Notify>>>,
}


impl LocalRunner {
    pub(crate) fn new(
        db: Arc<Mutex<ProjectDB>>,
        config: &ManagerConfig,
    ) -> Self {
        Self {
            db,
            config: Arc::new(config.clone()),
            pipelines: Mutex::new(BTreeMap::new()),
        }
    }

    async fn notify_pipeline_automaton(&self, tenant_id: TenantId, pipeline_id: PipelineId) {
        self.pipelines.lock().await.entry(pipeline_id).or_insert_with(|| {
            let notifier = Arc::new(Notify::new());
            spawn(PipelineAutomaton::new(pipeline_id, tenant_id, &self.config, self.db.clone(), notifier.clone()).run());
            notifier
        }).notify_one();
    }
}
