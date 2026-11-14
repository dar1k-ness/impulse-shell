#!/usr/bin/env python3
"""Writes `crates/qml/src/qt_defaults.tsv`, the defaults the engine takes from Qt, out of what
qt.py and attached.py wrote: every property, and every field of a group in GROUPS, whose value in a fresh instance is a
number, a boolean, a string or a colour and not the zero of its type, less `exclude.tsv`; the
objects of OBJECTS whole; the value types that are not what their type starts as; and every
number and boolean of an attached object that is not zero.

The key of a line is the C++ class declaring the property when every type read here that has
it from that class gives the same value, else the QML type.
"""
import collections, glob, os, re

HERE = os.path.dirname(os.path.abspath(__file__))
# What an item's layout, text or state gives, not a default.
DERIVED = re.compile(
    r"^(x|y|width|height|implicit\w*|content(Width|Height|X|Y)|available\w*|painted\w*"
    r"|baselineOffset|lineCount|count|visualPosition|visualSize|position|size|length|origin[XY]"
    r"|at[XY](Beginning|End)|baseUrl|effective\w*|\w*[pP]adding|\w*[iI]nset"
    r"|horizontal|vertical)$")
# The groups the engine keeps the fields of with their object, as `border.width`. A field of
# another object or value type (`font`, `drag`, `section`, `sourceSize`) is not read here.
GROUPS = {"anchors", "border", "layer", "icon", "easing"}
# The objects and gadgets a C++ type always has, which the engine gives a script as a group of
# its object too: `group` for the object (`group:<text>` for a gadget, with what String() of
# it gives), its fields at any depth with their values, the zero ones as well.
OBJECTS = {"drag", "visibleArea", "section", "xAxis", "yAxis", "point", "centroid",
           "cursorSelection", "textDocument", "trim", "colorSpace", "palette", "fontInfo",
           "pushEnter", "pushExit", "popEnter", "popExit", "replaceEnter", "replaceExit"}
# Those whose fields the engine works out.
WORKED = {"fontInfo"}
# The value types the engine has, by the name String() gives.
VALUE = re.compile(r'"(Q(?:PointF|SizeF|RectF|Size|Rect|Vector[234]D|Quaternion)\([-\d., e+]*\))"$')
# What a value type of that name is when nothing set it.
VALUE_ZERO = {'"QPointF(0, 0)"', '"QSizeF(-1, -1)"', '"QRectF(0, 0, 0, 0)"', '"QSize(-1, -1)"',
              '"QRect(0, 0, 0, 0)"', '"QVector2D(0, 0)"', '"QVector3D(0, 0, 0)"',
              '"QVector4D(0, 0, 0, 0)"', '"QQuaternion(1, 0, 0, 0)"'}
# Value types that an item's size, text or image give.
VALUE_DERIVED = {"childrenRect", "transformOriginPoint", "cursorRectangle", "sourceSize",
                 "canvasSize", "tileSize", "canvasWindow", "sourceClipRect"}
# The attached objects whose values are the screen and the graphics of the machine.
ENVIRONMENT = {"QQuickScreenAttached", "QQuickGraphicsInfo"}
ZERO = {"number:0", "boolean:false", '""', '"#000000"'}
COLOR = re.compile(r'"#[0-9a-f]{6,8}"$')

decl = {}
for l in open(os.path.join(HERE, "decl.tsv")):
    k, c, t = l.rstrip("\n").split("\t")
    decl[k] = (c, t)
exclude = {l.split("\t")[0] for l in open(os.path.join(HERE, "exclude.tsv"))
           if l.strip() and not l.startswith("#")}


def scalar(v):
    return v.startswith(("number:", "boolean:")) or (
        v.startswith('"') and not re.match(r'"(Q[A-Z]\w*(\(|::)|\[object|group_)', v))


qt = collections.defaultdict(dict)  # (class, path) -> {type: value}
raw = {}  # `Type.path` -> value
for f in glob.glob(os.path.join(HERE, "Qt*.txt")):
    for l in open(f):
        path, v = l.rstrip("\n").split("\t")
        raw[path] = v
        ty, rest = path.split(".", 1)
        top = rest.split(".")[0]
        if f"{ty}.{top}" in decl:
            qt[(decl[f"{ty}.{top}"][0], rest)][ty] = v
out = []


def emit(c, rest, m, enc, zero=()):
    """The line of `rest` under its class when every type gives one value, else a line a type."""
    if len(set(m.values())) == 1:
        v = next(iter(m.values()))
        if v not in zero and f"{c}.{rest}" not in exclude:
            out.append(f"{c}.{rest}\t{enc(v)}")
    else:
        for ty, v in sorted(m.items()):
            if v not in zero and f"{ty}.{rest}" not in exclude and f"{c}.{rest}" not in exclude:
                out.append(f"{ty}.{rest}\t{enc(v)}")


def obj(v):
    """What the table says for a value inside an object of OBJECTS, None for one it leaves."""
    if v == "object":
        return "group"
    if v == '"[object Object]"':
        return "group:[object Object]"
    if v == "null" or v.startswith(("number:", "boolean:")):
        return v
    if COLOR.match(v):
        return "color:" + v.strip('"')
    if VALUE.match(v):
        return "value:" + v.strip('"')
    if re.match(r'"Q[A-Z]\w*\(', v):
        return "group:" + v.strip('"')
    return v if v.startswith('"') and not v.startswith('"[object') else None


values = set()  # the value types written: their fields are not lines
for (c, rest), m in sorted(qt.items()):
    segs = rest.split(".")
    if any((c, ".".join(segs[:i])) in values for i in range(1, len(segs))):
        continue
    if segs[0] in OBJECTS:
        if segs[0] in WORKED and len(segs) > 1:
            continue
        if all(obj(v) is not None for v in m.values()):
            if all(obj(v).startswith("value:") for v in m.values()):
                values.add((c, rest))
            emit(c, rest, m, obj)
        continue
    if (len(segs) == 1 and rest not in VALUE_DERIVED
            and all(VALUE.match(v) for v in m.values())):
        values.add((c, rest))
        emit(c, rest, m, obj, VALUE_ZERO)
        continue
    if len(segs) > 1 and (len(segs) > 2 or segs[0] not in GROUPS):
        continue
    if not all(scalar(v) for v in m.values()):
        continue
    if len(segs) == 1 and DERIVED.match(rest):
        continue
    top_type = decl[f"{next(iter(m))}.{segs[0]}"][1]
    # A field of a group that has no C++ type of its own here reads `undefined` unless listed.
    zero = set() if top_type == "QEasingCurve" else ZERO

    def enc(v):
        return "color:" + v.strip('"') if COLOR.match(v) else v

    emit(c, rest, m, enc, zero)
# The attached objects (attached.py): what a property of one nothing wrote reads, under the
# attached C++ class.
for l in open(os.path.join(HERE, "attached.txt")):
    path, v = l.rstrip("\n").split("\t")
    c, rest = path.split(".", 1)
    if (c not in ENVIRONMENT and "." not in rest and v.startswith(("number:", "boolean:"))
            and v not in ZERO and path not in exclude):
        out.append(f"{path}\t{v}")
# The lines the rules above leave out that the engine reads another value for.
for l in open(os.path.join(HERE, "also.tsv")):
    if not l.startswith("#"):
        path = l.split("\t")[0]
        v = raw[path]
        out.append(f"{path}\t{'color:' + v.strip(chr(34)) if COLOR.match(v) else v}")
out.sort()
open(os.path.join(HERE, "../../src/qt_defaults.tsv"), "w").write("\n".join(out) + "\n")
