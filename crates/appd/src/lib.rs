//! `appd`: the application service.
//!
//! Loads the descriptor catalog, launches every application as an
//! independent supervised process through an `app_runtime_api::Runtime`, associates
//! compositor surfaces with instances by kernel pid, tracks foreground and
//! background, restarts the home surface within a budget, and serves the
//! `app_protocol` socket.
//!
//! This library depends only on `app-api` and `app-protocol`; runtime
//! implementations are composed by the `appd` binary. Everything here runs
//! on the host for tests (`FakeRuntime`, `FakeProcessTree`).

pub mod catalog;
pub mod lifecycle;
pub mod process_tree;
pub mod records;
pub mod server;
pub mod service;
pub mod testing;

pub use catalog::Catalog;
pub use lifecycle::{HomeRestartConfig, HomeRestartTracker, RestartDecision};
pub use process_tree::{ProcTree, ProcessTree};
pub use records::{EVIDENCE_MOCK, EVIDENCE_QEMU, Recorder, emit};
pub use server::Server;
pub use service::{LaunchOutcome, Service, ServiceOptions, Subscriber};
