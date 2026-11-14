#!/bin/bash
# orig.sh: host side. Unpacks the original in the guest as ~/parity-original (lib.sh, ORIG_TREE) and
# writes what it is to ~/parity-original/.rev. run.sh and `bench/vm.sh sync` both call it, so the view
# comparison and the benchmarks run the same original.
# Exit code: 2 this clone does not have the original.
# Env: SSH (guest ssh command, default `bench/vm.sh ssh`),
#      ORIG (REVISION:PATH of the original in git history).
set -eu
REPO=$(cd "$(dirname "$0")/../.." && pwd)
SSH=${SSH:-$REPO/bench/vm.sh ssh}
# The original is not in the checkout: `qml` is the project's own tree since #53 and imports the
# Impulse module, which Quickshell does not have. It is `ref/ii` of the last commit of `main` before
# #53, taken from git history.
ORIG=${ORIG:-c33d2c5f570e12061c263b4df8e92a4796aa2150:ref/ii}
rev=${ORIG%%:*} dir=${ORIG#*:}
git -C "$REPO" cat-file -e "$rev:$dir" 2>/dev/null ||
  { echo "orig.sh: this clone has no $ORIG (the original): git fetch --unshallow" >&2; exit 2; }
strip=$(awk -F/ '{ print NF }' <<< "$dir")
git -C "$REPO" archive "$rev" "$dir" |
  $SSH "rm -rf ~/parity-original && mkdir -p ~/parity-original && tar -x -m --strip-components=$strip -C ~/parity-original && echo $ORIG > ~/parity-original/.rev"
