//! The Bluetooth flows of Settings: saved devices, Pair new device (bounded
//! scan, available devices, the pairing prompt, the outcome), and the device
//! detail (connect, disconnect, forget with confirmation). Pure state over
//! the `ferrix-core` Bluetooth session; every request goes out as an
//! [`Action::Bluetooth`] and the core's session stays authoritative.
//!
//! Records name states only (`pair-scanning-3`, `confirm-passkey`,
//! `pair-failed-rejected`); never device names, ids, or passkeys.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use ferrix_hardware_api::{
    BluetoothAction, BluetoothCommand, BluetoothDevice, BluetoothDeviceKind, BluetoothDiscovery,
    BluetoothPairing, BluetoothSession, PairingFailure, PairingPhase,
};

use crate::model::{Action, Model, Page};

/// The Bluetooth session as last queried from the core.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum DeviceList {
    /// Not queried yet (or a fresh core connection).
    #[default]
    Unknown,
    /// `controls` is false when the target's adapter lists devices but
    /// offers no discovery or pairing (the session request is unsupported).
    Loaded {
        session: BluetoothSession,
        controls: bool,
    },
    /// The query failed with this typed reason.
    Failed(String),
}

/// Which Bluetooth screen is shown.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum BtView {
    /// Power, Pair new device, saved devices.
    #[default]
    Main,
    /// Scan and the available-device list.
    Pair,
    /// One saved device: connect/disconnect/forget.
    Device(String),
}

/// The focused pairing card that replaces the lists while it is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Card {
    Confirm { passkey: u32 },
    Authorize,
    Display { passkey: u32 },
    Pairing,
    Paired,
    Failed(PairingFailure),
}

impl Card {
    fn from_phase(phase: PairingPhase) -> Self {
        match phase {
            PairingPhase::ConfirmPasskey { passkey } => Card::Confirm { passkey },
            PairingPhase::Authorize => Card::Authorize,
            PairingPhase::DisplayPasskey { passkey } => Card::Display { passkey },
            PairingPhase::Pairing => Card::Pairing,
            PairingPhase::Paired => Card::Paired,
            PairingPhase::Failed { reason } => Card::Failed(reason),
        }
    }

    /// Whether the card offers Pair/Cancel.
    pub fn asks(self) -> bool {
        matches!(self, Card::Confirm { .. } | Card::Authorize)
    }
}

/// `482913` as `482 913`, the way phones show a pairing code.
pub fn format_passkey(passkey: u32) -> String {
    let digits = format!("{:06}", passkey % 1_000_000);
    format!("{} {}", &digits[..3], &digits[3..])
}

pub fn device_name(device: &BluetoothDevice) -> &str {
    device.name.as_deref().unwrap_or("Unnamed device")
}

pub fn kind_label(kind: BluetoothDeviceKind) -> &'static str {
    match kind {
        BluetoothDeviceKind::Audio => "audio",
        BluetoothDeviceKind::Input => "input",
        BluetoothDeviceKind::Phone => "phone",
        BluetoothDeviceKind::Computer => "computer",
        BluetoothDeviceKind::Wearable => "wearable",
        BluetoothDeviceKind::Other => "device",
    }
}

/// The plain sentence for a failed pairing, naming the device.
pub fn failure_text(reason: PairingFailure, name: &str) -> String {
    match reason {
        PairingFailure::Declined => "Pairing cancelled".to_owned(),
        PairingFailure::Rejected => format!("{name} rejected the pairing"),
        PairingFailure::TimedOut => "Pairing timed out".to_owned(),
        PairingFailure::Canceled => "Pairing was cancelled".to_owned(),
        PairingFailure::PinRequired => {
            format!("{name} needs a PIN; PIN entry is not supported yet")
        }
        PairingFailure::Unreachable => format!("{name} is out of range"),
        PairingFailure::Failed => format!("Couldn't pair with {name}"),
    }
}

