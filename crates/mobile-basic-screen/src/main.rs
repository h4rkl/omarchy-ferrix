//! Omarchy-styled QEMU reference basic screen.
//!
//! A small software-rendered Wayland client that draws the first mobile
//! surface from the versioned mock profile. It is QEMU/mock evidence for the
//! graphics path, not the product shell and not a physical-device claim.
//!
//! Hardware state (battery, network, modem) comes from the persistent
//! `mobile-core` over its state socket; the profile supplies only the panel
//! geometry and name. Product code depends on `hardware-api`, never on an
//! adapter.
//!
//! Usage:
//!   mobile-basic-screen [profile.toml] [--core /run/omarchy/core.sock]
//!                                                  # guest: connect to Wayland
//!   mobile-basic-screen [profile.toml] --preview p.ppm  # any host: render PPM
//!   mobile-basic-screen [profile.toml] --preview p.ppm --page terminal
//!                                                  # preview the Terminal app
//!                                                  # with canned shell output
//!
//! The preview has no daemon, so it bootstraps a session in-process and
//! renders that session's initial (mock) state.

mod draw;
mod interaction;
mod keymap;
mod osk;
mod screen;
mod terminal;
mod theme;

#[cfg(target_os = "linux")]
mod app;

use std::process::ExitCode;
use std::time::Duration;

use hardware_api::{HardwareState, WifiState};
use mobile_session::{BootstrapOptions, bootstrap};
use target_profile::Profile;

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

/// Canned shell transcript for the host preview; no PTY is involved.
const PREVIEW_TRANSCRIPT: &[u8] = b"root@omarchy:~# uname -a\r\n\
Linux omarchy 6.12.0-virt #1 SMP aarch64 Linux\r\n\
root@omarchy:~# ls --color=always /\r\n\
\x1b[1;34mbin\x1b[0m   \x1b[1;34mdev\x1b[0m   \x1b[1;34metc\x1b[0m   \x1b[1;34mproc\x1b[0m  \x1b[1;34mroot\x1b[0m  \x1b[1;34mrun\x1b[0m   \x1b[1;34msys\x1b[0m   \x1b[1;34musr\x1b[0m\r\n\
root@omarchy:~# printf '\\033[32mok\\033[0m\\n'\r\n\
\x1b[32mok\x1b[0m\r\n\
root@omarchy:~# ";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!(
                "{{\"evidence\":\"QEMU\",\"event\":\"basic-screen-error\",\"error\":\"{}\"}}",
                err.replace('"', "'")
            );
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut profile_path = mobile_session::GUEST_PROFILE_PATH.to_owned();
    let mut core_socket = std::env::var("OMARCHY_CORE_SOCKET")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| mobile_protocol::DEFAULT_SOCKET.to_owned());
    let mut preview = None;
    let mut page = "launcher".to_owned();
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        if arg == "--preview" {
            preview = Some(it.next().ok_or("--preview requires an output path")?);
        } else if arg == "--page" {
            page = it.next().ok_or("--page requires launcher|terminal")?;
        } else if arg == "--core" {
            core_socket = it.next().ok_or("--core requires a socket path")?;
        } else {
            profile_path = arg;
        }
    }
    let minute = utc_minute_of_day();

    if let Some(path) = preview {
        // No daemon on the host: bootstrap the same session the core would.
        let session = bootstrap(BootstrapOptions::from_env().with_profile_path(&profile_path))
            .map_err(|e| e.to_string())?;
        let profile = session.profile;
        let hw = session.services.snapshot();
        let ty = draw::Type::load()?;
        let (wifi_on, bt_on) = ui_toggles(&hw);
        let mut state = interaction::UiState::new(wifi_on, bt_on);
        let snapshot = match page.as_str() {
            "launcher" => None,
            "terminal" => {
                state.page = interaction::Page::App(interaction::AppId::Terminal);
                let m =
                    screen::terminal_metrics(&ty, profile.display.width, profile.display.height);
                Some(terminal::Snapshot::from_bytes(
                    m.cols,
                    m.rows,
                    PREVIEW_TRANSCRIPT,
                ))
            }
            other => return Err(format!("unknown --page {other}; use launcher|terminal")),
        };
        let canvas = screen::compose(
            &hw,
            minute,
            &ty,
            profile.display.width,
            profile.display.height,
            &state,
            snapshot.as_ref(),
        );
        std::fs::write(&path, canvas.ppm_bytes()).map_err(|e| e.to_string())?;
        println!(
            "{{\"evidence\":\"mock\",\"event\":\"basic-screen-preview\",\"page\":\"{page}\",\"path\":\"{path}\",\"width\":{},\"height\":{},\"charging\":{}}}",
            profile.display.width, profile.display.height, hw.power.charging
        );
        return Ok(());
    }

    // Guest: geometry from the profile, state from the core.
    let profile = Profile::load(&profile_path).map_err(|e| e.to_string())?;
    let core =
        mobile_protocol::subscribe(std::path::Path::new(&core_socket), Duration::from_secs(10))?;
    println!(
        "{{\"evidence\":\"QEMU\",\"event\":\"core-connected\",\"socket\":\"{core_socket}\",\"target\":\"{}\",\"charging\":{}}}",
        core.target, core.state.power.charging
    );

    #[cfg(target_os = "linux")]
    {
        app::run(&profile, core, minute)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (profile, core);
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
