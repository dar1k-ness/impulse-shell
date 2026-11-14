import sys, os
os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
from PySide6.QtCore import QUrl, QTimer, qInstallMessageHandler
from PySide6.QtGui import QGuiApplication
from PySide6.QtQuick import QQuickView, QQuickItem
def handler(t, ctx, msg):
    print(("console: " if ctx.category in ("qml", "js") else "QTMSG ") + msg, flush=True)
qInstallMessageHandler(handler)
app = QGuiApplication(sys.argv)
v = QQuickView()
v.setSource(QUrl.fromLocalFile(os.path.abspath(sys.argv[1])))
for e in v.errors(): print("LOADERR " + e.toString())
v.show()
def dump():
    rows = []
    def walk(it):
        if it.objectName():
            rows.append("geom: %s %.2f %.2f %.2f %.2f %.2f %.2f" % (it.objectName(), it.x(), it.y(), it.width(), it.height(), it.implicitWidth(), it.implicitHeight()))
        for c in it.childItems(): walk(c)
    walk(v.contentItem())
    for r in sorted(rows): print(r)
    app.quit()
QTimer.singleShot(800, dump)
app.exec()
