//! Target-independent interaction model shared by pointer and touch input.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use ferrix_app_api::AppId;
use ferrix_ui::draw::Rect;

use crate::catalog::Tile;
use crate::status::{NetworkSummary, TelephonySummary};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Launcher,
    /// Illustrative in-process page of a non-launchable tile (by index).
    Reference(usize),
    QuickSettings,
    Notifications,
}

impl Page {
    /// Stable name for evidence records.
    pub fn name(self) -> String {
        match self {
            Page::Launcher => "Launcher".to_owned(),
            Page::Reference(index) => format!("Reference({index})"),
            Page::QuickSettings => "QuickSettings".to_owned(),
            Page::Notifications => "Notifications".to_owned(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Status,
    Battery,
    Network,
    /// A launcher tile by index into `UiState::tiles`.
    App(usize),
    Back,
    /// The bottom gesture zone. Drawn as the pill; the compositor owns the
    /// gesture, so the launcher never acts on it.
    Home,
    Wifi,
    Bluetooth,
    Brightness,
    ClearNotifications,
}

/// Product hardware changes requested by the UI. The app sends these through
/// ferrix-core; the authoritative state still comes back through its event
/// subscription.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemAction {
    WifiEnabled(bool),
    BluetoothEnabled(bool),
    Brightness(u8),
}

/// Why an application notice is shown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NoticeKind {
    /// The process died (signal or non-zero exit).
    #[default]
    Crashed,
    /// `ferrix-appd` refused or failed the launch (typed `AppError`).
    LaunchFailed,
}

/// A low-noise notice shown above the grid after an application crashed or
/// could not start. Dismissed by the next tap or by a successful relaunch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CrashNotice {
    pub app: AppId,
    pub name: String,
    pub reason: String,
    pub kind: NoticeKind,
}

impl CrashNotice {
    pub fn crashed(app: AppId, name: String, reason: String) -> Self {
        Self {
            app,
            name,
            reason,
            kind: NoticeKind::Crashed,
        }
    }

    pub fn launch_failed(app: AppId, name: String, reason: String) -> Self {
        Self {
            app,
            name,
            reason,
            kind: NoticeKind::LaunchFailed,
        }
    }

