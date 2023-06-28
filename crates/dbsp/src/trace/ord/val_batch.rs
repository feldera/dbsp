use crate::{
    algebra::{Lattice, MonoidValue},
    time::{Antichain, AntichainRef},
    trace::{
        layers::{
            column_layer::{ColumnLayer, ColumnLayerBuilder},
            ordered::{OrderedBuilder, OrderedCursor, OrderedLayer},
            Builder as TrieBuilder, Cursor as TrieCursor, MergeBuilder, OrdOffset, Trie,
            TupleBuilder,
        },
        ord::merge_batcher::MergeBatcher,
        Batch, BatchReader, Builder, Consumer, Cursor, Merger, ValueConsumer,
    },
    DBData, DBTimestamp, DBWeight, NumEntries,
};
use bincode::{Decode, Encode};
use rand::Rng;
use size_of::SizeOf;
use std::{
    fmt::{Debug, Display, Formatter},
    marker::PhantomData,
};

pub type OrdValBatchLayer<K, V, T, R, O> =
    OrderedLayer<K, OrderedLayer<V, ColumnLayer<T, R>, O>, O>;

/// An immutable collection of update tuples, from a contiguous interval of
/// logical times.
#[derive(Debug, Clone, SizeOf, Encode, Decode)]
pub struct OrdValBatch<K, V, T, R, O = usize>
where
    K: Ord + 'static,
    V: Ord + 'static,
    T: Lattice + 'static,
    R: 'static,
    O: OrdOffset,
{
    /// Where all the dataz is.
    pub layer: OrdValBatchLayer<K, V, T, R, O>,
    pub lower: Antichain<T>,
    pub upper: Antichain<T>,
}

impl<K, V, T, R, O> NumEntries for OrdValBatch<K, V, T, R, O>
where
    K: DBData,
    V: DBData,
    T: DBTimestamp,
    R: DBWeight,
    O: OrdOffset,
{
    const CONST_NUM_ENTRIES: Option<usize> = <OrdValBatchLayer<K, V, R, R, O>>::CONST_NUM_ENTRIES;

    #[inline]
    fn num_entries_shallow(&self) -> usize {
        self.layer.num_entries_shallow()
    }

    #[inline]
    fn num_entries_deep(&self) -> usize {
        self.layer.num_entries_deep()
    }
}

impl<K, V, T, R, O> Display for OrdValBatch<K, V, T, R, O>
where
    K: DBData,
    V: DBData,
    T: DBTimestamp,
    R: DBWeight,
    O: OrdOffset,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), std::fmt::Error> {
        writeln!(
            f,
            "lower: {:?}, upper: {:?}\nlayer:\n{}",
            self.lower,
            self.upper,
            textwrap::indent(&self.layer.to_string(), "    ")
        )
    }
}

impl<K, V, T, R, O> BatchReader for OrdValBatch<K, V, T, R, O>
where
    K: DBData,
    V: DBData,
    T: DBTimestamp,
    R: DBWeight,
    O: OrdOffset,
{
    type Key = K;
    type Val = V;
    type Time = T;
    type R = R;

    type Cursor<'s> = OrdValCursor<'s, K, V, T, R, O>
    where
        O: 's;

    type Consumer = OrdValConsumer<K, V, T, R, O>;

    fn cursor(&self) -> Self::Cursor<'_> {
        OrdValCursor {
            cursor: self.layer.cursor(),
        }
    }

    fn consumer(self) -> Self::Consumer {
        todo!()
    }

    fn key_count(&self) -> usize {
        <OrdValBatchLayer<K, V, T, R, O> as Trie>::keys(&self.layer)
    }

    fn len(&self) -> usize {
        <OrdValBatchLayer<K, V, T, R, O> as Trie>::tuples(&self.layer)
    }

    fn lower(&self) -> AntichainRef<'_, T> {
        self.lower.as_ref()
    }

    fn upper(&self) -> AntichainRef<'_, T> {
        self.upper.as_ref()
    }

    fn truncate_keys_below(&mut self, lower_bound: &Self::Key) {
        self.layer.truncate_keys_below(lower_bound);
    }

    fn sample_keys<RG>(&self, rng: &mut RG, sample_size: usize, sample: &mut Vec<Self::Key>)
    where
        RG: Rng,
    {
        self.layer.sample_keys(rng, sample_size, sample);
    }
}

