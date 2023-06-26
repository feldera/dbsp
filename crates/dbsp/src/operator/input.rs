use crate::{
    algebra::ZRingValue,
    circuit::{
        operator_traits::{Operator, SourceOperator},
        LocalStoreMarker, RootCircuit, Scope,
    },
    default_hash,
    trace::Batch,
    Circuit, DBData, DBWeight, OrdIndexedZSet, OrdZSet, Runtime, Stream,
};
use std::{
    borrow::Cow,
    hash::{Hash, Hasher},
    marker::PhantomData,
    mem::{swap, take},
    ops::Range,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};
use typedmap::TypedMapKey;

pub type IndexedZSetStream<K, V, R> = Stream<RootCircuit, OrdIndexedZSet<K, V, R>>;
pub type ZSetStream<K, R> = Stream<RootCircuit, OrdZSet<K, R>>;

impl RootCircuit {
    /// Create an input stream that carries values of type `T`.
    ///
    /// Input streams are used to push data to the circuit from the outside
    /// world via the [`InputHandle`] object returned by this method:
    ///
    /// ```text
    ///                   ┌──────────────────────┐
    ///                   │Circuit               │
    ///                   │                      │
    /// ┌───────────┐     │   stream             │
    /// │InputHandle├──────────────────►         │
    /// └───────────┘     │                      │
    ///                   │                      │
    ///                   └──────────────────────┘
    /// ```
    ///
    /// At each clock cycle, the stream consumes the last value placed in it via
    /// the `InputHandle` (or `<T as Default>::default()` if no value was
    /// placed in the stream since the last clock cycle) and yields this
    /// value to all downstream operators connected to it.
    ///
    /// See [`InputHandle`] for more details.
    pub fn add_input_stream<T>(&self) -> (Stream<Self, T>, InputHandle<T>)
    where
        T: Default + Clone + Send + 'static,
    {
        let (input, input_handle) = Input::new(|x| x);
        let stream = self.add_source(input);
        (stream, input_handle)
    }

    /// Create an input stream that carries values of type [`OrdZSet<K,
    /// R>`](`OrdZSet`).
    ///
    /// Creates an input stream that carries values of type `OrdZSet<K, R>` and
    /// an input handle of type [`CollectionHandle<K, R>`](`CollectionHandle`)
    /// used to construct input Z-sets out of individual elements.  The
    /// client invokes [`CollectionHandle::push`] and
    /// [`CollectionHandle::append`] any number of times to add values to
    /// the input Z-set. These values are distributed across all worker
    /// threads (when running in a multithreaded [`Runtime`]) in a round-robin
    /// fashion and buffered until the start of the next clock
    /// cycle.  At the start of a clock cycle (triggered by
    /// [`DBSPHandle::step`](`crate::DBSPHandle::step`) or
    /// [`CircuitHandle::step`](`crate::CircuitHandle::step`)), the circuit
    /// reads all buffered values and assembles them into an `OrdZSet`.
    ///
    /// See [`CollectionHandle`] for more details.
    pub fn add_input_zset<K, R>(&self) -> (Stream<Self, OrdZSet<K, R>>, CollectionHandle<K, R>)
    where
        K: DBData,
        R: DBWeight,
    {
        let (input, input_handle) = Input::new(|tuples| OrdZSet::from_keys((), tuples));
        let stream = self.add_source(input);

        let zset_handle = <CollectionHandle<K, R>>::new(input_handle);

        (stream, zset_handle)
    }

    /// Create an input stream that carries values of type [`OrdIndexedZSet<K,
    /// V, R>`](`OrdIndexedZSet`).
    ///
    /// Creates an input stream that carries values of type `OrdIndexedZSet<K,
    /// V, R>` and an input handle of type [`CollectionHandle<K, (V,
    /// R)>`](`CollectionHandle`) used to construct input Z-sets out of
    /// individual elements.  The client invokes [`CollectionHandle::push`]
    /// and [`CollectionHandle::append`] any number of times to add
    /// `key/value/weight` triples the indexed Z-set. These triples are
    /// distributed across all worker threads (when running in a
    /// multithreaded [`Runtime`]) in a round-robin fashion, and
    /// buffered until the start of the next clock cycle.  At the start of a
    /// clock cycle (triggered by
    /// [`DBSPHandle::step`](`crate::DBSPHandle::step`) or
    /// [`CircuitHandle::step`](`crate::CircuitHandle::step`)), the circuit
    /// reads all buffered values and assembles them into an `OrdIndexedZSet`.
    ///
    /// See [`CollectionHandle`] for more details.
    #[allow(clippy::type_complexity)]
    pub fn add_input_indexed_zset<K, V, R>(
        &self,
    ) -> (IndexedZSetStream<K, V, R>, CollectionHandle<K, (V, R)>)
    where
        K: DBData,
        V: DBData,
        R: DBWeight,
    {
        let (input, input_handle) = Input::new(|tuples: Vec<(K, (V, R))>| {
            OrdIndexedZSet::from_tuples(
                (),
                tuples.into_iter().map(|(k, (v, w))| ((k, v), w)).collect(),
            )
        });
        let stream = self.add_source(input);

        let zset_handle = <CollectionHandle<K, (V, R)>>::new(input_handle);

        (stream, zset_handle)
    }

    fn add_upsert<K, VI, V, F, B>(
        &self,
        input_stream: Stream<Self, Vec<(K, VI)>>,
        upsert_func: F,
    ) -> Stream<Self, B>
    where
        K: DBData,
        F: Fn(VI) -> Option<V> + 'static,
        B: Batch<Key = K, Val = V, Time = ()>,
        B::R: ZRingValue,
        V: DBData,
        VI: DBData,
    {
        let sorted = input_stream
            .apply_owned(move |mut upserts| {
                // Sort the vector by key, preserving the history of updates for each key.
                // Upserts cannot be merged or reordered, therefore we cannot use unstable sort.
                upserts.sort_by(|(k1, _), (k2, _)| k1.cmp(k2));

                // Find the last upsert for each key, that's the only one that matters.
                upserts.dedup_by(|(k1, v1), (k2, v2)| {
                    if k1 == k2 {
                        swap(v1, v2);
                        true
                    } else {
                        false
                    }
                });

                upserts
                    .into_iter()
                    .map(|(k, v)| (k, upsert_func(v)))
                    .collect::<Vec<_>>()
            })
            // UpsertHandle shards its inputs.
            .mark_sharded();

        sorted.upsert::<B>()
    }

