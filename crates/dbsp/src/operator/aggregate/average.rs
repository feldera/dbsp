use crate::{
    algebra::{
        AddAssignByRef, AddByRef, GroupValue, HasOne, HasZero, IndexedZSet, MulByRef, NegByRef,
        ZRingValue,
    },
    circuit::WithClock,
    trace::layers::{column_layer::ColumnLayer, ordered::OrderedLayer},
    utils::VecExt,
    Circuit, DBData, DBTimestamp, DBWeight, OrdIndexedZSet, Stream,
};
use bincode::{Decode, Encode};
use size_of::SizeOf;
use std::{
    hash::Hash,
    ops::{Add, AddAssign, Div, Neg},
};

/// Representation of a partially computed average aggregate as a `(sum, count)`
/// tuple.
///
/// This struct represents the result of the linear part of the average
/// aggregate as a `(sum, count)` tuple (see [`Stream::average`]).  The actual
/// average value can be obtained by dividing `sum` by `count`.  `Avg` forms a
/// commutative monoid with point-wise plus operation `(sum1, count1) + (sum2,
/// count2) = (sum1 + sum2, count1 + count2)`.
#[derive(Debug, Default, Clone, Eq, Hash, PartialEq, Ord, PartialOrd, SizeOf)]
pub struct Avg<T, R> {
    sum: T,
    count: R,
}

impl<T, R> Avg<T, R> {
    /// Create a new `Avg` object with the given `sum` and `count`.
    pub const fn new(sum: T, count: R) -> Self {
        Self { sum, count }
    }

    /// Returns the `sum` component of the `(sum, count)` tuple.
    pub fn sum(&self) -> T
    where
        T: Clone,
    {
        self.sum.clone()
    }

    /// Returns the `count` component of the `(sum, count)` tuple.
    pub fn count(&self) -> R
    where
        R: Clone,
    {
        self.count.clone()
    }

    /// Returns `sum / count` or `None` if `count` is zero.
    pub fn compute_avg(&self) -> Option<T>
    where
        R: Clone + HasZero,
        T: From<R> + Div<Output = T> + Clone,
    {
        if self.count.is_zero() {
            None
        } else {
            Some(self.sum.clone() / T::from(self.count.clone()))
        }
    }
}

impl<T, R> bincode::Encode for Avg<T, R>
where
    T: bincode::Encode + bincode::Decode,
    R: bincode::Encode + bincode::Decode,
{
    fn encode<E: bincode::enc::Encoder>(
        &self,
        encoder: &mut E,
    ) -> core::result::Result<(), bincode::error::EncodeError> {
        bincode::Encode::encode(&self.sum, encoder)?;
        bincode::Encode::encode(&self.count, encoder)?;
        Ok(())
    }
}

impl<T, R> bincode::Decode for Avg<T, R>
where
    T: bincode::Encode + bincode::Decode,
    R: bincode::Encode + bincode::Decode,
{
    fn decode<D: bincode::de::Decoder>(
        decoder: &mut D,
    ) -> Result<Self, bincode::error::DecodeError> {
        let sum: T = bincode::Decode::decode(decoder)?;
        let count: R = bincode::Decode::decode(decoder)?;
        Ok(Self::new(sum, count))
    }
}

impl<T, R> HasZero for Avg<T, R>
where
    T: HasZero,
    R: HasZero,
{
    fn is_zero(&self) -> bool {
        self.sum.is_zero() && self.count.is_zero()
    }

    fn zero() -> Self {
        Self::new(T::zero(), R::zero())
    }
}

impl<T, R> Add for Avg<T, R>
where
    T: Add<Output = T>,
    R: Add<Output = R>,
{
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self::new(self.sum + rhs.sum, self.count + rhs.count)
    }
}

impl<T, R> AddByRef for Avg<T, R>
where
    T: AddByRef,
    R: AddByRef,
{
    fn add_by_ref(&self, other: &Self) -> Self {
        Self::new(
            self.sum.add_by_ref(&other.sum),
            self.count.add_by_ref(&other.count),
        )
    }
}

