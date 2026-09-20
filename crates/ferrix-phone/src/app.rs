//! Guest-side Phone client: the [`ClientHandler`] that folds telephony
//! updates and contact snapshots into the model, sends call requests, and
//! reports Back to `ferrix-appd`. Both daemons are followed through their
//! protocol reconnectors; a lost daemon leaves the UI usable and labelled.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::Duration;

use ferrix_app_protocol::{Client as AppdClient, ClientRole, Report};
use ferrix_contacts_protocol::{
    ClientKind as ContactsKind, Link as ContactsLink, Reconnector as ContactsReconnector,
};
use ferrix_telephony_api::{Dtmf, TelephonyError};
use ferrix_telephony_protocol::{Client as TeleClient, ClientKind, Reconnector, Request, Update};
use ferrix_ui::client::{ClientConfig, ClientHandler, Wake};
use ferrix_ui::draw::{Canvas, Type};
use ferrix_ui::evidence::{emit, json_escape, json_opt_str};
use ferrix_ui::keymap::{self, Modifiers};
use ferrix_ui::osk::Key;

use crate::interaction::Input;
use crate::model::{Action, Link, Model};
use crate::{APP_ID, EVIDENCE, screen};

const TITLE: &str = "Phone";
const TIMEOUT: Duration = Duration::from_secs(2);

struct Phone {
    ty: Type,
    minute: u32,
    model: Model,
    input: Input,
    size: (u32, u32),
    telephony: Receiver<Update>,
    telephony_socket: PathBuf,
    telephony_client: Option<TeleClient>,
    contacts: ContactsReconnector,
    appd_socket: Option<PathBuf>,
    appd: Option<AppdClient>,
    appd_unavailable_reported: bool,
    last_ui_state: (String, String),
    last_link: Link,
}

pub fn run(initial_size: (u32, u32), minute: u32) -> Result<(), String> {
    let config = ClientConfig {
        app_id: APP_ID.to_owned(),
        title: TITLE.to_owned(),
        initial_size,
        evidence: EVIDENCE.to_owned(),
        first_frame_event: "phone-frame".to_owned(),
        first_frame_extra: Vec::new(),
    };
    ferrix_ui::client::run(config, move |wake: Wake| {
        let telephony_socket = ferrix_telephony_protocol::socket_path();
        let telephony =
            Reconnector::new(telephony_socket.clone(), ClientKind::Application, TIMEOUT)
                .spawn(Arc::clone(&wake));
        let contacts = ContactsReconnector::start(
            ferrix_contacts_protocol::socket_path(),
            ContactsKind::Application,
            Duration::from_secs(1),
            Arc::clone(&wake),
        );
        let mut model = Model::new();
        model.region = std::env::var(ferrix_contacts_api::DEFAULT_REGION_ENV)
            .ok()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| ferrix_contacts_api::DEFAULT_REGION.to_owned());
        Ok(Phone {
            ty: Type::load()?,
            minute,
            model,
            input: Input::default(),
            size: initial_size,
            telephony,
            telephony_socket,
            telephony_client: None,
            contacts,
            appd_socket: std::env::var_os(ferrix_app_api::APPD_SOCKET_ENV)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from),
            appd: None,
            appd_unavailable_reported: false,
            last_ui_state: (String::new(), String::new()),
            last_link: Link::Connecting,
        })
    })
}

impl Phone {
    fn telephony_client(&mut self) -> Result<&mut TeleClient, String> {
        if self.telephony_client.is_none() {
            let client =
                TeleClient::connect(&self.telephony_socket, ClientKind::Application, TIMEOUT)?;
            self.telephony_client = Some(client);
        }
        self.telephony_client
            .as_mut()
            .ok_or_else(|| "no telephony client".to_owned())
    }

