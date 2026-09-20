//! Versioned SMS store: one JSON document under the data directory, written
//! atomically (temporary file, `fsync`, rename). Bodies and numbers live only
//! in this file and on authorized protocol connections; the daemon never logs
//! them. A document that fails to parse remains authoritative, is copied
//! aside for diagnosis (`*.corrupt-<unix-ms>`), and makes startup fail closed.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use ferrix_telephony_api::{MessageId, Sms};
use serde::{Deserialize, Serialize};

/// The only store schema this crate reads and writes.
pub const STORE_SCHEMA_VERSION: u32 = 1;
/// File name inside the data directory.
pub const STORE_FILE: &str = "sms.json";
/// Default data directory inside the guest.
pub const DEFAULT_DATA_DIR: &str = "/var/lib/ferrix/telephony";
/// Environment variable overriding the data directory.
pub const DATA_DIR_ENV: &str = "FERRIX_TELEPHONY_DIR";

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoreDocument {
    pub schema_version: u32,
    /// Next `m<n>` to hand out; monotonic across restarts.
    pub next_message: u64,
    #[serde(default)]
    pub messages: BTreeMap<MessageId, Sms>,
}

impl StoreDocument {
    pub fn empty() -> Self {
        Self {
            schema_version: STORE_SCHEMA_VERSION,
            next_message: 1,
            messages: BTreeMap::new(),
        }
    }
}

/// How a load ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Loaded {
    /// No file yet: a fresh store.
    Fresh,
    Ok,
}

pub struct SmsStore {
    dir: PathBuf,
}

impl SmsStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn path(&self) -> PathBuf {
        self.dir.join(STORE_FILE)
    }

    /// Reads the document. I/O errors other than "missing" are returned;
    /// parse errors and schema mismatches preserve the authoritative file,
    /// write a forensic copy beside it, and fail startup.
    pub fn load(&self) -> Result<(StoreDocument, Loaded), String> {
        let path = self.path();
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok((StoreDocument::empty(), Loaded::Fresh));
            }
            Err(e) => return Err(format!("cannot read {}: {e}", path.display())),
        };
        let parsed: Result<StoreDocument, String> = serde_json::from_str(&text)
            .map_err(|e| e.to_string())
            .and_then(|doc: StoreDocument| {
                if doc.schema_version == STORE_SCHEMA_VERSION {
                    Ok(doc)
                } else {
                    Err(format!(
                        "schema_version {} is not {STORE_SCHEMA_VERSION}",
                        doc.schema_version
                    ))
                }
            });
        match parsed {
            Ok(doc) => Ok((doc, Loaded::Ok)),
            Err(reason) => {
                let quarantine = self.dir.join(format!(
                    "{STORE_FILE}.corrupt-{}",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|duration| duration.as_millis())
                        .unwrap_or(0)
                ));
                fs::copy(&path, &quarantine).map_err(|e| {
                    format!(
                        "cannot copy corrupt store {} for diagnosis: {e}",
                        path.display()
                    )
                })?;
                if let Ok(file) = File::open(&quarantine) {
                    let _ = file.sync_all();
                }
                Err(format!(
                    "{} is corrupt ({reason}); preserved in place and copied to {}",
                    path.display(),
                    quarantine.display()
                ))
            }
        }
    }

    /// Atomic replace: write `sms.json.tmp`, fsync, rename over `sms.json`,
    /// fsync the directory.
    pub fn save(&self, document: &StoreDocument) -> Result<(), String> {
        fs::create_dir_all(&self.dir)
            .map_err(|e| format!("cannot create {}: {e}", self.dir.display()))?;
        let path = self.path();
        let tmp = self.dir.join(format!("{STORE_FILE}.tmp"));
        let text = serde_json::to_vec(document).map_err(|e| format!("encode store: {e}"))?;
        {
            let mut file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&tmp)
                .map_err(|e| format!("cannot open {}: {e}", tmp.display()))?;
            file.write_all(&text)
                .and_then(|()| file.sync_all())
                .map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
        }
        fs::rename(&tmp, &path).map_err(|e| {
            format!(
                "cannot rename {} over {}: {e}",
                tmp.display(),
                path.display()
            )
        })?;
        if let Ok(dir) = File::open(&self.dir) {
            let _ = dir.sync_all();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrix_telephony_api::{Direction, E164, PartyRef, SmsState};

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ferrix-telephonyd-store-{}-{tag}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn sms(id: &str) -> Sms {
        Sms {
            id: MessageId::from(id),
            direction: Direction::Incoming,
            party: PartyRef::new(E164::new("+15550100042").unwrap()),
            body: "hello".into(),
            parts: Vec::new(),
            state: SmsState::Received,
            timestamp_ms: 5,
            failure: None,
        }
    }

    #[test]
    fn fresh_then_round_trip_then_corrupt() {
        let dir = temp_dir("rt");
        let store = SmsStore::new(&dir);
        let (doc, loaded) = store.load().unwrap();
        assert_eq!(loaded, Loaded::Fresh);
        assert_eq!(doc, StoreDocument::empty());

        let mut doc = StoreDocument::empty();
        doc.messages.insert(MessageId::from("m1"), sms("m1"));
        doc.next_message = 2;
        store.save(&doc).unwrap();
        assert!(!store.dir().join(format!("{STORE_FILE}.tmp")).exists());
        let (back, loaded) = store.load().unwrap();
        assert_eq!(loaded, Loaded::Ok);
        assert_eq!(back, doc);

        fs::write(store.path(), "{ not json").unwrap();
        let error = store.load().unwrap_err();
        assert!(error.contains("is corrupt"));
        assert_eq!(fs::read_to_string(store.path()).unwrap(), "{ not json");
        assert_eq!(
            fs::read_dir(&dir)
                .unwrap()
                .filter_map(Result::ok)
                .filter(|entry| entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("sms.json.corrupt-"))
                .count(),
            1
        );

        fs::write(store.path(), r#"{"schema_version":2,"next_message":1}"#).unwrap();
        assert!(store.load().unwrap_err().contains("schema_version"));
        let _ = fs::remove_dir_all(&dir);
    }
}
