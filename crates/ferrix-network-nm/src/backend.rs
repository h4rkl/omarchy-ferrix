//! NetworkManager 1.x over D-Bus (`zbus`, blocking API).
//!
//! Design: every public state is derived from a fresh enumeration of the
//! bus (`snapshot`), and a watcher thread re-enumerates whenever
//! NetworkManager emits a `PropertiesChanged` signal, publishing the diff as
//! typed deltas. A `NameOwnerChanged` for `org.freedesktop.NetworkManager`
//! is a restart: object paths are re-resolved and the new snapshot is
//! published as `service-restarted`. Deriving from snapshots keeps the
//! adapter correct under lost or reordered signals at the cost of a few
//! extra property reads.
//!
//! The raw BSSID is read once per access point and hashed immediately; no
//! type outside this module ever holds it.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

use ferrix_network_adapter_api::{BackendError, BackendEvent, NetworkBackend};
use ferrix_network_api::{
    AccessPoint, AdapterKind, AdapterPresence, ApId, IpConfig, NetworkError, NetworkState,
    ProfileId, RadioState, SavedProfile, ScanState, SecurityType, Ssid, hash_hardware_address,
};
use zbus::blocking::{Connection, MessageIterator};
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};
use zbus::{MatchRule, message::Type as MessageType};

use crate::diff::diff;
use crate::{TransitionKeyMgmt, map};

const NM_SERVICE: &str = "org.freedesktop.NetworkManager";
const ROOT_PATH: &str = "/";

// ---------------------------------------------------------------------------
// Proxies
// ---------------------------------------------------------------------------

#[zbus::proxy(
    interface = "org.freedesktop.NetworkManager",
    default_service = "org.freedesktop.NetworkManager",
    default_path = "/org/freedesktop/NetworkManager"
)]
trait Manager {
    fn get_devices(&self) -> zbus::Result<Vec<OwnedObjectPath>>;
    fn check_connectivity(&self) -> zbus::Result<u32>;
    fn activate_connection(
        &self,
        connection: &ObjectPath<'_>,
        device: &ObjectPath<'_>,
        specific_object: &ObjectPath<'_>,
    ) -> zbus::Result<OwnedObjectPath>;
    fn add_and_activate_connection(
        &self,
        connection: HashMap<&str, HashMap<&str, Value<'_>>>,
        device: &ObjectPath<'_>,
        specific_object: &ObjectPath<'_>,
    ) -> zbus::Result<(OwnedObjectPath, OwnedObjectPath)>;
    fn deactivate_connection(&self, active_connection: &ObjectPath<'_>) -> zbus::Result<()>;
    #[zbus(property)]
    fn wireless_enabled(&self) -> zbus::Result<bool>;
    #[zbus(property)]
    fn set_wireless_enabled(&self, value: bool) -> zbus::Result<()>;
    #[zbus(property)]
    fn wireless_hardware_enabled(&self) -> zbus::Result<bool>;
    #[zbus(property)]
    fn connectivity(&self) -> zbus::Result<u32>;
}

#[zbus::proxy(
    interface = "org.freedesktop.NetworkManager.Device",
    default_service = "org.freedesktop.NetworkManager"
)]
trait Device {
    fn disconnect(&self) -> zbus::Result<()>;
    #[zbus(property)]
    fn device_type(&self) -> zbus::Result<u32>;
    #[zbus(property)]
    fn state(&self) -> zbus::Result<u32>;
    #[zbus(property)]
    fn state_reason(&self) -> zbus::Result<(u32, u32)>;
    #[zbus(property)]
    fn ip4_config(&self) -> zbus::Result<OwnedObjectPath>;
    #[zbus(property)]
    fn active_connection(&self) -> zbus::Result<OwnedObjectPath>;
    #[zbus(property)]
    fn managed(&self) -> zbus::Result<bool>;
}

