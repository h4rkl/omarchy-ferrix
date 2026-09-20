//! Strict scenario-file schema (ADR-006): `targets/qemu-aarch64/scenarios/*.toml`.
//!
//! Unknown keys, unknown services, a wrong `schema_version`, an empty step
//! list, a command without `cmd`, or a zero timeout are hard errors. The file
//! never carries wall-clock assertions: `timeout_ms` only bounds how long the
//! runner waits for the expected records.

use std::fmt;
use std::path::Path;

use serde::Deserialize;
use serde_json::{Map, Value};

/// The only schema version this runner accepts.
pub const SCHEMA_VERSION: u32 = 1;

/// Default bound for one step when the file does not set `timeout_ms`.
pub const DEFAULT_TIMEOUT_MS: u64 = 5000;

/// Which mock-control channel a step talks to.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Service {
    Telephony,
    Network,
    Core,
}

impl Service {
    /// Host socket file name inside the session directory (ADR-006 table).
    pub fn socket_name(self) -> &'static str {
        match self {
            Service::Telephony => "telephony-control.sock",
            Service::Network => "network-control.sock",
            Service::Core => "control.sock",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Service::Telephony => "telephony",
            Service::Network => "network",
            Service::Core => "core",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    pub service: Service,
    /// The JSON command line sent to the channel (`cmd` plus arguments).
    pub command: Value,
    /// JSON subsets that must appear, in order, in records emitted after the
    /// command was sent.
    pub expect: Vec<Value>,
    /// JSON subset the ack itself must satisfy (optional).
    pub ack: Option<Value>,
    /// The command is expected to be rejected (`"status":"error"`).
    pub expect_ack_error: bool,
    pub timeout_ms: u64,
    /// Free-text note carried into the evidence record.
    pub note: Option<String>,
}

impl Step {
    pub fn cmd(&self) -> &str {
        self.command
            .get("cmd")
            .and_then(Value::as_str)
            .unwrap_or("?")
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Scenario {
    pub name: String,
    pub description: String,
    pub steps: Vec<Step>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScenarioError {
    Read { path: String, reason: String },
    Schema(String),
    Invalid(String),
}

impl fmt::Display for ScenarioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScenarioError::Read { path, reason } => {
                write!(f, "cannot read scenario {path}: {reason}")
            }
            ScenarioError::Schema(reason) => write!(f, "invalid scenario: {reason}"),
            ScenarioError::Invalid(reason) => write!(f, "invalid scenario value: {reason}"),
        }
    }
}

impl std::error::Error for ScenarioError {}

// Wire form of the file; converted to the public types after validation.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileScenario {
    schema_version: u32,
    name: String,
    description: String,
    #[serde(default, rename = "step")]
    steps: Vec<FileStep>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileStep {
    service: Service,
    command: toml::Table,
    #[serde(default)]
    expect: Vec<toml::Value>,
    #[serde(default)]
    ack: Option<toml::Table>,
    #[serde(default)]
    expect_ack_error: bool,
    #[serde(default)]
    timeout_ms: Option<u64>,
    #[serde(default)]
    note: Option<String>,
}

fn to_json(value: toml::Value) -> Result<Value, ScenarioError> {
    Ok(match value {
        toml::Value::String(s) => Value::String(s),
        toml::Value::Integer(i) => Value::from(i),
        toml::Value::Float(f) => serde_json::Number::from_f64(f)
            .map(Value::Number)
            .ok_or_else(|| ScenarioError::Invalid(format!("non-finite number {f}")))?,
        toml::Value::Boolean(b) => Value::Bool(b),
        toml::Value::Datetime(d) => {
            return Err(ScenarioError::Invalid(format!(
                "datetime {d} is not allowed; scenarios never carry wall-clock values"
            )));
        }
        toml::Value::Array(items) => {
            Value::Array(items.into_iter().map(to_json).collect::<Result<_, _>>()?)
        }
        toml::Value::Table(table) => Value::Object(table_to_json(table)?),
    })
}

fn table_to_json(table: toml::Table) -> Result<Map<String, Value>, ScenarioError> {
    let mut out = Map::new();
    for (key, value) in table {
        out.insert(key, to_json(value)?);
    }
    Ok(out)
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !name.starts_with('-')
        && !name.ends_with('-')
}

impl Scenario {
    /// Parses and validates a scenario document.
    pub fn parse(input: &str) -> Result<Self, ScenarioError> {
        let file: FileScenario = toml::from_str(input)
            .map_err(|e| ScenarioError::Schema(e.message().trim().to_owned()))?;
        if file.schema_version != SCHEMA_VERSION {
            return Err(ScenarioError::Invalid(format!(
                "unsupported schema_version {} (expected {SCHEMA_VERSION})",
                file.schema_version
            )));
        }
        if !valid_name(&file.name) {
            return Err(ScenarioError::Invalid(format!(
                "name {:?} must be lowercase letters, digits, and single dashes",
                file.name
            )));
        }
        if file.description.trim().is_empty() {
            return Err(ScenarioError::Invalid(
                "description must not be empty".to_owned(),
            ));
        }
        if file.steps.is_empty() {
            return Err(ScenarioError::Invalid(
                "a scenario needs at least one [[step]]".to_owned(),
            ));
        }
        let mut steps = Vec::with_capacity(file.steps.len());
        for (index, step) in file.steps.into_iter().enumerate() {
            let command = Value::Object(table_to_json(step.command)?);
            match command.get("cmd") {
                Some(Value::String(cmd)) if !cmd.is_empty() => {}
                _ => {
                    return Err(ScenarioError::Invalid(format!(
                        "step {index}: command needs a non-empty string `cmd`"
                    )));
                }
            }
            let timeout_ms = step.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS);
            if timeout_ms == 0 {
                return Err(ScenarioError::Invalid(format!(
                    "step {index}: timeout_ms must be > 0"
                )));
            }
            let mut expect = Vec::with_capacity(step.expect.len());
            for (i, value) in step.expect.into_iter().enumerate() {
                let value = to_json(value)?;
                if !value.is_object() {
                    return Err(ScenarioError::Invalid(format!(
                        "step {index}: expect[{i}] must be an inline table (JSON subset of a record)"
                    )));
                }
                expect.push(value);
            }
            let ack = step
                .ack
                .map(|table| table_to_json(table).map(Value::Object))
                .transpose()?;
            steps.push(Step {
                service: step.service,
                command,
                expect,
                ack,
                expect_ack_error: step.expect_ack_error,
                timeout_ms,
                note: step.note,
            });
        }
        Ok(Scenario {
            name: file.name,
            description: file.description,
            steps,
        })
    }

