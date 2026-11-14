# Scene: "W H bg" then "x y w h radius tb png"
import sys
from PySide6.QtGui import QGuiApplication
from PySide6.QtQuick import QQuickView
from PySide6.QtCore import QUrl, QTimer
src, out = sys.argv[1], sys.argv[2]
lines = [l.split() for l in open(src) if l.strip() and not l.startswith('#')]
W, H, bg = lines[0]
app = QGuiApplication(sys.argv)
def qcol(c):
    c = c.lstrip('#')
    return f'"#{c}"'
items = []
for i, (x, y, w, h, r, tb, path) in enumerate(lines[1:]):
    items.append(f'Image {{ id: img{i}; x:{x}; y:{y}; width:{w}; height:{h}; visible:false; source:"file://{path}" }}')
    items.append(f'GaussianBlur {{ anchors.fill: img{i}; source: img{i}; radius: {r}; samples: {2*int(r)+1}; transparentBorder: {"true" if tb=="1" else "false"} }}')
qml = f'import QtQuick\nimport Qt5Compat.GraphicalEffects\nRectangle {{ width:{W}; height:{H}; color:"{bg}"\n' + "\n".join(items) + '\n}\n'
open('/tmp/oracle_effect.qml', 'w').write(qml)
v = QQuickView()
v.setSource(QUrl.fromLocalFile('/tmp/oracle_effect.qml'))
v.show()
def grab():
    v.grabWindow().save(out); app.quit()
QTimer.singleShot(1000, grab)
app.exec()

