# Guest body of the issue #92 S2 preflight. s5300-s2-preflight.sh sends it
# over the CDC-ACM shell (acm-probe.py, piped to the guest `sh`) as one file
# composed of the checked-in, pin-verified ferrix-s5300-cpif.sh library
# followed by this body. The library is the issue #92 alias-aware opener
# scan, so the scan below is the reviewed one even while the installed image
# still carries an older library. Whether the INSTALLED library matches is a
# separate fact that the host reports as image skew.
#
# Read-only. It never opens a CPIF node: /dev is reached only through the
# library's file tests and stat(2), and open files only through stat(2) on
# /proc/<pid>/task/<tid>/fd links. It writes no file, starts, stops, mounts,
# unmounts, attaches or provisions nothing, and never writes or consumes the
# arm token. The only programs it runs are the library's own probes:
# sha256sum on installed package files; the installed exynos-modem with
# --print-config (only when the pinned release is installed; it resolves the
# CLI and exits before any node is opened); and, only when
# ferrix-s5300-mounts is already started by someone else, the accepted
# service's own verify_own_mount, which reads sysfs, mountinfo and
# util-linux `blkid -p` superblock types and checks per-unit inputs by
# stat(2) metadata only.
#
# It reads no kernel log, IMEI, SIM, partition contents, file names under
# /mnt/vendor, or process arguments. It reads one per-unit identity value and
# the boot ID only to hash them with the host's session salt (issue #108);
# neither raw value is printed. Everything printed is an enum, count,
# version, package-file SHA-256, salted 16-hex identity prefix, or a
# sanitised kernel/process name.

[ "${cpif_target:-}" = google-pixel-9a-tegu-gxq96-mp1-0 ] || exit 5

# Defence in depth: the library's state-changing helpers are never called
# here, and are replaced so that no future edit can call them by accident.
cpif_write_arm() { return 1; }
cpif_consume_arm() { return 1; }
cpif_provision_scratch() { return 1; }
checkpath() { return 1; }
# The library reports refusals through OpenRC's eerror; the host classifies
# from the enumerated facts below, so the messages (which carry paths and
# pids) are dropped.
eerror() { :; }

s2_model=$(tr '\000' '\n' <"$cpif_proc/device-tree/model" 2>/dev/null | sed -n '1p')
[ "$s2_model" = "$cpif_dt_model" ] || exit 4
tr '\000' '\n' <"$cpif_proc/device-tree/compatible" 2>/dev/null | grep -Fxq "$cpif_dt_compatible" || exit 4

s2_clean() { tr -cd 'A-Za-z0-9._+~-' | cut -c1-48; }
s2_lines() { awk 'NF { n++ } END { print n + 0 }'; }
s2_entries() {
	n=0
	for entry; do
		[ -e "$entry" ] || [ -L "$entry" ] || continue
		n=$((n + 1))
	done
	printf '%s\n' "$n"
}
s2_result() { if "$@"; then echo pass; else echo fail; fi; }
s2_join() { awk 'NF { printf "%s%s", (n++ ? "," : ""), $0 } END { if (n) print "" }'; }

printf 'probe=s5300-s2-preflight\n'
printf 'dt_model=%s\ndt_compatible=%s\n' "$cpif_dt_model" "$cpif_dt_compatible"
printf 'kernel_release=%s\n' "$(uname -r | s2_clean)"
printf 'uid=%s\n' "$(id -u | tr -cd 0-9)"

# --- identity: exact key lookups only; no other bootconfig/cmdline value -----

s2_bootvalue() {
	sed -n "s/^[[:space:]]*androidboot\\.$1[[:space:]]*=[[:space:]]*\"*\\([^\"]*\\)\"*[[:space:]]*\$/\\1/p" \
		"$cpif_proc/bootconfig" 2>/dev/null
}
s2_revision=$(s2_bootvalue revision)
case "$(printf '%s\n' "$s2_revision" | s2_lines)" in
	0) s2_revision=absent ;;
	1) s2_revision=$(printf '%s' "$s2_revision" | s2_clean) ;;
	*) s2_revision=ambiguous ;;
