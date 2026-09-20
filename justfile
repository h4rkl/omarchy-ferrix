set shell := ["/bin/sh", "-eu", "-c"]

root := justfile_directory()
out := env_var_or_default("OMARCHY_OUT_DIR", root + "/out") + "/qemu-aarch64"

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
    @cargo run -p mobile-core -- targets/qemu-aarch64/profile.toml

test:
    @cargo test --workspace
