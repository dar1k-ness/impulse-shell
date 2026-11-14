"""Prints what Qt says for every case of Init.qml (argv[1]): the row of `go(n)` and the
errors the case printed as warnings, without their file and line."""
import os
import re
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "../../../../tools/testenv"))
import system_qt  # the Qt that answers is the system's, checked there

from PySide6.QtCore import QUrl, qInstallMessageHandler
from PySide6.QtGui import QGuiApplication
from PySide6.QtQml import QQmlComponent, QQmlEngine

said = [[]]


def handler(kind, context, message):
    if message == "MARK":
        said.append([])
    else:
        # Only what a refused write says: Qt's own remarks about value types are left out.
        text = re.sub(r"^file://\S*?\.qml(:\d+)?(:\d+)?: ", "", message)
        if re.match(r"\w*Error: ", text):
            said[-1].append(text)


qInstallMessageHandler(handler)
app = QGuiApplication(sys.argv[:1])
engine = QQmlEngine()
comp = QQmlComponent(engine, QUrl.fromLocalFile(sys.argv[1]))
obj = comp.create()
if obj is None:
    sys.exit(comp.errorString())
obj.finished.connect(app.quit)
obj.all()
app.exec()
for row, warned in zip(obj.property("report").split("\n"), said):
    row = row.replace("file://" + os.path.dirname(os.path.abspath(sys.argv[1])), "DIR")
    sys.stdout.write((row + " | warns " + "; ".join(warned)).rstrip() + "\n")
