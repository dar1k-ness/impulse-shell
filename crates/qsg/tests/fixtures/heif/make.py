#!/usr/bin/env python3
"""Regenerates the HEIF fixtures of tests/heif.rs with libheif (ctypes).

quad.avif   32x16 opaque: red | lime over blue | white quadrants.
quad.heic   the same as HEVC.
alpha.avif  32x16: opaque red left half; right half lime at alpha 128 over a transparent bottom.
alpha.heic  the same as HEVC.
deep.avif   10-bit 32x16 opaque: red | lime over blue | grey 512/1023 quadrants (near full scale).
rot.avif    quad.avif with `irot` 1 (90 degrees anti-clockwise) on the primary item (no `imir`:
            libheif before 1.16 read its axis the other way round).
"""

import ctypes
import struct
from pathlib import Path

HERE = Path(__file__).resolve().parent
HEIF = ctypes.CDLL("libheif.so.1")


class Err(ctypes.Structure):
    _fields_ = [("code", ctypes.c_int), ("subcode", ctypes.c_int), ("message", ctypes.c_char_p)]


for f in ("heif_context_get_encoder_for_format", "heif_image_create", "heif_image_add_plane",
          "heif_encoder_set_lossy_quality", "heif_context_encode_image",
          "heif_context_write_to_file"):
    getattr(HEIF, f).restype = Err
HEIF.heif_context_alloc.restype = ctypes.c_void_p
HEIF.heif_image_get_plane.restype = ctypes.POINTER(ctypes.c_uint8)

HEVC, AV1 = 1, 4  # heif_compression_format
RGB, RGB_I, RGBA, RRGGBB_LE, INTERLEAVED = 1, 10, 11, 14, 10  # colorspace, chromas, channel


def check(e):
    if e.code:
        raise SystemExit(e.message.decode())


def encode(path, w, h, pixel, fmt, bits=8, alpha=False):
    """Encodes `pixel(x, y) -> (r, g, b, a)` with libheif at quality 90 (with `alpha` as an aux
    image, else RGB without one); at 10 bits the samples are 0..1023, opaque only."""
    ctx = ctypes.c_void_p(HEIF.heif_context_alloc())
    enc = ctypes.c_void_p()
    check(HEIF.heif_context_get_encoder_for_format(ctx, fmt, ctypes.byref(enc)))
    check(HEIF.heif_encoder_set_lossy_quality(enc, 90))
    img = ctypes.c_void_p()
    chroma = RRGGBB_LE if bits != 8 else RGBA if alpha else RGB_I
    check(HEIF.heif_image_create(w, h, RGB, chroma, ctypes.byref(img)))
    ch = 4 if alpha else 3
    check(HEIF.heif_image_add_plane(img, INTERLEAVED, w, h, bits))
    stride = ctypes.c_int()
    plane = HEIF.heif_image_get_plane(img, INTERLEAVED, ctypes.byref(stride))
    for y in range(h):
        for x in range(w):
            px = pixel(x, y)
            for c, v in enumerate(px[:ch]):
                if bits == 8:
                    plane[y * stride.value + ch * x + c] = v
                else:
                    plane[y * stride.value + 6 * x + 2 * c] = v & 255
                    plane[y * stride.value + 6 * x + 2 * c + 1] = v >> 8
    check(HEIF.heif_context_encode_image(ctx, img, enc, None, None))
    check(HEIF.heif_context_write_to_file(ctx, str(path).encode()))
    HEIF.heif_image_release(img)
    HEIF.heif_encoder_release(enc)
    HEIF.heif_context_free(ctx)


def boxes(b):
    i = 0
    while i < len(b):
        size, kind = struct.unpack(">I4s", b[i : i + 8])
        yield [kind.decode(), b[i + 8 : i + size]]
        i += size


def box(kind, payload):
    return struct.pack(">I4s", 8 + len(payload), kind.encode()) + payload


