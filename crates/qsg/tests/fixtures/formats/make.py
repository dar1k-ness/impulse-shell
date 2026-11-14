#!/opt/ri-venv/bin/python
"""Regenerates the fixtures of this directory (Pillow 13 of tools/testenv, `magick`).

cmyk.jpg          200x150 CMYK JPEG as Pillow writes it (Adobe marker, transform 0).
ycck.jpg          the same picture as YCCK (Adobe marker, transform 2), written by ImageMagick.
cmyk_plain.jpg    cmyk.jpg without its Adobe marker (libjpeg still reads four components as CMYK).
rgb420.avif       200x150 opaque AVIF, 4:2:0 (chroma is upsampled on decode).
rgba.avif         the same with an alpha gradient, 4:4:4.
gray.avif         200x150 4:0:0.
multi.ico         8x8 red, 4x4 green, 16x16 blue BMP entries, in that order.
png.ico           one 11x16 palette PNG entry, one 16x16 RGBA BMP entry.
graypng.ico       one 8x8 grey PNG entry.
opaque.gif        40x30, no transparent colour.
transparent.gif   40x30 with a transparent colour.
unused.gif        opaque.gif's pixels, a transparent colour declared and not used.
"""

import io
import struct
import subprocess
from pathlib import Path

from PIL import Image

HERE = Path(__file__).resolve().parent
W, H = 200, 150


def picture():
    """A wallpaper-like picture: gradients, two saturated blocks, a grey ramp."""
    img = Image.new("RGB", (W, H))
    px = img.load()
    for y in range(H):
        for x in range(W):
            px[x, y] = (x * 255 // (W - 1), y * 255 // (H - 1), (x * 3 + y * 5) % 256)
    for y in range(20, 90):
        for x in range(30, 120):
            px[x, y] = (247, 42, 165)
    for y in range(70, 140):
        for x in range(100, 190):
            px[x, y] = (20, 200 + (x + y) % 40, 40)
    for y in range(120, 150):
        for x in range(0, 80):
            px[x, y] = (x * 3, x * 3, x * 3)
    return img


def segments(jpeg):
    """(marker, bytes) of the segments before SOS, then the rest."""
    out, i = [], 2
    while jpeg[i + 1] != 0xDA:
        (n,) = struct.unpack(">H", jpeg[i + 2 : i + 4])
        out.append((jpeg[i + 1], jpeg[i : i + 2 + n]))
        i += 2 + n
    return out, jpeg[i:]


def ico(entries):
    """An ICO file of `(width, height, bit count, data)` entries, in that order."""
    head = struct.pack("<HHH", 0, 1, len(entries))
    offset = 6 + 16 * len(entries)
    body = b""
    for w, h, bits, data in entries:
        head += struct.pack("<BBBBHHII", w, h, 0, 0, 1, bits, len(data), offset + len(body))
        body += data
    return head + body


def dib(w, h, rgba):
    """A 32-bit BMP icon entry of one colour: BITMAPINFOHEADER, pixels bottom-up, AND mask."""
    head = struct.pack("<IiiHHIIiiII", 40, w, 2 * h, 1, 32, 0, 0, 0, 0, 0, 0)
    r, g, b, a = rgba
    mask_row = (w + 31) // 32 * 4
    return head + bytes([b, g, r, a]) * (w * h) + bytes(mask_row * h)


def png(img):
    buf = io.BytesIO()
    img.save(buf, "PNG")
    return buf.getvalue()


def main():
    pic = picture()
    cmyk = pic.convert("CMYK")
    cmyk.save(HERE / "cmyk.jpg", quality=90)
    jpeg = (HERE / "cmyk.jpg").read_bytes()
    segs, rest = segments(jpeg)
    assert any(m == 0xEE for m, _ in segs)
    (HERE / "cmyk_plain.jpg").write_bytes(
        jpeg[:2] + b"".join(s for m, s in segs if m != 0xEE) + rest
    )
    pic.save(HERE / "src.png")
    subprocess.run(
        ["magick", HERE / "src.png", "-colorspace", "CMYK", "-quality", "90", "-strip",
         HERE / "ycck.jpg"],
        check=True,
    )
    (HERE / "src.png").unlink()

    pic.save(HERE / "rgb420.avif", quality=80, subsampling="4:2:0")
    rgba = pic.convert("RGBA")
    rgba.putalpha(Image.linear_gradient("L").resize((W, H)))
    rgba.save(HERE / "rgba.avif", quality=80, subsampling="4:4:4")
    pic.convert("L").save(HERE / "gray.avif", quality=80, subsampling="4:0:0")

    (HERE / "multi.ico").write_bytes(
        ico([
            (8, 8, 32, dib(8, 8, (255, 0, 0, 255))),
            (4, 4, 32, dib(4, 4, (0, 255, 0, 255))),
            (16, 16, 32, dib(16, 16, (0, 0, 255, 255))),
        ])
    )
    pal = pic.resize((11, 16)).quantize(16)
    (HERE / "png.ico").write_bytes(
        ico([(11, 16, 8, png(pal)), (16, 16, 32, dib(16, 16, (0, 255, 0, 128)))])
    )
    (HERE / "graypng.ico").write_bytes(ico([(8, 8, 8, png(pic.resize((8, 8)).convert("L")))]))

    small = pic.resize((40, 30)).quantize(32)
    small.save(HERE / "opaque.gif")
    small.save(HERE / "unused.gif", transparency=200)
    holes = small.copy()
    holes.paste(5, (4, 4, 20, 16))
    holes.save(HERE / "transparent.gif", transparency=5)


main()
