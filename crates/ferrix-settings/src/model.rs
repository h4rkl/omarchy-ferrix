//! Settings view model: the network snapshot from `ferrix-networkd`, the
//! redacted telephony snapshot for the Radios page, passphrase entry, and
//! typed reasons for everything that cannot be done. Pure state.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use ferrix_network_api::{
    AccessPoint, AdapterPresence, ApId, ConnectionState, Connectivity, NetworkEvent, NetworkState,
    ProfileId, RadioState, SavedProfile, ScanState, SecurityType,
};
use ferrix_network_protocol::Feed;
use ferrix_telephony_api::{RadioPower, RedactedTelephonyState};
use ferrix_telephony_protocol::{Event as TeleEvent, Update};
use ferrix_ui::osk::OskState;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Network,
    Radios,
    About,
}

impl Page {
    pub const ALL: [Page; 3] = [Page::Network, Page::Radios, Page::About];

    pub fn name(self) -> &'static str {
        match self {
            Page::Network => "network",
            Page::Radios => "radios",
            Page::About => "about",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Page::Network => "Network",
            Page::Radios => "Radios",
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
    Disconnect,
    Forget(ProfileId),
    /// Telephony radio power (airplane mode's cellular half).
    SetRadioPower(RadioPower),
    Back,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Model {
    pub page: Page,
    pub net_link: Link,
    pub net: Option<NetworkState>,
    pub net_evidence: Option<String>,
    pub tele_link: Link,
    pub tele: Option<RedactedTelephonyState>,
    pub tele_evidence: Option<String>,
    /// An access point awaiting a passphrase.
    pub passphrase_for: Option<ApId>,
    pub passphrase: String,
    pub osk: OskState,
    pub pending: Vec<Action>,
    pub last_error: Option<String>,
    pub target: String,
    net_lost_once: bool,
    tele_lost_once: bool,
}

impl Default for Model {
    fn default() -> Self {
        Self::new()
    }
}

impl Model {
    pub fn new() -> Self {
        Self {
            page: Page::Network,
            net_link: Link::Connecting,
            net: None,
            net_evidence: None,
            tele_link: Link::Connecting,
            tele: None,
            tele_evidence: None,
            passphrase_for: None,
            passphrase: String::new(),
            osk: OskState::default(),
            pending: Vec::new(),
            last_error: None,
            target: "unknown".to_owned(),
            net_lost_once: false,
            tele_lost_once: false,
        }
    }

    pub fn state_name(&self) -> String {
        match self.page {
            Page::Network => match (&self.passphrase_for, &self.net) {
                (Some(_), _) => "passphrase".to_owned(),
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

    // -- feeds ---------------------------------------------------------------

    pub fn apply_network(&mut self, feed: Feed) -> bool {
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
                let ssid = self
                    .net
                    .as_ref()
                    .and_then(|n| n.access_point(ap))
                    .map(|ap| ap.ssid.to_string())
                    .unwrap_or_else(|| ap.to_string());
                format!("connected \u{b7} {ssid}")
            }
            Some(ConnectionState::Failed { error }) => format!("failed \u{b7} {}", error.name()),
            Some(state) => state.name().to_owned(),
        }
    }

    /// The typed connection error, if the last attempt failed.
    pub fn connection_error(&self) -> Option<String> {
        match self.net.as_ref().map(|n| &n.connection) {
            Some(ConnectionState::Failed { error }) => {
                Some(format!("connect \u{b7} {}", error.name()))
            }
            _ => None,
        }
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

    pub fn access_points(&self) -> Vec<&AccessPoint> {
        let Some(net) = &self.net else {
            return Vec::new();
        };
        let mut aps: Vec<&AccessPoint> = net.access_points.iter().collect();
        aps.sort_by(|a, b| {
            b.signal_percent
                .cmp(&a.signal_percent)
                .then_with(|| a.id.as_str().cmp(b.id.as_str()))
        });
        aps
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

    // -- intents -------------------------------------------------------------

    pub fn show(&mut self, page: Page) {
        self.page = page;
        self.last_error = None;
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
        if ap.security.needs_passphrase() && !saved {
            self.passphrase_for = Some(id.clone());
            self.passphrase.clear();
            self.last_error = None;
        } else {
            self.pending.push(Action::Connect {
                ap: id.clone(),
                passphrase: None,
            });
        }
    }

    pub fn submit_passphrase(&mut self) -> Result<(), String> {
        let Some(ap) = self.passphrase_for.clone() else {
            return Err("no access point selected".to_owned());
        };
        if self.passphrase.chars().count() < 8 {
            return Err("passphrase \u{b7} shorter than 8 characters".to_owned());
        }
        self.pending.push(Action::Connect {
            ap,
            passphrase: Some(std::mem::take(&mut self.passphrase)),
        });
        self.passphrase_for = None;
        Ok(())
    }

    pub fn cancel_passphrase(&mut self) {
        self.passphrase_for = None;
        self.passphrase.clear();
    }

    pub fn forget(&mut self, id: &ProfileId) {
        match self.controllable() {
            Ok(()) => self.pending.push(Action::Forget(id.clone())),
            Err(reason) => self.last_error = Some(reason),
        }
    }

    pub fn toggle_airplane(&mut self) {
        let on = !self.airplane();
        self.pending.push(Action::SetRadio(!on));
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
        self.pending.push(Action::SetRadioPower(power));
    }

    pub fn back(&mut self) {
        if self.passphrase_for.is_some() {
            self.cancel_passphrase();
        } else if self.page != Page::Network {
            self.show(Page::Network);
        } else {
            self.pending.push(Action::Back);
        }
    }
}

/// Canned network snapshot for the host preview.
pub fn preview_network(connected: bool) -> NetworkState {
    use ferrix_network_api::{AdapterKind, IpConfig, Ssid};
    let aps = [
        ("ferrix-lab", 78, SecurityType::Wpa2Psk),
        ("guest-open", 54, SecurityType::Open),
        ("lab-enterprise", 31, SecurityType::Enterprise),
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
        scan: ScanState::Completed { count: 3 },
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
            assert_eq!(
                m.connection_error(),
                Some(format!("connect \u{b7} {}", error.name()))
            );
            assert_eq!(m.state_name(), "failed");
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
        assert_eq!(m.page, Page::Network);
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
}
