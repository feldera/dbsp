//! Traits and datastructures representing a collection trace.
//!
//! A collection trace is a set of updates of the form `(key, val, time, diff)`,
//! which determine the contents of a collection at given times by accumulating
//! updates whose time field is less or equal to the target field.
//!
//! The `Trace` trait describes those types and methods that a data structure
//! must implement to be viewed as a collection trace. This trait allows
//! operator implementations to be generic with respect to the type of trace,
//! and allows various data structures to be interpretable as multiple different
//! types of trace.

pub mod consolidation;
pub mod cursor;
pub mod layers;
pub mod ord;
#[cfg(feature = "persistence")]
pub mod persistent;
pub mod spine_fueled;

pub use cursor::{Consumer, Cursor, ValueConsumer};
#[cfg(feature = "persistence")]
pub use persistent::PersistentTrace as Spine;
#[cfg(not(feature = "persistence"))]
pub use spine_fueled::Spine;

#[cfg(test)]
pub mod test_batch;

use crate::{
    algebra::{HasZero, MonoidValue},
    circuit::Activator,
    time::{AntichainRef, Timestamp},
    NumEntries,
};
use bincode::{Decode, Encode};
use rand::Rng;
use size_of::SizeOf;
use std::{fmt::Debug, hash::Hash};

/// Trait for data stored in batches.
///
/// This trait is used as a bound on `BatchReader::Key` and `BatchReader::Val`
/// associated types (see [`trait BatchReader`]).  Hence when writing code that
/// must be generic over any relational data, it is sufficient to impose
/// `DBData` as a trait bound on types.  Conversely, a trait bound of the form
/// `B: BatchReader` implies `B::Key: DBData` and `B::Val: DBData`.
pub trait DBData:
    Clone + Eq + Ord + Hash + SizeOf + Send + Debug + Decode + Encode + 'static
{
}

impl<T> DBData for T where
    T: Clone + Eq + Ord + Hash + SizeOf + Send + Debug + Decode + Encode + 'static
{
}

/// Trait for data types used as weights.
///
/// A type used for weights in a batch (i.e., as `BatchReader::R`) must behave
/// as a monoid, i.e., a set with an associative `+` operation and a neutral
/// element (zero).
///
/// Some applications use a weight as a ring, that is, require it to support
/// multiplication too.
///
/// Finally, some applications require it to have `<` and `>` operations, in
/// particular to distinguish whether something is an insertion or deletion.
///
/// Signed integer types such as `i32` and `i64` are suitable as weights,
/// although if there is overflow then the results will be wrong.
///
/// When writing code generic over any weight type, it is sufficient to impose
/// `DBWeight` as a trait bound on types.  Conversely, a trait bound of the form
/// `B: BatchReader` implies `B::R: DBWeight`.
pub trait DBWeight: DBData + MonoidValue {}
impl<T> DBWeight for T where T: DBData + MonoidValue {}

/// Trait for data types used as logical timestamps.
pub trait DBTimestamp: DBData + Timestamp {}
impl<T> DBTimestamp for T where T: DBData + Timestamp {}

/// An append-only collection of `(key, val, time, diff)` tuples.
///
/// The trace must be constructable from, and navigable by the `Key`, `Val`,
/// `Time` types, but does not need to return them.
pub trait Trace: BatchReader {
    /// The type of an immutable collection of updates.
    type Batch: Batch<Key = Self::Key, Val = Self::Val, Time = Self::Time, R = Self::R>;

    /// Allocates a new empty trace.
    fn new(activator: Option<Activator>) -> Self;

