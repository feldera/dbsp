use crate::{
    DeCollectionHandle, DeScalarHandle, DeScalarHandleImpl, DeZSetHandle, SerOutputBatchHandle,
};
use dbsp::{
    algebra::ZRingValue,
    operator::{DelayedFeedback, NeighborhoodDescr},
    CollectionHandle, InputHandle, RootCircuit, Stream, ZSet,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use utoipa::ToSchema;

// This is only here so we can derive `ToSchema` for it without adding
// a `utoipa` dependency to the `dbsp` crate to derive ToSchema for
// `NeighborhoodDescr`.
/// A request to output a specific neighborhood of a table or view.
/// The neighborhood is defined in terms of its central point (`anchor`)
/// and the number of rows preceding and following the anchor to output.
#[derive(Deserialize, ToSchema)]
pub struct NeighborhoodQuery {
    pub anchor: utoipa::openapi::Object,
    pub before: u32,
    pub after: u32,
}

/// A set of stream handles associated with each output collection.
pub struct OutputCollectionHandles {
    /// A stream of changes to the collection.
    pub delta_handle: Box<dyn SerOutputBatchHandle>,

    /// Input stream used to submit neighborhood queries.
    ///
    /// The stream carries values of type `(bool, Option<NeighborhoodDescr<K, V>>)`,
    /// where `K` and `V` are the key and value types of the collection.
    ///
    /// The first component of the tuple is the `reset` flag, which instructs
    /// the circuit to start executing the neighborhood query specified in the
    /// second component of the tuple.  The outputs of the neighborhood query are
    /// emitted to [`neighborhood_handle`](`Self::neighborhood_handle`) and
    /// [`neighborhood_snapshot_handle`](`Self::neighborhood_snapshot_handle`)
    /// streams.  When the flag is `false`, this input is ignored.
    ///
    /// In more detail, the circuit handles inputs written to this stream as follows:
    ///
    /// * `(true, Some(descr))` - Start monitoring the specified descriptor.  The
    ///   circuit will output a complete snapshot of the neighborhood to the
    ///   [`neighborhood_snapshot_handle`](`Self::neighborhood_snapshot_handle`)
    ///   sstream at the end of the current clock cycle.  The
    ///   [`neighborhood_handle`](`Self::neighborhood_handle`) stream will output
    ///   the difference between the previous and the new neighborhoods at the end
    ///   of the current clock cycle and will contain changes to the new neighborhood
    ///   going forward.
    ///
    /// * `(true, None)` - Stop executing the neighborhood query.  This is equivalent
    ///   to writing `(true, Some(descr))`, where `descr` specifies an empty
    ///   neighborhood.
    ///
    /// * `(false, _)` - This is a no-op. The circuit will continue monitoring the
    ///   previously specified neighborhood if any.  Nothing is written to the
    ///   [`neighborhood_snapshot_handle`](`Self::neighborhood_snapshot_handle`)
    ///   stream.
    pub neighborhood_descr_handle: Box<dyn DeScalarHandle>,

    /// A stream of changes to the neighborhood, computed using the
    /// [`Stream::neighborhood`] operator.
    pub neighborhood_handle: Box<dyn SerOutputBatchHandle>,

    /// A stream that contains the full snapshot of the neighborhood.  Only produces
    /// an output whenever the `neighborhood_descr_handle` input is set to `Some(..)`.  
    pub neighborhood_snapshot_handle: Box<dyn SerOutputBatchHandle>,

    /// Input stream used to submit the quantiles query.
    ///
    /// The value in the stream specifies the number of quantiles to
    /// output.  When  greater than zero, it triggers the quantiles
    /// computation.  The result is output to the
    /// [`quantiles_handle`](`Self::quantiles_handle`) stream at the
    /// end of the current clock cycle.
    pub num_quantiles_handle: InputHandle<usize>,

    /// Quantiles stream.
    ///
    /// When the `num_quantiles_handle` input is set to `N`, `N>0`, this stream outputs
    /// up to `N` quantiles of the input collection, computed using the
    /// [`Stream::stream_key_quantiles`] operator.
    pub quantiles_handle: Box<dyn SerOutputBatchHandle>,
}

/// A query over an output stream.
///
/// We currently do not support ad hoc queries.  Instead the client can use
/// three pre-defined queries to inspect the contents of a table or view.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, PartialOrd, ToSchema, Ord)]
pub enum OutputQuery {
    /// Query the entire contents of the table (similar to `SELECT * FROM`).
    #[serde(rename = "table")]
    Table,
    /// Neighborhood query (see [`Stream::neighborhood`]).
    #[serde(rename = "neighborhood")]
    Neighborhood,
    /// Quantiles query (see `[Stream::stream_key_quantiles]`).
    #[serde(rename = "quantiles")]
    Quantiles,
}

impl Default for OutputQuery {
    fn default() -> Self {
        Self::Table
    }
}

/// Query result streams.
///
/// Stores the result of a a [query](`OutputQuery`) as a pair of streams:
/// a stream of changes and a snapshot, i.e., the integral, of all previous
/// changes.  Not all queries return both streams, e.g., the
/// [quantiles](`OutputQuery::Quantiles`) query only returns a snapshot,
/// while the [table](`OutputQuery::Table`) query currently only returns the
/// delta stream; therefore the stream handles are wrapped in `Option`s.
///
/// Whenever both streams are present, the client may consume the result in
/// a hybrid mode: read the initial snapshot containing a full answer to the
/// query based on previously received inputs, and then listen to the delta
/// stream for incremental updates triggered by new inputs.
pub struct OutputQueryHandles {
    pub delta: Option<Box<dyn SerOutputBatchHandle>>,
    pub snapshot: Option<Box<dyn SerOutputBatchHandle>>,
}

/// A catalog of input and output stream handles of a circuit.
///
/// An instance of this type is created by the user (or auto-generated code)
/// who constructs the circuit and is used by
/// [`Controller`](`crate::Controller`) to bind external data sources and sinks
/// to DBSP streams.
#[derive(Default)]
pub struct Catalog {
    input_collection_handles: BTreeMap<String, Box<dyn DeCollectionHandle>>,
    output_batch_handles: BTreeMap<String, OutputCollectionHandles>,
}

impl Catalog {
    /// Create an empty catalog.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an input stream of Z-sets to the catalog.
    pub fn register_input_zset<Z>(
        &mut self,
        name: &str,
        stream: Stream<RootCircuit, Z>,
        handle: CollectionHandle<Z::Key, Z::R>,
    ) where
        Z: ZSet + Send + Sync,
        Z::R: ZRingValue + Into<i64> + Sync,
        Z::Key: for<'de> Deserialize<'de> + Serialize + Sync,
    {
        self.register_input_collection_handle(name, DeZSetHandle::new(handle));

        // Inputs are also outputs.
        self.register_output_zset(name, stream);
    }

    /// Add a named input stream handle to the catalog.
    fn register_input_collection_handle<H>(&mut self, name: &str, handle: H)
    where
        H: DeCollectionHandle + 'static,
    {
        self.input_collection_handles
            .insert(name.to_owned(), Box::new(handle));
    }

    /// Add an output stream of Z-sets to the catalog.
    pub fn register_output_zset<Z>(&mut self, name: &str, stream: Stream<RootCircuit, Z>)
    where
        Z: ZSet + Send + Sync,
        Z::R: ZRingValue + Into<i64> + Sync,
        Z::Key: for<'de> Deserialize<'de> + Serialize + Sync,
    {
        let circuit = stream.circuit();

        // Create handle for the stream itself.
        let delta_handle = stream.output();

        // Create handles for the neighborhood query.
        let (neighborhood_descr_stream, neighborhood_descr_handle) =
            circuit.add_input_stream::<(bool, Option<NeighborhoodDescr<Z::Key, Z::Val>>)>();
        let neighborhood_stream = {
            // Create a feedback loop to latch the latest neighborhood descriptor
            // when `reset=true`.
            let feedback =
                <DelayedFeedback<RootCircuit, Option<NeighborhoodDescr<Z::Key, Z::Val>>>>::new(
                    stream.circuit(),
                );
            let new_neighborhood =
                feedback
                    .stream()
                    .apply2(&neighborhood_descr_stream, |old, (reset, new)| {
                        if *reset {
                            new.clone()
                        } else {
                            old.clone()
                        }
                    });
            feedback.connect(&new_neighborhood);
            stream.neighborhood(&new_neighborhood)
        };

        // Neighborhood delta stream.
        let neighborhood_handle = neighborhood_stream.output();

        // Neighborhood snapshot stream.  The integral computation
        // is essentially free thanks to stream caching.
        let neighborhood_snapshot_stream = neighborhood_stream.integrate();
        let neighborhood_snapshot_handle = neighborhood_snapshot_stream
            .output_guarded(&neighborhood_descr_stream.apply(|(reset, _descr)| *reset));

        // Handle for the quantiles query.
        let (num_quantiles_stream, num_quantiles_handle) = circuit.add_input_stream::<usize>();

        // Output of the quantiles query, only produced when `num_quantiles>0`.
        let quantiles_stream = stream
            .integrate_trace()
            .stream_key_quantiles(&num_quantiles_stream);
        let quantiles_handle = quantiles_stream
            .output_guarded(&num_quantiles_stream.apply(|num_quantiles| *num_quantiles > 0));

        let handles = OutputCollectionHandles {
            delta_handle: Box::new(delta_handle) as Box<dyn SerOutputBatchHandle>,

            neighborhood_descr_handle: Box::new(DeScalarHandleImpl::new(neighborhood_descr_handle))
                as Box<dyn DeScalarHandle>,
            neighborhood_handle: Box::new(neighborhood_handle) as Box<dyn SerOutputBatchHandle>,
            neighborhood_snapshot_handle: Box::new(neighborhood_snapshot_handle)
                as Box<dyn SerOutputBatchHandle>,

            num_quantiles_handle,
            quantiles_handle: Box::new(quantiles_handle) as Box<dyn SerOutputBatchHandle>,
        };

        self.output_batch_handles.insert(name.to_owned(), handles);
    }

    /// Look up an input stream handle by name.
    pub fn input_collection_handle(&self, name: &str) -> Option<&dyn DeCollectionHandle> {
        self.input_collection_handles.get(name).map(|b| &**b)
    }

    /// Look up output stream handles by name.
    pub fn output_handles(&self, name: &str) -> Option<&OutputCollectionHandles> {
        self.output_batch_handles.get(name)
    }

    /// Look up an output query handles by stream name and query type.
    pub fn output_query_handles(
        &self,
        name: &str,
        query: OutputQuery,
    ) -> Option<OutputQueryHandles> {
        self.output_batch_handles
            .get(name)
            .map(|handles| match query {
                OutputQuery::Table => OutputQueryHandles {
                    delta: Some(handles.delta_handle.fork()),
                    snapshot: None,
                },
                OutputQuery::Neighborhood => OutputQueryHandles {
                    delta: (Some(handles.neighborhood_handle.fork())),
                    snapshot: Some(handles.neighborhood_snapshot_handle.fork()),
                },
                OutputQuery::Quantiles => OutputQueryHandles {
                    delta: None,
                    snapshot: Some(handles.quantiles_handle.fork()),
                },
            })
    }
}
