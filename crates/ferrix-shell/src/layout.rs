//! Pure layout and focus logic shared by every backend.
//!
//! The product shell is portrait, touch-first and fullscreen-only: exactly one
//! toplevel (the newest mapped one) fills the output; there is no desktop
//! window management. Rotation is expressed as an output transform so clients
//! always see a logical portrait (or requested) surface.

// The compositor that consumes this policy is Linux-only; the policy itself
// (and its tests) build everywhere.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use ferrix_hardware_api::Orientation;
use ferrix_target_profile::SafeArea;

/// Output rotation applied between the panel scan-out and client logical space.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Rotation {
    #[default]
    Normal,
    Rotate90,
    Rotate180,
    Rotate270,
}

impl Rotation {
    pub fn label(self) -> &'static str {
        match self {
            Rotation::Normal => "normal",
            Rotation::Rotate90 => "90",
            Rotation::Rotate180 => "180",
            Rotation::Rotate270 => "270",
        }
    }

    /// Whether logical width/height are swapped relative to the panel.
    pub fn swaps_axes(self) -> bool {
        matches!(self, Rotation::Rotate90 | Rotation::Rotate270)
    }

    /// Rotate clockwise by 90 degrees (the user turned the device).
    pub fn next_clockwise(self) -> Rotation {
        match self {
            Rotation::Normal => Rotation::Rotate90,
            Rotation::Rotate90 => Rotation::Rotate180,
            Rotation::Rotate180 => Rotation::Rotate270,
            Rotation::Rotate270 => Rotation::Normal,
        }
    }
}

/// Pinned output plan: physical mode plus the logical size clients see.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutputLayout {
    pub panel_width: u32,
    pub panel_height: u32,
    pub rotation: Rotation,
}

impl OutputLayout {
    /// Choose a rotation so the logical output matches the requested
    /// orientation. A portrait profile on a portrait panel (the QEMU case) needs
    /// no transform; a landscape panel is rotated 90 degrees to present portrait.
    pub fn plan(panel_width: u32, panel_height: u32, orientation: Orientation) -> Self {
        let panel_is_portrait = panel_height >= panel_width;
        let rotation = match (panel_is_portrait, orientation) {
            (true, Orientation::Portrait) | (false, Orientation::Landscape) => Rotation::Normal,
            (true, Orientation::Landscape) | (false, Orientation::Portrait) => Rotation::Rotate90,
            (true, Orientation::PortraitFlipped) | (false, Orientation::LandscapeFlipped) => {
                Rotation::Rotate180
            }
            (true, Orientation::LandscapeFlipped) | (false, Orientation::PortraitFlipped) => {
                Rotation::Rotate270
            }
        };
        Self {
            panel_width,
            panel_height,
            rotation,
        }
    }

    pub fn with_rotation(self, rotation: Rotation) -> Self {
        Self { rotation, ..self }
    }

    /// Logical size presented to clients after the rotation.
    pub fn logical_size(&self) -> (u32, u32) {
        if self.rotation.swaps_axes() {
            (self.panel_height, self.panel_width)
        } else {
            (self.panel_width, self.panel_height)
        }
    }

    /// The logical rectangle `(x, y, width, height)` fullscreen clients
    /// occupy: the logical output minus the panel's safe-area insets, which
    /// turn with the content. Rotate90 is a quarter turn clockwise (the
    /// panel's top edge becomes the logical right edge).
    pub fn client_rect(&self, safe: SafeArea) -> (i32, i32, u32, u32) {
        let (top, right, bottom, left) = match self.rotation {
            Rotation::Normal => (safe.top, safe.right, safe.bottom, safe.left),
            Rotation::Rotate90 => (safe.left, safe.top, safe.right, safe.bottom),
            Rotation::Rotate180 => (safe.bottom, safe.left, safe.top, safe.right),
            Rotation::Rotate270 => (safe.right, safe.bottom, safe.left, safe.top),
        };
        let (w, h) = self.logical_size();
        (
            left as i32,
            top as i32,
            w.saturating_sub(left + right),
            h.saturating_sub(top + bottom),
        )
    }

