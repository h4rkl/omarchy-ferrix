//! Geometry and input for Contacts; rectangles are shared by the renderer
//! and the hit test.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use ferrix_ui::draw::Rect;
use ferrix_ui::gesture::{self, Gesture, Tracker};
use ferrix_ui::osk::{self, Key};
use ferrix_ui::widgets::{Edit, back_rect, basis, contains, edit_text, list_rects, scale};

use crate::model::{FORM_NUMBERS, Focus, Model, Page};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Back,
    Home,
    Search,
    Row(usize),
    New,
    Export,
    Edit,
    Delete,
    FormName,
    FormNumber(usize),
    FormLabel(usize),
    AddNumber,
    Save,
    ConfirmDelete,
    Cancel,
    Key(Key),
}

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

    pub fn search_rect(&self) -> Rect {
        let s = |v| self.s(v);
        Rect {
            x: s(48),
            y: s(290),
            w: self.w() - s(96),
            h: s(100),
        }
    }

    /// Rows below the search field; fewer when the keyboard is up.
    pub fn row_rects(&self, count: usize, keyboard: bool) -> Vec<Rect> {
        let bottom = if keyboard {
            self.osk_top() - self.s(24)
        } else {
            self.new_rect().y - self.s(24)
        };
        let limit = ((bottom - self.s(420)) / self.s(152)).max(1) as usize;
        list_rects(self.size, self.s(420), count.min(limit), 140)
    }

    pub fn new_rect(&self) -> Rect {
        let s = |v| self.s(v);
        Rect {
            x: s(96),
            y: self.h() * 92 / 100 - s(180),
            w: s(420),
            h: s(140),
        }
    }

    pub fn export_rect(&self) -> Rect {
        let s = |v| self.s(v);
        Rect {
            x: s(564),
            y: self.h() * 92 / 100 - s(180),
            w: s(420),
            h: s(140),
        }
    }

    pub fn detail_number_rects(&self, count: usize) -> Vec<Rect> {
        list_rects(self.size, self.s(720), count.min(6), 120)
    }

    pub fn edit_rect(&self) -> Rect {
        let s = |v| self.s(v);
        Rect {
            x: s(96),
            y: s(1700),
            w: s(420),
            h: s(150),
        }
    }

    pub fn delete_rect(&self) -> Rect {
        let s = |v| self.s(v);
        Rect {
            x: s(564),
            y: s(1700),
            w: s(420),
            h: s(150),
        }
    }

    pub fn name_rect(&self) -> Rect {
        let s = |v| self.s(v);
        Rect {
            x: s(48),
            y: s(300),
            w: self.w() - s(96),
            h: s(100),
        }
    }

    /// `(label chip, number field)` per form row.
    pub fn number_rows(&self, count: usize) -> Vec<(Rect, Rect)> {
        let s = |v| self.s(v);
        (0..count.min(FORM_NUMBERS))
            .map(|i| {
                let y = s(430) + i as i32 * s(120);
                (
                    Rect {
                        x: s(48),
                        y,
                        w: s(220),
                        h: s(100),
                    },
                    Rect {
                        x: s(284),
                        y,
                        w: self.w() - s(284) - s(48),
                        h: s(100),
                    },
                )
            })
            .collect()
    }

    pub fn add_number_rect(&self, count: usize) -> Rect {
        let s = |v| self.s(v);
        Rect {
            x: s(48),
            y: s(430) + count.min(FORM_NUMBERS) as i32 * s(120),
            w: s(360),
            h: s(72),
        }
    }

    pub fn save_rect(&self) -> Rect {
        let s = |v| self.s(v);
        Rect {
            x: s(96),
            y: self.osk_top() - s(20) - s(130),
            w: self.w() - 2 * s(96),
            h: s(130),
        }
    }

    pub fn error_rect(&self) -> Rect {
        let s = |v| self.s(v);
        let save = self.save_rect();
        Rect {
            x: s(48),
            y: save.y - s(120),
            w: self.w() - s(96),
            h: s(96),
        }
    }

    pub fn confirm_rect(&self) -> Rect {
        self.edit_rect()
    }

    pub fn cancel_rect(&self) -> Rect {
        self.delete_rect()
    }
}

fn osk_hit(model: &Model, position: (f64, f64), size: (u32, u32)) -> Option<Target> {
    osk::keys(size, &model.osk)
        .into_iter()
        .find(|(r, _)| osk::contains(*r, position))
        .map(|(_, k)| Target::Key(k))
}

