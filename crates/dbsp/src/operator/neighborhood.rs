use crate::{
    algebra::{HasZero, ZRingValue},
    circuit::{
        operator_traits::{BinaryOperator, Operator},
        Scope,
    },
    trace::{cursor::Cursor, Batch, BatchReader, Builder},
    Circuit, DBData, DBWeight, IndexedZSet, NumEntries, OrdIndexedZSet, OrdZSet, RootCircuit,
    Stream,
};
use serde::Deserialize;
use size_of::SizeOf;
use std::{borrow::Cow, marker::PhantomData};

/// A a contiguous range of rows preceding and following a fixed "anchor"
/// point.
///
/// A neighborhood is a contiguous range of rows preceding and following
/// a fixed "anchor" point.  The anchor may represent the first row in
/// the on-screen grid.  Rows are numbered, with number 0 representing
/// the anchor, negative numbers representing rows preceding the anchor,
/// and positive numbers rows following the anchor.
///
/// Each row contains a key/value pair.  Rows are ordered by keys and,
/// for each key, by values.
///
/// Similar to other DBSP collections, the [`Neighborhood`] type can represent
/// a complete neighborhood or a delta that must be applied to the previous
/// state of the neighborhood to obtain a new complete snapshot.  A complete
/// neighborhood always contains a contiguous set of indexes, as in the
/// following example:
///
/// ```text
///             │  #  │ key │ val │ w
///             ├─────┼─────┼─────┼───
///             │ -3  │ "a" │ 100 │ 1
///             │ -2  │ "a" │ 101 │ 1
///             │ -1  │ "b" │   5 │ 5
/// anchor ───► │  0  │ "b" │   7 │ 1
///             │  1  │ "b" │  10 │ 1
///             │  2  │ "c" │ 324 │ 1
///             │  3  │ "d" │   0 │ 1
///             │  4  │ "e" │  11 │ 2
///             │  5  │ "e" │  12 │ 1
/// ```
///
/// A delta on the other hand only contains affected indexes.  The following
/// delta, for instance, inserts the key/value pair `("a", 102)` in position
/// `-2` in the neighborhood, shifting `("a", 101)` to position `-3`
/// and pushing `("a", 100)` out of the neighborhood:
///
/// ```text
///             │  #  │ key │ val │ w
///             ├─────┼─────┼─────┼───
///             │ -3  │ "a" │ 100 │ -1 // ("a", 100) pushed out of the neighborhood
///             │ -3  │ "a" │ 101 │ +1 // ("a", 101) pushed from position -2 to -3
///             │ -2  │ "a" │ 101 │ -1
///             │ -2  │ "a" │ 102 │ +1 // ("a", 102) in inserted in positon -2.
/// ```
pub type Neighborhood<K, V, R> = OrdZSet<(isize, (K, V)), R>;

/// Neighborhood descriptor represents a request from the user to
/// output a specific neighborhood.
///
/// The neighborhood is defined in terms of its central point
/// (`anchor`) and the number of rows preceding and following the
/// anchor to output.
///
/// The `anchor` value of `None` is equivalent to specifying the
/// smallest value of type `K`.
#[derive(Clone, Debug, Deserialize, PartialOrd, Ord, PartialEq, Eq, Hash, SizeOf)]
pub struct NeighborhoodDescr<K, V> {
    anchor: Option<K>,
    #[serde(default)]
    anchor_val: V,
    before: usize,
    after: usize,
}

impl<K, V> NumEntries for NeighborhoodDescr<K, V> {
    const CONST_NUM_ENTRIES: Option<usize> = Some(1);

    fn num_entries_shallow(&self) -> usize {
        1
    }

    fn num_entries_deep(&self) -> usize {
        1
    }
}

/// Stream of neighborhoods output by the [`Stream::neighborhood`] operator.
pub type NeighborhoodStream<K, V, R> = Stream<RootCircuit, Neighborhood<K, V, R>>;

/// Stream of neighborhood descriptors supplied as input to the
/// [`Stream::neighborhood`] operator.
pub type NeighborhoodDescrStream<K, V> = Stream<RootCircuit, Option<NeighborhoodDescr<K, V>>>;

