//! Reference implementation of batch and trace traits for use in testing.
//!
//! So far, only methods/traits used in tests have been implemented.

use super::{
    Activator, AntichainRef, Batch, BatchReader, Batcher, Builder, Consumer, Cursor, Merger, Trace,
    ValueConsumer,
};
use crate::{algebra::HasZero, utils::VecExt, DBData, DBTimestamp, DBWeight, NumEntries};
use bincode::Decode;
use bincode::Encode;
use rand::seq::IteratorRandom;
use rand::thread_rng;
use rand::Rng;
use rand::SeedableRng;
use rand_chacha::ChaChaRng;
use size_of::SizeOf;
use std::{
    cmp::max,
    collections::{BTreeMap, BTreeSet},
    marker::PhantomData,
};

/// Convert any batch into a vector of tuples.
pub fn batch_to_tuples<B>(batch: &B) -> Vec<((B::Key, B::Val, B::Time), B::R)>
where
    B: BatchReader,
{
    let mut result = Vec::new();
    let mut cursor = batch.cursor();

    while cursor.key_valid() {
        while cursor.val_valid() {
            let key = cursor.key().clone();
            let val = cursor.val().clone();
            cursor
                .map_times(|t, r| result.push(((key.clone(), val.clone(), t.clone()), r.clone())));
            cursor.step_val();
        }
        cursor.step_key()
    }

    // Check that cursor iterates over keys, values, and times in order.
    assert!(result.is_sorted_by(|(k1, _), (k2, _)| Some(k1.cmp(k2))));

    <TestBatch<B::Key, B::Val, B::Time, B::R>>::from_data(&result)
        .data
        .into_iter()
        .collect::<Vec<_>>()
}

/// Convert any batch into a vector of tuples.
pub fn batch_to_tuples_reverse_vals<B>(batch: &B) -> Vec<((B::Key, B::Val, B::Time), B::R)>
where
    B: BatchReader,
{
    let mut result = Vec::new();
    let mut cursor = batch.cursor();

    while cursor.key_valid() {
        cursor.fast_forward_vals();

        while cursor.val_valid() {
            let key = cursor.key().clone();
            let val = cursor.val().clone();
            cursor
                .map_times(|t, r| result.push(((key.clone(), val.clone(), t.clone()), r.clone())));
            cursor.step_val_reverse();
        }
        cursor.step_key()
    }

    <TestBatch<B::Key, B::Val, B::Time, B::R>>::from_data(&result)
        .data
        .into_iter()
        .collect::<Vec<_>>()
}

pub fn batch_to_tuples_reverse_keys<B>(batch: &B) -> Vec<((B::Key, B::Val, B::Time), B::R)>
where
    B: BatchReader,
{
    let mut result = Vec::new();
    let mut cursor = batch.cursor();

    cursor.fast_forward_keys();

    while cursor.key_valid() {
        while cursor.val_valid() {
            let key = cursor.key().clone();
            let val = cursor.val().clone();
            cursor
                .map_times(|t, r| result.push(((key.clone(), val.clone(), t.clone()), r.clone())));
            cursor.step_val();
        }
        cursor.step_key_reverse()
    }

    <TestBatch<B::Key, B::Val, B::Time, B::R>>::from_data(&result)
        .data
        .into_iter()
        .collect::<Vec<_>>()
}

pub fn batch_to_tuples_reverse_keys_vals<B>(batch: &B) -> Vec<((B::Key, B::Val, B::Time), B::R)>
where
    B: BatchReader,
{
    let mut result = Vec::new();
    let mut cursor = batch.cursor();

    cursor.fast_forward_keys();

    while cursor.key_valid() {
        cursor.fast_forward_vals();

        while cursor.val_valid() {
            let key = cursor.key().clone();
            let val = cursor.val().clone();
            cursor
                .map_times(|t, r| result.push(((key.clone(), val.clone(), t.clone()), r.clone())));
            cursor.step_val_reverse();
        }
        cursor.step_key_reverse()
    }

    <TestBatch<B::Key, B::Val, B::Time, B::R>>::from_data(&result)
        .data
        .into_iter()
        .collect::<Vec<_>>()
}

