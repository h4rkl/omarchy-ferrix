//! Settings > Wi-Fi > Add network: join a network that broadcasts no name.
//!
//! A hidden network's beacon carries an empty SSID, so the list cannot
//! offer it; the user types the name, picks the security (Open, WPA2, or
//! WPA3; the beacon cannot be read for it) and, when one is needed, the
//! passphrase, all on the shared on-screen keyboard. The request is
//! `join-hidden` on `ferrix-networkd`. Model, geometry, hit test, and
//! drawing for the flow live together, like the Keyboard page.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::fmt;

use ferrix_network_api::{Passphrase, SSID_MAX_LEN, SecurityType, Ssid, validate_hidden_join};
use ferrix_ui::draw::{Canvas, Rect, Type};
use ferrix_ui::icons::Icon;
use ferrix_ui::osk::{self, Key, OskState};
use ferrix_ui::widgets::{
    ButtonKind, Edit, Tone, Trailing, button, chip, contains, edit_text, list_row_with,
    section_label, text_field, wrap_text,
};

use crate::interaction::{Layout, Target};
use crate::model::{Action, Model};

/// The security choices, in the order the segment row shows them.
pub const SECURITY_CHOICES: [(SecurityType, &str); 3] = [
    (SecurityType::Open, "Open"),
    (SecurityType::Wpa2Psk, "WPA2"),
    (SecurityType::Wpa3Sae, "WPA3"),
];

/// Shortest passphrase the form accepts (the WPA2 minimum; WPA3 is held to
/// the same rule so both choices behave alike).
pub const MIN_PASSPHRASE: usize = 8;
/// Longest WPA2 passphrase (64 hex digits are a raw key and also accepted).
pub const MAX_WPA2_PASSPHRASE: usize = 63;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddField {
    Name,
    Passphrase,
}

/// The form's draft. `Debug` never prints the passphrase.
#[derive(Clone, PartialEq, Eq)]
pub struct AddNetwork {
    pub ssid: String,
    pub security: SecurityType,
    pub passphrase: String,
    /// The passphrase shows plain text instead of bullets.
    pub shown: bool,
    pub focus: AddField,
}

impl fmt::Debug for AddNetwork {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AddNetwork")
            .field("ssid", &self.ssid)
            .field("security", &self.security)
            .field("passphrase", &"<redacted>")
            .field("shown", &self.shown)
            .field("focus", &self.focus)
            .finish()
    }
}

impl Default for AddNetwork {
    /// WPA2 is preselected: most hidden networks use it, and a protected
    /// default never joins an open network by mistake.
    fn default() -> Self {
        Self {
            ssid: String::new(),
            security: SecurityType::Wpa2Psk,
            passphrase: String::new(),
            shown: false,
            focus: AddField::Name,
        }
    }
}

impl AddNetwork {
    pub fn needs_passphrase(&self) -> bool {
        self.security.needs_passphrase()
    }

    /// Why the draft cannot be sent yet, in the form's own words.
    pub fn problem(&self) -> Option<&'static str> {
        if self.ssid.is_empty() {
            return Some("network name \u{b7} required");
        }
        if self.ssid.len() > SSID_MAX_LEN {
            return Some("network name \u{b7} longer than 32 bytes");
        }
        if !self.needs_passphrase() {
            return None;
        }
        let chars = self.passphrase.chars().count();
        let raw_key =
            self.passphrase.len() == 64 && self.passphrase.chars().all(|c| c.is_ascii_hexdigit());
        if chars < MIN_PASSPHRASE {
            Some("passphrase \u{b7} shorter than 8 characters")
        } else if self.security == SecurityType::Wpa2Psk && chars > MAX_WPA2_PASSPHRASE && !raw_key
        {
            Some("passphrase \u{b7} longer than 63 characters")
        } else {
            None
        }
    }
}

impl Model {
    /// The "Add network" row: opens the form, or says why it cannot.
    pub fn open_add_network(&mut self) {
        if let Err(reason) = self.controllable() {
            self.last_error = Some(reason);
            return;
        }
        if !self.radio_enabled() {
            self.last_error = Some(self.radio_label());
            return;
        }
        self.passphrase_for = None;
        self.passphrase.clear();
        self.add_network = Some(AddNetwork::default());
        self.osk = OskState::new(osk::Layout::Text);
        self.last_error = None;
    }