impl<K, V, T, R, O> Batch for OrdValBatch<K, V, T, R, O>
where
    K: DBData,
    V: DBData,
    T: DBTimestamp,
    R: DBWeight,
    O: OrdOffset,
{
    type Item = (K, V);
    type Batcher = MergeBatcher<(K, V), T, R, Self>;
    type Builder = OrdValBuilder<K, V, T, R, O>;
    type Merger = OrdValMerger<K, V, T, R, O>;

    fn item_from(key: K, val: V) -> Self::Item {
        (key, val)
    }

    fn from_keys(time: Self::Time, keys: Vec<(Self::Key, Self::R)>) -> Self
    where
        Self::Val: From<()>,
    {
        Self::from_tuples(
            time,
            keys.into_iter()
                .map(|(k, w)| ((k, From::from(())), w))
                .collect(),
        )
    }

    fn begin_merge(&self, other: &Self) -> Self::Merger {
        OrdValMerger::new_merger(self, other)
    }

    fn recede_to(&mut self, frontier: &T) {
        // Nothing to do if the batch is entirely before the frontier.
        if !self.upper().less_equal(frontier) {
            self.do_recede_to(frontier);
        }
    }
}

impl<K, V, T, R, O> OrdValBatch<K, V, T, R, O>
where
    K: Ord + Clone + 'static,
    V: Ord + Clone + 'static,
    T: Lattice + Ord + Clone + 'static,
    R: MonoidValue,
    O: OrdOffset,
{
    fn do_recede_to(&mut self, frontier: &T) {
        // We have unique ownership of the batch, and can advance times in place.
        // We must still sort, collapse, and remove empty updates.

        // We will zip throught the time leaves, calling advance on each,
        //    then zip through the value layer, sorting and collapsing each,
        //    then zip through the key layer, collapsing each .. ?

        // 1. For each (time, diff) pair, advance the time.
        for time in self.layer.vals.vals.keys_mut() {
            time.meet_assign(frontier);
        }

        // 2. For each `(val, off)` pair, sort the range, compact, and rewrite `off`.
        //    This may leave `val` with an empty range; filtering happens in step 3.
        let mut write_position = 0;
        for i in 0..self.layer.vals.keys.len() {
            // NB: batch.layer.vals.offs[i+1] will be used next iteration, and should not be
            // changed.     we will change batch.layer.vals.offs[i] in this
            // iteration, from `write_position`'s     initial value.

            let lower: usize = self.layer.vals.offs[i].into_usize();
            let upper: usize = self.layer.vals.offs[i + 1].into_usize();

            self.layer.vals.offs[i] = O::from_usize(write_position);

            let (times, diffs) = &mut self.layer.vals.vals.columns_mut();

            // sort the range by the times (ignore the diffs; they will collapse).
            let count = crate::trace::consolidation::consolidate_paired_slices(
                &mut times[lower..upper],
                &mut diffs[lower..upper],
            );

            for index in lower..(lower + count) {
                times.swap(write_position, index);
                diffs.swap(write_position, index);
                write_position += 1;
            }
        }
        self.layer.vals.vals.truncate(write_position);
        self.layer.vals.offs[self.layer.vals.keys.len()] = O::from_usize(write_position);

        // 3. For each `(key, off)` pair, (values already sorted), filter vals, and
        // rewrite `off`.    This may leave `key` with an empty range. Filtering
        // happens in step 4.
        let mut write_position = 0;
        for i in 0..self.layer.keys.len() {
            // NB: batch.layer.offs[i+1] must remain as is for the next iteration.
            //     instead, we update batch.layer.offs[i]

            let lower: usize = self.layer.offs[i].into_usize();
            let upper: usize = self.layer.offs[i + 1].into_usize();

            self.layer.offs[i] = O::from_usize(write_position);

            // values should already be sorted, but some might now be empty.
            for index in lower..upper {
                let val_lower: usize = self.layer.vals.offs[index].into_usize();
                let val_upper: usize = self.layer.vals.offs[index + 1].into_usize();
                if val_lower < val_upper {
                    self.layer.vals.keys.swap(write_position, index);
                    self.layer.vals.offs[write_position + 1] = self.layer.vals.offs[index + 1];
                    write_position += 1;
                }
            }
            // batch.layer.offs[i+1] = write_position;
        }
        self.layer.vals.keys.truncate(write_position);
        self.layer.vals.offs.truncate(write_position + 1);
        self.layer.offs[self.layer.keys.len()] = O::from_usize(write_position);

        // 4. Remove empty keys.
        let mut write_position = 0;
        for i in 0..self.layer.keys.len() {
            let lower: usize = self.layer.offs[i].into_usize();
            let upper: usize = self.layer.offs[i + 1].into_usize();

            if lower < upper {
                self.layer.keys.swap(write_position, i);
                // batch.layer.offs updated via `dedup` below; keeps me sane.
                write_position += 1;
            }
        }
        self.layer.offs.dedup();
        self.layer.keys.truncate(write_position);
        self.layer.offs.truncate(write_position + 1);
    }
}