impl<T, R> AddAssign for Avg<T, R>
where
    T: AddAssign,
    R: AddAssign,
{
    fn add_assign(&mut self, rhs: Self) {
        self.sum += rhs.sum;
        self.count += rhs.count;
    }
}

impl<T, R> AddAssignByRef for Avg<T, R>
where
    T: AddAssignByRef,
    R: AddAssignByRef,
{
    fn add_assign_by_ref(&mut self, rhs: &Self) {
        self.sum.add_assign_by_ref(&rhs.sum);
        self.count.add_assign_by_ref(&rhs.count);
    }
}

impl<T, R> Neg for Avg<T, R>
where
    T: Neg<Output = T>,
    R: Neg<Output = R>,
{
    type Output = Self;

    fn neg(self) -> Self {
        Self::new(self.sum.neg(), self.count.neg())
    }
}

impl<T, R> NegByRef for Avg<T, R>
where
    T: NegByRef,
    R: NegByRef,
{
    fn neg_by_ref(&self) -> Self {
        Self::new(self.sum.neg_by_ref(), self.count.neg_by_ref())
    }
}

impl<T, R> MulByRef<R> for Avg<T, R>
where
    T: MulByRef<Output = T>,
    T: From<R>,
    R: MulByRef<Output = R>,
    // This bound is only here to prevent conflict with `MulByRef<Present>` :(
    R: From<i8> + Clone,
{
    type Output = Avg<T, R>;

    fn mul_by_ref(&self, rhs: &R) -> Avg<T, R> {
        Self::new(
            self.sum.mul_by_ref(&T::from(rhs.clone())),
            self.count.mul_by_ref(rhs),
        )
    }
}

impl<C, Z> Stream<C, Z>
where
    C: Circuit,
    <C as WithClock>::Time: DBTimestamp,
    Z: Clone + 'static,
{
    /// Incremental average aggregate.
    ///
    /// This operator is a specialization of [`Stream::aggregate`] that for
    /// each key `k` in the input indexed Z-set computes the average value as:
    ///
    ///
    /// ```text
    ///    __                __
    ///    ╲                 ╲
    ///    ╱ v * w     /     ╱  w
    ///    ‾‾                ‾‾
    ///   (v,w) ∈ Z[k]      (v,w) ∈ Z[k]
    /// ```
    ///
    /// # Design
    ///
    /// Average is a quasi-linear aggregate, meaning that it can be efficiently
    /// computed as a composition of two linear aggregates: sum and count.
    /// The `(sum, count)` pair with pair-wise operations is also a linear
    /// aggregate and can be computed with a single
    /// [`Stream::aggregate_linear`] operator. The actual average is
    /// computed by applying the `(sum, count) -> sum / count`
    /// transformation to its output.
    #[track_caller]
    pub fn average<A, F>(&self, f: F) -> Stream<C, OrdIndexedZSet<Z::Key, A, Z::R>>
    where
        Z: IndexedZSet + Encode + Decode,
        Z::R: ZRingValue,
        Avg<A, Z::R>: MulByRef<Z::R, Output = Avg<A, Z::R>>,
        A: DBData + From<Z::R> + Div<Output = A> + GroupValue,
        F: Fn(&Z::Key, &Z::Val) -> A + Clone + 'static,
    {
        let aggregate = self.aggregate_linear(move |key, val| Avg::new(f(key, val), Z::R::one()));

        // We're the only possible consumer of the aggregated stream so we can use an
        // owned consumer to skip any cloning
        let average = aggregate.apply_owned_named("ApplyAverage", apply_average);

        // Note: Currently `.aggregate_linear()` is always sharded, but we just do this
        // check so that we don't get any unpleasant surprises if that ever changes
        average.mark_sharded_if(&aggregate);

        average
    }
}

