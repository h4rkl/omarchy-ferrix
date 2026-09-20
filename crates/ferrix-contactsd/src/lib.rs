//! `ferrix-contactsd`: the contacts service (ADR-003, ADR-004).
//!
//! Owns the versioned, atomically written contact store, its migrations,
//! vCard import/export, per-request authorization by verified peer identity,
//! and the `ferrix-contacts-protocol` socket. The library is transport- and
//! identity-source-agnostic so every rule runs in host tests; `main.rs`
//! composes the socket server and the `ferrix-appd` identity source.

pub mod auth;
pub mod migrate;
pub mod records;
pub mod server;
pub mod service;
pub mod store;
pub mod vcard;

pub use auth::{AllowAll, AppdIdentity, DenyAll, Identified, Identity, StaticIdentity};
pub use records::{EVIDENCE_MOCK, EVIDENCE_QEMU, Recorder};
pub use server::Server;
pub use service::{Connection, Outcome, Service};
pub use store::Store;
