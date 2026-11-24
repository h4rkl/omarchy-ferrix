//! Trusted power key, screen blanking, and the idle timeout (pure policy).
//!
//! The compositor intercepts the power key before any client sees it and
//! feeds presses and releases here with a monotonic clock; it also reports
//! every admitted user input as activity and calls [`ScreenLifecycle::tick`]
//! from its event loop. The returned [`Effect`]s are what the compositor
//! does: DPMS the output off or on, open or close the power menu.
//!
//! Rules (deliverables 1 and 2 of plans/todos/04):
//!
//! * short press: screen on -> off; off -> on. With the power menu open it
//!   closes the menu and turns the screen off.
//! * long press ([`LONG_PRESS`], fired while still held): opens the power
//!   menu, waking the screen first when it was off.
//! * idle: with the screen on and no input for the policy's effective idle
//!   timeout (none while an inhibitor is held), the screen turns off.
//! * while off, no input reaches clients and none counts as activity; only
//!   the power key wakes the screen.
//!
//! This module knows nothing about Wayland, Smithay, or DRM, so it builds and
//! tests on every host.

#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::time::{Duration, Instant};

/// Linux evdev `KEY_POWER` (`linux/input-event-codes.h`). QEMU's
/// virtio-keyboard reports it for the `power` qcode; PMIC and gpio-keys power
/// buttons on phones report the same code.
pub const KEY_POWER: u32 = 116;

/// Whether an xkb keycode (evdev code + 8, as libinput and winit report
/// keys to Smithay) is the power key.
pub fn is_power_key(xkb_keycode: u32) -> bool {
    xkb_keycode == KEY_POWER + 8
}

/// Hold time after which a power key press opens the power menu.
pub const LONG_PRESS: Duration = Duration::from_millis(600);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PowerPress {
    Short,
    Long,
}

impl PowerPress {
    pub fn label(self) -> &'static str {
        match self {
            PowerPress::Short => "short",
            PowerPress::Long => "long",
        }
    }
}

/// Short/long press recogniser. Kernel autorepeat (a second press while
/// held) is ignored; a long press fires once, while still held.
#[derive(Clone, Copy, Debug, Default)]
pub struct PowerKey {
    pressed_at: Option<Instant>,
    long_fired: bool,
}

impl PowerKey {
    pub fn press(&mut self, now: Instant) {
        if self.pressed_at.is_none() {
            self.pressed_at = Some(now);
            self.long_fired = false;
        }
    }

    /// The key lifted: a short press, a long one the loop had not polled
    /// yet, or nothing when the long press already fired.
    pub fn release(&mut self, now: Instant) -> Option<PowerPress> {
        let at = self.pressed_at.take()?;
        if self.long_fired {
            None
        } else if now.saturating_duration_since(at) >= LONG_PRESS {
            Some(PowerPress::Long)
        } else {
            Some(PowerPress::Short)
        }
    }

    /// Fires the long press once the key has been held long enough.
    pub fn poll(&mut self, now: Instant) -> Option<PowerPress> {
        let at = self.pressed_at?;
        if !self.long_fired && now.saturating_duration_since(at) >= LONG_PRESS {
            self.long_fired = true;
            Some(PowerPress::Long)
        } else {
            None
        }
    }

    pub fn held(&self) -> bool {
        self.pressed_at.is_some()
    }

    /// Forget a press whose release can never arrive (device removed).
    pub fn cancel(&mut self) {
        self.pressed_at = None;
        self.long_fired = false;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Screen {
    On,
    Off,
}

impl Screen {
    pub fn label(self) -> &'static str {
        match self {
            Screen::On => "on",
            Screen::Off => "off",
        }
    }
}

/// Why the screen changed state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Cause {
    PowerKey,
    Idle,
    /// Woken to show the power menu (long press while off).
    PowerMenu,
}

impl Cause {
    pub fn label(self) -> &'static str {
        match self {
            Cause::PowerKey => "power-key",
            Cause::Idle => "idle",
            Cause::PowerMenu => "power-menu",
        }
    }
}

