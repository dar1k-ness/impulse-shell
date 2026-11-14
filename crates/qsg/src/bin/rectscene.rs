//! Renders a rect scene file: `W H bg` then lines `x y w h radius pen color border aa`.
use qsg::{raster::Canvas, rect};

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
    /// An open clip: its layer canvas, rect and (translation x, y, origin x, y).
    type Clip = (Canvas, [i64; 4], (f32, f32, f64, f64));
    let mut clips: Vec<Clip> = Vec::new();
    for l in lines {
        let f: Vec<&str> = l.split_whitespace().collect();
        if f[0] == "clip" {
            let v = |i: usize| f[i].parse::<f64>().unwrap();
            let (ptx, pty, pax, pay) = clips.last().map_or((0f32, 0f32, 0.0, 0.0), |c| c.2);
            let (tx, ty) = (ptx + (v(1) - pax) as f32, pty + (v(2) - pay) as f32);
            let dev: Vec<usize> = std::env::var("QSG_DEVSIZE")
                .map(|d| d.split('x').map(|t| t.parse().unwrap()).collect())
                .unwrap_or_else(|_| vec![cv.w, cv.h]);
            let mut r = qsg::raster::scissor_rect(tx, ty, v(3), v(4), dev[0], dev[1]);
            if let Some((_, o, _)) = clips.last() {
                r = [
                    r[0].max(o[0]),
                    r[1].max(o[1]),
                    r[2].min(o[2]),
                    r[3].min(o[3]),
                ];
            }
            clips.push((cv.snapshot(), r, (tx, ty, v(1), v(2))));
            continue;
        }
        if f[0] == "endclip" {
            let (saved, r, _) = clips.pop().unwrap();
            cv.apply_scissor(&saved, r);
            continue;
        }
        let n = |i: usize| f[i].parse::<f32>().unwrap();
        // QQuickPen::isValid(): visible colour and a width that rounds to >= 1 (pixelAligned default).
        let border = hex(f[7]);
        let pen = if border[3] > 0 && n(5).round() >= 1.0 {
            n(5)
        } else {
            0.0
        };
        let p = rect::RectParams {
            width: n(2),
            height: n(3),
            radius: n(4),
            pen_width: pen,
            color: hex(f[6]),
            border_color: border,
            antialiasing: f[8] == "1",
            aligned: pen == 0.0,
            vertical: f.get(9).is_none_or(|o| *o != "h"),
            stops: f
                .iter()
                .skip(10)
                .map(|st| {
                    let (t, c) = st.split_once(':').unwrap();
                    (t.parse().unwrap(), hex(c))
                })
                .collect(),
            ..Default::default()
        };
        let g = rect::build(&p);
        cv.draw_smooth(&g, n(0), n(1), 1.0);
    }
    cv.save_png(&args[2]).unwrap();
}
