//! Launcher tiles from the application catalog.
//!
//! The home surface never hard-codes the app list: tiles come from `ferrix_appd`'s
//! catalog when it is reachable, otherwise from the descriptor directory
//! (`FERRIX_APP_DIR`, the guest install dir, or the repository `apps/`).

use std::path::{Path, PathBuf};

use ferrix_app_api::{AppDescriptor, AppId, AppInstance, AppRole, IconRef, IconTone};
use ferrix_ui::theme;

/// One launcher entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tile {
    pub id: AppId,
    pub name: String,
    pub icon: IconRef,
    /// Whether tapping launches a process through `ferrix_appd`; otherwise the tile
    /// opens an illustrative in-process reference page.
    pub launchable: bool,
    /// A process exists for the app (starting, foreground, or background).
    pub running: bool,
    /// A launch was requested and no foreground/background state arrived yet.
    pub starting: bool,
}

impl Tile {
    pub fn from_descriptor(descriptor: &AppDescriptor) -> Self {
        Self {
            id: descriptor.id.clone(),
            name: descriptor.name.clone(),
            icon: descriptor.icon.clone(),
            launchable: descriptor.launchable,
            running: false,
            starting: false,
        }
    }

    /// Last id segment (`org.ferrix.settings` -> `settings`), the key for the
    /// reference pages and the drawn icons.
    pub fn kind(&self) -> &str {
        self.id.as_str().rsplit('.').next().unwrap_or("")
    }
}

/// Maps a descriptor tone onto the active Ferrix theme (`docs/design.md`):
/// the "blue" family is steel, the warm family is the rust accent, and the
/// remaining tones are the status colors.
pub fn tone_color(tone: IconTone) -> u32 {
    let t = theme::get();
    match tone {
        IconTone::Blue | IconTone::Cyan => t.steel,
        IconTone::Purple | IconTone::Orange => t.rust,
        IconTone::Green => t.ok,
        IconTone::Yellow => t.warn,
        IconTone::Red => t.err,
        IconTone::Neutral => t.fg_dim,
    }
}

/// Deterministic launcher order: launchable applications first (the home
/// surface itself last among them), then reference entries; each group by
/// name.
pub fn tiles_from_catalog(apps: &[AppDescriptor], instances: &[AppInstance]) -> Vec<Tile> {
    // The home surface never lists itself: it is the launcher, not a tile.
    let mut sorted: Vec<&AppDescriptor> = apps.iter().filter(|d| d.role != AppRole::Home).collect();
    sorted.sort_by(|a, b| {
        let group = |d: &AppDescriptor| if d.launchable { 0 } else { 1 };
        group(a)
            .cmp(&group(b))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.id.cmp(&b.id))
    });
    let mut tiles: Vec<Tile> = sorted.into_iter().map(Tile::from_descriptor).collect();
    for tile in &mut tiles {
        if let Some(instance) = instances.iter().find(|i| i.app == tile.id) {
            tile.running = instance.state.is_running();
            tile.starting = instance.state == ferrix_app_api::LifecycleState::Starting;
        }
    }
    tiles
}

/// Reads every `*.json` descriptor in `dir`. Invalid files are skipped with
/// their reason collected in `errors`; a missing directory is an error.
pub fn load_descriptor_dir(dir: &Path) -> Result<(Vec<AppDescriptor>, Vec<String>), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut apps = Vec::new();
    let mut errors = Vec::new();
    // The grants policy (ADR-004) lives beside the descriptors and is not
    // one; `ferrix-appd` reads it, the launcher skips it.
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("json"))
        .filter(|path| {
            path.file_name().and_then(|n| n.to_str()) != Some(ferrix_app_api::GRANTS_FILE_NAME)
        })
        .collect();
    paths.sort();
    for path in paths {
        match std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|text| AppDescriptor::from_json(&text).map_err(|e| e.to_string()))
        {
            Ok(descriptor) => {
                if apps.iter().any(|d: &AppDescriptor| d.id == descriptor.id) {
                    errors.push(format!(
                        "{}: duplicate id {}",
                        path.display(),
                        descriptor.id
                    ));
                } else {
                    apps.push(descriptor);
                }
            }
            Err(error) => errors.push(format!("{}: {error}", path.display())),
        }
    }
    Ok((apps, errors))
}

/// Descriptor directories in precedence order: `FERRIX_APP_DIR`, the guest
/// install dir when present, then the repository `apps/` (relative to the
/// working directory, plus the source-tree copy for `cargo test`).
pub fn descriptor_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = std::env::var_os("FERRIX_APP_DIR").filter(|v| !v.is_empty()) {
        dirs.push(PathBuf::from(dir));
    }
    let guest = Path::new(ferrix_app_api::GUEST_APP_DIR);
    if guest.is_dir() {
        dirs.push(guest.to_path_buf());
    }
    dirs.push(PathBuf::from(ferrix_app_api::REPO_APP_DIR));
    dirs.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps"));
    dirs
}

