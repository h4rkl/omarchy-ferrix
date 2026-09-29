//! UPower system-D-Bus transport (`zbus`, blocking API).

use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

use ferrix_hardware_api::{Capability, HardwareError};
use zbus::MatchRule;
use zbus::blocking::{Connection, MessageIterator};
use zbus::message::Type as MessageType;

use super::{BusEvent, PowerReading, UpowerBus};

const UPOWER_SERVICE: &str = "org.freedesktop.UPower";
const DISPLAY_DEVICE_PATH: &str = "/org/freedesktop/UPower/devices/DisplayDevice";
const DEVICE_INTERFACE: &str = "org.freedesktop.UPower.Device";

#[zbus::proxy(
    interface = "org.freedesktop.UPower.Device",
    default_service = "org.freedesktop.UPower"
)]
trait Device {
    #[zbus(property)]
    fn percentage(&self) -> zbus::Result<f64>;

    #[zbus(property)]
    fn state(&self) -> zbus::Result<u32>;

    #[zbus(property)]
    fn is_present(&self) -> zbus::Result<bool>;
}

pub(super) struct SystemUpowerBus {
    connection: Connection,
}

impl SystemUpowerBus {
    pub(super) fn connect() -> Result<Self, HardwareError> {
        Connection::system()
            .map(|connection| Self { connection })
            .map_err(|error| HardwareError::failed(Capability::Power, error.to_string()))
    }

    fn display_device(&self) -> Result<DeviceProxyBlocking<'_>, String> {
        DeviceProxyBlocking::builder(&self.connection)
            .path(DISPLAY_DEVICE_PATH)
            .map_err(|error| error.to_string())?
            .build()
            .map_err(|error| error.to_string())
    }
}

impl UpowerBus for SystemUpowerBus {
    fn display_device(&self) -> Result<PowerReading, String> {
        let device = self.display_device()?;
        Ok(PowerReading {
            percentage: device.percentage().map_err(|error| error.to_string())?,
            state: device.state().map_err(|error| error.to_string())?,
            present: device.is_present().map_err(|error| error.to_string())?,
        })
    }

    fn watch(&self, sender: Sender<BusEvent>) -> Result<(), String> {
        let properties = MatchRule::builder()
            .msg_type(MessageType::Signal)
            .sender(UPOWER_SERVICE)
            .map_err(|error| error.to_string())?
            .interface("org.freedesktop.DBus.Properties")
            .map_err(|error| error.to_string())?
            .member("PropertiesChanged")
            .map_err(|error| error.to_string())?
            .path(DISPLAY_DEVICE_PATH)
            .map_err(|error| error.to_string())?
            .arg(0, DEVICE_INTERFACE)
            .map_err(|error| error.to_string())?
            .build();
        let properties = MessageIterator::for_match_rule(properties, &self.connection, Some(16))
            .map_err(|error| error.to_string())?;
        let changed_sender = sender.clone();
        thread::Builder::new()
            .name("ferrix-power-upower-signals".to_owned())
            .spawn(move || {
                for message in properties {
                    if message.is_ok() {
                        // Coalesce a short burst into the authoritative display
                        // device snapshot. Duplicate states are suppressed later.
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
            .arg(0, UPOWER_SERVICE)
            .map_err(|error| error.to_string())?
            .build();
        let owner = MessageIterator::for_match_rule(owner, &self.connection, Some(8))
            .map_err(|error| error.to_string())?;
        thread::Builder::new()
            .name("ferrix-power-upower-owner".to_owned())
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
