#!/bin/sh
# Re-derives the vendored Material Symbols subset from the commit pinned in
# tools/icons/material-symbols.pins.
#
#   sh tools/icons/material-symbols-fetch.sh            # fetch: rewrite the
#                                                       # vendored files and
#                                                       # SHA256SUMS
#   sh tools/icons/material-symbols-fetch.sh --verify   # verify: download into a
#                                                       # temporary directory and
#                                                       # compare byte-for-byte
#
# Fetch mode replaces crates/ferrix-ui/assets/icons/material-symbols/*.svg,
# LICENSE and SHA256SUMS; review the diff before committing. Verify mode
# changes nothing and exits 1 with a typed `material-symbols-rejected` record
# on any difference (missing, extra, or changed file). Both modes refuse an
# icon that has no FILL 1 file at the pin rather than falling back silently.
# Host tooling only (curl, shasum); nothing is installed. The build itself
# never touches the network: build.rs only checks the vendored hashes.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
pins="$here/material-symbols.pins"
dest="$root/crates/ferrix-ui/assets/icons/material-symbols"
mode=fetch

case ${1:-} in
    "") ;;
    --verify) mode=verify ;;
    -h|--help) sed -n '2,18p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "usage: $0 [--verify]" >&2; exit 2 ;;
esac

reject() {
    printf 'material-symbols-rejected reason=%s' "$1"
    shift
    while [ "$#" -ge 2 ]; do printf ' %s=%s' "$1" "$2"; shift 2; done
    printf '\n'
    exit 1
}

for tool in curl shasum; do
    command -v "$tool" >/dev/null 2>&1 || reject "missing-tool" tool "$tool"
done

pin() { sed -n "s/^$1=//p" "$pins" | head -n 1; }
commit=$(pin MATERIAL_SYMBOLS_COMMIT)
family=$(pin MATERIAL_SYMBOLS_FAMILY)
variant=$(pin MATERIAL_SYMBOLS_VARIANT)
icons=$(sed -n 's/^ICON=//p' "$pins")
printf '%s\n' "$commit" | grep -Eq '^[0-9a-f]{40}$' || reject "bad-pin" key MATERIAL_SYMBOLS_COMMIT
[ -n "$family" ] && [ -n "$variant" ] && [ -n "$icons" ] || reject "bad-pin" file "$pins"

base="https://raw.githubusercontent.com/google/material-design-icons/$commit"
work=$(mktemp -d "${TMPDIR:-/tmp}/ferrix-material-symbols.XXXXXX")
trap 'rm -rf "$work" "$work.sums"' EXIT INT TERM

get() {
    curl -fsSL --retry 2 -o "$2" "$1" || reject "download-failed" url "$1"
}

get "$base/LICENSE" "$work/LICENSE"
grep -q "Apache License" "$work/LICENSE" && grep -q "Version 2.0, January 2004" "$work/LICENSE" \
    || reject "licence-not-apache-2.0" commit "$commit"
for name in $icons; do
    get "$base/symbols/web/$name/$family/${name}_$variant.svg" "$work/$name.svg"
done
# One line per file, sorted by name, in `shasum -a 256` format; build.rs
# parses exactly this.
(cd "$work" && for f in $(ls | LC_ALL=C sort); do shasum -a 256 "$f"; done) > "$work.sums"
mv "$work.sums" "$work/SHA256SUMS"

if [ "$mode" = verify ]; then
    if ! diff -u "$dest/SHA256SUMS" "$work/SHA256SUMS" >&2; then
        reject "hash-mismatch" commit "$commit" vendored "$dest/SHA256SUMS"
    fi
    for f in "$work"/*; do
        cmp -s "$f" "$dest/$(basename "$f")" || reject "content-mismatch" file "$(basename "$f")"
    done
    for f in "$dest"/*; do
        [ -e "$work/$(basename "$f")" ] || [ "$(basename "$f")" = README.md ] \
            || reject "unpinned-file" file "$(basename "$f")"
    done
    count=$(printf '%s\n' $icons | wc -l | tr -d ' ')
    echo "material-symbols-verified commit=$commit family=$family variant=$variant icons=$count"
    exit 0
fi

mkdir -p "$dest"
find "$dest" -maxdepth 1 -type f ! -name README.md -exec rm -f {} +
cp "$work"/* "$dest/"
count=$(printf '%s\n' $icons | wc -l | tr -d ' ')
echo "material-symbols-fetched commit=$commit family=$family variant=$variant icons=$count dest=$dest"
