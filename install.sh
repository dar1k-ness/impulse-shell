#!/bin/bash
# install.sh: the whole install by one command, on Arch Linux with end-4's dots-hyprland in
# place. It ends with impulse-shell running in the Hyprland session it was started from.
#
#   ./install.sh              from a checkout: install that checkout
#   bash install.sh           from anywhere else (the file alone): clone the repository
#                             (its branch $RI_BRANCH) to $RI_SRC, or update the clone that is
#                             there, and install that
#   ./install.sh --no-start   everything but the last step: the shell that runs keeps running
#   ./install.sh --no-packages   install no packages (they are there, or this is not Arch)
#
# What it does, in this order; each step says what it does before it does it:
#   1. packages: what the build and the shell need and pacman does not have yet (sudo pacman -S).
#      The Rust toolchain comes from rustup (rust-toolchain.toml names the version); a `rust`
#      package that is installed instead is replaced by `rustup`, which provides it.
#   2. the sources: this checkout, or a clone (see above).
#   3. build and install: PREFIX=~/.local tools/install.sh (docs/install.md). The shell's
#      directories have one name, impulse-shell: ~/.config/impulse-shell (settings),
#      ~/.local/share/impulse-shell (its tree), ~/.local/state/impulse-shell,
#      ~/.cache/impulse-shell. What an older release kept under ~/.config/rustshell,
#      ~/.local/state/rustshell and ~/.cache/rustshell is moved there.
#   4. the user's data and files: tools/carry-over.sh, then tools/carry-over.sh --dots new. The
#      settings, state and caches are copied to the new names, and the Hyprland files call and
#      start impulse-shell where they called and started Quickshell. Nothing old is removed.
#   5. the switch, when this runs inside a Hyprland session: Hyprland reloads its config, the
#      shell that runs is stopped and impulse-shell is started. A session that is locked by then
#      (the build is long enough for hypridle) is switched once it is unlocked: the shell that
#      runs holds the lock screen. hypridle is started again, for the lock command of its
#      rewritten file. If the new shell does not show its panels in $RI_START_WAIT seconds,
#      the files of step 4 are put back and the shell that ran is started again: the session
#      is not left without a shell. Outside a session nothing is started: the next login
#      starts it.
#
# A home that runs this shell under its previous name (its tree is ~/.config/rustshell/<that
# name>) is switched from that build: step 4 carries its settings over and makes the files that
# started it start impulse-shell, by ~/.local/bin/impulse-shell-start where they named that
# build's start script (this puts the script there); step 5 stops that build's shell. A
# rollback watch that looked after that build is turned off before its shell is stopped: it
# would start that build again, or go back further, now or at the next login. It is turned on
# again when step 5 puts back what ran before. This installs no watch of its own. Nothing of
# that build is removed; the last lines say how to go back to it.
#
# Back to the original afterwards: tools/carry-over.sh --dots old, then log in again;
# tools/install.sh --uninstall removes the build.
#
# Environment:
#   RI_REPO        the repository to clone (default https://github.com/dar1k-ness/rusty-impulse.git:
#                  the repository on GitHub keeps the name the project had before)
#   RI_BRANCH      its branch (default impulse-next; empty: the one the repository gives)
#   RI_SRC         where the clone goes (default ~/.local/src/impulse-shell)
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
    start_script
    grep -rqsE "(^|[^[:alnum:]_-])($name +-c |$name-start([^[:alnum:]_.-]|\$))" "$config/hypr" ||
        echo "note: nothing under $config/hypr starts the shell: make Hyprland run \`$bin -c $name\` at login"

    if [ -z "$start" ]; then
        watch_off later
        echo "installed. Not started (--no-start): the next login starts it."
    elif in_session; then
        switch
    else
        watch_off later
        echo "installed. This is not a Hyprland session, so nothing was started: log in to Hyprland and the shell is there."
    fi
}

