//! Records and reports exchanged between the sandbox stages, the runtime, and
//! the log: JSON records on stdout (evidence label first) and the
//! line-oriented readiness reports sent over an abstract Unix socket on Linux.

use std::io::Write as _;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::{AppliedIsolation, IsolationPolicy, IsolationReport, NamespaceSet};

/// Builds one record: `evidence` first, then `event`, then the fields.
pub fn record(evidence: &str, event: &str, fields: Value) -> Value {
    let mut out = Map::new();
    out.insert("evidence".to_owned(), Value::String(evidence.to_owned()));
    out.insert("event".to_owned(), Value::String(event.to_owned()));
    if let Value::Object(fields) = fields {
        out.extend(fields);
    }
    Value::Object(out)
}

/// Prints one JSON record per line and flushes, so serial captures are exact.
pub fn emit(value: &Value) {
    let mut stdout = std::io::stdout().lock();
    let _ = serde_json::to_writer(&mut stdout, value);
    let _ = stdout.write_all(b"\n");
    let _ = stdout.flush();
}

/// Stage 1's readiness report: the `sandbox-ready` record body, also sent to
/// the runtime over the report channel.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SandboxReady {
    pub app: String,
    /// Stage 2 pid in the launcher's PID namespace.
    pub init_pid: u32,
    pub namespaces: NamespaceSet,
    pub private_tmp: bool,
    pub no_new_privs: bool,
    pub degraded: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl SandboxReady {
    pub fn applied(&self) -> AppliedIsolation {
        self.namespaces.applied(self.private_tmp, self.no_new_privs)
    }

    pub fn isolation_report(&self, requested: IsolationPolicy) -> IsolationReport {
        IsolationReport {
            requested,
            applied: self.applied(),
            degraded: self.degraded,
            reason: self.reason.clone(),
        }
    }
}

/// Stage 2's report to stage 1, sent just before the application starts.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct InitReport {
    pub private_tmp: bool,
    pub no_new_privs: bool,
    pub reasons: Vec<String>,
}

/// Body of the `sandbox-probe` record.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProbeReport {
    pub applied: AppliedIsolation,
    pub degraded: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Finds the last record with the given `event` in captured stdout.
pub fn find_record(stdout: &str, event: &str) -> Option<Value> {
    stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line.trim()).ok())
        .rfind(|value| value.get("event").and_then(Value::as_str) == Some(event))
}

