# Scene matrix for capture.sh. Each scene_X opens a state, shoots, and puts the shell back
# to the base state. Coordinates are for a 1920x1080 output at scale 1 (Virtual-1 in the guest).
SCENES="base bar_hover sidebarRight sidebarRight_tabs ptr_sidebarRight ptr_hold ptr_hold_buttons ptr_sidebarLeft ptr_overview ptr_notif ptr_overlay ptr_overlay_buttons region sidebarLeft overview search session cheatsheet
mediaControls overlay wallpaperSelector osdVolume osdLife notif anim bar_vertical bar_bottom bar_float waffle settings welcome killDialog polkit notifburst lock
firstStart"

scene_base() { park; shot base 3; }

# Bar hover states: each widget's hover highlight and its StyledPopup.
scene_bar_hover() {
  local spec name
  for spec in leftSide:104:20 resources:551:20 media:732:20 workspaces:842:20 \
      clock:1228:20 utilButtons:1380:20 rightSide:1877:20; do
    IFS=: read -r name x y <<< "$spec"
    ptr "$x" "$y"; shot "bar_hover_$name" 1.5
  done
  park; sleep 1
}

scene_sidebarRight() { ipc sidebarRight open; shot sidebarRight; ipc sidebarRight close; sleep 1; }
scene_sidebarRight_tabs() {
  ipc sidebarRight open; sleep 1.5
  local spec name
  for spec in todo:1520:920 timer:1520:977 calendar:1520:863; do
    IFS=: read -r name x y <<< "$spec"
    click "$x" "$y"; shot "sidebarRight_$name"
  done
  park; ipc sidebarRight close; sleep 1
}
# Pointer sweeps (real uinput hover/click/drag). Coordinates are 1920x1080 output pixels.
scene_ptr_sidebarRight() {
  ipc sidebarRight open; sleep 1.5
  local spec name x y
  for spec in internet:1567:151 bluetooth:1739:151 silence:1518:681 clearAll:1865:681 calNext:1869:738; do
    IFS=: read -r name x y <<< "$spec"
    ptr "$x" "$y"; shot "ptr_sidebarRight_hover_$name" 1
  done
  click 1869 738; shot ptr_sidebarRight_calNext
  click 1833 738; shot ptr_sidebarRight_calPrev
  click 1505 738; shot ptr_sidebarRight_calCollapse
  click 1505 1038; shot ptr_sidebarRight_calExpand
  click 1745 83; shot ptr_sidebarRight_editMode
  click 1745 83; shot ptr_sidebarRight_editModeOff
  park; ipc sidebarRight close; sleep 1
}

# The quick toggles in edit mode (the pencil), with the default toggles: right click changes a
# toggle's size, left click on an unused one (below the line) adds it, on a used one removes it.
# The shell writes each change to config.json; the scene puts the file back as it was.
scene_ptr_quickToggleEdit() {
  local saved; saved=$(jq -c '.sidebar.quickToggles' "$CFG")
  ipc sidebarRight open; sleep 1.5
  click 1745 83; ptr 1700 520; shot ptr_quickToggleEdit_on
  click 1514 150 right; shot ptr_quickToggleEdit_sizeInternet
  click 1611 294; shot ptr_quickToggleEdit_addDarkMode
  click 1694 294; shot ptr_quickToggleEdit_addGameMode
  click 1652 150; shot ptr_quickToggleEdit_removeBluetooth
  click 1694 213 right; shot ptr_quickToggleEdit_sizeDarkMode
  click 1745 83; ptr 1700 520; shot ptr_quickToggleEdit_off
  park; ipc sidebarRight close; sleep 1
  if [ "$saved" = null ]; then cfg '.sidebar |= del(.quickToggles) | .' .; else cfg .sidebar.quickToggles "$saved"; fi
}