/// Why the power menu closed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuClose {
    /// The Cancel button. Taps outside the buttons do nothing: the menu is
    /// a fullscreen decision, not a dismissable popup.
    Cancel,
    /// The screen turned off (power key or idle) with the menu open.
    ScreenOff,
}

impl MenuClose {
    pub fn label(self) -> &'static str {
        match self {
            MenuClose::Cancel => "cancel",
            MenuClose::ScreenOff => "screen-off",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Effect {
    /// DPMS the output off; drop input until [`Effect::ScreenOn`].
    ScreenOff(Cause),
    /// Restore the output and redraw.
    ScreenOn(Cause),
    OpenMenu,
    CloseMenu(MenuClose),
}

pub struct ScreenLifecycle {
    screen: Screen,
    menu_open: bool,
    /// Effective timeout: `None` for never or while inhibited.
    idle_timeout: Option<Duration>,
    last_activity: Instant,
    key: PowerKey,
}

impl ScreenLifecycle {
    pub fn new(now: Instant) -> Self {
        Self {
            screen: Screen::On,
            menu_open: false,
            idle_timeout: None,
            last_activity: now,
            key: PowerKey::default(),
        }
    }

    pub fn screen(&self) -> Screen {
        self.screen
    }

    #[cfg(test)]
    pub fn menu_open(&self) -> bool {
        self.menu_open
    }

    /// Whether ordinary input may reach clients (or the power menu).
    pub fn admits_input(&self) -> bool {
        self.screen == Screen::On
    }

    /// A new effective idle timeout (policy change). The idle period starts
    /// over: a timeout shortened below the time already idle must not blank
    /// the screen the instant it is chosen.
    pub fn set_idle_timeout(&mut self, timeout: Option<Duration>, now: Instant) {
        self.idle_timeout = timeout;
        self.last_activity = now;
    }

    /// Admitted user input. Ignored while the screen is off.
    pub fn activity(&mut self, now: Instant) {
        if self.screen == Screen::On {
            self.last_activity = now;
        }
    }

    /// When [`tick`](Self::tick) next has something to decide. (The
    /// backends tick every loop iteration, at most 16 ms apart.)
    #[cfg(test)]
    pub fn deadline(&self) -> Option<Instant> {
        if let Some(at) = self.key.pressed_at
            && !self.key.long_fired
        {
            return Some(at + LONG_PRESS);
        }
        match (self.screen, self.idle_timeout) {
            (Screen::On, Some(timeout)) => Some(self.last_activity + timeout),
            _ => None,
        }
    }

    /// Power key press (`true`) or release. Returns the recognised press, if
    /// any, and its effects.
    pub fn power_key(&mut self, pressed: bool, now: Instant) -> (Option<PowerPress>, Vec<Effect>) {
        if pressed {
            self.key.press(now);
            self.activity(now);
            return (None, Vec::new());
        }
        match self.key.release(now) {
            Some(press) => (Some(press), self.on_press(press, now)),
            None => (None, Vec::new()),
        }
    }

    /// Long-press and idle deadlines.
    pub fn tick(&mut self, now: Instant) -> (Option<PowerPress>, Vec<Effect>) {
        if let Some(press) = self.key.poll(now) {
            return (Some(press), self.on_press(press, now));
        }
        if self.screen == Screen::On
            && !self.key.held()
            && let Some(timeout) = self.idle_timeout
            && now.saturating_duration_since(self.last_activity) >= timeout
        {
            return (None, self.screen_off(Cause::Idle));
        }
        (None, Vec::new())
    }

    /// The user closed the menu (Cancel or a tap outside the actions).
    pub fn cancel_menu(&mut self, now: Instant) -> Vec<Effect> {
        if !self.menu_open {
            return Vec::new();
        }
        self.menu_open = false;
        self.activity(now);
        vec![Effect::CloseMenu(MenuClose::Cancel)]
    }

    /// The input device carrying a held power key went away.
    pub fn cancel_key(&mut self) {
        self.key.cancel();
    }

    fn on_press(&mut self, press: PowerPress, now: Instant) -> Vec<Effect> {
        match (press, self.screen) {
            (PowerPress::Short, Screen::On) => self.screen_off(Cause::PowerKey),
            (PowerPress::Short, Screen::Off) => self.screen_on(Cause::PowerKey, now),
            (PowerPress::Long, Screen::Off) => {
                let mut effects = self.screen_on(Cause::PowerMenu, now);
                effects.extend(self.open_menu(now));
                effects
            }
            (PowerPress::Long, Screen::On) => self.open_menu(now),
        }
    }

    fn open_menu(&mut self, now: Instant) -> Vec<Effect> {
        self.activity(now);
        if self.menu_open {
            return Vec::new();
        }
        self.menu_open = true;
        vec![Effect::OpenMenu]
    }

    fn screen_off(&mut self, cause: Cause) -> Vec<Effect> {
        let mut effects = Vec::new();
        if self.menu_open {
            self.menu_open = false;
            effects.push(Effect::CloseMenu(MenuClose::ScreenOff));
        }
        self.screen = Screen::Off;
        effects.push(Effect::ScreenOff(cause));
        effects
    }

    fn screen_on(&mut self, cause: Cause, now: Instant) -> Vec<Effect> {
        self.screen = Screen::On;
        self.last_activity = now;
        vec![Effect::ScreenOn(cause)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(v: u64) -> Duration {
        Duration::from_millis(v)
    }

    #[test]
    fn the_power_key_is_evdev_116_in_xkb_numbering() {
        assert!(is_power_key(124));
        assert!(!is_power_key(116), "raw evdev is not what Smithay reports");
        assert!(
            !is_power_key(75 + 8),
            "F9 debug rotation is not the power key"
        );
    }

    #[test]
    fn short_press_toggles_the_screen() {
        let t0 = Instant::now();
        let mut l = ScreenLifecycle::new(t0);
        assert_eq!(l.power_key(true, t0), (None, vec![]));
        let (press, effects) = l.power_key(false, t0 + ms(120));
        assert_eq!(press, Some(PowerPress::Short));
        assert_eq!(effects, [Effect::ScreenOff(Cause::PowerKey)]);
        assert!(!l.admits_input(), "nothing reaches clients while off");
        l.power_key(true, t0 + ms(1000));
        let (_, effects) = l.power_key(false, t0 + ms(1100));
        assert_eq!(effects, [Effect::ScreenOn(Cause::PowerKey)]);
        assert!(l.admits_input());
    }

    #[test]
    fn long_press_fires_while_held_and_the_release_is_silent() {
        let t0 = Instant::now();
        let mut l = ScreenLifecycle::new(t0);
        l.power_key(true, t0);
        assert_eq!(l.deadline(), Some(t0 + LONG_PRESS));
        assert_eq!(l.tick(t0 + ms(300)), (None, vec![]));
        let (press, effects) = l.tick(t0 + LONG_PRESS);
        assert_eq!(press, Some(PowerPress::Long));
        assert_eq!(effects, [Effect::OpenMenu]);
        assert!(l.menu_open());
        assert_eq!(l.power_key(false, t0 + ms(1500)), (None, vec![]));
        // A second long press with the menu open changes nothing.
        l.power_key(true, t0 + ms(2000));
        assert_eq!(l.tick(t0 + ms(2000) + LONG_PRESS).1, vec![]);
    }

    #[test]
    fn a_late_poll_still_reports_a_long_press_on_release() {
        let t0 = Instant::now();
        let mut key = PowerKey::default();
        key.press(t0);
        key.press(t0 + ms(400)); // autorepeat: ignored
        assert_eq!(key.release(t0 + ms(900)), Some(PowerPress::Long));
        assert_eq!(key.release(t0 + ms(950)), None, "no press is held");
    }

    #[test]
    fn long_press_while_off_wakes_into_the_menu() {
        let t0 = Instant::now();
        let mut l = ScreenLifecycle::new(t0);
        l.power_key(true, t0);
        l.power_key(false, t0 + ms(50));
        l.power_key(true, t0 + ms(500));
        let (_, effects) = l.tick(t0 + ms(500) + LONG_PRESS);
        assert_eq!(
            effects,
            [Effect::ScreenOn(Cause::PowerMenu), Effect::OpenMenu]
        );
    }

    #[test]
    fn short_press_with_the_menu_open_closes_it_and_blanks() {
        let t0 = Instant::now();
        let mut l = ScreenLifecycle::new(t0);
        l.power_key(true, t0);
        l.tick(t0 + LONG_PRESS);
        l.power_key(false, t0 + ms(700));
        l.power_key(true, t0 + ms(900));
        let (_, effects) = l.power_key(false, t0 + ms(950));
        assert_eq!(
            effects,
            [
                Effect::CloseMenu(MenuClose::ScreenOff),
                Effect::ScreenOff(Cause::PowerKey)
            ]
        );
        assert!(!l.menu_open());
        assert_eq!(l.cancel_menu(t0 + ms(1000)), vec![], "already closed");
    }

    #[test]
    fn idle_timeout_blanks_after_inactivity_only() {
        let t0 = Instant::now();
        let mut l = ScreenLifecycle::new(t0);
        assert_eq!(l.deadline(), None, "never by default");
        assert_eq!(l.tick(t0 + Duration::from_secs(3600)).1, vec![]);
        l.set_idle_timeout(Some(Duration::from_secs(5)), t0);
        l.activity(t0 + ms(4000));
        assert_eq!(l.tick(t0 + ms(8000)).1, vec![], "activity restarted it");
        assert_eq!(l.deadline(), Some(t0 + ms(9000)));
        assert_eq!(l.tick(t0 + ms(9000)).1, [Effect::ScreenOff(Cause::Idle)]);
        // Off: input is not activity and the timer is idle.
        l.activity(t0 + ms(9500));
        assert_eq!(l.deadline(), None);
        // Wake restarts the idle period from the wake.
        l.power_key(true, t0 + ms(20_000));
        l.power_key(false, t0 + ms(20_050));
        assert_eq!(l.deadline(), Some(t0 + ms(25_050)));
    }

    #[test]
    fn an_inhibitor_or_a_held_key_keeps_the_screen_on() {
        let t0 = Instant::now();
        let mut l = ScreenLifecycle::new(t0);
        l.set_idle_timeout(Some(Duration::from_secs(5)), t0);
        // Inhibited: the effective timeout is none.
        l.set_idle_timeout(None, t0 + ms(1000));
        assert_eq!(l.tick(t0 + Duration::from_secs(60)).1, vec![]);
        // Released: the idle period starts over from the change.
        l.set_idle_timeout(Some(Duration::from_secs(5)), t0 + ms(60_000));
        assert_eq!(l.tick(t0 + ms(64_000)).1, vec![]);
        l.power_key(true, t0 + ms(64_500));
        l.cancel_key();
        assert_eq!(l.tick(t0 + ms(70_000)).1, [Effect::ScreenOff(Cause::Idle)]);
    }

    #[test]
    fn idle_with_the_menu_open_closes_it() {
        let t0 = Instant::now();
        let mut l = ScreenLifecycle::new(t0);
        l.set_idle_timeout(Some(Duration::from_secs(5)), t0);
        l.power_key(true, t0);
        l.tick(t0 + LONG_PRESS);
        l.power_key(false, t0 + ms(700));
        assert_eq!(
            l.tick(t0 + ms(6000)).1,
            [
                Effect::CloseMenu(MenuClose::ScreenOff),
                Effect::ScreenOff(Cause::Idle)
            ]
        );
    }
}
