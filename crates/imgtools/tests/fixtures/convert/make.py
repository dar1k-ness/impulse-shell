#!/usr/bin/env python3
"""Writes the fixtures of this directory: files whose pixels each reader brings to 8 bits, turns
or flattens in its own way (16 bits per sample, an Orientation tag, a `tRNS` chunk, colour that
OpenCV reads as grey). The tests compare our readers on them with the original ones, live.

Usage (in the testenv image): /opt/ri-venv/bin/python make.py [OUT_DIR]
"""
import os
import struct
import sys
import zlib

import numpy as np
from PIL import Image

out = sys.argv[1] if len(sys.argv) > 1 else os.path.dirname(os.path.abspath(__file__))


def p(name):
    return os.path.join(out, name)


def base(w, h):
    """Every quarter differs, so a turn or a crop of the wrong region changes the result."""
    y, x = np.mgrid[0:h, 0:w]
    a = np.zeros((h, w, 3), np.uint8)
    a[..., 0] = 255 * x // (w - 1)
    a[..., 1] = 255 * y // (h - 1)
    a[..., 2] = (x * 3 + y * 5) % 256
    a[: h // 3, : w // 3] = (18, 20, 18)
    a[4 : h // 3 - 4 : 6, 6 : w // 3 - 6] = (230, 228, 235)
    a[h // 2 :, w // 2 :] = (236, 232, 234)
    a[h // 2 + 6 :: 9, w // 2 + 8 : -8] = (40, 30, 90)
    a[h // 2 : h // 2 + h // 6, : w // 4] = (250, 20, 150)
    return a


def ramp(w, h):
    y, x = np.mgrid[0:h, 0:w]
    return (255 * (x + y) // (w + h - 2)).astype(np.uint8)


def deep(a):
    """8-bit samples as the high byte of 16-bit ones, under every low byte."""
    low = np.random.default_rng(7).integers(0, 256, a.shape, dtype=np.uint16)
    return a.astype(np.uint16) * 256 + low


def png(name, a, colour_type, trns=None):
    """A PNG of the array `a` (h, w, channels), 8 or 16 bits per sample by its dtype."""
    h, w = a.shape[:2]
    depth = a.dtype.itemsize * 8
    rows = a.astype(">u2" if depth == 16 else np.uint8).reshape(h, -1)
    raw = b"".join(b"\0" + r.tobytes() for r in rows)

    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

    with open(p(name), "wb") as f:
        f.write(b"\x89PNG\r\n\x1a\n")
        f.write(chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, depth, colour_type, 0, 0, 0)))
        if trns is not None:
            f.write(chunk(b"tRNS", trns))
        f.write(chunk(b"IDAT", zlib.compress(raw, 9)))
        f.write(chunk(b"IEND", b""))


def tiff(name, a, extra=None):
    """An uncompressed little-endian TIFF of `a` (h, w, channels); `extra` is ExtraSamples."""
    h, w, c = a.shape
    depth = a.dtype.itemsize * 8
    data = a.astype("<u2" if depth == 16 else np.uint8).tobytes()
    tags = [
        (256, 4, 1, w),
        (257, 4, 1, h),
        (258, 3, c, [depth] * c),
        (259, 3, 1, 1),
        (262, 3, 1, 2 if c >= 3 else 1),
        (273, 4, 1, None),
        (277, 3, 1, c),
        (278, 4, 1, h),
        (279, 4, 1, len(data)),
        (284, 3, 1, 1),
    ]
    if extra is not None:
        tags.append((338, 3, 1, extra))
    tags.sort()
    ifd_at = 8
    after = ifd_at + 2 + 12 * len(tags) + 4
    blobs = b""
    entries = b""
    for tag, kind, n, v in tags:
        if tag == 273:
            v = None
        if isinstance(v, list) and n > 2:
            off = after + len(blobs)
            blobs += struct.pack("<%dH" % n, *v)
            entries += struct.pack("<HHII", tag, kind, n, off)
        elif isinstance(v, list):
            entries += struct.pack("<HHI", tag, kind, n) + struct.pack("<%dH" % n, *v).ljust(4, b"\0")
        elif v is None:
            entries += struct.pack("<HHII", tag, kind, n, 0)
        elif kind == 3:
            entries += struct.pack("<HHIHH", tag, kind, n, v, 0)
        else:
            entries += struct.pack("<HHII", tag, kind, n, v)
    strip = after + len(blobs)
    i = [t[0] for t in tags].index(273) * 12
    entries = entries[:i] + struct.pack("<HHII", 273, 4, 1, strip) + entries[i + 12 :]
    with open(p(name), "wb") as f:
        f.write(b"II*\0" + struct.pack("<I", ifd_at))
        f.write(struct.pack("<H", len(tags)) + entries + struct.pack("<I", 0))
        f.write(blobs + data)


def exif(orientation):
    e = Image.Exif()
    e[0x0112] = orientation
    return e


# 128 / 210 is no binary fraction, so OpenCV's downscale to 128 pixels of the long side rounds
# differently once the image is turned.
W, H = 210, 140
b8 = base(W, H)
im = Image.fromarray(b8)
small = base(48, 32)
small_a = np.dstack([small, ramp(48, 32)])

# 16 bits per sample.
png("rgb16.png", deep(small), 2)
png("rgba16.png", deep(small_a), 6)
png("gray16.png", deep(small[..., 1:2]), 0)
png("graya16.png", deep(small_a[..., [1, 3]]), 4)
key = deep(small)
key[8:16, 30:40] = (0x1234, 0x5678, 0x9ABC)
png("rgb16_trns.png", key, 2, trns=struct.pack(">HHH", 0x1234, 0x5678, 0x9ABC))
tiff("rgb16.tiff", deep(small))
tiff("rgba16.tiff", deep(small_a), extra=2)
tiff("rgba16_assoc.tiff", deep(small_a), extra=1)
tiff("gray16.tiff", deep(small[..., 1:2]))

# An Orientation tag: EXIF in JPEG, PNG and WebP, the TIFF tag itself in TIFF.
im.save(p("exif3.jpg"), quality=90, exif=exif(3))
im.save(p("exif6.jpg"), quality=90, exif=exif(6))
im.save(p("exif6.png"), exif=exif(6), optimize=True)
im.save(p("exif5.png"), exif=exif(5), optimize=True)
im.save(p("exif6.webp"), quality=85, exif=exif(6))
im.save(p("exif8.webp"), lossless=True, exif=exif(8))
im.save(p("exif3.tiff"), compression="tiff_adobe_deflate", tiffinfo={274: 3})
im.save(p("exif6.tiff"), compression="tiff_adobe_deflate", tiffinfo={274: 6})

# A `tRNS` chunk: a palette with alpha, and the transparent colour of a grey and an RGB PNG.
# They are larger than 128x128, so the colour script resizes them: with alpha it would do that
# on premultiplied colour.
im.quantize(64).save(p("pal_trns.png"), transparency=bytes((i * 4) % 256 for i in range(64)))
g = im.convert("L")
g.save(p("gray_trns.png"), transparency=int(np.bincount(np.asarray(g).ravel()).argmax()))
im.save(p("rgb_trns.png"), transparency=(236, 232, 234), optimize=True)

# Colour read as grey from formats other than JPEG and PNG, and alpha in a TIFF.
rgba = im.copy()
rgba.putalpha(Image.fromarray(ramp(W, H)))
im.save(p("rgb.webp"), quality=85)
rgba.save(p("rgba.webp"), quality=85)
im.save(p("rgb_lossless.webp"), lossless=True)
im.resize((64, 48)).save(p("rgb.bmp"))
im.save(p("rgb.gif"))
im.save(p("rgb.tiff"), compression="tiff_adobe_deflate")
tiff("rgba.tiff", small_a, extra=2)
tiff("rgba_assoc.tiff", small_a, extra=1)
