#!/bin/sh
# Offline checks for the discovery-only ALSA UCM profile of
# google-pixel-9a-tegu-gxq96-mp1-0. No device, ALSA, or PipeWire is used.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
package_dir="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/device-google-tegu"
apkbuild="$package_dir/APKBUILD"
entry="$package_dir/ucm-aoc-snd-card.conf"
verb="$package_dir/ucm-tegu-HiFi.conf"
compose="$root/tools/pixel-9a/dev-compose.py"
rootfs="$root/tools/pixel-9a/rootfs.sh"
inventory="$root/tools/pixel-9a/audio-inventory-remote.sh"

# Facts from the physical inventory (physical: google-pixel-9a-tegu-gxq96-mp1-0):
# /proc/asound/cards driver "aoc-snd-card", PCM 00-00 "EP1 playback",
# PCM 00-08 "EP1 capture", and the "TAS25XX ASI Switch" mixer control.
card_driver=aoc-snd-card
entry_dest=/usr/share/alsa/ucm2/conf.d/$card_driver/$card_driver.conf
verb_dest=/usr/share/alsa/ucm2/Google/tegu/HiFi.conf

fail() {
	echo "FAIL: $*" >&2
	exit 1
}

sha512_file() {
	if command -v sha512sum >/dev/null 2>&1; then
		sha512sum "$1" | awk '{print $1}'
	else
		shasum -a 512 "$1" | awk '{print $1}'
	fi
}

# UCM text without comments, for policy checks.
code() {
	sed -e 's/#.*$//' "$1"
}

for file in "$entry" "$verb"; do
	[ -f "$file" ] || fail "missing UCM file: $file"
	[ ! -L "$file" ] || fail "UCM file must not be a symlink: $file"
done

# --- Package integration -----------------------------------------------------
grep -Eq '^[[:space:]]*alsa-ucm-conf[[:space:]]*$' "$apkbuild" ||
	fail 'device package does not depend on alsa-ucm-conf (ucm2/ucm.conf lookup)'
for name in ucm-aoc-snd-card.conf ucm-tegu-HiFi.conf; do
	grep -Eq "^[[:space:]]+$name\$" "$apkbuild" || fail "APKBUILD source list lacks $name"
	expected=$(sha512_file "$package_dir/$name")
	grep -q "^$expected  $name\$" "$apkbuild" || fail "APKBUILD sha512sums entry for $name is stale"
	grep -q " $name " "$rootfs" || grep -q " $name;" "$rootfs" ||
		fail "rootfs.sh does not require $name"
done
grep -A1 '"$srcdir"/ucm-aoc-snd-card.conf' "$apkbuild" | grep -q "\"\$pkgdir\"$entry_dest\$" ||
	fail "APKBUILD does not install the conf.d entry at $entry_dest"
grep -A1 '"$srcdir"/ucm-tegu-HiFi.conf' "$apkbuild" | grep -q "\"\$pkgdir\"$verb_dest\$" ||
	fail "APKBUILD does not install the HiFi verb at $verb_dest"
grep -B1 '"$srcdir"/ucm-' "$apkbuild" | grep -q 'install -Dm644' ||
	fail 'UCM files must be installed 0644'
grep -q "(\"ucm-aoc-snd-card.conf\", \"$entry_dest\")" "$compose" ||
	fail 'dev-compose does not stage the conf.d entry at the APKBUILD path'
grep -q "(\"ucm-tegu-HiFi.conf\", \"$verb_dest\")" "$compose" ||
	fail 'dev-compose does not stage the HiFi verb at the APKBUILD path'
# The inventory reports a matched profile by this exact conf.d layout.
grep -q '"$ucm_dir/conf.d/$driver"' "$inventory" ||
	fail 'audio inventory no longer detects conf.d/<driver> UCM profiles'

# --- Card match and exact-target guard ---------------------------------------
[ "$(sed -n '1p' "$entry")" = 'Syntax 4' ] || fail 'conf.d entry must start with Syntax 4'
grep -q '^Syntax' "$verb" && fail 'verb file must inherit Syntax from the conf.d entry'
model=$(sed -n "s/^expected_model='\\(.*\\)'\$/\\1/p" "$inventory")
[ -n "$model" ] || fail 'cannot read the pinned device-tree model from the audio inventory'
grep -q 'String1 "${sys:firmware/devicetree/base/model}"' "$entry" ||
	fail 'conf.d entry does not read the device-tree model'
