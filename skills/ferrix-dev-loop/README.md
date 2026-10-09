# ferrix-dev-loop skill

The build → deploy → see → drive → evidence loop for Omarchy Ferrix on QEMU
and the Pixel 9a dev route, with the helper scripts it uses. `SKILL.md` is the
playbook and `scripts/` holds the POSIX sh helpers. Machine-local settings go
in the ignored `local/dev/dev-loop.env` (keys are listed in `scripts/env.sh`).
The older `out/dev-loop.env` path still works.

Load it the same way as `skills/mobile-device-provisioning` (see its README):
symlink the folder into `.claude/skills/`, `.codex/skills/`, or paste
`SKILL.md` into your agent's context. It is subordinate to `AGENTS.md` and to
the provisioning skill's device gates.
