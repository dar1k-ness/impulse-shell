# VM benchmarks (native rusty-impulse vs the original in Quickshell)

All numbers in PRs come from this setup; use it unchanged so runs are comparable.

## Guest
- Arch Linux cloud image (qcow2) under QEMU/KVM: `qemu-run.sh` (6 vCPU, 8 GB, virtio-vga 1920x1080, VNC :5, ssh on host port 2222). cloud-init user `dar1k` (wheel, video, input, seat; passwordless sudo).
- Packages: hyprland, quickshell (provides `/usr/bin/qs`), the shell's QML at `~/.config/rustshell/rusty-impulse` (a copy of `qml`), libnotify, perf, rust toolchain. Rendering is Mesa llvmpipe (no DRM GPU in the VM).
- Hyprland runs on the virtio-vga output; the scripts kill any running `qs`/`rusty-impulse` and start the one under test.
- Building the full shell (crate `ricompiled`) needs ~7.5 GB rustc RSS: add swap in the guest (btrfs: `btrfs filesystem mkswapfile --size 12g /swapfile && swapon /swapfile`).
- `gsync.sh` copies the host checkout to guest `~/rusty-impulse` (tar, no delete: remove stale files by hand) and builds there.

## Scripts (copy to guest `/tmp/`)
- `vflood.sh ORIG|NATIVE N K` — seed N history notifications, send K in parallel, poll IPC (`ipc call cliphistService update`, sent by `qs` to the original and by `rusty-impulse` to the native shell). Prints send time, max IPC latency, window, process CPU ticks, liveness. `BIN=<path>` selects the native binary, `WAIT` = startup wait (use 40). Standard: `WAIT=40 bash /tmp/vflood.sh NATIVE 50 300`; `RI_INTERP=1` = interpreter instead of compiled JS.
- `ORIG` is Quickshell running the original, `qs -p ~/parity-original`: `vm.sh sync` unpacks that tree (`tools/parity/orig.sh`). The `rusty-impulse` config is the project's own `qml`, which Quickshell cannot load. A script that finds no shell process after the startup wait says so and exits 1.
- `vswipe.sh NATIVE` — open-drawer swipe with 300 history; `vburst.sh` — 40-notification popup burst.
- Visual parity: run swipe and burst with `RI_DEBUG=dlcheck,dmgcheck,inccheck,laycheck`; expected 0 `dmgcheck ... bad [1-9]`, all `inccheck ok`, `laycheck-settle ... []` only.
- Ticks are noisy (±3%): run each variant at least twice, alternating A/B.

## Memory and CPU: `rss-cpu.sh`
RSS and CPU of `rusty-impulse` and of the original in Quickshell, idle, with both sidebars open and
over a wallpaper switch. Method and the recorded numbers: [docs/benchmarks.md](../docs/benchmarks.md).
It has its own guest, built from pinned inputs by `vm.sh` (the guest above was set up by hand):

```sh
bench/vm.sh create                      # once: image, packages, Hyprland on tty1 (guest-setup.sh, hyprland.lua)
bench/vm.sh sync                        # this checkout -> guest, qml -> ~/.config/rustshell/rusty-impulse, build + install rusty-impulse, the original -> ~/parity-original,
                                        # what tools/parity and tools/stress run (tools/parity/provision.sh, tools/stress/setup.sh)
bench/vm.sh ssh rusty-impulse/bench/rss-cpu.sh  # about 25 minutes; prints the table, results in guest ~/rss-cpu/<date>
bench/vm.sh stop
```

`sync` of any checkout (a newer or an older commit, files added or removed) leaves a build in the
guest equal to a clean one: the guest's `target` is kept, and what differs from the checkout built
last gets a new modification time there (`tools/ci/tree-times`, `docs/ci.md`; test:
`crates/shell/tests/bench_vm_copy.rs`).