/// The gist of what we're doing here is this:
///
/// - We receive an owned `OrdIndexedZSet` so we can do whatever we want with it
/// - `OrdIndexedZSet` consists of four discrete vectors of values, a `Vec<K>`
///   of keys, a `Vec<usize>` of value offsets, a `Vec<Avg<A>>` of average
///   aggregates and a `Vec<isize>` of differences
/// - Of these only one vector needs to be changed in *any* way: The
///   `Vec<Avg<A>>` needs to be transformed into a `Vec<A>`
/// - So following this fact, the other three vectors never need to be touched
/// - Ergo, we simply reuse those three untouched vectors in our output
///   `OrdIndexedZSet`, requiring us to do the minimum of work: dividing all of
///   our sums by all of our counts within the `Vec<Avg<A>>` to produce our
///   output `Vec<A>`
///
/// Note that unfortunately we can't reuse the `Vec<Avg<A>>`'s allocation here
/// since an `Avg<A>` will never have the same size as an `A` due to `Avg<A>`
/// containing an extra `isize` field
fn apply_average<K, A, R, W>(aggregate: OrdIndexedZSet<K, Avg<A, R>, W>) -> OrdIndexedZSet<K, A, W>
where
    K: Ord,
    A: DBData + From<R> + Div<Output = A>,
    R: DBData + ZRingValue,
    W: DBWeight,
{
    // Break the given `OrdIndexedZSet` into its components
    let OrderedLayer {
        keys,
        offs,
        vals,
        lower_bound,
    } = aggregate.layer;
    let (aggregates, diffs, lower_bound_avg) = vals.into_parts();

    // Average out the aggregated values
    // TODO: If we stored `Avg<A>` as two columns (one of `sum: A` and one of
    // `count: isize`) we could even reuse the `sum` vec by doing the division in
    // place (and we even could do it with simd depending on `A`'s type)
    let mut averages = Vec::with_capacity(aggregates.len());
    for avg in aggregates {
        // TODO: This can technically use an unchecked division (or an
        // `assume(count != 0)` call since `.div_unchecked()` is unstable) since `count`
        // should never be zero since zeroed elements are removed from zsets
        debug_assert!(!avg.count().is_zero());

        // Safety: We allocated the correct capacity for `aggregate_values`
        unsafe { averages.push_unchecked(avg.compute_avg().unwrap()) };
    }

    // Safety: `averages.len() == diffs.len()`
    let averages = unsafe { ColumnLayer::from_parts(averages, diffs, lower_bound_avg) };

    // Create a new `OrdIndexedZSet` from our components, notably this doesn't touch
    // `keys`, `offs` or `diffs` which means we don't allocate new vectors for them
    // or even touch their memory
    OrdIndexedZSet {
        // Safety: `keys.len() + 1 == offs.len()`
        layer: unsafe { OrderedLayer::from_parts(keys, offs, averages, lower_bound) },
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        indexed_zset,
        operator::aggregate::average::{apply_average, Avg},
    };

    #[test]
    fn apply_average_smoke() {
        let input = indexed_zset! {
            0 => { Avg::new(1000, 10) => 1 },
            1 => { Avg::new(1, 1) => -12 },
            1000 => { Avg::new(200, 20) => 544 },
        };
        let expected = indexed_zset! {
            0 => { 100 => 1 },
            1 => { 1 => -12 },
            1000 => { 10 => 544 },
        };

        let output = apply_average(input);
        assert_eq!(output, expected);
    }

    #[test]
    fn avg_decode_encode() {
        let mut slice = [0u8; 20];
        for input in [
            Avg::new(usize::MIN, isize::MIN),
            Avg::new(0, 0),
            Avg::new(usize::MAX, isize::MAX),
        ]
        .into_iter()
        {
            let _length =
                bincode::encode_into_slice(&input, &mut slice, bincode::config::standard())
                    .unwrap();
            let decoded: Avg<usize, isize> =
                bincode::decode_from_slice(&slice, bincode::config::standard())
                    .unwrap()
                    .0;
            assert_eq!(decoded, input);
        }
    }
}
