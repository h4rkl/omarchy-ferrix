//! The Ferrix palette and the `colors.toml` theming contract.
//!
//! Ferrix's look is a black chassis, amber type, and one rust accent used
//! sparingly. Teal carries passive information; status colors stay purposeful.
//! All values are `0xRRGGBB`.
//!
//! | Field        | Role                                                    |
//! |--------------|---------------------------------------------------------|
//! | `bg`         | main background                                         |
//! | `bg_dark`    | bars, keyboard wells, terminal grid                     |
//! | `surface`    | cards, tiles                                            |
//! | `surface_hi` | pressed / raised / tracks                               |
//! | `border`     | hairlines                                               |
//! | `fg`         | primary text                                            |
//! | `fg_dim`     | secondary text, inactive glyphs                         |
//! | `rust`       | the accent: focus, active toggles, primary glyphs       |
//! | `rust_hi`    | pressed accent, cursor ring                             |
//! | `rust_dim`   | fills behind accent text, inactive accent               |
//! | `steel`      | secondary accent: network glyphs, links, "blue" tone    |
//! | `ok`         | charging / connected (verdigris)                        |
//! | `warn`       | warnings, exited-shell notes (amber)                    |
//! | `err`        | errors, crash banners (signal red)                      |
//! | `ansi`       | terminal palette, normal 0-7 then bright 8-15           |
//!
//! # Loading
//!
//! Call [`init`] once at process start. It reads the highest-precedence
//! configured source (explicit path, then `$FERRIX_COLORS`, then
//! `/etc/ferrix/colors.toml`), falls back to [`Theme::ferrix`], and reports
//! the outcome as evidence records: `theme-invalid` (with `source` and
//! `error`) when a configured file is missing or bad, then always
//! `theme-loaded` with `source` = `"builtin"` or the file path. [`get`]
//! returns the active theme anywhere afterwards, and the built-in palette when
//! `init` was never called.
//!
//! # `colors.toml`
//!
//! The flat top-level keys are those of Omarchy desktop theme files
//! (`themes/<name>/colors.toml` in the omarchy repository), so a desktop theme
//! loads unchanged. An optional `[ferrix]` table carries the accents the
//! desktop format has no key for, each with a documented fallback. The
//! canonical file is `themes/ferrix/colors.toml` at the repository root and
//! documents the full key set and mapping.

use std::ffi::OsStr;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::Deserialize;

use crate::evidence::{emit, json_escape};

/// Environment variable naming a `colors.toml` to load when [`init`] gets no
/// explicit path.
pub const ENV_VAR: &str = "FERRIX_COLORS";

/// System-wide `colors.toml`, used when neither an explicit path nor
/// [`ENV_VAR`] is set. Missing is normal and selects the built-in palette.
pub const SYSTEM_PATH: &str = "/etc/ferrix/colors.toml";

/// Canonical palette file, checked in at the repository root.
pub const CANONICAL_TOML: &str = include_str!("../../../themes/ferrix/colors.toml");

/// One complete palette. See the module docs for what each field is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    pub bg: u32,
    pub bg_dark: u32,
    pub surface: u32,
    pub surface_hi: u32,
    pub border: u32,
    pub fg: u32,
    pub fg_dim: u32,
    pub rust: u32,
    pub rust_hi: u32,
    pub rust_dim: u32,
    pub steel: u32,
    pub ok: u32,
    pub warn: u32,
    pub err: u32,
    /// Terminal ANSI colors: normal 0-7, bright 8-15.
    pub ansi: [u32; 16],
}

impl Theme {
    /// The built-in Ferrix palette, identical to `themes/ferrix/colors.toml`.
    pub const fn ferrix() -> Self {
        Self {
            bg: 0x000000,
            bg_dark: 0x000000,
            surface: 0x171311,
            surface_hi: 0x2a211b,
            border: 0x6f6868,
            fg: 0xe7aa5a,
            fg_dim: 0x948a8b,
            rust: 0xbd4924,
            rust_hi: 0xdf782d,
            rust_dim: 0x6b2411,
            steel: 0x66a891,
            ok: 0x66a891,
            warn: 0xf9cc6c,
            err: 0xfd6883,
            ansi: [
                0x000000, 0xfd6883, 0xdf782d, 0xf9cc6c, 0x66a891, 0xa8a9eb, 0xbd4924, 0xe6d9db,
                0x6f6868, 0xff8297, 0xc8e292, 0xfcd675, 0xf8a788, 0xbebffd, 0x9bf1e1, 0xf1e5e7,
            ],
        }
    }

