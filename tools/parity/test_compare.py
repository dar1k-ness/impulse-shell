#!/usr/bin/env python3
"""Checks compare.py's verdicts on drawn shots: python3 tools/parity/test_compare.py

Rasteriser noise (a third of the pixels of a text-like pattern and their neighbours 35 to 40 of
255 off, a few thousand pixels) must pass with exit code 0; a small part that moved, changed
colour or is missing, or a measured time further apart than its limit, must give 1; a missing shot,
a shot of another size and a shot equal to the idle screen must give 2. A `layer:` mask must hide
its rectangle only on a shot taken with that layer mapped, and an area with limits of its own must
catch a part a shade lighter, which the whole-shot limit passes.
"""
import json
import os
import subprocess
import sys
import tempfile

import numpy as np
from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
W, H = 640, 360


def screen(panel=True, icon_x=100, icon=(255, 180, 170), noise=0):
    rng = np.random.default_rng(1)
    img = np.full((H, W, 3), 30, np.uint8)
    if panel:
        img[40:300, 60:400] = 45
        for y in range(60, 280, 12):          # "text": 1 px strokes with gaps
            xs = rng.integers(80, 380, 60)
            img[y:y + 7, xs] = 220
        img[20:36, icon_x:icon_x + 16] = icon
    if noise:                                  # antialiasing: stroke pixels and their neighbours a shade off
        r2 = np.random.default_rng(noise)
        edge = np.argwhere(img[..., 0] == 220)
        pick = edge[r2.random(len(edge)) < 0.3]
        img[pick[:, 0], pick[:, 1]] = 180
        side = img[pick[:, 0], pick[:, 1] + 1]
        img[pick[:, 0], pick[:, 1] + 1] = np.where(side == 45, 80, side)
    return img


def run(a, b, extra_a=(), extra_b=(), life=None, layers=None, masks=None, areas=None):
    with tempfile.TemporaryDirectory() as t:
        opts = []
        if masks is not None:
            json.dump(masks, open(f"{t}/masks.json", "w"))
            opts += ["--masks", f"{t}/masks.json"]
        if areas is not None:
            policy = json.load(open(os.path.join(HERE, "policy.json")))
            policy["areas"] = areas
            json.dump(policy, open(f"{t}/policy.json", "w"))
            opts += ["--policy", f"{t}/policy.json"]
        for side, shot, extra in (("a", a, extra_a), ("b", b, extra_b)):
            os.mkdir(os.path.join(t, side))
            Image.fromarray(screen(panel=False)).save(os.path.join(t, side, "idle.png"))
            if shot is not None:
                Image.fromarray(shot).save(os.path.join(t, side, "shot.png"))
            for name, im in extra:
                Image.fromarray(im).save(os.path.join(t, side, name))
            if life:
                open(os.path.join(t, side, "osd-life.log"), "w").write(f"osdVolume {life[side == 'b']} ms\n")
            if layers is not None:
                open(os.path.join(t, side, "layers.txt"), "w").write(f"shot {layers}\n")
        r = subprocess.run([sys.executable, os.path.join(HERE, "compare.py"), f"{t}/a", f"{t}/b", f"{t}/out"] + opts,
                           capture_output=True, text=True)
        return r.returncode, r.stderr.strip().splitlines()[-1].split(';')[0]


ICON_MASK = {"layer:panel+bar": [[96, 16, 28, 24, "the icon"]]}
ICON_AREA = {"shot": [[100, 20, 16, 16, 2, 4, "the icon"]]}
LIGHTER = (235, 160, 150)   # the icon 20 of 255 lighter or darker: under COARSE_DELTA

CASES = [
    ("equal shots", 0, lambda: run(screen(), screen())),
    ("rasteriser noise on every text edge", 0, lambda: run(screen(), screen(noise=7))),
    ("an icon moved by 2 px", 1, lambda: run(screen(), screen(icon_x=102))),
    ("an icon of another colour", 1, lambda: run(screen(), screen(icon=(60, 60, 60)))),
    ("noise plus a moved icon", 1, lambda: run(screen(), screen(icon_x=102, noise=7))),
    ("the OSD stays 1040 and 1100 ms", 0, lambda: run(screen(), screen(), life=(1040, 1100))),
    ("the OSD stays 1040 and 1240 ms", 1, lambda: run(screen(), screen(), life=(1040, 1240))),
    ("a shot missing on one side", 2, lambda: run(screen(), None)),
    ("a shot of another size", 2, lambda: run(screen(), screen()[:300])),
    ("a shot equal to the idle screen", 2, lambda: run(screen(panel=False), screen(panel=False))),
    ("a moved icon under a mask of the layers that are mapped", 0,
     lambda: run(screen(), screen(icon_x=102), layers="bar panel", masks=ICON_MASK)),
    ("a moved icon under a mask of a layer that is not mapped", 1,
     lambda: run(screen(), screen(icon_x=102), layers="bar", masks=ICON_MASK)),
    ("a moved icon under a layer mask, a run without layers.txt", 1,
     lambda: run(screen(), screen(icon_x=102), masks=ICON_MASK)),
    ("an icon a shade off, no area", 0, lambda: run(screen(), screen(icon=LIGHTER))),
    ("an icon a shade off, in an area with limits of its own", 1,
     lambda: run(screen(), screen(icon=LIGHTER), areas=ICON_AREA)),
    ("rasteriser noise, an equal icon in an area with limits of its own", 0,
     lambda: run(screen(), screen(noise=7), areas=ICON_AREA)),
]

bad = 0
for what, want, case in CASES:
    code, line = case()
    ok = code == want
    bad += not ok
    print(f"{'ok  ' if ok else 'FAIL'} {what}: exit {code}, expected {want} ({line})")
sys.exit(1 if bad else 0)
