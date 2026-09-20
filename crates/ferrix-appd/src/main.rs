//! `ferrix_appd` binary: composes the native runtime with the service and serves
//! the socket.
//!
//! 1. loads the catalog from every `--app-dir` (default
//!    `/usr/share/ferrix/apps` if it exists, else `apps`);
//! 2. binds the socket (`--socket`, `FERRIX_APPD_SOCKET`, default
//!    `/run/ferrix/ferrix_appd.sock`) and only then prints `ferrix_appd-ready`;
//! 3. autostarts the home surface and every `autostart` app;
//! 4. serves connections forever.
//!
//! Any startup failure prints `ferrix_appd-failed` and exits non-zero. This is the
//! only place that names a runtime implementation.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use ferrix_app_api::{DEFAULT_APPD_SOCKET, GUEST_APP_DIR, REPO_APP_DIR};
use ferrix_app_runtime_api::Runtime;
use ferrix_appd::process_tree::ProcTree;
use ferrix_appd::records::{EVIDENCE_MOCK, EVIDENCE_QEMU, Recorder};
use ferrix_appd::{Catalog, Server, Service, ServiceOptions};
use ferrix_runtime_native::{NativeRuntime, NativeRuntimeOptions};

const USAGE: &str =
    "usage: ferrix-appd [--socket PATH] [--app-dir DIR]... [--nested] [--no-autostart] [--sandbox PATH]

  --socket PATH    socket to serve (env FERRIX_APPD_SOCKET, default /run/ferrix/ferrix_appd.sock)
  --app-dir DIR    descriptor directory, repeatable (env FERRIX_APP_DIR;
                   default /usr/share/ferrix/apps if present, else apps)
  --nested         nested host run: records carry evidence \"mock\" instead of \"QEMU\"
  --no-autostart   do not launch the home surface or autostart apps
  --sandbox PATH   ferrix-sandbox binary (env FERRIX_SANDBOX_BIN, default PATH lookup)";

struct Args {
    socket: PathBuf,
    app_dirs: Vec<PathBuf>,
    nested: bool,
    autostart: bool,
    sandbox: Option<PathBuf>,
}

enum Parsed {
    Run(Args),
    Help,
}

fn parse_args(argv: impl IntoIterator<Item = String>) -> Result<Parsed, String> {
    let env = |key: &str| std::env::var(key).ok().filter(|v| !v.is_empty());
    let mut args = Args {
        socket: PathBuf::from(env("FERRIX_APPD_SOCKET").unwrap_or(DEFAULT_APPD_SOCKET.to_owned())),
        app_dirs: env("FERRIX_APP_DIR")
            .map(PathBuf::from)
            .into_iter()
            .collect(),
        nested: false,
        autostart: true,
        sandbox: env("FERRIX_SANDBOX_BIN").map(PathBuf::from),
    };
    let mut explicit_dirs = false;
    let mut it = argv.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--socket" => args.socket = PathBuf::from(it.next().ok_or("--socket needs a path")?),
            "--app-dir" => {
                if !explicit_dirs {
                    args.app_dirs.clear();
                    explicit_dirs = true;
                }
                args.app_dirs.push(PathBuf::from(
                    it.next().ok_or("--app-dir needs a directory")?,
                ));
            }
            "--nested" => args.nested = true,
            "--no-autostart" => args.autostart = false,
            "--sandbox" => {
                args.sandbox = Some(PathBuf::from(it.next().ok_or("--sandbox needs a path")?))
            }
            "--help" | "-h" => return Ok(Parsed::Help),
            other => return Err(format!("unknown argument {other}")),
        }
    }
    if args.app_dirs.is_empty() {
        args.app_dirs.push(default_app_dir());
    }
    Ok(Parsed::Run(args))
}

fn default_app_dir() -> PathBuf {
    let guest = Path::new(GUEST_APP_DIR);
    if guest.is_dir() {
        guest.to_path_buf()
    } else {
        PathBuf::from(REPO_APP_DIR)
    }
}

fn run(args: Args, recorder: Recorder) -> Result<(), String> {
    let mut catalog = Catalog::new();
    for dir in &args.app_dirs {
        catalog.extend_from_dir(dir).map_err(|e| e.to_string())?;
    }
    if catalog.is_empty() {
        return Err(format!(
            "no descriptors found in {}",
            args.app_dirs
                .iter()
                .map(|d| d.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    // Composition point: the only place a runtime implementation is named.
    let native: Box<dyn Runtime> = Box::new(NativeRuntime::new(NativeRuntimeOptions {
        sandbox_bin: args.sandbox.clone(),
        evidence: recorder.evidence().to_owned(),
    }));

    let listener = Server::bind(&args.socket)?;
    let options = ServiceOptions {
        socket: args.socket.clone(),
        ..ServiceOptions::default()
    };
    let service = Arc::new(Service::new(
        catalog,
        vec![native],
        Box::new(ProcTree::new()),
        recorder.clone(),
        options,
    ));

    recorder.appd_ready(
        &args.socket,
        service.catalog().len(),
        service.catalog().home().map(|d| &d.id),
    );
    service.announce_catalog();

    if args.autostart {
        // Failures are recorded (`app-launch-failed`); a tool can retry them.
        let _ = service.autostart();
    }

    Arc::new(Server::new(service)).serve(listener);
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
            Recorder::stdout(EVIDENCE_QEMU).appd_failed(&error);
            return ExitCode::FAILURE;
        }
    };
    let recorder = Recorder::stdout(if args.nested {
        EVIDENCE_MOCK
    } else {
        EVIDENCE_QEMU
    });
    match run(args, recorder.clone()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            recorder.appd_failed(&error);
            ExitCode::FAILURE
        }
    }
}
