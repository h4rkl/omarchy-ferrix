#!/bin/sh
# Runs ON the phone (root shell, Ferrix host side). Measures a fixed APK set in
# the Waydroid container and prints one JSON record per measurement on stdout.
# It starts/stops apps and injects input; it changes no settings. Host wrapper:
# tools/pixel-9a/devtest/apk-bench.sh. Needs the devtest injector
# (/tmp/inj.fifo, skills/ferrix-dev-loop/scripts/stage-devtest.sh).
#
# Spec: plans/todos/10-apk-runtime-performance.md deliverable 1.
#   launch  - `waydroid app launch` as the session user (the path ferrix-appd
#             uses, so the task becomes a Ferrix window), timed to the app's
#             first rendered frame by polling gfxinfo on a /proc/uptime clock.
#             Coarse (+ one waydroid-shell round trip): an upper bound.
#             `am start -W` is not used: under task streams Android reports
#             LaunchState UNKNOWN and no TotalTime.
#   cold    - after am force-stop;  warm - after switching to another app
#   scroll  - gfxinfo reset, N swipes through the Ferrix uinput injector (the
#             real compositor -> Waydroid input path), gfxinfo percentiles/jank
#   cpu     - host /proc/stat busy % across the scroll
#   pss     - dumpsys meminfo TOTAL PSS of the app (KiB)
set -u
PKGS=${FERRIX_BENCH_PKGS:-"org.lineageos.jelly com.android.deskclock com.android.settings"}
OTHER=${FERRIX_BENCH_OTHER:-com.android.calculator2}
SWIPES=${FERRIX_BENCH_SWIPES:-12}
EVIDENCE=physical:google-pixel-9a-tegu-gxq96-mp1-0

ws() { waydroid shell -- "$@" 2>/dev/null; }
activity() { ws cmd package resolve-activity --brief "$1" | tail -n 1 | tr -d '\r'; }
APPD=$(pgrep -u user -f '^/usr/bin/ferrix-appd' | head -n 1)
[ -n "$APPD" ] || { echo 'ferrix-appd is not running' >&2; exit 1; }
tr '\0' '\n' < "/proc/$APPD/environ" | grep -E '^(XDG_RUNTIME_DIR|WAYLAND_DISPLAY|HOME|PATH)=' > /tmp/apk-bench.env
[ -p /tmp/inj.fifo ] || { echo 'devtest injector is not running (stage-devtest.sh)' >&2; exit 1; }
now_ms() { awk '{ printf "%d", $1 * 1000 }' /proc/uptime; }
frames() { ws dumpsys gfxinfo "$1" | sed -n 's/^Total frames rendered: *\([0-9]*\).*/\1/p' | head -n 1; }
launch() { su -s /bin/sh user -c "set -a; . /tmp/apk-bench.env; waydroid app launch $1" >/dev/null 2>&1; }
# Launch $1 and print ms until it has rendered a frame (gfxinfo reset first).
timed_launch() {
	ws dumpsys gfxinfo "$1" reset >/dev/null
	t0=$(now_ms)
	launch "$1"
	while :; do
		f=$(frames "$1"); [ "${f:-0}" -gt 0 ] && break
		[ $(( $(now_ms) - t0 )) -gt 20000 ] && { printf ''; return; }
		sleep 0.1
	done
	printf '%s' $(( $(now_ms) - t0 ))
}
cpu_sample() { awk '/^cpu /{ idle=$5+$6; t=0; for (i=2;i<=NF;i++) t+=$i; print t, idle }' /proc/stat; }
num() { [ -n "$1" ] && printf '%s' "$1" || printf 'null'; }

refresh=$(ws dumpsys SurfaceFlinger --latency | head -n 1 | tr -d '\r')
printf '{"evidence":"%s","event":"apk-bench-env","sf_refresh_period_ns":%s,"kernel":"%s"}\n' \
	"$EVIDENCE" "$(num "$refresh")" "$(uname -r)"