/// State for an in-progress merge.
#[derive(SizeOf)]
pub struct OrdValMerger<K, V, T, R, O = usize>
where
    K: Ord + Clone + 'static,
    V: Ord + Clone + 'static,
    T: Lattice + Ord + Clone + Debug + 'static,
    R: MonoidValue,
    O: OrdOffset,
{
    // first batch, and position therein.
    lower1: usize,
    upper1: usize,
    // second batch, and position therein.
    lower2: usize,
    upper2: usize,
    // result that we are currently assembling.
    result: <OrdValBatchLayer<K, V, T, R, O> as Trie>::MergeBuilder,
    lower: Antichain<T>,
    upper: Antichain<T>,
}

impl<K, V, T, R, O> Merger<K, V, T, R, OrdValBatch<K, V, T, R, O>> for OrdValMerger<K, V, T, R, O>
where
    Self: SizeOf,
    K: DBData,
    V: DBData,
    T: DBTimestamp,
    R: DBWeight,
    O: OrdOffset,
{
    fn new_merger(
        batch1: &OrdValBatch<K, V, T, R, O>,
        batch2: &OrdValBatch<K, V, T, R, O>,
    ) -> Self {
        // Leonid: we do not require batch bounds to grow monotonically.
        // assert!(batch1.upper() == batch2.lower());

        OrdValMerger {
            lower1: batch1.layer.lower_bound(),
            upper1: batch1.layer.lower_bound() + batch1.layer.keys(),
            lower2: batch2.layer.lower_bound(),
            upper2: batch2.layer.lower_bound() + batch2.layer.keys(),
            result: <<OrdValBatchLayer<K, V, T, R, O> as Trie>::MergeBuilder as MergeBuilder>::with_capacity(&batch1.layer, &batch2.layer),
            lower: batch1.lower().meet(batch2.lower()),
            upper: batch1.upper().join(batch2.upper()),
        }
    }

    fn done(self) -> OrdValBatch<K, V, T, R, O> {
        assert!(self.lower1 == self.upper1);
        assert!(self.lower2 == self.upper2);

        OrdValBatch {
            layer: self.result.done(),
            lower: self.lower,
            upper: self.upper,
        }
    }

    fn work(
        &mut self,
        source1: &OrdValBatch<K, V, T, R, O>,
        source2: &OrdValBatch<K, V, T, R, O>,
        lower_val_bound: &Option<V>,
        fuel: &mut isize,
    ) {
        // Use the more expensive `push_merge_truncate_values_fueled`
        // method if we need to remove truncated values during merging.
        if let Some(bound) = lower_val_bound {
            self.result.push_merge_truncate_values_fueled(
                (&source1.layer, &mut self.lower1, self.upper1),
                (&source2.layer, &mut self.lower2, self.upper2),
                bound,
                fuel,
            );
        } else {
            self.result.push_merge_fueled(
                (&source1.layer, &mut self.lower1, self.upper1),
                (&source2.layer, &mut self.lower2, self.upper2),
                fuel,
            );
        }
    }
}

