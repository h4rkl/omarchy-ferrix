//! Architecture guard: product code depends on `ferrix-hardware-api` (and this
//! session layer), never on a target adapter; applications and shells depend
//! on `ferrix-app-api`/`ferrix-app-protocol`, never on an application runtime
//! implementation. `just arch-check` runs this.

use std::path::{Path, PathBuf};

const FORBIDDEN: &str = "ferrix-hardware-emulator";
const FORBIDDEN_RUNTIME: &str = "ferrix-runtime-native";
const INTERNAL_RUNTIME_API: &str = "ferrix-app-runtime-api";

/// Crates that must not name an adapter implementation.
const PRODUCT_CRATES: &[&str] = &[
    "ferrix-hardware-api",
    "ferrix-target-profile",
    "ferrix-app-api",
    "ferrix-app-protocol",
    "ferrix-appd",
    "ferrix-ui",
    "ferrix-terminal",
    "ferrix-core",
    "ferrix-basic-screen",
    "ferrix-shell",
];

/// Crates that must not depend on an application runtime implementation:
/// applications, shells, and the core. Only `ferrix_appd`'s binary composes runtimes.
const RUNTIME_FREE_CRATES: &[&str] = &[
    "ferrix-app-api",
    "ferrix-app-protocol",
    "ferrix-ui",
    "ferrix-terminal",
    "ferrix-core",
    "ferrix-basic-screen",
    "ferrix-shell",
];

/// Applications and the compositor talk to `ferrix_appd` over its protocol; they
/// never link the service itself.
const APPD_FREE_CRATES: &[&str] = &[
    "ferrix-ui",
    "ferrix-terminal",
    "ferrix-basic-screen",
    "ferrix-shell",
    "ferrix-core",
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
        "product crates must depend on ferrix-hardware-api, not {FORBIDDEN}: {offenders:?}"
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
        "applications, shells, and the core must depend on ferrix-app-api/ferrix-app-protocol, not runtime adapter crates: {offenders:?}"
    );
}

#[test]
fn appd_names_the_runtime_only_in_its_binary() {
    let ferrix_appd = Path::new(env!("CARGO_MANIFEST_DIR")).join("../ferrix-appd/src");
    let Ok(entries) = std::fs::read_dir(&ferrix_appd) else {
        return; // crate does not exist (yet)
    };
    let mut offenders = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.file_name().and_then(|n| n.to_str()) == Some("main.rs") {
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) == Some("rs")
            && std::fs::read_to_string(&path)
                .is_ok_and(|text| text.contains("ferrix_runtime_native"))
        {
            offenders.push(path.display().to_string());
        }
    }
    assert!(
        offenders.is_empty(),
        "ferrix-appd's library must work with any ferrix_app_runtime_api::Runtime; only main.rs composes ferrix-runtime-native: {offenders:?}"
    );
}

#[test]
fn applications_and_shells_do_not_link_appd() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let offenders: Vec<String> = APPD_FREE_CRATES
        .iter()
        .map(|name| crates.join(name).join("Cargo.toml"))
        .filter(|manifest| {
            manifest_names_dependency(manifest, "\"ferrix-appd\"")
                || manifest_names_dependency(manifest, "ferrix-appd =")
        })
        .map(|manifest| manifest.display().to_string())
        .collect();
    assert!(
        offenders.is_empty(),
        "applications and shells use ferrix-app-protocol, never the ferrix-appd crate: {offenders:?}"
    );
}

#[test]
fn hardware_api_has_no_target_conditionals() {
    assert_no_target_conditionals("ferrix-hardware-api");
}

