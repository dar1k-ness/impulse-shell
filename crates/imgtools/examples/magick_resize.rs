//! `magick_resize SRC SIZE OUT.png` == `magick SRC -resize SIZExSIZE OUT.png` (for benchmarks).

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() != 4 {
        eprintln!("usage: magick_resize SRC SIZE OUT.png");
        std::process::exit(2);
    }
    let t = std::time::Instant::now();
    imgtools::magick::resize_to_png(a[1].as_ref(), a[2].parse().unwrap(), a[3].as_ref()).unwrap();
    eprintln!("{:.1} ms", t.elapsed().as_secs_f64() * 1e3);
}
