//! Installed APKs as Ferrix apps (ADR-008, "Installed APKs").
//!
//! The Android side reports its launchable packages (`waydroid app list`);
//! this module turns each into a public [`AppDescriptor`] with
//! [`AppOrigin::Installed`], a stable id derived from the package
//! ([`installed_app_id`]), Android's label, a letter glyph, and the steel
//! tone of the first-party utility tiles. Android's own system surfaces
//! ([`HIDDEN_PACKAGES`]) never become apps.

use ferrix_app_api::{
    AppDescriptor, AppId, AppOrigin, AppRole, INSTALLED_APP_PREFIX, IconRef, IconTone, RuntimeKind,
    SCHEMA_VERSION, TrustLevel,
};

use crate::config::validate_package;
use crate::host::InstalledPackage;

/// Packages that are Android's own shell, never Ferrix apps: launchers
/// (disabled by provisioning, ADR-008 §4), the setup wizards, System UI,
/// and Settings (its `FallbackHome` activity is Android's home once the
/// launcher is disabled, so a Settings task window is usually that empty
/// home screen; Ferrix's own Settings owns device configuration).
pub const HIDDEN_PACKAGES: &[&str] = &[
    "com.android.launcher3",
    "org.lineageos.trebuchet",
    "com.android.settings",
    "com.android.systemui",
    "org.lineageos.setupwizard",
    "com.google.android.setupwizard",
    "com.android.provision",
];

/// Longest label shown for an installed app (characters).
const MAX_LABEL: usize = 40;

/// Whether `package` is one of Android's own shell surfaces.
pub fn is_hidden_package(package: &str) -> bool {
    HIDDEN_PACKAGES.contains(&package)
}

