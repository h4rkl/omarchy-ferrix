//! Filesystem access confined to the user's home directory.
//!
//! Every operation names a folder as components relative to the home root
//! plus, where needed, one entry name. The folder is canonicalized (so a
//! symlink anywhere on the way is resolved) and must still lie inside the
//! canonical home; entry names are single path components and are never
//! `.`/`..` or separators. Nothing here follows a path the person did not
//! reach by tapping an entry, and nothing can name a path outside `$HOME`.
//!
//! The sandbox gives Files a private `/tmp` and `/proc` but leaves the home
//! directory shared, so every change here is durable and visible to other
//! apps (Contacts writes its vCard export to `~/Documents`).
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::fmt;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use jiff::Timestamp;

/// Longest entry name accepted for create/rename (bytes, the common Linux
/// `NAME_MAX`).
pub const MAX_NAME_BYTES: usize = 255;
/// Entries listed per folder; the rest are counted, not shown.
pub const LIST_LIMIT: usize = 1000;
/// Children counted per subfolder before the count reads `999+`.
pub const CHILD_COUNT_LIMIT: usize = 999;
/// Bytes of a file the viewer reads.
pub const VIEW_LIMIT: usize = 256 * 1024;

/// Typed failures with the short, calm copy the surface shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FsError {
    /// `$HOME` is unset, relative, `/`, or not a readable folder.
    HomeUnavailable(String),
    /// The path resolves outside the home directory.
    OutsideHome,
    InvalidName(&'static str),
    NotFound,
    AlreadyExists,
    PermissionDenied,
    NotAFolder,
    NotAFile,
    Io(String),
}

impl fmt::Display for FsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FsError::HomeUnavailable(reason) => write!(f, "home unavailable \u{b7} {reason}"),
            FsError::OutsideHome => f.write_str("outside home \u{b7} not opened"),
            FsError::InvalidName(reason) => f.write_str(reason),
            FsError::NotFound => f.write_str("not found \u{b7} refresh"),
            FsError::AlreadyExists => f.write_str("name already exists"),
            FsError::PermissionDenied => f.write_str("permission denied"),
            FsError::NotAFolder => f.write_str("not a folder"),
            FsError::NotAFile => f.write_str("not a regular file"),
            FsError::Io(reason) => f.write_str(reason),
        }
    }
}

impl FsError {
    /// Stable kebab-case code for evidence records (never a path or name).
    pub fn code(&self) -> &'static str {
        match self {
            FsError::HomeUnavailable(_) => "home-unavailable",
            FsError::OutsideHome => "outside-home",
            FsError::InvalidName(_) => "invalid-name",
            FsError::NotFound => "not-found",
            FsError::AlreadyExists => "already-exists",
            FsError::PermissionDenied => "permission-denied",
            FsError::NotAFolder => "not-a-folder",
            FsError::NotAFile => "not-a-file",
            FsError::Io(_) => "io",
        }
    }
}

impl From<io::Error> for FsError {
    fn from(error: io::Error) -> Self {
        match error.kind() {
            io::ErrorKind::NotFound => FsError::NotFound,
            io::ErrorKind::AlreadyExists => FsError::AlreadyExists,
            io::ErrorKind::PermissionDenied => FsError::PermissionDenied,
            io::ErrorKind::NotADirectory => FsError::NotAFolder,
            io::ErrorKind::DirectoryNotEmpty => FsError::Io("folder not empty".to_owned()),
            _ => FsError::Io(error.to_string().to_lowercase()),
        }
    }
}

/// What an entry is, following symlinks that stay inside home.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Folder,
    File,
    /// A symlink that dangles or points outside home: listed, not opened.
    Link,
    /// Sockets, devices, FIFOs.
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub kind: Kind,
    /// Whether the entry itself is a symlink.
    pub link: bool,
    /// File size in bytes (0 for folders).
    pub size: u64,
    pub modified: Option<Timestamp>,
    /// Visible children of a folder, capped at [`CHILD_COUNT_LIMIT`] + 1.
    pub children: Option<usize>,
}

