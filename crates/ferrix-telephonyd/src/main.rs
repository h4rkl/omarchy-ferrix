//! `ferrix-telephonyd` binary: composes a backend with the service and serves
//! the socket. This is the only place that names a backend implementation.
//!
//! 1. parses arguments; a production backend with `--mock-control` is a typed
//!    startup error;
//! 2. builds the backend (`mock` from the profile, `modemmanager` behind the
//!    `production` feature), loads the store, publishes `service-restarted`;
//! 3. binds the socket and only then prints `telephonyd-ready`;
//! 4. opens the mock-control channel (mock only) and serves forever.
//!
//! Any startup failure prints `telephonyd-failed` and exits non-zero.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use ferrix_mock_control::{ChannelPath, Observer, Resolved};
use ferrix_target_profile::Profile;
use ferrix_telephony_adapter_api::ModemBackend;
use ferrix_telephony_mock::{MockControl, MockModem, MockTelephonyConfig};
use ferrix_telephony_protocol::{DEFAULT_SOCKET, SOCKET_ENV};
use ferrix_telephonyd::{
    AllowAll, AppdIdentity, DATA_DIR_ENV, DEFAULT_DATA_DIR, DenyAll, Identity, LocalControl,
    Recorder, Server, Service, ServiceOptions, SmsStore,
};

const USAGE: &str =
    "usage: ferrix-telephonyd --backend mock|modemmanager [--profile PATH] [--socket PATH]
                         [--mock-control auto|none|PATH] [--appd PATH|none] [--data-dir PATH]
                         [--nested] [--allow-unauthenticated]

  --backend KIND     mock (profile-driven, deterministic) or modemmanager
                     (production feature, Linux only)
  --profile PATH     target profile for the mock backend
                     (default targets/qemu-aarch64/profile.toml if present)
  --socket PATH      socket to serve (env FERRIX_TELEPHONYD_SOCKET,
                     default /run/ferrix/telephonyd.sock)
  --mock-control X   host mock-control channel: auto (virtio port
                     ferrix.telephony), none, or a path; mock backend only
  --appd PATH|none   application service socket for peer identity
                     (env FERRIX_APPD_SOCKET, default /run/ferrix/ferrix-appd.sock);
                     none denies every peer unless --allow-unauthenticated
  --data-dir PATH    SMS store directory (env FERRIX_TELEPHONY_DIR,
                     default /var/lib/ferrix/telephony)
  --nested           nested host run: records carry evidence \"mock\"
  --allow-unauthenticated
                     with --appd none: grant every capability to every
                     peer (nested development only; records a warning)";

const DEFAULT_APPD_SOCKET: &str = "/run/ferrix/ferrix-appd.sock";
const APPD_SOCKET_ENV: &str = "FERRIX_APPD_SOCKET";
const MOCK_CONTROL_PORT: &str = "ferrix.telephony";
const REPO_PROFILE: &str = "targets/qemu-aarch64/profile.toml";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BackendKind {
    Mock,
    ModemManager,
}

