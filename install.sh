#!/bin/bash
# install.sh: the whole install by one command, on Arch Linux with end-4's dots-hyprland in
# place. It ends with rusty-impulse running in the Hyprland session it was started from.
#
#   ./install.sh              from a checkout: install that checkout
#   bash install.sh           from anywhere else (the file alone): clone the repository to
#                             $RI_SRC, or update the clone that is there, and install that
#   ./install.sh --no-start   everything but the last step: the shell that runs keeps running
#   ./install.sh --no-packages   install no packages (they are there, or this is not Arch)
#
# What it does, in this order; each step says what it does before it does it:
#   1. packages: what the build and the shell need and pacman does not have yet (sudo pacman -S).
#      The Rust toolchain comes from rustup (rust-toolchain.toml names the version); a `rust`
#      package that is installed instead is replaced by `rustup`, which provides it.
#   2. the sources: this checkout, or a clone (see above).
#   3. build and install: PREFIX=~/.local tools/install.sh (docs/install.md).
#   4. the user's data and files: tools/carry-over.sh, then tools/carry-over.sh --dots new. The
#      settings, state and caches are copied to the new names, and the Hyprland files call and
#      start rusty-impulse where they called and started Quickshell. Nothing old is removed.
#   5. the switch, when this runs inside a Hyprland session: Hyprland reloads its config, the
#      shell that runs is stopped and rusty-impulse is started. If it does not show its panels
#      in $RI_START_WAIT seconds, the files of step 4 are put back and the shell that ran is
#      started again: the session is not left without a shell. Outside a session nothing is
#      started: the next login starts it.
#
# Back to the original afterwards: tools/carry-over.sh --dots old, then log in again;
# tools/install.sh --uninstall removes the build.
#
# Environment:
#   RI_REPO        the repository to clone (default https://github.com/dar1k-ness/rusty-impulse.git)
#   RI_SRC         where the clone goes (default ~/.local/src/rusty-impulse)
#   RI_START_WAIT  seconds to wait for the panels in step 5 (default 30)
#
# Exit status: 0 the shell is installed (and runs, if step 5 was made), 1 a step failed and
# says which; 2 the shell did not come up and what ran before is back.
set -euo pipefail

