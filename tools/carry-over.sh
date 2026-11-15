#!/usr/bin/env bash
# Copies what a user keeps under the old names (illogical-impulse, ii, ii-rs, quickshell) to the
# new one: impulse-shell, the one name of the shell's directories (its settings in ~/.config,
# its tree in ~/.local/share, its state in ~/.local/state, its cache in ~/.cache). Nothing
# under an old name is changed or removed, so the old build still finds everything it reads:
# going back to it loses nothing.
#
# A home that ran the shell under its previous name (its tree is ~/.config/rustshell/<that
# name>) is carried over from that build instead: its settings, cache, icon and keyring item,
# and the user's own files that name it. Its state and its caches (~/.local/state/rustshell,
# ~/.cache/rustshell) are one place the two builds share: tools/install.sh moves them to the
# new name and leaves the old one as a link for that build, nothing is copied there.
#
#   tools/carry-over.sh              copy the data: settings, state, caches, the icon, the keyring
#                                    item. What is already there under a new name is left alone
#                                    (a keyring item that is there gets what it lacks, see below)
#   tools/carry-over.sh --force      replace what is there under a new name with the old data;
#                                    what was there is kept beside it as <name>.before-carry-over-*
#   tools/carry-over.sh --dots new   make the user's own files (Hyprland, matugen, fish, kitty) name the
#                                    new build; each file's old text is kept, see --dots old.
#                                    The desktop widgets' layout is kept as it is at that moment
#   tools/carry-over.sh --dots old   put those files back as they were for the old build, the
#                                    layout too; a layout the new build saved stays beside it
#   tools/carry-over.sh --keyring    the keyring item only: what a run with the keyring locked left
#                                    uncopied. Exit status 5: the item under the new name was written
#   tools/carry-over.sh --status     what is under which name
#   -n, --dry-run                    with any of these: say what would be done, do nothing
#   -q, --quiet                      say only what was done or went wrong
#
# Exit status: 0 done or nothing to do, 1 failed, 3 something was left uncopied (it says what),
# 4 the keyring item could not be copied though the keyring is open ("keyring: FAILED" says why;
# everything else was done).
set -euo pipefail
shopt -s nullglob dotglob

# The names. WORD is what replaces "quickshell" in the names that are this project's own and
# no directory: the layers' prefix, the global shortcuts'. (Before the shell's directories had
# the one name it was their name too: what a text names so is rewritten, see rewrite.)
OLD_APP=illogical-impulse NEW_APP=impulse-shell
OLD_BIN=ii-rs NEW_BIN=impulse-shell
OLD_TREE=ii NEW_TREE=impulse-shell
OLD_WORD=quickshell NEW_WORD=${RI_WORD:-rustshell}