esac
printf 'hw_revision=%s\n' "${s2_revision:-absent}"

# Same order as ferrix-s5300-mounts resolve_modem_label: bootconfig, then
# the command line.
s2_slot_source=none
s2_slots=$(s2_bootvalue slot_suffix)
if [ -n "$s2_slots" ]; then
	s2_slot_source=bootconfig
else
	s2_slots=$(awk '{ for (i = 1; i <= NF; i++) if ($i ~ /^androidboot\.slot_suffix=/) { v = $i; sub(/^androidboot\.slot_suffix=/, "", v); print v } }' \
		"$cpif_proc/cmdline" 2>/dev/null)
	[ -z "$s2_slots" ] || s2_slot_source=cmdline
fi
s2_slot_count=$(printf '%s\n' "$s2_slots" | s2_lines)
s2_slot=none
if [ "$s2_slot_count" -eq 1 ]; then
	case "$s2_slots" in _a|_b) s2_slot=$s2_slots ;; *) s2_slot=other ;; esac
fi
printf 'slot_source=%s\nslot_value_count=%s\nslot_suffix=%s\n' "$s2_slot_source" "$s2_slot_count" "$s2_slot"

# --- redacted unit and boot identity (issue #108) -------------------------------
#
# The host prepends exactly one session line, s2_salt=<64 hex>, before the
# library. A raw identity value never leaves this shell: it is read into a
# variable, fed to sha256sum on stdin by the printf builtin (never as an
# argument), and only the 16-hex salted prefix is printed. The candidates
# are tried in a fixed order and the first one present wins; a present but
# repeated or malformed value stops the search (fail closed, no fallback).
# Which candidate exists on tegu is unconfirmed until a physical run
# (s5300-s2-preflight.md).
s2_salted=no
case "${s2_salt:-}" in
	''|*[!0-9a-f]*) ;;
	*) [ "${#s2_salt}" -ne 64 ] || s2_salted=yes ;;
esac
s2_hasher=no
case "$(command -v sha256sum 2>/dev/null)" in '') ;; *) s2_hasher=yes ;; esac
s2_digest() {
	printf 'ferrix-s5300-s2-%s-v1\n%s\n%s\n' "$1" "$s2_salt" "$2" | sha256sum 2>/dev/null | cut -c1-16 | tr -cd 0-9a-f
}
s2_unit_source=none
s2_unit_state=absent
s2_unit_value=
s2_unit_try() {
	[ "$s2_unit_state" = absent ] || return 0
	case "$(printf '%s\n' "$2" | s2_lines)" in
		0) return 0 ;;
		1) s2_unit_source=$1
			s2_unit_state=malformed
			case "$2" in *[!A-Za-z0-9._:-]*) return 0 ;; esac
			[ "${#2}" -ge 4 ] && [ "${#2}" -le 64 ] || return 0
			s2_unit_state=ok
			s2_unit_value=$2
			;;
		*) s2_unit_source=$1; s2_unit_state=ambiguous ;;
	esac
}
s2_unit_try bootconfig-serialno "$(s2_bootvalue serialno)"
s2_unit_try cmdline-serialno "$(awk '{ for (i = 1; i <= NF; i++) if ($i ~ /^androidboot\.serialno=/) { v = $i; sub(/^androidboot\.serialno=/, "", v); print v } }' \
	"$cpif_proc/cmdline" 2>/dev/null)"
s2_unit_try dt-serial-number "$(tr '\000' '\n' <"$cpif_proc/device-tree/serial-number" 2>/dev/null | awk 'NF')"
s2_unit_try chipid-unique-id "$(head -c 128 "$cpif_sys/devices/system/chip-id/unique_id" 2>/dev/null | awk 'NF')"
s2_unit_hash=unavailable
if [ "$s2_unit_state" = ok ]; then
	if [ "$s2_salted" != yes ]; then s2_unit_state=no-salt
	elif [ "$s2_hasher" != yes ]; then s2_unit_state=no-hasher
	else
		s2_unit_hash=$(s2_digest unit "$s2_unit_source:$s2_unit_value")
		[ "${#s2_unit_hash}" -eq 16 ] || { s2_unit_hash=unavailable; s2_unit_state=no-hasher; }
	fi
