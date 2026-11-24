//! Launcher tiles from the application catalog.
//!
//! The home surface never hard-codes the app list: tiles come from `ferrix_appd`'s
//! catalog when it is reachable, otherwise from the descriptor directory
//! (`FERRIX_APP_DIR`, the guest install dir, or the repository `apps/`).

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ferrix_app_api::{
    AppDescriptor, AppIconBitmap, AppId, AppInstance, AppOrigin, AppRole, IconRef, IconTone,
};
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
    /// An installed package (an APK) rather than a checked-in descriptor.
    pub installed: bool,
    /// The installed app's launcher bitmap from `ferrix_appd`
    /// (`Request::AppIcon`), once fetched. Never set on first-party tiles;
    /// `None` draws the letter glyph.
    pub bitmap: Option<Arc<AppIconBitmap>>,
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
            installed: descriptor.origin == AppOrigin::Installed,
            bitmap: None,
        }
    }

    /// Last id segment (`org.ferrix.settings` -> `settings`), the key for the
    /// reference pages and the drawn icons. Empty for an installed package,
    /// so an APK whose package ends in `.browser` never borrows a
    /// first-party glyph or page.
    pub fn kind(&self) -> &str {
        if self.installed {
            return "";
        }
        self.id.as_str().rsplit('.').next().unwrap_or("")
    }
}

/// The ten installed APKs the Pixel 9a listed (Waydroid LineageOS image),
/// as installed entries for host previews and layout tests. Mock data: no
/// Android runtime is involved.
pub fn sample_installed_tiles() -> Vec<Tile> {
    [
        ("org.lineageos.recorder", "Recorder"),
        ("com.android.calculator2", "Calculator"),
        ("org.lineageos.etar", "Calendar"),
        ("org.lineageos.aperture", "Camera"),
        ("com.android.deskclock", "Clock"),
        ("com.android.contacts", "Contacts"),
        ("com.android.documentsui", "Files"),
        ("acr.browser.lightning", "Fulguris"),
        ("org.lineageos.glimpse", "Gallery"),
        ("org.lineageos.twelve", "Music"),
    ]
    .into_iter()
    .map(|(package, name)| Tile {
        id: AppId::new(format!(
            "{}.{package}",
            ferrix_app_api::INSTALLED_APP_PREFIX
        ))
        .expect("valid sample id"),
        name: name.to_owned(),
        icon: IconRef {
            glyph: name[..1].to_owned(),
            tone: IconTone::Blue,
        },
        launchable: true,
        running: false,
        starting: false,
        installed: true,
        bitmap: None,
    })
    .enumerate()
    .map(|(index, mut tile)| {
        // Every sample but Calculator gets a synthetic launcher bitmap, so
        // previews show both the icon and the letter fallback.
        if tile.name != "Calculator" {
            tile.bitmap = Some(Arc::new(sample_icon(index)));
        }
        tile
    })
    .collect()
}

/// A synthetic 96 px launcher bitmap (mock data standing in for Waydroid's
/// PNG): an adaptive-icon-like disc in one of a few colours with a lighter
/// inner mark, transparent outside the disc.
pub fn sample_icon(seed: usize) -> AppIconBitmap {
    const COLORS: [[u8; 3]; 5] = [
        [0x3d, 0x7e, 0xd6],
        [0x2e, 0x9e, 0x6a],
        [0xd0, 0x5a, 0x3c],
        [0x8a, 0x5c, 0xc8],
        [0xe0, 0xa8, 0x2e],
    ];
    let side = ferrix_app_api::APP_ICON_MAX_SIDE;
    let [r, g, b] = COLORS[seed % COLORS.len()];
    let center = side as f32 / 2.0;
    let mut rgba = Vec::with_capacity((side * side * 4) as usize);
    for y in 0..side {
        for x in 0..side {
            let dx = x as f32 + 0.5 - center;
            let dy = y as f32 + 0.5 - center;
            let d = (dx * dx + dy * dy).sqrt();
            let coverage = (center - d).clamp(0.0, 1.0);
            let inner = d < center * 0.42;
            let (pr, pg, pb) = if inner { (0xf4, 0xf1, 0xea) } else { (r, g, b) };
            rgba.extend_from_slice(&[pr, pg, pb, (coverage * 255.0) as u8]);
        }
    }
    AppIconBitmap::new(side, side, rgba).expect("sample icon within bounds")
}

