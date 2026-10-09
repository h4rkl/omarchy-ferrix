//! Guest-side Files client: owns the model over the real `$HOME`, re-reads
//! the open folder at each minute tick (so a Contacts export appears without
//! a manual refresh), emits counts-only evidence, and reports Back from the
//! home folder to `ferrix-appd`.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use ferrix_app_protocol::{Client as AppdClient, ClientRole, Report};
use ferrix_ui::client::{ClientConfig, ClientHandler, Wake};
use ferrix_ui::clock::MinuteClock;
use ferrix_ui::draw::{Canvas, Type};
use ferrix_ui::evidence::{emit, json_escape, json_opt_str};
use ferrix_ui::keymap::{self, Modifiers};
use jiff::tz::TimeZone;

use crate::interaction::{Input, key_press};
use crate::model::{Action, Model, Page};
use crate::store::Home;
use crate::{APP_ID, EVIDENCE, screen};

const TIMEOUT: Duration = Duration::from_secs(2);

struct Files {
    ty: Type,
    clock: MinuteClock,
    model: Model,
    input: Input,
    appd_socket: Option<PathBuf>,
    appd: Option<AppdClient>,
    appd_unavailable_reported: bool,
    last_ui_state: (String, String),
}

pub fn run(initial_size: (u32, u32)) -> Result<(), String> {
    let config = ClientConfig {
        app_id: APP_ID.to_owned(),
        title: "Files".to_owned(),
        initial_size,
        evidence: EVIDENCE.to_string(),
        first_frame_event: "files-frame".to_owned(),
        first_frame_extra: Vec::new(),
    };
    ferrix_ui::client::run(config, move |wake: Wake| {
        let tick = Arc::clone(&wake);
        ferrix_ui::clock::spawn_minute_ticker(move || tick());
        let home = Home::from_env(|key| std::env::var(key).ok());
        let model = Model::new(home, TimeZone::system());
        let mut files = Files {
            ty: Type::load()?,
            clock: MinuteClock::now(),
            model,
            input: Input::default(),
            appd_socket: std::env::var_os(ferrix_app_api::APPD_SOCKET_ENV)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from),
            appd: None,
            appd_unavailable_reported: false,
            last_ui_state: (String::new(), String::new()),
        };
        files.report_ui_state();
        Ok(files)
    })
}

impl Files {
    fn after_input(&mut self) {
        for outcome in std::mem::take(&mut self.model.outcomes) {
            emit(format!(
                "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"files-action\",\"app\":\"{APP_ID}\",\"action\":\"{}\",\"status\":\"{}\",\"error\":{}}}",
                outcome.action,
                if outcome.error.is_some() {
                    "error"
                } else {
                    "ok"
                },
                json_opt_str(outcome.error)
            ));
        }
        for action in std::mem::take(&mut self.model.pending) {
            match action {
                Action::Back => self.yield_foreground(),
            }
        }
        self.report_ui_state();
    }

    fn report_ui_state(&mut self) {
        let current = (self.model.page.name().to_owned(), self.model.state_name());
        if current != self.last_ui_state {
            self.last_ui_state = current.clone();
            emit(format!(
                "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"ui-state\",\"app\":\"{APP_ID}\",\"page\":\"{}\",\"state\":\"{}\"}}",
                current.0,
                json_escape(&current.1)
            ));
        }
    }

    fn appd(&mut self) -> Option<&mut AppdClient> {
        if self.appd.is_none() {
            let Some(socket) = self.appd_socket.clone() else {
                self.report_appd_unavailable("FERRIX_APPD_SOCKET is not set");
                return None;
            };
            match AppdClient::connect(&socket, ClientRole::Application, TIMEOUT) {
                Ok(client) => self.appd = Some(client),
                Err(error) => {
                    self.report_appd_unavailable(&error);
                    return None;
                }
            }
        }
        self.appd.as_mut()
    }

    fn report_appd_unavailable(&mut self, reason: &str) {
        if self.appd_unavailable_reported {
            return;
        }
        self.appd_unavailable_reported = true;
        emit(format!(
            "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"ferrix-appd-unavailable\",\"app\":\"{APP_ID}\",\"reason\":\"{}\"}}",
            json_escape(reason)
        ));
    }

    fn yield_foreground(&mut self) {
        let result = self
            .appd()
            .map(|client| client.report(&Report::YieldForeground));
        let status = match result {
            Some(Ok(())) => "yielded",
            Some(Err(_)) => {
                self.appd = None;
                "failed"
            }
            None => "ferrix-appd-unavailable",
        };
        emit(format!(
            "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"files-back\",\"status\":\"{status}\"}}"
        ));
    }
}

impl ClientHandler for Files {
    fn draw(&mut self, size: (u32, u32)) -> Canvas {
        screen::compose(
            &self.ty,
            size,
            self.clock.minute(),
            &self.model,
            &self.input,
        )
    }

    fn pointer_down(&mut self, position: (f64, f64), size: (u32, u32)) -> bool {
        self.input.down(0, position, &self.model, size)
    }

    fn pointer_motion(&mut self, position: (f64, f64), size: (u32, u32)) -> bool {
        self.input.motion(0, position, &mut self.model, size)
    }

    fn pointer_up(&mut self, position: (f64, f64), size: (u32, u32)) -> bool {
        let redraw = self.input.up(0, Some(position), &mut self.model, size);
        if redraw {
            self.after_input();
        }
        redraw
    }

    fn pointer_leave(&mut self) -> bool {
        self.input.cancel()
    }

    fn touch_down(&mut self, id: i32, position: (f64, f64), size: (u32, u32)) -> bool {
        self.input.down(id, position, &self.model, size)
    }

    fn touch_motion(&mut self, id: i32, position: (f64, f64), size: (u32, u32)) -> bool {
        self.input.motion(id, position, &mut self.model, size)
    }

    fn touch_up(&mut self, id: i32, size: (u32, u32)) -> bool {
        let redraw = self.input.up(id, None, &mut self.model, size);
        if redraw {
            self.after_input();
        }
        redraw
    }

    fn touch_cancel(&mut self) -> bool {
        self.input.cancel()
    }

    fn key(&mut self, code: u32, mods: Modifiers) -> bool {
        let Some(key) = keymap::text_key(code, mods) else {
            return false;
        };
        let changed = key_press(&mut self.model, key);
        if changed {
            self.after_input();
        }
        changed
    }

    fn wake(&mut self) -> bool {
        // Pick up changes other apps made (a Contacts export) while the
        // list is on screen; other pages keep what they show.
        let changed = self.model.page == Page::Browse && self.model.refresh();
        if changed {
            self.report_ui_state();
        }
        self.clock.refresh() || changed
    }

    fn deadline(&self) -> Option<std::time::Instant> {
        self.input.deadline()
    }

    fn timer(&mut self, now: std::time::Instant) -> bool {
        let redraw = self.input.timer(&mut self.model, now);
        if redraw {
            self.after_input();
        }
        redraw
    }

    fn page_name(&self) -> String {
        self.model.page.name().to_owned()
    }
}
