# Omarchy Ferrix visual system

This document is the design authority for first-party Omarchy Ferrix surfaces.
It defines how the system looks, reads, and behaves; `ferrix-ui` implements the
shared primitives, and first-party shells and applications consume them.

`plans/baseline.md` remains the implementation authority. This document does
not alter targets, guest construction, hardware boundaries, or the distinction
between the product shell and diagnostic surfaces. Rendered evidence must be
labelled `QEMU`, `mock`, or with the exact admitted physical target.

## Design thesis

Omarchy Ferrix is a low-light mobile instrument: black, warm, direct, and
quiet. It is for someone operating a developer-grade Linux phone who needs to
understand system state and find the next action at a glance.

**Space Monkey supplies the signal colors. BMW-inspired cockpit design
supplies the discipline. Ferrix turns both into a touch-first system.**

The chassis is black. Amber readouts, rust actions, teal information, and rare
semantic color sit on top of it. This keeps Space Monkey's character without
washing the entire interface in brown. It is not a nostalgic desktop skin or
dashboard skeuomorphism, and it does not copy external logos, vehicle controls,
typefaces, iconography, wallpaper, or branding.

## Design dock

Use this sequence whenever a first-party surface is added or changed:

1. **Name the task.** What is the person trying to learn or do here?
2. **Choose the lead signal.** Give the most important state or next action the
   strongest position, type scale, or action treatment.
3. **Group supporting state.** Put related information into one readable
   cluster instead of distributing equal-weight cards across the screen.
4. **Spend one accent.** A region gets one rust action cue. Teal may carry
   passive information, but it must not compete for action priority.
5. **Remove one element.** If a divider, label, color, or animation does not
   clarify state or action, remove it.

### Source and adaptation

