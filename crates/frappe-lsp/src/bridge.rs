//! Driving the live bridge — [05](../../../docs/specs/05-live-bridge.md).
//!
//! The bridge is optional and everything stays correct without it, so every
//! path here ends in a *confidence*, never in an error the user has to act on.
//! A site that is absent, unreachable or refused changes how much the server
//! is willing to claim; it never changes whether the server answers.

use crate::guard::Recover;
use std::sync::{Arc, RwLock};

use frappe_bridge::{Client, credentials, sync};
use frappe_index::Index;

use crate::config::Config;

/// What a sync attempt did, for the log line and `indexStatus`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Layer 3 is in the graph.
    Synced {
        /// The site it came from.
        site: String,
        /// How many records were merged.
        records: usize,
    },
    /// The bridge is off: not enabled, or no credentials anywhere. The user
    /// has accepted that customizations are invisible, so absence checks stay
    /// on ([23](../../../docs/specs/23-diagnostics.md)).
    Disabled(&'static str),
    /// Configured and not reachable. The server knows its picture is
    /// incomplete and stops accusing valid code.
    Degraded(String),
}

/// Sync once, and leave the index in the state the outcome describes.
///
/// Never returns an error: the caller has nothing to do with one. The three
/// outcomes *are* the error handling.
pub async fn sync_once(index: &Arc<RwLock<Index>>, config: &Config) -> Outcome {
    if !config.bridge.enabled {
        return Outcome::Disabled("bridge.enabled is false");
    }

    let credentials = match resolve(config).await {
        Ok(Some(credentials)) => credentials,
        // Spec 05: logged once at info, and the bridge stays off. Not an
        // error — running without a bridge is the default posture.
        Ok(None) => {
            return Outcome::Disabled(
                "no credentials: set FRAPPE_LSP_TOKEN, or FRAPPE_API_KEY + \
                 FRAPPE_API_SECRET, or bridge.token_command",
            );
        }
        // A token command that fails is a configured bridge that cannot be
        // reached, which is `Degraded` rather than off.
        Err(why) => return Outcome::Degraded(why),
    };

    let site = credentials.site.clone();
    tracing::info!(%site, source = %credentials.source, "live bridge credentials resolved");
    let client = match Client::new(credentials) {
        Ok(client) => client,
        Err(error) => return Outcome::Degraded(error.to_string()),
    };

    let read = match sync::read(&client).await {
        Ok(read) => read,
        // A partial read is not merged: half a picture looks like absence,
        // which is the one thing Degraded exists to prevent.
        Err(error) => return Outcome::Degraded(error.to_string()),
    };

    let records = read.len();
    let custom_fields = sync::as_fixture_body(&read.custom_fields, "Custom Field");
    let property_setters = sync::as_fixture_body(&read.property_setters, "Property Setter");

    {
        let mut guard = index.write_or_recover();
        guard.apply_site(&site, &custom_fields, &property_setters);
    }

    if !read.custom_doctypes.is_empty() {
        // Not merged yet: a UI-created DocType has no fields until `DocField`
        // is read for it, and a DocType with no fields is worse than none —
        // every field on it would resolve to nothing.
        tracing::info!(
            count = read.custom_doctypes.len(),
            "custom DocTypes seen but not merged; their fields need a second read"
        );
    }

    Outcome::Synced { site, records }
}

/// The environment, else whatever `bridge.token_command` prints.
///
/// The command runs off the async worker: it is somebody's shell script, it
/// may prompt a password manager, and blocking the runtime on that would stop
/// every other request in the process.
async fn resolve(config: &Config) -> Result<Option<frappe_bridge::Credentials>, String> {
    if let Some(credentials) = credentials::from_environment(&config.bridge.site) {
        return Ok(Some(credentials));
    }

    let command = config.bridge.token_command.trim().to_string();
    if command.is_empty() {
        return Ok(None);
    }
    let site = config.bridge.site.clone();

    let output = tokio::task::spawn_blocking(move || {
        // Through a shell on purpose: the useful forms are pipelines and
        // password-manager invocations, and this is the user's own
        // configuration, never a workspace's ([30](../../../docs/specs/30-configuration.md)).
        std::process::Command::new("sh")
            .arg("-c")
            .arg(&command)
            .output()
    })
    .await
    .map_err(|error| format!("the token command panicked: {error}"))?
    .map_err(|error| format!("the token command could not run: {error}"))?;

    if !output.status.success() {
        // The token is on stdout; stderr is where the reason is, and it is the
        // only thing the user has to go on.
        let why = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "the token command exited {}: {}",
            output.status.code().unwrap_or(-1),
            why.lines()
                .next()
                .unwrap_or("")
                .chars()
                .take(200)
                .collect::<String>()
        ));
    }

    let printed = String::from_utf8_lossy(&output.stdout);
    Ok(credentials::from_command_output(&site, &printed))
}

/// Put the index into the state a failed or disabled bridge implies.
pub fn settle(index: &Arc<RwLock<Index>>, outcome: &Outcome) {
    let mut guard = index.write_or_recover();
    match outcome {
        Outcome::Synced { .. } => {}
        Outcome::Disabled(_) => {
            guard.evict_site(true);
        }
        Outcome::Degraded(_) => {
            guard.evict_site(false);
        }
    }
}

/// Report it once, at the level it deserves.
pub fn log(outcome: &Outcome) {
    match outcome {
        Outcome::Synced { site, records } => {
            tracing::info!(%site, records, "live bridge synced");
        }
        Outcome::Disabled(why) => tracing::info!(why, "live bridge off"),
        Outcome::Degraded(why) => {
            tracing::warn!(why, "live bridge unreachable; absence checks suppressed");
        }
    }
}
