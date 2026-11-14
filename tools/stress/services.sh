#!/bin/bash
RUN=$1; N=${2:-5}; . ~/h/common.sh
snap svc.before
for i in $(seq 1 $N); do
  systemctl --user restart pipewire pipewire-pulse wireplumber; sleep 4; snap svc.pw$i
  sudo systemctl restart NetworkManager; sleep 4; snap svc.nm$i
  sudo systemctl restart bluetooth; sleep 3; snap svc.bt$i
  sudo systemctl restart upower; sleep 3; snap svc.up$i
  # stop for a while, then start (daemon absent window)
  systemctl --user stop pipewire pipewire-pulse wireplumber pipewire.socket pipewire-pulse.socket; sudo systemctl stop NetworkManager bluetooth upower; sleep 6; snap svc.down$i
  systemctl --user start pipewire.socket pipewire-pulse.socket pipewire wireplumber pipewire-pulse; sudo systemctl start NetworkManager bluetooth upower; sleep 6; snap svc.up_again$i
done
sleep 10; snap svc.after
