"""What the ICU that the system's Qt 6.12 loads does, asked of the library itself: the one in
the process once tools/testenv/system_qt.py has put the system's Qt under PySide6.

  qt_icu.py names            converter, a tab, a name ICU has for it: one line per name
  qt_icu.py decode NAME FILE each record of FILE (a length byte, then that many bytes) read by
                             the converter NAME the way QStringDecoder reads a whole reply
                             (ucnv_toUnicode, nothing flushed); to stdout a record per record:
                             the count of UTF-16 units as a byte, then the units, low byte first
"""
import ctypes, os, sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "../../../tools/testenv"))
import system_qt  # the ICU that answers is the one the system's Qt loads, checked there
from PySide6 import QtCore  # loads the Qt whose ICU is asked

system_qt.checked()
lib = next(f for f in system_qt.loaded() if os.path.basename(f).startswith("libicuuc.so"))
icu = ctypes.CDLL(lib)
ver = os.path.basename(os.path.realpath(lib)).partition(".so.")[2].split(".")[0]
P, S = ctypes.c_void_p, ctypes.c_char_p


def f(name, res, *args):
    g = getattr(icu, name + "_" + ver)
    g.restype = res
    g.argtypes = args
    return g


err = ctypes.c_int(0)
e = ctypes.byref(err)
opened = f("ucnv_open", P, S, P)
close = f("ucnv_close", None, P)


def names():
    count = f("ucnv_countAvailable", ctypes.c_int32)
    available = f("ucnv_getAvailableName", S, ctypes.c_int32)
    aliases = f("ucnv_countAliases", ctypes.c_uint16, S, P)
    alias = f("ucnv_getAlias", S, S, ctypes.c_uint16, P)
    name = f("ucnv_getName", S, P, P)
    seen = set()
    for i in range(count()):
        of = available(i)
        for j in range(aliases(of, e)):
            a = alias(of, j, e)
            if a in seen:
                continue
            seen.add(a)
            c = opened(a, e)
            sys.stdout.write(name(c, e).decode() + "\t" + a.decode() + "\n")
            close(c)


def decode(name, path):
    reset = f("ucnv_reset", None, P)
    read = f("ucnv_toUnicode", None, P, P, P, P, P, P, ctypes.c_int8, P)
    data = open(path, "rb").read()
    src = ctypes.create_string_buffer(data, len(data))
    base = ctypes.addressof(src)
    room = ctypes.create_string_buffer(2048)
    to = ctypes.addressof(room)
    c = opened(name.encode(), e)
    if err.value > 0:
        sys.exit("ICU: error %d opening %s" % (err.value, name))
    target, source = P(), P()
    pt, ps = ctypes.byref(target), ctypes.byref(source)
    out = bytearray()
    at, end = 0, len(data)
    while at < end:
        n = data[at]
        at += 1
        reset(c)
        err.value = 0
        target.value = to
        source.value = base + at
        read(c, pt, to + 2048, ps, base + at + n, None, 0, e)
        at += n
        units = (target.value - to) // 2
        out.append(units)
        out += ctypes.string_at(to, 2 * units)
    close(c)
    sys.stdout.buffer.write(out)


if sys.argv[1] == "names":
    names()
else:
    decode(sys.argv[2], sys.argv[3])
if sys.argv[1] == "names" and err.value > 0:
    sys.exit("ICU: error %d" % err.value)
