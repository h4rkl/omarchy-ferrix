//! Stable, target-neutral telephony contract (ADR-001, schema 1).
//!
//! Pure types plus serde: modem, SIM, registration, calls, SMS, cellular
//! data bearer, audio route, radio power, capabilities, the aggregate
//! [`TelephonyState`] snapshot, [`TelephonyEvent`] deltas that fold into a
//! snapshot, and the typed [`TelephonyError`]. No I/O lives here and nothing
//! here names a modem vendor, a bus, a kernel, or an emulator: the service
//! (`ferrix-telephonyd`) owns identities and persistence, the protocol crate
//! owns the wire, and backend adapters are composed only by the daemon
//! binary.
//!
//! Party numbers are typed ([`E164`]) so a client that holds only the
//! `telephony-state` capability can be served a [`E164::redacted`] form and
//! evidence records never carry a number at all.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Current schema version of every serialized type in this crate and of the
/// `ferrix-telephonyd` wire protocol built on it.
pub const SCHEMA_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// Modem, SIM, registration, signal
// ---------------------------------------------------------------------------

/// Lifecycle of the modem itself.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum ModemState {
    /// No modem device exists on this target.
    #[default]
    Absent,
    /// A modem exists but is powered down or administratively disabled.
    Disabled,
    Initializing,
    Ready,
    Failed {
        reason: String,
    },
    /// The service is re-establishing the backend after a loss.
    Recovering,
}

impl ModemState {
    pub fn name(&self) -> &'static str {
        match self {
            ModemState::Absent => "absent",
            ModemState::Disabled => "disabled",
            ModemState::Initializing => "initializing",
            ModemState::Ready => "ready",
            ModemState::Failed { .. } => "failed",
            ModemState::Recovering => "recovering",
        }
    }
}

/// Which lock a SIM demands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SimLock {
    Pin,
    Puk,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum SimState {
    #[default]
    Absent,
    Locked {
        lock: SimLock,
        attempts_left: u8,
    },
    /// Permanently blocked (no attempts left).
    Blocked,
    Ready,
    Failed {
        reason: String,
    },
}

impl SimState {
    pub fn name(&self) -> &'static str {
        match self {
            SimState::Absent => "absent",
            SimState::Locked { .. } => "locked",
            SimState::Blocked => "blocked",
            SimState::Ready => "ready",
            SimState::Failed { .. } => "failed",
        }
    }
}

/// Network registration status.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum Registration {
    #[default]
    Unknown,
    Searching,
    Denied,
    Registered {
        roaming: bool,
    },
    EmergencyOnly,
}

impl Registration {
    pub fn name(self) -> &'static str {
        match self {
            Registration::Unknown => "unknown",
            Registration::Searching => "searching",
            Registration::Denied => "denied",
            Registration::Registered { .. } => "registered",
            Registration::EmergencyOnly => "emergency-only",
        }
    }

    /// Whether ordinary calls and messages can be attempted.
    pub fn is_registered(self) -> bool {
        matches!(self, Registration::Registered { .. })
    }
}

/// The serving operator. The mock uses synthetic values only.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Operator {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcc_mnc: Option<String>,
}

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum AccessTechnology {
    #[default]
    None,
    Gsm,
    Umts,
    Lte,
    Nr,
}

impl AccessTechnology {
    pub fn name(self) -> &'static str {
        match self {
            AccessTechnology::None => "none",
            AccessTechnology::Gsm => "gsm",
            AccessTechnology::Umts => "umts",
            AccessTechnology::Lte => "lte",
            AccessTechnology::Nr => "nr",
        }
    }
}

/// Signal strength summary. `bars` is clamped to `0..=4` on construction.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signal {
    pub bars: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rssi_dbm: Option<i16>,
}

impl Signal {
    pub const MAX_BARS: u8 = 4;

    pub fn new(bars: u8, rssi_dbm: Option<i16>) -> Self {
        Self {
            bars: bars.min(Self::MAX_BARS),
            rssi_dbm,
        }
    }

    pub fn bars(bars: u8) -> Self {
        Self::new(bars, None)
    }
}

// ---------------------------------------------------------------------------
// Numbers
// ---------------------------------------------------------------------------

/// Well-known emergency short codes. Dialing any of them is rejected with
/// `Unsupported { feature: emergency }` because no emergency path exists.
pub const EMERGENCY_NUMBERS: &[&str] = &["112", "911", "999", "000", "110", "119"];

/// Whether `digits` (without any `+`) is an emergency short code.
pub fn is_emergency_number(digits: &str) -> bool {
    EMERGENCY_NUMBERS.contains(&digits)
}

/// A validated international number: `+` followed by 8 to 15 digits.
///
/// Serialized as a plain string. Only sent on the protocol to clients whose
/// capability allows full numbers; [`E164::redacted`] is the form for
/// `telephony-state` clients. Never placed in evidence records.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct E164(String);

impl E164 {
    pub const MIN_DIGITS: usize = 8;
    pub const MAX_DIGITS: usize = 15;

    /// Parses and validates. Emergency short codes are *not* valid E.164
    /// numbers here; see [`E164::classify_dial_string`].
    pub fn new(text: impl Into<String>) -> Result<Self, TelephonyError> {
        let text = text.into();
        let Some(digits) = text.strip_prefix('+') else {
            return Err(TelephonyError::InvalidNumber {
                reason: "must start with '+'".to_owned(),
            });
        };
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return Err(TelephonyError::InvalidNumber {
                reason: "only digits may follow '+'".to_owned(),
            });
        }
        if digits.len() < Self::MIN_DIGITS || digits.len() > Self::MAX_DIGITS {
            return Err(TelephonyError::InvalidNumber {
                reason: format!(
                    "expected {}..={} digits, got {}",
                    Self::MIN_DIGITS,
                    Self::MAX_DIGITS,
                    digits.len()
                ),
            });
        }
        Ok(Self(text))
    }

    /// Classifies a raw dial string: an emergency short code (with or
    /// without `+`) is a typed `Unsupported { feature: emergency }`; anything
    /// else must be a valid [`E164`].
    pub fn classify_dial_string(text: &str) -> Result<Self, TelephonyError> {
        let digits = text.strip_prefix('+').unwrap_or(text);
        if is_emergency_number(digits) {
            return Err(TelephonyError::Unsupported {
                feature: TelephonyFeature::Emergency,
            });
        }
        Self::new(text)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The digits after `+`.
    pub fn digits(&self) -> &str {
        &self.0[1..]
    }

    /// `+***0042`: only the last four digits survive.
    pub fn redacted(&self) -> String {
        let digits = self.digits();
        let keep = digits.len().saturating_sub(4);
        format!("+***{}", &digits[keep..])
    }

    /// Whether the digits equal a well-known emergency short code. Always
    /// false for a value constructed through [`E164::new`] (short codes are
    /// too short), kept for callers that hold pre-validated numbers.
    pub fn is_emergency(&self) -> bool {
        is_emergency_number(self.digits())
    }
}