    /// Create an input table with set semantics.
    ///
    /// # Motivation
    ///
    /// DBSP represents relational data using Z-sets, i.e., tables where each
    /// record has a weight, which denotes the number of times the record occurs
    /// in the table.  Updates to Z-sets are also Z-sets, with
    /// positive weights representing insertions and negative weights
    /// representing deletions.  The contents of the Z-set after an update
    /// is computed by summing up the weights associated with each record.
    /// Z-set updates are commutative, e.g., insert->insert->delete and
    /// insert->delete->insert sequences are both equivalent to a single
    /// insert.  This internal representation enables efficient incremental
    /// computation, but it does not always match the data model used by the
    /// outside world, and may require a translation layer to eliminate this
    /// mismatch when ingesting data into DBSP.
    ///
    /// In particular, input tables often behave as sets.  A set is a special
    /// case of a Z-set where all weights are equal to 1.  Duplicate
    /// insertions and deletions to sets are ignored, i.e., inserting an
    /// existing element or deleting an element not in the set are both
    /// no-ops.  Set updates are not commutative, e.g., the
    /// insert->delete->insert sequence is equivalent to a single insert,
    /// while insert->insert->delete is equivalent to a delete.
    ///
    /// # Details
    ///
    /// The `add_input_set` operator creates an input table that internally
    /// appears as a Z-set with unit weights, but that ingests input data
    /// using set semantics. It returns a stream that carries values of type
    /// `OrdZSet<K, R>` and an input handle of type
    /// [`UpsertHandle<K,bool>`](`UpsertHandle`).  The client uses
    /// [`UpsertHandle::push`] and [`UpsertHandle::append`] to submit
    /// commands of the form `(val, true)` to insert an element to the set
    /// and `(val, false) ` to delete `val` from the set.  These commands
    /// are buffered until the start of the next clock cycle.
    ///
    /// At the start of a clock cycle (triggered by
    /// [`DBSPHandle::step`](`crate::DBSPHandle::step`) or
    /// [`CircuitHandle::step`](`crate::CircuitHandle::step`)), DBSP applies
    /// buffered commands in order and computes an update to the input set as
    /// an `OrdZSet` with weights `+1` and `-1` representing set insertions and
    /// deletions respectively. The following table illustrates the
    /// relationship between input commands, the contents of the set and the
    /// contents of the stream produced by this operator:
    ///
    /// ```text
    /// time │      input commands          │content of the   │ stream returned by     │  comment
    ///      │                              │input set        │ `add_input_set`        │
    /// ─────┼──────────────────────────────┼─────────────────┼────────────────────────┼───────────────────────────────────────────────────────
    ///    1 │{("foo",true),("bar",true)}   │  {"foo","bar"}  │ {("foo",+1),("bar",+1)}│
    ///    2 │{("foo",true),("bar",false)}  │  {"foo"}        │ {("bar",-1)}           │ignore duplicate insert of "foo"
    ///    3 │{("foo",false),("foo",true)}  │  {"foo"}        │ {}                     │deleting and re-inserting "foo" is a no-op
    ///    4 │{("foo",false),("bar",false)} │  {}             │ {("foo",-1)}           │deleting value "bar" that is not in the set is a no-op
    /// ─────┴──────────────────────────────┴─────────────────┴────────────────────────┴────────────────────────────────────────────────────────
    /// ```
    ///
    /// Internally, this operator maintains the contents of the input set
    /// partitioned across all worker threads based on the hash of the
    /// value.  Insert/delete commands are routed to the worker in charge of
    /// the given value.
    // TODO: Add a version that takes a custom hash function.
    pub fn add_input_set<K, R>(&self) -> (ZSetStream<K, R>, UpsertHandle<K, bool>)
    where
        K: DBData,
        R: DBData + ZRingValue,
    {
        self.region("input_set", || {
            let (input, input_handle) = Input::new(|tuples: Vec<(K, bool)>| tuples);
            let input_stream = self.add_source(input);
            let upsert_handle = <UpsertHandle<K, bool>>::new(input_handle);

            let upsert =
                self.add_upsert(input_stream, |insert| if insert { Some(()) } else { None });

            (upsert, upsert_handle)
        })
    }

