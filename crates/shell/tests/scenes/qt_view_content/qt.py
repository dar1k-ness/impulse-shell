# What Qt 6.12 leaves in each scene's `result`: the `// qt:` line of the scene.
#   tools/ci/gate --run /opt/qv4-venv/bin/python /src/crates/shell/tests/scenes/qt_view_content/qt.py [NAME...]
# The scene's timers run for a second; 320 ms in, each point of the root's `clicks` gets a
# left click, as `qt_view_content.rs` does on its frame 20.
import sys, os, glob
os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
from PySide6.QtCore import QUrl, QTimer, QPoint, Qt, qInstallMessageHandler
from PySide6.QtGui import QGuiApplication
from PySide6.QtQuick import QQuickView
from PySide6.QtTest import QTest
here = os.path.dirname(os.path.abspath(__file__))
names = sys.argv[1:] or sorted(os.path.basename(f)[:-4] for f in glob.glob(here + "/*.qml"))
def handler(t, ctx, msg):
    print("QTMSG " + msg, flush=True)
qInstallMessageHandler(handler)
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
    def click():
        clicks = v.rootObject().property("clicks")
        for p in clicks.toVariant() if clicks else []:
            QTest.mouseClick(v, Qt.LeftButton, Qt.NoModifier, QPoint(int(p[0]), int(p[1])))
    def done():
        print("// qt: %s: %s" % (names[i], v.rootObject().property("result")), flush=True)
        v.close()
        run(i + 1)
    QTimer.singleShot(320, click)
    QTimer.singleShot(1100, done)
run(0)
app.exec()
