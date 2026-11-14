#!/usr/bin/env python3
"""Regenerates the least_busy_region equivalence goldens from the original script.

Usage (from the repo root, in a venv with `opencv-contrib-python-headless numpy`):

    python crates/imgtools/tests/busy_golden.py            # rewrite goldens from existing fixtures
    python crates/imgtools/tests/busy_golden.py --fixtures # also regenerate the synthetic fixtures

Writes:
  tests/busy_golden.tsv  fixture \t exit code \t stdout (\\ and \n escaped) \t args (tab-separated)
                         of scripts/images/least_busy_region.py (crates/qml/tests/fixtures/original-unshipped)
  tests/busy_stages.tsv  fixture \t screen WxH \t mode \t md5 of each OpenCV stage
                         (imread gray, resized+cropped gray, imread color, resized+cropped color)
"""
import hashlib
import os
import struct
import subprocess
import sys
import zlib

import cv2
import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
FIX = os.path.join(HERE, "fixtures", "busy")
ROOT = os.path.abspath(os.path.join(HERE, "..", "..", ".."))
SCRIPT = os.path.join(
    ROOT, "crates/qml/tests/fixtures/original-unshipped/scripts/images/least_busy_region.py"
)

SHELL = ["--screen-width", "1920", "--screen-height", "1080", "--width", "300", "--height", "300",
         "--horizontal-padding", "200", "--vertical-padding", "200"]


def shell(sw, sh):
    a = list(SHELL)
    a[1], a[3] = str(sw), str(sh)
    return a


ARGSETS = [
    SHELL,
    SHELL + ["--busiest"],
    shell(2560, 1440),
    shell(1366, 768),
    shell(1080, 1920),
    shell(1279, 719) + ["--busiest"],
    SHELL + ["--screen-mode", "fit"],
    shell(300, 200),
    [],
    ["--screen-width", "640", "--screen-height", "360", "--width", "100", "--height", "80",
     "--horizontal-padding", "20", "--vertical-padding", "20", "--stride", "1"],
]
# CLI edge cases (argparse, verbose, largest-region mode, odd geometry), run on EDGE_FIXTURES only.
EDGE = [
    ["--largest-region"],
    ["--largest-region", "--aspect-ratio", "0.5", "-t", "50", "--stride", "3"],
    ["-l", "-t", "0"],
    ["-l", "--aspect-ratio", "inf"],
    ["-l", "--aspect-ratio", "-2"],
    ["-l", "--verbose", "--screen-mode", "fit"],
    ["--verbose"] + SHELL,
    ["--verbose", "--screen-width", "120", "--screen-height", "67", "--width", "500"],
    ["--screen-width", "0"],
    ["--screen-width", "-100"],
    ["--screen-height", "-50", "--screen-mode", "fit"],
    ["--width", "0"],
    ["--width", "-20", "--height", "-30"],
    ["--horizontal-padding", "-30", "--vertical-padding", "-10"],
    ["--horizontal-padding", "5000"],
    ["--stride", "0"],
    ["--stride", "-3", "--busiest"],
    ["--width", "5000", "--height", "5000", "--busiest"],
    ["--wid", "200", "--busi"],
    ["--width=250", "-hp", "10", "-vp", "10"],
    ["--h", "5"],
    ["--bogus"],
    ["--width", "abc"],
    ["--width", " 12 "],
    ["--screen-mode", "stretch"],
    ["extra_positional"],
    ["--", "--width"],
]
EDGE_FIXTURES = ["synth_photo.jpg", "grad_rgb.png"]
STAGE_SCREENS = [(1920, 1080, "fill"), (1366, 768, "fill"), (1920, 1080, "fit")]


def png_chunk(tag, data):
    c = tag + data
    return struct.pack(">I", len(data)) + c + struct.pack(">I", zlib.crc32(c) & 0xffffffff)


def png_with_chunks(png, chunks):
    """Inserts ancillary chunks right after IHDR."""
    assert png[:8] == b"\x89PNG\r\n\x1a\n"
    ihdr_end = 8 + 8 + struct.unpack(">I", png[8:12])[0] + 4
    return png[:ihdr_end] + b"".join(png_chunk(t, d) for t, d in chunks) + png[ihdr_end:]


def write_png(name, img, chunks=(), palette=None, trns=None):
    if palette is not None:
        # Hand-rolled 8-bit palette PNG (cv2 cannot write one).
        h, w = img.shape
        ihdr = struct.pack(">IIBBBBB", w, h, 8, 3, 0, 0, 0)
        raw = b"".join(b"\x00" + img[y].astype(np.uint8).tobytes() for y in range(h))
        cs = [(b"IHDR", ihdr), *chunks, (b"PLTE", palette.astype(np.uint8).tobytes())]
        if trns is not None:
            cs.append((b"tRNS", bytes(trns)))
        cs += [(b"IDAT", zlib.compress(raw, 9)), (b"IEND", b"")]
        data = b"\x89PNG\r\n\x1a\n" + b"".join(png_chunk(t, d) for t, d in cs)
    else:
        ok, buf = cv2.imencode(".png", img, [cv2.IMWRITE_PNG_COMPRESSION, 9])
        assert ok
        data = png_with_chunks(buf.tobytes(), chunks)
    with open(os.path.join(FIX, name), "wb") as f:
        f.write(data)


