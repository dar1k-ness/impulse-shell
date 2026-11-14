//! Scene: "W H bg" then "x y w h fillMode(stretch|fit|crop) smooth(0|1) path [srcW:srcH]" (see image_oracle.py).
use qsg::image::{self, FillMode};
use qsg::raster::Canvas;

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
        let fill = match f[4] {
            "fit" => FillMode::PreserveAspectFit,
            "crop" => FillMode::PreserveAspectCrop,
            _ => FillMode::Stretch,
        };
        let ss = f.get(7).map(|s| {
            let (a, b) = s.split_once(':').unwrap();
            (a.parse::<i32>().unwrap(), b.parse::<i32>().unwrap())
        });
        let tex = if f[6].ends_with(".svg") {
            let doc = qsg::svg::Doc::load(f[6]).unwrap();
            let (dw, dh) = doc.default_size();
            let (lw, lh) = ss.map_or((dw as usize, dh as usize), |(a, b)| {
                qsg::svg::load_size(dw, dh, a, b, fill)
            });
            doc.render(lw, lh).unwrap()
        } else {
            let data = std::fs::read(f[6]).unwrap();
            let sized = |w, h| ss.map_or((w, h), |(a, b)| image::load_size(w, h, a, b, fill));
            image::decode_scaled(&data, sized).unwrap().0
        };
        image::draw(
            &mut cv,
            &tex,
            n(0),
            n(1),
            n(2),
            n(3),
            fill,
            f[5] == "1",
            1.0,
        );
    }
    cv.save_png(&args[2]).unwrap();
}
