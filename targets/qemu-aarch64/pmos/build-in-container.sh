#!/bin/sh
# Runs inside containers/guest-builder as the unprivileged `pmos` user
# (started by pmos-build.sh). Configures pmbootstrap non-interactively from the
# checked-in pmbootstrap.cfg and pins.env, installs the postmarketOS rootfs for
# the generic QEMU aarch64 device, injects the project readiness marker,
# exports kernel/initramfs/rootfs image to /out and writes manifest.json.
set -eu
project=${FERRIX_PMOS_PROJECT:-/project}
out=${FERRIX_PMOS_OUT:-/out}
cache=${FERRIX_PMBOOTSTRAP_CACHE:-/cache/pmbootstrap}
work=$cache/work
cfg=$cache/pmbootstrap_v3.cfg
aports=$work/cache_git/pmaports
password=${FERRIX_PMOS_PASSWORD:-147147}
. "$project/pins.env"

log() { printf '[pmos-build] %s\n' "$*"; }
fail() {
  printf '{"evidence":"QEMU","event":"pmos-build-failed","reason":"%s","detail":"%s"}\n' "$1" "$2" >&2
  exit 1
}

# 1. pmbootstrap identity must match the pins baked into the builder image.
pmb_version=$(pmbootstrap --version)
image_tag=$(cat /opt/pmbootstrap-tag)
image_commit=$(cat /opt/pmbootstrap-commit)
[ "$image_tag" = "$PMBOOTSTRAP_TAG" ] && [ "$image_commit" = "$PMBOOTSTRAP_COMMIT" ] \
  || fail "pmbootstrap-pin-mismatch" "image has $image_tag/$image_commit, pins.env has $PMBOOTSTRAP_TAG/$PMBOOTSTRAP_COMMIT"
log "pmbootstrap $pmb_version (tag $PMBOOTSTRAP_TAG, commit $PMBOOTSTRAP_COMMIT)"

# 2. Work directory: what `pmbootstrap init` would have created.
work_version=$(python3 -c 'import pmb.config; print(pmb.config.work_version)')
mkdir -p "$work/cache_git"
chmod 0700 "$work"
[ -f "$work/version" ] || printf '%s\n' "$work_version" > "$work/version"
# OPT-IN Android runtime: the -android cfg only differs in extra_packages.
cfg_src=pmbootstrap.cfg
[ "${FERRIX_PMOS_ANDROID:-0}" != 1 ] || cfg_src=pmbootstrap-android.cfg
log "pmbootstrap config: $cfg_src"
cp "$project/$cfg_src" "$cfg"
chmod 0600 "$cfg"
cfg_work=$(sed -n 's/^work *= *//p' "$cfg")
[ "$cfg_work" = "$work" ] || fail "config-work-mismatch" "pmbootstrap.cfg work=$cfg_work, expected $work"

# 3. pmaports at the pinned channel branch and commit. pmbootstrap reads
#    channels.cfg from origin/main, so that ref is fetched as well.
if [ ! -d "$aports/.git" ]; then
  log "cloning pmaports $PMAPORTS_BRANCH"
  git clone --quiet --depth 1 --branch "$PMAPORTS_BRANCH" "$PMAPORTS_REPO" "$aports"
fi
git -C "$aports" remote set-url origin "$PMAPORTS_REPO"
git -C "$aports" fetch --quiet --depth 1 origin "+refs/heads/main:refs/remotes/origin/main"
if [ "$(git -C "$aports" rev-parse HEAD)" != "$PMAPORTS_COMMIT" ]; then
  log "pinning pmaports to $PMAPORTS_COMMIT"
  git -C "$aports" fetch --quiet --depth 1 origin "$PMAPORTS_COMMIT"
  git -C "$aports" checkout --quiet -B "$PMAPORTS_BRANCH" "$PMAPORTS_COMMIT"
fi
aports_commit=$(git -C "$aports" rev-parse HEAD)
[ "$aports_commit" = "$PMAPORTS_COMMIT" ] || fail "pmaports-pin-mismatch" "checked out $aports_commit"
channel=$(sed -n 's/^channel=//p' "$aports/pmaports.cfg")
[ "$channel" = "$PMAPORTS_CHANNEL" ] || fail "pmaports-channel-mismatch" "pmaports.cfg channel=$channel, pins.env has $PMAPORTS_CHANNEL"
channels_cfg=$(git -C "$aports" show origin/main:channels.cfg)
log "pmaports $PMAPORTS_BRANCH @ $aports_commit (channel $channel)"

# 4. Build the rootfs and disk image. `-y` answers pmbootstrap's confirmations;
#    the password is the only interactive install input and is test-only.
pmb() { pmbootstrap --config "$cfg" -y --details-to-stdout "$@"; }
pmb status || true
# Detach loop devices and mounts a previous (possibly aborted) run left behind
# in the Docker VM before touching the chroots.
pmb shutdown
if [ "${FERRIX_PMOS_KEEP_CHROOTS:-0}" != 1 ]; then
  log "zapping existing chroots (packages and caches are kept)"
  pmb zap
