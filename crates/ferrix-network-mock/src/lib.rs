//! Deterministic network backend for the QEMU guest and nested host runs.
//!
//! The mock owns a synthetic "world" of access points and a join state
//! machine driven by a deterministic clock: association, authentication,
//! and addressing each advance only when the host sends `tick`, never with
//! wall time. Scenario steps (ADR-006) arrive over the mock-control channel
//! as `cmd`-tagged JSON lines; every command is strictly typed and unknown
//! commands or fields are rejected.
//!
//! Hidden networks: an access point marked `hidden` is advertised with an
//! empty SSID, as a real hidden beacon is, so it can only be joined by name
//! (`join-hidden`). Once joined, its name is shown, as NetworkManager does
//! after association; replacing the world (`set-access-points`) hides it
//! again.
//!
//! Secrets: passphrases live in memory for one process lifetime, are
//! compared for authentication, and are never part of any ack, event, or
//! `Debug` output. Hardware addresses do not exist here at all: the
//! `bssid_hash` of a synthetic access point is a digest of its name.

use std::fmt;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, MutexGuard};

use ferrix_mock_control::{Ack, Handler};
use ferrix_network_adapter_api::{BackendError, BackendEvent, NetworkBackend};
use ferrix_network_api::{
    AccessPoint, AdapterKind, AdapterPresence, ApId, ConnectionState, Connectivity, IpConfig,
    NetworkError, NetworkEvent, NetworkState, Passphrase, ProfileId, RadioState, SavedProfile,
    ScanState, SecurityType, Ssid, hash_identifier,
};
use ferrix_target_profile::{
    Profile, ProfileAdapterPresence, ProfileRadioState, ProfileSecurityType,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Deterministic milliseconds each join phase (associating, authenticating,
/// configuring) takes on the mock clock.
pub const PHASE_MS: u64 = 500;
/// Deterministic milliseconds a scan takes on the mock clock.
pub const SCAN_MS: u64 = 1000;
/// Reported for every synthetic access point.
pub const DEFAULT_FREQUENCY_MHZ: u32 = 2437;

/// Synthetic fixture names. Nothing here is a real network.
pub const LAB_SSID: &str = "ferrix-lab";
pub const GUEST_SSID: &str = "ferrix-guest";
pub const LAB_PASSPHRASE: &str = "mock-passphrase";
/// The synthetic hidden network in the checked-in profile.
pub const HIDDEN_SSID: &str = "ferrix-hidden";

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// One synthetic access point in the world.
#[derive(Clone, PartialEq, Eq)]
pub struct MockAccessPoint {
    pub ssid: Ssid,
    pub security: SecurityType,
    pub signal_percent: u8,
    /// Secret needed to join when `security` needs one. Never printed.
    pub passphrase: Option<Passphrase>,
    /// Advertised without a name; joinable only with `join-hidden`.
    pub hidden: bool,
}

impl fmt::Debug for MockAccessPoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MockAccessPoint")
            .field("ssid", &self.ssid)
            .field("security", &self.security)
            .field("signal_percent", &self.signal_percent)
            .field(
                "passphrase",
                &self.passphrase.as_ref().map(|_| "<redacted>"),
            )
            .field("hidden", &self.hidden)
            .finish()
    }
}

impl MockAccessPoint {
    pub fn lab() -> Self {
        Self {
            ssid: Ssid::from(LAB_SSID),
            security: SecurityType::Wpa2Psk,
            signal_percent: 78,
            passphrase: Some(Passphrase::new(LAB_PASSPHRASE)),
            hidden: false,
        }
    }

    pub fn guest() -> Self {
        Self {
            ssid: Ssid::from(GUEST_SSID),
            security: SecurityType::Open,
            signal_percent: 54,
            passphrase: None,
            hidden: false,
        }
    }

    /// A WPA2 network that broadcasts no name.
    pub fn hidden() -> Self {
        Self {
            ssid: Ssid::from(HIDDEN_SSID),
            security: SecurityType::Wpa2Psk,
            signal_percent: 63,
            passphrase: Some(Passphrase::new(LAB_PASSPHRASE)),
            hidden: true,
        }
    }
}

/// Boot-time state of the mock.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MockNetworkConfig {
    pub adapter_present: bool,
    pub radio_enabled: bool,
    /// Name of the access point joined at boot, if any.
    pub connected: Option<Ssid>,
    pub access_points: Vec<MockAccessPoint>,
}

impl Default for MockNetworkConfig {
    fn default() -> Self {
        Self {
            adapter_present: true,
            radio_enabled: true,
            connected: Some(Ssid::from(LAB_SSID)),
            access_points: vec![MockAccessPoint::lab(), MockAccessPoint::guest()],
        }
    }
}

impl MockNetworkConfig {
    /// Derives the boot state from the versioned profile's `[network]`
    /// section (or the section the profile derives from `[connectivity]`
    /// when none is written). The section describes the mock Wi-Fi adapter
    /// only; a real `ethernet-like` transport is never described here.
    pub fn from_profile(profile: &Profile) -> Self {
        let section = profile.network_section();
        let access_points = section
            .access_points
            .iter()
            .map(|ap| MockAccessPoint {
                ssid: Ssid::from(ap.ssid.as_str()),
                security: match ap.security {
                    ProfileSecurityType::Open => SecurityType::Open,
                    ProfileSecurityType::Wep => SecurityType::Wep,
                    ProfileSecurityType::WpaPsk => SecurityType::WpaPsk,
                    ProfileSecurityType::Wpa2Psk => SecurityType::Wpa2Psk,
                    ProfileSecurityType::Wpa3Sae => SecurityType::Wpa3Sae,
                    ProfileSecurityType::Enterprise => SecurityType::Enterprise,
                },
                signal_percent: ap.signal_percent.min(100),
                passphrase: (!ap.passphrase.is_empty())
                    .then(|| Passphrase::new(ap.passphrase.as_str())),
                hidden: ap.hidden,
            })
            .collect();
        Self {
            adapter_present: section.adapter == ProfileAdapterPresence::Present,
            radio_enabled: section.radio == ProfileRadioState::Enabled,
            connected: (!section.connected.is_empty())
                .then(|| Ssid::from(section.connected.as_str())),
            access_points,
        }
    }
}

// ---------------------------------------------------------------------------
// Mock-control commands
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ScanOutcome {
    Completed,
    Failed,
}

/// Policy for the next join attempt. `connected` means "behave naturally":
/// the passphrase is checked and a match succeeds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConnectResult {
    #[default]
    Connected,
    AuthFailed,
    DhcpFailed,
    DnsFailed,
    Timeout,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConnectivityKind {
    None,
    Local,
    Portal,
    Limited,
    Full,
}

/// An access point as the host describes it.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccessPointSpec {
    pub ssid: String,
    pub security: SecurityType,
    pub signal_percent: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub passphrase: Option<Passphrase>,
    /// Advertise without a name (see the module docs).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
}

impl fmt::Debug for AccessPointSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AccessPointSpec")
            .field("ssid", &self.ssid)
            .field("security", &self.security)
            .field("signal_percent", &self.signal_percent)
            .field(
                "passphrase",
                &self.passphrase.as_ref().map(|_| "<redacted>"),
            )
            .field("hidden", &self.hidden)
            .finish()
    }
}

/// Host commands. Payload-free commands are empty struct variants so
/// `deny_unknown_fields` rejects stray keys on them as well.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "kebab-case", deny_unknown_fields)]
pub enum MockCommand {
    State {},
    SetAdapter {
        present: bool,
    },
    SetRadio {
        enabled: bool,
    },
    SetAccessPoints {
        access_points: Vec<AccessPointSpec>,
    },
    /// How the next scan ends when its time elapses on `tick`.
    ScanResult {
        outcome: ScanOutcome,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// Policy for the next join attempt.
    SetConnectResult {
        result: ConnectResult,
    },
    /// Connectivity after the next successful join, applied immediately
    /// when already connected.
    SetConnectivity {
        connectivity: ConnectivityKind,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        url: Option<String>,
    },
    /// The network drops the station.
    Disconnect {
        reason: String,
    },
    /// Addresses handed out on the next successful join, applied
    /// immediately when already connected.
    SetIp {
        addresses: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        gateway: Option<String>,
        dns: Vec<String>,
    },
    /// Advance the deterministic clock.
    Tick {
        ms: u64,
    },
    /// The backend process dies; the next `tick` brings it back.
    CrashBackend {},
}

impl MockCommand {
    pub fn name(&self) -> &'static str {
        match self {
            MockCommand::State {} => "state",
            MockCommand::SetAdapter { .. } => "set-adapter",
            MockCommand::SetRadio { .. } => "set-radio",
            MockCommand::SetAccessPoints { .. } => "set-access-points",
            MockCommand::ScanResult { .. } => "scan-result",
            MockCommand::SetConnectResult { .. } => "set-connect-result",
            MockCommand::SetConnectivity { .. } => "set-connectivity",
            MockCommand::Disconnect { .. } => "disconnect",
            MockCommand::SetIp { .. } => "set-ip",
            MockCommand::Tick { .. } => "tick",
            MockCommand::CrashBackend {} => "crash-backend",
        }
    }
}

