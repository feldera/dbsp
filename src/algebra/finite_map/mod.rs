#[macro_use]
mod map_macro;
#[cfg(test)]
mod tests;

use crate::algebra::{
    Add, AddAssign, AddAssignByRef, AddByRef, GroupValue, HasZero, Neg, NegByRef,
};
use hashbrown::{
    hash_map,
    hash_map::{Entry, HashMap, RawEntryMut},
};
use std::{
    fmt::{Debug, Formatter, Result},
    hash::Hash,
    iter::{repeat, FromIterator},
    mem::swap,
};

/// The properties we expect for a FiniteMap key.
pub trait KeyProperties: 'static + Clone + Eq + Hash {}

impl<T> KeyProperties for T where T: 'static + Clone + Eq + Hash {}

// FIXME: this is not abstract enough.
type SupportIter<'a, KeyType, ValueType> = hash_map::Keys<'a, KeyType, ValueType>;

/// Finite map trait.
///
/// A finite map maps arbitrary values (comparable for equality)
/// to values in a group.  It has finite support: it is non-zero
/// only for a finite number of values.
///
/// `KeyType` - Type of values stored in finite map.
/// `ValueType` - Type of results.
pub trait FiniteMap<Key, Value>:
    GroupValue + IntoIterator<Item = (Key, Value)> + FromIterator<(Key, Value)>
where
    Key: KeyProperties,
    Value: GroupValue,
{
    /// Find the value associated to the specified key
    fn lookup(&self, key: &Key) -> Value;

    /// Find the value associated to the specified key.
    ///
    /// Returns `None` when `key` is not in the support of `self`.
    fn get_in_support(&self, key: &Key) -> Option<&Value>;

    /// Modify the value associated with `key`.
    fn update<F>(&mut self, key: &Key, f: F)
    where
        F: FnOnce(&mut Value);

    /// Modify the value associated with `key`.
    fn update_owned<F>(&mut self, key: Key, f: F)
    where
        F: FnOnce(&mut Value);

    /// Return the set of values that are mapped to non-zero values.
    fn support(&self) -> SupportIter<'_, Key, Value>;

    /// The size of the support: number of elements for which the map does not
    /// return zero.
    fn support_size(&self) -> usize;

    /// Increase the value associated with `key` by the specified `value`.
    fn increment(&mut self, key: &Key, value: Value);

    /// Increase the value associated with `key` by the specified `value`.
    fn increment_owned(&mut self, key: Key, value: Value);

    /// Create a map containing a singleton value.
    fn singleton(key: Key, value: Value) -> Self;

    /// Apply map to every 'key' in the support of this map and generate a new
    /// map. The new map is generated by adding all the maps generated by
    /// this process.
    //  TODO: it would be nice for this to return a trait instead of a type.
    fn map<F, ConvertedKeyType>(&self, mapper: &F) -> FiniteHashMap<ConvertedKeyType, Value>
    where
        F: Fn(&Key) -> ConvertedKeyType,
        ConvertedKeyType: KeyProperties;

    /// Apply the `filter` function to every key in the map and
    /// generate a new map containing only the keys for which the `filter`
    /// function returns `true`
    fn filter<F>(&self, filter: F) -> Self
    where
        F: FnMut(&Key) -> bool;

    /// Apply the mapper to every 'key' in the support of this map and generate
    /// a new map. The new map is generated by adding all the maps generated
    /// in this process.
    //  TODO: this should return a trait instead of a type.
    fn flat_map<F, ConvertedKeyType, I>(&self, mapper: F) -> FiniteHashMap<ConvertedKeyType, Value>
    where
        F: FnMut(&Key) -> I,
        I: IntoIterator<Item = ConvertedKeyType>,
        ConvertedKeyType: KeyProperties;

    /// Combine the value corresponding to each key in self
    /// to the value corresponding to the same key in `other`
    /// using the `merger` function.  The `merger` function
    /// is guaranteed to return 0 when either input is 0, so
    /// we only need to look for the common keys between self and `other`.
    /// The data in the result is produced by applying the `merger`
    //  TODO: this should return a trait rather than a type.
    fn match_keys<Value2, Value3, FM2, F>(
        &self,
        other: &FM2,
        merger: F,
    ) -> FiniteHashMap<Key, Value3>
    where
        Value2: GroupValue,
        Value3: GroupValue,
        FM2: FiniteMap<Key, Value2>,
        for<'a> &'a FM2: IntoIterator<Item = (&'a Key, &'a Value2)>,
        F: FnMut(&Value, &Value2) -> Value3,
        for<'a> &'a Self: IntoIterator<Item = (&'a Key, &'a Value)>;
}

