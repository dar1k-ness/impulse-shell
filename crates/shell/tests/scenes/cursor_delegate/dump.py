# What Qt 6.12 answers for a case.
#
# `dump.py CASE.qml` prints the scene's `out` after its steps, each step run in an active
# window with a frame after it. In the testenv image:
#   tools/ci/gate --run as-tester sh -c 'cd crates/shell/tests/scenes/cursor_delegate &&
#     for f in *.qml; do QT_QUICK_CONTROLS_STYLE=Basic /opt/qv4-venv/bin/python dump.py $f > ${f%.qml}.qt; done'
#
# `dump.py CASE.qml CASE.png` saves the picture of the window once it is active and eight
# more frames were swapped, and prints the size of the item named "field". Drawn through
# OpenGL (llvmpipe) under the image's headless sway; the `offscreen` platform draws with
# QPainter, which blends differently. The window is held at the size of the scene: one that
# sway tiles to the size of the output has two pixels on the rounded corners of todo.qml one
# of 255 off. In the testenv image:
#   tools/ci/gate --run as-tester sh -c 'cd crates/shell/tests/scenes/cursor_delegate &&
#     QT_QUICK_CONTROLS_STYLE=Basic QT_QPA_PLATFORM=wayland-egl LIBGL_ALWAYS_SOFTWARE=1 \
#     QT_SCALE_FACTOR=1 /opt/qv4-venv/bin/python dump.py todo.qml todo.png'
import sys, os
os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
from PySide6.QtCore import QObject, QUrl, QTimer, QMetaObject, Q_ARG, Qt, qInstallMessageHandler
from PySide6.QtGui import QGuiApplication, QImage
from PySide6.QtQuick import QQuickView
def handler(t, ctx, msg):
    sys.stderr.write(msg + "\n")
qInstallMessageHandler(handler)
app = QGuiApplication(sys.argv)
v = QQuickView()
v.setSource(QUrl.fromLocalFile(os.path.abspath(sys.argv[1])))
for e in v.errors(): print("LOADERR " + e.toString())
root = v.rootObject()
k = [0]
def step():
    if not v.isActive():
        return
    if k[0] < root.property("steps"):
        QMetaObject.invokeMethod(root, "step", Qt.DirectConnection, Q_ARG("QVariant", k[0]))
        k[0] += 1
        return
    sys.stdout.write(root.property("out"))
    app.quit()
def swapped():
    if not v.isActive():
        return
    k[0] += 1
    if k[0] != 8:
        return
    w, h = round(root.width()), round(root.height())
    img = v.grabWindow().convertToFormat(QImage.Format.Format_RGBA8888)
    assert (img.width(), img.height()) == (w, h), (img.size(), w, h)
    img.save(sys.argv[2])
    field = root.findChild(QObject, "field")
    print("field", field.width(), field.height())
    sys.stdout.flush()
    # Not app.quit(): tearing the window down from inside its frameSwapped handler crashes.
    os._exit(0)
t = QTimer()
if len(sys.argv) > 2:
    # A window of one fixed size floats in sway; tiled, it would get the output's size.
    v.setMinimumSize(v.size())
    v.setMaximumSize(v.size())
    v.frameSwapped.connect(swapped)
    t.timeout.connect(v.requestUpdate)
    t.start(16)
else:
    t.timeout.connect(step)
    t.start(40)
v.show()
v.requestActivate()
app.exec()
