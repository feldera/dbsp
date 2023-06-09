//! Aggregation operators.

use std::{
    any::TypeId,
    borrow::Cow,
    cmp::{min, Ordering},
    collections::{BTreeMap, BTreeSet},
    marker::PhantomData,
};

use crate::{
    algebra::{
        DefaultSemigroup, GroupValue, HasOne, HasZero, IndexedZSet, Lattice, MulByRef,
        PartialOrder, Semigroup, ZRingValue,
    },
    circuit::{
        operator_traits::{BinaryOperator, Operator, UnaryOperator},
        Circuit, Scope, Stream, WithClock,
    },
    time::Timestamp,
    trace::{
        cursor::{Cursor, CursorGroup},
        Batch, BatchReader, Builder, Spine,
    },
    DBData, DBTimestamp, DBWeight, OrdIndexedZSet, OrdZSet,
};

// Some standard aggregators.
mod average;
mod fold;
mod max;
mod min;

pub use average::Avg;
use bincode::{Decode, Encode};
pub use fold::Fold;
pub use max::{Max, MaxSemigroup};
pub use min::{Min, MinSemigroup};

/// A trait for aggregator objects.  An aggregator summarizes the contents
/// of a Z-set into a single value.
///
/// This trait supports two aggregation methods that can be combined in
/// various ways to compute the aggregate efficiently.  First, the
/// [`aggregate`](`Self::aggregate`) method takes a cursor pointing to
/// the first key of a Z-set and scans the cursor to compute the aggregate
/// over all values in the cursor.  Second, the
/// [`Semigroup`](`Self::Semigroup`) associated type allows aggregating
/// partitioned Z-sets by combining aggregates computed over individual
/// partitions (e.g., computed by different worker threads).
///
/// This design requires aggregate values to form a semigroup with a `+`
/// operation such that `Agg(x + y) = Agg(x) + Agg(y)`, i.e., aggregating
/// a union of two Z-sets must produce the same result as the sum of
/// individual aggregates.  Not all aggregates have this property.  E.g.,
/// the average value of a Z-set cannot be computed by combining the
/// averages of its subsets.  We can get around this problem by representing
/// average as `(sum, count)` tuple with point-wise `+` operator
/// `(sum1, count1) + (sum2, count2) = (sum1+sum2, count1+count2)`.
/// The final result is converted to an actual average value by dividing
/// the first element of the tuple by the second.  This is a general technique
/// that works for all aggregators (although it may not always be optimal).
///
/// To support such aggregates, this trait distinguishes between the
/// `Accumulator` type returned by [`aggregate`](`Self::aggregate`),
/// which must implement
/// [`trait Semigroup`](`crate::algebra::Semigroup`), and the final
/// [`Output`](`Self::Output`) of the aggregator.  The latter is
/// computed by applying the [`finalize`](`Self::finalize`) method
/// to the final value of the accumulator.
///
/// This is a low-level trait that is mostly used to build libraries of
/// aggregators.  Users will typically work with ready-made implementations
/// like [`Min`] and [`Fold`].
// TODO: Owned aggregation using `Consumer`
pub trait Aggregator<K, T, R>: Clone + 'static {
    /// Accumulator type returned by [`Self::finalize`].
    type Accumulator: DBData;

    /// Semigroup structure over aggregate values.
    ///
    /// Can be used to separately aggregate subsets of values (e.g., in
    /// different worker threads) and combine the results.  This
    /// `Semigroup` implementation must be consistent with `Self::aggregate`,
    /// meaning that computing the aggregate piecewise and combining
    /// the results using `Self::Semigroup` should yield the same value as
    /// aggregating the entire input using `Self::aggregate`.
    // TODO: We currently only use this with `radix_tree`, which only
    // requires the semigroup structure (i.e., associativity).  In the future
    // we will also use this in computing regular aggregates by combining
    // per-worker aggregates computes over arbitrary subsets of values,
    // which additionally requires commutativity.  Do we want to introduce
    // the `CommutativeSemigroup` trait?
    type Semigroup: Semigroup<Self::Accumulator>;

    /// Aggregate type produced by this aggregator.
    type Output: DBData;

    /// Takes a cursor pointing to the first key of a Z-set and outputs
    /// an aggregate of the Z-set.
    ///
    /// # Invariants
    ///
    /// This is a low-level API that relies on the implementer to maintain the
    /// following invariants:
    ///
    /// * The method must return `None` if the total weight of each key is zero.
    ///   It must return `Some` otherwise.
    fn aggregate<C>(&self, cursor: &mut C) -> Option<Self::Accumulator>
    where
        C: Cursor<K, (), T, R>;

    /// Compute the final value of the aggregate.
    fn finalize(&self, accumulator: Self::Accumulator) -> Self::Output;

    /// Applies `aggregate` to `cursor` followed by `finalize`.
    fn aggregate_and_finalize<C>(&self, cursor: &mut C) -> Option<Self::Output>
    where
        C: Cursor<K, (), T, R>,
    {
        self.aggregate(cursor).map(|x| self.finalize(x))
    }
}

