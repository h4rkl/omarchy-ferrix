#!/bin/sh
set -eu

# Hash-verified, offline firmware inventory and explicitly allow-listed
# extraction for the exact tegu package. The output is ignored. This script
# never talks to a device and never copies an Android userspace tree wholesale.

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
target="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0"
pins=${FERRIX_PIXEL9A_PINS:-"$target/recovery.pins"}
policy="$target/firmware-policy.toml"
out_root=${FERRIX_OUT_DIR:-"$root/out"}
firmware_out=${FERRIX_PIXEL9A_FIRMWARE_OUT:-"$out_root/pixel-9a/firmware/tegu"}
inventory="$firmware_out/source-inventory.tsv"
manifest="$firmware_out/firmware-manifest.tsv"
stage="$firmware_out/staged"
confirmation=I-understand-this-extracts-verified-firmware

die() {
  printf 'FAIL pixel-9a-firmware: %s\n' "$*" >&2
  exit 1
}

need() {
  command -v "$1" >/dev/null 2>&1 || die "missing prerequisite '$1'; install it explicitly"
}

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{ print $1 }'
  else
    need shasum
    shasum -a 256 "$1" | awk '{ print $1 }'
  fi
}

bytes_of() {
  wc -c < "$1" | tr -d '[:space:]'
}

pin() {
  awk -F '\t' -v kind="$1" -v column="$2" '$1 == kind { print $column; exit }' "$pins"
}

archive_path() {
  filename=$(pin factory 4)
  [ -n "$filename" ] || die "factory pin is missing from $pins"
  printf '%s\n' "${FERRIX_PIXEL9A_FACTORY_ARCHIVE:-$target/recovery/$filename}"
}

verify_archive() {
  archive=$(archive_path)
  [ -f "$archive" ] || die "missing pinned factory archive: $archive"
  [ ! -L "$archive" ] || die "pinned factory archive must not be a symlink: $archive"
  expected_bytes=$(pin factory 7)
  expected_hash=$(pin factory 6)
  actual_bytes=$(bytes_of "$archive")
  actual_hash=$(sha256_of "$archive")
  [ "$actual_bytes" = "$expected_bytes" ] ||
    die "factory archive size mismatch: expected $expected_bytes, got $actual_bytes"
  [ "$actual_hash" = "$expected_hash" ] ||
    die "factory archive SHA-256 mismatch: expected $expected_hash, got $actual_hash"
  printf '%s\t%s\t%s\n' "$archive" "$actual_bytes" "$actual_hash"
}

nested_name() {
  archive=$1
  # Keep the expression in an awk variable: BSD awk parses an unescaped `/`
  # inside a regexp literal as the end of that literal, while GNU awk accepts
  # the same spelling. The variable form is portable across both hosts.
  names=$(unzip -Z1 "$archive" | awk -v pattern='(^|/)image-tegu-[^/]+[.]zip$' '$0 ~ pattern { print }')
  count=$(printf '%s\n' "$names" | awk 'NF { count++ } END { print count + 0 }')
  [ "$count" -eq 1 ] || die "expected exactly one nested tegu image archive, found $count"
  printf '%s\n' "$names"
}

check_policy() {
  [ -f "$pins" ] || die "missing recovery pins: $pins"
  [ -f "$policy" ] || die "missing firmware policy: $policy"
  [ -f "$target/firmware-stage-allowlist.tsv" ] || die "missing target firmware stage allowlist: $target/firmware-stage-allowlist.tsv"
  grep -q '^target_id = "google-pixel-9a-tegu-gxq96-mp1-0"$' "$policy" ||
    die "firmware policy target identity mismatch"
  grep -q '^allow_unlisted_paths = false$' "$policy" ||
    die "firmware policy must reject unlisted paths"
  grep -q '^require_exact_factory_hash = true$' "$policy" ||
    die "firmware policy must require the exact factory hash"
  # Keep policy driver identifiers tied to the exact pinned Kleaf module
  # outputs. The source-family names remain in kernel-sources.toml; this field
  # is consumed as a module basename by target review and evidence tooling.
  grep -q '^kernel_driver = "bcmdhd4383"$' "$policy" ||
    die "firmware policy must name the pinned BCM4383 module output"
  grep -q '^kernel_driver = "nitrous"$' "$policy" ||
    die "firmware policy must name the pinned Nitrous module output"
  grep -q '^kernel_driver = "cpif"$' "$policy" ||
    die "firmware policy must name the pinned S5300 primary module output"
  grep -q '^kernel_driver = "mali_kbase"$' "$policy" ||
    die "firmware policy must name the pinned Mali kbase module output"
}

