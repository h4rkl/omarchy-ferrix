//! Geometry and input for Files; rectangles are shared by the renderer and
//! the hit test, so interaction tests exercise what the framebuffer shows.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use ferrix_ui::draw::Rect;
use std::time::Instant;

use ferrix_ui::gesture::{self, Gesture, Tracker};
use ferrix_ui::keyboard::KeyRepeat;
use ferrix_ui::osk::{self, Key};
use ferrix_ui::widgets::{Edit, back_rect, basis, contains, edit_text, list_rects, scale};

use crate::model::{Model, Page};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Back,
    Home,
    /// Breadcrumb to the folder at this depth (0 is home).
    Crumb(usize),
    /// Open the entry at this listing index.
    Row(usize),
    /// The entry's details page (the trailing `···` of a row).
    RowMore(usize),
    /// The list's empty space: a drag surface, never a tap.
    ListArea,
    NewFolder,
    Refresh,
    Open,
    Rename,
    Delete,
    ConfirmDelete,
    Cancel,
    Field,
    Save,
    /// The viewer's text: a drag surface.
    ViewArea,
    Key(Key),
}

/// One breadcrumb chip slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Crumb {
    Depth(usize),
    /// Leading folders that did not fit.
    Elided,
}

/// Longest breadcrumb label before it is shortened.
const CRUMB_CHARS: usize = 14;
/// JetBrains Mono advances 600/1000 em; chip text is 0.42 of chip height.
const MONO_ADVANCE: f32 = 0.6;

pub struct Layout {
    pub size: (u32, u32),
    pub b: i32,
}

impl Layout {
    pub fn new(size: (u32, u32)) -> Self {
        Self {
            size,
            b: basis(size),
        }
    }

    pub fn s(&self, v: i32) -> i32 {
        scale(v, self.b)
    }

    fn w(&self) -> i32 {
        self.size.0 as i32
    }

    fn h(&self) -> i32 {
        self.size.1 as i32
    }

    pub fn osk_top(&self) -> i32 {
        osk::extent(self.size).0 - self.s(12)
    }

    // Browse ---------------------------------------------------------------

    fn crumb_h(&self) -> i32 {
        self.s(64)
    }

    pub fn crumb_y(&self) -> i32 {
        self.s(282)
    }

    /// Gap between chips; the renderer draws a `/` in it.
    pub fn crumb_gap(&self) -> i32 {
        self.s(40)
    }

    /// Breadcrumb chips with their labels. Leading folders collapse into one
    /// `…` chip when the path is wider than the panel; home and the current
    /// folder always show.
    pub fn crumb_rects(&self, labels: &[String]) -> Vec<(Rect, Crumb, String)> {
        let h = self.crumb_h();
        let advance = MONO_ADVANCE * (h as f32 * 0.42).max(12.0);
        let shown: Vec<String> = labels.iter().map(|l| crumb_label(l)).collect();
        let width = |label: &str| {
            ((label.chars().count() as f32 * advance).ceil() as i32 + self.s(40)).max(self.s(96))
        };
        let available = self.w() - 2 * self.s(48);
        let gap = self.crumb_gap();
        let total: i32 =
            shown.iter().map(|l| width(l)).sum::<i32>() + gap * (shown.len() as i32 - 1).max(0);
        let mut slots: Vec<(Crumb, String)> = Vec::new();
        if total <= available || shown.len() <= 2 {
            slots.extend(
                shown
                    .iter()
                    .cloned()
                    .enumerate()
                    .map(|(d, l)| (Crumb::Depth(d), l)),
            );
        } else {
            let ellipsis = "\u{2026}".to_owned();
            let mut used = width(&shown[0]) + gap + width(&ellipsis);
            let mut tail = Vec::new();
            for depth in (1..shown.len()).rev() {
                let w = gap + width(&shown[depth]);
                if !tail.is_empty() && used + w > available {
                    break;
                }
                used += w;
                tail.push((Crumb::Depth(depth), shown[depth].clone()));
            }
            tail.reverse();
            slots.push((Crumb::Depth(0), shown[0].clone()));
            if tail.first().is_some_and(|(c, _)| *c != Crumb::Depth(1)) {
                slots.push((Crumb::Elided, ellipsis));
            }
            slots.extend(tail);
        }
        let mut x = self.s(48);
        slots
            .into_iter()
            .map(|(crumb, label)| {
                let w = width(&label).min(available);
                let r = Rect {
                    x,
                    y: self.crumb_y(),
                    w,
                    h,
                };
                x += w + gap;
                (r, crumb, label)
            })
            .collect()
    }

