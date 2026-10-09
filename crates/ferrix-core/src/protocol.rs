//! Line-delimited JSON wire types.
//!
//! * The **state socket** (`core.sock`) uses the shared `ferrix-protocol`
//!   request/reply types for shells and applications.
//! * The **control channel** (a host-facing serial port on mocked targets) is
//!   for tests: [`ControlCommand`] in, [`Ack`] out.
//!
//! Both are versioned by [`ferrix_hardware_api::SCHEMA_VERSION`].

use ferrix_hardware_api::{
    AudioRoute, BluetoothCommand, BluetoothPin, HardwareState, MockPairingOutcome,
    ModemRegistration, Orientation, SimState, SuspendOutcome, WifiState,
};
use serde::{Deserialize, Serialize};

/// Commands accepted on the mock-control channel.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "kebab-case")]
pub enum ControlCommand {
    /// Report the current state without changing anything.
    State,
    SetPower {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        charging: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        battery_percent: Option<u8>,
    },
    SetWifi {
        state: WifiState,
    },
    SetBluetooth {
        present: bool,
    },
    SetAudio {
        route: AudioRoute,
    },
    SetModem {
        registration: ModemRegistration,
    },
    SetSim {
        state: SimState,
    },
    SetOrientation {
        orientation: Orientation,
    },
    SetBrightness {
        percent: u8,
    },
    /// Run a suspend cycle; `outcome` (mocked targets only) chooses its result.
    Suspend {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        outcome: Option<SuspendOutcome>,
    },
    /// Overrides how every following mock pairing ends (`null` restores
    /// each profile device's own outcome). Mocked targets only.
    SetBluetoothPairingOutcome {
        #[serde(default)]
        outcome: Option<MockPairingOutcome>,
    },
    /// Advances the mock Bluetooth clock. Mocked targets only.
    BluetoothTick {
        ms: u32,
    },
    /// A remote (mock id) starts pairing with this phone. Mocked targets
    /// only.
    BluetoothIncoming {
        device: String,
    },
    /// Client verbs: the same product Bluetooth operations Settings sends,
    /// through the portable capability (a real call, not a mock mutation).
    BluetoothDiscovery {
        active: bool,
    },
    BluetoothPair {
        device: String,
    },
    BluetoothReply {
        accept: bool,
    },
    /// Answers a PIN request. The PIN is never echoed: records name the
    /// command only.
    BluetoothReplyPin {
        pin: BluetoothPin,
    },
    BluetoothCancel,
    BluetoothConnect {
        device: String,
    },
    BluetoothDisconnect {
        device: String,
    },
    BluetoothForget {
        device: String,
    },
    /// Ask the guest to power off once the host has finished a check.
    Shutdown,
    /// Fault injection (mocked targets only): SIGKILL the named supervised
    /// daemon so a test can watch init restart it. Only the names in
    /// `daemon_kill::KILLABLE` are accepted.
    KillDaemon {
        name: String,
    },
}

