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
//! adapter. When `ferrix-telephonyd` / `ferrix-networkd` are reachable the
//! status bar and the Wi-Fi toggle use them (ADR-001/002) and fall back to
//! the coarse hardware-api summary and the core command otherwise.
//!
//! Colors come from `ferrix_ui::theme` (`colors.toml` contract), loaded once
//! here before the first frame in both modes.
//!
//! Usage:
//!   ferrix-basic-screen [profile.toml] [--core /run/ferrix/core.sock]
//!                       [--ferrix_appd /run/ferrix/ferrix_appd.sock]   # guest: connect to Wayland
//!   ferrix-basic-screen [profile.toml] --preview p.ppm [--page PAGE] [--size WxH]
//!                       [--sample-apks]                  # any host: render PPM
//!
//! `PAGE` is `launcher` (default), `apps` (the app list), `apps-end` (the
//! app list scrolled to its end), `quick-settings`, `notifications`,
//! `notifications-empty`, or the kind of a non-launchable catalog entry
//! (`camera`) for its reference page. `--sample-apks` appends the ten
//! installed-APK labels seen on the Pixel 9a as mock installed entries, so a
//! host preview shows a catalog with APKs (mock evidence, no Android); all
//! but Calculator carry a synthetic launcher bitmap standing in for
//! Waydroid's PNG, so the icon and the letter fallback both show. Launchable entries (Phone, Messages,
//! Contacts, Settings, Files, Terminal) are real apps started by
//! `ferrix-appd`, not pages of this surface. `--size` overrides the
//! profile's panel size, e.g. `830x1800` for the narrow QEMU display window.
//!
//! The profile defaults to the guest copy when present, else the repository
//! copy, so the manifest's native launch command works in the
//! guest and nested. The preview has no daemons, so it bootstraps a session
//! in-process and loads the catalog from the descriptor directory.

mod catalog;
mod interaction;
mod screen;
mod status;

#[cfg(target_os = "linux")]
mod app;

use std::path::PathBuf;
use std::process::ExitCode;
use std::str;
use std::time::Duration;

use ferrix_hardware_api::{HardwareState, WifiState};
use ferrix_session::{BootstrapOptions, bootstrap};
use ferrix_target_profile::Profile;
use ferrix_ui::draw::Type;
use ferrix_ui::evidence::{SessionEvidence, emit, json_escape};
use ferrix_ui::theme;

use crate::interaction::Page;

/// Runtime evidence label for every record this binary emits in a session,
/// resolved from `FERRIX_EVIDENCE` by the shared toolkit. Preview mode still
/// supplies its explicit `mock` records below.
pub(crate) const EVIDENCE: SessionEvidence = SessionEvidence;

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
    /// Preview-only panel size override (`WxH`).
    size: Option<(u32, u32)>,
    /// Preview-only: append [`catalog::sample_installed_tiles`].
    sample_apks: bool,
}

fn parse_size(text: &str) -> Result<(u32, u32), String> {
    let (w, h) = text
        .split_once('x')
        .ok_or_else(|| format!("--size expects WxH, got {text:?}"))?;
    match (w.parse::<u32>(), h.parse::<u32>()) {
        (Ok(w), Ok(h)) if w > 0 && h > 0 => Ok((w, h)),
        _ => Err(format!("--size expects WxH, got {text:?}")),
    }
}

