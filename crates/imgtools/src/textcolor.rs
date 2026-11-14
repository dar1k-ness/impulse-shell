//! Port of `scripts/images/text_color.py`: background = per-channel median of the four corner
//! pixels, text = per-channel median of the pixels at or beyond the 95th percentile of the
//! Euclidean RGB distance to that background (numpy `percentile`, method "linear").

use crate::cv::Bgr;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextColor {
    pub background: [u8; 3],
    pub text: [u8; 3],
}

/// `np.median(...).astype(int)` of non-negative integers.
fn median_trunc(v: &mut [i32]) -> i32 {
    v.sort_unstable();
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        ((v[n / 2 - 1] as f64 + v[n / 2] as f64) / 2.0) as i32
    }
}

/// `np.percentile(d, q)` with the default linear method (numpy's `_lerp`).
fn percentile(d: &mut [f64], q: f64) -> f64 {
    let n = d.len();
    let vi = (n - 1) as f64 * (q / 100.0);
    let prev = (vi.floor() as usize).min(n - 1);
    let next = (prev + 1).min(n - 1);
    let t = vi - prev as f64;
    let cmp = |a: &f64, b: &f64| a.total_cmp(b);
    let a = *d.select_nth_unstable_by(prev, cmp).1;
    let b = if next == prev {
        a
    } else {
        *d.select_nth_unstable_by(next, cmp).1
    };
    let diff = b - a;
    if t >= 0.5 {
        b - diff * (1.0 - t)
    } else {
        a + diff * t
    }
}

pub fn text_color(img: &Bgr) -> Option<TextColor> {
    let (w, h) = (img.w, img.h);
    if w == 0 || h == 0 {
        return None;
    }
    let rgb = |x, y| {
        let [b, g, r] = img.at(x, y);
        [r as i32, g as i32, b as i32]
    };
    let corners = [rgb(0, 0), rgb(w - 1, 0), rgb(0, h - 1), rgb(w - 1, h - 1)];
    let mut bg = [0i32; 3];
    for (c, out) in bg.iter_mut().enumerate() {
        let mut v: Vec<i32> = corners.iter().map(|p| p[c]).collect();
        *out = median_trunc(&mut v);
    }
    let dist: Vec<f64> = img
        .px
        .as_chunks::<3>()
        .0
        .iter()
        .map(|p| {
            let d = [
                p[2] as i32 - bg[0],
                p[1] as i32 - bg[1],
                p[0] as i32 - bg[2],
            ];
            ((d[0] * d[0] + d[1] * d[1] + d[2] * d[2]) as f64).sqrt()
        })
        .collect();
    let threshold = percentile(&mut dist.clone(), 95.0);
    let mut ch: [Vec<i32>; 3] = Default::default();
    for (p, d) in img.px.as_chunks::<3>().0.iter().zip(&dist) {
        if *d >= threshold {
            ch[0].push(p[2] as i32);
            ch[1].push(p[1] as i32);
            ch[2].push(p[0] as i32);
        }
    }
    let text = if ch[0].is_empty() {
        [255, 255, 255]
    } else {
        ch.each_mut().map(|v| median_trunc(v) as u8)
    };
    Some(TextColor {
        background: bg.map(|v| v as u8),
        text,
    })
}

fn hex(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

/// The script's stdout line (`json.dumps` of its result), without the newline.
pub fn to_json(r: Option<TextColor>) -> String {
    match r {
        Some(t) => format!(
            "{{\"background\": \"{}\", \"text\": \"{}\"}}",
            hex(t.background),
            hex(t.text)
        ),
        None => "{\"error\": \"Could not decode image data\"}".to_string(),
    }
}

/// `magick SRC +repage -crop WxH+X+Y` (IM 7 geometry: offsets truncate, sizes half up,
/// a 0 size means "to the edge"). A crop starting past the image is IM's 1x1 "missed image"
/// (transparent white, which `IMREAD_COLOR` reads as white).
pub fn crop(img: &Bgr, x: f64, y: f64, w: f64, h: f64) -> Bgr {
    let x = x.trunc() as i64;
    let y = y.trunc() as i64;
    let mut w = (w + 0.5).floor() as i64;
    let mut h = (h + 0.5).floor() as i64;
    if w == 0 {
        w = img.w as i64 - x;
    }
    if h == 0 {
        h = img.h as i64 - y;
    }
    let x0 = x.clamp(0, img.w as i64) as usize;
    let y0 = y.clamp(0, img.h as i64) as usize;
    let x1 = (x + w).clamp(0, img.w as i64) as usize;
    let y1 = (y + h).clamp(0, img.h as i64) as usize;
    if x > img.w as i64 || y > img.h as i64 {
        return Bgr {
            w: 1,
            h: 1,
            px: vec![255; 3],
        };
    }
    if x1 <= x0 || y1 <= y0 {
        // transform.c: an empty clip only warns and returns the image uncropped.
        return Bgr {
            w: img.w,
            h: img.h,
            px: img.px.clone(),
        };
    }
    let mut px = Vec::with_capacity((x1 - x0) * (y1 - y0) * 3);
    for row in y0..y1 {
        px.extend_from_slice(&img.px[(row * img.w + x0) * 3..(row * img.w + x1) * 3]);
    }
    Bgr {
        w: x1 - x0,
        h: y1 - y0,
        px,
    }
}

/// What ScreenTextOverlay's `magick … png:- | text-color-venv.sh` pipeline printed. An
/// unreadable file gives magick no output, so the script sees empty stdin.
///
/// `magick` crops the pixels as the file stores them and copies the EXIF block of a JPEG, PNG
/// or WebP into the PNG it writes; OpenCV then turns the crop by that block's orientation. A
/// TIFF's Orientation tag is not such a block, so its crop stays as stored.
pub fn region_json(path: &str, x: f64, y: f64, w: f64, h: f64) -> String {
    use crate::cv::{Via, exif_block_orientation, imdecode_bgr_stored, orient};
    let stored = std::fs::read(path).ok().and_then(|b| {
        Some((
            imdecode_bgr_stored(&b, Via::MagickPng)?,
            exif_block_orientation(&b),
        ))
    });
    match stored {
        Some((img, o)) => {
            let c = crop(&img, x, y, w, h);
            let (w, h, px) = orient(c.w, c.h, 3, c.px, o);
            to_json(text_color(&Bgr { w, h, px }))
        }
        None => "{\"error\": \"No data received via stdin\"}".to_string(),
    }
}