impl Model {
    // -- session feed --------------------------------------------------------

    pub fn set_bluetooth_session(&mut self, result: Result<BluetoothSession, String>) {
        self.bt_stale = false;
        self.bt_devices = match result {
            Ok(session) => DeviceList::Loaded {
                session,
                controls: true,
            },
            Err(reason) => DeviceList::Failed(reason),
        };
        self.reconcile_bt_view();
    }

    /// Fallback for adapters without a session: the device list only.
    pub fn set_bluetooth_devices(&mut self, result: Result<Vec<BluetoothDevice>, String>) {
        self.bt_stale = false;
        self.bt_devices = match result {
            Ok(devices) => DeviceList::Loaded {
                session: BluetoothSession {
                    devices,
                    ..BluetoothSession::default()
                },
                controls: false,
            },
            Err(reason) => DeviceList::Failed(reason),
        };
        self.reconcile_bt_view();
    }

    /// A device that disappeared closes its detail view.
    fn reconcile_bt_view(&mut self) {
        if let BtView::Device(id) = &self.bt_view
            && self.bt_session().is_some_and(|s| s.device(id).is_none())
        {
            self.bt_view = BtView::Main;
            self.bt_forget_confirm = false;
        }
    }

    pub fn bt_session(&self) -> Option<&BluetoothSession> {
        match &self.bt_devices {
            DeviceList::Loaded { session, .. } => Some(session),
            _ => None,
        }
    }

    /// Whether discovery and pairing are offered (on, and supported).
    pub fn bt_controls(&self) -> bool {
        matches!(self.bt_devices, DeviceList::Loaded { controls: true, .. }) && self.bluetooth_on()
    }

    pub fn devices(&self) -> &[BluetoothDevice] {
        self.bt_session().map_or(&[], |s| s.devices.as_slice())
    }

    pub fn saved_devices(&self) -> Vec<&BluetoothDevice> {
        self.devices().iter().filter(|d| d.paired).collect()
    }

    /// Unpaired devices in range: named first, strongest signal first.
    pub fn available_devices(&self) -> Vec<&BluetoothDevice> {
        let mut devices: Vec<&BluetoothDevice> =
            self.devices().iter().filter(|d| !d.paired).collect();
        devices.sort_by(|a, b| {
            a.name
                .is_none()
                .cmp(&b.name.is_none())
                .then_with(|| b.signal_bars.cmp(&a.signal_bars))
                .then_with(|| {
                    device_name(a)
                        .to_lowercase()
                        .cmp(&device_name(b).to_lowercase())
                })
                .then_with(|| a.id.cmp(&b.id))
        });
        devices
    }

    pub fn discovery(&self) -> BluetoothDiscovery {
        self.bt_session().map(|s| s.discovery).unwrap_or_default()
    }

    pub fn pairing(&self) -> Option<&BluetoothPairing> {
        self.bt_session().and_then(|s| s.pairing.as_ref())
    }

    /// The device a pairing attempt or detail view is about.
    pub fn bt_device(&self, id: &str) -> Option<&BluetoothDevice> {
        self.bt_session().and_then(|s| s.device(id))
    }

    pub fn pairing_name(&self) -> String {
        self.pairing()
            .and_then(|p| self.bt_device(&p.device))
            .map_or_else(|| "the device".to_owned(), |d| device_name(d).to_owned())
    }

    /// The pairing card, while it applies: a running attempt anywhere in
    /// the Bluetooth page (an incoming request too), a finished one only
    /// in the Pair view that started it.
    pub fn bt_card(&self) -> Option<Card> {
        if self.page != Page::Bluetooth || self.bluetooth_block().is_err() {
            return None;
        }
        let phase = self.pairing()?.phase;
        (!phase.is_finished() || self.bt_view == BtView::Pair).then(|| Card::from_phase(phase))
    }

