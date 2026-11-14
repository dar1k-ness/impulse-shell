"""Prints what Qt says for Main.qml (argv[1]): the rows of `all()`, the directory as DIR."""
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "../../../../tools/testenv"))
import system_qt  # the Qt that answers is the system's, checked there

from PySide6.QtCore import QUrl
from PySide6.QtGui import QGuiApplication
from PySide6.QtQml import QJSValue, QQmlComponent, QQmlEngine
from PySide6.QtCore import QMetaObject, Q_RETURN_ARG

app = QGuiApplication(sys.argv[:1])
engine = QQmlEngine()
comp = QQmlComponent(engine, QUrl.fromLocalFile(sys.argv[1]))
obj = comp.create()
if obj is None:
    sys.exit(comp.errorString())
rows = QMetaObject.invokeMethod(obj, "all", Q_RETURN_ARG("QVariant"))
here = "file://" + os.path.dirname(os.path.abspath(sys.argv[1]))
sys.stdout.write(str(rows).replace(here, "DIR") + "\n")
