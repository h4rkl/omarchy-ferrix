//! Runtime-private launch configuration parsed from the descriptor's `launch`
//! object. `ferrix-appd` treats that object as opaque; only this adapter reads
//! it, so the Android identifiers never leak into the public protocol.

use serde::{Deserialize, Serialize};

/// `launch` object of an `android` descriptor.
///
/// ```json
/// {
///   "package": "com.brave.browser",
///   "activity": null,
///   "apk": { "pins": "tools/android/brave.pins", "path": null }
/// }
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AndroidLaunch {
    /// Android package name (`[a-zA-Z_][a-zA-Z0-9_]*(\.[a-zA-Z_][a-zA-Z0-9_]*)+`).
    pub package: String,
    /// Fully qualified activity (or `.Relative` shorthand). `None` launches
    /// the package's default launcher activity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity: Option<String>,
    /// Where the APK comes from. Launching never downloads; installation is
    /// a separate, explicit step driven by the checked-in fetch/verify
    /// scripts named by `pins`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub apk: Option<ApkSource>,
}

/// APK acquisition metadata (informational for the runtime; verification is
/// the job of `tools/android/brave-verify.sh` against the pins file).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApkSource {
    /// Repository-relative pins file (tag, asset, sha256, signer fingerprint).
    pub pins: String,
    /// Optional local path of an already verified APK.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

impl AndroidLaunch {
    /// Validates the identifiers; the shape itself is validated by serde.
    pub fn validate(&self) -> Result<(), String> {
        validate_package(&self.package)?;
        if let Some(activity) = &self.activity {
            validate_activity(activity)?;
        }
        if let Some(apk) = &self.apk
            && apk.pins.trim().is_empty()
        {
            return Err("apk.pins must not be empty".to_owned());
        }
        Ok(())
    }

    /// `package/activity` as `am start -n` expects it, or `None` for the
    /// default launcher activity.
    pub fn component(&self) -> Option<String> {
        self.activity
            .as_ref()
            .map(|activity| format!("{}/{}", self.package, activity))
    }
}

fn is_identifier(segment: &str) -> bool {
    let mut chars = segment.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Android package names: at least two dot-separated Java identifiers made
/// of ASCII letters, digits, and underscores, never starting with a digit.
pub fn validate_package(package: &str) -> Result<(), String> {
    if package.len() > 255 {
        return Err("package name longer than 255 bytes".to_owned());
    }
    let segments: Vec<&str> = package.split('.').collect();
    if segments.len() < 2 {
        return Err(format!(
            "package {package:?} must contain at least two dot-separated segments"
        ));
    }
    if let Some(bad) = segments.iter().find(|s| !is_identifier(s)) {
        return Err(format!(
            "package {package:?} has an invalid segment {bad:?} (allowed: [a-zA-Z_][a-zA-Z0-9_]*)"
        ));
    }
    Ok(())
}

/// Activity class names: either fully qualified (same grammar as a package,
/// one or more segments) or the `.Relative` shorthand resolved against the
/// package by Android.
pub fn validate_activity(activity: &str) -> Result<(), String> {
    if activity.len() > 255 {
        return Err("activity name longer than 255 bytes".to_owned());
    }
    let body = activity.strip_prefix('.').unwrap_or(activity);
    if body.is_empty() {
        return Err("activity name must not be empty".to_owned());
    }
    if let Some(bad) = body.split('.').find(|s| !is_identifier(s)) {
        return Err(format!(
            "activity {activity:?} has an invalid segment {bad:?} (allowed: [a-zA-Z_][a-zA-Z0-9_]*)"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_packages_pass() {
        for ok in ["com.brave.browser", "org.example_1.App2", "a.b", "_x.y_"] {
            assert_eq!(validate_package(ok), Ok(()), "{ok}");
        }
    }

    #[test]
    fn invalid_packages_fail() {
        for bad in [
            "",
            "brave",
            "com..brave",
            ".com.brave",
            "com.brave.",
            "com.1brave",
            "com.brave browser",
            "com.brave;rm",
            "com.br-ave",
            "com/brave",
            "com.bräve",
        ] {
            assert!(validate_package(bad).is_err(), "{bad:?} should fail");
        }
    }

    #[test]
    fn activities_accept_qualified_and_relative_names() {
        assert_eq!(
            validate_activity("com.google.android.apps.chrome.Main"),
            Ok(())
        );
        assert_eq!(validate_activity(".Main"), Ok(()));
        assert_eq!(validate_activity("Main"), Ok(()));
        for bad in ["", ".", "..Main", "Main/Other", "a b", "1Main"] {
            assert!(validate_activity(bad).is_err(), "{bad:?} should fail");
        }
    }

    #[test]
    fn launch_config_parses_and_rejects_unknown_fields() {
        let launch: AndroidLaunch = serde_json::from_str(
            r#"{"package":"com.brave.browser","activity":null,"apk":{"pins":"tools/android/brave.pins"}}"#,
        )
        .unwrap();
        assert_eq!(launch.package, "com.brave.browser");
        assert_eq!(launch.activity, None);
        assert_eq!(launch.component(), None);
        assert_eq!(
            launch.apk.as_ref().unwrap().pins,
            "tools/android/brave.pins"
        );
        assert_eq!(launch.validate(), Ok(()));

        let with_activity: AndroidLaunch = serde_json::from_str(
            r#"{"package":"com.brave.browser","activity":"com.google.android.apps.chrome.Main"}"#,
        )
        .unwrap();
        assert_eq!(
            with_activity.component().as_deref(),
            Some("com.brave.browser/com.google.android.apps.chrome.Main")
        );

        assert!(
            serde_json::from_str::<AndroidLaunch>(
                r#"{"package":"com.brave.browser","exec":["x"]}"#
            )
            .is_err()
        );
        let bad: AndroidLaunch = serde_json::from_str(r#"{"package":"brave"}"#).unwrap();
        assert!(bad.validate().is_err());
    }
}
