//! `ferrix_appd` binary: composes the native and Android runtimes with the service and serves
//! the socket.
//!
//! 1. loads the catalog from every `--app-dir` (default
//!    `/usr/share/ferrix/apps` if it exists, else `apps`);
//! 2. loads the grants policy (`--grants`, `FERRIX_APP_GRANTS`, default
//!    `<first app dir>/grants.json`; absent means the empty policy);
//! 3. binds the socket (`--socket`, `FERRIX_APPD_SOCKET`, default
//!    `/run/ferrix/ferrix-appd.sock`) and only then prints `ferrix-appd-ready`;
//! 4. autostarts the home surface and every `autostart` app;
//! 5. serves connections forever.
//!
//! Any startup failure prints `ferrix-appd-failed` and exits non-zero. This is the
//! only place that names a runtime implementation.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use ferrix_app_api::{
    DEFAULT_APPD_SOCKET, GRANTS_PATH_ENV, GUEST_APP_DIR, REPO_APP_DIR, TELEPHONY_APN_ENV,
};
use ferrix_app_runtime_api::Runtime;
use ferrix_appd::process_tree::ProcTree;
use ferrix_appd::records::{Recorder, runtime_evidence};
use ferrix_appd::{Catalog, GrantsSource, Server, Service, ServiceOptions, load_grants};
use ferrix_runtime_android::{AndroidRuntime, AndroidRuntimeOptions, WaydroidCli, WaydroidOptions};
use ferrix_runtime_native::{NativeRuntime, NativeRuntimeOptions};

const USAGE: &str =
    "usage: ferrix-appd [--socket PATH] [--app-dir DIR]... [--grants PATH] [--nested] [--no-autostart] [--sandbox PATH]

  --socket PATH    socket to serve (env FERRIX_APPD_SOCKET, default /run/ferrix/ferrix-appd.sock)
  --app-dir DIR    descriptor directory, repeatable (env FERRIX_APP_DIR;
                   default /usr/share/ferrix/apps if present, else apps)
  --grants PATH    grants policy (env FERRIX_APP_GRANTS; default <first app dir>/grants.json
                   when present, else the empty policy: no app holds any capability)
  --nested         nested host run: records carry evidence \"mock\" instead of \"QEMU\"
  --no-autostart   do not launch the home surface or autostart apps
  --sandbox PATH   ferrix-sandbox binary (env FERRIX_SANDBOX_BIN, default PATH lookup)";

struct Args {
    socket: PathBuf,
    app_dirs: Vec<PathBuf>,
    grants: Option<PathBuf>,
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
        grants: env(GRANTS_PATH_ENV).map(PathBuf::from),
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
            "--grants" => {
                args.grants = Some(PathBuf::from(it.next().ok_or("--grants needs a path")?))
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

fn run(args: Args, recorder: Recorder, telephony_apn: Option<String>) -> Result<(), String> {
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

    // Authorization policy. A present-but-invalid file is fatal; an absent
    // default is the empty policy and is recorded as such.
    let (grants, source) = match load_grants(args.grants.as_deref(), &args.app_dirs) {
        Ok(loaded) => loaded,
        Err(error) => {
            recorder.grants_invalid(&error.to_string());
            return Err(error.to_string());
        }
    };
    match &source {
        GrantsSource::File(path) => recorder.grants_loaded(grants.len(), path),
        GrantsSource::Missing => recorder.grants_missing(),
    }

    // Composition point: the only place runtime implementations are named.
    // Further runtimes (`Box<dyn Runtime>`) are pushed into this vector.
    let native: Box<dyn Runtime> = Box::new(NativeRuntime::new(NativeRuntimeOptions {
        sandbox_bin: args.sandbox.clone(),
        evidence: recorder.evidence().to_owned(),
    }));
    // Android compatibility runtime (ADR-005): a private adapter over the
    // maintained Waydroid CLI. Nothing is probed here; an Android launch on a
    // guest without the runtime yields a typed `RuntimeUnsupported` with the
    // reason in the launch record, never a silent success.
    let android: Box<dyn Runtime> = Box::new(AndroidRuntime::new(
        Arc::new(WaydroidCli::new(WaydroidOptions::default())),
        AndroidRuntimeOptions {
            evidence: recorder.evidence().to_owned(),
            ..AndroidRuntimeOptions::default()
        },
    ));
    let runtimes: Vec<Box<dyn Runtime>> = vec![native, android];

    let listener = Server::bind(&args.socket)?;
    let options = ServiceOptions {
        socket: args.socket.clone(),
        telephony_apn,
        grants,
        ..ServiceOptions::default()
    };
    let service = Arc::new(Service::new(
        catalog,
        runtimes,
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
            let recorder = Recorder::stdout(&runtime_evidence(false));
            recorder.appd_failed(&error);
            return ExitCode::FAILURE;
        }
    };
    let evidence = runtime_evidence(args.nested);
    let recorder = Recorder::stdout(&evidence);
    let telephony_apn = std::env::var(TELEPHONY_APN_ENV)
        .ok()
        .filter(|value| !value.is_empty());
    match run(args, recorder.clone(), telephony_apn) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            recorder.appd_failed(&error);
            ExitCode::FAILURE
        }
    }
}
