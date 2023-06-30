//! An I/O controller that coordinates the creation, reconfiguration,
//! teardown of input/output adapters, and implements runtime flow control.
//!
//! # Design
//!
//! The controller logic is split into two tasks that run in separate threads.
//! The circuit thread owns the `DBSPHandle` and calls `step()` on it whenever
//! there is some input data available for the circuit.  It can be configured
//! to improve batching by slightly delaying the `step()` call if the number of
//! available input records is below some used-defined threshold.
//!
//! The backpressure thread controls the flow of data through transport
//! endpoints, pausing the endpoints either when the amount of data buffered by
//! the endpoint exceeds a user-defined threshold or in response to an explicit
//! user request.
//!
//! Both tasks require monitoring the state of the input buffers.  To this end,
//! the controller injects `InputProbe`s between each input endpoint and format
//! parser:
//!
//! ```text
//!                ┌────────────┐
//!                │ controller │
//!                └────────────┘
//!                      ▲
//!                      │stats
//!   ┌────────┐   ┌─────┴────┐   ┌──────┐   ┌──────┐
//!   │endpoint├──►│InputProbe├──►│parser├──►│handle│
//!   └────────┘   └──────────┘   └──────┘   └──────┘
//! ```
//!
//! The probe passes the data through to the parser, while counting the number
//! of transmitted bytes and records and updating respective performance
//! counters in the controller.

use crate::{
    Catalog, Encoder, InputConsumer, InputEndpoint, InputFormat, InputTransport, OutputConsumer,
    OutputEndpoint, OutputFormat, OutputQuery, OutputQueryHandles, OutputTransport, Parser,
    PipelineState, SerBatch,
};
use anyhow::{anyhow, Error as AnyError, Result as AnyResult};
use crossbeam::{
    queue::SegQueue,
    sync::{Parker, ShardedLock, Unparker},
};
use dbsp::DBSPHandle;
use erased_serde::Deserializer as ErasedDeserializer;
use log::{debug, error, info};
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    thread::{spawn, JoinHandle},
    time::{Duration, Instant},
};

mod config;
mod error;
mod stats;

pub use config::{
    FormatConfig, GlobalPipelineConfig, InputEndpointConfig, OutputEndpointConfig, PipelineConfig,
    TransportConfig,
};
pub use error::{ConfigError, ControllerError};
pub use stats::{ControllerStatus, InputEndpointStatus, OutputEndpointStatus};

/// Maximal number of concurrent API connections per circuit
/// (including both input and output connecions).
// TODO: make this configurable.
const MAX_API_CONNECTIONS: u64 = 100;

pub(crate) type EndpointId = u64;

/// Controller that coordinates the creation, reconfiguration, teardown of
/// input/output adapters, and implements runtime flow control.
///
/// The controller instantiates the input and output pipelines according to a
/// user-provided [configuration](`PipelineConfig`) and exposes an API to
/// reconfigure and monitor the pipelines at runtime.
pub struct Controller {
    inner: Arc<ControllerInner>,

    /// The circuit thread handle (see module-level docs).
    circuit_thread_handle: JoinHandle<Result<(), ControllerError>>,

    /// The backpressure thread handle (see module-level docs).
    backpressure_thread_handle: JoinHandle<()>,
}

impl Controller {
    /// Create a new I/O controller for a circuit.
    ///
    /// Creates a new instance of `Controller` that wraps `circuit`,  with
    /// input and output endpoints specified by `config`.  The controller is
    /// created with all endpoints in a paused state.  Call [`Self::start`]
    /// to unpause the endpoints and start ingesting data.
    ///
    /// # Arguments
    ///
    /// * `circuit` - A handle to a DBSP circuit managed by this controller. The
    ///   controller takes ownership of the circuit.
    ///
    /// * `catalog` - A catalog of input and output streams of the circuit.
    ///
    /// * `config` - Controller configuration, including global config settings
    ///   and individual endpoint configs.
    ///
    /// * `error_cb` - Error callback.  The controller doesn't implement its own
    ///   error handling policy, but simply forwards most errors to this
    ///   callback.
    ///
    /// # Errors
    ///
    /// The method may fail for the following reasons:
    ///
    /// * The input configuration is invalid, e.g., specifies an unknown
    ///   transport or data format.
    ///
    /// * One or more of the endpoints fails to initialize.
    pub fn with_config(
        mut circuit: DBSPHandle,
        catalog: Catalog,
        config: &PipelineConfig,
        error_cb: Box<dyn Fn(ControllerError) + Send + Sync>,
    ) -> Result<Self, ControllerError> {
        let circuit_thread_parker = Parker::new();
        let circuit_thread_unparker = circuit_thread_parker.unparker().clone();

        let backpressure_thread_parker = Parker::new();
        let backpressure_thread_unparker = backpressure_thread_parker.unparker().clone();

        let inner = Arc::new(ControllerInner::new(
            catalog,
            &config.global,
            circuit_thread_unparker,
            backpressure_thread_unparker,
            error_cb,
        ));

        if config.global.cpu_profiler {
            circuit
                .enable_cpu_profiler()
                .map_err(ControllerError::dbsp_error)?;
        }

        let backpressure_thread_handle = {
            let inner = inner.clone();
            spawn(move || Self::backpressure_thread(inner, backpressure_thread_parker))
        };

        let circuit_thread_handle = {
            let inner = inner.clone();
            spawn(move || Self::circuit_thread(circuit, inner, circuit_thread_parker))
        };

        for (input_name, input_config) in config.inputs.iter() {
            inner.connect_input(input_name, input_config)?;
        }

        for (output_name, output_config) in config.outputs.iter() {
            inner.connect_output(output_name, output_config)?;
        }

        Ok(Self {
            inner,
            circuit_thread_handle,
            backpressure_thread_handle,
        })
    }

