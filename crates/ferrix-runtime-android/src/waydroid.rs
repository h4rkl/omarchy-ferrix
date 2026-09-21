//! [`WaydroidCli`]: the production [`AndroidHost`] that drives the maintained
//! `waydroid` command-line tool. Every operation is a bounded subprocess; the
//! only kernel-facing knowledge here is the host-side `/proc` scan used to
//! find the Android application's pid.
//!
//! Limitation recorded in `docs/android-runtime.md`: the Wayland client the
//! compositor sees is Waydroid's container-side composer process, not the
//! Android application, so `ferrix-appd` must associate surfaces with the pid
//! this adapter reports ([`LaunchedApp::surface_client_pid`]) rather than
//! with a process ancestry walk.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::host::{AndroidHost, HostError, LaunchedApp, RuntimeStatus};

/// Default binary name resolved through `PATH`.
pub const WAYDROID_BIN: &str = "waydroid";
/// Environment override for the binary, mirroring `FERRIX_SANDBOX_BIN`.
pub const WAYDROID_BIN_ENV: &str = "FERRIX_WAYDROID_BIN";
/// Interval between `waydroid status` polls while a session starts.
const POLL: Duration = Duration::from_millis(500);
/// Bound for any single CLI invocation other than the session supervisor.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(60);

/// Options for [`WaydroidCli`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WaydroidOptions {
    /// Explicit binary; `None` uses `FERRIX_WAYDROID_BIN`, then `waydroid`.
    pub binary: Option<PathBuf>,
    /// `/proc` root scanned for Android processes (tests point it elsewhere).
    pub proc_root: PathBuf,
    /// Substring of the container-side Wayland client's command line used to
    /// find the surface client pid. Unverified default until QEMU evidence
    /// exists; see `docs/android-runtime.md`.
    pub surface_client_needle: String,
}

impl Default for WaydroidOptions {
    fn default() -> Self {
        Self {
            binary: None,
            proc_root: PathBuf::from("/proc"),
            surface_client_needle: "graphics.composer".to_owned(),
        }
    }
}

/// Parsed `waydroid status` output.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WaydroidStatus {
    pub session: Option<String>,
    pub container: Option<String>,
    pub vendor_type: Option<String>,
    pub wayland_display: Option<String>,
    pub not_initialized: bool,
}

impl WaydroidStatus {
    /// The session is running and the container is usable. Waydroid freezes
    /// an idle container by design; its app commands unfreeze it before use,
    /// so `FROZEN` is a healthy quiescent state rather than a failed session.
    pub fn is_running(&self) -> bool {
        self.session.as_deref() == Some("RUNNING")
            && self
                .container
                .as_deref()
                .is_none_or(|c| matches!(c, "RUNNING" | "FROZEN"))
    }
}

/// Parses the text `waydroid status` prints (`Key:<tab>VALUE` lines, or the
/// "not initialized" notice). Unknown lines are ignored.
pub fn parse_status(text: &str) -> WaydroidStatus {
    let mut status = WaydroidStatus::default();
    for line in text.lines() {
        let line = line.trim();
        if line.to_ascii_lowercase().contains("not initialized") {
            status.not_initialized = true;
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim().to_owned();
        match key.trim().to_ascii_lowercase().as_str() {
            "session" => status.session = Some(value.to_ascii_uppercase()),
            "container" => status.container = Some(value.to_ascii_uppercase()),
            "vendor type" => status.vendor_type = Some(value),
            "wayland display" => status.wayland_display = Some(value),
            _ => {}
        }
    }
    status
}

/// Extracts package names from `waydroid app list` output (`packageName:`
/// lines).
pub fn parse_app_list(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let (key, value) = line.trim().split_once(':')?;
            (key.trim().eq_ignore_ascii_case("packageName")).then(|| value.trim().to_owned())
        })
        .filter(|name| !name.is_empty())
        .collect()
}

