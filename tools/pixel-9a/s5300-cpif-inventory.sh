#!/bin/sh
set -eu

# Issue #1 E1: read-only inventory of the Google Kleaf CPIF surface on an
# ALREADY booted Google Pixel 9a (tegu / GXQ96 / MP1.0) Ferrix dev guest,
# reached over the USB CDC-ACM bring-up shell (the rootfs has no sshd).
# tools/pixel-9a/acm-probe.py sends the exact bytes of the guest probe
# (s5300-cpif-inventory-remote.sh). The probe checks the exact device tree,
# then reads /proc/modules, the samsung,exynos-cp platform device's driver
# link and modem_state, inode metadata of the CPIF /dev nodes, their sysfs
# class entries, and the wwan/rmnet sysfs counts. It never opens a CPIF
# character device (open() runs the link's init_comm), never boots or powers
# the CP, and reads no dmesg, node contents, serials, SIM or IMEI data.
#
# The host compares those facts with the E1 criteria in
# targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/s5300-route-decision.md
# and reports e1_result=go|no-go|incomplete. `go` means only that the
# pre-boot kernel surface matches the route-A expectation; see e1_unknowns.
#
# FERRIX_SHELL selects the host CDC-ACM node (default
# /dev/cu.usbmodemferrix_tegu3); it is never printed or retained. Set
# FERRIX_PIXEL9A_S5300_CPIF_EVIDENCE=mock for stubbed runs; the default label
# is the exact physical target. Transcripts go to ignored
# out/pixel-9a/s5300-cpif-inventory/.

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
target_id=google-pixel-9a-tegu-gxq96-mp1-0
probe="$root/tools/pixel-9a/s5300-cpif-inventory-remote.sh"
transport="$root/tools/pixel-9a/acm-probe.py"
out="${FERRIX_OUT_DIR:-$root/out}/pixel-9a/s5300-cpif-inventory"
modules='cpif shm_ipc cpif_page'
nodes='umts_boot0 umts_ipc0 umts_ipc1 umts_rfs0 oem_ipc0 oem_ipc1 oem_ipc2 oem_ipc3 oem_ipc4 oem_ipc5 oem_ipc6 oem_ipc7'
# Source claim only (s5300-source-evidence.md): Google's module registers a
# `cpif` class. Recorded for comparison; the observed class is what counts.
source_class=cpif
dt_model='ZUMA PRO TEGU MP on ZUMA PRO'
dt_compatible='google,ZUMA PRO TEGU'
max_output=65536
acm_timeout=${FERRIX_PIXEL9A_ACM_TIMEOUT:-60}
unknowns='cp-boot-runtime,nv-file-sizes,fmt-payload-equivalence,rfs-service,node-open-runtime-behaviour,rmnet-after-online,cp-crash-handling,driver-firmware-skew'

usage() {
	printf 'usage: tools/pixel-9a/s5300-cpif-inventory.sh\n' >&2
	printf 'environment: FERRIX_SHELL (CDC-ACM node) FERRIX_PIXEL9A_ACM_TIMEOUT (seconds, default 60)\n' >&2
	printf '             FERRIX_PIXEL9A_S5300_CPIF_EVIDENCE (physical|mock) FERRIX_OUT_DIR\n' >&2
	exit 2
}

[ "$#" -eq 0 ] || usage
case "${FERRIX_PIXEL9A_S5300_CPIF_EVIDENCE:-physical}" in
	physical) evidence="physical:$target_id" ;;
	mock) evidence=mock ;;
	*) usage ;;
esac
case "$acm_timeout" in ''|*[!0-9]*) usage ;; esac
[ "$acm_timeout" -ge 1 ] && [ "$acm_timeout" -le 3600 ] || usage

transcript=''
die() {
	code=${2:-1}
	[ -z "$transcript" ] || printf 'result=refused\nreason=%s\n' "$1" >>"$transcript"
	printf 'REFUSING pixel-9a S5300 CPIF E1 inventory: %s\n' "$1" >&2
	printf '{"event":"pixel-9a-s5300-cpif-inventory","evidence":"%s","status":"failed","reason":"%s","exit":%s}\n' \
		"$evidence" "$(printf '%s' "$1" | tr '"\\' "''")" "$code" >&2
	exit "$code"
}

