//! Compare qml::layout against Qt oracle output (layout_oracle.py). usage: layoutcheck PREFIX
use qml::layout::*;

fn main() {
    qml::debug::init_log();
    let p = std::env::args().nth(1).expect("prefix");
    let scen = std::fs::read_to_string(format!("{p}.scen")).unwrap();
    let qt = std::fs::read_to_string(format!("{p}.qt")).unwrap();
    let mut qt_blocks = qt.split("\nE\n").map(|b| {
        b.lines()
            .map(|l| {
                l.split(' ')
                    .skip(1)
                    .map(|v| v.parse::<f64>().unwrap())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
    });
    let mut cur: Option<(usize, f64, [f64; 2], Vec<Child>)> = None;
    let (mut total, mut bad) = (0, 0);
    let mut check = |s: (usize, f64, [f64; 2], Vec<Child>), k: usize| {
        let q = qt_blocks.next().unwrap();
        let l = Linear {
            children: &s.3,
            o: s.0,
            spacing: s.1,
            uniform: false,
        };
        let h = l.size_hints();
        let r = l.arrange(s.2);
        let mut ok = (h[1][0] - q[0][0]).abs() < 1e-6 && (h[1][1] - q[0][1]).abs() < 1e-6;
        for (i, g) in r.iter().enumerate() {
            ok &= g.iter().zip(&q[i + 1]).all(|(a, b)| (a - b).abs() < 1e-6);
        }
        total += 1;
        if !ok {
            bad += 1;
            if bad <= 8 {
                println!(
                    "scenario {k}: o={} sp={} size={:?}\n  rust implicit {:?} rects {:?}\n  qt   {:?}",
                    s.0, s.1, s.2, h[1], r, q
                );
            }
        }
    };
    let mut k = 0;
    for line in scen.lines() {
        let v: Vec<f64> = line
            .split(' ')
            .skip(1)
            .map(|x| x.parse().unwrap())
            .collect();
        if line.starts_with('S') {
            if let Some(s) = cur.take() {
                check(s, k);
                k += 1;
            }
            cur = Some((v[0] as usize, v[1], [v[2], v[3]], Vec::new()));
        } else {
            let o = |x: f64| if x >= 0.0 { Some(x) } else { None };
            let m = v[13];
            let a = Attached {
                min: [o(v[2]), o(v[3])],
                pref: [o(v[4]), o(v[5])],
                max: [o(v[6]), o(v[7])],
                margins: [m; 4],
                ..Attached::new()
            };
            let implicit = [v[0], v[1]];
            let hints = effective_size_hints(&a, implicit, implicit);
            let pol = |f: f64| if f != 0.0 { PREFERRED } else { FIXED };
            cur.as_mut().unwrap().3.push(Child {
                hints,
                policy: [pol(v[8]), pol(v[9])],
                stretch: [v[11] as i32, v[12] as i32],
                align: v[10] as u32,
                margins: [m; 4],
            });
        }
    }
    if let Some(s) = cur.take() {
        check(s, k);
    }
    println!("layout scenarios {total} mismatched {bad}");
}
