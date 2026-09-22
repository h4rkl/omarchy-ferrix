# Open Wallet Standard integration (future)

Status: **backlog concept / not approved for implementation**
Last reviewed: 2026-09-22

## Purpose

Evaluate [Open Wallet Standard (OWS)](https://openwallet.sh/) as an optional,
local signing backend for future Ferrix wallet and agent-payment features. OWS
describes a local encrypted vault, a chain-neutral signing interface, and
policy-gated requests intended to keep private keys out of agent processes.
This is a research and sequencing note, not evidence that Ferrix supports OWS,
custody, any chain, agent payments, or real-value transactions.

`plans/baseline.md` remains authoritative. This plan extends, but does not
replace, [contact-wallet-payments.md](contact-wallet-payments.md): contact
links and Messages remain untrusted callers that may create drafts only.

## Proposed shape

```text
Apps / Messages / future agents
        | permissioned payment intent
        v
Ferrix wallet portal + trusted approval
        | versioned, narrow adapter
        v
OWS backend (optional local process/vault)
        | signed transaction only
        v
chain RPC or payment protocol
```

- Do not expose an OWS CLI, MCP server, vault path, seed phrase, or private key
  to ordinary apps, the shell, an LLM context, or logs.
- The Ferrix portal owns caller identity, permissions, user confirmation,
  limits, cancellation, and redacted history; OWS is a replaceable backend,
  not Ferrix's public application API.
- Keep OWS integration outside `ferrix-hardware-api`, target adapters, and
  application runtime implementations. Its absence must produce a typed
  `unsupported` result and leave Contacts and Messages usable.

## Delivery gates

1. Review the upstream specification/source, release provenance, maintenance,
   license, supported Rust integration route, vault location/permissions,
   cryptographic and process-isolation assumptions, and upgrade/migration
   behavior. Do not run a curl-to-shell installer in project workflows.
2. Approve a Ferrix wallet threat model and portal contract before selecting
   OWS. Define explicit transaction fields, simulation, fee display,
   idempotency, unknown-broadcast handling, and policy ownership.
3. Build a deterministic mock signer and QEMU-only tests for allow, deny,
   cancel, limit-exceeded, malformed request, backend crash, and unavailable
   backend paths. Test keys and assets are ephemeral; no real network is
   required.
4. Add an opt-in OWS adapter only after the above gates pass. Run it with the
   minimum filesystem and IPC access, verify every request against Ferrix
   policy, and present a trusted review surface before signing.
5. Evaluate individual integrations (such as x402, MPP, Solana, or MCP) as
   separate, permissioned adapters. Autonomous or mainnet spending requires a
   separate explicit approval and security review.

## Evidence and non-goals

Initial evidence is limited to `mock`/`QEMU` test transactions and inspected
approval UI. It cannot establish hardware-backed custody, TEE/Seed Vault,
physical-device security, or production readiness. Never commit vaults,
keys, recovery material, full addresses, or signing payloads.

## Research inputs

- [MoonPay announcement, 2026-03-23](https://www.moonpay.com/newsroom/open-wallet-standard)
- [Open Wallet Standard](https://openwallet.sh/)
- [Existing Ferrix contact-wallet plan](contact-wallet-payments.md)

Verify all external APIs, security claims, licenses, and compatibility from
primary sources when this item is promoted.