/// Panic if `batch1` and `batch2` contain different tuples.
pub fn assert_batch_eq<B1, B2>(batch1: &B1, batch2: &B2)
where
    B1: BatchReader,
    B2: BatchReader<Key = B1::Key, Val = B1::Val, Time = B1::Time, R = B1::R>,
{
    let tuples1 = batch_to_tuples(batch1);
    assert_eq!(tuples1, batch_to_tuples_reverse_vals(batch1));
    assert_eq!(tuples1, batch_to_tuples_reverse_keys(batch1));
    assert_eq!(tuples1, batch_to_tuples_reverse_keys_vals(batch1));

    let tuples2 = batch_to_tuples(batch2);
    assert_eq!(tuples2, batch_to_tuples_reverse_vals(batch2));

    assert_eq!(tuples1, tuples2);
}

pub fn assert_trace_eq<T1, T2>(trace1: &T1, trace2: &T2)
where
    T1: Trace,
    T2: Trace<Key = T1::Key, Val = T1::Val, Time = T1::Time, R = T1::R>,
{
    let mut tuples1 = batch_to_tuples(trace1);
    assert_eq!(tuples1, batch_to_tuples_reverse_vals(trace1));
    assert_eq!(tuples1, batch_to_tuples_reverse_keys(trace1));
    assert_eq!(tuples1, batch_to_tuples_reverse_keys_vals(trace1));

    if let Some(bound) = trace1.lower_value_bound() {
        tuples1.retain(|((_k, v, _t), _r)| v >= bound);
    }

    let mut tuples2 = batch_to_tuples(trace2);
    assert_eq!(tuples2, batch_to_tuples_reverse_vals(trace2));

    if let Some(bound) = trace2.lower_value_bound() {
        tuples2.retain(|((_k, v, _t), _r)| v >= bound);
    }

    assert_eq!(tuples1, tuples2);
}

pub fn assert_batch_cursors_eq<C, B>(mut cursor: C, ref_batch: &B, seed: u64)
where
    B: BatchReader,
    C: Cursor<B::Key, B::Val, B::Time, B::R>,
{
    // Extract all key/value pairs.
    let mut tuples = batch_to_tuples(ref_batch)
        .into_iter()
        .map(|((k, v, _t), _r)| (k, v))
        .collect::<Vec<_>>();
    tuples.dedup();

    // Randomly sample 1/3 of the pairs.
    let sample_len = tuples.len() / 3;
    let mut rng = ChaChaRng::seed_from_u64(seed);
    let mut sample: Vec<(B::Key, B::Val)> =
        tuples.into_iter().choose_multiple(&mut rng, sample_len);
    sample.sort();
    sample.dedup();

    let mut sample_map = <BTreeMap<B::Key, Vec<B::Val>>>::new();

    for (k, v) in sample.into_iter() {
        sample_map.entry(k).or_default().push(v);
    }

    for key in sample_map.keys() {
        cursor.seek_key(key);
        assert!(cursor.key_valid());
        assert_eq!(cursor.key(), key);

        for val in sample_map.get(key).unwrap().iter() {
            cursor.seek_val(val);
            assert!(cursor.val_valid());
            assert_eq!(cursor.val(), val);
        }

        cursor.fast_forward_vals();
        assert!(cursor.val_valid());

        for val in sample_map.get(key).unwrap().iter().rev() {
            cursor.seek_val_reverse(val);
            assert!(cursor.val_valid());
            assert_eq!(cursor.val(), val);
        }
    }

    cursor.fast_forward_keys();

    for key in sample_map.keys().rev() {
        cursor.seek_key_reverse(key);
        assert!(cursor.key_valid());
        assert_eq!(cursor.key(), key);

        for val in sample_map.get(key).unwrap().iter() {
            cursor.seek_val(val);
            assert!(cursor.val_valid());
            assert_eq!(cursor.val(), val);
        }

        cursor.fast_forward_vals();
        assert!(cursor.val_valid());

        for val in sample_map.get(key).unwrap().iter().rev() {
            cursor.seek_val_reverse(val);
            assert!(cursor.val_valid());
            assert_eq!(cursor.val(), val);
        }
    }
}

