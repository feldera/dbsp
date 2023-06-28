use crate::{
    algebra::{AddAssignByRef, AddByRef, HasZero, MonoidValue, NegByRef},
    time::AntichainRef,
    trace::{
        consolidation::consolidate_payload_from,
        layers::{
            column_layer::{
                ColumnLayer, ColumnLayerBuilder, ColumnLayerConsumer, ColumnLayerCursor,
                ColumnLayerValues,
            },
            Builder as TrieBuilder, Cursor as TrieCursor, MergeBuilder, Trie, TupleBuilder,
        },
        ord::merge_batcher::MergeBatcher,
        Batch, BatchReader, Builder, Consumer, Cursor, Merger, ValueConsumer,
    },
    DBData, DBWeight, NumEntries,
};
use bincode::{Decode, Encode};
use rand::Rng;
use size_of::SizeOf;
use std::{
    cmp::max,
    fmt::{self, Debug, Display},
    ops::{Add, AddAssign, Neg},
    rc::Rc,
};

/// An immutable collection of `(key, weight)` pairs without timing information.
#[derive(Debug, Clone, Eq, PartialEq, Encode, Decode, SizeOf)]
pub struct OrdZSet<K, R>
where
    K: 'static,
    R: 'static,
{
    #[doc(hidden)]
    pub layer: ColumnLayer<K, R>,
}

impl<K, R> OrdZSet<K, R> {
    #[inline]
    pub fn len(&self) -> usize {
        self.layer.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.layer.is_empty()
    }

    #[inline]
    pub fn retain<F>(&mut self, retain: F)
    where
        F: FnMut(&K, &R) -> bool,
    {
        self.layer.retain(retain);
    }

    #[doc(hidden)]
    #[inline]
    pub fn from_columns(mut keys: Vec<K>, mut diffs: Vec<R>) -> Self
    where
        K: Ord,
        R: HasZero + AddAssign,
    {
        consolidate_payload_from(&mut keys, &mut diffs, 0);

        Self {
            // Safety: We've ensured that keys and diffs are the same length
            // and are sorted & consolidated
            layer: unsafe { ColumnLayer::from_parts(keys, diffs, 0) },
        }
    }
}

impl<K, R> Display for OrdZSet<K, R>
where
    K: DBData,
    R: DBWeight,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "layer:\n{}",
            textwrap::indent(&self.layer.to_string(), "    ")
        )
    }
}

impl<K, R> From<ColumnLayer<K, R>> for OrdZSet<K, R> {
    fn from(layer: ColumnLayer<K, R>) -> Self {
        Self { layer }
    }
}

impl<K, R> From<ColumnLayer<K, R>> for Rc<OrdZSet<K, R>> {
    fn from(layer: ColumnLayer<K, R>) -> Self {
        Rc::new(From::from(layer))
    }
}

impl<K, R> NumEntries for OrdZSet<K, R>
where
    K: DBData,
    R: DBWeight,
{
    const CONST_NUM_ENTRIES: Option<usize> = <ColumnLayer<K, R>>::CONST_NUM_ENTRIES;

    fn num_entries_shallow(&self) -> usize {
        self.layer.num_entries_shallow()
    }

    fn num_entries_deep(&self) -> usize {
        self.layer.num_entries_deep()
    }
}

impl<K, R> Default for OrdZSet<K, R> {
    fn default() -> Self {
        Self {
            layer: ColumnLayer::empty(),
        }
    }
}

impl<K, R> NegByRef for OrdZSet<K, R>
where
    K: DBData,
    R: MonoidValue + NegByRef,
{
    fn neg_by_ref(&self) -> Self {
        Self {
            layer: self.layer.neg_by_ref(),
        }
    }
}

impl<K, R> Neg for OrdZSet<K, R>
where
    K: DBData,
    R: MonoidValue + Neg<Output = R>,
{
    type Output = Self;

    fn neg(self) -> Self {
        Self {
            layer: self.layer.neg(),
        }
    }
}

