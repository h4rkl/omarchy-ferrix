//! Omarchy Ferrix mobile product compositor (`ferrix-shell`).
//!
//! A Smithay-based Wayland compositor for the emulator-first mobile Linux
//! environment: portrait, touch-first, fullscreen-only xdg-shell toplevels,
//! software rendering by default (pixman into DRM dumb buffers on the QEMU
//! virtio-gpu route) and a nested winit window for host development from the
//! same versioned mock profile. Physical Linux evidence is selected by the
//! target bootstrap through `FERRIX_EVIDENCE`.
//!
//! Applications are independent processes supervised by `ferrix_appd`; the
//! compositor links to it (`appd_link`) to report which client pid owns each
//! mapped surface and holds focus, and obeys its activate directives. The
//! trusted bottom-edge home gesture (`gesture`) is recognised here so no
//! application can swallow it.
//!
//! Usage:
//!   ferrix-shell [/etc/ferrix/profile.toml]              # guest: DRM/libinput
//!   ferrix-shell --nested targets/qemu-aarch64/profile.toml # Linux host window
//!   ferrix-shell --print-config PROFILE                     # any host: resolved config

mod appd_link;
mod cli;
mod color;
mod config;
mod evidence;
mod gesture;
mod layout;

#[cfg(target_os = "linux")]
mod compositor;

use std::process::ExitCode;
use std::time::Duration;

use evidence::{Evidence, Value};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let nested = args.iter().any(|a| a == "--nested");
    let evidence = if nested {
        Evidence::Mock
    } else if std::env::var_os("FERRIX_EVIDENCE").is_some() {
        Evidence::Physical
    } else {
        Evidence::Qemu
    };
    match run(args, evidence) {
        Ok(()) => ExitCode::SUCCESS,
        Err((reason, detail)) => {
            evidence::emit(&evidence::compositor_failed(evidence, reason, &detail));
            ExitCode::FAILURE
        }
    }
}

type Failure = (&'static str, String);

fn emit_geometry(evidence: Evidence, config: &config::ShellConfig, plan: layout::OutputLayout) {
    let (logical_w, logical_h) = plan.logical_size();
    evidence::emit(&evidence::record(
        evidence,
        "display-geometry",
        &[
            ("width", config.geometry.width.into()),
            ("height", config.geometry.height.into()),
            ("logical_width", logical_w.into()),
            ("logical_height", logical_h.into()),
            ("rotation", plan.rotation.label().into()),
            ("source", config.geometry_source.label().into()),
            ("profile", config.profile_name.as_str().into()),
            ("socket", config.socket_name.as_str().into()),
            ("touch", Value::Bool(config.touch)),
            ("ferrix-appd", config.appd_label().into()),
        ],
    ));
}

fn run(args: Vec<String>, evidence: Evidence) -> Result<(), Failure> {
    let args = cli::parse(args).map_err(|e| ("usage", e))?;
    if args.help {
        println!("{}", cli::USAGE);
        return Ok(());
    }
    let profile = ferrix_target_profile::Profile::load(&args.profile_path)
        .map_err(|e| ("profile", e.to_string()))?;
    let cmdline = match &args.cmdline_path {
        Some(path) => std::fs::read_to_string(path).ok(),
        None => None,
    };
    let mut config = config::ShellConfig::resolve(
        &profile,
        args.display,
        cmdline.as_deref(),
        args.socket.as_deref(),
    )
    .with_appd(args.appd_socket.clone());
    let mut plan = layout::OutputLayout::plan(
        config.geometry.width,
        config.geometry.height,
        config.orientation,
    );
    if args.print_config {
        emit_geometry(evidence, &config, plan);
        return Ok(());
    }
    // The palette is loaded once, before anything paints: the clear color
    // under every client comes from the same `colors.toml` the surfaces read
    // ($FERRIX_COLORS, then /etc/ferrix/colors.toml, then the built-in). This
    // emits the `theme-loaded` (and, for a bad file, `theme-invalid`) record.
    let evidence_label = evidence.label();
    ferrix_ui::theme::init(&evidence_label, None);

    let core = ferrix_protocol::subscribe(
        std::path::Path::new(&args.core_socket),
        Duration::from_secs(10),
    )
    .map_err(|error| ("core", error))?;
    config.orientation = core.state.display.orientation;
    plan = layout::OutputLayout::plan(
        config.geometry.width,
        config.geometry.height,
        config.orientation,
    );
    evidence::emit(&evidence::record(
        evidence,
        "core-connected",
        &[
            ("socket", args.core_socket.as_str().into()),
            ("target", core.target.as_str().into()),
            ("orientation", core.state.display.orientation.name().into()),
        ],
    ));
    emit_geometry(evidence, &config, plan);

    #[cfg(target_os = "linux")]
    {
        if args.nested {
            compositor::run_nested(config, plan, evidence, core)
        } else {
            compositor::run_udev(config, plan, evidence, core)
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = plan;
        Err((
            "platform",
            "the Wayland compositor only runs on Linux (QEMU guest or nested Linux host); use --print-config or cargo test here".to_owned(),
        ))
    }
}
