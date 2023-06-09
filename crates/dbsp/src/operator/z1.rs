//! z^-1 operator delays its input by one timestamp.

use crate::{
    algebra::HasZero,
    circuit::{
        metadata::{MetaItem, OperatorMeta},
        operator_traits::{Operator, StrictOperator, StrictUnaryOperator, UnaryOperator},
        Circuit, ExportId, ExportStream, FeedbackConnector, GlobalNodeId, OwnershipPreference,
        Scope, Stream,
    },
    circuit_cache_key, NumEntries,
};
use size_of::{Context, SizeOf};
use std::{borrow::Cow, mem::replace};

circuit_cache_key!(DelayedId<C, D>(GlobalNodeId => Stream<C, D>));
circuit_cache_key!(NestedDelayedId<C, D>(GlobalNodeId => Stream<C, D>));

/// Like [`FeedbackConnector`] but specialized for [`Z1`] feedback operator.
///
/// Use this API instead of the low-level [`Circuit::add_feedback`] API to
/// create feedback loops with `Z1` operator.  In addition to being more
/// concise, this API takes advantage of [caching](`crate::circuit::cache`).
pub struct DelayedFeedback<C, D>
where
    C: Circuit,
{
    feedback: FeedbackConnector<C, D, D, Z1<D>>,
    output: Stream<C, D>,
    export: Stream<C::Parent, D>,
}

impl<C, D> DelayedFeedback<C, D>
where
    C: Circuit,
    D: Eq + SizeOf + NumEntries + Clone + HasZero + 'static,
{
    /// Create a feedback loop with `Z1` operator.  Use [`Self::connect`] to
    /// close the loop.
    pub fn new(circuit: &C) -> Self {
        let (ExportStream { local, export }, feedback) =
            circuit.add_feedback_with_export(Z1::new(D::zero()));

        Self {
            feedback,
            output: local,
            export,
        }
    }

    /// Output stream of the `Z1` operator.
    pub fn stream(&self) -> &Stream<C, D> {
        &self.output
    }

    /// Connect `input` stream to the input of the `Z1` operator.
    pub fn connect(self, input: &Stream<C, D>) {
        let Self {
            feedback,
            output,
            export,
        } = self;
        let circuit = output.circuit().clone();

        feedback.connect_with_preference(input, OwnershipPreference::STRONGLY_PREFER_OWNED);
        circuit.cache_insert(DelayedId::new(input.origin_node_id().clone()), output);
        circuit.cache_insert(ExportId::new(input.origin_node_id().clone()), export);
    }
}

/// Like [`FeedbackConnector`] but specialized for [`Z1Nested`] feedback
/// operator.
///
/// Use this API instead of the low-level [`Circuit::add_feedback`] API to
/// create feedback loops with `Z1Nested` operator.  In addition to being more
/// concise, this API takes advantage of [caching](`crate::circuit::cache`).
pub struct DelayedNestedFeedback<C, D> {
    feedback: FeedbackConnector<C, D, D, Z1Nested<D>>,
    output: Stream<C, D>,
}

impl<C, D> DelayedNestedFeedback<C, D>
where
    C: Circuit,
    D: Eq + SizeOf + NumEntries + Clone + 'static,
{
    /// Create a feedback loop with `Z1` operator.  Use [`Self::connect`] to
    /// close the loop.
    pub fn new(circuit: &C) -> Self
    where
        D: HasZero,
    {
        let (output, feedback) = circuit.add_feedback(Z1Nested::new(D::zero()));
        Self { feedback, output }
    }

    /// Output stream of the `Z1Nested` operator.
    pub fn stream(&self) -> &Stream<C, D> {
        &self.output
    }

    /// Connect `input` stream to the input of the `Z1Nested` operator.
    pub fn connect(self, input: &Stream<C, D>) {
        let Self { feedback, output } = self;
        let circuit = output.circuit().clone();

        feedback.connect_with_preference(input, OwnershipPreference::STRONGLY_PREFER_OWNED);
        circuit.cache_insert(NestedDelayedId::new(input.origin_node_id().clone()), output);
    }
}

