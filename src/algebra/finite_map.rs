/*
MIT License
SPDX-License-Identifier: MIT

Copyright (c) 2021 VMware, Inc
*/

//! This module implements groups that are finite maps
//! from values to a group.

use super::{GroupValue, *};
use std::{
    collections::{hash_map, hash_map::Entry, HashMap},
    fmt::{Display, Formatter, Result},
    hash::Hash,
    iter::{repeat, FromIterator},
    mem::swap,
};

/// These are the properties we expect for a FiniteMap key.
pub trait KeyProperties: 'static + Clone + Eq + Hash {}

impl<T> KeyProperties for T where T: 'static + Clone + Eq + Hash {}

// FIXME: this is not abstract enough.
type SupportIter<'a, KeyType, ValueType> = hash_map::Keys<'a, KeyType, ValueType>;

////////////////////////////////////////////////////////
/// Finite map trait.
///
/// A finite map maps arbitrary values (comparable for equality)
/// to values in a group.  It has finite support: it is non-zero
/// only for a finite number of values.
///
/// `KeyType` - Type of values stored in finite map.
/// `ValueType` - Type of results.
pub trait FiniteMap<KeyType, ValueType>:
    GroupValue + IntoIterator<Item = (KeyType, ValueType)> + FromIterator<(KeyType, ValueType)>
where
    KeyType: KeyProperties,
    ValueType: GroupValue,
{
    /// Find the value associated to the specified key
    fn lookup(&self, key: &KeyType) -> ValueType;

    /// Return the set of values that are mapped to non-zero values.
    fn support(&self) -> SupportIter<'_, KeyType, ValueType>;

    /// The size of the support: number of elements for which the map does not
    /// return zero.
    fn support_size(&self) -> usize;

    /// Increase the value associated to `key` by the specified `value`
    fn increment(&mut self, key: &KeyType, value: ValueType);

    /// Create a map containing a singleton value.
    fn singleton(key: KeyType, value: ValueType) -> Self;

    /// Apply map to every 'key' in the support of this map and generate a new
    /// map. The new map is generated by adding all the maps generated by
    /// this process.
    //  TODO: it would be nice for this to return a trait instead of a type.
    fn map<F, ConvertedKeyType>(&self, mapper: &F) -> FiniteHashMap<ConvertedKeyType, ValueType>
    where
        F: Fn(&KeyType) -> ConvertedKeyType,
        ConvertedKeyType: KeyProperties;

    /// Apply the `filter` function to every key in the map and
    /// generate a new map containing only the keys for which the `filter`
    /// function returns `true`
    fn filter<F>(&self, filter: &F) -> Self
    where
        F: Fn(&KeyType) -> bool;

    /// Apply the mapper to every 'key' in the support of this map and generate
    /// a new map. The new map is generated by adding all the maps generated
    /// in this process.
    //  TODO: this should return a trait instead of a type.
    fn flatmap<F, ConvertedKeyType, I>(
        &self,
        mapper: &F,
    ) -> FiniteHashMap<ConvertedKeyType, ValueType>
    where
        F: Fn(&KeyType) -> I,
        I: IntoIterator<Item = ConvertedKeyType>,
        ConvertedKeyType: KeyProperties;

    /// Combine the value corresponding to each key in self
    /// to the value corresponding to the same key in `other`
    /// using the `merger` function.  The `merger` function
    /// is guaranteed to return 0 when either input is 0, so
    /// we only need to look for the common keys between self and `other`.
    /// The data in the result is produced by applying the `merger`
    //  TODO: this should return a trait rather than a type.
    fn match_keys<ValueType2, ValueType3, FM2, F>(
        &self,
        other: &FM2,
        merger: &F,
    ) -> FiniteHashMap<KeyType, ValueType3>
    where
        ValueType2: GroupValue,
        ValueType3: GroupValue,
        FM2: FiniteMap<KeyType, ValueType2>,
        for<'a> &'a FM2: IntoIterator<Item = (&'a KeyType, &'a ValueType2)>,
        F: Fn(&ValueType, &ValueType2) -> ValueType3,
        for<'a> &'a Self: IntoIterator<Item = (&'a KeyType, &'a ValueType)>;
}

