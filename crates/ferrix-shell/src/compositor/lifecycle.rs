//! Smithay side of the screen lifecycle (`crate::power`) and the trusted
//! power menu (`crate::power_menu`).
//!
//! * The power key is taken out of the keyboard stream before xkb or any
//!   client sees it (`input.rs`), and fed to [`ScreenLifecycle`].
//! * Screen off: every toplevel leaves the output (Waydroid's hwcomposer
//!   treats that as display-off and lets Android sleep), live touches and
//!   pointer buttons are cancelled for clients, and the backend DPMSes the
//!   DRM output (`take_display_power`). While off, no input reaches clients.
//! * Screen on: the backend restores the output, the front toplevel enters
//!   it again (`refocus`, so the hwcomposer wakes and the front APK draws),
//!   and the whole scene is redrawn.
//! * The menu is drawn by the compositor and owns pointer/touch input while
//!   open; its actions go to `ferrix-core` as typed `HardwareCommand`s on a
//!   worker thread so the compositor loop never blocks.
//! * The idle timeout and inhibitors come from `ferrix-core`'s screen policy
//!   watch; the shell only enforces them.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use ferrix_hardware_api::{HardwareCommand, PowerAction, ScreenPolicy, SuspendStatus};
use ferrix_protocol::PolicyFeed;
use smithay::{
    backend::{
        allocator::Fourcc,
        input::{ButtonState, TouchSlot},
        renderer::element::memory::MemoryRenderBuffer,
    },
    desktop::space::SpaceElement,
    input::pointer::ButtonEvent,
    reexports::calloop::ping::Ping,
    utils::{SERIAL_COUNTER, Transform},
};

use super::Shell;
use crate::evidence::Value;
use crate::power::{Effect, MenuClose, PowerPress, Screen, ScreenLifecycle};
use crate::power_menu::{self, MenuButton, MenuLayout, MenuStatus};

/// Bound on one power command to the core.
const POWER_COMMAND_TIMEOUT: Duration = Duration::from_secs(5);

/// An open power menu.
pub struct MenuState {
    pub layout: MenuLayout,
    pub status: MenuStatus,
    /// The button under the active contact (pressed feedback).
    pub pressed: Option<MenuButton>,
    /// The drawn menu; rebuilt when `dirty`.
    pub buffer: Option<MemoryRenderBuffer>,
    dirty: bool,
}

/// Lifecycle state the compositor carries (one field on [`Shell`]).
pub struct Lifecycle {
    pub screen: ScreenLifecycle,
    pub menu: Option<MenuState>,
    policy: Option<ScreenPolicy>,
    policy_feed: Option<Receiver<PolicyFeed>>,
    /// Tells the core whether the screen is off (the suspend policy's input).
    screen_report: Option<Sender<bool>>,
    power_tx: Sender<(PowerAction, Result<(), String>)>,
    power_rx: Receiver<(PowerAction, Result<(), String>)>,
    ping: Ping,
    core_socket: PathBuf,
    /// A DPMS change the backend has not applied yet.
    pending_display: Option<Screen>,
    /// The backend restored the output; report the first frame after it.
    pub wake_frame_pending: bool,
    /// Touch slots whose down reached a client (only their motion/up may).
    live_touch: Vec<TouchSlot>,
    /// Pointer buttons whose press reached a client.
    live_buttons: Vec<u32>,
    /// The touch contact driving the open menu and its last position.
    pub menu_touch: Option<(TouchSlot, (f64, f64))>,
    /// Loaded on first use (the menu is the compositor's only text).
    ty: Option<ferrix_ui::draw::Type>,
}

impl Lifecycle {
    pub fn new(core_socket: PathBuf, ping: Ping) -> Self {
        let (power_tx, power_rx) = mpsc::channel();
        Self {
            screen: ScreenLifecycle::new(Instant::now()),
            menu: None,
            policy: None,
            policy_feed: None,
            screen_report: None,
            power_tx,
            power_rx,
            ping,
            core_socket,
            pending_display: None,
            wake_frame_pending: false,
            live_touch: Vec::new(),
            live_buttons: Vec::new(),
            menu_touch: None,
            ty: None,
        }
    }

