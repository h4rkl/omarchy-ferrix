//! `ferrix-contactsd` binary.
//!
//! Usage:
//!   ferrix-contactsd [--socket PATH] [--data-dir PATH] [--appd PATH|none]
//!                    [--nested] [--fixtures PATH] [--allow-unauthenticated]
//!                    [--region CC]
//!
//! `--nested` labels evidence `mock` and defaults the socket and data
//! directory under `out/qemu-aarch64/`. `--appd none` denies every request
//! unless `--allow-unauthenticated` is also given (nested development; a
//! warning record says so). `--fixtures` loads a deterministic synthetic
//! contact set when the store is empty. Environment: `FERRIX_CONTACTSD_SOCKET`,
//! `FERRIX_CONTACTS_DIR`, `FERRIX_APPD_SOCKET`, `FERRIX_DEFAULT_REGION`.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use ferrix_contactsd::{AllowAll, AppdIdentity, DenyAll, Identity, Recorder, Server, Service};

const DEFAULT_APPD_SOCKET: &str = "/run/ferrix/ferrix-appd.sock";
const NESTED_SOCKET: &str = "out/qemu-aarch64/contactsd.sock";
const NESTED_DATA_DIR: &str = "out/qemu-aarch64/contacts";

struct Args {
    socket: PathBuf,
    data_dir: PathBuf,
    appd: Option<PathBuf>,
    nested: bool,
    fixtures: Option<PathBuf>,
    allow_unauthenticated: bool,
    region: String,
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut socket = None;
    let mut data_dir = None;
    let mut appd: Option<Option<PathBuf>> = None;
    let mut nested = false;
    let mut fixtures = None;
    let mut allow_unauthenticated = false;
    let mut region = std::env::var(ferrix_contacts_api::DEFAULT_REGION_ENV)
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| ferrix_contacts_api::DEFAULT_REGION.to_owned());
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        let mut value = |flag: &str| it.next().ok_or_else(|| format!("{flag} requires a value"));
        match arg.as_str() {
            "--socket" => socket = Some(PathBuf::from(value("--socket")?)),
            "--data-dir" => data_dir = Some(PathBuf::from(value("--data-dir")?)),
            "--appd" => {
                let v = value("--appd")?;
                appd = Some((v != "none").then(|| PathBuf::from(v)));
            }
            "--nested" => nested = true,
            "--fixtures" => fixtures = Some(PathBuf::from(value("--fixtures")?)),
            "--allow-unauthenticated" => allow_unauthenticated = true,
            "--region" => region = value("--region")?,
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    let socket = socket
        .or_else(|| env_path(ferrix_contacts_protocol::SOCKET_ENV))
        .unwrap_or_else(|| {
            PathBuf::from(if nested {
                NESTED_SOCKET
            } else {
                ferrix_contacts_protocol::DEFAULT_SOCKET
            })
        });
    let data_dir = data_dir
        .or_else(|| env_path(ferrix_contactsd::store::DIR_ENV))
        .unwrap_or_else(|| {
            PathBuf::from(if nested {
                NESTED_DATA_DIR
            } else {
                ferrix_contactsd::store::DEFAULT_DIR
            })
        });
    let appd = match appd {
        Some(explicit) => explicit,
        None => Some(
            env_path("FERRIX_APPD_SOCKET").unwrap_or_else(|| PathBuf::from(DEFAULT_APPD_SOCKET)),
        ),
    };
    Ok(Args {
        socket,
        data_dir,
        appd,
        nested,
        fixtures,
        allow_unauthenticated,
        region,
    })
}

fn main() -> ExitCode {
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(error) => {
            Recorder::stdout(ferrix_contactsd::EVIDENCE_MOCK).failed(&error);
            return ExitCode::FAILURE;
        }
    };
    let evidence = if args.nested {
        ferrix_contactsd::EVIDENCE_MOCK
    } else {
        ferrix_contactsd::EVIDENCE_QEMU
    };
    let recorder = Recorder::stdout(evidence);
    let identity: Box<dyn Identity> = match (&args.appd, args.allow_unauthenticated) {
        (_, true) => {
            recorder.emit(
                "contactsd-warning",
                [(
                    "reason",
                    serde_json::Value::from(
                        "allow-unauthenticated: every peer is granted read and write",
                    ),
                )],
            );
            Box::new(AllowAll)
        }
        (Some(socket), false) => Box::new(AppdIdentity::new(socket, Duration::from_secs(2))),
        (None, false) => Box::new(DenyAll),
    };
    let identity_label = identity.describe();
    let service = match Service::open(
        &args.data_dir,
        identity,
        recorder.clone(),
        &args.region,
        args.fixtures.as_deref(),
    ) {
        Ok(service) => service,
        Err(error) => {
            recorder.failed(&error.to_string());
            return ExitCode::FAILURE;
        }
    };
    let listener = match Server::bind(&args.socket) {
        Ok(listener) => listener,
        Err(error) => {
            recorder.failed(&error);
            return ExitCode::FAILURE;
        }
    };
    recorder.emit(
        "contactsd-identity",
        [
            ("source", serde_json::Value::from(identity_label)),
            ("region", serde_json::Value::from(args.region.clone())),
            (
                "data_dir",
                serde_json::Value::from(args.data_dir.display().to_string()),
            ),
        ],
    );
    recorder.ready(&args.socket, service.contact_count());
    Arc::new(Server::new(service)).serve(listener);
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_have_guest_and_nested_defaults() {
        let args = parse_args(Vec::<String>::new()).unwrap();
        assert!(args.socket.ends_with("contactsd.sock"));
        assert!(!args.nested);
        let args = parse_args(
            [
                "--nested",
                "--appd",
                "none",
                "--fixtures",
                "f.json",
                "--region",
                "GB",
            ]
            .map(str::to_owned),
        )
        .unwrap();
        assert_eq!(args.socket, PathBuf::from(NESTED_SOCKET));
        assert_eq!(args.data_dir, PathBuf::from(NESTED_DATA_DIR));
        assert_eq!(args.appd, None);
        assert_eq!(args.fixtures, Some(PathBuf::from("f.json")));
        assert_eq!(args.region, "GB");
        assert!(!args.allow_unauthenticated);
        let args =
            parse_args(["--appd", "/tmp/a.sock", "--allow-unauthenticated"].map(str::to_owned))
                .unwrap();
        assert_eq!(args.appd, Some(PathBuf::from("/tmp/a.sock")));
        assert!(args.allow_unauthenticated);
        assert!(parse_args(["--bogus".to_owned()]).is_err());
        assert!(parse_args(["--socket".to_owned()]).is_err());
    }
}