for tool in python3 awk sed; do
	command -v "$tool" >/dev/null 2>&1 || die "missing host tool '$tool'; install it explicitly"
done
[ -f "$probe" ] && [ ! -L "$probe" ] || die 'guest probe script is missing or linked'
[ -f "$transport" ] && [ ! -L "$transport" ] || die 'CDC-ACM transport is missing or linked'

mkdir -p "$out"
stamp=$(date -u +%Y%m%dT%H%M%SZ)
transcript="$out/inventory-$stamp-$$.txt"
raw=$(mktemp "${TMPDIR:-/tmp}/pixel-9a-s5300-cpif.XXXXXX")
transport_log=$(mktemp "${TMPDIR:-/tmp}/pixel-9a-s5300-cpif-transport.XXXXXX")
trap 'rm -f "$raw" "$transport_log"' EXIT HUP INT TERM
{
	printf '# Pixel 9a S5300 CPIF E1 read-only inventory (issue #1), %s UTC\n' "$stamp"
	printf '# evidence: %s\n' "$evidence"
	printf '# No serials, port names, SIM/IMEI data, kernel log, or node contents are collected. Never commit this file.\n'
	printf 'target_id=%s\nevidence=%s\nsource=cdc-acm-read-only\ncp_nodes_opened=no\n' "$target_id" "$evidence"
	printf 'probe_sha256=%s\n' "$( (shasum -a 256 "$probe" 2>/dev/null || sha256sum "$probe") | cut -c1-64)"
} >"$transcript"

# The only guest session; see s5300-source-inventory.sh for the transport
# contract (exact bytes, guest-side hash check, stderr discarded, bounded).
transport_status=0
python3 "$transport" --probe "$probe" --timeout "$acm_timeout" --max-bytes "$max_output" --stderr discard \
	>"$raw" 2>"$transport_log" || transport_status=$?
case "$transport_status" in
	0) ;;
	255)
		reason=$(sed -n 's/^ferrix-acm: .* reason=\([a-z-]*\)$/\1/p' "$transport_log" | sed -n '1p')
		die "guest CDC-ACM shell transport failed (${reason:-unknown})" 3
		;;
	4) die 'guest is not the exact tegu board (device-tree gate refused)' 4 ;;
	*) die "guest probe failed (exit $transport_status)" 3 ;;
esac

# --- validate the probe output ----------------------------------------------

[ "$(wc -c <"$raw" | tr -d ' ')" -le "$max_output" ] || die 'probe output exceeds the reviewed size limit' 5
awk -v modules="$modules" -v nodes="$nodes" '
	BEGIN {
		split("probe dt_model dt_compatible kernel_release uid modules_readable platform_readable cp_platform_count cp_platform_driver cp_modem_state_file cp_modem_state stat_impl oem_ipc_name_count wwan_devices rmnet_count probe_complete", g, " ")
		for (i in g) required[g[i]] = 1
		split(modules, m, " "); split("count state sysfs", mf, " ")
		for (i in m) for (j in mf) required["module." m[i] "." mf[j]] = 1
		split(nodes, n, " "); split("kind link rdev mode owner classes sysfs_dev subsystem", nf, " ")
		for (i in n) for (j in nf) required["node." n[i] "." nf[j]] = 1
	}
	{
		eq = index($0, "=")
		if (eq < 2) { bad = 1; next }
		key = substr($0, 1, eq - 1)
		if (!(key in required) || (key in seen)) bad = 1
		if (substr($0, eq + 1) !~ /^[A-Za-z0-9 ._:,+~-]*$/) bad = 1
		seen[key] = 1
	}
	END { if (bad) exit 1; for (k in required) if (!(k in seen)) exit 1 }
' "$raw" || die 'probe output has a malformed, unknown, duplicate, or missing field' 5

get() {
	awk -v key="$1=" 'index($0, key) == 1 { print substr($0, length(key) + 1); exit }' "$raw"
}
count_field() {
	value=$(get "$1")
	case "$value" in ''|*[!0-9]*) die "probe reported a malformed count for $1" 5 ;; esac
	printf '%s' "$value"
}
yes_no() {
	value=$(get "$1")
	case "$value" in yes|no) printf '%s' "$value" ;; *) die "probe reported a malformed $1" 5 ;; esac
}

[ "$(get probe)" = s5300-cpif-inventory ] && [ "$(get probe_complete)" = yes ] ||
	die 'probe did not complete' 5