fi
s2_boot_value=$(head -c 64 "$cpif_proc/sys/kernel/random/boot_id" 2>/dev/null | tr -d '\n')
s2_boot_hash=unavailable
if [ -z "$s2_boot_value" ]; then s2_boot_state=absent
else
	s2_boot_state=malformed
	case "$s2_boot_value" in
		*[!0-9a-f-]*) ;;
		*) [ "${#s2_boot_value}" -ne 36 ] || s2_boot_state=ok ;;
	esac
fi
if [ "$s2_boot_state" = ok ]; then
	if [ "$s2_salted" != yes ]; then s2_boot_state=no-salt
	elif [ "$s2_hasher" != yes ]; then s2_boot_state=no-hasher
	else
		s2_boot_hash=$(s2_digest boot "$s2_boot_value")
		[ "${#s2_boot_hash}" -eq 16 ] || { s2_boot_hash=unavailable; s2_boot_state=no-hasher; }
	fi
fi
unset s2_salt s2_unit_value s2_boot_value
printf 'unit_id_source=%s\nunit_id_state=%s\nunit_id_hash=%s\n' "$s2_unit_source" "$s2_unit_state" "$s2_unit_hash"
printf 'boot_id_state=%s\nboot_id_hash=%s\n' "$s2_boot_state" "$s2_boot_hash"

case "$(readlink "$cpif_proc/self/ns/pid" 2>/dev/null)" in
	"$cpif_init_pidns") printf 'pidns=initial\n' ;;
	'') printf 'pidns=unreadable\n' ;;
	*) printf 'pidns=other\n' ;;
esac

# backends.env is parsed, never sourced. Only its two S5300 keys are used.
s2_env_file="$cpif_fs/etc/ferrix/backends.env"
s2_env() {
	awk -v k="$1" 'index($0, k "=") == 1 { n++; v = substr($0, length(k) + 2) }
		END { if (n == 0) print "\001missing"; else if (n > 1) print "\001duplicate"; else print v }' "$s2_env_file"
}
FERRIX_TARGET=
FERRIX_S5300_KERNEL_BACKEND=
if [ -L "$s2_env_file" ]; then
	printf 'env_file=symlink\nenv.target=missing\nenv.backend=missing\n'
elif [ ! -f "$s2_env_file" ]; then
	printf 'env_file=missing\nenv.target=missing\nenv.backend=missing\n'
else
	printf 'env_file=present\n'
	s2_value=$(s2_env FERRIX_TARGET)
	case "$s2_value" in
		"$cpif_target") FERRIX_TARGET=$s2_value; printf 'env.target=exact\n' ;;
		"$(printf '\001')missing") printf 'env.target=missing\n' ;;
		"$(printf '\001')duplicate") printf 'env.target=duplicate\n' ;;
		*) printf 'env.target=other\n' ;;
	esac
	s2_value=$(s2_env FERRIX_S5300_KERNEL_BACKEND)
	case "$s2_value" in
		google-kleaf-cpif|linux-google-tegu-wwan) FERRIX_S5300_KERNEL_BACKEND=$s2_value; printf 'env.backend=%s\n' "$s2_value" ;;
		"$(printf '\001')missing") printf 'env.backend=missing\n' ;;
		"$(printf '\001')duplicate") printf 'env.backend=duplicate\n' ;;
		'') printf 'env.backend=empty\n' ;;
		*) printf 'env.backend=other\n' ;;
	esac
fi
export FERRIX_TARGET FERRIX_S5300_KERNEL_BACKEND
printf 'check.target=%s\n' "$(s2_result cpif_check_target)"