# A button held down. The ripple of a RippleButton grows from the pointer for 1.2 s (rippleDuration)
# and then covers the button for as long as the button is down; a click never shows it, because
# its ripple fades before the screen is still. The calendar's "next month" is a RippleButton and
# releasing it is a click, so "previous month" puts the calendar back.
scene_ptr_hold() {
  ipc sidebarRight open; sleep 1.5
  ptr 1869 738; shot ptr_hold_calNext_hover 1
  hold 1869 738; shot ptr_hold_calNext 1.5; release
  shot ptr_hold_calNext_released
  click 1833 738; shot ptr_hold_calPrev
  park; ipc sidebarRight close; sleep 1
}
# The same on two RippleButtons of other kinds: the bar's on-screen keyboard toggle (a
# CircleUtilButton; releasing it opens the keyboard, a click closes it again) and the close button
# of the settings window (releasing it closes the window).
scene_ptr_hold_buttons() {
  ptr 1407 19; shot ptr_hold_barOsk_hover 1
  hold 1407 19; shot ptr_hold_barOsk 1.5; release; sleep 1.5
  shot ptr_hold_barOsk_released 1
  click 1407 19; sleep 1.5; park
  app settings; sleep 6; ptr 1871 86; shot ptr_hold_settingsClose_hover 1
  hold 1871 86; shot ptr_hold_settingsClose 1.5; release; sleep 1.5
  app_close settings
}
scene_ptr_sidebarLeft() {
  ipc sidebarLeft open; sleep 1.5
  ptr 404 1040; shot ptr_sidebarLeft_hover_clear 1
  click 294 81; shot ptr_sidebarLeft_anime
  click 184 81; shot ptr_sidebarLeft_intelligence
  park; ipc sidebarLeft close; sleep 1
}

scene_ptr_overview() {
  ipc search workspacesToggle; sleep 1.5
  ptr 959 229; shot ptr_overview_hover_ws3 1
  ptr 1068 77; shot ptr_overview_hover_searchIcon 1
  click 610 229; shot ptr_overview_ws2
  ipc search workspacesToggle; sleep 1.5
  click 258 229; sleep 1
  park; shot ptr_overview_ws1
}

scene_ptr_notif() {
  timeout 30 notify-send -a parity -A open=Open -A later=Later "Actions" "Notification with actions" &
  sleep 1.5
  ptr 1714 73; shot ptr_notif_popup_hover 1
  click 1891 64; shot ptr_notif_popup_expanded
  park; sleep 1
  ipc sidebarRight open; sleep 1.5
  ptr 1690 306; shot ptr_notif_sidebar_hover 1
  click 1870 283; shot ptr_notif_sidebar_expanded
  # 200 px is past the 70 px that dismisses and stays on the 1920 px output.
  drag 1690 306 200 0; shot ptr_notif_sidebar_dragged 2
  park; ipc sidebarRight close; sleep 1
  wait
}

