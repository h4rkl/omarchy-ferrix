set shell := ["/bin/sh", "-eu", "-c"]

root := justfile_directory()
out := env_var_or_default("FERRIX_OUT_DIR", root + "/out") + "/qemu-aarch64"

host-check:
    @"{{root}}/tools/dev/check.sh"

license-check:
    @sh "{{root}}/tools/dev/license-check.sh"

# Explicit, opt-in host package installation. See AGENT_INSTALL.md.
dev-setup:
    @"{{root}}/tools/dev/install.sh"

oneplus-tools-check:
    @command -v adb >/dev/null && adb version | head -2 || { echo "FAIL adb — brew install --cask android-platform-tools" >&2; exit 1; }
    @command -v fastboot >/dev/null && fastboot --version | head -1 || { echo "FAIL fastboot — brew install --cask android-platform-tools" >&2; exit 1; }

# Pixel 9a read-only tooling checks and identity discovery. The workflow
# refuses zero or multiple devices and never mutates a physical device.
pixel-9a-tools-check:
    @command -v adb >/dev/null && adb version | head -2 || { echo "FAIL adb — brew install --cask android-platform-tools" >&2; exit 1; }
    @command -v fastboot >/dev/null && fastboot --version | head -1 || { echo "FAIL fastboot — brew install --cask android-platform-tools" >&2; exit 1; }

pixel-9a-discover:
    @"{{root}}/tools/pixel-9a/discover.sh"

# Offline safety/privacy tests with strict adb/fastboot stubs; never contacts a
# physical device.
pixel-9a-tools-test:
    @sh "{{root}}/tools/pixel-9a/tests/discover-read-only.sh"
    @sh "{{root}}/tools/pixel-9a/tests/inspect-factory.sh"
    @sh "{{root}}/tools/pixel-9a/tests/sideload-workflow.sh"
    @sh "{{root}}/tools/pixel-9a/tests/factory-restore-workflow.sh"
    @sh "{{root}}/tools/pixel-9a/tests/unlock-workflow.sh"

# Separate native-Linux Pixel host environment. Loading it is opt-in and
# read-only; it never creates output directories or contacts a device.
pixel-9a-env-validate:
    @"{{root}}/tools/pixel-9a/env.sh" validate

pixel-9a-env-print:
    @"{{root}}/tools/pixel-9a/env.sh" print

pixel-9a-env-check group="all":
    @"{{root}}/tools/pixel-9a/env.sh" check "{{group}}"

pixel-9a-env-test:
    @sh "{{root}}/tools/pixel-9a/tests/env.sh"

pixel-9a-recovery-download:
    @"{{root}}/tools/pixel-9a/recovery-download.sh"

# Offline only: verify the pinned local factory archive and record its package,
# android-info, fastboot-info, and nested image inventory under out/pixel-9a/.
pixel-9a-factory-inspect:
    @"{{root}}/tools/pixel-9a/inspect-factory.sh"

# Native-Linux-only, device-free Google downstream kernel workflow. Sync is a
# large opt-in download; build uses Google's Kleaf/Bazel target and writes only
# to ignored cache/output paths.
pixel-9a-kernel-check:
    @"{{root}}/tools/pixel-9a/kernel.sh" preflight

pixel-9a-kernel-sync:
    @"{{root}}/tools/pixel-9a/kernel.sh" sync

pixel-9a-kernel-verify:
    @"{{root}}/tools/pixel-9a/kernel.sh" verify

pixel-9a-kernel-build:
    @"{{root}}/tools/pixel-9a/kernel.sh" build

pixel-9a-kernel-test:
    @sh "{{root}}/tools/pixel-9a/tests/kernel-workflow.sh"

# Separate native-Linux Kbuild route for the radio-first s5300_modem WWAN
# backend. It is deliberately not a mode of the Google Kleaf/cpif workflow.
pixel-9a-kernel-radio-check:
    @"{{root}}/tools/pixel-9a/kernel-radio.sh" preflight

pixel-9a-kernel-radio-sync:
    @"{{root}}/tools/pixel-9a/kernel-radio.sh" sync

pixel-9a-kernel-radio-verify:
    @"{{root}}/tools/pixel-9a/kernel-radio.sh" verify

pixel-9a-kernel-radio-build:
    @"{{root}}/tools/pixel-9a/kernel-radio.sh" build

pixel-9a-kernel-radio-test:
    @sh "{{root}}/tools/pixel-9a/tests/kernel-radio-workflow.sh"

