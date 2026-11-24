# IBM Plex faces (third-party notice)

The font files in this directory are unmodified copies of four
**IBM Plex** faces from the `IBM/plex` repository, licensed under the
**SIL Open Font License, Version 1.1** (the `LICENSE.txt` file beside them is
the upstream `license.txt` at the pinned commit). They are not Ferrix work.
"Plex" is a Reserved Font Name: these files are distributed unmodified, and
any modified version would have to be renamed.

| Fact | Value |
| --- | --- |
| Source | <https://github.com/IBM/plex> |
| Commit | `2e90fe366fdc27c12682db13e5aa20ded3d90908` (`@ibm/plex-sans-condensed@2.3.1`; master at 2026-11-18T17:42:42Z) |
| Licence | OFL-1.1 (GitHub licence API, and `license.txt` at that commit) |
| Upstream path | `packages/<package>/fonts/complete/ttf/<file>` |
| Pins | `tools/fonts/ibm-plex.pins`; per-file SHA-256 in `SHA256SUMS` (enforced by `crates/ferrix-ui/build.rs`) |
| Re-derive | `just fonts-fetch` (rewrite) or `just fonts-verify` (compare with the pinned commit, no changes) |

## Faces (4)

- `IBMPlexSansCondensed-Regular.ttf`: body text and chrome (`Face::Sans`, `Weight::Regular`).
- `IBMPlexSansCondensed-SemiBold.ttf`: titles and readouts (`Face::Sans`, `Weight::Bold`).
- `IBMPlexMono-Regular.ttf`: terminal cells and source text (`Face::Mono`, `Weight::Regular`).
- `IBMPlexMono-Bold.ttf`: bold terminal cells (`Face::Mono`, `Weight::Bold`).

Every Ferrix binary that links `ferrix-ui` embeds these files. The licence
travels with them: the QEMU guests install it as
`/usr/share/licenses/ferrix/ibm-plex/LICENSE.txt`, it is exposed to code as
`ferrix_ui::text::IBM_PLEX_LICENSE`, and Settings > About names it.

Scripts Plex does not cover come from the guest's Noto packages (also
OFL-1.1, not vendored here); see `docs/licensing.md`.
