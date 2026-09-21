# Contact Wallet Links and Message-Initiated Payments

Status: **backlog concept / not approved for implementation**  
Last reviewed: 2026-09-21

## Intent

Explore an opt-in Ferrix feature that lets a user associate public wallet
accounts with contacts and start a cryptocurrency payment from a contact or
message conversation. The desired experience may eventually include native
Solana accounts, assets, transaction flows, Solana ecosystem protocols, and
agent-payment systems such as pay.sh.

This document preserves the product idea and its dependency order. It is not
an implementation plan, an architecture decision, or evidence that Ferrix has
a wallet, secure key storage, Solana support, mainnet readiness, or physical
device security.

`plans/baseline.md` remains the implementation authority. The wallet and
Solana boundaries in `plans/ferrum-mobile-architecture/` also apply.

## Envisioned experience

- Wallet functionality is an optional product component. Contacts and
  Messages continue to work normally when it is absent or disabled.
- A contact may have one or more explicitly labelled, chain-qualified public
  accounts, with no private key material stored in the contacts database.
- From a contact or an existing Messages conversation, the user can create a
  payment draft such as `/pay 25 USDC`.
- The command is interpreted locally and is not sent as an ordinary SMS.
- A trusted review flow displays the contact, network, asset, exact amount,
  destination, fees, and wallet-verification state before asking a wallet to
  sign.
- A receipt or payment request may be shared through Messages, but SMS sender
  identity never authorizes a transaction.
- An incoming SMS may at most open an untrusted, reviewable payment request.
  It must never trigger automatic signing, submission, or retry.

## Why this is deferred

Contact payments sit near the end of a larger wallet programme. Implementing
the contact or SMS surface first would create pressure to put custody,
transaction policy, or signing authority in the wrong application.

The following capabilities need designs and evidence first:

1. **Trusted approval and authentication**
   - A confirmation surface ordinary applications cannot convincingly spoof.
   - Explicit authentication intent for every value-moving operation.
   - Lock/session integration, cancellation, timeout, and fail-closed crash
     behavior.
2. **Native wallet service boundary**
   - Project-owned, versioned wallet/signing APIs and protocols.
   - Separate permissions for account discovery, payment drafting, signing,
     submission, contact-link management, and history.
   - A decision between external-wallet signing, local custody, hardware-backed
     custody, or a supported combination.
3. **Key lifecycle and recovery**
   - Creation/import, encrypted storage, unlock, in-memory lifetime, backup,
     recovery, rotation, export, and deletion policies.
   - No seed phrase, private key, signing payload, full address, or sensitive
     memo in routine evidence records.
   - QEMU uses ephemeral test keys and cannot prove hardware-backed security.
4. **Chain-neutral transaction model**
   - Explicit network, account, asset, amount/decimal, fee, nonce or recent
     block reference, simulation, submission, and confirmation types.
   - Idempotency and an `unknown` submission outcome; uncertain broadcasts are
     never blindly retried.
   - Chain-qualified public-account identifiers where practical, with
     chain-specific canonicalization and checksum validation.
5. **Native Solana implementation**
   - A maintained Solana adapter for account/address validation, RPC access,
     transaction construction, simulation, signing requests, submission, and
     commitment tracking.
   - SOL and selected SPL-token handling, including mint identity, decimals,
     associated token accounts, fees, blockhash expiry, and durable user-facing
     failure states.
   - Deterministic local-validator or mock tests first, then devnet evidence.
     Mainnet remains a separate readiness gate.
6. **Ecosystem interoperability**
   - Mock Mobile Wallet Adapter flows in the Android compatibility runtime, as
     already anticipated by the mobile architecture plan.
   - Evaluate Solana Pay for payment requests and links.
   - Evaluate x402, MPP, and pay.sh for wallet-approved HTTP/API payments.
     pay.sh is related wallet infrastructure, not the protocol for linking a
     person's SMS number to a payment destination; keep those use cases
     separate behind shared signing policy where appropriate.
7. **Operational foundations**
   - RPC-provider trust and privacy policy, endpoint configuration, network
     failure behavior, rate limits, transaction-history ownership, token and
     address-book poisoning defenses, dependency review, and update policy.

## Tentative component boundary

Names are placeholders until an architecture decision is accepted.

