use crate::{
    trace::{
        layers::{advance, column_layer::ColumnLayer, ordered::OrderedLayer, OrdOffset},
        Consumer, ValueConsumer,
    },
    utils::{assume, cursor_position_oob},
};
use std::{mem::MaybeUninit, ptr};

/// A [`Consumer`] implementation for [`OrderedLayer`]s that contain
/// [`ColumnLayer`]s
// TODO: Fuzz testing for correctness and drop safety
#[derive(Debug)]
pub struct OrderedLayerConsumer<K, V, R, O>
where
    K: 'static,
    V: 'static,
    R: 'static,
    O: OrdOffset,
{
    /// The position within `storage.keys` and `storage.offs` we're currently
    /// at. `position < storage.keys.len() && position < storage.offs.len()`
    /// unless the consumer is exhausted. To get the current key's range of
    /// values, use `storage.offs[position]..storage.offs[position + 1]`
    position: usize,
    /// The storage backing the consumer. Contains `MaybeUninit` values which
    /// (as the name suggests) may or may not be uninitialized. All keys (not
    /// offsets, those are only dropped when the consumer is) at
    /// positions greater than or equal to `position` are initialized. In other
    /// words, `storage.keys[..position]` are uninit and
    /// `storage.keys[position..]` are init. For values, all values at positions
    /// greater than or equal to `storage.offs[position]` are init and all
    /// others are uninit, meaning that
    /// `storage.values[..storage.offs[position]]` are uninit and
    /// `storage.values[storage.offs[position]..]` are init
    storage: OrderedLayer<MaybeUninit<K>, ColumnLayer<MaybeUninit<V>, MaybeUninit<R>>, O>,
}

impl<K, V, R, O> Consumer<K, V, R, ()> for OrderedLayerConsumer<K, V, R, O>
where
    O: OrdOffset,
{
    type ValueConsumer<'a> = OrderedLayerValues<'a, V, R>
    where
        Self: 'a;

    fn key_valid(&self) -> bool {
        // Safety: `position` is always less than or equal to `storage.keys.len()`,
        // knowing this this can allow the compiler to generate better code
        unsafe { assume(self.position <= self.storage.keys.len()) };

        self.position < self.storage.keys.len()
    }

    fn peek_key(&self) -> &K {
        if !self.key_valid() {
            cursor_position_oob(self.position, self.storage.keys.len());
        }

        // Safety: The current key is valid
        unsafe { self.storage.keys[self.position].assume_init_ref() }
    }

    fn next_key(&mut self) -> (K, Self::ValueConsumer<'_>) {
        let idx = self.position;
        if !self.key_valid() {
            cursor_position_oob(idx, self.storage.keys.len());
        }

        // We increment position before reading out the key and diff values
        self.position += 1;

        // Read out the key value
        debug_assert!(idx < self.storage.keys.len());
        let key = unsafe { self.storage.keys.get_unchecked(idx).assume_init_read() };

        (key, OrderedLayerValues::new(self))
    }

    fn seek_key(&mut self, key: &K)
    where
        K: Ord,
    {
        // If the consumer is exhausted, do nothing
        if !self.key_valid() {
            return;
        }

        let start_position = self.position;

        // Search for the given key
        let offset = advance(&self.storage.keys[start_position..], |k| unsafe {
            k.assume_init_ref().lt(key)
        });

        // Increment the offset before we drop the elements for panic safety
        self.position += offset;

        debug_assert!(
            start_position < self.storage.offs.len() && self.position < self.storage.offs.len(),
        );
        debug_assert!(
            start_position < self.storage.keys.len() && self.position < self.storage.keys.len(),
        );

        // We get the range of the skipped values by getting the offsets of the initial
        // `self.position` and the initial `self.position + offset`
        let value_start = unsafe { self.storage.offs.get_unchecked(start_position).into_usize() };
        let value_end = unsafe { self.storage.offs.get_unchecked(self.position).into_usize() };

        // Drop the skipped elements
        unsafe {
            // Drop the skipped keys
            ptr::drop_in_place(
                self.storage
                    .keys
                    .get_unchecked_mut(start_position..self.position)
                    as *mut [MaybeUninit<K>] as *mut [K],
            );

            // Drop the skipped values and diffs
            self.storage.vals.drop_range(value_start..value_end);
        }
    }
}

