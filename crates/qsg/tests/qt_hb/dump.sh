#!/bin/sh
# Writes qt.txt: the Unicode data Qt 6.12 gives HarfBuzz (probe.cpp).
# Run in the testenv image: tools/ci/gate --run as-tester crates/qsg/tests/qt_hb/dump.sh
set -eu
dir=$(cd "$(dirname "$0")" && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
g++ -O1 -fPIC -std=c++17 -rdynamic "$dir/probe.cpp" -o "$tmp/probe" \
    $(pkg-config --cflags --libs Qt6Gui harfbuzz) -ldl
"$tmp/probe" > "$tmp/qt.txt"
cp "$tmp/qt.txt" "$dir/qt.txt"
