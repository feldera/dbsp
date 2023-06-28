//! Exchange operators implement a N-to-N communication pattern where
//! each participant sends exactly one value to and receives exactly one
//! value from each peer at every clock cycle.

// TODO: We may want to generalize these operators to implement N-to-M
// communication, including 1-to-N and N-to-1.

use crate::{
    circuit::{
        metadata::OperatorLocation,
        operator_traits::{Operator, SinkOperator, SourceOperator},
        Host, LocalStoreMarker, OwnershipPreference, Runtime, Scope,
    },
    circuit_cache_key,
};
use bincode::{decode_from_slice, Decode, Encode};

use crossbeam_utils::CachePadded;
use futures::{future, prelude::*};
use once_cell::sync::{Lazy, OnceCell};
use std::{
    borrow::Cow,
    collections::HashMap,
    marker::PhantomData,
    net::SocketAddr,
    ops::Range,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex, RwLock,
    },
};
use tarpc::{
    client, context,
    serde_transport::tcp::{connect, listen},
    server::{self, Channel},
    tokio_serde::formats::Bincode,
    tokio_util::sync::{CancellationToken, DropGuard},
};
use tokio::{
    runtime::Runtime as TokioRuntime,
    sync::{Notify, OnceCell as TokioOnceCell},
    time::sleep,
};
use typedmap::TypedMapKey;

// All circuits can share a single Tokio runtime.
static TOKIO: Lazy<TokioRuntime> = Lazy::new(|| TokioRuntime::new().unwrap());

// We use the `Runtime::local_store` mechanism to connect multiple workers
// to an `Exchange` instance.  During circuit construction, each worker
// allocates a unique id that happens to be the same across all workers.
// The worker then allocates a new `Exchange` and adds it to the local store
// using the id as a key.  If there already is an `Exchange` with this id in
// the store, created by another worker, a reference to that `Exchange` will
// be used instead.
circuit_cache_key!(local ExchangeCacheId<T>(ExchangeId => Arc<Exchange<T>>));

#[tarpc::service]
trait ExchangeService {
    /// Sends messages in `exchange_id` from all of the worker threads in
    /// `senders` to all of the worker thread receivers in the server that
    /// processes the message.  The Bincode-encoded message from `sender` to
    /// `receiver` is `data[sender - senders.start][receiver -
    /// receivers.start]`, where `receivers` is the `Range<usize>` of worker
    /// thread IDs in the server that processes the message.
    async fn exchange(exchange_id: usize, senders: Range<usize>, data: Vec<Vec<Vec<u8>>>);
}

type ExchangeId = usize;

// Maps from an `exchange_id` to the `Inner` that implements the exchange.
type ExchangeDirectory = Arc<RwLock<HashMap<ExchangeId, Arc<InnerExchange>>>>;

#[derive(Clone)]
struct ExchangeServer(ExchangeDirectory);

#[tarpc::server]
impl ExchangeService for ExchangeServer {
    async fn exchange(
        self,
        _: context::Context,
        exchange_id: ExchangeId,
        senders: Range<usize>,
        data: Vec<Vec<Vec<u8>>>,
    ) {
        let inner = self.0.read().unwrap().get(&exchange_id).unwrap().clone();
        inner.received(senders, data).await;
    }
}

// Maps from a range of worker IDs to the RPC client used to contact those
// workers.  Only worker IDs for remote workers appear in the map.
struct Clients(Vec<(Host, TokioOnceCell<ExchangeServiceClient>)>);

impl Clients {
    fn new(runtime: &Runtime) -> Clients {
        Self(
            runtime
                .layout()
                .other_hosts()
                .map(|host| (host.clone(), TokioOnceCell::new()))
                .collect(),
        )
    }

    /// Returns a client for `worker`, which must be a remote worker ID, first
    /// establishing a connection if there isn't one yet.
    async fn connect(&self, worker: usize) -> &ExchangeServiceClient {
        let (host, cell) = self
            .0
            .iter()
            .find(|(host, _client)| host.workers.contains(&worker))
            .unwrap();
        cell.get_or_init(|| async {
            let transport = loop {
                let mut transport = connect(host.address, Bincode::default);
                transport.config_mut().max_frame_length(usize::MAX);
                match transport.await {
                    Ok(transport) => break transport,
                    Err(error) => println!(
                        "connection to {} failed ({error}), waiting to retry",
                        host.address
                    ),
                }
                sleep(std::time::Duration::from_millis(1000)).await;
            };
            ExchangeServiceClient::new(client::Config::default(), transport).spawn()
        })
        .await
    }
}