# --- installed package files, pins, and versions -----------------------------

if [ -L "$cpif_pins" ]; then printf 'pins_file=symlink\n'
elif [ -f "$cpif_pins" ]; then printf 'pins_file=present\n'
else printf 'pins_file=missing\n'
fi
for s2_path in $cpif_pinned_paths; do
	s2_key=${s2_path##*/}
	s2_file="$cpif_fs$s2_path"
	if [ -L "$s2_file" ]; then s2_sha=symlink
	elif [ -f "$s2_file" ]; then
		s2_sha=$(cpif_sha256 "$s2_file")
		case "$s2_sha" in ''|*[!0-9a-f]*) s2_sha=unreadable ;; esac
	else s2_sha=missing
	fi
	s2_pin=$(awk -v p="$s2_path" '$2 == p { n++; h = $1 } END { if (n == 1) print h }' "$cpif_pins" 2>/dev/null)
	if [ -z "$s2_pin" ]; then s2_pinned=unpinned
	elif [ "$s2_pin" = "$s2_sha" ]; then s2_pinned=match
	else s2_pinned=mismatch
	fi
	printf 'file.%s.sha256=%s\nfile.%s.pin=%s\n' "$s2_key" "$s2_sha" "$s2_key" "$s2_pinned"
done
printf 'check.package=%s\n' "$(s2_result cpif_check_package)"

s2_apk_version() {
	[ -f "$cpif_apk_db" ] || { echo unreadable; return; }
	awk -v p="$1" '/^P:/ { name = substr($0, 3) } /^V:/ && name == p { v = substr($0, 3); n++ } /^$/ { name = "" }
		END { if (n == 1) print v; else if (n == 0) print "missing"; else print "duplicate" }' "$cpif_apk_db" 2>/dev/null | s2_clean
}
for s2_pkg in device-google-tegu exynos-modem modemmanager-samsung-sit; do
	printf 'pkg.%s=%s\n' "$s2_pkg" "$(s2_apk_version "$s2_pkg")"
done

# The exact CPIF CLI and --print-config text of exynos-modem.initd (the
# offline test checks both against the init script). The binary runs only
# when the apk database names the pinned patched release.
s2_modem_args='--model 5300 --kernel-backend google-kleaf-cpif --uecap-dir /vendor/firmware/uecapconfig --boot-dev /dev/umts_boot0 --rfs-dev /dev/umts_rfs0 --oem-gems-dev /dev/oem_ipc1'
s2_modem_config='kernel_backend=google-kleaf-cpif
boot_dev=/dev/umts_boot0
rfs_dev=/dev/umts_rfs0
oem_gems_dev=/dev/oem_ipc1
uecap_dir=/vendor/firmware/uecapconfig
scratch_dir=/run/exynos-modem
crash_dump_boot=disabled'
if [ "$(s2_apk_version exynos-modem)" = "$cpif_modem_version" ]; then
	# shellcheck disable=SC2086 # the CLI is split exactly like command_args
	printf 'check.cli=%s\n' "$(s2_result cpif_check_cli "$cpif_modem_bin" "$s2_modem_config" $s2_modem_args)"
else
	printf 'check.cli=skipped\n'
fi

# --- kernel surface ----------------------------------------------------------

for s2_module in $cpif_modules; do
	s2_state=$(awk -v m="$s2_module" '$1 == m { n++; s = $5 } END { if (n == 1) print s; else if (n == 0) print "absent"; else print "duplicate" }' \
		"$cpif_proc/modules" 2>/dev/null | s2_clean)
	printf 'module.%s=%s\n' "$s2_module" "${s2_state:-unreadable}"