// TODO: by-value merge
impl<K, R> Add<Self> for OrdZSet<K, R>
where
    K: DBData,
    R: MonoidValue,
{
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self {
            layer: self.layer.add(rhs.layer),
        }
    }
}

impl<K, R> AddAssign<Self> for OrdZSet<K, R>
where
    K: DBData,
    R: MonoidValue,
{
    fn add_assign(&mut self, rhs: Self) {
        self.layer.add_assign(rhs.layer);
    }
}

impl<K, R> AddAssignByRef for OrdZSet<K, R>
where
    K: DBData,
    R: MonoidValue,
{
    fn add_assign_by_ref(&mut self, rhs: &Self) {
        self.layer.add_assign_by_ref(&rhs.layer);
    }
}

impl<K, R> AddByRef for OrdZSet<K, R>
where
    K: DBData,
    R: MonoidValue,
{
    fn add_by_ref(&self, rhs: &Self) -> Self {
        Self {
            layer: self.layer.add_by_ref(&rhs.layer),
        }
    }
}

impl<K, R> BatchReader for OrdZSet<K, R>
where
    K: DBData,
    R: DBWeight,
{
    type Key = K;
    type Val = ();
    type Time = ();
    type R = R;
    type Cursor<'s> = OrdZSetCursor<'s, K, R>;
    type Consumer = OrdZSetConsumer<K, R>;

    #[inline]
    fn cursor(&self) -> Self::Cursor<'_> {
        OrdZSetCursor {
            valid: true,
            cursor: self.layer.cursor(),
        }
    }

    #[inline]
    fn consumer(self) -> Self::Consumer {
        OrdZSetConsumer {
            consumer: ColumnLayerConsumer::from(self.layer),
        }
    }

    #[inline]
    fn key_count(&self) -> usize {
        Trie::keys(&self.layer)
    }

    #[inline]
    fn len(&self) -> usize {
        self.layer.tuples()
    }

    #[inline]
    fn lower(&self) -> AntichainRef<'_, ()> {
        AntichainRef::new(&[()])
    }

    #[inline]
    fn upper(&self) -> AntichainRef<'_, ()> {
        AntichainRef::empty()
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

impl<K, R> Batch for OrdZSet<K, R>
where
    K: DBData,
    R: DBWeight,
{
    type Item = K;
    type Batcher = MergeBatcher<K, (), R, Self>;
    type Builder = OrdZSetBuilder<K, R>;
    type Merger = OrdZSetMerger<K, R>;

    fn item_from(key: K, _val: ()) -> Self::Item {
        key
    }

    fn from_keys(time: Self::Time, keys: Vec<(Self::Key, Self::R)>) -> Self {
        Self::from_tuples(time, keys)
    }

    fn begin_merge(&self, other: &Self) -> Self::Merger {
        OrdZSetMerger::new_merger(self, other)
    }

    fn recede_to(&mut self, _frontier: &()) {}

    fn empty(_time: Self::Time) -> Self {
        Self {
            layer: ColumnLayer::empty(),
        }
    }
}

/// State for an in-progress merge.
#[derive(SizeOf)]
pub struct OrdZSetMerger<K, R>
where
    K: DBData,
    R: DBWeight,
{
    // result that we are currently assembling.
    result: <ColumnLayer<K, R> as Trie>::MergeBuilder,
}

impl<K, R> Merger<K, (), (), R, OrdZSet<K, R>> for OrdZSetMerger<K, R>
where
    Self: SizeOf,
    K: DBData,
    R: DBWeight,
{
    fn new_merger(batch1: &OrdZSet<K, R>, batch2: &OrdZSet<K, R>) -> Self {
        Self {
            result: <<ColumnLayer<K, R> as Trie>::MergeBuilder as MergeBuilder>::with_capacity(
                &batch1.layer,
                &batch2.layer,
            ),
        }
    }

    fn done(self) -> OrdZSet<K, R> {
        OrdZSet {
            layer: self.result.done(),
        }
    }

    fn work(
        &mut self,
        source1: &OrdZSet<K, R>,
        source2: &OrdZSet<K, R>,
        _lower_val_bound: &Option<()>,
        fuel: &mut isize,
    ) {
        *fuel -= self
            .result
            .push_merge(source1.layer.cursor(), source2.layer.cursor()) as isize;
        *fuel = max(*fuel, 1);
    }
}

