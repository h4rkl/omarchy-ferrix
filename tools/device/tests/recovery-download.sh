#!/bin/sh
# Offline proof for the generic pinned downloader. A tiny file is served from
# disk through a file:// URL, so the test never touches the network, a vendor
# server, or a device. It also checks the checked-in Pixel 9a pins parse.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
download="$root/tools/device/recovery-download.sh"
fails=0
# shellcheck disable=SC1091
. "$here/lib.sh"

work=$(mktemp -d "${TMPDIR:-/tmp}/device-recovery-test.XXXXXX")
trap 'rm -rf "$work"' EXIT
boards="$work/boards"
mkdir -p "$boards"
copy_board_metadata "$root/targets/boards" "$boards"
export FERRIX_BOARDS_DIR="$boards" FERRIX_RECOVERY_ALLOW_FILE_URL=1
opt_in="I-understand-this-downloads-vendor-firmware"

sha256_of() {
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{ print $1 }'
  else
    sha256sum "$1" | awk '{ print $1 }'
  fi
}

check "static: downloader never invokes adb/fastboot" sh -c "! grep -nE '^[[:space:]]*[^#]*(adb|fastboot)([[:space:]]|$)' '$download'"
check "static: downloader never extracts archives" sh -c "! grep -nE '^[[:space:]]*[^#]*(unzip|tar |bsdtar|7z)' '$download'"

# Synthetic target with a tiny pinned file served from disk.
tid=oneplus-syn2001-synthetic-op-sku7-evt2
tdir="$boards/$tid"
write_manifest "$tdir" OnePlus SYN2001 synthetic-op SKU7 EVT2
mkdir -p "$work/serve"
printf 'synthetic factory payload\n' > "$work/serve/factory.zip"
printf 'synthetic ota payload, longer\n' > "$work/serve/ota.zip"
f_hash=$(sha256_of "$work/serve/factory.zip")
f_bytes=$(wc -c < "$work/serve/factory.zip" | tr -d '[:space:]')
o_hash=$(sha256_of "$work/serve/ota.zip")
o_bytes=$(wc -c < "$work/serve/ota.zip" | tr -d '[:space:]')
tab=$(printf '\t')
{
  printf '# kind<TAB>build<TAB>version<TAB>filename<TAB>url<TAB>sha256<TAB>bytes<TAB>source-page\n'
  printf 'factory%sSYN.001%s14%ssyn-factory.zip%sfile://%s/serve/factory.zip%s%s%s%s%shttps://example.invalid/images\n' "$tab" "$tab" "$tab" "$tab" "$work" "$tab" "$f_hash" "$tab" "$f_bytes" "$tab"
  printf 'ota%sSYN.001%s14%ssyn-ota.zip%sfile://%s/serve/ota.zip%s%s%s%s%shttps://example.invalid/ota\n' "$tab" "$tab" "$tab" "$tab" "$work" "$tab" "$o_hash" "$tab" "$o_bytes" "$tab"
} > "$tdir/recovery.pins"

run_dl() {
  status=0
  sh "$download" "$@" > "$work/stdout" 2> "$work/stderr" || status=$?
}

run_dl --target "$tid"
check "opt-in: refuses without FERRIX_RECOVERY_DOWNLOAD" [ "$status" -eq 1 ]
check "opt-in: nothing downloaded without opt-in" [ ! -e "$tdir/recovery/syn-factory.zip" ]
export FERRIX_RECOVERY_DOWNLOAD="$opt_in"

run_dl
check "usage: missing --target exits 2" [ "$status" -eq 2 ]
run_dl --target "../$tid"
check "refusal: path-like target id refused" [ "$status" -eq 1 ]
run_dl --target no-such-target
check "refusal: unknown target refused" [ "$status" -eq 1 ]
run_dl --target "$tid" --kind weird
check "refusal: unknown kind refused" [ "$status" -eq 1 ]

run_dl --target "$tid" --kind factory
check "download: factory kind succeeds" [ "$status" -eq 0 ]
check "download: factory file present and verified" cmp -s "$tdir/recovery/syn-factory.zip" "$work/serve/factory.zip"
check "download: ota not fetched for --kind factory" [ ! -e "$tdir/recovery/syn-ota.zip" ]
check "download: no .part left behind" sh -c "! ls '$tdir/recovery'/*.part 2>/dev/null | grep -q ."
check "manifest: written" [ -f "$tdir/recovery/download-manifest.txt" ]
check "manifest: lists factory row with hash" grep -q "^factory	syn-factory.zip	$f_bytes	$f_hash\$" "$tdir/recovery/download-manifest.txt"

run_dl --target "$tid"
check "download: all kinds succeed" [ "$status" -eq 0 ]
check "download: already-verified factory is skipped" grep -q '^verified existing .*syn-factory.zip' "$work/stdout"
check "download: ota fetched" cmp -s "$tdir/recovery/syn-ota.zip" "$work/serve/ota.zip"
check "manifest: lists both rows" sh -c "[ \"\$(grep -vc '^#' '$tdir/recovery/download-manifest.txt')\" -eq 2 ]"

