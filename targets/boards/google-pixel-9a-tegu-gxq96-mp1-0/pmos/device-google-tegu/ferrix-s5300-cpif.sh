# Fail-closed checks for the google-kleaf-cpif S5300 backend on the exact
# google-pixel-9a-tegu-gxq96-mp1-0 (tegu / GXQ96 / MP1.0) only. Sourced by
# /etc/init.d/exynos-modem and /etc/init.d/sit-proxy; never run on its own.
# See ../../s5300-cpif-integration.md.
#
# Nothing here opens a CPIF character device. Opening /dev/umts_boot0 or
# /dev/umts_ipc* runs the link's init_comm hook, so /dev paths are touched only
# by shell file tests (-e -L -c), stat(2), and readlink(2) of /proc/<pid>/fd
# links. No per-unit file content is read: the modem image, NV files, and
# cpsha are checked by stat(2) metadata only. The only programs run are
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

# cpif_check_unheld NAME...: no process has /dev/NAME open. Reads only the
# /proc/<pid>/fd link targets.
cpif_check_unheld() {
	for fd in "$cpif_proc"/[0-9]*/fd/*; do
		target=$(readlink "$fd" 2>/dev/null) || continue
		for node; do
			[ "$target" = "/dev/$node" ] || continue
			pid=${fd#"$cpif_proc"/}
			eerror "/dev/$node is already open (pid ${pid%%/*})"
			return 1
		done
	done
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
	cpif_check_unheld $cpif_nodes || return 1
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