The signal palette is selectively adapted from
[Ty Richards' Space Monkey theme](https://github.com/TyRichards/omarchy-space-monkey-theme/)
at commit `814af8923d701e779d2894bd1b2b1d4cbec820ff`. Space Monkey's amber,
rust, teal, violet, and pink relationships are the visual reference. Its
GPL-3.0 desktop configuration, artwork, wallpapers, and branding are not
bundled.

The adaptation is intentional:

- Ferrix replaces the theme's brown field with black and warm charcoal. Brown
  survives only as a trace in raised surfaces and the rust action family.
- Space Monkey's bright gold desktop selection becomes a dark charcoal
  `surface_hi`, because a full mobile tile must remain comfortable in low
  light.
- Ferrix adds semantic action, information, success, warning, and error roles
  so state is consistent across independent processes.
- The existing token name `steel` remains stable in code, but its current
  visual role is Space Monkey teal.

### BMW-inspired philosophy

BMW is a usability reference, not a styling kit:

- **Oriented toward the operator.** The next action occupies the natural visual
  and thumb path; secondary controls recede.
- **Readable at a glance.** Numeric state, status, and warnings use distinct
  positions and intensities rather than ornamental containers.
- **Stable controls.** Controls do not move when their value changes. A person
  learns the surface spatially.
- **Progressive disclosure.** The default view shows operational state;
  adjustment and detail appear only when requested.
- **Tactile response without imitation.** Pressed fills, outlines, and short
  state changes communicate contact. Faux switches, gauges, chrome, and carbon
  fibre do not belong here.

## Signature: the instrument rail

The instrument rail is the system's one distinctive graphic device: a thin
warm rule that ties a title, active destination, or primary readout to its
control context. It may appear beneath a status strip, beside a crash notice,
or above the Terminal identity area.

- Use `rust_dim` for structure and `rust_hi` only when the rail identifies the
  currently actionable region.
- Keep it 1–2 px at the 1080 px design width.
- Use at most one rail per region. It never becomes a frame, glow, underline
  on every label, or decorative grid.

The rail is where the design spends its boldness. Everything around it stays
quiet.

## Core principles

- **Black chassis, warm instruments.** Black carries the field, amber carries
  the reading order, rust carries action, and teal carries information.
- **Touch-first portrait.** The reference canvas is 1080×2340. Fullscreen,
  focused flows take priority over desktop window-management patterns.
- **Terminal-first, not terminal-only.** Terminal typography and ANSI color are
  first-class, while ordinary applications remain clear to non-shell users.
- **Software-rendered by default.** Correctness cannot require blur, large
  shadows, translucency stacks, or GPU effects.
- **State before decoration.** Color, type, spacing, and rules explain current
  state or available action. They do not fill empty space.
- **Calm failure.** Errors become specific, local, and recoverable. Alarm color
  is reserved for the element that needs attention.

## Palette

Values are `0xRRGGBB`. The canonical values live in
`themes/ferrix/colors.toml`; the built-in `Theme::ferrix()` must remain exactly
equivalent.

### Chassis and surfaces

| Token | Value | Role |
| --- | --- | --- |
| `bg` | `#000000` | Main black field. |
| `bg_dark` | `#000000` | Status strips, keyboard wells, terminal grid. |
| `surface` | `#171311` | Cards, rows, and launcher tiles. |
| `surface_hi` | `#2a211b` | Pressed and raised surfaces; inactive tracks. |
| `border` | `#6f6868` | Quiet warm-stone hairlines and neutral outlines. |

### Type and semantic color

| Token | Value | Role |
| --- | --- | --- |
| `fg` | `#e7aa5a` | Primary text and numeric readouts. |
| `fg_dim` | `#948a8b` | Secondary text and inactive glyphs. |
| `rust` | `#bd4924` | Focus, active toggles, action outlines, primary glyphs. |
| `rust_hi` | `#df782d` | Direct manipulation, pressed action, cursor ring. |
| `rust_dim` | `#6b2411` | Instrument rails, quiet action structure, Terminal tile. |
| `steel` | `#66a891` | Teal information, network state, links, descriptor blue. |
| `ok` | `#66a891` | Charging, connected, completed successfully. |
| `warn` | `#f9cc6c` | Degraded state and action required soon. |
| `err` | `#fd6883` | Failed action, crash, destructive consequence. |

### Color budget

- Most pixels belong to `bg`, `bg_dark`, and `surface`.
- Amber establishes reading order; it is not a large-area fill.
- Rust is scarce: one action cue per region, strengthened from `rust_dim` to
  `rust` to `rust_hi` as interaction priority increases.
- Teal describes passive information and connection. It never becomes the
  primary action color.
- Warning and error colors appear only when those states exist.
- Pure white is not part of the first-party UI palette. Pure black is reserved
  for recessed areas, not used to flatten all surface depth.

### Terminal ANSI

Normal colors are indices 0–7; bright colors are 8–15. Index 0 matches `bg`,
and index 15 is the brightest foreground, so the terminal belongs to the same
visual system without flattening command output into one hue.

| Index | Normal | Index | Bright |
| --- | --- | --- | --- |
| 0 | `#000000` | 8 | `#6f6868` |
| 1 | `#fd6883` | 9 | `#ff8297` |
| 2 | `#df782d` | 10 | `#c8e292` |
| 3 | `#f9cc6c` | 11 | `#fcd675` |
| 4 | `#66a891` | 12 | `#f8a788` |
| 5 | `#a8a9eb` | 13 | `#bebffd` |
| 6 | `#bd4924` | 14 | `#9bf1e1` |
| 7 | `#e6d9db` | 15 | `#f1e5e7` |

### Descriptor tone mapping

Application descriptors declare the stable `IconTone` enum from
`ferrix-app-api`. Theme language must not leak into that public contract.

| `IconTone` | Rendered token |
| --- | --- |
| `Blue`, `Cyan` | `steel` |
| `Purple`, `Orange` | `rust` |
| `Yellow` | `warn` |
| `Green` | `ok` |
| `Red` | `err` |
| `Neutral` | `fg_dim` |

## Contrast and accessibility

- Text, icons, shape, and wording work together. Color alone never
  communicates focus, success, warning, failure, charging, or connectivity.
- Primary text uses `fg`; `fg_dim` is limited to supporting text, metadata, and
  inactive controls at a size that remains legible.
- A border is structure, not state. Active controls also change label, glyph,
  outline weight, or adjacent wording.
- Every interactive element has visible default, focused, pressed, active,
  disabled, and error treatment where those states apply.
- Touch targets stay at least 48 logical pixels on the final scaled surface;
  visual glyphs may be smaller inside them.
- The UI must remain understandable in monochrome inspection and under common
  color-vision deficiencies.
- Missing glyphs, truncated values, and unavailable services are explicit
  states, never silent disappearance.

## Layout

All metrics scale from the 1080 px reference width using
`scaled = value * width / 1080`. Multiply before dividing so narrow surfaces do
not jump to unscaled values.

### Portrait hierarchy

```text
┌────────────────────────────────┐
│ status / immediate system state│
├────────────────────────────────┤  instrument rail
│                                │
│ primary readout or task title  │
│ supporting identity / context  │
│                                │
│ grouped operational state      │
│                                │
│ focused content or controls    │
│                                │
│ contextual action / explanation│
├────────────────────────────────┤
│ trusted home gesture zone      │
└────────────────────────────────┘
```

- Keep the status strip stable across first-party surfaces.
- Place the primary readout or task title above operational detail.
- Prefer one focused content column. Launcher tiles may use a grid because the
  catalog is a peer set; settings and task flows should not.
- Preserve the compositor-owned bottom gesture zone. Application content and
  hit targets must not compete with it.

### Landscape adaptation

Landscape is a reflow, not a scaled-down portrait screenshot. Put the primary
state or navigation cluster in the leading region and the active content in
the wider trailing region. Do not create more hierarchy levels merely because
horizontal space exists.

### Spacing and geometry

- Cards and tiles use 8–10 px radii at reference width; compact chips and back
  controls use 6 px. Only the trusted home indicator is pill-shaped.
- Hairlines are 1–2 px and never truncate below one physical pixel.
- Prefer negative space over dividers. When a divider is necessary, use one
  hairline or one instrument rail, not both.
- Align related labels and values to a shared edge. Avoid centered body copy;
  centered alignment is reserved for a singular readout or empty state.
- No gradient, bloom, glass, large drop shadow, or decorative translucency.

## Typography

### Intended family

- **System — IBM Plex Sans Condensed.** Regular for body and chrome; Medium or
  SemiBold for titles and numeric readouts. Its compact engineered forms suit
  the instrument character without copying an automotive face.
- **Terminal — IBM Plex Mono.** Terminal cell metrics always come from a true
  monospace face.
- **Fallback — Noto.** Resolve by script to Noto Sans UI and, where fixed cells
  are available, Noto Sans Mono or Noto Sans Mono CJK. Ship locale-selected
  packs rather than pretending one Latin face supports every language.

### Hierarchy

- **Readout:** time, percentage, call duration, or the one value anchoring the
  page. Bold, tabular where available, and visually dominant.
- **Title:** the task or destination, not a slogan.
- **Body:** current state, explanation, or content.
- **Utility:** short uppercase navigation labels and compact metadata, with
  restrained tracking.
- **Terminal:** grid-bound monospace text; never substitute proportional text
  inside terminal cells.

White is never used as a large luminous field. Uppercase is a utility voice,
not a body-copy style.

### Current implementation boundary

The renderer currently embeds JetBrains Mono as a temporary single-family
implementation. It has no fallback chain, bidirectional layout, or complex-text
shaping. IBM Plex and Noto are design selections, not shipped claims, until
font packaging, shaping, fallback, and script-specific graphical evidence
exist. Unsupported text must expose a missing-glyph state honestly.

## Icons

First-party glyphs come from one set: a pinned subset of
[Remix Icon](https://github.com/Remix-Design/RemixIcon/tree/v4.7.0) v4.7.0
(Apache-2.0; pin and licence evidence in `docs/licensing.md`). `ferrix-ui`
compiles the vendored SVGs into outline data at build time (`build.rs`, with
hash-pinned inputs) and fills them through the same `tiny-skia` software path
as every other shape, at the exact pixel size requested. There is no run-time
SVG parsing, icon font, file, or network dependency. The API is
`ferrix_ui::icons::{Icon, draw, draw_centered, draw_in}`.

Rules:

- **Filled always (owner rule, 2026-10-10).** Use the filled variant (`*-fill`)
  wherever Remix has one, in every state, including inactive ones. Inactive
  versus active is shown with colour, never by switching between line and
  fill glyphs.
- **Colour by state.** `fg_dim` for inactive or supporting glyphs, `fg` for
  primary content (folders, the backspace key), `rust` for the active or
  actionable state (an on toggle, the back affordance), `steel` for network
  and connection state, the descriptor tone on launcher tiles, and `ok`/`err`
  for charging and low battery. The colour budget above still applies.
- **Additive, never a substitute.** Every glyph sits beside the text label or
  accessible name that already existed (tile names, toggle names, tab labels,
  `back`/`delete` targets in `ui-state`). Glyphs never carry state on their
  own (`docs/design.md` contrast rules).
- **UI symbols only.** Never use a glyph as a logo, brand mark, or product
  identity (the Remix `Logos` category is not vendored). Ferrix remains the
  only product identity.
- **Sizes at the 1080 px basis:** 40 px in the 72 px status strip, 88 px back
  affordance box (the chevron itself is about 22x44 px), 56 px leading row
  glyphs, 72 px quick-settings glyphs, 96 px launcher glyphs inside about
  290x290 px tiles. Scale with `value * basis / 1080` like every other metric.
- **The home indicator stays a pill.** It is the one pill shape in the system
  and is not an icon.

Subset (Remix name): launcher `phone`, `message-2`, `contacts-book`,
`settings-3`, `terminal-box`, `camera`, `folder` (Files), `global` (Browser),
`home-5`; status `signal-wifi-off`,
`signal-wifi-1`, `signal-wifi-2`, `signal-wifi`, `battery`, `battery-low`,
`battery-charge`; quick settings, notifications and Settings `bluetooth`, `sun`, `keyboard-box`,
`notification-3`; navigation and actions `arrow-left-s` (back),
`arrow-right-s` (forward chevron, breadcrumb separator), `more` (row
details), `add`, `close`, `delete-back-2` (backspace), `grid` (Dialer tab),
`history` (Recent tab); Files types `folder`, `file`, `file-text`,
`file-image`, `file-music`, `file-video`, `file-zip`, `file-code`, `links`,
`file-unknow`. All are `-fill` variants.

Exceptions and departures:

- **No line glyphs are used.** Every glyph in the subset has a filled variant,
  so there is no line/outline exception today. Remix's filled `arrow-left-s`
  and `arrow-right-s` are solid triangles rather than open chevrons; that is
  the accepted consequence of the filled rule.
- **Cellular signal meter:** Remix v4.7.0 has no cellular signal-level glyphs,
  so the status strip keeps its drawn four-bar meter (`steel` lit, `surface_hi`
  unlit).
- **Battery levels:** Remix's half-level `battery-2` is a portrait battery that
  does not match the others, so only full, low (<20 %), and charging glyphs are
  used; the percentage is always printed beside the glyph.
- **On-screen keyboard and Terminal:** key caps keep their text labels
  (`⌫`, `←`, `→`, `SHIFT`); the keyboard is a text instrument and its labels
  are shared with the terminal's key mapping. Only the Phone dialer's
  backspace button uses the `delete-back-2` glyph.
- **Not drawn yet:** `home-5` and `close` are pinned for the home and
  dismiss controls but no current surface draws a home button or a close
  control; the home gesture stays a pill.
- **Third-party apps** without a first-party glyph keep their descriptor glyph
  or first letter on the launcher tile.

## Components and states

### Status strip

Show only immediately useful system state: time, connectivity, and power. Use
stable positions and compact glyphs. Opening quick settings expands this state;
the strip itself does not become a second launcher.

### Operational card

Lead with the value, follow with the state, then its evidence label when
mocked. A card groups one subject. It does not collect unrelated metrics merely
to balance the layout.

### Launcher tile

The glyph and label identify the application. Running, starting, and crashed
states add a compact state cue without moving the tile. The catalog gets one
rust anchor—the Terminal—not a rainbow of equal accents.

### Rows and controls

Rows use a stable label edge and place the current value or affordance opposite
it. Active toggles keep the black surface and gain rust outline plus a textual
or glyph state. Disabled controls remain legible and state why they cannot act.

### Notices and errors

A notice uses a local instrument rail or semantic outline, a factual reason,
and the next available action. It never takes over the entire screen unless the
screen genuinely cannot continue.

### On-screen keyboard

The keyboard is an instrument bed: `bg_dark` well, `surface` character keys,
darker function keys, and rust only for pressed or latched state. Key geometry
and hit-test geometry are the same data.

Letter rows are data (`crates/ferrix-ui/layouts/*.toml`); a layout wider than
ten letters narrows every letter key evenly rather than shrinking one row.
Held backspace, arrows, and space repeat; letters do not (a held letter is
not a request for `aaaa`).

### Terminal selection, scrollbar, and bell

Selection is passive state, so it is teal: selected cells take a `steel`
wash over the grid well, the two drag handles are `steel` drops, and the
transient scrollbar thumb is `steel` on a `surface_hi` track. While a
selection exists the block cursor drops to `fg_dim` and the Copy action
carries the region's one rust cue. The visual bell is a brief `warn` outline
of the grid well (the shell asked for attention), never a full-screen flash.
A program's OSC 0/2 title is the header's status line, not the page title.

### Interaction state matrix

| State | Surface | Outline / rail | Content |
| --- | --- | --- | --- |
| Default | `surface` | `border` hairline | `fg` or `fg_dim` |
| Focused | `surface` | `rust` | `fg`; explicit focus target |
| Pressed | `surface_hi` | heavier `rust` | action label or glyph |
| Active | `surface` | `rust` | state word and active glyph |
| Disabled | `bg` or `surface` | quiet `border` | `fg_dim` plus reason when needed |
| Warning | normal surface | local `warn` cue | specific warning text |
| Error | normal surface | local `err` cue | failure and recovery action |

## Motion and feedback

Current correctness does not depend on animation. Immediate pressed-state
feedback is required; decorative motion is not.

Future motion must:

- explain a state transition, spatial relationship, or gesture result;
- remain short and interruptible;
- avoid continuous ambient movement;
- honor reduced-motion settings by removing travel rather than merely speeding
  it up;
- leave the final state fully legible when animation is unavailable.

## Copy

The voice is short, factual, and calm. Write from the person's side of the
screen: name what they can recognize and what they can do next.

| Avoid | Use |
| --- | --- |
| You're all caught up | No notifications |
| Settings are ready | Mock state · QEMU |
| Good morning | `ferrix · qemu-aarch64 · mock` |
| Something went wrong | Network unavailable · retry |
| Submit | Save changes |

Rules:

- Name the thing, then the state. Separate compact fields with ` · `.
- Use the same verb before and after an action: `Save changes` becomes
  `Changes saved`.
- Empty states explain the available action; errors state the cause and
  recovery route when known.
- Say `mock` or `QEMU` when state comes from the emulator. Never imply physical
  hardware evidence.
- Serial and JSON evidence are contracts. Their keys and values do not change
  for tone.
- No greetings, apologies, reassurance, exclamation marks, or promotional
  copy in system surfaces.

## Theming contract

Colors load once at process start through `ferrix_ui::theme`.

- **Canonical file:** `themes/ferrix/colors.toml`.
- **Equivalence:** a unit test asserts that the canonical file parses to the
  built-in `Theme::ferrix()`.
- **Lookup order:** explicit path passed to `theme::init` → `$FERRIX_COLORS` →
  `/etc/ferrix/colors.toml` → built-in theme.
- **Desktop-compatible keys:** `accent`, `selection`, `muted`, the background
  and foreground families, and named normal/bright terminal colors. Supported
  unused desktop keys are accepted and ignored.
- **Ferrix extension:** optional `[ferrix]` keys are `steel`, `ok`, `rust_hi`,
  `rust_dim`, and `bright_black`, each with a documented desktop-key fallback.
- **Schema:** optional `schema_version`; only version 1 is accepted.
- **Evidence:** `theme-loaded` names the built-in or resolved file;
  `theme-invalid` names a rejected configured source before falling back.

This is a file contract, not a live synchronization service. A desktop theme
can be previewed through the compatible keys, but the Ferrix default remains
the adapted and tested mobile palette above.

## Implementation and evidence

Implemented today:

- shared palette, ANSI mapping, descriptor-tone mapping, and theme loading;
- software-rendered cards, rows, fields, keyboard, launcher, status surfaces,
  quick settings, notifications, and Terminal;
- tight radii, hairlines, instrument rails, focused fullscreen flows, and
  concise evidence-labelled copy;
- the pinned filled Remix Icon glyph set on the launcher, status strip, quick
  settings, notifications, page headers (back), Files rows and breadcrumbs,
  Settings rows, and the Phone tabs and backspace key.

Selected but not yet shipped:

- IBM Plex and Noto font stack;
- script shaping, bidirectional layout, and fallback-chain support;
- compositor-owned final shell chrome and any future motion system.

Every graphical change must be exercised through the documented preview or
nested workflow, then captured and inspected in the actual QEMU framebuffer
before `docs/status.md` makes a rendered claim. A process-start record alone is
not visual evidence. QEMU evidence never establishes physical-device support.

## UX resolution order

Resolve an unsettled choice in this order:

1. Explicit task requirements and `plans/baseline.md`.
2. This visual system.
3. GrapheneOS interaction conventions, adapted to this system for status,
   quick settings, gestures, permissions, and recovery.

Ferrix remains the only product identity. Space Monkey is a documented palette
reference, and BMW is a documented usability influence.
