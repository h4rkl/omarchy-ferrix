//! Per-application uid through the user namespace (issue #10): the pure
//! parts.
//!
//! With `uid: per-app` (root launchers only) stage 1 unshares a user
//! namespace whose map holds two single-id lines, identity-mapped:
//!
//! ```text
//! 0 0 1        the sandbox stages: root's file ownership, but capabilities
//!              only inside the application's own namespaces
//! S S 1        the application's subuid (and subgid) S = app_subuid(id)
//! ```
//!
//! Only the application process switches to `S` (stage 2 starts it with
//! `uid`/`gid` set, which also clears supplementary groups), so it holds no
//! capability even inside its namespaces. Its data directory and its
//! stand-in `$HOME` are owned by `S`. Writing a map with an id other than
//! the writer's own needs `CAP_SETUID` in the parent namespace, which stage
//! 1 gives up by unsharing; a helper started before the `unshare`
//! (`ferrix-sandbox --map-ids`) writes it.

use serde::{Deserialize, Serialize};

/// First id of the Ferrix application range: high and distinctive, clear of
/// `/etc/subuid` ranges handed to users (typically from 100000) and of
/// `nobody` (65534).
pub const SUBUID_BASE: u32 = 0x6000_0000;
/// Number of ids in the range (2^24).
pub const SUBUID_SLOTS: u32 = 1 << 24;

/// Which identity the application runs as.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UidMode {
    /// The launcher's own uid (mapped 1:1 when a user namespace exists).
    #[default]
    Launcher,
    /// A distinct, stable subuid per application id ([`app_subuid`]).
    PerApp,
}

/// What identity the application got: `{mode, uid, gid}`; `uid`/`gid` only
/// with `per-app`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct IdentityOutcome {
    pub mode: UidMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gid: Option<u32>,
}

/// 32-bit FNV-1a: stable across builds and platforms (unlike `std`'s
/// hasher), which keeps an application's uid, and so its files' ownership,
/// the same across reboots and upgrades.
fn fnv1a(text: &str) -> u32 {
    text.bytes().fold(0x811c_9dc5u32, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    })
}

/// The stable subuid (and subgid) of an application id. Two ids share one
/// with probability about n²/2^25 for n applications; a collision only
/// merges those two applications' uid, never with a system or user id.
pub fn app_subuid(app_id: &str) -> u32 {
    SUBUID_BASE + fnv1a(app_id) % SUBUID_SLOTS
}

/// The `uid_map`/`gid_map` text for a per-application namespace created by
/// a launcher whose (u|g)id is `launcher`.
pub fn per_app_map(launcher: u32, subuid: u32) -> String {
    format!("{launcher} {launcher} 1\n{subuid} {subuid} 1\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subuids_are_stable_distinct_and_in_range() {
        let files = app_subuid("org.ferrix.files");
        assert_eq!(files, app_subuid("org.ferrix.files"));
        // Pinned: a change here would change every installed app's file
        // ownership.
        assert_eq!(fnv1a(""), 0x811c_9dc5);
        assert_eq!(fnv1a("a"), 0xe40c_292c);
        let ids = [
            "org.ferrix.home",
            "org.ferrix.terminal",
            "org.ferrix.phone",
            "org.ferrix.messages",
            "org.ferrix.contacts",
            "org.ferrix.settings",
            "org.ferrix.files",
            "org.ferrix.camera",
            "org.ferrix.browser",
            "org.ferrix.test",
        ];
        let mut uids: Vec<u32> = ids.iter().map(|id| app_subuid(id)).collect();
        assert!(
            uids.iter()
                .all(|u| (SUBUID_BASE..SUBUID_BASE + SUBUID_SLOTS).contains(u))
        );
        uids.sort_unstable();
        uids.dedup();
        assert_eq!(uids.len(), ids.len(), "checked-in ids must not collide");
        assert_eq!(per_app_map(0, 7), "0 0 1\n7 7 1\n");
    }

    #[test]
    fn identity_outcome_serializes_compactly() {
        assert_eq!(
            serde_json::to_string(&IdentityOutcome::default()).unwrap(),
            r#"{"mode":"launcher"}"#
        );
        let per_app = IdentityOutcome {
            mode: UidMode::PerApp,
            uid: Some(5),
            gid: Some(5),
        };
        assert_eq!(
            serde_json::to_string(&per_app).unwrap(),
            r#"{"mode":"per-app","uid":5,"gid":5}"#
        );
    }
}
