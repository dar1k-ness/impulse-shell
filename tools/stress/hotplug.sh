#!/bin/bash
RUN=$1; N=${2:-30}; . ~/h/common.sh
snap hotplug.before
for i in $(seq 1 $N); do
  hyprctl output create headless HP$i >/dev/null; sleep 1.5
  [ $((i%3)) = 0 ] && { hyprctl eval "hl.monitor({ output = \"HP$i\", mode = \"1280x720@60\", position = \"auto\", scale = 1.5 })" >/dev/null; sleep 1; }
  # open a panel while the extra output exists
  [ $((i%5)) = 0 ] && { $QS ipc call sidebarRight toggle >/dev/null; sleep 0.5; $QS ipc call sidebarRight toggle >/dev/null; }
  hyprctl output remove HP$i >/dev/null; sleep 1.5
done
# two at once, then remove in reverse
hyprctl output create headless HPA >/dev/null; hyprctl output create headless HPB >/dev/null; sleep 2
hyprctl output remove HPB >/dev/null; sleep 0.2; hyprctl output remove HPA >/dev/null; sleep 5
snap hotplug.after
