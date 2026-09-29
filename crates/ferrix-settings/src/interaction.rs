//! Geometry and input for Settings; rectangles are shared by the renderer
//! and the hit test.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use ferrix_ui::draw::Rect;
use ferrix_ui::gesture::{self, Gesture, Tracker};
use ferrix_ui::osk::{self, Key};
use ferrix_ui::widgets::{Edit, back_rect, basis, contains, edit_text, list_rects, scale};

use crate::model::{Model, Page};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Back,
    Home,
    Tab(Page),
    Radio,
    Scan,
    Ap(usize),
    /// Forget the saved profile at index.
    Profile(usize),
    PassField,
    PassConnect,
    PassCancel,
    Key(Key),
    Airplane,
    Wifi,
    Cellular,
    CellularData,
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

    pub fn tab_rects(&self) -> Vec<(Rect, Page)> {
        let s = |v| self.s(v);
        let w = (self.w() - 2 * s(96) - 2 * s(16)) / 3;
        Page::ALL
            .into_iter()
            .enumerate()
            .map(|(i, p)| {
                (
                    Rect {
                        x: s(96) + i as i32 * (w + s(16)),
                        y: s(270),
                        w,
                        h: s(64),
                    },
                    p,
                )
            })
            .collect()
    }

    pub fn radio_rect(&self) -> Rect {
        let s = |v| self.s(v);
        Rect {
            x: s(96),
            y: s(380),
            w: s(440),
            h: s(80),
        }
    }

    pub fn scan_rect(&self) -> Rect {
        let s = |v| self.s(v);
        Rect {
            x: s(560),
            y: s(380),
            w: self.w() - s(560) - s(96),
            h: s(80),
        }
    }

    pub fn connection_rect(&self) -> Rect {
        let s = |v| self.s(v);
        Rect {
            x: s(96),
            y: s(540),
            w: s(440),
            h: s(72),
        }
    }

    pub fn connectivity_rect(&self) -> Rect {
        let s = |v| self.s(v);
        Rect {
            x: s(560),
            y: s(540),
            w: self.w() - s(560) - s(96),
            h: s(72),
        }
    }

    pub fn error_rect(&self) -> Rect {
        let s = |v| self.s(v);
        Rect {
            x: s(48),
            y: s(740),
            w: self.w() - s(96),
            h: s(90),
        }
    }

    pub fn ap_rects(&self, count: usize) -> Vec<Rect> {
        list_rects(self.size, self.s(900), count.min(5), 120)
    }

    pub fn profile_rects(&self, count: usize) -> Vec<Rect> {
        list_rects(self.size, self.s(1650), count.min(3), 100)
    }

    pub fn pass_field_rect(&self) -> Rect {
        let s = |v| self.s(v);
        Rect {
            x: s(48),
            y: s(900),
            w: self.w() - s(96),
            h: s(100),
        }
    }

    pub fn pass_connect_rect(&self) -> Rect {
        let s = |v| self.s(v);
        Rect {
            x: s(96),
            y: s(1030),
            w: s(420),
            h: s(120),
        }
    }

    pub fn pass_cancel_rect(&self) -> Rect {
        let s = |v| self.s(v);
        Rect {
            x: s(564),
            y: s(1030),
            w: s(420),
            h: s(120),
        }
    }

    pub fn airplane_rect(&self) -> Rect {
        let s = |v| self.s(v);
        Rect {
            x: s(48),
            y: s(400),
            w: self.w() - s(96),
            h: s(150),
        }
    }

    pub fn wifi_rect(&self) -> Rect {
        let s = |v| self.s(v);
        Rect {
            x: s(48),
            y: s(600),
            w: self.w() - s(96),
            h: s(150),
        }
    }

    pub fn cellular_rect(&self) -> Rect {
        let s = |v| self.s(v);
        Rect {
            x: s(48),
            y: s(800),
            w: self.w() - s(96),
            h: s(150),
        }
    }

    pub fn cellular_data_rect(&self) -> Rect {
        let s = |v| self.s(v);
        Rect {
            x: s(48),
            y: s(1000),
            w: self.w() - s(96),
            h: s(150),
        }
    }
}

pub fn keyboard_visible(model: &Model) -> bool {
    model.page == Page::Network && model.passphrase_for.is_some()
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
        && let Some((_, key)) = osk::keys(size, &model.osk)
            .into_iter()
            .find(|(r, _)| osk::contains(*r, position))
    {
        return Some(Target::Key(key));
    }
    for (r, page) in l.tab_rects() {
        if contains(r, position) {
            return Some(Target::Tab(page));
        }
    }
    match model.page {
        Page::Network => {
            if contains(l.radio_rect(), position) {
                return Some(Target::Radio);
            }
            if contains(l.scan_rect(), position) {
                return Some(Target::Scan);
            }
            if model.passphrase_for.is_some() {
                if contains(l.pass_field_rect(), position) {
                    return Some(Target::PassField);
                }
                if contains(l.pass_connect_rect(), position) {
                    return Some(Target::PassConnect);
                }
                return contains(l.pass_cancel_rect(), position).then_some(Target::PassCancel);
            }
            if let Some(i) = l
                .ap_rects(model.access_points().len())
                .into_iter()
                .position(|r| contains(r, position))
            {
                return Some(Target::Ap(i));
            }
            l.profile_rects(model.profiles().len())
                .into_iter()
                .position(|r| contains(r, position))
                .map(Target::Profile)
        }
        Page::Radios => {
            if contains(l.airplane_rect(), position) {
                return Some(Target::Airplane);
            }
            if contains(l.wifi_rect(), position) {
                return Some(Target::Wifi);
            }
            if contains(l.cellular_rect(), position) {
                return Some(Target::Cellular);
            }
            contains(l.cellular_data_rect(), position).then_some(Target::CellularData)
        }
        Page::About => None,
    }
}

