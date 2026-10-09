//! Target-independent interaction model for the Terminal, shared by pointer
//! and touch input. Pure geometry and state: no Wayland, no PTY. Time is
//! passed in, so key repeat, long-press selection, and the transient
//! scrollbar are deterministic under test.
//!
//! - **Keys** fire on release; a held repeatable key (backspace, arrows,
//!   space) repeats through [`KeyRepeat`] and then does not fire again on
//!   release.
//! - **Grid**: a vertical drag scrolls history one line per text line and
//!   reveals the scrollbar. Holding still for [`LONG_PRESS`] selects the word
//!   under the finger; continuing to drag extends the selection. While a
//!   selection exists it shows two drag handles and a Copy / Paste / Done
//!   bar; a tap elsewhere on the grid clears it.
//! - **Scrollbar**: a thin steel thumb at the grid's right edge, shown while
//!   scrolling and for [`SCROLLBAR_LINGER`] after; while shown it can be
//!   dragged to any history position.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::time::{Duration, Instant};

use ferrix_ui::draw::Rect;
use ferrix_ui::keyboard::KeyRepeat;
use ferrix_ui::osk::{self, Key, OskState};

use crate::selection::{Handle, Point, Selection};
use crate::terminal::Snapshot;

/// Hold time that turns a touch on the grid into a selection.
pub const LONG_PRESS: Duration = Duration::from_millis(500);
/// How long the scrollbar stays after the last scroll.
pub const SCROLLBAR_LINGER: Duration = Duration::from_millis(1500);
/// Length of the visual bell's border flash.
pub const BELL_FLASH: Duration = Duration::from_millis(180);
/// How long a clipboard notice stays.
pub const NOTICE: Duration = Duration::from_millis(1800);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelAction {
    Copy,
    Paste,
    Done,
}

impl SelAction {
    pub const ALL: [SelAction; 3] = [SelAction::Copy, SelAction::Paste, SelAction::Done];

    pub fn label(self) -> &'static str {
        match self {
            SelAction::Copy => "Copy",
            SelAction::Paste => "Paste",
            SelAction::Done => "Done",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// The header arrow: yield the foreground to the home surface.
    Back,
    /// The bottom gesture zone. Drawn as the pill; owned by the compositor,
    /// so the app never acts on it.
    Home,
    /// An on-screen keyboard key.
    Key(Key),
    /// The terminal grid itself: drags scroll, a long press selects.
    TermArea,
    /// The transient scrollbar strip.
    Scrollbar,
    /// A selection drag handle.
    Handle(Handle),
    /// A button of the selection bar.
    Action(SelAction),
}

/// Terminal work produced by the interaction model for the session owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TermAction {
    /// Bytes for the shell's stdin.
    Input(Vec<u8>),
    /// Viewport scroll in lines; positive moves into history.
    Scroll(i32),
    /// Scroll to an absolute display offset (0 is the live screen).
    ScrollTo(usize),
    /// Copy this selection to the clipboard.
    Copy(Selection),
    /// Paste the clipboard into the shell.
    Paste,
}

/// Where the grid is drawn and what it shows, refreshed from each frame's
/// metrics and snapshot so hit tests match the pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GridView {
    pub area: Rect,
    pub origin: (i32, i32),
    pub cell_w: i32,
    pub cell_h: i32,
    pub cols: usize,
    pub rows: usize,
    /// Lines of history above the live screen.
    pub history: usize,
    /// Current display offset into history.
    pub offset: usize,
}

impl GridView {
    pub fn cell_rect(&self, row: usize, col: usize) -> Rect {
        Rect {
            x: self.origin.0 + col as i32 * self.cell_w,
            y: self.origin.1 + row as i32 * self.cell_h,
            w: self.cell_w,
            h: self.cell_h,
        }
    }

    /// The viewport `(row, col)` nearest to `position`, clamped to the grid.
    pub fn cell_at(&self, position: (f64, f64)) -> Option<(usize, usize)> {
        if self.cols == 0 || self.rows == 0 || self.cell_w <= 0 || self.cell_h <= 0 {
            return None;
        }
        let col = ((position.0 - f64::from(self.origin.0)) / f64::from(self.cell_w)).floor();
        let row = ((position.1 - f64::from(self.origin.1)) / f64::from(self.cell_h)).floor();
        let clamp = |v: f64, n: usize| v.clamp(0.0, (n - 1) as f64) as usize;
        Some((clamp(row, self.rows), clamp(col, self.cols)))
    }

