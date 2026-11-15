# Notification flood: original Qt `qs -c ii` vs native, by phase

Guest: Arch, 6 vCPU / 16 GB, Hyprland 0.57, Qt 6.12.0, quickshell 0.3.2, Mesa llvmpipe (GLES2).
Native: `impulse-shell` release from `2012943` (#65), frame pointers + line tables.
Workload: `bench/qtflood/vfperf.sh` (= `vflood.sh` plus profiler hooks), N=50 history, K=300 `notify-send` (3 apps), 40 s settle wait before the flood.

## CPU per flood (no profiler attached, 3 runs each)

| | run 1 | run 2 | run 3 | mean | per notification |
|---|---|---|---|---|---|
| Qt `quickshell -c ii` | 236 | 316 | 204 | **252 ticks** | 8.4 ms |
| native `impulse-shell` | 532 | 607 | 604 | **581 ticks** | 19.4 ms |

Native is 2.3x Qt here (RX 580: ~3.3x). Qt also answers `notify-send` slower (300 sends take 2.1-3.1 s vs 0.4-0.7 s native).

## CPU by phase (ms per notification)

`perf record -e cpu-clock -F 2000 --call-graph fp` on the whole process during the flood. Each sample goes to the first phase in the table order whose functions appear anywhere on its stack (so a binding evaluated while creating a delegate counts as create, a binding re-run because a handler wrote a property counts as binding). Shares are scaled to the unprofiled means above. Rules: `bench/qtflood/phase.py`.

| phase | Qt frames | native frames | Qt | native | native - Qt |
|---|---|---|---|---|---|
| delegate create | `QQmlObjectCreator::*`, `incubate`, `QQmlDelegateModel`, `Repeater`, `~QObject` | `create_object`, `instantiate`, `update_delegators_inner`, `World::destroy` | 0.27 | 1.01 | +0.74 |
| finalize / componentComplete | `QQmlObjectCreator::finalize`, `componentComplete` | `finalize_settle` | 0.46 | 3.94 | **+3.48** |
| binding evaluation | `QQmlBinding::*`, `QPropertyBinding` | `eval_binding_inner`, `flush_sync` | 1.09 | 1.47 | +0.38 |
| JS handlers / functions | `QQmlBoundSignalExpression`, `QV4::*`, JIT code | `run_handler*`, `call_compiled`, `Interp::call`, `ricompiled` | 1.49 | 1.26 | -0.23 |
| JSON.stringify | `JsonObject`, `Stringify` | `json_write`, `json_str` | ~0 (not on any stack) | 0.19 | +0.19 |
| JS GC | `QV4::MemoryManager`, `GCStateMachine`, mark | (none, Rc) | 0.76 | 0 | -0.76 |
| layout / text (main thread) | `polishItems`, `updatePolish`, `QQuick*Layout`, `QQuickText*`, fonts | `relayout_*`, `layout_window`, `qml::layout`, text | 0.06 (polish incl. bindings: 0.56) | 4.00 | **+3.94** |
| render, main thread | `syncSceneGraph`, `QSG*`, `QRhi`, GL/wayland | `App::draw`, `scene`, `gpu`, GL/wayland | 0.10 | 2.77 | **+2.67** |
| render thread | `QSGRenderThread` | (none, main thread renders) | 0.92 | 0 | -0.92 |
| llvmpipe workers | `llvmpipe-*` | `llvmpipe-*` | 1.74 | 2.58 | +0.84 |
| D-Bus | `QDBus`, `libdbus` | `zbus`, notifications | 0.03 | 0.25 | +0.22 |
| other, main thread | event loop, kernel, malloc | event loop, kernel, malloc | 0.23 | 1.26 | +1.03 |
| other threads | QQmlThread, pixmap reader, pools | zbus, async-io | 1.02 | 0.39 | -0.63 |
| child procs (`cliphist` poll) | | | 0.25 | 0.29 | +0.04 |
| **total** | | | **8.4** | **19.4** | **+11.0** |

Main thread share of all samples: Qt 53%, native 83%.

## Per-notification counts

Qt: `qmlprofiler` attached to `quickshell --debug 3768`, recording only the 300-notification flood (counts only; its timings are inflated by the instrumentation). Native: `RI_PROF=0` log of every binding/handler run, flood run minus a K=0 run. Parser: `bench/qtflood/qtd.py`.

| count / notification | Qt | native |
|---|---|---|
| QML objects created | 38.0 (`Creating` ranges) | 94 new instance ids seen in bindings/handlers (id range grows 152) |
| bindings evaluated | 182 | 459 |
| signal handlers run | 5.8 (QML `on...` handlers) | 656 (includes implicit `onXChanged`: Parallel/SequentialAnimation/Behavior `onChildrenChanged` 84, NumberAnimation `on*Changed` 30) |
| `NotificationItem.opacity` evaluated | 0.15 | 3.5 (plus `onOpacityChanged` 9.9) |
| `onlyNotification` evaluated (NotificationGroup.qml:248) | 5.5 | 3.8 |
| `Behavior.animation` bindings | 3.2 Behavior objects created | 8.3 |
| `groupsForList` / `stringifyList` calls | 4.0 / 1.0 | |
| JS function/expression executions | 3718 (Notifications.qml:80 751, :120 650, NotificationGroup.qml:161 450) | |

## Where native loses most

1. **Layout, +3.9 ms/notification.** Native spends 21% of all CPU in `relayout_targets` / `layout_window`; Qt's whole polish pass, including the bindings it triggers, is 0.56 ms.
2. **Finalize, +3.5 ms.** `finalize_settle` is 20% of native CPU vs 5.5% for Qt's `QQmlObjectCreator::finalize`. Native also creates 2.5-4x as many objects and evaluates 2.5x as many bindings per notification (459 vs 182), and dispatches ~100x as many handlers, mostly implicit `on*Changed` on animation/Behavior objects that Qt never runs as QML handlers.
3. **Render on the main thread, +2.7 ms (+2.6 ms with llvmpipe).** Qt syncs on the main thread for 0.1 ms and does the rest on `QSGRenderThread`; native `App::draw` is 14% of CPU on the main thread.
4. Create (+0.7) and "other main thread" (+1.0, malloc/free and kernel).

Bindings, JS handlers and JSON together cost about the same on both sides: native 2.9 vs Qt 3.3 ms counting Qt's GC. So the extra cost is in object setup, layout and drawing, not in running the JS.

Caveats: llvmpipe makes render far more expensive than on the RX 580. With perf attached, native ran 1041 ticks vs 581 without, and Qt 262 vs 252, so native phase shares may be skewed somewhat. Many `libQt6Qml` leaf symbols are unresolved, but their callers resolve, so phase attribution from callers is unaffected.

## Render phase: frame count, not per-frame cost

Same guest and workload, `impulse-shell` release from `7028f2b`, Qt via `QT_LOGGING_RULES=qt.scenegraph.time.renderloop=true`, native via `RI_FRAMES` / `RI_STATS` / `RI_CMDSTAT` / `RI_GPUPROF`.

| per flood | Qt | native |
|---|---|---|
| frames rendered | 142-179 (all windows) | 557 (popup 410x1040: 345, bar: 182 + 205 skipped with no damage) |
| main-thread render per frame | sync < 0.5 ms | ~1.5 ms (paint ~0.4, `Gpu::render` + llvmpipe draw/swap ~1.1) |
| render-thread work per frame | render ~0.3-0.8 ms, swap blocks 10-20 ms | none: no render thread |

- Native draws about 3.3x as many frames. They are real animation frames: popup `NotificationItem` opacity/x/scale animations (`RI_PROF=0`: 3634 / 1695 / 1340 `on*Changed`) plus per-notification text changes. Qt's GUI thread is busy longer (300 sends take 2.5-3.8 s vs 0.5-0.9 s), and its render thread blocks in swap, so Qt samples the same time-based animations less often.
- Per frame, native main-thread render costs about the same as Qt's render thread. Qt's 0.1 ms "main-thread render" leaves out its render thread.
- Leftover waste per frame is small. GL uploads are tiny (~170 `glTexSubImage2D` for the whole flood), there are 77 layer misses (mostly 20x20), and bar redraws with no damage are already skipped. `paint_window` is spread over `text_style_scaled`, `num`, `Frame::smooth`, `gpu_layer` and `subtree_extent` (each <= 0.7% of samples). One-shot llvmpipe shader compile is ~1.5%.
- So pixel-identical per-frame cuts in the render path cannot close the +2.7 ms; each is below the +-10% tick noise of `riflood`. Closing it needs fewer frames, which changes animation frames, or a render thread, which moves the work off the main thread without reducing CPU ticks.

Measured cut: cache GL state in `quads` / `draw_tris`, skipping `use_program`, uniform and vertex-attribute calls that repeat the current state. That is pixel-identical by construction. Six alternating rounds of `vfperf.sh NATIVE 50 300` on `d204921`, A = base, B = cache:

| | ticks per run | mean |
|---|---|---|
| A | 411 402 405 430 421 479 | 425 |
| B | 364 446 424 431 411 528 | 434 |

No win: Mesa already ignores GL calls that repeat its current state. The cut was not merged.