impl<K, V> NeighborhoodDescr<K, V> {
    pub fn new(k: Option<K>, v: V, before: usize, after: usize) -> Self {
        Self {
            anchor: k,
            anchor_val: v,
            before,
            after,
        }
    }
}

impl<B> Stream<RootCircuit, B>
where
    B: IndexedZSet + Send,
    B::R: ZRingValue,
{
    /// Returns a small contiguous range of rows ([`Neighborhood`]) of the input
    /// table.
    ///
    /// This operator helps to visualize the contents of the input table in a
    /// UI.  The UI client may not have enough throughput/memory to store the
    /// entire table, and will instead limit its state to a small range of
    /// rows that fit on the screen.  We specify such a range, or
    /// _neighborhood_, in terms of its center (or "anchor"), and the number
    /// of rows preceding and following the anchor (see
    /// [`NeighborhoodDescr`]).  The user may be interested in a static
    /// snapshot of the neighborhood or in a changing view.  Both modes are
    /// supported by this operator (see the `reset` argument).  The output of
    /// the operator is a stream of [`Neighborhood`]s.
    ///
    /// NOTE: This operator assumes that the integral of the input stream does
    /// not contain negative weights (which should normally be the case) and
    /// may produce incorrect outputs otherwise.
    ///
    /// # Arguments
    ///
    /// * `self` - a stream of changes to an indexed Z-set.
    ///
    /// * `neighborhood_descr` - contains the neighborhood descriptor to
    ///   evaluate at every clock tick.  Set to `None` to disable the operator
    ///   (it will output an empty neighborhood).
    ///
    /// # Output
    ///
    /// Outputs a stream of changes to the neighborhood.
    ///
    /// The output neighborhood will contain rows with indexes between
    /// `-descr.before` and `descr.after - 1`.  Row 0 is the anchor row, i.e.,
    /// is the first row in the input stream greater than or equal to
    /// `descr.anchor`.  If there is no such row (i.e., all rows in the input
    /// stream are smaller than the anchor), then the neighborhood will only
    /// contain negative indexes.
    ///
    /// The first index in the neightborhood may be greater
    /// than `-descr.before` if the input stream doesn't contain enough rows
    /// preceding the specified anchor.  The last index may be smaller than
    /// `descr.after - 1` if the input stream doesn't contain `descr.after`
    /// rows following the anchor point.
    pub fn neighborhood(
        &self,
        neighborhood_descr: &NeighborhoodDescrStream<B::Key, B::Val>,
    ) -> NeighborhoodStream<B::Key, B::Val, B::R> {
        self.circuit().region("neighborhood", || {
            // Compute local neighborhood in each worker.  We don't shard
            // the input stream, which means that multiple workers can
            // contain the same key in their neighborhood.  This shouldn't
            // affect correctness assuming that the integral of the input
            // stream does not contain negative weights (which should normally
            // be the case) and so identical key/value pairs in different
            // workers won't cancel out.
            let stream = self.try_sharded_version();
            let local_output = self
                .circuit()
                .add_binary_operator(
                    NeighborhoodLocal::new(),
                    &stream.integrate_trace(),
                    neighborhood_descr,
                )
                .differentiate();

            // Gather all results in worker 0.  Worker 0 then computes
            // the final neighborhood.
            // TODO: use different workers for different collections.
            let output = self.circuit().add_binary_operator(
                NeighborhoodNumbered::new(),
                &local_output.gather(0).integrate_trace(),
                neighborhood_descr,
            );

            output.differentiate()
        })
    }
}

/// Computes the neighborhood without row numbers.
///
/// Used for per-worker neighborhood computation in each worker, where
/// row numbers are not needed (since row numbers are assigned by
/// worker 0).
///
/// The operator takes a trace of the input table and produces
/// a complete neighborhood as its output.  The internal implementation
/// is non-incremental and will do the work proportional to the size
/// of the neighborhood even if the neighborhood has not
/// changed since the last clock cycle.  This is ok assuming small
/// neighborhoods.  We may want to switch to an incremental
/// implementation in the future.
struct NeighborhoodLocal<T>
where
    T: BatchReader,
{
    _phantom: PhantomData<T>,
}