    /// Follow the core's screen policy (reconnecting).
    pub fn watch_policy(&mut self) {
        let ping = self.ping.clone();
        self.policy_feed = Some(ferrix_protocol::watch_screen_policy(
            self.core_socket.clone(),
            Duration::from_secs(2),
            Duration::from_secs(1),
            std::sync::Arc::new(move || ping.ping()),
        ));
        let report = ferrix_protocol::report_screen(
            self.core_socket.clone(),
            Duration::from_secs(2),
            Duration::from_secs(1),
        );
        let _ = report.send(self.screen_off());
        self.screen_report = Some(report);
    }

    /// Tell the core the screen's power state (never blocks).
    fn report_screen(&self) {
        if let Some(report) = &self.screen_report {
            let _ = report.send(self.screen_off());
        }
    }

    pub fn screen_off(&self) -> bool {
        !self.screen.admits_input()
    }

    pub fn touch_went_down(&mut self, slot: TouchSlot) {
        if !self.live_touch.contains(&slot) {
            self.live_touch.push(slot);
        }
    }

    /// Whether this slot's motion/up may reach a client.
    pub fn touch_is_live(&self, slot: TouchSlot) -> bool {
        self.live_touch.contains(&slot)
    }

    pub fn touch_lifted(&mut self, slot: TouchSlot) {
        self.live_touch.retain(|s| *s != slot);
    }

    /// The device cancelled every contact.
    pub fn touches_cancelled(&mut self) {
        self.live_touch.clear();
        self.menu_touch = None;
    }

    pub fn button_pressed(&mut self, button: u32) {
        if !self.live_buttons.contains(&button) {
            self.live_buttons.push(button);
        }
    }

    /// Whether this button's release may reach a client.
    pub fn button_is_live(&self, button: u32) -> bool {
        self.live_buttons.contains(&button)
    }

    pub fn button_released(&mut self, button: u32) {
        self.live_buttons.retain(|b| *b != button);
    }
}

impl Shell {
    /// A power key press or release from any keyboard device.
    pub fn power_key(&mut self, pressed: bool) {
        let (press, effects) = self.life.screen.power_key(pressed, Instant::now());
        self.report_press(press);
        self.apply_effects(effects);
    }

    fn report_press(&self, press: Option<PowerPress>) {
        if let Some(press) = press {
            self.emit(
                "power-key",
                &[
                    ("press", press.label().into()),
                    ("screen", self.life.screen.screen().label().into()),
                ],
            );
        }
    }

    /// Called every backend loop iteration: long-press and idle deadlines.
    pub fn lifecycle_tick(&mut self) {
        let (press, effects) = self.life.screen.tick(Instant::now());
        self.report_press(press);
        self.apply_effects(effects);
    }

    /// Admitted input that counts as user activity.
    pub fn user_activity(&mut self) {
        self.life.screen.activity(Instant::now());
    }

    /// The DPMS state the backend must apply, once.
    pub fn take_display_power(&mut self) -> Option<Screen> {
        self.life.pending_display.take()
    }

    /// Record one dropped contact (only its first event, never motion).
    pub fn input_dropped(&self, kind: &str) {
        self.emit(
            "input-dropped",
            &[("kind", kind.into()), ("reason", "screen-off".into())],
        );
    }

    fn apply_effects(&mut self, effects: Vec<Effect>) {
        for effect in effects {
            match effect {
                Effect::ScreenOff(cause) => self.blank(cause.label()),
                Effect::ScreenOn(cause) => self.unblank(cause.label()),
                Effect::OpenMenu => self.open_menu(),
                Effect::CloseMenu(reason) => self.close_menu(reason),
            }
        }
    }

