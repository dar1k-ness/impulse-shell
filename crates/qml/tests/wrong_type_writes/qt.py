#!/opt/qv4-venv/bin/python
r"""What Qt 6.12 does with each value of `values.tsv` written to each property of `props.tsv`,
by a script (`s.p = value`) and as the result of a binding (`p: v`, then `v = value`).
Run in the tools/testenv image (PySide6 6.12 on the image's own Qt,
tools/testenv/system_qt.py) from the repository root:

    docker run --rm -u "$(id -u):$(id -g)" -e HOME=/tmp -v "$PWD":/src -w /src \
        --entrypoint /opt/qv4-venv/bin/python ri-testenv:<tag> \
        crates/qml/tests/wrong_type_writes/qt.py

Writes `qt.tsv` next to this file: `S` or `B`, the property, the value, what `script` or
`bind` of probe.qml returns, and for a binding the `Unable to assign ...` Qt logs. An address
is written `PTR` and a character outside printable ASCII `\u{hex}`.
"""
import os, re, sys

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
os.environ["TZ"] = "UTC"
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "../../../../tools/testenv"))
import system_qt  # the Qt that answers is the system's, checked there

from PySide6.QtCore import QUrl, qInstallMessageHandler
from PySide6.QtGui import QGuiApplication
from PySide6.QtQml import QJSValue, QQmlComponent, QQmlEngine

HERE = os.path.dirname(os.path.abspath(__file__))
read = lambda n: open(os.path.join(HERE, n)).read()
values = [l.split("\t") for l in read("values.tsv").splitlines()]
log = []
qInstallMessageHandler(lambda t, ctx, msg: log.append(msg))
app = QGuiApplication(sys.argv[:1])
eng = QQmlEngine()
keep, out = [], []


def text(r):
    r = re.sub(r"\(0x[0-9a-f]+\)", "(PTR)", r)
    return re.sub(r"[^\x20-\x7e]", lambda m: "\\u{%x}" % ord(m.group()), r)


for l in read("props.tsv").splitlines():
    label, path, good, s, b = l.split("\t")
    src = (read("probe.qml").replace("/*S*/", s).replace("/*B*/", b)
           .replace("/*PATH*/", path).replace("/*GOOD*/", good)
           .replace("/*VALUES*/", ",\n".join(f"        () => {e}" for _, e in values)))
    c = QQmlComponent(eng)
    keep.append(c)
    c.setData(src.encode(), QUrl("file:///nonexistent/wrong_type/x.qml"))
    o = c.create()
    assert o is not None, c.errors()
    js = eng.newQObject(o)
    for i, (v, _) in enumerate(values):
        r = js.property("script").callWithInstance(js, [QJSValue(i)]).toString()
        out.append(f"S\t{label}\t{v}\t{text(r)}\t")
        del log[:]
        r = js.property("bind").callWithInstance(js, [QJSValue(i)]).toString()
        after = log[log.index("CASE") + 1:]
        warn = [m[m.index("Unable to assign"):] for m in after if "Unable to assign" in m]
        out.append(f"B\t{label}\t{v}\t{text(r)}\t{' ; '.join(warn)}")
open(os.path.join(HERE, "qt.tsv"), "w").write("\n".join(out) + "\n")
