"""Prints what show.qml's show() answers in Qt, a line a question: the group, the question
and the answer. Without arguments the groups are the locales Qt knows that write numbers in
digits other than ASCII's, each by its language, script and territory, then CONTROLS; with
`--system TEXT` the one group `system TEXT`, the system locale of this process's environment.
qt.sh runs it. The Qt is the image's own, with the system's ICU (tools/testenv/system_qt.py)."""
import os, sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "../../../../tools/testenv"))
import system_qt  # the Qt that answers is the system's, checked there
from PySide6.QtCore import QCoreApplication, QLocale, QMetaObject, QUrl, Q_ARG, Q_RETURN_ARG
from PySide6.QtQml import QQmlComponent, QQmlEngine

system_qt.checked()

HERE = os.path.dirname(os.path.abspath(__file__))
# Locales with ASCII digits, and names Qt reads as it reads those of the environment.
CONTROLS = ["C", "en_US", "de_DE", "ru_RU", "hi_IN", "ar_SA", "ar-EG", "AR_eg.UTF-8", "ff_Adlm", "ff_GN",
            "sat_Deva_IN", "xx_YY"]

app = QCoreApplication(sys.argv[:1])
engine = QQmlEngine()
component = QQmlComponent(engine, QUrl.fromLocalFile(os.path.join(HERE, "show.qml")))
root = component.create()
if root is None:
    sys.exit("; ".join(e.toString() for e in component.errors()))


def show(group, name):
    text = QMetaObject.invokeMethod(root, "show", Q_RETURN_ARG("QVariant"), Q_ARG("QVariant", name))
    for line in text.split("\n"):
        print("%s\t%s" % (group, line))


if sys.argv[1:2] == ["--system"]:
    show("system " + sys.argv[2], "")
else:
    every = QLocale.matchingLocales(QLocale.Language.AnyLanguage, QLocale.Script.AnyScript,
                                    QLocale.Country.AnyTerritory)
    names = sorted({"_".join([QLocale.languageToCode(l.language()), QLocale.scriptToCode(l.script()),
                              QLocale.territoryToCode(l.territory())])
                    for l in every if l.zeroDigit() != "0"})
    for name in names + CONTROLS:
        show(name, name)
