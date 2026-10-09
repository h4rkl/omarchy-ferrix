#!/bin/sh
# usage: stage-devtest.sh -- push fbgrab.py and inject.py to the phone and start the injector.
# Needed once per boot (/tmp is tmpfs).
. "$(dirname -- "$0")/env.sh"
for f in fbgrab.py inject.py; do
	python3 "$DEVTEST/shell-put.py" "$DEVTEST/$f" "/tmp/$f" >/dev/null
done
phone 'pkill -f /tmp/inject.py; (python3 /tmp/inject.py > /tmp/inject.log 2>&1 &); sleep 2; ls /tmp/inj.fifo' 8
