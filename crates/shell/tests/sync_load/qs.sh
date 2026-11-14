#!/bin/sh
# Prints the `report` of QsProbe.qml as Quickshell writes it: run in the fixture's directory.
export XDG_RUNTIME_DIR=$(mktemp -d) HOME=$(mktemp -d) QT_QPA_PLATFORM=offscreen LANG=C.UTF-8
d=$(pwd)
timeout 60 quickshell -p "$d" 2>&1 | grep -o "CASE .*" | sed 's/^CASE //; s/\x1b\[[0-9;]*m//g' | sed "s|file://$d/|DIR/|g; s|$d/|DIR/|g; s|qs:@/qs/|DIR/|g"