scene_sidebarLeft() { ipc sidebarLeft open; shot sidebarLeft; ipc sidebarLeft close; sleep 1; }
scene_overview() { ipc search workspacesToggle; shot overview; ipc search workspacesToggle; sleep 1; }
scene_search() {
  ipc search toggle; shot search
  key "term"; shot search_query
  key -k Escape; sleep 1; ipc search close 2>/dev/null; sleep 0.5
}
scene_session() { ipc session open; shot session; ipc session close; sleep 1; }
scene_cheatsheet() { ipc cheatsheet open; shot cheatsheet; ipc cheatsheet close; sleep 1; }
scene_mediaControls() { ipc mediaControls open; shot mediaControls; ipc mediaControls close; sleep 1; }
# The overlay is toggled by IPC only; these two say which way and stop the run when it did not go
# there (a click beside a widget closes the overlay, and a blind toggle then opens it for good).
overlay_open() { layer overlay || ipc overlay toggle; wait_for 10 layer overlay || die "the overlay did not open"; sleep 1; }
overlay_close() { no_layer overlay || ipc overlay toggle; wait_for 10 no_layer overlay || die "the overlay did not close"; sleep 1; }
overlay_is_open() { layer overlay || die "$1 closed the overlay: the click missed its widget"; }
# Overlay widgets move with a DragHandler: drag the crosshair, then the volume mixer, click a pin.
# The positions are persistent state, so the shell is restarted on the default ones afterwards.
scene_ptr_overlay() {
  overlay_open
  drag 960 490 300 200; shot ptr_overlay_crosshair 1.5
  drag 445 930 60 40; shot ptr_overlay_mixer 1.5
  click 396 109; shot ptr_overlay_pinned 1
  click 396 109; overlay_is_open "the pin button"; overlay_close
  relaunch overlay-reset
}
# Every overlay button: recorder actions, "Open recordings folder" (Qt.openUrlExternally), widget header, taskbar toggles.
# Taskbar toggles go last: re-adding a widget makes the original's delegates show another widget's content.
# The recorder buttons close the overlay themselves; what they did is in overlay-actions.log.
scene_ptr_overlay_buttons() {
  local act=$OUT/overlay-actions.log
  region_warm
  overlay_open
  click 155 170; region_shot ptr_overlay_btn_region
  overlay_open
  wl-copy -c 2>/dev/null; click 230 170; shot ptr_overlay_btn_screenshot 1.5
  echo "screenshot: clipboard has $(wl-paste -l 2>&1 | tr '\n' ' ')" >> "$act"
  overlay_open
  click 306 170; region_shot ptr_overlay_btn_regionRecord
  overlay_open
  click 382 170; sleep 2; shot ptr_overlay_btn_screenRecord 0.5
  echo "screenRecord: wf-recorder $(pgrep -x wf-recorder >/dev/null && echo running || echo not running)" >> "$act"
  pkill -INT -x wf-recorder; wait_for 10 no_proc wf-recorder || die "wf-recorder did not stop"
  rm -f "$HOME"/Videos/recording_*.mp4
  # record.sh's "Starting recording" popup stays for a few seconds: not on the next shot.
  wait_for 20 no_layer notificationPopup || die "the recording notification did not go away"
  overlay_open
  : > /tmp/parity-opened.log; click 269 230; shot ptr_overlay_btn_folder 1.5
  wait_for 10 test -s /tmp/parity-opened.log || true
  echo "folder: asked the desktop to open $(tr '\n' ' ' < /tmp/parity-opened.log)" >> "$act"
  overlay_open
  click 999 469; overlay_is_open "the crosshair widget"; shot ptr_overlay_btn_center 1
  local i=0 x
  for x in 776 824 872 920 967 1016 1064; do
    click "$x" 78; overlay_is_open "taskbar button $i"; shot "ptr_overlay_btn_task$i" 1
    click "$x" 78; sleep 0.5; i=$((i + 1))
  done
  overlay_close
  # The taskbar toggles change which widgets are open, which is persistent state too.
  relaunch overlay-buttons-reset
}
no_proc() { ! pgrep -x "$1" >/dev/null; }
# region_shot NAME: the region selector the click before it opened. It is up when its layer is
# mapped and has drawn the frozen screen and the regions found on it; Escape closes it.
region_shot() {
  wait_for 15 layer regionSelector || die "$1: the region selector did not open"
  shot "$1" 1
  key -k Escape; wait_for 10 no_layer regionSelector || die "$1: the region selector did not close"
  sleep 1
}
# The region selector over the idle screen. The overlay's buttons open it while the overlay closes,
# and what it then freezes depends on which of the two happens first (REPORT.md, S1).
scene_region() {
  region_warm
  ipc region screenshot; region_shot region_screenshot
  ipc region record; region_shot region_record
}
# The original's first region selection after a start freezes the screen late, with its own dimming
# and pointer hint already on it, and the hint is at the pointer or at 0,0 by chance (two runs of
# the original differ by 22000 pixels there). Every later one is clean, so one is opened and closed first.
region_warm() {
  ipc region screenshot; wait_for 15 layer regionSelector || die "the region selector did not open"
  sleep 1; key -k Escape; wait_for 10 no_layer regionSelector || die "the region selector did not close"
  sleep 1
}
scene_overlay() { overlay_open; shot overlay; overlay_close; }
scene_wallpaperSelector() { ipc wallpaperSelector toggle; shot wallpaperSelector 2.5; ipc wallpaperSelector toggle; sleep 1; }
scene_osdVolume() {
  ipc osdVolume trigger; wait_for 10 layer onScreenDisplay || die "osdVolume: no OSD"
  shot osdVolume 0.3; layer onScreenDisplay || die "osdVolume: the OSD closed before the shot"
  wait_for 15 no_layer onScreenDisplay || die "osdVolume: the OSD did not close"
}

