//! Scene: "W H bg" then "x y w h maskradius fill1 fill2" (see oracle mask_oracle.py).
use qsg::{layer, raster::Canvas, rect};

fn hex(s: &str) -> [u8; 4] {
    let s = s.trim_start_matches('#');
    let p = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).unwrap();
    if s.len() == 8 {
        [p(2), p(4), p(6), p(0)]
    } else {
        [p(0), p(2), p(4), 255]
    }
}

fn rect_on(cv: &mut Canvas, x: f32, y: f32, w: f32, h: f32, r: f32, c: [u8; 4]) {
    let p = rect::RectParams {
        width: w,
        height: h,
        radius: r,
        color: c,
        antialiasing: r != 0.0,
        aligned: true,
        ..Default::default()
    };
    cv.draw_smooth(&rect::build(&p), x, y, 1.0);
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
        let n = |i: usize| f[i].parse::<f32>().unwrap();
        let (w, h) = (n(2), n(3));
        let mut tex = layer::texture(w as f64, h as f64);
        rect_on(&mut tex, 0.0, 0.0, w, h, 0.0, hex(f[5]));
        rect_on(
            &mut tex,
            -10.0,
            -10.0,
            w * 0.6 + 10.0,
            h * 0.5 + 10.0,
            0.0,
            hex(f[6]),
        );
        let mut mask = layer::texture(w as f64, h as f64);
        rect_on(&mut mask, 0.0, 0.0, w, h, n(4), [255, 255, 255, 255]);
        layer::draw_opacity_mask(
            &mut cv,
            &tex,
            &mask,
            false,
            n(0) as f64,
            n(1) as f64,
            w as f64,
            h as f64,
            1.0,
        );
    }
    cv.save_png(&args[2]).unwrap();
}