def write_gray_lowbit(name, img, bits):
    """Hand-rolled gray PNG with 1/2/4-bit samples (cv2 only writes 8/16-bit)."""
    h, w = img.shape
    v = (img.astype(np.uint16) >> (8 - bits)).astype(np.uint8)
    per = 8 // bits
    rows = []
    for y in range(h):
        r = np.zeros((w + per - 1) // per, np.uint8)
        for x in range(w):
            r[x // per] |= v[y, x] << (8 - bits * (x % per + 1))
        rows.append(b"\x00" + r.tobytes())
    ihdr = struct.pack(">IIBBBBB", w, h, bits, 0, 0, 0, 0)
    cs = [(b"IHDR", ihdr), (b"IDAT", zlib.compress(b"".join(rows), 9)), (b"IEND", b"")]
    with open(os.path.join(FIX, name), "wb") as f:
        f.write(b"\x89PNG\r\n\x1a\n" + b"".join(png_chunk(t, d) for t, d in cs))


def write_jpg(name, img, params=(), app1=None):
    ok, buf = cv2.imencode(".jpg", img, list(params))
    assert ok
    data = buf.tobytes()
    if app1 is not None:
        seg = b"\xff\xe1" + struct.pack(">H", len(app1) + 2) + app1
        data = data[:2] + seg + data[2:]
    with open(os.path.join(FIX, name), "wb") as f:
        f.write(data)


def exif_orientation(o):
    tiff = b"MM\x00\x2a" + struct.pack(">I", 8) + struct.pack(">H", 1)
    tiff += struct.pack(">HHIHH", 0x0112, 3, 1, o, 0) + struct.pack(">I", 0)
    return b"Exif\x00\x00" + tiff


def landscape(w, h, rng, noise=6.0):
    """Photo-like: sky gradient, sun, two hill silhouettes, film grain."""
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    img = np.zeros((h, w, 3))
    t = y / h
    img[..., 0] = 230 - 90 * t
    img[..., 1] = 180 - 60 * t
    img[..., 2] = 120 + 60 * t
    sun = np.exp(-((x - 0.7 * w) ** 2 + (y - 0.3 * h) ** 2) / (0.004 * w * w))
    img += sun[..., None] * np.array([40, 120, 135])
    hill1 = h * (0.62 + 0.08 * np.sin(x / w * 5.0))
    hill2 = h * (0.75 + 0.05 * np.sin(x / w * 9.0 + 1.0))
    img[y > hill1] = [60, 110, 70]
    img[y > hill2] = [30, 70, 40]
    img += rng.normal(0, noise, img.shape)
    return np.clip(img, 0, 255).astype(np.uint8)


def gradient(w, h):
    y, x = np.mgrid[0:h, 0:w]
    return np.dstack([(x * 255 // max(w - 1, 1)), (y * 255 // max(h - 1, 1)),
                      ((x + y) * 255 // max(w + h - 2, 1))]).astype(np.uint8)


def make_fixtures():
    rng = np.random.default_rng(20261031)
    srgb = [(b"sRGB", b"\x00")]
    gama = lambda g: [(b"gAMA", struct.pack(">I", g))]
    write_jpg("synth_photo.jpg", landscape(480, 300, rng, 4), [cv2.IMWRITE_JPEG_QUALITY, 85])
    write_jpg("portrait_444.jpg", landscape(180, 300, rng, 3),
              [cv2.IMWRITE_JPEG_QUALITY, 85, cv2.IMWRITE_JPEG_SAMPLING_FACTOR, cv2.IMWRITE_JPEG_SAMPLING_FACTOR_444])
    write_jpg("progressive.jpg", landscape(240, 180, rng, 6), [cv2.IMWRITE_JPEG_PROGRESSIVE, 1])
    write_jpg("gray.jpg", cv2.cvtColor(landscape(200, 150, rng, 3), cv2.COLOR_BGR2GRAY))
    write_jpg("exif6.jpg", landscape(160, 100, rng, 3), app1=exif_orientation(6))
    write_jpg("exif3.jpg", landscape(100, 160, rng, 3), app1=exif_orientation(3))
    write_jpg("black.jpg", np.zeros((90, 160, 3), np.uint8))
    write_jpg("tall.jpg", landscape(150, 900, rng, 3), [cv2.IMWRITE_JPEG_QUALITY, 70])
    write_png("grad_rgb.png", gradient(400, 225))
    write_png("noise_rgb.png", rng.integers(0, 256, (48, 64, 3), dtype=np.uint8))
    write_png("flat_gray.png", np.full((64, 64), 137, np.uint8))
    dark = np.zeros((120, 200, 3), np.uint8)
    dark[rng.random((120, 200)) < 0.02] = [200, 40, 90]
    dark[40:60, 150:170] = 5
    write_png("dark_sparse.png", dark)
    rgba = np.dstack([landscape(120, 80, rng, 2), gradient(120, 80)[..., 1]])
    write_png("rgba.png", rgba)
    pal = rng.integers(0, 256, (16, 3))
    idx = (gradient(200, 150)[..., 0] // 16).astype(np.uint8)
    write_png("palette_trns.png", idx, palette=pal, trns=[0, 128, 255])
    write_png("palette_srgb.png", idx, chunks=srgb, palette=pal)
    photo = landscape(96, 64, rng, 2)
    write_png("srgb.png", photo, srgb)
    write_png("gama045.png", photo, gama(45455))
    write_png("gama1.png", photo, gama(100000))
    write_png("gama_then_srgb.png", photo, gama(70000) + srgb)
    write_png("gray_gama.png", cv2.cvtColor(photo, cv2.COLOR_BGR2GRAY), gama(45455))
    write_png("exif6.png", landscape(96, 64, rng, 2), [(b"eXIf", exif_orientation(6)[6:])])
    write_gray_lowbit("gray2.png", cv2.cvtColor(landscape(90, 60, rng, 2), cv2.COLOR_BGR2GRAY), 2)
    p16 = (landscape(80, 60, rng, 2).astype(np.uint16) * 257 + rng.integers(0, 257, (60, 80, 3))).astype(np.uint16)
    write_png("rgb16.png", p16)
    write_png("rgb16_gama.png", p16, gama(45455))
    write_png("rgb16_sbit_srgb.png", p16, [(b"sBIT", b"\x0a\x0a\x0a")] + srgb)
    write_png("rgba16.png", np.dstack([p16, np.full((60, 80), 40000, np.uint16)]))
    write_png("gray16.png", cv2.cvtColor(p16, cv2.COLOR_BGR2GRAY))
    write_png("narrow_103x80.png", gradient(103, 80))
    write_png("wide_1200x535.png", gradient(1200, 535)[:, :, ::-1].copy())
    write_png("tiny.png", rng.integers(0, 256, (20, 30, 3), dtype=np.uint8))


def scaled(img, sw, sh, mode):
    oh, ow = img.shape[:2]
    sx, sy = sw / ow, sh / oh
    s = max(sx, sy) if mode == "fill" else min(sx, sy)
    img = cv2.resize(img, (int(ow * s), int(oh * s)), interpolation=cv2.INTER_LANCZOS4)
    h, w = img.shape[:2]
    if w == sw and h == sh:
        return img
    x1, y1 = max(0, (w - sw) // 2), max(0, (h - sh) // 2)
    return img[y1:y1 + sh, x1:x1 + sw]


def md5(a):
    return "%dx%dx%d:%s" % (a.shape[1], a.shape[0], 1 if a.ndim == 2 else a.shape[2],
                            hashlib.md5(np.ascontiguousarray(a).tobytes()).hexdigest())


def main():
    os.makedirs(FIX, exist_ok=True)
    if "--fixtures" in sys.argv:
        make_fixtures()
    names = sorted(os.listdir(FIX))
    cases = [(n, a) for n in names for a in ARGSETS]
    cases += [(n, a) for n in EDGE_FIXTURES for a in EDGE]
    cases += [("missing.png", SHELL)]
    rows = []
    for n, args in cases:
        # Arguments are tab-joined so values with spaces survive; the image path goes first.
        p = subprocess.run([sys.executable, SCRIPT, os.path.join(FIX, n), *args],
                           capture_output=True, text=True)
        out = p.stdout.replace("\\", "\\\\").replace("\n", "\\n")
        assert "\t" not in out, out
        rows.append("%s\t%d\t%s\t%s\n" % (n, p.returncode, out, "\t".join(args)))
    with open(os.path.join(HERE, "busy_golden.tsv"), "w") as f:
        f.writelines(rows)
    stages = []
    for n in names:
        g = cv2.imread(os.path.join(FIX, n), cv2.IMREAD_GRAYSCALE)
        c = cv2.imread(os.path.join(FIX, n))
        for sw, sh, mode in STAGE_SCREENS:
            stages.append("\t".join([n, "%dx%d" % (sw, sh), mode, md5(g), md5(scaled(g, sw, sh, mode)),
                                     md5(c), md5(scaled(c, sw, sh, mode))]) + "\n")
    with open(os.path.join(HERE, "busy_stages.tsv"), "w") as f:
        f.writelines(stages)
    print("opencv", cv2.__version__, "numpy", np.__version__, "cases", len(rows))


if __name__ == "__main__":
    main()