main() {
    local start=1 packages=1 arg
    for arg in "$@"; do
        case $arg in
        --no-start) start= ;;
        --no-packages) packages= ;;
        -h | --help)
            usage
            return 0
            ;;
        *) die "unknown argument '$arg' (--help)" ;;
        esac
    done
    [ "$(id -u)" != 0 ] || die "run it as the user whose shell it is, not as root (it uses sudo itself for pacman)"
    [[ ${HOME-} == /* ]] || die "HOME is not set"
    [[ ${RI_START_WAIT:-30} =~ ^[1-9][0-9]*$ ]] || die "RI_START_WAIT is a number of seconds, not '$RI_START_WAIT'; nothing was done"

    [ -z "$packages" ] || install_packages
    sources "$@"
    [ ! -d "$HOME/.cargo/bin" ] || PATH=$HOME/.cargo/bin:$PATH

    step "building and installing to ~/.local (some minutes the first time)"
    before=$(readlink -f -- "$bin" 2>/dev/null || true)
    PREFIX=$HOME/.local "$src/tools/install.sh" || die "the build or the install failed; nothing else was changed"
    default_toolchain

    step "carrying the settings over"
    local rc=0
    "$src/tools/carry-over.sh" --quiet || rc=$?
    # 3 and 4: something stays uncopied (a locked keyring, a file in the way) and the script
    # has said what; the shell runs without it.
    case $rc in 0 | 3 | 4) ;; *) die "tools/carry-over.sh failed; the build is installed, the Hyprland files are untouched" ;; esac
    step "making the Hyprland files call and start $bin"
    "$src/tools/carry-over.sh" --dots new --quiet || die "tools/carry-over.sh --dots new failed; put the files back with: $src/tools/carry-over.sh --dots old"
    grep -rqsE "(^|[^[:alnum:]_-])$name +-c " "$config/hypr" ||
        echo "note: nothing under $config/hypr starts the shell: make Hyprland run \`$bin -c $name\` at login"

    if [ -z "$start" ]; then
        echo "installed. Not started (--no-start): the next login starts it."
    elif in_session; then
        switch
    else
        echo "installed. This is not a Hyprland session, so nothing was started: log in to Hyprland and the shell is there."
    fi
}

name=rusty-impulse
word=rustshell
bin=$HOME/.local/bin/$name
config=${XDG_CONFIG_HOME-}
[[ $config == /* ]] || config=$HOME/.config
src=
before=

die() {
    echo "install.sh: $*" >&2
    exit 1
}
step() { echo "== $*"; }
usage() { awk 'NR > 1 && /^#/ { sub(/^# ?/, ""); print; next } NR > 1 { exit }' "${BASH_SOURCE[0]}"; }

# What the build needs (docs/build.md) and what the shell loads when it runs: libheif for HEIF
# wallpapers, libavif for the colours of AVIF ones, libthai for Thai line breaking. The
# programs the shell calls (matugen, cliphist, ...) are the ones end-4's dots installed.
PACKAGES=(base-devel rustup clang cmake nasm pkgconf git jq
    wayland wayland-protocols libxkbcommon mesa libglvnd libdrm libinput
    fontconfig freetype2 harfbuzz libjpeg-turbo pipewire pam file
    libheif libavif libthai)

install_packages() {
    if ! command -v pacman >/dev/null; then
        echo "note: no pacman here, so no packages are installed: docs/build.md lists what the build needs"
        return 0
    fi
    local missing
    # pacman -T prints what is not satisfied; base-devel is a package that depends on the tools.
    mapfile -t missing < <(pacman -T "${PACKAGES[@]}" || true)
    # rustup provides rust, so -T finds it satisfied by either; the build needs rustup itself.
    if ! command -v rustup >/dev/null && [ ! -x "$HOME/.cargo/bin/rustup" ]; then
        if pacman -Qq rust >/dev/null 2>&1 && ! pacman -Qq rustup >/dev/null 2>&1; then
            step "replacing the package rust by rustup (it provides rust; rust-toolchain.toml names the toolchain)"
            # The two conflict, and pacman asks before it removes one for the other.
            sudo pacman -S --needed rustup < <(yes) || die "pacman did not replace rust by rustup"
        else
            missing+=(rustup)
        fi
    fi
    [ ${#missing[@]} != 0 ] || return 0
    step "installing packages: ${missing[*]}"
    sudo pacman -S --needed --noconfirm "${missing[@]}" ||
        die "pacman did not install them. If it could not download a package, its lists are old: run \`sudo pacman -Syu\`, then this again"
}

# A toolchain to build with outside this checkout too: rustup has no default one after it
# replaced the package rust, and `cargo` then answers nothing elsewhere. The one this build
# has just installed costs no download.
default_toolchain() {
    command -v rustup >/dev/null || return 0
    rustup default 2>/dev/null | grep -q . && return 0
    local channel
    channel=$(sed -n 's/^channel *= *"\(.*\)"/\1/p' "$src/rust-toolchain.toml")
    [ -z "$channel" ] || rustup default "$channel" >/dev/null 2>&1 || true
}

# The checkout this file is in, or a clone. A clone runs its own install.sh: that is the one
# written for the sources it installs.
sources() {
    local here=${BASH_SOURCE[0]-}
    here=$([ -f "$here" ] && cd "$(dirname "$here")" && pwd) || here=
    if [ -n "$here" ] && [ -f "$here/tools/install.sh" ] && [ -f "$here/Cargo.toml" ]; then
        src=$here
        return 0
    fi
    local repo=${RI_REPO:-https://github.com/dar1k-ness/rusty-impulse.git}
    src=${RI_SRC:-$HOME/.local/src/rusty-impulse}
    command -v git >/dev/null || die "git is needed to get the sources"
    if [ -d "$src/.git" ]; then
        step "updating $src"
        git -C "$src" pull --ff-only || die "cannot update $src (changes of its own?); nothing was installed"
    else
        step "cloning $repo to $src"
        mkdir -p -- "$(dirname "$src")"
        git clone -- "$repo" "$src" || die "cannot clone $repo; nothing was installed"
    fi
    [ -f "$src/install.sh" ] || die "$src has no install.sh; nothing was installed"
    exec bash "$src/install.sh" --no-packages "$@"
}

hc() { hyprctl "$@" 2>&1; }
in_session() { [ -n "${HYPRLAND_INSTANCE_SIGNATURE-}" ] && command -v hyprctl >/dev/null && hyprctl version >/dev/null 2>&1; }
pids() { pgrep -u "$(id -u)" -x "$1" || true; }
# The layers (panels) a shell has on screen: Quickshell's are "quickshell:...", ours "$word:...".
nlayers() { hc layers -j | jq --arg ns "$1:" '[.[].levels[][] | select(.namespace | startswith($ns))] | length' 2>/dev/null || echo 0; }
# Runs a command the way Hyprland runs one of its own (its environment, not this terminal's):
# through the Lua config's dispatcher, or the older config's.
hypr_exec() {
    local out
    if [ -f "$config/hypr/hyprland.lua" ]; then
        out=$(hc dispatch "hl.dsp.exec_cmd(\"$1\")")
    else
        out=$(hc dispatch exec "$1")
    fi
    [ "$out" = ok ] || { echo "install.sh: hyprctl dispatch answered: $out" >&2; return 1; }
}
stop() {
    local n p
    for n in "$@"; do
        for p in $(pids "$n"); do kill "$p" 2>/dev/null || true; done
    done
    for _ in $(seq 50); do
        p=
        for n in "$@"; do p+=$(pids "$n"); done
        [ -n "$p" ] || return 0
        sleep 0.1
    done
    for n in "$@"; do
        for p in $(pids "$n"); do kill -9 "$p" 2>/dev/null || true; done
    done
}

# What Hyprland started at login from a line that called qs keeps that text until the next
# login: end-4's clipboard watchers (wl-paste --watch bash -c '... qs -c $qsConfig ipc call
# cliphistService update'), which tell the shell that the clipboard history has changed. They
# are stopped and started again from the lines as they are now.
restart_watchers() {
    local p line stopped=''
    for p in $(pids wl-paste); do
        if tr '\0' ' ' <"/proc/$p/cmdline" 2>/dev/null | grep -qE '(^|[^[:alnum:]_./-])qs +-c '; then
            kill "$p" 2>/dev/null && stopped=1
        fi
    done
    [ -n "$stopped" ] || return 0
    grep -rhsE "^[[:space:]]*(hl\.exec_cmd\(\"|exec-once *= *)wl-paste .*$name" "$config/hypr" |
        sed -E -e 's/^[[:space:]]*hl\.exec_cmd\("(.*)"\)[[:space:]]*(--.*)?$/\1/' -e 's/^[[:space:]]*exec-once *= *//' |
        while IFS= read -r line; do
            [[ $line == *'"'* ]] || hypr_exec "$line" || true
        done
}

switch() {
    local wait=${RI_START_WAIT:-30} was='' p i log=${XDG_RUNTIME_DIR:-/tmp}/$name.log
    # The build that runs is the one just installed (this was run a second time): it stays.
    for p in $(pids "$name"); do
        if [ "$(readlink -f "/proc/$p/exe")" = "$(readlink -f -- "$bin")" ] && [ "$(nlayers "$word")" -gt 0 ]; then
            restart_watchers
            echo "installed and running already: $bin (pid $p)"
            return 0
        fi
    done
    # What runs now, to start it again if the new shell does not come up: its command line.
    for p in $(pids qs) $(pids quickshell) $(pids "$name"); do
        was=$(tr '\0' ' ' <"/proc/$p/cmdline" 2>/dev/null) && [ -n "$was" ] && break
    done
    step "switching: Hyprland reloads its config, ${was:+\`${was% }\` stops, }\`$name -c $name\` starts"
    hc reload >/dev/null
    stop qs quickshell "$name"
    hypr_exec "$bin -c $name >$log 2>&1" || true
    for ((i = 0; i < wait * 2; i++)); do
        if [ -n "$(pids "$name")" ] && [ "$(nlayers "$word")" -gt 0 ]; then
            restart_watchers
            echo "installed and running: $bin (pid $(pids "$name" | head -n 1)), its log: $log"
            echo "back to the original: $src/tools/carry-over.sh --dots old, then log in again"
            return 0
        fi
        sleep 0.5
    done

    echo "install.sh: $name did not show its panels in ${wait}s (its log: $log); putting back what ran before" >&2
    stop "$name"
    case $was in
    "") ;;
    "$bin "* | "$name "*)
        # An update: the release before this one, with the files as they are.
        [ -z "$before" ] || [ "$before" = "$(readlink -f -- "$bin")" ] ||
            PREFIX=$HOME/.local "$src/tools/install.sh" --rollback >&2 || true
        hypr_exec "${was% }" || true
        ;;
    *)
        "$src/tools/carry-over.sh" --dots old --quiet >&2 || true
        hc reload >/dev/null
        hypr_exec "${was% }" || true
        echo "install.sh: the build stays installed; the Hyprland files are the ones from before" >&2
        ;;
    esac
    exit 2
}

main "$@"
