# Scene: "W H bg" then canvases: "canvas x y w h" followed by ops until next canvas:
#   M x y | L x y | C c1x c1y c2x c2y x y | A xc yc r sa ea acw | Z | fill #argb | stroke #argb width cap join
import sys
from PySide6.QtGui import QGuiApplication
from PySide6.QtQuick import QQuickView
from PySide6.QtCore import QUrl, QTimer
src, out = sys.argv[1], sys.argv[2]
lines = [l.split() for l in open(src) if l.strip() and not l.startswith('#')]
W, H, bg = lines[0]
def qcol(c):
    c = c.lstrip('#')
    if len(c) == 8:
        a, r, g, b = (int(c[i:i+2], 16) for i in (0, 2, 4, 6))
        return f'"rgba({r},{g},{b},{a/255:.6f})"'
    return f'"#{c}"'
canv = []
for l in lines[1:]:
    if l[0] == 'canvas':
        canv.append((l[1:5], [])); continue
    ops = canv[-1][1]
    k = l[0]
    if k == 'M': ops.append(f'ctx.moveTo({l[1]},{l[2]});')
    elif k == 'L': ops.append(f'ctx.lineTo({l[1]},{l[2]});')
    elif k == 'C': ops.append(f'ctx.bezierCurveTo({",".join(l[1:7])});')
    elif k == 'A': ops.append(f'ctx.arc({l[1]},{l[2]},{l[3]},{l[4]},{l[5]},{"true" if l[6]=="1" else "false"});')
    elif k == 'Z': ops.append('ctx.closePath();')
    elif k == 'fill': ops.append(f'ctx.fillStyle={qcol(l[1])}; ctx.fill(); ctx.beginPath();')
    elif k == 'dash': ops.append(f'ctx.setLineDash([{",".join(l[2:])}]); ctx.lineDashOffset={l[1]};')
    elif k == 'srect': ops.append(f'ctx.strokeStyle={qcol(l[1])}; ctx.lineWidth={l[2]}; ctx.lineCap="butt"; ctx.strokeRect({",".join(l[3:7])});')
    elif k == 'stroke': ops.append(f'ctx.strokeStyle={qcol(l[1])}; ctx.lineWidth={l[2]}; ctx.lineCap="{l[3]}"; ctx.lineJoin="{l[4]}"; ctx.stroke(); ctx.beginPath();')
items = []
for (x, y, w, h), ops in canv:
    items.append(f'Canvas {{ x:{x}; y:{y}; width:{w}; height:{h}; onPaint: {{ var ctx = getContext("2d"); ctx.reset(); ctx.beginPath(); {" ".join(ops)} }} }}')
qml = f'import QtQuick\nRectangle {{ width:{W}; height:{H}; color:"{bg}"\n' + "\n".join(items) + '\n}\n'
open('/tmp/oracle_canvas.qml', 'w').write(qml)
app0 = QGuiApplication(sys.argv)
v = QQuickView()
v.setSource(QUrl.fromLocalFile('/tmp/oracle_canvas.qml'))
v.show()
def grab():
    v.grabWindow().copy(0, 0, int(W), int(H)).save(out); app.quit()
app = QGuiApplication.instance()
QTimer.singleShot(1000, grab)
app.exec()
