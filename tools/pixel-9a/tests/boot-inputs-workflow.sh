#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
script="$root/tools/pixel-9a/boot-inputs.sh"
target="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0"
work=$(mktemp -d "${TMPDIR:-/tmp}/ferrix-pixel9a-boot-inputs.XXXXXX")
trap 'rm -rf "$work"' EXIT HUP INT TERM
tab=$(printf '\t')

fail() {
	printf 'boot inputs workflow test failed: %s\n' "$*" >&2
	exit 1
}

hash_file() {
	if command -v sha256sum >/dev/null 2>&1; then
		sha256sum "$1" | awk '{ print $1 }'
	else
		shasum -a 256 "$1" | awk '{ print $1 }'
	fi
}

bytes() { wc -c <"$1" | tr -d '[:space:]'; }

[ -x "$script" ] || fail 'boot-input workflow is not executable'
sh -n "$script" || fail 'shell syntax'
if grep -nE '(^|[^[:alnum:]_-])(adb|fastboot)([^[:alnum:]_-]|$)' "$script" >/dev/null; then
	fail 'device transport token'
fi
grep -q 'source-owned' "$script" || fail 'source-owned handoff marker missing'
grep -q 'bootable=false' "$script" || fail 'unbooted boundary marker missing'

mkdir -p "$work/bin" \
	"$work/out/pixel-9a/kernel-radio/tegu-wwan/dist/split-inputs/module-install/lib/modules/6.12.0-ferrix" \
	"$work/out/pixel-9a/rootfs/tegu" \
	"$work/rootfs-tree/usr/bin" "$work/rootfs-tree/usr/lib" \
	"$work/rootfs-tree/etc/busybox-paths.d"

cat >"$work/bin/uname" <<'EOF'
#!/bin/sh
case "${1:-}" in
	-m) printf '%s\n' x86_64 ;;
	*) printf '%s\n' Linux ;;
esac
EOF
cat >"$work/bin/readelf" <<'EOF'
#!/bin/sh
set -eu
case "${1:-}" in
	-h)
		grep -Fqx source-built-busybox "$2" || exit 1
		printf '%s\n' 'Class: ELF64' 'Machine: AArch64'
		;;
	-l) printf '%s\n' 'Requesting program interpreter: /lib/ld-musl-aarch64.so.1]' ;;
	-d) printf '%s\n' 'Shared library: [libc.musl-aarch64.so.1]' ;;
	*) exit 2 ;;
esac
EOF
cat >"$work/bin/cpio" <<'EOF'
#!/bin/sh
set -eu
printf '%s\n' synthetic-cpio
EOF
cat >"$work/bin/depmod" <<'EOF'
#!/bin/sh
set -eu
root=
release=
while [ "$#" -gt 0 ]; do
	case "$1" in
		-b) root=$2; shift 2 ;;
		-*) shift ;;
		*) release=$1; shift ;;
	esac
done
[ -n "$root" ] && [ -n "$release" ]
mkdir -p "$root/lib/modules/$release"
printf '%s\n' '# synthetic depmod output' >"$root/lib/modules/$release/modules.dep"
EOF
chmod 0755 "$work/bin/uname" "$work/bin/readelf" "$work/bin/cpio" "$work/bin/depmod"

printf '%s\n' source-built-busybox >"$work/rootfs-tree/usr/bin/busybox"
printf '%s\n' source-built-loader >"$work/rootfs-tree/usr/lib/ld-musl-aarch64.so.1"
ln -s ld-musl-aarch64.so.1 "$work/rootfs-tree/usr/lib/libc.musl-aarch64.so.1"
ln -s usr/bin "$work/rootfs-tree/bin"
ln -s usr/lib "$work/rootfs-tree/lib"
# The real pmOS archive lists this package metadata before the executable.
# A basename-only lookup must not mistake either decoy for a runtime file.
printf '%s\n' package-path-list-not-elf >"$work/rootfs-tree/etc/busybox-paths.d/busybox"
printf '%s\n' not-the-runtime-loader >"$work/rootfs-tree/etc/ld-musl-aarch64.so.1"
system_tar=$(command -v tar)
"$system_tar" -cf "$work/out/pixel-9a/rootfs/tegu/rootfs.tar" -C "$work/rootfs-tree" ./etc ./usr ./bin ./lib
printf '%s\n' synthetic-rootfs-image >"$work/out/pixel-9a/rootfs/tegu/rootfs.img"

