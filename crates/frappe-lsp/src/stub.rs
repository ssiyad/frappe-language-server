//! Keeping the type stub of [24](../../../docs/specs/24-type-integration.md)
//! current. Off unless `types.stub` is set.

use std::path::PathBuf;
use std::sync::RwLock;

use frappe_index::Index;
use frappe_index::stub::{self, Scope};

use crate::config::{Config, StubScope};
use crate::guard::Recover;

/// Regenerate the stub and write it if it changed. Returns a line for the
/// user when there is something to say: the first write, or a failure.
pub fn refresh(index: &RwLock<Index>, config: &Config) -> Option<String> {
    if !config.types.stub {
        return None;
    }
    let scope = match config.types.scope {
        StubScope::Referenced => Scope::Referenced,
        StubScope::All => Scope::All,
    };
    let (generated, directory) = {
        let guard = index.read_or_recover();
        // Not before the reference table: an empty referenced set would write
        // a stub that types every call as `Document`, then rewrite it.
        if scope == Scope::Referenced && !guard.occurrences().scanned {
            return None;
        }
        (stub::generate(&guard, scope), directory(&guard, config)?)
    };
    match generated {
        Ok(generated) => match stub::write(&directory, &generated) {
            Ok(true) => {
                tracing::info!(
                    doctypes = generated.doctypes,
                    overloads = generated.overloads,
                    path = %directory.display(),
                    "type stub written"
                );
                Some(format!(
                    "frappe-lsp: type stub for {} DocTypes written to {}; point pyright's stubPath at it",
                    generated.doctypes,
                    directory.display()
                ))
            }
            Ok(false) => None,
            Err(error) => Some(format!(
                "frappe-lsp: could not write the type stub: {error}"
            )),
        },
        Err(why) => Some(format!("frappe-lsp: no type stub: {why}")),
    }
}

/// `types.stub_dir`, against the bench when relative. Never inside `apps/`:
/// the stub is a build artifact, not source.
fn directory(index: &Index, config: &Config) -> Option<PathBuf> {
    let detection = &index.workspace().detection;
    let bench = detection.bench_root()?;
    let directory = if config.types.stub_dir.is_absolute() {
        config.types.stub_dir.clone()
    } else {
        bench.join(&config.types.stub_dir)
    };
    (!directory.starts_with(bench.join("apps"))).then_some(directory)
}