/// Aggregator used internally by [`Stream::aggregate_linear`].  Computes
/// the total sum of weights.
#[derive(Clone)]
struct WeightedCount;

impl<T, R> Aggregator<(), T, R> for WeightedCount
where
    T: Timestamp,
    R: DBWeight,
{
    type Accumulator = R;
    type Output = R;
    type Semigroup = DefaultSemigroup<R>;

    fn aggregate<C>(&self, cursor: &mut C) -> Option<Self::Output>
    where
        C: Cursor<(), (), T, R>,
    {
        let mut weight = R::zero();

        cursor.map_times(|_t, w| weight.add_assign_by_ref(w));

        if weight.is_zero() {
            None
        } else {
            Some(weight)
        }
    }

    fn finalize(&self, accumulator: Self::Accumulator) -> Self::Output {
        accumulator
    }
}

impl<C, Z> Stream<C, Z>
where
    C: Circuit,
    <C as WithClock>::Time: DBTimestamp,
    Z: Clone + Encode + Decode + 'static,
{
    /// Aggregate values associated with each key in an indexed Z-set.
    ///
    /// An indexed Z-set `IndexedZSet<K, V, R>` maps each key into a
    /// set of `(value, weight)` tuples `(V, R)`.  These tuples form
    /// a nested Z-set `ZSet<V, R>`.  This method applies `aggregator`
    /// to each such Z-set and adds it to the output indexed Z-set with
    /// weight `+1`.
    ///
    /// [`Min`], [`Max`], and [`Fold`] are provided as example `Aggregator`s.
    #[allow(clippy::type_complexity)]
    pub fn stream_aggregate<A>(
        &self,
        aggregator: A,
    ) -> Stream<C, OrdIndexedZSet<Z::Key, A::Output, Z::R>>
    where
        Z: IndexedZSet + Send,
        A: Aggregator<Z::Val, (), Z::R>,
        Z::R: ZRingValue,
    {
        self.stream_aggregate_generic(aggregator)
    }

    /// Like [`Self::stream_aggregate`], but can return any batch type.
    pub fn stream_aggregate_generic<A, O>(&self, aggregator: A) -> Stream<C, O>
    where
        Z: IndexedZSet + Send,
        A: Aggregator<Z::Val, (), Z::R>,
        O: IndexedZSet<Key = Z::Key, Val = A::Output>,
        O::R: ZRingValue,
    {
        self.circuit()
            .add_unary_operator(Aggregate::new(aggregator), &self.shard())
            .mark_sharded()
    }

    /// A version of [`Self::stream_aggregate`] optimized for linear aggregation
    /// functions.
    ///
    /// This method only works for linear aggregation functions `f`, i.e.,
    /// functions that satisfy `f(a+b) = f(a) + f(b)`.  It will produce
    /// incorrect results if `f` is not linear.  Linearity means that
    /// `f` can be defined per `(key, value)` tuple.
    pub fn stream_aggregate_linear<F, A>(&self, f: F) -> Stream<C, OrdIndexedZSet<Z::Key, A, Z::R>>
    where
        Z: IndexedZSet + Send,
        A: DBData + MulByRef<Z::R, Output = A> + GroupValue,
        Z::R: ZRingValue,
        F: Fn(&Z::Key, &Z::Val) -> A + Clone + 'static,
    {
        self.stream_aggregate_linear_generic(f)
    }

    /// Like [`Self::stream_aggregate_linear`], but can return any batch type.
    pub fn stream_aggregate_linear_generic<F, A, O>(&self, f: F) -> Stream<C, O>
    where
        Z: IndexedZSet + Send,
        A: DBData + MulByRef<Z::R, Output = A> + GroupValue,
        O: IndexedZSet<Key = Z::Key, Val = A>,
        O::R: ZRingValue,
        F: Fn(&Z::Key, &Z::Val) -> A + Clone + 'static,
    {
        self.weigh(f).stream_aggregate_generic(WeightedCount)
    }

    /// Incremental aggregation operator.
    ///
    /// This operator is an incremental version of [`Self::stream_aggregate`].
    /// It transforms a stream of changes to an indexed Z-set to a stream of
    /// changes to its aggregate computed by applying `aggregator` to each
    /// key in the input.
    ///
    /// [`Min`], [`Max`], and [`Fold`] are provided as example `Aggregator`s.
    #[allow(clippy::type_complexity)]
    pub fn aggregate<A>(&self, aggregator: A) -> Stream<C, OrdIndexedZSet<Z::Key, A::Output, Z::R>>
    where
        Z: IndexedZSet + Send,
        A: Aggregator<Z::Val, <C as WithClock>::Time, Z::R>,
        Z::R: ZRingValue,
    {
        self.aggregate_generic::<A, OrdIndexedZSet<Z::Key, A::Output, Z::R>>(aggregator)
    }

    /// Like [`Self::aggregate`], but can return any batch type.
    pub fn aggregate_generic<A, O>(&self, aggregator: A) -> Stream<C, O>
    where
        Z: IndexedZSet + Send,
        A: Aggregator<Z::Val, <C as WithClock>::Time, Z::R>,
        O: Batch<Key = Z::Key, Val = A::Output, Time = ()>,
        O::R: ZRingValue,
    {
        let circuit = self.circuit();
        let stream = self.shard();

        // We construct the following circuit.  See `AggregateIncremental` documentation
        // for details.
        //
        // ```
        //          ┌────────────────────────────────────────┐
        //          │                                        │
        //          │                                        ▼
        //  stream  │     ┌─────┐  stream.trace()  ┌────────────────────┐      ┌──────┐
        // ─────────┴─────┤trace├─────────────────►│AggregateIncremental├─────►│upsert├──────►
        //                └─────┘                  └────────────────────┘      └──────┘
        // ```

        circuit
            .add_binary_operator(
                AggregateIncremental::new(aggregator, circuit.clone()),
                &stream,
                &stream.trace::<Spine<<<C as WithClock>::Time as Timestamp>::OrdValBatch<Z::Key, Z::Val, Z::R>>>(),
            )
            .upsert::<O>()
            .mark_sharded()
    }

    /// A version of [`Self::aggregate`] optimized for linear
    /// aggregation functions.
    ///
    /// This method only works for linear aggregation functions `f`, i.e.,
    /// functions that satisfy `f(a+b) = f(a) + f(b)`.  It will produce
    /// incorrect results if `f` is not linear.  Linearity means that
    /// `f` can be defined per `(key, value)` tuple.
    pub fn aggregate_linear<F, A>(&self, f: F) -> Stream<C, OrdIndexedZSet<Z::Key, A, Z::R>>
    where
        Z: IndexedZSet,
        A: DBData + MulByRef<Z::R, Output = A> + GroupValue,
        F: Fn(&Z::Key, &Z::Val) -> A + Clone + 'static,
        Z::R: ZRingValue,
    {
        self.aggregate_linear_generic(f)
    }

    /// Like [`Self::aggregate_linear`], but can return any batch type.
    pub fn aggregate_linear_generic<F, O>(&self, f: F) -> Stream<C, O>
    where
        Z: IndexedZSet,
        F: Fn(&Z::Key, &Z::Val) -> O::Val + Clone + 'static,
        O: Batch<Key = Z::Key, Time = ()>,
        O::R: ZRingValue,
        O::Val: MulByRef<Z::R, Output = O::Val> + GroupValue,
    {
        self.weigh(f).aggregate_generic(WeightedCount)
    }

    /// Convert indexed Z-set `Z` into a Z-set where the weight of each key
    /// is computed as:
    ///
    /// ```text
    ///    __
    ///    ╲
    ///    ╱ f(k,v) * w
    ///    ‾‾
    /// (k,v,w) ∈ Z
    /// ```
    ///
    /// Discards the values from the input.
    ///
    /// This is a linear operator.
    pub fn weigh<F, T>(&self, f: F) -> Stream<C, OrdZSet<Z::Key, T>>
    where
        Z: IndexedZSet,
        F: Fn(&Z::Key, &Z::Val) -> T + 'static,
        T: DBWeight + MulByRef<Z::R, Output = T>,
    {
        self.weigh_generic::<_, OrdZSet<_, _>>(f)
    }

    /// Like [`Self::weigh`], but can return any batch type.
    pub fn weigh_generic<F, O>(&self, f: F) -> Stream<C, O>
    where
        Z: IndexedZSet,
        F: Fn(&Z::Key, &Z::Val) -> O::R + 'static,
        O: Batch<Key = Z::Key, Val = (), Time = ()>,
        O::R: MulByRef<Z::R, Output = O::R>,
    {
        let output = self
            .try_sharded_version()
            .apply_named("Weigh", move |batch| {
                let mut delta = <O::Builder>::with_capacity((), batch.key_count());
                let mut cursor = batch.cursor();
                while cursor.key_valid() {
                    let mut agg: O::R = HasZero::zero();
                    while cursor.val_valid() {
                        agg += f(cursor.key(), cursor.val()).mul_by_ref(&cursor.weight());
                        cursor.step_val();
                    }
                    if !agg.is_zero() {
                        delta.push((O::item_from(cursor.key().clone(), ()), agg));
                    }
                    cursor.step_key();
                }
                delta.done()
            });

        output.mark_sharded_if(self);
        output
    }
}

