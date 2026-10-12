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

pub fn apply(services: &HardwareServices, command: &ControlCommand) -> Result<Applied, String> {
    let mut events = Vec::new();
    let mut outcome = None;
    let mut shutdown = false;
    let result: Result<(), HardwareError> = (|| {
        if let Some(request) = command.bluetooth_command() {
            // A real capability call: the adapter publishes the session
            // events itself; the ack names the resulting summary.
            request.apply(&*services.bluetooth)?;
            events.push(HardwareEvent::BluetoothSessionChanged(
                services.bluetooth.bluetooth_session()?.summary(),
            ));
            return Ok(());
        }
        match command.clone() {
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
            ControlCommand::SetBluetoothPairingOutcome { outcome } => mock(
                services,
                MockCommand::SetBluetoothPairingOutcome { outcome },
                &mut events,
            )?,
            ControlCommand::BluetoothTick { ms } => {
                mock(services, MockCommand::BluetoothTick { ms }, &mut events)?
            }
            ControlCommand::BluetoothIncoming { device } => mock(
                services,
                MockCommand::BluetoothIncomingPairing { device },
                &mut events,
            )?,
            // Routed above through `bluetooth_command`.
            ControlCommand::BluetoothDiscovery { .. }
            | ControlCommand::BluetoothPair { .. }
            | ControlCommand::BluetoothReply { .. }
            | ControlCommand::BluetoothReplyPin { .. }
            | ControlCommand::BluetoothCancel
            | ControlCommand::BluetoothConnect { .. }
            | ControlCommand::BluetoothDisconnect { .. }
            | ControlCommand::BluetoothForget { .. } => {}
            ControlCommand::Shutdown => shutdown = true,
            // A process action, not hardware state: the control handler in
            // main.rs performs it (see daemon_kill.rs) and never calls here.
            ControlCommand::KillDaemon { .. } => {}
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
        HardwareCommand::SetRotationLock { locked } => {
            services.display.set_rotation_lock(locked)?;
        }
        HardwareCommand::SetAudioRoute { route } => {
            services.audio.set_audio_route(route)?;
        }
        HardwareCommand::SetAudioVolume { percent } => {
            services.audio.set_audio_volume(percent)?;
        }
        HardwareCommand::SetAudioMuted { muted } => {
            services.audio.set_audio_muted(muted)?;
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
            &ControlCommand::SetPower {
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
                &ControlCommand::SetPower {
                    charging: None,
                    battery_percent: None
                }
            )
            .is_err()
        );
        assert!(
            apply(
                &s,
                &ControlCommand::SetPower {
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
            &ControlCommand::SetWifi {
                state: WifiState::Disabled,
            },
        )
        .unwrap();
        apply(
            &s,
            &ControlCommand::SetOrientation {
                orientation: Orientation::Landscape,
            },
        )
        .unwrap();
        apply(
            &s,
            &ControlCommand::SetAudio {
                route: AudioRoute::Headphones,
            },
        )
        .unwrap();
        apply(&s, &ControlCommand::SetBrightness { percent: 10 }).unwrap();
        let applied = apply(
            &s,
            &ControlCommand::Suspend {
                outcome: Some(SuspendOutcome::Failed),
            },
        )
        .unwrap();
        assert_eq!(applied.outcome, Some(SuspendOutcome::Failed));
        assert!(apply(&s, &ControlCommand::Shutdown).unwrap().shutdown);
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
        apply_product(&s, HardwareCommand::SetRotationLock { locked: true }).unwrap();
        apply(
            &s,
            &ControlCommand::SetOrientation {
                orientation: Orientation::Landscape,
            },
        )
        .unwrap();
        let display = s.snapshot().display;
        assert!(display.rotation_locked);
        assert_eq!(display.orientation, Orientation::Portrait, "lock holds");
        apply_product(&s, HardwareCommand::SetRotationLock { locked: false }).unwrap();
        assert_eq!(s.snapshot().display.orientation, Orientation::Landscape);
    }

    #[test]
    fn audio_level_commands_reach_the_capability_and_stay_unsupported_on_qemu() {
        let s = services();
        assert_eq!(s.snapshot().audio.output, None);
        for command in [
            HardwareCommand::SetAudioVolume { percent: 40 },
            HardwareCommand::SetAudioMuted { muted: true },
        ] {
            assert_eq!(
                apply_product(&s, command),
                Err(HardwareError::unsupported(Capability::Audio))
            );
        }
        assert_eq!(s.snapshot().audio.output, None);
    }

    #[test]
    fn bluetooth_verbs_run_the_product_capability_and_report_the_session() {
        let s = services();
        let applied = apply(&s, &ControlCommand::BluetoothDiscovery { active: true }).unwrap();
        assert_eq!(applied.events.len(), 1);
        let session = s.bluetooth.bluetooth_session().unwrap();
        assert!(session.discovery.active);
        let headphones = session
            .devices
            .iter()
            .find(|d| d.name.as_deref() == Some("Ferrix Mock Headphones"))
            .unwrap()
            .id
            .clone();
        apply(
            &s,
            &ControlCommand::BluetoothPair {
                device: headphones.clone(),
            },
        )
        .unwrap();
        apply(&s, &ControlCommand::BluetoothReply { accept: true }).unwrap();
        apply(&s, &ControlCommand::BluetoothTick { ms: 1_000 }).unwrap();
        let session = s.bluetooth.bluetooth_session().unwrap();
        assert!(session.device(&headphones).unwrap().paired);
        assert!(
            apply(&s, &ControlCommand::BluetoothReply { accept: true }).is_err(),
            "no prompt is waiting"
        );
    }

    /// Replays every checked-in `bluetooth-*` scenario against the core's
    /// command handler and the mock controller in-process (`mock`
    /// evidence; `just qemu-scenario-check` runs the same files in QEMU).
    #[test]
    fn bluetooth_scenarios_replay_against_the_mock_core() {
        use ferrix_scenario::{Scenario, Service, is_subset, match_in_order};
        use serde_json::{Value, json};
        let dir = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../targets/qemu-aarch64/scenarios"
        );
        let mut files: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("bluetooth-") && n.ends_with(".toml"))
            })
            .collect();
        files.sort();
        assert!(files.len() >= 3, "bluetooth scenarios are checked in");
        let s = services();
        let events = s.subscribe();
        for file in files {
            let scenario = Scenario::parse(&std::fs::read_to_string(&file).unwrap()).unwrap();
            for (index, step) in scenario.steps.iter().enumerate() {
                let at = format!("{} step {index}", scenario.name);
                assert_eq!(step.service, Service::Core, "{at}");
                // A command the channel cannot parse (a malformed PIN) is an
                // error ack there too.
                let parsed = serde_json::from_value::<ControlCommand>(step.command.clone())
                    .map_err(|e| e.to_string());
                let result = parsed
                    .clone()
                    .and_then(|command| apply(&s, &command).map(|applied| (command, applied)));
                let ack = match &result {
                    Ok((command, applied)) => {
                        serde_json::to_value(ack_for(&s, command, applied)).unwrap()
                    }
                    Err(reason) => json!({"status": "error", "reason": reason}),
                };
                assert_eq!(result.is_err(), step.expect_ack_error, "{at}: {ack}");
                if let Some(expected) = &step.ack {
                    assert!(is_subset(expected, &ack), "{at}: ack {ack}");
                }
                let records: Vec<Value> = events
                    .try_iter()
                    .map(|event| {
                        let mut record = serde_json::to_value(event).unwrap();
                        record["evidence"] = json!("mock");
                        record
                    })
                    .collect();
                // Records of other processes (ferrix-appd's `app-intent`, the
                // attention thread's `bluetooth-attention`) only exist in the
                // guest; in-process only the hardware events are checked.
                let expect: Vec<Value> = step
                    .expect
                    .iter()
                    .filter(|e| {
                        !matches!(
                            e["event"].as_str(),
                            Some("app-intent" | "bluetooth-attention")
                        )
                    })
                    .cloned()
                    .collect();
                if let Err(missing) = match_in_order(&records, &expect) {
                    panic!("{at}: missing {} in {records:?}", expect[missing]);
                }
            }
        }
    }

    #[test]
    fn bluetooth_devices_come_from_the_checked_in_profile() {
        let s = services();
        let devices = s.bluetooth.bluetooth_devices().unwrap();
        assert_eq!(devices.len(), 3);
        assert!(
            devices[0].connected,
            "the connected mock earbuds sort first"
        );
    }
}
