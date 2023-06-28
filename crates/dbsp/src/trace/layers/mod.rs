//! Traits and types for building trie-based indices.
//!
//! The trie structure has each each element of each layer indicate a range of
//! elements in the next layer. Similarly, ranges of elements in the layer
//! itself may correspond to single elements in the layer above.

mod advance;

pub mod column_layer;
pub mod erased;
pub mod ordered;
pub mod ordered_leaf;
pub mod unordered;
// pub mod hashed;
// pub mod weighted;

#[cfg(test)]
mod test;

pub use advance::{advance, advance_erased, advance_raw, retreat};
use bincode::{Decode, Encode};

use crate::algebra::HasZero;
use size_of::SizeOf;
use std::{
    fmt::Debug,
    ops::{Add, Sub},
};

/// A collection of tuples, and types for building and enumerating them.
///
/// There are some implicit assumptions about the elements in trie-structured
/// data, mostly that the items have some `(key, val)` structure. Perhaps we
/// will nail these down better in the future and get a better name for the
/// trait.
pub trait Trie: Sized {
    /// The type of item from which the type is constructed.
    type Item;

    /// The type of cursor used to navigate the type.
    type Cursor<'s>: Cursor<'s>
    where
        Self: 's;

    /// The type used to merge instances of the type together.
    type MergeBuilder: MergeBuilder<Trie = Self>;

    /// The type used to assemble instances of the type from its `Item`s.
    type TupleBuilder: TupleBuilder<Trie = Self, Item = Self::Item>;

    /// The number of distinct keys, as distinct from the total number of
    /// tuples.
    fn keys(&self) -> usize;

    /// True if `self.keys()` is zero.
    fn is_empty(&self) -> bool {
        self.keys() == 0
    }

    /// The total number of tuples in the collection.
    fn tuples(&self) -> usize;

    /// Returns a cursor capable of navigating the collection.
    fn cursor(&self) -> Self::Cursor<'_> {
        self.cursor_from(self.lower_bound(), self.lower_bound() + self.keys())
    }

    /// Returns a cursor over a range of data, commonly used by others to
    /// restrict navigation to sub-collections.
    fn cursor_from(&self, lower: usize, upper: usize) -> Self::Cursor<'_>;

    /// Merges two collections into a third.
    ///
    /// Collections are allowed their own semantics for merging. For example,
    /// unordered collections simply collect values, whereas weighted
    /// collections accumulate weights and discard elements whose weights
    /// are zero.
    fn merge(&self, other: &Self) -> Self {
        let mut merger = Self::MergeBuilder::with_capacity(self, other);
        // println!("{:?} and {:?}", self.keys(), other.keys());
        merger.push_merge(self.cursor(), other.cursor());
        merger.done()
    }

    /// Informs the trie that keys below `lower_bound` will no longer be
    /// accessed.
    ///
    /// This operation affects the behavior of the trie in two ways:
    ///
    /// * The default cursor returned by the [`Self::cursor`] method starts
    ///   iterating from this bound and not from the start of the trie.
    /// * Merging two tries using [`Self::merge`] does not include truncated
    ///   tuples in the result.
    ///
    /// This operation is ignored if `lower_bound` is less than or equal to
    /// the bound specified in an earlier call to this method.
    fn truncate_below(&mut self, lower_bound: usize);

    /// Returns the current value of the lower bound.
    ///
    /// The result is equal to the largest bound specified via
    /// [`Self::truncate_below`] or 0 if `truncate_below` was never called.
    fn lower_bound(&self) -> usize;
}

/// A type used to assemble collections.
pub trait Builder {
    /// The type of collection produced.
    type Trie: Trie;

    /// Requests a commitment to the offset of the current-most sub-collection.
    ///
    /// This is most often used by parent collections to indicate that some set
    /// of values are now logically distinct from the next set of values,
    /// and that the builder should acknowledge this and report the limit
    /// (to store as an offset in the parent collection).
    fn boundary(&mut self) -> usize;

    /// Finalizes the building process and returns the collection.
    fn done(self) -> Self::Trie;
}

/// A type used to assemble collections by merging other instances.
pub trait MergeBuilder: Builder {
    /// Allocates an instance of the builder with sufficient capacity to contain
    /// the merged data.
    fn with_capacity(other1: &Self::Trie, other2: &Self::Trie) -> Self;

    fn with_key_capacity(cap: usize) -> Self;

    fn reserve(&mut self, additional: usize);

    /// The number of keys pushed to the builder so far.
    fn keys(&self) -> usize;

    /// Copies sub-collections of `other` into this collection.
    fn copy_range(&mut self, other: &Self::Trie, lower: usize, upper: usize);

    /// Merges two sub-collections into one sub-collection.
    fn push_merge<'a>(
        &'a mut self,
        other1: <Self::Trie as Trie>::Cursor<'a>,
        other2: <Self::Trie as Trie>::Cursor<'a>,
    ) -> usize;
}

/// A type used to assemble collections from ordered sequences of tuples.
pub trait TupleBuilder: Builder {
    /// The type of item accepted for construction.
    type Item;

    /// Allocates a new builder.
    fn new() -> Self;

    /// Allocates a new builder with capacity for at least `cap` tuples.
    fn with_capacity(capacity: usize) -> Self; // <-- unclear how to set child capacities...

    /// Reserve space for `additional` new tuples to be added to the current
    /// builder
    fn reserve_tuples(&mut self, additional: usize);

    /// Inserts a new tuple into the current builder
    fn push_tuple(&mut self, tuple: Self::Item);

