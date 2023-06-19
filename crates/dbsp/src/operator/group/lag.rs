use bincode::{Decode, Encode};

use super::{GroupTransformer, Monotonicity};
use crate::{
    algebra::{HasZero, ZRingValue},
    trace::{
        cursor::{CursorPair, ReverseKeyCursor},
        Cursor,
    },
    DBData, DBWeight, IndexedZSet, OrdIndexedZSet, RootCircuit, Stream,
};
use std::{cmp::Ordering, marker::PhantomData};

const MAX_RETRACTIONS_CAPACITY: usize = 100_000usize;

impl<B> Stream<RootCircuit, B>
where
    B: IndexedZSet + Send + Encode + Decode,
{
    /// Lag operator matches each row in a group with a previous row in the
    /// same group.
    ///
    /// For each key in the input stream, it matches each associated value with
    /// a previous value (i.e., value with a smaller index according to
    /// ascending order of values), applies projection function `project` to
    /// it and outputs the input value along with this projection.
    ///
    /// # Arguments
    ///
    /// * `offset` - offset to the previous value.
    /// * `project` - projection function to apply to the delayed row.
    #[allow(clippy::type_complexity)]
    pub fn lag<OV, PF>(
        &self,
        offset: usize,
        project: PF,
    ) -> Stream<RootCircuit, OrdIndexedZSet<B::Key, (B::Val, OV), B::R>>
    where
        B::R: ZRingValue,
        OV: DBData,
        PF: Fn(Option<&B::Val>) -> OV + 'static,
    {
        self.group_transform(Lag::new(
            offset,
            true,
            project,
            |k1, k2| k1.cmp(k2),
            |v1, v2| v1.cmp(v2),
        ))
    }

    /// Lead operator matches each row in a group with a subsequent row in the
    /// same group.
    ///
    /// For each key in the input stream, matches each associated value with
    /// a subsequent value (i.e., value with a larger index according to
    /// ascending order of values), applies projection function `project` to
    /// it and outputs the input value along with this projection.
    ///
    /// # Arguments
    ///
    /// * `offset` - offset to the subsequent value.
    /// * `project` - projection function to apply to the subsequent row. The
    ///   argument is `None` for out-of-range values.
    #[allow(clippy::type_complexity)]
    pub fn lead<OV, PF>(
        &self,
        offset: usize,
        project: PF,
    ) -> Stream<RootCircuit, OrdIndexedZSet<B::Key, (B::Val, OV), B::R>>
    where
        B::R: ZRingValue,
        OV: DBData,
        PF: Fn(Option<&B::Val>) -> OV + 'static,
    {
        self.group_transform(Lag::new(
            offset,
            false,
            project,
            |k1, k2| k2.cmp(k1),
            |v1, v2| v2.cmp(v1),
        ))
    }
}

/// Implement both `lag` and `lead` operators.
struct Lag<I, O, R, PF, KCF, VCF> {
    name: String,
    lag: usize,
    /// `true` for `lag`, `false` for `lead`.
    asc: bool,
    project: PF,
    /// Array of retractions reused across multiple
    /// invocations of the operator.
    retractions: Vec<Retraction<(I, O), R>>,
    /// Index of the next key from `retractions` we expect to
    /// encounter.
    next_key: isize,
    /// Number of steps the input cursor took after encountering
    /// the previous key from `retractions`.
    offset_from_prev: usize,
    /// Key comparison function.  Set to `cmp` for ascending
    /// order and the reverse of `cmp` for descending order.
    key_cmp: KCF,
    /// Value comparison function.
    val_cmp: VCF,
    _phantom: PhantomData<(I, O, R)>,
}

