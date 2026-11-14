#!/bin/sh
# Runs kernel/run.py up to bundle discovery against a fake /kaggle/input holding two stub
# bundles (tar.xz form, as the dataset ships them, then unpacked form), without a GPU:
# catches a run.py that cannot even start on Kaggle.
#   tools/reference/kaggle-gpu/check-kernel.sh
set -eu
k=$(cd "$(dirname "$0")" && pwd)
t=$(mktemp -d)
trap 'rm -rf "$t"' EXIT
stub() {
    d=$t/stub/$1/ri-gpushot
    mkdir -p "$d/bin" "$d/lib" "$d/egl_vendor.d"
    : >"$d/run-gpushot"; : >"$d/bin/gpushot"; : >"$d/lib/ld-linux-x86-64.so.2"
    echo '{}' >"$d/egl_vendor.d/50_mesa.json"
    echo "rev-$1" >"$d/REVISION"
}
stub base; stub new
mkdir -p "$t/input/ds"
for v in base new; do tar -C "$t/stub/$v" -cJf "$t/input/ds/ri-gpushot-$v.tar.xz" ri-gpushot; done
run() {
    KAGGLE_INPUT=$t/input KAGGLE_TMP=$t/tmp KAGGLE_WORKING=$t/work python3 "$k/kernel/run.py" --discover
}
out=$(run)
echo "$out"
echo "$out" | grep -q '^bundle base .* rev-base$' || { echo "check-kernel: base bundle not found" >&2; exit 1; }
echo "$out" | grep -q '^bundle new .* rev-new$' || { echo "check-kernel: new bundle not found" >&2; exit 1; }
test -f "$t/tmp/egl_vendor.d/50_mesa.json" -a -f "$t/tmp/egl_vendor.d/10_nvidia.json" ||
    { echo "check-kernel: EGL vendor dir not written" >&2; exit 1; }
rm -rf "$t/input" "$t/tmp"; mkdir -p "$t/input"
cp -r "$t/stub/new" "$t/input/ds"
out=$(run)
echo "$out"
echo "$out" | grep -q '^bundle ds .* rev-new$' || { echo "check-kernel: unpacked bundle not found" >&2; exit 1; }
echo "check-kernel: ok"
