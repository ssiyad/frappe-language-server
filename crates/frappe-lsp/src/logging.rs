//! Tracing to stderr. Stdout carries the protocol and nothing else — spec 30.

use tracing_subscriber::EnvFilter;

/// Environment variable of spec 30. Takes a level or a full tracing filter.
pub const ENV: &str = "FRAPPE_LSP_LOG";

const DEFAULT: &str = "info";

pub fn init(log_level: Option<&str>) {
    let filter = match log_level {
        Some(level) => EnvFilter::new(level),
        None => EnvFilter::try_from_env(ENV).unwrap_or_else(|_| EnvFilter::new(DEFAULT)),
    };

    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        // The editor writes this to a log file; escape codes only obscure it.
        .with_ansi(false)
        .with_env_filter(filter)
        .init();
}
