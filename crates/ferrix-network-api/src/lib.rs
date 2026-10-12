//! Stable, target-neutral networking contract (ADR-002, schema 2).
//!
//! Every process that talks about Wi-Fi or connectivity (the networking
//! service, the protocol client, applications, scenario tooling) shares the
//! identities, states, deltas, and typed errors in this crate. Nothing here
//! performs I/O and nothing names an implementation: the service that owns
//! the state and the adapters that produce it live elsewhere.
//!
//! Identifiers on the wire are stable per service run (`ApId`, `ProfileId`)
//! or digests (`bssid_hash`, [`Ssid::hash`]). A raw hardware address never
//! appears in any type of this crate, so serialized values and `Debug`
//! output can be logged as evidence without redaction.

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// Current schema version of every serialized type in this crate and of the
/// networking service wire protocol built on it.
///
/// History (ADR-002):
/// - 1: access points, connect/disconnect, saved profiles, typed failures.
/// - 2: adds the `join-hidden` request (join a network by name, for access
///   points that do not broadcast their SSID) and [`validate_hidden_join`].
///   No existing message changed shape; the handshake is still an exact
///   match, so a schema-1 peer is refused with a typed mismatch rather than
///   meeting a request it cannot parse.
pub const SCHEMA_VERSION: u32 = 2;

/// Longest SSID 802.11 allows, in bytes.
pub const SSID_MAX_LEN: usize = 32;

/// Default socket path of the networking service inside the guest.
pub const DEFAULT_NETWORKD_SOCKET: &str = "/run/ferrix/networkd.sock";
/// Environment variable overriding [`DEFAULT_NETWORKD_SOCKET`].
pub const NETWORKD_SOCKET_ENV: &str = "FERRIX_NETWORKD_SOCKET";

/// Number of hexadecimal characters kept from a SHA-256 digest.
pub const HASH_PREFIX_LEN: usize = 16;

/// Digest used for every hashed identifier: the first [`HASH_PREFIX_LEN`]
/// hex characters of `sha256(bytes)`. Adapters hash hardware addresses with
/// this at their boundary; the raw value never leaves them.
pub fn hash_identifier(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut text = String::with_capacity(HASH_PREFIX_LEN);
    for byte in digest.iter() {
        if text.len() >= HASH_PREFIX_LEN {
            break;
        }
        text.push_str(&format!("{byte:02x}"));
    }
    text.truncate(HASH_PREFIX_LEN);
    text
}

/// Hashes a hardware address after normalizing case, so the same address
/// written `AA:BB` and `aa:bb` yields one digest.
pub fn hash_hardware_address(raw: &str) -> String {
    hash_identifier(raw.trim().to_ascii_lowercase().as_bytes())
}

// ---------------------------------------------------------------------------
// Identities
// ---------------------------------------------------------------------------

/// Identifier of a visible access point, stable for one service run.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ApId(String);

impl ApId {
    pub fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ApId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Identifier of a saved connection profile, stable for one service run.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProfileId(String);

impl ProfileId {
    pub fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ProfileId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A network name: raw bytes (an SSID need not be text). Serialized as a
/// JSON string when it is valid UTF-8 and as `{"hex": "..."}` otherwise;
/// both forms are accepted on input.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ssid(Vec<u8>);

impl Ssid {
    pub fn new(bytes: impl Into<Vec<u8>>) -> Self {
        Self(bytes.into())
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// The name as text when it is valid UTF-8.
    pub fn as_text(&self) -> Option<&str> {
        std::str::from_utf8(&self.0).ok()
    }

    /// Stable digest for evidence records (`ssid_hash`).
    pub fn hash(&self) -> String {
        hash_identifier(&self.0)
    }

    fn hex(&self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }

    fn from_hex(text: &str) -> Result<Self, String> {
        if !text.len().is_multiple_of(2) || !text.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(format!(
                "ssid hex {text:?} is not an even-length hex string"
            ));
        }
        let bytes = (0..text.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&text[i..i + 2], 16).map_err(|e| e.to_string()))
            .collect::<Result<Vec<u8>, String>>()?;
        Ok(Self(bytes))
    }
}

impl From<&str> for Ssid {
    fn from(text: &str) -> Self {
        Self(text.as_bytes().to_vec())
    }
}

impl From<String> for Ssid {
    fn from(text: String) -> Self {
        Self(text.into_bytes())
    }
}

impl fmt::Display for Ssid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.as_text() {
            Some(text) => f.write_str(text),
            None => write!(f, "<{}>", self.hex()),
        }
    }
}

impl fmt::Debug for Ssid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Ssid({self})")
    }
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum SsidWire {
    Text(String),
    Bytes { hex: String },
}

