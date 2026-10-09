//! Shared on-screen keyboard preferences: the data-driven letter layouts,
//! the per-user keyboard configuration file, and the key-repeat timer.
//!
//! - **Layouts** are TOML files under `crates/ferrix-ui/layouts/`, embedded at
//!   build time. Each one names only its three letter rows; the symbol pages,
//!   modifiers, space/enter row, and terminal accessory row stay shared, so a
//!   new layout is a data file plus one line in [`BUILTIN_LAYOUTS`]. Every
//!   layout must contain `a`-`z` so all printable ASCII stays reachable.
//! - **Configuration** lives in `keyboard.toml` (see [`config_path`]) in the
//!   user's durable config directory, which every sandboxed app inherits
//!   (the sandbox replaces only `/proc`, `/tmp`, and `/run`). Settings writes
//!   the `layout` key; the repeat timings are hand-editable. Apps pick up a
//!   change on their next frame through [`refresh`], which stats the file at
//!   most twice a second.
//! - **Repeat** ([`KeyRepeat`]) is a pure timer: a held repeatable key fires
//!   once after the delay and then at the interval; releasing it after it
//!   fired suppresses the ordinary tap so the count is exact. Surfaces arm it
//!   on a key press and poll it from [`crate::client::ClientHandler::timer`].
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};
use std::time::{Duration, Instant, SystemTime};

use serde::Deserialize;

use crate::evidence::{emit, json_escape, label};
use crate::osk::Key;

/// Environment variable naming an explicit configuration file.
pub const CONFIG_ENV: &str = "FERRIX_KEYBOARD_CONFIG";
/// Default layout id.
pub const DEFAULT_LAYOUT: &str = "us";
/// Defaults close to the phone-keyboard convention: a deliberate hold, then
/// about 20 repeats per second.
pub const DEFAULT_REPEAT_DELAY_MS: u64 = 400;
pub const DEFAULT_REPEAT_INTERVAL_MS: u64 = 50;
/// Bounds for hand-edited values; out-of-range numbers are clamped.
const DELAY_RANGE_MS: (u64, u64) = (150, 2000);
const INTERVAL_RANGE_MS: (u64, u64) = (16, 500);
/// At most this many repeats are delivered per poll, so a stalled frame
/// (slow TCG emulation) cannot dump a burst of deletes.
const MAX_REPEATS_PER_POLL: u32 = 4;
/// Minimum time between configuration-file checks.
const REFRESH_INTERVAL: Duration = Duration::from_millis(500);

/// The embedded layout files, in the order Settings lists them.
pub const BUILTIN_LAYOUTS: [&str; 3] = [
    include_str!("../layouts/us.toml"),
    include_str!("../layouts/de.toml"),
    include_str!("../layouts/sv.toml"),
];

/// One letter layer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LetterLayout {
    pub id: String,
    pub name: String,
    pub rows: [Vec<char>; 3],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LayoutFile {
    schema_version: u32,
    id: String,
    name: String,
    letters: Vec<String>,
}

impl LetterLayout {
    /// Parses and validates one layout file.
    pub fn from_toml(text: &str) -> Result<Self, String> {
        let file: LayoutFile = toml::from_str(text).map_err(|e| e.to_string())?;
        if file.schema_version != 1 {
            return Err(format!(
                "unsupported schema_version {}",
                file.schema_version
            ));
        }
        let valid_id = !file.id.is_empty()
            && file
                .id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if !valid_id {
            return Err(format!("invalid layout id {:?}", file.id));
        }
        let [r1, r2, r3]: [String; 3] = file
            .letters
            .try_into()
            .map_err(|_| "letters must list exactly three rows".to_owned())?;
        let rows = [r1, r2, r3].map(|row| row.chars().collect::<Vec<char>>());
        for (index, row) in rows.iter().enumerate() {
            if row.is_empty() || row.len() > 12 {
                return Err(format!("row {} must hold 1-12 letters", index + 1));
            }
            if row.iter().any(|c| !c.is_alphabetic() || c.is_uppercase()) {
                return Err(format!("row {} must hold lowercase letters", index + 1));
            }
        }
        if let Some(missing) = ('a'..='z').find(|c| !rows.iter().any(|row| row.contains(c))) {
            return Err(format!("layout lacks {missing:?}"));
        }
        Ok(Self {
            id: file.id,
            name: file.name,
            rows,
        })
    }

