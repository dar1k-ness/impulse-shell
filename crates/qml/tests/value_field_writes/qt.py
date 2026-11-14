#!/opt/qv4-venv/bin/python
r"""What Qt 6.12 does with a write to a field of a value type property nothing set
(`t.sourceClipRect.x = 3`), for every such property of each Qt type the `qml` tree creates:
the properties `../unset_defaults/decl.tsv` (Qt's metadata) gives one of the types of FIELD.
Run in the tools/testenv image from the repository root:

    docker run --rm -u "$(id -u):$(id -g)" -e HOME=/tmp -v "$PWD":/src -w /src \
        --entrypoint /opt/qv4-venv/bin/python ri-testenv:<tag> \
        crates/qml/tests/value_field_writes/qt.py

Writes `qt.tsv` next to this file: the module, `Type.property`, the field written and what
probe.js gives.
"""
import glob, os, sys

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
os.environ["QT_QUICK_CONTROLS_STYLE"] = "Basic"
from PySide6.QtCore import QUrl
from PySide6.QtGui import QGuiApplication
from PySide6.QtQml import QQmlComponent, QQmlEngine

HERE = os.path.dirname(os.path.abspath(__file__))
D = os.path.join(HERE, "../unset_defaults")
JS = open(os.path.join(HERE, "probe.js")).read()
# The field written, by the C++ type of the property.
FIELD = {"QPointF": "x", "QPoint": "x", "QRectF": "x", "QRect": "x", "QSizeF": "width",
         "QSize": "width", "QVector2D": "x", "QVector3D": "x", "QVector4D": "x",
         "QQuaternion": "scalar", "QMatrix4x4": "m11"}
module = {}
for f in glob.glob(os.path.join(D, "Qt*.txt")):
    for l in open(f):
        module[l.split(".", 1)[0]] = os.path.basename(f)[:-4]
app = QGuiApplication(sys.argv[:1])
eng = QQmlEngine()
keep, out = [], []
for l in open(os.path.join(D, "decl.tsv")):
    k, _, t = l.rstrip("\n").split("\t")
    if t not in FIELD:
        continue
    ty, p = k.split(".")
    m = module[ty]
    imp = "import QtQuick\n" + ("" if m == "QtQuick" else f"import {m}\n")
    src = (f"{imp}Item {{\n    id: root\n    property string result\n    {ty} {{ id: t }}\n{JS}\n"
           f"    Component.onCompleted: result = probe(t, \"{p}\", \"{FIELD[t]}\")\n}}\n")
    c = QQmlComponent(eng)
    keep.append(c)
    c.setData(src.encode(), QUrl("file:///nonexistent/defaults/x.qml"))
    o = c.create()
    assert o is not None, c.errors()
    out.append(f"{m}\t{k}\t{FIELD[t]}\t{o.property('result')}")
open(os.path.join(HERE, "qt.tsv"), "w").write("\n".join(sorted(out)) + "\n")