    fn send(&mut self, action: Action) {
        let name = match &action {
            Action::Dial(_) => "dial",
            Action::Answer(_) => "answer",
            Action::Reject(_) => "reject",
            Action::Hangup(_) => "hangup",
            Action::Hold(_) => "hold",
            Action::Resume(_) => "resume",
            Action::Dtmf(..) => "dtmf",
            Action::SetRoute(_) => "set-audio-route",
            Action::Back => "back",
        };
        let outcome: Result<(), Result<TelephonyError, String>> = match action {
            Action::Back => {
                self.yield_foreground();
                return;
            }
            Action::Dial(number) => match self.telephony_client() {
                Ok(client) => client.dial(&number).map(|_| ()),
                Err(e) => Err(Err(e)),
            },
            Action::Dtmf(call_id, digit) => match Dtmf::new(digit) {
                Ok(digit) => self.expect_ok(Request::Dtmf { call_id, digit }),
                Err(error) => Err(Ok(error)),
            },
            Action::Answer(call_id) => self.expect_ok(Request::Answer { call_id }),
            Action::Reject(call_id) => self.expect_ok(Request::Reject { call_id }),
            Action::Hangup(call_id) => self.expect_ok(Request::Hangup { call_id }),
            Action::Hold(call_id) => self.expect_ok(Request::Hold { call_id }),
            Action::Resume(call_id) => self.expect_ok(Request::Resume { call_id }),
            Action::SetRoute(route) => self.expect_ok(Request::SetAudioRoute { route }),
        };
        let (status, error) = match outcome {
            Ok(()) => ("ok", None),
            Err(Ok(error)) => {
                self.model.last_error = Some(error.to_string());
                ("error", Some(error.kind().to_owned()))
            }
            Err(Err(transport)) => {
                self.telephony_client = None;
                self.model.last_error = Some("telephony \u{b7} unavailable".to_owned());
                ("unavailable", Some(transport))
            }
        };
        emit(format!(
            "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"phone-action\",\"action\":\"{name}\",\"status\":\"{status}\",\"error\":{}}}",
            json_opt_str(error.as_deref())
        ));
    }

    fn expect_ok(&mut self, request: Request) -> Result<(), Result<TelephonyError, String>> {
        match self.telephony_client() {
            Ok(client) => client.expect_ok(&request),
            Err(e) => Err(Err(e)),
        }
    }

    fn after_input(&mut self) {
        for action in std::mem::take(&mut self.model.pending) {
            self.send(action);
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
        if self.model.link != self.last_link {
            self.last_link = self.model.link;
            emit(format!(
                "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"telephony-link\",\"app\":\"{APP_ID}\",\"status\":\"{}\"}}",
                self.model.link.name()
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
            "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"phone-back\",\"status\":\"{status}\"}}"
        ));
    }
}

impl ClientHandler for Phone {
    fn draw(&mut self, size: (u32, u32)) -> Canvas {
        self.size = size;
        screen::compose(&self.ty, size, self.minute, &self.model, &self.input)
    }

    fn pointer_down(&mut self, position: (f64, f64), size: (u32, u32)) -> bool {
        self.input.down(0, position, &self.model, size)
    }

    fn pointer_motion(&mut self, position: (f64, f64), size: (u32, u32)) -> bool {
        self.input.motion(0, position, &self.model, size)
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
        self.input.motion(id, position, &self.model, size)
    }

    fn touch_up(&mut self, id: i32, size: (u32, u32)) -> bool {
        // Touch up carries no position; the tracker keeps the last motion.
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
        // A hardware keyboard types into the dialer: digits, `*`, `#`, `+`,
        // backspace, and Enter to call.
        match keymap::key_for(code, mods.shift) {
            Some(Key::Char(ch)) if ch.is_ascii_digit() || matches!(ch, '*' | '#' | '+') => {
                self.model.key(ch)
            }
            Some(Key::Backspace) => self.model.delete(),
            Some(Key::Esc) => self.model.clear(),
            Some(Key::Enter) => {
                if let Err(block) = self.model.call() {
                    self.model.last_error = Some(block.reason().to_owned());
                }
            }
            _ => return false,
        }
        self.after_input();
        true
    }

    fn wake(&mut self) -> bool {
        let mut changed = false;
        loop {
            match self.telephony.try_recv() {
                Ok(update) => changed |= self.model.apply(update),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }
        for link in self.contacts.drain() {
            match link {
                ContactsLink::Connected { contacts } => {
                    emit(format!(
                        "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"contacts-link\",\"app\":\"{APP_ID}\",\"status\":\"connected\",\"contacts\":{}}}",
                        contacts.len()
                    ));
                    self.model.set_contacts(contacts);
                    changed = true;
                }
                ContactsLink::Event(_) => {
                    // A change arrives as an event; the next reconnect or
                    // lookup refreshes names. Names are display-only here.
                }
                ContactsLink::Lost { .. } => {
                    emit(format!(
                        "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"contacts-link\",\"app\":\"{APP_ID}\",\"status\":\"lost\"}}"
                    ));
                }
            }
        }
        if changed {
            self.report_ui_state();
        }
        changed
    }

    fn page_name(&self) -> String {
        self.model.page.name().to_owned()
    }
}