for pkg in $PKGS; do
	act=$(activity "$pkg")
	case "$act" in */*) ;; *) printf '{"evidence":"%s","event":"apk-bench-skip","package":"%s","reason":"no launcher activity"}\n' "$EVIDENCE" "$pkg"; continue ;; esac
	ws am force-stop "$pkg"
	sleep 2
	cold=$(timed_launch "$pkg")
	sleep 4
	launch "$OTHER"
	sleep 4
	warm=$(timed_launch "$pkg")
	sleep 4
	ws dumpsys gfxinfo "$pkg" reset >/dev/null
	set -- $(cpu_sample); t0=$1; i0=$2
	n=0
	while [ "$n" -lt "$SWIPES" ]; do
		if [ $((n % 2)) -eq 0 ]; then echo 'swipe 540 1800 540 700 200' > /tmp/inj.fifo; else echo 'swipe 540 700 540 1800 200' > /tmp/inj.fifo; fi
		n=$((n + 1))
		sleep 0.7
	done
	set -- $(cpu_sample); t1=$1; i1=$2
	busy=$(awk -v t="$((t1 - t0))" -v i="$((i1 - i0))" 'BEGIN { if (t > 0) printf "%.1f", 100 * (t - i) / t }')
	g=$(ws dumpsys gfxinfo "$pkg")
	frames=$(printf '%s\n' "$g" | sed -n 's/^Total frames rendered: *\([0-9]*\).*/\1/p' | head -n 1)
	janky=$(printf '%s\n' "$g" | sed -n 's/^Janky frames: *[0-9]* *(\([0-9.]*\)%).*/\1/p' | head -n 1)
	p50=$(printf '%s\n' "$g" | sed -n 's/^50th percentile: *\([0-9]*\)ms.*/\1/p' | head -n 1)
	p90=$(printf '%s\n' "$g" | sed -n 's/^90th percentile: *\([0-9]*\)ms.*/\1/p' | head -n 1)
	p95=$(printf '%s\n' "$g" | sed -n 's/^95th percentile: *\([0-9]*\)ms.*/\1/p' | head -n 1)
	p99=$(printf '%s\n' "$g" | sed -n 's/^99th percentile: *\([0-9]*\)ms.*/\1/p' | head -n 1)
	pss=$(ws dumpsys meminfo "$pkg" | sed -n 's/^ *TOTAL PSS: *\([0-9]*\).*/\1/p; s/^ *TOTAL *\([0-9][0-9]*\) .*/\1/p' | head -n 1)
	printf '{"evidence":"%s","event":"apk-bench","package":"%s","cold_first_frame_ms":%s,"warm_first_frame_ms":%s,"swipes":%s,"frames":%s,"janky_pct":%s,"p50_ms":%s,"p90_ms":%s,"p95_ms":%s,"p99_ms":%s,"host_cpu_busy_pct":%s,"pss_kib":%s}\n' \
		"$EVIDENCE" "$pkg" "$(num "$cold")" "$(num "$warm")" "$SWIPES" "$(num "$frames")" "$(num "$janky")" \
		"$(num "$p50")" "$(num "$p90")" "$(num "$p95")" "$(num "$p99")" "$(num "$busy")" "$(num "$pss")"
done
total=$(ws dumpsys meminfo | sed -n 's/^ *Total PSS: *\([0-9,]*\)K.*/\1/p; s/^ *Total RAM:.*//' | tr -d , | head -n 1)
[ -n "$total" ] || total=$(ws dumpsys meminfo | awk '/^Total PSS by process:/{p=1;next} p&&/K: /{gsub(/[,K]/,"",$1); t+=$1} /^$/{if(p)exit} END{if(t)print t}')
printf '{"evidence":"%s","event":"apk-bench-container","total_pss_kib":%s}\n' "$EVIDENCE" "$(num "$total")"
