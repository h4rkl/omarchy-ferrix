//! Executes a [`Scenario`] against a running session.
//!
//! Guest mode drives the QEMU virtio-serial mock-control sockets and matches
//! records appended to the captured serial log (`QEMU` evidence). Nested mode
//! drives the daemons' Unix sockets and matches their combined stdout log
//! (`mock` evidence). Only ordering and bounded waits are asserted; no step
//! ever asserts elapsed time.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use ferrix_mock_control::Ack;
use serde_json::{Value, json};

use crate::matcher::{is_subset, match_in_order};
use crate::scenario::{Scenario, Service, Step};

/// Where the session lives and which log carries its records.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mode {
    Guest {
        /// Directory holding `control.sock`, `telephony-control.sock`,
        /// `network-control.sock`, and (agent sessions) `qmp.sock`.
        sockets: PathBuf,
        serial_log: PathBuf,
        /// Capture the framebuffer through this QMP helper after the last step.
        frame: Option<FrameCapture>,
    },
    Nested {
        sockets: PathBuf,
        log: PathBuf,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameCapture {
    pub qmp_socket: PathBuf,
    pub qmp_helper: PathBuf,
}

impl Mode {
    pub fn evidence(&self) -> &'static str {
        match self {
            Mode::Guest { .. } => "QEMU",
            Mode::Nested { .. } => "mock",
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Mode::Guest { .. } => "guest",
            Mode::Nested { .. } => "nested",
        }
    }

    fn sockets(&self) -> &Path {
        match self {
            Mode::Guest { sockets, .. } | Mode::Nested { sockets, .. } => sockets,
        }
    }

    fn log(&self) -> &Path {
        match self {
            Mode::Guest { serial_log, .. } => serial_log,
            Mode::Nested { log, .. } => log,
        }
    }

    pub fn socket_for(&self, service: Service) -> PathBuf {
        self.sockets().join(service.socket_name())
    }
}

/// Follows a log file from a byte offset and yields complete new lines.
struct LogTail {
    path: PathBuf,
    offset: u64,
    partial: Vec<u8>,
}

impl LogTail {
    fn at_end(path: &Path) -> Result<Self, String> {
        let offset = std::fs::metadata(path)
            .map(|m| m.len())
            .map_err(|e| format!("cannot stat {}: {e}", path.display()))?;
        Ok(LogTail {
            path: path.to_path_buf(),
            offset,
            partial: Vec::new(),
        })
    }

    fn new_lines(&mut self) -> Result<Vec<String>, String> {
        let mut file = File::open(&self.path)
            .map_err(|e| format!("cannot open {}: {e}", self.path.display()))?;
        let len = file
            .metadata()
            .map_err(|e| format!("cannot stat {}: {e}", self.path.display()))?
            .len();
        if len < self.offset {
            // Truncated behind us (a fresh session): start over from the top.
            self.offset = 0;
            self.partial.clear();
        }
        file.seek(SeekFrom::Start(self.offset))
            .map_err(|e| format!("cannot seek {}: {e}", self.path.display()))?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)
            .map_err(|e| format!("cannot read {}: {e}", self.path.display()))?;
        self.offset += buf.len() as u64;
        self.partial.extend_from_slice(&buf);
        let mut lines = Vec::new();
        while let Some(pos) = self.partial.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.partial.drain(..=pos).collect();
            let text = String::from_utf8_lossy(&line[..line.len() - 1]);
            lines.push(text.trim_end_matches('\r').to_owned());
        }
        Ok(lines)
    }
}

fn parse_record(line: &str) -> Option<Value> {
    // Serial lines may carry a kernel/console prefix before the JSON record.
    let start = line.find('{')?;
    serde_json::from_str::<Value>(&line[start..])
        .ok()
        .filter(Value::is_object)
}