#[zbus::proxy(
    interface = "org.freedesktop.NetworkManager.Device.Wireless",
    default_service = "org.freedesktop.NetworkManager"
)]
trait Wireless {
    fn get_all_access_points(&self) -> zbus::Result<Vec<OwnedObjectPath>>;
    fn request_scan(&self, options: HashMap<&str, Value<'_>>) -> zbus::Result<()>;
    #[zbus(property)]
    fn active_access_point(&self) -> zbus::Result<OwnedObjectPath>;
    #[zbus(property)]
    fn last_scan(&self) -> zbus::Result<i64>;
}

#[zbus::proxy(
    interface = "org.freedesktop.NetworkManager.AccessPoint",
    default_service = "org.freedesktop.NetworkManager"
)]
trait AccessPointIface {
    #[zbus(property)]
    fn flags(&self) -> zbus::Result<u32>;
    #[zbus(property)]
    fn wpa_flags(&self) -> zbus::Result<u32>;
    #[zbus(property)]
    fn rsn_flags(&self) -> zbus::Result<u32>;
    #[zbus(property)]
    fn ssid(&self) -> zbus::Result<Vec<u8>>;
    #[zbus(property)]
    fn frequency(&self) -> zbus::Result<u32>;
    #[zbus(property)]
    fn hw_address(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn strength(&self) -> zbus::Result<u8>;
}

#[zbus::proxy(
    interface = "org.freedesktop.NetworkManager.Settings",
    default_service = "org.freedesktop.NetworkManager",
    default_path = "/org/freedesktop/NetworkManager/Settings"
)]
trait Settings {
    fn list_connections(&self) -> zbus::Result<Vec<OwnedObjectPath>>;
}

#[zbus::proxy(
    interface = "org.freedesktop.NetworkManager.Settings.Connection",
    default_service = "org.freedesktop.NetworkManager"
)]
trait SettingsConnection {
    fn get_settings(&self) -> zbus::Result<HashMap<String, HashMap<String, OwnedValue>>>;
    fn delete(&self) -> zbus::Result<()>;
}

#[zbus::proxy(
    interface = "org.freedesktop.NetworkManager.Connection.Active",
    default_service = "org.freedesktop.NetworkManager"
)]
trait ActiveConnection {
    #[zbus(property)]
    fn state(&self) -> zbus::Result<u32>;
    #[zbus(property)]
    fn connection(&self) -> zbus::Result<OwnedObjectPath>;
    #[zbus(property)]
    fn ip4_config(&self) -> zbus::Result<OwnedObjectPath>;
}

#[zbus::proxy(
    interface = "org.freedesktop.NetworkManager.IP4Config",
    default_service = "org.freedesktop.NetworkManager"
)]
trait Ip4Config {
    #[zbus(property)]
    fn address_data(&self) -> zbus::Result<Vec<HashMap<String, OwnedValue>>>;
    #[zbus(property)]
    fn gateway(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn nameserver_data(&self) -> zbus::Result<Vec<HashMap<String, OwnedValue>>>;
}

// ---------------------------------------------------------------------------
// Backend
// ---------------------------------------------------------------------------

fn bus_error(error: zbus::Error) -> BackendError {
    BackendError::unavailable(error.to_string())
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Typed read of one entry of a D-Bus `a{sv}` dictionary.
fn field<T>(dict: &HashMap<String, OwnedValue>, key: &str) -> Option<T>
where
    T: TryFrom<OwnedValue>,
{
    dict.get(key).and_then(|v| T::try_from(v.clone()).ok())
}

/// The device this service manages: the first Wi-Fi device, else the first
/// Ethernet-like device (QEMU `virtio-net`), else none.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ManagedDevice {
    path: OwnedObjectPath,
    kind: AdapterKind,
}

/// Bookkeeping the watcher needs between enumerations.
struct Watch {
    last: NetworkState,
    /// `LastScan` value when the current scan was requested.
    scan_started_at: Option<i64>,
    /// The scan is complete once `LastScan` moves past this.
    scanning: bool,
}

pub struct NetworkManagerBackend {
    connection: Connection,
    watch: Mutex<Watch>,
    sinks: Mutex<Vec<Sender<BackendEvent>>>,
    /// Access-point ids we handed out, so `connect` can resolve the path.
    aps: Mutex<HashMap<ApId, OwnedObjectPath>>,
    transition: TransitionKeyMgmt,
}

impl NetworkManagerBackend {
    /// Connects to the system bus and starts the change watcher.
    pub fn connect() -> Result<Arc<Self>, BackendError> {
        Self::connect_with(TransitionKeyMgmt::default())
    }

