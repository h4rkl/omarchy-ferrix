//! Screen policy owned by the core: the user's idle timeout and the live
//! blank inhibitors (calls, video, navigation).
//!
//! The shell enforces the policy (it owns input and the DRM output); the
//! core owns the decision inputs so applications never talk to the
//! compositor. Inhibitors from the state socket are tied to the requesting
//! connection (`server.rs` releases them when it closes); inhibitors from the
//! mock-control channel are named by holder and released explicitly.
//!
//! Every change is one `screen-policy` record on stdout and one
//! [`PolicyUpdate`] line to every watcher. Only the idle timeout persists
//! (`--screen-state`); inhibitors never outlive the core.

use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use ferrix_hardware_api::{
    HardwareError, InhibitReason, SCHEMA_VERSION, ScreenInhibitor, ScreenPolicy,
    validate_idle_timeout,
};
use ferrix_protocol::{PolicyUpdate, Reply};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Longest holder label kept (evidence and Settings caption only).
const MAX_HOLDER_LEN: usize = 64;

/// What persists across core restarts: the user's choice only.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Saved {
    idle_timeout_secs: Option<u32>,
}

struct Inner {
    policy: ScreenPolicy,
    next_id: u64,
}

pub struct ScreenPolicyStore {
    inner: Mutex<Inner>,
    watchers: Mutex<Vec<UnixStream>>,
    persist: Option<PathBuf>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn holder_label(holder: &str) -> String {
    let clean: String = holder
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_HOLDER_LEN)
        .collect();
    if clean.trim().is_empty() {
        "unnamed".to_owned()
    } else {
        clean
    }
}

fn write_line(stream: &mut UnixStream, value: &impl Serialize) -> std::io::Result<()> {
    let mut text = serde_json::to_string(value).map_err(std::io::Error::other)?;
    text.push('\n');
    stream.write_all(text.as_bytes())?;
    stream.flush()
}

impl ScreenPolicyStore {
    /// Starts from the target profile's default timeout, then the saved
    /// user choice at `persist` when it is readable and valid.
    pub fn new(profile_default: Option<u32>, persist: Option<PathBuf>) -> Self {
        let mut idle = profile_default;
        let mut source = "profile";
        if let Some(path) = &persist {
            match std::fs::read_to_string(path) {
                Ok(text) => match serde_json::from_str::<Saved>(&text)
                    .map_err(|e| e.to_string())
                    .and_then(|s| {
                        validate_idle_timeout(s.idle_timeout_secs).map_err(|e| e.to_string())
                    }) {
                    Ok(saved) => {
                        idle = saved;
                        source = "saved";
                    }
                    Err(reason) => crate::emit(json!({
                        "evidence": crate::evidence_label(),
                        "event": "screen-policy-invalid",
                        "path": path.display().to_string(),
                        "reason": reason,
                    })),
                },
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => crate::emit(json!({
                    "evidence": crate::evidence_label(),
                    "event": "screen-policy-invalid",
                    "path": path.display().to_string(),
                    "reason": e.to_string(),
                })),
            }
        }
        let store = Self {
            inner: Mutex::new(Inner {
                policy: ScreenPolicy {
                    idle_timeout_secs: idle,
                    inhibitors: Vec::new(),
                },
                next_id: 1,
            }),
            watchers: Mutex::new(Vec::new()),
            persist,
        };
        store.record(&store.policy(), source, None);
        store
    }

    pub fn policy(&self) -> ScreenPolicy {
        lock(&self.inner).policy.clone()
    }

    pub fn reply(&self) -> Reply {
        Reply::ScreenPolicy {
            schema_version: SCHEMA_VERSION,
            policy: self.policy(),
        }
    }

    pub fn set_idle_timeout(&self, secs: Option<u32>) -> Result<ScreenPolicy, HardwareError> {
        let secs = validate_idle_timeout(secs)?;
        let policy = {
            let mut inner = lock(&self.inner);
            inner.policy.idle_timeout_secs = secs;
            inner.policy.clone()
        };
        self.save(secs);
        self.changed(&policy, "set-idle-timeout", None);
        Ok(policy)
    }

    pub fn inhibit(&self, reason: InhibitReason, holder: &str) -> (u64, ScreenPolicy) {
        let holder = holder_label(holder);
        let (id, policy) = {
            let mut inner = lock(&self.inner);
            let id = inner.next_id;
            inner.next_id += 1;
            inner.policy.inhibitors.push(ScreenInhibitor {
                id,
                reason,
                holder: holder.clone(),
            });
            (id, inner.policy.clone())
        };
        self.changed(&policy, "inhibit", Some(&holder));
        (id, policy)
    }

    /// Releases one inhibitor; `None` when no such id is held.
    pub fn release(&self, id: u64, cause: &str) -> Option<ScreenPolicy> {
        let (holder, policy) = {
            let mut inner = lock(&self.inner);
            let index = inner.policy.inhibitors.iter().position(|i| i.id == id)?;
            let removed = inner.policy.inhibitors.remove(index);
            (removed.holder, inner.policy.clone())
        };
        self.changed(&policy, cause, Some(&holder));
        Some(policy)
    }

