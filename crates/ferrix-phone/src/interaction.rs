//! Geometry and input for the Phone: every rectangle here is shared by the
//! renderer and the hit test, so interaction tests exercise what the
//! framebuffer shows. Pure: no Wayland, no sockets.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use ferrix_telephony_api::{AudioRoute, CallState};
use ferrix_ui::draw::Rect;
use ferrix_ui::gesture::{self, Gesture, Tracker};
use ferrix_ui::widgets::{back_rect, basis, contains, keypad_rects, list_rects, scale};

use crate::model::{Model, Page};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Back,
    /// The compositor's zone; drawn as the pill, never acted on.
    Home,
    Tab(Page),
    Key(char),
    Delete,
    Call,
    Answer,
    Reject,
    Hangup,
    Mute,
    Hold,
    DtmfToggle,
    Dtmf(char),
    Route(AudioRoute),
    /// A Recent row by index.
    Recent(usize),
    /// Ended page: dial the same party again.
    Again,
    Done,
}

pub const ROUTES: [AudioRoute; 4] = [
    AudioRoute::Earpiece,
    AudioRoute::Speaker,
    AudioRoute::Wired,
    AudioRoute::Bluetooth,
];

/// Page geometry at `size`.
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

    pub fn tab_rects(&self) -> [(Rect, Page); 2] {
        let s = |v| self.s(v);
        let w = (self.w() - 2 * s(96) - s(16)) / 2;
        [
            (
                Rect {
                    x: s(96),
                    y: s(280),
                    w,
                    h: s(72),
                },
                Page::Dialer,
            ),
            (
                Rect {
                    x: s(96) + w + s(16),
                    y: s(280),
                    w,
                    h: s(72),
                },
                Page::Recent,
            ),
        ]
    }

    pub fn number_rect(&self) -> Rect {
        let s = |v| self.s(v);
        Rect {
            x: s(96),
            y: s(400),
            w: self.w() - 2 * s(96),
            h: s(170),
        }
    }

    pub fn dial_keys(&self) -> Vec<(Rect, char)> {
        keypad_rects(self.size, self.s(720), 150)
    }

    pub fn call_rect(&self) -> Rect {
        let s = |v| self.s(v);
        Rect {
            x: s(96),
            y: s(1420),
            w: s(560),
            h: s(150),
        }
    }

    pub fn delete_rect(&self) -> Rect {
        let s = |v| self.s(v);
        Rect {
            x: s(672),
            y: s(1420),
            w: self.w() - s(672) - s(96),
            h: s(150),
        }
    }

    pub fn error_rect(&self) -> Rect {
        let s = |v| self.s(v);
        Rect {
            x: s(48),
            y: s(1640),
            w: self.w() - s(96),
            h: s(100),
        }
    }

    pub fn recent_rects(&self, count: usize) -> Vec<Rect> {
        let limit = ((self.size.1 as i32 * 92 / 100 - self.s(300)) / self.s(152)).max(1) as usize;
        list_rects(self.size, self.s(300), count.min(limit), 140)
    }

    pub fn answer_rect(&self) -> Rect {
        let s = |v| self.s(v);
        Rect {
            x: s(96),
            y: s(1700),
            w: s(420),
            h: s(170),
        }
    }

    pub fn reject_rect(&self) -> Rect {
        let s = |v| self.s(v);
        Rect {
            x: s(564),
            y: s(1700),
            w: s(420),
            h: s(170),
        }
    }

    /// Route chips in a 2x2 grid, wide enough for full labels.
    pub fn route_rects(&self) -> Vec<(Rect, AudioRoute)> {
        let s = |v| self.s(v);
        let gap = s(16);
        let w = (self.w() - 2 * s(96) - gap) / 2;
        let h = s(72);
        ROUTES
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let (col, row) = ((i % 2) as i32, (i / 2) as i32);
                (
                    Rect {
                        x: s(96) + col * (w + gap),
                        y: s(900) + row * (h + s(12)),
                        w,
                        h,
                    },
                    *r,
                )
            })
            .collect()
    }

    /// The in-call control row: Mute | Hold | Keypad, equal widths.
    fn control_rect(&self, column: i32) -> Rect {
        let s = |v| self.s(v);
        let gap = s(24);
        let w = (self.w() - 2 * s(96) - 2 * gap) / 3;
        Rect {
            x: s(96) + column * (w + gap),
            y: s(1084),
            w,
            h: s(120),
        }
    }

    pub fn mute_rect(&self) -> Rect {
        self.control_rect(0)
    }

    pub fn hold_rect(&self) -> Rect {
        self.control_rect(1)
    }

    pub fn dtmf_toggle_rect(&self) -> Rect {
        self.control_rect(2)
    }

    pub fn dtmf_keys(&self) -> Vec<(Rect, char)> {
        keypad_rects(self.size, self.s(1236), 110)
    }

    pub fn hangup_rect(&self) -> Rect {
        let s = |v| self.s(v);
        Rect {
            x: s(240),
            y: s(1860),
            w: s(600),
            h: s(170),
        }
    }

    pub fn again_rect(&self) -> Rect {
        self.answer_rect()
    }

    pub fn done_rect(&self) -> Rect {
        self.reject_rect()
    }
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
        Page::Dialer => {
            for (r, page) in l.tab_rects() {
                if contains(r, position) {
                    return Some(Target::Tab(page));
                }
            }
            for (r, ch) in l.dial_keys() {
                if contains(r, position) {
                    return Some(Target::Key(ch));
                }
            }
            if contains(l.call_rect(), position) {
                return Some(Target::Call);
            }
            contains(l.delete_rect(), position).then_some(Target::Delete)
        }
        Page::Recent => {
            for (r, page) in l.tab_rects() {
                if contains(r, position) {
                    return Some(Target::Tab(page));
                }
            }
            l.recent_rects(model.recent().len())
                .into_iter()
                .position(|r| contains(r, position))
                .map(Target::Recent)
        }
        Page::Incoming => {
            if contains(l.answer_rect(), position) {
                return Some(Target::Answer);
            }
            contains(l.reject_rect(), position).then_some(Target::Reject)
        }
        Page::InCall => {
            for (r, route) in l.route_rects() {
                if contains(r, position) {
                    return Some(Target::Route(route));
                }
            }
            if contains(l.mute_rect(), position) {
                return Some(Target::Mute);
            }
            if contains(l.hold_rect(), position) {
                return Some(Target::Hold);
            }
            if contains(l.dtmf_toggle_rect(), position) {
                return Some(Target::DtmfToggle);
            }
            if model.dtmf_open {
                for (r, ch) in l.dtmf_keys() {
                    if contains(r, position) {
                        return Some(Target::Dtmf(ch));
                    }
                }
            }
            contains(l.hangup_rect(), position).then_some(Target::Hangup)
        }
        Page::Ended => {
            if contains(l.again_rect(), position) {
                return Some(Target::Again);
            }
            contains(l.done_rect(), position).then_some(Target::Done)
        }
    }
}

