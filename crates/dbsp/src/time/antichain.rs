use crate::algebra::{PartialOrder, TotalOrder};
use bincode::{Decode, Encode};
use size_of::SizeOf;
use std::{
    fmt::{self, Debug},
    hash::{Hash, Hasher},
    iter::{FromIterator, IntoIterator},
    ops::Deref,
    slice::Iter,
    vec::IntoIter,
};

/// A set of mutually incomparable elements.
///
/// An antichain is a set of partially ordered elements, each of which is
/// incomparable to the others. This antichain implementation allows you to
/// repeatedly introduce elements to the antichain, and which will evict larger
/// elements to maintain the *minimal* antichain, those incomparable elements no
/// greater than any other element.
///
/// Two antichains are equal if the contain the same set of elements, even if in
/// different orders. This can make equality testing quadratic, though linear in
/// the common case that the sequences are identical.
#[derive(Default, SizeOf, Encode, Decode)]
pub struct Antichain<T>
where
    T: 'static,
{
    // TODO: We can specialize containers based on the inner type, meaning we could give ourselves
    //       a more favorable memory footprint for things like `Antichain<()>`
    elements: Vec<T>,
}

impl<T> Antichain<T> {
    /// Creates a new empty `Antichain`.
    ///
    /// # Examples
    ///
    ///```
    /// use dbsp::time::Antichain;
    ///
    /// let mut frontier = Antichain::<u32>::new();
    /// ```
    pub const fn new() -> Self {
        Self {
            elements: Vec::new(),
        }
    }

    /// Creates a new empty `Antichain` with space for `capacity` elements.
    ///
    /// # Examples
    ///
    ///```
    /// use dbsp::time::Antichain;
    ///
    /// let mut frontier = Antichain::<u32>::with_capacity(10);
    /// ```
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            elements: Vec::with_capacity(capacity),
        }
    }

    /// Creates a new singleton `Antichain`.
    ///
    /// # Examples
    ///
    ///```
    /// use dbsp::time::Antichain;
    ///
    /// let mut frontier = Antichain::from_elem(2);
    /// ```
    pub fn from_elem(element: T) -> Self {
        Self {
            elements: vec![element],
        }
    }

    /// Clears the contents of the antichain.
    ///
    /// # Examples
    ///
    ///```
    /// use dbsp::time::Antichain;
    ///
    /// let mut frontier = Antichain::from_elem(2);
    /// frontier.clear();
    /// assert!(frontier.is_empty());
    /// ```
    pub fn clear(&mut self) {
        self.elements.clear();
    }

    /// Sorts the elements so that comparisons between antichains can be made.
    pub fn sort(&mut self)
    where
        T: Ord,
    {
        self.elements.sort()
    }

    /// Reveals the elements in the antichain.
    ///
    /// # Examples
    ///
    ///```
    /// use dbsp::time::Antichain;
    ///
    /// let mut frontier = Antichain::from_elem(2);
    /// assert_eq!(frontier.as_slice(), &[2]);
    /// ```
    #[inline]
    pub fn as_slice(&self) -> &[T] {
        &self.elements
    }

    /// Reveals the elements in the antichain.
    ///
    /// # Examples
    ///
    ///```
    /// use dbsp::time::Antichain;
    ///
    /// let mut frontier = Antichain::from_elem(2);
    /// assert_eq!(&*frontier.as_ref(), &[2]);
    /// ```
    #[inline]
    pub fn as_ref(&self) -> AntichainRef<T> {
        AntichainRef::new(&self.elements)
    }
}

