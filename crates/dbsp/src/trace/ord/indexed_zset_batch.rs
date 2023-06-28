use crate::{
    algebra::{AddAssignByRef, AddByRef, MonoidValue, NegByRef},
    time::AntichainRef,
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
    DBData, DBWeight, NumEntries,
};
use bincode::{Decode, Encode};
use rand::Rng;
use size_of::SizeOf;
use std::{
    fmt::{self, Debug, Display},
    marker::PhantomData,
    ops::{Add, AddAssign, Neg},
    rc::Rc,
};

type Layers<K, V, R, O> = OrderedLayer<K, ColumnLayer<V, R>, O>;

/// An immutable collection of update tuples.
#[derive(Debug, Clone, Eq, PartialEq, Encode, Decode, SizeOf)]
pub struct OrdIndexedZSet<K, V, R, O = usize>
where
    K: Ord + 'static,
    V: Ord + 'static,
    R: Clone + 'static,
    O: OrdOffset + 'static,
{
    /// Where all the data is.
    #[doc(hidden)]
    pub layer: Layers<K, V, R, O>,
}

impl<K, V, R, O> Display for OrdIndexedZSet<K, V, R, O>
where
    K: DBData,
    V: DBData,
    R: DBWeight,
    O: OrdOffset,
    Layers<K, V, R, O>: Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "layer:\n{}",
            textwrap::indent(&self.layer.to_string(), "    ")
        )
    }
}

impl<K, V, R, O> Default for OrdIndexedZSet<K, V, R, O>
where
    K: DBData,
    V: DBData,
    R: DBWeight,
    O: OrdOffset,
{
    #[inline]
    fn default() -> Self {
        Self::empty(())
    }
}

impl<K, V, R, O> From<Layers<K, V, R, O>> for OrdIndexedZSet<K, V, R, O>
where
    K: Ord,
    V: Ord,
    R: Clone,
    O: OrdOffset,
{
    #[inline]
    fn from(layer: Layers<K, V, R, O>) -> Self {
        Self { layer }
    }
}

impl<K, V, R, O> From<Layers<K, V, R, O>> for Rc<OrdIndexedZSet<K, V, R, O>>
where
    K: Ord,
    V: Ord,
    R: Clone,
    O: OrdOffset,
{
    #[inline]
    fn from(layer: Layers<K, V, R, O>) -> Self {
        Rc::new(From::from(layer))
    }
}

impl<K, V, R, O> NumEntries for OrdIndexedZSet<K, V, R, O>
where
    K: DBData,
    V: DBData,
    R: DBWeight,
    O: OrdOffset,
{
    const CONST_NUM_ENTRIES: Option<usize> = Layers::<K, V, R, O>::CONST_NUM_ENTRIES;

    #[inline]
    fn num_entries_shallow(&self) -> usize {
        self.layer.num_entries_shallow()
    }

    #[inline]
    fn num_entries_deep(&self) -> usize {
        self.layer.num_entries_deep()
    }
}

impl<K, V, R, O> NegByRef for OrdIndexedZSet<K, V, R, O>
where
    K: Ord + Clone,
    V: Ord + Clone,
    R: MonoidValue + NegByRef,
    O: OrdOffset,
{
    #[inline]
    fn neg_by_ref(&self) -> Self {
        Self {
            layer: self.layer.neg_by_ref(),
        }
    }
}

impl<K, V, R, O> Neg for OrdIndexedZSet<K, V, R, O>
where
    K: Ord + Clone,
    V: Ord + Clone,
    R: MonoidValue + Neg<Output = R>,
    O: OrdOffset,
{
    type Output = Self;

    #[inline]
    fn neg(self) -> Self {
        Self {
            layer: self.layer.neg(),
        }
    }
}

// TODO: by-value merge
impl<K, V, R, O> Add<Self> for OrdIndexedZSet<K, V, R, O>
where
    K: Ord + Clone + 'static,
    V: Ord + Clone,
    R: MonoidValue,
    O: OrdOffset,
{
    type Output = Self;
    #[inline]

    fn add(self, rhs: Self) -> Self::Output {
        Self {
            layer: self.layer.add(rhs.layer),
        }
    }
}

impl<K, V, R, O> AddAssign<Self> for OrdIndexedZSet<K, V, R, O>
where
    K: Ord + Clone + 'static,
    V: Ord + Clone,
    R: MonoidValue,
    O: OrdOffset,
{
    #[inline]
    fn add_assign(&mut self, rhs: Self) {
        self.layer.add_assign(rhs.layer);
    }
}

