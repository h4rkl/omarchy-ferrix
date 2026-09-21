# Agent development install

This is the entry point to give an automated coding agent when it needs to
prepare a fresh Omarchy Ferrix checkout. The installer is intentionally
non-project-specific outside this repository: it resolves the checkout from
its own path, is safe to run repeatedly, and finishes by checking the complete
host contract.

From the repository root, run:

```sh
sh tools/dev/install.sh
```

Do not replace that command with a guessed package list. The checked-in script
installs the versions and host packages expected by this checkout, including:

- the Rust toolchain pinned by `rust-toolchain.toml`, with `rustfmt`, `clippy`,
  and `aarch64-unknown-linux-gnu`;
- `just`, QEMU's AArch64 system emulator and image tools, Python 3, `cpio`,
  hashing/archive utilities, and normal native build tools;
- Docker, which builds the pinned Linux guest and postmarketOS containers; and
- Linux-only libraries needed to compile and run the nested compositor.

The supported bootstrap hosts are Apple Silicon macOS and 64-bit Debian or
Ubuntu Linux. Linux on either `x86_64` or `aarch64` is accepted; the installer
adds QEMU binfmt support for the repository's pinned arm64 containers.

The script changes host packages and downloads Rust components. On macOS it
may open Docker Desktop, whose first-run license and privileged configuration
must be completed by the human owner. On Linux it may require `sudo`; a newly
added Docker group membership requires starting a new login shell. Those are
the only expected interactive boundaries, and the script prints the exact
continuation command if it encounters one.

To inspect the environment without changing it, run either:

```sh
sh tools/dev/check.sh
# or, once just is installed
just host-check
```

After a successful install, establish the normal development baseline with:

```sh
cargo test --workspace
just qemu-build
just qemu-check
```

`qemu-build` downloads pinned container images and can take several minutes on
its first run. It is deliberately not part of host installation because it
creates repository build artifacts rather than developer-machine tooling.

## Scope

This installs the default Rust/QEMU development environment only. It does not
install `adb`, `fastboot`, Android SDK Build Tools, download Brave or Waydroid
images, touch a physical device, build a guest, or modify repository data.
Those are opt-in target workflows with their own safety and evidence rules.