    /// [`connect`](Self::connect) with the target's transition-mode policy.
    pub fn connect_with(transition: TransitionKeyMgmt) -> Result<Arc<Self>, BackendError> {
        let connection = Connection::system().map_err(bus_error)?;
        let backend = Arc::new(Self {
            connection,
            watch: Mutex::new(Watch {
                last: NetworkState::default(),
                scan_started_at: None,
                scanning: false,
            }),
            sinks: Mutex::new(Vec::new()),
            aps: Mutex::new(HashMap::new()),
            transition,
        });
        let first = backend.enumerate()?;
        lock(&backend.watch).last = first;
        backend.spawn_watchers()?;
        Ok(backend)
    }

    fn manager(&self) -> Result<ManagerProxyBlocking<'_>, BackendError> {
        ManagerProxyBlocking::new(&self.connection).map_err(bus_error)
    }

    fn device(&self, path: &OwnedObjectPath) -> Result<DeviceProxyBlocking<'_>, BackendError> {
        DeviceProxyBlocking::builder(&self.connection)
            .path(path.clone())
            .map_err(bus_error)?
            .build()
            .map_err(bus_error)
    }

    fn wireless(&self, path: &OwnedObjectPath) -> Result<WirelessProxyBlocking<'_>, BackendError> {
        WirelessProxyBlocking::builder(&self.connection)
            .path(path.clone())
            .map_err(bus_error)?
            .build()
            .map_err(bus_error)
    }

    fn managed_device(&self) -> Result<Option<ManagedDevice>, BackendError> {
        let mut ethernet = None;
        for path in self.manager()?.get_devices().map_err(bus_error)? {
            let device = self.device(&path)?;
            if !device.managed().unwrap_or(true) {
                continue;
            }
            match device.device_type().ok().and_then(map::adapter_kind) {
                Some(AdapterKind::Wifi) => {
                    return Ok(Some(ManagedDevice {
                        path,
                        kind: AdapterKind::Wifi,
                    }));
                }
                Some(AdapterKind::EthernetLike) if ethernet.is_none() => {
                    ethernet = Some(ManagedDevice {
                        path,
                        kind: AdapterKind::EthernetLike,
                    });
                }
                _ => {}
            }
        }
        Ok(ethernet)
    }

    /// Reads one access point; the hardware address is hashed here and the
    /// raw string dropped before this function returns.
    fn access_point(&self, path: &OwnedObjectPath) -> Result<AccessPoint, BackendError> {
        let proxy = AccessPointIfaceProxyBlocking::builder(&self.connection)
            .path(path.clone())
            .map_err(bus_error)?
            .build()
            .map_err(bus_error)?;
        let bssid_hash = hash_hardware_address(&proxy.hw_address().map_err(bus_error)?);
        Ok(AccessPoint {
            id: ApId::new(map::id_from_path("ap", path.as_str())),
            ssid: Ssid::new(proxy.ssid().map_err(bus_error)?),
            bssid_hash,
            signal_percent: map::signal_percent(proxy.strength().map_err(bus_error)?),
            security: map::security(
                proxy.flags().map_err(bus_error)?,
                proxy.wpa_flags().map_err(bus_error)?,
                proxy.rsn_flags().map_err(bus_error)?,
            ),
            frequency_mhz: proxy.frequency().ok(),
        })
    }

    fn ip_config(&self, path: &OwnedObjectPath) -> Option<IpConfig> {
        if path.as_str() == ROOT_PATH {
            return None;
        }
        let proxy = Ip4ConfigProxyBlocking::builder(&self.connection)
            .path(path.clone())
            .ok()?
            .build()
            .ok()?;
        let addresses = proxy
            .address_data()
            .ok()?
            .into_iter()
            .filter_map(|entry| {
                let address: String = field(&entry, "address")?;
                let prefix: u32 = field(&entry, "prefix")?;
                Some(format!("{address}/{prefix}"))
            })
            .collect();
        let gateway = proxy.gateway().ok().filter(|g| !g.is_empty());
        let dns = proxy
            .nameserver_data()
            .ok()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|entry| field::<String>(&entry, "address"))
            .collect();
        Some(IpConfig {
            addresses,
            gateway,
            dns,
            // NetworkManager does not expose the method per address here;
            // the guest route uses DHCP and static setups are not modelled.
            dhcp: true,
        })
    }

    fn profiles(&self) -> Result<Vec<SavedProfile>, BackendError> {
        let settings = SettingsProxyBlocking::new(&self.connection).map_err(bus_error)?;
        let mut profiles = Vec::new();
        for path in settings.list_connections().map_err(bus_error)? {
            let proxy = SettingsConnectionProxyBlocking::builder(&self.connection)
                .path(path.clone())
                .map_err(bus_error)?
                .build()
                .map_err(bus_error)?;
            let Ok(all) = proxy.get_settings() else {
                continue;
            };
            let Some(wireless) = all.get("802-11-wireless") else {
                continue; // wired and other profiles are not Wi-Fi profiles
            };
            let ssid: Vec<u8> = field(wireless, "ssid").unwrap_or_default();
            let key_mgmt: Option<String> = all
                .get("802-11-wireless-security")
                .and_then(|security| field(security, "key-mgmt"));
            let security = match key_mgmt.as_deref() {
                None | Some("none") => SecurityType::Open,
                Some("wpa-psk") => SecurityType::Wpa2Psk,
                Some("sae") => SecurityType::Wpa3Sae,
                Some("wpa-eap") | Some("wpa-eap-suite-b-192") => SecurityType::Enterprise,
                Some("ieee8021x") => SecurityType::Wep,
                Some(_) => SecurityType::Unknown,
            };
            let autoconnect: bool = all
                .get("connection")
                .and_then(|connection| field(connection, "autoconnect"))
                .unwrap_or(true);
            profiles.push(SavedProfile {
                id: ProfileId::new(map::id_from_path("profile", path.as_str())),
                ssid: Ssid::new(ssid),
                security,
                autoconnect,
            });
        }
        Ok(profiles)
    }

    /// Full enumeration of the bus into a snapshot.
    fn enumerate(&self) -> Result<NetworkState, BackendError> {
        let manager = self.manager()?;
        let mut state = NetworkState {
            connectivity: map::connectivity(manager.connectivity().map_err(bus_error)?),
            profiles: self.profiles()?,
            ..NetworkState::default()
        };
        let Some(managed) = self.managed_device()? else {
            return Ok(state);
        };
        state.adapter = AdapterPresence::Present { kind: managed.kind };
        let device = self.device(&managed.path)?;
        let (device_state, reason) = device.state_reason().unwrap_or((0, 0));
        state.ip = device
            .ip4_config()
            .ok()
            .and_then(|path| self.ip_config(&path));
        match managed.kind {
            AdapterKind::EthernetLike => {
                state.radio = RadioState::Unavailable {
                    reason: "no wireless adapter; ethernet-like transport".to_owned(),
                };
                let link = ApId::new(map::id_from_path("link", managed.path.as_str()));
                state.connection = map::connection_state(device_state, reason, || link);
            }
            AdapterKind::Wifi => {
                state.radio = if manager.wireless_enabled().map_err(bus_error)? {
                    RadioState::Enabled
                } else if manager.wireless_hardware_enabled().unwrap_or(true) {
                    RadioState::Disabled
                } else {
                    RadioState::Unavailable {
                        reason: "hardware switch".to_owned(),
                    }
                };
                let wireless = self.wireless(&managed.path)?;
                let mut ids = HashMap::new();
                for path in wireless.get_all_access_points().map_err(bus_error)? {
                    if let Ok(ap) = self.access_point(&path) {
                        ids.insert(ap.id.clone(), path);
                        state.access_points.push(ap);
                    }
                }
                state
                    .access_points
                    .sort_by_key(|ap| std::cmp::Reverse(ap.signal_percent));
                let active = wireless.active_access_point().ok();
                let active_id = active
                    .filter(|p| p.as_str() != ROOT_PATH)
                    .map(|p| ApId::new(map::id_from_path("ap", p.as_str())));
                state.connection = map::connection_state(device_state, reason, || {
                    active_id.unwrap_or_else(|| ApId::new("ap-unknown"))
                });
                *lock(&self.aps) = ids;
                let mut watch = lock(&self.watch);
                let last_scan = wireless.last_scan().unwrap_or(0);
                if watch.scanning {
                    if watch
                        .scan_started_at
                        .is_some_and(|started| last_scan > started)
                    {
                        watch.scanning = false;
                        watch.scan_started_at = None;
                        state.scan = ScanState::Completed {
                            count: state.access_points.len(),
                        };
                    } else {
                        state.scan = ScanState::Scanning;
                    }
                } else {
                    state.scan = match &watch.last.scan {
                        ScanState::Completed { .. } => ScanState::Completed {
                            count: state.access_points.len(),
                        },
                        other => other.clone(),
                    };
                }
            }
        }
        if !state.connection.is_connected() {
            state.ip = None;
        }
        Ok(state)
    }

    fn broadcast(&self, event: BackendEvent) {
        lock(&self.sinks).retain(|sink| sink.send(event.clone()).is_ok());
    }

    /// Re-enumerates and publishes the diff (or a restart snapshot).
    fn refresh(&self, restarted: bool) {
        match self.enumerate() {
            Ok(fresh) => {
                let events = {
                    let mut watch = lock(&self.watch);
                    let events = diff(&watch.last, &fresh);
                    watch.last = fresh.clone();
                    events
                };
                if restarted {
                    self.broadcast(BackendEvent::Restarted { state: fresh });
                } else {
                    for event in events {
                        self.broadcast(BackendEvent::Changed(event));
                    }
                }
            }
            Err(error) => self.broadcast(BackendEvent::Lost {
                reason: error.to_string(),
            }),
        }
    }

    /// Watches NetworkManager's signals and owner changes.
    ///
    /// The draining threads never call D-Bus: zbus's socket reader waits for
    /// room in a match rule's queue, so a consumer that blocked on a method
    /// call over the same connection while signals piled up would starve the
    /// reader of the very reply it is waiting for (a deadlock seen on
    /// hardware during a Wi-Fi join). They only mark the state dirty; one
    /// refresher thread coalesces the marks and does the D-Bus work.
    fn spawn_watchers(self: &Arc<Self>) -> Result<(), BackendError> {
        let (wake, woken) = mpsc::sync_channel::<()>(1);
        let full_refresh = Arc::new(AtomicBool::new(false));

        let changes = MatchRule::builder()
            .msg_type(MessageType::Signal)
            .sender(NM_SERVICE)
            .map_err(bus_error)?
            .build();
        let changes = MessageIterator::for_match_rule(changes, &self.connection, Some(64))
            .map_err(bus_error)?;
        let signal_wake = wake.clone();
        thread::Builder::new()
            .name("ferrix-network-nm-signals".to_owned())
            .spawn(move || {
                for message in changes {
                    if message.is_ok() {
                        // A pending wake already covers this change.
                        let _ = signal_wake.try_send(());
                    }
                }
            })
            .map_err(|e| BackendError::unavailable(e.to_string()))?;

        let owner = MatchRule::builder()
            .msg_type(MessageType::Signal)
            .sender("org.freedesktop.DBus")
            .map_err(bus_error)?
            .interface("org.freedesktop.DBus")
            .map_err(bus_error)?
            .member("NameOwnerChanged")
            .map_err(bus_error)?
            .arg(0, NM_SERVICE)
            .map_err(bus_error)?
            .build();
        let owner =
            MessageIterator::for_match_rule(owner, &self.connection, Some(8)).map_err(bus_error)?;
        let backend = Arc::clone(self);
        let owner_full = Arc::clone(&full_refresh);
        thread::Builder::new()
            .name("ferrix-network-nm-owner".to_owned())
            .spawn(move || {
                for message in owner {
                    let Ok(message) = message else {
                        continue;
                    };
                    let Ok((_, _old, new_owner)) =
                        message.body().deserialize::<(String, String, String)>()
                    else {
                        continue;
                    };
                    if new_owner.is_empty() {
                        backend.broadcast(BackendEvent::Lost {
                            reason: "NetworkManager left the bus".to_owned(),
                        });
                    } else {
                        owner_full.store(true, Ordering::SeqCst);
                        let _ = wake.try_send(());
                    }
                }
            })
            .map_err(|e| BackendError::unavailable(e.to_string()))?;

        let backend = Arc::clone(self);
        thread::Builder::new()
            .name("ferrix-network-nm-refresh".to_owned())
            .spawn(move || {
                while woken.recv().is_ok() {
                    // Coalesce bursts: NetworkManager emits many property
                    // changes per transition.
                    thread::sleep(Duration::from_millis(50));
                    let full = full_refresh.swap(false, Ordering::SeqCst);
                    if full {
                        // Give a new instance a moment to export its objects.
                        thread::sleep(Duration::from_millis(500));
                    }
                    backend.refresh(full);
                }
            })
            .map_err(|e| BackendError::unavailable(e.to_string()))?;
        Ok(())
    }

    fn wifi_device(&self) -> Result<OwnedObjectPath, BackendError> {
        match self.managed_device()? {
            Some(ManagedDevice {
                path,
                kind: AdapterKind::Wifi,
            }) => Ok(path),
            Some(ManagedDevice {
                kind: AdapterKind::EthernetLike,
                ..
            }) => Err(BackendError::unsupported("wifi (ethernet-like transport)")),
            None => Err(NetworkError::NoAdapter.into()),
        }
    }

    fn require_radio(&self) -> Result<(), BackendError> {
        if self.manager()?.wireless_enabled().map_err(bus_error)? {
            Ok(())
        } else {
            Err(NetworkError::RadioDisabled.into())
        }
    }
}

