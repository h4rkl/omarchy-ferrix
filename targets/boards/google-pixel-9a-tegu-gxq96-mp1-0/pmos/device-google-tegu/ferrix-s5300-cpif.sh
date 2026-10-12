# Fail-closed checks for the google-kleaf-cpif S5300 backend on the exact
# google-pixel-9a-tegu-gxq96-mp1-0 (tegu / GXQ96 / MP1.0) only. Sourced by
# /etc/init.d/exynos-modem and /etc/init.d/sit-proxy; never run on its own.
# See ../../s5300-cpif-integration.md.
#
# Nothing here opens a CPIF character device. Opening /dev/umts_boot0 or
# /dev/umts_ipc* runs the link's init_comm hook, so /dev paths are touched only
# by shell file tests (-e -L -c), stat(2), and find(1)'s directory walk,
# including stat through the /proc/<pid>/task/<tid>/fd links. No CPIF device
# is opened. The one device the library opens is the opener scan's positive
# control, /dev/null, read-only, and only after stat(2) shows that it is the
# mem-class null device under no CPIF major (issue #107). No per-unit file
# content is read: the modem image, NV files, and cpsha are checked by
# stat(2) metadata only. The only programs run are
# sha256sum on installed package files, the util-linux probes that the
# accepted ferrix-s5300-mounts verify path itself runs, and the two patched
# binaries with --print-config, which resolve their CLI and exit before any
# node is opened.

cpif_target=google-pixel-9a-tegu-gxq96-mp1-0
cpif_backend=google-kleaf-cpif
cpif_dt_model='ZUMA PRO TEGU MP on ZUMA PRO'
cpif_dt_compatible='google,ZUMA PRO TEGU'
cpif_kernel=6.1.124-android14-11-ga4662a84a15f
# E1 observed exactly these on the exact device (README "S5300 CPIF E1
# inventory"): one bound cp_interface platform device and twelve nodes in the
# one cpif class.
cpif_class=cpif
cpif_driver=cp_interface
cpif_nodes='umts_boot0 umts_ipc0 umts_ipc1 umts_rfs0 oem_ipc0 oem_ipc1 oem_ipc2 oem_ipc3 oem_ipc4 oem_ipc5 oem_ipc6 oem_ipc7'
cpif_modules='cpif shm_ipc cpif_page'
# E0 row T1: the CP3A TOC sizes both NV sections at 0x80000 bytes (#79).
cpif_nv_min_bytes=524288
# The exynos-modem package that carries patches 0001-0003.
cpif_modem_package=exynos-modem
cpif_modem_version=0.1.0_git20260731-r3
# The ferrix-s5300-mounts service accepted for the #34 gate. Its verify path
# is reused unchanged below, so it is hash-checked before it is sourced.
cpif_mounts_sha256=b90d3b41ac175c93e5da5db0858327157da414bc9ca06d403c598108db22f74b

# The offline test rewrites only these lines to point at a fixture.
cpif_fs=
cpif_proc=/proc
cpif_sys=/sys
cpif_dev=/dev
cpif_run=/run
cpif_uid=0
cpif_gid=0

cpif_pins="$cpif_fs/usr/share/ferrix/pixel-9a/s5300-cpif-services.sha256"
cpif_mounts_initd="$cpif_fs/etc/init.d/ferrix-s5300-mounts"
cpif_apk_db="$cpif_fs/lib/apk/db/installed"
cpif_modem_bin="$cpif_fs/usr/sbin/exynos-modem"
cpif_sit_bin="$cpif_fs/usr/sbin/sit-proxy"
cpif_scratch="$cpif_run/exynos-modem"
cpif_arm_token="$cpif_run/ferrix-s5300-cpif.armed"
# Every installed file whose content decides what this backend does. Each
# must appear exactly once in the pins file.
cpif_pinned_paths='/etc/init.d/exynos-modem /etc/init.d/sit-proxy /etc/init.d/ferrix-s5300-mounts /etc/init.d/modemmanager /usr/lib/ferrix/s5300-cpif.sh /usr/share/ferrix/pixel-9a/s5300-userspace-sources.toml /usr/lib/udev/rules.d/99-google-tegu-sit.rules'