    /// Create an input table as a key-value map with upsert update semantics.
    ///
    /// # Motivation
    ///
    /// DBSP represents indexed data using indexed Z-sets, i.e., sets
    /// of `(key, value, weight)` tuples, where `weight`
    /// denotes the number of times the key-value pair occurs in
    /// the table.  Updates to indexed Z-sets are also indexed Z-sets, with
    /// positive weights representing insertions and negative weights
    /// representing deletions.  The contents of the indexed Z-set after an
    /// update is computed by summing up weights associated with each
    /// key-value pair. This representation enables efficient incremental
    /// computation, but it does not always match the data model used by the
    /// outside world, and may require a translation layer to eliminate this
    /// mismatch when ingesting indexed data into DBSP.
    ///
    /// In particular, input tables often behave as key-value maps.
    /// A map is a special case of an indexed Z-set where each key has
    /// a unique value associated with it and where all weights are 1.
    /// Map updates follow the update-or-insert (*upsert*) semantics,
    /// where inserting a new key-value pair overwrites the old value
    /// associated with the key, if any.
    ///
    /// # Details
    ///
    /// The `add_input_map` operator creates an input table that internally
    /// appears as an indexed Z-set with all unit weights, but that ingests
    /// input data using upsert semantics. It returns a stream that carries
    /// values of type `OrdIndexedZSet<K, V, R>` and an input handle of type
    /// [`UpsertHandle<K,Option<V>>`](`UpsertHandle`).  The client uses
    /// [`UpsertHandle::push`] and [`UpsertHandle::append`] to submit
    /// commands of the form `(key, Some(val))` to insert a new key-value
    /// pair and `(key, None) ` to delete the value associated with `key` is
    /// any. These commands are buffered until the start of the next clock
    /// cycle.
    ///
    /// At the start of a clock cycle (triggered by
    /// [`DBSPHandle::step`](`crate::DBSPHandle::step`) or
    /// [`CircuitHandle::step`](`crate::CircuitHandle::step`)),
    /// DBSP applies buffered commands in order and
    /// computes an update to the input set as an `OrdIndexedZSet` with weights
    /// `+1` and `-1` representing insertions and deletions respectively.
    /// The following table illustrates the relationship between input commands,
    /// the contents of the map and the contents of the stream produced by this
    /// operator:
    ///
    /// ```text
    /// time │      input commands               │content of the        │ stream returned by         │  comment
    ///      │                                   │input map             │ `add_input_map`            │
    /// ─────┼───────────────────────────────────┼──────────────────────┼────────────────────────────┼───────────────────────────────────────────────────────
    ///    1 │{(1,Some("foo"), (2,Some("bar"))}  │{(1,"foo"),(2,"bar")} │ {(1,"foo",+1),(2,"bar",+1)}│
    ///    2 │{(1,Some("foo"), (2,Some("baz"))}  │{(1,"foo"),(2,"baz")} │ {(2,"bar",-1),(2,"baz",+1)}│ Ignore duplicate insert of (1,"foo"). New value
    ///      |                                   |                      |                            | "baz" for key 2 overwrites the old value "bar".
    ///    3 │{(1,None),(2,Some("bar")),(2,None)}│{}                    │ {(1,"foo",-1),(2,"baz",-1)}│ Delete both keys. Upsert (2,"bar") is overridden
    ///      |                                   |                      |                            | by subsequent delete command.
    /// ─────┴───────────────────────────────────┴──────────────────────┴────────────────────────────┴────────────────────────────────────────────────────────
    /// ```
    ///
    /// Note that upsert commands cannot fail.  Duplicate inserts and deletes
    /// are simply ignored.
    ///
    /// Internally, this operator maintains the contents of the map
    /// partitioned across all worker threads based on the hash of the
    /// key.  Upsert/delete commands are routed to the worker in charge of
    /// the given key.
    // TODO: Add a version that takes a custom hash function.
    pub fn add_input_map<K, V, R>(&self) -> (IndexedZSetStream<K, V, R>, UpsertHandle<K, Option<V>>)
    where
        K: DBData,
        V: DBData,
        R: DBData + ZRingValue,
    {
        self.region("input_map", || {
            let (input, input_handle) = Input::new(|tuples: Vec<(K, Option<V>)>| tuples);
            let input_stream = self.add_source(input);
            let zset_handle = <UpsertHandle<K, Option<V>>>::new(input_handle);

            let upsert = self.add_upsert(input_stream, |val| val);

            (upsert, zset_handle)
        })
    }
}

/*
// We may want to uncomment and use the following operator based on
// profiling data.  At the moment the `Input` operator assembles input
// tuples into batches as they are received from `CollectionHandle`s.
// Since `CollectionHandle` doesn't consistently map keys to workers,
// resulting batches may need to be re-sharded by the next operator.
// It may be more efficient to shard update vectors received from
// `CollectionHandle` directly without paying the cost of assembling
// them into batches first.  This is what this operator does.
impl<K, V> Stream<Circuit<()>, Vec<(K, V)>>
where
    K: Send + Hash + Clone + 'static,
    V: Send + Clone + 'static,
{
    fn shard_vec(&self) -> Stream<Circuit<()>, Vec<(K, V)>> {
        Runtime::runtime()
            .map(|runtime| {
                let num_workers = runtime.num_workers();

                if num_workers == 1 {
                    self.clone()
                } else {
                    let (sender, receiver) = self.circuit().new_exchange_operators(
                        &runtime,
                        Runtime::worker_index(),
                        move |batch: Vec<(K, V)>, batches: &mut Vec<Vec<(K, V)>>| {
                            for _ in 0..num_workers {
                                batches.push(Vec::with_capacity(batch.len() / num_workers));
                            }

                            for (key, val) in batch.into_iter() {
                                let batch_index = fxhash::hash(&key) % num_workers;
                                batches[batch_index].push((key, val))
                            }
                        },
                        move |output: &mut Vec<(K, V)>, batch: Vec<(K, V)>| {
                            if output.is_empty() {
                                output.reserve(batch.len() * num_workers);
                            }
                            output.extend(batch);
                        },
                    );

                    self.circuit().add_exchange(sender, receiver, self)
                }
            })
            .unwrap_or_else(|| self.clone())
    }
}
*/

/// `TypedMapKey` entry used to share InputHandle objects across workers in a
/// runtime. The first worker to create the handle will store it in the map,
/// subsequent workers will get a clone of the same handle.
struct InputId<T> {
    id: usize,
    _marker: PhantomData<T>,
}

unsafe impl<T> Sync for InputId<T> {}

// Implement `Hash`, `Eq` manually to avoid `T: Hash` type bound.
impl<T> Hash for InputId<T> {
    fn hash<H>(&self, state: &mut H)
    where
        H: Hasher,
    {
        self.id.hash(state);
    }
}

impl<T> PartialEq for InputId<T> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl<T> Eq for InputId<T> {}

impl<T> InputId<T> {
    fn new(id: usize) -> Self {
        Self {
            id,
            _marker: PhantomData,
        }
    }
}

impl<T> TypedMapKey<LocalStoreMarker> for InputId<T>
where
    T: 'static,
{
    type Value = InputHandle<T>;
}

/// Mailbox that buffers data between the circuit and the outside world.
/// It is used inside an `InputHandle` to store data sent to a worker
/// thread and inside an `OutputHandle` to store data sent by a worker
/// thread to the outside world.
#[derive(Clone)]
pub(super) struct Mailbox<T> {
    value: Arc<Mutex<T>>,
}

impl<T> Mailbox<T>
where
    T: Default,
{
    pub(super) fn new() -> Self {
        Self {
            value: Arc::new(Mutex::new(Default::default())),
        }
    }

    pub(super) fn take(&self) -> T {
        take(&mut *self.value.lock().unwrap())
    }

    fn update<F>(&self, f: F)
    where
        F: FnOnce(&mut T),
    {
        f(&mut *self.value.lock().unwrap());
    }

    pub(super) fn set(&self, v: T) {
        *self.value.lock().unwrap() = v;
    }
}

