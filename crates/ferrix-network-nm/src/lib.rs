//! NetworkManager adapter for the networking service (ADR-002).
//!
//! Two layers:
//!
//! - [`map`] and [`diff`]: pure, host-tested tables that translate
//!   NetworkManager's numeric enums and flag words into the ADR types and
//!   compute the deltas between two snapshots. They compile everywhere and
//!   never touch a bus.
//! - [`backend`] (feature `production`): the D-Bus backend built on `zbus`'s
//!   blocking API. It hashes every hardware address at the adapter boundary;
//!   the raw BSSID never leaves this crate.
//!
//! Transport truth: the QEMU `virtio-net` device is reported as
//! `AdapterPresence::Present { kind: ethernet-like }` and never as Wi-Fi;
//! see `README.md`.

pub mod map {
    //! NetworkManager enum and flag tables (values from `NetworkManager.h`,
    //! stable across the 1.x series).

    use super::TransitionKeyMgmt;
    use ferrix_network_api::{
        AdapterKind, ConnectionState, Connectivity, NetworkError, SecurityType,
    };

    /// `NMDeviceType`.
    pub const NM_DEVICE_TYPE_ETHERNET: u32 = 1;
    pub const NM_DEVICE_TYPE_WIFI: u32 = 2;

    /// `NMDeviceState` (selected values).
    pub const NM_DEVICE_STATE_UNMANAGED: u32 = 10;
    pub const NM_DEVICE_STATE_UNAVAILABLE: u32 = 20;
    pub const NM_DEVICE_STATE_DISCONNECTED: u32 = 30;
    pub const NM_DEVICE_STATE_PREPARE: u32 = 40;
    pub const NM_DEVICE_STATE_CONFIG: u32 = 50;
    pub const NM_DEVICE_STATE_NEED_AUTH: u32 = 60;
    pub const NM_DEVICE_STATE_IP_CONFIG: u32 = 70;
    pub const NM_DEVICE_STATE_IP_CHECK: u32 = 80;
    pub const NM_DEVICE_STATE_SECONDARIES: u32 = 90;
    pub const NM_DEVICE_STATE_ACTIVATED: u32 = 100;
    pub const NM_DEVICE_STATE_DEACTIVATING: u32 = 110;
    pub const NM_DEVICE_STATE_FAILED: u32 = 120;

    /// `NMConnectivityState`.
    pub const NM_CONNECTIVITY_UNKNOWN: u32 = 0;
    pub const NM_CONNECTIVITY_NONE: u32 = 1;
    pub const NM_CONNECTIVITY_PORTAL: u32 = 2;
    pub const NM_CONNECTIVITY_LIMITED: u32 = 3;
    pub const NM_CONNECTIVITY_FULL: u32 = 4;

    /// `NM80211ApFlags`.
    pub const NM_802_11_AP_FLAGS_PRIVACY: u32 = 0x1;
    /// `NM80211ApSecurityFlags` key-management bits.
    pub const NM_802_11_AP_SEC_KEY_MGMT_PSK: u32 = 0x100;
    pub const NM_802_11_AP_SEC_KEY_MGMT_802_1X: u32 = 0x200;
    pub const NM_802_11_AP_SEC_KEY_MGMT_SAE: u32 = 0x400;
    pub const NM_802_11_AP_SEC_KEY_MGMT_OWE: u32 = 0x800;

    /// Adapter kind for a device type; `None` for devices this service
    /// does not model (modems, bridges, loopback, ...).
    pub fn adapter_kind(device_type: u32) -> Option<AdapterKind> {
        match device_type {
            NM_DEVICE_TYPE_WIFI => Some(AdapterKind::Wifi),
            NM_DEVICE_TYPE_ETHERNET => Some(AdapterKind::EthernetLike),
            _ => None,
        }
    }