xdg() { local v=${!1-}; [[ $v == /* ]] || v=$HOME/$2; echo "$v"; }
config=$(xdg XDG_CONFIG_HOME .config)
state=$(xdg XDG_STATE_HOME .local/state)
cache=$(xdg XDG_CACHE_HOME .cache)
data=$(xdg XDG_DATA_HOME .local/share)
# The build this home ran last: the shell under its previous name where its tree is there,
# else the one from before both renames, named above. (The previous name is put together from
# halves: tools/rename/rename.py renames it wherever it stands whole.)
was=rusty
was+=-impulse
if [ -e "$config/$NEW_WORD/$was" ]; then
    OLD_APP=$was OLD_BIN=$was OLD_TREE=$was OLD_WORD=$NEW_WORD
    # Its start script, and the prefix of its variables in the user's environment.
    OLD_START=$was-shell OLD_ENV=RUSTY
    OLD_ENV+=_IMPULSE_
fi
runtime=${XDG_RUNTIME_DIR-}
[[ $runtime == /* ]] || runtime=/run/user/$(id -u)

own=$state/$NEW_BIN/carry-over
stamp=$(date +%Y%m%d-%H%M%S)
dry= quiet= force= left=0 failed=0

die() { echo "carry-over.sh: $*" >&2; exit 1; }
say() { [ -n "$quiet" ] || echo "$*"; }
did() { echo "$*"; }
leave() { echo "carry-over.sh: $*" >&2; left=3; }
note() { [ -n "$dry" ] || { mkdir -p "$own" && echo "$(date -Is) $*" >>"$own/log"; }; }
usage() { awk 'NR > 1 && /^#/ { sub(/^# ?/, ""); print; next } NR > 1 { exit }' "$0"; }

# The pairs of data: old path, new path.
pairs() {
    printf '%s\n' \
        "$config/$OLD_APP" "$config/$NEW_APP" \
        "$cache/$OLD_BIN" "$cache/$NEW_BIN" \
        "$data/icons/$OLD_APP.svg" "$data/icons/$NEW_APP.svg"
    # The state and the cache the previous name's build has are this build's own (see above).
    [ "$OLD_WORD" = "$NEW_WORD" ] || printf '%s\n' \
        "$state/$OLD_WORD" "$state/$NEW_BIN" \
        "$cache/$OLD_WORD" "$cache/$NEW_BIN"
}
# What a run from before the one name wrote into the log for the pair that ends in `$1`: the
# state and the cache went under the engine's word then, and tools/install.sh has moved them.
logged_as() {
    case $1 in "$state/$NEW_BIN" | "$cache/$NEW_BIN") echo "${1%/*}/$NEW_WORD" ;; *) echo "$1" ;; esac
}
# Makes a text that names the shell's directories as they were before the one name (the tree
# in ~/.config/WORD/impulse-shell, the state and the cache under WORD), the app id org.WORD or
# /tmp/WORD name them as they are: what the rewrite below has made, and what one from before
# the one name made (the texts this script keeps for --dots new).
one_name() {
    local W=$NEW_WORD A=$NEW_BIN e='([^[:alnum:]_.-]|$)' home='XDG_DATA_HOME\1'
    [ "$data" != "$HOME/.local/share" ] || home='HOME\1/.local/share'
    sed -E \
        -e "s#\\\$\\{XDG_CONFIG_HOME:-(\\\$HOME|\\\$\\{HOME\\}|~)/\\.config\\}/$W/$A$e#\${XDG_DATA_HOME:-\\1/.local/share}/$A\\2#g" \
        -e "s#XDG_CONFIG_HOME(\\}?[\"']?)/$W/$A$e#$home/$A\\2#g" \
        -e "s#\\.config/$W/$A$e#.local/share/$A\\1#g" \
        -e "s#\\.config/$W/(\\\$\\{?qsConfig\\}?)#.local/share/\\1#g" \
        -e "s#((\\.local/state|\\.cache|XDG_STATE_HOME|XDG_CACHE_HOME)\\}?[\"']?/)$W$e#\\1$A\\3#g" \
        -e "s#/tmp/$W$e#/tmp/$A\\1#g" \
        -e "s#org(\\\\*)\\.$W([^[:alnum:]_-]|\$)#org\\1.$A\\2#g"
}

# What rewrites a text that names the old build into one that names the new build. A file is
# rewritten only where a whole name stands: "ii" alone is a word in too many other places.
#
# The old build answered Quickshell's own client: "qs -c ii ipc call ..." reached it, and
# "qs -p <its tree>/settings.qml" opened its windows. The new build has a socket and a tree of
# its own that qs does not know, so where a line calls qs for this shell (its config by name or
# by \$qsConfig, or a file of its tree) the command becomes the new build's own binary: by its
# full path under ~/.local/bin when it is installed there (that directory is not on Hyprland's
# PATH), else by name. An alias for "qs -c ii" (fish: alias q 'qs -c ii', used as "q ipc call
# ...") becomes the new build's too. Any other "qs -c ii" starts a shell and stays as it is:
# that is the original.
#
# "qs -c \$qsConfig" without "ipc" starts the shell the variable names (end-4's execs.lua at
# login, and what Ctrl+Super+R runs after its killall). The variable is rewritten to the new
# build's tree, which qs cannot start, so that command becomes the new build's binary too. A
# killall or pidof that lists "qs quickshell" (the restart binding, hypridle's "is a shell
# there to lock the screen") gets the new build's name after them: its process is neither.
rewrite() {
    local client=$NEW_BIN ours="($OLD_TREE|\\\$\\{?qsConfig\\}?)"
    [ ! -x "$HOME/.local/bin/$NEW_BIN" ] || client="\$HOME/.local/bin/$NEW_BIN"
    sed -E \
        -e "s#(^|[^[:alnum:]_./-])qs( +-c +[\"']?$ours[\"']? +ipc([^[:alnum:]_-]|\$))#\\1$client\\2#g" \
        -e "s#(^|[^[:alnum:]_./-])qs( +(-p|--path) +[^ \"']*$OLD_WORD/$ours/)#\\1$client\\2#g" \
        -e "s#(^|[^[:alnum:]_./-])qs( +-c +[\"']?\\\$\\{?qsConfig\\}?[\"']?([^[:alnum:]_\"'-]|\$))#\\1$client\\2#g" \
        -e "s#((^|[^[:alnum:]_./-])(pidof|killall)( +[^;&|]*)? +qs +quickshell)( +$NEW_BIN)?([^[:alnum:]_.-]|\$)#\\1 $NEW_BIN\\6#g" \
        -e "/(^|[^[:alnum:]_-])alias[ =]/s#(^|[^[:alnum:]_./-])qs( +-c +[\"']?$OLD_TREE[\"']?([^[:alnum:]_-]|\$))#\\1$client\\2#g" \
        -e "s#(^|[^[:alnum:]_./-])(qs +-c +[\"']?)$OLD_TREE([\"']?)([^[:alnum:]_\"'-]|\$)#\\1\\2\\x01\\3\\4#g" \
        -e "s#$OLD_WORD:#$NEW_WORD:#g" \
        -e "s#org(\\\\*)\\.$OLD_WORD#org\\1.$NEW_WORD#g" \
        -e "s#(^|[^[:alnum:]_.-])$OLD_WORD/$OLD_TREE([^[:alnum:]_-]|\$)#\\1$NEW_WORD/$NEW_TREE\\2#g" \
        -e "s#((\\.config|\\.local/state|\\.cache|_HOME\\}?|_home|_dir|Home|Dir)[\"' .]*/)$OLD_WORD([^[:alnum:]_-]|\$)#\\1$NEW_WORD\\3#g" \
        -e "s#/tmp/$OLD_WORD([^[:alnum:]_-]|\$)#/tmp/$NEW_WORD\\1#g" \
        -e "s#$OLD_APP#$NEW_APP#g" \
        -e "s#ILLOGICAL_IMPULSE_#IMPULSE_SHELL_#g" \
        -e "s#(^|[^[:alnum:]_.-])$OLD_BIN([^[:alnum:]_.-]|\\.log|\$)#\\1$NEW_BIN\\2#g" \
        -e "s#(^|[^[:alnum:]_.-])ii-shell([^[:alnum:]_.-]|\$)#\\1$NEW_BIN-start\\2#g" \
        -e "s#hl-virtual-keyboard-$OLD_BIN#hl-virtual-keyboard-$NEW_BIN#g" \
        -e "s#__ii_superIntr#__ri_superIntr#g" \
        -e "s#(-c[ \"',]+|qsConfig[\"']?[ ]*[=,][ ]*[\"'])$OLD_TREE([^[:alnum:]_-]|\$)#\\1$NEW_TREE\\2#g" \
        -e "s#\\x01#$OLD_TREE#g" |
        one_name
}
# From the previous name the engine's word stays, so only whole names change. The start script
# goes first: its new name is not the old one with the new name put in.
if [ "$OLD_WORD" = "$NEW_WORD" ]; then
    rewrite() {
        sed -E \
            -e "s#(^|[^[:alnum:]_.-])$OLD_START([^[:alnum:]_.-]|\$)#\\1$NEW_BIN-start\\2#g" \
            -e "s#$OLD_APP#$NEW_APP#g" \
            -e "s#$OLD_ENV#IMPULSE_SHELL_#g" |
            one_name
    }
fi
names_old() { ! rewrite <"$1" | cmp -s - "$1"; }
# A file something reads: by its suffix, or a script without one. Copies kept beside the files
# (execs.lua.pre-native-default, hypridle.conf.new, *.bak) are not rewritten.
is_read() {
    case ${1##*/} in
        *.lua | *.conf | *.toml | *.fish | *.sh | *.py | *.css | *.scss | *.json | *.jsonc | *.ini | *.kdl | *.rasi | \
            *.yaml | *.yml | *.txt | *.colors | *.theme | *.qss | *.xml | *.desktop | *.service) return 0 ;;
        *.*) return 1 ;;
        *) return 0 ;;
    esac
}
is_text() { [ -f "$1" ] && [ ! -L "$1" ] && { [ ! -s "$1" ] || grep -qI . "$1"; }; }