    /// Connect a new input endpoint with specified name and configuration.
    ///
    /// Creates an endpoint with data transport and format specified by
    /// `config` and starts streaming data from the endpoint if the pipeline
    /// is running.
    ///
    /// # Errors
    ///
    /// The method may fail for the following reasons:
    ///
    /// * The endpoint configuration is invalid, e.g., specifies an unknown
    ///   transport or data format.
    ///
    /// * The endpoint fails to initialize, e.g., because the network address or
    ///   filename specified in the transport config is unreachable.
    pub fn connect_input(
        &self,
        endpoint_name: &str,
        config: &InputEndpointConfig,
    ) -> Result<EndpointId, ControllerError> {
        self.inner.connect_input(endpoint_name, config)
    }

    /// Disconnect an existing input endpoint.
    ///
    /// This method is asynchronous and may return before all endpoint
    /// threads have terminated.
    pub fn disconnect_input(&self, endpoint_id: &EndpointId) {
        self.inner.disconnect_input(endpoint_id)
    }

    /// Connect a previously instantiated input endpoint.
    ///
    /// Used to connect an endpoint instantiated manually rather than from an
    /// [`InputEndpointConfig`].
    ///
    /// # Arguments
    ///
    /// * `endpoint_name` - endpoint name unique within the pipeline.
    ///
    /// * `endpoint_config` - (partial) endpoint config.  Only `format.name`
    ///   and `stream` fields need to be initialized.
    ///
    /// * `format_config` - a deserializer used to extract format-specific
    ///    configuration.
    ///
    /// * `endpoint` - transport endpoint object.
    pub fn add_input_endpoint(
        &self,
        endpoint_name: &str,
        endpoint_config: InputEndpointConfig,
        format_config: &mut dyn ErasedDeserializer,
        endpoint: Box<dyn InputEndpoint>,
    ) -> Result<EndpointId, ControllerError> {
        self.inner
            .add_input_endpoint(endpoint_name, endpoint_config, format_config, endpoint)
    }

    /// Disconnect an existing output endpoint.
    ///
    /// This method is asynchronous and may return before all endpoint
    /// threads have terminated.
    pub fn disconnect_output(&self, endpoint_id: &EndpointId) {
        self.inner.disconnect_output(endpoint_id)
    }

    /// Connect a previously instantiated output endpoint.
    ///
    /// Used to connect an endpoint instantiated manually rather than from an
    /// [`OutputEndpointConfig`].
    ///
    /// # Arguments
    ///
    /// * `endpoint_name` - endpoint name unique within the pipeline.
    ///
    /// * `endpoint_config` - (partial) endpoint config.  Only `format.name`
    ///   and `stream` fields need to be initialized.
    ///
    /// * `format_config` - a deserializer used to extract format-specific
    ///    configuration.
    ///
    /// * `endpoint` - transport endpoint object.
    pub fn add_output_endpoint(
        &self,
        endpoint_name: &str,
        endpoint_config: &OutputEndpointConfig,
        format_config: &mut dyn ErasedDeserializer,
        endpoint: Box<dyn OutputEndpoint>,
    ) -> Result<EndpointId, ControllerError> {
        self.inner
            .add_output_endpoint(endpoint_name, endpoint_config, format_config, endpoint)
    }

    /// Increment the nubmber of active API connections.
    ///
    /// API connections are created dynamically via the `ingress` and `egress`
    /// REST API endpoints.
    ///
    /// Fails if the number of connections exceeds the current limit,
    /// returning the number of existing API connections.
    pub fn register_api_connection(&self) -> Result<(), u64> {
        self.inner.register_api_connection()
    }

    /// Decrement the nubmber of active API connections.
    pub fn unregister_api_connection(&self) {
        self.inner.unregister_api_connection();
    }

    /// Return the number of active API connections.
    pub fn num_api_connections(&self) -> u64 {
        self.inner.num_api_connections()
    }

    /// Force the circuit to perform a step even if all of its
    /// input buffers are empty or nearly empty.
    ///
    /// Can be used in two situations:
    ///
    /// * To process scalar inputs, such as neighborhood or quantile querues that
    ///   are not reflected in input buffer sizes.
    ///
    /// * When input buffers are not completely empty, but contain fewer than
    ///   `min_batch_size_records` records the circuit will normally wait for a small
    ///   timeout before processing the data.  This function can be used to force
    ///   immediate processing.
    pub fn request_step(&self) {
        self.inner.request_step();
    }

    /// Change the state of all input endpoints to running.
    ///
    /// Start streaming data through all connected input endpoints.
    pub fn start(&self) {
        self.inner.start();
    }

    /// Pause all input endpoints.
    ///
    /// Sends a pause command to all input endpoints.  Upon receiving the
    /// command, the endpoints must stop pushing data to the pipeline.  This
    /// method is asynchronous and may return before all endpoints have been
    /// fully paused.
    pub fn pause(&self) {
        self.inner.pause();
    }

    /// Returns controller status.
    pub fn status(&self) -> &ControllerStatus {
        // Update pipeline metrics computed on-demand.
        self.inner.status.update();
        &self.inner.status
    }

    pub fn catalog(&self) -> &Arc<Mutex<Catalog>> {
        &self.inner.catalog
    }

    pub fn dump_profile(&self) {
        self.inner.dump_profile();
    }

    /// Terminate the controller, stop all input endpoints and destroy the
    /// circuit.
    pub fn stop(self) -> Result<(), ControllerError> {
        self.inner.stop();
        self.circuit_thread_handle
            .join()
            .map_err(|_| ControllerError::controller_panic())??;
        self.backpressure_thread_handle
            .join()
            .map_err(|_| ControllerError::controller_panic())?;
        Ok(())
    }

