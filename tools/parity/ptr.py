#!/usr/bin/env python3
"""Real pointer events through uinput, in output pixels: `move X Y`, `click X Y [left|right]`,
`drag X0 Y0 X1 Y1`, `hold X Y`.

`hold` presses the left button at (X,Y), prints `down` and keeps the button down until a line
arrives on stdin (or stdin closes), then releases it there. The button cannot stay down between
two runs of this script: the kernel releases the keys of a uinput device when it is closed.

The device reports absolute positions (like QEMU's tablet), so the compositor puts the pointer
exactly where asked: relative motion goes through pointer acceleration and a drag ended up to
24 px away from run to run. PARITY_OUTPUT=WxH is the output the coordinates are in.

The compositor is Hyprland, and the script goes on when it has answered, not after a pause:
the first event is sent once it lists the device, the device is closed once it has read the
last event. With pauses instead (0.4 s and 0.1 s) a compositor that was busy for longer lost
events: `move 1300 700` left the pointer at 1299, the pixel `arrive` goes through, or where it
was. PARITY_INPUT_DIR is where the kernel puts the device's node (/dev/input).
"""
import ctypes
import os
import select
import struct
import subprocess
import sys
import time

from evdev import AbsInfo, UInput, ecodes as e

W, H = (int(v) for v in os.environ.get("PARITY_OUTPUT", "1920x1080").split("x"))
# SUB device units per pixel, and every position is the middle of its pixel: the compositor
# scales the axis in floating point, and a pixel's edge can round into its neighbour.
SUB = 16
# Every open of a device node from here on, as inotify events: the compositor takes a device by
# opening its node (logind or seatd opens it for the compositor).
IN_OPEN, TAKEN_S = 0x20, 30
libc = ctypes.CDLL(None, use_errno=True)
opens = libc.inotify_init1(os.O_CLOEXEC)
if opens < 0 or libc.inotify_add_watch(opens, os.environ.get("PARITY_INPUT_DIR", "/dev/input").encode(), IN_OPEN) < 0:
    sys.exit(f"ptr.py: cannot watch the device nodes: {os.strerror(ctypes.get_errno())}")
# The name is this run's own: the compositor may still list the device of the run before.
NAME = f"parity-ptr-{os.getpid()}"
ui = UInput({
    e.EV_KEY: [e.BTN_LEFT, e.BTN_RIGHT],
    e.EV_ABS: [(e.ABS_X, AbsInfo(0, 0, W * SUB - 1, 0, 0, 0)), (e.ABS_Y, AbsInfo(0, 0, H * SUB - 1, 0, 0, 0))],
}, name=NAME)


def hyprctl(request):
    """The compositor's answer. It reads its devices and its control socket in one loop, in the
    order they became readable, and reads all a device has: the answer to a request made after
    an event comes after it has read the event."""
    out = subprocess.run(["hyprctl", request], stdout=subprocess.PIPE, text=True)
    if out.returncode:
        sys.exit(f"ptr.py: the compositor did not answer `hyprctl {request}`")
    return out.stdout


def opened(node):
    """Returns when a process has opened the device's node since the last call."""
    while True:
        if not select.select([opens], [], [], TAKEN_S)[0]:
            sys.exit(f"ptr.py: the compositor did not take the device in {TAKEN_S} s")
        events, at, found = os.read(opens, 4096), 0, False
        while at < len(events):
            size = struct.unpack_from("iIII", events, at)[3]
            found |= events[at + 16:at + 16 + size].rstrip(b"\0") == node
            at += 16 + size
        if found:
            return


# What is sent before the compositor has the device moves nothing, and the compositor hears of
# the device from udev some time after it exists. It lists the device once it has opened it;
# an open by another process (python-evdev's own) is followed by the next one.
while NAME not in hyprctl("devices"):
    opened(os.path.basename(ui.device.path).encode())


def to(x, y):
    ui.write(e.EV_ABS, e.ABS_X, x * SUB + SUB // 2)
    ui.write(e.EV_ABS, e.ABS_Y, y * SUB + SUB // 2)
    ui.syn()
    time.sleep(0.016)


def arrive(x, y):
    # Through the neighbouring pixel: the kernel drops an absolute event that repeats the device's
    # last value (0,0 on a new device), and the client needs a motion to see the pointer enter.
    to(x - 1 if x else 1, y)
    to(x, y)
    time.sleep(0.05)


def button(b, v):
    ui.write(e.EV_KEY, b, v)
    ui.syn()
    time.sleep(0.06)


cmd, args = sys.argv[1], sys.argv[2:]
if cmd == "move":
    arrive(int(args[0]), int(args[1]))
elif cmd == "click":
    arrive(int(args[0]), int(args[1]))
    b = e.BTN_RIGHT if args[2:] == ["right"] else e.BTN_LEFT
    button(b, 1)
    button(b, 0)
elif cmd == "drag":
    x0, y0, x1, y1 = (int(v) for v in args)
    arrive(x0, y0)
    button(e.BTN_LEFT, 1)
    n = 20
    for k in range(1, n + 1):
        to(x0 + (x1 - x0) * k // n, y0 + (y1 - y0) * k // n)
    time.sleep(0.05)
    button(e.BTN_LEFT, 0)
elif cmd == "hold":
    arrive(int(args[0]), int(args[1]))
    button(e.BTN_LEFT, 1)
    print("down", flush=True)
    sys.stdin.readline()
    button(e.BTN_LEFT, 0)
else:
    sys.exit(f"ptr.py: unknown command {cmd}")
# Closing the device throws away what the compositor has not read from it yet: evdev's read
# fails with ENODEV before it hands out the events it has queued.
hyprctl("cursorpos")
ui.close()