    /// Push all timestamps in the trace back to `frontier`.
    ///
    /// Modifies all timestamps `t` that are not less than or equal to
    /// `frontier` to `t.meet(frontier)`.  As a result, the trace can no
    /// longer distinguish between timestamps that map to the same value,
    /// but it will contain fewer different timestamps, thus reducing its
    /// memory footprint.
    ///
    /// This also enables us to use fewer bits to represent timestamps.
    /// In DBSP, computations inside a nested circuit only need to distinguish
    /// between updates added during the current run of the nested circuit
    /// vs all previous runs.  Thus, we only need a single bit for the outer
    /// time stamp.  When the nested clock epoch completes, all tuples with
    /// outer timestamp `1` are demoted to `0`, so they appear as old
    /// updates during the next run of the circuit.
    ///
    /// The downside of the 1-bit clock is that it requires rewriting timestamps
    /// and rearranging batches in the trace at the end of every clock epoch.
    /// Unlike merging of batches, which can be done in the background, this
    /// work must be completed synchronously before the start of the next
    /// epoch. This cost should be roughly proportional to the number of
    /// updates added to the trace during the last epoch.
    ///
    /// See [`NestedTimestamp32`](`crate::time::NestedTimestamp32`) for an
    /// example of a timestamp type that takes advantage of the 1-bit
    /// timestamp representation.
    fn recede_to(&mut self, frontier: &Self::Time);

    /// Exert merge effort, even without updates.
    fn exert(&mut self, effort: &mut isize);

    /// Merge all updates in a trace into a single batch.
    fn consolidate(self) -> Option<Self::Batch>;

    /// Introduces a batch of updates to the trace.
    ///
    /// Batches describe the time intervals they contain, and they should be
    /// added to the trace in contiguous intervals. If a batch arrives with
    /// a lower bound that does not equal the upper bound of the most recent
    /// addition, the trace will add an empty batch. It is an error to then try
    /// to populate that region of time.
    ///
    /// This restriction could be relaxed, especially if we discover ways in
    /// which batch interval order could commute. For now, the trace should
    /// complain, to the extent that it cares about contiguous intervals.
    fn insert(&mut self, batch: Self::Batch);

    /// Clears the value of the "dirty" flag to `false`.
    ///
    /// The "dirty" flag is used to efficiently track changes to the trace,
    /// e.g., as part of checking whether a circuit has reached a fixed point.
    /// Pushing a non-empty batch to the trace sets the flag to `true`. The
    /// [`Self::dirty`] method returns true iff the trace has changed since the
    /// last call to `clear_dirty_flag`.
    fn clear_dirty_flag(&mut self);

    /// Returns the value of the dirty flag.
    fn dirty(&self) -> bool;

    /// Informs the trace that values smaller than `lower_bound` are no longer
    /// used and can be removed from the trace.
    ///
    /// The implementation is not required to remove truncated values instantly
    /// or at all.  This method is just a hint that values below `lower_bound`
    /// are no longer of interest to the consumer of the trace and can be
    /// garbage collected.
    // This API is similar to `BatchReader::truncate_keys_below`, however we make
    // it a method of `trait Trace` rather than `trait BatchReader`.  The difference
    // is that a batch can truncate its keys instanly by simply moving an internal
    // pointer to the first remaining key.  However, there is no similar way to
    // truncate values instantly, this can only be done as part of trace maintenance
    // when either merging or compacting batches.
    fn truncate_values_below(&mut self, lower_bound: &Self::Val);

    /// Current lower value bound.
    fn lower_value_bound(&self) -> &Option<Self::Val>;
}

