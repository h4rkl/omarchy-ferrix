//! Deterministic implementation of the portable hardware contracts for QEMU and tests.

use hardware_api::{Power, PowerEvent, PowerState};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Profile {
    pub schema_version: u32,
    pub name: String,
    pub battery_percent: u8,
    pub charging: bool,
}

impl Profile {
    pub fn parse(input: &str) -> Result<Self, String> {
        let (mut schema_version, mut name, mut battery_percent, mut charging) =
            (None, None, None, None);
        let mut section = "";
        for raw in input.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                section = &line[1..line.len() - 1];
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                return Err(format!("invalid profile line: {line}"));
            };
            match (section, key.trim()) {
                ("", "schema_version") => {
                    schema_version = Some(
                        value
                            .trim()
                            .parse()
                            .map_err(|_| "schema_version must be an integer")?,
                    )
                }
                ("", "name") => name = Some(value.trim().trim_matches('"').to_owned()),
                ("power", "battery_percent") => {
                    battery_percent = Some(
                        value
                            .trim()
                            .parse()
                            .map_err(|_| "battery_percent must be an integer")?,
                    )
                }
                ("power", "charging") => {
                    charging = Some(
                        value
                            .trim()
                            .parse()
                            .map_err(|_| "charging must be true or false")?,
                    )
                }
                _ => {}
            }
        }
        let profile = Self {
            schema_version: schema_version.ok_or("missing schema_version")?,
            name: name.ok_or("missing name")?,
            battery_percent: battery_percent.ok_or("missing power.battery_percent")?,
            charging: charging.ok_or("missing power.charging")?,
        };
        if profile.schema_version != 1 {
            return Err(format!(
                "unsupported schema_version {}",
                profile.schema_version
            ));
        }
        if profile.battery_percent > 100 {
            return Err("battery_percent must be 0..=100".to_owned());
        }
        Ok(profile)
    }
}

#[derive(Clone, Debug)]
pub struct EmulatorPower {
    state: PowerState,
}
impl EmulatorPower {
    pub fn new(profile: &Profile) -> Self {
        Self {
            state: PowerState {
                battery_percent: profile.battery_percent,
                charging: profile.charging,
            },
        }
    }
    pub fn set_charging(&mut self, charging: bool) -> PowerEvent {
        self.state.charging = charging;
        PowerEvent::Changed(self.state)
    }
}
impl Power for EmulatorPower {
    fn state(&self) -> PowerState {
        self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn charging_transition_is_deterministic() {
        let profile = Profile::parse("schema_version = 1\nname = \"test\"\n[power]\nbattery_percent = 75\ncharging = false\n").unwrap();
        let mut power = EmulatorPower::new(&profile);
        assert_eq!(
            power.set_charging(true),
            PowerEvent::Changed(PowerState {
                battery_percent: 75,
                charging: true
            })
        );
    }
}
