//! Status-bar summaries with an explicit source: `ferrix-telephonyd` and
//! `ferrix-networkd` when reachable, else the coarse `ferrix-hardware-api`
//! summary the core publishes (ADR-001/002: "summary only; the service is
//! authoritative"). Pure functions so the fallback rule is unit-tested.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use ferrix_hardware_api::{HardwareState, ModemRegistration, WifiState};
use ferrix_network_api::{Connectivity, NetworkState};
use ferrix_telephony_api::RedactedTelephonyState;

/// Where a summary's values came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Service,
    HardwareApi,
}

impl Source {
    pub fn name(self) -> &'static str {
        match self {
            Source::Service => "service",
            Source::HardwareApi => "hardware-api",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TelephonySummary {
    /// 0..=4 signal bars.
    pub bars: u8,
    pub registered: bool,
    pub registration: String,
    pub sim: String,
    pub source: Source,
}

impl Default for TelephonySummary {
    fn default() -> Self {
        Self {
            bars: 0,
            registered: false,
            registration: "unknown".to_owned(),
            sim: "unknown".to_owned(),
            source: Source::HardwareApi,
        }
    }
}

/// Connectivity as the status glyph shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reach {
    /// Radio off.
    Off,
    /// Radio on, no link.
    Disconnected,
    /// Linked without a verified route out.
    Local,
    Portal,
    Limited,
    Full,
}

impl Reach {
    pub fn name(self) -> &'static str {
        match self {
            Reach::Off => "off",
            Reach::Disconnected => "disconnected",
            Reach::Local => "local",
            Reach::Portal => "portal",
            Reach::Limited => "limited",
            Reach::Full => "full",
        }
    }