# Read-only live partition inventory. The workflow refuses ambiguous devices
# and writes only a redacted ignored transcript.
pixel-9a-partition-inventory:
    @"{{root}}/tools/pixel-9a/partition-inventory.sh"

# Read-only exact-target byte-capacity handoff for the radio-first split
# composer. It reads both A/B static and logical mappings over ADB, or the
# independent A/B partition-size getvars from an owner-entered Fastbootd
# session, and preserves the broader inventory's complete/partial result.
pixel-9a-partition-capacity:
    @"{{root}}/tools/pixel-9a/partition-capacity.sh"

pixel-9a-partition-test:
    @sh "{{root}}/tools/pixel-9a/tests/partition-inventory.sh"

# This may succeed while the broader Android inventory remains partial; it
# never authorizes a boot or storage operation.
pixel-9a-partition-capacity-test:
    @sh "{{root}}/tools/pixel-9a/tests/partition-capacity.sh"

# Device-free native-Linux artifact and firmware policy checks. Manifest and
# staging commands remain gated on verified build inputs and explicit opt-in.
pixel-9a-artifacts-check:
    @"{{root}}/tools/pixel-9a/artifacts.sh" check

pixel-9a-artifacts-manifest:
    @"{{root}}/tools/pixel-9a/artifacts.sh" manifest

pixel-9a-artifacts-test:
    @sh "{{root}}/tools/pixel-9a/tests/artifacts-workflow.sh"

pixel-9a-firmware-check:
    @"{{root}}/tools/pixel-9a/firmware.sh" preflight

pixel-9a-firmware-inventory:
    @"{{root}}/tools/pixel-9a/firmware.sh" inventory

pixel-9a-firmware-stage:
    @"{{root}}/tools/pixel-9a/firmware.sh" stage

pixel-9a-firmware-test:
    @sh "{{root}}/tools/pixel-9a/tests/firmware-workflow.sh"

# Pixel product build for the exact target. The product stage is isolated from
# QEMU in its own target-scoped Cargo/cache/output roots and runs on the
# provisioned native Linux x86_64 environment. Kernel/Kleaf, pmOS, recovery,
# and firmware concerns remain separate workflows. The build itself remains
# explicitly opt-in.
pixel-9a-product-check:
    @"{{root}}/tools/pixel-9a/product.sh" preflight

pixel-9a-product-build:
    @"{{root}}/tools/pixel-9a/product.sh" build

pixel-9a-product-test:
    @sh "{{root}}/tools/pixel-9a/tests/product-workflow.sh"

# Device-free split-boot composition. The build route requires pinned kernel
# and rootfs manifests, native boot tooling, and an explicit per-run token.
pixel-9a-boot-check:
    @"{{root}}/tools/pixel-9a/boot.sh" preflight

pixel-9a-boot-build:
    @"{{root}}/tools/pixel-9a/boot.sh" build

pixel-9a-boot-test:
    @sh "{{root}}/tools/pixel-9a/tests/boot-workflow.sh"

# Device-free boot composition for the independent radio-first kernel route.
# Its output directory is separate from the canonical Google Kleaf route and
# is rejected by the canonical boot contract.
pixel-9a-boot-radio-check:
    @"{{root}}/tools/pixel-9a/boot-radio.sh" preflight

pixel-9a-boot-radio-build:
    @"{{root}}/tools/pixel-9a/boot-radio.sh" build

pixel-9a-boot-radio-test:
    @sh "{{root}}/tools/pixel-9a/tests/boot-radio-workflow.sh"

# Device-free native-Linux split-image completion for the independent
# radio-first route. It requires a redacted exact-target partition-capacity
# handoff plus source-built kernel/rootfs/module inputs; it never invokes a
# device operation and keeps the output bootable=false.
pixel-9a-radio-split-check:
    @"{{root}}/tools/pixel-9a/radio-split.sh" preflight

pixel-9a-radio-split-build:
    @"{{root}}/tools/pixel-9a/radio-split.sh" build

pixel-9a-radio-split-test:
    @sh "{{root}}/tools/pixel-9a/tests/radio-split-workflow.sh"

# Native Linux/postmarketOS Pixel 9a rootfs substrate. These recipes are
# device-free; build refuses missing kernel/product/firmware inputs and never
# invokes a boot or flash route.
pixel-9a-rootfs-check:
    @"{{root}}/tools/pixel-9a/rootfs.sh" preflight

pixel-9a-rootfs-validate:
    @"{{root}}/tools/pixel-9a/rootfs.sh" validate

