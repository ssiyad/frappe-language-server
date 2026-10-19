//! The transport — [05](../../../docs/specs/05-live-bridge.md).
//!
//! `GET` against `/api/v2`, and nothing else. The bridge reads a site it does
//! not own: a server that can write to a production database because a config
//! key was mistyped is not a trade worth making, so the method is not a
//! parameter anywhere in this module.

use std::time::Duration;

use serde::de::DeserializeOwned;

use crate::credentials::Credentials;

/// How long a single request may take before the site counts as unreachable.
const TIMEOUT: Duration = Duration::from_secs(10);

/// `/api/v2` pages by `start` and `limit`, and `limit` defaults to 20 — which
/// would make a sync of 5 000 Custom Fields 250 round trips.
const PAGE: usize = 500;

/// Total rows accepted for one doctype, so a pathological site cannot make the
/// server allocate without bound.
const CEILING: usize = 50_000;

/// A read-only client for one site.
#[derive(Debug, Clone)]
pub struct Client {
    http: reqwest::Client,
    credentials: Credentials,
}

/// What went wrong. Every variant means the same thing to the caller — the
/// bridge is `Degraded` — but the message differs, and it is the only thing
/// the user has to go on.
#[derive(Debug)]
pub enum Error {
    /// The site could not be reached, or answered too slowly.
    Unreachable(String),
    /// The site answered, and said no.
    Rejected {
        /// The HTTP status.
        status: u16,
        /// The first line of the body, which is where Frappe puts the reason.
        message: String,
    },
    /// The site answered with something this client cannot read.
    Malformed(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreachable(why) => write!(f, "site unreachable: {why}"),
            Self::Rejected { status, message } => {
                write!(f, "site refused the request ({status}): {message}")
            }
            Self::Malformed(why) => write!(f, "site answered with {why}"),
        }
    }
}

impl std::error::Error for Error {}

impl Client {
    /// Build a client. Fails only if the TLS backend cannot start.
    ///
    /// # Errors
    ///
    /// When the underlying HTTP client cannot be constructed.
    pub fn new(credentials: Credentials) -> Result<Self, Error> {
        let http = reqwest::Client::builder()
            .timeout(TIMEOUT)
            .user_agent(concat!("frappe-lsp/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| Error::Unreachable(error.to_string()))?;
        Ok(Self { http, credentials })
    }

    /// The site this client reads.
    #[must_use]
    pub fn site(&self) -> &str {
        &self.credentials.site
    }

    /// Every row of one DocType, paged.
    ///
    /// # Errors
    ///
    /// Any transport failure, refusal, or unreadable body.
    pub async fn list<T: DeserializeOwned>(
        &self,
        doctype: &str,
        fields: &[&str],
        filters: Option<&str>,
    ) -> Result<Vec<T>, Error> {
        let mut rows = Vec::new();
        let mut start = 0usize;

        loop {
            let fields = serde_json::to_string(fields)
                .map_err(|error| Error::Malformed(error.to_string()))?;
            let start_text = start.to_string();
            let page = PAGE.to_string();
            // `start` and `limit`, not v1's `limit_start` and
            // `limit_page_length`: v2 ignores those, answers 20 rows, and a
            // sync that reads 20 of 3 000 Custom Fields reports the rest as
            // absent.
            let mut query: Vec<(&str, &str)> = vec![
                ("fields", fields.as_str()),
                ("start", start_text.as_str()),
                ("limit", page.as_str()),
                // Without an order the page boundaries are not stable and a
                // sync can miss or repeat rows.
                ("order_by", "name asc"),
            ];
            if let Some(filters) = filters {
                query.push(("filters", filters));
            }

            let (mut page_rows, more): (Vec<T>, Option<bool>) = self.get(doctype, &query).await?;
            let count = page_rows.len();
            rows.append(&mut page_rows);

            // The site says whether there is more. A site that does not say is
            // read until a page comes back short.
            let more = more.unwrap_or(count >= PAGE);
            if !more || count == 0 || rows.len() >= CEILING {
                if rows.len() >= CEILING {
                    tracing::warn!(doctype, ceiling = CEILING, "stopped reading at the ceiling");
                }
                break;
            }
            start += PAGE;
        }
        Ok(rows)
    }

    /// One `GET` against `/api/v2/document/<doctype>`.
    async fn get<T: DeserializeOwned>(
        &self,
        doctype: &str,
        query: &[(&str, &str)],
    ) -> Result<(Vec<T>, Option<bool>), Error> {
        let url = format!(
            "{}/api/v2/document/{}",
            self.credentials.site,
            urlencode(doctype)
        );

        let response = self
            .http
            .get(&url)
            .header("Authorization", self.credentials.header())
            .header("Accept", "application/json")
            .query(query)
            .send()
            .await
            .map_err(|error| Error::Unreachable(error.to_string()))?;

        let status = response.status();
        if !status.is_success() {
            let message = response.text().await.unwrap_or_default();
            return Err(Error::Rejected {
                status: status.as_u16(),
                // A refusal body can be an HTML error page; the first line of
                // it is the useful part and the rest is noise in a log.
                message: message
                    .lines()
                    .next()
                    .unwrap_or("")
                    .chars()
                    .take(200)
                    .collect(),
            });
        }

        #[derive(serde::Deserialize)]
        struct Envelope<T> {
            data: Vec<T>,
            has_next_page: Option<bool>,
        }

        let envelope: Envelope<T> = response
            .json()
            .await
            .map_err(|error| Error::Malformed(error.to_string()))?;
        Ok((envelope.data, envelope.has_next_page))
    }
}

/// Percent-encode the path segment. DocType names carry spaces.
fn urlencode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_doctype_name_with_a_space_is_encoded() {
        assert_eq!(urlencode("Custom Field"), "Custom%20Field");
        assert_eq!(urlencode("Property Setter"), "Property%20Setter");
        assert_eq!(urlencode("Item"), "Item");
    }
}
