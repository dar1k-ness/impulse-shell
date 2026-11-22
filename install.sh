#!/bin/bash
# install.sh: the whole install by one command, on Arch Linux with end-4's dots-hyprland in
# place. It ends with impulse-shell running in the Hyprland session it was started from.
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

    if [ -n "${RI_FORCE_BUILD-}" ] || [ ! -f "$bin" ] || [ ! -x "$bin" ]; then
        step "building and installing to ~/.local (some minutes the first time)"
        before=$(readlink -f -- "$bin" 2>/dev/null || true)
        PREFIX=$HOME/.local "$src/tools/install.sh" || die "the build or the install failed; nothing else was changed"
        default_toolchain
    else
        step "already installed: $bin (skipping the build; RI_FORCE_BUILD=1 rebuilds it)"
        before=$(readlink -f -- "$bin" 2>/dev/null || true)
    fi
    register_shell

    setup_bundled_dots

    step "carrying the settings over"
    local rc=0
    "$src/tools/carry-over.sh" --quiet || rc=$?
    case $rc in 0 | 3 | 4) ;; *) die "tools/carry-over.sh failed; the build is installed, the Hyprland files are untouched" ;; esac

    step "making the Hyprland files call and start $bin"
    "$src/tools/carry-over.sh" --dots new --quiet || die "tools/carry-over.sh --dots new failed; put the files back with: $src/tools/carry-over.sh --dots old"
    start_script
    ensure_autostart

    if [ -z "$start" ]; then
        watch_off later
        echo "installed. Not started (--no-start): the next login starts it."
    elif in_session; then
        switch
    else
        watch_off later
        echo "installed. This is not a Hyprland session, so nothing was started: log in to Hyprland and the shell is there."
    fi
    echo "OK: $name установлен и добавлен в автозапуск (${AUTOSTART_FILE:-$config/hypr})"
}

