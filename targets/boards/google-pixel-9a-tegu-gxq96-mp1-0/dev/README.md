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

The loader does not create any directory. It resolves the checked-in relative
paths to absolute paths and exports these stage boundaries:

| Stage | Canonical output/cache interface |
| --- | --- |
| Product | `FERRIX_PIXEL9A_PRODUCT_OUT` (including its built-in `manifest.tsv`) and `FERRIX_PIXEL9A_CARGO_TARGET_DIR` from direct Cargo builds on the native Linux host |
| Kernel | `FERRIX_PIXEL9A_KERNEL_SOURCE`, `FERRIX_PIXEL9A_KERNEL_BAZEL_CACHE`, `FERRIX_PIXEL9A_KERNEL_DIST`, `FERRIX_PIXEL9A_KERNEL_MANIFEST` |
| Radio kernel | `FERRIX_PIXEL9A_RADIO_KERNEL_CACHE_DIR`, `FERRIX_PIXEL9A_RADIO_KERNEL_OUT`, `FERRIX_PIXEL9A_RADIO_KERNEL_DIST`, `FERRIX_PIXEL9A_RADIO_KERNEL_MANIFEST` |
| Rootfs | `FERRIX_PIXEL9A_PRODUCT_DIR`, `FERRIX_PIXEL9A_RADIO_KERNEL_DIST`, `FERRIX_PIXEL9A_RADIO_KERNEL_MANIFEST`, `FERRIX_PIXEL9A_FIRMWARE_DIR`, `FERRIX_PIXEL9A_ROOTFS_OUT`, `FERRIX_PIXEL9A_ROOTFS_MANIFEST` |
| Split boot | `FERRIX_PIXEL9A_BOOT_OUT`, `FERRIX_PIXEL9A_BOOT_MANIFEST` plus the kernel/rootfs interfaces |
| Radio-first split boot | `FERRIX_PIXEL9A_RADIO_BOOT_OUT`, `FERRIX_PIXEL9A_RADIO_BOOT_MANIFEST` plus the radio kernel/rootfs interfaces |
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

The selected radio-first build requires the exact source archive and clean
detached checkout in `kernel-radio-route.toml`, the clean target-local
pmaports checkout at `pmos/pins.env`, and the separately staged, hash-verified
factory firmware manifest. Product and rootfs builds are opt-in and refuse
missing or dirty inputs. The physical device contributes only the redacted
ten-partition capacity handoff and any reviewed source-built vendor boot
inputs; it is not contacted by the product, kernel, or rootfs builders.

Each handoff is independently verifiable: the product manifest records the
pinned Rust channel plus `Cargo.lock` and `rust-toolchain.toml` hashes; the
rootfs manifest records the pmaports commit and product-manifest hash; the
radio-kernel and boot manifests record source/artifact hashes. Transfer the
ignored outputs and their manifests out-of-band to the transport host, then
re-run the target-scoped validators there. Firmware, factory archives, AVB
keys, device identifiers, and generated images remain outside Git.

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
DTB/DTBO inputs, and `modules_install` output. Once a complete, redacted
partition-capacity handoff has been obtained from the exact target, the
device-free split composer can turn those inputs into the remaining source
artifacts:

```sh
FERRIX_PIXEL9A_RADIO_PARTITION_SIZES=/path/to/partition-capacities.tsv \
FERRIX_PIXEL9A_VENDOR_KERNEL_BOOT_CPIO=/path/to/vendor-kernel-boot.cpio \
FERRIX_PIXEL9A_VENDOR_DLKM_ROOT=/path/to/source-built/vendor-dlkm \
FERRIX_PIXEL9A_SYSTEM_DLKM_ROOT=/path/to/source-built/system-dlkm \
FERRIX_PIXEL9A_AVB_ALGORITHM=NONE \
just pixel-9a-radio-split-check
FERRIX_PIXEL9A_RADIO_SPLIT_BUILD=I-understand-this-composes-the-radio-first-split-artifacts \
FERRIX_PIXEL9A_RADIO_PARTITION_SIZES=/path/to/partition-capacities.tsv \
FERRIX_PIXEL9A_VENDOR_KERNEL_BOOT_CPIO=/path/to/vendor-kernel-boot.cpio \
FERRIX_PIXEL9A_VENDOR_DLKM_ROOT=/path/to/source-built/vendor-dlkm \
FERRIX_PIXEL9A_SYSTEM_DLKM_ROOT=/path/to/source-built/system-dlkm \
FERRIX_PIXEL9A_AVB_ALGORITHM=NONE \
just pixel-9a-radio-split-build
```

The composer requires source-built DTBOs, modules, initramfs/vendor ramdisk,
bootconfig, and the radio-first kernel/rootfs manifests. It creates `dtbo`,
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

Only after observing and retaining its redacted transcript may the owner
authorize the inactive-slot write:

```sh
FERRIX_PIXEL9A_RADIO_TEMP_BOOT_EVIDENCE=out/pixel-9a/flash-radio/boot-temp-<stamp>-<pid>.txt \
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
