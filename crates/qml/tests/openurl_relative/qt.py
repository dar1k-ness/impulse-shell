# What Qt 6.12 hands to `xdg-open` for every case of Open.qml: `qt.sh` runs this.
import json, os, sys
from PySide6.QtCore import QUrl
from PySide6.QtGui import QGuiApplication
from PySide6.QtQml import QQmlEngine, QQmlComponent

doc, fifo = sys.argv[1], sys.argv[2]
app = QGuiApplication(sys.argv[:1])
engine = QQmlEngine()
comp = QQmlComponent(engine, QUrl.fromLocalFile(doc))
obj = comp.create()
if obj is None:
    sys.exit(comp.errorString())
calls = os.fdopen(os.open(fifo, os.O_RDWR), "r", encoding="utf-8", newline="\n")
cases, kinds = obj.property("cases").toVariant(), obj.property("kinds").toVariant()
for at in range(len(cases) * len(kinds)):
    obj.setProperty("at", at)
    said = obj.property("said")
    # The program is started detached: what it was given comes when it has run.
    arg = calls.readline().rstrip("\n") if said == "true" else "-"
    print("\t".join([json.dumps(cases[at // 3], ensure_ascii=False), kinds[at % 3], said, arg]))
