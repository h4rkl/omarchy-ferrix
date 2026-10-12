# plans/ index

This index says what each entry in `plans/` is for and how much weight it
carries. It adds no rules. [`AGENTS.md`](../AGENTS.md) decides when each
document must be read.

- **Authority.** These decide architecture and scope.
  [`baseline.md`](baseline.md) is the implementation authority, and
  [`plan.md`](plan.md) is the long-term direction. Read both before changing
  architecture, targets, guest construction, hardware APIs or the product
  shell. Baseline wins conflicts.
- **Proposal/strategy.** Forward designs, concepts and positioning. None of
  them is approval to implement or evidence that anything exists. Where one
  conflicts with an authority document, the authority document wins.
- **Delivery tracker.** A persistent brief for one delivery goal. It points
  to GitHub issues for live status, to `docs/status.md` for dated evidence
  and to the board capability matrix for capability claims.

Status meanings:

- **live:** current and maintained.
- **draft:** a proposal or concept, not approved for implementation.
- **superseded:** replaced by another document. No entry is superseded at
  present.

| Entry | Class | Status | Purpose |
| --- | --- | --- | --- |
| [`baseline.md`](baseline.md) | Authority | live | Implementation authority for the generic AArch64 QEMU virtual phone: the baseline decisions, boundaries and checks. Wins conflicts. |
| [`plan.md`](plan.md) | Authority | live | Long-term direction: the mission, the safety contract, the architecture and the phased roadmap. |
| [`pixel-9a-daily-phone.md`](pixel-9a-daily-phone.md) | Delivery tracker | live | Brief for making the exact Pixel 9a `google-pixel-9a-tegu-gxq96-mp1-0` a dependable daily phone. It holds the release gate, an evidence snapshot and the workstreams. |
| [`ferrix-mobile-architecture/`](ferrix-mobile-architecture/SKILL.md) | Proposal/strategy | live | Proposed Rust-first, Linux-hosted architecture with Rust and TypeScript apps and an optional contained Android runtime. [`docs/app-platform.md`](../docs/app-platform.md) implements phases 1 and 2 of its path. It is written as an agent skill, with references and `agents/openai.yaml`; its frontmatter still uses the earlier name `ferrum-mobile-architecture`. |
| [`ferrix-app-foundry/`](ferrix-app-foundry/SKILL.md) | Proposal/strategy | draft | Forward blueprint for a catalogue and an agent-assisted foundry that turns upstream Omarchy plugin ideas into reviewable Ferrix apps. Written as an agent skill. |
| [`ferrix-encrypted-spaces/`](ferrix-encrypted-spaces/SKILL.md) | Proposal/strategy | draft | Forward blueprint for trusted locking and multiple encrypted spaces, including cover-PIN behaviour, key and mount lifecycle and notification isolation. Written as an agent skill. |
| [`ferrix-marketing-position/`](ferrix-marketing-position/SKILL.md) | Proposal/strategy | draft | Product positioning and branding notes for Omarchy Ferrix. This is not an engineering specification. |
| [`ferrix-mobile-interaction-plan.md`](ferrix-mobile-interaction-plan.md) | Proposal/strategy | draft | Design proposal for a command-first touch interaction system built around a persistent command pill. |
| [`contact-wallet-payments.md`](contact-wallet-payments.md) | Proposal/strategy | draft | Backlog concept, not approved: contact wallet links and payments started from Messages. |
| [`open-wallet-standard.md`](open-wallet-standard.md) | Proposal/strategy | draft | Backlog concept, not approved: Open Wallet Standard as an optional local signing backend. |

When you add a document to `plans/`, add a row here with its class and
status. When a document is replaced, mark it `superseded` and name its
replacement rather than deleting it.