    /// The compositor's home indicator for clients that draw none (Android
    /// windows): the rows of a rounded pill, as `(x, y, w, h)` rectangles,
    /// centred in the bottom safe-area inset of the rotated output. It has
    /// the proportions of `ferrix_ui::widgets::gesture_pill` (320x12 at a
    /// 1080 px basis). Empty when the inset cannot hold it.
    pub fn system_pill_rows(&self, safe: SafeArea) -> Vec<(i32, i32, u32, u32)> {
        let bottom = match self.rotation {
            Rotation::Normal => safe.bottom,
            Rotation::Rotate90 => safe.right,
            Rotation::Rotate180 => safe.top,
            Rotation::Rotate270 => safe.left,
        };
        let (w, h) = self.logical_size();
        let basis = w.min(h);
        let pill_w = 320 * basis / 1080;
        let pill_h = (12 * basis / 1080).max(2);
        if bottom < pill_h * 2 || pill_w == 0 {
            return Vec::new();
        }
        let x0 = (w - pill_w) as i32 / 2;
        let y0 = (h - bottom / 2 - pill_h / 2) as i32;
        // Round the caps: each row is inset by the circle of radius pill_h/2;
        // equal neighbouring rows merge into one rectangle.
        let r = f64::from(pill_h) / 2.0;
        let mut rows: Vec<(i32, i32, u32, u32)> = Vec::new();
        for row in 0..pill_h {
            let dy = f64::from(row) + 0.5 - r;
            let inset = (r - (r * r - dy * dy).max(0.0).sqrt()).round() as u32;
            let (x, width) = (x0 + inset as i32, pill_w - 2 * inset);
            match rows.last_mut() {
                Some(last) if last.0 == x && last.2 == width => last.3 += 1,
                _ => rows.push((x, y0 + row as i32, width, 1)),
            }
        }
        rows
    }
}

/// Whether a toplevel draws Ferrix's own gesture pill. Native Ferrix surfaces
/// (`org.ferrix.*`) draw it themselves; anything else (Waydroid's Android
/// window) gets the compositor's indicator in the bottom inset. The app_id is
/// client-claimed, which is fine here: it only decides decoration.
pub fn draws_own_pill(app_id: Option<&str>) -> bool {
    app_id.is_some_and(|id| id.starts_with("org.ferrix."))
}

/// Whether a toplevel's claimed xdg `app_id` is in the namespace of the
/// shared application runtime's windows: Android task windows
/// (`waydroid.<package>`) and the runtime's full UI (`Waydroid`). Such a
/// window is mapped *withheld*: never drawn or focused until `ferrix-appd`
/// activates that exact surface, because the client pid alone cannot say
/// which application (if any) the window belongs to. The `app_id` is
/// client-claimed, which is fine here: withholding only restricts
/// presentation, so a false claim harms only the claimant, and the
/// association itself is decided by `ferrix-appd` from the runtime-reported
/// pid.
pub fn withheld_until_activated(app_id: Option<&str>) -> bool {
    app_id.is_some_and(|id| id == "Waydroid" || id.starts_with("waydroid."))
}

/// The one configure size every toplevel gets: the client area (logical
/// output minus the safe area). It never depends on the window, its client,
/// or its app_id. Waydroid's multi-window hwcomposer resizes the whole
/// Android display whenever any task window is configured to a new size, so
/// per-window sizes would make tasks fight over the display.
pub fn toplevel_configure_size(plan: &OutputLayout, safe: SafeArea) -> (u32, u32) {
    let (_, _, w, h) = plan.client_rect(safe);
    (w, h)
}

/// Fullscreen-only toplevel stack. Newest mapped window is focused and drawn;
/// unmapped toplevels (no buffer yet) are tracked but never focused, and
/// withheld ones (see [`withheld_until_activated`]) are mapped but never
/// focused or drawn until [`WindowStack::raise_surface`] reveals them.
///
/// Every entry carries the owning client's process id (from the kernel via
/// `SO_PEERCRED`, so it cannot be spoofed), a compositor-assigned surface id
/// (several toplevels may share a pid), and the last xdg `app_id` the client
/// claimed (untrusted). The pid and surface id are what link a window to an
/// `ferrix_appd` application instance.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WindowStack<T: PartialEq + Clone> {
    entries: Vec<Entry<T>>,
    last_surface: u64,
}

