#!/bin/bash
# scr.sh native|orig [TAG]: screenshot every panel into ~/scr/$1$2 (BIN= overrides the native binary)
# orig is Quickshell on the original in ~/parity-original (README.md), addressed as `qs -p ~/parity-original`.
. ~/h/common.sh; M=$1; O=~/scr/$M$2; rm -rf $O; mkdir -p $O
stop_shell; cd ~
if [ $M = native ]; then (setsid ${BIN:-$HOME/impulse-shell/target/release/impulse-shell} > $O/log 2>&1 < /dev/null &)
else (setsid qs -p ~/parity-original > $O/log 2>&1 < /dev/null &); fi
if [ $M = orig ]; then QS="qs -p $HOME/parity-original"; else QS="impulse-shell -c impulse-shell"; fi
sleep 30
hyprctl dispatch "hl.dsp.focus({ workspace = 1 })" >/dev/null
shot() { sleep ${2:-2.5}; grim $O/$1.png; }
shot base 1
for t in sidebarRight sidebarLeft session cheatsheet mediaControls overlay wallpaperSelector; do
  $QS ipc call $t toggle >/dev/null 2>&1; shot $t; $QS ipc call $t toggle >/dev/null 2>&1; sleep 1.5
done
$QS ipc call search toggle >/dev/null; shot search; $QS ipc call search close >/dev/null; sleep 1.5
$QS ipc call search workspacesToggle >/dev/null; shot overview; $QS ipc call search workspacesToggle >/dev/null; sleep 1.5
$QS ipc call osdVolume trigger >/dev/null; shot osdVolume 0.6; sleep 3
notify-send -a Test "Screenshot test" "Body text for parity"; shot notif 1.5; sleep 6
echo done $(ls $O/*.png | wc -l)