/// A batch of updates whose contents may be read.
///
/// This is a restricted interface to batches of updates, which support the
/// reading of the batch's contents, but do not expose ways to construct the
/// batches. This trait is appropriate for views of the batch, and is especially
/// useful for views derived from other sources in ways that prevent the
/// construction of batches from the type of data in the view (for example,
/// filtered views, or views with extended time coordinates).
pub trait BatchReader: NumEntries + SizeOf + 'static
where
    Self: Sized,
{
    /// Key by which updates are indexed.
    type Key: DBData;

    /// Values associated with keys.
    type Val: DBData;

    /// Timestamps associated with updates
    type Time: DBTimestamp;

    /// Associated update.
    type R: DBWeight;

    /// The type used to enumerate the batch's contents.
    type Cursor<'s>: Cursor<Self::Key, Self::Val, Self::Time, Self::R>
    where
        Self: 's;

    type Consumer: Consumer<Self::Key, Self::Val, Self::R, Self::Time>;

    /// Acquires a cursor to the batch's contents.
    fn cursor(&self) -> Self::Cursor<'_>;

    fn consumer(self) -> Self::Consumer;

    /// The number of keys in the batch.
    // TODO: return `(usize, Option<usize>)`, similar to
    // `Iterator::size_hint`, since not all implementations
    // can compute the number of keys precisely.  Same for
    // `len()`.
    fn key_count(&self) -> usize;

    /// The number of updates in the batch.
    fn len(&self) -> usize;

    /// True if the batch is empty.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// All times in the batch are greater or equal to an element of `lower`.
    fn lower(&self) -> AntichainRef<'_, Self::Time>;

    /// All times in the batch are not greater or equal to any element of
    /// `upper`.
    fn upper(&self) -> AntichainRef<'_, Self::Time>;

    /// Remove keys smaller than `lower_bound` from the batch.
    ///
    /// The removed tuples may not get deallocated instantly but they won't
    /// appear when iterating over the batch.
    fn truncate_keys_below(&mut self, lower_bound: &Self::Key);

    /// Returns a uniform random sample of distincts keys from the batch.
    ///
    /// Does not take into account the number values associated with each
    /// key and their weights, i.e., a key that has few values is as likely
    /// to appear in the output sample as a key with many values.
    ///
    /// # Arguments
    ///
    /// * `rng` - random number generator used to generate the sample.
    ///
    /// * `sample_size` - requested sample size.
    ///
    /// * `sample` - output
    ///
    /// # Invariants
    ///
    /// The actual sample computed by the method can be smaller than
    /// `sample_size` even if `self` contains `>sample_size` keys.
    ///
    /// A correct implementation must enforce the following invariants:
    ///
    /// * The output sample size cannot exceed `sample_size`.
    ///
    /// * The output sample can only contain keys present in `self` (with
    ///   non-zero weights).
    ///
    /// * If `sample_size` is greater than or equal to the number of keys
    ///   present in `self` (with non-zero weights), the resulting sample must
    ///   contain all such keys.
    ///
    /// * The output sample contains keys sorted in ascending order.
    fn sample_keys<RG>(&self, rng: &mut RG, sample_size: usize, sample: &mut Vec<Self::Key>)
    where
        Self::Time: PartialEq<()>,
        RG: Rng;
}

/// An immutable collection of updates.
pub trait Batch: BatchReader + Clone
where
    Self: Sized,
{
    /// Items used to assemble the batch.  Must be one of `Self::Key`
    /// (when `Self::Val = ()`) or `(Self::Key, Self::Val)`.
    type Item;

    /// A type used to assemble batches from disordered updates.
    type Batcher: Batcher<Self::Item, Self::Time, Self::R, Self>;

    /// A type used to assemble batches from ordered update sequences.
    type Builder: Builder<Self::Item, Self::Time, Self::R, Self>;

    /// A type used to progressively merge batches.
    type Merger: Merger<Self::Key, Self::Val, Self::Time, Self::R, Self>;

    /// Create an item from a `(key, value)` pair.
    fn item_from(key: Self::Key, val: Self::Val) -> Self::Item;

    /// Assemble an unordered vector of weighted items into a batch.
    #[allow(clippy::type_complexity)]
    fn from_tuples(time: Self::Time, mut tuples: Vec<(Self::Item, Self::R)>) -> Self {
        let mut batcher = Self::Batcher::new_batcher(time);
        batcher.push_batch(&mut tuples);
        batcher.seal()
    }

    /// Assemble an unordered vector of keys into a batch.
    ///
    /// This method is only defined for batches whose `Val` type is `()`.
    fn from_keys(time: Self::Time, keys: Vec<(Self::Key, Self::R)>) -> Self
    where
        Self::Val: From<()>;

    /// Initiates the merging of consecutive batches.
    ///
    /// The result of this method can be exercised to eventually produce the
    /// same result that a call to `self.merge(other)` would produce, but it
    /// can be done in a measured fashion. This can help to avoid latency
    /// spikes where a large merge needs to happen.
    fn begin_merge(&self, other: &Self) -> Self::Merger {
        Self::Merger::new_merger(self, other)
    }

    /// Merges `self` with `other` by running merger to completion.
    fn merge(&self, other: &Self) -> Self {
        let mut fuel = isize::max_value();
        let mut merger = Self::Merger::new_merger(self, other);
        merger.work(self, other, &None, &mut fuel);
        merger.done()
    }

    /// Creates an empty batch.
    fn empty(time: Self::Time) -> Self {
        Self::Builder::new_builder(time).done()
    }

    /// Push all timestamps in the batch back to `frontier`.
    ///
    /// Modifies all timestamps `t` that are not less than or equal to
    /// `frontier` to `t.meet(frontier)`.  See [`Trace::recede_to`].
    fn recede_to(&mut self, frontier: &Self::Time);
}

