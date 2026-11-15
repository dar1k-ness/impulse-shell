#!/bin/bash
# A home that runs the shell under its previous name, the way the owner's PC does, in the home
# $HOME names (an empty directory): that build's link, tree, start script and running shell, a
# rollback watch of its own (a unit that names its script) and an execs.lua that starts the
# shell by that start script. Runs the repository's install.sh (the one command) there, from a
# checkout laid out beside it, and prints what is there afterwards.
#   run.sh REPO comes   the new shell comes up
#   run.sh REPO fails   the new shell never shows its panels
#   run.sh REPO later   --no-start: nothing is stopped or started
# (The previous name is in halves here: tools/rename/rename.py renames it wherever it stands
# whole.) Stand-ins: Hyprland (hyprctl), systemctl, tools/install.sh, the watch's script and
# the two shells (copies of sleep under their names). In a PID namespace of its own:
# install.sh stops every shell of the user, and here those are only this home's.
set -u
[ -n "${ONE_COMMAND_NS-}" ] || exec env ONE_COMMAND_NS=1 unshare --user --map-user="$(id -u)" --map-group="$(id -g)" \
    --pid --fork --mount-proc bash "$0" "$@"
repo=$1 case=$2
export XDG_RUNTIME_DIR=$HOME/run PATH=$HOME/bin:/usr/bin:/bin HYPRLAND_INSTANCE_SIGNATURE=test
unset XDG_CONFIG_HOME XDG_STATE_HOME XDG_CACHE_HOME XDG_DATA_HOME WAYLAND_DISPLAY LANG LANGUAGE LC_ALL RI_REPO RI_SRC RI_WORD
was=rusty
was+=-impulse
src=$HOME/src kit=$HOME/pcbuild/stable-old hypr=$HOME/.config/hypr
mkdir -p "$XDG_RUNTIME_DIR" "$HOME/bin" "$src/tools" "$kit" "$HOME/.local/bin" "$HOME/.config/rustshell/$was" \
    "$HOME/.config/$was" "$hypr/hyprland" "$HOME/.config/systemd/user"
chmod 700 "$XDG_RUNTIME_DIR"
program() { printf '#!/bin/bash\n%s\n' "$2" >"$1" && chmod 755 "$1"; }
# started PROGRAM ARG...: the program in a session of its own. It runs under its name when this
# returns (Popen returns when the exec is done), so what looks for it by name next finds it.
program "$HOME/bin/started" 'exec python3 -c "import subprocess, sys
n = subprocess.DEVNULL
subprocess.Popen(sys.argv[1:], start_new_session=True, stdin=n, stdout=n, stderr=n)" "$@"'

new_shell='started "$HOME/.local/bin/impulse-shell" infinity'
[ "$case" != fails ] || new_shell=:
program "$HOME/bin/hyprctl" 'echo "hyprctl $*" >>"$HOME/calls"
case "$*" in
    version) echo Hyprland ;;
    reload) echo ok ;;
    "layers -j") if pgrep -x impulse-shell >/dev/null; then echo "{\"M\":{\"levels\":{\"0\":[{\"namespace\":\"rustshell:bar\"}]}}}"; else echo "{}"; fi ;;
    dispatch*'"$was"'-shell*) started "$HOME/.local/bin/'"$was"'" infinity
        echo ok ;;
    dispatch*impulse-shell*) '"$new_shell"'
        echo ok ;;
esac'
program "$HOME/bin/systemctl" 'echo "systemctl $*" >>"$HOME/calls"'
program "$kit/rollback-watch" 'echo "old watch: $*" >>"$HOME/calls"'
printf '[Service]\nExecStart=%s run\n[Install]\nWantedBy=hyprland-session.target\n' "$kit/rollback-watch" \
    >"$HOME/.config/systemd/user/$was-rollback-watch.service"

cp "$repo/install.sh" "$src/" && cp "$repo/tools/carry-over.sh" "$src/tools/" && : >"$src/Cargo.toml"
program "$src/tools/install.sh" '[ "${1-}" != --settle ] || exit 0
mkdir -p "$PREFIX/bin" "$HOME/.local/share/impulse-shell" && cp /usr/bin/sleep "$PREFIX/bin/impulse-shell"'
cp /usr/bin/sleep "$HOME/.local/bin/$was"
program "$HOME/.local/bin/$was-shell" 'exit 0'
echo 'ShellRoot {}' >"$HOME/.config/rustshell/$was/shell.qml"
echo '{"bar":{"bottom":true}}' >"$HOME/.config/$was/config.json"
: >"$hypr/hyprland.lua"
printf '%s\n' "hl.exec_cmd(\"\$HOME/.local/bin/$was-shell\")" >"$hypr/hyprland/execs.lua"
printf '%s\n' "hl.bind(\"CTRL + SUPER + R\", hl.dsp.exec_cmd(\"killall qs quickshell $was; \$HOME/.local/bin/$was-shell\"))" >"$hypr/hyprland/keybinds.lua"
started "$HOME/.local/bin/$was" infinity

args=(--no-packages)
[ "$case" != later ] || args+=(--no-start)
RI_START_WAIT=3 timeout 120 bash "$src/install.sh" "${args[@]}" >"$HOME/install.out" 2>&1
rc=$?
echo "install: status $rc"
sed 's/^/install: /' "$HOME/install.out"
echo "new shell: $(pgrep -x impulse-shell >/dev/null && echo runs || echo does not run)"
echo "old shell: $(pgrep -x "$was" >/dev/null && echo runs || echo does not run)"
sed 's/^/execs.lua: /' "$hypr/hyprland/execs.lua"
sed 's/^/keybinds.lua: /' "$hypr/hyprland/keybinds.lua"
echo "start script: $([ -x "$HOME/.local/bin/impulse-shell-start" ] && tail -n 1 "$HOME/.local/bin/impulse-shell-start" || echo none)"
echo "settings: $(cat "$HOME/.config/impulse-shell/config.json" 2>/dev/null || echo none)"
echo "old settings: $(cat "$HOME/.config/$was/config.json" 2>/dev/null || echo none)"
grep -E '^(old watch|systemctl)' "$HOME/calls" | sed 's/^/call: /'
grep -n -m1 "dispatch.*$was-shell" "$HOME/calls" | sed 's/:.*//; s/^/old shell started at call /'
grep -n -m1 '^old watch: enable' "$HOME/calls" | sed 's/:.*//; s/^/old watch enabled at call /'