impl<K, V, R, O> AddAssignByRef for OrdIndexedZSet<K, V, R, O>
where
    K: Ord + Clone + 'static,
    V: Ord + Clone,
    R: MonoidValue,
    O: OrdOffset,
{
    #[inline]
    fn add_assign_by_ref(&mut self, rhs: &Self) {
        self.layer.add_assign_by_ref(&rhs.layer);
    }
}

impl<K, V, R, O> AddByRef for OrdIndexedZSet<K, V, R, O>
where
    K: Ord + Clone + 'static,
    V: Ord + Clone,
    R: MonoidValue,
    O: OrdOffset,
{
    #[inline]
    fn add_by_ref(&self, rhs: &Self) -> Self {
        Self {
            layer: self.layer.add_by_ref(&rhs.layer),
        }
    }
}

impl<K, V, R, O> BatchReader for OrdIndexedZSet<K, V, R, O>
where
    K: DBData,
    V: DBData,
    R: DBWeight,
    O: OrdOffset,
{
    type Key = K;
    type Val = V;
    type Time = ();
    type R = R;
    type Cursor<'s> = OrdIndexedZSetCursor<'s, K, V, R, O>
    where
        V: 's,
        O: 's;
    type Consumer = OrdIndexedZSetConsumer<K, V, R, O>;

    #[inline]
    fn cursor(&self) -> Self::Cursor<'_> {
        OrdIndexedZSetCursor {
            cursor: self.layer.cursor(),
        }
    }

    #[inline]
    fn consumer(self) -> Self::Consumer {
        OrdIndexedZSetConsumer {
            consumer: OrderedLayerConsumer::from(self.layer),
        }
    }

    #[inline]
    fn key_count(&self) -> usize {
        self.layer.keys()
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

impl<K, V, R, O> Batch for OrdIndexedZSet<K, V, R, O>
where
    K: DBData,
    V: DBData,
    R: DBWeight,
    O: OrdOffset,
{
    type Item = (K, V);
    type Batcher = MergeBatcher<(K, V), (), R, Self>;
    type Builder = OrdIndexedZSetBuilder<K, V, R, O>;
    type Merger = OrdIndexedZSetMerger<K, V, R, O>;

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
        OrdIndexedZSetMerger::new_merger(self, other)
    }

    fn recede_to(&mut self, _frontier: &()) {}

    fn empty(_time: Self::Time) -> Self {
        Self {
            layer: OrderedLayer::default(),
        }
    }
}

/// State for an in-progress merge.
#[derive(SizeOf)]
pub struct OrdIndexedZSetMerger<K, V, R, O>
where
    K: DBData,
    V: DBData,
    R: DBWeight,
    O: OrdOffset,
{
    // first batch, and position therein.
    lower1: usize,
    upper1: usize,
    // second batch, and position therein.
    lower2: usize,
    upper2: usize,
    // result that we are currently assembling.
    result: <Layers<K, V, R, O> as Trie>::MergeBuilder,
}

impl<K, V, R, O> Merger<K, V, (), R, OrdIndexedZSet<K, V, R, O>>
    for OrdIndexedZSetMerger<K, V, R, O>
