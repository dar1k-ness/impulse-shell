//! RadialGradient scene: `W H bg` then `x y w h hOff vOff hR vR angle pos:#color...`.
use qsg::{gradient, raster::Canvas};

fn hex(s: &str) -> [u8; 4] {
    let s = s.trim_start_matches('#');
    let b = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).unwrap();
    if s.len() == 8 {
        [b(2), b(4), b(6), b(0)]
    } else {
        [b(0), b(2), b(4), 255]
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let txt = std::fs::read_to_string(&args[1]).unwrap();
    let mut lines = txt.lines().filter(|l| !l.trim().is_empty());
    let hdr: Vec<&str> = lines.next().unwrap().split_whitespace().collect();
    let mut cv = Canvas::new(
        hdr[0].parse().unwrap(),
        hdr[1].parse().unwrap(),
        hex(hdr[2]),
    );
    for l in lines {
        let f: Vec<&str> = l.split_whitespace().collect();
        let n = |i: usize| f[i].parse::<f64>().unwrap();
        let stops: Vec<(f32, [u8; 4])> = f[9..]
            .iter()
            .map(|s| {
                let (t, c) = s.split_once(':').unwrap();
                (t.parse().unwrap(), hex(c))
            })
            .collect();
        let r = gradient::Radial::new(n(2), n(3), n(4), n(5), n(6), n(7), n(8));
        let tex = gradient::gradient_texture(&stops, true, 1.0);
        gradient::radial(&mut cv, &tex, &r, None, n(0), n(1), n(2), n(3), 1.0);
    }
    cv.save_png(&args[2]).unwrap();
}