cpif_sha256() {
	sha256sum "$1" 2>/dev/null | cut -c1-64
}

# Exact target, backend, kernel, and caller.
cpif_check_target() {
	[ "${FERRIX_TARGET:-}" = "$cpif_target" ] || {
		eerror "S5300 CPIF refuses FERRIX_TARGET '${FERRIX_TARGET:-}'; only $cpif_target is supported"
		return 1
	}
	[ "${FERRIX_S5300_KERNEL_BACKEND:-}" = "$cpif_backend" ] || {
		eerror "S5300 CPIF checks need FERRIX_S5300_KERNEL_BACKEND=$cpif_backend"
		return 1
	}
	model=$(tr '\000' '\n' <"$cpif_proc/device-tree/model" 2>/dev/null | sed -n '1p')
	[ "$model" = "$cpif_dt_model" ] &&
		tr '\000' '\n' <"$cpif_proc/device-tree/compatible" 2>/dev/null | grep -Fxq "$cpif_dt_compatible" || {
		eerror "the device tree is not the exact tegu MP board"
		return 1
	}
	[ "$(uname -r)" = "$cpif_kernel" ] || {
		eerror "the running kernel is not the pinned Kleaf $cpif_kernel"
		return 1
	}
	[ "$(id -u)" = "$cpif_uid" ] || {
		eerror "S5300 CPIF checks must run as root"
		return 1
	}
}