impl Entry {
    pub fn is_folder(&self) -> bool {
        self.kind == Kind::Folder
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Listing {
    /// Folders first, then files; each by case-folded name.
    pub entries: Vec<Entry>,
    /// Dot-entries and names that are not UTF-8: counted, not shown.
    pub hidden: usize,
    /// Entries beyond [`LIST_LIMIT`].
    pub truncated: usize,
}

/// A file opened read-only for the viewer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextFile {
    /// `None` when the content is not text.
    pub text: Option<String>,
    pub size: u64,
    pub modified: Option<Timestamp>,
    /// Whether only the first [`VIEW_LIMIT`] bytes were read.
    pub truncated: bool,
}

/// The canonical home directory every operation is confined to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Home {
    root: PathBuf,
}

/// Rejects names that could escape the folder they are joined to. Applies
/// to names read from a listing as well as typed ones.
fn component(name: &str) -> Result<&str, FsError> {
    if name.is_empty() {
        return Err(FsError::InvalidName("name is empty"));
    }
    if name == "." || name == ".." {
        return Err(FsError::InvalidName("name is reserved"));
    }
    if name.contains('/') || name.contains('\0') {
        return Err(FsError::InvalidName("name cannot contain /"));
    }
    Ok(name)
}

/// Validates a typed name for create/rename.
pub fn validate_new_name(name: &str) -> Result<&str, FsError> {
    if name.trim().is_empty() {
        return Err(FsError::InvalidName("name is empty"));
    }
    component(name)?;
    if name.starts_with('.') {
        return Err(FsError::InvalidName("names starting with . are hidden"));
    }
    if name.len() > MAX_NAME_BYTES {
        return Err(FsError::InvalidName("name is too long"));
    }
    if name != name.trim() {
        return Err(FsError::InvalidName("name has leading or trailing spaces"));
    }
    Ok(name)
}

fn timestamp(time: io::Result<SystemTime>) -> Option<Timestamp> {
    time.ok().and_then(|t| Timestamp::try_from(t).ok())
}

impl Home {
    /// Opens `path` as the home root. It must be absolute, not `/`, and a
    /// folder; it is canonicalized so later checks compare real paths.
    pub fn new(path: &Path) -> Result<Self, FsError> {
        if !path.is_absolute() {
            return Err(FsError::HomeUnavailable("HOME is not absolute".to_owned()));
        }
        let root = fs::canonicalize(path)
            .map_err(|e| FsError::HomeUnavailable(FsError::from(e).to_string()))?;
        if root.parent().is_none() {
            return Err(FsError::HomeUnavailable("HOME is /".to_owned()));
        }
        if !root.is_dir() {
            return Err(FsError::HomeUnavailable("HOME is not a folder".to_owned()));
        }
        Ok(Self { root })
    }

    /// The home root from `$HOME`; an unset or empty value is refused
    /// rather than guessed.
    pub fn from_env(env: impl Fn(&str) -> Option<String>) -> Result<Self, FsError> {
        let home = env("HOME")
            .filter(|v| !v.is_empty())
            .ok_or_else(|| FsError::HomeUnavailable("HOME is unset".to_owned()))?;
        Self::new(Path::new(&home))
    }

    #[cfg(test)]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Canonicalizes `path` and requires it to stay inside home.
    fn confine(&self, path: &Path) -> Result<PathBuf, FsError> {
        let real = fs::canonicalize(path)?;
        if real.starts_with(&self.root) {
            Ok(real)
        } else {
            Err(FsError::OutsideHome)
        }
    }

    /// The canonical folder at `rel` (components below home).
    pub fn folder(&self, rel: &[String]) -> Result<PathBuf, FsError> {
        let mut path = self.root.clone();
        for part in rel {
            path.push(component(part)?);
        }
        let real = self.confine(&path)?;
        if !real.is_dir() {
            return Err(FsError::NotAFolder);
        }
        Ok(real)
    }