/// A cursor for navigating a single layer.
#[derive(Debug, SizeOf)]
pub struct OrdZSetCursor<'s, K, R>
where
    K: DBData,
    R: DBWeight,
{
    valid: bool,
    cursor: ColumnLayerCursor<'s, K, R>,
}

impl<'s, K, R> Cursor<K, (), (), R> for OrdZSetCursor<'s, K, R>
where
    K: DBData,
    R: DBWeight,
{
    fn key(&self) -> &K {
        self.cursor.current_key()
    }

    fn val(&self) -> &() {
        &()
    }

    fn fold_times<F, U>(&mut self, init: U, mut fold: F) -> U
    where
        F: FnMut(U, &(), &R) -> U,
    {
        if self.cursor.valid() {
            fold(init, &(), self.cursor.current_diff())
        } else {
            init
        }
    }

    fn fold_times_through<F, U>(&mut self, _upper: &(), init: U, fold: F) -> U
    where
        F: FnMut(U, &(), &R) -> U,
    {
        self.fold_times(init, fold)
    }

    fn weight(&mut self) -> R {
        debug_assert!(&self.cursor.valid());
        self.cursor.current_diff().clone()
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
        self.cursor.seek_key_with(|k| !predicate(k));
        self.valid = true;
    }

    fn seek_key_with_reverse<P>(&mut self, predicate: P)
    where
        P: Fn(&K) -> bool + Clone,
    {
        self.cursor.seek_key_with_reverse(|k| !predicate(k));
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

/// A builder for creating layers from unsorted update tuples.
#[derive(SizeOf)]
pub struct OrdZSetBuilder<K, R>
where
    K: Ord,
    R: DBWeight,
{
    builder: ColumnLayerBuilder<K, R>,
}

impl<K, R> Builder<K, (), R, OrdZSet<K, R>> for OrdZSetBuilder<K, R>
where
    Self: SizeOf,
    K: DBData,
    R: DBWeight,
{
    #[inline]
    fn new_builder(_time: ()) -> Self {
        Self {
            builder: ColumnLayerBuilder::new(),
        }
    }

    #[inline]
    fn with_capacity(_time: (), capacity: usize) -> Self {
        Self {
            builder: <ColumnLayerBuilder<K, R> as TupleBuilder>::with_capacity(capacity),
        }
    }

    #[inline]
    fn reserve(&mut self, additional: usize) {
        self.builder.reserve(additional);
    }

    #[inline]
    fn push(&mut self, (key, diff): (K, R)) {
        self.builder.push_tuple((key, diff));
    }

    #[inline(never)]
    fn done(self) -> OrdZSet<K, R> {
        OrdZSet {
            layer: self.builder.done(),
        }
    }
}

#[derive(Debug, SizeOf)]
pub struct OrdZSetConsumer<K, R>
where
    K: 'static,
    R: 'static,
{
    consumer: ColumnLayerConsumer<K, R>,
}

impl<K, R> Consumer<K, (), R, ()> for OrdZSetConsumer<K, R> {
    type ValueConsumer<'a> = OrdZSetValueConsumer<'a, K, R>
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
        (key, OrdZSetValueConsumer { values })
    }

    fn seek_key(&mut self, key: &K)
    where
        K: Ord,
    {
        self.consumer.seek_key(key);
    }
}

#[derive(Debug)]
pub struct OrdZSetValueConsumer<'a, K, R>
where
    K: 'static,
    R: 'static,
{
    values: ColumnLayerValues<'a, K, R>,
}

impl<'a, K, R> ValueConsumer<'a, (), R, ()> for OrdZSetValueConsumer<'a, K, R> {
    fn value_valid(&self) -> bool {
        self.values.value_valid()
    }

    fn next_value(&mut self) -> ((), R, ()) {
        self.values.next_value()
    }

    fn remaining_values(&self) -> usize {
        self.values.remaining_values()
    }
}