    /// The longest letter row; it sets the letter key width.
    pub fn widest_row(&self) -> usize {
        self.rows.iter().map(Vec::len).max().unwrap_or(10)
    }
}

/// Every built-in layout, parsed once. A broken embedded file is a build
/// defect caught by the unit tests; at runtime it is skipped.
pub fn layouts() -> &'static [LetterLayout] {
    static LAYOUTS: OnceLock<Vec<LetterLayout>> = OnceLock::new();
    LAYOUTS.get_or_init(|| {
        BUILTIN_LAYOUTS
            .iter()
            .filter_map(|text| LetterLayout::from_toml(text).ok())
            .collect()
    })
}

/// The layout with `id`, if built in.
pub fn layout(id: &str) -> Option<&'static LetterLayout> {
    layouts().iter().find(|l| l.id == id)
}

/// The US layout, which every other lookup falls back to.
pub fn us() -> &'static LetterLayout {
    layout(DEFAULT_LAYOUT)
        .or_else(|| layouts().first())
        .expect("the embedded US layout parses")
}

/// Key-repeat timing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RepeatConfig {
    pub delay: Duration,
    pub interval: Duration,
}

impl Default for RepeatConfig {
    fn default() -> Self {
        Self {
            delay: Duration::from_millis(DEFAULT_REPEAT_DELAY_MS),
            interval: Duration::from_millis(DEFAULT_REPEAT_INTERVAL_MS),
        }
    }
}

/// Parsed `keyboard.toml`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyboardConfig {
    pub layout: String,
    pub repeat: RepeatConfig,
}

impl Default for KeyboardConfig {
    fn default() -> Self {
        Self {
            layout: DEFAULT_LAYOUT.to_owned(),
            repeat: RepeatConfig::default(),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigFile {
    schema_version: Option<u32>,
    layout: Option<String>,
    repeat_delay_ms: Option<u64>,
    repeat_interval_ms: Option<u64>,
}

impl KeyboardConfig {
    /// Parses the file. Unknown layout ids are rejected so a typo reads as an
    /// error instead of silently typing US letters.
    pub fn from_toml(text: &str) -> Result<Self, String> {
        let file: ConfigFile = toml::from_str(text).map_err(|e| e.to_string())?;
        if let Some(version) = file.schema_version
            && version != 1
        {
            return Err(format!("unsupported schema_version {version}"));
        }
        let layout = file.layout.unwrap_or_else(|| DEFAULT_LAYOUT.to_owned());
        if self::layout(&layout).is_none() {
            return Err(format!("unknown layout {layout:?}"));
        }
        let clamp = |v: Option<u64>, default: u64, (lo, hi): (u64, u64)| {
            Duration::from_millis(v.unwrap_or(default).clamp(lo, hi))
        };
        Ok(Self {
            layout,
            repeat: RepeatConfig {
                delay: clamp(
                    file.repeat_delay_ms,
                    DEFAULT_REPEAT_DELAY_MS,
                    DELAY_RANGE_MS,
                ),
                interval: clamp(
                    file.repeat_interval_ms,
                    DEFAULT_REPEAT_INTERVAL_MS,
                    INTERVAL_RANGE_MS,
                ),
            },
        })
    }

    /// The file contents for this configuration.
    pub fn to_toml(&self) -> String {
        format!(
            "# Ferrix on-screen keyboard. Settings > Keyboard writes `layout`;\n\
             # repeat timings are in milliseconds.\n\
             schema_version = 1\n\
             layout = \"{}\"\n\
             repeat_delay_ms = {}\n\
             repeat_interval_ms = {}\n",
            self.layout,
            self.repeat.delay.as_millis(),
            self.repeat.interval.as_millis()
        )
    }

    pub fn letters(&self) -> &'static LetterLayout {
        layout(&self.layout).unwrap_or_else(us)
    }
}

/// Where `keyboard.toml` lives: `$FERRIX_KEYBOARD_CONFIG`, else
/// `$XDG_CONFIG_HOME/ferrix/keyboard.toml`, else
/// `$HOME/.config/ferrix/keyboard.toml`. `None` without any of them.
pub fn config_path() -> Option<PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
    if let Some(path) = var(CONFIG_ENV) {
        return Some(PathBuf::from(path));
    }
    let base = var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| var("HOME").map(|home| Path::new(&home).join(".config")))?;
    Some(base.join("ferrix").join("keyboard.toml"))
}