# Installed service files match the package pins, the installed modem
# package is the patched release, and both binaries resolve exactly the
# expected CPIF configuration.
cpif_check_package() {
	[ -f "$cpif_pins" ] && [ ! -L "$cpif_pins" ] || {
		eerror "S5300 CPIF service pins are missing: $cpif_pins"
		return 1
	}
	for pinned in $cpif_pinned_paths; do
		pin=$(awk -v p="$pinned" '$2 == p { n++; h = $1 } END { if (n == 1) print h }' "$cpif_pins")
		[ -n "$pin" ] || {
			eerror "S5300 CPIF pins must name $pinned exactly once"
			return 1
		}
		[ ! -L "$cpif_fs$pinned" ] && [ "$(cpif_sha256 "$cpif_fs$pinned")" = "$pin" ] || {
			eerror "installed $pinned is not the pinned package file (stale or modified)"
			return 1
		}
	done
	version=$(awk -v p="$cpif_modem_package" '
		/^P:/ { name = substr($0, 3) }
		/^V:/ && name == p { print substr($0, 3); n++ }
		/^$/ { name = "" }
		END { if (n != 1) exit 1 }' "$cpif_apk_db" 2>/dev/null) || version=
	[ "$version" = "$cpif_modem_version" ] || {
		eerror "installed $cpif_modem_package is '${version:-missing}', not $cpif_modem_version"
		return 1
	}
}

# cpif_check_cli BINARY EXPECTED ARGS...: BINARY with ARGS and --print-config
# must print exactly EXPECTED. --print-config exits before any node is opened.
cpif_check_cli() {
	bin=$1
	expected=$2
	shift 2
	[ -x "$bin" ] || {
		eerror "$bin is missing"
		return 1
	}
	got=$("$bin" "$@" --print-config 2>/dev/null) || got=
	[ "$got" = "$expected" ] || {
		eerror "$bin does not resolve the reviewed $cpif_backend configuration"
		return 1
	}
}

# cpif_check_surface STATE: the three modules are Live, exactly one
# samsung,exynos-cp device is bound to cp_interface, its modem_state is
# exactly STATE, and no WWAN device exists.
cpif_check_surface() {
	wanted=$1
	for module in $cpif_modules; do
		state=$(awk -v m="$module" '$1 == m { n++; s = $5 } END { if (n == 1) print s }' "$cpif_proc/modules" 2>/dev/null)
		[ "$state" = Live ] || {
			eerror "kernel module $module is not loaded exactly once and Live"
			return 1
		}
	done
	count=0
	cp_entry=
	for entry in "$cpif_sys"/bus/platform/devices/*; do
		[ -f "$entry/of_node/compatible" ] || continue
		tr '\000' '\n' <"$entry/of_node/compatible" 2>/dev/null | grep -Fxq 'samsung,exynos-cp' || continue
		count=$((count + 1))
		cp_entry=$entry
	done
	[ "$count" -eq 1 ] || {
		eerror "expected exactly one samsung,exynos-cp platform device (found $count)"
		return 1
	}
	driver=$(readlink "$cp_entry/driver" 2>/dev/null | sed 's#/*$##; s#.*/##')
	[ "$driver" = "$cpif_driver" ] || {
		eerror "the CP platform device is bound to '${driver:-nothing}', not $cpif_driver"
		return 1
	}
	state=$(head -c 64 "$cp_entry/modem_state" 2>/dev/null | tr -cd 'A-Za-z0-9_')
	[ "$state" = "$wanted" ] || {
		eerror "modem_state is '${state:-unreadable}', not $wanted"
		return 1
	}
	for entry in "$cpif_sys"/class/wwan/*; do
		[ -e "$entry" ] || [ -L "$entry" ] || continue
		eerror "a WWAN device exists; this is not the Kleaf CPIF surface"
		return 1
	done
}

# cpif_check_node NAME: /dev/NAME is a character device, not a symlink, with
# exactly one sysfs class entry, in the cpif class, whose dev and subsystem
# match. Only file tests and stat(2) touch the node.
cpif_check_node() {
	node=$1
	path="$cpif_dev/$node"
	[ ! -L "$path" ] && [ -c "$path" ] || {
		eerror "$path is missing, a symlink, or not a character device"
		return 1
	}
	meta=$(stat -c '%t %T' "$path" 2>/dev/null) || meta=
	major=${meta% *}
	minor=${meta#* }
	case "$major:$minor" in
		*[!0-9a-fA-F:]*|:*|*:) eerror "cannot read the device number of $path"; return 1 ;;
	esac
	rdev=$(printf '%d:%d' "0x$major" "0x$minor")
	count=0
	class_entry=
	for entry in "$cpif_sys"/class/*/"$node"; do
		[ -e "$entry" ] || [ -L "$entry" ] || continue
		count=$((count + 1))
		class_entry=$entry
	done
	[ "$count" -eq 1 ] || {
		eerror "$node must have exactly one sysfs class entry (found $count)"
		return 1
	}
	class=${class_entry%/*}
	class=${class##*/}
	subsystem=$(readlink "$class_entry/subsystem" 2>/dev/null | sed 's#/*$##; s#.*/##')
	[ "$class" = "$cpif_class" ] && [ "$subsystem" = "$cpif_class" ] || {
		eerror "$node is in class '$class' (subsystem '${subsystem:-none}'), not $cpif_class"
		return 1
	}
	[ "$(cat "$class_entry/dev" 2>/dev/null)" = "$rdev" ] || {
		eerror "$path ($rdev) is not the device that sysfs names $node"
		return 1
	}
}

# The initial PID namespace's fixed inode (PROC_PID_INIT_INO). Only there
# does /proc list every task on the system.
cpif_init_pidns='pid:[4026531836]'

# The opener scan's positive control (issue #107): a harmless character
# device that is not CPIF, held open read-only by the scanning shell itself
# while it scans. It is the null device of the sysfs mem class. It is opened
# only after stat(2) shows that the node is a character device (not a
# symlink) with exactly that device number, under no CPIF major.
cpif_control_dev="$cpif_dev/null"
cpif_control_sys="$cpif_sys/class/mem/null/dev"

# cpif_rdev_key MAJOR:MINOR: a decimal sysfs device number in the "%t:%T"
# form stat(1) prints (lower-case hex). Fails on anything else.
cpif_rdev_key() {
	case "$1" in
		[0-9]*:[0-9]*) ;;
		*) return 1 ;;
	esac
	case "${1%%:*}${1#*:}" in *[!0-9]*) return 1 ;; esac
	case "$1" in 0[0-9]*|*:0[0-9]*) return 1 ;; esac
	printf '%x:%x' "${1%%:*}" "${1#*:}"
}

