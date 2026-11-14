#!/bin/bash
# The benchmark guest (README.md "Guest"): a pinned Arch Linux cloud image under QEMU/KVM.
#   bench/vm.sh create        download and verify the image, first boot, provision (guest-setup.sh)
#   bench/vm.sh start|stop    boot / power off the guest (ssh on host port $VM_SSH_PORT, VNC :5)
#   bench/vm.sh ssh [CMD...]  run CMD in the guest as dar1k (no CMD: a shell)
#   bench/vm.sh sync          copy this checkout to guest ~/rusty-impulse, install its qml as the
#                             config `rusty-impulse`, build + install rusty-impulse (tools/install.sh), unpack
#                             the original as guest ~/parity-original (tools/parity/orig.sh) and
#                             install what tools/parity and tools/stress run (provision.sh,
#                             tools/stress/setup.sh): after it the scenarios need no more
#   bench/vm.sh copy          the first step of sync alone: the checkout to guest ~/rusty-impulse
# Host needs qemu-system-x86_64 with KVM, qemu-img, cloud-localds (cloud-image-utils), ssh, curl.
# State lives in $VM_DIR (default ~/work/vm). VM_CPUS / VM_MEM change the guest's size: CPU
# numbers are only comparable between runs with the same VM_CPUS on the same host.
set -euo pipefail
VM_DIR=${VM_DIR:-$HOME/work/vm}
VM_CPUS=${VM_CPUS:-6}
VM_MEM=${VM_MEM:-16G}
VM_SSH_PORT=${VM_SSH_PORT:-2222}
# https://geo.mirror.pkgbuild.com/images/ (Arch Linux "arch-boxes" cloud image) and its
# published SHA256.
IMG_VER=20261101.620452
IMG_SHA256=794eb4889b39e1f0ed7d5ae998b2a1ed2137db8cf8c3476cf2962c7437ecb003
IMG_URL=https://geo.mirror.pkgbuild.com/images/v$IMG_VER/Arch-Linux-x86_64-cloudimg-$IMG_VER.qcow2
repo=$(cd "$(dirname "$0")/.." && pwd)
ssh_opts=(-p "$VM_SSH_PORT" -i "$VM_DIR/id" -o StrictHostKeyChecking=no
    -o UserKnownHostsFile="$VM_DIR/known_hosts" -o LogLevel=ERROR)

guest() { ssh "${ssh_opts[@]}" dar1k@127.0.0.1 "$@"; }

running() { [ -s "$VM_DIR/qemu.pid" ] && kill -0 "$(cat "$VM_DIR/qemu.pid")" 2>/dev/null; }

start() {
    running && return
    qemu-system-x86_64 -enable-kvm -cpu host -smp "$VM_CPUS" -m "$VM_MEM" \
        -drive file="$VM_DIR/arch.qcow2",if=virtio \
        -drive file="$VM_DIR/seed.iso",format=raw,if=virtio \
        -nic user,model=virtio-net-pci,hostfwd=tcp:127.0.0.1:"$VM_SSH_PORT"-:22 \
        -device virtio-vga,xres=1920,yres=1080 -display none -vnc :5 \
        -serial file:"$VM_DIR/serial.log" \
        -device virtio-keyboard-pci -device virtio-tablet-pci \
        -daemonize -pidfile "$VM_DIR/qemu.pid"
    # QEMU's user network accepts the forwarded port before the guest's sshd listens, so the
    # only signal the host has is a login that works.
    local i
    for i in $(seq 1 300); do
        guest true 2>/dev/null && return
        running || { echo "vm.sh: qemu exited; see $VM_DIR/serial.log" >&2; exit 1; }
        sleep 1
    done
    echo "vm.sh: no ssh login after 300 s; see $VM_DIR/serial.log" >&2
    exit 1
}

stop() {
    running || return 0
    local pid
    pid=$(cat "$VM_DIR/qemu.pid")
    guest sudo systemctl poweroff 2>/dev/null || true
    # The power-off ends the qemu process; tail --pid returns when it is gone.
    tail --pid="$pid" -f /dev/null
}

# Hyprland is started by the autologin on tty1; it writes its lock once its sockets listen.
wait_session() {
    guest 'until ls /run/user/$(id -u)/hypr/*/hyprland.lock >/dev/null 2>&1; do sleep 1; done'
}

