# Compare QImage.scaled(SmoothTransformation) with impulse-shell scaletest, plus Image{sourceSize} implicit sizes.
import sys, subprocess, random
import numpy as np
from PIL import Image as P
from PySide6.QtGui import QGuiApplication, QImage
from PySide6.QtCore import Qt
app = QGuiApplication(sys.argv)
random.seed(3)
def mk(path, w, h, alpha):
    a = np.zeros((h, w, 4), np.uint8)
    yy, xx = np.mgrid[0:h, 0:w]
    a[..., 0] = (xx * 255 // max(1, w - 1)); a[..., 1] = (yy * 255 // max(1, h - 1))
    a[..., 2] = np.random.RandomState(w).randint(0, 256, (h, w)); a[..., 3] = np.random.RandomState(h).randint(0, 256, (h, w)) if alpha else 255
    P.fromarray(a, 'RGBA' if alpha else 'RGBA').convert('RGBA' if alpha else 'RGB').save(path)
mk('/tmp/st_a.png', 97, 61, True); mk('/tmp/st_rgb.png', 123, 77, False)
for src, alpha in [('/tmp/st_a.png', 1), ('/tmp/st_rgb.png', 0)]:
    q = QImage(src)
    for dw, dh in [(40, 25), (200, 150), (160, 30), (30, 140), (7, 5), (97, 61), (300, 61), (13, 200)]:
        s = q.scaled(dw, dh, Qt.IgnoreAspectRatio, Qt.SmoothTransformation).convertToFormat(QImage.Format_ARGB32_Premultiplied)
        b = np.frombuffer(s.constBits(), np.uint8).reshape(dh, s.bytesPerLine())[:, :dw * 4].reshape(dh, dw, 4)[..., [2, 1, 0, 3]].astype(int)
        subprocess.run([sys.argv[1], src, str(dw), str(dh), str(alpha), '/tmp/st.raw'], check=True)
        r = np.fromfile('/tmp/st.raw', np.uint8).reshape(dh, dw, 4).astype(int)
        d = np.abs(b - r)
        print(src[5:], q.format(), dw, dh, 'diff px', int((d.max(axis=2) > 0).sum()), 'max', int(d.max()))
