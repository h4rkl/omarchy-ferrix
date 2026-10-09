//! BlueZ system-D-Bus transport (`zbus`, blocking API).
//!
//! Threading rules (the deadlock this repository already hit once in the
//! NetworkManager backend): zbus's socket reader stops when a
//! `MessageIterator` queue is full, so
//!
//! * each iterator has a dedicated consumer thread that only forwards an
//!   event over an unbounded std channel and never makes a D-Bus call;
//! * every blocking call (`Pair`, `Connect`, `GetManagedObjects`, ...) runs on
//!   a worker or the adapter's state thread, never on an iterator consumer;
//! * the exported `org.bluez.Agent1` methods run on zbus's executor: they
//!   only touch the adapter's in-memory state and `await` the user's answer
//!   on an async channel, so a pending prompt never blocks the executor or
//!   the `Pair` reply it is part of.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

use ferrix_hardware_api::{Capability, HardwareError, PairingFailure, PairingPhase};
use zbus::blocking::{Connection, MessageIterator};
use zbus::message::Type as MessageType;
use zbus::names::OwnedInterfaceName;
use zbus::proxy::CacheProperties;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};
use zbus::{DBusError, MatchRule};

use super::{
    AdapterRecord, AgentCallbacks, BluezBus, BusError, BusEvent, DeviceRecord, PromptAnswer, redact,
};

const BLUEZ_SERVICE: &str = "org.bluez";
const ADAPTER_INTERFACE: &str = "org.bluez.Adapter1";
const DEVICE_INTERFACE: &str = "org.bluez.Device1";
const AGENT_PATH: &str = "/org/ferrix/bluetooth/agent";
/// The agent can show a six-digit code and take a yes/no answer.
const AGENT_CAPABILITY: &str = "DisplayYesNo";

type Properties = HashMap<String, OwnedValue>;
type Interfaces = HashMap<OwnedInterfaceName, Properties>;
type ManagedObjects = HashMap<OwnedObjectPath, Interfaces>;

#[zbus::proxy(
    interface = "org.freedesktop.DBus.ObjectManager",
    default_service = "org.bluez",
    default_path = "/"
)]
trait ObjectManager {
    fn get_managed_objects(&self) -> zbus::Result<ManagedObjects>;
}

#[zbus::proxy(interface = "org.bluez.Adapter1", default_service = "org.bluez")]
trait Adapter {
    #[zbus(property)]
    fn set_powered(&self, powered: bool) -> zbus::Result<()>;

    fn start_discovery(&self) -> zbus::Result<()>;

    fn stop_discovery(&self) -> zbus::Result<()>;