impl fmt::Debug for E164 {
    /// Debug output is redacted so accidental `{:?}` logging never leaks.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "E164({})", self.redacted())
    }
}

impl fmt::Display for E164 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for E164 {
    type Err = TelephonyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl TryFrom<String> for E164 {
    type Error = TelephonyError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<E164> for String {
    fn from(value: E164) -> Self {
        value.0
    }
}

/// The remote party of a call or message.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartyRef {
    pub number: E164,
    /// Display text resolved by a contacts service or the client; optional.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<String>,
}

impl PartyRef {
    pub fn new(number: E164) -> Self {
        Self {
            number,
            display: None,
        }
    }
}

/// A party as seen by a `telephony-state` client: no number, only the
/// redacted form and the display text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedactedParty {
    pub party_redacted: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<String>,
}

impl From<&PartyRef> for RedactedParty {
    fn from(party: &PartyRef) -> Self {
        Self {
            party_redacted: party.number.redacted(),
            display: party.display.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// Calls
// ---------------------------------------------------------------------------

/// Service-assigned call identity (`c<n>`, per daemon run).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CallId(pub String);

impl CallId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CallId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for CallId {
    fn from(value: &str) -> Self {
        CallId(value.to_owned())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Direction {
    Incoming,
    Outgoing,
}

impl Direction {
    pub fn name(self) -> &'static str {
        match self {
            Direction::Incoming => "incoming",
            Direction::Outgoing => "outgoing",
        }
    }
}

/// ```text
/// outgoing: dialing -> active <-> held -> ended
/// incoming: ringing -> active <-> held -> ended
/// any live state -> ended
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CallState {
    Dialing,
    Ringing,
    Active,
    Held,
    Ended,
}

impl CallState {
    pub fn name(self) -> &'static str {
        match self {
            CallState::Dialing => "dialing",
            CallState::Ringing => "ringing",
            CallState::Active => "active",
            CallState::Held => "held",
            CallState::Ended => "ended",
        }
    }

    pub fn is_live(self) -> bool {
        self != CallState::Ended
    }

    /// Whether `self -> next` is a legal transition.
    #[allow(clippy::match_like_matches_macro)] // one arm per edge reads as the state machine
    pub fn can_transition_to(self, next: CallState) -> bool {
        use CallState::*;
        match (self, next) {
            (Dialing | Ringing, Active) => true,
            (Active, Held) | (Held, Active) => true,
            (Dialing | Ringing | Active | Held, Ended) => true,
            _ => false,
        }
    }
}

impl fmt::Display for CallState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum CallEndReason {
    LocalHangup,
    RemoteHangup,
    Busy,
    Rejected,
    NoAnswer,
    TimedOut,
    NoService,
    ModemLost,
    Failed { reason: String },
}

impl CallEndReason {
    pub fn name(&self) -> &'static str {
        match self {
            CallEndReason::LocalHangup => "local-hangup",
            CallEndReason::RemoteHangup => "remote-hangup",
            CallEndReason::Busy => "busy",
            CallEndReason::Rejected => "rejected",
            CallEndReason::NoAnswer => "no-answer",
            CallEndReason::TimedOut => "timed-out",
            CallEndReason::NoService => "no-service",
            CallEndReason::ModemLost => "modem-lost",
            CallEndReason::Failed { .. } => "failed",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Call {
    pub id: CallId,
    pub direction: Direction,
    pub state: CallState,
    pub party: PartyRef,
    /// Service clock (ms) when the call was first seen; not wall-clock in
    /// the mock.
    pub started_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_reason: Option<CallEndReason>,
}

impl Call {
    /// The same call with the party redacted for `telephony-state` clients.
    pub fn redacted(&self) -> RedactedCall {
        RedactedCall {
            id: self.id.clone(),
            direction: self.direction,
            state: self.state,
            party: RedactedParty::from(&self.party),
            started_at_ms: self.started_at_ms,
            end_reason: self.end_reason.clone(),
        }
    }
}

/// [`Call`] as served to clients without `telephony-call`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedactedCall {
    pub id: CallId,
    pub direction: Direction,
    pub state: CallState,
    pub party: RedactedParty,
    pub started_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_reason: Option<CallEndReason>,
}

/// A DTMF digit: `0-9`, `*`, `#`, `A-D`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "char", into = "char")]
pub struct Dtmf(char);

impl Dtmf {
    pub fn new(c: char) -> Result<Self, TelephonyError> {
        let c = c.to_ascii_uppercase();
        if c.is_ascii_digit() || matches!(c, '*' | '#' | 'A'..='D') {
            Ok(Self(c))
        } else {
            Err(TelephonyError::Protocol {
                reason: format!("invalid dtmf digit {c:?}"),
            })
        }
    }

    pub fn as_char(self) -> char {
        self.0
    }
}

impl fmt::Debug for Dtmf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Dtmf({:?})", self.0)
    }
}

impl TryFrom<char> for Dtmf {
    type Error = TelephonyError;

    fn try_from(value: char) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<Dtmf> for char {
    fn from(value: Dtmf) -> Self {
        value.0
    }
}

// ---------------------------------------------------------------------------
// SMS
// ---------------------------------------------------------------------------

/// Service-assigned message identity (`m<n>`), stable across restarts once
/// persisted.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MessageId(pub String);

impl MessageId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for MessageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for MessageId {
    fn from(value: &str) -> Self {
        MessageId(value.to_owned())
    }
}

/// ```text
/// outgoing: sending -> sent -> delivered ; sending -> failed
/// incoming: received -> read
/// any -> deleted
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SmsState {
    Sending,
    Sent,
    Delivered,
    Failed,
    Received,
    Read,
    Deleted,
}

impl SmsState {
    pub fn name(self) -> &'static str {
        match self {
            SmsState::Sending => "sending",
            SmsState::Sent => "sent",
            SmsState::Delivered => "delivered",
            SmsState::Failed => "failed",
            SmsState::Received => "received",
            SmsState::Read => "read",
            SmsState::Deleted => "deleted",
        }
    }

    #[allow(clippy::match_like_matches_macro)]
    pub fn can_transition_to(self, next: SmsState) -> bool {
        use SmsState::*;
        match (self, next) {
            (Sending, Sent | Failed) => true,
            (Sent, Delivered | Failed) => true,
            (Received, Read) => true,
            (Sending | Sent | Delivered | Failed | Received | Read, Deleted) => true,
            _ => false,
        }
    }
}

impl fmt::Display for SmsState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum SmsFailure {
    NoService,
    Rejected,
    TimedOut,
    ModemLost,
    InvalidDestination,
    Unsupported,
    Failed { reason: String },
}

impl SmsFailure {
    pub fn name(&self) -> &'static str {
        match self {
            SmsFailure::NoService => "no-service",
            SmsFailure::Rejected => "rejected",
            SmsFailure::TimedOut => "timed-out",
            SmsFailure::ModemLost => "modem-lost",
            SmsFailure::InvalidDestination => "invalid-destination",
            SmsFailure::Unsupported => "unsupported",
            SmsFailure::Failed { .. } => "failed",
        }
    }

    /// Reason used when a concatenated message never completed.
    pub fn incomplete_multipart() -> Self {
        SmsFailure::Failed {
            reason: "incomplete-multipart".to_owned(),
        }
    }
}

/// One segment of a concatenated SMS as reported by an adapter. `index` is
/// 1-based, `count` is the total number of segments.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MultipartPart {
    pub ref_id: u16,
    pub index: u8,
    pub count: u8,
    pub text: String,
}

