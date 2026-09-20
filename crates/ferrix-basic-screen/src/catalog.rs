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

/// Maps a descriptor tone onto the Tokyo Night palette.
pub fn tone_color(tone: IconTone) -> u32 {
    match tone {
        IconTone::Blue => theme::BLUE,
        IconTone::Green => theme::GREEN,
        IconTone::Purple => theme::PURPLE,
        IconTone::Cyan => theme::CYAN,
        IconTone::Yellow => theme::YELLOW,
        IconTone::Orange => theme::ORANGE,
        IconTone::Red => theme::RED,
        IconTone::Neutral => theme::FG_DIM,
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
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("json"))
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
        let tiles = tiles_from_catalog(&repo_apps(), &[]);
        // Six tiles: the home surface itself is never listed.
        assert_eq!(tiles.len(), 6);
        assert!(tiles.iter().all(|t| t.kind() != "home"));
        assert_eq!(tiles[0].kind(), "terminal");
        assert!(tiles[0].launchable);
        let rest: Vec<&str> = tiles[1..].iter().map(Tile::kind).collect();
        assert_eq!(rest, ["camera", "dialer", "files", "messages", "settings"]);
        assert!(tiles[1..].iter().all(|t| !t.launchable));
        assert!(tiles.iter().all(|t| !t.running && !t.starting));
    }

    #[test]
    fn instances_mark_running_and_starting_tiles() {
        let apps = repo_apps();
        let terminal = AppId::new("org.ferrix.terminal").unwrap();
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
        assert!(tiles[0].running && !tiles[0].starting);
        let tiles = tiles_from_catalog(&apps, &[instance(LifecycleState::Starting, None)]);
        assert!(tiles[0].running && tiles[0].starting);
        let tiles = tiles_from_catalog(
            &apps,
            &[instance(
                LifecycleState::Crashed,
                Some(ExitReason::Signaled { signal: 11 }),
            )],
        );
        assert!(!tiles[0].running && !tiles[0].starting);
    }

    #[test]
    fn descriptor_fallback_finds_the_repository_copy() {
        let (dir, tiles) = load_descriptor_tiles().expect("descriptors");
        assert!(dir.ends_with("apps"), "{}", dir.display());
        assert_eq!(tiles.len(), 6);
    }

    #[test]
    fn tones_map_onto_the_palette() {
        assert_eq!(tone_color(IconTone::Purple), theme::PURPLE);
        assert_eq!(tone_color(IconTone::Neutral), theme::FG_DIM);
    }

    #[test]
    fn broken_descriptors_are_skipped_not_fatal() {
        let dir = std::env::temp_dir().join(format!("ferrix-catalog-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("bad.json"), "{").unwrap();
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
