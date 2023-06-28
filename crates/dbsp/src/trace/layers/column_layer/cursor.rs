use crate::{
    trace::layers::{advance, column_layer::ColumnLayer, retreat, Cursor},
    utils::cursor_position_oob,
    DBData, DBWeight,
};
use std::fmt::{self, Display};

/// A cursor for walking through an [`ColumnLayer`].
#[derive(Debug, Clone)]
pub struct ColumnLayerCursor<'s, K, R>
where
    K: Ord + Clone + 'static,
    R: Clone + 'static,
{
    // We represent current position of the cursor as isize, so we can use `-1`
    // to represent invalid cursor that rolled over the left bound of the range.
    pos: isize,
    storage: &'s ColumnLayer<K, R>,
    bounds: (usize, usize),
}

impl<'s, K, R> ColumnLayerCursor<'s, K, R>
where
    K: Ord + Clone,
    R: Clone,
{
    pub const fn new(pos: usize, storage: &'s ColumnLayer<K, R>, bounds: (usize, usize)) -> Self {
        Self {
            pos: pos as isize,
            storage,
            bounds,
        }
    }

    pub(super) const fn storage(&self) -> &'s ColumnLayer<K, R> {
        self.storage
    }

    pub(super) const fn bounds(&self) -> (usize, usize) {
        self.bounds
    }

    pub fn seek_key_with<P>(&mut self, predicate: P)
    where
        P: Fn(&K) -> bool,
    {
        unsafe { self.storage.assume_invariants() }
        if self.valid() {
            self.pos += advance(
                &self.storage.keys[self.pos as usize..self.bounds.1],
                predicate,
            ) as isize;
        }
    }

    pub fn seek_key_with_reverse<P>(&mut self, predicate: P)
    where
        P: Fn(&K) -> bool,
    {
        unsafe { self.storage.assume_invariants() }
        if self.valid() {
            self.pos -= retreat(
                &self.storage.keys[self.bounds.0..=self.pos as usize],
                predicate,
            ) as isize;
        }
    }

    pub fn current_key(&self) -> &K {
        debug_assert!(self.pos >= 0);
        &self.storage.keys[self.pos as usize]
    }

    pub fn current_diff(&self) -> &R {
        debug_assert!(self.pos >= 0);
        &self.storage.diffs[self.pos as usize]
    }
}

impl<'s, K, R> Cursor<'s> for ColumnLayerCursor<'s, K, R>
where
    K: Ord + Clone,
    R: Clone,
{
    type Key = K;

    type Item<'k> = (&'k K, &'k R)
    where
        Self: 'k;

    type ValueStorage = ();

    fn keys(&self) -> usize {
        self.bounds.1 - self.bounds.0
    }

    fn item(&self) -> Self::Item<'s> {
        // Elide extra bounds checking
        unsafe { self.storage.assume_invariants() }

        if self.pos >= self.storage.keys.len() as isize || self.pos < 0 {
            cursor_position_oob(self.pos, self.storage.keys.len());
        }

        (
            &self.storage.keys[self.pos as usize],
            &self.storage.diffs[self.pos as usize],
        )
    }

    fn values(&self) {}

    fn step(&mut self) {
        self.pos += 1;

        if self.pos >= self.bounds.1 as isize {
            self.pos = self.bounds.1 as isize;
        }
    }

    fn seek(&mut self, key: &Self::Key) {
        unsafe { self.storage.assume_invariants() }
        if self.valid() {
            self.pos += advance(&self.storage.keys[self.pos as usize..self.bounds.1], |k| {
                k.lt(key)
            }) as isize;
        }
    }

    fn valid(&self) -> bool {
        self.pos >= self.bounds.0 as isize && self.pos < self.bounds.1 as isize
    }

    fn rewind(&mut self) {
        self.pos = self.bounds.0 as isize;
    }

    fn position(&self) -> usize {
        self.pos as usize
    }

    fn reposition(&mut self, lower: usize, upper: usize) {
        self.pos = lower as isize;
        self.bounds = (lower, upper);
    }

    fn step_reverse(&mut self) {
        self.pos -= 1;

        if self.pos < self.bounds.0 as isize {
            self.pos = self.bounds.0 as isize - 1;
        }
    }

    fn seek_reverse(&mut self, key: &Self::Key) {
        unsafe { self.storage.assume_invariants() }
        if self.valid() {
            self.pos -= retreat(&self.storage.keys[self.bounds.0..=self.pos as usize], |k| {
                k.gt(key)
            }) as isize;
        }
    }

    fn fast_forward(&mut self) {
        self.pos = self.bounds.1 as isize - 1;
    }
}

impl<'a, K, R> Display for ColumnLayerCursor<'a, K, R>
where
    K: DBData,
    R: DBWeight,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut cursor: ColumnLayerCursor<K, R> = self.clone();

        while cursor.valid() {
            let (key, val) = cursor.item();
            writeln!(f, "{key:?} -> {val:?}")?;
            cursor.step();
        }

        Ok(())
    }
}