// ---------------------------------------------------------------------------
// State machine
// ---------------------------------------------------------------------------

struct WorldAp {
    /// As advertised: `ap.ssid` is empty while the network is hidden.
    ap: AccessPoint,
    /// The network's real name, used for joins and profiles.
    name: Ssid,
    hidden: bool,
    passphrase: Option<Passphrase>,
}

struct StoredProfile {
    profile: SavedProfile,
    passphrase: Option<Passphrase>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Associating,
    Authenticating,
    Configuring,
}

struct Join {
    /// `None` when a join by name found no network of that name: the
    /// attempt fails with `not-found` once association times out.
    ap: Option<ApId>,
    phase: Phase,
    due_at: u64,
    secret_ok: bool,
    passphrase: Option<Passphrase>,
}

struct Inner {
    state: NetworkState,
    world: Vec<WorldAp>,
    profiles: Vec<StoredProfile>,
    next_ap: u32,
    next_profile: u32,
    clock_ms: u64,
    radio_wanted: bool,
    scan_due: Option<u64>,
    scan_outcome: (ScanOutcome, Option<String>),
    connect_result: ConnectResult,
    connectivity_policy: Connectivity,
    ip_policy: IpConfig,
    join: Option<Join>,
    crashed: bool,
}

fn default_ip() -> IpConfig {
    IpConfig {
        addresses: vec!["10.0.2.15/24".to_owned()],
        gateway: Some("10.0.2.2".to_owned()),
        dns: vec!["10.0.2.3".to_owned()],
        dhcp: true,
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Whether `given` is the secret `world` expects (no expected secret
/// accepts anything).
fn secret_matches(world: &WorldAp, given: Option<&Passphrase>) -> bool {
    match (&world.passphrase, given) {
        (Some(expected), Some(given)) => expected == given,
        (None, _) => true,
        (Some(_), None) => false,
    }
}

/// Whether a join chosen as `chosen` can complete against a network of
/// type `actual`: the same type, or WPA/WPA2 pre-shared key either way.
fn security_fits(chosen: SecurityType, actual: SecurityType) -> bool {
    let psk = |s| matches!(s, SecurityType::WpaPsk | SecurityType::Wpa2Psk);
    chosen == actual || (psk(chosen) && psk(actual))
}

/// The deterministic backend. Shared between the daemon (as a
/// [`NetworkBackend`]) and the mock-control channel (as a [`Handler`]).
pub struct MockNetwork {
    inner: Mutex<Inner>,
    sinks: Mutex<Vec<Sender<BackendEvent>>>,
}

impl MockNetwork {
    pub fn new(config: MockNetworkConfig) -> Arc<Self> {
        let mut inner = Inner {
            state: NetworkState::default(),
            world: Vec::new(),
            profiles: Vec::new(),
            next_ap: 1,
            next_profile: 1,
            clock_ms: 0,
            radio_wanted: config.radio_enabled,
            scan_due: None,
            scan_outcome: (ScanOutcome::Completed, None),
            connect_result: ConnectResult::Connected,
            connectivity_policy: Connectivity::Full,
            ip_policy: default_ip(),
            join: None,
            crashed: false,
        };
        inner.set_world(&config.access_points);
        if config.adapter_present {
            inner.state.adapter = AdapterPresence::Present {
                kind: AdapterKind::Wifi,
            };
            if config.radio_enabled {
                inner.state.radio = RadioState::Enabled;
                inner.state.access_points = inner.visible();
                if let Some(ssid) = &config.connected
                    && let Some(world) = inner.world.iter().find(|w| &w.name == ssid)
                {
                    let id = world.ap.id.clone();
                    let passphrase = world.passphrase.clone();
                    let _ = inner.complete_join(&id, passphrase);
                }
            } else {
                inner.state.radio = RadioState::Disabled;
            }
        }
        Arc::new(Self {
            inner: Mutex::new(inner),
            sinks: Mutex::new(Vec::new()),
        })
    }

    /// Current deterministic clock in milliseconds.
    pub fn clock_ms(&self) -> u64 {
        lock(&self.inner).clock_ms
    }

    /// Applies one typed command: returns the events it produced, in order.
    pub fn apply(&self, command: &MockCommand) -> Result<Vec<NetworkEvent>, String> {
        let events = {
            let mut inner = lock(&self.inner);
            match command {
                MockCommand::State {} => Vec::new(),
                MockCommand::SetAdapter { present } => inner.set_adapter(*present),
                MockCommand::SetRadio { enabled } => inner.set_radio(*enabled),
                MockCommand::SetAccessPoints { access_points } => {
                    let aps: Vec<MockAccessPoint> = access_points
                        .iter()
                        .map(|spec| MockAccessPoint {
                            ssid: Ssid::from(spec.ssid.as_str()),
                            security: spec.security,
                            signal_percent: spec.signal_percent.min(100),
                            passphrase: spec.passphrase.clone(),
                            hidden: spec.hidden,
                        })
                        .collect();
                    inner.set_world(&aps);
                    inner.refresh_visible()
                }
                MockCommand::ScanResult { outcome, reason } => {
                    inner.scan_outcome = (*outcome, reason.clone());
                    Vec::new()
                }
                MockCommand::SetConnectResult { result } => {
                    inner.connect_result = *result;
                    Vec::new()
                }
                MockCommand::SetConnectivity { connectivity, url } => {
                    inner.connectivity_policy = match connectivity {
                        ConnectivityKind::None => Connectivity::None,
                        ConnectivityKind::Local => Connectivity::Local,
                        ConnectivityKind::Portal => Connectivity::Portal { url: url.clone() },
                        ConnectivityKind::Limited => Connectivity::Limited,
                        ConnectivityKind::Full => Connectivity::Full,
                    };
                    if inner.state.connection.is_connected() {
                        let connectivity = inner.connectivity_policy.clone();
                        inner.set_connectivity(connectivity)
                    } else {
                        Vec::new()
                    }
                }
                MockCommand::Disconnect { reason } => inner.drop_link(ConnectionState::Failed {
                    error: NetworkError::Backend {
                        reason: reason.clone(),
                    },
                }),
                MockCommand::SetIp {
                    addresses,
                    gateway,
                    dns,
                } => {
                    inner.ip_policy = IpConfig {
                        addresses: addresses.clone(),
                        gateway: gateway.clone(),
                        dns: dns.clone(),
                        dhcp: false,
                    };
                    if inner.state.connection.is_connected() {
                        let ip = inner.ip_policy.clone();
                        inner.set_ip(Some(ip))
                    } else {
                        Vec::new()
                    }
                }
                MockCommand::Tick { ms } => {
                    if inner.crashed {
                        inner.crashed = false;
                        let state = inner.state.clone();
                        drop(inner);
                        self.broadcast(BackendEvent::Restarted { state });
                        let mut inner = lock(&self.inner);
                        let events = inner.tick(*ms);
                        drop(inner);
                        self.broadcast_all(&events);
                        return Ok(events);
                    }
                    inner.tick(*ms)
                }
                MockCommand::CrashBackend {} => {
                    inner.crashed = true;
                    inner.join = None;
                    inner.scan_due = None;
                    drop(inner);
                    self.broadcast(BackendEvent::Lost {
                        reason: "mock backend crashed".to_owned(),
                    });
                    return Ok(Vec::new());
                }
            }
        };
        self.broadcast_all(&events);
        Ok(events)
    }

    fn broadcast_all(&self, events: &[NetworkEvent]) {
        for event in events {
            self.broadcast(BackendEvent::Changed(event.clone()));
        }
    }

    /// Deltas are dropped while the backend is down; `Lost` and `Restarted`
    /// themselves always go out.
    fn broadcast(&self, event: BackendEvent) {
        if matches!(event, BackendEvent::Changed(_)) && lock(&self.inner).crashed {
            return;
        }
        lock(&self.sinks).retain(|sink| sink.send(event.clone()).is_ok());
    }

    /// Evidence-safe snapshot plus the mock clock.
    pub fn summary(&self) -> Value {
        let inner = lock(&self.inner);
        let mut summary = inner.state.summary();
        if let Value::Object(map) = &mut summary {
            map.insert("clock_ms".to_owned(), Value::from(inner.clock_ms));
            map.insert(
                "backend".to_owned(),
                Value::from(if inner.crashed { "crashed" } else { "running" }),
            );
        }
        summary
    }

    fn guard(&self, inner: &Inner) -> Result<(), BackendError> {
        if inner.crashed {
            return Err(BackendError::unavailable("mock backend crashed"));
        }
        if !inner.state.adapter.is_present() {
            return Err(NetworkError::NoAdapter.into());
        }
        Ok(())
    }
}

impl Inner {
    fn visible(&self) -> Vec<AccessPoint> {
        self.world.iter().map(|w| w.ap.clone()).collect()
    }

    /// Replaces the world, keeping ids stable for (ssid, security) pairs.
    fn set_world(&mut self, aps: &[MockAccessPoint]) {
        let mut world = Vec::with_capacity(aps.len());
        for spec in aps {
            let existing = self
                .world
                .iter()
                .find(|w| w.name == spec.ssid && w.ap.security == spec.security)
                .map(|w| w.ap.id.clone());
            let id = existing.unwrap_or_else(|| {
                let id = ApId::new(format!("ap-{}", self.next_ap));
                self.next_ap += 1;
                id
            });
            let mut seed = b"mock-bssid:".to_vec();
            seed.extend_from_slice(spec.ssid.as_bytes());
            world.push(WorldAp {
                ap: AccessPoint {
                    id,
                    ssid: if spec.hidden {
                        Ssid::new(Vec::new())
                    } else {
                        spec.ssid.clone()
                    },
                    bssid_hash: hash_identifier(&seed),
                    signal_percent: spec.signal_percent,
                    security: spec.security,
                    frequency_mhz: Some(DEFAULT_FREQUENCY_MHZ),
                },
                name: spec.ssid.clone(),
                hidden: spec.hidden,
                passphrase: spec.passphrase.clone(),
            });
        }
        self.world = world;
    }

    fn radio_on(&self) -> bool {
        self.state.adapter.is_present() && self.state.radio.is_enabled()
    }

    fn refresh_visible(&mut self) -> Vec<NetworkEvent> {
        if !self.radio_on() {
            return Vec::new();
        }
        let visible = self.visible();
        if visible == self.state.access_points {
            return Vec::new();
        }
        self.state.access_points = visible.clone();
        vec![NetworkEvent::AccessPointsChanged {
            access_points: visible,
        }]
    }

    fn set_connection(&mut self, connection: ConnectionState) -> Vec<NetworkEvent> {
        if self.state.connection == connection {
            return Vec::new();
        }
        self.state.connection = connection.clone();
        vec![NetworkEvent::ConnectionChanged { connection }]
    }

    fn set_ip(&mut self, ip: Option<IpConfig>) -> Vec<NetworkEvent> {
        if self.state.ip == ip {
            return Vec::new();
        }
        self.state.ip = ip.clone();
        vec![NetworkEvent::IpChanged { ip }]
    }

    fn set_connectivity(&mut self, connectivity: Connectivity) -> Vec<NetworkEvent> {
        if self.state.connectivity == connectivity {
            return Vec::new();
        }
        self.state.connectivity = connectivity.clone();
        vec![NetworkEvent::ConnectivityChanged { connectivity }]
    }

    /// Ends any link or join and reports `connection`.
    fn drop_link(&mut self, connection: ConnectionState) -> Vec<NetworkEvent> {
        self.join = None;
        let mut events = self.set_connection(connection);
        events.extend(self.set_ip(None));
        events.extend(self.set_connectivity(Connectivity::None));
        events
    }

    fn set_adapter(&mut self, present: bool) -> Vec<NetworkEvent> {
        let mut events = Vec::new();
        if present == self.state.adapter.is_present() {
            return events;
        }
        if present {
            self.state.adapter = AdapterPresence::Present {
                kind: AdapterKind::Wifi,
            };
            events.push(NetworkEvent::AdapterChanged {
                adapter: self.state.adapter,
            });
            self.state.radio = if self.radio_wanted {
                RadioState::Enabled
            } else {
                RadioState::Disabled
            };
            events.push(NetworkEvent::RadioChanged {
                radio: self.state.radio.clone(),
            });
            events.extend(self.refresh_visible());
        } else {
            self.state.adapter = AdapterPresence::Absent;
            events.push(NetworkEvent::AdapterChanged {
                adapter: AdapterPresence::Absent,
            });
            self.state.radio = RadioState::Unavailable {
                reason: "no adapter".to_owned(),
            };
            events.push(NetworkEvent::RadioChanged {
                radio: self.state.radio.clone(),
            });
            events.extend(self.clear_air());
            events.extend(self.drop_link(ConnectionState::Disconnected));
        }
        events
    }

    /// Radio off or adapter gone: no scan, no visible access points.
    fn clear_air(&mut self) -> Vec<NetworkEvent> {
        let mut events = Vec::new();
        self.scan_due = None;
        if self.state.scan != ScanState::Idle {
            self.state.scan = ScanState::Idle;
            events.push(NetworkEvent::ScanChanged {
                scan: ScanState::Idle,
            });
        }
        if !self.state.access_points.is_empty() {
            self.state.access_points.clear();
            events.push(NetworkEvent::AccessPointsChanged {
                access_points: Vec::new(),
            });
        }
        events
    }

    fn set_radio(&mut self, enabled: bool) -> Vec<NetworkEvent> {
        self.radio_wanted = enabled;
        if !self.state.adapter.is_present() {
            return Vec::new();
        }
        let mut events = Vec::new();
        if enabled {
            if !self.state.radio.is_enabled() {
                self.state.radio = RadioState::Enabled;
                events.push(NetworkEvent::RadioChanged {
                    radio: RadioState::Enabled,
                });
            }
            events.extend(self.refresh_visible());
        } else {
            if self.state.radio != RadioState::Disabled {
                self.state.radio = RadioState::Disabled;
                events.push(NetworkEvent::RadioChanged {
                    radio: RadioState::Disabled,
                });
            }
            events.extend(self.clear_air());
            events.extend(self.drop_link(ConnectionState::Disconnected));
        }
        events
    }

    fn start_scan(&mut self) -> Vec<NetworkEvent> {
        if self.scan_due.is_some() {
            return Vec::new();
        }
        self.scan_due = Some(self.clock_ms + SCAN_MS);
        self.state.scan = ScanState::Scanning;
        vec![NetworkEvent::ScanChanged {
            scan: ScanState::Scanning,
        }]
    }

    fn finish_scan(&mut self) -> Vec<NetworkEvent> {
        self.scan_due = None;
        let (outcome, reason) =
            std::mem::replace(&mut self.scan_outcome, (ScanOutcome::Completed, None));
        let mut events = Vec::new();
        match outcome {
            ScanOutcome::Completed => {
                let visible = self.visible();
                self.state.scan = ScanState::Completed {
                    count: visible.len(),
                };
                events.push(NetworkEvent::ScanChanged {
                    scan: self.state.scan.clone(),
                });
                self.state.access_points = visible.clone();
                events.push(NetworkEvent::AccessPointsChanged {
                    access_points: visible,
                });
            }
            ScanOutcome::Failed => {
                self.state.scan = ScanState::Failed {
                    reason: reason.unwrap_or_else(|| "scan failed".to_owned()),
                };
                events.push(NetworkEvent::ScanChanged {
                    scan: self.state.scan.clone(),
                });
            }
        }
        events
    }

    fn start_join(&mut self, ap: &ApId, passphrase: Option<Passphrase>) -> Vec<NetworkEvent> {
        let (secret_ok, effective) = {
            let world = self
                .world
                .iter()
                .find(|w| &w.ap.id == ap)
                .expect("caller checked visibility");
            if !world.ap.security.needs_passphrase() {
                (true, None)
            } else {
                let supplied = passphrase.or_else(|| {
                    self.profiles
                        .iter()
                        .find(|p| p.profile.ssid == world.name)
                        .and_then(|p| p.passphrase.clone())
                });
                (secret_matches(world, supplied.as_ref()), supplied)
            }
        };
        self.begin_join(Some(ap.clone()), secret_ok, effective)
    }

    /// A join by name: the network need not be visible, and the caller's
    /// `security` must fit the network's (as a supplicant would fail the
    /// handshake otherwise). No network of that name fails with
    /// `not-found` after the association phase.
    fn start_hidden_join(
        &mut self,
        ssid: &Ssid,
        security: SecurityType,
        passphrase: Option<Passphrase>,
    ) -> Vec<NetworkEvent> {
        let Some(world) = self.world.iter().find(|w| &w.name == ssid) else {
            return self.begin_join(None, false, None);
        };
        let fits = security_fits(security, world.ap.security);
        let secret_ok = fits
            && (!world.ap.security.needs_passphrase()
                || secret_matches(world, passphrase.as_ref()));
        let id = world.ap.id.clone();
        self.begin_join(Some(id), secret_ok, passphrase)
    }

    fn begin_join(
        &mut self,
        ap: Option<ApId>,
        secret_ok: bool,
        effective: Option<Passphrase>,
    ) -> Vec<NetworkEvent> {
        let mut events = self.drop_link(ConnectionState::Associating);
        if events.is_empty() {
            // Re-joining from the same state still restarts the machine.
            events.push(NetworkEvent::ConnectionChanged {
                connection: ConnectionState::Associating,
            });
        }
        self.join = Some(Join {
            ap,
            phase: Phase::Associating,
            due_at: self.clock_ms + PHASE_MS,
            secret_ok,
            passphrase: effective,
        });
        events
    }

    /// Joins immediately (boot state) without a phase walk. A hidden
    /// network's name is shown from now on.
    fn complete_join(&mut self, ap: &ApId, passphrase: Option<Passphrase>) -> Vec<NetworkEvent> {
        let Some(world) = self.world.iter_mut().find(|w| &w.ap.id == ap) else {
            return Vec::new();
        };
        let (ssid, security) = (world.name.clone(), world.ap.security);
        let mut events = Vec::new();
        if world.hidden && world.ap.ssid != world.name {
            world.ap.ssid = world.name.clone();
            events.extend(self.refresh_visible());
        }
        events.extend(self.set_connection(ConnectionState::Connected { ap: ap.clone() }));
        let ip = self.ip_policy.clone();
        events.extend(self.set_ip(Some(ip)));
        let connectivity = self.connectivity_policy.clone();
        events.extend(self.set_connectivity(connectivity));
        events.extend(self.save_profile(ssid, security, passphrase));
        events
    }

    fn save_profile(
        &mut self,
        ssid: Ssid,
        security: SecurityType,
        passphrase: Option<Passphrase>,
    ) -> Vec<NetworkEvent> {
        if let Some(existing) = self.profiles.iter_mut().find(|p| p.profile.ssid == ssid) {
            if passphrase.is_some() {
                existing.passphrase = passphrase;
            }
            return Vec::new();
        }
        let id = ProfileId::new(format!("profile-{}", self.next_profile));
        self.next_profile += 1;
        self.profiles.push(StoredProfile {
            profile: SavedProfile {
                id,
                ssid,
                security,
                autoconnect: true,
            },
            passphrase,
        });
        self.state.profiles = self.profiles.iter().map(|p| p.profile.clone()).collect();
        vec![NetworkEvent::ProfilesChanged {
            profiles: self.state.profiles.clone(),
        }]
    }

    fn tick(&mut self, ms: u64) -> Vec<NetworkEvent> {
        self.clock_ms = self.clock_ms.saturating_add(ms);
        let mut events = Vec::new();
        if self.scan_due.is_some_and(|due| due <= self.clock_ms) {
            events.extend(self.finish_scan());
        }
        while let Some(join) = self.join.as_ref()
            && join.due_at <= self.clock_ms
        {
            events.extend(self.advance_join());
        }
        events
    }

    fn advance_join(&mut self) -> Vec<NetworkEvent> {
        let Some(join) = self.join.as_mut() else {
            return Vec::new();
        };
        let policy = self.connect_result;
        let fail = |this: &mut Inner, error: NetworkError| {
            this.connect_result = ConnectResult::Connected;
            this.drop_link(ConnectionState::Failed { error })
        };
        match join.phase {
            Phase::Associating => {
                if policy == ConnectResult::Timeout {
                    return fail(self, NetworkError::TimedOut);
                }
                if join.ap.is_none() {
                    // NetworkManager's SSID_NOT_FOUND maps the same way.
                    return fail(
                        self,
                        NetworkError::NotFound {
                            id: "ssid".to_owned(),
                        },
                    );
                }
                join.phase = Phase::Authenticating;
                join.due_at += PHASE_MS;
                self.set_connection(ConnectionState::Authenticating)
            }
            Phase::Authenticating => {
                if policy == ConnectResult::AuthFailed || !join.secret_ok {
                    return fail(self, NetworkError::AuthenticationFailed);
                }
                join.phase = Phase::Configuring;
                join.due_at += PHASE_MS;
                self.set_connection(ConnectionState::Configuring)
            }
            Phase::Configuring => match policy {
                ConnectResult::DhcpFailed => fail(self, NetworkError::DhcpFailed),
                ConnectResult::DnsFailed => fail(self, NetworkError::DnsFailed),
                _ => {
                    let ap = join.ap.clone();
                    let passphrase = join.passphrase.take();
                    self.join = None;
                    self.connect_result = ConnectResult::Connected;
                    match ap {
                        Some(ap) => self.complete_join(&ap, passphrase),
                        None => Vec::new(),
                    }
                }
            },
        }
    }
}

impl NetworkBackend for MockNetwork {
    fn name(&self) -> &'static str {
        "mock"
    }

    fn snapshot(&self) -> Result<NetworkState, BackendError> {
        let inner = lock(&self.inner);
        if inner.crashed {
            return Err(BackendError::unavailable("mock backend crashed"));
        }
        Ok(inner.state.clone())
    }

    fn set_radio(&self, enabled: bool) -> Result<(), BackendError> {
        let events = {
            let mut inner = lock(&self.inner);
            self.guard(&inner)?;
            inner.set_radio(enabled)
        };
        self.broadcast_all(&events);
        Ok(())
    }

    fn scan(&self) -> Result<(), BackendError> {
        let events = {
            let mut inner = lock(&self.inner);
            self.guard(&inner)?;
            if !inner.state.radio.is_enabled() {
                return Err(NetworkError::RadioDisabled.into());
            }
            inner.start_scan()
        };
        self.broadcast_all(&events);
        Ok(())
    }

    fn connect(&self, ap: &ApId, passphrase: Option<&str>) -> Result<(), BackendError> {
        let events = {
            let mut inner = lock(&self.inner);
            self.guard(&inner)?;
            if !inner.state.radio.is_enabled() {
                return Err(NetworkError::RadioDisabled.into());
            }
            if inner.state.access_point(ap).is_none() || !inner.world.iter().any(|w| &w.ap.id == ap)
            {
                return Err(NetworkError::NotFound {
                    id: ap.as_str().to_owned(),
                }
                .into());
            }
            // A beacon without a name cannot be joined as such; the name
            // has to come from the user (`join-hidden`).
            if inner
                .world
                .iter()
                .any(|w| &w.ap.id == ap && w.ap.ssid.as_bytes().is_empty())
            {
                return Err(BackendError::unsupported(
                    "joining a hidden network without its name",
                ));
            }
            inner.start_join(ap, passphrase.map(Passphrase::new))
        };
        self.broadcast_all(&events);
        Ok(())
    }

    fn join_hidden(
        &self,
        ssid: &Ssid,
        security: SecurityType,
        passphrase: Option<&str>,
    ) -> Result<(), BackendError> {
        let events = {
            let mut inner = lock(&self.inner);
            self.guard(&inner)?;
            if !inner.state.radio.is_enabled() {
                return Err(NetworkError::RadioDisabled.into());
            }
            inner.start_hidden_join(ssid, security, passphrase.map(Passphrase::new))
        };
        self.broadcast_all(&events);
        Ok(())
    }

    fn connect_profile(&self, profile: &ProfileId) -> Result<(), BackendError> {
        let events = {
            let mut inner = lock(&self.inner);
            self.guard(&inner)?;
            if !inner.state.radio.is_enabled() {
                return Err(NetworkError::RadioDisabled.into());
            }
            let not_found = || NetworkError::NotFound {
                id: profile.as_str().to_owned(),
            };
            let stored = inner
                .profiles
                .iter()
                .find(|p| &p.profile.id == profile)
                .ok_or_else(not_found)?;
            let (ssid, security, passphrase) = (
                stored.profile.ssid.clone(),
                stored.profile.security,
                stored.passphrase.clone(),
            );
            // By name, as a saved hidden profile is joined without a
            // visible name; the radio is on, so the world is on the air.
            let ap = inner
                .world
                .iter()
                .find(|w| w.name == ssid && w.ap.security == security)
                .map(|w| w.ap.id.clone())
                .ok_or_else(not_found)?;
            inner.start_join(&ap, passphrase)
        };
        self.broadcast_all(&events);
        Ok(())
    }

    fn disconnect(&self) -> Result<(), BackendError> {
        let events = {
            let mut inner = lock(&self.inner);
            self.guard(&inner)?;
            inner.drop_link(ConnectionState::Disconnected)
        };
        self.broadcast_all(&events);
        Ok(())
    }

    fn forget(&self, profile: &ProfileId) -> Result<(), BackendError> {
        let events = {
            let mut inner = lock(&self.inner);
            if inner.crashed {
                return Err(BackendError::unavailable("mock backend crashed"));
            }
            let index = inner
                .profiles
                .iter()
                .position(|p| &p.profile.id == profile)
                .ok_or_else(|| NetworkError::NotFound {
                    id: profile.as_str().to_owned(),
                })?;
            let removed = inner.profiles.remove(index);
            inner.state.profiles = inner.profiles.iter().map(|p| p.profile.clone()).collect();
            let mut events = vec![NetworkEvent::ProfilesChanged {
                profiles: inner.state.profiles.clone(),
            }];
            let linked = inner
                .state
                .connection
                .connected_ap()
                .cloned()
                .or_else(|| inner.join.as_ref().and_then(|j| j.ap.clone()))
                .and_then(|id| inner.world.iter().find(|w| w.ap.id == id))
                .is_some_and(|w| w.name == removed.profile.ssid);
            if linked {
                events.extend(inner.drop_link(ConnectionState::Disconnected));
            }
            events
        };
        self.broadcast_all(&events);
        Ok(())
    }

    fn subscribe(&self, sink: Sender<BackendEvent>) {
        lock(&self.sinks).push(sink);
    }

    fn barrier(&self, token: u64) -> Result<(), BackendError> {
        self.broadcast(BackendEvent::Barrier { token });
        Ok(())
    }
}

impl Handler for MockNetwork {
    fn handle(&self, line: &str) -> Ack {
        let command: MockCommand = match serde_json::from_str(line) {
            Ok(command) => command,
            Err(error) => {
                let name = serde_json::from_str::<Value>(line)
                    .ok()
                    .and_then(|v| v.get("cmd").and_then(Value::as_str).map(str::to_owned));
                return Ack::error(
                    name.as_deref(),
                    format!("invalid network mock command: {error}"),
                );
            }
        };
        match self.apply(&command) {
            Ok(events) => Ack::ok(
                command.name(),
                events.iter().map(|e| e.name().to_owned()).collect(),
                self.summary(),
            ),
            Err(reason) => Ack::error(Some(command.name()), reason),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn mock(config: MockNetworkConfig) -> (Arc<MockNetwork>, mpsc::Receiver<BackendEvent>) {
        let mock = MockNetwork::new(config);
        let (tx, rx) = mpsc::channel();
        mock.subscribe(tx);
        (mock, rx)
    }

    fn drain(rx: &mpsc::Receiver<BackendEvent>) -> Vec<BackendEvent> {
        rx.try_iter().collect()
    }

    fn names(events: &[BackendEvent]) -> Vec<&'static str> {
        events.iter().map(BackendEvent::name).collect()
    }

    fn cmd(text: &str) -> MockCommand {
        serde_json::from_str(text).unwrap()
    }

    fn lab_id(mock: &MockNetwork) -> ApId {
        mock.snapshot()
            .unwrap()
            .access_points
            .iter()
            .find(|ap| ap.ssid == Ssid::from(LAB_SSID))
            .map(|ap| ap.id.clone())
            .expect("ferrix-lab visible")
    }

    fn disconnected() -> MockNetworkConfig {
        MockNetworkConfig {
            connected: None,
            ..MockNetworkConfig::default()
        }
    }

    #[test]
    fn default_boot_is_connected_to_the_lab() {
        let (mock, _rx) = mock(MockNetworkConfig::default());
        let state = mock.snapshot().unwrap();
        assert_eq!(
            state.adapter,
            AdapterPresence::Present {
                kind: AdapterKind::Wifi
            }
        );
        assert!(state.radio.is_enabled());
        assert_eq!(state.access_points.len(), 2);
        let lab = lab_id(&mock);
        assert_eq!(state.connection, ConnectionState::Connected { ap: lab });
        assert_eq!(state.connectivity, Connectivity::Full);
        assert!(state.ip.as_ref().is_some_and(|ip| ip.dhcp));
        assert_eq!(state.profiles.len(), 1);
        assert_eq!(state.profiles[0].ssid, Ssid::from(LAB_SSID));
        // Synthetic ids and digests only.
        let text = serde_json::to_string(&state).unwrap();
        assert!(!text.contains("mock-passphrase"));
        assert!(!text.contains("\"bssid\":"));
    }

    #[test]
    fn profile_network_section_maps_to_boot_state() {
        let base = r#"
schema_version = 1
name = "emulator-dev"
[display]
width = 1080
height = 2340
scale = 1.0
orientation = "portrait"
[input]
touch = true
rotation = true
[power]
battery_percent = 75
charging = false
[connectivity]
wifi = "connected"
bluetooth_present = true
modem = "registered"
sim = "present"
[audio]
route = "speaker"
"#;
        // No [network] section: the profile derives one from [connectivity].
        let connected = Profile::parse(base).unwrap();
        let config = MockNetworkConfig::from_profile(&connected);
        assert!(config.adapter_present && config.radio_enabled);
        assert!(config.connected.is_some());
        assert!(!config.access_points.is_empty());
        let disabled =
            Profile::parse(&base.replace("wifi = \"connected\"", "wifi = \"disabled\"")).unwrap();
        let config = MockNetworkConfig::from_profile(&disabled);
        assert!(!config.radio_enabled);
        assert_eq!(config.connected, None);

        // Explicit section.
        let explicit = format!(
            "{base}\n[network]\nadapter = \"present\"\nradio = \"enabled\"\nconnected = \"\"\n\n[[network.access_points]]\nssid = \"ferrix-cafe\"\nsecurity = \"wpa3-sae\"\nsignal_percent = 200\npassphrase = \"mock-passphrase\"\n\n[[network.access_points]]\nssid = \"ferrix-guest\"\nsecurity = \"open\"\nsignal_percent = 41\n"
        );
        match Profile::parse(&explicit) {
            Ok(profile) => {
                let config = MockNetworkConfig::from_profile(&profile);
                assert_eq!(config.connected, None);
                assert_eq!(config.access_points.len(), 2);
                assert_eq!(config.access_points[0].security, SecurityType::Wpa3Sae);
                assert!(config.access_points[0].passphrase.is_some());
                assert_eq!(config.access_points[1].passphrase, None);
                let (mock, _rx) = mock(config);
                let state = mock.snapshot().unwrap();
                assert_eq!(state.connection, ConnectionState::Disconnected);
                assert_eq!(state.access_points.len(), 2);
            }
            // The parser may reject signal_percent > 100; that is its call.
            Err(_) => {
                let valid = explicit.replace("signal_percent = 200", "signal_percent = 90");
                let profile = Profile::parse(&valid).unwrap();
                let config = MockNetworkConfig::from_profile(&profile);
                assert_eq!(config.access_points.len(), 2);
            }
        }

        // The checked-in profile boots connected to ferrix-lab.
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../targets/qemu-aarch64/profile.toml"
        );
        let profile = Profile::load(path).expect("checked-in profile parses");
        let config = MockNetworkConfig::from_profile(&profile);
        assert_eq!(config.connected, Some(Ssid::from(LAB_SSID)));
        assert!(
            config
                .access_points
                .iter()
                .any(|ap| ap.hidden && ap.ssid == Ssid::from(HIDDEN_SSID))
        );
        let (mock, _rx) = mock(config);
        let state = mock.snapshot().unwrap();
        assert!(state.connection.is_connected());
        assert_eq!(state.access_points.len(), 3);
        assert_eq!(
            state
                .access_points
                .iter()
                .filter(|ap| ap.ssid.as_bytes().is_empty())
                .count(),
            1,
            "the hidden network is listed without its name"
        );
    }

    fn with_hidden() -> MockNetworkConfig {
        MockNetworkConfig {
            connected: None,
            access_points: vec![
                MockAccessPoint::lab(),
                MockAccessPoint::guest(),
                MockAccessPoint::hidden(),
            ],
            ..MockNetworkConfig::default()
        }
    }

    fn hidden_name() -> Ssid {
        Ssid::from(HIDDEN_SSID)
    }

    #[test]
    fn a_hidden_network_joins_only_by_name_and_is_named_once_joined() {
        let (mock, rx) = mock(with_hidden());
        let state = mock.snapshot().unwrap();
        let hidden = state
            .access_points
            .iter()
            .find(|ap| ap.ssid.as_bytes().is_empty())
            .expect("hidden beacon listed")
            .clone();
        assert_eq!(hidden.security, SecurityType::Wpa2Psk);
        // Its beacon has no name, so it cannot be joined as such.
        assert_eq!(
            mock.connect(&hidden.id, Some(LAB_PASSPHRASE)),
            Err(BackendError::unsupported(
                "joining a hidden network without its name"
            ))
        );
        mock.join_hidden(&hidden_name(), SecurityType::Wpa2Psk, Some(LAB_PASSPHRASE))
            .unwrap();
        assert_eq!(names(&drain(&rx)), ["connection-changed"]);
        let events = mock.apply(&cmd(r#"{"cmd":"tick","ms":1500}"#)).unwrap();
        assert_eq!(
            events.iter().map(NetworkEvent::name).collect::<Vec<_>>(),
            [
                "connection-changed",
                "connection-changed",
                "access-points-changed",
                "connection-changed",
                "ip-changed",
                "connectivity-changed",
                "profiles-changed"
            ]
        );
        let state = mock.snapshot().unwrap();
        assert_eq!(
            state.connection,
            ConnectionState::Connected {
                ap: hidden.id.clone()
            }
        );
        assert_eq!(state.access_point(&hidden.id).unwrap().ssid, hidden_name());
        assert_eq!(state.profiles.len(), 1);
        assert_eq!(state.profiles[0].ssid, hidden_name());
        assert_eq!(state.profiles[0].security, SecurityType::Wpa2Psk);
        // The saved profile rejoins without the name being visible again.
        mock.apply(&cmd(
            r#"{"cmd":"set-access-points","access_points":[{"ssid":"ferrix-hidden","security":"wpa2-psk","signal_percent":63,"passphrase":"mock-passphrase","hidden":true}]}"#,
        ))
        .unwrap();
        assert!(
            mock.snapshot().unwrap().access_points[0]
                .ssid
                .as_bytes()
                .is_empty()
        );
        mock.disconnect().unwrap();
        mock.connect_profile(&state.profiles[0].id).unwrap();
        mock.apply(&cmd(r#"{"cmd":"tick","ms":1500}"#)).unwrap();
        assert!(mock.snapshot().unwrap().connection.is_connected());
        let text = format!("{:?}", MockAccessPoint::hidden());
        assert!(!text.contains(LAB_PASSPHRASE) && text.contains("hidden: true"));
    }

    #[test]
    fn hidden_joins_fail_on_a_wrong_secret_wrong_security_or_unknown_name() {
        let (mock, _rx) = mock(with_hidden());
        let failed = |mock: &MockNetwork| mock.snapshot().unwrap().connection;
        for (security, secret) in [
            (SecurityType::Wpa2Psk, Some("wrong-passphrase")),
            (SecurityType::Wpa2Psk, None),
            (SecurityType::Wpa3Sae, Some(LAB_PASSPHRASE)),
            (SecurityType::Open, None),
        ] {
            mock.join_hidden(&hidden_name(), security, secret).unwrap();
            mock.apply(&cmd(r#"{"cmd":"tick","ms":1000}"#)).unwrap();
            assert_eq!(
                failed(&mock),
                ConnectionState::Failed {
                    error: NetworkError::AuthenticationFailed
                },
                "{security} {}",
                secret.is_some()
            );
        }
        assert!(mock.snapshot().unwrap().profiles.is_empty());
        // WPA (1) and WPA2 share the pre-shared key handshake.
        mock.join_hidden(&hidden_name(), SecurityType::WpaPsk, Some(LAB_PASSPHRASE))
            .unwrap();
        mock.apply(&cmd(r#"{"cmd":"tick","ms":1500}"#)).unwrap();
        assert!(mock.snapshot().unwrap().connection.is_connected());
        // No network of that name: not found after association, like
        // NetworkManager's SSID_NOT_FOUND.
        mock.join_hidden(&Ssid::from("ferrix-nowhere"), SecurityType::Open, None)
            .unwrap();
        assert_eq!(failed(&mock), ConnectionState::Associating);
        let events = mock.apply(&cmd(r#"{"cmd":"tick","ms":500}"#)).unwrap();
        assert_eq!(
            events[0],
            NetworkEvent::ConnectionChanged {
                connection: ConnectionState::Failed {
                    error: NetworkError::NotFound { id: "ssid".into() }
                }
            }
        );
        // A visible network may be joined by name too.
        mock.join_hidden(&Ssid::from(GUEST_SSID), SecurityType::Open, None)
            .unwrap();
        mock.apply(&cmd(r#"{"cmd":"tick","ms":1500}"#)).unwrap();
        assert!(mock.snapshot().unwrap().connection.is_connected());
        // Radio off refuses it like every join.
        mock.set_radio(false).unwrap();
        assert_eq!(
            mock.join_hidden(&hidden_name(), SecurityType::Wpa2Psk, Some(LAB_PASSPHRASE)),
            Err(BackendError::Network(NetworkError::RadioDisabled))
        );
    }

    #[test]
    fn disabled_radio_refuses_scan_and_connect_then_enables() {
        let (mock, rx) = mock(MockNetworkConfig {
            radio_enabled: false,
            connected: None,
            ..MockNetworkConfig::default()
        });
        assert_eq!(
            mock.scan(),
            Err(BackendError::Network(NetworkError::RadioDisabled))
        );
        assert_eq!(
            mock.connect(&ApId::new("ap-1"), None),
            Err(BackendError::Network(NetworkError::RadioDisabled))
        );
        mock.set_radio(true).unwrap();
        let events = drain(&rx);
        assert_eq!(names(&events), ["radio-changed", "access-points-changed"]);
        assert_eq!(mock.snapshot().unwrap().access_points.len(), 2);
        // Turning it off again drops everything on the air.
        mock.set_radio(false).unwrap();
        let events = drain(&rx);
        assert_eq!(names(&events), ["radio-changed", "access-points-changed"]);
        assert!(mock.snapshot().unwrap().access_points.is_empty());
    }

    #[test]
    fn scanning_completes_on_tick_not_on_wall_clock() {
        let (mock, rx) = mock(disconnected());
        mock.scan().unwrap();
        assert_eq!(names(&drain(&rx)), ["scan-changed"]);
        assert_eq!(mock.snapshot().unwrap().scan, ScanState::Scanning);
        // A second scan while scanning is a no-op.
        mock.scan().unwrap();
        assert!(drain(&rx).is_empty());
        mock.apply(&cmd(r#"{"cmd":"tick","ms":999}"#)).unwrap();
        assert!(drain(&rx).is_empty(), "scan must not finish early");
        let events = mock.apply(&cmd(r#"{"cmd":"tick","ms":1}"#)).unwrap();
        assert_eq!(
            events.iter().map(NetworkEvent::name).collect::<Vec<_>>(),
            ["scan-changed", "access-points-changed"]
        );
        assert_eq!(
            mock.snapshot().unwrap().scan,
            ScanState::Completed { count: 2 }
        );
        assert_eq!(mock.clock_ms(), 1000);

        // A failed scan applies the one-shot policy and leaves the list alone.
        mock.apply(&cmd(
            r#"{"cmd":"scan-result","outcome":"failed","reason":"busy"}"#,
        ))
        .unwrap();
        mock.scan().unwrap();
        let events = mock.apply(&cmd(r#"{"cmd":"tick","ms":1000}"#)).unwrap();
        assert_eq!(
            events,
            [NetworkEvent::ScanChanged {
                scan: ScanState::Failed {
                    reason: "busy".into()
                }
            }]
        );
        mock.scan().unwrap();
        mock.apply(&cmd(r#"{"cmd":"tick","ms":1000}"#)).unwrap();
        assert_eq!(
            mock.snapshot().unwrap().scan,
            ScanState::Completed { count: 2 }
        );
    }

    #[test]
    fn connect_walks_phases_and_saves_a_profile() {
        let (mock, rx) = mock(disconnected());
        let lab = lab_id(&mock);
        mock.connect(&lab, Some(LAB_PASSPHRASE)).unwrap();
        assert_eq!(names(&drain(&rx)), ["connection-changed"]);
        assert_eq!(
            mock.snapshot().unwrap().connection,
            ConnectionState::Associating
        );
        mock.apply(&cmd(r#"{"cmd":"tick","ms":500}"#)).unwrap();
        assert_eq!(
            mock.snapshot().unwrap().connection,
            ConnectionState::Authenticating
        );
        mock.apply(&cmd(r#"{"cmd":"tick","ms":500}"#)).unwrap();
        assert_eq!(
            mock.snapshot().unwrap().connection,
            ConnectionState::Configuring
        );
        let events = mock.apply(&cmd(r#"{"cmd":"tick","ms":500}"#)).unwrap();
        assert_eq!(
            events.iter().map(NetworkEvent::name).collect::<Vec<_>>(),
            [
                "connection-changed",
                "ip-changed",
                "connectivity-changed",
                "profiles-changed"
            ]
        );
        let state = mock.snapshot().unwrap();
        assert_eq!(
            state.connection,
            ConnectionState::Connected { ap: lab.clone() }
        );
        assert_eq!(state.connectivity, Connectivity::Full);
        assert_eq!(state.profiles.len(), 1);
        drain(&rx);

        // One big tick walks all remaining phases at once.
        mock.disconnect().unwrap();
        assert_eq!(
            mock.snapshot().unwrap().connection,
            ConnectionState::Disconnected
        );
        let profile = state.profiles[0].id.clone();
        mock.connect_profile(&profile).unwrap();
        mock.apply(&cmd(r#"{"cmd":"tick","ms":5000}"#)).unwrap();
        let state = mock.snapshot().unwrap();
        assert_eq!(state.connection, ConnectionState::Connected { ap: lab });
        assert_eq!(state.profiles.len(), 1, "re-joining reuses the profile");

        // Forgetting the connected profile disconnects.
        mock.forget(&profile).unwrap();
        let state = mock.snapshot().unwrap();
        assert!(state.profiles.is_empty());
        assert_eq!(state.connection, ConnectionState::Disconnected);
        assert_eq!(
            mock.forget(&profile),
            Err(BackendError::Network(NetworkError::NotFound {
                id: "profile-1".into()
            }))
        );
        assert_eq!(
            mock.connect_profile(&profile),
            Err(BackendError::Network(NetworkError::NotFound {
                id: "profile-1".into()
            }))
        );
    }

    #[test]
    fn wrong_passphrase_fails_authentication() {
        let (mock, _rx) = mock(disconnected());
        let lab = lab_id(&mock);
        mock.connect(&lab, Some("wrong")).unwrap();
        mock.apply(&cmd(r#"{"cmd":"tick","ms":1000}"#)).unwrap();
        let state = mock.snapshot().unwrap();
        assert_eq!(
            state.connection,
            ConnectionState::Failed {
                error: NetworkError::AuthenticationFailed
            }
        );
        assert!(state.profiles.is_empty(), "no profile for a failed join");
        // A missing passphrase on a protected network fails the same way.
        mock.connect(&lab, None).unwrap();
        mock.apply(&cmd(r#"{"cmd":"tick","ms":1000}"#)).unwrap();
        assert_eq!(
            mock.snapshot().unwrap().connection,
            ConnectionState::Failed {
                error: NetworkError::AuthenticationFailed
            }
        );
        // Forced policy fails even with the right secret, once.
        mock.apply(&cmd(
            r#"{"cmd":"set-connect-result","result":"auth-failed"}"#,
        ))
        .unwrap();
        mock.connect(&lab, Some(LAB_PASSPHRASE)).unwrap();
        mock.apply(&cmd(r#"{"cmd":"tick","ms":1000}"#)).unwrap();
        assert_eq!(
            mock.snapshot().unwrap().connection,
            ConnectionState::Failed {
                error: NetworkError::AuthenticationFailed
            }
        );
        mock.connect(&lab, Some(LAB_PASSPHRASE)).unwrap();
        mock.apply(&cmd(r#"{"cmd":"tick","ms":1500}"#)).unwrap();
        assert!(mock.snapshot().unwrap().connection.is_connected());
    }

    #[test]
    fn dhcp_dns_and_timeout_policies_fail_at_the_right_phase() {
        let (mock, rx) = mock(disconnected());
        let lab = lab_id(&mock);
        for (policy, error, phases) in [
            ("dhcp-failed", NetworkError::DhcpFailed, 3),
            ("dns-failed", NetworkError::DnsFailed, 3),
            ("timeout", NetworkError::TimedOut, 1),
        ] {
            mock.apply(&cmd(&format!(
                r#"{{"cmd":"set-connect-result","result":"{policy}"}}"#
            )))
            .unwrap();
            mock.connect(&lab, Some(LAB_PASSPHRASE)).unwrap();
            drain(&rx);
            for _ in 1..phases {
                mock.apply(&cmd(r#"{"cmd":"tick","ms":500}"#)).unwrap();
                assert!(mock.snapshot().unwrap().connection.is_transitional());
            }
            let events = mock.apply(&cmd(r#"{"cmd":"tick","ms":500}"#)).unwrap();
            assert_eq!(
                events[0],
                NetworkEvent::ConnectionChanged {
                    connection: ConnectionState::Failed { error }
                },
                "{policy}"
            );
            assert!(mock.snapshot().unwrap().ip.is_none());
        }
    }

    #[test]
    fn captive_portal_offline_and_recovery_change_connectivity_only() {
        let (mock, rx) = mock(MockNetworkConfig::default());
        let events = mock
            .apply(&cmd(
                r#"{"cmd":"set-connectivity","connectivity":"portal","url":"http://portal.example/login"}"#,
            ))
            .unwrap();
        assert_eq!(
            events,
            [NetworkEvent::ConnectivityChanged {
                connectivity: Connectivity::Portal {
                    url: Some("http://portal.example/login".into())
                }
            }]
        );
        assert!(mock.snapshot().unwrap().connection.is_connected());
        let events = mock
            .apply(&cmd(r#"{"cmd":"set-connectivity","connectivity":"none"}"#))
            .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(mock.snapshot().unwrap().connectivity, Connectivity::None);
        let events = mock
            .apply(&cmd(r#"{"cmd":"set-connectivity","connectivity":"full"}"#))
            .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(mock.snapshot().unwrap().connectivity, Connectivity::Full);
        assert_eq!(
            names(&drain(&rx)),
            [
                "connectivity-changed",
                "connectivity-changed",
                "connectivity-changed"
            ]
        );
        // A remote drop reports a failed link with the reason, then recovery.
        let events = mock
            .apply(&cmd(r#"{"cmd":"disconnect","reason":"ap-lost"}"#))
            .unwrap();
        assert_eq!(
            events.iter().map(NetworkEvent::name).collect::<Vec<_>>(),
            ["connection-changed", "ip-changed", "connectivity-changed"]
        );
        assert_eq!(
            mock.snapshot().unwrap().connection,
            ConnectionState::Failed {
                error: NetworkError::Backend {
                    reason: "ap-lost".into()
                }
            }
        );
        let profile = mock.snapshot().unwrap().profiles[0].id.clone();
        mock.connect_profile(&profile).unwrap();
        mock.apply(&cmd(r#"{"cmd":"tick","ms":1500}"#)).unwrap();
        assert!(mock.snapshot().unwrap().connection.is_connected());
    }

    #[test]
    fn set_ip_applies_now_and_to_the_next_join() {
        let (mock, _rx) = mock(MockNetworkConfig::default());
        let events = mock
            .apply(&cmd(
                r#"{"cmd":"set-ip","addresses":["192.0.2.10/24"],"gateway":"192.0.2.1","dns":["192.0.2.53"]}"#,
            ))
            .unwrap();
        assert_eq!(events.len(), 1);
        let ip = mock.snapshot().unwrap().ip.unwrap();
        assert_eq!(ip.addresses, ["192.0.2.10/24"]);
        assert!(!ip.dhcp);
        mock.disconnect().unwrap();
        assert!(mock.snapshot().unwrap().ip.is_none());
        let profile = mock.snapshot().unwrap().profiles[0].id.clone();
        mock.connect_profile(&profile).unwrap();
        mock.apply(&cmd(r#"{"cmd":"tick","ms":1500}"#)).unwrap();
        assert_eq!(
            mock.snapshot().unwrap().ip.unwrap().gateway,
            Some("192.0.2.1".into())
        );
    }

    #[test]
    fn adapter_absent_refuses_everything_and_returns() {
        let (mock, rx) = mock(MockNetworkConfig::default());
        let events = mock
            .apply(&cmd(r#"{"cmd":"set-adapter","present":false}"#))
            .unwrap();
        assert_eq!(
            events.iter().map(NetworkEvent::name).collect::<Vec<_>>(),
            [
                "adapter-changed",
                "radio-changed",
                "access-points-changed",
                "connection-changed",
                "ip-changed",
                "connectivity-changed"
            ]
        );
        let state = mock.snapshot().unwrap();
        assert_eq!(state.adapter, AdapterPresence::Absent);
        assert!(matches!(state.radio, RadioState::Unavailable { .. }));
        for result in [
            mock.scan(),
            mock.set_radio(true),
            mock.connect(&ApId::new("ap-1"), None),
            mock.disconnect(),
        ] {
            assert_eq!(result, Err(BackendError::Network(NetworkError::NoAdapter)));
        }
        drain(&rx);
        let events = mock
            .apply(&cmd(r#"{"cmd":"set-adapter","present":true}"#))
            .unwrap();
        assert_eq!(
            events.iter().map(NetworkEvent::name).collect::<Vec<_>>(),
            ["adapter-changed", "radio-changed", "access-points-changed"]
        );
        assert!(mock.snapshot().unwrap().radio.is_enabled());
        // Absent from boot: no access points, typed errors.
        let (absent, _rx) = self::mock(MockNetworkConfig {
            adapter_present: false,
            ..MockNetworkConfig::default()
        });
        let state = absent.snapshot().unwrap();
        assert_eq!(state.adapter, AdapterPresence::Absent);
        assert!(state.access_points.is_empty());
        assert_eq!(state.connection, ConnectionState::Disconnected);
    }

    #[test]
    fn backend_crash_and_recovery_publish_lost_then_restarted() {
        let (mock, rx) = mock(MockNetworkConfig::default());
        let events = mock.apply(&cmd(r#"{"cmd":"crash-backend"}"#)).unwrap();
        assert!(events.is_empty());
        assert_eq!(names(&drain(&rx)), ["backend-lost"]);
        assert!(matches!(
            mock.snapshot(),
            Err(BackendError::Unavailable { .. })
        ));
        assert!(matches!(mock.scan(), Err(BackendError::Unavailable { .. })));
        // Host mutations while down are folded into the recovery snapshot.
        let events = mock
            .apply(&cmd(
                r#"{"cmd":"set-access-points","access_points":[{"ssid":"ferrix-guest","security":"open","signal_percent":40}]}"#,
            ))
            .unwrap();
        assert_eq!(events.len(), 1);
        assert!(drain(&rx).is_empty(), "no deltas while the backend is down");
        assert_eq!(mock.summary()["backend"], "crashed");
        let _ = mock.apply(&cmd(r#"{"cmd":"tick","ms":100}"#)).unwrap();
        let events = drain(&rx);
        assert_eq!(names(&events), ["service-restarted"]);
        let BackendEvent::Restarted { state } = &events[0] else {
            panic!("restart carries a snapshot");
        };
        assert_eq!(state.access_points.len(), 1);
        assert_eq!(state.access_points[0].ssid, Ssid::from(GUEST_SSID));
        assert_eq!(mock.summary()["backend"], "running");
        assert!(mock.snapshot().is_ok());
    }

    #[test]
    fn access_point_ids_stay_stable_across_world_updates() {
        let (mock, _rx) = mock(disconnected());
        let before = mock.snapshot().unwrap();
        let lab_before = lab_id(&mock);
        mock.apply(&cmd(
            r#"{"cmd":"set-access-points","access_points":[{"ssid":"ferrix-lab","security":"wpa2-psk","signal_percent":20,"passphrase":"mock-passphrase"},{"ssid":"ferrix-cafe","security":"wpa3-sae","signal_percent":90,"passphrase":"other"}]}"#,
        ))
        .unwrap();
        let after = mock.snapshot().unwrap();
        assert_eq!(lab_id(&mock), lab_before);
        let lab = after
            .access_points
            .iter()
            .find(|ap| ap.id == lab_before)
            .unwrap();
        assert_eq!(lab.signal_percent, 20);
        assert_eq!(
            lab.bssid_hash,
            before.access_point(&lab_before).unwrap().bssid_hash
        );
        let cafe = after
            .access_points
            .iter()
            .find(|ap| ap.ssid == Ssid::from("ferrix-cafe"))
            .unwrap();
        assert_eq!(cafe.id, ApId::new("ap-3"), "new names get fresh ids");
        assert_eq!(cafe.frequency_mhz, Some(DEFAULT_FREQUENCY_MHZ));
    }

    #[test]
    fn handler_acks_carry_names_events_and_hashed_state_only() {
        let mock = MockNetwork::new(MockNetworkConfig::default());
        let ack = mock.handle(r#"{"cmd":"state"}"#);
        let Ack::Ok { ack, events, state } = ack else {
            panic!("state must ack ok");
        };
        assert_eq!(ack, "state");
        assert!(events.is_empty());
        assert_eq!(state["connection"], "connected");
        assert_eq!(state["clock_ms"], 0);
        let text = state.to_string();
        assert!(!text.contains("ferrix-lab"), "ssids appear as digests only");
        assert!(text.contains(&Ssid::from(LAB_SSID).hash()));
        assert!(!text.contains("mock-passphrase"));

        let ack = mock.handle(r#"{"cmd":"set-radio","enabled":false}"#);
        let Ack::Ok { events, .. } = ack else {
            panic!("set-radio must ack ok");
        };
        assert_eq!(
            events,
            [
                "radio-changed",
                "access-points-changed",
                "connection-changed",
                "ip-changed",
                "connectivity-changed"
            ]
        );
        let ack = mock.handle(r#"{"cmd":"tick","ms":250}"#);
        assert!(ack.is_ok());
        assert_eq!(mock.clock_ms(), 250);
    }

    #[test]
    fn handler_rejects_unknown_commands_fields_and_malformed_lines() {
        let mock = MockNetwork::new(MockNetworkConfig::default());
        for line in [
            r#"{"cmd":"reboot"}"#,
            r#"{"cmd":"tick"}"#,
            r#"{"cmd":"tick","ms":1,"extra":true}"#,
            r#"{"cmd":"state","x":1}"#,
            r#"{"cmd":"crash-backend","hard":true}"#,
            r#"{"cmd":"set-access-points","access_points":[{"ssid":"a","security":"open","signal_percent":1,"bssid":"aa:bb"}]}"#,
            r#"{"cmd":"set-connect-result","result":"maybe"}"#,
            r#"{"cmd":"set-connectivity","connectivity":"portal","url":1}"#,
            r#"{"nope":1}"#,
            "garbage",
        ] {
            let ack = mock.handle(line);
            assert!(!ack.is_ok(), "{line}");
        }
        let Ack::Error { ack, reason } = mock.handle(r#"{"cmd":"tick"}"#) else {
            panic!("must be an error ack");
        };
        assert_eq!(ack.as_deref(), Some("tick"));
        assert!(reason.contains("ms"), "{reason}");
        // Nothing mutated.
        assert_eq!(mock.clock_ms(), 0);
        assert!(mock.snapshot().unwrap().connection.is_connected());
    }

    #[test]
    fn every_command_name_is_exact_and_debug_redacts_secrets() {
        let commands = [
            (r#"{"cmd":"state"}"#, "state"),
            (r#"{"cmd":"set-adapter","present":true}"#, "set-adapter"),
            (r#"{"cmd":"set-radio","enabled":true}"#, "set-radio"),
            (
                r#"{"cmd":"set-access-points","access_points":[]}"#,
                "set-access-points",
            ),
            (
                r#"{"cmd":"scan-result","outcome":"completed"}"#,
                "scan-result",
            ),
            (
                r#"{"cmd":"set-connect-result","result":"connected"}"#,
                "set-connect-result",
            ),
            (
                r#"{"cmd":"set-connectivity","connectivity":"limited"}"#,
                "set-connectivity",
            ),
            (r#"{"cmd":"disconnect","reason":"x"}"#, "disconnect"),
            (r#"{"cmd":"set-ip","addresses":[],"dns":[]}"#, "set-ip"),
            (r#"{"cmd":"tick","ms":1}"#, "tick"),
            (r#"{"cmd":"crash-backend"}"#, "crash-backend"),
        ];
        for (line, name) in commands {
            let command = cmd(line);
            assert_eq!(command.name(), name);
            let wire = serde_json::to_value(&command).unwrap();
            assert_eq!(wire["cmd"], name);
        }
        let command = cmd(
            r#"{"cmd":"set-access-points","access_points":[{"ssid":"ferrix-lab","security":"wpa2-psk","signal_percent":1,"passphrase":"mock-passphrase"}]}"#,
        );
        let debug = format!("{command:?}");
        assert!(!debug.contains("mock-passphrase"), "{debug}");
        assert!(debug.contains("redacted"));
        assert!(!format!("{:?}", MockAccessPoint::lab()).contains("mock-passphrase"));
    }
}