    /// Check whether the pipeline has processed all input data to completion.
    ///
    /// Returns `true` when the following conditions are satisfied:
    ///
    /// * All input endpoints have signalled end-of-input.
    /// * All input records received from all endpoints have been processed by
    ///   the circuit.
    /// * All output records have been sent to respective output transport
    ///   endponts.
    ///
    /// Note that, depending on the type and configuration of the output
    /// transport, this may not guarantee that all output records have been
    /// written to a persistent storage or delivered to the recipient.
    pub fn pipeline_complete(&self) -> bool {
        self.inner.status.pipeline_complete()
    }

    /// Circuit thread function: holds the handle to the circuit, calls `step`
    /// on it whenever input data is available, pushes output batches
    /// produced by the circuit to output pipelines.
    fn circuit_thread(
        mut circuit: DBSPHandle,
        controller: Arc<ControllerInner>,
        parker: Parker,
    ) -> Result<(), ControllerError> {
        let mut start: Option<Instant> = None;

        let max_buffering_delay =
            Duration::from_micros(controller.status.global_config.max_buffering_delay_usecs);
        let min_batch_size_records = controller.status.global_config.min_batch_size_records;

        loop {
            let dump_profile = controller
                .dump_profile_request
                .swap(false, Ordering::AcqRel);
            if dump_profile {
                // Generate profile in the current working directory.
                // TODO: make this location configurable.
                match circuit.dump_profile("profile") {
                    Ok(path) => {
                        info!(
                            "circuit profile dump created in '{}'",
                            path.canonicalize().unwrap_or_default().display()
                        );
                    }
                    Err(e) => {
                        error!("failed to dump circuit profile: {e}");
                    }
                }
            }
            match controller.state() {
                PipelineState::Running | PipelineState::Paused => {
                    // Backpressure in the output pipeline: wait for room in output buffers to
                    // become available.
                    if controller.output_buffers_full() {
                        debug!("circuit thread: park waiting for output buffer space");
                        parker.park();
                        debug!("circuit thread: unparked");
                        continue;
                    }

                    let buffered_records = controller.status.num_buffered_input_records();

                    // We have sufficient buffered inputs or the buffering delay has expired or
                    // the client explicitly requested the circuit to run -- kick the circuit to
                    // consume buffered data.
                    // Use strict inequality in case `min_batch_size_records` is 0.
                    if controller.status.step_requested()
                        || buffered_records > min_batch_size_records
                        || start
                            .map(|start| start.elapsed() >= max_buffering_delay)
                            .unwrap_or(false)
                    {
                        start = None;
                        // Reset all counters of buffered records and bytes to 0.
                        controller.status.consume_buffered_inputs();

                        // All input records accumulated so far (and possibly some more) will
                        // be fully processed after the `step()` call returns.
                        let processed_records = controller.status.num_total_input_records();

                        // Wake up the backpressure thread to unpause endpoints blocked due to
                        // backpressure.
                        controller.unpark_backpressure();
                        debug!("circuit thread: calling 'circuit.step'");
                        circuit
                            .step()
                            .unwrap_or_else(|e| controller.error(ControllerError::dbsp_error(e)));
                        debug!("circuit thread: 'circuit.step' returned");

                        controller
                            .status
                            .set_num_total_processed_records(processed_records);

                        // Push output batches to output pipelines.
                        let outputs = controller.outputs.read().unwrap();
                        for ((_stream, _query), (output_handles, endpoints)) in
                            outputs.iter_by_stream()
                        {
                            // TODO: add an endpoint config option to consolidate output batches.

                            let mut delta_batch = output_handles
                                .delta
                                .as_ref()
                                .map(|handle| handle.take_from_all());
                            let num_delta_records = delta_batch
                                .as_ref()
                                .map(|batch| batch.iter().map(|b| b.len()).sum());

                            let mut snapshot_batch = output_handles
                                .snapshot
                                .as_ref()
                                .map(|handle| handle.take_from_all());
                            let num_snapshot_records = snapshot_batch
                                .as_ref()
                                .map(|batch| batch.iter().map(|b| b.len()).sum());

                            for (i, endpoint_id) in endpoints.iter().enumerate() {
                                let endpoint = outputs.lookup_by_id(endpoint_id).unwrap();

                                // If the endpoint has a snapshot stream associated with it,
                                // then the first output sent to this endpoint must be
                                // the snapshot.  Subsequent outputs are deltas on top of
                                // the snapshot.
                                if !endpoint.snapshot_sent.load(Ordering::Acquire)
                                    && output_handles.snapshot.is_some()
                                {
                                    if snapshot_batch.is_some() {
                                        // Increment stats first, so we don't end up with negative counts.
                                        controller.status.enqueue_batch(
                                            *endpoint_id,
                                            num_snapshot_records.unwrap(),
                                        );

                                        // Send the batch without cloning to the last consumer.
                                        let batch = if i == endpoints.len() - 1 {
                                            snapshot_batch.take().unwrap()
                                        } else {
                                            snapshot_batch.as_ref().unwrap().clone()
                                        };

                                        // Associate the input frontier with the batch.  Once the batch has
                                        // been sent to the output endpoint, the endpoint will get labeled
                                        // with this frontier.
                                        endpoint.queue.push((batch, processed_records));
                                        endpoint.snapshot_sent.store(true, Ordering::Release);
                                    }
                                } else if delta_batch.is_some() {
                                    controller
                                        .status
                                        .enqueue_batch(*endpoint_id, num_delta_records.unwrap());

                                    let batch = if i == endpoints.len() - 1 {
                                        delta_batch.take().unwrap()
                                    } else {
                                        delta_batch.as_ref().unwrap().clone()
                                    };

                                    endpoint.queue.push((batch, processed_records));
                                }

                                // Wake up the output thread.  We're not trying to be smart here and
                                // wake up the thread conditionally if it was previously idle, as I
                                // don't expect this to make any real difference.
                                endpoint.unparker.unpark();
                            }
                        }
                    } else if buffered_records > 0 {
                        // We have some buffered data, but less than `min_batch_size_records` --
                        // wait up to `max_buffering_delay` for more data to
                        // arrive.
                        if start.is_none() {
                            start = Some(Instant::now());
                        }
                        parker.park_timeout(Duration::from_millis(1));
                    } else {
                        debug!("circuit thread: park: input buffers empty");
                        parker.park();
                        debug!("circuit thread: unparked");
                    }
                }
                PipelineState::Terminated => {
                    circuit.kill().map_err(|_| ControllerError::dbsp_panic())?;
                    return Ok(());
                }
            }
        }
    }

