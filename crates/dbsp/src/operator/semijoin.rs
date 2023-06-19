use bincode::{Decode, Encode};

use crate::{
    algebra::{MulByRef, ZSet},
    circuit::{
        operator_traits::{BinaryOperator, Operator},
        Scope,
    },
    circuit::{GlobalNodeId, OwnershipPreference},
    circuit_cache_key,
    trace::{Batch, BatchReader, Builder, Consumer, Cursor, ValueConsumer},
    Circuit, Stream,
};
use std::{
    borrow::Cow,
    cmp::{min, Ordering},
    marker::PhantomData,
};

circuit_cache_key!(SemijoinId<C, D>((GlobalNodeId, GlobalNodeId) => Stream<C, D>));

impl<C, Pairs> Stream<C, Pairs>
where
    C: Circuit,
    Pairs: Encode + Decode,
{
    /// Semijoin two streams of batches.
    ///
    /// The operator takes two streams of batches indexed with the same key type
    /// (`Pairs::Key = Keys::Key`) and outputs a stream obtained by joining each
    /// pair of inputs.
    ///
    /// Input streams will typically be produced by [`Stream::index()`] or
    /// [`Stream::index_with()`]
    ///
    /// #### Type arguments
    ///
    /// * `Pairs` - batch type in the first input stream.
    /// * `Keys` - batch type in the second input stream.
    /// * `Out` - output Z-set type.
    pub fn semijoin_stream<Keys, Out>(&self, keys: &Stream<C, Keys>) -> Stream<C, Out>
    where
        // TODO: Associated type bounds (rust/#52662) really simplify things
        // TODO: Allow non-unit timestamps
        Pairs: Batch<Time = ()> + Send,
        Keys: Batch<Key = Pairs::Key, Val = (), Time = ()> + Send + Encode + Decode,
        // TODO: Should this be `IndexedZSet<Key = Pairs::Key, Val = Pairs::Val>`?
        Out: ZSet<Key = (Pairs::Key, Pairs::Val)>,
        Pairs::R: MulByRef<Keys::R, Output = Out::R>,
    {
        self.circuit()
            .cache_get_or_insert_with(
                SemijoinId::new((self.origin_node_id().clone(), keys.origin_node_id().clone())),
                move || {
                    self.circuit()
                        .add_binary_operator(SemiJoinStream::new(), &self.shard(), &keys.shard())
                        // This is valid because both of the input streams are sharded. Since this
                        // operator doesn't transform the keys of the inputs
                        // any, the stream they produce is automatically
                        // sharded by the same metric that the inputs are
                        .mark_sharded()
                },
            )
            .clone()
    }
}

/// Semijoin two streams of batches, see [`Stream::semijoin_stream`]
pub struct SemiJoinStream<Pairs, Keys, Out> {
    _types: PhantomData<(Pairs, Keys, Out)>,
}

impl<Pairs, Keys, Out> SemiJoinStream<Pairs, Keys, Out> {
    pub const fn new() -> Self {
        Self {
            _types: PhantomData,
        }
    }
}

impl<Pairs, Keys, Out> Operator for SemiJoinStream<Pairs, Keys, Out>
where
    Pairs: 'static,
    Keys: 'static,
    Out: 'static,
{
    fn name(&self) -> Cow<'static, str> {
        Cow::Borrowed("SemiJoinStream")
    }

    fn fixedpoint(&self, _scope: Scope) -> bool {
        true
    }
}