    /// Reopens the form for the network the last join by name named, with
    /// the passphrase asked again (Retry after a failure).
    pub(crate) fn reopen_add_network(&mut self, ssid: String, security: SecurityType) {
        self.add_network = Some(AddNetwork {
            ssid,
            security,
            focus: if security.needs_passphrase() {
                AddField::Passphrase
            } else {
                AddField::Name
            },
            ..AddNetwork::default()
        });
        self.osk = OskState::new(osk::Layout::Text);
        self.last_error = None;
    }

    pub fn cancel_add_network(&mut self) {
        self.add_network = None;
        self.last_error = None;
    }

    pub fn add_network_security(&mut self, index: usize) {
        let Some((security, _)) = SECURITY_CHOICES.get(index) else {
            return;
        };
        if let Some(form) = self.add_network.as_mut() {
            form.security = *security;
            if !form.needs_passphrase() {
                form.passphrase.clear();
                form.focus = AddField::Name;
            }
            self.last_error = None;
        }
    }

    pub fn add_network_focus(&mut self, field: AddField) {
        if let Some(form) = self.add_network.as_mut()
            && (field == AddField::Name || form.needs_passphrase())
        {
            form.focus = field;
        }
    }

    pub fn toggle_add_network_reveal(&mut self) {
        if let Some(form) = self.add_network.as_mut() {
            form.shown = !form.shown;
        }
    }

    /// Sends `join-hidden` for a complete draft. The form closes and the
    /// attempt is remembered (name and security only) for Retry.
    pub fn submit_add_network(&mut self) -> Result<(), String> {
        let Some(form) = self.add_network.as_ref() else {
            return Err("no network to add".to_owned());
        };
        if let Some(problem) = form.problem() {
            return Err(problem.to_owned());
        }
        self.controllable()?;
        let passphrase = form.needs_passphrase().then(|| form.passphrase.clone());
        // The form's own checks mirror the service's; this keeps them equal.
        validate_hidden_join(
            &Ssid::from(form.ssid.as_str()),
            form.security,
            passphrase.as_ref().map(Passphrase::new).as_ref(),
        )
        .map_err(|error| format!("network \u{b7} {error}"))?;
        let Some(form) = self.add_network.take() else {
            return Err("no network to add".to_owned());
        };
        self.last_connect = None;
        self.last_hidden = Some((form.ssid.clone(), form.security));
        self.connect_error = None;
        self.last_error = None;
        self.pending.push(Action::JoinHidden {
            ssid: form.ssid,
            security: form.security,
            passphrase,
        });
        Ok(())
    }

    /// Routes a key to the focused field while the form is open; `false`
    /// when the form is not shown.
    pub fn add_network_key(&mut self, key: Key) -> bool {
        let Some(form) = self.add_network.as_mut() else {
            return false;
        };
        let mut osk = self.osk;
        let outcome = match form.focus {
            AddField::Name => {
                let before = form.ssid.clone();
                let outcome = edit_text(&mut form.ssid, key, &mut osk);
                if form.ssid.len() > SSID_MAX_LEN {
                    form.ssid = before;
                    Edit::Unchanged
                } else {
                    outcome
                }
            }
            AddField::Passphrase => edit_text(&mut form.passphrase, key, &mut osk),
        };
        self.osk = osk;
        match outcome {
            // Enter on the name moves on to the passphrase when one is
            // needed; otherwise (and on the passphrase) it joins.
            Edit::Submit if form.focus == AddField::Name && form.needs_passphrase() => {
                form.focus = AddField::Passphrase;
            }
            Edit::Submit => {
                if let Err(reason) = self.submit_add_network() {
                    self.last_error = Some(reason);
                }
            }
            Edit::Dismiss => self.cancel_add_network(),
            Edit::Changed => self.last_error = None,
            Edit::Unchanged => {}
        }
        true
    }

    /// The Add network row's subtitle: what it is for, plus how many
    /// nameless beacons the last scan heard.
    pub fn add_network_summary(&self) -> String {
        match self.hidden_count() {
            0 => "join a hidden network by name".to_owned(),
            1 => "join a hidden network by name \u{b7} 1 hidden nearby".to_owned(),
            n => format!("join a hidden network by name \u{b7} {n} hidden nearby"),
        }
    }
}

