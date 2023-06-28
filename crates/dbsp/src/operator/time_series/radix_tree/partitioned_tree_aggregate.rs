use super::{radix_tree_update, Prefix, RadixTreeCursor, TreeNode};
use crate::{
    algebra::{HasOne, HasZero, Semigroup, ZRingValue},
    circuit::{
        operator_traits::{Operator, TernaryOperator},
        GlobalNodeId, Scope,
    },
    circuit_cache_key,
    operator::{
        time_series::{
            PartitionCursor, PartitionedBatch, PartitionedBatchReader, PartitionedIndexedZSet,
        },
        trace::{TraceBounds, TraceFeedback},
        Aggregator,
    },
    trace::{cursor::CursorEmpty, Builder, Cursor, Spine},
    Circuit, DBData, DBWeight, OrdIndexedZSet, RootCircuit, Stream,
};
use num::PrimInt;
use size_of::SizeOf;
use std::{
    borrow::Cow,
    cmp::Ordering,
    collections::BTreeMap,
    fmt,
    fmt::{Debug, Write},
    marker::PhantomData,
    ops::Neg,
};

circuit_cache_key!(PartitionedTreeAggregateId<C, D, Agg>(GlobalNodeId => Stream<C, D>));

/// Partitioned radix tree batch.
///
/// Partitioned batch where each partition contains a radix tree.
pub trait PartitionedRadixTreeBatch<TS, A>: PartitionedBatch<Prefix<TS>, TreeNode<TS, A>> {}

impl<TS, A, B> PartitionedRadixTreeBatch<TS, A> for B where
    B: PartitionedBatch<Prefix<TS>, TreeNode<TS, A>>
{
}

pub trait PartitionedRadixTreeReader<TS, A>:
    PartitionedBatchReader<Prefix<TS>, TreeNode<TS, A>>
{
}

impl<TS, A, B> PartitionedRadixTreeReader<TS, A> for B where
    B: PartitionedBatchReader<Prefix<TS>, TreeNode<TS, A>>
{
}

type OrdPartitionedRadixTree<PK, TS, A, R> = OrdIndexedZSet<PK, (Prefix<TS>, TreeNode<TS, A>), R>;
type OrdPartitionedRadixTreeStream<PK, TS, A, R> =
    Stream<RootCircuit, OrdPartitionedRadixTree<PK, TS, A, R>>;

/// Cursor over partitioned radix tree.
pub trait PartitionedRadixTreeCursor<PK, TS, A, R>:
    Cursor<PK, (Prefix<TS>, TreeNode<TS, A>), (), R> + Sized
{
    /// Produce a semi-human-readable representation of the partitioned tree
    /// for debugging purposes.
    fn format_tree<W>(&mut self, writer: &mut W) -> Result<(), fmt::Error>
    where
        PK: Debug,
        TS: DBData + PrimInt,
        A: DBData,
        R: HasZero,
        W: Write,
    {
        while self.key_valid() {
            writeln!(writer, "Partition: {:?}", self.key())?;

            let mut partition_cursor = PartitionCursor::new(self);
            partition_cursor.format_tree(writer)?;
            self.step_key();
        }
        Ok(())
    }

    /// Self-diagnostics: validate that `self` points to a well-formed
    /// partitioned radix tree whose contents is equivalent to `contents`.
    fn validate<S>(&mut self, contents: &BTreeMap<PK, BTreeMap<TS, A>>)
    where
        PK: Ord,
        TS: DBData + PrimInt,
        R: DBWeight + ZRingValue,
        A: DBData,
        S: Semigroup<A>,
    {
        let empty = BTreeMap::new();

        while self.key_valid() {
            let partition_contents = contents.get(self.key()).unwrap_or(&empty);

            let mut partition_cursor = PartitionCursor::new(self);
            partition_cursor.validate::<S>(partition_contents);

            self.step_key();
        }
    }
}

impl<PK, TS, A, R, C> PartitionedRadixTreeCursor<PK, TS, A, R> for C where
    C: Cursor<PK, (Prefix<TS>, TreeNode<TS, A>), (), R>
{
}

