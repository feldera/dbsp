//! Distinct operator.

use crate::{
    algebra::{AddByRef, HasOne, HasZero, IndexedZSet, Lattice, PartialOrder, Present, ZRingValue},
    circuit::{
        metadata::{MetaItem, OperatorMeta},
        operator_traits::{BinaryOperator, Operator, UnaryOperator},
        Circuit, GlobalNodeId, Scope, Stream, WithClock,
    },
    circuit_cache_key,
    trace::{ord::OrdValSpine, Batch, BatchReader, Builder, Cursor as TraceCursor, Trace},
    DBTimestamp, OrdIndexedZSet, Timestamp,
};
use size_of::SizeOf;
use std::{
    borrow::Cow,
    cmp::{min, Ordering},
    collections::BTreeMap,
    marker::PhantomData,
    ops::Neg,
};

circuit_cache_key!(DistinctId<C, D>(GlobalNodeId => Stream<C, D>));
circuit_cache_key!(DistinctIncrementalId<C, D>(GlobalNodeId => Stream<C, D>));

impl<C, Z> Stream<C, Z>
where
    C: Circuit,
{
    /// Reduces input batches to one occurrence of each element.
    ///
    /// For each input batch `B`, the operator produces an output batch
    /// that contains at most one occurrence of each tuple in `B`.
    /// Specifically, for each input tuple `(key, value, weight)` with
    /// `weight > 0` the operator produces an output tuple `(key, value, 1)`.
    /// Tuples with `weight <= 0` are dropped.
    ///
    /// Intuitively, the operator converts the input multiset into a set
    /// by eliminating duplicates.
    pub fn stream_distinct(&self) -> Stream<C, Z>
    where
        Z: IndexedZSet + Send,
        Z::R: ZRingValue,
    {
        self.circuit()
            .cache_get_or_insert_with(DistinctId::new(self.origin_node_id().clone()), || {
                self.circuit()
                    .add_unary_operator(Distinct::new(), &self.shard())
                    .mark_sharded()
            })
            .clone()
    }

    /// Incrementally deduplicate input stream.
    ///
    /// This is an incremental version of the
    /// [`stream_distinct`](`Self::stream_distinct`) operator.
    /// Given a stream of changes to relation `A`, it computes a stream of
    /// changes to relation `A'`, that for each `(key, value, weight)` tuple
    /// in `A` with `weight > 0`, contains a tuple `(key, value, 1)`.
    ///
    /// Intuitively, the operator converts the input multiset into a set
    /// by eliminating duplicates.
    pub fn distinct(&self) -> Stream<C, Z>
    where
        Z: IndexedZSet + Send,
        Z::R: ZRingValue,
        <C as WithClock>::Time: DBTimestamp,
    {
        let circuit = self.circuit();
        circuit
            .cache_get_or_insert_with(
                DistinctIncrementalId::new(self.origin_node_id().clone()),
                || {
                    let stream = self.shard();

                    circuit.region("distinct", || {
                        if circuit.root_scope() == 0 {
                            // Use an implementation optimized to work in the root scope.
                            circuit.add_binary_operator(
                                DistinctIncrementalTotal::new(),
                                &stream,
                                &stream.integrate_trace().delay_trace(),
                            )
                        } else {
                            // ```
                            //          ┌────────────────────────────────────┐
                            //          │                                    │
                            //          │                                    ▼
                            //  stream  │     ┌─────┐  stream.trace()  ┌───────────────────┐
                            // ─────────┴─────┤trace├─────────────────►│DistinctIncremental├─────►
                            //                └─────┘                  └───────────────────┘
                            // ```
                            circuit.add_binary_operator(
                                DistinctIncremental::new(circuit.clone()),
                                &stream,
                                // TODO use OrdIndexedZSetSpine if `Z::Val = ()`
                                &stream.trace::<OrdValSpine<Z::Key, Z::Val, <C as WithClock>::Time, Z::R>>(),
                            )
                        }
                        .mark_sharded()
                    })
                },
            )
            .clone()
    }
}

/// `Distinct` operator changes all weights in the support of a Z-set to 1.
pub struct Distinct<Z> {
    _type: PhantomData<Z>,
}

impl<Z> Distinct<Z> {
    pub fn new() -> Self {
        Self { _type: PhantomData }
    }
}

impl<Z> Default for Distinct<Z> {
    fn default() -> Self {
        Self::new()
    }
}

impl<Z> Operator for Distinct<Z>
where
    Z: 'static,
{
    fn name(&self) -> Cow<'static, str> {
        Cow::from("Distinct")
    }

    fn fixedpoint(&self, _scope: Scope) -> bool {
        true
    }
}

