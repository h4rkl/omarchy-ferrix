//! The application's filesystem view (issue #10): the pure parts.
//!
//! With `fs_view.mode = minimal` stage 2 of `ferrix-sandbox` assembles a new
//! root on a tmpfs and pivots into it. The root holds read-only binds of the
//! system directories ([`SYSTEM_DIRS`]), a minimal `/dev`, the sandbox's own
//! `/proc`, `/tmp` and `/run` views, a read-only `/sys`, the application's
//! writable data directory ([`app_data_dir`]), the read-only Ferrix
//! configuration directory, and the user's home folder **only** with the
//! `files-home` grant. Everything else on the host (other users' files,
//! `/var`, `/srv`, `/mnt`, the home folder itself) is absent.
//!
//! This module computes paths and parses `/proc/self/mountinfo`; it touches
//! nothing and is tested on any host.

use std::path::{Path, PathBuf};

/// System directories bound read-only into a minimal root when present.
/// A symlink (a merged-`/usr` layout's `/bin -> usr/bin`) is recreated as
/// the same symlink instead.
pub const SYSTEM_DIRS: &[&str] = &["/usr", "/etc", "/bin", "/sbin", "/lib", "/lib64", "/opt"];
/// Device nodes bound into the minimal `/dev`.
pub const DEV_NODES: &[&str] = &["null", "zero", "full", "random", "urandom", "tty"];
/// GPU render and display nodes, bound into `/dev` when the host has them
/// (Mesa; acceleration is optional, so their absence is not an error).
pub const DEV_DIRS: &[&str] = &["dri"];
/// Directories a home folder may never be, nor sit under: exposing one as
/// "home" would hand the application a writable system tree.
pub const RESERVED_DIRS: &[&str] = &[
    "/usr", "/etc", "/bin", "/sbin", "/lib", "/lib64", "/opt", "/proc", "/sys", "/dev", "/run",
    "/tmp", "/boot", "/var",
];

/// Name of the per-application data directory under the data root.
pub const DATA_SUBDIR: &str = "ferrix";
/// Name of the Ferrix configuration directory under the config root
/// (`keyboard.toml` and other shared, user-editable settings).
pub const CONFIG_SUBDIR: &str = "ferrix";

fn absolute(env: &impl Fn(&str) -> Option<String>, key: &str) -> Option<PathBuf> {
    env(key)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

/// `$HOME` when it is an absolute path.
pub fn home_dir(env: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    absolute(&env, "HOME")
}

/// The application's private, persistent data directory:
/// `$XDG_DATA_HOME/ferrix/<app-id>`, else `$HOME/.local/share/ferrix/<app-id>`.
/// `None` when neither is an absolute path (nothing is guessed).
pub fn app_data_dir(env: impl Fn(&str) -> Option<String>, app_id: &str) -> Option<PathBuf> {
    let root = absolute(&env, "XDG_DATA_HOME")
        .or_else(|| absolute(&env, "HOME").map(|home| home.join(".local/share")))?;
    Some(root.join(DATA_SUBDIR).join(app_id))
}

/// The shared Ferrix configuration directory, bound read-only:
/// `$XDG_CONFIG_HOME/ferrix`, else `$HOME/.config/ferrix`.
pub fn ferrix_config_dir(env: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    let root = absolute(&env, "XDG_CONFIG_HOME")
        .or_else(|| absolute(&env, "HOME").map(|home| home.join(".config")))?;
    Some(root.join(CONFIG_SUBDIR))
}

/// Whether `home` may be exposed as the user's home folder: absolute,
/// normalized (no `.`/`..`), not `/`, and neither a [`RESERVED_DIRS`] entry
/// nor inside one.
pub fn exposable_home(home: &Path) -> Result<(), String> {
    use std::path::Component;
    if !home.is_absolute() {
        return Err(format!("home {} is not absolute", home.display()));
    }
    if home
        .components()
        .any(|c| matches!(c, Component::CurDir | Component::ParentDir))
    {
        return Err(format!("home {} is not normalized", home.display()));
    }
    if home.parent().is_none() {
        return Err("refusing to expose / as the home folder".to_owned());
    }
    if let Some(reserved) = RESERVED_DIRS.iter().find(|dir| home.starts_with(dir)) {
        return Err(format!(
            "refusing to expose {} (under {reserved}) as the home folder",
            home.display()
        ));
    }
    Ok(())
}

/// `path` relative to `/`, for joining under a staging root.
pub fn under_root(root: &Path, path: &Path) -> PathBuf {
    root.join(path.strip_prefix("/").unwrap_or(path))
}

/// One line of `/proc/self/mountinfo`: the mount point and its per-mount
/// options (`rw,nosuid,nodev,relatime`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MountEntry {
    pub mount_point: PathBuf,
    pub options: Vec<String>,
}

impl MountEntry {
    pub fn has(&self, option: &str) -> bool {
        self.options.iter().any(|o| o == option)
    }
}