done
s2_cp_count=0
s2_cp_entry=
for s2_entry in "$cpif_sys"/bus/platform/devices/*; do
	[ -f "$s2_entry/of_node/compatible" ] || continue
	tr '\000' '\n' <"$s2_entry/of_node/compatible" 2>/dev/null | grep -Fxq 'samsung,exynos-cp' || continue
	s2_cp_count=$((s2_cp_count + 1))
	s2_cp_entry=$s2_entry
done
s2_cp_driver=none
s2_modem_state=none
if [ "$s2_cp_count" -eq 1 ]; then
	s2_cp_driver=$(readlink "$s2_cp_entry/driver" 2>/dev/null | sed 's#/*$##; s#.*/##' | s2_clean)
	s2_modem_state=$(head -c 64 "$s2_cp_entry/modem_state" 2>/dev/null | tr -cd 'A-Za-z0-9_' | cut -c1-32)
fi
printf 'cp_platform_count=%s\ncp_driver=%s\nmodem_state=%s\n' "$s2_cp_count" "${s2_cp_driver:-none}" "${s2_modem_state:-unreadable}"
printf 'wwan_devices=%s\n' "$(s2_entries "$cpif_sys"/class/wwan/*)"
printf 'check.surface=%s\n' "$(s2_result cpif_check_surface OFFLINE)"

for s2_node in $cpif_nodes; do
	printf 'node.%s=%s\n' "$s2_node" "$(s2_result cpif_check_node "$s2_node")"
done

# --- the issue #92 opener scan over every cpif class entry -------------------

s2_class_nodes=$(cpif_class_nodes)
printf 'class_node_count=%s\n' "$(printf '%s\n' "$s2_class_nodes" | s2_lines)"
s2_extra=0
for s2_node in $s2_class_nodes; do
	case " $cpif_nodes " in *" $s2_node "*) ;; *) s2_extra=$((s2_extra + 1)) ;; esac
done
printf 'class_extra_nodes=%s\n' "$s2_extra"
# The library's scan carries its own positive control and the CPIF-major
# checks (issue #107); its facts are re-validated here before they leave.
s2_scan_out=
if [ -z "$s2_class_nodes" ]; then
	s2_scan=not-run
else
	s2_scan_out=$(cpif_opener_scan $s2_class_nodes 2>/dev/null) || :
	s2_scan=$(printf '%s\n' "$s2_scan_out" | sed -n 's/^fact opener_scan=//p' | sed -n '1p')
	case "$s2_scan" in clean|held) ;; *) s2_scan=incomplete ;; esac
fi
s2_holders=$(printf '%s\n' "$s2_scan_out" | sed -n 's/^holder //p')
s2_scan_fact() { printf '%s\n' "$s2_scan_out" | sed -n "s/^fact $1=//p" | sed -n '1p'; }
s2_control=$(s2_scan_fact opener_positive_control)
case "$s2_control" in detected) ;; *) s2_control=missing ;; esac
s2_format=$(s2_scan_fact opener_format_control)
case "$s2_format" in matched|mismatch) ;; *) s2_format=missing ;; esac
s2_foreign=$(s2_scan_fact opener_major_foreign)
case "$s2_foreign" in ''|*[!0-9]*) s2_foreign=unknown ;; esac
s2_vanished=$(s2_scan_fact opener_vanished)
case "$s2_vanished" in ''|*[!0-9]*) s2_vanished=0 ;; esac
printf 'opener_scan=%s\n' "$s2_scan"
printf 'opener_positive_control=%s\nopener_format_control=%s\n' "$s2_control" "$s2_format"
printf 'opener_major_foreign=%s\nopener_vanished=%s\n' "$(printf '%s' "$s2_foreign" | cut -c1-6)" "$(printf '%s' "$s2_vanished" | cut -c1-6)"
s2_pids=$(printf '%s\n' "$s2_holders" | awk 'NF { print $1 }' | sort -u)
s2_held=$(printf '%s\n' "$s2_holders" | awk -v known="$cpif_nodes" '
	BEGIN { split(known, names, " "); for (i in names) allowed[names[i]] = 1 }
	NF { if ($2 in allowed) print $2; else print "other" }
' | sort -u | head -n 16 | s2_join)
printf 'opener_holder_count=%s\n' "$(printf '%s\n' "$s2_pids" | s2_lines)"
printf 'opener_held_nodes=%s\n' "${s2_held:-none}"
s2_comms=
for s2_pid in $s2_pids; do
	case "$s2_pid" in *[!0-9]*) continue ;; esac
	# A process may choose its own comm, so never send arbitrary task names
	# across the transport or retain them in the transcript.
	s2_comm=$(head -c 32 "$cpif_proc/$s2_pid/comm" 2>/dev/null)
	case "$s2_comm" in
		exynos-modem|sit-proxy|ModemManager) ;;
		*) s2_comm=other ;;
	esac
	s2_comms="$s2_comms${s2_comm:-gone}
