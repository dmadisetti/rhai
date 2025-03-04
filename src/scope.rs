//! Module that defines the [`Scope`] type representing a function call-stack scope.

use crate::dynamic::{AccessMode, Variant};
use crate::stdlib::{borrow::Cow, boxed::Box, iter, vec::Vec};
use crate::{Dynamic, Identifier, StaticVec};

/// Keep a number of entries inline (since [`Dynamic`] is usually small enough).
const SCOPE_SIZE: usize = 16;

/// Type containing information about the current scope.
/// Useful for keeping state between [`Engine`][crate::Engine] evaluation runs.
///
/// # Thread Safety
///
/// Currently, [`Scope`] is neither [`Send`] nor [`Sync`].
/// Turn on the `sync` feature to make it [`Send`] `+` [`Sync`].
///
/// # Example
///
/// ```
/// # fn main() -> Result<(), Box<rhai::EvalAltResult>> {
/// use rhai::{Engine, Scope};
///
/// let engine = Engine::new();
/// let mut my_scope = Scope::new();
///
/// my_scope.push("z", 40_i64);
///
/// engine.eval_with_scope::<()>(&mut my_scope, "let x = z + 1; z = 0;")?;
///
/// assert_eq!(engine.eval_with_scope::<i64>(&mut my_scope, "x + 1")?, 42);
///
/// assert_eq!(my_scope.get_value::<i64>("x").unwrap(), 41);
/// assert_eq!(my_scope.get_value::<i64>("z").unwrap(), 0);
/// # Ok(())
/// # }
/// ```
///
/// When searching for entries, newly-added entries are found before similarly-named but older entries,
/// allowing for automatic _shadowing_.
//
// # Implementation Notes
//
// [`Scope`] is implemented as two [`Vec`]'s of exactly the same length.  Variables data (name, type, etc.)
// is manually split into two equal-length arrays.  That's because variable names take up the most space,
// with [`Cow<str>`][Cow] being four words long, but in the vast majority of cases the name is NOT used to
// look up a variable.  Variable lookup is usually via direct indexing, by-passing the name altogether.
//
// Since [`Dynamic`] is reasonably small, packing it tightly improves cache locality when variables are accessed.
//
// The alias is `Box`'ed because it occurs infrequently.
#[derive(Debug, Clone, Hash)]
pub struct Scope<'a> {
    /// Current value of the entry.
    values: smallvec::SmallVec<[Dynamic; SCOPE_SIZE]>,
    /// (Name, aliases) of the entry.
    names: Vec<(Cow<'a, str>, Option<Box<StaticVec<Identifier>>>)>,
}

impl Default for Scope<'_> {
    #[inline(always)]
    fn default() -> Self {
        Self {
            values: Default::default(),
            names: Vec::with_capacity(SCOPE_SIZE),
        }
    }
}

