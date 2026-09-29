#!/bin/sh
# Offline proof for the board-target scaffolder. It writes only into temporary
# FERRIX_BOARDS_DIR / FERRIX_DOCS_DIR copies and proves the fake serial never
# reaches any produced file.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
propose="$root/tools/device/propose-board.sh"
match="$root/tools/device/match-target.sh"
fails=0
# shellcheck disable=SC1091
. "$here/lib.sh"

work=$(mktemp -d "${TMPDIR:-/tmp}/device-propose-test.XXXXXX")
trap 'rm -rf "$work"' EXIT
boards="$work/boards"
docs="$work/docs"
mkdir -p "$boards" "$docs" "$work/out/device"
copy_board_metadata "$root/targets/boards" "$boards"
export FERRIX_BOARDS_DIR="$boards" FERRIX_DOCS_DIR="$docs" FERRIX_OUT_DIR="$work/out"

fake_serial='SYN.00[1]/&$FAKESERIAL'
# The identity file itself must never carry the serial; write it the way
# discover.sh would (prefix only), and additionally plant the raw serial in a
# transcript next to it to prove the scaffolder does not read transcripts.
write_identity "$work/out/device/identity-latest.env" OnePlus "SYN 2001" synthetic-op "SKU'7" EVT2
printf 'serial=%s\n' "$fake_serial" > "$work/out/device/discovery-20260928T000000Z.txt"

check "static: scaffolder never invokes adb/fastboot/curl" sh -c "! grep -nE '^[[:space:]]*[^#]*(adb|fastboot|curl)([[:space:]]|$)' '$propose'"

run_propose() {
  status=0
  sh "$propose" "$@" > "$work/stdout" 2> "$work/stderr" || status=$?
}

run_propose
check "propose: unknown synthetic vendor succeeds" [ "$status" -eq 0 ]
tid=oneplus-syn-2001-synthetic-op-sku-7-evt2
tdir="$boards/$tid"
check "propose: target_id derived in pixel style" grep -q "^target_id=$tid\$" "$work/stdout"
for f in manifest.toml capabilities.toml recovery.pins README.md; do
  check "propose: $f created" [ -f "$tdir/$f" ]
  check "propose: $f path printed" grep -q "^created=$tdir/$f\$" "$work/stdout"
done
check "propose: proposal doc created under FERRIX_DOCS_DIR" [ -f "$docs/synthetic-op-attached.md" ]

check "manifest: kind physical" grep -q '^kind = "physical"$' "$tdir/manifest.toml"
check "manifest: status proposed-identity-only" grep -q '^status = "proposed-identity-only"$' "$tdir/manifest.toml"
check "manifest: schema_version 1" grep -q '^schema_version = 1$' "$tdir/manifest.toml"
check "manifest: manufacturer recorded" grep -q '^manufacturer = "OnePlus"$' "$tdir/manifest.toml"
check "manifest: quote-bearing SKU escaped as TOML" grep -q "^sku = \"SKU'7\"\$" "$tdir/manifest.toml"
check "manifest: hardware_revision recorded" grep -q '^hardware_revision = "EVT2"$' "$tdir/manifest.toml"
check "manifest: discovery workflow recorded" grep -q '^discovery_workflow = "just device-discover"$' "$tdir/manifest.toml"
check "manifest: discovered_utc recorded" grep -q '^discovered_utc = "20260928T000000Z"$' "$tdir/manifest.toml"
check "manifest: capability_matrix pointer" grep -q '^capability_matrix = "capabilities.toml"$' "$tdir/manifest.toml"
check "manifest: recovery_pins pointer" grep -q '^recovery_pins = "recovery.pins"$' "$tdir/manifest.toml"
check "manifest: gates identity passed" grep -q '^identity = "passed"$' "$tdir/manifest.toml"
check "manifest: per_run_confirmation required" grep -q '^per_run_confirmation = "required"$' "$tdir/manifest.toml"
check "manifest: every other gate pending" sh -c "! awk '/^\\[gates\\]/{g=1;next} g' '$tdir/manifest.toml' | grep -vE '^(identity = \"passed\"|per_run_confirmation = \"required\"|[a-z_]+ = \"pending\")$' | grep -q ."
check "manifest: only [gates] table plus top-level keys" sh -c "[ \"\$(grep -c '^\\[' '$tdir/manifest.toml')\" -eq 1 ]"

check "capabilities: mock-control unsupported" grep -q '^\[mock-control\]$' "$tdir/capabilities.toml"
check "capabilities: nine unsupported sections" sh -c "[ \"\$(grep -c '^status = \"unsupported\"$' '$tdir/capabilities.toml')\" -eq 9 ]"
check "capabilities: evidence sentence matches Pixel wording" sh -c "[ \"\$(grep -c '^evidence = \"No Ferrix adapter or physical Linux evidence.\"$' '$tdir/capabilities.toml')\" -eq 8 ]"
check "capabilities: no status other than unsupported" sh -c "! grep '^status = ' '$tdir/capabilities.toml' | grep -vq unsupported"

