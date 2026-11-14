//! Scene format: see oracle canvas_oracle.py (fill subset).
use qsg::canvas::{Cap, Context, Join, Path};
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

const JOIN_MITER: Join = Join::SvgMiter;

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
    /// The open canvas: item rect (x, y, w, h), its context and current path.
    type Open = ((f64, f64, f64, f64), Context, Path);
    let mut cur: Option<Open> = None;
    let mut dash: (Vec<f64>, f64) = (Vec::new(), 0.0);
    const ID: [f64; 6] = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    let mut m = ID;
    let mut saved: Vec<[f64; 6]> = Vec::new();
    // QQuickCanvasItem: image is canvasSize.toSize() (rounded), texture drawn over the item rect.
    let flush = |cv: &mut Canvas, c: Option<Open>| {
        if let Some(((x, y, w, h), ctx, _)) = c {
            if (w, h) == (ctx.img.w as f64, ctx.img.h as f64) {
                layer::draw(cv, &ctx.img, x, y, w, h, 1.0);
            } else {
                let fill = qsg::image::FillMode::Stretch;
                qsg::image::draw(cv, &ctx.img, x, y, w, h, fill, true, 1.0);
            }
        }
    };
    for l in lines {
        let f: Vec<&str> = l.split_whitespace().collect();
        let n = |i: usize| f[i].parse::<f64>().unwrap();
        if f[0] == "canvas" {
            flush(&mut cv, cur.take());
            dash = (Vec::new(), 0.0);
            m = ID;
            saved.clear();
            cur = Some((
                (n(1), n(2), n(3), n(4)),
                Context::new(n(3).round() as usize, n(4).round() as usize),
                Path::default(),
            ));
            continue;
        }
        let (_, ctx, path) = cur.as_mut().unwrap();
        match f[0] {
            "M" => path.move_to(n(1), n(2)),
            "L" => path.line_to(n(1), n(2)),
            "C" => path.bezier_curve_to(n(1), n(2), n(3), n(4), n(5), n(6)),
            "A" => path.arc(n(1), n(2), n(3), n(4), n(5), f[6] == "1"),
            "Z" => path.close_path(),
            "A2" => path.arc_to_points(n(1), n(2), n(3), n(4), n(5)),
            // QQuickContext2D keeps the path in user space: inverse-map it on CTM changes.
            "scale" => {
                let (sx, sy) = (n(1), n(2));
                m = [m[0] * sx, m[1] * sx, m[2] * sy, m[3] * sy, m[4], m[5]];
                *path = path.mapped([1.0 / sx, 0.0, 0.0, 1.0 / sy, 0.0, 0.0]);
            }
            "translate" => {
                let (tx, ty) = (n(1), n(2));
                m[4] += m[0] * tx + m[2] * ty;
                m[5] += m[1] * tx + m[3] * ty;
                *path = path.mapped([1.0, 0.0, 0.0, 1.0, -tx, -ty]);
            }
            "save" => saved.push(m),
            "restore" => m = saved.pop().unwrap_or(ID),
            "fill" if m != ID => {
                ctx.fill_opacity(path, hex(f[1]), false, 256, Some(m));
                *path = Path::default();
            }
            "fill" => {
                ctx.fill(path, hex(f[1]), false);
                *path = Path::default();
            }
            "stroke" => {
                let cap = match f[3] {
                    "round" => Cap::Round,
                    "square" => Cap::Square,
                    _ => Cap::Flat,
                };
                let join = match f[4] {
                    "round" => Join::Round,
                    "bevel" => Join::Bevel,
                    _ => JOIN_MITER,
                };
                if m != ID {
                    elog::eprintln!("canvasscene: transformed stroke approximated");
                }
                let sc = (m[0] * m[3] - m[1] * m[2]).abs().sqrt();
                let dev = path.mapped(m);
                ctx.stroke_dashed(&dev, hex(f[1]), n(2) * sc, cap, join, 10.0, &dash.0, dash.1);
                *path = Path::default();
            }
            // dash offset a b ... (empty list = solid)
            "dash" => dash = (f[2..].iter().map(|v| v.parse().unwrap()).collect(), n(1)),
            // srect #argb width x y w h  (ctx.strokeRect)
            "srect" => {
                let mut r = Path::default();
                r.add_rect(n(3), n(4), n(5), n(6));
                ctx.stroke_dashed(
                    &r,
                    hex(f[1]),
                    n(2),
                    Cap::Flat,
                    JOIN_MITER,
                    10.0,
                    &dash.0,
                    dash.1,
                );
            }
            _ => elog::eprintln!("unsupported op {}", f[0]),
        }
    }
    flush(&mut cv, cur.take());
    cv.save_png(&args[2]).unwrap();
}
