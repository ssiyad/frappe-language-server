//! Language server for Frappe Framework apps.
//!
//! The binary is a thin wrapper over this crate; the split exists so the
//! protocol layer can be exercised from tests.

pub mod capabilities;
pub mod cli;
pub mod config;
pub mod document;
pub mod logging;
pub mod server;
