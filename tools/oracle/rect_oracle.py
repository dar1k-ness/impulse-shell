# Renders the same rect scene file with real Qt Quick (PySide6) and grabs the window.
import sys
from PySide6.QtGui import QGuiApplication
from PySide6.QtQuick import QQuickView
from PySide6.QtCore import QUrl, QTimer
src, out = sys.argv[1], sys.argv[2]
lines = [l.split() for l in open(src) if l.strip()]
W, H, bg = lines[0]
items = []
stack = [(0.0, 0.0)]
for f in lines[1:]:
    if f[0] == 'clip':
        px, py = stack[-1]; cx, cy = float(f[1]), float(f[2])
        items.append(f'Item {{ x:{cx-px}; y:{cy-py}; width:{f[3]}; height:{f[4]}; clip:true')
        stack.append((cx, cy)); continue
    if f[0] == 'endclip':
        stack.pop(); items.append('}'); continue
    ox, oy = stack[-1]
    f = [str(float(f[0]) - ox), str(float(f[1]) - oy)] + f[2:]
    x, y, w, h, r, pen, col, bcol, aa = f[:9]
    grad = ''
    if len(f) > 10:
        ori = 'Gradient.Horizontal' if f[9] == 'h' else 'Gradient.Vertical'
        st = ' '.join(f'GradientStop {{ position:{t}; color:"{c}" }}' for t, c in (s.split(':') for s in f[10:]))
        grad = f'gradient: Gradient {{ orientation:{ori}; {st} }} '
    items.append(f'Rectangle {{ x:{x}; y:{y}; width:{w}; height:{h}; radius:{r}; color:"{col}"; {grad}'
                 f'border.width:{pen}; border.color:"{bcol}"; antialiasing:{"true" if aa=="1" else "false"} }}')
qml = f'import QtQuick\nRectangle {{ width:{W}; height:{H}; color:"{bg}"\n' + "\n".join(items) + '\n}\n'
open('/tmp/oracle_scene.qml', 'w').write(qml)
app = QGuiApplication(sys.argv)
v = QQuickView()
v.setResizeMode(QQuickView.SizeRootObjectToView)
v.setSource(QUrl.fromLocalFile('/tmp/oracle_scene.qml'))
v.resize(int(W), int(H))
v.show()
def grab():
    print("DEVSIZE", v.width(), v.height(), flush=True)
    v.grabWindow().copy(0, 0, int(W), int(H)).save(out)
    app.quit()
QTimer.singleShot(700, grab)
app.exec()