impl<Z> Stream<RootCircuit, Z>
where
    Z: Clone + 'static,
{
    /// Given a batch of updates to a partitioned time series stream, computes a
    /// stream of updates to its partitioned radix tree.
    ///
    /// This is a building block for higher-level operators such as
    /// [`Stream::partitioned_rolling_aggregate`].
    pub fn partitioned_tree_aggregate<TS, V, Agg>(
        &self,
        aggregator: Agg,
    ) -> OrdPartitionedRadixTreeStream<Z::Key, TS, Agg::Accumulator, isize>
    where
        Z: PartitionedIndexedZSet<TS, V> + SizeOf,
        TS: DBData + PrimInt,
        V: DBData,
        Agg: Aggregator<V, (), Z::R>,
        Agg::Accumulator: Default,
    {
        self.partitioned_tree_aggregate_generic::<TS, V, Agg, OrdPartitionedRadixTree<Z::Key, TS, Agg::Accumulator, isize>>(
            aggregator,
        )
    }

    /// Like [`Self::partitioned_tree_aggregate`], but can return any
    /// partitioned batch type.
    ///
    /// This is a building block for higher-level operators such as
    /// [`Stream::partitioned_rolling_aggregate`].
    pub fn partitioned_tree_aggregate_generic<TS, V, Agg, O>(
        &self,
        aggregator: Agg,
    ) -> Stream<RootCircuit, O>
    where
        Z: PartitionedIndexedZSet<TS, V> + SizeOf,
        TS: DBData + PrimInt,
        V: DBData,
        Agg: Aggregator<V, (), Z::R>,
        Agg::Accumulator: Default,
        O: PartitionedRadixTreeBatch<TS, Agg::Accumulator, Key = Z::Key>,
        O::R: ZRingValue,
    {
        self.circuit()
            .cache_get_or_insert_with(
                <PartitionedTreeAggregateId<_, _, Agg>>::new(self.origin_node_id().clone()),
                move || {
                    let aggregator = aggregator.clone();
                    self.circuit()
                        .region("partitioned_tree_aggregate", move || {
                            let circuit = self.circuit();
                            let stream = self.shard();

                            // We construct the following circuit.  See `RadixTreeAggregate`
                            // documentation for details.
                            //
                            // ```
                            //          ┌─────────────────────────────────────────┐
                            //          │                                         │                                output
                            //          │                                         │                        ┌─────────────────────────────────►
                            //          │                                         ▼                        │
                            //    stream│     ┌───────────────┐         ┌─────────────────────────────┐    │      ┌──────────────────┐
                            // ─────────┴─────┤integrate_trace├───────► │PartitionedRadixTreeAggregate├────┴─────►│UntimedTraceAppend├──┐
                            //                └───────────────┘         └─────────────────────────────┘           └──────────────────┘  │
                            //                                                    ▲                                    ▲                │output_trace
                            //                                                    │                                    │                │
                            //                                                    │                                ┌───┴───┐            │
                            //                                                    └────────────────────────────────┤Z1Trace│◄───────────┘
                            //                                                          delayed_trace              └───────┘
                            // ```

                            // Note: In most use cases `partitioned_tree_aggregate` is applied to
                            // the output of the `window` operator, in which case its input and
                            // output traces are naturally bounded as we are maintaining the tree
                            // over a bounded range of keys.
                            let bounds = <TraceBounds<O::Key, O::Val>>::unbounded();

                            let feedback = circuit.add_integrate_trace_feedback::<Spine<O>>(bounds);

                            let output = circuit
                                .add_ternary_operator(
                                    PartitionedRadixTreeAggregate::new(aggregator),
                                    &stream,
                                    &stream.integrate_trace(),
                                    &feedback.delayed_trace,
                                )
                                .mark_sharded();

                            feedback.connect(&output);

                            output
                        })
                },
            )
            .clone()
    }
}

/// Ternary operator that implements the internals of
/// `partitioned_tree_aggregate`.
///
/// * Input stream 1: updates to the time series.  Only used to identify
///   affected times in each partition.
/// * Input stream 2: trace containing the accumulated partitioned time series
///   data.
/// * Input stream 3: trace containing the current contents of the partitioned
///   radix tree.
struct PartitionedRadixTreeAggregate<TS, V, Z, IT, OT, Agg, O> {
    aggregator: Agg,
    phantom: PhantomData<(TS, V, Z, IT, OT, O)>,
}

impl<TS, V, Z, IT, OT, Agg, O> PartitionedRadixTreeAggregate<TS, V, Z, IT, OT, Agg, O> {
    pub fn new(aggregator: Agg) -> Self {
        Self {
            aggregator,
            phantom: PhantomData,
        }
    }
}

impl<TS, V, Z, IT, OT, Agg, O> Operator for PartitionedRadixTreeAggregate<TS, V, Z, IT, OT, Agg, O>
where
    TS: 'static,
    V: 'static,
    Z: 'static,
    IT: 'static,
    OT: 'static,
    Agg: 'static,
    O: 'static,
{
    fn name(&self) -> Cow<'static, str> {
        Cow::from("PartitionedRadixTreeAggregate")
    }

    fn fixedpoint(&self, _scope: Scope) -> bool {
        true
    }
}

