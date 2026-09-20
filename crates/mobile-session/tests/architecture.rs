//! Architecture guard: product code depends on `hardware-api` (and this
//! session layer), never on a target adapter; applications and shells depend
//! on `app-api`/`app-protocol`, never on an application runtime
//! implementation. `just arch-check` runs this.

use std::path::Path;

const FORBIDDEN: &str = "hardware-emulator";
const FORBIDDEN_RUNTIME: &str = "runtime-native";
const INTERNAL_RUNTIME_API: &str = "app-runtime-api";

/// Crates that must not name an adapter implementation.
const PRODUCT_CRATES: &[&str] = &[
    "hardware-api",
    "target-profile",
    "app-api",
    "app-protocol",
    "appd",
    "ferrum-ui",
    "ferrum-terminal",
    "mobile-core",
    "mobile-basic-screen",
    "mobile-shell",
];

/// Crates that must not depend on an application runtime implementation:
/// applications, shells, and the core. Only `appd`'s binary composes runtimes.
const RUNTIME_FREE_CRATES: &[&str] = &[
    "app-api",
    "app-protocol",
    "ferrum-ui",
    "ferrum-terminal",
    "mobile-core",
    "mobile-basic-screen",
    "mobile-shell",
];

/// Applications and the compositor talk to `appd` over its protocol; they
/// never link the service itself.
const APPD_FREE_CRATES: &[&str] = &[
    "ferrum-ui",
    "ferrum-terminal",
    "mobile-basic-screen",
    "mobile-shell",
    "mobile-core",
];

fn manifest_names_dependency(manifest: &Path, needle: &str) -> bool {
    std::fs::read_to_string(manifest).is_ok_and(|text| {
        text.lines()
            .map(str::trim)
            .filter(|l| !l.starts_with('#'))
            .any(|l| l.contains(needle))
    })
}

#[test]
fn product_crates_do_not_depend_on_the_emulator() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut offenders = Vec::new();
    for name in PRODUCT_CRATES {
        let manifest = crates.join(name).join("Cargo.toml");
        let Ok(text) = std::fs::read_to_string(&manifest) else {
            continue; // crate does not exist (yet)
        };
        if text
            .lines()
            .map(str::trim)
            .filter(|l| !l.starts_with('#'))
            .any(|l| l.contains(FORBIDDEN))
        {
            offenders.push(manifest.display().to_string());
        }
    }
    assert!(
        offenders.is_empty(),
        "product crates must depend on hardware-api, not {FORBIDDEN}: {offenders:?}"
    );
}

#[test]
fn product_crates_do_not_depend_on_an_application_runtime() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let offenders: Vec<String> = RUNTIME_FREE_CRATES
        .iter()
        .map(|name| crates.join(name).join("Cargo.toml"))
        .filter(|manifest| {
            manifest_names_dependency(manifest, FORBIDDEN_RUNTIME)
                || manifest_names_dependency(manifest, INTERNAL_RUNTIME_API)
        })
        .map(|manifest| manifest.display().to_string())
        .collect();
    assert!(
        offenders.is_empty(),
        "applications, shells, and the core must depend on app-api/app-protocol, not runtime adapter crates: {offenders:?}"
    );
}

#[test]
fn appd_names_the_runtime_only_in_its_binary() {
    let appd = Path::new(env!("CARGO_MANIFEST_DIR")).join("../appd/src");
    let Ok(entries) = std::fs::read_dir(&appd) else {
        return; // crate does not exist (yet)
    };
    let mut offenders = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.file_name().and_then(|n| n.to_str()) == Some("main.rs") {
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) == Some("rs")
            && std::fs::read_to_string(&path).is_ok_and(|text| text.contains("runtime_native"))
        {
            offenders.push(path.display().to_string());
        }
    }
    assert!(
        offenders.is_empty(),
        "appd's library must work with any app_runtime_api::Runtime; only main.rs composes runtime-native: {offenders:?}"
    );
}

#[test]
fn applications_and_shells_do_not_link_appd() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let offenders: Vec<String> = APPD_FREE_CRATES
        .iter()
        .map(|name| crates.join(name).join("Cargo.toml"))
        .filter(|manifest| {
            manifest_names_dependency(manifest, "\"appd\"")
                || manifest_names_dependency(manifest, "appd =")
        })
        .map(|manifest| manifest.display().to_string())
        .collect();
    assert!(
        offenders.is_empty(),
        "applications and shells use app-protocol, never the appd crate: {offenders:?}"
    );
}

#[test]
fn hardware_api_has_no_target_conditionals() {
    assert_no_target_conditionals("hardware-api");
}

#[test]
fn app_api_has_no_target_or_runtime_conditionals() {
    assert_no_target_conditionals("app-api");
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../app-api/src");
    for entry in std::fs::read_dir(&src).expect("app-api/src") {
        let path = entry.unwrap().path();
        let code = code_without_comments(&path);
        for needle in [
            "waydroid",
            "binder",
            "webkit",
            "wry",
            "unshare",
            "sandbox",
            "namespace",
            "no_new_privs",
            "exec",
        ] {
            assert!(
                !code.contains(needle),
                "{} mentions {needle} outside comments; the contract must not name an implementation",
                path.display()
            );
        }
    }
}

fn code_without_comments(path: &Path) -> String {
    // Code only: doc comments may explain what the crate does *not* know.
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .map(|l| l.to_lowercase())
        .collect::<Vec<_>>()
        .join("\n")
}

fn assert_no_target_conditionals(crate_name: &str) {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../{crate_name}/src"));
    for entry in std::fs::read_dir(&src).unwrap_or_else(|e| panic!("{crate_name}/src: {e}")) {
        let path = entry.unwrap().path();
        let code = code_without_comments(&path);
        for needle in ["qemu", "seeker", "cfg(target", "cfg(feature", "emulator"] {
            assert!(
                !code.contains(needle),
                "{} mentions {needle} outside comments",
                path.display()
            );
        }
    }
}

#[test]
fn target_profile_is_the_only_toml_schema_parser() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut offenders = Vec::new();
    for name in PRODUCT_CRATES {
        if *name == "target-profile" {
            continue;
        }
        let root = crates.join(name);
        let manifest = root.join("Cargo.toml");
        if std::fs::read_to_string(&manifest).is_ok_and(|text| {
            text.lines()
                .any(|line| line.trim_start().starts_with("toml ="))
        }) {
            offenders.push(manifest.display().to_string());
        }
        let src = root.join("src");
        if let Ok(entries) = std::fs::read_dir(src) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|value| value.to_str()) == Some("rs")
                    && std::fs::read_to_string(&path).is_ok_and(|text| text.contains("toml::"))
                {
                    offenders.push(path.display().to_string());
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "profile TOML must be parsed only by target-profile: {offenders:?}"
    );
}
