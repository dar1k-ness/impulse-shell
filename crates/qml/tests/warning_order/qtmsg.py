import sys, os
from PySide6.QtCore import QUrl, qInstallMessageHandler
from PySide6.QtGui import QGuiApplication
from PySide6.QtQml import QQmlApplicationEngine

d = "file://" + os.path.dirname(os.path.abspath(sys.argv[1])) + "/"
def on_message(kind, context, text):
    k = "console" if context.category == "qml" else "warning"
    sys.stdout.write(k + ": " + text.replace(d, "") + "\n")

qInstallMessageHandler(on_message)
app = QGuiApplication(sys.argv[:1])
engine = QQmlApplicationEngine()
engine.load(QUrl.fromLocalFile(sys.argv[1]))
if not engine.rootObjects():
    sys.exit(3)
sys.exit(app.exec())
