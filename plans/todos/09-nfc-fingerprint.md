# 09 — NFC and fingerprint (feasibility first)

Status: open, not started · Issue: #29 · Depends on: 00, 12 (unlock policy
for fingerprint) · Release: owner decides whether these are required

## Goal

Two independent GO/NO-GO decisions with exact-device evidence. Enable each
feature only when an end-to-end user flow can be proven.

## Deliverables

### NFC

1. Read-only inventory: the controller (the native plan does not claim the
   ST54 secure-element NFC), the kernel binding, firmware, and the userspace
   interface.
2. If GO: a maintained Linux NFC stack (neard or libnfc, licence-checked)
   behind a typed adapter. Prove a tag read through an app-facing flow.
   Payments are out of scope: no secure-element or HCE claim follows from a
   tag read.

### Fingerprint

3. Read-only inventory: the sensor (under-display), its transport, and
   whether matching runs in a TEE / Trusty that Linux cannot reach.
4. If GO: libfprint / fprintd behind a typed adapter. Enrolment and
   revocation designed together with spec 12's lock screen. No raw
   biometric data in project code, logs or evidence. Fingerprint may only
   unlock what the PIN or passphrase already protects, never something
   stronger.

### Both

5. Unsupported features stay visibly absent from Settings, or are labelled.
   Any vendor-only dependency is documented.

## Acceptance

Separate, documented decisions for NFC and fingerprint, each with
exact-device evidence. A supported claim includes an end-to-end flow.

## Owner-gated

Release scope if either is NO-GO. Any biometric enrolment on the device.

## Risks

Matching on Pixel fingerprint sensors normally runs in the TEE. A NO-GO is
likely without vendor TEE access.