pixel-9a-rootfs-build:
    @"{{root}}/tools/pixel-9a/rootfs.sh" build

pixel-9a-rootfs-test:
    @sh "{{root}}/tools/pixel-9a/tests/rootfs-workflow.sh"

[positional-arguments]
pixel-9a-unlock-bootloader *args:
    @"{{root}}/tools/pixel-9a/unlock-bootloader.sh" "$@"

# Stock recovery routes. Preflight is read-only; execution modes require typed
# per-run confirmations and refuse ambiguous or mismatched devices.
# Stub-tested only; not yet run on hardware.
[positional-arguments]
pixel-9a-sideload-ota *args:
    @"{{root}}/tools/pixel-9a/sideload-ota.sh" "$@"

[positional-arguments]
pixel-9a-factory-restore *args:
    @"{{root}}/tools/pixel-9a/factory-restore.sh" "$@"

# Gated temporary boot and persistent flash of the composed Ferrix split-boot
# artifacts on the exact Pixel 9a. Preflight is read-only; boot-temp and flash
# require exact identity, re-hashed inputs, a verified factory archive, and a
# typed per-run phrase. Stub-tested only; not yet run on hardware.
[positional-arguments]
pixel-9a-flash-check *args:
    @"{{root}}/tools/pixel-9a/flash.sh" preflight "$@"

[positional-arguments]
pixel-9a-flash-boot-temp *args:
    @"{{root}}/tools/pixel-9a/flash.sh" boot-temp "$@"

[positional-arguments]
pixel-9a-flash *args:
    @"{{root}}/tools/pixel-9a/flash.sh" flash "$@"

pixel-9a-flash-test:
    @sh "{{root}}/tools/pixel-9a/tests/flash-workflow.sh"

# Radios on a Pixel 9a already running Ferrix/pmOS over the USB network
# (172.16.42.1, SSH). preflight/inventory/evidence are read-only; enable and
# connect-wifi change radio state only, after a typed confirmation. Stub-tested
# only: no radio has been observed working on the physical device.
pixel-9a-radios-check:
    @"{{root}}/tools/pixel-9a/radios.sh" preflight

pixel-9a-radios-inventory:
    @"{{root}}/tools/pixel-9a/radios.sh" inventory

pixel-9a-radios-associated:
    @"{{root}}/tools/pixel-9a/radios.sh" associated

[positional-arguments]
pixel-9a-radios-enable *args:
    @"{{root}}/tools/pixel-9a/radios.sh" enable "$@"

pixel-9a-radios-connect-wifi ssid:
    @"{{root}}/tools/pixel-9a/radios.sh" connect-wifi "{{ssid}}"

pixel-9a-radios-evidence:
    @"{{root}}/tools/pixel-9a/radios.sh" evidence

pixel-9a-radios-test:
	@sh "{{root}}/tools/pixel-9a/tests/radios-workflow.sh"
	@sh "{{root}}/tools/pixel-9a/tests/radio-firmware-runtime.sh"

# Generic, vendor-agnostic read-only device front door. Discovery only uses
# adb devices/getprop and fastboot devices/getvar; matching, proposing, and
# pinned downloads never contact a device. Mutation lives only in target-scoped
# tools such as tools/pixel-9a/. See skills/mobile-device-provisioning/.
#
# Read-only any-vendor ADB/Fastboot identity discovery (redacted transcript).
device-discover:
    @"{{root}}/tools/device/discover.sh"

# Offline: match out/device/identity-latest.env to a checked-in board target.
device-match:
    @"{{root}}/tools/device/match-target.sh"

# Offline: scaffold a board-support proposal for an unmatched device.
device-propose:
    @"{{root}}/tools/device/propose-board.sh"

# Pinned vendor recovery download; requires FERRIX_RECOVERY_DOWNLOAD opt-in.
device-recovery-download target kind="all":
    @"{{root}}/tools/device/recovery-download.sh" --target "{{target}}" --kind "{{kind}}"

# Offline safety/privacy tests with strict adb/fastboot stubs and a file://
# download fixture; never contacts a device or the network.
#
# Offline stub tests for the generic device tools.
device-tools-test:
    @sh "{{root}}/tools/device/tests/discover-read-only.sh"
    @sh "{{root}}/tools/device/tests/match-target.sh"
    @sh "{{root}}/tools/device/tests/propose-board.sh"
    @sh "{{root}}/tools/device/tests/recovery-download.sh"

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
# and CI are unchanged. The Android display density is QEMU-target-local;
# graphical limitations remain recorded in docs/status.md.
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
