#!/bin/sh
# Writes qt.tsv: what Qt 6.12 and its ICU hold for the code points of points.txt.
# Run in the testenv image: tools/ci/gate --run as-tester crates/qml/tests/qt_unicode/dump.sh
set -eu
dir=$(cd "$(dirname "$0")" && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
g++ -O1 -fPIC -std=c++17 "$dir/probe.cpp" -o "$tmp/probe" $(pkg-config --cflags --libs Qt6Core icu-uc)
"$tmp/probe" "$dir/points.txt" > "$tmp/qt.tsv"
cp "$tmp/qt.tsv" "$dir/qt.tsv"
