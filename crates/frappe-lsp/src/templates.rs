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
}
