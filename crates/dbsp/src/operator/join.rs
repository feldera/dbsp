//! Relational join operator.

use crate::{
    algebra::{IndexedZSet, Lattice, MulByRef, PartialOrder, ZRingValue, ZSet},
    circuit::{
        metadata::{MetaItem, OperatorLocation, OperatorMeta},
        operator_traits::{BinaryOperator, Operator},
        Circuit, GlobalNodeId, RootCircuit, Scope, Stream, WithClock,
    },
    circuit_cache_key,
    operator::FilterMap,
    time::Timestamp,
    trace::{cursor::Cursor as TraceCursor, Batch, BatchReader, Batcher, Builder, Spine, Trace},
    DBData, DBTimestamp, OrdIndexedZSet, OrdZSet,
};
use bincode::{Decode, Encode};
use size_of::{Context, SizeOf};
use std::{
    borrow::Cow,
    cmp::{min, Ordering},
    collections::HashMap,
    iter::once,
    marker::PhantomData,
    mem::{needs_drop, MaybeUninit},
    panic::Location,
};

circuit_cache_key!(AntijoinId<C, D>((GlobalNodeId, GlobalNodeId) => Stream<C, D>));

impl<C, I1> Stream<C, I1>
where
    C: Circuit,
{
    /// Join two streams of batches.
    ///
    /// The operator takes two streams of batches indexed with the same key type
    /// (`I1::Key = I2::Key`) and outputs a stream obtained by joining each pair
    /// of inputs.
    ///
    /// Input streams will typically be produced by
    /// [`index`](`crate::circuit::Stream::index`) or
    /// [`index_with`](`crate::circuit::Stream::index_with`) operators.
    ///
    /// # Type arguments
    ///
    /// * `F` - join function type: maps key and a pair of values from input
    ///   batches to an output value.
    /// * `I1` - batch type in the first input stream.
    /// * `I2` - batch type in the second input stream.
    /// * `V` - output value type.
    #[track_caller]
    #[allow(clippy::type_complexity)]
    pub fn stream_join<F, I2, V>(
        &self,
        other: &Stream<C, I2>,
        join: F,
    ) -> Stream<C, OrdZSet<V, <I1::R as MulByRef<I2::R>>::Output>>
    where
        I1: Batch<Time = ()> + Send + Encode + Decode,
        I2: Batch<Key = I1::Key, Time = ()> + Send + Encode + Decode,
        I1::R: MulByRef<I2::R>,
        <I1::R as MulByRef<I2::R>>::Output: DBData + ZRingValue,
        F: Fn(&I1::Key, &I1::Val, &I2::Val) -> V + 'static,
        V: DBData,
    {
        self.stream_join_generic(other, join)
    }

    /// Like [`Self::stream_join`], but can return any batch type.
    // TODO: Allow taking two different `R` types for each collection
    #[track_caller]
    pub fn stream_join_generic<F, I2, Z>(&self, other: &Stream<C, I2>, join: F) -> Stream<C, Z>
    where
        I1: Batch<Time = ()> + Send + Encode + Decode,
        I2: Batch<Key = I1::Key, Time = ()> + Send + Encode + Decode,
        Z: ZSet,
        I1::R: MulByRef<I2::R, Output = Z::R>,
        F: Fn(&I1::Key, &I1::Val, &I2::Val) -> Z::Key + 'static,
    {
        self.circuit().add_binary_operator(
            Join::new(join, Location::caller()),
            &self.shard(),
            &other.shard(),
        )
    }

    /// More efficient than [`Self::stream_join`], but the output of the join
    /// function must grow monotonically as `(k, v1, v2)` tuples are fed to it
    /// in lexicographic order.
    ///
    /// One such monotonic function is a join function that returns `(k, v1,
    /// v2)` itself.
    #[track_caller]
    pub fn monotonic_stream_join<F, I2, Z>(&self, other: &Stream<C, I2>, join: F) -> Stream<C, Z>
    where
        I1: Batch<Time = ()> + Send + Encode + Decode,
        I2: Batch<Key = I1::Key, Time = ()> + Send + Encode + Decode,
        Z: ZSet,
        I1::R: MulByRef<I2::R, Output = Z::R>,
        F: Fn(&I1::Key, &I1::Val, &I2::Val) -> Z::Key + 'static,
    {
        self.circuit().add_binary_operator(
            MonotonicJoin::new(join, Location::caller()),
            &self.shard(),
            &other.shard(),
        )
    }

    fn stream_join_inner<F, I2, Z>(
        &self,
        other: &Stream<C, I2>,
        join: F,
        location: &'static Location<'static>,
    ) -> Stream<C, Z>
    where
        I1: BatchReader<Time = (), R = Z::R> + Clone + Encode + Decode,
        I2: BatchReader<Key = I1::Key, Time = (), R = Z::R> + Clone + Encode + Decode,
        Z: ZSet,
        Z::R: ZRingValue,
        F: Fn(&I1::Key, &I1::Val, &I2::Val) -> Z::Key + 'static,
    {
        self.circuit()
            .add_binary_operator(Join::new(join, location), self, other)
    }
}

impl<I1> Stream<RootCircuit, I1> {
    /// Incremental join of two streams of batches.
    ///
    /// Given streams `a` and `b` of changes to relations `A` and `B`
    /// respectively, computes a stream of changes to `A ⋈ B` (where `⋈`
    /// is the join operator):
    ///
    /// ```text
    /// delta(A ⋈ B) = A ⋈ B - z^-1(A) ⋈ z^-1(B) = a ⋈ z^-1(B) + z^-1(A) ⋈ b + a ⋈ b
    /// ```
    ///
    /// This method only works in the top-level scope.  It is superseded by
    /// [`join`](`crate::circuit::Stream::join`), which works in arbitrary
    /// nested scopes.  We keep this implementation for testing and
    /// benchmarking purposes.
    #[track_caller]
    #[doc(hidden)]
    pub fn join_incremental<F, I2, Z>(
        &self,
        other: &Stream<RootCircuit, I2>,
        join_func: F,
    ) -> Stream<RootCircuit, Z>
    where
        I1: IndexedZSet + Send + Encode + Decode,
        I2: IndexedZSet<Key = I1::Key, R = I1::R> + Send + Encode + Decode,
        F: Clone + Fn(&I1::Key, &I1::Val, &I2::Val) -> Z::Key + 'static,
        Z: ZSet<R = I1::R>,
        Z::R: ZRingValue,
    {
        let left = self.shard();
        let right = other.shard();

        left.integrate_trace()
            .delay_trace()
            .stream_join_inner(&right, join_func.clone(), Location::caller())
            .plus(&left.stream_join_inner(&right.integrate_trace(), join_func, Location::caller()))
    }
}

