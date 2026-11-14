#!/usr/bin/env python3
"""Regenerates tests/fixtures/textcolor/: fixtures plus the original text_color.py's stdout.

Run with a venv holding opencv-python-headless + numpy, from the repo root:
  python crates/imgtools/tests/textcolor_golden.py [path/to/scripts/images/text_color.py]
"""
import os, subprocess, sys
import cv2
import numpy as np

here = os.path.dirname(os.path.abspath(__file__))
out = os.path.join(here, "fixtures", "textcolor")
script = sys.argv[1] if len(sys.argv) > 1 else "crates/qml/tests/fixtures/original-unshipped/scripts/images/text_color.py"
os.makedirs(out, exist_ok=True)
rng = np.random.default_rng(7)

def text_like(w, h, bg, fg, noise):
    img = np.empty((h, w, 3), np.int32)
    img[:] = bg
    for _ in range(max(1, w // 12)):
        x, y = rng.integers(0, w), rng.integers(0, h)
        img[y:y + max(1, h // 3), x:x + 2] = fg
    img += rng.integers(-noise, noise + 1, img.shape)
    return np.clip(img, 0, 255).astype(np.uint8)

cases = {
    "dark_on_light.png": text_like(120, 30, (240, 238, 230), (20, 20, 25), 3),
    "light_on_dark.png": text_like(64, 18, (30, 32, 40), (250, 250, 250), 6),
    "gradient.png": np.dstack([np.tile(np.linspace(0, 255, 97), (41, 1))] * 3).astype(np.uint8),
    "flat.png": np.full((10, 10, 3), (12, 200, 99), np.uint8),
    "one_px.png": np.array([[[1, 2, 3]]], np.uint8),
    "two_px.png": np.array([[[1, 2, 3], [200, 100, 50]]], np.uint8),
    "noise.png": rng.integers(0, 256, (33, 77, 3)).astype(np.uint8),
    "colored.png": text_like(200, 48, (60, 120, 200), (255, 220, 0), 10),
}
for name, img in cases.items():
    cv2.imwrite(os.path.join(out, name), img)
cv2.imwrite(os.path.join(out, "photo.jpg"), text_like(150, 40, (200, 180, 160), (40, 10, 90), 20), [cv2.IMWRITE_JPEG_QUALITY, 85])
cv2.imwrite(os.path.join(out, "rgba.png"), np.dstack([cases["colored.png"], rng.integers(0, 256, (48, 200)).astype(np.uint8)]))

for name in sorted(os.listdir(out)):
    if name.endswith(".json"):
        continue
    with open(os.path.join(out, name), "rb") as f:
        res = subprocess.run([sys.executable, script], stdin=f, capture_output=True, check=True)
    with open(os.path.join(out, name + ".json"), "wb") as f:
        f.write(res.stdout)
