//! `imgbench N busy|regions|textcolor|thumb ARGS...`: runs the in-process port N times, prints
//! the stdout once and min/median wall ms to stderr (compared against the scripts' `hyperfine`).
use std::time::Instant;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let n: usize = a[0].parse().unwrap();
    let rest = &a[2..];
    let run = || -> String {
        match a[1].as_str() {
            "busy" => imgtools::busy::cli(rest).0,
            "regions" => imgtools::regions::cli(rest).0,
            "textcolor" => {
                let f = |i: usize| rest[i].parse::<f64>().unwrap();
                imgtools::textcolor::region_json(&rest[0], f(1), f(2), f(3), f(4))
            }
            "thumb" => {
                let out = std::path::Path::new(&rest[2]);
                imgtools::magick::resize_to_png(
                    std::path::Path::new(&rest[0]),
                    rest[1].parse().unwrap(),
                    out,
                )
                .unwrap();
                String::new()
            }
            m => panic!("{m}"),
        }
    };
    let mut ms = Vec::new();
    let mut out = String::new();
    for _ in 0..n {
        let t = Instant::now();
        out = run();
        ms.push(t.elapsed().as_secs_f64() * 1e3);
    }
    ms.sort_by(f64::total_cmp);
    print!("{out}");
    eprintln!("min {:.2} ms  median {:.2} ms  (n={n})", ms[0], ms[n / 2]);
}
