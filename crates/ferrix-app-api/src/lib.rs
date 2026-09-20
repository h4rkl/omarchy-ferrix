//! Stable application contract.
//!
//! This crate is the application-side "stable waist": every application
//! runtime (Rust-native today; TypeScript/web and Android compatibility are
//! reserved kinds) is described, launched, and tracked through the same
//! identities, descriptors, lifecycle states, and typed errors. The
//! application service (`appd`), the compositor, the home surface, and
//! applications all depend on this crate; none of them depend on a runtime
//! implementation or its private launch configuration.
//!
//! Nothing here knows about QEMU, kernels, boards, or a particular container
//! or web engine. The internal adapter contract lives in `app-runtime-api` and
//! is composed only in `appd`'s bootstrap code.

use std::collections::BTreeSet;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Current schema version of every serialized type in this crate and of the
/// `appd` wire protocol built on it.
pub const SCHEMA_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// Launch environment contract
// ---------------------------------------------------------------------------

/// Environment variable carrying the launched application's [`AppId`].
pub const APP_ID_ENV: &str = "FERRIX_APP_ID";
/// Environment variable carrying the launched instance number.
pub const APP_INSTANCE_ENV: &str = "FERRIX_APP_INSTANCE";
/// Environment variable carrying the path of the `appd` socket.
pub const APPD_SOCKET_ENV: &str = "FERRIX_APPD_SOCKET";
/// Default `appd` socket path inside the guest.
pub const DEFAULT_APPD_SOCKET: &str = "/run/ferrix/appd.sock";
/// Directory of application descriptors inside the guest.
pub const GUEST_APP_DIR: &str = "/usr/share/ferrix/apps";
/// Directory of application descriptors when run from the repository root.
pub const REPO_APP_DIR: &str = "apps";

// ---------------------------------------------------------------------------
// Identity
// ---------------------------------------------------------------------------

/// A stable, globally namespaced application identifier, for example
/// `org.ferrix.terminal`.
///
/// The same id is used for launch, permissions, storage, notifications, and
/// crash records. Runtime-specific identifiers (an Android package name, a
/// web origin) are attributes of a descriptor, never a second identity.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct AppId(String);

impl AppId {
    pub const MAX_LEN: usize = 128;

    /// Parses and validates an id: two or more dot-separated segments of
    /// `[a-z0-9-]`, each starting with a letter or digit.
    pub fn new(text: impl Into<String>) -> Result<Self, AppError> {
        let text = text.into();
        Self::validate(&text).map_err(|reason| AppError::InvalidAppId {
            id: text.clone(),
            reason,
        })?;
        Ok(Self(text))
    }

    fn validate(text: &str) -> Result<(), String> {
        if text.is_empty() {
            return Err("empty".to_owned());
        }
        if text.len() > Self::MAX_LEN {
            return Err(format!("longer than {} bytes", Self::MAX_LEN));
        }
        let segments: Vec<&str> = text.split('.').collect();
        if segments.len() < 2 {
            return Err("needs at least two dot-separated segments".to_owned());
        }
        for segment in segments {
            if segment.is_empty() {
                return Err("empty segment".to_owned());
            }
            let mut chars = segment.chars();
            let first = chars.next().unwrap_or('.');
            if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
                return Err(format!("segment {segment:?} must start with [a-z0-9]"));
            }
            if !segment
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            {
                return Err(format!("segment {segment:?} may only contain [a-z0-9-]"));
            }
        }
        Ok(())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AppId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for AppId {
    type Err = AppError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl TryFrom<String> for AppId {
    type Error = AppError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<AppId> for String {
    fn from(id: AppId) -> Self {
        id.0
    }
}

// ---------------------------------------------------------------------------
// Descriptor
// ---------------------------------------------------------------------------

/// The runtime an application belongs to. Every kind shares one identity and
/// lifecycle model; only the launch mechanism differs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeKind {
    /// An independent Wayland client started as a supervised process.
    Native,
    /// A TypeScript/web bundle in the system-owned web runtime (reserved).
    Web,
    /// An APK in the contained Android compatibility runtime (reserved).
    Android,
}

impl RuntimeKind {
    pub fn name(self) -> &'static str {
        match self {
            RuntimeKind::Native => "native",
            RuntimeKind::Web => "web",
            RuntimeKind::Android => "android",
        }
    }
}

impl fmt::Display for RuntimeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Shell role of an application. Exactly one catalog entry may be the home
/// surface; it is auto-started and restarted, and edge gestures return to it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AppRole {
    #[default]
    Application,
    Home,
}

