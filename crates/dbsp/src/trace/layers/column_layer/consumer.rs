use crate::{
    trace::{
        layers::{advance, column_layer::ColumnLayer},
        Consumer, ValueConsumer,
    },
    utils::cursor_position_oob,
};
use std::mem::MaybeUninit;

// TODO: Fuzz for correctness and equivalence to Cursor
// TODO: Fuzz `.seek_key()`
// TODO: Fuzz for panic and drop safety

#[derive(Debug)]
pub struct ColumnLayerConsumer<K, R>
where
    K: 'static,
    R: 'static,
{
    // Invariant: `storage.len <= self.position`, if `storage.len == self.position` the cursor is
    // exhausted
    position: usize,
    // Invariant: `storage.keys[position..]` and `storage.diffs[position..]` are all valid
    storage: ColumnLayer<MaybeUninit<K>, MaybeUninit<R>>,
}

impl<K, R> Consumer<K, (), R, ()> for ColumnLayerConsumer<K, R> {
    type ValueConsumer<'a> = ColumnLayerValues<'a, K, R>
    where
        Self: 'a;

    fn key_valid(&self) -> bool {
        self.position < self.storage.len()
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
            cursor_position_oob(idx, self.storage.len());
        }

        // We increment position before reading out the key and diff values
        self.position += 1;

        // Copy out the key and diff
        let key = unsafe { self.storage.keys[idx].assume_init_read() };

        (key, ColumnLayerValues::new(self))
    }

    fn seek_key(&mut self, key: &K)
    where
        K: Ord,
    {
        let start_position = self.position;

        // Search for the given key
        let offset = advance(&self.storage.keys[start_position..], |k| unsafe {
            k.assume_init_ref().lt(key)
        });

        // Increment the offset before we drop the elements for panic safety
        self.position += offset;

        // Drop the skipped elements
        unsafe {
            self.storage
                .drop_range(start_position..start_position + offset);
        }
    }
}

impl<K, R> From<ColumnLayer<K, R>> for ColumnLayerConsumer<K, R> {
    #[inline]
    fn from(leaf: ColumnLayer<K, R>) -> Self {
        Self {
            position: 0,
            storage: leaf.into_uninit(),
        }
    }
}

impl<K, R> Drop for ColumnLayerConsumer<K, R> {
    fn drop(&mut self) {
        // Drop all remaining elements
        unsafe { self.storage.drop_range(self.position..) }
    }
}

#[derive(Debug)]
pub struct ColumnLayerValues<'a, K, R>
where
    K: 'static,
    R: 'static,
{
    done: bool,
    consumer: &'a mut ColumnLayerConsumer<K, R>,
}

impl<'a, K, R> ColumnLayerValues<'a, K, R> {
    #[inline]
    fn new(consumer: &'a mut ColumnLayerConsumer<K, R>) -> Self {
        Self {
            done: false,
            consumer,
        }
    }
}

impl<'a, K, R> ValueConsumer<'a, (), R, ()> for ColumnLayerValues<'a, K, R> {
    fn value_valid(&self) -> bool {
        !self.done
    }

    fn next_value(&mut self) -> ((), R, ()) {
        if self.done {
            value_already_consumed();
        }
        self.done = true;

        // The consumer increments `position` before creating the value consumer
        let idx = self.consumer.position - 1;
        let diff = unsafe { self.consumer.storage.diffs[idx].assume_init_read() };

        ((), diff, ())
    }

    fn remaining_values(&self) -> usize {
        !self.done as usize
    }
}

impl<'a, K, R> Drop for ColumnLayerValues<'a, K, R> {
    fn drop(&mut self) {
        // If the value consumer was never used, drop the difference value
        if !self.done {
            // The consumer increments `position` before creating the value consumer
            let idx = self.consumer.position - 1;

            // Drop the unused difference value
            unsafe { self.consumer.storage.diffs[idx].assume_init_drop() };
        }
    }
}

#[cold]
#[inline(never)]
fn value_already_consumed() -> ! {
    panic!("attempted to consume a value that was already consumed")
}
