# ADR-007: Fulguris browser APK input

Status: accepted for the opt-in QEMU postmarketOS route on 2026-09-28. This
ADR supersedes the browser choice in ADR-005; ADR-005's Brave runs remain
historical evidence and are not Fulguris validation.

## Decision

Keep Waydroid as the Android compatibility runtime, but replace the Brave APK
with the F-Droid build of Fulguris Web Browser:

- package: `net.slions.fulguris.full.fdroid`
- version: `2.0.10` / version code `261`
- APK: `net.slions.fulguris.full.fdroid_261.apk`
- source page: <https://f-droid.org/en/packages/net.slions.fulguris.full.fdroid/>
- direct download: <https://f-droid.org/repo/net.slions.fulguris.full.fdroid_261.apk>

The checked-in `tools/android/fulguris.pins` records the exact URL, size,
SHA-256, and F-Droid signing-certificate fingerprint. The APK is fetched only
from `f-droid.org`, verified with Android SDK Build-Tools `apksigner`, and
stored only under ignored `out/android/`. It is never auto-updated or
committed.

The stable Ferrix descriptor id remains `org.ferrix.browser`; only its
runtime-private Android package, display name, and APK pin path change. The
product bootstrap installs Fulguris when absent, assigns the Android browser
role, and verifies HTTPS resolution. `ferrix-appd` remains launch-only.

## Claim boundary

The replacement changes the packaged input and acquisition evidence. A fresh
QEMU run now reaches and has inspected the Fulguris onboarding framebuffer;
the prior Brave framebuffer evidence must not be relabeled as Fulguris
evidence, and this run is not a general APK compatibility claim. Interactive
advancement beyond the onboarding surface remains partially unverified.

Fulguris is MPL-2.0. Its third-party components retain their upstream licenses;
Ferrix metadata and code remain Apache-2.0.
