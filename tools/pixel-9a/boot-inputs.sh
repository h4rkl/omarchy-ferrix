#!/bin/sh
set -eu

# Build the source-owned ramdisk and module-root inputs consumed by the
# radio-first split composer. This is a device-free native-Linux workflow:
# every byte comes from the pinned radio kernel/rootfs handoffs or from the
# checked-in target package. It never extracts stock Android boot images and
# never contacts ADB/Fastboot.

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
target="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0"
package_dir="$target/pmos/device-google-tegu"
route="$target/kernel-radio-route.toml"
out_root=${FERRIX_OUT_DIR:-"$root/out"}
kernel_dist=${FERRIX_PIXEL9A_RADIO_KERNEL_DIST:-"$out_root/pixel-9a/kernel-radio/tegu-wwan/dist"}
kernel_manifest=${FERRIX_PIXEL9A_RADIO_KERNEL_MANIFEST:-"$out_root/pixel-9a/kernel-radio/tegu-wwan/build-manifest.tsv"}
rootfs_out=${FERRIX_PIXEL9A_ROOTFS_OUT:-"$out_root/pixel-9a/rootfs/tegu"}
rootfs_manifest=${FERRIX_PIXEL9A_ROOTFS_MANIFEST:-"$rootfs_out/manifest.tsv"}
inputs_out=${FERRIX_PIXEL9A_BOOT_INPUTS_OUT:-"$out_root/pixel-9a/boot-inputs/tegu-wwan"}
inputs_manifest=${FERRIX_PIXEL9A_BOOT_INPUTS_MANIFEST:-"$inputs_out/manifest.tsv"}
initfs_list="$package_dir/modules-initfs-radio"
vendor_list="$package_dir/modules-vendor-dlkm"
system_list="$package_dir/modules-system-dlkm"
init_script="$package_dir/initramfs-init"
vendor_bootconfig_source="$package_dir/vendor-bootconfig"
vendor_ramdisk_source="$package_dir/vendor-ramdisk"
build_confirmation=I-understand-this-builds-source-owned-pixel-9a-boot-inputs
tab=$(printf '\t')

die() {
	printf 'FAIL pixel-9a-boot-inputs: %s\n' "$*" >&2
	exit 1
}

sha256_of() {
	if command -v sha256sum >/dev/null 2>&1; then
		sha256sum "$1" | awk '{ print $1 }'
	else
		shasum -a 256 "$1" | awk '{ print $1 }'
	fi
}

file_size() { wc -c <"$1" | tr -d '[:space:]'; }

safe_relative() {
	case "$1" in
		''|/*|.|..|../*|*/../*|*/..|*' '*|*"$tab"*) die "unsafe relative path: $1" ;;
	esac
}

require_file() {
	[ -f "$1" ] || die "$2 is missing: $1"
	[ ! -L "$1" ] || die "$2 must not be a symlink: $1"
}

require_dir() {
	[ -d "$1" ] || die "$2 is missing: $1"
	[ ! -L "$1" ] || die "$2 must not be a symlink: $1"
}

manifest_value() {
	awk -F "$tab" -v key="$2" '$1 == key { print $2; exit }' "$1"
}

manifest_artifact_row() {
	awk -F "$tab" -v name="$2" '$1 == "artifact" && $2 == name { print; found=1; exit } END { if (!found) exit 1 }' "$1"
}

field() { printf '%s\n' "$1" | awk -F "$tab" -v number="$2" '{ print $number }'; }

