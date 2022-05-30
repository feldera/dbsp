use std::{
    cmp::max,
    convert::TryFrom,
    fmt::Debug,
    ops::{Add, AddAssign, Neg},
    rc::Rc,
};

use timely::progress::Antichain;

use crate::{
    algebra::{
        AddAssignByRef, AddByRef, HasOne, HasZero, IndexedZSet, MonoidValue, NegByRef, ZRingValue,
        ZSet,
    },
    lattice::Lattice,
    trace::{
        description::Description,
        layers::{
            ordered_leaf::{OrderedLeaf, OrderedLeafBuilder, OrderedLeafCursor},
            Builder as TrieBuilder, Cursor as TrieCursor, MergeBuilder, Trie, TupleBuilder,
        },
        ord::merge_batcher::MergeBatcher,
        Batch, BatchReader, Builder, Cursor, Merger,
    },
    NumEntries, SharedRef,
};

use deepsize::DeepSizeOf;

/// An immutable collection of `(key, weight)` pairs without timing information.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct OrdZSet<K, R>
where
    K: Ord,
{
    /// Where all the dataz is.
    pub layer: OrderedLeaf<K, R>,
    pub desc: Description<()>,
}

impl<K, R> From<OrderedLeaf<K, R>> for OrdZSet<K, R>
where
    K: Ord,
{
    fn from(layer: OrderedLeaf<K, R>) -> Self {
        Self {
            layer,
            desc: Description::new(Antichain::from_elem(()), Antichain::new()),
        }
    }
}

impl<K, R> From<OrderedLeaf<K, R>> for Rc<OrdZSet<K, R>>
where
    K: Ord,
{
    fn from(layer: OrderedLeaf<K, R>) -> Self {
        Rc::new(From::from(layer))
    }
}

impl<K, R> TryFrom<Rc<OrdZSet<K, R>>> for OrdZSet<K, R>
where
    K: Ord,
{
    type Error = Rc<OrdZSet<K, R>>;

    fn try_from(batch: Rc<OrdZSet<K, R>>) -> Result<Self, Self::Error> {
        Rc::try_unwrap(batch)
    }
}

impl<K, R> DeepSizeOf for OrdZSet<K, R>
where
    K: DeepSizeOf + Ord,
    R: DeepSizeOf,
{
    fn deep_size_of_children(&self, _context: &mut deepsize::Context) -> usize {
        self.layer.deep_size_of()
    }
}

impl<K, R> NumEntries for OrdZSet<K, R>
where
    K: Ord + Clone,
    R: Eq + HasZero + AddAssignByRef + Clone,
{
    fn num_entries_shallow(&self) -> usize {
        self.layer.num_entries_shallow()
    }

    fn num_entries_deep(&self) -> usize {
        self.layer.num_entries_deep()
    }

    fn const_num_entries() -> Option<usize> {
        <OrderedLeaf<K, R>>::const_num_entries()
    }
}

impl<K, R> HasZero for OrdZSet<K, R>
where
    K: Ord + Clone + 'static,
    R: MonoidValue,
{
    fn zero() -> Self {
        Self::empty(())
    }

    fn is_zero(&self) -> bool {
        self.is_empty()
    }
}

impl<K, R> SharedRef for OrdZSet<K, R>
where
    K: Ord + Clone,
    R: Clone,
{
    type Target = Self;

    fn try_into_owned(self) -> Result<Self::Target, Self> {
        Ok(self)
    }
}

impl<K, R> NegByRef for OrdZSet<K, R>
where
    K: Ord + Clone,
    R: MonoidValue + NegByRef,
{
    fn neg_by_ref(&self) -> Self {
        Self {
            layer: self.layer.neg_by_ref(),
            desc: self.desc.clone(),
        }
    }
}

impl<K, R> Neg for OrdZSet<K, R>
where
    K: Ord + Clone,
    R: MonoidValue + Neg<Output = R>,
{
    type Output = Self;

    fn neg(self) -> Self {
        Self {
            layer: self.layer.neg(),
            desc: self.desc,
        }
    }
}

