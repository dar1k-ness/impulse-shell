#!/bin/bash
# tools/build.sh: builds the shipped impulse-shell (release, no JavaScript interpreter) from
# this checkout and prints the path of the binary; tools/install.sh installs what it builds.
#
#   tools/build.sh            build; the path of the binary on stdout
#   tools/build.sh --sum DIR  print the checksum of the QML tree DIR, as the build computes it
#   tools/build.sh --commit   print the commit the build would name
#
# One commit and one QML tree give one file, byte for byte, wherever the checkout, Cargo's
# home and the target directory are: the binary names its commit and the checksum of its QML
# tree (`impulse-shell --version`) and none of this machine's directories. A build that cannot
# tell its commit, or whose binary names such a directory, fails. What the script does not
# fix, and what therefore still gives another file: another Rust toolchain than
# rust-toolchain.toml's, another C compiler or linker, another version of a system library a
# crate reads at build time, other rustc flags (RUSTFLAGS, Cargo's configuration), another
# target.
#
# Environment:
#   RI_ROOT    the QML tree to compile (default: the checkout's qml)
#   RI_COMMIT  the commit of a copy of the sources without .git, which cannot tell it itself:
#              what `tools/build.sh --commit` prints in the checkout the copy was made from.
#              Without the variable the copy's file .build-commit is read (bench/vm.sh writes
#              it). A git checkout tells its own, and RI_COMMIT naming another one is an error.
set -euo pipefail
die() {
    echo "build.sh: $*" >&2
    exit 1
}
# A checksum of what the tree `$1` holds: the path and the bytes of every file, read through
# links, whatever their times (not .git, which the build does not read either).
# crates/ricompiled-gen/build.rs computes the same from the files it compiles.
tree_sum() {
    (
        cd "$1" &&
            find -L . -name .git -prune -o -type f -print0 | LC_ALL=C sort -z | xargs -0 -r sha256sum
    ) | sha256sum | cut -d' ' -f1
}
repo=$(cd "$(dirname "$0")/.." && pwd)
# The commit `impulse-shell --version` names: this checkout's, "+changes" when a file differs
# from it or is not in it (ignored files apart). A directory that is not the top of a git
# checkout (a copy without .git, also one inside another repository) cannot tell: the caller
# names the commit the copy was made from, or the copy's .build-commit does.
commit() {
    local given=${RI_COMMIT:-} top changes
    if top=$(git -C "$repo" rev-parse --show-prefix 2>/dev/null) && [ -z "$top" ] &&
        RI_COMMIT=$(git -C "$repo" rev-parse --verify HEAD 2>/dev/null); then
        changes=$(git -C "$repo" status --porcelain) || die "git status failed in $repo"
        [ -z "$changes" ] || RI_COMMIT+=+changes
        [ -z "$given" ] || [ "$given" = "$RI_COMMIT" ] ||
            die "RI_COMMIT is $given, and this checkout is at $RI_COMMIT: RI_COMMIT is for a copy of the sources without .git"
    elif [ -n "$given" ]; then
        RI_COMMIT=$given
    elif [ -s "$repo/.build-commit" ]; then
        RI_COMMIT=$(head -n 1 "$repo/.build-commit")
    else
        die "cannot tell the commit of $repo, which is not a git checkout: name the commit this copy was made from (what tools/build.sh --commit prints there), RI_COMMIT=<commit> tools/build.sh; nothing was built"
    fi
    case $RI_COMMIT in
    *[!0-9A-Za-z+._-]* | "") die "RI_COMMIT is '$RI_COMMIT': a commit is letters, digits and + . _ -" ;;
    esac
    export RI_COMMIT
}
case ${1:-} in
--sum)
    [ $# -eq 2 ] || die "--sum takes one directory"
    [ -d "$2" ] || die "no directory $2"
    tree_sum "$2"
    exit
    ;;
--commit)
    [ $# -eq 1 ] || die "--commit takes no argument"
    commit
    echo "$RI_COMMIT"
    exit
    ;;
-h | --help)
    sed -n '2,/^set -euo/{/^set -euo/!s/^# \{0,1\}//p}' "$0"
    exit 0
    ;;