/// Non-incremental aggregation operator.
struct Aggregate<Z, A, O> {
    aggregator: A,
    _type: PhantomData<(Z, O)>,
}

impl<Z, A, O> Aggregate<Z, A, O> {
    pub fn new(aggregator: A) -> Self {
        Self {
            aggregator,
            _type: PhantomData,
        }
    }
}

impl<Z, A, O> Operator for Aggregate<Z, A, O>
where
    Z: 'static,
    A: 'static,
    O: 'static,
{
    fn name(&self) -> Cow<'static, str> {
        Cow::from("Aggregate")
    }
    fn fixedpoint(&self, _scope: Scope) -> bool {
        true
    }
}

impl<Z, A, O> UnaryOperator<Z, O> for Aggregate<Z, A, O>
where
    Z: IndexedZSet,
    A: Aggregator<Z::Val, (), Z::R>,
    O: IndexedZSet<Key = Z::Key, Val = A::Output>,
    O::R: ZRingValue,
{
    fn eval(&mut self, i: &Z) -> O {
        let mut builder = O::Builder::with_capacity((), i.len());

        let mut cursor = i.cursor();
        while cursor.key_valid() {
            if let Some(agg) = self
                .aggregator
                .aggregate_and_finalize(&mut CursorGroup::new(&mut cursor, ()))
            {
                builder.push((O::item_from(cursor.key().clone(), agg), O::R::one()));
            }
            cursor.step_key();
        }
        builder.done()
    }
}

