# Pixel 9a development profile

`profile.env` is the canonical host-side profile for the exact Google Pixel
9a `GXQ96` / `tegu` / `MP1.0` target. Load it through:

```sh
. tools/pixel-9a/env.sh
```

The loader exports the physical target identity, the exact evidence label,
native Linux production backend selection, and target-scoped cache/output
paths. It does not create directories or contact a device. Run its validation
without loading it into the current shell with:

```sh
tools/pixel-9a/env.sh validate
tools/pixel-9a/env.sh print
tools/pixel-9a/env.sh check build
```

`check` requires a native Linux `x86_64` host and checks the selected workflow's
tools. Product, Google Kleaf kernel, firmware, rootfs, boot, and artifact
inputs all run on that host. A pinned `linux/amd64` Docker tool container is
allowed only when its Docker engine is running on native Linux `x86_64`; it is
not a substitute for an Apple-Silicon/ARM64 host, an emulated VM, or a
cross-build, and it must never receive ADB/Fastboot USB access.
The product profile targets `aarch64-unknown-linux-musl`, so a native build
host must explicitly provision that Rust standard library and the configured
`aarch64-linux-musl-gcc` linker (or set an equivalent
`FERRIX_PIXEL9A_RUST_LINKER`). Repository workflows do not install either
tool.
The profile groups build, recovery, inspection, and firmware tools so a
developer can keep those concerns in separate environments. The Pixel
kernel/rootfs/image workflows remain independently gated and opt-in.

For the reproducible native-Linux build path, source the loader before invoking
the `just` recipes so every child process receives the same resolved interface:

```sh
. tools/pixel-9a/env.sh
just pixel-9a-env-check build
```

The loader does not create any directory. It resolves the profile's relative
paths to absolute paths. The workflows also accept the stage-specific
boot-input and complete split-image overrides listed below:

| Stage | Canonical output/cache interface |
| --- | --- |
| Product | `FERRIX_PIXEL9A_PRODUCT_OUT` (including its built-in `manifest.tsv`) and `FERRIX_PIXEL9A_CARGO_TARGET_DIR` from direct Cargo builds on the native Linux host |
| Kernel | `FERRIX_PIXEL9A_KERNEL_SOURCE`, `FERRIX_PIXEL9A_KERNEL_BAZEL_CACHE`, `FERRIX_PIXEL9A_KERNEL_DIST`, `FERRIX_PIXEL9A_KERNEL_MANIFEST` |
| Radio kernel | `FERRIX_PIXEL9A_RADIO_KERNEL_CACHE_DIR`, `FERRIX_PIXEL9A_RADIO_KERNEL_OUT`, `FERRIX_PIXEL9A_RADIO_KERNEL_DIST`, `FERRIX_PIXEL9A_RADIO_KERNEL_MANIFEST` |
| Rootfs | `FERRIX_PIXEL9A_PRODUCT_DIR`, `FERRIX_PIXEL9A_RADIO_KERNEL_DIST`, `FERRIX_PIXEL9A_RADIO_KERNEL_MANIFEST`, `FERRIX_PIXEL9A_FIRMWARE_DIR`, `FERRIX_PIXEL9A_ROOTFS_OUT`, `FERRIX_PIXEL9A_ROOTFS_MANIFEST` |
| Split boot | `FERRIX_PIXEL9A_BOOT_OUT`, `FERRIX_PIXEL9A_BOOT_MANIFEST` plus the kernel/rootfs interfaces |
| Radio-first diagnostic boot | `FERRIX_PIXEL9A_RADIO_BOOT_OUT`, `FERRIX_PIXEL9A_RADIO_BOOT_MANIFEST` plus the radio kernel/rootfs interfaces |
| Source-owned boot inputs | `FERRIX_PIXEL9A_BOOT_INPUTS_OUT`, `FERRIX_PIXEL9A_BOOT_INPUTS_MANIFEST` plus the radio kernel/rootfs interfaces |
| Complete radio-first split images | `FERRIX_PIXEL9A_RADIO_SPLIT_OUT`, `FERRIX_PIXEL9A_RADIO_SPLIT_MANIFEST`, `FERRIX_PIXEL9A_RADIO_PARTITION_SIZES` plus the boot-input interfaces |
| Artifact manifest | `FERRIX_PIXEL9A_IMAGE_OUT`, `FERRIX_PIXEL9A_BOOT_OUT`, `FERRIX_PIXEL9A_BOOT_MANIFEST` plus the kernel/rootfs manifests |

