//! Scene: "W H bg" then "x y w h blurMax blur saturation png" (Image → GaussianBlur, samples = 2r+1).
use qsg::image::{self, FillMode};
use qsg::{layer, multieffect, raster::Canvas};

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
        let tex = image::load_png(f[7]).unwrap();
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
        let mut p = multieffect::Params {
            blur_enabled: n(4) > 0.0,
            blur: n(5) as f32,
            blur_max: n(4) as i32,
            saturation: n(6) as f32,
            auto_padding: false,
            ..Default::default()
        };
        for o in &f[8..] {
            let (k, val) = o.split_once('=').unwrap();
            let fv = || val.parse::<f32>().unwrap();
            match k {
                "pad" => p.auto_padding = val == "1",
                "bright" => p.brightness = fv(),
                "contrast" => p.contrast = fv(),
                "mult" => p.blur_multiplier = fv(),
                "colz" => {
                    let (a, c) = val.split_once(':').unwrap();
                    p.colorization = a.parse().unwrap();
                    p.colorization_color = hex(c).map(|k| k as u16 * 257);
                }
                "mask" => {
                    let mt = image::load_png(val).unwrap();
                    let mut m = layer::texture(w, h);
                    image::draw(&mut m, &mt, 0.0, 0.0, w, h, FillMode::Stretch, true, 1.0);
                    p.mask = Some(m);
                }
                "mmin" => p.mask_threshold_min = fv(),
                "mspread" => p.mask_spread_at_min = fv(),
                "minv" => p.mask_inverted = val == "1",
                _ => panic!("unknown option {k}"),
            }
        }
        multieffect::draw_with(&mut cv, &item, &p, x, y, 1.0);
    }
    cv.save_png(&args[2]).unwrap();
}
