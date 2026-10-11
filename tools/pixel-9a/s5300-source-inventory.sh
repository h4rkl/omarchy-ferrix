#!/bin/sh
set -eu

# Read-only filesystem inventory of the four S5300 modem source partitions
# (active-slot modem, efs, persist, modem_userdata) on an ALREADY booted
# Google Pixel 9a (tegu / GXQ96 / MP1.0) Ferrix guest reachable over the pmOS
# USB network with SSH (issue #34). The guest probe
# (s5300-source-inventory-remote.sh) checks the exact device tree, then
# resolves each by-partlabel link and kernel PARTNAME uevent, reads the
# superblock type with util-linux `blkid -p`, and copies matching
# /proc/self/mountinfo lines. Nothing is mounted, written, or dumped.
#
# The host compares those facts with the contract of
# pmos/device-google-tegu/ferrix-s5300-mounts.initd. It never reports the
# issue #34 safety gate as passed: a clean physical run is
# `owner-review-required`, and whether ext4 `noload` / f2fs `norecovery`
# really suppress every write on this kernel stays unknown until observed.
#
# Uses the same SSH settings as radios.sh. Set
# FERRIX_PIXEL9A_S5300_INVENTORY_EVIDENCE=mock for stubbed runs; the default
# label is the exact physical target. Transcripts go to ignored
# out/pixel-9a/s5300-source-inventory/ and contain no serials, UUIDs, labels,
# or partition bytes.

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
target_id=google-pixel-9a-tegu-gxq96-mp1-0
pmos_cfg="$root/targets/boards/$target_id/pmos/pmbootstrap.cfg"
probe="$root/tools/pixel-9a/s5300-source-inventory-remote.sh"
out="${FERRIX_OUT_DIR:-$root/out}/pixel-9a/s5300-source-inventory"
mount_root=/mnt/vendor
roles='modem_img efs persist modem_userdata'
dt_model='ZUMA PRO TEGU MP on ZUMA PRO'
dt_compatible='google,ZUMA PRO TEGU'

ssh_host=${FERRIX_PIXEL9A_SSH_HOST:-172.16.42.1}
ssh_port=${FERRIX_PIXEL9A_SSH_PORT:-22}
ssh_opts=${FERRIX_PIXEL9A_SSH_OPTS:-}
if [ -z "${FERRIX_PIXEL9A_SSH_USER:-}" ]; then
	ssh_user=$(sed -n 's/^user = *//p' "$pmos_cfg" 2>/dev/null | head -n 1)
	ssh_user=${ssh_user:-user}
else
	ssh_user=$FERRIX_PIXEL9A_SSH_USER
fi

usage() {
	printf 'usage: tools/pixel-9a/s5300-source-inventory.sh\n' >&2
	printf 'environment: FERRIX_PIXEL9A_SSH_HOST FERRIX_PIXEL9A_SSH_USER FERRIX_PIXEL9A_SSH_PORT\n' >&2
	printf '             FERRIX_PIXEL9A_SSH_OPTS FERRIX_PIXEL9A_S5300_INVENTORY_EVIDENCE (physical|mock) FERRIX_OUT_DIR\n' >&2
	exit 2
}

[ "$#" -eq 0 ] || usage
case "${FERRIX_PIXEL9A_S5300_INVENTORY_EVIDENCE:-physical}" in
	physical) evidence="physical:$target_id" ;;
	mock) evidence=mock ;;
	*) usage ;;
esac

transcript=''
die() {
	code=${2:-1}
	[ -z "$transcript" ] || printf 'result=refused\nreason=%s\n' "$1" >>"$transcript"
	printf 'REFUSING pixel-9a S5300 source inventory: %s\n' "$1" >&2
	printf '{"event":"pixel-9a-s5300-source-inventory","evidence":"%s","status":"failed","reason":"%s","exit":%s}\n' \
		"$evidence" "$(printf '%s' "$1" | tr '"\\' "''")" "$code" >&2
	exit "$code"
}