"
done
s2_comms=$(printf '%s' "$s2_comms" | sort -u | head -n 8 | s2_join)
printf 'opener_holder_comms=%s\n' "${s2_comms:-none}"

# Live S5300 daemons, by kernel task name only.
s2_daemons=0
s2_mm=0
for s2_comm_file in "$cpif_proc"/[0-9]*/comm; do
	case "$(head -c 32 "$s2_comm_file" 2>/dev/null)" in
		exynos-modem|sit-proxy) s2_daemons=$((s2_daemons + 1)) ;;
		ModemManager) s2_mm=$((s2_mm + 1)) ;;
	esac
done
printf 'proc.s5300_daemons=%s\nproc.modemmanager=%s\n' "$s2_daemons" "$s2_mm"

# --- OpenRC state, runlevels and overrides ------------------------------------

s2_rc="$cpif_run/openrc"
if [ -d "$s2_rc/started" ]; then printf 'openrc_state_dir=present\n'; else printf 'openrc_state_dir=missing\n'; fi
# OpenRC keeps one directory per state flag; a service in none is stopped.
s2_service_state() {
	[ -d "$s2_rc/started" ] || { echo unknown; return; }
	states=
	for st in started starting stopping inactive wasinactive failed; do
		[ -e "$s2_rc/$st/$1" ] || [ -L "$s2_rc/$st/$1" ] || continue
		[ -z "$states" ] || states="$states+"
		states="$states$st"
	done
	echo "${states:-stopped}"
}
service_started() { [ "$(s2_service_state "$1")" = started ]; }
for s2_svc in ferrix-s5300-mounts exynos-modem sit-proxy modemmanager ferrix-radio-firmware; do
	printf 'service.%s=%s\n' "$s2_svc" "$(s2_service_state "$s2_svc")"
done
for s2_svc in exynos-modem sit-proxy; do
	printf 'rc_daemon.%s=%s\n' "$s2_svc" "$(s2_entries "$s2_rc/daemons/$s2_svc" "$s2_rc/daemons/$s2_svc"/*)"
done
for s2_svc in ferrix-s5300-mounts exynos-modem sit-proxy; do
	printf 'runlevel.%s=%s\n' "$s2_svc" "$(s2_entries "$cpif_fs"/etc/runlevels/*/"$s2_svc" "$cpif_fs"/etc/runlevels/*/"$s2_svc".*)"
done
printf 'init_instances=%s\n' "$(s2_entries "$cpif_fs"/etc/init.d/ferrix-s5300-mounts.* "$cpif_fs"/etc/init.d/exynos-modem.* \
	"$cpif_fs"/etc/init.d/sit-proxy.* "$cpif_fs"/etc/init.d/modemmanager.*)"
# conf.d files are sourced by openrc-run before the service and can replace
# command_args, dependencies or supervision. The only expected line is
# postmarketOS's modemmanager_opts, which modemmanager.initd filters.
s2_confd() {
	file="$cpif_fs/etc/conf.d/$1"
	if [ -L "$file" ]; then echo symlink; return; fi
	[ -e "$file" ] || { echo absent; return; }
	[ -f "$file" ] || { echo other; return; }
	lines=$(grep -v '^[[:space:]]*\(#.*\)\{0,1\}$' "$file" 2>/dev/null)
	[ -n "$lines" ] || { echo empty; return; }
	if [ "$1" = modemmanager ] && ! printf '%s\n' "$lines" | grep -qv '^modemmanager_opts='; then
		echo expected
	else
		echo override
	fi
}
for s2_svc in ferrix-s5300-mounts exynos-modem sit-proxy modemmanager; do
	printf 'confd.%s=%s\n' "$s2_svc" "$(s2_confd "$s2_svc")"
