#!/bin/bash
RUN=$1; N=${2:-20}; . ~/h/common.sh
snap reload.before
for i in $(seq 1 $N); do hyprctl reload >/dev/null; sleep 1.5; done
sleep 5; snap reload.after
