#!/bin/bash
# install.sh: the whole install by one command, on Arch Linux with end-4's dots-hyprland in
# place. It ends with impulse-shell running in the Hyprland session it was started from.
set -euo pipefail
shopt -u patsub_replacement 2>/dev/null || true

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
    [[ ${RI_START_WAIT:-30} =~ ^[1-9][0-9]*$ ]] || die "RI_START_WAIT is a number of seconds, not '${RI_START_WAIT-}'; nothing was done"

    [ -z "$packages" ] || install_packages
    sources "$@"
    [ ! -d "$HOME/.cargo/bin" ] || PATH=$HOME/.cargo/bin:$PATH

<<<<<<< Updated upstream
    if [ -n "${RI_FORCE_BUILD-}" ] || [ ! -f "$bin" ] || [ ! -x "$bin" ]; then
        step "building and installing to ~/.local (some minutes the first time)"
        before=$(readlink -f -- "$bin" 2>/dev/null || true)
=======
    show_welcome "$assume_yes"
    if [ -n "$packages" ]; then
        if approve "Install the system packages needed to build impulse-shell."; then
            install_packages
        else
            packages=
        fi
    fi
    if [ -n "$fonts" ] && approve "Install the illogical-impulse fonts and icon assets."; then
        # Fonts must never block the installation of the shell itself.
        install_fonts "$packages" ||
            warn "fonts/icon fonts were not fully installed: the shell is still installed, but icons or text may look wrong (fix the network/AUR problem and rerun the installer)"
    fi

    if [ -n "${RI_FORCE_BUILD-}" ] || [ ! -f "$bin" ] || [ ! -x "$bin" ] || [ ! -f "$shell_tree/shell.qml" ]; then
        if [ -x "$bin" ] && [ ! -f "$shell_tree/shell.qml" ]; then
            step "the installed QML tree is missing; repairing the installation"
        else
            step "building and installing impulse-shell to ~/.local"
        fi
        approve "Build and install impulse-shell to ~/.local." ||
            die "the build is required before the shell can be integrated"
        ensure_toolchain
>>>>>>> Stashed changes
        PREFIX=$HOME/.local "$src/tools/install.sh" || die "the build or the install failed; nothing else was changed"
    else
        step "already installed: $bin (skipping the build; RI_FORCE_BUILD=1 rebuilds it)"
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
<<<<<<< Updated upstream
    echo "OK: $name installed and added to startup (${AUTOSTART_FILE:-$config/hypr})"