/// Applies a tap. Returns whether the model changed.
pub fn activate(model: &mut Model, target: Target) -> bool {
    match target {
        Target::Back => model.back(),
        Target::Home => return false,
        Target::Tab(Page::Recent) => model.show_recent(),
        Target::Tab(_) => model.show_dialer(),
        Target::Key(ch) => model.key(ch),
        Target::Delete => model.delete(),
        Target::Call => {
            if let Err(block) = model.call() {
                model.last_error = Some(block.reason().to_owned());
            }
        }
        Target::Answer => model.answer(),
        Target::Reject => model.reject(),
        Target::Hangup => model.hangup(),
        Target::Mute => model.toggle_mute(),
        Target::Hold => model.toggle_hold(),
        Target::DtmfToggle => model.dtmf_open = !model.dtmf_open,
        Target::Dtmf(ch) => model.dtmf(ch),
        Target::Route(route) => model.set_route(route),
        Target::Recent(index) => {
            if let Some(view) = model.recent().get(index) {
                let id = view.call.id.clone();
                model.redial(&id);
            }
        }
        Target::Again => {
            if let Some(view) = model.focused() {
                let id = view.call.id.clone();
                model.dismiss_ended();
                model.redial(&id);
            }
        }
        Target::Done => model.dismiss_ended(),
    }
    true
}