#[derive(Debug, Clone)]
pub struct FiniteHashMap<KeyType, ValueType>
where
    KeyType: KeyProperties,
{
    // Unfortunately I cannot just implement these traits for
    // HashMap since they conflict with some existing traits.
    // We maintain the invariant that the keys (and only these keys)
    // that have non-zero values are in this map.
    pub(super) value: HashMap<KeyType, ValueType>,
}

impl<KeyType, ValueType> FiniteHashMap<KeyType, ValueType>
where
    KeyType: KeyProperties,
    ValueType: GroupValue,
{
    /// Allocate an empty FiniteHashMap
    pub fn new() -> Self {
        FiniteHashMap::default()
    }

    /// Allocate an empty FiniteHashMap that is expected to hold 'size' values.
    pub fn with_capacity(size: usize) -> Self {
        FiniteHashMap::<KeyType, ValueType> {
            value: HashMap::with_capacity(size),
        }
    }
}

/// Consuming iterator
impl<KeyType, ValueType> IntoIterator for FiniteHashMap<KeyType, ValueType>
where
    KeyType: KeyProperties,
    ValueType: GroupValue,
{
    type Item = (KeyType, ValueType);
    type IntoIter = std::collections::hash_map::IntoIter<KeyType, ValueType>;

    fn into_iter(self) -> Self::IntoIter {
        self.value.into_iter()
    }
}

/// Read-only iterator.
impl<'a, KeyType, ValueType> IntoIterator for &'a FiniteHashMap<KeyType, ValueType>
where
    KeyType: KeyProperties,
    ValueType: GroupValue,
{
    type Item = (&'a KeyType, &'a ValueType);
    type IntoIter = std::collections::hash_map::Iter<'a, KeyType, ValueType>;

    fn into_iter(self) -> Self::IntoIter {
        self.value.iter()
    }
}

impl<KeyType, ValueType> FromIterator<(KeyType, ValueType)> for FiniteHashMap<KeyType, ValueType>
where
    KeyType: KeyProperties,
    ValueType: GroupValue,
{
    fn from_iter<T>(iter: T) -> Self
    where
        T: IntoIterator<Item = (KeyType, ValueType)>,
    {
        let mut result = FiniteHashMap::new();
        for (k, v) in iter {
            result.increment(&k, v);
        }
        result
    }
}

