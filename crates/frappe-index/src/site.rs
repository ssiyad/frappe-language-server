//! Sites — spec 03, "Site configuration".
//!
//! `sites/` is never walked. Two things there are worth reading, and both are
//! opened by name.

use std::path::{Path, PathBuf};

/// A site directory under `sites/`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Site {
    /// The directory name, which is the site name.
    pub name: String,
    /// `sites/<name>/site_config.json`. Supplies the bridge target
    /// ([05](../../../docs/specs/05-live-bridge.md)); credentials in it MUST
    /// NOT be read by default ([30](../../../docs/specs/30-configuration.md)).
    pub config: PathBuf,
}

/// Every site, and the one `currentsite.txt` names.
#[must_use]
pub fn discover(sites: &Path) -> (Vec<Site>, Option<String>) {
    let mut found = Vec::new();
    if let Ok(entries) = std::fs::read_dir(sites) {
        for entry in entries.flatten() {
            let path = entry.path();
            let config = path.join("site_config.json");
            // `assets` and `apps.json` are the noise this filter removes: a
            // site is a directory carrying a site_config.json, nothing else.
            if !config.is_file() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            found.push(Site {
                name: name.to_string(),
                config,
            });
        }
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));

    let current = std::fs::read_to_string(sites.join("currentsite.txt"))
        .ok()
        .map(|text| text.trim().to_string())
        .filter(|name| !name.is_empty());

    (found, current)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sites() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("repo root")
            .join("tests/fixtures/bench/sites")
    }

    #[test]
    fn a_site_is_a_directory_with_a_site_config() {
        let (found, current) = discover(&sites());

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "library.localhost");
        assert!(found[0].config.is_file());
        assert_eq!(current.as_deref(), Some("library.localhost"));
    }

    #[test]
    fn assets_is_not_a_site() {
        let (found, _) = discover(&sites());
        assert!(found.iter().all(|site| site.name != "assets"));
    }
}