impl<K, V, R, O> From<OrderedLayer<K, ColumnLayer<V, R>, O>> for OrderedLayerConsumer<K, V, R, O>
where
    O: OrdOffset,
{
    fn from(layer: OrderedLayer<K, ColumnLayer<V, R>, O>) -> Self {
        Self {
            position: 0,
            storage: layer.into_uninit(),
        }
    }
}

impl<K, V, R, O> Drop for OrderedLayerConsumer<K, V, R, O>
where
    O: OrdOffset,
{
    fn drop(&mut self) {
        unsafe {
            // Drop any remaining keys
            ptr::drop_in_place(
                &mut self.storage.keys[self.position..] as *mut [MaybeUninit<K>] as *mut [K],
            );

            // Drop any remaining values & diffs
            self.storage
                .vals
                .drop_range(self.storage.offs[self.position].into_usize()..);
        }
    }
}

/// A [`ValueConsumer`] impl for the values yielded by [`OrderedLayerConsumer`]
#[derive(Debug)]
pub struct OrderedLayerValues<'a, V, R>
where
    V: 'static,
    R: 'static,
{
    // Invariant: `current <= end` (if `current == end` then the current consumer is exhausted)
    // Invariant: `current` will always be a valid index into `consumer`
    current: usize,
    end: usize,
    values: &'a mut ColumnLayer<MaybeUninit<V>, MaybeUninit<R>>,
}

impl<'a, V, R> OrderedLayerValues<'a, V, R> {
    pub fn new<K, O>(consumer: &'a mut OrderedLayerConsumer<K, V, R, O>) -> Self
    where
        O: OrdOffset,
    {
        unsafe {
            let position = consumer.position.into_usize();

            // Assert storage's invariants to the compiler
            consumer.storage.assume_invariants();
            // `position` will always be greater than one, so `position - 1` will never
            // underflow.
            // `position` is always inbounds of `consumer.storage.offs`
            assume(position >= 1 && position < consumer.storage.offs.len());

            // The consumer increments `value_position` before creating the value consumer
            // so we look at `value_position` for the value range's start
            debug_assert!(
                position < consumer.storage.offs.len()
                    && position - 1 < consumer.storage.offs.len()
            );
            let end = consumer.storage.offs.get_unchecked(position).into_usize();
            let current = consumer
                .storage
                .offs
                .get_unchecked(position - 1)
                .into_usize();

            // `current <= end` and both `current` and `end` are valid indices into
            // `consumer.storage.vals`
            assume(
                current <= end
                    && current < consumer.storage.vals.len()
                    && end <= consumer.storage.vals.len(),
            );

            Self {
                current,
                end,
                values: &mut consumer.storage.vals,
            }
        }
    }
}

impl<'a, V, R> ValueConsumer<'a, V, R, ()> for OrderedLayerValues<'a, V, R> {
    fn value_valid(&self) -> bool {
        // Safety: `current` is always less than or equal to `end`, knowing this this
        // can allow the compiler to generate better code
        unsafe { assume(self.current <= self.end) };

        self.current < self.end
    }

    fn next_value(&mut self) -> (V, R, ()) {
        if !self.value_valid() {
            invalid_value();
        }

        // Increment the current index before doing anything else
        let idx = self.current;
        self.current += 1;

        unsafe {
            // Elide bounds checking
            assume(idx < self.values.len());
            self.values.assume_invariants();

            // Read out the value and diff
            debug_assert!(idx < self.values.keys.len() && idx < self.values.diffs.len());
            let value = self.values.keys.get_unchecked(idx).assume_init_read();
            let diff = self.values.diffs.get_unchecked(idx).assume_init_read();

            (value, diff, ())
        }
    }

    fn remaining_values(&self) -> usize {
        // Safety: `current` is always less than or equal to `end`, knowing this this
        // can allow the compiler to generate better code
        unsafe { assume(self.current <= self.end) };

        self.end - self.current
    }
}

impl<V, R> Drop for OrderedLayerValues<'_, V, R> {
    fn drop(&mut self) {
        // Drop any unconsumed diffs and values
        unsafe { self.values.drop_range(self.current..self.end) }
    }
}

#[cold]
#[inline(never)]
fn invalid_value() -> ! {
    panic!("called `ValueConsumer::next_value()` on invalid value")
}
