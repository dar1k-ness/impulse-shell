"""probe.py DIR: for each DIR/*.qml prints `name status json(errorString)` as Qt 6.12 answers."""
import json, os, sys
os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
from PySide6.QtCore import QUrl, qInstallMessageHandler
from PySide6.QtGui import QGuiApplication
from PySide6.QtQml import QQmlEngine, QQmlComponent
qInstallMessageHandler(lambda *a: None)
app = QGuiApplication(sys.argv[:1])
eng = QQmlEngine()
d = os.path.abspath(sys.argv[1])
pre = QUrl.fromLocalFile(d).toString() + "/"
for n in sorted(os.listdir(d)):
    if not n.endswith(".qml"):
        continue
    c = QQmlComponent(eng, QUrl.fromLocalFile(os.path.join(d, n)))
    print(n[:-4], int(c.status().value), json.dumps(c.errorString().replace(pre, ""), ensure_ascii=False), flush=True)