for tool in ssh awk sed; do
	command -v "$tool" >/dev/null 2>&1 || die "missing host tool '$tool'; install it explicitly"
done
[ -f "$probe" ] && [ ! -L "$probe" ] || die 'guest probe script is missing or linked'

mkdir -p "$out"
stamp=$(date -u +%Y%m%dT%H%M%SZ)
transcript="$out/inventory-$stamp-$$.txt"
raw=$(mktemp "${TMPDIR:-/tmp}/pixel-9a-s5300-inventory.XXXXXX")
trap 'rm -f "$raw"' EXIT HUP INT TERM
{
	printf '# Pixel 9a S5300 source partition read-only inventory, %s UTC\n' "$stamp"
	printf '# evidence: %s\n' "$evidence"
	printf '# No serials, filesystem UUIDs/labels, or partition contents are collected. Never commit this file.\n'
	printf 'target_id=%s\nevidence=%s\nsource=ssh-read-only\n' "$target_id" "$evidence"
} >"$transcript"

# The only SSH invocation. The probe is fed on stdin to a remote `sh -s`.
# shellcheck disable=SC2086 # FERRIX_PIXEL9A_SSH_OPTS is intentionally word-split.
ssh_status=0
ssh -o BatchMode=yes -o ConnectTimeout=10 -o StrictHostKeyChecking=accept-new -p "$ssh_port" $ssh_opts \
	"$ssh_user@$ssh_host" 'sh -s' <"$probe" >"$raw" 2>/dev/null || ssh_status=$?
case "$ssh_status" in
	0) ;;
	255) die 'guest unreachable over SSH' 3 ;;
	4) die 'guest is not the exact tegu board (device-tree gate refused)' 4 ;;
	*) die "guest probe failed (exit $ssh_status)" 3 ;;
esac

# --- validate the probe output ----------------------------------------------

[ "$(wc -c <"$raw" | tr -d ' ')" -le 65536 ] || die 'probe output exceeds the reviewed size limit' 5
awk -v roles="$roles" '
	BEGIN {
		split("probe dt_model dt_compatible kernel_release uid privilege blkid_impl slot_source slot_value_count slot_suffix mountinfo_readable probe_complete", g, " ")
		for (i in g) required[g[i]] = 1
		split(roles, r, " ")
		split("label link_state link_device partname_count partname_devices dev blkid_exit blkid_type", f, " ")
		for (i in r) { role[r[i]] = 1; for (j in f) required["part." r[i] "." f[j]] = 1 }
	}
	{
		eq = index($0, "=")
		if (eq < 2) { bad = 1; next }
		key = substr($0, 1, eq - 1)
		if (key ~ /^part\.[a-z_]+\.mount$/) {
			name = key; sub(/^part\./, "", name); sub(/\.mount$/, "", name)
			if (!(name in role)) bad = 1
			next
		}
		if (!(key in required) || (key in seen)) bad = 1
		if (substr($0, eq + 1) !~ /^[A-Za-z0-9 ._:,+~-]*$/) bad = 1
		seen[key] = 1
	}
	END { if (bad) exit 1; for (k in required) if (!(k in seen)) exit 1 }
' "$raw" || die 'probe output has a malformed, unknown, duplicate, or missing field' 5

get() {
	awk -v key="$1=" 'index($0, key) == 1 { print substr($0, length(key) + 1); exit }' "$raw"
}

[ "$(get probe)" = s5300-source-inventory ] && [ "$(get probe_complete)" = yes ] ||
	die 'probe did not complete' 5
[ "$(get dt_model)" = "$dt_model" ] && [ "$(get dt_compatible)" = "$dt_compatible" ] ||
	die 'probe reported a device tree other than the exact tegu board' 4

slot=$(get slot_suffix)
case "$slot" in
	_a|_b) slot_status=resolved ;;
	unresolved|unsupported) slot_status=$slot ;;
	*) die 'probe reported a malformed slot suffix' 5 ;;