/// A cursor for navigating a single layer.
#[derive(Debug, SizeOf)]
pub struct OrdValCursor<'s, K, V, T, R, O = usize>
where
    K: Ord + Clone + 'static,
    V: Ord + Clone + 'static,
    T: Lattice + Ord + Clone + 'static,
    R: MonoidValue,
    O: OrdOffset,
{
    cursor: OrderedCursor<'s, K, O, OrderedLayer<V, ColumnLayer<T, R>, O>>,
}

impl<'s, K, V, T, R, O> Cursor<K, V, T, R> for OrdValCursor<'s, K, V, T, R, O>
where
    K: Ord + Clone,
    V: Ord + Clone,
    T: Lattice + Ord + Clone,
    R: MonoidValue,
    O: OrdOffset,
{
    fn key(&self) -> &K {
        self.cursor.item()
    }

    fn val(&self) -> &V {
        self.cursor.child.item()
    }

    fn fold_times<F, U>(&mut self, mut init: U, mut fold: F) -> U
    where
        F: FnMut(U, &T, &R) -> U,
    {
        self.cursor.child.child.rewind();
        while self.cursor.child.child.valid() {
            init = fold(
                init,
                self.cursor.child.child.current_key(),
                self.cursor.child.child.current_diff(),
            );
            self.cursor.child.child.step();
        }

        init
    }

    fn fold_times_through<F, U>(&mut self, upper: &T, mut init: U, mut fold: F) -> U
    where
        F: FnMut(U, &T, &R) -> U,
    {
        self.cursor.child.child.rewind();
        while self.cursor.child.child.valid() {
            if self.cursor.child.child.item().0.less_equal(upper) {
                init = fold(
                    init,
                    self.cursor.child.child.current_key(),
                    self.cursor.child.child.current_diff(),
                );
            }
            self.cursor.child.child.step();
        }

        init
    }

    fn weight(&mut self) -> R
    where
        T: PartialEq<()>,
    {
        debug_assert!(self.cursor.child.child.valid());
        self.cursor.child.child.item().1.clone()
    }

    fn key_valid(&self) -> bool {
        self.cursor.valid()
    }
    fn val_valid(&self) -> bool {
        self.cursor.child.valid()
    }
    fn step_key(&mut self) {
        self.cursor.step();
    }
    fn step_key_reverse(&mut self) {
        self.cursor.step_reverse();
    }
    fn seek_key(&mut self, key: &K) {
        self.cursor.seek(key);
    }
    fn seek_key_with<P>(&mut self, predicate: P)
    where
        P: Fn(&K) -> bool + Clone,
    {
        self.cursor.seek_with(|k| !predicate(k));
    }
    fn seek_key_with_reverse<P>(&mut self, predicate: P)
    where
        P: Fn(&K) -> bool + Clone,
    {
        self.cursor.seek_with_reverse(|k| !predicate(k));
    }
    fn seek_key_reverse(&mut self, key: &K) {
        self.cursor.seek_reverse(key);
    }
    fn step_val(&mut self) {
        self.cursor.child.step();
    }
    fn seek_val(&mut self, val: &V) {
        self.cursor.child.seek(val);
    }
    fn seek_val_with<P>(&mut self, predicate: P)
    where
        P: Fn(&V) -> bool + Clone,
    {
        self.cursor.child.seek_with(|v| !predicate(v));
    }
    fn rewind_keys(&mut self) {
        self.cursor.rewind();
    }
    fn fast_forward_keys(&mut self) {
        self.cursor.fast_forward();
    }
    fn rewind_vals(&mut self) {
        self.cursor.child.rewind();
    }

    fn step_val_reverse(&mut self) {
        self.cursor.child.step_reverse();
    }

    fn seek_val_reverse(&mut self, val: &V) {
        self.cursor.child.seek_reverse(val);
    }

    fn seek_val_with_reverse<P>(&mut self, predicate: P)
    where
        P: Fn(&V) -> bool + Clone,
    {
        self.cursor.child.seek_with_reverse(|v| !predicate(v));
    }

    fn fast_forward_vals(&mut self) {
        self.cursor.child.fast_forward();
    }
}