// TODO: by-value merge
impl<K, R> Add<Self> for OrdZSet<K, R>
where
    K: Ord + Clone + 'static,
    R: MonoidValue,
{
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        let desc = Description::new(
            self.lower().meet(rhs.lower()),
            self.upper().join(rhs.upper()),
        );

        Self {
            layer: self.layer.add(rhs.layer),
            desc,
        }
    }
}

impl<K, R> AddAssign<Self> for OrdZSet<K, R>
where
    K: Ord + Clone + 'static,
    R: MonoidValue,
{
    fn add_assign(&mut self, rhs: Self) {
        self.desc = Description::new(
            self.lower().meet(rhs.lower()),
            self.upper().join(rhs.upper()),
        );
        self.layer.add_assign(rhs.layer);
    }
}

impl<K, R> AddAssignByRef for OrdZSet<K, R>
where
    K: Ord + Clone + 'static,
    R: MonoidValue,
{
    fn add_assign_by_ref(&mut self, rhs: &Self) {
        self.layer.add_assign_by_ref(&rhs.layer);
        self.desc = Description::new(
            self.lower().meet(rhs.lower()),
            self.upper().join(rhs.upper()),
        );
    }
}

impl<K, R> AddByRef for OrdZSet<K, R>
where
    K: Ord + Clone + 'static,
    R: MonoidValue,
{
    fn add_by_ref(&self, rhs: &Self) -> Self {
        Self {
            layer: self.layer.add_by_ref(&rhs.layer),
            desc: Description::new(
                self.lower().meet(rhs.lower()),
                self.upper().join(rhs.upper()),
            ),
        }
    }
}

impl<K, R> BatchReader for OrdZSet<K, R>
where
    K: Ord + Clone + 'static,
    R: MonoidValue,
{
    type Key = K;
    type Val = ();
    type Time = ();
    type R = R;
    type Cursor = OrdZSetCursor;

    fn cursor(&self) -> Self::Cursor {
        OrdZSetCursor {
            empty: (),
            valid: true,
            cursor: self.layer.cursor(),
        }
    }
    fn len(&self) -> usize {
        <OrderedLeaf<K, R> as Trie>::tuples(&self.layer)
    }
    fn description(&self) -> &Description<()> {
        &self.desc
    }
}

impl<K, R> Batch for OrdZSet<K, R>
where
    K: Ord + Clone + 'static,
    R: MonoidValue,
{
    type Batcher = MergeBatcher<K, (), (), R, Self>;
    type Builder = OrdZSetBuilder<K, R>;
    type Merger = OrdZSetMerger<K, R>;

    fn begin_merge(&self, other: &Self) -> Self::Merger {
        OrdZSetMerger::new(self, other)
    }

    fn recede_to(&mut self, _frontier: &()) {}
}

/// State for an in-progress merge.
pub struct OrdZSetMerger<K, R>
where
    K: Ord + Clone + 'static,
    R: MonoidValue,
{
    // result that we are currently assembling.
    result: <OrderedLeaf<K, R> as Trie>::MergeBuilder,
}

impl<K, R> Merger<K, (), (), R, OrdZSet<K, R>> for OrdZSetMerger<K, R>
where
    K: Ord + Clone + 'static,
    R: MonoidValue,
{
    fn new(batch1: &OrdZSet<K, R>, batch2: &OrdZSet<K, R>) -> Self {
        OrdZSetMerger {
            result: <<OrderedLeaf<K, R> as Trie>::MergeBuilder as MergeBuilder>::with_capacity(
                &batch1.layer,
                &batch2.layer,
            ),
        }
    }
    fn done(self) -> OrdZSet<K, R> {
        OrdZSet {
            layer: self.result.done(),
            desc: Description::new(Antichain::from_elem(()), Antichain::new()),
        }
    }
    fn work(&mut self, source1: &OrdZSet<K, R>, source2: &OrdZSet<K, R>, fuel: &mut isize) {
        *fuel -= self.result.push_merge(
            (&source1.layer, source1.layer.cursor()),
            (&source2.layer, source2.layer.cursor()),
        ) as isize;
        *fuel = max(*fuel, 1);
    }
}