impl NetworkBackend for NetworkManagerBackend {
    fn name(&self) -> &'static str {
        "networkmanager"
    }

    fn snapshot(&self) -> Result<NetworkState, BackendError> {
        let fresh = self.enumerate()?;
        lock(&self.watch).last = fresh.clone();
        Ok(fresh)
    }

    fn set_radio(&self, enabled: bool) -> Result<(), BackendError> {
        self.wifi_device()?;
        self.manager()?
            .set_wireless_enabled(enabled)
            .map_err(bus_error)
    }

    fn scan(&self) -> Result<(), BackendError> {
        let device = self.wifi_device()?;
        self.require_radio()?;
        let wireless = self.wireless(&device)?;
        let started = wireless.last_scan().unwrap_or(0);
        wireless.request_scan(HashMap::new()).map_err(bus_error)?;
        {
            let mut watch = lock(&self.watch);
            watch.scanning = true;
            watch.scan_started_at = Some(started);
        }
        self.refresh(false);
        Ok(())
    }

    fn connect(&self, ap: &ApId, passphrase: Option<&str>) -> Result<(), BackendError> {
        let device = self.wifi_device()?;
        self.require_radio()?;
        let path = lock(&self.aps)
            .get(ap)
            .cloned()
            .ok_or_else(|| NetworkError::NotFound {
                id: ap.as_str().to_owned(),
            })?;
        let point = self.access_point(&path)?;
        let rsn_flags = AccessPointIfaceProxyBlocking::builder(&self.connection)
            .path(path.clone())
            .map_err(bus_error)?
            .build()
            .map_err(bus_error)?
            .rsn_flags()
            .map_err(bus_error)?;
        let key_mgmt =
            map::join_key_mgmt(point.security, rsn_flags, self.transition).ok_or_else(|| {
                BackendError::unsupported(format!("joining {} networks", point.security))
            })?;
        let mut settings: HashMap<&str, HashMap<&str, Value<'_>>> = HashMap::new();
        let mut connection = HashMap::new();
        connection.insert("type", Value::from("802-11-wireless"));
        connection.insert("id", Value::from(point.ssid.to_string()));
        settings.insert("connection", connection);
        let mut wireless = HashMap::new();
        wireless.insert("ssid", Value::from(point.ssid.as_bytes().to_vec()));
        settings.insert("802-11-wireless", wireless);
        if point.security.needs_passphrase() {
            let secret = passphrase.ok_or(NetworkError::AuthenticationFailed)?;
            let mut security = HashMap::new();
            security.insert("key-mgmt", Value::from(key_mgmt));
            security.insert("psk", Value::from(secret));
            settings.insert("802-11-wireless-security", security);
        }
        self.manager()?
            .add_and_activate_connection(settings, &device, &path)
            .map_err(bus_error)?;
        Ok(())
    }

    /// Adds a profile with `802-11-wireless.hidden = true` (NetworkManager
    /// then probes for the name instead of waiting for a beacon) and
    /// activates it on the Wi-Fi device without a specific access point.
    /// Progress and failure arrive like any join: the refresh thread maps
    /// the device state and its reason (wrong secret, SSID not found)
    /// through [`map::connection_state`]. The passphrase goes only into the
    /// settings dictionary; no error path formats it.
    fn join_hidden(
        &self,
        ssid: &Ssid,
        security: SecurityType,
        passphrase: Option<&str>,
    ) -> Result<(), BackendError> {
        let device = self.wifi_device()?;
        self.require_radio()?;
        let key_mgmt = map::hidden_key_mgmt(security).ok_or_else(|| {
            BackendError::unsupported(format!("joining {security} networks by name"))
        })?;
        let mut settings: HashMap<&str, HashMap<&str, Value<'_>>> = HashMap::new();
        let mut connection = HashMap::new();
        connection.insert("type", Value::from("802-11-wireless"));
        connection.insert("id", Value::from(ssid.to_string()));
        settings.insert("connection", connection);
        let mut wireless = HashMap::new();
        wireless.insert("ssid", Value::from(ssid.as_bytes().to_vec()));
        wireless.insert("hidden", Value::from(true));
        settings.insert("802-11-wireless", wireless);
        if security.needs_passphrase() {
            let secret = passphrase.ok_or(NetworkError::AuthenticationFailed)?;
            let mut wireless_security = HashMap::new();
            wireless_security.insert("key-mgmt", Value::from(key_mgmt));
            wireless_security.insert("psk", Value::from(secret));
            settings.insert("802-11-wireless-security", wireless_security);
        }
        let root = ObjectPath::try_from(ROOT_PATH)
            .map_err(|e| BackendError::unavailable(e.to_string()))?;
        self.manager()?
            .add_and_activate_connection(settings, &device, &root)
            .map_err(bus_error)?;
        Ok(())
    }

    fn connect_profile(&self, profile: &ProfileId) -> Result<(), BackendError> {
        let device = self.wifi_device()?;
        self.require_radio()?;
        let settings = SettingsProxyBlocking::new(&self.connection).map_err(bus_error)?;
        let path = settings
            .list_connections()
            .map_err(bus_error)?
            .into_iter()
            .find(|p| map::id_from_path("profile", p.as_str()) == profile.as_str())
            .ok_or_else(|| NetworkError::NotFound {
                id: profile.as_str().to_owned(),
            })?;
        let root = ObjectPath::try_from(ROOT_PATH)
            .map_err(|e| BackendError::unavailable(e.to_string()))?;
        self.manager()?
            .activate_connection(&path, &device, &root)
            .map_err(bus_error)?;
        Ok(())
    }

    fn disconnect(&self) -> Result<(), BackendError> {
        let managed = self.managed_device()?.ok_or(NetworkError::NoAdapter)?;
        self.device(&managed.path)?.disconnect().map_err(bus_error)
    }

    fn forget(&self, profile: &ProfileId) -> Result<(), BackendError> {
        let settings = SettingsProxyBlocking::new(&self.connection).map_err(bus_error)?;
        let path = settings
            .list_connections()
            .map_err(bus_error)?
            .into_iter()
            .find(|p| map::id_from_path("profile", p.as_str()) == profile.as_str())
            .ok_or_else(|| NetworkError::NotFound {
                id: profile.as_str().to_owned(),
            })?;
        SettingsConnectionProxyBlocking::builder(&self.connection)
            .path(path)
            .map_err(bus_error)?
            .build()
            .map_err(bus_error)?
            .delete()
            .map_err(bus_error)
    }

    fn subscribe(&self, sink: Sender<BackendEvent>) {
        lock(&self.sinks).push(sink);
    }
}