struct InnerExchange {
    exchange_id: ExchangeId,
    /// The number of communicating peers.
    npeers: usize,
    /// Range of worker IDs on the local host.
    local_workers: Range<usize>,
    /// Counts the number of messages yet to be received in the current round of
    /// communication per receiver.  The receiver must wait until it has all
    /// `npeers` messages before reading all of them from mailboxes in one
    /// pass.
    receiver_counters: Vec<AtomicUsize>,
    /// Callback invoked when all `npeers` messages are ready for a receiver.
    receiver_callbacks: Vec<OnceCell<Box<dyn Fn() + Send + Sync>>>,
    /// Counts the number of empty mailboxes ready to accept new data per
    /// sender. The sender waits until it has `npeers` available mailboxes
    /// before writing all of them in one pass.
    sender_counters: Vec<CachePadded<AtomicUsize>>,
    /// Callback invoked when all `npeers` mailboxes are available.
    sender_callbacks: Vec<OnceCell<Box<dyn Fn() + Send + Sync>>>,
    /// The number of workers that have already sent their messages in the
    /// current round.
    sent: AtomicUsize,
    /// The RPC clients to contact remote hosts.
    clients: Arc<Clients>,
    /// This allows the `exchange` RPC to wait until the receiver has taken its
    /// data out of the mailbox.  There are `n_remote_workers * n_local_workers`
    /// elements.
    sender_notifies: Vec<Notify>,
    /// A callback that takes the raw data exchanged over RPC and deserializes
    /// and delivers it to the receiver's mailbox.
    deliver: Box<dyn Fn(Vec<u8>, usize, usize) + Send + Sync + 'static>,
}

impl InnerExchange {
    fn new(
        exchange_id: ExchangeId,
        deliver: impl Fn(Vec<u8>, usize, usize) + Send + Sync + 'static,
        clients: Arc<Clients>,
    ) -> InnerExchange {
        let runtime = Runtime::runtime().unwrap();
        let npeers = runtime.num_workers();
        let local_workers = runtime.layout().local_workers();
        let n_local_workers = local_workers.len();
        let n_remote_workers = npeers - n_local_workers;
        Self {
            exchange_id,
            npeers,
            local_workers,
            clients,
            receiver_counters: (0..npeers).map(|_| AtomicUsize::new(0)).collect(),
            receiver_callbacks: (0..npeers).map(|_| OnceCell::new()).collect(),
            sender_notifies: (0..n_local_workers * n_remote_workers)
                .map(|_| Notify::new())
                .collect(),
            sender_counters: (0..npeers)
                .map(|_| CachePadded::new(AtomicUsize::new(npeers)))
                .collect(),
            sender_callbacks: (0..npeers).map(|_| OnceCell::new()).collect(),
            deliver: Box::new(deliver),
            sent: AtomicUsize::new(0),
        }
    }

    /// Returns the `sender_notify` for a sender/receiver pair.  `receiver`
    /// must be a local worker ID, and `sender` must be a remote worker ID.
    fn sender_notify(&self, sender: usize, receiver: usize) -> &Notify {
        debug_assert!(sender < self.npeers && !self.local_workers.contains(&sender));
        debug_assert!(self.local_workers.contains(&receiver));
        let n_local_workers = self.local_workers.len();
        let sender_ofs = if sender >= self.local_workers.start {
            sender - n_local_workers
        } else {
            sender
        };
        let receiver_ofs = receiver - self.local_workers.start;
        &self.sender_notifies[sender_ofs * n_local_workers + receiver_ofs]
    }

    /// Receives messages sent from all of the worker threads in `senders` to
    /// all of the local worker threads `receivers` in `self`.  The
    /// Bincode-encoded `Vec<u8>` message from `sender` to `receiver` is
    /// `data[sender - senders.start][receiver - receivers.start]`.
    async fn received(self: &Arc<Self>, senders: Range<usize>, data: Vec<Vec<Vec<u8>>>) {
        let receivers = &self.local_workers;

        // Deliver all of the data into the exchange's mailboxes.
        for (sender, data) in senders.clone().zip(data.into_iter()) {
            assert_eq!(data.len(), receivers.len());
            for (receiver, data) in receivers.clone().zip(data.into_iter()) {
                (self.deliver)(data, sender, receiver);
            }
        }

        // Increment the receiver counters and deliver callbacks if necessary.
        for receiver in receivers.clone() {
            let n = senders.len();
            let old_counter = self.receiver_counters[receiver].fetch_add(n, Ordering::AcqRel);
            if old_counter >= self.npeers - n {
                if let Some(cb) = self.receiver_callbacks[receiver].get() {
                    cb()
                }
            }
        }

        // Wait for the receivers to pick up their mail before returning.
        for sender in senders {
            for receiver in receivers.clone() {
                self.sender_notify(sender, receiver).notified().await;
            }
        }
    }

    /// Returns an index for the sender/receiver pair.
    fn mailbox_index(&self, sender: usize, receiver: usize) -> usize {
        debug_assert!(sender < self.npeers);
        debug_assert!(receiver < self.npeers);
        sender * self.npeers + receiver
    }

