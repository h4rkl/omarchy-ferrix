# Implementation Path and Evidence Gates

Read this reference when turning the flexible architecture into repository changes or a roadmap.

## Baseline decision gate

The current repository baseline defines the implemented authority. Before introducing an Android runtime, a web application runtime, new public app APIs, or a device target:

1. Compare the proposed change with `docs/baseline.md` and `docs/plan.md`.
2. Record the conflict or expansion explicitly.
3. Change the authoritative documents only when the user has requested the architectural pivot.
4. Preserve the existing QEMU smoke contract while adding new, separately bounded checks.

An architecture skill is not approval to install Android tooling, add a board target, operate a device, or redistribute proprietary images.

## Current-to-future mapping

| Current component | Intended responsibility |
| --- | --- |
| `hardware-api` | Stable internal hardware capability traits and shared state |
| `hardware-emulator` | Deterministic QEMU/mock adapter |
| `mobile-session` | Target selection and adapter composition only |
| `mobile-core` | Rust domain state, orchestration, and privileged service façade |
| `mobile-protocol` | Current internal transport; evolve or supplement with versioned language-neutral app APIs |
| `mobile-shell` | Product compositor, surface policy, trusted global gestures |
| `mobile-basic-screen` | Reference UX and migration source, not the permanent container for launcher and apps |
| `target-profile` | Strict emulator and target configuration parsing |
| proposed `appd` | Unified application catalog, activation, lifecycle, and crash state |
| proposed native SDK | Rust app lifecycle and portal clients |
| proposed web runtime | TypeScript bundle host and generated portal API |
| proposed Android runtime adapter | Contained APK runtime lifecycle and bridge |
| proposed portal service | Permissioned capabilities shared across host app runtimes |

Do not force every new responsibility into `mobile-core`. Split independently supervised services when the security boundary, crash behavior, or update lifecycle differs.

## Suggested repository shape

Create directories only as implementation requires them:

```text
crates/
  app-api/                 # descriptors, IDs, lifecycle, capabilities
  appd/                    # catalog and lifecycle service
  portal-api/              # language-neutral portal contracts
  portal-service/          # permission enforcement and dispatch
  runtime-native/          # native bundle discovery/launch adapter
  runtime-web/             # TypeScript/web bundle adapter
  runtime-android/         # Android compatibility adapter
  mobile-home/             # launcher/home client
  mobile-shell-ui/         # trusted status/navigation/quick settings
sdk/
  rust/
  typescript/
targets/
  qemu-aarch64/
```

Names are illustrative. Prefer small changes that establish contracts and evidence over creating empty scaffolding.

## Phase 1: extract the application contract

Outcome:

- Define stable app IDs, runtime kinds, descriptors, lifecycle states, and typed errors.
- Add an in-memory registry with native/reference entries.
- Make the launcher consume the registry rather than a hard-coded app enum.
- Keep existing views working during migration.

Evidence:

- Unit tests for identity, duplicate handling, lifecycle transitions, and unsupported operations.
- Architecture test preventing runtime implementations from leaking into launcher/product crates.
- Existing QEMU graphical flow remains functional.

## Phase 2: independent native applications

Outcome:

- Move at least one reference app, preferably Terminal, into an independent Wayland client.
- `appd` launches and tracks it.
- Home returns focus to a persistent launcher rather than swapping an in-process page.
- Closing/crashing the app returns to a safe surface.

Evidence:

- Process and surface identity records.
- Launch, background, resume, exit, crash, and relaunch tests.
- Captured framebuffer for launcher, app, and crash recovery.

## Phase 3: public portals and Rust SDK

Outcome:

- Separate privileged core APIs from public application APIs.
- Provide a versioned portal contract and Rust client.
- Implement the smallest useful domains, such as notifications and open-URI.
- Enforce application identity and permissions at the service, not only in the SDK.

Evidence:

- Allowed and denied request tests.
- Caller identity cannot be forged through request payloads.
- Protocol compatibility tests across at least two schema/interface versions when versioning begins.

## Phase 4: TypeScript/web vertical slice

Outcome:

- Evaluate the web engine/runtime against ARM64, Wayland, software rendering, OSK, accessibility, isolation, and security-update requirements.
- Install a signed local bundle.
- Launch it as an independently managed application.
- Call one permissioned portal through a generated TypeScript API.

Evidence:

- QEMU framebuffer and touch input.
- Storage/origin separation between two test apps.
- Denied direct host access.
- Suspend/resume and runtime crash recovery.

Do not freeze the package format or engine until the vertical slice demonstrates these properties.

## Phase 5: Android compatibility spike

Start as a bounded feasibility experiment rather than a product claim.

Outcome:

- Evaluate Waydroid or alternatives for the guest architecture and license constraints.
- Boot an ARM64 Android userspace inside the QEMU Linux guest.
- Install one redistributable test APK.
- Import its label/icon into `appd`.
- Launch it into a Wayland surface and deliver touch input.
- Stop or freeze the runtime cleanly.

Kernel/runtime prerequisites to evaluate include Binder/binderfs, namespaces, cgroups, networking, shared memory, graphics transport, audio, storage, and memory pressure.

Evidence:

- Machine-readable Android-runtime readiness or failure.
- Package install and activity launch records.
- Connected/mapped Wayland client evidence.
- Nonblank framebuffer capture.
- Input acknowledgement.
- Bounded clean shutdown.

Pin Android image inputs and hashes. Do not download mutable images implicitly during ordinary checks.

## Phase 6: controlled integration bridges

Add one bridge at a time, starting with notifications or URI opening. For each bridge specify:

- Direction of trust and data flow.
- Caller identity.
- User consent or permission.
- Validation and size/rate limits.
- Lifecycle and failure behavior.
- Whether content crosses host/container storage boundaries.
- Tests for malformed and unauthorized requests.

Avoid a generic “call arbitrary Android intent/Binder” host API.

## Phase 7: Solana compatibility evidence

In QEMU or another emulator, use redistributable test components:

- Install a mock MWA wallet and a small MWA dApp in the Android runtime.
- Demonstrate association, user approval, signing of test data, cancellation, and wallet/runtime failure.
- Keep keys test-only and ephemeral.

This establishes generic MWA behavior only. It does not establish dApp Store, Seed Vault, TEE, attestation, or Seeker support.

For host-native Rust/TypeScript apps, design wallet access as a permissioned service with explicit confirmation. Do not bridge Seed Vault or production keys without a separate threat model and supported vendor interface.

## Phase 8: additional targets

Only after QEMU contracts are stable, define the target-admission process. A target package must not change product logic.

Evaluate targets in this order:

1. Mainline Linux capability and missing drivers.
2. Maintained standard-Linux service integration.
3. Need for vendor Android HAL reuse.
4. Feasibility and maintenance cost of Halium/libhybris-style adaptation.
5. Recovery, update, signing, licensing, and security evidence.

Seeker requires its own approved proposal. Absence of public boot, recovery, vendor, or Seed Vault integration evidence is a blocker to claiming support, not a reason to guess.

## Cross-cutting review checklist

For each architecture change, answer:

- What is the stable contract?
- Which process owns the state?
- Which runtime and trust ring is the caller in?
- What happens when the component is absent, unsupported, slow, or crashed?
- How is capability discovered?
- How are permissions enforced independently of client libraries?
- Does target-specific code leak into product crates?
- Can the implementation be replaced without changing applications?
- What QEMU evidence demonstrates the claim?
- What claim remains unverified on physical devices?

## Validation commands

Run the applicable existing repository gates before handoff:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
just qemu-check
```

Graphical changes additionally require the documented graphical QEMU workflow, inspection of the actual framebuffer, and an honest `docs/status.md` update. New runtime checks must remain bounded and report machine-readable failures.