grep -q "String2 \"$model\"" "$entry" ||
	fail "conf.d entry does not require the exact model '$model'"
grep -q 'Control "name='"'"'TAS25XX ASI Switch'"'"'"' "$entry" ||
	fail 'conf.d entry does not require the observed TAS25xx control'
[ "$(code "$entry" | grep -c 'SectionUseCase\.')" -eq 1 ] || fail 'expected exactly one use case'
code "$entry" | grep -q 'SectionUseCase\."HiFi"' || fail 'use case must be HiFi'
file_ref=$(sed -n 's/^[[:space:]]*File "\(.*\)"$/\1/p' "$entry")
[ "/usr/share/alsa/ucm2$file_ref" = "$verb_dest" ] ||
	fail "use case File '$file_ref' does not resolve to $verb_dest"
# The guard must wrap the use case: it sits at the deepest nesting level.
awk '
	{ line = $0; sub(/#.*/, "", line) }
	line ~ /SectionUseCase/ { at = depth }
	{ depth += gsub(/\{/, "{", line) - gsub(/\}/, "}", line) }
	END { exit !(at == 4) }
' "$entry" || fail 'use case is not nested inside both If guards'

# --- Devices and PCMs --------------------------------------------------------
[ "$(code "$verb" | grep -c 'SectionDevice\.')" -eq 2 ] || fail 'expected exactly two devices'
code "$verb" | grep -q 'SectionDevice\."Speaker"' || fail 'missing Speaker device'
code "$verb" | grep -q 'SectionDevice\."Mic"' || fail 'missing Mic device'
code "$verb" | grep -q 'PlaybackPCM "hw:${CardId},0"' || fail 'Speaker must use PCM 0 (EP1 playback)'
code "$verb" | grep -q 'CapturePCM "hw:${CardId},8"' || fail 'Mic must use PCM 8 (EP1 capture)'
[ "$(code "$verb" | grep -c 'PlaybackPCM')" -eq 1 ] || fail 'only the Speaker may define a playback PCM'
[ "$(code "$verb" | grep -c 'CapturePCM')" -eq 1 ] || fail 'only the Mic may define a capture PCM'
for value in 'PlaybackChannels 2' 'PlaybackRate 48000' 'CaptureChannels 2' 'CaptureRate 48000'; do
	code "$verb" | grep -q "$value" || fail "missing $value"
done
# ferrix-audio-wireplumber classifies sinks by these description tokens.
grep -q 'Comment "Speaker"' "$verb" || fail 'Speaker description must classify as a speaker route'

# --- Discovery-only safety ---------------------------------------------------
# ACP runs verb and device sequences at session start, and alsactl runs boot
# sequences. Neither file may write a control, run a command, bind a hardware
# volume, or name calibration data.
for file in "$entry" "$verb"; do
	for keyword in EnableSequence DisableSequence TransitionSequence BootSequence \
		FixedBootSequence cset cset-bin-file cset-tlv cset-new ctl-remove cdev \
		exec shell sysw cfg-save sleep msleep usleep \
		PlaybackVolume PlaybackSwitch PlaybackMixer PlaybackMixerElem PlaybackMasterElem \
		CaptureVolume CaptureSwitch CaptureMixer CaptureMixerElem CaptureMasterElem \
		JackControl JackDev Include; do
		if code "$file" | grep -Eq "(^|[^A-Za-z])$keyword([^A-Za-z-]|\$)"; then
			fail "$(basename "$file") uses '$keyword'; the profile must stay discovery-only"
		fi
	done
	if grep -Eiq 'persist|calib|efs|speaker\.cal|smartamp|ti_sp_tuning' "$file"; then
		fail "$(basename "$file") references per-unit calibration or protected storage"
	fi
	awk '
		{ line = $0; sub(/#.*/, "", line)
		  depth += gsub(/\{/, "{", line) - gsub(/\}/, "}", line)
		  if (depth < 0) exit 1 }
		END { exit depth != 0 }
	' "$file" || fail "$(basename "$file") has unbalanced braces"
done

echo "Pixel 9a discovery-only UCM profile checks passed (offline; no device or audio)."
