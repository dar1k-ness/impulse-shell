#!/bin/sh
# dump.sh SCENE.qml...: writes SCENE.qt for each scene. Run in the testenv image, in this directory.
set -eu
g++ -O1 -fPIC -o /tmp/qt-frames-dump dump.cpp $(pkg-config --cflags --libs Qt6Quick Qt6Gui Qt6Qml Qt6Core)
for s in "$@"; do
    /tmp/qt-frames-dump "$s" > "${s%.qml}.qt"
done
