//! Per-application cgroup v2 groups under the Ferrix slice (issue #10).
//!
//! Stage 1 of `ferrix-sandbox` creates `<slice>/app-<app-id>-<pid>`, writes
//! the limits from [`crate::ResourcePolicy`], and moves itself into the
//! group **before** it unshares anything, so stage 2 and every application
//! process inherit it. The runtime removes the group after the supervisor
//! has been reaped ([`remove_app_cgroup`]) and exposes its path through
//! `RunningApp::cgroup_path`, which is the handle a later scheduler boost
//! (`cpu.uclamp.min`, spec 10) targets.
//!
//! Only plain file writes are involved (the cgroup v2 interface files), so
//! everything here is safe Rust over `std::fs` and testable on any host
//! against a directory tree that imitates the hierarchy.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::ResourcePolicy;

/// Mount point of the unified cgroup hierarchy.
pub const CGROUP_FS: &str = "/sys/fs/cgroup";
/// Name of the Ferrix slice directly under [`CGROUP_FS`].
pub const FERRIX_SLICE: &str = "ferrix.slice";
/// Environment variable overriding the slice path (nested runs, tests).
pub const CGROUP_SLICE_ENV: &str = "FERRIX_CGROUP_SLICE";

/// The slice every application group is created in: `FERRIX_CGROUP_SLICE`
/// when set and non-empty, else `/sys/fs/cgroup/ferrix.slice`.
pub fn cgroup_slice() -> PathBuf {
    match std::env::var_os(CGROUP_SLICE_ENV) {
        Some(path) if !path.is_empty() => PathBuf::from(path),
        _ => Path::new(CGROUP_FS).join(FERRIX_SLICE),
    }
}

/// Group name of one launch: the application id plus the pid of the
/// sandbox supervisor (unique while it runs; the runtime knows it from the
/// spawned child even when no readiness report arrives).
pub fn app_cgroup_name(app_id: &str, supervisor_pid: u32) -> String {
    format!("app-{app_id}-{supervisor_pid}")
}

/// Full path of one launch's group under `slice`.
pub fn app_cgroup_path(slice: &Path, app_id: &str, supervisor_pid: u32) -> PathBuf {
    slice.join(app_cgroup_name(app_id, supervisor_pid))
}

/// What the sandbox established: the group path and the limits written.
/// Empty (`{}` in records) when no group was created.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CgroupOutcome {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_max: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pids_max: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_weight: Option<u16>,
}

impl CgroupOutcome {
    pub fn applied(&self) -> bool {
        self.path.is_some()
    }
}

/// Why no group was created.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CgroupError {
    /// The system lacks the feature: no cgroup v2 hierarchy at the slice's
    /// parent, a controller the kernel does not offer there, or a hierarchy
    /// the launcher may not write (no delegation). Reported as degraded,
    /// like a kernel without an IPC namespace; never silently ignored.
    Unavailable(String),
    /// The hierarchy is there but setting up the group failed. Fatal under
    /// `IsolationMode::Required`.
    Failed(String),
}

impl CgroupError {
    pub fn message(&self) -> &str {
        match self {
            CgroupError::Unavailable(m) | CgroupError::Failed(m) => m,
        }
    }
}

/// Controllers a policy needs, in the order they are enabled.
pub fn needed_controllers(policy: &ResourcePolicy) -> Vec<&'static str> {
    let mut needed = Vec::new();
    if policy.memory_max.is_some() {
        needed.push("memory");
    }
    if policy.pids_max.is_some() {
        needed.push("pids");
    }
    if policy.cpu_weight.is_some() {
        needed.push("cpu");
    }
    needed
}

/// Validates the numeric limits before anything is written.
pub fn validate_resources(policy: &ResourcePolicy) -> Result<(), String> {
    if let Some(weight) = policy.cpu_weight
        && !(1..=10_000).contains(&weight)
    {
        return Err(format!("cpu_weight {weight} is outside 1..=10000"));
    }
    if policy.pids_max == Some(0) {
        return Err("pids_max 0 would forbid the application itself".to_owned());
    }
    if let Some(bytes) = policy.memory_max
        && bytes < 4096
    {
        return Err(format!("memory_max {bytes} is below one page"));
    }
    Ok(())
}

fn controller_list(path: &Path) -> std::io::Result<Vec<String>> {
    Ok(std::fs::read_to_string(path)?
        .split_whitespace()
        .map(|c| c.trim_start_matches('+').to_owned())
        .collect())
}

fn write_file(path: &Path, value: &str) -> Result<(), String> {
    std::fs::write(path, value).map_err(|e| format!("write {value:?} to {}: {e}", path.display()))
}

/// A permission-type error on the hierarchy itself means "not delegated to
/// this launcher", which is a missing system feature, not a failure.
fn classify(error: std::io::Error, what: String) -> CgroupError {
    match error.kind() {
        ErrorKind::PermissionDenied | ErrorKind::ReadOnlyFilesystem => {
            CgroupError::Unavailable(format!("{what}: {error} (hierarchy not delegated)"))
        }
        _ => CgroupError::Failed(format!("{what}: {error}")),
    }
}

