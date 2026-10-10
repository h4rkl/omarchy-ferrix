#!/bin/sh
# Builds Waydroid's LineageOS 23.2 (Android 16 QPR2) arm64_only system and
# vendor images from source, for the Pixel 9a stock-Mali route (#19). Linux
# x86_64 build host only; nothing here touches a device.
#
#   sh tools/android/waydroid-lineage-build.sh tools     # pinned repo + git-lfs (user-local)
#   sh tools/android/waydroid-lineage-build.sh init      # repo init (shallow, LFS)
#   sh tools/android/waydroid-lineage-build.sh sync      # Waydroid manifests + repo sync
#   sh tools/android/waydroid-lineage-build.sh lfs       # fetch LFS prebuilts (part of sync)
#   sh tools/android/waydroid-lineage-build.sh pins      # write + check Waydroid revision pins
#   sh tools/android/waydroid-lineage-build.sh patch     # Waydroid + Ferrix patches
#   sh tools/android/waydroid-lineage-build.sh build     # systemimage + vendorimage
#   sh tools/android/waydroid-lineage-build.sh all
#
# Every phase needs FERRIX_WAYDROID_LINEAGE_BUILD=I-understand-this-syncs-and-builds-lineageos
# (sync is ~150 GB, a build takes hours). Inputs come from
# tools/android/waydroid-lineage.pins: repo is a pinned tag whose commit is
# checked, git-lfs a pinned release tarball whose SHA-256 is checked; both
# land in the cache and are put on PATH for this script only (no global
# config, no sudo). The synced tree is recorded with `repo manifest -r`.
# Outputs: ${FERRIX_OUT_DIR:-out}/android/waydroid-lineage-23.2/<stamp>/
# {system.img,vendor.img,manifest-snapshot.xml,manifest.tsv}. Source tree:
# ${FERRIX_CACHE_DIR:-.cache/ferrix}/android/lineage-23.2 (ignored).
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
pins="$here/waydroid-lineage.pins"
patches="$here/waydroid-patches"
cache=${FERRIX_CACHE_DIR:-"$root/.cache/ferrix"}
tools="$cache/android/tools"
src=${FERRIX_WAYDROID_LINEAGE_SRC:-"$cache/android/lineage-23.2"}
out_root=${FERRIX_OUT_DIR:-"$root/out"}/android/waydroid-lineage-23.2
jobs=${FERRIX_WAYDROID_LINEAGE_JOBS:-10}
sync_jobs=${FERRIX_WAYDROID_LINEAGE_SYNC_JOBS:-4}
ack=I-understand-this-syncs-and-builds-lineageos

die() { printf 'FAIL waydroid-lineage-build: %s\n' "$*" >&2; exit 1; }
pin() {
  v=$(sed -n "s/^$1=//p" "$pins" | head -n 1)
  [ -n "$v" ] || die "pin $1 missing from $pins"
  printf '%s\n' "$v"
}
sha256_of() { sha256sum "$1" | awk '{ print $1 }'; }

phase=${1:-}
[ -n "$phase" ] || { sed -n '2,22p' "$0" | sed 's/^# \{0,1\}//'; exit 2; }
[ "${FERRIX_WAYDROID_LINEAGE_BUILD:-}" = "$ack" ] ||
  die "opt-in; set FERRIX_WAYDROID_LINEAGE_BUILD=$ack"
[ "$(uname -s)-$(uname -m)" = Linux-x86_64 ] || die "needs a Linux x86_64 build host"
for t in git curl tar python3 sha256sum; do
  command -v "$t" >/dev/null 2>&1 || die "missing prerequisite '$t'; install it explicitly"
done
PATH="$tools/bin:$PATH"
export PATH

do_tools() {
  mkdir -p "$tools/bin"
  tag=$(pin REPO_TAG); commit=$(pin REPO_COMMIT)
  if [ ! -d "$tools/git-repo/.git" ]; then
    # A full clone (a few MB): repo's launcher cannot re-clone a shallow one.
    git clone -q -b "$tag" "$(pin REPO_URL)" "$tools/git-repo"
  fi
  [ "$(git -C "$tools/git-repo" rev-parse "$tag^{commit}")" = "$commit" ] ||
    die "repo tag $tag is not the pinned commit $commit"
  [ "$(git -C "$tools/git-repo" rev-parse HEAD)" = "$commit" ] ||
    die "repo checkout is not the pinned commit $commit"
  ln -sf "$tools/git-repo/repo" "$tools/bin/repo"
  if [ ! -x "$tools/bin/git-lfs" ]; then
    tmp=$(mktemp -d "$tools/.lfs.XXXXXX")
    curl -fsSL -o "$tmp/lfs.tgz" "$(pin GIT_LFS_URL)"
    [ "$(sha256_of "$tmp/lfs.tgz")" = "$(pin GIT_LFS_SHA256)" ] ||
      { rm -rf "$tmp"; die "git-lfs tarball hash mismatch"; }
    tar -xzf "$tmp/lfs.tgz" -C "$tmp"
    install -m 0755 "$(find "$tmp" -type f -name git-lfs | head -n 1)" "$tools/bin/git-lfs"
    rm -rf "$tmp"
  fi
  mesa_host_tools
  printf 'PASS waydroid-lineage tools: repo %s (%s), %s, meson %s\n' "$tag" "$commit" \
    "$(git-lfs version)" "$("$mesa_bin/meson" --version)"
}