impl ControlCommand {
    pub fn name(&self) -> &'static str {
        match self {
            ControlCommand::State => "state",
            ControlCommand::SetPower { .. } => "set-power",
            ControlCommand::SetWifi { .. } => "set-wifi",
            ControlCommand::SetBluetooth { .. } => "set-bluetooth",
            ControlCommand::SetAudio { .. } => "set-audio",
            ControlCommand::SetModem { .. } => "set-modem",
            ControlCommand::SetSim { .. } => "set-sim",
            ControlCommand::SetOrientation { .. } => "set-orientation",
            ControlCommand::SetBrightness { .. } => "set-brightness",
            ControlCommand::Suspend { .. } => "suspend",
            ControlCommand::SetBluetoothPairingOutcome { .. } => "set-bluetooth-pairing-outcome",
            ControlCommand::BluetoothTick { .. } => "bluetooth-tick",
            ControlCommand::BluetoothIncoming { .. } => "bluetooth-incoming",
            ControlCommand::BluetoothReplyPin { .. } => "bluetooth-reply-pin",
            ControlCommand::BluetoothDiscovery { .. } => "bluetooth-discovery",
            ControlCommand::BluetoothPair { .. } => "bluetooth-pair",
            ControlCommand::BluetoothReply { .. } => "bluetooth-reply",
            ControlCommand::BluetoothCancel => "bluetooth-cancel",
            ControlCommand::BluetoothConnect { .. } => "bluetooth-connect",
            ControlCommand::BluetoothDisconnect { .. } => "bluetooth-disconnect",
            ControlCommand::BluetoothForget { .. } => "bluetooth-forget",
            ControlCommand::Shutdown => "shutdown",
            ControlCommand::KillDaemon { .. } => "kill-daemon",
        }
    }

    /// The product request behind a Bluetooth client verb.
    pub fn bluetooth_command(&self) -> Option<BluetoothCommand> {
        Some(match self {
            ControlCommand::BluetoothDiscovery { active: true } => BluetoothCommand::StartDiscovery,
            ControlCommand::BluetoothDiscovery { active: false } => BluetoothCommand::StopDiscovery,
            ControlCommand::BluetoothPair { device } => BluetoothCommand::Pair {
                device: device.clone(),
            },
            ControlCommand::BluetoothReply { accept } => {
                BluetoothCommand::ReplyPairing { accept: *accept }
            }
            ControlCommand::BluetoothReplyPin { pin } => {
                BluetoothCommand::ReplyPin { pin: pin.clone() }
            }
            ControlCommand::BluetoothCancel => BluetoothCommand::CancelPairing,
            ControlCommand::BluetoothConnect { device } => BluetoothCommand::Connect {
                device: device.clone(),
            },
            ControlCommand::BluetoothDisconnect { device } => BluetoothCommand::Disconnect {
                device: device.clone(),
            },
            ControlCommand::BluetoothForget { device } => BluetoothCommand::Forget {
                device: device.clone(),
            },
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum Ack {
    Ok {
        ack: String,
        /// Names of the events the command produced, in order.
        events: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        outcome: Option<SuspendOutcome>,
        state: HardwareState,
    },
    Error {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ack: Option<String>,
        reason: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_commands_use_the_documented_wire_form() {
        let cmd: ControlCommand =
            serde_json::from_str(r#"{"cmd":"set-power","charging":true}"#).unwrap();
        assert_eq!(
            cmd,
            ControlCommand::SetPower {
                charging: Some(true),
                battery_percent: None
            }
        );
        let cmd: ControlCommand =
            serde_json::from_str(r#"{"cmd":"set-wifi","state":"disconnected"}"#).unwrap();
        assert_eq!(
            cmd,
            ControlCommand::SetWifi {
                state: WifiState::Disconnected
            }
        );
        let cmd: ControlCommand =
            serde_json::from_str(r#"{"cmd":"suspend","outcome":"resumed"}"#).unwrap();
        assert_eq!(
            cmd,
            ControlCommand::Suspend {
                outcome: Some(SuspendOutcome::Resumed)
            }
        );
        assert!(serde_json::from_str::<ControlCommand>(r#"{"cmd":"explode"}"#).is_err());
        let cmd: ControlCommand =
            serde_json::from_str(r#"{"cmd":"kill-daemon","name":"ferrix-networkd"}"#).unwrap();
        assert_eq!(
            cmd,
            ControlCommand::KillDaemon {
                name: "ferrix-networkd".to_owned()
            }
        );
        assert_eq!(cmd.name(), "kill-daemon");
        assert!(
            serde_json::from_str::<ControlCommand>(r#"{"cmd":"set-sim","state":"maybe"}"#).is_err()
        );
    }

    #[test]
    fn bluetooth_verbs_parse_and_map_to_product_requests() {
        let cmd: ControlCommand =
            serde_json::from_str(r#"{"cmd":"bluetooth-pair","device":"mock-bt-found-0"}"#).unwrap();
        assert_eq!(cmd.name(), "bluetooth-pair");
        assert_eq!(
            cmd.bluetooth_command(),
            Some(BluetoothCommand::Pair {
                device: "mock-bt-found-0".into()
            })
        );
        let cmd: ControlCommand =
            serde_json::from_str(r#"{"cmd":"set-bluetooth-pairing-outcome","outcome":"timeout"}"#)
                .unwrap();
        assert_eq!(
            cmd,
            ControlCommand::SetBluetoothPairingOutcome {
                outcome: Some(MockPairingOutcome::Timeout)
            }
        );
        assert_eq!(cmd.bluetooth_command(), None);
        let reset: ControlCommand =
            serde_json::from_str(r#"{"cmd":"set-bluetooth-pairing-outcome"}"#).unwrap();
        assert_eq!(
            reset,
            ControlCommand::SetBluetoothPairingOutcome { outcome: None }
        );
    }

    #[test]
    fn control_state_parses() {
        assert_eq!(
            serde_json::from_str::<ControlCommand>(r#"{"cmd":"state"}"#).unwrap(),
            ControlCommand::State
        );
    }
}