    /// Cancel everything clients currently hold: touches, pressed buttons,
    /// and the home gesture. Used when the screen turns off or the menu
    /// takes input, so no contact can complete behind either.
    fn cancel_client_contacts(&mut self) {
        self.gesture.cancel();
        self.gesture_source = None;
        if let Some(touch) = self.seat.get_touch() {
            touch.cancel(self);
        }
        self.life.live_touch.clear();
        let buttons = std::mem::take(&mut self.life.live_buttons);
        if let Some(pointer) = self.seat.get_pointer()
            && !buttons.is_empty()
        {
            let time = self.clock.now().as_millis();
            for button in buttons {
                pointer.button(
                    self,
                    &ButtonEvent {
                        button,
                        state: ButtonState::Released,
                        serial: SERIAL_COUNTER.next_serial(),
                        time,
                    },
                );
            }
            pointer.frame(self);
        }
    }

    fn blank(&mut self, cause: &str) {
        self.cancel_client_contacts();
        // Off the output, every window is invisible: Waydroid's hwcomposer
        // takes this as display-off. Leave is sent only to windows that
        // entered, so this is the front window.
        let mut left = 0;
        for window in self.windows.iter() {
            window.output_leave(&self.output);
            left += 1;
        }
        self.life.pending_display = Some(Screen::Off);
        self.life.wake_frame_pending = false;
        self.emit(
            "screen-power",
            &[
                ("state", "off".into()),
                ("cause", cause.into()),
                ("windows_left", Value::Int(left)),
            ],
        );
        // After the record, so the core's report-screen record follows it.
        self.life.report_screen();
    }

    fn unblank(&mut self, cause: &str) {
        self.life.pending_display = Some(Screen::On);
        self.life.wake_frame_pending = true;
        // Re-enter the front window (wl_surface.enter): the hwcomposer wakes
        // and the front APK draws again. Also invalidates damage history.
        self.refocus();
        self.emit(
            "screen-power",
            &[
                ("state", "on".into()),
                ("cause", cause.into()),
                ("front_pid", self.focused_pid().into()),
                (
                    "front_app_id",
                    self.windows
                        .focused()
                        .and_then(|w| self.windows.app_id_of(w))
                        .unwrap_or_default()
                        .into(),
                ),
            ],
        );
        self.life.report_screen();
    }

    fn open_menu(&mut self) {
        self.cancel_client_contacts();
        let (w, h) = self.plan.logical_size();
        self.life.menu = Some(MenuState {
            layout: MenuLayout::new((w, h)),
            status: MenuStatus::Choosing,
            pressed: None,
            buffer: None,
            dirty: true,
        });
        self.emit("power-menu", &[("state", "open".into())]);
        self.request_full_redraw();
    }

    fn close_menu(&mut self, reason: MenuClose) {
        // A finger still on the menu never reaches a client after it.
        self.life.menu_touch = None;
        if self.life.menu.take().is_some() {
            self.emit(
                "power-menu",
                &[
                    ("state", "closed".into()),
                    ("reason", reason.label().into()),
                ],
            );
            self.request_full_redraw();
        }
    }

    /// The output was re-planned (rotation): redraw the menu at the new size.
    pub fn relayout_menu(&mut self) {
        let (w, h) = self.plan.logical_size();
        if let Some(menu) = self.life.menu.as_mut() {
            menu.layout = MenuLayout::new((w, h));
            menu.pressed = None;
            menu.dirty = true;
        }
    }

    pub fn menu_open(&self) -> bool {
        self.life.menu.is_some()
    }

    /// A contact went down on the menu (logical output coordinates).
    pub fn menu_press(&mut self, position: (f64, f64)) {
        let Some(menu) = self.life.menu.as_mut() else {
            return;
        };
        let target = menu.layout.target(position, &menu.status);
        if menu.pressed != target {
            menu.pressed = target;
            menu.dirty = true;
            self.request_redraw();
        }
    }

