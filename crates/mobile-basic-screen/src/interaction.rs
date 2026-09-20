//! Target-independent interaction model shared by pointer and touch input.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use crate::osk::{self, Key, OskState};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppId {
    Settings,
    Messages,
    Terminal,
    Dialer,
    Camera,
    Files,
}

impl AppId {
    pub const ALL: [Self; 6] = [
        Self::Settings,
        Self::Messages,
        Self::Terminal,
        Self::Dialer,
        Self::Camera,
        Self::Files,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Settings => "Settings",
            Self::Messages => "Messages",
            Self::Terminal => "Terminal",
            Self::Dialer => "Dialer",
            Self::Camera => "Camera",
            Self::Files => "Files",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Launcher,
    App(AppId),
    QuickSettings,
    Notifications,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Status,
    Battery,
    Network,
    App(AppId),
    Back,
    Home,
    Wifi,
    Bluetooth,
    Brightness,
    ClearNotifications,
    /// An on-screen keyboard key on the Terminal page.
    Key(Key),
    /// The terminal grid itself: vertical drags scroll history.
    TermArea,
}

/// Terminal work produced by the interaction model for the session owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TermAction {
    /// Bytes for the shell's stdin.
    Input(Vec<u8>),
    /// Viewport scroll in lines; positive moves into history.
    Scroll(i32),
}

/// Product hardware changes requested by the UI. The app sends these through
/// mobile-core; the authoritative state still comes back through its event
/// subscription.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemAction {
    WifiEnabled(bool),
    BluetoothEnabled(bool),
    Brightness(u8),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UiState {
    pub page: Page,
    pub pressed: Option<Target>,
    pub wifi_on: bool,
    pub bluetooth_on: bool,
    pub brightness: u8,
    pub notification_visible: bool,
    pub osk: OskState,
    /// Queued terminal actions; the session owner drains these after input.
    pub pending: Vec<TermAction>,
    /// Queued hardware requests; the session owner drains these after input.
    pub pending_system: Vec<SystemAction>,
    /// Mirror of the terminal's DECCKM mode so arrows encode correctly.
    pub term_app_cursor: bool,
}

impl UiState {
    pub fn new(wifi_on: bool, bluetooth_on: bool) -> Self {
        Self {
            page: Page::Launcher,
            pressed: None,
            wifi_on,
            bluetooth_on,
            brightness: 72,
            notification_visible: true,
            osk: OskState::default(),
            pending: Vec::new(),
            pending_system: Vec::new(),
            term_app_cursor: false,
        }
    }

    fn home(&mut self) {
        self.page = Page::Launcher;
    }

    fn back(&mut self) {
        match self.page {
            Page::Launcher => {}
            Page::App(_) | Page::QuickSettings | Page::Notifications => self.home(),
        }
    }

    fn activate(&mut self, target: Target) {
        match target {
            Target::Status => self.page = Page::QuickSettings,
            Target::Battery | Target::Network => self.page = Page::QuickSettings,
            Target::App(app) => self.page = Page::App(app),
            Target::Back => self.back(),
            Target::Home => self.home(),
            Target::Wifi => {
                self.wifi_on = !self.wifi_on;
                self.pending_system
                    .push(SystemAction::WifiEnabled(self.wifi_on));
            }
            Target::Bluetooth => {
                self.bluetooth_on = !self.bluetooth_on;
                self.pending_system
                    .push(SystemAction::BluetoothEnabled(self.bluetooth_on));
            }
            Target::Brightness => {}
            Target::ClearNotifications => self.notification_visible = false,
            Target::Key(key) => {
                if let Some(bytes) = self.osk.press(key, self.term_app_cursor) {
                    self.pending.push(TermAction::Input(bytes));
                }
            }
            Target::TermArea => {}
        }
    }
}

/// Terminal scroll step in panel pixels: one text line on the 1080 panel.
fn scroll_line(size: (u32, u32)) -> f64 {
    f64::from(size.0) * 39.0 / 1080.0
}

#[derive(Clone, Copy, Debug)]
struct Contact {
    id: i32,
    start: (f64, f64),
    current: (f64, f64),
    /// Last position at which a scroll step was emitted.
    anchor: (f64, f64),
    target: Option<Target>,
}

#[derive(Default)]
pub struct Input {
    contact: Option<Contact>,
}

