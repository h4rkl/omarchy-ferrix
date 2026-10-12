//! Settings view model: the network snapshot from `ferrix-networkd`, the
//! redacted telephony snapshot for the Radios and Mobile pages, the
//! `ferrix-core` snapshot for Display and Bluetooth (see `system.rs`),
//! passphrase entry, and typed reasons for everything that cannot be done.
//! Pure state.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use ferrix_hardware_api::{BluetoothCommand, CapabilityMatrix, HardwareState};
use ferrix_network_api::{
    AccessPoint, AdapterPresence, ApId, ConnectionState, Connectivity, NetworkError, NetworkEvent,
    NetworkState, ProfileId, RadioState, SavedProfile, ScanState, SecurityType,
};
use ferrix_network_protocol::Feed;
use ferrix_telephony_api::{DataBearer, DataProfile, RadioPower, RedactedTelephonyState};
use ferrix_telephony_protocol::{Event as TeleEvent, Update};
use ferrix_ui::osk::OskState;

/// The root list and one focused page per category (no tab strip: each
/// category is a fullscreen flow with Back to the list).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Root,
    Network,
    Bluetooth,
    Mobile,
    Display,
    Sound,
    Keyboard,
    Radios,
    About,
}

impl Page {
    pub const ALL: [Page; 9] = [
        Page::Root,
        Page::Network,
        Page::Bluetooth,
        Page::Mobile,
        Page::Display,
        Page::Sound,
        Page::Keyboard,
        Page::Radios,
        Page::About,
    ];

