# 02 — Carrier voice, VoLTE/IMS and emergency calling

Status: open, not started · Issue: #27 · Depends on: 01 (call state), 03
(call audio), 04 (proximity sensor and blanking during calls) · Release
blocker: emergency calling

## Goal

Decide, then prove, whether the exact Pixel 9a can give dependable carrier
voice on Ferrix, and keep an honest emergency-calling status.

## Why this is the hardest item

Most carriers have switched off 2G/3G, so voice now needs VoLTE, which means
IMS registration. On Android, IMS for Pixel modems runs in proprietary
vendor userspace. A Linux route on the S5300 has to find out whether the
modem firmware does IMS itself (exposed through SIT/AT), or whether there is
an open IMS client that can drive it. If neither works, the phone has no
carrier voice on LTE-only networks, whatever else works.

## Deliverables

1. **Feasibility report (offline + read-only).** Write it under
   `targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/`:
   - the voice and IMS interfaces the S5300 firmware exposes on route A;
   - what Android's IMS stack does for this modem, from pinned public
     sources only;
   - whether ModemManager or a maintained open IMS client can register;
   - which proprietary pieces are missing.
   End it with an explicit GO or NO-GO before any voice code.
2. **The owner's carrier test plan.** Decide on a test SIM, carrier, test
   numbers, and LTE-only versus fallback conditions. Phone numbers and
   subscriber data never enter Git.
3. **Ordinary voice (if GO).** Prove each of these with two-way audio
   through spec 03's routes: outgoing, incoming, answer, reject, hang up,
   mute (real uplink mute), speaker/earpiece/headset switching, hold if
   supported, and network loss and recovery during a call.
4. **VoLTE/IMS.** Prove IMS registration as its own state; don't infer it
   from data registration. Record LTE-only results and any CSFB fallback.
5. **In-call UX.** Proximity blanking and touch lock during calls (spec 04),
   the in-call screen surviving a shell restart, and a missed-call
   notification (spec 11).
6. **Emergency policy.** Write an ADR with the owner:
   - the verification protocol, which must be coordinated with the carrier
     or an approved test facility;
   - UI rules while emergency service is unsupported;
   - what Ferrix tells the user on the lock screen and the dialer.
   **No agent places a live emergency call.**

## Acceptance

- A recorded GO/NO-GO for voice and for IMS.
- Physical evidence for every claimed voice feature, with carrier and test
  conditions and redacted call outcomes.
- Emergency calling either independently validated by the approved
  procedure, or still shown as unsupported in Phone, the lock screen and
  the matrix. While it is unsupported, the release report (spec 14) cannot
  say "fully functioning".

## Owner-gated

Everything involving a live carrier: SIM use, test calls, IMS provisioning,
any contact with carriers or Google, and the emergency policy.

## Risks

- NO-GO is a realistic outcome. If so, the owner has to decide whether a
  narrower release target is acceptable (for example "data/SMS phone,
  no carrier voice"), and that name must appear in the release report.
