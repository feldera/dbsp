use crate::{
    algebra::{Lattice, MonoidValue},
    time::{Antichain, AntichainRef},
    trace::{
        layers::{
            column_layer::{ColumnLayer, ColumnLayerBuilder},
            ordered::{
                OrderedBuilder, OrderedCursor, OrderedLayer, OrderedLayerConsumer,
                OrderedLayerValues,
            },
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
    fmt::{self, Debug, Display},
    marker::PhantomData,
};

pub type OrdKeyBatchLayer<K, T, R, O> = OrderedLayer<K, ColumnLayer<T, R>, O>;

/// An immutable collection of update tuples, from a contiguous interval of
/// logical times.
#[derive(Debug, Clone, Encode, Decode, SizeOf)]
pub struct OrdKeyBatch<K, T, R, O = usize>
where
    K: 'static,
    T: 'static,
    R: 'static,
    O: 'static,
{
    /// Where all the dataz is.
    pub layer: OrdKeyBatchLayer<K, T, R, O>,
    pub lower: Antichain<T>,
    pub upper: Antichain<T>,
}

impl<K, T, R, O> Display for OrdKeyBatch<K, T, R, O>
where
    K: DBData,
    T: DBTimestamp,
    R: DBWeight,
    O: OrdOffset,
    OrdKeyBatchLayer<K, T, R, O>: Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "layer:\n{}",
            textwrap::indent(&self.layer.to_string(), "    ")
        )
    }
}

impl<K, T, R, O> NumEntries for OrdKeyBatch<K, T, R, O>
where
    K: DBData,
    T: DBTimestamp,
    R: DBWeight,
    O: OrdOffset,
{
    const CONST_NUM_ENTRIES: Option<usize> = <OrdKeyBatchLayer<K, T, R, O>>::CONST_NUM_ENTRIES;

    #[inline]
    fn num_entries_shallow(&self) -> usize {
        self.layer.num_entries_shallow()
    }

    #[inline]
    fn num_entries_deep(&self) -> usize {
        self.layer.num_entries_deep()
    }
}