/// Given `(pid, cmdline)` pairs, the pid whose `argv[0]` is exactly
/// `package` (the Android main process), or failing that the lowest pid of a
/// `package:<subprocess>` child. Android's zygote rewrites the command line
/// of every application process to its process name.
pub fn pid_from_cmdlines<'a, I>(package: &str, cmdlines: I) -> Option<u32>
where
    I: IntoIterator<Item = (u32, &'a str)>,
{
    let mut child = None;
    for (pid, cmdline) in cmdlines {
        let argv0 = cmdline.split('\0').next().unwrap_or("");
        if argv0 == package {
            return Some(pid);
        }
        if argv0
            .strip_prefix(package)
            .is_some_and(|rest| rest.starts_with(':'))
        {
            child = Some(child.map_or(pid, |c: u32| c.min(pid)));
        }
    }
    child
}

/// Production host: shells out to `waydroid`.
pub struct WaydroidCli {
    options: WaydroidOptions,
    /// The detached `waydroid session start` supervisor this adapter spawned,
    /// kept so it is reaped and its pid can be reported.
    session: Mutex<Option<Child>>,
}

impl WaydroidCli {
    pub fn new(options: WaydroidOptions) -> Self {
        Self {
            options,
            session: Mutex::new(None),
        }
    }

    pub fn options(&self) -> &WaydroidOptions {
        &self.options
    }

    /// Resolves the binary: explicit path, then `FERRIX_WAYDROID_BIN`, then
    /// `waydroid` on `PATH`.
    pub fn binary(&self) -> PathBuf {
        if let Some(path) = &self.options.binary {
            return path.clone();
        }
        match std::env::var_os(WAYDROID_BIN_ENV) {
            Some(path) if !path.is_empty() => PathBuf::from(path),
            _ => PathBuf::from(WAYDROID_BIN),
        }
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(self.binary());
        command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }

    /// Runs `waydroid <args>` and returns `(stdout, stderr, exit code)`.
    fn run(&self, args: &[&str]) -> Result<(String, String, Option<i32>), HostError> {
        let mut child = self.command(args).spawn().map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                HostError::Absent(format!("{} not found", self.binary().display()))
            } else {
                HostError::CommandFailed(format!("cannot run {}: {error}", self.binary().display()))
            }
        })?;
        let deadline = Instant::now() + COMMAND_TIMEOUT;
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(POLL.min(Duration::from_millis(50)))
                }
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(HostError::CommandFailed(format!(
                        "waydroid {} timed out after {}s",
                        args.join(" "),
                        COMMAND_TIMEOUT.as_secs()
                    )));
                }
                Err(error) => return Err(HostError::CommandFailed(error.to_string())),
            }
        }
        let output = child
            .wait_with_output()
            .map_err(|error| HostError::CommandFailed(error.to_string()))?;
        Ok((
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
            output.status.code(),
        ))
    }

    fn run_ok(&self, args: &[&str]) -> Result<String, HostError> {
        let (stdout, stderr, code) = self.run(args)?;
        if code == Some(0) {
            Ok(stdout)
        } else {
            Err(HostError::CommandFailed(format!(
                "waydroid {} exited with {}: {}",
                args.join(" "),
                code.map_or("signal".to_owned(), |c| c.to_string()),
                first_line(&stderr)
                    .or_else(|| first_line(&stdout))
                    .unwrap_or_default()
            )))
        }
    }

    /// Reaps a finished session supervisor and reports its pid if alive.
    fn session_pid(&self) -> Option<u32> {
        let mut guard = self.session.lock().ok()?;
        let alive = match guard.as_mut() {
            Some(child) => matches!(child.try_wait(), Ok(None)),
            None => false,
        };
        if alive {
            guard.as_ref().map(Child::id)
        } else {
            *guard = None;
            None
        }
    }

    fn status_inner(&self) -> RuntimeStatus {
        match self.run(&["status"]) {
            Err(HostError::Absent(reason)) => RuntimeStatus::Absent { reason },
            Err(error) => RuntimeStatus::Failed {
                reason: error.to_string(),
            },
            Ok((stdout, stderr, code)) => {
                let parsed = parse_status(&format!("{stdout}\n{stderr}"));
                if parsed.not_initialized {
                    return RuntimeStatus::Failed {
                        reason: "not-initialized".to_owned(),
                    };
                }
                if code != Some(0) && parsed.session.is_none() {
                    return RuntimeStatus::Failed {
                        reason: format!(
                            "waydroid status exited with {}: {}",
                            code.map_or("signal".to_owned(), |c| c.to_string()),
                            first_line(&stderr).unwrap_or_default()
                        ),
                    };
                }
                match parsed.session.as_deref() {
                    Some("RUNNING") if parsed.is_running() => RuntimeStatus::Running {
                        session_pid: self.session_pid(),
                    },
                    Some("RUNNING") => RuntimeStatus::Failed {
                        reason: format!(
                            "session running but container {}",
                            parsed.container.unwrap_or_else(|| "unknown".to_owned())
                        ),
                    },
                    Some("STOPPED") | None if self.session_pid().is_some() => {
                        RuntimeStatus::Starting
                    }
                    Some("STOPPED") => RuntimeStatus::Stopped,
                    Some(other) => RuntimeStatus::Failed {
                        reason: format!("session {other}"),
                    },
                    None => RuntimeStatus::Failed {
                        reason: "unrecognised status output".to_owned(),
                    },
                }
            }
        }
    }

    /// Scans `/proc/*/cmdline` for `(pid, cmdline)` pairs.
    fn cmdlines(&self) -> Vec<(u32, String)> {
        let Ok(entries) = std::fs::read_dir(&self.options.proc_root) else {
            return Vec::new();
        };
        entries
            .flatten()
            .filter_map(|entry| {
                let pid: u32 = entry.file_name().to_str()?.parse().ok()?;
                let cmdline = std::fs::read(entry.path().join("cmdline")).ok()?;
                Some((pid, String::from_utf8_lossy(&cmdline).into_owned()))
            })
            .collect()
    }
}

