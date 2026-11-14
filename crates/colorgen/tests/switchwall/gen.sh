#!/usr/bin/env bash
# Goldens for tests/switchwall.rs: runs the original switchwall.sh of the tree (matugen 4.2.0 +
# materialyoucolor 2.0.10 venv) in a sandbox HOME with logging fakes for every external tool.
# Usage: VENV=<venv with materialyoucolor 2.0.10, pillow, opencv> gen.sh
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$HERE/../../../.." && pwd)
II=$REPO/qml
# the original's switchwall.sh and its helpers, kept with the tests: the shell ships a switchwall.sh of its
# own, which runs rusty-impulse, and the terminal templates.
UNSHIPPED=$REPO/crates/qml/tests/fixtures/original-unshipped
: "${VENV:?}"
OUT=$HERE/expected
rm -rf "$OUT"; mkdir -p "$OUT"
WORK=$(mktemp -d); trap 'rm -rf "$WORK"' EXIT

fake() { # name [body]
  printf '#!/usr/bin/env bash\necho "%s $*" >> "$SW_LOG"\n%s\n' "$1" "${2:-}" > "$WORK/bin/$1"; chmod +x "$WORK/bin/$1"
}
mkdir -p "$WORK/bin"
fake gsettings '[[ $1 == get ]] && echo "'"'"'prefer-dark'"'"'"; exit 0'
fake hyprctl 'case "$1" in monitors) echo '"'"'[{"name":"DP-1","width":1920,"height":1080,"x":0,"y":0,"scale":1.0,"focused":true},{"name":"HDMI-A-1","width":1280,"height":720,"x":1920,"y":0,"scale":1.0,"focused":false}]'"'"';; cursorpos) echo '"'"'{"x":100,"y":100}'"'"';; esac'
fake pkill 'exit 1'
fake pgrep 'exit 1'
fake pidof 'exit 1'
fake mpvpaper
fake notify-send
fake kdialog
fake hyprpicker 'echo "#3A5F0B"'
fake identify 'case "$2" in %w) python3 -c "import struct,sys;print(1)";; esac'
fake upscayl
fake ffmpeg 'cp "$SW_FIX/thumb.jpg" "${@: -1}"'
fake xdg-user-dir 'echo "$HOME/Pictures"'
# `identify`: real dimensions through Pillow
cat > "$WORK/bin/identify" <<'ID'
#!/usr/bin/env bash
echo "identify $*" >> "$SW_LOG"
"$VENV/bin/python3" -c "import sys;from PIL import Image;i=Image.open(sys.argv[2]);print(i.width if sys.argv[1]=='%w' else i.height,end='')" "$2" "$3"
ID
chmod +x "$WORK/bin/identify"

# case <name> <setup shell> -- <switchwall args...>
case_() {
  local name=$1 setup=$2; shift 3
  local H=$WORK/$name
  rm -rf "$H"; mkdir -p "$H"
  cp -r "$HERE/fixture/home/." "$H/"
  mkdir -p "$H/.config/rustshell/rusty-impulse/assets/images" "$H/pts"
  cp -r "$II/scripts" "$H/.config/rustshell/rusty-impulse/"
  cp -r "$UNSHIPPED/scripts/." "$H/.config/rustshell/rusty-impulse/scripts/"
  cp "$II/assets/images/default_wallpaper.png" "$H/.config/rustshell/rusty-impulse/assets/images/"
  sed -i "s|/dev/pts/\*|\$SW_PTS/*|; s|\^/dev/pts/|^\$SW_PTS/|" "$H/.config/rustshell/rusty-impulse/scripts/colors/applycolor.sh"
  : > "$H/pts/0"; : > "$H/pts/1"; : > "$H/pts/ptmx"
  local W=$H/Pictures/Wallpapers CFG=$H/.config/rusty-impulse/config.json GEN=$H/.local/state/rustshell/user/generated
  (cd "$H" && W=$W CFG=$CFG GEN=$GEN bash -c "$setup")
  mkdir -p "$OUT/$name/pre"
  (cd "$H" && find .config/rusty-impulse .local -type f 2>/dev/null) | while read -r f; do
    mkdir -p "$OUT/$name/pre/$(dirname "$f")"; sed "s|$H|@HOME@|g" "$H/$f" > "$OUT/$name/pre/$f"
  done
  local args=()
  for a in "$@"; do args+=("${a//@W@/$W}"); done
  [ -n "${KEEP_WORK:-}" ] && { mkdir -p "$KEEP_WORK"; rm -rf "$KEEP_WORK/$name"; cp -a "$H" "$KEEP_WORK/$name"; printf '%s\n' "$@" > "$KEEP_WORK/$name.args"; }
  : > "$H/calls.log"
  # Every process the script starts inherits fd 3, the pipe into `cat`: the pipeline ends once
  # the script's background jobs (`&`, `& disown`) have exited too.
  (cd "$H" && env -i HOME="$H" PATH="$WORK/bin:$HOME/.cargo/bin:/usr/bin:/bin" SW_LOG="$H/calls.log" SW_PTS="$H/pts" SW_FIX="$HERE/fixture" VENV="$VENV" \
    RUSTY_IMPULSE_VIRTUAL_ENV="$VENV" bash "$H/.config/rustshell/rusty-impulse/scripts/colors/switchwall.sh" "${args[@]}" >/dev/null 2>"$H/stderr.log"; echo $? > "$H/rc") 3>&1 | cat
  local O=$OUT/$name
  mkdir -p "$O"
  printf '%s\n' "$@" > "$O/args"
  printf '%s\n' "$setup" > "$O/setup"
  cp "$H/rc" "$O/rc"
  grep -v '^hyprctl \|^gsettings get\|^identify ' "$H/calls.log" | sed "s|$H|@HOME@|g" | sort > "$O/calls"
  (cd "$H" && find .local .cache .config/hypr .config/fuzzel .config/gtk-3.0 .config/gtk-4.0 .config/rusty-impulse .config/Code .config/Cursor pts -type f 2>/dev/null | sort) | while read -r f; do
    mkdir -p "$O/files/$(dirname "$f")"
    if grep -Iq . "$H/$f" || [ ! -s "$H/$f" ]; then sed "s|$H|@HOME@|g; s|^# Time: .*|# Time: @DATE@|" "$H/$f" > "$O/files/$f"; else cp "$H/$f" "$O/files/$f"; fi
  done
}

