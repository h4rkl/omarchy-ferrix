//! The default seccomp profile (issue #10): a deny list of dangerous system
//! calls, the escape hatch that relaxes it per launch configuration, and (on
//! Linux) the syscall numbers. The filter itself is compiled and installed by
//! `ferrix-sandbox` with `seccompiler` (rust-vmm, Apache-2.0 OR
//! BSD-3-Clause).
//!
//! The profile is "default-deny-dangerous": everything is allowed except
//! the calls below, which return `EPERM`. They load kernel code or
//! programs (`kexec_*`, `*_module`, `bpf`), reach into other processes
//! (`ptrace`, `process_vm_*`, `pidfd_getfd`, `kcmp`), change the
//! filesystem view or namespaces after the sandbox built them (`mount`,
//! `umount2`, `pivot_root`, `chroot`, the new mount API, `setns`,
//! `unshare`, and `clone` with any namespace flag), touch the kernel
//! keyring, change system-wide state (clock, hostname, swap, reboot,
//! accounting, quotas, the kernel log), or are classic exploit primitives
//! (`userfaultfd`, `perf_event_open`, `open_by_handle_at`). `clone3` returns
//! `ENOSYS` instead, because its flags sit behind a pointer the filter cannot
//! read; libc then falls back to `clone`, whose flags it can.

use serde::{Deserialize, Serialize};

/// The default deny list, by name, on every supported architecture.
pub const DENIED_SYSCALLS: &[&str] = &[
    "acct",
    "add_key",
    "adjtimex",
    "bpf",
    "chroot",
    "clock_adjtime",
    "clock_settime",
    "delete_module",
    "finit_module",
    "fsconfig",
    "fsmount",
    "fsopen",
    "fspick",
    "init_module",
    "kcmp",
    "kexec_file_load",
    "kexec_load",
    "keyctl",
    "mount",
    "mount_setattr",
    "move_mount",
    "name_to_handle_at",
    "open_by_handle_at",
    "open_tree",
    "perf_event_open",
    "pidfd_getfd",
    "pivot_root",
    "process_vm_readv",
    "process_vm_writev",
    "ptrace",
    "quotactl",
    "quotactl_fd",
    "reboot",
    "request_key",
    "setdomainname",
    "sethostname",
    "setns",
    "settimeofday",
    "swapoff",
    "swapon",
    "syslog",
    "umount2",
    "unshare",
    "userfaultfd",
    "vhangup",
];

/// x86_64-only calls denied as well (port I/O and legacy module calls).
pub const DENIED_SYSCALLS_X86_64: &[&str] = &["ioperm", "iopl", "uselib"];

/// Name used for the `clone`-with-namespace-flags rule in `allow`.
pub const CLONE_NAMESPACES: &str = "clone-namespaces";

/// Which profile a launch gets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SeccompProfile {
    /// [`DENIED_SYSCALLS`] minus [`SeccompPolicy::allow`].
    #[default]
    Default,
    /// No filter at all: the escape hatch for a runtime whose payload
    /// brings its own syscall policy. (The Android runtime does not use this
    /// sandbox at all.)
    Unconfined,
}

/// The seccomp part of a native launch configuration. It lives in the
/// runtime-private launch data, so each runtime adapter decides what its
/// payload needs; `allow` can only remove entries from the deny list, never
/// add calls the default profile does not know.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SeccompPolicy {
    pub profile: SeccompProfile,
    /// Deny-list entries this launch still needs (for example `ptrace` for
    /// a debugger), or [`CLONE_NAMESPACES`] for nested namespaces.
    pub allow: Vec<String>,
}

impl SeccompPolicy {
    pub fn unconfined() -> Self {
        Self {
            profile: SeccompProfile::Unconfined,
            allow: Vec::new(),
        }
    }

    pub fn is_confined(&self) -> bool {
        self.profile == SeccompProfile::Default
    }
}

/// What was installed: `{profile, denied, allowed}` (`profile: null` when
/// no filter was installed).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SeccompOutcome {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<SeccompProfile>,
    /// Number of system calls the installed filter refuses.
    pub denied: usize,
    /// Deny-list entries relaxed by the launch configuration.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub allowed: Vec<String>,
}

impl SeccompOutcome {
    pub fn applied(&self) -> bool {
        self.profile.is_some()
    }
}

/// The deny list for the architecture this crate is built for.
pub fn denied_for_target() -> Vec<&'static str> {
    let mut names = DENIED_SYSCALLS.to_vec();
    if cfg!(target_arch = "x86_64") {
        names.extend_from_slice(DENIED_SYSCALLS_X86_64);
    }
    names
}

/// Rejects `allow` entries that are not on the deny list (a typo must not
/// read as a relaxation that is not there).
pub fn validate_allow(policy: &SeccompPolicy) -> Result<(), String> {
    let known = denied_for_target();
    for name in &policy.allow {
        if name != CLONE_NAMESPACES && !known.contains(&name.as_str()) {
            return Err(format!(
                "seccomp.allow {name:?} is not on the default deny list"
            ));
        }
    }
    Ok(())
}

/// The deny list after the escape hatch: what the filter refuses.
pub fn effective_denied(policy: &SeccompPolicy) -> Vec<&'static str> {
    denied_for_target()
        .into_iter()
        .filter(|name| !policy.allow.iter().any(|a| a == name))
        .collect()
}

/// Whether `clone` with namespace flags stays refused.
pub fn denies_clone_namespaces(policy: &SeccompPolicy) -> bool {
    !policy.allow.iter().any(|a| a == CLONE_NAMESPACES)
}