    /// Security type from an access point's flag words.
    pub fn security(flags: u32, wpa_flags: u32, rsn_flags: u32) -> SecurityType {
        let enterprise = NM_802_11_AP_SEC_KEY_MGMT_802_1X;
        if rsn_flags & NM_802_11_AP_SEC_KEY_MGMT_SAE != 0 {
            SecurityType::Wpa3Sae
        } else if (rsn_flags | wpa_flags) & enterprise != 0 {
            SecurityType::Enterprise
        } else if rsn_flags & NM_802_11_AP_SEC_KEY_MGMT_PSK != 0 {
            SecurityType::Wpa2Psk
        } else if wpa_flags & NM_802_11_AP_SEC_KEY_MGMT_PSK != 0 {
            SecurityType::WpaPsk
        } else if rsn_flags & NM_802_11_AP_SEC_KEY_MGMT_OWE != 0 {
            // OWE needs no passphrase, but it is not an open `key-mgmt`:
            // NetworkManager requires `owe`, whereas the target-neutral
            // contract has no OWE variant.  Preserve a conservative
            // unsupported classification instead of making connect() send
            // `key-mgmt=none` and falsely treating the AP as open.
            SecurityType::Unknown
        } else if flags & NM_802_11_AP_FLAGS_PRIVACY != 0 && wpa_flags == 0 && rsn_flags == 0 {
            SecurityType::Wep
        } else if flags & NM_802_11_AP_FLAGS_PRIVACY == 0 {
            SecurityType::Open
        } else {
            SecurityType::Unknown
        }
    }

    /// `key-mgmt` for an access point, honouring the transition-mode policy:
    /// an SAE access point that also advertises PSK may be joined with
    /// `wpa-psk` when the target's driver cannot complete SAE.
    pub fn join_key_mgmt(
        security: SecurityType,
        rsn_flags: u32,
        transition: TransitionKeyMgmt,
    ) -> Option<&'static str> {
        let transition_ap = rsn_flags & NM_802_11_AP_SEC_KEY_MGMT_SAE != 0
            && rsn_flags & NM_802_11_AP_SEC_KEY_MGMT_PSK != 0;
        if security == SecurityType::Wpa3Sae
            && transition_ap
            && transition == TransitionKeyMgmt::Psk
        {
            return Some("wpa-psk");
        }
        key_mgmt(security)
    }

    /// NetworkManager's `key-mgmt` setting for joining a network of this
    /// type, or `None` when this adapter does not support joining it.
    pub fn key_mgmt(security: SecurityType) -> Option<&'static str> {
        match security {
            SecurityType::Open => Some("none"),
            SecurityType::WpaPsk | SecurityType::Wpa2Psk => Some("wpa-psk"),
            SecurityType::Wpa3Sae => Some("sae"),
            SecurityType::Wep | SecurityType::Enterprise | SecurityType::Unknown => None,
        }
    }

    /// `NMDeviceStateReason` for a failed device, mapped to the typed error.
    pub fn failure(reason: u32) -> NetworkError {
        match reason {
            // IP_CONFIG_UNAVAILABLE, IP_CONFIG_EXPIRED, DHCP_START_FAILED,
            // DHCP_ERROR, DHCP_FAILED
            5 | 6 | 15 | 16 | 17 => NetworkError::DhcpFailed,
            // NO_SECRETS, SUPPLICANT_DISCONNECT, SUPPLICANT_CONFIG_FAILED,
            // SUPPLICANT_FAILED
            7..=10 => NetworkError::AuthenticationFailed,
            // SUPPLICANT_TIMEOUT
            11 => NetworkError::TimedOut,
            // SSID_NOT_FOUND
            53 => NetworkError::NotFound {
                id: "ssid".to_owned(),
            },
            other => NetworkError::Backend {
                reason: format!("device state reason {other}"),
            },
        }
    }

    /// Whether a `NMDeviceStateReason` describes a failed activation (the
    /// reasons [`failure`] maps to a specific error), as opposed to a
    /// requested or idle disconnect.
    pub fn is_join_failure(reason: u32) -> bool {
        matches!(reason, 5 | 6 | 7..=11 | 15..=17 | 53)
    }

    /// Connection state of a device. `connected` names the access point (or
    /// link) id used for `connected { ap }`.
    pub fn connection_state(
        device_state: u32,
        reason: u32,
        connected: impl FnOnce() -> ferrix_network_api::ApId,
    ) -> ConnectionState {
        match device_state {
            NM_DEVICE_STATE_PREPARE | NM_DEVICE_STATE_CONFIG => ConnectionState::Associating,
            NM_DEVICE_STATE_NEED_AUTH => ConnectionState::Authenticating,
            NM_DEVICE_STATE_IP_CONFIG | NM_DEVICE_STATE_IP_CHECK | NM_DEVICE_STATE_SECONDARIES => {
                ConnectionState::Configuring
            }
            NM_DEVICE_STATE_ACTIVATED => ConnectionState::Connected { ap: connected() },
            NM_DEVICE_STATE_FAILED => ConnectionState::Failed {
                error: failure(reason),
            },
            // NetworkManager passes through FAILED to DISCONNECTED at once
            // and keeps the failure as the state reason; a user-requested
            // or idle disconnect carries a different reason.
            NM_DEVICE_STATE_DISCONNECTED if is_join_failure(reason) => ConnectionState::Failed {
                error: failure(reason),
            },
            _ => ConnectionState::Disconnected,
        }
    }

    pub fn connectivity(state: u32) -> Connectivity {
        match state {
            NM_CONNECTIVITY_PORTAL => Connectivity::Portal { url: None },
            NM_CONNECTIVITY_LIMITED => Connectivity::Limited,
            NM_CONNECTIVITY_FULL => Connectivity::Full,
            NM_CONNECTIVITY_NONE | NM_CONNECTIVITY_UNKNOWN => Connectivity::None,
            _ => Connectivity::None,
        }
    }

    /// Signal strength: NetworkManager reports 0..=100 already.
    pub fn signal_percent(strength: u8) -> u8 {
        strength.min(100)
    }

    /// Stable id from the numeric tail of a D-Bus object path
    /// (`/org/freedesktop/NetworkManager/AccessPoint/17` -> `ap-17`).
    pub fn id_from_path(prefix: &str, path: &str) -> String {
        let tail = path.rsplit('/').next().unwrap_or(path);
        format!("{prefix}-{tail}")
    }
}