/// Trust ring of the code behind a descriptor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TrustLevel {
    /// Part of the platform itself (compositor, core services).
    System,
    /// First-party surfaces allowed to use privileged core APIs (home).
    Privileged,
    /// Ordinary applications; only public portals are intended for them.
    #[default]
    Untrusted,
}

/// Capabilities an application declares. Declaration is a contract for
/// discovery and future permission enforcement; it does not grant anything by
/// itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AppCapability {
    /// Read hardware state and events from the core service.
    CoreState,
    /// Issue product hardware commands (Wi-Fi, Bluetooth, brightness).
    CoreControl,
    /// Spawn an interactive user shell.
    Shell,
    /// Keep running while not in the foreground.
    Background,
}

/// Visual tone of a launcher icon, mapped to the product palette by the home
/// surface. Descriptors never carry raw colors.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IconTone {
    Blue,
    Green,
    Purple,
    Cyan,
    Yellow,
    Orange,
    Red,
    #[default]
    Neutral,
}

/// A launcher icon: a short glyph plus a palette tone.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IconRef {
    /// One to three characters drawn as the tile glyph.
    pub glyph: String,
    #[serde(default)]
    pub tone: IconTone,
}

/// A versioned application descriptor. One file per application in the
/// descriptor directory, serialized as JSON. Runtime-private launch data is
/// deliberately not part of this public type.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppDescriptor {
    pub schema_version: u32,
    pub id: AppId,
    pub runtime: RuntimeKind,
    pub name: String,
    #[serde(default)]
    pub icon: IconRef,
    /// Whether the home surface may start it. Reference entries that only
    /// have an illustrative in-process page are not launchable.
    #[serde(default = "default_true")]
    pub launchable: bool,
    /// Start when `appd` starts (the home surface is always auto-started).
    #[serde(default)]
    pub autostart: bool,
    #[serde(default)]
    pub role: AppRole,
    #[serde(default)]
    pub trust: TrustLevel,
    #[serde(default)]
    pub capabilities: BTreeSet<AppCapability>,
}

fn default_true() -> bool {
    true
}