/// Incremental version of the `Aggregate` operator that works
/// in arbitrarily nested scopes.
///
/// This is a binary operator with the following inputs:
/// * `delta` - stream of changes to the input indexed Z-set, only used to
///   compute the set of affected keys.
/// * `input_trace` - a trace of the input indexed Z-set.
///
/// # Type arguments
///
/// * `Z` - input batch type in the `delta` stream.
/// * `IT` - input trace type.
/// * `A` - aggregator to apply to each input group.
/// * `Clk` - clock that keeps track of the current logical time.
///
/// # Design
///
/// There are two possible strategies for incremental implementation of
/// non-linear operators like `distinct` and `aggregate`: (1) compute
/// the value to retract for each updated key using the input trace,
/// (2) compute new values of updated keys only and extract the old values
/// to retract from the trace of the output collection. We adopt the
/// second approach here, which avoids re-computation by using more
/// memory.  This is based on two considerations.  First, computing
/// an aggregate can be a relatively expensive operation, as it
/// typically requires scanning all values associated with each key.
/// Second, the aggregated output trace will typically use less memory
/// than the input trace, as it summarizes all values per key in one
/// aggregate value.  Of course these are not always true, and we may
/// want to one day build an alternative implementation using the
/// other approach.
struct AggregateIncremental<Z, IT, A, Clk>
where
    IT: BatchReader,
{
    clock: Clk,
    aggregator: A,
    // The last input batch was empty - used in fixedpoint computation.
    empty_input: bool,
    // The last output batch was empty - used in fixedpoint computation.
    empty_output: bool,
    // Keys that may need updating at future times.
    keys_of_interest: BTreeMap<IT::Time, BTreeSet<IT::Key>>,
    // Buffer used in computing per-key outputs.
    // Keep it here to reuse allocation across multiple operations.
    _type: PhantomData<(Z, IT)>,
}

