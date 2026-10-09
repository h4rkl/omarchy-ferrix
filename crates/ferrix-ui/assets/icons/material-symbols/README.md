# Material Symbols subset (third-party notice)

The SVG files in this directory are an unmodified subset of Google's
**Material Symbols**, copied from the `google/material-design-icons`
repository and licensed under the **Apache License, Version 2.0** (the
`LICENSE` file beside them is the upstream file at the pinned commit). They are
not Ferrix work. Ferrix uses them only as UI glyphs, never as a logo or brand
mark.

| Fact | Value |
| --- | --- |
| Source | <https://github.com/google/material-design-icons> |
| Commit | `49d4db35df873165d6bd6ba09b063c7dafbac2f4` (master, 2026-10-09T02:25:39Z) |
| Licence | Apache-2.0 (GitHub licence API, and `LICENSE` at that commit) |
| Style | Material Symbols **Rounded**, weight 400, grade 0, optical size 24, **FILL 1** |
| Upstream path | `symbols/web/<name>/materialsymbolsrounded/<name>_fill1_24px.svg`, vendored as `<name>.svg` |
| Pins | `tools/icons/material-symbols.pins`; per-file SHA-256 in `SHA256SUMS` (enforced by `crates/ferrix-ui/build.rs`) |
| Re-derive | `just icons-fetch` (rewrite) or `just icons-verify` (compare with the pinned commit, no changes) |

## Icons (48)

Launcher: `call`, `chat`, `contacts`, `settings`, `terminal`, `photo_camera`,
`language`, `home`; Files tile uses `folder`.

Status bar and quick settings: `signal_wifi_off`, `network_wifi_1_bar`,
`network_wifi_2_bar`, `signal_wifi_4_bar`, `signal_cellular_0_bar` to
`signal_cellular_4_bar`, `battery_android_0` to `battery_android_6`,
`battery_android_full`, `battery_android_bolt`, `bluetooth`, `notifications`,
`light_mode`, `keyboard`.

Navigation and actions: `chevron_left`, `chevron_right`, `more_horiz`, `add`,
`close`, `backspace`, `dialpad`, `history`.

Files entry types: `folder`, `draft`, `description`, `image`, `audio_file`,
`video_file`, `folder_zip`, `code_blocks`, `link`, `unknown_document`.

## Filled variants and fallbacks

Every icon above is the FILL 1 file; **no icon falls back to the outlined
(FILL 0) file**, because every one has a FILL 1 file at the pinned commit. For
27 of them the upstream FILL 1 file is byte-identical to FILL 0, because the
glyph has no separate filled form (strokes, chevrons, level meters whose lit
part is already solid): `add`, `battery_android_0` to `battery_android_6`,
`battery_android_full`, `bluetooth`, `chevron_left`, `chevron_right`, `close`,
`dialpad`, `history`, `language`, `link`, `more_horiz`, `network_wifi_1_bar`,
`network_wifi_2_bar`, `signal_cellular_0_bar` to `signal_cellular_4_bar`,
`signal_wifi_4_bar`, `terminal`.

Active and pressed states are shown with theme colour (`fg_dim`, `fg`, or the
region's single accent), never by switching fill.