    fn set_discovery_filter(&self, filter: HashMap<&str, Value<'_>>) -> zbus::Result<()>;

    fn remove_device(&self, device: &ObjectPath<'_>) -> zbus::Result<()>;
}

#[zbus::proxy(interface = "org.bluez.Device1", default_service = "org.bluez")]
trait Device {
    fn pair(&self) -> zbus::Result<()>;

    fn cancel_pairing(&self) -> zbus::Result<()>;

    fn connect(&self) -> zbus::Result<()>;

    fn disconnect(&self) -> zbus::Result<()>;

    #[zbus(property)]
    fn set_trusted(&self, trusted: bool) -> zbus::Result<()>;
}

#[zbus::proxy(
    interface = "org.bluez.AgentManager1",
    default_service = "org.bluez",
    default_path = "/org/bluez"
)]
trait AgentManager {
    fn register_agent(&self, agent: &ObjectPath<'_>, capability: &str) -> zbus::Result<()>;

    fn request_default_agent(&self, agent: &ObjectPath<'_>) -> zbus::Result<()>;
}

/// Errors the agent returns to BlueZ (`org.bluez.Error.*`).
#[derive(Debug, DBusError)]
#[zbus(prefix = "org.bluez.Error")]
enum AgentError {
    #[zbus(error)]
    ZBus(zbus::Error),
    Rejected(String),
    Canceled(String),
}

/// The exported `org.bluez.Agent1` object.
struct Agent {
    callbacks: Arc<dyn AgentCallbacks>,
}

impl Agent {
    /// Hands a prompt to the session and waits (asynchronously) for the
    /// user's answer, the adapter's prompt timeout, or a cancellation.
    async fn ask(
        &self,
        device: &OwnedObjectPath,
        phase: PairingPhase,
    ) -> Result<PromptAnswer, AgentError> {
        let (answer, answered) = async_channel::bounded::<PromptAnswer>(1);
        self.callbacks.prompt(
            device.as_str(),
            phase,
            Box::new(move |reply| {
                let _ = answer.try_send(reply);
            }),
        );
        match answered.recv().await {
            Ok(PromptAnswer::Decline) => Err(AgentError::Rejected("declined".to_owned())),
            Ok(reply) => Ok(reply),
            Err(_) => Err(AgentError::Canceled("no answer".to_owned())),
        }
    }

    /// A yes/no prompt: only an explicit accept bonds.
    async fn confirm(
        &self,
        device: &OwnedObjectPath,
        phase: PairingPhase,
    ) -> Result<(), AgentError> {
        match self.ask(device, phase).await? {
            PromptAnswer::Accept => Ok(()),
            _ => Err(AgentError::Rejected("declined".to_owned())),
        }
    }
}

#[zbus::interface(name = "org.bluez.Agent1")]
impl Agent {
    fn release(&self) {}

    /// Legacy pairing: Settings asks for the remote's PIN. The typed PIN
    /// is this method's return value and nothing else keeps it.
    async fn request_pin_code(&self, device: OwnedObjectPath) -> Result<String, AgentError> {
        match self.ask(&device, PairingPhase::RequestPin).await? {
            PromptAnswer::Pin(pin) => Ok(pin.expose().to_owned()),
            _ => Err(AgentError::Rejected("no PIN entered".to_owned())),
        }
    }

    fn display_pin_code(&self, device: OwnedObjectPath, pincode: String) -> Result<(), AgentError> {
        match pincode.parse::<u32>() {
            Ok(code) if pincode.len() <= 6 => {
                self.callbacks.display(device.as_str(), code);
                Ok(())
            }
            _ => {
                self.callbacks
                    .refuse(device.as_str(), PairingFailure::PinRequired);
                Err(AgentError::Rejected(
                    "PIN display is not supported".to_owned(),
                ))
            }
        }
    }

    /// A `DisplayYesNo` agent is not asked to type a remote's passkey;
    /// refuse rather than guess.
    fn request_passkey(&self, device: OwnedObjectPath) -> Result<u32, AgentError> {
        self.callbacks
            .refuse(device.as_str(), PairingFailure::Failed);
        Err(AgentError::Rejected(
            "passkey entry is not supported".to_owned(),
        ))
    }

    fn display_passkey(&self, device: OwnedObjectPath, passkey: u32, _entered: u16) {
        self.callbacks.display(device.as_str(), passkey);
    }

    async fn request_confirmation(
        &self,
        device: OwnedObjectPath,
        passkey: u32,
    ) -> Result<(), AgentError> {
        self.confirm(&device, PairingPhase::ConfirmPasskey { passkey })
            .await
    }

    async fn request_authorization(&self, device: OwnedObjectPath) -> Result<(), AgentError> {
        self.confirm(&device, PairingPhase::Authorize).await
    }

    /// Services are authorized for bonded devices only, without a prompt.
    fn authorize_service(&self, device: OwnedObjectPath, _uuid: String) -> Result<(), AgentError> {
        if self.callbacks.service_allowed(device.as_str()) {
            Ok(())
        } else {
            Err(AgentError::Rejected("device is not paired".to_owned()))
        }
    }

    fn cancel(&self) {
        self.callbacks.cancel();
    }
}

pub(super) struct SystemBluezBus {
    connection: Connection,
}

/// Converts a zbus failure into a [`BusError`]: the D-Bus error name when
/// BlueZ answered, and a redacted text either way.
fn bus_error(error: zbus::Error) -> BusError {
    match error {
        zbus::Error::MethodError(name, message, _) => BusError {
            name: Some(name.to_string()),
            text: redact(message.as_deref().unwrap_or_default()),
        },
        other => BusError {
            name: None,
            text: redact(&other.to_string()),
        },
    }
}

fn text(error: impl std::fmt::Display) -> String {
    redact(&error.to_string())
}

impl SystemBluezBus {
    pub(super) fn connect() -> Result<Self, HardwareError> {
        Connection::system()
            .map(|connection| Self { connection })
            .map_err(|error| HardwareError::failed(Capability::Bluetooth, text(error)))
    }

    fn managed_objects(&self) -> Result<ManagedObjects, String> {
        ObjectManagerProxyBlocking::builder(&self.connection)
            .cache_properties(CacheProperties::No)
            .build()
            .map_err(text)?
            .get_managed_objects()
            .map_err(text)
    }

    fn adapter(&self, path: &str) -> Result<AdapterProxyBlocking<'_>, BusError> {
        AdapterProxyBlocking::builder(&self.connection)
            .path(path.to_owned())
            .map_err(bus_error)?
            .cache_properties(CacheProperties::No)
            .build()
            .map_err(bus_error)
    }

    fn device(&self, path: &str) -> Result<DeviceProxyBlocking<'_>, BusError> {
        DeviceProxyBlocking::builder(&self.connection)
            .path(path.to_owned())
            .map_err(bus_error)?
            .cache_properties(CacheProperties::No)
            .build()
            .map_err(bus_error)
    }
}