/// The first descriptor directory that loads. Returns the directory used and
/// its descriptors, or the reasons every candidate failed.
pub fn load_descriptor_tiles() -> Result<(PathBuf, Vec<Tile>), String> {
    let mut failures = Vec::new();
    for dir in descriptor_dirs() {
        match load_descriptor_dir(&dir) {
            Ok((apps, _errors)) if !apps.is_empty() => {
                return Ok((dir, tiles_from_catalog(&apps, &[])));
            }
            Ok(_) => failures.push(format!("{}: no descriptors", dir.display())),
            Err(error) => failures.push(error),
        }
    }
    Err(failures.join("; "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrix_app_api::{ExitReason, LifecycleState, RuntimeKind};

    fn repo_apps() -> Vec<AppDescriptor> {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps");
        let (apps, errors) = load_descriptor_dir(&dir).expect("apps dir");
        assert!(errors.is_empty(), "{errors:?}");
        apps
    }

    #[test]
    fn checked_in_descriptors_become_ordered_tiles() {
        let apps = repo_apps();
        let tiles = tiles_from_catalog(&apps, &[]);
        // Every descriptor but the home surface itself becomes a tile.
        assert_eq!(tiles.len(), apps.len() - 1);
        assert!(tiles.iter().all(|t| t.kind() != "home"));
        // Launchable apps first, then reference entries, each group by name.
        let first_reference = tiles
            .iter()
            .position(|t| !t.launchable)
            .unwrap_or(tiles.len());
        assert!(tiles[..first_reference].iter().all(|t| t.launchable));
        assert!(tiles[first_reference..].iter().all(|t| !t.launchable));
        for group in [&tiles[..first_reference], &tiles[first_reference..]] {
            let names: Vec<String> = group.iter().map(|t| t.name.to_lowercase()).collect();
            let mut sorted = names.clone();
            sorted.sort();
            assert_eq!(names, sorted);
        }
        let launchable: Vec<&str> = tiles[..first_reference].iter().map(Tile::kind).collect();
        for kind in ["contacts", "messages", "phone", "settings", "terminal"] {
            assert!(launchable.contains(&kind), "{kind} is a launchable app");
        }
        let reference: Vec<&str> = tiles[first_reference..].iter().map(Tile::kind).collect();
        assert_eq!(reference, ["camera", "files"]);
        assert!(
            !launchable.contains(&"dialer"),
            "the illustrative dialer entry is gone"
        );
        assert!(tiles.iter().all(|t| !t.running && !t.starting));
    }

    #[test]
    fn browser_tile_is_browser_and_not_the_retired_browser() {
        let tiles = tiles_from_catalog(&repo_apps(), &[]);
        let browser = tiles
            .iter()
            .find(|tile| tile.id.as_str() == "org.ferrix.browser")
            .expect("browser tile");

        assert_eq!(browser.name, "Browser");
        assert_eq!(browser.icon.glyph, "B");
        assert!(browser.launchable);
        assert_eq!(
            tiles.iter().filter(|tile| tile.name == "Browser").count(),
            1
        );
    }

    #[test]
    fn instances_mark_running_and_starting_tiles() {
        let apps = repo_apps();
        let terminal = AppId::new("org.ferrix.terminal").unwrap();
        let index = tiles_from_catalog(&apps, &[])
            .iter()
            .position(|t| t.id == terminal)
            .expect("terminal tile");
        let instance = |state, exit| AppInstance {
            app: terminal.clone(),
            runtime: RuntimeKind::Native,
            role: AppRole::Application,
            instance: 1,
            state,
            pid: Some(42),
            exit,
            surface_mapped: false,
        };
        let tiles = tiles_from_catalog(&apps, &[instance(LifecycleState::Foreground, None)]);
        assert!(tiles[index].running && !tiles[index].starting);
        let tiles = tiles_from_catalog(&apps, &[instance(LifecycleState::Starting, None)]);
        assert!(tiles[index].running && tiles[index].starting);
        let tiles = tiles_from_catalog(
            &apps,
            &[instance(
                LifecycleState::Crashed,
                Some(ExitReason::Signaled { signal: 11 }),
            )],
        );
        assert!(!tiles[index].running && !tiles[index].starting);
    }

    #[test]
    fn descriptor_fallback_finds_the_repository_copy() {
        let (dir, tiles) = load_descriptor_tiles().expect("descriptors");
        assert!(dir.ends_with("apps"), "{}", dir.display());
        assert_eq!(tiles.len(), repo_apps().len() - 1);
    }

    #[test]
    fn tones_map_onto_the_palette() {
        let t = theme::get();
        assert_eq!(tone_color(IconTone::Purple), t.rust);
        assert_eq!(tone_color(IconTone::Orange), t.rust);
        assert_eq!(tone_color(IconTone::Blue), t.steel);
        assert_eq!(tone_color(IconTone::Cyan), t.steel);
        assert_eq!(tone_color(IconTone::Green), t.ok);
        assert_eq!(tone_color(IconTone::Yellow), t.warn);
        assert_eq!(tone_color(IconTone::Red), t.err);
        assert_eq!(tone_color(IconTone::Neutral), t.fg_dim);
    }

    #[test]
    fn checked_in_descriptors_spend_the_accent_on_the_terminal_only() {
        let t = theme::get();
        let rust: Vec<String> = tiles_from_catalog(&repo_apps(), &[])
            .iter()
            .filter(|tile| tone_color(tile.icon.tone) == t.rust)
            .map(|tile| tile.kind().to_owned())
            .collect();
        assert_eq!(rust, ["terminal"], "the accent budget is one tile");
    }

    #[test]
    fn broken_descriptors_are_skipped_not_fatal() {
        let dir = std::env::temp_dir().join(format!("ferrix-catalog-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("bad.json"), "{").unwrap();
        std::fs::write(
            dir.join(ferrix_app_api::GRANTS_FILE_NAME),
            "{\"schema_version\":1,\"grants\":{}}",
        )
        .unwrap();
        std::fs::write(
            dir.join("org.example.one.json"),
            r#"{"schema_version":1,"id":"org.example.one","runtime":"native","name":"One","launchable":false}"#,
        )
        .unwrap();
        let (apps, errors) = load_descriptor_dir(&dir).unwrap();
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(apps.len(), 1);
        assert_eq!(errors.len(), 1);
    }
}
