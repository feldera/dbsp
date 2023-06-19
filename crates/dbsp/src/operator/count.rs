//! Count operators.

use bincode::{Decode, Encode};

use crate::{
    algebra::{HasOne, IndexedZSet, ZRingValue},
    circuit::{Circuit, Stream, WithClock},
    trace::Batch,
    DBTimestamp, OrdIndexedZSet,
};

impl<C, Z> Stream<C, Z>
where
    C: Circuit,
    <C as WithClock>::Time: DBTimestamp,
    Z: IndexedZSet + Encode + Decode,
    Z::R: ZRingValue,
{
    /// Incrementally sums the weights for each key `self` into an indexed Z-set
    /// that maps from the original keys to the weights.  Both the input and
    /// output are streams of updates.
    #[allow(clippy::type_complexity)]
    pub fn weighted_count(&self) -> Stream<C, OrdIndexedZSet<Z::Key, Z::R, Z::R>> {
        self.weighted_count_generic()
    }

    /// Like [`Self::weighted_count`], but can return any batch type.
    pub fn weighted_count_generic<O>(&self) -> Stream<C, O>
    where
        O: Batch<Key = Z::Key, Val = Z::R, R = Z::R, Time = ()>,
    {
        self.aggregate_linear_generic(|_k, _v| Z::R::one())
    }

    /// Incrementally, for each key in `self`, counts the number of unique
    /// values having positive weights, and outputs it as an indexed Z-set
    /// that maps from the original keys to the unique value counts.  Both
    /// the input and output are streams of updates.
    #[allow(clippy::type_complexity)]
    pub fn distinct_count(&self) -> Stream<C, OrdIndexedZSet<Z::Key, Z::R, Z::R>>
    where
        Z: Send,
    {
        self.distinct_count_generic()
    }

    /// Like [`Self::distinct_count`], but can return any batch type.
    pub fn distinct_count_generic<O>(&self) -> Stream<C, O>
    where
        O: Batch<Key = Z::Key, Val = Z::R, R = Z::R, Time = ()>,
        Z: Send,
    {
        self.distinct().weighted_count_generic()
    }

    /// Non-incrementally sums the weights for each key `self` into an indexed
    /// Z-set that maps from the original keys to the weights.  Both the
    /// input and output are streams of data (not updates).
    #[allow(clippy::type_complexity)]
    pub fn stream_weighted_count(&self) -> Stream<C, OrdIndexedZSet<Z::Key, Z::R, Z::R>>
    where
        Z: Send,
    {
        self.stream_weighted_count_generic()
    }

    /// Like [`Self::stream_weighted_count`], but can return any batch type.
    pub fn stream_weighted_count_generic<O>(&self) -> Stream<C, O>
    where
        O: IndexedZSet<Key = Z::Key, Val = Z::R, R = Z::R, Time = ()>,
        Z: Send,
    {
        self.stream_aggregate_linear_generic(|_k, _v| Z::R::one())
    }

    /// Incrementally, for each key in `self`, counts the number of unique
    /// values having positive weights, and outputs it as an indexed Z-set
    /// that maps from the original keys to the unique value counts.  Both
    /// the input and output are streams of data (not updates).
    #[allow(clippy::type_complexity)]
    pub fn stream_distinct_count(&self) -> Stream<C, OrdIndexedZSet<Z::Key, Z::R, Z::R>>
    where
        Z: Send,
    {
        self.stream_distinct_count_generic()
    }

    /// Like [`Self::distinct_count`], but can return any batch type.
    pub fn stream_distinct_count_generic<O>(&self) -> Stream<C, O>
    where
        O: IndexedZSet<Key = Z::Key, Val = Z::R, R = Z::R, Time = ()>,
        Z: Send,
    {
        self.stream_distinct().stream_weighted_count_generic()
    }
}

#[cfg(test)]
mod test {
    use crate::{
        indexed_zset,
        operator::{FilterMap, Generator},
        trace::Batch,
        zset, Circuit, OrdIndexedZSet, RootCircuit,
    };
    use core::ops::Range;
    use rand::{rngs::StdRng, seq::SliceRandom, Rng, SeedableRng};

