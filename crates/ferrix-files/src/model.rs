//! Files view model: browse a folder under home, open a file read-only,
//! inspect an entry, create a folder, rename, and delete with a
//! confirmation. All filesystem work goes through [`Home`], so the model
//! cannot reach a path outside `$HOME`.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use ferrix_ui::osk::{Layout as OskLayout, OskState};
use jiff::Timestamp;
use jiff::tz::TimeZone;

use crate::scroll::Scroll;
use crate::store::{Entry, FsError, Home, Kind, Listing, TextFile, human_size};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Browse,
    View,
    Item,
    NewFolder,
    Rename,
    ConfirmDelete,
}

impl Page {
    pub fn name(self) -> &'static str {
        match self {
            Page::Browse => "browse",
            Page::View => "view",
            Page::Item => "item",
            Page::NewFolder => "new-folder",
            Page::Rename => "rename",
            Page::ConfirmDelete => "confirm-delete",
        }
    }

    /// Pages whose single field takes the touch keyboard.
    pub fn has_form(self) -> bool {
        matches!(self, Page::NewFolder | Page::Rename)
    }
}

/// Requests the app shell carries out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Back from the home folder: yield the foreground to the home surface.
    Back,
}

/// The one local status line: the result of the last change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Notice {
    Ok(String),
    Err(String),
}

/// Outcome of a filesystem change, for the evidence record (codes only).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub action: &'static str,
    pub error: Option<&'static str>,
}

pub struct Model {
    pub home: Result<Home, FsError>,
    /// Folder components below home.
    pub cwd: Vec<String>,
    pub listing: Result<Listing, FsError>,
    pub page: Page,
    /// Entry name the item, viewer, rename, and delete pages act on.
    pub selected: Option<String>,
    pub file: Option<Result<TextFile, FsError>>,
    pub form: String,
    /// Whether the form changed since it opened (validation shows after).
    pub form_edited: bool,
    pub osk: OskState,
    pub scroll: Scroll,
    pub view_scroll: Scroll,
    pub notice: Option<Notice>,
    /// Items inside the folder awaiting delete confirmation.
    pub delete_items: Option<usize>,
    pub pending: Vec<Action>,
    pub outcomes: Vec<Outcome>,
    pub zone: TimeZone,
}

impl Model {
    pub fn new(home: Result<Home, FsError>, zone: TimeZone) -> Self {
        let listing = Err(FsError::NotFound);
        let mut model = Self {
            home,
            cwd: Vec::new(),
            listing,
            page: Page::Browse,
            selected: None,
            file: None,
            form: String::new(),
            form_edited: false,
            osk: OskState::new(OskLayout::Text),
            scroll: Scroll::default(),
            view_scroll: Scroll::default(),
            notice: None,
            delete_items: None,
            pending: Vec::new(),
            outcomes: Vec::new(),
            zone,
        };
        model.refresh();
        model
    }

    /// Re-reads the current folder. Returns whether the listing changed.
    pub fn refresh(&mut self) -> bool {
        let listing = match &self.home {
            Ok(home) => home.list(&self.cwd),
            Err(error) => Err(error.clone()),
        };
        let changed = listing != self.listing;
        self.listing = listing;
        changed
    }

    pub fn entries(&self) -> &[Entry] {
        self.listing.as_ref().map_or(&[], |l| l.entries.as_slice())
    }

    pub fn entry(&self, name: &str) -> Option<&Entry> {
        self.entries().iter().find(|e| e.name == name)
    }

    pub fn selected_entry(&self) -> Option<&Entry> {
        self.selected.as_deref().and_then(|n| self.entry(n))
    }

    pub fn writable(&self) -> bool {
        self.home.is_ok() && self.listing.is_ok()
    }

    /// `~`, `~/Documents`, ... for the folder (and optional entry).
    pub fn display_path(&self, name: Option<&str>) -> String {
        let mut text = "~".to_owned();
        for part in self.cwd.iter().map(String::as_str).chain(name) {
            text.push('/');
            text.push_str(part);
        }
        text
    }

