//! A Canvas repainted every frame inside a `layer.effect: OpacityMask` (the wave of the media
//! panel's progress slider, which moves while the player is paused too) does not have the
//! layer drawn again for each of its frames: where the mask is 1 and nothing of the layer is
//! over it, the Canvas is drawn over the masked layer instead of into it, and its next picture
//! is swapped into the window's last frame without painting the window. Every frame is still,
//! byte for byte, the one a window drawn once from nothing shows for the same Canvas: in the
//! mask's corner and under a sibling too, where the Canvas stays in the layer.

use qml::runtime::Runtime;
use qml::value::Value;
use qsg::raster::Canvas;
use shell::scene::{self, Target};

const W: usize = 160;
const H: usize = 96;

/// The wave as WavyLine.qml paints it, with the phase a property instead of the clock.
const WAVE: &str = r##"Canvas {
            x: 30; y: 30; width: 80; height: 20
            property real phase: root.phase
            onPhaseChanged: requestPaint()
            onPaint: {
                var ctx = getContext("2d");
                ctx.clearRect(0, 0, width, height);
                ctx.strokeStyle = "#f0e0a0";
                ctx.lineWidth = 4;
                ctx.lineCap = "round";
                ctx.beginPath();
                for (var x = 2; x <= width - 2; x += 1) {
                    var y = height / 2 + 2 * Math.sin(6 * 2 * Math.PI * x / 160 + phase);
                    if (x === 0) ctx.moveTo(x, y); else ctx.lineTo(x, y);
                }
                ctx.stroke();
            }
        }"##;

/// A translucent bar as wide as `phase` says: at (x, y), 40 x 12.
fn bar(x: usize, y: usize) -> String {
    format!(
        r##"Canvas {{
            x: {x}; y: {y}; width: 40; height: 12
            property real phase: root.phase
            onPhaseChanged: requestPaint()
            onPaint: {{
                var ctx = getContext("2d");
                ctx.clearRect(0, 0, width, height);
                ctx.fillStyle = "#80ffffff";
                ctx.fillRect(0, 0, 4 + 4 * phase, height);
            }}
        }}"##
    )
}

/// A rounded panel like PlayerControl.qml's background, with the wave in its middle. With
/// `more`: a bar in the corner the mask rounds off, and one under a translucent sibling.
fn source(more: bool) -> String {
    let more = if more {
        format!(
            "{}\n        {}\n        Rectangle {{ x: 110; y: 62; width: 30; height: 12; color: \"#6000ff00\" }}",
            bar(0, 0),
            bar(96, 58)
        )
    } else {
        String::new()
    };
    format!(
        r##"import QtQuick
import Qt5Compat.GraphicalEffects
Item {{
    id: root
    width: {W}; height: {H}
    property real phase: 0
    Rectangle {{ anchors.fill: parent; color: "#203040" }}
    Rectangle {{
        id: background
        x: 8; y: 8; width: 144; height: 80
        color: "#804020"
        radius: 20
        layer.enabled: true
        layer.effect: OpacityMask {{
            maskSource: Rectangle {{
                width: background.width
                height: background.height
                radius: background.radius
            }}
        }}
        Rectangle {{ width: 72; height: 80; color: "#206080" }}
        {WAVE}
        {more}
    }}
}}
"##
    )
}

/// What one run of `phases` showed and did.
struct Run {
    frames: Vec<Canvas>,
    /// Per frame: whether the window was painted for it.
    painted: Vec<bool>,
    /// Per frame: how many times a layer's texture was drawn into.
    layer_draws: Vec<u64>,
    /// Canvases drawn over their layer in all.
    lifts: u64,
}