impl<I, O, R, PF, KCF, VCF> Lag<I, O, R, PF, KCF, VCF>
where
    I: DBData,
    O: DBData,
    R: DBWeight + ZRingValue,
    PF: Fn(Option<&I>) -> O + 'static,
    KCF: Fn(&I, &I) -> Ordering + 'static,
    VCF: Fn(&O, &O) -> Ordering + 'static,
{
    fn new(lag: usize, asc: bool, project: PF, key_cmp: KCF, val_cmp: VCF) -> Self {
        Self {
            name: format!("{}({lag})", if asc { "lag" } else { "lead" }),
            lag,
            asc,
            project,
            retractions: Vec::new(),
            next_key: 0,
            offset_from_prev: 0,
            key_cmp,
            val_cmp,
            _phantom: PhantomData,
        }
    }

    fn push_retraction(&mut self, key: (I, O), w: R) {
        self.retractions.push(Retraction::new(key, w));
    }

    /// Move input `cursor` `n` steps back.  Keep track of keys in the
    /// `self.retractions` array encountered along the way, update their
    /// `offset` and `new_weight` fields.
    fn step_key_reverse_n<C>(&mut self, cursor: &mut C, mut steps: usize)
    where
        C: Cursor<I, (), (), R>,
    {
        while steps > 0 && cursor.key_valid() {
            // println!("reverse_n: next_key = {next_key}");
            step_key_reverse_skip_zeros(cursor);

            while self.next_key >= 0
                && (!cursor.key_valid()
                    || (self.key_cmp)(
                        cursor.key(),
                        &self.retractions[self.next_key as usize].key().0,
                    ) == Ordering::Less)
            {
                self.retractions[self.next_key as usize].offset = Some(None);
                self.next_key -= 1;
            }

            steps -= 1;
            self.offset_from_prev += 1;

            if cursor.key_valid()
                && self.next_key >= 0
                && cursor.key() == &self.retractions[self.next_key as usize].key().0
            {
                // println!("reverse_n: found {:?}, offset {offset_from_prev}", cursor.key());
                self.retractions[self.next_key as usize].offset = Some(Some(self.offset_from_prev));
                self.offset_from_prev = 0;
                self.retractions[self.next_key as usize].new_weight = cursor.weight();
                self.next_key -= 1;
            }
        }
    }

    /// Forward pass: compute keys that require updates,
    /// record these keys in `self.retractions`.
    ///
    /// # Example
    ///
    /// Consider `lag(3)` operator and assume that the input collection
    /// contains values `{1, 2, 3, 4, 6, 7, 8, 9, 10}` and the delta contains
    /// keys `{0, 5, 6}`.  The set of affected keys includes all keys in delta
    /// and for each key in delta `3` following keys in the output trace:
    /// `{0, 1, 2, 3, 5, 6, 7, 8, 9}`.
    fn compute_retractions<C1, C2>(&mut self, input_delta: &mut C1, output_trace: &mut C2)
    where
        C1: Cursor<I, (), (), R>,
        C2: Cursor<(I, O), (), (), R>,
    {
        self.retractions.clear();

        let mut offset = self.lag + 1;

        skip_zeros(output_trace);

        while input_delta.key_valid() && output_trace.key_valid() {
            // - `input_delta` points to the _next_ delta key to process.
            // - `offset` is the number of steps taken from the current key being processed.

            match (self.key_cmp)(&output_trace.key().0, input_delta.key()) {
                Ordering::Less => {
                    if offset <= self.lag {
                        // We are processing the previous key and haven't taken `lag`
                        // steps yet.
                        self.push_retraction(
                            output_trace.key().clone(),
                            output_trace.weight().neg(),
                        );

                        step_key_skip_zeros(output_trace);
                        offset += 1;
                    } else {
                        // Done processing previous key. Seek to the next key to process.
                        let delta_key = input_delta.key();
                        output_trace.seek_key_with(|(key, _)| {
                            (self.key_cmp)(key, delta_key) != Ordering::Less
                        });
                        skip_zeros(output_trace);
                    }
                }
                Ordering::Equal => {
                    // Reached the next key -- reset offset to 0 and move `input_delta` to the next
                    // key.
                    input_delta.step_key();
                    offset = 0;
                }
                Ordering::Greater => {
                    // Output cursor overshot next key.  This is only possible if the key is not
                    // present in the output trace.  Record the key as a zero-weight retraction,
                    // so we process it during reverse pass.
                    self.push_retraction(
                        (input_delta.key().clone(), (self.project)(None)),
                        HasZero::zero(),
                    );
                    input_delta.step_key();
                    offset = 1;
                }
            }
        }

        // println!("retractions before suffix: {:?}", retractions);

        // Finish processing the last key.
        while output_trace.key_valid() && offset <= self.lag {
            self.push_retraction(output_trace.key().clone(), output_trace.weight().neg());
            step_key_skip_zeros(output_trace);
            offset += 1;
        }

        // println!("retractions after output_trace: {:?}", retractions);

        // Record remaining keys in `input_delta`.
        while input_delta.key_valid() {
            self.push_retraction(
                (input_delta.key().clone(), (self.project)(None)),
                HasZero::zero(),
            );
            input_delta.step_key();
        }

        // println!("retractions: {:?}", self.retractions);
    }

    /// Backward pass: compute updated values for all keys in
    /// `self.retractions`.
    fn compute_updates<C, CB>(&mut self, input_cursor: &mut C, mut output_cb: CB)
    where
        C: Cursor<I, (), (), R>,
        CB: FnMut((I, O), R),
    {
        input_cursor.fast_forward_keys();
        // println!("current key after fast_forward: {:?}", input_cursor.key());

        // Index of the current key in the retractions array for which we are
        // computing update.  We traverse the array backward, hence the index
        // is relative to the end of the array.
        let mut current = self.retractions.len() as isize - 1;

        // The first key in the `retractions` array that hasn't been observed by the
        // cursor yet.  Once the key has been observed, it is assigned a number equal
        // to its distance from the previous key in `retractions` or `self.lag` if the
        // previous key is more than `self.lag` steps behind.
        self.next_key = current;
        self.offset_from_prev = 0;

        while current >= 0 {
            // println!("current: {current}");

            match self.retractions[current as usize].offset {
                Some(Some(offset)) => {
                    // println!("offset: {offset}");

                    // The key has been observed by the cursor and is known to be
                    // exactly `offset` steps ahead of the previous key.
                    // Move the cursor `offset` steps to point to the delayed record
                    // in the trace.

                    // update offsets in keys we enconter along the way.
                    self.step_key_reverse_n(input_cursor, offset);

                    // Output retraction and insertion in the correct order.

                    let new_val = (self.project)(input_cursor.get_key());
                    let ((key, old_val), old_weight) = self.retractions[current as usize]
                        .key_weight
                        .take()
                        .unwrap();
                    let new_weight = self.retractions[current as usize].new_weight.clone();

                    if old_weight.is_zero() {
                        if !new_weight.is_zero() {
                            output_cb((key, new_val), new_weight);
                        }
                    } else if new_weight.is_zero() {
                        output_cb((key.clone(), old_val), old_weight);
                    } else {
                        match (self.val_cmp)(&old_val, &new_val) {
                            Ordering::Greater => {
                                output_cb((key.clone(), old_val), old_weight);
                                output_cb((key, new_val), new_weight);
                            }
                            Ordering::Equal => {
                                let weight = new_weight + old_weight;
                                if !weight.is_zero() {
                                    output_cb((key, old_val), weight);
                                }
                            }
                            Ordering::Less => {
                                output_cb((key.clone(), new_val), new_weight);
                                output_cb((key, old_val), old_weight);
                            }
                        }
                    }

                    current -= 1;
                }
                Some(None) => {
                    // println!("offset: Some(None)");
                    // Key does not occur in the input trace.  Output retraction only.
                    let ((key, old_val), old_weight) = self.retractions[current as usize]
                        .key_weight
                        .take()
                        .unwrap();
                    if !old_weight.is_zero() {
                        output_cb((key, old_val), old_weight);
                    }
                    current -= 1;
                }
                None => {
                    // println!("offset: None");
                    // Key is ahead of the current location of the cursor.
                    // Seek to the key.
                    input_cursor.seek_key_reverse(&self.retractions[current as usize].key().0);
                    self.offset_from_prev = 0;

                    // We may have skipped over `current` and potentially multiple other keys.
                    // All skipped keys are not in the trace.  Output retractions for them and
                    // move `current` to the first key that has not been skipped.
                    while !input_cursor.key_valid()
                        || (self.key_cmp)(
                            input_cursor.key(),
                            &self.retractions[current as usize].key().0,
                        ) == Ordering::Less
                    {
                        // println!("retract {:?} (key: {:?})", &self.retractions[current as
                        // usize].key().0, input_cursor.key());
                        let (old_key_val, old_weight) = self.retractions[current as usize]
                            .key_weight
                            .take()
                            .unwrap();
                        if !old_weight.is_zero() {
                            output_cb(old_key_val, old_weight);
                        }
                        current -= 1;
                        if current < 0 {
                            break;
                        }
                    }

                    if current >= 0
                        && &self.retractions[current as usize].key().0 == input_cursor.key()
                    {
                        // The cursor points to current key.  Move it `lag` steps to point to
                        // the matching delayed record.
                        self.retractions[current as usize].offset = Some(Some(self.lag));
                        self.retractions[current as usize].new_weight = input_cursor.weight();
                        self.next_key = current - 1;
                    } else {
                        // New current key is again ahead of the cursor; we'll seek to it at the
                        // next iteration.
                        self.next_key = current;
                    }
                }
            }
        }

        // We want to reuse the allocation across multiple calls, but cap it
        // at `MAX_RETRACTIONS_CAPACITY`.
        if self.retractions.capacity() >= MAX_RETRACTIONS_CAPACITY {
            self.retractions = Vec::new();
        }
    }
}

