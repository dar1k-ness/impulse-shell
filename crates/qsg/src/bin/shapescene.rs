//! Scene: "W H bg" then "x y size lineWidth value colPrimary colSecondary layer(0|1)" (the original CircularProgress, see shape_oracle.py).
use qsg::shape::{self, Cap, Join, PathCmd, Stroke};
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
    let mut lines = src.lines().filter(|l| !l.trim().is_empty());
    let hdr: Vec<&str> = lines.next().unwrap().split_whitespace().collect();
    let mut cv = Canvas::new(
        hdr[0].parse().unwrap(),
        hdr[1].parse().unwrap(),
        hex(hdr[2]),
    );
    let dump = std::env::var("SS_DUMP").is_ok();
    for l in lines {
        let f: Vec<&str> = l.split_whitespace().collect();
        let n = |i: usize| f[i].parse::<f64>().unwrap();
        let (x, y, s, lw, val) = (n(0), n(1), n(2), n(3), n(4));
        let deg = val * 360.0;
        let r = s / 2.0 - lw;
        let gap = 360.0 / 18.0;
        let arc = |start: f64, sweep: f64| {
            vec![PathCmd::AngleArc {
                cx: s / 2.0,
                cy: s / 2.0,
                rx: r,
                ry: r,
                start,
                sweep,
                move_to_start: true,
            }]
        };
        let paths = [
            (arc(-90.0 - gap, -(360.0 - deg - 2.0 * gap)), hex(f[6])),
            (arc(-90.0, deg), hex(f[5])),
        ];
        let use_layer = f[7] == "1";
        let mut tex = layer::texture(s, s);
        for (cmds, col) in paths {
            let st = Stroke {
                width: lw as f32,
                color: col,
                cap: Cap::Round,
                join: Join::Bevel,
                miter_limit: 2.0,
            };
            let el = shape::stroke_elements(&cmds, (0.0, 0.0));
            let tris = shape::stroke_tris(&el, &st);
            if dump {
                elog::eprintln!("elements {} tris {}", el.len(), tris.len());
                for e in &el {
                    elog::eprintln!("  {:?}", e);
                }
            }
            if use_layer {
                shape::draw_stroke(&mut tex, &tris, &st, 0.0, 0.0, 1.0);
            } else {
                shape::draw_stroke(&mut cv, &tris, &st, x as f32, y as f32, 1.0);
            }
        }
        if use_layer {
            layer::draw(&mut cv, &tex, x, y, s, s, 1.0);
        }
    }
    cv.save_png(&args[2]).unwrap();
}