kernel_dist="$work/out/pixel-9a/kernel-radio/tegu-wwan/dist"
kernel_manifest="$work/out/pixel-9a/kernel-radio/tegu-wwan/build-manifest.tsv"
module_manifest="$kernel_dist/split-inputs/module-install.tsv"
printf 'schema_version\t1\ninstall_root\tsplit-inputs/module-install\n' >"$module_manifest"
{
	printf 'schema_version\t1\n'
	printf 'target_id\tgoogle-pixel-9a-tegu-gxq96-mp1-0\n'
	printf 'route_id\texperimental-linux-google-tegu-s5300-wwan\n'
	printf 'kernel_backend\tlinux-google-tegu-wwan\n'
	printf 'status\tbuilt-not-booted\n'
	printf 'bootable\tfalse\n'
	printf 'module_install\tsplit-inputs/module-install.tsv\t%s\t%s\n' "$(bytes "$module_manifest")" "$(hash_file "$module_manifest")"
} >"$kernel_manifest"

rootfs_out="$work/out/pixel-9a/rootfs/tegu"
rootfs_manifest="$rootfs_out/manifest.tsv"
write_rootfs_manifest() {
	{
		printf 'schema_version\t1\n'
		printf 'target_id\tgoogle-pixel-9a-tegu-gxq96-mp1-0\n'
		printf 'kernel_backend\tlinux-google-tegu-wwan\n'
		printf 'status\tbuilt-substrate-only\n'
		printf 'bootable\tfalse\n'
		printf 'artifact\trootfs.tar\t%s\t%s\n' "$(bytes "$rootfs_out/rootfs.tar")" "$(hash_file "$rootfs_out/rootfs.tar")"
		printf 'artifact\trootfs.img\t%s\t%s\n' "$(bytes "$rootfs_out/rootfs.img")" "$(hash_file "$rootfs_out/rootfs.img")"
	} >"$rootfs_manifest"
}
write_rootfs_manifest

base_env="PATH=$work/bin:$PATH FERRIX_OUT_DIR=$work/out FERRIX_PIXEL9A_RADIO_KERNEL_DIST=$kernel_dist FERRIX_PIXEL9A_RADIO_KERNEL_MANIFEST=$kernel_manifest FERRIX_PIXEL9A_ROOTFS_OUT=$rootfs_out FERRIX_PIXEL9A_ROOTFS_MANIFEST=$rootfs_manifest FERRIX_PIXEL9A_BOOT_INPUTS_OUT=$work/out/pixel-9a/boot-inputs/tegu-wwan"
if env $base_env "$script" preflight >"$work/preflight.out" 2>"$work/preflight.err"; then
	:
else
	fail "preflight unexpectedly refused: $(sed -n '1p' "$work/preflight.err")"
fi
grep -q 'PASS pixel-9a-boot-inputs preflight' "$work/preflight.out" || fail 'preflight did not report success'

cp "$module_manifest" "$work/module-install.original.tsv"
printf 'tampered\n' >>"$module_manifest"
if env $base_env "$script" preflight >"$work/tampered.out" 2>"$work/tampered.err"; then
	fail 'accepted a module manifest that differs from the kernel handoff'
fi
grep -q 'module manifest size differs from kernel handoff' "$work/tampered.err" || fail 'module provenance refusal missing'
cp "$work/module-install.original.tsv" "$module_manifest"

if env $base_env "$script" build >"$work/no-token.out" 2>"$work/no-token.err"; then
	fail 'build ran without explicit confirmation'
