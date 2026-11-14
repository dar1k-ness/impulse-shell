#!/bin/sh
# Regenerate crates/qml/src/cava_ref.txt from cava 1.0.0's cavacore.c (needs fftw3, git).
set -e
here=$(cd "$(dirname "$0")" && pwd)
tmp=$(mktemp -d)
git clone -q --depth 1 --branch 1.0.0 https://github.com/karlstav/cava.git "$tmp/cava"
cc -O2 -I"$tmp/cava" -I"$here" "$here/harness.c" "$tmp/cava/cavacore.c" -lfftw3 -lm -o "$tmp/harness"
"$tmp/harness" > "$here/../../../crates/qml/src/cava_ref.txt"
rm -rf "$tmp"