/// A cursor for navigating a single layer.
#[derive(Debug)]
pub struct OrdZSetCursor {
    valid: bool,
    empty: (),
    cursor: OrderedLeafCursor,
}

impl<K, R> Cursor<K, (), (), R> for OrdZSetCursor
where
    K: Ord + Clone,
    R: MonoidValue,
{
    type Storage = OrdZSet<K, R>;

    fn key<'a>(&self, storage: &'a Self::Storage) -> &'a K {
        &self.cursor.key(&storage.layer).0
    }
    fn val<'a>(&self, _storage: &'a Self::Storage) -> &'a () {
        unsafe { ::std::mem::transmute(&self.empty) }
    }
    fn map_times<L: FnMut(&(), &R)>(&mut self, storage: &Self::Storage, mut logic: L) {
        if self.cursor.valid(&storage.layer) {
            logic(&(), &self.cursor.key(&storage.layer).1);
        }
    }
    fn weight<'a>(&self, storage: &'a Self::Storage) -> &'a R {
        debug_assert!(&self.cursor.valid(&storage.layer));
        &self.cursor.key(&storage.layer).1
    }
    fn key_valid(&self, storage: &Self::Storage) -> bool {
        self.cursor.valid(&storage.layer)
    }
    fn val_valid(&self, _storage: &Self::Storage) -> bool {
        self.valid
    }
    fn step_key(&mut self, storage: &Self::Storage) {
        self.cursor.step(&storage.layer);
        self.valid = true;
    }
    fn seek_key(&mut self, storage: &Self::Storage, key: &K) {
        self.cursor.seek_key(&storage.layer, key);
        self.valid = true;
    }
    fn step_val(&mut self, _storage: &Self::Storage) {
        self.valid = false;
    }
    fn seek_val(&mut self, _storage: &Self::Storage, _val: &()) {}
    fn rewind_keys(&mut self, storage: &Self::Storage) {
        self.cursor.rewind(&storage.layer);
        self.valid = true;
    }
    fn rewind_vals(&mut self, _storage: &Self::Storage) {
        self.valid = true;
    }
}

/// A builder for creating layers from unsorted update tuples.
pub struct OrdZSetBuilder<K, R>
where
    K: Ord,
    R: MonoidValue,
{
    builder: OrderedLeafBuilder<K, R>,
}

impl<K, R> Builder<K, (), (), R, OrdZSet<K, R>> for OrdZSetBuilder<K, R>
where
    K: Ord + Clone + 'static,
    R: MonoidValue,
{
    fn new(_time: ()) -> Self {
        OrdZSetBuilder {
            builder: <OrderedLeafBuilder<K, R>>::new(),
        }
    }

    fn with_capacity(_time: (), cap: usize) -> Self {
        OrdZSetBuilder {
            builder: <OrderedLeafBuilder<K, R> as TupleBuilder>::with_capacity(cap),
        }
    }

    #[inline]
    fn push(&mut self, (key, (), diff): (K, (), R)) {
        self.builder.push_tuple((key, diff));
    }

    #[inline(never)]
    fn done(self) -> OrdZSet<K, R> {
        OrdZSet {
            layer: self.builder.done(),
            desc: Description::new(Antichain::from_elem(()), Antichain::new()),
        }
    }
}

impl<K, W> IndexedZSet for OrdZSet<K, W>
where
    K: Clone + Ord + 'static,
    W: ZRingValue,
{
}

impl<K, W> ZSet for OrdZSet<K, W>
where
    K: Ord + Clone + 'static,
    W: ZRingValue,
{
    fn distinct(&self) -> Self {
        let mut builder = Self::Builder::with_capacity((), self.len());
        let mut cursor = self.cursor();

        while cursor.key_valid(self) {
            let key = cursor.key(self);
            let w = cursor.weight(self);
            if w.ge0() {
                builder.push((key.clone(), (), HasOne::one()));
            }
            cursor.step_key(self);
        }

        builder.done()
    }

    // TODO: optimized implementation for owned values
    fn distinct_owned(self) -> Self {
        self.distinct()
    }
}