impl<Z> UnaryOperator<Z, Z> for Distinct<Z>
where
    Z: IndexedZSet,
    Z::R: ZRingValue,
{
    fn eval(&mut self, input: &Z) -> Z {
        input.distinct()
    }

    fn eval_owned(&mut self, input: Z) -> Z {
        input.distinct_owned()
    }
}

/// Incremental version of the distinct operator that only works in the
/// top-level scope (i.e., for totally ordered timestamps).
///
/// Takes a stream `a` of changes to relation `A` and a stream with delayed
/// value of `A`: `z^-1(A) = a.integrate().delay()` and computes
/// `distinct(A) - distinct(z^-1(A))` incrementally, by only considering
/// values in the support of `a`.
struct DistinctIncrementalTotal<Z, I> {
    _type: PhantomData<(Z, I)>,
}

impl<Z, I> DistinctIncrementalTotal<Z, I> {
    pub fn new() -> Self {
        Self { _type: PhantomData }
    }
}

impl<Z, I> Default for DistinctIncrementalTotal<Z, I> {
    fn default() -> Self {
        Self::new()
    }
}

impl<Z, I> Operator for DistinctIncrementalTotal<Z, I>
where
    Z: 'static,
    I: 'static,
{
    fn name(&self) -> Cow<'static, str> {
        Cow::from("DistinctIncrementalTotal")
    }

    fn fixedpoint(&self, _scope: Scope) -> bool {
        true
    }
}

impl<Z, I> BinaryOperator<Z, I, Z> for DistinctIncrementalTotal<Z, I>
where
    Z: IndexedZSet,
    Z::R: ZRingValue,
    I: BatchReader<Key = Z::Key, Val = Z::Val, Time = (), R = Z::R>,
{
    fn eval(&mut self, delta: &Z, delayed_integral: &I) -> Z {
        let mut builder = Z::Builder::with_capacity((), delta.len());
        let mut delta_cursor = delta.cursor();
        let mut integral_cursor = delayed_integral.cursor();

        while delta_cursor.key_valid() {
            integral_cursor.seek_key(delta_cursor.key());

            if integral_cursor.key_valid() && integral_cursor.key() == delta_cursor.key() {
                while delta_cursor.val_valid() {
                    let w = delta_cursor.weight();
                    let v = delta_cursor.val();

                    integral_cursor.seek_val(v);
                    let old_weight = if integral_cursor.val_valid() && integral_cursor.val() == v {
                        integral_cursor.weight()
                    } else {
                        HasZero::zero()
                    };

                    let new_weight = old_weight.add_by_ref(&w);

                    if old_weight.le0() {
                        // Weight changes from non-positive to positive.
                        if new_weight.ge0() && !new_weight.is_zero() {
                            builder.push((
                                Z::item_from(delta_cursor.key().clone(), v.clone()),
                                HasOne::one(),
                            ));
                        }
                    } else if new_weight.le0() {
                        // Weight changes from positive to non-positive.
                        builder.push((
                            Z::item_from(delta_cursor.key().clone(), v.clone()),
                            Z::R::one().neg(),
                        ));
                    }

                    delta_cursor.step_val();
                }
            } else {
                while delta_cursor.val_valid() {
                    let new_weight = delta_cursor.weight();

                    if new_weight.ge0() && !new_weight.is_zero() {
                        builder.push((
                            Z::item_from(delta_cursor.key().clone(), delta_cursor.val().clone()),
                            HasOne::one(),
                        ));
                    }
                    delta_cursor.step_val();
                }
            };

            delta_cursor.step_key();
        }

        builder.done()
    }

    // TODO: owned implementation.
    fn eval_owned_and_ref(&mut self, delta: Z, delayed_integral: &I) -> Z {
        self.eval(&delta, delayed_integral)
    }

    fn eval_owned(&mut self, delta: Z, delayed_integral: I) -> Z {
        self.eval_owned_and_ref(delta, &delayed_integral)
    }
}

/// Track key/value pairs that must be recomputed at
/// future times.  Represent them as `((key, value), Present)`
/// tuples so we can use the `Batcher` API to compile them
/// into a batch.
type KeysOfInterest<TS, K, V> = BTreeMap<TS, Vec<((K, V), Present)>>;

