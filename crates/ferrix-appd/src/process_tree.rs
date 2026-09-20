//! Parent lookup for surface association.
//!
//! The compositor reports the pid of the client that mapped a surface
//! (kernel-provided `SO_PEERCRED`). Sandboxed applications are grandchildren
//! of `ferrix_appd` (`ferrix_appd -> ferrix-sandbox -> init -> app`), so the reported pid
//! rarely equals the pid `ferrix_appd` spawned. The service walks parents upward
//! until it meets a pid it launched. The walk is behind a trait so tests use
//! a fake tree.

use std::path::{Path, PathBuf};

/// Maximum number of parent hops taken while resolving a pid.
pub const MAX_HOPS: usize = 16;

pub trait ProcessTree: Send + Sync {
    /// Parent pid of `pid`, or `None` when unknown or when `pid` is gone.
    fn parent(&self, pid: u32) -> Option<u32>;
}

/// `/proc/<pid>/status`-backed tree. Only Linux has `/proc`; elsewhere every
/// lookup yields `None`, so only direct pid matches associate.
#[derive(Clone, Debug)]
pub struct ProcTree {
    root: PathBuf,
}

impl Default for ProcTree {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcTree {
    pub fn new() -> Self {
        Self::with_root(Path::new("/proc"))
    }

    /// A tree rooted somewhere else (tests write synthetic `status` files).
    pub fn with_root(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }
}

impl ProcessTree for ProcTree {
    fn parent(&self, pid: u32) -> Option<u32> {
        let status =
            std::fs::read_to_string(self.root.join(pid.to_string()).join("status")).ok()?;
        parse_ppid(&status)
    }
}

/// Extracts `PPid:` from a `/proc/<pid>/status` body.
pub fn parse_ppid(status: &str) -> Option<u32> {
    status
        .lines()
        .find_map(|line| line.strip_prefix("PPid:"))
        .and_then(|rest| rest.trim().parse().ok())
}

/// `pid` followed by its ancestors, at most [`MAX_HOPS`] parents, stopping at
/// pid 0 or a self-loop.
pub fn ancestors(tree: &dyn ProcessTree, pid: u32) -> impl Iterator<Item = u32> + '_ {
    let mut current = Some(pid);
    let mut hops = 0usize;
    std::iter::from_fn(move || {
        let pid = current?;
        if hops > MAX_HOPS {
            return None;
        }
        hops += 1;
        current = tree
            .parent(pid)
            .filter(|&parent| parent != 0 && parent != pid);
        Some(pid)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::FakeProcessTree;

    #[test]
    fn parses_ppid_from_status() {
        let status = "Name:\tferrum-terminal\nUmask:\t0022\nState:\tS (sleeping)\nTgid:\t51\nNgid:\t0\nPid:\t51\nPPid:\t49\n";
        assert_eq!(parse_ppid(status), Some(49));
        assert_eq!(parse_ppid("Name: x\n"), None);
    }

    #[test]
    fn proc_tree_reads_synthetic_status_files() {
        let root = std::env::temp_dir().join(format!("ferrix-appd-proc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("51")).unwrap();
        std::fs::write(root.join("51/status"), "Pid:\t51\nPPid:\t49\n").unwrap();
        let tree = ProcTree::with_root(&root);
        assert_eq!(tree.parent(51), Some(49));
        assert_eq!(tree.parent(49), None);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn ancestors_walk_up_and_stop() {
        let tree = FakeProcessTree::default();
        tree.set_parent(51, 50);
        tree.set_parent(50, 49);
        tree.set_parent(49, 1);
        tree.set_parent(1, 0);
        let chain: Vec<u32> = ancestors(&tree, 51).collect();
        assert_eq!(chain, [51, 50, 49, 1]);

        // A loop or a very deep chain is bounded.
        let deep = FakeProcessTree::default();
        for pid in 2..200u32 {
            deep.set_parent(pid, pid - 1);
        }
        assert_eq!(ancestors(&deep, 199).count(), MAX_HOPS + 1);
        let looped = FakeProcessTree::default();
        looped.set_parent(7, 7);
        assert_eq!(ancestors(&looped, 7).collect::<Vec<_>>(), [7]);
    }
}