    /// The categories listed on the root page, in order.
    pub const SECTIONS: [Page; 8] = [
        Page::Network,
        Page::Bluetooth,
        Page::Mobile,
        Page::Display,
        Page::Sound,
        Page::Keyboard,
        Page::Radios,
        Page::About,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Page::Root => "root",
            Page::Network => "network",
            Page::Bluetooth => "bluetooth",
            Page::Mobile => "mobile",
            Page::Display => "display",
            Page::Sound => "sound",
            Page::Keyboard => "keyboard",
            Page::Radios => "radios",
            Page::About => "about",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Page::Root => "Settings",
            Page::Network => "Wi-Fi",
            Page::Bluetooth => "Bluetooth",
            Page::Mobile => "Mobile network",
            Page::Display => "Display",
            Page::Sound => "Sound",
            Page::Keyboard => "Keyboard",
            Page::Radios => "Airplane mode",
            Page::About => "About",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Page::ALL.into_iter().find(|p| p.name() == name)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Link {
    #[default]
    Connecting,
    Connected,
    Reconnected,
    Unavailable,
}

impl Link {
    pub fn name(self) -> &'static str {
        match self {
            Link::Connecting => "connecting",
            Link::Connected => "connected",
            Link::Reconnected => "reconnected",
            Link::Unavailable => "unavailable",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    SetRadio(bool),
    Scan,
    Connect {
        ap: ApId,
        passphrase: Option<String>,
    },
    /// Join a network by name (Add network), for one that broadcasts no
    /// SSID. The passphrase is never recorded.
    JoinHidden {
        ssid: String,
        security: SecurityType,
        passphrase: Option<String>,
    },
    Disconnect,
    Forget(ProfileId),
    /// Telephony radio power (airplane mode's cellular half).
    SetRadioPower(RadioPower),
    /// Cellular packet-data bearer control, independent of radio power.
    SetCellularData(bool),
    /// Bluetooth controller power through `ferrix-core`.
    SetBluetooth(bool),
    /// Re-query the Bluetooth session from `ferrix-core`.
    RefreshBluetoothDevices,
    /// A product Bluetooth request (discovery, pairing, connections).
    Bluetooth(BluetoothCommand),
    SetBrightness(u8),
    /// Auto-rotate off (`true`, pinned portrait) or on.
    SetRotationLock(bool),
    /// Active output volume through `ferrix-core`.
    SetVolume(u8),
    /// Active output (playback) mute through `ferrix-core`.
    SetMuted(bool),
    /// Persist the on-screen keyboard layout (a `ferrix_ui::keyboard` id).
    SetKeyboardLayout(String),
    Back,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Model {
    pub page: Page,
    pub net_link: Link,
    pub net: Option<NetworkState>,
    pub net_evidence: Option<String>,
    pub tele_link: Link,
    pub tele: Option<RedactedTelephonyState>,
    pub tele_evidence: Option<String>,
    /// Non-secret cellular data parameters supplied by the target runtime.
    /// The Samsung-SIT backend requires an explicit APN; the value is never
    /// included in UI state or evidence records.
    pub data_profile: DataProfile,
    /// An access point awaiting a passphrase.
    pub passphrase_for: Option<ApId>,
    pub passphrase: String,
    /// The passphrase field shows plain text instead of bullets; every new
    /// prompt starts masked.
    pub passphrase_shown: bool,
    pub osk: OskState,
    pub pending: Vec<Action>,
    pub last_error: Option<String>,
    pub target: String,
    /// A saved profile awaiting the user's Forget confirmation.
    pub forget_confirm: Option<ProfileId>,
    /// The access point of the last connect request, for Retry.
    pub last_connect: Option<ApId>,
    /// Name and security of the last join by name, for Retry (never the
    /// passphrase).
    pub last_hidden: Option<(String, SecurityType)>,
    /// The Add network form, while it is open.
    pub add_network: Option<crate::hidden::AddNetwork>,
    /// A typed error the connect request itself returned (the daemon may
    /// also report the failure through `ConnectionState::Failed`).
    pub connect_error: Option<NetworkError>,
    /// Link to `ferrix-core` (Display, Bluetooth).
    pub core_link: Link,
    pub hw: Option<HardwareState>,
    pub hw_capabilities: CapabilityMatrix,
    /// Target id the core reported.
    pub hw_target: Option<String>,
    pub bt_devices: crate::bluetooth::DeviceList,
    /// The shown session is older than the core's last Bluetooth event.
    pub bt_stale: bool,
    pub bt_view: crate::bluetooth::BtView,
    /// The detail view's Forget awaits confirmation.
    pub bt_forget_confirm: bool,
    /// The legacy PIN being typed (never recorded).
    pub bt_pin: crate::bluetooth::PinDraft,
    /// When the shown session arrived (anchors the scan countdown).
    pub bt_session_at: Option<std::time::Instant>,
    /// The UI clock the countdown is drawn against.
    pub bt_now: Option<std::time::Instant>,
    /// Brightness preview while the rail is dragged.
    pub brightness_drag: Option<u8>,
    /// Volume preview while the rail is dragged.
    pub volume_drag: Option<u8>,
    /// The on-screen keyboard layout id shown as active (Keyboard page).
    pub kbd_layout: String,
    /// The Keyboard page's scratch field and whether it has focus.
    pub kbd_try: String,
    pub kbd_try_focused: bool,
    pub(crate) core_lost_once: bool,
    net_lost_once: bool,
    tele_lost_once: bool,
}

/// A hidden network advertises no name (empty or NUL-padded SSID).
fn is_hidden(ap: &AccessPoint) -> bool {
    ap.ssid.as_bytes().iter().all(|b| *b == 0)
}

/// Shown when the backend reports `authentication-failed`.
pub const AUTH_FAILED_TEXT: &str = "Wrong password or unsupported security";

/// Shown when no network answered to the name (`not-found` for `ssid`).
pub const NOT_FOUND_TEXT: &str = "No network with that name is in range";

/// Converts the optional target launch setting into the typed data profile.
/// Invalid values are dropped at the UI boundary; `ferrix-telephonyd` still
/// performs the authoritative validation for every socket request.
pub fn data_profile_from_env(apn: Option<&str>) -> DataProfile {
    let profile = DataProfile {
        apn: apn.filter(|value| !value.is_empty()).map(str::to_owned),
    };
    if profile.validate().is_ok() {
        profile
    } else {
        DataProfile::default()
    }
}

impl Default for Model {
    fn default() -> Self {
        Self::new()
    }
}

impl Model {
    pub fn new() -> Self {
        Self {
            page: Page::Root,
            net_link: Link::Connecting,
            net: None,
            net_evidence: None,
            tele_link: Link::Connecting,
            tele: None,
            tele_evidence: None,
            data_profile: DataProfile::default(),
            passphrase_for: None,
            passphrase: String::new(),
            passphrase_shown: false,
            osk: OskState::default(),
            pending: Vec::new(),
            last_error: None,
            target: "unknown".to_owned(),
            forget_confirm: None,
            last_connect: None,
            last_hidden: None,
            add_network: None,
            connect_error: None,
            core_link: Link::Connecting,
            hw: None,
            hw_capabilities: CapabilityMatrix::default(),
            hw_target: None,
            bt_devices: crate::bluetooth::DeviceList::Unknown,
            bt_stale: false,
            bt_view: crate::bluetooth::BtView::Main,
            bt_forget_confirm: false,
            bt_pin: crate::bluetooth::PinDraft::default(),
            bt_session_at: None,
            bt_now: None,
            brightness_drag: None,
            volume_drag: None,
            kbd_layout: ferrix_ui::keyboard::DEFAULT_LAYOUT.to_owned(),
            kbd_try: String::new(),
            kbd_try_focused: false,
            core_lost_once: false,
            net_lost_once: false,
            tele_lost_once: false,
        }
    }

    /// Applies an optional target-runtime APN without making carrier
    /// assumptions. Invalid values are ignored here; the privileged daemon
    /// performs the authoritative typed validation before any modem call.
    pub fn set_data_profile(&mut self, profile: DataProfile) {
        if profile.validate().is_ok() {
            self.data_profile = profile;
        }
    }

    pub fn state_name(&self) -> String {
        match self.page {
            Page::Root => "root".to_owned(),
            Page::Bluetooth => self
                .bluetooth_flow_state()
                .unwrap_or_else(|| self.bluetooth_state_name()),
            Page::Mobile => self.mobile_state_name(),
            Page::Display => self.display_state_name(),
            Page::Sound => self.sound_state_name(),
            Page::Keyboard => self.keyboard_state_name(),
            Page::Network if self.forget_confirm.is_some() => "confirm-forget".to_owned(),
            Page::Network if self.add_network.is_some() => "add-network".to_owned(),
            Page::Network if self.auth_failed() && self.passphrase_for.is_none() => {
                "auth-failed".to_owned()
            }
            Page::Network => match (&self.passphrase_for, &self.net) {
                (Some(_), _) => "passphrase".to_owned(),
                (None, Some(_)) if self.failure().is_some() => "failed".to_owned(),
                (None, Some(net)) => net.connection.name().to_owned(),
                (None, None) => self.net_link.name().to_owned(),
            },
            Page::Radios => format!("airplane-{}", if self.airplane() { "on" } else { "off" }),
            Page::About => self.target.clone(),
        }
    }

    pub fn network_label(&self) -> String {
        format!("network \u{b7} {}", self.net_link.name())
    }

    pub fn telephony_label(&self) -> String {
        format!("telephony \u{b7} {}", self.tele_link.name())
    }

    /// Records the evidence label `ferrix-telephonyd` reported in its hello.
    pub fn set_tele_evidence(&mut self, evidence: &str) {
        self.tele_evidence = (!evidence.is_empty()).then(|| evidence.to_owned());
    }

    /// The About row for telephony evidence: the daemon's label once known,
    /// `connected` while a snapshot is held but no label arrived yet.
    pub fn tele_evidence_label(&self) -> String {
        match (&self.tele_evidence, &self.tele) {
            (Some(evidence), _) => evidence.clone(),
            (None, Some(_)) => "connected \u{b7} label not reported".to_owned(),
            (None, None) => format!("no snapshot \u{b7} {}", self.tele_link.name()),
        }
    }

    pub fn net_evidence_label(&self) -> String {
        self.net_evidence
            .clone()
            .unwrap_or_else(|| format!("no snapshot \u{b7} {}", self.net_link.name()))
    }

    // -- feeds ---------------------------------------------------------------

    pub fn apply_network(&mut self, feed: Feed) -> bool {
        // A failed join is often visible only for one transition: the
        // backend settles on `disconnected` right after. Keep the typed
        // failure until the next attempt or a connection succeeds.
        let failed = match &feed {
            Feed::Snapshot { state, .. }
            | Feed::Event(NetworkEvent::ServiceRestarted { state }) => match &state.connection {
                ConnectionState::Failed { error } => Some(error.clone()),
                _ => None,
            },
            Feed::Event(NetworkEvent::ConnectionChanged {
                connection: ConnectionState::Failed { error },
            }) => Some(error.clone()),
            _ => None,
        };
        if let Some(error) = failed {
            self.connect_error = Some(error);
        }
        match feed {
            Feed::Snapshot { state, evidence } => {
                self.net_link = if self.net_lost_once {
                    Link::Reconnected
                } else {
                    Link::Connected
                };
                self.net_lost_once = false;
                self.net = Some(state);
                self.net_evidence = Some(evidence);
                self.validate_passphrase_target();
            }
            Feed::Event(NetworkEvent::ServiceRestarted { state }) => {
                self.net_link = Link::Reconnected;
                self.net = Some(state);
                self.validate_passphrase_target();
            }
            Feed::Event(event) => {
                if let Some(net) = self.net.as_mut() {
                    event.apply_to(net);
                }
                self.validate_passphrase_target();
            }
            Feed::Disconnected { .. } => {
                self.net_link = Link::Unavailable;
                self.net_lost_once = true;
            }
        }
        true
    }

    fn validate_passphrase_target(&mut self) {
        if let Some(id) = &self.forget_confirm
            && !self
                .net
                .as_ref()
                .is_some_and(|n| n.profiles.iter().any(|p| &p.id == id))
        {
            self.forget_confirm = None;
        }
        if self
            .net
            .as_ref()
            .is_some_and(|n| matches!(n.connection, ConnectionState::Connected { .. }))
        {
            self.connect_error = None;
        }
        if let Some(ap) = &self.passphrase_for
            && !self
                .net
                .as_ref()
                .is_some_and(|n| n.access_point(ap).is_some())
        {
            self.passphrase_for = None;
            self.passphrase.clear();
        }
    }

    pub fn apply_telephony(&mut self, update: Update) -> bool {
        match update {
            Update::Snapshot(view) => {
                self.tele_link = if self.tele_lost_once {
                    Link::Reconnected
                } else {
                    Link::Connected
                };
                self.tele_lost_once = false;
                self.tele = Some(view.redacted());
            }
            Update::Disconnected => {
                self.tele_link = Link::Unavailable;
                self.tele_lost_once = true;
            }
            Update::Event(TeleEvent::Full(
                ferrix_telephony_api::TelephonyEvent::ServiceRestarted { state },
            )) => {
                self.tele_link = Link::Reconnected;
                self.tele = Some(state.redacted());
            }
            Update::Event(TeleEvent::Redacted(
                ferrix_telephony_api::RedactedTelephonyEvent::ServiceRestarted { state },
            )) => {
                self.tele_link = Link::Reconnected;
                self.tele = Some(state);
            }
            Update::Event(event) => {
                if let Some(tele) = self.tele.as_mut() {
                    event.apply_redacted(tele);
                }
            }
        }
        true
    }

    // -- derived -------------------------------------------------------------

    pub fn adapter_label(&self) -> String {
        match self.net.as_ref().map(|n| &n.adapter) {
            None => "adapter \u{b7} unknown".to_owned(),
            Some(AdapterPresence::Absent) => "adapter \u{b7} absent".to_owned(),
            Some(AdapterPresence::Present { kind }) => format!("adapter \u{b7} {}", kind.name()),
        }
    }

    pub fn radio_enabled(&self) -> bool {
        self.net.as_ref().is_some_and(|n| n.radio.is_enabled())
    }

    pub fn radio_label(&self) -> String {
        match self.net.as_ref().map(|n| &n.radio) {
            None => "radio \u{b7} unknown".to_owned(),
            Some(RadioState::Unavailable { reason }) => {
                format!("radio \u{b7} unavailable \u{b7} {reason}")
            }
            Some(radio) => format!("radio \u{b7} {}", radio.name()),
        }
    }

    pub fn scan_label(&self) -> String {
        match self.net.as_ref().map(|n| &n.scan) {
            None => "scan \u{b7} unknown".to_owned(),
            Some(ScanState::Completed { count }) => format!("scan \u{b7} {count} found"),
            Some(ScanState::Failed { reason }) => format!("scan \u{b7} failed \u{b7} {reason}"),
            Some(scan) => format!("scan \u{b7} {}", scan.name()),
        }
    }

    pub fn connection_label(&self) -> String {
        match self.net.as_ref().map(|n| &n.connection) {
            None => "connection \u{b7} unknown".to_owned(),
            Some(ConnectionState::Connected { ap }) => {
                // A hidden network may stay nameless in the scan list even
                // once joined; the name the user typed stands in for it.
                let ssid = self
                    .net
                    .as_ref()
                    .and_then(|n| n.access_point(ap))
                    .filter(|ap| !is_hidden(ap))
                    .map(|ap| ap.ssid.to_string())
                    .or_else(|| self.last_hidden.as_ref().map(|(name, _)| name.clone()))
                    .unwrap_or_else(|| ap.to_string());
                format!("connected \u{b7} {ssid}")
            }
            // The notice below carries the full sentence; the chip stays short.
            Some(ConnectionState::Failed {
                error: NetworkError::AuthenticationFailed,
            }) => "failed \u{b7} auth".to_owned(),
            Some(ConnectionState::Failed { error }) => format!("failed \u{b7} {}", error.name()),
            Some(state) => state.name().to_owned(),
        }
    }

    /// The failure of the last connect attempt: the request's own typed
    /// error first, else the daemon's `Failed` connection state.
    fn failure(&self) -> Option<&NetworkError> {
        self.connect_error
            .as_ref()
            .or(match self.net.as_ref().map(|n| &n.connection) {
                Some(ConnectionState::Failed { error }) => Some(error),
                _ => None,
            })
    }

    /// Whether the last attempt failed on secrets/authentication: the
    /// backend's `authentication-failed` (NetworkManager maps its
    /// supplicant and missing-secrets reasons to it).
    pub fn auth_failed(&self) -> bool {
        matches!(self.failure(), Some(NetworkError::AuthenticationFailed))
    }

    /// The connection failure as shown: a plain sentence for bad secrets,
    /// otherwise the daemon's own error text.
    pub fn connection_error(&self) -> Option<String> {
        self.failure().map(|error| match error {
            NetworkError::AuthenticationFailed => AUTH_FAILED_TEXT.to_owned(),
            NetworkError::NotFound { id } if id == "ssid" => NOT_FOUND_TEXT.to_owned(),
            other => format!("connect \u{b7} {other}"),
        })
    }

    /// Whether the last failure is one a new attempt can fix (not an
    /// unsupported security type or a missing adapter).
    fn retryable(&self) -> bool {
        !matches!(
            self.failure(),
            None | Some(NetworkError::Unsupported { .. })
                | Some(NetworkError::NoAdapter)
                | Some(NetworkError::Unauthorized { .. })
        )
    }

    /// Whether Retry is offered: for a listed access point, or for the
    /// network the last join by name named.
    pub fn can_retry(&self) -> bool {
        self.retry_target().is_some() || (self.retryable() && self.last_hidden.is_some())
    }

    /// The access point Retry would reconnect to, while it is still listed.
    /// Retry is offered after any failure a new attempt can fix (not an
    /// unsupported security type or a missing adapter).
    pub fn retry_target(&self) -> Option<&ApId> {
        if !self.retryable() {
            return None;
        }
        self.last_connect.as_ref().filter(|id| {
            self.net
                .as_ref()
                .is_some_and(|n| n.access_point(id).is_some())
        })
    }

    pub fn connectivity_label(&self) -> String {
        match self.net.as_ref().map(|n| &n.connectivity) {
            None => "connectivity \u{b7} unknown".to_owned(),
            Some(Connectivity::Portal { url: Some(url) }) => {
                format!("connectivity \u{b7} portal \u{b7} {url}")
            }
            Some(c) => format!("connectivity \u{b7} {}", c.name()),
        }
    }

    pub fn ip_lines(&self) -> Vec<String> {
        match self.net.as_ref().and_then(|n| n.ip.as_ref()) {
            None => vec!["ip \u{b7} none".to_owned()],
            Some(ip) => vec![
                format!(
                    "ip \u{b7} {} \u{b7} {}",
                    if ip.addresses.is_empty() {
                        "none".to_owned()
                    } else {
                        ip.addresses.join(", ")
                    },
                    if ip.dhcp { "dhcp" } else { "static" }
                ),
                format!(
                    "gateway \u{b7} {} \u{b7} dns \u{b7} {}",
                    ip.gateway.as_deref().unwrap_or("none"),
                    if ip.dns.is_empty() {
                        "none".to_owned()
                    } else {
                        ip.dns.join(", ")
                    }
                ),
            ],
        }
    }

    /// Joinable networks, strongest first, one row per name: a network
    /// served by several access points (bands, mesh nodes) is listed once,
    /// through its strongest access point. Hidden networks (an empty or
    /// all-NUL SSID) are left out: a nameless row could only ever fail, so
    /// they are joined through Add network (`join-hidden`) instead, and
    /// [`Model::hidden_count`] reports how many beacons were omitted.
    pub fn access_points(&self) -> Vec<&AccessPoint> {
        let Some(net) = &self.net else {
            return Vec::new();
        };
        let mut aps: Vec<&AccessPoint> = net
            .access_points
            .iter()
            .filter(|ap| !is_hidden(ap))
            .collect();
        aps.sort_by(|a, b| {
            b.signal_percent
                .cmp(&a.signal_percent)
                .then_with(|| a.id.as_str().cmp(b.id.as_str()))
        });
        let mut seen = std::collections::HashSet::new();
        aps.retain(|ap| seen.insert(ap.ssid.as_bytes().to_vec()));
        aps
    }

    pub fn hidden_count(&self) -> usize {
        self.net.as_ref().map_or(0, |n| {
            n.access_points.iter().filter(|ap| is_hidden(ap)).count()
        })
    }

    pub fn profiles(&self) -> Vec<&SavedProfile> {
        self.net
            .as_ref()
            .map(|n| n.profiles.iter().collect())
            .unwrap_or_default()
    }

    pub fn connected_ap(&self) -> Option<&ApId> {
        self.net.as_ref().and_then(|n| n.connection.connected_ap())
    }

    pub fn controllable(&self) -> Result<(), String> {
        if self.net_link == Link::Unavailable || self.net.is_none() {
            return Err("network \u{b7} unavailable".to_owned());
        }
        if !self.net.as_ref().is_some_and(|n| n.adapter.is_present()) {
            return Err("adapter \u{b7} absent".to_owned());
        }
        Ok(())
    }

    pub fn cellular_on(&self) -> bool {
        self.tele
            .as_ref()
            .is_some_and(|t| t.radio == RadioPower::On)
    }

    /// Airplane mode: both radios off. Unknown daemons count as off.
    pub fn airplane(&self) -> bool {
        !self.radio_enabled() && !self.cellular_on()
    }

    pub fn cellular_label(&self) -> String {
        match &self.tele {
            None => format!("cellular \u{b7} {}", self.tele_link.name()),
            Some(t) => format!(
                "cellular \u{b7} radio {} \u{b7} modem {}",
                t.radio.name(),
                t.modem.name()
            ),
        }
    }

    pub fn cellular_data_on(&self) -> bool {
        self.tele.as_ref().is_some_and(|t| {
            matches!(
                &t.data,
                DataBearer::Connected { .. } | DataBearer::Connecting
            )
        })
    }

    pub fn cellular_data_label(&self) -> String {
        match &self.tele {
            None => format!("cellular data · {}", self.tele_link.name()),
            Some(t) => format!("cellular data · {}", t.data.name()),
        }
    }

    // -- intents -------------------------------------------------------------

    pub fn show(&mut self, page: Page) {
        if page != self.page {
            self.bt_view = crate::bluetooth::BtView::Main;
            self.bt_forget_confirm = false;
            self.add_network = None;
        }
        self.page = page;
        self.last_error = None;
        self.forget_confirm = None;
        self.brightness_drag = None;
        self.volume_drag = None;
        self.kbd_try_focused = false;
        self.request_devices_if_shown();
    }

    /// One-line state of a root-page category.
    pub fn section_summary(&self, page: Page) -> String {
        match page {
            Page::Root => String::new(),
            Page::Network => match self.net.as_ref() {
                None => self.network_label(),
                Some(_) if !self.radio_enabled() => self.radio_label(),
                Some(_) => self.connection_label(),
            },
            Page::Bluetooth => self.bluetooth_summary(),
            Page::Mobile => self.mobile_summary(),
            Page::Display => self.display_summary(),
            Page::Sound => self.sound_summary(),
            Page::Keyboard => self.keyboard_summary(),
            Page::Radios => format!(
                "airplane mode {}",
                if self.airplane() { "on" } else { "off" }
            ),
            Page::About => self.target.clone(),
        }
    }

    pub fn toggle_radio(&mut self) {
        match self.controllable() {
            Ok(()) => {
                let enabled = !self.radio_enabled();
                self.pending.push(Action::SetRadio(enabled));
            }
            Err(reason) => self.last_error = Some(reason),
        }
    }

    pub fn scan(&mut self) {
        match self.controllable() {
            // Names the actual radio state: `disabled`, or the typed
            // `unavailable · reason` an ethernet-like adapter reports.
            Ok(()) if !self.radio_enabled() => self.last_error = Some(self.radio_label()),
            Ok(()) => self.pending.push(Action::Scan),
            Err(reason) => self.last_error = Some(reason),
        }
    }

    /// Tapping an access point: disconnect when connected, ask for a
    /// passphrase when the security needs one, else connect.
    pub fn select_ap(&mut self, id: &ApId) {
        if let Err(reason) = self.controllable() {
            self.last_error = Some(reason);
            return;
        }
        if self.connected_ap() == Some(id) {
            self.pending.push(Action::Disconnect);
            return;
        }
        let Some(ap) = self.net.as_ref().and_then(|n| n.access_point(id)).cloned() else {
            return;
        };
        let saved = self
            .profiles()
            .iter()
            .any(|p| p.ssid == ap.ssid && p.security == ap.security);
        // After a failed join the saved secret is suspect: ask again
        // instead of silently retrying with it.
        if ap.security.needs_passphrase() && (!saved || self.failure().is_some()) {
            self.passphrase_for = Some(id.clone());
            self.passphrase.clear();
            self.passphrase_shown = false;
            self.last_error = None;
        } else {
            self.begin_connect(id.clone(), None);
        }
    }

    fn begin_connect(&mut self, ap: ApId, passphrase: Option<String>) {
        self.last_connect = Some(ap.clone());
        self.last_hidden = None;
        self.connect_error = None;
        self.last_error = None;
        self.pending.push(Action::Connect { ap, passphrase });
    }

    /// Retry after an authentication failure: ask for the passphrase again
    /// (a saved one was evidently wrong), or reconnect an open network.
    pub fn retry_connect(&mut self) {
        if self.retry_target().is_none()
            && self.retryable()
            && let Some((ssid, security)) = self.last_hidden.clone()
        {
            // A join by name: reopen the form with the name and security,
            // asking for the passphrase again.
            self.reopen_add_network(ssid, security);
            return;
        }
        let Some(id) = self.retry_target().cloned() else {
            return;
        };
        let security = self
            .net
            .as_ref()
            .and_then(|n| n.access_point(&id))
            .map(|ap| ap.security);
        if security.is_some_and(SecurityType::needs_passphrase) {
            self.passphrase_for = Some(id);
            self.passphrase.clear();
            self.last_error = None;
        } else {
            self.begin_connect(id, None);
        }
    }

    /// Records a typed error the connect request returned.
    pub fn connect_failed(&mut self, error: NetworkError) {
        self.connect_error = Some(error);
    }

    pub fn submit_passphrase(&mut self) -> Result<(), String> {
        let Some(ap) = self.passphrase_for.clone() else {
            return Err("no access point selected".to_owned());
        };
        if self.passphrase.chars().count() < 8 {
            return Err("passphrase \u{b7} shorter than 8 characters".to_owned());
        }
        let passphrase = std::mem::take(&mut self.passphrase);
        self.begin_connect(ap, Some(passphrase));
        self.passphrase_for = None;
        Ok(())
    }

    pub fn cancel_passphrase(&mut self) {
        self.passphrase_for = None;
        self.passphrase.clear();
    }

    /// Tapping a saved profile asks for confirmation first, like
    /// Contacts' delete; nothing is sent until [`Model::confirm_forget`].
    pub fn forget(&mut self, id: &ProfileId) {
        match self.controllable() {
            Ok(()) => {
                self.forget_confirm = Some(id.clone());
                self.last_error = None;
            }
            Err(reason) => self.last_error = Some(reason),
        }
    }

    pub fn confirm_forget(&mut self) {
        let Some(id) = self.forget_confirm.take() else {
            return;
        };
        match self.controllable() {
            Ok(()) => self.pending.push(Action::Forget(id)),
            Err(reason) => self.last_error = Some(reason),
        }
    }

    pub fn cancel_forget(&mut self) {
        self.forget_confirm = None;
    }

    /// The SSID of the profile awaiting Forget confirmation.
    pub fn forget_ssid(&self) -> Option<String> {
        let id = self.forget_confirm.as_ref()?;
        self.profiles()
            .into_iter()
            .find(|p| &p.id == id)
            .map(|p| p.ssid.to_string())
    }

    pub fn toggle_airplane(&mut self) {
        let on = !self.airplane();
        self.pending.push(Action::SetRadio(!on));
        if on && self.cellular_data_on() {
            self.pending.push(Action::SetCellularData(false));
        }
        self.pending.push(Action::SetRadioPower(if on {
            RadioPower::Off
        } else {
            RadioPower::On
        }));
    }

    pub fn toggle_cellular(&mut self) {
        let power = if self.cellular_on() {
            RadioPower::Off
        } else {
            RadioPower::On
        };
        if power == RadioPower::Off && self.cellular_data_on() {
            self.pending.push(Action::SetCellularData(false));
        }
        self.pending.push(Action::SetRadioPower(power));
    }

    pub fn back(&mut self) {
        if self.forget_confirm.is_some() {
            self.cancel_forget();
        } else if self.add_network.is_some() {
            self.cancel_add_network();
        } else if self.passphrase_for.is_some() {
            self.cancel_passphrase();
        } else if self.page == Page::Bluetooth && self.bluetooth_back() {
            // Handled inside the Bluetooth flows (prompt, Pair, Device).
        } else if self.keyboard_back() {
            // Closed the Keyboard page's scratch field.
        } else if self.page != Page::Root {
            self.show(Page::Root);
        } else {
            self.pending.push(Action::Back);
        }
    }
}

/// Canned telephony snapshot for the host preview: a ready mock modem
/// with a SIM, registered, data supported but not connected.
pub fn preview_telephony() -> ferrix_telephony_api::TelephonyState {
    use ferrix_telephony_api::{
        AccessTechnology, ModemState, Operator, Registration, Signal, SimState, TelephonyFeature,
    };
    let mut tele = ferrix_telephony_api::TelephonyState::new();
    tele.modem = ModemState::Ready;
    tele.sim = SimState::Ready;
    tele.registration = Registration::Registered { roaming: false };
    tele.operator = Some(Operator {
        name: "Ferrix Mock".to_owned(),
        mcc_mnc: None,
    });
    tele.access = AccessTechnology::Lte;
    tele.signal = Signal::bars(3);
    tele.radio = RadioPower::On;
    tele.data = DataBearer::Disconnected;
    tele.capabilities = [TelephonyFeature::Data, TelephonyFeature::RadioPower]
        .into_iter()
        .collect();
    tele
}

/// Canned network snapshot for the host preview.
pub fn preview_network(connected: bool) -> NetworkState {
    use ferrix_network_api::{AdapterKind, IpConfig, Ssid};
    // The empty name is a hidden network's beacon (joined via Add network).
    let aps = [
        ("ferrix-lab", 78, SecurityType::Wpa2Psk),
        ("guest-open", 54, SecurityType::Open),
        ("lab-enterprise", 31, SecurityType::Enterprise),
        ("", 63, SecurityType::Wpa2Psk),
    ];
    let access_points: Vec<AccessPoint> = aps
        .iter()
        .enumerate()
        .map(|(i, (ssid, signal, security))| AccessPoint {
            id: ApId::new(format!("ap-{i}")),
            ssid: Ssid::from(*ssid),
            bssid_hash: format!("{:016x}", 0x1234_5678_9abc_def0_u64 + i as u64),
            signal_percent: *signal,
            security: *security,
            frequency_mhz: Some(2412 + i as u32 * 5),
        })
        .collect();
    NetworkState {
        schema_version: ferrix_network_api::SCHEMA_VERSION,
        adapter: AdapterPresence::Present {
            kind: AdapterKind::Wifi,
        },
        radio: RadioState::Enabled,
        scan: ScanState::Completed { count: 4 },
        connection: if connected {
            ConnectionState::Connected {
                ap: ApId::new("ap-0"),
            }
        } else {
            ConnectionState::Disconnected
        },
        ip: connected.then(|| IpConfig {
            addresses: vec!["10.0.2.15/24".to_owned()],
            gateway: Some("10.0.2.2".to_owned()),
            dns: vec!["10.0.2.3".to_owned()],
            dhcp: true,
        }),
        connectivity: if connected {
            Connectivity::Full
        } else {
            Connectivity::None
        },
        profiles: vec![SavedProfile {
            id: ProfileId::new("profile-lab"),
            ssid: Ssid::from("ferrix-lab"),
            security: SecurityType::Wpa2Psk,
            autoconnect: true,
        }],
        access_points,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrix_network_api::NetworkError;

    fn connected() -> Model {
        let mut m = Model::new();
        m.show(Page::Network);
        m.apply_network(Feed::Snapshot {
            state: preview_network(true),
            evidence: "mock".into(),
        });
        m
    }

    #[test]
    fn labels_follow_the_snapshot_and_typed_errors() {
        let mut m = Model::new();
        assert_eq!(m.adapter_label(), "adapter \u{b7} unknown");
        assert_eq!(
            m.controllable(),
            Err("network \u{b7} unavailable".to_owned())
        );
        m = connected();
        assert_eq!(m.adapter_label(), "adapter \u{b7} wifi");
        assert_eq!(m.connection_label(), "connected \u{b7} ferrix-lab");
        assert_eq!(m.connectivity_label(), "connectivity \u{b7} full");
        assert_eq!(m.ip_lines()[0], "ip \u{b7} 10.0.2.15/24 \u{b7} dhcp");
        assert_eq!(m.state_name(), "connected");
        assert_eq!(
            m.access_points()[0].ssid.to_string(),
            "ferrix-lab",
            "strongest first"
        );
        for error in [
            NetworkError::AuthenticationFailed,
            NetworkError::DhcpFailed,
            NetworkError::DnsFailed,
            NetworkError::RouteFailed,
            NetworkError::TimedOut,
        ] {
            m.apply_network(Feed::Event(NetworkEvent::ConnectionChanged {
                connection: ConnectionState::Failed {
                    error: error.clone(),
                },
            }));
            let expected = if error == NetworkError::AuthenticationFailed {
                AUTH_FAILED_TEXT.to_owned()
            } else {
                format!("connect \u{b7} {error}")
            };
            assert_eq!(m.connection_error(), Some(expected));
            let state = if error == NetworkError::AuthenticationFailed {
                "auth-failed"
            } else {
                "failed"
            };
            assert_eq!(m.state_name(), state);
        }
        for state in [
            ConnectionState::Associating,
            ConnectionState::Authenticating,
            ConnectionState::Configuring,
        ] {
            m.apply_network(Feed::Event(NetworkEvent::ConnectionChanged {
                connection: state.clone(),
            }));
            assert_eq!(m.connection_label(), state.name());
        }
        m.apply_network(Feed::Event(NetworkEvent::ConnectivityChanged {
            connectivity: Connectivity::Portal {
                url: Some("http://portal.invalid".into()),
            },
        }));
        assert!(m.connectivity_label().ends_with("portal.invalid"));
        m.apply_network(Feed::Event(NetworkEvent::AdapterChanged {
            adapter: AdapterPresence::Present {
                kind: ferrix_network_api::AdapterKind::EthernetLike,
            },
        }));
        assert_eq!(m.adapter_label(), "adapter \u{b7} ethernet-like");
    }

    #[test]
    fn controls_queue_actions_and_passphrase_flow() {
        let mut m = connected();
        m.toggle_radio();
        assert_eq!(m.pending.pop(), Some(Action::SetRadio(false)));
        m.scan();
        assert_eq!(m.pending.pop(), Some(Action::Scan));
        // Connected AP tap disconnects; open AP connects; secured AP asks.
        m.select_ap(&ApId::new("ap-0"));
        assert_eq!(m.pending.pop(), Some(Action::Disconnect));
        m.select_ap(&ApId::new("ap-1"));
        assert_eq!(
            m.pending.pop(),
            Some(Action::Connect {
                ap: ApId::new("ap-1"),
                passphrase: None
            })
        );
        m.select_ap(&ApId::new("ap-2"));
        assert_eq!(m.passphrase_for, Some(ApId::new("ap-2")));
        assert_eq!(m.state_name(), "passphrase");
        m.passphrase = "short".to_owned();
        assert!(m.submit_passphrase().is_err());
        m.passphrase = "mock-passphrase".to_owned();
        assert_eq!(m.submit_passphrase(), Ok(()));
        assert_eq!(
            m.pending.pop(),
            Some(Action::Connect {
                ap: ApId::new("ap-2"),
                passphrase: Some("mock-passphrase".into())
            })
        );
        assert!(m.passphrase.is_empty() && m.passphrase_for.is_none());
        // A saved profile's AP connects without asking.
        m.apply_network(Feed::Event(NetworkEvent::ConnectionChanged {
            connection: ConnectionState::Disconnected,
        }));
        m.select_ap(&ApId::new("ap-0"));
        assert_eq!(
            m.pending.pop(),
            Some(Action::Connect {
                ap: ApId::new("ap-0"),
                passphrase: None
            })
        );
        m.forget(&ProfileId::new("profile-lab"));
        assert!(m.pending.is_empty(), "forget waits for confirmation");
        m.confirm_forget();
        assert_eq!(
            m.pending.pop(),
            Some(Action::Forget(ProfileId::new("profile-lab")))
        );
        // Back cancels a passphrase prompt first, then pages, then yields.
        m.select_ap(&ApId::new("ap-2"));
        m.back();
        assert_eq!(m.passphrase_for, None);
        m.show(Page::About);
        m.back();
        assert_eq!(m.page, Page::Root, "a category returns to the list");
        assert_eq!(m.state_name(), "root");
        m.back();
        assert_eq!(m.pending, vec![Action::Back]);
        // Unavailable network refuses control with a typed reason.
        m.pending.clear();
        m.apply_network(Feed::Disconnected {
            reason: "gone".into(),
            retry_in: std::time::Duration::from_millis(100),
        });
        m.scan();
        assert!(m.pending.is_empty());
        assert_eq!(m.last_error.as_deref(), Some("network \u{b7} unavailable"));
        m.apply_network(Feed::Snapshot {
            state: preview_network(false),
            evidence: "mock".into(),
        });
        assert_eq!(m.net_link, Link::Reconnected);
        m.apply_network(Feed::Event(NetworkEvent::RadioChanged {
            radio: RadioState::Disabled,
        }));
        m.scan();
        assert_eq!(m.last_error.as_deref(), Some("radio \u{b7} disabled"));
        m.apply_network(Feed::Event(NetworkEvent::RadioChanged {
            radio: RadioState::Unavailable {
                reason: "ethernet-like".into(),
            },
        }));
        m.scan();
        assert_eq!(
            m.last_error.as_deref(),
            Some("radio \u{b7} unavailable \u{b7} ethernet-like")
        );
    }

    #[test]
    fn airplane_mode_drives_both_radios() {
        let mut m = connected();
        let mut tele = ferrix_telephony_api::TelephonyState::new();
        tele.radio = RadioPower::On;
        m.apply_telephony(Update::Snapshot(
            ferrix_telephony_protocol::StateView::Redacted(tele.redacted()),
        ));
        assert!(!m.airplane());
        assert!(m.cellular_on());
        m.toggle_airplane();
        assert_eq!(
            m.pending,
            vec![
                Action::SetRadio(false),
                Action::SetRadioPower(RadioPower::Off)
            ]
        );
        m.pending.clear();
        m.apply_network(Feed::Event(NetworkEvent::RadioChanged {
            radio: RadioState::Disabled,
        }));
        m.apply_telephony(Update::Event(TeleEvent::Redacted(
            ferrix_telephony_api::RedactedTelephonyEvent::RadioPowerChanged {
                radio: RadioPower::Off,
            },
        )));
        assert!(m.airplane());
        assert_eq!(
            m.state_name(),
            "connected",
            "network page names the connection"
        );
        m.page = Page::Radios;
        assert_eq!(m.state_name(), "airplane-on");
        m.toggle_cellular();
        assert_eq!(m.pending, vec![Action::SetRadioPower(RadioPower::On)]);
        m.apply_telephony(Update::Disconnected);
        assert_eq!(m.telephony_label(), "telephony \u{b7} unavailable");
        assert_eq!(Page::parse("about"), Some(Page::About));
    }

    #[test]
    fn cellular_data_is_separate_from_radio_power() {
        let mut m = Model::new();
        let mut tele = ferrix_telephony_api::TelephonyState::new();
        tele.radio = RadioPower::On;
        tele.data = ferrix_telephony_api::DataBearer::Connected {
            interface: Some("rmnet0".into()),
        };
        m.apply_telephony(Update::Snapshot(
            ferrix_telephony_protocol::StateView::Redacted(tele.redacted()),
        ));
        assert!(m.cellular_data_on());
        assert_eq!(m.cellular_data_label(), "cellular data \u{b7} connected");
        m.toggle_cellular_data();
        assert_eq!(m.pending, vec![Action::SetCellularData(false)]);
        m.pending.clear();
        m.toggle_cellular();
        assert_eq!(
            m.pending,
            vec![
                Action::SetCellularData(false),
                Action::SetRadioPower(RadioPower::Off)
            ]
        );
    }

    #[test]
    fn target_apn_is_optional_and_invalid_values_are_dropped() {
        assert_eq!(data_profile_from_env(None), DataProfile::default());
        assert_eq!(data_profile_from_env(Some("")), DataProfile::default());
        assert_eq!(
            data_profile_from_env(Some("internet.example")),
            DataProfile {
                apn: Some("internet.example".to_owned())
            }
        );
        assert_eq!(
            data_profile_from_env(Some("has whitespace")),
            DataProfile::default()
        );
    }

    #[test]
    fn forget_needs_confirmation_and_back_cancels() {
        let mut m = connected();
        let id = ProfileId::new("profile-lab");
        m.forget(&id);
        assert!(m.pending.is_empty());
        assert_eq!(m.state_name(), "confirm-forget");
        assert_eq!(m.forget_ssid().as_deref(), Some("ferrix-lab"));
        m.back();
        assert_eq!(m.forget_confirm, None);
        assert!(m.pending.is_empty(), "back cancels, never forgets");
        m.forget(&id);
        m.cancel_forget();
        assert!(m.pending.is_empty());
        m.forget(&id);
        m.confirm_forget();
        assert_eq!(m.pending, vec![Action::Forget(id.clone())]);
        assert_eq!(m.forget_confirm, None);
        // A profile that disappears drops its pending confirmation.
        m.forget(&id);
        let mut gone = preview_network(true);
        gone.profiles.clear();
        m.apply_network(Feed::Snapshot {
            state: gone,
            evidence: "mock".into(),
        });
        assert_eq!(m.forget_confirm, None);
    }

    #[test]
    fn authentication_failure_reads_plainly_and_retry_reprompts() {
        let mut m = connected();
        m.apply_network(Feed::Event(NetworkEvent::ConnectionChanged {
            connection: ConnectionState::Disconnected,
        }));
        // ferrix-lab is saved, so it connects without asking.
        m.select_ap(&ApId::new("ap-0"));
        m.pending.clear();
        m.apply_network(Feed::Event(NetworkEvent::ConnectionChanged {
            connection: ConnectionState::Failed {
                error: NetworkError::AuthenticationFailed,
            },
        }));
        assert!(m.auth_failed());
        assert_eq!(m.connection_error().as_deref(), Some(AUTH_FAILED_TEXT));
        assert_eq!(m.state_name(), "auth-failed");
        assert_eq!(m.retry_target(), Some(&ApId::new("ap-0")));
        m.retry_connect();
        assert_eq!(
            m.passphrase_for,
            Some(ApId::new("ap-0")),
            "retry asks for the passphrase even for a saved profile"
        );
        m.passphrase = "another-pass".to_owned();
        m.submit_passphrase().unwrap();
        assert_eq!(
            m.pending.pop(),
            Some(Action::Connect {
                ap: ApId::new("ap-0"),
                passphrase: Some("another-pass".into())
            })
        );
        // A typed error returned by the request itself reads the same way.
        m.apply_network(Feed::Event(NetworkEvent::ConnectionChanged {
            connection: ConnectionState::Disconnected,
        }));
        m.connect_failed(NetworkError::AuthenticationFailed);
        assert_eq!(m.connection_error().as_deref(), Some(AUTH_FAILED_TEXT));
        // Other failures surface the daemon's error text, no Retry.
        m.connect_failed(NetworkError::Unsupported {
            feature: "wpa3-sae".into(),
        });
        assert!(!m.auth_failed());
        assert_eq!(m.retry_target(), None);
        assert_eq!(
            m.connection_error().as_deref(),
            Some("connect \u{b7} wpa3-sae is not supported by this backend")
        );
        // Connecting clears the request error.
        m.apply_network(Feed::Event(NetworkEvent::ConnectionChanged {
            connection: ConnectionState::Connected {
                ap: ApId::new("ap-0"),
            },
        }));
        assert_eq!(m.connection_error(), None);
    }

    #[test]
    fn a_network_with_several_access_points_is_listed_once_by_its_strongest() {
        let mut state = preview_network(false);
        let first = state.access_points[0].clone();
        let mut weaker = first.clone();
        weaker.id = ApId::new("same-name-weaker");
        weaker.signal_percent = first.signal_percent.saturating_sub(30);
        state.access_points.push(weaker);
        let mut m = Model::new();
        m.apply_network(Feed::Snapshot {
            state,
            evidence: "mock".into(),
        });
        let rows: Vec<_> = m
            .access_points()
            .iter()
            .filter(|ap| ap.ssid == first.ssid)
            .map(|ap| ap.id.as_str().to_owned())
            .collect();
        assert_eq!(rows, vec![first.id.as_str().to_owned()]);
    }

    #[test]
    fn hidden_networks_are_counted_not_listed() {
        let mut state = preview_network(false);
        for (i, ssid) in [b"".to_vec(), vec![0, 0, 0]].into_iter().enumerate() {
            state.access_points.push(AccessPoint {
                id: ApId::new(format!("hidden-{i}")),
                ssid: ferrix_network_api::Ssid::new(ssid),
                bssid_hash: "00".into(),
                signal_percent: 99,
                security: SecurityType::Wpa2Psk,
                frequency_mhz: None,
            });
        }
        let mut m = Model::new();
        m.apply_network(Feed::Snapshot {
            state,
            evidence: "mock".into(),
        });
        assert_eq!(m.access_points().len(), 3, "hidden rows are not drawn");
        assert!(
            m.access_points()
                .iter()
                .all(|ap| !ap.ssid.as_bytes().is_empty())
        );
        // Two added here plus the preview's own hidden beacon.
        assert_eq!(m.hidden_count(), 3);
    }

    #[test]
    fn a_failed_join_stays_visible_after_the_backend_settles_on_disconnected() {
        let mut m = connected();
        m.apply_network(Feed::Event(NetworkEvent::ConnectionChanged {
            connection: ConnectionState::Disconnected,
        }));
        // Saved profile: the first attempt reuses the stored secret.
        m.select_ap(&ApId::new("ap-0"));
        assert!(matches!(
            m.pending.pop(),
            Some(Action::Connect {
                passphrase: None,
                ..
            })
        ));
        for connection in [
            ConnectionState::Associating,
            ConnectionState::Failed {
                error: NetworkError::Backend {
                    reason: "device state reason 99".into(),
                },
            },
            ConnectionState::Disconnected,
        ] {
            m.apply_network(Feed::Event(NetworkEvent::ConnectionChanged { connection }));
        }
        assert_eq!(m.state_name(), "failed", "not a silent disconnect");
        let text = m.connection_error().expect("failure is shown");
        assert!(text.contains("device state reason 99"), "{text}");
        assert_eq!(m.retry_target(), Some(&ApId::new("ap-0")));
        // Retry, or tapping the saved network again, asks for the secret.
        m.retry_connect();
        assert_eq!(m.passphrase_for, Some(ApId::new("ap-0")));
        m.cancel_passphrase();
        m.select_ap(&ApId::new("ap-0"));
        assert_eq!(m.passphrase_for, Some(ApId::new("ap-0")));
        m.cancel_passphrase();
        // A snapshot taken after the failure (app opened later) shows it too.
        let mut fresh = Model::new();
        fresh.show(Page::Network);
        let mut state = preview_network(false);
        state.connection = ConnectionState::Failed {
            error: NetworkError::AuthenticationFailed,
        };
        fresh.apply_network(Feed::Snapshot {
            state,
            evidence: "mock".into(),
        });
        assert_eq!(fresh.state_name(), "auth-failed");
        assert_eq!(fresh.connection_error().as_deref(), Some(AUTH_FAILED_TEXT));
        // Success clears it.
        m.apply_network(Feed::Event(NetworkEvent::ConnectionChanged {
            connection: ConnectionState::Connected {
                ap: ApId::new("ap-0"),
            },
        }));
        assert_eq!(m.connection_error(), None);
        assert_eq!(m.state_name(), "connected");
    }

    #[test]
    fn telephony_evidence_comes_from_the_live_daemon() {
        let mut m = Model::new();
        assert_eq!(m.tele_evidence_label(), "no snapshot \u{b7} connecting");
        let mut tele = ferrix_telephony_api::TelephonyState::new();
        tele.radio = RadioPower::On;
        m.apply_telephony(Update::Snapshot(
            ferrix_telephony_protocol::StateView::Redacted(tele.redacted()),
        ));
        assert_eq!(
            m.tele_evidence_label(),
            "connected \u{b7} label not reported"
        );
        m.set_tele_evidence("QEMU");
        assert_eq!(m.tele_evidence_label(), "QEMU");
        m.apply_telephony(Update::Disconnected);
        assert_eq!(m.tele_evidence_label(), "QEMU", "last daemon's label kept");
    }
}