[ "$(get dt_model)" = "$dt_model" ] && [ "$(get dt_compatible)" = "$dt_compatible" ] ||
	die 'probe reported a device tree other than the exact tegu board' 4

emit() {
	printf '%s=%s\n' "$1" "$2" >>"$transcript"
	printf '%s=%s\n' "$1" "$2"
}

# no_go: an observed fact contradicts E1. unknown: a fact could not be read.
no_go=''
missing=''
nogo() { no_go="$no_go${no_go:+,}$1"; }
unknown() { missing="$missing${missing:+,}$1"; }

emit kernel_release "$(get kernel_release)"
emit guest_uid "$(get uid)"

# --- modules -----------------------------------------------------------------

modules_readable=$(yes_no modules_readable)
emit modules_readable "$modules_readable"
for name in $modules; do
	count=$(count_field "module.$name.count")
	state=$(get "module.$name.state")
	sysfs=$(yes_no "module.$name.sysfs")
	if [ "$modules_readable" != yes ]; then status=unknown
	elif [ "$count" -gt 1 ]; then die "probe reported $count /proc/modules lines for $name" 5
	elif [ "$count" -eq 1 ]; then
		case "$state" in
			Live) status=loaded ;;
			Loading|Unloading) status=transitional ;;
			*) die "probe reported a malformed /proc/modules state for $name" 5 ;;
		esac
	elif [ "$sysfs" = yes ]; then status=builtin
	else status=absent
	fi
	# The pinned Kleaf build ships these as modules (modules-initfs);
	# built-in means a different kernel than the one E1 is about.
	case "$status" in
		loaded) ;;
		unknown) unknown "module-$name" ;;
		*) nogo "module-$name-$status" ;;
	esac
	emit "module.$name" "$status"
done

# --- platform device and modem_state -------------------------------------------

platform_readable=$(yes_no platform_readable)
cp_count=$(count_field cp_platform_count)
cp_driver=$(get cp_platform_driver)
state_file=$(get cp_modem_state_file)
state=$(get cp_modem_state)
case "$state_file" in present|unreadable|missing) ;; *) die 'probe reported a malformed modem_state file status' 5 ;; esac
if [ "$platform_readable" != yes ]; then platform=unknown
elif [ "$cp_count" -eq 0 ]; then platform=missing
elif [ "$cp_count" -gt 1 ]; then platform=ambiguous
elif [ -n "$cp_driver" ]; then platform=bound
else platform=unbound
fi
case "$platform" in
	bound) ;;
	unknown) unknown platform ;;
	*) nogo "cp-platform-$platform" ;;
esac
# State strings exported by the pinned driver (ferrix_s5300_cpif::ModemState).
if [ "$platform" != bound ] && [ "$platform" != unbound ]; then
	state_status=not-applicable
	[ -z "$state" ] && [ "$state_file" = missing ] || die 'probe reported modem_state without one platform device' 5
else
	case "$state_file:$state" in
		present:OFFLINE) state_status=offline ;;
		present:ONLINE) state_status=online ;;
		present:BOOTING|present:LOADER_DONE|present:NV_REBUILDING|present:SIM_ATTACH|present:SIM_DETACH|present:RESET) state_status=active ;;
		present:CRASH_RESET|present:CRASH_EXIT|present:WDT_RESET) state_status=crashed ;;
		present:) die 'probe reported an empty modem_state' 5 ;;
		present:*) state_status=unrecognised ;;
		unreadable:) state_status=unreadable ;;
		missing:) state_status=missing ;;
		*) die 'probe reported a malformed modem_state' 5 ;;
	esac
	case "$state_status" in
		offline) ;;
		unreadable) unknown modem-state ;;
		*) nogo "modem-state-$state_status" ;;
	esac
fi
emit platform_readable "$platform_readable"
emit cp_platform_count "$cp_count"
emit cp_platform "$platform"
emit cp_platform_driver "${cp_driver:-none}"
emit cp_modem_state "${state:-none}"
emit cp_modem_state_status "$state_status"

# --- CPIF character nodes -------------------------------------------------------

