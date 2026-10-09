#!/usr/bin/env python3
"""Run one command on the Ferrix bring-up shell (CDC-ACM) and print its output."""
import os, select, sys, termios, time, tty
port = os.environ.get("FERRIX_SHELL", "/dev/cu.usbmodemferrix_tegu3")
cmd = sys.argv[1]
wait = float(sys.argv[2]) if len(sys.argv) > 2 else 3.0
fd = os.open(port, os.O_RDWR | os.O_NOCTTY | os.O_NONBLOCK)
tty.setraw(fd)
marker = "__FERRIX_DONE_%d__" % os.getpid()
os.write(fd, ("\n%s; echo %s\n" % (cmd, marker)).encode())
out = b""
end = time.time() + wait
while time.time() < end:
    r, _, _ = select.select([fd], [], [], 0.2)
    if r:
        try:
            chunk = os.read(fd, 65536)
        except BlockingIOError:
            continue
        out += chunk
        if out.count(marker.encode()) >= 2:
            break
os.close(fd)
text = out.decode(errors="replace").replace("\r", "")
# drop the echoed command line(s)
lines = [l for l in text.split("\n") if marker not in l and not l.endswith(cmd)]
print("\n".join(lines).strip())