/// Pressed-state feedback plus gesture classification over [`Tracker`].
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

    /// `position` is `None` for touch, which reports no release position;
    /// the tracker's last motion is used instead.
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
            Gesture::Tap(target) => {
                activate(model, target);
            }
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

/// Mute button copy and whether it is usable: the backend must advertise
/// `mic-mute` and the focused call must be connected (active or held).
pub fn mute_label(model: &Model) -> (&'static str, bool) {
    let label = if model.muted() { "Unmute" } else { "Mute" };
    (label, model.can_mute())
}

/// Whether the focused call can be put on hold or resumed.
pub fn hold_label(model: &Model) -> (&'static str, bool) {
    match model.focused().map(|c| c.call.state) {
        Some(CallState::Held) => ("Resume", true),
        Some(CallState::Active) => (
            "Hold",
            model.supports(ferrix_telephony_api::TelephonyFeature::Hold),
        ),
        _ => ("Hold", false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Action, preview_state};
    use ferrix_telephony_api::{CallEndReason, Direction};
    use ferrix_telephony_protocol::{StateView, Update};
    use ferrix_ui::widgets::center;

    const SIZE: (u32, u32) = (1080, 2340);

    fn ready(call: Option<(Direction, CallState, Option<CallEndReason>)>) -> Model {
        let mut m = Model::new();
        m.apply(Update::Snapshot(StateView::Full(preview_state(call))));
        m
    }

    fn tap(input: &mut Input, model: &mut Model, at: (f64, f64)) {
        input.down(0, at, model, SIZE);
        input.up(0, None, model, SIZE);
    }

    #[test]
    fn keypad_types_and_call_dials() {
        let mut m = ready(None);
        let mut input = Input::default();
        let l = Layout::new(SIZE);
        let keys = l.dial_keys();
        for wanted in "5550100042".chars() {
            let (r, _) = keys.iter().find(|(_, k)| *k == wanted).unwrap();
            input.down(0, center(*r), &m, SIZE);
            assert_eq!(input.pressed, Some(Target::Key(wanted)));
            input.up(0, Some(center(*r)), &mut m, SIZE);
        }
        assert_eq!(m.typed, "5550100042");
        tap(&mut input, &mut m, center(l.delete_rect()));
        assert_eq!(m.typed, "555010004");
        let (r, _) = keys.iter().find(|(_, k)| *k == '2').unwrap();
        tap(&mut input, &mut m, center(*r));
        tap(&mut input, &mut m, center(l.call_rect()));
        assert_eq!(m.pending, vec![Action::Dial("+15550100042".to_owned())]);
        assert_eq!(m.last_error, None);
    }

    #[test]
    fn emergency_numbers_show_the_reason_and_never_dial() {
        let mut m = ready(None);
        let mut input = Input::default();
        let l = Layout::new(SIZE);
        for wanted in "112".chars() {
            let (r, _) = l
                .dial_keys()
                .into_iter()
                .find(|(_, k)| *k == wanted)
                .unwrap();
            tap(&mut input, &mut m, center(r));
        }
        tap(&mut input, &mut m, center(l.call_rect()));
        assert!(m.pending.is_empty());
        assert_eq!(
            m.last_error.as_deref(),
            Some("Emergency calling not supported")
        );
    }

    #[test]
    fn tabs_recent_rows_and_back_navigation() {
        let mut m = ready(None);
        let mut input = Input::default();
        let l = Layout::new(SIZE);
        tap(&mut input, &mut m, center(l.tab_rects()[1].0));
        assert_eq!(m.page, Page::Recent);
        let rows = l.recent_rects(m.recent().len());
        assert_eq!(rows.len(), 3);
        tap(&mut input, &mut m, center(rows[0]));
        assert_eq!(m.page, Page::Dialer);
        assert_eq!(m.typed, "+15550100003", "newest call first");
        tap(&mut input, &mut m, center(l.tab_rects()[1].0));
        // Left-edge swipe returns to the dialer; from the dialer it yields.
        input.down(0, (20.0, 900.0), &m, SIZE);
        input.motion(0, (400.0, 900.0), &m, SIZE);
        input.up(0, Some((400.0, 900.0)), &mut m, SIZE);
        assert_eq!(m.page, Page::Dialer);
        tap(&mut input, &mut m, (80.0, 160.0));
        assert_eq!(m.pending, vec![Action::Back]);
        // The bottom zone is the compositor's.
        assert_eq!(hit_test(&m, (540.0, 2290.0), SIZE), Some(Target::Home));
        m.pending.clear();
        tap(&mut input, &mut m, (540.0, 2290.0));
        assert!(m.pending.is_empty());
    }

    #[test]
    fn in_call_controls_map_to_actions() {
        let mut m = ready(Some((Direction::Outgoing, CallState::Active, None)));
        let mut input = Input::default();
        let l = Layout::new(SIZE);
        assert_eq!(m.page, Page::InCall);
        tap(&mut input, &mut m, center(l.route_rects()[1].0));
        assert_eq!(m.pending.pop(), Some(Action::SetRoute(AudioRoute::Speaker)));
        assert_eq!(hold_label(&m), ("Hold", true));
        tap(&mut input, &mut m, center(l.hold_rect()));
        assert!(matches!(m.pending.pop(), Some(Action::Hold(_))));
        assert_eq!(mute_label(&m), ("Mute", true));
        tap(&mut input, &mut m, center(l.mute_rect()));
        assert_eq!(m.pending.pop(), Some(Action::SetMuted(true)));
        let mut muted = m.state.clone().unwrap();
        muted.audio.muted = true;
        m.apply(Update::Event(ferrix_telephony_protocol::Event::Redacted(
            ferrix_telephony_api::RedactedTelephonyEvent::AudioRouteChanged {
                audio: muted.audio.clone(),
            },
        )));
        assert_eq!(mute_label(&m), ("Unmute", true));
        assert_eq!(m.state_name(), "active-muted");
        tap(&mut input, &mut m, center(l.mute_rect()));
        assert_eq!(m.pending.pop(), Some(Action::SetMuted(false)));
        // Without the backend feature the button is inert.
        m.state
            .as_mut()
            .unwrap()
            .capabilities
            .remove(&ferrix_telephony_api::TelephonyFeature::MicMute);
        assert!(!mute_label(&m).1);
        tap(&mut input, &mut m, center(l.mute_rect()));
        assert!(m.pending.is_empty());
        let row = [l.mute_rect(), l.hold_rect(), l.dtmf_toggle_rect()];
        assert!(row[0].x + row[0].w < row[1].x && row[1].x + row[1].w < row[2].x);
        assert_eq!(row[2].x + row[2].w, 1080 - l.s(96), "row spans the margins");
        assert_eq!(
            hit_test(&m, center(l.dtmf_keys()[0].0), SIZE),
            None,
            "keypad closed"
        );
        tap(&mut input, &mut m, center(l.dtmf_toggle_rect()));
        assert!(m.dtmf_open);
        tap(&mut input, &mut m, center(l.dtmf_keys()[0].0));
        assert!(matches!(m.pending.pop(), Some(Action::Dtmf(_, '1'))));
        tap(&mut input, &mut m, center(l.hangup_rect()));
        assert!(matches!(m.pending.pop(), Some(Action::Hangup(_))));

        let mut m = ready(Some((Direction::Incoming, CallState::Ringing, None)));
        tap(&mut input, &mut m, center(l.answer_rect()));
        assert!(matches!(m.pending.pop(), Some(Action::Answer(_))));
        tap(&mut input, &mut m, center(l.reject_rect()));
        assert!(matches!(m.pending.pop(), Some(Action::Reject(_))));

        let mut m = ready(Some((
            Direction::Outgoing,
            CallState::Ended,
            Some(CallEndReason::Busy),
        )));
        m.focused_call = Some(ferrix_telephony_api::CallId("call-live".into()));
        m.page = Page::Ended;
        tap(&mut input, &mut m, center(l.again_rect()));
        assert_eq!((m.page, m.typed.as_str()), (Page::Dialer, "+15550100042"));
        m.page = Page::Ended;
        m.focused_call = Some(ferrix_telephony_api::CallId("call-live".into()));
        tap(&mut input, &mut m, center(l.done_rect()));
        assert_eq!(m.page, Page::Dialer);
    }
}
