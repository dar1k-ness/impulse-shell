//! Software raster hot paths: `cargo bench -p qsg --bench raster`.
use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use std::path::{Path, PathBuf};

/// The sidebar's RectangularShadow (445x1030, radius 19, blur 9) on its 492x1077 surface.
fn shadow(c: &mut Criterion) {
    let p = qsg::shadow::ShadowParams {
        width: 445.0,
        height: 1030.0,
        radius: 19.0,
        corners: None,
        blur: 9.0,
        spread: 1.0,
        offset: (0.0, 1.0),
        color: [0, 0, 0, 77 * 257],
        antialiasing: false,
        cached: true,
    };
    c.bench_function("shadow/sidebar", |b| {
        b.iter(|| {
            let mut cv = qsg::raster::Canvas::new(492, 1077, [0, 0, 0, 0]);
            qsg::shadow::draw(&mut cv, black_box(&p), 23.5, 23.5, 1.0);
            cv
        })
    });
}

fn svgs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        match e.file_type() {
            Ok(t) if t.is_dir() => svgs(&p, out),
            Ok(t) if t.is_file() && p.extension().is_some_and(|x| x == "svg") => out.push(p),
            _ => {}
        }
    }
}

/// `Doc::load` and load + render at 32x32 over every SVG asset of the QML tree.
fn svg(c: &mut Criterion) {
    let mut files = vec![];
    svgs(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qml"),
        &mut files,
    );
    files.sort();
    assert!(!files.is_empty(), "no SVGs under qml");
    let paths: Vec<String> = files
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    let mut g = c.benchmark_group("svg/ri_assets");
    g.sample_size(20);
    g.bench_function("load", |b| {
        b.iter(|| {
            for p in &paths {
                black_box(qsg::svg::Doc::load(p).ok());
            }
        })
    });
    g.bench_function("load_render32", |b| {
        b.iter(|| {
            for p in &paths {
                black_box(qsg::svg::Doc::load(p).ok().map(|d| d.render(32, 32).ok()));
            }
        })
    });
    g.finish();
}

/// A picture whose neighbouring pixels all differ, with alpha.
fn picture(w: usize, h: usize) -> qsg::raster::Canvas {
    let mut c = qsg::raster::Canvas::new(w, h, [0, 0, 0, 0]);
    for (n, p) in c.px.iter_mut().enumerate() {
        let (x, y) = (n % w, n / w);
        let a = 128 + ((x ^ y) % 128) as u32;
        let k = |v: usize| ((v % 256) as u32 * a / 255) as u8;
        *p = [k(x * 7), k(y * 5), k((x + y) * 3), a as u8];
    }
    c
}

/// An Image over the screen: a 256x256 picture stretched smoothly to 400x400, the wallpaper
/// as Background.qml draws it (2560x1440 on 1920x1080, `smooth: false`) and the same smoothly.
fn image(c: &mut Criterion) {
    use qsg::image::{FillMode, draw};
    use qsg::raster::Canvas;
    let paint = |tex: &Canvas, [w, h]: [usize; 2], fill: FillMode, smooth: bool| {
        let mut cv = Canvas::new(w, h, [0x20, 0x20, 0x20, 0xff]);
        draw(
            &mut cv, tex, 0.0, 0.0, w as f64, h as f64, fill, smooth, 1.0,
        );
        cv
    };
    let (small, wall) = (picture(256, 256), picture(2560, 1440));
    let mut g = c.benchmark_group("image");
    g.bench_function("stretched", |b| {
        b.iter(|| paint(black_box(&small), [400, 400], FillMode::Stretch, true))
    });
    g.sample_size(20);
    for (name, smooth) in [("wallpaper", false), ("wallpaper_smooth", true)] {
        let fill = FillMode::PreserveAspectCrop;
        g.bench_function(name, |b| {
            b.iter(|| paint(black_box(&wall), [1920, 1080], fill, smooth))
        });
    }
    g.finish();
}

/// The lock screen's GaussianBlur of the wallpaper (Background.qml): 1920x1080, radius 100,
/// 201 samples.
fn blur(c: &mut Criterion) {
    use qsg::raster::Canvas;
    let wall = picture(1920, 1080);
    let mut g = c.benchmark_group("blur");
    g.sample_size(10);
    g.bench_function("lock", |b| {
        b.iter(|| {
            let mut cv = Canvas::new(1920, 1080, [0x20, 0x20, 0x20, 0xff]);
            qsg::blur::gaussian_blur(
                &mut cv,
                black_box(&wall),
                100.0,
                201,
                None,
                false,
                0.0,
                0.0,
                1.0,
            );
            cv
        })
    });
    g.finish();
}

criterion_group!(benches, shadow, svg, image, blur);
criterion_main!(benches);