impl<T> NeighborhoodLocal<T>
where
    T: BatchReader,
{
    fn new() -> Self {
        Self {
            _phantom: PhantomData,
        }
    }
}

impl<T> Operator for NeighborhoodLocal<T>
where
    T: BatchReader + 'static,
{
    fn name(&self) -> Cow<'static, str> {
        Cow::from("NeighborhoodLocal")
    }
    fn fixedpoint(&self, _scope: Scope) -> bool {
        true
    }
}

impl<T>
    BinaryOperator<
        T,
        Option<NeighborhoodDescr<T::Key, T::Val>>,
        OrdIndexedZSet<T::Key, T::Val, T::R>,
    > for NeighborhoodLocal<T>
where
    T: BatchReader<Time = ()>,
    T::Key: DBData,
    T::Val: DBData,
    T::R: DBWeight,
{
    fn eval<'a>(
        &mut self,
        input_trace: &T,
        descr: &Option<NeighborhoodDescr<T::Key, T::Val>>,
    ) -> OrdIndexedZSet<T::Key, T::Val, T::R> {
        let mut cursor = input_trace.cursor();

        if let Some(descr) = descr {
            let anchor_key = &descr.anchor;
            let anchor_val = &descr.anchor_val;

            // Forward pass: locate the anchor and `decr.after`
            // following rows.
            let mut after = Vec::with_capacity(descr.after + 1);
            let mut offset = 0;

            if let Some(anchor_key) = anchor_key {
                cursor.seek_keyval(anchor_key, anchor_val);
            };
            while cursor.keyval_valid() && offset <= descr.after {
                let w = cursor.weight();

                if !cursor.weight().is_zero() {
                    after.push(((cursor.key().clone(), cursor.val().clone()), w));
                    offset += 1;
                }
                cursor.step_keyval();
            }

            // Reverse pass: find `descr.before` rows preceding the anchor.
            cursor.fast_forward_keys();
            cursor.fast_forward_vals();

            let mut before = Vec::with_capacity(descr.before);
            offset = 1;

            if let Some(anchor_key) = anchor_key {
                cursor.seek_keyval_reverse(anchor_key, anchor_val);
                if cursor.keyval_valid() && cursor.keyval() == (anchor_key, anchor_val) {
                    cursor.step_keyval_reverse();
                }

                while cursor.keyval_valid() && offset <= descr.before {
                    let w = cursor.weight();

                    if !cursor.weight().is_zero() {
                        before.push(((cursor.key().clone(), cursor.val().clone()), w));
                        offset += 1;
                    }
                    cursor.step_keyval_reverse();
                }
            }

            // Assemble final result.
            let mut builder = <<OrdIndexedZSet<_, _, _> as Batch>::Builder>::with_capacity(
                (),
                before.len() + after.len(),
            );
            for update in before.into_iter().rev() {
                builder.push(update);
            }
            for update in after.into_iter() {
                builder.push(update);
            }

            builder.done()
        } else {
            Batch::empty(())
        }
    }
}

/// Computes the neighborhood including row numbers.
///
/// Used to compute the final output of the [`Stream::neighborhood`]
/// operator in worker-0.
struct NeighborhoodNumbered<T>
where
    T: BatchReader,
{
    _phantom: PhantomData<T>,
}

impl<T> NeighborhoodNumbered<T>
where
    T: BatchReader,
{
    fn new() -> Self {
        Self {
            _phantom: PhantomData,
        }
    }
}

impl<T> Operator for NeighborhoodNumbered<T>
where
    T: BatchReader + 'static,
    T::Key: DBData,
    T::Val: DBData,
    T::R: DBWeight,
{
    fn name(&self) -> Cow<'static, str> {
        Cow::from("NeighborhoodNumbered")
    }
    fn fixedpoint(&self, _scope: Scope) -> bool {
        true
    }
}

impl<T>
    BinaryOperator<T, Option<NeighborhoodDescr<T::Key, T::Val>>, Neighborhood<T::Key, T::Val, T::R>>
    for NeighborhoodNumbered<T>
