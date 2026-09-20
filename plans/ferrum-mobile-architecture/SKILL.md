---
name: ferrum-mobile-architecture
description: Guide architecture, planning, and implementation for Ferrum's proposed Linux-hosted, Rust-first mobile OS with Rust/TypeScript-native apps, QEMU-first development, portable device adapters, and optional Android APK compatibility. Use for shell, app-platform, runtime, service, hardware-boundary, or target-design work; do not use for unrelated fixes or as evidence of current Seeker support.
---

# Ferrum Mobile Architecture

Use this skill to keep architectural work aligned with the proposed long-term product direction: Linux is the host OS, Rust owns project services and policy, Rust and TypeScript are first-class application paths, and Android is an optional contained compatibility runtime rather than the host architecture.

## Authority and status

This skill records a **proposed architecture**, not implemented state, device evidence, or authorization for hardware work.

Before acting:

1. Read the repository `AGENTS.md`.
2. Read `plans/baseline.md` and `plans/plan.md` for architecture work; baseline wins conflicts.
3. Read `docs/status.md` for implemented state and limitations.
4. Read `targets/qemu-aarch64/README.md` when changing QEMU workflows or evidence.

If this skill conflicts with the current baseline, identify the conflict. Do not silently treat the proposal as approved or rewrite the baseline unless the user explicitly requests that architectural decision.

## Required architectural invariants

- Keep Linux, not Android, as the host product environment.
- Keep project-owned services and policy Rust-first. Use a non-Rust component only at a documented ecosystem or platform boundary.
- Treat Rust-native, TypeScript/web, and Android APK applications as separate runtime types behind one application registry and lifecycle service.
- Treat Android as a contained compatibility subsystem with a narrow bridge. Host product code must not depend directly on Binder, Android framework internals, or a particular Android container implementation.
- Preserve QEMU as the authoritative portable development and CI target. Label evidence `mock` or `QEMU`.
- Put target selection and hardware adapter wiring in session/bootstrap code. Keep device, board, vendor, and QEMU conditionals out of apps, `mobile-core`, and `hardware-api`.
- Put standard Linux services and any Android-HAL compatibility behind typed project-owned adapters.
- Prefer protocols and capability discovery over shared implementation crates at public app boundaries. Do not require all application developers to use Rust.
- Mediate sensitive app access through portal-like, permission-checked services. Never expose raw devices or unrestricted compatibility-runtime IPC to ordinary applications.
- Do not claim APK installation implies universal Android compatibility, certification, Google services, store acceptance, Seed Vault availability, or physical-device support.
- Do not add or operate a Seeker target without a separately approved board-support proposal, recovery design, and capability evidence.

## Apply the architecture

Read [references/architecture.md](references/architecture.md) for the component model, runtime contracts, device strategies, security boundaries, and explicit non-goals.

When proposing repository changes, sequencing milestones, or implementing a vertical slice, also read [references/implementation-path.md](references/implementation-path.md). It maps the proposal onto the current crates and defines evidence gates.

For each design or change:

1. Identify the owning layer: shell/compositor, app platform, native runtime, web runtime, Android runtime, portal, core service, hardware adapter, target package, or build/evidence tooling.
2. Define the stable contract before selecting an implementation. Keep candidate technologies replaceable.
3. State whether the behavior is host-native, compatibility-runtime-only, or shared.
4. State the trust level: system, privileged first-party, or untrusted application.
5. Preserve lifecycle, permission, crash-isolation, and capability-discovery behavior across runtimes.
6. Validate on QEMU first unless the task is explicitly limited to pure host tests. Do not infer device support from emulator success.

## Candidate technology policy

Waydroid, LXC, WebKit/Wry, Tauri-like packaging, Halium, and libhybris are evaluation candidates, not architectural dependencies. Select or reject them through a documented fit evaluation covering maintenance, licensing, security, ARM64 support, software rendering, lifecycle control, and QEMU behavior.

When current facts about these projects, Android compatibility, Solana Mobile, or Seeker matter, verify them from primary sources at implementation time.
