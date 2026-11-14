//! Scene: "W H bg" then "x y w h radius transparentBorder(0|1) png" (Image → GaussianBlur, samples = 2r+1).
use qsg::image::{self, FillMode};
use qsg::{blur, layer, raster::Canvas};

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
        let (x, y, w, h, r) = (n(0), n(1), n(2), n(3), n(4) as i32);
        let tex = image::load_png(f[6]).unwrap();
        let mut item = layer::texture(w, h);
        image::draw(
            &mut item,
            &tex,
            0.0,
            0.0,
            w,
            h,
            FillMode::Stretch,
            true,
            1.0,
        );
        if f.len() > 7 {
            blur::drop_shadow(
                &mut cv,
                &item,
                (item.w as f64, item.h as f64),
                r as f64,
                2 * r + 1,
                hex(f[7]).map(|k| k as u16 * 257),
                0.0,
                (0.0, 0.0),
                f[5] == "1",
                x,
                y,
                1.0,
                [1.0, 1.0],
            );
        } else if f[5] == "1" {
            blur::gaussian_blur(&mut cv, &item, r as f64, 2 * r + 1, None, true, x, y, 1.0);
        } else {
            blur::gaussian_blur_direct(
                &mut cv,
                &tex,
                w as usize,
                h as usize,
                r as f64,
                2 * r + 1,
                x,
                y,
                1.0,
            );
        }
    }
    cv.save_png(&args[2]).unwrap();
}
