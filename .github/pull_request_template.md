<!--
See AGENTS.md. Keep the PR scoped, reproducible through checked-in scripts or
`just` recipes, and redact unique identifiers: never paste device serials,
IMEI/MEID, MAC/Bluetooth addresses, keys, firmware or partition dumps.
-->

## Summary

<!-- What changes and why. Link the issue: "Refs #N" or "Closes #N". -->

## Evidence

Tick every label that applies to evidence in this PR. Evidence never carries
across labels: QEMU or mock results say nothing about hardware, and one
hardware revision says nothing about another.

- [ ] `QEMU` (AArch64 `virt`, TCG, `just qemu-check` / scenarios)
- [ ] `mock` (host tests, mock profile, offline contract tests)
- [ ] `physical:<target>`, exact target id: <!-- e.g. google-pixel-9a-tegu-gxq96-mp1-0 -->
- [ ] No runtime evidence needed (docs, CI or process only)

<!-- Summarise the evidence or link where it is recorded. Graphical claims need
a capture of the actual framebuffer or window, not a process-start log. -->

## Risk

- [ ] **Capability change?** This PR changes a board `capabilities.toml`
      status or evidence, or a capability claim in docs. If checked, link the
      exact `physical:<target>` evidence for that target and hardware revision;
      QEMU or mock evidence does not qualify.
- [ ] **Recovery or flash tooling touched?** (`tools/pixel-9a/dev-flash*`,
      `factory-restore*`, `enter-fastboot*`, `flash*`, `unlock-bootloader*`,
      `recovery.pins`, `skills/mobile-device-provisioning/`, ...). If checked,
      confirm the recovery invariant is unchanged (exact single device, tested
      hash-verified restore to the pinned factory image, no relock, erase,
      format or `oem`) and that the matching `tools/pixel-9a/tests/*-workflow.sh`
      tests pass.

## Handoff checks

Tick what was run; strike out or explain anything not applicable.

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [ ] `cargo test --workspace`
- [ ] `just qemu-check`
- [ ] Relevant `tools/pixel-9a/tests/` scripts (list them):
- [ ] Graphical change: documented graphical workflow run and output inspected

## Status

- [ ] Any new evidence or limitation is a dated entry at the top of the current
      month's `docs/status-log/YYYY-MM.md`, with its evidence label (or no new
      evidence in this PR). The `docs/status.md` "Current state" table is
      updated only when a capability matrix changes.
