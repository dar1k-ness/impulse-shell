"""The one way a fixture or a generator asks Qt 6.12: PySide6's bindings on the image's own Qt
(qt6-base and qt6-declarative of Arch, the libraries /usr/lib/qt6/bin/qml runs on), not on the
Qt inside the PySide6 wheel. The wheel's Qt carries an ICU of its own, 73, and Arch's uses the
system's, so what comes of ICU's data (a zone's names, case mapping, collation) is another
answer there, and the Qt the shell replaces is the system's. A script imports this module
before PySide6:

    sys.path.insert(0, ".../tools/testenv")
    import system_qt

That starts the script again with the system's Qt libraries loaded ahead of the wheel's, and
ends it with a text and status 1 (checked() at once, the end of the script if it was not
asked) unless every Qt and ICU library the process has loaded is the system's and its ICU is
the system's libicu, of the version ICU below.

    python tools/testenv/system_qt.py     prints the Qt and the ICU that answer"""
import atexit
import glob
import os
import re
import sys

# The ICU the answers in the tree were taken with (docs/qt-differences.md says so too). With
# another one in the image they are taken again, and this line changed.
ICU = "78.3"
LIBDIR = "/usr/lib"
# The Qt libraries the bindings of QtCore, QtGui and QtQml load.
LIBRARIES = ("Core", "Gui", "Network", "Qml", "QmlMeta", "QmlModels", "QmlWorkerScript")
PRELOAD = " ".join("%s/libQt6%s.so.6" % (LIBDIR, name) for name in LIBRARIES)


def system_icu():
    """The version of the image's libicu, as its file is named: `78.3`."""
    files = glob.glob(LIBDIR + "/libicuuc.so.*")
    return sorted({os.path.basename(os.path.realpath(f)).partition(".so.")[2] for f in files})


def loaded():
    """The files of Qt and of ICU this process has loaded."""
    with open("/proc/self/maps") as maps:
        files = {line.split(None, 5)[5].rstrip("\n") for line in maps if line.count("/")}
    return sorted(f for f in files if re.search(r"/lib(Qt6|icu)[^/]*\.so|/PySide6/Qt/", f))


def wrong():
    """Why this process is not the system's Qt with the ICU of ICU, or nothing."""
    files = loaded()
    foreign = [f for f in files if os.path.dirname(os.path.realpath(f)) != LIBDIR]
    if foreign:
        return "these are not the system's Qt and ICU: " + ", ".join(foreign)
    if not any(os.path.basename(f).startswith("libQt6Core.so") for f in files):
        return "no Qt is loaded"
    answering = sorted({os.path.basename(os.path.realpath(f)).partition(".so.")[2]
                        for f in files if os.path.basename(f).startswith("libicuuc.so")})
    if answering != system_icu():
        return "Qt answers with ICU %s, the system's libicu is %s" % (
            " and ".join(answering) or "none", " and ".join(system_icu()) or "none")
    if answering != [ICU]:
        return ("the image's ICU is %s, the answers in the tree are of ICU %s: take them again "
                "and change ICU in tools/testenv/system_qt.py and docs/qt-differences.md"
                % (answering[0], ICU))
    return None


def checked():
    """Ends the script with a text unless the Qt that answers in it is the system's."""
    why = wrong()
    if why:
        sys.stderr.write("tools/testenv/system_qt.py: %s\n" % why)
        sys.stderr.flush()
        os._exit(1)


if os.environ.get("LD_PRELOAD") != PRELOAD:
    os.execve(sys.executable, [sys.executable] + sys.argv,
              {**os.environ, "LD_PRELOAD": PRELOAD, "QML_IMPORT_PATH": LIBDIR + "/qt6/qml",
               "QT_PLUGIN_PATH": LIBDIR + "/qt6/plugins"})
atexit.register(checked)

if __name__ == "__main__":
    from PySide6.QtCore import QCoreApplication, QUrl, qVersion
    from PySide6.QtQml import QQmlComponent, QQmlEngine

    app = QCoreApplication(sys.argv[:1])
    engine = QQmlEngine()
    component = QQmlComponent(engine)
    component.setData(b"import QtQml\nQtObject { property string lower: 'I'.toLocaleLowerCase('tr') }",
                      QUrl.fromLocalFile("/asked.qml"))
    root = component.create()
    if root is None or root.property("lower") != "\u0131":
        sys.exit("tools/testenv/system_qt.py: Qt's QML engine does not answer: " + component.errorString())
    checked()
    print("Qt %s with ICU %s" % (qVersion(), ICU))
