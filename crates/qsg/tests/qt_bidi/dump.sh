#!/bin/sh
# Writes qt.txt: what Qt 6.12 holds for the bidirectional algorithm (probe.cpp).
# Run in the testenv image: tools/ci/gate --run as-tester crates/qsg/tests/qt_bidi/dump.sh
set -eu
dir=$(cd "$(dirname "$0")" && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
g++ -O1 -fPIC -std=c++17 "$dir/probe.cpp" -o "$tmp/probe" $(pkg-config --cflags --libs Qt6Core)
"$tmp/probe" > "$tmp/qt.txt"
cp "$tmp/qt.txt" "$dir/qt.txt"