def add_properties(data, props):
    """Appends `props` (encoded boxes) to ipco and associates them (essential) with item 1."""
    top = list(boxes(data))
    out = []
    delta = 0
    for kind, body in top:
        if kind != "meta":
            out.append([kind, body])
            continue
        head, inner = body[:4], list(boxes(body[4:]))
        for child in inner:
            if child[0] != "iprp":
                continue
            ip = list(boxes(child[1]))
            n_before = 0
            for p in ip:
                if p[0] == "ipco":
                    n_before = len(list(boxes(p[1])))
                    p[1] += b"".join(props)
            for p in ip:
                if p[0] != "ipma":
                    continue
                vf, body2 = p[1][:4], p[1][4:]
                version, flags = vf[0], int.from_bytes(vf[1:], "big")
                assert version == 0 and not flags & 1
                (count,) = struct.unpack(">I", body2[:4])
                j, items = 4, b""
                for _ in range(count):
                    item, n = struct.unpack(">HB", body2[j : j + 3])
                    assoc = body2[j + 3 : j + 3 + n]
                    j += 3 + n
                    if item == 1:
                        assoc += bytes(0x80 | (n_before + k + 1) for k in range(len(props)))
                    items += struct.pack(">HB", item, len(assoc)) + assoc
                p[1] = vf + struct.pack(">I", count) + items
            child[1] = b"".join(box(k, v) for k, v in ip)
        new = head + b"".join(box(k, v) for k, v in inner)
        delta = len(new) - len(body)
        out.append([kind, new, inner])
    # Shift iloc offsets (construction method 0, file offsets) by the growth of meta.
    for entry in out:
        if entry[0] != "meta":
            continue
        for child in entry[2]:
            if child[0] != "iloc":
                continue
            b = bytearray(child[1])
            version = b[0]
            osz, lsz, bsz = b[4] >> 4, b[4] & 15, b[5] >> 4
            isz = b[5] & 15 if version else 0
            assert osz == 4 and bsz in (0, 4) and version < 2
            j = 6
            (count,) = struct.unpack(">H", b[j : j + 2])
            j += 2
            for _ in range(count):
                j += 2
                if version == 1:
                    j += 2
                j += 2
                base = int.from_bytes(b[j : j + bsz], "big") if bsz else 0
                if bsz and base:
                    b[j : j + 4] = struct.pack(">I", base + delta)
                j += bsz
                (ext,) = struct.unpack(">H", b[j : j + 2])
                j += 2
                for _ in range(ext):
                    j += isz
                    if not base:
                        off = int.from_bytes(b[j : j + 4], "big")
                        b[j : j + 4] = struct.pack(">I", off + delta)
                    j += osz + lsz
            child[1] = bytes(b)
        entry[1] = entry[1][:4] + b"".join(box(k, v) for k, v in entry[2])
    return b"".join(box(e[0], e[1]) for e in out)


def quad(x, y):
    return [(255, 0, 0, 255), (0, 255, 0, 255), (0, 0, 255, 255), (255, 255, 255, 255)][
        (x >= 16) + 2 * (y >= 8)
    ]


def alpha(x, y):
    if x < 16:
        return (255, 0, 0, 255)
    return (0, 255, 0, 128) if y < 8 else (0, 0, 0, 0)


def main():
    for name, pixel in (("quad", quad), ("alpha", alpha)):
        for ext, fmt in (("avif", AV1), ("heic", HEVC)):
            encode(HERE / f"{name}.{ext}", 32, 16, pixel, fmt, alpha=name == "alpha")
    deep = lambda x, y: [(1020, 0, 0), (0, 1020, 0), (0, 0, 1020), (512, 512, 512)][
        (x >= 16) + 2 * (y >= 8)
    ]
    encode(HERE / "deep.avif", 32, 16, deep, AV1, bits=10)
    irot = box("irot", bytes([1]))
    (HERE / "rot.avif").write_bytes(add_properties((HERE / "quad.avif").read_bytes(), [irot]))


main()