verify_handoff_artifact() {
	handoff_manifest=$1
	base=$2
	name=$3
	row=$(manifest_artifact_row "$handoff_manifest" "$name") || die "$name is missing from $handoff_manifest"
	relative=$(field "$row" 2)
	expected_bytes=$(field "$row" 3)
	expected_hash=$(field "$row" 4)
	safe_relative "$relative"
	case "$expected_bytes" in ''|*[!0-9]*) die "$name has a malformed byte count" ;; esac
	case "$expected_hash" in ''|*[!0-9a-f]*) die "$name has a malformed SHA-256" ;; esac
	[ "${#expected_hash}" -eq 64 ] || die "$name has a malformed SHA-256 length"
	file="$base/$relative"
	require_file "$file" "$name handoff artifact"
	[ "$(file_size "$file")" = "$expected_bytes" ] || die "$name handoff size differs from its manifest"
	[ "$(sha256_of "$file")" = "$expected_hash" ] || die "$name handoff SHA-256 differs from its manifest"
	printf '%s\n' "$file"
}

route_value() {
	key=$1
	awk -v key="$key" '$0 ~ "^" key "[[:space:]]*=" {
		value=$0
		sub(/^[^=]*=[[:space:]]*/, "", value)
		sub(/[[:space:]]*#.*/, "", value)
		if (value ~ /^"/) { sub(/^"/, "", value); sub(/"[[:space:]]*$/, "", value) }
		print value; found=1; exit
	} END { if (!found) exit 1 }' "$route"
}

check_target_inputs() {
	require_file "$route" 'radio route metadata'
	[ "$(route_value target_id)" = google-pixel-9a-tegu-gxq96-mp1-0 ] || die 'radio route target is not the exact Pixel 9a'
	[ "$(route_value route_id)" = experimental-linux-google-tegu-s5300-wwan ] || die 'radio route is not the reviewed WWAN route'
	[ "$(route_value status)" = experimental-not-adopted ] || die 'radio route must remain experimental-not-adopted'
	for file in "$init_script" "$vendor_bootconfig_source" "$initfs_list" "$vendor_list" "$system_list"; do
		require_file "$file" 'checked-in boot input'
	done
	require_dir "$vendor_ramdisk_source" 'source-owned vendor ramdisk'
	if find "$vendor_ramdisk_source" -type l -print -quit | grep -q .; then
		die 'source-owned vendor ramdisk contains a symlink'
	fi
	if find "$vendor_ramdisk_source" ! -type f ! -type d -print -quit | grep -q .; then
		die 'source-owned vendor ramdisk contains a non-regular entry'
	fi
	while IFS= read -r ramdisk_file; do
		relative=${ramdisk_file#"$vendor_ramdisk_source"/}
		[ "$relative" != "$ramdisk_file" ] || continue
		safe_relative "$relative"
	done <<EOF
$(find "$vendor_ramdisk_source" -mindepth 1 -type f -print)
EOF
	grep -Fqx 'ferrix.target_id = "google-pixel-9a-tegu-gxq96-mp1-0"' "$vendor_bootconfig_source" || die 'vendor bootconfig target identity is missing'
	grep -Fqx 'ferrix.route_id = "experimental-linux-google-tegu-s5300-wwan"' "$vendor_bootconfig_source" || die 'vendor bootconfig route identity is missing'
	grep -Fqx 'ferrix.rootfs_partlabel = "userdata"' "$vendor_bootconfig_source" || die 'vendor bootconfig rootfs mapping is missing'
	grep -Fqx '#!/bin/busybox sh' "$init_script" || die 'initramfs entrypoint must use the source-built rootfs BusyBox'
	grep -Fq 'root_partlabel=userdata' "$init_script" || die 'initramfs entrypoint does not use the reviewed userdata PARTLABEL'
}

check_host() {
	os=$(uname -s)
	arch=$(uname -m)
	[ "$os" = Linux ] || die "native Linux host required; detected $os/$arch"
	[ "$arch" = x86_64 ] || die "native Linux x86_64 host required; detected $os/$arch"
}

check_tools() {
	for command_name in awk basename chmod cp cpio dirname find grep head mkdir mktemp mv readelf rm sed sort tar tr wc; do
		command -v "$command_name" >/dev/null 2>&1 || die "missing prerequisite '$command_name'"
	done
	command -v depmod >/dev/null 2>&1 || die "missing prerequisite 'depmod'"
	command -v sha256sum >/dev/null 2>&1 || command -v shasum >/dev/null 2>&1 || die 'missing sha256sum or shasum'
}

check_handoffs() {
	require_file "$kernel_manifest" 'radio kernel manifest'
	require_file "$rootfs_manifest" 'radio rootfs manifest'
	[ "$(manifest_value "$kernel_manifest" target_id)" = google-pixel-9a-tegu-gxq96-mp1-0 ] || die 'radio kernel manifest target is not exact Pixel 9a'
	[ "$(manifest_value "$kernel_manifest" route_id)" = experimental-linux-google-tegu-s5300-wwan ] || die 'radio kernel manifest route is not the selected route'
	[ "$(manifest_value "$kernel_manifest" kernel_backend)" = linux-google-tegu-wwan ] || die 'radio kernel manifest backend is not WWAN/SIT'
	[ "$(manifest_value "$kernel_manifest" status)" = built-not-booted ] || die 'radio kernel manifest is not a completed build handoff'
	[ "$(manifest_value "$kernel_manifest" bootable)" = false ] || die 'radio kernel manifest must remain bootable=false'
	[ "$(manifest_value "$rootfs_manifest" target_id)" = google-pixel-9a-tegu-gxq96-mp1-0 ] || die 'radio rootfs manifest target is not exact Pixel 9a'
	[ "$(manifest_value "$rootfs_manifest" kernel_backend)" = linux-google-tegu-wwan ] || die 'radio rootfs manifest backend is not WWAN/SIT'
	[ "$(manifest_value "$rootfs_manifest" status)" = built-substrate-only ] || die 'radio rootfs manifest is not a completed substrate handoff'
	[ "$(manifest_value "$rootfs_manifest" bootable)" = false ] || die 'radio rootfs manifest must remain bootable=false'
	rootfs_tar=$(verify_handoff_artifact "$rootfs_manifest" "$rootfs_out" rootfs.tar)
	module_row=$(awk -F "$tab" '$1 == "module_install" { print; found=1; exit } END { if (!found) exit 1 }' "$kernel_manifest") || die 'radio kernel manifest lacks module_install'
	module_manifest_relative=$(field "$module_row" 2)
	safe_relative "$module_manifest_relative"
	[ "$module_manifest_relative" = split-inputs/module-install.tsv ] || die 'source-built module manifest path is not canonical'
	module_manifest="$kernel_dist/$module_manifest_relative"
	require_file "$module_manifest" 'source-built module manifest'
	[ "$(file_size "$module_manifest")" = "$(field "$module_row" 3)" ] || die 'source-built module manifest size differs from kernel handoff'
	[ "$(sha256_of "$module_manifest")" = "$(field "$module_row" 4)" ] || die 'source-built module manifest hash differs from kernel handoff'
	[ "$(manifest_value "$module_manifest" schema_version)" = 1 ] || die 'source-built module manifest schema is not 1'
	[ "$(manifest_value "$module_manifest" install_root)" = split-inputs/module-install ] || die 'source-built module install root is not canonical'
	module_install_root="$kernel_dist/split-inputs/module-install"
	require_dir "$module_install_root" 'source-built module install root'
	module_release_count=$(find "$module_install_root/lib/modules" -mindepth 1 -maxdepth 1 -type d -print 2>/dev/null | wc -l | tr -d '[:space:]')
	[ "$module_release_count" = 1 ] || die "source-built module install root must contain exactly one kernel release, found $module_release_count"
	kernel_release=$(find "$module_install_root/lib/modules" -mindepth 1 -maxdepth 1 -type d -print | sed 's#.*/##')
	case "$kernel_release" in ''|*[!A-Za-z0-9._+-]*) die "kernel release is malformed: $kernel_release" ;; esac
}

preflight() {
	check_host
	check_tools
	check_target_inputs
	check_handoffs
	printf '%s\n' 'PASS pixel-9a-boot-inputs preflight: exact source handoffs and target-owned boot inputs are ready'
}

tar_runtime_member_for() {
	runtime_path=$1
	usr_runtime_path=$2
	members=$(tar -tf "$rootfs_tar" | awk -v runtime_path="$runtime_path" -v usr_runtime_path="$usr_runtime_path" '
			{
				path=$0
				sub(/^\.\//, "", path)
				if (path == runtime_path || path == usr_runtime_path) print $0
			}
		')
	member_count=$(printf '%s\n' "$members" | awk 'NF { count++ } END { print count + 0 }')
	[ "$member_count" = 1 ] || die "runtime path '$runtime_path' resolved to $member_count archive members"
	printf '%s\n' "$members"
}

extract_regular_member() {
	member=$1
	destination=$2
	[ -n "$member" ] || die "rootfs.tar has no regular member named $(basename -- "$destination")"
	safe_relative "${member#./}"
	extract_root="$stage_dir/.rootfs-extract"
	rm -rf -- "$extract_root"
	mkdir -p "$extract_root"
	tar -xf "$rootfs_tar" -C "$extract_root" --no-same-owner --no-same-permissions "$member"
	source="$extract_root/${member#./}"
	[ -f "$source" ] || die "rootfs member is not a regular file: $member"
	[ ! -L "$source" ] || die "rootfs member is a symlink, not a regular file: $member"
	mkdir -p "$(dirname -- "$destination")"
	cp -L "$source" "$destination"
}

build_initramfs() {
	initramfs_root="$stage_dir/initramfs-root"
	mkdir -p "$initramfs_root/bin" "$initramfs_root/dev" "$initramfs_root/proc" "$initramfs_root/sys" "$initramfs_root/newroot"
	busybox_member=$(tar_runtime_member_for bin/busybox usr/bin/busybox)
	extract_regular_member "$busybox_member" "$initramfs_root/bin/busybox"
	chmod 0755 "$initramfs_root/bin/busybox"
	readelf -h "$initramfs_root/bin/busybox" | grep -Eiq 'AArch64|ARM aarch64' || die 'rootfs BusyBox is not an AArch64 executable'
	interpreter=$(readelf -l "$initramfs_root/bin/busybox" | sed -n 's/.*Requesting program interpreter: \([^]]*\)].*/\1/p' | head -1)
	if [ -n "$interpreter" ]; then
		[ "$interpreter" = /lib/ld-musl-aarch64.so.1 ] || die "BusyBox has an unreviewed runtime interpreter: $interpreter"
		interpreter_member=$(tar_runtime_member_for "${interpreter#/}" "usr/${interpreter#/}")
		extract_regular_member "$interpreter_member" "$initramfs_root/$interpreter"
		while IFS= read -r needed; do
			[ -n "$needed" ] || continue
			case "$needed" in
				*'/'*|.*) die "BusyBox has an unsafe runtime dependency: $needed" ;;
			esac
			case "$needed" in
				libc.musl-aarch64.so.1)
					# musl's loader is also libc. Alpine may archive this name
					# as a symlink; copy the already verified loader bytes.
					cp "$initramfs_root/$interpreter" "$initramfs_root/lib/$needed"
					;;
				*)
					needed_member=$(tar_runtime_member_for "lib/$needed" "usr/lib/$needed")
					extract_regular_member "$needed_member" "$initramfs_root/lib/$needed"
					;;
			esac
		done <<EOF
$(readelf -d "$initramfs_root/bin/busybox" | sed -n 's/.*Shared library: \[\([^]]*\)\].*/\1/p')
EOF
	fi
	cp "$init_script" "$initramfs_root/init"
	chmod 0755 "$initramfs_root/init"
	( cd "$initramfs_root" && find . -print | LC_ALL=C sort | cpio --quiet -o -H newc ) >"$stage_dir/initramfs.cpio"
}

