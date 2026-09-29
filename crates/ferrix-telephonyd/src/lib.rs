//! `ferrix-telephonyd`: the privileged telephony service (ADR-001).
//!
//! Owns modem, SIM, registration, call, and SMS state through a
//! `ferrix_telephony_adapter_api::ModemBackend`; assigns `CallId`/`MessageId`
//! (`c<n>`/`m<n>`); assembles multipart SMS within a bounded window; persists
//! messages to a versioned atomic store; authorizes every request by the
//! peer's verified application identity and capability (ADR-004); and serves
//! `ferrix-telephony-protocol` over a Unix socket.
//!
//! This library never names a backend implementation; the binary composes
//! one. Everything here runs on the host for tests (`testing::FakeBackend`,
//! `identity::StaticIdentity`).

pub mod control;
pub mod identity;
pub mod records;
pub mod server;
pub mod service;
pub mod store;
pub mod testing;

pub use control::{HOST_TOOL_APP, LOCAL_COMMANDS, LocalCommand, LocalControl};
pub use identity::{
    AllowAll, AppdIdentity, CAP_CALL, CAP_DATA, CAP_RADIO, CAP_SMS, CAP_STATE, DenyAll, Identity,
    IdentityInfo, StaticIdentity,
};
pub use records::{EVIDENCE_MOCK, EVIDENCE_QEMU, Recorder, emit};
pub use server::{Connection, Outcome, Server, dispatch, required_capability};
pub use service::{MULTIPART_WINDOW_MS, Service, ServiceOptions, project_event, project_state};
pub use store::{DATA_DIR_ENV, DEFAULT_DATA_DIR, SmsStore, StoreDocument};