#[derive(Clone, Debug, PartialEq)]
struct Entry<T> {
    id: T,
    surface: u64,
    mapped: bool,
    held: bool,
    pid: Option<u32>,
    app_id: Option<String>,
}

impl<T: PartialEq + Clone> Entry<T> {
    fn visible(&self) -> bool {
        self.mapped && !self.held
    }
}

impl<T: PartialEq + Clone> WindowStack<T> {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            last_surface: 0,
        }
    }

    /// Track a new (not yet mapped) toplevel owned by the client with `pid`
    /// (`None` when the kernel did not report credentials). Assigns its
    /// surface id (unique for the compositor's lifetime, never 0).
    pub fn push_with_pid(&mut self, id: T, pid: Option<u32>) {
        if !self.contains(&id) {
            self.last_surface += 1;
            self.entries.push(Entry {
                id,
                surface: self.last_surface,
                mapped: false,
                held: false,
                pid,
                app_id: None,
            });
        }
    }

    pub fn contains(&self, id: &T) -> bool {
        self.entries.iter().any(|e| &e.id == id)
    }

    pub fn is_mapped(&self, id: &T) -> bool {
        self.entries.iter().any(|e| &e.id == id && e.mapped)
    }

    /// Mark a toplevel mapped (first buffer committed) and raise it. Returns
    /// `true` the first time it maps.
    pub fn mark_mapped(&mut self, id: &T) -> bool {
        let Some(index) = self.entries.iter().position(|e| &e.id == id) else {
            return false;
        };
        if self.entries[index].mapped {
            return false;
        }
        let mut entry = self.entries.remove(index);
        entry.mapped = true;
        self.entries.push(entry);
        true
    }

    /// Withhold (or stop withholding) a toplevel from focus and drawing.
    pub fn set_held(&mut self, id: &T, held: bool) {
        if let Some(entry) = self.entries.iter_mut().find(|e| &e.id == id) {
            entry.held = held;
        }
    }

    pub fn is_held(&self, id: &T) -> bool {
        self.entries.iter().any(|e| &e.id == id && e.held)
    }

    /// Move a mapped, visible toplevel to the top so
    /// [`WindowStack::focused`] returns it. Returns `false` when the toplevel
    /// is unknown, not mapped yet, or withheld.
    pub fn raise(&mut self, id: &T) -> bool {
        let Some(index) = self.entries.iter().position(|e| &e.id == id && e.visible()) else {
            return false;
        };
        let entry = self.entries.remove(index);
        self.entries.push(entry);
        true
    }

    /// Raise the newest visible toplevel of the client with `pid`. Returns
    /// `false` when that client has none. Never reveals a withheld window:
    /// a pid shared by several applications' windows says nothing about
    /// which one is meant.
    pub fn raise_pid(&mut self, pid: u32) -> bool {
        let Some(index) = self
            .entries
            .iter()
            .rposition(|e| e.visible() && e.pid == Some(pid))
        else {
            return false;
        };
        let entry = self.entries.remove(index);
        self.entries.push(entry);
        true
    }

    /// Reveal and raise the mapped toplevel with surface id `surface` owned
    /// by `pid`. Returns `false` when there is no such mapped toplevel.
    pub fn raise_surface(&mut self, pid: u32, surface: u64) -> bool {
        let Some(index) = self
            .entries
            .iter()
            .position(|e| e.mapped && e.surface == surface && e.pid == Some(pid))
        else {
            return false;
        };
        let mut entry = self.entries.remove(index);
        entry.held = false;
        self.entries.push(entry);
        true
    }

    /// The toplevel with surface id `surface` owned by `pid`.
    pub fn find_surface(&self, pid: u32, surface: u64) -> Option<&T> {
        self.entries
            .iter()
            .find(|e| e.surface == surface && e.pid == Some(pid))
            .map(|e| &e.id)
    }

    pub fn remove(&mut self, id: &T) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| &e.id != id);
        before != self.entries.len()
    }

    /// The single visible, focused toplevel: the newest mapped one that is
    /// not withheld.
    pub fn focused(&self) -> Option<&T> {
        self.entries
            .iter()
            .rev()
            .find(|e| e.visible())
            .map(|e| &e.id)
    }

    /// The visible toplevel furthest down the stack: the fallback "home"
    /// when no application service can say which window is home.
    pub fn oldest_mapped(&self) -> Option<&T> {
        self.entries.iter().find(|e| e.visible()).map(|e| &e.id)
    }

    /// Mapped toplevels, withheld ones included.
    pub fn mapped_count(&self) -> usize {
        self.entries.iter().filter(|e| e.mapped).count()
    }

    /// Mapped toplevels that may be focused and drawn.
    pub fn visible_count(&self) -> usize {
        self.entries.iter().filter(|e| e.visible()).count()
    }

    /// Mapped toplevels owned by the client with `pid`.
    pub fn mapped_count_for_pid(&self, pid: u32) -> usize {
        self.entries
            .iter()
            .filter(|e| e.mapped && e.pid == Some(pid))
            .count()
    }

    /// Process id of the client owning `id`, when the kernel reported one.
    pub fn pid_of(&self, id: &T) -> Option<u32> {
        self.entries
            .iter()
            .find(|e| &e.id == id)
            .and_then(|e| e.pid)
    }

    /// Compositor-assigned surface id of `id`.
    pub fn surface_of(&self, id: &T) -> Option<u64> {
        self.entries.iter().find(|e| &e.id == id).map(|e| e.surface)
    }

    /// Last xdg `app_id` seen for `id` (client-claimed, untrusted).
    pub fn app_id_of(&self, id: &T) -> Option<&str> {
        self.entries
            .iter()
            .find(|e| &e.id == id)
            .and_then(|e| e.app_id.as_deref())
    }

    /// Record the xdg `app_id` a client claimed for `id`.
    pub fn set_app_id(&mut self, id: &T, app_id: Option<String>) {
        if let Some(entry) = self.entries.iter_mut().find(|e| &e.id == id) {
            entry.app_id = app_id;
        }
    }

    /// All tracked toplevels, bottom to top.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.entries.iter().map(|e| &e.id)
    }

    /// Mapped toplevels (withheld ones included) with their pid and surface
    /// id, bottom to top.
    pub fn mapped(&self) -> impl Iterator<Item = (&T, Option<u32>, u64)> {
        self.entries
            .iter()
            .filter(|e| e.mapped)
            .map(|e| (&e.id, e.pid, e.surface))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portrait_profile_on_portrait_panel_needs_no_rotation() {
        let layout = OutputLayout::plan(1080, 2340, Orientation::Portrait);
        assert_eq!(layout.rotation, Rotation::Normal);
        assert_eq!(layout.logical_size(), (1080, 2340));
    }

    #[test]
    fn portrait_profile_on_landscape_panel_rotates() {
        let layout = OutputLayout::plan(1920, 1080, Orientation::Portrait);
        assert_eq!(layout.rotation, Rotation::Rotate90);
        assert_eq!(layout.logical_size(), (1080, 1920));
    }

    #[test]
    fn landscape_profile_on_portrait_panel_rotates() {
        let layout = OutputLayout::plan(1080, 2340, Orientation::Landscape);
        assert_eq!(layout.rotation, Rotation::Rotate90);
        assert_eq!(layout.logical_size(), (2340, 1080));
    }

    #[test]
    fn manual_rotation_cycles_and_swaps_axes() {
        let layout = OutputLayout::plan(1080, 2340, Orientation::Portrait);
        let turned = layout.with_rotation(layout.rotation.next_clockwise());
        assert_eq!(turned.rotation, Rotation::Rotate90);
        assert_eq!(turned.logical_size(), (2340, 1080));
        let back = (0..4).fold(layout.rotation, |r, _| r.next_clockwise());
        assert_eq!(back, Rotation::Normal);
        assert!(!Rotation::Rotate180.swaps_axes());
    }

    #[test]
    fn flipped_orientations_preserve_direction() {
        assert_eq!(
            OutputLayout::plan(1080, 2340, Orientation::PortraitFlipped).rotation,
            Rotation::Rotate180
        );
        assert_eq!(
            OutputLayout::plan(1080, 2340, Orientation::LandscapeFlipped).rotation,
            Rotation::Rotate270
        );
    }

    #[test]
    fn newest_mapped_window_is_focused() {
        let mut stack: WindowStack<u32> = WindowStack::new();
        assert_eq!(stack.focused(), None);
        stack.push_with_pid(1, None);
        assert_eq!(
            stack.focused(),
            None,
            "unmapped toplevels are never focused"
        );
        assert!(stack.mark_mapped(&1));
        assert!(!stack.mark_mapped(&1), "mapping is reported once");
        assert_eq!(stack.focused(), Some(&1));
        stack.push_with_pid(2, None);
        assert_eq!(stack.focused(), Some(&1));
        assert!(stack.mark_mapped(&2));
        assert_eq!(stack.focused(), Some(&2));
        assert_eq!(stack.mapped_count(), 2);
    }

    #[test]
    fn background_buffer_commits_do_not_raise_the_window() {
        let mut stack: WindowStack<&'static str> = WindowStack::new();
        stack.push_with_pid("home", Some(100));
        stack.mark_mapped(&"home");
        stack.push_with_pid("terminal", Some(200));
        stack.mark_mapped(&"terminal");
        assert_eq!(stack.focused(), Some(&"terminal"));

        assert!(!stack.mark_mapped(&"home"), "home was already mapped");
        assert_eq!(
            stack.focused(),
            Some(&"terminal"),
            "a background clock redraw must not change fullscreen focus"
        );
        assert_eq!(
            stack.iter().copied().collect::<Vec<_>>(),
            vec!["home", "terminal"]
        );
    }

    #[test]
    fn removing_the_top_window_falls_back_to_previous() {
        let mut stack: WindowStack<&'static str> = WindowStack::new();
        stack.push_with_pid("launcher", None);
        stack.mark_mapped(&"launcher");
        stack.push_with_pid("terminal", None);
        stack.mark_mapped(&"terminal");
        assert!(stack.remove(&"terminal"));
        assert!(!stack.remove(&"terminal"));
        assert_eq!(stack.focused(), Some(&"launcher"));
        assert!(stack.remove(&"launcher"));
        assert_eq!(stack.focused(), None);
        assert_eq!(stack.mapped_count(), 0);
    }

    #[test]
    fn push_is_idempotent_and_keeps_order() {
        let mut stack: WindowStack<u8> = WindowStack::new();
        stack.push_with_pid(1, None);
        stack.push_with_pid(2, None);
        stack.push_with_pid(1, None);
        assert_eq!(stack.iter().copied().collect::<Vec<_>>(), vec![1, 2]);
        stack.mark_mapped(&1);
        assert_eq!(
            stack.iter().copied().collect::<Vec<_>>(),
            vec![2, 1],
            "mapping raises"
        );
        assert!(stack.is_mapped(&1));
        assert!(!stack.is_mapped(&2));
        assert!(!stack.mark_mapped(&9), "unknown toplevels are ignored");
    }

    #[test]
    fn windows_remember_their_client_pid_and_app_id() {
        let mut stack: WindowStack<&'static str> = WindowStack::new();
        stack.push_with_pid("home", Some(100));
        stack.push_with_pid("terminal", Some(200));
        stack.push_with_pid("anonymous", None);
        assert_eq!(stack.pid_of(&"home"), Some(100));
        assert_eq!(stack.pid_of(&"anonymous"), None);
        assert_eq!(stack.pid_of(&"missing"), None);

        assert_eq!(stack.app_id_of(&"home"), None);
        stack.set_app_id(&"home", Some("org.ferrix.home".to_owned()));
        assert_eq!(stack.app_id_of(&"home"), Some("org.ferrix.home"));
        stack.set_app_id(&"missing", Some("ignored".to_owned()));

        assert_eq!(
            stack.mapped_count_for_pid(100),
            0,
            "unmapped does not count"
        );
        stack.mark_mapped(&"home");
        stack.mark_mapped(&"terminal");
        assert_eq!(stack.mapped_count_for_pid(100), 1);
        assert_eq!(stack.mapped_count_for_pid(999), 0);
        let mapped: Vec<_> = stack.mapped().collect();
        assert_eq!(
            mapped,
            vec![(&"home", Some(100), 1), (&"terminal", Some(200), 2),]
        );
    }

    #[test]
    fn raise_moves_a_mapped_window_to_the_top() {
        let mut stack: WindowStack<&'static str> = WindowStack::new();
        stack.push_with_pid("home", Some(100));
        stack.push_with_pid("terminal", Some(200));
        stack.push_with_pid("pending", Some(300));
        stack.mark_mapped(&"home");
        stack.mark_mapped(&"terminal");
        assert_eq!(stack.focused(), Some(&"terminal"));
        assert_eq!(stack.oldest_mapped(), Some(&"home"));

        assert!(stack.raise(&"home"));
        assert_eq!(stack.focused(), Some(&"home"));
        assert_eq!(stack.oldest_mapped(), Some(&"terminal"));
        assert!(
            !stack.raise(&"pending"),
            "unmapped windows cannot be raised"
        );
        assert!(!stack.raise(&"missing"));
        assert_eq!(stack.focused(), Some(&"home"));

        assert!(stack.raise_pid(200));
        assert_eq!(stack.focused(), Some(&"terminal"));
        assert!(!stack.raise_pid(300), "pid with no mapped toplevel");
        assert!(!stack.raise_pid(999));
        assert_eq!(stack.focused(), Some(&"terminal"));
        assert_eq!(
            stack.iter().copied().collect::<Vec<_>>(),
            vec!["pending", "home", "terminal"],
            "raising keeps the unmapped entry in place"
        );
    }

    #[test]
    fn raise_pid_picks_the_newest_toplevel_of_that_client() {
        let mut stack: WindowStack<u8> = WindowStack::new();
        stack.push_with_pid(1, Some(7));
        stack.push_with_pid(2, Some(7));
        stack.push_with_pid(3, Some(8));
        for id in 1..=3 {
            stack.mark_mapped(&id);
        }
        assert!(stack.raise_pid(7));
        assert_eq!(stack.focused(), Some(&2));
        assert_eq!(stack.mapped_count_for_pid(7), 2);
        stack.remove(&2);
        assert_eq!(stack.mapped_count_for_pid(7), 1);
        stack.remove(&1);
        assert_eq!(stack.mapped_count_for_pid(7), 0);
        assert_eq!(stack.oldest_mapped(), Some(&3));
    }

    #[test]
    fn oldest_mapped_is_none_without_mapped_windows() {
        let mut stack: WindowStack<u8> = WindowStack::new();
        assert_eq!(stack.oldest_mapped(), None);
        stack.push_with_pid(1, None);
        assert_eq!(stack.oldest_mapped(), None);
        stack.mark_mapped(&1);
        assert_eq!(stack.oldest_mapped(), Some(&1));
    }

    #[test]
    fn system_pill_sits_centred_in_the_bottom_inset_with_round_caps() {
        let safe = SafeArea {
            top: 137,
            bottom: 72,
            left: 0,
            right: 0,
        };
        let portrait = OutputLayout::plan(1080, 2424, Orientation::Portrait);
        let rows = portrait.system_pill_rows(safe);
        let top = rows.iter().map(|r| r.1).min().unwrap();
        let bottom = rows.iter().map(|r| r.1 + r.3 as i32).max().unwrap();
        assert_eq!((top, bottom), (2424 - 36 - 6, 2424 - 36 + 6));
        // Widest row is the full pill, centred; the caps are narrower.
        let widest = rows.iter().max_by_key(|r| r.2).unwrap();
        assert_eq!((widest.0, widest.2), (380, 320));
        assert!(rows.first().unwrap().2 < 320 && rows.last().unwrap().2 < 320);
        // Rows tile the pill height exactly.
        assert_eq!(rows.iter().map(|r| r.3).sum::<u32>(), 12);
        // No inset, no pill.
        assert!(portrait.system_pill_rows(SafeArea::default()).is_empty());
    }

    #[test]
    fn toplevels_sharing_a_pid_get_distinct_surface_ids() {
        let mut stack: WindowStack<&'static str> = WindowStack::new();
        stack.push_with_pid("browser", Some(4242));
        stack.push_with_pid("wizard", Some(4242));
        stack.push_with_pid("browser", Some(4242));
        let browser = stack.surface_of(&"browser").unwrap();
        let wizard = stack.surface_of(&"wizard").unwrap();
        assert_ne!(browser, wizard);
        assert!(browser > 0 && wizard > 0);
        assert_eq!(stack.find_surface(4242, wizard), Some(&"wizard"));
        assert_eq!(stack.find_surface(1, wizard), None, "pid must match too");
        // Ids are never reused after a removal.
        stack.remove(&"wizard");
        stack.push_with_pid("again", Some(4242));
        assert!(stack.surface_of(&"again").unwrap() > wizard);
    }

    #[test]
    fn withheld_windows_are_never_focused_until_raised_by_surface() {
        let mut stack: WindowStack<&'static str> = WindowStack::new();
        stack.push_with_pid("home", Some(100));
        stack.mark_mapped(&"home");
        stack.push_with_pid("browser", Some(4242));
        stack.push_with_pid("wizard", Some(4242));
        stack.mark_mapped(&"browser");
        stack.set_held(&"browser", true);
        stack.mark_mapped(&"wizard");
        stack.set_held(&"wizard", true);
        assert_eq!(stack.focused(), Some(&"home"));
        assert_eq!(stack.mapped_count(), 3);
        assert_eq!(stack.visible_count(), 1);
        // A pid-only activation cannot reveal a withheld window.
        assert!(!stack.raise_pid(4242));
        assert!(!stack.raise(&"wizard"));
        assert_eq!(stack.focused(), Some(&"home"));
        // The surface-addressed activation reveals exactly that window.
        let browser = stack.surface_of(&"browser").unwrap();
        assert!(!stack.raise_surface(1, browser), "wrong pid");
        assert!(stack.raise_surface(4242, browser));
        assert_eq!(stack.focused(), Some(&"browser"));
        assert!(!stack.is_held(&"browser"));
        assert!(stack.is_held(&"wizard"));
        // Withholding the focused window falls back to the one below.
        stack.set_held(&"browser", true);
        assert_eq!(stack.focused(), Some(&"home"));
        assert_eq!(stack.oldest_mapped(), Some(&"home"));
    }

    #[test]
    fn only_runtime_task_window_app_ids_are_withheld() {
        assert!(withheld_until_activated(Some(
            "waydroid.net.slions.fulguris.full.fdroid"
        )));
        assert!(withheld_until_activated(Some(
            "waydroid.org.lineageos.setupwizard"
        )));
        assert!(withheld_until_activated(Some("Waydroid")));
        assert!(!withheld_until_activated(Some("org.ferrix.terminal")));
        assert!(!withheld_until_activated(Some("waydroid")));
        assert!(!withheld_until_activated(None));
    }

    #[test]
    fn every_toplevel_is_configured_to_the_same_client_area() {
        let safe = SafeArea {
            top: 120,
            right: 0,
            bottom: 60,
            left: 0,
        };
        let portrait = OutputLayout::plan(1080, 2424, Orientation::Portrait);
        let size = toplevel_configure_size(&portrait, safe);
        assert_eq!(size, (1080, 2424 - 180));
        let (_, _, w, h) = portrait.client_rect(safe);
        assert_eq!(size, (w, h));
        // Only the output plan changes it (rotation), never the window.
        let rotated = portrait.with_rotation(Rotation::Rotate90);
        assert_eq!(toplevel_configure_size(&rotated, safe), (2424 - 180, 1080));
        assert_ne!(toplevel_configure_size(&rotated, safe), (0, 0));
    }

    #[test]
    fn only_native_ferrix_surfaces_draw_their_own_pill() {
        assert!(draws_own_pill(Some("org.ferrix.home")));
        assert!(draws_own_pill(Some("org.ferrix.files")));
        assert!(!draws_own_pill(Some("Waydroid")));
        assert!(!draws_own_pill(None));
    }

    #[test]
    fn client_rect_applies_safe_area_insets_and_turns_with_the_output() {
        let safe = SafeArea {
            top: 96,
            bottom: 36,
            left: 0,
            right: 0,
        };
        let portrait = OutputLayout::plan(1080, 2424, Orientation::Portrait);
        assert_eq!(portrait.client_rect(safe), (0, 96, 1080, 2292));
        assert_eq!(
            portrait.client_rect(SafeArea::default()),
            (0, 0, 1080, 2424)
        );
        let landscape = OutputLayout::plan(1080, 2424, Orientation::Landscape);
        assert_eq!(landscape.client_rect(safe), (36, 0, 2424 - 96 - 36, 1080));
    }
}
