//! Omarchy Ferrix home surface (QEMU reference launcher).
//!
//! A small software-rendered Wayland client (xdg app_id `org.ferrix.home`)
//! that draws the status bar, the launcher grid from the application catalog,
//! quick settings, and notifications. Applications are independent processes
//! launched and tracked by `ferrix_appd`; this surface only requests launches and
//! shows their lifecycle (running dot, starting state, crash notice). It is
//! QEMU/mock evidence for the graphics path, not the product shell and not a
//! physical-device claim.
//!
//! Hardware state (battery, network, modem) comes from the persistent
//! `ferrix-core` over its state socket; the profile supplies only the panel
//! geometry and name. Product code depends on `ferrix-hardware-api`, never on an
//! adapter.
//!
//! Usage:
//!   ferrix-basic-screen [profile.toml] [--core /run/ferrix/core.sock]
//!                       [--ferrix_appd /run/ferrix/ferrix_appd.sock]   # guest: connect to Wayland
//!   ferrix-basic-screen [profile.toml] --preview p.ppm [--page launcher]
//!                                                        # any host: render PPM
//!
//! The profile defaults to the guest copy when present, else the repository
//! copy, so the manifest's native launch command works in the
//! guest and nested. The preview has no daemons, so it bootstraps a session
//! in-process and loads the catalog from the descriptor directory.

mod catalog;
mod interaction;
mod screen;

#[cfg(target_os = "linux")]
mod app;

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use ferrix_hardware_api::{HardwareState, WifiState};
use ferrix_session::{BootstrapOptions, bootstrap};
use ferrix_target_profile::Profile;
use ferrix_ui::draw::Type;
use ferrix_ui::evidence::{emit, json_escape};

/// Evidence label for every record this binary emits in a session.
const EVIDENCE: &str = "QEMU";

/// Initial quick-settings toggles derived from live hardware state.
fn ui_toggles(hw: &HardwareState) -> (bool, bool) {
    (hw.network.wifi != WifiState::Disabled, hw.bluetooth.enabled)
}

/// The checked-in profile's initial hardware state for unit tests.
#[cfg(test)]
pub(crate) fn test_state() -> HardwareState {
    bootstrap(BootstrapOptions::default().with_profile_path(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../targets/qemu-aarch64/profile.toml"
    )))
    .expect("test session")
    .services
    .snapshot()
}

/// The checked-in descriptors as launcher tiles for unit tests.
#[cfg(test)]
pub(crate) fn test_tiles() -> Vec<catalog::Tile> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps");
    let (apps, errors) = catalog::load_descriptor_dir(&dir).expect("apps dir");
    assert!(errors.is_empty(), "{errors:?}");
    catalog::tiles_from_catalog(&apps, &[])
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!(
                "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"basic-screen-error\",\"error\":\"{}\"}}",
                json_escape(&err)
            );
            ExitCode::FAILURE
        }
    }
}

struct Args {
    profile_path: PathBuf,
    core_socket: PathBuf,
    appd_socket: PathBuf,
    preview: Option<String>,
    page: String,
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let env_path = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
    let mut parsed = Args {
        profile_path: ferrix_session::default_profile_path(),
        core_socket: env_path("FERRIX_CORE_SOCKET")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(ferrix_protocol::DEFAULT_SOCKET)),
        appd_socket: env_path(ferrix_app_api::APPD_SOCKET_ENV)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(ferrix_app_protocol::DEFAULT_SOCKET)),
        preview: None,
        page: "launcher".to_owned(),
    };
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--preview" => {
                parsed.preview = Some(it.next().ok_or("--preview requires an output path")?);
            }
            "--page" => parsed.page = it.next().ok_or("--page requires launcher")?,
            "--core" => {
                parsed.core_socket =
                    PathBuf::from(it.next().ok_or("--core requires a socket path")?);
            }
            "--ferrix-appd" => {
                parsed.appd_socket =
                    PathBuf::from(it.next().ok_or("--ferrix-appd requires a socket path")?);
            }
            other if other.starts_with("--") => return Err(format!("unknown argument {other:?}")),
            _ => parsed.profile_path = PathBuf::from(arg),
        }
    }
    Ok(parsed)
}

fn run() -> Result<(), String> {
    let args = parse_args(std::env::args().skip(1))?;
    let minute = utc_minute_of_day();

    if let Some(path) = args.preview {
        if args.page != "launcher" {
            return Err(format!("unknown --page {}; use launcher", args.page));
        }
        // No daemons on the host: bootstrap the same session the core would
        // and read the catalog straight from the descriptors.
        let session = bootstrap(BootstrapOptions::from_env().with_profile_path(&args.profile_path))
            .map_err(|e| e.to_string())?;
        let profile = session.profile;
        let hw = session.services.snapshot();
        let ty = Type::load()?;
        let (wifi_on, bt_on) = ui_toggles(&hw);
        let (app_dir, tiles) = catalog::load_descriptor_tiles()?;
        let mut state = interaction::UiState::new(wifi_on, bt_on, tiles);
        state.brightness = hw.display.brightness_percent;
        let canvas = screen::compose(
            &hw,
            minute,
            &ty,
            profile.display.width,
            profile.display.height,
            &state,
        );
        std::fs::write(&path, canvas.ppm_bytes()).map_err(|e| e.to_string())?;
        emit(format!(
            "{{\"evidence\":\"mock\",\"event\":\"basic-screen-preview\",\"page\":\"{}\",\"path\":\"{}\",\"width\":{},\"height\":{},\"charging\":{},\"tiles\":{},\"app_dir\":\"{}\"}}",
            json_escape(&args.page),
            json_escape(&path),
            profile.display.width,
            profile.display.height,
            hw.power.charging,
            state.tiles.len(),
            json_escape(&app_dir.display().to_string())
        ));
        return Ok(());
    }

    // Guest: geometry from the profile, state from the core.
    let profile = Profile::load(&args.profile_path).map_err(|e| e.to_string())?;
    let core = ferrix_protocol::subscribe(&args.core_socket, Duration::from_secs(10))?;
    emit(format!(
        "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"core-connected\",\"socket\":\"{}\",\"target\":\"{}\",\"charging\":{}}}",
        json_escape(&args.core_socket.display().to_string()),
        json_escape(&core.target),
        core.state.power.charging
    ));

    #[cfg(target_os = "linux")]
    {
        app::run(&profile, core, &args.appd_socket, minute)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (profile, core, args.appd_socket);
        Err("the Wayland session is guest-only on this host; use --preview <out.ppm>".to_owned())
    }
}

fn utc_minute_of_day() -> u32 {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    ((secs % 86_400) / 60) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_parse_with_defaults_and_overrides() {
        let args = parse_args(Vec::<String>::new()).unwrap();
        assert!(args.profile_path.ends_with("profile.toml"));
        assert!(args.preview.is_none());
        assert_eq!(args.page, "launcher");

        let args = parse_args(
            [
                "custom.toml",
                "--preview",
                "out.ppm",
                "--core",
                "/tmp/core.sock",
                "--ferrix-appd",
                "/tmp/ferrix-appd.sock",
            ]
            .map(str::to_owned),
        )
        .unwrap();
        assert_eq!(args.profile_path, PathBuf::from("custom.toml"));
        assert_eq!(args.preview.as_deref(), Some("out.ppm"));
        assert_eq!(args.core_socket, PathBuf::from("/tmp/core.sock"));
        assert_eq!(args.appd_socket, PathBuf::from("/tmp/ferrix-appd.sock"));
        assert!(parse_args(["--bogus".to_owned()]).is_err());
        assert!(parse_args(["--preview".to_owned()]).is_err());
    }
}
