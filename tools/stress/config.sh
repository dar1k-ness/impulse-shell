#!/bin/bash
# config.sh RUN [N]: live edits of ~/.config/impulse-shell/config.json (bar position/style, transparency); restored at the end
RUN=$1; N=${2:-12}; . ~/h/common.sh
C=~/.config/impulse-shell/config.json; cp $C $RUN/config.bak
snap cfg.before
for i in $(seq 1 $N); do
  jq ".bar.bottom = $([ $((i%2)) = 1 ] && echo true || echo false) | .bar.cornerStyle = $((i%3)) | .appearance.transparency.enable = $([ $((i%4)) -lt 2 ] && echo true || echo false)" $RUN/config.bak > $C.tmp && mv $C.tmp $C
  sleep 3; snap cfg.$i
done
cp $RUN/config.bak $C; sleep 6; snap cfg.after
