import sys, os, glob
os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
from PySide6.QtCore import QUrl, QCoreApplication, QTimer, QMetaObject, qInstallMessageHandler, Q_ARG
from PySide6.QtGui import QGuiApplication
from PySide6.QtQml import QQmlEngine, QQmlComponent
app = QGuiApplication(sys.argv[:1])
msgs = []
qInstallMessageHandler(lambda t, c, m: msgs.append(m))
os.makedirs(sys.argv[2], exist_ok=True)
for f in sorted(glob.glob(sys.argv[1] + "/*.qml")):
    name = os.path.basename(f)[:-4]
    msgs.clear()
    eng = QQmlEngine()
    c = QQmlComponent(eng, QUrl.fromLocalFile(os.path.abspath(f)))
    if c.isError():
        res = "LOADERR " + " ## ".join("%d:%d: %s" % (e.line(), e.column(), e.description()) for e in c.errors()) + "\n"
    else:
        o = c.create()
        if o is None:
            res = "CREATEERR " + " ## ".join(e.toString() for e in c.errors()) + "\n"
        else:
            for _ in range(5): app.processEvents()
            QMetaObject.invokeMethod(o, 'go')
            for k in range(1, 14):
                QMetaObject.invokeMethod(o, 'step', Q_ARG('QVariant', k))
                for _ in range(3): app.processEvents()
            QMetaObject.invokeMethod(o, 'fin')
            res = o.property("out")
    res += "".join("  W " + m.replace(os.path.abspath(sys.argv[1]), "") + "\n" for m in msgs)
    open(os.path.join(sys.argv[2], name + ".txt"), "w").write(res)
    print(name, len(res.splitlines()), flush=True)
