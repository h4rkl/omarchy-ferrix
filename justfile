set shell := ["/bin/sh", "-eu", "-c"]

root := justfile_directory()
out := env_var_or_default("FERRIX_OUT_DIR", root + "/out") + "/qemu-aarch64"

host-check:
    @echo "Host checks (no tools are installed)"
    @command -v qemu-system-aarch64 >/dev/null && qemu-system-aarch64 --version | head -1 || echo "FAIL qemu-system-aarch64 — brew install qemu"
    @command -v docker >/dev/null && docker version >/dev/null 2>&1 && echo "PASS docker runtime" || echo "FAIL docker runtime — start Docker Desktop"
    @rustc --version
    @cargo --version
    @rustup target list --installed | grep -qx aarch64-unknown-linux-gnu && echo "PASS Rust aarch64 target" || echo "FAIL Rust target — rustup target add aarch64-unknown-linux-gnu"
    @command -v aarch64-linux-gnu-gcc >/dev/null && echo "PASS aarch64 linker" || echo "WARN aarch64 linker unavailable locally — guest-builder container will provide it"
    @command -v cpio >/dev/null && echo "PASS cpio" || echo "FAIL cpio — brew install cpio"
    @echo "resolved out: {{out}}"

oneplus-tools-check:
    @command -v adb >/dev/null && adb version | head -2 || { echo "FAIL adb — brew install --cask android-platform-tools" >&2; exit 1; }
    @command -v fastboot >/dev/null && fastboot --version | head -1 || { echo "FAIL fastboot — brew install --cask android-platform-tools" >&2; exit 1; }

oneplus-discover:
    @"{{root}}/tools/oneplus/discover.sh"

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

qemu-check:
    @"{{root}}/targets/qemu-aarch64/scripts/qemu-check.sh"

qemu-clean:
    @echo "Removing resolved generated path: {{out}}"
    @test "{{out}}" != "/" && rm -rf "{{out}}"

shell-nested:
    @"{{root}}/targets/qemu-aarch64/scripts/shell-nested.sh"

# Product compositor (ferrix-shell) nested in a host window; Linux host session only.
shell-compositor-nested:
    @"{{root}}/targets/qemu-aarch64/scripts/shell-nested.sh"

# Positional args ("$@") keep raw JSON commands intact through the shell.
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
