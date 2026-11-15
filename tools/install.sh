#!/bin/bash
# Install the native shell as `impulse-shell` (its own name: it no longer poses as Quickshell's `qs`).
# Hyprland starts it as `impulse-shell -c impulse-shell`; `impulse-shell ipc call ...` is its Qt-free IPC client. A
# real `qs` does not reach it: its socket is under $XDG_RUNTIME_DIR/rustshell, not Quickshell's. `pidof qs`
# checks (hypridle lock_cmd) must also accept `pidof impulse-shell`.
#   tools/install.sh              build and install, keeping the release it replaces
#   tools/install.sh --rollback   switch back to that release (again: forward to the newer one)
#   tools/install.sh --mark-good  the current release has proved good here: it is kept until
#                                 another one is marked, however many installs follow
#                                 (SHA256=<hex>: only if that is its binary's sha256)
#   tools/install.sh --rollback-good  switch back to the release marked good
#   tools/install.sh --status     the current and the previous release, the impulse-shell config, and
#                                 how many replaced configs are kept
#   tools/install.sh --uninstall  removes the releases and the link to them; the impulse-shell config, the
#                                 kept configs and an impulse-shell of another origin stay
#   tools/install.sh --help       this text
#   PREFIX=/usr/local (default)   Needs cargo and jq to build.
#   NOBUILD=1: install the existing build of this checkout: the impulse-shell in release/ of Cargo's
#   target directory or, built for a named target (build.target), in <target>/release/.
#   BIN=<file>: install this impulse-shell, built elsewhere from the tree RI_ROOT (needs no cargo).
#   RI_ROOT=<tree> (default: `qml` of this checkout): the QML tree the binary is compiled
#   from. It has no JavaScript interpreter and refuses to start on a tree whose scripts differ
#   from this one's.
# Run it as the user whose shell it is, not as root (sudo, doas, pkexec, su): it uses sudo
# itself where the target directories need it. The impulse-shell config is ~/.config/rustshell/impulse-shell
# ($XDG_CONFIG_HOME), what `impulse-shell -c impulse-shell` runs: an install makes it a copy of the tree it
# built, with the files' executable bits. A relative XDG_CONFIG_HOME or XDG_STATE_HOME is
# ignored, as the XDG Base Directory Specification and impulse-shell have it.
#
# The binary and the QML tree it was compiled from are one release:
#   $PREFIX/bin/impulse-shell                    -> ../lib/impulse-shell/current/impulse-shell
#   $PREFIX/lib/impulse-shell/current, previous  -> releases/<id>
#   $PREFIX/lib/impulse-shell/good               -> releases/<id>, the release marked good, if one is
#   $PREFIX/lib/impulse-shell/releases/<id>/     impulse-shell and tree/, a copy of the tree it runs (files
#                                        through links copied as files, without .git)
# An install first asks the new binary whether it has a JavaScript interpreter
# (`impulse-shell --version`) and whether it runs the tree (`impulse-shell --check-tree`), and replaces
# nothing if it has one or does not. It then puts the release's tree in the impulse-shell config's place
# and asks the binary the way Hyprland starts it, `impulse-shell -c impulse-shell --check-tree`: if that does not
# find the config and run it (a shell.qml of the user's own in ~/.config/rustshell hides the
# named configs beside it: Quickshell's rule), the install or rollback is undone.
# A binary that $PREFIX/bin/impulse-shell runs and no install put there (a plain file, a link to a
# build elsewhere) is kept as a release first, legacy-<id>, so it can be rolled back to. Its
# tree is the config it ran, and a rollback puts that config back as it was: a link in it to
# a file of the tree itself is stored, and comes back, as that link.
# The release it replaces becomes the previous one, unless both have the same binary: then
# the previous one stays. A rollback switches the links back and puts that release's tree in
# the config's place if the two releases' trees differ, the older binary does not run the
# config as it is now, or there is no config. An install or a rollback that fails at a step
# puts back what it replaced: the release that was installed stays, with the config it runs.
# Two releases are kept, the current and the previous one: each install removes the others.
# After two installs in a row the previous release is the one the first of them made, and
# the release that ran before both is gone. So a release that has proved good on this machine
# is marked (--mark-good, by tools/pc/rollback-watch once the shell has run long enough): it
# is kept beside those two, and --rollback-good switches to it, with its tree, as --rollback
# does to the previous one; the release switched from becomes the previous one.
# One that is killed between its steps (SIGKILL, the power) leaves a shell that starts: while
# the config and the links are switched, `current` names releases/.switch, whose impulse-shell starts
# the release that runs the config as it is. Each of these steps is written to the disk before
# the next one is made: $PREFIX and the config may be on two file systems, and after a power
# cut neither is then ahead of the other. The next install, rollback or uninstall first
# finishes what was killed if the config is already the new release's, and undoes it if not.
# On SIGINT, SIGTERM or SIGHUP the script undoes what it has switched and exits, also when
# its terminal is gone and nothing can be printed.
# A config edited since its install is kept as ~/.local/state/impulse-shell/replaced-<time>.<random>
# ($XDG_STATE_HOME) once it is replaced, its links kept as links; these copies are never
# removed by the script. An install and a rollback do not restart a running shell.
#
# The shell's own windows (Settings, Welcome, the conflicts window) have the app id
# org.rustshell. An install writes that id's desktop entry and icon into the user's data
# directory, ~/.local/share ($XDG_DATA_HOME):
#   applications/org.rustshell.desktop
#   icons/hicolor/scalable/apps/org.rustshell.svg
# There impulse-shell finds the icon the bar, the overview and the dock show for these windows
# (without them: a placeholder). Both files are written out of this script, so a copy of the
# script alone installs them. A rollback leaves them; --uninstall removes them.
set -euo pipefail
PREFIX=${PREFIX:-/usr/local}
bin_dir=$PREFIX/bin
lib=$PREFIX/lib/impulse-shell
# The XDG base directories as impulse-shell finds them (crates/qml/src/xdg.rs): the variable's value
# if it is an absolute path, the default under $HOME otherwise. The XDG Base Directory
# Specification has a relative path in these variables ignored.
config_home=${XDG_CONFIG_HOME-}
[[ $config_home == /* ]] || config_home=$HOME/.config
state_home=${XDG_STATE_HOME-}
[[ $state_home == /* ]] || state_home=$HOME/.local/state
data_home=${XDG_DATA_HOME-}
[[ $data_home == /* ]] || data_home=$HOME/.local/share
for v in XDG_CONFIG_HOME XDG_STATE_HOME XDG_DATA_HOME; do
    [[ -z ${!v-} || ${!v} == /* ]] ||
        echo "install.sh: $v=${!v} is not an absolute path and is ignored, as impulse-shell ignores it (XDG Base Directory Specification)" >&2
done
config_tree=$config_home/rustshell/impulse-shell
state_dir=$state_home/impulse-shell
# The desktop entry and the icon of the shell's own windows, by their app id.
app_id=org.rustshell
app_entry=$data_home/applications/$app_id.desktop
app_icon=$data_home/icons/hicolor/scalable/apps/$app_id.svg

die() { echo "install.sh: $*" >&2; exit 1; }
# The message of a function whose caller still has something to put back: status 1.
fail() {
    echo "install.sh: $*" >&2
    return 1
}
usage() { awk 'NR > 1 && /^#/ { sub(/^# ?/, ""); print; next } NR > 1 { exit }' "$0"; }
# Whether `$1` can be created or written without more rights: asks its nearest existing parent.
writable() {
    local d=$1
    while [ ! -e "$d" ]; do d=$(dirname "$d"); done
    [ -w "$d" ]
}
SUDO=
if [ "$(id -u)" != 0 ] && ! { writable "$bin_dir" && writable "$lib"; }; then SUDO=sudo; fi
# Runs a command that makes files under $PREFIX with the umask of installed files, whatever
# the umask of the user who installs: the user who runs impulse-shell reads them, also when they are
# root's (sudo runs a command with the stricter of its own umask and the caller's).
in_prefix() { (umask 022 && $SUDO "$@"); }
# Run as root by another user, $HOME and the config would be root's, not those of the user
# whose shell it is. sudo, doas and pkexec name that user in the environment (sudo(8),
# doas(1), pkexec(1)); su does not, so the name the session was logged in with is asked for
# (logname(1)).
as_the_user() {
    local who how
    [ "$(id -u)" = 0 ] || return 0
    if [ -n "${SUDO_USER:-}" ]; then
        who=$SUDO_USER how="through sudo"
    elif [ -n "${DOAS_USER:-}" ]; then
        who=$DOAS_USER how="through doas"
    elif [ -n "${PKEXEC_UID:-}" ]; then
        who=$(id -un "$PKEXEC_UID" 2>/dev/null) || who="the user $PKEXEC_UID"
        how="through pkexec"
    else
        who=$(logname 2>/dev/null) || who=
        how="as root"
    fi
    [ -z "$who" ] || [ "$who" = root ] ||
        die "run it as $who, not $how: the impulse-shell config is $who's (it uses sudo itself where it needs to)"
}

# Earlier installs replaced `qs` with a link to impulse-shell: drop it so `qs` is Quickshell again.
drop_qs_link() { [ "$(readlink "$bin_dir/qs" 2>/dev/null)" != impulse-shell ] || $SUDO rm -f "$bin_dir/qs"; }
# Whether the binary `$1` runs the QML tree `$2` as it is: exit status 0 if it does, 1 with the
# files on stderr if it does not. With an empty environment: a binary older than --check-tree
# ignores the option and starts as a shell, which then finds no compositor to connect to and
# exits with an error. (`exec -c`, not env(1), which takes a path with `=` in it for a
# variable.)
accepts() { (exec -c "$1" --check-tree -p "$2"); }
# Whether the binary `$1`, started the way Hyprland starts the shell (`impulse-shell -c impulse-shell`), finds
# the impulse-shell config and runs it as it is: status 0 with its answer on stdout if it does, 1 with
# the reason on stderr if not. It finds a named config by Quickshell's rule (locateConfigFile):
# in the first of $XDG_CONFIG_HOME/rustshell and the quickshell directories of
# $XDG_CONFIG_DIRS that holds it, and never in one that has a shell.qml of its own. HOME and
# those two variables decide the answer; the rest of the environment is left out as for accepts.
launches() {
    local out rc=0 at v
    out=$(
        for v in $(compgen -e); do
            case $v in HOME | XDG_CONFIG_HOME | XDG_CONFIG_DIRS) ;; *) export -n "$v" ;; esac
        done
        exec "$1" -c impulse-shell --check-tree
    ) || rc=$?
    at=$(realpath "$config_tree" 2>/dev/null) || at=$config_tree
    if [ $rc = 0 ] && [ "$out" = "impulse-shell: $at has the scripts this binary was compiled from" ]; then
        echo "$out"
        return 0
    fi
    {
        echo "install.sh: \`impulse-shell -c impulse-shell\`, how Hyprland starts the shell, does not run the impulse-shell config $config_tree (exit status $rc${out:+; it says: $out})"
        [ ! -f "$config_home/rustshell/shell.qml" ] ||
            echo "install.sh: $config_home/rustshell/shell.qml is a config of its own: with it there, Quickshell and impulse-shell look up no named config such as impulse-shell in that directory. Move it, with the files it uses, to $config_home/rustshell/default/ and it stays the default config"
    } >&2
    return 1
}
# Has the file system that holds `$@` write what was made and renamed on it to the disk. The
# step after it may be on another file system (the config and $PREFIX need not share one), and
# after a power cut that one must not be ahead of this one.
flush() { sync -f "$@" 2>/dev/null || sync; }
# Refuses the binary `$1` unless it runs and says it has no JavaScript interpreter.
vet() {
    local out rc=0
    out=$(exec -c "$1" --version 2>&1) || rc=$?
    case $rc in
    0) ;;
    126 | 127) die "cannot run $1: $out; nothing was installed" ;;
    *) die "$1 does not answer --version (exit status $rc): it is not impulse-shell, or one too old to say whether it has a JavaScript interpreter; nothing was installed" ;;
    esac
    grep -qx 'JavaScript interpreter: none' <<<"$out" ||
        die "$1 has a JavaScript interpreter (its --version: $(tr '\n' ' ' <<<"$out")); the installed shell must have none: build it with tools/install.sh; nothing was installed"
}
# Says what the binary `$1` misses on this machine of the libraries it opens only when they
# are there. An impulse-shell from before it told (no such line in its --version) says nothing.
optional() {
    local out
    out=$(exec -c "$1" --version 2>/dev/null) || return 0
    ! grep -qx 'Thai line breaking: at spaces only.*' <<<"$out" ||
        echo "install.sh: note: libthai (libthai.so.0) is not installed: Thai text is broken at spaces only, as Qt breaks it without the library; with libthai it is broken between words" >&2
}
# The desktop entry of the shell's own windows: the one Quickshell's package has for its app
# id (in its assets/), under this shell's app id. NoDisplay: it names the icon and is not an
# application to start.
app_entry_text() {
    cat <<EOF
[Desktop Entry]
Version=1.5
Type=Application
NoDisplay=true

Name=Rustshell
Icon=$app_id
EOF
}
# Their icon, the gear the original's windows have: Quickshell's own icon, unchanged (the
# file quickshell.svg in the assets of Quickshell's source, git.outfoxxed.me, commit 9b0f339b,
# which its package installs under its app id). Copyright the Quickshell contributors, GNU
# LGPL 3; see docs/third-party.md.
app_icon_text() {
    cat <<'EOF'
<svg xmlns="http://www.w3.org/2000/svg" width="724.635" height="724.635"><path fill="#359757" stroke="#359757" stroke-linecap="square" stroke-linejoin="round" stroke-width="74.755" d="m37.378 160.237 122.859-122.86h527.02v527.02l-122.86 122.86H37.378Z"/><path fill="#fff" stroke="#fff" stroke-linecap="round" stroke-linejoin="round" stroke-width="12.201" d="M323.051 96.412a268.74 268.74 0 0 0-3.51.542c-4.052 14.481-7.815 29.941-14.904 42.692a230.02 230.02 0 0 0-59.406 24.679c-14.036-3.974-27.647-12.214-40.766-19.562a268.788 268.788 0 0 0-60.035 60.16c7.376 13.105 15.645 26.698 19.648 40.726a230.02 230.02 0 0 0-24.554 59.458c-12.735 7.115-28.186 10.913-42.66 14.994a268.789 268.789 0 0 0 .09 84.992c14.48 4.05 29.941 7.814 42.691 14.903a230.02 230.02 0 0 0 24.68 59.406c-3.974 14.037-12.215 27.647-19.563 40.766a268.788 268.788 0 0 0 60.161 60.035c13.104-7.376 26.696-15.645 40.725-19.648a230.02 230.02 0 0 0 59.457 24.555c7.116 12.735 10.913 28.186 14.995 42.659a268.788 268.788 0 0 0 84.99-.09c4.052-14.482 7.817-29.941 14.906-42.691a230.02 230.02 0 0 0 59.405-24.68c14.037 3.974 33.069 17.638 46.188 24.986a268.788 268.788 0 0 0 60.035-60.161c-7.376-13.105-21.068-32.12-25.071-46.149a230.02 230.02 0 0 0 24.555-59.457c12.735-7.116 28.186-10.912 42.659-14.993a268.788 268.788 0 0 0-.089-84.993c-14.482-4.051-29.942-7.814-42.692-14.904a230.02 230.02 0 0 0-24.68-59.405c3.974-14.037 12.216-27.647 19.565-40.767a268.788 268.788 0 0 0-60.161-60.035c-13.105 7.376-26.698 15.645-40.726 19.649a230.02 230.02 0 0 0-59.458-24.555c-7.115-12.735-10.913-28.187-14.994-42.66a268.788 268.788 0 0 0-81.481-.452zm15.778 106.85c58.282-8.328 116.455 15.865 151.643 63.065 35.19 47.2 41.766 109.86 17.144 163.337l-41.728-22.688s-38.558 31.44-57.344 63.012l23.893 36.326a160.78 160.78 0 0 1-46.633 15.058c-87.854 12.99-169.6-47.708-182.573-135.564-12.974-87.855 47.74-169.59 135.598-182.546Z"/></svg>
EOF
}
# Writes stdin as the file `$1` of the user's, whole or not at all.
put_file() {
    local to=$1 new=${1%/*}/.${1##*/}.new
    mkdir -p "${to%/*}" && cat >"$new" || return 1
    if cmp -s "$new" "$to"; then rm -f "$new"; else mv -f "$new" "$to"; fi
}
# The shell runs without the two files (its own windows then show a placeholder icon), so an
# install that cannot write them is still one.
put_app_icon() {
    app_entry_text | put_file "$app_entry" && app_icon_text | put_file "$app_icon" ||
        echo "install.sh: note: cannot write $app_entry and $app_icon: the shell's own windows (Settings, Welcome) show a placeholder as their icon" >&2
}
# Removes the two files and the directories of the icon that are empty then.
drop_app_icon() {
    rm -f "$app_entry" "$app_icon" "${app_entry%/*}/.${app_entry##*/}.new" "${app_icon%/*}/.${app_icon##*/}.new"
    (cd "$data_home/icons" 2>/dev/null && rmdir -p hicolor/scalable/apps 2>/dev/null) || true
}
# The executable files of the tree `$1`, read through links, .git aside.
executables() {
    (cd "$1" && find -L . -name .git -prune -o -type f -perm -u+x -print0 | LC_ALL=C sort -z)
}
# Whether the trees `$1` and `$2` hold the same files, read through links, .git aside: their
# bytes, and which of them are executable (all of a mode that a commit holds).
same_tree() {
    diff -rq -x .git "$1/." "$2/." >/dev/null 2>&1 &&
        cmp -s <(executables "$1" 2>/dev/null) <(executables "$2" 2>/dev/null)
}
# The files that are executable in only one of the trees `$1` and `$2`, one a line.
other_modes() {
    LC_ALL=C comm -z -3 <(executables "$1") <(executables "$2") | tr -d '\t' | tr '\0' '\n'
}
# The id of the release the link `$1` (current, previous) names, if it has a binary.
release() {
    local target
    target=$(readlink "$lib/$1" 2>/dev/null) && [ -x "$lib/$target/impulse-shell" ] && echo "${target#releases/}"
}
# Makes the link `$1` (current, previous) point at `$2`, or removes it if `$2` is empty. Link
# then rename: the name always points at a complete release, and renaming over a running
# binary's name does not fail with ETXTBSY as writing to it does.
set_link() {
    if [ -z "$2" ]; then
        $SUDO rm -f "$lib/$1"
    else
        $SUDO ln -sfn "$2" "$lib/.$1.new" &&
            $SUDO mv -Tf "$lib/.$1.new" "$lib/$1"
    fi
}
point() { set_link "$1" "releases/$2"; }
# Nothing to do when the name already is the link: it follows `current`.
link_bin() {
    [ "$(readlink "$bin_dir/impulse-shell" 2>/dev/null)" != ../lib/impulse-shell/current/impulse-shell ] || return 0
    in_prefix mkdir -p "$bin_dir" &&
        $SUDO ln -sfn ../lib/impulse-shell/current/impulse-shell "$bin_dir/.impulse-shell.new" &&
        $SUDO mv -Tf "$bin_dir/.impulse-shell.new" "$bin_dir/impulse-shell"
}
# The switch in progress. The config and the links cannot be replaced in one step, so from
# before the config is replaced until the links name the new pair, `current` names this
# directory. Its impulse-shell (begin_switch writes it) starts the release that runs the config as it
# is at that moment, and its journal says what was being switched, for recover: an install or
# a rollback that is killed at any step leaves a shell that starts.
switch=$lib/releases/.switch
switching() { [ "$(readlink "$lib/current" 2>/dev/null)" = releases/.switch ]; }
# Writes the journal and that impulse-shell for a switch from the release `$1` (none: a first
# install), whose `previous` link is `$2`, to the release `$3` with `$4` as the previous one,
# and makes `current` name them. `$5` is the release to remove if the switch is undone (the
# one an install made), `$6` the link the name of the binary was if it was not an install's
# (link_bin replaces it). Of the config, when prepare_tree prepared one ($tree_change): $live
# is replaced by $new, through $old if there is a config.
begin_switch() {
    local stage=$switch.new j_live='' q script
    [ "$tree_change" != prepared ] || j_live=$live
    q=$(printf 'j_cur=%q j_prev=%q j_to=%q j_other=%q j_drop=%q j_live=%q j_old=%q j_new=%q j_kept=%q j_bin=%q' \
        "$1" "$2" "$3" "$4" "$5" "$j_live" "${j_live:+$old}" "${j_live:+$new}" "${j_live:+$kept}" "$6")
    $SUDO rm -rf "$stage" "$switch" && in_prefix mkdir "$stage" || return 1
    # The two files are written from here-strings, not through pipelines (see `apart`).
    script=$(
        echo '#!/bin/bash'
        echo '# Written by tools/install.sh for the time it switches releases; one that was killed'
        echo '# left it here. It starts the release that runs the impulse-shell config as it is. The next'
        echo '# tools/install.sh (an install, --rollback, --uninstall) removes it.'
        printf 'lib=%q\n%s\n' "$lib" "$q"
        cat <<'EOS'
# A tools/install.sh still at work holds the lock: wait for it. The lock's descriptor is one
# that was free: those this script was started with (a reloading shell passes the pipe of
# its signals on) reach the release.
exec {lock}<"$lib/.lock" && flock "$lock"
# The release starts under the name this script was started as ($PREFIX/bin/impulse-shell): that is
# the path its reload and its own windows run, which leads to the release installed since.
if [ "$(readlink "$lib/current")" != releases/.switch ]; then
    exec {lock}<&-
    exec -a "$0" "$lib/current/impulse-shell" "$@"
fi
run=$j_to
if [ -n "$j_live" ]; then
    # Killed between the two renames: the config that was there goes back.
    if [ -n "$j_old" ] && [ ! -e "$j_live" ] && [ -e "$j_old" ]; then mv -T "$j_old" "$j_live"; fi
    # The new tree is still beside the config: the config is the one that was there.
    [ ! -e "$j_new" ] || run=$j_cur
fi
exec {lock}<&-
if [ -z "$run" ]; then
    echo "impulse-shell: its first install was interrupted: run tools/install.sh again" >&2
    exit 1
fi
exec -a "$0" "$lib/releases/$run/impulse-shell" "$@"
EOS
    )
    $SUDO tee "$stage/impulse-shell" >/dev/null <<<"$script" &&
        $SUDO tee "$stage/journal" >/dev/null <<<"$q" &&
        $SUDO chmod 755 "$stage/impulse-shell" && $SUDO chmod 644 "$stage/journal" || return 1
    # What the switch renames into place is on the disk before it is: the release, these two
    # files and the copy of the tree beside the config (the power may go at any step). And
    # `current` names the switch on the disk before the config is replaced.
    flush "$stage" ${j_live:+"$new"}
    $SUDO mv -T "$stage" "$switch" && set_link current releases/.switch && flush "$lib"
}
# Under the lock: finishes or undoes the switch that a killed install or rollback left, and
# removes what a killed one left behind. Finished if the config is the new release's or the
# switch did not replace it, undone otherwise; `undo` as `$1` (this run, on a signal): undone
# in both cases.
recover() {
    local j_cur='' j_prev='' j_to='' j_other='' j_drop='' j_live='' j_old='' j_new='' j_kept='' j_bin='' d finished=1
    if switching && [ ! -e "$switch/journal" ]; then
        echo "install.sh: $lib/current names a switch that is gone: no release is the current one" >&2
        set_link current ""
    fi
    if [ -e "$switch/journal" ]; then
        # shellcheck disable=SC1091
        . "$switch/journal"
        [ -z "$j_live" ] || [ -w "$(dirname "$j_live")" ] ||
            die "an install or a rollback that replaces $j_live was interrupted: the next one by that config's user finishes or undoes it (impulse-shell starts all the same)"
        if [ "${1:-}" = undo ] && switching; then
            finished=
            # The config is the new release's: that tree goes back beside it.
            if [ -n "$j_live" ] && [ ! -e "$j_new" ]; then
                [ -z "$j_old" ] || [ ! -e "$j_live/.git" ] || mv -T "$j_live/.git" "$j_old/.git"
                mv -T "$j_live" "$j_new"
            fi
        fi
        # Between the two renames: the config that was there goes back.
        if [ -n "$j_old" ] && [ ! -e "$j_live" ] && [ -e "$j_old" ]; then mv -T "$j_old" "$j_live"; fi
        [ -z "$j_live" ] || [ ! -e "$j_new" ] || finished=
        # The config is as it stays, on the disk, before a link names a release again.
        [ -z "$j_live" ] || flush "$(dirname "$j_live")"
        if switching; then
            if [ -n "$finished" ]; then
                set_link previous "${j_other:+releases/$j_other}" && set_link current "releases/$j_to" ||
                    die "cannot finish the install or rollback that was interrupted: the links in $lib cannot be switched (impulse-shell starts all the same)"
                prune "$j_to" "$j_other"
                echo "finished the install or rollback that was interrupted: $j_to is the current release"
            else
                set_link previous "$j_prev" && set_link current "${j_cur:+releases/$j_cur}" ||
                    die "cannot undo the install or rollback that was interrupted: the links in $lib cannot be switched (impulse-shell starts all the same)"
                [ -n "$j_cur" ] && [ -z "$j_bin" ] || unlink_bin "$j_bin"
                echo "undid the install or rollback that was interrupted: ${j_cur:-no release} is the current release"
            fi
        fi
        if [ -n "$finished" ]; then
            if [ -n "$j_old" ]; then
                [ ! -e "$j_old/.git" ] || mv -T "$j_old/.git" "$j_live/.git"
                rm -rf "$j_old"
                flush "$(dirname "$j_live")"
            fi
            # The copy of the config that was replaced: the killed run did not get to say
            # where it is, or to give it its name.
            if [ -n "$j_kept" ]; then
                [ ! -e "$(keeping "$j_kept")" ] || mv -T "$(keeping "$j_kept")" "$j_kept"
                [ ! -e "$j_kept" ] || say_kept "$j_live" "$j_kept" "$j_cur"
            fi
        else
            rm -rf ${j_kept:+"$j_kept" "$(keeping "$j_kept")"} ${j_new:+"$j_new"}
        fi
        [ -z "$j_drop" ] || [ "$(readlink "$lib/current" 2>/dev/null)" = "releases/$j_drop" ] ||
            $SUDO rm -rf "$lib/releases/$j_drop"
    fi
    $SUDO rm -rf "$switch" "$switch.new" "$lib/releases/.staging"
    # Copies that a killed run made and did not use: of a release's tree beside the config,
    # of an edited config in the state directory.
    d=$(realpath "$config_tree" 2>/dev/null) || d=$config_tree
    for d in "$(dirname "$d")"/.ri-restore.?????? "$state_dir"/.replaced-*; do
        [ ! -d "$d" ] || rm -rf "$d"
    done
}
# Removes the releases other than `$1`, `$2` and the one marked good.
prune() {
    local d good
    good=$(release good) || good=
    for d in "$lib"/releases/*/; do
        case $(basename "$d") in "$1" | "$2" | "$good") ;; *) $SUDO rm -rf "$d" ;; esac
    done
}
# Makes the release `$1` the current one and `$2` the previous one (empty: there is none),
# replaces the config if prepare_tree prepared that, and links the binary, all inside a
# switch (begin_switch): `current` names `$1` as the last step. If a step fails, the config
# and the links are put back as they were, the release `$3` (the one an install made) is
# removed and the status is 1, after a message that ends in `$4`. If the links cannot be put
# back either, the switch stays for the next install or rollback to finish or undo.
switch_links() {
    local was_cur was_prev was_bin made_bin= why="cannot switch the links in $lib"
    was_cur=$(readlink "$lib/current" 2>/dev/null) || was_cur=
    was_prev=$(readlink "$lib/previous" 2>/dev/null) || was_prev=
    # The name of the binary as it is, unless it is an install's link already: none, or a
    # link that leads to no binary (adopt_bin has kept a binary it led to as a release).
    was_bin=$(readlink "$bin_dir/impulse-shell" 2>/dev/null) || was_bin=
    if [ "$was_bin" = ../lib/impulse-shell/current/impulse-shell ]; then was_bin=; else made_bin=1; fi
    if begin_switch "${was_cur#releases/}" "$was_prev" "$1" "$2" "$3" "$was_bin" && link_bin &&
        { [ "$tree_change" != prepared ] || swap_tree; } &&
        # The new pair as Hyprland starts it. A binary from before releases cannot be asked.
        { [ -e "$lib/releases/$1/unverified" ] || launches "$lib/releases/$1/impulse-shell" >/dev/null ||
            { why="$bin_dir/impulse-shell would not start as \`impulse-shell -c impulse-shell\`" && false; }; } &&
        set_link previous "${2:+releases/$2}" && set_link current "releases/$1"; then
        # The config that was replaced is gone from the disk before the journal that names it.
        [ "$tree_change" != replaced ] || [ -z "$old" ] || { rm -rf "$old" && flush "$(dirname "$live")"; }
        $SUDO rm -rf "$switch"
        return 0
    fi
    $SUDO rm -f "$lib/.current.new" "$lib/.previous.new" "$bin_dir/.impulse-shell.new" || true
    # The config goes back while `current` still names the switch, whose impulse-shell starts the
    # release that runs the config at each step of that; then the links.
    if { [ "$tree_change" != replaced ] || unswap_tree; } &&
        put_link_back previous "$was_prev" && put_link_back current "$was_cur"; then
        [ -z "$made_bin" ] || unlink_bin "$was_bin"
        $SUDO rm -rf "$switch" "$switch.new" ${3:+"$lib/releases/$3"}
        drop_prepared
        tree_change=
        fail "$why; $4"
        return 1
    fi
    fail "cannot switch the links in $lib, nor put back what was replaced: impulse-shell starts the release that runs the impulse-shell config as it is, and the next install or rollback finishes or undoes this one"
    return 1
}
put_link_back() {
    [ "$(readlink "$lib/$1" 2>/dev/null)" != "$2" ] || return 0
    set_link "$1" "$2"
}
# Undoes link_bin, if the name of the binary is the link an install made: the name is again
# the link to `$1` that it was, or with no `$1` is removed.
unlink_bin() {
    [ "$(readlink "$bin_dir/impulse-shell" 2>/dev/null)" = ../lib/impulse-shell/current/impulse-shell ] || return 0
    if [ -n "${1:-}" ]; then
        $SUDO ln -sfn "$1" "$bin_dir/.impulse-shell.new" && $SUDO mv -Tf "$bin_dir/.impulse-shell.new" "$bin_dir/impulse-shell"
    else
        $SUDO rm -f "$bin_dir/impulse-shell"
    fi
}
# Runs the pipeline `$@` (a function) while the lock is held, and returns its status. This
# shell does not start a pipeline itself: bash can run `interrupted` between two commands,
# also between the two of a pipeline it is starting, and then the handler is part of that
# pipeline (it waited for a `tar` that wrote into a pipe nobody read yet, and was ended by
# `set -e` on the status of one that had failed). A subshell runs the pipeline in the
# background and this shell waits for it with `wait`: a signal makes `wait` return at once,
# and then the handler runs (bash manual, Signals). The subshell ignores the three signals,
# and so do the commands it runs (Command Execution Environment): a pipeline that was
# started ends by itself, and what it wrote is undone after that. `$apart` is set before the
# subshell is started, so at every command the handler knows whether `$!` is such a subshell.
apart=
apart() {
    local rc=0
    apart=1
    (
        trap '' INT TERM HUP
        "$@"
    ) &
    wait $! || rc=$?
    apart=
    return $rc
}
# The pipelines: the tree `$1` is copied into the directory `$2`. A release holds the files
# that links lead to, not .git, and belongs to whoever owns $lib; the copy of a config that is
# kept holds no .git either.
release_copy() { tar -C "$1" --exclude=.git -chf - . | $SUDO tar -C "$2" --no-same-owner -xpf -; }
config_copy() { tar -C "$1" -cf - . | tar -C "$2" -xf -; }
kept_copy() { tar -C "$1" --exclude=.git -cf - . | tar -C "$2" -xpf -; }
# The tree a binary ran before it became a release (adopt_bin): a rollback puts that tree back
# as it was, so a link in it that leads to a file or a directory of the tree itself stays that
# link (the original's tree has icons that are links to other icons). A link that leads out of the tree
# or names an absolute path is copied as what it leads to, as release_copy does: a release
# holds all that it runs. Each link is put back only where the copy then reads the same.
adopted_copy() {
    local from to l t at links
    release_copy "$1" "$2" && from=$(realpath "$1") && to=$($SUDO realpath "$2") || return 1
    mapfile -d '' -t links < <(cd "$1" && find . -name .git -prune -o -type l -print0 | LC_ALL=C sort -z)
    for l in "${links[@]}"; do
        t=$(readlink "$1/$l") || return 1
        [[ $t != /* ]] || continue
        at=$(realpath -e "$1/$l" 2>/dev/null) && [[ $at == "$from"/* ]] || continue
        at=$($SUDO realpath -e "$2/$(dirname "$l")/$t" 2>/dev/null) && [[ $at == "$to"/* ]] || continue
        $SUDO diff -rq "$2/$l" "$at" >/dev/null 2>&1 || continue
        $SUDO rm -rf "$2/$l" && $SUDO ln -s -- "$t" "$2/$l" || return 1
    done
    return 0
}
# SIGINT, SIGTERM, SIGHUP while the lock is held: what this run switched is undone and what it
# made is removed. (A run whose last link was switched is complete: that stays.)
staged=
interrupted() {
    # The terminal may be gone (SIGHUP: it was closed), or the reader of a pipe (Ctrl-C ended
    # it too): what is printed from here on may fail, and the undo does not depend on it.
    trap '' INT TERM HUP PIPE
    # A pipeline that `apart` started may still run and write: the undo starts when it has
    # ended. (Nothing is sent to it: this shell knows the subshell, not the pipeline's
    # commands, and one that runs under $SUDO takes no signal from this user.) One that
    # `apart` has just waited for is no child any more, which bash 5.3 says: not an error.
    [ -z "$apart" ] || [ -z "${!:-}" ] || wait "$!" 2>/dev/null || :
    echo "install.sh: interrupted" >&2 2>/dev/null || exec 2>/dev/null
    recover undo >&2
    # Replaced and still so: the switch was complete. Otherwise what was prepared for it goes,
    # also a copy that was being made when the signal came.
    if [ "$tree_change" != replaced ]; then
        drop_prepared
    elif [ ! -e "$config_tree" ]; then
        unmake_dirs
    fi
    [ -z "$staged" ] || [ "$(readlink "$lib/current" 2>/dev/null)" = "releases/$staged" ] ||
        $SUDO rm -rf "$lib/releases/$staged"
    exit 1
}
# Whether an install, a rollback or an uninstall is running on $lib: it holds the lock.
at_work() { [ -e "$lib/.lock" ] && ! (exec 9<"$lib/.lock" && flock -n 9) 2>/dev/null; }
# One install, rollback or uninstall at a time; the lock goes with the process.
lock() {
    in_prefix mkdir -p "$lib/releases"
    [ -e "$lib/.lock" ] || in_prefix touch "$lib/.lock"
    exec 9<"$lib/.lock"
    flock -n 9 || die "another install, rollback or uninstall is running on $lib"
    trap interrupted INT TERM HUP
    recover
}
# Copies the binary `$1` and the tree `$2` into the new release `$3`, which appears complete
# or not at all. Files reached through links are copied as files (a config of links into a
# dotfiles directory), .git is left out, and the copy belongs to whoever owns $lib. Everyone
# can read it, whatever the modes of the files it was copied from (a checkout made with umask
# 077): the user who runs impulse-shell is not always its owner. `$4`: a file to mark the release
# with; `$5`: the pipeline that copies the tree, if not release_copy.
store() {
    local stage=$lib/releases/.staging
    $SUDO rm -rf "$stage"
    in_prefix mkdir -p "$stage/tree"
    $SUDO install -m755 "$1" "$stage/impulse-shell"
    apart "${5:-release_copy}" "$2" "$stage/tree" &&
        $SUDO chmod -R a+rX "$stage/tree" || {
        $SUDO rm -rf "$stage"
        die "cannot copy $2 (a link in it that leads nowhere?); nothing was installed"
    }
    [ -z "${4:-}" ] || in_prefix touch "$stage/$4"
    $SUDO mv -T "$stage" "$lib/releases/$3"
}
new_id() {
    local base id n=1
    base=${2:-}$(date -u +%Y%m%dT%H%M%SZ)-$(sha256sum "$1" | cut -c1-8)
    id=$base
    while [ -e "$lib/releases/$id" ]; do
        n=$((n + 1))
        id=$base.$n
    done
    echo "$id"
}

# Whether the name of the binary runs a binary that is not a release: one installed before
# releases existed (a plain file), or one built elsewhere that the name is a link to.
foreign_bin() {
    [ -f "$bin_dir/impulse-shell" ] && [ "$(readlink "$bin_dir/impulse-shell" 2>/dev/null)" != ../lib/impulse-shell/current/impulse-shell ]
}
# Under the lock: such a binary becomes the current release, so that what ran before an
# install or a rollback can be rolled back to, and is still there if a later step fails. It
# cannot say which tree it was compiled from; the config it has been running is stored, or
# the tree `$1` where there is no config, with its links to its own files as links
# (adopted_copy). $kept_bin is that binary as it was before a build.
# If it cannot be kept, the script ends with a message that ends in `$2`.
kept_bin=
adopt_bin() {
    local legacy running=$1 from=${kept_bin:-$bin_dir/impulse-shell} was_cur
    foreign_bin || return 0
    was_cur=$(readlink "$lib/current" 2>/dev/null) || was_cur=
    legacy=$(new_id "$from" legacy-)
    [ ! -f "$config_tree/shell.qml" ] || running=$config_tree
    store "$from" "$running" "$legacy" unverified adopted_copy
    # The name becomes the link to that copy now: from here on `current` says what runs. The
    # copy, then `current`, are on the disk before the name leads to them.
    flush "$lib/releases/$legacy"
    if point current "$legacy" && flush "$lib" && link_bin; then return 0; fi
    $SUDO rm -f "$lib/.current.new" "$bin_dir/.impulse-shell.new" || true
    put_link_back current "$was_cur" && $SUDO rm -rf "$lib/releases/$legacy" || true
    die "cannot keep $bin_dir/impulse-shell, which no install put there, as a release in $lib; $2"
}

install_release() {
    local bin=$1 tree=$2 cur prev keep id rc=0
    [ -f "$tree/shell.qml" ] || die "no QML tree at $tree (RI_ROOT names it); nothing was installed"
    vet "$bin"
    accepts "$bin" "$tree" >/dev/null || rc=$?
    case $rc in
    0) ;;
    1) die "$bin was not compiled from $tree; nothing was installed" ;;
    *) die "$bin --check-tree failed with exit status $rc; nothing was installed" ;;
    esac
    lock
    adopt_bin "$tree" "nothing was installed"
    cur=$(release current) || cur=
    prev=$(release previous) || prev=
    if [ -n "$cur" ] && [ ! -e "$lib/releases/$cur/unverified" ] &&
        cmp -s "$bin" "$lib/releases/$cur/impulse-shell" && same_tree "$tree" "$lib/releases/$cur/tree"; then
        replace_tree "$lib/releases/$cur/tree" "$lib/releases/$cur/tree" || exit 1
        if [ -n "$tree_change" ]; then
            switch_links "$cur" "$prev" "" "nothing changed" || exit 1
        else
            launches "$lib/releases/$cur/impulse-shell" >/dev/null ||
                die "$bin_dir/impulse-shell is this build ($cur) and would not start as \`impulse-shell -c impulse-shell\`; nothing changed"
        fi
        keep_tree
        link_bin
        drop_qs_link
        put_app_icon
        echo "$bin_dir/impulse-shell is already this build ($cur)"
        optional "$lib/releases/$cur/impulse-shell"
        return 0
    fi
    id=$(new_id "$bin")
    staged=$id
    store "$bin" "$tree" "$id"
    # The pair as stored, not as it was before the copy.
    accepts "$lib/releases/$id/impulse-shell" "$lib/releases/$id/tree" >/dev/null || {
        $SUDO rm -rf "$lib/releases/$id"
        die "the stored copy of $tree is not the tree the binary was compiled from (did it change during the install?); nothing was installed"
    }
    # The same binary with another tree (a file the build does not read) takes the current
    # release's place: the release to roll back to stays one with another binary.
    keep=$cur
    if [ -n "$cur" ] && cmp -s "$bin" "$lib/releases/$cur/impulse-shell"; then keep=$prev; fi
    # The config and the links. If either cannot be replaced, what was replaced is put back:
    # the installed release stays the current one, with the config it runs and the release to
    # roll back to.
    replace_tree "$lib/releases/$id/tree" "${cur:+$lib/releases/$cur/tree}" || {
        $SUDO rm -rf "$lib/releases/$id"
        exit 1
    }
    switch_links "$id" "$keep" "$id" "nothing was installed" || exit 1
    staged=
    keep_tree
    drop_qs_link
    put_app_icon
    prune "$id" "$keep"
    echo "installed $bin_dir/impulse-shell ($id)"
    [ "$keep" = "$cur" ] || echo "it replaces $cur, which has the same binary"
    [ -z "$keep" ] || echo "previous: $keep (tools/install.sh --rollback)"
    optional "$lib/releases/$id/impulse-shell"
}

# Makes the impulse-shell config a copy of the stored tree `$1`, in parts, so that an install or a
# rollback either happens whole or changes nothing. prepare_tree does all that can fail
# before the config is touched: the directories for a config where there is none, the copy of
# `$1` beside the config and, if the config was edited since it was the stored tree `$2` (or
# `$2` is empty: no release was installed), a copy of it in the state directory. swap_tree
# renames (inside a switch, see switch_links), unswap_tree undoes that, drop_prepared removes
# what prepare_tree made. The config's .git is not part of a release and stays with the
# config. The copy gets the stored files' modes less the user's umask: a release is readable
# by everyone, the user's config is as private as the user's other files.
# The copy of an edited config holds what the config's directory held: a link in it stays a
# link (the user's own files are not a release; one that leads nowhere is kept as well). It
# is made under the name keeping() gives and gets its name, $kept, when the config is
# replaced: replaced-* are the configs that were replaced, whenever a run is killed.
# $tree_change: empty, `prepared` (by prepare_tree) or `replaced` (by swap_tree).
tree_change=
new=
old=
kept=
kept_of=
made=
prepare_tree() {
    local parent why=", edited since its install,"
    kept_of=$2
    [ -n "$2" ] || why=
    new=
    old=
    kept=
    made=
    if [ ! -e "$config_tree" ]; then
        parent=$(dirname "$config_tree")
        if [ ! -e "$parent" ]; then
            made=$parent
            while [ ! -e "$(dirname "$made")" ]; do made=$(dirname "$made"); done
        fi
        mkdir -p "$parent" || {
            drop_prepared
            fail "cannot write in $parent; nothing changed"
            return 1
        }
    fi
    live=$(realpath "$config_tree") &&
        new=$(mktemp -d "$(dirname "$live")/.ri-restore.XXXXXX") || {
        drop_prepared
        fail "cannot write beside $config_tree; nothing changed"
        return 1
    }
    apart config_copy "$1" "$new" || {
        drop_prepared
        fail "cannot copy $1; nothing changed"
        return 1
    }
    [ -e "$live" ] || return 0
    old=$new.replaced
    if [ -n "$2" ] && same_tree "$live" "$2"; then return 0; fi
    if ! { mkdir -p "$state_dir" && kept=$(mktemp -d "$state_dir/.replaced-$(date -u +%Y%m%dT%H%M%SZ).XXXXXX") &&
        kept=$state_dir/${kept##*/.} &&
        apart kept_copy "$live" "$(keeping "$kept")"; }; then
        drop_prepared
        fail "cannot keep a copy of $live$why in $state_dir; nothing changed"
        return 1
    fi
}
# The name under which the copy `$1` of an edited config is made: --status does not count it.
keeping() { echo "${1%/*}/.${1##*/}"; }
drop_prepared() {
    rm -rf ${kept:+"$kept" "$(keeping "$kept")"} ${new:+"$new"}
    kept=
    new=
    unmake_dirs
}
# Removes the directories prepare_tree made for a config where there was none.
unmake_dirs() {
    local d
    [ -n "$made" ] || return 0
    d=$(dirname "$config_tree")
    while rmdir "$d" 2>/dev/null && [ "$d" != "$made" ]; do d=$(dirname "$d"); done
    made=
}
swap_tree() {
    [ -z "$old" ] || mv -T "$live" "$old" || return 1
    mv -T "$new" "$live" || {
        [ -z "$old" ] || mv -T "$old" "$live"
        return 1
    }
    tree_change=replaced
    [ -z "$old" ] || [ ! -e "$old/.git" ] || mv -T "$old/.git" "$live/.git" || return 1
    [ -z "$kept" ] || mv -T "$(keeping "$kept")" "$kept" || return 1
    # The config is replaced on the disk before a link names the release that runs it.
    flush "$(dirname "$live")" ${kept:+"$state_dir"}
}
unswap_tree() {
    [ -z "$old" ] || [ ! -e "$live/.git" ] || mv -T "$live/.git" "$old/.git"
    mv -T "$live" "$new" && { [ -z "$old" ] || mv -T "$old" "$live"; } || return 1
    tree_change=prepared
    [ -z "$kept" ] || [ ! -e "$kept" ] || mv -T "$kept" "$(keeping "$kept")"
    # And is back on the disk before the links are.
    flush "$(dirname "$live")"
}
# Prepares the impulse-shell config to become a copy of the stored tree `$1`, unless it is one; `$2` as
# for prepare_tree. switch_links then replaces it.
replace_tree() {
    tree_change=
    if [ -e "$config_tree" ] && same_tree "$config_tree" "$1"; then return 0; fi
    prepare_tree "$1" "$2" || return 1
    tree_change=prepared
}
# After a switch: says where an edited config was kept, and how many such copies there are:
# the script never removes them.
keep_tree() {
    [ -z "$kept" ] || say_kept "$live" "$kept" "$kept_of"
}
# The same for the config `$1` kept at `$2`; `$3` is empty if no install put that config
# there.
say_kept() {
    local copies note="had changes since its install"
    [ -n "$3" ] || note="was there before the first install"
    echo "$1 $note: kept at $2"
    copies=("$state_dir"/replaced-*)
    [ ${#copies[@]} -le 1 ] ||
        echo "$state_dir holds ${#copies[@]} such copies; they are yours to remove"
}

# Under the lock: whether the previous release was put back without this script, its binary
# copied to the name and its tree to the config (a watcher that rolls back by hand). It is
# that release, not a new one, so nothing is copied: the links are switched to say what
# runs, and the name is the link again. `previous` first: cut between two steps, the newer
# release `$1` stays the one a rollback goes to.
put_back_by_hand() {
    local prev
    foreign_bin || return 1
    prev=$(release previous) && [ "$prev" != "$1" ] || return 1
    cmp -s "$bin_dir/impulse-shell" "$lib/releases/$prev/impulse-shell" || return 1
    [ -f "$config_tree/shell.qml" ] && same_tree "$config_tree" "$lib/releases/$prev/tree" || return 1
    if point previous "$1" && point current "$prev" && flush "$lib" && link_bin; then return 0; fi
    $SUDO rm -f "$lib/.previous.new" "$lib/.current.new" "$bin_dir/.impulse-shell.new" || true
    put_link_back current "releases/$1" && put_link_back previous "releases/$prev" || true
    die "cannot switch the links in $lib to the release that $bin_dir/impulse-shell is a copy of; nothing changed"
}

# The build from before the rename is not a release of this directory: it stays installed under
# its old names, and no install or rollback here touches it. (The old names are put together
# from halves, so that tools/rename/rename.py, which renames them everywhere, leaves them here.)
old_names_note() {
    local bin=ii tree=ii word=quick was=rusty
    bin+=-rs word+=shell was+=-impulse
    # The shell under its previous name is the build before this one where it is installed.
    if [ -e "$bin_dir/$was" ] || [ -e "$PREFIX/lib/$was/current" ]; then
        bin=$was tree=$was word=rustshell
    elif [ ! -e "$bin_dir/$bin" ] && [ ! -e "$PREFIX/lib/$bin/current" ]; then
        return 0
    fi
    echo "; the build from before the rename is still installed under its old names ($bin_dir/$bin, $config_home/$word/$tree): this script does not switch to it. What starts it again is the rollback-now that tools/pc/switch filled in, in the build directory switch was run from (~/pcbuild/stable-<sha>/rollback-now); tools/pc/rollback-now of a checkout has no rollback copies and does nothing"
}

# `$1`: previous (the default) or good, the link that names the release to switch to.
rollback() {
    local cur prev target restore= to=${1:-previous}
    as_the_user
    [ -d "$lib/releases" ] || die "nothing is installed under $lib"
    lock
    cur=$(release current) || die "nothing is installed under $lib"
    # A binary put at the name since the last install is what a second rollback goes back to;
    # a copy of the previous release there is that release, and the rollback leaves it.
    put_back_by_hand "$cur" ||
        adopt_bin "$lib/releases/$cur/tree" "nothing changed"
    cur=$(release current)
    prev=$(release "$to") || prev=$cur
    if [ "$prev" = "$cur" ]; then
        [ "$to" = previous ] || die "no release marked good other than the current one ($cur) to roll back to"
        die "no previous release to roll back to$(old_names_note)"
    fi
    target=$lib/releases/$prev
    if [ ! -f "$config_tree/shell.qml" ]; then
        # No impulse-shell config for the older binary to run: it gets its release's, as from an install.
        restore=1
    # The newer release changed the tree: undo that as well. Otherwise the config stays as it
    # is when the older binary runs it.
    elif ! same_tree "$lib/releases/$cur/tree" "$target/tree"; then
        restore=1
    elif [ -e "$target/unverified" ]; then
        same_tree "$config_tree" "$target/tree" || restore=1
    elif ! accepts "$target/impulse-shell" "$config_tree" >/dev/null 2>&1; then
        restore=1
    fi
    if [ -n "$restore" ]; then
        [ -e "$target/unverified" ] || accepts "$target/impulse-shell" "$target/tree" >/dev/null 2>&1 ||
            die "the tree stored with $prev is not the one its binary was compiled from; nothing changed"
        replace_tree "$target/tree" "$lib/releases/$cur/tree" || exit 1
    fi
    switch_links "$prev" "$cur" "" "nothing changed" || exit 1
    # The release that was the previous one is neither any more.
    [ "$to" = previous ] || prune "$prev" "$cur"
    echo "rolled back $bin_dir/impulse-shell to $prev (was $cur)"
    if [ "$tree_change" = replaced ]; then
        if [ -n "$old" ]; then
            echo "restored $config_tree from that release"
        else
            echo "made $config_tree from that release"
        fi
    fi
    keep_tree
}

# Marks the current release as the one that has proved good: it stays until another is marked.
# With SHA256 set, only if the current release's binary has that sha256: whoever saw a shell
# run names the binary it saw, and an install made since then is not marked in its place.
mark_good() {
    local cur prev sum
    as_the_user
    [ -d "$lib/releases" ] || die "nothing is installed under $lib"
    lock
    ! foreign_bin || die "$bin_dir/impulse-shell is not a release of $lib (no install put it there); nothing was marked"
    cur=$(release current) || die "nothing is installed under $lib"
    if [ -n "${SHA256:-}" ]; then
        sum=$(sha256sum <"$lib/releases/$cur/impulse-shell" | cut -d' ' -f1)
        [ "$sum" = "$SHA256" ] ||
            die "the current release $cur has the sha256 $sum, not $SHA256; nothing was marked"
    fi
    prev=$(release previous) || prev=
    point good "$cur" && flush "$lib" || die "cannot write the link $lib/good; nothing was marked"
    prune "$cur" "$prev"
    echo "marked $cur as the release that proved good (tools/install.sh --rollback-good)"
}

status() {
    local cur prev copies
    as_the_user
    if switching; then
        if at_work; then
            echo "an install or a rollback is switching releases now: an impulse-shell started meanwhile waits for it"
        else
            echo "an install or a rollback was interrupted: impulse-shell starts the release that runs the impulse-shell config as it is, and the next install or rollback finishes or undoes it"
        fi
        return 0
    fi
    cur=$(release current) || die "nothing is installed under $lib"
    prev=$(release previous) || prev=none
    echo "current:  $cur"
    echo "previous: $prev"
    ! release good >/dev/null || echo "good:     $(release good)"
    [ -z "$(old_names_note)" ] || echo "before the rename:$(old_names_note | cut -c2-)"
    copies=("$state_dir"/replaced-*)
    [ ! -e "${copies[0]}" ] ||
        echo "replaced impulse-shell configs: ${#copies[@]} kept in $state_dir (yours to remove)"
    if [ ! -f "$config_tree/shell.qml" ]; then
        echo "impulse-shell config: none at $config_tree"
        return 0
    fi
    if same_tree "$config_tree" "$lib/releases/$cur/tree"; then
        echo "impulse-shell config: $config_tree is the tree stored with $cur"
    else
        echo "impulse-shell config: $config_tree differs from the tree stored with $cur:"
        {
            diff -rq -x .git "$lib/releases/$cur/tree/." "$config_tree/." 2>&1 || true
            other_modes "$lib/releases/$cur/tree" "$config_tree" 2>&1 |
                sed 's/^/Executable in only one of the two: /'
        } | head -n 20 | sed 's/^/  /' || true
    fi
    [ -e "$lib/releases/$cur/unverified" ] || launches "$lib/releases/$cur/impulse-shell"
}

# Edit mode's shortcut, bound as end-4's dots bind the shell: Hyprland dispatches the shell's
# global shortcut (hyprland-global-shortcuts-v1, `editModeToggle` in EditMode.qml), so no
# command is looked up in PATH. It goes once into the user's own keybinds file (the dots'
# custom/keybinds.lua, which their updates leave alone); a bind of the shortcut the user
# already has, under any keys, is kept as it is. Hyprland reloads its config by itself.
hypr_dir=${XDG_CONFIG_HOME:-$HOME/.config}/hypr
user_binds=$hypr_dir/custom/keybinds.lua
edit_bind='hl.bind("SUPER + ALT + E", hl.dsp.global("rustshell:editModeToggle"), { description = "Shell: Toggle edit mode" })'
add_edit_bind() {
    if [ ! -f "$hypr_dir/hyprland.lua" ]; then
        echo "no Lua Hyprland config in $hypr_dir: bind edit mode yourself to hl.dsp.global(\"rustshell:editModeToggle\")"
        return
    fi
    if grep -rqsF --include='*.lua' 'rustshell:editModeToggle' "$hypr_dir"; then
        return
    fi
    mkdir -p "${user_binds%/*}"
    # A file whose last line has no newline gets one first: the bind is a line of its own.
    if [ -s "$user_binds" ] && [ -n "$(tail -c1 "$user_binds")" ]; then echo >> "$user_binds"; fi
    printf '%s\n' "$edit_bind" >> "$user_binds"
    echo "bound Super+Alt+E to edit mode in $user_binds"
}
drop_edit_bind() {
    [ -f "$user_binds" ] && grep -qxF "$edit_bind" "$user_binds" || return 0
    grep -vxF "$edit_bind" "$user_binds" > "$user_binds.new" || true
    mv -f "$user_binds.new" "$user_binds"
}

uninstall() {
    local name=$bin_dir/impulse-shell ours=
    as_the_user
    [ "$(readlink "$name" 2>/dev/null)" != ../lib/impulse-shell/current/impulse-shell ] || ours=1
    # Only the link an install made is the script's to remove.
    if [ ! -d "$lib" ] && [ -z "$ours" ]; then
        [ ! -e "$name" ] && [ ! -L "$name" ] ||
            die "nothing is installed under $PREFIX: $name is not the link an install makes (it has another origin) and stays"
        die "nothing is installed under $PREFIX"
    fi
    [ ! -d "$lib" ] || lock
    drop_qs_link
    unlink_bin
    $SUDO rm -rf "$lib"
    drop_app_icon
    drop_edit_bind
    if [ -n "$ours" ]; then
        echo "removed $name and $lib"
    else
        echo "removed $lib"
        [ ! -e "$name" ] && [ ! -L "$name" ] ||
            echo "left: $name, which is not the link an install makes (it has another origin)"
    fi
    [ ! -e "$config_tree" ] ||
        echo "left: the impulse-shell config $config_tree, the tree impulse-shell ran (yours to remove)"
    if compgen -G "$state_dir/replaced-*" >/dev/null; then
        echo "kept: the impulse-shell configs that installs and rollbacks replaced, in $state_dir (yours to remove)"
    fi
}

[ $# -le 1 ] || die "unexpected argument '$2' (tools/install.sh --help)"
case ${1:-} in
-h | --help)
    usage
    exit 0
    ;;
--uninstall)
    uninstall
    exit 0
    ;;
--rollback)
    rollback
    exit 0
    ;;