fn first_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(str::to_owned)
}

impl AndroidHost for WaydroidCli {
    fn name(&self) -> &'static str {
        "waydroid"
    }

    fn status(&self) -> RuntimeStatus {
        self.status_inner()
    }

    fn ensure_session(
        &self,
        env: &[(String, String)],
        timeout: Duration,
    ) -> Result<RuntimeStatus, HostError> {
        match self.status_inner() {
            RuntimeStatus::Absent { reason } => return Err(HostError::Absent(reason)),
            RuntimeStatus::Failed { reason } => return Err(HostError::SessionFailed(reason)),
            running @ RuntimeStatus::Running { .. } => return Ok(running),
            RuntimeStatus::Stopped | RuntimeStatus::Starting => {}
        }
        if self.session_pid().is_none() {
            // `waydroid session start` stays in the foreground for the life
            // of the session, so it is detached here and polled below.
            let mut command = Command::new(self.binary());
            command
                .args(["session", "start"])
                .envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
                .stdin(Stdio::null())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit());
            #[cfg(unix)]
            {
                use std::os::unix::process::CommandExt as _;
                command.process_group(0);
            }
            let child = command.spawn().map_err(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    HostError::Absent(format!("{} not found", self.binary().display()))
                } else {
                    HostError::SessionFailed(format!("cannot start session: {error}"))
                }
            })?;
            if let Ok(mut guard) = self.session.lock() {
                *guard = Some(child);
            }
        }
        let deadline = Instant::now() + timeout;
        loop {
            match self.status_inner() {
                running @ RuntimeStatus::Running { .. } => return Ok(running),
                RuntimeStatus::Absent { reason } => return Err(HostError::Absent(reason)),
                RuntimeStatus::Failed { reason } => return Err(HostError::SessionFailed(reason)),
                RuntimeStatus::Stopped if self.session_pid().is_none() => {
                    return Err(HostError::SessionFailed(
                        "session supervisor exited before reporting RUNNING".to_owned(),
                    ));
                }
                RuntimeStatus::Stopped | RuntimeStatus::Starting => {}
            }
            if Instant::now() >= deadline {
                return Err(HostError::SessionFailed(format!(
                    "session not running after {}s",
                    timeout.as_secs()
                )));
            }
            std::thread::sleep(POLL);
        }
    }

    fn install(&self, apk: &Path) -> Result<(), HostError> {
        let path = apk
            .to_str()
            .ok_or_else(|| HostError::CommandFailed("apk path is not valid UTF-8".to_owned()))?;
        self.run_ok(&["app", "install", path]).map(drop)
    }

    fn is_installed(&self, package: &str) -> Result<bool, HostError> {
        let listing = self.run_ok(&["app", "list"])?;
        Ok(parse_app_list(&listing).iter().any(|p| p == package))
    }

    fn launch(&self, package: &str, activity: Option<&str>) -> Result<LaunchedApp, HostError> {
        match activity {
            None => self.run_ok(&["app", "launch", package])?,
            Some(activity) => {
                let component = format!("{package}/{activity}");
                self.run_ok(&["shell", "--", "am", "start", "-n", &component])?
            }
        };
        Ok(LaunchedApp {
            pid: self.app_pid(package),
            surface_client_pid: self.surface_client_pid(),
        })
    }

    fn is_running(&self, package: &str) -> Result<bool, HostError> {
        if self.app_pid(package).is_some() {
            return Ok(true);
        }
        if self.options.proc_root.is_dir() {
            // `/proc` is readable and the process is not there.
            return Ok(false);
        }
        // No host `/proc` view: ask the Android side (container pids only).
        let (stdout, _, code) = self.run(&["shell", "--", "pidof", "-s", package])?;
        Ok(code == Some(0) && !stdout.trim().is_empty())
    }

    fn stop(&self, package: &str) -> Result<(), HostError> {
        self.run_ok(&["shell", "--", "am", "force-stop", package])
            .map(drop)
    }

    fn app_pid(&self, package: &str) -> Option<u32> {
        let cmdlines = self.cmdlines();
        pid_from_cmdlines(
            package,
            cmdlines.iter().map(|(pid, cmd)| (*pid, cmd.as_str())),
        )
    }

    fn surface_client_pid(&self) -> Option<u32> {
        let needle = &self.options.surface_client_needle;
        if needle.is_empty() {
            return None;
        }
        self.cmdlines()
            .into_iter()
            .filter(|(_, cmd)| cmd.contains(needle.as_str()))
            .map(|(pid, _)| pid)
            .min()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_running_is_parsed() {
        let text = "Session:\tRUNNING\nContainer:\tRUNNING\nVendor type:\tMAINLINE\nIP address:\t192.168.240.112\nSession user:\tuser(1000)\nWayland display:\twayland-0\n";
        let status = parse_status(text);
        assert_eq!(status.session.as_deref(), Some("RUNNING"));
        assert_eq!(status.container.as_deref(), Some("RUNNING"));
        assert_eq!(status.vendor_type.as_deref(), Some("MAINLINE"));
        assert_eq!(status.wayland_display.as_deref(), Some("wayland-0"));
        assert!(status.is_running());
        assert!(!status.not_initialized);
    }

    #[test]
    fn status_stopped_and_not_initialized_are_parsed() {
        let stopped = parse_status("Session:\tSTOPPED\n");
        assert_eq!(stopped.session.as_deref(), Some("STOPPED"));
        assert!(!stopped.is_running());

        let frozen = parse_status("Session:\tRUNNING\nContainer:\tFROZEN\n");
        assert!(frozen.is_running());

        let uninit = parse_status("WayDroid is not initialized, run \"waydroid init\"\n");
        assert!(uninit.not_initialized);
        assert!(uninit.session.is_none());

        assert_eq!(parse_status("garbage\n\n"), WaydroidStatus::default());
    }

    #[test]
    fn app_list_yields_package_names() {
        let text = "Name: Brave\npackageName: com.brave.browser\ncategories: \n\tandroid.intent.category.LAUNCHER\n\nName: Files\npackageName: com.android.documentsui\n";
        assert_eq!(
            parse_app_list(text),
            vec![
                "com.brave.browser".to_owned(),
                "com.android.documentsui".to_owned()
            ]
        );
        assert!(parse_app_list("").is_empty());
    }

    #[test]
    fn cmdline_scan_prefers_the_main_process() {
        let cmdlines = [
            (1u32, "/sbin/init\0"),
            (900, "com.brave.browser:sandboxed_process0:org.chromium\0"),
            (850, "com.brave.browser:privileged_process0\0"),
            (700, "com.brave.browser\0"),
            (2, "com.brave.browser.other\0"),
        ];
        assert_eq!(
            pid_from_cmdlines("com.brave.browser", cmdlines.iter().copied()),
            Some(700)
        );
        assert_eq!(
            pid_from_cmdlines("com.brave.browser", cmdlines[..3].iter().copied()),
            Some(850)
        );
        assert_eq!(
            pid_from_cmdlines("org.other.app", cmdlines.iter().copied()),
            None
        );
    }

    #[test]
    fn missing_binary_is_absent() {
        let cli = WaydroidCli::new(WaydroidOptions {
            binary: Some(PathBuf::from("/nonexistent/waydroid")),
            ..WaydroidOptions::default()
        });
        assert_eq!(cli.name(), "waydroid");
        assert!(matches!(cli.status(), RuntimeStatus::Absent { .. }));
        assert!(matches!(
            cli.ensure_session(&[], Duration::from_millis(10)),
            Err(HostError::Absent(_))
        ));
        assert!(matches!(
            cli.is_installed("com.brave.browser"),
            Err(HostError::Absent(_))
        ));
    }

    #[test]
    fn proc_scan_reads_cmdline_files() {
        let root = std::env::temp_dir().join(format!(
            "ferrix-android-proc-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(root.join("4321")).unwrap();
        std::fs::create_dir_all(root.join("77")).unwrap();
        std::fs::create_dir_all(root.join("self")).unwrap();
        std::fs::write(root.join("4321/cmdline"), b"com.brave.browser\0").unwrap();
        std::fs::write(
            root.join("77/cmdline"),
            b"/vendor/bin/hw/android.hardware.graphics.composer@2.1-service\0",
        )
        .unwrap();
        let cli = WaydroidCli::new(WaydroidOptions {
            binary: Some(PathBuf::from("/nonexistent/waydroid")),
            proc_root: root.clone(),
            ..WaydroidOptions::default()
        });
        assert_eq!(cli.app_pid("com.brave.browser"), Some(4321));
        assert_eq!(cli.surface_client_pid(), Some(77));
        assert_eq!(cli.is_running("com.brave.browser"), Ok(true));
        assert_eq!(cli.is_running("org.none"), Ok(false));
        let _ = std::fs::remove_dir_all(root);
    }
}
