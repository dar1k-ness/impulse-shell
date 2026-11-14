# svg_oracle.py OUT LIST: renders each "W H PATH" line of LIST with QImageReader (QtSvg's image
# plugin, what Qt Quick's Image and QIcon load an SVG with) scaled to WxH, and appends its
# premultiplied RGBA bytes to OUT. Run with QT_QPA_PLATFORM=offscreen.
import sys
from PySide6.QtCore import QSize
from PySide6.QtGui import QGuiApplication, QImage, QImageReader

app = QGuiApplication(sys.argv)
with open(sys.argv[1], 'wb') as out:
    for line in open(sys.argv[2]):
        w, h, path = line.rstrip('\n').split(' ', 2)
        w, h = int(w), int(h)
        reader = QImageReader(path)
        reader.setScaledSize(QSize(w, h))
        img = reader.read().convertToFormat(QImage.Format.Format_RGBA8888_Premultiplied)
        if img.isNull() or (img.width(), img.height()) != (w, h):
            sys.exit(f'{path}: {reader.errorString()}')
        out.write(bytes(img.constBits())[:w * h * 4])
