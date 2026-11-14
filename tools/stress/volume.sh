#!/bin/bash
# volume.sh RUN [N]: volume/mute spam (OSD) and audio sink hotplug / default-sink switching
RUN=$1; N=${2:-100}; . ~/h/common.sh
need wpctl pactl
snap vol.before
for i in $(seq 1 $N); do wpctl set-volume @DEFAULT_AUDIO_SINK@ $([ $((i%2)) = 0 ] && echo 5%+ || echo 4%-) 2>/dev/null; sleep 0.05; done
for i in $(seq 1 20); do wpctl set-mute @DEFAULT_AUDIO_SINK@ toggle; sleep 0.1; done
for i in $(seq 1 20); do
  m=$(pactl load-module module-null-sink sink_name=stress$i sink_properties=device.description=Stress$i); sleep 0.5
  pactl set-default-sink stress$i; sleep 0.5; wpctl set-volume @DEFAULT_AUDIO_SINK@ 30%; sleep 0.3
  pactl unload-module $m; sleep 0.5
done
wpctl set-volume @DEFAULT_AUDIO_SINK@ 50%; wpctl set-mute @DEFAULT_AUDIO_SINK@ 0
sleep 5; snap vol.after
