# Status

## 2026-10-01 (Pixel 9a vendor boot layout correction; no new device load)

- A further offline check found that the failed candidate's `dtbo.img` has
  266 entries from generic ARM board builds; its first entry has ID/revision
  zero. The verified stock image has 10 entries. Its exact tegu MP overlay
  has board ID `0x60b06` and revision `0x10000`, consistent with Google's
  [pinned `zumapro-tegu-mp.dts`](https://android.googlesource.com/kernel/devices/google/tegu/+/6276884d9940694b25a6934671918ec8ac412ee8/dts/zumapro-tegu-mp.dts).
  The selected experimental kernel source currently has no tegu `.dtbo`.
  Applying the stock MP overlay to its candidate DTB with `fdtoverlay`
  failed with `FDT_ERR_BADOFFSET`. The DTB also lacks `/__symbols__`, which
  can prevent overlay fixups. This is an offline compatibility diagnostic,
  not a physical boot result. Both composers now
  require exactly one source-built tegu MP overlay and the observed ID/revision
  in the final DTBO table. They refuse to compose a new bundle until that
  source and base-tree compatibility are supplied. The kernel build now
  refuses the current source before starting Kbuild because the MP overlay
  source and its Kbuild selection are absent. The native builder wrapper
  produced that exact refusal at commit `402d14f` without creating output.
  Future kernel manifests will inventory only the exact MP overlay instead
  of copying every unrelated ARM board overlay, and the kernel workflow will
  require the compiled overlay to apply to its source-built base DTB before
  emitting a manifest. This offline check cannot establish hardware boot.
- The verified stock comparison showed a version-4 boot image with fixed
  4 KiB pages, but both vendor boot containers use 2 KiB pages. Stock
  `vendor_boot` has no embedded DTB; `vendor_kernel_boot` carries the DTBs.
  The selected static and split source contracts and composers now encode
  that placement and page size. The static transfer verifier reads the
  version-4 vendor headers to reject a wrong page size, DTB placement, or
  command line.
- The existing checked-in `kernel-cmdline.conf` now supplies early console
  and `bootconfig`; the source-owned boot-input generator hashes a rendered
  `vendor-cmdline`, and both composers pass it to `mkbootimg`. The target
  package checksum and route-pinned contract hashes were updated. The
  previously built candidate is still blocked and cannot be reused under
  the revised contracts. No phone command or new physical loading occurred
  for this correction. Hardware compatibility remains unproved.
- A new clean native attempt `20260930T190221Z` began at source commit
  `ec8c4015ad162da7e7916b6ca95739042a74159b` with the same named
  capacity handoff. Source verification passed, but the kernel build was
  stopped with exit 143 after the missing exact-target overlay was found;
  it produced no image candidate. Its ignored kernel log remains on deba.
  Product, rootfs, boot-input, and image stages were not started for this
  attempt. The later exact-MP DTBO gate is a new source contract and needs a
  separate source-bound attempt after the overlay/base-tree work is complete.

## 2026-10-01 (Pixel 9a temporary probe not observed)

- The fresh `20260930T142411Z` candidate passed live read-only admission in
  ordinary bootloader Fastboot on the exact unlocked `tegu` / `MP1.0`, slot A.
  The checked-in temporary workflow then sent its pinned kernel-only
  `boot.img` (SHA-256
  `f310cae8e6fd2c9d7992d68b3020901c6bb4c59fa5fa1fbf3f0c32d2064c8bfe`)
  with `fastboot boot`; the bootloader reported `OKAY`. The redacted boot
  transcript SHA-256 is `febe0c41f55d3e9da6f8939c56e576be929d05dc22dc6c89f10680c166a08038`.
  No partition, slot, or bootloader state was changed.
- Neither ADB nor Fastboot was visible shortly after the command. The same
  Pixel returned as authorized stock Android ADB on slot A about two minutes
  later. The target-scoped capture workflow refused because live
  `/proc/version` differs from the kernel banner embedded in the pinned
  `boot.img`; Android also denied `/proc/device-tree/model`. A hash-bound,
  serial-redacted mismatch diagnostic was saved under ignored `out/pixel-9a/`
  (SHA-256 `58f6427a3043434a9046278536c04a8a07b93d5a844724ba66fdc32f686a0c75`).
  Its expected and observed banner hashes differ. The updated capture workflow
  records this negative result while refusing full observation; offline tests
  also verify that a version-only capture cannot satisfy the gate.
- The checked-in candidate is now `probe-unobserved`, so further physical
  loading of those bytes is refused. No Ferrix kernel progress, Ferrix
  userspace, or radio capability is claimed. No persistent image was flashed.
  The one-time rootfs password file was deleted from the builder. A new
  source-bound build and compatibility investigation are required before
  another loading attempt; `image_compatibility=partial` remains unchanged.
- A checked-in, read-only exact-target follow-up queried only boot-reason
  properties and console pstore availability. After the phone returned to
  stock Android, `ro.boot.bootreason=watchdog,apc,early` and
  `sys.boot.reason=watchdog`; console pstore listing was unavailable to ADB.
  The ignored, serial-redacted transcript has SHA-256
  `8d2c8c793773c7696bb738308966af3127fce036568de43024ecd322f987a835`.
  [AOSP's boot-reason specification](https://source.android.com/docs/core/architecture/bootloader/boot-reason)
  explains the bootloader and system properties. The watchdog reason is
  consistent with the return after this probe, but does not identify the
  failed component or prove that the candidate kernel reached Linux.
- Offline review found a limitation in the temporary probe: the checked-in
  command supplies only `boot.img`, while the phone continues to use stock
  `init_boot`, `vendor_boot`, `vendor_kernel_boot`, and `dtbo`. AOSP lists
  `vendor_kernel_boot` and `dtbo` among Pixel 9a kernel partitions and places
  the device tree in a vendor boot container for header version 4
  ([Pixel kernel build guide](https://source.android.com/docs/setup/build/building-pixel-kernels),
  [boot image header](https://source.android.com/docs/core/architecture/bootloader/boot-image-header)).
  Thus the watchdog cannot validate or reject the complete eight-image bundle.
  The existing install gate still requires observed same-candidate kernel
  progress and full image compatibility; it cannot be bypassed by this clue.
- An offline comparison unpacked the verified pinned stock factory archive
  (SHA-256 `0c2f9053bf064b09483c937cf16f4824371afe92fa057029d6f9237668d1c0d7`)
  and the failed candidate using AOSP `unpack_bootimg.py` at commit
  `d2bb0af5ba6d3198a3e99529c97eda1be0b5a093`. Stock
  `vendor_kernel_boot` uses 2 KiB pages and carries four DTBs totaling
  1,547,478 bytes; the candidate uses 4 KiB pages and one 68,753-byte DTB.
  Stock `vendor_boot` likewise uses 2 KiB pages, has no DTB, and supplies a
  vendor command line; the candidate uses 4 KiB pages, embeds that 68,753-byte
  DTB, and has an empty vendor command line. These differences leave full-image
  compatibility unproven. The boot-only temporary probe used the stock vendor
  images, so it did not exercise those candidate differences.

## 2026-10-01 (fresh Pixel 9a static bundle and Fastbootd return)

- Attempt `20260930T142411Z` used clean native-builder source commit
  `c4f0abf18b41a5906e0b9c421cb7afc130876e3f` and the reviewed named
  static capacity handoff (SHA-256
  `9e7c7406b77b03c9cc0dbcc66d7f4682b9149a041a355c468a99454687b2d66d`).
  Fresh radio kernel, product, rootfs, and boot inputs passed; their manifest
  hashes are respectively `5f28d65a73e94a87b30c8a45e0e7d613b43765aec531814b48ca2915b5c4364f`,
  `eba8e1b7c829af9c3c1457bf865793ed06b345af2fdf97f37dc65e517d19908e`,
  `e893edf54e115c9a328dbacf22d3a2573d170fe3a5dfd154fd9587c19cc1fdd2`,
  and `f5335584a82466f9ac6f6763b1b61371aff60a23a1f12eb93a1e98a1f430e4a1`.
- The first rootfs invocation refused a missing pmaports environment path
  before building an image. A retry supplied the clean pinned checkout and
  passed. Its 2 GiB ext4 image passed `e2fsck -fn`; the one-time password did
  not appear in the build log and no temporary native-builder env file remained.
  The first boot-input invocation likewise refused the generated relative
  output path before writing files; a retry supplied the absolute path under
  `out/pixel-9a` and passed. The attempt generator now supplies both values
  automatically, with a passing regression test. Both retries used the same
  unchanged source and fresh attempt outputs.
- Native static composition and AVB payload verification passed. The 30-file
  bundle `out/pixel-9a/images/tegu-wwan-static-20260930T142411Z` has manifest
  SHA-256 `78c7d21d01d12cc464c76be98bef60afccd38b4d8cd57e0b6c77ff0e670512ed`,
  `boot.img` SHA-256 `f310cae8e6fd2c9d7992d68b3020901c6bb4c59fa5fa1fbf3f0c32d2064c8bfe`,
  and `rootfs.img` SHA-256 `1e265155a3723fa61f0caf865d7c952ba3baf0f7f0e7d38abe0b925f93b8ee36`.
  The complete bundle was transferred to the Mac, where the exact-path
  transfer verifier passed. It remains `composed-not-booted` / `bootable=false`.
- The checked-in Fastbootd-to-bootloader workflow passed offline tests and a
  live read-only preflight. With the owner's unattended-work authorization,
  it returned the one exact Pixel 9a to ordinary bootloader Fastboot and
  verified the same serial and slot A afterward. No storage, slot, or user
  data changed. Persistent install still refuses `image_compatibility=partial`;
  no Ferrix boot or physical radio behavior has been observed.

## 2026-10-01 (fresh Pixel 9a attempt: rootfs wrapper failure)

- Attempt `20260930T111220Z` on the native x86_64 builder was bound to clean
  source commit `c17c0b7a0c28178598f94cfe80a576142f2b091a` and an exact
  static capacity handoff. Device-free source, product, firmware, and rootfs
  preflights passed. The fresh radio kernel and product stages passed; their
  manifest SHA-256 values are `1c91764d6c6649725566caac584cc8262a94182fa14250fb1aed6149c70e77cb`
  and `eba8e1b7c829af9c3c1457bf865793ed06b345af2fdf97f37dc65e517d19908e`.
- The privileged rootfs command produced a 2 GiB ext4 image and a manifest
  with SHA-256 `6c784bc463fcfa1ced11e6c0c94a0236c20e486e7fb80b3dd00e461f97940b10`;
  `e2fsck -fn` passed. The native wrapper then launched the command a second
  time without privileges and exited 2 (`reason=no-loop-control`). The wrapper
  now returns immediately after the privileged Docker run. Its test verifies
  exactly one dispatch, preserved failure status, and temporary env-file
  cleanup. The three rootfs AWK expressions that emitted quote-escape
  warnings were also corrected.
- This attempt has no composed boot images and is **not an install candidate**.
  Its one-time rootfs password file was deleted. No phone mutation occurred;
  the existing `image_compatibility=partial` gate still refuses persistent
  installation.

## 2026-09-30 (Pixel 9a build, radios, and setup audit)

- Later in the attached-device session, `just device-discover` found one
  `tegu` / `MP1.0` device in userspace Fastboot, unlocked on slot A. The
  generic matcher initially treated Fastbootd's fixed `MSM USF` variant as a
  conflicting SKU. It now requires `is-userspace=yes` and complete same-serial
  Android evidence of `GXQ96`; `just device-match` then returned the exact
  checked-in target. Bootloader-mode or uncorrelated placeholders still fail.
  `just pixel-9a-discover` also passed. These are read-only transport and
  identity observations, not Ferrix boot evidence. The read-only partition
  inventory now applies the same narrowly scoped placeholder rule and passed
  on the live Fastbootd transport with `inventory_completeness=partial`.
- `just pixel-9a-partition-capacity` refused in Fastbootd because
  `partition-size:vendor_boot_a` was unavailable. The checked-in diagnostic
  found 12 unavailable static-partition size queries and live zero sizes for
  inactive logical `vendor_dlkm_b` and `system_dlkm_b` (active A sizes were
  positive). Both logical A/B classifications were reported, and the snapshot
  status was `none`. No ten-partition capacity handoff was produced; the split
  image route still lacks the required inactive allocation. No partition or
  slot was changed.
- The native builder wrapper had used `exec docker run` after writing a
  temporary env file that may contain the one-time rootfs password; that
  bypassed its exit trap. It now waits for Docker and deletes the file on
  success and failure, preserving Docker's exit status. Offline tests cover
  ordinary and privileged runs and failure cleanup. Scoped stale wrapper
  env files older than ten minutes were removed from the Mac's current temp
  directory and the authorized builder's `/tmp`; both checked directories
  then had zero matching files.
- Reviewed commits were fast-forwarded to the clean native x86_64 builder.
  Its native-wrapper and environment/attempt tests passed there. The builder
  lacks `just`, so the static attempt handoff now names the checked-in shell
  script directly; no host tool was installed. Docker reports `linux/x86_64`;
  the builder image was refreshed from the reviewed Dockerfile as
  `sha256:3cdcd7157ec2ac18dfc3c1b0cf50b042cc9d22ad3ba7291dc2d3e444d97755c8`.
  A fresh container `env check build` passed, and its temporary env file was
  removed. No new ROM image was built in this audit.
- The phone was detached for the initial audit, then attached with owner
  authorization for ADB/Fastboot. Initial CLI discovery saw no transport even
  though macOS detected a Pixel 9a USB interface; a later read-only discovery
  succeeded as described above. IOKit showed USB clients from Opera, Brave,
  and Spotify, but no cause for the initial CLI failure was established. The
  earlier tested stock recovery and static bundle remain separate historical
  evidence; neither establishes a Ferrix boot.
- The selected physical build path remains the eight-static-partition
  WWAN/SIT bundle. The ten-partition split path is still blocked by missing
  positive inactive DLKM capacities. The current static candidate remains
  `composed-not-booted` / `bootable=false`, and `image_compatibility=partial`
  refuses persistent installation. A successful offline build or Fastboot
  command cannot resolve that gate; an observed kernel probe and separate
  compatibility review are still needed.
- Added `pixel-9a-attempt-env` to generate one ignored, fresh-output handoff
  from a clean reviewed commit and exact named static capacity TSV. The native
  builder rechecks the commit and capacity SHA-256 before each device-free
  stage. This closes the prior manual override mismatch that made boot inputs
  name a different kernel manifest during a fresh attempt. The target dev
  README now gives one concise stage order and exact-path transfer check.
- The radio inventory correctly keeps telephony unsupported without live
  WWAN/SIT and Ferrix evidence. Its radio-enable fallback had used a CPIF-only
  explanation even for the selected WWAN/SIT route; that diagnostic is now
  route neutral. The target README and Linux plan no longer describe the
  unbooted native builds as absent or state that CPIF is the reason all
  telephony remains unsupported. The runtime firmware, SIT, and ModemManager
  service chain was inspected offline; no physical radio functionality is
  inferred.
- Device-free verification passed: the selected Pixel build, static install,
  guest evidence, and radio recipes (including the attempt generator), plus
  the generic device, canonical comparison, radio split/flash, inactive-slot,
  and partition workflow suites;
  `cargo fmt --check`,
  `cargo clippy --workspace --all-targets --all-features -- -D warnings`,
  and `cargo test --workspace`. `just host-check` reported that the local
  Docker engine is not running; no host tools were installed. QEMU graphics
  and physical Ferrix behavior remain untested by this change set.

## 2026-09-30 (fresh Pixel 9a attempt and build gate refusal)

- The owner now requires a new native build before every physical loading
  attempt. The mobile-device playbook and target README state that policy, and
  the native wrapper forwards separate attempt-specific kernel, product, and
  rootfs output paths. Its offline test passed. The reviewed source commit is
  `29eb332d54505e0c1fccb84482915d810277c95c` on native deba Linux x86_64.
- Attempt `20260930T032948Z` freshly built a radio kernel (manifest SHA-256
  `578c0e35bdafe7ee11b4475ffe58470ef9a309a473ee21b128dbd29827a3d809`),
  product (manifest SHA-256
  `eba8e1b7c829af9c3c1457bf865793ed06b345af2fdf97f37dc65e517d19908e`),
  and pmbootstrap rootfs (manifest SHA-256
  `5be513bc17f5f02cb0934ab69a5aaf1c3769e179652c59a44739d3d0e15b0162`)
  under new ignored output paths. The product used Cargo's verified build
  cache; its output directory and manifest were created for this attempt.
- The fresh boot-input build passed, but the static composer's read-only
  preflight refused verbatim: `FAIL pixel-9a-static-boot: boot inputs name
  another kernel_manifest`. The boot-input builder had used its hard-coded
  default kernel manifest instead of the attempt kernel output. No final
  bundle was composed or transferred, and no Pixel command was run. The
  producer default has been corrected to follow `FERRIX_PIXEL9A_RADIO_KERNEL_OUT`;
  its offline regression test now covers a nondefault kernel output. The
  failed boot-input output remains ignored as evidence and is not a candidate.
- After the owner directed continuation, corrected boot inputs were built into
  a new directory from commit `80534d7ac34dc116e6ace9e607a0bf8b383bc24e`
  (manifest SHA-256
  `4edca3eba008734497b2e8a422bd07d79fff6acc15390789f0d533fd77584399`).
  Static preflight, composition, and native AVB verification passed. The new
  bundle `out/pixel-9a/images/tegu-wwan-static-20260930T032948Z` has manifest
  SHA-256 `17edb90a0ef6ba1e1921279deced2d942e84edc89b74c19e6fd6a728c077c122`,
  `boot.img` SHA-256
  `bffd420dada4cee402977fab39374107c8239b6ba2acbbbb073fd9e898c7964c`,
  and `rootfs.img` SHA-256
  `dbe076d82d63312bdde9877ff7207d49d2e0c362238148866149ae631a015ef3`.
  The complete 30-file, 3,033,910,426-byte bundle was transferred to the
  Mac. `pixel-9a-static-boot-verify-transfer` passed with the exact named
  output path. Kernel, product, and rootfs outputs were freshly built for
  this attempt from commit `29eb332`; only the corrected boot-input producer
  and final composer ran from commit `80534d7`.
- Read-only `pixel-9a-flash-static-check` passed at about 04:36 UTC on the
  exact Google Pixel 9a / `tegu` / `GXQ96` / `MP1.0`, in ordinary Fastboot,
  unlocked, active slot A. It rechecked the new bundle and tested stock
  recovery artifacts. Ignored redacted transcript:
  `out/pixel-9a/flash-static/preflight-20260930T043607087786Z-21642.txt`.
  The candidate remains `composed-not-booted`, and target
  `image_compatibility=partial` still forbids persistent flash. No temporary
  boot, partition write, or slot change has run for this bundle.

## 2026-09-30 (fresh Pixel 9a static capacities and unbooted native images)

- **Physical target and transport:** One owner-approved run of
  `just pixel-9a-enter-fastboot --confirm-enter` rechecked the exact Pixel 9a /
  `tegu` / `GXQ96` / `MP1.0`, active slot A, and entered ordinary bootloader
  Fastboot at about 01:44 UTC. It changed no partition, slot, bootloader
  state, or user data. The phone remained in Fastboot at that point; no Ferrix image has
  been booted or flashed.
- **Fresh static capacities:** `just pixel-9a-partition-capacity-static`
  passed at `2026-09-30T01:44:54Z`, after the verified stock restore. Both
  slots independently reported the same positive sizes for the eight static
  partitions: boot 64 MiB, init_boot 8 MiB, vendor_boot 64 MiB,
  vendor_kernel_boot 64 MiB, dtbo 16 MiB, and each vbmeta partition 64 KiB.
  The schema-2 handoff is
  `out/pixel-9a/partition-capacities-static-20260930T014454Z-48820.tsv`
  (SHA-256 `9e7c7406b77b03c9cc0dbcc66d7f4682b9149a041a355c468a99454687b2d66d`).
  Its read-only transcript is
  `out/pixel-9a/partition-capacity-static-20260930T014454Z-48820.txt`
  (SHA-256 `f99498bbdfd9fe8100b98b9d1430281c1232d33232293f4d8417dd85fa214aa3`).
  The ignored handoff was copied to the authorized native builder. The
  subsequent Fastboot inventory remains `partial` and by itself does not
  establish a rootfs mapping (`out/pixel-9a/partition-inventory-20260930T014504Z.txt`,
  SHA-256 `08b9eda6bd5a789955d41c2723853f45fbf31135c7683211f4b223b2d0790634`).
- **Targeted rootfs map:** `just pixel-9a-userdata-map` correlated the
  post-restore same-device ADB by-name record with live ordinary-Fastboot
  `has-slot`, `is-logical`, type, snapshot, and size getvars at
  `2026-09-30T02:02:26Z`. It found unslotted physical `userdata` at Android
  `/dev/block/sda34`, 117,543,358,464 bytes, distinct from `super`, matching
  the source initramfs `PARTLABEL=userdata`. The ignored handoff is
  `out/pixel-9a/userdata-map-20260930T020226Z-97470.tsv` (SHA-256
  `a0122271f54d467adb4a2feead0288c84a86aaa76b843329455e68a6e65fd316`);
  its transcript SHA-256 is
  `9ccf603b5c78287e39e805526e04839c10cbc1b3695bbc3842fe8ebf0e829618`.
  The broader inventory is still `partial`, and this targeted read-only map
  authorizes no format, flash, or user-data wipe.
- **Native build and validation:** The fresh source-pinned radio kernel rebuild
  completed on deba Linux x86_64. Its ignored `build-manifest.tsv` SHA-256 is
  `5e45844cab8841f6b3cfe359bafe612db917a480372078b7ea46cffdf8708eec`,
  with the current split-contract hash
  `0af2c011b468cbf97837abca29d666f828109999cb30f4c158534530e202046e`.
  Rebuilt source-owned boot inputs have manifest SHA-256
  `29a1adcc2128512dbdc9d3111486532ff254ab981583f9751db1d8b267e07e20`.
  `pixel-9a-static-boot` preflight, opt-in build, and a separate native
  verifier run passed. The output manifest at
  `out/pixel-9a/images/tegu-wwan-static-rebuild-20260930/boot-manifest.tsv`
  has SHA-256 `95b5bcf20f74b83eb00f4159f1a69b6b9e31f0ea143953f6b0f4640bb4ebd78a`.
  It records eight static images, checked source and rootfs handoffs, fresh
  AVB descriptors, `composed-not-booted`, and `bootable=false`. After
  transfer to the Mac, the manifest and all 29 recorded file hashes matched.
  Kbuild's `depmod` reported unresolved symbols in optional installed
  modules; no loadable module is selected for the static boot path, and this
  build evidence does not establish a successful device boot.
- **Static physical preflight:** A separate checked-in candidate pins the
  transferred bundle, fresh capacity handoff, same-device Android discovery,
  targeted `userdata` map, and tested factory/OTA recovery artifacts.
  `just pixel-9a-flash-static-check` passed read-only on the exact device in
  ordinary Fastboot on slot A. Its ignored, serial-redacted transcript is
  `out/pixel-9a/flash-static/preflight-20260930T022033Z-23683.txt` (SHA-256
  `b7caf1095175d9748e947eabc698b01f08ef31f2e20ecbc418eeb28d549171b8`).
  The candidate's image-compatibility gate remains `partial`; it authorizes
  no persistent write. The owner approved one temporary `fastboot boot` of
  the pinned kernel-only `boot.img` (SHA-256
  `be3c389af8d8e49914ccddefb7f00c8820a2a8ec7f950ef76c2adfd74bd8a5c8`)
  after checked-in gate and per-run confirmation. It can probe kernel
  progress with stock `init_boot` and `vendor_boot`, not the full Ferrix boot.
- **Probe stopped before device action:** A later optional
  `just pixel-9a-static-boot-verify-transfer` invocation omitted the custom
  `FERRIX_PIXEL9A_STATIC_BOOT_OUT` path and refused verbatim:
  `FAIL pixel-9a-static-boot: static image bundle is missing or a symlink`.
  This invocation inspected the default output location, not the pinned
  rebuild bundle. The prior bundle hash check and native AVB verifier remain
  recorded above, but the failed invocation was not retried. No temporary
  boot command was issued; the owner's one-probe approval remains unused.
  The checked-in static workflow now has an offline-tested `observe-temp`
  mode that binds a reviewed kernel capture to the exact boot transcript and
  image hash. Offline tests now also require the complete kernel banner
  embedded in the pinned image, rejecting a stock tegu banner. A checked-in
  read-only `capture-temp` mode can collect matching `/proc/version` and
  device-tree model if Android ADB returns on the exact phone after a future
  probe. Its procfs snapshot would match the running build banner but would
  not prove loaded image bytes, absence of earlier kernel errors, or a Ferrix
  userspace boot. Both modes are
  stub-tested only; neither has run on physical evidence or changes the
  stopped device route.
- **Persistent static adapter (offline only):** A separate target workflow now
  defines exact inactive slot B boot writes, a separately confirmed shared
  `userdata` rootfs write, and later slot B activation. Its mock transport
  test checks the eight-partition allow-list, refused partial compatibility,
  explicit unsigned-AVB policy, typed confirmation, stage evidence binding,
  a failed first write, command order, and a separate pinned slot-B activation
  proposal. The image candidate itself still forbids slot selection. The live
  target reports `image_compatibility=partial`; no static flash or slot
  selection was attempted. Kernel progress and
  full Ferrix boot remain unobserved; an accepted Fastboot write would not be
  a readback verification of its bytes.
- **Post-activation guest inventory (offline only):** A target-scoped,
  read-only SSH workflow now checks the exact tegu device tree after
  validating the pinned candidate, temporary-boot observation, both static
  flash-stage records, and the slot-B activation record. It records
  normalized slot, OpenRC, socket, pidfile, DRM, and input inventory under
  ignored `out/pixel-9a/guest-evidence/`. Stub tests include wrong-device
  refusal and refuse SSH before activation evidence. Presence is explicitly
  separate from a connected Wayland client, an inspected display capture,
  delivered touch, or working services. No Ferrix guest exists on the phone
  yet; this workflow has not run against hardware.
- **Recovery/storage decision:** The owner chose the tested pinned factory
  restore as the stock recovery path if Ferrix later uses shared `userdata`.
  This is a storage-plan choice, not authorization for a `userdata` wipe or
  any physical write. A persistent static flash adapter, independent boot
  observation, and separate per-run confirmation remain outstanding.

## 2026-09-30 (post-restore Pixel 9a admission and build gate refresh)

- **Exact physical target:** Read-only `just device-discover`, `device-match`,
  and `pixel-9a-discover` at 01:19–01:20 UTC found one authorized Google
  Pixel 9a / `tegu` / `GXQ96` / `MP1.0`, still on stock Android
  `CP3A.260905.009`, active slot A, unlocked/orange. Redacted discovery:
  `out/pixel-9a/discovery-20260930T012016Z.txt` (SHA-256
  `e00ec15ae3e8fec30a4cd03cf469e9d0e37fa98cdefec1fdbb8d099e69f71ff3`).
  The fresh ADB partition inventory is still `partial`; its ignored transcript
  is `out/pixel-9a/partition-inventory-20260930T012016Z.txt` (SHA-256
  `d6b3e4af551650cb185c5e25e0e99964b02764dbb8867b7c7805854e1aa58ef8`).
- **Capacity refusal:** The read-only Android handoff stopped with
  `REFUSING pixel-9a partition capacity handoff: boot_a capacity is not readable from /sys/class/block/sda13/size`.
  Transcript: `out/pixel-9a/partition-capacity-20260930T012016Z.txt`
  (SHA-256 `0ee63a7a0432108bcddf893334ca5208fab28f3c47f897a853c5e708926d549f`).
  No post-restore static or ten-partition capacity handoff was published;
  the pre-restore static capacity file is historical only.
- **Native builder refusal:** The authorized `deba` Linux x86_64 checkout at
  `/home/ezy/github-projects/omarchy-ferrix` remains clean at `26fc9f9`.
  `tools/pixel-9a/native-builder.sh run boot-radio preflight` refused:
  `FAIL pixel-9a-radio-boot: radio kernel manifest split-artifact contract hash differs from the checked-in contract`.
  The existing kernel manifest names SHA-256 `221347949752c3a7865264958bd5ad58e0fec1f9a080630902b3c384488bac12`;
  the current checked-in contract and route pin are
  `0af2c011b468cbf97837abca29d666f828109999cb30f4c158534530e202046e`.
  No image composition or rebuild followed that refusal. The kernel, rootfs,
  and boot-input outputs remain prior, unbooted handoffs, not a current
  static-only image set.
- **Physical boundary:** No reboot, temporary boot, flash, slot change,
  rootfs write, firmware write, or radio test was performed. The tested stock
  restore remains available. The static-only composer and flash adapter,
  fresh bootloader capacities, complete rootfs mapping, AVB validation, and
  exact-device boot evidence remain outstanding.

## 2026-09-30 (stock recovery verified on the exact Pixel 9a)

- **Physical target:** `google-pixel-9a-tegu-gxq96-mp1-0`. Owner-run
  `just pixel-9a-discover` at `2026-09-30T01:02:38Z` returned `result=ok`
  through one authorized ADB device. The saved, redacted transcript matches
  the completed restore's device identity and reports
  `google/tegu/tegu:17/CP3A.260905.009/16091614:user/release-keys`, Android
  17, security patch `2026-09-05`, bootloader `tegu-17.0-15819938`, slot
  `_a`, unlocked/orange. The Android slot-count property remains empty;
  discovery alone does not refresh the partition inventory.
- **Recovery evidence accepted:** the successful factory restore plus this
  same-device stock Android discovery satisfy the provisioning skill's
  recovery-test requirement. `[gates].recovery_procedure` is now `tested`.
  The manifest records both ignored evidence paths and their SHA-256 hashes;
  raw transcripts and unique identifiers remain out of Git. This verifies
  restoration to the accepted newer pinned stock build, not downgrade to the
  original build or recovery from every possible hardware/firmware failure.
- **Still gated:** Ferrix `image_compatibility=partial`, native Linux boot
  not attempted, capabilities unsupported, fresh post-restore partition
  evidence required, static-only composer/flash adapter still incomplete.
  Backup/data-policy and per-run confirmation requirements are unchanged.
  No further phone writes or boot-state changes occurred during verification.
- The owner's normal-terminal workflow supplied valid device evidence; the
  agent's earlier sandbox ADB startup failure is a host-access limitation,
  not evidence that the stock restore failed. No sandbox bypass was used.

## 2026-09-30 (authorized stock restore completed; Android verification pending)

- **Exact physical target:** `google-pixel-9a-tegu-gxq96-mp1-0`. After
  explicit owner approval of the pinned stock restore and development-device
  data policy, `just pixel-9a-factory-restore --confirm-first-test` completed
  with exit 0 and `result=factory-restore-complete`. All three confirmation
  phrases were entered individually at their prompts. Ignored evidence:
  `out/pixel-9a/restore/20260930T002730Z.txt`.
- **Observed writes:** stock bootloader and radio packs were installed;
  post-reboot versions matched `tegu-17.0-15819938` and
  `g5300t-260604-260807-B-16036236`. The pinned `CP3A.260905.009`
  update wrote stock slot-A boot images and the shared `super` image,
  erased userdata/metadata, selected the already-current slot A, and issued
  the final reboot. These are stock vendor-update actions, not Ferrix's
  firmware/active-slot-preserving write contract. No relock occurred.
- **Verification remains pending:** read-only discovery at
  `2026-09-30T00:35:32Z` returned exit 2 / `result=no-device`, with zero
  ADB and Fastboot devices. Ignored evidence:
  `out/pixel-9a/discovery-20260930T003532Z.txt`. Device operations stopped
  pending owner setup/USB-debugging authorization. No Android boot or build
  has yet been observed after the restore; `recovery_procedure` remains
  `documented-not-tested`, not promoted from command success alone.
- The workflow removed its temporary extracted images; the verified
  recovery archives and redacted transcript remain available. No Ferrix
  image was flashed and no capability was promoted. Previous partition
  capacity evidence must be refreshed after this stock layout rewrite.

## 2026-09-30 (approved recovery bootstrap and static capacity handoff)

- **Owner approval:** implement the separate static-only installation
  contract and first-stock-recovery-test workflow. Physical writes still
  require their exact preflight and fresh per-run approval; the accepted
  recovery destination is pinned stock `CP3A.260905.009`.
- **Recovery bootstrap implemented:** `--preflight-first-test` and
  `--confirm-first-test` preserve exact identity, pinned archive hashes,
  stock board requirements, vendor-sequence verification, and data safety.
  Only this explicit mode permits `documented-not-tested` recovery and
  partial Ferrix image compatibility; it verifies the independent stock
  inputs, demands an extra acknowledgement, and always uses the vendor wipe
  sequence. Normal restore gates remain unchanged. No mode edits the target
  gates or claims a successful Android boot from command completion.
- **Physical read-only preflight:** exact target
  `google-pixel-9a-tegu-gxq96-mp1-0`, slot `a`, unlocked, ordinary bootloader,
  passed at `2026-09-30T00:16:27Z`; ignored transcript
  `out/pixel-9a/restore/20260930T001627Z.txt`. Both pinned archives were
  rehashed. The reviewed stock sequence requires newer bootloader
  `tegu-17.0-15819938` and baseband `g5300t-260604-260807-B-16036236`;
  these changes and the data wipe were disclosed. Per-run authorization was
  requested, not inferred from tooling approval. No restore was executed.
- **Static capacities acquired:** after the owner entered bootloader mode,
  read-only inventory and the new schema-2 `--static-boot` capacity workflow
  succeeded. Both slots independently reported boot/vendor_boot/
  vendor_kernel_boot 67,108,864 bytes, init_boot 8,388,608 bytes, dtbo
  16,777,216 bytes, and each vbmeta partition 65,536 bytes. All sixteen
  partition entries were classified physical. The separate ignored handoff
  is `out/pixel-9a/partition-capacities-static-20260930T001738Z-91048.tsv`;
  no dynamic sizes were substituted and the ten-partition handoff remains
  unavailable. Broader inventory remains partial.
- **Native static contract:** the approved source-only TOML contract is
  checked in and hash-pinned separately from the existing split contract.
  Extracting IKCONFIG from the verified builder `Image` with the pinned
  upstream helper reproduced configuration hash
  `a644e47fac409e47d2c132dfb1dc02839f645812ce0d10a7bc2e93634c766b2d`.
  The static-only composer/flash adapter are not implemented yet; the
  existing route must not consume the new eight-partition handoff.
- **Validation:** factory-restore and capacity regressions pass, including
  first-test acknowledgement, ordinary-mode refusal, data safety, gate
  non-promotion, wrong transport, physical classification, and separate
  output schema. No firmware write, slot change, partition write, native
  rebuild, or capability promotion was performed.

## 2026-09-30 (remaining-blocker investigation and scoped Fastbootd fix)

- **Implemented:** the radio-first flash adapter now accepts the observed
  Fastbootd placeholder only after a fully verified ordinary-bootloader
  identity in the same process, complete same-serial ADB proof of `GXQ96`,
  and all live identity/state rechecks. Bootloader placeholders and absent
  correlation remain refused. Shared SKU helpers and recovery gates were
  not changed. Offline flash tests pass with the placeholder through the
  complete simulated round trip and refuse missing proof before image writes.
  Capacity and inactive-slot tests, shell syntax, `cargo fmt --check`, and
  `git diff --check` also pass; full Rust/QEMU tests were not rerun for these
  host-workflow changes.
- **Exact physical target:** `google-pixel-9a-tegu-gxq96-mp1-0`. A local
  read-only diagnostic at `2026-09-30T00:09:13Z` recorded unlocked/secure
  `yes`, snapshot-update-status `none`, super-partition name `super`, and
  logical classification `yes` for all four DLKM slot entries. The prior
  missing static-size pairs and zero inactive DLKM sizes persist. Metadata
  observations do not establish free extents or authorize allocation changes.
  Ignored evidence:
  `out/pixel-9a/partition-capacity-diagnostic-20260930T000913Z-36717.txt`.
- **Builder read-only evidence:** deba remains at
  `26fc9f9beb1d1faf041a8d4df71fe6142353c133` with built source inputs. Empty
  selected module policies and generated load lists, together with built-in
  Exynos UFS, ext4, USB, display, and radio drivers, support investigating an
  independent eight-static-partition contract that leaves `super` untouched.
  The target Linux plan records the configuration hash and required checks;
  no alternative route is implemented or approved by these observations.
- **Recovery:** no `factory-restore-complete` transcript was found in the
  local restore evidence directory. The owner's earlier report is retained,
  not invalidated or upgraded. The existing restore execution path requires
  already-tested recovery and Ferrix image compatibility; an explicit first
  stock-recovery-test contract has been proposed for owner approval rather
  than bypassing or relabelling either gate.
- **Boundary:** only read-only phone/builder probes and local code/tests ran.
  No firmware write, flash, resize, reboot, slot selection, recovery-pin edit,
  rootfs mapping change, builder sync, or native rebuild occurred. Approval
  was requested for the separate static-only route and first-test tooling;
  physical actions still need their own exact preflight and confirmation.

## 2026-09-30 (stock-preserving radio-first flash safeguards)

- **Exact target:** `google-pixel-9a-tegu-gxq96-mp1-0`. The owner confirmed
  preserving bootloader/radio firmware and the active stock slot. The target
  Linux plan now separates inactive boot-image writes from shared logical
  allocation, snapshots, and `userdata`; rootfs mapping remains unset.
- **Implementation, not hardware evidence:** radio-first persistent-flash
  preflight with an explicit inactive slot now reads all twenty A/B sizes
  and requires equality with the reviewed capacity handoff. Missing, zero,
  changed, ambiguous, overflowing, and unaligned sizes refuse before mutation.
  Logical image sizes must equal their existing allocation so a smaller or
  larger image cannot implicitly authorize a resize. Temporary boot remains
  independent of this persistent-write check.
- **Pre-write sequencing:** after typed confirmation, the workflow visits
  Fastbootd to check exact identity, logical classification, sizes, and
  `snapshot-update-status=none`, then returns and rechecks before the first
  image write. These checks are repeated at the write boundary. Early
  Fastbootd failures now leave images untouched, although the device can
  remain in Fastbootd; failures after writes begin can still leave a partially
  flashed inactive slot. The successful stub route performs ten inactive
  writes and four transport reboots, never slot selection or firmware writes.
- **Validation:** radio-first flash, inactive-slot activation, capacity, and
  radio-split regression suites pass, along with shell syntax,
  `cargo fmt --check`, and `git diff --check`. Fixtures cover smaller logical
  images with valid hashes, zero/changed/malformed/duplicate capacities,
  failed queries with zero CLI exit, Fastbootd allocation differences,
  snapshot refusal, and logical classification. The flash test now selects
  successful temporary-boot evidence explicitly instead of relying on
  filesystem iteration order, and clears per-case stub overrides. Full Rust
  runtime and QEMU suites were not rerun for these host-shell-only changes.
- **Recovery distinction:** owner-reported restoration is not promoted by
  these changes. The pinned recovery `CP3A.260905.009` is not the observed
  stock build `BD4A.240925.111`; restoring the former cannot be described as
  restoring that exact old ROM. The owner accepted the pinned newer build as
  the recovery destination in this session, without authorizing a flash.
  Recovery completion still needs evidence; no downgrade compatibility is
  claimed.
- **Still blocked:** the live zero-capacity inactive DLKM pair, incomplete
  cross-mode size evidence, mutation-path `MSM USF` SKU refusal, recovery
  completion evidence, and rootfs storage choice are unresolved. No safety
  gate, capacity contract, recovery pin, or capability claim was relaxed.
  This session contacted no phone and performed no flash, reboot, resize,
  slot change, or builder sync/build.

## 2026-09-30 (capacity diagnostics fixed; physical layout blockers identified)

- **Owner-authorized read-only fix:** capacity queries now retain their
  serial-redacted raw responses and process exit status. Missing-size
  refusals point to this evidence instead of incorrectly suggesting another
  Fastbootd transition. Success status is emitted only after the complete
  capacity handoff is published. Literal serial redaction also handles
  regex-special characters.
- **Diagnostic route:** `just pixel-9a-partition-capacity-diagnose` checks
  exact identity and userspace mode before collecting all twenty fixed A/B
  size queries. It records failures and zero values without creating any
  capacity TSV or changing a gate. A completed scan is explicitly
  `diagnostic-only`, not installation readiness.
- **Validation:** capacity and radio-split regression suites pass, as do
  shell syntax, `cargo fmt --check`, and `git diff --check`. New fixtures
  cover failed queries with either zero or nonzero CLI exit, zero inactive
  capacity rejection, complete diagnostic scanning without TSV publication,
  literal serial redaction, and wrong-transport/identity refusals. Rust
  runtime and QEMU suites were not rerun for these shell-only changes.
- **Physical evidence:** the local USB-enabled scan at
  `2026-09-29T23:46:50Z` verified the exact Pixel 9a target, current slot `a`,
  two slots, and userspace Fastboot. Twelve queries (both slots of
  `vendor_boot`, `vendor_kernel_boot`, `dtbo`, `vbmeta`, `vbmeta_vendor`,
  and `vbmeta_system`) returned
  `FAILED (remote: 'Could not open partition')`, despite process exit zero.
  Both boot sizes were `0x4000000`; both init_boot sizes were `0x800000`.
  `vendor_dlkm_a` was `0x1310000`, `system_dlkm_a` was `0xB2B000`, and both
  corresponding `_b` sizes were `0x0`. The ignored transcript is
  `out/pixel-9a/partition-capacity-diagnostic-20260929T234650Z-57156.txt`.
- **Boundary:** missing static sizes and zero inactive logical capacities
  independently prevent a complete positive/equal A/B handoff. No capacities
  were guessed or copied between slots, and no prior bootloader observations
  were silently combined with current evidence. Any cross-mode acquisition
  or logical-partition allocation change needs a reviewed target plan;
  allocation changes additionally require fresh per-run mutation approval.
  No phone reboot, flash, resize, slot change, or recovery/rootfs gate change
  was performed. Stock restore remains owner-reported, not newly verified.

## 2026-09-30 (Fastbootd identity correction passed; capacity still incomplete)

- **Owner-authorized correction:** only the read-only capacity workflow now
  handles Google's `MSM USF` placeholder after live userspace-mode, exact
  product/revision, and A/B slot checks. Complete matching serial-bound ADB
  discovery must still prove `GXQ96`. Observed variant and verified SKU are
  recorded separately; shared flashing/restoration/activation gates are
  unchanged.
- **Regression evidence:** the new fixture reproduced the original refusal
  before the fix. Capacity and radio-split tests now pass, including rejection
  of absent/incomplete/wrong-SKU/wrong-serial evidence, bootloader mode,
  unknown variants, wrong product/revision/slot fields, and multiple devices.
  A direct shared-helper test confirms that mutating workflows did not gain
  the placeholder exception. Shell syntax, formatting, and diff checks pass;
  no Rust product or QEMU behavior changed.
- **Physical retry:** the authorized local read-only run at
  `2026-09-29T23:40:28Z` passed identity correlation for
  `google-pixel-9a-tegu-gxq96-mp1-0`, with `is-userspace=yes`, current slot
  `a`, and two slots. Both boot capacities were 67,108,864 bytes and both
  init_boot capacities were 8,388,608 bytes. It then stopped with
  `REFUSING pixel-9a partition capacity handoff: partition-size:vendor_boot_a
  is not reported by Fastboot; dynamic partitions require fastbootd and
  independent A/B getvar rows`.
- **Boundary:** no complete capacity file was published. The partial ignored
  transcript is `out/pixel-9a/partition-capacity-20260929T234028Z.txt`, not a
  usable ten-partition handoff. No further device calls, reboot, flash, slot
  selection, recovery-gate change, or rootfs mapping change followed the
  refusal. The next blocker is obtaining the missing capacities through a
  reviewed read-only workflow; no partition size may be guessed.

## 2026-09-30 (Fastbootd variant compatibility refusal)

- The owner supplied `is-userspace: yes` after manually entering Fastbootd.
  The local USB-enabled capacity workflow then returned exit 1:
  `REFUSING pixel-9a partition capacity handoff: SKU/variant 'MSM USF' is
  unavailable and no matching redacted ADB identity proves GXQ96`.
- **Read-only diagnosis:** the shared SKU helper rejects this non-SKU string
  before trying its ADB correlation path. The existing exact-target ADB
  discovery still records Google / Pixel 9a / tegu / GXQ96 / MP1.0.
  Google's [Pixel Fastboot HAL source](https://android.googlesource.com/platform/hardware/google/pixel/+/1c8e4019/fastboot/Fastboot_aidl.cpp)
  hardcodes `MSM USF` in `getVariant`; that source corroborates a placeholder
  interpretation, not the attached device's SKU. A correction would need to
  be scoped to verified userspace Fastboot and still require matching
  serial-bound complete ADB identity, live product/revision, and slot checks.
- No capacity handoff was published, no further device calls were made after
  the refusal, and no identity rule, recovery gate, or phone state was changed.
  The provisioning playbook stopped the run pending review/authorization of
  that compatibility correction and a fresh read-only attempt.

## 2026-09-30 (recovery transport observed, Fastbootd still pending)

- Following the owner's next mode-change confirmation, the local,
  USB-enabled `just pixel-9a-partition-capacity` check returned exit 1:
  `REFUSING pixel-9a partition capacity handoff: ADB state is 'recovery',
  not device`. The transport is now visible as ADB recovery, not the
  Fastbootd mode required by the capacity workflow.
- No new exact-target identity or capacities were established by this
  attempt, no complete handoff was published, and no device mutation or
  gate change occurred. The provisioning playbook stopped further device
  operations pending owner-controlled entry into Fastbootd.

## 2026-09-30 (local USB access restored outside sandbox)

- After the owner demonstrated local Fastboot enumeration, the checked-in
  `just pixel-9a-partition-capacity` workflow was run on the Mac with approved
  sandbox escalation. It detected a single Fastboot transport and passed the
  target identity/slot checks, then returned exit 1:
  `REFUSING pixel-9a partition capacity handoff: Fastboot capacity handoff
  requires fastbootd (is-userspace=yes) so both dynamic A/B logical partitions
  are independently observable; no reboot was issued`.
- This narrows the immediate blocker to transport mode, not a demonstrated
  cable fault: the workflow can access the local phone outside the sandbox,
  but it is not reporting userspace Fastboot. No capacity handoff was
  published and no reboot, flash, slot change, or gate bypass occurred.
  Per the provisioning playbook, the run stopped pending owner-entered
  Fastbootd. Deba was not used for any USB/device operation.

## 2026-09-30 (owner-entered Fastbootd, transport not visible)

- The owner reported enabling Fastbootd for the admitted
  `google-pixel-9a-tegu-gxq96-mp1-0` device. The read-only
  `just pixel-9a-partition-capacity` workflow returned exit 1:
  `REFUSING pixel-9a partition capacity handoff: capacity handoff requires
  exactly one ADB or one Fastboot device (adb=0 fastboot=0)`.
- No live identity, userspace-mode flag, or partition capacity could be
  verified in this attempt. No complete handoff was published, no phone
  operation was performed, and no gate was changed. The provisioning session
  stopped pending restored USB visibility; no automatic retry was attempted.

## 2026-09-30 (owner-reported recovery and live partition refusal)

- **Recovery report:** the owner reports having tested recovery using
  `tools/pixel-9a/factory-restore.sh` and requests continued Ferrix delivery.
  The restored build and completed execution transcript remain unconfirmed;
  the existing local restore records contain preflight evidence only. No
  additional recovery rehearsal was scheduled. The reported workflow is
  recorded as owner testimony, not independently verified execution, and the
  machine-enforced recovery gate was not changed without the missing details.
- **Physical identity:** read-only discovery at `2026-09-29T23:27:59Z` and
  exact-target discovery at `2026-09-29T23:28:01Z` matched
  `google-pixel-9a-tegu-gxq96-mp1-0` over ADB. The phone runs stock Android 15
  build `BD4A.240925.111`, with an unlocked bootloader, orange verified boot,
  and current slot `_a`. This corroborates current stock Android operation,
  not the procedure or image used in the owner's recovery test.
- **Partition blocker:** the fresh read-only inventory at
  `2026-09-29T23:28:12Z` observed both slots but remained `partial` because
  Android denies `/proc/partitions` and `/proc/cmdline`. The capacity workflow
  then stopped with `REFUSING pixel-9a partition capacity handoff: boot_a
  capacity is not readable from /sys/class/block/sda13/size`. No complete
  capacity handoff was published. The supported next capacity acquisition
  requires owner-entered Fastbootd; it does not replace the separate complete
  inventory and reviewed rootfs mapping gates. No reboot, temporary boot,
  flash, slot change, or gate bypass occurred.
- **Validation and builder:** host/tool checks and all provisioning-playbook
  prerequisite workflow tests passed (stub evidence only). Deba remains at
  `26fc9f9` with the previously verified boot-input manifest; it has no final
  split boot manifest or complete capacity handoff. Device transcripts remain
  ignored under `out/device/` and `out/pixel-9a/`; no generated output or
  unique identifiers are included in this record.

## 2026-09-30 (native boot-input retry passed)

- **Source synchronization:** selector correction `206b41f` was pushed and
  fast-forwarded into the clean `deba` build checkout. The checked-in native
  builder's boot-input preflight and build both passed. No phone operation
  ran and no recovery or hardware capability was promoted.
- **Unchanged substrate:** the rootfs archive remains SHA-256
  `fef00f97a511c752d6480fc46e4b3f7c9b48eff18f1876636782b14eee83a16c`;
  its manifest remains
  `3398822910ab9bbfa390d4beac3d51b0629824132a80c49538584c52b33530c5`.
  The architecture, regular-file, and handoff hash gates remain intact.
- **Published build evidence:**
  `out/pixel-9a/boot-inputs/tegu-wwan/manifest.tsv` is SHA-256
  `14026948e3ea66354675b50543655a239605d0727b5d3a429a88fdef451d099f`.
  All four published artifact sizes and hashes were independently rechecked.
  Native `readelf` identifies the extracted BusyBox as ELF64 AArch64 with
  `/lib/ld-musl-aarch64.so.1`; the copied libc alias matches the loader bytes.
  Outputs remain ignored, `built-source-inputs`, and `bootable=false`.
- **Validation:** boot-input and radio-split workflow tests passed, along
  with shell syntax, `cargo fmt --check`, and diff checks. These regression
  tests use stubs; the native result above is real artifact construction,
  not boot or recovery evidence. Workspace tests, Clippy, and QEMU were not
  rerun for this shell-only selector correction.
- **Remaining delivery gates:** first stock-recovery rehearsal still needs
  a reviewed bootstrap procedure that does not falsely assert prior testing.
  Complete partition evidence, reviewed rootfs mapping, final split-image
  construction/AVB checks, observed temporary boot, installation, and guest
  validation remain pending. This build result authorizes none of those
  physical operations.

## 2026-09-30 (source synchronization and native boot-input failure)

- **Source synchronization:** `7075934` was committed, pushed to the canonical
  repository, and fast-forwarded into the clean native builder checkout at
  `/home/ezy/github-projects/omarchy-ferrix`. The older Orca `remora` worktree
  was left unchanged.
- **Native result:** the boot-input preflight passed, but construction stopped
  with `FAIL pixel-9a-boot-inputs: rootfs BusyBox is not an AArch64 executable`.
  No completed boot-input output was published and no device operation ran.
- **Read-only diagnosis:** the rootfs archive contains both
  `etc/busybox-paths.d/busybox` (a text file) and `usr/bin/busybox`. The generator
  selects the first basename match, so it extracted the text file. Inspection
  of the actual `usr/bin/busybox` reports AArch64 ELF with the expected musl
  interpreter. The architecture gate correctly rejected the wrong member;
  neither it nor the rootfs hash check was bypassed. The owner authorized
  correcting the selector and retrying the device-free native build with
  the same verified archive.
- **Selector correction:** runtime files now resolve only at exact `/bin`
  and `/lib` paths or their merged-`/usr` equivalents. Missing or duplicate
  archive members are refused. The regression fixture reproduced the
  original failure with metadata ordered first and now passes, including
  duplicate-member, missing-executable, and invalid-ELF refusal cases.
  The subsequently authorized native retry is recorded above; no physical
  operation is authorized by this build-only approval.
- **Recovery lifecycle review:** both factory restore and OTA execution
  currently require `recovery_procedure=tested|passed|verified`, while the
  exact target still records `documented-not-tested`. This prevents the
  first recovery rehearsal through these workflows. No status was fabricated
  and no recovery gate was relaxed; a separately reviewed initial-recovery
  test procedure is still needed before Ferrix installation.
- **Offline validation:** device discovery/matching, Pixel tools (including
  restore/sideload/unlock), radio kernel, product, firmware, rootfs, Fastboot
  entry, diagnostic radio boot, artifacts, canonical flash, partition capacity,
  radios, and inactive-slot workflow tests passed. These are stub results,
  not physical recovery or boot evidence. Formatting and diff checks passed.

## 2026-09-30 (native builder access restored)

- **Builder connectivity:** SSH to the authorized `deba` host now succeeds.
  The host reports Linux x86_64; the existing native builder's
  `tools/pixel-9a/native-builder.sh run env check build` passed. No host tools
  were installed and no phone transport was exposed to the builder.
- **Workspace reconciliation:** the clean builder checkout at
  `/home/ezy/github-projects/omarchy-ferrix` is at `9728c29`, matching the
  Mac's committed baseline. The suggested Orca `remora` worktree is clean
  but older (`1354afd`) and has no `out/pixel-9a` outputs. Neither checkout
  was switched or overwritten.
- **Existing handoffs located:** kernel, product, and rootfs manifest hashes
  were re-read on the builder and match the earlier substrate handoff
  recorded below. This check did not revalidate every payload or establish
  compatibility with the uncommitted split-image fixes. Those fixes still
  need source synchronization and native build validation.
- **Physical boundary unchanged:** stock restoration, complete live partition
  evidence, temporary-boot observation, installation, and guest/radio
  validation remain pending. No physical operation was performed.

## 2026-09-30 (observed temporary-boot gate)

- **Implementation evidence for `google-pixel-9a-tegu-gxq96-mp1-0`:**
  successful `fastboot boot` command evidence alone no longer permits
  radio-first persistent writes or inactive-slot activation. Both workflows
  require a reviewed kernel capture bound to the exact device, boot image,
  and temporary-boot transcript. Activation also checks the transcript's
  boot-image hash against the current image manifest.
- **Observation workflow:** `just pixel-9a-observe-temp-boot` records the
  operator-reviewed capture after read-only identity/recovery checks and a
  typed attestation. It requires kernel-version and tegu machine records,
  rejects known panic/lockup markers, and hashes the retained redacted log.
  This is kernel-progress evidence only, not proof of a complete Ferrix boot;
  automated marker checks do not replace observation of hangs or reboot loops.
- **Offline validation:** `just pixel-9a-flash-radio-test` and
  `just pixel-9a-boot-inactive-test` passed, including missing observations,
  same-size capture tampering, duplicate success records, panic captures, and
  mismatched boot images. All device interactions in these tests are stubs.
- **Physical handoff remains blocked:** the authorized native builder's SSH
  connection was refused with `Operation not permitted`; the last discovery
  reported `No ADB or Fastboot device is visible.` No physical operation was
  retried after that refusal. Corrected native image construction, complete
  capacity/inventory evidence, reviewed rootfs mapping, tested stock recovery,
  observed temporary boot, installation, and guest validation remain pending.
  No hardware capability or recovery status is promoted.

## 2026-09-30 (split image construction corrections)

- **Implementation evidence for `google-pixel-9a-tegu-gxq96-mp1-0`:** the
  radio split composer now creates both vendor images with the header-v4
  vendor boot container. Previously `vendor_boot` used the ordinary boot
  output option and `vendor_kernel_boot` was a bare CPIO. The composer checks
  the four boot-image magic values, and the flash manifest now requires the
  generated-container origin for `vendor_kernel_boot`.
- **AVB construction:** removed the option that suppressed the embedded
  vbmeta blob/footer needed by descriptor inclusion. DLKM filesystems now
  use explicit 4096-byte blocks and reserve avbtool's calculated metadata
  space. Each final vbmeta image must pass `avbtool verify_image` before the
  output is published. The stricter stub test reproduced the old vendor
  output failure and now covers container output, AVB metadata space,
  descriptor availability, and failure to verify payloads.
- **Boot-input handoff:** the module-install manifest is checked against the
  kernel handoff before use. musl's libc alias is populated from its loader
  even when the rootfs archive contains the libc name as a symlink. Every
  vendor-ramdisk source file and every DLKM root file is now hash-recorded;
  the composer rejects equal-length tampering and unlisted added files.
- **Local validation:** the boot-input, radio-split, radio-flash, diagnostic
  radio-boot, comparison-boot, rootfs, radio-kernel, and native-builder
  workflow tests passed. These tests do not execute the real native image
  tools or access a device.
- **Validation boundary:** workflow tests are synthetic host evidence. These
  corrected images have not been built with the real tools on the authorized
  native builder or booted on the Pixel. The current SSH attempt to the
  authorized builder failed with `Operation not permitted`; the last device
  discovery reported `No ADB or Fastboot device is visible.` No new physical
  capability or recovery gate is promoted.

## 2026-09-30 (source-owned split boot handoff)

- **Boot-input construction implemented:** `tools/pixel-9a/boot-inputs.sh` builds a
  source-owned initramfs from the native rootfs BusyBox, the Ferrix vendor
  ramdisk/bootconfig, and the reviewed source-built module roots. Its manifest
  records source hashes and keeps the handoff `bootable=false`; stock Android
  boot contents and legacy ad-hoc input variables are refused.
- **Split output handoff:** `tools/pixel-9a/radio-split.sh` consumes that
  manifest, generates the missing persistent split artifacts and fresh AVB
  metadata, and embeds relative kernel/rootfs/capacity/boot-input provenance
  so the Mac-side verifier is independent of native-builder paths.
- **Offline validation:** the boot-input, radio-split, radio flash, radio boot,
  rootfs, and radio-kernel workflow tests pass. No physical boot, flash, slot
  change, or Fastbootd transition was performed.
- **Still externally blocked:** the exact ten-partition capacity handoff still
  requires one owner-entered Fastbootd device, and the current Mac ADB daemon
  is unavailable. Recovery testing, complete native output construction,
  exact-device temporary boot, and persistent installation remain gated and
  unclaimed.

## 2026-09-30 (native Pixel 9a substrate completed)

- **Exact recovery input:** The official `tegu` factory package remains
  `CP3A.260905.009` with SHA-256
  `0c2f9053bf064b09483c937cf16f4824371afe92fa057029d6f9237668d1c0d7`.
  Firmware is still ignored/generated and was consumed only through its
  verified external manifest.
- **Native build host:** The exact-target product, radio-first kernel, and
  postmarketOS rootfs were built on the authorized native Linux x86_64 `deba`
  builder. No ADB, Fastboot, USB, temporary boot, or physical-device command
  was used by the build.
- **Radio-first kernel handoff:** The source-pinned Kbuild revision is
  `88335b765d920cc345cdffb9c1a6d77e3a531ff9`. The manifest SHA-256 is
  `87d7a398e47daffe6713d5d424bb4d5ec051d2fb462224d01a3c378887f2faab`;
  `Image` is 45,173,248 bytes with SHA-256
  `b6ccbdb8bc30854dce44c31c7182230a3dff7b2c3bad8f2a6da5bc99384bce75`, and
  `dtb.img` is 68,753 bytes with SHA-256
  `d248f800b76d55376021b1f1e73778b00dcae6722f3061d54b8f4dd768d0d9d0`.
  This remains `built-not-booted` / `bootable=false` and is not a complete
  split-boot handoff.
- **Product and rootfs handoff:** The product manifest SHA-256 is
  `eba8e1b7c829af9c3c1457bf865793ed06b345af2fdf97f37dc65e517d19908e`.
  The rootfs manifest SHA-256 is
  `3398822910ab9bbfa390d4beac3d51b0629824132a80c49538584c52b33530c5`;
  `rootfs.tar` is 611,676,160 bytes with SHA-256
  `fef00f97a511c752d6480fc46e4b3f7c9b48eff18f1876636782b14eee83a16c`, and
  the 2 GiB `rootfs.img` has SHA-256
  `56eeff4de8e5a90dae2169f4def73e9a4f904f06db583114b9dd8120ab8497c6`.
  Its manifest records `status=built-substrate-only`,
  `bootable=false`, pmaports `v26.06` at commit
  `368093c7a882637ee00d32932fb0dafd24cfc4d4`, and
  `initramfs_policy=external-kernel-owned-not-generated`. The archive was
  checked after build: `dev`, `proc`, and `sys` contain only empty mount-point
  directories.
- **S5300 package gate:** The target-local `exynos-modem`, `samsung-sit-ffi`,
  `sit-proxy`, and SIT ModemManager packages built from their pinned sources;
  the native package test suite, including `samsung-sit-protocol`, passed.
  This is build/protocol evidence only. There is still no physical boot,
  modem, SIM, bearer, Wi-Fi, Bluetooth, display, input, or capability
  evidence.
- **Remaining gates:** The complete partition-capacity handoff, tested
  recovery procedure, complete split-boot artifacts, exact-device temporary
  boot, and any persistent flash remain pending. The owner-declared
  dev-device/no-user-data policy is checked in, but no personal-data backup is
  claimed or recorded. QEMU remains the passing development/CI baseline; no
  QEMU result is promoted to the Pixel target.
- **Development transport entry:** Ordinary bootloader Fastboot can now be
  entered without hard buttons through the checked-in,
  confirmation-gated `just pixel-9a-enter-fastboot` workflow when exact-target
  ADB is available. Its read-only preflight has now run on the exact target;
  it never enters Fastbootd or changes storage, and no reboot mutation has
  been run through it yet.
- **Read-only transport refresh (exact target):** On 2026-09-30 the Mac saw
  exactly one Fastboot device for `google-pixel-9a-tegu-gxq96-mp1-0`:
  `tegu` / `MP1.0`, unlocked, secure, current slot `a`, two slots, and
  `is-userspace=no`. Fastboot reported `variant=N/A`; the checked-in matcher
  and radio workflows now accept that only when the same serial hash has a
  complete redacted ADB discovery proving Google / Pixel 9a / `tegu` /
  `GXQ96` / `MP1.0`. This is admission evidence only.
- **Recovery preflight (read-only):** On 2026-09-29T21:11:49Z the exact
  Fastboot device passed `just pixel-9a-factory-restore --preflight`. The
  pinned factory and OTA archives matched their SHA-256/size pins; the
  offline factory inventory required board `tegu`, bootloader
  `tegu-17.0-15819938`, and baseband
  `g5300t-260604-260807-B-16036236`; and the reviewed vendor sequence matched.
  This remains `recovery_procedure=documented-not-tested` and
  `image_compatibility=partial`: no factory restore, stock recovery boot, or
  Ferrix mutation was run.
- **Fastboot inventory refresh (read-only):** At 2026-09-29T21:12:06Z the
  exact target's ordinary bootloader Fastboot `getvar all` inventory completed
  with the same `tegu` / `MP1.0` / two-slot identity and included `userdata`
  capacity. The Android-side inventory remains `partial` because Android
  denied the broader `/proc` reads; this does not satisfy the separate
  Fastbootd capacity handoff gate.
- **Capacity gate:** The real read-only capacity workflow refused ordinary
  bootloader Fastboot with `Fastboot capacity handoff requires fastbootd
  (is-userspace=yes) ...; no reboot was issued`. An owner-entered Fastbootd
  session is still required; no raw Fastboot transition was attempted.
- **Development-device data policy:** The checked-in target manifest now uses
  `backup = "not-required-dev-device"` plus
  `data_policy = "dev-device-no-user-data"` for the owner-declared new
  development device. This never claims a verified backup; every mutating
  workflow still requires the explicit session opt-in and per-run confirmation.
- **Inactive-slot boot workflow:** `just pixel-9a-boot-inactive` is now a
  checked-in, stub-tested route for selecting and rebooting an already-flashed
  inactive radio-first slot. It re-hashes the split manifest and both recovery
  archives, verifies temporary-boot and persistent-flash evidence from the
  same device, rechecks identity after `set_active`, and never accepts a raw
  slot-changing command. It has not run on hardware.

## 2026-09-29 (factory-version reconciliation and native-build handoff)

- **Factory version:** Google's current official `tegu` factory-image table
  lists `17.0.0 (CP3A.260905.009, Sep 2026)` with SHA-256
  `0c2f9053bf064b09483c937cf16f4824371afe92fa057029d6f9237668d1c0d7`.
  That value matches the exact locally staged archive recorded in
  `recovery.pins`; no older factory build was substituted.
- **Firmware/product stages:** The authorized native Linux builder completed
  the exact-target firmware stage from the verified factory archive and the
  separate Ferrix product build. Firmware remains ignored/generated output;
  the product manifest is `status=built` / `bootable=false`.
- **Route reconciliation:** The A3 BCM4383 source mapping changed the checked-
  in `firmware-radio-route.tsv` hash to
  `3a4f62ec18691401723e434c159abdd6c85eb9a4bb6f9b9f50107fc6bbb3f5e3`.
  Commit `ebe2319` updates `kernel-radio-route.toml` to that hash. The prior
  radio-kernel manifest is therefore not reused; a corrected native Kbuild
  handoff must be inspected before rootfs construction.
- **Current boundary:** The corrected radio-kernel build was launched on the
  native `x86_64` builder, but the build host is temporarily unreachable from
  this session (`deba` DNS/Tailscale transport failure). Its completion is
  intentionally unclaimed, and the radio-first rootfs remains unbuilt. No
  temporary boot, persistent flash, or physical radio evidence exists.
- **Local validation:** The target-scoped environment, kernel-route, firmware,
  rootfs, and radio safety workflows pass after the reconciliation. These are
  source/workflow tests, not physical-device capability evidence.

## 2026-09-29 (native remote builder handoff and radio-first flash route)

- **Build host:** The supplied `ezy@deba` endpoint is a native Linux
  `x86_64` host (`Linux 7.2.5-3-omarchy`) with roughly 893 GiB free under
  `/home`. The repository is cloned at
  `/home/ezy/github-projects/omarchy-ferrix`. This satisfies the physical
  architecture and disk-space gate; it is distinct from the rejected
  Apple-Silicon OrbStack/UTM emulation path. Docker access is now enabled for
  `ezy`; the pinned target builder image
  `ferrix-pixel9a-builder:rust-1.98.1` was built on the native engine as
  `linux/amd64` (`sha256:a12eaf304773edb45590e7a0a764ca3b6465649bacd10aeadde136de098fd7c2`).
  Its device-free `env check build`, radio-kernel, product, and rootfs
  preflights pass.
- **Authorized native build evidence:** The radio-first source archive was
  synced and hash-verified on this host at revision
  `88335b765d920cc345cdffb9c1a6d77e3a531ff9`. The authorized Pixel 9a
  product build completed with manifest SHA-256
  `eba8e1b7c829af9c3c1457bf865793ed06b345af2fdf97f37dc65e517d19908e`; its
  manifest remains `status=built` / `bootable=false`. The authorized
  radio-first kernel build completed with manifest SHA-256
  `fd3e0f5cc894a18a1c9d5eaebcd8ff315401902a34d8374497b603064d343da4` and
  remains `status=built-not-booted` / `bootable=false`. Its `Image` SHA-256 is
  `41eff637278cd66d75499e1aa1cd0143ab4fe786165b1347e8e83c187e0c5a1c`, and
  its `dtb.img` SHA-256 is
  `d248f800b76d55376021b1f1e73778b00dcae6722f3061d54b8f4dd768d0d9d0`.
- **Build-stage boundary:** Firmware remains unstaged and untransferred;
  the radio-first rootfs remains `status=not-built`; the complete radio split
  handoff and boot artifacts remain unbuilt; and no temporary boot, persistent
  flash, or physical Ferrix Linux session has been attempted. These native
  build manifests are prerequisites only and provide no radio-runtime or
  capability evidence.
- **Radio-first physical adapter:** `flash-radio.sh` and its strict offline
  test now provide the separate physical route for the
  `linux-google-tegu-wwan` split manifest. It validates the route/contract
  hashes, source-built artifact origins and fresh AVB binding rows, exact
  target identity, recovery and backup gates, inactive-slot selection,
  fastbootd handling, temporary-boot evidence, serial-redacted transcripts,
  and typed confirmations. The canonical Google Kleaf `flash.sh` route still
  refuses radio-first manifests. `just pixel-9a-flash-radio-test` passes; no
  physical boot or flash has been attempted.
- **Phone state:** The checked-in read-only discovery now sees exactly one
  Pixel 9a (`tegu` / `GXQ96` / `MP1.0`) over Mac ADB, unlocked in slot A with
  dynamic partitions. This remains admission evidence only: Ferrix Linux and
  all-radio support are unproven, the complete capacity handoff is still
  outstanding, and no physical boot or flash has been attempted.

## 2026-09-28 (latest read-only transport retry)

- **Pixel transport:** `just pixel-9a-tools-check` found ADB 37.0.1 and
  Fastboot 37.0.1. A fresh `just pixel-9a-discover` attempt remained blocked
  before enumeration: macOS ADB could not create its USB interface plug-in
  (`e00002be`) and could not start the daemon (`Operation not permitted`).
  The checked-in workflow recorded `adb_devices=0`, `fastboot_devices=0`,
  `result=no-device`, and issued no device mutation. The prior redacted exact
  target admission/inventory transcript remains the authoritative identity
  evidence until transport is restored.
- **Rust validation:** `cargo check --workspace --all-targets`, workspace
  Clippy with `-D warnings`, and `cargo test -p ferrix-target-pixel9a` passed
  on the host. These are host/static checks only and do not promote any Pixel
  Linux or radio capability.
- **Capacity handoff:** Added the read-only
  `just pixel-9a-partition-capacity` workflow and stub test. With ADB it can
  collect all ten exact-target static/dynamic byte capacities from Android
  sysfs; with exactly one owner-entered Fastbootd device it can instead read
  independent A/B `partition-size:*` getvars. Both paths retain
  `inventory_completeness=partial`; neither was run on hardware in this
  session because transport failed before enumeration.

## 2026-09-28 (native builder tool gate)

- **Handoff completeness:** The authoritative Pixel build profile now requires
  `mkdtboimg.py` and `cpio`, which are needed by the source-built DTBO and
  ramdisk stages but are easy to omit from an otherwise complete kernel/product
  tool installation. `just pixel-9a-env-test` and
  `just pixel-9a-env-validate` pass; the Darwin/arm64 host still refuses the
  physical build before tool probing.

## 2026-09-28 (OrbStack local container runtime)

- **Local container runtime:** macOS is configured to use OrbStack. Its
  Docker-compatible `docker` CLI remains the intentional command surface for
  QEMU and postmarketOS image builds; the selected macOS context is
  `orbstack`, but the daemon was stopped/unreachable during this validation.
  A non-destructive build-session start attempt (`orbctl start`) did not bring the
  VM online; `orbctl status` remained `Stopped` and the Docker socket stayed
  inaccessible. No image or container state was changed by the repository
  workflows.
  Linux and CI continue to use a Docker-compatible engine without changing
  their evidence boundaries.

## 2026-09-28 (QEMU/artifact validation continuation)

- **QEMU contract:** `just qemu-scenario-validate` passed all 25 checked-in
  mock telephony/Wi-Fi scenarios. `just qemu-check` remains unclaimable in
  this workspace because the generated guest artifacts are absent; the
  subsequent `just qemu-build` could not reach the running container-engine
  socket from this restricted host session. This does not change the physical
  Pixel claim boundary.
- **Artifact gate:** `tools/pixel-9a/artifacts.sh` now rejects unsafe artifact
  paths and requires exact Pixel 9a identity, Google Kleaf kernel metadata,
  postmarketOS rootfs metadata, header-v4 boot metadata, and an explicit AVB
  mode before writing the final artifact manifest. The offline artifact test
  covers a hash-valid traversal path and passes.
- **Firmware gate:** the Pixel radio init script now validates every manifest
  destination against the reviewed firmware allowlist before checking payload
  hashes, including descendants of the explicitly allowlisted modem config
  directories. An unreviewed firmware row is rejected by the runtime fixture;
  no new firmware payload was added.
- **Radio split handoff:** `pixel-9a-radio-split-check/build` now provides a
  device-free composer for the missing source-built `dtbo`,
  `vendor_kernel_boot`, `vendor_dlkm`, `system_dlkm`, and split AVB handoff
  artifacts. It requires all ten exact-target, redacted partition capacities
  and hash-verified source inputs. After the final images are composed, native
  `avbtool add_hash_footer` generates salted descriptors from those exact bytes
  and `make_vbmeta_image --include_descriptors_from_image` creates fresh
  `vbmeta`, `vbmeta_vendor`, and `vbmeta_system`; external or stock AVB input
  manifests are refused. The offline workflow test covers the fresh-generation
  calls and binding rows. It keeps the result `composed-not-booted` /
  `bootable=false`; no physical flash route consumes it.
- **OrbStack candidate:** OrbStack 2.2.3 is installed, but its service start
  timed out in this session and the Docker socket is not usable from the
  restricted host shell. Its Apple-Silicon x86 path is emulation rather than a
  native Linux `x86_64` host (see the [OrbStack architecture
  notes](https://docs.orbstack.dev/architecture)), so it remains unsuitable
  for the physical Pixel build gate. It may still be used for generic
  QEMU/container work once its service is available.

## 2026-09-28 (serial transport and build-host audit)

- **USB/ADB:** A serial `just pixel-9a-discover` retry completed at
  `2026-09-28T06:46:24Z`; a separate `ADB_LIBUSB=0 just pixel-9a-discover`
  retry completed at `2026-09-28T06:46:43Z`. Both failed before enumeration
  with the macOS ADB 37.0.1 USB plug-in error `e00002be` and recorded
  `adb_devices=0`, `fastboot_devices=0`, and `result=no-device`. macOS I/O
  Registry still sees a Google `Pixel 9a` USB product, so the failure is the
  host ADB USB interface/permission boundary rather than an identity result.
  Both workflows were read-only and issued no device mutation.
- **Build host:** `uname` reports `Darwin/arm64`; `just pixel-9a-env-check`
  refuses the physical build as required. No local native Linux `x86_64`
  builder, UTM x86_64 guest, OrbStack/Lima builder, or running Docker engine
  is available. The next valid build action remains handoff to a provisioned
  native Linux `x86_64` host; emulation and container substitutes remain
  disallowed by the physical-target contract.

## 2026-09-28 (latest read-only Pixel transport retry)

- **ADB transport:** `just pixel-9a-discover` was retried at
  `2026-09-28T06:40:32Z`. The macOS ADB daemon still cannot create its USB
  interface plugin (`e00002be`) and exits before device enumeration. The
  checked-in workflow recorded `adb_devices=0`, `fastboot_devices=0`, and
  `result=no-device`; it issued no device mutation. Earlier exact-target
  admission evidence remains the authoritative identity record until USB
  transport is restored. A legacy-path retry with `ADB_LIBUSB=0` at
  `2026-09-28T06:43:51Z` failed with the same plugin/daemon error.

## 2026-09-28 (fresh Pixel 9a admission recheck)

- **Read-only identity:** `just pixel-9a-discover` re-observed one exact Google
  Pixel 9a (`tegu` / `GXQ96` / `MP1.0`) over ADB after unlock. The current
  Android state is `orange` verified boot, `unlocked`, slot `_a`, and dynamic
  partitions enabled; no storage, slot, boot, or firmware mutation was issued.
- **Evidence timestamps:** The latest discovery completed at
  `2026-09-28T05:57:17Z`; the partition inventory completed at
  `2026-09-28T05:57:22Z`. Both transcripts remain serial-redacted and ignored
  under `out/pixel-9a/`.
- **Partition topology:** `just pixel-9a-partition-inventory` re-observed both
  A/B by-name suffixes and recorded `slot_count_source=block-by-name`. Android
  still denies `/proc/partitions` and `/proc/cmdline`, so the ignored inventory
  remains `inventory_completeness=partial` and cannot authorize a rootfs flash.
  This is admission/recovery evidence only; no Ferrix Linux or radio capability
  claim changes.
- **A/B modem safety:** The target modem mount service now resolves the exact
  `modem_a`/`modem_b` label from the bootloader's single
  `androidboot.slot_suffix` value and refuses a missing, duplicate, or unknown
  slot. This keeps an inactive-slot Ferrix boot paired with its matching modem
  partition instead of silently using `_a` calibration data.
- **Radio split handoff:** The source-only radio kernel workflow now records
  hash-verified DTB/DTBO source inputs, `modules_install` output, and explicit
  `status=not-built` partition-size rows. Boot/rootfs validators consume those
  rows and fail closed; no stock modules, Android userspace, or fabricated
  split images were introduced.
- **Associated-radio boundary:** The target-local audit now records the
  candidate `samsung,kepler` GNSS binding as a raw `/dev/gnss*` kernel
  surface only. The radio-first initfs rejects unreviewed `kepler`, `st21nfc`,
  `st54spi`, and `bcm47765` module names; NFC/ST54 and UWB remain unselected
  until a native Linux binding and runtime evidence exist. No location/NFC
  product API or physical capability claim was added.
- **Latest transport check:** A subsequent read-only `just pixel-9a-discover`
  attempt at `2026-09-28T06:26:25Z` could not start the macOS ADB daemon
  (`Unable to create an interface plug-in (e00002be)` / `Operation not
  permitted`). It issued no device mutation and does not invalidate the
  earlier exact-target admission transcript; a native Linux build and
  physical Ferrix boot remain outstanding.

## 2026-09-28 (QEMU Android Pixel-class display density)

- **Target-local display profile:** The opt-in `FERRIX_PMOS_ANDROID=1` QEMU
  image now sets `ro.sf.lcd_density=400`, giving the 1080-pixel panel an
  approximately 432 dp logical width with a slightly less zoomed-in
  Pixel-class Android composition.
  The property is emitted only inside the QEMU Android finalize branch and is
  recorded as `guest.android.density_dpi`; it is absent from the default pmOS
  image and physical-target workflows.
- **Why:** The previous image was physically portrait but exposed a very wide
  logical viewport, which made Google Images choose a four-column layout. The
  target keeps its 1080×2340 physical geometry for Ferrix and changes only the
  Android logical density.
- **Validation:** `just android-bootstrap-test` covers the density property in
  the materialized Waydroid properties; the Android image was rebuilt and its
  manifest records `density_dpi: 400`. A fresh HVF graphical boot also
  emitted QEMU records for applying and materializing `density_dpi: 400` and
  produced an inspected 830×1800 framebuffer capture at
  `out/qemu-aarch64/pmos-android/verification-density-400.png`. The setting
  is QEMU-only evidence, not a Pixel hardware claim.

## 2026-09-28 (Browser tile and Android onboarding input)

- **Launcher identity:** The `org.ferrix.browser` home tile now presents as
  `Browser` with a `B` glyph. The runtime-private Android package remains the
  pinned `net.slions.fulguris.full.fdroid`; the product label does not expose
  that implementation name.
- **Onboarding input:** The compositor's Home-start zone is now only the
  bottom 3% of the output (`START_ZONE = 0.97`), keeping the Android browser's
  onboarding arrow above the system navigation strip available to the app. A
  regression test covers both sides of that boundary.
- **Graphical evidence (QEMU, pmOS Android, 1080×2340, HVF):** A fresh
  Android-enabled image was built and booted with the Fulguris APK installed.
  Inspected framebuffer captures show the home tile as `Browser` and the real
  Fulguris onboarding surface with its arrow above the navigation bar. The
  ignored captures are `out/qemu-aarch64/pmos-android/verification-home-browser.png`
  and `out/qemu-aarch64/pmos-android/verification-fulguris-onboarding.png`;
  the matching serial log is `out/qemu-aarch64/pmos-android/verification-serial.log`.
  The automated QMP tap path moved the pointer but did not produce a reliable
  Android button activation, so the next onboarding page is not claimed as
  independently captured here; the input boundary is covered by the
  compositor regression test.
- **Validation:** Affected Rust tests, `just qemu-build`,
  `APKSIGNER=/Library/Android/sdk/build-tools/36.0.0/apksigner just
  pmos-android-build`, `just pmos-android-check`, and `just qemu-check` pass.
  The Android check is headless TCG evidence and does not broaden the
  graphical or physical-device claim.

## 2026-09-28 (Fulguris launcher identity)

- **Launcher:** The stable `org.ferrix.browser` home tile now displays
  `Fulguris` with a distinct `F` glyph and blue descriptor tone. Its launch
  package remains the pinned `net.slions.fulguris.full.fdroid` replacement;
  no active app, runtime, or QEMU target source references the retired browser.
- **Mock graphical evidence:** The 1080×2340 `ferrix-basic-screen` launcher
  preview was rendered and inspected with the Fulguris tile visible. `just
  qemu-build` regenerated the product descriptor with the same metadata. The
  headless TCG `just qemu-check` now passes with the smoke runner's
  `FERRIX_QEMU_MEMORY=2048` MiB default; this remains emulator/mock evidence,
  not Pixel hardware evidence.

## 2026-09-28 (F-Droid Fulguris browser input)

- **Browser replacement:** The Android browser descriptor remains
  `org.ferrix.browser`, but now names Fulguris Web Browser and package
  `net.slions.fulguris.full.fdroid`. The target-scoped Waydroid bootstrap,
  runtime fixtures, product UI fallback, and QEMU staging path use Fulguris;
  `ferrix-appd` remains launch-only.
- **Pinned APK:** `tools/android/fulguris.pins` selects F-Droid version
  `2.0.10` / build `261`, asset
  `net.slions.fulguris.full.fdroid_261.apk`, 9,310,262 bytes, SHA-256
  `cc151e4608d68fa32aba0b5b80c9c89a286050d7b0710a081e77bc1dddbcc8b6`, and
  signer fingerprint
  `66958f3366388c23ca1a3cbfd91545cca4abf3e79edaab4fe988213e392b07a1`.
  The source is the [F-Droid Fulguris package page](https://f-droid.org/en/packages/net.slions.fulguris.full.fdroid/)
  and its direct APK link; the APK remains ignored under `out/android/`.
- **Claim boundary:** No Fulguris graphical or physical-device evidence has
  been collected. The dated Brave QEMU/framebuffer entries below remain
  historical and are explicitly not Fulguris validation.
- **Validation:** The Android pin and bootstrap fixtures pass (19/19 and
  2/2), workspace formatting, Clippy, and tests pass, and the fresh QEMU
  build plus headless `just qemu-check` pass. That QEMU check covers the
  emulator's mocked core/service readiness and power transition; it does not
  claim Fulguris graphical rendering or touch behavior.
## 2026-09-28 (mobile-device-provisioning skill and gated Pixel 9a provisioning workflows)

- **Skill:** `skills/mobile-device-provisioning/` is an LLM-agnostic playbook
  (open Agent Skills layout: `SKILL.md` plus `references/`) that takes any
  attached Android phone from ADB/Fastboot identification through recovery
  images, backup and unlock, stock recovery, Ferrix image build, gated flash,
  and radio evidence. It encodes the owner's 2026-09-28 decisions: generic
  read-only discovery and proposal scaffolding for any device, mutation only
  through checked-in target-scoped workflows, and per-run human confirmation
  in chat before an agent types any confirmation phrase. `AGENTS.md` points
  to it.
- **Generic device front door:** `tools/device/{discover,match-target,
  propose-board,recovery-download}.sh` (`just device-discover`, `device-match`,
  `device-propose`, `device-recovery-download`) are vendor-agnostic and
  read-only against the device. Discovery uses only `adb devices/getprop` and
  `fastboot devices/getvar`, refuses zero or multiple devices, and writes a
  serial-redacted transcript plus `out/device/identity-latest.env`. Matching
  requires manufacturer, codename, and hardware revision (and SKU when both
  sides have it) and reports same-codename/different-revision as `none` with
  an explicit do-not-reuse note. Proposal scaffolding writes an unapproved
  board package and proposal document with every required section and never
  writes a serial. `just device-tools-test` passes 175 offline stub checks
  (synthetic non-Google fixtures prove vendor independence).
- **Pixel 9a stock recovery workflows (not device-tested):**
  `tools/pixel-9a/sideload-ota.sh` and `tools/pixel-9a/factory-restore.sh`
  implement the gated full-OTA sideload and Fastboot factory restore with
  exact-identity, pinned-hash, single-device, vendor-sequence, and typed
  per-run confirmation gates and serial-redacted transcripts. The restore
  path verifies the archive's own `flash-all.sh` against the reviewed
  sequence and re-checks bootloader/baseband versions between stages. Offline
  stub tests (49 and 69 checks, now part of `just pixel-9a-tools-test`) prove
  the exact mutation order, refusal paths, and redaction. Neither workflow has
  been executed against the physical device.
- **Pixel 9a flash workflow (not device-tested):** `tools/pixel-9a/flash.sh`
  adds `preflight`, `boot-temp` (`fastboot boot` of the manifest `boot.img`),
  and `flash` (slot-explicit writes of boot, init_boot, vendor_boot,
  vendor_kernel_boot, dtbo, and vbmeta images, then vendor_dlkm/system_dlkm
  from fastbootd). Every mode re-hashes all composed artifacts, verifies the
  pinned factory archive, requires exactly one Fastboot device reporting
  `product=tegu`, `hw-revision=MP1.0`, `unlocked=yes`, and prints serial,
  slot, partition targets, and input hashes before each announced mutation.
  Persistent flashing requires a matching `boot-temp` evidence transcript
  (image-layout policy) and a typed per-stage phrase; the rootfs stage now
  accepts the generated ext4 `rootfs.img` only after the target package
  explicitly selects `userdata` and the operator supplies a matching,
  redacted read-only ADB partition inventory proving the exact target,
  revision, SKU, slot state, and live `userdata` mapping. The checked-in
  target intentionally leaves that partition selection unset, so no rootfs
  flash is currently authorized.
  `just pixel-9a-flash-test` passes its offline checks; no Ferrix boot or flash
  has occurred. `boot.sh` now records final artifact paths in
  `boot-manifest.tsv` instead of the staging directory that is renamed away.
- **Pixel 9a radio workflow (not device-tested):** `tools/pixel-9a/radios.sh`
  connects over the postmarketOS USB network/SSH to a guest already running
  Ferrix Linux, refuses any guest whose device tree is not the pinned
  `google,ZUMA PRO TEGU` / `ZUMA PRO TEGU MP on ZUMA PRO` identity
  (QEMU `linux,dummy-virt` is refused explicitly), inventories rfkill,
  NetworkManager, BlueZ, ModemManager, firmware presence, and the
  `ferrix-core`/`ferrix-networkd`/`ferrix-telephonyd` adapter records and
  sockets, gates radio enablement behind one typed confirmation, joins Wi-Fi
  with the PSK fed over stdin, and writes redacted transcripts plus
  `evidence-latest.tsv` (`status` is only `supported|unsupported`, matching
  `CapabilityStatus`; nuance is in a `progress` column). `just
  pixel-9a-radios-test` passes its offline checks, including degraded-state
  refusal coverage. No Wi-Fi, Bluetooth, or
  modem has been observed working on the device; `capabilities.toml` is
  unchanged and the `evidence` command only prints a fragment for the owner.
- **Associated radios and cellular data boundary:** `just
  pixel-9a-radios-associated` now performs a focused, read-only exact-target
  probe for GNSS/KEPLER, NFC/ST21, ST54, and UWB and writes a separate
  `associated-evidence-latest.tsv`; all four remain `unsupported` until a
  native binding and Ferrix adapter are evidenced. Cellular data evidence now
  records the typed `connect-data`/`disconnect-data` and `telephony-data`
  grant boundary, APN fail-closed policy, and bearer/`rmnet*` requirements;
  SSH cannot bypass appd authorization.
- **Flash-slot safety resolved:** persistent `flash` now requires an explicit
  inactive `--slot a|b` and refuses both an omitted slot and the active slot;
  `preflight` may still omit it for read-only inspection. The workflow never
  issues `set_active`, so the first artifact set remains safely inactive until
  a separately approved slot change.
- **Validation:** `just device-tools-test`, `just pixel-9a-tools-test`,
  `just pixel-9a-firmware-test`, `just pixel-9a-flash-test`,
  `just pixel-9a-radios-test` (including the focused associated-radio and
  cellular-data boundary checks), `just pixel-9a-kernel-radio-test`,
  `just pixel-9a-boot-radio-test`, and `just pixel-9a-boot-test` pass on macOS
  with strict `adb`/`fastboot`/`ssh` stubs;
  `tests/inspect-factory.sh` no longer needs `rg`. No device-facing command
  reached a phone during these offline tests; the previously recorded
  physical discovery/unlock evidence is unchanged. Evidence from this
  validation remains offline-test evidence only; no physical Linux or radio
  claim changes.
- **Live read-only inventory:** With one exact Pixel 9a attached, `just
  pixel-9a-partition-inventory` passed on 2026-09-28 and matched `tegu` /
  `GXQ96` / `MP1.0`, slot `_a`, and the A/B `/dev/block/by-name` topology.
  Android returned an empty `ro.boot.slot_count`, so the workflow recorded
  `slot_count_source=block-by-name` only after observing both slot suffixes.
  Android denied `/proc/partitions` and `/proc/cmdline`; the ignored,
  serial-redacted transcript is exact-target/topology admission evidence, not
  proof of a Ferrix Linux boot or radio functionality.

## 2026-09-28 (Pixel 9a native-Linux package boundary)

- **Target-local package:** The checked-in
  `targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/` package now includes the
  native-Linux postmarketOS package at `pmos/device-google-tegu/`, the typed
  target composition crate at `adapter/` (`ferrix-target-pixel9a`), the target
  manifest and unsupported capability matrix, the pinned downstream-kernel and
  image-layout records, and the exact-source firmware policy. These are
  implementation inputs for `tegu` / `GXQ96` / `MP1.0`, not a bootable image
  or a support claim. The package's `deviceinfo` deliberately sets
  `deviceinfo_flash_method="none"`; its boot layout remains
  `substrate-only-not-bootable`.
- **Adapter composition:** Session/bootstrap selects the target-local seam and
  gives product code only a target-neutral `HardwareBundle`. On Linux, the
  composition maps UPower to power, BlueZ to Bluetooth, WirePlumber to audio,
  NetworkManager to network, and ModemManager to the existing telephony
  contract, with local DRM/backlight, input/IIO-presence, and the Linux
  `/sys/power/state` suspend ABI for display, input, and lifecycle. Connection
  first verifies that the running device tree identifies the pinned
  `google,ZUMA PRO TEGU` board model, which
  prevents accidental composition on QEMU or another board. `ferrix-target-pixel9a` returns an
  all-`unsupported` capability matrix; composing or compiling these adapters
  is not physical hardware evidence.
- **pmOS service dependencies:** The target package now pulls the Alpine
  OpenRC companions for D-Bus, udev, NetworkManager, ModemManager, BlueZ, and
  seatd, plus `pipewire-spa-bluez` for Bluetooth audio. UPower remains
  D-Bus-activated and is not incorrectly linked into an OpenRC runlevel; the
  rootfs workflow has regression assertions for this wiring.
- **Native Pixel build boundary:** The physical product handoff now builds
  directly through Cargo on a provisioned native Linux x86_64 host, with
  target-side libudev/libseat/DRM inputs and production
  NetworkManager/ModemManager features. The physical-device playbook rejects
  macOS, containerized builders, and cross-build substitutes for product,
  kernel, firmware, rootfs, boot, and artifact stages; product, kernel, and
  rootfs caches and outputs remain distinct.
  The physical product profile targets `aarch64-unknown-linux-musl`; its
  preflight now requires the pinned Rust target standard library and an
  explicit target linker (`aarch64-linux-musl-gcc` by default), without
  installing either host tool automatically.
- **Pinned kernel configuration contract:** The board package now preserves
  Google's exact `tegu_defconfig` as `kernel-config.tegu`. Kernel source
  verification compares it byte-for-byte with the pinned checkout before
  Kleaf builds, and the kernel manifest records its SHA-256. The fragment
  explicitly enables the S5300 CPIF S5100/PCIe/shared-memory/packet-processing
  path and CP thermal support; it is a native Linux kernel input, not Android
  userspace and not physical evidence.
- **Build provenance corrections:** The kernel workflow now resolves the
  BCM4383 checkout at the exact manifest path
  `private/google-modules/wlan/bcm4383` (distinct from its upstream URL), and
  the composed artifact manifest preserves path, byte count, and SHA-256 for
  every layout/kernel/rootfs input. Focused kernel, artifact, and rootfs
  workflow tests cover both contracts; rootfs validation also re-hashes the
  staged product profile, binaries, colors, and app descriptors before any
  image is made.
- **Native-radio research:** An external `linux-google-tegu` branch provides a
  plausible non-Android S5300/BCM4383/BT kernel direction. The target now has
  a separate, source-pinned radio-first Kbuild workflow
  (`kernel-radio-route.toml`, `kernel-radio.sh`) plus explicit
  `bcmdhd`→`brcmfmac4383-pcie.*` firmware copy/rename rows. Its paired
  userspace loader extracts the CP BOOT/PBL section from the read-only
  `modem_a` `modem.bin` and builds the handover block from runtime device
  inputs; Ferrix deliberately does not invent or stage `cp_pbl.bin`/
  `cp_handover.bin` aliases. It still requires the exact read-only modem/EFS/
  persist payload contract and a new SIT/GEMS userspace stack; the route is
  `experimental-not-adopted`, not built here, and supplies no physical
  evidence. Radio sync now verifies the pinned archive byte count and
  SHA-256/SHA-512 before the clean detached checkout is accepted. The
  canonical Google Kleaf/CPIF path and the radio-first WWAN/SIT path remain
  deliberately incompatible.
- **Rootfs and split boot:** The device-free `pixel-9a-rootfs-*` workflow
  validates the target package, a pinned clean pmaports checkout, the
  separately built radio-first Kbuild handoff, the native-Linux product
  handoff, and an external canonical source/path provenance manifest for
  firmware before a native-Linux pmbootstrap build. `pixel-9a-product-*` uses
  its own `out/pixel-9a/product/tegu` and Cargo target/cache roots and enables
  the production NetworkManager and ModemManager features; it never reuses the
  QEMU product. Rootfs stages the product binaries/descriptors, target profile,
  service wiring, canonical firmware, and explicit radio firmware aliases into
  deterministic `rootfs.tar`/`rootfs.img` artifacts whose manifest remains
  `built-substrate-only` / `bootable=false`. The canonical
  `pixel-9a-boot-*` workflow remains a Google Kleaf comparison route and
  rejects this radio-first rootfs; `pixel-9a-boot-radio-*` composes the
  independent Image/DTB/boot handoff but currently omits persistent
  `vendor_kernel_boot`, `dtbo`, `vendor_dlkm`, and `system_dlkm` images, so it
  remains `composed-not-booted` / `bootable=false` and is not flashable.
  Neither workflow calls ADB/Fastboot or boots, flashes, or changes a device.
- **Firmware policy:** Firmware is accepted only from the exact hash-verified
  official Google factory archive named by `recovery.pins`; bytes remain local
  and ignored. Inventory is offline, and explicit staging requires a reviewed
  allowlist plus an external `firmware-manifest.tsv`; broad Android `/vendor`
  copying is prohibited. The checked-in required allowlist names the five
  BCM4383 Wi-Fi files, the exact factory-selected Broadcom `BCM.hcd` path, and
  the `carrierconfig`/`uecapconfig` directory bundles required by the native
  S5300 loader. Bundle extraction expands only regular files into
  destination-aware rows and rejects symlinks, special entries, unsafe names,
  empty bundles, and destination overwrite attempts.
  The target now records the exact pinned Kleaf module basenames
  (`bcmdhd4383`, `nitrous`, and `cpif`) and the BCM4383 request/path contract
  in `wifi-firmware-source-evidence.md`; malformed or duplicate staging rows
  fail before archive extraction. The guest radio inventory now re-hashes every
  manifest row and refuses network promotion on a missing, symlinked, size-,
  or SHA-256-mismatched file. The canonical `bcmdhd` contract and the
  radio-first `brcmfmac` contract are not interchangeable; the latter is
  admitted only through the explicit hash-pinned alias manifest. Bluetooth
  provenance is now source- and hash-documented,
  but the Linux HCI/Nitrous integration and physical controller result remain
  unverified. The target-local package's source-pinned `exynos-modem` and
  Samsung-SIT ModemManager recipes now provide the native Linux userspace
  boundary, read-only modem/efs/persist mounts, SIT proxy, and OpenRC ordering;
  they still require a native Linux build and exact-device runtime evidence.
  S5300 cellular and display/audio controller families remain policy entries
  without a complete physical capability result.
- **Bounded S5300 slice:** `crates/ferrix-s5300-cpif` is a source-pinned,
  target-independent SIPC5/SIT link-frame codec and channel metadata
  boundary. The exact tegu DTS selects Exynos SIT; the crate covers its
  bounded 12-byte header/fragment codec alongside the generic SIPC5
  ordinary, multi-frame, and extended-length layouts, plus bounded parsing,
  padding, reserved-channel checks, and a bounded payload-only transport for
  an explicitly supplied IPC character node. It does not
  implement CPIF rings/doorbells, device-tree channel binding, device
  discovery, modem commands, SIM/SMS/data/voice/IMS, or modem protocol
  mapping. It now also bounds the seven-byte userspace FMT envelope and can
  wrap an explicitly supplied payload transport, while preserving the raw
  command tuple without interpreting it. The Pixel adapter does not wire this
  crate: its telephony slot is still ModemManager. A real S5300 userspace
  backend or ModemManager plugin, exact
  `tegu` DT/channel evidence, firmware, and protocol integration remain
  outstanding. The target-local package's `s5300-channels.toml` now records the
  complete source-derived I/O-device catalog (including 30 symbolic `rmnet`
  channels and eight `oem_ipc` expansions), exact SIT numeric channel values,
  plus the exact `umts_ipc0`/`umts_ipc1` names and indexed FMT binding, but is
  not runtime evidence. The crate's
  read-only `discover_at` status seam observes only the exact platform
  compatible, driver binding, and kernel `modem_state`; `ONLINE` is not treated
  as telephony support.
- **Android modem boundary:** Google's public `gs-common` `modem_svc_sit`
  metadata names a vendor `shared_modem_platform` Binder service with RIL,
  radio-device, firmware, and modem-userdata access, but publishes neither
  its implementation nor a SIT/GEMS command ABI. Ferrix records this in
  `targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/s5300-source-evidence.md`
  and deliberately does not import that Android service, RIL, Binder contract,
  or policy into the native Linux guest.
- **ModemManager cellular data:** The production adapter reads standard
  `Modem.Simple`/`Bearer` D-Bus objects and folds bearer lifecycle plus the
  kernel-facing interface name into `DataBearer` snapshots. The privileged
  telephony service now exposes target-neutral `connect-data` and
  `disconnect-data` operations under the separate `telephony-data` grant;
  `Modem.Simple.Connect`/`Disconnect` are used when that interface exists.
  This still does not make the S5300 CPIF path a ModemManager modem, and no
  physical-device evidence is implied by the contract or adapter tests. The
  pinned Samsung-SIT plugin requires a carrier-specific APN; Settings now
  accepts the non-secret `FERRIX_TELEPHONY_APN` runtime value from the target
  environment and sends it only in the typed connect request. The privileged
  ModemManager adapter also accepts that value as a daemon-only fallback when
  a client submits the default profile; explicit APNs win and malformed
  configured values fail closed. The checked-in target leaves it empty rather
  than guessing a carrier profile.
- **Radio inventory boundary:** The target-scoped SSH inventory now requires
  the exact `ZUMA PRO TEGU MP on ZUMA PRO` model (after trimming device-tree
  NUL padding) and records a read-only CPIF surface probe for the pinned
  `samsung,exynos-cp` platform, driver binding, kernel `modem_state`,
  `umts_ipc0`/`umts_ipc1`, and `rmnet*` nodes. This only makes the next
  cellular gate observable; it does not open CPIF character devices, create a
  modem backend, or promote cellular support.
- **Radio evidence promotion:** Wi-Fi is promoted only when NetworkManager
  reports full connectivity, the exact Wi-Fi device is associated with an IPv4
  address, and both the Ferrix network service and core return successful typed
  state replies. Bluetooth likewise requires a powered BlueZ controller and an
  explicit `present=true, enabled=true` state from the Ferrix core socket.
  Daemon startup, an error envelope, or a disconnected core state can no longer produce a false
  `supported` row; the offline workflow test covers the degraded case.
- **Tegu Wi-Fi guard:** External Tegu Linux bring-up evidence reports an
  `aware_nmi*` auxiliary interface that must not be touched by NetworkManager.
  The native device package now installs a target-local unmanaged-device guard
  for that interface while retaining normal management of the primary Wi-Fi
  link. The external port uses an Android/Halium cellular bridge, so its radio
  matrix remains correlation only and is not Ferrix physical evidence.
- **Evidence and gates:** No physical Ferrix Linux evidence exists: there is
  no physical boot, framebuffer/input result, live service result, exact
  Bluetooth firmware result, S5300 result, or capability evidence. The
  ADB/Fastboot identity, recovery, and unlock observations in the existing
  entry below are admission/recovery history only. The target manifest still
  records `kernel_build_status = "not-built"`, `rootfs_build_status =
  "not-built"`, and `boot_status = "not-attempted"`; all physical Ferrix
  boot, provisioning, and capability gates remain outstanding. Any
  `FERRIX_EVIDENCE=physical:<target-id>` value in target-local environment or
  generated metadata is an exact-target scope label, not evidence.

## 2026-09-28 (Pixel 9a exact-target admission and stock recovery preparation)

- **Physical-target identity:** With the owner's request and a checked-in
  target-scoped workflow, read-only ADB discovery identified one exact device
  as Google Pixel 9a, model/SKU `GXQ96`, codename `tegu`, hardware revision
  `MP1.0`. Before unlock, the observed bootloader was
  `tegu-16.0-12754673`, locked; verified boot was `green`; current slot was
  `_a`; dynamic partitions were enabled. The gated Fastboot workflow then
  verified `unlocked=yes`, slot `a`, and slot count `2`. The serial is
  redacted and no unique device identifier, user data, IMEI, keys, or raw
  partition dump is committed.
- **Target package:** `targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/` records
  the exact identity, conservative capability matrix, recovery gates, and
  official source references. It claims no Ferrix Linux boot or hardware
  capability; all nine `ferrix-hardware-api` capabilities remain
  `unsupported` pending target-specific Linux evidence.
- **Stock recovery inputs:** `just pixel-9a-recovery-download` is pinned to
  Google's public `CP3A.260905.009` Pixel 9a build and verifies the official
  factory and full OTA archives by size and SHA-256 under the ignored
  repository-local target folder
  `targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/recovery/`. The full OTA
  is the preferred ADB recovery route; the factory archive is the Fastboot
  route and may wipe data/require an unlocked bootloader. A factory restore
  has not been tested; the separate bootloader unlock reset completed.
- **Bootloader unlock gate:** The read-only `just
  pixel-9a-unlock-bootloader --preflight` passed the exact-device, single-
  transport, and recovery-artifact checks. The execution workflow then
  required two typed confirmations plus the phone's own confirmation,
  issued `adb reboot bootloader` and `fastboot flashing unlock`, and verified
  `unlocked=yes`. No factory flash, slot change, or relock has run; unlocking
  erased user data as documented.

## 2026-09-21 (developer-environment trust and app-surface validation)

- **Developer-focused security review:** `AGENT_SECURITY.md` now asks one
  bounded question: whether an ordinary developer can safely follow the
  repository's documented setup, build, test, and emulator workflows. It
  requires a per-workflow verdict and concentrates on host mutation, supply
  chain execution, credentials/data, emulator exposure, and the physical-device
  boundary instead of broad product-security commentary.
- **Issues fixed during the review:** Waydroid pin parsing no longer evaluates
  downloaded or alternate pin-file contents as shell code; an adversarial test
  proves command substitution is rejected without execution. Native sandbox
  policies with `mode = "required"` now fail closed before application exec if
  any requested namespace, private `/tmp`, `no_new_privs`, or private runtime
  directory is missing; a Linux integration test verifies status 126 and that
  the application command never runs. The QEMU agent now proves a neutral
  pointer motion reached the compositor (`cursor-ready`) before declaring the
  launcher ready, fixing a race that could consume the first developer tap.
- **Developer surfaces validated:** `just host-check`, formatting, workspace
  Clippy with warnings denied, all workspace tests, Android tool tests,
  Linux-native sandbox tests, `just qemu-check`, the graphical QEMU gesture
  workflow, `just pmos-build`, `just pmos-check`, and the host-only Android
  bootstrap test pass. The opt-in pinned Brave and Waydroid inputs were then
  fetched and verified, `just pmos-android-build` completed, and
  `just pmos-android-check` passed under TCG in 52 seconds with Binder,
  offline Waydroid initialization, container startup, Brave staging, product
  services, and clean poweroff records. The graphical gesture run exercised
  launch, minimize, left/right switching, and close; its 1080x2340
  `out/qemu-aarch64/gesture-check/home-after-close.png` framebuffer was
  inspected and showed the complete eight-tile developer launcher.
- **Graphical app evidence:** A fresh Android-enabled pmOS HVF boot recorded
  Brave provisioning as `installed`, the native home sandbox with every
  required control applied and no degradation, `home-ready`,
  `product-wayland-ready`, and healthy product services. Its inspected
  1080x2340 framebuffer at
  `out/qemu-aarch64/pmos-android/evidence/home-post-fix.png` shows the complete
  launcher. A persistent-overlay boot recorded Brave as `present`; the
  inspected framebuffer at
  `out/qemu-aarch64/pmos-android/evidence/post-fix-qmp/home.png` shows Brave's
  Android first-run surface rendered full-screen. Host-nested compositor
  execution remains Linux-only and therefore was not runnable on this macOS
  host; its Rust paths are covered by the workspace and pinned Linux tests.
- **Scope:** All runtime evidence is QEMU/mock evidence. No physical device was
  enumerated, booted, unlocked, flashed, or otherwise changed.

## 2026-09-21 (documentation audit and GitHub handoff)

- **Current-state reconciliation:** Audited the repository documentation and
  READMEs against the workspace manifests, application descriptors, `justfile`, QEMU
  and postmarketOS scripts, and the latest recorded evidence. The top-level
  overview, application-platform contract, Android runtime record, Android ADR,
  appd README, long-term layout, and QEMU guide now agree that Phone, Messages,
  Contacts, and Settings are independent native apps; Android has a private
  Waydroid adapter; Brave provisioning/persistence and graphical pmOS evidence
  exist; and, among catalog/runtime entries, Web remains unimplemented while
  Camera and Files remain placeholders.
- **Historical clarity:** `docs/audit-2026-09-21.md` is explicitly marked as a
  commit-specific phase-0 snapshot rather than current status. Older entries in
  this chronological file remain unchanged as historical evidence.
- **Publication checks:** Relative Markdown links and documented `just` recipe
  names were checked against the tree. `git diff --check`, formatting, Clippy,
  workspace tests, and the architecture guard are the validation set for this
  documentation-only handoff; no new graphical, QEMU, or physical-device claim
  is introduced by the audit.

## 2026-09-21 (postmarketOS development-window sizing)

- **Aspect-preserving pmOS window:** `pmos-run.sh`, shared by `pmos-run` and
  `pmos-android-run`, now applies the existing smoke-guest window-sizing
  contract. Graphical runs read the 1080x2340 profile, fit it to the host's
  visible height without upscaling, pass the resulting even-pixel mode to
  `virtio-gpu` and `ferrix.display`, and preserve the portrait aspect ratio.
  `FERRIX_QEMU_WINDOW_HEIGHT` provides an explicit logical-point override.
- **Live QEMU evidence (Apple Silicon/HVF, Cocoa, Android pmOS):** On a Retina
  desktop with 960 visible logical points, the runner selected an 830x1800
  guest panel. macOS reported a 415x900-point Cocoa content area plus its
  32-point title bar. The ignored capture at
  `out/qemu-aarch64/pmos-android/evidence/window-fit.png` was inspected and
  visibly shows Brave filling the correctly proportioned portrait panel.
  This is QEMU evidence only; no product rendering behavior changed.
- **Validation and limitation:** Shell syntax and diff checks pass; generated
  graphical, explicit 720-point override, and headless QEMU commands were
  inspected. The unchanged headless pmOS route also passed `just pmos-check`
  under TCG in 43 seconds. Automatic host metrics are currently implemented
  for macOS; other hosts use an 800-point fallback unless
  `FERRIX_QEMU_WINDOW_HEIGHT` is set. GTK remains unverified here.

## 2026-09-21 (Brave shipped and live in Android-enabled postmarketOS QEMU)

- **Root cause and renderer fix:** Waydroid created `waydroid_base.prop`
  during `init`, before Ferrix appended its software-rendering configuration.
  The container therefore ignored the configured values and auto-selected
  Mesa/GBM from QEMU's render node. Live Android logs showed SurfaceFlinger
  repeatedly aborting with `output buffer not gpu writeable`. The first-boot
  hook now runs Waydroid's offline upgrade when the materialized base
  properties are not `gralloc=default` and `egl=swiftshader`, and records
  `waydroid-properties {status:"materialized", renderer:"swiftshader"}`.
- **Permanent Brave provisioning:** The Android product build refuses to
  succeed unless the pinned Brave APK exists and passes SHA-256, APK signature,
  and signer-certificate verification. The image stages it at
  `/usr/share/ferrix/android/brave.apk`. After the compositor creates Wayland,
  product bootstrap starts the Waydroid session, waits for Android framework
  boot completion, installs only if Android has no `com.brave.browser` package
  path, and verifies the resulting path. It also assigns the Android browser
  role as Android's shell UID and verifies that an HTTPS intent resolves to
  Brave. `ferrix-appd` remains launch-only.
- **Lifecycle fix:** Waydroid reports an idle container as `FROZEN` and
  automatically unfreezes it for app commands. The private Android adapter now
  treats `Session=RUNNING, Container=FROZEN` as a healthy quiescent runtime;
  the previous classification rejected a valid Brave launch.
- **Live QEMU evidence (Apple Silicon/HVF, QEMU 11.1.1, 1080x2340):** A fresh
  Android overlay recorded `android-app-provision` status `installed` with
  `browser_role:"default"`.
  Launching `org.ferrix.browser` through the appd protocol recorded
  `android-app-launched` for `com.brave.browser`, `app-started`, and a mapped
  `waydroid.com.brave.browser` surface. The Cocoa framebuffer was captured to
  ignored `out/qemu-aarch64/pmos-android/evidence/brave-default.png` and
  inspected: it visibly shows Brave's first-run UI without Android's
  default-browser chooser. An orderly reboot without `FERRIX_PMOS_FRESH=1`
  recorded provisioning status `present` with `browser_role:"default"`,
  proving the package, role, and Android data persist in the run overlay. This
  is QEMU evidence only.
- **Validation:** Android bootstrap test and shell syntax pass; the 21
  `ferrix-runtime-android` tests pass, including the frozen-container state
  regression; the Android pmOS image rebuilt with the pinned APK verified;
  fresh and persistent graphical boots completed as described above.

## 2026-09-21 (Space Monkey palette and BMW design dock)

- **Default theme:** `themes/ferrix/colors.toml` and the matching built-in
  `Theme::ferrix()` now use a black chassis with adapted Space Monkey signal
  colors: black `#000000` ground and recessed areas, warm-charcoal
  surfaces, amber `#e7aa5a` type, rust `#bd4924` action, teal `#66a891`
  information, gold warnings, and pink-red errors. This replaces the first,
  overly brown cocoa pass. The raised touch surface is intentionally `#2a211b`
  rather than Space Monkey's bright desktop gold selection. The exact palette
  remains staged into the QEMU guest and recorded by its existing
  `colors_sha256` manifest field.
- **Design dock:** `docs/design.md` now makes the division explicit: Space
  Monkey is the palette reference; BMW-inspired cockpit philosophy governs
  driver-oriented priority, grouped information, low-light legibility, and a
  stable action hierarchy. It explicitly excludes external logos, typefaces,
  artwork, wallpapers, desktop configuration, vehicle-interface copy, and
  branding. This is a visual-system change only; targets, guest construction,
  hardware APIs, and product-shell architecture are unchanged.
- **Graphical evidence (QEMU/mock, TCG, 1080×2340):** After rebuilding the
  guest, `just qemu-agent-start` reached `home-ready` with eight catalog tiles
  and `just qemu-frame` captured the nonblank launcher at
  `out/qemu-aarch64/agent/frame.png`. Inspection confirms a black field,
  charcoal cards and tiles, amber readouts and labels, teal network and
  information glyphs, quiet stone hairlines, and the single rust Terminal
  anchor. The first brown/cocoa capture is superseded historical evidence.
  This is QEMU/mock evidence only.
- **Validation:** `cargo fmt --check`, workspace Clippy with warnings denied,
  `cargo test --workspace`, a fresh `just qemu-build`, inspected graphical QEMU
  capture, and `just qemu-check` all pass.

## 2026-09-21 (mobile navigation and app dismissal gestures)

- **Gestures:** A normal bottom-edge tap or upward swipe minimizes the foreground application to Home and keeps its supervised instance alive. An upward swipe longer than 55% of the panel's shorter side sends the new compositor-only `CloseGesture`; `ferrix-appd` terminates the foreground application and explicitly protects Home. Inward swipes from either side edge perform application Back, covering Android's two-sided convention and the iOS left-edge convention; at an application root, Back yields to Home instead of killing the process.
- **Input fix:** Focus changes no longer synthesize pointer focus before a newly mapped client has bound `wl_pointer`. The first real motion now delivers the required enter event, fixing taps being silently dropped after opening an application.
- **Graphical evidence (QEMU/mock, TCG, 1080x2340):** Messages accepted a tap after launch (`input-press`/`input-release`, page `conversations`). A normal bottom swipe raised Home while Messages remained supervised; relaunching resumed that instance. A long bottom swipe recorded `close-gesture`, `app-terminating`, `client-gone`, and exit signal 15 before revealing Home. A right-edge inward swipe from a relaunched Messages instance recorded `messages-back` with status `yielded` and raised Home. The resulting Home framebuffers were inspected. This is QEMU/mock evidence only.
- **QEMU agent correction:** Repeating the graphical test from independent command invocations exposed that the short-lived `qemu-agent-start` shell could take its background QEMU process down when it exited, leaving a stale PID and an unreachable QMP socket. Headless agent sessions now use QEMU's own `-daemonize` and `-pidfile` support; a clean start remained reachable through separate status, frame, tap, and swipe commands for the entire gesture sequence.
- **Validation:** `cargo fmt --check`; `cargo clippy --workspace --all-targets --all-features -- -D warnings`; `cargo test --workspace`; a fresh `just qemu-build`; `just qemu-check` (TCG, required services, deterministic unplugged-to-charging transition, clean power-off); `just qemu-scenario-check` (25 passed); `just qemu-gesture-check` (minimize, left- and right-edge yield, close, final framebuffer); shell syntax checks for the QEMU run and agent launch scripts; and the graphical gesture sequence above.
- **Limitation:** App dismissal is a direct, one-stage adaptation of swiping away a recent-app card. A recent-app thumbnail overview and interactive hold/drag animation remain future product-shell work.

## 2026-09-21 (mobile connectivity: telephony, networking, contacts, apps, authorization, Android runtime)

- **Authority:** `docs/baseline.md`/`docs/plan.md` had been moved by the owner to `plans/baseline.md`/`plans/plan.md` (byte-identical, commits `19d43fe`/`99ce57a`); references were repointed and the resolution recorded in `docs/audit-2026-09-21.md`. Architecture decisions for this programme are `docs/adr/001..006`, subordinate to the baseline.
- **Telephony (ADR-001):** `ferrix-telephony-api`/`-protocol` (schema 1) model modem, SIM (incl. PIN attempts), registration/operator/access/signal, calls (dialing/ringing/active/held/ended with typed end reasons), SMS (sending/sent/delivered/failed/received/read/deleted with typed failures, multipart assembly), data bearer, audio route, radio power, and feature discovery. `ferrix-telephonyd` owns call/SMS identity, an atomic versioned SMS store with fail-closed corruption handling and a forensic copy, per-operation authorization, and reconciliation (`service-restarted` snapshot); the mock backend (`ferrix-telephony-mock`) is profile-seeded and tick-driven (no wall clock); `ferrix-telephony-mm` consumes ModemManager over `zbus` (feature `production`) and never QMI/MBIM/QRTR/AT. Emergency numbers are rejected as `unsupported{feature: emergency}`; nothing represents emergency calling as supported. Calls and SMS were deliberately not folded into `ferrix-hardware-api`; its coarse `Telephony`/`Network` summaries remain as fallbacks only.
- **Networking (ADR-002):** `ferrix-network-api`/`-protocol` distinguish adapter presence (`ethernet-like` for virtio-net, never Wi-Fi), radio, scan state, hashed access points, security type, association/authentication/configuration states, saved profiles, IP config, local-vs-Internet connectivity, captive portal, and typed auth/DHCP/DNS/route/timeout errors. `ferrix-networkd` with the synthetic-AP mock backend and a NetworkManager adapter (`ferrix-network-nm`, `zbus`, feature `production`). The Alpine smoke guest stays offline by decision (ADR-006); real IP connectivity remains the postmarketOS route's property.
- **Contacts (ADR-003):** radio-independent `ferrix-contactsd` with stable `c_<hex>` ids, E.164 normalization (`phonenumber`, default region `US`), search/lookup, vCard 4.0 export and 3.0/4.0 import, atomic tmp+fsync+rename writes, fail-closed corruption handling with a forensic copy, explicit migrations with fixtures, and synthetic fixtures (`Test Contact NN`, `+1555010000N`).
- **Applications:** `org.ferrix.phone`, `org.ferrix.messages`, `org.ferrix.contacts`, `org.ferrix.settings` are real independently supervised Wayland clients launched only by `ferrix-appd` (the illustrative `org.ferrix.dialer` entry was removed; `camera`/`files` remain reference pages). The home surface reads telephony/network state from the services when reachable and falls back to the hardware-api summaries, refreshes tiles on `CatalogChanged`, and routes the Wi-Fi toggle to `ferrix-networkd`. Host previews for every page are produced under ignored `out/previews/` (`--preview`).
- **Authorization (ADR-004):** `AppCapability` gained `telephony-state/-call/-sms`, `contacts-read/-write`, `network-state/-control`. `apps/grants.json` is a checked-in policy; the effective set is declared ∩ granted (declaration is discovery, not authorization). Daemons resolve the peer pid through `ferrix-appd` (`service` role, `identify{pid}` → `identity{app,trust,granted}` from `SO_PEERCRED` ancestry) and check the capability on every request; long-lived subscriptions re-resolve before every event and are dropped on revocation. Denials are typed `unauthorized` plus `*-request-denied` records. The sandbox gained `run_dir.mode = private|private-run`: a private `/run/ferrix` (or `/run`) view holding only the sockets implied by the app's grants; every launchable native app uses `private-run`, which also hides the system D-Bus socket. Remaining limitations: shared uid, no seccomp/cgroups, compositor/tool/service roles are bootstrap trust.
- **QEMU simulation (ADR-006):** profile gained optional strict `[telephony]` and `[network]` sections (schema stays 1); each mocked daemon has its own host-only virtio-serial channel (`ferrix.telephony`, `ferrix.network`) via the shared `ferrix-mock-control` crate, refused with production backends. Test-only client verbs (`dial`, `answer`, `send-sms`, `connect`, ...) run through the daemons' normal dispatch path under a synthetic host-tool identity; acks are written only after the records they cause. `ferrix-scenario` runs the 25 checked-in scenario files (`targets/qemu-aarch64/scenarios/`) with strict schema and JSON-subset matching, writing per-scenario evidence under `out/qemu-aarch64/scenarios/<name>/`.
- **Android runtime (ADR-005):** `docs/android-runtime.md` evaluates Waydroid, Anbox (archived), redroid, Android-x86/Bliss, and AVD (excluded by baseline) against the required criteria from upstream sources fetched 2026-09-21; Waydroid is the only viable candidate and the pinned pmOS kernel (`linux-postmarketos-stable` 7.0.12) already enables binder devices. `ferrix-runtime-android` implements `RuntimeKind::Android` behind `ferrix-app-runtime-api` over the Waydroid CLI (absent runtime → typed `RuntimeUnsupported`), composed in `ferrix-appd`. `apps/org.ferrix.browser.json` describes Brave (an Android application running in the compatibility runtime; package/activity are runtime-private). `tools/android/brave-fetch.sh` downloads only the pinned official GitHub release asset (`v1.95.104`, `BraveMonoarm64.apk`), verifies a pinned SHA-256 plus the APK signing schemes and signer certificate with Android SDK Build-Tools `apksigner`, refuses foreign redirect hosts and implausible sizes, and stores only under ignored `out/android/`; `--local` verifies a supplied APK. The Android first-boot bootstrap now installs a staged verified APK and verifies `com.brave.browser` through `waydroid app list`; `just android-bootstrap-test` covers install and subsequent-boot idempotence with a fake CLI. **Android claim level demonstrated: none**: no Android QEMU image exists on this host, and the Android bounded check currently stops at its required `pmos-android/manifest.json`.
- **Physical target:** `just oneplus-discover` (read-only) again found no ADB or Fastboot device; the board-support proposal now carries the exact-target checklist (all `pending`) and the physical validation matrix (all `not started`). `tools/oneplus/provision-gate.sh` resolves and prints identity/slot/partition/hash/recovery fields, requires a typed per-run confirmation, and always exits with `provisioning-refused: no-approved-proposal`; a test proves it contains no flash/erase/unlock/boot/slot command. Nothing destructive exists or ran.
- **Validation (host, `mock`):** `cargo fmt --check`; `cargo clippy --workspace --all-targets --all-features -- -D warnings`; `cargo test --workspace` 529 passed, 0 failed (baseline before this work: 200); `just arch-check` 11 guards (new: contract crates vendor-neutral, product crates free of adapter/daemon deps, daemons name backends only in `main.rs`, evidence-record source lines never carry number/body/IMSI/ICCID/IMEI/BSSID/passphrase keys); `sh tools/android/tests/run.sh` 17 passed; `just oneplus-tools-test` passed. **Validation (Linux container, `mock`):** `just linux-check test --workspace` 542 passed, 0 failed, including 14 real-namespace sandbox tests with `run_dir` socket filtering and full-`/run` replacement coverage. **Validation (QEMU):** fresh `just qemu-build` (manifest records every new binary hash, `grants_sha256`, `contacts_fixtures_sha256`, and the mock-control ports); `just qemu-check` `{"event":"check-passed","transition":"unplugged-to-charging","services":"required","seconds":4}` with `telephonyd-ready`, `networkd-ready`, `contactsd-ready contacts:8` on serial; `just qemu-scenario-check` `{"event":"scenario-check-passed","passed":25,"failed":0,"seconds":30}` under TCG, headless, offline. The scenario session records Home with non-degraded `private-run` isolation and only `ferrix-appd.sock`, `core.sock`, `networkd.sock`, and `telephonyd.sock` visible.
- **Graphical evidence (QEMU, headless TCG agent session, 1080x2340, frames under `out/qemu-aarch64/agent/`, serial `agent/serial-graphical-evidence.log` and `agent/serial-recapture.log`):** `launcher.png`/`launcher-services.png` show eight tiles (Brave, Contacts, Messages, Phone, Settings, Terminal, Camera, Files) with the status bar and network card sourced from the services (`home ui-state telephony_source:"service", network_source:"service"`; zero `*-request-denied` for `org.ferrix.home` after the descriptor fix). Phone: `phone.png` dialer with `ready · sim ready · registered · 3 bars · mock`; `phone-ringing.png` incoming `+15550100042` with Answer/Reject; `phone-in-call.png` active call, speaker route selected, Hold/Keypad/Hang up, redacted line; `phone-after-hangup.png` `remote-hangup` badge; `just qemu-scenario incoming-call-answer-route-remote-hangup --frame` passed (10 steps) with Phone in the foreground. Messages: `messages-multipart.png` after `sms-incoming-multipart --frame` (9 steps) shows the conversation row with the assembled three-part synthetic body and a `2 unread` chip, and a new `ui-state 1-conversations` record; the first attempt exposed a missing wake in the telephony-protocol reconnector, fixed with a regression test. `contacts.png` lists the eight synthetic fixtures with labels, search, New, Export vCard. `settings.png` shows the Network page (radio enabled, mock Wi-Fi adapter, `ferrix-lab` connected, connectivity full, DHCP address, two access points, saved profile). `home-brave-notice.png`: tapping Brave without Waydroid in the guest yields serial `android-runtime-absent {host:waydroid, reason:"waydroid not found"}`, `app-launch-failed {runtime-unsupported}`, `home-launch-notice`, and the visible banner `Brave could not start · no android runtime is available`. Those capture-session `app-started` records used the earlier `run_dir.mode=private`; that isolation evidence is superseded by the fresh headless QEMU `private-run` record above. The pixels remain valid UI evidence because this audit did not change rendering. `qemu-agent-start` waits for basic-screen's `home-ready` record so first taps are deterministic. All frames were inspected by the lead, not inferred from process logs.
- **postmarketOS route (QEMU):** a fresh `just pmos-build` installs the daemons, apps, grants, and contact fixtures into the pmbootstrap-built image (manifest records every new binary hash, `grants_sha256`, `contacts_fixtures_sha256`, `telephony_backend_default=mock`, `network_backend_default=mock`, `networkmanager_installed=true` (1.52.2), `modemmanager_installed=false`). `just pmos-check` (TCG, 43 s) observed in order `product-core-ready`, `product-service-started` for telephonyd/networkd/contactsd, `product-services-ready {…, persistent:true}`, `telephonyd-ready` (9 capabilities), `networkd-ready backend mock`, `contactsd-ready contacts:8`, `grants-loaded apps:7`, `ferrix-appd-ready apps:9`, `pmos-boot-health status ok`, `pmos-check-poweroff`, then `pmos-check-passed`. Backend selection travels as `ferrix.telephony=`/`ferrix.network=` kernel parameters (default `mock`); the NetworkManager/ModemManager backends are wired but not exercised in this entry, and ModemManager is not installed in the image.
- **Limitations:** QEMU/mock evidence only; no physical radio, SIM, carrier, Wi-Fi firmware, audio, suspend, or recovery claim. The ModemManager and NetworkManager adapters compile with unit-tested mapping tables but have not run against live services (no instance in QEMU); telephony audio routing on the production adapter is `unsupported`. Daemon *process* restart in the guest is covered only by init supervision plus the `crash-backend` reconciliation scenarios (no host-to-guest kill path). Peer-credential authorization is Linux-only; nested macOS runs need `--appd none --allow-unauthenticated`. Brave verification requires a suitable Android SDK Build-Tools `apksigner` on the host. Waydroid packaging for the pmOS route is implemented as an opt-in (`FERRIX_PMOS_ANDROID=1`: `tools/android/waydroid.pins`, allow-listed `waydroid-images-fetch.sh`, `pmbootstrap-android.cfg`, a first-boot `00-ferrix-android.start` that records binder devices and runs `waydroid init` offline with software-rendering properties, and `just pmos-android-build|run|check`) but was **not built or booted** in this entry because the host lacked the ~5 GiB the pinned images need; the default pmOS route is unaffected (`pmos-check-passed … "android":0`). Android claim level remains none.

## 2026-09-20 (application focus and Terminal visibility)

- **Fix (application platform):** `ferrix-appd` now keeps the sandbox supervisor PID separate from the Wayland surface PID reported by the compositor. A launch queues activation until that app's first surface is mapped, then activates the mapped client PID; later single-instance launches activate that same client. This is a runtime-wide rule for every independently supervised app, not a Terminal exception. The appd link also forwards a notification into its calloop queue before waking the compositor, closing a race that could leave an activation unread until unrelated input arrived.
- **Fix (compositor):** A buffer commit from an already mapped background client no longer moves that client to the top of the fullscreen stack. Focus transitions request a full repaint and reset DRM buffer ages before rendering, preventing stale pixels when Home, Terminal, or a future app is raised. Machine-readable `focus-frame` records identify the client PID rendered after each activation.
- **Build safety:** `qemu-build` writes the initramfs through a temporary sibling, verifies it with `gzip -t`, and only then replaces the prior artifact atomically. A failed or interrupted compression therefore cannot leave a truncated `initramfs.cpio.gz` that fails before guest `/init`.
- **Graphical evidence (QEMU/mock):** The exact Cocoa/HVF route (`FERRIX_QEMU_GRAPHICS=cocoa FERRIX_QEMU_ACCEL=hvf just qemu-agent-start`, which uses the same guest and display path as `FERRIX_QEMU_GRAPHICS=cocoa just qemu-run`) opened Terminal from the launcher at 830x1800. Inspected framebuffer captures at 2 and 8 seconds both show the Terminal, real `root@org:~#` PTY prompt, and on-screen keyboard. Serial records `terminal-ready`, then `client-activated` and `focus-frame` for mapped Terminal client PID 690 with `raised:true` and `empty:false`; there is no app/process exit. A second inspected sequence shows Terminal, Home after the bottom-edge gesture, then the same live Terminal instance after tapping its tile again. Home PID 672 and Terminal PID 690 each report successful activation. This resolves the supervisor/client PID limitation recorded in the visual-system checkpoint below.
- **Validation:** `cargo fmt --check`; `cargo clippy --workspace --all-targets --all-features -- -D warnings`; all 200 host workspace tests (including 33 appd unit tests, 3 appd socket tests, and 42 shell tests); shell syntax checks for `qemu-build.sh` and `pmos-run.sh`; a fresh `just qemu-build`; `just qemu-check` (TCG, deterministic unplugged-to-charging transition, clean power-off); and the two Cocoa/HVF framebuffer-inspected sessions above. Evidence is QEMU/mock only; no physical-device claim is implied.

## 2026-09-20 (pmOS interactive overlay invalidation)

- **Fix (QEMU):** `pmos-run` now records the manifest's raw-disk SHA-256 beside each qcow2 overlay and recreates a persistent run overlay when a new `pmos-build` changes its backing image. Previously, the retained overlay could contain the old partition/filesystem UUIDs while the newly exported kernel command line named the new UUIDs, causing postmarketOS initramfs to enter its debug shell. Check mode remains fresh on every boot, and `FERRIX_PMOS_FRESH=1` still forces an interactive reset.
- **Validation (QEMU):** Shell syntax and `git diff --check` pass. A fresh TCG `just pmos-check` reached `product-core-ready` and `pmos-boot-health`, then powered off cleanly in 43 seconds. The existing unmarked run overlay was automatically refreshed; an HVF console boot reached the `ferrix login:` prompt, and the exact `FERRIX_QEMU_GRAPHICS=cocoa just pmos-run` route reached `product-wayland-ready`. Its 1080x2340 QEMU framebuffer was captured from the monitor to `out/qemu-aarch64/pmos/run/frame.png` and inspected as a nonblank Ferrix launcher. This is QEMU/mock evidence only.

## 2026-09-20 (OnePlus target admission started)

- **Authorization:** Repository policy, baseline, plan, and README now admit exact-model physical targets through a checked-in board-support proposal. The owner approved read-only discovery and proposal work for the attached OnePlus; destructive operations remain gated on exact identity, compatible hashed images, verified backup/recovery, and explicit per-run confirmation.
- **Host tooling:** Android Platform Tools 37.0.1 was installed through Homebrew. `adb` and `fastboot` resolve from `/opt/homebrew/bin`; the ADB server starts successfully.
- **Workflow:** `just oneplus-tools-check` verifies both tools. `just oneplus-discover` performs read-only, single-device ADB/Fastboot discovery and refuses ambiguous transports; it contains no reboot, unlock, boot, erase, format, flash, or slot-changing command. The pending proposal is `docs/board-support/oneplus-attached.md`.
- **Current evidence:** Neither ADB, Fastboot, macOS System Profiler, nor the lower-level IOUSB registry exposed the phone. No serial, model, codename, revision, bootloader state, or partition information was obtained, and no phone state was changed. A board directory and device image were intentionally not fabricated without that identity.

## 2026-09-20 (Ferrix visual system and unified theming)

- **Palette:** First-party surfaces no longer use the Tokyo Night palette (`#1a1b26` background, `#7aa2f7` blue and `#bb9af7` purple accents). The Ferrix system in `docs/design.md` replaces it: slate surfaces (`bg` `#15181c`, `bg_dark` `#0f1114`, `surface` `#1c2126`, `surface_hi` `#262c33`, `border` `#353d46`), steel type (`fg` `#d7dde3`, `fg_dim` `#7d8a96`), one rust accent (`rust` `#d8642a`, `rust_hi` `#f0803f`, `rust_dim` `#8a3f1a`) with `steel` `#8fa3b3` as the secondary accent, desaturated status colors (`ok`, `warn`, `err`), and a sixteen-color terminal ANSI set whose index 0 equals `bg`. `IconTone` in `ferrix-app-api` is unchanged; `ferrix-basic-screen` maps `Blue`/`Cyan` to steel, `Purple`/`Orange` to rust, `Green`/`Yellow`/`Red` to the status colors, and `Neutral` to `fg_dim`.
- **Theming contract (`ferrix-ui`):** `ferrix_ui::theme::Theme` (fourteen `0xRRGGBB` tokens plus `ansi: [u32; 16]`) is loaded once per process by `theme::init(evidence_label, explicit_path)` from, in order, the explicit path, `$FERRIX_COLORS`, `/etc/ferrix/colors.toml`, then the built-in `Theme::ferrix()`; `theme::get()` returns the active theme (the built-in when `init` never ran). `Theme::from_toml` uses `toml` + `serde` and accepts the flat key set of Omarchy desktop `colors.toml` files (`accent`, `selection`, `muted`, `background`, `dark_background`, `lighter_background`, `foreground`, `dark_foreground`, `light_foreground`, `bright_foreground`, the eight normal and six bright ANSI names; unused desktop keys such as `mode`, `orange`, `darker_background` are ignored) plus an optional `[ferrix]` table (`steel`, `ok`, `rust_hi`, `rust_dim`, `bright_black`) whose entries fall back to `blue`, `green`, `accent`, `brown`-or-`accent`, and `muted` respectively, so a desktop theme loads unchanged. `schema_version` is optional and only `1` is accepted; malformed colors, missing keys, and unknown `[ferrix]` keys are typed `ThemeError`s. The canonical `themes/ferrix/colors.toml` is checked in and a unit test asserts it parses to `Theme::ferrix()`. Evidence records: `theme-loaded` with `source` = `builtin` or the resolved path, and `theme-invalid` (with `source` and `error`) when a configured file is missing or rejected, after which the process continues on the built-in palette. The former `theme::BG`/`BLUE`/`PURPLE` constants are gone; nothing outside `ferrix-ui` used them. The architecture guard now allows `ferrix-ui/src/theme.rs` as the one TOML schema parser besides `ferrix-target-profile`.
- **Guest packaging (QEMU):** Both guest routes ship the palette as `/etc/ferrix/colors.toml`. `qemu-build` stages `themes/ferrix/colors.toml` (or `FERRIX_QEMU_COLORS`) beside `profile.toml`, the initramfs init copies it into `/etc/ferrix/`, and `out/qemu-aarch64/manifest.json` records `colors_sha256` and `colors`; the fresh build's `colors_sha256` `7593bb3a2a7eda6ed24440bafaf17b5e235ce65ac1ff0366ee2679076923dafa` equals `shasum -a 256 themes/ferrix/colors.toml`. The pmOS finalize step installs the same product copy and records `colors_path`/`colors_sha256` in `image-facts.json`; `pmos-build` fails if the product artifact is missing. `just shell-nested` exports `FERRIX_COLORS` pointing at the repository file. In the headless TCG agent session every themed process reported `{"evidence":"QEMU","event":"theme-loaded","source":"/etc/ferrix/colors.toml"}`: three records in the session that launched Terminal (compositor log replay, home surface, Terminal) and two in the launcher-only session, with zero `theme-invalid`, `compositor-failed`, `boot-failed`, `ferrix-appd-failed`, `compositor-exit`, or `wayland-failed` records in either.
- **Compositor and cursor (QEMU):** `ferrix-shell` calls `theme::init` before anything paints and clears every output to the theme `bg` (`crates/ferrix-shell/src/color.rs`; `render::clear_color()` replaces the Tokyo Night constant on the udev and winit backends), so an uncovered framebuffer is `#15181c`. The `ferrix-ui` virtual-tablet cursor ring is now `rust_hi` read at install time; serial recorded `cursor-ready` (`transparent-ring`, 32 px) and the ring is visible as 16 `#f0803f` pixels at the tap point in the Quick settings capture.
- **Restyle (QEMU, mock):** Cards, tiles, and panels use a 10 px radius at 1080 width (8 px terminal grid well, 6 px key chips and back buttons) instead of 16-34 px pills, 1-2 px `border` hairlines that never truncate below 1 px on narrow panels, thin rust rule lines for section emphasis (page titles, the crash notice's left rule), and brightness/battery tracks at 4 px radius. The accent budget is one rust element per region: on the launcher the only `#d8642a` pixels (1370) are inside the Terminal tile; in the Terminal they are the back chevron and the block cursor (the cursor drops the accent when the shell has exited); in Quick settings the rust outline and state word mark exactly the toggles that are on. Network glyphs are steel, never rust. Pressed tiles raise to `surface_hi` with a 3 px rust outline. `ferrix-basic-screen --preview` gained `--page` (`launcher`, `quick-settings`, `notifications`, `notifications-empty`, or a reference kind) and `--size WxH`; both previews call `theme::init("mock", None)`.
- **Low-light orange refinement (QEMU):** The visual system now applies the marketing position as a low-light instrument-panel principle: matte slate, warm oxidized-orange hierarchy, no glow or bright field. The Terminal is a deep `rust_dim` (`#8a3f1a`) launcher anchor with a `rust_hi` identity rule; Terminal screen dividers and reference-page rules are quiet `rust_dim`, while direct focus remains `rust`/`rust_hi`. A fresh 1080×2340 QEMU framebuffer was inspected at `out/qemu-aarch64/agent/ferrix-launcher-orange.png`; the Terminal tile is visibly warmer while the utility tiles and information cards stay dark, low-glare slate. A fresh tap then launched the Terminal, captured its framebuffer, and serial recorded `app-started`, `terminal-spawned` (`/bin/sh`, 55×32), `terminal-ready` (`root@org:~#`), `app-surface`, and `client-mapped`. This is QEMU evidence only.
- **Typography selection (not yet shipped):** `docs/design.md` selects IBM Plex Sans Condensed for system chrome, IBM Plex Mono for terminal cells, and script-specific Noto Sans UI / Noto Sans Mono fallbacks. The current renderer still embeds JetBrains Mono only and lacks font fallback, bidi, and complex-text shaping, so no multilingual-font implementation is claimed yet.
- **Copy:** Surface strings follow `docs/design.md`: name the thing, then the state, fields separated by ` · `, evidence-labelled, no greetings. Examples: the `Good morning/afternoon/evening` greeting is now the identity line `ferrix · qemu-aarch64 · mock` (target from the connected session); `You're all caught up` is `No notifications`; `Settings are ready` is `Mock state · QEMU`; `Starting shell...` is `shell · starting`; `Charging - mock` is `charging · mock`; the launcher caption is `reference surface · not the product shell`; the home window title is `ferrix · home`. Serial/JSON record names and keys are unchanged. The Terminal's canned `--preview` transcript is labelled `canned transcript · mock · no pty` and its `uname` line carries a placeholder release (`canned-virt`) rather than a kernel version the real guest may not run.
- **Catalog tones (`apps/*.json`):** Terminal `purple` -> `orange` (rust, the one accented tile), Messages `green` -> `blue` (steel), Camera `yellow` -> `neutral`, Files `orange` -> `neutral`. A `ferrix-basic-screen` unit test asserts the checked-in descriptors spend the accent on the Terminal tile only. The manifest `apps_sha256` changed accordingly.
- **Framebuffers (QEMU, headless TCG agent session, 1080x2340):** `out/qemu-aarch64/agent/ferrix-launcher.png` (slate launcher: identity line, battery/network cards with `discharging · mock` and `LTE ok · SIM present`, six tiles with the rust Terminal glyph), `out/qemu-aarch64/agent/ferrix-terminal.png` (Terminal after tapping the tile: `bg_dark` grid well, `root@org:~#` prompt from the real `/bin/sh` PTY, rust block cursor, hairline key chips; serial `terminal-ready`), and `out/qemu-aarch64/agent/ferrix-quick-settings.png` (status-bar tap from the launcher in a second session; serial `input-release` `page` `QuickSettings`). Pixel sampling of the PPM dumps: launcher background `#15181c`, tiles `#1c2126`, status bar `#0f1114` over a `#353d46` hairline; every frame contains zero pixels of `#1a1b26`, `#7aa2f7`, `#bb9af7`, or `#ad8ee6`. Serial logs for both sessions are preserved as `out/qemu-aarch64/agent/ferrix-session-1-serial.log` and `ferrix-session-2-serial.log`.
- **Documentation:** `docs/design.md` is the visual-system reference (principles, palette tables, icon-tone mapping, shape and type rules, copy rules, theming contract, and what this pass intentionally does not do); `README.md`, `plans/baseline.md`, `plans/plan.md`, and the QEMU README's new "Unified theming" section point at it.
- **Intentionally not done:** No micro-tiling or terminal-first navigation; the launcher remains a catalog tile grid in the focused-fullscreen, touch-first shell required by `AGENTS.md`, and gesture semantics, hit-test geometry, protocol contracts, and `IconTone` are unchanged. Theming is a file contract only: there is no live desktop-to-guest sync service, file watcher, or transport. Nothing else in the positioning (agents, clipboard or history sync, key storage) is implemented or claimed.
- **Validation:** `cargo fmt --check`; `cargo clippy --workspace --all-targets --all-features -- -D warnings` (macOS host); `cargo test --workspace` (198 tests on the macOS host: `ferrix-ui` 17 including the theme loader and lookup-order tests, `ferrix-basic-screen` 27, `ferrix-terminal` 12, `ferrix-shell` 41); `just arch-check` (7 tests); `just linux-check` (207 tests on aarch64 Linux in the pinned container, including the 10 `ferrix-sandbox` namespace tests); a fresh `just qemu-build`; `just qemu-check` (TCG, `check-passed`, unplugged-to-charging in 4 s, `reboot: Power down`); and the two agent sessions above (`just qemu-agent-start`, `qemu-frame`, `qemu-tap`, `qemu-swipe`, `qemu-agent-stop`), each reaching `wayland-ready` in about 6 s.
- **Limitations:** QEMU/mock evidence only; no physical-device claims. `just qemu-check` runs with graphics off, so its serial log has no `theme-loaded` record; that evidence comes from the graphical session. `theme-loaded` carries no process field, so which process emitted each record is inferred from log order. Returning home from the Terminal did not work in this session: the compositor's bottom-edge gesture reached `ferrix-appd` (`home-gesture` `swipe`, `linked` true) but the resulting `client-activated` record was `{"pid":675,"app_id":"org.ferrix.home","raised":false}` because `ferrix-appd` activates by the instance's supervisor pid (`app-started` pid 675) while the compositor maps the home window under the sandboxed client's pid (`app-surface`/`client-mapped` pid 677); the Terminal's own back chevron produced no serial record. `ferrix-appd` is not part of this change set, so the pre-existing activation mismatch is recorded here and left for the application-platform work; `ferrix-home-after-gesture.png` shows the Terminal still in front. Quick settings therefore comes from a fresh session opened directly from the launcher. Both mock toggles are on in the profile, so that page shows two rust outlines by state, not by decoration. The pmOS route installs `colors.toml` and records its hash but has not received fresh framebuffer validation in this entry. No new `theme-invalid` path was exercised in QEMU (unit tests cover it).

## 2026-09-20 (Omarchy Ferrix graphical validation)

- **QEMU:** A fresh AArch64 guest build produced an Omarchy Ferrix manifest with `runtime_namespace: "ferrix"` and Ferrix-prefixed binary hashes. An agent-operated TCG QEMU session reached `wayland-ready`, captured the nonblank `ferrix-basic-screen` framebuffer, and reported the home surface title as `Omarchy Ferrix home`.
- **Application launch:** Tapping Terminal in that captured QEMU session made `ferrix-appd` launch `org.ferrix.terminal`; serial evidence recorded `app-started` with all required PID/mount/IPC/UTS namespaces and `terminal-ready` from its real `/bin/sh` PTY. Evidence is QEMU only; no physical-device claim is implied.

## 2026-09-20 (application platform and crash isolation)

- **Architecture:** Applications are now independent, supervised Linux processes managed by a new application service, `ferrix-appd`, instead of in-process pages of one client. The public contract lives in two target-neutral crates: `ferrix-app-api` (validated `AppId`s, `RuntimeKind`, public JSON `AppDescriptor`s, lifecycle state, and typed errors) and `ferrix-app-protocol` (role-gated line-delimited JSON, schema v1). The replaceable daemon-to-runtime traits live separately in internal `ferrix-app-runtime-api`; native executable and namespace configuration remain private to `ferrix-runtime-native` and are never returned to launcher/application clients. `ferrix-appd` owns the catalog (`apps/*.json`), single-instance activation, lifecycle, crash records, home-restart backoff, and surface association by kernel-reported pid. On Linux/QEMU it also reconciles launched clients' claimed protocol roles with `SO_PEERCRED` pid ancestry, preventing an app from self-assigning `home`, `tool`, or `compositor`. The full design is in `docs/app-platform.md`. The architecture guard fails if an application, shell, or the core depends on either runtime crate or `ferrix-appd`, or if `ferrix-app-api` gains target/implementation conditionals.
- **Namespaced apps (QEMU):** `ferrix-runtime-native` launches every native app through `ferrix-sandbox`, a safe-Rust (rustix/signal-hook) supervisor that unshares PID, mount, IPC, and UTS namespaces (user namespace when unprivileged), mounts a private `/proc` and tmpfs `/tmp`, sets `no_new_privs`, and runs a PID 1 that forwards signals and reports the exact exit reason. In a headless TCG agent session the home surface and Terminal each started with `sandbox-ready` reporting all four namespaces true; inside the Terminal, `cat /proc/1/comm` returned `ferrix-sandbox`, `/proc` listed only 5 processes, and the prompt hostname (`root@org:…`) reflected the UTS hostname set to the app id.
- **Crash isolation (QEMU):** The Terminal is now `ferrix-terminal`, a separate Wayland client launched by `ferrix-appd` on a tap. Running `kill -9 -1` inside its shell (confined to its PID namespace) produced, on serial: `client-gone` (pid 700) from the compositor, `app-crashed` `signal:9` from `ferrix-appd`, `home-crash-notice`, and the compositor refocusing `org.ferrix.home` to the foreground. The compositor, `ferrix-appd`, and `ferrix-core` all survived (zero `compositor-exit`/`compositor-failed`/`ferrix-appd-failed`/`boot-failed` records). Framebuffers captured under `out/qemu-aarch64/agent/`: `launcher-catalog.png` (catalog-driven launcher), `terminal-namespaced.png` (Terminal in its namespace), `home-after-crash.png` (home with the "Terminal stopped unexpectedly · signal 9" banner), and `qa-terminal-launch.png` (fresh validation after separating the public descriptor from native launch configuration).
- **Compositor:** `ferrix-shell` now links `ferrix-appd` (ret: reports surfaces/focus by `SO_PEERCRED` pid, obeys `Activate`, reconnects on loss) and owns a trusted bottom-edge home gesture (`home-gesture{kind,source,linked}`). New records: `client-gone`, `client-activated`, `home-gesture`, `ferrix-appd-link`. A regression was found and fixed during bring-up: re-pointing pointer focus on every `refocus()` pinned Smithay's pointer focus to a surface whose client had not yet bound `wl_pointer`, so taps never reached the client; pointer focus now follows real motion and is only re-pointed when the pointer was already over a client (crash recovery).
- **Shared UI:** The software-rendering toolkit and the fullscreen Wayland client runner moved to `ferrix-ui`; `ferrix-basic-screen` is now the catalog-driven home surface (launchable tiles go through `ferrix-appd`; reference entries keep illustrative pages; the home descriptor is never listed as a tile).
- **Validation:** `cargo fmt --check`, workspace clippy with warnings denied (macOS and, via the pinned-container `just linux-check`, aarch64 Linux with `--all-features`), all workspace tests on both hosts (including the `ferrix-sandbox` namespace integration tests, which ran rather than skipped in the privileged container), the privileged-role regression test, `just arch-check`, a fresh `just qemu-build`, and `just qemu-check` (TCG, unplugged-to-charging in 4s, clean power-off) pass. A fresh graphical QEMU run was framebuffer-inspected at the launcher and after launching Terminal through the private native manifest configuration; both rendered correctly.
- **Limitations:** QEMU/mock evidence only; no physical-device claims. Isolation is process- and namespace-based (PID, mount, IPC, UTS, private `/proc` and `/tmp`, `no_new_privs`); apps still run as the session user with access to the shared filesystem and the Wayland/core/ferrix-appd sockets. Launched-app roles are tied to peer PID ancestry, but compositor/tool authentication remains a bootstrap trust boundary. Portals and permission enforcement, seccomp, cgroup resource limits, a network namespace, a restricted filesystem view, the TypeScript/web and Android runtimes, and supervision of the core and compositor themselves remain future work. The pmOS graphical route was updated to start `ferrix-appd` but has not received fresh framebuffer validation in this entry.

## 2026-09-20 (Omarchy Ferrix namespace)

- **Identity:** The project and ship are now Omarchy Ferrix. Its `ferrix` kernel/userspace runtime namespace owns runtime identifiers (`FERRIX_*`, `ferrix.*`, `/run/ferrix`, `/etc/ferrix`, `ferrix.control`, and the `ferrix` guest hostname). Gossan is reserved for a future device image; the current emulator remains the generic `qemu-aarch64` target.
- **Boundary:** Project prose, UI labels, application ids, service files, cache/container names, CI keys, and the bundled architecture skill no longer use the external product's namespace. The generic target id remains `qemu-aarch64`, and all evidence remains QEMU/mock only.
- **Validation:** `cargo fmt --check`, workspace Clippy with warnings denied, all 78 workspace tests, shell syntax checks, a fresh guest build under `.cache/ferrix`, and `just qemu-check` pass. The TCG serial record confirms `ferrix.mode=check`, `/run/ferrix/core.sock`, `ferrix.control`, the deterministic unplugged-to-charging transition, and clean power-off.

## 2026-09-20 (foundation QA remediation)

- **Product control path (QEMU):** `ferrix-protocol` now owns the typed, schema-versioned state/subscribe/command contract shared by `ferrix-core`, `ferrix-shell`, and `ferrix-basic-screen`. Wi-Fi, Bluetooth, and brightness controls issue portable `ferrix-hardware-api` commands through the core and reconcile from authoritative state/events; host-only battery/SIM/modem mutations remain isolated on the mock-control channel. Audio is now a first-class typed capability with route state and mock coverage.
- **Product compositor (QEMU):** `ferrix-shell` is the default guest compositor (Smithay 0.7, pixman, DRM dumb buffers). It uses the one strict `ferrix-target-profile` parser and subscribes to `ferrix-core`; orientation events now re-plan the live output rather than relying on F9. Actual framebuffer inspection covered portrait and the corrected 2340x1080 landscape launcher (`out/qemu-aarch64/agent/frame-portrait.png` and `frame-landscape.png`), with `output-rotated` sourced from `ferrix-core`. Weston remains only a selectable diagnostic fallback.
- **Product substrate (QEMU):** `just pmos-build` now installs the same `ferrix-core`, `ferrix-shell`, `ferrix-basic-screen`, strict profile, and required runtime libraries into the pmbootstrap-built postmarketOS rootfs. OpenRC starts the core before its health marker; a fresh TCG `just pmos-check` observed `product-core-ready`, `pmos-boot-health`, orderly shutdown, and power-off in 43 seconds. The pmOS manifest records product hashes. Its graphical startup path is installed but has not yet received a pmOS framebuffer validation; graphical evidence in this entry is from the Alpine smoke guest.
- **Reproducibility and CI:** Rust is pinned to 1.98.1 and the guest manifest records that version. Linux CI installs the native Smithay dependencies before all-feature clippy/tests. `just shell-nested` and its compatibility alias now launch the core and nested compositor together from the same profile. Validation passed: formatting, 78 workspace tests, macOS clippy, Linux AArch64 all-feature check, pinned guest build, `just qemu-check`, graphical QEMU framebuffer inspection, fresh `just pmos-build`, and `just pmos-check`.
- **Remaining limitations:** QEMU/mock evidence only, no physical-device claims. `ferrix-basic-screen` remains a reference product client and several app pages remain illustrative. Crash recovery, production Linux-service adapters, packaging the project as native postmarketOS packages, and pmOS graphical framebuffer validation remain future work.

## 2026-09-20 (Terminal app)

- **QEMU (Terminal app):** The Terminal tile in `ferrix-basic-screen` now opens a working shell instead of an illustrative card. The client spawns BusyBox `/bin/sh` on a PTY through `alacritty_terminal` 0.26 (Alacritty's VT parser/grid/PTY layer, Apache-2.0) and renders the grid with the existing tiny-skia/fontdue software path in Omarchy Ferrix's Tokyo Night ANSI palette (55x32 cells on the 1080x2340 panel). The client's Wayland loop moved to calloop (`smithay-client-toolkit` `calloop` feature) so the PTY reader thread wakes the UI via a ping source. The session survives Home/back navigation; Enter on an exited shell restarts it.
- **Input:** A touch-first on-screen keyboard (letters, `?123` symbols, shifted symbol row covering all printable ASCII, accessory row `esc tab ctrl ` \ | ← ↓ ↑ →`, sticky shift/ctrl) is part of the Terminal page and shares its geometry with the hit tests. A `virtio-keyboard-pci` device is now attached in graphical/agent sessions; raw `wl_keyboard` evdev codes go through a small hand-rolled US table (`keymap.rs`) because the static musl guest binary does not link libxkbcommon. `just qemu-type TEXT` drives it over QMP.
- **Evidence (QEMU, headless TCG agent session):** serial records `terminal-spawned` (cols 55, rows 32), `terminal-ready` with line `root@ferrix:~#`, `keyboard-input`, and `terminal-exited` (`exit code 130`). Framebuffers under `out/qemu-aarch64/agent/`: `terminal-open.png` (prompt), `terminal-osk-uname.png` (`uname -a` typed entirely by touch-key taps, real `Linux ferrix 6.6.142-0-virt ... aarch64` output), `terminal-keyboard.png` (`echo hw-ok; ls /` via `qemu-type`, colored `ls` output), `terminal-after-ctrlc.png` (sticky ctrl + `c` interrupts `sleep 100`, `^C` shown), `launcher-with-terminal-in-background.png` / `terminal-resumed.png` (session persists across Home), `terminal-exited.png` (yellow exit note), `terminal-restarted.png`. Host preview: `--preview out.ppm --page terminal` renders a canned transcript without a PTY. `cargo fmt --check`, clippy (host and `aarch64-unknown-linux-gnu`), 19 unit tests (OSK layout/coverage, keymap, snapshot colors, a real `/bin/sh` PTY round trip, terminal-page rendering, touch interaction), and `just qemu-check` pass.
- **Limitations:** No key repeat, no selection/copy, no touch scrollbar (drag-to-scroll only, shown as an "N lines up" badge), no bell/title handling, and the keyboard layout is US-only. Redraws are full-frame; under TCG each frame takes tens of milliseconds, which is fine for a reference surface. The product `ferrix-shell` now hosts the client by default; the terminal and on-screen keyboard still belong to the reference client rather than compositor-owned shell chrome. No physical-device claims.

## 2026-09-20

- **Layout (mock):** Swiping down from the status bar no longer changes the bar or notification-shade metrics on non-1080 panels. Shell pages previously multiplied layout by `s(1).max(1)`, which truncates to 1px on the 830-wide Cocoa QEMU window and jumped the status bar from 55px to 72px. Every page now uses the same multiply-then-divide scale as the launcher (`v * width / 1080`). Covered by a unit test at 830, 1080, and 1200 widths.
- **QEMU (basic screen):** The ASCII `weston-terminal` reference layout is replaced by `ferrix-basic-screen`, a Rust Wayland client (smithay-client-toolkit + tiny-skia + fontdue, software rendering only) that draws a Omarchy Ferrix-styled (Tokyo Night, JetBrains Mono) basic screen with a GrapheneOS-style status bar, clock, battery/network cards from the mock profile, a six-tile launcher grid, and a gesture pill. Verified by the actual rendered framebuffer at `out/qemu-aarch64/agent/frame.png` after the guest emitted `basic-screen-frame` and `wayland-ready` records on serial; `just qemu-check` still passes headlessly under TCG.
- **Build:** `just qemu-build` now also compiles the guest client for aarch64 musl in a pinned `rust` container (digest and binary hash recorded in the manifest). A host-side `--preview` mode renders the same composition to PPM without QEMU.
- **Input:** The reference surface accepts QEMU absolute-pointer and Wayland touch events with press feedback, six launcher app views, status-bar quick settings, Wi-Fi/Bluetooth mock toggles, a draggable brightness mock, notifications, Back/Home buttons, and top/left/bottom edge gestures. `just qemu-tap` and `just qemu-swipe` provide deterministic QMP input. Verified in QEMU through serial `input-ready`, `input-press`, and `input-release` records plus captured Settings, Notifications, and adjusted Quick Settings framebuffers under `out/qemu-aarch64/agent/`.
- **Pointer feedback (QEMU):** Virtual-tablet input uses a dedicated 32-pixel ARGB Wayland cursor: transparent center, Tokyo Night blue ring and halo, plus an opaque center dot at the exact hotspot. Real Wayland touch contacts remain cursorless. Verified in the actual QEMU framebuffer at `out/qemu-aarch64/agent/touch-cursor.png` with a serial `cursor-ready` record.
- **Window geometry (QEMU, Cocoa):** The on-screen QEMU window previously opened at the wrong size and aspect ratio because QEMU's Cocoa UI pushes its initial 640x480 view size (times backing scale) into virtio-gpu as the preferred mode, so the guest surface ignored `xres`/`yres`; a first attempt that only shrank `xres`/`yres` did not help. `qemu-run.sh` now sizes the panel from the host's visible screen height and backing scale, and the guest init pins Weston to that mode via a generated `weston.ini` modeline. Evidence on this 1920x1080-point Retina host: the window measures 415x932 points (900 content + title bar) at y=37, the guest emits `display-geometry` 830x1800 and `basic-screen-frame` 830x1800, Weston reports `830x1800@60.0, current`, a window capture shows the full portrait launcher, and `just qemu-tap 202 1236` opened `App(Settings)`. Headless agent sessions still render 1080x2340. `qmp.py` now accepts QEMU's row-padded PPM dumps (830-pixel rows carry two padding bytes). GTK backend sizing is unverified.
- **Limitations at that checkpoint:** The clock did not tick and app views were illustrative. Later work above replaced local-only quick-setting state, completed the Catacomb/Smithay decision, made `ferrix-shell` the default compositor, and connected rotation. Crash recovery and compositor-owned product chrome remain outstanding. Evidence is QEMU/mock only; no physical-device claims.

## 2026-09-16

- **mock:** The strict, schema-versioned QEMU profile parser and deterministic unplugged-to-charging transition are covered by a Rust unit test.
- **QEMU:** `just qemu-check` booted the generic AArch64 `virt` guest under TCG and observed serial boot-health plus the deterministic unplugged-to-charging mock event. The guest powers down cleanly after the smoke check.
- **QEMU (agent workflow):** `just qemu-agent-start`, `just qemu-frame`, `just qemu-tap`, and `just qemu-agent-stop` provide local QMP control, mandatory serial capture, and actual rendered-framebuffer evidence at `out/qemu-aarch64/agent/frame.png`. Readiness requires both the Wayland socket and a live, connected reference client. `qemu-frame` emits a machine-readable failure for an entirely blank framebuffer. The virtio display takes its 1080x2340 portrait geometry from the mock profile.

This historical checkpoint used Weston as diagnostic evidence. The current default and product foundation are the Rust `ferrix-shell` described above.

No physical-device testing or claims have been made.
