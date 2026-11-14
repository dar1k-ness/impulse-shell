#!/bin/bash
# osk.sh RUN [N]: open the on-screen keyboard, tap keys with the real pointer (q w e a s, Shift, Space, Backspace;
# 1920x1080 coordinates), close; N rounds.
RUN=$1; N=${2:-20}; . ~/h/common.sh
need python3
snap osk.before
for i in $(seq 1 $N); do
  $QS ipc call osk open >/dev/null; sleep 1
  for k in 712:886 760:886 810:886 668:986 734:936 784:936 960:1036 1312:836; do tap ${k%:*} ${k#*:}; done
  $QS ipc call osk close >/dev/null; sleep 0.5
  [ $((i%5)) = 0 ] && snap osk.$i
done
hyprctl dispatch "hl.dsp.cursor.move({x=1900,y=540})" >/dev/null
sleep 5; snap osk.after
