//! Geometry and input for Messages; rectangles are shared by the renderer
//! and the hit test. Bubbles use fixed row heights so the geometry needs no
//! font metrics and stays pure.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use ferrix_ui::draw::Rect;
use ferrix_ui::gesture::{self, Gesture, Tracker};
use ferrix_ui::osk::{self, Key, OskState};
use ferrix_ui::widgets::{Edit, back_rect, basis, contains, edit_text, list_rects, scale};

use crate::model::{Field, Model, Page};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Back,
    Home,
    Conversation(usize),
    New,
    /// A message bubble by index into the thread (oldest first).
    Bubble(usize),
    Field(Field),
    Send,
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

    pub fn conversation_rects(&self, count: usize) -> Vec<Rect> {
        let bottom = self.new_rect().y - self.s(24);
        let limit = ((bottom - self.s(300)) / self.s(162)).max(1) as usize;
        list_rects(self.size, self.s(300), count.min(limit), 150)
    }

    pub fn new_rect(&self) -> Rect {
        let s = |v| self.s(v);
        Rect {
            x: s(96),
            y: self.h() * 92 / 100 - s(180),
            w: self.w() - 2 * s(96),
            h: s(140),
        }
    }

    /// Top of the keyboard tray.
    pub fn osk_top(&self) -> i32 {
        osk::extent(self.size).0 - self.s(12)
    }

    pub fn draft_rect(&self) -> Rect {
        let s = |v| self.s(v);
        Rect {
            x: s(48),
            y: self.osk_top() - s(20) - s(96),
            w: self.w() - s(48) - s(48) - s(220),
            h: s(96),
        }
    }

    pub fn send_rect(&self) -> Rect {
        let s = |v| self.s(v);
        let d = self.draft_rect();
        Rect {
            x: self.w() - s(48) - s(200),
            y: d.y,
            w: s(200),
            h: d.h,
        }
    }

    /// Height of one message row in the thread (bubble plus chip).
    pub fn bubble_row(&self) -> i32 {
        self.s(190)
    }

    /// Rectangles of the visible thread messages, oldest first; only the
    /// newest that fit above the draft field are returned, so the first
    /// element maps to index `count - returned.len()`.
    pub fn bubble_rects(&self, count: usize, outgoing: &[bool]) -> (usize, Vec<Rect>) {
        let s = |v| self.s(v);
        let top = s(300);
        let bottom = self.draft_rect().y - s(24);
        let row = self.bubble_row();
        let fit = ((bottom - top) / row).max(1) as usize;
        let shown = count.min(fit);
        let first = count - shown;
        let width = self.w() * 72 / 100;
        let rects = (0..shown)
            .map(|i| {
                let y = bottom - (shown - i) as i32 * row;
                let out = outgoing.get(first + i).copied().unwrap_or(false);
                Rect {
                    x: if out { self.w() - s(48) - width } else { s(48) },
                    y,
                    w: width,
                    h: s(120),
                }
            })
            .collect();
        (first, rects)
    }

    pub fn to_rect(&self) -> Rect {
        let s = |v| self.s(v);
        Rect {
            x: s(48),
            y: s(300),
            w: self.w() - s(96),
            h: s(100),
        }
    }

    pub fn body_rect(&self) -> Rect {
        let s = |v| self.s(v);
        Rect {
            x: s(48),
            y: s(420),
            w: self.w() - s(96),
            h: s(100),
        }
    }

    pub fn compose_send_rect(&self) -> Rect {
        let s = |v| self.s(v);
        Rect {
            x: s(96),
            y: s(560),
            w: self.w() - 2 * s(96),
            h: s(120),
        }
    }
}

/// The keyboard layout the focused field declares: the recipient takes the
/// dial pad, message bodies take the text keyboard.
pub fn field_layout(model: &Model) -> osk::Layout {
    match (model.page, model.focus) {
        (Page::Compose, Field::To) => osk::Layout::Phone,
        _ => osk::Layout::Text,
    }
}