# cpif_opener_scan NAME...: the opener scan. It prints "holder PID NAME" for
# each task with an open file that is the character device sysfs names NAME
# in the cpif class (cpif_check_node has matched it to /dev/NAME), and
# "holder PID +foreign" for an open character device under a CPIF major that
# is not in the cpif class at all. Then it prints these facts, once each:
#
#   fact opener_positive_control=detected|missing
#   fact opener_format_control=matched|mismatch|missing
#   fact opener_major_foreign=<count>|unknown
#   fact opener_vanished=<count>
#   fact opener_scan=clean|held|incomplete
#
# Matching is by device number from stat(2) through each
# /proc/<pid>/task/<tid>/fd magic link, so any path, alias, deleted node, or
# mount namespace that reaches the same device counts, and every thread's fd
# table is read. stat(2) never opens the file.
#
# Positive control: the shell that runs the scan holds the control device
# open on fd 7 for the whole scan. The scan counts only if the same stat
# format, the same /proc walk and the same matcher report that device as held
# by that shell's own pid (read from /proc/self/stat by the same process). A
# stat(1) output format, /proc layout, or pid parse that the matcher does not
# understand therefore reads as `incomplete`, never as `clean`. Because the
# null device's numbers (1:3) read the same in decimal and hex, the format
# control adds the CPIF numbers themselves: every /dev character device named
# like a cpif class entry must print, through the same stat format and
# matcher, exactly that entry's sysfs number.
#
# Every character device under a CPIF major, in sysfs (/sys/dev/char) and in
# /dev, must be the cpif class entry of that name and number. Others are
# counted (never named) in opener_major_foreign. Every cpif class entry must
# appear in /sys/dev/char.
#
# The scan is `incomplete` (status 2) if this shell is outside the initial
# PID namespace; a live pid's task directory or a live task's fd directory
# cannot be listed, or is empty; a live fd or /dev entry cannot be stat'ed;
# /dev cannot be walked; the positive or format control fails; a cpif class
# entry is missing from /sys/dev/char; or the scan subshell does not reach its
# completion line. A pid, task, or fd that is gone when re-checked has exited
# or closed (process churn): it is skipped, and pids and tasks are counted in
# opener_vanished. Status 2 also follows a foreign device. The scan is a
# snapshot: an open after a task was read is not seen, and SCM_RIGHTS fds in
# flight or kernel-internal references are not visible at all
# (s5300-cpif-integration.md).
cpif_opener_scan() (
	control=missing format=missing foreign=unknown vanished=0 result=incomplete
	cpif_scan_facts() {
		printf 'fact opener_positive_control=%s\nfact opener_format_control=%s\nfact opener_major_foreign=%s\nfact opener_vanished=%s\nfact opener_scan=%s\n' \
			"$control" "$format" "$foreign" "$vanished" "$result"
	}
	[ "$(readlink "$cpif_proc/self/ns/pid" 2>/dev/null)" = "$cpif_init_pidns" ] || {
		eerror "the opener scan must run in the initial PID namespace" >&2
		cpif_scan_facts
		exit 2
	}
	[ "$#" -gt 0 ] || {
		eerror "no CPIF device was named for the opener scan" >&2
		cpif_scan_facts
		exit 2
	}
	# Every cpif class entry by its stat(1) key, and its decimal major.
	class=
	majors=
	for entry in "$cpif_sys/class/$cpif_class"/*; do
		[ -e "$entry/dev" ] || continue
		name=${entry##*/}
		rdev=$(cat "$entry/dev" 2>/dev/null) || rdev=
		key=$(cpif_rdev_key "$rdev") && case "$name" in *[!A-Za-z0-9_.-]*) false ;; esac || {
			eerror "cannot read the cpif class entry $name" >&2
			cpif_scan_facts
			exit 2
		}
		class="$class $key=$name"
		majors="$majors ${rdev%%:*}"
	done
	wanted=
	for node; do
		rdev=$(cat "$cpif_sys/class/$cpif_class/$node/dev" 2>/dev/null) || rdev=
		key=$(cpif_rdev_key "$rdev") || {
			eerror "cannot read the device number of $node" >&2
			cpif_scan_facts
			exit 2
		}
		wanted="$wanted $key=$node"
	done
	# The control is opened only if it is the mem-class null device and its
	# major is no CPIF major.
	control_key=$(cpif_rdev_key "$(cat "$cpif_control_sys" 2>/dev/null)") || control_key=
	control_ok=no
	case "$control_key: $class " in
		:*|*" ${control_key%%:*}:"*) ;;
		*)
			[ "$(stat -c '%t:%T|%F' -- "$cpif_control_dev" 2>/dev/null | tr 'A-F' 'a-f')" = "$control_key|character special file" ] &&
				control_ok=yes
			;;
	esac
	cpif_scan_task() {
		set -- "$1"/fd/[0-9]*
		task=${1%/fd/*}
		if [ "$#" -eq 1 ] && [ ! -e "$1" ] && [ ! -L "$1" ]; then
			# No open fd, a directory that cannot be listed, or a task that
			# has exited.
			ls -A "$task/fd" >/dev/null 2>&1 && return
			if [ -e "$task" ]; then echo unreadable; else echo vanished; fi
			return
		fi
		stat -L -c 'fd|%n|%t:%T|%F' -- "$@" 2>/dev/null && return
		for fd; do
			[ -L "$fd" ] || continue
			stat -L -c %F -- "$fd" >/dev/null 2>&1 || [ ! -L "$fd" ] || echo unreadable
		done
	}
	# Runs with the control on fd 7, in the process whose pid it reports.
	cpif_scan_body() {
		self=
		read -r self _ 2>/dev/null <"$cpif_proc/self/stat" || self=
		echo "self ${self:-none}"
		for pid in "$cpif_proc"/[0-9]*; do
			case "${pid##*/}" in *[!0-9]*) continue ;; esac
			set -- "$pid"/task/[0-9]*
			if [ "$#" -eq 1 ] && [ ! -e "$1" ] && [ ! -L "$1" ]; then
				# A live process always has a task; an empty or unlistable
				# task directory blocks unless the pid has gone.
				if [ -e "$pid" ]; then echo unreadable; else echo vanished; fi
				continue
			fi
			for task; do
				cpif_scan_task "$task"
			done
		done
		set -f
		old_ifs=$IFS
		IFS='
'
		devs=$(find "$cpif_dev" ! -type d ! -type l 2>/dev/null) || echo unreadable
		set -- $devs
		IFS=$old_ifs
		set +f
		if [ "$#" -gt 0 ] && ! stat -c 'dev|%n|%t:%T|%F' -- "$@" 2>/dev/null; then
			for entry; do
				stat -c %F -- "$entry" >/dev/null 2>&1 || { [ ! -e "$entry" ] && [ ! -L "$entry" ]; } || echo unreadable
			done
		fi
		for entry in "$cpif_sys"/dev/char/*; do
			number=${entry##*/}
			case " $majors " in *" ${number%%:*} "*) ;; *) continue ;; esac
			key=$(cpif_rdev_key "$number") || key=bad
			name=$(readlink "$entry" 2>/dev/null) && subsystem=$(readlink "$entry/subsystem" 2>/dev/null) || {
				echo unreadable
				continue
			}
			echo "sys|${name##*/}|$key|${subsystem##*/}"
		done
		echo complete
	}
	# The scan ends with a "complete" line, so a subshell that failed part
	# way can never read as an empty, clean scan.
	if [ "$control_ok" = yes ]; then
		scan=$(cpif_scan_body 7<"$cpif_control_dev") || scan=
	else
		eerror "the opener scan's positive control is not the mem-class null device; not opening it" >&2
		scan=$(cpif_scan_body) || scan=
	fi
	printf '%s\n' "$scan" | awk -v wanted="$wanted" -v class="$class" -v ctl="$control_key" -v ctl_ok="$control_ok" \
		-v proc="$cpif_proc/" -v cls_name="$cpif_class" '
		function major(k) { return substr(k, 1, index(k, ":") - 1) }
		function load(list, arr,    n, w, i, eq) {
			n = split(list, w, " ")
			for (i = 1; i <= n; i++) { eq = index(w[i], "="); arr[substr(w[i], 1, eq - 1)] = substr(w[i], eq + 1) }
		}
		BEGIN {
			load(wanted, want); load(class, cls)
			for (k in cls) { majors[major(k)] = 1; byname[cls[k]] = k }
		}
		$0 == "complete" { done = 1; next }
		$0 == "unreadable" { bad = 1; next }
		$0 == "vanished" { gone++; next }
		$1 == "self" && NF == 2 { if ($2 ~ /^[0-9]+$/) self = $2; next }
		{
			n = split($0, f, "|")
			if (n < 4) { bad = 1; next }
			kind = f[n]; k = tolower(f[n - 1]); path = f[2]
			for (i = 3; i < n - 1; i++) path = path "|" f[i]
		}
		f[1] == "fd" {
			if (kind != "character special file") next
			rel = substr(path, length(proc) + 1)
			pid = substr(rel, 1, index(rel, "/") - 1)
			if (k == ctl) { if (ctl_ok == "yes" && self != "" && pid == self) control = 1; next }
			if (k in want) name = want[k]
			else if (k in cls) next
			else if (major(k) in majors) name = "+foreign"
			else next
			key = pid " " name
			if (!(key in seen)) { seen[key] = 1; holder[++held] = key }
			next
		}
		f[1] == "dev" {
			if (kind != "character special file") next
			b = path; sub(/.*\//, "", b)
			if (b in byname) { if (byname[b] == k) fmt_ok++; else fmt_bad++ }
			if ((major(k) in majors) && !((k in cls) && cls[k] == b)) foreign++
			next
		}
		f[1] == "sys" {
			if (k == "bad") { bad = 1; next }
			if (kind == cls_name && (k in cls) && cls[k] == path) listed[k] = 1
			else foreign++
			next
		}
		{ bad = 1 }
		END {
			for (k in cls) if (!(k in listed)) unlisted = 1
			for (i = 1; i <= held; i++) print "holder " holder[i]
			fmt = fmt_bad ? "mismatch" : (fmt_ok ? "matched" : "missing")
			incomplete = bad || !done || unlisted || !control || fmt != "matched"
			printf "fact opener_positive_control=%s\n", control ? "detected" : "missing"
			printf "fact opener_format_control=%s\n", fmt
			printf "fact opener_major_foreign=%s\n", (bad || !done || unlisted) ? "unknown" : foreign + 0
			printf "fact opener_vanished=%d\n", gone
			printf "fact opener_scan=%s\n", incomplete ? "incomplete" : (held ? "held" : "clean")
			exit ((incomplete || foreign) ? 2 : 0)
		}'
)