/// Reference to a received segment kept on the assembled message (text is
/// folded into `body`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartRef {
    pub ref_id: u16,
    pub index: u8,
    pub count: u8,
}

impl From<&MultipartPart> for PartRef {
    fn from(part: &MultipartPart) -> Self {
        Self {
            ref_id: part.ref_id,
            index: part.index,
            count: part.count,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sms {
    pub id: MessageId,
    pub direction: Direction,
    pub party: PartyRef,
    pub body: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<PartRef>,
    pub state: SmsState,
    pub timestamp_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<SmsFailure>,
}

impl Sms {
    /// Redacted for `telephony-state` clients: no number, no body.
    pub fn redacted(&self) -> RedactedSms {
        RedactedSms {
            id: self.id.clone(),
            direction: self.direction,
            party: RedactedParty::from(&self.party),
            parts: self.parts.len(),
            state: self.state,
            timestamp_ms: self.timestamp_ms,
            failure: self.failure.clone(),
        }
    }
}

/// [`Sms`] as served to clients without `telephony-sms`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedactedSms {
    pub id: MessageId,
    pub direction: Direction,
    pub party: RedactedParty,
    /// Number of concatenated segments (0 for a single-part message).
    pub parts: usize,
    pub state: SmsState,
    pub timestamp_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<SmsFailure>,
}

// ---------------------------------------------------------------------------
// Data, audio, radio, capabilities
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum DataBearer {
    #[default]
    Unsupported,
    Disconnected,
    Connecting,
    Connected {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        interface: Option<String>,
    },
    Failed {
        reason: String,
    },
}

impl DataBearer {
    pub fn name(&self) -> &'static str {
        match self {
            DataBearer::Unsupported => "unsupported",
            DataBearer::Disconnected => "disconnected",
            DataBearer::Connecting => "connecting",
            DataBearer::Connected { .. } => "connected",
            DataBearer::Failed { .. } => "failed",
        }
    }
}

/// Non-secret parameters for a cellular data connection.
///
/// An empty profile asks the backend to use its normal carrier/profile
/// selection when that backend supports it.  Some modem plugins (including
/// the target's Samsung-SIT plugin) require an explicit APN and will return a
/// typed backend error for an empty profile.  The APN is intentionally the
/// only parameter in this first contract: credentials and other modem-specific
/// properties must not be carried through logs or the general telephony state
/// API.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataProfile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub apn: Option<String>,
}

impl DataProfile {
    /// Validates the user-supplied APN before it reaches a backend.
    pub fn validate(&self) -> Result<(), &'static str> {
        let Some(apn) = self.apn.as_deref() else {
            return Ok(());
        };
        if apn.is_empty() {
            return Err("apn must not be empty");
        }
        if apn.len() > 100 {
            return Err("apn is too long");
        }
        if apn
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
        {
            return Err("apn must not contain whitespace or control characters");
        }
        Ok(())
    }
}

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum AudioRoute {
    #[default]
    Earpiece,
    Speaker,
    Wired,
    Bluetooth,
}