pub mod diff {
    //! Deltas between two snapshots, in the order the service publishes.

    use ferrix_network_api::{NetworkEvent, NetworkState};

    pub fn diff(old: &NetworkState, new: &NetworkState) -> Vec<NetworkEvent> {
        let mut events = Vec::new();
        if old.adapter != new.adapter {
            events.push(NetworkEvent::AdapterChanged {
                adapter: new.adapter,
            });
        }
        if old.radio != new.radio {
            events.push(NetworkEvent::RadioChanged {
                radio: new.radio.clone(),
            });
        }
        if old.scan != new.scan {
            events.push(NetworkEvent::ScanChanged {
                scan: new.scan.clone(),
            });
        }
        if old.access_points != new.access_points {
            events.push(NetworkEvent::AccessPointsChanged {
                access_points: new.access_points.clone(),
            });
        }
        if old.connection != new.connection {
            events.push(NetworkEvent::ConnectionChanged {
                connection: new.connection.clone(),
            });
        }
        if old.ip != new.ip {
            events.push(NetworkEvent::IpChanged { ip: new.ip.clone() });
        }
        if old.connectivity != new.connectivity {
            events.push(NetworkEvent::ConnectivityChanged {
                connectivity: new.connectivity.clone(),
            });
        }
        if old.profiles != new.profiles {
            events.push(NetworkEvent::ProfilesChanged {
                profiles: new.profiles.clone(),
            });
        }
        events
    }
}

/// How a WPA2/WPA3 transition-mode access point is joined (the target
/// profile's `network.transition_key_mgmt`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TransitionKeyMgmt {
    #[default]
    Sae,
    Psk,
}

#[cfg(feature = "production")]
mod backend;
#[cfg(feature = "production")]
pub use backend::NetworkManagerBackend;

#[cfg(test)]
mod tests {
    use super::diff::diff;
    use super::map::*;
    use ferrix_network_api::{
        AdapterKind, AdapterPresence, ApId, ConnectionState, Connectivity, NetworkError,
        NetworkEvent, NetworkState, RadioState, SecurityType, hash_hardware_address,
    };

