//! `ferrix-scenario`: run checked-in scenario files against a QEMU guest or a
//! nested host session. See `targets/qemu-aarch64/README.md` ("Scenarios").
//!
//! ```text
//! ferrix-scenario [--guest | --nested --log PATH] [--out DIR] [--sockets DIR]
//!                 [--serial PATH] [--frame] [--qmp PATH] [--qmp-helper PATH]
//!                 [--validate] SCENARIO.toml...
//! ```
//!
//! Exit status: 0 when every scenario passed, 1 when one failed, 2 on usage or
//! setup errors. One `scenario-passed`/`scenario-failed` record per scenario
//! is printed on stdout; the evidence directory holds the details.

use std::path::PathBuf;
use std::process::ExitCode;

use ferrix_scenario::{FrameCapture, Mode, Scenario, run};

const USAGE: &str = "usage: ferrix-scenario [--guest | --nested --log PATH] [--out DIR] [--sockets DIR] [--serial PATH] [--frame] [--qmp PATH] [--qmp-helper PATH] [--validate] SCENARIO.toml...";

struct Args {
    guest: bool,
    nested: bool,
    log: Option<PathBuf>,
    out: Option<PathBuf>,
    sockets: Option<PathBuf>,
    serial: Option<PathBuf>,
    frame: bool,
    qmp: Option<PathBuf>,
    qmp_helper: Option<PathBuf>,
    validate: bool,
    files: Vec<PathBuf>,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        guest: false,
        nested: false,
        log: None,
        out: None,
        sockets: None,
        serial: None,
        frame: false,
        qmp: None,
        qmp_helper: None,
        validate: false,
        files: Vec::new(),
    };
    let mut iter = std::env::args_os().skip(1);
    while let Some(arg) = iter.next() {
        let text = arg.to_string_lossy().into_owned();
        let mut path_value = |name: &str| -> Result<PathBuf, String> {
            iter.next()
                .map(PathBuf::from)
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match text.as_str() {
            "--guest" => args.guest = true,
            "--nested" => args.nested = true,
            "--log" => args.log = Some(path_value("--log")?),
            "--out" => args.out = Some(path_value("--out")?),
            "--sockets" => args.sockets = Some(path_value("--sockets")?),
            "--serial" => args.serial = Some(path_value("--serial")?),
            "--frame" => args.frame = true,
            "--qmp" => args.qmp = Some(path_value("--qmp")?),
            "--qmp-helper" => args.qmp_helper = Some(path_value("--qmp-helper")?),
            "--validate" => args.validate = true,
            "-h" | "--help" => return Err(USAGE.to_owned()),
            other if other.starts_with('-') => {
                return Err(format!("unknown option {other}\n{USAGE}"));
            }
            _ => args.files.push(PathBuf::from(arg)),
        }
    }
    if args.files.is_empty() {
        return Err(format!("no scenario files given\n{USAGE}"));
    }
    if args.guest && args.nested {
        return Err("--guest and --nested are mutually exclusive".to_owned());
    }
    if args.nested && args.log.is_none() {
        return Err("--nested requires --log PATH (the combined daemon stdout log)".to_owned());
    }
    Ok(args)
}

fn resolve_mode(args: &Args) -> Result<(Mode, PathBuf), String> {
    let out = args.out.clone().unwrap_or_else(|| {
        std::env::var_os("FERRIX_OUT_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("out"))
            .join("qemu-aarch64")
    });
    if args.nested {
        let sockets = args.sockets.clone().unwrap_or_else(|| out.clone());
        let log = args.log.clone().expect("checked in parse_args");
        return Ok((Mode::Nested { sockets, log }, out));
    }
    // Guest mode: the agent session when it has a serial log, else the plain
    // qemu-run/qemu-check session directory.
    let sockets = args.sockets.clone().unwrap_or_else(|| {
        if out.join("agent").join("serial.log").is_file() {
            out.join("agent")
        } else {
            out.clone()
        }
    });
    let serial_log = args
        .serial
        .clone()
        .unwrap_or_else(|| sockets.join("serial.log"));
    if !args.guest && !serial_log.is_file() {
        return Err(format!(
            "no serial log at {} (start a QEMU session, or pass --nested --log PATH)",
            serial_log.display()
        ));
    }
    let frame = if args.frame {
        let qmp_socket = args.qmp.clone().unwrap_or_else(|| sockets.join("qmp.sock"));
        let qmp_helper = args
            .qmp_helper
            .clone()
            .unwrap_or_else(|| PathBuf::from("targets/qemu-aarch64/scripts/qmp.py"));
        if !qmp_helper.is_file() {
            return Err(format!(
                "qmp helper {} not found; pass --qmp-helper",
                qmp_helper.display()
            ));
        }
        Some(FrameCapture {
            qmp_socket,
            qmp_helper,
        })
    } else {
        None
    };
    Ok((
        Mode::Guest {
            sockets,
            serial_log,
            frame,
        },
        out,
    ))
}

fn setup_error(reason: &str) -> ExitCode {
    println!(
        "{}",
        serde_json::json!({"evidence":"mock","event":"scenario-runner-failed","reason":reason})
    );
    eprintln!("ferrix-scenario: {reason}");
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(args) => args,
        Err(reason) => return setup_error(&reason),
    };
    let mut scenarios = Vec::new();
    for file in &args.files {
        match Scenario::load(file) {
            Ok(scenario) => scenarios.push(scenario),
            Err(e) => return setup_error(&e.to_string()),
        }
    }
    if args.validate {
        for scenario in &scenarios {
            println!(
                "{}",
                serde_json::json!({"evidence":"mock","event":"scenario-valid","name":scenario.name,"steps":scenario.steps.len()})
            );
        }
        return ExitCode::SUCCESS;
    }
    let (mode, out) = match resolve_mode(&args) {
        Ok(resolved) => resolved,
        Err(reason) => return setup_error(&reason),
    };
    let mut failed = 0;
    for scenario in &scenarios {
        match run(scenario, &mode, &out) {
            Ok(outcome) => {
                println!("{}", outcome.record(&mode));
                if !outcome.passed {
                    failed += 1;
                }
            }
            Err(reason) => {
                return setup_error(&format!("{}: {reason}", scenario.name));
            }
        }
    }
    if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}