    /// The device of the detail view.
    pub fn detail_device(&self) -> Option<&BluetoothDevice> {
        match &self.bt_view {
            BtView::Device(id) => self.bt_device(id),
            _ => None,
        }
    }

    /// The running or failed operation on the detail view's device.
    pub fn detail_operation(&self) -> Option<&ferrix_hardware_api::BluetoothOperation> {
        let device = self.detail_device()?;
        self.bt_session()?
            .operation
            .as_ref()
            .filter(|o| o.device == device.id)
    }

    // -- intents -------------------------------------------------------------

    fn bt_request(&mut self, command: BluetoothCommand) {
        self.last_error = None;
        self.pending.push(Action::Bluetooth(command));
    }

    fn bt_refused(&mut self) -> bool {
        if let Err(block) = self.bluetooth_block() {
            self.last_error = Some(block.reason().to_owned());
            return true;
        }
        if !self.bluetooth_on() {
            self.last_error = Some("bluetooth \u{b7} off".to_owned());
            return true;
        }
        if !self.bt_controls() {
            self.last_error =
                Some("bluetooth \u{b7} pairing not supported on this target".to_owned());
            return true;
        }
        false
    }

    /// Opens Pair new device and starts a bounded scan.
    pub fn open_pair_page(&mut self) {
        if self.bt_refused() {
            return;
        }
        self.bt_view = BtView::Pair;
        self.bt_forget_confirm = false;
        if !self.discovery().active {
            self.bt_request(BluetoothCommand::StartDiscovery);
        }
    }

    pub fn rescan(&mut self) {
        if self.bt_refused() {
            return;
        }
        self.bt_request(BluetoothCommand::StartDiscovery);
    }

    pub fn pair_available(&mut self, index: usize) {
        if self.bt_refused() {
            return;
        }
        let Some(id) = self.available_devices().get(index).map(|d| d.id.clone()) else {
            return;
        };
        self.bt_request(BluetoothCommand::Pair { device: id });
    }

    /// Try again after a failed attempt.
    pub fn retry_pairing(&mut self) {
        let Some(id) = self.pairing().map(|p| p.device.clone()) else {
            return;
        };
        if self.bt_refused() {
            return;
        }
        self.bt_request(BluetoothCommand::Pair { device: id });
    }

    pub fn reply_pairing(&mut self, accept: bool) {
        if self.bt_card().is_some_and(Card::asks) {
            self.bt_request(BluetoothCommand::ReplyPairing { accept });
        }
    }

    /// Cancel a running attempt, or dismiss a finished one's outcome.
    pub fn dismiss_pairing(&mut self) {
        if self.pairing().is_some() {
            self.bt_request(BluetoothCommand::CancelPairing);
        }
    }

    /// Done on the outcome card: dismiss it and return to saved devices.
    pub fn finish_pairing(&mut self) {
        self.dismiss_pairing();
        self.leave_pair_page();
    }

    fn leave_pair_page(&mut self) {
        if self.discovery().active && self.bt_controls() {
            self.bt_request(BluetoothCommand::StopDiscovery);
        }
        self.bt_view = BtView::Main;
    }

    pub fn open_saved(&mut self, index: usize) {
        if let Some(id) = self.saved_devices().get(index).map(|d| d.id.clone()) {
            self.bt_view = BtView::Device(id);
            self.bt_forget_confirm = false;
            self.last_error = None;
        }
    }

    pub fn toggle_connection(&mut self) {
        let Some(device) = self.detail_device() else {
            return;
        };
        let id = device.id.clone();
        let connected = device.connected;
        if self.bt_refused() {
            return;
        }
        self.bt_request(if connected {
            BluetoothCommand::Disconnect { device: id }
        } else {
            BluetoothCommand::Connect { device: id }
        });
    }

    pub fn ask_forget_device(&mut self) {
        if self.detail_device().is_some() && !self.bt_refused() {
            self.bt_forget_confirm = true;
        }
    }