impl<Z, IT, A, Clk> AggregateIncremental<Z, IT, A, Clk>
where
    Clk: WithClock<Time = IT::Time>,
    Z: IndexedZSet,
    IT: BatchReader<Key = Z::Key, Val = Z::Val, R = Z::R>,
    A: Aggregator<Z::Val, IT::Time, Z::R>,
{
    pub fn new(aggregator: A, clock: Clk) -> Self {
        Self {
            clock,
            aggregator,
            empty_input: false,
            empty_output: false,
            keys_of_interest: BTreeMap::new(),
            _type: PhantomData,
        }
    }

    /// Compute output of the operator for `key`.
    ///
    /// # Arguments
    ///
    /// * `input_cursor` - cursor over the input trace that contains all updates
    ///   to the indexed Z-set that we are aggregating, up to and including the
    ///   current timestamp.
    /// * `builder` - builder that accumulates output tuples for the current
    ///   evalution of the operator.
    ///
    /// # Computing output
    ///
    /// We use the `input_cursor` to compute the current value of the
    /// aggregate as:
    ///
    /// ```text
    /// (1) agg = aggregate({(v, w) | (v, t, w) ∈ input_cursor[key], t <= time})
    /// ```
    ///
    /// # Updating `keys_of_interest`
    ///
    /// For each `(v, t, w)` tuple in `input_cursor`, such that `t <= time`
    /// does not hold, the tuple can affect the value of the aggregate at
    /// time `time.join(t)`.  We compute the smallest (according to the
    /// global lexicographic ordering of timestamps that reflects the order in
    /// which DBSP processes timestamps) such `t` and insert `key` in
    /// `keys_of_interest` for that time.
    ///
    /// Note that this implementation may end up running `input_cursor` twice,
    /// once when computing the aggregate and once when updating
    /// `keys_of_interest`. This allows cleanly encapsulating the
    /// aggregation logic in the `Aggregator` trait.  The second iteration
    /// is only needed inside nested scopes and can in the future be
    /// optimized to terminate early.
    fn eval_key(
        &mut self,
        key: &Z::Key,
        input_cursor: &mut IT::Cursor<'_>,
        output: &mut Vec<(Z::Key, Option<A::Output>)>,
        time: &Clk::Time,
    ) {
        // println!(
        //     "{}: eval_key({key}) @ {:?}",
        //     Runtime::worker_index(),
        //     time
        // );

        // Lookup key in input.
        input_cursor.seek_key(key);

        // If found, compute `agg` using formula (1) above; otherwise the aggregate is
        // `0`.
        if input_cursor.key_valid() && input_cursor.key() == key {
            // Apply aggregator to a `CursorGroup` that iterates over the nested
            // Z-set associated with `input_cursor.key()` at time `time`.
            if let Some(aggregate) = self
                .aggregator
                .aggregate_and_finalize(&mut CursorGroup::new(input_cursor, time.clone()))
            {
                output.push((key.clone(), Some(aggregate)));
            } else {
                output.push((key.clone(), None));
            }

            // Compute the closest future timestamp when we may need to reevaluate
            // this key (See 'Updating keys of interest' section above).
            //
            // Skip this relatively expensive computation when running in the root
            // scope using unit timestamps (`IT::Time = ()`).
            if TypeId::of::<IT::Time>() != TypeId::of::<()>() {
                input_cursor.rewind_vals();

                let mut time_of_interest = None;
                while input_cursor.val_valid() {
                    // TODO: More efficient lookup of the smallest timestamp exceeding
                    // `time`, without scanning everything.
                    time_of_interest =
                        input_cursor.fold_times(time_of_interest, |time_of_interest, ts, _| {
                            if !ts.less_equal(time) {
                                match time_of_interest {
                                    None => Some(time.join(ts)),
                                    Some(time_of_interest) => {
                                        Some(min(time_of_interest, time.join(ts)))
                                    }
                                }
                            } else {
                                time_of_interest
                            }
                        });
                    input_cursor.step_val();
                }

                if let Some(t) = time_of_interest {
                    self.keys_of_interest
                        .entry(t)
                        .or_insert_with(BTreeSet::new)
                        .insert(key.clone());
                }
            }
        } else {
            output.push((key.clone(), None));
        }
    }
}

impl<Z, IT, A, Clk> Operator for AggregateIncremental<Z, IT, A, Clk>
where
    Clk: WithClock<Time = IT::Time> + 'static,
    Z: 'static,
    IT: BatchReader + 'static,
    A: 'static,
{
    fn name(&self) -> Cow<'static, str> {
        Cow::from("AggregateIncremental")
    }

    fn clock_start(&mut self, scope: Scope) {
        if scope == 0 {
            self.empty_input = false;
            self.empty_output = false;
        }
    }

    fn clock_end(&mut self, scope: Scope) {
        debug_assert!(self.keys_of_interest.keys().all(|ts| {
            if ts.less_equal(&self.clock.time().epoch_end(scope)) {
                println!(
                    "ts: {ts:?}, epoch_end: {:?}",
                    self.clock.time().epoch_end(scope)
                );
            }
            !ts.less_equal(&self.clock.time().epoch_end(scope))
        }));
    }

    fn fixedpoint(&self, scope: Scope) -> bool {
        let epoch_end = self.clock.time().epoch_end(scope);

        self.empty_input
            && self.empty_output
            && self
                .keys_of_interest
                .keys()
                .all(|ts| !ts.less_equal(&epoch_end))
    }
}

impl<Z, IT, A, Clk> BinaryOperator<Z, IT, Vec<(Z::Key, Option<A::Output>)>>
    for AggregateIncremental<Z, IT, A, Clk>
