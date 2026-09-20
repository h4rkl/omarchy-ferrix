# Omarchy Ferrix visual system

This is the reference for how Omarchy Ferrix surfaces look, read, and are
themed. `ferrix-ui` implements it; `ferrix-basic-screen`, `ferrix-terminal`,
and future first-party surfaces consume it. `plans/baseline.md` stays the
implementation authority; this document does not change targets, guest
construction, or the hardware boundary. All rendered evidence is `QEMU` or
`mock`.

## Principles

- **Restrained, cohesive, low-noise.** Dark slate surfaces, steel type, and
  oxidized orange as the unmistakable Ferrix signature. Nothing animates or
  glows to attract attention.
- **Industrial.** Tight radii, hairline borders, thin rule lines, generous dark
  negative space. Panels read as machined parts, not cards floating on a
  gradient.
- **Low-light instrument panel.** The visual approach takes inspiration from
  the legibility of 1980s BMW dashboards: a non-reflective dark field, warm
  orange hierarchy, and a small luminance range that keeps controls readable
  in a dark room. It is an interaction principle, not copied vehicle branding
  or an imitation of a dashboard.
- **Terminal-first.** The Terminal uses a true monospace face and the ANSI
  palette is part of the system, not an afterthought.
- **Touch-first portrait.** 1080x2340 is the design width; every metric scales
  with `v * width / 1080`. Fullscreen focused flows, no desktop window
  management.
- **Software-rendered.** tiny-skia and fontdue into a plain buffer.
  Acceleration is never required for correctness, so effects that need it
  (blur, large shadows, alpha-heavy layering) are out.

## Palette

Values are `0xRRGGBB`. These are the only colors first-party surfaces use.

### Surfaces (slate)

| Token | Value | Role |
| --- | --- | --- |
| `bg` | `#15181c` | Main background. |
| `bg_dark` | `#0f1114` | Bars, keyboard wells, terminal grid background. |
| `surface` | `#1c2126` | Cards and tiles. |
| `surface_hi` | `#262c33` | Pressed, raised, and track fills. |
| `border` | `#353d46` | Hairlines. |

### Type (steel)

| Token | Value | Role |
| --- | --- | --- |
| `fg` | `#d7dde3` | Primary text. |
| `fg_dim` | `#7d8a96` | Secondary text, inactive glyphs. |

### Accents

| Token | Value | Role |
| --- | --- | --- |
| `rust` | `#d8642a` | The accent: focus, active toggles, pressed outlines, primary glyphs. |
| `rust_hi` | `#f0803f` | Brighter direct-action orange: pressed controls, cursor ring, and Terminal identity rule. |
| `rust_dim` | `#8a3f1a` | Deep Terminal tile, fill behind accent text, and quiet structural orange. |
| `steel` | `#8fa3b3` | Secondary accent: network glyphs, links, anything an app calls "blue". |

### Status (desaturated)

| Token | Value | Role |
| --- | --- | --- |
| `ok` | `#6fa287` | Charging, connected (verdigris). |
| `warn` | `#d9a441` | Warnings, exited-shell notes (amber). |
| `err` | `#cf5b4c` | Errors, crash banners (signal red). |

### Terminal ANSI

Normal 0-7, bright 8-15. Colors 0 and 15 match `bg` and the brightest
foreground so the grid sits flush with the rest of the surface.

| Index | Normal | Index | Bright |
| --- | --- | --- | --- |
| 0 | `#15181c` | 8 | `#4a5560` |
| 1 | `#cf5b4c` | 9 | `#e0705f` |
| 2 | `#7fa86a` | 10 | `#97c07f` |
| 3 | `#d9a441` | 11 | `#e8b95a` |
| 4 | `#7595b3` | 12 | `#8fb0cf` |
| 5 | `#b08aa0` | 13 | `#c7a1ba` |
| 6 | `#7fb2b0` | 14 | `#98cbc9` |
| 7 | `#c5ccd3` | 15 | `#e6ebef` |

### Icon tones

Application descriptors (`apps/*.json`) declare an `IconTone` from
`ferrix-app-api`. The enum is a stable contract and is not renamed; the theme
maps it:

| `IconTone` | Token |
| --- | --- |
| `Blue`, `Cyan` | `steel` |
| `Purple`, `Orange` | `rust` |
| `Yellow` | `warn` |
| `Green` | `ok` |
| `Red` | `err` |
| `Neutral` | `fg_dim` |

## Shape and type

- **Radii.** Cards and tiles 8-10 px at 1080 width; small chips 6 px. No pill
  shapes except the compositor home indicator.
- **Hairlines.** 1-2 px borders use `border`. Section emphasis uses a thin
  `rust_dim` rule; the Terminal identity rule may use `rust_hi`. Never use a
  filled header.
- **System face — IBM Plex Sans Condensed.** The compact, engineered forms
  give system labels the disciplined instrument-panel character without
  copying an automotive typeface. Use Regular for body/chrome and Medium or
  SemiBold for titles and numeric readouts.
