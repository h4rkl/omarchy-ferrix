//! Guest-side Terminal client: the [`ClientHandler`] that owns the shell
//! session, forwards keyboard and touch input, and reports to `ferrix_appd`.
//!
//! The PTY reader thread wakes the UI through the runner's [`Wake`] handle;
//! `wake` then pumps emulator events and redraws. The Back control reports
//! `YieldForeground` to `ferrix_appd` over an application-role connection when
//! `FERRIX_APPD_SOCKET` is set; otherwise it is recorded and skipped, and the
//! shell keeps running either way.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use ferrix_app_protocol::{Client, ClientRole, Report};
use ferrix_ui::client::{ClientConfig, ClientHandler, Wake};
use ferrix_ui::clock::MinuteClock;
use ferrix_ui::draw::{Canvas, Type};
use ferrix_ui::evidence::{emit, json_escape};
use ferrix_ui::keymap::{self, Modifiers};

use crate::EVIDENCE;
use crate::interaction::{Input, TermAction, UiState};
use crate::screen;
use crate::terminal::{Geometry, Session, Snapshot};

pub const APP_ID: &str = "org.ferrix.terminal";
const TITLE: &str = "Terminal";
const APPD_TIMEOUT: Duration = Duration::from_secs(1);

struct Terminal {
    ty: Type,
    clock: MinuteClock,
    shell: String,
    ui: UiState,
    input: Input,
    size: (u32, u32),
    session: Option<Session>,
    session_error: Option<String>,
    wake: Wake,
    terminal_ready_reported: bool,
    terminal_exit_reported: bool,
    keyboard_reported: bool,
    appd_socket: Option<PathBuf>,
    ferrix_appd: Option<Client>,
    appd_unavailable_reported: bool,
}

pub fn run(initial_size: (u32, u32), shell: String) -> Result<(), String> {
    let config = ClientConfig {
        app_id: APP_ID.to_owned(),
        title: TITLE.to_owned(),
        initial_size,
        evidence: EVIDENCE.to_string(),
        first_frame_event: "terminal-frame".to_owned(),
        first_frame_extra: vec![("shell".to_owned(), shell.clone())],
    };
    ferrix_ui::client::run(config, move |wake| {
        // Redraw the status-bar clock at each local minute boundary.
        let tick = Arc::clone(&wake);
        ferrix_ui::clock::spawn_minute_ticker(move || tick());
        Ok(Terminal {
            ty: Type::load()?,
            clock: MinuteClock::now(),
            shell,
            ui: UiState::new(),
            input: Input::default(),
            size: initial_size,
            session: None,
            session_error: None,
            wake,
            terminal_ready_reported: false,
            terminal_exit_reported: false,
            keyboard_reported: false,
            appd_socket: std::env::var_os(ferrix_app_api::APPD_SOCKET_ENV)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from),
            ferrix_appd: None,
            appd_unavailable_reported: false,
        })
    })
}

impl Terminal {
    fn geometry(&self) -> Geometry {
        let m = screen::terminal_metrics(&self.ty, self.size.0, self.size.1);
        let clamp = |v: usize| u16::try_from(v).unwrap_or(u16::MAX);
        let clamp_px = |v: i32| u16::try_from(v.max(1)).unwrap_or(u16::MAX);
        Geometry {
            cols: clamp(m.cols),
            rows: clamp(m.rows),
            cell_w: clamp_px(m.cell_w),
            cell_h: clamp_px(m.cell_h),
        }
    }

    fn open_session(&mut self) {
        let geometry = self.geometry();
        self.terminal_ready_reported = false;
        self.terminal_exit_reported = false;
        match Session::spawn(&self.shell, &[], geometry, Arc::clone(&self.wake)) {
            Ok(session) => {
                emit(format!(
                    "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"terminal-spawned\",\"shell\":\"{}\",\"cols\":{},\"rows\":{}}}",
                    json_escape(session.program()),
                    geometry.cols,
                    geometry.rows
                ));
                self.session = Some(session);
                self.session_error = None;
            }
            Err(error) => {
                emit(format!(
                    "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"terminal-error\",\"shell\":\"{}\",\"error\":\"{}\"}}",
                    json_escape(&self.shell),
                    json_escape(&error)
                ));
                self.session_error = Some(error);
            }
        }
    }

    /// Starts the shell on the first frame, then forwards queued actions.
    /// Enter on an exited shell restarts it.
    fn after_input(&mut self) {
        if self.session.is_none() && self.session_error.is_none() {
            self.open_session();
        }
        for action in std::mem::take(&mut self.ui.pending) {
            match action {
                TermAction::Input(bytes) => {
                    let live = self
                        .session
                        .as_ref()
                        .is_some_and(|session| session.exited().is_none());
                    if live {
                        if let Some(session) = &self.session {
                            session.write(bytes);
                        }
                    } else if bytes == b"\r" {
                        self.session = None;
                        self.session_error = None;
                        self.open_session();
                    }
                }
                TermAction::Scroll(lines) => {
                    if let Some(session) = &self.session {
                        session.scroll(lines);
                    }
                }
            }
        }
        while self.ui.pending_back > 0 {
            self.ui.pending_back -= 1;
            self.yield_foreground();
        }
    }

    fn appd_client(&mut self) -> Option<&mut Client> {
        if self.ferrix_appd.is_none() {
            let Some(socket) = self.appd_socket.clone() else {
                self.report_appd_unavailable("FERRIX_APPD_SOCKET is not set");
                return None;
            };
            match Client::connect(&socket, ClientRole::Application, APPD_TIMEOUT) {
                Ok(client) => self.ferrix_appd = Some(client),
                Err(error) => {
                    self.report_appd_unavailable(&error);
                    return None;
                }
            }
        }
        self.ferrix_appd.as_mut()
    }

