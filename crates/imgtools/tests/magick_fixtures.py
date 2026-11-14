#!/usr/bin/env python3
"""Regenerates the input corpus in tests/fixtures/magick/ (needs pillow + numpy).

Goldens are produced from these by tests/magick_golden.sh on a desktop with ImageMagick 7.
  python crates/imgtools/tests/magick_fixtures.py
"""
import os, struct, zlib
import numpy as np
from PIL import Image

here = os.path.dirname(os.path.abspath(__file__))
out = os.path.join(here, "fixtures", "magick")
os.makedirs(out, exist_ok=True)
rng = np.random.default_rng(11)


def photo(w, h, ch=3, depth=8):
    """Smooth gradients + noise + hard edges (exercises Lanczos ringing and clamping)."""
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    base = [
        0.5 + 0.5 * np.sin(x / max(w, 1) * 7 + y / max(h, 1) * 3),
        x / max(w - 1, 1),
        0.5 + 0.5 * np.cos(np.hypot(x - w / 3, y - h / 2) / 9),
    ]
    img = np.dstack(base[:ch] if ch <= 3 else base)
    img[(x.astype(int) // 17 + y.astype(int) // 13) % 5 == 0] = 1.0
    img[(x.astype(int) % 29) < 2] = 0.0
    img = img + rng.normal(0, 0.03, img.shape)
    top = 65535 if depth == 16 else 255
    return np.clip(np.rint(img * top), 0, top).astype(np.uint16 if depth == 16 else np.uint8)


def alpha(w, h, depth=8):
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    a = np.clip(x / max(w - 1, 1) * 1.4 - 0.2 + 0.2 * np.sin(y / 5), 0, 1)
    a[(y.astype(int) // 9) % 4 == 0] = 0
    top = 65535 if depth == 16 else 255
    return np.rint(a * top).astype(np.uint16 if depth == 16 else np.uint8)


def png16(path, arr):
    """Pillow can't write 16-bit RGB(A); emit the PNG by hand."""
    h, w = arr.shape[:2]
    ch = 1 if arr.ndim == 2 else arr.shape[2]
    ctype = {1: 0, 2: 4, 3: 2, 4: 6}[ch]
    raw = b"".join(b"\0" + arr[r].astype(">u2").tobytes() for r in range(h))

    def chunk(t, d):
        return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)

    with open(path, "wb") as f:
        f.write(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 16, ctype, 0, 0, 0))
                + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b""))


def save(name, img, **kw):
    img.save(os.path.join(out, name), **kw)


rgb = photo(320, 180)
save("rgb420.jpg", Image.fromarray(rgb), quality=88)
save("rgb444.jpg", Image.fromarray(rgb), quality=92, subsampling=0)
save("rgb422.jpg", Image.fromarray(photo(203, 117)), quality=80, subsampling=1)
save("progressive.jpg", Image.fromarray(photo(300, 200)), quality=85, progressive=True)
save("gray.jpg", Image.fromarray(photo(190, 250)[:, :, 1]), quality=90)
save("cmyk.jpg", Image.fromarray(photo(240, 160)).convert("CMYK"), quality=90)
exif = Image.Exif()
exif[0x0112] = 6
save("exif_rot6.jpg", Image.fromarray(photo(280, 150)), quality=90, exif=exif.tobytes())
save("wide.jpg", Image.fromarray(photo(1400, 37)), quality=90)
save("tall.jpg", Image.fromarray(photo(23, 900)), quality=90)

save("rgb.png", Image.fromarray(rgb))
save("rgba.png", Image.fromarray(np.dstack([photo(260, 190), alpha(260, 190)]), "RGBA"))
save("rgba_opaque.png", Image.fromarray(np.dstack([photo(150, 210), np.full((210, 150), 255, np.uint8)]), "RGBA"))
save("gray.png", Image.fromarray(photo(170, 230)[:, :, 0]))
save("graya.png", Image.fromarray(np.dstack([photo(200, 140)[:, :, 2], alpha(200, 140)]), "LA"))
pal = Image.fromarray(photo(210, 150)).quantize(64, dither=Image.Dither.NONE)
save("palette.png", pal)
palt = Image.fromarray(photo(180, 120)).quantize(32, dither=Image.Dither.NONE)
save("palette_trns.png", palt, transparency=3)
save("bilevel.png", Image.fromarray(photo(160, 100)[:, :, 0] > 128))
png16(os.path.join(out, "rgb16.png"), photo(230, 170, depth=16))
png16(os.path.join(out, "rgba16.png"), np.dstack([photo(190, 190, depth=16), alpha(190, 190, 16)]))
png16(os.path.join(out, "gray16.png"), photo(150, 200, depth=16)[:, :, 0])
png16(os.path.join(out, "rgb16_reducible.png"), photo(140, 100).astype(np.uint16) * 257)
save("small.png", Image.fromarray(photo(37, 23)))
save("small_rgba.png", Image.fromarray(np.dstack([photo(45, 30), alpha(45, 30)]), "RGBA"))
save("wide.png", Image.fromarray(photo(2000, 9)))
save("tall.png", Image.fromarray(photo(7, 1300)))
save("exact256.png", Image.fromarray(photo(256, 144)))
save("square.png", Image.fromarray(photo(300, 300)))
save("onepx.png", Image.fromarray(photo(1, 1)))

save("lossy.webp", Image.fromarray(photo(250, 170)), quality=80)
save("lossless.webp", Image.fromarray(photo(190, 130)), lossless=True)
save("alpha.webp", Image.fromarray(np.dstack([photo(220, 160), alpha(220, 160)]), "RGBA"), quality=85)