/// Resolves a `--page` name against the loaded catalog. Reference pages are
/// named by the tile's kind so the CLI needs no index arithmetic.
fn preview_page(name: &str, state: &mut interaction::UiState) -> Result<Page, String> {
    match name {
        "launcher" => Ok(Page::Launcher),
        "apps" => Ok(Page::Apps),
        "apps-end" => {
            state.apps_scroll = i32::MAX;
            Ok(Page::Apps)
        }
        "quick-settings" => Ok(Page::QuickSettings),
        "notifications" => Ok(Page::Notifications),
        "notifications-empty" => {
            state.notification_visible = false;
            Ok(Page::Notifications)
        }
        kind => state
            .tiles
            .iter()
            .position(|tile| !tile.launchable && tile.kind() == kind)
            .map(Page::Reference)
            .ok_or_else(|| {
                let kinds: Vec<&str> = state
                    .tiles
                    .iter()
                    .filter(|tile| !tile.launchable)
                    .map(catalog::Tile::kind)
                    .collect();
                format!(
                    "unknown --page {name:?}; use launcher, apps, apps-end, quick-settings, notifications, notifications-empty, or a reference kind ({})",
                    kinds.join(", ")
                )
            }),
    }
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
        size: None,
        sample_apks: false,
    };
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--preview" => {
                parsed.preview = Some(it.next().ok_or("--preview requires an output path")?);
            }
            "--page" => parsed.page = it.next().ok_or("--page requires a page name")?,
            "--sample-apks" => parsed.sample_apks = true,
            "--size" => {
                parsed.size = Some(parse_size(&it.next().ok_or("--size requires WxH")?)?);
            }
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

    if let Some(path) = args.preview {
        let minute = ferrix_ui::clock::local_minute_of_day();
        // Host render: the theme is mock evidence like the frame it colors.
        theme::init("mock", None);
        // No daemons on the host: bootstrap the same session the core would
        // and read the catalog straight from the descriptors.
        let session = bootstrap(BootstrapOptions::from_env().with_profile_path(&args.profile_path))
            .map_err(|e| e.to_string())?;
        let profile = session.profile;
        let hw = session.services.snapshot();
        let ty = Type::load()?;
        let (wifi_on, bt_on) = ui_toggles(&hw);
        let (app_dir, mut tiles) = catalog::load_descriptor_tiles()?;
        if args.sample_apks {
            tiles.extend(catalog::sample_installed_tiles());
        }
        let mut state = interaction::UiState::new(wifi_on, bt_on, tiles);
        state.brightness = hw.display.brightness_percent;
        state.set_session(&session.target_id, session.capabilities.clone());
        // No daemons on the host: the status bar shows the hardware-api summary.
        state.tele = status::telephony_summary(None, &hw);
        state.net = status::network_summary(None, &hw);
        state.page = preview_page(&args.page, &mut state)?;
        let (width, height) = args
            .size
            .unwrap_or((profile.display.width, profile.display.height));
        if state.apps_scroll == i32::MAX {
            state.apps_scroll = interaction::apps_max_scroll((width, height), state.tiles.len());
        }
        let canvas = screen::compose(&hw, minute, &ty, width, height, &state);
        std::fs::write(&path, canvas.ppm_bytes()).map_err(|e| e.to_string())?;
        emit(format!(
            "{{\"evidence\":\"mock\",\"event\":\"basic-screen-preview\",\"page\":\"{}\",\"path\":\"{}\",\"width\":{},\"height\":{},\"charging\":{},\"tiles\":{},\"app_dir\":\"{}\"}}",
            json_escape(&args.page),
            json_escape(&path),
            width,
            height,
            hw.power.charging,
            state.tiles.len(),
            json_escape(&app_dir.display().to_string())
        ));
        return Ok(());
    }

    // Guest: geometry from the profile, state from the core. Theme first so
    // the first frame is already in the loaded palette.
    theme::init(&EVIDENCE, None);
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
        app::run(&profile, core, &args.appd_socket)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (profile, core, args.appd_socket);
        Err("the Wayland session is guest-only on this host; use --preview <out.ppm>".to_owned())
    }
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

        let args =
            parse_args(["--page", "quick-settings", "--size", "830x1800"].map(str::to_owned))
                .unwrap();
        assert_eq!(args.page, "quick-settings");
        assert_eq!(args.size, Some((830, 1800)));
        assert!(parse_args(["--size", "830"].map(str::to_owned)).is_err());
        assert!(parse_args(["--size", "0x10"].map(str::to_owned)).is_err());
    }

    #[test]
    fn preview_pages_resolve_by_name_and_reference_kind() {
        let mut state = interaction::UiState::new(true, true, test_tiles());
        assert_eq!(preview_page("launcher", &mut state), Ok(Page::Launcher));
        assert_eq!(preview_page("apps", &mut state), Ok(Page::Apps));
        assert_eq!(preview_page("apps-end", &mut state), Ok(Page::Apps));
        assert_eq!(
            state.apps_scroll,
            i32::MAX,
            "resolved against the size later"
        );
        assert!(
            parse_args(["--sample-apks".to_owned()])
                .unwrap()
                .sample_apks
        );
        assert_eq!(
            preview_page("quick-settings", &mut state),
            Ok(Page::QuickSettings)
        );
        assert_eq!(
            preview_page("notifications", &mut state),
            Ok(Page::Notifications)
        );
        assert!(state.notification_visible);
        assert_eq!(
            preview_page("notifications-empty", &mut state),
            Ok(Page::Notifications)
        );
        assert!(!state.notification_visible);
        let camera = state.tile_index(&ferrix_app_api::AppId::new("org.ferrix.camera").unwrap());
        assert_eq!(
            preview_page("camera", &mut state).ok(),
            camera.map(Page::Reference)
        );
        for launchable in ["terminal", "settings", "phone"] {
            let error = preview_page(launchable, &mut state).unwrap_err();
            assert!(
                error.contains("camera"),
                "launchable kinds are not pages: {error}"
            );
        }
        assert!(preview_page("bogus", &mut state).is_err());
    }
}