impl<T> Antichain<T>
where
    T: PartialOrder,
{
    /// Updates the `Antichain` if the element is not greater than or equal to
    /// some present element.
    ///
    /// Returns `true` if the element was added to the set
    ///
    /// # Examples
    ///
    ///```
    /// use dbsp::time::Antichain;
    ///
    /// let mut frontier = Antichain::new();
    /// assert!(frontier.insert(2));
    /// assert!(!frontier.insert(3));
    /// ```
    pub fn insert(&mut self, element: T) -> bool {
        if !self.elements.iter().any(|x| x.less_equal(&element)) {
            self.elements.retain(|x| !element.less_equal(x));
            self.elements.push(element);
            true
        } else {
            false
        }
    }

    /// Reserves capacity for at least `additional` more elements to be inserted
    /// in the given `Antichain`
    pub fn reserve(&mut self, additional: usize) {
        self.elements.reserve(additional);
    }

    /// Performs a sequence of insertion and return true if any insertions
    /// happen
    ///
    /// # Examples
    ///
    ///```
    /// use dbsp::time::Antichain;
    ///
    /// let mut frontier = Antichain::new();
    /// assert!(frontier.extend(Some(3)));
    /// assert!(frontier.extend(vec![2, 5]));
    /// assert!(!frontier.extend(vec![3, 4]));
    /// ```
    pub fn extend<I>(&mut self, iterator: I) -> bool
    where
        I: IntoIterator<Item = T>,
    {
        let mut added = false;
        for element in iterator {
            added |= self.insert(element);
        }

        added
    }

    /// Returns true if any item in the antichain is strictly less than the
    /// argument.
    ///
    /// # Examples
    ///
    ///```
    /// use dbsp::time::Antichain;
    ///
    /// let mut frontier = Antichain::from_elem(2);
    /// assert!(frontier.less_than(&3));
    /// assert!(!frontier.less_than(&2));
    /// assert!(!frontier.less_than(&1));
    ///
    /// frontier.clear();
    /// assert!(!frontier.less_than(&3));
    /// ```
    #[inline]
    pub fn less_than(&self, time: &T) -> bool {
        self.elements.iter().any(|x| x.less_than(time))
    }

    /// Returns true if any item in the antichain is less than or equal to the
    /// argument.
    ///
    /// # Examples
    ///
    ///```
    /// use dbsp::time::Antichain;
    ///
    /// let mut frontier = Antichain::from_elem(2);
    /// assert!(frontier.less_equal(&3));
    /// assert!(frontier.less_equal(&2));
    /// assert!(!frontier.less_equal(&1));
    ///
    /// frontier.clear();
    /// assert!(!frontier.less_equal(&3));
    /// ```
    #[inline]
    pub fn less_equal(&self, time: &T) -> bool {
        self.elements.iter().any(|x| x.less_equal(time))
    }
}

impl<T> Antichain<T>
where
    T: TotalOrder,
{
    /// Convert to the at most one element the antichain contains.
    pub fn into_option(mut self) -> Option<T> {
        debug_assert!(self.len() <= 1);
        self.elements.pop()
    }

    /// Return a reference to the at most one element the antichain contains.
    pub fn as_option(&self) -> Option<&T> {
        debug_assert!(self.len() <= 1);
        self.elements.last()
    }
}

impl<T> PartialEq for Antichain<T>
where
    T: PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        // Lengths should be the same, with the option for fast acceptance if identical.
        self.len() == other.len()
            && (self.iter().zip(other.iter()).all(|(t1, t2)| t1 == t2)
                || self.iter().all(|t1| other.iter().any(|t2| t1 == t2)))
    }
}

impl<T> Eq for Antichain<T> where T: Eq {}

impl<T> PartialOrder for Antichain<T>
where
    T: PartialOrder,
{
    fn less_equal(&self, other: &Self) -> bool {
        other
            .iter()
            .all(|t2| self.iter().any(|t1| t1.less_equal(t2)))
    }
}

impl<T> Clone for Antichain<T>
where
    T: Clone,
{
    #[inline]
    fn clone(&self) -> Self {
        Self {
            elements: self.elements.clone(),
        }
    }

    #[inline]
    fn clone_from(&mut self, source: &Self) {
        self.elements.clone_from(&source.elements);
    }
}

impl<T: TotalOrder> TotalOrder for Antichain<T> {}

