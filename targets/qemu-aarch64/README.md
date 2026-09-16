# Generic AArch64 QEMU virtual phone

This is a **QEMU/mock** target, never a physical-device claim. It uses QEMU `virt`, a serial console, and the versioned mock profile in `profile.toml`.

Run `just host-check`, then `just qemu-build`, `just qemu-run`, or the bounded headless `just qemu-check`. On Apple Silicon, `qemu-run` selects HVF automatically; `qemu-check` always uses portable TCG. The initial serial-only image uses the pinned Alpine arm64 builder image and its generic `linux-virt` kernel; the planned postmarketOS/pmbootstrap guest remains the full mobile-image route.

The first boot contract is a JSON `boot-health` record on serial. The smoke check also requires a deterministic mock transition from unplugged to charging.