# cpif_holders NAME...: print "PID NAME" for each holder that the opener scan
# above reports. Returns 2, with nothing printed, if the scan is incomplete
# or found a foreign device under a CPIF major.
cpif_holders() {
	scan_out=$(cpif_opener_scan "$@") || :
	case "$scan_out" in
		*"fact opener_scan=clean"*|*"fact opener_scan=held"*) ;;
		*)
			case "$scan_out" in
				*"fact opener_positive_control=detected"*) ;;
				*) eerror "the CPIF opener scan's positive control was not detected" >&2 ;;
			esac
			eerror "the CPIF opener scan could not read every open file; refusing" >&2
			return 2
			;;
	esac
	case "$scan_out" in
		*"fact opener_major_foreign=0"*) ;;
		*)
			eerror "a character device under a CPIF major is not the cpif class entry of that name; refusing" >&2
			return 2
			;;
	esac
	printf '%s\n' "$scan_out" | sed -n 's/^holder //p'
}

# Every entry of the cpif sysfs class, so that the modem preflight also sees
# a holder of a CPIF device outside the twelve E1 names.
cpif_class_nodes() {
	for entry in "$cpif_sys/class/$cpif_class"/*; do
		[ -e "$entry/dev" ] || continue
		name=${entry##*/}
		case "$name" in *[!A-Za-z0-9_.-]*) continue ;; esac
		printf '%s\n' "$name"
	done
}