where
    Self: SizeOf,
    K: DBData,
    V: DBData,
    R: DBWeight,
    O: OrdOffset,
{
    #[inline]
    fn new_merger(
        batch1: &OrdIndexedZSet<K, V, R, O>,
        batch2: &OrdIndexedZSet<K, V, R, O>,
    ) -> Self {
        Self {
            lower1: batch1.layer.lower_bound(),
            upper1: batch1.layer.lower_bound() + batch1.layer.keys(),
            lower2: batch2.layer.lower_bound(),
            upper2: batch2.layer.lower_bound() + batch2.layer.keys(),
            result: <<Layers<K, V, R, O> as Trie>::MergeBuilder as MergeBuilder>::with_capacity(
                &batch1.layer,
                &batch2.layer,
            ),
        }
    }

    #[inline]
    fn done(self) -> OrdIndexedZSet<K, V, R, O> {
        OrdIndexedZSet {
            layer: self.result.done(),
        }
    }

    fn work(
        &mut self,
        source1: &OrdIndexedZSet<K, V, R, O>,
        source2: &OrdIndexedZSet<K, V, R, O>,
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
pub struct OrdIndexedZSetCursor<'s, K, V, R, O>
where
    K: Ord + Clone + 'static,
    V: Ord + Clone + 'static,
    R: MonoidValue,
    O: OrdOffset + PartialEq,
{
    cursor: OrderedCursor<'s, K, O, ColumnLayer<V, R>>,
}

impl<'s, K, V, R, O> Cursor<K, V, (), R> for OrdIndexedZSetCursor<'s, K, V, R, O>
where
    K: Ord + Clone,
    V: Ord + Clone,
    R: MonoidValue,
    O: OrdOffset,
{
    fn key(&self) -> &K {
        self.cursor.item()
    }

    fn val(&self) -> &V {
        self.cursor.child.current_key()
    }

    fn fold_times<F, U>(&mut self, init: U, mut fold: F) -> U
    where
        F: FnMut(U, &(), &R) -> U,
    {
        if self.cursor.child.valid() {
            fold(init, &(), self.cursor.child.current_diff())
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
        debug_assert!(self.cursor.child.valid());
        self.cursor.child.current_diff().clone()
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
        self.cursor.child.seek_key_with(|v| !predicate(v));
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
        self.cursor.child.seek_key_with_reverse(|v| !predicate(v));
    }

    fn fast_forward_vals(&mut self) {
        self.cursor.child.fast_forward();
    }
}

type IndexBuilder<K, V, R, O> = OrderedBuilder<K, ColumnLayerBuilder<V, R>, O>;

/// A builder for creating layers from unsorted update tuples.
#[derive(SizeOf)]
pub struct OrdIndexedZSetBuilder<K, V, R, O>
where
    K: Ord,
    V: Ord,
    R: MonoidValue,
    O: OrdOffset,
{
    builder: IndexBuilder<K, V, R, O>,
}

impl<K, V, R, O> Builder<(K, V), (), R, OrdIndexedZSet<K, V, R, O>>
    for OrdIndexedZSetBuilder<K, V, R, O>
where
    Self: SizeOf,
    K: DBData,
    V: DBData,
    R: DBWeight,
    O: OrdOffset,
{
    #[inline]
    fn new_builder(_time: ()) -> Self {
        Self {
            builder: IndexBuilder::<K, V, R, O>::new(),
        }
    }

    #[inline]
    fn with_capacity(_time: (), capacity: usize) -> Self {
        Self {
            builder: <IndexBuilder<K, V, R, O> as TupleBuilder>::with_capacity(capacity),
        }
    }

    #[inline]
    fn reserve(&mut self, additional: usize) {
        self.builder.reserve(additional);
    }

    #[inline]
    fn push(&mut self, ((key, val), diff): ((K, V), R)) {
        self.builder.push_tuple((key, (val, diff)));
    }

    #[inline(never)]
    fn done(self) -> OrdIndexedZSet<K, V, R, O> {
        OrdIndexedZSet {
            layer: self.builder.done(),
        }
    }
}

pub struct OrdIndexedZSetConsumer<K, V, R, O>
where
    K: 'static,
    V: 'static,
    R: 'static,
    O: OrdOffset,
{
    consumer: OrderedLayerConsumer<K, V, R, O>,
}

impl<K, V, R, O> Consumer<K, V, R, ()> for OrdIndexedZSetConsumer<K, V, R, O>
where
    O: OrdOffset,
{
    type ValueConsumer<'a> = OrdIndexedZSetValueConsumer<'a, K, V,  R, O>
    where
        Self: 'a;

    fn key_valid(&self) -> bool {
        self.consumer.key_valid()
    }

    fn peek_key(&self) -> &K {
        self.consumer.peek_key()
    }

    fn next_key(&mut self) -> (K, Self::ValueConsumer<'_>) {
        let (key, consumer) = self.consumer.next_key();
        (key, OrdIndexedZSetValueConsumer::new(consumer))
    }

    fn seek_key(&mut self, key: &K)
    where
        K: Ord,
    {
        self.consumer.seek_key(key)
    }
}

pub struct OrdIndexedZSetValueConsumer<'a, K, V, R, O>
where
    V: 'static,
    R: 'static,
{
    consumer: OrderedLayerValues<'a, V, R>,
    __type: PhantomData<(K, O)>,
}

impl<'a, K, V, R, O> OrdIndexedZSetValueConsumer<'a, K, V, R, O> {
    #[inline]
    const fn new(consumer: OrderedLayerValues<'a, V, R>) -> Self {
        Self {
            consumer,
            __type: PhantomData,
        }
    }
}

impl<'a, K, V, R, O> ValueConsumer<'a, V, R, ()> for OrdIndexedZSetValueConsumer<'a, K, V, R, O> {
    fn value_valid(&self) -> bool {
        self.consumer.value_valid()
    }

    fn next_value(&mut self) -> (V, R, ()) {
        self.consumer.next_value()
    }

    fn remaining_values(&self) -> usize {
        self.consumer.remaining_values()
    }
}