# osd_life NAME LAYER EVENT...: how long the OSD stays after its last event, 5 times: two events
# 0.5 s apart (the second restarts the timeout), then ms from the second to the layer being gone.
# The middle of the 5 goes to osd-life.log, which compare.py compares between the two runs.
osd_life() {
  local name=$1 l=$2 t0 ms=() i; shift 2
  for i in 1 2 3 4 5; do
    "$@"; sleep 0.5; "$@"; t0=$(mono_ms)
    layer "$l" || die "$name: no OSD after the event"
    until no_layer "$l"; do [ $(( $(mono_ms) - t0 )) -lt 15000 ] || die "$name: the OSD did not close"; done
    ms+=( $(( $(mono_ms) - t0 )) ); sleep 1
  done
  echo "$name $(printf '%s\n' "${ms[@]}" | sort -n | sed -n 3p) ms from the last event to no OSD (${ms[*]})" >> "$OUT/osd-life.log"
}

# No shot: the OSD shots are taken with osd.timeout 5000 (setup.sh), so the time the OSD stays is
# measured here with the original's default, 1000.
scene_osdLife() {
  cfg .osd.timeout 1000
  osd_life osdVolume onScreenDisplay ipc osdVolume trigger
  cfg .osd.timeout 5000
}

scene_notif() {
  notify-send -a parity -u normal "Parity check" "A normal notification body"; shot notif_popup 1.2
  notify-send -a parity -u critical "Critical" "Critical urgency"; shot notif_popup2 1.2
  sleep 8
  ipc sidebarRight open; shot notif_sidebar; ipc sidebarRight close; sleep 1
}

# Opening animations at fixed frame offsets (timestamps in frames.txt).
scene_anim() {
  layer_anim sidebarRight off; layer_anim session off
  frames anim_sidebarRight 12 ipc sidebarRight open; sleep 1; ipc sidebarRight close; sleep 1.5
  frames anim_overview 12 ipc search workspacesToggle; sleep 1; ipc search workspacesToggle; sleep 1.5
  frames anim_session 12 ipc session open; sleep 1; ipc session close; sleep 1.5
  layer_anim sidebarRight on; layer_anim session on
}

scene_bar_vertical() {
  # *_live: config.json edited under a running shell (Config.qml watches it);
  # the rest after a relaunch, so rendering is compared even if live reload breaks.
  cfg .bar.vertical true; park; shot bar_vertical_live 3
  relaunch vertical; park; shot bar_vertical 1
  ipc sidebarRight open; shot bar_vertical_sidebarRight; ipc sidebarRight close; sleep 1
  cfg .bar.vertical false; relaunch horizontal
}

scene_bar_bottom() {
  # The bar is moved by an AnchorChanges of a state (Bar.qml); bar_top_live is the live edit back.
  cfg .bar.bottom true; park; shot bar_bottom_live 3
  relaunch bottom; park; shot bar_bottom 1
  cfg .bar.bottom false; park; shot bar_top_live 3
  relaunch horizontal
}