The firmware interface intentionally points at
`out/pixel-9a/firmware/tegu/staged`, whose `firmware-manifest.tsv` is produced
by the reviewed firmware staging workflow. It is still an external,
hash-verified input; the loader does not extract or copy firmware.

The resolved roots are deliberately separate:

```text
.cache/ferrix/pixel-9a/                 target cache root
.cache/ferrix/pixel-9a/product-target/ Cargo target directory
.cache/ferrix/pixel-9a/kernel/          kernel checkout and Bazel cache
.cache/ferrix/pixel-9a/kernel-radio/    radio-first kernel checkout/archive cache
out/pixel-9a/product/tegu/              product handoff
out/pixel-9a/kernel/tegu/               Google comparison kernel dist/manifest
out/pixel-9a/kernel-radio/tegu-wwan/    selected radio-first kernel dist/manifest
out/pixel-9a/rootfs/tegu/               rootfs handoff and manifest
out/pixel-9a/firmware/tegu/             firmware inventory/staged input
out/pixel-9a/partition-capacities-latest.tsv  exact-target read-only capacity handoff
out/pixel-9a/images/tegu/               Google comparison split-boot outputs
out/pixel-9a/images/tegu-wwan/          selected radio-first boot handoff
out/pixel-9a/boot-inputs/tegu-wwan/     source-owned ramdisks and module roots
out/pixel-9a/images/tegu-wwan-split/    complete radio-first split handoff
```

`FERRIX_CACHE_DIR=.cache/ferrix` and `FERRIX_OUT_DIR=out` remain compatibility
parents for generic discovery/recovery tooling and QEMU; do not point either
variable at `out/pixel-9a`, because those older interfaces append their own
target namespace. Use the explicit `FERRIX_PIXEL9A_*` variable for a one-off
stage override instead. The QEMU profile and `out/qemu-aarch64/` namespace are
unchanged when this environment is sourced.

`just pixel-9a-env-validate` checks profile identity, backend selection, safe
relative paths, and cache/output separation without checking the host. The
`build` check additionally requires native Linux `x86_64`, the complete
host-tool set, the installed Rust standard library for the selected target, the
configured target linker, and the target-specific inputs. Product, kernel, and
rootfs workflows repeat their own native-Linux gates; boot and artifact
validation remain device-free and consume only hash-verified manifests.

## Native builder handoff contract

The physical route needs Linux `x86_64` running on x86_64 hardware. An
Apple-Silicon OrbStack VM/container, Apple-Silicon UTM/QEMU CPU emulation,
Rosetta, or an x86 cross-build does not satisfy this contract, even when
`uname` inside it reports a Linux userland. A bare-metal host or a
hardware-virtualized Linux guest on x86_64 hardware is acceptable; keep the
repository checkout and all ignored `.cache/ferrix/pixel-9a` and
`out/pixel-9a` paths on that host. The host must provide:

- the pinned Rust channel from `rust-toolchain.toml`, the
  `aarch64-unknown-linux-musl` standard library, and an actual
  `aarch64-linux-musl-gcc`-compatible linker;
- GNU build tools, LLVM/Clang `ld.lld`, Kbuild prerequisites, `repo` and Git
  for the optional Google Kleaf comparison route, and `pmbootstrap` plus
  `mke2fs` for the selected radio-first rootfs route; and
- the checked-in `mkbootimg`, `mkdtboimg.py`, and `avbtool` interfaces, with
  `cpio`, `tar`, hashing, and Python support. `env.sh check build` is the
  authoritative command/tool gate; it never installs any of these tools.

The native builder image also installs Debian's maintained
`android-sdk-libsparse-utils` host package at the pinned Bookworm version
`1:29.0.6-28` (the package archive SHA-256 is recorded in the Dockerfile).
This supplies `simg2img`, which is required when the hash-verified factory
`vendor.img` is Android-sparse; `e2fsprogs` alone supplies `debugfs` but not
that converter. The utility is used only on the host during offline firmware
extraction and is never copied into the Rust OS guest or used as Android
userspace.

The selected radio-first build requires the exact source archive and clean
detached checkout in `kernel-radio-route.toml`, the clean target-local
pmaports checkout at `pmos/pins.env`, and the separately staged, hash-verified
factory firmware manifest. Product and rootfs builds are opt-in and refuse
missing or dirty inputs. The physical device contributes the redacted
ten-partition capacity handoff. Vendor boot inputs come from the source-owned
boot-input generator; the product, kernel, rootfs, and boot-input builders do
not contact the device.