/// Makes `controllers` available to the children of `dir` by writing
/// `+<controller>` to its `cgroup.subtree_control`, one at a time so an
/// error names the controller.
fn enable_for_children(dir: &Path, controllers: &[&str]) -> Result<(), CgroupError> {
    let control = dir.join("cgroup.subtree_control");
    let enabled = controller_list(&control)
        .map_err(|e| classify(e, format!("read {}", control.display())))?;
    for controller in controllers {
        if enabled.iter().any(|c| c == controller) {
            continue;
        }
        std::fs::write(&control, format!("+{controller}"))
            .map_err(|e| classify(e, format!("enable {controller} in {}", control.display())))?;
    }
    Ok(())
}

/// Creates the group `<slice>/<name>` with the policy's limits. The slice is
/// created when missing, and the controllers are enabled down the path
/// (slice parent → slice). An existing group of the same name is reused only
/// when it is empty (a stale group left by a crashed launcher).
pub fn setup_app_cgroup(
    slice: &Path,
    name: &str,
    policy: &ResourcePolicy,
) -> Result<CgroupOutcome, CgroupError> {
    validate_resources(policy).map_err(CgroupError::Failed)?;
    let parent = slice
        .parent()
        .ok_or_else(|| CgroupError::Failed(format!("slice {} has no parent", slice.display())))?;
    let available = parent.join("cgroup.controllers");
    let offered = match controller_list(&available) {
        Ok(list) => list,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            return Err(CgroupError::Unavailable(format!(
                "no cgroup v2 hierarchy at {} (not mounted, or a v1/hybrid layout)",
                parent.display()
            )));
        }
        Err(e) => return Err(classify(e, format!("read {}", available.display()))),
    };
    let needed = needed_controllers(policy);
    let missing: Vec<&str> = needed
        .iter()
        .copied()
        .filter(|c| !offered.iter().any(|o| o == c))
        .collect();
    if !missing.is_empty() {
        return Err(CgroupError::Unavailable(format!(
            "controller(s) {} not offered at {}",
            missing.join(","),
            parent.display()
        )));
    }
    enable_for_children(parent, &needed)?;
    match std::fs::create_dir(slice) {
        Ok(()) => {}
        Err(e) if e.kind() == ErrorKind::AlreadyExists => {}
        Err(e) => return Err(classify(e, format!("create {}", slice.display()))),
    }
    enable_for_children(slice, &needed)?;

    let group = slice.join(name);
    match std::fs::create_dir(&group) {
        Ok(()) => {}
        Err(e) if e.kind() == ErrorKind::AlreadyExists => {
            let procs = std::fs::read_to_string(group.join("cgroup.procs")).unwrap_or_default();
            if !procs.trim().is_empty() {
                return Err(CgroupError::Failed(format!(
                    "{} already exists and is in use",
                    group.display()
                )));
            }
        }
        Err(e) => return Err(classify(e, format!("create {}", group.display()))),
    }
    let result = write_limits(&group, policy);
    if let Err(error) = result {
        let _ = std::fs::remove_dir(&group);
        return Err(CgroupError::Failed(error));
    }
    Ok(CgroupOutcome {
        path: Some(group.display().to_string()),
        memory_max: policy.memory_max,
        pids_max: policy.pids_max,
        cpu_weight: policy.cpu_weight,
    })
}

fn write_limits(group: &Path, policy: &ResourcePolicy) -> Result<(), String> {
    if let Some(bytes) = policy.memory_max {
        write_file(&group.join("memory.max"), &bytes.to_string())?;
        // One OOM kill takes the whole application down (stage 1, stage 2
        // and every process), so crash recovery sees one clean signal death
        // instead of a half-dead process tree. Older kernels lack the file.
        let oom_group = group.join("memory.oom.group");
        if oom_group.exists() {
            write_file(&oom_group, "1")?;
        }
    }
    if let Some(pids) = policy.pids_max {
        write_file(&group.join("pids.max"), &pids.to_string())?;
    }
    if let Some(weight) = policy.cpu_weight {
        write_file(&group.join("cpu.weight"), &weight.to_string())?;
    }
    Ok(())
}

/// Moves process `pid` (0 = the caller) into the group.
pub fn join_cgroup(group: &Path, pid: u32) -> Result<(), String> {
    write_file(&group.join("cgroup.procs"), &pid.to_string())
}