impl<Pairs, Keys, Out> BinaryOperator<Pairs, Keys, Out> for SemiJoinStream<Pairs, Keys, Out>
where
    Pairs: BatchReader<Time = ()>,
    Keys: BatchReader<Key = Pairs::Key, Val = (), Time = ()>,
    Out: ZSet<Key = (Pairs::Key, Pairs::Val)>,
    Pairs::R: MulByRef<Keys::R, Output = Out::R>,
{
    fn eval(&mut self, pairs: &Pairs, keys: &Keys) -> Out {
        let mut pair_cursor = pairs.cursor();
        let mut key_cursor = keys.cursor();

        // Choose capacity heuristically.
        let mut builder = Out::Builder::with_capacity((), min(pairs.len(), keys.len()));

        // While both keys are valid
        while key_cursor.key_valid() && pair_cursor.key_valid() {
            match key_cursor.key().cmp(pair_cursor.key()) {
                // Match up both the cursors
                Ordering::Less => key_cursor.seek_key(pair_cursor.key()),
                Ordering::Greater => pair_cursor.seek_key(key_cursor.key()),

                Ordering::Equal => {
                    // TODO: Can the value of `()` ever be invalid? Do we need an `if
                    // key_cursor.val_valid()` check?
                    let key_weight = key_cursor.weight();
                    while pair_cursor.val_valid() {
                        // Get the weight of the output kv pair by multiplying them together
                        let pair_weight = pair_cursor.weight();
                        let kv_weight = pair_weight.mul_by_ref(&key_weight);

                        // Add to our output batch
                        builder.push((
                            Out::item_from(
                                (pair_cursor.key().clone(), pair_cursor.val().clone()),
                                (),
                            ),
                            kv_weight,
                        ));
                        pair_cursor.step_val();
                    }

                    pair_cursor.step_key();
                    key_cursor.step_key();
                }
            }
        }

        // Create the output stream
        builder.done()
    }

    fn eval_owned(&mut self, pairs: Pairs, keys: Keys) -> Out {
        // Choose capacity heuristically.
        let mut builder = Out::Builder::with_capacity((), min(pairs.len(), keys.len()));

        let mut pairs = pairs.consumer();
        let mut keys = keys.consumer();

        // While both keys are valid
        while keys.key_valid() && pairs.key_valid() {
            match keys.peek_key().cmp(pairs.peek_key()) {
                // Match up both the cursors
                Ordering::Less => keys.seek_key(pairs.peek_key()),
                Ordering::Greater => pairs.seek_key(keys.peek_key()),

                Ordering::Equal => {
                    // Get the key's weight
                    let (_, mut key_value) = keys.next_key();
                    debug_assert!(key_value.value_valid());
                    let ((), key_weight, ()) = key_value.next_value();

                    // TODO: We could specialize for when pairs has a single value to add the
                    // weights by value and to not clone pair_key

                    let (pair_key, mut pair_values) = pairs.next_key();
                    while pair_values.value_valid() {
                        // Get the weight of the output kv pair by multiplying them together
                        let (pair_value, pair_weight, ()) = pair_values.next_value();
                        let kv_weight = pair_weight.mul_by_ref(&key_weight);

                        // Add to our output batch
                        builder.push((
                            Out::item_from((pair_key.clone(), pair_value), ()),
                            kv_weight,
                        ));
                    }
                }
            }
        }

        // Create the output stream
        builder.done()
    }

    fn eval_owned_and_ref(&mut self, pairs: Pairs, keys: &Keys) -> Out {
        // Choose capacity heuristically.
        let mut builder = Out::Builder::with_capacity((), min(pairs.len(), keys.len()));

        let mut pairs = pairs.consumer();
        let mut keys = keys.cursor();

        // While both keys are valid
        while keys.key_valid() && pairs.key_valid() {
            match keys.key().cmp(pairs.peek_key()) {
                // Match up both the cursors
                Ordering::Less => keys.seek_key(pairs.peek_key()),
                Ordering::Greater => pairs.seek_key(keys.key()),

                Ordering::Equal => {
                    // Get the key's weight and its weight
                    let key_weight = keys.weight();

                    let (pair_key, mut pair_values) = pairs.next_key();
                    while pair_values.value_valid() {
                        // Get the weight of the output kv pair by multiplying them together
                        let (pair_value, pair_weight, ()) = pair_values.next_value();
                        let kv_weight = pair_weight.mul_by_ref(&key_weight);

                        // Add to our output batch
                        builder.push((
                            Out::item_from((pair_key.clone(), pair_value), ()),
                            kv_weight,
                        ));
                    }
                }
            }
        }

        // Create the output stream
        builder.done()
    }

    fn input_preference(&self) -> (OwnershipPreference, OwnershipPreference) {
        // We get bigger gains from pairs being owned than from keys being owned
        (
            OwnershipPreference::WEAKLY_PREFER_OWNED,
            OwnershipPreference::PREFER_OWNED,
        )
    }
}