impl<C, I1> Stream<C, I1>
where
    C: Circuit,
    <C as WithClock>::Time: DBTimestamp,
    I1: IndexedZSet + Send + Encode + Decode,
    I1::R: ZRingValue,
{
    // TODO: Derive `TS` type from circuit.
    /// Incrementally join two streams of batches.
    ///
    /// Given streams `self` and `other` of batches that represent changes to
    /// relations `A` and `B` respectively, computes a stream of changes to
    /// `A ⋈ B` (where `⋈` is the join operator):
    ///
    /// # Type arguments
    ///
    /// * `I1` - batch type in the first input stream.
    /// * `I2` - batch type in the second input stream.
    /// * `F` - join function type: maps key and a pair of values from input
    ///   batches to an output value.
    /// * `V` - output value type.
    #[track_caller]
    pub fn join<I2, F, V>(
        &self,
        other: &Stream<C, I2>,
        join_func: F,
    ) -> Stream<C, OrdZSet<V, I1::R>>
    where
        I2: IndexedZSet<Key = I1::Key, R = I1::R> + Send + Encode + Decode,
        F: Fn(&I1::Key, &I1::Val, &I2::Val) -> V + Clone + 'static,
        V: DBData,
    {
        self.join_generic(other, move |k, v1, v2| once((join_func(k, v1, v2), ())))
    }

    /// Incrementally join two streams of batches, producing an indexed output
    /// stream.
    ///
    /// This method generalizes [`Self::join`].  It takes a join function that
    /// returns an iterable collection of `(key, value)` pairs, used to
    /// construct an indexed output Z-set.
    #[track_caller]
    pub fn join_index<I2, F, K, V, It>(
        &self,
        other: &Stream<C, I2>,
        join_func: F,
    ) -> Stream<C, OrdIndexedZSet<K, V, I1::R>>
    where
        I2: IndexedZSet<Key = I1::Key, R = I1::R> + Send + Encode + Decode,
        F: Fn(&I1::Key, &I1::Val, &I2::Val) -> It + Clone + 'static,
        K: DBData,
        V: DBData,
        It: IntoIterator<Item = (K, V)> + 'static,
    {
        self.join_generic(other, join_func)
    }

    /// Like [`Self::join_index`], but can return any indexed Z-set type.
    #[track_caller]
    pub fn join_generic<I2, F, Z, It>(&self, other: &Stream<C, I2>, join_func: F) -> Stream<C, Z>
    where
        I2: IndexedZSet<Key = I1::Key, R = I1::R> + Send + Encode + Decode,
        Z: IndexedZSet<R = I1::R>,
        Z::R: MulByRef<Output = Z::R>,
        F: Fn(&I1::Key, &I1::Val, &I2::Val) -> It + Clone + 'static,
        It: IntoIterator<Item = (Z::Key, Z::Val)> + 'static,
    {
        // TODO: I think this is correct, but we need a proper proof.

        // We use the following formula for nested incremental join with arbitrary
        // nesting depth:
        //
        // ```
        // (↑(a ⋈ b)∆)[t] =
        //      __         __            __
        //      ╲          ╲             ╲
        //      ╱          ╱             ╱  {f(k,v1,v2), w1*w2}
        //      ‾‾         ‾‾            ‾‾
        //     k∈K  (t1,t2).t1\/t2=t  (k,v1,w1)∈a[t1]
        //                            (k,v2,w2)∈b[t2]
        // ```
        // where `t1\/t2 = t1.join(t2)` is the least upper bound of logical timestamps
        // t1 and t2, `f` is the join function that combines values from input streams
        // `a` and `b`.  This sum can be split into two terms `left + right`:
        //
        // ```
        //           __         __            __
        //           ╲          ╲             ╲
        // left=     ╱          ╱             ╱  {f(k,v1,v2), w1*w2}
        //           ‾‾         ‾‾            ‾‾
        //          k∈K  (t1,t2).t1\/t2=t  (k,v1,w1)∈a[t1]
        //                 and t2<t1       (k,v2,w2)∈b[t2]
        //           __         __            __
        //           ╲          ╲             ╲
        // right=    ╱          ╱             ╱  {f(k,v1,v2), w1*w2}
        //           ‾‾         ‾‾            ‾‾
        //          k∈K  (t1,t2).t1\/t2=t  (k,v1,w1)∈a[t1]
        //                 and t2>=t1      (k,v2,w2)∈b[t2]
        // ```
        // where `t2<t1` and `t2>=t1` refer to the total order in which timestamps are
        // observed during the execution of the circuit, not their logical partial
        // order.  In particular, all iterations of an earlier clock epoch precede the
        // first iteration of a newer epoch.
        //
        // The advantage of this representation is that each term can be computed
        // as a join of one of the input streams with the trace of the other stream,
        // implemented by the `JoinTrace` operator.
        let left = self.shard();
        let right = other.shard();

        let left_trace = left.trace::<Spine<<<C as WithClock>::Time as Timestamp>::OrdValBatch<I1::Key, I1::Val, I1::R>>>();
        let right_trace = right.trace::<Spine<<<C as WithClock>::Time as Timestamp>::OrdValBatch<I1::Key, I2::Val, I1::R>>>();

        let left = self.circuit().add_binary_operator(
            JoinTrace::new(
                join_func.clone(),
                Location::caller(),
                self.circuit().clone(),
            ),
            &left,
            &right_trace,
        );

        let right = self.circuit().add_binary_operator(
            JoinTrace::new(
                move |k: &I1::Key, v2: &I2::Val, v1: &I1::Val| join_func(k, v1, v2),
                Location::caller(),
                self.circuit().clone(),
            ),
            &right,
            &left_trace.delay_trace(),
        );

        left.plus(&right)
    }

    /// Incremental anti-join operator.
    ///
    /// Returns indexed Z-set consisting of the contents of `self`,
    /// excluding keys that are present in `other`.
    pub fn antijoin<I2>(&self, other: &Stream<C, I2>) -> Stream<C, I1>
    where
        I2: IndexedZSet<Key = I1::Key, R = I1::R> + Send + Encode + Decode,
    {
        self.circuit()
            .cache_get_or_insert_with(
                AntijoinId::new((
                    self.origin_node_id().clone(),
                    other.origin_node_id().clone(),
                )),
                move || {
                    let stream1 = self.shard();
                    let stream2 = other.distinct().shard();

                    stream1
                        .minus(&stream1.join_generic(&stream2, |k, v1, _v2| {
                            std::iter::once((k.clone(), v1.clone()))
                        }))
                        .mark_sharded()
                },
            )
            .clone()
    }
}