    pub fn list_top(&self) -> i32 {
        self.s(380)
    }

    pub fn row_step(&self) -> i32 {
        self.s(152)
    }

    fn list_bottom(&self) -> i32 {
        self.notice_rect().y - self.s(24)
    }

    /// Rows that fit between the breadcrumb and the status line.
    pub fn capacity(&self) -> usize {
        ((self.list_bottom() - self.list_top() + self.s(12)) / self.row_step()).max(1) as usize
    }

    pub fn row_rects(&self, visible: usize) -> Vec<Rect> {
        list_rects(
            self.size,
            self.list_top(),
            visible.min(self.capacity()),
            140,
        )
    }

    pub fn list_area(&self) -> Rect {
        Rect {
            x: 0,
            y: self.list_top(),
            w: self.w(),
            h: self.list_bottom() - self.list_top(),
        }
    }

    /// The scroll rail, in the right margin beside the rows.
    pub fn rail_track(&self) -> Rect {
        Rect {
            x: self.w() - self.s(30),
            y: self.list_top(),
            w: self.s(6).max(2),
            h: self.capacity() as i32 * self.row_step() - self.s(12),
        }
    }

    /// Trailing hit area of a row that opens its details page.
    pub fn more_rect(&self, row: Rect) -> Rect {
        let w = self.s(200);
        Rect {
            x: row.x + row.w - w,
            y: row.y,
            w,
            h: row.h,
        }
    }

    pub fn new_folder_rect(&self) -> Rect {
        Rect {
            x: self.s(96),
            y: self.h() * 92 / 100 - self.s(180),
            w: self.s(420),
            h: self.s(140),
        }
    }

    pub fn refresh_rect(&self) -> Rect {
        Rect {
            x: self.s(564),
            ..self.new_folder_rect()
        }
    }

    pub fn notice_rect(&self) -> Rect {
        Rect {
            x: self.s(48),
            y: self.new_folder_rect().y - self.s(130),
            w: self.w() - self.s(96),
            h: self.s(96),
        }
    }

    // Item and confirmation ------------------------------------------------

    pub fn open_rect(&self) -> Rect {
        Rect {
            x: self.s(96),
            y: self.s(1530),
            w: self.w() - 2 * self.s(96),
            h: self.s(150),
        }
    }

    pub fn rename_rect(&self) -> Rect {
        Rect {
            x: self.s(96),
            y: self.s(1710),
            w: self.s(420),
            h: self.s(150),
        }
    }

    pub fn delete_rect(&self) -> Rect {
        Rect {
            x: self.s(564),
            ..self.rename_rect()
        }
    }

    pub fn item_notice_rect(&self) -> Rect {
        Rect {
            x: self.s(48),
            y: self.s(1400),
            w: self.w() - self.s(96),
            h: self.s(96),
        }
    }

    pub fn confirm_rect(&self) -> Rect {
        self.rename_rect()
    }

    pub fn cancel_rect(&self) -> Rect {
        self.delete_rect()
    }

    // Forms ----------------------------------------------------------------

    pub fn field_rect(&self) -> Rect {
        Rect {
            x: self.s(48),
            y: self.s(360),
            w: self.w() - self.s(96),
            h: self.s(100),
        }
    }

    pub fn save_rect(&self) -> Rect {
        Rect {
            x: self.s(96),
            y: self.osk_top() - self.s(20) - self.s(130),
            w: self.w() - 2 * self.s(96),
            h: self.s(130),
        }
    }

    pub fn form_notice_rect(&self) -> Rect {
        Rect {
            x: self.s(48),
            y: self.save_rect().y - self.s(120),
            w: self.w() - self.s(96),
            h: self.s(96),
        }
    }

    // Viewer ---------------------------------------------------------------

