# steps_oracle.py SCENE.qml OUT
# Shows the scene (an Item root with `signal shoot()` and `signal finished()`) in a QQuickView,
# as effects_oracle.py does, and lets the scene change itself: each `shoot()` writes the device
# pixels the root covers then to OUT<n> (n from 0; u32 width, u32 height, little endian, then
# premultiplied RGBA rows), `finished()` ends the run. Needs a platform with OpenGL (wayland
# under a headless sway).
import os
import struct
import sys

from PySide6.QtCore import QUrl
from PySide6.QtGui import QGuiApplication, QImage
from PySide6.QtQuick import QQuickView

src, out = sys.argv[1], sys.argv[2]
app = QGuiApplication(sys.argv)
v = QQuickView()
v.setSource(QUrl.fromLocalFile(src))
if v.status() != QQuickView.Status.Ready:
    sys.exit(f"{src}: {v.errors()}")
root = v.rootObject()
# A window of one fixed size floats in sway (see effects_oracle.py).
v.setMinimumSize(v.size())
v.setMaximumSize(v.size())
shots = 0


def shoot():
    global shots
    dpr = v.effectiveDevicePixelRatio()
    w, h = round(root.width() * dpr), round(root.height() * dpr)
    img = v.grabWindow().convertToFormat(QImage.Format.Format_RGBA8888_Premultiplied)
    assert (img.width(), img.height()) == (w, h), (img.size(), w, h)
    with open(f"{out}{shots}", "wb") as f:
        f.write(struct.pack("<II", w, h))
        for y in range(h):
            f.write(bytes(img.constScanLine(y))[: w * 4])
    shots += 1


def finished():
    sys.stdout.flush()
    # Not app.quit(): tearing the window down from inside a handler of its own crashes.
    os._exit(0)


root.shoot.connect(shoot)
root.finished.connect(finished)
v.show()
app.exec()