- **Terminal face — IBM Plex Mono.** Its sober, highly differentiated glyphs
  suit a real shell and visually belong to the system face. Terminal cell
  metrics always come from this face; do not substitute a proportional
  fallback inside a grid.
- **Global fallback — Noto.** Fall back by script to Noto Sans UI (system) and
  Noto Sans Mono / Noto Sans Mono CJK (terminal where a fixed cell is
  available). Ship only the script packs selected by locale or language
  settings: Latin/Cyrillic/Greek first, then Arabic UI, Indic UI, CJK, and
  other Noto Sans script families on demand. This avoids pretending that a
  Latin-only display face serves every language.
- **Current implementation boundary.** The renderer currently embeds
  JetBrains Mono as a temporary single-face implementation and has no
  fallback-chain or complex-text shaping support. The font selections above
  become a shipped claim only with script fallback, bidi, and shaping support;
  until then non-Latin text must show an explicit missing-glyph fallback rather
  than silently presenting itself as supported.
- **Type.** Section labels are uppercase, letter-spaced, in `fg_dim` or
  `rust_dim` when they establish navigation. Numeric readouts (clock,
  percentages) are bold `fg`; white is never used as a large luminous field.
- **Orange hierarchy.** `rust_dim` may carry quiet structural rules and
  the Terminal tile. Reserve `rust`/`rust_hi` for its icon and direct focus
  and action: the focused tile, active toggle, cursor ring, or pressed
  control. The palette moves from dim → rust → rust_hi only with increasing
  interaction priority; this keeps orange a clear product signature without
  eye-straining bright fills.
- **Night readability.** Do not use gradients, bloom, large white areas, or
  competing saturated colors. Keep ambient information steel-gray, use orange
  to locate navigation and control state, and use status colors only when the
  state itself needs attention.
- **Negative space.** Prefer spacing over dividers. Dividers, when needed, are
  hairlines.
- **Status colors** appear only on the element they describe (a battery glyph,
  a crash banner), never as decoration.

## Copy

System-style lines: short, factual, evidence-labelled where state is mocked.
No greetings, no reassurance, no exclamation marks.

| Before | After |
| --- | --- |
| You're all caught up | No notifications |
| Settings are ready | Mock state · QEMU |
| Good morning / afternoon / evening | `ferrix · qemu-aarch64 · mock` |

Rules:

- Name the thing, then the state. Separate fields with ` · `.
- Say `mock` or `QEMU` when the value comes from the profile or the emulator.
  Never imply a physical device.
- Serial and JSON evidence records are a machine-readable contract. Their
  record names, keys, and values do not change for tone.
- Documentation follows the same register: state what exists, what it does
  not do, and how to verify it.

## Theming

Colors are loaded once at process start from a TOML file and exposed through
`ferrix_ui::theme`.

- **Canonical file:** `themes/ferrix/colors.toml`, checked in. A unit test
  asserts it parses to the built-in `Theme::ferrix()`.
- **Lookup order:** explicit path passed to `theme::init` → `$FERRIX_COLORS`
  → `/etc/ferrix/colors.toml` → built-in `Theme::ferrix()`.
- **Format:** the token names above, each `#rrggbb`, plus `ansi` as a list of
  sixteen colors. Unknown keys and malformed colors are errors.
- **Evidence:** `theme-loaded` with `source` = `builtin` or the resolved path;
  `theme-invalid` with the error when a file was present but rejected (the
  process then continues on the built-in theme).
- **Access:** `theme::get()` returns the loaded theme; if `init` was never
  called it returns the built-in.

This is a file contract. The desktop "Unified Theming" story is satisfied by
writing a compatible `colors.toml` to `/etc/ferrix/`; there is no live sync
service, file watcher, or desktop transport in this repository.

## What this pass implements

Implemented:

- The palette, ANSI colors, and icon-tone mapping above, replacing the
  earlier Tokyo Night values.
- The `colors.toml` theming contract, lookup order, and evidence records.
- The industrial shape language (radii, hairlines, rule lines) applied to the
  home surface, quick settings, notifications, and Terminal.
- The copy rules, applied to first-party surface strings.

Intentional departures from the marketing position, and future work:

- **Layout.** The launcher keeps a catalog tile grid restyled in this system.
  Micro-tiling layouts and terminal-first navigation are not part of the
  current launcher; the product shell stays focused-fullscreen and touch-first
  per `AGENTS.md`.
- **Theme sync.** A file contract, not a live sync service (above).
- **Everything else in the positioning** (agents, clipboard or history sync,
  key storage) is not implemented and is not claimed by any surface or
  document in this repository.

## UX resolution order

When a choice is not settled here, resolve it in this order, matching
`AGENTS.md`:

1. Explicit requirements in the task or in `plans/baseline.md`.
2. This visual system.
3. GrapheneOS interaction conventions, adapted to this system (status bar
   layout, quick-settings behavior, gesture semantics).

No external branding is copied. "Ferrix" is the product name only; palette,
type, and iconography are the project's own.
