#!/bin/sh
# Builds gpushot in tools/testenv and packs it with every shared library it loads (glibc and
# its loader included), Mesa's EGL/llvmpipe, the fonts and the QML tree into a directory that
# runs on any x86-64 Linux: Kaggle's Ubuntu image gets it as a dataset, since the repo is
# private and the shell does not build there. OUT_DIR becomes that dataset's folder:
# dataset-metadata.json and ri-gpushot.tar.xz (the unpacked tree stays in OUT_DIR/ri-gpushot).
# The dataset id's owner is --user NAME, else $KAGGLE_USERNAME (the token's username).
#   docker run --rm -v "$PWD:/src" ri-testenv tools/reference/kaggle-gpu/bundle.sh OUT_DIR --user NAME
#   kaggle datasets create -p OUT_DIR   # or: kaggle datasets version -p OUT_DIR -m "..."
set -eu
out=${1:?usage: bundle.sh OUT_DIR [--user NAME]}
src=$(cd "$(dirname "$0")/../../.." && pwd)
user=$("$src/tools/reference/kaggle-gpu/kaggle-user" "$@")
cargo build --release --manifest-path "$src/Cargo.toml" -p shell --features tools --bin gpushot
bin=$(cargo metadata --manifest-path "$src/Cargo.toml" --format-version 1 --no-deps |
    jq -r .target_directory)/release/gpushot
b=$out/ri-gpushot
rm -rf "$b"
mkdir -p "$b/bin" "$b/lib/dri" "$b/egl_vendor.d" "$b/fonts"
cp "$bin" "$b/bin/"
# glvnd's libEGL (and GLES for its entry points), libEGL_mesa and the DRI drivers are all
# dlopened, so ldd of the binary alone misses them.
glvnd="/usr/lib/libEGL.so.1 /usr/lib/libGLESv2.so.2 /usr/lib/libGLdispatch.so.0"
mesa=$(ls /usr/lib/libEGL_mesa.so.0)
dri=$(ls /usr/lib/dri/swrast_dri.so /usr/lib/dri/kms_swrast_dri.so 2>/dev/null || true)
for f in "$bin" $glvnd "$mesa" $dri; do
    ldd "$f" | awk '/=> \//{print $3} /^\t\/.*ld-linux/{print $1}'
done | sort -u | while read -r l; do
    cp -L "$l" "$b/lib/$(basename "$l")"
done
for l in $glvnd "$mesa"; do cp -L "$l" "$b/lib/"; done
for d in $dri; do cp -L "$d" "$b/lib/dri/"; done
cp /usr/share/glvnd/egl_vendor.d/50_mesa.json "$b/egl_vendor.d/"
cp -rL /usr/share/fonts "$b/fonts/files"
# the original's icon font; Arch's testenv has none.
curl -fsSL -o "$b/fonts/files/MaterialSymbolsRounded.ttf" \
    "https://github.com/google/material-design-icons/raw/master/variablefont/MaterialSymbolsRounded%5BFILL%2CGRAD%2Copsz%2Cwght%5D.ttf" ||
    echo "bundle: no Material Symbols Rounded (icons render as boxes)" >&2
mkdir -p "$b/fonts/conf.d"
cp -L /etc/fonts/conf.d/*.conf "$b/fonts/conf.d/"
cp -r "$src/qml" "$b/tree"
cp "$src"/tools/reference/kaggle-gpu/palettes/*.json "$b/"
cp "$src/tools/reference/kaggle-gpu/run-gpushot" "$b/"
git -C "$src" rev-parse HEAD >"$b/REVISION" 2>/dev/null || true
du -sh "$b"
tar -C "$out" -cf - ri-gpushot | xz -9 -T0 >"$out/ri-gpushot.tar.xz"
sed "s|KAGGLE_USER/|$user/|" "$src/tools/reference/kaggle-gpu/dataset-metadata.json" >"$out/dataset-metadata.json"
ls -la "$out"
