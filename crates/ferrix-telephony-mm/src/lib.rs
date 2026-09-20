//! ModemManager 1.x production adapter (ADR-001).
//!
//! Talks to `org.freedesktop.ModemManager1` over the system D-Bus through
//! `zbus` proxies: `Modem`, `Modem.Modem3gpp` (registration), `Modem.Voice` +
//! `Call`, `Modem.Messaging` + `Sms`, and `Sim`. It never opens QMI, MBIM,
//! QRTR, or AT devices; ModemManager owns the modem, this crate owns the
//! mapping onto the typed contract.
//!
//! Layout:
//!
//! - [`mapping`]: pure functions from ModemManager enum values to
//!   `ferrix-telephony-api` types. No D-Bus, tested on every host.
//! - [`backend`] (feature `production`): the `ModemBackend` implementation,
//!   its D-Bus proxies, and the signal watcher thread.
//!
//! Audio routing is `Unsupported { feature: audio-route }` here: routing
//! belongs to the audio service once it exists, and this adapter does not
//! imitate it. Cellular data is reported as `Unsupported` too; bearer
//! management is a later slice.

pub mod mapping;

#[cfg(feature = "production")]
pub mod backend;

#[cfg(feature = "production")]
pub use backend::ModemManagerBackend;
