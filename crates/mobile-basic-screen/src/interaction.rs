//! Target-independent interaction model shared by pointer and touch input.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UiState {
    pub page: Page,
    pub pressed: Option<Target>,
    pub wifi_on: bool,
    pub bluetooth_on: bool,
    pub brightness: u8,
    pub notification_visible: bool,
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
            Target::Wifi => self.wifi_on = !self.wifi_on,
            Target::Bluetooth => self.bluetooth_on = !self.bluetooth_on,
            Target::Brightness => {}
            Target::ClearNotifications => self.notification_visible = false,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Contact {
    id: i32,
    start: (f64, f64),
    current: (f64, f64),
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
        let target = hit_test(state.page, position, size);
        self.contact = Some(Contact {
            id,
            start: position,
            current: position,
            target,
        });
        state.pressed = target;
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
        let target = hit_test(state.page, position, size);
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

        if contact.start.1 <= h * 0.08 && dy > swipe && dy.abs() > dx.abs() {
            state.page = Page::Notifications;
        } else if contact.start.1 >= h * 0.90 && -dy > swipe && dy.abs() > dx.abs() {
            state.home();
        } else if contact.start.0 <= w * 0.08 && dx > swipe && dx.abs() > dy.abs() {
            state.back();
        } else if distance(contact.start, contact.current) < tap_slop(size)
            && let Some(target) = contact.target
            && hit_test(state.page, contact.current, size) == Some(target)
        {
            if target == Target::Brightness {
                state.brightness = brightness_at(contact.current.0, size.0);
            }
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

pub fn hit_test(page: Page, position: (f64, f64), size: (u32, u32)) -> Option<Target> {
    let (x, y) = position;
    let (w, h) = (f64::from(size.0), f64::from(size.1));
    if y >= h * 0.92 {
        return Some(Target::Home);
    }
    match page {
        Page::Launcher => {
            if y <= h * 0.08 {
                return Some(Target::Status);
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
        input.down(0, (540.0, 1130.0), &mut state, SIZE);
        input.motion(0, (900.0, 1130.0), &mut state, SIZE);
        input.up(0, &mut state, SIZE);
        assert!(state.brightness > 90);
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
}