    /// Backpressure thread function.
    fn backpressure_thread(controller: Arc<ControllerInner>, parker: Parker) {
        // `global_pause` flag is `true` when the entire controller is paused
        // (the controller starts in a paused state and switches between
        // paused and running states in responsed to `Controller::start()` and
        // `Controller::pause()` methods).
        let mut global_pause = true;

        // Endpoints paused due to backpressure.
        let mut paused_endpoints = HashSet::new();

        loop {
            let inputs = controller.inputs.lock().unwrap();

            match controller.state() {
                PipelineState::Paused => {
                    // Pause circuit if not yet paused.
                    if !global_pause {
                        for (epid, ep) in inputs.iter() {
                            // Pause the endpoint unless it's already paused due to backpressure.
                            if !paused_endpoints.contains(epid) {
                                ep.endpoint.pause().unwrap_or_else(|e| {
                                    controller.input_transport_error(
                                        *epid,
                                        &ep.endpoint_name,
                                        true,
                                        e,
                                    )
                                });
                            }
                        }
                    }
                    global_pause = true;
                }
                PipelineState::Running => {
                    // Resume endpoints that have buffer space, pause endpoints with full buffers.
                    for (epid, ep) in inputs.iter() {
                        if controller.status.input_endpoint_full(epid) {
                            // The endpoint is full and is not yet in the paused state -- pause it
                            // now.
                            if !global_pause && !paused_endpoints.contains(epid) {
                                ep.endpoint.pause().unwrap_or_else(|e| {
                                    controller.input_transport_error(
                                        *epid,
                                        &ep.endpoint_name,
                                        true,
                                        e,
                                    )
                                });
                            }
                            paused_endpoints.insert(*epid);
                        } else {
                            // The endpoint is paused when it should be running -- unpause it.
                            if global_pause || paused_endpoints.contains(epid) {
                                ep.endpoint.start().unwrap_or_else(|e| {
                                    controller.input_transport_error(
                                        *epid,
                                        &ep.endpoint_name,
                                        true,
                                        e,
                                    )
                                });
                            }
                            paused_endpoints.remove(epid);
                        }
                    }
                    global_pause = false;
                }
                PipelineState::Terminated => return,
            }

            drop(inputs);

            parker.park();
        }
    }
}

/// State tracked by the controller for each input endpoint.
struct InputEndpointDescr {
    endpoint_name: String,
    endpoint: Box<dyn InputEndpoint>,
}

impl InputEndpointDescr {
    pub fn new(endpoint_name: &str, endpoint: Box<dyn InputEndpoint>) -> Self {
        Self {
            endpoint_name: endpoint_name.to_owned(),
            endpoint,
        }
    }
}

/// A lock-free queue used to send output batches from the circuit thread
/// to output endpoint threads.  Each entry is annotated with a progress label
/// that is equal to the number of input records fully processed by
/// DBSP before emitting this batch of outputs.  The label increases
/// monotonically over time.
type BatchQueue = SegQueue<(Vec<Arc<dyn SerBatch>>, u64)>;

/// State tracked by the controller for each output endpoint.
struct OutputEndpointDescr {
    /// Endpoint name.
    endpoint_name: String,

    /// Stream name that the endpoint is connected to.
    stream_name: String,

    /// Query associated with the endpoint.
    query: OutputQuery,

    /// FIFO queue of batches read from the stream.
    queue: Arc<BatchQueue>,

    /// True if the endpoint has already received a complete snapshot
    /// of the query result.
    snapshot_sent: AtomicBool,

    /// Used to notify the endpoint thread that the endpoint is being disconnected.
    disconnect_flag: Arc<AtomicBool>,

    /// Unparker for the endpoint thread.
    unparker: Unparker,
}

impl OutputEndpointDescr {
    pub fn new(
        endpoint_name: &str,
        stream_name: &str,
        query: OutputQuery,
        unparker: Unparker,
    ) -> Self {
        Self {
            endpoint_name: endpoint_name.to_string(),
            stream_name: stream_name.to_string(),
            query,
            queue: Arc::new(SegQueue::new()),
            snapshot_sent: AtomicBool::new(false),
            disconnect_flag: Arc::new(AtomicBool::new(false)),
            unparker,
        }
    }
}

type StreamEndpointMap =
    BTreeMap<(String, OutputQuery), (OutputQueryHandles, BTreeSet<EndpointId>)>;

struct OutputEndpoints {
    by_id: BTreeMap<EndpointId, OutputEndpointDescr>,
    by_stream: StreamEndpointMap,
}

impl OutputEndpoints {
    fn new() -> Self {
        Self {
            by_id: BTreeMap::new(),
            by_stream: BTreeMap::new(),
        }
    }

    fn iter_by_stream(
        &self,
    ) -> impl Iterator<
        Item = (
            &'_ (String, OutputQuery),
            &'_ (OutputQueryHandles, BTreeSet<EndpointId>),
        ),
    > {
        self.by_stream.iter()
    }