fn interface<'a>(interfaces: &'a Interfaces, name: &str) -> Option<&'a Properties> {
    interfaces
        .iter()
        .find(|(interface, _)| interface.as_str() == name)
        .map(|(_, properties)| properties)
}

fn flag(properties: &Properties, key: &str) -> bool {
    properties
        .get(key)
        .and_then(|value| bool::try_from(value).ok())
        .unwrap_or(false)
}

impl BluezBus for SystemBluezBus {
    fn adapters(&self) -> Result<Vec<AdapterRecord>, String> {
        Ok(self
            .managed_objects()?
            .into_iter()
            .filter_map(|(path, interfaces)| {
                interface(&interfaces, ADAPTER_INTERFACE).map(|properties| AdapterRecord {
                    path: path.to_string(),
                    powered: flag(properties, "Powered"),
                    discovering: flag(properties, "Discovering"),
                })
            })
            .collect())
    }

    fn devices(&self) -> Result<Vec<DeviceRecord>, String> {
        Ok(self
            .managed_objects()?
            .into_iter()
            .filter_map(|(path, interfaces)| {
                let properties = interface(&interfaces, DEVICE_INTERFACE)?;
                let string = |key: &str| {
                    properties
                        .get(key)
                        .and_then(|value| <&str>::try_from(value).ok())
                        .map(str::to_owned)
                };
                Some(DeviceRecord {
                    path: path.to_string(),
                    adapter: properties
                        .get("Adapter")
                        .and_then(|value| <&ObjectPath<'_>>::try_from(value).ok())
                        .map(ToString::to_string)
                        .unwrap_or_default(),
                    // `Name` only: BlueZ's `Alias` falls back to the address.
                    name: string("Name"),
                    paired: flag(properties, "Paired"),
                    connected: flag(properties, "Connected"),
                    rssi: properties
                        .get("RSSI")
                        .and_then(|value| i16::try_from(value).ok()),
                    icon: string("Icon"),
                    appearance: properties
                        .get("Appearance")
                        .and_then(|value| u16::try_from(value).ok()),
                })
            })
            .collect())
    }

    fn set_powered(&self, path: &str, powered: bool) -> Result<(), String> {
        self.adapter(path)
            .map_err(|error| error.describe())?
            .set_powered(powered)
            .map_err(text)
    }

    fn watch(&self, sender: Sender<BusEvent>) -> Result<(), String> {
        let changes = MatchRule::builder()
            .msg_type(MessageType::Signal)
            .sender(BLUEZ_SERVICE)
            .map_err(text)?
            .build();
        let changes =
            MessageIterator::for_match_rule(changes, &self.connection, Some(64)).map_err(text)?;
        let changed_sender = sender.clone();
        thread::Builder::new()
            .name("ferrix-bluetooth-bluez-signals".to_owned())
            .spawn(move || {
                // Never block here: an unbounded send, no D-Bus call. The
                // state thread coalesces bursts (discovery is chatty).
                for message in changes {
                    if message.is_ok() && changed_sender.send(BusEvent::Changed).is_err() {
                        break;
                    }
                }
            })
            .map_err(text)?;

        let owner = MatchRule::builder()
            .msg_type(MessageType::Signal)
            .sender("org.freedesktop.DBus")
            .map_err(text)?
            .interface("org.freedesktop.DBus")
            .map_err(text)?
            .member("NameOwnerChanged")
            .map_err(text)?
            .arg(0, BLUEZ_SERVICE)
            .map_err(text)?
            .build();
        let owner =
            MessageIterator::for_match_rule(owner, &self.connection, Some(8)).map_err(text)?;
        thread::Builder::new()
            .name("ferrix-bluetooth-bluez-owner".to_owned())
            .spawn(move || {
                for message in owner {
                    let Ok(message) = message else {
                        continue;
                    };
                    let Ok((_, _, new_owner)) =
                        message.body().deserialize::<(String, String, String)>()
                    else {
                        continue;
                    };
                    let event = if new_owner.is_empty() {
                        BusEvent::ServiceLost
                    } else {
                        // Give a new BlueZ a moment to export its objects
                        // (owner changes are rare; the queue cannot fill).
                        thread::sleep(Duration::from_millis(250));
                        BusEvent::Changed
                    };
                    if sender.send(event).is_err() {
                        break;
                    }
                }
            })
            .map_err(text)?;
        Ok(())
    }

    fn register_agent(&self, agent: Arc<dyn AgentCallbacks>) -> Result<(), BusError> {
        let server = self.connection.object_server();
        // Re-export with the current callbacks (a BlueZ restart keeps our
        // object but forgets the registration).
        let _ = server.remove::<Agent, _>(AGENT_PATH);
        server
            .at(AGENT_PATH, Agent { callbacks: agent })
            .map_err(bus_error)?;
        let path = ObjectPath::try_from(AGENT_PATH).map_err(|error| bus_error(error.into()))?;
        let manager = AgentManagerProxyBlocking::builder(&self.connection)
            .cache_properties(CacheProperties::No)
            .build()
            .map_err(bus_error)?;
        match manager
            .register_agent(&path, AGENT_CAPABILITY)
            .map_err(bus_error)
        {
            Ok(()) => {}
            Err(error) if error.is("AlreadyExists") => {}
            Err(error) => return Err(error),
        }
        manager.request_default_agent(&path).map_err(bus_error)
    }

    fn start_discovery(&self, adapter: &str) -> Result<(), BusError> {
        let proxy = self.adapter(adapter)?;
        // BR/EDR and LE, one result per device (RSSI updates still arrive
        // as property changes).
        let filter = HashMap::from([("Transport", Value::from("auto"))]);
        let _ = proxy.set_discovery_filter(filter);
        proxy.start_discovery().map_err(bus_error)
    }

    fn stop_discovery(&self, adapter: &str) -> Result<(), BusError> {
        self.adapter(adapter)?.stop_discovery().map_err(bus_error)
    }

    fn pair(&self, device: &str) -> Result<(), BusError> {
        self.device(device)?.pair().map_err(bus_error)
    }

    fn cancel_pairing(&self, device: &str) -> Result<(), BusError> {
        self.device(device)?.cancel_pairing().map_err(bus_error)
    }

    fn set_trusted(&self, device: &str) -> Result<(), BusError> {
        self.device(device)?.set_trusted(true).map_err(bus_error)
    }

    fn connect_device(&self, device: &str) -> Result<(), BusError> {
        self.device(device)?.connect().map_err(bus_error)
    }

    fn disconnect_device(&self, device: &str) -> Result<(), BusError> {
        self.device(device)?.disconnect().map_err(bus_error)
    }

    fn remove_device(&self, adapter: &str, device: &str) -> Result<(), BusError> {
        let path = ObjectPath::try_from(device).map_err(|error| bus_error(error.into()))?;
        self.adapter(adapter)?
            .remove_device(&path)
            .map_err(bus_error)
    }
}
