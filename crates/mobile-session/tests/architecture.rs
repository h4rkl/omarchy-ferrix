//! Architecture guard: product code depends on `hardware-api` (and this
//! session layer), never on a target adapter. `just arch-check` runs this.

use std::path::Path;

const FORBIDDEN: &str = "hardware-emulator";

/// Crates that must not name an adapter implementation.
const PRODUCT_CRATES: &[&str] = &[
    "hardware-api",
    "target-profile",
    "mobile-core",
    "mobile-basic-screen",
    "mobile-shell",
];

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
fn hardware_api_has_no_target_conditionals() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../hardware-api/src");
    for entry in std::fs::read_dir(&src).expect("hardware-api/src") {
        let path = entry.unwrap().path();
        // Code only: doc comments may explain what the crate does *not* know.
        let code: String = std::fs::read_to_string(&path)
            .unwrap()
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .map(|l| l.to_lowercase())
            .collect::<Vec<_>>()
            .join("\n");
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