    /// Releases every inhibitor `holder` holds (mock-control verb);
    /// returns how many were released.
    pub fn release_holder(&self, holder: &str) -> usize {
        let holder = holder_label(holder);
        let ids: Vec<u64> = lock(&self.inner)
            .policy
            .inhibitors
            .iter()
            .filter(|i| i.holder == holder)
            .map(|i| i.id)
            .collect();
        for id in &ids {
            self.release(*id, "release");
        }
        ids.len()
    }

    /// Sends the current policy and registers `stream` for every change.
    /// The watcher lock is held across both so no change is missed.
    pub fn watch(&self, mut stream: UnixStream) -> std::io::Result<()> {
        let mut watchers = lock(&self.watchers);
        write_line(&mut stream, &self.reply())?;
        watchers.push(stream);
        Ok(())
    }

    fn save(&self, secs: Option<u32>) {
        let Some(path) = &self.persist else {
            return;
        };
        let result = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| {
                let tmp = path.with_extension("json.tmp");
                let text = serde_json::to_string(&Saved {
                    idle_timeout_secs: secs,
                })
                .map_err(std::io::Error::other)?;
                std::fs::write(&tmp, format!("{text}\n"))?;
                std::fs::rename(&tmp, path)
            });
        if let Err(e) = result {
            crate::emit(json!({
                "evidence": crate::evidence_label(),
                "event": "screen-policy-save-failed",
                "path": path.display().to_string(),
                "reason": e.to_string(),
            }));
        }
    }

    fn record(&self, policy: &ScreenPolicy, cause: &str, holder: Option<&str>) {
        crate::emit(json!({
            "evidence": crate::evidence_label(),
            "event": "screen-policy",
            "cause": cause,
            "holder": holder,
            "idle_timeout_secs": policy.idle_timeout_secs,
            "inhibited": policy.inhibited(),
            "reasons": policy.reasons().iter().map(|r| r.name()).collect::<Vec<_>>(),
            "inhibitors": policy.inhibitors.len(),
        }));
    }

    fn changed(&self, policy: &ScreenPolicy, cause: &str, holder: Option<&str>) {
        self.record(policy, cause, holder);
        let update = PolicyUpdate::ScreenPolicyChanged {
            policy: policy.clone(),
        };
        lock(&self.watchers).retain_mut(|stream| write_line(stream, &update).is_ok());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ferrix-screen-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("screen.json")
    }

    #[test]
    fn idle_timeout_persists_and_inhibitors_do_not() {
        let path = temp("persist");
        let store = ScreenPolicyStore::new(None, Some(path.clone()));
        assert_eq!(store.policy().idle_timeout_secs, None, "profile default");
        store.set_idle_timeout(Some(30)).unwrap();
        let (id, policy) = store.inhibit(InhibitReason::Call, "org.ferrix.phone");
        assert!(policy.inhibited());
        assert_eq!(policy.effective_idle_timeout_secs(), None);
        let restarted = ScreenPolicyStore::new(None, Some(path.clone()));
        let policy = restarted.policy();
        assert_eq!(policy.idle_timeout_secs, Some(30), "the user choice wins");
        assert!(!policy.inhibited(), "inhibitors never outlive the core");
        assert!(store.release(id, "release").is_some());
        assert!(store.release(id, "release").is_none(), "released once");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn invalid_values_are_typed_errors_and_a_bad_file_falls_back() {
        let path = temp("invalid");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{"idle_timeout_secs":1}"#).unwrap();
        let store = ScreenPolicyStore::new(Some(60), Some(path.clone()));
        assert_eq!(store.policy().idle_timeout_secs, Some(60));
        assert!(store.set_idle_timeout(Some(2)).is_err());
        assert_eq!(store.policy().idle_timeout_secs, Some(60), "unchanged");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn holders_are_sanitized_and_released_by_name() {
        let store = ScreenPolicyStore::new(Some(60), None);
        store.inhibit(InhibitReason::Video, "scenario");
        store.inhibit(InhibitReason::Navigation, "scenario");
        let (_, policy) = store.inhibit(InhibitReason::Call, "\u{7}");
        assert_eq!(policy.inhibitors[2].holder, "unnamed");
        assert_eq!(store.release_holder("scenario"), 2);
        assert_eq!(store.policy().reasons(), [InhibitReason::Call]);
    }

    #[test]
    fn watchers_get_the_policy_then_every_change() {
        let store = ScreenPolicyStore::new(Some(60), None);
        let (ours, theirs) = UnixStream::pair().unwrap();
        store.watch(ours).unwrap();
        store.set_idle_timeout(Some(15)).unwrap();
        let mut lines = BufReader::new(theirs).lines();
        let first: Reply = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
        assert!(matches!(
            first,
            Reply::ScreenPolicy { policy, .. } if policy.idle_timeout_secs == Some(60)
        ));
        let next: PolicyUpdate = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
        let PolicyUpdate::ScreenPolicyChanged { policy } = next;
        assert_eq!(policy.idle_timeout_secs, Some(15));
    }
}