struct InputHandleInternal<T> {
    mailbox: Vec<Mailbox<T>>,
    offset: usize,
}

impl<T> InputHandleInternal<T>
where
    T: Default + Clone,
{
    // Returns a new `InputHandleInternal` for workers with indexes in the range
    // of `workers`.
    fn new(workers: Range<usize>) -> Self {
        assert!(!workers.is_empty());
        Self {
            mailbox: workers.clone().map(|_| Mailbox::new()).collect(),
            offset: workers.start,
        }
    }

    fn workers(&self) -> Range<usize> {
        self.offset..self.offset + self.mailbox.len()
    }

    fn set_for_worker(&self, worker: usize, v: T) {
        self.mailbox(worker).set(v);
    }

    fn update_for_worker<F>(&self, worker: usize, f: F)
    where
        F: FnOnce(&mut T),
    {
        self.mailbox(worker).update(f);
    }

    /// Send the same value to all workers.
    fn set_for_all(&self, v: T) {
        for i in 0..self.mailbox.len() - 1 {
            self.mailbox[i].set(v.clone());
        }
        self.mailbox[self.mailbox.len() - 1].set(v);
    }

    fn clear_for_all(&self) {
        for mailbox in self.mailbox.iter() {
            mailbox.set(Default::default());
        }
    }

    fn mailbox(&self, worker: usize) -> &Mailbox<T> {
        &self.mailbox[worker - self.offset]
    }
}

/// A handle used to write data to an input stream created by
/// the [`RootCircuit::add_input_stream`] method.
///
/// Internally, the handle manages an array of mailboxes, one for
/// each worker thread.  At the start of each clock cycle, the
/// circuit reads the current value from each mailbox and writes
/// it to the input stream associated with the handle, leaving
/// the mailbox empty (more precisely, the mailbox will contain
/// `T::default()`).  The handle is then used to write new values
/// to the mailboxes, which will be consumed at the next
/// logical clock tick.
#[derive(Clone)]
pub struct InputHandle<T>(Arc<InputHandleInternal<T>>);

impl<T> InputHandle<T>
where
    T: Default + Send + Clone + 'static,
{
    fn new() -> Self {
        match Runtime::runtime() {
            None => Self(Arc::new(InputHandleInternal::new(0..1))),
            Some(runtime) => {
                let input_id = runtime.sequence_next(Runtime::worker_index());

                runtime
                    .local_store()
                    .entry(InputId::new(input_id))
                    .or_insert_with(|| {
                        Self(Arc::new(InputHandleInternal::new(
                            runtime.layout().local_workers(),
                        )))
                    })
                    .value()
                    .clone()
            }
        }
    }

    /// Returns the range of worker indexes that this input handle covers, that
    /// is, all of the workers on this host (all workers everywhere, for a
    /// single-host circuit).
    fn workers(&self) -> Range<usize> {
        self.0.workers()
    }

    fn mailbox(&self, worker: usize) -> &Mailbox<T> {
        self.0.mailbox(worker)
    }

    /// Write value `v` to the specified worker's mailbox,
    /// overwriting any previous value in the mailbox.
    pub fn set_for_worker(&self, worker: usize, v: T) {
        self.0.set_for_worker(worker, v);
    }

    /// Mutate the contents of the specified worker's mailbox
    /// using closure `f`.
    pub fn update_for_worker<F>(&self, worker: usize, f: F)
    where
        F: FnOnce(&mut T),
    {
        self.0.update_for_worker(worker, f);
    }

    /// Write value `v` to all worker mailboxes.
    pub fn set_for_all(&self, v: T) {
        self.0.set_for_all(v);
    }

    pub fn clear_for_all(&self) {
        self.0.clear_for_all();
    }
}

/// A handle used to write data to an input stream created by
/// [`add_input_zset`](`RootCircuit::add_input_zset`),
/// and [`add_input_indexed_zset`](`RootCircuit::add_input_indexed_zset`)
/// methods.
///
/// The handle provides an API to push updates to the stream in
/// the form of `(key, value)` tuples:
///
///    * For `add_input_zset`, the tuples have the form `(key, weight)`.
///
///    * For `add_input_indexed_zset`, the tuples have the form `(key, (value,
///      weight)`).
///
/// See [`add_input_zset`](`RootCircuit::add_input_zset`),
/// [`add_input_indexed_zset`](`RootCircuit::add_input_indexed_zset`) and
/// documentation for the exact semantics of these updates.
///
/// Internally, the handle manages an array of mailboxes, one for each worker
/// thread on this host. It automatically partitions updates across mailboxes in
/// a round robin fashion.  At the start of each clock cycle, the circuit
/// consumes updates buffered in each mailbox, leaving the mailbox empty.
pub struct CollectionHandle<K, V> {
    input_handle: InputHandle<Vec<(K, V)>>,
    // Used to send tuples to workers in round robin.  Oftentimes the
    // workers will immediately repartition the inputs based on the hash
    // of the key; however this is more efficient than doing it here, as
    // the work will be evenly split across workers.
    next_worker: AtomicUsize,
}

impl<K, V> Clone for CollectionHandle<K, V>
where
    K: DBData,
    V: DBData,
{
    fn clone(&self) -> Self {
        Self::new(self.input_handle.clone())
    }
}

