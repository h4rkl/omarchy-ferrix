# mobile-device-provisioning skill

This skill lives at `skills/mobile-device-provisioning/` in the Omarchy Ferrix
repository and is referenced from `AGENTS.md`. It is an LLM-agnostic playbook in
the open Agent Skills convention: `SKILL.md` with
YAML frontmatter (`name`, `description`) plus a `references/` folder the agent
opens on demand. It tells any coding agent how to identify an attached Android
phone, match it to a checked-in Ferrix board target, fetch official recovery
images, back up and unlock, restore stock, build and flash Ferrix Linux, bring
up radios, and record redacted evidence, never running a raw `adb`/`fastboot`
mutation and always with per-run human confirmation.

Loading it:

- **Claude Code**: copy or symlink this folder to
  `.claude/skills/mobile-device-provisioning/` and invoke `/mobile-device-provisioning`.
- **Codex**: place or symlink the folder under `.codex/skills/` and reference it
  by name (check your tool's current docs for the exact path).
- **Cursor**: add `SKILL.md` as a project rule under `.cursor/rules/` or attach it
  with `@` (check your tool's current docs).
- **Any other agent**: paste `SKILL.md` into the system prompt and give the agent
  file access to `references/` and the repository.

The skill is subordinate to `AGENTS.md`; it grants no permission the repository withholds.