/// Inefficient but simple batch implementation as a B-tree map.
#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode, SizeOf)]
pub struct TestBatch<K, V, T, R>
where
    K: Ord,
    V: Ord,
    T: Ord,
{
    data: BTreeMap<(K, V, T), R>,
    lower_key_bound: Option<K>,
    lower_val_bound: Option<V>,
}

impl<K, V, T, R> NumEntries for TestBatch<K, V, T, R>
where
    K: Ord,
    V: Ord,
    T: Ord,
{
    const CONST_NUM_ENTRIES: Option<usize> = None;

    fn num_entries_shallow(&self) -> usize {
        self.data.len()
    }

    fn num_entries_deep(&self) -> usize {
        self.data.len()
    }
}

impl<K, V, T, R> TestBatch<K, V, T, R>
where
    K: DBData,
    V: DBData,
    T: DBTimestamp,
    R: DBWeight,
{
    /// Create batch from sorted or unsorted tuples.
    pub fn from_data(records: &[((K, V, T), R)]) -> Self {
        let mut data: BTreeMap<(K, V, T), R> = BTreeMap::new();

        for ((k, v, t), r) in records.iter() {
            data.entry((k.clone(), v.clone(), t.clone()))
                .or_insert_with(HasZero::zero)
                .add_assign_by_ref(r);
        }

        data.retain(|_, r| !r.is_zero());

        Self {
            data,
            lower_key_bound: None,
            lower_val_bound: None,
        }
    }
}

pub struct TestBatchConsumer<K, V, T, R> {
    _phantom: PhantomData<(K, V, T, R)>,
}
pub struct TestBatchValueConsumer<V, T, R> {
    _phantom: PhantomData<(V, T, R)>,
}

impl<'a, V, T, R> ValueConsumer<'a, V, R, T> for TestBatchValueConsumer<V, T, R>
where
    V: DBData,
    T: DBTimestamp,
    R: DBWeight,
{
    fn value_valid(&self) -> bool {
        todo!()
    }
    fn next_value(&mut self) -> (V, R, T) {
        todo!()
    }
    fn remaining_values(&self) -> usize {
        todo!()
    }
}

impl<K, V, T, R> Consumer<K, V, R, T> for TestBatchConsumer<K, V, T, R>
where
    K: DBData,
    V: DBData,
    T: DBTimestamp,
    R: DBWeight,
{
    type ValueConsumer<'a> = TestBatchValueConsumer<V, T, R>;

    fn key_valid(&self) -> bool {
        todo!()
    }
    fn peek_key(&self) -> &K {
        todo!()
    }
    fn next_key(&mut self) -> (K, Self::ValueConsumer<'_>) {
        todo!()
    }
    /// Advances the cursor to the specified value
    fn seek_key(&mut self, _key: &K)
    where
        K: Ord,
    {
        todo!()
    }
}

#[derive(SizeOf, Encode, Decode)]
pub struct TestBatchBatcher<K, V, T, R>
where
    K: Ord,
    V: Ord,
    T: Ord,
{
    time: T,
    result: TestBatch<K, V, T, R>,
}

impl<K, V, T, R> Batcher<(K, V), T, R, TestBatch<K, V, T, R>> for TestBatchBatcher<K, V, T, R>
where
    K: DBData,
    V: DBData,
    T: DBTimestamp,
    R: DBWeight,
{
    fn new_batcher(time: T) -> Self {
        Self {
            time,
            result: TestBatch::new(None),
        }
    }

    fn push_batch(&mut self, batch: &mut Vec<((K, V), R)>) {
        for ((k, v), r) in batch.iter() {
            self.result
                .data
                .entry((k.clone(), v.clone(), self.time.clone()))
                .or_insert_with(HasZero::zero)
                .add_assign_by_ref(r);
        }
    }

    fn push_consolidated_batch(&mut self, batch: &mut Vec<((K, V), R)>) {
        self.push_batch(batch);
    }

    fn tuples(&self) -> usize {
        self.result.len()
    }

    fn seal(mut self) -> TestBatch<K, V, T, R> {
        self.result.data.retain(|_, r| !r.is_zero());
        self.result
    }
}

