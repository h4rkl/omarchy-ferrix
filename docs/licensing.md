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
| Remix Icon v4.7.0 SVG subset (36 glyphs) | `crates/ferrix-ui/assets/icons/remixicon/` | Apache-2.0, verified against the upstream `License` file at the exact tag (see below). The upstream `License` is vendored beside the SVGs, exposed to code as `ferrix_ui::icons::REMIX_LICENSE`, installed in the QEMU guests (initramfs and postmarketOS) as `/usr/share/licenses/ferrix/remixicon-4.7.0/License`, and named in Settings > About. |
| `usvg`, `sha2` and their dependencies | Build-time only (`crates/ferrix-ui/build.rs`); not linked into any binary | MIT and/or Apache-2.0 (`usvg`, `kurbo`, `roxmltree`, `svgtypes`, `simplecss`, `euclid`, `data-url`, `polycool`, `siphasher`), MIT (`float-cmp`, `imagesize`, `pico-args`), BSD-3-Clause (`tiny-skia-path`). |
| Rust dependencies | Resolved by `Cargo.lock` and linked into the workspace | Each crate retains its upstream SPDX expression. The dependency set includes MIT, BSD, Zlib, and other compatible licenses in addition to Apache-2.0 options. |
| Fulguris Web Browser | Downloaded only into ignored `out/android/` by `tools/android/fulguris-fetch.sh` | MPL-2.0; bundled third-party components retain their own licenses. The F-Droid APK is never committed or relabeled. |
| Waydroid and Android images | Opt-in postmarketOS Android route | Waydroid is GPL-3.0; Android/LineageOS images include their own Apache-2.0, GPL-2.0, and other upstream components. They are external runtime inputs, not Ferrix source. |
| postmarketOS, Alpine, pmbootstrap, and guest system libraries | QEMU/postmarketOS build and runtime | Each project and package retains its upstream license. Ferrix invokes or packages these inputs but does not relicense them. |

### Remix Icon pin (decision 2026-10-10, issue #20)

Remix Icon changed to the custom, non-OSI "Remix Icon License v1.0" in
January 2026 (v4.9.0 onwards). That licence forbids, among other things, using
an icon as a logo or brand identifier; it is not MIT/Apache-2.0-compatible in
the sense `AGENTS.md` requires, so no release under it may be vendored.

Ferrix pins **v4.7.0**, the last release the issue identified as pre-change:

| Fact | Value |
| --- | --- |
| Tag | `v4.7.0`, commit `4ef50878f17accbed89ef2691bdc357562ed0a17` (2025-10-10) |
| `License` at that tag | Apache License, Version 2.0, January 2004 (full text); SHA-256 `c71d239df91726fc519c6eb72d318ec65820627232b2f796219e87dcf35d0ab4` |
| `package.json` at that tag | `"license": "Apache-2.0"` |
| Source | `https://github.com/Remix-Design/RemixIcon/tree/v4.7.0` (the `License` was read with `gh api repos/Remix-Design/RemixIcon/contents/License?ref=v4.7.0`) |
| Archive used for extraction | `https://codeload.github.com/Remix-Design/RemixIcon/tar.gz/refs/tags/v4.7.0`, SHA-256 `b2b610a4633d5b96bc0d215322190ad5ec84a3d1f1fc07ac18f56344900f2c17` on 2026-10-10 (GitHub does not promise byte-stable tag archives; the per-file hashes below are the pin) |
| Per-file pin | `crates/ferrix-ui/assets/icons/remixicon/SHA256SUMS`; `build.rs` refuses a mismatched or unlisted file |

v4.8.0's `License` is also still Apache-2.0, but it shipped days before the
change; v4.7.0 is the conservative pin the issue asked for. The v4.7.0 README
asks that the icons "are not for sale"; Ferrix does not sell them, and
`docs/design.md` additionally forbids using any glyph as a logo or brand mark.
Only the 36 SVGs Ferrix draws are vendored; the rest of the set (including
the `Logos` category) is not.

The Ferrix browser descriptor is first-party Apache-2.0 metadata; that does not
change the license of the Fulguris APK it identifies. Likewise, the Ferrix color
palette is an original adaptation and does not bundle the GPL-3.0 Space Monkey
theme configuration, artwork, wallpapers, or branding.

The license audit deliberately checks only work that Ferrix owns. It reports
third-party licenses for review rather than falsely claiming that upstream
software is Apache-2.0.