    fn lookup_by_id(&self, endpoint_id: &EndpointId) -> Option<&OutputEndpointDescr> {
        self.by_id.get(endpoint_id)
    }

    fn lookup_by_name(&self, endpoint_name: &str) -> Option<&OutputEndpointDescr> {
        self.by_id
            .values()
            .find(|ep| ep.endpoint_name == endpoint_name)
    }

    fn alloc_endpoint_id(&self) -> EndpointId {
        self.by_id.keys().rev().next().map(|k| k + 1).unwrap_or(0)
    }

    fn insert(
        &mut self,
        endpoint_id: EndpointId,
        handles: OutputQueryHandles,
        endpoint_descr: OutputEndpointDescr,
    ) {
        self.by_stream
            .entry((endpoint_descr.stream_name.clone(), endpoint_descr.query))
            .or_insert_with(|| (handles, BTreeSet::new()))
            .1
            .insert(endpoint_id);
        self.by_id.insert(endpoint_id, endpoint_descr);
    }

    fn remove(&mut self, endpoint_id: &EndpointId) -> Option<OutputEndpointDescr> {
        self.by_id.remove(endpoint_id).map(|descr| {
            self.by_stream
                .get_mut(&(descr.stream_name.clone(), descr.query))
                .map(|(_, endpoints)| endpoints.remove(endpoint_id));
            descr
        })
    }
}

/// Controller state sharable across threads.
///
/// A reference to this struct is held by each input probe and by both
/// controller threads.
struct ControllerInner {
    status: Arc<ControllerStatus>,
    num_api_connections: AtomicU64,
    dump_profile_request: AtomicBool,
    catalog: Arc<Mutex<Catalog>>,
    inputs: Mutex<BTreeMap<EndpointId, InputEndpointDescr>>,
    outputs: ShardedLock<OutputEndpoints>,
    circuit_thread_unparker: Unparker,
    backpressure_thread_unparker: Unparker,
    error_cb: Box<dyn Fn(ControllerError) + Send + Sync>,
}

impl ControllerInner {
    fn new(
        catalog: Catalog,
        global_config: &GlobalPipelineConfig,
        circuit_thread_unparker: Unparker,
        backpressure_thread_unparker: Unparker,
        error_cb: Box<dyn Fn(ControllerError) + Send + Sync>,
    ) -> Self {
        let status = Arc::new(ControllerStatus::new(global_config));
        let dump_profile_request = AtomicBool::new(false);

        Self {
            status,
            num_api_connections: AtomicU64::new(0),
            dump_profile_request,
            catalog: Arc::new(Mutex::new(catalog)),
            inputs: Mutex::new(BTreeMap::new()),
            outputs: ShardedLock::new(OutputEndpoints::new()),
            circuit_thread_unparker,
            backpressure_thread_unparker,
            error_cb,
        }
    }

    fn connect_input(
        self: &Arc<Self>,
        endpoint_name: &str,
        endpoint_config: &InputEndpointConfig,
    ) -> Result<EndpointId, ControllerError> {
        // Create transport endpoint.
        let transport = <dyn InputTransport>::get_transport(&endpoint_config.transport.name)
            .ok_or_else(|| {
                ControllerError::unknown_input_transport(&endpoint_config.transport.name)
            })?;

        let endpoint = transport
            .new_endpoint(endpoint_name, &endpoint_config.transport.config)
            .map_err(|e| ControllerError::input_transport_error(endpoint_name, true, e))?;

        self.add_input_endpoint(
            endpoint_name,
            endpoint_config.clone(),
            &mut <dyn ErasedDeserializer>::erase(&endpoint_config.format.config),
            endpoint,
        )
    }

    fn disconnect_input(self: &Arc<Self>, endpoint_id: &EndpointId) {
        let mut inputs = self.inputs.lock().unwrap();

        if let Some(ep) = inputs.remove(endpoint_id) {
            ep.endpoint.disconnect();
            self.status.remove_input(endpoint_id);
            self.unpark_circuit();
            self.unpark_backpressure();
        }
    }

    fn add_input_endpoint(
        self: &Arc<Self>,
        endpoint_name: &str,
        endpoint_config: InputEndpointConfig,
        format_config: &mut dyn ErasedDeserializer,
        mut endpoint: Box<dyn InputEndpoint>,
    ) -> Result<EndpointId, ControllerError> {
        let mut inputs = self.inputs.lock().unwrap();

        if inputs.values().any(|ep| ep.endpoint_name == endpoint_name) {
            Err(ControllerError::duplicate_input_endpoint(endpoint_name))?;
        }

        // Create input pipeline, consisting of a transport endpoint, controller
        // probe, and parser.
        //
        // ┌────────┐   ┌──────────┐   ┌──────┐
        // │endpoint├──►│InputProbe├──►│parser├──►
        // └────────┘   └──────────┘   └──────┘

        let catalog = self.catalog.lock().unwrap();
        let input_stream = catalog
            .input_collection_handle(&endpoint_config.stream)
            .ok_or_else(|| ControllerError::unknown_input_stream(&endpoint_config.stream))?;

        // Create parser.
        let format = <dyn InputFormat>::get_format(&endpoint_config.format.name)
            .ok_or_else(|| ControllerError::unknown_input_format(&endpoint_config.format.name))?;

        let parser = format
            .new_parser(input_stream, format_config)
            .map_err(|e| ControllerError::parse_error(endpoint_name, e))?;

        // Create probe.
        let endpoint_id = inputs.keys().rev().next().map(|k| k + 1).unwrap_or(0);
        let probe = Box::new(InputProbe::new(
            endpoint_id,
            endpoint_name,
            parser,
            self.clone(),
            self.circuit_thread_unparker.clone(),
            self.backpressure_thread_unparker.clone(),
        ));

        // Initialize endpoint stats.
        self.status
            .add_input(&endpoint_id, endpoint_name, endpoint_config);

        endpoint
            .connect(probe)
            .map_err(|e| ControllerError::input_transport_error(endpoint_name, true, e))?;
        if self.state() == PipelineState::Running {
            endpoint
                .start()
                .map_err(|e| ControllerError::input_transport_error(endpoint_name, true, e))?;
        }

        inputs.insert(
            endpoint_id,
            InputEndpointDescr::new(endpoint_name, endpoint),
        );

        drop(inputs);

        self.unpark_backpressure();
        Ok(endpoint_id)
    }