where
    T: BatchReader<Time = ()> + Clone,
{
    fn eval<'a>(
        &mut self,
        input_trace: &T,
        descr: &Option<NeighborhoodDescr<T::Key, T::Val>>,
    ) -> Neighborhood<T::Key, T::Val, T::R> {
        let mut cursor = input_trace.cursor();

        if let Some(descr) = &descr {
            let anchor_key = &descr.anchor;
            let anchor_val = &descr.anchor_val;

            let mut after = Vec::with_capacity(descr.after + 1);
            let mut offset = 0;

            if let Some(anchor_key) = anchor_key {
                cursor.seek_keyval(anchor_key, anchor_val);
            }
            while cursor.keyval_valid() && offset <= descr.after {
                let w = cursor.weight();

                if !cursor.weight().is_zero() {
                    after.push((
                        (
                            offset as isize,
                            (cursor.key().clone(), cursor.val().clone()),
                        ),
                        w,
                    ));
                    offset += 1;
                }
                cursor.step_keyval();
            }

            cursor.fast_forward_keys();
            cursor.fast_forward_vals();

            let mut before = Vec::with_capacity(descr.before);
            offset = 1;

            if let Some(anchor_key) = anchor_key {
                cursor.seek_keyval_reverse(anchor_key, anchor_val);
                if cursor.keyval_valid() && cursor.keyval() == (anchor_key, anchor_val) {
                    cursor.step_keyval_reverse();
                }

                while cursor.keyval_valid() && offset <= descr.before {
                    let w = cursor.weight();

                    if !cursor.weight().is_zero() {
                        before.push((
                            (
                                -(offset as isize),
                                (cursor.key().clone(), cursor.val().clone()),
                            ),
                            w,
                        ));
                        offset += 1;
                    }
                    cursor.step_keyval_reverse();
                }
            }

            let mut builder = <<Neighborhood<_, _, _> as Batch>::Builder>::with_capacity(
                (),
                before.len() + after.len(),
            );
            for update in before.into_iter().rev() {
                builder.push(update);
            }
            for update in after.into_iter() {
                builder.push(update);
            }

            builder.done()
        } else {
            Batch::empty(())
        }
    }
}

#[cfg(test)]
mod test {
    use crate::{
        algebra::ZRingValue,
        operator::neighborhood::NeighborhoodDescr,
        trace::{
            test_batch::{assert_batch_eq, batch_to_tuples, TestBatch},
            Trace,
        },
        CollectionHandle, DBData, DBWeight, InputHandle, OrdIndexedZSet, OutputHandle, RootCircuit,
        Runtime,
    };
    use anyhow::Result as AnyResult;
    use std::cmp::{max, min};

    impl<K, V, R> TestBatch<K, V, (), R>
    where
        K: DBData,
        V: DBData,
        R: DBWeight + ZRingValue,
    {
        fn neighborhood(
            &self,
            descr: &Option<NeighborhoodDescr<K, V>>,
        ) -> TestBatch<isize, (K, V), (), R> {
            if let Some(descr) = &descr {
                let anchor_k = &descr.anchor;
                let anchor_v = &descr.anchor_val;

                let tuples = batch_to_tuples(self);
                let start = if let Some(anchor_k) = anchor_k {
                    tuples
                        .iter()
                        .position(|((k, v, ()), _w)| (k, v) >= (anchor_k, anchor_v))
                        .unwrap_or(tuples.len()) as isize
                } else {
                    0
                };

                let mut from = start - descr.before as isize;
                let mut to = start + descr.after as isize + 1;

                from = max(from, 0);
                to = min(to, tuples.len() as isize);

                let output = tuples[from as usize..to as usize]
                    .iter()
                    .enumerate()
                    .map(|(i, ((k, v, ()), w))| {
                        (
                            (i as isize - (start - from), (k.clone(), v.clone()), ()),
                            w.clone(),
                        )
                    })
                    .collect::<Vec<_>>();

                TestBatch::from_data(&output)
            } else {
                TestBatch::new(None)
            }
        }
    }

