//! Command handling: applies a [`ControlCommand`] through the session's
//! `ferrix-hardware-api` handles and reports the events it produced. No adapter type
//! is named here; mocked mutations go through the optional `MockControl`
//! capability the session exposes.

use ferrix_hardware_api::{
    Capability, HardwareCommand, HardwareError, HardwareEvent, MockCommand, SuspendOutcome,
};
use ferrix_session::HardwareServices;

use crate::protocol::{Ack, ControlCommand};

pub struct Applied {
    pub events: Vec<HardwareEvent>,
    pub outcome: Option<SuspendOutcome>,
    pub shutdown: bool,
}

fn mock(
    services: &HardwareServices,
    command: MockCommand,
    events: &mut Vec<HardwareEvent>,
) -> Result<(), HardwareError> {
    let control = services
        .mock_control
        .as_ref()
        .ok_or(HardwareError::unsupported(Capability::MockControl))?;
    if let Some(event) = control.apply(command)? {
        events.push(event);
    }
    Ok(())
}

pub fn apply(services: &HardwareServices, command: ControlCommand) -> Result<Applied, String> {
    let mut events = Vec::new();
    let mut outcome = None;
    let mut shutdown = false;
    let result: Result<(), HardwareError> = (|| {
        match command {
            ControlCommand::State => {}
            ControlCommand::SetPower {
                charging,
                battery_percent,
            } => {
                if charging.is_none() && battery_percent.is_none() {
                    return Err(HardwareError::invalid(
                        Capability::Power,
                        "set-power needs charging and/or battery_percent",
                    ));
                }
                if let Some(battery_percent) = battery_percent {
                    mock(
                        services,
                        MockCommand::SetBatteryPercent { battery_percent },
                        &mut events,
                    )?;
                }
                if let Some(charging) = charging {
                    mock(services, MockCommand::SetCharging { charging }, &mut events)?;
                }
            }
            ControlCommand::SetWifi { state } => {
                mock(services, MockCommand::SetWifi { state }, &mut events)?
            }
            ControlCommand::SetBluetooth { present } => mock(
                services,
                MockCommand::SetBluetoothPresent { present },
                &mut events,
            )?,
            ControlCommand::SetAudio { route } => {
                mock(services, MockCommand::SetAudioRoute { route }, &mut events)?
            }
            ControlCommand::SetModem { registration } => mock(
                services,
                MockCommand::SetModemRegistration { registration },
                &mut events,
            )?,
            ControlCommand::SetSim { state } => {
                mock(services, MockCommand::SetSim { state }, &mut events)?
            }
            ControlCommand::SetOrientation { orientation } => mock(
                services,
                MockCommand::SetOrientation { orientation },
                &mut events,
            )?,
            ControlCommand::SetBrightness { percent } => {
                // A real capability call, not a mock mutation: the adapter
                // emits display-changed itself.
                services.display.set_brightness(percent)?;
                events.push(HardwareEvent::DisplayChanged(
                    services.display.display_state(),
                ));
            }
            ControlCommand::Suspend { outcome: wanted } => {
                if let Some(wanted) = wanted {
                    mock(
                        services,
                        MockCommand::SetSuspendOutcome { outcome: wanted },
                        &mut events,
                    )?;
                }
                let lifecycle_before = services.lifecycle.lifecycle_state();
                let result = services.lifecycle.suspend();
                let lifecycle = services.lifecycle.lifecycle_state();
                let reported = match result {
                    Ok(o) => o,
                    Err(HardwareError::Failed { .. }) => SuspendOutcome::Failed,
                    Err(other) => return Err(other),
                };
                if lifecycle != lifecycle_before {
                    events.push(HardwareEvent::Suspended {
                        outcome: reported,
                        lifecycle,
                    });
                }
                outcome = Some(reported);
            }
            ControlCommand::Shutdown => shutdown = true,
        }
        Ok(())
    })();
    result.map_err(|e| e.to_string())?;
    Ok(Applied {
        events,
        outcome,
        shutdown,
    })
}

pub fn ack_for(services: &HardwareServices, command: &ControlCommand, applied: &Applied) -> Ack {
    Ack::Ok {
        ack: command.name().to_owned(),
        events: applied.events.iter().map(|e| e.name().to_owned()).collect(),
        outcome: applied.outcome,
        state: services.snapshot(),
    }
}