    fn register_api_connection(&self) -> Result<(), u64> {
        let num_connections = self.num_api_connections.load(Ordering::Acquire);

        if num_connections >= MAX_API_CONNECTIONS {
            Err(num_connections)
        } else {
            self.num_api_connections.fetch_add(1, Ordering::AcqRel);
            Ok(())
        }
    }

    fn unregister_api_connection(&self) {
        let old = self.num_api_connections.fetch_sub(1, Ordering::AcqRel);
        debug_assert!(old > 0);
    }

    fn num_api_connections(&self) -> u64 {
        self.num_api_connections.load(Ordering::Acquire)
    }

    pub fn request_step(&self) {
        self.status.request_step(&self.circuit_thread_unparker);
    }

    /// Unpark the circuit thread.
    fn unpark_circuit(&self) {
        self.circuit_thread_unparker.unpark();
    }

    /// Unpark the backpressure thread.
    fn unpark_backpressure(&self) {
        self.backpressure_thread_unparker.unpark();
    }

    fn connect_output(
        self: &Arc<Self>,
        endpoint_name: &str,
        endpoint_config: &OutputEndpointConfig,
    ) -> Result<EndpointId, ControllerError> {
        // Create transport endpoint.
        let transport = <dyn OutputTransport>::get_transport(&endpoint_config.transport.name)
            .ok_or_else(|| {
                ControllerError::unknown_output_transport(&endpoint_config.transport.name)
            })?;

        let endpoint = transport
            .new_endpoint(endpoint_name, endpoint_config)
            .map_err(|e| ControllerError::output_transport_error(endpoint_name, true, e))?;
        self.add_output_endpoint(
            endpoint_name,
            endpoint_config,
            &mut <dyn ErasedDeserializer>::erase(&endpoint_config.format.config),
            endpoint,
        )
    }

    fn disconnect_output(self: &Arc<Self>, endpoint_id: &EndpointId) {
        let mut outputs = self.outputs.write().unwrap();

        if let Some(ep) = outputs.remove(endpoint_id) {
            ep.disconnect_flag.store(true, Ordering::Release);
            ep.unparker.unpark();
            self.status.remove_output(endpoint_id);
        }
    }

    fn add_output_endpoint(
        self: &Arc<Self>,
        endpoint_name: &str,
        endpoint_config: &OutputEndpointConfig,
        format_config: &mut dyn ErasedDeserializer,
        endpoint: Box<dyn OutputEndpoint>,
    ) -> Result<EndpointId, ControllerError> {
        let mut outputs = self.outputs.write().unwrap();

        if outputs.lookup_by_name(endpoint_name).is_some() {
            Err(ControllerError::duplicate_output_endpoint(endpoint_name))?;
        }

        // Create output pipeline, consisting of an encoder, output probe and
        // transport endpoint; run the pipeline in a separate thread.
        //
        // ┌───────┐   ┌───────────┐   ┌────────┐
        // │encoder├──►│OutputProbe├──►│endpoint├──►
        // └───────┘   └───────────┘   └────────┘

        // Lookup output handle in catalog.
        let handles = self
            .catalog
            .lock()
            .unwrap()
            .output_query_handles(&endpoint_config.stream, endpoint_config.query)
            .ok_or_else(|| ControllerError::unknown_output_stream(&endpoint_config.stream))?;

        let endpoint_id = outputs.alloc_endpoint_id();
        let endpoint_name_str = endpoint_name.to_string();

        let self_weak = Arc::downgrade(self);
        endpoint
            .connect(Box::new(move |fatal: bool, e: AnyError| {
                if let Some(controller) = self_weak.upgrade() {
                    controller.output_transport_error(endpoint_id, &endpoint_name_str, fatal, e)
                }
            }))
            .map_err(|e| ControllerError::output_transport_error(endpoint_name, true, e))?;

        // Create probe.
        let probe = Box::new(OutputProbe::new(
            endpoint_id,
            endpoint_name,
            endpoint,
            self.clone(),
        ));

        // Create encoder.
        let format = <dyn OutputFormat>::get_format(&endpoint_config.format.name)
            .ok_or_else(|| ControllerError::unknown_output_format(&endpoint_config.format.name))?;
        let encoder = format
            .new_encoder(format_config, probe)
            .map_err(|e| ControllerError::encode_error(endpoint_name, e))?;

        let parker = Parker::new();
        let endpoint_descr = OutputEndpointDescr::new(
            endpoint_name,
            &endpoint_config.stream,
            endpoint_config.query,
            parker.unparker().clone(),
        );
        let queue = endpoint_descr.queue.clone();
        let disconnect_flag = endpoint_descr.disconnect_flag.clone();
        let controller = self.clone();

        outputs.insert(endpoint_id, handles, endpoint_descr);

        let endpoint_name_string = endpoint_name.to_string();
        // Thread to run the output pipeline.
        spawn(move || {
            Self::output_thread_func(
                endpoint_id,
                endpoint_name_string,
                encoder,
                parker,
                queue,
                disconnect_flag,
                controller,
            )
        });

        drop(outputs);

        // Initialize endpoint stats.
        self.status
            .add_output(&endpoint_id, endpoint_name, endpoint_config);

        Ok(endpoint_id)
    }