esac
mountinfo_readable=$(get mountinfo_readable)
case "$mountinfo_readable" in yes|no) ;; *) die 'probe reported malformed mountinfo readability' 5 ;; esac

emit() {
	printf '%s=%s\n' "$1" "$2" >>"$transcript"
	printf '%s=%s\n' "$1" "$2"
}

emit kernel_release "$(get kernel_release)"
emit guest_privilege "$(get privilege)"
emit blkid_impl "$(get blkid_impl)"
emit slot_source "$(get slot_source)"
emit slot_value_count "$(get slot_value_count)"
emit slot_suffix "$slot"
emit slot_status "$slot_status"
emit mountinfo_readable "$mountinfo_readable"

# --- compare each source with the mount service's contract -------------------

# Prints "status|recovery-flag-observed" for every mountinfo line that the
# probe recorded for $1. A line is accepted only with the properties
# ferrix-s5300-mounts.initd verify_mount() requires.
mount_eval() {
	awk -v key="part.$1.mount=" -v dest="$mount_root/$1" -v dev="$2" -v fstype="$3" '
		function has(list, option) { return index("," list ",", "," option ",") > 0 }
		function add(reason) { if (index("," reasons ",", "," reason ",") == 0) reasons = reasons (reasons == "" ? "" : ",") reason }
		index($0, key) == 1 {
			count++
			nf = split(substr($0, length(key) + 1), f, " ")
			sep = 0
			for (i = 7; i <= nf; i++) if (f[i] == "-") { sep = i; break }
			if (!sep || sep + 3 > nf || f[3] !~ /^[0-9]+:[0-9]+$/) { malformed = 1; next }
			if (dev == "" || f[3] != dev) { add("foreign-mount-at-destination"); next }
			if (f[5] != dest) add("mounted-outside-destination")
			if (f[4] != "/") add("not-whole-filesystem")
			if (fstype != "ext4" && fstype != "f2fs") add("superblock-type-unknown")
			else if (f[sep + 1] != fstype) add("type-mismatch")
			if (!has(f[6], "ro")) add("mount-not-ro")
			if (!has(f[sep + 3], "ro")) add("superblock-not-ro")
			super = f[sep + 3]
			if ((fstype == "ext4" && (has(super, "noload") || has(super, "norecovery"))) ||
			    (fstype == "f2fs" && has(super, "norecovery"))) flag = "observed"
			else add("recovery-not-suppressed")
		}
		END {
			if (!count) status = "not-mounted"
			else if (malformed) status = "malformed-mountinfo"
			else if (reasons != "") status = "contract-violated:" reasons
			else status = "contract-met"
			print status "|" (flag == "" ? "unknown" : flag)
		}
	' "$raw"
}

problems=''
problem() { problems="$problems${problems:+,}$1"; }
[ "$slot_status" = resolved ] || problem "slot-$slot_status"
[ "$mountinfo_readable" = yes ] || problem mountinfo-unreadable
ext4_flag=unknown
f2fs_flag=unknown