    /// The buffer point nearest to `position`.
    pub fn point_at(&self, position: (f64, f64)) -> Option<Point> {
        self.cell_at(position)
            .map(|(row, col)| Point::from_viewport(row, col, self.offset))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UiState {
    pub pressed: Option<Target>,
    pub osk: OskState,
    /// Queued terminal actions; the session owner drains these after input.
    pub pending: Vec<TermAction>,
    /// Number of Back requests not yet delivered to `ferrix_appd`.
    pub pending_back: u32,
    /// Mirror of the terminal's DECCKM mode so arrows encode correctly.
    pub term_app_cursor: bool,
    pub view: GridView,
    pub repeat: KeyRepeat,
    pub selection: Option<Selection>,
    /// The scrollbar is drawn (and draggable).
    pub scrollbar_visible: bool,
    pub scrollbar_until: Option<Instant>,
    /// The visual bell's border flash is on.
    pub bell: bool,
    pub bell_until: Option<Instant>,
    /// A short clipboard note (`copied · 12 chars`).
    pub notice: Option<String>,
    pub notice_until: Option<Instant>,
}

impl Default for UiState {
    fn default() -> Self {
        Self::new()
    }
}

impl UiState {
    pub fn new() -> Self {
        Self {
            pressed: None,
            osk: OskState::new(osk::Layout::Terminal),
            pending: Vec::new(),
            pending_back: 0,
            term_app_cursor: false,
            view: GridView::default(),
            repeat: KeyRepeat::default(),
            selection: None,
            scrollbar_visible: false,
            scrollbar_until: None,
            bell: false,
            bell_until: None,
            notice: None,
            notice_until: None,
        }
    }

    fn back(&mut self) {
        self.pending_back += 1;
    }

    fn type_key(&mut self, key: Key) {
        if let Some(bytes) = self.osk.press(key, self.term_app_cursor) {
            self.pending.push(TermAction::Input(bytes));
        }
    }

    fn activate(&mut self, target: Target) {
        match target {
            Target::Back => self.back(),
            Target::Key(key) => self.type_key(key),
            Target::Action(action) => {
                match action {
                    SelAction::Copy => {
                        if let Some(selection) = self.selection {
                            self.pending.push(TermAction::Copy(selection));
                        }
                    }
                    SelAction::Paste => self.pending.push(TermAction::Paste),
                    SelAction::Done => {}
                }
                self.selection = None;
            }
            Target::Home | Target::TermArea | Target::Scrollbar | Target::Handle(_) => {}
        }
    }

    pub fn show_scrollbar(&mut self, now: Instant) {
        if self.view.history > 0 {
            self.scrollbar_visible = true;
            self.scrollbar_until = Some(now + SCROLLBAR_LINGER);
        }
    }

    /// The shell rang the bell.
    pub fn ring(&mut self, now: Instant) {
        self.bell = true;
        self.bell_until = Some(now + BELL_FLASH);
    }

    pub fn note(&mut self, text: String, now: Instant) {
        self.notice = Some(text);
        self.notice_until = Some(now + NOTICE);
    }

    /// Folds a fresh snapshot into the view: grid size, history, offset.
    /// Output that pushed lines into history moves the selection with its
    /// text; a selection that left the buffer is dropped.
    pub fn sync(&mut self, view: GridView) {
        let grown = view.history as i64 - self.view.history as i64;
        if self.view.rows != view.rows {
            // A resize reflows the buffer; the old points mean nothing.
            self.selection = None;
        } else if let Some(selection) = self.selection.as_mut()
            && grown > 0
        {
            selection.shift(grown as i32);
        }
        self.selection = self.selection.filter(|s| s.in_buffer(view.history));
        if view.history == 0 {
            self.scrollbar_visible = false;
            self.scrollbar_until = None;
        }
        self.view = view;
    }
}

/// Terminal scroll step in panel pixels: one text line on the 1080 panel.
fn scroll_line(size: (u32, u32)) -> f64 {
    f64::from(size.0) * 39.0 / 1080.0
}

fn s(v: i32, size: (u32, u32)) -> i32 {
    v * size.0.min(size.1) as i32 / 1080
}

/// The scrollbar's visible track inside the grid well's right edge.
pub fn scrollbar_track(view: &GridView, size: (u32, u32)) -> Rect {
    let a = view.area;
    Rect {
        x: a.x + a.w - s(20, size),
        y: a.y + s(16, size),
        w: s(8, size).max(2),
        h: (a.h - s(32, size)).max(1),
    }
}

/// The thumb: its height is the screen's share of the buffer, its position
/// the display offset (bottom is the live screen).
pub fn scrollbar_thumb(view: &GridView, size: (u32, u32)) -> Option<Rect> {
    if view.history == 0 || view.rows == 0 {
        return None;
    }
    let track = scrollbar_track(view, size);
    let total = (view.history + view.rows) as f64;
    let h = ((f64::from(track.h) * view.rows as f64 / total) as i32)
        .max(s(72, size))
        .min(track.h);
    let travel = f64::from(track.h - h);
    let frac = 1.0 - view.offset.min(view.history) as f64 / view.history as f64;
    Some(Rect {
        y: track.y + (travel * frac).round() as i32,
        h,
        ..track
    })
}

/// The touch strip of the scrollbar: wider than the thumb.
pub fn scrollbar_hit(view: &GridView, size: (u32, u32)) -> Rect {
    let a = view.area;
    let w = s(72, size);
    Rect {
        x: a.x + a.w - w,
        y: a.y,
        w,
        h: a.h,
    }
}

/// The display offset that puts the thumb's top at `thumb_top`.
pub fn offset_for(view: &GridView, size: (u32, u32), thumb_top: f64) -> usize {
    let (Some(thumb), track) = (scrollbar_thumb(view, size), scrollbar_track(view, size)) else {
        return 0;
    };
    let travel = f64::from(track.h - thumb.h);
    if travel <= 0.0 {
        return 0;
    }
    let frac = ((thumb_top - f64::from(track.y)) / travel).clamp(0.0, 1.0);
    ((1.0 - frac) * view.history as f64).round() as usize
}

/// Radius of a drawn selection handle and of its touch target.
pub fn handle_radius(size: (u32, u32)) -> i32 {
    s(18, size).max(4)
}

fn handle_hit_radius(size: (u32, u32)) -> f64 {
    f64::from(s(60, size).max(12))
}

/// Centre of a selection handle: below the start cell's left edge or the
/// end cell's right edge. `None` while that end is scrolled out of view.
pub fn handle_center(
    view: &GridView,
    size: (u32, u32),
    selection: &Selection,
    handle: Handle,
) -> Option<(i32, i32)> {
    let point = match handle {
        Handle::Start => selection.start(),
        Handle::End => selection.end(),
    };
    let row = point.viewport_row(view.offset, view.rows)?;
    let cell = view.cell_rect(row, point.col.min(view.cols.saturating_sub(1)));
    let x = match handle {
        Handle::Start => cell.x,
        Handle::End => cell.x + cell.w,
    };
    Some((x, cell.y + cell.h + handle_radius(size)))
}

/// Copy / Paste / Done along the top of the grid, or along its bottom when
/// the selection starts in the top rows.
pub fn action_rects(
    view: &GridView,
    size: (u32, u32),
    selection: &Selection,
) -> [(Rect, SelAction); 3] {
    let a = view.area;
    let (w, h, gap) = (s(190, size), s(88, size), s(16, size));
    let near_top = selection
        .start()
        .viewport_row(view.offset, view.rows)
        .is_some_and(|row| row < 4);
    let y = if near_top {
        a.y + a.h - h - s(24, size)
    } else {
        a.y + s(24, size)
    };
    let right = a.x + a.w - s(96, size);
    SelAction::ALL.map(|action| {
        let i = match action {
            SelAction::Copy => 2,
            SelAction::Paste => 1,
            SelAction::Done => 0,
        };
        (
            Rect {
                x: right - (i + 1) * w - i * gap,
                y,
                w,
                h,
            },
            action,
        )
    })
}

fn distance(a: (f64, f64), b: (f64, f64)) -> f64 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

fn tap_slop(size: (u32, u32)) -> f64 {
    f64::from(size.0.min(size.1)) * 0.035
}

pub fn hit_test(state: &UiState, position: (f64, f64), size: (u32, u32)) -> Option<Target> {
    let (x, y) = position;
    let (w, h) = (f64::from(size.0), f64::from(size.1));
    if y >= h * 0.92 {
        return Some(Target::Home);
    }
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
    if let Some(selection) = &state.selection {
        if let Some((_, action)) = action_rects(&state.view, size, selection)
            .into_iter()
            .find(|(r, _)| osk::contains(*r, position))
        {
            return Some(Target::Action(action));
        }
        let near = [Handle::End, Handle::Start].into_iter().find(|handle| {
            handle_center(&state.view, size, selection, *handle).is_some_and(|(cx, cy)| {
                distance((f64::from(cx), f64::from(cy)), position) <= handle_hit_radius(size)
            })
        });
        if let Some(handle) = near {
            return Some(Target::Handle(handle));
        }
    }
    if state.scrollbar_visible
        && state.view.history > 0
        && osk::contains(scrollbar_hit(&state.view, size), position)
    {
        return Some(Target::Scrollbar);
    }
    osk::contains(area, position).then_some(Target::TermArea)
}

#[derive(Clone, Copy, Debug)]
struct Contact {
    id: i32,
    start: (f64, f64),
    current: (f64, f64),
    /// Last position at which a scroll step was emitted.
    anchor: (f64, f64),
    target: Option<Target>,
    /// When a still touch on the grid becomes a selection.
    long_press_at: Option<Instant>,
    /// The long press fired: motion extends the selection.
    selecting: bool,
    /// The handle being dragged (it can swap ends mid-drag).
    handle: Option<Handle>,
    /// Finger offset from the thumb's top while dragging the scrollbar.
    grab: f64,
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
        now: Instant,
    ) -> bool {
        if self.contact.is_some() {
            return false;
        }
        let target = hit_test(state, position, size);
        let mut contact = Contact {
            id,
            start: position,
            current: position,
            anchor: position,
            target,
            long_press_at: None,
            selecting: false,
            handle: None,
            grab: 0.0,
        };
        match target {
            Some(Target::Key(key)) => state.repeat.arm(key, now),
            Some(Target::TermArea) => contact.long_press_at = Some(now + LONG_PRESS),
            Some(Target::Handle(handle)) => contact.handle = Some(handle),
            Some(Target::Scrollbar) => {
                state.show_scrollbar(now);
                let thumb = scrollbar_thumb(&state.view, size);
                contact.grab = match thumb {
                    Some(t) if osk::contains(t, position) => position.1 - f64::from(t.y),
                    Some(t) => f64::from(t.h) / 2.0,
                    None => 0.0,
                };
                self.contact = Some(contact);
                self.drag_scrollbar(position, state, size);
                state.pressed = target;
                return true;
            }
            _ => {}
        }
        self.contact = Some(contact);
        // The grid itself has no press feedback; keys and controls do.
        state.pressed = target.filter(|t| *t != Target::TermArea);
        true
    }

    fn drag_scrollbar(&mut self, position: (f64, f64), state: &mut UiState, size: (u32, u32)) {
        let Some(contact) = self.contact.as_ref() else {
            return;
        };
        let offset = offset_for(&state.view, size, position.1 - contact.grab);
        if offset != state.view.offset {
            state.view.offset = offset;
            state.pending.push(TermAction::ScrollTo(offset));
        }
    }

    pub fn motion(
        &mut self,
        id: i32,
        position: (f64, f64),
        state: &mut UiState,
        size: (u32, u32),
        now: Instant,
    ) -> bool {
        let Some(contact) = self.contact.as_mut().filter(|contact| contact.id == id) else {
            return false;
        };
        contact.current = position;
        match contact.target {
            Some(Target::Scrollbar) => {
                state.show_scrollbar(now);
                self.drag_scrollbar(position, state, size);
                return true;
            }
            Some(Target::Handle(_)) => {
                // The finger sits below the handle; aim at the cell above it.
                let lift = f64::from(state.view.cell_h) / 2.0 + f64::from(handle_radius(size));
                let aim = (position.0, position.1 - lift);
                if let (Some(selection), Some(point), Some(handle)) = (
                    state.selection.as_mut(),
                    state.view.point_at(aim),
                    contact.handle,
                ) {
                    let before = *selection;
                    contact.handle = Some(selection.drag(handle, point));
                    return *selection != before;
                }
                return false;
            }
            Some(Target::TermArea) if contact.selecting => {
                if let (Some(selection), Some(point)) =
                    (state.selection.as_mut(), state.view.point_at(position))
                {
                    let before = *selection;
                    selection.extend(point);
                    return *selection != before;
                }
                return false;
            }
            Some(Target::TermArea) => {
                if distance(contact.start, position) >= tap_slop(size) {
                    contact.long_press_at = None;
                }
                // Dragging down reveals history, one step per text line.
                let step = scroll_line(size);
                let lines = ((position.1 - contact.anchor.1) / step).trunc() as i32;
                if lines != 0 {
                    contact.long_press_at = None;
                    contact.anchor.1 += f64::from(lines) * step;
                    state.pending.push(TermAction::Scroll(lines));
                    state.show_scrollbar(now);
                    return true;
                }
                return false;
            }
            _ => {}
        }
        let target = hit_test(state, position, size);
        let pressed = if distance(contact.start, position) < tap_slop(size) {
            contact
                .target
                .filter(|candidate| Some(*candidate) == target)
        } else {
            None
        };
        if pressed.is_none() && state.repeat.key().is_some() {
            state.repeat.cancel();
        }
        let changed = state.pressed != pressed;
        state.pressed = pressed;
        changed
    }

    pub fn up(&mut self, id: i32, state: &mut UiState, size: (u32, u32), now: Instant) -> bool {
        let Some(contact) = self.contact.filter(|contact| contact.id == id) else {
            return false;
        };
        self.contact = None;
        state.pressed = None;
        let repeated = state.repeat.release();
        match contact.target {
            Some(Target::Scrollbar) => {
                state.show_scrollbar(now);
                return true;
            }
            Some(Target::Handle(_)) => return true,
            Some(Target::TermArea) if contact.selecting => return true,
            _ => {}
        }
        let (w, h) = (f64::from(size.0), f64::from(size.1));
        let dx = contact.current.0 - contact.start.0;
        let dy = contact.current.1 - contact.start.1;
        let swipe = w.min(h) * 0.12;

        let from_left = contact.start.0 <= w * 0.08 && dx > swipe;
        let from_right = contact.start.0 >= w * 0.92 && -dx > swipe;
        if (from_left || from_right) && dx.abs() > dy.abs() {
            state.back();
        } else if distance(contact.start, contact.current) < tap_slop(size)
            && let Some(target) = contact.target
            && hit_test(state, contact.current, size) == Some(target)
        {
            match target {
                Target::TermArea => state.selection = None,
                Target::Key(_) if repeated => {}
                _ => state.activate(target),
            }
        }
        true
    }

    pub fn cancel(&mut self, state: &mut UiState) -> bool {
        state.repeat.cancel();
        self.contact.take().is_some() || state.pressed.take().is_some()
    }

    /// The next instant [`Input::timer`] has work.
    pub fn deadline(&self, state: &UiState) -> Option<Instant> {
        [
            state.repeat.deadline(),
            self.contact.and_then(|c| c.long_press_at),
            state.scrollbar_until,
            state.bell_until,
            state.notice_until,
        ]
        .into_iter()
        .flatten()
        .min()
    }

    /// Runs whatever is due at `now`: key repeats, the long press (which
    /// selects the word under the finger in `snapshot`), and expiring the
    /// scrollbar, bell flash, and notice. Returns whether to redraw.
    pub fn timer(
        &mut self,
        state: &mut UiState,
        snapshot: Option<&Snapshot>,
        now: Instant,
    ) -> bool {
        let mut redraw = false;
        if let Some((key, count)) = state.repeat.poll(now) {
            for _ in 0..count {
                state.type_key(key);
            }
            redraw = true;
        }
        if let Some(contact) = self.contact.as_mut()
            && contact.long_press_at.is_some_and(|at| at <= now)
        {
            contact.long_press_at = None;
            if let Some((row, col)) = state.view.cell_at(contact.current) {
                let line = snapshot.map(|s| s.row_chars(row)).unwrap_or_default();
                let at = Point::from_viewport(row, col, state.view.offset);
                state.selection = Some(Selection::word(&line, at));
                contact.selecting = true;
                redraw = true;
            }
        }
        if state.scrollbar_until.is_some_and(|at| at <= now) && self.scrollbar_drag().is_none() {
            state.scrollbar_until = None;
            state.scrollbar_visible = false;
            redraw = true;
        }
        if state.bell_until.is_some_and(|at| at <= now) {
            state.bell_until = None;
            state.bell = false;
            redraw = true;
        }
        if state.notice_until.is_some_and(|at| at <= now) {
            state.notice_until = None;
            state.notice = None;
            redraw = true;
        }
        redraw
    }

    fn scrollbar_drag(&self) -> Option<i32> {
        self.contact
            .filter(|c| c.target == Some(Target::Scrollbar))
            .map(|c| c.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: (u32, u32) = (1080, 2340);

    fn center(keys: &[(ferrix_ui::draw::Rect, Key)], wanted: Key) -> (f64, f64) {
        let (r, _) = keys
            .iter()
            .find(|(_, k)| *k == wanted)
            .expect("key on layout");
        (f64::from(r.x + r.w / 2), f64::from(r.y + r.h / 2))
    }

    fn mid(r: Rect) -> (f64, f64) {
        (f64::from(r.x + r.w / 2), f64::from(r.y + r.h / 2))
    }

    /// The 1080x2340 grid geometry the renderer reports (55x32 cells).
    fn view(history: usize, offset: usize) -> GridView {
        let area = osk::terminal_area(SIZE);
        GridView {
            area,
            origin: (area.x + 16, area.y + 16),
            cell_w: 18,
            cell_h: 39,
            cols: 55,
            rows: 32,
            history,
            offset,
        }
    }

    fn state_with(history: usize) -> UiState {
        let mut state = UiState::new();
        state.view = view(history, 0);
        state
    }

    fn ms(t0: Instant, v: u64) -> Instant {
        t0 + Duration::from_millis(v)
    }

    #[test]
    fn keys_queue_shell_input_and_drags_scroll_history() {
        let t0 = Instant::now();
        let mut state = state_with(100);
        let mut input = Input::default();
        let keys = osk::keys(SIZE, &state.osk);
        for key in [Key::Char('l'), Key::Char('s'), Key::Enter] {
            input.down(0, center(&keys, key), &mut state, SIZE, t0);
            assert_eq!(state.pressed, Some(Target::Key(key)));
            input.up(0, &mut state, SIZE, t0);
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
        input.down(0, center(&keys, Key::Ctrl), &mut state, SIZE, t0);
        input.up(0, &mut state, SIZE, t0);
        input.down(0, center(&keys, Key::Char('c')), &mut state, SIZE, t0);
        input.up(0, &mut state, SIZE, t0);
        assert_eq!(state.pending, vec![TermAction::Input(vec![0x03])]);
        state.pending.clear();

        // Shift is sticky for one letter too.
        input.down(0, center(&keys, Key::Shift), &mut state, SIZE, t0);
        input.up(0, &mut state, SIZE, t0);
        assert!(state.osk.shift);
        let shifted = osk::keys(SIZE, &state.osk);
        input.down(0, center(&shifted, Key::Char('a')), &mut state, SIZE, t0);
        input.up(0, &mut state, SIZE, t0);
        assert_eq!(state.pending, vec![TermAction::Input(b"A".to_vec())]);
        assert!(!state.osk.shift);
        state.pending.clear();

        // Dragging down inside the grid scrolls into history without
        // pressing, and reveals the scrollbar.
        let area = osk::terminal_area(SIZE);
        let x = f64::from(area.x + area.w / 2);
        let y = f64::from(area.y + area.h / 2);
        input.down(0, (x, y), &mut state, SIZE, t0);
        assert_eq!(state.pressed, None);
        assert!(!state.scrollbar_visible);
        input.motion(0, (x, y + 200.0), &mut state, SIZE, t0);
        input.up(0, &mut state, SIZE, t0);
        assert_eq!(state.pending, vec![TermAction::Scroll(5)]);
        assert!(state.scrollbar_visible);
        assert_eq!(state.selection, None, "a drag is not a long press");
    }

    #[test]
    fn held_backspace_repeats_then_release_does_not_add_one() {
        let t0 = Instant::now();
        let mut state = state_with(0);
        let mut input = Input::default();
        let keys = osk::keys(SIZE, &state.osk);
        let bs = center(&keys, Key::Backspace);
        input.down(0, bs, &mut state, SIZE, t0);
        let delay = state.repeat.deadline().expect("armed");
        assert_eq!(input.deadline(&state), Some(delay));
        assert!(!input.timer(&mut state, None, delay - Duration::from_millis(1)));
        assert!(input.timer(&mut state, None, delay));
        assert!(input.timer(&mut state, None, delay + Duration::from_millis(60)));
        input.up(0, &mut state, SIZE, delay + Duration::from_millis(70));
        assert_eq!(state.pending, vec![TermAction::Input(vec![0x7f]); 2]);
        state.pending.clear();

        // A quick tap types once; letters never repeat.
        input.down(0, bs, &mut state, SIZE, t0);
        input.up(0, &mut state, SIZE, ms(t0, 100));
        input.down(0, center(&keys, Key::Char('q')), &mut state, SIZE, t0);
        assert_eq!(input.deadline(&state), None);
        input.up(0, &mut state, SIZE, ms(t0, 2000));
        assert_eq!(
            state.pending,
            vec![
                TermAction::Input(vec![0x7f]),
                TermAction::Input(b"q".to_vec())
            ]
        );
        state.pending.clear();

        // Sliding off a held arrow stops it without typing.
        let left = center(&keys, Key::Left);
        input.down(0, left, &mut state, SIZE, t0);
        input.motion(0, (left.0, left.1 - 400.0), &mut state, SIZE, t0);
        assert_eq!(state.repeat.deadline(), None);
        assert!(!input.timer(&mut state, None, ms(t0, 1000)));
        input.up(0, &mut state, SIZE, ms(t0, 1000));
        assert!(state.pending.is_empty());
    }

    #[test]
    fn long_press_selects_a_word_and_handles_and_bar_act_on_it() {
        let t0 = Instant::now();
        let snap = Snapshot::from_bytes(55, 32, b"root@ferrix:~# cat /etc/hostname\r\nqemu-guest");
        let mut state = state_with(0);
        let mut input = Input::default();
        let v = state.view;
        // Press on "/etc/hostname" (row 0, col 22) and hold.
        let on = mid(v.cell_rect(0, 22));
        input.down(0, on, &mut state, SIZE, t0);
        assert_eq!(input.deadline(&state), Some(t0 + LONG_PRESS));
        assert!(!input.timer(&mut state, Some(&snap), ms(t0, 499)));
        assert!(input.timer(&mut state, Some(&snap), ms(t0, 500)));
        let selection = state.selection.expect("selected");
        assert_eq!(selection.range(), (Point::new(0, 19), Point::new(0, 31)));
        // Keep dragging: the selection extends to the next line.
        input.motion(0, mid(v.cell_rect(1, 3)), &mut state, SIZE, ms(t0, 600));
        assert_eq!(
            state.selection.unwrap().range(),
            (Point::new(0, 19), Point::new(1, 3))
        );
        input.up(0, &mut state, SIZE, ms(t0, 700));
        assert!(state.selection.is_some(), "lifting keeps the selection");
        assert!(state.pending.is_empty(), "nothing typed or scrolled");

        // Drag the end handle back onto row 0; the handle sits below the cell.
        let sel = state.selection.unwrap();
        let (hx, hy) = handle_center(&state.view, SIZE, &sel, Handle::End).unwrap();
        let grab = (f64::from(hx), f64::from(hy));
        assert_eq!(
            hit_test(&state, grab, SIZE),
            Some(Target::Handle(Handle::End))
        );
        input.down(0, grab, &mut state, SIZE, ms(t0, 800));
        let lift = f64::from(v.cell_h) / 2.0 + f64::from(handle_radius(SIZE));
        let target_cell = mid(v.cell_rect(0, 25));
        input.motion(
            0,
            (target_cell.0, target_cell.1 + lift),
            &mut state,
            SIZE,
            ms(t0, 850),
        );
        input.up(0, &mut state, SIZE, ms(t0, 900));
        assert_eq!(
            state.selection.unwrap().range(),
            (Point::new(0, 19), Point::new(0, 25))
        );

        // Copy queues the selection and clears it.
        let sel = state.selection.unwrap();
        let (copy, _) = action_rects(&state.view, SIZE, &sel)
            .into_iter()
            .find(|(_, a)| *a == SelAction::Copy)
            .unwrap();
        input.down(0, mid(copy), &mut state, SIZE, ms(t0, 1000));
        assert_eq!(state.pressed, Some(Target::Action(SelAction::Copy)));
        input.up(0, &mut state, SIZE, ms(t0, 1050));
        assert_eq!(state.pending, vec![TermAction::Copy(sel)]);
        assert_eq!(state.selection, None);
        state.pending.clear();

        // Long-press again, then Paste; then a plain tap clears a selection.
        input.down(0, on, &mut state, SIZE, ms(t0, 2000));
        input.timer(&mut state, Some(&snap), ms(t0, 2500));
        input.up(0, &mut state, SIZE, ms(t0, 2600));
        let sel = state.selection.unwrap();
        let (paste, _) = action_rects(&state.view, SIZE, &sel)
            .into_iter()
            .find(|(_, a)| *a == SelAction::Paste)
            .unwrap();
        input.down(0, mid(paste), &mut state, SIZE, ms(t0, 2700));
        input.up(0, &mut state, SIZE, ms(t0, 2750));
        assert_eq!(state.pending, vec![TermAction::Paste]);
        input.down(0, on, &mut state, SIZE, ms(t0, 3000));
        input.timer(&mut state, Some(&snap), ms(t0, 3500));
        input.up(0, &mut state, SIZE, ms(t0, 3600));
        assert!(state.selection.is_some());
        let blank = mid(v.cell_rect(20, 10));
        input.down(0, blank, &mut state, SIZE, ms(t0, 4000));
        input.up(0, &mut state, SIZE, ms(t0, 4100));
        assert_eq!(state.selection, None, "tap clears");
    }

    #[test]
    fn selection_follows_output_into_history() {
        let mut state = state_with(10);
        state.selection = Some(Selection::new(Point::new(5, 0), Point::new(5, 4)));
        state.sync(view(13, 0));
        assert_eq!(
            state.selection.unwrap().range(),
            (Point::new(2, 0), Point::new(2, 4))
        );
        // A resize drops it; so does scrolling past the history cap's reach.
        state.sync(GridView {
            rows: 20,
            ..view(13, 0)
        });
        assert_eq!(state.selection, None);
    }

    #[test]
    fn scrollbar_is_transient_and_draggable() {
        let t0 = Instant::now();
        let mut state = state_with(200);
        let mut input = Input::default();
        let strip = scrollbar_hit(&state.view, SIZE);
        let at = mid(strip);
        assert_eq!(
            hit_test(&state, at, SIZE),
            Some(Target::TermArea),
            "hidden bar is not a target"
        );
        state.show_scrollbar(t0);
        assert_eq!(hit_test(&state, at, SIZE), Some(Target::Scrollbar));
        let thumb = scrollbar_thumb(&state.view, SIZE).unwrap();
        let track = scrollbar_track(&state.view, SIZE);
        assert_eq!(
            thumb.y + thumb.h,
            track.y + track.h,
            "live screen at the bottom"
        );
        assert!(thumb.h < track.h / 4, "32 of 232 lines");
        // Grab the thumb and drag it to the top: oldest history.
        let grab = (f64::from(thumb.x), f64::from(thumb.y + 4));
        input.down(0, grab, &mut state, SIZE, ms(t0, 100));
        input.motion(
            0,
            (grab.0, f64::from(track.y) - 50.0),
            &mut state,
            SIZE,
            ms(t0, 200),
        );
        assert_eq!(state.pending.last(), Some(&TermAction::ScrollTo(200)));
        // Halfway down the travel: about half of history.
        let travel = f64::from(track.h - thumb.h);
        input.motion(
            0,
            (grab.0, f64::from(track.y) + travel / 2.0 + 4.0),
            &mut state,
            SIZE,
            ms(t0, 300),
        );
        assert_eq!(state.pending.last(), Some(&TermAction::ScrollTo(100)));
        // Held past the linger: it stays while dragged, and no back swipe.
        input.timer(&mut state, None, ms(t0, 5000));
        assert!(state.scrollbar_visible);
        input.motion(
            0,
            (grab.0 - 400.0, f64::from(track.y)),
            &mut state,
            SIZE,
            ms(t0, 5100),
        );
        input.up(0, &mut state, SIZE, ms(t0, 5200));
        assert_eq!(state.pending_back, 0);
        // It fades after the linger.
        assert!(input.timer(&mut state, None, ms(t0, 5200) + SCROLLBAR_LINGER));
        assert!(!state.scrollbar_visible);
        assert_eq!(
            offset_for(&state.view, SIZE, f64::from(track.y + track.h)),
            0
        );
    }

    #[test]
    fn bell_flash_and_notice_expire() {
        let t0 = Instant::now();
        let mut state = state_with(0);
        let mut input = Input::default();
        state.ring(t0);
        state.note("copied \u{b7} 5 chars".to_owned(), t0);
        assert!(state.bell && state.notice.is_some());
        assert_eq!(input.deadline(&state), Some(t0 + BELL_FLASH));
        assert!(input.timer(&mut state, None, t0 + BELL_FLASH));
        assert!(!state.bell && state.notice.is_some());
        assert!(input.timer(&mut state, None, t0 + NOTICE));
        assert_eq!(state.notice, None);
        assert_eq!(input.deadline(&state), None);
    }

    #[test]
    fn back_header_and_side_edge_swipes_request_yield() {
        let t0 = Instant::now();
        let mut state = state_with(0);
        let mut input = Input::default();
        assert_eq!(hit_test(&state, (80.0, 160.0), SIZE), Some(Target::Back));
        input.down(0, (80.0, 160.0), &mut state, SIZE, t0);
        assert_eq!(state.pressed, Some(Target::Back));
        input.up(0, &mut state, SIZE, t0);
        assert_eq!(state.pending_back, 1);

        input.down(0, (20.0, 900.0), &mut state, SIZE, t0);
        input.motion(0, (300.0, 900.0), &mut state, SIZE, t0);
        input.up(0, &mut state, SIZE, t0);
        assert_eq!(state.pending_back, 2);

        input.down(0, (1060.0, 900.0), &mut state, SIZE, t0);
        input.motion(0, (780.0, 900.0), &mut state, SIZE, t0);
        input.up(0, &mut state, SIZE, t0);
        assert_eq!(state.pending_back, 3);
        assert!(state.pending.is_empty(), "a swipe types nothing");
    }

    #[test]
    fn bottom_zone_belongs_to_the_compositor() {
        let t0 = Instant::now();
        let mut state = state_with(0);
        let mut input = Input::default();
        assert_eq!(hit_test(&state, (540.0, 2290.0), SIZE), Some(Target::Home));
        input.down(0, (540.0, 2290.0), &mut state, SIZE, t0);
        assert_eq!(state.pressed, Some(Target::Home));
        input.motion(0, (540.0, 1900.0), &mut state, SIZE, t0);
        input.up(0, &mut state, SIZE, t0);
        assert_eq!(state.pending_back, 0);
        assert!(state.pending.is_empty());
        assert_eq!(state.pressed, None);
    }
}