    /// Reads, parses, and validates the file at `path`. The file stem must
    /// equal `name` so `just qemu-scenario NAME` is unambiguous.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ScenarioError> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path).map_err(|e| ScenarioError::Read {
            path: path.display().to_string(),
            reason: e.to_string(),
        })?;
        let scenario = Self::parse(&text)?;
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        if stem != scenario.name {
            return Err(ScenarioError::Invalid(format!(
                "file stem {stem:?} does not match name {:?}",
                scenario.name
            )));
        }
        Ok(scenario)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"
schema_version = 1
name = "incoming-call-answer"
description = "Ring, answer, remote hangup."

[[step]]
service = "telephony"
command = { cmd = "incoming-call", number = "+15550100042" }
expect = [ { event = "telephony-event", kind = "call-changed", state = "ringing" } ]
timeout_ms = 5000

[[step]]
service = "telephony"
command = { cmd = "tick", ms = 1000 }

[[step]]
service = "network"
command = { cmd = "set-radio", enabled = false }
expect = [ { event = "network-event" }, { event = "network-event", radio = "disabled" } ]
expect_ack_error = false
note = "two records, in order"
"#;

    #[test]
    fn fixture_parses() {
        let s = Scenario::parse(FIXTURE).expect("fixture parses");
        assert_eq!(s.name, "incoming-call-answer");
        assert_eq!(s.steps.len(), 3);
        assert_eq!(s.steps[0].service, Service::Telephony);
        assert_eq!(s.steps[0].cmd(), "incoming-call");
        assert_eq!(s.steps[0].command["number"], "+15550100042");
        assert_eq!(s.steps[0].expect.len(), 1);
        assert_eq!(s.steps[0].timeout_ms, 5000);
        assert_eq!(s.steps[1].timeout_ms, DEFAULT_TIMEOUT_MS);
        assert!(s.steps[1].expect.is_empty());
        assert_eq!(s.steps[1].command["ms"], 1000);
        assert_eq!(s.steps[2].service, Service::Network);
        assert_eq!(s.steps[2].command["enabled"], false);
        assert_eq!(s.steps[2].expect.len(), 2);
        assert_eq!(s.steps[2].note.as_deref(), Some("two records, in order"));
    }

    #[test]
    fn unknown_top_level_key_is_rejected() {
        let text = FIXTURE.replace("description =", "author = \"x\"\ndescription =");
        let err = Scenario::parse(&text).expect_err("unknown key must fail");
        assert!(matches!(err, ScenarioError::Schema(_)), "{err}");
        assert!(err.to_string().contains("author"), "{err}");
    }

    #[test]
    fn unknown_step_key_is_rejected() {
        let text = FIXTURE.replace("timeout_ms = 5000", "timeout_ms = 5000\nretries = 3");
        let err = Scenario::parse(&text).expect_err("unknown step key must fail");
        assert!(matches!(err, ScenarioError::Schema(_)), "{err}");
        assert!(err.to_string().contains("retries"), "{err}");
    }

    #[test]
    fn unknown_service_is_rejected() {
        let text = FIXTURE.replace("service = \"network\"", "service = \"bluetooth\"");
        let err = Scenario::parse(&text).expect_err("unknown service must fail");
        assert!(matches!(err, ScenarioError::Schema(_)), "{err}");
    }

    #[test]
    fn wrong_schema_version_is_rejected() {
        let text = FIXTURE.replace("schema_version = 1", "schema_version = 2");
        let err = Scenario::parse(&text).expect_err("schema 2 must fail");
        assert!(
            err.to_string().contains("unsupported schema_version 2"),
            "{err}"
        );
    }

    #[test]
    fn command_without_cmd_is_rejected() {
        let text = FIXTURE.replace("{ cmd = \"tick\", ms = 1000 }", "{ ms = 1000 }");
        let err = Scenario::parse(&text).expect_err("missing cmd must fail");
        assert!(err.to_string().contains("`cmd`"), "{err}");
    }

    #[test]
    fn zero_timeout_and_empty_steps_are_rejected() {
        let text = FIXTURE.replace("timeout_ms = 5000", "timeout_ms = 0");
        assert!(
            Scenario::parse(&text)
                .unwrap_err()
                .to_string()
                .contains("timeout_ms")
        );
        let text = &FIXTURE[..FIXTURE.find("[[step]]").unwrap()];
        assert!(
            Scenario::parse(text)
                .unwrap_err()
                .to_string()
                .contains("[[step]]")
        );
    }

    #[test]
    fn bad_names_and_expect_shapes_are_rejected() {
        let text = FIXTURE.replace(
            "name = \"incoming-call-answer\"",
            "name = \"Incoming Call\"",
        );
        assert!(
            Scenario::parse(&text)
                .unwrap_err()
                .to_string()
                .contains("name")
        );
        let text = FIXTURE.replace(
            "expect = [ { event = \"network-event\" },",
            "expect = [ \"network-event\",",
        );
        assert!(
            Scenario::parse(&text)
                .unwrap_err()
                .to_string()
                .contains("inline table")
        );
    }

    #[test]
    fn datetimes_are_rejected() {
        let text = FIXTURE.replace("ms = 1000", "at = 2026-09-21T00:00:00Z");
        let err = Scenario::parse(&text).expect_err("datetime must fail");
        assert!(err.to_string().contains("wall-clock"), "{err}");
    }

    #[test]
    fn load_requires_matching_file_stem() {
        let dir = std::env::temp_dir().join(format!("ferrix-scenario-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let wrong = dir.join("other-name.toml");
        std::fs::write(&wrong, FIXTURE).unwrap();
        let err = Scenario::load(&wrong).expect_err("stem mismatch must fail");
        assert!(err.to_string().contains("file stem"), "{err}");
        let right = dir.join("incoming-call-answer.toml");
        std::fs::write(&right, FIXTURE).unwrap();
        Scenario::load(&right).expect("matching stem loads");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
