//! Language server for Frappe Framework apps.
//!
//! The binary is a thin wrapper over this crate; the split exists so the
//! protocol layer can be exercised from tests.

pub mod analysis;
pub mod buffers;
pub mod capabilities;
pub mod cli;
pub mod completion;
pub mod config;
pub mod definition;
pub mod hover;
pub mod locations;
pub mod logging;
pub mod server;