    fn test_circuit(
        circuit: &mut RootCircuit,
    ) -> AnyResult<(
        InputHandle<Option<NeighborhoodDescr<i32, i32>>>,
        CollectionHandle<i32, (i32, i32)>,
        OutputHandle<OrdIndexedZSet<isize, (i32, i32), i32>>,
    )> {
        let (descr_stream, descr_handle) =
            circuit.add_input_stream::<Option<NeighborhoodDescr<i32, i32>>>();
        let (input_stream, input_handle) = circuit.add_input_indexed_zset::<i32, i32, i32>();

        let range_handle = input_stream
            .neighborhood(&descr_stream)
            .integrate()
            .index()
            .output();

        Ok((descr_handle, input_handle, range_handle))
    }

    #[test]
    fn neighborhood_test() {
        let (mut dbsp, (descr_handle, input_handle, output_handle)) =
            Runtime::init_circuit(4, test_circuit).unwrap();

        // Empty collection.
        descr_handle.set_for_all(Some(NeighborhoodDescr::new(Some(10), 10, 3, 5)));

        dbsp.step().unwrap();

        assert!(batch_to_tuples(&output_handle.consolidate()).is_empty());

        descr_handle.set_for_all(Some(NeighborhoodDescr::new(None, 10, 3, 5)));

        dbsp.step().unwrap();

        assert!(batch_to_tuples(&output_handle.consolidate()).is_empty());

        descr_handle.set_for_all(Some(NeighborhoodDescr::new(Some(10), 10, 3, 5)));
        input_handle.push(9, (0, 1));

        dbsp.step().unwrap();

        assert_eq!(
            &batch_to_tuples(&output_handle.consolidate()),
            &[((-1, (9, 0), ()), 1)]
        );

        descr_handle.set_for_all(Some(NeighborhoodDescr::new(None, 10, 3, 5)));
        dbsp.step().unwrap();
        assert_eq!(
            &batch_to_tuples(&output_handle.consolidate()),
            &[((0, (9, 0), ()), 1)]
        );

        descr_handle.set_for_all(Some(NeighborhoodDescr::new(Some(10), 10, 3, 5)));
        input_handle.push(9, (1, 1));

        dbsp.step().unwrap();

        assert_eq!(
            &batch_to_tuples(&output_handle.consolidate()),
            &[((-2, (9, 0), ()), 1), ((-1, (9, 1), ()), 1)]
        );

        descr_handle.set_for_all(Some(NeighborhoodDescr::new(Some(10), 10, 3, 5)));
        input_handle.push(8, (1, 1));

        dbsp.step().unwrap();

        assert_eq!(
            &batch_to_tuples(&output_handle.consolidate()),
            &[
                ((-3, (8, 1), ()), 1),
                ((-2, (9, 0), ()), 1),
                ((-1, (9, 1), ()), 1)
            ]
        );

        descr_handle.set_for_all(Some(NeighborhoodDescr::new(Some(10), 10, 3, 5)));
        input_handle.push(7, (1, 1));

        dbsp.step().unwrap();

        assert_eq!(
            &batch_to_tuples(&output_handle.consolidate()),
            &[
                ((-3, (8, 1), ()), 1),
                ((-2, (9, 0), ()), 1),
                ((-1, (9, 1), ()), 1)
            ]
        );

        descr_handle.set_for_all(Some(NeighborhoodDescr::new(Some(10), 10, 3, 5)));
        input_handle.push(10, (10, 1));

        dbsp.step().unwrap();

        assert_eq!(
            &batch_to_tuples(&output_handle.consolidate()),
            &[
                ((-3, (8, 1), ()), 1),
                ((-2, (9, 0), ()), 1),
                ((-1, (9, 1), ()), 1),
                ((0, (10, 10), ()), 1)
            ]
        );

        descr_handle.set_for_all(Some(NeighborhoodDescr::new(Some(10), 10, 3, 5)));
        input_handle.push(10, (11, 1));
        input_handle.push(12, (0, 1));

        dbsp.step().unwrap();

        assert_eq!(
            &batch_to_tuples(&output_handle.consolidate()),
            &[
                ((-3, (8, 1), ()), 1),
                ((-2, (9, 0), ()), 1),
                ((-1, (9, 1), ()), 1),
                ((0, (10, 10), ()), 1),
                ((1, (10, 11), ()), 1),
                ((2, (12, 0), ()), 1)
            ]
        );

        descr_handle.set_for_all(Some(NeighborhoodDescr::new(Some(10), 10, 3, 5)));
        input_handle.push(10, (10, -1));

        dbsp.step().unwrap();

        assert_eq!(
            &batch_to_tuples(&output_handle.consolidate()),
            &[
                ((-3, (8, 1), ()), 1),
                ((-2, (9, 0), ()), 1),
                ((-1, (9, 1), ()), 1),
                ((0, (10, 11), ()), 1),
                ((1, (12, 0), ()), 1)
            ]
        );

        descr_handle.set_for_all(Some(NeighborhoodDescr::new(Some(10), 10, 3, 5)));
        input_handle.push(13, (0, 1));
        input_handle.push(14, (0, 1));
        input_handle.push(14, (1, 1));
        input_handle.push(14, (2, 1));
        input_handle.push(14, (3, 1));

        dbsp.step().unwrap();

        assert_eq!(
            &batch_to_tuples(&output_handle.consolidate()),
            &[
                ((-3, (8, 1), ()), 1),
                ((-2, (9, 0), ()), 1),
                ((-1, (9, 1), ()), 1),
                ((0, (10, 11), ()), 1),
                ((1, (12, 0), ()), 1),
                ((2, (13, 0), ()), 1),
                ((3, (14, 0), ()), 1),
                ((4, (14, 1), ()), 1),
                ((5, (14, 2), ()), 1)
            ]
        );

        descr_handle.set_for_all(Some(NeighborhoodDescr::new(None, 10, 3, 5)));

        dbsp.step().unwrap();

        assert_eq!(
            &batch_to_tuples(&output_handle.consolidate()),
            &[
                ((0, (7, 1), ()), 1),
                ((1, (8, 1), ()), 1),
                ((2, (9, 0), ()), 1),
                ((3, (9, 1), ()), 1),
                ((4, (10, 11), ()), 1),
                ((5, (12, 0), ()), 1),
            ]
        );
    }

