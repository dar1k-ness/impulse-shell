"""qt.py <fixture.qml> [ms]: what Qt gives for a fixture of runtime_audit_r3.rs, the text of its
`.qt` file: the root's `log` after `ms` milliseconds of event loop, or the load errors.
Run with the Qt 6.12 of the ri-testenv image: /opt/qv4-venv/bin/python qt.py R05Keys.qml"""
import os
import sys

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
from PySide6.QtCore import QTimer, QUrl
from PySide6.QtGui import QGuiApplication
from PySide6.QtQml import QQmlComponent, QQmlEngine

app = QGuiApplication(sys.argv[:1])
engine = QQmlEngine()
path = os.path.abspath(sys.argv[1])
component = QQmlComponent(engine, QUrl.fromLocalFile(path))
root = None if component.isError() else component.create()
if root is None:
    for e in component.errors():
        print(e.toString().replace(QUrl.fromLocalFile(os.path.dirname(path)).toString() + "/", ""))
    sys.exit(0)
QTimer.singleShot(int(sys.argv[2]) if len(sys.argv) > 2 else 700, app.quit)
app.exec()
sys.stdout.write(root.property("log"))