impl<'a> IntoIterator for Scope<'a> {
    type Item = (Cow<'a, str>, Dynamic);
    type IntoIter = Box<dyn Iterator<Item = Self::Item> + 'a>;

    #[inline(always)]
    fn into_iter(self) -> Self::IntoIter {
        Box::new(
            self.values
                .into_iter()
                .zip(self.names.into_iter())
                .map(|(value, (name, _))| (name, value)),
        )
    }
}

impl<'a> Scope<'a> {
    /// Create a new [`Scope`].
    ///
    /// # Example
    ///
    /// ```
    /// use rhai::Scope;
    ///
    /// let mut my_scope = Scope::new();
    ///
    /// my_scope.push("x", 42_i64);
    /// assert_eq!(my_scope.get_value::<i64>("x").unwrap(), 42);
    /// ```
    #[inline(always)]
    pub fn new() -> Self {
        Default::default()
    }
    /// Empty the [`Scope`].
    ///
    /// # Example
    ///
    /// ```
    /// use rhai::Scope;
    ///
    /// let mut my_scope = Scope::new();
    ///
    /// my_scope.push("x", 42_i64);
    /// assert!(my_scope.contains("x"));
    /// assert_eq!(my_scope.len(), 1);
    /// assert!(!my_scope.is_empty());
    ///
    /// my_scope.clear();
    /// assert!(!my_scope.contains("x"));
    /// assert_eq!(my_scope.len(), 0);
    /// assert!(my_scope.is_empty());
    /// ```
    #[inline(always)]
    pub fn clear(&mut self) -> &mut Self {
        self.names.clear();
        self.values.clear();
        self
    }
    /// Get the number of entries inside the [`Scope`].
    ///
    /// # Example
    ///
    /// ```
    /// use rhai::Scope;
    ///
    /// let mut my_scope = Scope::new();
    /// assert_eq!(my_scope.len(), 0);
    ///
    /// my_scope.push("x", 42_i64);
    /// assert_eq!(my_scope.len(), 1);
    /// ```
    #[inline(always)]
    pub fn len(&self) -> usize {
        self.values.len()
    }
    /// Is the [`Scope`] empty?
    ///
    /// # Example
    ///
    /// ```
    /// use rhai::Scope;
    ///
    /// let mut my_scope = Scope::new();
    /// assert!(my_scope.is_empty());
    ///
    /// my_scope.push("x", 42_i64);
    /// assert!(!my_scope.is_empty());
    /// ```
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.values.len() == 0
    }
    /// Add (push) a new entry to the [`Scope`].
    ///
    /// # Example
    ///
    /// ```
    /// use rhai::Scope;
    ///
    /// let mut my_scope = Scope::new();
    ///
    /// my_scope.push("x", 42_i64);
    /// assert_eq!(my_scope.get_value::<i64>("x").unwrap(), 42);
    /// ```
    #[inline(always)]
    pub fn push(
        &mut self,
        name: impl Into<Cow<'a, str>>,
        value: impl Variant + Clone,
    ) -> &mut Self {
        self.push_dynamic_value(name, AccessMode::ReadWrite, Dynamic::from(value))
    }
    /// Add (push) a new [`Dynamic`] entry to the [`Scope`].
    ///
    /// # Example
    ///
    /// ```
    /// use rhai::{Dynamic,  Scope};
    ///
    /// let mut my_scope = Scope::new();
    ///
    /// my_scope.push_dynamic("x", Dynamic::from(42_i64));
    /// assert_eq!(my_scope.get_value::<i64>("x").unwrap(), 42);
    /// ```
    #[inline(always)]
    pub fn push_dynamic(&mut self, name: impl Into<Cow<'a, str>>, value: Dynamic) -> &mut Self {
        self.push_dynamic_value(name, value.access_mode(), value)
    }
    /// Add (push) a new constant to the [`Scope`].
    ///
    /// Constants are immutable and cannot be assigned to.  Their values never change.
    /// Constants propagation is a technique used to optimize an [`AST`][crate::AST].
    ///
    /// # Example
    ///
    /// ```
    /// use rhai::Scope;
    ///
    /// let mut my_scope = Scope::new();
    ///
    /// my_scope.push_constant("x", 42_i64);
    /// assert_eq!(my_scope.get_value::<i64>("x").unwrap(), 42);
    /// ```
    #[inline(always)]
    pub fn push_constant(
        &mut self,
        name: impl Into<Cow<'a, str>>,
        value: impl Variant + Clone,
    ) -> &mut Self {
        self.push_dynamic_value(name, AccessMode::ReadOnly, Dynamic::from(value))
    }
    /// Add (push) a new constant with a [`Dynamic`] value to the Scope.
    ///
    /// Constants are immutable and cannot be assigned to.  Their values never change.
    /// Constants propagation is a technique used to optimize an [`AST`][crate::AST].
    ///
    /// # Example
    ///
    /// ```
    /// use rhai::{Dynamic, Scope};
    ///
    /// let mut my_scope = Scope::new();
    ///
    /// my_scope.push_constant_dynamic("x", Dynamic::from(42_i64));
    /// assert_eq!(my_scope.get_value::<i64>("x").unwrap(), 42);
    /// ```
    #[inline(always)]
    pub fn push_constant_dynamic(
        &mut self,
        name: impl Into<Cow<'a, str>>,
        value: Dynamic,
    ) -> &mut Self {
        self.push_dynamic_value(name, AccessMode::ReadOnly, value)
    }
    /// Add (push) a new entry with a [`Dynamic`] value to the [`Scope`].
    #[inline(always)]
    pub(crate) fn push_dynamic_value(
        &mut self,
        name: impl Into<Cow<'a, str>>,
        access: AccessMode,
        mut value: Dynamic,
    ) -> &mut Self {
        self.names.push((name.into(), None));
        value.set_access_mode(access);
        self.values.push(value.into());
        self
    }
    /// Truncate (rewind) the [`Scope`] to a previous size.
    ///
    /// # Example
    ///
    /// ```
    /// use rhai::Scope;
    ///
    /// let mut my_scope = Scope::new();
    ///
    /// my_scope.push("x", 42_i64);
    /// my_scope.push("y", 123_i64);
    /// assert!(my_scope.contains("x"));
    /// assert!(my_scope.contains("y"));
    /// assert_eq!(my_scope.len(), 2);
    ///
    /// my_scope.rewind(1);
    /// assert!(my_scope.contains("x"));
    /// assert!(!my_scope.contains("y"));
    /// assert_eq!(my_scope.len(), 1);
    ///
    /// my_scope.rewind(0);
    /// assert!(!my_scope.contains("x"));
    /// assert!(!my_scope.contains("y"));
    /// assert_eq!(my_scope.len(), 0);
    /// assert!(my_scope.is_empty());
    /// ```
    #[inline(always)]
    pub fn rewind(&mut self, size: usize) -> &mut Self {
        self.names.truncate(size);
        self.values.truncate(size);
        self
    }
    /// Does the [`Scope`] contain the entry?
    ///
    /// # Example
    ///
    /// ```
    /// use rhai::Scope;
    ///
    /// let mut my_scope = Scope::new();
    ///
    /// my_scope.push("x", 42_i64);
    /// assert!(my_scope.contains("x"));
    /// assert!(!my_scope.contains("y"));
    /// ```
    #[inline(always)]
    pub fn contains(&self, name: &str) -> bool {
        self.names
            .iter()
            .rev() // Always search a Scope in reverse order
            .any(|(key, _)| name == key.as_ref())
    }
    /// Find an entry in the [`Scope`], starting from the last.
    #[inline(always)]
    pub(crate) fn get_index(&self, name: &str) -> Option<(usize, AccessMode)> {
        self.names
            .iter()
            .enumerate()
            .rev() // Always search a Scope in reverse order
            .find_map(|(index, (key, _))| {
                if name == key.as_ref() {
                    Some((index, self.values[index].access_mode()))
                } else {
                    None
                }
            })
    }
    /// Get the value of an entry in the [`Scope`], starting from the last.
    ///
    /// # Example
    ///
    /// ```
    /// use rhai::Scope;
    ///
    /// let mut my_scope = Scope::new();
    ///
    /// my_scope.push("x", 42_i64);
    /// assert_eq!(my_scope.get_value::<i64>("x").unwrap(), 42);
    /// ```
    #[inline(always)]
    pub fn get_value<T: Variant + Clone>(&self, name: &str) -> Option<T> {
        self.names
            .iter()
            .enumerate()
            .rev()
            .find(|(_, (key, _))| name == key.as_ref())
            .and_then(|(index, _)| self.values[index].flatten_clone().try_cast())
    }
    /// Update the value of the named entry in the [`Scope`].
    ///
    /// Search starts backwards from the last, and only the first entry matching the specified name is updated.
    /// If no entry matching the specified name is found, a new one is added.
    ///
    /// # Panics
    ///
    /// Panics when trying to update the value of a constant.
    ///
    /// # Example
    ///
    /// ```
    /// use rhai::Scope;
    ///
    /// let mut my_scope = Scope::new();
    ///
    /// my_scope.push("x", 42_i64);
    /// assert_eq!(my_scope.get_value::<i64>("x").unwrap(), 42);
    ///
    /// my_scope.set_value("x", 0_i64);
    /// assert_eq!(my_scope.get_value::<i64>("x").unwrap(), 0);
    /// ```
    #[inline(always)]
    pub fn set_value(&mut self, name: &'a str, value: impl Variant + Clone) -> &mut Self {
        match self.get_index(name) {
            None => {
                self.push(name, value);
            }
            Some((_, AccessMode::ReadOnly)) => panic!("variable {} is constant", name),
            Some((index, AccessMode::ReadWrite)) => {
                *self.values.get_mut(index).unwrap() = Dynamic::from(value);
            }
        }
        self
    }
    /// Get a mutable reference to an entry in the [`Scope`].
    ///
    /// If the entry by the specified name is not found, of if it is read-only,
    /// [`None`] is returned.
    ///
    /// # Example
    ///
    /// ```
    /// use rhai::Scope;
    ///
    /// let mut my_scope = Scope::new();
    ///
    /// my_scope.push("x", 42_i64);
    /// assert_eq!(my_scope.get_value::<i64>("x").unwrap(), 42);
    ///
    /// let ptr = my_scope.get_mut("x").unwrap();
    /// *ptr = 123_i64.into();
    ///
    /// assert_eq!(my_scope.get_value::<i64>("x").unwrap(), 123);
    /// ```
    pub fn get_mut(&mut self, name: &str) -> Option<&mut Dynamic> {
        self.get_index(name)
            .and_then(move |(index, access)| match access {
                AccessMode::ReadWrite => Some(self.get_mut_by_index(index)),
                AccessMode::ReadOnly => None,
            })
    }
    /// Get a mutable reference to an entry in the [`Scope`] based on the index.
    #[inline(always)]
    pub(crate) fn get_mut_by_index(&mut self, index: usize) -> &mut Dynamic {
        self.values.get_mut(index).expect("invalid index in Scope")
    }
    /// Update the access type of an entry in the [`Scope`].
    #[cfg(not(feature = "no_module"))]
    #[inline(always)]
    pub(crate) fn add_entry_alias(
        &mut self,
        index: usize,
        alias: impl Into<Identifier> + PartialEq<Identifier>,
    ) -> &mut Self {
        let entry = self.names.get_mut(index).expect("invalid index in Scope");
        if entry.1.is_none() {
            entry.1 = Some(Default::default());
        }
        if !entry.1.as_ref().unwrap().iter().any(|a| &alias == a) {
            entry.1.as_mut().unwrap().push(alias.into());
        }
        self
    }
    /// Clone the [`Scope`], keeping only the last instances of each variable name.
    /// Shadowed variables are omitted in the copy.
    #[inline(always)]
    pub(crate) fn clone_visible(&self) -> Self {
        let mut entries: Self = Default::default();

        self.names
            .iter()
            .enumerate()
            .rev()
            .for_each(|(i, (name, alias))| {
                if !entries.names.iter().any(|(key, _)| key == name) {
                    entries.names.push((name.clone(), alias.clone()));
                    entries.values.push(self.values[i].clone());
                }
            });

        entries
    }
    /// Get an iterator to entries in the [`Scope`].
    #[inline(always)]
    #[allow(dead_code)]
    pub(crate) fn into_iter(
        self,
    ) -> impl Iterator<Item = (Cow<'a, str>, Dynamic, Vec<Identifier>)> {
        self.names
            .into_iter()
            .zip(self.values.into_iter())
            .map(|((name, alias), value)| {
                (name, value, alias.map(|a| a.to_vec()).unwrap_or_default())
            })
    }
    /// Get an iterator to entries in the [`Scope`].
    /// Shared values are flatten-cloned.
    ///
    /// # Example
    ///
    /// ```
    /// use rhai::{Dynamic, Scope};
    ///
    /// let mut my_scope = Scope::new();
    ///
    /// my_scope.push("x", 42_i64);
    /// my_scope.push_constant("foo", "hello");
    ///
    /// let mut iter = my_scope.iter();
    ///
    /// let (name, is_constant, value) = iter.next().unwrap();
    /// assert_eq!(name, "x");
    /// assert!(!is_constant);
    /// assert_eq!(value.cast::<i64>(), 42);
    ///
    /// let (name, is_constant, value) = iter.next().unwrap();
    /// assert_eq!(name, "foo");
    /// assert!(is_constant);
    /// assert_eq!(value.cast::<String>(), "hello");
    /// ```
    #[inline(always)]
    pub fn iter(&self) -> impl Iterator<Item = (&str, bool, Dynamic)> {
        self.iter_raw()
            .map(|(name, constant, value)| (name, constant, value.flatten_clone()))
    }
    /// Get an iterator to entries in the [`Scope`].
    /// Shared values are not expanded.
    #[inline(always)]
    pub fn iter_raw(&self) -> impl Iterator<Item = (&str, bool, &Dynamic)> {
        self.names
            .iter()
            .zip(self.values.iter())
            .map(|((name, _), value)| (name.as_ref(), value.is_read_only(), value))
    }
}

impl<'a, K: Into<Cow<'a, str>>> iter::Extend<(K, Dynamic)> for Scope<'a> {
    #[inline(always)]
    fn extend<T: IntoIterator<Item = (K, Dynamic)>>(&mut self, iter: T) {
        iter.into_iter().for_each(|(name, value)| {
            self.names.push((name.into(), None));
            self.values.push(value);
        });
    }
}
