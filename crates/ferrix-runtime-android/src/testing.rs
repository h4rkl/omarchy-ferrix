//! [`FakeAndroidHost`]: an in-memory [`AndroidHost`] for tests. It never
//! touches the operating system; a test scripts the runtime state, the
//! installed packages, and which packages are "running", and inspects the
//! calls the adapter made.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use crate::host::{AndroidHost, HostError, LaunchedApp, RuntimeStatus};

#[derive(Debug, Default)]
struct State {
    status: Option<RuntimeStatus>,
    session_start_fails: Option<String>,
    installed: BTreeSet<String>,
    running: BTreeSet<String>,
    launch_fails: Option<String>,
    launches: Vec<(String, Option<String>)>,
    stops: Vec<String>,
    installs: Vec<PathBuf>,
    session_starts: u32,
    next_pid: u32,
    surface_client_pid: Option<u32>,
    surface_client_pids: Option<Vec<u32>>,
}

/// Clone handles share state: a test keeps one and hands another to the
/// runtime under test.
#[derive(Clone, Default)]
pub struct FakeAndroidHost(Arc<Mutex<State>>);

fn lock(state: &Mutex<State>) -> MutexGuard<'_, State> {
    state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl FakeAndroidHost {
    /// A stopped, initialised runtime with nothing installed.
    pub fn new() -> Self {
        let host = Self::default();
        {
            let mut state = lock(&host.0);
            state.status = Some(RuntimeStatus::Stopped);
            state.next_pid = 5000;
            state.surface_client_pid = Some(4242);
        }
        host
    }

    /// A host whose runtime binary is missing.
    pub fn absent(reason: &str) -> Self {
        let host = Self::new();
        host.set_status(RuntimeStatus::Absent {
            reason: reason.to_owned(),
        });
        host
    }

    pub fn set_status(&self, status: RuntimeStatus) {
        lock(&self.0).status = Some(status);
    }

    /// `ensure_session` fails with `reason` instead of starting.
    pub fn fail_session_start(&self, reason: &str) {
        lock(&self.0).session_start_fails = Some(reason.to_owned());
    }

    /// The next `launch` fails with `reason`.
    pub fn fail_launch(&self, reason: &str) {
        lock(&self.0).launch_fails = Some(reason.to_owned());
    }

    pub fn install_package(&self, package: &str) {
        lock(&self.0).installed.insert(package.to_owned());
    }

    /// Simulates the Android process disappearing (crash, LMK, force stop).
    pub fn kill(&self, package: &str) -> bool {
        lock(&self.0).running.remove(package)
    }

    pub fn is_running(&self, package: &str) -> bool {
        lock(&self.0).running.contains(package)
    }

    pub fn launches(&self) -> Vec<(String, Option<String>)> {
        lock(&self.0).launches.clone()
    }

    pub fn stops(&self) -> Vec<String> {
        lock(&self.0).stops.clone()
    }

    pub fn installs(&self) -> Vec<PathBuf> {
        lock(&self.0).installs.clone()
    }

    pub fn session_starts(&self) -> u32 {
        lock(&self.0).session_starts
    }

    pub fn set_surface_client_pid(&self, pid: Option<u32>) {
        lock(&self.0).surface_client_pid = pid;
    }

    /// Every container Wayland client pid (multi-window: several connections).
    pub fn set_surface_client_pids(&self, pids: Vec<u32>) {
        let mut state = lock(&self.0);
        state.surface_client_pid = pids.iter().copied().min();
        state.surface_client_pids = Some(pids);
    }
}

impl AndroidHost for FakeAndroidHost {
    fn name(&self) -> &'static str {
        "fake"
    }

    fn status(&self) -> RuntimeStatus {
        lock(&self.0)
            .status
            .clone()
            .unwrap_or(RuntimeStatus::Stopped)
    }

    fn ensure_session(
        &self,
        _env: &[(String, String)],
        _timeout: Duration,
    ) -> Result<RuntimeStatus, HostError> {
        let mut state = lock(&self.0);
        match state.status.clone().unwrap_or(RuntimeStatus::Stopped) {
            RuntimeStatus::Absent { reason } => return Err(HostError::Absent(reason)),
            RuntimeStatus::Failed { reason } => return Err(HostError::SessionFailed(reason)),
            running @ RuntimeStatus::Running { .. } => return Ok(running),
            RuntimeStatus::Stopped | RuntimeStatus::Starting => {}
        }
        state.session_starts += 1;
        if let Some(reason) = state.session_start_fails.clone() {
            state.status = Some(RuntimeStatus::Failed {
                reason: reason.clone(),
            });
            return Err(HostError::SessionFailed(reason));
        }
        let running = RuntimeStatus::Running {
            session_pid: Some(777),
        };
        state.status = Some(running.clone());
        Ok(running)
    }

    fn install(&self, apk: &Path) -> Result<(), HostError> {
        lock(&self.0).installs.push(apk.to_path_buf());
        Ok(())
    }

    fn is_installed(&self, package: &str) -> Result<bool, HostError> {
        Ok(lock(&self.0).installed.contains(package))
    }

    fn launch(&self, package: &str, activity: Option<&str>) -> Result<LaunchedApp, HostError> {
        let mut state = lock(&self.0);
        state
            .launches
            .push((package.to_owned(), activity.map(str::to_owned)));
        if let Some(reason) = state.launch_fails.take() {
            return Err(HostError::CommandFailed(reason));
        }
        if !state.installed.contains(package) {
            return Err(HostError::NotInstalled);
        }
        state.running.insert(package.to_owned());
        let pid = state.next_pid;
        state.next_pid += 1;
        Ok(LaunchedApp {
            pid: Some(pid),
            surface_client_pid: state.surface_client_pid,
        })
    }

    fn is_running(&self, package: &str) -> Result<bool, HostError> {
        Ok(lock(&self.0).running.contains(package))
    }

    fn stop(&self, package: &str) -> Result<(), HostError> {
        let mut state = lock(&self.0);
        state.stops.push(package.to_owned());
        state.running.remove(package);
        Ok(())
    }

    fn app_pid(&self, package: &str) -> Option<u32> {
        lock(&self.0).running.contains(package).then_some(1)
    }

    fn surface_client_pid(&self) -> Option<u32> {
        lock(&self.0).surface_client_pid
    }

    fn window_app_id(&self, package: &str) -> String {
        format!("waydroid.{package}")
    }

    fn surface_client_pids(&self) -> Vec<u32> {
        let state = lock(&self.0);
        match &state.surface_client_pids {
            Some(pids) => pids.clone(),
            None => state.surface_client_pid.into_iter().collect(),
        }
    }
}
