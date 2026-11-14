#!/bin/bash
# A PC laid out for tools/pc/switch in the home $HOME names (an empty directory), with a real
# gnome-keyring on a session bus of its own, open, holding the old build's item the way a
# PC in use has it: a Cyrillic label, a Cyrillic attribute, real keys. Runs the repository's
# switch and carry-over.sh there and prints what the keyring holds afterwards.
#   run.sh REPO moves   the keyring works
#   run.sh REPO fails   "secret-tool store" fails: the keyring step cannot write its item
# Stand-ins: Hyprland (hyprctl), install.sh, the rollback scripts, the two shells (copies of
# sleep under their names) and rsync (switch wants it there for the rollback scripts and does
# not call it). In a PID namespace of its own: switch stops every ii-rs and
# rusty-impulse of the user, and here those are only this home's.
set -u
[ -n "${PC_SWITCH_NS-}" ] || exec env PC_SWITCH_NS=1 unshare --user --map-user="$(id -u)" --map-group="$(id -g)" \
    --pid --fork --mount-proc dbus-run-session -- bash "$0" "$@"
repo=$1 case=$2
export XDG_RUNTIME_DIR=$HOME/run PATH=$HOME/bin:/usr/bin:/bin
unset XDG_CONFIG_HOME XDG_STATE_HOME XDG_CACHE_HOME XDG_DATA_HOME HYPRLAND_INSTANCE_SIGNATURE WAYLAND_DISPLAY LANG LANGUAGE LC_ALL
here=$HOME/pcbuild/stable-test
mkdir -p "$XDG_RUNTIME_DIR" "$HOME/bin" "$here/tree" "$HOME/.local/bin" "$HOME/.config/quickshell/ii"
chmod 700 "$XDG_RUNTIME_DIR"
program() { printf '#!/bin/bash\n%s\n' "$2" >"$1" && chmod 755 "$1"; }

program "$HOME/bin/hyprctl" 'case "$*" in
    "monitors -j") echo "[{}]" ;;
    "layers -j") if pgrep -x rusty-impulse >/dev/null; then echo "{\"M\":{\"levels\":{\"0\":[{\"namespace\":\"rustshell:bar\"}]}}}"; else echo "{}"; fi ;;
    dispatch*rusty-impulse-shell*) setsid "$HOME/.local/bin/rusty-impulse" infinity >/dev/null 2>&1 & ;;
    dispatch*ii-shell*) setsid "$HOME/.local/bin/ii-rs" infinity >/dev/null 2>&1 & ;;
esac'
program "$HOME/bin/rsync" 'echo "rsync: a stand-in, not to be called" >&2; exit 1'
cp "$repo/tools/pc/switch" "$repo/tools/carry-over.sh" "$here/"
cp "$(command -v sleep)" "$here/rusty-impulse"
cp "$(command -v sleep)" "$HOME/.local/bin/ii-rs"
echo 'ShellRoot {}' >"$HOME/.config/quickshell/ii/shell.qml"
program "$here/install.sh" 'mkdir -p "$PREFIX/bin" "$HOME/.config/rustshell/rusty-impulse" && cp "$BIN" "$PREFIX/bin/rusty-impulse"'
program "$here/rollback-watch" 'OLD_BIN=
OLD_TREE=
OLD_SHA=
exit 0'
program "$here/rollback-now" 'OLD_BIN=
OLD_TREE=
OLD_SHA=
exit 0'
for f in "$here/rusty-impulse-shell" "$here/rusty-impulse-default" "$HOME/.local/bin/ii-shell"; do program "$f" 'exit 0'; done

# The keyring, open. Waited for as the owner of its bus name ("gdbus wait" returns when the bus
# says the name has one, and starts nothing): a secret-tool that asks before the daemon has it
# makes the bus start a second daemon, whose keyring is not open. The limits here and below
# only make a step that never ends a failure that names it.
printf pw | gnome-keyring-daemon --foreground --unlock --components=secrets >/dev/null 2>&1 &
gdbus wait --session --timeout 30 org.freedesktop.secrets ||
    { echo "run.sh: starting the keyring: org.freedesktop.secrets has no owner after 30 s"; exit 1; }
printf '%s' '{"apiKeys":{"gemini":"AIzaSy-ключ-9f3","openai":"sk-proj-Zx81"}}' >"$HOME/keys"
LC_ALL=C.UTF-8 timeout 30 secret-tool store --label='Ключи illogical-impulse' application illogical-impulse \
    explanation 'Ключи для ИИ' <"$HOME/keys"
case $? in
    0) ;;
    124) echo "run.sh: storing the old item: secret-tool store did not end in 30 s"; exit 1 ;;
    *) echo "run.sh: storing the old item: secret-tool store failed"; exit 1 ;;
esac
setsid "$HOME/.local/bin/ii-rs" infinity >/dev/null 2>&1 &

[ "$case" != fails ] || program "$HOME/bin/secret-tool" '[ "$1" != store ] || { echo "secret-tool: the keyring does not take this item" >&2; exit 1; }
exec /usr/bin/secret-tool "$@"'

(cd "$here" && START_WAIT=10 timeout 120 ./switch) >"$HOME/switch.out" 2>&1
rc=$?
echo "switch: status $rc"
sed 's/^/switch: /' "$HOME/switch.out"
[ "$rc" != 124 ] || { echo "run.sh: running switch: it did not end in 120 s"; exit 1; }
echo "shell: $(pgrep -x rusty-impulse >/dev/null && echo the new build runs || echo the new build does not run)"
LC_ALL=C.UTF-8 /usr/bin/secret-tool search application rusty-impulse 2>&1 | grep -E '^(label|attribute\.)' | sed 's/^/new item: /'
if LC_ALL=C.UTF-8 /usr/bin/secret-tool lookup application rusty-impulse >"$HOME/new" 2>/dev/null; then
    cmp -s "$HOME/keys" "$HOME/new" && echo "new item: secret equals the old one byte for byte" || echo "new item: secret differs: $(cat "$HOME/new")"
else
    echo "new item: none"
fi
LC_ALL=C.UTF-8 /usr/bin/secret-tool lookup application illogical-impulse | cmp -s "$HOME/keys" - && echo "old item: as it was"
