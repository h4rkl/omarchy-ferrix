//! Internal contract between `ferrix-telephonyd` and a modem backend
//! (ADR-001). Applications never link this crate; only the daemon binary
//! composes an implementation.
//!
//! The backend reports raw facts with its own opaque handles. The daemon owns
//! public identities (`CallId`/`MessageId`), SMS persistence, multipart
//! assembly, authorization, and fan-out. Everything is synchronous and small:
//! a backend answers a command immediately (accepted or a typed error) and
//! reports progress through [`BackendEvent`]s on the channel handed to
//! [`ModemBackend::subscribe`].
//!
//! Time: the daemon's clock is driven by [`BackendEvent::Tick`]. The mock
//! advances it only on explicit host commands; a production adapter emits it
//! from a timer thread. The daemon never reads the wall clock itself, which
//! keeps every timeout (ringing, multipart window) deterministic in tests.

use std::fmt;
use std::sync::mpsc::Sender;

use ferrix_telephony_api::{
    AccessTechnology, AudioRoute, AudioRouteState, CallEndReason, CallState, Capabilities,
    DataBearer, DataProfile, Direction, E164, ModemState, MultipartPart, Operator, RadioPower,
    Registration, Signal, SimState, SmsFailure, SmsState, TelephonyError, TelephonyFeature,
};
use serde::{Deserialize, Serialize};

/// Backend-private call handle (an object path, an index, a counter). The
/// daemon maps it to a `CallId` and never exposes it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CallHandle(pub String);

impl fmt::Display for CallHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for CallHandle {
    fn from(value: &str) -> Self {
        CallHandle(value.to_owned())
    }
}

/// Backend-private handle of an outgoing message.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SmsHandle(pub String);

impl fmt::Display for SmsHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for SmsHandle {
    fn from(value: &str) -> Self {
        SmsHandle(value.to_owned())
    }
}

/// A call as the backend sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackendCall {
    pub handle: CallHandle,
    pub direction: Direction,
    pub number: E164,
    pub state: CallState,
    pub end_reason: Option<CallEndReason>,
}

/// The backend's complete view at one instant. SMS storage is the daemon's;
/// a backend that holds received messages replays them as events after
/// `subscribe`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackendSnapshot {
    /// Backend clock in milliseconds (see [`BackendEvent::Tick`]).
    pub now_ms: u64,
    pub modem: ModemState,
    pub sim: SimState,
    pub registration: Registration,
    pub operator: Option<Operator>,
    pub access: AccessTechnology,
    pub signal: Signal,
    pub radio: RadioPower,
    pub data: DataBearer,
    pub audio: AudioRouteState,
    pub capabilities: Capabilities,
    pub calls: Vec<BackendCall>,
}

impl Default for BackendSnapshot {
    fn default() -> Self {
        Self {
            now_ms: 0,
            modem: ModemState::Absent,
            sim: SimState::Absent,
            registration: Registration::Unknown,
            operator: None,
            access: AccessTechnology::None,
            signal: Signal::default(),
            radio: RadioPower::On,
            data: DataBearer::Unsupported,
            audio: AudioRouteState::default(),
            capabilities: Capabilities::new(),
            calls: Vec::new(),
        }
    }
}

/// Facts reported by a backend, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BackendEvent {
    /// The backend clock advanced. The daemon runs its own timers on this.
    Tick {
        now_ms: u64,
    },
    Modem(ModemState),
    Sim(SimState),
    Registration {
        registration: Registration,
        operator: Option<Operator>,
        access: AccessTechnology,
    },
    Signal(Signal),
    Radio(RadioPower),
    Data(DataBearer),
    Audio(AudioRouteState),
    Capabilities(Capabilities),
    /// A new call exists: `ringing` for incoming, `dialing` for outgoing.
    CallAdded {
        handle: CallHandle,
        direction: Direction,
        number: E164,
        state: CallState,
    },
    CallChanged {
        handle: CallHandle,
        state: CallState,
        end_reason: Option<CallEndReason>,
    },
    /// A complete single-part message arrived.
    SmsReceived {
        from: E164,
        body: String,
    },
    /// One segment of a concatenated message arrived; the daemon assembles.
    SmsPartReceived {
        from: E164,
        part: MultipartPart,
    },
    /// An outgoing message progressed (`sent`, `delivered`, `failed`).
    SmsChanged {
        handle: SmsHandle,
        state: SmsState,
        failure: Option<SmsFailure>,
    },
    /// The backend disappeared (daemon crash, bus loss). The daemon marks the
    /// modem `failed` then `recovering` and ends live calls with `modem-lost`.
    BackendLost {
        reason: String,
    },
    /// The backend is back; the daemon re-reads the snapshot and publishes
    /// `service-restarted`.
    BackendRestarted,
    /// Daemon-internal synchronization marker sent by the daemon to its own
    /// event loop so it can wait for everything queued before it. Adapters
    /// never emit it; the daemon ignores tokens it did not issue.
    Barrier {
        token: u64,
    },
}

impl BackendEvent {
    pub fn name(&self) -> &'static str {
        match self {
            BackendEvent::Tick { .. } => "tick",
            BackendEvent::Modem(_) => "modem",
            BackendEvent::Sim(_) => "sim",
            BackendEvent::Registration { .. } => "registration",
            BackendEvent::Signal(_) => "signal",
            BackendEvent::Radio(_) => "radio",
            BackendEvent::Data(_) => "data",
            BackendEvent::Audio(_) => "audio",
            BackendEvent::Capabilities(_) => "capabilities",
            BackendEvent::CallAdded { .. } => "call-added",
            BackendEvent::CallChanged { .. } => "call-changed",
            BackendEvent::SmsReceived { .. } => "sms-received",
            BackendEvent::SmsPartReceived { .. } => "sms-part-received",
            BackendEvent::SmsChanged { .. } => "sms-changed",
            BackendEvent::BackendLost { .. } => "backend-lost",
            BackendEvent::BackendRestarted => "backend-restarted",
            BackendEvent::Barrier { .. } => "barrier",
        }
    }
}

