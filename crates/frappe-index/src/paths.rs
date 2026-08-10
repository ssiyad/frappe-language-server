//! The one dotted-path resolver of [07](../../../docs/specs/07-dotted-paths.md).
//!
//! Hooks, patches, `frappe.call`, `frappe.enqueue`, reports and fixtures all
//! spell targets the same way, so they all come through here. Callers declare
//! the [`Kind`] they expect, which is what lets the resolver tell
//! `unresolvable-path` apart from `wrong-path-kind`
//! ([23](../../../docs/specs/23-diagnostics.md)).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::python::{self, Whitelist};
use crate::span::Range;

/// What the caller requires of a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// A module-level `def` or `class`.
    Callable,
    /// [`Kind::Callable`] and decorated with `@frappe.whitelist()`.
    Whitelisted,
    /// A module, with no symbol segment.
    Module,
    /// A `class` statement.
    Class,
    /// `Class.method` on a resolved class.
    Method,
}

/// What a resolved path landed on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SymbolKind {
    /// A `.py` file or a package `__init__.py`.
    Module,
    /// A module-level `def`.
    Function,
    /// A `class` statement.
    Class,
    /// A `def` in a class body.
    Method,
}

/// One thing a path could mean. Go-to-definition presents the list rather than
/// the resolver guessing ([07](../../../docs/specs/07-dotted-paths.md)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// The file holding it.
    pub path: PathBuf,
    /// What it is.
    pub symbol: SymbolKind,
    /// The symbol name, empty for a module.
    pub name: String,
    /// The definition, body included. Empty for a module — the whole file is
    /// the target.
    pub range: Range,
    /// The name alone, for `targetSelectionRange`.
    pub selection_range: Range,
    /// Whether this candidate satisfies the requested [`Kind`].
    pub matches_kind: bool,
}

/// Whether a path resolved, and how well.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// At least one candidate satisfies the requested kind.
    Resolved,
    /// Something was found, but nothing of the requested kind.
    WrongKind,
    /// Found, but the requested property cannot be determined — a
    /// [`Kind::Whitelisted`] path behind decorator indirection. MUST NOT
    /// produce a diagnostic.
    Unknown,
    /// Nothing was found.
    Unresolved,
}

/// The answer to one resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    /// The path as written.
    pub dotted: String,
    /// Everything the path could mean, kind-matching candidates first.
    pub candidates: Vec<Candidate>,
    /// Whether the path satisfies the requested kind.
    pub outcome: Outcome,
}

impl Resolution {
    /// The first candidate satisfying the requested kind.
    #[must_use]
    pub fn best(&self) -> Option<&Candidate> {
        self.candidates
            .iter()
            .find(|candidate| candidate.matches_kind)
            .or_else(|| self.candidates.first())
    }
}

/// App package roots plus every Python module read from them.
#[derive(Debug, Clone, Copy)]
pub struct Resolver<'a> {
    /// `(package directory name, package directory)`, in app order. The dotted
    /// path's first segment is the package **directory** name, which is neither
    /// the app name nor the directory under `apps/`
    /// ([03](../../../docs/specs/03-workspace-model.md)).
    pub packages: &'a [(String, PathBuf)],
    /// Every `.py` under those packages, keyed by path.
    pub modules: &'a BTreeMap<PathBuf, python::Module>,
}

