//! Configuration loading and merge — spec 30.

mod schema;
mod secrets;

use std::fmt;
use std::path::{Path, PathBuf};

use serde_json::Value;

pub use schema::*;

/// The name of the committed, team-wide config file, read from the bench root.
pub const BENCH_FILE: &str = ".frappe-lsp.toml";

/// Where a patch came from. Carried by every error so a rejected file can be
/// found without guessing which of the four sources produced it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    BenchFile(PathBuf),
    UserFile(PathBuf),
    InitializationOptions,
    WorkspaceConfiguration,
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BenchFile(path) | Self::UserFile(path) => write!(f, "{}", path.display()),
            Self::InitializationOptions => f.write_str("initializationOptions"),
            Self::WorkspaceConfiguration => f.write_str("workspace/didChangeConfiguration"),
        }
    }
}

#[derive(Debug)]
pub struct ConfigError {
    pub origin: Origin,
    pub kind: ErrorKind,
}

#[derive(Debug)]
pub enum ErrorKind {
    Read(std::io::Error),
    Syntax(String),
    Schema(String),
    /// Something that looks like a credential. The source is dropped whole.
    Credential(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let origin = &self.origin;
        match &self.kind {
            ErrorKind::Read(error) => write!(f, "{origin}: {error}"),
            ErrorKind::Syntax(message) => write!(f, "{origin}: {message}"),
            ErrorKind::Schema(message) => write!(f, "{origin}: {message}"),
            ErrorKind::Credential(key) => write!(
                f,
                "{origin}: `{key}` looks like a credential; the whole file is ignored. \
                 Credentials belong in the environment or a frappe-cli profile (spec 05)."
            ),
        }
    }
}

impl std::error::Error for ConfigError {}

/// The four sources of spec 30, kept apart so a `didChangeConfiguration` can be
/// re-merged without re-reading the files.
#[derive(Debug, Default)]
pub struct ConfigSources {
    bench_file: ConfigPatch,
    user_file: ConfigPatch,
    initialization_options: ConfigPatch,
    workspace_configuration: ConfigPatch,
    bench_root: Option<PathBuf>,
}

impl ConfigSources {
    /// Load sources 1–3. Errors are returned, never fatal: a bad config file
    /// must not cost the user the session.
    pub fn load(
        workspace_roots: &[PathBuf],
        initialization_options: Option<Value>,
    ) -> (Self, Vec<ConfigError>) {
        let mut errors = Vec::new();
        let mut sources = Self::default();

        if let Some(value) = initialization_options {
            sources.initialization_options =
                collect(parse(Origin::InitializationOptions, value), &mut errors);
        }

        if let Some(path) = user_config_path() {
            sources.user_file = collect(read_file(Origin::UserFile(path)), &mut errors);
        }

        // The bench file lives at the bench root, which the config itself may
        // name — so the two higher sources are consulted for `workspace.root`
        // before the lower one can be located.
        sources.bench_root = configured_root(&sources.initialization_options)
            .or_else(|| configured_root(&sources.user_file))
            .or_else(|| workspace_roots.first().cloned());

        if let Some(root) = &sources.bench_root {
            let path = root.join(BENCH_FILE);
            sources.bench_file = collect(read_file(Origin::BenchFile(path)), &mut errors);
        }

        (sources, errors)
    }

    /// Replace source 4 from a `workspace/didChangeConfiguration` payload.
    pub fn set_workspace_configuration(&mut self, settings: Value) -> Result<(), ConfigError> {
        self.workspace_configuration = parse(Origin::WorkspaceConfiguration, unwrap(settings))?;
        Ok(())
    }

    /// The directory the bench file was looked for in, if any.
    pub fn bench_root(&self) -> Option<&Path> {
        self.bench_root.as_deref()
    }

    /// Merge in the order of spec 30 and fill in defaults.
    pub fn resolve(&self) -> Config {
        let mut merged = self.bench_file.clone();
        // `.frappe-lsp.toml` is committed team-wide configuration, which means
        // it arrives with a `git clone` from whoever wrote the repository. It
        // may not choose what gets executed: `test.bench` from that source is
        // dropped, so a cloned workspace cannot point the one feature that
        // runs anything at a program of its choosing
        // ([25](../../../../docs/specs/25-test-runner.md)).
        if let Some(test) = merged.test.as_mut() {
            test.bench = None;
        }
        merged.merge(self.user_file.clone());
        merged.merge(self.initialization_options.clone());
        merged.merge(self.workspace_configuration.clone());
        Config::from_patch(&merged)
    }
}

fn collect(result: Result<ConfigPatch, ConfigError>, errors: &mut Vec<ConfigError>) -> ConfigPatch {
    match result {
        Ok(patch) => patch,
        Err(error) => {
            errors.push(error);
            ConfigPatch::default()
        }
    }
}

fn read_file(origin: Origin) -> Result<ConfigPatch, ConfigError> {
    let (Origin::BenchFile(path) | Origin::UserFile(path)) = &origin else {
        unreachable!("read_file is only called with file origins")
    };

    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        // An absent config file is the zero-config launch, not an error.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ConfigPatch::default());
        }
        Err(error) => {
            return Err(ConfigError {
                origin,
                kind: ErrorKind::Read(error),
            });
        }
    };

    // TOML and JSON reach the schema through the same value, so the two source
    // kinds cannot drift in key structure or in what counts as a credential.
    let value: Value = toml::from_str(&text).map_err(|error| ConfigError {
        origin: origin.clone(),
        kind: ErrorKind::Syntax(error.to_string()),
    })?;

    parse(origin, value)
}