scene_bar_float() {
  # cornerStyle 1: the bar floats over a shadow whose `anchors.fill` is taken away (BarContent.qml).
  cfg .bar.cornerStyle 1; park; shot bar_float_live 3
  relaunch float; park; shot bar_float 1
  cfg .bar.cornerStyle 0; relaunch horizontal
}

# A volume change the waffle OSD shows; every second call is back at 0.45, what waffle_osd left.
waffle_volume() {
  WAFFLE_VOL=$(( 85 - ${WAFFLE_VOL:-45} ))
  wpctl set-volume @DEFAULT_AUDIO_SINK@ 0.$WAFFLE_VOL 2>/dev/null
}

scene_waffle() {
  cfg .panelFamily '"waffle"'; park; shot waffle_live 4
  relaunch waffle; park; shot waffle 1
  local spec name x y
  for spec in start:872:1054 search:916:1054 clock:1873:1056; do
    IFS=: read -r name x y <<< "$spec"
    ptr "$x" "$y"; shot "waffle_hover_$name" 1
  done
  park
  # The start menu by its button, the Task View by `search workspacesToggle`: `search toggle` opens
  # the Task View in the original (two handlers of one target, the Task View's wins) and the start
  # menu in native since #93, which gave the Task View its own target.
  click 872 1054; park; shot waffle_startMenu; click 872 1054; park; sleep 1
  ipc search workspacesToggle; shot waffle_taskView; ipc search workspacesToggle; sleep 1
  ipc sidebarLeft toggle; shot waffle_actionCenter; ipc sidebarLeft toggle; sleep 1
  ipc sidebarRight toggle; shot waffle_notificationCenter; ipc sidebarRight toggle; sleep 1
  # the original's own `osd trigger` IPC calls a function that does not exist (WaffleOSD.qml), in both
  # shells: the OSD is shown by what shows it for a user, a volume change.
  wpctl set-volume @DEFAULT_AUDIO_SINK@ 0.45 2>/dev/null
  wait_for 10 layer wOnScreenDisplay || die "waffle_osd: no OSD after a volume change"
  shot waffle_osd 0.3; layer wOnScreenDisplay || die "waffle_osd: the OSD closed before the shot"
  wait_for 15 no_layer wOnScreenDisplay || die "waffle_osd: the OSD did not close"
  cfg .osd.timeout 1000
  osd_life waffle_osd wOnScreenDisplay waffle_volume
  cfg .osd.timeout 5000
  cfg .panelFamily '"ii"'; relaunch rusty
}

# Standalone apps (`qs -p FILE`): the original runs its own file under Quickshell, native the
# `impulse-shell` config's as `$BIN -p FILE`.
app() {
  if [ "$MODE" = native ]; then hdsp "hl.dsp.exec_cmd(\"${BIN} -p $HOME/.config/rustshell/impulse-shell/$1.qml\")"
  else hdsp "hl.dsp.exec_cmd(\"qs -p $ORIG_TREE/$1.qml\")"; fi
}
app_close() { pkill -f "[-]p .*/$1.qml"; sleep 1.5; }
scene_settings() { app settings; shot settings 6; app_close settings; }
scene_welcome() { app welcome; shot welcome 6; app_close welcome; }
scene_killDialog() { app killDialog; shot killDialog 6; app_close killDialog; }

