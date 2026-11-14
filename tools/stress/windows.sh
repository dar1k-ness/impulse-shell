#!/bin/bash
# windows.sh RUN [ROUNDS]: window open/close churn across workspaces, fullscreen, rapid workspace switching, overview with windows
RUN=$1; R=${2:-8}; . ~/h/common.sh
need kitty
snap win.before
for r in $(seq 1 $R); do
  for i in $(seq 1 8); do hws $((i%5+1)); kitty --class stressw$i sleep 20 >/dev/null 2>&1 & sleep 0.4; done
  sleep 3
  hws 1; hyprctl dispatch 'hl.dsp.window.fullscreen({ mode = "fullscreen", action = "toggle" })' >/dev/null; sleep 1
  hyprctl dispatch 'hl.dsp.window.fullscreen({ mode = "fullscreen", action = "toggle" })' >/dev/null
  hglobal overviewWorkspacesToggle; sleep 1.5; hglobal overviewWorkspacesToggle; sleep 0.5
  for j in $(seq 1 60); do hws $((j%10+1)); sleep 0.05; done
  hws 1; pkill -x kitty; sleep 2
  snap win.r$r
done
sleep 5; snap win.after
