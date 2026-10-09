//! `kill-daemon`: host-driven fault injection for supervision tests.
//!
//! The QEMU guest runs the service daemons under a supervise loop that
//! restarts a daemon after a non-zero exit and records `daemon-restarted`.
//! Backend-loss scenarios (`crash-backend`) only exercise reconciliation
//! inside a live process; this command really kills one, so a scenario can
//! prove the process comes back. It is a mock-control command: the core only
//! accepts it when the session has the mock-control capability, only for the
//! daemons in [`KILLABLE`], and only by exact executable name.

use std::path::Path;

/// Daemons the guest init supervises (restart on non-zero exit). The core,
/// `ferrix-appd`, and the compositor are deliberately absent: init does not
/// supervise them, so killing one would end the session instead of testing
/// a restart.
pub const KILLABLE: &[&str] = &["ferrix-telephonyd", "ferrix-networkd", "ferrix-contactsd"];

/// Pids under `proc_root` whose `argv[0]` basename is exactly `name`,
/// excluding `own_pid`. `comm` is not used because Linux truncates it to 15
/// bytes (`ferrix-telephon`).
pub fn find_pids(proc_root: &Path, name: &str, own_pid: u32) -> Vec<u32> {
    let Ok(entries) = std::fs::read_dir(proc_root) else {
        return Vec::new();
    };
    let mut pids: Vec<u32> = entries
        .flatten()
        .filter_map(|entry| entry.file_name().to_str()?.parse::<u32>().ok())
        .filter(|pid| *pid != own_pid)
        .filter(|pid| {
            let Ok(cmdline) = std::fs::read(proc_root.join(pid.to_string()).join("cmdline")) else {
                return false;
            };
            let argv0 = cmdline.split(|b| *b == 0).next().unwrap_or_default();
            let base = argv0.rsplit(|b| *b == b'/').next().unwrap_or_default();
            base == name.as_bytes()
        })
        .collect();
    pids.sort_unstable();
    pids
}

/// Why a kill request was refused.
#[derive(Debug, PartialEq, Eq)]
pub enum KillError {
    NotKillable,
    NotRunning,
    Signal(String),
}

impl KillError {
    pub fn reason(&self, name: &str) -> String {
        match self {
            KillError::NotKillable => format!(
                "{name} is not a supervised daemon (killable: {})",
                KILLABLE.join(", ")
            ),
            KillError::NotRunning => format!("no running process named {name}"),
            KillError::Signal(error) => format!("cannot signal {name}: {error}"),
        }
    }
}

/// The pids a `kill-daemon` for `name` would signal (normally exactly one).
pub fn resolve(proc_root: &Path, name: &str) -> Result<Vec<u32>, KillError> {
    if !KILLABLE.contains(&name) {
        return Err(KillError::NotKillable);
    }
    let pids = find_pids(proc_root, name, std::process::id());
    if pids.is_empty() {
        return Err(KillError::NotRunning);
    }
    Ok(pids)
}

/// Sends SIGKILL to each pid.
pub fn kill(pids: &[u32]) -> Result<(), KillError> {
    for pid in pids {
        let target = i32::try_from(*pid)
            .ok()
            .and_then(rustix::process::Pid::from_raw)
            .ok_or_else(|| KillError::Signal(format!("invalid pid {pid}")))?;
        rustix::process::kill_process(target, rustix::process::Signal::KILL)
            .map_err(|e| KillError::Signal(e.to_string()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_proc(tag: &str, processes: &[(u32, &[u8])]) -> std::path::PathBuf {
        let root =
            std::env::temp_dir().join(format!("ferrix-core-proc-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("self")).unwrap();
        for (pid, cmdline) in processes {
            let dir = root.join(pid.to_string());
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("cmdline"), cmdline).unwrap();
        }
        root
    }

    #[test]
    fn finds_daemons_by_exact_argv0_basename() {
        let root = fake_proc(
            "find",
            &[
                (10, b"/usr/bin/ferrix-networkd\0--backend\0mock\0"),
                (11, b"/usr/bin/ferrix-telephonyd\0--backend\0mock\0"),
                (12, b"/bin/sh\0-c\0ferrix-networkd\0"),
                (13, b"/usr/bin/ferrix-networkd-helper\0"),
                (14, b""),
                (15, b"ferrix-networkd\0"),
            ],
        );
        assert_eq!(find_pids(&root, "ferrix-networkd", 1), vec![10, 15]);
        assert_eq!(find_pids(&root, "ferrix-networkd", 15), vec![10]);
        assert_eq!(find_pids(&root, "ferrix-telephonyd", 1), vec![11]);
        assert!(find_pids(&root, "ferrix-contactsd", 1).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn refuses_unsupervised_names_and_absent_daemons() {
        let root = fake_proc("refuse", &[(20, b"/usr/bin/ferrix-core\0")]);
        assert_eq!(resolve(&root, "ferrix-core"), Err(KillError::NotKillable));
        assert_eq!(resolve(&root, "init"), Err(KillError::NotKillable));
        assert_eq!(
            resolve(&root, "ferrix-networkd"),
            Err(KillError::NotRunning)
        );
        assert!(
            KillError::NotKillable
                .reason("ferrix-core")
                .contains("ferrix-networkd")
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