impl<C, D> Stream<C, D>
where
    C: Circuit,
{
    /// Applies [`Z1`] operator to `self`.
    pub fn delay(&self) -> Stream<C, D>
    where
        D: Eq + SizeOf + NumEntries + Clone + HasZero + 'static,
    {
        self.circuit()
            .cache_get_or_insert_with(DelayedId::new(self.origin_node_id().clone()), || {
                self.circuit().add_unary_operator(Z1::new(D::zero()), self)
            })
            .clone()
    }

    /// Applies [`Z1Nested`] operator to `self`.
    pub fn delay_nested(&self) -> Stream<C, D>
    where
        D: Eq + Clone + HasZero + SizeOf + NumEntries + 'static,
    {
        self.circuit()
            .cache_get_or_insert_with(NestedDelayedId::new(self.origin_node_id().clone()), || {
                self.circuit()
                    .add_unary_operator(Z1Nested::new(D::zero()), self)
            })
            .clone()
    }
}

/// z^-1 operator delays its input by one timestamp.
///
/// The operator outputs a user-defined "zero" value in the first timestamp
/// after [clock_start](`Z1::clock_start`).  For all subsequent timestamps, it
/// outputs the value received as input at the previous timestamp.  The zero
/// value is typically the neutral element of a monoid (e.g., 0 for addition
/// or 1 for multiplication).
///
/// It is a [strict
/// operator](`crate::circuit::operator_traits::StrictOperator`).
///
/// # Examples
///
/// ```text
/// time | input | output
/// ---------------------
///   0  |   5   |   0
///   1  |   6   |   5
///   2  |   7   |   6
///   3  |   8   |   7
///         ...
/// ```
pub struct Z1<T> {
    zero: T,
    empty_output: bool,
    values: T,
}

impl<T> Z1<T>
where
    T: Clone,
{
    pub fn new(zero: T) -> Self {
        Self {
            zero: zero.clone(),
            empty_output: false,
            values: zero,
        }
    }
}

impl<T> Operator for Z1<T>
where
    T: Eq + SizeOf + NumEntries + Clone + 'static,
{
    fn name(&self) -> Cow<'static, str> {
        Cow::from("Z^-1")
    }

    fn clock_start(&mut self, _scope: Scope) {}
    fn clock_end(&mut self, _scope: Scope) {
        self.empty_output = false;
        self.values = self.zero.clone();
    }

    fn metadata(&self, meta: &mut OperatorMeta) {
        let bytes = self.values.size_of();
        meta.extend(metadata! {
            "total size" => self.values.num_entries_deep(),
            "allocated bytes" => MetaItem::bytes(bytes.total_bytes()),
            "used bytes" => MetaItem::bytes(bytes.used_bytes()),
            "allocations" => bytes.distinct_allocations(),
            "shared bytes" => MetaItem::bytes(bytes.shared_bytes()),
        });
    }

    fn fixedpoint(&self, scope: Scope) -> bool {
        if scope == 0 {
            self.values.num_entries_shallow() == 0 && self.empty_output
        } else {
            true
        }
    }
}

impl<T> UnaryOperator<T, T> for Z1<T>
where
    T: Eq + SizeOf + NumEntries + Clone + 'static,
{
    fn eval(&mut self, i: &T) -> T {
        replace(&mut self.values, i.clone())
    }

    fn eval_owned(&mut self, i: T) -> T {
        replace(&mut self.values, i)
    }

    fn input_preference(&self) -> OwnershipPreference {
        OwnershipPreference::PREFER_OWNED
    }
}

impl<T> StrictOperator<T> for Z1<T>
where
    T: Eq + SizeOf + NumEntries + Clone + 'static,
{
    fn get_output(&mut self) -> T {
        self.empty_output = self.values.num_entries_shallow() == 0;
        replace(&mut self.values, self.zero.clone())
    }

    fn get_final_output(&mut self) -> T {
        self.get_output()
    }
}

impl<T> StrictUnaryOperator<T, T> for Z1<T>
where
    T: Eq + SizeOf + NumEntries + Clone + 'static,
{
    fn eval_strict(&mut self, i: &T) {
        self.values = i.clone();
    }

    fn eval_strict_owned(&mut self, i: T) {
        self.values = i;
    }

    fn input_preference(&self) -> OwnershipPreference {
        OwnershipPreference::PREFER_OWNED
    }
}

