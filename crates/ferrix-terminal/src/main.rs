//! Omarchy Ferrix Terminal: an independent Wayland client running a real shell.
//!
//! One supervised process per application: this binary is launched by `ferrix_appd`
//! (xdg app_id `org.ferrix.terminal`), renders its own fullscreen surface
//! through `ferrix_ui::client`, and drives `/bin/sh` (or `$SHELL`) on a PTY
//! through `alacritty_terminal`. Its Back control asks `ferrix_appd` to return to the
//! home surface; the shell keeps running in the background. The bottom edge
//! belongs to the compositor's home gesture.
//!
//! Usage:
//!   ferrix-terminal                       # guest/nested: connect to Wayland
//!   ferrix-terminal --preview out.ppm [--size WxH]
//!                                         # any host: render a canned transcript
//!
//! Environment (set by `ferrix_appd` for launched apps): `FERRIX_APP_ID`,
//! `FERRIX_APP_INSTANCE`, `FERRIX_APPD_SOCKET`, plus `SHELL`.

mod interaction;
mod screen;
mod terminal;

#[cfg(target_os = "linux")]
mod app;

use std::process::ExitCode;

use ferrix_ui::draw::Type;
use ferrix_ui::evidence::{SessionEvidence, emit, json_escape, json_opt_str};
use ferrix_ui::theme;

/// Evidence label for every record this binary emits.
const EVIDENCE: SessionEvidence = SessionEvidence;
/// Default preview geometry: the checked-in portrait mock panel.
const PREVIEW_SIZE: (u32, u32) = (1080, 2340);

/// Canned shell transcript for the host preview; no PTY is involved and the
/// first line says so. The `uname` line has the shape of a QEMU `virt` guest
/// with a placeholder release (the real guest kernel differs), the `ls` line exercises bold ANSI blue, and two swatch lines show
/// all sixteen ANSI colors (normal beside bright) as text and as backgrounds
/// so a preview doubles as a palette legibility check on `bg_dark`.
const PREVIEW_TRANSCRIPT: &[u8] = b"root@ferrix:~# # canned transcript \xc2\xb7 mock \xc2\xb7 no pty\r\n\
root@ferrix:~# uname -a\r\n\
Linux ferrix canned-virt #1 SMP aarch64 Linux\r\n\
root@ferrix:~# echo $TERM\r\n\
xterm-256color\r\n\
root@ferrix:~# ls --color=always /\r\n\
\x1b[1;34mbin\x1b[0m   \x1b[1;34mdev\x1b[0m   \x1b[1;34metc\x1b[0m   \x1b[1;34mproc\x1b[0m  \x1b[1;34mroot\x1b[0m  \x1b[1;34mrun\x1b[0m   \x1b[1;34msys\x1b[0m   \x1b[1;34musr\x1b[0m\r\n\
root@ferrix:~# # ansi fg 0-7 with 8-15, then bg\r\n\
\x1b[30m0 \x1b[90m0 \x1b[0m\x1b[31m1 \x1b[91m1 \x1b[0m\x1b[32m2 \x1b[92m2 \x1b[0m\x1b[33m3 \x1b[93m3 \x1b[0m\x1b[34m4 \x1b[94m4 \x1b[0m\x1b[35m5 \x1b[95m5 \x1b[0m\x1b[36m6 \x1b[96m6 \x1b[0m\x1b[37m7 \x1b[97m7 \x1b[0m\r\n\
\x1b[40m  \x1b[100m  \x1b[0m\x1b[41m  \x1b[101m  \x1b[0m\x1b[42m  \x1b[102m  \x1b[0m\x1b[43m  \x1b[103m  \x1b[0m\x1b[44m  \x1b[104m  \x1b[0m\x1b[45m  \x1b[105m  \x1b[0m\x1b[46m  \x1b[106m  \x1b[0m\x1b[47m  \x1b[107m  \x1b[0m\r\n\
root@ferrix:~# printf '\\033[32mok\\033[0m\\n'\r\n\
\x1b[32mok\x1b[0m\r\n\
root@ferrix:~# ";

/// Launch context handed down by `ferrix_appd`, or nulls when started by hand.
struct AppContext {
    app_id: Option<String>,
    instance: Option<u32>,
}

impl AppContext {
    fn from_env() -> Self {
        Self {
            app_id: std::env::var(ferrix_app_api::APP_ID_ENV)
                .ok()
                .filter(|v| !v.is_empty()),
            instance: std::env::var(ferrix_app_api::APP_INSTANCE_ENV)
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
        // Host render: the theme is mock evidence like the frame it colors.
        theme::init("mock", None);
        let ty = Type::load()?;
        let state = interaction::UiState::new();
        let m = screen::terminal_metrics(&ty, size.0, size.1);
        let snapshot = terminal::Snapshot::from_bytes(m.cols, m.rows, PREVIEW_TRANSCRIPT);
        let canvas = screen::compose(
            &ty,
            size.0,
            size.1,
            ferrix_ui::clock::local_minute_of_day(),
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

    // Guest: load `colors.toml` (or the built-in Ferrix palette) once, before
    // any frame is composed.
    theme::init(&EVIDENCE, None);

    #[cfg(target_os = "linux")]
    {
        app::run(size, shell_program())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = shell_program();
        Err("the Wayland session is Linux-only on this host; use --preview <out.ppm>".to_owned())
    }
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
            Some("root@ferrix:~# # canned transcript \u{b7} mock \u{b7} no pty")
        );
        assert_eq!(
            snap.line_text(2),
            "Linux ferrix canned-virt #1 SMP aarch64 Linux"
        );
        assert!(snap.cursor.is_some());
        // The swatch lines put every ANSI color on the grid.
        let t = theme::get();
        let swatch_row = (0..m.rows)
            .find(|&row| snap.line_text(row).starts_with("0 0 1 1"))
            .expect("foreground swatch line");
        let fgs: Vec<u32> = (0..32)
            .step_by(2)
            .map(|col| snap.cell(col, swatch_row).unwrap().fg)
            .collect();
        let expected: Vec<u32> = (0..8).flat_map(|i| [t.ansi[i], t.ansi[i + 8]]).collect();
        assert_eq!(fgs, expected);
    }
}