# Text-fit probe (probe_qtr.qml): SqueezedAnnotationStyledText (screen translator) and
# MaterialTextField, as a `-p` window in the config dir so `qs.*` imports resolve. Not in
# SCENES: release impulse-shell runs only compiled QML, so the native binary must be built with the
# probe copied into the tree first. Run as `run.sh probe`.
scene_probe() {
  local qs=qs tree=$ORIG_TREE
  [ "$MODE" = native ] && qs=${BIN:-$HOME/impulse-shell/target/release/impulse-shell} tree=$HOME/.config/rustshell/impulse-shell
  # The probe is written in the original's words; this shell gets it in its own.
  if [ "$MODE" = native ]; then
    sed -e 's/^import Quickshell/import Rustshell/' -e 's/^import qs[.]/import ri./' -e 's/[.]modules[.]i\{2\}[.]/.modules.rusty./' \
      "$PARITY/probe_qtr.qml" > "$tree/probe_qtr.qml"
  else cp "$PARITY/probe_qtr.qml" "$tree/probe_qtr.qml"; fi
  hdsp "hl.dsp.exec_cmd(\"$qs -p $tree/probe_qtr.qml\")"
  shot probe 5
  pkill -f "[ ]-p .*probe_qtr.qml"; sleep 1.5
}

scene_polkit() {
  hdsp "hl.dsp.exec_cmd(\"pkexec --disable-internal-agent true\")"
  shot polkit 3
  key "wrong"; key -k Return; shot polkit_wrong 4
  key -k Tab; shot polkit_unfocused 1  # the field's idle outline (Material hintTextColor)
  pkill -x pkexec; sleep 2
}

# Needs a password on the guest account (setup.sh sets PARITY_PW). What each step waits for is
# what the compositor and the screen say, the same for both shells:
# - the session lock is held, and gone after the password: a fixed 3 s shot the original still
#   locked once, with the password already accepted;
# - the typed character has changed the screen, and typing it a second time ends on the same frame
#   (shot_twice): on a cold start the original draws its first password symbol up to 2 s late, in
#   steps, and a shot of the field between two of them is a difference the next run does not have.
locked() { [ "$(hyprctl -j locked | jq .locked)" = "${1:-true}" ]; }
differs() { grim -t ppm "$TMPF/now.ppm" && ! cmp -s "$1" "$TMPF/now.ppm"; }
lock_type() { key "x"; wait_for 10 differs "$TMPF/field.ppm" || die "lock_typing: typing changed nothing on the lock screen"; }
lock_erase() { key -k BackSpace; sleep 0.5; settle "$TMPF/field.ppm" || true; }
scene_lock() {
  ipc lock activate; wait_for 30 locked || die "lock: the screen did not lock"
  shot lock 10
  cp "$OUT/lock.ppm" "$TMPF/field.ppm"
  shot_twice lock_typing 1 lock_erase lock_type
  key -k BackSpace; key "${PARITY_PW:-parity}"; key -k Return
  wait_for 30 locked false || die "lock_after_unlock: still locked 30 s after the password"
  park; shot lock_after_unlock 1
}

# A burst of Notify calls: popup frames while it is delivered, the settled popup list, and the
# grouped sidebar list.
scene_notifburst() {
  for i in $(seq 1 40); do notify-send -a burst "Burst $i" "Body of burst notification $i"; done
  frames notifburst 6 true
  shot notifburst_end 1.5
  sleep 8
  ipc sidebarRight open; shot notifburst_sidebar; ipc sidebarRight close; sleep 1
}

# The first start after a login (start_shell ... first): the original sees a new Hyprland instance and drops
# the "Keep awake" a previous login left on, so its tile is off and states.json says so; a restart
# inside the login keeps it on. Ends with a plain restart, back to the state of every other scene.
scene_firstStart() {
  relaunch firstStart first; park; shot firstStart 1
  ipc sidebarRight open; shot firstStart_sidebarRight; ipc sidebarRight close; sleep 1
  echo "firstStart: idle.inhibit=$(jq -c .idle.inhibit "$STATES")" >> "$OUT/state.log"
  KEEP_INHIBIT=1 relaunch sameLogin; park
  ipc sidebarRight open; shot sameLogin_sidebarRight; ipc sidebarRight close; sleep 1
  echo "sameLogin: idle.inhibit=$(jq -c .idle.inhibit "$STATES")" >> "$OUT/state.log"
  relaunch firstStart-reset
}
