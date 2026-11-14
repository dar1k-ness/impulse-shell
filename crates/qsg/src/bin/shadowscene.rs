//! Scene: "W H bg" then "x y w h radius blur spread offx offy #aarrggbb cached(0|1)" per line.
use qsg::{raster::Canvas, shadow};

fn hex(s: &str) -> [u8; 4] {
    let v = u32::from_str_radix(s.trim_start_matches('#'), 16).unwrap();
    if s.len() == 9 {
        [(v >> 16) as u8, (v >> 8) as u8, v as u8, (v >> 24) as u8]
    } else {
        [(v >> 16) as u8, (v >> 8) as u8, v as u8, 255]
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let src = std::fs::read_to_string(&args[1]).unwrap();
    let mut lines = src.lines().filter(|l| !l.trim().is_empty());
    let hdr: Vec<&str> = lines.next().unwrap().split_whitespace().collect();
    let mut cv = Canvas::new(
        hdr[0].parse().unwrap(),
        hdr[1].parse().unwrap(),
        hex(hdr[2]),
    );
    for l in lines {
        let f: Vec<&str> = l.split_whitespace().collect();
        let n = |i: usize| f[i].parse::<f64>().unwrap();
        let p = shadow::ShadowParams {
            width: n(2),
            height: n(3),
            radius: n(4),
            corners: None,
            blur: n(5),
            spread: n(6),
            offset: (n(7), n(8)),
            color: hex(f[9]).map(|k| k as u16 * 257),
            antialiasing: true,
            cached: f[10] == "1",
        };
        shadow::draw(&mut cv, &p, n(0), n(1), 1.0);
    }
    cv.save_png(&args[2]).unwrap();
}