#[derive(Clone)]
pub struct FiniteHashMap<Key, Value> {
    // Unfortunately I cannot just implement these traits for
    // HashMap since they conflict with some existing traits.
    // We maintain the invariant that the keys (and only these keys)
    // that have non-zero values are in this map.
    pub(super) value: HashMap<Key, Value>,
}

impl<Key, Value> FiniteHashMap<Key, Value> {
    /// Create a new map
    pub fn new() -> Self {
        Self {
            value: HashMap::default(),
        }
    }

    /// Create an empty [`FiniteHashMap`] that with the capacity to hold `size`
    /// elements without reallocating
    pub fn with_capacity(size: usize) -> Self {
        Self {
            value: HashMap::with_capacity(size),
        }
    }
}

impl<Key, Value> IntoIterator for FiniteHashMap<Key, Value>
where
    Key: KeyProperties,
    Value: GroupValue,
{
    type Item = (Key, Value);
    type IntoIter = hash_map::IntoIter<Key, Value>;

    fn into_iter(self) -> Self::IntoIter {
        self.value.into_iter()
    }
}

impl<'a, Key, Value> IntoIterator for &'a FiniteHashMap<Key, Value>
where
    Key: KeyProperties,
    Value: GroupValue,
{
    type Item = (&'a Key, &'a Value);
    type IntoIter = hash_map::Iter<'a, Key, Value>;

    fn into_iter(self) -> Self::IntoIter {
        self.value.iter()
    }
}

impl<Key, Value> FromIterator<(Key, Value)> for FiniteHashMap<Key, Value>
where
    Key: KeyProperties,
    Value: GroupValue,
{
    fn from_iter<T>(iter: T) -> Self
    where
        T: IntoIterator<Item = (Key, Value)>,
    {
        let mut result = Self::new();
        for (k, v) in iter {
            result.increment(&k, v);
        }

        result
    }
}

