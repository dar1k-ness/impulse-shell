# Qt's answer for a probe of this directory: sample() after each of 60 steps of Qt's animation
# timer, then the probe's `log` and Qt's warnings. The steps are those of an animation of this
# script, which Qt steps on the timer the probe's animations run on; sample() is queued from a
# step, so it runs once every animation took that step. The probes log no value that depends on
# where in a frame an animation is.
#   QT_QPA_PLATFORM=offscreen python qt.py when_back.qml > when_back.qt612.txt
import os, sys
from PySide6.QtCore import QCoreApplication, QMetaObject, QUrl, QVariantAnimation, Qt, qInstallMessageHandler
from PySide6.QtGui import QGuiApplication
from PySide6.QtQml import QQmlComponent, QQmlEngine

warnings = []
qInstallMessageHandler(lambda kind, ctx, msg: warnings.append(msg))
app = QGuiApplication(sys.argv)
engine = QQmlEngine()
component = QQmlComponent(engine, QUrl.fromLocalFile(os.path.abspath(sys.argv[1])))
root = component.create()
if root is None:
    sys.exit("\n".join(e.toString() for e in component.errors()))
QCoreApplication.processEvents()
steps = QVariantAnimation(startValue=0.0, endValue=1.0, duration=600000)
left = [60]


def step(_):
    if left[0] > 0:
        left[0] -= 1
        QMetaObject.invokeMethod(root, "sample", Qt.QueuedConnection)
    else:
        QMetaObject.invokeMethod(app, "quit", Qt.QueuedConnection)


steps.valueChanged.connect(step)
steps.start()
app.exec()
sys.stdout.write(root.property("log"))
for w in warnings:
    print("warning: " + w.split(": ", 1)[-1])