name=impulse-shell
word=rustshell
bin=$HOME/.local/bin/$name
config=${XDG_CONFIG_HOME-}
[[ $config == /* ]] || config=$HOME/.config
src=
before=
prev=rusty
prev+=-impulse
prev_start=$HOME/.local/bin/$prev-shell
prev_unit=$prev-rollback-watch.service
prev_watch=
watch_back=()
AUTOSTART_FILE=

die() {
    echo "install.sh: $*" >&2
    exit 1
}
step() { echo "== $*"; }
usage() { awk 'NR > 1 && /^#/ { sub(/^# ?/, ""); print; next } NR > 1 { exit }' "${BASH_SOURCE[0]}"; }

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
    mapfile -t missing < <(pacman -T "${PACKAGES[@]}" || true)
    if ! command -v rustup >/dev/null && [ ! -x "$HOME/.cargo/bin/rustup" ]; then
        if pacman -Qq rust >/dev/null 2>&1 && ! pacman -Qq rustup >/dev/null 2>&1; then
            step "replacing the package rust by rustup (it provides rust; rust-toolchain.toml names the toolchain)"
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

default_toolchain() {
    command -v rustup >/dev/null || return 0
    rustup default 2>/dev/null | grep -q . && return 0
    local channel
    channel=$(sed -n 's/^channel *= *"\(.*\)"/\1/p' "$src/rust-toolchain.toml")
    [ -z "$channel" ] || rustup default "$channel" >/dev/null 2>&1 || true
}

register_shell() {
    local shells=/etc/shells
    [ -f "$shells" ] || return 0
    grep -qxF -- "$bin" "$shells" && return 0
    step "registering $bin in $shells (like the other shells)"
    echo "$bin" | sudo tee -a "$shells" >/dev/null || {
        echo "note: could not register $bin in $shells; chsh -l will not list it (add the line yourself: $bin)" >&2
        return 0
    }
}

sources() {
    local here=${BASH_SOURCE[0]-}
    here=$([ -f "$here" ] && cd "$(dirname "$here")" && pwd) || here=
    if [ -n "$here" ] && [ -f "$here/tools/install.sh" ] && [ -f "$here/Cargo.toml" ]; then
        src=$here
        return 0
    fi
    local repo=${RI_REPO:-https://github.com} branch=${RI_BRANCH-main}
    src=${RI_SRC:-$HOME/.local/src/impulse-shell}
    command -v git >/dev/null || die "git is needed to get the sources"
    if [ -d "$src/.git" ]; then
        local cur_url
        cur_url=$(git -C "$src" remote get-url origin 2>/dev/null || true)
        if [ "$cur_url" != "$repo" ]; then
            step "switching $src origin to $repo"
            git -C "$src" remote set-url origin "$repo"
        fi
        step "updating $src"
        git -C "$src" fetch origin ${branch:+"$branch"} || die "cannot fetch $repo"
        git -C "$src" checkout -f ${branch:+"$branch"} >/dev/null 2>&1 || git -C "$src" checkout -B "${branch:-main}" "origin/${branch:-main}"
        git -C "$src" reset --hard "origin/${branch:-main}"
    else
        step "cloning $repo${branch:+ (branch $branch)} to $src"
        mkdir -p -- "$(dirname "$src")"
        git clone ${branch:+--branch "$branch"} -- "$repo" "$src" || die "cannot clone $repo; nothing was installed"
    fi
    [ -f "$src/install.sh" ] || die "$src has no install.sh; nothing was installed"
    exec bash "$src/install.sh" --no-packages "$@"
}
setup_bundled_dots() {
    local bundled="$src/dots/hypr"
    [ -d "$bundled" ] || return 0
    if [ ! -f "$config/hypr/hyprland/keybinds.lua" ] && [ ! -f "$config/hypr/hyprland/keybinds.conf" ]; then
        step "deploying bundled Hyprland dots from $bundled to $config/hypr"
        if [ -e "$config/hypr" ]; then
            local bak="$config/hypr.pre-impulse-$(date +%Y%m%d_%H%M%S)"
            mv -- "$config/hypr" "$bak"
            echo "note: previous $config/hypr moved to $bak"
        fi
        mkdir -p -- "$config"
        cp -a -- "$bundled" "$config/hypr"
    fi
}

hc() { hyprctl "$@" 2>&1; }
in_session() { [ -n "${HYPRLAND_INSTANCE_SIGNATURE-}" ] && command -v hyprctl >/dev/null && hyprctl version >/dev/null 2>&1; }
pids() {
    pgrep -u "$(id -u)" -f "(^|/)$1( |$)" || true
}
nlayers() { hc layers -j | jq --arg ns "$1:" '[.[].levels[][] | select(.namespace | startswith($ns))] | length' 2>/dev/null || echo 0; }

hypr_exec() {
    local out
    if [ -f "$config/hypr/hyprland.lua" ] || hc getoption general:border_size 2>&1 | grep -q "hl\."; then
        out=$(hc dispatch "hl.dsp.exec_cmd(\"$1\")")
    else
        out=$(hc dispatch exec "$1")
        if [[ "$out" == *"hl.dispatch"* ]]; then
            out=$(hc dispatch "hl.dsp.exec_cmd(\"$1\")")
        fi
    fi
    [ "$out" = ok ] || { echo "install.sh: hyprctl dispatch answered: $out" >&2; return 1; }
}

locked() { [ "$(hc -j locked | jq -r .locked 2>/dev/null)" = true ]; }
wait_unlocked() {
    locked || return 0
    step "the session is locked: the switch is made once it is unlocked"
    echo "(stopping the shell now would take its lock screen away. Interrupted here, nothing is lost: the build is installed and the next login starts it)"
    while locked; do sleep 1; done
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

start_script() {
    local s=$HOME/.local/bin/$name-start
    step "writing the start script $s (cd ~, logs to \${XDG_RUNTIME_DIR:-/tmp}/$name.log)"
    mkdir -p -- "${s%/*}" && cat >"$s.new" <<EOF && chmod 755 "$s.new" && mv -f -- "$s.new" "$s" || die "cannot write $s"
#!/bin/bash
cd ~
exec "\$HOME/.local/bin/$name" -c $name >"\${XDG_RUNTIME_DIR:-/tmp}/$name.log" 2>&1 </dev/null
EOF
}

# The command Hyprland should run to start the shell. The start script ($name-start)
# does `cd ~` and logs to $XDG_RUNTIME_DIR/$name.log, so autostart matches exactly
# what switch() runs in the session (which is known to work) and leaves a log to look
# at if a login start fails. Falls back to the bare binary if the script is missing.
autostart_cmd() {
    local s=$HOME/.local/bin/$name-start
    if [ -x "$s" ]; then printf '%s' "$s"; else printf '%s' "$bin -c $name"; fi
}

is_our_shell() {
    local cmd0=${1%% *}
    [ "$cmd0" = "$name" ] || [ "${cmd0##*/}" = "$name" ] || [ "$cmd0" = "$bin" ] ||
        [ "$cmd0" = "$name-start" ] || [ "${cmd0##*/}" = "$name-start" ]
}