    #[test]
    fn device_types_map_and_virtio_net_is_never_wifi() {
        assert_eq!(adapter_kind(NM_DEVICE_TYPE_WIFI), Some(AdapterKind::Wifi));
        assert_eq!(
            adapter_kind(NM_DEVICE_TYPE_ETHERNET),
            Some(AdapterKind::EthernetLike)
        );
        for other in [0, 8, 14, 32] {
            assert_eq!(adapter_kind(other), None, "type {other}");
        }
    }

    #[test]
    fn security_flags_map_to_the_adr_types() {
        let p = NM_802_11_AP_FLAGS_PRIVACY;
        assert_eq!(security(0, 0, 0), SecurityType::Open);
        assert_eq!(security(p, 0, 0), SecurityType::Wep);
        assert_eq!(
            security(p, NM_802_11_AP_SEC_KEY_MGMT_PSK, 0),
            SecurityType::WpaPsk
        );
        assert_eq!(
            security(p, 0, NM_802_11_AP_SEC_KEY_MGMT_PSK),
            SecurityType::Wpa2Psk
        );
        assert_eq!(
            security(
                p,
                NM_802_11_AP_SEC_KEY_MGMT_PSK,
                NM_802_11_AP_SEC_KEY_MGMT_PSK
            ),
            SecurityType::Wpa2Psk
        );
        assert_eq!(
            security(p, 0, NM_802_11_AP_SEC_KEY_MGMT_SAE),
            SecurityType::Wpa3Sae
        );
        assert_eq!(
            security(
                p,
                0,
                NM_802_11_AP_SEC_KEY_MGMT_PSK | NM_802_11_AP_SEC_KEY_MGMT_SAE
            ),
            SecurityType::Wpa3Sae,
            "mixed WPA2/WPA3 networks join with SAE"
        );
        assert_eq!(
            security(p, 0, NM_802_11_AP_SEC_KEY_MGMT_802_1X),
            SecurityType::Enterprise
        );
        assert_eq!(
            security(p, 0, NM_802_11_AP_SEC_KEY_MGMT_OWE),
            SecurityType::Unknown
        );
        assert_eq!(security(p, 0, 0x1), SecurityType::Unknown);
        assert_eq!(key_mgmt(SecurityType::Wpa2Psk), Some("wpa-psk"));
        assert_eq!(key_mgmt(SecurityType::Wpa3Sae), Some("sae"));
        assert_eq!(key_mgmt(SecurityType::Open), Some("none"));
        assert_eq!(key_mgmt(SecurityType::Enterprise), None);
        assert_eq!(key_mgmt(SecurityType::Wep), None);
    }

    #[test]
    fn transition_policy_only_changes_mixed_sae_psk_access_points() {
        use super::TransitionKeyMgmt::{Psk, Sae};
        let mixed = NM_802_11_AP_SEC_KEY_MGMT_SAE | NM_802_11_AP_SEC_KEY_MGMT_PSK;
        let sae_only = NM_802_11_AP_SEC_KEY_MGMT_SAE;
        assert_eq!(
            join_key_mgmt(SecurityType::Wpa3Sae, mixed, Psk),
            Some("wpa-psk")
        );
        assert_eq!(
            join_key_mgmt(SecurityType::Wpa3Sae, mixed, Sae),
            Some("sae")
        );
        assert_eq!(
            join_key_mgmt(SecurityType::Wpa3Sae, sae_only, Psk),
            Some("sae")
        );
        assert_eq!(
            join_key_mgmt(SecurityType::Wpa2Psk, NM_802_11_AP_SEC_KEY_MGMT_PSK, Psk),
            Some("wpa-psk")
        );
    }

