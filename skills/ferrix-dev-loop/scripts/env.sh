#!/bin/sh
# Shared settings for the dev-loop scripts. Source it; do not run it.
# Machine-local values (builder host, paths) live in the ignored file
# out/dev-loop.env so no host name or path is committed. Example:
#   FERRIX_REMOTE=user@builder
#   FERRIX_REMOTE_REPO=/home/user/omarchy-ferrix
#   FERRIX_REMOTE_BRANCH=mac-handoff
#   FERRIX_CAPACITY_TSV=out/pixel-9a/partition-capacities-static-<capture>.tsv
#   FERRIX_BASE_ROOTFS=out/pixel-9a/rootfs/tegu-android-<stamp>/rootfs.img
#   FERRIX_WEBCAM=0
#   FERRIX_WEBCAM_CROP=900:1000:500:40

FERRIX_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
[ -r "$FERRIX_ROOT/out/dev-loop.env" ] && . "$FERRIX_ROOT/out/dev-loop.env"

: "${FERRIX_REMOTE_BRANCH:=mac-handoff}"
: "${FERRIX_SHOTS:=$FERRIX_ROOT/out/dev-loop/shots}"
: "${FERRIX_SHELL:=/dev/cu.usbmodemferrix_tegu3}"
export FERRIX_SHELL
DEVTEST="$FERRIX_ROOT/tools/pixel-9a/devtest"
mkdir -p "$FERRIX_SHOTS"

need() {
	for v in "$@"; do
		eval "x=\${$v-}"
		[ -n "$x" ] || { echo "STOP: set $v in out/dev-loop.env" >&2; exit 2; }
	done
}

# Run a command in the builder checkout, after loading the attempt env.
remote() {
	need FERRIX_REMOTE FERRIX_REMOTE_REPO
	ssh -n "$FERRIX_REMOTE" "cd '$FERRIX_REMOTE_REPO' && $1"
}

# Run one command on the phone over the CDC-ACM bring-up shell.
phone() {
	python3 "$DEVTEST/shell-run.py" "$1" "${2:-5}"
}