impl<Key, Value> FiniteMap<Key, Value> for FiniteHashMap<Key, Value>
where
    Key: KeyProperties,
    Value: GroupValue,
{
    fn lookup(&self, key: &Key) -> Value {
        self.value.get(key).cloned().unwrap_or_else(Value::zero)
    }

    fn get_in_support(&self, key: &Key) -> Option<&Value> {
        self.value.get(key)
    }

    fn update<F>(&mut self, key: &Key, f: F)
    where
        F: FnOnce(&mut Value),
    {
        match self.value.raw_entry_mut().from_key(key) {
            RawEntryMut::Occupied(mut oe) => {
                let val = oe.get_mut();
                f(val);
                if val.is_zero() {
                    oe.remove();
                }
            }
            RawEntryMut::Vacant(ve) => {
                let mut val = Value::zero();
                f(&mut val);
                ve.insert(key.clone(), val);
            }
        }
    }

    fn update_owned<F>(&mut self, key: Key, f: F)
    where
        F: FnOnce(&mut Value),
    {
        match self.value.entry(key) {
            Entry::Occupied(mut oe) => {
                let val = oe.get_mut();
                f(val);
                if val.is_zero() {
                    oe.remove();
                }
            }
            Entry::Vacant(ve) => {
                let mut val = Value::zero();
                f(&mut val);
                ve.insert(val);
            }
        }
    }

    fn support<'a>(&self) -> SupportIter<'_, Key, Value> {
        self.value.keys()
    }

    fn support_size(&self) -> usize {
        self.value.len()
    }

    fn increment(&mut self, key: &Key, value: Value) {
        if value.is_zero() {
            return;
        }

        match self.value.raw_entry_mut().from_key(key) {
            RawEntryMut::Vacant(vacant) => {
                vacant.insert(key.clone(), value);
            }

            RawEntryMut::Occupied(mut occupied) => {
                occupied.get_mut().add_assign(value);
                if occupied.get().is_zero() {
                    occupied.remove_entry();
                }
            }
        }
    }

    fn increment_owned(&mut self, key: Key, value: Value) {
        if value.is_zero() {
            return;
        }

        // TODO: the HashMap API does not support avoiding this clone.
        // This has been a known issue since 2015: https://github.com/rust-lang/rust/issues/56167
        // We should use a different implementation or API if one becomes available.
        match self.value.entry(key) {
            Entry::Vacant(vacant) => {
                vacant.insert(value);
            }

            Entry::Occupied(mut occupied) => {
                occupied.get_mut().add_assign(value);
                if occupied.get().is_zero() {
                    occupied.remove_entry();
                }
            }
        }
    }

    fn singleton(key: Key, value: Value) -> Self {
        let mut result = Self::with_capacity(1);
        result.value.insert(key, value);
        result
    }

    fn map<F, NewKey>(&self, mapper: &F) -> FiniteHashMap<NewKey, Value>
    where
        F: Fn(&Key) -> NewKey,
        NewKey: KeyProperties,
    {
        self.into_iter()
            .map(|(k, v)| (mapper(k), v.clone()))
            .collect()
    }

    fn filter<F>(&self, mut filter: F) -> Self
    where
        F: FnMut(&Key) -> bool,
    {
        self.into_iter()
            .filter(|(k, _)| filter(k))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    fn flat_map<F, NewKey, I>(&self, mut mapper: F) -> FiniteHashMap<NewKey, Value>
    where
        F: FnMut(&Key) -> I,
        I: IntoIterator<Item = NewKey>,
        NewKey: KeyProperties,
    {
        self.into_iter()
            .flat_map(|(k, v)| mapper(k).into_iter().zip(repeat(v.clone())))
            .collect()
    }

    /// Combine the value corresponding to each key in self
    /// to the value corresponding to the same key in `other`
    /// using the `merger` function.  The `merger` function
    /// is guaranteed to return 0 when either input is 0, so
    /// we only need to look for the common keys between self and `other`.
    /// The data in the result is produced by applying the `merger`
    //  TODO: this should return a trait rather than a type.
    fn match_keys<Value2, Value3, FM2, F>(
        &self,
        other: &FM2,
        mut merger: F,
    ) -> FiniteHashMap<Key, Value3>
    where
        Value2: GroupValue,
        Value3: GroupValue,
        FM2: FiniteMap<Key, Value2>,
        for<'a> &'a FM2: IntoIterator<Item = (&'a Key, &'a Value2)>,
        F: FnMut(&Value, &Value2) -> Value3,
        for<'a> &'a Self: IntoIterator<Item = (&'a Key, &'a Value)>,
    {
        let mut result = FiniteHashMap::<Key, Value3>::new();
        // iterate on the smaller set
        if self.support_size() < other.support_size() {
            for (k, v) in self {
                if let Some(v2) = other.get_in_support(k) {
                    if v2.is_zero() {
                        continue;
                    }

                    let merged = merger(v, v2);
                    result.increment(k, merged)
                }
            }
        } else {
            for (k, v2) in other {
                if let Some(v) = self.get_in_support(k) {
                    if v.is_zero() {
                        continue;
                    }

                    let merged = merger(v, v2);
                    result.increment(k, merged)
                }
            }
        }

        result
    }
}

impl<Key, Value> Default for FiniteHashMap<Key, Value>
where
    Key: KeyProperties,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<Key, Value> Add for FiniteHashMap<Key, Value>
where
    Key: KeyProperties,
    Value: GroupValue,
{
    type Output = Self;

    fn add(self, other: Self) -> Self {
        fn add_inner<Key, Value>(
            mut this: FiniteHashMap<Key, Value>,
            other: FiniteHashMap<Key, Value>,
        ) -> FiniteHashMap<Key, Value>
        where
            Key: KeyProperties,
            Value: GroupValue,
        {
            for (key, value) in other.value {
                match this.value.entry(key) {
                    Entry::Vacant(vacant) => {
                        vacant.insert(value);
                    }

                    Entry::Occupied(mut occupied) => {
                        occupied.get_mut().add_assign(value);
                        if occupied.get().is_zero() {
                            occupied.remove_entry();
                        }
                    }
                }
            }

            this
        }

        if self.support_size() > other.support_size() {
            add_inner(self, other)
        } else {
            add_inner(other, self)
        }
    }
}
impl<Key, Value> AddByRef for FiniteHashMap<Key, Value>
where
    Key: KeyProperties,
    Value: GroupValue,
{
    fn add_by_ref(&self, other: &Self) -> Self {
        fn add_inner<Key, Value>(
            mut this: FiniteHashMap<Key, Value>,
            other: &FiniteHashMap<Key, Value>,
        ) -> FiniteHashMap<Key, Value>
        where
            Key: KeyProperties,
            Value: GroupValue,
        {
            for (key, value) in &other.value {
                // TODO: unfortunately there is no way to avoid this k.clone() currently.
                // See also note on 'insert' below.
                match this.value.entry(key.clone()) {
                    Entry::Vacant(vacant) => {
                        vacant.insert(value.clone());
                    }

                    Entry::Occupied(mut occupied) => {
                        occupied.get_mut().add_assign_by_ref(value);
                        if occupied.get().is_zero() {
                            occupied.remove_entry();
                        }
                    }
                }
            }

            this
        }

        if self.support_size() > other.support_size() {
            add_inner(self.clone(), other)
        } else {
            add_inner(other.clone(), self)
        }
    }
}

impl<Key, Value> AddAssign for FiniteHashMap<Key, Value>
where
    Key: KeyProperties,
    Value: GroupValue,
{
    fn add_assign(&mut self, other: Self) {
        for (key, value) in other.value {
            match self.value.entry(key) {
                Entry::Vacant(vacant) => {
                    vacant.insert(value);
                }

                Entry::Occupied(mut occupied) => {
                    occupied.get_mut().add_assign(value);
                    if occupied.get().is_zero() {
                        occupied.remove_entry();
                    }
                }
            }
        }
    }
}

impl<KeyType, ValueType> AddAssignByRef for FiniteHashMap<KeyType, ValueType>
where
    KeyType: KeyProperties,
    ValueType: GroupValue,
{
    fn add_assign_by_ref(&mut self, other: &Self) {
        for (key, value) in &other.value {
            // TODO: unfortunately there is no way to avoid this clone.
            match self.value.entry(key.clone()) {
                Entry::Vacant(vacant) => {
                    vacant.insert(value.clone());
                }

                Entry::Occupied(mut occupied) => {
                    occupied.get_mut().add_assign_by_ref(value);
                    if occupied.get().is_zero() {
                        occupied.remove_entry();
                    }
                }
            }
        }
    }
}

impl<Key, Value> HasZero for FiniteHashMap<Key, Value>
where
    Key: KeyProperties,
    Value: GroupValue,
{
    fn is_zero(&self) -> bool {
        self.value.is_empty()
    }

    fn zero() -> Self {
        Self::default()
    }
}

impl<Key, Value> NegByRef for FiniteHashMap<Key, Value>
where
    Key: KeyProperties,
    Value: GroupValue,
{
    fn neg_by_ref(&self) -> Self {
        let mut result = self.clone();
        for val in result.value.values_mut() {
            let mut tmp = Value::zero();
            swap(val, &mut tmp);
            *val = tmp.neg();
        }

        result
    }
}

impl<Key, Value> Neg for FiniteHashMap<Key, Value>
where
    Key: KeyProperties,
    Value: GroupValue,
{
    type Output = Self;

    fn neg(mut self) -> Self {
        for val in self.value.values_mut() {
            let mut tmp = Value::zero();
            swap(val, &mut tmp);
            *val = tmp.neg();
        }

        self
    }
}

impl<Key, Value> PartialEq for FiniteHashMap<Key, Value>
where
    Key: KeyProperties,
    Value: GroupValue,
{
    fn eq(&self, other: &Self) -> bool {
        self.value.eq(&other.value)
    }
}

impl<Key, Value> Eq for FiniteHashMap<Key, Value>
where
    Key: KeyProperties,
    Value: GroupValue,
{
}

impl<K, V> Debug for FiniteHashMap<K, V>
where
    K: Debug,
    V: Debug,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        self.value.fmt(f)
    }
}