    fn ready_to_send(&self, sender: usize) -> bool {
        debug_assert!(self.local_workers.contains(&sender));
        self.sender_counters[sender].load(Ordering::Acquire) == self.npeers
    }

    fn ready_to_receive(&self, receiver: usize) -> bool {
        debug_assert!(receiver < self.npeers);
        self.receiver_counters[receiver].load(Ordering::Acquire) == self.npeers
    }

    fn register_sender_callback<F>(&self, sender: usize, cb: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        debug_assert!(sender < self.npeers);
        debug_assert!(self.sender_callbacks[sender].get().is_none());
        let res = self.sender_callbacks[sender].set(Box::new(cb) as Box<dyn Fn() + Send + Sync>);
        debug_assert!(res.is_ok());
    }

    fn register_receiver_callback<F>(&self, receiver: usize, cb: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        debug_assert!(receiver < self.npeers);
        debug_assert!(self.receiver_callbacks[receiver].get().is_none());
        let res =
            self.receiver_callbacks[receiver].set(Box::new(cb) as Box<dyn Fn() + Send + Sync>);
        debug_assert!(res.is_ok());
    }
}

/// `Exchange` is an N-to-N communication primitive that partitions data across
/// multiple concurrent threads.
///
/// An instance of `Exchange` can be shared by multiple threads that communicate
/// in rounds.  In each round each peer _first_ sends exactly one data value to
/// every other peer (and itself) and then receives one value from each peer.
/// The send operation can only proceed when all peers have retrieved data
/// produced at the previous round.  Likewise, the receive operation can proceed
/// once all incoming values are ready for the current round.
///
/// Each worker has one ExchangeServiceClient and ExchangeServer for every
/// worker (including itself), so N*N total.
///
/// In a round, each worker invokes exchange() once on each of its clients.
/// Each server handles N calls to exchange(), once for each other worker and
/// itself.
///
/// Each call to exchange populates a mailbox.  When all the mailboxes for a
/// worker have been populated, it can read and clear them.
pub(crate) struct Exchange<T> {
    inner: Arc<InnerExchange>,
    /// `npeers^2` mailboxes, one for each sender/receiver pair.  Each mailbox
    /// is accessed by exactly two threads, so contention is low.
    ///
    /// We only use the mailboxes where either the sender or the receiver is one
    /// of our local workers. In the diagram below, L is mailboxes used for
    /// local exchange, S mailboxes used for sending RPC exchange, and R
    /// mailboxes used for receiving exchange via RPC:
    ///
    /// ```text
    ///           <-------receivers------->
    ///                  local
    ///                 workers
    /// ^         -------------------------
    /// |         |     |RRRRR|     |     |
    ///           |     |RRRRR|     |     |
    /// s         |-----|-----|-----|-----|
    /// e  local  |SSSSS|LLLLL|SSSSS|SSSSS|
    /// n workers |SSSSS|LLLLL|SSSSS|SSSSS|
    /// d         |-----|-----|-----|-----|
    /// e         |     |RRRRR|     |     |
    /// r         |     |RRRRR|     |     |
    /// s         |-----|-----|-----|-----|
    ///           |     |RRRRR|     |     |
    /// |         |     |RRRRR|     |     |
    /// v         |-----|-----|-----|-----|
    /// ```
    mailboxes: Arc<Vec<Mutex<Option<T>>>>,
}

struct ExchangeListener(DropGuard);

impl ExchangeListener {
    fn new(address: SocketAddr, directory: ExchangeDirectory) -> Self {
        let token = CancellationToken::new();
        let drop = token.clone().drop_guard();
        TOKIO.spawn(async move {
            println!("listening on {address}");
            let mut listener = listen(address, Bincode::default).await.unwrap();
            listener.config_mut().max_frame_length(usize::MAX);
            let incoming = listener
                .filter_map(|r| future::ready(r.ok()))
                .map(server::BaseChannel::with_defaults)
                .map(move |channel| {
                    let server = ExchangeServer(directory.clone());
                    channel.execute(server.serve())
                })
                .buffer_unordered(10)
                .for_each(|_| async {});
            tokio::select! {
                _ = incoming => {}
                _ = token.cancelled() => {}
            }
        });
        Self(drop)
    }
}

