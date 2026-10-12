#!/bin/sh
set -eu

# Issue #108 decision-binding checker for the exact Google Pixel 9a
# google-pixel-9a-tegu-gxq96-mp1-0 S5300 S2 workflow. Offline: it reads
# local files only and never contacts a device.
#
#   s5300-s2-binding.sh check   RECORD FACTS   validate, do not consume
#   s5300-s2-binding.sh consume RECORD FACTS   consume by rename, then validate
#
# RECORD is a binding-<stamp>-<pid>.rec written by s5300-s2-preflight.sh for
# a `go`. FACTS is the transcript (preflight-<stamp>-<pid>.txt) of a fresh
# preflight that the caller ran itself, immediately before. The later active
# S2 workflow must call `consume` and act only on exit 0; a record that is
# consumed and then fails validation stays consumed.
#
# Refused: a record that is malformed, not 0600, unsealed or edited (HMAC
# with the host salt), mock-origin, not from a `go`, made with another salt,
# expired (10 minutes) or not yet valid, already consumed (renamed, or its
# record_id in the ledger), whose transcript is missing or changed; current
# facts that are not a fresh physical `go` in the same phase, with another
# unit, another boot, an unavailable identity, another kernel or slot; and
# checked-in library, body, package pins, mounts service, releases, or the
# pinned factory archive hash that differ from the record.
#
# Output: binding_check=pass|refused, binding_refusal=<token> on refusal,
# binding_record_id on pass, binding_consumed=<path> after a consume.
# Exit status: 0 pass, 2 usage, 7 refused. The salt is never printed.

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
target_id=google-pixel-9a-tegu-gxq96-mp1-0
# shellcheck source=tools/pixel-9a/s5300-s2-binding-lib.sh
. "$root/tools/pixel-9a/s5300-s2-binding-lib.sh"

usage() {
	printf 'usage: tools/pixel-9a/s5300-s2-binding.sh check|consume RECORD FACTS\n' >&2
	exit 2
}
[ "$#" -eq 3 ] || usage
case "$1" in check|consume) mode=$1 ;; *) usage ;; esac
record=$2
facts=$3

refuse() {
	printf 'binding_check=refused\nbinding_refusal=%s\n' "$1"
	exit 7
}

s2b_init "$root"
umask 077

# --- the record file -------------------------------------------------------------

case "${record##*/}" in
	binding-*.rec) ;;
	binding-*.rec.consumed-*) refuse record-consumed ;;
	*) refuse record-name ;;
esac
if [ ! -e "$record" ] && [ ! -L "$record" ]; then
	for sibling in "$record".consumed-*; do
		[ ! -e "$sibling" ] || refuse record-already-consumed
	done
fi
{ [ -f "$record" ] && [ ! -L "$record" ]; } || refuse record-unreadable
s2b_private "$record" 600 || refuse record-not-private
s2b_salt_load no || refuse "$s2b_reason"

if [ "$mode" = consume ]; then
	# rename(2) is atomic: of any number of consumers exactly one succeeds,
	# before any validation or action.
	consumed="$record.consumed-$(date +%s)-$$"
	mv -- "$record" "$consumed" 2>/dev/null || refuse record-already-consumed
	[ -f "$consumed" ] && [ ! -L "$consumed" ] || refuse record-already-consumed
	printf 'binding_consumed=%s\n' "$consumed"
	record=$consumed
fi

# --- schema and seal ----------------------------------------------------------------

awk -v keys="$s2b_record_keys" '
	BEGIN { n = split(keys, k, " ") }
	{
		eq = index($0, "=")
		if (NR > n || eq < 2 || substr($0, 1, eq - 1) != k[NR]) bad = 1
		if (substr($0, eq + 1) !~ /^[A-Za-z0-9._:,+~-]+$/) bad = 1
	}
	END { if (bad || NR != n) exit 1 }
' "$record" || refuse record-malformed
rget() { awk -v key="$1=" 'index($0, key) == 1 { print substr($0, length(key) + 1); exit }' "$record"; }
work=$(mktemp -d "${TMPDIR:-/tmp}/pixel-9a-s5300-s2-binding.XXXXXX")
trap 'rm -rf "$work"' EXIT HUP INT TERM
sed '$d' "$record" >"$work/body"
s2b_mac_equal "$(s2b_mac "$work/body")" "$(rget record_mac)" || refuse record-mac

record_id=$(rget record_id)
s2b_hex "$record_id" 32 || refuse record-malformed
if [ -f "$s2b_ledger" ] && grep -Fxq "$record_id" "$s2b_ledger"; then refuse record-consumed; fi
if [ "$mode" = consume ]; then
	{ [ ! -e "$s2b_ledger" ] || { [ -f "$s2b_ledger" ] && s2b_private "$s2b_ledger" 600; }; } || refuse ledger-not-private
	printf '%s\n' "$record_id" >>"$s2b_ledger"
fi

# --- the record's own claims ------------------------------------------------------------