fn parse(origin: Origin, value: Value) -> Result<ConfigPatch, ConfigError> {
    if let Some(key) = secrets::find(&value) {
        return Err(ConfigError {
            origin,
            kind: ErrorKind::Credential(key),
        });
    }

    serde_json::from_value(value).map_err(|error| ConfigError {
        origin,
        kind: ErrorKind::Schema(error.to_string()),
    })
}

/// Clients commonly nest their settings under the server's name.
fn unwrap(settings: Value) -> Value {
    let Value::Object(map) = &settings else {
        return settings;
    };
    for key in ["frappe-lsp", "frappeLsp", "frappe_lsp"] {
        if let Some(inner) = map.get(key) {
            return inner.clone();
        }
    }
    settings
}

fn configured_root(patch: &ConfigPatch) -> Option<PathBuf> {
    match patch.workspace.as_ref()?.root.as_ref()? {
        WorkspaceRoot::Auto => None,
        WorkspaceRoot::Path(path) => Some(path.clone()),
    }
}

fn user_config_path() -> Option<PathBuf> {
    let base = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(value) if !value.is_empty() => PathBuf::from(value),
        _ => PathBuf::from(std::env::var_os("HOME")?).join(".config"),
    };
    Some(base.join("frappe-lsp").join("config.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_bench_file_is_not_an_error() {
        let path = std::env::temp_dir()
            .join("frappe-lsp-no-such-bench")
            .join(BENCH_FILE);
        let patch = read_file(Origin::BenchFile(path)).expect("absent file is fine");
        assert_eq!(Config::from_patch(&patch), Config::default());
    }

    #[test]
    fn a_bench_file_is_read_from_the_bench_root() {
        let root = std::env::temp_dir().join(format!("frappe-lsp-bench-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("temp dir");
        std::fs::write(root.join(BENCH_FILE), "[index]\ncache = false\n").expect("write");

        let patch = read_file(Origin::BenchFile(root.join(BENCH_FILE))).expect("valid file");
        assert!(!Config::from_patch(&patch).index.cache);

        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn a_malformed_bench_file_names_its_path() {
        let root = std::env::temp_dir().join(format!("frappe-lsp-bad-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("temp dir");
        let path = root.join(BENCH_FILE);
        std::fs::write(&path, "[index\ncache = false\n").expect("write");

        let error = read_file(Origin::BenchFile(path.clone())).expect_err("malformed");
        assert!(matches!(error.kind, ErrorKind::Syntax(_)));
        assert!(error.to_string().contains(&path.display().to_string()));

        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn a_committed_config_cannot_choose_what_gets_executed() {
        // `.frappe-lsp.toml` arrives with a `git clone`. Every other key in it
        // is the repository's business; the program the test runner spawns is
        // not ([25](../../../../docs/specs/25-test-runner.md)).
        let sources = ConfigSources {
            bench_file: parse(
                Origin::BenchFile("/x/.frappe-lsp.toml".into()),
                serde_json::json!({"test": {"bench": "/tmp/not-bench", "failfast": true}}),
            )
            .expect("valid"),
            ..Default::default()
        };

        let config = sources.resolve();
        assert_eq!(
            config.test.bench, "bench",
            "a cloned repo chose the program"
        );
        // The rest of the block still applies: only the executable is refused.
        assert!(config.test.failfast);
    }

    #[test]
    fn a_user_file_may_still_set_the_bench_path() {
        // The user's own config is theirs; a venv-local `bench` is ordinary.
        let sources = ConfigSources {
            user_file: parse(
                Origin::UserFile("/home/x/config.toml".into()),
                serde_json::json!({"test": {"bench": "/home/x/env/bin/bench"}}),
            )
            .expect("valid"),
            ..Default::default()
        };
        assert_eq!(sources.resolve().test.bench, "/home/x/env/bin/bench");
    }

    #[test]
    fn a_credential_rejects_the_whole_source() {
        let error = parse(
            Origin::InitializationOptions,
            serde_json::json!({"bridge": {"enabled": true, "api_secret": "x"}}),
        )
        .expect_err("credential");

        assert!(matches!(error.kind, ErrorKind::Credential(ref key) if key == "bridge.api_secret"));
        assert!(error.to_string().contains("initializationOptions"));
    }

    #[test]
    fn workspace_configuration_wins_over_initialization_options() {
        let mut sources = ConfigSources {
            initialization_options: parse(
                Origin::InitializationOptions,
                serde_json::json!({"hover": {"enabled": false}}),
            )
            .expect("valid"),
            ..Default::default()
        };
        sources
            .set_workspace_configuration(serde_json::json!({"hover": {"enabled": true}}))
            .expect("valid");

        assert!(sources.resolve().hover.enabled);
    }

    #[test]
    fn settings_nested_under_the_server_name_are_unwrapped() {
        let mut sources = ConfigSources::default();
        sources
            .set_workspace_configuration(serde_json::json!({
                "frappe-lsp": {"completion": {"max_items": 7}}
            }))
            .expect("valid");

        assert_eq!(sources.resolve().completion.max_items, 7);
    }
}