/// Keyboard state shown, hit-tested, and edited for the focused field.
pub fn keyboard_state(model: &Model) -> OskState {
    model.osk.with_layout(field_layout(model))
}

fn osk_hit(model: &Model, position: (f64, f64), size: (u32, u32)) -> Option<Target> {
    osk::keys(size, &keyboard_state(model))
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
    match model.page {
        Page::Conversations => {
            if contains(l.new_rect(), position) {
                return Some(Target::New);
            }
            l.conversation_rects(model.conversations().len())
                .into_iter()
                .position(|r| contains(r, position))
                .map(Target::Conversation)
        }
        Page::Thread => {
            if let Some(t) = osk_hit(model, position, size) {
                return Some(t);
            }
            if contains(l.send_rect(), position) {
                return Some(Target::Send);
            }
            if contains(l.draft_rect(), position) {
                return Some(Target::Field(Field::Body));
            }
            let msgs = model.thread_messages();
            let outgoing: Vec<bool> = msgs
                .iter()
                .map(|m| m.direction == ferrix_telephony_api::Direction::Outgoing)
                .collect();
            let (first, rects) = l.bubble_rects(msgs.len(), &outgoing);
            rects
                .into_iter()
                .position(|r| contains(r, position))
                .map(|i| Target::Bubble(first + i))
        }
        Page::Compose => {
            if let Some(t) = osk_hit(model, position, size) {
                return Some(t);
            }
            if contains(l.to_rect(), position) {
                return Some(Target::Field(Field::To));
            }
            if contains(l.body_rect(), position) {
                return Some(Target::Field(Field::Body));
            }
            contains(l.compose_send_rect(), position).then_some(Target::Send)
        }
    }
}

pub fn activate(model: &mut Model, target: Target) {
    match target {
        Target::Back => model.back(),
        Target::Home => {}
        Target::Conversation(index) => {
            if let Some(conv) = model.conversations().get(index) {
                let key = conv.key.clone();
                model.open_thread(&key);
            }
        }
        Target::New => model.open_compose(),
        Target::Bubble(index) => {
            if let Some(m) = model.thread_messages().get(index) {
                let id = m.id.clone();
                model.retry(&id);
            }
        }
        Target::Field(field) => model.focus = field,
        Target::Send => {
            if let Err(block) = model.send() {
                model.last_error = Some(block.reason().to_owned());
            }
        }
        Target::Key(key) => key_press(model, key),
    }
}

