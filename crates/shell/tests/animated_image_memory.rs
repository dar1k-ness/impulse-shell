//! AnimatedImage keeps one frame however many the file has. `work.gif` is 940 bytes of 40
//! frames on a 4000x4000 canvas, 61 MB a frame: Qt 6.12 with `cache: true` (its default) ends
//! the first round 2.4 GB larger, with `cache: false` it stays level. The shell stays level
//! with both.
use qml::value::Value;
use qsg::raster::Canvas;
use shell::scene::{self, Target};

const FRAMES: usize = 40;
const SIDE: u16 = 4000;
const W: usize = 16;
const H: usize = 12;

/// A GIF whose `frames` 1x1 images each composite onto a `w`x`h` canvas, 50 ms apart.
fn gif_bomb(w: u16, h: u16, frames: usize) -> Vec<u8> {
    let mut g = b"GIF89a".to_vec();
    g.extend(w.to_le_bytes());
    g.extend(h.to_le_bytes());
    g.extend([0x80, 0, 0, 0, 0, 0, 255, 255, 255]);
    for _ in 0..frames {
        g.extend([0x21, 0xF9, 4, 0, 5, 0, 0, 0]);
        g.extend([0x2C, 0, 0, 0, 0, 1, 0, 1, 0, 0, 2, 2, 0x44, 1, 0]);
    }
    g.push(0x3B);
    g
}

/// The bytes of the process's own pages in memory (not those of mapped files).
fn anon_rss() -> u64 {
    let s = std::fs::read_to_string("/proc/self/statm").unwrap();
    let f: Vec<u64> = s.split_whitespace().map(|v| v.parse().unwrap()).collect();
    (f[1] - f[2]) * 4096
}

/// Plays `work.gif` for two rounds by the runtime's own clock and returns the memory of the
/// process after each frame was shown.
fn play(file: &str, props: &str) -> Vec<u64> {
    let qml = format!(
        "import QtQuick\nimport Quickshell\nFloatingWindow {{\n    width: {W}; height: {H}\n    \
         property Item img: a\n    AnimatedImage {{ id: a; source: \"{file}\"; {props} }}\n}}\n"
    );
    let mut rt = qml::runtime::Runtime::new(None);
    let win = rt.load_str(&qml).expect("scene loads");
    let Value::Item(a) = rt.get(win, "img") else {
        panic!("img");
    };
    let mut fonts = scene::Fonts::default();
    let mut seen = Vec::new();
    for k in 0..=2 * FRAMES {
        // The file's frames are 50 ms apart: each turn of the clock is due one frame. The
        // first frame shows before the clock runs, and its delay counts from the clock's
        // first turn.
        if k > 0 {
            rt.tick(k as f64 * 50.0);
        }
        for _ in 0..3 {
            rt.settle();
            scene::layout_window(&mut rt, &mut fonts, win, [W as f64, H as f64]);
        }
        let mut c = Canvas::new(W, H, [0, 0, 0, 0]);
        scene::paint_window(&mut rt, &mut fonts, win, &mut Target::Cpu(&mut c));
        assert_eq!(c.px[0], [0, 0, 0, 255], "{props}: frame {k} shows");
        assert_eq!(
            rt.get(a, "currentFrame").to_number(),
            (k % FRAMES) as f64,
            "{props}: the frame after {k} delays"
        );
        assert_eq!(rt.get(a, "frameCount").to_number(), FRAMES as f64);
        seen.push(anon_rss());
        if k == 0 {
            rt.tick(0.0);
        }
    }
    seen
}

#[test]
fn memory_is_level_over_two_rounds() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("work.gif");
    std::fs::write(&file, gif_bomb(SIDE, SIDE, FRAMES)).unwrap();
    // A frame as it is shown: 4000 x 4000 pixels of four bytes.
    let frame = u64::from(SIDE) * u64::from(SIDE) * 4;
    // At a `sourceSize` a kept frame is the scaled one, a few kilobytes: Qt is level there too.
    for props in ["", "cache: false"] {
        let seen = play(file.to_str().unwrap(), props);
        let (low, high) = (seen.iter().min().unwrap(), seen.iter().max().unwrap());
        // Keeping the frames shown would add 39 frames in the first round.
        assert!(
            high - low < frame / 2,
            "{props:?}: memory after each frame, MB: {:?}",
            seen.iter().map(|b| b >> 20).collect::<Vec<_>>()
        );
    }
}