validate_allowlist() {
  allowlist_file=$1
  [ -f "$allowlist_file" ] || die "firmware stage allowlist is missing: $allowlist_file"
  awk -F '\t' '
    /^[[:space:]]*#/ || NF == 0 { next }
    NF != 4 && NF != 5 { bad = 1; next }
    NF == 5 && $5 != "bundle" { bad = 1; next }
    $1 == "" || $2 == "" || $3 == "" || $4 == "" { bad = 1 }
    seen[$4]++ { duplicate = 1 }
    END { exit(bad || duplicate) }
  ' "$allowlist_file" || die "firmware stage allowlist must contain four fields, or five fields ending in bundle, and unique runtime paths: $allowlist_file"
  # Per-unit calibration (persist-partition data such as the speaker
  # calibration, or any *calib* file) is device data, not factory firmware.
  # Fail closed even though such paths are absent from the allowed images.
  awk -F '\t' '
    /^[[:space:]]*#/ || NF == 0 { next }
    $3 ~ /(^|\/)persist(\/|$)/ || $3 ~ /calib/ || $3 ~ /speaker[.]cal$/ ||
      $4 ~ /calib/ || $4 ~ /speaker[.]cal$/ { bad = 1 }
    END { exit bad }
  ' "$allowlist_file" || die "firmware stage allowlist names per-unit calibration data: $allowlist_file"
  count=$(awk -F '\t' '$0 !~ /^[[:space:]]*#/ && (NF == 4 || (NF == 5 && $5 == "bundle")) { count++ } END { print count + 0 }' "$allowlist_file")
  [ "$count" -gt 0 ] || die "firmware stage allowlist is empty: $allowlist_file"
}

preflight() {
  check_policy
  validate_allowlist "$target/firmware-stage-allowlist.tsv"
  for command_name in awk grep unzip wc tr; do need "$command_name"; done
  if ! command -v sha256sum >/dev/null 2>&1 && ! command -v shasum >/dev/null 2>&1; then
    die "missing sha256sum or shasum"
  fi
  printf '%s\n' 'PASS pixel-9a-firmware policy: exact-source, no-broad-copy rules loaded'
}

write_inventory() {
  preflight
  mkdir -p "$firmware_out"
  source=$(verify_archive)
  archive=$(printf '%s\n' "$source" | awk -F '\t' '{ print $1 }')
  archive_bytes=$(printf '%s\n' "$source" | awk -F '\t' '{ print $2 }')
  archive_hash=$(printf '%s\n' "$source" | awk -F '\t' '{ print $3 }')
  nested=$(nested_name "$archive")
  tmp="$inventory.tmp.$$"
  trap 'rm -f "$tmp"' EXIT HUP INT TERM
  {
    printf 'schema_version\t1\n'
    printf 'target_id\tgoogle-pixel-9a-tegu-gxq96-mp1-0\n'
    printf 'factory_archive\t%s\t%s\t%s\n' "$archive" "$archive_bytes" "$archive_hash"
    printf 'nested_image_archive\t%s\n' "$nested"
    printf 'policy\t%s\t%s\n' "$policy" "$(sha256_of "$policy")"
    unzip -Z1 "$archive" | awk 'NF { print "outer_member\t" $0 }'
  } > "$tmp"
  mv "$tmp" "$inventory"
  trap - EXIT HUP INT TERM
  printf '%s\n' "PASS pixel-9a-firmware inventory: $inventory"
}

