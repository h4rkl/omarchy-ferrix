//! BlueZ system-D-Bus transport (`zbus`, blocking API).

use std::collections::HashMap;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

use ferrix_hardware_api::{Capability, HardwareError};
use zbus::MatchRule;
use zbus::blocking::{Connection, MessageIterator};
use zbus::message::Type as MessageType;
use zbus::names::OwnedInterfaceName;
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

use super::{AdapterRecord, BluezBus, BusEvent};

const BLUEZ_SERVICE: &str = "org.bluez";
const ADAPTER_INTERFACE: &str = "org.bluez.Adapter1";

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
    fn powered(&self) -> zbus::Result<bool>;

    #[zbus(property)]
    fn set_powered(&self, powered: bool) -> zbus::Result<()>;
}

pub(super) struct SystemBluezBus {
    connection: Connection,
}

impl SystemBluezBus {
    pub(super) fn connect() -> Result<Self, HardwareError> {
        Connection::system()
            .map(|connection| Self { connection })
            .map_err(|error| HardwareError::failed(Capability::Bluetooth, error.to_string()))
    }

    fn object_manager(&self) -> Result<ObjectManagerProxyBlocking<'_>, String> {
        ObjectManagerProxyBlocking::new(&self.connection).map_err(|error| error.to_string())
    }
}

impl BluezBus for SystemBluezBus {
    fn adapters(&self) -> Result<Vec<AdapterRecord>, String> {
        let objects = self
            .object_manager()?
            .get_managed_objects()
            .map_err(|error| error.to_string())?;
        let mut adapters = Vec::new();
        for (path, interfaces) in objects {
            if !interfaces
                .keys()
                .any(|name| name.as_str() == ADAPTER_INTERFACE)
            {
                continue;
            }
            let proxy = AdapterProxyBlocking::builder(&self.connection)
                .path(path.clone())
                .map_err(|error| error.to_string())?
                .build()
                .map_err(|error| error.to_string())?;
            adapters.push(AdapterRecord {
                path: path.to_string(),
                powered: proxy.powered().map_err(|error| error.to_string())?,
            });
        }
        Ok(adapters)
    }

    fn set_powered(&self, path: &str, powered: bool) -> Result<(), String> {
        AdapterProxyBlocking::builder(&self.connection)
            .path(path)
            .map_err(|error| error.to_string())?
            .build()
            .map_err(|error| error.to_string())?
            .set_powered(powered)
            .map_err(|error| error.to_string())
    }

    fn watch(&self, sender: Sender<BusEvent>) -> Result<(), String> {
        let changes = MatchRule::builder()
            .msg_type(MessageType::Signal)
            .sender(BLUEZ_SERVICE)
            .map_err(|error| error.to_string())?
            .build();
        let changes = MessageIterator::for_match_rule(changes, &self.connection, Some(64))
            .map_err(|error| error.to_string())?;
        let changed_sender = sender.clone();
        thread::Builder::new()
            .name("ferrix-bluetooth-bluez-signals".to_owned())
            .spawn(move || {
                for message in changes {
                    if message.is_ok() {
                        // BlueZ can emit a burst for one transition. The
                        // adapter publishes only the resulting state delta.
                        thread::sleep(Duration::from_millis(25));
                        if changed_sender.send(BusEvent::Changed).is_err() {
                            break;
                        }
                    }
                }
            })
            .map_err(|error| error.to_string())?;

        let owner = MatchRule::builder()
            .msg_type(MessageType::Signal)
            .sender("org.freedesktop.DBus")
            .map_err(|error| error.to_string())?
            .interface("org.freedesktop.DBus")
            .map_err(|error| error.to_string())?
            .member("NameOwnerChanged")
            .map_err(|error| error.to_string())?
            .arg(0, BLUEZ_SERVICE)
            .map_err(|error| error.to_string())?
            .build();
        let owner = MessageIterator::for_match_rule(owner, &self.connection, Some(8))
            .map_err(|error| error.to_string())?;
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
                        thread::sleep(Duration::from_millis(250));
                        BusEvent::Changed
                    };
                    if sender.send(event).is_err() {
                        break;
                    }
                }
            })
            .map_err(|error| error.to_string())?;
        Ok(())
    }
}