    /// Parses a `colors.toml` document. Colors are `"#rrggbb"`.
    pub fn from_toml(text: &str) -> Result<Self, ThemeError> {
        let file: File =
            toml::from_str(text).map_err(|error| ThemeError::Parse(error.message().to_owned()))?;
        if file.schema_version != 1 {
            return Err(ThemeError::UnsupportedSchema(file.schema_version));
        }
        let ext = &file.ferrix;
        let color = |key: &str, value: &str| parse_hex(key, value);
        let extension = |key: &str, value: Option<&String>, fallback: u32| match value {
            Some(v) => parse_hex(&format!("ferrix.{key}"), v),
            None => Ok(fallback),
        };

        let accent = color("accent", &file.accent)?;
        let muted = color("muted", &file.muted)?;
        let red = color("red", &file.red)?;
        let yellow = color("yellow", &file.yellow)?;
        let green = color("green", &file.green)?;
        let blue = color("blue", &file.blue)?;
        let brown = file
            .brown
            .as_deref()
            .map(|v| color("brown", v))
            .transpose()?;
        let bg = color("background", &file.background)?;

        Ok(Self {
            bg,
            bg_dark: color("dark_background", &file.dark_background)?,
            surface: color("lighter_background", &file.lighter_background)?,
            surface_hi: color("selection", &file.selection)?,
            border: muted,
            fg: color("foreground", &file.foreground)?,
            fg_dim: color("dark_foreground", &file.dark_foreground)?,
            rust: accent,
            rust_hi: extension("rust_hi", ext.rust_hi.as_ref(), accent)?,
            rust_dim: extension("rust_dim", ext.rust_dim.as_ref(), brown.unwrap_or(accent))?,
            steel: extension("steel", ext.steel.as_ref(), blue)?,
            ok: extension("ok", ext.ok.as_ref(), green)?,
            warn: yellow,
            err: red,
            ansi: [
                bg,
                red,
                green,
                yellow,
                blue,
                color("magenta", &file.magenta)?,
                color("cyan", &file.cyan)?,
                color("light_foreground", &file.light_foreground)?,
                extension("bright_black", ext.bright_black.as_ref(), muted)?,
                color("bright_red", &file.bright_red)?,
                color("bright_green", &file.bright_green)?,
                color("bright_yellow", &file.bright_yellow)?,
                color("bright_blue", &file.bright_blue)?,
                color("bright_magenta", &file.bright_magenta)?,
                color("bright_cyan", &file.bright_cyan)?,
                color("bright_foreground", &file.bright_foreground)?,
            ],
        })
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::ferrix()
    }
}

/// Why a `colors.toml` was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeError {
    /// The file could not be read.
    Read(String),
    /// Not valid TOML, or a required key is missing.
    Parse(String),
    /// `schema_version` is not one this build understands.
    UnsupportedSchema(u32),
    /// A color value is not `#rrggbb`.
    Color { key: String, value: String },
}

impl fmt::Display for ThemeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(error) => write!(f, "read: {error}"),
            Self::Parse(error) => write!(f, "parse: {error}"),
            Self::UnsupportedSchema(version) => {
                write!(f, "unsupported schema_version {version} (expected 1)")
            }
            Self::Color { key, value } => write!(f, "{key}: {value:?} is not #rrggbb"),
        }
    }
}

impl std::error::Error for ThemeError {}