/// z^-1 operator over streams of streams.
///
/// The operator stores a complete nested stream consumed at the last iteration
/// of the parent clock and outputs it at the next parent clock cycle.
/// It outputs a stream of zeros in the first parent clock tick.
///
/// One important subtlety is that mathematically speaking nested streams are
/// infinite, but we can only compute and store finite prefixes of such
/// streams.  When asked to produce output beyond the stored prefix, the z^-1
/// operator repeats the last value in the stored prefix.  The assumption
/// here is that the circuit was evaluated to a fixed point at the previous
/// parent timestamp, and hence the rest of the stream will contain the same
/// fixed point value.  This is not generally true for all circuits, and
/// users should keep this in mind when instantiating the operator.
///
/// It is a [strict
/// operator](`crate::circuit::operator_traits::StrictOperator`).
///
/// # Examples
///
/// Input (one row per parent timestamps):
///
/// ```text
/// 1 2 3 4
/// 1 1 1 1 1
/// 2 2 2 0 0
/// ```
///
/// Output:
///
/// ```text
/// 0 0 0 0
/// 1 2 3 4 4
/// 1 1 1 1 1
/// ```
pub struct Z1Nested<T> {
    zero: T,
    timestamp: usize,
    values: Vec<T>,
}

impl<T> Z1Nested<T> {
    const fn new(zero: T) -> Self {
        Self {
            zero,
            timestamp: 0,
            values: Vec::new(),
        }
    }

    fn reset(&mut self) {
        self.timestamp = 0;
        self.values.clear();
    }
}

impl<T> Operator for Z1Nested<T>
where
    T: Eq + SizeOf + NumEntries + Clone + 'static,
{
    fn name(&self) -> Cow<'static, str> {
        Cow::from("Z^-1 (nested)")
    }

    fn clock_start(&mut self, scope: Scope) {
        if scope == 0 {
            self.values.truncate(self.timestamp);
        }
        self.timestamp = 0;
    }

    fn clock_end(&mut self, scope: Scope) {
        if scope > 0 {
            self.reset();
        }
    }

    fn metadata(&self, meta: &mut OperatorMeta) {
        let total_size: usize = self
            .values
            .iter()
            .map(|batch| batch.num_entries_deep())
            .sum();

        let batch_sizes = self
            .values
            .iter()
            .map(|batch| MetaItem::Int(batch.num_entries_deep()))
            .collect();

        let total_bytes = {
            let mut context = Context::new();
            for value in &self.values {
                value.size_of_with_context(&mut context);
            }
            context.total_size()
        };

        meta.extend(metadata! {
            "total size" => total_size,
            "batch sizes" => MetaItem::Array(batch_sizes),
            "allocated bytes" => MetaItem::bytes(total_bytes.total_bytes()),
            "used bytes" => MetaItem::bytes(total_bytes.used_bytes()),
            "allocations" => total_bytes.distinct_allocations(),
            "shared bytes" => MetaItem::bytes(total_bytes.shared_bytes()),
        });
    }

    fn fixedpoint(&self, scope: Scope) -> bool {
        if scope == 0 {
            self.values
                .iter()
                .skip(self.timestamp - 1)
                .all(|v| *v == self.zero)
        } else {
            false
        }
    }
}

impl<T> UnaryOperator<T, T> for Z1Nested<T>
where
    T: Eq + SizeOf + NumEntries + Clone + 'static,
{
    fn eval(&mut self, i: &T) -> T {
        debug_assert!(self.timestamp <= self.values.len());

        if self.timestamp == self.values.len() {
            self.values.push(self.zero.clone());
        } else if self.timestamp == self.values.len() - 1 {
            self.values.push(self.values.last().unwrap().clone());
        }

        let result = replace(&mut self.values[self.timestamp], i.clone());

        self.timestamp += 1;
        result
    }

    fn eval_owned(&mut self, i: T) -> T {
        debug_assert!(self.timestamp <= self.values.len());

        if self.timestamp == self.values.len() {
            self.values.push(self.zero.clone());
        } else if self.timestamp == self.values.len() - 1 {
            self.values.push(self.values.last().unwrap().clone());
        }

        let result = replace(&mut self.values[self.timestamp], i);
        self.timestamp += 1;

        result
    }

    fn input_preference(&self) -> OwnershipPreference {
        OwnershipPreference::PREFER_OWNED
    }
}

impl<T> StrictOperator<T> for Z1Nested<T>
where
    T: Eq + SizeOf + NumEntries + Clone + 'static,
{
    fn get_output(&mut self) -> T {
        if self.timestamp >= self.values.len() {
            assert_eq!(self.timestamp, self.values.len());
            self.values.push(self.zero.clone());
        } else if self.timestamp == self.values.len() - 1 {
            self.values.push(self.values.last().unwrap().clone())
        }

        replace(
            // Safe due to the check above.
            unsafe { self.values.get_unchecked_mut(self.timestamp) },
            self.zero.clone(),
        )
    }

    fn get_final_output(&mut self) -> T {
        self.get_output()
    }
}