impl Resolver<'_> {
    /// Resolve `dotted`, requiring `kind`.
    #[must_use]
    pub fn resolve(&self, dotted: &str, kind: Kind) -> Resolution {
        let mut candidates = Vec::new();
        let mut unknown = false;

        let segments: Vec<&str> = dotted.split('.').filter(|s| !s.is_empty()).collect();
        if segments.is_empty() || segments.len() != dotted.split('.').count() {
            return Resolution {
                dotted: dotted.to_string(),
                candidates,
                outcome: Outcome::Unresolved,
            };
        }

        for (directory, mut at) in self.roots(&segments) {
            // 2. Walk remaining segments as directories while they exist.
            let mut directory = directory;
            while at < segments.len() && directory.join(segments[at]).is_dir() {
                directory = directory.join(segments[at]);
                at += 1;
            }

            // 3. The next segment is `<segment>.py`, else `<segment>/__init__.py`.
            let (module_path, next) = if at < segments.len() {
                let file = directory.join(format!("{}.py", segments[at]));
                let package = directory.join(segments[at]).join("__init__.py");
                if self.modules.contains_key(&file) {
                    (file, at + 1)
                } else if self.modules.contains_key(&package) {
                    (package, at + 1)
                } else {
                    continue;
                }
            } else {
                (directory.join("__init__.py"), at)
            };

            let Some(module) = self.modules.get(&module_path) else {
                continue;
            };
            let rest = &segments[next..];

            match rest {
                [] => candidates.push(Candidate {
                    path: module_path,
                    symbol: SymbolKind::Module,
                    name: String::new(),
                    range: Range::none(),
                    selection_range: Range::none(),
                    matches_kind: kind == Kind::Module,
                }),
                [symbol] => {
                    if let Some(function) = module.function(symbol) {
                        let whitelisted = match &function.whitelist {
                            Whitelist::Yes(_) => true,
                            Whitelist::Unknown => {
                                unknown |= kind == Kind::Whitelisted;
                                false
                            }
                            Whitelist::No => false,
                        };
                        candidates.push(Candidate {
                            path: module_path.clone(),
                            symbol: SymbolKind::Function,
                            name: (*symbol).to_string(),
                            range: function.range,
                            selection_range: function.name_range,
                            matches_kind: match kind {
                                Kind::Callable => true,
                                Kind::Whitelisted => whitelisted,
                                _ => false,
                            },
                        });
                    }
                    if let Some(class) = module.class(symbol) {
                        candidates.push(Candidate {
                            path: module_path.clone(),
                            symbol: SymbolKind::Class,
                            name: (*symbol).to_string(),
                            range: class.range,
                            selection_range: class.name_range,
                            matches_kind: matches!(kind, Kind::Callable | Kind::Class),
                        });
                    }
                }
                [class_name, method_name] => {
                    if let Some(method) = module
                        .class(class_name)
                        .and_then(|class| class.method(method_name))
                    {
                        if matches!(method.whitelist, Whitelist::Unknown) {
                            unknown |= kind == Kind::Whitelisted;
                        }
                        candidates.push(Candidate {
                            path: module_path.clone(),
                            symbol: SymbolKind::Method,
                            name: format!("{class_name}.{method_name}"),
                            range: method.range,
                            selection_range: method.name_range,
                            matches_kind: match kind {
                                Kind::Method => true,
                                Kind::Whitelisted => matches!(method.whitelist, Whitelist::Yes(_)),
                                _ => false,
                            },
                        });
                    }
                }
                // Deeper than `Class.method` is not a Python attribute path the
                // resolver can follow without evaluating code.
                _ => {}
            }
        }

        candidates.sort_by_key(|candidate| !candidate.matches_kind);
        let outcome = if candidates.iter().any(|candidate| candidate.matches_kind) {
            Outcome::Resolved
        } else if unknown {
            Outcome::Unknown
        } else if candidates.is_empty() {
            Outcome::Unresolved
        } else {
            Outcome::WrongKind
        };

        Resolution {
            dotted: dotted.to_string(),
            candidates,
            outcome,
        }
    }

    /// Every app root the path could start at, longest prefix first.
    ///
    /// Package directory names hold no dots, so a prefix is one segment; the
    /// loop is written over prefixes anyway because
    /// [07](../../../docs/specs/07-dotted-paths.md) states the rule that way and
    /// a nested package directory would otherwise be missed silently.
    fn roots(&self, segments: &[&str]) -> Vec<(PathBuf, usize)> {
        let mut roots = Vec::new();
        for (name, directory) in self.packages {
            let taken = name.split('.').count();
            if segments.len() >= taken && segments[..taken].join(".") == *name {
                roots.push((directory.clone(), taken));
            }
        }
        roots.sort_by_key(|(_, taken)| std::cmp::Reverse(*taken));
        roots
    }
}

/// The dotted module path of a `.py` file inside `package`.
///
/// `<apps>/library/library/api.py` in package `<apps>/library/library` is
/// `library.api`; an `__init__.py` is its own directory.
#[must_use]
pub fn dotted_module(package: &Path, file: &Path) -> Option<String> {
    let root = package.parent()?;
    let relative = file.strip_prefix(root).ok()?;

    let mut segments: Vec<String> = relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect();

    let last = segments.pop()?;
    let stem = last.strip_suffix(".py")?;
    if stem != "__init__" {
        segments.push(stem.to_string());
    }
    (!segments.is_empty()).then(|| segments.join("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bench() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("repo root")
            .join("tests/fixtures/bench")
    }

    #[test]
    fn a_module_path_is_relative_to_the_package_parent() {
        let package = bench().join("apps/library/library");
        assert_eq!(
            dotted_module(&package, &package.join("api.py")).as_deref(),
            Some("library.api")
        );
        assert_eq!(
            dotted_module(&package, &package.join("patches/v1_0/__init__.py")).as_deref(),
            Some("library.patches.v1_0")
        );
        assert_eq!(
            dotted_module(
                &package,
                &package.join("library/doctype/library_member/library_member.py")
            )
            .as_deref(),
            Some("library.library.doctype.library_member.library_member")
        );
        assert_eq!(dotted_module(&package, &package.join("library.js")), None);
    }
}
