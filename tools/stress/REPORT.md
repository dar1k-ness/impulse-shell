# Final stress gate (2026-10-29, `9dbae36`: #95 #96 #98 #99)

Guest as below (QEMU Arch, Hyprland 0.56.2, Quickshell 0.3.2, Qt 6.12, llvmpipe, 8 vCPU / 20 GB).

```
TAG=gate FLOOD=5000 BIN=~/impulse-shell-gate ~/h/matrix.sh native; TAG=gate FLOOD=5000 ~/h/matrix.sh orig
BIN=~/impulse-shell-gate ~/h/shellcycle.sh native 20; ~/h/shellcycle.sh orig 20
```

## Verdict

- Crashes / hangs: **none in either**. 15/15 scenarios OK per shell, no coredumps, no IPC timeouts, no restart;
  20/20 start/SIGTERM cycles per shell, every exit in 0.5 s.
- No native-only defect found on this head; nothing to fix.
- fds/threads over the whole run: native 36→39 / 49→54, original 69→71 / 40→37.
- Notification flood (5000): native 28 s, 2691 ticks, 288 ms max IPC; original 371 s, 36801 ticks, 3103 ms.
- Errors: native 163 (1 in `panels`, 162 in `hotplug`: the known output-removal binding errors); original 1422.
- End of run RSS: native 1410 MB, original 1591 MB.

| scenario | native RSS MB (peak) | native anon | native CPU / err / IPC max | orig RSS MB (peak) | orig anon | orig CPU / err / IPC max |
|---|---|---|---|---|---|---|
| notify_flood | 330→374 (374) | 200→241 | 2691 / 0 / 288 | 1077→1265 (1801) | 888→1076 | 36801 / 191 / 3103 |
| panels | 923→1234 (1324) | 789→1105 | 25046 / 1 / 2147 | 1265→1582 (1732) | 1076→1357 | 16327 / 853 / 409 |
| hotplug | 1299→1294 (1315) | 1159→1162 | 8385 / 162 / 1348 | 1875→1999 (2003) | 1632→1795 | 1937 / 336 / 431 |
| services | 1270→1269 | 1138→1138 | 199 / 0 / 277 | 1867→1836 | 1663→1645 | 92 / 10 / 239 |
| netchurn | 1269→1272 | 1138→1141 | 98 / 0 / 152 | 1847→1879 | 1656→1690 | 23 / 0 / 154 |
| reload | 1273→1278 | 1142→1147 | 58 / 0 / 102 | 1886→1844 | 1697→1653 | 43 / 0 / 147 |
| windows | 1281→1313 (1398) | 1149→1181 | 2459 / 0 / 199 | 1839→1476 (1978) | 1648→1292 | 1822 / 10 / 318 |
| clip | 1313→1326 | 1181→1194 | 1453 / 0 / 492 | 1406→1368 | 1219→1182 | 621 / 4 / 212 |
| volume | 1326→1330 | 1194→1197 | 285 / 0 / 144 | 1410→1377 | 1220→1186 | 129 / 0 / 230 |
| mpris | 1326→1329 | 1194→1197 | 2223 / 0 / 130 | 1429→1354 | 1234→1162 | 1324 / 10 / 320 |
| config | 1331→1328 | 1199→1196 | 129 / 0 / 108 | 1339→1339 | 1147→1151 | 55 / 0 / 206 |
| lock | 1367→1381 | 1227→1224 | 1167 / 0 / 131 | 1434→1443 | 1220→1239 | 8904 / 1 / 453 |
| polkit | 1381→1406 | 1224→1250 | 566 / 0 / 129 | 1521→1468 | 1294→1261 | 14862 / 0 / 375 |
| osk | 1407→1397 | 1251→1240 | 531 / 0 / 130 | 1468→1485 | 1261→1278 | 3081 / 0 / 236 |
| idle | 1397→1410 (1437) | 1240→1245 | 776 / 0 / 130 | 1486→1591 (1718) | 1278→1381 | 8562 / 7 / 314 |

### Shell restart (`shellcycle.sh`, 20 each)

| | start to first IPC reply ms (min/med/max) | exit after SIGTERM | RSS 5 s after start | hangs | coredumps |
|---|---|---|---|---|---|
| native | 1127 / 1142 / 1397 | 0.5 s every time | 395 MB | 0 | 0 |
| original | 1564 / 1576 / 1850 | 0.5 s every time | 1190 MB | 0 | 0 |

### Sockets

Both shells leave one `by-id/<id>/{instance.lock,ipc.sock}` per start (7-8 per minute in either shell's 20-cycle
run); none of them is listening after exit, and `qs` treats unlocked instances as dead. The native matches
Quickshell here, nothing native-only.

### `panels` memory

Native anon RSS steps up within the first round of panel opens and then stays flat with jitter (gate: 789 →
1063 at 22 s, 1121-1170 for the remaining 300 s). Not a per-open leak. The step depends on the build, fresh shell,
`notify_flood` (5000) then `panels`:

