#!/usr/bin/env python3
"""Regenerate fixtures and goldens for tests/regions.rs from the original script.

Usage: regions_golden.py [--fixtures] [--script PATH] [--python PATH]
Reference env: tools/testenv/requirements.txt (opencv-contrib-python-headless==4.13.0.92,
numpy==2.3.5), on a CPU with AVX2 and FMA3.

OpenCV picks its filter code by CPU: the AVX2 build fuses multiply-adds, so a blurred image
differs in its last bits from the SSE4.1 build's and a few region lists differ with it.
golden.tsv is the script as it runs on such a CPU, golden_noavx2.tsv the same run with
OPENCV_CPU_DISABLE=AVX2.
"""
import argparse, os, subprocess, sys
import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
DIR = os.path.join(HERE, "regions")
ROOT = os.path.abspath(os.path.join(HERE, "../../.."))
SCRIPT = os.path.join(
    ROOT, "crates/qml/tests/fixtures/original-unshipped/scripts/images/find_regions.py"
)


def gradient(w, h, rng):
    c0, c1 = rng.integers(0, 256, 3), rng.integers(0, 256, 3)
    t = (np.arange(w)[None, :] / w * 0.6 + np.arange(h)[:, None] / h * 0.4)[..., None]
    return (c0 * (1 - t) + c1 * t).astype(np.uint8)


def ui(w, h, seed, windows):
    rng = np.random.default_rng(seed)
    img = gradient(w, h, rng)
    for _ in range(windows):
        ww, wh = int(rng.integers(w // 8, w // 2)), int(rng.integers(h // 8, h // 2))
        x, y = int(rng.integers(0, w - ww)), int(rng.integers(0, h - wh))
        img[y:y + wh, x:x + ww] = rng.integers(20, 240, 3)
        img[y:y + 28, x:x + ww] = rng.integers(0, 256, 3)
        for _ in range(int(rng.integers(3, 12))):
            bw, bh = int(rng.integers(20, max(21, ww // 3))), int(rng.integers(8, max(9, wh // 4)))
            bx, by = x + int(rng.integers(0, max(1, ww - bw))), y + 30 + int(rng.integers(0, max(1, wh - bh - 30)))
            img[by:by + bh, bx:bx + bw] = rng.integers(0, 256, 3)
    img[h - 40:] = (30, 30, 30)
    return img


def photo(w, h, seed, noise):
    rng = np.random.default_rng(seed)
    img = gradient(w, h, rng).astype(np.float64)
    yy, xx = np.mgrid[0:h, 0:w]
    for _ in range(8):
        cx, cy, r = rng.uniform(0, w), rng.uniform(0, h), rng.uniform(w / 10, w / 3)
        m = np.exp(-((xx - cx) ** 2 + (yy - cy) ** 2) / (2 * r * r))[..., None]
        img = img * (1 - m) + rng.integers(0, 256, 3) * m
    img += rng.normal(0, noise, img.shape)
    return np.clip(img, 0, 255).astype(np.uint8)


def make_fixtures():
    import cv2
    os.makedirs(DIR, exist_ok=True)
    png = [cv2.IMWRITE_PNG_COMPRESSION, 9]
    cv2.imwrite(os.path.join(DIR, "ui_1920x1080.png"), ui(1920, 1080, 1, 6), png)
    cv2.imwrite(os.path.join(DIR, "ui_2560x1440.png"), ui(2560, 1440, 2, 8), png)
    cv2.imwrite(os.path.join(DIR, "ui_1366x768.png"), ui(1366, 768, 3, 4), png)
    cv2.imwrite(os.path.join(DIR, "ui_tall_600x1600.png"), ui(600, 1600, 4, 3), png)
    cv2.imwrite(os.path.join(DIR, "photo_800x600.jpg"), photo(800, 600, 5, 4), [cv2.IMWRITE_JPEG_QUALITY, 75])
    cv2.imwrite(os.path.join(DIR, "noise_160x120.png"), photo(160, 120, 6, 2), png)
    cv2.imwrite(os.path.join(DIR, "flat_800x600.png"), np.full((600, 800, 3), (90, 140, 200), np.uint8), png)
    cv2.imwrite(os.path.join(DIR, "small_333x187.png"), ui(333, 187, 7, 2), png)
    cv2.imwrite(os.path.join(DIR, "tiny_5x5.png"), ui(5, 5, 8, 0)[:5, :5], png)
    cv2.imwrite(os.path.join(DIR, "ui2_2560x1440.png"), ui(2560, 1440, 110, 8), png)


# The call RegionSelection.qml makes: `--hyprctl --image P --max-width W --max-height H`, the
# maxima half the screen's size, rounded as Math.round does. The port takes nothing else.
def half(n):
    return str((n + 1) // 2)


CASES = [
    (img, ["--hyprctl", "--max-width", half(w), "--max-height", half(h)])
    for img, w, h in [
        ("ui_1920x1080.png", 1920, 1080),
        ("ui_2560x1440.png", 2560, 1440),
        ("ui2_2560x1440.png", 2560, 1440),
        ("ui_1366x768.png", 1366, 768),
        ("ui_tall_600x1600.png", 600, 1600),
        ("photo_800x600.jpg", 800, 600),
        ("noise_160x120.png", 160, 120),
        ("flat_800x600.png", 800, 600),
        ("small_333x187.png", 333, 187),
        ("tiny_5x5.png", 5, 5),
        ("missing.png", 1920, 1080),
    ]
] + [
    # No upper limit in effect, and one that leaves nothing.
    ("ui_1920x1080.png", ["--hyprctl", "--max-width", "100000", "--max-height", "100000"]),
    ("ui_2560x1440.png", ["--hyprctl", "--max-width", "0", "--max-height", "-5"]),
]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--fixtures", action="store_true")
    ap.add_argument("--script", default=SCRIPT)
    ap.add_argument("--python", default=sys.executable)
    ap.add_argument("--out", default=DIR)
    a = ap.parse_args()
    if a.fixtures:
        make_fixtures()
    flags = open("/proc/cpuinfo").read().split()
    assert "avx2" in flags and "fma" in flags, "golden.tsv needs a CPU with AVX2 and FMA3"
    for name, disable in [("golden.tsv", ""), ("golden_noavx2.tsv", "AVX2")]:
        env = dict(os.environ, OPENCV_CPU_DISABLE=disable)
        golden = []
        for img, args in CASES:
            argv = ["--image", os.path.join(DIR, img)] + args
            p = subprocess.run([a.python, a.script] + argv, capture_output=True, text=True, env=env)
            assert "\t" not in p.stdout and p.stdout.count("\n") <= 1
            golden.append("\t".join([img, " ".join(args), str(p.returncode), p.stdout.rstrip("\n")]))
            print(name, img, args, p.returncode, len(p.stdout), file=sys.stderr)
        with open(os.path.join(a.out, name), "w") as f:
            f.write("# image\targs\texit code\tstdout (minus print's newline)\n")
            f.write("\n".join(golden) + "\n")


if __name__ == "__main__":
    main()