impl<K, V> CollectionHandle<K, V>
where
    K: DBData,
    V: DBData,
{
    fn new(input_handle: InputHandle<Vec<(K, V)>>) -> Self {
        Self {
            input_handle,
            next_worker: AtomicUsize::new(0),
        }
    }

    #[inline]
    fn num_partitions(&self) -> usize {
        self.input_handle.0.mailbox.len()
    }

    /// Push a single `(key,value)` pair to the input stream.
    pub fn push(&self, k: K, v: V) {
        let next_worker = match self.num_partitions() {
            1 => 0,
            n => self.next_worker.fetch_add(1, Ordering::AcqRel) % n,
        };
        self.input_handle
            .update_for_worker(next_worker + self.input_handle.workers().start, |tuples| {
                tuples.push((k, v))
            });
    }

    /// Push multiple `(key,value)` pairs to the input stream.
    ///
    /// This is more efficient than pushing values one-by-one using
    /// [`Self::push`].
    ///
    /// # Concurrency
    ///
    /// This method partitions updates across workers and then buffers them
    /// atomically with respect to each worker, i.e., each worker observes
    /// all updates in an `append` at the same logical time.  However the
    /// operation is not atomic as a whole: concurrent `append` and
    /// `clear_input` calls (performed via clones of the
    /// same `CollectionHandle`) may apply in different orders in different
    /// worker threads.  This method is also not atomic with respect to
    /// [`DBSPHandle::step`](`crate::DBSPHandle::step`) and
    /// [`CircuitHandle::step`](`crate::CircuitHandle::step`) methods: a
    /// `DBSPHandle::step` call performed concurrently with `append` may
    /// result in only a subset of the workers observing updates from this
    /// `append` operation.  The remaining updates will appear
    /// during subsequent logical clock cycles.
    pub fn append(&self, vals: &mut Vec<(K, V)>) {
        let num_partitions = self.num_partitions();
        let next_worker = if num_partitions > 1 {
            self.next_worker.load(Ordering::Acquire)
        } else {
            0
        };

        // We divide `val` across `num_partitions` workers as evenly as we can.  The
        // first `remainder` workers will receive `quotient + 1` values, and the
        // rest will receive `quotient`.
        let quotient = vals.len() / num_partitions;
        let remainder = vals.len() % num_partitions;
        let worker_ofs = self.input_handle.workers().start;
        for i in 0..num_partitions {
            let mut partition_size = quotient;
            if i < remainder {
                partition_size += 1;
            }

            let worker = (next_worker + i) % num_partitions + worker_ofs;
            if partition_size == vals.len() {
                self.input_handle.update_for_worker(worker, |tuples| {
                    if tuples.is_empty() {
                        *tuples = take(vals);
                    } else {
                        tuples.append(vals);
                    }
                });
                break;
            }

            // Draining from the end should be more efficient as it doesn't
            // require memcpy'ing the tail of the vector to the front.
            let tail = vals.drain(vals.len() - partition_size..);
            self.input_handle.update_for_worker(worker, |tuples| {
                if tuples.is_empty() {
                    *tuples = tail.collect();
                } else {
                    tuples.extend(tail);
                }
            });
        }
        assert_eq!(vals.len(), 0);

        // If `remainder` is positive, then the values were not distributed completely
        // evenly. Advance `self.next_worker` so that the next batch of values
        // will give extra values to the ones that didn't get extra this time.
        if remainder > 0 {
            self.next_worker
                .store(next_worker + remainder, Ordering::Release);
        }
    }

    /// Clear all inputs buffered since the start of the last clock cycle.
    ///
    /// # Concurrency
    ///
    /// Similar to [`Self::append`], this method atomically clears updates
    /// buffered for each worker thread, i.e., the worker observes all or none
    /// of the updates buffered before the call to `clear_input`; however the
    /// operation is not atomic as a whole: concurrent `append` and
    /// `clear_input` calls (performed via clones of the
    /// same `CollectionHandle`) may apply in different orders in different
    /// worker threads.  This method is also not atomic with respect to
    /// [`DBSPHandle::step`](`crate::DBSPHandle::step`) and
    /// [`CircuitHandle::step`](`crate::CircuitHandle::step`) methods: a
    /// `DBSPHandle::step` call performed concurrently with `clear_input` may
    /// result in only a subset of the workers observing empty inputs, while
    /// other workers observe updates buffered prior to the `clear_input` call.
    pub fn clear_input(&self) {
        self.input_handle.set_for_all(Vec::new());
    }
}

pub trait HashFunc<K>: Fn(&K) -> u32 + Send + Sync {}

impl<K, F> HashFunc<K> for F where F: Fn(&K) -> u32 + Send + Sync {}

/// A handle used to write data to an input stream created by
/// [`add_input_set`](`RootCircuit::add_input_set`) and
/// [`add_input_map`](`RootCircuit::add_input_map`)
/// methods.
///
/// The handle provides an API to push updates to the stream in
/// the form of `(key, value)` tuples:
///
///    * For `add_input_set`, the tuples have the form `(Key, bool)`.
///
///    * For `add_input_map`, the tuples have the form `(Key, Option<Value>)`.
///
/// See [`add_input_set`](`RootCircuit::add_input_set`) and
/// [`add_input_map`](`RootCircuit::add_input_map`) documentation for the exact
/// semantics of these updates.
///
/// Internally, the handle manages an array of mailboxes, one for
/// each worker thread. It automatically partitions updates across
/// mailboxes based on the hash of the key.
/// At the start of each clock cycle, the
/// circuit consumes updates buffered in each mailbox, leaving
/// the mailbox empty.
pub struct UpsertHandle<K, V> {
    buffers: Vec<Vec<(K, V)>>,
    input_handle: InputHandle<Vec<(K, V)>>,
    // Sharding the input collection based on the hash of the key is more
    // expensive than simple round robin partitioning used by
    // `CollectionHandle`; however it is necessary here, since the `Upsert`
    // operator requires that all updates to the same key are processed
    // by the same worker thread and in the same order they were pushed
    // by the client.
    hash_func: Arc<dyn HashFunc<K>>,
}

impl<K, V> Clone for UpsertHandle<K, V>
where
    K: DBData,
    V: DBData,
{
    fn clone(&self) -> Self {
        // Don't clone buffers.
        Self::with_hasher(self.input_handle.clone(), self.hash_func.clone())
    }
}