impl<T> StrictUnaryOperator<T, T> for Z1Nested<T>
where
    T: Eq + SizeOf + NumEntries + Clone + 'static,
{
    fn eval_strict(&mut self, i: &T) {
        debug_assert!(self.timestamp < self.values.len());

        self.values[self.timestamp] = i.clone();
        self.timestamp += 1;
    }

    fn eval_strict_owned(&mut self, i: T) {
        debug_assert!(self.timestamp < self.values.len());

        self.values[self.timestamp] = i;
        self.timestamp += 1;
    }

    fn input_preference(&self) -> OwnershipPreference {
        OwnershipPreference::PREFER_OWNED
    }
}

#[cfg(test)]
mod test {
    use crate::{
        circuit::operator_traits::{Operator, StrictOperator, StrictUnaryOperator, UnaryOperator},
        operator::{Z1Nested, Z1},
    };

    #[test]
    fn z1_test() {
        let mut z1 = Z1::new(0);

        let expected_result = vec![0, 1, 2, 0, 4, 5];

        // Test `UnaryOperator` API.
        let mut res = Vec::new();
        z1.clock_start(0);
        res.push(z1.eval(&1));
        res.push(z1.eval(&2));
        res.push(z1.eval(&3));
        z1.clock_end(0);

        z1.clock_start(0);
        res.push(z1.eval_owned(4));
        res.push(z1.eval_owned(5));
        res.push(z1.eval_owned(6));
        z1.clock_end(0);

        assert_eq!(res, expected_result);

        // Test `StrictUnaryOperator` API.
        let mut res = Vec::new();
        z1.clock_start(0);
        res.push(z1.get_output());
        z1.eval_strict(&1);
        res.push(z1.get_output());
        z1.eval_strict(&2);
        res.push(z1.get_output());
        z1.eval_strict(&3);
        z1.clock_end(0);

        z1.clock_start(0);
        res.push(z1.get_output());
        z1.eval_strict_owned(4);
        res.push(z1.get_output());
        z1.eval_strict_owned(5);
        res.push(z1.get_output());
        z1.eval_strict_owned(6);
        z1.clock_end(0);

        assert_eq!(res, expected_result);
    }

    #[test]
    fn z1_nested_test() {
        let mut z1 = Z1Nested::new(0);

        // Test `UnaryOperator` API.

        z1.clock_start(1);

        let mut res = Vec::new();
        z1.clock_start(0);
        res.push(z1.eval_owned(1));
        res.push(z1.eval(&2));
        res.push(z1.eval(&3));
        z1.clock_end(0);
        assert_eq!(res.as_slice(), &[0, 0, 0]);

        let mut res = Vec::new();
        z1.clock_start(0);
        res.push(z1.eval_owned(4));
        res.push(z1.eval_owned(5));
        z1.clock_end(0);
        assert_eq!(res.as_slice(), &[1, 2]);

        let mut res = Vec::new();
        z1.clock_start(0);
        res.push(z1.eval_owned(6));
        res.push(z1.eval_owned(7));
        res.push(z1.eval(&8));
        res.push(z1.eval(&9));
        z1.clock_end(0);
        assert_eq!(res.as_slice(), &[4, 5, 5, 5]);

        z1.clock_end(1);

        // Test `StrictUnaryOperator` API.
        z1.clock_start(1);

        let mut res = Vec::new();
        z1.clock_start(0);
        res.push(z1.get_output());
        z1.eval_strict(&1);
        res.push(z1.get_output());
        z1.eval_strict(&2);
        res.push(z1.get_output());
        z1.eval_strict(&3);
        z1.clock_end(0);
        assert_eq!(res.as_slice(), &[0, 0, 0]);

        let mut res = Vec::new();

        z1.clock_start(0);
        res.push(z1.get_output());
        z1.eval_strict_owned(4);
        res.push(z1.get_output());
        z1.eval_strict_owned(5);
        z1.clock_end(0);

        assert_eq!(res.as_slice(), &[1, 2]);

        let mut res = Vec::new();

        z1.clock_start(0);
        res.push(z1.get_output());
        z1.eval_strict_owned(6);
        res.push(z1.get_output());
        z1.eval_strict_owned(7);
        res.push(z1.get_output());
        z1.eval_strict_owned(8);
        res.push(z1.get_output());
        z1.eval_strict_owned(9);
        z1.clock_end(0);

        assert_eq!(res.as_slice(), &[4, 5, 5, 5]);

        z1.clock_end(1);
    }
}