#[derive(SizeOf)]
pub struct TestBatchBuilder<K, V, T, R>
where
    K: Ord,
    V: Ord,
    T: Ord,
{
    time: T,
    result: TestBatch<K, V, T, R>,
}

impl<K, V, T, R> Builder<(K, V), T, R, TestBatch<K, V, T, R>> for TestBatchBuilder<K, V, T, R>
where
    K: DBData,
    V: DBData,
    T: DBTimestamp,
    R: DBWeight,
{
    fn new_builder(time: T) -> Self {
        Self {
            time,
            result: TestBatch::new(None),
        }
    }

    fn with_capacity(time: T, _cap: usize) -> Self {
        Self::new_builder(time)
    }

    fn push(&mut self, element: ((K, V), R)) {
        let ((k, v), r) = element;

        self.result
            .data
            .entry((k, v, self.time.clone()))
            .or_insert_with(HasZero::zero)
            .add_assign_by_ref(&r)
    }

    fn reserve(&mut self, _additional: usize) {}

    fn done(mut self) -> TestBatch<K, V, T, R> {
        self.result.data.retain(|_, r| !r.is_zero());
        self.result
    }
}

#[derive(SizeOf)]
pub struct TestBatchMerger<K, V, T, R>
where
    K: Ord,
    V: Ord,
    T: Ord,
{
    result: TestBatch<K, V, T, R>,
}

impl<K, V, T, R> Merger<K, V, T, R, TestBatch<K, V, T, R>> for TestBatchMerger<K, V, T, R>
where
    K: DBData,
    V: DBData,
    T: DBTimestamp,
    R: DBWeight,
{
    fn new_merger(source1: &TestBatch<K, V, T, R>, source2: &TestBatch<K, V, T, R>) -> Self {
        let data = source1
            .data
            .iter()
            .chain(source2.data.iter())
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect::<Vec<_>>();
        let result = TestBatch::from_data(&data);

        Self { result }
    }

    fn work(
        &mut self,
        _source1: &TestBatch<K, V, T, R>,
        _source2: &TestBatch<K, V, T, R>,
        _lower_val_bound: &Option<V>,
        _fuel: &mut isize,
    ) {
    }

    fn done(self) -> TestBatch<K, V, T, R> {
        self.result
    }
}

pub struct TestBatchCursor<K, V, T, R> {
    // All tuples in the batch flattened into a vector.
    data: Vec<((K, V, T), R)>,
    // Index that the cursor currently points to.
    index: usize,
    // Set to true after calling `step_val` on the last
    // value for a key.
    val_valid: bool,
}

impl<K, V, T, R> TestBatchCursor<K, V, T, R>
where
    K: DBData,
    V: DBData,
    T: DBTimestamp,
    R: DBWeight,
{
    fn new(batch: &TestBatch<K, V, T, R>) -> Self {
        Self {
            data: batch
                .data
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect::<Vec<_>>(),
            index: 0,
            val_valid: !batch.data.is_empty(),
        }
    }
}

