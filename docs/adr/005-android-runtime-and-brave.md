# ADR-005: Android compatibility runtime and the Brave reference app

Status: accepted and implemented for the opt-in QEMU postmarketOS route on
2026-09-21. Waydroid is the selected runtime; `docs/android-runtime.md` records
the pinned inputs, measured evidence, and remaining limitations. This is not a
physical-device or general Android compatibility claim.

**Superseded in part by [ADR-007](007-fulguris-browser-input.md) (2026-09-28):**
the Browser app is now Fulguris (`apps/org.ferrix.browser.json` launches
`net.slions.fulguris.full.fdroid`), acquired by `tools/android/fulguris-fetch.sh`
/ `fulguris-verify.sh` with pins in `tools/android/fulguris.pins`. The Brave
descriptor values, `tools/android/brave-fetch.sh` and `brave.pins` described
below no longer exist; they and the Brave runs are historical. The Waydroid
runtime decision and the adapter boundary still stand.

## Constraints the evaluation must measure

Upstream maintenance, AArch64 support, postmarketOS compatibility, Binder
requirements (binderfs on mainline), kernel configuration, container/process
isolation, Wayland integration, software-rendering correctness under QEMU
`virtio-gpu` **without** virgl or host GPU, input and on-screen keyboard,
audio, networking (virtio-net), lifecycle and crash reporting, data
persistence, permission mediation, APK install/update model, reproducible
builds, licensing, security-update path. Candidates: Waydroid, Anbox
(archived), redroid, Android-x86/Bliss derivatives, Android emulator/AVD
(excluded by baseline as the Linux-phone emulator).

## Boundary regardless of runtime

- `RuntimeKind::Android` is implemented by a **private** adapter crate
  `ferrix-runtime-android` behind `ferrix-app-runtime-api::Runtime`.
  `ferrix-appd` keeps identity, launch, activation, lifecycle, crash records,
  and surface association. The Android package and activity live only in the
  descriptor's daemon-private `launch` object
  (`{"package":"com.brave.browser","activity":"..."}`) and are never
  returned by `ferrix-app-protocol`.
- Descriptor: `apps/org.ferrix.browser.json`, `runtime: "android"`,
  `name: "Brave"`, `trust: "untrusted"`, capabilities `background` only.
  UI and docs say "Brave (Android application in the compatibility runtime)".
- The adapter shells out to the runtime's maintained CLI/API (for Waydroid:
  `waydroid` session/app commands and its container status), maps the
  Android window to the compositor surface by client pid, and classifies
  exits. It never speaks Binder.
- Absence or failure of the runtime is a typed `AppError::RuntimeUnsupported`
  or `LaunchFailed` with a reason; tests cover both.

## Brave acquisition

`tools/android/brave-fetch.sh` downloads **only** from the official Brave
GitHub release asset for a pinned tag, records tag and URL, verifies a pinned
SHA-256, uses Android SDK Build-Tools `apksigner verify --print-certs` to
validate the APK signing schemes, and pins the verified signer certificate's
SHA-256 fingerprint,
refuses redirects to other hosts, refuses unexpected content types or sizes,
stores the APK only under ignored `out/android/` (`FERRIX_OUT_DIR`), and
never auto-updates. Pins live in `tools/android/brave.pins`. A
`--local PATH` mode verifies a developer-supplied APK against the same pins.
Brave is MPL-2.0 (with Chromium's BSD-3 and others) and is a third-party
product; Ferrix code is Apache-2.0. Nothing from Brave is
committed. Tests cover wrong hash, wrong certificate, wrong host, missing
tools.

## Claim levels

Report by demonstrated level only: installable → launchable → graphical
(inspected framebuffer) → interactive → integrated. If the runtime cannot
render correctly through the software-compatible QEMU path, record the
limitation and escalate; do not claim completion.
