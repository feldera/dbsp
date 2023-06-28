//! Operators to shard batches across multiple worker threads based on keys
//! and to gather sharded batches in one worker.

// TODOs:
// - different sharding modes.

use crate::{
    circuit::GlobalNodeId,
    circuit_cache_key, default_hash,
    operator::communication::exchange::new_exchange_operators,
    trace::{cursor::Cursor, Batch, BatchReader, Builder, Spine, Trace},
    Circuit, Runtime, Stream,
};
use std::{hash::Hash, panic::Location};

circuit_cache_key!(ShardId<C, D>((GlobalNodeId, ShardingPolicy) => Stream<C, D>));

// An attempt to future-proof the design for when we support multiple sharding
// disciplines.
#[derive(Hash, PartialEq, Eq)]
pub struct ShardingPolicy;

fn sharding_policy<C>(_circuit: &C) -> ShardingPolicy {
    ShardingPolicy
}

impl<C, IB> Stream<C, IB>
where
    C: Circuit,
    IB: BatchReader<Time = ()> + Clone,
    IB::Key: Ord + Clone + Hash,
    IB::Val: Ord + Clone,
{
    /// Shard batches across multiple worker threads based on keys.
    ///
    /// # Theory
    ///
    /// We parallelize processing across `N` worker threads by creating a
    /// replica of the same circuit per thread and sharding data across
    /// replicas.  To ensure correctness (i.e., that the sum of outputs
    /// produced by individual workers is equal to the output produced
    /// by processing the entire dataset by one worker), sharding must satisfy
    /// certain requirements determined by each operator.  In particular,
    /// for `distinct`, and `aggregate` all tuples that share the same key
    /// must be processed by the same worker.  For `join`, tuples from both
    /// input streams with the same key must be processed by the same worker.
    ///
    /// Other operators, e.g., `filter` and `flat_map`, impose no restrictions
    /// on the sharding scheme: as long as each tuple in a batch is
    /// processed by some worker, the correct result will be produced.  This
    /// is true for all linear operators.
    ///
    /// The `shard` operator shards input batches based on the hash of the key,
    /// making sure that tuples with the same key always end up at the same
    /// worker.  More precisely, the operator **re-shards** its input by
    /// partitioning batches in the input stream of each worker based on the
    /// hash of the key, distributing resulting fragments amond peers
    /// and re-assembling fragments at each peer:
    ///
    /// ```text
    ///         ┌──────────────────┐
    /// worker1 │                  │
    /// ───────►├─────┬───────────►├──────►
    ///         │     │            │
    /// ───────►├─────┴───────────►├──────►
    /// worker2 │                  │
    ///         └──────────────────┘
    /// ```
    ///
    /// # Usage
    ///
    /// Most users do not need to invoke `shard` directly (and doing so is
    /// likely to lead to incorrect results unless you know exactly what you
    /// are doing).  Instead, each operator re-shards its inputs as
    /// necessary, e.g., `join` applies `shard` to both of its
    /// input streams, while `filter` consumes its input directly without
    /// re-sharding.
    ///
    /// # Performance considerations
    ///
    /// In the current implementation, the `shard` operator introduces a
    /// synchronization barrier across all workers: its output at any worker
    /// is only produced once input batches have been collected from all
    /// workers.  This limits the scalability since a slow worker (e.g., running
    /// on a busy CPU core or sharing the core with other workers) or uneven
    /// sharding can slow down the whole system and reduce gains from
    /// parallelization.
    #[track_caller]
    pub fn shard(&self) -> Stream<C, IB>
    where
        IB: Batch + Send,
    {
        // `shard_generic` returns `None` if there is only one worker thread
        // and hence sharding is a no-op.  In this case, we simply return the
        // input stream.  This allows us to use `shard` unconditionally without
        // incurring any overhead in the single-threaded case.
        self.shard_generic().unwrap_or_else(|| self.clone())
    }

    /// Like [`Self::shard`], but can assemble the results into any output batch
    /// type `OB`.
    ///
    /// Returns `None` when the circuit is not running inside a multithreaded
    /// rutime or is running in a runtime with a single worker thread.
    #[track_caller]
    pub fn shard_generic<OB>(&self) -> Option<Stream<C, OB>>
    where
        OB: Batch<Key = IB::Key, Val = IB::Val, Time = (), R = IB::R> + Send,
    {
        let location = Location::caller();

        Runtime::runtime().and_then(|runtime| {
            let num_workers = runtime.num_workers();

            if num_workers == 1 {
                None
            } else {
                let output = self
                    .circuit()
                    .cache_get_or_insert_with(
                        ShardId::new((
                            self.origin_node_id().clone(),
                            sharding_policy(self.circuit()),
                        )),
                        move || {
                            // As a minor optimization, we reuse this array across all invocations
                            // of the sharding operator.
                            let mut builders = Vec::with_capacity(runtime.num_workers());
                            let (sender, receiver) = new_exchange_operators(
                                &runtime,
                                Runtime::worker_index(),
                                Some(location),
                                move |batch: IB, batches: &mut Vec<OB>| {
                                    Self::shard_batch(&batch, num_workers, &mut builders, batches);
                                },
                                |trace: &mut Spine<OB>, batch: OB| trace.insert(batch),
                            );

                            // Is `consolidate` always necessary? Some (all?) consumers may be happy
                            // working with traces.
                            let output = self
                                .circuit()
                                .add_exchange(sender, receiver, self)
                                .consolidate();

                            self.circuit().cache_insert(
                                ShardId::new((
                                    output.origin_node_id().clone(),
                                    sharding_policy(self.circuit()),
                                )),
                                output.clone(),
                            );

                            output
                        },
                    )
                    .clone();

                Some(output)
            }
        })
    }

    // Partitions the batch into `nshards` partitions based on the hash of the key.
    fn shard_batch<OB>(
        batch: &IB,
        shards: usize,
        builders: &mut Vec<OB::Builder>,
        outputs: &mut Vec<OB>,
    ) where
        OB: Batch<Key = IB::Key, Val = IB::Val, Time = (), R = IB::R>,
    {
        builders.clear();

        for _ in 0..shards {
            // We iterate over tuples in the batch in order; hence tuples added
            // to each shard are also ordered, so we can use the more efficient
            // `Builder` API (instead of `Batcher`) to construct output batches.
            builders.push(OB::Builder::with_capacity((), batch.len() / shards));
        }

        let mut cursor = batch.cursor();

        while cursor.key_valid() {
            let batch_index = default_hash(cursor.key()) as usize % shards;
            while cursor.val_valid() {
                builders[batch_index].push((
                    OB::item_from(cursor.key().clone(), cursor.val().clone()),
                    cursor.weight(),
                ));
                cursor.step_val();
            }
            cursor.step_key();
        }

        for builder in builders.drain(..) {
            outputs.push(builder.done());
        }
    }
}