impl<K, V> UpsertHandle<K, V>
where
    K: DBData,
    V: DBData,
{
    fn new(input_handle: InputHandle<Vec<(K, V)>>) -> Self
    where
        K: Hash,
    {
        Self::with_hasher(
            input_handle,
            Arc::new(|k: &K| default_hash(k) as u32) as Arc<dyn HashFunc<K>>,
        )
    }

    fn with_hasher(
        input_handle: InputHandle<Vec<(K, V)>>,
        hash_func: Arc<dyn HashFunc<K>>,
    ) -> Self {
        Self {
            buffers: vec![Vec::new(); input_handle.0.mailbox.len()],
            input_handle,
            hash_func,
        }
    }

    #[inline]
    fn num_partitions(&self) -> usize {
        self.buffers.len()
    }

    /// Push a single `(key,value)` pair to the input stream.
    pub fn push(&self, k: K, v: V) {
        let num_partitions = self.num_partitions();

        if num_partitions > 1 {
            self.input_handle
                .update_for_worker(((self.hash_func)(&k) as usize) % num_partitions, |tuples| {
                    tuples.push((k, v))
                });
        } else {
            self.input_handle
                .update_for_worker(0, |tuples| tuples.push((k, v)));
        }
    }

    /// Push multiple `(key,value)` pairs to the input stream.
    ///
    /// This is more efficient than pushing values one-by-one using
    /// [`Self::push`].
    ///
    /// # Concurrency
    ///
    /// This method partitions updates across workers and then buffers them
    /// atomically with respect to each worker, i.e., each worker observes
    /// all updates in an `append` at the same logical time.  However the
    /// operation is not atomic as a whole: concurrent `append` and
    /// `clear_input` calls (performed via clones of the
    /// same `UpsertHandle`) may apply in different orders in different
    /// worker threads.  This method is also not atomic with respect to
    /// [`DBSPHandle::step`](`crate::DBSPHandle::step`) and
    /// [`CircuitHandle::step`](`crate::CircuitHandle::step`) methods: a
    /// `DBSPHandle::step` call performed concurrently with `append` may
    /// result in only a subset of the workers observing updates from this
    /// `append` operation.  The remaining updates will appear
    /// during subsequent logical clock cycles.
    pub fn append(&mut self, vals: &mut Vec<(K, V)>) {
        let num_partitions = self.num_partitions();

        if num_partitions > 1 {
            for (k, v) in vals.drain(..) {
                self.buffers[((self.hash_func)(&k) as usize) % num_partitions].push((k, v));
            }
            for worker in 0..num_partitions {
                self.input_handle.update_for_worker(worker, |tuples| {
                    if tuples.is_empty() {
                        *tuples = take(&mut self.buffers[worker]);
                    } else {
                        tuples.append(&mut self.buffers[worker]);
                    }
                })
            }
        } else {
            self.input_handle.update_for_worker(0, |tuples| {
                if tuples.is_empty() {
                    *tuples = take(vals);
                } else {
                    tuples.append(vals);
                }
            });
        }
    }

    /// Clear all inputs buffered since the start of the last clock cycle.
    ///
    /// # Concurrency
    ///
    /// Similar to [`Self::append`], this method atomically clears updates
    /// buffered for each worker thread, i.e., the worker observes all or none
    /// of the updates buffered before the call to `clear_input`; however the
    /// operation is not atomic as a whole: concurrent `append` and
    /// `clear_input` calls (performed via clones of the
    /// same `UpsertHandle`) may apply in different orders in different
    /// worker threads.  This method is also not atomic with respect to
    /// [`DBSPHandle::step`](`crate::DBSPHandle::step`) and
    /// [`CircuitHandle::step`](`crate::CircuitHandle::step`) methods: a
    /// `DBSPHandle::step` call performed concurrently with `clear_input` may
    /// result in only a subset of the workers observing empty inputs, while
    /// other workers observe updates buffered prior to the `clear_input` call.
    pub fn clear_input(&self) {
        self.input_handle.set_for_all(Vec::new());
    }
}

/// Source operator that injects data received via `InputHandle` to the circuit.
///
/// ```text
///                   ┌───────────────────┐
///                   │Circuit            │
///                   │                   │
/// ┌───────────┐     │    ┌─────┐        │
/// │InputHandle├─────────►│Input├─────►  │
/// └───────────┘     │    └─────┘        │
///                   │                   │
///                   └───────────────────┘
/// ```
struct Input<IT, OT, F> {
    mailbox: Mailbox<IT>,
    input_func: F,
    phantom: PhantomData<OT>,
}

impl<IT, OT, F> Input<IT, OT, F>
where
    IT: Default + Clone + Send + 'static,
{
    fn new(input_func: F) -> (Self, InputHandle<IT>) {
        let handle = InputHandle::new();
        let mailbox = handle.mailbox(Runtime::worker_index()).clone();

        let input = Self {
            mailbox,
            input_func,
            phantom: PhantomData,
        };

        (input, handle)
    }
}

impl<IT, OT, F> Operator for Input<IT, OT, F>
where
    IT: 'static,
    OT: 'static,
    F: 'static,
{
    fn name(&self) -> Cow<'static, str> {
        Cow::from("Input")
    }

    fn fixedpoint(&self, _scope: Scope) -> bool {
        false
    }
}

impl<IT, OT, F> SourceOperator<OT> for Input<IT, OT, F>
where
    IT: Default + 'static,
    OT: 'static,
    F: Fn(IT) -> OT + 'static,
{
    fn eval(&mut self) -> OT {
        let v = self.mailbox.take();
        (self.input_func)(v)
    }
}

#[cfg(test)]
mod test {
    use crate::{
        indexed_zset,
        trace::{cursor::Cursor, BatchReader},
        zset, CollectionHandle, InputHandle, OrdIndexedZSet, OrdZSet, RootCircuit, Runtime,
        UpsertHandle,
    };
    use anyhow::Result as AnyResult;
    use std::iter::once;

    fn input_batches() -> Vec<OrdZSet<usize, isize>> {
        vec![
            zset! { 1 => 1, 2 => 1, 3 => 1 },
            zset! { 5 => -1, 10 => 2, 11 => 11 },
            zset! {},
        ]
    }

    fn input_vecs() -> Vec<Vec<(usize, isize)>> {
        input_batches()
            .into_iter()
            .map(|batch| {
                let mut cursor = batch.cursor();
                let mut result = Vec::new();

                while cursor.key_valid() {
                    result.push((*cursor.key(), cursor.weight()));
                    cursor.step_key();
                }
                result
            })
            .collect()
    }

    fn input_test_circuit(
        circuit: &RootCircuit,
        nworkers: usize,
    ) -> AnyResult<InputHandle<OrdZSet<usize, isize>>> {
        let (stream, handle) = circuit.add_input_stream::<OrdZSet<usize, isize>>();

        let mut expected_batches = input_batches()
            .into_iter()
            .chain(input_batches().into_iter())
            .chain(input_batches().into_iter().map(move |batch| {
                let mut result = batch.clone();
                for _ in 1..nworkers {
                    result += batch.clone();
                }
                result
            }));
        stream.gather(0).inspect(move |batch| {
            if Runtime::worker_index() == 0 {
                assert_eq!(batch, &expected_batches.next().unwrap())
            }
        });

        Ok(handle)
    }