    /// `name` inside the folder at `rel`; the leaf itself is not resolved,
    /// so operations on a symlink act on the link.
    fn child(&self, rel: &[String], name: &str) -> Result<PathBuf, FsError> {
        Ok(self.folder(rel)?.join(component(name)?))
    }

    fn entry(&self, folder: &Path, name: String) -> Entry {
        let path = folder.join(&name);
        let link = fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink());
        // A link is followed only when its target stays inside home.
        let followed = if link {
            self.confine(&path).ok().and_then(|p| fs::metadata(p).ok())
        } else {
            fs::metadata(&path).ok()
        };
        let Some(meta) = followed else {
            return Entry {
                name,
                kind: if link { Kind::Link } else { Kind::Other },
                link,
                size: 0,
                modified: None,
                children: None,
            };
        };
        let kind = if meta.is_dir() {
            Kind::Folder
        } else if meta.is_file() {
            Kind::File
        } else {
            Kind::Other
        };
        let children = (kind == Kind::Folder).then(|| {
            fs::read_dir(&path).map_or(0, |dir| {
                dir.flatten()
                    .filter(|e| e.file_name().to_str().is_some_and(|n| !n.starts_with('.')))
                    .take(CHILD_COUNT_LIMIT + 1)
                    .count()
            })
        });
        Entry {
            name,
            kind,
            link,
            size: if kind == Kind::File { meta.len() } else { 0 },
            modified: timestamp(meta.modified()),
            children,
        }
    }

    /// Lists the folder at `rel`: folders first, then by case-folded name.
    pub fn list(&self, rel: &[String]) -> Result<Listing, FsError> {
        let folder = self.folder(rel)?;
        let mut listing = Listing::default();
        let mut names = Vec::new();
        for entry in fs::read_dir(&folder)? {
            let entry = entry?;
            match entry.file_name().into_string() {
                Ok(name) if !name.starts_with('.') => names.push(name),
                _ => listing.hidden += 1,
            }
        }
        let mut entries: Vec<Entry> = names
            .into_iter()
            .map(|name| self.entry(&folder, name))
            .collect();
        sort(&mut entries);
        if entries.len() > LIST_LIMIT {
            listing.truncated = entries.len() - LIST_LIMIT;
            entries.truncate(LIST_LIMIT);
        }
        listing.entries = entries;
        Ok(listing)
    }

    /// Creates the folder `name` inside `rel`.
    pub fn create_folder(&self, rel: &[String], name: &str) -> Result<(), FsError> {
        let name = validate_new_name(name)?;
        fs::create_dir(self.child(rel, name)?)?;
        Ok(())
    }

    /// Renames `from` to `to` inside `rel`; an existing `to` is never
    /// replaced.
    pub fn rename(&self, rel: &[String], from: &str, to: &str) -> Result<(), FsError> {
        let to = validate_new_name(to)?;
        let source = self.child(rel, from)?;
        fs::symlink_metadata(&source)?;
        if from == to {
            return Ok(());
        }
        let target = self.child(rel, to)?;
        if fs::symlink_metadata(&target).is_ok() {
            return Err(FsError::AlreadyExists);
        }
        fs::rename(source, target)?;
        Ok(())
    }

    /// Visible items inside the folder `name` (for the delete
    /// confirmation), or `None` when `name` is not a real folder.
    pub fn folder_items(&self, rel: &[String], name: &str) -> Option<usize> {
        let path = self.child(rel, name).ok()?;
        let meta = fs::symlink_metadata(&path).ok()?;
        if !meta.is_dir() {
            return None;
        }
        fn walk(path: &Path, budget: &mut usize) -> usize {
            let Ok(dir) = fs::read_dir(path) else {
                return 0;
            };
            let mut count = 0;
            for entry in dir.flatten() {
                if *budget == 0 {
                    break;
                }
                *budget -= 1;
                count += 1;
                if entry.file_type().is_ok_and(|t| t.is_dir()) {
                    count += walk(&entry.path(), budget);
                }
            }
            count
        }
        let mut budget = 10_000;
        Some(walk(&path, &mut budget))
    }

    /// Deletes `name` inside `rel`: a symlink or file is unlinked, a real
    /// folder is removed with its contents (std's `remove_dir_all` does not
    /// follow symlinks inside it).
    pub fn delete(&self, rel: &[String], name: &str) -> Result<(), FsError> {
        let path = self.child(rel, name)?;
        let meta = fs::symlink_metadata(&path)?;
        if meta.is_dir() {
            fs::remove_dir_all(&path)?;
        } else {
            fs::remove_file(&path)?;
        }
        Ok(())
    }

    /// Opens `name` inside `rel` read-only. Content with NUL bytes or that
    /// is not UTF-8 is reported as not text.
    pub fn read_text(&self, rel: &[String], name: &str) -> Result<TextFile, FsError> {
        let path = self.confine(&self.child(rel, name)?)?;
        let meta = fs::metadata(&path)?;
        if !meta.is_file() {
            return Err(FsError::NotAFile);
        }
        let mut bytes = Vec::new();
        fs::File::open(&path)?
            .take(VIEW_LIMIT as u64 + 1)
            .read_to_end(&mut bytes)?;
        let truncated = bytes.len() > VIEW_LIMIT;
        bytes.truncate(VIEW_LIMIT);
        Ok(TextFile {
            text: decode_text(&bytes, truncated),
            size: meta.len(),
            modified: timestamp(meta.modified()),
            truncated,
        })
    }
}

