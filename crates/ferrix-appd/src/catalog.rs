//! The application catalog: every descriptor `ferrix_appd` knows about.
//!
//! Descriptors are `*.json` files in one or more directories
//! (`/usr/share/ferrix/apps` in the guest, `apps/` in the repository). The
//! catalog rejects duplicate ids and more than one home surface; the
//! descriptor files are the only source of truth, nothing is built in.

use std::collections::BTreeMap;
use std::path::Path;

use ferrix_app_api::{AppDescriptor, AppError, AppId, AppRole};
use serde::Deserialize;
use serde_json::Value;

/// Daemon-private manifest entry. Only `descriptor` crosses ferrix-app-protocol;
/// `launch` is opaque to ferrix_appd and consumed by the selected runtime adapter.
#[derive(Clone, Debug, Deserialize)]
struct Manifest {
    #[serde(flatten)]
    descriptor: AppDescriptor,
    #[serde(default)]
    launch: Value,
}

#[derive(Clone, Debug)]
pub struct CatalogEntry {
    pub descriptor: AppDescriptor,
    pub launch: Value,
}

#[derive(Clone, Debug, Default)]
pub struct Catalog {
    apps: BTreeMap<AppId, CatalogEntry>,
    home: Option<AppId>,
}

impl Catalog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds a catalog from in-memory descriptors (tests, fallbacks).
    pub fn from_descriptors(
        descriptors: impl IntoIterator<Item = AppDescriptor>,
    ) -> Result<Self, AppError> {
        let mut catalog = Self::new();
        for descriptor in descriptors {
            catalog.insert(descriptor)?;
        }
        Ok(catalog)
    }

    /// Loads every `*.json` descriptor in `dir`.
    pub fn load_dir(dir: &Path) -> Result<Self, AppError> {
        let mut catalog = Self::new();
        catalog.extend_from_dir(dir)?;
        Ok(catalog)
    }

    /// Adds every `*.json` descriptor in `dir`; returns how many were added.
    /// Files are visited in name order so failures are deterministic.
    pub fn extend_from_dir(&mut self, dir: &Path) -> Result<usize, AppError> {
        let entries = std::fs::read_dir(dir).map_err(|e| AppError::Io {
            reason: format!("cannot read app directory {}: {e}", dir.display()),
        })?;
        let mut paths: Vec<_> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("json"))
            .collect();
        paths.sort();
        let mut added = 0;
        for path in paths {
            let text = std::fs::read_to_string(&path).map_err(|e| AppError::Io {
                reason: format!("cannot read {}: {e}", path.display()),
            })?;
            let manifest: Manifest =
                serde_json::from_str(&text).map_err(|error| AppError::InvalidDescriptor {
                    id: None,
                    reason: format!("{}: {error}", path.display()),
                })?;
            self.insert_entry(CatalogEntry {
                descriptor: manifest.descriptor,
                launch: manifest.launch,
            })?;
            added += 1;
        }
        Ok(added)
    }

    /// Adds one validated descriptor. Rejects a repeated id and a second
    /// home surface.
    pub fn insert(&mut self, descriptor: AppDescriptor) -> Result<(), AppError> {
        self.insert_entry(CatalogEntry {
            descriptor,
            // Fake runtimes used with in-memory catalogs accept an empty
            // opaque configuration.
            launch: Value::Object(Default::default()),
        })
    }

    pub fn insert_entry(&mut self, entry: CatalogEntry) -> Result<(), AppError> {
        let descriptor = &entry.descriptor;
        descriptor.validate()?;
        if descriptor.launchable && entry.launch.is_null() {
            return Err(AppError::InvalidDescriptor {
                id: Some(descriptor.id.clone()),
                reason: "launchable app has no runtime launch configuration".to_owned(),
            });
        }
        if self.apps.contains_key(&descriptor.id) {
            return Err(AppError::DuplicateApp {
                id: descriptor.id.clone(),
            });
        }
        if descriptor.role == AppRole::Home {
            if let Some(existing) = &self.home {
                return Err(AppError::InvalidDescriptor {
                    id: Some(descriptor.id.clone()),
                    reason: format!("second home surface; {existing} is already the home"),
                });
            }
            self.home = Some(descriptor.id.clone());
        }
        self.apps.insert(descriptor.id.clone(), entry);
        Ok(())
    }

    pub fn get(&self, id: &AppId) -> Option<&CatalogEntry> {
        self.apps.get(id)
    }

    pub fn contains(&self, id: &AppId) -> bool {
        self.apps.contains_key(id)
    }

    pub fn len(&self) -> usize {
        self.apps.len()
    }

    pub fn is_empty(&self) -> bool {
        self.apps.is_empty()
    }

    /// All descriptors ordered by id.
    pub fn iter(&self) -> impl Iterator<Item = &AppDescriptor> {
        self.apps.values().map(|entry| &entry.descriptor)
    }

    /// Owned copy of every descriptor, for `Reply::Catalog`.
    pub fn descriptors(&self) -> Vec<AppDescriptor> {
        self.iter().cloned().collect()
    }

    /// Descriptors the home surface may start.
    pub fn launchable(&self) -> impl Iterator<Item = &AppDescriptor> {
        self.iter().filter(|d| d.launchable)
    }

    /// Launchable descriptors flagged `autostart`, home excluded (it is
    /// always started first by the service).
    pub fn autostart(&self) -> impl Iterator<Item = &AppDescriptor> {
        self.launchable()
            .filter(|d| d.autostart && d.role != AppRole::Home)
    }

    /// The single home surface, when the catalog has one.
    pub fn home(&self) -> Option<&AppDescriptor> {
        self.home
            .as_ref()
            .and_then(|id| self.apps.get(id))
            .map(|entry| &entry.descriptor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrix_app_api::{IconRef, RuntimeKind, SCHEMA_VERSION, TrustLevel};

    pub(crate) fn descriptor(id: &str, role: AppRole, launchable: bool) -> AppDescriptor {
        AppDescriptor {
            schema_version: SCHEMA_VERSION,
            id: AppId::new(id).unwrap(),
            runtime: RuntimeKind::Native,
            name: id.rsplit('.').next().unwrap().to_owned(),
            icon: IconRef::default(),
            launchable,
            autostart: false,
            role,
            trust: TrustLevel::Untrusted,
            capabilities: Default::default(),
        }
    }

    fn repo_apps_dir() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps")
    }

    #[test]
    fn loads_the_repository_descriptors() {
        let catalog = Catalog::load_dir(&repo_apps_dir()).unwrap();
        assert!(catalog.len() >= 2);
        let home = catalog.home().expect("home");
        assert_eq!(home.id.as_str(), "org.ferrix.home");
        assert!(catalog.launchable().count() >= 2);
        assert!(
            catalog
                .iter()
                .any(|d| d.id.as_str() == "org.ferrix.terminal" && d.launchable)
        );
        // Home is autostarted by the service, never listed twice.
        assert!(catalog.autostart().all(|d| d.role != AppRole::Home));
    }

    #[test]
    fn rejects_duplicates_and_a_second_home() {
        let mut catalog = Catalog::new();
        catalog
            .insert(descriptor("org.ferrix.home", AppRole::Home, true))
            .unwrap();
        assert!(matches!(
            catalog.insert(descriptor("org.ferrix.home", AppRole::Home, true)),
            Err(AppError::DuplicateApp { .. })
        ));
        assert!(matches!(
            catalog.insert(descriptor("org.ferrix.other-home", AppRole::Home, true)),
            Err(AppError::InvalidDescriptor { id: Some(_), .. })
        ));
        assert_eq!(catalog.len(), 1);
    }

    #[test]
    fn directory_errors_are_typed_and_name_the_file() {
        let dir = std::env::temp_dir().join(format!(
            "ferrix-appd-catalog-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("broken.json"), "{").unwrap();
        let err = Catalog::load_dir(&dir).unwrap_err();
        match err {
            AppError::InvalidDescriptor { id: None, reason } => {
                assert!(reason.contains("broken.json"), "{reason}");
            }
            other => panic!("unexpected {other:?}"),
        }
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(matches!(Catalog::load_dir(&dir), Err(AppError::Io { .. })));
    }
}