impl<T> Exchange<T>
where
    T: Clone + Send + Encode + Decode + 'static,
{
    /// Create a new exchange operator for `npeers` communicating threads.
    fn new(exchange_id: ExchangeId, clients: Arc<Clients>, directory: ExchangeDirectory) -> Self {
        let npeers = Runtime::runtime().unwrap().num_workers();
        let mailboxes: Arc<Vec<Mutex<Option<T>>>> =
            Arc::new((0..npeers * npeers).map(|_| Mutex::new(None)).collect());
        let mailboxes2: Arc<Vec<Mutex<Option<T>>>> = mailboxes.clone();
        let deliver = move |data: Vec<u8>, sender, receiver| {
            let index: usize = sender * npeers + receiver;
            let data = decode_from_slice(&data, bincode::config::standard())
                .unwrap()
                .0;
            let mut mailbox = mailboxes2[index].lock().unwrap();
            assert!((*mailbox).is_none());
            *mailbox = Some(data);
        };

        let inner = Arc::new(InnerExchange::new(exchange_id, deliver, clients));
        directory
            .write()
            .unwrap()
            .entry(exchange_id)
            .and_modify(|_| panic!())
            .or_insert(inner.clone());
        Self { inner, mailboxes }
    }

    /// Returns a reference to a mailbox for the sender/receiver pair.
    fn mailbox(&self, sender: usize, receiver: usize) -> &Mutex<Option<T>> {
        &self.mailboxes[self.inner.mailbox_index(sender, receiver)]
    }

    /// Create a new `Exchange` instance if an instance with the same id
    /// (created by another thread) does not yet exist within `runtime`.
    /// The number of peers will be set to `runtime.num_workers()`.
    pub(crate) fn with_runtime(runtime: &Runtime, exchange_id: ExchangeId) -> Arc<Self> {
        let directory = runtime
            .local_store()
            .entry(DirectoryId)
            .or_insert_with(|| Arc::new(RwLock::new(HashMap::new())))
            .clone();

        runtime.local_store().entry(ListenerId).or_insert_with(|| {
            // Create a listener for remote exchange to connect to us.
            runtime
                .layout()
                .local_address()
                .map(|address| ExchangeListener::new(address, directory.clone()))
        });

        let clients = runtime
            .local_store()
            .entry(ClientsId)
            .or_insert_with(|| {
                // Create clients for remote exchange.
                Arc::new(Clients::new(runtime))
            })
            .clone();

        runtime
            .local_store()
            .entry(ExchangeCacheId::new(exchange_id))
            .or_insert_with(|| Arc::new(Exchange::new(exchange_id, clients.clone(), directory)))
            .value()
            .clone()
    }

    /// True if all `sender`'s outgoing mailboxes are free and ready to accept
    /// data.
    ///
    /// Once this function returns true, a subsequent `try_send_all` operation
    /// is guaranteed to succeed for `sender`.
    fn ready_to_send(&self, sender: usize) -> bool {
        self.inner.ready_to_send(sender)
    }

    /// Write all outgoing messages for `sender` to mailboxes.
    ///
    /// Values to be sent are retrieved from the `data` iterator, with the
    /// first value delivered to receiver 0, second value delivered to receiver
    /// 1, and so on.
    ///
    /// # Errors
    ///
    /// Fails if at least one of the sender's outgoing mailboxes is not empty.
    ///
    /// # Panics
    ///
    /// Panics if `data` yields fewer than `self.npeers` items.
    pub(crate) fn try_send_all<I>(self: &Arc<Self>, sender: usize, data: &mut I) -> bool
    where
        I: Iterator<Item = T> + Send,
    {
        let npeers = self.inner.npeers;
        if self.inner.sender_counters[sender]
            .compare_exchange(npeers, 0, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return false;
        }

        // Deliver all of the data to local mailboxes.
        let local_workers = &self.inner.local_workers;
        for receiver in 0..npeers {
            *self.mailbox(sender, receiver).lock().unwrap() = data.next();

            if local_workers.contains(&receiver) {
                let old_counter =
                    self.inner.receiver_counters[receiver].fetch_add(1, Ordering::AcqRel);
                if old_counter >= npeers - 1 {
                    if let Some(cb) = self.inner.receiver_callbacks[receiver].get() {
                        cb()
                    }
                }
            }
        }

        // In a single-host layout, or if some of our local workers haven't yet
        // sent in this round, we're all done for now.
        if npeers == local_workers.len()
            || self.inner.sent.fetch_add(1, Ordering::AcqRel) + 1 != local_workers.len()
        {
            return true;
        }
        self.inner.sent.store(0, Ordering::Release);

        // All of the local workers have sent their data in this round.  Take
        // all of their data and send it to the remote hosts.
        let this = self.clone();
        let runtime = Runtime::runtime().unwrap();
        TOKIO.spawn(async move {
            let mut futures = Vec::new();

            // For each range of worker IDs `receivers` on a remote host,
            // accumulate all of the data from our local `senders` to all
            // of the `receivers` on that host.
            let senders = &this.inner.local_workers;
            for host in runtime.layout().other_hosts() {
                let receivers = &host.workers;
                let items: Vec<Vec<_>> = senders
                    .clone()
                    .map(|sender| {
                        receivers
                            .clone()
                            .map(|receiver| {
                                let item = this
                                    .mailbox(sender, receiver)
                                    .lock()
                                    .unwrap()
                                    .take()
                                    .unwrap();
                                bincode::encode_to_vec(item, bincode::config::standard()).unwrap()
                            })
                            .collect()
                    })
                    .collect();

                let client = this.inner.clients.connect(receivers.start).await;

                // Send it.
                futures.push(client.exchange(
                    context::current(),
                    this.inner.exchange_id,
                    senders.clone(),
                    items,
                ));
            }

            // Wait for each send to complete.
            for future in futures {
                future.await.unwrap();
            }

            // Record that the sends completed.
            let n = npeers - senders.len();
            for sender in senders.clone() {
                let old_counter = this.inner.sender_counters[sender].fetch_add(n, Ordering::AcqRel);
                if old_counter >= npeers - n {
                    if let Some(cb) = this.inner.sender_callbacks[sender].get() {
                        cb()
                    }
                }
            }
        });

        true
    }

    /// True if all `receiver`'s incoming mailboxes contain data.
    ///
    /// Once this function returns true, a subsequent `try_receive_all`
    /// operation is guaranteed for `receiver`.
    pub(crate) fn ready_to_receive(&self, receiver: usize) -> bool {
        self.inner.ready_to_receive(receiver)
    }

    /// Read all incoming messages for `receiver`.
    ///
    /// Values are passed to callback function `cb`.
    ///
    /// # Errors
    ///
    /// Fails if at least one of the receiver's incoming mailboxes is empty.
    pub(crate) fn try_receive_all<F>(&self, receiver: usize, mut cb: F) -> bool
    where
        F: FnMut(T),
    {
        let npeers = self.inner.npeers;
        if self.inner.receiver_counters[receiver]
            .compare_exchange(npeers, 0, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return false;
        }

        for sender in 0..self.inner.npeers {
            let data = self
                .mailbox(sender, receiver)
                .lock()
                .unwrap()
                .take()
                .unwrap();
            cb(data);
            if self.inner.local_workers.contains(&sender) {
                let old_counter = self.inner.sender_counters[sender].fetch_add(1, Ordering::AcqRel);
                if old_counter >= self.inner.npeers - 1 {
                    if let Some(cb) = self.inner.sender_callbacks[sender].get() {
                        cb()
                    }
                }
            } else {
                self.inner.sender_notify(sender, receiver).notify_one();
            }
        }
        true
    }

    /// Register callback to be invoked whenever the `ready_to_send` condition
    /// becomes true.
    ///
    /// The callback can be setup at most once (e.g., when a scheduler attaches
    /// to the circuit) and cannot be unregistered.  Notifications delivered
    /// before the callback is registered are lost.  The client should call
    /// `ready_to_send` after installing the callback to check the status.
    ///
    /// After the callback has been registered, notifications are delivered with
    /// at-least-once semantics: a notification is generated whenever the
    /// status changes from not ready to ready, but spurious notifications
    /// can occur occasionally.  Therefore, the user must check the status
    /// explicitly by calling `ready_to_send` or be prepared that `try_send_all`
    /// can fail.
    pub(crate) fn register_sender_callback<F>(&self, sender: usize, cb: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.inner.register_sender_callback(sender, cb)
    }

    /// Register callback to be invoked whenever the `ready_to_receive`
    /// condition becomes true.
    ///
    /// The callback can be setup at most once (e.g., when a scheduler attaches
    /// to the circuit) and cannot be unregistered.  Notifications delivered
    /// before the callback is registered are lost.  The client should call
    /// `ready_to_receive` after installing the callback to check
    /// the status.
    ///
    /// After the callback has been registered, notifications are delivered with
    /// at-least-once semantics: a notification is generated whenever the
    /// status changes from not ready to ready, but spurious notifications
    /// can occur occasionally.  The user must check the status explicitly
    /// by calling `ready_to_receive` or be prepared that `try_receive_all`
    /// can fail.
    pub(crate) fn register_receiver_callback<F>(&self, receiver: usize, cb: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.inner.register_receiver_callback(receiver, cb)
    }
}

