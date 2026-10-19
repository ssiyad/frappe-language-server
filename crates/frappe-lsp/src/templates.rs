//! Template paths in Jinja — [13](../../../docs/specs/13-surface-jinja.md).
//!
//! `{% include %}`, `{% extends %}`, `{% import %}` and `{% from %}` name a
//! template the way frappe's loader finds it, which is not relative to the
//! file that names it.

use std::path::PathBuf;

use frappe_index::Index;

/// Where frappe's loader finds `written`, or `None` when no app has it.
///
/// `frappe/utils/jinja.py` chains two loaders: a prefix loader that reads
/// `app/templates/x.html` from that app's package, then every app's package in
/// turn, most recently installed first, for `templates/x.html`.
#[must_use]
pub fn resolve(index: &Index, written: &str) -> Option<PathBuf> {
    let apps = index.apps();
    if let Some((first, rest)) = written.split_once('/')
        && let Some(app) = apps.iter().find(|app| {
            app.name == first || app.package.file_name().is_some_and(|name| name == first)
        })
    {
        let path = app.package.join(rest);
        if path.is_file() {
            return Some(path);
        }
    }

    let mut ordered: Vec<_> = apps.iter().collect();
    ordered.sort_by_key(|app| std::cmp::Reverse(app.order));
    ordered
        .into_iter()
        .map(|app| app.package.join(written))
        .find(|path| path.is_file())
}

/// Every template the loader could serve, by the path a template names it
/// with: relative to its app's package.
#[must_use]
pub fn names(index: &Index) -> Vec<String> {
    let mut names: Vec<String> = index
        .apps()
        .iter()
        .flat_map(|app| {
            frappe_index::walk::Walk::new(&app.package).filter_map(|path| {
                if path.extension().is_none_or(|extension| extension != "html") {
                    return None;
                }
                let relative = path.strip_prefix(&app.package).ok()?;
                Some(relative.to_string_lossy().replace('\\', "/"))
            })
        })
        .collect();
    names.sort();
    names.dedup();
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bench() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("repo root")
            .join("tests/fixtures/bench")
    }

    #[test]
    fn both_loader_forms_find_the_template() {
        let index = Index::build(&bench());
        let unprefixed = resolve(&index, "www/books.html").expect("in library");
        assert!(
            unprefixed.ends_with("library/library/www/books.html"),
            "{unprefixed:?}"
        );
        let prefixed = resolve(&index, "library/www/books.html").expect("by prefix");
        assert_eq!(prefixed, unprefixed);
        assert_eq!(resolve(&index, "templates/nowhere.html"), None);
    }

    #[test]
    fn names_are_the_paths_a_template_includes_by() {
        let index = Index::build(&bench());
        let names = names(&index);
        assert!(names.contains(&"www/books.html".to_string()), "{names:?}");
    }
}
