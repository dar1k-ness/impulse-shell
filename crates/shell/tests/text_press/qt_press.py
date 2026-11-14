# The answer of Qt for press.qml: `python qt_press.py press.qml 50,30 50,130` (PySide6).
import os
import sys

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
from PySide6.QtCore import QMetaObject, QPoint, Qt, QUrl
from PySide6.QtGui import QGuiApplication
from PySide6.QtQuick import QQuickView
from PySide6.QtTest import QTest

app = QGuiApplication(sys.argv[:1])
v = QQuickView()
v.setSource(QUrl.fromLocalFile(os.path.abspath(sys.argv[1])))
v.show()
v.requestActivate()
QTest.qWaitForWindowExposed(v)
for a in sys.argv[2:]:
    x, y = a.split(",")
    QTest.mouseClick(v, Qt.LeftButton, Qt.NoModifier, QPoint(int(x), int(y)))
    QTest.qWait(50)
QMetaObject.invokeMethod(v.rootObject(), "done")
print("qt:", v.rootObject().property("result"))