done
s2_pattern='exynos[-_]modem|sit[-_]proxy|s5300|modemmanager|cpif|umts_|oem_ipc'
s2_mentions() {
	n=0
	for file; do
		[ -f "$file" ] || continue
		grep -v '^[[:space:]]*#' "$file" 2>/dev/null | grep -Eiq "$s2_pattern" && n=$((n + 1))
	done
	printf '%s\n' "$n"
}
printf 'rc_conf_mentions=%s\n' "$(s2_mentions "$cpif_fs/etc/rc.conf" "$cpif_fs"/etc/rc.conf.d/*)"
printf 'confd_instances=%s\n' "$(s2_entries "$cpif_fs"/etc/conf.d/ferrix-s5300-mounts.* "$cpif_fs"/etc/conf.d/exynos-modem.* \
	"$cpif_fs"/etc/conf.d/sit-proxy.*)"
printf 'local_d_mentions=%s\n' "$(s2_mentions "$cpif_fs"/etc/local.d/*)"
printf 'udev_overrides=%s\n' "$(s2_mentions "$cpif_fs"/etc/udev/rules.d/*)"

# --- manual-start state: token, pidfiles, /run, scratch, firmware --------------

if [ -L "$cpif_arm_token" ]; then printf 'arm_token=symlink\n'
elif [ -e "$cpif_arm_token" ]; then printf 'arm_token=present\n'
else printf 'arm_token=absent\n'
fi
for s2_svc in exynos-modem sit-proxy; do
	printf 'pidfile.%s=%s\n' "$s2_svc" "$(s2_entries "$cpif_run/$s2_svc.pid")"
done
printf 'sit_socket_dir=%s\n' "$(s2_entries "$cpif_run/sit-proxy")"
printf 'check.run=%s\n' "$(s2_result cpif_check_run)"
if [ -L "$cpif_scratch" ]; then s2_scratch=symlink
elif [ ! -e "$cpif_scratch" ]; then s2_scratch=absent
elif [ ! -d "$cpif_scratch" ]; then s2_scratch=not-directory
elif [ "$(stat -c '%u %g %a' "$cpif_scratch" 2>/dev/null)" = "$cpif_uid $cpif_gid 700" ]; then s2_scratch=ok
else s2_scratch=wrong-owner-mode
fi
printf 'scratch=%s\n' "$s2_scratch"
s2_scratch_entries=0
if [ "$s2_scratch" = ok ]; then
	# A failed listing must never turn an unreadable scratch directory into
	# an apparent empty directory.
	s2_scratch_entries=unavailable
	if s2_listing=$(ls -A "$cpif_scratch" 2>/dev/null); then
		s2_scratch_entries=$(printf '%s\n' "$s2_listing" | s2_lines)
	fi
fi
printf 'scratch_entries=%s\n' "$s2_scratch_entries"
for s2_dir in uecapconfig carrierconfig; do
	if [ -d "$cpif_fs/vendor/firmware/$s2_dir" ]; then s2_fw=present; else s2_fw=missing; fi
	printf 'fw.%s=%s\n' "$s2_dir" "$s2_fw"
done

# --- source mounts: partial or stale state, read from mountinfo and sysfs ------