    #[test]
    fn weighted_count_test() {
        let (circuit, (counts, stream_counts, expected_counts)) =
            RootCircuit::build(move |circuit| {
                // Generate sequence with key 1 and weights 1, -2, 4, -8, 16, -32, ...
                let mut next = 1;
                let ones = circuit.add_source(Generator::new(move || {
                    let this = zset! { 1 => next };
                    next *= -2;
                    this
                }));

                // Generate sequence with key 2 and delayed weights.
                let twos = ones.map(|_| 2).delay();

                let counts = ones.plus(&twos).weighted_count().integrate();
                let stream_counts = ones.plus(&twos).integrate().stream_weighted_count();

                // Generate expected values in `counts` by another means, using the formula for
                // A077925 (https://oeis.org/A077925).
                let mut term = 0;
                fn a077925(n: isize) -> isize {
                    let mut x = 2 << n;
                    if (n & 1) == 0 {
                        x = -x;
                    }
                    (1 - x) / 3
                }
                let expected_ones = circuit.add_source(Generator::new(move || {
                    term += 1;
                    indexed_zset! { 1 => {a077925 (term - 1) => 1 } }
                }));
                let expected_twos = expected_ones.map_index(|(&_k, &v)| (2, v)).delay();
                let expected_counts = expected_ones.plus(&expected_twos);

                Ok((
                    counts.output(),
                    stream_counts.output(),
                    expected_counts.output(),
                ))
            })
            .unwrap();

        for _ in 0..10 {
            circuit.step().unwrap();
            let counts = counts.consolidate();
            let stream_counts = stream_counts.consolidate();
            let expected_counts = expected_counts.consolidate();
            // println!("counts={}", counts);
            // println!("stream_counts={}", stream_counts);
            // println!("expected={}", expected_counts);
            assert_eq!(counts, expected_counts);
            assert_eq!(stream_counts, expected_counts);
        }
    }

    #[test]
    fn distinct_count_test() {
        // Number of steps to test.
        const N: usize = 50;

        // Generate `input` as a vector of `N` Z-sets with keys in range `K`, values in
        // range `V`, and weights in range `W`, and `expected` as a vector that
        // for each element in `input` contains a Z-set that maps from each key
        // to the number of values with positive weight.
        const K: Range<usize> = 0..10; // Range of keys in Z-set.
        const V: Range<usize> = 0..10; // Range of values in Z-set.
        const W: Range<isize> = -10..10; // Range of weights in Z-set.
        let mut rng = StdRng::seed_from_u64(0); // Make the test reproducible.
        let mut input: Vec<OrdIndexedZSet<usize, isize, isize>> = Vec::new();
        let mut expected: Vec<OrdIndexedZSet<usize, isize, isize>> = Vec::new();
        for _ in 0..N {
            let mut input_tuples = Vec::new();
            let mut expected_tuples = Vec::new();
            for k in K {
                let mut v: Vec<usize> = V.collect();
                let n = rng.gen_range(V);
                v.partial_shuffle(&mut rng, n);

                let mut distinct_count = 0;
                for &v in &v[0..n] {
                    let w = rng.gen_range(W);
                    input_tuples.push(((k, v as isize), w));
                    if w > 0 {
                        distinct_count += 1;
                    }
                }
                if distinct_count > 0 {
                    expected_tuples.push(((k, distinct_count), 1));
                }
            }
            input.push(OrdIndexedZSet::from_tuples((), input_tuples));
            expected.push(OrdIndexedZSet::from_tuples((), expected_tuples));
        }
        let input_copy = input.clone();

        let (circuit, (counts, stream_counts)) = RootCircuit::build(move |circuit| {
            let mut iter = input.into_iter();
            let source =
                circuit.add_source(Generator::new(move || iter.next().unwrap_or_default()));
            let counts = source.differentiate().distinct_count().integrate();
            let stream_counts = source.stream_distinct_count();
            Ok((counts.output(), stream_counts.output()))
        })
        .unwrap();

        for (_input, expected_counts) in input_copy.into_iter().zip(expected.into_iter()) {
            circuit.step().unwrap();
            let counts = counts.consolidate();
            let stream_counts = stream_counts.consolidate();
            // println!("input={}", _input);
            // println!("counts={}", counts);
            // println!("stream_counts={}", stream_counts);
            // println!("expected={}", expected_counts);
            assert_eq!(counts, expected_counts);
            assert_eq!(stream_counts, expected_counts);
        }
    }
}