/// Folders first, then case-folded name, then the exact name.
pub fn sort(entries: &mut [Entry]) {
    entries.sort_by(|a, b| {
        b.is_folder()
            .cmp(&a.is_folder())
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name))
    });
}

/// UTF-8 text without NUL bytes; a sequence cut by the read limit is
/// dropped rather than turning the whole file into "binary".
pub fn decode_text(bytes: &[u8], truncated: bool) -> Option<String> {
    if bytes.contains(&0) {
        return None;
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => Some(text.to_owned()),
        Err(e) if truncated && e.error_len().is_none() => {
            Some(String::from_utf8_lossy(&bytes[..e.valid_up_to()]).into_owned())
        }
        Err(_) => None,
    }
}

/// `1.2 KB`-style size: decimal units, one decimal below 10.
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if bytes < 1000 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit + 1 < UNITS.len() {
        value /= 1000.0;
        unit += 1;
    }
    if value < 10.0 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{value:.0} {}", UNITS[unit])
    }
}

#[cfg(test)]
pub mod testdir {
    //! A unique scratch folder per test, removed on drop (std only, so the
    //! crate needs no extra dev-dependency).
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    pub struct TestDir(PathBuf);

    impl TestDir {
        pub fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "ferrix-files-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        pub fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testdir::TestDir;
    use super::*;

    fn rel(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|p| (*p).to_owned()).collect()
    }

    fn names(listing: &Listing) -> Vec<&str> {
        listing.entries.iter().map(|e| e.name.as_str()).collect()
    }

    fn fixture() -> (TestDir, Home) {
        let dir = TestDir::new();
        let home = dir.path().join("home");
        fs::create_dir_all(home.join("Documents/inner")).unwrap();
        fs::create_dir_all(home.join("downloads")).unwrap();
        fs::write(home.join("Documents/contacts.vcf"), "BEGIN:VCARD\n").unwrap();
        fs::write(home.join("b.txt"), "bee").unwrap();
        fs::write(home.join("A.txt"), "a").unwrap();
        fs::write(home.join(".hidden"), "x").unwrap();
        fs::write(dir.path().join("outside.txt"), "secret").unwrap();
        let h = Home::new(&home).unwrap();
        (dir, h)
    }

