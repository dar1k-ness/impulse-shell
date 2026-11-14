# effects_oracle.py SCENE.qml OUT.rgba [OBJECT.PROPERTY...]
# Shows the scene (an Item root) in a QQuickView at the devicePixelRatio QT_SCALE_FACTOR gives
# it (or, with ORACLE_DPR set, at that ratio once the compositor's output scale gave it to the
# window) and writes the device pixels the root covers, once the third frame was swapped: u32 width,
# u32 height (little endian), then premultiplied RGBA rows. Prints "DPR <ratio>" and, per
# argument, "PROP <objectName.property> <value>". Needs a platform with OpenGL (wayland under a
# headless sway): the effects are shaders, which the software backend of `offscreen` skips.
# With ORACLE_CURSOR_FLASH_TIME set, that is QStyleHints::cursorFlashTime in ms: 0 is a caret
# that does not blink, for a scene whose caret must be in the shot whenever it is taken.
import os
import struct
import sys

from PySide6.QtCore import QObject, QUrl
from PySide6.QtGui import QGuiApplication, QImage
from PySide6.QtQuick import QQuickView

src, out, props = sys.argv[1], sys.argv[2], sys.argv[3:]
app = QGuiApplication(sys.argv)
flash = os.environ.get("ORACLE_CURSOR_FLASH_TIME")
if flash is not None:
    app.styleHints().setCursorFlashTime(int(flash))
v = QQuickView()
v.setSource(QUrl.fromLocalFile(src))
if v.status() != QQuickView.Status.Ready:
    sys.exit(f"{src}: {v.errors()}")
root = v.rootObject()
# A window of one fixed size floats in sway. Tiled, it would get the output's size, which the
# scale may not divide, and Qt would stretch the scene by that fraction of a pixel.
v.setMinimumSize(v.size())
v.setMaximumSize(v.size())
frames = 0


want_dpr = os.environ.get("ORACLE_DPR")


def swapped():
    global frames
    if want_dpr and v.effectiveDevicePixelRatio() != float(want_dpr):
        v.requestUpdate()
        return
    frames += 1
    if frames < 3:
        v.requestUpdate()
        return
    if frames > 3:
        return
    dpr = v.effectiveDevicePixelRatio()
    w, h = round(root.width() * dpr), round(root.height() * dpr)
    img = v.grabWindow().convertToFormat(QImage.Format.Format_RGBA8888_Premultiplied)
    assert (img.width(), img.height()) == (w, h), (img.size(), w, h)
    with open(out, "wb") as f:
        f.write(struct.pack("<II", w, h))
        for y in range(h):
            f.write(bytes(img.constScanLine(y))[: w * 4])
    print("DPR", dpr)
    for p in props:
        name, prop = p.split(".")
        print("PROP", p, root.findChild(QObject, name).property(prop))
    sys.stdout.flush()
    # Not app.quit(): tearing the window down from inside its frameSwapped handler crashes.
    os._exit(0)


def scale_known():
    global root
    if v.effectiveDevicePixelRatio() != float(scale):
        return
    v.devicePixelRatioChanged.disconnect(scale_known)
    # Loaded again: a layer made before keeps its texture's size (qquickshadereffectsource.cpp:764).
    v.setSource(QUrl())
    v.setSource(QUrl.fromLocalFile(src))
    if v.status() != QQuickView.Status.Ready:
        sys.exit(f"{src}: {v.errors()}")
    root = v.rootObject()
    v.frameSwapped.connect(swapped)


# ORACLE_DPR: the scale the compositor's output gives the window (wp_fractional_scale_v1), which
# the window hears of before or after its first frame. The frames are counted from then on.
scale = os.environ.get("ORACLE_DPR")
if scale and v.effectiveDevicePixelRatio() != float(scale):
    v.devicePixelRatioChanged.connect(scale_known)
else:
    v.frameSwapped.connect(swapped)
v.show()
app.exec()