impl Input {
    pub fn down(
        &mut self,
        id: i32,
        position: (f64, f64),
        state: &mut UiState,
        size: (u32, u32),
    ) -> bool {
        if self.contact.is_some() {
            return false;
        }
        let target = hit_test(state, position, size);
        self.contact = Some(Contact {
            id,
            start: position,
            current: position,
            anchor: position,
            target,
        });
        // The grid itself has no press feedback; keys and controls do.
        state.pressed = target.filter(|t| *t != Target::TermArea);
        true
    }

    pub fn motion(
        &mut self,
        id: i32,
        position: (f64, f64),
        state: &mut UiState,
        size: (u32, u32),
    ) -> bool {
        let Some(contact) = self.contact.as_mut().filter(|contact| contact.id == id) else {
            return false;
        };
        contact.current = position;
        if contact.target == Some(Target::Brightness) {
            let old = state.brightness;
            state.brightness = brightness_at(position.0, size.0);
            return old != state.brightness;
        }
        if contact.target == Some(Target::TermArea) {
            // Dragging down reveals history, one step per text line.
            let step = scroll_line(size);
            let lines = ((position.1 - contact.anchor.1) / step).trunc() as i32;
            if lines != 0 {
                contact.anchor.1 += f64::from(lines) * step;
                state.pending.push(TermAction::Scroll(lines));
                return true;
            }
            return false;
        }
        let target = hit_test(state, position, size);
        let pressed = if distance(contact.start, position) < tap_slop(size) {
            contact
                .target
                .filter(|candidate| Some(*candidate) == target)
        } else {
            None
        };
        let changed = state.pressed != pressed;
        state.pressed = pressed;
        changed
    }

    pub fn up(&mut self, id: i32, state: &mut UiState, size: (u32, u32)) -> bool {
        let Some(contact) = self.contact.filter(|contact| contact.id == id) else {
            return false;
        };
        self.contact = None;
        let (w, h) = (f64::from(size.0), f64::from(size.1));
        let dx = contact.current.0 - contact.start.0;
        let dy = contact.current.1 - contact.start.1;
        let swipe = w.min(h) * 0.12;

        if contact.target == Some(Target::Brightness) {
            state.brightness = brightness_at(contact.current.0, size.0);
            state
                .pending_system
                .push(SystemAction::Brightness(state.brightness));
            state.pressed = None;
            return true;
        }

        if contact.start.1 <= h * 0.08 && dy > swipe && dy.abs() > dx.abs() {
            state.page = Page::Notifications;
        } else if contact.start.1 >= h * 0.90 && -dy > swipe && dy.abs() > dx.abs() {
            state.home();
        } else if contact.start.0 <= w * 0.08 && dx > swipe && dx.abs() > dy.abs() {
            state.back();
        } else if distance(contact.start, contact.current) < tap_slop(size)
            && let Some(target) = contact.target
            && hit_test(state, contact.current, size) == Some(target)
        {
            state.activate(target);
        }
        state.pressed = None;
        true
    }

    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub fn cancel(&mut self, state: &mut UiState) -> bool {
        self.contact.take().is_some() || state.pressed.take().is_some()
    }
}

fn distance(a: (f64, f64), b: (f64, f64)) -> f64 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

fn tap_slop(size: (u32, u32)) -> f64 {
    f64::from(size.0.min(size.1)) * 0.035
}

fn brightness_at(x: f64, width: u32) -> u8 {
    let w = f64::from(width);
    ((x.clamp(w * 0.12, w * 0.88) - w * 0.12) * 100.0 / (w * 0.76)).round() as u8
}