    /// Breadcrumb labels from home to the current folder.
    pub fn crumbs(&self) -> Vec<String> {
        std::iter::once("~".to_owned())
            .chain(self.cwd.iter().cloned())
            .collect()
    }

    fn record(&mut self, action: &'static str, result: &Result<(), FsError>) {
        self.outcomes.push(Outcome {
            action,
            error: result.as_ref().err().map(FsError::code),
        });
    }

    fn fail(&mut self, error: &FsError) {
        self.notice = Some(Notice::Err(error.to_string()));
    }

    fn go_to(&mut self, cwd: Vec<String>) {
        let before = std::mem::replace(&mut self.cwd, cwd);
        if let Err(error) = self
            .home
            .as_ref()
            .map_err(Clone::clone)
            .and_then(|h| h.folder(&self.cwd))
        {
            // Stay where we were and say why.
            self.cwd = before;
            self.fail(&error);
            return;
        }
        self.refresh();
        self.scroll.reset();
        self.page = Page::Browse;
        self.selected = None;
        self.notice = None;
    }

    /// Opens the entry at `index` of the listing: folders are entered,
    /// files open in the viewer, anything else says why it cannot open.
    pub fn open_index(&mut self, index: usize) {
        if let Some(name) = self.entries().get(index).map(|e| e.name.clone()) {
            self.open(&name);
        }
    }

    pub fn open(&mut self, name: &str) {
        let Some(entry) = self.entry(name).cloned() else {
            self.fail(&FsError::NotFound);
            self.refresh();
            return;
        };
        match entry.kind {
            Kind::Folder => {
                let mut cwd = self.cwd.clone();
                cwd.push(entry.name);
                self.go_to(cwd);
            }
            Kind::File => self.view(&entry.name),
            Kind::Link => self.fail(&FsError::OutsideHome),
            Kind::Other => self.fail(&FsError::NotAFile),
        }
    }

    /// Navigates to the breadcrumb at `depth` (0 is home).
    pub fn crumb(&mut self, depth: usize) {
        if depth < self.cwd.len() {
            let cwd = self.cwd[..depth].to_vec();
            self.go_to(cwd);
        }
    }

    pub fn view(&mut self, name: &str) {
        let Ok(home) = &self.home else {
            return;
        };
        let file = home.read_text(&self.cwd, name);
        if let Err(error) = &file {
            self.fail(error);
            return;
        }
        self.file = Some(file);
        self.selected = Some(name.to_owned());
        self.view_scroll.reset();
        self.notice = None;
        self.page = Page::View;
    }

    /// The details page for the entry at `index`.
    pub fn inspect(&mut self, index: usize) {
        if let Some(name) = self.entries().get(index).map(|e| e.name.clone()) {
            self.selected = Some(name);
            self.notice = None;
            self.page = Page::Item;
        }
    }

    pub fn new_folder(&mut self) {
        if !self.writable() {
            return;
        }
        self.open_form(Page::NewFolder, String::new());
    }

    pub fn rename_selected(&mut self) {
        if let Some(name) = self.selected_entry().map(|e| e.name.clone()) {
            self.open_form(Page::Rename, name);
        }
    }

    fn open_form(&mut self, page: Page, text: String) {
        self.form = text;
        self.form_edited = false;
        self.osk = OskState::new(OskLayout::Text);
        self.notice = None;
        self.page = page;
    }

    pub fn ask_delete(&mut self) {
        let Some(name) = self.selected_entry().map(|e| e.name.clone()) else {
            return;
        };
        self.delete_items = self
            .home
            .as_ref()
            .ok()
            .and_then(|h| h.folder_items(&self.cwd, &name));
        self.notice = None;
        self.page = Page::ConfirmDelete;
    }

    pub fn confirm_delete(&mut self) {
        let (Ok(home), Some(name)) = (&self.home, self.selected.clone()) else {
            return;
        };
        let result = home.delete(&self.cwd, &name);
        self.record("delete", &result);
        self.refresh();
        match result {
            Ok(()) => {
                self.notice = Some(Notice::Ok(format!("{name} \u{b7} deleted")));
                self.selected = None;
                self.page = Page::Browse;
            }
            Err(error) => {
                self.fail(&error);
                self.page = if self.selected_entry().is_some() {
                    Page::Item
                } else {
                    Page::Browse
                };
            }
        }
    }

