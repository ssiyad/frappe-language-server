//! The three grammars of [06](../../../docs/specs/06-analysis-pipeline.md).

use std::fmt;
use std::path::Path;

/// A grammar this crate can parse with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    /// Controllers, `hooks.py`, reports, patches.
    Python,
    /// Form scripts, list views, bundles.
    JavaScript,
    /// DocType and fixture files.
    ///
    /// The index reads these with `serde_json`; the grammar exists only so an
    /// open buffer has in-buffer positions
    /// ([06](../../../docs/specs/06-analysis-pipeline.md)).
    Json,
    /// Jinja templates. Parsed with the Python grammar after
    /// [`crate::jinja::to_python`] rewrites the template to a Python document
    /// of the same bytes ([13](../../../docs/specs/13-surface-jinja.md)).
    Jinja,
}

impl Language {
    /// The language the extractor reads it as: a template's expressions are
    /// Python.
    #[must_use]
    pub fn reads_as(self) -> Self {
        match self {
            Self::Jinja => Self::Python,
            other => other,
        }
    }

    /// The grammar a file extension selects, or `None` for a file this crate
    /// has nothing to say about.
    #[must_use]
    pub fn from_path(path: &Path) -> Option<Self> {
        match path.extension()?.to_str()? {
            "py" => Some(Self::Python),
            "js" => Some(Self::JavaScript),
            "json" => Some(Self::Json),
            "html" | "jinja" | "j2" => Some(Self::Jinja),
            _ => None,
        }
    }

    /// The compiled grammar.
    #[must_use]
    pub fn grammar(self) -> tree_sitter::Language {
        match self {
            Self::Python | Self::Jinja => tree_sitter_python::LANGUAGE.into(),
            Self::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Self::Json => tree_sitter_json::LANGUAGE.into(),
        }
    }

    /// Parse `source` the way this language is read: a template through its
    /// Python rewrite ([`crate::jinja::to_python`]), anything else as it is.
    /// Every caller that parses a file of any language goes through here, so a
    /// template is never handed to the Python grammar raw.
    pub fn parse(
        self,
        parser: &mut tree_sitter::Parser,
        source: &str,
    ) -> Option<tree_sitter::Tree> {
        match self {
            Self::Jinja => parser.parse(crate::jinja::to_python(source), None),
            _ => parser.parse(source, None),
        }
    }

    /// A parser bound to this grammar.
    ///
    /// Fails only on an ABI mismatch between `tree-sitter` and a grammar crate,
    /// which is fixed at compile time by the workspace pins — but a `Result`
    /// rather than a panic, because a panic ends the session.
    pub fn parser(self) -> Result<tree_sitter::Parser, GrammarError> {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&self.grammar())
            .map_err(|source| GrammarError::Abi {
                language: self,
                source,
            })?;
        Ok(parser)
    }
}

impl fmt::Display for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Python => "Python",
            Self::JavaScript => "JavaScript",
            Self::Json => "JSON",
            Self::Jinja => "Jinja",
        })
    }
}

/// A buffer that could not be parsed, naming the grammar that refused it.
#[derive(Debug)]
pub enum GrammarError {
    /// The grammar's ABI does not match the linked `tree-sitter`. Fixed at
    /// compile time by the workspace pins, so it is a build error in disguise.
    Abi {
        /// The grammar that failed to load.
        language: Language,
        /// What `tree-sitter` reported.
        source: tree_sitter::LanguageError,
    },
    /// The parser returned no tree at all — cancellation or exhaustion, not a
    /// syntax error. A syntax error yields a tree carrying `ERROR` nodes.
    Empty {
        /// The grammar that produced nothing.
        language: Language,
    },
}

impl GrammarError {
    /// The grammar involved, whichever way the parse failed.
    #[must_use]
    pub fn language(&self) -> Language {
        match self {
            Self::Abi { language, .. } | Self::Empty { language } => *language,
        }
    }
}

impl fmt::Display for GrammarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Abi { language, source } => {
                write!(f, "{language} grammar rejected: {source}")
            }
            Self::Empty { language } => write!(f, "{language} parser produced no tree"),
        }
    }
}

impl std::error::Error for GrammarError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Abi { source, .. } => Some(source),
            Self::Empty { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_select_the_grammar() {
        assert_eq!(
            Language::from_path(Path::new("a/b/library_member.py")),
            Some(Language::Python)
        );
        assert_eq!(
            Language::from_path(Path::new("a/b/library_member.js")),
            Some(Language::JavaScript)
        );
        assert_eq!(
            Language::from_path(Path::new("a/b/library_member.json")),
            Some(Language::Json)
        );
        assert_eq!(
            Language::from_path(Path::new("a/www/books.html")),
            Some(Language::Jinja)
        );
    }

    #[test]
    fn files_this_crate_does_not_read_select_nothing() {
        for path in ["hooks.txt", "patches.txt", "library.css", "x"] {
            assert_eq!(Language::from_path(Path::new(path)), None, "{path}");
        }
    }

    #[test]
    fn every_grammar_loads_under_the_pinned_abi() {
        for language in [
            Language::Python,
            Language::JavaScript,
            Language::Json,
            Language::Jinja,
        ] {
            assert!(language.parser().is_ok(), "{language}");
        }
    }
}