    #[test]
    fn home_comes_from_an_absolute_existing_folder() {
        let (dir, home) = fixture();
        let env = |v: &'static str| move |k: &str| (k == "HOME").then(|| v.to_owned());
        assert!(matches!(
            Home::from_env(|_| None),
            Err(FsError::HomeUnavailable(_))
        ));
        assert!(matches!(
            Home::from_env(env("")),
            Err(FsError::HomeUnavailable(_))
        ));
        assert!(matches!(
            Home::from_env(env("relative/home")),
            Err(FsError::HomeUnavailable(_))
        ));
        assert!(
            matches!(Home::from_env(env("/")), Err(FsError::HomeUnavailable(r)) if r == "HOME is /")
        );
        let file = dir.path().join("outside.txt");
        assert!(Home::new(&file).is_err());
        assert!(Home::new(&dir.path().join("missing")).is_err());
        assert!(home.root().is_absolute());
    }

    #[test]
    fn listing_sorts_folders_first_and_hides_dot_entries() {
        let (_dir, home) = fixture();
        let listing = home.list(&[]).unwrap();
        assert_eq!(
            names(&listing),
            ["Documents", "downloads", "A.txt", "b.txt"]
        );
        assert_eq!(listing.hidden, 1);
        assert_eq!(listing.truncated, 0);
        let docs = &listing.entries[0];
        assert_eq!(docs.kind, Kind::Folder);
        assert_eq!(docs.children, Some(2));
        assert_eq!(docs.size, 0);
        let b = &listing.entries[3];
        assert_eq!((b.kind, b.size, b.children), (Kind::File, 3, None));
        assert!(b.modified.is_some());
        let inner = home.list(&rel(&["Documents"])).unwrap();
        assert_eq!(names(&inner), ["inner", "contacts.vcf"]);
    }

    #[test]
    fn paths_never_leave_home() {
        let (dir, home) = fixture();
        assert_eq!(
            home.list(&rel(&[".."])),
            Err(FsError::InvalidName("name is reserved"))
        );
        assert_eq!(
            home.list(&rel(&["Documents/../.."])),
            Err(FsError::InvalidName("name cannot contain /"))
        );
        assert_eq!(home.list(&rel(&["missing"])), Err(FsError::NotFound));
        assert_eq!(home.list(&rel(&["A.txt"])), Err(FsError::NotAFolder));
        assert_eq!(
            home.read_text(&[], "../outside.txt"),
            Err(FsError::InvalidName("name cannot contain /"))
        );
        assert_eq!(
            home.delete(&[], ".."),
            Err(FsError::InvalidName("name is reserved"))
        );
        assert_eq!(
            home.rename(&[], "A.txt", "../escaped"),
            Err(FsError::InvalidName("name cannot contain /"))
        );
        assert!(dir.path().join("outside.txt").exists());
        #[cfg(unix)]
        {
            // Symlinks out of home are listed but never followed.
            std::os::unix::fs::symlink(dir.path(), home.root().join("escape")).unwrap();
            std::os::unix::fs::symlink(
                dir.path().join("outside.txt"),
                home.root().join("leak.txt"),
            )
            .unwrap();
            std::os::unix::fs::symlink(home.root().join("Documents"), home.root().join("docs"))
                .unwrap();
            let listing = home.list(&[]).unwrap();
            let escape = listing.entries.iter().find(|e| e.name == "escape").unwrap();
            assert_eq!((escape.kind, escape.link), (Kind::Link, true));
            let docs = listing.entries.iter().find(|e| e.name == "docs").unwrap();
            assert_eq!((docs.kind, docs.link), (Kind::Folder, true));
            assert_eq!(home.list(&rel(&["escape"])), Err(FsError::OutsideHome));
            assert_eq!(home.read_text(&[], "leak.txt"), Err(FsError::OutsideHome));
            assert_eq!(
                names(&home.list(&rel(&["docs"])).unwrap()),
                ["inner", "contacts.vcf"]
            );
            // Deleting a link removes the link, not what it points at.
            home.delete(&[], "escape").unwrap();
            home.delete(&[], "docs").unwrap();
            assert!(dir.path().join("outside.txt").exists());
            assert!(home.root().join("Documents/contacts.vcf").exists());
        }
    }