impl<TS, V, Z, IT, OT, Agg, O> TernaryOperator<Z, IT, OT, O>
    for PartitionedRadixTreeAggregate<TS, V, Z, IT, OT, Agg, O>
where
    Z: PartitionedBatchReader<TS, V> + Clone,
    TS: DBData + PrimInt,
    V: DBData,
    IT: PartitionedBatchReader<TS, V, Key = Z::Key, R = Z::R> + Clone,
    OT: PartitionedRadixTreeReader<TS, Agg::Accumulator, Key = Z::Key, R = O::R> + Clone,
    Agg: Aggregator<V, (), Z::R>,
    Agg::Accumulator: Default,
    O: PartitionedRadixTreeBatch<TS, Agg::Accumulator, Key = Z::Key>,
    O::R: ZRingValue,
{
    fn eval<'a>(
        &mut self,
        delta: Cow<'a, Z>,
        input_trace: Cow<'a, IT>,
        output_trace: Cow<'a, OT>,
    ) -> O {
        let mut builder = O::Builder::with_capacity((), delta.len() * 2);
        let mut updates = Vec::new();

        let mut delta_cursor = delta.cursor();
        let mut input_cursor = input_trace.cursor();
        let mut output_cursor = output_trace.cursor();

        while delta_cursor.key_valid() {
            // println!("partition: {:?}", delta_cursor.key());

            let key = delta_cursor.key().clone();

            let delta_partition_cursor = PartitionCursor::new(&mut delta_cursor);

            input_cursor.seek_key(&key);
            output_cursor.seek_key(&key);

            if input_cursor.key_valid() && input_cursor.key() == &key {
                // println!("input partition exists");
                /*while input_cursor.val_valid() {
                    // println!("input val: {:x?}", input_cursor.val());
                    input_cursor.step_val();
                }*/
                //input_cursor.rewind_vals();

                if output_cursor.key_valid() && output_cursor.key() == &key {
                    // println!("tree partition exists");

                    radix_tree_update::<TS, V, Z::R, Agg, _, _, _, _>(
                        delta_partition_cursor,
                        PartitionCursor::new(&mut input_cursor),
                        PartitionCursor::new(&mut output_cursor),
                        &self.aggregator,
                        &mut updates,
                    );
                } else {
                    radix_tree_update::<TS, V, Z::R, Agg, _, _, _, _>(
                        delta_partition_cursor,
                        PartitionCursor::new(&mut input_cursor),
                        <CursorEmpty<_, _, _, O::R>>::new(),
                        &self.aggregator,
                        &mut updates,
                    );
                }
            } else if output_cursor.key_valid() && output_cursor.key() == &key {
                radix_tree_update::<TS, V, Z::R, Agg, _, _, _, _>(
                    delta_partition_cursor,
                    CursorEmpty::new(),
                    PartitionCursor::new(&mut output_cursor),
                    &self.aggregator,
                    &mut updates,
                );
            } else {
                radix_tree_update::<TS, V, Z::R, Agg, _, _, _, _>(
                    delta_partition_cursor,
                    CursorEmpty::new(),
                    <CursorEmpty<_, _, _, O::R>>::new(),
                    &self.aggregator,
                    &mut updates,
                );
            }

            // `updates` are already ordered by prefix.  All that remains is to order
            // insertion and deletion within each update.
            for update in updates.drain(..) {
                match update.new.cmp(&update.old) {
                    Ordering::Equal => {}
                    Ordering::Less => {
                        if let Some(new) = update.new {
                            builder.push((
                                O::item_from(key.clone(), (update.prefix.clone(), new)),
                                O::R::one(),
                            ));
                        };
                        if let Some(old) = update.old {
                            builder.push((
                                O::item_from(key.clone(), (update.prefix, old)),
                                O::R::one().neg(),
                            ));
                        };
                    }
                    Ordering::Greater => {
                        if let Some(old) = update.old {
                            builder.push((
                                O::item_from(key.clone(), (update.prefix.clone(), old)),
                                O::R::one().neg(),
                            ));
                        };
                        if let Some(new) = update.new {
                            builder.push((
                                O::item_from(key.clone(), (update.prefix, new)),
                                O::R::one(),
                            ));
                        };
                    }
                }
            }

            delta_cursor.step_key();
        }

        builder.done()
    }
}

