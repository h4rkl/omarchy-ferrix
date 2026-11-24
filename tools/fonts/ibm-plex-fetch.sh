#!/bin/sh
# Re-derives the vendored IBM Plex faces from the commit pinned in
# tools/fonts/ibm-plex.pins.
#
#   sh tools/fonts/ibm-plex-fetch.sh            # fetch: rewrite the vendored
#                                               # faces, LICENSE.txt and
#                                               # SHA256SUMS
#   sh tools/fonts/ibm-plex-fetch.sh --verify   # verify: download into a
#                                               # temporary directory and
#                                               # compare byte-for-byte
#
# Fetch mode replaces crates/ferrix-ui/assets/fonts/ibm-plex/*.ttf,
# LICENSE.txt and SHA256SUMS (README.md is kept); review the diff before
# committing. Verify mode changes nothing and exits 1 with a typed
# `ibm-plex-rejected` record on any difference (missing, extra, or changed
# file), or when the upstream licence is not the SIL OFL 1.1. Host tooling
# only (curl, shasum); nothing is installed. The build itself never touches
# the network: build.rs only checks the vendored hashes.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
pins="$here/ibm-plex.pins"
dest="$root/crates/ferrix-ui/assets/fonts/ibm-plex"
mode=fetch

case ${1:-} in
    "") ;;
    --verify) mode=verify ;;
    -h|--help) sed -n '2,18p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "usage: $0 [--verify]" >&2; exit 2 ;;
esac

reject() {
    printf 'ibm-plex-rejected reason=%s' "$1"
    shift
    while [ "$#" -ge 2 ]; do printf ' %s=%s' "$1" "$2"; shift 2; done
    printf '\n'
    exit 1
}

for tool in curl shasum; do
    command -v "$tool" >/dev/null 2>&1 || reject "missing-tool" tool "$tool"
done

pin() { sed -n "s/^$1=//p" "$pins" | head -n 1; }
commit=$(pin IBM_PLEX_COMMIT)
licence=$(pin IBM_PLEX_LICENSE)
fonts=$(sed -n 's/^FONT=//p' "$pins")
printf '%s\n' "$commit" | grep -Eq '^[0-9a-f]{40}$' || reject "bad-pin" key IBM_PLEX_COMMIT
[ -n "$licence" ] && [ -n "$fonts" ] || reject "bad-pin" file "$pins"

base="https://raw.githubusercontent.com/IBM/plex/$commit"
work=$(mktemp -d "${TMPDIR:-/tmp}/ferrix-ibm-plex.XXXXXX")
trap 'rm -rf "$work" "$work.sums"' EXIT INT TERM

get() {
    curl -fsSL --retry 2 -o "$2" "$1" || reject "download-failed" url "$1"
}

get "$base/$licence" "$work/LICENSE.txt"
grep -q "SIL OPEN FONT LICENSE Version 1.1" "$work/LICENSE.txt" \
    || reject "licence-not-ofl-1.1" commit "$commit"
for path in $fonts; do
    get "$base/$path" "$work/$(basename "$path")"
done
# One line per file, sorted by name, in `shasum -a 256` format; build.rs
# parses exactly this.
(cd "$work" && for f in $(ls | LC_ALL=C sort); do shasum -a 256 "$f"; done) > "$work.sums"
mv "$work.sums" "$work/SHA256SUMS"

count=$(printf '%s\n' $fonts | wc -l | tr -d ' ')
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
    echo "ibm-plex-verified commit=$commit faces=$count"
    exit 0
fi

mkdir -p "$dest"
find "$dest" -maxdepth 1 -type f ! -name README.md -exec rm -f {} +
cp "$work"/* "$dest/"
echo "ibm-plex-fetched commit=$commit faces=$count dest=$dest"
