//! Live site client — [05](../../../docs/specs/05-live-bridge.md).
//!
//! Optional Layer 3 for [04](../../../docs/specs/04-metadata-index.md). Off by
//! default, and everything the server does stays correct without it: a bridge
//! that is absent, unreachable or refused degrades the answer's *confidence*
//! and never its correctness.
//!
//! Two rules shape this crate. It issues `GET` and nothing else, because it
//! reads a database it does not own. And it takes credentials only from the
//! environment or a credential store, never from a config file, because an LSP
//! config file is commonly committed
//! ([30](../../../docs/specs/30-configuration.md)).

#![warn(missing_docs)]

pub mod client;
pub mod credentials;
pub mod sync;

pub use client::{Client, Error};
pub use credentials::{Credentials, Source};
pub use sync::Sync;