    /// The contact moved: pressed feedback follows it off the button.
    pub fn menu_motion(&mut self, position: (f64, f64)) {
        let Some(menu) = self.life.menu.as_mut() else {
            return;
        };
        if menu.pressed.is_some() && menu.layout.target(position, &menu.status) != menu.pressed {
            menu.pressed = None;
            menu.dirty = true;
            self.request_redraw();
        }
    }

    /// The contact lifted: act on the button it is still over.
    pub fn menu_release(&mut self, position: (f64, f64)) {
        let Some(menu) = self.life.menu.as_mut() else {
            return;
        };
        let pressed = menu.pressed.take();
        let target = menu.layout.target(position, &menu.status);
        menu.dirty = true;
        self.request_redraw();
        let Some(button) = pressed.filter(|p| Some(*p) == target) else {
            return;
        };
        self.emit(
            "power-menu",
            &[
                ("state", "selected".into()),
                ("button", button.label().into()),
            ],
        );
        match button.action() {
            None => {
                let effects = self.life.screen.cancel_menu(Instant::now());
                self.apply_effects(effects);
            }
            Some(action) => self.send_power_action(action),
        }
    }

    fn send_power_action(&mut self, action: PowerAction) {
        if let Some(menu) = self.life.menu.as_mut() {
            menu.status = MenuStatus::Working(action);
            menu.dirty = true;
        }
        let socket = self.life.core_socket.clone();
        let tx = self.life.power_tx.clone();
        let ping = self.life.ping.clone();
        let spawned = std::thread::Builder::new()
            .name("power-action".to_owned())
            .spawn(move || {
                let result = ferrix_protocol::command(
                    &socket,
                    HardwareCommand::PowerAction { action },
                    POWER_COMMAND_TIMEOUT,
                )
                .map(|_| ());
                let _ = tx.send((action, result));
                ping.ping();
            });
        if let Err(error) = spawned {
            let _ = self
                .life
                .power_tx
                .send((action, Err(format!("cannot start request: {error}"))));
        }
        self.request_redraw();
    }

    /// Ping callback: policy changes and power command results.
    pub(super) fn lifecycle_wake(&mut self) {
        let mut feed = Vec::new();
        if let Some(rx) = &self.life.policy_feed {
            while let Ok(item) = rx.try_recv() {
                feed.push(item);
            }
        }
        for item in feed {
            match item {
                PolicyFeed::Policy(policy) => self.apply_policy(policy),
                PolicyFeed::Disconnected { reason } => {
                    // Keep enforcing the last policy; say it may be stale.
                    self.emit(
                        "idle-policy",
                        &[("status", "core-lost".into()), ("reason", reason.into())],
                    );
                }
            }
        }
        while let Ok((action, result)) = self.life.power_rx.try_recv() {
            let (status, reason) = match &result {
                Ok(()) => (MenuStatus::Accepted(action), None),
                Err(reason) => (MenuStatus::Failed(action, reason.clone()), Some(reason)),
            };
            self.emit(
                "power-menu",
                &[
                    ("state", status.label().into()),
                    ("action", action.name().into()),
                    ("reason", reason.cloned().map_or(Value::Null, Value::from)),
                ],
            );
            if let Some(menu) = self.life.menu.as_mut() {
                menu.status = status;
                menu.dirty = true;
            }
            self.request_redraw();
        }
    }