    #[test]
    fn input_test_st() {
        let (circuit, input_handle) =
            RootCircuit::build(move |circuit| input_test_circuit(circuit, 1)).unwrap();

        for batch in input_batches().into_iter() {
            input_handle.set_for_worker(0, batch);
            circuit.step().unwrap();
        }

        for batch in input_batches().into_iter() {
            input_handle.update_for_worker(0, |b| *b = batch);
            circuit.step().unwrap();
        }

        for batch in input_batches().into_iter() {
            input_handle.set_for_all(batch);
            circuit.step().unwrap();
        }
    }

    fn input_test_mt(workers: usize) {
        let (mut dbsp, input_handle) =
            Runtime::init_circuit(workers, move |circuit| input_test_circuit(circuit, workers))
                .unwrap();

        for (round, batch) in input_batches().into_iter().enumerate() {
            input_handle.set_for_worker(round % workers, batch);
            dbsp.step().unwrap();
        }

        for (round, batch) in input_batches().into_iter().enumerate() {
            input_handle.update_for_worker(round % workers, |b| *b = batch);
            dbsp.step().unwrap();
        }

        for batch in input_batches().into_iter() {
            input_handle.set_for_all(batch);
            dbsp.step().unwrap();
        }

        dbsp.kill().unwrap();
    }

    #[test]
    fn input_test_mt1() {
        input_test_mt(1);
    }

    #[test]
    fn input_test_mt4() {
        input_test_mt(4);
    }

    fn zset_test_circuit(circuit: &RootCircuit) -> AnyResult<CollectionHandle<usize, isize>> {
        let (stream, handle) = circuit.add_input_zset::<usize, isize>();

        let mut expected_batches = input_batches()
            .into_iter()
            .chain(input_batches().into_iter())
            .chain(once(zset! {}));
        stream.gather(0).inspect(move |batch| {
            if Runtime::worker_index() == 0 {
                assert_eq!(batch, &expected_batches.next().unwrap())
            }
        });

        Ok(handle)
    }

    #[test]
    fn zset_test_st() {
        let (circuit, input_handle) =
            RootCircuit::build(move |circuit| zset_test_circuit(circuit)).unwrap();

        for mut vec in input_vecs().into_iter() {
            input_handle.append(&mut vec);
            circuit.step().unwrap();
        }

        for vec in input_vecs().into_iter() {
            for (k, w) in vec.into_iter() {
                input_handle.push(k, w);
            }
            input_handle.push(5, 1);
            input_handle.push(5, -1);
            circuit.step().unwrap();
        }

        for mut vec in input_vecs().into_iter() {
            input_handle.append(&mut vec);
        }
        input_handle.clear_input();
        circuit.step().unwrap();
    }

    fn zset_test_mt(workers: usize) {
        let (mut dbsp, input_handle) =
            Runtime::init_circuit(workers, |circuit| zset_test_circuit(circuit)).unwrap();

        for mut vec in input_vecs().into_iter() {
            input_handle.append(&mut vec);
            dbsp.step().unwrap();
        }

        for vec in input_vecs().into_iter() {
            for (k, w) in vec.into_iter() {
                input_handle.push(k, w);
            }
            input_handle.push(5, 1);
            input_handle.push(5, -1);
            dbsp.step().unwrap();
        }

        for mut vec in input_vecs().into_iter() {
            input_handle.append(&mut vec);
        }
        input_handle.clear_input();
        dbsp.step().unwrap();

        dbsp.kill().unwrap();
    }

    #[test]
    fn zset_test_mt1() {
        zset_test_mt(1);
    }

    #[test]
    fn zset_test_mt4() {
        zset_test_mt(4);
    }

    fn input_indexed_batches() -> Vec<OrdIndexedZSet<usize, usize, isize>> {
        vec![
            indexed_zset! { 1 => {1 => 1, 2 => 1}, 2 => { 3 => 1 }, 3 => {4 => -1, 5 => 5} },
            indexed_zset! { 5 => {10 => -1}, 10 => {2 => 1, 3 => -1}, 11 => {11 => 11} },
            indexed_zset! {},
        ]
    }

    fn input_indexed_vecs() -> Vec<Vec<(usize, (usize, isize))>> {
        input_indexed_batches()
            .into_iter()
            .map(|batch| {
                let mut cursor = batch.cursor();
                let mut result = Vec::new();

                while cursor.key_valid() {
                    while cursor.val_valid() {
                        result.push((*cursor.key(), (*cursor.val(), cursor.weight())));
                        cursor.step_val();
                    }
                    cursor.step_key();
                }
                result
            })
            .collect()
    }

    fn indexed_zset_test_circuit(
        circuit: &RootCircuit,
    ) -> AnyResult<CollectionHandle<usize, (usize, isize)>> {
        let (stream, handle) = circuit.add_input_indexed_zset::<usize, usize, isize>();

        let mut expected_batches = input_indexed_batches()
            .into_iter()
            .chain(input_indexed_batches().into_iter());
        stream.gather(0).inspect(move |batch| {
            if Runtime::worker_index() == 0 {
                assert_eq!(batch, &expected_batches.next().unwrap())
            }
        });

        Ok(handle)
    }

    #[test]
    fn indexed_zset_test_st() {
        let (circuit, input_handle) =
            RootCircuit::build(move |circuit| indexed_zset_test_circuit(circuit)).unwrap();

        for mut vec in input_indexed_vecs().into_iter() {
            input_handle.append(&mut vec);
            circuit.step().unwrap();
        }

        for vec in input_indexed_vecs().into_iter() {
            for (k, v) in vec.into_iter() {
                input_handle.push(k, v);
            }
            input_handle.push(5, (7, 1));
            input_handle.push(5, (7, -1));
            circuit.step().unwrap();
        }
    }