/// Operator that partitions incoming data across all workers.
///
/// This operator works in tandem with [`ExchangeReceiver`], which reassembles
/// the data on the receiving side.  Together they implement an all-to-all
/// comunication mechanism, where at every clock cycle each worker partitions
/// its incoming data into `N` values, one for each worker, using a
/// user-provided closure.  It then reads values sent to it by all peers and
/// reassembles them into a single value using another user-provided closure.
///
/// The exchange mechanism is split into two operators, so that after sending
/// the data the circuit does not need to block waiting for its peers to finish
/// sending and can instead schedule other operators.
///
/// ```text
///                    ExchangeSender  ExchangeReceiver
///                       ┌───────┐      ┌───────┐
///                       │       │      │       │
///        ┌───────┐      │       │      │       │          ┌───────┐
///        │source ├─────►│       │      │       ├─────────►│ sink  │
///        └───────┘      │       │      │       │          └───────┘
///                       │       ├───┬─►│       │
///                       │       │   │  │       │
///                       └───────┘   │  └───────┘
/// WORKER 1                          │
/// ──────────────────────────────────┼──────────────────────────────
/// WORKER 2                          │
///                                   │
///                       ┌───────┐   │  ┌───────┐
///                       │       ├───┴─►│       │
///        ┌───────┐      │       │      │       │          ┌───────┐
///        │source ├─────►│       │      │       ├─────────►│ sink  │
///        └───────┘      │       │      │       │          └───────┘
///                       │       │      │       │
///                       │       │      │       │
///                       └───────┘      └───────┘
///                    ExchangeSender  ExchangeReceiver
/// ```
///
/// `ExchangeSender` is an asynchronous operator., i.e.,
/// [`ExchangeSender::is_async`] returns `true`.  It becomes schedulable
/// ([`ExchangeSender::ready`] returns `true`) once all peers have retrieved
/// values written by the operator in the previous clock cycle.  The scheduler
/// should use [`ExchangeSender::register_ready_callback`] to get notified when
/// the operator becomes schedulable.
///
/// `ExchangeSender` doesn't have a public constructor and must be instantiated
/// using the [`new_exchange_operators`] function, which creates an
/// [`ExchangeSender`]/[`ExchangeReceiver`] pair of operators and connects them
/// to their counterparts in other workers as in the diagram above.
///
/// An [`ExchangeSender`]/[`ExchangeReceiver`] pair is added to a circuit using
/// the [`Circuit::add_exchange`](`crate::circuit::Circuit::add_exchange`)
/// method, which registers a dependency between them, making sure that
/// `ExchangeSender` is evaluated before `ExchangeReceiver`.
///
/// # Examples
///
/// The following example instantiates the circuit in the diagram above.
///
/// ```
/// # #[cfg(miri)]
/// # fn main() {}
///
/// # #[cfg(not(miri))]
/// # fn main() {
/// use dbsp::{
///     operator::{communication::new_exchange_operators, Generator},
///     Circuit, RootCircuit, Runtime,
/// };
///
/// const WORKERS: usize = 16;
/// const ROUNDS: usize = 10;
///
/// let hruntime = Runtime::run(WORKERS, || {
///     let circuit = RootCircuit::build(|circuit| {
///         // Create a data source that generates numbers 0, 1, 2, ...
///         let mut n: usize = 0;
///         let source = circuit.add_source(Generator::new(move || {
///             let result = n;
///             n += 1;
///             result
///         }));
///
///         // Create an `ExchangeSender`/`ExchangeReceiver pair`.
///         let (sender, receiver) = new_exchange_operators(
///             &Runtime::runtime().unwrap(),
///             Runtime::worker_index(),
///             None,
///             // Partitioning function sends a copy of the input `n` to each peer.
///             |n, output| {
///                 for _ in 0..WORKERS {
///                     output.push(n)
///                 }
///             },
///             // Reassemble received values into a vector.
///             |v: &mut Vec<usize>, n| v.push(n),
///         );
///
///         // Add exchange operators to the circuit.
///         let combined = circuit.add_exchange(sender, receiver, &source);
///         let mut round = 0;
///
///         // Expected output stream of`ExchangeReceiver`:
///         // [0,0,0,...]
///         // [1,1,1,...]
///         // [2,2,2,...]
///         // ...
///         combined.inspect(move |v| {
///             assert_eq!(&vec![round; WORKERS], v);
///             round += 1;
///         });
///         Ok(())
///     })
///     .unwrap()
///     .0;
///
///     for _ in 1..ROUNDS {
///         circuit.step();
///     }
/// });
///
/// hruntime.join().unwrap();
/// # }
/// ```
pub struct ExchangeSender<D, T, L>
where
    T: Send + Encode + Decode + 'static + Clone,
{
    worker_index: usize,
    location: OperatorLocation,
    partition: L,
    outputs: Vec<T>,
    exchange: Arc<Exchange<T>>,
    phantom: PhantomData<D>,
}