where
    Clk: WithClock<Time = IT::Time> + 'static,
    Z: IndexedZSet,
    IT: BatchReader<Key = Z::Key, Val = Z::Val, R = Z::R> + Clone,
    A: Aggregator<Z::Val, IT::Time, Z::R> + 'static,
{
    fn eval(&mut self, delta: &Z, input_trace: &IT) -> Vec<(Z::Key, Option<A::Output>)> {
        // println!(
        //     "{}: AggregateIncremental::eval @{:?}\ndelta:{delta}",
        //     Runtime::worker_index(),
        //     self.time
        // );
        self.empty_input = delta.is_empty();

        let mut result = Vec::with_capacity(delta.key_count());

        let mut delta_cursor = delta.cursor();
        let mut input_trace_cursor = input_trace.cursor();

        let time = self.clock.time();

        // Previously encountered keys that may affect output at the
        // current time.
        let keys_of_interest = self.keys_of_interest.remove(&time).unwrap_or_default();

        let mut keys_of_interest = keys_of_interest.iter();

        let mut key_of_interest = keys_of_interest.next();

        // Iterate over all keys in `delta_cursor` and `keys_of_interest`.
        while delta_cursor.key_valid() && key_of_interest.is_some() {
            let key_of_interest_ref = key_of_interest.unwrap();

            match delta_cursor.key().cmp(key_of_interest_ref) {
                // Key only appears in `delta`.
                Ordering::Less => {
                    self.eval_key(
                        delta_cursor.key(),
                        &mut input_trace_cursor,
                        &mut result,
                        &time,
                    );
                    delta_cursor.step_key();
                }
                // Key only appears in `keys_of_interest`.
                Ordering::Greater => {
                    self.eval_key(
                        key_of_interest_ref,
                        &mut input_trace_cursor,
                        &mut result,
                        &time,
                    );
                    key_of_interest = keys_of_interest.next();
                }
                // Key appears in both `delta` and `keys_of_interest`.
                Ordering::Equal => {
                    self.eval_key(
                        delta_cursor.key(),
                        &mut input_trace_cursor,
                        &mut result,
                        &time,
                    );
                    delta_cursor.step_key();
                    key_of_interest = keys_of_interest.next();
                }
            }
        }

        while delta_cursor.key_valid() {
            self.eval_key(
                delta_cursor.key(),
                &mut input_trace_cursor,
                &mut result,
                &time,
            );
            delta_cursor.step_key();
        }

        while key_of_interest.is_some() {
            self.eval_key(
                key_of_interest.unwrap(),
                &mut input_trace_cursor,
                &mut result,
                &time,
            );
            key_of_interest = keys_of_interest.next();
        }

        self.empty_output = result.is_empty();
        result
    }
}

#[cfg(test)]
mod test {
    use anyhow::Result as AnyResult;

    use std::{
        cell::RefCell,
        rc::Rc,
        sync::{Arc, Mutex},
    };

    use crate::{
        algebra::DefaultSemigroup,
        indexed_zset,
        operator::GeneratorNested,
        operator::{Fold, Min},
        trace::{cursor::Cursor, Batch, BatchReader},
        zset, Circuit, OrdIndexedZSet, OrdZSet, RootCircuit, Runtime, Stream,
    };

    type TestZSet = OrdZSet<(usize, isize), isize>;