/// Most icons one catalog update asks `ferrix_appd` for.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub const MAX_ICON_FETCH: usize = 64;

/// Installed-app bitmaps the home already has, and the ids being fetched.
/// First-party tiles are never fetched or touched. Failures are not cached:
/// Waydroid may write an icon after its package is listed, so the next
/// catalog change asks again.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[derive(Default)]
pub struct IconCache {
    icons: HashMap<AppId, Arc<AppIconBitmap>>,
    pending: BTreeSet<AppId>,
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))] // used by the Wayland home (app.rs)
impl IconCache {
    /// After the tiles changed: applies known bitmaps, forgets apps no
    /// longer listed, and returns the installed ids to fetch now (at most
    /// [`MAX_ICON_FETCH`], marked pending).
    pub fn sync(&mut self, tiles: &mut [Tile]) -> Vec<AppId> {
        let listed: BTreeSet<&AppId> = tiles
            .iter()
            .filter(|t| t.installed)
            .map(|t| &t.id)
            .collect();
        self.icons.retain(|id, _| listed.contains(id));
        let mut fetch = Vec::new();
        for tile in tiles.iter_mut() {
            if !tile.installed {
                tile.bitmap = None;
                continue;
            }
            tile.bitmap = self.icons.get(&tile.id).cloned();
            if tile.bitmap.is_none()
                && !self.pending.contains(&tile.id)
                && fetch.len() < MAX_ICON_FETCH
            {
                fetch.push(tile.id.clone());
            }
        }
        self.pending.extend(fetch.iter().cloned());
        fetch
    }

