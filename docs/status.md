# Status

## 2026-09-16

- **mock:** The strict, schema-versioned QEMU profile parser and deterministic unplugged-to-charging transition are covered by a Rust unit test.
- **QEMU:** `just qemu-check` booted the generic AArch64 `virt` guest under TCG and observed serial boot-health plus the deterministic unplugged-to-charging mock event. The guest powers down cleanly after the smoke check.

No physical-device testing or claims have been made.