--rollback-good)
    rollback good
    exit 0
    ;;
--mark-good)
    mark_good
    exit 0
    ;;
--status)
    status
    exit
    ;;
"") ;;
*) die "unknown argument '$1' (tools/install.sh --help)" ;;
esac

as_the_user
repo=$(cd "$(dirname "$0")/.." && pwd)
RI_ROOT=${RI_ROOT:-$repo/qml}
RI_ROOT=$(cd "$RI_ROOT" 2>/dev/null && pwd) || die "no directory RI_ROOT=$RI_ROOT; nothing was installed"
export RI_ROOT
need() { command -v "$1" >/dev/null || die "$1 is needed $2; nothing was installed"; }
# Ask cargo where the binary is (CARGO_TARGET_DIR or build.target-dir move it): the build's
# `executable` artifact message, or for NOBUILD the workspace's target directory.
if [ -n "${BIN:-}" ]; then
    bin=$BIN
elif [ -z "${NOBUILD:-}" ]; then
    need cargo "to build"
    need jq "to read where cargo put the binary"
    # The name of the binary may be a link to the file this build writes (a build of this
    # checkout that was run in place): the binary as it is now is what adopt_bin keeps.
    if foreign_bin; then
        kept_bin=$(mktemp) && cp "$bin_dir/impulse-shell" "$kept_bin" ||
            die "cannot keep a copy of $bin_dir/impulse-shell, which no install put there; nothing was installed"
        trap 'rm -f "$kept_bin"' EXIT
    fi
    # tools/build.sh builds the shipped binary: it names its commit and the checksum of its
    # QML tree, and is the same file wherever this checkout is; without a commit to name, or
    # with one of this machine's directories in the binary, it fails and nothing is installed.
    bin=$("$repo/tools/build.sh") || die "the build failed; nothing was installed"
else
    need cargo "to find the build"
    need jq "to read where cargo put the build"
    # Cargo Book, Build cache: release/ of the target directory holds a build for the host,
    # <target>/release/ one for a target that was named (--target, build.target). Which of
    # them a build would make now is in Cargo's configuration, which the script cannot read:
    # the one that is there is installed, and of several none.
    target_dir=$(cd "$repo" && cargo metadata --locked --format-version 1 --no-deps | jq -r .target_directory)
    builds=()
    for bin in "$target_dir"/release/impulse-shell "$target_dir"/*/release/impulse-shell; do
        [ ! -x "$bin" ] || builds+=("$bin")
    done
    [ ${#builds[@]} -le 1 ] ||
        die "more than one build of impulse-shell under $target_dir: ${builds[*]}; name the one to install with BIN=<file>; nothing was installed"
    bin=${builds[0]:-$target_dir/release/impulse-shell}
fi
# A file is named by its path, also one in the current directory: it is not looked for in
# $PATH.
case $bin in /*) ;; *) bin=$PWD/$bin ;; esac
[ -x "$bin" ] || die "no impulse-shell binary at '$bin'"
install_release "$bin" "$RI_ROOT"
add_edit_bind