impl<KeyType, ValueType> FiniteMap<KeyType, ValueType> for FiniteHashMap<KeyType, ValueType>
where
    KeyType: KeyProperties,
    ValueType: GroupValue,
{
    fn lookup(&self, key: &KeyType) -> ValueType {
        let val = self.value.get(key);
        match val {
            Some(w) => w.clone(),
            None => ValueType::zero(),
        }
    }

    fn support<'a>(&self) -> SupportIter<'_, KeyType, ValueType> {
        self.value.keys()
    }

    fn support_size(&self) -> usize {
        self.value.len()
    }

    fn increment(&mut self, key: &KeyType, value: ValueType) {
        if value.is_zero() {
            return;
        }
        // TODO: the HashMap API does not support avoiding this clone.
        // This has been a known issue since 2015: https://github.com/rust-lang/rust/issues/56167
        // We should use a different implementation or API if one becomes available.
        let e = self.value.entry(key.clone());
        match e {
            Entry::Vacant(ve) => {
                ve.insert(value);
            }
            Entry::Occupied(mut oe) => {
                oe.get_mut().add_assign(value);
                if oe.get().is_zero() {
                    oe.remove_entry();
                }
            }
        };
    }

    fn singleton(key: KeyType, value: ValueType) -> Self {
        let mut result = Self::with_capacity(1);
        result.value.insert(key, value);
        result
    }

    fn map<F, ConvertedKeyType>(&self, mapper: &F) -> FiniteHashMap<ConvertedKeyType, ValueType>
    where
        F: Fn(&KeyType) -> ConvertedKeyType,
        ConvertedKeyType: KeyProperties,
    {
        self.into_iter()
            .map(|(k, v)| (mapper(k), v.clone()))
            .collect()
    }

    fn filter<F>(&self, filter: &F) -> Self
    where
        F: Fn(&KeyType) -> bool,
    {
        self.into_iter()
            .filter(|(k, _)| filter(k))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    fn flatmap<F, ConvertedKeyType, I>(
        &self,
        mapper: &F,
    ) -> FiniteHashMap<ConvertedKeyType, ValueType>
    where
        F: Fn(&KeyType) -> I,
        I: IntoIterator<Item = ConvertedKeyType>,
        ConvertedKeyType: KeyProperties,
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
    fn match_keys<ValueType2, ValueType3, FM2, F>(
        &self,
        other: &FM2,
        merger: &F,
    ) -> FiniteHashMap<KeyType, ValueType3>
    where
        ValueType2: GroupValue,
        ValueType3: GroupValue,
        FM2: FiniteMap<KeyType, ValueType2>,
        for<'a> &'a FM2: IntoIterator<Item = (&'a KeyType, &'a ValueType2)>,
        F: Fn(&ValueType, &ValueType2) -> ValueType3,
        for<'a> &'a Self: IntoIterator<Item = (&'a KeyType, &'a ValueType)>,
    {
        let mut result = FiniteHashMap::<KeyType, ValueType3>::new();
        // iterate on the smaller set
        if self.support_size() < other.support_size() {
            for (k, v) in self {
                let v2 = other.lookup(k);
                if v2.is_zero() {
                    continue;
                }
                let merged = merger(v, &v2);
                result.increment(k, merged)
            }
        } else {
            for (k, v2) in other {
                let v = self.lookup(k);
                if v.is_zero() {
                    continue;
                }
                let merged = merger(&v, v2);
                result.increment(k, merged)
            }
        }
        result
    }
}

impl<KeyType, ValueType> Default for FiniteHashMap<KeyType, ValueType>
where
    KeyType: KeyProperties,
{
    fn default() -> Self {
        FiniteHashMap::<KeyType, ValueType> {
            value: HashMap::default(),
        }
    }
}

impl<KeyType, ValueType> Add<FiniteHashMap<KeyType, ValueType>>
    for FiniteHashMap<KeyType, ValueType>
where
    KeyType: KeyProperties,
    ValueType: GroupValue,
{
    type Output = Self;

    fn add(self, other: Self) -> Self {
        fn do_add<KeyType, ValueType>(
            mut this: FiniteHashMap<KeyType, ValueType>,
            other: FiniteHashMap<KeyType, ValueType>,
        ) -> FiniteHashMap<KeyType, ValueType>
        where
            KeyType: KeyProperties,
            ValueType: GroupValue,
        {
            for (k, v) in other.value {
                let entry = this.value.entry(k);
                match entry {
                    Entry::Vacant(e) => {
                        e.insert(v);
                    }
                    Entry::Occupied(mut e) => {
                        e.get_mut().add_assign(v);
                        if e.get().is_zero() {
                            e.remove_entry();
                        }
                    }
                }
            }
            this
        }
        if self.support_size() > other.support_size() {
            do_add(self, other)
        } else {
            do_add(other, self)
        }
    }
}
impl<KeyType, ValueType> AddByRef for FiniteHashMap<KeyType, ValueType>
where
    KeyType: KeyProperties,
    ValueType: GroupValue,
{
    fn add_by_ref(&self, other: &Self) -> Self {
        fn do_add<KeyType, ValueType>(
            mut this: FiniteHashMap<KeyType, ValueType>,
            other: &FiniteHashMap<KeyType, ValueType>,
        ) -> FiniteHashMap<KeyType, ValueType>
        where
            KeyType: KeyProperties,
            ValueType: GroupValue,
        {
            for (k, v) in &other.value {
                // TODO: unfortunately there is no way to avoid this k.clone() currently.
                // See also note on 'insert' below.
                let entry = this.value.entry(k.clone());
                match entry {
                    Entry::Vacant(e) => {
                        e.insert(v.clone());
                    }
                    Entry::Occupied(mut e) => {
                        e.get_mut().add_assign_by_ref(v);
                        if e.get().is_zero() {
                            e.remove_entry();
                        }
                    }
                }
            }
            this
        }

        if self.support_size() > other.support_size() {
            do_add(self.clone(), other)
        } else {
            do_add(other.clone(), self)
        }
    }
}

impl<KeyType, ValueType> AddAssign for FiniteHashMap<KeyType, ValueType>
where
    KeyType: KeyProperties,
    ValueType: GroupValue,
{
    fn add_assign(&mut self, other: Self) {
        for (k, v) in other.value {
            let entry = self.value.entry(k);
            match entry {
                Entry::Vacant(e) => {
                    e.insert(v);
                }
                Entry::Occupied(mut e) => {
                    e.get_mut().add_assign(v);
                    if e.get().is_zero() {
                        e.remove_entry();
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
        for (k, v) in &other.value {
            // TODO: unfortunately there is no way to avoid this clone.
            let entry = self.value.entry(k.clone());
            match entry {
                Entry::Vacant(e) => {
                    e.insert(v.clone());
                }
                Entry::Occupied(mut e) => {
                    e.get_mut().add_assign_by_ref(v);
                    if e.get().is_zero() {
                        e.remove_entry();
                    }
                }
            }
        }
    }
}

impl<KeyType, ValueType> HasZero for FiniteHashMap<KeyType, ValueType>
where
    KeyType: KeyProperties,
    ValueType: GroupValue,
{
    fn is_zero(&self) -> bool {
        self.value.is_empty()
    }

    fn zero() -> Self {
        FiniteHashMap::default()
    }
}

impl<KeyType, ValueType> NegByRef for FiniteHashMap<KeyType, ValueType>
where
    KeyType: KeyProperties,
    ValueType: GroupValue,
{
    fn neg_by_ref(&self) -> Self {
        let mut result = self.clone();
        for val in result.value.values_mut() {
            let mut tmp = ValueType::zero();
            swap(val, &mut tmp);
            *val = tmp.neg();
        }
        result
    }
}

impl<KeyType, ValueType> Neg for FiniteHashMap<KeyType, ValueType>
where
    KeyType: KeyProperties,
    ValueType: GroupValue,
{
    type Output = Self;

    fn neg(mut self) -> Self {
        for val in self.value.values_mut() {
            let mut tmp = ValueType::zero();
            swap(val, &mut tmp);
            *val = tmp.neg();
        }
        self
    }
}

impl<KeyType, ValueType> PartialEq for FiniteHashMap<KeyType, ValueType>
where
    KeyType: KeyProperties,
    ValueType: GroupValue,
{
    fn eq(&self, other: &Self) -> bool {
        self.value.eq(&other.value)
    }
}

impl<KeyType, ValueType> Eq for FiniteHashMap<KeyType, ValueType>
where
    KeyType: KeyProperties,
    ValueType: GroupValue,
{
}

/// This class knows how to display a FiniteMap to a string, but only
/// if the map keys support comparison
impl<KeyType, ValueType> Display for FiniteHashMap<KeyType, ValueType>
where
    KeyType: KeyProperties + Display + Ord,
    ValueType: GroupValue + Display,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        let mut vec: Vec<KeyType> = self.support().cloned().collect();
        vec.sort_by(KeyType::cmp);
        write!(f, "{{")?;

        let mut first = true;
        for k in vec {
            if !first {
                write!(f, ",")?;
            } else {
                first = false;
            }
            let val = self.lookup(&k);
            write!(f, "{}", k)?;
            write!(f, "=>")?;
            write!(f, "{}", val)?;
        }
        write!(f, "}}")
    }
}

#[test]
fn hashmap_tests() {
    let mut z = FiniteHashMap::<i64, i64>::with_capacity(5);
    assert_eq!(0, z.support_size());
    assert_eq!("{}", z.to_string());
    assert_eq!(0, z.lookup(&0)); // not present -> 0
    assert_eq!(z, FiniteHashMap::<i64, i64>::zero());
    assert!(z.is_zero());
    let z2 = FiniteHashMap::<i64, i64>::new();
    assert_eq!(z, z2);

    let z3 = FiniteHashMap::singleton(3, 4);
    assert_eq!("{3=>4}", z3.to_string());

    z.increment(&0, 1);
    assert_eq!(1, z.support_size());
    assert_eq!("{0=>1}", z.to_string());
    assert_eq!(1, z.lookup(&0));
    assert_eq!(0, z.lookup(&1));
    assert_ne!(z, FiniteHashMap::<i64, i64>::zero());
    assert!(!z.is_zero());

    z.increment(&2, 0);
    assert_eq!(1, z.support_size());
    assert_eq!("{0=>1}", z.to_string());

    z.increment(&1, -1);
    assert_eq!(2, z.support_size());
    assert_eq!("{0=>1,1=>-1}", z.to_string());

    z.increment(&-1, 1);
    assert_eq!(3, z.support_size());
    assert_eq!("{-1=>1,0=>1,1=>-1}", z.to_string());

    let d = z.neg_by_ref();
    assert_eq!(3, d.support_size());
    assert_eq!("{-1=>-1,0=>-1,1=>1}", d.to_string());
    assert_ne!(d, z);

    let d = z.clone().neg();
    assert_eq!(3, d.support_size());
    assert_eq!("{-1=>-1,0=>-1,1=>1}", d.to_string());
    assert_ne!(d, z);

    let i = d.clone().into_iter().collect::<FiniteHashMap<i64, i64>>();
    assert_eq!(i, d);

    z.increment(&1, 1);
    assert_eq!(2, z.support_size());
    assert_eq!("{-1=>1,0=>1}", z.to_string());

    let mut z2 = z.add_by_ref(&z);
    assert_eq!(2, z2.support_size());
    assert_eq!("{-1=>2,0=>2}", z2.to_string());

    let z2_owned = z.clone().add(z.clone());
    assert_eq!(2, z2_owned.support_size());
    assert_eq!("{-1=>2,0=>2}", z2_owned.to_string());

    z2.add_assign_by_ref(&z);
    assert_eq!(2, z2.support_size());
    assert_eq!("{-1=>3,0=>3}", z2.to_string());

    let z3 = z2.map(&|_x| 0);
    assert_eq!(1, z3.support_size());
    assert_eq!("{0=>6}", z3.to_string());

    let z4 = z2.filter(&|x| *x >= 0);
    assert_eq!(1, z4.support_size());
    assert_eq!("{0=>3}", z4.to_string());

    z2.increment(&4, 2);
    let z5 = z2.flatmap(&|x| std::iter::once(*x));
    assert_eq!(&z5, &z2);
    let z5 = z2.flatmap(&|x| {
        if *x > 0 {
            (0..*x).into_iter().collect::<Vec<i64>>().into_iter()
        } else {
            std::iter::once(*x).collect::<Vec<i64>>().into_iter()
        }
    });
    assert_eq!("{-1=>3,0=>3,4=>2}", z2.to_string());
    assert_eq!("{-1=>3,0=>5,1=>2,2=>2,3=>2}", z5.to_string());

    let z6 = z2.match_keys(&z5, &|w, w2| w + w2);
    assert_eq!("{-1=>6,0=>8}", z6.to_string());
}