    #[test]
    fn create_rename_and_delete() {
        let (_dir, home) = fixture();
        home.create_folder(&[], "Notes").unwrap();
        assert_eq!(
            home.create_folder(&[], "Notes"),
            Err(FsError::AlreadyExists)
        );
        assert_eq!(
            home.create_folder(&[], ""),
            Err(FsError::InvalidName("name is empty"))
        );
        assert_eq!(
            home.create_folder(&[], "   "),
            Err(FsError::InvalidName("name is empty"))
        );
        assert_eq!(
            home.create_folder(&[], ".cache"),
            Err(FsError::InvalidName("names starting with . are hidden"))
        );
        assert_eq!(
            home.create_folder(&[], &"x".repeat(256)),
            Err(FsError::InvalidName("name is too long"))
        );
        assert_eq!(
            home.create_folder(&[], " pad"),
            Err(FsError::InvalidName("name has leading or trailing spaces"))
        );
        assert_eq!(
            home.create_folder(&rel(&["missing"]), "x"),
            Err(FsError::NotFound)
        );
        home.create_folder(&rel(&["Documents"]), "Sub").unwrap();
        assert!(home.root().join("Documents/Sub").is_dir());

        home.rename(&[], "b.txt", "bee.txt").unwrap();
        assert!(home.root().join("bee.txt").is_file());
        assert_eq!(
            home.rename(&[], "bee.txt", "A.txt"),
            Err(FsError::AlreadyExists)
        );
        assert_eq!(home.rename(&[], "missing", "x"), Err(FsError::NotFound));
        home.rename(&[], "bee.txt", "bee.txt").unwrap();
        home.rename(&[], "Notes", "Journal").unwrap();
        assert!(home.root().join("Journal").is_dir());

        assert_eq!(home.folder_items(&[], "Documents"), Some(3));
        assert_eq!(home.folder_items(&[], "A.txt"), None);
        home.delete(&[], "Documents").unwrap();
        assert!(!home.root().join("Documents").exists());
        home.delete(&[], "A.txt").unwrap();
        assert_eq!(home.delete(&[], "A.txt"), Err(FsError::NotFound));
        assert_eq!(
            names(&home.list(&[]).unwrap()),
            ["downloads", "Journal", "bee.txt"]
        );
    }

    #[test]
    fn reading_text_binary_and_large_files() {
        let (_dir, home) = fixture();
        let file = home
            .read_text(&rel(&["Documents"]), "contacts.vcf")
            .unwrap();
        assert_eq!(file.text.as_deref(), Some("BEGIN:VCARD\n"));
        assert_eq!((file.size, file.truncated), (12, false));
        assert_eq!(home.read_text(&[], "Documents"), Err(FsError::NotAFile));
        assert_eq!(home.read_text(&[], "missing"), Err(FsError::NotFound));
        fs::write(home.root().join("blob.bin"), [0x7f, 0x45, 0x00, 0x01]).unwrap();
        assert_eq!(home.read_text(&[], "blob.bin").unwrap().text, None);
        fs::write(home.root().join("latin1.txt"), [b'c', b'a', 0xe9]).unwrap();
        assert_eq!(home.read_text(&[], "latin1.txt").unwrap().text, None);
        let big = "é".repeat(VIEW_LIMIT);
        fs::write(home.root().join("big.txt"), &big).unwrap();
        let file = home.read_text(&[], "big.txt").unwrap();
        assert!(file.truncated);
        assert_eq!(file.size, big.len() as u64);
        let text = file.text.unwrap();
        assert_eq!(text.len(), VIEW_LIMIT, "cut on a character boundary");
        assert!(text.chars().all(|c| c == 'é'));
    }

    #[test]
    fn sizes_read_as_short_decimal_units() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(999), "999 B");
        assert_eq!(human_size(1234), "1.2 KB");
        assert_eq!(human_size(56_000), "56 KB");
        assert_eq!(human_size(3_400_000_000), "3.4 GB");
    }
}
