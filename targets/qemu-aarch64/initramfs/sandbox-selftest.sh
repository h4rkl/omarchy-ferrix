#!/bin/sh
# Sandbox negative proof (issue #10), run by init in check mode before the
# services start. Each check starts ferrix-sandbox (the same binary
# ferrix-appd launches applications through) with an application that tries
# one forbidden action, and prints a typed record:
#
#   {"evidence":"QEMU","event":"sandbox-denied","check":NAME,"status":"ok"|"failed","exit":N}
#
# then one summary, which qemu-check requires:
#
#   {"evidence":"QEMU","event":"sandbox-selftest","status":"ok"|"failed","checks":N,"failed":[...]}
#
# The sandbox's own sandbox-ready records (namespaces, cgroup, network,
# fs_view, seccomp, identity) land on the same console. Evidence: QEMU only.
set -u

sandbox=/usr/bin/ferrix-sandbox
app=org.ferrix.selftest
checks=0
failed=""

# check NAME EXPECTED_EXIT POLICY_JSON SCRIPT
check() {
  name=$1 expected=$2 policy=$3 script=$4
  checks=$((checks + 1))
  "$sandbox" --policy "$policy" --app-id "$app" --evidence QEMU -- sh -c "$script" </dev/null
  status=$?
  if [ "$status" = "$expected" ]; then
    result=ok
  else
    result=failed
    failed="$failed${failed:+,}\"$name\""
  fi
  printf '{"evidence":"QEMU","event":"sandbox-denied","check":"%s","status":"%s","exit":%s,"expected":%s}\n' \
    "$name" "$result" "$status" "$expected"
}

if [ ! -x "$sandbox" ]; then
  printf '{"evidence":"QEMU","event":"sandbox-selftest","status":"failed","reason":"ferrix-sandbox-missing"}\n'
  exit 1
fi

# The default policy: every control on (minimal root, loopback-only network,
# seccomp, cgroup). `{}` means "IsolationPolicy::default()".
default='{}'

# Forbidden path: the daemons' data and the host's writable system tree are
# outside the minimal root; /usr is read-only.
check forbidden-path 0 "$default" '
  [ -e /var/lib/ferrix ] && exit 10
  touch /usr/ferrix-probe 2>/dev/null && exit 11
  [ -x /bin/sh ] || exit 12
  exit 0'

# Forbidden system calls: a root application inside its own namespaces
# would succeed at each of these without the filter.
check forbidden-syscall 0 "$default" '
  mkdir -p /tmp/m
  mount -t tmpfs none /tmp/m 2>/dev/null && exit 20
  unshare -n true 2>/dev/null && exit 21
  chroot / true 2>/dev/null && exit 22
  grep -q "^Seccomp:[[:space:]]*2" /proc/self/status || exit 23
  exit 0'

# No network without the grant: only lo exists, and it is up.
check no-network 0 "$default" '
  [ "$(tail -n +3 /proc/net/dev | cut -d: -f1 | tr -d " ")" = lo ] || exit 30
  ping -c 1 -W 2 127.0.0.1 >/dev/null 2>&1 || exit 31
  exit 0'

# pids.max: a fork loop is refused (the child shell dies at the first
# refused fork; the parent reads its own group's pids.events).
check pids-limit 0 '{"resources":{"pids_max":16}}' '
  sh -c "i=0; while [ \$i -lt 64 ]; do sleep 5 & i=\$((i+1)); done" 2>/dev/null
  group=$(cut -d: -f3 /proc/self/cgroup)
  grep -q "^max [1-9]" "/sys/fs/cgroup$group/pids.events" || exit 40
  exit 0'

# memory.max: 48 MiB into the private tmpfs under a 24 MiB limit ends in
# one OOM kill of the whole group (the supervisor too: 128 + SIGKILL).
check memory-limit 137 '{"resources":{"memory_max":25165824}}' '
  exec dd if=/dev/zero of=/tmp/fill bs=1M count=48 2>/dev/null'

# Per-app uid: the application runs as its own subuid with no capability,
# so a root-only file is unreadable.
check per-app-uid 0 '{"uid":"per-app"}' '
  [ "$(id -u)" != 0 ] || exit 50
  [ -e /etc/shadow ] || exit 51
  cat /etc/shadow >/dev/null 2>&1 && exit 52
  grep -q "^CapEff:[[:space:]]*0*$" /proc/self/status || exit 53
  exit 0'

if [ -z "$failed" ]; then
  printf '{"evidence":"QEMU","event":"sandbox-selftest","status":"ok","checks":%s,"failed":[]}\n' "$checks"
else
  printf '{"evidence":"QEMU","event":"sandbox-selftest","status":"failed","checks":%s,"failed":[%s]}\n' "$checks" "$failed"
  exit 1
fi
