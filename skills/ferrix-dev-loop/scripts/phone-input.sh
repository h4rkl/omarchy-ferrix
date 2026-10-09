#!/bin/sh
# usage: phone-input.sh 'tap X Y' | 'swipe X1 Y1 X2 Y2 [MS]' | 'type TEXT' | 'key NAME'
# Coordinates are panel pixels, portrait 1080x2424. Run stage-devtest.sh first.
. "$(dirname -- "$0")/env.sh"
phone "echo '$1' > /tmp/inj.fifo" 3 >/dev/null
