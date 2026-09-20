//! Resolved compositor configuration.
//!
//! Geometry precedence is: kernel command line `ferrix.display=WxH` (the QEMU
//! launcher pins the guest panel to the virtio-gpu size it created), then an
//! explicit `--display WxH` flag, then the versioned mock profile's `[display]`
//! table. Values are emulator-test configuration, never hardware facts.

#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use hardware_api::Orientation;
use target_profile::Profile;

/// Panel size in physical pixels.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DisplayGeometry {
    pub width: u32,
    pub height: u32,
}

impl DisplayGeometry {
    /// Parse `WxH`; both sides must be positive integers.
    pub fn parse(text: &str) -> Result<Self, String> {
        let (w, h) = text
            .trim()
            .split_once('x')
            .ok_or_else(|| format!("display geometry must be WxH, got {text:?}"))?;
        let width: u32 = w
            .parse()
            .map_err(|_| format!("display width must be an integer, got {w:?}"))?;
        let height: u32 = h
            .parse()
            .map_err(|_| format!("display height must be an integer, got {h:?}"))?;
        if width == 0 || height == 0 {
            return Err(format!("display geometry must be non-zero, got {text:?}"));
        }
        Ok(Self { width, height })
    }
}

/// Which source produced the pinned output geometry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GeometrySource {
    Cmdline,
    Flag,
    Profile,
}

impl GeometrySource {
    pub fn label(self) -> &'static str {
        match self {
            GeometrySource::Cmdline => "cmdline",
            GeometrySource::Flag => "flag",
            GeometrySource::Profile => "profile",
        }
    }
}

/// Extract `ferrix.display=WxH` from a kernel command line.
pub fn display_from_cmdline(cmdline: &str) -> Option<DisplayGeometry> {
    cmdline
        .split_whitespace()
        .filter_map(|word| word.strip_prefix("ferrix.display="))
        .find_map(|value| DisplayGeometry::parse(value).ok())
}

/// Fully resolved compositor settings.
#[derive(Clone, Debug, PartialEq)]
pub struct ShellConfig {
    pub profile_name: String,
    pub geometry: DisplayGeometry,
    pub geometry_source: GeometrySource,
    pub orientation: Orientation,
    pub scale: f64,
    pub touch: bool,
    pub socket_name: String,
    /// `appd` socket the compositor links to (with retries); `None` disables
    /// the link and the home gesture falls back to the oldest mapped window.
    pub appd_socket: Option<String>,
}

impl ShellConfig {
    /// Resolve the configuration: `cmdline` is the kernel command line text (or
    /// `None` when running nested), `flag` the optional `--display` override.
    pub fn resolve(
        profile: &Profile,
        flag: Option<DisplayGeometry>,
        cmdline: Option<&str>,
        socket_name: Option<&str>,
    ) -> Self {
        let (geometry, geometry_source) = match (cmdline.and_then(display_from_cmdline), flag) {
            (Some(g), _) => (g, GeometrySource::Cmdline),
            (None, Some(g)) => (g, GeometrySource::Flag),
            (None, None) => (
                DisplayGeometry {
                    width: profile.display.width,
                    height: profile.display.height,
                },
                GeometrySource::Profile,
            ),
        };
        Self {
            profile_name: profile.name.clone(),
            geometry,
            geometry_source,
            orientation: profile.display.orientation,
            scale: f64::from(profile.display.scale),
            touch: profile.input.touch,
            socket_name: socket_name.unwrap_or("wayland-0").to_owned(),
            appd_socket: None,
        }
    }

    /// Set the `appd` socket to link to (`None` disables the link).
    pub fn with_appd(mut self, appd_socket: Option<String>) -> Self {
        self.appd_socket = appd_socket;
        self
    }

