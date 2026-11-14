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
for i, f in enumerate(lines[1:]):
    x, y, w, h, bmax, b, sat, path = f[:8]
    opts = dict(o.split('=', 1) for o in f[8:])
    items.append(f'Image {{ id: img{i}; x:{x}; y:{y}; width:{w}; height:{h}; visible:false; source:"file://{path}" }}')
    extra = f'autoPaddingEnabled: {"true" if opts.get("pad") == "1" else "false"}; '
    if 'bright' in opts: extra += f'brightness: {opts["bright"]}; '
    if 'contrast' in opts: extra += f'contrast: {opts["contrast"]}; '
    if 'mult' in opts: extra += f'blurMultiplier: {opts["mult"]}; '
    if 'colz' in opts:
        a_, c_ = opts['colz'].split(':')
        extra += f'colorization: {a_}; colorizationColor: {qcol(c_)}; '
    if 'mask' in opts:
        items.append(f'Image {{ id: m{i}; x:{x}; y:{y}; width:{w}; height:{h}; visible:false; source:"file://{opts["mask"]}" }}')
        extra += f'maskEnabled: true; maskSource: m{i}; maskThresholdMin: {opts.get("mmin", 0)}; maskSpreadAtMin: {opts.get("mspread", 0)}; maskInverted: {"true" if opts.get("minv") == "1" else "false"}; '
    items.append(f'MultiEffect {{ anchors.fill: img{i}; source: img{i}; blurEnabled: {"true" if float(bmax) > 0 else "false"}; blurMax: {bmax}; blur: {b}; saturation: {sat}; {extra}}}')
qml = f'import QtQuick\nimport QtQuick.Effects\nRectangle {{ width:{W}; height:{H}; color:"{bg}"\n' + "\n".join(items) + '\n}\n'
open('/tmp/oracle_effect.qml', 'w').write(qml)
v = QQuickView()
v.setSource(QUrl.fromLocalFile('/tmp/oracle_effect.qml'))
v.show()
def grab():
    v.grabWindow().save(out); app.quit()
QTimer.singleShot(1000, grab)
app.exec()