/// Reads the configuration at `path`: a missing file is the default; an
/// unreadable or invalid one is the default plus the error.
pub fn read_config(path: &Path) -> (KeyboardConfig, Option<String>) {
    match std::fs::read_to_string(path) {
        Ok(text) => match KeyboardConfig::from_toml(&text) {
            Ok(config) => (config, None),
            Err(error) => (KeyboardConfig::default(), Some(error)),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            (KeyboardConfig::default(), None)
        }
        Err(error) => (KeyboardConfig::default(), Some(error.to_string())),
    }
}

/// Writes `config` to `path` atomically (temp file + rename), creating the
/// directory.
pub fn write_config(path: &Path, config: &KeyboardConfig) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, config.to_toml()).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

struct Active {
    config: KeyboardConfig,
    stamp: Option<(PathBuf, Option<SystemTime>)>,
    checked: Option<Instant>,
}

fn active_lock() -> &'static RwLock<Active> {
    static ACTIVE: OnceLock<RwLock<Active>> = OnceLock::new();
    ACTIVE.get_or_init(|| {
        RwLock::new(Active {
            config: KeyboardConfig::default(),
            stamp: None,
            checked: None,
        })
    })
}

/// The active configuration (the default until [`refresh`] finds a file).
pub fn active() -> KeyboardConfig {
    active_lock()
        .read()
        .map(|a| a.config.clone())
        .unwrap_or_default()
}

/// The active letter layout.
pub fn active_letters() -> &'static LetterLayout {
    let id = active_lock().read().map(|a| a.config.layout.clone());
    id.ok().and_then(|id| layout(&id)).unwrap_or_else(us)
}

/// Re-reads `keyboard.toml` when its modification time changed, at most
/// every [`REFRESH_INTERVAL`]. Returns whether the active configuration
/// changed; a change emits a `keyboard-config` record.
pub fn refresh() -> bool {
    let now = Instant::now();
    {
        let Ok(active) = active_lock().read() else {
            return false;
        };
        if active
            .checked
            .is_some_and(|at| now.duration_since(at) < REFRESH_INTERVAL)
        {
            return false;
        }
    }
    let path = config_path();
    let modified = path
        .as_deref()
        .and_then(|p| std::fs::metadata(p).ok())
        .and_then(|m| m.modified().ok());
    let stamp = path.clone().map(|p| (p, modified));
    let Ok(mut active) = active_lock().write() else {
        return false;
    };
    let first = active.checked.is_none();
    active.checked = Some(now);
    if !first && active.stamp == stamp {
        return false;
    }
    active.stamp = stamp;
    let (config, error) = match path.as_deref() {
        Some(p) => read_config(p),
        None => (KeyboardConfig::default(), None),
    };
    let source = match (&path, modified) {
        (Some(p), Some(_)) => p.display().to_string(),
        _ => "builtin".to_owned(),
    };
    if let Some(error) = &error {
        emit(format!(
            "{{\"evidence\":\"{}\",\"event\":\"keyboard-config-invalid\",\"source\":\"{}\",\"error\":\"{}\"}}",
            json_escape(label()),
            json_escape(&source),
            json_escape(error)
        ));
    }
    let changed = active.config != config;
    if changed || first {
        emit(format!(
            "{{\"evidence\":\"{}\",\"event\":\"keyboard-config\",\"layout\":\"{}\",\"repeat_delay_ms\":{},\"repeat_interval_ms\":{},\"source\":\"{}\"}}",
            json_escape(label()),
            json_escape(&config.layout),
            config.repeat.delay.as_millis(),
            config.repeat.interval.as_millis(),
            json_escape(&source)
        ));
    }
    active.config = config;
    changed
}

