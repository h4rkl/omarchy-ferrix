//! Guest-side Settings client: follows `ferrix-networkd`,
//! `ferrix-telephonyd`, and `ferrix-core` through their reconnectors, sends
//! control requests, and reports Back to `ferrix-appd`.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use ferrix_app_protocol::{Client as AppdClient, ClientRole, Report};
use ferrix_hardware_api::HardwareCommand;
use ferrix_network_protocol::{
    Client as NetClient, ClientError, ClientKind as NetKind, Feed, Reconnector as NetReconnector,
};
use ferrix_telephony_protocol::{
    Client as TeleClient, ClientKind as TeleKind, Reconnector as TeleReconnector,
    Request as TeleRequest, Update,
};
use ferrix_ui::client::{ClientConfig, ClientHandler, Wake};
use ferrix_ui::clock::MinuteClock;
use ferrix_ui::draw::{Canvas, Type};
use ferrix_ui::evidence::{emit, json_escape, json_opt_str};
use ferrix_ui::keymap::{self, Modifiers};

use crate::interaction::{Input, key_press};
use crate::model::{Action, Link, Model, data_profile_from_env};
use crate::{APP_ID, EVIDENCE, screen};

const TIMEOUT: Duration = Duration::from_secs(2);

struct Settings {
    ty: Type,
    clock: MinuteClock,
    model: Model,
    input: Input,
    network: Receiver<Feed>,
    network_socket: PathBuf,
    network_client: Option<NetClient>,
    telephony: Receiver<Update>,
    telephony_socket: PathBuf,
    telephony_client: Option<TeleClient>,
    core: Receiver<ferrix_protocol::Feed>,
    core_socket: PathBuf,
    appd_socket: Option<PathBuf>,
    appd: Option<AppdClient>,
    appd_unavailable_reported: bool,
    last_ui_state: (String, String),
    last_links: (Link, Link),
    last_core_link: Link,
}

pub fn run(initial_size: (u32, u32), target: String) -> Result<(), String> {
    let config = ClientConfig {
        app_id: APP_ID.to_owned(),
        title: "Settings".to_owned(),
        initial_size,
        evidence: EVIDENCE.to_string(),
        first_frame_event: "settings-frame".to_owned(),
        first_frame_extra: vec![("target".to_owned(), target.clone())],
    };
    ferrix_ui::client::run(config, move |wake: Wake| {
        // Redraw the status-bar clock at each local minute boundary.
        let tick = Arc::clone(&wake);
        ferrix_ui::clock::spawn_minute_ticker(move || tick());
        let network_socket = ferrix_network_protocol::socket_path();
        let network = NetReconnector::new(network_socket.clone(), NetKind::Application)
            .with_timeout(TIMEOUT)
            .spawn(Arc::clone(&wake));
        let telephony_socket = ferrix_telephony_protocol::socket_path();
        let telephony =
            TeleReconnector::new(telephony_socket.clone(), TeleKind::Application, TIMEOUT)
                .spawn(Arc::clone(&wake));
        let core_socket = std::env::var_os("FERRIX_CORE_SOCKET")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(ferrix_protocol::DEFAULT_SOCKET));
        let core = ferrix_protocol::spawn_reconnecting(
            core_socket.clone(),
            TIMEOUT,
            Duration::from_secs(1),
            Arc::clone(&wake),
        );
        let mut model = Model::new();
        let apn = std::env::var(ferrix_app_api::TELEPHONY_APN_ENV).ok();
        model.set_data_profile(data_profile_from_env(apn.as_deref()));
        model.target = target;
        Ok(Settings {
            ty: Type::load()?,
            clock: MinuteClock::now(),
            model,
            input: Input::default(),
            network,
            network_socket,
            network_client: None,
            telephony,
            telephony_socket,
            telephony_client: None,
            core,
            core_socket,
            appd_socket: std::env::var_os(ferrix_app_api::APPD_SOCKET_ENV)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from),
            appd: None,
            appd_unavailable_reported: false,
            last_ui_state: (String::new(), String::new()),
            last_links: (Link::Connecting, Link::Connecting),
            last_core_link: Link::Connecting,
        })
    })
}

impl Settings {
    fn network_client(&mut self) -> Result<&mut NetClient, ClientError> {
        if self.network_client.is_none() {
            self.network_client = Some(NetClient::connect(
                &self.network_socket,
                NetKind::Application,
                TIMEOUT,
            )?);
        }
        self.network_client.as_mut().ok_or(ClientError::Io {
            reason: "no network client".to_owned(),
        })
    }

