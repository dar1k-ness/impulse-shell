#!/opt/qv4-venv/bin/python
r"""What Qt 6.12 gives for every property of a fresh instance of each Qt type the `qml` tree
creates. Run in the tools/testenv image from the repository root:

    docker run --rm -u "$(id -u):$(id -g)" -e HOME=/tmp -v "$PWD":/src -w /src \
        --entrypoint /opt/qv4-venv/bin/python ri-testenv:<tag> \
        crates/qml/tests/unset_defaults/qt.py

Writes, next to this file, `<module>.txt` (`Type.path<TAB>value`, the paths going into groups,
value types and non-item objects, see desc.js) and `decl.tsv` (`Type.property`, the C++ class
declaring it, its C++ type). Then run table.py.
"""
import collections, os, re, sys, tempfile

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
os.environ["QT_QUICK_CONTROLS_STYLE"] = "Basic"
from PySide6.QtCore import QCoreApplication, QUrl
from PySide6.QtGui import QGuiApplication
from PySide6.QtQml import QQmlComponent, QQmlEngine

HERE = os.path.dirname(os.path.abspath(__file__))
QML = os.path.join(HERE, "../../../../qml")
MODULES = ["QtQuick", "QtQuick.Layouts", "QtQuick.Controls", "QtQuick.Effects", "QtQuick.Shapes",
           "Qt5Compat.GraphicalEffects", "Qt.labs.synchronizer", "Qt.labs.folderlistmodel",
           "QtPositioning", "QtCore"]
JS = open(os.path.join(HERE, "desc.js")).read()


def used():
    """The names the `qml` tree writes before `{`, less the components it defines itself, each
    with the modules of MODULES every file that writes it imports without `as`, in the file's
    order."""
    local, names = set(), collections.defaultdict(set)
    pat = re.compile(r"(?<![\w.])([A-Z]\w*)(?:\s+on\s+[\w.]+)?\s*\{")
    imp = re.compile(r"^[ \t]*import[ \t]+([A-Za-z][\w.]*)(?:[ \t]+[\d.]+)?[ \t]*;?[ \t]*$", re.M)
    for d, _, fs in os.walk(QML):
        for f in fs:
            if not f.endswith(".qml"):
                continue
            local.add(f[:-4])
            s = open(os.path.join(d, f), encoding="utf-8").read()
            s = re.sub(r"/\*.*?\*/", "", s, flags=re.S)
            s = re.sub(r"//[^\n]*", "", s)
            s = re.sub(r'"(?:\\.|[^"\\\n])*"', '""', s)
            s = re.sub(r"'(?:\\.|[^'\\\n])*'", "''", s)
            mods = tuple(m for m in imp.findall(s) if m in MODULES)
            for m in pat.finditer(s):
                names[m.group(1)].add(mods)
    return {n: sorted(v) for n, v in sorted(names.items()) if n not in local}


app = QGuiApplication(sys.argv[:1])
eng = QQmlEngine()
# PySide6 has no Qt5Compat.GraphicalEffects: the image's own Qt 6.12 has it. Only that module
# is taken from there, every other one stays PySide6's.
compat = tempfile.mkdtemp()
os.symlink("/usr/lib/qt6/qml/Qt5Compat", os.path.join(compat, "Qt5Compat"))
eng.addImportPath(compat)
keep = []


def create(module, ty, body, alias=""):
    """An Item with a `ty` of `module` in it; behind `alias`, so that only the module's own
    type of that name will do."""
    imp = "import QtQuick\n" + ("" if module == "QtQuick" else f"import {module}\n")
    if alias:
        imp, ty = f"import QtQuick\nimport {module} as {alias}\n", f"{alias}.{ty}"
    src = f"{imp}Item {{\n    id: root\n    property string result\n    property QtObject o: t\n    {ty} {{ id: t }}\n{body}\n}}\n"
    c = QQmlComponent(eng)
    keep.append(c)
    # The directory is one Qt cannot list: FolderListModel.folder and Text.baseUrl name it.
    c.setData(src.encode(), QUrl("file:///nonexistent/defaults/x.qml"))
    return None if c.isError() else c.create()


# A name is the type of the last import that has it (QQmlImports), so `RadialGradient` is the
# one of the module the file that writes it imports, not QtQuick.Shapes' of the same name.
found = collections.defaultdict(list)
missing = []
for ty, files in used().items():
    mods = set()
    for imports in files:
        for m in reversed(imports):
            o = create(m, ty, "    Component.onCompleted: result = 'ok'", "M")
            if o is not None and o.property("result") == "ok":
                mods.add(m)
                break
    for m in MODULES:
        if m in mods:
            found[m].append(ty)
    if not mods:
        missing.append(ty)
print("not Qt types of these modules:", " ".join(missing), file=sys.stderr)

decl = []
for m, tys in found.items():
    lines = []
    for ty in tys:
        o = create(m, ty, JS + "\n    Component.onCompleted: result = dump(t)")
        for l in o.property("result").split("\n"):
            if l:
                lines.append(f"{ty}.{re.sub(r'0x[0-9a-f]+', '0x', l)}")
        mo, seen = o.property("o").metaObject(), set()
        while mo is not None:
            for i in range(mo.propertyOffset(), mo.propertyCount()):
                p = mo.property(i)
                if p.name() not in seen:
                    seen.add(p.name())
                    # A type that is a QML file (`RadialGradient_QMLTYPE_536`) is no C++ class.
                    if "_QMLTYPE_" not in mo.className():
                        decl.append(f"{ty}.{p.name()}\t{mo.className()}\t{p.typeName()}")
            mo = mo.superClass()
        o.deleteLater()
        QCoreApplication.processEvents()
    open(os.path.join(HERE, m + ".txt"), "w").write("\n".join(lines) + "\n")
    print(m, len(tys), "types,", len(lines), "lines", file=sys.stderr)
open(os.path.join(HERE, "decl.tsv"), "w").write("\n".join(decl) + "\n")