/// On-disk shape. Unknown top-level keys are allowed so desktop files with
/// keys Ferrix does not use (`mode`, `orange`, `darker_background`) load.
#[derive(Deserialize)]
struct File {
    #[serde(default = "default_schema_version")]
    schema_version: u32,
    accent: String,
    selection: String,
    muted: String,
    background: String,
    dark_background: String,
    lighter_background: String,
    foreground: String,
    dark_foreground: String,
    light_foreground: String,
    bright_foreground: String,
    red: String,
    yellow: String,
    green: String,
    cyan: String,
    blue: String,
    magenta: String,
    brown: Option<String>,
    bright_red: String,
    bright_yellow: String,
    bright_green: String,
    bright_cyan: String,
    bright_blue: String,
    bright_magenta: String,
    #[serde(default)]
    ferrix: Extension,
}

/// Ferrix-only accents. Unknown keys are rejected here so a typo does not
/// silently fall back.
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct Extension {
    steel: Option<String>,
    ok: Option<String>,
    rust_hi: Option<String>,
    rust_dim: Option<String>,
    bright_black: Option<String>,
}

fn default_schema_version() -> u32 {
    1
}

fn parse_hex(key: &str, value: &str) -> Result<u32, ThemeError> {
    let parsed = value
        .strip_prefix('#')
        .filter(|digits| digits.len() == 6 && digits.bytes().all(|b| b.is_ascii_hexdigit()))
        .and_then(|digits| u32::from_str_radix(digits, 16).ok());
    parsed.ok_or_else(|| ThemeError::Color {
        key: key.to_owned(),
        value: value.to_owned(),
    })
}

static BUILTIN: Theme = Theme::ferrix();
static ACTIVE: OnceLock<Theme> = OnceLock::new();

/// The active theme: what [`init`] loaded, or the built-in palette when
/// `init` was never called.
pub fn get() -> &'static Theme {
    ACTIVE.get().unwrap_or(&BUILTIN)
}

/// Loads the theme once and reports the outcome as evidence records.
///
/// `evidence` is the record label (`"mock"` or `"QEMU"`), owned by the
/// calling surface. `path` is an explicit file that wins over [`ENV_VAR`] and
/// [`SYSTEM_PATH`]. A configured file (explicit or env) that is missing or
/// invalid emits `theme-invalid` and selects the built-in palette; a missing
/// [`SYSTEM_PATH`] is silent. Every call after the first returns the theme
/// already active without touching the filesystem or emitting anything.
pub fn init(evidence: &str, path: Option<&Path>) -> &'static Theme {
    if let Some(active) = ACTIVE.get() {
        return active;
    }
    let outcome = load(
        path,
        std::env::var_os(ENV_VAR).as_deref(),
        Path::new(SYSTEM_PATH),
    );
    if ACTIVE.set(outcome.theme).is_ok() {
        if let Some((source, error)) = &outcome.error {
            emit(format!(
                "{{\"evidence\":\"{}\",\"event\":\"theme-invalid\",\"source\":\"{}\",\"error\":\"{}\"}}",
                json_escape(evidence),
                json_escape(&source.display().to_string()),
                json_escape(&error.to_string())
            ));
        }
        emit(format!(
            "{{\"evidence\":\"{}\",\"event\":\"theme-loaded\",\"source\":\"{}\"}}",
            json_escape(evidence),
            json_escape(&outcome.source_label())
        ));
    }
    ACTIVE.get_or_init(|| outcome.theme)
}

struct Outcome {
    theme: Theme,
    source: Option<PathBuf>,
    error: Option<(PathBuf, ThemeError)>,
}

impl Outcome {
    fn source_label(&self) -> String {
        self.source
            .as_ref()
            .map_or_else(|| "builtin".to_owned(), |p| p.display().to_string())
    }
}