# cpif_check_unheld NAME...: no task holds any of the named CPIF devices, by
# the cpif_holders scan above. An incomplete scan is a refusal.
cpif_check_unheld() {
	holders=$(cpif_holders "$@") || return 1
	[ -z "$holders" ] || {
		set -- $holders
		case "$2" in
			+foreign) eerror "a character device under a CPIF major outside the cpif class is already open (pid $1)" ;;
			*) eerror "/dev/$2 is already open (pid $1)" ;;
		esac
		return 1
	}
}

# Directory $1 is not a symlink, is owned by root, and is not group- or
# other-writable.
cpif_safe_dir() {
	[ ! -L "$1" ] && [ -d "$1" ] || return 1
	meta=$(stat -c '%u %a' "$1" 2>/dev/null) || return 1
	[ "${meta% *}" = "$cpif_uid" ] && [ $((0${meta#* } & 022)) -eq 0 ]
}

# /run is a tmpfs, and / and /run cannot be renamed under by anyone but root.
cpif_check_run() {
	run_type=$(awk '$5 == "/run" { for (i = 7; i <= NF && $i != "-"; i++) ; t = $(i + 1) } END { print t }' \
		"$cpif_proc/self/mountinfo" 2>/dev/null)
	[ "$run_type" = tmpfs ] || {
		eerror "/run is '${run_type:-not mounted}', not tmpfs"
		return 1
	}
	cpif_safe_dir "$cpif_fs/" && cpif_safe_dir "$cpif_run" || {
		eerror "/ or /run is a symlink, not root-owned, or group/other-writable"
		return 1
	}
}

cpif_check_scratch() {
	cpif_check_run || return 1
	[ ! -L "$cpif_scratch" ] && [ -d "$cpif_scratch" ] &&
		[ "$(stat -c '%u %g %a' "$cpif_scratch" 2>/dev/null)" = "$cpif_uid $cpif_gid 700" ] || {
		eerror "$cpif_scratch is not a root-owned 0700 directory; run 'rc-service exynos-modem provision'"
		return 1
	}
}

# Create the private scratch directory only. Refuses a symlink or non-directory
# rather than repairing it; checkpath never runs on an unsafe /run.
cpif_provision_scratch() {
	cpif_check_run || return 1
	[ ! -L "$cpif_scratch" ] && { [ ! -e "$cpif_scratch" ] || [ -d "$cpif_scratch" ]; } || {
		eerror "$cpif_scratch exists and is not a directory; refusing it"
		return 1
	}
	checkpath -d -m 0700 -o root:root "$cpif_scratch" || return 1
	cpif_check_scratch
}

# The accepted ferrix-s5300-mounts is started and every destination still
# passes its own verify_own_mount (read-only loop over the exact source,
# recovery suppressed). Its verify path is sourced, unchanged, in a subshell
# after the hash check; nothing is mounted or unmounted. Then the daemon's
# inputs are checked by metadata only.
cpif_check_sources() {
	service_started ferrix-s5300-mounts || {
		eerror "start ferrix-s5300-mounts manually before the CPIF preflight"
		return 1
	}
	[ ! -L "$cpif_mounts_initd" ] && [ "$(cpif_sha256 "$cpif_mounts_initd")" = "$cpif_mounts_sha256" ] || {
		eerror "installed ferrix-s5300-mounts is not the service accepted for the #34 gate"
		return 1
	}
	(
		. "$cpif_mounts_initd"
		for destination in modem_img efs persist modem_userdata; do
			verify_own_mount "$mount_root/$destination" || exit 1
		done
	) || {
		eerror "an S5300 source is not a verified read-only loop mount"
		return 1
	}
	vendor="$cpif_fs/mnt/vendor"
	for file in "$vendor/modem_img/images/default/modem.bin" "$vendor/efs/nv_normal.bin" \
		"$vendor/efs/nv_protected.bin" "$vendor/persist/modem/cpsha"; do
		[ ! -L "$file" ] && [ -f "$file" ] || {
			eerror "required S5300 input is missing or not a regular file: ${file#"$cpif_fs"}"
			return 1
		}
	done
	for file in "$vendor/efs/nv_normal.bin" "$vendor/efs/nv_protected.bin"; do
		size=$(stat -c '%s' "$file" 2>/dev/null) || size=
		case "$size" in ''|*[!0-9]*) size=0 ;; esac
		[ "$size" -ge "$cpif_nv_min_bytes" ] || {
			eerror "${file#"$cpif_fs"} is smaller than $cpif_nv_min_bytes bytes"
			return 1
		}
	done
	for dir in uecapconfig carrierconfig; do
		[ -d "$cpif_fs/vendor/firmware/$dir" ] || {
			eerror "external firmware directory is missing: /vendor/firmware/$dir"
			return 1
		}
	done
}

# One-shot manual-start token on tmpfs /run. `arm` writes it only after the
# full preflight passes; exynos-modem start consumes it before anything else,
# so a reboot (including an AP panic) never restarts the CP by itself.
cpif_write_arm() {
	[ ! -e "$cpif_arm_token" ] && [ ! -L "$cpif_arm_token" ] || {
		eerror "$cpif_arm_token already exists"
		return 1
	}
	(set -C; umask 077; : >"$cpif_arm_token") || {
		eerror "could not create $cpif_arm_token"
		return 1
	}
}

cpif_consume_arm() {
	[ ! -L "$cpif_arm_token" ] && [ -f "$cpif_arm_token" ] || {
		eerror "the CPIF backend starts only manually: run 'rc-service exynos-modem arm' first"
		return 1
	}
	[ "$(stat -c '%u %a %h %s' "$cpif_arm_token" 2>/dev/null)" = "$cpif_uid 600 1 0" ] || {
		eerror "$cpif_arm_token was not written by 'arm'; refusing it"
		return 1
	}
	rm -f -- "$cpif_arm_token" && [ ! -e "$cpif_arm_token" ] || {
		eerror "could not consume $cpif_arm_token"
		return 1
	}
}

# cpif_modem_preflight EXPECTED ARGS...: everything exynos-modem needs before
# its first open, with the CP OFFLINE and no CPIF node held by anyone.
cpif_modem_preflight() {
	expected=$1
	shift
	cpif_check_target || return 1
	cpif_check_package || return 1
	cpif_check_cli "$cpif_modem_bin" "$expected" "$@" || return 1
	cpif_check_surface OFFLINE || return 1
	for node in $cpif_nodes; do
		cpif_check_node "$node" || return 1
	done
	cpif_check_unheld $(cpif_class_nodes) || return 1
	cpif_check_sources || return 1
	cpif_check_scratch
}

# cpif_sit_preflight EXPECTED ARGS...: sit-proxy opens the FMT nodes only
# after exynos-modem has the CP ONLINE and nobody else holds them.
cpif_sit_preflight() {
	expected=$1
	shift
	cpif_check_target || return 1
	cpif_check_package || return 1
	cpif_check_cli "$cpif_sit_bin" "$expected" "$@" || return 1
	service_started exynos-modem || {
		eerror "sit-proxy on $cpif_backend needs a running exynos-modem"
		return 1
	}
	cpif_check_surface ONLINE || return 1
	cpif_check_node umts_ipc0 && cpif_check_node umts_ipc1 || return 1
	cpif_check_unheld umts_ipc0 umts_ipc1
}