fn send_with_retry(socket: &Path, command: &Value, timeout: Duration) -> Result<Ack, String> {
    let deadline = Instant::now() + timeout;
    loop {
        match ferrix_mock_control::send(socket, command, timeout) {
            Ok(ack) => return Ok(ack),
            Err(e) if e.starts_with("cannot connect") && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return Err(e),
        }
    }
}

#[derive(Debug)]
pub struct StepReport {
    pub index: usize,
    pub ack: Value,
    pub matched: Vec<Value>,
    pub elapsed_ms: u128,
}

#[derive(Debug)]
pub struct Outcome {
    pub scenario: String,
    pub passed: bool,
    pub failed_step: Option<usize>,
    pub reason: Option<String>,
    pub steps: Vec<StepReport>,
    pub excerpt: Vec<String>,
    pub frame: Option<Value>,
    pub evidence_dir: PathBuf,
}

impl Outcome {
    /// The one machine-readable record the runner prints.
    pub fn record(&self, mode: &Mode) -> Value {
        let mut record = json!({
            "evidence": mode.evidence(),
            "event": if self.passed { "scenario-passed" } else { "scenario-failed" },
            "name": self.scenario,
            "mode": mode.name(),
            "steps": self.steps.len(),
        });
        if let Some(step) = self.failed_step {
            record["step"] = json!(step);
        }
        if let Some(reason) = &self.reason {
            record["reason"] = json!(reason);
        }
        if let Some(frame) = &self.frame {
            record["frame"] = frame.clone();
        }
        record["evidence_dir"] = json!(self.evidence_dir.display().to_string());
        record
    }
}