/// Decodes the octal escapes mountinfo uses for space, tab, newline and
/// backslash (`\040`).
fn unescape(field: &str) -> String {
    let bytes = field.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\'
            && i + 3 < bytes.len()
            && bytes[i + 1..i + 4]
                .iter()
                .all(|b| (b'0'..=b'7').contains(b))
        {
            let value = bytes[i + 1..i + 4]
                .iter()
                .fold(0u32, |acc, b| acc * 8 + u32::from(b - b'0'));
            out.push(u8::try_from(value).unwrap_or(b'?'));
            i += 4;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Mounts at or below `under`, in mountinfo order (parents before
/// children), from the text of `/proc/self/mountinfo`.
pub fn mounts_under(mountinfo: &str, under: &Path) -> Vec<MountEntry> {
    mountinfo
        .lines()
        .filter_map(|line| {
            let mut fields = line.split(' ');
            let mount_point = PathBuf::from(unescape(fields.nth(4)?));
            let options = fields.next()?.split(',').map(str::to_owned).collect();
            Some(MountEntry {
                mount_point,
                options,
            })
        })
        .filter(|entry| entry.mount_point.starts_with(under))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |key| {
            pairs
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| (*v).to_owned())
        }
    }

    #[test]
    fn data_dir_follows_xdg_then_home_and_never_guesses() {
        assert_eq!(
            app_data_dir(env(&[("HOME", "/home/user")]), "org.ferrix.files"),
            Some(PathBuf::from(
                "/home/user/.local/share/ferrix/org.ferrix.files"
            ))
        );
        assert_eq!(
            app_data_dir(
                env(&[("HOME", "/home/user"), ("XDG_DATA_HOME", "/data")]),
                "a.b"
            ),
            Some(PathBuf::from("/data/ferrix/a.b"))
        );
        // Relative or empty values are ignored, not resolved.
        assert_eq!(
            app_data_dir(env(&[("HOME", "home"), ("XDG_DATA_HOME", "")]), "a.b"),
            None
        );
        assert_eq!(app_data_dir(env(&[]), "a.b"), None);
        assert_eq!(
            ferrix_config_dir(env(&[("HOME", "/root")])),
            Some(PathBuf::from("/root/.config/ferrix"))
        );
        assert_eq!(
            ferrix_config_dir(env(&[("HOME", "/root"), ("XDG_CONFIG_HOME", "/cfg")])),
            Some(PathBuf::from("/cfg/ferrix"))
        );
        assert_eq!(
            home_dir(env(&[("HOME", "/home/user")])),
            Some(PathBuf::from("/home/user"))
        );
        assert_eq!(home_dir(env(&[("HOME", "~")])), None);
    }

    #[test]
    fn only_a_plain_user_folder_can_be_the_home() {
        assert!(exposable_home(Path::new("/home/user")).is_ok());
        assert!(exposable_home(Path::new("/root")).is_ok());
        for bad in [
            "/",
            "/usr",
            "/usr/local/home",
            "/etc",
            "/run/user/0",
            "/tmp/home",
            "/var/home/user",
            "home/user",
            "/home/../etc",
        ] {
            assert!(exposable_home(Path::new(bad)).is_err(), "{bad}");
        }
        // A prefix match is a path-component match, not a string match.
        assert!(exposable_home(Path::new("/usrdata/home")).is_ok());
        assert_eq!(
            under_root(Path::new("/tmp/r"), Path::new("/home/user")),
            PathBuf::from("/tmp/r/home/user")
        );
    }

    #[test]
    fn mountinfo_is_parsed_with_escapes_and_filtered_by_prefix() {
        let info = "\
22 1 0:21 / / rw,relatime shared:1 - overlay overlay rw
23 22 0:22 / /proc rw,nosuid,nodev,noexec,relatime - proc proc rw
24 22 0:23 / /etc rw,relatime - ext4 /dev/vda rw
25 24 0:24 /hosts /etc/hosts ro,nosuid,nodev,relatime - ext4 /dev/vda rw
26 22 0:25 / /media/my\\040disk rw,relatime - vfat /dev/sdb rw
27 22 0:26 / /etcetera rw - tmpfs tmpfs rw";
        let etc = mounts_under(info, Path::new("/etc"));
        assert_eq!(etc.len(), 2, "{etc:?}");
        assert_eq!(etc[0].mount_point, PathBuf::from("/etc"));
        assert_eq!(etc[1].mount_point, PathBuf::from("/etc/hosts"));
        assert!(etc[1].has("ro") && etc[1].has("nosuid") && !etc[1].has("noexec"));
        let media = mounts_under(info, Path::new("/media"));
        assert_eq!(media[0].mount_point, PathBuf::from("/media/my disk"));
        assert_eq!(mounts_under(info, Path::new("/")).len(), 6);
        assert!(mounts_under("garbage", Path::new("/")).is_empty());
    }
}
