#!/bin/sh
set -eu

# Device-free diagnostic only: test a stock MP overlay against a source-built
# base DTS with symbols. The stock overlay must never enter a Ferrix image.
root=$(CDPATH= cd -P -- "$(dirname -- "$0")/../.." && pwd)
mode=symbolize
if [ "${1:-}" = --base-dtb ]; then mode=base-dtb; shift; fi
[ "$#" -eq 2 ] || { printf 'usage: tools/pixel-9a/dt-symbol-probe.sh [--base-dtb] out/pixel-9a/BASE.dts.tmp|BASE.dtb out/pixel-9a/MP.dtbo\n' >&2; exit 2; }
check_input() {
	case "$1" in
		"$root/out/pixel-9a/"*) path=$1 ;;
		out/pixel-9a/*) path="$root/$1" ;;
		*) printf 'FAIL pixel-9a-dt-symbol-probe: input must be under out/pixel-9a/\n' >&2; exit 1 ;;
	esac
	case "$path" in *'/../'*|*'/./'*|*'/..'|*'/.' ) printf 'FAIL pixel-9a-dt-symbol-probe: unsafe path\n' >&2; exit 1 ;; esac
	[ -f "$path" ] && [ ! -L "$path" ] || { printf 'FAIL pixel-9a-dt-symbol-probe: input missing or symlink\n' >&2; exit 1; }
	physical="$(CDPATH= cd -P -- "$(dirname -- "$path")" && pwd)/$(basename -- "$path")"
	case "$physical" in
		"$root/out/pixel-9a/"*) ;;
		*) printf 'FAIL pixel-9a-dt-symbol-probe: input resolves outside out/pixel-9a/\n' >&2; exit 1 ;;
	esac
	printf '%s\n' "$physical"
}
base=$(check_input "$1")
overlay=$(check_input "$2")
for tool in dtc fdtoverlay fdtget mktemp; do
	command -v "$tool" >/dev/null 2>&1 || { printf 'FAIL pixel-9a-dt-symbol-probe: missing %s\n' "$tool" >&2; exit 1; }
done
if command -v sha256sum >/dev/null 2>&1; then
	hash_file() { sha256sum "$1" | awk '{ print $1 }'; }
elif command -v shasum >/dev/null 2>&1; then
	hash_file() { shasum -a 256 "$1" | awk '{ print $1 }'; }
else
	printf 'FAIL pixel-9a-dt-symbol-probe: sha256sum or shasum is required\n' >&2
	exit 1
fi
base_hash=$(hash_file "$base")
overlay_hash=$(hash_file "$overlay")
[ "$overlay_hash" = 634753b139e7edf5a986314be9212ca3e92c710bfc177263252adbf4fcfdb7c5 ] || {
	printf 'FAIL pixel-9a-dt-symbol-probe: overlay differs from verified pinned stock MP1.0 diagnostic\n' >&2
	exit 1
}
probe=$(mktemp -d "$root/out/pixel-9a/dt-symbol-probe.XXXXXX")
printf 'probe=%s\n' "${probe#"$root/"}"
printf 'base_input=%s sha256=%s\n' "${base#"$root/"}" "$base_hash"
printf 'overlay_input=%s sha256=%s\n' "${overlay#"$root/"}" "$overlay_hash"
if [ "$mode" = base-dtb ]; then
	cp "$base" "$probe/base-symbols.dtb"
else
	if ! dtc -@ -I dts -O dtb -o "$probe/base-symbols.dtb" "$base" >"$probe/compile.log" 2>&1; then
		printf 'result=base-compile-failed log=%s\n' "${probe#"$root/"}/compile.log"
		exit 1
	fi
fi
symbols=$(fdtget -p "$probe/base-symbols.dtb" /__symbols__ | wc -l | tr -d '[:space:]')
printf 'base_symbol_count=%s\n' "$symbols"
if ! fdtoverlay -i "$probe/base-symbols.dtb" -o "$probe/mp-applied.dtb" "$overlay" >"$probe/apply.log" 2>&1; then
	printf 'result=overlay-apply-failed log=%s\n' "${probe#"$root/"}/apply.log"
	exit 1
fi
printf 'result=overlay-applied-offline output=%s\n' "${probe#"$root/"}/mp-applied.dtb"