impl<K, V, T, R> Cursor<K, V, T, R> for TestBatchCursor<K, V, T, R>
where
    K: DBData,
    V: DBData,
    T: DBTimestamp,
    R: DBWeight,
{
    fn key_valid(&self) -> bool {
        self.index < self.data.len()
    }

    fn val_valid(&self) -> bool {
        self.key_valid() && self.val_valid
    }

    fn key(&self) -> &K {
        &self.data[self.index].0 .0
    }

    fn val(&self) -> &V {
        &self.data[self.index].0 .1
    }

    fn fold_times<F, U>(&mut self, init: U, mut fold: F) -> U
    where
        F: FnMut(U, &T, &R) -> U,
    {
        let mut acc = init;

        let current_key = self.data[self.index].0 .0.clone();
        let current_val = self.data[self.index].0 .1.clone();

        let mut index = self.index;

        while index < self.data.len()
            && self.data[index].0 .0 == current_key
            && self.data[index].0 .1 == current_val
        {
            acc = fold(acc, &self.data[index].0 .2, &self.data[index].1);
            index += 1;
        }

        acc
    }

    fn fold_times_through<F, U>(&mut self, _upper: &T, _init: U, _fold: F) -> U
    where
        F: FnMut(U, &T, &R) -> U,
    {
        todo!()
    }

    fn weight(&mut self) -> R
    where
        T: PartialEq<()>,
    {
        self.data[self.index].1.clone()
    }

    fn step_key(&mut self) {
        let current_key = self.data[self.index].0 .0.clone();

        while self.index < self.data.len() && self.data[self.index].0 .0 == current_key {
            self.index += 1;
        }

        self.val_valid = true;
    }

    fn step_key_reverse(&mut self) {
        todo!()
    }

    fn seek_key(&mut self, key: &K) {
        while self.index < self.data.len() && &self.data[self.index].0 .0 < key {
            self.index += 1;
        }
    }

    fn seek_key_with<P>(&mut self, _predicate: P)
    where
        P: Fn(&K) -> bool + Clone,
    {
        todo!()
    }

    fn seek_key_with_reverse<P>(&mut self, _predicate: P)
    where
        P: Fn(&K) -> bool + Clone,
    {
        todo!()
    }

    fn seek_key_reverse(&mut self, _key: &K) {
        todo!()
    }

    fn step_val(&mut self) {
        let current_key = self.data[self.index].0 .0.clone();
        let current_val = self.data[self.index].0 .1.clone();

        while self.index < self.data.len() && self.data[self.index].0 .1 == current_val {
            if self.index + 1 < self.data.len() && self.data[self.index + 1].0 .0 == current_key {
                self.index += 1;
            } else {
                self.val_valid = false;
                return;
            }
        }
    }

    fn seek_val(&mut self, val: &V) {
        loop {
            if !self.val_valid() || self.val() >= val {
                break;
            }
            self.step_val();
        }
    }

    fn seek_val_with<P>(&mut self, _predicate: P)
    where
        P: Fn(&V) -> bool + Clone,
    {
        todo!()
    }

    fn rewind_keys(&mut self) {
        self.index = 0;
    }

    fn fast_forward_keys(&mut self) {
        todo!()
    }

    fn rewind_vals(&mut self) {
        todo!()
    }

    fn step_val_reverse(&mut self) {
        let current_key = self.data[self.index].0 .0.clone();
        let current_val = self.data[self.index].0 .1.clone();

        while self.data[self.index].0 .1 == current_val {
            if self.index > 0 && self.data[self.index - 1].0 .0 == current_key {
                self.index -= 1;
            } else {
                self.val_valid = false;
                return;
            }
        }

        let current_val = self.data[self.index].0 .1.clone();

        while self.index > 0
            && self.data[self.index - 1].0 .0 == current_key
            && self.data[self.index - 1].0 .1 == current_val
        {
            self.index -= 1;
        }
    }

    fn seek_val_reverse(&mut self, val: &V) {
        loop {
            if !self.val_valid() || self.val() >= val {
                break;
            }
            self.step_val_reverse();
        }
    }

    fn seek_val_with_reverse<P>(&mut self, _predicate: P)
    where
        P: Fn(&V) -> bool + Clone,
    {
        todo!()
    }

    fn fast_forward_vals(&mut self) {
        let current_key = self.data[self.index].0 .0.clone();

        while self.index + 1 < self.data.len() && self.data[self.index + 1].0 .0 == current_key {
            self.index += 1;
        }

        let current_val = self.data[self.index].0 .1.clone();

        while self.index > 0
            && self.data[self.index - 1].0 .0 == current_key
            && self.data[self.index - 1].0 .1 == current_val
        {
            self.index -= 1;
        }

        self.val_valid = true;
    }
}