impl AudioRoute {
    pub fn name(self) -> &'static str {
        match self {
            AudioRoute::Earpiece => "earpiece",
            AudioRoute::Speaker => "speaker",
            AudioRoute::Wired => "wired",
            AudioRoute::Bluetooth => "bluetooth",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioRouteState {
    pub active: AudioRoute,
    #[serde(default)]
    pub available: BTreeSet<AudioRoute>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RadioPower {
    #[default]
    On,
    /// Airplane mode / radios disabled.
    Off,
}

impl RadioPower {
    pub fn name(self) -> &'static str {
        match self {
            RadioPower::On => "on",
            RadioPower::Off => "off",
        }
    }
}

/// Features a backend may support. `Emergency` exists only so the typed
/// rejection of emergency numbers can name it; no backend advertises it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TelephonyFeature {
    Voice,
    Sms,
    MultipartSms,
    DeliveryReports,
    Dtmf,
    Hold,
    Data,
    AudioRoute,
    SimPin,
    RadioPower,
    Emergency,
}

impl TelephonyFeature {
    pub fn name(self) -> &'static str {
        match self {
            TelephonyFeature::Voice => "voice",
            TelephonyFeature::Sms => "sms",
            TelephonyFeature::MultipartSms => "multipart-sms",
            TelephonyFeature::DeliveryReports => "delivery-reports",
            TelephonyFeature::Dtmf => "dtmf",
            TelephonyFeature::Hold => "hold",
            TelephonyFeature::Data => "data",
            TelephonyFeature::AudioRoute => "audio-route",
            TelephonyFeature::SimPin => "sim-pin",
            TelephonyFeature::RadioPower => "radio-power",
            TelephonyFeature::Emergency => "emergency",
        }
    }

    /// Parses the kebab-case name.
    pub fn parse(name: &str) -> Option<Self> {
        [
            Self::Voice,
            Self::Sms,
            Self::MultipartSms,
            Self::DeliveryReports,
            Self::Dtmf,
            Self::Hold,
            Self::Data,
            Self::AudioRoute,
            Self::SimPin,
            Self::RadioPower,
            Self::Emergency,
        ]
        .into_iter()
        .find(|f| f.name() == name)
    }
}

impl fmt::Display for TelephonyFeature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

pub type Capabilities = BTreeSet<TelephonyFeature>;

// ---------------------------------------------------------------------------
// Snapshot and events
// ---------------------------------------------------------------------------

/// The aggregate snapshot served at hello, on `state`, and on
/// `service-restarted`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TelephonyState {
    pub schema_version: u32,
    pub modem: ModemState,
    pub sim: SimState,
    pub registration: Registration,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operator: Option<Operator>,
    pub access: AccessTechnology,
    pub signal: Signal,
    pub radio: RadioPower,
    pub data: DataBearer,
    pub audio: AudioRouteState,
    pub capabilities: Capabilities,
    /// Live and recently ended calls keyed by id.
    #[serde(default)]
    pub calls: BTreeMap<CallId, Call>,
    /// Messages keyed by id (deleted messages are removed).
    #[serde(default)]
    pub sms: BTreeMap<MessageId, Sms>,
}

impl TelephonyState {
    pub fn new() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            ..Self::default()
        }
    }

    /// Live (not ended) calls in id order.
    pub fn live_calls(&self) -> impl Iterator<Item = &Call> {
        self.calls.values().filter(|c| c.state.is_live())
    }

    /// The snapshot as served to a `telephony-state` client: numbers and
    /// bodies removed.
    pub fn redacted(&self) -> RedactedTelephonyState {
        RedactedTelephonyState {
            schema_version: self.schema_version,
            modem: self.modem.clone(),
            sim: self.sim.clone(),
            registration: self.registration,
            operator: self.operator.clone(),
            access: self.access,
            signal: self.signal,
            radio: self.radio,
            data: self.data.clone(),
            audio: self.audio.clone(),
            capabilities: self.capabilities.clone(),
            calls: self
                .calls
                .iter()
                .map(|(k, v)| (k.clone(), v.redacted()))
                .collect(),
            sms: self
                .sms
                .iter()
                .map(|(k, v)| (k.clone(), v.redacted()))
                .collect(),
        }
    }
}

/// [`TelephonyState`] with every party and body redacted.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedactedTelephonyState {
    pub schema_version: u32,
    pub modem: ModemState,
    pub sim: SimState,
    pub registration: Registration,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operator: Option<Operator>,
    pub access: AccessTechnology,
    pub signal: Signal,
    pub radio: RadioPower,
    pub data: DataBearer,
    pub audio: AudioRouteState,
    pub capabilities: Capabilities,
    #[serde(default)]
    pub calls: BTreeMap<CallId, RedactedCall>,
    #[serde(default)]
    pub sms: BTreeMap<MessageId, RedactedSms>,
}

/// Deltas published by the service. Each folds into a [`TelephonyState`]
/// through [`TelephonyEvent::apply_to`]; `service-restarted` replaces it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "kebab-case")]
pub enum TelephonyEvent {
    ModemChanged {
        modem: ModemState,
    },
    SimChanged {
        sim: SimState,
    },
    RegistrationChanged {
        registration: Registration,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        operator: Option<Operator>,
        access: AccessTechnology,
    },
    SignalChanged {
        signal: Signal,
    },
    CallChanged {
        call: Call,
    },
    SmsChanged {
        sms: Sms,
    },
    DataChanged {
        data: DataBearer,
    },
    AudioRouteChanged {
        audio: AudioRouteState,
    },
    RadioPowerChanged {
        radio: RadioPower,
    },
    CapabilitiesChanged {
        capabilities: Capabilities,
    },
    /// The service (re)started: replace local state with `state`.
    ServiceRestarted {
        state: TelephonyState,
    },
}

