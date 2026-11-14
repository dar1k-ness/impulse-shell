//! Text scene: `W H bg` then lines `font_path px wght x y w h color|text` (VCenter in w×h box).
use qsg::{raster::Canvas, text::FontStack};

fn hex(s: &str) -> [u8; 4] {
    let s = s.trim_start_matches('#');
    let p = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).unwrap();
    if s.len() == 8 {
        [p(2), p(4), p(6), p(0)]
    } else {
        [p(0), p(2), p(4), 255]
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
        let (spec, text) = l.split_once('|').unwrap();
        let f: Vec<&str> = spec.split_whitespace().collect();
        let n = |i: usize| f[i].parse::<f64>().unwrap();
        let mut axes = vec![];
        if n(2) > 0.0 {
            axes.push(("wght", n(2) as f32));
        }
        let font = FontStack::from_path(f[0], n(1) as u32, 400.0, &axes).unwrap();
        let line = font.shape(text);
        let dy = (n(6) - line.height()) / 2.0;
        if std::env::var_os("TS_DUMP").is_some() {
            elog::eprintln!(
                "RS h {} asc {} adv {} glyphs {:?}",
                line.height(),
                line.ascent,
                line.advance,
                line.glyphs
            );
        }
        font.draw_line(&mut cv, &line, [n(3), n(4)], [0.0, dy], hex(f[7]), 1.0);
    }
    cv.save_png(&args[2]).unwrap();
}