    /// Why the form cannot be submitted yet, once it has been edited.
    pub fn form_hint(&self) -> Option<String> {
        if !self.form_edited {
            return None;
        }
        crate::store::validate_new_name(&self.form)
            .err()
            .map(|e| e.to_string())
    }

    pub fn can_submit(&self) -> bool {
        crate::store::validate_new_name(&self.form).is_ok()
    }

    /// Creates the folder or applies the rename the form describes.
    pub fn submit(&mut self) {
        let Ok(home) = &self.home else {
            return;
        };
        let name = self.form.clone();
        match self.page {
            Page::NewFolder => {
                let result = home.create_folder(&self.cwd, &name);
                self.record("create-folder", &result);
                match result {
                    Ok(()) => {
                        self.refresh();
                        self.page = Page::Browse;
                        self.notice = Some(Notice::Ok(format!("{name} \u{b7} created")));
                        self.reveal(&name);
                    }
                    Err(error) => self.fail(&error),
                }
            }
            Page::Rename => {
                let Some(from) = self.selected.clone() else {
                    return;
                };
                let result = home.rename(&self.cwd, &from, &name);
                self.record("rename", &result);
                match result {
                    Ok(()) => {
                        self.refresh();
                        self.selected = Some(name.clone());
                        self.page = Page::Item;
                        self.notice = Some(Notice::Ok(format!("{from} \u{b7} renamed")));
                        self.reveal(&name);
                    }
                    Err(error) => self.fail(&error),
                }
            }
            _ => {}
        }
    }

    /// Puts `name` at the top of the list window (clamped when drawn).
    fn reveal(&mut self, name: &str) {
        if let Some(index) = self.entries().iter().position(|e| e.name == name) {
            let count = self.entries().len();
            self.scroll.reveal(index, count, 1);
        }
    }

    pub fn back(&mut self) {
        match self.page {
            Page::Browse => {
                if self.cwd.is_empty() {
                    self.pending.push(Action::Back);
                } else {
                    let mut cwd = self.cwd.clone();
                    cwd.pop();
                    self.go_to(cwd);
                }
            }
            Page::View | Page::Item | Page::NewFolder => {
                self.page = Page::Browse;
                self.file = None;
                self.notice = None;
                if self.selected_entry().is_none() {
                    self.selected = None;
                }
            }
            Page::Rename | Page::ConfirmDelete => {
                self.notice = None;
                self.page = if self.selected_entry().is_some() {
                    Page::Item
                } else {
                    Page::Browse
                };
            }
        }
    }

    /// Visible entry count and hidden/truncated notes for the status strip.
    pub fn count_label(&self) -> String {
        match &self.listing {
            Ok(l) => {
                let n = l.entries.len() + l.truncated;
                let mut text = if n == 1 {
                    "1 item".to_owned()
                } else {
                    format!("{n} items")
                };
                if l.hidden > 0 {
                    text.push_str(&format!(" \u{b7} {} hidden", l.hidden));
                }
                text
            }
            Err(_) => "files".to_owned(),
        }
    }

    /// `YYYY-MM-DD HH:MM` in the session zone.
    pub fn date(&self, time: Option<Timestamp>) -> String {
        time.map_or_else(
            || "unknown date".to_owned(),
            |t| {
                t.to_zoned(self.zone.clone())
                    .strftime("%Y-%m-%d %H:%M")
                    .to_string()
            },
        )
    }

    /// A row subtitle: kind or size, then the modification date.
    pub fn subtitle(&self, entry: &Entry) -> String {
        match entry.kind {
            Kind::Folder => format!(
                "{} \u{b7} {}",
                Self::items_label(entry),
                self.date(entry.modified)
            ),
            Kind::File => format!(
                "{} \u{b7} {}",
                human_size(entry.size),
                self.date(entry.modified)
            ),
            Kind::Link => "link \u{b7} outside home".to_owned(),
            Kind::Other => "special file \u{b7} not opened".to_owned(),
        }
    }

