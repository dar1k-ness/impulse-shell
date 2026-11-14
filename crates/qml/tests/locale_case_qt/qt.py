#!/usr/bin/env python3
"""Writes what Qt 6.12 answers to each expression of the .cases files here behind its `=> `: the
`typeof` and the text of it, in a process whose locale variables are those of the block's `@`
line and no others. The Qt is the image's own, with the system's ICU
(tools/testenv/system_qt.py): the one inside the PySide6 wheel carries ICU 73, whose case
mapping is Unicode 15's. In the tools/testenv image:
  tools/ci/gate --run as-tester /opt/qv4-venv/bin/python \
      /src/crates/qml/tests/locale_case_qt/qt.py"""
import json, os, subprocess, sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "../../../../tools/testenv"))
import system_qt  # the Qt that answers is the system's, checked there

HERE = os.path.dirname(os.path.abspath(__file__))
VARS = ("LANG", "LANGUAGE")

if len(sys.argv) > 1:
    from PySide6.QtCore import QCoreApplication, QUrl
    from PySide6.QtQml import QQmlComponent, QQmlEngine

    system_qt.checked()
    app = QCoreApplication(sys.argv)
    engine = QQmlEngine()
    asked = json.loads(sys.stdin.read())
    shown = ",\n".join("(v => typeof v + ':' + v)(%s)" % e for e in asked)
    comp = QQmlComponent(engine)
    comp.setData(("import QtQml\nQtObject { property var answers: [\n%s\n] }" % shown).encode(),
                 QUrl.fromLocalFile(os.path.join(HERE, "asked.qml")))
    root = comp.create()
    if root is None:
        sys.exit(comp.errorString())
    sys.stdout.write(json.dumps([str(a) for a in root.property("answers").toVariant()]))
    sys.exit(0)

for name in sorted(n for n in os.listdir(HERE) if n.endswith(".cases")):
    path = os.path.join(HERE, name)
    lines = open(path, encoding="utf-8").read().split("\n")
    blocks = []
    for line in lines:
        if line.startswith("@ "):
            blocks.append((line, []))
        elif line and not line.startswith("=> "):
            blocks[-1][1].append(line)
    out = []
    for head, asked in blocks:
        env = {k: v for k, v in os.environ.items() if k not in VARS and not k.startswith("LC_")}
        env.update(v.split("=", 1) for v in head[2:].split())
        env["QT_QPA_PLATFORM"] = "offscreen"
        qt = subprocess.run([sys.executable, __file__, "--block"], env=env, check=True,
                            input=json.dumps(asked).encode(), stdout=subprocess.PIPE).stdout
        out.append(head)
        for expression, answer in zip(asked, json.loads(qt)):
            out += [expression, "=> " + answer]
    open(path, "w", encoding="utf-8").write("\n".join(out) + "\n")
    print(name, sum(len(b[1]) for b in blocks), "answers of", len(blocks), "environments")
