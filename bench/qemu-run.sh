#!/bin/bash
cd ~/work/vm
exec qemu-system-x86_64 -enable-kvm -cpu host -smp 6 -m 8G -drive file=arch.qcow2,if=virtio -drive file=seed.iso,format=raw,if=virtio \
 -nic user,model=virtio-net-pci,hostfwd=tcp::2222-:22 -device virtio-vga,xres=1920,yres=1080 -display none -vnc :5 -serial file:serial.log \
 -device virtio-keyboard-pci -device virtio-tablet-pci -daemonize -pidfile qemu.pid