    pub fn confirm_forget_device(&mut self) {
        let Some(id) = self.detail_device().map(|d| d.id.clone()) else {
            return;
        };
        self.bt_forget_confirm = false;
        if self.bt_refused() {
            return;
        }
        self.bt_request(BluetoothCommand::Forget { device: id });
        self.bt_view = BtView::Main;
    }

    pub fn cancel_forget_device(&mut self) {
        self.bt_forget_confirm = false;
    }

    /// Back inside the Bluetooth page: answer a prompt with Cancel, close
    /// the forget confirmation, leave Pair/Device for the saved list.
    /// Returns false when Back should leave the page.
    pub fn bluetooth_back(&mut self) -> bool {
        if self.bt_forget_confirm {
            self.bt_forget_confirm = false;
            return true;
        }
        match self.bt_card() {
            Some(card) if card.asks() => {
                self.reply_pairing(false);
                return true;
            }
            Some(Card::Display { .. } | Card::Pairing) => {
                self.dismiss_pairing();
                return true;
            }
            Some(Card::Paired | Card::Failed(_)) => {
                self.finish_pairing();
                return true;
            }
            _ => {}
        }
        match self.bt_view {
            BtView::Main => false,
            BtView::Pair => {
                self.leave_pair_page();
                true
            }
            BtView::Device(_) => {
                self.bt_view = BtView::Main;
                true
            }
        }
    }

    // -- names ---------------------------------------------------------------

    /// The header label of the Bluetooth page's current screen.
    pub fn bluetooth_subtitle(&self) -> &'static str {
        match self.bt_view {
            BtView::Main => "Bluetooth",
            BtView::Pair => "Pair new device",
            BtView::Device(_) => "Device",
        }
    }

    /// `ui-state` name of the Bluetooth flows (counts and stages only).
    pub fn bluetooth_flow_state(&self) -> Option<String> {
        if let Some(card) = self.bt_card() {
            return Some(match card {
                Card::Confirm { .. } => "confirm-passkey".to_owned(),
                Card::Authorize => "authorize".to_owned(),
                Card::Display { .. } => "display-passkey".to_owned(),
                Card::Pairing => "pairing".to_owned(),
                Card::Paired => "paired".to_owned(),
                Card::Failed(reason) => format!("pair-failed-{}", reason.name()),
            });
        }
        match &self.bt_view {
            BtView::Main => None,
            BtView::Pair => Some(format!(
                "pair-{}-{}",
                if self.discovery().active {
                    "scanning"
                } else {
                    "idle"
                },
                self.available_devices().len()
            )),
            BtView::Device(_) if self.bt_forget_confirm => Some("confirm-forget-device".to_owned()),
            BtView::Device(_) => Some(match (self.detail_device(), self.detail_operation()) {
                (_, Some(op)) if op.failed.is_some() => {
                    format!("device-{}-failed", op.action.name())
                }
                (_, Some(op)) => format!(
                    "device-{}",
                    match op.action {
                        BluetoothAction::Connect => "connecting",
                        BluetoothAction::Disconnect => "disconnecting",
                    }
                ),
                (Some(d), None) if d.connected => "device-connected".to_owned(),
                _ => "device-disconnected".to_owned(),
            }),
        }
    }
}