impl AppDescriptor {
    /// Parses a descriptor from JSON and validates it.
    pub fn from_json(text: &str) -> Result<Self, AppError> {
        let descriptor: Self =
            serde_json::from_str(text).map_err(|e| AppError::InvalidDescriptor {
                id: None,
                reason: e.to_string(),
            })?;
        descriptor.validate()?;
        Ok(descriptor)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    /// Semantic checks beyond deserialization.
    pub fn validate(&self) -> Result<(), AppError> {
        let invalid = |reason: String| AppError::InvalidDescriptor {
            id: Some(self.id.clone()),
            reason,
        };
        if self.schema_version != SCHEMA_VERSION {
            return Err(invalid(format!(
                "schema_version {} is not {SCHEMA_VERSION}",
                self.schema_version
            )));
        }
        if self.name.trim().is_empty() {
            return Err(invalid("name is empty".to_owned()));
        }
        if self.icon.glyph.chars().count() > 3 {
            return Err(invalid(
                "icon glyph is longer than three characters".to_owned(),
            ));
        }
        if self.role == AppRole::Home && !self.launchable {
            return Err(invalid("the home surface must be launchable".to_owned()));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Lifecycle
// ---------------------------------------------------------------------------

/// Observable lifecycle states shared by every runtime.
///
/// ```text
/// not-running -> starting -> foreground <-> background -> frozen
///                    \                              /
///                     -> crashed / terminated <----
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LifecycleState {
    #[default]
    NotRunning,
    Starting,
    Foreground,
    Background,
    Frozen,
    /// Exited abnormally: killed by a signal or a non-zero exit status.
    Crashed,
    /// Exited normally or was stopped on request.
    Terminated,
}

impl LifecycleState {
    pub fn name(self) -> &'static str {
        match self {
            LifecycleState::NotRunning => "not-running",
            LifecycleState::Starting => "starting",
            LifecycleState::Foreground => "foreground",
            LifecycleState::Background => "background",
            LifecycleState::Frozen => "frozen",
            LifecycleState::Crashed => "crashed",
            LifecycleState::Terminated => "terminated",
        }
    }

    /// True while a process exists for the instance.
    pub fn is_running(self) -> bool {
        matches!(
            self,
            LifecycleState::Starting
                | LifecycleState::Foreground
                | LifecycleState::Background
                | LifecycleState::Frozen
        )
    }

    /// Whether `self -> next` is a legal transition.
    #[allow(clippy::match_like_matches_macro)] // one arm per edge reads as the state machine
    pub fn can_transition_to(self, next: LifecycleState) -> bool {
        use LifecycleState::*;
        match (self, next) {
            (NotRunning | Crashed | Terminated, Starting) => true,
            (Starting, Foreground | Background) => true,
            (Foreground, Background) | (Background, Foreground) => true,
            (Background, Frozen) | (Frozen, Background | Foreground) => true,
            (Starting | Foreground | Background | Frozen, Crashed | Terminated) => true,
            _ => false,
        }
    }

    /// Applies a transition or returns the typed error.
    pub fn transition(self, next: LifecycleState) -> Result<LifecycleState, AppError> {
        if self.can_transition_to(next) {
            Ok(next)
        } else {
            Err(AppError::InvalidTransition {
                from: self,
                to: next,
            })
        }
    }
}

impl fmt::Display for LifecycleState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// How a process ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ExitReason {
    Exited { code: i32 },
    Signaled { signal: i32 },
}

impl ExitReason {
    /// Signals and non-zero exits are crashes; exit code 0 is a clean exit.
    pub fn is_crash(self) -> bool {
        match self {
            ExitReason::Exited { code } => code != 0,
            ExitReason::Signaled { .. } => true,
        }
    }

    /// The lifecycle state this exit leads to.
    pub fn final_state(self) -> LifecycleState {
        if self.is_crash() {
            LifecycleState::Crashed
        } else {
            LifecycleState::Terminated
        }
    }

    /// Shell convention: `128 + signal` for a signal death.
    pub fn shell_status(self) -> i32 {
        match self {
            ExitReason::Exited { code } => code,
            ExitReason::Signaled { signal } => 128 + signal,
        }
    }

    /// Inverse of [`ExitReason::shell_status`] for statuses forwarded by a
    /// supervisor process.
    pub fn from_shell_status(status: i32) -> Self {
        if status > 128 && status < 128 + 65 {
            ExitReason::Signaled {
                signal: status - 128,
            }
        } else {
            ExitReason::Exited { code: status }
        }
    }
}

impl fmt::Display for ExitReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExitReason::Exited { code } => write!(f, "exit code {code}"),
            ExitReason::Signaled { signal } => write!(f, "signal {signal}"),
        }
    }
}

/// One tracked run of an application.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppInstance {
    pub app: AppId,
    pub runtime: RuntimeKind,
    pub role: AppRole,
    /// Monotonic per-service launch counter; distinguishes relaunches.
    pub instance: u32,
    pub state: LifecycleState,
    /// Process id in the service's namespace, when the runtime has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit: Option<ExitReason>,
    /// Whether a compositor surface has been associated with the instance.
    #[serde(default)]
    pub surface_mapped: bool,
}