impl TelephonyEvent {
    pub fn name(&self) -> &'static str {
        match self {
            TelephonyEvent::ModemChanged { .. } => "modem-changed",
            TelephonyEvent::SimChanged { .. } => "sim-changed",
            TelephonyEvent::RegistrationChanged { .. } => "registration-changed",
            TelephonyEvent::SignalChanged { .. } => "signal-changed",
            TelephonyEvent::CallChanged { .. } => "call-changed",
            TelephonyEvent::SmsChanged { .. } => "sms-changed",
            TelephonyEvent::DataChanged { .. } => "data-changed",
            TelephonyEvent::AudioRouteChanged { .. } => "audio-route-changed",
            TelephonyEvent::RadioPowerChanged { .. } => "radio-power-changed",
            TelephonyEvent::CapabilitiesChanged { .. } => "capabilities-changed",
            TelephonyEvent::ServiceRestarted { .. } => "service-restarted",
        }
    }

    /// Folds the delta into `state`. A deleted SMS is removed from the map.
    pub fn apply_to(&self, state: &mut TelephonyState) {
        match self {
            TelephonyEvent::ModemChanged { modem } => state.modem = modem.clone(),
            TelephonyEvent::SimChanged { sim } => state.sim = sim.clone(),
            TelephonyEvent::RegistrationChanged {
                registration,
                operator,
                access,
            } => {
                state.registration = *registration;
                state.operator = operator.clone();
                state.access = *access;
            }
            TelephonyEvent::SignalChanged { signal } => state.signal = *signal,
            TelephonyEvent::CallChanged { call } => {
                state.calls.insert(call.id.clone(), call.clone());
            }
            TelephonyEvent::SmsChanged { sms } => {
                if sms.state == SmsState::Deleted {
                    state.sms.remove(&sms.id);
                } else {
                    state.sms.insert(sms.id.clone(), sms.clone());
                }
            }
            TelephonyEvent::DataChanged { data } => state.data = data.clone(),
            TelephonyEvent::AudioRouteChanged { audio } => state.audio = audio.clone(),
            TelephonyEvent::RadioPowerChanged { radio } => state.radio = *radio,
            TelephonyEvent::CapabilitiesChanged { capabilities } => {
                state.capabilities = capabilities.clone();
            }
            TelephonyEvent::ServiceRestarted { state: full } => *state = full.clone(),
        }
    }

    /// The same event with parties and bodies redacted, for
    /// `telephony-state` subscribers.
    pub fn redacted(&self) -> RedactedTelephonyEvent {
        match self {
            TelephonyEvent::ModemChanged { modem } => RedactedTelephonyEvent::ModemChanged {
                modem: modem.clone(),
            },
            TelephonyEvent::SimChanged { sim } => {
                RedactedTelephonyEvent::SimChanged { sim: sim.clone() }
            }
            TelephonyEvent::RegistrationChanged {
                registration,
                operator,
                access,
            } => RedactedTelephonyEvent::RegistrationChanged {
                registration: *registration,
                operator: operator.clone(),
                access: *access,
            },
            TelephonyEvent::SignalChanged { signal } => {
                RedactedTelephonyEvent::SignalChanged { signal: *signal }
            }
            TelephonyEvent::CallChanged { call } => RedactedTelephonyEvent::CallChanged {
                call: call.redacted(),
            },
            TelephonyEvent::SmsChanged { sms } => RedactedTelephonyEvent::SmsChanged {
                sms: sms.redacted(),
            },
            TelephonyEvent::DataChanged { data } => {
                RedactedTelephonyEvent::DataChanged { data: data.clone() }
            }
            TelephonyEvent::AudioRouteChanged { audio } => {
                RedactedTelephonyEvent::AudioRouteChanged {
                    audio: audio.clone(),
                }
            }
            TelephonyEvent::RadioPowerChanged { radio } => {
                RedactedTelephonyEvent::RadioPowerChanged { radio: *radio }
            }
            TelephonyEvent::CapabilitiesChanged { capabilities } => {
                RedactedTelephonyEvent::CapabilitiesChanged {
                    capabilities: capabilities.clone(),
                }
            }
            TelephonyEvent::ServiceRestarted { state } => {
                RedactedTelephonyEvent::ServiceRestarted {
                    state: state.redacted(),
                }
            }
        }
    }
}

/// [`TelephonyEvent`] as pushed to `telephony-state` subscribers. Same tags,
/// redacted payloads.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "kebab-case")]
pub enum RedactedTelephonyEvent {
    ModemChanged {
        modem: ModemState,
    },
    SimChanged {
        sim: SimState,
    },
    RegistrationChanged {
        registration: Registration,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        operator: Option<Operator>,
        access: AccessTechnology,
    },
    SignalChanged {
        signal: Signal,
    },
    CallChanged {
        call: RedactedCall,
    },
    SmsChanged {
        sms: RedactedSms,
    },
    DataChanged {
        data: DataBearer,
    },
    AudioRouteChanged {
        audio: AudioRouteState,
    },
    RadioPowerChanged {
        radio: RadioPower,
    },
    CapabilitiesChanged {
        capabilities: Capabilities,
    },
    ServiceRestarted {
        state: RedactedTelephonyState,
    },
}