impl<C, Z> Stream<C, Z>
where
    C: Circuit,
    <C as WithClock>::Time: DBTimestamp,
    Z: IndexedZSet + Send + Encode + Decode,
    Z::Key: Default,
    Z::Val: Default,
    Z::R: ZRingValue,
{
    /// Outer join:
    /// - returns the output of `join_func` for common keys.
    /// - returns the output of `left_func` for keys only found in `self`, but
    ///   not `other`.
    /// - returns the output of `right_func` for keys only found in `other`, but
    ///   not `self`.
    pub fn outer_join<Z2, F, FL, FR, O>(
        &self,
        other: &Stream<C, Z2>,
        join_func: F,
        left_func: FL,
        right_func: FR,
    ) -> Stream<C, OrdZSet<O, Z::R>>
    where
        Self: FilterMap<C, R = Z::R>,
        Z2: IndexedZSet<Key = Z::Key, R = Z::R> + Send + Encode + Decode,
        Z2::Val: Default,
        Stream<C, Z2>: FilterMap<C, R = Z::R>,
        O: DBData + Default,
        F: Fn(&Z::Key, &Z::Val, &Z2::Val) -> O + Clone + 'static,
        for<'a> FL: Fn(<Self as FilterMap<C>>::ItemRef<'a>) -> O + Clone + 'static,
        for<'a> FR: Fn(<Stream<C, Z2> as FilterMap<C>>::ItemRef<'a>) -> O + Clone + 'static,
    {
        let center = self.join_generic(other, move |k, v1, v2| {
            std::iter::once((join_func(k, v1, v2), ()))
        });
        let left = self.antijoin(other).map_generic(left_func);
        let right = other.antijoin(self).map_generic(right_func);
        center.sum(&[left, right])
    }

    /// Like `outer_join`, but uses default value for the missing side of the
    /// join.
    pub fn outer_join_default<Z2, F, O>(
        &self,
        other: &Stream<C, Z2>,
        join_func: F,
    ) -> Stream<C, OrdZSet<O, Z::R>>
    where
        Self: for<'a> FilterMap<C, R = Z::R, ItemRef<'a> = (&'a Z::Key, &'a Z::Val)>,
        Z2: IndexedZSet<Key = Z::Key, R = Z::R> + Send + Encode + Decode,
        Z2::Val: Default,
        Stream<C, Z2>: for<'a> FilterMap<C, R = Z::R, ItemRef<'a> = (&'a Z2::Key, &'a Z2::Val)>,
        O: DBData + Default,
        F: Fn(&Z::Key, &Z::Val, &Z2::Val) -> O + Clone + 'static,
    {
        let join_func_left = join_func.clone();
        let join_func_right = join_func.clone();
        self.outer_join(
            other,
            join_func,
            move |(k, v1)| join_func_left(k, v1, &<Z2::Val>::default()),
            move |(k, v2)| join_func_right(k, &<Z::Val>::default(), v2),
        )
    }
}

/// Join two streams of batches.
///
/// See [`Stream::join`](`crate::circuit::Stream::join`).
pub struct Join<F, I1, I2, Z> {
    join_func: F,
    location: &'static Location<'static>,
    _types: PhantomData<(I1, I2, Z)>,
}

impl<F, I1, I2, Z> Join<F, I1, I2, Z> {
    pub fn new(join_func: F, location: &'static Location<'static>) -> Self {
        Self {
            join_func,
            location,
            _types: PhantomData,
        }
    }
}

impl<F, I1, I2, Z> Operator for Join<F, I1, I2, Z>
where
    I1: 'static,
    I2: 'static,
    F: 'static,
    Z: 'static,
{
    fn name(&self) -> Cow<'static, str> {
        Cow::Borrowed("Join")
    }

    fn location(&self) -> OperatorLocation {
        Some(self.location)
    }

    fn fixedpoint(&self, _scope: Scope) -> bool {
        true
    }
}

impl<F, I1, I2, Z> BinaryOperator<I1, I2, Z> for Join<F, I1, I2, Z>
where
    I1: BatchReader<Time = ()>,
    I1::R: MulByRef<I2::R, Output = Z::R>,
    I2: BatchReader<Key = I1::Key, Time = ()>,
    F: Fn(&I1::Key, &I1::Val, &I2::Val) -> Z::Key + 'static,
    Z: ZSet,
{
    fn eval(&mut self, i1: &I1, i2: &I2) -> Z {
        let mut cursor1 = i1.cursor();
        let mut cursor2 = i2.cursor();

        // Choose capacity heuristically.
        let mut batch = Vec::with_capacity(min(i1.len(), i2.len()));

        while cursor1.key_valid() && cursor2.key_valid() {
            match cursor1.key().cmp(cursor2.key()) {
                Ordering::Less => cursor1.seek_key(cursor2.key()),
                Ordering::Greater => cursor2.seek_key(cursor1.key()),
                Ordering::Equal => {
                    while cursor1.val_valid() {
                        let w1 = cursor1.weight();
                        let v1 = cursor1.val();
                        while cursor2.val_valid() {
                            let w2 = cursor2.weight();
                            let v2 = cursor2.val();

                            batch.push((
                                (self.join_func)(cursor1.key(), v1, v2),
                                w1.mul_by_ref(&w2),
                            ));
                            cursor2.step_val();
                        }

                        cursor2.rewind_vals();
                        cursor1.step_val();
                    }

                    cursor1.step_key();
                    cursor2.step_key();
                }
            }
        }

        Z::from_keys((), batch)
    }

    // TODO: Impls using consumers
}

pub struct MonotonicJoin<F, I1, I2, Z> {
    join_func: F,
    location: &'static Location<'static>,
    _types: PhantomData<(I1, I2, Z)>,
}

impl<F, I1, I2, Z> MonotonicJoin<F, I1, I2, Z> {
    pub fn new(join_func: F, location: &'static Location<'static>) -> Self {
        Self {
            join_func,
            location,
            _types: PhantomData,
        }
    }
}

impl<F, I1, I2, Z> Operator for MonotonicJoin<F, I1, I2, Z>
where
    I1: 'static,
    I2: 'static,
    F: 'static,
    Z: 'static,
{
    fn name(&self) -> Cow<'static, str> {
        Cow::Borrowed("MonotonicJoin")
    }

    fn location(&self) -> OperatorLocation {
        Some(self.location)
    }

    fn fixedpoint(&self, _scope: Scope) -> bool {
        true
    }
}