```text
Contacts / Messages / future agents
              |
       narrow payment intents
              v
  payment policy and contact-link service
              |
       wallet/signing portal
              |
    chain adapter (initially Solana)
              |
  RPC / external signer / approved key backend
```

- `ferrix-contactsd` continues to own people, names, and telephone numbers.
- A payment service owns contact-to-public-account links, payment intents,
  transaction state, limits, and redacted history.
- A wallet/signing service owns accounts and signing policy. Contacts and
  Messages never receive private keys or direct signing authority.
- Chain and RPC implementations remain replaceable adapters. Product apps do
  not depend on Solana crates, RPC providers, Android services, pay.sh, QEMU,
  or board-specific facilities.
- Applications receive only the sockets implied by explicit wallet/payment
  capabilities, following the existing app identity and service authorization
  model.

## Suggested delivery sequence

1. Write and approve a wallet threat model and architecture decision.
2. Implement permissioned wallet contracts with a deterministic mock signer.
3. Add the trusted approval/authentication surface and negative authorization
   tests.
4. Implement the native Solana adapter against a local validator or deterministic
   mock, followed by devnet.
5. Evaluate Solana Pay, MWA, x402/MPP, and pay.sh as distinct integrations.
6. Add the separate contact-wallet link store and opt-in Contacts UI.
7. Add local payment-draft commands to Messages.
8. Complete security review, recovery exercises, graphical QEMU evidence, and
   an explicit mainnet-readiness decision before allowing real-value use.

Each step should be independently useful and must not silently enable the next
step. Mainnet, autonomous payments, and on-device production custody all
require separate explicit approval.

## Security and product invariants

- A telephone number is a lookup hint, not proof of wallet ownership.
- Incoming SMS content is untrusted data, never an authorization channel.
- Parsing a command creates only a draft. Signing always requires a separate,
  explicit approval event.
- New, changed, ambiguous, or unverified destinations receive stronger warnings
  and cannot be silently selected as defaults.
- The confirmation view shows the actual chain, asset/mint, amount, destination,
  and fee—not only a contact name or ticker.
- No component invents cryptography, seed storage, signature formats, or chain
  protocols when maintained implementations are available.
- Evidence uses synthetic contacts, test assets, and ephemeral test keys and is
  labelled `mock` or `QEMU` as appropriate.
- Physical-device secure storage, Seed Vault, TEE, biometric, attestation, and
  Seeker claims require exact-target evidence and the repository's board-support
  approval process.

## Questions to resolve before promotion from backlog

- Is the first wallet externally signed, locally custodial, or capable of both?
- Is Solana the first and only supported chain, or merely the first adapter to a
  chain-neutral contract?
- Which initial assets are allowed, and who owns the trusted asset registry?
- What authentication and secure-attention mechanism can the product shell
  actually guarantee?
- How are public wallet links verified, changed, recovered, exported, and
  removed when a contact is deleted?
- Are payment receipts local-only, optionally sent by SMS, or represented as
  signed payment links?
- What spending limits, cooling-off rules, and additional confirmation are
  required for new destinations or automation?
- Which pay.sh use cases belong on-device: user-driven paid API calls, agent
  budgets, provider tooling, or only protocol compatibility?
- What is the minimum evidence required before devnet and before mainnet?

## Promotion criteria

Promote this concept into a detailed plan or dedicated repository skill only
after the wallet service boundary, custody model, trusted confirmation path,
and initial Solana scope have accepted decisions. The promoted artifact should
then define schemas, crates, capabilities, migrations, mock profiles,
scenarios, graphical evidence, dependency/licensing review, and validation
gates.

## Research starting points

- [Ferrix mobile architecture](ferrum-mobile-architecture/SKILL.md)
- [Ferrix application platform](../docs/app-platform.md)
- [Solana payment integration overview](https://solana.com/docs/payments/accept-payments)
- [pay.sh documentation](https://pay.sh/docs)
- [Solana Foundation `pay` CLI](https://github.com/solana-foundation/pay)
- [CAIP-10 chain-qualified account identifiers](https://standards.chainagnostic.org/CAIPs/caip-10)
- [NIST guidance on restricted PSTN authentication](https://pages.nist.gov/800-63-4/sp800-63b.html)

External references are research inputs, not implementation authority. Verify
their current APIs, security model, maintenance, licenses, and compatibility
from primary sources when this backlog item is promoted.