impl RedactedTelephonyEvent {
    pub fn name(&self) -> &'static str {
        match self {
            RedactedTelephonyEvent::ModemChanged { .. } => "modem-changed",
            RedactedTelephonyEvent::SimChanged { .. } => "sim-changed",
            RedactedTelephonyEvent::RegistrationChanged { .. } => "registration-changed",
            RedactedTelephonyEvent::SignalChanged { .. } => "signal-changed",
            RedactedTelephonyEvent::CallChanged { .. } => "call-changed",
            RedactedTelephonyEvent::SmsChanged { .. } => "sms-changed",
            RedactedTelephonyEvent::DataChanged { .. } => "data-changed",
            RedactedTelephonyEvent::AudioRouteChanged { .. } => "audio-route-changed",
            RedactedTelephonyEvent::RadioPowerChanged { .. } => "radio-power-changed",
            RedactedTelephonyEvent::CapabilitiesChanged { .. } => "capabilities-changed",
            RedactedTelephonyEvent::ServiceRestarted { .. } => "service-restarted",
        }
    }

    /// Folds into a redacted snapshot (mirror of [`TelephonyEvent::apply_to`]).
    pub fn apply_to(&self, state: &mut RedactedTelephonyState) {
        match self {
            RedactedTelephonyEvent::ModemChanged { modem } => state.modem = modem.clone(),
            RedactedTelephonyEvent::SimChanged { sim } => state.sim = sim.clone(),
            RedactedTelephonyEvent::RegistrationChanged {
                registration,
                operator,
                access,
            } => {
                state.registration = *registration;
                state.operator = operator.clone();
                state.access = *access;
            }
            RedactedTelephonyEvent::SignalChanged { signal } => state.signal = *signal,
            RedactedTelephonyEvent::CallChanged { call } => {
                state.calls.insert(call.id.clone(), call.clone());
            }
            RedactedTelephonyEvent::SmsChanged { sms } => {
                if sms.state == SmsState::Deleted {
                    state.sms.remove(&sms.id);
                } else {
                    state.sms.insert(sms.id.clone(), sms.clone());
                }
            }
            RedactedTelephonyEvent::DataChanged { data } => state.data = data.clone(),
            RedactedTelephonyEvent::AudioRouteChanged { audio } => state.audio = audio.clone(),
            RedactedTelephonyEvent::RadioPowerChanged { radio } => state.radio = *radio,
            RedactedTelephonyEvent::CapabilitiesChanged { capabilities } => {
                state.capabilities = capabilities.clone();
            }
            RedactedTelephonyEvent::ServiceRestarted { state: full } => *state = full.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Typed failures. Unsupported operations are errors, never imitated success.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum TelephonyError {
    Unsupported {
        feature: TelephonyFeature,
    },
    /// The connection lacks the named capability string (`telephony-state`,
    /// `telephony-call`, `telephony-sms`).
    Unauthorized {
        capability: String,
    },
    NoModem,
    NoSim,
    SimLocked,
    NoService,
    InvalidNumber {
        reason: String,
    },
    Busy,
    NotFound {
        id: String,
    },
    InvalidState {
        from: String,
        op: String,
    },
    Backend {
        reason: String,
    },
    Protocol {
        reason: String,
    },
}

impl TelephonyError {
    pub fn kind(&self) -> &'static str {
        match self {
            TelephonyError::Unsupported { .. } => "unsupported",
            TelephonyError::Unauthorized { .. } => "unauthorized",
            TelephonyError::NoModem => "no-modem",
            TelephonyError::NoSim => "no-sim",
            TelephonyError::SimLocked => "sim-locked",
            TelephonyError::NoService => "no-service",
            TelephonyError::InvalidNumber { .. } => "invalid-number",
            TelephonyError::Busy => "busy",
            TelephonyError::NotFound { .. } => "not-found",
            TelephonyError::InvalidState { .. } => "invalid-state",
            TelephonyError::Backend { .. } => "backend",
            TelephonyError::Protocol { .. } => "protocol",
        }
    }

    pub fn unsupported(feature: TelephonyFeature) -> Self {
        TelephonyError::Unsupported { feature }
    }

    pub fn not_found(id: impl fmt::Display) -> Self {
        TelephonyError::NotFound { id: id.to_string() }
    }

    pub fn invalid_state(from: impl fmt::Display, op: &str) -> Self {
        TelephonyError::InvalidState {
            from: from.to_string(),
            op: op.to_owned(),
        }
    }

    pub fn backend(reason: impl fmt::Display) -> Self {
        TelephonyError::Backend {
            reason: reason.to_string(),
        }
    }

    pub fn protocol(reason: impl fmt::Display) -> Self {
        TelephonyError::Protocol {
            reason: reason.to_string(),
        }
    }
}

impl fmt::Display for TelephonyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TelephonyError::Unsupported { feature } => write!(f, "unsupported feature {feature}"),
            TelephonyError::Unauthorized { capability } => {
                write!(f, "missing capability {capability}")
            }
            TelephonyError::NoModem => f.write_str("no modem"),
            TelephonyError::NoSim => f.write_str("no sim"),
            TelephonyError::SimLocked => f.write_str("sim locked"),
            TelephonyError::NoService => f.write_str("no service"),
            TelephonyError::InvalidNumber { reason } => write!(f, "invalid number: {reason}"),
            TelephonyError::Busy => f.write_str("busy"),
            TelephonyError::NotFound { id } => write!(f, "{id} not found"),
            TelephonyError::InvalidState { from, op } => {
                write!(f, "cannot {op} from state {from}")
            }
            TelephonyError::Backend { reason } => write!(f, "backend error: {reason}"),
            TelephonyError::Protocol { reason } => write!(f, "protocol error: {reason}"),
        }
    }
}