impl BackendKind {
    fn parse(text: &str) -> Result<Self, String> {
        match text {
            "mock" => Ok(BackendKind::Mock),
            "modemmanager" => Ok(BackendKind::ModemManager),
            other => Err(format!("unknown backend {other:?} (mock|modemmanager)")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum AppdMode {
    Socket(PathBuf),
    None,
}

#[derive(Debug)]
struct Args {
    backend: BackendKind,
    profile: Option<PathBuf>,
    socket: PathBuf,
    mock_control: ChannelPath,
    appd: AppdMode,
    data_dir: PathBuf,
    nested: bool,
    allow_unauthenticated: bool,
}

#[derive(Debug)]
enum Parsed {
    Run(Args),
    Help,
}

fn parse_args(argv: impl IntoIterator<Item = String>) -> Result<Parsed, String> {
    let env = |key: &str| std::env::var(key).ok().filter(|v| !v.is_empty());
    let mut backend = None;
    let mut args = Args {
        backend: BackendKind::Mock,
        profile: None,
        socket: PathBuf::from(env(SOCKET_ENV).unwrap_or(DEFAULT_SOCKET.to_owned())),
        mock_control: ChannelPath::None,
        appd: AppdMode::Socket(PathBuf::from(
            env(APPD_SOCKET_ENV).unwrap_or(DEFAULT_APPD_SOCKET.to_owned()),
        )),
        data_dir: PathBuf::from(env(DATA_DIR_ENV).unwrap_or(DEFAULT_DATA_DIR.to_owned())),
        nested: false,
        allow_unauthenticated: false,
    };
    let mut it = argv.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--backend" => {
                backend = Some(BackendKind::parse(
                    &it.next().ok_or("--backend needs mock|modemmanager")?,
                )?);
            }
            "--profile" => {
                args.profile = Some(PathBuf::from(it.next().ok_or("--profile needs a path")?));
            }
            "--socket" => args.socket = PathBuf::from(it.next().ok_or("--socket needs a path")?),
            "--mock-control" => {
                let text = it.next().ok_or("--mock-control needs auto|none|PATH")?;
                args.mock_control = ChannelPath::parse(&text, MOCK_CONTROL_PORT);
            }
            "--appd" => {
                let text = it.next().ok_or("--appd needs PATH|none")?;
                args.appd = if text == "none" {
                    AppdMode::None
                } else {
                    AppdMode::Socket(PathBuf::from(text))
                };
            }
            "--data-dir" => {
                args.data_dir = PathBuf::from(it.next().ok_or("--data-dir needs a path")?);
            }
            "--nested" => args.nested = true,
            "--allow-unauthenticated" => args.allow_unauthenticated = true,
            "--help" | "-h" => return Ok(Parsed::Help),
            other => return Err(format!("unknown argument {other}")),
        }
    }
    args.backend = backend.ok_or("--backend is required")?;
    if args.backend != BackendKind::Mock && args.mock_control != ChannelPath::None {
        return Err(
            "--mock-control is only valid with --backend mock; a production backend refuses the mock-control channel"
                .to_owned(),
        );
    }
    if args.allow_unauthenticated && args.appd != AppdMode::None {
        return Err("--allow-unauthenticated requires --appd none".to_owned());
    }
    Ok(Parsed::Run(args))
}

struct ControlObserver(Recorder);

impl Observer for ControlObserver {
    fn ready(&self, resolved: &Resolved) {
        self.0
            .mock_control("ready", Some(resolved.path()), Some(resolved.kind()));
    }

