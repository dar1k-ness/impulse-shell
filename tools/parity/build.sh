#!/bin/bash
# build.sh: build native rusty-impulse in the guest from the checkout `bench/vm.sh sync` copied to ~/rusty-impulse,
# the way tools/install.sh builds what gets installed (the checkout's `qml` as RI_ROOT, no interpreter),
# and record next to the binary what it was built from: `REVISION SHA256` in rusty-impulse.rev. capture.sh
# refuses a native binary without that record, with another checksum or from another revision.
# The revision is ~/rusty-impulse/.commit (written by bench/vm.sh), or git's when the checkout is a clone.
set -euo pipefail
PARITY=$(cd "$(dirname "$0")" && pwd)
. "$PARITY/lib.sh"
SRC=${SRC:-$HOME/rusty-impulse}
cd "$SRC"
if [ -d .git ]; then rev=$(git describe --always --dirty --abbrev=12); else rev=$(cat .commit); fi
[ -n "$rev" ] || die "no revision for $SRC (.commit or .git)"
# setup.sh pins the clock to 2026-01-15: cargo then cannot download crates (TLS) and outputs get
# dates older than their sources. Build on the real time; the next run.sh pins it again.
real_clock
config=${XDG_CONFIG_HOME:-$HOME/.config}/rustshell/rusty-impulse
export RI_ROOT=${RI_ROOT:-$SRC/qml}
[ -f "$RI_ROOT/shell.qml" ] || die "no QML tree at $RI_ROOT: bench/vm.sh sync"
# tools/build.sh, as tools/install.sh: the binary names its commit (a copy without .git: the one
# in .build-commit, written by bench/vm.sh) and the checksum of its QML tree, and none of the
# guest's directories.
bin=$(tools/build.sh) || die "the build made no rusty-impulse"
"$bin" --check-tree -p "$config" >/dev/null || die "$bin does not run the rusty-impulse config at $config"
echo "$rev $(sha256sum < "$bin" | cut -d' ' -f1)" > "$bin.rev"
echo "built $rev: $bin"
