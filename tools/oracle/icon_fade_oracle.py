# icon_fade_oracle.py SCENE.qml
# Runs the scene (a Window) and gives it `fade`, whose time() is the time in ms of the fade of
# the Kirigami.Icon with objectName "first", or -1 while the icon does not fade: the
# currentTime of the icon's own QPropertyAnimation, a child object QML cannot reach. It only
# reads. Prints "FADE <class> <duration>" at the first call and, once the scene is loaded,
# "LIBRARIES <path>..." with every Qt library, plugin and QML module the process has loaded.
# The Qt is the image's own (tools/testenv/system_qt.py).
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "../testenv"))
import system_qt

from PySide6.QtCore import QAbstractAnimation, QObject, QUrl, Slot
from PySide6.QtGui import QGuiApplication
from PySide6.QtQml import QQmlApplicationEngine


class Fade(QObject):
    def __init__(self, engine):
        super().__init__()
        self.engine = engine
        # Kept: a wrapper that goes away takes the wrappers of the object's children with it.
        self.window = self.icon = self.animation = None

    @Slot(result=int)
    def time(self):
        if self.animation is None:
            self.window = self.engine.rootObjects()[0]
            self.icon = self.window.findChild(QObject, "first")
            self.animation = self.icon.findChild(QAbstractAnimation)
            name = self.animation.metaObject().className()
            print("FADE", name, self.animation.duration(), file=sys.stderr, flush=True)
        if self.animation.state() != QAbstractAnimation.State.Running:
            return -1
        return self.animation.currentTime()


system_qt.checked()
app = QGuiApplication(sys.argv)
engine = QQmlApplicationEngine()
fade = Fade(engine)
engine.rootContext().setContextProperty("fade", fade)
engine.load(QUrl.fromLocalFile(sys.argv[1]))
if not engine.rootObjects():
    sys.exit(f"{sys.argv[1]} did not load")
with open("/proc/self/maps") as maps:
    paths = {line.split()[-1] for line in maps if "/" in line}
print(
    "LIBRARIES",
    *sorted(p for p in paths if any(part in p for part in ("/libQt6", "irigami", "/plugins/", "/qml/"))),
    file=sys.stderr,
    flush=True,
)
sys.exit(app.exec())