is_allowed_image() {
  image=$1
  case "$image" in
    vendor.img|vendor_dlkm.img) return 0 ;;
    *) return 1 ;;
  esac
}

extract_image() {
  archive=$1
  nested=$2
  image=$3
  destination=$4
  [ "$image" != modem.img ] ||
    die "modem.img is a raw radio payload, not an ext4 filesystem; use the board radio-loader path"
  is_allowed_image "$image" || die "source image is not allow-listed by firmware policy: $image"
  unzip -p "$nested" "$image" > "$destination" ||
    die "nested factory image is missing $image"
}

prepare_filesystem() {
  image=$1
  raw=$2
  filesystem=$3
  magic=$(od -An -tx4 -N4 "$raw" | tr -d '[:space:]')
  case "$magic" in
    ed26ff3a)
      need simg2img
      simg2img "$raw" "$filesystem" || die "simg2img failed for $image"
      ;;
    *)
      cp "$raw" "$filesystem"
      ;;
  esac
}

validate_path() {
  value=$1
  kind=$2
  [ -n "$value" ] || die "$kind is empty"
  case "$value" in
    *..*|*' '*|*'	'*|*[!A-Za-z0-9_./+-]*) die "$kind contains an unsafe path: $value" ;;
  esac
}

validate_bundle_component() {
  value=$1
  case "$value" in
    ''|.|..|*'/'*|*..*|*' '*|*'	'*|*[!A-Za-z0-9._+-]*)
      die "bundle entry contains an unsafe path component: $value"
      ;;
  esac
}

prepare_image() {
  image=$1
  raw="$work/$image"
  filesystem="$work/$image.ext4"
  if [ ! -f "$raw" ]; then
    extract_image "$archive" "$nested_path" "$image" "$raw"
    printf 'source\t%s\t%s\t%s\n' "$image" "$(bytes_of "$raw")" "$(sha256_of "$raw")" >> "$manifest"
  fi
  [ -f "$filesystem" ] || prepare_filesystem "$image" "$raw" "$filesystem"
  printf '%s\n' "$filesystem"
}

ensure_stage_tree() {
  [ ! -L "$stage" ] || die "firmware stage directory must not be a symlink: $stage"
  if [ -e "$stage" ]; then
    [ -d "$stage" ] || die "firmware stage path is not a directory: $stage"
    find "$stage" -type l -print -quit | grep -q . &&
      die "firmware stage tree contains a symlink: $stage"
  fi
  mkdir -p "$stage"
}

list_ext4_directory() {
  filesystem=$1
  source_directory=$2
  listing=$3
  raw_listing="$listing.raw"
  if ! DEBUGFS_PAGER=cat debugfs -R "ls -p $source_directory" "$filesystem" > "$raw_listing" 2>"$listing.err"; then
    rm -f "$raw_listing" "$listing" "$listing.err"
    die "source bundle directory is not readable: $source_directory"
  fi
  # debugfs -p emits /inode/mode/uid/gid/name// for directories and
  # /inode/mode/uid/gid/name/size/ for non-directories. Its octal mode text
  # uses prefixes such as 040755 (directory), 100644 (regular), and 120777
  # (symlink); accept the alternate zero-padded regular/symlink spellings as
  # well. Remaining mode types are special files. Names are kept as a
  # slash-delimited field, then rejected below before becoming paths.
  awk -F '/' '
    $1 == "" && $2 ~ /^[0-9]+$/ && $3 ~ /^[0-7]+$/ && $4 ~ /^-?[0-9]+$/ && $5 ~ /^-?[0-9]+$/ && $6 != "" {
      if ($3 ~ /^04/) kind = "directory"
      else if ($3 ~ /^100/ || $3 ~ /^010/) kind = "regular"
      else if ($3 ~ /^12/ || $3 ~ /^012/) kind = "symlink"
      else kind = "special"
      print kind "\t" $6
      found = 1
    }
    END { if (!found) exit 0 }
  ' "$raw_listing" | LC_ALL=C sort -t "$(printf '\t')" -k2,2 > "$listing"
  rm -f "$raw_listing" "$listing.err"
}