impl<C, T> Stream<C, T>
where
    C: Circuit,
    T: 'static,
{
    /// Marks the data within the current stream as sharded, meaning that all
    /// further calls to `.shard()` will have no effect.
    ///
    /// This must only be used on streams of values that are properly sharded
    /// across workers, otherwise this will cause the dataflow to yield
    /// incorrect results
    pub fn mark_sharded(&self) -> Self {
        self.circuit().cache_insert(
            ShardId::new((
                self.origin_node_id().clone(),
                sharding_policy(self.circuit()),
            )),
            self.clone(),
        );
        self.clone()
    }

    /// Returns `true` if a sharded version of the current stream exists
    pub fn has_sharded_version(&self) -> bool {
        self.circuit().cache_contains(&ShardId::<C, T>::new((
            self.origin_node_id().clone(),
            sharding_policy(self.circuit()),
        )))
    }

    /// Returns the sharded version of the stream if it exists
    /// (which may be the stream itself or the result of applying
    /// the `shard` operator to it).  Otherwise, returns `self`.
    pub fn try_sharded_version(&self) -> Self {
        self.circuit()
            .cache_get(&ShardId::new((
                self.origin_node_id().clone(),
                sharding_policy(self.circuit()),
            )))
            .unwrap_or_else(|| self.clone())
    }

    /// Marks `self` as sharded if `input` has a sharded version of itself
    pub fn mark_sharded_if<C2, U>(&self, input: &Stream<C2, U>)
    where
        C2: Circuit,
        U: 'static,
    {
        if input.has_sharded_version() {
            self.mark_sharded();
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        operator::Generator,
        trace::{Batch, BatchReader},
        Circuit, OrdIndexedZSet, RootCircuit, Runtime,
    };

    #[test]
    fn test_shard() {
        do_test_shard(2);
        do_test_shard(4);
        do_test_shard(16);
    }

    fn test_data(worker_index: usize, num_workers: usize) -> OrdIndexedZSet<usize, usize, isize> {
        let tuples: Vec<_> = (0..1000)
            .filter(|n| n % num_workers == worker_index)
            .flat_map(|n| vec![((n, n), 1), ((n, 1000 * n), 1)])
            .collect();
        <OrdIndexedZSet<usize, usize, isize>>::from_tuples((), tuples)
    }

    fn do_test_shard(workers: usize) {
        let hruntime = Runtime::run(workers, || {
            let circuit = RootCircuit::build(move |circuit| {
                let input = circuit.add_source(Generator::new(|| {
                    let worker_index = Runtime::worker_index();
                    let num_workers = Runtime::runtime().unwrap().num_workers();
                    test_data(worker_index, num_workers)
                }));
                input
                    .shard()
                    .gather(0)
                    .inspect(|batch: &OrdIndexedZSet<usize, usize, isize>| {
                        if Runtime::worker_index() == 0 {
                            assert_eq!(batch, &test_data(0, 1))
                        } else {
                            assert_eq!(batch.len(), 0);
                        }
                    });
                Ok(())
            })
            .unwrap()
            .0;

            for _ in 0..3 {
                circuit.step().unwrap();
            }
        });

        hruntime.join().unwrap();
    }
}