# Rewrites a file in place through a temporary file beside it, keeping its mode and owner.
rewrite_file() {
    local f=$1 tmp
    tmp=$(mktemp "${f%/*}/.carry-over.XXXXXX")
    cp -p -- "$f" "$tmp" && rewrite <"$f" >"$tmp" && mv -f -- "$tmp" "$f" || { rm -f -- "$tmp"; return 1; }
}

# Both folders are there (the new one was made before the data was carried over): what the new
# one lacks is copied in, nothing in it is replaced.
add_missing() {
    local old=$1 new=$2 rel n=0 kept=0
    while IFS= read -r -d '' rel; do
        rel=${rel#./}
        if [ -e "$new/$rel" ] || [ -L "$new/$rel" ]; then
            kept=$((kept + 1))
            continue
        fi
        n=$((n + 1))
        [ -z "$dry" ] || continue
        mkdir -p -- "$new/${rel%"${rel##*/}"}"
        cp -a -- "$old/$rel" "$new/$rel" || die "cannot copy $old/$rel"
        if [ "$old" = "$config/$OLD_APP" ] && is_text "$new/$rel" && names_old "$new/$rel"; then
            rewrite_file "$new/$rel" || die "cannot rewrite $new/$rel"
            did "  $rel: old names replaced in the copy"
        fi
    done < <(cd -- "$old" && find . \( -type f -o -type l -o -type d -empty \) -print0)
    if [ -n "$dry" ]; then
        did "would add $n files of $old to $new, which is there already ($kept it has are kept as they are)"
        return 0
    fi
    note "added $old -> $new"
    did "added $n files of $old to $new, which was there already ($kept it had are kept as they are)"
}

copy_pair() {
    local old=$1 new=$2 tmp aside f
    [ "$old" != "$new" ] || return 0
    [ -e "$old" ] || [ -L "$old" ] || { say "nothing under $old"; return 0; }
    if [ -e "$new" ] || [ -L "$new" ]; then
        if [ -z "$force" ]; then
            if [ -f "$own/log" ] && grep -qF -e " copied $old -> $new" -e " added $old -> $new" \
                -e " copied $old -> $(logged_as "$new")" -e " added $old -> $(logged_as "$new")" "$own/log"; then
                say "already carried over: $new"
            elif [ -d "$old" ] && [ ! -L "$old" ] && [ -d "$new" ] && [ ! -L "$new" ]; then
                add_missing "$old" "$new"
            else
                leave "$new is there already and was not made from $old: left as it is (--force replaces it with the old data and keeps it beside)"
            fi
            return 0
        fi
        aside=$new.before-carry-over-$stamp
    else
        aside=
    fi
    if [ -n "$dry" ]; then
        did "would copy $old -> $new${aside:+ (keeping what is there as $aside)}"
        return 0
    fi
    mkdir -p -- "${new%/*}"
    tmp=$(mktemp -d "${new%/*}/.carry-over.XXXXXX")
    cp -a -- "$old" "$tmp/it" || { rm -rf -- "$tmp"; die "cannot copy $old"; }
    # The user's settings name paths and commands of the old build: in the copy they name the new.
    if [ "$old" = "$config/$OLD_APP" ]; then
        while IFS= read -r -d '' f; do
            is_text "$f" && names_old "$f" || continue
            rewrite_file "$f" || { rm -rf -- "$tmp"; die "cannot rewrite ${f#"$tmp/it"} of $old"; }
            did "  ${f#"$tmp/it/"}: old names replaced in the copy"
        done < <(find "$tmp/it" -type f -print0)
    fi
    [ -z "$aside" ] || mv -T -- "$new" "$aside"
    mv -T -- "$tmp/it" "$new" || { [ -z "$aside" ] || mv -T -- "$aside" "$new"; rm -rf -- "$tmp"; die "cannot put $new in place"; }
    rmdir -- "$tmp"
    note "copied $old -> $new"
    did "copied $old -> $new${aside:+ (what was there: $aside)}"
}

# The keyring item with the API keys: looked up by application=<name>. The copy keeps the item's
# other attributes and its label; the secrets go through files only their owner can read, on tmpfs.
#
# Nobody is asked for the keyring's password: whether it is locked is read from the bus, and a
# locked keyring is left alone (a lookup would put its password prompt on screen and wait for as
# long as that stays there; tools/pc/switch has stopped the old shell by then). What could not be
# copied is written down in $pending and copied by the next run that finds the keyring open:
# "carry-over.sh --keyring", which tools/pc/rollback-watch makes for as long as that file is
# there. The end of it goes as one line to stdout and to the log named in CARRY_LOG at the time.
#
# The new build writes an item of its own, {}, the first time it reads an open keyring and finds
# none. So an item that is already there is completed, not skipped: what the old item has and
# the new one lacks is added, nothing in the new one is replaced.
#
# secret-tool reads the label and the attributes from its arguments and prints them in the
# charset of its locale. Under LC_ALL=C (tools/pc/switch and rollback-watch run this so) a label
# or an attribute that is not ASCII comes out of "search" as question marks or is refused by
# "store" ("Invalid byte sequence in conversion input"), and the item is not copied. So
# secret-tool runs under a UTF-8 locale whatever the caller's is: C.UTF-8, else another one the
# system has. CHARSET is what GLib asks before the locale: the arguments pass as UTF-8 even
# where no such locale is found. LANGUAGE=C: its messages stay in English, kr_is_locked reads
# one of them.
KEYRING_WAIT=${KEYRING_WAIT:-15}
pending=$own/keyring-pending
wrote=
kr_locale=
for l in C.UTF-8 en_US.UTF-8 $(locale -a 2>/dev/null | grep -i -E '\.utf-?8$' || true); do
    [ "$(LC_ALL=$l locale charmap 2>/dev/null)" = UTF-8 ] || continue
    kr_locale=$l
    break
done
st() { LC_ALL=${kr_locale:-C} LANGUAGE=C CHARSET=UTF-8 timeout "$KEYRING_WAIT" secret-tool "$@"; }
# $1 how the keyring step ended, $2 non-empty: the item under the new name was written.
kr_done() {
    local log
    echo "keyring: $1"
    [ -z "$dry" ] || return 0
    [ -z "${2-}" ] || wrote=1
    if [ ! -f "$pending" ]; then
        [ -z "${2-}" ] || note "keyring: $1"
        return 0
    fi
    log=$(sed -n 's/^log=//p' "$pending")
    [ -z "$log" ] || [ ! -f "$log" ] || echo "$(date '+%F %T') keyring, after the keyring was opened: $1" >>"$log" || true
    note "keyring, after the keyring was opened: $1"
    rm -f -- "$pending"
}
# Writes down that the item is not copied, for the next run and for where its end is logged.
kr_pending() {
    local log=${CARRY_LOG-} since
    [ -z "$dry" ] || return 0
    [ -n "$log" ] || log=$(sed -n 's/^log=//p' "$pending" 2>/dev/null) || true
    since=$(sed -n 's/^since=//p' "$pending" 2>/dev/null) || true
    mkdir -p "$own" && printf 'since=%s\nlog=%s\n' "${since:-$(date -Is)}" "$log" >"$pending"
}
# $1 why the item is not copied yet: something to wait for (a locked keyring, no answer).
kr_wait() {
    echo "carry-over.sh: keyring: $1" >&2
    left=3
    kr_pending
}
# $1 what went wrong with an open keyring. Waiting does not mend that, so it is an error and
# the exit status says so; the next run still tries again.
kr_fail() {
    echo "carry-over.sh: keyring: FAILED, the API keys did NOT move to application=$NEW_APP: $1. The item application=$OLD_APP is as it was" >&2
    failed=4
    kr_pending
}
# secret-tool search without the secret itself. Status 124: no answer. A locked keyring answers
# it without asking for its password and names the item by a hashed attribute only: that is how
# "locked" is told. Its Locked property is not read: one busctl get-property in some dozens
# makes gnome-keyring-daemon abort (it answers a caller it has not registered yet).
kr_search() { st search application "$1" 2>&1 | grep -v '^secret = '; return "${PIPESTATUS[0]}"; }
kr_is_locked() { grep -q -e 'locked object' -e '^attribute\.gkr:compat:hashed:application = ' <<<"$1"; }
copy_keyring() {
    local found tmp rc
    command -v secret-tool >/dev/null || { kr_done "no secret-tool here: the keyring was not looked at"; return 0; }
    rc=0
    found=$(kr_search "$OLD_APP") || rc=$?
    if [ "$rc" = 124 ]; then
        kr_wait "NOT COPIED YET: the keyring did not answer in $KEYRING_WAIT s; the item application=$OLD_APP is copied by the next run that finds it open"
        return 0
    fi
    if ! kr_is_locked "$found" && ! grep -q '^attribute\.application = ' <<<"$found"; then
        if [ -S "$runtime/bus" ] || [ -n "${DBUS_SESSION_BUS_ADDRESS-}" ]; then
            kr_done "nothing under application=$OLD_APP: there is nothing to copy"
        else
            kr_wait "NOT COPIED YET: no session bus here, the item application=$OLD_APP was not looked for"
        fi
        return 0
    fi
    if kr_is_locked "$found"; then
        kr_wait "NOT COPIED YET: the keyring is locked. The item application=$OLD_APP stays where it is; the new build has no API keys until the keyring is opened (its password at the lock screen, or at login) and the next run of this copies the item"
        return 0
    fi
    [ -d "$runtime" ] || die "no $runtime to pass the secret through"
    tmp=$(umask 077 && mktemp -d "$runtime/.carry-over.XXXXXX")
    # shellcheck disable=SC2064
    trap "rm -rf -- '$tmp'" EXIT
    keyring_items "$found" "$tmp" || kr_fail "copying the item stopped with status $?"
    rm -rf -- "$tmp"
    trap - EXIT
}
# $1 the search answer for the old item, $2 a directory for the secrets.
keyring_items() {
    local found=$1 tmp=$2 attrs=() label='' k v rc n what err now i
    rc=0
    (umask 077 && st lookup application "$OLD_APP" >"$tmp/old" 2>/dev/null) || rc=$?
    if [ "$rc" != 0 ] && [ "$rc" != 124 ]; then
        kr_fail "the keyring is open, lists the item application=$OLD_APP and secret-tool lookup ended with status $rc for it"
        return 0
    fi
    if [ "$rc" != 0 ] || [ ! -s "$tmp/old" ]; then
        kr_wait "NOT COPIED YET: the item application=$OLD_APP could not be read ($([ "$rc" = 124 ] && echo "no answer in $KEYRING_WAIT s" || echo "it came back empty"))"
        return 0
    fi
    rc=0
    (umask 077 && st lookup application "$NEW_APP" >"$tmp/new" 2>/dev/null) || rc=$?
    if [ "$rc" = 124 ]; then
        kr_wait "NOT COPIED YET: no answer in $KEYRING_WAIT s when looking for application=$NEW_APP"
        return 0
    fi
    if [ "$rc" != 0 ] || [ -n "$force" ]; then
        cp -- "$tmp/old" "$tmp/put"
        what="copied the item application=$OLD_APP -> application=$NEW_APP"
    elif cmp -s -- "$tmp/old" "$tmp/new"; then
        kr_done "application=$NEW_APP is there and holds what application=$OLD_APP holds"
        return 0
    elif [ -z "$(tr -d '[:space:]{}' <"$tmp/new")" ]; then
        cp -- "$tmp/old" "$tmp/put"
        what="application=$NEW_APP was there and empty (the new shell stores it so when it finds none): the item application=$OLD_APP copied into it"
        found=$(kr_search "$NEW_APP") || true
    elif command -v jq >/dev/null && jq -e 'type == "object"' "$tmp/old" >/dev/null 2>&1 && jq -e 'type == "object"' "$tmp/new" >/dev/null 2>&1; then
        (umask 077 && jq -c -s '.[0] * .[1]' "$tmp/old" "$tmp/new" | tr -d '\n' >"$tmp/put")
        n=$(jq -s '([.[0] | paths(scalars)] - [.[1] | paths(scalars)]) | length' "$tmp/put" "$tmp/new")
        if [ "$n" = 0 ]; then
            kr_done "application=$NEW_APP is there and lacks nothing application=$OLD_APP holds (what differs is the new build's own, left as it is)"
            return 0
        fi
        what="application=$NEW_APP was there without $n of the values application=$OLD_APP holds: added, nothing in it replaced"
        found=$(kr_search "$NEW_APP") || true
    else
        kr_done "application=$NEW_APP is there with content of its own and cannot be compared with application=$OLD_APP (not JSON objects, or no jq): left as it is"
        return 0
    fi
    label=$(sed -n 's/^label = //p' <<<"$found" | head -n1)
    label=${label//$OLD_APP/$NEW_APP}
    while IFS= read -r v; do
        k=${v%% = *} k=${k#attribute.} v=${v#* = }
        case $k in application | xdg:schema) ;; *) attrs+=("$k" "$v") ;; esac
    done < <(grep '^attribute\.' <<<"$found" | awk '!seen[$1]++')
    if [ -n "$dry" ]; then
        echo "keyring: would be: $what"
        return 0
    fi
    label=${label:-$NEW_APP Safe Storage}
    rc=0
    err=$(st store --label="$label" application "$NEW_APP" "${attrs[@]}" <"$tmp/put" 2>&1) || rc=$?
    if [ "$rc" = 124 ]; then
        kr_wait "NOT COPIED YET: no answer in $KEYRING_WAIT s when writing the item application=$NEW_APP"
        return 0
    fi
    if [ "$rc" != 0 ]; then
        kr_fail "secret-tool store ended with status $rc for the label \"$label\"${attrs[*]:+ and the attributes ${attrs[*]}} (${err%%$'\n'*})"
        return 0
    fi
    # What was written is what was meant: the secret byte for byte, the label, each attribute.
    if ! st lookup application "$NEW_APP" 2>/dev/null | cmp -s - "$tmp/put"; then
        kr_fail "the item was written and its secret does not read back as what was put there"
        return 0
    fi
    # The keys are there. The names are looked at too where the keyring answers: an item named
    # in question marks is found by nothing that looks for it by its attributes.
    if now=$(kr_search "$NEW_APP"); then
        if ! grep -qxF -- "label = $label" <<<"$now"; then
            kr_fail "the item was written and its label does not read back as \"$label\""
            return 0
        fi
        for ((i = 0; i < ${#attrs[@]}; i += 2)); do
            grep -qxF -- "attribute.${attrs[i]} = ${attrs[i + 1]}" <<<"$now" && continue
            kr_fail "the item was written and its attribute ${attrs[i]} does not read back as \"${attrs[i + 1]}\""
            return 0
        done
    fi
    kr_done "$what" 1
}

carry() {
    local old
    while IFS= read -r old && IFS= read -r new; do copy_pair "$old" "$new"; done < <(pairs)
    copy_keyring
}

# The user's own files that name the build: every text file under these that holds an old name.
DOT_DIRS=(hypr matugen fish kitty foot fuzzel wlogout)
# $1: read (the files --dots rewrites) or kept (copies beside them that name the old build).
dot_files() {
    local d f
    for d in "${DOT_DIRS[@]}"; do
        [ -d "$config/$d" ] || continue
        while IFS= read -r -d '' f; do
            is_text "$f" && names_old "$f" || continue
            if is_read "$f"; then [ "${1-read}" != read ] || echo "$f"; else [ "${1-read}" != kept ] || echo "$f"; fi
        done < <(find -L "$config/$d" -type f ! -path '*/.git/*' -print0 | sort -z)
    done
    return 0
}

# Files elsewhere in the config directory that name the old build and that --dots does not touch:
# they are only listed, for a person to look at.
strays() {
    local f
    while IFS= read -r -d '' f; do
        is_read "$f" && is_text "$f" && names_old "$f" && echo "$f"
    done < <(find -L "$config" -maxdepth 4 \( $(printf -- "-path $config/%s -o " "${DOT_DIRS[@]}") -path "$config/systemd" \
        -o -path "$config/$OLD_WORD" -o -path "$config/$NEW_WORD" -o -path "$config/$OLD_APP" -o -path "$config/$NEW_APP" \
        -o -name .git -o -name node_modules \) -prune -o -type f -size -512k -print0 2>/dev/null | sort -z)
    return 0
}

# Writes text over a file through a temporary file beside what the path resolves to, so a link
# into a dotfiles checkout stays a link.
put_text() {
    local to tmp
    to=$(realpath -- "$2")
    tmp=$(mktemp "${to%/*}/.carry-over.XXXXXX")
    cp -p -- "$to" "$tmp" && cat -- "$1" >"$tmp" && mv -f -- "$tmp" "$to" || { rm -f -- "$tmp"; return 1; }
}

# The desktop widgets' layout (layout.toml, and the previous copy the shell keeps beside it) is
# in ~/.config/impulse-shell. While the settings were one folder for both builds, the old one
# could not read what the new one saved there (edge = "rusty"), so --dots new keeps each of the
# two files as it is at that moment, or that it is not there, and --dots old puts exactly that
# back. The old build has a folder of its own now and reads neither file; the two steps are
# made as before, and going back and forth leaves the new build its layout.
# What the new build saved is not lost: it stays beside the file as <name>.from-impulse-shell,
# and the next --dots new makes it the file again when the file still is what --dots old put
# there. A copy with that name that holds something else is kept under a name with the time;
# of those only the newest stays, so they do not add up with every switch.
LAYOUT_FILES=(layout.toml layout.toml.prev)
# $1: the copy's name without the time.
layout_prune() {
    local f first=1
    while IFS= read -r f; do
        [ -n "$first" ] || rm -f -- "$f"
        first=
    done < <(ls -1d -- "$1".[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]-[0-9][0-9][0-9][0-9][0-9][0-9] 2>/dev/null | sort -r)
}
# True when file $1 is what was kept as $2, or is not there as $2.absent says.
is_kept() {
    if [ -e "$2.absent" ]; then [ ! -e "$1" ] && [ ! -L "$1" ]; else [ -f "$2" ] && [ -f "$1" ] && cmp -s -- "$1" "$2"; fi
}
# A copy of $1 becomes $2 (mode and time too) through a temporary file beside what $2 resolves
# to: a reader finds what was there or the whole copy, and a link stays a link.
put_copy() {
    local to=$2 tmp
    [ ! -L "$to" ] || to=$(realpath -- "$to")
    mkdir -p -- "${to%/*}" && tmp=$(mktemp "${to%/*}/.carry-over.XXXXXX") || return 1
    cp -p -- "$1" "$tmp" && mv -f -- "$tmp" "$to" || { rm -f -- "$tmp"; return 1; }
}
layout_new() {
    local keep=$own/layout n f side take
    for n in "${LAYOUT_FILES[@]}"; do
        f=$config/$NEW_APP/$n side=$config/$NEW_APP/$n.from-$NEW_BIN
        [ -n "$dry" ] || layout_prune "$side"
        # Kept at the switch and not put back since: the file may be the new build's by now.
        if [ -e "$keep/$n" ] || [ -e "$keep/$n.absent" ]; then say "kept for $OLD_BIN already: $f"; continue; fi
        if [ ! -f "$f" ] && { [ -e "$f" ] || [ -L "$f" ]; }; then leave "$f is not a file: left as it is, not kept for $OLD_BIN"; continue; fi
        if [ -n "$dry" ]; then did "would keep $f as it is now for $OLD_BIN"; continue; fi
        mkdir -p -- "$keep/back"
        take=
        ! { [ -f "$side" ] && is_kept "$f" "$keep/back/$n"; } || take=1
        if [ -f "$f" ]; then put_copy "$f" "$keep/$n" || die "cannot keep $f"; else : >"$keep/$n.absent"; fi
        rm -f -- "$keep/back/$n" "$keep/back/$n.absent"
        note "layout new $f"
        [ -n "$take" ] || { say "kept for $OLD_BIN as it is now: $f"; continue; }
        put_copy "$side" "$f" && rm -f -- "$side" || die "cannot write $f"
        did "what $NEW_BIN saved last time is back: $f"
    done
}
layout_old() {
    local keep=$own/layout n f side k
    for n in "${LAYOUT_FILES[@]}"; do
        f=$config/$NEW_APP/$n side=$config/$NEW_APP/$n.from-$NEW_BIN
        [ -e "$keep/$n" ] || [ -e "$keep/$n.absent" ] || continue
        if ! is_kept "$f" "$keep/$n"; then
            if [ -n "$dry" ]; then did "would put back $f"; continue; fi
            if [ -f "$f" ]; then
                [ ! -e "$side" ] || cmp -s -- "$f" "$side" || { mv -f -- "$side" "$side.$stamp" && layout_prune "$side"; } || die "cannot move $side"
                put_copy "$f" "$side" || die "cannot keep $f as $side"
            fi
            if [ -e "$keep/$n.absent" ]; then
                rm -f -- "$f" || die "cannot remove $f"
                did "removed for $OLD_BIN, it was not there before: $f (what $NEW_BIN saved: $side)"
            else
                put_copy "$keep/$n" "$f" || die "cannot write $f"
                did "put back for $OLD_BIN: $f$([ ! -f "$side" ] || echo " (what $NEW_BIN saved: $side)")"
            fi
            note "layout old $f"
        fi
        [ -z "$dry" ] || continue
        mkdir -p -- "$keep/back" && rm -f -- "$keep/back/$n" "$keep/back/$n.absent"
        for k in "$n" "$n.absent"; do [ ! -e "$keep/$k" ] || mv -f -- "$keep/$k" "$keep/back/$k"; done
    done
}

dots_new() {
    local f n=0 keep from
    layout_new
    while IFS= read -r f; do
        n=$((n + 1))
        if [ -n "$dry" ]; then
            did "would rewrite $f:"
            diff <(cat -- "$f") <(rewrite <"$f") | sed -n 's/^[<>]/   &/p' || true
            continue
        fi
        keep=$own/dots
        mkdir -p -- "$keep/old${f%/*}" "$keep/new${f%/*}"
        # The text made for the new build last time goes back when the old text is the one that
        # was put back then: what the user edited while on the new build is not lost. A file
        # that still is what the last run made (this script rewrites more than the one that
        # ran then) keeps the old text of that run: that is still what --dots old puts back.
        from=$f
        if [ -f "$keep/new$f" ] && cmp -s -- "$f" "$keep/old$f"; then
            from=$keep/new$f
        elif [ ! -f "$keep/old$f" ] || ! cmp -s -- "$f" "$keep/new$f"; then
            cp -p -- "$f" "$keep/old$f"
        fi
        rewrite <"$from" >"$keep/new$f.tmp" && mv -f -- "$keep/new$f.tmp" "$keep/new$f" || die "cannot write $keep/new$f"
        put_text "$keep/new$f" "$f" || die "cannot write $f"
        note "dots new $f"
        did "rewritten for $NEW_BIN: $f"
    done < <(dot_files)
    [ "$n" != 0 ] || say "no file names the old build"
}

dots_old() {
    local keep=$own/dots f n=0
    layout_old
    [ -d "$keep/old" ] || { say "no file was rewritten: nothing to put back"; return 0; }
    while IFS= read -r -d '' f; do
        f=${f#"$keep/old"}
        [ -e "$f" ] || { leave "$f is gone: its old text stays in $keep/old$f"; continue; }
        cmp -s -- "$f" "$keep/old$f" && continue
        n=$((n + 1))
        if [ -n "$dry" ]; then did "would put back $f"; continue; fi
        mkdir -p -- "$keep/new${f%/*}"
        cp -p -- "$f" "$keep/new$f"
        put_text "$keep/old$f" "$f" || die "cannot write $f"
        note "dots old $f"
        did "put back for $OLD_BIN: $f"
    done < <(find "$keep/old" -type f -print0 | sort -z)
    [ "$n" != 0 ] || say "every file is as it was for the old build"
}

status() {
    local old new f found rc2
    while IFS= read -r old && IFS= read -r new; do
        if [ "$old" = "$new" ]; then
            printf '%-8s %s\n' "$([ -e "$old" ] && echo both || echo -)" "$old"
            continue
        fi
        printf '%-8s %s\n' "$([ -e "$old" ] && echo old || echo -)" "$old"
        printf '%-8s %s\n' "$([ -e "$new" ] && echo new || echo -)" "$new"
    done < <(pairs)
    if command -v secret-tool >/dev/null; then
        for f in "$OLD_APP" "$NEW_APP"; do
            found=$(kr_search "$f") && rc2=0 || rc2=$?
            if [ "$rc2" = 124 ]; then found='no answer'
            elif kr_is_locked "$found"; then found=locked
            elif grep -q '^attribute\.application' <<<"$found"; then found=yes
            else found=-; fi
            printf '%-8s keyring application=%s\n' "$found" "$f"
        done
        [ ! -f "$pending" ] || echo "the keyring item is not copied yet (since $(sed -n 's/^since=//p' "$pending")): carry-over.sh --keyring copies it once the keyring is open, rollback-watch runs that while it is on"
    fi
    while IFS= read -r f; do echo "names the old build: $f"; done < <(dot_files)
    while IFS= read -r f; do echo "names the old build, a copy kept beside a file, never rewritten: $f"; done < <(dot_files kept)
    while IFS= read -r f; do echo "names the old build, not one of the files --dots rewrites: $f"; done < <(strays)
    [ ! -r /etc/geoclue/geoclue.conf ] || ! grep -q "^\[org\.$OLD_WORD\]" /etc/geoclue/geoclue.conf ||
        grep -q "^\[org\.$NEW_BIN\]" /etc/geoclue/geoclue.conf ||
        echo "/etc/geoclue/geoclue.conf allows org.$OLD_WORD by name and not org.$NEW_BIN: the new build gets no location until a section [org.$NEW_BIN] is added beside it"
    [ ! -f "$own/log" ] || { echo "done so far ($own/log):"; sed 's/^/  /' "$own/log"; }
}

mode=carry
while [ $# -gt 0 ]; do
    case $1 in
        -n | --dry-run) dry=1 ;;
        -q | --quiet) quiet=1 ;;
        --force) force=1 ;;
        --status) mode=status ;;
        --keyring) mode=copy_keyring ;;
        --dots)
            [ $# -ge 2 ] || die "--dots needs new or old"
            case $2 in new | old) mode=dots_$2 ;; *) die "--dots needs new or old" ;; esac
            shift
            ;;
        -h | --help) usage; exit 0 ;;
        *) die "unknown argument $1 (see --help)" ;;
    esac
    shift
done
[ "$(id -u)" != 0 ] || die "run this as the user whose data it is, not as root"
[ "$NEW_WORD" != quickshell ] || die "RI_WORD is quickshell: that is the old name"
[[ $NEW_WORD =~ ^[a-z][a-z0-9-]*$ ]] || die "RI_WORD=$NEW_WORD is not a name"
$mode
[ "$failed" = 0 ] || exit "$failed"
[ "$mode" != copy_keyring ] || [ "$left" != 0 ] || [ -z "$wrote" ] || exit 5
exit "$left"