Each handoff is independently verifiable: the product manifest records the
pinned Rust channel plus `Cargo.lock` and `rust-toolchain.toml` hashes; the
rootfs manifest records the pmaports commit and product-manifest hash; the
radio-kernel and boot manifests record source/artifact hashes. Transfer the
ignored outputs and their manifests out-of-band to the transport host, then
re-run the target-scoped validators there. Firmware, factory archives, AVB
keys, device identifiers, and generated images remain outside Git.

For development sessions, ordinary bootloader Fastboot does not require a
hard-button entry when Android ADB is available:

```sh
just pixel-9a-enter-fastboot --preflight
just pixel-9a-enter-fastboot --confirm-enter
```

This target-scoped workflow performs only an exact-identity-checked,
confirmation-gated `adb reboot bootloader` and verifies the resulting
bootloader Fastboot transport. It never enters Fastbootd; the partition
capacity workflow remains read-only and requires an owner-entered Fastbootd
session when Android cannot expose inactive logical mappings.

## Radio-first split-artifact completion

First, with exactly one authorized Pixel 9a visible over ADB, collect the
capacity handoff without changing the device:

```sh
just pixel-9a-partition-capacity
```

This reads both A/B static by-name block sizes and independently resolves the
active/inactive dynamic `vendor_dlkm` and `system_dlkm` mapper nodes. It writes
`out/pixel-9a/partition-capacities-latest.tsv` plus a redacted transcript. A
`partial` broader Android inventory does not invalidate these ten capacities,
but the handoff never authorizes flashing and must still be reviewed against
the exact target transcript.

If Android hides the inactive dynamic mapper nodes, run the same command only
after an owner-controlled transition to Fastbootd, with exactly one Fastboot
device visible. That branch reads independent A/B `partition-size:*` getvars,
requires `is-userspace=yes`, and refuses the ordinary bootloader; it does not
reboot, change slots, or write a partition.

The radio-first Kbuild handoff intentionally stops at source-built `Image`,
DTB/DTBO inputs, and `modules_install` output. On the native builder, first
build the source-owned ramdisks and module roots from that handoff and the
completed rootfs:

```sh
just pixel-9a-boot-inputs-check
FERRIX_PIXEL9A_BOOT_INPUTS_BUILD=I-understand-this-builds-source-owned-pixel-9a-boot-inputs \
just pixel-9a-boot-inputs-build
```

Then, after obtaining and reviewing the exact target's redacted capacity
handoff, compose the complete image set:

```sh
FERRIX_PIXEL9A_RADIO_PARTITION_SIZES=/path/to/partition-capacities.tsv \
FERRIX_PIXEL9A_AVB_ALGORITHM=NONE \
just pixel-9a-radio-split-check
FERRIX_PIXEL9A_RADIO_SPLIT_BUILD=I-understand-this-composes-the-radio-first-split-artifacts \
FERRIX_PIXEL9A_RADIO_PARTITION_SIZES=/path/to/partition-capacities.tsv \
FERRIX_PIXEL9A_AVB_ALGORITHM=NONE \
just pixel-9a-radio-split-build
```

The composer verifies the boot-input manifest; legacy ad-hoc ramdisk/module
environment variables are rejected. It requires source-built DTBOs, modules,
initramfs/vendor ramdisk, bootconfig, and the radio-first kernel/rootfs
manifests. It creates `dtbo`,
`vendor_kernel_boot`, `vendor_dlkm`, and `system_dlkm`, then invokes native
`avbtool add_hash_footer` on each final source-built partition and
`make_vbmeta_image --include_descriptors_from_image` for all three fresh
vbmeta images. This ordering is the binding gate: salted hash descriptors are
generated from the exact bytes in the staged output, and no stock or prebuilt
Android vbmeta image is accepted. Set `FERRIX_PIXEL9A_AVB_ALGORITHM` to a
supported signing algorithm and provide `FERRIX_PIXEL9A_AVB_KEY` when signing;
`NONE` is allowed only as the explicit unlocked-device development mode. All
inputs use the pinned 4 KiB/header-v4 contract. The output remains
`composed-not-booted` / `bootable=false`. The checked-in
`tools/pixel-9a/flash-radio.sh` route now consumes this exact six-field
radio-first manifest through its own identity, recovery, backup, inactive-slot,
AVB, and confirmation gates; it is not accepted by the canonical
`pixel-9a-flash-*` route. A complete, redacted capacity handoff is still
required, and no physical boot or flash has been run on the target.

