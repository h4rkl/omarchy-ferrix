//! Omarchy-styled QEMU reference basic screen.
//!
//! A small software-rendered Wayland client that draws the first mobile
//! surface from the versioned mock profile. It is QEMU/mock evidence for the
//! graphics path, not the product shell and not a physical-device claim.
//!
//! Usage:
//!   mobile-basic-screen [profile.toml]            # guest: connect to Wayland
//!   mobile-basic-screen [profile.toml] --preview p.ppm  # any host: render PPM
//!   mobile-basic-screen [profile.toml] --preview p.ppm --page terminal
//!                                                  # preview the Terminal app
//!                                                  # with canned shell output

mod draw;
mod interaction;
mod keymap;
mod osk;
mod profile;
mod screen;
mod terminal;
mod theme;

#[cfg(target_os = "linux")]
mod app;

use std::process::ExitCode;

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
    let mut profile_path = "/etc/omarchy/profile.toml".to_owned();
    let mut preview = None;
    let mut page = "launcher".to_owned();
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        if arg == "--preview" {
            preview = Some(it.next().ok_or("--preview requires an output path")?);
        } else if arg == "--page" {
            page = it.next().ok_or("--page requires launcher|terminal")?;
        } else {
            profile_path = arg;
        }
    }
    let text = std::fs::read_to_string(&profile_path)
        .map_err(|e| format!("cannot read profile {profile_path}: {e}"))?;
    let profile = profile::Profile::parse(&text)?;
    let minute = utc_minute_of_day();

    if let Some(path) = preview {
        let ty = draw::Type::load()?;
        let mut state =
            interaction::UiState::new(profile.wifi == "connected", profile.bluetooth_present);
        let snapshot = match page.as_str() {
            "launcher" => None,
            "terminal" => {
                state.page = interaction::Page::App(interaction::AppId::Terminal);
                let m =
                    screen::terminal_metrics(&ty, profile.display_width, profile.display_height);
                Some(terminal::Snapshot::from_bytes(
                    m.cols,
                    m.rows,
                    PREVIEW_TRANSCRIPT,
                ))
            }
            other => return Err(format!("unknown --page {other}; use launcher|terminal")),
        };
        let canvas = screen::compose(
            &profile,
            minute,
            &ty,
            profile.display_width,
            profile.display_height,
            &state,
            snapshot.as_ref(),
        );
        std::fs::write(&path, canvas.ppm_bytes()).map_err(|e| e.to_string())?;
        println!(
            "{{\"evidence\":\"mock\",\"event\":\"basic-screen-preview\",\"page\":\"{page}\",\"path\":\"{path}\",\"width\":{},\"height\":{}}}",
            profile.display_width, profile.display_height
        );
        return Ok(());
    }

    #[cfg(target_os = "linux")]
    {
        app::run(&profile, minute)
    }
    #[cfg(not(target_os = "linux"))]
    {
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