    /// The one line the banner shows: name the thing, then the state.
    pub fn text(&self) -> String {
        match self.kind {
            NoticeKind::Crashed => format!("{} \u{b7} crashed \u{b7} {}", self.name, self.reason),
            NoticeKind::LaunchFailed => {
                format!("{} could not start \u{b7} {}", self.name, self.reason)
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UiState {
    pub page: Page,
    pub pressed: Option<Target>,
    pub wifi_on: bool,
    pub bluetooth_on: bool,
    pub brightness: u8,
    pub notification_visible: bool,
    /// The line under the clock (`ferrix · <target> · mock`); the session
    /// owner sets the target it actually connected to.
    pub identity: String,
    pub tiles: Vec<Tile>,
    /// Status-bar summaries with their source (service or hardware-api).
    pub tele: TelephonySummary,
    pub net: NetworkSummary,
    /// Launch requests for launchable tiles; the session owner drains these
    /// after input and sends them to `ferrix_appd`.
    pub pending_launch: Vec<AppId>,
    /// Queued hardware requests; the session owner drains these after input.
    pub pending_system: Vec<SystemAction>,
    pub crash_notice: Option<CrashNotice>,
}

impl UiState {
    pub fn new(wifi_on: bool, bluetooth_on: bool, tiles: Vec<Tile>) -> Self {
        Self {
            page: Page::Launcher,
            pressed: None,
            wifi_on,
            bluetooth_on,
            brightness: 72,
            notification_visible: true,
            identity: crate::screen::identity_line(ferrix_session::DEFAULT_TARGET_ID),
            tiles,
            tele: TelephonySummary::default(),
            net: NetworkSummary::default(),
            pending_launch: Vec::new(),
            pending_system: Vec::new(),
            crash_notice: None,
        }
    }

    /// Replaces the catalog, keeping the current page valid.
    pub fn set_tiles(&mut self, tiles: Vec<Tile>) {
        self.tiles = tiles;
        if let Page::Reference(index) = self.page
            && index >= self.tiles.len()
        {
            self.page = Page::Launcher;
        }
    }

    pub fn tile_index(&self, id: &AppId) -> Option<usize> {
        self.tiles.iter().position(|tile| tile.id == *id)
    }

    fn home(&mut self) {
        self.page = Page::Launcher;
    }

    fn back(&mut self) {
        match self.page {
            Page::Launcher => {}
            Page::Reference(_) | Page::QuickSettings | Page::Notifications => self.home(),
        }
    }

    fn activate(&mut self, target: Target) {
        match target {
            Target::Status => self.page = Page::QuickSettings,
            Target::Battery | Target::Network => self.page = Page::QuickSettings,
            Target::App(index) => {
                if let Some(tile) = self.tiles.get_mut(index) {
                    if tile.launchable {
                        tile.starting = true;
                        self.pending_launch.push(tile.id.clone());
                    } else {
                        self.page = Page::Reference(index);
                    }
                }
            }
            Target::Back => self.back(),
            Target::Home => {}
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
        let target = hit_test(state, position, size);
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

        let tap = distance(contact.start, contact.current) < tap_slop(size);
        // Any tap dismisses a crash notice; the tap still acts normally.
        if tap {
            state.crash_notice = None;
        }
        if contact.start.1 <= h * 0.08 && dy > swipe && dy.abs() > dx.abs() {
            state.page = Page::Notifications;
        } else if ((contact.start.0 <= w * 0.08 && dx > swipe)
            || (contact.start.0 >= w * 0.92 && -dx > swipe))
            && dx.abs() > dy.abs()
        {
            state.back();
        } else if tap
            && let Some(target) = contact.target
            && hit_test(state, contact.current, size) == Some(target)
        {
            state.activate(target);
        }
        state.pressed = None;
        true
    }

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

/// Scale a 1080-wide design length (multiply first so narrow panels never
/// truncate to zero).
fn scale(v: i32, basis: i32) -> i32 {
    v * basis / 1080
}

/// Launcher tile rectangles, shared by rendering and hit testing. Three
/// columns; rows grow with the catalog. Portrait keeps the original geometry
/// (48px margin, 24px gap, grid top at one panel width); landscape places
/// the grid right of the cards and fits the rows into the free height.
pub fn tile_rects(size: (u32, u32), count: usize) -> Vec<Rect> {
    let (w, h) = (size.0 as i32, size.1 as i32);
    let basis = w.min(h);
    let s = |v: i32| scale(v, basis);
    let gap = s(24);
    let rows = count.div_ceil(3).max(1) as i32;
    let (left, top, tile) = if w > h {
        let left = s(940);
        let top = s(120);
        let tile =
            ((w - left - s(40) - 2 * gap) / 3).min((h - top - s(90) - (rows - 1) * gap) / rows);
        (left, top, tile)
    } else {
        let margin = s(48);
        (margin, w, (w - 2 * margin - 2 * gap) / 3)
    };
    (0..count)
        .map(|index| {
            let col = (index % 3) as i32;
            let row = (index / 3) as i32;
            Rect {
                x: left + col * (tile + gap),
                y: top + row * (tile + gap),
                w: tile,
                h: tile,
            }
        })
        .collect()
}

fn contains(r: Rect, position: (f64, f64)) -> bool {
    let (x, y) = position;
    x >= f64::from(r.x)
        && x <= f64::from(r.x + r.w)
        && y >= f64::from(r.y)
        && y <= f64::from(r.y + r.h)
}

pub fn hit_test(state: &UiState, position: (f64, f64), size: (u32, u32)) -> Option<Target> {
    let (x, y) = position;
    let (w, h) = (f64::from(size.0), f64::from(size.1));
    if y >= h * 0.92 {
        return Some(Target::Home);
    }
    match state.page {
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
            } else if y >= w * 620.0 / 1080.0 && y <= w * 920.0 / 1080.0 {
                if x >= w * 48.0 / 1080.0 && x <= w * 528.0 / 1080.0 {
                    return Some(Target::Battery);
                }
                if x >= w * 552.0 / 1080.0 && x <= w * 1032.0 / 1080.0 {
                    return Some(Target::Network);
                }
            }
            tile_rects(size, state.tiles.len())
                .into_iter()
                .position(|rect| contains(rect, position))
                .map(Target::App)
        }
        Page::Reference(_) => (x <= w * 0.22 && y <= h * 0.12).then_some(Target::Back),
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
    use crate::test_tiles;

    const SIZE: (u32, u32) = (1080, 2340);

    fn state() -> UiState {
        UiState::new(true, true, test_tiles())
    }

    fn tile_center(size: (u32, u32), count: usize, index: usize) -> (f64, f64) {
        let r = tile_rects(size, count)[index];
        (f64::from(r.x + r.w / 2), f64::from(r.y + r.h / 2))
    }

    #[test]
    fn launchable_tiles_queue_a_launch_and_reference_tiles_open_pages() {
        let mut state = state();
        let mut input = Input::default();
        let terminal = state
            .tile_index(&AppId::new("org.ferrix.terminal").unwrap())
            .expect("terminal tile");
        assert!(
            state.tiles[..=terminal].iter().all(|t| t.launchable),
            "launchable apps come first"
        );
        let n = state.tiles.len();
        assert!(input.down(0, tile_center(SIZE, n, terminal), &mut state, SIZE));
        assert_eq!(state.pressed, Some(Target::App(terminal)));
        assert!(input.up(0, &mut state, SIZE));
        assert_eq!(state.page, Page::Launcher, "launching leaves the launcher");
        assert_eq!(
            state.pending_launch,
            vec![AppId::new("org.ferrix.terminal").unwrap()]
        );
        assert!(state.tiles[terminal].starting);

        // Settings is a real app now: its tile launches instead of opening a page.
        let settings = state
            .tile_index(&AppId::new("org.ferrix.settings").unwrap())
            .expect("settings tile");
        assert!(state.tiles[settings].launchable);
        input.down(0, tile_center(SIZE, n, settings), &mut state, SIZE);
        input.up(0, &mut state, SIZE);
        assert_eq!(state.page, Page::Launcher);
        assert_eq!(state.pending_launch.len(), 2);
        let camera = state
            .tile_index(&AppId::new("org.ferrix.camera").unwrap())
            .expect("camera tile");
        input.down(0, tile_center(SIZE, n, camera), &mut state, SIZE);
        input.up(0, &mut state, SIZE);
        assert_eq!(state.page, Page::Reference(camera));
        assert_eq!(
            state.pending_launch.len(),
            2,
            "reference pages do not launch"
        );
    }

    #[test]
    fn seven_tiles_fill_three_rows_that_stay_above_the_footer() {
        let rects = tile_rects(SIZE, 7);
        assert_eq!(rects.len(), 7);
        assert_eq!(rects[0].y, 1080);
        assert_eq!(rects[3].y, rects[0].y + rects[0].h + 24);
        assert_eq!(rects[6].x, rects[0].x);
        assert!(rects[6].y + rects[6].h < 2340 - 150, "{:?}", rects[6]);
        assert!(tile_rects(SIZE, 0).is_empty());
    }

    #[test]
    fn bottom_zone_is_the_compositors_and_does_nothing_here() {
        let mut state = state();
        state.page = Page::QuickSettings;
        let mut input = Input::default();
        input.down(0, (540.0, 2290.0), &mut state, SIZE);
        assert_eq!(state.pressed, Some(Target::Home));
        input.motion(0, (540.0, 1900.0), &mut state, SIZE);
        input.up(0, &mut state, SIZE);
        assert_eq!(state.page, Page::QuickSettings, "no in-process home action");
        assert_eq!(state.pressed, None);
    }

    #[test]
    fn top_gestures_and_quick_controls_are_interactive() {
        let mut state = state();
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
    fn either_edge_swipe_navigates_back() {
        let mut state = state();
        state.page = Page::Reference(3);
        let mut input = Input::default();
        input.down(0, (20.0, 900.0), &mut state, SIZE);
        input.motion(0, (300.0, 900.0), &mut state, SIZE);
        input.up(0, &mut state, SIZE);
        assert_eq!(state.page, Page::Launcher);

        state.page = Page::Reference(3);
        input.down(0, (1060.0, 900.0), &mut state, SIZE);
        input.motion(0, (780.0, 900.0), &mut state, SIZE);
        input.up(0, &mut state, SIZE);
        assert_eq!(state.page, Page::Launcher);
    }

    #[test]
    fn cards_open_controls_and_notifications_clear_in_place() {
        let mut state = state();
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
        let mut state = state();
        let mut input = Input::default();
        let landscape = (2340, 1080);
        let rects = tile_rects(landscape, state.tiles.len());
        assert!(rects.iter().all(|r| r.y + r.h <= 1080 - 90), "{rects:?}");
        assert!(
            contains(rects[0], (1150.0, 300.0)),
            "first tile keeps its place: {:?}",
            rects[0]
        );
        input.down(0, (1150.0, 300.0), &mut state, landscape);
        input.up(0, &mut state, landscape);
        assert_eq!(state.pending_launch.len(), 1, "first tile is launchable");
        let last = rects.len() - 1;
        let center = (
            f64::from(rects[last].x + rects[last].w / 2),
            f64::from(rects[last].y + rects[last].h / 2),
        );
        input.down(0, center, &mut state, landscape);
        input.up(0, &mut state, landscape);
        assert_eq!(state.page, Page::Reference(last));
    }

    #[test]
    fn a_tap_anywhere_dismisses_the_crash_notice() {
        let mut state = state();
        state.crash_notice = Some(CrashNotice::crashed(
            AppId::new("org.ferrix.terminal").unwrap(),
            "Terminal".to_owned(),
            "signal 11".to_owned(),
        ));
        let mut input = Input::default();
        input.down(0, (540.0, 1000.0), &mut state, SIZE);
        input.up(0, &mut state, SIZE);
        assert_eq!(state.crash_notice, None);
        assert_eq!(state.page, Page::Launcher);
    }

    #[test]
    fn notices_name_the_app_then_the_state() {
        let app = AppId::new("org.ferrix.browser").unwrap();
        let failed = CrashNotice::launch_failed(
            app.clone(),
            "Browser".to_owned(),
            "no android runtime is available".to_owned(),
        );
        assert_eq!(
            failed.text(),
            "Browser could not start \u{b7} no android runtime is available"
        );
        let crashed = CrashNotice::crashed(app, "Browser".to_owned(), "signal 11".to_owned());
        assert_eq!(crashed.text(), "Browser \u{b7} crashed \u{b7} signal 11");
        // A failed launch never leaves a tile stuck in `starting`.
        let mut state = state();
        let index = state
            .tile_index(&AppId::new("org.ferrix.terminal").unwrap())
            .unwrap();
        state.tiles[index].starting = true;
        state.crash_notice = Some(failed);
        assert_eq!(
            state.crash_notice.as_ref().map(|n| n.kind),
            Some(NoticeKind::LaunchFailed)
        );
    }

    #[test]
    fn replacing_the_catalog_keeps_the_page_valid() {
        let mut state = state();
        state.page = Page::Reference(6);
        let mut fewer = test_tiles();
        fewer.truncate(2);
        state.set_tiles(fewer);
        assert_eq!(state.page, Page::Launcher);
    }
}