impl<B> HasZero for B
where
    B: Batch<Time = ()>,
{
    fn zero() -> Self {
        Self::empty(())
    }

    fn is_zero(&self) -> bool {
        self.is_empty()
    }
}

/// Functionality for collecting and batching updates.
pub trait Batcher<I, T, R, Output>: SizeOf
where
    Output: Batch<Item = I, Time = T, R = R>,
{
    /// Allocates a new empty batcher.  All tuples in the batcher (and its
    /// output batch) will have timestamp `time`.
    fn new_batcher(time: T) -> Self;

    /// Adds an unordered batch of elements to the batcher.
    fn push_batch(&mut self, batch: &mut Vec<(I, R)>);

    /// Adds a consolidated batch of elements to the batcher
    fn push_consolidated_batch(&mut self, batch: &mut Vec<(I, R)>);

    /// Returns the number of tuples in the batcher.
    fn tuples(&self) -> usize;

    /// Returns all updates not greater or equal to an element of `upper`.
    fn seal(self) -> Output;
}

/// Functionality for building batches from ordered update sequences.
pub trait Builder<I, T, R, Output>: SizeOf
where
    Output: Batch<Item = I, Time = T, R = R>,
{
    /// Allocates an empty builder.  All tuples in the builder (and its output
    /// batch) will have timestamp `time`.
    fn new_builder(time: T) -> Self;

    /// Allocates an empty builder with some capacity.  All tuples in the
    /// builder (and its output batch) will have timestamp `time`.
    fn with_capacity(time: T, cap: usize) -> Self;

    /// Adds an element to the batch.
    fn push(&mut self, element: (I, R));

    fn reserve(&mut self, additional: usize);

    /// Adds an ordered sequence of elements to the batch.
    #[inline]
    fn extend<It: Iterator<Item = (I, R)>>(&mut self, iter: It) {
        let (lower, upper) = iter.size_hint();
        self.reserve(upper.unwrap_or(lower));

        for item in iter {
            self.push(item);
        }
    }

    /// Completes building and returns the batch.
    fn done(self) -> Output;
}

/// Represents a merge in progress.
pub trait Merger<K, V, T, R, Output>: SizeOf
where
    Output: Batch<Key = K, Val = V, Time = T, R = R>,
{
    /// Creates a new merger to merge the supplied batches.
    fn new_merger(source1: &Output, source2: &Output) -> Self;

    /// Perform some amount of work, decrementing `fuel`.
    ///
    /// If `fuel` is greater than zero after the call, the merging is complete
    /// and one should call `done` to extract the merged results.
    fn work(
        &mut self,
        source1: &Output,
        source2: &Output,
        lower_val_bound: &Option<V>,
        fuel: &mut isize,
    );

    /// Extracts merged results.
    ///
    /// This method should only be called after `work` has been called and
    /// has not brought `fuel` to zero. Otherwise, the merge is still in
    /// progress.
    fn done(self) -> Output;
}