Both vendor images use `mkbootimg --vendor_boot` with header v4 and 4096-byte
pages. The early-module CPIO is a payload inside `vendor_kernel_boot.img`.
The container choice follows the upstream [mkbootimg interface](https://android.googlesource.com/platform/system/tools/mkbootimg/+/refs/heads/main/mkbootimg.py)
and [vendor kernel boot image rule](https://android.googlesource.com/kernel/build/+/refs/heads/main/kleaf/docs/api_reference/kernel.md#vendor_boot_image).
DLKM filesystems use an explicit 4096-byte block size and reserve the space
reported by `avbtool add_hash_footer --calc_max_image_size`. The composer
appends the vbmeta blob and footer before including its descriptors in the
three vbmeta images and verifies each image's payload digests before
publishing the output; `--do_not_append_vbmeta_image` would discard that
embedded metadata, as defined by [avbtool](https://android.googlesource.com/platform/external/avb/+/main/avbtool.py).

## Radio-first physical handoff

After the split composer has produced `out/pixel-9a/images/tegu-wwan-split/`
and the exact target is in bootloader Fastboot, inspect the route without a
state change:

```sh
just pixel-9a-flash-radio-check --stage kernel --slot b --allow-unsigned-avb
```

The route prints the exact product/variant/hardware revision, serial hash,
current and inactive target slot, every partition image hash, recovery hash,
and the fastbootd transition plan. A temporary boot must be issued first:

```sh
just pixel-9a-flash-radio-boot-temp --allow-unsigned-avb
```

The command transcript proves only that the boot request was issued. Retain a
redacted text capture of kernel output showing the Linux version and tegu
machine record. After reviewing the capture and observing progress without a
hang, panic, or reboot loop, return the device to ordinary Fastboot through
the approved recovery/transport procedure and record that observation:

```sh
FERRIX_PIXEL9A_RADIO_TEMP_BOOT_EVIDENCE=out/pixel-9a/flash-radio/boot-temp-<stamp>-<pid>.txt \
FERRIX_PIXEL9A_RADIO_TEMP_BOOT_KERNEL_LOG=/path/to/reviewed-redacted-kernel.txt \
just pixel-9a-observe-temp-boot --allow-unsigned-avb
```

This workflow reads identity and writes local evidence. Its typed confirmation
attests to the observation and capture redaction; it performs no device
mutation. It binds the capture hash to the exact boot image, command transcript,
and device. It rejects recorded kernel failures, but human observation is
still required to rule out a silent hang or loop. Failure to obtain this
capture leaves persistent writes gated; do not fabricate an observation.

Only after this observation passes may the owner authorize the inactive-slot
write:

```sh
FERRIX_PIXEL9A_RADIO_TEMP_BOOT_EVIDENCE=out/pixel-9a/flash-radio/boot-temp-<stamp>-<pid>.txt \
FERRIX_PIXEL9A_RADIO_TEMP_BOOT_OBSERVATION=out/pixel-9a/flash-radio/observe-temp-<stamp>-<pid>.txt \
just pixel-9a-flash-radio --stage kernel --slot b --allow-unsigned-avb
```

The route never unlocks/locks, erases, formats, wipes, changes the active
slot, flashes bootloader/radio firmware, or exposes the phone to the build
container. It remains stub-tested only until this exact Pixel 9a has a
successful observed Linux boot and radio evidence.

The pinned Rust channel and target declarations are read from
`rust-toolchain.toml`; the default developer install provisions both the QEMU
GNU target and the physical-product musl standard library. If a native Linux
Pixel build host was provisioned before the musl target was declared, add that
target explicitly using the pinned channel (without changing the repository
toolchain) before the check:

```sh
rustup target add --toolchain 1.98.1 aarch64-unknown-linux-musl
command -v aarch64-linux-musl-gcc
just pixel-9a-env-check build
```

This profile is intentionally distinct from both:

- `targets/qemu-aarch64/profile.toml`, which describes synthetic QEMU/mock
  state and must never provide physical-device evidence; and
- `pmos/`, which contains files installed into the Pixel guest and is not a
  host-tool environment.

Existing workflows may consume the exported values when invoked from a shell
that has sourced `tools/pixel-9a/env.sh`. This slice does not rewrite those
workflows, QEMU configuration, or the pmOS package files.