pub fn activate(model: &mut Model, target: Target) {
    match target {
        Target::Back => model.back(),
        Target::Home => {}
        Target::Tab(page) => model.show(page),
        Target::Radio | Target::Wifi => model.toggle_radio(),
        Target::Scan => model.scan(),
        Target::Ap(i) => {
            if let Some(ap) = model.access_points().get(i) {
                let id = ap.id.clone();
                model.select_ap(&id);
            }
        }
        Target::Profile(i) => {
            if let Some(p) = model.profiles().get(i) {
                let id = p.id.clone();
                model.forget(&id);
            }
        }
        Target::PassField => {}
        Target::PassConnect => {
            if let Err(reason) = model.submit_passphrase() {
                model.last_error = Some(reason);
            }
        }
        Target::PassCancel => model.cancel_passphrase(),
        Target::Key(key) => key_press(model, key),
        Target::Airplane => model.toggle_airplane(),
        Target::Cellular => model.toggle_cellular(),
        Target::CellularData => model.toggle_cellular_data(),
    }
}

pub fn key_press(model: &mut Model, key: Key) {
    if model.passphrase_for.is_none() {
        return;
    }
    let mut osk = model.osk;
    let outcome = edit_text(&mut model.passphrase, key, &mut osk);
    model.osk = osk;
    match outcome {
        Edit::Submit => {
            if let Err(reason) = model.submit_passphrase() {
                model.last_error = Some(reason);
            }
        }
        Edit::Dismiss => model.cancel_passphrase(),
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
    use crate::model::{Action, preview_network};
    use ferrix_network_api::ApId;
    use ferrix_network_protocol::Feed;
    use ferrix_ui::widgets::center;

    const SIZE: (u32, u32) = (1080, 2340);

    fn ready() -> Model {
        let mut m = Model::new();
        m.apply_network(Feed::Snapshot {
            state: preview_network(false),
            evidence: "mock".into(),
        });
        m
    }

    fn tap(input: &mut Input, model: &mut Model, at: (f64, f64)) {
        input.down(0, at, model, SIZE);
        input.up(0, None, model, SIZE);
    }

    #[test]
    fn network_controls_and_passphrase_entry() {
        let mut m = ready();
        let mut input = Input::default();
        let l = Layout::new(SIZE);
        tap(&mut input, &mut m, center(l.scan_rect()));
        assert_eq!(m.pending.pop(), Some(Action::Scan));
        tap(&mut input, &mut m, center(l.radio_rect()));
        assert_eq!(m.pending.pop(), Some(Action::SetRadio(false)));
        let aps = l.ap_rects(3);
        // Strongest first: ferrix-lab (saved profile) connects directly.
        tap(&mut input, &mut m, center(aps[0]));
        assert_eq!(
            m.pending.pop(),
            Some(Action::Connect {
                ap: ApId::new("ap-0"),
                passphrase: None
            })
        );
        // Enterprise AP asks for a passphrase; keyboard types it.
        tap(&mut input, &mut m, center(aps[2]));
        assert!(keyboard_visible(&m));
        assert_eq!(
            hit_test(&m, center(aps[1]), SIZE),
            None,
            "list hidden behind the prompt"
        );
        for ch in "mockpass".chars() {
            let (r, _) = osk::keys(SIZE, &m.osk)
                .into_iter()
                .find(|(_, k)| *k == Key::Char(ch))
                .unwrap();
            tap(&mut input, &mut m, center(r));
        }
        assert_eq!(m.passphrase, "mockpass");
        tap(&mut input, &mut m, center(l.pass_connect_rect()));
        assert_eq!(
            m.pending.pop(),
            Some(Action::Connect {
                ap: ApId::new("ap-2"),
                passphrase: Some("mockpass".into())
            })
        );
        assert!(!keyboard_visible(&m));
        tap(&mut input, &mut m, center(aps[2]));
        tap(&mut input, &mut m, center(l.pass_cancel_rect()));
        assert_eq!(m.passphrase_for, None);
        tap(&mut input, &mut m, center(l.profile_rects(1)[0]));
        assert!(matches!(m.pending.pop(), Some(Action::Forget(_))));
        // Tabs switch pages; Back from Radios returns to Network, then yields.
        tap(&mut input, &mut m, center(l.tab_rects()[1].0));
        assert_eq!(m.page, Page::Radios);
        tap(&mut input, &mut m, center(l.airplane_rect()));
        assert_eq!(m.pending.len(), 2);
        m.pending.clear();
        tap(&mut input, &mut m, center(l.cellular_rect()));
        assert!(matches!(m.pending.pop(), Some(Action::SetRadioPower(_))));
        tap(&mut input, &mut m, center(l.cellular_data_rect()));
        assert!(matches!(
            m.pending.pop(),
            Some(Action::SetCellularData(true))
        ));
        tap(&mut input, &mut m, (80.0, 160.0));
        assert_eq!(m.page, Page::Network);
        tap(&mut input, &mut m, (80.0, 160.0));
        assert_eq!(m.pending, vec![Action::Back]);
        assert_eq!(hit_test(&m, (540.0, 2290.0), SIZE), Some(Target::Home));
    }
}
