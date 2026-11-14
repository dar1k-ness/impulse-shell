//! Scene: "W H bg" then "x y w h overlay|desat param png" (Image → Qt5Compat effect, see effect_oracle.py).
use qsg::image::{self, FillMode};
use qsg::{layer, raster::Canvas};

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
    let mut lines = src
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'));
    let hdr: Vec<&str> = lines.next().unwrap().split_whitespace().collect();
    let mut cv = Canvas::new(
        hdr[0].parse().unwrap(),
        hdr[1].parse().unwrap(),
        hex(hdr[2]),
    );
    for l in lines {
        let f: Vec<&str> = l.split_whitespace().collect();
        let n = |i: usize| f[i].parse::<f64>().unwrap();
        let (x, y, w, h) = (n(0), n(1), n(2), n(3));
        let tex = image::load_png(f[6]).unwrap();
        let mut layer_tex = layer::texture(w, h);
        image::draw(
            &mut layer_tex,
            &tex,
            0.0,
            0.0,
            w,
            h,
            FillMode::Stretch,
            true,
            1.0,
        );
        match f[4] {
            "overlay" => {
                let c = hex(f[5]).map(|k| k as u16 * 257);
                layer::draw_color_overlay(&mut cv, &layer_tex, c, true, x, y, w, h, 1.0)
            }
            _ => layer::draw_desaturate(&mut cv, &layer_tex, n(5) as f32, true, x, y, w, h, 1.0),
        }
    }
    cv.save_png(&args[2]).unwrap();
}
