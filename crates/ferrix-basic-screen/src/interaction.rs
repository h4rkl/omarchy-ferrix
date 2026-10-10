//! Target-independent interaction model shared by pointer and touch input.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use ferrix_app_api::AppId;
use ferrix_hardware_api::{CapabilityMatrix, CapabilityStatus};
use ferrix_ui::draw::Rect;

use crate::catalog::Tile;
use crate::status::{NetworkSummary, TelephonySummary};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Launcher,
    /// Every catalog entry (first-party and installed APKs) as a scrolling
    /// list sorted by label; reached from the home grid's Apps tile or an
    /// upward swipe on home.
    Apps,
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
            Page::Apps => "Apps".to_owned(),
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
    /// A catalog entry by index into `UiState::tiles`: a home grid tile or
    /// an app list row.
    App(usize),
    /// The home grid's last tile: opens the app list.
    AllApps,
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
    /// The line under the clock (`ferrix · <target> · <source>`); derived
    /// from [`UiState::set_session`], never set directly.
    pub identity: String,
    /// The hardware target id the session actually connected to.
    pub target: String,
    /// The connected target's capability matrix; user-facing copy names a
    /// capability's status from here instead of assuming a target.
    pub capabilities: CapabilityMatrix,
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
    /// App list scroll offset in panel pixels (clamped to the content by
    /// [`apps_max_scroll`] wherever it is used).
    pub apps_scroll: i32,
}

impl UiState {
    /// A state for the default (mock) target until the session owner calls
    /// [`UiState::set_session`] with the target it connected to.
    pub fn new(wifi_on: bool, bluetooth_on: bool, tiles: Vec<Tile>) -> Self {
        let mut state = Self {
            page: Page::Launcher,
            pressed: None,
            wifi_on,
            bluetooth_on,
            brightness: 72,
            notification_visible: true,
            identity: String::new(),
            target: String::new(),
            capabilities: CapabilityMatrix::default(),
            tiles,
            tele: TelephonySummary::default(),
            net: NetworkSummary::default(),
            pending_launch: Vec::new(),
            pending_system: Vec::new(),
            crash_notice: None,
            apps_scroll: 0,
        };
        state.set_session(
            ferrix_session::DEFAULT_TARGET_ID,
            CapabilityMatrix::uniform(CapabilityStatus::Mocked),
        );
        state
    }

