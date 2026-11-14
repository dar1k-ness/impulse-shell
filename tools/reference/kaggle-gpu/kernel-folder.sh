#!/bin/sh
# Writes the Kaggle kernel folder (kernel-metadata.json + run.py) to OUT_DIR, with the kernel and
# dataset ids under --user NAME, else $KAGGLE_USERNAME.
#   tools/reference/kaggle-gpu/kernel-folder.sh OUT_DIR --user NAME && kaggle kernels push -p OUT_DIR
set -eu
out=${1:?usage: kernel-folder.sh OUT_DIR [--user NAME]}
k=$(cd "$(dirname "$0")" && pwd)
user=$("$k/kaggle-user" "$@")
mkdir -p "$out"
sed "s|KAGGLE_USER/|$user/|g" "$k/kernel/kernel-metadata.json" >"$out/kernel-metadata.json"
cp "$k/kernel/run.py" "$out/"