fn reason_of(value: &Value) -> Option<String> {
    value
        .get("reason")
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// Interprets a `ferrix-sandbox --probe` run: status 0 with a `sandbox-probe`
/// record is success; anything else is the reason the policy cannot be met.
pub fn parse_probe_output(stdout: &str, status: Option<i32>) -> Result<AppliedIsolation, String> {
    let probe = find_record(stdout, "sandbox-probe");
    match status {
        Some(0) => probe
            .and_then(|value| serde_json::from_value::<ProbeReport>(value).ok())
            .map(|report| report.applied)
            .ok_or_else(|| "probe exited 0 without a readable sandbox-probe record".to_owned()),
        Some(code) => Err(probe
            .as_ref()
            .and_then(reason_of)
            .or_else(|| {
                find_record(stdout, "sandbox-failed")
                    .as_ref()
                    .and_then(reason_of)
            })
            .unwrap_or_else(|| format!("probe exited with status {code}"))),
        None => Err("probe was killed by a signal".to_owned()),
    }
}

/// Helper for tests and tools: the `sandbox-test-skipped` record.
pub fn skipped_record(evidence: &str, reason: &str) -> Value {
    record(
        evidence,
        "sandbox-test-skipped",
        json!({ "reason": reason }),
    )
}

/// Line-oriented readiness reports over an abstract Unix socket. Abstract
/// names live in the network namespace, which the sandbox does not change,
/// so both stages and the runtime can reach each other without touching the
/// filesystem (which the mount namespace and private `/tmp` would change) or
/// the application's stdio.
#[cfg(target_os = "linux")]
pub mod channel {
    use std::io::{self, Read as _, Write as _};
    use std::net::Shutdown;
    use std::os::linux::net::SocketAddrExt as _;
    use std::os::unix::net::{SocketAddr, UnixListener, UnixStream};
    use std::process::{Child, ExitStatus};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    /// How the wait for a report ended without a report.
    #[derive(Debug)]
    pub enum WaitOutcome {
        /// The child exited before reporting.
        ChildExited(ExitStatus),
        TimedOut,
        Io(String),
    }

    /// A listener for exactly one report line.
    pub struct ReportListener {
        name: String,
        listener: UnixListener,
    }

    impl ReportListener {
        /// Binds a fresh abstract name derived from `prefix`.
        pub fn bind(prefix: &str) -> io::Result<Self> {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0);
            let name = format!("{prefix}-{}-{nanos}", std::process::id());
            let addr = SocketAddr::from_abstract_name(name.as_bytes())?;
            let listener = UnixListener::bind_addr(&addr)?;
            listener.set_nonblocking(true)?;
            Ok(Self { name, listener })
        }

        pub fn name(&self) -> &str {
            &self.name
        }

        fn try_accept_line(&self) -> io::Result<Option<String>> {
            match self.listener.accept() {
                Ok((mut stream, _)) => {
                    stream.set_nonblocking(false)?;
                    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
                    let mut text = String::new();
                    stream.read_to_string(&mut text)?;
                    Ok(Some(text.trim().to_owned()))
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => Ok(None),
                Err(e) => Err(e),
            }
        }

        /// Waits until a report line arrives, `child` exits, or `timeout`
        /// passes. A report that raced with the child's exit still wins.
        /// `on_tick` runs once per poll so a single-threaded caller can keep
        /// forwarding signals meanwhile.
        pub fn wait_for_line(
            &self,
            child: &mut Child,
            timeout: Duration,
            on_tick: &mut dyn FnMut(),
        ) -> Result<String, WaitOutcome> {
            let deadline = Instant::now() + timeout;
            loop {
                on_tick();
                match self.try_accept_line() {
                    Ok(Some(line)) => return Ok(line),
                    Ok(None) => {}
                    Err(e) => return Err(WaitOutcome::Io(e.to_string())),
                }
                match child.try_wait() {
                    Ok(Some(status)) => {
                        return match self.try_accept_line() {
                            Ok(Some(line)) => Ok(line),
                            _ => Err(WaitOutcome::ChildExited(status)),
                        };
                    }
                    Ok(None) => {}
                    Err(e) => return Err(WaitOutcome::Io(e.to_string())),
                }
                if Instant::now() >= deadline {
                    return Err(WaitOutcome::TimedOut);
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }

    /// Sends one report line to the abstract socket `name` and closes.
    pub fn send_line(name: &str, line: &str) -> io::Result<()> {
        let addr = SocketAddr::from_abstract_name(name.as_bytes())?;
        let mut stream = UnixStream::connect_addr(&addr)?;
        stream.set_write_timeout(Some(Duration::from_secs(2)))?;
        stream.write_all(line.as_bytes())?;
        stream.write_all(b"\n")?;
        stream.shutdown(Shutdown::Write)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_lead_with_evidence_then_event() {
        let value = record("mock", "sandbox-ready", json!({ "app": "org.ferrix.x" }));
        let text = serde_json::to_string(&value).unwrap();
        assert!(
            text.starts_with(r#"{"evidence":"mock","event":"sandbox-ready""#),
            "{text}"
        );
        assert_eq!(value["app"], "org.ferrix.x");
        assert_eq!(
            skipped_record("mock", "EPERM")["event"],
            "sandbox-test-skipped"
        );
    }

    #[test]
    fn probe_output_is_interpreted_by_status_and_record() {
        let ok = r#"{"evidence":"QEMU","event":"sandbox-probe","applied":{"pid_namespace":true,"mount_namespace":true,"ipc_namespace":true,"uts_namespace":true,"user_namespace":false,"private_tmp":true,"no_new_privs":true},"degraded":false}"#;
        let applied = parse_probe_output(ok, Some(0)).unwrap();
        assert!(applied.pid_namespace && applied.private_tmp && !applied.user_namespace);

        let failed = r#"noise
{"evidence":"QEMU","event":"sandbox-probe","applied":{},"degraded":true,"reason":"unshare: EPERM"}"#;
        assert_eq!(
            parse_probe_output(failed, Some(126)).unwrap_err(),
            "unshare: EPERM"
        );
        let legacy = r#"{"evidence":"QEMU","event":"sandbox-failed","reason":"not linux"}"#;
        assert_eq!(
            parse_probe_output(legacy, Some(126)).unwrap_err(),
            "not linux"
        );
        assert!(parse_probe_output("", Some(0)).is_err());
        assert!(parse_probe_output("", None).is_err());
        assert!(
            parse_probe_output("", Some(1))
                .unwrap_err()
                .contains("status 1")
        );
    }

    #[test]
    fn ready_report_converts_to_an_isolation_report() {
        let ready = SandboxReady {
            app: "org.ferrix.terminal".to_owned(),
            init_pid: 42,
            namespaces: NamespaceSet {
                pid: true,
                mount: true,
                ipc: true,
                uts: true,
                user: false,
            },
            private_tmp: true,
            no_new_privs: true,
            degraded: false,
            reason: None,
        };
        let text = serde_json::to_string(&ready).unwrap();
        let back: SandboxReady = serde_json::from_str(&text).unwrap();
        assert_eq!(back, ready);
        let report = ready.isolation_report(IsolationPolicy::default());
        assert!(!report.degraded);
        assert!(report.applied.mount_namespace && report.applied.private_tmp);
        assert_eq!(report.requested, IsolationPolicy::default());
    }
}
