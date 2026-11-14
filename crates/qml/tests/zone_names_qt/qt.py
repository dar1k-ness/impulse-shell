"""Runs the steps of group argv[1] of steps.txt (the lines after its `fresh`) and prints, for
each `show`, the group's title, the step's own, and what show.qml's show() answers in Qt.
Exits 3 when there is no such group. qt.sh runs it, one process a group. The Qt is the image's
own, with the system's ICU (tools/testenv/system_qt.py)."""
import os, shutil, sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "../../../../tools/testenv"))
import system_qt  # the Qt that answers is the system's, checked there
from PySide6.QtCore import QCoreApplication, QMetaObject, QUrl, Q_RETURN_ARG
from PySide6.QtQml import QQmlComponent, QQmlEngine

system_qt.checked()

HERE = os.path.dirname(os.path.abspath(__file__))
LOCAL = "/etc/localtime"


def unlink():
    for path in (LOCAL, "/etc/lt2"):
        if os.path.lexists(path):
            os.remove(path)


def write(path, text):
    with open(path, "w") as f:
        f.write(text + "\n")


group, n, title, root = int(sys.argv[1]), -1, "", None
for line in open(os.path.join(HERE, "steps.txt")):
    op, _, arg = line.rstrip("\n").partition(" ")
    if op == "fresh":
        n, title = n + 1, arg
    elif n != group:
        pass
    elif op == "link":
        unlink()
        os.symlink(arg, LOCAL)
    elif op == "link2":
        unlink()
        os.symlink(arg, "/etc/lt2")
        os.symlink("/etc/lt2", LOCAL)
    elif op == "link2rel":
        unlink()
        os.symlink(arg, "/etc/lt2")
        os.symlink("lt2", LOCAL)
    elif op == "chain":
        count, target = arg.split(" ")
        count = int(count)
        unlink()
        for i in range(1, count):
            os.symlink(target if i == count - 1 else "/etc/c%d" % (i + 1), "/etc/c%d" % i)
        os.symlink("/etc/c1" if count > 1 else target, LOCAL)
    elif op == "tz":
        os.environ["TZ"] = arg
    elif op == "untz":
        os.environ.pop("TZ", None)
    elif op == "tzdir":
        os.environ["TZDIR"] = arg
    elif op == "copy":
        unlink()
        shutil.copy(arg, LOCAL)
    elif op == "timezone":
        write("/etc/timezone", arg)
    elif op == "retimezone":
        write("/etc/timezone.new", arg)
        os.replace("/etc/timezone.new", "/etc/timezone")
    elif op == "etctz":
        write("/etc/TZ", arg)
    elif op == "show":
        if root is None:
            app = QCoreApplication(sys.argv[:1])
            engine = QQmlEngine()
            component = QQmlComponent(engine, QUrl.fromLocalFile(os.path.join(HERE, "show.qml")))
            root = component.create()
            if root is None:
                sys.exit("; ".join(e.toString() for e in component.errors()))
        text = QMetaObject.invokeMethod(root, "show", Q_RETURN_ARG("QVariant"))
        print("%s\t%s\t%s" % (title, arg, text))
    else:
        sys.exit("steps.txt: " + line)
if n < group:
    sys.exit(3)
