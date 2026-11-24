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
| JetBrains Mono font files | `crates/ferrix-ui/assets/JetBrainsMono-*.ttf` | SIL Open Font License 1.1; the accompanying `OFL.txt` is retained and installed in the QEMU guests as `/usr/share/licenses/ferrix/jetbrains-mono/OFL.txt`. |
| Google Material Symbols SVG subset (49 glyphs, Rounded, FILL 1) | `crates/ferrix-ui/assets/icons/material-symbols/` | Apache-2.0, verified with the GitHub licence API and the upstream `LICENSE` file at the exact pinned commit (see below). The upstream `LICENSE` and a `README.md` notice are vendored beside the SVGs, exposed to code as `ferrix_ui::icons::MATERIAL_SYMBOLS_LICENSE`, installed in the QEMU guests (initramfs and postmarketOS) under `/usr/share/licenses/ferrix/material-symbols/`, and named in Settings > About. |
| `usvg`, `sha2` and their dependencies | Build-time only (`crates/ferrix-ui/build.rs`); not linked into any binary | MIT and/or Apache-2.0 (`usvg`, `kurbo`, `roxmltree`, `svgtypes`, `simplecss`, `euclid`, `data-url`, `polycool`, `siphasher`), MIT (`float-cmp`, `imagesize`, `pico-args`), BSD-3-Clause (`tiny-skia-path`). |
| Rust dependencies | Resolved by `Cargo.lock` and linked into the workspace | Each crate retains its upstream SPDX expression. The dependency set includes MIT, BSD, Zlib, and other compatible licenses in addition to Apache-2.0 options. |
| LineageOS Jelly (the Browser) | Built into the Android system image (LineageOS 23.2 Ferrix build on the Pixel 9a route; `/product/app/Jelly/Jelly.apk` in the pinned Waydroid LineageOS 20 image on QEMU). No separate APK is fetched, staged, or committed. | Apache-2.0 (LineageOS); its notices travel with the image (Android's own Settings > Legal information) as Apache-2.0 §4 requires for redistribution. That is a distribution notice, not UI branding: Ferrix labels it "Browser" and does not show LineageOS or Jelly marks. |
| Waydroid and Android images | Opt-in postmarketOS Android route | Waydroid is GPL-3.0; Android/LineageOS images include their own Apache-2.0, GPL-2.0, and other upstream components. They are external runtime inputs, not Ferrix source. |
| postmarketOS, Alpine, pmbootstrap, and guest system libraries | QEMU/postmarketOS build and runtime | Each project and package retains its upstream license. Ferrix invokes or packages these inputs but does not relicense them. |

### Material Symbols pin (decision 2026-10-10, issue #20)

Remix Icon was pinned first (v4.7.0, the last Apache-2.0 tag), but its
project moved to the custom, non-OSI "Remix Icon License v1.0" in January
2026. The owner chose Google Material Symbols instead, so no Remix file is
vendored any more.

| Fact | Value |
| --- | --- |
| Repository | `https://github.com/google/material-design-icons` |
| Commit | `49d4db35df873165d6bd6ba09b063c7dafbac2f4` (master, 2026-10-09T02:25:39Z) |
| Licence | Apache-2.0: `gh api repos/google/material-design-icons --jq .license.spdx_id` returned `Apache-2.0` on 2026-10-10, and the root `LICENSE` at the commit is the full Apache License, Version 2.0 text (SHA-256 `58d1e17ffe5109a7ae296caafcadfdbe6a7d176f0bc4ab01e12a689b0499d8bd`) |
| Style | Material Symbols Rounded, weight 400, grade 0, optical size 24, FILL 1 (`symbols/web/<name>/materialsymbolsrounded/<name>_fill1_24px.svg`) |
| Pins | `tools/icons/material-symbols.pins` (commit, style, icon list); per-file `crates/ferrix-ui/assets/icons/material-symbols/SHA256SUMS`, which `build.rs` enforces (a mismatched or unlisted SVG fails the build) |
| Re-derivation | `just icons-verify` downloads each file from `raw.githubusercontent.com` at the commit and compares it byte-for-byte; `just icons-fetch` rewrites the vendored files for review. The build never uses the network. |

Only the 48 SVGs Ferrix draws are vendored. Every one is the FILL 1 file; none
falls back to outlined. The glyphs are used only as UI symbols, never as a
logo or brand mark.

The Ferrix browser descriptor is first-party Apache-2.0 metadata; that does not
change the license of the Android package it identifies (LineageOS Jelly).
Fulguris (MPL-2.0), the Browser before ADR-008, is no longer fetched or
shipped. Likewise, the Ferrix color
palette is an original adaptation and does not bundle the GPL-3.0 Space Monkey
theme configuration, artwork, wallpapers, or branding.

The license audit deliberately checks only work that Ferrix owns. It reports
third-party licenses for review rather than falsely claiming that upstream
software is Apache-2.0.