# Mesa's Android makefile runs host meson, python3 (with mako), ld.lld and
# mesa_clc under a hard-coded PATH; the Ferrix mesa3d patch puts FERRIX_MESA_HOST_PATH first.
mesa_bin="$tools/mesa-host/path"
mesa_host_tools() {
  req="$here/waydroid-mesa-host.requirements"
  stamp="$tools/mesa-host/.requirements.sha256"
  if [ "$(cat "$stamp" 2>/dev/null)" != "$(sha256_of "$req")" ]; then
    rm -rf "$tools/mesa-host"
    python3 -m venv "$tools/mesa-host"
    "$tools/mesa-host/bin/pip" install -q --disable-pip-version-check \
      --require-hashes --no-deps --only-binary=:all: -r "$req"
    sha256_of "$req" >"$stamp"
  fi
  mkdir -p "$mesa_bin"
  ln -sf ../bin/meson "$mesa_bin/meson"
  ln -sf ../bin/python3 "$mesa_bin/python3"
  # Mesa's own ninja step and its ld.lld wrapper (MESON_MESA3D_GEN/ld.lld
  # execs ld.lld from PATH) come from the tree's prebuilts: Soong's ninja and
  # the default clang's lld.
  if [ -x "$src/prebuilts/build-tools/linux-x86/bin/ninja" ]; then
    ln -sf "$src/prebuilts/build-tools/linux-x86/bin/ninja" "$mesa_bin/ninja"
  fi
  clang=$(sed -n 's/^[[:space:]]*ClangDefaultVersion[[:space:]]*=[[:space:]]*"\(clang-[^"]*\)".*/\1/p' \
    "$src/build/soong/cc/config/global.go" 2>/dev/null | head -n 1)
  if [ -n "$clang" ] && [ -x "$src/prebuilts/clang/host/linux-x86/$clang/bin/ld.lld" ]; then
    ln -sf "$src/prebuilts/clang/host/linux-x86/$clang/bin/ld.lld" "$mesa_bin/ld.lld"
  fi
  # -Dmesa-clc=system -Dprecomp-compiler=system: the tree's prebuilt CL
  # compilers (self-verifying wrappers over prebuilts/mesa-tools/root), which
  # need a libxml2.so.2 the bundle does not carry.
  lib="$tools/mesa-host/lib"
  if [ ! -e "$lib/libxml2.so.2" ]; then
    command -v zstd >/dev/null 2>&1 || die "missing prerequisite 'zstd'; install it explicitly"
    tmp=$(mktemp -d "$tools/.libxml2.XXXXXX")
    curl -fsSL -o "$tmp/pkg.tar.zst" "$(pin MESA_LIBXML2_URL)"
    [ "$(sha256_of "$tmp/pkg.tar.zst")" = "$(pin MESA_LIBXML2_SHA256)" ] ||
      { rm -rf "$tmp"; die "libxml2-legacy package hash mismatch"; }
    zstd -dcq "$tmp/pkg.tar.zst" | tar -x -C "$tmp" --wildcards 'usr/lib/libxml2.so.2*'
    mkdir -p "$lib"
    cp -P "$tmp"/usr/lib/libxml2.so.2* "$lib/"
    rm -rf "$tmp"
  fi
  for t in mesa_clc vtn_bindgen2 asahi_clc panfrost_compile; do
    [ -x "$src/prebuilts/mesa-tools/$t" ] || continue
    rm -f "$mesa_bin/$t"
    printf '#!/bin/sh\nLD_LIBRARY_PATH=%s${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH} exec %s "$@"\n' \
      "$lib" "$src/prebuilts/mesa-tools/$t" >"$mesa_bin/$t"
    chmod 0755 "$mesa_bin/$t"
  done
  FERRIX_MESA_HOST_PATH=$mesa_bin
  export FERRIX_MESA_HOST_PATH
}

repo_flags() {
  printf '%s\n' "--repo-url=$tools/git-repo" "--repo-rev=$(pin REPO_TAG)" --no-repo-verify
}

