#!/bin/sh
# Verify the repository's first-party license policy without modifying files.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)

python3 - "$root" <<'PY'
import json
import pathlib
import subprocess
import sys

root = pathlib.Path(sys.argv[1]).resolve()
metadata = subprocess.check_output(
    [
        "cargo",
        "metadata",
        "--manifest-path",
        str(root / "Cargo.toml"),
        "--no-deps",
        "--locked",
        "--format-version",
        "1",
    ],
    cwd=root,
    text=True,
)
data = json.loads(metadata)
packages = []
failures = []

for package in data["packages"]:
    manifest = pathlib.Path(package["manifest_path"]).resolve()
    try:
        manifest.relative_to(root / "crates")
    except ValueError:
        continue
    packages.append(package["name"])
    if package.get("license") != "Apache-2.0":
        failures.append(
            f"{package['name']} ({manifest}): {package.get('license')!r}"
        )

if failures:
    print("FAIL first-party Rust package license metadata:", file=sys.stderr)
    for failure in failures:
        print(f"  {failure}", file=sys.stderr)
    raise SystemExit(1)

if not packages:
    print("FAIL no first-party Rust packages found", file=sys.stderr)
    raise SystemExit(1)

print(f"PASS {len(packages)} first-party Rust packages declare Apache-2.0")
PY

test -f "$root/LICENSE"
printf '%s\n' 'PASS canonical Apache-2.0 LICENSE exists'

python3 - "$root" <<'PY'
import json
import pathlib
import sys

root = pathlib.Path(sys.argv[1]).resolve()
descriptors = sorted((root / "apps").glob("org.ferrix.*.json"))
failures = []

for path in descriptors:
    try:
        descriptor = json.loads(path.read_text())
    except (OSError, json.JSONDecodeError) as error:
        failures.append(f"{path}: {error}")
        continue
    if descriptor.get("id") != path.stem:
        failures.append(
            f"{path}: id {descriptor.get('id')!r} does not match the filename"
        )

if failures:
    print("FAIL first-party application descriptor audit:", file=sys.stderr)
    for failure in failures:
        print(f"  {failure}", file=sys.stderr)
    raise SystemExit(1)

print(f"PASS {len(descriptors)} first-party application descriptors are valid")
PY
