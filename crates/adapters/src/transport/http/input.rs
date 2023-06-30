use crate::{
    server::PipelineError, ControllerError, InputConsumer, InputEndpoint, PipelineState,
    TransportConfig,
};
use actix::Message;
use actix_web::{web::Payload, HttpResponse};
use anyhow::{anyhow, Error as AnyError, Result as AnyResult};
use futures_util::StreamExt;
use log::debug;
use num_traits::FromPrimitive;
use serde::Deserialize;
use serde_yaml::Value as YamlValue;
use std::{
    borrow::Cow,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::{sync::watch, time::timeout};

#[derive(Clone, Debug, Deserialize)]
pub(crate) enum HttpIngressMode {
    Batch,
    Stream,
    Chunks,
}

/// HTTP input transport.
///
/// HTTP endpoints are instantiated via the REST API, so this type doesn't
/// implement `trait InputTransport`.  It is only used to
/// collect static functions related to HTTP.
pub(crate) struct HttpInputTransport;

impl HttpInputTransport {
    /// Default data format assumed by API endpoints when not explicit "?format="
    /// argument provided.
    // TODO: json is a better default, once we support it.
    pub(crate) fn default_format() -> String {
        String::from("csv")
    }

    pub(crate) fn default_max_buffered_records() -> u64 {
        100_000
    }

    // pub(crate) fn default_mode() -> HttpIngressMode {
    //    HttpIngressMode::Stream
    // }

    pub(crate) fn config() -> TransportConfig {
        TransportConfig {
            name: Cow::from("api"),
            config: YamlValue::Null,
        }
    }
}

struct HttpInputEndpointInner {
    name: String,
    state: AtomicU32,
    status_notifier: watch::Sender<()>,
    consumer: Mutex<Option<Box<dyn InputConsumer>>>,
}

impl HttpInputEndpointInner {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            state: AtomicU32::new(PipelineState::Paused as u32),
            status_notifier: watch::channel(()).0,
            consumer: Mutex::new(None),
        }
    }
}

#[derive(Message)]
#[rtype(result = "()")]
struct EndpointStateNotification;

/// Input endpoint that streams input data via HTTP.
#[derive(Clone)]
pub(crate) struct HttpInputEndpoint {
    inner: Arc<HttpInputEndpointInner>,
}

impl HttpInputEndpoint {
    pub(crate) fn new(name: &str) -> Self {
        Self {
            inner: Arc::new(HttpInputEndpointInner::new(name)),
        }
    }

    fn state(&self) -> PipelineState {
        PipelineState::from_u32(self.inner.state.load(Ordering::Acquire)).unwrap()
    }

    fn name(&self) -> &str {
        &self.inner.name
    }

    fn notify(&self) {
        self.inner.status_notifier.send_replace(());
    }

    fn push_bytes(&self, bytes: &[u8]) -> AnyResult<()> {
        self.inner
            .consumer
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .input(bytes)
    }

    fn eoi(&self) -> AnyResult<()> {
        self.inner.consumer.lock().unwrap().as_mut().unwrap().eoi()
    }

    fn error(&self, fatal: bool, error: AnyError) {
        self.inner
            .consumer
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .error(fatal, error);
    }

    /// Read the `payload` stream and push it to the pipeline.
    ///
    /// Returns on reaching the end of the `payload` stream
    /// (if any) or when the pipeline terminates.
    pub(crate) async fn complete_request(
        &self,
        mut payload: Payload,
    ) -> Result<HttpResponse, PipelineError> {
        debug!("HTTP input endpoint '{}': start of request", self.name());

        let mut num_bytes = 0;
        let mut status_watch = self.inner.status_notifier.subscribe();
        loop {
            match self.state() {
                PipelineState::Paused => {
                    let _ = status_watch.changed().await;
                }
                PipelineState::Terminated => {
                    return Err(PipelineError::Terminating);
                }
                PipelineState::Running => {
                    // Check pipeline status at least every second.
                    match timeout(Duration::from_millis(1_000), payload.next()).await {
                        Err(_elapsed) => (),
                        Ok(Some(Ok(bytes))) => {
                            num_bytes += bytes.len();
                            if let Err(e) = self.push_bytes(&bytes) {
                                Err(ControllerError::parse_error(self.name(), e))?
                            }
                        }
                        Ok(Some(Err(e))) => {
                            self.error(true, anyhow!(e.to_string()));
                            Err(ControllerError::input_transport_error(
                                self.name(),
                                true,
                                anyhow!(e),
                            ))?
                        }
                        Ok(None) => {
                            if let Err(e) = self.eoi() {
                                Err(ControllerError::parse_error(self.name(), e))?
                            }
                            break;
                        }
                    }
                }
            }
        }

        debug!(
            "HTTP input endpoint '{}': end of request, {num_bytes} received",
            self.name()
        );
        Ok(HttpResponse::Ok().finish())
    }
}

impl InputEndpoint for HttpInputEndpoint {
    fn connect(&mut self, consumer: Box<dyn InputConsumer>) -> AnyResult<()> {
        *self.inner.consumer.lock().unwrap() = Some(consumer);
        Ok(())
    }

    fn pause(&self) -> AnyResult<()> {
        self.inner
            .state
            .store(PipelineState::Paused as u32, Ordering::Release);
        self.notify();

        Ok(())
    }

    fn start(&self) -> AnyResult<()> {
        self.inner
            .state
            .store(PipelineState::Running as u32, Ordering::Release);
        self.notify();

        Ok(())
    }

    fn disconnect(&self) {
        self.inner
            .state
            .store(PipelineState::Terminated as u32, Ordering::Release);
        self.notify();
    }
}
