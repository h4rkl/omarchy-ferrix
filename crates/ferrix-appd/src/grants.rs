//! Loading the grants policy (`ferrix_app_api::GrantsPolicy`).
//!
//! The policy is a checked-in file beside the descriptors
//! (`apps/grants.json`, installed to `/usr/share/ferrix/apps/grants.json`).
//! `ferrix_appd` reads it once at startup:
//!
//! - an explicit path (`--grants`, `FERRIX_APP_GRANTS`) must exist and parse;
//! - otherwise `<first app dir>/grants.json` is used when present;
//! - otherwise the policy is **empty** and every application has zero
//!   effective capabilities. That is recorded (`grants-missing`), never
//!   hidden.
//!
//! A file that exists but is invalid is a startup failure (`grants-invalid`).

use std::path::{Path, PathBuf};

use ferrix_app_api::{AppError, GRANTS_FILE_NAME, GrantsPolicy};

/// Where the effective policy came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GrantsSource {
    /// Parsed from this file.
    File(PathBuf),
    /// No file was configured or found; the policy is empty.
    Missing,
}

/// The path to read: the explicit one, else `<first app dir>/grants.json`
/// when that file exists.
pub fn resolve_grants_path(explicit: Option<&Path>, app_dirs: &[PathBuf]) -> Option<PathBuf> {
    if let Some(path) = explicit {
        return Some(path.to_path_buf());
    }
    let candidate = app_dirs.first()?.join(GRANTS_FILE_NAME);
    candidate.is_file().then_some(candidate)
}

/// Loads the policy. An explicit path that cannot be read is an error; an
/// absent default yields the empty policy with [`GrantsSource::Missing`].
pub fn load_grants(
    explicit: Option<&Path>,
    app_dirs: &[PathBuf],
) -> Result<(GrantsPolicy, GrantsSource), AppError> {
    let Some(path) = resolve_grants_path(explicit, app_dirs) else {
        return Ok((GrantsPolicy::empty(), GrantsSource::Missing));
    };
    let text = std::fs::read_to_string(&path).map_err(|e| AppError::InvalidGrants {
        reason: format!("cannot read {}: {e}", path.display()),
    })?;
    let policy = GrantsPolicy::from_json(&text).map_err(|error| match error {
        AppError::InvalidGrants { reason } => AppError::InvalidGrants {
            reason: format!("{}: {reason}", path.display()),
        },
        other => other,
    })?;
    Ok((policy, GrantsSource::File(path)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrix_app_api::{AppCapability, AppId};

    fn repo_apps_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps")
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ferrix-appd-grants-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The checked-in policy: every id and capability parses, and `shell` is
    /// granted to the Terminal only. Whether every granted id has a
    /// descriptor is deliberately not asserted (descriptors and grants may
    /// land separately); a grant for an unknown app grants nothing.
    #[test]
    fn repository_grants_are_valid_and_shell_is_terminal_only() {
        let (policy, source) = load_grants(None, &[repo_apps_dir()]).unwrap();
        assert_eq!(
            source,
            GrantsSource::File(repo_apps_dir().join(GRANTS_FILE_NAME))
        );
        assert!(!policy.is_empty());
        for (id, capabilities) in &policy.grants {
            assert!(AppId::new(id.as_str()).is_ok(), "{id}");
            for capability in capabilities {
                assert_eq!(
                    capability.name().parse::<AppCapability>().unwrap(),
                    *capability
                );
            }
            if id.as_str() != "org.ferrix.terminal" {
                assert!(
                    !capabilities.contains(&AppCapability::Shell),
                    "{id} must not be granted shell"
                );
            }
        }
        let terminal = AppId::new("org.ferrix.terminal").unwrap();
        assert!(policy.granted(&terminal).contains(&AppCapability::Shell));
        let home = AppId::new("org.ferrix.home").unwrap();
        assert!(policy.granted(&home).contains(&AppCapability::CoreControl));
        assert!(
            !policy
                .granted(&home)
                .contains(&AppCapability::TelephonyCall)
        );
    }

    #[test]
    fn missing_default_is_the_empty_policy_and_explicit_paths_are_strict() {
        let dir = temp_dir("missing");
        let (policy, source) = load_grants(None, std::slice::from_ref(&dir)).unwrap();
        assert_eq!(source, GrantsSource::Missing);
        assert!(policy.is_empty());
        assert_eq!(resolve_grants_path(None, &[]), None);

        let explicit = dir.join("nope.json");
        assert!(matches!(
            load_grants(Some(&explicit), std::slice::from_ref(&dir)),
            Err(AppError::InvalidGrants { .. })
        ));

        let invalid = dir.join(GRANTS_FILE_NAME);
        std::fs::write(
            &invalid,
            r#"{"schema_version":1,"grants":{"org.x":["nope"]}}"#,
        )
        .unwrap();
        match load_grants(None, std::slice::from_ref(&dir)) {
            Err(AppError::InvalidGrants { reason }) => {
                assert!(reason.contains("grants.json"), "{reason}");
            }
            other => panic!("unexpected {other:?}"),
        }

        std::fs::write(
            &invalid,
            r#"{"schema_version":1,"grants":{"org.x.y":["background"]}}"#,
        )
        .unwrap();
        let (policy, source) = load_grants(None, std::slice::from_ref(&dir)).unwrap();
        assert_eq!(source, GrantsSource::File(invalid));
        assert_eq!(policy.len(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
