//! Contacts view model: list with search, detail, create/edit form, delete
//! confirmation, and export. Radio-independent: only `ferrix-contactsd` is
//! involved. Call and Message on the detail page open typed `dial` /
//! `compose` intents through `ferrix-appd`, which picks the handler (Phone,
//! Messages) and prefills the number; nothing is dialed or sent here.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use ferrix_app_api::AppIntent;
use ferrix_contacts_api::{
    Contact, ContactEvent, ContactId, ContactInput, NumberInput, NumberLabel, search,
};
use ferrix_contacts_protocol::Link as ProtoLink;
use ferrix_ui::osk::OskState;
use std::path::{Path, PathBuf};

/// File name of a vCard export inside the export directory.
pub const EXPORT_FILE: &str = "contacts.vcf";
/// Overrides the export directory (tests, nested runs).
pub const EXPORT_DIR_ENV: &str = "FERRIX_CONTACTS_EXPORT_DIR";

/// Where an export is written. The sandbox gives every app a private tmpfs
/// `/tmp` that vanishes with the process, so exports go to a durable
/// per-user directory on the shared filesystem instead:
/// `$FERRIX_CONTACTS_EXPORT_DIR`, else `$XDG_DOCUMENTS_DIR`, else
/// `$HOME/Documents`. Relative values are ignored; with no usable value
/// the export is refused rather than written somewhere it would be lost.
pub fn export_path(env: impl Fn(&str) -> Option<String>) -> Result<PathBuf, String> {
    let absolute = |key: &str| {
        env(key)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
    };
    let dir = absolute(EXPORT_DIR_ENV)
        .or_else(|| absolute("XDG_DOCUMENTS_DIR"))
        .or_else(|| absolute("HOME").map(|home| home.join("Documents")))
        .ok_or_else(|| "export \u{b7} no documents directory (HOME is unset)".to_owned())?;
    Ok(dir.join(EXPORT_FILE))
}

/// The export path as shown: `$HOME` abbreviated to `~`.
pub fn display_path(path: &Path, home: Option<&str>) -> String {
    if let Some(home) = home.filter(|h| !h.is_empty() && *h != "/")
        && let Ok(rest) = path.strip_prefix(home)
    {
        return format!("~/{}", rest.display());
    }
    path.display().to_string()
}

/// The detail page's name-parts line: only the parts present, and nothing
/// when they merely repeat the display name.
pub fn name_parts_line(contact: &Contact) -> Option<String> {
    let given = contact
        .given
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty());
    let family = contact
        .family
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty());
    let joined = [given, family]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ");
    if joined.is_empty() || joined == contact.display_name.trim() {
        return None;
    }
    let parts: Vec<String> = [("given", given), ("family", family)]
        .into_iter()
        .filter_map(|(key, value)| value.map(|v| format!("{key} {v}")))
        .collect();
    Some(parts.join(" \u{b7} "))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    List,
    Detail,
    Edit,
    ConfirmDelete,
}

impl Page {
    pub fn name(self) -> &'static str {
        match self {
            Page::List => "list",
            Page::Detail => "detail",
            Page::Edit => "edit",
            Page::ConfirmDelete => "confirm-delete",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "list" => Page::List,
            "detail" => Page::Detail,
            "edit" => Page::Edit,
            "confirm-delete" => Page::ConfirmDelete,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Link {
    #[default]
    Connecting,
    Connected,
    Reconnected,
    Unavailable,
}

impl Link {
    pub fn name(self) -> &'static str {
        match self {
            Link::Connecting => "connecting",
            Link::Connected => "connected",
            Link::Reconnected => "reconnected",
            Link::Unavailable => "unavailable",
        }
    }
}

/// Which form field has the keyboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Search,
    Name,
    Number(usize),
}

/// Maximum numbers editable in the form (the panel shows four rows).
pub const FORM_NUMBERS: usize = 4;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Form {
    pub editing: Option<ContactId>,
    pub name: String,
    pub numbers: Vec<(NumberLabel, String)>,
}

impl Form {
    fn from_contact(contact: &Contact) -> Self {
        Self {
            editing: Some(contact.id.clone()),
            name: contact.display_name.clone(),
            numbers: contact
                .numbers
                .iter()
                .map(|n| (n.label, n.raw.clone()))
                .collect(),
        }
    }