    fn aggregate_test_circuit(
        circuit: &mut RootCircuit,
        inputs: Vec<Vec<TestZSet>>,
    ) -> AnyResult<()> {
        let mut inputs = inputs.into_iter();

        circuit
            .iterate(|child| {
                let counter = Rc::new(RefCell::new(0));
                let counter_clone = counter.clone();

                let input = child
                    .add_source(GeneratorNested::new(Box::new(move || {
                        *counter_clone.borrow_mut() = 0;
                        if Runtime::worker_index() == 0 {
                            let mut deltas = inputs.next().unwrap_or_default().into_iter();
                            Box::new(move || deltas.next().unwrap_or_else(|| zset! {}))
                        } else {
                            Box::new(|| zset! {})
                        }
                    })))
                    .index();

                // Weighted sum aggregate.
                let sum = <Fold<_, DefaultSemigroup<_>, _, _>>::new(
                    0,
                    |acc: &mut isize, v: &isize, w: isize| *acc += *v * w,
                );

                // Weighted sum aggregate that returns only the weighted sum
                // value and is therefore linear.
                let sum_linear = |_key: &usize, val: &isize| -> isize { *val };

                let sum_inc = input.aggregate(sum.clone()).gather(0);
                let sum_inc_linear: Stream<_, OrdIndexedZSet<usize, isize, isize>> =
                    input.aggregate_linear(sum_linear).gather(0);
                let sum_noninc = input
                    .integrate_nested()
                    .integrate()
                    .stream_aggregate(sum)
                    .differentiate()
                    .differentiate_nested()
                    .gather(0);
                let sum_noninc_linear = input
                    .integrate_nested()
                    .integrate()
                    .stream_aggregate_linear(sum_linear)
                    .differentiate()
                    .differentiate_nested()
                    .gather(0);

                // Compare outputs of all four implementations.
                sum_inc
                    .apply2(
                        &sum_noninc,
                        |d1: &OrdIndexedZSet<usize, isize, isize>,
                         d2: &OrdIndexedZSet<usize, isize, isize>| {
                            (d1.clone(), d2.clone())
                        },
                    )
                    .inspect(|(d1, d2)| {
                        //println!("{}: incremental: {:?}", Runtime::worker_index(), d1);
                        //println!("{}: non-incremental: {:?}", Runtime::worker_index(), d2);
                        assert_eq!(d1, d2);
                    });

                sum_inc.apply2(
                    &sum_inc_linear,
                    |d1: &OrdIndexedZSet<usize, isize, isize>,
                     d2: &OrdIndexedZSet<usize, isize, isize>| {
                        // println!("{}: incremental: {:?}", Runtime::worker_index(), d1);
                        // println!("{}: linear: {:?}", Runtime::worker_index(), d2);

                        // Compare d1 and d2 modulo 0 values (linear aggregation removes them
                        // from the collection).
                        let mut cursor1 = d1.cursor();
                        let mut cursor2 = d2.cursor();

                        while cursor1.key_valid() {
                            while cursor1.val_valid() {
                                if *cursor1.val() != 0 {
                                    assert!(cursor2.key_valid());
                                    assert_eq!(cursor2.key(), cursor1.key());
                                    assert!(cursor2.val_valid());
                                    assert_eq!(cursor2.val(), cursor1.val());
                                    assert_eq!(cursor2.weight(), cursor1.weight());
                                    cursor2.step_val();
                                }

                                cursor1.step_val();
                            }

                            if cursor2.key_valid() && cursor2.key() == cursor1.key() {
                                cursor2.step_key();
                            }

                            cursor1.step_key();
                        }
                        assert!(!cursor2.key_valid());
                    },
                );

                sum_inc_linear
                    .apply2(
                        &sum_noninc_linear,
                        |d1: &OrdIndexedZSet<usize, isize, isize>,
                         d2: &OrdIndexedZSet<usize, isize, isize>| {
                            (d1.clone(), d2.clone())
                        },
                    )
                    .inspect(|(d1, d2)| {
                        //println!("{}: incremental: {:?}", Runtime::worker_index(), d1);
                        //println!("{}: non-incremental: {:?}", Runtime::worker_index(), d2);
                        assert_eq!(d1, d2);
                    });

                let min_inc = input.aggregate(Min).gather(0);
                let min_noninc = input
                    .integrate_nested()
                    .integrate()
                    .stream_aggregate(Min)
                    .differentiate()
                    .differentiate_nested()
                    .gather(0);

                min_inc
                    .apply2(
                        &min_noninc,
                        |d1: &OrdIndexedZSet<usize, isize, isize>,
                         d2: &OrdIndexedZSet<usize, isize, isize>| {
                            (d1.clone(), d2.clone())
                        },
                    )
                    .inspect(|(d1, d2)| {
                        assert_eq!(d1, d2);
                    });

                Ok((
                    move || {
                        *counter.borrow_mut() += 1;
                        Ok(*counter.borrow() == MAX_ITERATIONS)
                    },
                    (),
                ))
            })
            .unwrap();
        Ok(())
    }

    use proptest::{collection, prelude::*};

    const MAX_ROUNDS: usize = 15;
    const MAX_ITERATIONS: usize = 15;
    const NUM_KEYS: usize = 5;
    const MAX_VAL: isize = 3;
    const MAX_TUPLES: usize = 10;

    fn test_zset() -> impl Strategy<Value = TestZSet> {
        collection::vec(
            ((0..NUM_KEYS, -MAX_VAL..MAX_VAL), -1..=1isize),
            0..MAX_TUPLES,
        )
        .prop_map(|tuples| OrdZSet::from_tuples((), tuples))
    }
    fn test_input() -> impl Strategy<Value = Vec<Vec<TestZSet>>> {
        collection::vec(
            collection::vec(test_zset(), 0..MAX_ITERATIONS),
            0..MAX_ROUNDS,
        )
    }

    proptest! {
        #[test]
        #[cfg_attr(feature = "persistence", ignore = "takes a long time?")]
        fn proptest_aggregate_test_st(inputs in test_input()) {
            let iterations = inputs.len();
            let circuit = RootCircuit::build(|circuit| aggregate_test_circuit(circuit, inputs)).unwrap().0;

            for _ in 0..iterations {
                circuit.step().unwrap();
            }
        }

        #[test]
        #[cfg_attr(feature = "persistence", ignore = "takes a long time?")]
        fn proptest_aggregate_test_mt(inputs in test_input(), workers in (2..=16usize)) {
            let iterations = inputs.len();
            let mut circuit = Runtime::init_circuit(workers, |circuit| aggregate_test_circuit(circuit, inputs)).unwrap().0;

            for _ in 0..iterations {
                circuit.step().unwrap();
            }

            circuit.kill().unwrap();
        }
    }