type RawOrdValBuilder<K, V, T, R, O> =
    OrderedBuilder<K, OrderedBuilder<V, ColumnLayerBuilder<T, R>, O>, O>;

/// A builder for creating layers from unsorted update tuples.
#[derive(SizeOf)]
pub struct OrdValBuilder<K, V, T, R, O = usize>
where
    K: Ord,
    V: Ord,
    T: Ord + Lattice,
    R: MonoidValue,
    O: OrdOffset,
{
    time: T,
    builder: RawOrdValBuilder<K, V, T, R, O>,
}

impl<K, V, T, R, O> Builder<(K, V), T, R, OrdValBatch<K, V, T, R, O>>
    for OrdValBuilder<K, V, T, R, O>
where
    Self: SizeOf,
    K: DBData,
    V: DBData,
    T: DBTimestamp,
    R: DBWeight,
    O: OrdOffset,
{
    #[inline]
    fn new_builder(time: T) -> Self {
        Self {
            time,
            builder: RawOrdValBuilder::<K, V, T, R, O>::new(),
        }
    }

    #[inline]
    fn with_capacity(time: T, cap: usize) -> Self {
        Self {
            time,
            builder: <RawOrdValBuilder<K, V, T, R, O> as TupleBuilder>::with_capacity(cap),
        }
    }

    #[inline]
    fn reserve(&mut self, additional: usize) {
        self.builder.reserve(additional);
    }

    #[inline]
    fn push(&mut self, ((key, val), diff): ((K, V), R)) {
        self.builder
            .push_tuple((key, (val, (self.time.clone(), diff))));
    }

    #[inline(never)]
    fn done(self) -> OrdValBatch<K, V, T, R, O> {
        let time_next = self.time.advance(0);
        let upper = if time_next <= self.time {
            Antichain::new()
        } else {
            Antichain::from_elem(time_next)
        };
        OrdValBatch {
            layer: self.builder.done(),
            lower: Antichain::from_elem(self.time),
            upper,
        }
    }
}

pub struct OrdValConsumer<K, V, T, R, O> {
    __type: PhantomData<(K, V, T, R, O)>,
}

impl<K, V, T, R, O> Consumer<K, V, R, T> for OrdValConsumer<K, V, T, R, O> {
    type ValueConsumer<'a> = OrdValValueConsumer<'a, K, V, T, R, O>
    where
        Self: 'a;

    fn key_valid(&self) -> bool {
        todo!()
    }

    fn peek_key(&self) -> &K {
        todo!()
    }

    fn next_key(&mut self) -> (K, Self::ValueConsumer<'_>) {
        todo!()
    }

    fn seek_key(&mut self, _key: &K)
    where
        K: Ord,
    {
        todo!()
    }
}

pub struct OrdValValueConsumer<'a, K, V, T, R, O> {
    __type: PhantomData<&'a (K, V, T, R, O)>,
}

impl<'a, K, V, T, R, O> ValueConsumer<'a, V, R, T> for OrdValValueConsumer<'a, K, V, T, R, O> {
    fn value_valid(&self) -> bool {
        todo!()
    }

    fn next_value(&mut self) -> (V, R, T) {
        todo!()
    }

    fn remaining_values(&self) -> usize {
        todo!()
    }
}
