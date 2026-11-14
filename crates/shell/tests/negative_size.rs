//! Anchors that cross give an item a negative size (`scenes/qt_geometry/anchor_cross.qml`).
//! What Qt 6.12 then paints, grabbed from its software backend in the testenv image: nothing
//! for a Rectangle of negative size, nothing inside a clipping item of negative size, the
//! children of a non-clipping one as usual, a Text that does not elide as if it had no width
//! set, and nothing for a Text that elides (`scenes/qt_geometry/elide_no_room.qml`), unless
//! its implicit size is used and it never had room: that one is drawn in full.
//! The scene is `scenes/negative_size/scene.qml`, compiled into the test (`testscenes`): a
//! build without the interpreter paints it from compiled code.

use qsg::raster::Canvas;
use shell::scene::{self, Target};

const W: usize = 160;
const H: usize = 64;

fn paint(gpu: bool) -> Canvas {
    let (mut rt, win) = testscenes::load("negative_size/scene.qml");
    let mut fonts = scene::Fonts::default();
    for _ in 0..3 {
        rt.settle();
        scene::layout_window(&mut rt, &mut fonts, win, [W as f64, H as f64]);
    }
    if !gpu {
        let mut c = Canvas::new(W, H, [0, 0, 0, 0]);
        scene::paint_window(&mut rt, &mut fonts, win, &mut Target::Cpu(&mut c));
        return c;
    }
    let (mut g, _) =
        shell::gpu::Gpu::headless("surfaceless").expect("surfaceless EGL/GLES2 context (Mesa)");
    let mut fr = shell::gpu::Frame::new(W, H);
    scene::paint_window(&mut rt, &mut fonts, win, &mut Target::Gpu(&mut fr));
    let mut s = g.pbuffer(W as i32, H as i32).expect("pbuffer");
    s.reswap();
    g.render(&mut s, &mut fr, || {});
    g.read_surface(&s)
}

/// The bounding box of the pixels `pick` accepts: `[x0, y0, x1, y1]`, inclusive.
fn bbox(c: &Canvas, pick: impl Fn([u8; 4]) -> bool) -> Option<[usize; 4]> {
    let mut b: Option<[usize; 4]> = None;
    for y in 0..H {
        for x in 0..W {
            if pick(c.px[y * W + x]) {
                let b = b.get_or_insert([x, y, x, y]);
                *b = [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)];
            }
        }
    }
    b
}

fn check(c: &Canvas) {
    let hi = |v: u8| v > 0xc0;
    let lo = |v: u8| v < 0x40;
    assert_eq!(
        bbox(c, |p| hi(p[0]) && lo(p[1]) && lo(p[2])),
        None,
        "a Rectangle of negative size is not drawn"
    );
    assert_eq!(
        bbox(c, |p| lo(p[0]) && lo(p[1]) && hi(p[2])),
        None,
        "nor is its border"
    );
    assert_eq!(
        bbox(c, |p| hi(p[0]) && lo(p[1]) && hi(p[2])),
        None,
        "a clipping item of negative size shows nothing"
    );
    assert_eq!(
        bbox(c, |p| lo(p[0]) && hi(p[1]) && hi(p[2])),
        Some([60, 32, 65, 37]),
        "a child of an item of negative size is drawn where Qt draws it"
    );
    let yellow = bbox(c, |p| hi(p[0]) && hi(p[1]) && lo(p[2])).expect("Text is drawn");
    assert!(
        yellow[0] >= 80 && yellow[2] > 100 && yellow[1] >= 40,
        "Text of negative width starts at its x and is not cut: {yellow:?}"
    );
    let green = bbox(c, |p| lo(p[0]) && hi(p[1]) && lo(p[2])).expect("wrapping Text is drawn");
    assert!(
        green[0] >= 80 && green[2] > 100 && green[3] < 18,
        "wrapping Text of negative width stays on one line: {green:?}"
    );
    assert_eq!(
        bbox(c, |p| hi(p[0]) && hi(p[1]) && hi(p[2])),
        None,
        "a Text that elides draws nothing when its width is negative or zero"
    );
    let orange = bbox(c, |p| hi(p[0]) && (0x60..0xa0).contains(&p[1]) && lo(p[2]))
        .expect("an eliding Text whose implicit size is used is drawn");
    assert!(
        orange[0] >= 5 && orange[2] > 20 && orange[2] < 40 && orange[1] >= 40,
        "it starts at its x and is not cut: {orange:?}"
    );
}

#[test]
fn cpu() {
    check(&paint(false));
}

#[test]
fn gpu() {
    check(&paint(true));
}