impl<T> Hash for Antichain<T>
where
    T: Ord + Hash,
{
    fn hash<H: Hasher>(&self, state: &mut H) {
        let mut temp = self.elements.iter().collect::<Vec<_>>();
        temp.sort();

        for element in temp {
            element.hash(state);
        }
    }
}

impl<T> From<Vec<T>> for Antichain<T>
where
    T: PartialOrder,
{
    fn from(vec: Vec<T>) -> Self {
        // TODO: We could reuse `vec` with some care.
        let mut temp = Antichain::new();
        for elem in vec.into_iter() {
            temp.insert(elem);
        }

        temp
    }
}

impl<T> From<Antichain<T>> for Vec<T> {
    #[inline]
    fn from(frontier: Antichain<T>) -> Self {
        frontier.elements
    }
}

impl<T> Deref for Antichain<T> {
    type Target = [T];

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.elements
    }
}

impl<T> FromIterator<T> for Antichain<T>
where
    T: PartialOrder,
{
    #[inline]
    fn from_iter<I>(iterator: I) -> Self
    where
        I: IntoIterator<Item = T>,
    {
        let mut result = Self::new();
        result.extend(iterator);
        result
    }
}

impl<'a, T> IntoIterator for &'a Antichain<T> {
    type Item = &'a T;
    type IntoIter = Iter<'a, T>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.elements.iter()
    }
}

impl<T> IntoIterator for Antichain<T> {
    type Item = T;
    type IntoIter = IntoIter<T>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.elements.into_iter()
    }
}

impl<T> Debug for Antichain<T>
where
    T: Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(&self.elements).finish()
    }
}

/// A wrapper for the elements of an antichain
#[derive(Default, SizeOf)]
pub struct AntichainRef<'a, T: 'a> {
    /// Elements contained within the antichain
    frontier: &'a [T],
}

impl<'a, T: 'a> AntichainRef<'a, T> {
    /// Create a new `AntichainRef` from a reference to a slice of elements
    /// forming the frontier.
    ///
    /// This method does not check that this antichain has any particular
    /// properties, for example that there are no elements strictly less
    /// than other elements.
    pub const fn new(frontier: &'a [T]) -> Self {
        Self { frontier }
    }

    /// Create an empty `AntichainRef` with zero elements forming the frontier
    pub const fn empty() -> Self {
        Self { frontier: &[] }
    }

    /// Constructs an owned antichain from the antichain reference.
    ///
    /// # Examples
    ///
    ///```
    /// use dbsp::time::{Antichain, AntichainRef};
    ///
    /// let frontier = AntichainRef::new(&[1u64]);
    /// assert_eq!(frontier.to_owned(), Antichain::from_elem(1u64));
    /// ```
    pub fn to_owned(&self) -> Antichain<T>
    where
        T: Clone,
    {
        Antichain {
            elements: self.frontier.to_vec(),
        }
    }
}

impl<'a, T> AntichainRef<'a, T>
where
    T: PartialOrder + 'a,
{
    /// Returns true if any item in the `AntichainRef` is strictly less than the
    /// argument.
    ///
    /// # Examples
    ///
    ///```
    /// use dbsp::time::AntichainRef;
    ///
    /// let frontier = AntichainRef::new(&[1u64]);
    /// assert!(!frontier.less_than(&0));
    /// assert!(!frontier.less_than(&1));
    /// assert!(frontier.less_than(&2));
    /// ```
    #[inline]
    pub fn less_than(&self, time: &T) -> bool {
        self.iter().any(|x| x.less_than(time))
    }

    /// Returns true if any item in the `AntichainRef` is less than or equal to
    /// the argument.
    ///
    /// # Examples
    ///
    ///```
    /// use dbsp::time::AntichainRef;
    ///
    /// let frontier = AntichainRef::new(&[1u64]);
    /// assert!(!frontier.less_equal(&0));
    /// assert!(frontier.less_equal(&1));
    /// assert!(frontier.less_equal(&2));
    /// ```
    #[inline]
    pub fn less_equal(&self, time: &T) -> bool {
        self.iter().any(|x| x.less_equal(time))
    }
}