do_init() {
  mkdir -p "$src"
  cd "$src"
  # shellcheck disable=SC2046
  repo init -u "$(pin LINEAGE_MANIFEST_URL)" -b "$(pin LINEAGE_BRANCH)" --git-lfs --depth=1 \
    $(repo_flags)
  printf 'PASS waydroid-lineage init: %s\n' "$src"
}

do_sync() {
  cd "$src"
  [ -d .repo ] || die "run init first"
  repo sync -q -c --no-tags build/make
  # Outside the source tree: the build scans every Android.mk under $src.
  vendor="$cache/android/android_vendor_waydroid-pin"
  if [ ! -d "$vendor/.git" ]; then
    git clone -q -b "$(pin WAYDROID_VENDOR_BRANCH)" "$(pin WAYDROID_VENDOR_URL)" "$vendor"
  fi
  git -C "$vendor" fetch -q origin "$(pin WAYDROID_VENDOR_COMMIT)" 2>/dev/null || true
  git -C "$vendor" checkout -q "$(pin WAYDROID_VENDOR_COMMIT)"
  # The script prefers its own checked-out manifests-<sdk> directory, so the
  # local manifests come from the pinned commit, not from the network.
  TERM=${TERM:-dumb} WAYDROID_PATCH_SDK=$(pin WAYDROID_PATCH_SDK) \
    bash "$vendor/manifest_scripts/generate-manifest.sh"
  write_revision_pins
  # googlesource answers HTTP 429 to aggressive parallel fetches; keep the
  # fetch fan-out small, let repo retry, and resume whole passes.
  pass=1
  until repo sync -c --no-tags -j"$sync_jobs" --optimized-fetch --retry-fetches=8; do
    [ "$pass" -lt 4 ] || die "repo sync failed after $pass passes"
    pass=$((pass + 1))
    printf 'repo sync pass %s after a failure; waiting 120s\n' "$pass" >&2
    sleep 120
  done
  lfs_pull
  printf 'PASS waydroid-lineage sync: %s\n' "$src"
}

# Waydroid's local manifests follow branches; pin every project they add to
# the reviewed revisions (repo applies 99-* after them).
revisions="$here/waydroid-lineage-revisions.tsv"
write_revision_pins() {
  [ -f "$revisions" ] || die "missing $revisions"
  {
    printf '<?xml version="1.0" encoding="UTF-8"?>\n<manifest>\n'
    printf '  <!-- Generated by tools/android/waydroid-lineage-build.sh from waydroid-lineage-revisions.tsv -->\n'
    awk -F '\t' '!/^#/ && NF >= 3 { printf "  <extend-project name=\"%s\" path=\"%s\" revision=\"%s\" />\n", $2, $1, $3 }' "$revisions"
    printf '</manifest>\n'
  } >"$src/.repo/local_manifests/99-ferrix-revisions.xml"
}

revision_check() {
  bad=$(awk -F '\t' '!/^#/ && NF >= 3 { print $1 " " $3 }' "$revisions" | while read -r path rev; do
    [ "$(git -C "$src/$path" rev-parse HEAD 2>/dev/null)" = "$rev" ] || printf '%s ' "$path"
  done)
  [ -z "$bad" ] || die "projects not at their pinned revisions (run the sync phase): $bad"
}

# `repo init --git-lfs` does not install the lfs filter into every project
# checkout, so LFS-tracked prebuilts (external/chromium-webview's
# webview.apk) can stay as pointer files and fail late in the build. Pull
# them explicitly; git-lfs checks each object against its pointer's oid.
lfs_pull() {
  # shellcheck disable=SC2016
  repo forall -c 'if grep -qs "filter=lfs" .gitattributes; then
      git lfs install --local >/dev/null && git lfs pull || exit 1
    fi' || die "git lfs pull failed"
  lfs_check
}