# This checkout (without target/ and .git) to guest ~/rusty-impulse, with its commit in ~/rusty-impulse/.commit
# and, as the build names it (the guest's binary is then the same file as a build of this
# checkout anywhere else), in ~/rusty-impulse/.build-commit;
# the guest's target/ is kept. The files arrive with the times they have here, and cargo knows
# a changed file by its time alone: a tree older than the guest's last build (an older commit,
# a file removed from a directory a build script lists) would keep that build. So, as the gate
# does for its checkouts, tools/ci/tree-times gives what differs from the tree built last a
# new time (its record: ~/rusty-impulse/target/ri-gate-tree), and the build equals a clean one.
copy() {
    tar -C "$repo" -czf - --exclude=./target --exclude=./.git . |
        guest 'mkdir -p ~/rusty-impulse/target && cd ~/rusty-impulse &&
            find . -mindepth 1 -maxdepth 1 ! -name target -exec rm -rf {} + && tar xzf -'
    git -C "$repo" describe --always --dirty --abbrev=12 | guest 'cat >~/rusty-impulse/.commit'
    "$repo/tools/build.sh" --commit | guest 'cat >~/rusty-impulse/.build-commit'
    guest 'python3 ~/rusty-impulse/tools/ci/tree-times ~/rusty-impulse ~/rusty-impulse/target/ri-gate-tree'
}

# What the scenarios of tools/parity and tools/stress run that guest-setup.sh does not install
# (wtype, python-evdev and /dev/uinput for the pointer, python-gobject, kitty, the user's
# password).
# Both scripts leave alone what is there already. provision.sh first takes the clock back from
# the date tools/parity pins: under that date no package or crate downloads.
tools() {
    guest 'bash ~/rusty-impulse/tools/parity/provision.sh && ~/rusty-impulse/tools/stress/setup.sh'
}

case ${1:-} in
create)
    mkdir -p "$VM_DIR"
    [ ! -e "$VM_DIR/arch.qcow2" ] || { echo "vm.sh: $VM_DIR/arch.qcow2 exists" >&2; exit 1; }
    [ -e "$VM_DIR/base.qcow2" ] || curl -fL -o "$VM_DIR/base.qcow2" "$IMG_URL"
    echo "$IMG_SHA256  $VM_DIR/base.qcow2" | sha256sum -c -
    qemu-img create -q -f qcow2 -b "$VM_DIR/base.qcow2" -F qcow2 "$VM_DIR/arch.qcow2" 60G
    [ -e "$VM_DIR/id" ] || ssh-keygen -q -t ed25519 -N '' -f "$VM_DIR/id"
    cat >"$VM_DIR/user-data" <<EOF
#cloud-config
hostname: ri-bench
users:
  - name: dar1k
    groups: [wheel, video, input]
    sudo: ALL=(ALL) NOPASSWD:ALL
    shell: /bin/bash
    ssh_authorized_keys:
      - $(cat "$VM_DIR/id.pub")
EOF
    printf 'instance-id: ri-bench\nlocal-hostname: ri-bench\n' >"$VM_DIR/meta-data"
    cloud-localds "$VM_DIR/seed.iso" "$VM_DIR/user-data" "$VM_DIR/meta-data"
    start
    guest sudo cloud-init status --wait
    # The package snapshot of the test image: the guest runs the Quickshell, Qt and Hyprland
    # the gate compares against.
    snapshot=$(grep -o 'archive.archlinux.org/repos/[0-9/]*' "$repo/tools/testenv/Containerfile" |
        cut -d/ -f3-5)
    copy
    guest "~/rusty-impulse/bench/guest-setup.sh $snapshot"
    # Reboot into the installed kernel, Mesa and the autologin session. `tools` comes after it:
    # the kernel that booted the image has lost its modules to the upgrade, and provision.sh
    # loads one (uinput).
    stop
    start
    wait_session
    tools
    ;;
start)
    start
    wait_session
    ;;
stop) stop ;;
ssh)
    shift
    guest "$@"
    ;;
copy)
    start
    copy
    ;;
sync)
    start
    wait_session
    copy
    tools
    # install.sh compiles the checkout's qml into the binary and makes
    # ~/.config/rustshell/rusty-impulse a copy of it, as on the PC.
    guest 'rm -rf ~/.config/rustshell/rusty-impulse ~/.local/state/rusty-impulse && cd ~/rusty-impulse && tools/install.sh'
    # The original, which the benchmarks measure rusty-impulse against: the tree tools/parity
    # compares the view with, unpacked the same way (guest ~/parity-original).
    "$repo/tools/parity/orig.sh"
    ;;
*)
    sed -n '2,15p' "$0" >&2
    exit 2
    ;;
esac