| native build | flood anon MB | panels anon MB (plateau) |
|---|---|---|
| #81 (`impulse-shell-fix`) | 162→222 | 263→468 (457-527) |
| #87 head (`impulse-shell-h87`) | 375→1189 | 1189→1188 (1188-1267) |
| ab08bca (#91) | 162→309 | 309→1272 (1205-1293) |
| 9dbae36 gate (after full flood run) | 200→241 | 789→1105 (1063-1170) |

So the current builds sit ~0.7 GB above the #81 build once the history holds 5000 notifications (the h87 shell
already reached that level during the flood). Still below the original (1357 anon after `panels`). Cause not isolated yet.

### Guest-only: interpreted fallback

The guest's `~/.local/share/impulse-shell` is older than `qml` (no assistant chat; `Persistent.qml`,
`LauncherSearch.qml`, `LeftSidebarButton.qml`, `SidebarLeftContent.qml`, OSK files differ), so those files run
interpreted. Builds made later the same day from the same sources (`9dbae36`, `2577151`, `685f425`) then log
`Persistent is not defined` / `LauncherSearch is not defined` at startup, while the `9dbae36` gate binary built at
15:15 does not; with `Persistent.qml` copied from `qml` the `Persistent` errors disappear. Configs that match
`qml` (arch-pc: all five files identical) run compiled and are not affected. Open: why the interpreted
`Persistent` fails in the later builds.

# Previous run: current-head rerun (2026-10-29, `ee0ad68`, after #80/#82)