lfs_check() {
  pointers=$(repo forall -c 'if grep -qs "filter=lfs" .gitattributes; then
      git lfs ls-files 2>/dev/null | awk -v p="$REPO_PATH" "\$2 == \"-\" { print p \"/\" \$3 }"
    fi')
  [ -z "$pointers" ] || die "LFS objects not fetched (run the sync phase): $pointers"
}

apply_ferrix_patches() {
  [ -d "$patches" ] || return 0
  find "$patches" -type f -name '*.patch' | LC_ALL=C sort | while IFS= read -r patch; do
    project=$(dirname -- "${patch#"$patches"/}")
    [ -d "$src/$project" ] || die "patch targets a missing project: $project"
    if git -C "$src/$project" apply --reverse --check "$patch" 2>/dev/null; then
      printf '= ferrix patch already applied: %s\n' "${patch#"$patches"/}"
    else
      git -C "$src/$project" apply --check "$patch" || die "patch does not apply: ${patch#"$patches"/}"
      git -C "$src/$project" apply "$patch"
      printf '+ ferrix patch applied: %s\n' "${patch#"$patches"/}"
    fi
  done
}

do_patch() {
  cd "$src"
  [ -f build/envsetup.sh ] || die "run sync first"
  # apply-waydroid-patches reports a conflict when a project already carries
  # a patch (Waydroid's forks of system/core do). Judge by content instead:
  # every patch subject must be in its project's history afterwards.
  bash -c '. build/envsetup.sh >/dev/null && apply-waydroid-patches' || true
  missing=0
  patch_root="$src/vendor/extra/waydroid-patches/base-patches-$(pin WAYDROID_PATCH_SDK)"
  for patch in $(cd "$patch_root" && find . -name '*.patch' | LC_ALL=C sort); do
    project=$(dirname "${patch#./}")
    if grep -qxF "${patch#./}" "$patches/superseded.txt" 2>/dev/null; then
      printf '~ waydroid patch superseded by a Ferrix patch: %s\n' "${patch#./}"
      continue
    fi
    # git mailinfo unfolds the header and strips the [PATCH] prefix.
    subject=$(git mailinfo /dev/null /dev/null <"$patch_root/$patch" | sed -n 's/^Subject: //p')
    { [ -n "$subject" ] && git -C "$src/$project" log -n 5000 --format=%s | grep -qxF "$subject"; } ||
      git -C "$src/$project" apply --check -R "$patch_root/$patch" 2>/dev/null || {
      printf 'missing waydroid patch: %s\n' "${patch#./}" >&2; missing=$((missing + 1)); }
  done
  [ "$missing" -eq 0 ] || die "$missing Waydroid patch(es) not applied"
  apply_ferrix_patches
  printf 'PASS waydroid-lineage patch\n'
}

target_release() {
  if [ -n "${FERRIX_WAYDROID_LINEAGE_RELEASE:-}" ]; then
    printf '%s\n' "$FERRIX_WAYDROID_LINEAGE_RELEASE"
    return
  fi
  # LineageOS ships one release config per branch (e.g. bp4a); pick it.
  ls "$src/vendor/lineage/release/release_configs" 2>/dev/null | sed -n 's/\.textproto$//p' | grep -v -E '^(root|trunk_staging)$' | head -n 1
}

do_build() {
  cd "$src"
  revision_check
  lfs_check
  release=$(target_release)
  [ -n "$release" ] || die "cannot resolve the release config; set FERRIX_WAYDROID_LINEAGE_RELEASE"
  combo="$(pin LUNCH_PRODUCT)-$release-$(pin LUNCH_VARIANT)"
  mkdir -p "$cache/android/logs"
  log="$cache/android/logs/build-$(date -u +%Y%m%dT%H%M%SZ).log"
  bash -c ". build/envsetup.sh >/dev/null && lunch $combo && m -j$jobs systemimage vendorimage" >"$log" 2>&1 ||
    { tail -40 "$log" >&2; die "build failed; log $log"; }
  product=$(ls -d "$src"/out/target/product/waydroid_arm64_only 2>/dev/null) || die "no product output"
  stamp=$(date -u +%Y%m%dT%H%M%SZ)
  dest="$out_root/$stamp"
  mkdir -p "$dest"
  repo manifest -r -o "$dest/manifest-snapshot.xml" >/dev/null
  for image in system vendor; do
    cp "$product/$image.img" "$dest/$image.img"
  done
  {
    printf 'schema_version\t1\n'
    printf 'combo\t%s\n' "$combo"
    printf 'pins\t%s\n' "$(sha256_of "$pins")"
    printf 'manifest_snapshot\t%s\n' "$(sha256_of "$dest/manifest-snapshot.xml")"
    for image in system vendor; do
      printf 'artifact\t%s.img\t%s\t%s\n' "$image" "$(wc -c <"$dest/$image.img" | tr -d ' ')" "$(sha256_of "$dest/$image.img")"
    done
  } >"$dest/manifest.tsv"
  printf 'PASS waydroid-lineage build: %s (%s)\n' "$dest" "$combo"
}

case $phase in
  tools) do_tools ;;
  init) do_tools; do_init ;;
  sync) do_tools; do_sync ;;
  lfs) do_tools; cd "$src"; lfs_pull ;;
  pins) do_tools; cd "$src"; write_revision_pins; repo manifest >/dev/null; revision_check
        printf 'PASS waydroid-lineage pins: %s projects\n' "$(grep -vc '^#' "$revisions")" ;;
  patch) do_tools; do_patch ;;
  build) do_tools; do_build ;;
  all) do_tools; do_init; do_sync; do_patch; do_build ;;
  *) die "unknown phase: $phase" ;;
esac