/// Typed backend failures; each maps onto one [`TelephonyError`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BackendError {
    Unsupported(TelephonyFeature),
    NoModem,
    NoSim,
    SimLocked,
    NoService,
    InvalidNumber(String),
    Busy,
    NotFound(String),
    InvalidState { from: String, op: String },
    Backend(String),
}

impl BackendError {
    pub fn backend(reason: impl fmt::Display) -> Self {
        BackendError::Backend(reason.to_string())
    }

    pub fn invalid_state(from: impl fmt::Display, op: &str) -> Self {
        BackendError::InvalidState {
            from: from.to_string(),
            op: op.to_owned(),
        }
    }
}

impl From<BackendError> for TelephonyError {
    fn from(error: BackendError) -> Self {
        match error {
            BackendError::Unsupported(feature) => TelephonyError::Unsupported { feature },
            BackendError::NoModem => TelephonyError::NoModem,
            BackendError::NoSim => TelephonyError::NoSim,
            BackendError::SimLocked => TelephonyError::SimLocked,
            BackendError::NoService => TelephonyError::NoService,
            BackendError::InvalidNumber(reason) => TelephonyError::InvalidNumber { reason },
            BackendError::Busy => TelephonyError::Busy,
            BackendError::NotFound(id) => TelephonyError::NotFound { id },
            BackendError::InvalidState { from, op } => TelephonyError::InvalidState { from, op },
            BackendError::Backend(reason) => TelephonyError::Backend { reason },
        }
    }
}

impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&TelephonyError::from(self.clone()), f)
    }
}

impl std::error::Error for BackendError {}

/// A modem backend. Every operation is synchronous: it is accepted (and
/// progress arrives as events) or fails with a typed error. A backend that
/// lacks a feature returns [`BackendError::Unsupported`]; nothing imitates
/// success.
pub trait ModemBackend: Send + Sync {
    /// Short backend name for records (`mock`, `modemmanager`).
    fn name(&self) -> &'static str;
    fn capabilities(&self) -> Capabilities;
    fn snapshot(&self) -> BackendSnapshot;
    /// Registers an event sink. Every later event is sent to every sink.
    fn subscribe(&self, sink: Sender<BackendEvent>);
    fn dial(&self, number: &E164) -> Result<CallHandle, BackendError>;
    fn answer(&self, call: &CallHandle) -> Result<(), BackendError>;
    /// Declines a ringing incoming call.
    fn reject(&self, call: &CallHandle) -> Result<(), BackendError>;
    fn hangup(&self, call: &CallHandle) -> Result<(), BackendError>;
    fn hold(&self, call: &CallHandle) -> Result<(), BackendError>;
    fn resume(&self, call: &CallHandle) -> Result<(), BackendError>;
    fn dtmf(
        &self,
        call: &CallHandle,
        digit: ferrix_telephony_api::Dtmf,
    ) -> Result<(), BackendError>;
    fn send_sms(&self, to: &E164, body: &str) -> Result<SmsHandle, BackendError>;
    /// Starts a packet-data bearer. Progress and the resulting interface are
    /// reported through [`BackendEvent::Data`].
    fn connect_data(&self, profile: &DataProfile) -> Result<(), BackendError>;
    /// Tears down all packet-data bearers owned by this modem. The operation
    /// is idempotent when no bearer exists.
    fn disconnect_data(&self) -> Result<(), BackendError>;
    fn set_radio_power(&self, power: RadioPower) -> Result<(), BackendError>;
    fn enter_pin(&self, pin: &str) -> Result<(), BackendError>;
    fn set_audio_route(&self, route: AudioRoute) -> Result<(), BackendError>;
    /// Mutes or unmutes the live call's microphone uplink. The change is
    /// reported through [`BackendEvent::Audio`]; a backend that cannot mute
    /// returns `Unsupported(MicMute)`.
    fn set_mic_muted(&self, muted: bool) -> Result<(), BackendError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_errors_map_onto_typed_telephony_errors() {
        assert_eq!(
            TelephonyError::from(BackendError::Unsupported(TelephonyFeature::Hold)),
            TelephonyError::Unsupported {
                feature: TelephonyFeature::Hold
            }
        );
        assert_eq!(
            TelephonyError::from(BackendError::NotFound("c9".into())),
            TelephonyError::NotFound { id: "c9".into() }
        );
        assert_eq!(
            TelephonyError::from(BackendError::invalid_state(CallState::Ended, "answer")),
            TelephonyError::InvalidState {
                from: "ended".into(),
                op: "answer".into()
            }
        );
        assert_eq!(BackendError::Busy.to_string(), "busy");
    }

    #[test]
    fn events_have_names_and_default_snapshot_is_absent() {
        assert_eq!(BackendEvent::Tick { now_ms: 1 }.name(), "tick");
        assert_eq!(BackendEvent::BackendRestarted.name(), "backend-restarted");
        let snapshot = BackendSnapshot::default();
        assert_eq!(snapshot.modem, ModemState::Absent);
        assert_eq!(snapshot.data, DataBearer::Unsupported);
    }
}