impl Serialize for Ssid {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.as_text() {
            Some(text) => SsidWire::Text(text.to_owned()).serialize(serializer),
            None => SsidWire::Bytes { hex: self.hex() }.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for Ssid {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match SsidWire::deserialize(deserializer)? {
            SsidWire::Text(text) => Ok(Ssid::from(text)),
            SsidWire::Bytes { hex } => Ssid::from_hex(&hex).map_err(serde::de::Error::custom),
        }
    }
}

/// A Wi-Fi secret in transit. Serialized as a plain string on the wire (it
/// travels only in `connect`), but `Debug` prints `<redacted>` so no log,
/// record, or assertion message can ever carry the value.
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Passphrase(String);

impl Passphrase {
    pub fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    /// The secret itself; call only where the backend consumes it.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Passphrase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Passphrase(<redacted>)")
    }
}

/// Security types a network can be joined with by name (`join-hidden`).
/// Hidden networks advertise no beacon to read the type from, so the user
/// chooses it; WEP and enterprise networks are not offered.
pub const HIDDEN_JOIN_SECURITY: [SecurityType; 3] = [
    SecurityType::Open,
    SecurityType::Wpa2Psk,
    SecurityType::Wpa3Sae,
];

/// Checks a `join-hidden` request before any backend sees it. The error
/// text names the field and the rule, never the passphrase.
///
/// - `ssid`: 1..=[`SSID_MAX_LEN`] bytes and not all NUL (an all-NUL name
///   is what a hidden beacon carries, not a name).
/// - `security`: one of [`HIDDEN_JOIN_SECURITY`] (`wpa-psk` is accepted as
///   the WPA1 spelling of the same pre-shared key setup).
/// - `passphrase`: absent for open networks; for WPA2 8..=63 characters
///   or 64 hex digits; for WPA3 (SAE) at least one character.
pub fn validate_hidden_join(
    ssid: &Ssid,
    security: SecurityType,
    passphrase: Option<&Passphrase>,
) -> Result<(), NetworkError> {
    let invalid = |reason: &str| NetworkError::Protocol {
        reason: format!("join-hidden: {reason}"),
    };
    let bytes = ssid.as_bytes();
    if bytes.is_empty() || bytes.iter().all(|b| *b == 0) {
        return Err(invalid("ssid is empty"));
    }
    if bytes.len() > SSID_MAX_LEN {
        return Err(invalid("ssid is longer than 32 bytes"));
    }
    if !(HIDDEN_JOIN_SECURITY.contains(&security) || security == SecurityType::WpaPsk) {
        return Err(NetworkError::Unsupported {
            feature: format!("joining {security} networks by name"),
        });
    }
    match (security, passphrase.map(Passphrase::expose)) {
        (SecurityType::Open, None) => Ok(()),
        (SecurityType::Open, Some(_)) => Err(invalid("open networks take no passphrase")),
        (_, None) | (_, Some("")) => Err(invalid("passphrase is required")),
        (SecurityType::Wpa3Sae, Some(_)) => Ok(()),
        (_, Some(secret)) => {
            let chars = secret.chars().count();
            let hex = secret.len() == 64 && secret.chars().all(|c| c.is_ascii_hexdigit());
            if hex || (8..=63).contains(&chars) {
                Ok(())
            } else {
                Err(invalid("passphrase must be 8 to 63 characters"))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// States
// ---------------------------------------------------------------------------

/// What kind of link an adapter provides. Wired-style transports (for
/// example a virtual NIC in an emulator) are never reported as Wi-Fi.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AdapterKind {
    Wifi,
    EthernetLike,
}

impl AdapterKind {
    pub fn name(self) -> &'static str {
        match self {
            AdapterKind::Wifi => "wifi",
            AdapterKind::EthernetLike => "ethernet-like",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum AdapterPresence {
    #[default]
    Absent,
    Present {
        kind: AdapterKind,
    },
}

impl AdapterPresence {
    pub fn name(&self) -> &'static str {
        match self {
            AdapterPresence::Absent => "absent",
            AdapterPresence::Present { .. } => "present",
        }
    }

    pub fn is_present(&self) -> bool {
        matches!(self, AdapterPresence::Present { .. })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum RadioState {
    Disabled,
    Enabled,
    Unavailable { reason: String },
}

impl Default for RadioState {
    fn default() -> Self {
        RadioState::Unavailable {
            reason: "no adapter".to_owned(),
        }
    }
}

impl RadioState {
    pub fn name(&self) -> &'static str {
        match self {
            RadioState::Disabled => "disabled",
            RadioState::Enabled => "enabled",
            RadioState::Unavailable { .. } => "unavailable",
        }
    }

    pub fn is_enabled(&self) -> bool {
        matches!(self, RadioState::Enabled)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum ScanState {
    #[default]
    Idle,
    Scanning,
    Completed {
        count: usize,
    },
    Failed {
        reason: String,
    },
}

impl ScanState {
    pub fn name(&self) -> &'static str {
        match self {
            ScanState::Idle => "idle",
            ScanState::Scanning => "scanning",
            ScanState::Completed { .. } => "completed",
            ScanState::Failed { .. } => "failed",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SecurityType {
    Open,
    Wep,
    WpaPsk,
    Wpa2Psk,
    Wpa3Sae,
    Enterprise,
    #[default]
    Unknown,
}

impl SecurityType {
    pub fn name(self) -> &'static str {
        match self {
            SecurityType::Open => "open",
            SecurityType::Wep => "wep",
            SecurityType::WpaPsk => "wpa-psk",
            SecurityType::Wpa2Psk => "wpa2-psk",
            SecurityType::Wpa3Sae => "wpa3-sae",
            SecurityType::Enterprise => "enterprise",
            SecurityType::Unknown => "unknown",
        }
    }

    /// Whether joining needs a passphrase from the user.
    pub fn needs_passphrase(self) -> bool {
        !matches!(self, SecurityType::Open)
    }
}

impl fmt::Display for SecurityType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A visible access point. Only a digest of the hardware address is kept.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AccessPoint {
    pub id: ApId,
    pub ssid: Ssid,
    /// [`hash_hardware_address`] of the BSSID; the raw value is never stored.
    pub bssid_hash: String,
    pub signal_percent: u8,
    pub security: SecurityType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frequency_mhz: Option<u32>,
}

impl AccessPoint {
    /// Fields safe for an evidence record: id, `ssid_hash`, signal, security.
    pub fn summary(&self) -> Value {
        json!({
            "ap": self.id.as_str(),
            "ssid_hash": self.ssid.hash(),
            "signal_percent": self.signal_percent,
            "security": self.security.name(),
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum ConnectionState {
    #[default]
    Disconnected,
    Associating,
    Authenticating,
    Configuring,
    Connected {
        ap: ApId,
    },
    Failed {
        error: NetworkError,
    },
}

impl ConnectionState {
    pub fn name(&self) -> &'static str {
        match self {
            ConnectionState::Disconnected => "disconnected",
            ConnectionState::Associating => "associating",
            ConnectionState::Authenticating => "authenticating",
            ConnectionState::Configuring => "configuring",
            ConnectionState::Connected { .. } => "connected",
            ConnectionState::Failed { .. } => "failed",
        }
    }

    pub fn is_connected(&self) -> bool {
        matches!(self, ConnectionState::Connected { .. })
    }

    /// True while a join is in progress.
    pub fn is_transitional(&self) -> bool {
        matches!(
            self,
            ConnectionState::Associating
                | ConnectionState::Authenticating
                | ConnectionState::Configuring
        )
    }

    pub fn connected_ap(&self) -> Option<&ApId> {
        match self {
            ConnectionState::Connected { ap } => Some(ap),
            _ => None,
        }
    }
}

/// A saved connection. Secrets are never part of the contract.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SavedProfile {
    pub id: ProfileId,
    pub ssid: Ssid,
    pub security: SecurityType,
    pub autoconnect: bool,
}

impl SavedProfile {
    pub fn summary(&self) -> Value {
        json!({
            "profile": self.id.as_str(),
            "ssid_hash": self.ssid.hash(),
            "security": self.security.name(),
            "autoconnect": self.autoconnect,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IpConfig {
    /// Addresses with prefix length, for example `10.0.2.15/24`.
    pub addresses: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gateway: Option<String>,
    pub dns: Vec<String>,
    pub dhcp: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum Connectivity {
    #[default]
    None,
    Local,
    Portal {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        url: Option<String>,
    },
    Limited,
    Full,
}

impl Connectivity {
    pub fn name(&self) -> &'static str {
        match self {
            Connectivity::None => "none",
            Connectivity::Local => "local",
            Connectivity::Portal { .. } => "portal",
            Connectivity::Limited => "limited",
            Connectivity::Full => "full",
        }
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Typed failures. Unsupported operations are errors, never silent successes.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum NetworkError {
    Unauthorized { capability: String },
    RadioDisabled,
    NoAdapter,
    NotFound { id: String },
    AuthenticationFailed,
    DhcpFailed,
    DnsFailed,
    RouteFailed,
    TimedOut,
    Unsupported { feature: String },
    Backend { reason: String },
    Protocol { reason: String },
}

impl NetworkError {
    pub fn name(&self) -> &'static str {
        match self {
            NetworkError::Unauthorized { .. } => "unauthorized",
            NetworkError::RadioDisabled => "radio-disabled",
            NetworkError::NoAdapter => "no-adapter",
            NetworkError::NotFound { .. } => "not-found",
            NetworkError::AuthenticationFailed => "authentication-failed",
            NetworkError::DhcpFailed => "dhcp-failed",
            NetworkError::DnsFailed => "dns-failed",
            NetworkError::RouteFailed => "route-failed",
            NetworkError::TimedOut => "timed-out",
            NetworkError::Unsupported { .. } => "unsupported",
            NetworkError::Backend { .. } => "backend",
            NetworkError::Protocol { .. } => "protocol",
        }
    }
}

impl fmt::Display for NetworkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NetworkError::Unauthorized { capability } => {
                write!(f, "unauthorized: capability {capability} is not granted")
            }
            NetworkError::RadioDisabled => f.write_str("the radio is disabled"),
            NetworkError::NoAdapter => f.write_str("no network adapter is present"),
            NetworkError::NotFound { id } => write!(f, "{id} was not found"),
            NetworkError::AuthenticationFailed => f.write_str("authentication failed"),
            NetworkError::DhcpFailed => f.write_str("address configuration (DHCP) failed"),
            NetworkError::DnsFailed => f.write_str("name resolution (DNS) failed"),
            NetworkError::RouteFailed => f.write_str("no route to the network"),
            NetworkError::TimedOut => f.write_str("the operation timed out"),
            NetworkError::Unsupported { feature } => {
                write!(f, "{feature} is not supported by this backend")
            }
            NetworkError::Backend { reason } => write!(f, "backend error: {reason}"),
            NetworkError::Protocol { reason } => write!(f, "protocol error: {reason}"),
        }
    }
}

impl std::error::Error for NetworkError {}

// ---------------------------------------------------------------------------
// Aggregate state and events
// ---------------------------------------------------------------------------

/// The full networking snapshot a client receives on (re)connect.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkState {
    pub schema_version: u32,
    pub adapter: AdapterPresence,
    pub radio: RadioState,
    pub scan: ScanState,
    pub access_points: Vec<AccessPoint>,
    pub connection: ConnectionState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ip: Option<IpConfig>,
    pub connectivity: Connectivity,
    pub profiles: Vec<SavedProfile>,
}

impl Default for NetworkState {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            adapter: AdapterPresence::default(),
            radio: RadioState::default(),
            scan: ScanState::default(),
            access_points: Vec::new(),
            connection: ConnectionState::default(),
            ip: None,
            connectivity: Connectivity::default(),
            profiles: Vec::new(),
        }
    }
}

impl NetworkState {
    /// Finds a visible access point by id.
    pub fn access_point(&self, id: &ApId) -> Option<&AccessPoint> {
        self.access_points.iter().find(|ap| &ap.id == id)
    }

    /// Finds a saved profile by id.
    pub fn profile(&self, id: &ProfileId) -> Option<&SavedProfile> {
        self.profiles.iter().find(|p| &p.id == id)
    }

    /// Evidence-safe view: state names, ids, and digests only.
    pub fn summary(&self) -> Value {
        json!({
            "adapter": self.adapter.name(),
            "adapter_kind": match self.adapter {
                AdapterPresence::Present { kind } => Value::from(kind.name()),
                AdapterPresence::Absent => Value::Null,
            },
            "radio": self.radio.name(),
            "scan": self.scan.name(),
            "access_points": self.access_points.iter().map(AccessPoint::summary).collect::<Vec<_>>(),
            "connection": self.connection.name(),
            "ap": self.connection.connected_ap().map(ApId::as_str),
            "ip": self.ip.as_ref().map(|ip| json!({
                "addresses": ip.addresses.len(),
                "gateway": ip.gateway.is_some(),
                "dns": ip.dns.len(),
                "dhcp": ip.dhcp,
            })),
            "connectivity": self.connectivity.name(),
            "profiles": self.profiles.iter().map(SavedProfile::summary).collect::<Vec<_>>(),
        })
    }
}

/// Deltas published by the networking service. Applying every event in
/// order to the snapshot received at subscription reproduces the service's
/// state; `service-restarted` replaces it wholesale.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "kebab-case")]
pub enum NetworkEvent {
    AdapterChanged {
        adapter: AdapterPresence,
    },
    RadioChanged {
        radio: RadioState,
    },
    ScanChanged {
        scan: ScanState,
    },
    AccessPointsChanged {
        access_points: Vec<AccessPoint>,
    },
    ConnectionChanged {
        connection: ConnectionState,
    },
    IpChanged {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ip: Option<IpConfig>,
    },
    ConnectivityChanged {
        connectivity: Connectivity,
    },
    ProfilesChanged {
        profiles: Vec<SavedProfile>,
    },
    /// The service (or its backend) restarted; `state` is authoritative.
    ServiceRestarted {
        state: NetworkState,
    },
}

impl NetworkEvent {
    pub fn name(&self) -> &'static str {
        match self {
            NetworkEvent::AdapterChanged { .. } => "adapter-changed",
            NetworkEvent::RadioChanged { .. } => "radio-changed",
            NetworkEvent::ScanChanged { .. } => "scan-changed",
            NetworkEvent::AccessPointsChanged { .. } => "access-points-changed",
            NetworkEvent::ConnectionChanged { .. } => "connection-changed",
            NetworkEvent::IpChanged { .. } => "ip-changed",
            NetworkEvent::ConnectivityChanged { .. } => "connectivity-changed",
            NetworkEvent::ProfilesChanged { .. } => "profiles-changed",
            NetworkEvent::ServiceRestarted { .. } => "service-restarted",
        }
    }

    /// Folds the delta into `state`.
    pub fn apply_to(&self, state: &mut NetworkState) {
        match self {
            NetworkEvent::AdapterChanged { adapter } => state.adapter = *adapter,
            NetworkEvent::RadioChanged { radio } => state.radio = radio.clone(),
            NetworkEvent::ScanChanged { scan } => state.scan = scan.clone(),
            NetworkEvent::AccessPointsChanged { access_points } => {
                state.access_points = access_points.clone();
            }
            NetworkEvent::ConnectionChanged { connection } => {
                state.connection = connection.clone();
            }
            NetworkEvent::IpChanged { ip } => state.ip = ip.clone(),
            NetworkEvent::ConnectivityChanged { connectivity } => {
                state.connectivity = connectivity.clone();
            }
            NetworkEvent::ProfilesChanged { profiles } => state.profiles = profiles.clone(),
            NetworkEvent::ServiceRestarted { state: fresh } => *state = fresh.clone(),
        }
    }

    /// Evidence-safe fields for a record: the delta's state name plus ids
    /// and digests. Ordered so `state` follows the event name.
    pub fn summary(&self) -> Vec<(&'static str, Value)> {
        match self {
            NetworkEvent::AdapterChanged { adapter } => {
                let mut fields = vec![("state", Value::from(adapter.name()))];
                if let AdapterPresence::Present { kind } = adapter {
                    fields.push(("kind", Value::from(kind.name())));
                }
                fields
            }
            NetworkEvent::RadioChanged { radio } => {
                let mut fields = vec![("state", Value::from(radio.name()))];
                if let RadioState::Unavailable { reason } = radio {
                    fields.push(("reason", Value::from(reason.as_str())));
                }
                fields
            }
            NetworkEvent::ScanChanged { scan } => {
                let mut fields = vec![("state", Value::from(scan.name()))];
                match scan {
                    ScanState::Completed { count } => fields.push(("count", Value::from(*count))),
                    ScanState::Failed { reason } => {
                        fields.push(("reason", Value::from(reason.as_str())));
                    }
                    ScanState::Idle | ScanState::Scanning => {}
                }
                fields
            }
            NetworkEvent::AccessPointsChanged { access_points } => vec![
                ("count", Value::from(access_points.len())),
                (
                    "access_points",
                    Value::Array(access_points.iter().map(AccessPoint::summary).collect()),
                ),
            ],
            NetworkEvent::ConnectionChanged { connection } => {
                let mut fields = vec![("state", Value::from(connection.name()))];
                match connection {
                    ConnectionState::Connected { ap } => {
                        fields.push(("ap", Value::from(ap.as_str())));
                    }
                    ConnectionState::Failed { error } => {
                        fields.push(("error", Value::from(error.name())));
                    }
                    _ => {}
                }
                fields
            }
            NetworkEvent::IpChanged { ip } => vec![
                ("configured", Value::from(ip.is_some())),
                (
                    "addresses",
                    Value::from(ip.as_ref().map_or(0, |ip| ip.addresses.len())),
                ),
                ("dhcp", Value::from(ip.as_ref().is_some_and(|ip| ip.dhcp))),
            ],
            NetworkEvent::ConnectivityChanged { connectivity } => {
                vec![("state", Value::from(connectivity.name()))]
            }
            NetworkEvent::ProfilesChanged { profiles } => vec![
                ("count", Value::from(profiles.len())),
                (
                    "profiles",
                    Value::Array(profiles.iter().map(SavedProfile::summary).collect()),
                ),
            ],
            NetworkEvent::ServiceRestarted { state } => vec![("snapshot", state.summary())],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ap(id: &str, ssid: &str) -> AccessPoint {
        AccessPoint {
            id: ApId::new(id),
            ssid: Ssid::from(ssid),
            bssid_hash: hash_hardware_address("AA:BB:CC:DD:EE:FF"),
            signal_percent: 78,
            security: SecurityType::Wpa2Psk,
            frequency_mhz: Some(2437),
        }
    }

    #[test]
    fn hashes_are_stable_sha256_prefixes() {
        // sha256("ferrix-lab") is fixed; the prefix is 16 hex characters.
        let hash = hash_identifier(b"ferrix-lab");
        assert_eq!(hash.len(), HASH_PREFIX_LEN);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(hash, hash_identifier(b"ferrix-lab"));
        assert_ne!(hash, hash_identifier(b"ferrix-guest"));
        assert_eq!(Ssid::from("ferrix-lab").hash(), hash);
        assert_eq!(
            hash_hardware_address("AA:BB:CC:DD:EE:FF"),
            hash_hardware_address(" aa:bb:cc:dd:ee:ff ")
        );
    }

    #[test]
    fn ssid_displays_text_or_hex_and_round_trips() {
        let text = Ssid::from("ferrix-lab");
        assert_eq!(text.to_string(), "ferrix-lab");
        assert_eq!(serde_json::to_string(&text).unwrap(), r#""ferrix-lab""#);
        assert_eq!(format!("{text:?}"), "Ssid(ferrix-lab)");

        let bytes = Ssid::new(vec![0xff, 0x00, 0x41]);
        assert_eq!(bytes.to_string(), "<ff0041>");
        let wire = serde_json::to_string(&bytes).unwrap();
        assert_eq!(wire, r#"{"hex":"ff0041"}"#);
        assert_eq!(serde_json::from_str::<Ssid>(&wire).unwrap(), bytes);
        assert_eq!(
            serde_json::from_str::<Ssid>(r#""ferrix-guest""#).unwrap(),
            Ssid::from("ferrix-guest")
        );
        assert!(serde_json::from_str::<Ssid>(r#"{"hex":"abc"}"#).is_err());
        assert!(serde_json::from_str::<Ssid>(r#"{"hex":"zz"}"#).is_err());
    }

    #[test]
    fn wire_forms_are_kebab_case_and_tagged() {
        assert_eq!(
            serde_json::to_string(&AdapterPresence::Present {
                kind: AdapterKind::EthernetLike
            })
            .unwrap(),
            r#"{"state":"present","kind":"ethernet-like"}"#
        );
        assert_eq!(
            serde_json::to_string(&AdapterPresence::Absent).unwrap(),
            r#"{"state":"absent"}"#
        );
        assert_eq!(
            serde_json::to_string(&RadioState::Unavailable {
                reason: "rfkill".into()
            })
            .unwrap(),
            r#"{"state":"unavailable","reason":"rfkill"}"#
        );
        assert_eq!(
            serde_json::to_string(&ScanState::Completed { count: 3 }).unwrap(),
            r#"{"state":"completed","count":3}"#
        );
        assert_eq!(
            serde_json::to_string(&ConnectionState::Connected {
                ap: ApId::new("ap-1")
            })
            .unwrap(),
            r#"{"state":"connected","ap":"ap-1"}"#
        );
        assert_eq!(
            serde_json::to_string(&ConnectionState::Failed {
                error: NetworkError::AuthenticationFailed
            })
            .unwrap(),
            r#"{"state":"failed","error":{"kind":"authentication-failed"}}"#
        );
        assert_eq!(
            serde_json::to_string(&Connectivity::Portal {
                url: Some("http://portal.example/login".into())
            })
            .unwrap(),
            r#"{"state":"portal","url":"http://portal.example/login"}"#
        );
        assert_eq!(
            serde_json::to_string(&Connectivity::Portal { url: None }).unwrap(),
            r#"{"state":"portal"}"#
        );
        assert_eq!(
            serde_json::to_string(&SecurityType::Wpa3Sae).unwrap(),
            r#""wpa3-sae""#
        );
        assert_eq!(
            serde_json::to_string(&NetworkError::Unauthorized {
                capability: "network-control".into()
            })
            .unwrap(),
            r#"{"kind":"unauthorized","capability":"network-control"}"#
        );
        assert_eq!(
            serde_json::to_string(&NetworkError::Unsupported {
                feature: "scan".into()
            })
            .unwrap(),
            r#"{"kind":"unsupported","feature":"scan"}"#
        );
        let event = NetworkEvent::RadioChanged {
            radio: RadioState::Enabled,
        };
        assert_eq!(
            serde_json::to_string(&event).unwrap(),
            r#"{"event":"radio-changed","radio":{"state":"enabled"}}"#
        );
        assert_eq!(
            serde_json::to_string(&NetworkEvent::IpChanged { ip: None }).unwrap(),
            r#"{"event":"ip-changed"}"#
        );
    }

    #[test]
    fn every_event_name_matches_the_adr() {
        let events = [
            NetworkEvent::AdapterChanged {
                adapter: AdapterPresence::Absent,
            },
            NetworkEvent::RadioChanged {
                radio: RadioState::Enabled,
            },
            NetworkEvent::ScanChanged {
                scan: ScanState::Idle,
            },
            NetworkEvent::AccessPointsChanged {
                access_points: vec![],
            },
            NetworkEvent::ConnectionChanged {
                connection: ConnectionState::Disconnected,
            },
            NetworkEvent::IpChanged { ip: None },
            NetworkEvent::ConnectivityChanged {
                connectivity: Connectivity::None,
            },
            NetworkEvent::ProfilesChanged { profiles: vec![] },
            NetworkEvent::ServiceRestarted {
                state: NetworkState::default(),
            },
        ];
        let names: Vec<&str> = events.iter().map(NetworkEvent::name).collect();
        assert_eq!(
            names,
            [
                "adapter-changed",
                "radio-changed",
                "scan-changed",
                "access-points-changed",
                "connection-changed",
                "ip-changed",
                "connectivity-changed",
                "profiles-changed",
                "service-restarted",
            ]
        );
        for event in &events {
            let wire = serde_json::to_value(event).unwrap();
            assert_eq!(wire["event"], event.name());
            assert_eq!(
                serde_json::from_value::<NetworkEvent>(wire).unwrap(),
                *event
            );
        }
    }

    #[test]
    fn events_fold_into_state_and_restart_replaces_it() {
        let mut state = NetworkState::default();
        assert_eq!(state.adapter, AdapterPresence::Absent);
        let deltas = [
            NetworkEvent::AdapterChanged {
                adapter: AdapterPresence::Present {
                    kind: AdapterKind::Wifi,
                },
            },
            NetworkEvent::RadioChanged {
                radio: RadioState::Enabled,
            },
            NetworkEvent::ScanChanged {
                scan: ScanState::Completed { count: 1 },
            },
            NetworkEvent::AccessPointsChanged {
                access_points: vec![ap("ap-1", "ferrix-lab")],
            },
            NetworkEvent::ConnectionChanged {
                connection: ConnectionState::Connected {
                    ap: ApId::new("ap-1"),
                },
            },
            NetworkEvent::IpChanged {
                ip: Some(IpConfig {
                    addresses: vec!["10.0.2.15/24".into()],
                    gateway: Some("10.0.2.2".into()),
                    dns: vec!["10.0.2.3".into()],
                    dhcp: true,
                }),
            },
            NetworkEvent::ConnectivityChanged {
                connectivity: Connectivity::Full,
            },
            NetworkEvent::ProfilesChanged {
                profiles: vec![SavedProfile {
                    id: ProfileId::new("profile-1"),
                    ssid: Ssid::from("ferrix-lab"),
                    security: SecurityType::Wpa2Psk,
                    autoconnect: true,
                }],
            },
        ];
        for delta in &deltas {
            delta.apply_to(&mut state);
        }
        assert!(state.adapter.is_present());
        assert!(state.radio.is_enabled());
        assert_eq!(state.scan, ScanState::Completed { count: 1 });
        assert_eq!(state.access_points.len(), 1);
        assert!(state.connection.is_connected());
        assert_eq!(
            state
                .access_point(&ApId::new("ap-1"))
                .map(|ap| ap.signal_percent),
            Some(78)
        );
        assert!(state.profile(&ProfileId::new("profile-1")).is_some());
        assert_eq!(state.connectivity, Connectivity::Full);
        assert!(state.ip.as_ref().is_some_and(|ip| ip.dhcp));

        let fresh = NetworkState::default();
        NetworkEvent::ServiceRestarted {
            state: fresh.clone(),
        }
        .apply_to(&mut state);
        assert_eq!(state, fresh);
    }

    #[test]
    fn access_points_never_expose_a_raw_bssid() {
        let ap = ap("ap-1", "ferrix-lab");
        let debug = format!("{ap:?}");
        let json = serde_json::to_string(&ap).unwrap();
        let summary = ap.summary().to_string();
        for text in [&debug, &json, &summary] {
            assert!(!text.to_lowercase().contains("aa:bb:cc:dd:ee:ff"), "{text}");
            assert!(!text.contains("\"bssid\":"), "{text}");
        }
        assert!(json.contains("\"bssid_hash\":"));
        assert_eq!(ap.bssid_hash.len(), HASH_PREFIX_LEN);
        // The summary is digest-only: no plain ssid either.
        assert!(!summary.contains("ferrix-lab"));
        assert!(summary.contains(&Ssid::from("ferrix-lab").hash()));
        // The wire form round-trips with the hash intact.
        assert_eq!(serde_json::from_str::<AccessPoint>(&json).unwrap(), ap);
    }

    #[test]
    fn summaries_carry_names_ids_and_digests_only() {
        let mut state = NetworkState::default();
        state.access_points.push(ap("ap-1", "ferrix-lab"));
        state.connection = ConnectionState::Connected {
            ap: ApId::new("ap-1"),
        };
        let summary = state.summary();
        assert_eq!(summary["connection"], "connected");
        assert_eq!(summary["ap"], "ap-1");
        assert_eq!(summary["access_points"][0]["ap"], "ap-1");
        assert!(!summary.to_string().contains("ferrix-lab"));

        let fields = NetworkEvent::ConnectionChanged {
            connection: ConnectionState::Failed {
                error: NetworkError::DhcpFailed,
            },
        }
        .summary();
        assert_eq!(fields[0], ("state", Value::from("failed")));
        assert_eq!(fields[1], ("error", Value::from("dhcp-failed")));
    }

    #[test]
    fn passphrases_never_debug_print() {
        let secret = Passphrase::new("mock-passphrase");
        assert_eq!(format!("{secret:?}"), "Passphrase(<redacted>)");
        assert_eq!(
            format!("{:?}", Some(secret.clone())),
            "Some(Passphrase(<redacted>))"
        );
        assert_eq!(
            serde_json::to_string(&secret).unwrap(),
            r#""mock-passphrase""#
        );
        assert_eq!(secret.expose(), "mock-passphrase");
    }

    #[test]
    fn errors_display_and_name() {
        let error = NetworkError::Unauthorized {
            capability: "network-control".into(),
        };
        assert_eq!(error.name(), "unauthorized");
        assert!(error.to_string().contains("network-control"));
        assert_eq!(NetworkError::TimedOut.name(), "timed-out");
        assert!(SecurityType::Wpa2Psk.needs_passphrase());
        assert!(!SecurityType::Open.needs_passphrase());
    }

    #[test]
    fn hidden_join_validation_names_the_rule_never_the_secret() {
        let name = Ssid::from("ferrix-hidden");
        let secret = Passphrase::new("mock-passphrase");
        assert_eq!(
            validate_hidden_join(&name, SecurityType::Wpa2Psk, Some(&secret)),
            Ok(())
        );
        assert_eq!(
            validate_hidden_join(&name, SecurityType::Wpa3Sae, Some(&Passphrase::new("x"))),
            Ok(())
        );
        assert_eq!(
            validate_hidden_join(&name, SecurityType::Open, None),
            Ok(())
        );
        let hex = Passphrase::new("ab".repeat(32));
        assert_eq!(
            validate_hidden_join(&name, SecurityType::Wpa2Psk, Some(&hex)),
            Ok(())
        );
        for (ssid, security, passphrase) in [
            (Ssid::from(""), SecurityType::Open, None),
            (Ssid::new(vec![0, 0]), SecurityType::Open, None),
            (Ssid::from("x".repeat(33)), SecurityType::Open, None),
            (name.clone(), SecurityType::Open, Some(&secret)),
            (name.clone(), SecurityType::Wpa2Psk, None),
            (
                name.clone(),
                SecurityType::Wpa3Sae,
                Some(&Passphrase::new("")),
            ),
            (
                name.clone(),
                SecurityType::Wpa2Psk,
                Some(&Passphrase::new("short")),
            ),
        ] {
            let error = validate_hidden_join(&ssid, security, passphrase).unwrap_err();
            assert_eq!(error.name(), "protocol", "{ssid:?} {security}");
            assert!(!error.to_string().contains("mock-passphrase"));
        }
        for security in [
            SecurityType::Wep,
            SecurityType::Enterprise,
            SecurityType::Unknown,
        ] {
            assert_eq!(
                validate_hidden_join(&name, security, Some(&secret))
                    .unwrap_err()
                    .name(),
                "unsupported"
            );
        }
    }

    #[test]
    fn state_snapshot_round_trips_and_carries_the_schema() {
        let state = NetworkState::default();
        let wire = serde_json::to_value(&state).unwrap();
        assert_eq!(wire["schema_version"], SCHEMA_VERSION);
        assert_eq!(wire["adapter"]["state"], "absent");
        assert_eq!(wire["radio"]["state"], "unavailable");
        assert!(wire.get("ip").is_none());
        assert_eq!(serde_json::from_value::<NetworkState>(wire).unwrap(), state);
    }
}
