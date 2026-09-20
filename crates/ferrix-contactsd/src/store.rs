//! The contacts store (ADR-003): one versioned JSON document written
//! atomically, explicit migrations, corrupt-file quarantine, and a refusal
//! to touch data written by a newer schema.
//!
//! ```text
//! <dir>/contacts.json            the document {"schema_version": N, "contacts": [...]}
//! <dir>/contacts.json.tmp        in-flight write; renamed over the document
//! <dir>/contacts.json.corrupt-*  forensic copies of unparsable documents
//! ```

use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};

use ferrix_contacts_api::{Contact, ContactsError, SCHEMA_VERSION};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const FILE_NAME: &str = "contacts.json";
pub const DEFAULT_DIR: &str = "/var/lib/ferrix/contacts";
pub const DIR_ENV: &str = "FERRIX_CONTACTS_DIR";

/// The on-disk document at the current schema.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Document {
    pub schema_version: u32,
    #[serde(default)]
    pub contacts: Vec<Contact>,
}

/// What loading did besides reading contacts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LoadReport {
    /// Migration steps applied, as `(from, to)` pairs.
    pub migrations: Vec<(u32, u32)>,
    /// The document did not exist yet.
    pub fresh: bool,
}

pub struct Store {
    dir: PathBuf,
    contacts: Vec<Contact>,
}

fn store_err(reason: impl ToString) -> ContactsError {
    ContactsError::Store {
        reason: reason.to_string(),
    }
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl Store {
    pub fn path_in(dir: &Path) -> PathBuf {
        dir.join(FILE_NAME)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn path(&self) -> PathBuf {
        Self::path_in(&self.dir)
    }

    pub fn contacts(&self) -> &[Contact] {
        &self.contacts
    }

    pub fn contacts_mut(&mut self) -> &mut Vec<Contact> {
        &mut self.contacts
    }

    /// Opens (creating `dir`) and loads the document, migrating older
    /// schemas in memory. Corrupt input is copied aside for diagnosis and
    /// startup fails closed; the authoritative file is never removed or
    /// replaced. A document newer than [`SCHEMA_VERSION`] is a typed error
    /// and nothing is written.
    pub fn open(dir: &Path) -> Result<(Self, LoadReport), ContactsError> {
        fs::create_dir_all(dir).map_err(|e| store_err(format!("{}: {e}", dir.display())))?;
        let path = Self::path_in(dir);
        // A leftover `.tmp` is an interrupted write; the document is authoritative.
        let _ = fs::remove_file(tmp_path(&path));
        let mut report = LoadReport::default();
        let mut store = Self {
            dir: dir.to_path_buf(),
            contacts: Vec::new(),
        };
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                report.fresh = true;
                return Ok((store, report));
            }
            Err(e) => return Err(store_err(format!("{}: {e}", path.display()))),
        };
        match parse_document(&text) {
            Ok((document, migrations)) => {
                store.contacts = document.contacts;
                report.migrations = migrations;
                if !report.migrations.is_empty() {
                    store.save()?;
                }
            }
            Err(ContactsError::SchemaTooNew { found, supported }) => {
                return Err(ContactsError::SchemaTooNew { found, supported });
            }
            Err(error) => {
                let quarantine = dir.join(format!("{FILE_NAME}.corrupt-{}", unix_ms()));
                fs::copy(&path, &quarantine).map_err(|e| {
                    store_err(format!("quarantine copy of {}: {e}", path.display()))
                })?;
                if let Ok(file) = File::open(&quarantine) {
                    let _ = file.sync_all();
                }
                return Err(store_err(format!(
                    "{} is corrupt ({error}); preserved in place and copied to {}",
                    path.display(),
                    quarantine.display()
                )));
            }
        }
        Ok((store, report))
    }

    /// Serializes and writes atomically: tmp, fsync, rename, directory fsync.
    pub fn save(&self) -> Result<(), ContactsError> {
        let document = Document {
            schema_version: SCHEMA_VERSION,
            contacts: self.contacts.clone(),
        };
        let mut bytes = serde_json::to_vec_pretty(&document).map_err(store_err)?;
        bytes.push(b'\n');
        write_atomic(&self.path(), &bytes)
    }
}

fn tmp_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(".tmp");
    path.with_file_name(name)
}

