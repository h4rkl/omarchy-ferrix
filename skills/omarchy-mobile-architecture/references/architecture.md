# Flexible Rust-First Mobile Architecture

## Decision statement

The intended product is a Linux-hosted mobile operating environment with:

- Rust-first system services, policy, shell, and tooling.
- First-class Rust-native and TypeScript/web application development.
- A unified launcher and lifecycle model across application runtimes.
- Android APK availability through an isolated compatibility runtime.
- QEMU-first development, testing, and evidence.
- Target-specific hardware enablement behind stable project-owned contracts.

Android compatibility is an application facility. It must not determine the host service architecture, compositor, developer workflow, or hardware API.

## System shape

```text
                         product shell
               launcher / status / navigation / OSK
                                |
                  application registry + appd
                   /             |              \
          Rust-native       TypeScript/web     Android APK
          Wayland apps       web runtime       compatibility runtime
                   \             |              /
                    permissioned system portals
                                |
                 Rust domain services / mobile-core
                                |
                          hardware-api
                       /                \
             standard Linux          Android-HAL/vendor
                adapters                  adapter
                       \                /
                   target/session bootstrap
                                |
                     QEMU or approved device
```

## Stable waist

Future-proof through narrow contracts, not a universal framework.

The stable waist comprises:

- Wayland protocols for application surfaces and input.
- A versioned application descriptor and lifecycle contract.
- Versioned, language-neutral portal/service protocols.
- `hardware-api` traits and shared capability types inside the product layer.
- Runtime capability discovery and typed unsupported results.
- Stable application IDs used consistently for launch, permissions, storage, notifications, and crash records.

Keep rendering toolkit, web engine, Android container, IPC library, init system integration, package storage format, and target adapter implementation replaceable.

## Application model

Applications belong to one runtime but share an identity and lifecycle model.

Conceptual types, not frozen source definitions:

```rust
enum RuntimeKind {
    Native,
    Web,
    Android,
}

struct AppDescriptor {
    id: AppId,
    runtime: RuntimeKind,
    name: String,
    icon: IconRef,
    launchable: bool,
    capabilities: Vec<AppCapability>,
}
```

Application IDs should be stable, globally namespaced identifiers. Runtime-specific identifiers such as Android package names are attributes of that identity, not identities invented independently by each shell component.

`appd` owns catalog aggregation, activation, foreground/background state, crash records, and surface association. The launcher queries `appd`; it does not scan packages, fork commands, or speak directly to a compatibility runtime.

### Lifecycle

Support at least these conceptual states:

```text
not-running -> starting -> foreground -> background -> frozen
                    \                         /
                     -> crashed/terminated <-
```

Runtimes may implement the states differently, but applications and shell policy receive consistent observable results. Background execution is a granted capability, not an assumption.

## Runtime contracts

### Native Rust runtime

Native apps are independent Wayland clients. Provide an SDK for lifecycle, theme, notifications, files, clipboard, share/open-URI, background work, wallet requests, and capability discovery.

Do not make one Rust UI toolkit part of the OS ABI. Toolkits consume protocols and SDKs.

### TypeScript/web runtime

TypeScript apps run as signed/verified local bundles in a system-owned web runtime. Prefer a Tauri-like split: TypeScript UI with an optional Rust domain backend. The OS, not each app, owns the web engine update and security policy.

The runtime must provide:

- Per-app origin and storage isolation.
- A narrow generated TypeScript API for portals.
- Explicit content-security policy and network permissions.
- Touch, virtual-keyboard, accessibility, suspend/resume, and low-memory behavior.
- No arbitrary host filesystem, process, or D-Bus access.

WebKit/Wry or another engine is an implementation choice. Verify Wayland, software rendering, ARM64, accessibility, and security-update behavior before selection.

### Android compatibility runtime

Run a complete Android userspace in an isolated container or equivalent compatibility domain. The runtime owns ART, PackageManager, Android lifecycle, intents, and application sandbox semantics.

Expose a project-owned adapter with operations such as status, catalog, install, remove, launch, freeze, and health. Do not expose the implementation CLI or unrestricted Binder to shell and product apps.

The bridge may integrate only reviewed surfaces:

- Application catalog and icons.
- Wayland windows/surfaces.
- Touch, keyboard, and rotation.
- Notifications and user actions.
- Clipboard under policy.
- Document/file exchange through portals.
- URI/share intents through validation.
- Audio focus and lifecycle.
- Wallet association only after a separate security design.

Avoid raw hardware pass-through where mediation is practical. Record every passed device, socket, mount, capability, and namespace permission.

Waydroid is relevant prior art because it runs Android in Linux namespaces, exposes apps through Wayland, and supports package installation and lifecycle control. It must still pass a project-specific fit, security, licensing, and QEMU evaluation.

## Shell boundaries

`mobile-shell` owns compositor mechanisms and global policy:

- Surface roles, rendering, focus, and fullscreen stacking.
- Secure input routing and global gestures.
- Output geometry, scale, and rotation.
- Layer surfaces for trusted chrome.
- Crash isolation and a recoverable safe surface.

Prefer separate recoverable processes for:

- Home/launcher.
- Status/navigation/quick settings.
- On-screen keyboard or input method.
- Application registry/lifecycle daemon.

The shell must treat native, web, and Android surfaces consistently once mapped while retaining enough runtime metadata for lifecycle and security decisions.

## Services and portals

Keep internal Rust traits distinct from public application APIs.

Trust rings:

1. Internal adapters: Rust traits used by session wiring and core services.
2. Privileged system APIs: shell, Settings, updater, runtime manager.
3. Public portals: narrow permission-checked operations available to native and web apps.

Initial portal domains should include documents, URI/share, notifications, clipboard, camera/microphone, location, background work, screen capture, and wallet/signing requests. Android apps normally use Android APIs inside their runtime; bridges translate only deliberate cross-runtime interactions.

## Hardware and target portability

Support two adapter strategies without changing product code.

### Mainline/standard Linux

The host owns hardware through maintained Linux services such as NetworkManager, UPower, PipeWire, ModemManager, and sensor services. Project-owned typed adapters translate those services into `hardware-api`.

### Android-vendor compatibility

For Android-derived devices lacking sufficient mainline support, a contained vendor/HAL environment may own some hardware. A project adapter translates its supported interfaces into `hardware-api`. Halium/libhybris are prior art, not assumed dependencies.

Do not run two uncoordinated Android environments that compete for Binder, GPU, camera, audio, modem, sensors, or power control. Define one owner for each resource and broker access to other runtimes.

Every target supplies:

- Target identifier and versioned manifest.
- Capability matrix: supported, unsupported, or mocked.
- Adapter wiring only in session/bootstrap.
- Kernel/rootfs/runtime requirements.
- Evidence for every claimed capability.
- Recovery and rollback procedure before destructive hardware work.

## QEMU contract

QEMU remains the portability and CI baseline. Extend validation in layers:

```text
core check       Linux boot, mobile-core, deterministic mock events
shell check      compositor socket, connected client, framebuffer evidence
native check     install and launch a Rust application
web check        install and launch a TypeScript bundle
Android check    boot runtime, install APK, map surface, deliver input
integration      notification/file/URI bridge and clean shutdown
```

Keep the smallest check headless, TCG-compatible, bounded, and display-independent. More expensive runtime checks may be separate jobs, but they require machine-readable readiness and failure records.

## APK compatibility claims

Report compatibility by demonstrated level:

1. Installable: package accepted.
2. Launchable: activity starts.
3. Graphical: nonblank mapped surface observed.
4. Interactive: input and lifecycle verified.
5. Integrated: selected notifications, files, clipboard, audio, and links work.
6. Solana-compatible: MWA flow demonstrated with a test wallet.
7. Device-integrated: specified hardware and secure services verified on an approved target.
8. Certified: applicable external compatibility/store requirements passed.

Never collapse these levels into “Android compatible.” Explicitly identify Google services, attestation, DRM, banking, codecs, camera, telephony, biometrics, and background execution as unverified until tested.

## Solana and Seeker boundary

The desired ecosystem includes APK-based Solana applications, Mobile Wallet Adapter, the Solana dApp Store, and potentially Seed Vault-backed flows. Generic APK or MWA success does not prove any Seeker-specific capability.

Before a Seeker target exists, an approved proposal must establish at least:

- Supported boot/unlock and recovery procedures.
- Kernel, device tree, vendor image, and redistributable component availability.
- Verified-boot, signing, rollback, and OTA design.
- Ownership of camera, GPU, modem, audio, sensors, biometrics, and power paths.
- Whether the dApp Store may run and be redistributed in the proposed environment.
- Whether Seed Vault's Android service, TEE application, trust chain, and user confirmation UI can remain functional and secure.
- A no-device-data policy and evidence plan.

Until then, use a mock MWA wallet in emulation and label all evidence `mock` or `QEMU`.

## Non-goals

- Reimplementing Android framework APIs in the host OS.
- Making Android the host product architecture.
- Requiring native apps to understand Android.
- Requiring all apps to use one Rust UI toolkit.
- Granting APKs or web apps unrestricted host access.
- Claiming Google service, Play Integrity, store, or Seeker compatibility from package installation alone.
- Embedding board conditionals in product crates.

## External reference points

Verify versions and status when used:

- Waydroid architecture and sources: <https://github.com/waydroid/waydroid>
- Halium scope: <https://docs.halium.org/en/latest/project/Scope.html>
- Solana Mobile documentation: <https://docs.solanamobile.com/>
- Wry webview documentation: <https://docs.rs/wry/>
