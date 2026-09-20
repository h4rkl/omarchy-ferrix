//! `ferrix-networkd`: the privileged networking service.
//!
//! 1. parses the typed command line (`--backend mock|networkmanager`);
//! 2. composes the backend (this file is the only place a backend crate is
//!    named), the identity source, and the mock-control channel;
//! 3. binds the protocol socket and only then prints `networkd-ready`;
//! 4. serves `ferrix-network-protocol` until killed.
//!
//! Any startup failure prints a `networkd-failed` record and exits non-zero.

use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use ferrix_mock_control::ChannelPath;
use ferrix_network_adapter_api::NetworkBackend;
use ferrix_networkd::ControlObserver;
use ferrix_networkd::cli::{self, Args, BackendKind, CliError, IdentitySource};
use ferrix_networkd::control::MockControl;
use ferrix_networkd::identity::{AllowAll, AppdIdentity, DenyAll, Identity};
use ferrix_networkd::records::Recorder;
use ferrix_networkd::server::Server;
use ferrix_networkd::service::Service;

/// A backend plus, for the mock, the handler that drives it.
struct Composed {
    backend: Arc<dyn NetworkBackend>,
    control: Option<Arc<dyn ferrix_mock_control::Handler>>,
}

fn compose_backend(args: &Args) -> Result<Composed, String> {
    match args.backend {
        BackendKind::Mock => {
            let config = match &args.profile {
                Some(path) => {
                    let profile = ferrix_target_profile::Profile::load(path)
                        .map_err(|e| format!("profile: {e}"))?;
                    ferrix_network_mock::MockNetworkConfig::from_profile(&profile)
                }
                None => ferrix_network_mock::MockNetworkConfig::default(),
            };
            let mock = ferrix_network_mock::MockNetwork::new(config);
            Ok(Composed {
                backend: mock.clone(),
                control: Some(mock),
            })
        }
        BackendKind::NetworkManager => compose_production(),
    }
}

#[cfg(feature = "production")]
fn compose_production() -> Result<Composed, String> {
    let backend = ferrix_network_nm::NetworkManagerBackend::connect()
        .map_err(|e| format!("networkmanager: {e}"))?;
    Ok(Composed {
        backend,
        control: None,
    })
}

#[cfg(not(feature = "production"))]
fn compose_production() -> Result<Composed, String> {
    Err(
        "the networkmanager backend is not compiled in (build with --features production)"
            .to_owned(),
    )
}

fn compose_identity(source: &IdentitySource, recorder: &Recorder) -> Arc<dyn Identity> {
    let identity: Arc<dyn Identity> = match source {
        IdentitySource::Appd(path) => Arc::new(AppdIdentity::new(path, Duration::from_secs(5))),
        IdentitySource::Deny => Arc::new(DenyAll),
        IdentitySource::AllowAll => {
            recorder.unauthenticated_allowed();
            Arc::new(AllowAll)
        }
    };
    recorder.identity_source(identity.name());
    identity
}

fn run(args: Args, recorder: Recorder) -> Result<(), String> {
    let composed = compose_backend(&args)?;
    let identity = compose_identity(&args.identity, &recorder);
    let service = Service::new(composed.backend, identity, recorder.clone())
        .map_err(|e| format!("backend snapshot: {e}"))?;
    service.start();

    match (&args.mock_control, composed.control) {
        (ChannelPath::None, _) => recorder.mock_control("none", None::<&Path>, None),
        (channel, Some(handler)) => match channel.resolve() {
            Some(resolved) => {
                let control = MockControl::new(Arc::clone(&service), handler);
                ferrix_mock_control::spawn(
                    resolved,
                    control,
                    ControlObserver::new(recorder.clone()),
                )?;
            }
            None => recorder.mock_control(
                "none",
                None::<&Path>,
                Some("no virtio-serial port named ferrix.network"),
            ),
        },
        (_, None) => return Err(CliError::MockControlWithProductionBackend.to_string()),
    }

    let listener = Server::bind(&args.socket)?;
    recorder.ready(&args.socket, service.backend_name());
    Arc::new(Server::new(service)).serve(listener);
    Err("socket listener ended".to_owned())
}

fn main() -> ExitCode {
    let env = |key: &str| std::env::var(key).ok();
    let args = match cli::parse(std::env::args().skip(1), &env) {
        Ok(args) => args,
        Err(error) => {
            Recorder::stdout(ferrix_networkd::records::EVIDENCE_MOCK).failed(&error.to_string());
            return ExitCode::from(2);
        }
    };
    let recorder = Recorder::stdout(args.evidence());
    match run(args, recorder.clone()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(reason) => {
            recorder.failed(&reason);
            ExitCode::from(1)
        }
    }
}