=======

    if [ ${#WARNINGS[@]} -gt 0 ]; then
        echo "== finished with ${#WARNINGS[@]} warning(s):" >&2
        for arg in "${WARNINGS[@]}"; do echo "  - $arg" >&2; done
    fi
    if [ -n "${AUTOSTART_FILE-}" ]; then
        echo "OK: $name Installed and added to startup ($AUTOSTART_FILE)"
    else
        echo "OK: $name installed"
    fi
>>>>>>> Stashed changes
}

name=impulse-shell
word=rustshell
bin=$HOME/.local/bin/$name
config=${XDG_CONFIG_HOME-}
[[ $config == /* ]] || config=$HOME/.config
src=
prev=rusty
prev+=-impulse
prev_unit=$prev-rollback-watch.service
prev_watch=
watch_back=()
AUTOSTART_FILE=
WARNINGS=()
HYPR_BACKUP=

die() {
    echo "install.sh: $*" >&2
    exit 1
}
warn() {
    echo "warning: $*" >&2
    WARNINGS+=("$*")
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
            local m have=
            for m in "${missing[@]}"; do [ "$m" != rustup ] || have=1; done
            [ -n "$have" ] || missing+=(rustup)
        fi
    fi
    [ ${#missing[@]} != 0 ] || return 0
    step "installing packages: ${missing[*]}"
    sudo pacman -S --needed --noconfirm "${missing[@]}" ||
        die "pacman did not install them. If it could not download a package, its lists are old: run \`sudo pacman -Syu\`, then this again"
<<<<<<< Updated upstream
}

default_toolchain() {
    command -v rustup >/dev/null || return 0
    rustup default 2>/dev/null | grep -q . && return 0
    local channel
    channel=$(sed -n 's/^channel *= *"\(.*\)"/\1/p' "$src/rust-toolchain.toml")
    [ -z "$channel" ] || rustup default "$channel" >/dev/null 2>&1 || true
=======

}

# rustup >= 1.28 no longer installs the toolchain named by rust-toolchain.toml
# on its own, so a fresh rustup has nothing to build with. Do it explicitly.
ensure_toolchain() {
    command -v rustup >/dev/null || return 0
    local channel out
    channel=$(sed -n 's/^[[:space:]]*channel[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' "$src/rust-toolchain.toml" 2>/dev/null | head -n 1 || true)
    if [ -n "$channel" ]; then
        step "making sure the Rust toolchain '$channel' is installed"
        rustup toolchain install "$channel" --profile minimal ||
            warn "could not install the Rust toolchain '$channel'; the build below will probably fail (network?)"
    fi
    out=$(rustup default 2>&1 || true)
    if [[ $out != *"(default)"* ]]; then
        rustup default "${channel:-stable}" >/dev/null 2>&1 || true
    fi
}

show_welcome() {
    local assume_yes=$1 answer
    printf '\033[1;36m[%s]: Hi there! Before we start:\033[0m\n' "$name"
    printf '\n'
    printf "This installer is for Arch Linux systems that already have end-4's illogical-impulse dotfiles.\n"
    printf 'Quick overview:\n'
    printf '  1. Install the packages needed to build impulse-shell.\n'
    printf '  2. Install the same font families and icon fonts used by illogical-impulse.\n'
    printf '  3. Build and install impulse-shell under ~/.local.\n'
    printf '  4. Carry settings over and configure Hyprland autostart.\n'
    printf '\n'
    printf 'It does not install Hyprland, GPU drivers or the rest of the desktop environment.\n'
    printf 'Existing Hyprland configuration is backed up before it is modified.\n\n'

    if [ "$assume_yes" = 1 ]; then
        ASK=false
        echo 'Confirmation prompts disabled (--yes).'
        return 0
    fi
    [ -r /dev/tty ] || die 'no interactive terminal; run from a terminal or pass --yes'
    printf 'Do you want to confirm every time before an action executes?\n'
    printf '  y = Yes, ask before each action. (DEFAULT)\n'
    printf '  n = No, run the actions without further questions.\n'
    printf '  a = Abort.\n'
    read -r -p '===> [Y/n/a]: ' answer </dev/tty || die 'could not read from the terminal'
    case $answer in
        n | N) ASK=false ;;
        a | A) die 'installation aborted by user' ;;
        *) ASK=true ;;
    esac
}

confirm_action() {
    local description=$1 answer
    [ "$ASK" = false ] && return 0
    printf '\n\033[1;34m[%s]: Next action:\033[0m\n' "$name"
    printf '  %s\n' "$description"
    while true; do
        printf '  y = Yes\n'
        printf '  e = Exit now\n'
        printf '  s = Skip this action (the installation may be incomplete)\n'
        printf "  yesforall = Yes and don't ask again\n"
        read -r -p '====> ' answer </dev/tty || return 2
        case ${answer,,} in
            y | yes | '') return 0 ;;
            e | a | exit) return 2 ;;
            s | skip) return 1 ;;
            yesforall) ASK=false; return 0 ;;
            *) echo 'Please enter y, e, s or yesforall.' ;;
        esac
    done
}

approve() {
    local rc=0
    confirm_action "$1" || rc=$?
    case $rc in
        0) return 0 ;;
        1) echo "note: skipped: $1"; return 1 ;;
        *) die "installation aborted at: $1" ;;
    esac
}

font_installed() {
    # Do not use grep -q here: with set -o pipefail, grep may exit early
    # and make fc-list's SIGPIPE look like a failed font lookup.
    fc-list -f '%{family}\n' 2>/dev/null | grep -Fi -- "$1" >/dev/null
}

# The font functions never call die: a font problem is a warning, not a reason
# to leave the shell uninstalled. They return 1 on failure instead.
ensure_yay() {
    command -v yay >/dev/null 2>&1 && return 0
    command -v paru >/dev/null 2>&1 && return 0
    approve 'Build and install yay-bin from the Arch User Repository (AUR).' || {
        warn 'AUR font packages were skipped (no yay/paru); install them manually or rerun the installer'
        return 1
    }
    command -v makepkg >/dev/null 2>&1 || { warn 'makepkg is missing; install base-devel first'; return 1; }

    local tmp ok=
    tmp=$(mktemp -d "${TMPDIR:-/tmp}/impulse-shell-yay.XXXXXX") || { warn 'cannot create a temporary directory'; return 1; }
    if git clone --depth=1 https://aur.archlinux.org/yay-bin.git "$tmp/yay-bin" &&
        (cd "$tmp/yay-bin" && makepkg -si --noconfirm); then
        ok=1
    fi
    rm -rf -- "$tmp"
    if [ -z "$ok" ] || ! command -v yay >/dev/null 2>&1; then
        warn 'could not build/install yay-bin from the AUR'
        return 1
    fi
    return 0
}

install_font_packages() {
    command -v pacman >/dev/null 2>&1 || {
        echo 'note: pacman is unavailable; skipping package-managed fonts and icon themes'
        return 0
    }

    local -a missing=()
    local package rc=0
    for package in "${FONT_PACKAGES[@]}"; do
        pacman -Qq "$package" >/dev/null 2>&1 || missing+=("$package")
    done
    [ ${#missing[@]} -gt 0 ] || {
        echo 'font and icon-theme packages are already installed'
        return 0
    }

    ensure_yay || return 1
    local aur_helper=yay
    command -v yay >/dev/null 2>&1 || aur_helper=paru
    step "installing font and icon-theme packages: ${missing[*]}"
    # One by one: a single broken AUR package must not skip the others.
    for package in "${missing[@]}"; do
        "$aur_helper" -S --needed --noconfirm "$package" ||
            { warn "could not install the font/icon package: $package"; rc=1; }
    done
    return $rc
}

font_google_sans_flex() {
    local tmp=$1 url sha
    url='https://github.com/googlefonts/googlesans-flex/releases/download/v5.000/GoogleSansFlex-v5.000.zip'
    sha='c20b00878dd801d1bfc06d771732447940c7255bf94cf538ce0102c7fb06fa85'
    font_installed 'Google Sans Flex' && return 0
    step 'downloading Google Sans Flex (pinned release v5.000)'
    curl -fL --retry 3 --output "$tmp/google-sans-flex.zip" "$url" ||
        { warn 'could not download Google Sans Flex'; return 1; }
    printf '%s  %s\n' "$sha" "$tmp/google-sans-flex.zip" | sha256sum -c - ||
        { warn 'Google Sans Flex checksum mismatch; refusing to install the file'; return 1; }
    mkdir -p -- "$tmp/google-sans-flex"
    unzip -oq "$tmp/google-sans-flex.zip" -d "$tmp/google-sans-flex" ||
        { warn 'could not unpack Google Sans Flex'; return 1; }
    find "$tmp/google-sans-flex" -type f \( -iname '*.ttf' -o -iname '*.otf' \) \
        -exec install -m 0644 -t "$FONT_DIR" -- {} + ||
        { warn 'could not copy Google Sans Flex font files'; return 1; }
}

font_material_symbols() {
    local tmp=$1 url sha
    url='https://raw.githubusercontent.com/google/material-design-icons/10ecc14790612ca8fd164e3f4ffe9c16fc50d5b7/variablefont/MaterialSymbolsRounded%5BFILL,GRAD,opsz,wght%5D.ttf'
    sha='7e74e36533ccee12c2b81e75bfcf3ccf123d3bb7e7e46d65d94445f23db0c97c'
    font_installed 'Material Symbols Rounded' && return 0
    step 'downloading Material Symbols Rounded (pinned upstream revision)'
    curl -fL --retry 3 --output "$tmp/MaterialSymbolsRounded.ttf" "$url" ||
        { warn 'could not download Material Symbols Rounded'; return 1; }
    printf '%s  %s\n' "$sha" "$tmp/MaterialSymbolsRounded.ttf" | sha256sum -c - ||
        { warn 'Material Symbols Rounded checksum mismatch; refusing to install the file'; return 1; }
    install -m 0644 -- "$tmp/MaterialSymbolsRounded.ttf" "$FONT_DIR/MaterialSymbolsRounded.ttf" ||
        { warn 'could not install Material Symbols Rounded'; return 1; }
}

install_pinned_fonts() {
    local tool tmp rc=0
    for tool in fc-list fc-cache curl sha256sum unzip; do
        command -v "$tool" >/dev/null 2>&1 || { warn "$tool is required to install fonts but is missing"; return 1; }
    done
    mkdir -p -- "$FONT_DIR" || { warn "cannot create $FONT_DIR"; return 1; }
    tmp=$(mktemp -d "${TMPDIR:-/tmp}/impulse-shell-fonts.XXXXXX") || { warn 'cannot create a temporary directory'; return 1; }

    font_google_sans_flex "$tmp" || rc=1
    font_material_symbols "$tmp" || rc=1
    rm -rf -- "$tmp"

    # Refresh system fonts and explicitly scan the just-installed user font directory.
    # The explicit scan matters on systems where ~/.local/share/fonts is not
    # picked up by the global fc-cache pass until its directory cache is created.
    fc-cache -f >/dev/null 2>&1 || true
    fc-cache -f "$FONT_DIR" >/dev/null 2>&1 || true
    font_installed 'Google Sans Flex' || { warn 'Google Sans Flex is not visible to fontconfig'; rc=1; }
    font_installed 'Material Symbols Rounded' || { warn 'Material Symbols Rounded is not visible to fontconfig'; rc=1; }
    return $rc
}

install_fonts() {
    local with_packages=$1 rc=0
    if [ -n "$with_packages" ]; then
        install_font_packages || rc=1
    else
        echo 'note: package installation was skipped; downloading Google Sans Flex and Material Symbols only'
    fi
    install_pinned_fonts || rc=1
    [ $rc != 0 ] || echo "fonts and icon fonts are ready (user font directory: $FONT_DIR)"
    return $rc
>>>>>>> Stashed changes
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
<<<<<<< Updated upstream
    local repo=${RI_REPO:-https://github.com} branch=${RI_BRANCH-main}
=======
    [ -z "${RI_EXECED-}" ] || die "the sources in ${RI_SRC:-$HOME/.local/src/impulse-shell} are incomplete (no tools/install.sh or Cargo.toml)"
    local repo=${RI_REPO:-https://github.com/dar1k-ness/impulse-shell.git} branch=${RI_BRANCH-main}
>>>>>>> Stashed changes
    src=${RI_SRC:-$HOME/.local/src/impulse-shell}
    command -v git >/dev/null || die "git is needed to get the sources"
    if [ -d "$src/.git" ]; then
        if [ -n "$(git -C "$src" status --porcelain --untracked-files=no 2>/dev/null)" ]; then
            # Never wipe local edits with reset --hard: use the tree as it is.
            step "$src has local changes: using it as is, not updating it"
        else
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
        fi
    else
        step "cloning $repo${branch:+ (branch $branch)} to $src"
        mkdir -p -- "$(dirname "$src")"
        git clone ${branch:+--branch "$branch"} -- "$repo" "$src" || die "cannot clone $repo; nothing was installed"
    fi
    [ -f "$src/install.sh" ] || die "$src has no install.sh; nothing was installed"
<<<<<<< Updated upstream
    exec bash "$src/install.sh" --no-packages "$@"
=======
    RI_EXECED=1 exec bash "$src/install.sh" "$@"
>>>>>>> Stashed changes
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
# Number of layer surfaces of the shell. The namespace prefix is either the
# historical one ($word) or the current name, so a leftover name can't fail the start check.
nlayers() {
    hc layers -j | jq --arg a "$word:" --arg b "$name:" \
        '[.[].levels[][] | select((.namespace | startswith($a)) or (.namespace | startswith($b)))] | length' 2>/dev/null || echo 0
}

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
<<<<<<< Updated upstream
    grep -rqsE "(^|[^[:alnum:]_.-])$name-start([^[:alnum:]_.-]|\$)" "$config/hypr" || return 0
    [ ! -x "$s" ] || return 0
    step "putting the start script the Hyprland files name: $s"
    mkdir -p -- "${s%/*}" && cat >"$s.new" <<EOF && chmod 755 "$s.new" && mv -f -- "$s.new" "$s" || die "cannot write $s"
#!/bin/bash
cd ~
=======
    step "writing the start script $s (cd ~, logs to \$XDG_RUNTIME_DIR/$name.log)"
    mkdir -p -- "${s%/*}" || die "cannot create ${s%/*}"
    cat >"$s.new" <<EOF || die "cannot write $s"
#!/bin/bash
cd ~ || exit 1
>>>>>>> Stashed changes
exec "\$HOME/.local/bin/$name" -c $name >"\${XDG_RUNTIME_DIR:-/tmp}/$name.log" 2>&1 </dev/null
EOF
    chmod 755 "$s.new" && mv -f -- "$s.new" "$s" || die "cannot install $s"
}

<<<<<<< Updated upstream
ensure_autostart() {
    mkdir -p -- "$config/hypr"

    local rf
    for rf in "$config/hypr/hyprland/rules.lua" "$config/hypr/custom/rules.lua" "$config/hypr/hyprland/rules.conf" "$config/hypr/custom/rules.conf"; do
        if [ -f "$rf" ]; then
            sed -i -E 's/(opacity[[:space:]]+0\.[0-9]+|alpha[[:space:]]+0\.[0-9]+)/opacity 1.0/g' "$rf" 2>/dev/null || true
        fi
    done
=======
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
# `qs ipc ...`, `qs kill` and the like talk to a running shell and are not starts.
# caelestia is only its `shell` subcommand, never its other commands.
is_other_shell() {
    local -a t
    local a
    read -ra t <<<"$1"
    [ ${#t[@]} -gt 0 ] || return 1
    case ${t[0]##*/} in
    qs | quickshell)
        for a in "${t[@]:1}"; do
            case $a in ipc | kill | list | log) return 1 ;; esac
        done
        return 0
        ;;
    "$prev" | "$prev-shell") return 0 ;;
    caelestia) [ "${t[1]-}" = shell ] && return 0 ;;
    esac
    return 1
}