impl std::error::Error for TelephonyError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn number() -> E164 {
        E164::new("+15550100042").unwrap()
    }

    #[test]
    fn e164_validates_plus_and_digit_count() {
        assert!(E164::new("+15550100042").is_ok());
        assert!(E164::new("+12345678").is_ok()); // 8 digits
        assert!(E164::new("+123456789012345").is_ok()); // 15 digits
        for bad in [
            "",
            "15550100042",
            "+1234567",          // 7 digits
            "+1234567890123456", // 16 digits
            "+1555 0100042",
            "+1555-0100042",
            "+",
            "++15550100042",
            "+1555010004a",
        ] {
            let err = E164::new(bad).expect_err(bad);
            assert!(matches!(err, TelephonyError::InvalidNumber { .. }), "{bad}");
        }
    }

    #[test]
    fn e164_serializes_as_a_string_and_rejects_bad_input() {
        let n: E164 = serde_json::from_str("\"+15550100042\"").unwrap();
        assert_eq!(n.as_str(), "+15550100042");
        assert_eq!(serde_json::to_string(&n).unwrap(), "\"+15550100042\"");
        assert!(serde_json::from_str::<E164>("\"911\"").is_err());
        assert!(serde_json::from_str::<E164>("\"15550100042\"").is_err());
    }

    #[test]
    fn e164_redaction_keeps_last_four_digits_and_debug_is_redacted() {
        assert_eq!(number().redacted(), "+***0042");
        assert_eq!(format!("{:?}", number()), "E164(+***0042)");
        assert_eq!(format!("{}", number()), "+15550100042");
    }

    #[test]
    fn emergency_numbers_are_typed_unsupported() {
        for n in EMERGENCY_NUMBERS {
            assert!(is_emergency_number(n));
            assert_eq!(
                E164::classify_dial_string(n),
                Err(TelephonyError::Unsupported {
                    feature: TelephonyFeature::Emergency
                }),
                "{n}"
            );
            let plus = format!("+{n}");
            assert!(matches!(
                E164::classify_dial_string(&plus),
                Err(TelephonyError::Unsupported {
                    feature: TelephonyFeature::Emergency
                })
            ));
        }
        assert!(!number().is_emergency());
        assert_eq!(E164::classify_dial_string("+15550100042"), Ok(number()));
        assert!(matches!(
            E164::classify_dial_string("123"),
            Err(TelephonyError::InvalidNumber { .. })
        ));
    }

    #[test]
    fn dtmf_restricts_digits() {
        for ok in ['0', '9', '*', '#', 'A', 'd'] {
            assert!(Dtmf::new(ok).is_ok(), "{ok}");
        }
        assert_eq!(Dtmf::new('d').unwrap().as_char(), 'D');
        for bad in ['e', ' ', '+', 'x'] {
            assert!(Dtmf::new(bad).is_err(), "{bad}");
        }
        assert_eq!(
            serde_json::to_string(&Dtmf::new('5').unwrap()).unwrap(),
            "\"5\""
        );
        assert!(serde_json::from_str::<Dtmf>("\"z\"").is_err());
    }

    #[test]
    fn serde_forms_are_kebab_case_and_tagged() {
        assert_eq!(
            serde_json::to_string(&ModemState::Failed {
                reason: "init".into()
            })
            .unwrap(),
            r#"{"state":"failed","reason":"init"}"#
        );
        assert_eq!(
            serde_json::to_string(&SimState::Locked {
                lock: SimLock::Pin,
                attempts_left: 3
            })
            .unwrap(),
            r#"{"state":"locked","lock":"pin","attempts_left":3}"#
        );
        assert_eq!(
            serde_json::to_string(&Registration::Registered { roaming: true }).unwrap(),
            r#"{"state":"registered","roaming":true}"#
        );
        assert_eq!(
            serde_json::to_string(&Registration::EmergencyOnly).unwrap(),
            r#"{"state":"emergency-only"}"#
        );
        assert_eq!(
            serde_json::to_string(&CallEndReason::RemoteHangup).unwrap(),
            r#"{"kind":"remote-hangup"}"#
        );
        assert_eq!(
            serde_json::to_string(&SmsFailure::incomplete_multipart()).unwrap(),
            r#"{"kind":"failed","reason":"incomplete-multipart"}"#
        );
        assert_eq!(
            serde_json::to_string(&TelephonyFeature::MultipartSms).unwrap(),
            "\"multipart-sms\""
        );
        assert_eq!(
            serde_json::to_string(&DataBearer::Connected { interface: None }).unwrap(),
            r#"{"state":"connected"}"#
        );
        assert_eq!(
            serde_json::to_string(&TelephonyError::Unsupported {
                feature: TelephonyFeature::Emergency
            })
            .unwrap(),
            r#"{"kind":"unsupported","feature":"emergency"}"#
        );
        assert_eq!(
            serde_json::to_string(&TelephonyError::Unauthorized {
                capability: "telephony-call".into()
            })
            .unwrap(),
            r#"{"kind":"unauthorized","capability":"telephony-call"}"#
        );
        assert_eq!(serde_json::to_string(&CallState::Held).unwrap(), "\"held\"");
        assert_eq!(serde_json::to_string(&RadioPower::Off).unwrap(), "\"off\"");
    }

    #[test]
    fn data_profiles_allow_default_or_apn_and_reject_unsafe_values() {
        assert!(DataProfile::default().validate().is_ok());
        assert!(
            DataProfile {
                apn: Some("internet".into())
            }
            .validate()
            .is_ok()
        );
        for apn in ["", "has whitespace", "has\nnewline", &"x".repeat(101)] {
            assert!(
                DataProfile {
                    apn: Some(apn.to_owned())
                }
                .validate()
                .is_err()
            );
        }
        assert_eq!(
            serde_json::to_string(&DataProfile {
                apn: Some("internet".into())
            })
            .unwrap(),
            r#"{"apn":"internet"}"#
        );
        assert_eq!(
            serde_json::to_string(&DataProfile::default()).unwrap(),
            "{}"
        );
    }

    #[test]
    fn signal_clamps_bars() {
        assert_eq!(Signal::bars(9).bars, 4);
        assert_eq!(Signal::new(2, Some(-90)).rssi_dbm, Some(-90));
    }

    #[test]
    fn call_state_machine() {
        use CallState::*;
        assert!(Dialing.can_transition_to(Active));
        assert!(Ringing.can_transition_to(Active));
        assert!(Active.can_transition_to(Held));
        assert!(Held.can_transition_to(Active));
        assert!(Ringing.can_transition_to(Ended));
        assert!(!Ended.can_transition_to(Active));
        assert!(!Dialing.can_transition_to(Held));
        assert!(!Ringing.can_transition_to(Dialing));
    }

    #[test]
    fn sms_state_machine() {
        use SmsState::*;
        assert!(Sending.can_transition_to(Sent));
        assert!(Sent.can_transition_to(Delivered));
        assert!(Sending.can_transition_to(Failed));
        assert!(Received.can_transition_to(Read));
        assert!(Read.can_transition_to(Deleted));
        assert!(!Delivered.can_transition_to(Sent));
        assert!(!Received.can_transition_to(Sent));
        assert!(!Deleted.can_transition_to(Read));
    }

    fn call(id: &str, state: CallState) -> Call {
        Call {
            id: CallId::from(id),
            direction: Direction::Incoming,
            state,
            party: PartyRef::new(number()),
            started_at_ms: 10,
            end_reason: None,
        }
    }

    fn sms(id: &str, state: SmsState) -> Sms {
        Sms {
            id: MessageId::from(id),
            direction: Direction::Incoming,
            party: PartyRef::new(number()),
            body: "hello".to_owned(),
            parts: Vec::new(),
            state,
            timestamp_ms: 20,
            failure: None,
        }
    }

    #[test]
    fn events_fold_into_the_snapshot() {
        let mut state = TelephonyState::new();
        assert_eq!(state.schema_version, SCHEMA_VERSION);
        TelephonyEvent::ModemChanged {
            modem: ModemState::Ready,
        }
        .apply_to(&mut state);
        TelephonyEvent::SimChanged {
            sim: SimState::Ready,
        }
        .apply_to(&mut state);
        TelephonyEvent::RegistrationChanged {
            registration: Registration::Registered { roaming: false },
            operator: Some(Operator {
                name: "Ferrix Mock".into(),
                mcc_mnc: Some("001-01".into()),
            }),
            access: AccessTechnology::Lte,
        }
        .apply_to(&mut state);
        TelephonyEvent::SignalChanged {
            signal: Signal::bars(3),
        }
        .apply_to(&mut state);
        TelephonyEvent::CallChanged {
            call: call("c1", CallState::Ringing),
        }
        .apply_to(&mut state);
        TelephonyEvent::CallChanged {
            call: call("c1", CallState::Active),
        }
        .apply_to(&mut state);
        TelephonyEvent::SmsChanged {
            sms: sms("m1", SmsState::Received),
        }
        .apply_to(&mut state);
        TelephonyEvent::RadioPowerChanged {
            radio: RadioPower::Off,
        }
        .apply_to(&mut state);
        TelephonyEvent::DataChanged {
            data: DataBearer::Disconnected,
        }
        .apply_to(&mut state);
        TelephonyEvent::AudioRouteChanged {
            audio: AudioRouteState {
                active: AudioRoute::Speaker,
                available: [AudioRoute::Earpiece, AudioRoute::Speaker].into(),
            },
        }
        .apply_to(&mut state);
        TelephonyEvent::CapabilitiesChanged {
            capabilities: [TelephonyFeature::Voice].into(),
        }
        .apply_to(&mut state);

        assert_eq!(state.modem, ModemState::Ready);
        assert_eq!(state.sim, SimState::Ready);
        assert!(state.registration.is_registered());
        assert_eq!(state.operator.as_ref().unwrap().name, "Ferrix Mock");
        assert_eq!(state.access, AccessTechnology::Lte);
        assert_eq!(state.signal.bars, 3);
        assert_eq!(state.calls.len(), 1);
        assert_eq!(state.calls[&CallId::from("c1")].state, CallState::Active);
        assert_eq!(state.live_calls().count(), 1);
        assert_eq!(state.sms.len(), 1);
        assert_eq!(state.radio, RadioPower::Off);
        assert_eq!(state.data, DataBearer::Disconnected);
        assert_eq!(state.audio.active, AudioRoute::Speaker);
        assert!(state.capabilities.contains(&TelephonyFeature::Voice));

        // Deleting removes the message; ending keeps the call for display.
        TelephonyEvent::SmsChanged {
            sms: sms("m1", SmsState::Deleted),
        }
        .apply_to(&mut state);
        assert!(state.sms.is_empty());
        TelephonyEvent::CallChanged {
            call: call("c1", CallState::Ended),
        }
        .apply_to(&mut state);
        assert_eq!(state.live_calls().count(), 0);
        assert_eq!(state.calls.len(), 1);

        // Restart replaces everything.
        let fresh = TelephonyState::new();
        TelephonyEvent::ServiceRestarted {
            state: fresh.clone(),
        }
        .apply_to(&mut state);
        assert_eq!(state, fresh);
    }

    #[test]
    fn events_are_tagged_and_round_trip() {
        let event = TelephonyEvent::CallChanged {
            call: call("c1", CallState::Ringing),
        };
        let text = serde_json::to_string(&event).unwrap();
        assert!(
            text.starts_with(r#"{"event":"call-changed","call":{"id":"c1""#),
            "{text}"
        );
        assert_eq!(
            serde_json::from_str::<TelephonyEvent>(&text).unwrap(),
            event
        );
        assert_eq!(event.name(), "call-changed");
        let restarted = TelephonyEvent::ServiceRestarted {
            state: TelephonyState::new(),
        };
        let text = serde_json::to_string(&restarted).unwrap();
        assert!(text.starts_with(r#"{"event":"service-restarted","state":{"schema_version":1"#));
    }

    #[test]
    fn redaction_removes_numbers_and_bodies_but_keeps_ids_and_states() {
        let mut state = TelephonyState::new();
        TelephonyEvent::CallChanged {
            call: call("c1", CallState::Ringing),
        }
        .apply_to(&mut state);
        TelephonyEvent::SmsChanged {
            sms: sms("m1", SmsState::Received),
        }
        .apply_to(&mut state);
        let redacted = state.redacted();
        let text = serde_json::to_string(&redacted).unwrap();
        assert!(!text.contains("+15550100042"), "{text}");
        assert!(!text.contains("hello"), "{text}");
        assert!(text.contains(r#""party_redacted":"+***0042""#), "{text}");
        assert!(text.contains(r#""id":"c1""#));
        assert!(text.contains(r#""id":"m1""#));
        assert!(text.contains(r#""state":"received""#));

        let event = TelephonyEvent::SmsChanged {
            sms: sms("m2", SmsState::Received),
        }
        .redacted();
        assert_eq!(event.name(), "sms-changed");
        let text = serde_json::to_string(&event).unwrap();
        assert!(text.starts_with(r#"{"event":"sms-changed""#));
        assert!(!text.contains("hello"));

        let mut folded = RedactedTelephonyState::default();
        event.apply_to(&mut folded);
        assert_eq!(folded.sms.len(), 1);
        RedactedTelephonyEvent::SmsChanged {
            sms: sms("m2", SmsState::Deleted).redacted(),
        }
        .apply_to(&mut folded);
        assert!(folded.sms.is_empty());
    }

    #[test]
    fn feature_names_round_trip() {
        for name in [
            "voice",
            "sms",
            "multipart-sms",
            "delivery-reports",
            "dtmf",
            "hold",
            "data",
            "audio-route",
            "sim-pin",
            "radio-power",
            "emergency",
        ] {
            let feature = TelephonyFeature::parse(name).expect(name);
            assert_eq!(feature.name(), name);
            assert_eq!(
                serde_json::to_string(&feature).unwrap(),
                format!("\"{name}\"")
            );
        }
        assert!(TelephonyFeature::parse("volte").is_none());
    }

    #[test]
    fn errors_display_and_carry_kinds() {
        assert_eq!(TelephonyError::NoService.kind(), "no-service");
        assert_eq!(TelephonyError::not_found("c9").to_string(), "c9 not found");
        assert_eq!(
            TelephonyError::invalid_state(CallState::Ended, "answer").to_string(),
            "cannot answer from state ended"
        );
    }
}
