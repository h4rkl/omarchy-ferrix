# tools/pixel-9a

Target-scoped tooling for the exact board target
`google-pixel-9a-tegu-gxq96-mp1-0`. Board facts, the boot route and the
evidence live under `targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/`. The
operating playbooks are `skills/ferrix-dev-loop/SKILL.md` (build, deploy and
test) and `skills/mobile-device-provisioning/SKILL.md` (identity, flashing and
recovery). This file only describes how the directory is laid out.

Every script that can change a device prints the exact identity, slot,
partition, image hash and recovery artifact first. It also needs a per-run
typed confirmation (see `AGENTS.md`). Nothing under `tests/` contacts a
device.

## Naming convention

| Pattern | Role | Example |
| --- | --- | --- |
| `<feature>.sh` | Host entry point. It validates the arguments and the device identity, writes ignored output under `out/pixel-9a/`, and is what the `just` recipe calls. | `s5300-mount-proof.sh` |
| `<feature>-remote.sh` | Guest body. The host script sends it to the guest, usually through `acm-probe.py` over the CDC-ACM bring-up shell, piped to the guest `sh`. The host script often prepends a few validated variable assignments first. It never runs on the host. | `s5300-mount-proof-remote.sh` |
| `<feature>-lib.sh` | Shared shell functions. The host script and its test source it; it is never run directly. | `s5300-s2-binding-lib.sh` |
| `tests/<feature>.sh` | Offline test for `<feature>`. It uses stubbed `adb`, `fastboot` and guest commands with fixtures under `tests/fixtures/`. | `tests/s5300-mount-proof.sh` |
| `<feature>.py` | Host entry point written in Python (standard library only). | `static-boot.py` |

The `-remote.sh` features are `a2dp-active-proof`, `a2dp-output-preflight`,
`audio-inventory`, `guest-evidence`, `mic-capture-proof`,
`s5300-cpif-inventory`, `s5300-mount-proof`, `s5300-nv-metadata`,
`s5300-s2-preflight` and `s5300-source-inventory`.

### Exceptions in the current tree

The tree mostly follows the convention, with these known exceptions. None of
them is renamed, because callers and docs refer to them by their current
names.

- **Test suffixes.** Older tests are named `tests/<feature>-workflow.sh`
  (`boot`, `flash`, `dev-flash`, `factory-restore`, `kernel`, `rootfs`, ...).
  Tests of Python entry points are `tests/<feature>-contract.py`
  (`static-boot`, `flash-static`, `install-static`, `guest-evidence`,
  `dev-compose-android`).
- **Test names that differ from the script name.**
  - `tests/discover-read-only.sh` tests `discover.sh`.
  - `tests/unlock-workflow.sh` tests `unlock-bootloader.sh`.
  - `tests/sideload-workflow.sh` tests `sideload-ota.sh`.
  - `tests/acm-probe.sh` tests `acm-probe.py`.
- **Tests of files outside this directory.** These tests check pmOS
  device-package files, which live under the board target's
  `pmos/device-google-tegu/`:
  - `tests/aoc-firmware-runtime.sh`
  - `tests/radio-firmware-runtime.sh`
  - `tests/audio-ucm-profile.sh`
  - `tests/s5300-mounts-service.sh`
  - `tests/s5300-cpif-services.sh`

  `tests/exynos-modem-cpif-adapter.sh` tests the target-local patches to the
  pinned exynos-modem crate.
- **Shared helpers without the `-lib` suffix.** Several host scripts source
  or call these:
  - `adb-correlation.sh`, `data-safety.sh` and `temp-boot-observation.sh`
    are sourced by the flash, recovery and partition workflows.
  - `acm-probe.py` is the CDC-ACM probe transport.
  - `mic-acoustic-tone.py` is used by `mic-capture-proof`.
  - `pmbootstrap-rootfs.py` is used by `rootfs.sh`.
  - `reboot-bootloader.rs` is compiled by `dev-compose.py`.
- **Test helpers.** `tests/acm-fake-guest.sh` (sourced) and
  `tests/acm-fake-guest.py` (spawned) are the fake CDC-ACM guest for the
  ACM-based tests. They are not tests, and the offline runner skips them.
