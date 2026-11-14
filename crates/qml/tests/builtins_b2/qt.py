"""Prints the `log` property of the QML file argv[1] as Qt creates it."""
import sys
from PySide6.QtCore import QUrl
from PySide6.QtGui import QGuiApplication
from PySide6.QtQml import QQmlComponent, QQmlEngine

app = QGuiApplication(sys.argv[:1])
engine = QQmlEngine()
comp = QQmlComponent(engine, QUrl.fromLocalFile(sys.argv[1]))
obj = comp.create()
if obj is None:
    sys.exit(comp.errorString())
sys.stdout.write(obj.property("log"))