    /// Value of the `appd` field in the `display-geometry` record.
    pub fn appd_label(&self) -> &str {
        self.appd_socket.as_deref().unwrap_or("disabled")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROFILE: &str = r#"
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

[audio]
route = "speaker"
"#;

    #[test]
    fn parses_checked_in_profile_shape() {
        let profile = Profile::parse(PROFILE).unwrap();
        assert_eq!(profile.name, "emulator-dev");
        assert_eq!(profile.display.width, 1080);
        assert_eq!(profile.display.height, 2340);
        assert_eq!(profile.display.orientation, Orientation::Portrait);
        assert!(profile.input.touch);
    }

    #[test]
    fn parses_repository_profile_file() {
        let text = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../targets/qemu-aarch64/profile.toml"
        ))
        .unwrap();
        let profile = Profile::parse(&text).unwrap();
        let config = ShellConfig::resolve(&profile, None, None, None);
        assert_eq!(
            config.geometry,
            DisplayGeometry {
                width: 1080,
                height: 2340
            }
        );
        assert_eq!(config.geometry_source, GeometrySource::Profile);
        assert_eq!(config.socket_name, "wayland-0");
        assert_eq!(config.appd_socket, None);
        assert_eq!(config.appd_label(), "disabled");
        let config = config.with_appd(Some("/run/ferrix/appd.sock".to_owned()));
        assert_eq!(config.appd_label(), "/run/ferrix/appd.sock");
    }

    #[test]
    fn rejects_bad_schema_and_geometry() {
        assert!(
            Profile::parse("schema_version = 2\nname = \"x\"\n[display]\nwidth = 1\nheight = 1\n")
                .is_err()
        );
        assert!(
            Profile::parse("schema_version = 1\nname = \"x\"\n[display]\nwidth = 0\nheight = 1\n")
                .is_err()
        );
        assert!(Profile::parse("not toml at all [").is_err());
    }

    #[test]
    fn geometry_parsing() {
        assert_eq!(
            DisplayGeometry::parse("830x1800").unwrap(),
            DisplayGeometry {
                width: 830,
                height: 1800
            }
        );
        assert!(DisplayGeometry::parse("830").is_err());
        assert!(DisplayGeometry::parse("0x10").is_err());
        assert!(DisplayGeometry::parse("ax10").is_err());
    }

    #[test]
    fn cmdline_geometry_is_extracted() {
        let cmdline = "console=ttyAMA0 panic=-1 ferrix.mode=agent ferrix.graphics=on ferrix.display=830x1800 ferrix.shell=mobile-shell";
        assert_eq!(
            display_from_cmdline(cmdline),
            Some(DisplayGeometry {
                width: 830,
                height: 1800
            })
        );
        assert_eq!(display_from_cmdline("console=ttyAMA0"), None);
        assert_eq!(display_from_cmdline("ferrix.display=bad"), None);
    }

    #[test]
    fn precedence_is_cmdline_then_flag_then_profile() {
        let profile = Profile::parse(PROFILE).unwrap();
        let flag = DisplayGeometry {
            width: 540,
            height: 1170,
        };
        let cfg = ShellConfig::resolve(&profile, Some(flag), Some("ferrix.display=830x1800"), None);
        assert_eq!(
            cfg.geometry,
            DisplayGeometry {
                width: 830,
                height: 1800
            }
        );
        assert_eq!(cfg.geometry_source, GeometrySource::Cmdline);

        let cfg = ShellConfig::resolve(
            &profile,
            Some(flag),
            Some("console=ttyAMA0"),
            Some("wayland-7"),
        );
        assert_eq!(cfg.geometry, flag);
        assert_eq!(cfg.geometry_source, GeometrySource::Flag);
        assert_eq!(cfg.socket_name, "wayland-7");

        let cfg = ShellConfig::resolve(&profile, None, None, None);
        assert_eq!(
            cfg.geometry,
            DisplayGeometry {
                width: 1080,
                height: 2340
            }
        );
        assert_eq!(cfg.geometry_source, GeometrySource::Profile);
    }
}