module_row_for_entry() {
	entry=$1
	case "$entry" in
		''|'#'*) return 1 ;;
		*/*)
			case "$entry" in *.ko) wanted=$entry ;; *) wanted="$entry.ko" ;; esac
			matches=$(awk -F "$tab" -v wanted="$wanted" '$1 == "file" && $2 == wanted { print }' "$module_manifest")
			;;
		*)
			case "$entry" in *.ko) wanted=$entry ;; *) wanted="$entry.ko" ;; esac
			matches=$(awk -F "$tab" -v wanted="$wanted" '$1 == "file" && $2 == wanted { print }' "$module_manifest")
			if [ -z "$matches" ]; then
				matches=$(awk -F "$tab" -v wanted="$wanted" '$1 == "file" { path=$2; sub(/^.*\//, "", path); if (path == wanted) print }' "$module_manifest")
			fi
			;;
	esac
	match_count=$(printf '%s\n' "$matches" | awk 'NF { count++ } END { print count + 0 }')
	[ "$match_count" = 1 ] || die "module policy entry '$entry' resolves to $match_count source-built modules"
	module_resolved_row=$matches
}

verify_module_row() {
	row=$1
	module_relative=$(field "$row" 2)
	module_bytes=$(field "$row" 3)
	module_hash=$(field "$row" 4)
	safe_relative "$module_relative"
	case "$module_relative" in
		lib/modules/$kernel_release/*.ko) ;;
		*) die "module policy names a path outside kernel release $kernel_release: $module_relative" ;;
	esac
	case "$module_bytes" in ''|*[!0-9]*) die "module byte count is malformed: $module_relative" ;; esac
	case "$module_hash" in ''|*[!0-9a-f]*) die "module SHA-256 is malformed: $module_relative" ;; esac
	[ "${#module_hash}" -eq 64 ] || die "module SHA-256 length is malformed: $module_relative"
	module_source="$module_install_root/$module_relative"
	require_file "$module_source" 'source-built kernel module'
	[ "$(file_size "$module_source")" = "$module_bytes" ] || die "module size differs from source manifest: $module_relative"
	[ "$(sha256_of "$module_source")" = "$module_hash" ] || die "module hash differs from source manifest: $module_relative"
}

run_depmod() {
	root_dir=$1
	label=$2
	mkdir -p "$root_dir/lib/modules/$kernel_release"
	depmod -b "$root_dir" "$kernel_release" >"$stage_dir/depmod-$label.log" 2>&1 || die "depmod failed for source-built $label module root"
	[ -f "$root_dir/lib/modules/$kernel_release/modules.dep" ] || die "depmod did not produce modules.dep for $label"
}

copy_module_policy() {
	policy=$1
	root_dir=$2
	label=$3
	load_file="$root_dir/lib/modules/$kernel_release/modules.load"
	mkdir -p "$(dirname -- "$load_file")"
	: >"$load_file"
	while IFS= read -r entry; do
		case "$entry" in ''|'#'*) continue ;; esac
		module_row_for_entry "$entry"
		verify_module_row "$module_resolved_row"
		module_relative=$(field "$module_resolved_row" 2)
		case " $selected_modules " in *" $module_relative "*) die "source-built module is selected by more than one split policy: $module_relative" ;; esac
		selected_modules="$selected_modules $module_relative"
		short_relative=${module_relative#lib/modules/$kernel_release/}
		destination="$root_dir/lib/modules/$kernel_release/$short_relative"
		mkdir -p "$(dirname -- "$destination")"
		cp "$module_install_root/$module_relative" "$destination"
		printf '%s\n' "$short_relative" >>"$load_file"
	done <"$policy"
	run_depmod "$root_dir" "$label"
}

make_cpio() {
	source_dir=$1
	destination=$2
	( cd "$source_dir" && find . -print | LC_ALL=C sort | cpio --quiet -o -H newc ) >"$destination"
	[ -s "$destination" ] || die "source-owned CPIO is empty: $destination"
}

build_vendor_ramdisk() {
	ramdisk_root="$stage_dir/vendor-ramdisk-root"
	mkdir -p "$ramdisk_root"
	cp -R "$vendor_ramdisk_source"/. "$ramdisk_root"/
	make_cpio "$ramdisk_root" "$stage_dir/vendor-ramdisk.cpio"
	cp "$vendor_bootconfig_source" "$stage_dir/vendor-bootconfig"
}

write_tree_rows() (
	tree_name=$1
	tree_root=$2
	find "$tree_root" -type f -print | LC_ALL=C sort | while IFS= read -r tree_file; do
		relative=${tree_file#"$tree_root"/}
		safe_relative "$relative"
		require_file "$tree_file" 'boot-input tree file'
		printf 'tree_file\t%s\t%s\t%s\t%s\n' "$tree_name" "$relative" "$(file_size "$tree_file")" "$(sha256_of "$tree_file")"
	done
)

write_manifest() {
	manifest_tmp="$stage_dir/manifest.tsv.tmp"
	{
		printf 'schema_version\t1\n'
		printf 'target_id\tgoogle-pixel-9a-tegu-gxq96-mp1-0\n'
		printf 'route_id\texperimental-linux-google-tegu-s5300-wwan\n'
		printf 'kernel_backend\tlinux-google-tegu-wwan\n'
		printf 'status\tbuilt-source-inputs\n'
		printf 'bootable\tfalse\n'
		printf 'kernel_release\t%s\n' "$kernel_release"
		printf 'kernel_manifest\t%s\t%s\t%s\n' "$kernel_manifest" "$(file_size "$kernel_manifest")" "$(sha256_of "$kernel_manifest")"
		printf 'rootfs_manifest\t%s\t%s\t%s\n' "$rootfs_manifest" "$(file_size "$rootfs_manifest")" "$(sha256_of "$rootfs_manifest")"
		printf 'input\tinitramfs_source\t%s\t%s\t%s\n' "$init_script" "$(file_size "$init_script")" "$(sha256_of "$init_script")"
		printf 'input\tvendor_bootconfig_source\t%s\t%s\t%s\n' "$vendor_bootconfig_source" "$(file_size "$vendor_bootconfig_source")" "$(sha256_of "$vendor_bootconfig_source")"
		printf 'input\tvendor_ramdisk_source\t%s\n' "$vendor_ramdisk_source"
		printf 'input\tmodules_initfs_policy\t%s\t%s\t%s\n' "$initfs_list" "$(file_size "$initfs_list")" "$(sha256_of "$initfs_list")"
		printf 'input\tmodules_vendor_policy\t%s\t%s\t%s\n' "$vendor_list" "$(file_size "$vendor_list")" "$(sha256_of "$vendor_list")"
		printf 'input\tmodules_system_policy\t%s\t%s\t%s\n' "$system_list" "$(file_size "$system_list")" "$(sha256_of "$system_list")"
		printf 'artifact\tinitramfs.cpio\tinitramfs.cpio\t%s\t%s\tsource-built-initramfs\n' "$(file_size "$stage_dir/initramfs.cpio")" "$(sha256_of "$stage_dir/initramfs.cpio")"
		printf 'artifact\tvendor-ramdisk.cpio\tvendor-ramdisk.cpio\t%s\t%s\tsource-built-vendor-ramdisk\n' "$(file_size "$stage_dir/vendor-ramdisk.cpio")" "$(sha256_of "$stage_dir/vendor-ramdisk.cpio")"
		printf 'artifact\tvendor-kernel-boot.cpio\tvendor-kernel-boot.cpio\t%s\t%s\tsource-built-early-modules\n' "$(file_size "$stage_dir/vendor-kernel-boot.cpio")" "$(sha256_of "$stage_dir/vendor-kernel-boot.cpio")"
		printf 'artifact\tvendor-bootconfig\tvendor-bootconfig\t%s\t%s\tsource-owned-bootconfig\n' "$(file_size "$stage_dir/vendor-bootconfig")" "$(sha256_of "$stage_dir/vendor-bootconfig")"
		printf 'module_root\tvendor_dlkm\tvendor-dlkm\t%s\t%s\n' "$(sha256_of "$vendor_list")" "$(file_size "$stage_dir/vendor-dlkm/lib/modules/$kernel_release/modules.dep")"
		printf 'module_root\tsystem_dlkm\tsystem-dlkm\t%s\t%s\n' "$(sha256_of "$system_list")" "$(file_size "$stage_dir/system-dlkm/lib/modules/$kernel_release/modules.dep")"
		write_tree_rows vendor_ramdisk_source "$vendor_ramdisk_source"
		write_tree_rows vendor_dlkm "$stage_dir/vendor-dlkm"
		write_tree_rows system_dlkm "$stage_dir/system-dlkm"
	} >"$manifest_tmp"
	mv "$manifest_tmp" "$stage_dir/manifest.tsv"
}

build() {
	preflight >/dev/null
	[ "${FERRIX_PIXEL9A_BOOT_INPUTS_BUILD:-}" = "$build_confirmation" ] || die "boot-input composition is opt-in; set FERRIX_PIXEL9A_BOOT_INPUTS_BUILD=$build_confirmation"
	case "$inputs_out" in "$out_root"/*) ;; *) die "boot-input output must remain below the ignored output root: $out_root" ;; esac
	[ ! -e "$inputs_out" ] || die "refusing to overwrite existing boot-input output; choose a fresh FERRIX_PIXEL9A_BOOT_INPUTS_OUT: $inputs_out"
	[ ! -e "$inputs_manifest" ] || die "refusing to overwrite existing boot-input manifest: $inputs_manifest"
	stage_parent=$(dirname -- "$inputs_out")
	mkdir -p "$stage_parent"
	stage_dir=$(mktemp -d "$stage_parent/.tegu-boot-inputs.XXXXXX")
	trap 'rm -rf -- "$stage_dir"' EXIT HUP INT TERM
	selected_modules=
	mkdir -p "$stage_dir/vendor-dlkm" "$stage_dir/system-dlkm"
	build_initramfs
	copy_module_policy "$initfs_list" "$stage_dir/vendor-kernel-root" initfs
	make_cpio "$stage_dir/vendor-kernel-root" "$stage_dir/vendor-kernel-boot.cpio"
	copy_module_policy "$vendor_list" "$stage_dir/vendor-dlkm" vendor_dlkm
	copy_module_policy "$system_list" "$stage_dir/system-dlkm" system_dlkm
	build_vendor_ramdisk
	write_manifest
	mv "$stage_dir" "$inputs_out"
	if [ "$inputs_manifest" != "$inputs_out/manifest.tsv" ]; then
		mkdir -p "$(dirname -- "$inputs_manifest")"
		cp "$inputs_out/manifest.tsv" "$inputs_manifest"
	fi
	trap - EXIT HUP INT TERM
	printf '%s\n' "PASS pixel-9a-boot-inputs built source-owned inputs: $inputs_out"
	printf '%s\n' 'NOTE: inputs are hash-recorded, unbooted, and bootable=false; no device operation was performed'
}

usage() {
	printf '%s\n' 'usage: tools/pixel-9a/boot-inputs.sh preflight|build' >&2
	exit 2
}

case "${1:-}" in
	preflight) [ "$#" -eq 1 ] || usage; preflight ;;
	build) [ "$#" -eq 1 ] || usage; build ;;
	*) usage ;;
esac