    fn report_appd_unavailable(&mut self, reason: &str) {
        if self.appd_unavailable_reported {
            return;
        }
        self.appd_unavailable_reported = true;
        emit(format!(
            "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"ferrix-appd-unavailable\",\"socket\":{},\"reason\":\"{}\"}}",
            ferrix_ui::evidence::json_opt_str(self.appd_socket.as_deref().and_then(|p| p.to_str())),
            json_escape(reason)
        ));
    }

    /// Back: ask ferrix_appd to bring the home surface forward. The shell keeps
    /// running; a lost connection is dropped so the next Back reconnects.
    fn yield_foreground(&mut self) {
        let result = self
            .appd_client()
            .map(|client| client.report(&Report::YieldForeground));
        let status = match result {
            Some(Ok(())) => "yielded",
            Some(Err(error)) => {
                self.ferrix_appd = None;
                emit(format!(
                    "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"ferrix-appd-report-failed\",\"error\":\"{}\"}}",
                    json_escape(&error)
                ));
                "failed"
            }
            None => "ferrix-appd-unavailable",
        };
        emit(format!(
            "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"terminal-back\",\"status\":\"{status}\"}}"
        ));
    }

    fn snapshot(&self) -> Option<Snapshot> {
        match (&self.session, &self.session_error) {
            (Some(session), _) => Some(session.snapshot()),
            (None, Some(error)) => Some(Snapshot::failed(error.clone())),
            (None, None) => None,
        }
    }
}

impl ClientHandler for Terminal {
    fn draw(&mut self, size: (u32, u32)) -> Canvas {
        self.size = size;
        if self.session.is_none() && self.session_error.is_none() {
            self.open_session();
        }
        let snapshot = self.snapshot();
        if let Some(snapshot) = &snapshot {
            self.ui.term_app_cursor = snapshot.app_cursor;
        }
        screen::compose(
            &self.ty,
            size.0,
            size.1,
            self.clock.minute(),
            &self.ui,
            snapshot.as_ref(),
        )
    }

    fn configure(&mut self, size: (u32, u32)) {
        self.size = size;
        let geometry = self.geometry();
        if let Some(session) = self.session.as_mut() {
            session.resize(geometry);
        }
    }

    fn pointer_down(&mut self, position: (f64, f64), size: (u32, u32)) -> bool {
        let redraw = self.input.down(0, position, &mut self.ui, size);
        if redraw {
            self.after_input();
        }
        redraw
    }

    fn pointer_motion(&mut self, position: (f64, f64), size: (u32, u32)) -> bool {
        let redraw = self.input.motion(0, position, &mut self.ui, size);
        if redraw {
            self.after_input();
        }
        redraw
    }

    fn pointer_up(&mut self, position: (f64, f64), size: (u32, u32)) -> bool {
        let mut redraw = self.input.motion(0, position, &mut self.ui, size);
        redraw |= self.input.up(0, &mut self.ui, size);
        if redraw {
            self.after_input();
        }
        redraw
    }

    fn pointer_leave(&mut self) -> bool {
        self.input.cancel(&mut self.ui)
    }

    fn touch_down(&mut self, id: i32, position: (f64, f64), size: (u32, u32)) -> bool {
        let redraw = self.input.down(id, position, &mut self.ui, size);
        if redraw {
            self.after_input();
        }
        redraw
    }

    fn touch_motion(&mut self, id: i32, position: (f64, f64), size: (u32, u32)) -> bool {
        let redraw = self.input.motion(id, position, &mut self.ui, size);
        if redraw {
            self.after_input();
        }
        redraw
    }

    fn touch_up(&mut self, id: i32, size: (u32, u32)) -> bool {
        let redraw = self.input.up(id, &mut self.ui, size);
        if redraw {
            self.after_input();
        }
        redraw
    }

    fn touch_cancel(&mut self) -> bool {
        self.input.cancel(&mut self.ui)
    }

    fn key(&mut self, code: u32, mods: Modifiers) -> bool {
        let Some(bytes) = keymap::bytes(code, mods, self.ui.term_app_cursor) else {
            return false;
        };
        if !self.keyboard_reported {
            self.keyboard_reported = true;
            emit(format!(
                "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"keyboard-input\",\"code\":{code},\"bytes\":{}}}",
                bytes.len()
            ));
        }
        self.ui.pending.push(TermAction::Input(bytes));
        self.after_input();
        true
    }

    /// The PTY reader produced output or the shell exited.
    fn wake(&mut self) -> bool {
        let clock = self.clock.refresh();
        let Some(session) = self.session.as_mut() else {
            return clock;
        };
        session.pump();
        let exited = session.exited().map(str::to_owned);
        let first_line = (!self.terminal_ready_reported)
            .then(|| session.snapshot().first_text_line())
            .flatten();
        if let Some(reason) = exited
            && !self.terminal_exit_reported
        {
            self.terminal_exit_reported = true;
            emit(format!(
                "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"terminal-exited\",\"reason\":\"{}\"}}",
                json_escape(&reason)
            ));
        }
        if let Some(line) = first_line {
            self.terminal_ready_reported = true;
            emit(format!(
                "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"terminal-ready\",\"shell\":\"{}\",\"line\":\"{}\"}}",
                json_escape(&self.shell),
                json_escape(&line)
            ));
        }
        true
    }

    fn page_name(&self) -> String {
        "Terminal".to_owned()
    }
}