impl<F, I1, I2, Z> BinaryOperator<I1, I2, Z> for MonotonicJoin<F, I1, I2, Z>
where
    I1: BatchReader<Time = ()>,
    I2: BatchReader<Key = I1::Key, Time = ()>,
    F: Fn(&I1::Key, &I1::Val, &I2::Val) -> Z::Key + 'static,
    Z: ZSet,
    I1::R: MulByRef<I2::R, Output = Z::R>,
{
    fn eval(&mut self, i1: &I1, i2: &I2) -> Z {
        let mut cursor1 = i1.cursor();
        let mut cursor2 = i2.cursor();

        // Choose capacity heuristically.
        let mut builder = Z::Builder::with_capacity((), min(i1.len(), i2.len()));

        while cursor1.key_valid() && cursor2.key_valid() {
            match cursor1.key().cmp(cursor2.key()) {
                Ordering::Less => cursor1.seek_key(cursor2.key()),
                Ordering::Greater => cursor2.seek_key(cursor1.key()),
                Ordering::Equal => {
                    while cursor1.val_valid() {
                        let w1 = cursor1.weight();
                        let v1 = cursor1.val();
                        while cursor2.val_valid() {
                            let w2 = cursor2.weight();
                            let v2 = cursor2.val();

                            builder.push((
                                Z::item_from((self.join_func)(cursor1.key(), v1, v2), ()),
                                w1.mul_by_ref(&w2),
                            ));
                            cursor2.step_val();
                        }

                        cursor2.rewind_vals();
                        cursor1.step_val();
                    }

                    cursor1.step_key();
                    cursor2.step_key();
                }
            }
        }

        builder.done()
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
struct JoinStats {
    lhs_tuples: usize,
    rhs_tuples: usize,
    output_tuples: usize,
    produced_tuples: usize,
}

impl JoinStats {
    pub const fn new() -> Self {
        Self {
            lhs_tuples: 0,
            rhs_tuples: 0,
            output_tuples: 0,
            produced_tuples: 0,
        }
    }
}

pub struct JoinTrace<F, I, T, Z, It, Clk>
where
    T: BatchReader,
    Z: IndexedZSet,
{
    clock: Clk,
    join_func: F,
    location: &'static Location<'static>,
    // Future update batches computed ahead of time, indexed by time
    // when each batch should be output.
    output_batchers: HashMap<T::Time, Z::Batcher>,
    // True if empty input batch was received at the current clock cycle.
    empty_input: bool,
    // True if empty output was produced at the current clock cycle.
    empty_output: bool,
    stats: JoinStats,
    _types: PhantomData<(I, T, Z, It)>,
}

impl<F, I, T, Z, It, Clk> JoinTrace<F, I, T, Z, It, Clk>
where
    T: BatchReader,
    Z: IndexedZSet,
{
    pub fn new(join_func: F, location: &'static Location<'static>, clock: Clk) -> Self {
        Self {
            clock,
            join_func,
            location,
            output_batchers: HashMap::new(),
            empty_input: false,
            empty_output: false,
            stats: JoinStats::new(),
            _types: PhantomData,
        }
    }
}

impl<F, I, T, Z, It, Clk> Operator for JoinTrace<F, I, T, Z, It, Clk>
where
    F: 'static,
    I: 'static,
    T: BatchReader,
    Z: IndexedZSet,
    It: 'static,
    Clk: WithClock<Time = T::Time> + 'static,
{
    fn name(&self) -> Cow<'static, str> {
        Cow::Borrowed("JoinTrace")
    }

    fn location(&self) -> OperatorLocation {
        Some(self.location)
    }

    fn clock_start(&mut self, scope: Scope) {
        if scope == 0 {
            self.empty_input = false;
            self.empty_output = false;
        }
    }

    fn clock_end(&mut self, _scope: Scope) {
        debug_assert!(self
            .output_batchers
            .keys()
            .all(|time| !time.less_equal(&self.clock.time())));
    }

    fn metadata(&self, meta: &mut OperatorMeta) {
        let total_size: usize = self
            .output_batchers
            .values()
            .map(|batcher| batcher.tuples())
            .sum();

        let batch_sizes = MetaItem::Array(
            self.output_batchers
                .values()
                .map(|batcher| {
                    let size = batcher.size_of();

                    MetaItem::Map(
                        metadata! {
                            "allocated" => MetaItem::bytes(size.total_bytes()),
                            "used" => MetaItem::bytes(size.used_bytes()),
                        }
                        .into(),
                    )
                })
                .collect(),
        );

        let bytes = {
            let mut context = Context::new();
            for batcher in self.output_batchers.values() {
                batcher.size_of_with_context(&mut context);
            }

            context.size_of()
        };

        // Find the percentage of consolidated outputs
        let mut output_redundancy = ((self.stats.output_tuples as f64
            - self.stats.produced_tuples as f64)
            / self.stats.output_tuples as f64)
            * 100.0;
        if output_redundancy.is_nan() {
            output_redundancy = 0.0;
        } else if output_redundancy.is_infinite() {
            output_redundancy = 100.0;
        }

        meta.extend(metadata! {
            "total size" => total_size,
            "batch sizes" => batch_sizes,
            "used bytes" => MetaItem::bytes(bytes.used_bytes()),
            "allocations" => bytes.distinct_allocations(),
            "shared bytes" => MetaItem::bytes(bytes.shared_bytes()),
            "left inputs" => self.stats.lhs_tuples,
            "right inputs" => self.stats.rhs_tuples,
            "computed outputs" => self.stats.output_tuples,
            "produced outputs" => self.stats.produced_tuples,
            "output redundancy" => MetaItem::Percent(output_redundancy),
        });
    }

    fn fixedpoint(&self, scope: Scope) -> bool {
        let epoch_end = self.clock.time().epoch_end(scope);
        // We're in a stable state if input and output at the current clock cycle are
        // both empty, and there are no precomputed outputs before the end of the
        // clock epoch.
        self.empty_input
            && self.empty_output
            && self
                .output_batchers
                .keys()
                .all(|time| !time.less_equal(&epoch_end))
    }
}

impl<F, I, T, Z, It, Clk> BinaryOperator<I, T, Z> for JoinTrace<F, I, T, Z, It, Clk>
where
    I: IndexedZSet,
    T: Trace<Key = I::Key, R = I::R>,
    F: Clone + Fn(&I::Key, &I::Val, &T::Val) -> It + 'static,
    Z: IndexedZSet<R = I::R>, /* + ::std::fmt::Display */
    Z::R: ZRingValue,
    It: IntoIterator<Item = (Z::Key, Z::Val)> + 'static,
    Clk: WithClock<Time = T::Time> + 'static,
{
    fn eval(&mut self, index: &I, trace: &T) -> Z {
        self.stats.lhs_tuples += index.len();
        self.stats.rhs_tuples = trace.len();

        self.empty_input = index.is_empty();

        // Buffer to collect output tuples.
        // One allocation per clock tick is acceptable; however the actual output can be
        // larger than `index.len()`.  If re-allocations becomes a problem, we
        // may need to do something smarter, like a chain of buffers.
        // TODO: Sub-scopes can cause a lot of inner clock cycles to be set off, so this
        //       actually could be significant
        let mut output_tuples = Vec::with_capacity(index.len());

        let mut index_cursor = index.cursor();
        let mut trace_cursor = trace.cursor();

        let time = self.clock.time();

        while index_cursor.key_valid() && trace_cursor.key_valid() {
            match index_cursor.key().cmp(trace_cursor.key()) {
                Ordering::Less => index_cursor.seek_key(trace_cursor.key()),
                Ordering::Greater => trace_cursor.seek_key(index_cursor.key()),
                Ordering::Equal => {
                    //println!("key: {}", index_cursor.key(index));

                    while index_cursor.val_valid() {
                        let w1 = index_cursor.weight();
                        let v1 = index_cursor.val();
                        //println!("v1: {}, w1: {}", v1, w1);

                        while trace_cursor.val_valid() {
                            let output =
                                (self.join_func)(index_cursor.key(), v1, trace_cursor.val());
                            for (k, v) in output {
                                trace_cursor.map_times(|ts, w2| {
                                    output_tuples.push((
                                        ts.join(&time),
                                        MaybeUninit::new((
                                            Z::item_from(k.clone(), v.clone()),
                                            w1.mul_by_ref(w2),
                                        )),
                                    ));
                                });
                            }
                            trace_cursor.step_val();
                        }
                        trace_cursor.rewind_vals();
                        index_cursor.step_val();
                    }

                    index_cursor.step_key();
                    trace_cursor.step_key();
                }
            }
        }

        self.stats.output_tuples += output_tuples.len();
        // Sort `output_tuples` by timestamp and push all tuples for each unique
        // timestamp to the appropriate batcher.
        output_tuples.sort_by(|(t1, _), (t2, _)| t1.cmp(t2));

        /// Ensures that we don't leak any values if we panic while creating
        /// output batches
        struct BatchPusher<T, U> {
            // Invariant: `tuples[start..]` are all initialized
            start: usize,
            tuples: Vec<(T, MaybeUninit<U>)>,
        }

        impl<T, U> BatchPusher<T, U> {
            const fn new(tuples: Vec<(T, MaybeUninit<U>)>) -> Self {
                Self { start: 0, tuples }
            }

            fn is_empty(&self) -> bool {
                self.start >= self.tuples.len()
            }

            fn current_time(&self) -> &T {
                &self.tuples[self.start].0
            }
        }

        impl<T, U> Drop for BatchPusher<T, U> {
            fn drop(&mut self) {
                if needs_drop::<U>() {
                    for (_, tuple) in &mut self.tuples[self.start..] {
                        // Safety: `tuples[start..]` are all initialized
                        unsafe { tuple.assume_init_drop() };
                    }
                }
            }
        }

        let mut batch = Vec::new();
        let mut pusher = BatchPusher::new(output_tuples);

        while !pusher.is_empty() {
            let batch_time = pusher.current_time().clone();

            let run_length =
                pusher.tuples[pusher.start..].partition_point(|(time, _)| *time == batch_time);
            batch.reserve(run_length);

            let run = pusher.tuples[pusher.start..pusher.start + run_length]
                .iter_mut()
                // Safety: All values in `pusher.tuples[pusher.start..]` are initialized and we
                //         mark the values we take from it by incrementing `pusher.start`
                .map(|(_, tuple)| unsafe { tuple.assume_init_read() });
            pusher.start += run_length;
            batch.extend(run);

            self.output_batchers
                .entry(batch_time)
                .or_insert_with(|| Z::Batcher::new_batcher(()))
                .push_batch(&mut batch);
            batch.clear();
        }

        // Finalize the batch for the current timestamp and return it.
        let batcher = self
            .output_batchers
            .remove(&time)
            .unwrap_or_else(|| Z::Batcher::new_batcher(()));

        let result = batcher.seal();
        self.stats.produced_tuples += result.len();
        self.empty_output = result.is_empty();

        result
    }
}

#[cfg(test)]
mod test {
    use crate::{
        circuit::WithClock,
        indexed_zset,
        operator::{DelayedFeedback, FilterMap, Generator},
        trace::{
            ord::{OrdIndexedZSet, OrdZSet},
            Batch,
        },
        zset, Circuit, DBTimestamp, RootCircuit, Runtime, Stream, Timestamp,
    };
    use size_of::SizeOf;
    use std::{
        fmt::{Display, Formatter},
        hash::Hash,
        sync::{Arc, Mutex},
        vec,
    };

    #[test]
    fn join_test() {
        let circuit = RootCircuit::build(move |circuit| {
            let mut input1 = vec![
                zset! {
                    (1, "a".to_string()) => 1,
                    (1, "b".to_string()) => 2,
                    (2, "c".to_string()) => 3,
                    (2, "d".to_string()) => 4,
                    (3, "e".to_string()) => 5,
                    (3, "f".to_string()) => -2,
                },
                zset! {(1, "a".to_string()) => 1},
                zset! {(1, "a".to_string()) => 1},
                zset! {(4, "n".to_string()) => 2},
                zset! {(1, "a".to_string()) => 0},
            ]
            .into_iter();
            let mut input2 = vec![
                zset! {
                    (2, "g".to_string()) => 3,
                    (2, "h".to_string()) => 4,
                    (3, "i".to_string()) => 5,
                    (3, "j".to_string()) => -2,
                    (4, "k".to_string()) => 5,
                    (4, "l".to_string()) => -2,
                },
                zset! {(1, "b".to_string()) => 1},
                zset! {(4, "m".to_string()) => 1},
                zset! {},
                zset! {},
            ]
            .into_iter();
            let mut outputs = vec![
                zset! {
                    (2, "c g".to_string()) => 9,
                    (2, "c h".to_string()) => 12,
                    (2, "d g".to_string()) => 12,
                    (2, "d h".to_string()) => 16,
                    (3, "e i".to_string()) => 25,
                    (3, "e j".to_string()) => -10,
                    (3, "f i".to_string()) => -10,
                    (3, "f j".to_string()) => 4
                },
                zset! {
                    (1, "a b".to_string()) => 1,
                },
                zset! {},
                zset! {},
                zset! {},
            ]
            .into_iter();
            let inc_outputs_vec = vec![
                zset! {
                    (2, "c g".to_string()) => 9,
                    (2, "c h".to_string()) => 12,
                    (2, "d g".to_string()) => 12,
                    (2, "d h".to_string()) => 16,
                    (3, "e i".to_string()) => 25,
                    (3, "e j".to_string()) => -10,
                    (3, "f i".to_string()) => -10,
                    (3, "f j".to_string()) => 4
                },
                zset! {
                    (1, "a b".to_string()) => 2,
                    (1, "b b".to_string()) => 2,
                },
                zset! {
                    (1, "a b".to_string()) => 1,
                },
                zset! {
                    (4, "n k".to_string()) => 10,
                    (4, "n l".to_string()) => -4,
                    (4, "n m".to_string()) => 2,
                },
                zset! {},
            ];

            let mut inc_outputs = inc_outputs_vec.clone().into_iter();
            let mut inc_outputs2 = inc_outputs_vec.into_iter();

            let index1: Stream<_, OrdIndexedZSet<usize, String, isize>> = circuit
                .add_source(Generator::new(move || {
                    if Runtime::worker_index() == 0 {
                        input1.next().unwrap()
                    } else {
                        <OrdZSet<_, _>>::empty(())
                    }
                }))
                .index();
            let index2: Stream<_, OrdIndexedZSet<usize, String, isize>> = circuit
                .add_source(Generator::new(move || {
                    if Runtime::worker_index() == 0 {
                        input2.next().unwrap()
                    } else {
                        <OrdZSet<_, _>>::empty(())
                    }
                }))
                .index();
            index1
                .stream_join(&index2, |&k: &usize, s1, s2| (k, format!("{} {}", s1, s2)))
                .gather(0)
                .inspect(move |fm: &OrdZSet<(usize, String), _>| {
                    if Runtime::worker_index() == 0 {
                        assert_eq!(fm, &outputs.next().unwrap())
                    }
                });
            index1
                .join_incremental(&index2, |&k: &usize, s1, s2| (k, format!("{} {}", s1, s2)))
                .gather(0)
                .inspect(move |fm: &OrdZSet<(usize, String), _>| {
                    if Runtime::worker_index() == 0 {
                        assert_eq!(fm, &inc_outputs.next().unwrap())
                    }
                });

            index1
                .join(&index2, |&k: &usize, s1, s2| (k, format!("{} {}", s1, s2)))
                .gather(0)
                .inspect(move |fm: &OrdZSet<(usize, String), _>| {
                    if Runtime::worker_index() == 0 {
                        assert_eq!(fm, &inc_outputs2.next().unwrap())
                    }
                });
            Ok(())
        })
        .unwrap()
        .0;

        for _ in 0..5 {
            circuit.step().unwrap();
        }
    }

    fn do_join_test_mt(workers: usize) {
        let hruntime = Runtime::run(workers, || {
            join_test();
        });

        hruntime.join().unwrap();
    }

    #[test]
    fn join_test_mt() {
        do_join_test_mt(1);
        do_join_test_mt(2);
        do_join_test_mt(4);
        do_join_test_mt(16);
    }

    // Compute pairwise reachability relation between graph nodes as the
    // transitive closure of the edge relation.
    #[test]
    fn join_trace_test() {
        let circuit = RootCircuit::build(move |circuit| {
            // Changes to the edges relation.
            let mut edges: vec::IntoIter<OrdZSet<(usize, usize), isize>> = vec![
                zset! { (1, 2) => 1 },
                zset! { (2, 3) => 1},
                zset! { (1, 3) => 1},
                zset! { (3, 1) => 1},
                zset! { (3, 1) => -1},
                zset! { (1, 2) => -1},
                zset! { (2, 4) => 1, (4, 1) => 1 },
                zset! { (2, 3) => -1, (3, 2) => 1 },
            ]
            .into_iter();

            // Expected content of the reachability relation.
            let mut outputs: vec::IntoIter<OrdZSet<(usize, usize), isize>> = vec![
                zset! { (1, 2) => 1 },
                zset! { (1, 2) => 1, (2, 3) => 1, (1, 3) => 1 },
                zset! { (1, 2) => 1, (2, 3) => 1, (1, 3) => 1 },
                zset! { (1, 1) => 1, (2, 2) => 1, (3, 3) => 1, (1, 2) => 1, (1, 3) => 1, (2, 3) => 1, (2, 1) => 1, (3, 1) => 1, (3, 2) => 1},
                zset! { (1, 2) => 1, (2, 3) => 1, (1, 3) => 1 },
                zset! { (2, 3) => 1, (1, 3) => 1 },
                zset! { (1, 3) => 1, (2, 3) => 1, (2, 4) => 1, (2, 1) => 1, (4, 1) => 1, (4, 3) => 1 },
                zset! { (1, 1) => 1, (2, 2) => 1, (3, 3) => 1, (4, 4) => 1,
                              (1, 2) => 1, (1, 3) => 1, (1, 4) => 1,
                              (2, 1) => 1, (2, 3) => 1, (2, 4) => 1,
                              (3, 1) => 1, (3, 2) => 1, (3, 4) => 1,
                              (4, 1) => 1, (4, 2) => 1, (4, 3) => 1 },
            ]
            .into_iter();

            let edges: Stream<_, OrdZSet<(usize, usize), isize>> =
                circuit
                    .add_source(Generator::new(move || edges.next().unwrap()));

            let paths = circuit.recursive(|child, paths_delayed: Stream<_, OrdZSet<(usize, usize), isize>>| {
                // ```text
                //                             distinct
                //               ┌───┐          ┌───┐
                // edges         │   │          │   │  paths
                // ────┬────────►│ + ├──────────┤   ├────────┬───►
                //     │         │   │          │   │        │
                //     │         └───┘          └───┘        │
                //     │           ▲                         │
                //     │           │                         │
                //     │         ┌─┴─┐                       │
                //     │         │   │                       │
                //     └────────►│ X │ ◄─────────────────────┘
                //               │   │
                //               └───┘
                //               join
                // ```
                let edges = edges.delta0(child);

                let paths_inverted: Stream<_, OrdZSet<(usize, usize), isize>> = paths_delayed
                    .map(|&(x, y)| (y, x));

                let paths_inverted_indexed = paths_inverted.index();
                let edges_indexed = edges.index();

                Ok(edges.plus(&paths_inverted_indexed.join(&edges_indexed, |_via, from, to| (*from, *to))))
            })
            .unwrap();

            paths.integrate().stream_distinct().inspect(move |ps| {
                assert_eq!(*ps, outputs.next().unwrap());
            });
            Ok(())
        })
        .unwrap().0;

        for _ in 0..8 {
            //eprintln!("{}", i);
            circuit.step().unwrap();
        }
    }

    #[derive(
        Clone,
        Debug,
        Default,
        Hash,
        Ord,
        PartialOrd,
        Eq,
        PartialEq,
        SizeOf,
        bincode::Decode,
        bincode::Encode,
    )]
    struct Label(pub usize, pub u16);

    impl Display for Label {
        fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), std::fmt::Error> {
            write!(f, "L({},{})", self.0, self.1)
        }
    }

    #[derive(
        Clone,
        Debug,
        Default,
        Ord,
        PartialOrd,
        Hash,
        Eq,
        PartialEq,
        SizeOf,
        bincode::Decode,
        bincode::Encode,
    )]
    struct Edge(pub usize, pub usize);

    impl Display for Edge {
        fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), std::fmt::Error> {
            write!(f, "E({},{})", self.0, self.1)
        }
    }
    // Recursively propagate node labels in an acyclic graph.
    // The reason for supporting acyclic graphs only is that we use this to test
    // the join operator in isolation, so we don't want to use `distinct`.
    fn propagate<C>(
        circuit: &C,
        edges: &Stream<C, OrdZSet<Edge, isize>>,
        labels: &Stream<C, OrdZSet<Label, isize>>,
    ) -> Stream<C, OrdZSet<Label, isize>>
    where
        C: Circuit,
        <<C as WithClock>::Time as Timestamp>::Nested: DBTimestamp,
    {
        let computed_labels = circuit
            .fixedpoint(|child| {
                let edges = edges.delta0(child);
                let labels = labels.delta0(child);

                let computed_labels = <DelayedFeedback<_, OrdZSet<_, _>>>::new(child);
                let result: Stream<_, OrdZSet<Label, isize>> =
                    labels.plus(computed_labels.stream());

                computed_labels.connect(&result.index_with(|label| (label.0, label.1)).join(
                    &edges.index_with(|edge| (edge.0, edge.1)),
                    |_from, label, to| Label(*to, *label),
                ));

                Ok(result.integrate_trace().export())
            })
            .unwrap();

        computed_labels.consolidate()
    }

    #[test]
    fn propagate_test() {
        let circuit = RootCircuit::build(move |circuit| {
            let mut edges: vec::IntoIter<OrdZSet<Edge, isize>> = vec![
                zset! { Edge(1, 2) => 1, Edge(1, 3) => 1, Edge(2, 4) => 1, Edge(3, 4) => 1 },
                zset! { Edge(5, 7) => 1, Edge(6, 7) => 1 },
                zset! { Edge(4, 5) => 1, Edge(4, 6) => 1, },
                zset! { Edge(3, 8) => 1, Edge(8, 9) => 1 },
                zset! { Edge(2, 4) => -1, Edge(7, 10) => 1 },
                zset! { Edge(3, 4) => -1 },
                zset! { Edge(1, 4) => 1 },
                zset! { Edge(9, 7) => 1 },
            ]
            .into_iter();

            let mut labels: vec::IntoIter<OrdZSet<Label, isize>> = vec![
                zset! { Label(1, 0) => 1 },
                zset! { Label(4, 1) => 1 },
                zset! { },
                zset! { Label(1, 0) => -1, Label(1, 2) => 1 },
                zset! { },
                zset! { Label(8, 3) => 1 },
                zset! { Label(4, 1) => -1 },
                zset! { },
            ]
            .into_iter();

            let mut outputs: vec::IntoIter<OrdZSet<Label, isize>> = vec![
                zset! { Label(1, 0) => 1, Label(2, 0) => 1, Label(3, 0) => 1, Label(4, 0) => 2 },
                zset! { Label(4, 1) => 1 },
                zset! { Label(5, 0) => 2, Label(5, 1) => 1, Label(6, 0) => 2, Label(6, 1) => 1, Label(7, 0) => 4, Label(7, 1) => 2 },
                zset! { Label(1, 0) => -1, Label(1, 2) => 1, Label(2, 0) => -1, Label(2, 2) => 1, Label(3, 0) => -1, Label(3, 2) => 1, Label(4, 0) => -2, Label(4, 2) => 2, Label(5, 0) => -2, Label(5, 2) => 2, Label(6, 0) => -2, Label(6, 2) => 2, Label(7, 0) => -4, Label(7, 2) => 4, Label(8, 2) => 1, Label(9, 2) => 1 },
                zset! { Label(4, 2) => -1, Label(5, 2) => -1, Label(6, 2) => -1, Label(7, 2) => -2, Label(10, 1) => 2, Label(10, 2) => 2 },
                zset! { Label(4, 2) => -1, Label(5, 2) => -1, Label(6, 2) => -1, Label(7, 2) => -2, Label(8, 3) => 1, Label(9, 3) => 1, Label(10, 2) => -2 },
                zset! { Label(4, 1) => -1, Label(4, 2) => 1, Label(5, 1) => -1, Label(5, 2) => 1, Label(6, 1) => -1, Label(6, 2) => 1, Label(7, 1) => -2, Label(7, 2) => 2, Label(10, 1) => -2, Label(10, 2) => 2 },
                zset! { Label(7, 2) => 1, Label(7, 3) => 1, Label(10, 2) => 1, Label(10, 3) => 1 },
            ]
            .into_iter();

            let edges: Stream<_, OrdZSet<Edge, isize>> =
                circuit
                    .add_source(Generator::new(move || edges.next().unwrap()));

            let labels: Stream<_, OrdZSet<Label, isize>> =
                circuit
                    .add_source(Generator::new(move || labels.next().unwrap()));

            propagate(circuit, &edges, &labels).inspect(move |labeled| {
                assert_eq!(*labeled, outputs.next().unwrap());
            });
            Ok(())
        })
        .unwrap().0;

        for _ in 0..8 {
            //eprintln!("{}", i);
            circuit.step().unwrap();
        }
    }

    #[test]
    fn propagate_nested_test() {
        let circuit = RootCircuit::build(move |circuit| {
            let mut edges: vec::IntoIter<OrdZSet<Edge, isize>> = vec![
                zset! { Edge(1, 2) => 1, Edge(1, 3) => 1, Edge(2, 4) => 1, Edge(3, 4) => 1 },
                zset! { Edge(5, 7) => 1, Edge(6, 7) => 1 },
                zset! { Edge(4, 5) => 1, Edge(4, 6) => 1 },
                zset! { Edge(3, 8) => 1, Edge(8, 9) => 1 },
                zset! { Edge(2, 4) => -1, Edge(7, 10) => 1 },
                zset! { Edge(3, 4) => -1 },
                zset! { Edge(1, 4) => 1 },
                zset! { Edge(9, 7) => 1 },
            ]
            .into_iter();

            let mut labels: vec::IntoIter<OrdZSet<Label, isize>> = vec![
                zset! { Label(1, 0) => 1 },
                zset! { Label(4, 1) => 1 },
                zset! { },
                zset! { Label(1, 0) => -1, Label(1, 2) => 1 },
                zset! { },
                zset! { Label(8, 3) => 1 },
                zset! { Label(4, 1) => -1 },
                zset! { },
            ]
            .into_iter();

            let mut outputs: vec::IntoIter<OrdZSet<Label, isize>> = vec![
                zset!{ Label(1,0) => 2, Label(2,0) => 3, Label(3,0) => 3, Label(4,0) => 8 },
                zset!{ Label(4,1) => 2 },
                zset!{ Label(5,0) => 10, Label(5,1) => 3, Label(6,0) => 10, Label(6,1) => 3, Label(7,0) => 24, Label(7,1) => 8 },
                zset!{ Label(1,0) => -2, Label(1,2) => 2, Label(2,0) => -3, Label(2,2) => 3, Label(3,0) => -3, Label(3,2) => 3,
                       Label(4,0) => -8, Label(4,2) => 8, Label(5,0) => -10, Label(5,2) => 10, Label(6,0) => -10, Label(6,2) => 10,
                       Label(7,0) => -24, Label(7,2) => 24, Label(8,2) => 4, Label(9,2) => 5 },
                zset!{ Label(4,2) => -4, Label(5,2) => -5, Label(6,2) => -5, Label(7,2) => -12, Label(10,1) => 10, Label(10,2) => 14 },
                zset!{ Label(4,2) => -4, Label(5,2) => -5, Label(6,2) => -5, Label(7,2) => -12, Label(8,3) => 2,
                       Label(9,3) => 3, Label(10,2) => -14 },
                zset!{ Label(4,1) => -2, Label(4,2) => 3, Label(5,1) => -3, Label(5,2) => 4, Label(6,1) => -3, Label(6,2) => 4,
                       Label(7,1) => -8, Label(7,2) => 10, Label(10,1) => -10, Label(10,2) => 12 },
                zset!{ Label(7,2) => 6, Label(7,3) => 4, Label(10,2) => 7, Label(10,3) => 5 },
            ].into_iter();

            let edges: Stream<_, OrdZSet<Edge, isize>> =
                circuit
                    .add_source(Generator::new(move || edges.next().unwrap()));

            let labels: Stream<_, OrdZSet<Label, isize>> =
                circuit
                    .add_source(Generator::new(move || labels.next().unwrap()));

            let result = circuit.iterate(|child| {

                let counter = std::cell::RefCell::new(0);
                let edges = edges.delta0(child);
                let labels = labels.delta0(child);

                let computed_labels = <DelayedFeedback<_, OrdZSet<_, _>>>::new(child);
                let result = propagate(child, &edges, &labels.plus(computed_labels.stream()));
                computed_labels.connect(&result);

                //result.inspect(|res: &OrdZSet<Label, isize>| println!("delta: {}", res));
                Ok((move || {
                    let mut counter = counter.borrow_mut();
                    // reset to 0 on each outer loop iteration.
                    if *counter == 2 {
                        *counter = 0;
                    }
                    *counter += 1;
                    //println!("counter: {}", *counter);
                    Ok(*counter == 2)
                },
                result.integrate_trace().export()))
            }).unwrap();

            result.consolidate().inspect(move |res: &OrdZSet<Label, isize>| {
                assert_eq!(*res, outputs.next().unwrap());
            });
            Ok(())
        })
        .unwrap().0;

        for _ in 0..8 {
            //eprintln!("{}", i);
            circuit.step().unwrap();
        }
    }

    #[test]
    fn antijoin_test() {
        let output = Arc::new(Mutex::new(OrdIndexedZSet::empty(())));
        let output_clone = output.clone();

        let (mut circuit, (input1, input2)) = Runtime::init_circuit(4, move |circuit| {
            let (input1, input_handle1) = circuit.add_input_indexed_zset::<usize, usize, isize>();
            let (input2, input_handle2) = circuit.add_input_indexed_zset::<usize, usize, isize>();

            input1.antijoin(&input2).gather(0).inspect(move |batch| {
                if Runtime::worker_index() == 0 {
                    *output_clone.lock().unwrap() = batch.clone();
                }
            });

            Ok((input_handle1, input_handle2))
        })
        .unwrap();

        input1.append(&mut vec![
            (1, (0, 1)),
            (1, (1, 2)),
            (2, (0, 1)),
            (2, (1, 1)),
        ]);
        circuit.step().unwrap();
        assert_eq!(
            &*output.lock().unwrap(),
            &indexed_zset! { 1 => { 0 => 1, 1 => 2}, 2 => { 0 => 1, 1 => 1 } }
        );

        input1.append(&mut vec![(3, (1, 1))]);
        circuit.step().unwrap();
        assert_eq!(&*output.lock().unwrap(), &indexed_zset! { 3 => { 1 => 1 } });

        input2.append(&mut vec![(1, (1, 3))]);
        circuit.step().unwrap();
        assert_eq!(
            &*output.lock().unwrap(),
            &indexed_zset! { 1 => { 0 => -1, 1 => -2 } }
        );

        input2.append(&mut vec![(2, (5, 1))]);
        input1.append(&mut vec![(2, (2, 1)), (4, (1, 1))]);
        circuit.step().unwrap();
        assert_eq!(
            &*output.lock().unwrap(),
            &indexed_zset! { 2 => { 0 => -1, 1 => -1 }, 4 => { 1 => 1 } }
        );

        circuit.kill().unwrap();
    }
}
