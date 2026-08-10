//! String interning for the two names that repeat across the whole graph.
//!
//! Fieldnames and fieldtypes recur ~100k times on a reference bench
//! ([31](../../../docs/specs/31-performance.md)); DocType names recur ~2.5k
//! times and are the primary lookup key, so they stay owned `String`s where a
//! symbol would only add a resolve step to every call site.

use std::collections::HashMap;

/// An interned string, valid only against the [`Interner`] that issued it.
///
/// Symbol values depend on insertion order, so they are never compared across
/// two indexes — [`crate::Index::fingerprint`] resolves them back to text
/// before comparing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Symbol(u32);

impl Symbol {
    /// The raw index, for callers that need a dense array key.
    #[must_use]
    pub fn as_u32(self) -> u32 {
        self.0
    }
}

/// Stores each distinct string once and hands out [`Symbol`]s for it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Interner {
    values: Vec<Box<str>>,
    ids: HashMap<Box<str>, Symbol>,
}

impl Interner {
    /// An empty interner.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The symbol for `text`, interning it if it is new.
    pub fn intern(&mut self, text: &str) -> Symbol {
        if let Some(symbol) = self.ids.get(text) {
            return *symbol;
        }
        let symbol = Symbol(
            u32::try_from(self.values.len())
                .expect("a workspace with 4 billion distinct names is not a workspace"),
        );
        let owned: Box<str> = text.into();
        self.values.push(owned.clone());
        self.ids.insert(owned, symbol);
        symbol
    }

    /// The symbol `text` already has, without interning it.
    #[must_use]
    pub fn get(&self, text: &str) -> Option<Symbol> {
        self.ids.get(text).copied()
    }

    /// The text behind a symbol this interner issued.
    #[must_use]
    pub fn resolve(&self, symbol: Symbol) -> &str {
        // Symbols are only constructed here and the vector never shrinks, so a
        // symbol from this interner always indexes into it.
        &self.values[symbol.0 as usize]
    }

    /// How many distinct strings are stored.
    #[must_use]
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Whether nothing has been interned.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_text_always_yields_the_same_symbol() {
        let mut interner = Interner::new();
        let first = interner.intern("member_name");
        let second = interner.intern("member_name");

        assert_eq!(first, second);
        assert_eq!(interner.len(), 1);
        assert_eq!(interner.resolve(first), "member_name");
    }

    #[test]
    fn distinct_text_yields_distinct_symbols() {
        let mut interner = Interner::new();
        let name = interner.intern("member_name");
        let email = interner.intern("email_id");

        assert_ne!(name, email);
        assert_eq!(interner.len(), 2);
        assert_eq!(interner.resolve(email), "email_id");
    }

    #[test]
    fn an_uninterned_string_has_no_symbol() {
        let mut interner = Interner::new();
        interner.intern("Data");

        assert!(interner.get("Data").is_some());
        assert_eq!(interner.get("Link"), None);
    }

    #[test]
    fn interning_is_case_sensitive_because_frappe_is() {
        let mut interner = Interner::new();
        assert_ne!(interner.intern("Data"), interner.intern("data"));
    }
}
