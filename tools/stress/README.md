# Stability matrix

Scripts run inside the Arch/Hyprland test guest (never on a real desktop), copied to `~/h`.
In the guest of `bench/vm.sh`, `create` and `sync` run `tools/parity/provision.sh` and
`tools/stress/setup.sh`: the scripts are in `~/h`, `~/henv` holds the session environment every
script sources, the programs the scenarios run are installed (`wtype`, `python-evdev` and
`/dev/uinput` for the pointer, `python-gobject`, `kitty`), and the user has the password `lock.sh` types
(`$PARITY_PW`, default `parity`, as in `tools/parity`). `setup.sh` fails naming what is missing.

A scenario that cannot do its work exits 1 and says why: a program it needs is not installed
(`need` in `common.sh`), a pointer click was not made (`tap`, which is `tools/parity/ptr.py click`),
a lock round did not unlock. `crates/shell/tests/stress_scripts.rs` runs `matrix.sh` and these
cases without the guest.

`pam_faillock` counts a polkit request cancelled by `polkit.sh` as a refused password, and after
three of them refuses the right password for 10 minutes: the lock screen answers "Incorrect
password" to every round. `polkit.sh` clears the user's record when it ends and `lock.sh` when it
starts (`faillock --user USER --reset`). The shell has no part in it. Measured in the guest with
the two resets taken out, from an empty record: under `impulse-shell` and under Quickshell with the
original alike, `faillock --user` lists 1, 2 and 3 records after rounds 1, 2 and 3 of
`polkit.sh` (Cancel clicked, `pkexec` killed, Cancel clicked) and 3 after each of the other 27;
`lock.sh` after it under Quickshell unlocked 0 rounds of 15.

`lock.sh` waits for the compositor, not for a fixed time: it asks `hyprctl -j locked` until the
session is locked (20 s at most), types the password 3 s later, and asks again until the session
is unlocked (20 s at most). A round counts when the lock was taken, then let go, and the screen
1 s later is the one from before the first round. It runs no round on a session that is locked
already (exit 1): Hyprland keeps the lock of a shell that died, and every round would look at
that same screen.

Measured after `dies` (`matrix.sh MODE dies lock`, which reports `dies` as `HANG`: the shell was
killed), `faillock` empty before every run. On an idle host `impulse-shell` unlocked 15 and 14 rounds of
15 (with the fixed waits `lock.sh` had before: 3 s to the password, 4 s to the look at the
screen). With eight busy loops on the host and the waits above, `impulse-shell` unlocked 14 and 13 and
Quickshell 11, 11 and 7. Every round Quickshell did not unlock had a failed PAM check; in its
third run three of them locked the user out (`faillock`) for the last five rounds, and in its
second run two more rounds unlocked to a screen that differed. Under `impulse-shell` one round had a
failed PAM check in each run, and one round had no check at all: the password did not reach the
field. The shell handles keyboard events after it has drawn its windows (about 1 s for one
window in the guest right after it starts), the keys typed in that time are handled in one batch
with `wl_keyboard.enter`, and such keys are dropped, as Qt drops them
(`key_before_activation_equiv.rs`). Quickshell draws in other threads: typed right after a
restart, it took all 26 passwords whose keys it received, from 3 ms after the enter on; `impulse-shell`
lost 8 of the 9 typed up to 1 s after the enter and none of the 12 typed later. Without `dies`,
ten times `vm.sh sync` and `lock` at once: 150 rounds of 150.

The two modes run two trees. `native` is `impulse-shell` on `~/.config/rustshell/impulse-shell`, a copy of this
checkout's `qml`. `orig` is Quickshell on the original in the guest's `~/parity-original`, started
as `qs -p ~/parity-original`: Quickshell cannot load `qml`, which imports the `Impulse` module only
`impulse-shell` has. The original is `ref/ii` of the last commit of `main` that had it, read from git
history; the repository holds no copy. `bench/vm.sh sync` unpacks it in its guest
(`tools/parity/orig.sh`, the tree `tools/parity` and `bench` use too). For another guest, from a
checkout on the host:

```sh
SSH='ssh GUEST' tools/parity/orig.sh
```

The scripts address the running shell's IPC through `$QS` (`common.sh`): `impulse-shell -c impulse-shell` in
`native`, `qs -p ~/parity-original` in `orig`.

- `matrix.sh native|orig [scenarios]`: starts `impulse-shell` or `qs -p ~/parity-original`, samples RSS/fds/threads
  every 5 s, probes IPC latency every 2 s (`ipcprobe.sh`) and runs
  `notify_flood panels hotplug services netchurn reload windows clip volume mpris config lock polkit osk idle`,
  recording OK / FAIL / CRASH / RESTARTED / HANG, CPU, error-line and IPC-latency deltas per scenario
  in `~/stress/<mode>-<date>/` (plus coredumps and the user journal). `results.txt` there has one line
  `SCENARIO RESULT` for each. FAIL: the scenario exited other than 0 and the shell is as it was.
  CRASH: the shell left during the scenario; the next one gets a new shell. Exit 0 when all ended OK.
  `CLEAN=1` wipes notification history first; `IDLE=secs` shortens the idle phase.
- `summ.sh RUNDIR`: markdown table of the run (result, RSS/anon MB, fds, threads, CPU, errors, IPC).
- `panelleak.sh native|orig [N]`: fresh shell (`impulse-shell`, or Quickshell on `~/parity-original`), N open/close cycles per panel, anon-RSS delta per panel.
- `panelgrowth.sh PANEL [rounds] [cycles]`: native only, `RI_LOG=ri::wstat`; anon RSS, `World` table sizes and
  malloc in-use (`allocated=` of the shell's last `GC live=` line) per round.
- `shellcycle.sh native|orig [N]`: starts the shell, waits for IPC, sends SIGTERM, N times; startup ms,
  RSS/fds when up, exit time (`HANG` after 10 s) and coredumps in `~/stress/<mode>-cycle-<date>/`.
- `scr.sh native|orig [TAG]`: starts the shell and screenshots every panel into `~/scr/<mode><TAG>/`.
- `restart.sh [ENV=VAL ...]`: restarts the native shell with extra environment, log in `~/impulse-shell.log`.
- Guest needs: PipeWire null sink (`volume`), `python-gobject` (`mpris_fake.py`), ImageMagick (`lock` RMSE).
  BlueZ restarts only exercise the daemon: the guest has no adapter.
- Results of the last full run: `REPORT.md`.
- `hyprrestart.sh`: kills Hyprland under the shell.
- `longcycle.sh N`: N rounds opening/closing every panel; prints RSS and live/slot
  counts from `ri-live.txt` (written by `impulse-shell -c impulse-shell ipc call ri-debug dump` into the instance's
  `$XDG_RUNTIME_DIR/rustshell/by-id/<id>/`; the shell must run with `RI_DEBUG=dump`).
- `vis.sh OUTDIR`: arch-pc visual pass on the running native (overview, clock popup, tray menu, sidebar,
  Settings About, waffle Creativity folder). The tray icon is located on screen (left of the bar's
  unpinned dot; `vis.tray at=none` = no tray item); the Creativity folder opens via its label button,
  logged as `vis.creativity opened|NOT opened`.