impl<D, T, L> ExchangeSender<D, T, L>
where
    T: Send + Encode + Decode + 'static + Clone,
{
    fn new(
        runtime: &Runtime,
        worker_index: usize,
        location: OperatorLocation,
        exchange_id: ExchangeId,
        partition: L,
    ) -> Self {
        debug_assert!(worker_index < runtime.num_workers());
        Self {
            worker_index,
            location,
            partition,
            outputs: Vec::with_capacity(runtime.num_workers()),
            exchange: Exchange::with_runtime(runtime, exchange_id),
            phantom: PhantomData,
        }
    }
}

impl<D, T, L> Operator for ExchangeSender<D, T, L>
where
    D: 'static,
    T: Send + Encode + Decode + 'static + Clone,
    L: 'static,
{
    fn name(&self) -> Cow<'static, str> {
        Cow::from("ExchangeSender")
    }

    fn location(&self) -> OperatorLocation {
        self.location
    }

    fn clock_start(&mut self, _scope: Scope) {}
    fn clock_end(&mut self, _scope: Scope) {}

    fn is_async(&self) -> bool {
        true
    }

    fn register_ready_callback<F>(&mut self, cb: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.exchange
            .register_sender_callback(self.worker_index, cb)
    }

    fn ready(&self) -> bool {
        self.exchange.ready_to_send(self.worker_index)
    }

    fn fixedpoint(&self, _scope: Scope) -> bool {
        true
    }
}