#[test]
fn app_api_has_no_target_or_runtime_conditionals() {
    assert_no_target_conditionals("ferrix-app-api");
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../ferrix-app-api/src");
    for entry in std::fs::read_dir(&src).expect("ferrix-app-api/src") {
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

/// Modules allowed to parse a TOML schema other than the target profile, as
/// `(crate, source file)`. `ferrix-ui`'s theme loader reads `colors.toml`;
/// its keyboard module reads the user's `keyboard.toml` and the embedded
/// letter-layout files (user preferences and UI data, never target facts).
const OTHER_TOML_SCHEMA_PARSERS: &[(&str, &str)] =
    &[("ferrix-ui", "theme.rs"), ("ferrix-ui", "keyboard.rs")];

#[test]
fn target_profile_is_the_only_toml_schema_parser() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut offenders = Vec::new();
    for name in PRODUCT_CRATES {
        if *name == "ferrix-target-profile" {
            continue;
        }
        let allowed_file = |file: &str| {
            OTHER_TOML_SCHEMA_PARSERS
                .iter()
                .any(|(krate, allowed)| krate == name && *allowed == file)
        };
        let root = crates.join(name);
        let manifest = root.join("Cargo.toml");
        let declares_toml = std::fs::read_to_string(&manifest).is_ok_and(|text| {
            text.lines()
                .any(|line| line.trim_start().starts_with("toml ="))
        });
        if declares_toml
            && !OTHER_TOML_SCHEMA_PARSERS
                .iter()
                .any(|(krate, _)| krate == name)
        {
            offenders.push(manifest.display().to_string());
        }
        let src = root.join("src");
        if let Ok(entries) = std::fs::read_dir(src) {
            for entry in entries.flatten() {
                let path = entry.path();
                let file = path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("");
                if path.extension().and_then(|value| value.to_str()) == Some("rs")
                    && !allowed_file(file)
                    && std::fs::read_to_string(&path).is_ok_and(|text| text.contains("toml::"))
                {
                    offenders.push(path.display().to_string());
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "profile TOML must be parsed only by ferrix-target-profile: {offenders:?}"
    );
}

// ---------------------------------------------------------------------------
// Mobile-connectivity programme guards (docs/adr/001..006)
// ---------------------------------------------------------------------------

/// Adapter implementations and internal adapter contracts that only a daemon
/// binary may compose.
const SERVICE_ADAPTER_CRATES: &[&str] = &[
    "ferrix-audio-wireplumber",
    "ferrix-bluetooth-bluez",
    "ferrix-power-upower",
    "ferrix-telephony-mock",
    "ferrix-telephony-mm",
    "ferrix-telephony-adapter-api",
    "ferrix-network-mock",
    "ferrix-network-nm",
    "ferrix-network-adapter-api",
    "ferrix-runtime-android",
];

/// Daemon crates that applications and shells talk to only over a protocol.
const SERVICE_DAEMON_CRATES: &[&str] =
    &["ferrix-telephonyd", "ferrix-networkd", "ferrix-contactsd"];

/// Applications, shells, the core, and public contract crates: none may name
/// a service adapter, a daemon crate, or a runtime implementation.
const CONNECTIVITY_PRODUCT_CRATES: &[&str] = &[
    "ferrix-phone",
    "ferrix-messages",
    "ferrix-contacts",
    "ferrix-settings",
    "ferrix-files",
    "ferrix-terminal",
    "ferrix-basic-screen",
    "ferrix-shell",
    "ferrix-core",
    "ferrix-ui",
    "ferrix-telephony-api",
    "ferrix-telephony-protocol",
    "ferrix-network-api",
    "ferrix-network-protocol",
    "ferrix-contacts-api",
    "ferrix-contacts-protocol",
    "ferrix-app-api",
    "ferrix-app-protocol",
];

/// Public contract crates that must stay vendor- and implementation-neutral.
const NEUTRAL_CONTRACT_CRATES: &[&str] = &[
    "ferrix-telephony-api",
    "ferrix-telephony-protocol",
    "ferrix-network-api",
    "ferrix-network-protocol",
    "ferrix-contacts-api",
    "ferrix-contacts-protocol",
];

/// Daemons whose library must work with any backend; only `main.rs` names one.
const DAEMON_COMPOSITION: &[(&str, &[&str])] = &[
    (
        "ferrix-telephonyd",
        &["ferrix_telephony_mock", "ferrix_telephony_mm"],
    ),
    (
        "ferrix-networkd",
        &["ferrix_network_mock", "ferrix_network_nm"],
    ),
    ("ferrix-appd", &["ferrix_runtime_android"]),
];

#[test]
fn product_and_contract_crates_do_not_depend_on_service_adapters_or_daemons() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut offenders = Vec::new();
    for name in CONNECTIVITY_PRODUCT_CRATES {
        let manifest = crates.join(name).join("Cargo.toml");
        if !manifest.is_file() {
            continue;
        }
        for forbidden in SERVICE_ADAPTER_CRATES.iter().chain(SERVICE_DAEMON_CRATES) {
            if manifest_names_dependency(&manifest, &format!("\"{forbidden}\""))
                || manifest_names_dependency(&manifest, &format!("{forbidden} ="))
            {
                offenders.push(format!("{} -> {forbidden}", manifest.display()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "apps, shells, the core, and contract crates use *-api/*-protocol only: {offenders:?}"
    );
}

#[test]
fn service_contract_crates_are_vendor_neutral() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    for name in NEUTRAL_CONTRACT_CRATES {
        let src = crates.join(name).join("src");
        let Ok(entries) = std::fs::read_dir(&src) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let code = code_without_comments(&path);
            for needle in [
                "qemu",
                "cfg(target",
                "cfg(feature",
                "emulator",
                "qmi",
                "mbim",
                "qrtr",
                "modemmanager",
                "networkmanager",
                "waydroid",
                "binder",
                "zbus",
                "dbus",
                "wpa_supplicant",
                "iwd",
            ] {
                assert!(
                    !code.contains(needle),
                    "{} mentions {needle} outside comments; contracts must not name an implementation",
                    path.display()
                );
            }
        }
        let manifest = crates.join(name).join("Cargo.toml");
        if manifest.is_file() {
            for needle in ["zbus", "dbus", "rustix", "libc"] {
                assert!(
                    !manifest_names_dependency(&manifest, needle),
                    "{} depends on {needle}; contract crates are pure types and a socket client",
                    manifest.display()
                );
            }
        }
    }
}

#[test]
fn daemons_name_their_backends_only_in_main() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut offenders = Vec::new();
    for (daemon, backends) in DAEMON_COMPOSITION {
        let src = crates.join(daemon).join("src");
        let Ok(entries) = std::fs::read_dir(&src) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.file_name().and_then(|n| n.to_str()) == Some("main.rs") {
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let code = code_without_comments(&path);
            for backend in *backends {
                if code.contains(backend) {
                    offenders.push(format!("{} names {backend}", path.display()));
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "daemon libraries work with any backend; only main.rs composes one: {offenders:?}"
    );
}

/// Evidence records must not carry personal data. This scans every crate's
/// Rust sources for record fields that would leak numbers or bodies.
#[test]
fn evidence_records_do_not_name_personal_fields() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut offenders = Vec::new();
    let forbidden_keys = [
        "\\\"number\\\":",
        "\\\"body\\\":",
        "\\\"imsi\\\":",
        "\\\"iccid\\\":",
        "\\\"imei\\\":",
        "\\\"bssid\\\":",
        "\\\"passphrase\\\":",
        "(\"number\",",
        "(\"body\",",
        "(\"imsi\",",
        "(\"iccid\",",
        "(\"imei\",",
        "(\"bssid\",",
        "(\"passphrase\",",
    ];
    for entry in std::fs::read_dir(&crates).expect("crates dir").flatten() {
        let src = entry.path().join("src");
        let Ok(files) = std::fs::read_dir(&src) else {
            continue;
        };
        let mut stack: Vec<PathBuf> = files.flatten().map(|e| e.path()).collect();
        while let Some(path) = stack.pop() {
            if path.is_dir() {
                if let Ok(more) = std::fs::read_dir(&path) {
                    stack.extend(more.flatten().map(|e| e.path()));
                }
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            for line in text.lines() {
                let trimmed = line.trim_start();
                if trimmed.starts_with("//") {
                    continue;
                }
                // Only lines that build an evidence record are checked.
                let is_record = trimmed.contains("\"evidence\"")
                    || trimmed.contains("emit(")
                    || trimmed.contains(".record(");
                if !is_record {
                    continue;
                }
                for key in forbidden_keys {
                    if trimmed.contains(key) {
                        offenders.push(format!("{}: {}", path.display(), trimmed));
                    }
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "evidence records carry ids and states only, never numbers, bodies, or radio identifiers: {offenders:?}"
    );
}
