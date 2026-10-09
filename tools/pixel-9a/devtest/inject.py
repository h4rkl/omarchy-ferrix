#!/usr/bin/env python3
"""On-device test input: a virtual touchscreen + keyboard via /dev/uinput.

Run as root in the background; send commands to /tmp/inj.fifo:
  tap X Y | swipe X1 Y1 X2 Y2 [MS] | type TEXT | key NAME
"""
import fcntl, os, struct, time

W, H = 1080, 2424
UI_SET_EVBIT, UI_SET_KEYBIT, UI_SET_ABSBIT, UI_SET_PROPBIT = 0x40045564, 0x40045565, 0x40045567, 0x4004556E
UI_DEV_CREATE = 0x5501
EV_SYN, EV_KEY, EV_ABS = 0, 1, 3
BTN_TOUCH = 0x14A
ABS_X, ABS_Y, ABS_MT_SLOT, ABS_MT_POSITION_X, ABS_MT_POSITION_Y, ABS_MT_TRACKING_ID = 0, 1, 0x2F, 0x35, 0x36, 0x39


def make(name, setup, absrange=None):
    fd = os.open("/dev/uinput", os.O_WRONLY | os.O_NONBLOCK)
    setup(fd)
    absmax = [0] * 64
    absmin = [0] * 64
    for code, (lo, hi) in (absrange or {}).items():
        absmin[code], absmax[code] = lo, hi
    dev = struct.pack("80sHHHHi", name.encode(), 3, 0x1234, 0x5678, 1, 0)
    dev += struct.pack("64i", *absmax) + struct.pack("64i", *absmin) + struct.pack("64i", *[0] * 64) * 2
    os.write(fd, dev)
    fcntl.ioctl(fd, UI_DEV_CREATE)
    return fd


def ev(fd, t, c, v):
    now = time.time()
    os.write(fd, struct.pack("qqHHi", int(now), int((now % 1) * 1e6), t, c, v))


def syn(fd):
    ev(fd, EV_SYN, 0, 0)


def touch_setup(fd):
    fcntl.ioctl(fd, UI_SET_EVBIT, EV_KEY)
    fcntl.ioctl(fd, UI_SET_KEYBIT, BTN_TOUCH)
    fcntl.ioctl(fd, UI_SET_EVBIT, EV_ABS)
    for code in (ABS_X, ABS_Y, ABS_MT_SLOT, ABS_MT_POSITION_X, ABS_MT_POSITION_Y, ABS_MT_TRACKING_ID):
        fcntl.ioctl(fd, UI_SET_ABSBIT, code)
    fcntl.ioctl(fd, UI_SET_PROPBIT, 1)  # INPUT_PROP_DIRECT


def kbd_setup(fd):
    fcntl.ioctl(fd, UI_SET_EVBIT, EV_KEY)
    for code in range(1, 249):
        fcntl.ioctl(fd, UI_SET_KEYBIT, code)


touch = make("ferrix-test-touch", touch_setup, {
    ABS_X: (0, W - 1), ABS_Y: (0, H - 1), ABS_MT_POSITION_X: (0, W - 1),
    ABS_MT_POSITION_Y: (0, H - 1), ABS_MT_SLOT: (0, 1), ABS_MT_TRACKING_ID: (0, 65535)})
kbd = make("ferrix-test-keyboard", kbd_setup)
tracking = [0]


def down(x, y):
    tracking[0] += 1
    ev(touch, EV_ABS, ABS_MT_SLOT, 0)
    ev(touch, EV_ABS, ABS_MT_TRACKING_ID, tracking[0])
    ev(touch, EV_ABS, ABS_MT_POSITION_X, x)
    ev(touch, EV_ABS, ABS_MT_POSITION_Y, y)
    ev(touch, EV_ABS, ABS_X, x)
    ev(touch, EV_ABS, ABS_Y, y)
    ev(touch, EV_KEY, BTN_TOUCH, 1)
    syn(touch)


def move(x, y):
    ev(touch, EV_ABS, ABS_MT_POSITION_X, x)
    ev(touch, EV_ABS, ABS_MT_POSITION_Y, y)
    ev(touch, EV_ABS, ABS_X, x)
    ev(touch, EV_ABS, ABS_Y, y)
    syn(touch)


def up():
    ev(touch, EV_ABS, ABS_MT_TRACKING_ID, -1)
    ev(touch, EV_KEY, BTN_TOUCH, 0)
    syn(touch)


KEYS = {c: k for k, c in enumerate("1234567890", start=2)}
KEYS.update({c: k for c, k in zip("qwertyuiop", range(16, 26))})
KEYS.update({c: k for c, k in zip("asdfghjkl", range(30, 39))})
KEYS.update({c: k for c, k in zip("zxcvbnm", range(44, 51))})
KEYS.update({" ": 57, "-": 12, ".": 52, "/": 53, ",": 51, "@": None})
NAMED = {"enter": 28, "backspace": 14, "esc": 1, "tab": 15, "up": 103, "down": 108, "left": 105, "right": 106}
SHIFTED = {"!": 2, "@": 3, "#": 4, "$": 5, "%": 6, "^": 7, "&": 8, "*": 9, "(": 10, ")": 11, "_": 12}


def press(code, shift=False):
    if shift:
        ev(kbd, EV_KEY, 42, 1); syn(kbd)
    ev(kbd, EV_KEY, code, 1); syn(kbd)
    ev(kbd, EV_KEY, code, 0); syn(kbd)
    if shift:
        ev(kbd, EV_KEY, 42, 0); syn(kbd)
    time.sleep(0.03)


def type_text(text):
    for ch in text:
        if ch in SHIFTED:
            press(SHIFTED[ch], True)
        elif ch.lower() in KEYS and KEYS[ch.lower()]:
            press(KEYS[ch.lower()], ch.isupper())


fifo = "/tmp/inj.fifo"
if not os.path.exists(fifo):
    os.mkfifo(fifo)
while True:
    with open(fifo) as f:
        for line in f:
            parts = line.split(None, 1)
            if not parts:
                continue
            cmd, rest = parts[0], (parts[1].rstrip("\n") if len(parts) > 1 else "")
            try:
                if cmd == "tap":
                    x, y = map(int, rest.split())
                    down(x, y); time.sleep(0.06); up()
                elif cmd == "swipe":
                    v = list(map(int, rest.split()))
                    x1, y1, x2, y2 = v[:4]
                    ms = v[4] if len(v) > 4 else 250
                    steps = max(5, ms // 16)
                    down(x1, y1)
                    for i in range(1, steps + 1):
                        move(x1 + (x2 - x1) * i // steps, y1 + (y2 - y1) * i // steps)
                        time.sleep(ms / 1000 / steps)
                    up()
                elif cmd == "type":
                    type_text(rest)
                elif cmd == "key":
                    press(NAMED[rest.strip()])
            except Exception as e:  # keep serving
                print("inj error:", e, flush=True)