fi
log "pmbootstrap install (device qemu-aarch64, ui console, openrc)"
pmb install --password "$password"
img="$work/chroot_native/home/pmos/rootfs/qemu-aarch64.img"
[ -f "$img" ] || fail "no-rootfs-image" "$img missing after pmbootstrap install"
pmb shutdown

# 5. Finalize as root: inject the readiness marker, export boot files, record
#    image facts, copy the image to /out.
sudo -n env FERRIX_PMOS_PROJECT="$project" FERRIX_PMOS_OUT="$out" FERRIX_PMOS_ANDROID="${FERRIX_PMOS_ANDROID:-0}" sh "$project/finalize-in-container.sh" "$img"
[ -f "$out/image-facts.json" ] || fail "no-image-facts" "finalize did not write image-facts.json"

# 6. Manifest.
export pmb_version channels_cfg aports_commit channel
export GUEST_BUILDER_BASE PMBOOTSTRAP_REPO PMBOOTSTRAP_TAG PMBOOTSTRAP_COMMIT PMAPORTS_REPO PMAPORTS_BRANCH
python3 - "$out" "$project" "$cfg" <<'PY'
import configparser, datetime, hashlib, json, os, sys
out, project, cfg_path = sys.argv[1:4]
from pmb.core.config import Config

def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()

facts = json.load(open(os.path.join(out, "image-facts.json")))
channels = configparser.ConfigParser()
channels.read_string(os.environ["channels_cfg"])
channel = os.environ["channel"]
chan = channels[channel] if channel in channels else {}
cfg = configparser.ConfigParser()
cfg.read(cfg_path)
guest_cfg = {k: cfg["pmbootstrap"][k] for k in ("device", "kernel", "ui", "service_manager", "user", "hostname", "extra_packages", "build_pkgs_on_install")}

artifacts = {}
for name in sorted(facts["exported"]):
    path = os.path.join(out, name)
    artifacts[name] = {"sha256": sha256(path), "bytes": os.path.getsize(path)}

manifest = {
    "schema_version": 1,
    "evidence": "QEMU",
    "identity": {
        "ship": "Omarchy Ferrix",
        "runtime_namespace": "ferrix",
    },
    "route": "postmarketos-pmbootstrap",
    "target": "qemu-aarch64",
    "built_at": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    "builder": {
        "base_image": os.environ["GUEST_BUILDER_BASE"],
        "image_tag": os.environ.get("FERRIX_BUILDER_IMAGE_TAG", ""),
        "image_id": os.environ.get("FERRIX_BUILDER_IMAGE_ID", ""),
        "privileges": "--privileged (mount, chroot, loop devices)",
        "work_mount": os.environ.get("FERRIX_PMOS_WORK_MOUNT", ""),
    },
    "pmbootstrap": {
        "version": os.environ["pmb_version"],
        "repo": os.environ["PMBOOTSTRAP_REPO"],
        "tag": os.environ["PMBOOTSTRAP_TAG"],
        "commit": os.environ["PMBOOTSTRAP_COMMIT"],
        "work_version": open(os.path.join(os.path.dirname(cfg_path), "work", "version")).read().strip(),
    },
    "pmaports": {
        "repo": os.environ["PMAPORTS_REPO"],
        "channel": channel,
        "branch": os.environ["PMAPORTS_BRANCH"],
        "commit": os.environ["aports_commit"],
        "branch_aports": chan.get("branch_aports", ""),
        "mirrordir_alpine": chan.get("mirrordir_alpine", ""),
    },
    "mirrors": {"alpine": Config.mirrors["alpine"], "pmaports": Config.mirrors["pmaports"]},
    "guest": {
        **guest_cfg,
        "os_release": facts["os_release"],
        "packages": facts["packages"],
        "kernel_cmdline": facts["kernel_cmdline"],
        "kernel_cmdline_source": facts["kernel_cmdline_source"],
        "partitions": facts["partitions"],
        "readiness_marker": facts["readiness_marker"],
        "product": facts["product"],
        "android": facts.get("android", {"enabled": False}),
    },
    "project": {
        "revision": os.environ.get("FERRIX_PROJECT_REV", "unknown"),
        "pins_env_sha256": sha256(os.path.join(project, "pins.env")),
        "pmbootstrap_cfg_sha256": sha256(os.path.join(project, "pmbootstrap-android.cfg" if os.environ.get("FERRIX_PMOS_ANDROID") == "1" else "pmbootstrap.cfg")),
        "pmbootstrap_cfg": "pmbootstrap-android.cfg" if os.environ.get("FERRIX_PMOS_ANDROID") == "1" else "pmbootstrap.cfg",
        "marker_sha256": sha256(os.path.join(project, "local.d", "ferrix-boot-health.start")),
        "product_start_sha256": sha256(os.path.join(project, "local.d", "00-ferrix-product.start")),
    },
    "artifacts": artifacts,
}
with open(os.path.join(out, "manifest.json"), "w") as f:
    json.dump(manifest, f, indent=2, sort_keys=True)
    f.write("\n")
print("[pmos-build] manifest written:", os.path.join(out, "manifest.json"))
PY
log "done: $(ls "$out")"