- **Scripts with no test of their own**: `dt-inspect.sh`,
  `dt-symbol-probe.sh`, `recovery-download.sh`, `postprobe-diagnose.py` and
  `s5300-s2-binding.sh`. The binding checker is covered through
  `tests/s5300-s2-preflight.sh`.
- **Subdirectories.**
  - `devtest/` holds the UI test helpers for the dev image; see
    `devtest/README.md`.
  - `factory-tool/` is a Rust crate.

## How `just` recipes map to scripts

Recipes are named `pixel-9a-<feature>[-<verb>]`. They call
`tools/pixel-9a/<feature>.sh` (or `.py`), passing the verb as the script's
mode or argument:

- `pixel-9a-kernel-check`, `-sync`, `-verify` and `-build` call
  `kernel.sh check|sync|verify|build`.
- `pixel-9a-a2dp-active-proof-dry` and `-play` call `a2dp-active-proof.sh`
  in its two modes.

`pixel-9a-<feature>-test` runs `tests/<feature>*.sh` (or `*.py`). Some test
recipes group several related tests:

- `pixel-9a-tools-test` runs the identity, recovery, unlock and flash safety
  tests.
- `pixel-9a-env-test` runs `env.sh` and `attempt-env.sh`.
- `pixel-9a-firmware-test` and `pixel-9a-radios-test` also run the matching
  `*-firmware-runtime.sh` tests.

`just --list` shows the recipes in groups. The Pixel 9a groups are:

- `pixel-9a-build`
- `pixel-9a-flash-recovery`
- `pixel-9a-probes`
- `pixel-9a-audio`
- `pixel-9a-s5300`
- `tests`, which holds every device-free test recipe.

## Running all offline tests

```sh
just pixel-9a-test-offline                 # every test, sorted
just pixel-9a-test-offline acm-probe.sh    # a subset, by file name
```

This calls `tools/pixel-9a/test-offline.sh`, which works as follows:

- It discovers `tests/*.sh` (run with `sh`) and `tests/*.py` (run with
  `python3`) in `LC_ALL=C` sorted order, and leaves out the two
  `acm-fake-guest` helpers.
- It runs every test even after one fails.
- It prints the tail of each failing log and a summary at the end.
- It exits non-zero if any test failed, apart from Linux-quarantined tests
  (see below).

Tests mark a stage they cannot run on this host with a line that starts with
`skip ` and still exit 0. Two examples:

- `exynos-modem-cpif-adapter.sh` skips its stage when the pinned archive is
  not cached; set `FERRIX_S5300_ADAPTER_FETCH=1` to fetch it.
- `s5300-cpif-services.sh` skips when busybox `awk` is absent.

The Python entry points import `tomllib`, so `python3` must be 3.11 or
newer. On macOS, `/usr/bin/python3` is 3.9, and with it
`flash-static-contract.py`, `guest-evidence-contract.py`,
`install-static-contract.py`, `static-boot-contract.py` and
`radio-split-workflow.sh` fail with `ModuleNotFoundError`. Put a newer
`python3` first on `PATH`.

The runner reports these tests as passed with skips and repeats the skip
lines. Logs go to a fresh temporary directory, or to
`FERRIX_PIXEL9A_TEST_LOG_DIR` if it is set. Some tests take several minutes.
`a2dp-active-proof.sh` is the slowest, at about six minutes.

### Linux quarantine (#127)

`tests/quarantine-linux.txt` lists tests that fail on Linux CI hosts
(ubuntu-latest) but pass on macOS. The list holds one file name per line, and
`#` starts a comment. Root causes are tracked in #127.

The runner reads the list only when `uname -s` is `Linux`:

- Listed tests still run.
- A listed test that fails is reported as `QUARANTINED (#127)`, with its log
  tail, and does not fail the run.
- A listed test that passes is reported as `quarantined but passed; remove
  from list`.
- A test that is not listed still fails the run when it fails.

On macOS and other hosts the list is ignored, and every test counts. Remove an
entry in the PR that fixes the test. Never quarantine a test by editing its
assertions.

To add a test, drop `tests/<feature>.sh` (or `.py`) in place. The runner and
CI pick it up automatically. Also add a `pixel-9a-<feature>-test` recipe so
the test can be run on its own.
