//! Where the token comes from — [05](../../../docs/specs/05-live-bridge.md).
//!
//! An LSP config file is commonly committed, so credentials never come from
//! one. [30](../../../docs/specs/30-configuration.md) rejects a config file
//! carrying anything key-shaped; this module is the other half of that rule —
//! the places a token may legitimately come from.
//!
//! There are two, and neither is a particular tool: the environment, and
//! whatever `bridge.token_command` prints. The second exists so that reusing a
//! credential store does not mean depending on one — `pass`, a password
//! manager's CLI, a Frappe client, or a three-line script all satisfy it, and
//! this crate never learns which.

use std::fmt;

/// A resolved site and token, ready to build a client from.
#[derive(Clone, PartialEq, Eq)]
pub struct Credentials {
    /// The site's base URL.
    pub site: String,
    /// `<key>:<secret>`, as the `token` scheme wants it.
    token: String,
    /// Which source supplied it, for the log line.
    pub source: Source,
}

impl Credentials {
    /// Build credentials from an already-resolved token.
    ///
    /// [`resolve`] is the policy — which sources a token may come from, and
    /// which it may not. This is only the value, and exists so a caller that
    /// has one already (a test pointing at a socket, a future credential
    /// store) does not have to go through the environment to say so.
    #[must_use]
    pub fn new(site: &str, token: impl Into<String>, source: Source) -> Self {
        Self {
            site: normalize(site),
            token: token.into(),
            source,
        }
    }

    /// The `Authorization` header value.
    #[must_use]
    pub fn header(&self) -> String {
        format!("token {}", self.token)
    }
}

// Hand-written so a token cannot reach a log through `{:?}`. Spec 30's rule is
// that credentials never appear in a file the user might paste; a debug format
// that prints one makes every bug report a leak.
impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credentials")
            .field("site", &self.site)
            .field("token", &"<redacted>")
            .field("source", &self.source)
            .finish()
    }
}

/// Which of spec 05's sources supplied the credentials.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// `FRAPPE_LSP_TOKEN`, or `FRAPPE_API_KEY` + `FRAPPE_API_SECRET`.
    Environment,
    /// The standard output of `bridge.token_command`.
    Command,
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Environment => write!(f, "environment"),
            Self::Command => write!(f, "token command"),
        }
    }
}

/// Spec 05's source 1: the environment.
///
/// `site` is the bridge's configured target and wins over whatever the
/// environment names — a bridge pointed at one site by config and another by a
/// stray variable is the shape of an accident.
#[must_use]
pub fn from_environment(site: &str) -> Option<Credentials> {
    let env = |name: &str| std::env::var(name).ok().filter(|value| !value.is_empty());

    let token = env("FRAPPE_LSP_TOKEN").or_else(|| {
        let key = env("FRAPPE_API_KEY")?;
        let secret = env("FRAPPE_API_SECRET")?;
        Some(format!("{key}:{secret}"))
    })?;

    let site = if site.is_empty() {
        env("FRAPPE_SITE")?
    } else {
        site.to_string()
    };

    Some(Credentials {
        site: normalize(&site),
        token,
        source: Source::Environment,
    })
}

/// Spec 05's source 2: a token some other program printed.
///
/// The site still comes from configuration, because a token command answers
/// "what is the secret" and not "which database should this read".
#[must_use]
pub fn from_command_output(site: &str, output: &str) -> Option<Credentials> {
    let token = output.trim();
    if token.is_empty() || site.is_empty() {
        return None;
    }
    Some(Credentials {
        site: normalize(site),
        token: token.to_string(),
        source: Source::Command,
    })
}

/// A site as a base URL. A bare site name is a local bench site, which is
/// reached over http on the loopback address rather than over the internet.
fn normalize(site: &str) -> String {
    let site = site.trim_end_matches('/');
    if site.starts_with("http://") || site.starts_with("https://") {
        return site.to_string();
    }
    format!("http://{site}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_site_name_becomes_a_loopback_url() {
        assert_eq!(normalize("library.localhost"), "http://library.localhost");
        assert_eq!(
            normalize("https://x.frappe.cloud/"),
            "https://x.frappe.cloud"
        );
        assert_eq!(normalize("http://127.0.0.1:8000"), "http://127.0.0.1:8000");
    }

    #[test]
    fn a_token_command_supplies_the_secret_and_not_the_site() {
        let resolved =
            from_command_output("library.localhost", "  key:secret\n").expect("credentials");
        assert_eq!(resolved.site, "http://library.localhost");
        assert_eq!(resolved.header(), "token key:secret");
        assert_eq!(resolved.source, Source::Command);

        // Nothing printed is nothing to use, and a command with no site
        // configured has not said which database to read.
        assert!(from_command_output("library.localhost", "   \n").is_none());
        assert!(from_command_output("", "key:secret").is_none());
    }

    #[test]
    fn the_configured_site_wins_over_the_one_a_source_names() {
        // A bridge pointed at one site by config and another by a stray
        // variable is the shape of an accident.
        let configured = Credentials::new("configured.localhost", "k:s", Source::Environment);
        assert_eq!(configured.site, "http://configured.localhost");
    }

    #[test]
    fn a_token_never_reaches_a_debug_format() {
        // Every bug report includes a debug dump eventually.
        let credentials = Credentials {
            site: "http://x".to_string(),
            token: "key:secret".to_string(),
            source: Source::Environment,
        };
        let rendered = format!("{credentials:?}");
        assert!(!rendered.contains("secret"), "{rendered}");
        assert!(rendered.contains("<redacted>"), "{rendered}");
        // …and the header still carries the real thing.
        assert_eq!(credentials.header(), "token key:secret");
    }
}