/// `kexec_file_load`: the musl `libc` bindings for aarch64 lack the
/// constant. Numbers from the kernel's tables (asm-generic `unistd.h`, used
/// by aarch64 and riscv64; `syscall_64.tbl` for x86_64).
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const SYS_KEXEC_FILE_LOAD: i64 = 320;
#[cfg(all(target_os = "linux", not(target_arch = "x86_64")))]
const SYS_KEXEC_FILE_LOAD: i64 = 294;

/// System call numbers for the deny list on this architecture.
#[cfg(target_os = "linux")]
pub fn syscall_number(name: &str) -> Option<i64> {
    use libc as c;
    let number = match name {
        "acct" => c::SYS_acct,
        "add_key" => c::SYS_add_key,
        "adjtimex" => c::SYS_adjtimex,
        "bpf" => c::SYS_bpf,
        "chroot" => c::SYS_chroot,
        "clock_adjtime" => c::SYS_clock_adjtime,
        "clock_settime" => c::SYS_clock_settime,
        "delete_module" => c::SYS_delete_module,
        "finit_module" => c::SYS_finit_module,
        "fsconfig" => c::SYS_fsconfig,
        "fsmount" => c::SYS_fsmount,
        "fsopen" => c::SYS_fsopen,
        "fspick" => c::SYS_fspick,
        "init_module" => c::SYS_init_module,
        "kcmp" => c::SYS_kcmp,
        "kexec_file_load" => SYS_KEXEC_FILE_LOAD,
        "kexec_load" => c::SYS_kexec_load,
        "keyctl" => c::SYS_keyctl,
        "mount" => c::SYS_mount,
        "mount_setattr" => c::SYS_mount_setattr,
        "move_mount" => c::SYS_move_mount,
        "name_to_handle_at" => c::SYS_name_to_handle_at,
        "open_by_handle_at" => c::SYS_open_by_handle_at,
        "open_tree" => c::SYS_open_tree,
        "perf_event_open" => c::SYS_perf_event_open,
        "pidfd_getfd" => c::SYS_pidfd_getfd,
        "pivot_root" => c::SYS_pivot_root,
        "process_vm_readv" => c::SYS_process_vm_readv,
        "process_vm_writev" => c::SYS_process_vm_writev,
        "ptrace" => c::SYS_ptrace,
        "quotactl" => c::SYS_quotactl,
        "quotactl_fd" => c::SYS_quotactl_fd,
        "reboot" => c::SYS_reboot,
        "request_key" => c::SYS_request_key,
        "setdomainname" => c::SYS_setdomainname,
        "sethostname" => c::SYS_sethostname,
        "setns" => c::SYS_setns,
        "settimeofday" => c::SYS_settimeofday,
        "swapoff" => c::SYS_swapoff,
        "swapon" => c::SYS_swapon,
        "syslog" => c::SYS_syslog,
        "umount2" => c::SYS_umount2,
        "unshare" => c::SYS_unshare,
        "userfaultfd" => c::SYS_userfaultfd,
        "vhangup" => c::SYS_vhangup,
        #[cfg(target_arch = "x86_64")]
        "ioperm" => c::SYS_ioperm,
        #[cfg(target_arch = "x86_64")]
        "iopl" => c::SYS_iopl,
        #[cfg(target_arch = "x86_64")]
        "uselib" => c::SYS_uselib,
        _ => return None,
    };
    Some(number)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allow_only_relaxes_known_entries() {
        let policy = SeccompPolicy {
            allow: vec!["ptrace".to_owned()],
            ..SeccompPolicy::default()
        };
        assert!(validate_allow(&policy).is_ok());
        let denied = effective_denied(&policy);
        assert!(!denied.contains(&"ptrace") && denied.contains(&"mount"));
        assert_eq!(denied.len() + 1, denied_for_target().len());
        assert!(denies_clone_namespaces(&policy));

        let nested = SeccompPolicy {
            allow: vec![CLONE_NAMESPACES.to_owned(), "unshare".to_owned()],
            ..SeccompPolicy::default()
        };
        assert!(validate_allow(&nested).is_ok());
        assert!(!denies_clone_namespaces(&nested));

        let typo = SeccompPolicy {
            allow: vec!["ptarce".to_owned()],
            ..SeccompPolicy::default()
        };
        assert!(validate_allow(&typo).unwrap_err().contains("ptarce"));
        // Allowing something that is not denied anyway is refused too.
        let read = SeccompPolicy {
            allow: vec!["read".to_owned()],
            ..SeccompPolicy::default()
        };
        assert!(validate_allow(&read).is_err());
    }

    #[test]
    fn deny_list_is_sorted_unique_and_parses_with_defaults() {
        let mut sorted = DENIED_SYSCALLS.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted, DENIED_SYSCALLS, "keep the list sorted and unique");
        let parsed: SeccompPolicy = serde_json::from_str(r#"{"allow":["ptrace"]}"#).unwrap();
        assert_eq!(parsed.profile, SeccompProfile::Default);
        let parsed: SeccompPolicy = serde_json::from_str(r#"{"profile":"unconfined"}"#).unwrap();
        assert!(!parsed.is_confined());
        assert!(serde_json::from_str::<SeccompPolicy>(r#"{"deny":["read"]}"#).is_err());
        let outcome = SeccompOutcome::default();
        assert_eq!(serde_json::to_string(&outcome).unwrap(), r#"{"denied":0}"#);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn every_denied_name_has_a_number_on_this_architecture() {
        for name in denied_for_target() {
            assert!(syscall_number(name).is_some(), "{name}");
        }
        assert_eq!(syscall_number("read"), None);
    }
}
