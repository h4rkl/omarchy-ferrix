use hardware_api::Power;
use hardware_emulator::{EmulatorPower, Profile};
use std::{env, fs, process::ExitCode};

fn main() -> ExitCode {
    let profile_path = env::args()
        .nth(1)
        .unwrap_or_else(|| "targets/qemu-aarch64/profile.toml".to_owned());
    let text = match fs::read_to_string(&profile_path) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("mobile-core: cannot read {profile_path}: {error}");
            return ExitCode::FAILURE;
        }
    };
    let profile = match Profile::parse(&text) {
        Ok(profile) => profile,
        Err(error) => {
            eprintln!("mobile-core: invalid profile: {error}");
            return ExitCode::FAILURE;
        }
    };
    let state = EmulatorPower::new(&profile).state();
    println!(
        "{{\"evidence\":\"mock\",\"event\":\"boot-health\",\"status\":\"ok\",\"profile\":\"{}\",\"battery_percent\":{},\"charging\":{}}}",
        profile.name, state.battery_percent, state.charging
    );
    ExitCode::SUCCESS
}
