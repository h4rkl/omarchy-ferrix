//! Line-delimited JSON wire types.
//!
//! * The **state socket** (`core.sock`) uses the shared `ferrix-protocol`
//!   request/reply types for shells and applications.
//! * The **control channel** (a host-facing serial port on mocked targets) is
//!   for tests: [`ControlCommand`] in, [`Ack`] out.
//!
//! Both are versioned by [`ferrix_hardware_api::SCHEMA_VERSION`].

use ferrix_hardware_api::{
    AudioRoute, HardwareState, ModemRegistration, Orientation, SimState, SuspendOutcome, WifiState,
};
use serde::{Deserialize, Serialize};

/// Commands accepted on the mock-control channel.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
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
    /// Ask the guest to power off once the host has finished a check.
    Shutdown,
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
            ControlCommand::Shutdown => "shutdown",
        }
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
        assert!(
            serde_json::from_str::<ControlCommand>(r#"{"cmd":"set-sim","state":"maybe"}"#).is_err()
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