# One backup of the whole hypr config per run, made right before the first edit.
backup_hypr() {
    [ -z "$HYPR_BACKUP" ] || return 0
    [ -d "$config/hypr" ] || return 0
    HYPR_BACKUP=$config/hypr.bak-impulse-$(date +%Y%m%d_%H%M%S)
    if cp -aL -- "$config/hypr" "$HYPR_BACKUP"; then
        echo "note: backup of the Hyprland configuration: $HYPR_BACKUP"
    else
        warn "could not back up $config/hypr before editing it"
        HYPR_BACKUP=
    fi
}

# Point an existing autostart line at impulse-shell instead of the other shell. Reusing
# the slot that already started a shell at login is more reliable than appending a new
# line in a guessed location, and it removes the old shell in one step.
point_other_shell_in_file() {
    local f=$1 line stripped inner kind kw whole indent cmd was changed=
    local -a out=()
    local re_lua='hl\.exec_cmd\([[:space:]]*"([^"]*)"[[:space:]]*\)'
    local re_conf='^(exec-once|exec)[[:space:]]*=[[:space:]]*(.*)$'
    cmd=$(autostart_cmd)
    while IFS= read -r line || [ -n "$line" ]; do
        stripped=${line#"${line%%[![:space:]]*}"}
        inner= kind= kw= whole=
        if [[ $stripped != --* && $stripped != \#* ]]; then
            if [[ $stripped =~ $re_lua ]]; then
                kind=lua
                whole=${BASH_REMATCH[0]}
                inner=${BASH_REMATCH[1]}
            elif [[ $stripped =~ $re_conf ]]; then
                kind=conf
                kw=${BASH_REMATCH[1]}
                inner=${BASH_REMATCH[2]}
            fi
        fi
        if [ -n "$inner" ] && ! is_our_shell "$inner" && is_other_shell "$inner"; then
            if [ "$kind" = lua ]; then
                out+=("${line/"$whole"/hl.exec_cmd(\"$cmd\")}")
            else
                indent=${line%%[![:space:]]*}
                was=${inner//\$/}
                out+=("${indent}${kw} = $cmd # $name autostart (was: $was)")
            fi
            changed=1
        else
            out+=("$line")
        fi
    done <"$f"
    [ -n "$changed" ] || return 0
    backup_hypr
    # Written in place (not mv) so symlinked dotfiles stay symlinks.
    printf '%s\n' "${out[@]}" >"$f" &&
        step "pointed another shell's autostart at $name in ${f#"$config/hypr/"}"
}

point_other_shells() {
    [ -d "$config/hypr" ] || return 0
    local f
    # -L: the hypr dir and the files in it are often symlinks into a dotfiles repo.
    while IFS= read -r -d '' f; do
        point_other_shell_in_file "$f"
    done < <(find -L "$config/hypr" -type f \( -name '*.lua' -o -name '*.conf' \) -print0)
}

# Prints the first config file that has an ACTIVE (not commented out) line starting
# impulse-shell at login. Backups live outside $config/hypr and are not looked at.
find_autostart() {
    [ -d "$config/hypr" ] || return 0
    local pat="(^|[^[:alnum:]_-])($name +-c |$name-start([^[:alnum:]_.-]|\$))"
    grep -RlsE --include='*.conf' --include='*.lua' \
        -e "^[[:space:]]*(exec-once|exec)[[:space:]]*=.*$pat" \
        -e "^[^-]*hl\\.exec_cmd\\(.*$pat" \
        "$config/hypr" | head -n 1 || true
}

# Is $1 (a file under $config/hypr) pulled in by the main config $2?
# Used so autostart is never written to a file that Hyprland does not read.
loaded_by_main() {
    local target=$1 main=$2 rel pat
    [ "$target" != "$main" ] || return 0
    [ -f "$main" ] || return 0
    rel=${target#"$config/hypr/"}
    rel=${rel%.*}
    pat=${rel//[\/.]/[./]}
    grep -qE -- "$pat" "$main" 2>/dev/null
}

ensure_autostart() {
    mkdir -p -- "$config/hypr"

    point_other_shells
>>>>>>> Stashed changes

    local existing
    existing=$(find_autostart)
    if [ -n "$existing" ]; then
        AUTOSTART_FILE=$existing
        return 0
    fi

    local target main cand
    backup_hypr
    if [ -f "$config/hypr/hyprland.lua" ]; then
<<<<<<< Updated upstream
        if [ -f "$config/hypr/custom/execs.lua" ]; then
            target="$config/hypr/custom/execs.lua"
        elif [ -f "$config/hypr/hyprland/execs.lua" ]; then
            target="$config/hypr/hyprland/execs.lua"
        elif [ -f "$config/hypr/config/autostart.lua" ]; then
            target="$config/hypr/config/autostart.lua"
        else
            target="$config/hypr/hyprland.lua"
        fi
        line="hl.exec_cmd(\"$bin -c $name\") -- $name autostart"

=======
        main=$config/hypr/hyprland.lua
        target=$main
        for cand in custom/execs.lua hyprland/execs.lua config/autostart.lua; do
            if [ -f "$config/hypr/$cand" ]; then
                target=$config/hypr/$cand
                break
            fi
        done
        loaded_by_main "$target" "$main" || target=$main
>>>>>>> Stashed changes
        step "adding $name to Lua autostart in $target"
        # Own hook block: valid at top level, runs once at compositor start,
        # not on every config reload.
        printf '\nhl.on("hyprland.start", function()\n  hl.exec_cmd("%s")\nend) -- %s autostart\n' \
            "$(autostart_cmd)" "$name" >>"$target"
    else
<<<<<<< Updated upstream
        if [ -f "$config/hypr/custom/execs.conf" ]; then
            target="$config/hypr/custom/execs.conf"
        elif [ -f "$config/hypr/hyprland/execs.conf" ]; then
            target="$config/hypr/hyprland/execs.conf"
        else
            target="$config/hypr/hyprland.conf"
        fi
        line="exec-once = $bin -c $name # $name autostart"
=======
        main=$config/hypr/hyprland.conf
        target=$main
        for cand in custom/execs.conf hyprland/execs.conf; do
            if [ -f "$config/hypr/$cand" ]; then
                target=$config/hypr/$cand
                break
            fi
        done
        loaded_by_main "$target" "$main" || target=$main
>>>>>>> Stashed changes
        step "adding $name to autostart in $target"
        touch -- "$target"
        printf '\nexec-once = %s # %s autostart\n' "$(autostart_cmd)" "$name" >>"$target"
    fi
    AUTOSTART_FILE=$target

    # Never claim success without looking at the result.
    if [ -z "$(find_autostart)" ]; then
        AUTOSTART_FILE=
        warn "autostart was written to $target but could not be found there afterwards: add '$(autostart_cmd)' to your Hyprland autostart by hand"
    fi
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
    grep -RhsE "^[[:space:]]*(hl\.exec_cmd\(\"|exec-once *= *)wl-paste .*$name" "$config/hypr" |
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

    step "stopping running shell ($name, $prev, qs)"
    stop "$name" "$prev" "$prev-shell" qs

    local cmd="$bin -c $name"
    [ -x "$HOME/.local/bin/$name-start" ] && cmd="$HOME/.local/bin/$name-start"

    step "starting $name"
    if ! hypr_exec "$cmd"; then
        watch_on
        die "failed to start $name via hyprctl"
    fi

    local i n wait=${RI_START_WAIT:-30} log=${XDG_RUNTIME_DIR:-/tmp}/$name.log
    for i in $(seq "$wait"); do
        if [ -n "$(pids "$name")" ]; then
            n=$(nlayers)
            [[ $n =~ ^[0-9]+$ ]] || n=0
            if [ "$n" -gt 0 ]; then
                restart_watchers
                restart_idle
                return 0
            fi
        fi
        sleep 1
    done

    if [ -n "$(pids "$name")" ]; then
        # Alive, but no layer surface with a known namespace: keep it running
        # (rolling back to the old shell would only make things worse).
        warn "$name is running, but no layer surface with the '$word:' or '$name:' namespace appeared within ${wait}s; if the bar is missing, see $log and 'hyprctl layers'"
        restart_watchers
        restart_idle
        return 0
    fi
    watch_on
<<<<<<< Updated upstream
    die "$name did not start within ${RI_START_WAIT:-30}s (check \${XDG_RUNTIME_DIR:-/tmp}/$name.log)"
=======
    [ ! -f "$log" ] || { echo "--- last lines of $log:" >&2; tail -n 20 "$log" >&2 || true; }
    die "$name did not start within ${wait}s (check $log)"
>>>>>>> Stashed changes
}

if [ -z "${RI_SOURCE_ONLY-}" ]; then
    main "$@"
fi