/// The Ferrix id of an installed package, in the reserved
/// [`INSTALLED_APP_PREFIX`]:
///
/// - `apk.<package>` when the package is already a valid id (lowercase
///   ASCII letters and digits, the common case: `org.lineageos.jelly` ->
///   `apk.org.lineageos.jelly`);
/// - otherwise `apk.<readable>.x-<hash>`, where `<readable>` is the package
///   lowercased with `_` turned into `-` (segments that would be invalid
///   become `x`) and `<hash>` is the 64-bit FNV-1a hash of the exact package
///   in hex. Only this form contains a `-`, so the two forms never collide;
///   distinct packages that differ only in case or `_` differ in the hash.
///
/// `None` for a string that is not an Android package name.
pub fn installed_app_id(package: &str) -> Option<AppId> {
    validate_package(package).ok()?;
    let direct = format!("{INSTALLED_APP_PREFIX}.{package}");
    let clean = package.split('.').all(|s| {
        s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    });
    if clean && let Ok(id) = AppId::new(direct) {
        return Some(id);
    }
    let hash = format!("x-{:016x}", fnv1a64(package.as_bytes()));
    let readable: Vec<String> = package
        .split('.')
        .map(|segment| {
            let lowered: String = segment
                .chars()
                .map(|c| {
                    if c == '_' {
                        '-'
                    } else {
                        c.to_ascii_lowercase()
                    }
                })
                .collect();
            let trimmed = lowered.trim_start_matches('-');
            if trimmed.is_empty() {
                "x".to_owned()
            } else {
                trimmed.to_owned()
            }
        })
        .collect();
    // Keep as many leading segments as fit the id length limit.
    let budget = AppId::MAX_LEN - INSTALLED_APP_PREFIX.len() - hash.len() - 2;
    let mut middle = String::new();
    for segment in readable {
        let extra = segment.len() + usize::from(!middle.is_empty());
        if middle.len() + extra > budget {
            break;
        }
        if !middle.is_empty() {
            middle.push('.');
        }
        middle.push_str(&segment);
    }
    let text = if middle.is_empty() {
        format!("{INSTALLED_APP_PREFIX}.{hash}")
    } else {
        format!("{INSTALLED_APP_PREFIX}.{middle}.{hash}")
    };
    AppId::new(text).ok()
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

/// Android's label made safe for the launcher: control characters dropped,
/// whitespace collapsed, at most [`MAX_LABEL`] characters; the last package
/// segment when nothing is left.
pub fn display_label(name: &str, package: &str) -> String {
    let cleaned: String = name
        .split_whitespace()
        .map(|word| word.chars().filter(|c| !c.is_control()).collect::<String>())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let label: String = cleaned.chars().take(MAX_LABEL).collect();
    if label.is_empty() {
        package.rsplit('.').next().unwrap_or(package).to_owned()
    } else {
        label
    }
}

/// The installed app's descriptor, or `None` for a hidden, non-launchable,
/// or malformed package.
pub fn installed_descriptor(entry: &InstalledPackage) -> Option<AppDescriptor> {
    if is_hidden_package(&entry.package) || !entry.is_launchable() {
        return None;
    }
    let id = installed_app_id(&entry.package)?;
    let name = display_label(&entry.name, &entry.package);
    let glyph = name
        .chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().collect::<String>())
        .filter(|g| g.chars().count() <= 3)
        .unwrap_or_else(|| "A".to_owned());
    // Steel, like the first-party utility tiles: the rust accent belongs to
    // the Terminal alone and the status colours mean status
    // (`docs/design.md`), so installed apps differ by their letter, not by
    // colour.
    let tone = IconTone::Blue;
    let descriptor = AppDescriptor {
        schema_version: SCHEMA_VERSION,
        id,
        runtime: RuntimeKind::Android,
        name,
        icon: IconRef { glyph, tone },
        launchable: true,
        autostart: false,
        role: AppRole::Application,
        trust: TrustLevel::Untrusted,
        capabilities: Default::default(),
        intents: Default::default(),
        origin: AppOrigin::Installed,
    };
    descriptor.validate().ok()?;
    Some(descriptor)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(package: &str, name: &str, launchable: bool) -> InstalledPackage {
        InstalledPackage {
            package: package.to_owned(),
            name: name.to_owned(),
            categories: if launchable {
                vec!["android.intent.category.LAUNCHER".to_owned()]
            } else {
                vec![]
            },
        }
    }

    #[test]
    fn plain_packages_map_to_their_own_name_under_apk() {
        assert_eq!(
            installed_app_id("org.lineageos.jelly").unwrap().as_str(),
            "apk.org.lineageos.jelly"
        );
        assert_eq!(
            installed_app_id("com.example2.app").unwrap().as_str(),
            "apk.com.example2.app"
        );
    }

    #[test]
    fn packages_with_case_or_underscores_get_a_hashed_stable_id() {
        let a = installed_app_id("com.Example.My_App").unwrap();
        let b = installed_app_id("com.example.my_app").unwrap();
        let c = installed_app_id("com.example.my-app");
        assert!(a.as_str().starts_with("apk.com.example.my-app.x-"), "{a}");
        assert!(b.as_str().starts_with("apk.com.example.my-app.x-"), "{b}");
        assert_ne!(a, b, "case differences stay distinct");
        assert_eq!(c, None, "not an Android package name");
        // Stable across calls.
        assert_eq!(installed_app_id("com.Example.My_App").unwrap(), a);
        // Never equal to the plain form of another package.
        assert_ne!(
            installed_app_id("com.example.myapp").unwrap().as_str(),
            a.as_str()
        );
        // `_x` segments stay valid.
        let underscore = installed_app_id("_x.y_").unwrap();
        assert!(
            underscore.as_str().starts_with("apk.x.y-.x-"),
            "{underscore}"
        );
    }

    #[test]
    fn long_packages_fit_the_id_limit() {
        let long = format!("org.{}.app", "a".repeat(140));
        let id = installed_app_id(&long).unwrap();
        assert!(id.as_str().len() <= AppId::MAX_LEN);
        assert!(id.as_str().starts_with("apk."));
        let long_upper = format!("org.{}.App", "a".repeat(140));
        let id = installed_app_id(&long_upper).unwrap();
        assert!(id.as_str().len() <= AppId::MAX_LEN, "{id}");
    }

    #[test]
    fn descriptors_are_untrusted_installed_apps_with_android_labels() {
        let d =
            installed_descriptor(&entry("org.example.notes", " My\tNotes\u{7} ", true)).unwrap();
        assert_eq!(d.id.as_str(), "apk.org.example.notes");
        assert_eq!(d.name, "My Notes");
        assert_eq!(d.icon.glyph, "M");
        assert_eq!(d.origin, AppOrigin::Installed);
        assert_eq!(d.trust, TrustLevel::Untrusted);
        assert!(d.capabilities.is_empty());
        assert_eq!(d.runtime, RuntimeKind::Android);
        assert_eq!(
            d.icon.tone,
            IconTone::Blue,
            "never the Terminal's rust accent"
        );
        let unnamed = installed_descriptor(&entry("org.example.notes", "  ", true)).unwrap();
        assert_eq!(unnamed.name, "notes");
        let long =
            installed_descriptor(&entry("org.example.notes", &"x".repeat(90), true)).unwrap();
        assert_eq!(long.name.chars().count(), 40);
    }

    #[test]
    fn android_shell_surfaces_and_non_launchable_packages_are_not_apps() {
        for hidden in HIDDEN_PACKAGES {
            assert_eq!(installed_descriptor(&entry(hidden, "System", true)), None);
        }
        assert_eq!(
            installed_descriptor(&entry("org.example.service", "Svc", false)),
            None
        );
        assert_eq!(installed_descriptor(&entry("bad", "Bad", true)), None);
    }
}