    fn network_action(
        &mut self,
        name: &str,
        run: impl FnOnce(&mut NetClient) -> Result<(), ClientError>,
    ) {
        let outcome = self.network_client().and_then(run);
        let (status, error) = match outcome {
            Ok(()) => ("ok", None),
            Err(error) => {
                let text = error.to_string();
                match error {
                    // A typed connect failure (wrong secrets, unsupported
                    // security) is the connection's state, shown with Retry.
                    ClientError::Network(error) if name == "connect" => {
                        self.model.connect_failed(error);
                    }
                    ClientError::Network(_) | ClientError::Protocol { .. } => {
                        self.model.last_error = Some(format!("network \u{b7} {text}"));
                    }
                    _ => {
                        self.network_client = None;
                        self.model.last_error = Some(format!("network \u{b7} {text}"));
                    }
                }
                ("error", Some(text))
            }
        };
        emit(format!(
            "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"settings-action\",\"action\":\"{name}\",\"service\":\"network\",\"status\":\"{status}\",\"error\":{}}}",
            json_opt_str(error.as_deref())
        ));
    }

    fn telephony_action(&mut self, name: &str, request: TeleRequest) {
        let outcome = match self.telephony_client.is_some() {
            true => Ok(()),
            false => TeleClient::connect(&self.telephony_socket, TeleKind::Application, TIMEOUT)
                .map(|c| {
                    self.telephony_client = Some(c);
                }),
        }
        .map_err(Err)
        .and_then(|()| match self.telephony_client.as_mut() {
            Some(client) => client.expect_ok(&request),
            None => Err(Err("no telephony client".to_owned())),
        });
        let (status, error) = match outcome {
            Ok(()) => ("ok", None),
            Err(Ok(error)) => {
                self.model.last_error = Some(format!("cellular \u{b7} {error}"));
                ("error", Some(error.kind().to_owned()))
            }
            Err(Err(transport)) => {
                self.telephony_client = None;
                self.model.last_error = Some("telephony \u{b7} unavailable".to_owned());
                ("unavailable", Some(transport))
            }
        };
        emit(format!(
            "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"settings-action\",\"action\":\"{name}\",\"service\":\"telephony\",\"status\":\"{status}\",\"error\":{}}}",
            json_opt_str(error.as_deref())
        ));
    }

    /// The reconnector's snapshot carries no evidence label; the daemon
    /// reports it in every hello, so (re)open the request connection and
    /// take it from there.
    fn refresh_tele_evidence(&mut self) {
        self.telephony_client = None;
        match TeleClient::connect(&self.telephony_socket, TeleKind::Application, TIMEOUT) {
            Ok(client) => {
                self.model.set_tele_evidence(client.evidence());
                self.telephony_client = Some(client);
            }
            Err(error) => emit(format!(
                "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"telephony-evidence-unavailable\",\"app\":\"{APP_ID}\",\"reason\":\"{}\"}}",
                json_escape(&error)
            )),
        }
    }

    /// A product command through `ferrix-core`; the core's events reconcile
    /// the visible state, so nothing is applied locally.
    fn core_action(&mut self, name: &str, command: HardwareCommand) {
        let (status, error) = match ferrix_protocol::command(&self.core_socket, command, TIMEOUT) {
            Ok(_) => ("ok", None),
            Err(error) => {
                self.model.last_error = Some(error.clone());
                ("error", Some(error))
            }
        };
        emit(format!(
            "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"settings-action\",\"action\":\"{name}\",\"service\":\"core\",\"status\":\"{status}\",\"error\":{}}}",
            json_opt_str(error.as_deref())
        ));
    }

    /// Re-queries the Bluetooth session; an adapter without one (no
    /// discovery or pairing) falls back to its plain device list.
    fn refresh_devices(&mut self) {
        let session = ferrix_protocol::bluetooth_session(&self.core_socket, TIMEOUT);
        let unsupported = matches!(&session, Err(reason) if reason.contains("unsupported"));
        let count = match session {
            Ok(session) => {
                let count = session.devices.len();
                self.model.set_bluetooth_session(Ok(session));
                Some(count)
            }
            Err(_) if unsupported => {
                let devices = ferrix_protocol::bluetooth_devices(&self.core_socket, TIMEOUT);
                let count = devices.as_ref().ok().map(Vec::len);
                self.model.set_bluetooth_devices(devices);
                count
            }
            Err(reason) => {
                self.model.set_bluetooth_session(Err(reason));
                None
            }
        };
        let (status, count) = match count {
            Some(count) => ("ok", count.to_string()),
            None => ("error", "null".to_owned()),
        };
        // Counts only: device names and ids never reach evidence records.
        emit(format!(
            "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"settings-action\",\"action\":\"bluetooth-devices\",\"service\":\"core\",\"status\":\"{status}\",\"devices\":{count}}}"
        ));
    }