#[cfg(test)]
mod test {
    use super::{super::test::test_aggregate_range, PartitionCursor, PartitionedRadixTreeCursor};
    use crate::{
        algebra::{DefaultSemigroup, HasZero, Semigroup},
        operator::Fold,
        trace::BatchReader,
        CollectionHandle, DBData, RootCircuit,
    };
    use num::PrimInt;
    use std::{
        collections::{btree_map::Entry, BTreeMap},
        sync::{Arc, Mutex},
    };

    // Checks that `aggregate_range` correctly computes aggregates for all
    // possible ranges in all partitions.
    fn test_partitioned_aggregate_range<PK, TS, A, R, C, S>(
        cursor: &mut C,
        contents: &BTreeMap<PK, BTreeMap<TS, A>>,
    ) where
        C: PartitionedRadixTreeCursor<PK, TS, A, R>,
        PK: DBData,
        TS: DBData + PrimInt,
        A: DBData,
        R: DBData + HasZero,
        S: Semigroup<A>,
    {
        let empty = BTreeMap::new();

        while cursor.key_valid() {
            let partition_contents = contents.get(cursor.key()).unwrap_or(&empty);

            let mut partition_cursor = PartitionCursor::new(cursor);
            test_aggregate_range::<_, _, _, _, S>(&mut partition_cursor, partition_contents);

            cursor.step_key();
        }
    }

    fn update_key(
        input: &CollectionHandle<u64, ((u64, u64), isize)>,
        contents: &mut BTreeMap<u64, BTreeMap<u64, u64>>,
        partition: u64,
        key: u64,
        upd: (u64, isize),
    ) {
        input.push(partition, ((key, upd.0), upd.1));

        match contents.entry(partition).or_default().entry(key) {
            Entry::Vacant(ve) => {
                assert_eq!(upd.1, 1);
                ve.insert(upd.0);
            }
            Entry::Occupied(mut oe) => {
                assert!(upd.1 == 1 || upd.1 == -1);
                if upd.1 == 1 {
                    *oe.get_mut() += upd.0;
                } else {
                    *oe.get_mut() -= upd.0;
                }
                if *oe.get() == 0 {
                    oe.remove();
                }
            }
        }
    }