    fn output_thread_func(
        endpoint_id: EndpointId,
        endpoint_name: String,
        mut encoder: Box<dyn Encoder>,
        parker: Parker,
        queue: Arc<BatchQueue>,
        disconnect_flag: Arc<AtomicBool>,
        controller: Arc<ControllerInner>,
    ) {
        loop {
            if controller.state() == PipelineState::Terminated {
                return;
            }

            if disconnect_flag.load(Ordering::Acquire) {
                return;
            }

            // Dequeue the next output batch and push it to the encoder.
            if let Some((data, processed_records)) = queue.pop() {
                let num_records = data.iter().map(|b| b.len()).sum();

                encoder.consumer().batch_start();
                encoder
                    .encode(data.as_slice())
                    .unwrap_or_else(|e| controller.encode_error(endpoint_id, &endpoint_name, e));
                encoder.consumer().batch_end();

                // `num_records` output records have been transmitted --
                // update output stats, wake up the circuit thread if the
                // number of queued records drops below high water mark.
                controller.status.output_batch(
                    endpoint_id,
                    processed_records,
                    num_records,
                    &controller.circuit_thread_unparker,
                );
            } else {
                // Queue is empty -- wait for the circuit thread to wake us up when
                // more data is available.
                parker.park();
            }
        }
    }

    fn state(self: &Arc<Self>) -> PipelineState {
        self.status.state()
    }

    fn start(self: &Arc<Self>) {
        self.status.set_state(PipelineState::Running);
        self.unpark_backpressure();
    }

    fn pause(self: &Arc<Self>) {
        self.status.set_state(PipelineState::Paused);
        self.unpark_backpressure();
    }

    fn stop(self: &Arc<Self>) {
        let mut inputs = self.inputs.lock().unwrap();

        for ep in inputs.values() {
            ep.endpoint.disconnect();
        }
        inputs.clear();

        self.status.set_state(PipelineState::Terminated);

        self.unpark_circuit();
        self.unpark_backpressure();
    }

    fn dump_profile(&self) {
        self.dump_profile_request.store(true, Ordering::Release);
        self.unpark_circuit();
    }

    fn error(&self, error: ControllerError) {
        (self.error_cb)(error);
    }

    /// Process an input transport error.
    ///
    /// Update endpoint stats and notify the error callback.
    fn input_transport_error(
        &self,
        endpoint_id: EndpointId,
        endpoint_name: &str,
        fatal: bool,
        error: AnyError,
    ) {
        self.status
            .input_transport_error(endpoint_id, fatal, &error);
        self.error(ControllerError::input_transport_error(
            endpoint_name,
            fatal,
            error,
        ));
    }

    fn parse_error(&self, endpoint_id: EndpointId, endpoint_name: &str, error: AnyError) {
        self.status.parse_error(endpoint_id);
        self.error(ControllerError::parse_error(endpoint_name, error));
    }

    fn encode_error(&self, endpoint_id: EndpointId, endpoint_name: &str, error: AnyError) {
        self.status.encode_error(endpoint_id);
        self.error(ControllerError::encode_error(endpoint_name, error));
    }

    /// Process an output transport error.
    ///
    /// Update endpoint stats and notify the error callback.
    fn output_transport_error(
        &self,
        endpoint_id: EndpointId,
        endpoint_name: &str,
        fatal: bool,
        error: AnyError,
    ) {
        self.status
            .output_transport_error(endpoint_id, fatal, &error);
        self.error(ControllerError::output_transport_error(
            endpoint_name,
            fatal,
            error,
        ));
    }

    fn output_buffers_full(&self) -> bool {
        self.status.output_buffers_full()
    }
}

/// An input probe inserted between the transport endpoint and the parser to
/// track stats and errors.
struct InputProbe {
    endpoint_id: EndpointId,
    endpoint_name: String,
    parser: Box<dyn Parser>,
    controller: Arc<ControllerInner>,
    circuit_thread_unparker: Unparker,
    backpressure_thread_unparker: Unparker,
}

impl InputProbe {
    fn new(
        endpoint_id: EndpointId,
        endpoint_name: &str,
        parser: Box<dyn Parser>,
        controller: Arc<ControllerInner>,
        circuit_thread_unparker: Unparker,
        backpressure_thread_unparker: Unparker,
    ) -> Self {
        Self {
            endpoint_id,
            endpoint_name: endpoint_name.to_owned(),
            parser,
            controller,
            circuit_thread_unparker,
            backpressure_thread_unparker,
        }
    }
}

/// `InputConsumer` interface exposed to the transport endpoint.
impl InputConsumer for InputProbe {
    fn input(&mut self, data: &[u8]) -> AnyResult<()> {
        // println!("input consumer {} bytes", data.len());
        // Pass input buffer to the parser.
        match self.parser.input(data) {
            Ok(num_records) => {
                // Success: push data to the input handle, update stats.
                self.parser.flush();
                self.controller.status.input_batch(
                    self.endpoint_id,
                    data.len(),
                    num_records,
                    &self.controller.status.global_config,
                    &self.circuit_thread_unparker,
                    &self.backpressure_thread_unparker,
                );
                Ok(())
            }
            Err(error) => {
                // Wrap it in Arc, so we can pass it to `parse_error` and return from this function
                // as well.
                let error = Arc::new(error);
                self.parser.clear();
                self.controller.parse_error(
                    self.endpoint_id,
                    &self.endpoint_name,
                    anyhow!(error.clone()),
                );
                Err(anyhow!(error))
            }
        }
    }

