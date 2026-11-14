#!/usr/bin/env python3
"""Regenerates impulse-bitmap-test.{bdf,pcf,otb}, the bitmap font of text.rs `bitmap_glyph_tests`.

One 16 px strike (ascent 12, descent 4) of the family "Impulse Bitmap Test". Its glyphs are
1 bit per pixel, of widths that end inside a byte and on one (3, 5, 8, 13, 16, 19 px: one to
three bytes a row), and every row of a glyph differs from the others. The BDF file is written
here; bdftopcf makes the PCF file of it (rows padded to 4 bytes) and fonttosfnt the OpenType
file with only a bitmap strike (both are X.Org tools, Debian's xfonts-utils). fonttosfnt
writes the time into the file, so its bytes change with every run.
"""

import subprocess

from pathlib import Path

# name, code point, advance, width, height, x offset, y offset (of the lower left corner)
GLYPHS = [
    ("space", 32, 8, 0, 0, 0, 0),
    ("A", 65, 8, 8, 16, 0, -4),
    ("B", 66, 14, 13, 16, 1, -4),
    ("W", 87, 17, 16, 12, 0, 0),
    ("i", 105, 4, 3, 10, 1, 0),
    ("j", 106, 6, 5, 14, -1, -4),
    ("m", 109, 20, 19, 8, 0, 0),
]


def rows(width, height, seed):
    """`height` rows of `width` pseudo-random bits, left-aligned in whole bytes, as hex."""
    out, v = [], seed
    for row in range(height):
        v = (v * 1103515245 + 12345) & 0x7FFFFFFF
        bits = (v >> 8) & ((1 << width) - 1)
        bits |= 1 << (width - 1)  # the leftmost pixel
        if row % 2 == 0:
            bits |= 1  # the rightmost pixel
        size = (width + 7) // 8
        out.append("%0*X" % (size * 2, bits << (size * 8 - width)))
    return out


lines = [
    "STARTFONT 2.1",
    "FONT -impulse-Impulse Bitmap Test-medium-r-normal--16-160-75-75-p-80-iso10646-1",
    "SIZE 16 75 75",
    "FONTBOUNDINGBOX 19 16 -1 -4",
    "STARTPROPERTIES 14",
    'FOUNDRY "impulse"',
    'FAMILY_NAME "Impulse Bitmap Test"',
    'WEIGHT_NAME "Medium"',
    'SLANT "R"',
    'SETWIDTH_NAME "Normal"',
    "PIXEL_SIZE 16",
    "POINT_SIZE 160",
    "RESOLUTION_X 75",
    "RESOLUTION_Y 75",
    'SPACING "P"',
    "FONT_ASCENT 12",
    "FONT_DESCENT 4",
    'CHARSET_REGISTRY "ISO10646"',
    'CHARSET_ENCODING "1"',
    "ENDPROPERTIES",
    "CHARS %d" % len(GLYPHS),
]
for name, code, advance, width, height, x, y in GLYPHS:
    lines += [
        "STARTCHAR " + name,
        "ENCODING %d" % code,
        "SWIDTH %d 0" % (advance * 1000 * 72 // (16 * 75)),
        "DWIDTH %d 0" % advance,
        "BBX %d %d %d %d" % (width, height, x, y),
        "BITMAP",
        *rows(width, height, code),
        "ENDCHAR",
    ]
lines.append("ENDFONT")
bdf = Path(__file__).with_name("impulse-bitmap-test.bdf")
bdf.write_text("\n".join(lines) + "\n")
subprocess.run(["bdftopcf", "-o", bdf.with_suffix(".pcf"), bdf], check=True)
subprocess.run(["fonttosfnt", "-o", bdf.with_suffix(".otb"), bdf], check=True)