stage_bundle_file() {
  controller=$1
  image=$2
  source_file=$3
  runtime_file=$4
  validate_path "$source_file" source-path
  validate_path "$runtime_file" runtime-path
  case "$source_file" in /*) ;; *) die "bundle source path must be absolute: $source_file" ;; esac
  case "$runtime_file" in /*) die "bundle runtime path must be relative: $runtime_file" ;; esac
  ensure_stage_tree
  destination="$stage/$runtime_file"
  [ ! -L "$destination" ] || die "firmware destination must not be a symlink: $runtime_file"
  [ ! -e "$destination" ] || die "firmware destination already exists: $runtime_file"
  mkdir -p "$(dirname "$destination")"
  [ ! -L "$(dirname "$destination")" ] || die "firmware destination parent must not be a symlink: $runtime_file"
  debugfs -R "dump $source_file $destination" "$filesystem" >/dev/null 2>&1 ||
    die "source bundle entry is not readable in $image: $source_file"
  [ -f "$destination" ] || die "source bundle entry did not produce a regular file: $source_file"
  [ ! -L "$destination" ] || die "source bundle entry produced a symlink: $source_file"
  printf 'firmware\t%s\t%s\t%s\t%s\t%s\t%s\n' \
    "$controller" "$image" "$source_file" "$runtime_file" \
    "$(bytes_of "$destination")" "$(sha256_of "$destination")" >> "$manifest"
  bundle_file_count=$((bundle_file_count + 1))
}

walk_bundle() {
  controller=$1
  image=$2
  source_directory=$3
  runtime_directory=$4
  listing="$work/.bundle-list.$$.${bundle_sequence}"
  bundle_sequence=$((bundle_sequence + 1))
  # Keep this traversal frame in positional parameters. The helper functions
  # below use ordinary /bin/sh variables, and recursive calls otherwise
  # overwrite the caller's source/runtime paths and listing filename.
  set -- "$1" "$2" "$3" "$4" "$listing"
  list_ext4_directory "$filesystem" "$3" "$5"
  [ -s "$5" ] || die "required firmware bundle is empty: $3"
  while IFS="$(printf '\t')" read -r entry_kind entry_name; do
    [ -n "${entry_kind:-}" ] || continue
    case "$entry_name" in
      .|..) continue ;;
    esac
    validate_bundle_component "$entry_name"
    source_entry="$3/$entry_name"
    runtime_entry="$4/$entry_name"
    case "$entry_kind" in
      directory)
        walk_bundle "$1" "$2" "$source_entry" "$runtime_entry"
        ;;
      regular)
        stage_bundle_file "$1" "$2" "$source_entry" "$runtime_entry"
        ;;
      symlink)
        die "firmware bundle contains a symlink: $source_entry"
        ;;
      special)
        die "firmware bundle contains a non-regular, non-directory entry: $source_entry"
        ;;
      *)
        die "firmware bundle listing has an unknown entry kind: $entry_kind"
        ;;
    esac
  done < "$5"
  rm -f "$5"
}

stage_one() {
  controller=$1
  image=$2
  source_path=$3
  runtime_path=$4
  entry_type=${5:-file}
  validate_path "$controller" controller
  validate_path "$image" source-image
  validate_path "$source_path" source-path
  validate_path "$runtime_path" runtime-path
  case "$source_path" in /*) ;; *) die "source-path must be absolute: $source_path" ;; esac
  case "$runtime_path" in /*) die "runtime-path must be relative: $runtime_path" ;; esac
  grep -q "^id = \"$controller\"$" "$policy" ||
    die "controller is not listed in firmware policy: $controller"
  is_allowed_image "$image" || die "source image is not allow-listed: $image"
  case "$entry_type" in
    file)
      filesystem=$(prepare_image "$image")
      destination="$stage/$runtime_path"
      ensure_stage_tree
      [ ! -L "$destination" ] || die "firmware destination must not be a symlink: $runtime_path"
      [ ! -e "$destination" ] || die "firmware destination already exists: $runtime_path"
      mkdir -p "$(dirname "$destination")"
      [ ! -L "$(dirname "$destination")" ] || die "firmware destination parent must not be a symlink: $runtime_path"
      debugfs -R "dump $source_path $destination" "$filesystem" >/dev/null 2>&1 ||
        die "source path is not readable in $image: $source_path"
      [ -f "$destination" ] || die "source path did not produce a regular file: $source_path"
      [ ! -L "$destination" ] || die "source path produced a symlink: $source_path"
      printf 'firmware\t%s\t%s\t%s\t%s\t%s\t%s\n' \
        "$controller" "$image" "$source_path" "$runtime_path" \
        "$(bytes_of "$destination")" "$(sha256_of "$destination")" >> "$manifest"
      ;;
    bundle)
      case "$source_path" in */) die "bundle source path must not end with '/': $source_path" ;; esac
      case "$runtime_path" in */) die "bundle runtime path must not end with '/': $runtime_path" ;; esac
      filesystem=$(prepare_image "$image")
      bundle_file_count=0
      bundle_sequence=0
      root_stat="$work/.bundle-stat.$$.${bundle_sequence}"
      if ! DEBUGFS_PAGER=cat debugfs -R "stat $source_path" "$filesystem" > "$root_stat" 2>&1; then
        rm -f "$root_stat"
        die "source bundle path is not readable in $image: $source_path"
      fi
      grep -Eq 'Type:[[:space:]]+directory([[:space:]]|$)' "$root_stat" || {
        rm -f "$root_stat"
        die "source bundle path is not a directory: $source_path"
      }
      rm -f "$root_stat"
      ensure_stage_tree
      walk_bundle "$controller" "$image" "$source_path" "$runtime_path"
      [ "$bundle_file_count" -gt 0 ] || die "required firmware bundle is empty: $source_path"
      ;;
    *) die "unknown firmware stage entry type: $entry_type" ;;
  esac
}

