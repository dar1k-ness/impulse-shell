# qt.py SCENE.qml STEP...
# What Qt 6.12 gives for a scene of this directory: its `snap()` as loaded and after each STEP
# (a function of the scene), with `==` between them. Run in the testenv image (PySide6 6.12):
#   QT_QPA_PLATFORM=offscreen QT_QUICK_CONTROLS_STYLE=Basic /opt/qv4-venv/bin/python qt.py \
#       Places.qml end > Places.qt
import os
import sys

from PySide6.QtCore import QMetaObject, QTimer, QUrl, Q_RETURN_ARG, qInstallMessageHandler
from PySide6.QtGui import QGuiApplication
from PySide6.QtQuick import QQuickView

qInstallMessageHandler(lambda t, c, m: print("QT: " + m, file=sys.stderr, flush=True))
app = QGuiApplication(sys.argv)
v = QQuickView()
v.setSource(QUrl.fromLocalFile(os.path.abspath(sys.argv[1])))
if v.status() != QQuickView.Status.Ready:
    sys.exit(f"{sys.argv[1]}: {v.errors()}")
v.show()
steps = [""] + sys.argv[2:]
out = []


def step():
    name = steps.pop(0)
    if name:
        QMetaObject.invokeMethod(v.rootObject(), name)
    QTimer.singleShot(1500, snap)


def snap():
    out.append(QMetaObject.invokeMethod(v.rootObject(), "snap", Q_RETURN_ARG("QVariant")))
    if steps:
        step()
    else:
        print("\n==\n".join(out))
        app.quit()


QTimer.singleShot(0, step)
app.exec()
