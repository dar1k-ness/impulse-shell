#!/opt/qv4-venv/bin/python
"""What Qt 6.12 gives for a scene here: `snap()` as loaded and after each of its steps, with
`==` between them. Run in the tools/testenv image:

    docker run --rm -u "$(id -u):$(id -g)" -e HOME=/tmp -v "$PWD":/src -w /src \
        --entrypoint /opt/qv4-venv/bin/python ri-testenv:<tag> \
        crates/shell/tests/scenes/basic_sizes/qt.py sizes grow restyle strip shrink
    ... qt.py fields type narrow clear
    ... qt.py combo write reset strip

Writes `<scene>.qt` next to this file.
"""
import os, sys

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
os.environ["QT_QUICK_CONTROLS_STYLE"] = "Basic"
from PySide6.QtCore import QEventLoop, QMetaObject, QTimer, QUrl, Q_RETURN_ARG
from PySide6.QtGui import QGuiApplication
from PySide6.QtQuick import QQuickView

HERE = os.path.dirname(os.path.abspath(__file__))
app = QGuiApplication(sys.argv[:1])
v = QQuickView()
v.setSource(QUrl.fromLocalFile(os.path.join(HERE, sys.argv[1] + ".qml")))
for e in v.errors():
    sys.exit(e.toString())
v.show()


def frames():
    loop = QEventLoop()
    QTimer.singleShot(300, loop.quit)
    loop.exec()


out = []
for step in [""] + sys.argv[2:]:
    if step:
        QMetaObject.invokeMethod(v.rootObject(), step)
    frames()
    out.append(QMetaObject.invokeMethod(v.rootObject(), "snap", Q_RETURN_ARG("QVariant")))
open(os.path.join(HERE, sys.argv[1] + ".qt"), "w").write("\n==\n".join(out) + "\n")
