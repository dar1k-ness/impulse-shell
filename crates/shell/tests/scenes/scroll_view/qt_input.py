# qt_input.py SCENE.qml SCENE.steps
# What Qt 6.12 gives for a scene of this directory under the pointer and the keys: its `snap()`
# as loaded and after each line of SCENE.steps, with `==` between them. A step is
#   move X Y       the pointer goes to X, Y of the window
#   wheel X Y      the pointer goes there and the wheel turns one click down
#   drag X Y DY    the left button goes down at X, Y, the pointer goes DY down in steps of 5
#                  and the button goes up
#   press X Y DY   the same, and the button stays down
#   release        the button goes up where the pointer is
#   key NAME N     the key Qt.Key_NAME is pressed and released N times; Ctrl+NAME holds Ctrl
#   type TEXT      each letter and digit of TEXT is a key pressed and released; `_` is Space
#   call NAME      the scene's function NAME runs
# Run in the testenv image (PySide6 6.12 on the image's own Qt, tools/testenv/system_qt.py):
#   QT_QPA_PLATFORM=offscreen QT_QUICK_CONTROLS_STYLE=Basic /opt/qv4-venv/bin/python \
#       qt_input.py Input.qml Input.steps > Input.qt
import ctypes
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "../../../../../tools/testenv"))
import system_qt  # the Qt that answers is the system's, checked there

# system_qt.py puts the system's Qt of QtCore, QtGui and QtQml ahead of the wheel's; a window
# and its events need these too, and the bindings take the ones already loaded.
for name in ("OpenGL", "Quick", "Widgets", "Test"):
    ctypes.CDLL("%s/libQt6%s.so.6" % (system_qt.LIBDIR, name), ctypes.RTLD_GLOBAL)
from PySide6.QtCore import (QCoreApplication, QMetaObject, QPoint, QPointF, Qt, QTimer, QUrl,
                            Q_RETURN_ARG, qInstallMessageHandler)
from PySide6.QtGui import QGuiApplication, QWheelEvent
from PySide6.QtQuick import QQuickView
from PySide6.QtTest import QTest

qInstallMessageHandler(lambda t, c, m: print("QT: " + m, file=sys.stderr, flush=True))
app = QGuiApplication(sys.argv)
# The offscreen platform has no hover effects; a desktop with a mouse has them.
app.styleHints().setUseHoverEffects(True)
v = QQuickView()
v.setSource(QUrl.fromLocalFile(os.path.abspath(sys.argv[1])))
if v.status() != QQuickView.Status.Ready:
    sys.exit(f"{sys.argv[1]}: {v.errors()}")
v.show()
v.requestActivate()
steps = [""] + open(sys.argv[2]).read().split("\n")
steps = [s for n, s in enumerate(steps) if n == 0 or s]
out = []
held = QPoint()


def step():
    s = steps.pop(0).split()
    if s and s[0] in ("move", "wheel"):
        at = QPoint(int(s[1]), int(s[2]))
        QTest.mouseMove(v, at - QPoint(1, 1))
        QTest.mouseMove(v, at)
        if s[0] == "wheel":
            p = QPointF(at)
            QCoreApplication.sendEvent(v, QWheelEvent(
                p, QPointF(v.mapToGlobal(at)), QPoint(), QPoint(0, -120), Qt.MouseButton.NoButton,
                Qt.KeyboardModifier.NoModifier, Qt.ScrollPhase.NoScrollPhase, False))
    elif s and s[0] in ("drag", "press"):
        global held
        at = QPoint(int(s[1]), int(s[2]))
        QTest.mouseMove(v, at - QPoint(1, 1))
        QTest.mouseMove(v, at)
        QTest.mousePress(v, Qt.MouseButton.LeftButton, Qt.KeyboardModifier.NoModifier, at)
        for dy in range(5, int(s[3]) + 1, 5):
            QTest.mouseMove(v, at + QPoint(0, dy))
        held = at + QPoint(0, int(s[3]))
        if s[0] == "drag":
            QTest.mouseRelease(v, Qt.MouseButton.LeftButton, Qt.KeyboardModifier.NoModifier, held)
    elif s and s[0] == "release":
        QTest.mouseRelease(v, Qt.MouseButton.LeftButton, Qt.KeyboardModifier.NoModifier, held)
    elif s and s[0] == "key":
        ctrl = s[1].startswith("Ctrl+")
        mods = Qt.KeyboardModifier.ControlModifier if ctrl else Qt.KeyboardModifier.NoModifier
        for _ in range(int(s[2])):
            QTest.keyClick(v, getattr(Qt.Key, "Key_" + s[1].removeprefix("Ctrl+")), mods)
    elif s and s[0] == "type":
        for c in s[1]:
            QTest.keyClick(v, Qt.Key.Key_Space if c == "_" else Qt.Key(ord(c.upper())))
    elif s and s[0] == "call":
        QMetaObject.invokeMethod(v.rootObject(), s[1])
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