    pub fn view_area(&self) -> Rect {
        let top = self.s(330);
        Rect {
            x: self.s(48),
            y: top,
            w: self.w() - self.s(96),
            h: self.h() * 92 / 100 - self.s(24) - top,
        }
    }

    pub fn view_px(&self) -> i32 {
        self.s(28)
    }

    pub fn view_line(&self) -> i32 {
        self.s(40)
    }

    pub fn view_pad(&self) -> i32 {
        self.s(24)
    }

    /// Monospace columns per wrapped line (the rail keeps its own strip).
    pub fn view_cols(&self) -> usize {
        let usable = self.view_area().w - 2 * self.view_pad() - self.s(16);
        ((usable as f32 / (self.view_px() as f32 * MONO_ADVANCE)).floor() as usize).max(1)
    }

    pub fn view_rows(&self) -> usize {
        ((self.view_area().h - 2 * self.view_pad()) / self.view_line()).max(1) as usize
    }

    pub fn view_rail(&self) -> Rect {
        let a = self.view_area();
        Rect {
            x: a.x + a.w - self.s(18),
            y: a.y + self.view_pad(),
            w: self.s(6).max(2),
            h: a.h - 2 * self.view_pad(),
        }
    }
}

/// A breadcrumb label shortened to [`CRUMB_CHARS`].
pub fn crumb_label(text: &str) -> String {
    if text.chars().count() <= CRUMB_CHARS {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(CRUMB_CHARS - 1).collect();
    out.push('\u{2026}');
    out
}

/// Hard-wraps text to `cols` monospace columns: tabs become four spaces,
/// other control characters a visible `?`, `\r` is dropped.
pub fn wrap_lines(text: &str, cols: usize) -> Vec<String> {
    let cols = cols.max(1);
    let mut lines = Vec::new();
    if text.is_empty() {
        return lines;
    }
    for raw in text.split('\n') {
        let mut line = String::new();
        let mut n = 0;
        for ch in raw.chars() {
            let expanded: &[char] = match ch {
                '\r' => &[],
                '\t' => &[' ', ' ', ' ', ' '],
                c if c.is_control() => &['?'],
                _ => std::slice::from_ref(&ch),
            };
            for &c in expanded {
                if n == cols {
                    lines.push(std::mem::take(&mut line));
                    n = 0;
                }
                line.push(c);
                n += 1;
            }
        }
        lines.push(line);
    }
    // A trailing newline does not add an empty last line.
    if text.ends_with('\n') {
        lines.pop();
    }
    lines
}

/// Wrapped lines of the open file, or none when it is not text.
pub fn view_lines(model: &Model, l: &Layout) -> Vec<String> {
    match &model.file {
        Some(Ok(file)) => file
            .text
            .as_deref()
            .map(|t| wrap_lines(t, l.view_cols()))
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

pub fn keyboard_visible(model: &Model) -> bool {
    model.page.has_form()
}

fn osk_hit(model: &Model, position: (f64, f64), size: (u32, u32)) -> Option<Target> {
    osk::keys(size, &model.osk)
        .into_iter()
        .find(|(r, _)| osk::contains(*r, position))
        .map(|(_, k)| Target::Key(k))
}

pub fn hit_test(model: &Model, position: (f64, f64), size: (u32, u32)) -> Option<Target> {
    if gesture::in_home_zone(position, size) {
        return Some(Target::Home);
    }
    if contains(back_rect(size), position) {
        return Some(Target::Back);
    }
    let l = Layout::new(size);
    if keyboard_visible(model)
        && let Some(t) = osk_hit(model, position, size)
    {
        return Some(t);
    }
    match model.page {
        Page::Browse => {
            for (r, crumb, _) in l.crumb_rects(&model.crumbs()) {
                if let Crumb::Depth(d) = crumb
                    && d < model.cwd.len()
                    && contains(r, position)
                {
                    return Some(Target::Crumb(d));
                }
            }
            if model.writable() && contains(l.new_folder_rect(), position) {
                return Some(Target::NewFolder);
            }
            if contains(l.refresh_rect(), position) {
                return Some(Target::Refresh);
            }
            let window = model.scroll.window(model.entries().len(), l.capacity());
            for (index, r) in window.clone().zip(l.row_rects(window.len())) {
                if contains(r, position) {
                    return Some(if contains(l.more_rect(r), position) {
                        Target::RowMore(index)
                    } else {
                        Target::Row(index)
                    });
                }
            }
            contains(l.list_area(), position).then_some(Target::ListArea)
        }
        Page::View => contains(l.view_area(), position).then_some(Target::ViewArea),
        Page::Item => {
            model.selected_entry()?;
            if contains(l.open_rect(), position) {
                return Some(Target::Open);
            }
            if contains(l.rename_rect(), position) {
                return Some(Target::Rename);
            }
            contains(l.delete_rect(), position).then_some(Target::Delete)
        }
        Page::NewFolder | Page::Rename => {
            if contains(l.field_rect(), position) {
                return Some(Target::Field);
            }
            contains(l.save_rect(), position).then_some(Target::Save)
        }
        Page::ConfirmDelete => {
            if contains(l.confirm_rect(), position) {
                return Some(Target::ConfirmDelete);
            }
            contains(l.cancel_rect(), position).then_some(Target::Cancel)
        }
    }
}

pub fn activate(model: &mut Model, target: Target) {
    match target {
        Target::Back | Target::Cancel => model.back(),
        Target::Home | Target::ListArea | Target::ViewArea | Target::Field => {}
        Target::Crumb(depth) => model.crumb(depth),
        Target::Row(index) => model.open_index(index),
        Target::RowMore(index) => model.inspect(index),
        Target::NewFolder => model.new_folder(),
        Target::Refresh => {
            model.refresh();
            model.notice = None;
        }
        Target::Open => {
            if let Some(name) = model.selected.clone() {
                model.open(&name);
            }
        }
        Target::Rename => model.rename_selected(),
        Target::Delete => model.ask_delete(),
        Target::ConfirmDelete => model.confirm_delete(),
        Target::Save => {
            if model.can_submit() {
                model.submit();
            } else {
                model.form_edited = true;
            }
        }
        Target::Key(key) => {
            key_press(model, key);
        }
    }
}

/// Feeds one key to the form, or treats Escape as Back elsewhere. Returns
/// whether anything changed.
pub fn key_press(model: &mut Model, key: Key) -> bool {
    if !model.page.has_form() {
        if key == Key::Esc {
            model.back();
            return true;
        }
        return false;
    }
    match edit_text(&mut model.form, key, &mut model.osk) {
        Edit::Submit => {
            if model.can_submit() {
                model.submit();
            } else {
                model.form_edited = true;
            }
        }
        Edit::Dismiss => model.back(),
        Edit::Changed => {
            model.form_edited = true;
            model.notice = None;
        }
        Edit::Unchanged => {}
    }
    true
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Drag {
    List,
    View,
}

#[derive(Default)]
pub struct Input {
    tracker: Tracker<Target>,
    /// Repeat timer for a held on-screen key.
    repeat: KeyRepeat,
    drag: Option<Drag>,
    pub pressed: Option<Target>,
}

impl Input {
    pub fn down(&mut self, id: i32, position: (f64, f64), model: &Model, size: (u32, u32)) -> bool {
        let target = hit_test(model, position, size);
        match self.tracker.down(id, position, target) {
            Some(pressed) => {
                if let Some(Target::Key(key)) = pressed {
                    self.repeat.arm(key, Instant::now());
                }
                self.drag = match target {
                    Some(Target::Row(_) | Target::RowMore(_) | Target::ListArea) => {
                        Some(Drag::List)
                    }
                    Some(Target::ViewArea) => Some(Drag::View),
                    _ => None,
                };
                // Drag surfaces have no press feedback of their own.
                self.pressed =
                    pressed.filter(|t| !matches!(t, Target::ListArea | Target::ViewArea));
                true
            }
            None => false,
        }
    }

    pub fn motion(
        &mut self,
        id: i32,
        position: (f64, f64),
        model: &mut Model,
        size: (u32, u32),
    ) -> bool {
        let at = hit_test(model, position, size);
        let mut redraw = match self.tracker.motion(id, position, size, at, self.pressed) {
            Some(pressed) => {
                if pressed.is_none() {
                    self.repeat.cancel();
                }
                self.pressed =
                    pressed.filter(|t| !matches!(t, Target::ListArea | Target::ViewArea));
                true
            }
            None => false,
        };
        let l = Layout::new(size);
        match self.drag {
            Some(Drag::List) => {
                let steps = self.tracker.drag_steps(id, f64::from(l.row_step()));
                // Dragging up reveals later rows.
                redraw |= steps != 0
                    && model
                        .scroll
                        .scroll_by(-steps, model.entries().len(), l.capacity());
            }
            Some(Drag::View) => {
                let steps = self.tracker.drag_steps(id, f64::from(l.view_line()));
                let count = view_lines(model, &l).len();
                redraw |= steps != 0 && model.view_scroll.scroll_by(-steps, count, l.view_rows());
            }
            None => {}
        }
        redraw
    }

    pub fn up(
        &mut self,
        id: i32,
        position: Option<(f64, f64)>,
        model: &mut Model,
        size: (u32, u32),
    ) -> bool {
        let at = position
            .or_else(|| self.tracker.current(id))
            .and_then(|p| hit_test(model, p, size));
        let Some(gesture) = self.tracker.up(id, size, at) else {
            return false;
        };
        self.pressed = None;
        self.drag = None;
        let repeated = self.repeat.release();
        match gesture {
            // A key that already repeated does not fire again on release.
            Gesture::Tap(Target::Key(_)) if repeated => {}
            Gesture::Tap(target) => activate(model, target),
            Gesture::BackSwipe => model.back(),
            Gesture::PullDown | Gesture::Release => {}
        }
        true
    }

    pub fn cancel(&mut self) -> bool {
        self.drag = None;
        self.repeat.cancel();
        let had = self.tracker.cancel();
        had || self.pressed.take().is_some()
    }

    /// When a held key next repeats.
    pub fn deadline(&self) -> Option<Instant> {
        self.repeat.deadline()
    }

    /// Delivers due key repeats to the focused field.
    pub fn timer(&mut self, model: &mut Model, now: Instant) -> bool {
        let Some((key, count)) = self.repeat.poll(now) else {
            return false;
        };
        for _ in 0..count {
            key_press(model, key);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Action, Notice};
    use crate::store::Home;
    use crate::store::testdir::TestDir;
    use ferrix_ui::widgets::center;
    use jiff::tz::TimeZone;
    use std::fs;

    const SIZE: (u32, u32) = (1080, 2340);

    fn model(files: usize) -> (TestDir, Model) {
        let dir = TestDir::new();
        let home = dir.path().join("home");
        fs::create_dir_all(home.join("Documents")).unwrap();
        for i in 0..files {
            fs::write(home.join(format!("file-{i:02}.txt")), format!("line {i}\n")).unwrap();
        }
        let m = Model::new(Home::new(&home), TimeZone::UTC);
        (dir, m)
    }

    fn tap(input: &mut Input, model: &mut Model, at: (f64, f64)) {
        input.down(0, at, model, SIZE);
        input.up(0, None, model, SIZE);
    }

    fn drag(input: &mut Input, model: &mut Model, from: (f64, f64), dy: f64) {
        input.down(0, from, model, SIZE);
        let steps = 20;
        for i in 1..=steps {
            let p = (from.0, from.1 + dy * f64::from(i) / f64::from(steps));
            input.motion(0, p, model, SIZE);
        }
        input.up(0, None, model, SIZE);
    }

    fn type_text(input: &mut Input, model: &mut Model, text: &str) {
        for ch in text.chars() {
            let key = if ch == ' ' { Key::Space } else { Key::Char(ch) };
            let (r, _) = osk::keys(SIZE, &model.osk)
                .into_iter()
                .find(|(_, k)| *k == key)
                .unwrap();
            tap(input, model, center(r));
        }
    }

    #[test]
    fn geometry_fits_the_panel_and_avoids_the_gesture_zone() {
        let l = Layout::new(SIZE);
        assert_eq!(l.capacity(), 9);
        let rows = l.row_rects(20);
        assert_eq!(rows.len(), 9);
        let last = rows.last().unwrap();
        assert!(last.y + last.h <= l.notice_rect().y);
        let home_zone = (f64::from(SIZE.1) * gesture::HOME_ZONE) as i32;
        for r in [l.new_folder_rect(), l.refresh_rect(), l.view_area()] {
            assert!(r.y + r.h <= home_zone, "{r:?} reaches the home zone");
        }
        assert!(l.rename_rect().y > l.open_rect().y + l.open_rect().h);
        assert!(l.save_rect().y + l.save_rect().h < osk::extent(SIZE).0);
        assert!(l.view_cols() >= 50 && l.view_rows() >= 40);
        let track = l.rail_track();
        assert!(track.x > rows[0].x + rows[0].w && track.x + track.w <= SIZE.0 as i32);
    }

    #[test]
    fn breadcrumbs_elide_leading_folders_but_keep_home_and_current() {
        let l = Layout::new(SIZE);
        let short = l.crumb_rects(&["~".to_owned(), "Documents".to_owned()]);
        assert_eq!(
            short.iter().map(|(_, c, _)| *c).collect::<Vec<_>>(),
            [Crumb::Depth(0), Crumb::Depth(1)]
        );
        let deep: Vec<String> = std::iter::once("~".to_owned())
            .chain((1..=8).map(|i| format!("folder-number-{i}")))
            .collect();
        let crumbs = l.crumb_rects(&deep);
        assert_eq!(crumbs[0].1, Crumb::Depth(0));
        assert_eq!(crumbs[1].1, Crumb::Elided);
        assert_eq!(crumbs.last().unwrap().1, Crumb::Depth(8));
        assert_eq!(crumbs.last().unwrap().2, "folder-number\u{2026}");
        let end = crumbs.last().unwrap().0;
        assert!(end.x + end.w <= SIZE.0 as i32 - l.s(48));
        assert_eq!(crumb_label("Documents"), "Documents");
    }

    #[test]
    fn wrapping_is_monospace_and_visible() {
        assert_eq!(wrap_lines("abcdef", 4), ["abcd", "ef"]);
        assert_eq!(wrap_lines("a\tb\r\nc\n", 80), ["a    b", "c"]);
        assert_eq!(wrap_lines("x\u{1b}y", 80), ["x?y"]);
        assert_eq!(wrap_lines("", 80), Vec::<String>::new());
        assert_eq!(wrap_lines("\n\n", 80), ["", ""]);
    }

    #[test]
    fn taps_open_folders_files_details_and_breadcrumbs() {
        let (_d, mut m) = model(3);
        let mut input = Input::default();
        let l = Layout::new(SIZE);
        let rows = l.row_rects(4);
        assert_eq!(hit_test(&m, center(rows[0]), SIZE), Some(Target::Row(0)));
        let more = center(l.more_rect(rows[1]));
        assert_eq!(hit_test(&m, more, SIZE), Some(Target::RowMore(1)));
        // A file opens in the viewer; Back returns to the list.
        tap(&mut input, &mut m, center(rows[1]));
        assert_eq!(m.page, Page::View);
        assert_eq!(view_lines(&m, &l), ["line 0"]);
        tap(&mut input, &mut m, (80.0, 160.0));
        assert_eq!(m.page, Page::Browse);
        // Enter Documents, then the home crumb returns.
        tap(&mut input, &mut m, center(rows[0]));
        assert_eq!(m.cwd, ["Documents"]);
        let crumbs = l.crumb_rects(&m.crumbs());
        assert_eq!(
            hit_test(&m, center(crumbs[1].0), SIZE),
            None,
            "the current folder's crumb is not a target"
        );
        tap(&mut input, &mut m, center(crumbs[0].0));
        assert!(m.cwd.is_empty());
        // Details page from the trailing area.
        tap(&mut input, &mut m, more);
        assert_eq!(
            (m.page, m.selected.as_deref()),
            (Page::Item, Some("file-00.txt"))
        );
        tap(&mut input, &mut m, center(l.open_rect()));
        assert_eq!(m.page, Page::View);
        // An edge swipe is Back too.
        input.down(0, (10.0, 1200.0), &m, SIZE);
        input.motion(0, (400.0, 1210.0), &mut m, SIZE);
        input.up(0, None, &mut m, SIZE);
        assert_eq!(m.page, Page::Browse);
        tap(&mut input, &mut m, (80.0, 160.0));
        assert_eq!(m.pending, vec![Action::Back]);
        assert_eq!(hit_test(&m, (540.0, 2290.0), SIZE), Some(Target::Home));
    }

    #[test]
    fn dragging_scrolls_the_list_and_the_viewer_without_tapping() {
        let (_d, mut m) = model(30);
        let mut input = Input::default();
        let l = Layout::new(SIZE);
        let row = center(l.row_rects(1)[0]);
        let step = f64::from(l.row_step());
        drag(
            &mut input,
            &mut m,
            (row.0, row.1 + 600.0),
            -3.0 * step - 10.0,
        );
        assert_eq!(m.page, Page::Browse, "a drag is not a tap");
        assert_eq!(m.scroll.first(m.entries().len(), l.capacity()), 3);
        // The hit test follows the scrolled window.
        assert_eq!(hit_test(&m, row, SIZE), Some(Target::Row(3)));
        drag(&mut input, &mut m, (row.0, row.1 + 300.0), 10.0 * step);
        assert_eq!(m.scroll.first(m.entries().len(), l.capacity()), 0);

        let root = m.home.as_ref().unwrap().root().to_path_buf();
        let long: String = (0..200).map(|i| format!("line {i}\n")).collect();
        fs::write(root.join("Documents/long.txt"), long).unwrap();
        m.open_index(0);
        m.open_index(0);
        assert_eq!(m.page, Page::View);
        let line = f64::from(l.view_line());
        drag(&mut input, &mut m, (540.0, 1600.0), -5.0 * line - 4.0);
        assert_eq!(m.view_scroll.first(200, l.view_rows()), 5);
        assert_eq!(m.page, Page::View);
    }

    #[test]
    fn new_folder_rename_and_delete_through_the_touch_keyboard() {
        let (_d, mut m) = model(1);
        let mut input = Input::default();
        let l = Layout::new(SIZE);
        tap(&mut input, &mut m, center(l.new_folder_rect()));
        assert_eq!(m.page, Page::NewFolder);
        assert!(keyboard_visible(&m));
        // Save with an empty name shows the reason instead of acting.
        tap(&mut input, &mut m, center(l.save_rect()));
        assert_eq!(m.page, Page::NewFolder);
        assert_eq!(m.form_hint().as_deref(), Some("name is empty"));
        type_text(&mut input, &mut m, "music");
        assert_eq!(m.form, "music");
        assert_eq!(m.form_hint(), None);
        tap(&mut input, &mut m, center(l.save_rect()));
        assert_eq!(m.page, Page::Browse);
        assert_eq!(
            m.notice,
            Some(Notice::Ok("music \u{b7} created".to_owned()))
        );
        assert!(m.entry("music").is_some_and(|e| e.is_folder()));

        let index = m.entries().iter().position(|e| e.name == "music").unwrap();
        let row = l.row_rects(index + 1)[index];
        tap(&mut input, &mut m, center(l.more_rect(row)));
        tap(&mut input, &mut m, center(l.rename_rect()));
        assert_eq!((m.page, m.form.as_str()), (Page::Rename, "music"));
        key_press(&mut m, Key::Backspace);
        type_text(&mut input, &mut m, "x");
        key_press(&mut m, Key::Enter);
        assert_eq!((m.page, m.selected.as_deref()), (Page::Item, Some("musix")));

        tap(&mut input, &mut m, center(l.delete_rect()));
        assert_eq!(m.page, Page::ConfirmDelete);
        tap(&mut input, &mut m, center(l.cancel_rect()));
        assert_eq!(m.page, Page::Item);
        tap(&mut input, &mut m, center(l.delete_rect()));
        tap(&mut input, &mut m, center(l.confirm_rect()));
        assert_eq!(m.page, Page::Browse);
        assert!(m.entry("musix").is_none());
        // Escape dismisses a form.
        m.new_folder();
        assert!(key_press(&mut m, Key::Esc));
        assert_eq!(m.page, Page::Browse);
    }
}