impl<K, V, T, R> BatchReader for TestBatch<K, V, T, R>
where
    K: DBData,
    V: DBData,
    T: DBTimestamp,
    R: DBWeight,
{
    type Key = K;
    type Val = V;
    type Time = T;
    type R = R;

    type Cursor<'s> = TestBatchCursor<K, V, T, R>;

    type Consumer = TestBatchConsumer<K, V, T, R>;

    fn cursor(&self) -> Self::Cursor<'_> {
        TestBatchCursor::new(self)
    }

    fn consumer(self) -> Self::Consumer {
        todo!()
    }

    fn key_count(&self) -> usize {
        self.data
            .keys()
            .map(|(k, _, _)| k.clone())
            .collect::<BTreeSet<_>>()
            .len()
    }

    fn len(&self) -> usize {
        self.data.len()
    }

    fn lower(&self) -> AntichainRef<'_, Self::Time> {
        todo!()
    }

    fn upper(&self) -> AntichainRef<'_, Self::Time> {
        todo!()
    }

    fn truncate_keys_below(&mut self, lower_bound: &Self::Key) {
        let bound = if let Some(bound) = &self.lower_key_bound {
            max(bound, lower_bound).clone()
        } else {
            lower_bound.clone()
        };

        self.lower_key_bound = Some(bound.clone());
        self.data.retain(|(k, _v, _t), _r| k >= &bound);
    }

    fn sample_keys<RG>(&self, _rng: &mut RG, _sample_size: usize, _sample: &mut Vec<Self::Key>)
    where
        RG: Rng,
    {
        unimplemented!()
    }
}

impl<K, V, T, R> Batch for TestBatch<K, V, T, R>
where
    K: DBData,
    V: DBData,
    T: DBTimestamp,
    R: DBWeight,
{
    type Item = (K, V);

    type Batcher = TestBatchBatcher<K, V, T, R>;
    type Builder = TestBatchBuilder<K, V, T, R>;
    type Merger = TestBatchMerger<K, V, T, R>;

    fn item_from(key: Self::Key, val: Self::Val) -> Self::Item {
        (key, val)
    }

    fn from_keys(time: Self::Time, keys: Vec<(Self::Key, Self::R)>) -> Self
    where
        Self::Val: From<()>,
    {
        let tuples = keys
            .into_iter()
            .map(|(k, r)| ((k, <Self::Val>::from(())), r))
            .collect::<Vec<_>>();
        Self::from_tuples(time, tuples)
    }

    fn recede_to(&mut self, frontier: &Self::Time) {
        let data = self
            .data
            .iter()
            .map(|((k, v, t), r)| ((k.clone(), v.clone(), t.meet(frontier)), r.clone()))
            .collect::<Vec<_>>();

        self.data = Self::from_data(&data).data;
    }
}

impl<K, V, T, R> Trace for TestBatch<K, V, T, R>
where
    K: DBData,
    V: DBData,
    T: DBTimestamp,
    R: DBWeight,
{
    type Batch = Self;

    fn new(_activator: Option<Activator>) -> Self {
        Self {
            data: BTreeMap::new(),
            lower_key_bound: None,
            lower_val_bound: None,
        }
    }

    fn recede_to(&mut self, frontier: &Self::Time) {
        Batch::recede_to(self, frontier);
    }

    fn exert(&mut self, _effort: &mut isize) {}

    fn consolidate(self) -> Option<Self::Batch> {
        if self.data.is_empty() {
            None
        } else {
            Some(self)
        }
    }

    fn insert(&mut self, mut batch: Self::Batch) {
        if let Some(bound) = &self.lower_key_bound {
            batch.truncate_keys_below(bound);
        }
        if let Some(bound) = &self.lower_val_bound {
            batch.truncate_values_below(bound);
        }

        self.data = self.merge(&batch).data;
    }

    fn clear_dirty_flag(&mut self) {}

    fn dirty(&self) -> bool {
        todo!()
    }

    fn truncate_values_below(&mut self, lower_bound: &Self::Val) {
        let bound = if let Some(bound) = &self.lower_val_bound {
            max(bound, lower_bound).clone()
        } else {
            lower_bound.clone()
        };

        self.data.retain(|(_k, v, _t), _r| v >= &bound);
        self.lower_val_bound = Some(bound);
    }

    fn lower_value_bound(&self) -> &Option<Self::Val> {
        &self.lower_val_bound
    }
}

