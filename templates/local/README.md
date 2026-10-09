# Private development workspace

This directory is ignored by Git. Keep machine names, device identifiers,
credentials, recovery notes, and personal development settings here. Do not
copy private values into tracked source files, issues, or build manifests.

```text
local/
  dev/                 host and builder settings
    dev-loop.env.example
    dev-loop.env       optional: copied and filled in locally
  devices/
    pixel-9a/
      device.md         private identity and recovery notes
  audits/              private review reports and temporary snapshots
```

The dev-loop scripts load `local/dev/dev-loop.env` when it exists. They still
accept the older `out/dev-loop.env` file. Keep build outputs and device
workflow artifacts in their existing ignored `out/` paths; scripts depend on
those paths. The `local/` directory is for per-developer configuration and
notes, not an input-image or firmware store.

See `docs/pixel-9a-developer-setup.md` for the clone-to-development path.