s2_mountinfo="$cpif_proc/self/mountinfo"
printf 'vendor_mounts=%s\n' "$(awk '$5 == "/mnt/vendor" || index($5, "/mnt/vendor/") == 1 { n++ } END { print n + 0 }' "$s2_mountinfo" 2>/dev/null)"
for s2_dest in modem_img efs persist modem_userdata; do
	s2_entry=$(awk -v d="/mnt/vendor/$s2_dest" '$5 == d {
			for (i = 7; i <= NF && $i != "-"; i++) ;
			n++; e = $3 " " $6 " " $(i + 1) " " $(i + 3)
		} END { if (n) print n " " e }' "$s2_mountinfo" 2>/dev/null)
	if [ -z "$s2_entry" ]; then
		printf 'dest.%s.mounts=0\ndest.%s.source=none\ndest.%s.ro=none\ndest.%s.fstype=none\n' "$s2_dest" "$s2_dest" "$s2_dest" "$s2_dest"
		continue
	fi
	set -- $s2_entry
	case "$2" in 7:*) s2_source=loop ;; *) s2_source=other ;; esac
	s2_ro=no
	case ",$3," in *,ro,*) case ",$5," in *,ro,*) s2_ro=yes ;; esac ;; esac
	printf 'dest.%s.mounts=%s\ndest.%s.source=%s\ndest.%s.ro=%s\ndest.%s.fstype=%s\n' "$s2_dest" "$1" "$s2_dest" "$s2_source" \
		"$s2_dest" "$s2_ro" "$s2_dest" "$(printf '%s' "$4" | s2_clean)"
done
# Partitions by exact GPT name from the kernel's uevent files, as the mounts
# service resolves them. Only counts and flags are printed, never names.
s2_part_label() {
	case "$1" in
		modem) case "$s2_slot" in _a|_b) printf 'modem%s\n' "$s2_slot" ;; esac ;;
		*) printf '%s\n' "$1" ;;
	esac
}
for s2_role in modem efs persist modem_userdata; do
	s2_label=$(s2_part_label "$s2_role")
	s2_owners=
	[ -z "$s2_label" ] || s2_owners=$(grep -Flx "PARTNAME=$s2_label" "$cpif_sys"/class/block/*/uevent 2>/dev/null)
	s2_count=$(printf '%s\n' "$s2_owners" | s2_lines)
	s2_bdev_ro=unknown s2_loops=0 s2_direct=0 s2_holders=0
	if [ "$s2_count" -eq 1 ]; then
		s2_devname=$(awk -F= '$1 == "DEVNAME" { print $2; exit }' "$s2_owners" 2>/dev/null)
		s2_block="$cpif_sys/class/block/$s2_devname"
		s2_bdev_ro=$(cat "$s2_block/ro" 2>/dev/null | tr -cd 01)
		s2_dev=$(cat "$s2_block/dev" 2>/dev/null | tr -cd 0-9:)
		[ -z "$s2_dev" ] || s2_direct=$(awk -v d="$s2_dev" '$3 == d { n++ } END { print n + 0 }' "$s2_mountinfo" 2>/dev/null)
		s2_holders=$(s2_entries "$s2_block"/holders/*)
		for s2_loop in "$cpif_sys"/class/block/loop*; do
			[ -f "$s2_loop/loop/backing_file" ] || continue
			[ "$(cat "$s2_loop/loop/backing_file" 2>/dev/null)" = "/dev/$s2_devname" ] && s2_loops=$((s2_loops + 1))
		done
	fi
	printf 'part.%s.count=%s\npart.%s.bdev_ro=%s\npart.%s.loops=%s\npart.%s.direct_mounts=%s\npart.%s.holders=%s\n' \
		"$s2_role" "$s2_count" "$s2_role" "${s2_bdev_ro:-unknown}" "$s2_role" "$s2_loops" "$s2_role" "${s2_direct:-0}" "$s2_role" "$s2_holders"
done

# The source and per-unit input checks run only if the accepted mounts
# service is already started; this probe never starts it.
if service_started ferrix-s5300-mounts; then
	printf 'check.sources=%s\n' "$(s2_result cpif_check_sources)"
else
	printf 'check.sources=deferred\n'
fi
printf 'probe_complete=yes\n'