[ "$(rget record)" = ferrix-s5300-s2-decision-binding ] && [ "$(rget record_version)" = 1 ] || refuse record-malformed
[ "$(rget target_id)" = "$target_id" ] || refuse record-target
[ "$(rget evidence)" = "physical:$target_id" ] && [ "$(rget preflight_gate)" = go ] || refuse record-mock-origin
[ "$(rget preflight_result)" = go ] || refuse record-not-go
[ "$(rget cp_boot_authorized)" = no ] && [ "$(rget single_use)" = consume-by-rename ] || refuse record-malformed
[ "$(rget binding_salt_id)" = "$(s2b_salt_id)" ] || refuse record-salt-changed
phase=$(rget preflight_phase)
case "$phase" in pre-mount|post-mount) ;; *) refuse record-malformed ;; esac
unit=$(rget unit_identity)
boot=$(rget boot_identity)
s2b_hex "$unit" 16 && s2b_hex "$boot" 16 || refuse record-malformed

created=$(rget created_epoch)
expires=$(rget expires_epoch)
case "$created:$expires" in *[!0-9:]*|:*|*:) refuse record-malformed ;; esac
[ "$((expires - created))" -eq "$s2b_lifetime" ] || refuse record-malformed
now=$(date +%s)
case "$now" in ''|*[!0-9]*) refuse clock-unreadable ;; esac
[ "$now" -ge "$created" ] || refuse record-not-yet-valid
[ "$now" -le "$expires" ] || refuse record-expired

record_dir=$(dirname -- "$record")
transcript="$record_dir/$(rget preflight_transcript)"
case "$(rget preflight_transcript)" in preflight-*.txt) ;; *) refuse record-malformed ;; esac
{ [ -f "$transcript" ] && [ ! -L "$transcript" ]; } || refuse transcript-missing
[ "$(s2b_transcript_sha "$transcript")" = "$(rget preflight_transcript_sha256)" ] || refuse transcript-mismatch

# --- checked-in pins, now -------------------------------------------------------------

s2b_host_pins "$root"
for pair in "library_sha256|$s2b_pin_library" "probe_body_sha256|$s2b_pin_body" "composed_probe_sha256|$s2b_pin_composed" \
	"package_pins_sha256|$s2b_pin_pins" "mounts_service_sha256|$s2b_pin_mounts" "pkg.device-google-tegu|$s2b_pin_device" \
	"pkg.exynos-modem|$s2b_pin_modem" "pkg.modemmanager-samsung-sit|$s2b_pin_mm"; do
	[ "$(rget "${pair%%|*}")" = "${pair#*|}" ] || refuse pins-changed
done
[ "$(rget host_recovery_factory_archive)" = verified ] || refuse host-recovery-not-verified
s2b_hex "$s2b_pin_factory" 64 && [ "$(rget factory_archive_sha256)" = "$s2b_pin_factory" ] || refuse factory-changed

# --- current facts --------------------------------------------------------------------

{ [ -f "$facts" ] && [ ! -L "$facts" ]; } || refuse facts-unreadable
s2b_private "$facts" 600 || refuse facts-not-private
case "${facts##*/}" in preflight-*.txt) ;; *) refuse facts-unreadable ;; esac
# A record's own transcript (or a copy of it) is not a fresh observation.
{ [ "${facts##*/}" != "${transcript##*/}" ] && ! cmp -s "$facts" "$transcript"; } || refuse facts-not-fresh
# Only the host's classification counts: lines before the raw probe output.
fget() {
	awk -v key="$1=" '$0 == "## raw probe output" { exit } index($0, key) == 1 { n++; v = substr($0, length(key) + 1) }
		END { if (n == 1) print v }' "$facts"
}
[ "$(fget target_id)" = "$target_id" ] || refuse facts-malformed
[ "$(fget evidence)" = "physical:$target_id" ] || refuse facts-not-physical
[ "$(fget preflight_result)" = go ] && [ "$(fget preflight_gate)" = go ] || refuse facts-not-go
[ "$(fget preflight_phase)" = "$phase" ] || refuse phase-mismatch
[ "$(fget binding_salt_id)" = "$(rget binding_salt_id)" ] || refuse record-salt-changed
s2b_hex "$(fget unit_identity)" 16 || refuse unit-identity-unavailable
[ "$(fget unit_identity)" = "$unit" ] && [ "$(fget unit_identity_source)" = "$(rget unit_identity_source)" ] || refuse unit-mismatch
s2b_hex "$(fget boot_identity)" 16 || refuse boot-identity-unavailable
[ "$(fget boot_identity)" = "$boot" ] || refuse boot-changed
[ "$(fget kernel_release)" = "$(rget kernel_release)" ] || refuse kernel-changed
[ "$(fget slot_suffix)" = "$(rget slot_suffix)" ] || refuse slot-changed
[ "$(fget library_sha256)" = "$s2b_pin_library" ] && [ "$(fget probe_body_sha256)" = "$s2b_pin_body" ] || refuse pins-changed
[ "$(fget host_recovery_factory_archive)" = verified ] || refuse host-recovery-not-verified

printf 'binding_check=pass\nbinding_record_id=%s\nbinding_phase=%s\n' "$record_id" "$phase"