/// Removes a group once its processes are gone: `cgroup.kill` first (kernel
/// 5.14+; stragglers of a SIGKILLed supervisor), then `rmdir` retried while
/// the kernel still reports it busy. `Ok` when the group is gone or never
/// existed.
pub fn remove_app_cgroup(group: &Path, timeout: Duration) -> Result<(), String> {
    if !group.exists() {
        return Ok(());
    }
    let kill = group.join("cgroup.kill");
    if kill.exists() {
        let _ = std::fs::write(&kill, "1");
    }
    let deadline = Instant::now() + timeout;
    loop {
        match std::fs::remove_dir(group) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
            Err(e) if Instant::now() >= deadline => {
                return Err(format!("remove {}: {e}", group.display()));
            }
            Err(_) => std::thread::sleep(Duration::from_millis(20)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A directory tree imitating a cgroup v2 root.
    fn fake_root(controllers: &str) -> PathBuf {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let root = std::env::temp_dir().join(format!(
            "ferrix-cgroup-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("cgroup.controllers"), controllers).unwrap();
        std::fs::write(root.join("cgroup.subtree_control"), "").unwrap();
        root
    }

    /// The kernel creates interface files in a new group; the fake tree
    /// pre-creates the slice so its control file exists for the second step.
    fn prepare_slice(root: &Path) -> PathBuf {
        let slice = root.join(FERRIX_SLICE);
        std::fs::create_dir_all(&slice).unwrap();
        std::fs::write(slice.join("cgroup.subtree_control"), "").unwrap();
        slice
    }

    #[test]
    fn group_is_created_with_the_limits_and_controllers_enabled() {
        let root = fake_root("cpuset cpu io memory pids");
        let slice = prepare_slice(&root);
        let policy = ResourcePolicy::default();
        let outcome = setup_app_cgroup(&slice, "app-org.ferrix.test-42", &policy).unwrap();
        let group = slice.join("app-org.ferrix.test-42");
        assert_eq!(outcome.path.as_deref(), Some(group.to_str().unwrap()));
        assert_eq!(outcome.memory_max, policy.memory_max);
        let read = |name: &str| std::fs::read_to_string(group.join(name)).unwrap();
        assert_eq!(read("memory.max"), policy.memory_max.unwrap().to_string());
        assert_eq!(read("pids.max"), policy.pids_max.unwrap().to_string());
        assert_eq!(read("cpu.weight"), policy.cpu_weight.unwrap().to_string());
        // The fake file keeps the last write; the kernel would accumulate.
        assert!(
            std::fs::read_to_string(slice.join("cgroup.subtree_control"))
                .unwrap()
                .starts_with('+')
        );
        join_cgroup(&group, 7).unwrap();
        assert_eq!(read("cgroup.procs"), "7");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_hierarchy_or_controller_is_unavailable_not_failed() {
        let nowhere = std::env::temp_dir().join(format!(
            "ferrix-cgroup-none-{}/{FERRIX_SLICE}",
            std::process::id()
        ));
        let err = setup_app_cgroup(&nowhere, "g", &ResourcePolicy::default()).unwrap_err();
        assert!(
            matches!(&err, CgroupError::Unavailable(m) if m.contains("no cgroup v2")),
            "{err:?}"
        );

        let root = fake_root("cpu memory");
        let slice = prepare_slice(&root);
        let err = setup_app_cgroup(&slice, "g", &ResourcePolicy::default()).unwrap_err();
        assert!(
            matches!(&err, CgroupError::Unavailable(m) if m.contains("pids")),
            "{err:?}"
        );
        // Without a pids limit the same hierarchy is enough.
        let no_pids = ResourcePolicy {
            pids_max: None,
            ..ResourcePolicy::default()
        };
        assert!(setup_app_cgroup(&slice, "g", &no_pids).is_ok());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_busy_group_of_the_same_name_is_refused_and_an_empty_one_reused() {
        let root = fake_root("cpu memory pids");
        let slice = prepare_slice(&root);
        let group = slice.join("g");
        std::fs::create_dir_all(&group).unwrap();
        std::fs::write(group.join("cgroup.procs"), "").unwrap();
        assert!(setup_app_cgroup(&slice, "g", &ResourcePolicy::default()).is_ok());
        std::fs::write(group.join("cgroup.procs"), "1234\n").unwrap();
        assert!(matches!(
            setup_app_cgroup(&slice, "g", &ResourcePolicy::default()),
            Err(CgroupError::Failed(_))
        ));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn limits_are_validated_and_names_are_stable() {
        let bad = |policy: ResourcePolicy| validate_resources(&policy).unwrap_err();
        assert!(
            bad(ResourcePolicy {
                cpu_weight: Some(0),
                ..ResourcePolicy::default()
            })
            .contains("cpu_weight")
        );
        assert!(
            bad(ResourcePolicy {
                pids_max: Some(0),
                ..ResourcePolicy::default()
            })
            .contains("pids_max")
        );
        assert!(
            bad(ResourcePolicy {
                memory_max: Some(1),
                ..ResourcePolicy::default()
            })
            .contains("memory_max")
        );
        assert!(validate_resources(&ResourcePolicy::default()).is_ok());
        assert_eq!(
            app_cgroup_name("org.ferrix.files", 812),
            "app-org.ferrix.files-812"
        );
        assert_eq!(
            app_cgroup_path(Path::new("/sys/fs/cgroup/ferrix.slice"), "a.b", 3),
            PathBuf::from("/sys/fs/cgroup/ferrix.slice/app-a.b-3")
        );
        assert_eq!(
            needed_controllers(&ResourcePolicy::default()),
            ["memory", "pids", "cpu"]
        );
        assert!(remove_app_cgroup(Path::new("/nonexistent/ferrix-group"), Duration::ZERO).is_ok());
    }
}
