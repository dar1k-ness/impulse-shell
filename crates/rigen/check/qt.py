#!/usr/bin/env python3
"""What Qt leaves in the root's `result` for QML cases: the value their `// qt:` line pins.

    QT_QPA_PLATFORM=offscreen python qt.py cases/qml/*.qml

Needs PySide6 (the testenv image has 6.12.0 in /opt/qv4-venv). Prints `<case>: <result>` per
file, and exits 1 if one differs from its `// qt:` line.
"""

import os
import sys

from PySide6.QtCore import QTimer, QUrl
from PySide6.QtGui import QGuiApplication
from PySide6.QtQml import QQmlComponent, QQmlEngine

app = QGuiApplication(sys.argv[:1])
bad = 0
for path in sys.argv[1:]:
    name = os.path.splitext(os.path.basename(path))[0]
    engine = QQmlEngine()
    comp = QQmlComponent(engine, QUrl.fromLocalFile(os.path.abspath(path)))
    root = comp.create()
    if root is None:
        got = "load error " + "; ".join(e.toString() for e in comp.errors())
    else:
        # Queued work (a Loader's incubation, a Timer of interval 0) runs from the event loop.
        QTimer.singleShot(50, app.quit)
        app.exec()
        got = root.property("result")
    want = None
    with open(path) as f:
        for line in f:
            if line.startswith("// qt: "):
                want = line[len("// qt: "):].rstrip("\n")
    mark = "" if want is None or want == got else f"   != pinned {want!r}"
    bad += bool(mark)
    print(f"{name}: {got}{mark}")
    del root, comp, engine
sys.exit(1 if bad else 0)
