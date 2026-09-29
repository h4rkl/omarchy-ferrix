#!/bin/sh

# Read-only correlation helper for Pixel 9a bootloader sessions.  Fastboot
# commonly reports variant=N/A, so a target workflow may use a prior redacted
# ADB discovery only when its serial hash and the complete Android identity
# match the exact target.  The helper prints a matching transcript path and
# never contacts a device.

pixel9a_adb_correlation_file() {
	correlation_prefix=$1
	correlation_root=${FERRIX_OUT_DIR:-"$root/out"}
	[ -n "$correlation_prefix" ] || return 1
	[ -d "$correlation_root" ] || return 1

	find "$correlation_root" -type f -name 'discovery-*.txt' -print 2>/dev/null | sort -r |
	while IFS= read -r candidate; do
		[ -f "$candidate" ] || continue
		awk -v prefix="$correlation_prefix" '
			$0 == "transport=adb" { transport = 1 }
			$0 == "result=ok" { result = 1 }
			$0 == "serial_sha256_prefix=" prefix { serial = 1 }
			$0 == "ro.product.manufacturer=Google" || $0 == "identity.manufacturer=Google" { manufacturer = 1 }
			$0 == "ro.product.model=Pixel 9a" || $0 == "identity.model=Pixel 9a" { model = 1 }
			$0 == "ro.product.device=tegu" || $0 == "identity.codename=tegu" { codename = 1 }
			$0 == "ro.boot.hardware.revision=MP1.0" || $0 == "identity.hardware_revision=MP1.0" { revision = 1 }
			$0 == "ro.boot.hardware.sku=GXQ96" || $0 == "identity.sku=GXQ96" { sku = 1 }
			END { exit !(transport && result && serial && manufacturer && model && codename && revision && sku) }
		' "$candidate" || continue
		printf '%s\n' "$candidate"
		exit 0
	done
}

pixel9a_fastboot_sku_gate() {
	observed_variant=$1
	correlation_prefix=$2
	case "$observed_variant" in
		GXQ96)
			pixel9a_sku_source=fastboot
			pixel9a_sku_evidence=fastboot-variant
			return 0
			;;
		N/A|NA|unknown|unavailable|'')
			pixel9a_sku_correlation=$(pixel9a_adb_correlation_file "$correlation_prefix" || true)
			[ -n "$pixel9a_sku_correlation" ] || return 1
			pixel9a_sku_source=adb-correlated
			pixel9a_sku_evidence=$pixel9a_sku_correlation
			return 0
			;;
		*) return 1 ;;
	esac
}