stage() {
  preflight
  [ "${FERRIX_PIXEL9A_FIRMWARE:-}" = "$confirmation" ] ||
    die "firmware extraction is opt-in; set FERRIX_PIXEL9A_FIRMWARE=$confirmation"
  allowlist=${FERRIX_PIXEL9A_FIRMWARE_ALLOWLIST:-"$target/firmware-stage-allowlist.tsv"}
  validate_allowlist "$allowlist"
  need debugfs
  need od
  need cp
  need dirname
  need find
  need mktemp
  need sort
  mkdir -p "$firmware_out" "$stage"
  source=$(verify_archive)
  archive=$(printf '%s\n' "$source" | awk -F '\t' '{ print $1 }')
  nested=$(nested_name "$archive")
  work=$(mktemp -d "$firmware_out/.work.XXXXXX")
  nested_path="$work/image.zip"
  trap 'rm -rf "$work"' EXIT HUP INT TERM
  unzip -p "$archive" "$nested" > "$nested_path" || die "could not materialize nested image archive"
  tmp="$manifest.tmp.$$"
  : > "$tmp"
  mv "$tmp" "$manifest"
  printf 'schema_version\t1\n' >> "$manifest"
  printf 'target_id\tgoogle-pixel-9a-tegu-gxq96-mp1-0\n' >> "$manifest"
  printf 'factory_archive\t%s\t%s\n' "$archive" "$(printf '%s\n' "$source" | awk -F '\t' '{ print $3 }')" >> "$manifest"
  while IFS="$(printf '\t')" read -r controller image source_path runtime_path entry_type extra; do
    [ -n "${controller:-}" ] || continue
    case "$controller" in \#*) continue ;; esac
    [ -z "${extra:-}" ] || die "allowlist has more than five TSV fields"
    [ -n "${entry_type:-}" ] || entry_type=file
    stage_one "$controller" "$image" "$source_path" "$runtime_path" "$entry_type"
  done < "$allowlist"
  grep -q '^firmware[[:space:]]' "$manifest" || die "allowlist produced no staged firmware"
  cp "$manifest" "$stage/firmware-manifest.tsv"
  printf '%s\n' "PASS pixel-9a-firmware staged allowlist: $manifest"
}