/// Writes `bytes` to `path` so that a crash at any point leaves either the
/// old or the new content, never a mix.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), ContactsError> {
    let tmp = tmp_path(path);
    let dir = path.parent().ok_or_else(|| store_err("no parent dir"))?;
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&tmp)
        .map_err(|e| store_err(format!("{}: {e}", tmp.display())))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|e| store_err(format!("{}: {e}", tmp.display())))?;
    drop(file);
    fs::rename(&tmp, path).map_err(|e| store_err(format!("rename {}: {e}", tmp.display())))?;
    // Directory fsync makes the rename durable; not every filesystem needs
    // it, and none forbids it.
    if let Ok(dir) = File::open(dir) {
        let _ = dir.sync_all();
    }
    Ok(())
}

/// Parses any known document version and migrates it to the current one,
/// returning the steps taken.
pub fn parse_document(text: &str) -> Result<(Document, Vec<(u32, u32)>), ContactsError> {
    let value: Value = serde_json::from_str(text).map_err(|e| store_err(format!("parse: {e}")))?;
    if !value.is_object() {
        return Err(store_err("document is not a JSON object"));
    }
    crate::migrate::migrate(value)
}

#[cfg(test)]
pub(crate) fn temp_dir(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "ferrix-contactsd-{tag}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrix_contacts_api::{ContactId, NumberLabel, PhoneNumber};

    fn contact(n: u8) -> Contact {
        Contact {
            id: ContactId::from_bytes([0, 0, 0, 0, 0, 0, 0, n]),
            display_name: format!("Test Contact {n:02}"),
            given: None,
            family: None,
            numbers: vec![PhoneNumber::new(
                NumberLabel::Mobile,
                format!("+1555010{n:04}"),
                "US",
            )],
            created_ms: 1,
            updated_ms: 1,
        }
    }

    #[test]
    fn fresh_store_saves_and_reloads_identically() {
        let dir = temp_dir("fresh");
        let (mut store, report) = Store::open(&dir).unwrap();
        assert!(report.fresh);
        assert!(store.contacts().is_empty());
        store.contacts_mut().push(contact(1));
        store.contacts_mut().push(contact(2));
        store.save().unwrap();
        assert!(!tmp_path(&store.path()).exists(), "tmp renamed away");
        let (reloaded, report) = Store::open(&dir).unwrap();
        assert!(!report.fresh && report.migrations.is_empty());
        assert_eq!(reloaded.contacts(), store.contacts());
        let text = fs::read_to_string(store.path()).unwrap();
        assert!(text.starts_with("{\n  \"schema_version\": 1"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn interrupted_write_keeps_the_previous_document() {
        let dir = temp_dir("crash");
        let (mut store, _) = Store::open(&dir).unwrap();
        store.contacts_mut().push(contact(1));
        store.save().unwrap();
        // Simulated crash: a new document reached the tmp file but the rename
        // never happened.
        let tmp = tmp_path(&store.path());
        fs::write(&tmp, "{\"schema_version\":1,\"contacts\":[{\"id\":\"c_00").unwrap();
        drop(store);
        let (reloaded, _) = Store::open(&dir).unwrap();
        assert_eq!(reloaded.contacts().len(), 1);
        assert_eq!(reloaded.contacts()[0].display_name, "Test Contact 01");
        assert!(!tmp.exists(), "stale tmp is discarded");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn corrupt_document_is_copied_and_store_refuses_to_start() {
        let dir = temp_dir("corrupt");
        let path = Store::path_in(&dir);
        fs::write(&path, "{ not json").unwrap();
        let error = Store::open(&dir).err().expect("refused");
        assert!(error.to_string().contains("preserved in place"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "{ not json");
        let copies: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("contacts.json.corrupt-")
            })
            .collect();
        assert_eq!(copies.len(), 1);
        assert_eq!(fs::read_to_string(copies[0].path()).unwrap(), "{ not json");
        // Repeated startup still fails: it cannot silently become empty.
        assert!(Store::open(&dir).is_err());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn newer_schema_refuses_to_start_and_leaves_the_file_alone() {
        let dir = temp_dir("newer");
        let text = "{\"schema_version\":99,\"contacts\":[]}";
        fs::write(Store::path_in(&dir), text).unwrap();
        let error = Store::open(&dir).err().expect("refused");
        assert_eq!(
            error,
            ContactsError::SchemaTooNew {
                found: 99,
                supported: SCHEMA_VERSION
            }
        );
        assert_eq!(fs::read_to_string(Store::path_in(&dir)).unwrap(), text);
        fs::remove_dir_all(&dir).ok();
    }
}
