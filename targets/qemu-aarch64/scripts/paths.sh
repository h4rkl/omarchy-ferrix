# Shared path resolution for the QEMU scripts (sourced, never executed).
#
# The caller sets `root` (the repository) first. This sets:
#   out         resolved output dir: ${FERRIX_OUT_DIR:-$root/out}/qemu-aarch64
#   runtime     agent-session logs, pid file and frames: $out/agent
#   sock_base   short runtime dir for this out dir's Unix sockets
#   sock_agent  agent-session sockets (qmp.sock and the mock-control sockets)
#   sock_run    qemu-run / qemu-check mock-control sockets
#
# Unix socket paths are limited to 104 bytes on macOS (108 on Linux), and a
# deep checkout (for example a `.claude/worktrees/...` worktree) pushes
# `$out/agent/telephony-control.sock` past that. Sockets therefore live in a
# short per-user directory keyed by the out dir: `${TMPDIR:-/tmp}/ferrix-<uid>-<key>`
# (falling back to /tmp when TMPDIR itself is long), where <key> is the POSIX
# `cksum` of the resolved out path. Two out dirs never share sockets; logs,
# serial captures, frames and evidence stay in the out dir.
# FERRIX_QEMU_SOCKET_DIR overrides the socket base directory.
out=${FERRIX_OUT_DIR:-"$root/out"}/qemu-aarch64
runtime="$out/agent"
if [ -n "${FERRIX_QEMU_SOCKET_DIR:-}" ]; then
  sock_base=$FERRIX_QEMU_SOCKET_DIR
else
  sock_key=$(printf '%s' "$out" | cksum | awk '{ print $1 }')
  sock_tmp=${TMPDIR:-/tmp}
  sock_tmp=${sock_tmp%/}
  sock_base="$sock_tmp/ferrix-$(id -u)-$sock_key"
  # Longest name below the base: "/agent/telephony-control.sock" (29 bytes);
  # keep the whole path under the 104-byte macOS limit with a margin.
  [ $(( ${#sock_base} + 29 )) -lt 100 ] || sock_base="/tmp/ferrix-$(id -u)-$sock_key"
fi
sock_agent="$sock_base/agent"
sock_run="$sock_base/run"
# ferrix_socket_dirs: create the socket dirs, private to this user.
ferrix_socket_dirs() {
  mkdir -p "$sock_agent" "$sock_run"
  chmod 700 "$sock_base" "$sock_agent" "$sock_run"
}