/// Tracks per-key state used by the `lag` operator.
///
/// During forward pass:
/// * Tracks keys affected by the input delta whose must be updated.
/// * Records retractions to be applied to the output collection.
///
/// During backward pass:
/// * Records the new weight of each affected key.
/// * Records offset from the previous encountered affected key. Determines how
///   far to move the cursor to reach the matching delayed record.
#[derive(Debug)]
struct Retraction<K, R> {
    /// Record to retract from the output collection.  This field is populated
    /// during forward pass.
    key_weight: Option<(K, R)>,
    /// The new weight of the key in the input collection.  Populated during
    /// backward pass.
    new_weight: R,
    /// Offset from the previous key in the retractions array.  Populated during
    /// backward pass.
    ///
    /// * `None` - key hasn't been encountered yet
    /// * `Some(None)` - key does not occur in the input trace.
    /// * `Some(Some(n))` - key is `n` steps away from the previous key from
    ///   `retractions` that is present in the input trace.
    offset: Option<Option<usize>>,
}

impl<K, R> Retraction<K, R> {
    fn new(key: K, weight: R) -> Self
    where
        R: DBWeight,
    {
        Self {
            key_weight: Some((key, weight)),
            new_weight: HasZero::zero(),
            offset: None,
        }
    }

    fn key(&self) -> &K {
        &self.key_weight.as_ref().unwrap().0
    }
}

