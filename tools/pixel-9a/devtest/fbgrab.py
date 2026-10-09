#!/usr/bin/env python3
"""Read the scanned-out framebuffer of the first active CRTC (root only) and
write it as PPM. Usage: fbgrab.py OUT.ppm [SCALE] [--reversed]"""
import fcntl, mmap, os, struct, sys

GETRES, GETCRTC, GETFB2, MAP_DUMB = 0xC04064A0, 0xC06864A1, 0xC06864CE, 0xC01064B3
out = sys.argv[1]
scale = int(sys.argv[2]) if len(sys.argv) > 2 and sys.argv[2].isdigit() else 1
fd = os.open("/dev/dri/card0", os.O_RDWR)

res = bytearray(64)
fcntl.ioctl(fd, GETRES, res)
n_crtcs = struct.unpack_from("I", res, 36)[0]

import ctypes
buf = ctypes.create_string_buffer(4 * n_crtcs)
res = bytearray(64)
struct.pack_into("QQQQ", res, 0, 0, ctypes.addressof(buf), 0, 0)
struct.pack_into("IIII", res, 32, 0, n_crtcs, 0, 0)
fcntl.ioctl(fd, GETRES, res)
ids = struct.unpack(f"{n_crtcs}I", buf.raw)

fb_id = 0
for cid in ids:
    c = bytearray(104)
    struct.pack_into("I", c, 12, cid)
    fcntl.ioctl(fd, GETCRTC, c)
    fb = struct.unpack_from("I", c, 16)[0]
    if fb:
        fb_id = fb
        break
if not fb_id:
    sys.exit("no active framebuffer")

f = bytearray(104)
struct.pack_into("I", f, 0, fb_id)
fcntl.ioctl(fd, GETFB2, f)
_, w, h, fmt, _ = struct.unpack_from("5I", f, 0)
handle = struct.unpack_from("I", f, 20)[0]
pitch = struct.unpack_from("I", f, 36)[0]
m = bytearray(16)
struct.pack_into("I", m, 0, handle)
fcntl.ioctl(fd, MAP_DUMB, m)
offset = struct.unpack_from("Q", m, 8)[0]
mm = mmap.mmap(fd, pitch * h, mmap.MAP_SHARED, mmap.PROT_READ, offset=offset)
data = mm[:]
code = struct.pack("I", fmt).decode(errors="replace")
# A byte-reversed scanout profile registers the reverse of the rendered code
# (rendered XR24 -> registered BX24); decode the rendered bytes.
code = {"BX24": "XR24", "BA24": "AR24", "RX24": "XB24", "RA24": "AB24"}.get(code, code) if "--reversed" in sys.argv else code
# Byte positions of R,G,B in a little-endian 32-bit pixel for the registered code.
pos = {"XR24": (2, 1, 0), "AR24": (2, 1, 0), "XB24": (0, 1, 2), "AB24": (0, 1, 2),
       "BX24": (1, 2, 3), "BA24": (1, 2, 3), "RX24": (3, 2, 1), "RA24": (3, 2, 1)}.get(code, (2, 1, 0))
ow, oh = w // scale, h // scale
rows = []
for y in range(0, oh * scale, scale):
    row = data[y * pitch: y * pitch + w * 4]
    px = bytearray(ow * 3)
    for i, x in enumerate(range(0, ow * scale, scale)):
        b = x * 4
        px[i * 3] = row[b + pos[0]]
        px[i * 3 + 1] = row[b + pos[1]]
        px[i * 3 + 2] = row[b + pos[2]]
    rows.append(bytes(px))
with open(out, "wb") as o:
    o.write(f"P6\n{ow} {oh}\n255\n".encode())
    o.write(b"".join(rows))
print(f"fb={fb_id} {w}x{h} {code} pitch={pitch} -> {out} ({ow}x{oh})")