#[derive(SizeOf)]
struct DistinctIncremental<Z, T, Clk>
where
    Z: IndexedZSet,
    T: BatchReader<Key = Z::Key, Val = Z::Val, R = Z::R>,
{
    #[size_of(skip)]
    clock: Clk,
    // Keys that may need updating at future times.
    keys_of_interest: KeysOfInterest<T::Time, Z::Key, Z::Val>,
    // True if the operator received empty input during the last clock
    // tick.
    empty_input: bool,
    // True if the operator produced empty output at the last clock tick.
    empty_output: bool,
    // Used in computing partial derivatives
    // (we keep it here to reuse allocations across `eval_keyval` calls).
    distinct_vals: Vec<(Option<T::Time>, Z::R)>,
    _type: PhantomData<(Z, T)>,
}

impl<Z, T, Clk> DistinctIncremental<Z, T, Clk>
where
    Z: IndexedZSet,
    T: BatchReader<Key = Z::Key, Val = Z::Val, R = Z::R>,
    Z::R: ZRingValue,
    Clk: WithClock<Time = T::Time>,
{
    fn new(clock: Clk) -> Self {
        let depth = clock.nesting_depth();

        Self {
            clock,
            keys_of_interest: BTreeMap::new(),
            empty_input: false,
            empty_output: false,
            distinct_vals: vec![(None, HasZero::zero()); 2 << depth],
            _type: PhantomData,
        }
    }

    /// Compute the output of the operator for a single key/value pair.
    ///
    /// # Theory
    ///
    /// According to the definition of a lifted incremental operator, we are
    /// computing a partial derivative:
    ///
    /// ```text
    ///     ∂^n                      ∂^(n-1)                      ∂^(n-1)
    ///  ------------f(t1,..,tn) = ------------f(t1,..,tn)  -  ------------f(t1-1,..,tn).
    ///  ∂_t1 .. ∂_tn              ∂_t2 .. ∂_tn                ∂_t2 .. ∂_tn
    /// ```
    ///
    /// where `n` is the nesting level of the circuit, i.e., 1 for the top-level
    /// circuit, 2 for its subcircuit, etc., `ti` is the i'th component of
    /// the n-dimensional timestamp, and `f(t1..tn)` is the value of the
    /// function we want to compute (in this case `distinct`) at time
    /// `t1..tn`.
    ///
    /// For example, for a twice nested circuit, we get:
    ///
    /// ```text
    ///      ∂^3                        ∂^2                         ∂^2
    ///  --------------f(t1,t2,t3) = -----------f(t1,t2,t3)  -  -----------f(t1-1,t2,t3) =
    ///  ∂_t1 ∂_t2 ∂_t3               ∂_t2 ∂_t3                  ∂_t2 ∂_t3
    ///
    ///   ┌─                                     ─┐   ┌─                                          ─┐
    ///   │   ∂                   ∂               │   │   ∂                    ∂                   │
    ///   │ -----f(t1,t2,t3)  - -----f(t1,t2-1,t3)│ - │ -----f(t1-1,t2,t3)  - -----f(t1-1,t2-1,t3) │ =
    /// = │ ∂_t3                 ∂_t3             │   │ ∂_t3                  ∂_t3                 │
    ///   └─                                     ─┘   └─                                          ─┘
    ///
    /// = ((f(t1,t2,t3) - f(t1,t2,t3-1)) - (f(t1,t2-1,t2) - f(t1,t2-1,t3-1))) -
    ///   ((f(t1-1,t2,t3) - f(t1-1,t2,t3-1)) - (f(t1-1,t2-1,t2) - f(t1-1,t2-1,t3-1))).
    /// ```
    ///
    /// In general, in order to compute the partial derivative, we need to know
    /// the values of `f` for all times within the n-dimensional cube with
    /// edge 1 of the current time `t1..tn` (i.e., all `2^n` possible
    /// combinations of `ti`'s and `ti-1`'s).
    ///
    /// Computing the value of `f` at time `t`, in turn, requires first
    /// computing the sum of all updates with time `t' < t` in the input
    /// stream:
    ///
    /// ```text
    ///              __
    ///              \
    /// f(t) =dist(  / stream[t'][k][v].weight )
    ///              --
    ///              t'<=t
    /// ```
    ///
    /// where `stream[t'][k][v].weight` is pseudocode for selecting the weight
    /// of the update at time `t'` for key `k` and value `v`, and
    ///
    /// ```text
    ///            ┌
    ///            │  1, if w > 0
    /// dist(w) = <
    ///            │  0, otherwise
    ///            └
    /// ```
    ///
    /// We compute `f(t)` for all `2^n` times `t` of interest simultaneously in
    /// a single run of the cursor by maintaining an array of `2^n`
    /// accumulators and updating a subset of them at each step.
    ///
    /// # `keys_of_interest` map.
    ///
    /// The `distinct` operator does not have nice properties like linearity
    /// that help with incremental evaluation: computing an update for each
    /// key/value pair requires inspecting the entire history of updates for
    /// this pair.  Fortunately, we do not need to look at all key/value pairs
    /// in the trace.  Specifically, a key/value pair `(k, v)` can only
    /// appear in the output of the operator at time `t` if it satisfies one
    /// of the following conditions:
    /// * `(k,v)` occurs in the current input `delta`
    /// * there exist `t1<t` and `t2<t` such that `t = t1 /\ t2` and `(k,v)`
    ///   occurs in the inputs of the operator at times `t1` and `t2`.
    ///
    /// To efficiently compute tuples that satisfy the second condition, we use
    /// the `keys_of_interest` map.  For each `(k,v)` observed at time `t1`
    /// we lookup the smallest time `t'` such that `t' = t1 /\ t2` for some
    /// `t2` such that `not (t2 <= t1)` at which we've encountered `(k,v)`
    /// and record `(k,v)` in `keys_of_interest[t']`.  When evaluating the
    /// operator at time `t'` we simply scan all tuples in
    /// `keys_of_interest[t2]` for candidates.
    fn eval_keyval(
        &mut self,
        time: &Clk::Time,
        key: &Z::Key,
        val: &Z::Val,
        trace_cursor: &mut T::Cursor<'_>,
        output: &mut Z::Builder,
    ) {
        if trace_cursor.get_key() == Some(key) {
            trace_cursor.seek_val(val);

            if trace_cursor.get_val() == Some(val) {
                // The nearest future timestamp when we need to update this
                // key/value pair.
                let mut time_of_interest = None;

                // Reset all counters to 0.
                self.clear_distinct_vals();

                trace_cursor.map_times(|trace_ts, weight| {
                    // Update weights in `distinct_vals`.
                    for (ts, total_weight) in self.distinct_vals.iter_mut() {
                        if let Some(ts) = ts {
                            if trace_ts.less_equal(ts) {
                                *total_weight += weight.clone();
                            }
                        }
                    }
                    // Timestamp in the future - update `time_of_interest`.
                    if !trace_ts.less_equal(time) {
                        time_of_interest = match time_of_interest.take() {
                            None => Some(time.join(trace_ts)),
                            Some(time_of_interest) => {
                                Some(min(time_of_interest, time.join(trace_ts)))
                            }
                        }
                    }
                });

                // Compute `dist` for each entry in `distinct_vals`.
                for (_time, weight) in self.distinct_vals.iter_mut() {
                    if weight.le0() {
                        *weight = HasZero::zero();
                    } else {
                        *weight = HasOne::one();
                    }
                }

                // We have computed `f` at all the relevant point in times; we can now
                // compute the partial derivative.
                let output_weight = Self::partial_derivative(&self.distinct_vals);
                if !output_weight.is_zero() {
                    output.push((Z::item_from(key.clone(), val.clone()), output_weight));
                }

                if let Some(t) = time_of_interest {
                    self.keys_of_interest
                        .entry(t)
                        .or_insert_with(Vec::new)
                        .push(((key.clone(), val.clone()), Present));
                }
            }
        }
    }

    fn init_distinct_vals(vals: &mut [(Option<T::Time>, Z::R)], ts: Option<T::Time>) {
        if vals.len() == 1 {
            vals[0] = (ts, HasZero::zero());
        } else {
            let half_len = vals.len() >> 1;
            Self::init_distinct_vals(
                &mut vals[0..half_len],
                ts.as_ref()
                    .and_then(|ts| ts.checked_recede(half_len.ilog2() as Scope)),
            );
            Self::init_distinct_vals(&mut vals[half_len..], ts);
        }
    }

    fn clear_distinct_vals(&mut self) {
        for (_time, val) in self.distinct_vals.iter_mut() {
            *val = HasZero::zero();
        }
    }

    /// Compute partial derivative.
    fn partial_derivative(vals: &[(Option<T::Time>, Z::R)]) -> Z::R {
        // Split vals in two halves.  Compute `partial_derivative` recursively
        // for each half, return the difference.
        if vals.len() == 1 {
            vals[0].1.clone()
        } else {
            Self::partial_derivative(&vals[vals.len() >> 1..])
                + Self::partial_derivative(&vals[0..vals.len() >> 1]).neg()
        }
    }
}

