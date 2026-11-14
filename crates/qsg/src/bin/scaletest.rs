//! scaletest src.png dw dh alpha(0|1) out.raw — dump premultiplied RGBA of qSmoothScaleImage.
use qsg::image;
use std::io::Write;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let src = image::load_png(&a[1]).unwrap();
    let out = image::smooth_scale(
        &src,
        a[2].parse().unwrap(),
        a[3].parse().unwrap(),
        a[4] == "1",
    );
    let mut f = std::fs::File::create(&a[5]).unwrap();
    for p in &out.px {
        f.write_all(p).unwrap();
    }
}