pub fn keyboard_visible(model: &Model) -> bool {
    matches!(model.page, Page::Edit)
        || (model.page == Page::List && model.focus == Some(Focus::Search))
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
        Page::List => {
            if contains(l.search_rect(), position) {
                return Some(Target::Search);
            }
            let keyboard = keyboard_visible(model);
            if !keyboard {
                if contains(l.new_rect(), position) {
                    return Some(Target::New);
                }
                if contains(l.export_rect(), position) {
                    return Some(Target::Export);
                }
            }
            l.row_rects(model.filtered().len(), keyboard)
                .into_iter()
                .position(|r| contains(r, position))
                .map(Target::Row)
        }
        Page::Detail => {
            if contains(l.edit_rect(), position) {
                return Some(Target::Edit);
            }
            contains(l.delete_rect(), position).then_some(Target::Delete)
        }
        Page::Edit => {
            if contains(l.name_rect(), position) {
                return Some(Target::FormName);
            }
            for (i, (label, field)) in l
                .number_rows(model.form.numbers.len())
                .into_iter()
                .enumerate()
            {
                if contains(label, position) {
                    return Some(Target::FormLabel(i));
                }
                if contains(field, position) {
                    return Some(Target::FormNumber(i));
                }
            }
            if model.form.numbers.len() < FORM_NUMBERS
                && contains(l.add_number_rect(model.form.numbers.len()), position)
            {
                return Some(Target::AddNumber);
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
        Target::Back => model.back(),
        Target::Home => {}
        Target::Search => model.focus = Some(Focus::Search),
        Target::Row(index) => {
            if let Some(contact) = model.filtered().get(index) {
                let id = contact.id.clone();
                model.open(&id);
            }
        }
        Target::New => model.new_contact(),
        Target::Export => model.export(),
        Target::Edit => model.edit_selected(),
        Target::Delete => model.ask_delete(),
        Target::FormName => model.focus = Some(Focus::Name),
        Target::FormNumber(i) => model.focus = Some(Focus::Number(i)),
        Target::FormLabel(i) => model.cycle_label(i),
        Target::AddNumber => model.add_number(),
        Target::Save => {
            if let Err(error) = model.save() {
                model.last_error = Some(error);
            }
        }
        Target::ConfirmDelete => model.confirm_delete(),
        Target::Cancel => model.back(),
        Target::Key(key) => key_press(model, key),
    }
}

pub fn key_press(model: &mut Model, key: Key) {
    let mut osk = model.osk;
    let Some(text) = model.focused_text() else {
        return;
    };
    let outcome = edit_text(text, key, &mut osk);
    model.osk = osk;
    match outcome {
        Edit::Submit => match model.focus {
            Some(Focus::Search) => model.focus = None,
            Some(Focus::Name) => model.focus = Some(Focus::Number(0)),
            Some(Focus::Number(i)) if i + 1 < model.form.numbers.len() => {
                model.focus = Some(Focus::Number(i + 1))
            }
            Some(Focus::Number(_)) => {
                if let Err(error) = model.save() {
                    model.last_error = Some(error);
                }
            }
            None => {}
        },
        Edit::Dismiss => model.back(),
        Edit::Changed => model.last_error = None,
        Edit::Unchanged => {}
    }
}

#[derive(Default)]
pub struct Input {
    tracker: Tracker<Target>,
    pub pressed: Option<Target>,
}

impl Input {
    pub fn down(&mut self, id: i32, position: (f64, f64), model: &Model, size: (u32, u32)) -> bool {
        match self
            .tracker
            .down(id, position, hit_test(model, position, size))
        {
            Some(pressed) => {
                self.pressed = pressed;
                true
            }
            None => false,
        }
    }

    pub fn motion(
        &mut self,
        id: i32,
        position: (f64, f64),
        model: &Model,
        size: (u32, u32),
    ) -> bool {
        let at = hit_test(model, position, size);
        match self.tracker.motion(id, position, size, at, self.pressed) {
            Some(pressed) => {
                self.pressed = pressed;
                true
            }
            None => false,
        }
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
        match gesture {
            Gesture::Tap(target) => activate(model, target),
            Gesture::BackSwipe => model.back(),
            Gesture::PullDown | Gesture::Release => {}
        }
        true
    }

    pub fn cancel(&mut self) -> bool {
        let had = self.tracker.cancel();
        had || self.pressed.take().is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Action, preview_contacts};
    use ferrix_contacts_protocol::Link as ProtoLink;
    use ferrix_ui::widgets::center;

    const SIZE: (u32, u32) = (1080, 2340);

    fn ready() -> Model {
        let mut m = Model::new();
        m.apply(ProtoLink::Connected {
            contacts: preview_contacts(),
        });
        m
    }

    fn tap(input: &mut Input, model: &mut Model, at: (f64, f64)) {
        input.down(0, at, model, SIZE);
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
    fn search_with_keyboard_then_open_detail() {
        let mut m = ready();
        let mut input = Input::default();
        let l = Layout::new(SIZE);
        assert_eq!(l.row_rects(8, false).len(), 8);
        tap(&mut input, &mut m, center(l.search_rect()));
        assert_eq!(m.focus, Some(Focus::Search));
        assert!(keyboard_visible(&m));
        assert!(l.row_rects(8, true).len() < 8, "keyboard shortens the list");
        let (layer, _) = osk::keys(SIZE, &m.osk)
            .into_iter()
            .find(|(_, k)| *k == Key::Layer)
            .unwrap();
        tap(&mut input, &mut m, center(layer));
        type_text(&mut input, &mut m, "05");
        assert_eq!(m.query, "05");
        assert_eq!(m.filtered().len(), 1);
        key_press(&mut m, Key::Enter);
        assert_eq!(m.focus, None);
        let rows = l.row_rects(1, false);
        tap(&mut input, &mut m, center(rows[0]));
        assert_eq!(m.page, Page::Detail);
        assert_eq!(m.selected().unwrap().display_name, "Test Contact 05");
        assert_eq!(
            hit_test(&m, center(l.edit_rect()), SIZE),
            Some(Target::Edit)
        );
        tap(&mut input, &mut m, center(l.delete_rect()));
        assert_eq!(m.page, Page::ConfirmDelete);
        tap(&mut input, &mut m, center(l.cancel_rect()));
        assert_eq!(m.page, Page::Detail);
        tap(&mut input, &mut m, center(l.delete_rect()));
        tap(&mut input, &mut m, center(l.confirm_rect()));
        assert!(matches!(m.pending.as_slice(), [Action::Delete(_)]));
        assert_eq!(m.page, Page::List);
    }

    #[test]
    fn edit_form_types_labels_and_saves() {
        let mut m = ready();
        let mut input = Input::default();
        let l = Layout::new(SIZE);
        tap(&mut input, &mut m, center(l.new_rect()));
        assert_eq!(m.page, Page::Edit);
        type_text(&mut input, &mut m, "new one");
        assert_eq!(m.form.name, "new one");
        let (label, field) = l.number_rows(1)[0];
        tap(&mut input, &mut m, center(label));
        assert_eq!(m.form.numbers[0].0, ferrix_contacts_api::NumberLabel::Home);
        tap(&mut input, &mut m, center(field));
        assert_eq!(m.focus, Some(Focus::Number(0)));
        let (layer, _) = osk::keys(SIZE, &m.osk)
            .into_iter()
            .find(|(_, k)| *k == Key::Layer)
            .unwrap();
        tap(&mut input, &mut m, center(layer));
        type_text(&mut input, &mut m, "5550100123");
        tap(&mut input, &mut m, center(l.add_number_rect(1)));
        assert_eq!(m.form.numbers.len(), 2);
        assert_eq!(m.focus, Some(Focus::Number(1)));
        tap(&mut input, &mut m, center(l.save_rect()));
        assert!(
            matches!(&m.pending[0], Action::Create(input) if input.numbers.len() == 1 && input.numbers[0].raw == "5550100123")
        );
        assert_eq!(m.page, Page::List);
        // Export from the list; Back from the list yields.
        m.pending.clear();
        tap(&mut input, &mut m, center(l.export_rect()));
        assert_eq!(m.pending, vec![Action::Export]);
        m.pending.clear();
        tap(&mut input, &mut m, (80.0, 160.0));
        assert_eq!(m.pending, vec![Action::Back]);
        assert_eq!(hit_test(&m, (540.0, 2290.0), SIZE), Some(Target::Home));
    }
}