name=impulse-shell
word=rustshell
bin=$HOME/.local/bin/$name
config=${XDG_CONFIG_HOME-}
[[ $config == /* ]] || config=$HOME/.config
src=
before=
# The shell under its previous name: its process, its start script, the unit of its rollback
# watch. (The name is put together from halves: tools/rename/rename.py renames it wherever it
# stands whole.)
prev=rusty
prev+=-impulse
prev_start=$HOME/.local/bin/$prev-shell
prev_unit=$prev-rollback-watch.service
# The script of that watch, once watch_off has turned it off; how watch_on turns it on again.
prev_watch=
watch_back=()

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
    local repo=${RI_REPO:-https://github.com/dar1k-ness/rusty-impulse.git} branch=${RI_BRANCH-impulse-next}
    src=${RI_SRC:-$HOME/.local/src/impulse-shell}
    command -v git >/dev/null || die "git is needed to get the sources"
    if [ -d "$src/.git" ]; then
        step "updating $src"
        git -C "$src" pull --ff-only || die "cannot update $src (changes of its own?); nothing was installed"
    else
        step "cloning $repo${branch:+ (branch $branch)} to $src"
        mkdir -p -- "$(dirname "$src")"
        git clone ${branch:+--branch "$branch"} -- "$repo" "$src" || die "cannot clone $repo; nothing was installed"
    fi
    [ -f "$src/install.sh" ] || die "$src has no install.sh; nothing was installed"
    exec bash "$src/install.sh" --no-packages "$@"
}

hc() { hyprctl "$@" 2>&1; }
in_session() { [ -n "${HYPRLAND_INSTANCE_SIGNATURE-}" ] && command -v hyprctl >/dev/null && hyprctl version >/dev/null 2>&1; }
pids() { pgrep -u "$(id -u)" -x "$1" || true; }
# The layers (panels) a shell has on screen: their names begin with its word and a colon.
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
# Whether the session is locked. A shell that is stopped while it holds the lock screen leaves
# Hyprland on "the lockscreen app died", which only another tty gets out of.
locked() { [ "$(hc -j locked | jq -r .locked 2>/dev/null)" = true ]; }
# The switch waits for the unlock: a build is long enough for hypridle to lock the session.
wait_unlocked() {
    locked || return 0
    step "the session is locked: the switch is made once it is unlocked"
    echo "(stopping the shell now would take its lock screen away. Interrupted here, nothing is lost: the build is installed and the next login starts it)"
    while locked; do sleep 1; done
    # What the shell does at the unlock (its lock screen leaving) is not cut short.
    sleep 1
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

# The start script the previous build's files named, under this build's name: where the
# Hyprland files name it now (tools/carry-over.sh --dots new wrote that) and it is not there,
# it is put there. tools/pc/switch puts the same one; tools/install.sh --uninstall leaves it.
start_script() {
    local s=$HOME/.local/bin/$name-start
    grep -rqsE "(^|[^[:alnum:]_.-])$name-start([^[:alnum:]_.-]|\$)" "$config/hypr" || return 0
    [ ! -x "$s" ] || return 0
    step "putting the start script the Hyprland files name: $s"
    mkdir -p -- "${s%/*}" && cat >"$s.new" <<EOF && chmod 755 "$s.new" && mv -f -- "$s.new" "$s" || die "cannot write $s"
#!/bin/bash
# Starts $name from the home directory, what it prints going to its log. The
# Hyprland files start it by this script where they started the build before it by that one's.
cd ~
exec "\$HOME/.local/bin/$name" -c $name >"\${XDG_RUNTIME_DIR:-/tmp}/$name.log" 2>&1 </dev/null
EOF
}

# The rollback watch of the previous build looks after that build's process and, at each login,
# expects it to be the shell: left on, it starts that build again or goes back further. It is
# turned off by its own script (which removes its unit; the unit names the script), else by
# systemctl. "later": the shell that runs is not stopped now, so the watch keeps running until
# the logout and is only not started at the next login.
watch_off() {
    local unit=$config/systemd/user/$prev_unit p
    [ -e "$unit" ] || return 0
    command -v systemctl >/dev/null || return 0
    prev_watch=$(sed -n 's/^ExecStart=\(.*\) run$/\1/p' "$unit" | head -n 1)
    if [ "${1-}" = later ]; then
        systemctl --user disable "$prev_unit" >/dev/null 2>&1 || true
        watch_back=(systemctl --user enable "$prev_unit")
        echo "the rollback watch of $prev will not start at the next login ($prev_unit is disabled); on again: ${watch_back[*]}"
        return 0
    fi
    step "turning off the rollback watch of $prev ($prev_unit): its shell is stopped next"
    if [ -n "$prev_watch" ] && [ -x "$prev_watch" ]; then
        "$prev_watch" stop >/dev/null 2>&1 || true
        watch_back=("$prev_watch" enable)
    else
        watch_back=(systemctl --user enable --now "$prev_unit")
    fi
    systemctl --user disable --now "$prev_unit" >/dev/null 2>&1 || true
    p=$(cat "${XDG_RUNTIME_DIR:-/tmp}/$prev-rollback-watch.pid" 2>/dev/null || true)
    if [[ $p =~ ^[0-9]+$ ]] && grep -qa rollback-watch "/proc/$p/cmdline" 2>/dev/null; then kill "$p" 2>/dev/null || true; fi
}
watch_on() {
    [ ${#watch_back[@]} != 0 ] || return 0
    if "${watch_back[@]}" >/dev/null 2>&1; then
        echo "install.sh: the rollback watch of $prev is on again" >&2
    else
        echo "install.sh: the rollback watch of $prev did not start; again: ${watch_back[*]}" >&2
    fi
}

# What Hyprland started at login from a line that called qs keeps that text until the next
# login: end-4's clipboard watchers (wl-paste --watch bash -c '... qs -c $qsConfig ipc call
# cliphistService update'), which tell the shell that the clipboard history has changed. They
# are stopped and started again from the lines as they are now. So are the ones that called
# the shell under its previous name.
restart_watchers() {
    local p line stopped=''
    for p in $(pids wl-paste); do
        if tr '\0' ' ' <"/proc/$p/cmdline" 2>/dev/null | grep -qE "(^|[^[:alnum:]_./-])qs +-c |(^|[^[:alnum:]_.-])$prev +-c "; then
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

# hypridle read its config when Hyprland started it, and the lock command in it calls the shell
# that ran then: a lock would now be hyprlock's, or none at all where there is no hyprlock.
# Where its file was written after it started, it is started again. Not while it has a screen
# off: started again, it would not turn that screen on.
restart_idle() {
    local p f=$config/hypr/hypridle.conf up
    p=$(pids hypridle | head -n 1)
    [ -n "$p" ] && [ -f "$f" ] || return 0
    up=$(ps -o etimes= -p "$p" 2>/dev/null | tr -d ' ')
    [[ $up =~ ^[0-9]+$ ]] && [ "$(($(date +%s) - up))" -lt "$(stat -c %Y -- "$f")" ] || return 0
    if [ "$(hc monitors -j | jq '[.[] | select(.dpmsStatus == false)] | length' 2>/dev/null)" != 0 ]; then
        echo "note: hypridle keeps the lock command it read at login until the next login (a screen is off, so it was not started again)"
        return 0
    fi
    stop hypridle
    hypr_exec hypridle || true
}

switch() {
    local wait=${RI_START_WAIT:-30} was='' p i log=${XDG_RUNTIME_DIR:-/tmp}/$name.log
    # The build that runs is the one just installed (this was run a second time): it stays.
    for p in $(pids "$name"); do
        if [ "$(readlink -f "/proc/$p/exe")" = "$(readlink -f -- "$bin")" ] && [ "$(nlayers "$word")" -gt 0 ]; then
            restart_watchers
            restart_idle
            echo "installed and running already: $bin (pid $p)"
            return 0
        fi
    done
    wait_unlocked
    # What runs now, to start it again if the new shell does not come up: its command line.
    for p in $(pids qs) $(pids quickshell) $(pids "$prev") $(pids "$name"); do
        was=$(tr '\0' ' ' <"/proc/$p/cmdline" 2>/dev/null) && [ -n "$was" ] && break
    done
    # The previous build is started again the way its files start it: by its start script.
    [ -z "$(pids "$prev")" ] || [ ! -x "$prev_start" ] || was=$prev_start
    watch_off
    step "switching: Hyprland reloads its config, ${was:+\`${was% }\` stops, }\`$name -c $name\` starts"
    hc reload >/dev/null
    stop qs quickshell "$prev" "$name"
    # A release from before the shell's directories had one name ran until now: what it
    # still wrote under the names the install moved is moved as well.
    PREFIX=$HOME/.local "$src/tools/install.sh" --settle || true
    hypr_exec "$bin -c $name >$log 2>&1" || true
    for ((i = 0; i < wait * 2; i++)); do
        if [ -n "$(pids "$name")" ] && [ "$(nlayers "$word")" -gt 0 ]; then
            restart_watchers
            restart_idle
            echo "installed and running: $bin (pid $(pids "$name" | head -n 1)), its log: $log"
            if [ ${#watch_back[@]} != 0 ]; then
                echo "the rollback watch of $prev is off, and this installs none"
                echo "back to $prev: $src/tools/carry-over.sh --dots old, then log in again, then: ${watch_back[*]}"
            else
                echo "back to the original: $src/tools/carry-over.sh --dots old, then log in again"
            fi
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
        watch_on
        ;;
    esac
    exit 2
}

main "$@"
