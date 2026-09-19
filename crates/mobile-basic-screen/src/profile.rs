//! Minimal, strict parse of the fields this reference surface renders from the
//! versioned QEMU mock profile. Values are emulator-test configuration, not
//! hardware facts.

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Profile {
    pub schema_version: u32,
    pub name: String,
    pub display_width: u32,
    pub display_height: u32,
    pub battery_percent: u8,
    pub charging: bool,
    pub wifi: String,
    pub bluetooth_present: bool,
    pub modem: String,
    pub sim: String,
}

impl Profile {
    pub fn parse(input: &str) -> Result<Self, String> {
        let mut schema_version = None;
        let mut name = None;
        let mut display_width = None;
        let mut display_height = None;
        let mut battery_percent = None;
        let mut charging = None;
        let mut wifi = None;
        let mut bluetooth_present = None;
        let mut modem = None;
        let mut sim = None;
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
            let key = key.trim();
            let value = value.trim();
            match (section, key) {
                ("", "schema_version") => {
                    schema_version = Some(
                        value
                            .parse()
                            .map_err(|_| "schema_version must be an integer".to_owned())?,
                    );
                }
                ("", "name") => name = Some(value.trim_matches('"').to_owned()),
                ("display", "width") => {
                    display_width = Some(
                        value
                            .parse()
                            .map_err(|_| "display width must be an integer".to_owned())?,
                    );
                }
                ("display", "height") => {
                    display_height = Some(
                        value
                            .parse()
                            .map_err(|_| "display height must be an integer".to_owned())?,
                    );
                }
                ("power", "battery_percent") => {
                    battery_percent = Some(
                        value
                            .parse()
                            .map_err(|_| "battery_percent must be an integer".to_owned())?,
                    );
                }
                ("power", "charging") => {
                    charging = Some(
                        value
                            .parse()
                            .map_err(|_| "charging must be true or false".to_owned())?,
                    );
                }
                ("connectivity", "wifi") => wifi = Some(value.trim_matches('"').to_owned()),
                ("connectivity", "bluetooth_present") => {
                    bluetooth_present = Some(
                        value
                            .parse()
                            .map_err(|_| "bluetooth_present must be true or false".to_owned())?,
                    );
                }
                ("connectivity", "modem") => modem = Some(value.trim_matches('"').to_owned()),
                ("connectivity", "sim") => sim = Some(value.trim_matches('"').to_owned()),
                _ => {}
            }
        }
        let schema_version: u32 = schema_version.ok_or("missing schema_version")?;
        if schema_version != 1 {
            return Err(format!("unsupported schema_version: {schema_version}"));
        }
        Ok(Self {
            schema_version,
            name: name.ok_or("missing name")?,
            display_width: display_width.ok_or("missing display width")?,
            display_height: display_height.ok_or("missing display height")?,
            battery_percent: battery_percent.ok_or("missing power battery_percent")?,
            charging: charging.ok_or("missing power charging")?,
            wifi: wifi.unwrap_or_else(|| "unknown".to_owned()),
            bluetooth_present: bluetooth_present.unwrap_or(false),
            modem: modem.unwrap_or_else(|| "unknown".to_owned()),
            sim: sim.unwrap_or_else(|| "unknown".to_owned()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::Profile;

    const FIXTURE: &str = r#"
schema_version = 1
name = "emulator-dev"

[display]
width = 1080
height = 2340
scale = 1.0
orientation = "portrait"

[input]
touch = true
rotation = true

[power]
battery_percent = 75
charging = false

[connectivity]
wifi = "connected"
bluetooth_present = true
modem = "registered"
sim = "present"
"#;

    #[test]
    fn parses_rendered_fields() {
        let p = Profile::parse(FIXTURE).expect("fixture parses");
        assert_eq!(p.schema_version, 1);
        assert_eq!(p.name, "emulator-dev");
        assert_eq!((p.display_width, p.display_height), (1080, 2340));
        assert_eq!(p.battery_percent, 75);
        assert!(!p.charging);
        assert_eq!(p.wifi, "connected");
        assert!(p.bluetooth_present);
        assert_eq!(p.modem, "registered");
        assert_eq!(p.sim, "present");
    }

    #[test]
    fn rejects_missing_power() {
        let err = Profile::parse(
            "schema_version = 1\nname = \"x\"\n[display]\nwidth = 10\nheight = 20\n",
        )
        .expect_err("missing power must fail");
        assert!(err.contains("battery_percent"));
    }

    #[test]
    fn rejects_unknown_schema() {
        let err = Profile::parse("schema_version = 2\n").expect_err("schema 2 must fail");
        assert!(err.contains("unsupported schema_version"));
    }
}