# Does an autostart command launch another shell? Matched on the first token's
# basename (so a path like /usr/bin/qs counts too), not a bare substring.
# carry-over.sh only rewrites the quickshell/ii names, so caelestia and others reach
# here. caelestia is only its `shell` subcommand, never its other commands.
is_other_shell() {
    local -a t
    read -ra t <<<"$1"
    [ ${#t[@]} -gt 0 ] || return 1
    case ${t[0]##*/} in
    qs | quickshell | "$prev" | "$prev-shell") return 0 ;;
    caelestia) [ "${t[1]-}" = shell ] && return 0 ;;
    esac
    return 1
}

# Point an existing autostart line at impulse-shell instead of the other shell. Reusing
# the slot that already started a shell at login is more reliable than appending a new
# line in a guessed location, and it removes the old shell in one step.
point_other_shell_in_file() {
    local f=$1 line stripped inner indent kw cmd out=() changed=
    cmd=$(autostart_cmd)
    while IFS= read -r line || [ -n "$line" ]; do
        stripped=${line#"${line%%[![:space:]]*}"}
        inner=
        if [[ $stripped == hl.exec_cmd\(\"* ]]; then
            inner=${stripped#hl.exec_cmd(\"}
            inner=${inner%%\"*}
        elif [[ $stripped == exec-once*=* ]] || [[ $stripped == exec*=* ]]; then
            inner=${stripped#*=}
            inner=${inner#"${inner%%[![:space:]]*}"}
        fi
        if [ -n "$inner" ] && [[ $stripped != --* ]] && [[ $stripped != \#* ]] &&
            ! is_our_shell "$inner" && is_other_shell "$inner"; then
            indent=${line%%[![:space:]]*}
            if [[ $stripped == hl.exec_cmd* ]]; then
                out+=("${indent}hl.exec_cmd(\"$cmd\") -- $name autostart (was: $inner)")
            else
                kw=${stripped%%=*}
                kw=${kw%"${kw##*[![:space:]]}"}
                out+=("${indent}${kw} = $cmd # $name autostart (was: $inner)")
            fi
            changed=1
        else
            out+=("$line")
        fi
    done <"$f"
    [ -z "$changed" ] || {
        printf '%s\n' "${out[@]}" >"$f.new" && mv -f -- "$f.new" "$f" &&
            step "pointed another shell's autostart at $name in ${f#"$config/hypr/"}"
    }
}

point_other_shells() {
    [ -d "$config/hypr" ] || return 0
    local f
    while IFS= read -r -d '' f; do
        point_other_shell_in_file "$f"
    done < <(find "$config/hypr" -type f \( -name '*.lua' -o -name '*.conf' \) -print0)
}

# Prints the file that genuinely autostarts our shell: an exec / exec-once / hl.exec_cmd
# whose command launches impulse-shell or its start script. Lines that only mention the
# binary -- the IPC-call variable and the Ctrl+Super+R restart bind in keybinds.lua -- are
# not autostart and are ignored, so they no longer make the installer think it is done
# (which left the real autostart entry unadded, so nothing started at login). Empty if none.
find_our_autostart() {
    [ -d "$config/hypr" ] || return 0
    local f line stripped inner
    while IFS= read -r -d '' f; do
        while IFS= read -r line || [ -n "$line" ]; do
            stripped=${line#"${line%%[![:space:]]*}"}
            [[ $stripped != --* ]] && [[ $stripped != \#* ]] || continue
            inner=
            if [[ $stripped == hl.exec_cmd\(\"* ]]; then
                inner=${stripped#hl.exec_cmd(\"}
                inner=${inner%%\"*}
            elif [[ $stripped == exec-once*=* ]] || [[ $stripped == exec*=* ]]; then
                inner=${stripped#*=}
                inner=${inner#"${inner%%[![:space:]]*}"}
            else
                continue
            fi
            [ -n "$inner" ] || continue
            [[ " $inner " == *" ipc "* ]] && continue
            if is_our_shell "$inner"; then
                printf '%s' "$f"
                return 0
            fi
        done <"$f"
    done < <(find "$config/hypr" -type f \( -name '*.lua' -o -name '*.conf' \) -print0)
}

ensure_autostart() {
    mkdir -p -- "$config/hypr"

    point_other_shells

    local rf
    for rf in "$config/hypr/hyprland/rules.lua" "$config/hypr/custom/rules.lua" "$config/hypr/hyprland/rules.conf" "$config/hypr/custom/rules.conf"; do
        if [ -f "$rf" ]; then
            sed -i -E 's/(opacity[[:space:]]+0\.[0-9]+|alpha[[:space:]]+0\.[0-9]+)/opacity 1.0/g' "$rf" 2>/dev/null || true
        fi
    done

    local existing
    existing=$(find_our_autostart)
    if [ -n "$existing" ]; then
        AUTOSTART_FILE="$existing"
        return 0
    fi

    local target line
    if [ -f "$config/hypr/hyprland.lua" ]; then
        if [ -f "$config/hypr/custom/execs.lua" ]; then
            target="$config/hypr/custom/execs.lua"
        elif [ -f "$config/hypr/hyprland/execs.lua" ]; then
            target="$config/hypr/hyprland/execs.lua"
        elif [ -f "$config/hypr/config/autostart.lua" ]; then
            target="$config/hypr/config/autostart.lua"
        else
            target="$config/hypr/hyprland.lua"
        fi
        line="hl.exec_cmd(\"$(autostart_cmd)\") -- $name autostart"

        step "adding $name to Lua autostart in $target"
        touch -- "$target"
        if grep -q 'hl\.on("hyprland\.start"' "$target"; then
            grep -qxF -- "  $line" "$target" || sed -i '/hl\.on("hyprland\.start"/a \  '"$line" "$target"
        else
            grep -qxF -- "$line" "$target" || printf '\n%s\n' "$line" >>"$target"
        fi
    else
        if [ -f "$config/hypr/custom/execs.conf" ]; then
            target="$config/hypr/custom/execs.conf"
        elif [ -f "$config/hypr/hyprland/execs.conf" ]; then
            target="$config/hypr/hyprland/execs.conf"
        else
            target="$config/hypr/hyprland.conf"
        fi
        line="exec-once = $(autostart_cmd) # $name autostart"
        step "adding $name to autostart in $target"
        touch -- "$target"
        grep -qxF -- "$line" "$target" || printf '\n%s\n' "$line" >>"$target"
    fi
    AUTOSTART_FILE="$target"
}

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

restart_idle() {
    command -v hypridle >/dev/null || return 0
    if [ -n "$(pids hypridle)" ]; then
        stop hypridle
        hypr_exec "hypridle" || true
    fi
}

switch() {
    wait_unlocked
    watch_off

    step "stopping running shell ($name, $prev, qs, quickshell, caelestia)"
    stop "$name" "$prev" "$prev-shell" qs quickshell caelestia

    local cmd="$bin -c $name"
    [ -x "$HOME/.local/bin/$name-start" ] && cmd="$HOME/.local/bin/$name-start"

    step "starting $name"
    if ! hypr_exec "$cmd"; then
        watch_on
        die "failed to start $name via hyprctl"
    fi

    local i
    for i in $(seq "${RI_START_WAIT:-30}"); do
        if [ -n "$(pids "$name")" ] && [ "$(nlayers "$word")" -gt 0 ]; then
            restart_watchers
            restart_idle
            return 0
        fi
        sleep 1
    done

    watch_on
    die "$name did not start within ${RI_START_WAIT:-30}s (check \${XDG_RUNTIME_DIR:-/tmp}/$name.log)"
}

main "$@"
