//! ModemManager 1.x production adapter (ADR-001).
//!
//! Talks to `org.freedesktop.ModemManager1` over the system D-Bus through
//! `zbus` proxies: `Modem`, `Modem.Modem3gpp` (registration), `Modem.Voice` +
//! `Call`, `Modem.Messaging` + `Sms`, `Modem.Simple` + `Bearer` (cellular
//! data), and `Sim`. It never opens QMI, MBIM, QRTR, or AT devices; ModemManager
//! owns the modem, this crate owns the mapping onto the typed contract.
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
//! imitate it. When ModemManager exports its standard `Modem.Simple` and
//! `Bearer` objects, the adapter observes their packet-data lifecycle and
//! interface name. The data feature is advertised only when `Modem.Simple`
//! is present; all bearer mutation remains target-neutral at the daemon API.
//! When a client submits an empty `DataProfile`, the production adapter may
//! use the non-secret `FERRIX_TELEPHONY_APN` target environment fallback; an
//! invalid configured value fails closed.

pub mod mapping;

#[cfg(feature = "production")]
pub mod backend;

#[cfg(feature = "production")]
pub use backend::ModemManagerBackend;