/// The window after each of `phases` is written, shown the way the shell shows it
/// (`App::mark_dirty`): a Canvas that painted something new and changed in nothing else is
/// swapped in the window's last frame where that draws it over its layer; anything else drops
/// the recorded draws of what changed and paints the window. On a thread of its own: the draw
/// caches are per thread, so the window starts from nothing.
fn run(more: bool, phases: Vec<f64>) -> Run {
    std::thread::spawn(move || {
        let mut rt = Runtime::new(None);
        let root = rt.load_str(&source(more)).expect("scene loads");
        let mut fonts = scene::Fonts::default();
        let (mut gpu, _) =
            shell::gpu::Gpu::headless("surfaceless").expect("surfaceless EGL/GLES2 context (Mesa)");
        let mut surface = gpu.pbuffer(W as i32, H as i32).expect("pbuffer");
        let mut out = Run {
            frames: Vec::new(),
            painted: Vec::new(),
            layer_draws: Vec::new(),
            lifts: 0,
        };
        let mut last: Option<shell::gpu::Frame> = None;
        let mut now = 0.0;
        for phase in phases {
            rt.world
                .borrow_mut()
                .write(root, "phase", Value::Num(phase));
            now += 16.0;
            rt.tick(now);
            rt.settle();
            let (touched, moved, repainted) = {
                let mut w = rt.world.borrow_mut();
                (
                    std::mem::take(&mut w.touched),
                    std::mem::take(&mut w.moved),
                    std::mem::take(&mut w.repainted),
                )
            };
            {
                let w = rt.world.borrow();
                scene::invalidate_moved(&w, &moved);
                scene::invalidate_layers(&w, &touched);
            }
            let only = scene::only_repainted(&touched, &moved, &repainted);
            // `root` is touched by the write of `phase`, which changes nothing it draws.
            let rest = touched.iter().any(|t| *t != root && !only.contains(t));
            let swapped = !rest
                && moved.is_empty()
                && last.as_mut().is_some_and(|fr| {
                    let mut w = rt.world.borrow_mut();
                    // Each one is tried, as the shell tries each.
                    let n = only
                        .iter()
                        .filter(|&&c| scene::swap_repainted(&mut w, c, fr))
                        .count();
                    n == only.len()
                });
            let mut fr = match last.take() {
                Some(fr) if swapped => fr,
                _ => {
                    scene::layout_window(&mut rt, &mut fonts, root, [W as f64, H as f64]);
                    rt.canvases_available();
                    rt.settle();
                    // What changed up to here is painted now (`App::forget_touched`).
                    let mut w = rt.world.borrow_mut();
                    let touched = std::mem::take(&mut w.touched);
                    let moved = std::mem::take(&mut w.moved);
                    w.repainted.clear();
                    scene::invalidate_moved(&w, &moved);
                    scene::invalidate_layers(&w, &touched);
                    drop(w);
                    let mut fr = shell::gpu::Frame::new(W, H);
                    scene::paint_window(&mut rt, &mut fonts, root, &mut Target::Gpu(&mut fr));
                    fr
                }
            };
            surface.reswap();
            shell::gpu::take_layer_draws();
            gpu.render(&mut surface, &mut fr, || {});
            out.layer_draws.push(shell::gpu::take_layer_draws());
            out.painted.push(!swapped);
            out.frames.push(gpu.read_surface(&surface));
            last = Some(fr);
        }
        out.lifts = scene::canvas_lifts();
        out
    })
    .join()
    .expect("the window is drawn")
}

fn phases() -> Vec<f64> {
    (1..=8).map(|n| n as f64 * 0.4).collect()
}

#[test]
fn a_canvas_repainted_every_frame_leaves_its_masked_layer_alone() {
    let r = run(false, phases());

    // The wave moves: no two frames in a row are the same.
    for (n, pair) in r.frames.windows(2).enumerate() {
        assert!(
            pair[0].px != pair[1].px,
            "frames {n} and {} are the same",
            n + 1
        );
    }
    // The first frames draw the layer (it is new, then the Canvas leaves it); after them
    // only the Canvas is drawn, and the window is not painted for it.
    assert_eq!(
        r.layer_draws[4..],
        [0; 4],
        "times a layer was drawn into, per frame: {:?} (Canvases drawn over their layer: {})",
        r.layer_draws,
        r.lifts
    );
    assert_eq!(
        r.painted[4..],
        [false; 4],
        "frames the window was painted for: {:?}",
        r.painted
    );
}

#[test]
fn every_frame_is_the_one_a_fresh_window_shows() {
    for more in [false, true] {
        let live = run(more, phases());

        assert!(
            live.lifts > 0,
            "more {more}: no Canvas was drawn over its layer"
        );
        for (n, phase) in phases().into_iter().enumerate() {
            let fresh = run(more, vec![phase]);
            assert_eq!(fresh.lifts, 0);
            let (a, b) = (&live.frames[n], &fresh.frames[0]);
            let differ: Vec<usize> = (0..W * H).filter(|&i| a.px[i] != b.px[i]).collect();
            if let Some(&i) = differ.first() {
                panic!(
                    "more {more}, frame {n}: {} pixels differ, the first at ({}, {}): {:?}, drawn once {:?}",
                    differ.len(),
                    i % W,
                    i / W,
                    a.px[i],
                    b.px[i]
                );
            }
        }
    }
}
