//! Equivalence with `magick SRC -resize NxN OUT.png`: goldens in tests/fixtures/magick/golden were
//! made by the real ImageMagick 7.1.2-35 Q16-HDRI (tests/magick_golden.sh).

use std::path::{Path, PathBuf};

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/magick")
}

/// Decoded PNG as (w, h, alpha, 16-bit, RGBA samples widened to u16).
fn decode(p: &Path) -> (u32, u32, bool, bool, Vec<u16>) {
    let mut d = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(p).unwrap()));
    d.set_transformations(png::Transformations::EXPAND);
    let mut r = d.read_info().unwrap();
    let mut buf = vec![0; r.output_buffer_size().unwrap()];
    let fi = r.next_frame(&mut buf).unwrap();
    buf.truncate(fi.buffer_size());
    let wide = fi.bit_depth == png::BitDepth::Sixteen;
    let s: Vec<u16> = if wide {
        buf.as_chunks::<2>()
            .0
            .iter()
            .map(|b| u16::from_be_bytes([b[0], b[1]]))
            .collect()
    } else {
        buf.iter().map(|&v| v as u16).collect()
    };
    let top = if wide { 65535 } else { 255 };
    let rgba: Vec<u16> = match fi.color_type {
        png::ColorType::Grayscale => s.iter().flat_map(|&g| [g, g, g, top]).collect(),
        png::ColorType::GrayscaleAlpha => s
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Rgb => s
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2], top])
            .collect(),
        png::ColorType::Rgba => s,
        png::ColorType::Indexed => unreachable!(),
    };
    let alpha = matches!(
        fi.color_type,
        png::ColorType::GrayscaleAlpha | png::ColorType::Rgba
    );
    (fi.width, fi.height, alpha, wide, rgba)
}

#[test]
fn matches_magick() {
    let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("magick");
    std::fs::create_dir_all(&out).unwrap();
    let mut goldens: Vec<PathBuf> = std::fs::read_dir(dir().join("golden"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "png"))
        .collect();
    goldens.sort();
    let mut bad = Vec::new();
    for g in &goldens {
        let name = g.file_name().unwrap().to_str().unwrap();
        let (src, size) = name.strip_suffix(".png").unwrap().rsplit_once('.').unwrap();
        let ours = out.join(name);
        imgtools::magick::resize_to_png(&dir().join(src), size.parse().unwrap(), &ours).unwrap();
        let (a, b) = (decode(&ours), decode(g));
        if (a.0, a.1, a.2, a.3) != (b.0, b.1, b.2, b.3) {
            bad.push(format!(
                "{name}: ours {:?} magick {:?}",
                (a.0, a.1, a.2, a.3),
                (b.0, b.1, b.2, b.3)
            ));
            continue;
        }
        let diffs: Vec<u16> =
            a.4.iter()
                .zip(&b.4)
                .map(|(x, y)| x.abs_diff(*y))
                .filter(|&d| d > 0)
                .collect();
        if !diffs.is_empty() {
            bad.push(format!(
                "{name}: {} of {} samples differ, max abs diff {}",
                diffs.len(),
                a.4.len(),
                diffs.iter().max().unwrap()
            ));
        }
    }
    assert!(goldens.len() >= 60);
    assert!(
        bad.is_empty(),
        "{} mismatches:\n{}",
        bad.len(),
        bad.join("\n")
    );
}
