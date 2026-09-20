//! Platform socket names and the capability → socket mapping (ADR-004 §4).
//!
//! Every daemon socket lives in one directory (`/run/ferrix`, overridable
//! with `FERRIX_RUN_DIR`). The native runtime turns an application's
//! effective grants into the list of socket files that stay visible in its
//! private view of that directory. The names are defined here and nowhere
//! else in this crate.

use std::collections::BTreeSet;
use std::path::PathBuf;

use ferrix_app_api::AppCapability;

/// Default platform socket directory inside the guest.
pub const RUN_DIR: &str = "/run/ferrix";
/// Environment variable overriding [`RUN_DIR`] (nested host runs, tests).
pub const RUN_DIR_ENV: &str = "FERRIX_RUN_DIR";
/// The system runtime directory that [`crate::RunDirMode::PrivateRun`]
/// replaces.
pub const RUN_ROOT: &str = "/run";
/// Per-user runtime directories (`XDG_RUNTIME_DIR`, the Wayland socket);
/// bound back under [`crate::RunDirMode::PrivateRun`].
pub const USER_RUN_DIR: &str = "/run/user";

/// `ferrix-appd` socket file; always visible (needed for `YieldForeground`).
pub const APPD_SOCKET_FILE: &str = "ferrix-appd.sock";
/// `ferrix-core` socket file (`core-state`, `core-control`).
pub const CORE_SOCKET_FILE: &str = "core.sock";
/// `ferrix-telephonyd` socket file (`telephony-*`).
pub const TELEPHONYD_SOCKET_FILE: &str = "telephonyd.sock";
/// `ferrix-networkd` socket file (`network-*`).
pub const NETWORKD_SOCKET_FILE: &str = "networkd.sock";
/// `ferrix-contactsd` socket file (`contacts-*`).
pub const CONTACTSD_SOCKET_FILE: &str = "contactsd.sock";

/// The socket directory: `FERRIX_RUN_DIR` when set and non-empty, else
/// [`RUN_DIR`].
pub fn run_dir() -> PathBuf {
    match std::env::var_os(RUN_DIR_ENV) {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => PathBuf::from(RUN_DIR),
    }
}

/// The daemon socket a capability is served through, if any (`shell` and
/// `background` need none).
pub fn socket_file_for(capability: AppCapability) -> Option<&'static str> {
    match capability {
        AppCapability::CoreState | AppCapability::CoreControl => Some(CORE_SOCKET_FILE),
        AppCapability::TelephonyState
        | AppCapability::TelephonyCall
        | AppCapability::TelephonySms => Some(TELEPHONYD_SOCKET_FILE),
        AppCapability::NetworkState | AppCapability::NetworkControl => Some(NETWORKD_SOCKET_FILE),
        AppCapability::ContactsRead | AppCapability::ContactsWrite => Some(CONTACTSD_SOCKET_FILE),
        AppCapability::Shell | AppCapability::Background => None,
    }
}

/// The socket files an application with these effective grants may see:
/// the `ferrix-appd` socket first, then each daemon socket at most once, in
/// a stable order.
pub fn sockets_for_grants(granted: &BTreeSet<AppCapability>) -> Vec<String> {
    let mut names = vec![APPD_SOCKET_FILE.to_owned()];
    let daemons: BTreeSet<&'static str> =
        granted.iter().filter_map(|c| socket_file_for(*c)).collect();
    names.extend(daemons.into_iter().map(str::to_owned));
    names
}

/// A socket entry is a bare file name: no directory part, not `.`/`..`.
pub fn is_valid_socket_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains('/') && !name.contains('\0')
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn appd_socket_name_matches_the_public_default_path() {
        let default = Path::new(ferrix_app_api::DEFAULT_APPD_SOCKET);
        assert_eq!(
            default.file_name().and_then(|n| n.to_str()),
            Some(APPD_SOCKET_FILE)
        );
        assert_eq!(default.parent(), Some(Path::new(RUN_DIR)));
        assert!(Path::new(RUN_DIR).starts_with(RUN_ROOT));
        assert!(Path::new(USER_RUN_DIR).starts_with(RUN_ROOT));
    }

    #[test]
    fn grants_map_to_daemon_sockets_with_appd_always_present() {
        assert_eq!(sockets_for_grants(&BTreeSet::new()), vec![APPD_SOCKET_FILE]);
        let shell_only: BTreeSet<_> = [AppCapability::Shell, AppCapability::Background]
            .into_iter()
            .collect();
        assert_eq!(sockets_for_grants(&shell_only), vec![APPD_SOCKET_FILE]);

        let phone: BTreeSet<_> = [
            AppCapability::TelephonyState,
            AppCapability::TelephonyCall,
            AppCapability::ContactsRead,
            AppCapability::Background,
        ]
        .into_iter()
        .collect();
        assert_eq!(
            sockets_for_grants(&phone),
            vec![
                APPD_SOCKET_FILE,
                CONTACTSD_SOCKET_FILE,
                TELEPHONYD_SOCKET_FILE
            ]
        );

        let home: BTreeSet<_> = [
            AppCapability::CoreState,
            AppCapability::CoreControl,
            AppCapability::NetworkState,
        ]
        .into_iter()
        .collect();
        assert_eq!(
            sockets_for_grants(&home),
            vec![APPD_SOCKET_FILE, CORE_SOCKET_FILE, NETWORKD_SOCKET_FILE]
        );
        for capability in AppCapability::ALL {
            let name = socket_file_for(capability);
            assert!(name.is_none_or(is_valid_socket_name), "{capability}");
        }
    }

    #[test]
    fn socket_names_are_bare_file_names() {
        assert!(is_valid_socket_name("core.sock"));
        for bad in ["", ".", "..", "../etc/passwd", "a/b", "x\0"] {
            assert!(!is_valid_socket_name(bad), "{bad:?}");
        }
    }
}
