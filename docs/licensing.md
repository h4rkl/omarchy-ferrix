# Licensing

Omarchy Ferrix's first-party work is licensed under the Apache License,
Version 2.0. The canonical text is [`LICENSE`](../LICENSE).

This first-party scope includes:

- all Rust crates and binaries in `crates/`;
- the Ferrix application software and the application descriptors in `apps/`;
- repository-owned scripts, configuration, test fixtures, themes, and
  documentation, except where a file is explicitly identified below as
  third-party material.

Every workspace package declares exactly `Apache-2.0` through
`[workspace.package]`. Run `just license-check` to verify the package metadata,
the canonical license file, and the checked-in application descriptors.

## Third-party material and software inputs

Third-party work keeps its upstream license. It is not possible or lawful to
change these licenses to Apache-2.0 merely by changing Ferrix metadata:

| Material or software | Location or use | Upstream license boundary |
| --- | --- | --- |
| JetBrains Mono font files | `crates/ferrix-ui/assets/JetBrainsMono-*.ttf` | SIL Open Font License 1.1; the accompanying `OFL.txt` is retained. |
| Rust dependencies | Resolved by `Cargo.lock` and linked into the workspace | Each crate retains its upstream SPDX expression. The dependency set includes MIT, BSD, Zlib, and other compatible licenses in addition to Apache-2.0 options. |
| Fulguris Web Browser | Downloaded only into ignored `out/android/` by `tools/android/fulguris-fetch.sh` | MPL-2.0; bundled third-party components retain their own licenses. The F-Droid APK is never committed or relabeled. |
| Waydroid and Android images | Opt-in postmarketOS Android route | Waydroid is GPL-3.0; Android/LineageOS images include their own Apache-2.0, GPL-2.0, and other upstream components. They are external runtime inputs, not Ferrix source. |
| postmarketOS, Alpine, pmbootstrap, and guest system libraries | QEMU/postmarketOS build and runtime | Each project and package retains its upstream license. Ferrix invokes or packages these inputs but does not relicense them. |

The Ferrix browser descriptor is first-party Apache-2.0 metadata; that does not
change the license of the Fulguris APK it identifies. Likewise, the Ferrix color
palette is an original adaptation and does not bundle the GPL-3.0 Space Monkey
theme configuration, artwork, wallpapers, or branding.

The license audit deliberately checks only work that Ferrix owns. It reports
third-party licenses for review rather than falsely claiming that upstream
software is Apache-2.0.
