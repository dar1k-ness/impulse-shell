#!/bin/bash
# netchurn.sh RUN [N]: NetworkManager connections appear/activate/vanish (dummy devices; ssh link untouched)
RUN=$1; N=${2:-20}; . ~/h/common.sh
need nmcli
snap net.before
for i in $(seq 1 $N); do
  sudo nmcli con add type dummy ifname dmy$i con-name stress$i ipv4.method manual ipv4.addresses 10.77.$i.1/24 >/dev/null 2>&1
  sudo nmcli con up stress$i >/dev/null 2>&1; sleep 1
  sudo nmcli con down stress$i >/dev/null 2>&1; sudo nmcli con delete stress$i >/dev/null 2>&1; sleep 0.5
done
sleep 5; snap net.after