impl<K, T, R, O> BatchReader for OrdKeyBatch<K, T, R, O>
where
    K: DBData,
    T: DBTimestamp,
    R: DBWeight,
    O: OrdOffset,
{
    type Key = K;
    type Val = ();
    type Time = T;
    type R = R;
    type Cursor<'s> = OrdKeyCursor<'s, K, T, R, O> where O: 's;
    type Consumer = OrdKeyConsumer<K, T, R, O>;

    fn cursor(&self) -> Self::Cursor<'_> {
        OrdKeyCursor {
            valid: true,
            cursor: self.layer.cursor(),
        }
    }

    fn consumer(self) -> Self::Consumer {
        todo!()
    }

    fn key_count(&self) -> usize {
        <OrdKeyBatchLayer<K, T, R, O> as Trie>::keys(&self.layer)
    }

    fn len(&self) -> usize {
        <OrdKeyBatchLayer<K, T, R, O> as Trie>::tuples(&self.layer)
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

impl<K, T, R, O> Batch for OrdKeyBatch<K, T, R, O>
where
    K: DBData,
    T: DBTimestamp,
    R: DBWeight,
    O: OrdOffset,
{
    type Item = K;
    type Batcher = MergeBatcher<K, T, R, Self>;
    type Builder = OrdKeyBuilder<K, T, R, O>;
    type Merger = OrdKeyMerger<K, T, R, O>;

    fn item_from(key: K, _val: ()) -> Self::Item {
        key
    }

    fn from_keys(time: Self::Time, keys: Vec<(Self::Key, Self::R)>) -> Self {
        Self::from_tuples(time, keys)
    }

    fn begin_merge(&self, other: &Self) -> Self::Merger {
        Self::Merger::new_merger(self, other)
    }

    fn recede_to(&mut self, frontier: &T) {
        // Nothing to do if the batch is entirely before the frontier.
        if !self.upper().less_equal(frontier) {
            // TODO: Optimize case where self.upper()==self.lower().
            self.do_recede_to(frontier);
        }
    }
}

impl<K, T, R, O> OrdKeyBatch<K, T, R, O>
where
    K: Ord + Clone + 'static,
    T: Lattice + Ord + Clone + 'static,
    R: MonoidValue,
    O: OrdOffset,
{
    fn do_recede_to(&mut self, frontier: &T) {
        // We will zip through the time leaves, calling advance on each,
        //    then zip through the value layer, sorting and collapsing each,
        //    then zip through the key layer, collapsing each .. ?

        // 1. For each (time, diff) pair, advance the time.
        for time in self.layer.vals.keys_mut() {
            time.meet_assign(frontier);
        }
        // for time_diff in self.layer.vals.vals.iter_mut() {
        //     time_diff.0 = time_diff.0.advance_by(frontier);
        // }

        // 2. For each `(val, off)` pair, sort the range, compact, and rewrite `off`.
        //    This may leave `val` with an empty range; filtering happens in step 3.
        let mut write_position = 0;
        for i in 0..self.layer.keys.len() {
            // NB: batch.layer.vals.offs[i+1] will be used next iteration, and should not be
            // changed.     we will change batch.layer.vals.offs[i] in this
            // iteration, from `write_position`'s     initial value.

            let lower: usize = self.layer.offs[i].into_usize();
            let upper: usize = self.layer.offs[i + 1].into_usize();

            self.layer.offs[i] = O::from_usize(write_position);

            let (times, diffs) = self.layer.vals.columns_mut();

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
        self.layer.vals.truncate(write_position);
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
pub struct OrdKeyMerger<K, T, R, O = usize>
where
    K: Ord + Clone + 'static,
    T: Lattice + Ord + Clone + 'static,
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
    result: <OrdKeyBatchLayer<K, T, R, O> as Trie>::MergeBuilder,
    lower: Antichain<T>,
    upper: Antichain<T>,
}

impl<K, T, R, O> Merger<K, (), T, R, OrdKeyBatch<K, T, R, O>> for OrdKeyMerger<K, T, R, O>
where
    Self: SizeOf,
    K: DBData,
    T: DBTimestamp,
    R: DBWeight,
    O: OrdOffset,
{
    fn new_merger(batch1: &OrdKeyBatch<K, T, R, O>, batch2: &OrdKeyBatch<K, T, R, O>) -> Self {
        // Leonid: we do not require batch bounds to grow monotonically.
        //assert!(batch1.upper() == batch2.lower());

        OrdKeyMerger {
            lower1: batch1.layer.lower_bound(),
            upper1: batch1.layer.lower_bound() + batch1.layer.keys(),
            lower2: batch2.layer.lower_bound(),
            upper2: batch2.layer.lower_bound() + batch2.layer.keys(),
            result: <<OrdKeyBatchLayer<K, T, R, O> as Trie>::MergeBuilder as MergeBuilder>::with_capacity(&batch1.layer, &batch2.layer),
            lower: batch1.lower().meet(batch2.lower()),
            upper: batch1.upper().join(batch2.upper()),
        }
    }

    fn done(self) -> OrdKeyBatch<K, T, R, O> {
        assert!(self.lower1 == self.upper1);
        assert!(self.lower2 == self.upper2);

        OrdKeyBatch {
            layer: self.result.done(),
            lower: self.lower,
            upper: self.upper,
        }
    }

    fn work(
        &mut self,
        source1: &OrdKeyBatch<K, T, R, O>,
        source2: &OrdKeyBatch<K, T, R, O>,
        _lower_val_bound: &Option<()>,
        fuel: &mut isize,
    ) {
        self.result.push_merge_fueled(
            (&source1.layer, &mut self.lower1, self.upper1),
            (&source2.layer, &mut self.lower2, self.upper2),
            fuel,
        );
    }
}

/// A cursor for navigating a single layer.
#[derive(Debug, SizeOf)]
pub struct OrdKeyCursor<'s, K, T, R, O = usize>
where
    O: OrdOffset,
    K: Ord + Clone + 'static,
    T: Lattice + Ord + Clone + 'static,
    R: MonoidValue,
{
    valid: bool,
    cursor: OrderedCursor<'s, K, O, ColumnLayer<T, R>>,
}

impl<'s, K, T, R, O> Cursor<K, (), T, R> for OrdKeyCursor<'s, K, T, R, O>
where
    K: Ord + Clone,
    T: Lattice + Ord + Clone,
    R: MonoidValue,
    O: OrdOffset,
{
    fn key(&self) -> &K {
        self.cursor.item()
    }

    fn val(&self) -> &() {
        &()
    }

    fn fold_times<F, U>(&mut self, mut init: U, mut fold: F) -> U
    where
        F: FnMut(U, &T, &R) -> U,
    {
        self.cursor.child.rewind();
        while self.cursor.child.valid() {
            init = fold(
                init,
                self.cursor.child.current_key(),
                self.cursor.child.current_diff(),
            );
            self.cursor.child.step();
        }

        init
    }

    fn fold_times_through<F, U>(&mut self, upper: &T, mut init: U, mut fold: F) -> U
    where
        F: FnMut(U, &T, &R) -> U,
    {
        self.cursor.child.rewind();
        while self.cursor.child.valid() {
            if self.cursor.child.item().0.less_equal(upper) {
                init = fold(
                    init,
                    self.cursor.child.current_key(),
                    self.cursor.child.current_diff(),
                );
            }
            self.cursor.child.step();
        }

        init
    }

    fn weight(&mut self) -> R
    where
        T: PartialEq<()>,
    {
        debug_assert!(self.cursor.child.valid());
        self.cursor.child.item().1.clone()
    }

    fn key_valid(&self) -> bool {
        self.cursor.valid()
    }

    fn val_valid(&self) -> bool {
        self.valid
    }

    fn step_key(&mut self) {
        self.cursor.step();
        self.valid = true;
    }

    fn step_key_reverse(&mut self) {
        self.cursor.step_reverse();
        self.valid = true;
    }

    fn seek_key(&mut self, key: &K) {
        self.cursor.seek(key);
        self.valid = true;
    }

    fn seek_key_with<P>(&mut self, predicate: P)
    where
        P: Fn(&K) -> bool + Clone,
    {
        self.cursor.seek_with(|k| !predicate(k));
        self.valid = true;
    }

    fn seek_key_with_reverse<P>(&mut self, predicate: P)
    where
        P: Fn(&K) -> bool + Clone,
    {
        self.cursor.seek_with_reverse(|k| !predicate(k));
        self.valid = true;
    }

    fn seek_key_reverse(&mut self, key: &K) {
        self.cursor.seek_reverse(key);
        self.valid = true;
    }

    fn step_val(&mut self) {
        self.valid = false;
    }

    fn seek_val(&mut self, _val: &()) {}
    fn seek_val_with<P>(&mut self, predicate: P)
    where
        P: Fn(&()) -> bool + Clone,
    {
        if !predicate(&()) {
            self.valid = false;
        }
    }

    fn rewind_keys(&mut self) {
        self.cursor.rewind();
        self.valid = true;
    }

    fn fast_forward_keys(&mut self) {
        self.cursor.fast_forward();
        self.valid = true;
    }

    fn rewind_vals(&mut self) {
        self.valid = true;
    }

    fn step_val_reverse(&mut self) {
        self.valid = false;
    }

    fn seek_val_reverse(&mut self, _val: &()) {}

    fn seek_val_with_reverse<P>(&mut self, predicate: P)
    where
        P: Fn(&()) -> bool + Clone,
    {
        if !predicate(&()) {
            self.valid = false;
        }
    }

    fn fast_forward_vals(&mut self) {
        self.valid = true;
    }
}

type RawOrdKeyBuilder<K, T, R, O> = OrderedBuilder<K, ColumnLayerBuilder<T, R>, O>;

/// A builder for creating layers from unsorted update tuples.
#[derive(SizeOf)]
pub struct OrdKeyBuilder<K, T, R, O = usize>
where
    K: Ord,
    T: Ord + Lattice,
    R: MonoidValue,
    O: OrdOffset,
{
    time: T,
    builder: RawOrdKeyBuilder<K, T, R, O>,
}

impl<K, T, R, O> Builder<K, T, R, OrdKeyBatch<K, T, R, O>> for OrdKeyBuilder<K, T, R, O>
where
    Self: SizeOf,
    K: DBData,
    T: DBTimestamp,
    R: DBWeight,
    O: OrdOffset,
{
    #[inline]
    fn new_builder(time: T) -> Self {
        Self {
            time,
            builder: <RawOrdKeyBuilder<K, T, R, O> as TupleBuilder>::new(),
        }
    }

    #[inline]
    fn with_capacity(time: T, cap: usize) -> Self {
        Self {
            time,
            builder: <RawOrdKeyBuilder<K, T, R, O> as TupleBuilder>::with_capacity(cap),
        }
    }

    #[inline]
    fn reserve(&mut self, additional: usize) {
        self.builder.reserve(additional);
    }

    #[inline]
    fn push(&mut self, (key, diff): (K, R)) {
        self.builder.push_tuple((key, (self.time.clone(), diff)));
    }

    #[inline(never)]
    fn done(self) -> OrdKeyBatch<K, T, R, O> {
        let time_next = self.time.advance(0);
        let upper = if time_next <= self.time {
            Antichain::new()
        } else {
            Antichain::from_elem(time_next)
        };

        OrdKeyBatch {
            layer: self.builder.done(),
            lower: Antichain::from_elem(self.time),
            upper,
        }
    }
}

pub struct OrdKeyConsumer<K, T, R, O>
where
    K: 'static,
    T: 'static,
    R: 'static,
    O: OrdOffset,
{
    consumer: OrderedLayerConsumer<K, T, R, O>,
}

impl<K, T, R, O> Consumer<K, (), R, T> for OrdKeyConsumer<K, T, R, O>
where
    O: OrdOffset,
{
    type ValueConsumer<'a> = OrdKeyValueConsumer<'a, K, T, R, O>
    where
        Self: 'a;

    fn key_valid(&self) -> bool {
        self.consumer.key_valid()
    }

    fn peek_key(&self) -> &K {
        self.consumer.peek_key()
    }

    fn next_key(&mut self) -> (K, Self::ValueConsumer<'_>) {
        let (key, values) = self.consumer.next_key();
        (key, OrdKeyValueConsumer::new(values))
    }

    fn seek_key(&mut self, key: &K)
    where
        K: Ord,
    {
        self.consumer.seek_key(key);
    }
}

pub struct OrdKeyValueConsumer<'a, K, T, R, O>
where
    T: 'static,
    R: 'static,
{
    consumer: OrderedLayerValues<'a, T, R>,
    __type: PhantomData<(K, O)>,
}

impl<'a, K, T, R, O> OrdKeyValueConsumer<'a, K, T, R, O> {
    const fn new(consumer: OrderedLayerValues<'a, T, R>) -> Self {
        Self {
            consumer,
            __type: PhantomData,
        }
    }
}

impl<'a, K, T, R, O> ValueConsumer<'a, (), R, T> for OrdKeyValueConsumer<'a, K, T, R, O> {
    fn value_valid(&self) -> bool {
        self.consumer.value_valid()
    }

    fn next_value(&mut self) -> ((), R, T) {
        let (time, diff, ()) = self.consumer.next_value();
        ((), diff, time)
    }

    fn remaining_values(&self) -> usize {
        self.consumer.remaining_values()
    }
}