// -- geometry ------------------------------------------------------------------

impl Layout {
    pub fn add_name_rect(&self) -> Rect {
        let s = |v| self.s(v);
        Rect {
            x: s(48),
            y: s(900),
            w: self.size.0 as i32 - s(96),
            h: s(100),
        }
    }

    /// The Open / WPA2 / WPA3 segment row.
    pub fn add_security_rects(&self) -> [Rect; 3] {
        let s = |v| self.s(v);
        let gap = s(16);
        let w = (self.size.0 as i32 - s(96) - 2 * gap) / 3;
        std::array::from_fn(|i| Rect {
            x: s(48) + i as i32 * (w + gap),
            y: s(1030),
            w,
            h: s(80),
        })
    }

    pub fn add_pass_rect(&self) -> Rect {
        Rect {
            y: self.s(1140),
            ..self.pass_field_rect()
        }
    }

    pub fn add_reveal_rect(&self) -> Rect {
        Rect {
            y: self.s(1140),
            ..self.pass_reveal_rect()
        }
    }

    pub fn add_connect_rect(&self) -> Rect {
        Rect {
            y: self.s(1270),
            ..self.pass_connect_rect()
        }
    }

    pub fn add_cancel_rect(&self) -> Rect {
        Rect {
            y: self.s(1270),
            ..self.pass_cancel_rect()
        }
    }
}

/// Taps while the form is shown; the list behind it does not answer.
pub fn hit(model: &Model, l: &Layout, position: (f64, f64)) -> Option<Target> {
    let form = model.add_network.as_ref()?;
    if contains(l.add_name_rect(), position) {
        return Some(Target::AddName);
    }
    if let Some(i) = l
        .add_security_rects()
        .into_iter()
        .position(|r| contains(r, position))
    {
        return Some(Target::AddSecurity(i));
    }
    if form.needs_passphrase() {
        if contains(l.add_pass_rect(), position) {
            return Some(Target::AddPassphrase);
        }
        if contains(l.add_reveal_rect(), position) {
            return Some(Target::AddReveal);
        }
    }
    if contains(l.add_connect_rect(), position) {
        return Some(Target::AddConnect);
    }
    contains(l.add_cancel_rect(), position).then_some(Target::AddCancel)
}

// -- drawing -------------------------------------------------------------------

/// The "Add network" row under the access points.
pub fn draw_row(c: &mut Canvas, ty: &Type, r: Rect, model: &Model, pressed: bool) {
    list_row_with(
        c,
        ty,
        r,
        None,
        "Add network",
        &model.add_network_summary(),
        Trailing::Icon(Icon::Add, Tone::Accent),
        pressed,
    );
}

