#!/opt/qv4-venv/bin/python
r"""What Qt 6.12 gives for every property of the attached object of each Qt attached type, read
by a script on an Item that nothing attached it to. Run in the tools/testenv image from the
repository root, as qt.py:

    docker run --rm -u "$(id -u):$(id -g)" -v "$PWD":/src -w /src ri-testenv:<tag> \
        /opt/qv4-venv/bin/python crates/qml/tests/unset_defaults/attached.py

The attached types are those of `crates/qml/qmltypes` (`attachedType`) a Qt module exports.
Writes, next to this file, `attached.txt`: `<attached C++ class>.<path><TAB>value`, the paths
and values as qt.py writes them. Then run table.py.
"""
import glob, os, re, sys

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
os.environ["QT_QUICK_CONTROLS_STYLE"] = "Basic"
from PySide6.QtCore import QUrl
from PySide6.QtGui import QGuiApplication
from PySide6.QtQml import QQmlComponent, QQmlEngine

HERE = os.path.dirname(os.path.abspath(__file__))
JS = open(os.path.join(HERE, "desc.js")).read()
# The module a document imports for the types of one that is not imported by its own name.
IMPORT = {"QtQuick.Templates": "QtQuick.Controls"}


def attached():
    """(module, QML type, attached C++ class) of every attached type a Qt module exports."""
    out = {}
    for f in glob.glob(os.path.join(HERE, "../../qmltypes/Qt*/**/*.qmltypes"), recursive=True):
        for m in re.finditer(r"Component \{(.*?)\n    \}", open(f).read(), re.S):
            cls = re.search(r'attachedType: "([^"]+)"', m.group(1))
            exports = re.search(r"exports: \[(.*?)\]", m.group(1), re.S)
            if cls and exports:
                for e in re.findall(r'"([^" ]+/[^" ]+) ', exports.group(1)):
                    out[tuple(e.rsplit("/", 1))] = cls.group(1)
    return sorted((m, n, c) for (m, n), c in out.items())


app = QGuiApplication(sys.argv[:1])
eng = QQmlEngine()
keep, lines = [], []
for module, name, cls in attached():
    src = f"""import QtQuick
import {IMPORT.get(module, module)}
Item {{
    id: root
    property string result
    Item {{ id: t }}
{JS}
    Component.onCompleted: {{ var a = t.{name}; result = String(a) + "\\n" + (a ? dump(a) : "") }}
}}
"""
    c = QQmlComponent(eng)
    keep.append(c)
    c.setData(src.encode(), QUrl("file:///nonexistent/defaults/x.qml"))
    o = c.create()
    if o is None:
        print(f"{module}/{name}: not created here", file=sys.stderr)
        continue
    head, *rows = o.property("result").split("\n")
    if re.sub(r"\(0x[0-9a-f]+\)", "", head) != cls:
        print(f"{module}/{name}: reads {head}, not a {cls}", file=sys.stderr)
        continue
    lines += [f"{cls}.{re.sub(r'0x[0-9a-f]+', '0x', l)}" for l in rows if l]
open(os.path.join(HERE, "attached.txt"), "w").write("\n".join(lines) + "\n")
print(len(lines), "lines", file=sys.stderr)
