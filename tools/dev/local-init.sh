#!/bin/sh
# Create private per-checkout development notes without changing build inputs.
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
template_root=$repo_root/templates/local
local_root=$repo_root/local

umask 077
mkdir -p "$local_root/dev" "$local_root/devices/pixel-9a" "$local_root/audits"

for relative_path in README.md dev/dev-loop.env.example devices/pixel-9a/device.md; do
  destination=$local_root/$relative_path
  if [ ! -e "$destination" ] && [ ! -L "$destination" ]; then
    cp "$template_root/$relative_path" "$destination"
  fi
done

printf 'Local workspace ready: %s\n' "$local_root"
printf 'Private settings template: %s\n' "$local_root/dev/dev-loop.env.example"