    fn failed(&self, stage: &str, error: &str) {
        self.0
            .mock_control("failed", None, Some(&format!("{stage}: {error}")));
    }
}

fn load_profile(path: Option<&Path>) -> Result<Option<Profile>, String> {
    match path {
        Some(path) => Profile::load(path).map(Some).map_err(|e| e.to_string()),
        None => {
            let repo = Path::new(REPO_PROFILE);
            if repo.is_file() {
                Profile::load(repo).map(Some).map_err(|e| e.to_string())
            } else {
                Ok(None)
            }
        }
    }
}

fn build_backend(args: &Args) -> Result<(Arc<dyn ModemBackend>, Option<MockControl>), String> {
    match args.backend {
        BackendKind::Mock => {
            let config = match load_profile(args.profile.as_deref())? {
                Some(profile) => MockTelephonyConfig::from_profile(&profile),
                None => MockTelephonyConfig::default(),
            };
            let modem = MockModem::new(config);
            let control = MockControl::new(modem.clone());
            Ok((Arc::new(modem), Some(control)))
        }
        BackendKind::ModemManager => build_modemmanager(),
    }
}

#[cfg(feature = "production")]
fn build_modemmanager() -> Result<(Arc<dyn ModemBackend>, Option<MockControl>), String> {
    let backend = ferrix_telephony_mm::ModemManagerBackend::connect()?;
    Ok((Arc::new(backend), None))
}

#[cfg(not(feature = "production"))]
fn build_modemmanager() -> Result<(Arc<dyn ModemBackend>, Option<MockControl>), String> {
    Err("the modemmanager backend requires the `production` cargo feature (Linux only)".to_owned())
}

fn run(args: Args, recorder: Recorder) -> Result<(), String> {
    let (backend, control) = build_backend(&args)?;
    let identity: Arc<dyn Identity> = match &args.appd {
        AppdMode::Socket(path) => Arc::new(AppdIdentity::new(path.clone(), Duration::from_secs(2))),
        AppdMode::None if args.allow_unauthenticated => {
            recorder.unauthenticated_warning();
            Arc::new(AllowAll)
        }
        AppdMode::None => Arc::new(DenyAll),
    };
    let service = Service::new(
        backend,
        SmsStore::new(&args.data_dir),
        recorder.clone(),
        ServiceOptions::default(),
    )?;
    service.spawn_event_loop();

    let listener = Server::bind(&args.socket)?;
    recorder.ready(
        &args.socket,
        service.backend_name(),
        &service.capabilities(),
    );

    match (control, args.mock_control.resolve()) {
        (Some(control), Some(resolved)) => {
            // The daemon owns the channel: backend mutations are forwarded to
            // the mock adapter, local client verbs run through the service.
            let handler = LocalControl::new(Arc::clone(&service), Arc::new(control));
            let observer = Arc::new(ControlObserver(recorder.clone()));
            ferrix_mock_control::spawn(resolved, Arc::new(handler), observer)?;
        }
        (Some(_), None) if args.mock_control != ChannelPath::None => {
            recorder.mock_control("unavailable", None, Some("virtio port not found"));
        }
        _ => recorder.mock_control("none", None, None),
    }

    Arc::new(Server::new(service, identity)).serve(listener);
    Err("socket listener closed".to_owned())
}

fn main() -> ExitCode {
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(Parsed::Run(args)) => args,
        Ok(Parsed::Help) => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(error) => {
            eprintln!("{USAGE}");
            let evidence = ferrix_telephonyd::records::runtime_evidence(false);
            Recorder::stdout(&evidence).failed(&error);
            return ExitCode::FAILURE;
        }
    };
    let evidence = ferrix_telephonyd::records::runtime_evidence(args.nested);
    let recorder = Recorder::stdout(&evidence);
    match run(args, recorder.clone()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            recorder.failed(&error);
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Parsed, String> {
        parse_args(args.iter().map(|s| (*s).to_owned()))
    }

    #[test]
    fn backend_is_required_and_mock_control_is_mock_only() {
        assert!(parse(&[]).unwrap_err().contains("--backend"));
        let err = parse(&["--backend", "modemmanager", "--mock-control", "auto"]).unwrap_err();
        assert!(err.contains("production backend refuses"), "{err}");
        assert!(matches!(
            parse(&["--backend", "mock", "--mock-control", "auto"]),
            Ok(Parsed::Run(Args {
                mock_control: ChannelPath::Auto { .. },
                ..
            }))
        ));
        assert!(matches!(
            parse(&["--backend", "modemmanager", "--mock-control", "none"]),
            Ok(Parsed::Run(_))
        ));
        assert!(parse(&["--backend", "bogus"]).is_err());
    }

    #[test]
    fn appd_none_and_allow_unauthenticated() {
        let Ok(Parsed::Run(args)) = parse(&["--backend", "mock", "--appd", "none"]) else {
            panic!()
        };
        assert_eq!(args.appd, AppdMode::None);
        assert!(!args.allow_unauthenticated);
        assert!(
            parse(&["--backend", "mock", "--allow-unauthenticated"])
                .unwrap_err()
                .contains("--appd none")
        );
        let Ok(Parsed::Run(args)) = parse(&[
            "--backend",
            "mock",
            "--appd",
            "none",
            "--allow-unauthenticated",
            "--nested",
            "--data-dir",
            "/tmp/x",
        ]) else {
            panic!()
        };
        assert!(args.allow_unauthenticated && args.nested);
        assert_eq!(args.data_dir, PathBuf::from("/tmp/x"));
    }
}