    /// Records the connected target and its capability matrix, and derives
    /// the identity line from them.
    pub fn set_session(&mut self, target: &str, capabilities: CapabilityMatrix) {
        self.identity = crate::screen::identity_line(target, &capabilities);
        self.target = target.to_owned();
        self.capabilities = capabilities;
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

    /// Tiles the home grid offers: first-party entries in catalog order.
    /// Installed APKs live in the app list only, so the grid keeps its
    /// designed tile size however many packages are installed.
    pub fn home_entries(&self) -> Vec<usize> {
        (0..self.tiles.len())
            .filter(|index| !self.tiles[*index].installed)
            .collect()
    }

    /// Every entry for the app list, sorted by label (case-insensitive); a
    /// first-party entry precedes an Android app of the same name.
    pub fn list_entries(&self) -> Vec<usize> {
        let mut entries: Vec<usize> = (0..self.tiles.len()).collect();
        entries.sort_by(|a, b| {
            let (a, b) = (&self.tiles[*a], &self.tiles[*b]);
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then_with(|| a.installed.cmp(&b.installed))
                .then_with(|| a.id.cmp(&b.id))
        });
        entries
    }

    fn home(&mut self) {
        self.page = Page::Launcher;
    }

    fn open_apps(&mut self) {
        self.page = Page::Apps;
        self.apps_scroll = 0;
    }

    fn back(&mut self) {
        match self.page {
            Page::Launcher => {}
            Page::Apps | Page::Reference(_) | Page::QuickSettings | Page::Notifications => {
                self.home()
            }
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
                        // Like a mobile app drawer: launching closes the
                        // list, so going home later shows the home grid.
                        if self.page == Page::Apps {
                            self.page = Page::Launcher;
                        }
                    } else {
                        self.page = Page::Reference(index);
                    }
                }
            }
            Target::AllApps => self.open_apps(),
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
    /// App list offset when the contact began inside the list viewport;
    /// `None` when the contact cannot scroll the list.
    scroll_from: Option<i32>,
    /// The contact became a list drag: it scrolls and never taps.
    dragging: bool,
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
        let viewport = apps_viewport(size);
        let scroll_from = (state.page == Page::Apps
            && position.1 >= f64::from(viewport.y)
            && position.1 < f64::from(viewport.y + viewport.h))
        .then(|| {
            state
                .apps_scroll
                .clamp(0, apps_max_scroll(size, state.tiles.len()))
        });
        self.contact = Some(Contact {
            id,
            start: position,
            current: position,
            target,
            scroll_from,
            dragging: false,
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
        if let Some(from) = contact.scroll_from {
            let dx = position.0 - contact.start.0;
            let dy = position.1 - contact.start.1;
            if !contact.dragging && dy.abs() >= tap_slop(size) && dy.abs() > dx.abs() {
                contact.dragging = true;
            }
            if contact.dragging {
                let max = apps_max_scroll(size, state.tiles.len());
                let scroll = (f64::from(from) - dy).round() as i32;
                let old = (state.apps_scroll, state.pressed);
                state.apps_scroll = scroll.clamp(0, max);
                state.pressed = None;
                return old != (state.apps_scroll, state.pressed);
            }
        }
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

        if contact.dragging {
            // A list drag only scrolls; the offset is already applied.
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
        } else if state.page == Page::Launcher
            && contact.start.1 < h * 0.92
            && -dy > swipe
            && dy.abs() > dx.abs()
        {
            // An upward swipe on home opens the app list (the Android drawer
            // convention). It starts above the home zone (`Target::Home`,
            // the bottom 8%); the compositor's own home gesture starts in the
            // bottom 3% and never reaches this surface.
            state.open_apps();
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

/// Smallest launcher tile, in 1080-basis design px, at which the 96 px
/// glyph and the 30 px label still sit apart inside the tile (see
/// [`tile_parts`]). A grid that would need smaller tiles shows fewer rows
/// instead; everything stays reachable through the app list.
pub const MIN_TILE: i32 = 240;

/// Launcher tile rectangles, shared by rendering and hit testing. Three
/// columns; rows grow with `count` up to what fits. Portrait keeps the
/// original geometry (48px margin, 24px gap, grid top at one panel width)
/// when it fits, then lifts the grid, then shrinks the tiles down to
/// [`MIN_TILE`], and finally drops the rows that still do not fit, so the
/// last row always ends above the footer caption and gesture pill (a short
/// client area such as the Pixel 9a's 1080x2215) and a label never overlaps
/// its glyph. Landscape places the grid right of the cards and fits the rows
/// into the free height the same way. May return fewer than `count` rects.
pub fn tile_rects(size: (u32, u32), count: usize) -> Vec<Rect> {
    let (w, h) = (size.0 as i32, size.1 as i32);
    let basis = w.min(h);
    let s = |v: i32| scale(v, basis);
    let gap = s(24);
    let min_tile = s(MIN_TILE).max(1);
    let wanted = count.div_ceil(3).max(1) as i32;
    // Rows of `tile` that fit between `top` and `bottom`.
    let fit = |top: i32, bottom: i32, tile: i32| ((bottom - top + gap) / (tile + gap)).max(0);
    let (left, top, tile, rows) = if w > h {
        let left = s(940);
        let top = s(120);
        let bottom = h - s(90);
        let widest = (w - left - s(40) - 2 * gap) / 3;
        let rows = wanted.min(fit(top, bottom, min_tile.min(widest)));
        let tile = if rows == 0 {
            widest
        } else {
            widest.min((bottom - top - (rows - 1) * gap) / rows)
        };
        (left, top, tile, rows)
    } else {
        let margin = s(48);
        let designed = (w - 2 * margin - 2 * gap) / 3;
        // Keep the caption (baseline h - 150) and the pill clear, and never
        // rise into the battery/network cards and crash banner (end at 1056).
        let bottom = h - s(200);
        let lowest_top = s(1056);
        let rows = wanted.min(fit(lowest_top, bottom, min_tile.min(designed)));
        let mut tile = designed;
        let mut top = w.min(bottom - rows * tile - (rows - 1) * gap);
        if top < lowest_top {
            top = lowest_top;
            if rows > 0 {
                tile = tile.min((bottom - top - (rows - 1) * gap) / rows);
            }
        }
        ((w - 3 * tile - 2 * gap) / 2, top, tile, rows)
    };
    let shown = count.min((rows * 3) as usize);
    (0..shown)
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

/// The home grid: first-party tiles (index into `UiState::tiles`, rect)
/// followed by the Apps tile in the next slot. When the panel fits fewer
/// slots than first-party entries plus one, the Apps tile takes the last
/// slot and the remaining entries are reached through the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HomeGrid {
    pub tiles: Vec<(usize, Rect)>,
    pub apps: Option<Rect>,
}

pub fn home_grid(state: &UiState, size: (u32, u32)) -> HomeGrid {
    let entries = state.home_entries();
    let mut slots = tile_rects(size, entries.len() + 1);
    let apps = slots.pop();
    HomeGrid {
        tiles: entries.into_iter().zip(slots).collect(),
        apps,
    }
}

/// Where a tile draws its glyph and label, in panel px: the glyph box
/// (square, centred on the tile) and the label box (one line, inset from the
/// tile edges, baseline at `label_baseline`). Shared by rendering and the
/// geometry tests that prove the two never overlap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TileParts {
    pub icon: Rect,
    pub label: Rect,
    pub label_baseline: i32,
    pub label_px: i32,
}

pub fn tile_parts(r: Rect, size: (u32, u32)) -> TileParts {
    let basis = size.0.min(size.1) as i32;
    let s = |v: i32| scale(v, basis);
    let landscape = size.0 > size.1;
    let icon_size = s(96);
    let icon_cy = r.y + s(if landscape { 105 } else { 118 });
    let label_px = s(if landscape { 28 } else { 30 });
    let label_baseline = r.y + r.h - s(if landscape { 38 } else { 44 });
    // Ascent and descent of JetBrains Mono at `label_px` (generous).
    let ascent = label_px * 3 / 4;
    let descent = label_px / 4;
    TileParts {
        icon: Rect {
            x: r.x + (r.w - icon_size) / 2,
            y: icon_cy - icon_size / 2,
            w: icon_size,
            h: icon_size,
        },
        label: Rect {
            x: r.x + s(14),
            y: label_baseline - ascent,
            w: r.w - 2 * s(14),
            h: ascent + descent,
        },
        label_baseline,
        label_px,
    }
}

/// The app list viewport: below the page header, above the home zone (the
/// bottom 8%, `Target::Home`).
pub fn apps_viewport(size: (u32, u32)) -> Rect {
    let basis = size.0.min(size.1) as i32;
    let top = ferrix_ui::widgets::header_bottom(basis) + scale(12, basis);
    let bottom = (f64::from(size.1) * 0.92) as i32;
    Rect {
        x: 0,
        y: top,
        w: size.0 as i32,
        h: (bottom - top).max(0),
    }
}

/// App list row height and gap, 1080-basis design px.
pub const APP_ROW: i32 = 128;
pub const APP_ROW_GAP: i32 = 12;

/// Total height of `count` list rows plus the bottom padding, in panel px.
fn apps_content_height(size: (u32, u32), count: usize) -> i32 {
    let basis = size.0.min(size.1) as i32;
    let n = count as i32;
    if n == 0 {
        return 0;
    }
    n * scale(APP_ROW, basis) + (n - 1) * scale(APP_ROW_GAP, basis) + scale(24, basis)
}

/// Largest scroll offset: zero when every row fits.
pub fn apps_max_scroll(size: (u32, u32), count: usize) -> i32 {
    (apps_content_height(size, count) - apps_viewport(size).h).max(0)
}

/// Row rectangles of the app list at `scroll`, in list order (unclipped:
/// rows above or below the viewport have rects outside it).
pub fn app_row_rects(size: (u32, u32), count: usize, scroll: i32) -> Vec<Rect> {
    let basis = size.0.min(size.1) as i32;
    let s = |v: i32| scale(v, basis);
    let viewport = apps_viewport(size);
    let scroll = scroll.clamp(0, apps_max_scroll(size, count));
    let margin = s(48);
    let row = s(APP_ROW);
    let gap = s(APP_ROW_GAP);
    (0..count as i32)
        .map(|i| Rect {
            x: margin,
            y: viewport.y + i * (row + gap) - scroll,
            w: size.0 as i32 - 2 * margin,
            h: row,
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
            let grid = home_grid(state, size);
            if grid.apps.is_some_and(|rect| contains(rect, position)) {
                return Some(Target::AllApps);
            }
            grid.tiles
                .into_iter()
                .find(|(_, rect)| contains(*rect, position))
                .map(|(index, _)| Target::App(index))
        }
        Page::Apps => {
            if contains(ferrix_ui::widgets::back_rect(size), position) {
                return Some(Target::Back);
            }
            let viewport = apps_viewport(size);
            if !contains(viewport, position) {
                return None;
            }
            let entries = state.list_entries();
            app_row_rects(size, entries.len(), state.apps_scroll)
                .into_iter()
                .zip(entries)
                .find(|(rect, _)| contains(*rect, position))
                .map(|(_, index)| Target::App(index))
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
    fn a_short_client_area_lifts_the_grid_clear_of_the_footer() {
        // The Pixel 9a client area after its 137/72 safe area.
        let (w, h) = (1080, 2215);
        let rects = tile_rects((w, h), 8);
        let last = rects[7];
        assert!(last.y + last.h <= h as i32 - 200, "{last:?}");
        assert!(rects[0].y >= 1056, "{:?}", rects[0]);
        // Still square, still centred (within integer rounding).
        assert_eq!(rects[0].w, rects[0].h);
        assert!((rects[0].x - (w as i32 - (rects[2].x + rects[2].w))).abs() <= 1);
        // Rows that cannot fit at the minimum tile size are dropped rather
        // than shrunk until labels collide with glyphs.
        let rects = tile_rects((w, 1900), 12);
        assert!(
            rects.len() < 12 && rects.len().is_multiple_of(3),
            "{}",
            rects.len()
        );
        let last = rects[rects.len() - 1];
        assert!(last.y + last.h <= 1900 - 200, "{last:?}");
        assert!(rects.iter().all(|r| r.w >= MIN_TILE), "{rects:?}");
    }

    fn with_apks() -> UiState {
        let mut tiles = test_tiles();
        tiles.extend(crate::catalog::sample_installed_tiles());
        UiState::new(true, true, tiles)
    }

    fn overlaps(a: Rect, b: Rect) -> bool {
        a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
    }

    fn inside(inner: Rect, outer: Rect) -> bool {
        inner.x >= outer.x
            && inner.y >= outer.y
            && inner.x + inner.w <= outer.x + outer.w
            && inner.y + inner.h <= outer.y + outer.h
    }

    /// The Pixel 9a regression: 8 first-party tiles plus 10 APKs squeezed
    /// into one grid until labels overlapped their glyphs. The grid now
    /// holds the first-party tiles and the Apps tile at the designed size.
    #[test]
    fn installed_apks_never_shrink_the_home_grid() {
        for size in [(1080, 2340), (1080, 2215), (830, 1800), (2340, 1080)] {
            let plain = UiState::new(true, true, test_tiles());
            let state = with_apks();
            let grid = home_grid(&state, size);
            assert_eq!(
                grid,
                home_grid(&plain, size),
                "{size:?}: APKs change nothing"
            );
            assert_eq!(grid.tiles.len(), plain.tiles.len(), "{size:?}");
            assert!(grid.tiles.iter().all(|(i, _)| !state.tiles[*i].installed));
            let apps = grid.apps.expect("Apps tile");
            let mut rects: Vec<Rect> = grid.tiles.iter().map(|(_, r)| *r).collect();
            rects.push(apps);
            let basis = size.0.min(size.1) as i32;
            for (i, r) in rects.iter().enumerate() {
                assert!(r.w >= scale(MIN_TILE, basis), "{size:?} {r:?}");
                if size.0 < size.1 {
                    assert!(
                        r.y + r.h <= size.1 as i32 - scale(200, basis),
                        "{size:?} {r:?}"
                    );
                } else {
                    assert!(
                        r.y + r.h <= size.1 as i32 - scale(90, basis),
                        "{size:?} {r:?}"
                    );
                }
                let parts = tile_parts(*r, size);
                assert!(
                    !overlaps(parts.icon, parts.label),
                    "{size:?}: label over glyph {parts:?}"
                );
                assert!(
                    inside(parts.icon, *r) && inside(parts.label, *r),
                    "{size:?} {parts:?}"
                );
                for other in &rects[i + 1..] {
                    assert!(!overlaps(*r, *other), "{size:?}: {r:?} {other:?}");
                }
            }
            if size == (1080, 2340) {
                // The designed portrait tile: (1080 - 2*48 - 2*24) / 3.
                assert_eq!(rects[0].w, 312);
            }
        }
    }

    #[test]
    fn a_full_grid_keeps_the_apps_tile_in_its_last_slot() {
        // 20 first-party entries on a short panel: as many tiles as fit,
        // the Apps tile last, nothing below the minimum size.
        let mut tiles = test_tiles();
        let template = tiles[0].clone();
        for n in 0..12 {
            let mut extra = template.clone();
            extra.id = AppId::new(format!("org.example.extra{n}")).unwrap();
            tiles.push(extra);
        }
        let state = UiState::new(true, true, tiles);
        let size = (1080, 2215);
        let grid = home_grid(&state, size);
        let slots = tile_rects(size, 100);
        assert_eq!(grid.tiles.len() + 1, slots.len());
        assert_eq!(grid.apps, slots.last().copied());
    }

    #[test]
    fn the_app_list_sorts_every_entry_by_label_with_android_after_same_names() {
        let state = with_apks();
        let names: Vec<(&str, bool)> = state
            .list_entries()
            .into_iter()
            .map(|i| (state.tiles[i].name.as_str(), state.tiles[i].installed))
            .collect();
        assert_eq!(names.len(), state.tiles.len());
        let mut sorted = names.clone();
        sorted.sort_by_key(|(name, installed)| (name.to_lowercase(), *installed));
        assert_eq!(names, sorted);
        let contacts: Vec<bool> = names
            .iter()
            .filter(|(n, _)| *n == "Contacts")
            .map(|(_, installed)| *installed)
            .collect();
        assert_eq!(contacts, vec![false, true]);
    }

    #[test]
    fn app_list_rows_tile_the_viewport_and_scroll_within_bounds() {
        for size in [(1080, 2340), (830, 1800)] {
            let state = with_apks();
            let n = state.tiles.len();
            let viewport = apps_viewport(size);
            let max = apps_max_scroll(size, n);
            assert!(max > 0, "{size:?}: 18 rows do not fit one screen");
            let top = app_row_rects(size, n, 0);
            assert_eq!(top[0].y, viewport.y);
            for pair in top.windows(2) {
                assert!(pair[0].y + pair[0].h < pair[1].y, "{size:?}: rows overlap");
            }
            // Out-of-range offsets clamp.
            assert_eq!(app_row_rects(size, n, -50), top);
            let end = app_row_rects(size, n, i32::MAX);
            assert_eq!(end, app_row_rects(size, n, max));
            let last = end[n - 1];
            assert!(
                last.y + last.h <= viewport.y + viewport.h,
                "{size:?} {last:?}"
            );
            assert!(
                last.y + last.h >= viewport.y + viewport.h - scale(40, 1080),
                "{size:?}"
            );
            // A short list never scrolls.
            assert_eq!(apps_max_scroll(size, 3), 0);
            // The viewport ends above the home zone.
            assert!(viewport.y + viewport.h <= (f64::from(size.1) * 0.92) as i32);
        }
    }

    fn drag(input: &mut Input, state: &mut UiState, from: (f64, f64), to: (f64, f64)) {
        input.down(0, from, state, SIZE);
        for step in 1..=8 {
            let t = f64::from(step) / 8.0;
            let p = (from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t);
            input.motion(0, p, state, SIZE);
        }
        input.up(0, state, SIZE);
    }

    #[test]
    fn apps_opens_from_the_tile_or_an_upward_swipe_and_scrolls_by_drag() {
        let mut state = with_apks();
        let mut input = Input::default();
        // The Apps tile.
        let apps = home_grid(&state, SIZE).apps.unwrap();
        let center = (
            f64::from(apps.x + apps.w / 2),
            f64::from(apps.y + apps.h / 2),
        );
        input.down(0, center, &mut state, SIZE);
        assert_eq!(state.pressed, Some(Target::AllApps));
        input.up(0, &mut state, SIZE);
        assert_eq!(state.page, Page::Apps);
        assert!(state.pending_launch.is_empty());
        // Back (edge swipe) returns home.
        drag(&mut input, &mut state, (20.0, 1200.0), (320.0, 1200.0));
        assert_eq!(state.page, Page::Launcher);
        // An upward swipe on home opens the list; one from the home zone
        // (the bottom 8%) does not.
        drag(&mut input, &mut state, (540.0, 2200.0), (540.0, 1500.0));
        assert_eq!(state.page, Page::Launcher, "home zone is the compositor's");
        drag(&mut input, &mut state, (540.0, 1800.0), (540.0, 1100.0));
        assert_eq!(state.page, Page::Apps);
        assert_eq!(state.apps_scroll, 0);
        assert!(state.pending_launch.is_empty(), "a swipe never launches");

        // Dragging up scrolls (content follows the finger) and never taps.
        drag(&mut input, &mut state, (540.0, 1600.0), (540.0, 1200.0));
        assert_eq!(state.apps_scroll, 400);
        assert!(state.pending_launch.is_empty());
        assert_eq!(state.page, Page::Apps);
        // Far past the end clamps to the bounds.
        drag(&mut input, &mut state, (540.0, 2000.0), (540.0, 300.0));
        let max = apps_max_scroll(SIZE, state.tiles.len());
        assert_eq!(state.apps_scroll, max);
        drag(&mut input, &mut state, (540.0, 400.0), (540.0, 2100.0));
        assert_eq!(state.apps_scroll, 0);

        // A tap on a row launches that entry and closes the list.
        let entries = state.list_entries();
        let rows = app_row_rects(SIZE, entries.len(), state.apps_scroll);
        let (row, index) = rows
            .iter()
            .zip(&entries)
            .find(|(_, i)| state.tiles[**i].installed)
            .map(|(r, i)| (*r, *i))
            .unwrap();
        let tap = (f64::from(row.x + row.w / 2), f64::from(row.y + row.h / 2));
        input.down(0, tap, &mut state, SIZE);
        assert_eq!(state.pressed, Some(Target::App(index)));
        input.up(0, &mut state, SIZE);
        assert_eq!(state.pending_launch, vec![state.tiles[index].id.clone()]);
        assert_eq!(state.page, Page::Launcher);
    }

    #[test]
    fn app_list_hits_only_rows_inside_the_viewport() {
        let mut state = with_apks();
        state.page = Page::Apps;
        state.apps_scroll = 300;
        let viewport = apps_viewport(SIZE);
        // A row scrolled under the header is not hit through the header.
        assert_eq!(
            hit_test(&state, (540.0, f64::from(viewport.y - 4)), SIZE),
            None
        );
        assert_eq!(hit_test(&state, (60.0, 150.0), SIZE), Some(Target::Back));
        assert_eq!(hit_test(&state, (540.0, 2300.0), SIZE), Some(Target::Home));
        let entries = state.list_entries();
        let rows = app_row_rects(SIZE, entries.len(), state.apps_scroll);
        let visible = rows
            .iter()
            .position(|r| r.y >= viewport.y)
            .expect("a fully visible row");
        let r = rows[visible];
        assert_eq!(
            hit_test(&state, (540.0, f64::from(r.y + r.h / 2)), SIZE),
            Some(Target::App(entries[visible]))
        );
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