fn run_step(
    mode: &Mode,
    index: usize,
    step: &Step,
    tail: &mut LogTail,
) -> Result<StepReport, String> {
    let started = Instant::now();
    let timeout = Duration::from_millis(step.timeout_ms);
    // Records before the command was sent never count.
    tail.new_lines()?;
    let socket = mode.socket_for(step.service);
    let ack = send_with_retry(&socket, &step.command, timeout)
        .map_err(|e| format!("step {index} ({}): {e}", step.cmd()))?;
    let ack_value = serde_json::to_value(&ack).unwrap_or(Value::Null);
    match (&ack, step.expect_ack_error) {
        (Ack::Ok { .. }, true) => {
            return Err(format!(
                "step {index} ({}): ack-expected-error, got {ack_value}",
                step.cmd()
            ));
        }
        (Ack::Error { reason, .. }, false) => {
            return Err(format!(
                "step {index} ({}): ack-error: {reason}",
                step.cmd()
            ));
        }
        _ => {}
    }
    if let Some(expected) = &step.ack
        && !is_subset(expected, &ack_value)
    {
        return Err(format!(
            "step {index} ({}): ack-mismatch, expected subset {expected}, got {ack_value}",
            step.cmd()
        ));
    }
    let mut records: Vec<Value> = Vec::new();
    let deadline = started + timeout;
    loop {
        for line in tail.new_lines()? {
            if let Some(record) = parse_record(&line) {
                records.push(record);
            }
        }
        match match_in_order(&records, &step.expect) {
            Ok(found) => {
                return Ok(StepReport {
                    index,
                    ack: ack_value,
                    matched: found.iter().map(|i| records[*i].clone()).collect(),
                    elapsed_ms: started.elapsed().as_millis(),
                });
            }
            Err(missing) if Instant::now() >= deadline => {
                return Err(format!(
                    "step {index} ({}): timeout-waiting-for-expect-{missing}: {} (saw {} records)",
                    step.cmd(),
                    step.expect[missing],
                    records.len()
                ));
            }
            Err(_) => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

fn capture_frame(capture: &FrameCapture, dir: &Path) -> Value {
    let ppm = dir.join("frame.ppm");
    let png = dir.join("frame.png");
    let output = Command::new("python3")
        .arg(&capture.qmp_helper)
        .arg("--socket")
        .arg(&capture.qmp_socket)
        .arg("screenshot")
        .arg("--ppm")
        .arg(&ppm)
        .arg("--png")
        .arg(&png)
        .output();
    let _ = std::fs::remove_file(&ppm);
    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            serde_json::from_str::<Value>(stdout.trim()).unwrap_or_else(|_| {
                json!({
                    "status": if out.status.success() { "ok" } else { "failed" },
                    "reason": String::from_utf8_lossy(&out.stderr).trim(),
                    "path": png.display().to_string(),
                })
            })
        }
        Err(e) => json!({"status": "failed", "reason": format!("cannot run qmp helper: {e}")}),
    }
}

/// Runs every step in order and writes the evidence directory
/// (`<out>/scenarios/<name>/result.json`, `log-excerpt.log`, optional
/// `frame.png`). Returns the outcome; the caller prints its record.
pub fn run(scenario: &Scenario, mode: &Mode, out: &Path) -> Result<Outcome, String> {
    let evidence_dir = out.join("scenarios").join(&scenario.name);
    std::fs::create_dir_all(&evidence_dir)
        .map_err(|e| format!("cannot create {}: {e}", evidence_dir.display()))?;
    let log = mode.log();
    if !log.is_file() {
        return Err(format!(
            "log {} does not exist; is the session running?",
            log.display()
        ));
    }
    let mut tail = LogTail::at_end(log)?;
    let mut excerpt_tail = LogTail::at_end(log)?;
    let mut steps = Vec::new();
    let mut failed_step = None;
    let mut reason = None;
    for (index, step) in scenario.steps.iter().enumerate() {
        match run_step(mode, index, step, &mut tail) {
            Ok(report) => steps.push(report),
            Err(e) => {
                failed_step = Some(index);
                reason = Some(e);
                break;
            }
        }
    }
    // Give trailing records a moment so the excerpt shows what followed.
    std::thread::sleep(Duration::from_millis(200));
    let excerpt = excerpt_tail.new_lines()?;
    let frame = match mode {
        Mode::Guest {
            frame: Some(capture),
            ..
        } => Some(capture_frame(capture, &evidence_dir)),
        _ => None,
    };
    let outcome = Outcome {
        scenario: scenario.name.clone(),
        passed: failed_step.is_none(),
        failed_step,
        reason,
        steps,
        excerpt,
        frame,
        evidence_dir,
    };
    write_evidence(scenario, mode, &outcome)?;
    Ok(outcome)
}

fn write_evidence(scenario: &Scenario, mode: &Mode, outcome: &Outcome) -> Result<(), String> {
    let dir = &outcome.evidence_dir;
    let excerpt_path = dir.join("log-excerpt.log");
    let mut file = File::create(&excerpt_path)
        .map_err(|e| format!("cannot write {}: {e}", excerpt_path.display()))?;
    for line in &outcome.excerpt {
        writeln!(file, "{line}").map_err(|e| e.to_string())?;
    }
    let steps: Vec<Value> = scenario
        .steps
        .iter()
        .enumerate()
        .map(|(index, step)| {
            let report = outcome.steps.iter().find(|r| r.index == index);
            json!({
                "index": index,
                "service": step.service.name(),
                "cmd": step.cmd(),
                "note": step.note,
                "expect_count": step.expect.len(),
                "status": match (report, outcome.failed_step) {
                    (Some(_), _) => "passed",
                    (None, Some(failed)) if failed == index => "failed",
                    _ => "skipped",
                },
                "ack": report.map(|r| r.ack.clone()),
                "matched": report.map(|r| r.matched.clone()),
                "elapsed_ms": report.map(|r| r.elapsed_ms),
            })
        })
        .collect();
    let mut result = outcome.record(mode);
    result["description"] = json!(scenario.description);
    result["log"] = json!(mode.log().display().to_string());
    result["log_excerpt"] = json!(excerpt_path.display().to_string());
    result["step_reports"] = Value::Array(steps);
    let result_path = dir.join("result.json");
    let text = serde_json::to_string_pretty(&result).map_err(|e| e.to_string())?;
    std::fs::write(&result_path, format!("{text}\n"))
        .map_err(|e| format!("cannot write {}: {e}", result_path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::Scenario;
    use ferrix_mock_control::{ChannelPath, Handler, Observer, Resolved};
    use std::sync::{Arc, Mutex};

    /// Fake daemon: acks every command and appends records to the log file,
    /// in the order the test script dictates.
    struct Fake {
        log: PathBuf,
        script: Mutex<Vec<Vec<Value>>>,
    }

    impl Handler for Fake {
        fn handle(&self, line: &str) -> Ack {
            let cmd: Value = serde_json::from_str(line).unwrap();
            let name = cmd["cmd"].as_str().unwrap_or("").to_owned();
            if name == "reject-me" {
                return Ack::error(Some(&name), "unknown command");
            }
            let records = {
                let mut script = self.script.lock().unwrap();
                if script.is_empty() {
                    Vec::new()
                } else {
                    script.remove(0)
                }
            };
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(&self.log)
                .unwrap();
            for record in &records {
                writeln!(file, "{record}").unwrap();
            }
            Ack::ok(name, vec![], json!({}))
        }
    }

    struct Quiet;
    impl Observer for Quiet {
        fn ready(&self, _: &Resolved) {}
        fn failed(&self, _: &str, _: &str) {}
    }

    fn workspace(tag: &str) -> (PathBuf, Mode) {
        let dir = std::env::temp_dir().join(format!("ferrix-scn-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("daemons.log");
        std::fs::write(
            &log,
            "{\"evidence\":\"mock\",\"event\":\"telephonyd-ready\"}\n",
        )
        .unwrap();
        let mode = Mode::Nested {
            sockets: dir.clone(),
            log,
        };
        (dir, mode)
    }

    fn serve(mode: &Mode, service: Service, script: Vec<Vec<Value>>) {
        let socket = mode.socket_for(service);
        let resolved = ChannelPath::Explicit(socket).resolve().unwrap();
        let fake = Fake {
            log: mode.log().to_path_buf(),
            script: Mutex::new(script),
        };
        ferrix_mock_control::spawn(resolved, Arc::new(fake), Arc::new(Quiet)).unwrap();
    }

    const SCENARIO: &str = r#"
schema_version = 1
name = "order-test"
description = "records must appear in the scripted order"
[[step]]
service = "telephony"
command = { cmd = "incoming-call", number = "+15550100001" }
expect = [ { event = "telephony-event", state = "ringing" }, { event = "telephony-event", state = "active" } ]
timeout_ms = 1500
[[step]]
service = "telephony"
command = { cmd = "tick", ms = 10 }
ack = { status = "ok", ack = "tick" }
"#;

    #[test]
    fn steps_pass_when_records_arrive_in_order() {
        let (dir, mode) = workspace("ok");
        serve(
            &mode,
            Service::Telephony,
            vec![vec![
                json!({"evidence":"mock","event":"telephony-event","state":"ringing"}),
                json!({"evidence":"mock","event":"other"}),
                json!({"evidence":"mock","event":"telephony-event","state":"active"}),
            ]],
        );
        let scenario = Scenario::parse(SCENARIO).unwrap();
        let outcome = run(&scenario, &mode, &dir).unwrap();
        assert!(outcome.passed, "{:?}", outcome.reason);
        assert_eq!(outcome.steps.len(), 2);
        assert_eq!(outcome.steps[0].matched.len(), 2);
        assert_eq!(outcome.steps[0].matched[1]["state"], "active");
        let record = outcome.record(&mode);
        assert_eq!(record["event"], "scenario-passed");
        assert_eq!(record["evidence"], "mock");
        assert!(dir.join("scenarios/order-test/result.json").is_file());
        let excerpt =
            std::fs::read_to_string(dir.join("scenarios/order-test/log-excerpt.log")).unwrap();
        assert!(excerpt.contains("ringing") && !excerpt.contains("telephonyd-ready"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn steps_fail_when_records_arrive_out_of_order() {
        let (dir, mode) = workspace("order");
        serve(
            &mode,
            Service::Telephony,
            vec![vec![
                json!({"evidence":"mock","event":"telephony-event","state":"active"}),
                json!({"evidence":"mock","event":"telephony-event","state":"ringing"}),
            ]],
        );
        let scenario = Scenario::parse(SCENARIO).unwrap();
        let outcome = run(&scenario, &mode, &dir).unwrap();
        assert!(!outcome.passed);
        assert_eq!(outcome.failed_step, Some(0));
        assert!(
            outcome
                .reason
                .as_deref()
                .unwrap()
                .contains("timeout-waiting-for-expect-1")
        );
        let record = outcome.record(&mode);
        assert_eq!(record["event"], "scenario-failed");
        assert_eq!(record["step"], 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn records_before_the_step_do_not_count() {
        let (dir, mode) = workspace("stale");
        // Pre-existing record that would satisfy the expect if stale lines counted.
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(mode.log())
            .unwrap();
        writeln!(
            file,
            "{}",
            json!({"evidence":"mock","event":"telephony-event","state":"ringing"})
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            json!({"evidence":"mock","event":"telephony-event","state":"active"})
        )
        .unwrap();
        serve(&mode, Service::Telephony, vec![vec![]]);
        let scenario = Scenario::parse(SCENARIO).unwrap();
        let outcome = run(&scenario, &mode, &dir).unwrap();
        assert!(!outcome.passed);
        assert!(outcome.reason.as_deref().unwrap().contains("expect-0"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ack_errors_are_failures_unless_expected() {
        let (dir, mode) = workspace("ack");
        serve(&mode, Service::Network, vec![]);
        let rejected = r#"
schema_version = 1
name = "ack-test"
description = "unknown commands are rejected"
[[step]]
service = "network"
command = { cmd = "reject-me" }
timeout_ms = 500
"#;
        let scenario = Scenario::parse(rejected).unwrap();
        let outcome = run(&scenario, &mode, &dir).unwrap();
        assert!(!outcome.passed);
        assert!(outcome.reason.as_deref().unwrap().contains("ack-error"));
        let expected = rejected.replace(
            "timeout_ms = 500",
            "timeout_ms = 500\nexpect_ack_error = true",
        );
        let scenario = Scenario::parse(&expected).unwrap();
        let outcome = run(&scenario, &mode, &dir).unwrap();
        assert!(outcome.passed, "{:?}", outcome.reason);
        let mismatch = rejected.replace(
            "command = { cmd = \"reject-me\" }",
            "command = { cmd = \"state\" }\nack = { status = \"ok\", ack = \"other\" }",
        );
        let scenario = Scenario::parse(&mismatch).unwrap();
        let outcome = run(&scenario, &mode, &dir).unwrap();
        assert!(outcome.reason.as_deref().unwrap().contains("ack-mismatch"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_socket_is_a_step_failure_with_reason() {
        let (dir, mode) = workspace("nosock");
        let scenario = Scenario::parse(
            SCENARIO
                .replace("timeout_ms = 1500", "timeout_ms = 300")
                .as_str(),
        )
        .unwrap();
        let outcome = run(&scenario, &mode, &dir).unwrap();
        assert!(!outcome.passed);
        assert!(
            outcome
                .reason
                .as_deref()
                .unwrap()
                .contains("cannot connect")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn serial_prefixes_are_tolerated() {
        assert_eq!(
            parse_record("[    1.234] {\"event\":\"x\"}").unwrap()["event"],
            "x"
        );
        assert!(parse_record("plain text").is_none());
        assert!(parse_record("[1] not json {").is_none());
    }
}