    fn apply_policy(&mut self, policy: ScreenPolicy) {
        let effective = policy.effective_idle_timeout_secs();
        let old = self.life.policy.as_ref();
        let changed = old.is_none_or(|old| old.effective_idle_timeout_secs() != effective);
        // A change of the suspend half alone is not an idle-policy change.
        let suspend_only = old.is_some_and(|old| {
            old.idle_timeout_secs == policy.idle_timeout_secs
                && old.inhibitors == policy.inhibitors
                && old.suspend != policy.suspend
        });
        let resumed = old.is_some_and(|old| policy.suspend.cycles > old.suspend.cycles);
        if changed {
            self.life.screen.set_idle_timeout(
                effective.map(|s| Duration::from_secs(u64::from(s))),
                Instant::now(),
            );
        }
        if !suspend_only {
            let reasons: Vec<&str> = policy.screen_reasons().iter().map(|r| r.name()).collect();
            self.emit(
                "idle-policy",
                &[
                    ("status", "applied".into()),
                    ("idle_timeout_secs", policy.idle_timeout_secs.into()),
                    ("inhibited", Value::Bool(policy.inhibited())),
                    ("reasons", reasons.join(",").into()),
                    ("effective_secs", effective.into()),
                ],
            );
        }
        // A restarted core has no report; re-send whenever it disagrees.
        if policy.suspend.screen_off != self.life.screen_off()
            && policy.suspend.phase != ferrix_hardware_api::SuspendPhase::Suspending
        {
            self.life.report_screen();
        }
        if resumed {
            self.resumed(&policy.suspend);
        }
        self.life.policy = Some(policy);
    }

    /// The core finished a suspend cycle. The screen state is the shell's
    /// own: after a background wake (RTC, radio) it stays off and the DPMS
    /// off is re-asserted (the panel may come back lit from resume); when
    /// it is on (a power key press during entry, or one that woke the
    /// device and arrived first) the wake logic runs again: the output is
    /// restored, the front window re-enters it (`wl_surface.enter`, so the
    /// Waydroid hwcomposer wakes), and the scene is redrawn. A power key
    /// press that woke the device reaches `power_key` as an ordinary press
    /// (gpio-keys reports wakeup keys on resume) and wakes the screen there.
    fn resumed(&mut self, status: &SuspendStatus) {
        let action = match self.life.screen.screen() {
            Screen::Off => {
                self.life.pending_display = Some(Screen::Off);
                self.life.wake_frame_pending = false;
                "reassert-off"
            }
            Screen::On => {
                self.life.screen.activity(Instant::now());
                self.life.pending_display = Some(Screen::On);
                self.life.wake_frame_pending = true;
                self.refocus();
                self.request_full_redraw();
                "rewake"
            }
        };
        let last = status.last.as_ref();
        self.emit(
            "resume",
            &[
                ("screen", self.life.screen.screen().label().into()),
                ("action", action.into()),
                ("cycles", Value::Int(i64::from(status.cycles))),
                (
                    "outcome",
                    last.map_or(Value::Null, |r| r.outcome.name().into()),
                ),
                ("wake", last.map_or(Value::Null, |r| r.wake.name().into())),
            ],
        );
    }

    /// Redraw the menu's buffer when its state changed; the backend calls
    /// this before collecting render elements.
    pub fn prepare_menu(&mut self) {
        let Some(menu) = self.life.menu.as_mut() else {
            return;
        };
        if menu.dirty || menu.buffer.is_none() {
            if self.life.ty.is_none() {
                match ferrix_ui::draw::Type::load() {
                    Ok(ty) => self.life.ty = Some(ty),
                    Err(error) => {
                        tracing::warn!("power menu font: {error}");
                        return;
                    }
                }
            }
            let Some(ty) = self.life.ty.as_ref() else {
                return;
            };
            let canvas = power_menu::draw(ty, &menu.layout, &menu.status, menu.pressed);
            let (w, h) = menu.layout.size;
            menu.buffer = Some(MemoryRenderBuffer::from_slice(
                &canvas.xrgb_bytes(),
                Fourcc::Xrgb8888,
                (w as i32, h as i32),
                1,
                Transform::Normal,
                None,
            ));
            menu.dirty = false;
        }
    }

    /// The prepared menu buffer, when the menu is open.
    pub fn menu_buffer(&self) -> Option<&MemoryRenderBuffer> {
        self.life.menu.as_ref().and_then(|m| m.buffer.as_ref())
    }

    fn request_full_redraw(&mut self) {
        self.full_redraw_requested = true;
        self.request_redraw();
    }
}
