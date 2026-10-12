//! Geometry and input for Settings; rectangles are shared by the renderer
//! and the hit test.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use ferrix_ui::draw::Rect;
use std::time::Instant;

use ferrix_ui::gesture::{self, Gesture, Tracker};
use ferrix_ui::keyboard::KeyRepeat;
use ferrix_ui::osk::{self, Key};
use ferrix_ui::widgets::{
    Edit, back_rect, basis, contains, edit_text, list_rects, scale, slider_value,
};

use crate::bluetooth::{BtView, Card};
use crate::model::{Model, Page};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Back,
    Home,
    /// A category row on the root page.
    Section(Page),
    Radio,
    Scan,
    Ap(usize),
    /// Forget the saved profile at index.
    Profile(usize),
    PassField,
    /// Show/hide toggle beside the passphrase field.
    PassReveal,
    PassConnect,
    PassCancel,
    /// Retry after an authentication failure.
    Retry,
    /// Forget confirmation: confirm or keep the saved profile.
    ForgetConfirm,
    ForgetCancel,
    /// The "Add network" row under the access points.
    AddNetwork,
    /// Add network form: name field, a security segment by index,
    /// passphrase field, its show/hide toggle, Connect, Cancel.
    AddName,
    AddSecurity(usize),
    AddPassphrase,
    AddReveal,
    AddConnect,
    AddCancel,
    Key(Key),
    Airplane,
    Wifi,
    Cellular,
    CellularData,
    /// Bluetooth controller power.
    BluetoothPower,
    /// "Pair new device" on the saved-devices screen.
    BtPairNew,
    /// A saved device row: opens its detail.
    BtSaved(usize),
    /// The scan row of Pair new device (scan again when idle).
    BtScan,
    /// An available device row: pair it.
    BtAvailable(usize),
    /// The PIN card's field (focus only; keys go to it while shown).
    BtPinField,
    /// Pairing card: Pair (accept), Cancel (decline/cancel), Done, Try again.
    BtAccept,
    BtDecline,
    BtDone,
    BtRetry,
    /// Device detail: Connect/Disconnect, Forget, and its confirmation.
    BtConnect,
    BtForget,
    BtForgetConfirm,
    BtForgetCancel,
    /// The brightness rail (dragged, committed on release).
    Brightness,
    AutoRotate,
    /// The output volume rail (dragged, committed on release).
    Volume,
    /// Output (playback) mute.
    Mute,
    /// A layout row on the Keyboard page, by index.
    KbdLayout(usize),
    /// The Keyboard page's scratch field.
    KbdTry,
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

    /// Root-page category rows, in [`Page::SECTIONS`] order.
    pub fn section_rects(&self) -> Vec<(Rect, Page)> {
        list_rects(self.size, self.s(300), Page::SECTIONS.len(), 150)
            .into_iter()
            .zip(Page::SECTIONS)
            .collect()
    }

    /// A full-width toggle/info row at design `y` (the Radios row shape).
    pub fn row_rect(&self, y: i32) -> Rect {
        let s = |v| self.s(v);
        Rect {
            x: s(48),
            y: s(y),
            w: self.w() - s(96),
            h: s(150),
        }
    }

    pub fn bluetooth_power_rect(&self) -> Rect {
        self.row_rect(300)
    }

    /// "Pair new device" under the power row.
    pub fn bt_pair_row_rect(&self) -> Rect {
        self.row_rect(470)
    }

    /// Saved-device rows (at most six).
    pub fn saved_rects(&self, count: usize) -> Vec<Rect> {
        list_rects(self.size, self.s(710), count.min(6), 120)
    }

    /// The scan row of Pair new device.
    pub fn bt_scan_rect(&self) -> Rect {
        self.row_rect(300)
    }

    /// The scan's remaining-time rail under the scan row.
    pub fn bt_progress_rect(&self) -> Rect {
        let s = |v| self.s(v);
        Rect {
            x: s(64),
            y: s(468),
            w: self.w() - s(128),
            h: s(8),
        }
    }

    /// Available-device rows (at most eight).
    pub fn available_rects(&self, count: usize) -> Vec<Rect> {
        list_rects(self.size, self.s(590), count.min(8), 120)
    }

    /// The PIN field of the legacy PIN card.
    pub fn bt_pin_field_rect(&self) -> Rect {
        let s = |v| self.s(v);
        Rect {
            x: s(96),
            y: s(820),
            w: self.w() - s(192),
            h: s(110),
        }
    }

    /// The left (primary) and right buttons of a pairing card.
    pub fn bt_card_left_rect(&self) -> Rect {
        self.pass_connect_rect()
    }

    pub fn bt_card_right_rect(&self) -> Rect {
        self.pass_cancel_rect()
    }

    /// The detail view's device row.
    pub fn bt_device_rect(&self) -> Rect {
        self.row_rect(300)
    }

    pub fn bt_connect_rect(&self) -> Rect {
        let s = |v| self.s(v);
        Rect {
            x: s(96),
            y: s(640),
            w: s(420),
            h: s(120),
        }
    }

    pub fn bt_forget_rect(&self) -> Rect {
        Rect {
            x: self.s(564),
            ..self.bt_connect_rect()
        }
    }

    pub fn bt_forget_confirm_rect(&self) -> Rect {
        Rect {
            y: self.s(940),
            ..self.bt_connect_rect()
        }
    }

    pub fn bt_forget_cancel_rect(&self) -> Rect {
        Rect {
            y: self.s(940),
            ..self.bt_forget_rect()
        }
    }

    /// Mobile page info rows: SIM, operator, signal.
    pub fn mobile_rects(&self) -> [Rect; 3] {
        [self.row_rect(300), self.row_rect(470), self.row_rect(640)]
    }

    pub fn mobile_data_rect(&self) -> Rect {
        self.row_rect(860)
    }

    /// The visible brightness track.
    pub fn brightness_rect(&self) -> Rect {
        let s = |v| self.s(v);
        Rect {
            x: s(130),
            y: s(470),
            w: self.w() - s(260),
            h: s(28),
        }
    }

    /// The rail's touch target: the track plus a generous vertical margin
    /// and the knob's overhang at both ends.
    pub fn brightness_hit_rect(&self) -> Rect {
        let r = self.brightness_rect();
        let s = |v| self.s(v);
        Rect {
            x: r.x - s(40),
            y: r.y - s(60),
            w: r.w + s(80),
            h: r.h + s(120),
        }
    }

    pub fn auto_rotate_rect(&self) -> Rect {
        self.row_rect(700)
    }

    /// The visible volume track (the brightness rail's geometry).
    pub fn volume_rect(&self) -> Rect {
        self.brightness_rect()
    }

    pub fn volume_hit_rect(&self) -> Rect {
        self.brightness_hit_rect()
    }

    pub fn mute_rect(&self) -> Rect {
        self.row_rect(700)
    }

    /// The read-only active output row.
    pub fn output_route_rect(&self) -> Rect {
        self.row_rect(870)
    }

    /// The visible track of a rail target.
    fn rail_rect(&self, target: Target) -> Option<Rect> {
        match target {
            Target::Brightness => Some(self.brightness_rect()),
            Target::Volume => Some(self.volume_rect()),
            _ => None,
        }
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

    /// The failure notice, narrowed when a Retry button sits beside it.
    pub fn notice_rect(&self, retry: bool) -> Rect {
        let mut r = self.error_rect();
        if retry {
            r.w -= self.retry_rect().w + self.s(16);
        }
        r
    }

    pub fn retry_rect(&self) -> Rect {
        let e = self.error_rect();
        let w = self.s(220);
        Rect {
            x: e.x + e.w - w,
            y: e.y,
            w,
            h: e.h,
        }
    }

    pub fn forget_confirm_rect(&self) -> Rect {
        self.pass_connect_rect()
    }

    pub fn forget_cancel_rect(&self) -> Rect {
        self.pass_cancel_rect()
    }

    /// Access-point rows (at most four, so Add network fits below them).
    pub fn ap_rects(&self, count: usize) -> Vec<Rect> {
        list_rects(self.size, self.s(900), count.min(4), 120)
    }

    /// The Add network row: the slot after `aps` access-point rows (after
    /// the "none" line when the list is empty).
    pub fn add_row_rect(&self, aps: usize) -> Rect {
        let slot = aps.clamp(1, 4);
        list_rects(self.size, self.s(900), slot + 1, 120)[slot]
    }

    pub fn profile_rects(&self, count: usize) -> Vec<Rect> {
        list_rects(self.size, self.s(1650), count.min(3), 100)
    }

    pub fn pass_field_rect(&self) -> Rect {
        let s = |v| self.s(v);
        let reveal = self.pass_reveal_rect();
        Rect {
            x: s(48),
            y: s(900),
            w: reveal.x - s(16) - s(48),
            h: s(100),
        }
    }

    pub fn pass_reveal_rect(&self) -> Rect {
        let s = |v| self.s(v);
        Rect {
            x: self.w() - s(48) - s(180),
            y: s(900),
            w: s(180),
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
        self.row_rect(400)
    }

    pub fn wifi_rect(&self) -> Rect {
        self.row_rect(600)
    }

    pub fn cellular_rect(&self) -> Rect {
        self.row_rect(800)
    }
}

pub fn keyboard_visible(model: &Model) -> bool {
    (model.page == Page::Network && (model.passphrase_for.is_some() || model.add_network.is_some()))
        || model.pin_entry()
        || (model.page == Page::Keyboard && model.kbd_try_focused)
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
    match model.page {
        Page::Root => l
            .section_rects()
            .into_iter()
            .find(|(r, _)| contains(*r, position))
            .map(|(_, page)| Target::Section(page)),
        Page::Bluetooth => bluetooth_hit(model, &l, position),
        Page::Mobile => contains(l.mobile_data_rect(), position).then_some(Target::CellularData),
        Page::Display => {
            if contains(l.brightness_hit_rect(), position) {
                return Some(Target::Brightness);
            }
            contains(l.auto_rotate_rect(), position).then_some(Target::AutoRotate)
        }
        Page::Sound => {
            if contains(l.volume_hit_rect(), position) {
                return Some(Target::Volume);
            }
            contains(l.mute_rect(), position).then_some(Target::Mute)
        }
        Page::Keyboard => crate::keyboard::hit(&l, position),
        Page::Network => {
            if contains(l.radio_rect(), position) {
                return Some(Target::Radio);
            }
            if contains(l.scan_rect(), position) {
                return Some(Target::Scan);
            }
            if model.forget_confirm.is_some() {
                if contains(l.forget_confirm_rect(), position) {
                    return Some(Target::ForgetConfirm);
                }
                return contains(l.forget_cancel_rect(), position).then_some(Target::ForgetCancel);
            }
            if model.add_network.is_some() {
                return crate::hidden::hit(model, &l, position);
            }
            if model.passphrase_for.is_some() {
                if contains(l.pass_field_rect(), position) {
                    return Some(Target::PassField);
                }
                if contains(l.pass_reveal_rect(), position) {
                    return Some(Target::PassReveal);
                }
                if contains(l.pass_connect_rect(), position) {
                    return Some(Target::PassConnect);
                }
                return contains(l.pass_cancel_rect(), position).then_some(Target::PassCancel);
            }
            if model.can_retry() && contains(l.retry_rect(), position) {
                return Some(Target::Retry);
            }
            let aps = model.access_points().len();
            if let Some(i) = l
                .ap_rects(aps)
                .into_iter()
                .position(|r| contains(r, position))
            {
                return Some(Target::Ap(i));
            }
            if contains(l.add_row_rect(aps), position) {
                return Some(Target::AddNetwork);
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
            contains(l.cellular_rect(), position).then_some(Target::Cellular)
        }
        Page::About => None,
    }
}

/// The buttons a pairing card shows, left to right.
pub fn card_buttons(card: Card) -> (Option<Target>, Option<Target>) {
    match card {
        Card::Confirm { .. } | Card::Authorize | Card::Pin => {
            (Some(Target::BtAccept), Some(Target::BtDecline))
        }
        Card::Display { .. } | Card::Pairing => (None, Some(Target::BtDecline)),
        Card::Paired => (Some(Target::BtDone), None),
        Card::Failed(_) => (Some(Target::BtRetry), Some(Target::BtDone)),
    }
}

fn bluetooth_hit(model: &Model, l: &Layout, position: (f64, f64)) -> Option<Target> {
    if let Some(card) = model.bt_card() {
        // The card replaces the lists; only its buttons (and, on the saved
        // screen, the power row) answer.
        if card == Card::Pin && contains(l.bt_pin_field_rect(), position) {
            return Some(Target::BtPinField);
        }
        let (left, right) = card_buttons(card);
        if left.is_some() && contains(l.bt_card_left_rect(), position) {
            return left;
        }
        if right.is_some() && contains(l.bt_card_right_rect(), position) {
            return right;
        }
        return (model.bt_view == BtView::Main && contains(l.bluetooth_power_rect(), position))
            .then_some(Target::BluetoothPower);
    }
    match &model.bt_view {
        BtView::Main => {
            if contains(l.bluetooth_power_rect(), position) {
                return Some(Target::BluetoothPower);
            }
            if model.bluetooth_block().is_err() {
                return None;
            }
            if contains(l.bt_pair_row_rect(), position) {
                return Some(Target::BtPairNew);
            }
            l.saved_rects(model.saved_devices().len())
                .into_iter()
                .position(|r| contains(r, position))
                .map(Target::BtSaved)
        }
        BtView::Pair => {
            if contains(l.bt_scan_rect(), position) {
                return Some(Target::BtScan);
            }
            l.available_rects(model.available_devices().len())
                .into_iter()
                .position(|r| contains(r, position))
                .map(Target::BtAvailable)
        }
        BtView::Device(_) => {
            if model.bt_forget_confirm {
                if contains(l.bt_forget_confirm_rect(), position) {
                    return Some(Target::BtForgetConfirm);
                }
                return contains(l.bt_forget_cancel_rect(), position)
                    .then_some(Target::BtForgetCancel);
            }
            if contains(l.bt_connect_rect(), position) {
                return Some(Target::BtConnect);
            }
            contains(l.bt_forget_rect(), position).then_some(Target::BtForget)
        }
    }
}

pub fn activate(model: &mut Model, target: Target) {
    match target {
        Target::Back => model.back(),
        Target::Home => {}
        Target::Section(page) => model.show(page),
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
        Target::PassReveal => model.passphrase_shown = !model.passphrase_shown,
        Target::PassConnect => {
            if let Err(reason) = model.submit_passphrase() {
                model.last_error = Some(reason);
            }
        }
        Target::PassCancel => model.cancel_passphrase(),
        Target::Retry => model.retry_connect(),
        Target::ForgetConfirm => model.confirm_forget(),
        Target::ForgetCancel => model.cancel_forget(),
        Target::AddNetwork => model.open_add_network(),
        Target::AddName => model.add_network_focus(crate::hidden::AddField::Name),
        Target::AddSecurity(i) => model.add_network_security(i),
        Target::AddPassphrase => model.add_network_focus(crate::hidden::AddField::Passphrase),
        Target::AddReveal => model.toggle_add_network_reveal(),
        Target::AddConnect => {
            if let Err(reason) = model.submit_add_network() {
                model.last_error = Some(reason);
            }
        }
        Target::AddCancel => model.cancel_add_network(),
        Target::Key(key) => key_press(model, key),
        Target::Airplane => model.toggle_airplane(),
        Target::Cellular => model.toggle_cellular(),
        Target::CellularData => model.toggle_cellular_data(),
        Target::BluetoothPower => model.toggle_bluetooth(),
        Target::BtPairNew => model.open_pair_page(),
        Target::BtSaved(i) => model.open_saved(i),
        Target::BtScan => {
            if !model.discovery().active {
                model.rescan();
            }
        }
        Target::BtAvailable(i) => model.pair_available(i),
        Target::BtPinField => {}
        Target::BtAccept => model.reply_pairing(true),
        Target::BtDecline => match model.bt_card() {
            Some(card) if card.asks() => model.reply_pairing(false),
            _ => model.dismiss_pairing(),
        },
        Target::BtDone => model.finish_pairing(),
        Target::BtRetry => model.retry_pairing(),
        Target::BtConnect => model.toggle_connection(),
        Target::BtForget => model.ask_forget_device(),
        Target::BtForgetConfirm => model.confirm_forget_device(),
        Target::BtForgetCancel => model.cancel_forget_device(),
        // The rail acts on drag and release (see `Input`), a tap included.
        Target::Brightness => model.commit_brightness(),
        Target::AutoRotate => model.toggle_auto_rotate(),
        Target::Volume => model.commit_volume(),
        Target::Mute => model.toggle_mute(),
        Target::KbdLayout(i) => model.select_keyboard_layout(i),
        Target::KbdTry => model.focus_keyboard_try(),
    }
}

pub fn key_press(model: &mut Model, key: Key) {
    if model.pin_entry() {
        model.pin_key(key);
        return;
    }
    if model.keyboard_key(key) || model.add_network_key(key) || model.passphrase_for.is_none() {
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

/// Previews a rail target's value at horizontal position `x`.
fn drag_rail(model: &mut Model, target: Target, rail: Rect, x: f64) {
    let value = slider_value(rail, x);
    match target {
        Target::Brightness => model.drag_brightness(value),
        Target::Volume => model.drag_volume(value),
        _ => {}
    }
}

fn rail_value(model: &Model, target: Target) -> Option<u8> {
    match target {
        Target::Brightness => model.brightness(),
        Target::Volume => model.volume(),
        _ => None,
    }
}

fn commit_rail(model: &mut Model, target: Target) {
    match target {
        Target::Brightness => model.commit_brightness(),
        Target::Volume => model.commit_volume(),
        _ => {}
    }
}

#[derive(Default)]
pub struct Input {
    tracker: Tracker<Target>,
    /// Repeat timer for a held on-screen key.
    repeat: KeyRepeat,
    pub pressed: Option<Target>,
}

impl Input {
    pub fn down(
        &mut self,
        id: i32,
        position: (f64, f64),
        model: &mut Model,
        size: (u32, u32),
    ) -> bool {
        let target = hit_test(model, position, size);
        match self.tracker.down(id, position, target) {
            Some(pressed) => {
                if let Some(Target::Key(key)) = pressed {
                    self.repeat.arm(key, Instant::now());
                }
                self.pressed = pressed;
                if let Some(target) = pressed
                    && let Some(rail) = Layout::new(size).rail_rect(target)
                {
                    drag_rail(model, target, rail, position.0);
                }
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
        if self.tracker.is_tracking(id)
            && let Some(target) = self.tracker.target()
            && let Some(rail) = Layout::new(size).rail_rect(target)
        {
            // Keep the contact's position current so release commits it.
            let _ = self.tracker.motion(id, position, size, None, None);
            let before = rail_value(model, target);
            drag_rail(model, target, rail, position.0);
            return rail_value(model, target) != before;
        }
        let at = hit_test(model, position, size);
        match self.tracker.motion(id, position, size, at, self.pressed) {
            Some(pressed) => {
                if pressed.is_none() {
                    self.repeat.cancel();
                }
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
        if self.tracker.is_tracking(id)
            && let Some(target) = self.tracker.target()
            && let Some(rail) = Layout::new(size).rail_rect(target)
        {
            if let Some(p) = position.or_else(|| self.tracker.current(id)) {
                drag_rail(model, target, rail, p.0);
            }
            let _ = self.tracker.up(id, size, None);
            self.pressed = None;
            commit_rail(model, target);
            return true;
        }
        let at = position
            .or_else(|| self.tracker.current(id))
            .and_then(|p| hit_test(model, p, size));
        let Some(gesture) = self.tracker.up(id, size, at) else {
            return false;
        };
        self.pressed = None;
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
    use crate::model::{Action, preview_network};
    use ferrix_network_api::ApId;
    use ferrix_network_protocol::Feed;
    use ferrix_ui::widgets::center;

    const SIZE: (u32, u32) = (1080, 2340);

    fn ready() -> Model {
        let mut m = Model::new();
        m.show(Page::Network);
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
        // Masked by default; the toggle reveals and hides it again.
        assert!(!m.passphrase_shown);
        tap(&mut input, &mut m, center(l.pass_reveal_rect()));
        assert!(m.passphrase_shown);
        assert_eq!(m.passphrase, "mockpass", "toggling does not edit");
        tap(&mut input, &mut m, center(l.pass_reveal_rect()));
        assert!(!m.passphrase_shown);
        tap(&mut input, &mut m, center(l.pass_reveal_rect()));
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
        assert!(!m.passphrase_shown, "a new prompt starts masked");
        // Hardware Shift/Caps Lock reach the passphrase with their case.
        let shift = ferrix_ui::keymap::Modifiers::from_xkb(0x1, 0);
        for (code, mods) in [(25, shift), (30, Default::default()), (2, shift)] {
            key_press(&mut m, ferrix_ui::keymap::text_key(code, mods).unwrap());
        }
        assert_eq!(m.passphrase, "Pa!");
        tap(&mut input, &mut m, center(l.pass_cancel_rect()));
        assert_eq!(m.passphrase_for, None);
        tap(&mut input, &mut m, center(l.profile_rects(1)[0]));
        assert!(m.pending.is_empty(), "a single tap never forgets");
        assert_eq!(
            hit_test(&m, center(l.profile_rects(1)[0]), SIZE),
            None,
            "list hidden behind the confirmation"
        );
        tap(&mut input, &mut m, center(l.forget_cancel_rect()));
        assert_eq!(m.forget_confirm, None);
        assert!(m.pending.is_empty());
        tap(&mut input, &mut m, center(l.profile_rects(1)[0]));
        tap(&mut input, &mut m, center(l.forget_confirm_rect()));
        assert!(matches!(m.pending.pop(), Some(Action::Forget(_))));
        // Back returns to the category list; a row opens its page; Back
        // from the list yields.
        tap(&mut input, &mut m, (80.0, 160.0));
        assert_eq!(m.page, Page::Root);
        let rows = l.section_rects();
        assert_eq!(rows.len(), Page::SECTIONS.len());
        let radios = rows.iter().find(|(_, p)| *p == Page::Radios).unwrap().0;
        tap(&mut input, &mut m, center(radios));
        assert_eq!(m.page, Page::Radios);
        tap(&mut input, &mut m, center(l.airplane_rect()));
        assert_eq!(m.pending.len(), 2);
        m.pending.clear();
        tap(&mut input, &mut m, center(l.cellular_rect()));
        assert!(matches!(m.pending.pop(), Some(Action::SetRadioPower(_))));
        tap(&mut input, &mut m, (80.0, 160.0));
        assert_eq!(m.page, Page::Root);
        tap(&mut input, &mut m, (80.0, 160.0));
        assert_eq!(m.pending, vec![Action::Back]);
        assert_eq!(hit_test(&m, (540.0, 2290.0), SIZE), Some(Target::Home));
    }

    fn with_core() -> Model {
        let mut m = ready();
        m.apply_core(ferrix_protocol::Feed::Snapshot {
            target: "qemu-aarch64".into(),
            profile: "emulator-dev".into(),
            capabilities: crate::system::preview_capabilities(),
            state: crate::system::preview_hardware(),
        });
        m
    }

    #[test]
    fn brightness_rail_drags_and_commits_on_release() {
        let mut m = with_core();
        m.show(Page::Display);
        let l = Layout::new(SIZE);
        let rail = l.brightness_rect();
        let mut input = Input::default();
        let y = f64::from(rail.y + rail.h / 2);
        let start = (f64::from(rail.x + rail.w / 4), y);
        assert!(input.down(0, start, &mut m, SIZE));
        assert_eq!(m.brightness(), Some(25), "touch-down previews");
        let end = (f64::from(rail.x + rail.w * 3 / 4), y + 40.0);
        assert!(input.motion(0, end, &mut m, SIZE));
        assert_eq!(m.brightness(), Some(75));
        assert!(m.pending.is_empty(), "nothing sent while dragging");
        assert!(input.up(0, None, &mut m, SIZE));
        assert_eq!(m.pending, vec![Action::SetBrightness(75)]);
        assert_eq!(input.pressed, None);
        // Dragging off the rail's ends clamps.
        m.pending.clear();
        input.down(0, start, &mut m, SIZE);
        input.motion(0, (5000.0, y), &mut m, SIZE);
        input.up(0, Some((5000.0, y)), &mut m, SIZE);
        assert_eq!(m.pending, vec![Action::SetBrightness(100)]);
        m.pending.clear();
        tap(&mut input, &mut m, center(l.auto_rotate_rect()));
        assert_eq!(m.pending, vec![Action::SetRotationLock(true)]);
    }

    #[test]
    fn sound_page_volume_rail_and_mute_issue_typed_actions() {
        let mut m = with_core();
        m.show(Page::Root);
        let l = Layout::new(SIZE);
        let mut input = Input::default();
        let sound = l
            .section_rects()
            .into_iter()
            .find(|(_, p)| *p == Page::Sound)
            .unwrap()
            .0;
        tap(&mut input, &mut m, center(sound));
        assert_eq!(m.page, Page::Sound);
        let rail = l.volume_rect();
        let y = f64::from(rail.y + rail.h / 2);
        assert!(input.down(0, (f64::from(rail.x + rail.w / 4), y), &mut m, SIZE));
        assert_eq!(m.volume(), Some(25), "touch-down previews");
        assert!(input.motion(0, (f64::from(rail.x + rail.w / 2), y), &mut m, SIZE));
        assert_eq!(m.volume(), Some(50));
        assert!(m.pending.is_empty(), "nothing sent while dragging");
        assert!(input.up(0, None, &mut m, SIZE));
        assert_eq!(m.pending, vec![Action::SetVolume(50)]);
        assert_eq!(
            m.brightness_drag, None,
            "the volume rail leaves brightness alone"
        );
        m.pending.clear();
        tap(&mut input, &mut m, center(l.mute_rect()));
        assert_eq!(m.pending, vec![Action::SetMuted(true)]);
        assert_eq!(hit_test(&m, center(l.output_route_rect()), SIZE), None);
    }

    #[test]
    fn bluetooth_and_mobile_rows_route_to_their_controls() {
        let mut m = with_core();
        let mut input = Input::default();
        let l = Layout::new(SIZE);
        let row = |page| {
            l.section_rects()
                .into_iter()
                .find(|(_, p)| *p == page)
                .unwrap()
                .0
        };
        m.show(Page::Root);
        tap(&mut input, &mut m, center(row(Page::Bluetooth)));
        assert_eq!(m.page, Page::Bluetooth);
        assert_eq!(m.pending, vec![Action::RefreshBluetoothDevices]);
        m.pending.clear();
        tap(&mut input, &mut m, center(l.bluetooth_power_rect()));
        assert_eq!(m.pending, vec![Action::SetBluetooth(false)]);
        m.pending.clear();
        m.show(Page::Root);
        tap(&mut input, &mut m, center(row(Page::Mobile)));
        assert_eq!(m.page, Page::Mobile);
        tap(&mut input, &mut m, center(l.mobile_data_rect()));
        assert!(m.pending.is_empty());
        assert_eq!(
            m.last_error.as_deref(),
            Some("telephony \u{b7} unavailable"),
            "no snapshot yet: refused with the reason"
        );
        for r in l.mobile_rects() {
            assert_eq!(hit_test(&m, center(r), SIZE), None, "info rows are inert");
        }
    }

    #[test]
    fn bluetooth_pairing_flow_routes_taps_to_core_requests() {
        use crate::bluetooth::preview_session;
        use ferrix_hardware_api::{BluetoothCommand as B, PairingPhase};
        let mut m = with_core();
        m.show(Page::Bluetooth);
        m.pending.clear();
        let mut session = preview_session(None);
        session.discovery = Default::default();
        m.set_bluetooth_session(Ok(session));
        let mut input = Input::default();
        let l = Layout::new(SIZE);
        tap(&mut input, &mut m, center(l.bt_pair_row_rect()));
        assert_eq!(m.pending, vec![Action::Bluetooth(B::StartDiscovery)]);
        m.pending.clear();
        m.set_bluetooth_session(Ok(preview_session(None)));
        assert_eq!(
            hit_test(&m, center(l.bt_scan_rect()), SIZE),
            Some(Target::BtScan)
        );
        tap(&mut input, &mut m, center(l.bt_scan_rect()));
        assert!(m.pending.is_empty(), "no restart while scanning");
        tap(&mut input, &mut m, center(l.available_rects(4)[1]));
        assert_eq!(
            m.pending,
            vec![Action::Bluetooth(B::Pair {
                device: "mock-bt-found-1".into()
            })]
        );
        m.pending.clear();
        m.set_bluetooth_session(Ok(preview_session(Some(PairingPhase::ConfirmPasskey {
            passkey: 1,
        }))));
        assert_eq!(
            hit_test(&m, center(l.available_rects(4)[1]), SIZE),
            None,
            "the list is hidden behind the card"
        );
        tap(&mut input, &mut m, center(l.bt_card_left_rect()));
        assert_eq!(
            m.pending,
            vec![Action::Bluetooth(B::ReplyPairing { accept: true })]
        );
        m.pending.clear();
        tap(&mut input, &mut m, center(l.bt_card_right_rect()));
        assert_eq!(
            m.pending,
            vec![Action::Bluetooth(B::ReplyPairing { accept: false })]
        );
        m.pending.clear();
        m.set_bluetooth_session(Ok(preview_session(Some(PairingPhase::Pairing))));
        assert_eq!(
            hit_test(&m, center(l.bt_card_left_rect()), SIZE),
            None,
            "no Pair button while bonding"
        );
        tap(&mut input, &mut m, center(l.bt_card_right_rect()));
        assert_eq!(m.pending, vec![Action::Bluetooth(B::CancelPairing)]);
        m.pending.clear();
        m.set_bluetooth_session(Ok(preview_session(Some(PairingPhase::Paired))));
        tap(&mut input, &mut m, center(l.bt_card_left_rect()));
        assert_eq!(m.bt_view, crate::bluetooth::BtView::Main);
        assert!(m.pending.contains(&Action::Bluetooth(B::CancelPairing)));
        m.pending.clear();
        // Saved device detail: Disconnect, then Forget behind a confirmation.
        m.set_bluetooth_session(Ok(preview_session(None)));
        tap(&mut input, &mut m, center(l.saved_rects(2)[0]));
        assert!(matches!(m.bt_view, crate::bluetooth::BtView::Device(_)));
        tap(&mut input, &mut m, center(l.bt_connect_rect()));
        assert_eq!(
            m.pending,
            vec![Action::Bluetooth(B::Disconnect {
                device: "mock-bt-0".into()
            })]
        );
        m.pending.clear();
        tap(&mut input, &mut m, center(l.bt_forget_rect()));
        assert!(m.pending.is_empty());
        tap(&mut input, &mut m, center(l.bt_forget_confirm_rect()));
        assert_eq!(
            m.pending,
            vec![Action::Bluetooth(B::Forget {
                device: "mock-bt-0".into()
            })]
        );
    }

    #[test]
    fn retry_appears_only_after_an_authentication_failure() {
        use ferrix_network_api::{ConnectionState, NetworkError, NetworkEvent};
        let mut m = ready();
        let mut input = Input::default();
        let l = Layout::new(SIZE);
        assert_eq!(hit_test(&m, center(l.retry_rect()), SIZE), None);
        tap(&mut input, &mut m, center(l.ap_rects(3)[0]));
        m.pending.clear();
        m.apply_network(Feed::Event(NetworkEvent::ConnectionChanged {
            connection: ConnectionState::Failed {
                error: NetworkError::AuthenticationFailed,
            },
        }));
        assert_eq!(
            hit_test(&m, center(l.retry_rect()), SIZE),
            Some(Target::Retry)
        );
        tap(&mut input, &mut m, center(l.retry_rect()));
        assert_eq!(m.passphrase_for, Some(ApId::new("ap-0")));
        assert!(keyboard_visible(&m));
        let notice = l.notice_rect(true);
        assert!(
            notice.x + notice.w < l.retry_rect().x,
            "notice clears Retry"
        );
    }
}
