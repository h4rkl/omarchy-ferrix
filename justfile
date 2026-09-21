set shell := ["/bin/sh", "-eu", "-c"]

root := justfile_directory()
out := env_var_or_default("FERRIX_OUT_DIR", root + "/out") + "/qemu-aarch64"

host-check:
    @"{{root}}/tools/dev/check.sh"

# Explicit, opt-in host package installation. See AGENT_INSTALL.md.
dev-setup:
    @"{{root}}/tools/dev/install.sh"

oneplus-tools-check:
    @command -v adb >/dev/null && adb version | head -2 || { echo "FAIL adb — brew install --cask android-platform-tools" >&2; exit 1; }
    @command -v fastboot >/dev/null && fastboot --version | head -1 || { echo "FAIL fastboot — brew install --cask android-platform-tools" >&2; exit 1; }

# Read-only OnePlus discovery (adb/fastboot identity props). Writes a
# serial-redacted transcript under out/oneplus/; never mutates the device.
oneplus-discover:
    @"{{root}}/tools/oneplus/discover.sh"

# Provisioning gate skeleton: resolves and prints the exact device identity,
# image and recovery hashes, requires a typed confirmation, then REFUSES
# (exit 3, no-approved-proposal). Contains no flash/erase/unlock/boot command.
#   just oneplus-provision-gate --image IMG --recovery BACKUP --partition boot
[positional-arguments]
oneplus-provision-gate *args:
    @"{{root}}/tools/oneplus/provision-gate.sh" "$@"

# Shell tests for the OnePlus tooling (stubbed adb/fastboot; no device needed).
oneplus-tools-test:
    @sh "{{root}}/tools/oneplus/tests/provision-gate-is-inert.sh"

qemu-build:
    @"{{root}}/targets/qemu-aarch64/scripts/qemu-build.sh"

qemu-run:
    @"{{root}}/targets/qemu-aarch64/scripts/qemu-run.sh"

qemu-agent-start:
    @"{{root}}/targets/qemu-aarch64/scripts/qemu-agent-start.sh"

qemu-agent-status:
    @"{{root}}/targets/qemu-aarch64/scripts/qemu-agent-status.sh"

qemu-frame:
    @"{{root}}/targets/qemu-aarch64/scripts/qemu-frame.sh"

qemu-tap x y:
    @"{{root}}/targets/qemu-aarch64/scripts/qemu-input.sh" tap "{{x}}" "{{y}}"

qemu-swipe x1 y1 x2 y2:
    @"{{root}}/targets/qemu-aarch64/scripts/qemu-input.sh" swipe "{{x1}}" "{{y1}}" "{{x2}}" "{{y2}}"

qemu-key key:
    @"{{root}}/targets/qemu-aarch64/scripts/qemu-input.sh" key "{{key}}"

qemu-type text:
    @"{{root}}/targets/qemu-aarch64/scripts/qemu-input.sh" type "{{text}}"

qemu-agent-stop:
    @"{{root}}/targets/qemu-aarch64/scripts/qemu-agent-stop.sh"

# Boot a fresh graphical TCG guest and verify minimize, side-edge yield, and
# long-swipe close through compositor/appd records plus a final framebuffer.
qemu-gesture-check:
    @"{{root}}/targets/qemu-aarch64/scripts/qemu-gesture-check.sh"

# Headless TCG CI contract: boot-health, telephonyd-ready, networkd-ready,
# unplugged-to-charging over ferrix.control, clean poweroff.
# FERRIX_QEMU_REQUIRE_SERVICES=0 skips the two daemon assertions.
qemu-check:
    @"{{root}}/targets/qemu-aarch64/scripts/qemu-check.sh"

# Run one checked-in scenario (targets/qemu-aarch64/scenarios/NAME.toml) against
# the running QEMU session over the per-daemon mock-control channels.
#   just qemu-scenario incoming-call-answer-route-remote-hangup --frame
[positional-arguments]
qemu-scenario name *args:
    @"{{root}}/targets/qemu-aarch64/scripts/qemu-scenario.sh" "$@"

# List the checked-in scenarios.
qemu-scenario-list:
    @ls "{{root}}/targets/qemu-aarch64/scenarios" | sed 's/\.toml$//'

# Validate every scenario file's schema without touching QEMU.
qemu-scenario-validate:
    @cargo run -q --manifest-path "{{root}}/Cargo.toml" -p ferrix-scenario -- --validate "{{root}}"/targets/qemu-aarch64/scenarios/*.toml

# Headless TCG run of every checked-in scenario in a fresh agent session
# (needs `just qemu-build` artifacts). FERRIX_QEMU_SCENARIO_TIMEOUT bounds it.
qemu-scenario-check:
    @"{{root}}/targets/qemu-aarch64/scripts/qemu-scenario-check.sh"

qemu-clean:
    @echo "Removing resolved generated path: {{out}}"
    @test "{{out}}" != "/" && rm -rf "{{out}}"

shell-nested:
    @"{{root}}/targets/qemu-aarch64/scripts/shell-nested.sh"

# Product compositor (ferrix-shell) nested in a host window; Linux host session only.
shell-compositor-nested:
    @"{{root}}/targets/qemu-aarch64/scripts/shell-nested.sh"

# Positional args ("$@") keep raw JSON commands intact through the shell.
#   just qemu-mock set-power charging=true                         # ferrix-core
#   just qemu-mock --service telephony incoming-call number=+15550100042
#   just qemu-mock --service network set-radio enabled=false
[positional-arguments]
qemu-mock +args:
    @"{{root}}/targets/qemu-aarch64/scripts/qemu-mock.sh" "$@"

arch-check:
    @cargo test -p ferrix-session --test architecture

# Cargo for the Linux guest architecture inside the pinned arm64 Rust builder
# (host convenience for Linux-only crates; defaults to `test --workspace`).
[positional-arguments]
linux-check *args:
    @"{{root}}/targets/qemu-aarch64/scripts/linux-check.sh" "$@"

test:
    @cargo test --workspace

pmos-build:
    @"{{root}}/targets/qemu-aarch64/scripts/pmos-build.sh"

pmos-run:
    @"{{root}}/targets/qemu-aarch64/scripts/pmos-run.sh"

pmos-check:
    @"{{root}}/targets/qemu-aarch64/scripts/pmos-check.sh"

# OPT-IN Android compatibility runtime (Waydroid) on the pmOS route. Separate
# image under out/qemu-aarch64/pmos-android/; the default pmOS build, check,
# and CI are unchanged. The graphical route has QEMU-only Brave evidence.
waydroid-images-fetch:
    @sh "{{root}}/tools/android/waydroid-images-fetch.sh"

# Host-only deterministic test of the guest's staged-APK bootstrap. It uses a
# fake Waydroid CLI and never starts a container, QEMU, or a physical device.
android-bootstrap-test:
    @sh "{{root}}/targets/qemu-aarch64/scripts/android-bootstrap-test.sh"

pmos-android-build:
    @FERRIX_PMOS_ANDROID=1 "{{root}}/targets/qemu-aarch64/scripts/pmos-build.sh"

pmos-android-run:
    @FERRIX_PMOS_ANDROID=1 "{{root}}/targets/qemu-aarch64/scripts/pmos-run.sh"

pmos-android-check:
    @FERRIX_PMOS_ANDROID=1 "{{root}}/targets/qemu-aarch64/scripts/pmos-check.sh"
