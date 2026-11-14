# Scene: "W H bg" then "x y w h fillMode(stretch|fit|crop) smooth(0|1) path"
import sys
from PySide6.QtGui import QGuiApplication
from PySide6.QtQuick import QQuickView
from PySide6.QtCore import QUrl, QTimer
src, out = sys.argv[1], sys.argv[2]
lines = [l.split() for l in open(src) if l.strip() and not l.startswith('#')]
W, H, bg = lines[0]
app = QGuiApplication(sys.argv)
fm = {'stretch': 'Image.Stretch', 'fit': 'Image.PreserveAspectFit', 'crop': 'Image.PreserveAspectCrop'}
def ss(r):
    if len(r) < 8: return ''
    a, b = r[7].split(':'); return f'sourceSize.width:{a}; sourceSize.height:{b}; '
items = [f'Image {{ x:{r[0]}; y:{r[1]}; width:{r[2]}; height:{r[3]}; fillMode:{fm[r[4]]}; smooth:{"true" if r[5]=="1" else "false"}; {ss(r)}source:"file://{r[6]}" }}'
         for r in lines[1:]]
qml = f'import QtQuick\nRectangle {{ width:{W}; height:{H}; color:"{bg}"\n' + "\n".join(items) + '\n}\n'
open('/tmp/oracle_image.qml', 'w').write(qml)
v = QQuickView()
v.setSource(QUrl.fromLocalFile('/tmp/oracle_image.qml'))
v.show()
def grab():
    v.grabWindow().copy(0, 0, int(W), int(H)).save(out); app.quit()
QTimer.singleShot(1000, grab)
app.exec()
