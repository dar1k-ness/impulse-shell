#!/opt/qv4-venv/bin/python
r"""What Qt 6.12 gives for every property of each Qt type the `qml` tree creates, once an
instance nothing is set in is an item of a shown window and laid out. Run in the tools/testenv
image from the repository root, after crates/qml/tests/unset_defaults/qt.py (whose
`<module>.txt` name the types):

    docker run --rm -u "$(id -u):$(id -g)" -e HOME=/tmp -v "$PWD":/src -w /src \
        --entrypoint /opt/qv4-venv/bin/python \
        ri-testenv:<tag> crates/shell/tests/unset_window/qt.py

Writes, next to this file, `window.txt`: `Type.path<TAB>value` (the paths and values of
crates/qml/tests/unset_defaults/desc.js) for the lines where a window changes what
`<module>.txt` there has. The scene is `scene()` below; the test loads the same one, from
a file `x.qml` of a directory written here `$DIR`, in the directory of the `shell` crate,
written `$CWD` and the one above it `$UP`.
"""
import glob, os, re, sys, tempfile

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
os.environ["QT_QUICK_CONTROLS_STYLE"] = "Basic"
from PySide6.QtCore import QCoreApplication, QEventLoop, QUrl
from PySide6.QtGui import QGuiApplication
from PySide6.QtQuick import QQuickView

HERE = os.path.dirname(os.path.abspath(__file__))
FRESH = os.path.join(HERE, "../../../qml/tests/unset_defaults")
JS = open(os.path.join(FRESH, "desc.js")).read()


def scene(module, ty):
    imp = "import QtQuick\n" + ("" if module == "QtQuick" else f"import {module}\n")
    return (f"{imp}Item {{\n    id: root\n    width: 400\n    height: 300\n"
            f"    property string result\n    {ty} {{ id: t }}\n{JS}\n"
            "    Timer { interval: 200; running: true; onTriggered: { root.result = dump(t); Qt.quit() } }\n}\n")


# The test runs in the crate's directory, which is what an unset FolderListModel lists.
os.chdir(os.path.join(HERE, "../.."))
CWD = os.getcwd()
app = QGuiApplication(sys.argv[:1])
tmp = tempfile.mkdtemp()
out = []
for f in sorted(glob.glob(os.path.join(FRESH, "Qt*.txt"))):
    module = os.path.basename(f)[:-4]
    fresh = set(open(f).read().split("\n"))
    types = sorted({l.split(".", 1)[0] for l in fresh if l})
    lines = []
    for ty in types:
        path = os.path.join(tmp, "x.qml")
        open(path, "w").write(scene(module, ty))
        v = QQuickView()
        v.setSource(QUrl.fromLocalFile(path))
        for e in v.errors():
            print(ty, e.toString(), file=sys.stderr)
        # The scene writes `result` once, after its first frames, and then asks to quit.
        loop = QEventLoop()
        v.engine().quit.connect(loop.quit)
        v.show()
        loop.exec()
        for l in v.rootObject().property("result").split("\n"):
            if l:
                l = re.sub(r"0x[0-9a-f]+", "0x", l).replace(tmp, "$DIR")
                l = l.replace(CWD, "$CWD").replace(os.path.dirname(CWD), "$UP")
                lines.append(f"{ty}.{l}")
        v.close()
        v.deleteLater()
        QCoreApplication.processEvents()
    out += [l for l in lines if l not in fresh]
    print(module, len(types), "types,", len(lines), "lines", file=sys.stderr)
open(os.path.join(HERE, "window.txt"), "w").write("\n".join(out) + "\n")