impl<Z, T, Clk> Operator for DistinctIncremental<Z, T, Clk>
where
    Z: IndexedZSet,
    T: BatchReader<Key = Z::Key, Val = Z::Val, R = Z::R>,
    Clk: WithClock<Time = T::Time> + 'static,
{
    fn name(&self) -> Cow<'static, str> {
        Cow::Borrowed("DistinctIncremental")
    }

    fn metadata(&self, meta: &mut OperatorMeta) {
        let size: usize = self.keys_of_interest.values().map(Vec::len).sum();
        let bytes = self.size_of();

        meta.extend(metadata! {
            "total updates" => MetaItem::bytes(size),
            "used bytes" => MetaItem::bytes(bytes.used_bytes()),
            "allocations" => bytes.distinct_allocations(),
            "shared bytes" => MetaItem::bytes(bytes.shared_bytes()),
        });
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
                eprintln!(
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

impl<Z, T, Clk> BinaryOperator<Z, T, Z> for DistinctIncremental<Z, T, Clk>
where
    Z: IndexedZSet,
    Z::R: ZRingValue,
    T: Trace<Key = Z::Key, Val = Z::Val, R = Z::R>,
    Clk: WithClock<Time = T::Time> + 'static,
{
    // TODO: add eval_owned, so we can use keys and values from `delta` without
    // cloning.
    fn eval(&mut self, delta: &Z, trace: &T) -> Z {
        let time = self.clock.time();

        Self::init_distinct_vals(&mut self.distinct_vals, Some(time.clone()));
        self.empty_input = delta.is_empty();

        // We iterate over keys and values in order, so it is safe to use `Builder`.
        let mut result_builder = Z::Builder::with_capacity((), delta.len());

        let mut delta_cursor = delta.cursor();
        let mut trace_cursor = trace.cursor();

        // Previously encountered keys that may affect output at the
        // current time.
        let keys_of_interest = self.keys_of_interest.remove(&time).unwrap_or_default();

        let keys_of_interest =
            <OrdIndexedZSet<Z::Key, Z::Val, Present>>::from_tuples((), keys_of_interest);
        let mut keys_of_interest_cursor = keys_of_interest.cursor();

        // Iterate over all keys in `delta_cursor` and `keys_of_interest`.
        while delta_cursor.key_valid() && keys_of_interest_cursor.key_valid() {
            match delta_cursor.key().cmp(keys_of_interest_cursor.key()) {
                // Key only appears in `delta`.
                Ordering::Less => {
                    trace_cursor.seek_key(delta_cursor.key());

                    while delta_cursor.val_valid() {
                        self.eval_keyval(
                            &time,
                            delta_cursor.key(),
                            delta_cursor.val(),
                            &mut trace_cursor,
                            &mut result_builder,
                        );
                        delta_cursor.step_val();
                    }
                    delta_cursor.step_key();
                }
                // Key only appears in `keys_of_interest`.
                Ordering::Greater => {
                    trace_cursor.seek_key(keys_of_interest_cursor.key());

                    while keys_of_interest_cursor.val_valid() {
                        self.eval_keyval(
                            &time,
                            keys_of_interest_cursor.key(),
                            keys_of_interest_cursor.val(),
                            &mut trace_cursor,
                            &mut result_builder,
                        );
                        keys_of_interest_cursor.step_val();
                    }
                    keys_of_interest_cursor.step_key();
                }
                // Key appears in both `delta` and `keys_of_interest`:
                // Iterate over all values in both cursors.
                Ordering::Equal => {
                    trace_cursor.seek_key(keys_of_interest_cursor.key());

                    while delta_cursor.val_valid() && keys_of_interest_cursor.val_valid() {
                        match delta_cursor.val().cmp(keys_of_interest_cursor.val()) {
                            Ordering::Less => {
                                self.eval_keyval(
                                    &time,
                                    delta_cursor.key(),
                                    delta_cursor.val(),
                                    &mut trace_cursor,
                                    &mut result_builder,
                                );
                                delta_cursor.step_val();
                            }
                            Ordering::Greater => {
                                self.eval_keyval(
                                    &time,
                                    keys_of_interest_cursor.key(),
                                    keys_of_interest_cursor.val(),
                                    &mut trace_cursor,
                                    &mut result_builder,
                                );
                                keys_of_interest_cursor.step_val();
                            }
                            Ordering::Equal => {
                                self.eval_keyval(
                                    &time,
                                    delta_cursor.key(),
                                    delta_cursor.val(),
                                    &mut trace_cursor,
                                    &mut result_builder,
                                );
                                delta_cursor.step_val();
                                keys_of_interest_cursor.step_val();
                            }
                        }
                    }

                    // Iterate over remaining `delta_cursor` values.
                    while delta_cursor.val_valid() {
                        self.eval_keyval(
                            &time,
                            delta_cursor.key(),
                            delta_cursor.val(),
                            &mut trace_cursor,
                            &mut result_builder,
                        );
                        delta_cursor.step_val();
                    }

                    // Iterate over remaining `keys_of_interest` values.
                    while keys_of_interest_cursor.val_valid() {
                        self.eval_keyval(
                            &time,
                            keys_of_interest_cursor.key(),
                            keys_of_interest_cursor.val(),
                            &mut trace_cursor,
                            &mut result_builder,
                        );
                        keys_of_interest_cursor.step_val();
                    }

                    delta_cursor.step_key();
                    keys_of_interest_cursor.step_key();
                }
            }
        }

        // Iterate over remaining `delta_cursor` keys.
        while delta_cursor.key_valid() {
            trace_cursor.seek_key(delta_cursor.key());

            while delta_cursor.val_valid() {
                self.eval_keyval(
                    &time,
                    delta_cursor.key(),
                    delta_cursor.val(),
                    &mut trace_cursor,
                    &mut result_builder,
                );
                delta_cursor.step_val();
            }
            delta_cursor.step_key();
        }

        // Iterate over remaining `keys_of_interest_cursor` keys.
        while keys_of_interest_cursor.key_valid() {
            trace_cursor.seek_key(keys_of_interest_cursor.key());

            while keys_of_interest_cursor.val_valid() {
                self.eval_keyval(
                    &time,
                    keys_of_interest_cursor.key(),
                    keys_of_interest_cursor.val(),
                    &mut trace_cursor,
                    &mut result_builder,
                );
                keys_of_interest_cursor.step_val();
            }
            keys_of_interest_cursor.step_key();
        }

        let result = result_builder.done();
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
        indexed_zset,
        operator::{Generator, GeneratorNested},
        trace::Batch,
        zset, Circuit, OrdIndexedZSet, OrdZSet, OutputHandle, RootCircuit, Runtime,
    };

    fn do_distinct_inc_test_mt(workers: usize) {
        let hruntime = Runtime::run(workers, || {
            distinct_inc_test();
        });

        hruntime.join().unwrap();
    }

    #[test]
    fn distinct_inc_test_mt() {
        do_distinct_inc_test_mt(1);
        do_distinct_inc_test_mt(2);
        do_distinct_inc_test_mt(4);
        do_distinct_inc_test_mt(16);
    }

    #[test]
    fn distinct_inc_test() {
        let circuit = RootCircuit::build(move |circuit| {
            let mut inputs = vec![
                vec![zset! { 1 => 1, 2 => 1 }, zset! { 2 => -1, 3 => 2, 4 => 2 }],
                vec![zset! { 2 => 1, 3 => 1 }, zset! { 3 => -2, 4 => -1 }],
                vec![
                    zset! { 5 => 1, 6 => 1 },
                    zset! { 2 => -1, 7 => 1 },
                    zset! { 2 => 1, 7 => -1, 8 => 2, 9 => 1 },
                ],
            ]
            .into_iter();

            circuit
                .iterate(|child| {
                    let counter = Rc::new(RefCell::new(0));
                    let counter_clone = counter.clone();

                    let input = child.add_source(GeneratorNested::new(Box::new(move || {
                        *counter_clone.borrow_mut() = 0;
                        if Runtime::worker_index() == 0 {
                            let mut deltas = inputs.next().unwrap_or_default().into_iter();
                            Box::new(move || deltas.next().unwrap_or_else(|| zset! {}))
                        } else {
                            Box::new(|| zset! {})
                        }
                    })));

                    let distinct_inc = input.distinct().gather(0);
                    let distinct_noninc = input
                        // Non-incremental implementation of distinct_nested_incremental.
                        .integrate()
                        .integrate_nested()
                        .stream_distinct()
                        .differentiate()
                        .differentiate_nested()
                        .gather(0);

                    distinct_inc
                        .apply2(
                            &distinct_noninc,
                            |d1: &OrdZSet<usize, isize>, d2: &OrdZSet<usize, isize>| {
                                (d1.clone(), d2.clone())
                            },
                        )
                        .inspect(|(d1, d2)| assert_eq!(d1, d2));

                    Ok((
                        move || {
                            *counter.borrow_mut() += 1;
                            Ok(*counter.borrow() == 4)
                        },
                        (),
                    ))
                })
                .unwrap();
            Ok(())
        })
        .unwrap()
        .0;

        for _ in 0..3 {
            circuit.step().unwrap();
        }
    }

    #[test]
    fn distinct_indexed_test() {
        let output1 = Arc::new(Mutex::new(OrdIndexedZSet::empty(())));
        let output1_clone = output1.clone();

        let output2 = Arc::new(Mutex::new(OrdIndexedZSet::empty(())));
        let output2_clone = output2.clone();

        let (mut circuit, input) = Runtime::init_circuit(4, move |circuit| {
            let (input, input_handle) = circuit.add_input_indexed_zset::<usize, usize, isize>();

            input
                .integrate()
                .stream_distinct()
                .gather(0)
                .inspect(move |batch| {
                    if Runtime::worker_index() == 0 {
                        *output2_clone.lock().unwrap() = batch.clone();
                    }
                });

            input
                .distinct()
                .integrate()
                .gather(0)
                .inspect(move |batch| {
                    if Runtime::worker_index() == 0 {
                        *output1_clone.lock().unwrap() = batch.clone();
                    }
                });

            Ok(input_handle)
        })
        .unwrap();

        input.append(&mut vec![
            (1, (0, 1)),
            (1, (1, 2)),
            (2, (0, 1)),
            (2, (1, 1)),
        ]);
        circuit.step().unwrap();
        assert_eq!(
            &*output1.lock().unwrap(),
            &indexed_zset! { 1 => { 0 => 1, 1 => 1}, 2 => { 0 => 1, 1 => 1 } }
        );
        assert_eq!(&*output1.lock().unwrap(), &*output2.lock().unwrap(),);

        input.append(&mut vec![(3, (1, 1)), (2, (1, 1))]);
        circuit.step().unwrap();
        assert_eq!(
            &*output1.lock().unwrap(),
            &indexed_zset! { 1 => { 0 => 1, 1 => 1}, 2 => { 0 => 1, 1 => 1 }, 3 => { 1 => 1 } }
        );
        assert_eq!(&*output1.lock().unwrap(), &*output2.lock().unwrap(),);

        input.append(&mut vec![(1, (1, 3)), (2, (1, -3))]);
        circuit.step().unwrap();
        assert_eq!(
            &*output1.lock().unwrap(),
            &indexed_zset! { 1 => { 0 => 1, 1 => 1}, 2 => { 0 => 1 }, 3 => { 1 => 1 } }
        );
        assert_eq!(&*output1.lock().unwrap(), &*output2.lock().unwrap(),);

        circuit.kill().unwrap();
    }

    use proptest::{collection, prelude::*};

    type TestZSet = OrdZSet<usize, isize>;
    type TestIndexedZSet = OrdIndexedZSet<usize, isize, isize>;

    const MAX_ROUNDS: usize = 15;
    const MAX_ITERATIONS: usize = 15;
    const NUM_KEYS: usize = 10;
    const MAX_VAL: isize = 3;
    const MAX_TUPLES: usize = 10;

    fn test_zset() -> impl Strategy<Value = TestZSet> {
        collection::vec((0..NUM_KEYS, -1..=1isize), 0..MAX_TUPLES)
            .prop_map(|tuples| OrdZSet::from_tuples((), tuples))
    }

    fn test_input() -> impl Strategy<Value = Vec<TestZSet>> {
        collection::vec(test_zset(), 0..MAX_ROUNDS * MAX_ITERATIONS)
    }

    fn test_indexed_zset() -> impl Strategy<Value = TestIndexedZSet> {
        collection::vec(
            ((0..NUM_KEYS, -MAX_VAL..MAX_VAL), -1..=1isize),
            0..MAX_TUPLES,
        )
        .prop_map(|tuples| OrdIndexedZSet::from_tuples((), tuples))
    }

    fn test_indexed_input() -> impl Strategy<Value = Vec<TestIndexedZSet>> {
        collection::vec(test_indexed_zset(), 0..MAX_ROUNDS * MAX_ITERATIONS)
    }

    fn test_indexed_nested_input() -> impl Strategy<Value = Vec<Vec<TestIndexedZSet>>> {
        collection::vec(
            collection::vec(test_indexed_zset(), 0..MAX_ITERATIONS),
            0..MAX_ROUNDS,
        )
    }

    fn distinct_test_circuit(
        circuit: &mut RootCircuit,
        inputs: Vec<TestZSet>,
    ) -> AnyResult<(OutputHandle<TestZSet>, OutputHandle<TestZSet>)> {
        let mut inputs = inputs.into_iter();

        let input = circuit.add_source(Generator::new(Box::new(move || {
            if Runtime::worker_index() == 0 {
                inputs.next().unwrap_or_default()
            } else {
                zset! {}
            }
        })));

        let distinct_inc = input.distinct().output();
        let distinct_noninc = input.integrate().stream_distinct().differentiate().output();

        Ok((distinct_inc, distinct_noninc))
    }

    fn distinct_indexed_test_circuit(
        circuit: &mut RootCircuit,
        inputs: Vec<TestIndexedZSet>,
    ) -> AnyResult<(OutputHandle<TestIndexedZSet>, OutputHandle<TestIndexedZSet>)> {
        let mut inputs = inputs.into_iter();

        let input = circuit.add_source(Generator::new(Box::new(move || {
            if Runtime::worker_index() == 0 {
                inputs.next().unwrap_or_default()
            } else {
                indexed_zset! {}
            }
        })));

        let distinct_inc = input.distinct().output();
        let distinct_noninc = input.integrate().stream_distinct().differentiate().output();

        Ok((distinct_inc, distinct_noninc))
    }

    fn distinct_indexed_nested_test_circuit(
        circuit: &mut RootCircuit,
        inputs: Vec<Vec<TestIndexedZSet>>,
    ) -> AnyResult<()> {
        let mut inputs = inputs.into_iter();

        circuit
            .iterate(|child| {
                let counter = Rc::new(RefCell::new(0));
                let counter_clone = counter.clone();

                let input = child.add_source(GeneratorNested::new(Box::new(move || {
                    *counter_clone.borrow_mut() = 0;
                    if Runtime::worker_index() == 0 {
                        let mut deltas = inputs.next().unwrap_or_default().into_iter();
                        Box::new(move || deltas.next().unwrap_or_else(|| indexed_zset! {}))
                    } else {
                        Box::new(|| indexed_zset! {})
                    }
                })));

                let distinct_inc = input.distinct().gather(0);
                let distinct_noninc = input
                    .integrate_nested()
                    .integrate()
                    .stream_distinct()
                    .differentiate()
                    .differentiate_nested()
                    .gather(0);

                // Compare outputs of all three implementations.
                distinct_inc
                    .apply2(
                        &distinct_noninc,
                        |d1: &TestIndexedZSet, d2: &TestIndexedZSet| (d1.clone(), d2.clone()),
                    )
                    .inspect(|(d1, d2)| {
                        // if d1 != d2 {
                        //     println!("{}: incremental: {d1}", Runtime::worker_index());
                        //     println!("{}: non-incremental: {d2}", Runtime::worker_index());
                        // }
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

    proptest! {
        #[test]
        fn proptest_distinct_test_st(inputs in test_input()) {
            let iterations = inputs.len();
            let (circuit, (inc_output, noninc_output)) = RootCircuit::build(|circuit| distinct_test_circuit(circuit, inputs)).unwrap();

            for _ in 0..iterations {
                circuit.step().unwrap();
                assert_eq!(inc_output.consolidate(), noninc_output.consolidate());
            }
        }

        #[test]
        fn proptest_distinct_test_mt(inputs in test_input(), workers in (2..=16usize)) {
            let iterations = inputs.len();
            let (mut circuit, (inc_output, noninc_output)) = Runtime::init_circuit(workers, |circuit| distinct_test_circuit(circuit, inputs)).unwrap();

            for _ in 0..iterations {
                circuit.step().unwrap();
                assert_eq!(inc_output.consolidate(), noninc_output.consolidate());
            }

            circuit.kill().unwrap();
        }

        #[test]
        fn proptest_distinct_indexed_test_mt(inputs in test_indexed_input(), workers in (2..=4usize)) {
            let iterations = inputs.len();
            let (mut circuit, (inc_output, noninc_output)) = Runtime::init_circuit(workers, |circuit| distinct_indexed_test_circuit(circuit, inputs)).unwrap();

            for _ in 0..iterations {
                circuit.step().unwrap();
                assert_eq!(inc_output.consolidate(), noninc_output.consolidate());
            }

            circuit.kill().unwrap();
        }

        #[test]
        fn proptest_distinct_indexed_nested_test_mt(inputs in test_indexed_nested_input(), workers in (2..=4usize)) {
            let iterations = inputs.len();
            // for input in inputs.iter() {
            //     println!("round");
            //     for batch in input.iter() {
            //          println!("inputs: {batch}");
            //     }
            // }
            let mut circuit = Runtime::init_circuit(workers, |circuit| distinct_indexed_nested_test_circuit(circuit, inputs)).unwrap().0;

            for _ in 0..iterations {
                circuit.step().unwrap();
            }

            circuit.kill().unwrap();
        }
    }
}
