#!/usr/bin/env python3
"""aotcheck's QV4 oracle (see v4.rs): evaluates each program of a JSON-lines file (one JSON
string per line, a JS expression) in Qt's V4 engine, the Qt that runs `qs -c rusty-impulse`, and prints
one result per line (`\\`, newlines escaped). Needs PySide6 6.12 (PySide6-Essentials) and
system_qt.py (tools/testenv) next to this file: the Qt that answers is the system's, with the
system's ICU, not the one inside the wheel."""
import json, sys

import system_qt  # checked there

from PySide6.QtCore import QUrl, qInstallMessageHandler
from PySide6.QtGui import QGuiApplication
from PySide6.QtQml import QQmlComponent, QQmlEngine

system_qt.checked()
app = QGuiApplication(sys.argv)
qInstallMessageHandler(lambda _m, _c, msg: sys.stderr.write(msg + "\n"))
engine = QQmlEngine()
# Load QtQuick like the original's files do (it registers the value types QML bindings see).
comp = QQmlComponent(engine)
comp.setData(b"import QtQuick\nQtObject {}\n", QUrl())
keep = comp.create()
if keep is None:
    sys.exit("QtQuick: " + comp.errorString())
out = sys.stdout
with open(sys.argv[1], encoding="utf-8") as f:
    for line in f:
        r = engine.evaluate(json.loads(line))
        s = ("v4 uncaught " if r.isError() else "") + r.toString()
        out.write(s.replace("\\", "\\\\").replace("\n", "\\n").replace("\r", "\\r") + "\n")
out.flush()
