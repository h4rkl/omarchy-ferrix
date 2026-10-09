#!/usr/bin/env python3
"""Fetch a file from the Ferrix bring-up shell: fget.py REMOTE LOCAL (gzip+base64, sha256-checked)."""
import base64, gzip, hashlib, os, re, select, sys, time, tty

port = os.environ.get("FERRIX_SHELL", "/dev/cu.usbmodemferrix_tegu3")
remote, local = sys.argv[1], sys.argv[2]
fd = os.open(port, os.O_RDWR | os.O_NOCTTY | os.O_NONBLOCK)
tty.setraw(fd)
tag = "FG%d" % os.getpid()
# Quote-split markers so the echoed command line never contains them verbatim.
cmd = (f"echo {tag}'SHA' $(sha256sum {remote} | cut -c1-64); echo {tag}'BEGIN'; "
       f"gzip -c {remote} | base64; echo {tag}'END'\n")
os.write(fd, b"\n" + cmd.encode())
out = b""
end = time.time() + 300
while time.time() < end:
    r, _, _ = select.select([fd], [], [], 0.5)
    if r:
        try:
            out += os.read(fd, 1 << 16)
        except BlockingIOError:
            pass
        if (tag + "END").encode() in out.split((tag + "BEGIN").encode())[-1]:
            break
os.close(fd)
text = out.decode(errors="replace").replace("\r", "")
sha = re.findall(tag + r"SHA ([0-9a-f]{64})", text)[-1]
body = text.split(tag + "BEGIN\n")[-1].split(tag + "END")[0]
data = gzip.decompress(base64.b64decode("".join(l for l in body.split("\n") if re.fullmatch(r"[A-Za-z0-9+/=]+", l))))
if hashlib.sha256(data).hexdigest() != sha:
    sys.exit("sha mismatch")
open(local, "wb").write(data)
print(local, len(data))