inspect_sources() {
  preflight
  [ "${FERRIX_PIXEL9A_FIRMWARE:-}" = "$confirmation" ] ||
    die "firmware source inspection is opt-in; set FERRIX_PIXEL9A_FIRMWARE=$confirmation"
  allowlist=${FERRIX_PIXEL9A_FIRMWARE_ALLOWLIST:-"$target/firmware-stage-allowlist.tsv"}
  validate_allowlist "$allowlist"
  for command_name in debugfs od cp dirname mktemp; do need "$command_name"; done
  mkdir -p "$firmware_out"
  source=$(verify_archive)
  archive=$(printf '%s\n' "$source" | awk -F '\t' '{ print $1 }')
  nested=$(nested_name "$archive")
  work=$(mktemp -d "$firmware_out/.inspect.XXXXXX")
  nested_path="$work/image.zip"
  manifest="$work/source-check.tsv"
  : >"$manifest"
  trap 'rm -rf "$work"' EXIT HUP INT TERM
  unzip -p "$archive" "$nested" >"$nested_path" || die 'could not materialize nested image archive'
  while IFS="$(printf '\t')" read -r controller image source_path runtime_path entry_type extra; do
    [ -n "${controller:-}" ] || continue
    case "$controller" in \#*) continue ;; esac
    [ -z "${extra:-}" ] || die 'firmware allowlist has more than five TSV fields'
    [ -n "${entry_type:-}" ] || entry_type=file
    validate_path "$controller" controller
    validate_path "$image" source-image
    validate_path "$source_path" source-path
    validate_path "$runtime_path" runtime-path
    grep -q "^id = \"$controller\"$" "$policy" || die "controller is not listed in firmware policy: $controller"
    is_allowed_image "$image" || die "source image is not allow-listed: $image"
    filesystem=$(prepare_image "$image")
    printf '%s\n' "== $controller: $image:$source_path -> $runtime_path ($entry_type) =="
    stat_output=$(DEBUGFS_PAGER=cat debugfs -R "stat $source_path" "$filesystem" 2>&1 || true)
    printf '%s\n' "$stat_output"
    if ! printf '%s\n' "$stat_output" | grep -q '^Inode:'; then
      source_parent=${source_path%/*}
      [ -n "$source_parent" ] || source_parent=/
      printf 'source entry is unresolved; listing parent %s\n' "$source_parent"
      DEBUGFS_PAGER=cat debugfs -R "ls -p $source_parent" "$filesystem" ||
        printf 'source parent listing failed: %s\n' "$source_parent"
      continue
    fi
    if [ "$entry_type" = bundle ]; then
      DEBUGFS_PAGER=cat debugfs -R "ls -p $source_path" "$filesystem" ||
        printf 'source directory listing failed: %s\n' "$source_path"
    fi
  done <"$allowlist"
}

usage() {
  printf '%s\n' 'usage: tools/pixel-9a/firmware.sh preflight|inventory|inspect|stage' >&2
  exit 2
}

[ "$#" -eq 1 ] || usage
case "$1" in
  preflight) preflight ;;
  inventory) write_inventory ;;
  inspect) inspect_sources ;;
  stage) stage ;;
  *) usage ;;
esac