Build: `ee0ad68` (#79-#83 merged), guest workspace `cargo build --release`. Same guest and
scripts as the gate run below.

```
BIN=~/impulse-shell-head TAG=head FLOOD=5000 ~/h/matrix.sh native; TAG=head FLOOD=5000 ~/h/matrix.sh orig
~/h/shellcycle.sh native 20; ~/h/shellcycle.sh orig 20
```

## Verdict

- Crashes / hangs: **none in either**. 15/15 scenarios OK per shell, no coredumps, no IPC timeouts;
  20/20 start/SIGTERM cycles per shell, all exit in 0.5 s.
- Native RSS over the whole matrix: 301→735 MB (anon 180→575); before #80/#82 (gate run below) 326→1020 MB.
  `panels` now adds +220 MB (was +511 MB before #81). Original: 1098→1899 MB.
- fds/threads: native 36→39 / 49→53; original 69→71 / 40→37.
- Idle CPU (`cpu_pct`): native bar 0.76, sidebarRight 0.53, overview 1.32; original 10.24, 8.18, 13.90.
- Flood (5000): native 29 s, 2592 ticks, 334 ms max IPC; original 227 s, 21842 ticks, 2848 ms.
- Errors: native 169 (168 in `hotplug`, the known output-removal binding errors); original 1498.

## Native (`native-head-0115-1537`)

| scenario | RSS MB | anon MB | fds | threads | CPU s | errors | max IPC ms/timeouts | dur |
|---|---|---|---|---|---:|---:|---|---:|
| notify_flood | 301→380 | 180→247 | 36→36 | 49→49 | 25.8 | 0 | 334/0 | 29s |
| panels | 380→600 | 247→472 | 36→36 | 49→51 | 129.5 | 1 | 264/0 | 301s |
| hotplug | 600→633 | 472→504 | 36→36 | 51→52 | 49.0 | 168 | 1322/0 | 114s |
| services | 633→642 | 504→514 | 36→36 | 52→52 | 1.6 | 0 | 178/0 | 144s |
| netchurn | 642→643 | 514→514 | 36→36 | 52→52 | 0.7 | 0 | 110/0 | 40s |
| reload | 643→644 | 514→516 | 36→36 | 52→52 | 0.5 | 0 | 129/0 | 36s |
| windows | 644→667 | 516→538 | 36→39 | 52→53 | 22.3 | 0 | 183/0 | 126s |
| clip | 667→633 | 538→503 | 39→39 | 53→53 | 11.9 | 0 | 154/0 | 42s |
| volume | 633→632 | 503→503 | 39→39 | 53→53 | 2.5 | 0 | 97/0 | 50s |
| mpris | 632→636 | 503→506 | 39→39 | 53→53 | 19.8 | 0 | 106/0 | 97s |
| config | 636→636 | 506→507 | 39→39 | 53→53 | 1.1 | 0 | 97/0 | 42s |
| lock | 636→677 | 507→525 | 39→39 | 53→53 | 10.9 | 0 | 127/0 | 121s |
| polkit | 677→688 | 525→535 | 39→39 | 53→53 | 4.3 | 0 | 129/0 | 302s |
| osk | 688→682 | 535→529 | 39→39 | 53→53 | 4.6 | 0 | 119/0 | 298s |
| idle | 682→735 | 529→575 | 39→39 | 53→53 | 7.1 | 0 | 133/0 | 849s |

## Original (`orig-head-0115-1621`)

| scenario | RSS MB | anon MB | fds | threads | CPU s | errors | max IPC ms/timeouts | dur |
|---|---|---|---|---|---:|---:|---|---:|
| notify_flood | 1098→1269 | 917→1089 | 69→72 | 40→39 | 218.4 | 244 | 2848/0 | 227s |
| panels | 1269→1367 | 1089→1150 | 72→74 | 39→41 | 160.7 | 862 | 353/0 | 302s |
| hotplug | 1367→1903 | 1150→1704 | 74→74 | 41→41 | 19.4 | 342 | 487/0 | 114s |
| services | 1903→1768 | 1704→1580 | 74→70 | 41→37 | 0.9 | 10 | 124/0 | 144s |
| netchurn | 1768→1770 | 1580→1579 | 70→70 | 37→37 | 0.3 | 0 | 139/0 | 41s |
| reload | 1770→1750 | 1579→1562 | 70→70 | 37→37 | 0.3 | 0 | 129/0 | 35s |
| windows | 1750→1320 | 1562→1130 | 70→71 | 37→37 | 24.9 | 10 | 219/0 | 127s |
| clip | 1320→1263 | 1130→1070 | 71→71 | 37→37 | 9.1 | 4 | 113/0 | 41s |
| volume | 1263→1280 | 1070→1084 | 71→71 | 37→37 | 1.9 | 0 | 118/0 | 50s |
| mpris | 1280→1281 | 1084→1086 | 71→74 | 37→40 | 21.7 | 18 | 139/0 | 98s |
| config | 1281→1322 | 1086→1131 | 74→71 | 40→37 | 0.6 | 0 | 96/0 | 42s |
| lock | 1322→1391 | 1131→1182 | 71→71 | 37→37 | 110.0 | 1 | 293/0 | 122s |
| polkit | 1391→1633 | 1182→1424 | 71→75 | 37→41 | 161.7 | 0 | 362/0 | 303s |
| osk | 1633→1779 | 1424→1567 | 75→71 | 41→37 | 30.6 | 0 | 290/0 | 298s |
| idle | 1779→1899 | 1567→1687 | 71→71 | 37→37 | 89.8 | 7 | 304/0 | 849s |

## Shell restart (20 each)

| | start to first IPC reply ms (min/med/max) | exit after SIGTERM | hangs | coredumps |
|---|---|---|---|---|
| native | 797 / 810 / 1183 | 0.5 s every time | 0 | 0 |
| original | 1353 / 1367 / 1479 | 0.5 s every time | 0 | 0 |

# Previous run: long stability gate (2026-10-29, before #80/#82)

Build: the development branch with #79 merged, before #80/#82 (guest workspace build `cargo build --release`).
Guest: QEMU Arch, Hyprland 0.56.2 (Lua config), Quickshell 0.3.2, Qt 6.12, llvmpipe, 8 vCPU / 20 GB. Shells are
started by Hyprland (`launch.sh` via `exec_cmd`) so they live in the graphical logind session (needed by polkit).

```
TAG=long FLOOD=5000 ~/h/matrix.sh native; TAG=long FLOOD=5000 ~/h/matrix.sh orig
~/h/shellcycle.sh native 20; ~/h/shellcycle.sh orig 20
BIN=~/impulse-shell-base CLEAN=1 TAG=abbase ~/h/matrix.sh native panels osk    # #81 A/B
BIN=~/impulse-shell-fix  CLEAN=1 TAG=abfix  ~/h/matrix.sh native panels osk
```

Default scenario list now: `notify_flood panels hotplug services netchurn reload windows clip volume mpris config
lock polkit osk idle` (new: `polkit`, `osk`; `notify_flood` takes `FLOOD`, default 1000).

## Verdict

- Crashes / hangs: **none in either**. 15/15 scenarios OK per shell, no coredumps, no IPC timeouts, no restart;
  20/20 shell start/SIGTERM cycles per shell, all exit in 0.5 s.
- Native-only defects found and fixed:
  1. polkit: every cancelled request left a `polkit-agent-helper-1`, its socket and a reader thread (#79).
  2. runtime: released templates kept their creation plan and post copy (`World::plans`/`posts`, from #73);
     every Loader activation of an inline template left a `Props` clone + AST behind (#81). This is the native
     `panels` growth below (+511 MB here vs +150 MB in the previous rerun, before #73).
- fds/threads: native 36→39 / 53→54 over the whole run; original 69→71 / 40→37.
- Idle CPU (`cpu_pct`, ticks/s): native bar 0.78, sidebarRight 0.62, overview 1.82; original 10.26, 8.19, 15.62.
- Notification flood (5000): native 29 s, 2739 ticks, 392 ms max IPC; original 253 s, 24494 ticks, 3186 ms.

## Native (`native-long-0115-1302`)

| scenario | RSS MB | anon MB | fds | threads | CPU ticks | errors | max IPC ms/timeouts | dur |
|---|---|---|---|---|---:|---:|---|---:|
| notify_flood | 326→428 | 197→296 | 36→36 | 53→53 | 2739 | 0 | 392/0 | 29s |
| panels | 428→939 | 296→807 | 36→36 | 53→53 | 15852 | 1 | 405/0 | 305s |
| hotplug | 939→983 | 807→850 | 36→36 | 53→53 | 5791 | 186 | 1457/0 | 113s |
| services | 983→983 | 850→850 | 36→36 | 53→53 | 160 | 0 | 160/0 | 145s |
| netchurn | 983→985 | 850→852 | 36→36 | 53→53 | 120 | 0 | 231/0 | 57s |
| reload | 985→986 | 852→853 | 36→36 | 53→53 | 65 | 0 | 480/0 | 36s |
| windows | 986→1003 | 853→878 | 36→39 | 53→54 | 2231 | 0 | 392/0 | 132s |
| clip | 1003→1017 | 878→892 | 39→39 | 54→54 | 1458 | 0 | 467/0 | 43s |
| volume | 1017→1027 | 892→894 | 39→39 | 54→54 | 253 | 0 | 271/0 | 56s |
| mpris | 1027→1027 | 894→894 | 39→39 | 54→54 | 1562 | 0 | 306/0 | 99s |
| config | 1027→1027 | 894→894 | 39→39 | 54→54 | 142 | 0 | 191/0 | 43s |
| lock | 1027→979 | 894→822 | 39→39 | 54→54 | 915 | 0 | 290/0 | 128s |
| polkit | 979→1016 | 822→851 | 39→39 | 54→54 | 497 | 0 | 275/0 | 306s |
| osk | 1016→988 | 851→831 | 39→39 | 54→54 | 512 | 0 | 111/0 | 298s |
| idle | 988→1020 | 831→855 | 39→39 | 54→54 | 783 | 0 | 200/0 | 849s |

## Original (`orig-long-0115-1346`)

| scenario | RSS MB | anon MB | fds | threads | CPU ticks | errors | max IPC ms/timeouts | dur |
|---|---|---|---|---|---:|---:|---|---:|
| notify_flood | 1059→1245 | 869→1056 | 69→72 | 40→39 | 24494 | 248 | 3186/0 | 253s |
| panels | 1245→1442 | 1056→1214 | 72→74 | 39→41 | 17405 | 835 | 360/0 | 318s |
| hotplug | 1442→1882 | 1214→1680 | 74→74 | 41→41 | 2017 | 327 | 456/0 | 114s |
| services | 1882→1660 | 1680→1467 | 74→70 | 41→37 | 92 | 10 | 293/0 | 144s |
| netchurn | 1660→1705 | 1467→1509 | 70→70 | 37→37 | 27 | 0 | 139/0 | 40s |
| reload | 1705→1734 | 1509→1538 | 70→70 | 37→37 | 30 | 0 | 110/0 | 36s |
| windows | 1734→1573 | 1538→1379 | 70→71 | 37→37 | 2524 | 10 | 278/0 | 127s |
| clip | 1573→1512 | 1379→1320 | 71→71 | 37→37 | 955 | 4 | 130/0 | 42s |
| volume | 1512→1520 | 1320→1325 | 71→71 | 37→37 | 203 | 0 | 138/0 | 50s |
| mpris | 1520→1536 | 1325→1339 | 71→75 | 37→41 | 2167 | 10 | 130/0 | 97s |
| config | 1536→1551 | 1339→1356 | 75→71 | 41→37 | 63 | 0 | 111/0 | 43s |
| lock | 1551→1716 | 1356→1505 | 71→71 | 37→37 | 10289 | 1 | 476/0 | 121s |
| polkit | 1716→1914 | 1505→1704 | 71→75 | 37→41 | 14724 | 0 | 554/0 | 303s |
| osk | 1904→1958 | 1693→1744 | 75→71 | 41→37 | 3285 | 0 | 269/0 | 299s |
| idle | 1958→2026 | 1744→1812 | 71→71 | 37→37 | 9206 | 7 | 411/0 | 849s |

Native error count jumps only in `hotplug` (186: the same output-removal binding errors as the previous runs);
the original logs 248/835/327 errors in flood/panels/hotplug.

## #81 A/B (fresh native, `panels` + `osk`)

40 open/close rounds of every panel, then 20 OSK rounds, fresh shell each:

| native | panels RSS MB | panels anon MB | osk anon MB | panels CPU ticks |
|---|---|---|---|---:|
| base (before #81) | 289→706 | 168→577 | 577→575 | 8159 |
| #81 | 291→511 | 169→378 | 378→375 | 7453 |

With #81 the panels RSS reaches ~540 MB within the first round (first-open warm-up: fonts, images, plans of
the live templates) and then oscillates 495-560 MB for the remaining 39 rounds; before #81 it kept climbing.

## polkit

`polkit.sh` (30 requests, alternating Cancel click / `pkexec` kill): both shells `ended_ok=15/30 stuck=15
helpers=0->0 leftover_pkexec=0` (native sockets 13→13, original 8→8). The 15 "stuck" are the Cancel-click
rounds, identical in both shells: the scripted click does not land on the dialog's button in the guest; the
`pkexec` kill that follows ends them cleanly and nothing is left behind. Password entry is not covered: neither
`wtype` nor a uinput keyboard reaches the password field in the guest, for either shell.

## Shell restart (`shellcycle.sh`, 20 each)

| | start to first IPC reply ms (min/med/max) | exit after SIGTERM | hangs | coredumps |
|---|---|---|---|---|
| native | 737 / 786 / 1636 | 0.5 s every time | 0 | 0 |
| original | 1239 / 1257 / 1366 | 0.5 s every time | 0 | 0 |

## Idle growth note

Native anon RSS rose ~88 MB during the first 4 min of `idle` (bar only) after `osk`, then stayed flat to the byte
for the remaining 6 min and through the sidebar/overview phases; not a leak. The original grows 68 MB in `idle`.

# Previous run: compiled `impulse-shell` vs `qs -c ii` (2026-10-28)

Build: the development branch with `ri-native` merged (local merge `4e01cae`; it differs from the
official merge `1b4e8b1` only in `types.rs` lookup perf and the `loadFailed` code path, no lifecycle change).
`cargo build --release -p shell`. Guest: QEMU Arch, Hyprland 0.56.2 (Lua config), Quickshell 0.3.2, Qt 6.12,
llvmpipe, 8 vCPU / 20 GB. Same scenario order, one long-lived shell per mode, `CLEAN=1`.

```
CLEAN=1 TAG=full ~/h/matrix.sh orig|native notify_flood panels hotplug services netchurn reload \
  windows clip volume mpris config lock idle
~/h/panelleak.sh orig|native 40
```

## Verdict

- Crashes / hangs: **none in either** (13/13 scenarios OK, no coredumps, no IPC timeouts, no restart).
- Leaks: native holds flat after warm-up (~1.1 GB, fds 39-46, threads 53-55) through 10 later scenarios and
  14 min of idle; original grows 1.07 -> 2.13 GB (hotplug +600 MB, idle +540 MB), fds/threads stable.
  Native grows during `panels` only: issue 3 below (linear in the number of panel opens).
- Native-only defects found by this first run (all from `crates/qml/src/runtime.rs`; all three fixed since, see the rerun):
  1. `SearchItem` delegates lose their `entry` (2686 x2 errors) – parentless objects freed while referenced.
  2. Bare builtin item methods (`positionViewAtEnd`) are `undefined` as values.
  3. Every panel open leaves ~2-5 MB of dead-instance bookkeeping behind (ids/contexts never reused).

## Rerun after the runtime fixes (item 9, native `93f7fe7`)

Issues 1-3 below are fixed: reachability for `createObject(null)` (`0c4f244`), builtin
methods as values (`d2c8ce2`), panel-open leak (`88780ee` GPU mask layers, `d7c21d8` chunked id/ctx storage,
`6b4b34f` dead `Component` templates; cheatsheet RSS 3 vs 40 opens flat at ~240 MB, `RI_WSTAT` tables equal).

```
BIN=/tmp/ri-s2 CLEAN=1 TAG=s2 IDLE=120 ~/h/matrix.sh native notify_flood panels hotplug services netchurn \
  reload windows clip volume mpris config lock idle
BIN=/tmp/ri-s2 ~/h/hyprrestart.sh native; ~/h/hyprrestart.sh orig
```

Native, 13/13 OK, no coredump, no IPC timeout, no restart:

| scenario | RSS MB | anon MB | fds | threads | CPU ticks | errors | max IPC ms/timeouts | dur |
|---|---|---|---|---|---:|---:|---|---:|
| notify_flood | 290→373 | 170→251 | 36→37 | 49→49 | 1231 | 0 | 330/0 | 25s |
| panels | 373→525 | 251→402 | 36→36 | 49→51 | 8926 | 1 | 404/0 | 295s |
| hotplug | 525→568 | 402→445 | 36→36 | 51→53 | 5213 | 174 | 893/0 | 114s |
| services | 568→567 | 445→444 | 36→36 | 53→53 | 124 | 0 | 159/0 | 143s |
| netchurn | 567→568 | 444→444 | 36→36 | 53→53 | 55 | 0 | 98/0 | 40s |
| reload | 568→569 | 444→445 | 36→36 | 53→53 | 40 | 0 | 111/0 | 35s |
| windows | 569→583 | 445→460 | 36→39 | 53→54 | 2028 | 0 | 147/0 | 126s |
| clip | 583→609 | 460→486 | 39→39 | 54→54 | 1418 | 0 | 174/0 | 43s |
| volume | 609→611 | 486→488 | 39→39 | 54→54 | 186 | 0 | 123/0 | 50s |
| mpris | 611→615 | 488→491 | 39→39 | 54→54 | 1531 | 0 | 119/0 | 97s |
| config | 615→616 | 491→492 | 39→39 | 54→54 | 103 | 0 | 120/0 | 43s |
| lock | 616→583 | 492→451 | 39→39 | 54→54 | 1588 | 0 | 277/0 | 121s |
| idle | 583→567 | 451→436 | 39→39 | 54→54 | 105 | 0 | 122/0 | 178s |

Native peaks at 616 MB and ends at 567 MB (first run below: ~1.1 GB plateau); same scenarios on the original
reach 1.77 GB after `hotplug` alone (`orig-s1o`: 1040→1769 MB, 769 errors).

### Fixed: every screen change re-logged (and kept) all QML errors since startup

`set_screens` printed the whole `Runtime::errors()` history on each output change, and the main loop cloned
that ever-growing list on every revision change to find new entries. `hotplug` "errors" were almost all
reprints; `reload` (Hyprland `reload` re-sends monitors) logged 1020 lines with no new error. The shell now
takes only the errors raised since its last call (`Runtime::take_errors`; each distinct message once, the
seen-set is bounded) and binding errors when they first appear.

| native, same build otherwise | before (`s1`) | after (`s2`) |
|---|---:|---:|
| `hotplug` error lines | 8133 | 174 |
| `reload` error lines | 1020 | 0 |
| 6 hotplugs after one wallpaper-selector open/close (`ab.sh`) | 13 | 1 |

The remaining hotplug lines are monitor-null transients the original prints too (`WorkspaceModel`,
`workspacesForMonitor`, `NotificationPopup … enable`), plus one native-only
`AddressBreadcrumb.onDirectoryChanged: … 'startsWith' of undefined` per wallpaper-selector open: native runs
the `directory` change handler before `breadcrumbDirectory`'s default is applied (Qt does not run `on…Changed`
for initial values). Harmless (the next line assigns it); creation/binding order belongs to the binding session.

### Lock / unlock

`lock` inside the `s2` matrix counted 0/15 "unlocked frame matches pre-lock": its pre-lock screenshot was
Hyprland's "lockscreen app died" screen, left over from a shell killed while locked in an earlier aborted run;
all 15 native locks were taken and released and every unlocked frame showed the desktop (un1 vs un15 RMSE
0.017). Rerun on a clean compositor (`faillock --reset` first: three wrong-password attempts lock the account
in `pam_faillock` and make both shells fail):

| | unlocked OK | `session lock: locked` / `released` | faillock entries after |
|---|---:|---|---:|
| native (`ri-s2`) | 15/15 | 15 / 15 | 0 |
| orig | 15/15 | (Quickshell PAM log: 15 "Authenticated successfully") | 0 |

### Compositor exit (`hyprrestart.sh`, 3x each)

The guest has no getty autologin any more, so the script now starts Hyprland itself (seatd) when no new
instance appears within 15 s; the earlier `HANG` was that missing compositor (native then failed to connect),
not the shell. Both shells exit within 0.5 s every iteration; fresh start to first IPC reply native
735-803 ms, orig 1314-1337 ms. Coredumps in the window are only `hypridle`/`hyprsunset` (SIGABRT when their
compositor goes away), in both modes.

### Guest checks on this build

Rebased onto `65003b7` (#51 binding speed, #55-#60), rebuilt, rerun in the guest:
shell tests pass, evalcheck 20/20, swipe `inccheck ok` 95 with 0 bad dmg/dl/lay and 0 panics, burst all
zero; `ab.sh` 1 error line after the selector cycle; `matrix.sh native hotplug reload panels lock idle`: all
OK (hotplug 192 lines, reload 0, panels 1, lock 15/15 with 15 locked/released, idle 0, no IPC timeouts).
Locally: qml lib 94/94, notifications 9/9, `qml --no-default-features` builds.

## Per scenario

Original `qs -c ii`:

| scenario | result | RSS MB | anon MB | fds | threads | CPU s | errors | max IPC ms/timeouts | dur |
|---|---|---|---|---|---|---|---|---|---|
| notify_flood | OK | 1070 -> 1091 | 885 -> 902 | 75 -> 72 | 45 -> 41 | 8.9 | 134 | 300/0 | 28s |
| panels | OK | 1091 -> 1335 | 902 -> 1117 | 72 -> 76 | 41 -> 45 | 185.5 | 2046 | 650/0 | 238s |
| hotplug | OK | 1335 -> 1932 | 1117 -> 1737 | 76 -> 76 | 45 -> 45 | 44.2 | 763 | 470/0 | 114s |
| services | OK | 1932 -> 1742 | 1737 -> 1553 | 76 -> 72 | 45 -> 41 | 1.4 | 10 | 49/0 | 143s |
| netchurn | OK | 1742 -> 1782 | 1553 -> 1592 | 72 -> 72 | 41 -> 41 | 0.9 | 0 | 29/0 | 37s |
| reload | OK | 1782 -> 1784 | 1592 -> 1593 | 72 -> 72 | 41 -> 41 | 0.5 | 0 | 80/0 | 35s |
| windows | OK | 1784 -> 1522 | 1593 -> 1330 | 72 -> 77 | 41 -> 45 | 35.3 | 145 | 60/0 | 125s |
| clip | OK | 1522 -> 1447 | 1330 -> 1257 | 77 -> 73 | 45 -> 41 | 11.1 | 4 | 66/0 | 42s |
| volume | OK | 1447 -> 1514 | 1257 -> 1320 | 73 -> 73 | 41 -> 41 | 2.4 | 0 | 56/0 | 50s |
| mpris | OK | 1514 -> 1499 | 1320 -> 1304 | 73 -> 76 | 41 -> 44 | 33.4 | 21 | 86/0 | 96s |
| config | OK | 1499 -> 1536 | 1304 -> 1346 | 76 -> 73 | 44 -> 41 | 0.9 | 0 | 50/0 | 42s |
| lock | OK | 1536 -> 1598 | 1346 -> 1397 | 73 -> 73 | 41 -> 41 | 414.8 | 184 | 2924/0 | 124s |
| idle | OK | 1598 -> 2134 | 1397 -> 1941 | 73 -> 73 | 41 -> 41 | 18.3 | 4 | 145/0 | 850s |

Native `impulse-shell` (startup to first IPC reply 1.55 s):

| scenario | result | RSS MB | anon MB | fds | threads | CPU s | errors | max IPC ms/timeouts | dur |
|---|---|---|---|---|---|---|---|---|---|
| notify_flood | OK | 358 -> 463 | 223 -> 326 | 39 -> 39 | 53 -> 53 | 18.8 | 0 | 65/0 | 22s |
| panels | OK | 463 -> 1024 | 326 -> 886 | 39 -> 39 | 53 -> 53 | 107.3 | 0 | 328/0 | 232s |
| hotplug | OK | 1024 -> 1098 | 886 -> 959 | 39 -> 39 | 53 -> 53 | 55.2 | 4955 | 1267/0 | 112s |
| services | OK | 1098 -> 1099 | 959 -> 961 | 39 -> 39 | 53 -> 53 | 1.7 | 0 | 53/0 | 142s |
| netchurn | OK | 1099 -> 1100 | 961 -> 962 | 39 -> 39 | 53 -> 54 | 1.9 | 0 | 67/0 | 39s |
| reload | OK | 1100 -> 1101 | 962 -> 963 | 39 -> 39 | 54 -> 54 | 0.9 | 620 | 100/0 | 35s |
| windows | OK | 1101 -> 1096 | 963 -> 965 | 39 -> 46 | 54 -> 55 | 30.6 | 0 | 403/0 | 128s |
| clip | OK | 1096 -> 1102 | 965 -> 971 | 46 -> 46 | 55 -> 55 | 24.4 | 3 | 354/0 | 47s |
| volume | OK | 1102 -> 1097 | 971 -> 966 | 46 -> 46 | 55 -> 55 | 3.0 | 0 | 110/0 | 53s |
| mpris | OK | 1097 -> 1100 | 966 -> 969 | 46 -> 46 | 55 -> 55 | 16.4 | 0 | 207/0 | 96s |
| config | OK | 1100 -> 1104 | 969 -> 973 | 46 -> 46 | 55 -> 55 | 1.3 | 0 | 50/0 | 43s |
| lock | OK | 1104 -> 1065 | 973 -> 925 | 46 -> 46 | 55 -> 55 | 14.7 | 0 | 139/0 | 120s |
| idle | OK | 1065 -> 1129 | 925 -> 978 | 46 -> 46 | 55 -> 55 | 5.5 | 0 | 79/0 | 850s |

"errors" counts new `ERROR|WARN|TypeError|ReferenceError|Cannot read|is not defined|qml:` log lines; the
original's are mostly Qt warnings native does not print (missing icons, `cava` not installed,
`WorkspaceModel` null `id` during hotplug, binding loops), so compare kinds, not totals.

Other measurements:

- Idle CPU (bar / sidebarRight open / overview open): orig 0.90 / 0.96 / 8.82 %, native 0.53 / 0.45 / 1.28 %.
- Lock (15 lock/unlock cycles): both 15/15 unlocked; unlock screenshot vs pre-lock RMSE orig 0.016, native
  0.002-0.003. Orig burns 415 CPU-s and stalls IPC up to 2.9 s during the lock blur on llvmpipe; native 15 CPU-s.
- Hotplug: native max IPC latency 1267 ms vs 470 ms orig (screen-bound windows rebuilt on the main thread);
  no timeouts.
- services: pipewire, NetworkManager, upower, bluetooth restarted 3x each; both recover (volume, network,
  battery widgets re-bind). BlueZ is daemon-only: the guest has no adapter (no `hci_vhci`/`btvirt`).

## Native issue 1: `SearchItem` entries freed while delegates still use them

Log (after hotplug/reload re-evaluates the overview), 2686 times each:

```
qml: SearchItem.materialSymbol: Cannot read property 'iconType' of null
qml: SearchItem/Repeater.model: Cannot read property 'actions' of null
```

`LauncherSearch.results` builds its items with `resultComp.createObject(null, {...})` inside the binding;
`SearchWidget` copies them into `ScriptModel.values`, whose delegates hold them as `entry`. Native tracks
parentless objects per binding in `World::binding_made` and, on each re-evaluation, `destroy()`s the generation
from two evaluations ago (`runtime.rs`, binding update: `let old = mem::replace(&mut gens.0, ...); for o in old
{ w.destroy(o) }`) whether or not anything still references it. Qt only collects JS-owned objects once
unreferenced, so original keeps the live entries. Minimal repro (native prints `held=Null` after the second
re-evaluation; Qt keeps `held.v == 0`):

```qml
Item {
  id: root
  property int q: 0
  property var results: { root.q; return [comp.createObject(null, {v: root.q})] }
  property QtObject held: null
  function snap() { held = results[0] }   // then q = 1; q = 2
  Component { id: comp; QtObject { property int v } }
}
```

Fix direction: before destroying an old generation, skip objects still referenced by a property value / model
(or by any live `Value::Item` in a delegate's `entry`), i.e. reachability instead of age.

## Native issue 2: builtin methods are not values

The assistant chat: `onCountChanged: if (atYEnd || root.session.busy) Qt.callLater(positionViewAtEnd)` ->
`qml: StyledListView/ListView.onCountChanged: positionViewAtEnd is not defined`. In native,
`typeof positionViewAtEnd`, `typeof lv.positionViewAtEnd` and `typeof forceLayout` are all `"undefined"`
(Qt: `"function"`); builtin item methods are only dispatched in call position (`x.m(...)`), never resolved
by `resolve_sym`/`read`. Effect: the chat does not auto-scroll to the newest message. Fix direction: return a
bound native function for builtin method names in `resolve_sym`/`read` when no property matches.

## Native issue 3: every panel open leaks runtime bookkeeping (~2-5 MB per open)

`panels` was the one scenario where native kept growing (anon 613 -> 939 MB over rounds 10-50; orig flat
1.14-1.22 GB). `panelleak.sh` (fresh shell, 40 open/close cycles per panel, anon RSS):

| panel | orig anon MB before -> after (delta) | native anon MB before -> after (delta) |
|---|---:|---:|
| sidebarRight | 871 -> 945 (74) | 221 -> 230 (9) |
| sidebarLeft | 874 -> 907 (33) | 240 -> 240 (0) |
| session | 874 -> 915 (41) | 243 -> 242 (-1) |
| cheatsheet | 938 -> 903 (-35) | 299 -> 568 (269) |
| mediaControls | 919 -> 909 (-10) | 570 -> 562 (-8) |
| osk | 913 -> 874 (-39) | 566 -> 588 (22) |
| overlay | 900 -> 880 (-20) | 599 -> 694 (95) |
| wallpaperSelector | 895 -> 885 (-10) | 714 -> 810 (96) |
| search | 913 -> 927 (14) | 834 -> 822 (-12) |
| workspaces | 917 -> 931 (14) | 821 -> 830 (9) |
| clipboard | 936 -> 895 (-41) | 796 -> 658 (-138) |
| g:overviewEmojiToggle | 938 -> 907 (-31) | 674 -> 675 (1) |

`panelgrowth.sh` (native, `RI_WSTAT=1`, 5 rounds x 30 cycles) shows the live tree is constant while the
`World` tables grow linearly with the number of opens:

| cheatsheet, after round | anon MB | malloc in-use MB | `items` slots | live | `ctxs` | `tmpl` | `ctxjs` | `beh` |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| 0 (3 warm-up opens) | 277 | 191 | 12 231 | 11 520 | 6 542 | 275 | 74 | 1 192 |
| 5 (150 opens) | 1202 | 927 | 3 090 096 | 11 423 | 1 402 735 | 13 892 | 24 870 | 5 399 |

overlay: in-use 155 -> 515 MB, wallpaperSelector 159 -> 471 MB over the same 150 opens.

Root cause: instance ids and context indices are never reused (`Arena` / `IdSet` / `Ctx` doc comments
in `runtime.rs`). Destroying a panel's tree releases each `Inst` box but keeps, per created instance:
its 8-byte arena slot and dead bit; its `Ctx` (`new_ctx` only `push`es; `sweep_dead` clears `ids` but never
shrinks `ctxs`, and also walks the whole ever-growing `ctxs` vector on every sweep); its `ctx_js` entry
(`sweep_dead` removes it only when the context had ids: `if gone.contains(&c.root) && !c.ids.is_empty()`);
and the `Template`s pushed for its inline components/Loaders (`w.templates.push`, never removed; a dead
`Component` only empties `members`). Each cheatsheet open creates ~20 000 instances / ~9 000 contexts /
~90 templates, so ~5 MB stays behind per open (malloc in-use, not fragmentation: `malloc_trim` runs every
GC). Original Qt frees the whole tree.
Fix direction: free-list reuse of item ids and ctx indices (or compacting `ctxs`/`templates`/`ctx_js` on
`sweep_dead`), plus dropping `ctx_js` and `templates` entries of dead contexts/instances.

## Compositor exit (`hyprrestart.sh`, 3x each)

Hyprland exited under the running shell, getty autologin restarts it: both shells exit within 0.5 s
every time (no hang on the dead Wayland socket, no coredump); fresh start to first IPC reply
orig 1.39-1.43 s, native 0.72-1.09 s.

## Not covered

- BlueZ device/adapter paths (no Bluetooth adapter in the guest; only `bluetooth.service` restarts).
- Real GPU/DRM paths: llvmpipe only; frame-time numbers are not representative of hardware.