pub fn draw_form(c: &mut Canvas, ty: &Type, l: &Layout, model: &Model, pressed: Option<Target>) {
    let Some(form) = model.add_network.as_ref() else {
        return;
    };
    let s = |v| l.s(v);
    section_label(c, ty, "add network", s(96), s(870));
    text_field(
        c,
        ty,
        l.add_name_rect(),
        &form.ssid,
        "Network name (SSID)",
        form.focus == AddField::Name,
    );
    for ((r, (security, label)), i) in l
        .add_security_rects()
        .into_iter()
        .zip(SECURITY_CHOICES)
        .zip(0..)
    {
        let tone = if form.security == security {
            Tone::Accent
        } else {
            Tone::Neutral
        };
        chip(
            c,
            ty,
            r,
            label,
            tone,
            pressed == Some(Target::AddSecurity(i)),
        );
    }
    if form.needs_passphrase() {
        let text: String = if form.shown {
            form.passphrase.clone()
        } else {
            form.passphrase.chars().map(|_| '\u{2022}').collect()
        };
        text_field(
            c,
            ty,
            l.add_pass_rect(),
            &text,
            "Passphrase",
            form.focus == AddField::Passphrase,
        );
        chip(
            c,
            ty,
            l.add_reveal_rect(),
            if form.shown { "hide" } else { "show" },
            Tone::Neutral,
            pressed == Some(Target::AddReveal),
        );
    } else {
        wrap_text(
            c,
            ty,
            "Open networks need no passphrase; traffic on them is not encrypted.",
            s(96),
            s(1160),
            c.width() - s(192),
            s(27),
        );
    }
    button(
        c,
        ty,
        l.add_connect_rect(),
        "Connect",
        ButtonKind::Primary,
        pressed == Some(Target::AddConnect),
        form.problem().is_none(),
    );
    button(
        c,
        ty,
        l.add_cancel_rect(),
        "Cancel",
        ButtonKind::Secondary,
        pressed == Some(Target::AddCancel),
        true,
    );
    wrap_text(
        c,
        ty,
        "To find a hidden network the phone calls out its name, so nearby devices can \
         see the name.",
        s(96),
        s(1460),
        c.width() - s(192),
        s(27),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interaction::{Input, hit_test, key_press, keyboard_visible};
    use crate::model::{Page, preview_network};
    use ferrix_network_api::{ConnectionState, NetworkError, NetworkEvent};
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

    fn type_text(input: &mut Input, model: &mut Model, text: &str) {
        for ch in text.chars() {
            let (r, _) = osk::keys(SIZE, &model.osk)
                .into_iter()
                .find(|(_, k)| *k == Key::Char(ch))
                .unwrap_or_else(|| panic!("key {ch:?} on the current layer"));
            tap(input, model, center(r));
        }
    }

    #[test]
    fn the_form_joins_by_name_with_the_chosen_security() {
        let mut m = ready();
        let mut input = Input::default();
        let l = Layout::new(SIZE);
        let row = l.add_row_rect(m.access_points().len());
        assert_eq!(hit_test(&m, center(row), SIZE), Some(Target::AddNetwork));
        tap(&mut input, &mut m, center(row));
        assert!(m.add_network.is_some());
        assert_eq!(m.state_name(), "add-network");
        assert!(keyboard_visible(&m));
        let (kb_top, _) = osk::extent(SIZE);
        for r in [l.add_connect_rect(), l.add_cancel_rect(), l.add_pass_rect()] {
            assert!(r.y + r.h < kb_top, "{r:?} stays above the keyboard");
        }
        for r in l.ap_rects(3) {
            assert!(
                !matches!(hit_test(&m, center(r), SIZE), Some(Target::Ap(_))),
                "the list is hidden behind the form"
            );
        }
        type_text(&mut input, &mut m, "hiddenlab");
        assert_eq!(m.add_network.as_ref().unwrap().ssid, "hiddenlab");
        // WPA2 is preselected; Connect waits for a passphrase.
        assert_eq!(
            m.add_network.as_ref().unwrap().problem(),
            Some("passphrase \u{b7} shorter than 8 characters")
        );
        tap(&mut input, &mut m, center(l.add_security_rects()[2]));
        assert_eq!(
            m.add_network.as_ref().unwrap().security,
            SecurityType::Wpa3Sae
        );
        tap(&mut input, &mut m, center(l.add_pass_rect()));
        type_text(&mut input, &mut m, "mockpass");
        assert_eq!(m.add_network.as_ref().unwrap().ssid, "hiddenlab");
        assert_eq!(m.add_network.as_ref().unwrap().passphrase, "mockpass");
        assert!(!format!("{:?}", m.add_network).contains("mockpass"));
        tap(&mut input, &mut m, center(l.add_reveal_rect()));
        assert!(m.add_network.as_ref().unwrap().shown);
        tap(&mut input, &mut m, center(l.add_connect_rect()));
        assert_eq!(
            m.pending.pop(),
            Some(Action::JoinHidden {
                ssid: "hiddenlab".into(),
                security: SecurityType::Wpa3Sae,
                passphrase: Some("mockpass".into()),
            })
        );
        assert!(m.add_network.is_none() && !keyboard_visible(&m));
        assert_eq!(
            m.last_hidden,
            Some(("hiddenlab".to_owned(), SecurityType::Wpa3Sae))
        );
    }

    #[test]
    fn open_needs_no_passphrase_and_enter_moves_through_the_fields() {
        let mut m = ready();
        let l = Layout::new(SIZE);
        m.open_add_network();
        for ch in "lab".chars() {
            key_press(&mut m, Key::Char(ch));
        }
        key_press(&mut m, Key::Enter);
        assert_eq!(
            m.add_network.as_ref().unwrap().focus,
            AddField::Passphrase,
            "Enter on the name moves to the passphrase"
        );
        key_press(&mut m, Key::Enter);
        assert!(m.pending.is_empty());
        assert_eq!(
            m.last_error.as_deref(),
            Some("passphrase \u{b7} shorter than 8 characters")
        );
        m.add_network_security(0);
        assert_eq!(
            hit_test(&m, center(l.add_pass_rect()), SIZE),
            None,
            "no passphrase field for an open network"
        );
        key_press(&mut m, Key::Enter);
        assert_eq!(
            m.pending.pop(),
            Some(Action::JoinHidden {
                ssid: "lab".into(),
                security: SecurityType::Open,
                passphrase: None,
            })
        );
        // Names longer than 32 bytes stop growing.
        m.open_add_network();
        for _ in 0..40 {
            key_press(&mut m, Key::Char('x'));
        }
        assert_eq!(m.add_network.as_ref().unwrap().ssid.len(), SSID_MAX_LEN);
        // Back closes the form, then the page.
        m.back();
        assert!(m.add_network.is_none());
        assert_eq!(m.page, Page::Network);
    }

    #[test]
    fn a_failed_join_by_name_offers_retry_that_asks_for_the_passphrase_again() {
        let mut m = ready();
        let mut input = Input::default();
        let l = Layout::new(SIZE);
        m.open_add_network();
        let form = m.add_network.as_mut().unwrap();
        form.ssid = "ferrix-hidden".into();
        form.passphrase = "wrong-passphrase".into();
        m.submit_add_network().unwrap();
        m.pending.clear();
        m.apply_network(Feed::Event(NetworkEvent::ConnectionChanged {
            connection: ConnectionState::Failed {
                error: NetworkError::AuthenticationFailed,
            },
        }));
        assert_eq!(m.state_name(), "auth-failed");
        assert!(m.can_retry());
        assert_eq!(
            hit_test(&m, center(l.retry_rect()), SIZE),
            Some(Target::Retry)
        );
        tap(&mut input, &mut m, center(l.retry_rect()));
        let form = m.add_network.as_ref().expect("retry reopens the form");
        assert_eq!(form.ssid, "ferrix-hidden");
        assert_eq!(form.security, SecurityType::Wpa2Psk);
        assert!(form.passphrase.is_empty(), "the passphrase is asked again");
        assert_eq!(form.focus, AddField::Passphrase);
        // An unknown name reads plainly.
        m.cancel_add_network();
        m.apply_network(Feed::Event(NetworkEvent::ConnectionChanged {
            connection: ConnectionState::Failed {
                error: NetworkError::NotFound { id: "ssid".into() },
            },
        }));
        assert_eq!(m.connection_error().as_deref(), Some(NOT_FOUND_TEXT));
    }

    #[test]
    fn the_row_and_form_render() {
        let t = ferrix_ui::theme::get();
        let ty = Type::load().unwrap();
        let l = Layout::new(SIZE);
        let mut m = ready();
        assert_eq!(m.hidden_count(), 1, "the preview has one hidden beacon");
        let c = crate::screen::compose(&ty, SIZE, 600, &m, &Input::default());
        let row = l.add_row_rect(m.access_points().len());
        assert_eq!(c.get(row.x + 8, row.y + 8), t.surface, "Add network row");
        m.open_add_network();
        m.add_network.as_mut().unwrap().ssid = "ferrix-hidden".into();
        let c = crate::screen::compose(&ty, SIZE, 600, &m, &Input::default());
        let name = l.add_name_rect();
        assert_eq!(c.get(name.x + name.w / 2, name.y), t.rust, "name focused");
        let wpa2 = l.add_security_rects()[1];
        assert_eq!(c.get(wpa2.x + wpa2.w / 2, wpa2.y), t.rust, "WPA2 selected");
        let open = l.add_security_rects()[0];
        assert_ne!(c.get(open.x + open.w / 2, open.y), t.rust);
    }

    use crate::model::NOT_FOUND_TEXT;
}
