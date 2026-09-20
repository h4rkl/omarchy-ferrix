//! Ferrum Terminal: an independent Wayland client running a real shell.
//!
//! One supervised process per application: this binary is launched by `appd`
//! (xdg app_id `org.ferrix.terminal`), renders its own fullscreen surface
//! through `ferrum_ui::client`, and drives `/bin/sh` (or `$SHELL`) on a PTY
//! through `alacritty_terminal`. Its Back control asks `appd` to return to the
//! home surface; the shell keeps running in the background. The bottom edge
//! belongs to the compositor's home gesture.
//!
//! Usage:
//!   ferrum-terminal                       # guest/nested: connect to Wayland
//!   ferrum-terminal --preview out.ppm [--size WxH]
//!                                         # any host: render a canned transcript
//!
//! Environment (set by `appd` for launched apps): `FERRIX_APP_ID`,
//! `FERRIX_APP_INSTANCE`, `FERRIX_APPD_SOCKET`, plus `SHELL`.

mod interaction;
mod screen;
mod terminal;

#[cfg(target_os = "linux")]
mod app;

use std::process::ExitCode;

use ferrum_ui::draw::Type;
use ferrum_ui::evidence::{emit, json_escape, json_opt_str};

/// Evidence label for every record this binary emits.
const EVIDENCE: &str = "QEMU";
/// Default preview geometry: the checked-in portrait mock panel.
const PREVIEW_SIZE: (u32, u32) = (1080, 2340);

/// Canned shell transcript for the host preview; no PTY is involved.
const PREVIEW_TRANSCRIPT: &[u8] = b"root@ferrix:~# uname -a\r\n\
Linux ferrix 6.12.0-virt #1 SMP aarch64 Linux\r\n\
root@ferrix:~# ls --color=always /\r\n\
\x1b[1;34mbin\x1b[0m   \x1b[1;34mdev\x1b[0m   \x1b[1;34metc\x1b[0m   \x1b[1;34mproc\x1b[0m  \x1b[1;34mroot\x1b[0m  \x1b[1;34mrun\x1b[0m   \x1b[1;34msys\x1b[0m   \x1b[1;34musr\x1b[0m\r\n\
root@ferrix:~# printf '\\033[32mok\\033[0m\\n'\r\n\
\x1b[32mok\x1b[0m\r\n\
root@ferrix:~# ";

/// Launch context handed down by `appd`, or nulls when started by hand.
struct AppContext {
    app_id: Option<String>,
    instance: Option<u32>,
}

impl AppContext {
    fn from_env() -> Self {
        Self {
            app_id: std::env::var(app_api::APP_ID_ENV)
                .ok()
                .filter(|v| !v.is_empty()),
            instance: std::env::var(app_api::APP_INSTANCE_ENV)
                .ok()
                .and_then(|v| v.parse().ok()),
        }
    }

    fn record(&self) -> String {
        format!(
            "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"app-context\",\"app_id\":{},\"instance\":{}}}",
            json_opt_str(self.app_id.as_deref()),
            self.instance
                .map_or_else(|| "null".to_owned(), |n| n.to_string())
        )
    }
}

/// The shell to run: `$SHELL` when set, otherwise the guest's `/bin/sh`.
fn shell_program() -> String {
    std::env::var("SHELL")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "/bin/sh".to_owned())
}

fn parse_size(text: &str) -> Result<(u32, u32), String> {
    let (w, h) = text
        .split_once('x')
        .ok_or_else(|| format!("--size expects WxH, got {text:?}"))?;
    let w: u32 = w.parse().map_err(|_| format!("bad width in {text:?}"))?;
    let h: u32 = h.parse().map_err(|_| format!("bad height in {text:?}"))?;
    if w == 0 || h == 0 {
        return Err("--size must be non-zero".to_owned());
    }
    Ok((w, h))
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!(
                "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"terminal-error\",\"error\":\"{}\"}}",
                json_escape(&err)
            );
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut preview = None;
    let mut size = PREVIEW_SIZE;
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--preview" => preview = Some(it.next().ok_or("--preview requires an output path")?),
            "--size" => size = parse_size(&it.next().ok_or("--size requires WxH")?)?,
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    let context = AppContext::from_env();
    emit(context.record());

    if let Some(path) = preview {
        let ty = Type::load()?;
        let state = interaction::UiState::new();
        let m = screen::terminal_metrics(&ty, size.0, size.1);
        let snapshot = terminal::Snapshot::from_bytes(m.cols, m.rows, PREVIEW_TRANSCRIPT);
        let canvas = screen::compose(
            &ty,
            size.0,
            size.1,
            utc_minute_of_day(),
            &state,
            Some(&snapshot),
        );
        std::fs::write(&path, canvas.ppm_bytes()).map_err(|e| e.to_string())?;
        emit(format!(
            "{{\"evidence\":\"mock\",\"event\":\"terminal-preview\",\"path\":\"{}\",\"width\":{},\"height\":{},\"cols\":{},\"rows\":{}}}",
            json_escape(&path),
            size.0,
            size.1,
            m.cols,
            m.rows
        ));
        return Ok(());
    }

    #[cfg(target_os = "linux")]
    {
        app::run(size, utc_minute_of_day(), shell_program())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = shell_program();
        Err("the Wayland session is Linux-only on this host; use --preview <out.ppm>".to_owned())
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
    fn size_argument_parses_and_rejects_garbage() {
        assert_eq!(parse_size("830x1798"), Ok((830, 1798)));
        assert!(parse_size("830").is_err());
        assert!(parse_size("0x10").is_err());
        assert!(parse_size("ax10").is_err());
    }

    #[test]
    fn app_context_record_uses_nulls_when_unset() {
        let context = AppContext {
            app_id: None,
            instance: None,
        };
        assert_eq!(
            context.record(),
            r#"{"evidence":"QEMU","event":"app-context","app_id":null,"instance":null}"#
        );
        let context = AppContext {
            app_id: Some("org.ferrix.terminal".to_owned()),
            instance: Some(3),
        };
        assert!(
            context
                .record()
                .contains(r#""app_id":"org.ferrix.terminal","instance":3"#)
        );
    }

    #[test]
    fn preview_transcript_renders_through_the_emulator() {
        let ty = Type::load().expect("fonts");
        let m = screen::terminal_metrics(&ty, PREVIEW_SIZE.0, PREVIEW_SIZE.1);
        let snap = terminal::Snapshot::from_bytes(m.cols, m.rows, PREVIEW_TRANSCRIPT);
        assert_eq!(
            snap.first_text_line().as_deref(),
            Some("root@ferrix:~# uname -a")
        );
        assert!(snap.cursor.is_some());
    }
}