/// Events published by `appd` to subscribers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "kebab-case")]
pub enum AppEvent {
    /// An instance changed state (including start, crash, and exit).
    Lifecycle { instance: AppInstance },
    /// The catalog was (re)loaded.
    CatalogChanged { apps: usize },
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Typed failures. Unsupported operations are errors, never silent successes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum AppError {
    InvalidAppId {
        id: String,
        reason: String,
    },
    InvalidDescriptor {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<AppId>,
        reason: String,
    },
    DuplicateApp {
        id: AppId,
    },
    UnknownApp {
        id: AppId,
    },
    NotLaunchable {
        id: AppId,
    },
    AlreadyRunning {
        id: AppId,
        instance: u32,
    },
    NotRunning {
        id: AppId,
    },
    /// No runtime is composed for this kind.
    RuntimeUnsupported {
        runtime: RuntimeKind,
    },
    InvalidTransition {
        from: LifecycleState,
        to: LifecycleState,
    },
    LaunchFailed {
        id: AppId,
        reason: String,
    },
    /// Required isolation could not be established.
    IsolationUnavailable {
        id: AppId,
        reason: String,
    },
    Io {
        reason: String,
    },
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::InvalidAppId { id, reason } => write!(f, "invalid app id {id:?}: {reason}"),
            AppError::InvalidDescriptor {
                id: Some(id),
                reason,
            } => {
                write!(f, "invalid descriptor for {id}: {reason}")
            }
            AppError::InvalidDescriptor { id: None, reason } => {
                write!(f, "invalid descriptor: {reason}")
            }
            AppError::DuplicateApp { id } => write!(f, "duplicate app {id}"),
            AppError::UnknownApp { id } => write!(f, "unknown app {id}"),
            AppError::NotLaunchable { id } => write!(f, "app {id} is not launchable"),
            AppError::AlreadyRunning { id, instance } => {
                write!(f, "app {id} is already running (instance {instance})")
            }
            AppError::NotRunning { id } => write!(f, "app {id} is not running"),
            AppError::RuntimeUnsupported { runtime } => {
                write!(f, "no {runtime} runtime is available")
            }
            AppError::InvalidTransition { from, to } => {
                write!(f, "invalid lifecycle transition {from} -> {to}")
            }
            AppError::LaunchFailed { id, reason } => write!(f, "launch of {id} failed: {reason}"),
            AppError::IsolationUnavailable { id, reason } => {
                write!(f, "required isolation for {id} unavailable: {reason}")
            }
            AppError::Io { reason } => write!(f, "i/o error: {reason}"),
        }
    }
}

impl std::error::Error for AppError {}

