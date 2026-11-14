# What Qt 6.12 leaves in each scene's `result`: the `// qt:` line of the scene.
#   tools/ci/gate --run /opt/qv4-venv/bin/python /src/crates/shell/tests/scenes/view_current_row/qt.py [NAME...]
# A scene is shown until it has written its `result`.
import ctypes, sys, os, glob
os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "../../../../../tools/testenv"))
import system_qt  # the Qt that answers is the system's, checked there

# system_qt.py puts the system's Qt of QtCore, QtGui and QtQml ahead of the wheel's; a window
# needs these too, and the bindings take the ones already loaded.
for name in ("OpenGL", "Quick"):
    ctypes.CDLL("%s/libQt6%s.so.6" % (system_qt.LIBDIR, name), ctypes.RTLD_GLOBAL)
from PySide6.QtCore import QUrl, QTimer, qInstallMessageHandler
from PySide6.QtGui import QGuiApplication
from PySide6.QtQuick import QQuickView
here = os.path.dirname(os.path.abspath(__file__))
names = sys.argv[1:] or sorted(os.path.basename(f)[:-4] for f in glob.glob(here + "/*.qml"))
qInstallMessageHandler(lambda t, ctx, msg: print("QTMSG " + msg, flush=True))
app = QGuiApplication(sys.argv[:1])
views = []
def run(i):
    if i == len(names):
        app.quit()
        return
    v = QQuickView()
    views.append(v)
    v.setSource(QUrl.fromLocalFile(os.path.join(here, names[i] + ".qml")))
    for e in v.errors(): print("LOADERR " + e.toString())
    v.show()
    def look():
        result = v.rootObject().property("result")
        if not result:
            QTimer.singleShot(50, look)
            return
        print("// qt: %s: %s" % (names[i], result), flush=True)
        v.close()
        run(i + 1)
    look()
run(0)
app.exec()