/// Pure lookup: explicit path, then the env value, then the system path.
/// Explicit and env sources are required (missing is an error); the system
/// path is optional.
fn load(explicit: Option<&Path>, env: Option<&OsStr>, system: &Path) -> Outcome {
    let (candidate, required) = match (explicit, env) {
        (Some(path), _) => (path.to_path_buf(), true),
        (None, Some(value)) if !value.is_empty() => (PathBuf::from(value), true),
        _ => (system.to_path_buf(), false),
    };
    if !required && !candidate.is_file() {
        return Outcome {
            theme: Theme::ferrix(),
            source: None,
            error: None,
        };
    }
    match std::fs::read_to_string(&candidate)
        .map_err(|error| ThemeError::Read(error.to_string()))
        .and_then(|text| Theme::from_toml(&text))
    {
        Ok(theme) => Outcome {
            theme,
            source: Some(candidate),
            error: None,
        },
        Err(error) => Outcome {
            theme: Theme::ferrix(),
            source: None,
            error: Some((candidate, error)),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tokyo Night as shipped by the desktop, without `schema_version` or a
    /// `[ferrix]` table. Compatibility fixture, not a Ferrix palette.
    const DESKTOP_FILE: &str = r##"
mode = "dark"

accent = "#7aa2f7"
selection = "#292e42"
muted = "#414868"

background = "#1a1b26"
dark_background = "#13141c"
darker_background = "#0e0e14"
lighter_background = "#24283b"

foreground = "#a9b1d6"
dark_foreground = "#565f89"
light_foreground = "#b4bee6"
bright_foreground = "#c0caf5"

red = "#f7768e"
yellow = "#e0af68"
orange = "#eb927b"
green = "#9ece6a"
cyan = "#449dab"
blue = "#7aa2f7"
magenta = "#ad8ee6"
brown = "#75493d"

bright_red = "#ff7a93"
bright_yellow = "#ff9e64"
bright_green = "#b9f27c"
bright_cyan = "#0db9d7"
bright_blue = "#7da6ff"
bright_magenta = "#bb9af7"
"##;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("ferrix-ui-theme-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn write(&self, name: &str, text: &str) -> PathBuf {
            let path = self.0.join(name);
            std::fs::write(&path, text).unwrap();
            path
        }

        fn missing(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn canonical_file_matches_builtin() {
        assert_eq!(Theme::from_toml(CANONICAL_TOML).unwrap(), Theme::ferrix());
    }

    #[test]
    fn builtin_palette_is_self_consistent() {
        let t = Theme::ferrix();
        assert_eq!(t.ansi[0], t.bg);
        assert_eq!(t.ansi[1], t.err);
        assert_eq!(t.ansi[3], t.warn);
        assert_eq!(t, Theme::default());
        assert_eq!(*get(), t, "get() without init is the built-in");
    }

    #[test]
    fn desktop_theme_loads_with_documented_fallbacks() {
        let t = Theme::from_toml(DESKTOP_FILE).unwrap();
        assert_eq!(t.bg, 0x1a1b26);
        assert_eq!(t.rust, 0x7aa2f7);
        assert_eq!(t.rust_hi, t.rust, "rust_hi <- accent");
        assert_eq!(t.rust_dim, 0x75493d, "rust_dim <- brown");
        assert_eq!(t.steel, 0x7aa2f7, "steel <- blue");
        assert_eq!(t.ok, 0x9ece6a, "ok <- green");
        assert_eq!(t.ansi[8], 0x414868, "bright_black <- muted");
        assert_eq!(t.ansi[7], 0xb4bee6);
        assert_eq!(t.ansi[15], 0xc0caf5);
    }

    #[test]
    fn rust_dim_falls_back_to_accent_without_brown() {
        let text = DESKTOP_FILE.replace("brown = \"#75493d\"\n", "");
        let t = Theme::from_toml(&text).unwrap();
        assert_eq!(t.rust_dim, t.rust);
    }

    #[test]
    fn rejects_bad_hex() {
        for bad in ["#bd492", "bd4924", "#bd492g", "#bd492400", "rust"] {
            let text =
                CANONICAL_TOML.replace("accent = \"#bd4924\"", &format!("accent = \"{bad}\""));
            assert_eq!(
                Theme::from_toml(&text),
                Err(ThemeError::Color {
                    key: "accent".into(),
                    value: bad.into()
                }),
                "{bad}"
            );
        }
        let text = CANONICAL_TOML.replace("steel = \"#66a891\"", "steel = \"#66a89\"");
        assert_eq!(
            Theme::from_toml(&text),
            Err(ThemeError::Color {
                key: "ferrix.steel".into(),
                value: "#66a89".into()
            })
        );
    }

    #[test]
    fn rejects_missing_key() {
        let text = CANONICAL_TOML.replace("background = \"#000000\"\n", "");
        match Theme::from_toml(&text) {
            Err(ThemeError::Parse(message)) => assert!(
                message.contains("background"),
                "message names the key: {message}"
            ),
            other => panic!("expected a parse error, got {other:?}"),
        }
    }

    #[test]
    fn rejects_unknown_extension_key_and_bad_syntax() {
        let text = format!("{CANONICAL_TOML}\nstel = \"#000000\"\n");
        assert!(matches!(Theme::from_toml(&text), Err(ThemeError::Parse(_))));
        assert!(matches!(
            Theme::from_toml("accent = "),
            Err(ThemeError::Parse(_))
        ));
    }

    #[test]
    fn rejects_unsupported_schema_version() {
        let text = CANONICAL_TOML.replace("schema_version = 1", "schema_version = 2");
        assert_eq!(
            Theme::from_toml(&text),
            Err(ThemeError::UnsupportedSchema(2))
        );
    }

    #[test]
    fn lookup_order_explicit_then_env_then_system() {
        let dir = TempDir::new("lookup");
        let explicit = dir.write("explicit.toml", CANONICAL_TOML);
        let env = dir.write("env.toml", DESKTOP_FILE);
        let system = dir.write(
            "system.toml",
            &CANONICAL_TOML.replace("accent = \"#bd4924\"", "accent = \"#123456\""),
        );

        let out = load(Some(&explicit), Some(env.as_os_str()), &system);
        assert_eq!(out.theme, Theme::ferrix());
        assert_eq!(out.source.as_deref(), Some(explicit.as_path()));
        assert!(out.error.is_none());

        let out = load(None, Some(env.as_os_str()), &system);
        assert_eq!(out.theme.bg, 0x1a1b26);
        assert_eq!(out.source.as_deref(), Some(env.as_path()));

        let out = load(None, Some(OsStr::new("")), &system);
        assert_eq!(out.theme.rust, 0x123456, "empty env is unset");
        assert_eq!(out.source.as_deref(), Some(system.as_path()));
        assert_eq!(out.source_label(), system.display().to_string());
    }

    #[test]
    fn missing_system_file_is_silently_builtin() {
        let dir = TempDir::new("missing-system");
        let out = load(None, None, &dir.missing("absent.toml"));
        assert_eq!(out.theme, Theme::ferrix());
        assert!(out.source.is_none());
        assert!(out.error.is_none());
        assert_eq!(out.source_label(), "builtin");
    }

    #[test]
    fn missing_or_invalid_configured_file_reports_and_falls_back() {
        let dir = TempDir::new("invalid");
        let absent = dir.missing("absent.toml");
        let out = load(Some(&absent), None, &dir.missing("system.toml"));
        assert_eq!(out.theme, Theme::ferrix());
        assert!(out.source.is_none());
        match &out.error {
            Some((path, ThemeError::Read(_))) => assert_eq!(path, &absent),
            other => panic!("expected a read error, got {other:?}"),
        }

        let bad = dir.write("bad.toml", "accent = \"#zzzzzz\"");
        let out = load(None, Some(bad.as_os_str()), &dir.missing("system.toml"));
        assert_eq!(out.theme, Theme::ferrix());
        assert!(matches!(
            &out.error,
            Some((path, ThemeError::Parse(_))) if path == &bad
        ));
    }

    #[test]
    fn error_display_is_terse() {
        assert_eq!(
            ThemeError::Color {
                key: "accent".into(),
                value: "x".into()
            }
            .to_string(),
            "accent: \"x\" is not #rrggbb"
        );
        assert_eq!(
            ThemeError::UnsupportedSchema(3).to_string(),
            "unsupported schema_version 3 (expected 1)"
        );
    }
}