/// Apply a user-facing command through the portable capability traits. This
/// path is available to shells/apps and never exposes mock-only mutations.
pub fn apply_product(
    services: &HardwareServices,
    command: HardwareCommand,
) -> Result<(), HardwareError> {
    match command {
        HardwareCommand::SetWifiEnabled { enabled } => {
            services.network.set_wifi_enabled(enabled)?;
        }
        HardwareCommand::SetBluetoothEnabled { enabled } => {
            services.bluetooth.set_bluetooth_enabled(enabled)?;
        }
        HardwareCommand::SetBrightness { percent } => {
            services.display.set_brightness(percent)?;
        }
        HardwareCommand::SetAudioRoute { route } => {
            services.audio.set_audio_route(route)?;
        }
        HardwareCommand::Suspend => {
            services.lifecycle.suspend()?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrix_hardware_api::{AudioRoute, Orientation, WifiState};
    use ferrix_session::{BootstrapOptions, bootstrap};

    fn services() -> HardwareServices {
        bootstrap(BootstrapOptions::default().with_profile_path(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../targets/qemu-aarch64/profile.toml"
        )))
        .unwrap()
        .services
    }

    #[test]
    fn set_power_applies_battery_then_charging() {
        let s = services();
        let applied = apply(
            &s,
            ControlCommand::SetPower {
                charging: Some(true),
                battery_percent: Some(40),
            },
        )
        .unwrap();
        assert_eq!(applied.events.len(), 2);
        let state = s.snapshot();
        assert!(state.power.charging);
        assert_eq!(state.power.battery_percent, 40);
        match ack_for(&s, &ControlCommand::Shutdown, &applied) {
            Ack::Ok { events, .. } => assert_eq!(events, ["power-changed", "power-changed"]),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn empty_set_power_is_an_error_not_a_success() {
        let s = services();
        assert!(
            apply(
                &s,
                ControlCommand::SetPower {
                    charging: None,
                    battery_percent: None
                }
            )
            .is_err()
        );
        assert!(
            apply(
                &s,
                ControlCommand::SetPower {
                    charging: None,
                    battery_percent: Some(120)
                }
            )
            .is_err()
        );
    }

    #[test]
    fn every_other_command_routes() {
        let s = services();
        apply(
            &s,
            ControlCommand::SetWifi {
                state: WifiState::Disabled,
            },
        )
        .unwrap();
        apply(
            &s,
            ControlCommand::SetOrientation {
                orientation: Orientation::Landscape,
            },
        )
        .unwrap();
        apply(
            &s,
            ControlCommand::SetAudio {
                route: AudioRoute::Headphones,
            },
        )
        .unwrap();
        apply(&s, ControlCommand::SetBrightness { percent: 10 }).unwrap();
        let applied = apply(
            &s,
            ControlCommand::Suspend {
                outcome: Some(SuspendOutcome::Failed),
            },
        )
        .unwrap();
        assert_eq!(applied.outcome, Some(SuspendOutcome::Failed));
        assert!(apply(&s, ControlCommand::Shutdown).unwrap().shutdown);
        let state = s.snapshot();
        assert_eq!(state.network.wifi, WifiState::Disabled);
        assert_eq!(state.display.orientation, Orientation::Landscape);
        assert_eq!(state.audio.route, AudioRoute::Headphones);
        assert_eq!(state.display.brightness_percent, 10);
        assert_eq!(state.lifecycle.suspend_count, 1);
    }

    #[test]
    fn product_commands_use_capability_traits() {
        let s = services();
        apply_product(&s, HardwareCommand::SetWifiEnabled { enabled: false }).unwrap();
        apply_product(&s, HardwareCommand::SetBrightness { percent: 31 }).unwrap();
        apply_product(
            &s,
            HardwareCommand::SetAudioRoute {
                route: AudioRoute::Bluetooth,
            },
        )
        .unwrap();
        assert_eq!(s.snapshot().network.wifi, WifiState::Disabled);
        assert_eq!(s.snapshot().display.brightness_percent, 31);
        assert_eq!(s.snapshot().audio.route, AudioRoute::Bluetooth);
    }
}
