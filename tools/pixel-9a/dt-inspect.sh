#!/bin/sh
set -eu

# Read-only Pixel 9a device-tree comparison. Inputs remain under ignored out/.
root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
mode=summary
if [ "${1:-}" = --fixups ]; then mode=fixups; shift; fi
[ "$#" -eq 1 ] || { printf 'usage: tools/pixel-9a/dt-inspect.sh [--fixups] out/pixel-9a/FILE.dtb\n' >&2; exit 2; }
case "$1" in
	"$root/out/pixel-9a/"*) file=$1 ;;
	out/pixel-9a/*) file="$root/$1" ;;
	*) printf 'FAIL pixel-9a-dt-inspect: input must be under out/pixel-9a/\n' >&2; exit 1 ;;
esac
case "$file" in *'/../'*|*'/./'*|*'/..'|*'/.' ) printf 'FAIL pixel-9a-dt-inspect: unsafe input path\n' >&2; exit 1 ;; esac
[ -f "$file" ] && [ ! -L "$file" ] || { printf 'FAIL pixel-9a-dt-inspect: input is missing or a symlink\n' >&2; exit 1; }
command -v fdtget >/dev/null 2>&1 || { printf 'FAIL pixel-9a-dt-inspect: fdtget is required\n' >&2; exit 1; }
fdtget -p "$file" / >/dev/null 2>&1 || { printf 'FAIL pixel-9a-dt-inspect: input is not a readable device tree\n' >&2; exit 1; }
if [ "$mode" = fixups ]; then
	fdtget -p "$file" /__fixups__
	exit 0
fi
if command -v sha256sum >/dev/null 2>&1; then
	hash=$(sha256sum "$file" | awk '{ print $1 }')
elif command -v shasum >/dev/null 2>&1; then
	hash=$(shasum -a 256 "$file" | awk '{ print $1 }')
else
	printf 'FAIL pixel-9a-dt-inspect: sha256sum or shasum is required\n' >&2
	exit 1
fi
printf 'input=%s\n' "${file#"$root/"}"
printf 'sha256=%s\n' "$hash"
for property in model compatible; do
	value=$(fdtget -t s "$file" / "$property" 2>/dev/null || true)
	[ -n "$value" ] || continue
	printf '%s=%s\n' "$property" "$value"
done
for property in board_id board_rev google,chipid; do
	value=$(fdtget -t x "$file" / "$property" 2>/dev/null || true)
	[ -n "$value" ] || continue
	printf '%s=%s\n' "$property" "$value"
done
for property in model compatible; do
	value=$(fdtget -t s "$file" '/fragment@boardbase/__overlay__' "$property" 2>/dev/null || true)
	[ -n "$value" ] || continue
	printf 'mp_overlay_%s=%s\n' "$property" "$value"
done
if fdtget -l "$file" / 2>/dev/null | grep -Fqx '__symbols__'; then
	printf 'symbols=present\n'
	printf 'symbol_count=%s\n' "$(fdtget -p "$file" /__symbols__ | wc -l | tr -d '[:space:]')"
else
	printf 'symbols=absent\n'
fi
if fdtget -l "$file" / 2>/dev/null | grep -Fqx '__fixups__'; then
	printf 'external_fixup_count=%s\n' "$(fdtget -p "$file" /__fixups__ | wc -l | tr -d '[:space:]')"
fi