    #[test]
    fn test_partitioned_tree_aggregate() {
        let contents = Arc::new(Mutex::new(BTreeMap::new()));
        let contents_clone = contents.clone();

        let (circuit, input) = RootCircuit::build(move |circuit| {
            let (input, input_handle) = circuit.add_input_indexed_zset::<u64, (u64, u64), isize>();

            let aggregator = <Fold<_, DefaultSemigroup<_>, _, _>>::new(
                0u64,
                |agg: &mut u64, val: &u64, _w: isize| *agg += val,
            );

            input
                .partitioned_tree_aggregate::<u64, u64, _>(aggregator)
                .integrate_trace()
                .apply(move |tree_trace| {
                    println!("Radix trees:");
                    let mut treestr = String::new();
                    tree_trace.cursor().format_tree(&mut treestr).unwrap();
                    println!("{treestr}");
                    tree_trace
                        .cursor()
                        .validate::<DefaultSemigroup<_>>(&contents_clone.lock().unwrap());
                    test_partitioned_aggregate_range::<_, _, _, _, _, DefaultSemigroup<_>>(
                        &mut tree_trace.cursor(),
                        &contents_clone.lock().unwrap(),
                    );
                });

            Ok(input_handle)
        })
        .unwrap();

        circuit.step().unwrap();

        update_key(
            &input,
            &mut contents.lock().unwrap(),
            0,
            0x1000_0000_0000_0001,
            (1, 1),
        );
        update_key(
            &input,
            &mut contents.lock().unwrap(),
            1,
            0x1000_0000_0000_0001,
            (1, 1),
        );
        update_key(
            &input,
            &mut contents.lock().unwrap(),
            1,
            0x0000_0000_0000_0001,
            (2, 1),
        );
        circuit.step().unwrap();

        update_key(
            &input,
            &mut contents.lock().unwrap(),
            0,
            0x1000_0000_0000_0002,
            (2, 1),
        );
        update_key(
            &input,
            &mut contents.lock().unwrap(),
            1,
            0x0000_f000_0000_0002,
            (3, 1),
        );
        update_key(
            &input,
            &mut contents.lock().unwrap(),
            1,
            0x0000_0000_0000_0001,
            (2, -1),
        );
        circuit.step().unwrap();

        update_key(
            &input,
            &mut contents.lock().unwrap(),
            0,
            0x1000_1000_0000_0000,
            (3, 1),
        );
        update_key(
            &input,
            &mut contents.lock().unwrap(),
            2,
            0x0000_0000_f000_0000,
            (1, 1),
        );
        update_key(
            &input,
            &mut contents.lock().unwrap(),
            2,
            0x0000_0000_f100_0000,
            (2, 1),
        );
        update_key(
            &input,
            &mut contents.lock().unwrap(),
            2,
            0x0000_0000_f200_0000,
            (3, 1),
        );
        circuit.step().unwrap();

        update_key(
            &input,
            &mut contents.lock().unwrap(),
            0,
            0x1000_0000_0000_0002,
            (2, -1),
        );
        circuit.step().unwrap();

        update_key(
            &input,
            &mut contents.lock().unwrap(),
            0,
            0xf100_0000_0000_0001,
            (4, 1),
        );
        update_key(
            &input,
            &mut contents.lock().unwrap(),
            0,
            0xf200_0000_0000_0001,
            (5, 1),
        );
        update_key(
            &input,
            &mut contents.lock().unwrap(),
            0,
            0xf300_0000_0000_0001,
            (6, 1),
        );
        update_key(
            &input,
            &mut contents.lock().unwrap(),
            0,
            0xf300_1000_0000_0001,
            (7, 1),
        );
        update_key(
            &input,
            &mut contents.lock().unwrap(),
            0,
            0xf300_1000_1000_0001,
            (8, 1),
        );
        update_key(
            &input,
            &mut contents.lock().unwrap(),
            0,
            0xf300_1000_1000_1001,
            (9, 1),
        );
        update_key(
            &input,
            &mut contents.lock().unwrap(),
            0,
            0xf300_1000_1100_1001,
            (10, 1),
        );
        update_key(
            &input,
            &mut contents.lock().unwrap(),
            0,
            0xf300_1000_1100_1001,
            (10, -1),
        );
        update_key(
            &input,
            &mut contents.lock().unwrap(),
            2,
            0x0000_0000_f200_0000,
            (4, 1),
        );
        circuit.step().unwrap();

        update_key(
            &input,
            &mut contents.lock().unwrap(),
            0,
            0xf400_1000_1100_1001,
            (11, 1),
        );
        update_key(
            &input,
            &mut contents.lock().unwrap(),
            0,
            0xf300_1000_0000_0001,
            (7, -1),
        );
        circuit.step().unwrap();

        update_key(
            &input,
            &mut contents.lock().unwrap(),
            0,
            0x1000_0000_0000_0001,
            (1, -1),
        );
        update_key(
            &input,
            &mut contents.lock().unwrap(),
            0,
            0x1000_1000_0000_0000,
            (3, -1),
        );
        update_key(
            &input,
            &mut contents.lock().unwrap(),
            0,
            0xf100_0000_0000_0001,
            (4, -1),
        );
        update_key(
            &input,
            &mut contents.lock().unwrap(),
            0,
            0xf200_0000_0000_0001,
            (5, -1),
        );
        update_key(
            &input,
            &mut contents.lock().unwrap(),
            2,
            0x0000_0000_f200_0000,
            (4, -1),
        );
        update_key(
            &input,
            &mut contents.lock().unwrap(),
            2,
            0x0000_0000_f0f0_0000,
            (5, 1),
        );
        circuit.step().unwrap();

        update_key(
            &input,
            &mut contents.lock().unwrap(),
            0,
            0xf300_0000_0000_0001,
            (6, -1),
        );
        update_key(
            &input,
            &mut contents.lock().unwrap(),
            0,
            0xf300_1000_1000_0001,
            (8, -1),
        );
        update_key(
            &input,
            &mut contents.lock().unwrap(),
            0,
            0xf300_1000_1000_1001,
            (9, -1),
        );
        circuit.step().unwrap();

        update_key(
            &input,
            &mut contents.lock().unwrap(),
            0,
            0xf400_1000_1100_1001,
            (11, -1),
        );
        circuit.step().unwrap();

        update_key(
            &input,
            &mut contents.lock().unwrap(),
            0,
            0xf100_0000_0000_0001,
            (4, 1),
        );
        update_key(
            &input,
            &mut contents.lock().unwrap(),
            0,
            0xf200_0000_0000_0001,
            (5, 1),
        );
        update_key(
            &input,
            &mut contents.lock().unwrap(),
            0,
            0xf300_0000_0000_0001,
            (6, 1),
        );
        update_key(
            &input,
            &mut contents.lock().unwrap(),
            0,
            0xf300_1000_0000_0001,
            (7, 1),
        );
        update_key(
            &input,
            &mut contents.lock().unwrap(),
            0,
            0xf300_1000_1000_0001,
            (8, 1),
        );
        update_key(
            &input,
            &mut contents.lock().unwrap(),
            0,
            0xf300_1000_1000_0001,
            (11, 1),
        );
        circuit.step().unwrap();
    }
}
