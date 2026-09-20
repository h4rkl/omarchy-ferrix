# Foundry Architecture

Read this reference when designing the registry, catalogue experience, application boundaries, extension contracts, or implementation selection.

## Product shape

```text
Omarchy catalogue and repositories
             |
      ingestion snapshot
             |
     compatibility mappings
       /        |         \
desktop-only  flavours   existing implementations
                 |             |
          conformance      immutable artifacts
                 \             /
                  catalogue UI
                 /      |       \
             compare  request   install approved
                        port
                         |
                 isolated agent job
                         |
              source + evidence proposal
```

The catalogue UI consumes normalized registry data. It must not render arbitrary upstream HTML, execute upstream install commands, or treat repository metadata as trusted configuration.

## Entity model

### Upstream source

An upstream record should include:

- Registry namespace and upstream ID.
- Canonical repository URL and pinned commit.
- Manifest and metadata snapshot hashes.
- Upstream kinds such as `bar-widget`, `panel`, `overlay`, `menu`, `service`, or `bar`.
- License declaration and verification status.
- Last assessment, classification, confidence, and assessment-tool version.
- Links to zero or more flavour mappings and implementations.

Catalogue ingestion is metadata synchronization, not installation. Preserve upstream deletion or mutation as historical state rather than silently rewriting provenance.

### Flavour

A flavour is comparable to an intent plus conformance profile, not a visual theme or source fork. It should define:

- Globally namespaced ID and semantic contract version.
- Concise user outcome and explicit non-goals.
- Permitted surface types and interaction expectations.
- Intents or handlers it provides and whether each is exclusive.
- Portable capability requirements expressed through project-owned APIs.
- Settings and persistent-state schema.
- Foreground/background and offline behavior.
- Typed failure and unsupported behavior.
- Touch, rotation, software-rendering, localization, and accessibility expectations.
- Machine-executable conformance tests and required evidence.

Avoid embedding a particular implementation, toolkit, provider, target, or upstream plugin in the flavour contract. Compatibility ranges must be explicit; a new flavour version cannot silently change permissions or user-visible semantics.

Example conceptual record:

```json
{
  "schema_version": 1,
  "id": "org.ferrix.flavour.weather-glance",
  "version": "1.0.0",
  "outcome": "Show current conditions and a short forecast",
  "surfaces": ["application", "launcher-summary"],
  "intents": [{"id": "org.ferrix.intent.weather", "exclusive": false}],
  "capabilities": ["network"],
  "conformance_suite": "weather-glance-v1"
}
```

Names and fields are conceptual until adopted into an authoritative schema.

### Implementation

An implementation owns its Ferrix app identity and release lifecycle. Record:

- App ID, source repository, maintainer identity, and license.
- Claimed flavour IDs and compatible version ranges.
- Runtime kind and supported architectures/targets.
- Declared permissions and externally contacted services.
- Settings migration and data portability behavior.
- Resource expectations and background policy.
- Release channel, update policy, and support status.
- Conformance results and review tier.

Do not name implementations by appending an arbitrary runtime suffix to the flavour unless that is the maintainer's stable product identity.

### Artifact and evidence

An artifact record binds an implementation release to:

- Source commit and clean/dirty source state.
- Dependency lockfiles and relevant vendored-input hashes.
- Builder image/toolchain identity.
- Target/runtime/architecture.
- Package and contained-file hashes.
- Conformance, static-analysis, unit, integration, and graphical evidence.
- Signature/attestation identity and timestamp.
- Known limitations and revoked/superseded state.

QEMU evidence demonstrates QEMU behavior only. Physical evidence names the exact admitted target and hardware revision.

## Ownership boundaries

- **Registry service:** normalized metadata, mappings, release records, trust/review state. It does not build or execute apps.
- **Catalogue client:** search, grouping, comparison, job requests, review display, install requests. It receives no registry signing keys or build credentials.
- **Foundry orchestrator:** turns an approved proposal into bounded jobs and records their state. It does not decide product permissions.
- **Source analyzer:** parses manifests/source and produces a compatibility assessment without executing the source.
- **Generation worker:** receives pinned read-only inputs and an approved flavour/implementation plan. Its output is an untrusted source diff.
- **Build worker:** compiles and tests in a disposable environment; it does not receive model credentials or publishing authority.
- **Publisher:** admits artifacts only after policy gates. It cannot rewrite evidence supplied by build workers.
- **Installer/runtime services:** verify and activate approved packages through Ferrix-owned contracts. The launcher never clones repositories or forks arbitrary commands.

Keep these as logical trust boundaries even if an early prototype combines processes.

## Discovery and alternatives

Default catalogue presentation is flavour-first:

```text
Weather glance
  preferred: Ferrix Weather
  alternatives: Minimal Weather, Local Station
  inspired by: upstream plugin A, upstream plugin B
```

Show desktop-only records when useful for ecosystem discovery, but label them unavailable with a specific reason. Never create placeholder launcher apps for them.

Implementation tiers should convey evidence, not popularity:

1. `reference`: maintained under project governance and release policy.
2. `verified-community`: reproducible artifact plus current flavour conformance and required review.
3. `community`: valid package and baseline automated checks, without full verification.
4. `experimental-generated`: agent output or early port awaiting independent maintainership/review.

Downloads, stars, model confidence, and upstream marketplace presence do not raise a trust tier.

## Selection and defaults

Use explicit intent/default records rather than encoding defaults in flavour or app IDs. Selection must be reversible and survive a missing, crashed, revoked, or incompatible implementation.

For exclusive system roles:

- Require a privileged, user-visible selection operation.
- Validate that the candidate supports the role before switching.
- Keep a known-good built-in fallback.
- Apply the switch atomically.
- Roll back when health/readiness fails.
- Do not let ordinary app installation replace a role implicitly.

Ordinary implementations may coexist. The launcher can group them under a flavour while still exposing each installed app when the user requests alternatives.

## Repository integration

Build on the current stable waist:

- Extend `ferrix-app-api` only for portable public identity, package, intent, or capability contracts.
- Keep registry and installer protocols language-neutral where third-party tooling must consume them.
- Let `ferrix-appd` aggregate installed descriptors and manage lifecycle; do not make it a Git client, LLM orchestrator, or marketplace scraper.
- Put native launch details in runtime-private configuration.
- Keep shell-owned surfaces and privileged system roles distinct from ordinary application content.
- Keep target selection in session/bootstrap code and target-specific packaging under target workflows.

New crates or services should be introduced only when implementing a vertical slice; do not create an empty architecture tree.