    /// `3 items` for a folder (`999+ items` past the count limit).
    pub fn items_label(entry: &Entry) -> String {
        match entry.children {
            Some(n) if n > crate::store::CHILD_COUNT_LIMIT => {
                format!("{}+ items", crate::store::CHILD_COUNT_LIMIT)
            }
            Some(1) => "1 item".to_owned(),
            Some(n) => format!("{n} items"),
            None => "folder".to_owned(),
        }
    }

    pub fn kind_name(entry: &Entry) -> &'static str {
        match (entry.kind, entry.link) {
            (Kind::Folder, false) => "folder",
            (Kind::Folder, true) => "folder \u{b7} link",
            (Kind::File, false) => "file",
            (Kind::File, true) => "file \u{b7} link",
            (Kind::Link, _) => "link \u{b7} outside home",
            (Kind::Other, _) => "special file",
        }
    }

    /// `ui-state` value: depth and counts only, never names.
    pub fn state_name(&self) -> String {
        let listing = match &self.listing {
            Ok(l) => format!("{} items", l.entries.len()),
            Err(e) => e.code().to_owned(),
        };
        format!("depth {} \u{b7} {listing}", self.cwd.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::testdir::TestDir;
    use std::fs;

    fn model() -> (TestDir, Model) {
        let dir = TestDir::new();
        let home = dir.path().join("home");
        fs::create_dir_all(home.join("Documents")).unwrap();
        fs::create_dir_all(home.join("Downloads")).unwrap();
        fs::write(
            home.join("Documents/contacts.vcf"),
            "BEGIN:VCARD\nEND:VCARD\n",
        )
        .unwrap();
        fs::write(home.join("notes.txt"), "one\ntwo\n").unwrap();
        let m = Model::new(Home::new(&home), TimeZone::UTC);
        (dir, m)
    }

    fn names(m: &Model) -> Vec<&str> {
        m.entries().iter().map(|e| e.name.as_str()).collect()
    }

    #[test]
    fn navigation_enters_folders_and_back_climbs_to_home_then_yields() {
        let (_d, mut m) = model();
        assert_eq!(names(&m), ["Documents", "Downloads", "notes.txt"]);
        assert_eq!(m.display_path(None), "~");
        m.open_index(0);
        assert_eq!(m.cwd, ["Documents"]);
        assert_eq!(m.display_path(None), "~/Documents");
        assert_eq!(m.crumbs(), ["~", "Documents"]);
        assert_eq!(names(&m), ["contacts.vcf"]);
        m.open_index(0);
        assert_eq!(m.page, Page::View);
        let text = m.file.as_ref().unwrap().as_ref().unwrap().text.clone();
        assert_eq!(text.as_deref(), Some("BEGIN:VCARD\nEND:VCARD\n"));
        m.back();
        assert_eq!((m.page, m.cwd.len()), (Page::Browse, 1));
        m.back();
        assert!(m.cwd.is_empty());
        assert!(m.pending.is_empty());
        m.back();
        assert_eq!(m.pending, vec![Action::Back]);
        m.open_index(0);
        m.crumb(0);
        assert!(m.cwd.is_empty());
        m.crumb(5);
        assert!(m.cwd.is_empty(), "out-of-range crumbs do nothing");
    }

    #[test]
    fn create_rename_delete_flows_and_their_notices() {
        let (_d, mut m) = model();
        m.new_folder();
        assert_eq!(m.page, Page::NewFolder);
        assert_eq!(m.form_hint(), None, "no validation before an edit");
        m.form_edited = true;
        assert_eq!(m.form_hint().as_deref(), Some("name is empty"));
        assert!(!m.can_submit());
        m.form = "Notes".to_owned();
        m.submit();
        assert_eq!(m.page, Page::Browse);
        assert_eq!(
            m.notice,
            Some(Notice::Ok("Notes \u{b7} created".to_owned()))
        );
        assert_eq!(names(&m), ["Documents", "Downloads", "Notes", "notes.txt"]);
        // A duplicate stays on the form with the reason.
        m.new_folder();
        m.form = "Notes".to_owned();
        m.submit();
        assert_eq!(m.page, Page::NewFolder);
        assert_eq!(
            m.notice,
            Some(Notice::Err("name already exists".to_owned()))
        );
        m.back();

        m.inspect(2);
        assert_eq!(m.page, Page::Item);
        m.rename_selected();
        assert_eq!((m.page, m.form.as_str()), (Page::Rename, "Notes"));
        m.form = "Journal".to_owned();
        m.submit();
        assert_eq!(m.page, Page::Item);
        assert_eq!(m.selected.as_deref(), Some("Journal"));
        assert!(m.entry("Journal").is_some_and(Entry::is_folder));
        m.rename_selected();
        m.form = "notes.txt".to_owned();
        m.submit();
        assert_eq!(
            m.notice,
            Some(Notice::Err("name already exists".to_owned()))
        );
        m.back();
        assert_eq!(m.page, Page::Item);

        m.ask_delete();
        assert_eq!((m.page, m.delete_items), (Page::ConfirmDelete, Some(0)));
        m.back();
        assert_eq!(m.page, Page::Item, "cancel keeps the entry");
        m.ask_delete();
        m.confirm_delete();
        assert_eq!(m.page, Page::Browse);
        assert_eq!(
            m.notice,
            Some(Notice::Ok("Journal \u{b7} deleted".to_owned()))
        );
        assert_eq!(names(&m), ["Documents", "Downloads", "notes.txt"]);
        let actions: Vec<_> = m.outcomes.iter().map(|o| (o.action, o.error)).collect();
        assert_eq!(
            actions,
            [
                ("create-folder", None),
                ("create-folder", Some("already-exists")),
                ("rename", None),
                ("rename", Some("already-exists")),
                ("delete", None)
            ]
        );
    }

    #[test]
    fn external_changes_and_errors_are_reported_not_hidden() {
        let (_d, mut m) = model();
        let root = m.home.as_ref().unwrap().root().to_path_buf();
        fs::write(root.join("Documents/later.txt"), "x").unwrap();
        m.open_index(0);
        assert_eq!(names(&m), ["contacts.vcf", "later.txt"]);
        // The folder vanishes under us: delete reports not found.
        m.inspect(1);
        fs::remove_file(root.join("Documents/later.txt")).unwrap();
        m.ask_delete();
        m.confirm_delete();
        assert_eq!(
            m.notice,
            Some(Notice::Err("not found \u{b7} refresh".to_owned()))
        );
        assert_eq!(m.page, Page::Browse);
        fs::remove_dir_all(root.join("Documents")).unwrap();
        assert!(m.refresh());
        assert_eq!(m.listing, Err(FsError::NotFound));
        assert!(!m.writable());
        m.new_folder();
        assert_eq!(m.page, Page::Browse, "no create in a missing folder");
        assert_eq!(m.state_name(), "depth 1 \u{b7} not-found");
        m.back();
        assert!(m.cwd.is_empty());
        assert!(m.listing.is_ok());

        let none = Model::new(Home::from_env(|_| None), TimeZone::UTC);
        assert!(none.entries().is_empty());
        assert!(matches!(none.listing, Err(FsError::HomeUnavailable(_))));
        assert_eq!(none.count_label(), "files");
    }

    #[test]
    fn rows_describe_size_count_and_date() {
        let (_d, m) = model();
        let docs = &m.entries()[0];
        let t = Timestamp::from_second(1_760_000_000).unwrap();
        assert_eq!(m.date(Some(t)), "2025-10-09 08:53");
        assert!(m.subtitle(docs).starts_with("1 item \u{b7} 20"));
        let notes = m.entry("notes.txt").unwrap();
        assert!(m.subtitle(notes).starts_with("8 B \u{b7} "));
        assert_eq!(m.count_label(), "3 items");
        assert_eq!(m.state_name(), "depth 0 \u{b7} 3 items");
        assert_eq!(Page::ConfirmDelete.name(), "confirm-delete");
    }
}
