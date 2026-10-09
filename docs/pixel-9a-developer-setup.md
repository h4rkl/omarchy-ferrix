# Develop Ferrix with a Pixel 9a

This path works from a fresh clone and keeps each developer's host and device
details outside Git. QEMU is the portable development target. The checked-in
physical workflow is for the exact `google-pixel-9a-tegu-gxq96-mp1-0` board;
owning a Pixel 9a alone does not establish that its model, revision, stock
image, or recovery route matches this target.

## Start from a clone

```sh
git clone https://github.com/h4rkl/omarchy-ferrix.git
cd omarchy-ferrix
sh tools/dev/local-init.sh
sh tools/dev/check.sh
```

`local-init.sh` creates the ignored `local/` workspace and copies only missing
templates. It is safe to rerun. `check.sh` reports host prerequisites without
installing them. If tools are missing, follow `AGENT_INSTALL.md` for the
explicit, opt-in install path. Once `just` is available, `just local-init`
invokes the local workspace setup and `just host-check` invokes the host check.

Ask an agent to start with:

> Read `AGENTS.md`, `docs/status.md`, `AGENT_INSTALL.md`,
> `docs/pixel-9a-developer-setup.md`,
> `skills/mobile-device-provisioning/SKILL.md`, and
> `skills/ferrix-dev-loop/SKILL.md`. Use my ignored `local/` directory for
> host, builder, and device notes. Run the read-only host and Pixel discovery
> checks, then establish the QEMU development baseline. Treat physical
> actions as exact-target workflows with their documented recovery gates.

## Keep personal settings in `local/`

```text
local/
  README.md
  dev/
    dev-loop.env.example   # public template copied locally
    dev-loop.env           # optional private shell settings
    builder-access.md      # optional private host and checkout notes
  devices/
    pixel-9a/
      device.md            # private identity and recovery notes
  audits/                  # private reviews and scratch work
```

To use the dev-loop helper scripts, copy `local/dev/dev-loop.env.example` to
`local/dev/dev-loop.env` and fill in only the needed values. The scripts also
accept the older `out/dev-loop.env` location, so existing checkouts keep
working. Build products, captures, manifests, and target workflow transcripts
stay in their existing ignored `out/` paths. `local/` is for settings and notes;
it is not synchronized by Git. Set `FERRIX_SHELL` there if your host exposes
the Ferrix USB console at a different path.

## Establish the portable baseline

After the host prerequisites are met:

```sh
cargo test --workspace
just qemu-build
just qemu-check
```

The QEMU result is labelled `QEMU` or `mock`; it does not prove a physical
phone boot. `targets/qemu-aarch64/README.md` documents graphical capture and
input workflows. Host-nested UI work and the Pixel dev loop are in
`skills/ferrix-dev-loop/SKILL.md`.

## Identify your phone before target work

Install Android platform tools explicitly if needed, then run only the
checked-in, read-only entry points:

```sh
just pixel-9a-tools-check
just pixel-9a-discover
```

The discovery workflow refuses zero or multiple devices and records its
redacted transcript under ignored `out/pixel-9a/`. Put the full model, revision,
serial, current slot, bootloader state, and private recovery notes in
`local/devices/pixel-9a/device.md`. Compare the observed model and revision
with `targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/README.md` and its
board-support proposal. A mismatch needs its own checked-in board proposal
and owner approval before target-specific writes. Keep developing on QEMU in
the meantime.

The exact-target kernel, product, rootfs, and image path needs a native Linux
`x86_64` builder, which may be a separate machine. Its access details belong
in `local/dev/`, and its required tools are reported by
`just pixel-9a-env-check build` on that host. The target workflows retain their
existing output locations and identity, image, backup, and stock-recovery
gates. Use `skills/mobile-device-provisioning/SKILL.md` before any phone boot,
flash, unlock, restore, or radio bring-up. Do not infer physical capability
from QEMU or from another Pixel 9a revision.