    use proptest::{collection::vec, prelude::*};

    fn input_trace(
        max_key: i32,
        max_val: i32,
        max_batch_size: usize,
        max_batches: usize,
    ) -> impl Strategy<Value = Vec<(Vec<(i32, i32, i32)>, (i32, i32), usize, usize)>> {
        vec(
            (
                vec((0..max_key, 0..max_val, 1..2), 0..max_batch_size),
                (0..max_key, 0..max_val),
                (0..(max_key * max_val) as usize),
                (0..(max_key * max_val) as usize),
            ),
            0..max_batches,
        )
    }

    proptest! {
        #[test]
        fn neighborhood_proptest(trace in input_trace(100, 5, 200, 20)) {

            let (mut dbsp, (descr_handle, input_handle, output_handle)) =
                Runtime::init_circuit(4, test_circuit).unwrap();

            let mut ref_trace = TestBatch::new(None);

            for (batch, (start_key, start_val), before, after) in trace.into_iter() {

                let records = batch.iter().map(|(k, v, r)| ((*k, *v, ()), *r)).collect::<Vec<_>>();

                let ref_batch = TestBatch::from_data(&records);
                ref_trace.insert(ref_batch);

                for (k, v, r) in batch.into_iter() {
                    input_handle.push(k, (v, r));
                }
                let descr = NeighborhoodDescr::new(Some(start_key), start_val, before, after);
                descr_handle.set_for_all(Some(descr.clone()));

                dbsp.step().unwrap();

                let output = output_handle.consolidate();
                let ref_output = ref_trace.neighborhood(&Some(descr));

                assert_batch_eq(&output, &ref_output);

                let descr = NeighborhoodDescr::new(None, start_val, before, after);
                descr_handle.set_for_all(Some(descr.clone()));

                dbsp.step().unwrap();

                let output = output_handle.consolidate();
                let ref_output = ref_trace.neighborhood(&Some(descr));

                assert_batch_eq(&output, &ref_output);
            }
        }
    }
}