/// Canned session for the host previews (a mock target, never a device).
pub fn preview_session(phase: Option<PairingPhase>) -> BluetoothSession {
    let device = |id: &str, name: Option<&str>, kind, paired, connected, bars| BluetoothDevice {
        id: id.to_owned(),
        name: name.map(str::to_owned),
        paired,
        connected,
        kind,
        signal_bars: bars,
    };
    use BluetoothDeviceKind as K;
    let mut devices = vec![
        device(
            "mock-bt-0",
            Some("Ferrix Mock Earbuds"),
            K::Audio,
            true,
            true,
            None,
        ),
        device(
            "mock-bt-1",
            Some("Ferrix Mock Speaker"),
            K::Audio,
            true,
            false,
            None,
        ),
        device(
            "mock-bt-found-0",
            Some("Ferrix Mock Headphones"),
            K::Audio,
            false,
            false,
            Some(4),
        ),
        device(
            "mock-bt-found-1",
            Some("Ferrix Mock Keyboard"),
            K::Input,
            false,
            false,
            Some(3),
        ),
        device(
            "mock-bt-found-3",
            Some("Ferrix Mock Watch"),
            K::Wearable,
            false,
            false,
            Some(2),
        ),
        device("mock-bt-2", None, K::Other, false, false, None),
    ];
    if phase == Some(PairingPhase::Paired) {
        devices[2].paired = true;
        devices[2].connected = true;
    }
    ferrix_hardware_api::sort_bluetooth_devices(&mut devices);
    BluetoothSession {
        discovery: BluetoothDiscovery {
            active: phase.is_none(),
            remaining_ms: phase.is_none().then_some(23_000),
        },
        devices,
        pairing: phase.map(|phase| BluetoothPairing {
            device: "mock-bt-found-0".to_owned(),
            phase,
        }),
        operation: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system::{preview_capabilities, preview_hardware};
    use ferrix_protocol::Feed as CoreFeed;

    fn on_page() -> Model {
        let mut m = Model::new();
        m.apply_core(CoreFeed::Snapshot {
            target: "qemu-aarch64".into(),
            profile: "emulator-dev".into(),
            capabilities: preview_capabilities(),
            state: preview_hardware(),
        });
        m.show(Page::Bluetooth);
        m.pending.clear();
        m.set_bluetooth_session(Ok(preview_session(None)));
        m
    }

    fn requests(m: &mut Model) -> Vec<BluetoothCommand> {
        std::mem::take(&mut m.pending)
            .into_iter()
            .filter_map(|a| match a {
                Action::Bluetooth(c) => Some(c),
                _ => None,
            })
            .collect()
    }

    fn with_phase(m: &mut Model, phase: PairingPhase) {
        m.set_bluetooth_session(Ok(preview_session(Some(phase))));
    }

    #[test]
    fn saved_and_available_lists_split_on_the_bond() {
        let m = on_page();
        let saved: Vec<&str> = m.saved_devices().iter().map(|d| device_name(d)).collect();
        assert_eq!(saved, ["Ferrix Mock Earbuds", "Ferrix Mock Speaker"]);
        let available: Vec<&str> = m
            .available_devices()
            .iter()
            .map(|d| device_name(d))
            .collect();
        assert_eq!(
            available,
            [
                "Ferrix Mock Headphones",
                "Ferrix Mock Keyboard",
                "Ferrix Mock Watch",
                "Unnamed device"
            ],
            "named first, strongest first"
        );
        assert_eq!(format_passkey(482_913), "482 913");
        assert_eq!(format_passkey(7), "000 007");
    }

    #[test]
    fn pair_flow_scans_prompts_and_answers() {
        let mut m = on_page();
        m.set_bluetooth_session(Ok(BluetoothSession {
            discovery: BluetoothDiscovery::default(),
            ..preview_session(None)
        }));
        m.open_pair_page();
        assert_eq!(m.bt_view, BtView::Pair);
        assert_eq!(requests(&mut m), [BluetoothCommand::StartDiscovery]);
        m.set_bluetooth_session(Ok(preview_session(None)));
        assert_eq!(m.state_name(), "pair-scanning-4");
        m.pair_available(0);
        assert_eq!(
            requests(&mut m),
            [BluetoothCommand::Pair {
                device: "mock-bt-found-0".into()
            }]
        );
        with_phase(&mut m, PairingPhase::ConfirmPasskey { passkey: 482_913 });
        assert_eq!(m.bt_card(), Some(Card::Confirm { passkey: 482_913 }));
        assert_eq!(m.state_name(), "confirm-passkey");
        assert_eq!(m.pairing_name(), "Ferrix Mock Headphones");
        m.reply_pairing(true);
        assert_eq!(
            requests(&mut m),
            [BluetoothCommand::ReplyPairing { accept: true }]
        );
        with_phase(&mut m, PairingPhase::Paired);
        assert_eq!(m.state_name(), "paired");
        m.finish_pairing();
        assert_eq!(requests(&mut m), [BluetoothCommand::CancelPairing]);
        assert_eq!(m.bt_view, BtView::Main);
        assert_eq!(m.bt_card(), None, "a finished outcome stays in Pair");
    }

    #[test]
    fn back_answers_prompts_with_cancel_and_leaves_pair_stopping_the_scan() {
        let mut m = on_page();
        m.open_pair_page();
        assert!(requests(&mut m).is_empty(), "already scanning");
        with_phase(&mut m, PairingPhase::Authorize);
        m.back();
        assert_eq!(
            requests(&mut m),
            [BluetoothCommand::ReplyPairing { accept: false }]
        );
        with_phase(
            &mut m,
            PairingPhase::Failed {
                reason: PairingFailure::Rejected,
            },
        );
        assert_eq!(m.state_name(), "pair-failed-rejected");
        m.retry_pairing();
        assert_eq!(
            requests(&mut m),
            [BluetoothCommand::Pair {
                device: "mock-bt-found-0".into()
            }]
        );
        m.set_bluetooth_session(Ok(preview_session(None)));
        m.back();
        assert_eq!(requests(&mut m), [BluetoothCommand::StopDiscovery]);
        assert_eq!(m.bt_view, BtView::Main);
        m.back();
        assert_eq!(m.page, Page::Root);
    }

    #[test]
    fn device_detail_connects_disconnects_and_forgets_after_confirmation() {
        let mut m = on_page();
        m.open_saved(0);
        assert_eq!(m.state_name(), "device-connected");
        m.toggle_connection();
        assert_eq!(
            requests(&mut m),
            [BluetoothCommand::Disconnect {
                device: "mock-bt-0".into()
            }]
        );
        m.ask_forget_device();
        assert_eq!(m.state_name(), "confirm-forget-device");
        assert!(
            requests(&mut m).is_empty(),
            "nothing sent before confirming"
        );
        m.back();
        assert!(!m.bt_forget_confirm, "back cancels the confirmation");
        m.ask_forget_device();
        m.confirm_forget_device();
        assert_eq!(
            requests(&mut m),
            [BluetoothCommand::Forget {
                device: "mock-bt-0".into()
            }]
        );
        assert_eq!(m.bt_view, BtView::Main);
        // A device that vanished closes its detail view.
        m.open_saved(1);
        let mut session = preview_session(None);
        session.devices.retain(|d| d.id != "mock-bt-1");
        m.set_bluetooth_session(Ok(session));
        assert_eq!(m.bt_view, BtView::Main);
    }

    #[test]
    fn controls_are_refused_with_reasons_when_off_or_unsupported() {
        let mut m = on_page();
        m.set_bluetooth_devices(Ok(preview_session(None).devices));
        m.open_pair_page();
        assert_eq!(m.bt_view, BtView::Main);
        assert_eq!(
            m.last_error.as_deref(),
            Some("bluetooth \u{b7} pairing not supported on this target")
        );
        m.set_bluetooth_session(Ok(preview_session(None)));
        let mut hw = preview_hardware();
        hw.bluetooth.enabled = false;
        m.apply_core(CoreFeed::Event(
            ferrix_hardware_api::HardwareEvent::BluetoothChanged(hw.bluetooth),
        ));
        m.open_pair_page();
        assert_eq!(m.last_error.as_deref(), Some("bluetooth \u{b7} off"));
        assert!(
            requests(&mut m)
                .iter()
                .all(|c| *c != BluetoothCommand::StartDiscovery)
        );
    }
}