case_ img_dark_auto ':' -- --mode dark --image @W@/syn_grad.jpg
case_ img_light_type ':' -- --type scheme-fruit-salad --mode light @W@/syn_small.png
case_ img_mode_variant ':' -- --mode light --image @W@/duo-dark.png
case_ img_no_mode_gif ':' -- @W@/syn_anim.gif
case_ img_gray_neutral ':' -- --mode dark --image @W@/syn_gray.png
case_ img_invalid_type ':' -- --type bogus --mode dark --image @W@/syn_grad.jpg
case_ img_force_dark 'jq ".appearance.wallpaperTheming.terminalGenerationProps.forceDarkMode = true" $CFG > c && mv c $CFG' -- --mode light --image @W@/syn_grad.jpg
case_ img_term_off 'jq ".appearance.wallpaperTheming.enableTerminal = false | .appearance.wallpaperTheming.enableQtApps = false" $CFG > c && mv c $CFG' -- --mode dark --image @W@/syn_small.png
case_ img_apps_off 'jq ".appearance.wallpaperTheming.enableAppsAndShell = false" $CFG > c && mv c $CFG' -- --mode dark --image @W@/syn_small.png
case_ img_accent_cleared 'jq ".appearance.palette.accentColor = \"#A52F48\"" $CFG > c && mv c $CFG' -- --mode dark --image @W@/syn_small.png
case_ noswitch_wall 'jq ".background.wallpaperPath = \"$W/syn_grad.jpg\"" $CFG > c && mv c $CFG' -- --mode dark --noswitch
case_ noswitch_color ':' -- --noswitch --color '#A52F48' --mode light
case_ noswitch_color_nohash ':' -- --noswitch --color 4285F4 --mode dark
case_ noswitch_picker ':' -- --mode dark --noswitch --color
case_ noswitch_clear 'jq ".appearance.palette.accentColor = \"#A52F48\"" $CFG > c && mv c $CFG' -- --color clear --mode dark --noswitch
case_ noswitch_seed_file 'mkdir -p $GEN && printf "#D776DE" > $GEN/color.txt && echo scheme-rainbow > $GEN/scheme_type.txt' -- --mode light --noswitch
case_ noswitch_last_wall 'mkdir -p $GEN/wallpaper && echo "file://$W/syn_small.png" > $GEN/wallpaper/path.txt' -- --mode dark --noswitch
case_ noswitch_default ':' -- --mode dark --noswitch
case_ broken_keeps_previous 'mkdir -p $GEN && printf "\$primary: #111111;\n" > $GEN/material_colors.scss && printf "#111111" > $GEN/color.txt' -- --mode dark --image @W@/broken.png
case_ video 'jq ".background.wallpaperPath = \"\"" $CFG > c && mv c $CFG' -- --mode dark @W@/clip.mp4
[ -n "${KEEP_WORK:-}" ] && cp -a "$WORK/bin" "$KEEP_WORK/"
echo "$(ls "$OUT" | wc -l) cases"