    fn eoi(&mut self) -> AnyResult<()> {
        // The endpoint reached end-of-file.  Notify and flush the parser (even though
        // no new data has been received, the parser may contain some partially
        // parsed data and may be waiting for, e.g., and end-of-line or
        // end-of-file to finish parsing it).
        match self.parser.eoi() {
            Ok(num_records) => {
                self.parser.flush();
                self.controller.status.eoi(
                    self.endpoint_id,
                    num_records,
                    &self.circuit_thread_unparker,
                );
                Ok(())
            }
            Err(error) => {
                let error = Arc::new(error);
                self.parser.clear();
                self.controller.error(ControllerError::parse_error(
                    &self.endpoint_name,
                    anyhow!(error.clone()),
                ));
                Err(anyhow!(error))
            }
        }
    }

    fn error(&mut self, fatal: bool, error: AnyError) {
        self.controller
            .input_transport_error(self.endpoint_id, &self.endpoint_name, fatal, error);
    }

    fn fork(&self) -> Box<dyn InputConsumer> {
        Box::new(Self::new(
            self.endpoint_id,
            &self.endpoint_name,
            self.parser.fork(),
            self.controller.clone(),
            self.circuit_thread_unparker.clone(),
            self.backpressure_thread_unparker.clone(),
        ))
    }
}

/// An output probe inserted between the encoder and the output transport
/// endpoint to track stats.
struct OutputProbe {
    endpoint_id: EndpointId,
    endpoint_name: String,
    endpoint: Box<dyn OutputEndpoint>,
    controller: Arc<ControllerInner>,
}

impl OutputProbe {
    pub fn new(
        endpoint_id: EndpointId,
        endpoint_name: &str,
        endpoint: Box<dyn OutputEndpoint>,
        controller: Arc<ControllerInner>,
    ) -> Self {
        Self {
            endpoint_id,
            endpoint_name: endpoint_name.to_owned(),
            endpoint,
            controller,
        }
    }
}

impl OutputConsumer for OutputProbe {
    fn batch_start(&mut self) {
        self.endpoint.batch_start().unwrap_or_else(|e| {
            self.controller
                .output_transport_error(self.endpoint_id, &self.endpoint_name, false, e);
        })
    }

    fn push_buffer(&mut self, buffer: &[u8]) {
        let num_bytes = buffer.len();

        match self.endpoint.push_buffer(buffer) {
            Ok(()) => {
                self.controller
                    .status
                    .output_buffer(self.endpoint_id, num_bytes);
            }
            Err(error) => {
                self.controller.output_transport_error(
                    self.endpoint_id,
                    &self.endpoint_name,
                    false,
                    error,
                );
            }
        }
    }

    fn batch_end(&mut self) {
        self.endpoint.batch_end().unwrap_or_else(|e| {
            self.controller
                .output_transport_error(self.endpoint_id, &self.endpoint_name, false, e);
        })
    }
}

#[cfg(test)]
mod test {
    use crate::{
        test::{generate_test_batch, test_circuit, wait, TestStruct},
        Controller, PipelineConfig,
    };
    use csv::{ReaderBuilder as CsvReaderBuilder, WriterBuilder as CsvWriterBuilder};
    use std::fs::remove_file;
    use tempfile::NamedTempFile;

    use proptest::prelude::*;

    // TODO: Parameterize this with config string, so we can test different
    // input/output formats and transports when we support more than one.
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(30))]
        #[test]
        fn proptest_csv_file(
            data in generate_test_batch(5000),
            min_batch_size_records in 1..100usize,
            max_buffering_delay_usecs in 1..2000usize,
            input_buffer_size_bytes in 1..1000usize,
            output_buffer_size_records in 1..100usize)
        {
            let (circuit, catalog) = test_circuit(4);

            let temp_input_file = NamedTempFile::new().unwrap();
            let temp_output_path = NamedTempFile::new().unwrap().into_temp_path();
            let output_path = temp_output_path.to_str().unwrap().to_string();
            temp_output_path.close().unwrap();

            let config_str = format!(
                r#"
min_batch_size_records: {min_batch_size_records}
max_buffering_delay_usecs: {max_buffering_delay_usecs}
name: test
inputs:
    test_input1:
        stream: test_input1
        transport:
            name: file
            config:
                path: {:?}
                buffer_size_bytes: {input_buffer_size_bytes}
                follow: false
        format:
            name: csv
outputs:
    test_output1:
        stream: test_output1
        transport:
            name: file
            config:
                path: {:?}
        format:
            name: csv
            config:
                buffer_size_records: {output_buffer_size_records}
        "#,
            temp_input_file.path().to_str().unwrap(),
            output_path,
            );

            println!("input file: {}", temp_input_file.path().to_str().unwrap());
            println!("output file: {output_path}");
            let config: PipelineConfig = serde_yaml::from_str(&config_str).unwrap();

            let controller = Controller::with_config(
                circuit,
                catalog,
                &config,
                Box::new(|e| panic!("error: {e}")),
                )
                .unwrap();

            let mut writer = CsvWriterBuilder::new()
                .has_headers(false)
                .from_writer(temp_input_file.as_file());

            for val in data.iter().cloned() {
                writer.serialize(val).unwrap();
            }
            writer.flush().unwrap();
            controller.start();

            // Wait for the pipeline to output all records.
            wait(|| controller.pipeline_complete(), None);

            assert_eq!(controller.status().output_status().get(&0).unwrap().transmitted_records(), data.len() as u64);

            controller.stop().unwrap();

            let mut expected = data;
            expected.sort();

            let mut actual: Vec<_> = CsvReaderBuilder::new()
                .has_headers(false)
                .from_path(&output_path)
                .unwrap()
                .deserialize::<(TestStruct, i32)>()
                .map(|res| {
                    let (val, weight) = res.unwrap();
                    assert_eq!(weight, 1);
                    val
                })
                .collect();
            actual.sort();

            // Don't leave garbage in the FS.
            remove_file(&output_path).unwrap();

            assert_eq!(actual, expected);
        }
    }
}