# Corrupt the served OTA so hash/size mismatch is detected and the file removed.
rm -f "$tdir/recovery/syn-ota.zip"
printf 'tampered payload with the same-ish length\n' > "$work/serve/ota.zip"
run_dl --target "$tid" --kind ota
check "verify: mismatching download is refused" [ "$status" -eq 1 ]
check "verify: mismatching file deleted" [ ! -e "$tdir/recovery/syn-ota.zip" ]
check "verify: mismatching .part deleted" [ ! -e "$tdir/recovery/syn-ota.zip.part" ]
check "verify: mismatch reported" grep -Eq 'mismatch' "$work/stderr"

# Same size but different bytes: SHA-256 must still catch it.
printf 'synthetic ota payload, LONGER\n' > "$work/serve/ota.zip"
run_dl --target "$tid" --kind ota
check "verify: same-size hash mismatch refused" [ "$status" -eq 1 ]
check "verify: same-size hash mismatch reported as SHA-256" grep -q 'SHA-256 mismatch' "$work/stderr"

# Existing corrupted final file is replaced only after re-verification.
printf 'synthetic ota payload, longer\n' > "$work/serve/ota.zip"
printf 'stale\n' > "$tdir/recovery/syn-ota.zip"
run_dl --target "$tid" --kind ota
check "verify: stale existing file is replaced" cmp -s "$tdir/recovery/syn-ota.zip" "$work/serve/ota.zip"

# A generated recovery path must never follow a pre-existing symlink, even if
# its target happens to contain the expected bytes. The partial path is equally
# sensitive because curl writes there before the final hash check.
rm -f "$tdir/recovery/syn-ota.zip"
ln -s "$work/serve/ota.zip" "$tdir/recovery/syn-ota.zip"
run_dl --target "$tid" --kind ota
check "safety: symlinked final recovery artifact is refused" [ "$status" -eq 1 ]
check "safety: final recovery symlink remains untouched" [ -L "$tdir/recovery/syn-ota.zip" ]
check "safety: final symlink refusal is explicit" grep -q 'symlinked recovery artifact' "$work/stderr"
rm -f "$tdir/recovery/syn-ota.zip"

ln -s "$work/serve/ota.zip" "$tdir/recovery/syn-ota.zip.part"
run_dl --target "$tid" --kind ota
check "safety: symlinked partial recovery artifact is refused" [ "$status" -eq 1 ]
check "safety: partial recovery symlink remains untouched" [ -L "$tdir/recovery/syn-ota.zip.part" ]
check "safety: partial symlink refusal is explicit" grep -q 'symlinked partial recovery artifact' "$work/stderr"
rm -f "$tdir/recovery/syn-ota.zip.part"

# Empty pins (header only, as propose-board.sh scaffolds) refuse.
mkdir -p "$boards/empty-pins-target"
write_manifest "$boards/empty-pins-target" OnePlus X y Z W
printf '# kind<TAB>build<TAB>version<TAB>filename<TAB>url<TAB>sha256<TAB>bytes<TAB>source-page\n# comment\n' > "$boards/empty-pins-target/recovery.pins"
run_dl --target empty-pins-target
check "refusal: header-only pins refuse" [ "$status" -eq 1 ]
check "refusal: header-only pins explain pinning" grep -q 'pin the official vendor URL' "$work/stderr"

# Non-https URL refused; file:// refused without the explicit allow flag.
printf 'factory%sB%sV%sx.zip%shttp://example.invalid/x.zip%s%s%s1%s-\n' "$tab" "$tab" "$tab" "$tab" "$tab" "$f_hash" "$tab" "$tab" > "$boards/empty-pins-target/recovery.pins"
run_dl --target empty-pins-target
check "refusal: http:// pin refused" [ "$status" -eq 1 ]
env -u FERRIX_RECOVERY_ALLOW_FILE_URL sh "$download" --target "$tid" --kind factory > "$work/stdout" 2> "$work/stderr" && status=0 || status=$?
check "refusal: file:// pin refused without explicit allow flag" [ "$status" -eq 1 ]

# The unchanged checked-in Pixel 9a pins file parses: both rows are selected,
# the https URLs pass validation, and the run stops only at the network stage.
# A stub curl records the URL and fails, so nothing is downloaded.
mkdir -p "$work/bin"
cat > "$work/bin/curl" <<'STUB'
#!/bin/sh
printf 'curl %s\n' "$*" >> "$STUB_CURL_LOG"
exit 7
STUB
chmod 0755 "$work/bin/curl"
export STUB_CURL_LOG="$work/curl.log"
: > "$STUB_CURL_LOG"
status=0
PATH="$work/bin:$PATH" sh "$download" --target google-pixel-9a-tegu-gxq96-mp1-0 --kind factory > "$work/stdout" 2> "$work/stderr" || status=$?
check "pixel: pins parse and reach the curl stage" grep -q 'https://dl.google.com/dl/android/aosp/tegu-cp3a.260905.009-factory-0c2f9053.zip' "$STUB_CURL_LOG"
check "pixel: stub curl failure stops the run" [ "$status" -ne 0 ]
check "pixel: no partial file kept after curl failure" sh -c "! ls '$boards/google-pixel-9a-tegu-gxq96-mp1-0/recovery'/*.part 2>/dev/null | grep -q ."
check "pixel: curl called resumable with a .part output" grep -q -- '--output .*\.part' "$STUB_CURL_LOG"

check "isolation: real Pixel recovery folder untouched by the test" sh -c "! ls '$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/recovery'/*.part 2>/dev/null | grep -q ."

finish recovery-download