# Empty, or exactly MAJOR:MINOR in decimal.
majmin() {
	case "$1" in '') return 0 ;; *:*:*|*[!0-9:]*|:*|*:) return 1 ;; *:*) return 0 ;; *) return 1 ;; esac
}
stat_impl=$(get stat_impl)
case "$stat_impl" in present|missing) ;; *) die 'probe reported a malformed stat availability' 5 ;; esac
emit stat_impl "$stat_impl"
seen_classes=''
for node in $nodes; do
	kind=$(get "node.$node.kind")
	link=$(yes_no "node.$node.link")
	rdev=$(get "node.$node.rdev")
	mode=$(get "node.$node.mode")
	owner=$(get "node.$node.owner")
	classes=$(get "node.$node.classes")
	sysfs_dev=$(get "node.$node.sysfs_dev")
	subsystem=$(get "node.$node.subsystem")
	case "$kind" in char|block|fifo|other|missing) ;; *) die "probe reported a malformed node kind for $node" 5 ;; esac
	majmin "$rdev" || die "probe reported a malformed device number for $node" 5
	majmin "$sysfs_dev" || die "probe reported a malformed sysfs dev for $node" 5
	[ "$kind" = char ] || [ -z "$rdev" ] || die "probe reported a device number for non-character $node" 5

	class_count=$(printf '%s\n' "$classes" | awk '{ print NF }')
	if [ "$kind" = missing ] && [ "$link" = no ]; then status=missing
	elif [ "$link" = yes ]; then status=symlink
	elif [ "$kind" != char ]; then status=not-char
	elif [ "$class_count" -eq 0 ]; then status=no-sysfs-class
	elif [ "$class_count" -gt 1 ]; then status=ambiguous-class
	elif [ -z "$rdev" ] || [ -z "$sysfs_dev" ]; then status=dev-unknown
	elif [ "$rdev" != "$sysfs_dev" ]; then status=dev-mismatch
	elif [ "$subsystem" != "$classes" ]; then status=subsystem-mismatch
	else status=char-device
	fi
	case "$status" in
		char-device)
			case " $seen_classes " in *" $classes "*) ;; *) seen_classes="$seen_classes${seen_classes:+ }$classes" ;; esac
			;;
		dev-unknown) unknown "$node-dev" ;;
		*) nogo "$node-$status" ;;
	esac
	emit "node.$node" "$status"
	emit "node.$node.class" "${classes:-none}"
	emit "node.$node.subsystem" "${subsystem:-none}"
	emit "node.$node.rdev" "${rdev:-none}"
	emit "node.$node.sysfs_dev" "${sysfs_dev:-none}"
	emit "node.$node.mode" "${mode:-unknown}"
	emit "node.$node.owner" "${owner:-unknown}"
done
case "$seen_classes" in
	'') node_class=none ;;
	*' '*) node_class=mixed; nogo node-class-mixed ;;
	*) node_class=$seen_classes ;;
esac
case "$node_class" in
	none|mixed) class_claim=unknown ;;
	"$source_class") class_claim=yes ;;
	*) class_claim=no ;;
esac
emit node_class "$node_class"
emit node_class_matches_source_claim "$class_claim"

oem_total=$(count_field oem_ipc_name_count)
wwan_devices=$(count_field wwan_devices)
rmnet_count=$(count_field rmnet_count)
emit oem_ipc_name_count "$oem_total"
# A radio-first s5300_modem WWAN surface means this is not the Kleaf CPIF boot.
[ "$wwan_devices" -eq 0 ] || nogo wwan-surface-present
emit wwan_devices "$wwan_devices"
# Recorded only: rmnet absence before CP boot does not reject route A.
emit rmnet_count_before_cp_boot "$rmnet_count"

emit no_go "${no_go:-none}"
emit unknown_facts "${missing:-none}"
if [ -n "$no_go" ]; then e1=no-go
elif [ -n "$missing" ]; then e1=incomplete
else e1=go
fi
emit e1_result "$e1"
if [ "$evidence" = mock ]; then gate=not-evidence-mock; else gate=$e1; fi
emit e1_gate "$gate"
emit e1_unknowns "$unknowns"
emit capability_change none
emit result inventory-only
{
	printf '## raw probe output\n'
	cat "$raw"
} >>"$transcript"
cp "$transcript" "$out/inventory-latest.txt"
printf 'transcript=%s\n' "$transcript"
printf 'E1 is a pre-boot observation only; CP boot and FMT/RFS runtime behaviour stay unknown.\n'