fi
grep -q 'boot-input composition is opt-in' "$work/no-token.err" || fail 'missing opt-in refusal'

env $base_env FERRIX_PIXEL9A_BOOT_INPUTS_BUILD=I-understand-this-builds-source-owned-pixel-9a-boot-inputs \
	"$script" build >"$work/build.out"

inputs="$work/out/pixel-9a/boot-inputs/tegu-wwan"
[ -f "$inputs/manifest.tsv" ] || fail 'boot-input manifest missing'
cmp "$inputs/initramfs-root/bin/busybox" "$work/rootfs-tree/usr/bin/busybox" || fail 'package metadata was selected instead of runtime BusyBox'
cmp "$inputs/initramfs-root/lib/ld-musl-aarch64.so.1" "$work/rootfs-tree/usr/lib/ld-musl-aarch64.so.1" || fail 'non-runtime loader was selected'
cmp "$inputs/initramfs-root/lib/libc.musl-aarch64.so.1" "$work/rootfs-tree/usr/lib/ld-musl-aarch64.so.1" || fail 'musl libc alias does not contain loader bytes'
for artifact in initramfs.cpio vendor-ramdisk.cpio vendor-kernel-boot.cpio vendor-bootconfig; do
	[ -s "$inputs/$artifact" ] || fail "boot-input artifact missing: $artifact"
	awk -F "$tab" -v name="$artifact" '$1 == "artifact" && $2 == name && $6 != "" { found=1 } END { exit(found ? 0 : 1) }' "$inputs/manifest.tsv" ||
		fail "boot-input provenance row missing: $artifact"
done
for name in vendor-dlkm system-dlkm; do
	[ -f "$inputs/$name/lib/modules/6.12.0-ferrix/modules.dep" ] || fail "depmod output missing: $name"
done
awk -F "$tab" '$1 == "status" && $2 == "built-source-inputs" { found=1 } END { exit(found ? 0 : 1) }' "$inputs/manifest.tsv" || fail 'source-input status missing'
awk -F "$tab" '$1 == "bootable" && $2 == "false" { found=1 } END { exit(found ? 0 : 1) }' "$inputs/manifest.tsv" || fail 'bootable boundary missing'

expect_archive_refusal() {
	label=$1
	expected=$2
	refused_out="$work/out/pixel-9a/boot-inputs/$label"
	write_rootfs_manifest
	if env $base_env FERRIX_PIXEL9A_BOOT_INPUTS_OUT="$refused_out" \
		FERRIX_PIXEL9A_BOOT_INPUTS_BUILD=I-understand-this-builds-source-owned-pixel-9a-boot-inputs \
		"$script" build >"$work/$label.out" 2>"$work/$label.err"; then
		fail "accepted $label rootfs archive"
	fi
	grep -Fq "$expected" "$work/$label.err" || fail "unexpected $label refusal: $(sed -n '1p' "$work/$label.err")"
	[ ! -e "$refused_out" ] || fail "$label refusal published output"
}

# Duplicate records are ambiguous even when they name the same path.
"$system_tar" -rf "$rootfs_out/rootfs.tar" -C "$work/rootfs-tree" ./usr/bin/busybox
expect_archive_refusal duplicate-runtime 'resolved to 2 archive members'

# A metadata-only archive must not fall back to a matching basename in /etc.
"$system_tar" -cf "$rootfs_out/rootfs.tar" -C "$work/rootfs-tree" ./etc ./usr/lib
expect_archive_refusal missing-runtime 'resolved to 0 archive members'

printf '%s\n' wrong-runtime-content >"$work/rootfs-tree/usr/bin/busybox"
"$system_tar" -cf "$rootfs_out/rootfs.tar" -C "$work/rootfs-tree" ./etc ./usr ./bin ./lib
expect_archive_refusal invalid-elf 'rootfs BusyBox is not an AArch64 executable'

printf '%s\n' 'pixel-9a boot-input workflow tests passed'