check "pins: header line first" sh -c "head -1 '$tdir/recovery.pins' | grep -q '^# kind<TAB>build<TAB>version<TAB>filename<TAB>url<TAB>sha256<TAB>bytes<TAB>source-page$'"
check "pins: only comment lines" sh -c "! grep -vq '^#' '$tdir/recovery.pins'"
check "pins: explains SHA-256 and byte-size pinning" grep -q 'SHA-256' "$tdir/recovery.pins"

check "readme: no support claim" grep -q 'not a[[:space:]]*$' "$tdir/README.md"
check "readme: identity table row for revision" grep -q '^| Hardware revision | `EVT2` |$' "$tdir/README.md"
check "readme: support boundary section" grep -q '^## Support boundary$' "$tdir/README.md"
check "readme: sources section" grep -q '^## Sources$' "$tdir/README.md"

doc="$docs/synthetic-op-attached.md"
for heading in \
  "Owner authorization" \
  "Exact manufacturer model number, codename, and hardware revision" \
  "Bootloader state and supported boot route" \
  "Kernel, device-tree, and driver sources" \
  "Partition map" \
  "Backup and recovery procedure" \
  "Firmware handling" \
  "Capability matrix" \
  "Evidence plan" \
  "Data-collection policy"; do
  check "proposal: heading '$heading'" grep -q "^## $heading\$" "$doc"
done
check "proposal: states NOT approved" grep -q 'This proposal is NOT approved' "$doc"
check "proposal: destructive provisioning requires owner approval and gates" grep -q 'requires explicit owner' "$doc"
check "proposal: pending status rows present" grep -q '| pending |$' "$doc"
check "proposal: no generalization across revisions" grep -q 'another hardware revision' "$doc"

check "privacy: fake serial absent from every produced file" sh -c "! grep -rF '$fake_serial' '$tdir' '$docs'"
check "privacy: no serial key in produced files" sh -c "! grep -rE '(^|[^_])serial(no)? ?=' '$tdir' '$docs' | grep -v serial_sha256_prefix"
check "privacy: only the SHA-256 prefix is carried" grep -q '^serial_sha256_prefix = "0123456789ab"$' "$tdir/manifest.toml"
check "reminder: owner approval reminder printed" grep -q 'NOT approved' "$work/stdout"
check "reminder: gitignore block names recovery dir" grep -q "^/targets/boards/$tid/recovery/\$" "$work/stdout"
check "reminder: gitignore block names key glob" grep -q "^/targets/boards/$tid/\\*\\*/\\*.key\$" "$work/stdout"

# The scaffolded target must now match its own identity.
status=0
sh "$match" "$work/out/device/identity-latest.env" > "$work/match-stdout" 2>&1 || status=$?
check "roundtrip: scaffolded target matches the identity exactly" [ "$status" -eq 0 ]
check "roundtrip: match names the scaffolded target" grep -q "^match=exact target_id=$tid " "$work/match-stdout"

# Re-running refuses because the target now exists / matches.
run_propose
check "refusal: second run refuses" [ "$status" -eq 1 ]

# Known Pixel identity refuses because it already matches.
write_identity "$work/id-pixel.env" Google "Pixel 9a" tegu GXQ96 MP1.0
run_propose "$work/id-pixel.env"
check "refusal: known Pixel 9a identity is not re-proposed" [ "$status" -eq 1 ]
check "refusal: message points to device-match" grep -q 'device-match' "$work/stderr"
check "refusal: no Pixel proposal doc written" [ ! -e "$docs/tegu-attached.md" ]

# Missing revision or manufacturer refuses.
write_identity "$work/id-norev.env" OnePlus SYN2001 synthetic-op2 SKU7 ""
run_propose "$work/id-norev.env"
check "refusal: missing hardware_revision refuses" [ "$status" -eq 1 ]
write_identity "$work/id-noman.env" "" "" synthetic-op3 SKU7 EVT2 fastboot
run_propose "$work/id-noman.env"
check "refusal: missing manufacturer refuses" [ "$status" -eq 1 ]

# Missing identity refuses.
run_propose "$work/nope.env"
check "refusal: missing identity refuses" [ "$status" -eq 1 ]

check "isolation: nothing written under the real targets/boards" [ ! -e "$root/targets/boards/$tid" ]
check "isolation: nothing written under the real docs/board-support" [ ! -e "$root/docs/board-support/synthetic-op-attached.md" ]

finish propose
