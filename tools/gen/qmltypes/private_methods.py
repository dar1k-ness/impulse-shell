"""Lists the private methods of the C++ classes behind Qt's creatable QML types (and their
bases): `Class method` per line. qmltypes files list every slot without its access; QML only
sees the non-private ones (QQmlPropertyCache::append skips QMetaMethod::Private)."""
import os, re, sys, glob
os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
from PySide6.QtCore import QMetaMethod, QUrl
from PySide6.QtGui import QGuiApplication
from PySide6.QtQml import QQmlEngine, QQmlComponent
app = QGuiApplication(sys.argv[:1])
eng = QQmlEngine()
root = sys.argv[1]
types = set()
for f in glob.glob(root + "/**/*.qmltypes", recursive=True):
    for m in re.finditer(r'"([A-Za-z0-9_.]+)/([A-Za-z0-9_]+) \d+\.\d+"', open(f).read()):
        types.add((m.group(1), m.group(2)))
out = set(); seen = set(); made = 0
keep = []
for mod, ty in sorted(types):
    if mod.startswith(("Rustshell", "Impulse")): continue
    c = QQmlComponent(eng)
    c.setData(("import %s\n%s {}" % (mod, ty)).encode(), QUrl())
    if c.isError(): continue
    o = c.create()
    if o is None: continue
    keep.append(o); made += 1
    mo = o.metaObject()
    while mo is not None:
        n = mo.className()
        if n not in seen:
            seen.add(n)
            for i in range(mo.methodOffset(), mo.methodCount()):
                m = mo.method(i)
                if m.access() == QMetaMethod.Access.Private:
                    out.add((n, bytes(m.name()).decode()))
        mo = mo.superClass()
print("# created", made, "of", len(types), "types;", len(seen), "classes", file=sys.stderr)
for n, m in sorted(out): print(n, m)