impl From<std::io::Error> for AppError {
    fn from(error: std::io::Error) -> Self {
        AppError::Io {
            reason: error.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_ids_are_reverse_dns_like() {
        assert!(AppId::new("org.ferrix.terminal").is_ok());
        assert!(AppId::new("com.example.app-2").is_ok());
        for bad in [
            "",
            "terminal",
            "Org.Ferrix",
            "org..ferrix",
            "org.-x",
            "org.ferrix.a_b",
        ] {
            let err = AppId::new(bad).expect_err(bad);
            assert!(matches!(err, AppError::InvalidAppId { .. }), "{bad}");
        }
        let long = format!("org.{}", "a".repeat(AppId::MAX_LEN));
        assert!(AppId::new(long).is_err());
    }

    #[test]
    fn app_id_serializes_as_a_plain_string_and_rejects_bad_input() {
        let id: AppId = serde_json::from_str("\"org.ferrix.home\"").unwrap();
        assert_eq!(id.as_str(), "org.ferrix.home");
        assert_eq!(serde_json::to_string(&id).unwrap(), "\"org.ferrix.home\"");
        assert!(serde_json::from_str::<AppId>("\"Nope\"").is_err());
    }

    fn descriptor() -> AppDescriptor {
        AppDescriptor {
            schema_version: SCHEMA_VERSION,
            id: AppId::new("org.ferrix.terminal").unwrap(),
            runtime: RuntimeKind::Native,
            name: "Terminal".to_owned(),
            icon: IconRef {
                glyph: ">_".to_owned(),
                tone: IconTone::Purple,
            },
            launchable: true,
            autostart: false,
            role: AppRole::Application,
            trust: TrustLevel::Untrusted,
            capabilities: [AppCapability::Shell].into_iter().collect(),
        }
    }

    #[test]
    fn descriptor_round_trips_through_json_with_defaults() {
        let text = descriptor().to_json();
        let parsed = AppDescriptor::from_json(&text).unwrap();
        assert_eq!(parsed, descriptor());

        let minimal = r#"{
            "schema_version": 1,
            "id": "org.ferrix.files",
            "runtime": "native",
            "name": "Files",
            "launchable": false
        }"#;
        let parsed = AppDescriptor::from_json(minimal).unwrap();
        assert_eq!(parsed.role, AppRole::Application);
        assert_eq!(parsed.trust, TrustLevel::Untrusted);
    }

    #[test]
    fn descriptor_validation_is_typed() {
        let mut d = descriptor();
        d.schema_version = 2;
        assert!(d.validate().is_err());

        let mut d = descriptor();
        d.runtime = RuntimeKind::Android;
        assert!(d.validate().is_ok());

        assert!(matches!(
            AppDescriptor::from_json("{"),
            Err(AppError::InvalidDescriptor { id: None, .. })
        ));
    }

    #[test]
    fn lifecycle_transitions_follow_the_state_machine() {
        use LifecycleState::*;
        assert_eq!(NotRunning.transition(Starting), Ok(Starting));
        assert_eq!(Starting.transition(Foreground), Ok(Foreground));
        assert_eq!(Foreground.transition(Background), Ok(Background));
        assert_eq!(Background.transition(Frozen), Ok(Frozen));
        assert_eq!(Frozen.transition(Foreground), Ok(Foreground));
        assert_eq!(Foreground.transition(Crashed), Ok(Crashed));
        assert_eq!(Crashed.transition(Starting), Ok(Starting));
        assert_eq!(
            NotRunning.transition(Foreground),
            Err(AppError::InvalidTransition {
                from: NotRunning,
                to: Foreground
            })
        );
        assert!(Crashed.transition(Background).is_err());
        assert!(Terminated.transition(Crashed).is_err());
        assert!(Foreground.is_running());
        assert!(!Crashed.is_running());
    }

    #[test]
    fn exit_reasons_classify_crashes_and_round_trip_shell_statuses() {
        assert!(!ExitReason::Exited { code: 0 }.is_crash());
        assert!(ExitReason::Exited { code: 3 }.is_crash());
        assert!(ExitReason::Signaled { signal: 11 }.is_crash());
        assert_eq!(
            ExitReason::Exited { code: 0 }.final_state(),
            LifecycleState::Terminated
        );
        assert_eq!(
            ExitReason::Signaled { signal: 9 }.final_state(),
            LifecycleState::Crashed
        );
        for reason in [
            ExitReason::Exited { code: 0 },
            ExitReason::Exited { code: 7 },
            ExitReason::Signaled { signal: 11 },
            ExitReason::Signaled { signal: 15 },
        ] {
            assert_eq!(ExitReason::from_shell_status(reason.shell_status()), reason);
        }
        assert_eq!(
            serde_json::to_string(&ExitReason::Signaled { signal: 11 }).unwrap(),
            r#"{"kind":"signaled","signal":11}"#
        );
    }
}

#[cfg(test)]
mod repo_descriptors {
    use super::*;

    /// The checked-in descriptors under `apps/` must always parse.
    #[test]
    fn checked_in_descriptors_are_valid_and_unique() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps");
        let mut ids = std::collections::BTreeSet::new();
        let mut homes = 0;
        for entry in std::fs::read_dir(&dir).expect("apps dir") {
            let path = entry.unwrap().path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            let descriptor = AppDescriptor::from_json(&text)
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            assert_eq!(
                path.file_stem().and_then(|s| s.to_str()),
                Some(descriptor.id.as_str()),
                "file name must equal the app id"
            );
            assert!(
                ids.insert(descriptor.id.clone()),
                "duplicate {}",
                descriptor.id
            );
            if descriptor.role == AppRole::Home {
                homes += 1;
            }
        }
        assert!(ids.len() >= 2, "expected at least home and terminal");
        assert_eq!(homes, 1, "exactly one home surface");
    }
}