    /// Inserts all of the given tuples into the current builder
    fn extend_tuples<I>(&mut self, tuples: I)
    where
        I: IntoIterator<Item = Self::Item>,
    {
        let tuples = tuples.into_iter();

        let (lower, upper) = tuples.size_hint();
        self.reserve_tuples(upper.unwrap_or(lower));

        for tuple in tuples {
            self.push_tuple(tuple);
        }
    }

    fn tuples(&self) -> usize;
}

/// A type supporting navigation.
///
/// The precise meaning of this navigation is not defined by the trait. It is
/// likely that having navigated around, the cursor will be different in some
/// other way, but the `Cursor` trait does not explain how this is so.
pub trait Cursor<'s> {
    /// The type revealed by the cursor.
    type Item<'k>
    where
        Self: 'k;

    /// Key used to search the contents of the cursor.
    type Key;

    type ValueStorage: Trie;

    fn keys(&self) -> usize;

    /// Reveals the current item.
    fn item(&self) -> Self::Item<'s>;

    /// Returns cursor over values associted with the current key.
    fn values(&self) -> <Self::ValueStorage as Trie>::Cursor<'s>;

    /// Advances the cursor by one element.
    fn step(&mut self);

    /// Move cursor back by one element.
    fn step_reverse(&mut self);

    /// Advances the cursor until the location where `key` would be expected.
    fn seek(&mut self, key: &Self::Key);

    /// Move the cursor back until the location where `key` would be expected.
    fn seek_reverse(&mut self, key: &Self::Key);

    /// Returns `true` if the cursor points at valid data. Returns `false` if
    /// the cursor is exhausted.
    fn valid(&self) -> bool;

    /// Rewinds the cursor to its initial state.
    fn rewind(&mut self);

    /// Moves the cursor to the last position.
    fn fast_forward(&mut self);

    /// Current position of the cursor.
    fn position(&self) -> usize;

    /// Repositions the cursor to a different range of values.
    fn reposition(&mut self, lower: usize, upper: usize);
}

/// Trait for types used as offsets into an ordered layer.
/// This is usually `usize`, but `u32` can also be used in applications
/// where huge batches do not occur to reduce metadata size.
pub trait OrdOffset:
    Copy
    + PartialEq
    + Add<Output = Self>
    + Sub<Output = Self>
    + TryFrom<usize>
    + TryInto<usize>
    + HasZero
    + SizeOf
    + Sized
    + Encode
    + Decode
    + 'static
{
    fn from_usize(offset: usize) -> Self;

    fn into_usize(self) -> usize;
}

impl<O> OrdOffset for O
where
    O: Copy
        + PartialEq
        + Add<Output = Self>
        + Sub<Output = Self>
        + TryFrom<usize>
        + TryInto<usize>
        + HasZero
        + SizeOf
        + Sized
        + Encode
        + Decode
        + 'static,
    <O as TryInto<usize>>::Error: Debug,
    <O as TryFrom<usize>>::Error: Debug,
{
    #[inline]
    fn from_usize(offset: usize) -> Self {
        offset.try_into().unwrap()
    }

    #[inline]
    fn into_usize(self) -> usize {
        self.try_into().unwrap()
    }
}

impl Trie for () {
    type Item = ();
    type Cursor<'s> = ();
    type MergeBuilder = ();
    type TupleBuilder = ();

    fn keys(&self) -> usize {
        0
    }
    fn tuples(&self) -> usize {
        0
    }
    fn cursor_from(&self, _lower: usize, _upper: usize) -> Self::Cursor<'_> {}

    fn truncate_below(&mut self, _lower_bound: usize) {}
    fn lower_bound(&self) -> usize {
        0
    }
}

impl Builder for () {
    type Trie = ();

    fn boundary(&mut self) -> usize {
        0
    }
    fn done(self) -> Self::Trie {}
}

impl MergeBuilder for () {
    fn with_capacity(_other1: &(), _other2: &()) -> Self {}

    fn with_key_capacity(_capacity: usize) -> Self {}

    fn reserve(&mut self, _additional: usize) {}

    fn keys(&self) -> usize {
        0
    }

    fn copy_range(&mut self, _other: &Self::Trie, _lower: usize, _upper: usize) {}

    fn push_merge(
        &mut self,
        _other1: <Self::Trie as Trie>::Cursor<'static>,
        _other2: <Self::Trie as Trie>::Cursor<'static>,
    ) -> usize {
        0
    }
}

impl TupleBuilder for () {
    type Item = ();

    fn new() -> Self {}

    fn with_capacity(_capacity: usize) -> Self {}

    fn reserve_tuples(&mut self, _additional: usize) {}

    fn push_tuple(&mut self, _tuple: Self::Item) {}

    fn tuples(&self) -> usize {
        0
    }
}

impl<'s> Cursor<'s> for () {
    type Key = ();
    type Item<'k> = &'k ();

    type ValueStorage = ();

    fn keys(&self) -> usize {
        0
    }
    fn item(&self) -> Self::Item<'s> {
        &()
    }
    fn values(&self) {}
    fn step(&mut self) {}

    fn seek(&mut self, _key: &Self::Key) {}

    fn valid(&self) -> bool {
        false
    }
    fn rewind(&mut self) {}

    fn position(&self) -> usize {
        0
    }

    fn reposition(&mut self, _lower: usize, _upper: usize) {}

    fn step_reverse(&mut self) {}

    fn seek_reverse(&mut self, _key: &Self::Key) {}

    fn fast_forward(&mut self) {}
}