    pub fn input(&self) -> ContactInput {
        ContactInput {
            display_name: self.name.clone(),
            given: None,
            family: None,
            numbers: self
                .numbers
                .iter()
                .filter(|(_, raw)| !raw.trim().is_empty())
                .map(|(label, raw)| NumberInput {
                    label: *label,
                    raw: raw.clone(),
                })
                .collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Create(ContactInput),
    Update(ContactId, ContactInput),
    Delete(ContactId),
    Export,
    /// Open the handler of a dial/compose intent through `ferrix-appd`.
    Open(AppIntent),
    Back,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Model {
    pub page: Page,
    pub link: Link,
    pub contacts: Vec<Contact>,
    pub query: String,
    pub selected: Option<ContactId>,
    pub form: Form,
    /// The form as opened, so validation waits for an edit.
    pristine: Form,
    /// Latched once the form differed from `pristine`.
    form_edited: bool,
    pub focus: Option<Focus>,
    pub osk: OskState,
    pub pending: Vec<Action>,
    pub last_error: Option<String>,
    /// `exported · N bytes` after a successful export.
    pub export_status: Option<String>,
    /// Set when an event says the list is stale and must be re-fetched.
    pub stale: bool,
    lost_once: bool,
}

impl Default for Model {
    fn default() -> Self {
        Self::new()
    }
}

impl Model {
    pub fn new() -> Self {
        Self {
            page: Page::List,
            link: Link::Connecting,
            contacts: Vec::new(),
            query: String::new(),
            selected: None,
            form: Form::default(),
            pristine: Form::default(),
            form_edited: false,
            focus: None,
            osk: OskState::default(),
            pending: Vec::new(),
            last_error: None,
            export_status: None,
            stale: false,
            lost_once: false,
        }
    }

    pub fn link_label(&self) -> String {
        format!("contacts \u{b7} {}", self.link.name())
    }

    pub fn state_name(&self) -> String {
        match self.page {
            Page::List => format!("{}-shown", self.filtered().len()),
            Page::Detail => if self.selected().is_some() {
                "selected"
            } else {
                "none"
            }
            .to_owned(),
            Page::Edit => if self.form.editing.is_some() {
                "update"
            } else {
                "create"
            }
            .to_owned(),
            Page::ConfirmDelete => "confirm".to_owned(),
        }
    }

    // -- daemon --------------------------------------------------------------

    /// Folds a reconnector message. Returns whether a re-fetch is needed.
    pub fn apply(&mut self, link: ProtoLink) -> bool {
        match link {
            ProtoLink::Connected { contacts } => {
                self.link = if self.lost_once {
                    Link::Reconnected
                } else {
                    Link::Connected
                };
                self.lost_once = false;
                self.set_contacts(contacts);
                false
            }
            ProtoLink::Lost { .. } => {
                self.link = Link::Unavailable;
                self.lost_once = true;
                false
            }
            ProtoLink::Event(ContactEvent::ServiceRestarted) => {
                self.stale = true;
                true
            }
            ProtoLink::Event(ContactEvent::ContactChanged { .. }) => {
                self.stale = true;
                true
            }
        }
    }

    /// Replaces the list, keeping the selection valid.
    pub fn set_contacts(&mut self, contacts: Vec<Contact>) {
        self.contacts = contacts;
        self.stale = false;
        if let Some(id) = &self.selected
            && !self.contacts.iter().any(|c| c.id == *id)
        {
            self.selected = None;
            if matches!(self.page, Page::Detail | Page::ConfirmDelete) {
                self.page = Page::List;
            }
        }
    }

    // -- derived -------------------------------------------------------------

    pub fn filtered(&self) -> Vec<&Contact> {
        let mut out = search(&self.contacts, &self.query);
        out.sort_by(|a, b| {
            a.display_name
                .to_lowercase()
                .cmp(&b.display_name.to_lowercase())
        });
        out
    }

    pub fn selected(&self) -> Option<&Contact> {
        self.selected
            .as_ref()
            .and_then(|id| self.contacts.iter().find(|c| c.id == *id))
    }

    pub fn writable(&self) -> bool {
        self.link != Link::Unavailable
    }

    pub fn can_save(&self) -> Result<(), String> {
        if !self.writable() {
            return Err("contacts \u{b7} unavailable".to_owned());
        }
        self.form.input().validate().map_err(|e| e.to_string())
    }

    /// The hint under the form: unavailability always, validation only
    /// once the form was edited (a failed Save reports through
    /// `last_error`), never on a form nobody has typed into.
    pub fn form_hint(&self) -> Option<String> {
        if !self.writable() {
            return Some("contacts \u{b7} unavailable".to_owned());
        }
        if !self.form_edited && self.form == self.pristine {
            return None;
        }
        self.can_save().err()
    }

    /// Called after every input: remembers that the form was edited, so
    /// emptying a field again still shows why Save is disabled.
    pub fn note_form_edit(&mut self) {
        self.form_edited |= self.form != self.pristine;
    }

    // -- intents -------------------------------------------------------------

    pub fn open(&mut self, id: &ContactId) {
        self.selected = Some(id.clone());
        self.page = Page::Detail;
        self.focus = None;
        self.last_error = None;
    }

    pub fn new_contact(&mut self) {
        self.form = Form {
            editing: None,
            name: String::new(),
            numbers: vec![(NumberLabel::Mobile, String::new())],
        };
        self.pristine = self.form.clone();
        self.form_edited = false;
        self.page = Page::Edit;
        self.focus = Some(Focus::Name);
        self.last_error = None;
    }

    pub fn edit_selected(&mut self) {
        if let Some(contact) = self.selected() {
            self.form = Form::from_contact(contact);
            if self.form.numbers.is_empty() {
                self.form.numbers.push((NumberLabel::Mobile, String::new()));
            }
            self.pristine = self.form.clone();
            self.form_edited = false;
            self.page = Page::Edit;
            self.focus = Some(Focus::Name);
            self.last_error = None;
        }
    }

    pub fn add_number(&mut self) {
        if self.form.numbers.len() < FORM_NUMBERS {
            self.form.numbers.push((NumberLabel::Other, String::new()));
            self.focus = Some(Focus::Number(self.form.numbers.len() - 1));
        }
    }

    pub fn cycle_label(&mut self, index: usize) {
        if let Some((label, _)) = self.form.numbers.get_mut(index) {
            *label = label.next();
        }
    }

    pub fn save(&mut self) -> Result<(), String> {
        self.can_save()?;
        let input = self.form.input();
        match self.form.editing.clone() {
            Some(id) => {
                self.pending.push(Action::Update(id.clone(), input));
                self.selected = Some(id);
                self.page = Page::Detail;
            }
            None => {
                self.pending.push(Action::Create(input));
                self.page = Page::List;
            }
        }
        self.focus = None;
        Ok(())
    }

    /// The number Call and Message use: the selected contact's first one,
    /// normalized when the service could.
    pub fn primary_number(&self) -> Option<String> {
        self.selected()
            .and_then(|c| c.numbers.first())
            .map(|n| n.display().to_owned())
    }

    /// Call: open the dialer prefilled with the primary number. Never dials;
    /// the user presses Call in Phone.
    pub fn call_selected(&mut self) {
        self.open_with(|number| AppIntent::Dial { number });
    }

    /// Message: open a composer addressed to the primary number. Never
    /// sends.
    pub fn message_selected(&mut self) {
        self.open_with(|number| AppIntent::Compose { number });
    }

    fn open_with(&mut self, intent: impl FnOnce(String) -> AppIntent) {
        let Some(number) = self.primary_number() else {
            self.last_error = Some("number \u{b7} none".to_owned());
            return;
        };
        let intent = intent(number);
        match intent.validate() {
            Ok(()) => {
                self.last_error = None;
                self.pending.push(Action::Open(intent));
            }
            Err(_) => self.last_error = Some("number \u{b7} cannot be opened".to_owned()),
        }
    }

    pub fn ask_delete(&mut self) {
        if self.selected.is_some() {
            self.page = Page::ConfirmDelete;
        }
    }

    pub fn confirm_delete(&mut self) {
        if let Some(id) = self.selected.take() {
            self.pending.push(Action::Delete(id));
        }
        self.page = Page::List;
    }

    pub fn export(&mut self) {
        if self.writable() {
            self.pending.push(Action::Export);
        } else {
            self.last_error = Some("contacts \u{b7} unavailable".to_owned());
        }
    }

    /// The text buffer under the keyboard, if any.
    pub fn focused_text(&mut self) -> Option<&mut String> {
        match self.focus? {
            Focus::Search => Some(&mut self.query),
            Focus::Name => Some(&mut self.form.name),
            Focus::Number(i) => self.form.numbers.get_mut(i).map(|(_, raw)| raw),
        }
    }

    pub fn back(&mut self) {
        match self.page {
            Page::List => {
                if self.focus == Some(Focus::Search) {
                    self.focus = None;
                } else {
                    self.pending.push(Action::Back);
                }
            }
            Page::Detail => {
                self.page = Page::List;
                self.focus = None;
            }
            Page::Edit => {
                self.page = if self.form.editing.is_some() {
                    Page::Detail
                } else {
                    Page::List
                };
                self.focus = None;
            }
            Page::ConfirmDelete => self.page = Page::Detail,
        }
    }
}

/// Deterministic synthetic contacts for the host preview and tests.
pub fn preview_contacts() -> Vec<Contact> {
    (1..=8u8)
        .map(|n| Contact {
            id: ContactId::from_bytes([0, 0, 0, 0, 0, 0, 0, n]),
            display_name: format!("Test Contact {n:02}"),
            given: Some("Test".to_owned()),
            family: Some(format!("Contact {n:02}")),
            numbers: vec![ferrix_contacts_api::PhoneNumber::new(
                NumberLabel::ALL[(n as usize - 1) % 4],
                format!("+1 555 010 {n:04}"),
                "US",
            )],
            created_ms: 1_700_000_000_000 + u64::from(n),
            updated_ms: 1_700_000_000_000 + u64::from(n),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrix_contacts_api::ChangeKind;

    fn connected() -> Model {
        let mut m = Model::new();
        m.apply(ProtoLink::Connected {
            contacts: preview_contacts(),
        });
        m
    }

    #[test]
    fn search_filters_and_selection_survives_refresh() {
        let mut m = connected();
        assert_eq!(m.filtered().len(), 8);
        m.query = "03".to_owned();
        assert_eq!(m.filtered().len(), 1);
        m.query = "0100004".to_owned();
        assert_eq!(m.filtered()[0].display_name, "Test Contact 04");
        m.query.clear();
        let id = m.filtered()[2].id.clone();
        m.open(&id);
        assert_eq!(
            (m.page, m.state_name().as_str()),
            (Page::Detail, "selected")
        );
        assert!(m.apply(ProtoLink::Event(ContactEvent::ContactChanged {
            id: id.clone(),
            kind: ChangeKind::Deleted
        })));
        assert!(m.stale);
        let mut fewer = preview_contacts();
        fewer.retain(|c| c.id != id);
        m.set_contacts(fewer);
        assert_eq!(m.page, Page::List, "a vanished selection closes the detail");
        assert!(!m.stale);
    }

    #[test]
    fn create_edit_delete_and_export_queue_actions() {
        let mut m = connected();
        m.new_contact();
        assert_eq!((m.page, m.focus), (Page::Edit, Some(Focus::Name)));
        assert!(m.save().is_err(), "empty name refused");
        m.form.name = "Test Contact 99".to_owned();
        m.form.numbers[0].1 = "(555) 010-0099".to_owned();
        m.cycle_label(0);
        assert_eq!(m.form.numbers[0].0, NumberLabel::Home);
        m.add_number();
        assert_eq!(m.focus, Some(Focus::Number(1)));
        assert_eq!(m.save(), Ok(()));
        assert!(
            matches!(&m.pending[0], Action::Create(input) if input.numbers.len() == 1 && input.display_name == "Test Contact 99")
        );
        assert_eq!(m.page, Page::List);
        m.pending.clear();

        let id = m.contacts[0].id.clone();
        m.open(&id);
        m.edit_selected();
        assert_eq!(m.form.editing, Some(id.clone()));
        m.form.name.push_str(" edited");
        assert_eq!(m.save(), Ok(()));
        assert!(matches!(&m.pending[0], Action::Update(uid, _) if *uid == id));
        assert_eq!(m.page, Page::Detail);
        m.pending.clear();
        m.ask_delete();
        assert_eq!(m.page, Page::ConfirmDelete);
        m.back();
        assert_eq!(m.page, Page::Detail);
        m.ask_delete();
        m.confirm_delete();
        assert_eq!(m.pending, vec![Action::Delete(id)]);
        assert_eq!(m.page, Page::List);
        m.pending.clear();
        m.export();
        assert_eq!(m.pending, vec![Action::Export]);
        m.pending.clear();
        m.apply(ProtoLink::Lost {
            reason: "gone".into(),
        });
        assert_eq!(m.link, Link::Unavailable);
        m.export();
        assert!(m.pending.is_empty());
        assert_eq!(m.last_error.as_deref(), Some("contacts \u{b7} unavailable"));
        m.new_contact();
        m.form.name = "x".to_owned();
        assert_eq!(m.save(), Err("contacts \u{b7} unavailable".to_owned()));
        m.apply(ProtoLink::Connected {
            contacts: preview_contacts(),
        });
        assert_eq!(m.link, Link::Reconnected);
    }

    #[test]
    fn back_closes_search_then_yields() {
        let mut m = connected();
        m.focus = Some(Focus::Search);
        m.back();
        assert_eq!(m.focus, None);
        m.back();
        assert_eq!(m.pending, vec![Action::Back]);
        assert_eq!(Page::parse("confirm-delete"), Some(Page::ConfirmDelete));
    }

    #[test]
    fn new_form_validates_only_after_an_edit() {
        let mut m = connected();
        m.new_contact();
        assert_eq!(m.form_hint(), None, "nothing typed yet");
        assert!(m.can_save().is_err(), "Save stays disabled");
        m.form.name.push('T');
        m.note_form_edit();
        assert_eq!(m.form_hint(), None, "a valid edit has no hint");
        m.form.name.clear();
        m.note_form_edit();
        assert!(m.form_hint().is_some(), "an edit that empties the name");
        m.new_contact();
        assert_eq!(m.form_hint(), None, "a fresh form is pristine again");
        let id = m.contacts[0].id.clone();
        m.open(&id);
        m.edit_selected();
        assert_eq!(m.form_hint(), None);
        m.apply(ProtoLink::Lost {
            reason: "gone".into(),
        });
        assert_eq!(
            m.form_hint().as_deref(),
            Some("contacts \u{b7} unavailable")
        );
    }

    #[test]
    fn export_goes_to_a_durable_documents_directory() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |key: &str| {
                pairs
                    .iter()
                    .find(|(k, _)| *k == key)
                    .map(|(_, v)| (*v).to_owned())
            }
        };
        assert_eq!(
            export_path(env(&[("HOME", "/home/user")])),
            Ok(PathBuf::from("/home/user/Documents/contacts.vcf"))
        );
        assert_eq!(
            export_path(env(&[
                ("HOME", "/home/user"),
                ("XDG_DOCUMENTS_DIR", "/home/user/Docs")
            ])),
            Ok(PathBuf::from("/home/user/Docs/contacts.vcf"))
        );
        assert_eq!(
            export_path(env(&[
                ("HOME", "/home/user"),
                (EXPORT_DIR_ENV, "/srv/exports")
            ])),
            Ok(PathBuf::from("/srv/exports/contacts.vcf"))
        );
        assert_eq!(
            export_path(env(&[
                ("HOME", "/home/user"),
                ("XDG_DOCUMENTS_DIR", "docs")
            ])),
            Ok(PathBuf::from("/home/user/Documents/contacts.vcf")),
            "relative values are ignored"
        );
        let none = export_path(env(&[]));
        assert!(none.is_err(), "never falls back to the private /tmp");
        assert_eq!(
            export_path(env(&[("HOME", "/root")])),
            Ok(PathBuf::from("/root/Documents/contacts.vcf"))
        );
        assert_eq!(
            display_path(
                Path::new("/home/user/Documents/contacts.vcf"),
                Some("/home/user")
            ),
            "~/Documents/contacts.vcf"
        );
        assert_eq!(
            display_path(Path::new("/srv/exports/contacts.vcf"), Some("/home/user")),
            "/srv/exports/contacts.vcf"
        );
    }

    #[test]
    fn name_parts_hide_empty_and_redundant_values() {
        let mut c = preview_contacts().remove(0);
        assert_eq!(name_parts_line(&c), None, "repeats the display name");
        c.display_name = "Testy".to_owned();
        assert_eq!(
            name_parts_line(&c).as_deref(),
            Some("given Test \u{b7} family Contact 01")
        );
        c.family = None;
        assert_eq!(name_parts_line(&c).as_deref(), Some("given Test"));
        c.given = Some("  ".to_owned());
        assert_eq!(name_parts_line(&c), None);
    }
}