    /// Arcs lit on the Wi-Fi glyph (of three).
    pub fn arcs(self) -> u8 {
        match self {
            Reach::Off | Reach::Disconnected => 0,
            Reach::Local | Reach::Portal => 1,
            Reach::Limited => 2,
            Reach::Full => 3,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetworkSummary {
    pub wifi_on: bool,
    pub connected: bool,
    pub reach: Reach,
    /// `wifi`, `ethernet-like`, or `absent`.
    pub adapter: String,
    pub source: Source,
}

impl Default for NetworkSummary {
    fn default() -> Self {
        Self {
            wifi_on: false,
            connected: false,
            reach: Reach::Off,
            adapter: "unknown".to_owned(),
            source: Source::HardwareApi,
        }
    }
}

/// Telephony summary: the service snapshot when present, else the
/// hardware-api matrix (registered means four bars, searching one, else none).
pub fn telephony_summary(
    service: Option<&RedactedTelephonyState>,
    hw: &HardwareState,
) -> TelephonySummary {
    match service {
        Some(state) => TelephonySummary {
            bars: state.signal.bars.min(4),
            registered: state.registration.is_registered(),
            registration: state.registration.name().to_owned(),
            sim: state.sim.name().to_owned(),
            source: Source::Service,
        },
        None => TelephonySummary {
            bars: match hw.telephony.registration {
                ModemRegistration::Registered => 4,
                ModemRegistration::Searching => 1,
                ModemRegistration::Denied | ModemRegistration::Unregistered => 0,
            },
            registered: hw.telephony.registration == ModemRegistration::Registered,
            registration: hw.telephony.registration.name().to_owned(),
            sim: hw.telephony.sim.name().to_owned(),
            source: Source::HardwareApi,
        },
    }
}

/// Network summary: the service snapshot when present, else the three-state
/// hardware-api Wi-Fi summary (connected is assumed fully reachable).
pub fn network_summary(service: Option<&NetworkState>, hw: &HardwareState) -> NetworkSummary {
    match service {
        Some(state) => {
            let wifi_on = state.radio.is_enabled();
            let connected = state.connection.is_connected();
            let reach = if !wifi_on {
                Reach::Off
            } else if !connected {
                Reach::Disconnected
            } else {
                match state.connectivity {
                    Connectivity::None => Reach::Local,
                    Connectivity::Local => Reach::Local,
                    Connectivity::Portal { .. } => Reach::Portal,
                    Connectivity::Limited => Reach::Limited,
                    Connectivity::Full => Reach::Full,
                }
            };
            NetworkSummary {
                wifi_on,
                connected,
                reach,
                adapter: match &state.adapter {
                    ferrix_network_api::AdapterPresence::Absent => "absent".to_owned(),
                    ferrix_network_api::AdapterPresence::Present { kind } => kind.name().to_owned(),
                },
                source: Source::Service,
            }
        }
        None => {
            let (wifi_on, connected, reach) = match hw.network.wifi {
                WifiState::Connected => (true, true, Reach::Full),
                WifiState::Disconnected => (true, false, Reach::Disconnected),
                WifiState::Disabled => (false, false, Reach::Off),
            };
            NetworkSummary {
                wifi_on,
                connected,
                reach,
                adapter: "unknown".to_owned(),
                source: Source::HardwareApi,
            }
        }
    }
}

/// Where the Wi-Fi toggle should go: the network service when it is
/// reachable, else the core's product command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WifiRoute {
    Networkd,
    Core,
}

pub fn wifi_route(networkd_reachable: bool) -> WifiRoute {
    if networkd_reachable {
        WifiRoute::Networkd
    } else {
        WifiRoute::Core
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrix_network_api::{AdapterKind, AdapterPresence, ApId, ConnectionState, RadioState};
    use ferrix_telephony_api::{Registration, Signal, SimState, TelephonyState};

    #[test]
    fn telephony_prefers_the_service_and_falls_back_to_the_matrix() {
        let hw = crate::test_state();
        let fallback = telephony_summary(None, &hw);
        assert_eq!(fallback.source, Source::HardwareApi);
        assert_eq!(
            (fallback.bars, fallback.registered),
            (4, true),
            "profile: registered"
        );
        assert_eq!(fallback.sim, "present");
        let mut hw2 = hw;
        hw2.telephony.registration = ModemRegistration::Searching;
        assert_eq!(telephony_summary(None, &hw2).bars, 1);
        hw2.telephony.registration = ModemRegistration::Denied;
        assert_eq!(telephony_summary(None, &hw2).bars, 0);

        let mut state = TelephonyState::new();
        state.signal = Signal::bars(2);
        state.registration = Registration::Searching;
        state.sim = SimState::Ready;
        let service = telephony_summary(Some(&state.redacted()), &hw);
        assert_eq!(service.source, Source::Service);
        assert_eq!((service.bars, service.registered), (2, false));
        assert_eq!(service.sim, "ready");
        assert_eq!(service.registration, "searching");
    }

    #[test]
    fn network_prefers_the_service_and_maps_connectivity_to_arcs() {
        let hw = crate::test_state();
        let fallback = network_summary(None, &hw);
        assert_eq!(
            (fallback.wifi_on, fallback.connected, fallback.reach),
            (true, true, Reach::Full)
        );
        assert_eq!(fallback.source, Source::HardwareApi);
        let mut hw2 = hw;
        hw2.network.wifi = WifiState::Disabled;
        assert_eq!(network_summary(None, &hw2).reach, Reach::Off);

        let mut state = NetworkState {
            adapter: AdapterPresence::Present {
                kind: AdapterKind::EthernetLike,
            },
            radio: RadioState::Enabled,
            connection: ConnectionState::Connected {
                ap: ApId::new("ap-0"),
            },
            connectivity: Connectivity::Portal { url: None },
            ..NetworkState::default()
        };
        let service = network_summary(Some(&state), &hw);
        assert_eq!(service.source, Source::Service);
        assert_eq!(service.adapter, "ethernet-like");
        assert_eq!((service.reach, service.reach.arcs()), (Reach::Portal, 1));
        state.connectivity = Connectivity::Limited;
        assert_eq!(network_summary(Some(&state), &hw).reach.arcs(), 2);
        state.connectivity = Connectivity::Full;
        assert_eq!(network_summary(Some(&state), &hw).reach.arcs(), 3);
        state.connection = ConnectionState::Disconnected;
        assert_eq!(
            network_summary(Some(&state), &hw).reach,
            Reach::Disconnected
        );
        state.radio = RadioState::Disabled;
        assert_eq!(network_summary(Some(&state), &hw).reach, Reach::Off);
        assert_eq!(wifi_route(true), WifiRoute::Networkd);
        assert_eq!(wifi_route(false), WifiRoute::Core);
    }
}
