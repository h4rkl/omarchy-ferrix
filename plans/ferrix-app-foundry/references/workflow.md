# Classification, Generation, and Publication Workflow

Read this reference for catalogue ingestion, port requests, agent jobs, implementation work, publication, or installation.

## State model

Keep assessment, source generation, verification, and distribution distinct:

```text
discovered
   -> assessed: portable | adaptable | backend-reuse
   -> flavour proposed
   -> proposal approved
   -> source generated
   -> buildable
   -> conformant in mock/QEMU
   -> reviewed
   -> published
   -> installed/enabled

discovered -> assessed: desktop-only | unsafe-or-unsupported -> retained, no build
```

Failure or rejection at one stage does not erase earlier records. A new pinned upstream revision creates a new assessment input and cannot inherit verification automatically.

## 1. Ingest without execution

- Fetch catalogue metadata and public source through a read-only path.
- Resolve the repository to an immutable commit and record hashes.
- Reject ambiguous repositories, moving tags without resolved commits, nested repositories, unsafe paths, and symlinks where the package policy forbids them.
- Parse manifests and inventory source types, imports, external commands, services, assets, network destinations, native dependencies, and install hooks without running them.
- Record upstream install instructions as data only.

## 2. Classify the capability

Assess the essential user outcome separately from its desktop mechanism.

Questions that change the decision:

- Does the outcome make sense on a portrait, touch-first, focused-fullscreen device?
- Is the plugin essentially a workspace, tiling, multi-monitor, desktop-bar, or window-management control?
- Can its Linux service dependencies be accessed through existing or planned typed Ferrix adapters?
- Which logic/assets are separable from QML, Quickshell, Hyprland, or shell-process state?
- Does it require arbitrary commands, broad filesystem access, secrets, authentication UI, or privileged hardware?
- Is source and asset reuse legally allowed?
- Does an existing Ferrix flavour or implementation already cover the outcome?

Produce a structured assessment with classification, confidence, evidence, reusable elements, rewrite requirements, rejected elements, capability needs, licensing status, and human decisions. Confidence never substitutes for evidence.

## 3. Reuse or propose a flavour

Prefer an existing flavour when its user outcome and conformance behavior match. Do not create a near-duplicate merely because a different upstream plugin initiated the request.

For a new flavour proposal:

- Define the mobile outcome and non-goals.
- Select only already approved extension surfaces, or separately propose the missing public surface contract.
- Define permissions in product terms, not upstream API names.
- Define offline, missing-service, denied-permission, crash, and upgrade behavior.
- Write conformance criteria before generation.
- Identify whether multiple implementations may coexist and whether any intent is exclusive.

Changing product architecture or privileged APIs requires the repository's normal baseline/plan decision process; an agent job cannot approve it implicitly.

## 4. Choose an implementation path

Prefer in order:

1. Link an existing conformant implementation.
2. Contribute missing behavior to a maintained implementation when scope and governance fit.
3. Fork with explicit provenance when the original is abandoned or its product direction differs.
4. Generate a new implementation when there is no suitable maintained codebase.

An agent-generated implementation receives a new Ferrix app ID, repository, and maintainer/review status. It must not impersonate the upstream author or claim endorsement.

## 5. Generate from an approved plan

The generation job input should contain only:

- Pinned, read-only upstream inputs.
- Approved flavour contract and implementation proposal.
- Relevant public Ferrix APIs, templates, and repository instructions.
- Explicit dependency, licensing, permission, and resource constraints.
- Required tests and evidence outputs.

The generator should preserve useful algorithms and assets only when licensing permits. It redesigns desktop UI for Ferrix rather than performing mechanical QML-to-Rust translation. Generated apps depend on public app/portal protocols and must not import daemon or runtime implementations.

Treat generated source as hostile until reviewed. Do not execute model-suggested setup commands outside the build policy.

## 6. Validate progressively

Use the cheapest meaningful gate first:

1. Manifest/schema, license, forbidden-file, path, and dependency-policy validation.
2. Formatting, static analysis, unit tests, and flavour conformance tests.
3. Nested mock launch with deterministic profile state.
4. QEMU package installation, lifecycle, failure, and permission-denial checks.
5. Graphical QEMU run with connected Wayland client plus inspected framebuffer captures.
6. Resource and background-behavior checks appropriate to the flavour.
7. Exact physical-target evidence only through an approved board workflow.

Use the repository's applicable validation commands. A graphical claim requires framebuffer/window inspection, not a process-start record. Preserve machine-readable failures, logs, input revisions, and hashes.

## 7. Review and publish

Review should cover:

- Does the implementation meet the flavour rather than merely compile?
- Is the mobile interaction intentional and accessible?
- Are permissions minimal and enforced by services?
- Are dependencies maintained and license-compatible?
- Are provenance, generated portions, and manual changes disclosed?
- Are external services, data collection, credentials, and account requirements visible?
- Do evidence labels match what was actually tested?
- Can installation, update, disable, removal, and rollback fail safely?

Publishing records an immutable artifact; it does not publish a mutable branch head. Promotion between trust tiers is a separate reviewed action.

## 8. Install and select

- Display implementation identity, maintainer, trust tier, permissions, external services, size, evidence, and known limitations before installation.
- Verify metadata and artifact integrity before unpacking.
- Stage and validate atomically; leave third-party implementations disabled until the user enables them unless policy explicitly allows otherwise.
- Never run install hooks or request elevated privileges from package content.
- Preserve the previous version until the new version passes activation/health checks.
- Removing one implementation must not delete the flavour record, upstream mapping, or unrelated alternatives.

## Incremental delivery

### Phase 1: read-only catalogue and mappings

Normalize upstream records, record classifications, group existing manually registered Ferrix implementations, and explain unavailable desktop entries. No agent execution or package installation.

### Phase 2: flavour contracts and conformance

Adopt a versioned flavour schema, implement one low-risk flavour, and demonstrate two interchangeable implementations or one implementation plus a conformance fixture.

### Phase 3: developer-side agent workflow

Generate proposals and source branches or pull requests on a development host. Require human review; do not publish artifacts automatically.

### Phase 4: package and installer vertical slice

Define immutable package metadata, validation, user-owned storage, enable/disable, update, rollback, removal, and dynamic catalogue refresh. First prove a locally built, reviewed package.

### Phase 5: isolated foundry service

Separate source analysis, generation, build, evidence, and publishing workers. Add provenance and trust-tier promotion. The phone app requests and observes jobs but never gains builder or publisher credentials.

### Phase 6: community ecosystem

Add maintainer claims, implementation alternatives, compatibility updates, deprecation/revocation, reproducible builds, and governance for reference and verified-community tiers.
