"""Prints the `report` of Missing.qml (argv[1]) as Qt 6.12 writes it. The file the document
asks for by `want` is written into a new directory (`dir`) when it asks."""
import os
import shutil
import sys
import tempfile

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "../../../../tools/testenv"))
import system_qt  # the Qt that answers is the system's, checked there

from PySide6.QtCore import QMetaObject, QUrl
from PySide6.QtGui import QGuiApplication
from PySide6.QtQml import QQmlComponent, QQmlEngine

here = os.path.dirname(os.path.abspath(__file__))
tmp = tempfile.mkdtemp()
app = QGuiApplication(sys.argv[:1])
engine = QQmlEngine()
comp = QQmlComponent(engine, QUrl.fromLocalFile(sys.argv[1]))
obj = comp.createWithInitialProperties({"dir": tmp})
if obj is None:
    sys.exit(comp.errorString())


def wanted():
    name = obj.property("want")
    if name:
        shutil.copyfile(os.path.join(here, "picture.png"), os.path.join(tmp, name))
        QMetaObject.invokeMethod(obj, "fileThere")


obj.wantChanged.connect(wanted)
obj.finished.connect(app.quit)
wanted()
if not obj.property("report"):
    app.exec()
sys.stdout.write(obj.property("report") + "\n")
shutil.rmtree(tmp)
