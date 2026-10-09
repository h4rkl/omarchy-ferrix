#!/usr/bin/env python3
"""Push a file to the Ferrix bring-up shell: shell-put.py LOCAL REMOTE (base64)."""
import base64, os, select, sys, time, tty
port = os.environ.get("FERRIX_SHELL", "/dev/cu.usbmodemferrix_tegu3")
src, dst = sys.argv[1], sys.argv[2]
data = base64.encodebytes(open(src, "rb").read())
fd = os.open(port, os.O_RDWR | os.O_NOCTTY)
tty.setraw(fd)
def drain(t=0.5):
    end = time.time() + t
    out = b""
    while time.time() < end:
        r, _, _ = select.select([fd], [], [], 0.1)
        if r: out += os.read(fd, 65536)
    return out
os.write(fd, b"\nstty -echo; /usr/bin/base64 -d > " + dst.encode() + b"\n")
drain(1)
start = time.time()
view = memoryview(data)
for i in range(0, len(data), 4096):
    os.write(fd, view[i:i+4096])
    if i % (4096 * 64) == 0:
        drain(0.01)
os.write(fd, b"\n\x04")
drain(2)
os.write(fd, b"stty echo\n")
print("sent %d bytes in %.1fs" % (len(data), time.time() - start))
os.close(fd)
