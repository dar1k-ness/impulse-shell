#!/bin/sh
# The llvmpipe half of the Kaggle comparison, locally in tools/testenv: the bar and settings
# through a theme switch on Mesa's surfaceless platform and software EGL device, which must
# match each other exactly. Frames go to OUT_DIR (default /tmp/gpushot).
#   docker run --rm -v "$PWD:/src" ri-testenv tools/reference/kaggle-gpu/local-test.sh
set -eu
out=${1:-/tmp/gpushot}
k=$(dirname "$0")
cargo build --release -p shell --features tools --bin gpushot
bin=$(cargo metadata --format-version 1 --no-deps | jq -r .target_directory)/release/gpushot
st=0
for scene in bar settings; do
    "$bin" qml "$out/$scene" --scene "$scene" --egl surfaceless --egl software \
        --theme "$k/palettes/dark.json" "$k/palettes/light.json" --frames 40 --tol 0 || st=1
done
exit $st