    fn indexed_zset_test_mt(workers: usize) {
        let (mut dbsp, input_handle) =
            Runtime::init_circuit(workers, |circuit| indexed_zset_test_circuit(circuit)).unwrap();

        for mut vec in input_indexed_vecs().into_iter() {
            input_handle.append(&mut vec);
            dbsp.step().unwrap();
        }

        for vec in input_indexed_vecs().into_iter() {
            for (k, v) in vec.into_iter() {
                input_handle.push(k, v);
            }
            dbsp.step().unwrap();
        }

        dbsp.kill().unwrap();
    }

    #[test]
    fn indexed_zset_test_mt1() {
        indexed_zset_test_mt(1);
    }

    #[test]
    fn indexed_zset_test_mt4() {
        indexed_zset_test_mt(4);
    }

    fn input_set_updates() -> Vec<Vec<(usize, bool)>> {
        vec![
            vec![(1, true), (2, true), (3, false)],
            vec![(1, false), (2, true), (3, true), (4, true)],
            vec![(2, false), (2, true), (3, true), (4, false)],
            vec![(2, true), (2, false)],
        ]
    }

    fn output_set_updates() -> Vec<OrdZSet<usize, isize>> {
        vec![
            zset! { 1 => 1,  2 => 1},
            zset! { 1 => -1, 3 => 1,  4 => 1 },
            zset! { 4 => -1 },
            zset! { 2 => -1 },
        ]
    }

    fn set_test_circuit(circuit: &RootCircuit) -> AnyResult<UpsertHandle<usize, bool>> {
        let (stream, handle) = circuit.add_input_set::<usize, isize>();

        let mut expected_batches = output_set_updates().into_iter();

        stream.gather(0).inspect(move |batch| {
            if Runtime::worker_index() == 0 {
                assert_eq!(batch, &expected_batches.next().unwrap())
            }
        });

        Ok(handle)
    }

    #[test]
    fn set_test_st() {
        let (circuit, mut input_handle) =
            RootCircuit::build(move |circuit| set_test_circuit(circuit)).unwrap();

        for mut vec in input_set_updates().into_iter() {
            input_handle.append(&mut vec);
            circuit.step().unwrap();
        }

        let (circuit, input_handle) =
            RootCircuit::build(move |circuit| set_test_circuit(circuit)).unwrap();

        for vec in input_set_updates().into_iter() {
            for (k, b) in vec.into_iter() {
                input_handle.push(k, b);
            }
            circuit.step().unwrap();
        }
    }

    fn set_test_mt(workers: usize) {
        let (mut dbsp, mut input_handle) =
            Runtime::init_circuit(workers, |circuit| set_test_circuit(circuit)).unwrap();

        for mut vec in input_set_updates().into_iter() {
            input_handle.append(&mut vec);
            dbsp.step().unwrap();
        }

        dbsp.kill().unwrap();

        let (mut dbsp, input_handle) =
            Runtime::init_circuit(workers, |circuit| set_test_circuit(circuit)).unwrap();

        for vec in input_set_updates().into_iter() {
            for (k, b) in vec.into_iter() {
                input_handle.push(k, b);
            }
            dbsp.step().unwrap();
        }

        dbsp.kill().unwrap();
    }

    #[test]
    fn set_test_mt1() {
        set_test_mt(1);
    }

    #[test]
    fn set_test_mt4() {
        set_test_mt(4);
    }

    fn input_map_updates() -> Vec<Vec<(usize, Option<usize>)>> {
        vec![
            vec![(1, Some(1)), (1, Some(2)), (2, None), (3, Some(3))],
            vec![
                (1, Some(1)),
                (1, None),
                (2, Some(2)),
                (3, Some(4)),
                (4, Some(4)),
                (4, None),
                (4, Some(5)),
            ],
            vec![(1, Some(5)), (1, Some(6)), (3, None), (4, Some(6))],
        ]
    }

    fn output_map_updates() -> Vec<OrdIndexedZSet<usize, usize, isize>> {
        vec![
            indexed_zset! { 1 => {2 => 1},  3 => {3 => 1}},
            indexed_zset! { 1 => {2 => -1}, 2 => {2 => 1}, 3 => {3 => -1, 4 => 1}, 4 => {5 => 1}},
            indexed_zset! { 1 => {6 => 1},  3 => {4 => -1}, 4 => {5 => -1, 6 => 1}},
        ]
    }

    fn map_test_circuit(circuit: &RootCircuit) -> AnyResult<UpsertHandle<usize, Option<usize>>> {
        let (stream, handle) = circuit.add_input_map::<usize, usize, isize>();

        let mut expected_batches = output_map_updates().into_iter();

        stream.gather(0).inspect(move |batch| {
            if Runtime::worker_index() == 0 {
                assert_eq!(batch, &expected_batches.next().unwrap())
            }
        });

        Ok(handle)
    }

    #[test]
    fn map_test_st() {
        let (circuit, mut input_handle) =
            RootCircuit::build(move |circuit| map_test_circuit(circuit)).unwrap();

        for mut vec in input_map_updates().into_iter() {
            input_handle.append(&mut vec);
            circuit.step().unwrap();
        }

        let (circuit, input_handle) =
            RootCircuit::build(move |circuit| map_test_circuit(circuit)).unwrap();

        for vec in input_map_updates().into_iter() {
            for (k, v) in vec.into_iter() {
                input_handle.push(k, v);
            }
            circuit.step().unwrap();
        }
    }

    fn map_test_mt(workers: usize) {
        let (mut dbsp, mut input_handle) =
            Runtime::init_circuit(workers, |circuit| map_test_circuit(circuit)).unwrap();

        for mut vec in input_map_updates().into_iter() {
            input_handle.append(&mut vec);
            dbsp.step().unwrap();
        }

        dbsp.kill().unwrap();

        let (mut dbsp, input_handle) =
            Runtime::init_circuit(workers, |circuit| map_test_circuit(circuit)).unwrap();

        for vec in input_map_updates().into_iter() {
            for (k, v) in vec.into_iter() {
                input_handle.push(k, v);
            }
            dbsp.step().unwrap();
        }

        dbsp.kill().unwrap();
    }

    #[test]
    fn map_test_mt1() {
        map_test_mt(1);
    }

    #[test]
    fn map_test_mt4() {
        map_test_mt(4);
    }
}