    /// Records one fetch result. Returns whether a tile changed.
    pub fn resolve(&mut self, id: AppId, icon: Option<AppIconBitmap>, tiles: &mut [Tile]) -> bool {
        self.pending.remove(&id);
        let Some(icon) = icon.filter(|icon| !icon.is_blank()) else {
            return false;
        };
        let icon = Arc::new(icon);
        let mut changed = false;
        for tile in tiles.iter_mut().filter(|t| t.installed && t.id == id) {
            tile.bitmap = Some(Arc::clone(&icon));
            changed = true;
        }
        if changed {
            self.icons.insert(id, icon);
        }
        changed
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

/// Deterministic launcher order: launchable applications first, then
/// reference entries, then installed Android apps (APKs) appended after
/// every first-party tile; each group by name.
pub fn tiles_from_catalog(apps: &[AppDescriptor], instances: &[AppInstance]) -> Vec<Tile> {
    // The home surface never lists itself: it is the launcher, not a tile.
    let mut sorted: Vec<&AppDescriptor> = apps.iter().filter(|d| d.role != AppRole::Home).collect();
    sorted.sort_by(|a, b| {
        let group = |d: &AppDescriptor| match (d.origin, d.launchable) {
            (AppOrigin::Installed, _) => 2,
            (_, true) => 0,
            (_, false) => 1,
        };
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
        for kind in [
            "contacts", "files", "messages", "phone", "settings", "terminal",
        ] {
            assert!(launchable.contains(&kind), "{kind} is a launchable app");
        }
        let reference: Vec<&str> = tiles[first_reference..].iter().map(Tile::kind).collect();
        assert_eq!(reference, ["camera"]);
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
    fn installed_apks_are_appended_after_first_party_tiles() {
        let mut apps = repo_apps();
        let template = apps[0].clone();
        let installed = |id: &str, name: &str| AppDescriptor {
            id: AppId::new(id).unwrap(),
            name: name.to_owned(),
            icon: IconRef {
                glyph: name[..1].to_owned(),
                tone: IconTone::Blue,
            },
            origin: AppOrigin::Installed,
            runtime: RuntimeKind::Android,
            launchable: true,
            role: AppRole::Application,
            capabilities: Default::default(),
            ..template.clone()
        };
        apps.push(installed("apk.org.example.zebra", "Zebra"));
        apps.push(installed("apk.com.example.browser", "Another browser"));
        let tiles = tiles_from_catalog(&apps, &[]);
        let n = tiles.len();
        assert_eq!(tiles[n - 2].name, "Another browser");
        assert_eq!(tiles[n - 1].name, "Zebra");
        assert!(tiles[n - 2].installed && tiles[n - 2].launchable);
        // Never mistaken for the first-party Browser glyph or page.
        assert_eq!(tiles[n - 2].kind(), "");
        assert!(tiles[..n - 2].iter().all(|t| !t.installed));
        assert_eq!(tiles.iter().filter(|t| t.kind() == "browser").count(), 1);
    }

    fn installed_tile(id: &str) -> Tile {
        Tile {
            id: AppId::new(id).unwrap(),
            name: "Notes".to_owned(),
            icon: IconRef {
                glyph: "N".to_owned(),
                tone: IconTone::Blue,
            },
            launchable: true,
            running: false,
            starting: false,
            installed: true,
            bitmap: None,
        }
    }

    #[test]
    fn icon_cache_fetches_installed_apps_once_and_never_first_party_tiles() {
        let mut tiles = tiles_from_catalog(&repo_apps(), &[]);
        tiles.push(installed_tile("apk.org.example.notes"));
        tiles.push(installed_tile("apk.org.example.other"));
        let mut cache = IconCache::default();
        let fetch = cache.sync(&mut tiles);
        assert_eq!(
            fetch.iter().map(AppId::as_str).collect::<Vec<_>>(),
            ["apk.org.example.notes", "apk.org.example.other"]
        );
        // Pending ids are not asked for twice.
        assert!(cache.sync(&mut tiles).is_empty());
        let icon = sample_icon(0);
        let notes = AppId::new("apk.org.example.notes").unwrap();
        assert!(cache.resolve(notes.clone(), Some(icon.clone()), &mut tiles));
        // A missing icon keeps the letter and is retried later.
        let other = AppId::new("apk.org.example.other").unwrap();
        assert!(!cache.resolve(other.clone(), None, &mut tiles));
        let with_bitmap: Vec<&str> = tiles
            .iter()
            .filter(|t| t.bitmap.is_some())
            .map(|t| t.id.as_str())
            .collect();
        assert_eq!(with_bitmap, ["apk.org.example.notes"]);
        // A fresh catalog (new Tile values) gets the cached bitmap back.
        let mut fresh = tiles_from_catalog(&repo_apps(), &[]);
        fresh.push(installed_tile("apk.org.example.notes"));
        fresh.push(installed_tile("apk.org.example.other"));
        assert_eq!(
            cache
                .sync(&mut fresh)
                .iter()
                .map(AppId::as_str)
                .collect::<Vec<_>>(),
            ["apk.org.example.other"]
        );
        let notes_tile = fresh.iter().find(|t| t.id == notes).unwrap();
        assert_eq!(notes_tile.bitmap.as_deref(), Some(&icon));
        assert!(
            fresh
                .iter()
                .filter(|t| !t.installed)
                .all(|t| t.bitmap.is_none())
        );
        // Uninstalled apps are forgotten.
        let mut gone = tiles_from_catalog(&repo_apps(), &[]);
        cache.resolve(other, None, &mut gone);
        assert!(cache.sync(&mut gone).is_empty());
        let mut back = vec![installed_tile("apk.org.example.notes")];
        assert_eq!(
            cache.sync(&mut back).len(),
            1,
            "fetched again after removal"
        );
    }

    #[test]
    fn blank_icons_are_ignored_and_first_party_ids_never_take_a_bitmap() {
        let mut tiles = tiles_from_catalog(&repo_apps(), &[]);
        let mut cache = IconCache::default();
        assert!(
            cache.sync(&mut tiles).is_empty(),
            "no installed apps, no fetch"
        );
        let terminal = AppId::new("org.ferrix.terminal").unwrap();
        assert!(!cache.resolve(terminal, Some(sample_icon(1)), &mut tiles));
        let mut installed = vec![installed_tile("apk.org.example.notes")];
        cache.sync(&mut installed);
        let blank = AppIconBitmap::new(1, 1, vec![0, 0, 0, 0]).unwrap();
        assert!(!cache.resolve(installed[0].id.clone(), Some(blank), &mut installed));
        assert!(installed[0].bitmap.is_none());
    }

    #[test]
    fn icon_fetches_are_bounded_per_catalog_update() {
        let mut tiles: Vec<Tile> = (0..MAX_ICON_FETCH + 10)
            .map(|i| installed_tile(&format!("apk.org.example.app{i}")))
            .collect();
        let mut cache = IconCache::default();
        assert_eq!(cache.sync(&mut tiles).len(), MAX_ICON_FETCH);
        assert_eq!(cache.sync(&mut tiles).len(), 10);
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