impl<D, T, L> SinkOperator<D> for ExchangeSender<D, T, L>
where
    D: Clone + 'static,
    T: Clone + Send + Encode + Decode + 'static,
    L: FnMut(D, &mut Vec<T>) + 'static,
{
    fn eval(&mut self, input: &D) {
        self.eval_owned(input.clone());
    }

    fn eval_owned(&mut self, input: D) {
        debug_assert!(self.ready());
        self.outputs.clear();
        (self.partition)(input, &mut self.outputs);
        let res = self
            .exchange
            .try_send_all(self.worker_index, &mut self.outputs.drain(..));
        debug_assert!(res);
    }

    fn input_preference(&self) -> OwnershipPreference {
        OwnershipPreference::PREFER_OWNED
    }
}

/// Operator that receives values sent by the `ExchangeSender` operator and
/// assembles them into a single output value.
///
/// See [`ExchangeSender`] documentation for details.
///
/// `ExchangeReceiver` is an asynchronous operator., i.e.,
/// [`ExchangeReceiver::is_async`] returns `true`.  It becomes schedulable
/// ([`ExchangeReceiver::ready`] returns `true`) once all peers have sent values
/// for this worker in the current clock cycle.  The scheduler should use
/// [`ExchangeReceiver::register_ready_callback`] to get notified when the
/// operator becomes schedulable.
pub struct ExchangeReceiver<T, L>
where
    T: Send + Encode + Decode + 'static + Clone,
{
    worker_index: usize,
    location: OperatorLocation,
    combine: L,
    exchange: Arc<Exchange<T>>,
}

impl<T, L> ExchangeReceiver<T, L>
where
    T: Send + Encode + Decode + 'static + Clone,
{
    fn new(
        runtime: &Runtime,
        worker_index: usize,
        location: OperatorLocation,
        exchange_id: ExchangeId,
        combine: L,
    ) -> Self {
        debug_assert!(worker_index < runtime.num_workers());

        Self {
            worker_index,
            location,
            combine,
            exchange: Exchange::with_runtime(runtime, exchange_id),
        }
    }
}

impl<T, L> Operator for ExchangeReceiver<T, L>
where
    T: Send + Encode + Decode + 'static + Clone,
    L: 'static,
{
    fn name(&self) -> Cow<'static, str> {
        Cow::from("ExchangeReceiver")
    }

    fn location(&self) -> OperatorLocation {
        self.location
    }

    fn is_async(&self) -> bool {
        true
    }

    fn register_ready_callback<F>(&mut self, cb: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.exchange
            .register_receiver_callback(self.worker_index, cb)
    }

    fn ready(&self) -> bool {
        self.exchange.ready_to_receive(self.worker_index)
    }

    fn fixedpoint(&self, _scope: Scope) -> bool {
        true
    }
}

impl<D, T, L> SourceOperator<D> for ExchangeReceiver<T, L>
where
    D: Default + Clone,
    T: Clone + Send + Encode + Decode + 'static,
    L: Fn(&mut D, T) + 'static,
{
    fn eval(&mut self) -> D {
        debug_assert!(self.ready());
        let mut combined = Default::default();
        let res = self
            .exchange
            .try_receive_all(self.worker_index, |x| (self.combine)(&mut combined, x));

        debug_assert!(res);
        combined
    }
}

#[derive(Hash, PartialEq, Eq)]
struct ClientsId;

impl TypedMapKey<LocalStoreMarker> for ClientsId {
    type Value = Arc<Clients>;
}

#[derive(Hash, PartialEq, Eq)]
struct ListenerId;

impl TypedMapKey<LocalStoreMarker> for ListenerId {
    type Value = Option<ExchangeListener>;
}

#[derive(Hash, PartialEq, Eq)]
struct DirectoryId;

impl TypedMapKey<LocalStoreMarker> for DirectoryId {
    type Value = ExchangeDirectory;
}