    /// A product Bluetooth request; the reply carries the new session and
    /// later progress arrives as `bluetooth-session-changed`.
    fn bluetooth_action(&mut self, command: ferrix_hardware_api::BluetoothCommand) {
        let name = format!("bluetooth-{}", command.name());
        let (status, error) =
            match ferrix_protocol::bluetooth_command(&self.core_socket, command, TIMEOUT) {
                Ok(session) => {
                    self.model.set_bluetooth_session(Ok(session));
                    ("ok", None)
                }
                Err(error) => {
                    self.model.last_error = Some(format!("bluetooth \u{b7} {error}"));
                    ("error", Some(error))
                }
            };
        emit(format!(
            "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"settings-action\",\"action\":\"{name}\",\"service\":\"core\",\"status\":\"{status}\",\"error\":{}}}",
            json_opt_str(error.as_deref())
        ));
    }

    fn send(&mut self, action: Action) {
        match action {
            Action::SetBluetooth(enabled) => self.core_action(
                "set-bluetooth",
                HardwareCommand::SetBluetoothEnabled { enabled },
            ),
            Action::RefreshBluetoothDevices => self.refresh_devices(),
            Action::Bluetooth(command) => self.bluetooth_action(command),
            Action::SetBrightness(percent) => {
                self.core_action("set-brightness", HardwareCommand::SetBrightness { percent })
            }
            Action::SetRotationLock(locked) => self.core_action(
                "set-rotation-lock",
                HardwareCommand::SetRotationLock { locked },
            ),
            Action::Back => self.yield_foreground(),
            Action::SetRadio(enabled) => self.network_action("set-radio", |c| c.set_radio(enabled)),
            Action::Scan => self.network_action("scan", |c| c.scan()),
            Action::Connect { ap, passphrase } => {
                self.network_action("connect", move |c| c.connect_ap(&ap, passphrase.as_deref()));
            }
            Action::Disconnect => self.network_action("disconnect", |c| c.disconnect()),
            Action::Forget(profile) => self.network_action("forget", move |c| c.forget(&profile)),
            Action::SetRadioPower(power) => {
                self.telephony_action("set-radio-power", TeleRequest::SetRadioPower { power })
            }
            Action::SetCellularData(enabled) => {
                let (name, request) = if enabled {
                    (
                        "connect-data",
                        TeleRequest::ConnectData {
                            profile: self.model.data_profile.clone(),
                        },
                    )
                } else {
                    ("disconnect-data", TeleRequest::DisconnectData {})
                };
                self.telephony_action(name, request);
            }
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
        let links = (self.model.net_link, self.model.tele_link);
        if links.0 != self.last_links.0 {
            emit(format!(
                "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"network-link\",\"app\":\"{APP_ID}\",\"status\":\"{}\"}}",
                links.0.name()
            ));
        }
        if links.1 != self.last_links.1 {
            emit(format!(
                "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"telephony-link\",\"app\":\"{APP_ID}\",\"status\":\"{}\"}}",
                links.1.name()
            ));
        }
        self.last_links = links;
        if self.model.core_link != self.last_core_link {
            self.last_core_link = self.model.core_link;
            emit(format!(
                "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"core-link\",\"app\":\"{APP_ID}\",\"status\":\"{}\"}}",
                self.model.core_link.name()
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
            "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"settings-back\",\"status\":\"{status}\"}}"
        ));
    }
}

impl ClientHandler for Settings {
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
        self.input.down(0, position, &mut self.model, size)
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
        self.input.down(id, position, &mut self.model, size)
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
        key_press(&mut self.model, key);
        self.after_input();
        true
    }

    fn wake(&mut self) -> bool {
        let mut changed = false;
        while let Ok(feed) = self.network.try_recv() {
            changed |= self.model.apply_network(feed);
        }
        while let Ok(update) = self.telephony.try_recv() {
            let fresh = matches!(update, Update::Snapshot(_));
            if matches!(update, Update::Disconnected) {
                // The request connection died with the daemon.
                self.telephony_client = None;
            }
            changed |= self.model.apply_telephony(update);
            if fresh {
                self.refresh_tele_evidence();
            }
        }
        while let Ok(feed) = self.core.try_recv() {
            changed |= self.model.apply_core(feed);
        }
        if changed {
            // A core change can queue a device re-query.
            self.after_input();
        }
        self.clock.refresh() || changed
    }

    fn page_name(&self) -> String {
        self.model.page.name().to_owned()
    }
}