    #[test]
    fn device_states_and_reasons_map() {
        let ap = || ApId::new("ap-1");
        assert_eq!(
            connection_state(NM_DEVICE_STATE_PREPARE, 0, ap),
            ConnectionState::Associating
        );
        assert_eq!(
            connection_state(NM_DEVICE_STATE_CONFIG, 0, ap),
            ConnectionState::Associating
        );
        assert_eq!(
            connection_state(NM_DEVICE_STATE_NEED_AUTH, 0, ap),
            ConnectionState::Authenticating
        );
        for state in [
            NM_DEVICE_STATE_IP_CONFIG,
            NM_DEVICE_STATE_IP_CHECK,
            NM_DEVICE_STATE_SECONDARIES,
        ] {
            assert_eq!(connection_state(state, 0, ap), ConnectionState::Configuring);
        }
        assert_eq!(
            connection_state(NM_DEVICE_STATE_ACTIVATED, 0, ap),
            ConnectionState::Connected {
                ap: ApId::new("ap-1")
            }
        );
        for state in [
            0,
            NM_DEVICE_STATE_UNMANAGED,
            NM_DEVICE_STATE_UNAVAILABLE,
            NM_DEVICE_STATE_DISCONNECTED,
            NM_DEVICE_STATE_DEACTIVATING,
        ] {
            assert_eq!(
                connection_state(state, 0, ap),
                ConnectionState::Disconnected
            );
        }
        assert_eq!(
            connection_state(NM_DEVICE_STATE_FAILED, 7, ap),
            ConnectionState::Failed {
                error: NetworkError::AuthenticationFailed
            }
        );
        // FAILED passes straight to DISCONNECTED; the kept reason (no
        // secrets, supplicant failure) still surfaces as the failure, while
        // a user-requested disconnect (39) stays a plain disconnect.
        assert_eq!(
            connection_state(NM_DEVICE_STATE_DISCONNECTED, 7, ap),
            ConnectionState::Failed {
                error: NetworkError::AuthenticationFailed
            }
        );
        assert_eq!(
            connection_state(NM_DEVICE_STATE_DISCONNECTED, 39, ap),
            ConnectionState::Disconnected
        );
        assert!(!is_join_failure(0) && is_join_failure(8) && is_join_failure(53));
        assert_eq!(failure(17), NetworkError::DhcpFailed);
        assert_eq!(failure(11), NetworkError::TimedOut);
        assert_eq!(failure(53), NetworkError::NotFound { id: "ssid".into() });
        assert!(matches!(failure(99), NetworkError::Backend { .. }));
    }

    #[test]
    fn connectivity_and_strength_map() {
        assert_eq!(connectivity(NM_CONNECTIVITY_UNKNOWN), Connectivity::None);
        assert_eq!(connectivity(NM_CONNECTIVITY_NONE), Connectivity::None);
        assert_eq!(
            connectivity(NM_CONNECTIVITY_PORTAL),
            Connectivity::Portal { url: None }
        );
        assert_eq!(connectivity(NM_CONNECTIVITY_LIMITED), Connectivity::Limited);
        assert_eq!(connectivity(NM_CONNECTIVITY_FULL), Connectivity::Full);
        assert_eq!(connectivity(77), Connectivity::None);
        assert_eq!(signal_percent(78), 78);
        assert_eq!(signal_percent(255), 100);
        assert_eq!(
            id_from_path("ap", "/org/freedesktop/NetworkManager/AccessPoint/17"),
            "ap-17"
        );
        assert_eq!(
            id_from_path("profile", "/org/freedesktop/NetworkManager/Settings/3"),
            "profile-3"
        );
    }

    #[test]
    fn bssid_hashing_is_case_insensitive_and_never_the_raw_value() {
        let hash = hash_hardware_address("AA:BB:CC:DD:EE:FF");
        assert_eq!(hash, hash_hardware_address("aa:bb:cc:dd:ee:ff"));
        assert!(!hash.contains(':'));
        assert_eq!(hash.len(), 16);
    }

    #[test]
    fn diff_emits_only_changed_sections_in_publish_order() {
        let old = NetworkState::default();
        assert!(diff(&old, &old).is_empty());
        let mut new = old.clone();
        new.adapter = AdapterPresence::Present {
            kind: AdapterKind::EthernetLike,
        };
        new.radio = RadioState::Unavailable {
            reason: "no wireless adapter".into(),
        };
        new.connectivity = Connectivity::Full;
        let events = diff(&old, &new);
        assert_eq!(
            events.iter().map(NetworkEvent::name).collect::<Vec<_>>(),
            ["adapter-changed", "radio-changed", "connectivity-changed"]
        );
        let mut folded = old.clone();
        for event in &events {
            event.apply_to(&mut folded);
        }
        assert_eq!(folded, new);
    }
}