"") ;;
*) die "unknown argument '$1' (tools/build.sh --help)" ;;
esac
need() { command -v "$1" >/dev/null || die "$1 is needed $2"; }
need cargo "to build"
need jq "to read where cargo put the binary"
RI_ROOT=${RI_ROOT:-$repo/qml}
RI_ROOT=$(cd "$RI_ROOT" 2>/dev/null && pwd) || die "no directory RI_ROOT=$RI_ROOT"
export RI_ROOT
# Cargo compiles the tree again when this value differs from the one of the last build, and
# only then: a tree whose files carry older times (one a rollback or a backup put back) is
# compiled as well, and new file times or a new .git alone do not compile it again.
RI_ROOT_SUM=$(tree_sum "$RI_ROOT") || die "cannot read the QML tree at $RI_ROOT"
export RI_ROOT_SUM
commit
# Source paths the binary keeps (panic locations) name the crate, not this machine's
# directories: the checkout becomes `.`, Cargo's home `cargo`, the target directory (where
# the Rust compiled from the QML tree is written) `target`. Of several prefixes that match a
# path rustc takes the last one given. Cargo leaves flags that hold --remap-path-prefix out of
# the names of what it builds, so those names do not tell the directories either.
cargo_home=${CARGO_HOME:-$HOME/.cargo}
target_dir=$(cd "$repo" && cargo metadata --locked --format-version 1 --no-deps | jq -r .target_directory)
[ -n "$target_dir" ] || die "cargo metadata does not tell the target directory"
remap=()
dirs=()
# `$1` as it is named and as it is once its links are followed: Cargo passes either.
hide() {
    local dir real
    for dir in "$1" "$(cd "$1" 2>/dev/null && pwd -P)"; do
        [ -n "$dir" ] || continue
        case " ${remap[*]-} " in *" --remap-path-prefix=$dir=$2 "*) continue ;; esac
        remap+=("--remap-path-prefix=$dir=$2")
        dirs+=("$dir")
    done
}
hide "$repo" .
hide "$cargo_home" cargo
hide "$target_dir" target
# Cargo takes its extra rustc flags from the first of four sources that is set (Cargo Book,
# Configuration, build.rustflags): CARGO_ENCODED_RUSTFLAGS, RUSTFLAGS, the
# target.<triple or cfg>.rustflags entries, build.rustflags. The remap flags are added to the
# variable when one is set, and to build.rustflags otherwise (--config joins them to the
# user's).
flags=()
if [ -n "${CARGO_ENCODED_RUSTFLAGS+set}" ]; then
    [ -z "$CARGO_ENCODED_RUSTFLAGS" ] || IFS=$'\x1f' read -ra flags <<<"$CARGO_ENCODED_RUSTFLAGS"
elif [ -n "${RUSTFLAGS+set}" ]; then
    read -ra flags <<<"$RUSTFLAGS"
fi
config=()
if [ -n "${CARGO_ENCODED_RUSTFLAGS+set}${RUSTFLAGS+set}" ]; then
    flags+=("${remap[@]}")
    CARGO_ENCODED_RUSTFLAGS=$(IFS=$'\x1f' && echo "${flags[*]}")
    export CARGO_ENCODED_RUSTFLAGS
else
    config=(--config "build.rustflags=$(jq -cn '$ARGS.positional' --args -- "${remap[@]}")")
fi
bin=$(cd "$repo" && cargo build --locked --release -p shell "${config[@]}" \
    --message-format=json-render-diagnostics |
    jq -r 'select(.reason == "compiler-artifact" and .target.name == "impulse-shell" and .executable != null) | .executable')
[ -x "$bin" ] || die "the build made no impulse-shell"
# The binary tells whether it is what was asked for. A target.<triple or cfg>.rustflags entry
# of Cargo's configuration replaces build.rustflags, and the script cannot read it; code that
# asks for its own directory at compile time is not covered by the flags at all. A directory
# is named where its path starts a path: `/src/registry/` after `tracing-subscriber-0.3.23`
# is a directory of that crate, not the checkout `/src`.
for dir in "${dirs[@]}"; do
    at=$(printf %s "$dir" | sed 's/[][\.*^$()+?{}|]/\\&/g')
    ! LC_ALL=C grep -aqE "(^|[^[:alnum:]_./~+-])$at/" "$bin" ||
        die "$bin names this machine's directory $dir (see: grep -aoE '$at/[[:alnum:]_./-]*' $bin), so a build in another directory gives another file. If Cargo's configuration has a target.<triple>.rustflags, it replaces the build.rustflags that hide the path: put those flags in build.rustflags or RUSTFLAGS"
done
version=$("$bin" --version) || die "$bin does not answer --version"
case $version in
*"(commit $RI_COMMIT)"*"QML tree: sum $RI_ROOT_SUM"*) ;;
*) die "$bin is not the build of commit $RI_COMMIT and the QML tree with the sum $RI_ROOT_SUM; it says: $(tr '\n' ' ' <<<"$version")" ;;
esac
echo "build.sh: commit $RI_COMMIT, QML tree sum $RI_ROOT_SUM, sha256 $(sha256sum <"$bin" | cut -d' ' -f1)" >&2
echo "$bin"
