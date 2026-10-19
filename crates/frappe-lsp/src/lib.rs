//! Language server for Frappe Framework apps.
//!
//! The binary is a thin wrapper over this crate; the split exists so the
//! protocol layer can be exercised from tests.

pub mod agent;
pub mod analysis;
pub mod bridge;
pub mod buffers;
pub mod capabilities;
pub mod cast;
pub mod cli;
pub mod completion;
pub mod config;
pub mod definition;
pub mod diagnostics;
pub mod guard;
pub mod hooks;
pub mod hover;
pub mod locations;
pub mod logging;
pub mod scripts;
pub mod server;
pub mod symbols;
pub mod templates;
pub mod testing;
pub mod watcher;