/// Create an [`ExchangeSender`]/[`ExchangeReceiver`] operator pair.
///
/// See [`ExchangeSender`] documentation for details and example usage.
///
/// # Arguments
///
/// * `runtime` - [`Runtime`](`crate::circuit::Runtime`) within which operators
///   are created.
/// * `worker_index` - index of the current worker.
/// * `partition` - partitioning logic that, for each element of the input
///   stream, returns an iterator with exactly `runtime.num_workers()` values.
/// * `combine` - re-assemble logic that combines values received from all peers
///   into a single output value.
///
/// # Type arguments
/// * `TI` - Type of values in the input stream consumed by `ExchangeSender`.
/// * `TO` - Type of values in the output stream produced by `ExchangeReceiver`.
/// * `TE` - Type of values sent across workers.
/// * `PL` - Type of closure that splits a value of type `TI` into
///   `runtime.num_workers()` values of type `TE`.
/// * `I` - Iterator returned by `PL`.
/// * `CL` - Type of closure that folds `num_workers` values of type `TE` into a
///   value of type `TO`.
pub fn new_exchange_operators<TI, TO, TE, PL, CL>(
    runtime: &Runtime,
    worker_index: usize,
    location: OperatorLocation,
    partition: PL,
    combine: CL,
) -> (ExchangeSender<TI, TE, PL>, ExchangeReceiver<TE, CL>)
where
    TO: Default + Clone,
    TE: Send + Encode + Decode + 'static + Clone,
    PL: FnMut(TI, &mut Vec<TE>) + 'static,
    CL: Fn(&mut TO, TE) + 'static,
{
    let exchange_id = runtime.sequence_next(worker_index);
    let sender = ExchangeSender::new(runtime, worker_index, location, exchange_id, partition);
    let receiver = ExchangeReceiver::new(runtime, worker_index, location, exchange_id, combine);
    (sender, receiver)
}

#[cfg(test)]
mod tests {
    use super::Exchange;
    use crate::{
        circuit::{
            schedule::{DynamicScheduler, Scheduler, StaticScheduler},
            Runtime,
        },
        operator::{communication::new_exchange_operators, Generator},
        Circuit, RootCircuit,
    };
    use std::thread::yield_now;

    // We decrease the number of rounds we do when we're running under miri,
    // otherwise it'll run forever
    const ROUNDS: usize = if cfg!(miri) { 128 } else { 2048 };

    // Create an exchange object with `WORKERS` concurrent senders/receivers.
    // Iterate for `ROUNDS` rounds with each sender sending value `N` to each
    // receiver in round number `N`.  Both senders and receivers may retry
    // sending/receiving multiple times, but in the end each receiver should get
    // all values in correct order.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_exchange() {
        const WORKERS: usize = 16;

        let hruntime = Runtime::run(WORKERS, || {
            let exchange = Exchange::with_runtime(&Runtime::runtime().unwrap(), 0);

            for round in 0..ROUNDS {
                let output_data = vec![round; WORKERS];
                let mut output_iter = output_data.clone().into_iter();
                loop {
                    if exchange.try_send_all(Runtime::worker_index(), &mut output_iter) {
                        break;
                    }

                    yield_now();
                }

                let mut input_data = Vec::with_capacity(WORKERS);
                loop {
                    if exchange.try_receive_all(Runtime::worker_index(), |x| input_data.push(x)) {
                        break;
                    }

                    yield_now();
                }

                assert_eq!(input_data, output_data);
            }
        });

        hruntime.join().unwrap();
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_exchange_operators_static() {
        test_exchange_operators::<StaticScheduler>();
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_exchange_operators_dynamic() {
        test_exchange_operators::<DynamicScheduler>();
    }

    // Create a circuit with `WORKERS` concurrent workers with the following
    // structure: `Generator - ExchangeSender -> ExchangeReceiver -> Inspect`.
    // `Generator` - yields sequential numbers 0, 1, 2, ...
    // `ExchangeSender` - sends each number to all peers.
    // `ExchangeReceiver` - combines all received numbers in a vector.
    // `Inspect` - validates the output of the receiver.
    fn test_exchange_operators<S>()
    where
        S: Scheduler + 'static,
    {
        fn do_test<S>(workers: usize)
        where
            S: Scheduler + 'static,
        {
            let hruntime = Runtime::run(workers, move || {
                let circuit = RootCircuit::build_with_scheduler::<_, _, S>(move |circuit| {
                    let mut n: usize = 0;
                    let source = circuit.add_source(Generator::new(move || {
                        let result = n;
                        n += 1;
                        result
                    }));

                    let (sender, receiver) = new_exchange_operators(
                        &Runtime::runtime().unwrap(),
                        Runtime::worker_index(),
                        None,
                        move |n, vals| {
                            for _ in 0..workers {
                                vals.push(n)
                            }
                        },
                        |v: &mut Vec<usize>, n| v.push(n),
                    );

                    let mut round = 0;
                    circuit
                        .add_exchange(sender, receiver, &source)
                        .inspect(move |v| {
                            assert_eq!(&vec![round; workers], v);
                            round += 1;
                        });
                    Ok(())
                })
                .unwrap()
                .0;

                for _ in 1..ROUNDS {
                    circuit.step().unwrap();
                }
            });

            hruntime.join().unwrap();
        }

        do_test::<S>(1);
        do_test::<S>(16);
        do_test::<S>(32);
    }
}