impl<'a, T> AntichainRef<'a, T>
where
    T: TotalOrder + 'a,
{
    /// Return a reference to the at most one element the antichain contains
    #[inline]
    pub fn as_option(&self) -> Option<&T> {
        debug_assert!(self.len() <= 1);
        self.frontier.last()
    }
}

impl<'a, T: 'a> Clone for AntichainRef<'a, T> {
    #[inline]
    fn clone(&self) -> Self {
        Self {
            frontier: self.frontier,
        }
    }

    #[inline]
    fn clone_from(&mut self, source: &Self) {
        self.frontier = source.frontier;
    }
}

impl<'a, T: 'a> Copy for AntichainRef<'a, T> {}

impl<'a, T> PartialEq for AntichainRef<'a, T>
where
    T: PartialEq + 'a,
{
    fn eq(&self, other: &Self) -> bool {
        // Lengths should be the same, with the option for fast acceptance if identical.
        self.len() == other.len()
            && (self.iter().zip(other.iter()).all(|(t1, t2)| t1 == t2)
                || self.iter().all(|t1| other.iter().any(|t2| t1 == t2)))
    }
}

impl<'a, T> Eq for AntichainRef<'a, T> where T: Eq {}

impl<'a, T> PartialOrder for AntichainRef<'a, T>
where
    T: PartialOrder + 'a,
{
    fn less_equal(&self, other: &Self) -> bool {
        other
            .iter()
            .all(|t2| self.iter().any(|t1| t1.less_equal(t2)))
    }
}

impl<'a, T> TotalOrder for AntichainRef<'a, T> where T: TotalOrder {}

impl<'a, T> AsRef<[T]> for AntichainRef<'a, T> {
    #[inline]
    fn as_ref(&self) -> &[T] {
        self.frontier
    }
}

impl<'a, T> Deref for AntichainRef<'a, T> {
    type Target = [T];

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.frontier
    }
}

impl<'a, T: 'a> IntoIterator for AntichainRef<'a, T> {
    type Item = &'a T;
    type IntoIter = Iter<'a, T>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.frontier.iter()
    }
}

impl<'a, T: 'a> IntoIterator for &'a AntichainRef<'a, T> {
    type Item = &'a T;
    type IntoIter = Iter<'a, T>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.frontier.iter()
    }
}

impl<'a, T: 'a> Debug for AntichainRef<'a, T>
where
    T: Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.frontier).finish()
    }
}

#[cfg(test)]
mod tests {
    use crate::{algebra::PartialOrder, time::Antichain};
    use std::collections::HashSet;

    #[derive(PartialEq, Eq, PartialOrd, Ord, Hash)]
    struct Elem(char, usize);

    impl PartialOrder for Elem {
        fn less_equal(&self, other: &Self) -> bool {
            self.0 <= other.0 && self.1 <= other.1
        }
    }

    #[test]
    fn antichain_hash() {
        let mut hashed = HashSet::new();
        hashed.insert(Antichain::from(vec![Elem('a', 2), Elem('b', 1)]));

        assert!(hashed.contains(&Antichain::from(vec![Elem('a', 2), Elem('b', 1)])));
        assert!(hashed.contains(&Antichain::from(vec![Elem('b', 1), Elem('a', 2)])));

        assert!(!hashed.contains(&Antichain::from(vec![Elem('a', 2)])));
        assert!(!hashed.contains(&Antichain::from(vec![Elem('a', 1)])));
        assert!(!hashed.contains(&Antichain::from(vec![Elem('b', 2)])));
        assert!(!hashed.contains(&Antichain::from(vec![Elem('a', 1), Elem('b', 2)])));
        assert!(!hashed.contains(&Antichain::from(vec![Elem('c', 3)])));
        assert!(!hashed.contains(&Antichain::from(vec![])));
    }
}
