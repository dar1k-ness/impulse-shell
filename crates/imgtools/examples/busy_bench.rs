//! Wall time of `busy::cli` per image: `cargo run --release -p imgtools --example busy_bench -- IMG... `
//! (shell's default arguments, 1920x1080; prints the median of 15 runs after one warm-up).

use std::time::Instant;

fn main() {
    for img in std::env::args().skip(1) {
        let argv: Vec<String> = [
            img.as_str(),
            "--screen-width",
            "1920",
            "--screen-height",
            "1080",
            "--width",
            "300",
            "--height",
            "300",
            "--horizontal-padding",
            "200",
            "--vertical-padding",
            "200",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let (out, _) = imgtools::busy::cli(&argv);
        let mut t: Vec<f64> = (0..15)
            .map(|_| {
                let s = Instant::now();
                std::hint::black_box(imgtools::busy::cli(&argv));
                s.elapsed().as_secs_f64() * 1e3
            })
            .collect();
        t.sort_by(f64::total_cmp);
        println!("{img}\t{:.1} ms\t{}", t[7], out.trim_end());
    }
}
