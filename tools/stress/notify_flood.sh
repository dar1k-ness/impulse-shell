#!/bin/bash
# N notifications as fast as possible, half with replace ids / actions / icons / long bodies
RUN=$1; N=${2:-${FLOOD:-1000}}; . ~/h/common.sh
need notify-send gdbus
snap notif.before
for i in $(seq 1 $N); do
  case $((i%5)) in
   0) notify-send -a flood "Flood $i" "body $i" ;;
   1) notify-send -a flood -u critical -i dialog-information "Crit $i" "$(head -c 600 /dev/urandom | base64 -w0)" ;;
   2) gdbus call --session -d org.freedesktop.Notifications -o /org/freedesktop/Notifications -m org.freedesktop.Notifications.Notify app 0 "" "Act $i" "with actions" "['a','Open','b','Dismiss']" "{}" 5000 >/dev/null ;;
   3) gdbus call --session -d org.freedesktop.Notifications -o /org/freedesktop/Notifications -m org.freedesktop.Notifications.Notify app 7 "" "Replace $i" "replaces id 7" "[]" "{}" 3000 >/dev/null ;;
   4) notify-send -a flood -t 500 -h string:image-path:/usr/share/icons/hicolor/48x48/apps/kitty.png "Img $i" "image" ;;
  esac &
  [ $((i%50)) = 0 ] && wait
done; wait
snap notif.sent
sleep 20; snap notif.settled