pub fn hit_test(state: &UiState, position: (f64, f64), size: (u32, u32)) -> Option<Target> {
    let (x, y) = position;
    let (w, h) = (f64::from(size.0), f64::from(size.1));
    if y >= h * 0.92 {
        return Some(Target::Home);
    }
    match state.page {
        Page::App(AppId::Terminal) => {
            let area = osk::terminal_area(size);
            if x <= w * 0.22 && y < f64::from(area.y) {
                return Some(Target::Back);
            }
            if let Some((_, key)) = osk::keys(size, &state.osk)
                .into_iter()
                .find(|(rect, _)| osk::contains(*rect, position))
            {
                return Some(Target::Key(key));
            }
            osk::contains(area, position).then_some(Target::TermArea)
        }
        Page::Launcher => {
            if y <= h * 0.08 {
                return Some(Target::Status);
            }
            if w > h {
                let s = h / 1080.0;
                if y >= 430.0 * s && y <= 690.0 * s {
                    if x >= 40.0 * s && x <= 440.0 * s {
                        return Some(Target::Battery);
                    }
                    if x >= 460.0 * s && x <= 860.0 * s {
                        return Some(Target::Network);
                    }
                }
                let gap = 24.0 * s;
                let left = 940.0 * s;
                let top = 120.0 * s;
                let tile =
                    ((w - left - 40.0 * s - 2.0 * gap) / 3.0).min((h - top - 90.0 * s - gap) / 2.0);
                for (index, app) in AppId::ALL.into_iter().enumerate() {
                    let col = (index % 3) as f64;
                    let row = (index / 3) as f64;
                    let tile_x = left + col * (tile + gap);
                    let tile_y = top + row * (tile + gap);
                    if x >= tile_x && x <= tile_x + tile && y >= tile_y && y <= tile_y + tile {
                        return Some(Target::App(app));
                    }
                }
                return None;
            }
            if y >= w * 620.0 / 1080.0 && y <= w * 920.0 / 1080.0 {
                if x >= w * 48.0 / 1080.0 && x <= w * 528.0 / 1080.0 {
                    return Some(Target::Battery);
                }
                if x >= w * 552.0 / 1080.0 && x <= w * 1032.0 / 1080.0 {
                    return Some(Target::Network);
                }
            }
            let margin = w * 48.0 / 1080.0;
            let gap = w * 24.0 / 1080.0;
            let tile = (w - 2.0 * margin - 2.0 * gap) / 3.0;
            let top = w;
            for (index, app) in AppId::ALL.into_iter().enumerate() {
                let col = (index % 3) as f64;
                let row = (index / 3) as f64;
                let left = margin + col * (tile + gap);
                let upper = top + row * (tile + gap);
                if x >= left && x <= left + tile && y >= upper && y <= upper + tile {
                    return Some(Target::App(app));
                }
            }
            None
        }
        Page::App(_) => (x <= w * 0.22 && y <= h * 0.12).then_some(Target::Back),
        Page::QuickSettings => {
            if x <= w * 0.22 && y <= h * 0.12 {
                Some(Target::Back)
            } else if y >= h * 0.20 && y <= h * 0.34 && x >= w * 0.08 && x <= w * 0.48 {
                Some(Target::Wifi)
            } else if y >= h * 0.20 && y <= h * 0.34 && x >= w * 0.52 && x <= w * 0.92 {
                Some(Target::Bluetooth)
            } else if y >= h * 0.42 && y <= h * 0.55 && x >= w * 0.08 && x <= w * 0.92 {
                Some(Target::Brightness)
            } else {
                None
            }
        }
        Page::Notifications => {
            if x <= w * 0.22 && y <= h * 0.12 {
                Some(Target::Back)
            } else if y >= h * 0.72 && y <= h * 0.82 && x >= w * 0.25 && x <= w * 0.75 {
                Some(Target::ClearNotifications)
            } else {
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: (u32, u32) = (1080, 2340);

    #[test]
    fn launcher_tiles_open_and_home_gesture_returns() {
        let mut state = UiState::new(true, true);
        let mut input = Input::default();
        assert!(input.down(0, (180.0, 1230.0), &mut state, SIZE));
        assert!(input.up(0, &mut state, SIZE));
        assert_eq!(state.page, Page::App(AppId::Settings));
        input.down(0, (540.0, 2290.0), &mut state, SIZE);
        input.motion(0, (540.0, 1900.0), &mut state, SIZE);
        input.up(0, &mut state, SIZE);
        assert_eq!(state.page, Page::Launcher);
    }

    #[test]
    fn top_gestures_and_quick_controls_are_interactive() {
        let mut state = UiState::new(true, true);
        let mut input = Input::default();
        input.down(0, (500.0, 20.0), &mut state, SIZE);
        input.motion(0, (500.0, 400.0), &mut state, SIZE);
        input.up(0, &mut state, SIZE);
        assert_eq!(state.page, Page::Notifications);

        state.page = Page::QuickSettings;
        input.down(0, (250.0, 600.0), &mut state, SIZE);
        input.up(0, &mut state, SIZE);
        assert!(!state.wifi_on);
        assert_eq!(state.pending_system, vec![SystemAction::WifiEnabled(false)]);
        state.pending_system.clear();
        input.down(0, (540.0, 1130.0), &mut state, SIZE);
        input.motion(0, (900.0, 1130.0), &mut state, SIZE);
        input.up(0, &mut state, SIZE);
        assert!(state.brightness > 90);
        assert_eq!(
            state.pending_system,
            vec![SystemAction::Brightness(state.brightness)]
        );
    }

    #[test]
    fn terminal_keys_queue_shell_input_and_drags_scroll_history() {
        let mut state = UiState::new(true, true);
        let mut input = Input::default();
        // Tap the Terminal tile (third column of the first launcher row).
        input.down(0, (852.0, 1236.0), &mut state, SIZE);
        input.up(0, &mut state, SIZE);
        assert_eq!(state.page, Page::App(AppId::Terminal));

        let keys = osk::keys(SIZE, &state.osk);
        let center = |wanted: Key| {
            let (r, _) = keys
                .iter()
                .find(|(_, k)| *k == wanted)
                .expect("key on layout");
            (f64::from(r.x + r.w / 2), f64::from(r.y + r.h / 2))
        };
        for key in [Key::Char('l'), Key::Char('s'), Key::Enter] {
            input.down(0, center(key), &mut state, SIZE);
            assert_eq!(state.pressed, Some(Target::Key(key)));
            input.up(0, &mut state, SIZE);
        }
        assert_eq!(
            state.pending,
            vec![
                TermAction::Input(b"l".to_vec()),
                TermAction::Input(b"s".to_vec()),
                TermAction::Input(b"\r".to_vec()),
            ]
        );
        state.pending.clear();

        // Ctrl is sticky for one key: ctrl, c -> ETX.
        input.down(0, center(Key::Ctrl), &mut state, SIZE);
        input.up(0, &mut state, SIZE);
        input.down(0, center(Key::Char('c')), &mut state, SIZE);
        input.up(0, &mut state, SIZE);
        assert_eq!(state.pending, vec![TermAction::Input(vec![0x03])]);
        state.pending.clear();

        // Dragging down inside the grid scrolls into history without pressing.
        let area = osk::terminal_area(SIZE);
        let x = f64::from(area.x + area.w / 2);
        let y = f64::from(area.y + area.h / 2);
        input.down(0, (x, y), &mut state, SIZE);
        assert_eq!(state.pressed, None);
        input.motion(0, (x, y + 200.0), &mut state, SIZE);
        input.up(0, &mut state, SIZE);
        assert_eq!(state.pending, vec![TermAction::Scroll(5)]);
        assert_eq!(state.page, Page::App(AppId::Terminal));

        // Back arrow in the header returns to the launcher; the shell keeps running.
        input.down(0, (80.0, 160.0), &mut state, SIZE);
        input.up(0, &mut state, SIZE);
        assert_eq!(state.page, Page::Launcher);
    }

    #[test]
    fn left_edge_swipe_navigates_back() {
        let mut state = UiState::new(true, true);
        state.page = Page::App(AppId::Files);
        let mut input = Input::default();
        input.down(0, (20.0, 900.0), &mut state, SIZE);
        input.motion(0, (300.0, 900.0), &mut state, SIZE);
        input.up(0, &mut state, SIZE);
        assert_eq!(state.page, Page::Launcher);
    }

    #[test]
    fn cards_open_controls_and_notifications_clear_in_place() {
        let mut state = UiState::new(true, true);
        let mut input = Input::default();
        input.down(0, (200.0, 700.0), &mut state, SIZE);
        input.up(0, &mut state, SIZE);
        assert_eq!(state.page, Page::QuickSettings);

        state.page = Page::Notifications;
        input.down(0, (540.0, 1770.0), &mut state, SIZE);
        input.up(0, &mut state, SIZE);
        assert_eq!(state.page, Page::Notifications);
        assert!(!state.notification_visible);
    }

    #[test]
    fn landscape_launcher_tiles_remain_interactive() {
        let mut state = UiState::new(true, true);
        let mut input = Input::default();
        let landscape = (2340, 1080);
        input.down(0, (1150.0, 300.0), &mut state, landscape);
        input.up(0, &mut state, landscape);
        assert_eq!(state.page, Page::App(AppId::Settings));
    }
}