/// Keys that repeat while held: editing and cursor movement. Characters do
/// not repeat (a held letter on a phone keyboard is not a request for
/// `aaaa`), and modifiers, layer switches, and Enter never do.
pub fn repeats(key: Key) -> bool {
    matches!(
        key,
        Key::Backspace | Key::Delete | Key::Left | Key::Right | Key::Up | Key::Down | Key::Space
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Held {
    key: Key,
    next: Instant,
    fired: u32,
}

/// Repeat timer for one held on-screen key. Pure: callers pass the time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyRepeat {
    config: RepeatConfig,
    held: Option<Held>,
}

impl Default for KeyRepeat {
    fn default() -> Self {
        Self::new(RepeatConfig::default())
    }
}

impl KeyRepeat {
    pub fn new(config: RepeatConfig) -> Self {
        Self { config, held: None }
    }

    /// Uses the active configuration's timing for the next press.
    pub fn set_config(&mut self, config: RepeatConfig) {
        self.config = config;
    }

    /// A key went down: arms the timer for repeatable keys, clears it for
    /// everything else.
    pub fn press(&mut self, key: Key, now: Instant) {
        self.held = repeats(key).then_some(Held {
            key,
            next: now + self.config.delay,
            fired: 0,
        });
    }

    /// [`KeyRepeat::press`] with the active `keyboard.toml` timing.
    pub fn arm(&mut self, key: Key, now: Instant) {
        self.config = active().repeat;
        self.press(key, now);
    }

    /// The finger slid off the key or the contact was cancelled. Returns
    /// whether a key was held.
    pub fn cancel(&mut self) -> bool {
        self.held.take().is_some()
    }

    /// The key came up. Returns `true` when it already repeated, in which
    /// case the caller must not also deliver the ordinary tap.
    pub fn release(&mut self) -> bool {
        self.held.take().is_some_and(|h| h.fired > 0)
    }

    /// The held key, if any.
    pub fn key(&self) -> Option<Key> {
        self.held.map(|h| h.key)
    }

    /// When the next repeat is due.
    pub fn deadline(&self) -> Option<Instant> {
        self.held.map(|h| h.next)
    }

    /// Repeats due at `now` (at most a few per call), advancing the timer.
    pub fn poll(&mut self, now: Instant) -> Option<(Key, u32)> {
        let interval = self.config.interval.max(Duration::from_millis(1));
        let held = self.held.as_mut()?;
        if now < held.next {
            return None;
        }
        let late = now.duration_since(held.next);
        let due = 1 + (late.as_nanos() / interval.as_nanos()) as u64;
        let count = due.min(u64::from(MAX_REPEATS_PER_POLL)) as u32;
        // Skip what was dropped so the cadence restarts from now.
        held.next = if due > u64::from(count) {
            now + interval
        } else {
            held.next + interval * count
        };
        held.fired += count;
        Some((held.key, count))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_layouts_parse_and_cover_ascii_letters() {
        assert_eq!(layouts().len(), BUILTIN_LAYOUTS.len());
        assert_eq!(us().rows[0].iter().collect::<String>(), "qwertyuiop");
        let de = layout("de").expect("german layout");
        assert_eq!(de.rows[0].iter().collect::<String>(), "qwertzuiopü");
        assert_eq!(de.widest_row(), 11);
        assert!(layout("sv").is_some());
        assert!(layout("xx").is_none());
        let ty = crate::draw::Type::load().unwrap();
        for l in layouts() {
            for c in l.rows.iter().flatten() {
                let upper = crate::osk::upper(*c).to_string();
                assert!(
                    ty.has_glyphs(crate::draw::Weight::Regular, &format!("{c}{upper}")),
                    "{} {c}",
                    l.id
                );
            }
        }
    }

    #[test]
    fn layout_files_are_validated() {
        let ok = "schema_version = 1\nid = \"t\"\nname = \"T\"\nletters = [\"qwertyuiop\", \"asdfghjkl\", \"zxcvbnm\"]";
        assert!(LetterLayout::from_toml(ok).is_ok());
        let bad = [
            ok.replace("schema_version = 1", "schema_version = 2"),
            ok.replace("\"t\"", "\"T T\""),
            ok.replace(", \"zxcvbnm\"", ""),
            ok.replace("zxcvbnm", "zxcvbn"),
            ok.replace("zxcvbnm", "zxcvbnM"),
            ok.replace("zxcvbnm", "zxcvbnm1"),
            format!("{ok}\nextra = 1"),
        ];
        for text in bad {
            assert!(LetterLayout::from_toml(&text).is_err(), "{text}");
        }
    }

    #[test]
    fn config_parses_clamps_and_round_trips() {
        assert_eq!(
            KeyboardConfig::from_toml("").unwrap(),
            KeyboardConfig::default()
        );
        let c = KeyboardConfig::from_toml(
            "layout = \"de\"\nrepeat_delay_ms = 10\nrepeat_interval_ms = 90",
        )
        .unwrap();
        assert_eq!(c.layout, "de");
        assert_eq!(c.repeat.delay, Duration::from_millis(150), "clamped");
        assert_eq!(c.repeat.interval, Duration::from_millis(90));
        assert_eq!(c.letters().id, "de");
        assert_eq!(KeyboardConfig::from_toml(&c.to_toml()).unwrap(), c);
        assert!(KeyboardConfig::from_toml("layout = \"klingon\"").is_err());
        assert!(KeyboardConfig::from_toml("schema_version = 3").is_err());
        assert!(KeyboardConfig::from_toml("colour = 1").is_err());

        let dir = std::env::temp_dir().join(format!("ferrix-kbd-{}", std::process::id()));
        let path = dir.join("ferrix").join("keyboard.toml");
        assert_eq!(read_config(&path), (KeyboardConfig::default(), None));
        write_config(&path, &c).unwrap();
        assert_eq!(read_config(&path), (c, None));
        std::fs::write(&path, "layout = 3").unwrap();
        let (fallback, error) = read_config(&path);
        assert_eq!(fallback, KeyboardConfig::default());
        assert!(error.is_some());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn repeat_fires_after_the_delay_then_at_the_interval() {
        let config = RepeatConfig {
            delay: Duration::from_millis(400),
            interval: Duration::from_millis(50),
        };
        let t0 = Instant::now();
        let ms = |v: u64| t0 + Duration::from_millis(v);
        let mut r = KeyRepeat::new(config);
        r.press(Key::Backspace, t0);
        assert_eq!(r.deadline(), Some(ms(400)));
        assert_eq!(r.poll(ms(399)), None, "nothing before the delay");
        assert_eq!(r.poll(ms(400)), Some((Key::Backspace, 1)));
        assert_eq!(r.deadline(), Some(ms(450)));
        assert_eq!(r.poll(ms(449)), None);
        assert_eq!(r.poll(ms(455)), Some((Key::Backspace, 1)));
        // Two intervals late: both are delivered, the cadence is kept.
        assert_eq!(r.poll(ms(551)), Some((Key::Backspace, 2)));
        assert_eq!(r.deadline(), Some(ms(600)));
        // A long stall is capped and the cadence restarts from now.
        assert_eq!(
            r.poll(ms(2000)),
            Some((Key::Backspace, MAX_REPEATS_PER_POLL))
        );
        assert_eq!(r.deadline(), Some(ms(2050)));
        assert!(r.release(), "repeated, so the tap is suppressed");
        assert_eq!(r.deadline(), None);
        assert_eq!(r.poll(ms(5000)), None);
    }

    #[test]
    fn quick_taps_and_non_repeating_keys_do_not_repeat() {
        let t0 = Instant::now();
        let mut r = KeyRepeat::default();
        r.press(Key::Left, t0);
        assert!(!r.release(), "released before the delay: an ordinary tap");
        for key in [
            Key::Char('a'),
            Key::Enter,
            Key::Shift,
            Key::Layer,
            Key::Ctrl,
        ] {
            r.press(key, t0);
            assert_eq!(r.deadline(), None, "{key:?}");
            assert_eq!(r.poll(t0 + Duration::from_secs(5)), None);
            assert!(!r.release());
        }
        r.press(Key::Space, t0);
        assert!(r.cancel(), "sliding off cancels");
        assert!(!r.cancel());
        assert_eq!(r.poll(t0 + Duration::from_secs(5)), None);
    }
}