    fn count_test(workers: usize) {
        let count_weighted_output: Arc<Mutex<OrdIndexedZSet<usize, isize, isize>>> =
            Arc::new(Mutex::new(indexed_zset! {}));
        let sum_weighted_output: Arc<Mutex<OrdIndexedZSet<usize, isize, isize>>> =
            Arc::new(Mutex::new(indexed_zset! {}));
        let count_distinct_output: Arc<Mutex<OrdIndexedZSet<usize, usize, isize>>> =
            Arc::new(Mutex::new(indexed_zset! {}));
        let sum_distinct_output: Arc<Mutex<OrdIndexedZSet<usize, usize, isize>>> =
            Arc::new(Mutex::new(indexed_zset! {}));

        let count_weighted_output_clone = count_weighted_output.clone();
        let count_distinct_output_clone = count_distinct_output.clone();
        let sum_weighted_output_clone = sum_weighted_output.clone();
        let sum_distinct_output_clone = sum_distinct_output.clone();

        let (mut dbsp, input_handle) = Runtime::init_circuit(workers, move |circuit| {
            let (input_stream, input_handle) = circuit.add_input_indexed_zset();
            input_stream
                .weighted_count()
                .gather(0)
                .inspect(move |batch| {
                    if Runtime::worker_index() == 0 {
                        *count_weighted_output.lock().unwrap() = batch.clone();
                    }
                });

            input_stream
                .aggregate_linear(|_key, value: &usize| *value as isize)
                .gather(0)
                .inspect(move |batch| {
                    if Runtime::worker_index() == 0 {
                        *sum_weighted_output.lock().unwrap() = batch.clone();
                    }
                });

            input_stream
                .aggregate(<Fold<_, DefaultSemigroup<_>, _, _>>::new(
                    0,
                    |sum: &mut usize, _v: &usize, _w| *sum += 1,
                ))
                .gather(0)
                .inspect(move |batch| {
                    if Runtime::worker_index() == 0 {
                        *count_distinct_output.lock().unwrap() = batch.clone();
                    }
                });

            input_stream
                .aggregate(<Fold<_, DefaultSemigroup<_>, _, _>>::new(
                    0,
                    |sum: &mut usize, v: &usize, _w| *sum += v,
                ))
                .gather(0)
                .inspect(move |batch| {
                    if Runtime::worker_index() == 0 {
                        *sum_distinct_output.lock().unwrap() = batch.clone();
                    }
                });
            Ok(input_handle)
        })
        .unwrap();

        input_handle.append(&mut vec![(1, (1, 1)), (1, (2, 2))]);
        dbsp.step().unwrap();
        assert_eq!(
            &*count_distinct_output_clone.lock().unwrap(),
            &indexed_zset! {1 => {2 => 1}}
        );
        assert_eq!(
            &*sum_distinct_output_clone.lock().unwrap(),
            &indexed_zset! {1 => {3 => 1}}
        );
        assert_eq!(
            &*count_weighted_output_clone.lock().unwrap(),
            &indexed_zset! {1 => {3 => 1}}
        );
        assert_eq!(
            &*sum_weighted_output_clone.lock().unwrap(),
            &indexed_zset! {1 => {5 => 1}}
        );

        input_handle.append(&mut vec![(2, (2, 1)), (2, (4, 1)), (1, (2, -1))]);
        dbsp.step().unwrap();
        assert_eq!(
            &*count_distinct_output_clone.lock().unwrap(),
            &indexed_zset! {2 => {2 => 1}}
        );
        assert_eq!(
            &*sum_distinct_output_clone.lock().unwrap(),
            &indexed_zset! {2 => {6 => 1}}
        );
        assert_eq!(
            &*count_weighted_output_clone.lock().unwrap(),
            &indexed_zset! {1 => {3 => -1, 2 => 1}, 2 => {2 => 1}}
        );
        assert_eq!(
            &*sum_weighted_output_clone.lock().unwrap(),
            &indexed_zset! {2 => {6 => 1}, 1 => {5 => -1, 3 => 1}}
        );

        input_handle.append(&mut vec![(1, (3, 1)), (1, (2, -1))]);
        dbsp.step().unwrap();
        assert_eq!(
            &*count_distinct_output_clone.lock().unwrap(),
            &indexed_zset! {}
        );
        assert_eq!(
            &*sum_distinct_output_clone.lock().unwrap(),
            &indexed_zset! {1 => {3 => -1, 4 => 1}}
        );
        assert_eq!(
            &*count_weighted_output_clone.lock().unwrap(),
            &indexed_zset! {}
        );
        assert_eq!(
            &*sum_weighted_output_clone.lock().unwrap(),
            &indexed_zset! {1 => {3 => -1, 4 => 1}}
        );

        dbsp.kill().unwrap();
    }

    #[test]
    fn count_test1() {
        count_test(1);
    }

    #[test]
    fn count_test4() {
        count_test(4);
    }
}
