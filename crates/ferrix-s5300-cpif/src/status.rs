//! Read-only observation of the S5300 platform driver's sysfs state.
//!
//! The pinned kernel creates a `samsung,exynos-cp` platform device and exports
//! its current controller state as the `modem_state` sysfs attribute. This
//! module deliberately does not open an IPC/boot character device: the
//! driver's `open(2)` path can initialize CP communication, and its ioctls
//! include power, reset, image-load, and crash operations.

use std::fs;
use std::path::{Path, PathBuf};

use crate::Error;

/// The Linux platform-bus root used by the S5300 driver on a normal guest.
pub const DEFAULT_PLATFORM_SYSFS_ROOT: &str = "/sys/bus/platform/devices";
/// Exact compatible string from the pinned tegu device tree.
pub const CPIF_COMPATIBLE: &str = "samsung,exynos-cp";

/// State strings exported by `modem_state_show()` in the pinned driver.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModemState {
    Offline,
    CrashReset,
    CrashExit,
    Booting,
    Online,
    NvRebuilding,
    LoaderDone,
    SimAttach,
    SimDetach,
    CrashWatchdog,
    Reset,
    /// Preserve a future kernel state without treating it as healthy.
    Unknown(String),
}

impl ModemState {
    pub fn parse(value: &str) -> Self {
        match value.trim() {
            "OFFLINE" => Self::Offline,
            "CRASH_RESET" => Self::CrashReset,
            "CRASH_EXIT" => Self::CrashExit,
            "BOOTING" => Self::Booting,
            "ONLINE" => Self::Online,
            "NV_REBUILDING" => Self::NvRebuilding,
            "LOADER_DONE" => Self::LoaderDone,
            "SIM_ATTACH" => Self::SimAttach,
            "SIM_DETACH" => Self::SimDetach,
            "WDT_RESET" => Self::CrashWatchdog,
            "RESET" => Self::Reset,
            other => Self::Unknown(other.to_owned()),
        }
    }

    pub const fn is_online(&self) -> bool {
        matches!(self, Self::Online)
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Offline => "OFFLINE",
            Self::CrashReset => "CRASH_RESET",
            Self::CrashExit => "CRASH_EXIT",
            Self::Booting => "BOOTING",
            Self::Online => "ONLINE",
            Self::NvRebuilding => "NV_REBUILDING",
            Self::LoaderDone => "LOADER_DONE",
            Self::SimAttach => "SIM_ATTACH",
            Self::SimDetach => "SIM_DETACH",
            Self::CrashWatchdog => "WDT_RESET",
            Self::Reset => "RESET",
            Self::Unknown(value) => value,
        }
    }
}

/// Read-only state for one exact S5300 platform device.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CpifStatus {
    platform_path: PathBuf,
    driver_bound: bool,
    modem_state: Option<ModemState>,
}

impl CpifStatus {
    pub fn platform_path(&self) -> &Path {
        &self.platform_path
    }

    pub const fn driver_bound(&self) -> bool {
        self.driver_bound
    }

    pub fn modem_state(&self) -> Option<&ModemState> {
        self.modem_state.as_ref()
    }

    /// A usable CPIF controller requires both a bound driver and an explicit
    /// `ONLINE` state. Unknown or transitional states remain not-ready.
    pub fn is_online(&self) -> bool {
        self.driver_bound && self.modem_state.as_ref().is_some_and(ModemState::is_online)
    }
}

/// Read the first exact `samsung,exynos-cp` platform node beneath the default
/// Linux sysfs root. This operation reads only sysfs files and symlink
/// metadata; it never opens a CPIF character device.
pub fn discover() -> Result<Option<CpifStatus>, Error> {
    discover_at(Path::new(DEFAULT_PLATFORM_SYSFS_ROOT))
}

/// Fixture-friendly form of [`discover`] with an explicit sysfs root.
pub fn discover_at(root: &Path) -> Result<Option<CpifStatus>, Error> {
    let entries = fs::read_dir(root).map_err(|error| Error::Io {
        operation: "read S5300 platform sysfs",
        reason: error.to_string(),
    })?;
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| Error::Io {
            operation: "enumerate S5300 platform sysfs",
            reason: error.to_string(),
        })?;
        paths.push(entry.path());
    }
    paths.sort();

    for path in paths {
        let compatible_path = path.join("of_node/compatible");
        let Ok(compatible) = fs::read(&compatible_path) else {
            continue;
        };
        if !compatible
            .split(|byte| *byte == 0)
            .any(|entry| entry == CPIF_COMPATIBLE.as_bytes())
        {
            continue;
        }

        let driver_bound = path.join("driver").symlink_metadata().is_ok();
        let state_path = path.join("modem_state");
        let modem_state = match fs::read_to_string(&state_path) {
            Ok(state) => Some(ModemState::parse(&state)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(Error::Io {
                    operation: "read S5300 modem_state sysfs",
                    reason: error.to_string(),
                });
            }
        };
        return Ok(Some(CpifStatus {
            platform_path: path,
            driver_bound,
            modem_state,
        }));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn fixture_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "ferrix-s5300-status-{label}-{}",
            std::process::id()
        ))
    }

    #[test]
    fn parses_pinned_kernel_states_without_marking_transitions_online() {
        assert!(ModemState::parse("ONLINE").is_online());
        assert!(!ModemState::parse("BOOTING").is_online());
        assert!(!ModemState::parse("WDT_RESET").is_online());
        assert_eq!(ModemState::parse("future-state").as_str(), "future-state");
    }

    #[test]
    fn discovers_only_the_exact_compatible_platform_node() {
        let root = fixture_root("exact");
        let other = root.join("other");
        let cpif = root.join("cpif.0");
        fs::create_dir_all(other.join("of_node")).unwrap();
        fs::create_dir_all(cpif.join("of_node")).unwrap();
        fs::write(other.join("of_node/compatible"), b"samsung,other-cp\0").unwrap();
        fs::write(
            cpif.join("of_node/compatible"),
            b"google,gs101-cp-thermal\0samsung,exynos-cp\0",
        )
        .unwrap();
        fs::write(cpif.join("modem_state"), "ONLINE\n").unwrap();
        // A regular file is enough to model the driver's sysfs symlink for
        // this read-only fixture; no character-device path is touched.
        fs::write(cpif.join("driver"), b"s5300").unwrap();

        let status = discover_at(&root).unwrap().unwrap();
        assert_eq!(status.platform_path(), cpif.as_path());
        assert!(status.driver_bound());
        assert_eq!(status.modem_state(), Some(&ModemState::Online));
        assert!(status.is_online());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_driver_or_state_never_reports_online() {
        let root = fixture_root("not-ready");
        let cpif = root.join("cpif.0");
        fs::create_dir_all(cpif.join("of_node")).unwrap();
        fs::write(cpif.join("of_node/compatible"), b"samsung,exynos-cp\0").unwrap();

        let status = discover_at(&root).unwrap().unwrap();
        assert!(!status.driver_bound());
        assert_eq!(status.modem_state(), None);
        assert!(!status.is_online());
        fs::remove_dir_all(root).unwrap();
    }
}
