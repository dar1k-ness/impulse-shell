# Print Qt's loaded pixmap size for Image{sourceSize} combos: "fill sw sh -> implicitW implicitH"
import sys
from PySide6.QtGui import QGuiApplication
from PySide6.QtQml import QQmlComponent, QQmlEngine
from PySide6.QtCore import QUrl, QByteArray
app = QGuiApplication(sys.argv)
eng = QQmlEngine()
for src in ['/tmp/st_a.png', '/usr/share/icons/hicolor/256x256/apps/kitty.png']:
    for fill in ['Stretch', 'PreserveAspectFit', 'PreserveAspectCrop']:
        for sw, sh in [(40, 0), (0, 30), (40, 30), (30, 40), (200, 150), (500, 0), (64, 64), (50, 100)]:
            c = QQmlComponent(eng)
            c.setData(QByteArray(f'import QtQuick\nImage {{ asynchronous:false; fillMode:Image.{fill}; sourceSize.width:{sw}; sourceSize.height:{sh}; source:"file://{src}" }}'.encode()), QUrl())
            o = c.create()
            print(src.split('/')[-1], fill, sw, sh, '->', o.property('implicitWidth'), o.property('implicitHeight'))