/// Test random sampling methods.
///
/// Assumes that `B` is a simple batch, not a trace (i.e., no duplicate keys).
pub fn test_batch_sampling<B: BatchReader<Time = ()>>(batch: &B) {
    let mut sample = Vec::new();

    let mut all_keys = Vec::new();
    let mut cursor = batch.cursor();
    while cursor.key_valid() {
        all_keys.push(cursor.key().clone());
        cursor.step_key();
    }
    let all_keys_set = all_keys.iter().cloned().collect::<BTreeSet<_>>();

    // Sample size 0 - return empty sample.
    batch.sample_keys(&mut thread_rng(), 0, &mut sample);
    assert!(sample.is_empty());
    sample.clear();

    // Sample size == batch size - must return all keys in the batch.
    batch.sample_keys(&mut thread_rng(), batch.key_count(), &mut sample);
    assert_eq!(sample, all_keys);
    sample.clear();

    // Sample size > batch size - must return all keys in the batch.
    batch.sample_keys(&mut thread_rng(), batch.key_count() << 1, &mut sample);
    assert_eq!(sample, all_keys);
    sample.clear();

    // Sample size < batch size - return the exact number of keys requested,
    // no duplicates, all returned keys must belong to the batch.
    let sample_size = batch.key_count() >> 1;
    batch.sample_keys(&mut thread_rng(), sample_size, &mut sample);
    assert_eq!(sample.len(), sample_size);
    assert!(sample.is_sorted_by(|k1, k2| Some(k1.cmp(k2))));
    let sample_set = sample.iter().cloned().collect::<BTreeSet<_>>();
    assert_eq!(sample_set.len(), sample.len());
    for key in sample.iter() {
        assert!(all_keys_set.contains(key));
    }
    sample.clear();
}

/// Test random sampling methods.
///
/// Similar to `test_batch_sampling`, but allows the sample
/// to contain fewer keys than requested (as keys in a trace
/// can get canceled out).
pub fn test_trace_sampling<T: Trace<Time = ()>>(trace: &T) {
    let mut sample = Vec::new();

    let batch = TestBatch::from_data(&batch_to_tuples(trace));

    let mut all_keys = Vec::new();
    let mut cursor = batch.cursor();
    while cursor.key_valid() {
        all_keys.push(cursor.key().clone());
        cursor.step_key();
    }
    let all_keys_set = all_keys.iter().cloned().collect::<BTreeSet<_>>();

    // Sample size 0 - return empty sample.
    trace.sample_keys(&mut thread_rng(), 0, &mut sample);
    assert!(sample.is_empty());
    sample.clear();

    // Sample size == size - must return all keys in the batch.
    trace.sample_keys(&mut thread_rng(), trace.key_count(), &mut sample);
    assert_eq!(sample, all_keys);
    sample.clear();

    // Sample size > trace size - must return all keys in the trace.
    trace.sample_keys(&mut thread_rng(), trace.key_count() << 1, &mut sample);
    assert_eq!(sample, all_keys);
    sample.clear();

    // Sample size < trace size - return at most the number of keys requested,
    // no duplicates, all returned keys must belong to the trace.
    let sample_size = trace.key_count() >> 1;
    trace.sample_keys(&mut thread_rng(), sample_size, &mut sample);
    assert!(sample.len() <= sample_size);
    assert!(sample.is_sorted_by(|k1, k2| Some(k1.cmp(k2))));
    let sample_set = sample.iter().cloned().collect::<BTreeSet<_>>();
    assert_eq!(sample_set.len(), sample.len());
    for key in sample.iter() {
        assert!(all_keys_set.contains(key));
    }
    sample.clear();
}
