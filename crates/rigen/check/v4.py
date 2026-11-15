#!/usr/bin/env python3
"""Prints `<case> <result>` as Qt's V4 engine (the Qt that runs `qs -c impulse-shell`) computes it,
for the `// v4: <result>` pins in cases/. The Qt that answers is the system's, with the
system's ICU (tools/testenv/system_qt.py), through the bindings of the matching PySide6:
in ri-testenv /opt/qv4-venv (tools/testenv/Containerfile).
Usage: QT_QPA_PLATFORM=offscreen /opt/qv4-venv/bin/python v4.py cases/ii_color_utils.js ..."""
import os, shutil, sys, tempfile

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "../../../tools/testenv"))
import system_qt  # the Qt that answers is the system's, checked there

from PySide6.QtCore import QUrl, qInstallMessageHandler
from PySide6.QtGui import QGuiApplication
from PySide6.QtQml import QQmlComponent, QQmlEngine

system_qt.checked()
app = QGuiApplication(sys.argv)
out = {}


def handler(_mode, _ctx, msg):
    if msg.startswith("V4OUT "):
        name, res = msg[6:].split(" ", 1)
        out[name] = res
    else:
        sys.stderr.write(msg + "\n")


qInstallMessageHandler(handler)
work = tempfile.mkdtemp()
for path in sys.argv[1:]:
    name = os.path.basename(path)[:-3]
    shutil.copy(path, f"{work}/{name}.js")
    # Files the cases pass to Qt.include, at the same relative URLs.
    included = os.path.join(os.path.dirname(path), "included")
    if os.path.isdir(included):
        shutil.copytree(included, f"{work}/included", dirs_exist_ok=True)
    # QtQuick registers the colour provider that Qt.color/Qt.rgba need.
    with open(f"{work}/t_{name}.qml", "w") as f:
        f.write(f'import QtQuick\nimport "{name}.js" as C\n'
                f'QtObject {{ Component.onCompleted: console.log("V4OUT {name} " + String(C.result)) }}\n')
    engine = QQmlEngine()
    comp = QQmlComponent(engine, QUrl.fromLocalFile(f"{work}/t_{name}.qml"))
    if comp.create() is None:
        sys.stderr.write(f"{name}: {comp.errorString()}\n")
for name, res in out.items():
    print(name, res)
