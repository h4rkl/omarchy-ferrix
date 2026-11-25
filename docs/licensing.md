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
| IBM Plex Sans Condensed (Regular, SemiBold) and IBM Plex Mono (Regular, Bold) font files | `crates/ferrix-ui/assets/fonts/ibm-plex/`, compiled into every binary that links `ferrix-ui` | SIL Open Font License 1.1 (Reserved Font Name "Plex"), verified with the GitHub licence API and the upstream `license.txt` at the exact pinned commit (see below). Distributed unmodified. The upstream licence (`LICENSE.txt`) and a `README.md` notice are vendored beside the faces, exposed to code as `ferrix_ui::text::IBM_PLEX_LICENSE`, installed in the QEMU guests (initramfs and postmarketOS) and the Pixel dev compose under `/usr/share/licenses/ferrix/ibm-plex/`, and named in Settings > About. |
| Noto fallback fonts (Noto Sans, Sans Mono, Sans Arabic, Sans Devanagari: Regular and Bold; Noto Sans CJK Regular) | Not vendored. Installed in the QEMU initramfs from Alpine 3.20 `font-noto`, `font-noto-arabic`, `font-noto-devanagari`, `font-noto-cjk` (pruned to the listed files; versions and a tree hash are in the build manifest), and listed in the postmarketOS `extra_packages` for QEMU and the Pixel 9a. Loaded at run time from `/usr/share/fonts`. | SIL Open Font License 1.1 (Alpine package licence `OFL-1.1`). The copyright and licence notice travel inside each face's name table, which OFL §2 accepts; the files are redistributed unmodified as guest system packages. |
| `cosmic-text`, `swash`, `harfrust`, `skrifa`, `fontdb`, `unicode-bidi`, `unicode-script`, `unicode-segmentation` and their dependencies | Linked into `ferrix-ui` (text shaping, bidi, fallback, rasterisation) | MIT OR Apache-2.0 (`cosmic-text`, `swash`, `zeno`, `skrifa`, `read-fonts`, `font-types`, `ttf-parser`, `unicode-bidi`, `unicode-script`, `unicode-segmentation`, `linebender_resource_handle`, `smol_str`, `rangemap`, `sys-locale`, `memmap2`, `yazi`), MIT (`fontdb`, `harfrust`, `core_maths`), Apache-2.0 (`unicode-linebreak`), Zlib (`slotmap`), and `self_cell` under its Apache-2.0 option (Apache-2.0 OR GPL-2.0-only). |
| Google Material Symbols SVG subset (49 glyphs, Rounded, FILL 1) | `crates/ferrix-ui/assets/icons/material-symbols/` | Apache-2.0, verified with the GitHub licence API and the upstream `LICENSE` file at the exact pinned commit (see below). The upstream `LICENSE` and a `README.md` notice are vendored beside the SVGs, exposed to code as `ferrix_ui::icons::MATERIAL_SYMBOLS_LICENSE`, installed in the QEMU guests (initramfs and postmarketOS) under `/usr/share/licenses/ferrix/material-symbols/`, and named in Settings > About. |
| `usvg`, `sha2` and their dependencies | Build-time only (`crates/ferrix-ui/build.rs`); not linked into any binary | MIT and/or Apache-2.0 (`usvg`, `kurbo`, `roxmltree`, `svgtypes`, `simplecss`, `euclid`, `data-url`, `polycool`, `siphasher`), MIT (`float-cmp`, `imagesize`, `pico-args`), BSD-3-Clause (`tiny-skia-path`). |
| Rust dependencies | Resolved by `Cargo.lock` and linked into the workspace | Each crate retains its upstream SPDX expression. The dependency set includes MIT, BSD, Zlib, and other compatible licenses in addition to Apache-2.0 options. |
| LineageOS Jelly (the Browser) | Built into the Android system image (LineageOS 23.2 Ferrix build on the Pixel 9a route; `/product/app/Jelly/Jelly.apk` in the pinned Waydroid LineageOS 20 image on QEMU). No separate APK is fetched, staged, or committed. | Apache-2.0 (LineageOS); its notices travel with the image (Android's own Settings > Legal information) as Apache-2.0 §4 requires for redistribution. That is a distribution notice, not UI branding: Ferrix labels it "Browser" and does not show LineageOS or Jelly marks. |
| Waydroid and Android images | Opt-in postmarketOS Android route | Waydroid is GPL-3.0; Android/LineageOS images include their own Apache-2.0, GPL-2.0, and other upstream components. They are external runtime inputs, not Ferrix source. |
| postmarketOS, Alpine, pmbootstrap, and guest system libraries | QEMU/postmarketOS build and runtime | Each project and package retains its upstream license. Ferrix invokes or packages these inputs but does not relicense them. |

### IBM Plex pin (issue #14, 2026-11-24)

| Fact | Value |
| --- | --- |
| Repository | `https://github.com/IBM/plex` |
| Commit | `2e90fe366fdc27c12682db13e5aa20ded3d90908` (the `@ibm/plex-sans-condensed@2.3.1` release commit, master at 2026-11-18T17:42:42Z; Plex Mono there is `@ibm/plex-mono@2.5.0`) |
| Licence | OFL-1.1: `gh api repos/IBM/plex --jq .license.spdx_id` returned `OFL-1.1` on 2026-11-24; the vendored `LICENSE.txt` is `packages/plex-sans-condensed/fonts/complete/ttf/license.txt` at the commit (SHA-256 `91c25c350d3cac39da2736d74f7ba37ef648f5237a4e330a240615bc8d8c4360`) |
| Faces | `IBMPlexSansCondensed-Regular.ttf`, `IBMPlexSansCondensed-SemiBold.ttf`, `IBMPlexMono-Regular.ttf`, `IBMPlexMono-Bold.ttf` from `packages/<package>/fonts/complete/ttf/` |
| Pins | `tools/fonts/ibm-plex.pins`; per-file `crates/ferrix-ui/assets/fonts/ibm-plex/SHA256SUMS`, which `build.rs` enforces (a mismatched or unlisted `.ttf` fails the build) |
| Re-derivation | `just fonts-verify` downloads each file from `raw.githubusercontent.com` at the commit and compares it byte-for-byte; `just fonts-fetch` rewrites the vendored files for review. The build never uses the network. |

JetBrains Mono, the temporary single face before #14, is no longer vendored
or shipped.

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
