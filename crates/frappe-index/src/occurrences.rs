//! The reverse occurrence table — [21](../../../docs/specs/21-navigation.md).
//!
//! `textDocument/references` on a DocType has to cover every surface at once:
//! Python, JavaScript, other DocTypes' `options`, `hooks.py`, fixtures. A
//! result set that silently omits one is worse than none, and a grep per
//! request cannot meet the 100 ms budget in
//! [31](../../../docs/specs/31-performance.md) on a bench of 40 000 files.
//!
//! So it is a table: name → sites, built by a scan, stored per file so one
//! file's edit invalidates only its own rows, and carried in the index cache
//! so a warm start pays nothing for it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::span::Range;

/// One place a name is used.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Site {
    /// The file it is in.
    pub path: Arc<Path>,
    /// The name's own range, not the statement's.
    pub range: Range,
}

/// What a site refers to.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub enum Key {
    /// A DocType, by name.
    DocType(String),
    /// A field, scoped to its DocType — spec 21: `customer` on `Sales Invoice`
    /// must not return `customer` on `Sales Order`.
    Field(String, String),
}

/// Every occurrence in the workspace, by name and by file.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Occurrences {
    sites: BTreeMap<Key, Vec<Site>>,
    /// What each file contributed, so re-indexing it removes exactly that.
    by_file: BTreeMap<PathBuf, Vec<Key>>,
    /// `(call, DocType)` for each `frappe.get_doc`-family call a Python file
    /// makes with a literal DocType: what the type stub covers, call by call
    /// ([24](../../../docs/specs/24-type-integration.md)).
    constructed: BTreeMap<PathBuf, BTreeSet<(String, String)>>,
    /// Whether the scan has run. Until it has, references answers nothing
    /// rather than answering wrongly.
    pub scanned: bool,
}

impl Occurrences {
    /// Sites for one key, in file then position order.
    #[must_use]
    pub fn get(&self, key: &Key) -> &[Site] {
        self.sites.get(key).map_or(&[], Vec::as_slice)
    }

    /// Record one occurrence.
    pub fn insert(&mut self, key: Key, path: &Arc<Path>, range: Range) {
        let sites = self.sites.entry(key.clone()).or_default();
        let site = Site {
            path: Arc::clone(path),
            range,
        };
        if !sites.contains(&site) {
            sites.push(site);
        }
        let keys = self.by_file.entry(path.to_path_buf()).or_default();
        if !keys.contains(&key) {
            keys.push(key);
        }
    }

    /// Record that `path` calls `call` (`frappe.get_doc`, …) with `doctype`.
    pub fn construct(&mut self, path: &Path, call: &str, doctype: &str) {
        self.constructed
            .entry(path.to_path_buf())
            .or_default()
            .insert((call.to_string(), doctype.to_string()));
    }

    /// Every `(call, DocType)` some Python file in the workspace makes.
    #[must_use]
    pub fn constructed(&self) -> BTreeSet<(&str, &str)> {
        self.constructed
            .values()
            .flatten()
            .map(|(call, doctype)| (call.as_str(), doctype.as_str()))
            .collect()
    }

    /// Every file with a site naming one of `doctypes` or a field of one.
    #[must_use]
    pub fn files_naming(&self, doctypes: &BTreeSet<String>) -> BTreeSet<PathBuf> {
        self.sites
            .iter()
            .filter(|(key, _)| match key {
                Key::DocType(name) | Key::Field(name, _) => doctypes.contains(name),
            })
            .flat_map(|(_, sites)| sites.iter().map(|site| site.path.to_path_buf()))
            .collect()
    }

    /// Drop everything one file contributed.
    pub fn forget(&mut self, path: &Path) {
        self.constructed.remove(path);
        let Some(keys) = self.by_file.remove(path) else {
            return;
        };
        for key in keys {
            if let Some(sites) = self.sites.get_mut(&key) {
                sites.retain(|site| site.path.as_ref() != path);
                if sites.is_empty() {
                    self.sites.remove(&key);
                }
            }
        }
    }

    /// Sort every site list, so results are stable across runs.
    pub fn sort(&mut self) {
        for sites in self.sites.values_mut() {
            sites.sort_by(|a, b| a.path.cmp(&b.path).then(a.range.start.cmp(&b.range.start)));
        }
    }

    /// How many sites are held, for the status report.
    #[must_use]
    pub fn len(&self) -> usize {
        self.sites.values().map(Vec::len).sum()
    }

    /// Whether nothing has been recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sites.is_empty()
    }
}