/// Feeds one keyboard key to the focused field.
pub fn key_press(model: &mut Model, key: Key) {
    let mut osk = keyboard_state(model);
    let outcome = edit_text(model.focused_text(), key, &mut osk);
    model.osk = osk;
    match outcome {
        Edit::Submit => match (model.page, model.focus) {
            (Page::Compose, Field::To) => model.focus = Field::Body,
            _ => {
                if let Err(block) = model.send() {
                    model.last_error = Some(block.reason().to_owned());
                }
            }
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
    use crate::model::{Action, preview_state};
    use ferrix_telephony_api::MessageId;
    use ferrix_telephony_protocol::{StateView, Update};
    use ferrix_ui::widgets::center;

    const SIZE: (u32, u32) = (1080, 2340);

    fn ready() -> Model {
        let mut m = Model::new();
        m.apply(Update::Snapshot(StateView::Full(preview_state())));
        m
    }

    fn tap(input: &mut Input, model: &mut Model, at: (f64, f64)) {
        input.down(0, at, model, SIZE);
        input.up(0, None, model, SIZE);
    }

    fn type_text(input: &mut Input, model: &mut Model, text: &str) {
        for ch in text.chars() {
            let key = if ch == ' ' { Key::Space } else { Key::Char(ch) };
            let (r, _) = osk::keys(SIZE, &keyboard_state(model))
                .into_iter()
                .find(|(_, k)| *k == key)
                .unwrap();
            tap(input, model, center(r));
        }
    }

    #[test]
    fn opening_a_thread_marks_read_and_bubbles_retry() {
        let mut m = ready();
        let mut input = Input::default();
        let l = Layout::new(SIZE);
        let rows = l.conversation_rects(2);
        tap(&mut input, &mut m, center(rows[1]));
        assert_eq!(m.page, Page::Thread);
        assert_eq!(m.pending, vec![Action::MarkRead(MessageId("m3".into()))]);
        m.pending.clear();
        let msgs = m.thread_messages();
        let outgoing: Vec<bool> = msgs
            .iter()
            .map(|x| x.direction == ferrix_telephony_api::Direction::Outgoing)
            .collect();
        let (first, rects) = l.bubble_rects(msgs.len(), &outgoing);
        assert_eq!(first, 0);
        assert_eq!(rects.len(), 4);
        assert!(rects[1].x > rects[0].x, "outgoing bubbles sit right");
        // The failed message is the last one; tapping it retries.
        tap(&mut input, &mut m, center(rects[3]));
        assert!(
            matches!(m.pending.as_slice(), [Action::SendSms { body, .. }] if body == "Did this go out?")
        );
        m.pending.clear();
        tap(&mut input, &mut m, center(rects[1]));
        assert!(m.pending.is_empty(), "delivered bubbles do nothing");
        // Draft through the keyboard, Send button.
        type_text(&mut input, &mut m, "ok");
        assert_eq!(m.draft, "ok");
        tap(&mut input, &mut m, center(l.send_rect()));
        assert_eq!(
            m.pending,
            vec![Action::SendSms {
                to: "+15550100042".into(),
                body: "ok".into()
            }]
        );
        assert!(m.draft.is_empty());
        // Back swipe returns to conversations; from there Back yields.
        input.down(0, (20.0, 1000.0), &m, SIZE);
        input.motion(0, (400.0, 1000.0), &m, SIZE);
        input.up(0, Some((400.0, 1000.0)), &mut m, SIZE);
        assert_eq!(m.page, Page::Conversations);
        m.pending.clear();
        tap(&mut input, &mut m, (80.0, 160.0));
        assert_eq!(m.pending, vec![Action::Back]);
    }

    #[test]
    fn compose_types_into_the_focused_field_and_sends() {
        let mut m = ready();
        let mut input = Input::default();
        let l = Layout::new(SIZE);
        tap(&mut input, &mut m, center(l.new_rect()));
        assert_eq!((m.page, m.focus), (Page::Compose, Field::To));
        // The recipient field declares the dial pad: digits without a layer.
        assert_eq!(keyboard_state(&m).layout, osk::Layout::Phone);
        type_text(&mut input, &mut m, "5550100077");
        assert_eq!(m.compose_to, "5550100077");
        tap(&mut input, &mut m, center(l.body_rect()));
        assert_eq!(m.focus, Field::Body);
        assert_eq!(keyboard_state(&m).layout, osk::Layout::Text);
        // Shift capitalises the next letter only.
        let (shift, _) = osk::keys(SIZE, &keyboard_state(&m))
            .into_iter()
            .find(|(_, k)| *k == Key::Shift)
            .unwrap();
        tap(&mut input, &mut m, center(shift));
        type_text(&mut input, &mut m, "hi there");
        assert_eq!(m.draft, "Hi there");
        assert_eq!(
            hit_test(&m, center(l.compose_send_rect()), SIZE),
            Some(Target::Send)
        );
        tap(&mut input, &mut m, center(l.compose_send_rect()));
        assert_eq!(
            m.pending,
            vec![Action::SendSms {
                to: "+15550100077".into(),
                body: "Hi there".into()
            }]
        );
        assert_eq!(m.page, Page::Thread);
        // Enter in the To field moves focus; a blocked send shows its reason.
        let mut m = ready();
        m.open_compose();
        key_press(&mut m, Key::Enter);
        assert_eq!(m.focus, Field::Body);
        key_press(&mut m, Key::Enter);
        assert_eq!(m.last_error.as_deref(), Some("recipient \u{b7} empty"));
        assert_eq!(hit_test(&m, (540.0, 2290.0), SIZE), Some(Target::Home));
    }
}