for role in $roles; do
	label=$(get "part.$role.label")
	expected=$role
	[ "$role" != modem_img ] || expected=unresolved
	[ "$role" != modem_img ] || [ "$slot_status" != resolved ] || expected="modem$slot"
	[ "$label" = "$expected" ] || die "probe reported label '$label' for $role (expected $expected)" 5
	link_state=$(get "part.$role.link_state")
	link_device=$(get "part.$role.link_device")
	count=$(get "part.$role.partname_count")
	devices=$(get "part.$role.partname_devices")
	dev=$(get "part.$role.dev")
	blkid_exit=$(get "part.$role.blkid_exit")
	blkid_type=$(get "part.$role.blkid_type")
	case "$count" in ''|*[!0-9]*) die "probe reported a malformed PARTNAME count for $role" 5 ;; esac
	case "$link_state" in missing|block|not-block) ;; *) die "probe reported a malformed link state for $role" 5 ;; esac

	if [ "$label" = unresolved ]; then resolution=slot-unresolved
	elif [ "$link_state" = missing ]; then resolution=missing-partlabel
	elif [ "$count" -eq 0 ]; then resolution=missing-kernel-partname
	elif [ "$count" -gt 1 ]; then resolution=duplicate-partname
	elif [ "$link_state" != block ]; then resolution=not-block-device
	elif [ "$devices" != "$link_device" ]; then resolution=device-mismatch
	else resolution=unique
	fi

	fstype=unknown
	case "$resolution:$blkid_exit:$blkid_type" in
		unique:0:ext4|unique:0:f2fs) fstype=$blkid_type; type_status=$blkid_type ;;
		unique:0:*) type_status=unsupported ;;
		unique:2:*) type_status=no-signature ;;
		unique:8:*) type_status=ambivalent ;;
		unique:unavailable-*) type_status=unknown-${blkid_exit#unavailable-}-unavailable ;;
		unique:not-run:*) die "probe skipped blkid on a unique $role partition" 5 ;;
		unique:*) type_status=probe-error ;;
		*) type_status=not-probed ;;
	esac

	if [ "$mountinfo_readable" = yes ]; then
		evaluation=$(mount_eval "$role" "$dev" "$fstype")
		mount_status=${evaluation%|*}
		flag=${evaluation#*|}
	else
		mount_status=unknown
		flag=unknown
	fi
	[ "$flag" != observed ] || [ "$fstype" != ext4 ] || ext4_flag=observed
	[ "$flag" != observed ] || [ "$fstype" != f2fs ] || f2fs_flag=observed

	service_accepts=no
	if [ "$resolution" = unique ] && [ "$fstype" != unknown ]; then
		case "$mount_status" in not-mounted|contract-met) service_accepts=yes ;; esac
	fi
	[ "$resolution" = unique ] || problem "$role-$resolution"
	[ "$fstype" != unknown ] || [ "$resolution" != unique ] || problem "$role-type-$type_status"
	case "$mount_status" in not-mounted|contract-met|unknown) ;; *) problem "$role-mount-${mount_status%%:*}" ;; esac

	emit "$role.label" "$label"
	emit "$role.partlabel_link" "${link_device:-none}"
	emit "$role.link_state" "$link_state"
	emit "$role.kernel_partname_count" "$count"
	emit "$role.kernel_partname_devices" "${devices:-none}"
	emit "$role.resolution" "$resolution"
	emit "$role.blkid_exit" "$blkid_exit"
	emit "$role.raw_type" "${blkid_type:-none}"
	emit "$role.type_status" "$type_status"
	emit "$role.mount_status" "$mount_status"
	emit "$role.service_would_accept" "$service_accepts"
done

emit ext4_recovery_flag_in_mountinfo "$ext4_flag"
emit f2fs_recovery_flag_in_mountinfo "$f2fs_flag"
emit journal_replay_suppression_on_device unknown
emit problems "${problems:-none}"
if [ "$evidence" = mock ]; then
	gate=not-evaluated-mock
elif [ -n "$problems" ]; then
	gate=blocked
else
	gate=owner-review-required
fi
emit issue34_safety_gate "$gate"
emit result inventory-only
{
	printf '## raw probe output\n'
	# mountinfo's source field may use a device-unique by-id or by-UUID
	# symlink. The evaluator already used the raw line; omit that field from
	# the retained transcript while keeping device major:minor and flags.
	awk '
		/^part\.[a-z_]+\.mount=/ {
			for (i = 7; i + 2 <= NF; i++) {
				if ($i == "-") { $(i + 2) = "<redacted-source>"; break }
			}
		}
		{ print }
	' "$raw"
} >>"$transcript"
cp "$transcript" "$out/inventory-latest.txt"
printf 'transcript=%s\n' "$transcript"
printf 'An inventory never passes the #34 gate; recovery suppression on the device stays unknown until reviewed.\n'