impl<I, O, R, PF, KCF, VCF> GroupTransformer<I, (I, O), R> for Lag<I, O, R, PF, KCF, VCF>
where
    I: DBData,
    O: DBData,
    R: DBWeight + ZRingValue,
    PF: Fn(Option<&I>) -> O + 'static,
    KCF: Fn(&I, &I) -> Ordering + 'static,
    VCF: Fn(&O, &O) -> Ordering + 'static,
{
    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn monotonicity(&self) -> Monotonicity {
        // Since outputs are produced during the second (backward) pass,
        // `lag` produces outputs in the descending order, while `lead` -- in
        // ascending.
        if self.asc {
            Monotonicity::Descending
        } else {
            Monotonicity::Ascending
        }
    }

    fn transform<C1, C2, C3, CB>(
        &mut self,
        input_delta: &mut C1,
        input_trace: &mut C2,
        output_trace: &mut C3,
        output_cb: CB,
    ) where
        C1: Cursor<I, (), (), R>,
        C2: Cursor<I, (), (), R>,
        C3: Cursor<(I, O), (), (), R>,
        CB: FnMut((I, O), R),
    {
        /*
        {
            println!("input_delta:");
            while input_delta.key_valid() {
                let w = input_delta.weight();
                println!("    {:?} -> {:?}", input_delta.key(), w);
                input_delta.step_key();
            }
            input_delta.rewind_keys();
        }
        {
            println!("input_trace:");
            while input_trace.key_valid() {
                let w = input_trace.weight();
                println!("    {:?} -> {:?}", input_trace.key(), w);
                input_trace.step_key();
            }
            input_trace.rewind_keys();
        }
        {
            println!("output_trace:");
            while output_trace.key_valid() {
                let w = output_trace.weight();
                println!("    {:?} -> {:?}", output_trace.key(), w);
                output_trace.step_key();
            }
            output_trace.rewind_keys();
        }
        */

        if self.asc {
            self.compute_retractions(input_delta, output_trace);
            self.compute_updates(&mut CursorPair::new(input_delta, input_trace), output_cb);
        } else {
            self.compute_retractions(
                &mut ReverseKeyCursor::new(input_delta),
                &mut ReverseKeyCursor::new(output_trace),
            );
            self.compute_updates(
                &mut ReverseKeyCursor::new(&mut CursorPair::new(input_delta, input_trace)),
                output_cb,
            );
        }
    }
}

fn step_key_skip_zeros<C, I, R>(cursor: &mut C)
where
    C: Cursor<I, (), (), R>,
    R: DBWeight,
{
    cursor.step_key();
    skip_zeros(cursor)
}

fn step_key_reverse_skip_zeros<C, I, R>(cursor: &mut C)
where
    C: Cursor<I, (), (), R>,
    R: DBWeight,
{
    cursor.step_key_reverse();
    skip_zeros_reverse(cursor)
}

fn skip_zeros<C, I, R>(cursor: &mut C)
where
    C: Cursor<I, (), (), R>,
    R: DBWeight,
{
    while cursor.key_valid() && cursor.weight().is_zero() {
        cursor.step_key();
    }
}

fn skip_zeros_reverse<C, I, R>(cursor: &mut C)
where
    C: Cursor<I, (), (), R>,
    R: DBWeight,
{
    while cursor.key_valid() && cursor.weight().is_zero() {
        cursor.step_key_reverse();
    }
}
