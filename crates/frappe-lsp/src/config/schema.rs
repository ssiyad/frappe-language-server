//! The configuration schema of `docs/specs/30-configuration.md`.
//!
//! Every section exists twice: a resolved struct carrying the defaults, and a
//! patch struct of `Option` fields. Only the patch is deserialized — a source
//! that omits a key must not overwrite a lower source with a default, which is
//! indistinguishable from an explicit value once defaults are filled in.

use std::path::PathBuf;

use serde::Deserialize;

macro_rules! config_schema {
    (
        $(
            $(#[$section_meta:meta])*
            $name:ident: $Section:ident / $Patch:ident {
                $( $field:ident: $ty:ty = $default:expr ),* $(,)?
            }
        )*
    ) => {
        /// Effective configuration: every source merged, defaults filled in.
        #[derive(Debug, Clone, Default, PartialEq)]
        pub struct Config {
            $( pub $name: $Section, )*
        }

        /// One configuration source, before merging.
        #[derive(Debug, Clone, Default, Deserialize)]
        #[serde(default)]
        pub struct ConfigPatch {
            $( pub $name: Option<$Patch>, )*
        }

        impl ConfigPatch {
            /// Overlay `other` on top of `self`; `other` wins key by key.
            pub fn merge(&mut self, other: Self) {
                $(
                    match (&mut self.$name, other.$name) {
                        (_, None) => {}
                        (slot @ None, incoming) => *slot = incoming,
                        (Some(existing), Some(incoming)) => existing.merge(incoming),
                    }
                )*
            }
        }

        impl Config {
            /// Resolve a merged patch against the defaults.
            pub fn from_patch(patch: &ConfigPatch) -> Self {
                let mut config = Self::default();
                $(
                    if let Some(section) = &patch.$name {
                        section.apply(&mut config.$name);
                    }
                )*
                config
            }
        }

        $(
            #[derive(Debug, Clone, PartialEq)]
            pub struct $Section {
                $( pub $field: $ty, )*
            }

            impl Default for $Section {
                fn default() -> Self {
                    Self { $( $field: $default, )* }
                }
            }

            #[derive(Debug, Clone, Default, Deserialize)]
            #[serde(default)]
            $(#[$section_meta])*
            pub struct $Patch {
                $( pub $field: Option<$ty>, )*
            }

            impl $Patch {
                fn merge(&mut self, other: Self) {
                    $( if other.$field.is_some() { self.$field = other.$field; } )*
                }

                fn apply(&self, target: &mut $Section) {
                    $( if let Some(value) = &self.$field { target.$field = value.clone(); } )*
                }
            }
        )*
    };
}

config_schema! {
    workspace: Workspace / WorkspacePatch {
        root: WorkspaceRoot = WorkspaceRoot::Auto,
        apps: Vec<String> = Vec::new(),
        exclude: Vec<String> = Vec::new(),
    }

    index: Index / IndexPatch {
        cache: bool = true,
        cache_dir: PathBuf = PathBuf::from(".frappe-lsp"),
    }

    bridge: Bridge / BridgePatch {
        enabled: bool = false,
        site: String = String::new(),
        profile: String = String::new(),
        refresh_interval: u64 = 0,
    }

    #[serde(rename_all = "kebab-case")]
    diagnostics: Diagnostics / DiagnosticsPatch {
        enabled: bool = true,
        unknown_doctype: Level = Level::Warning,
        unknown_field: Level = Level::Warning,
        non_whitelisted_call: Level = Level::Error,
        unknown_hook: Level = Level::Warning,
        unknown_doc_event: Level = Level::Error,
        translation_literal: Level = Level::Error,
    }

    types: Types / TypesPatch {
        stub: bool = false,
        stub_dir: PathBuf = PathBuf::from(".frappe-lsp/stubs"),
        scope: StubScope = StubScope::Referenced,
        cast_action: bool = true,
    }

    completion: Completion / CompletionPatch {
        max_items: usize = 1000,
        detail: bool = true,
    }

    hover: Hover / HoverPatch {
        enabled: bool = true,
    }
}

/// `workspace.root`: `"auto"` or an absolute path.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub enum WorkspaceRoot {
    Auto,
    Path(PathBuf),
}

impl TryFrom<String> for WorkspaceRoot {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value == "auto" {
            return Ok(Self::Auto);
        }
        let path = PathBuf::from(&value);
        if path.is_absolute() {
            Ok(Self::Path(path))
        } else {
            Err(format!(
                "workspace.root must be \"auto\" or an absolute path, got {value:?}"
            ))
        }
    }
}

/// Severity of a diagnostic check, or `Off` to disable it — spec 23.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Error,
    Warning,
    Info,
    Hint,
    Off,
}

/// `types.scope` — spec 24.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StubScope {
    Referenced,
    All,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patch(toml_source: &str) -> ConfigPatch {
        let value: serde_json::Value = toml::from_str(toml_source).expect("valid TOML");
        serde_json::from_value(value).expect("valid config")
    }

    #[test]
    fn unset_keys_fall_back_to_defaults() {
        let config = Config::from_patch(&patch("[hover]\nenabled = false\n"));
        assert!(!config.hover.enabled);
        assert_eq!(config.completion.max_items, 1000);
        assert_eq!(config.diagnostics.unknown_field, Level::Warning);
    }

    #[test]
    fn later_source_wins_key_by_key() {
        let mut merged =
            patch("[diagnostics]\nunknown-field = \"off\"\nunknown-hook = \"error\"\n");
        merged.merge(patch("[diagnostics]\nunknown-hook = \"hint\"\n"));

        let config = Config::from_patch(&merged);
        assert_eq!(config.diagnostics.unknown_field, Level::Off);
        assert_eq!(config.diagnostics.unknown_hook, Level::Hint);
        // Untouched by both patches.
        assert_eq!(config.diagnostics.unknown_doc_event, Level::Error);
    }

    #[test]
    fn a_later_section_does_not_replace_an_earlier_one_wholesale() {
        let mut merged = patch("[bridge]\nenabled = true\nsite = \"dev.localhost\"\n");
        merged.merge(patch("[bridge]\nrefresh_interval = 30\n"));

        let config = Config::from_patch(&merged);
        assert!(config.bridge.enabled);
        assert_eq!(config.bridge.site, "dev.localhost");
        assert_eq!(config.bridge.refresh_interval, 30);
    }

    #[test]
    fn json_uses_the_same_key_structure_as_toml() {
        let json: ConfigPatch = serde_json::from_str(
            r#"{"diagnostics": {"unknown-doctype": "off"}, "completion": {"max_items": 50}}"#,
        )
        .expect("valid config");

        let config = Config::from_patch(&json);
        assert_eq!(config.diagnostics.unknown_doctype, Level::Off);
        assert_eq!(config.completion.max_items, 50);
    }

    #[test]
    fn relative_workspace_root_is_rejected() {
        let value: serde_json::Value =
            toml::from_str("[workspace]\nroot = \"../bench\"\n").expect("valid TOML");
        let error = serde_json::from_value::<ConfigPatch>(value).expect_err("relative path");
        assert!(error.to_string().contains("absolute path"));
    }
}
