//! Port of `scripts/images/least_busy_region.py` (all modes; AbstractBackgroundWidget uses the
//! default least-busy mode). Every OpenCV step the script runs is reproduced bit for bit:
//! `cv2.imread(.., IMREAD_GRAYSCALE)` (libpng `png_set_rgb_to_gray(1, 0.299, 0.587)` incl. its
//! gamma tables, libjpeg `JCS_GRAYSCALE`), `cv2.resize(.., INTER_LANCZOS4)`, numpy slicing, the
//! float64 integral-image variance, and `cv2.kmeans(.., KMEANS_RANDOM_CENTERS)` on `cv::theRNG()`.

use crate::cv::{self, Gray, Rng, Rows};
use crate::cv_resize::{Window, resize_lanczos4_rows};
use rayon::prelude::*;

/// Arguments of `least_busy_region.py` (defaults are the script's argparse defaults).
#[derive(Clone, Debug)]
pub struct BusyArgs {
    pub image: String,
    pub width: i64,
    pub height: i64,
    pub screen_width: i64,
    pub screen_height: i64,
    pub horizontal_padding: i64,
    pub vertical_padding: i64,
    pub stride: i64,
    pub busiest: bool,
    /// `--screen-mode fit` (default `fill`).
    pub fit: bool,
    /// `--largest-region`.
    pub largest_region: bool,
    /// `--variance-threshold`.
    pub variance_threshold: f64,
    /// `--aspect-ratio`.
    pub aspect_ratio: f64,
    /// `--verbose` (only changes `cli`'s stdout).
    pub verbose: bool,
}

impl Default for BusyArgs {
    fn default() -> Self {
        BusyArgs {
            image: String::new(),
            width: 300,
            height: 200,
            screen_width: 1920,
            screen_height: 1080,
            horizontal_padding: 50,
            vertical_padding: 50,
            stride: 10,
            busiest: false,
            fit: false,
            largest_region: false,
            variance_threshold: 1000.0,
            aspect_ratio: 1.78,
            verbose: false,
        }
    }
}

/// Script result: `center_x`, `center_y`, `width`, `height`, `variance`, `dominant_color`.
#[derive(Clone, Debug, PartialEq)]
pub struct Busy {
    pub center_x: i64,
    pub center_y: i64,
    pub width: i64,
    pub height: i64,
    pub variance: f64,
    pub dominant_color: String,
}

/// `None` where the script prints no result (an exception, or "No region found" in
/// largest-region mode).
pub fn least_busy_region(a: &BusyArgs) -> Option<Busy> {
    run(a, &mut String::new()).ok().flatten()
}

/// `json.dumps` of the script's result dict (no trailing newline).
pub fn to_json(b: &Busy) -> String {
    format!(
        "{{\"center_x\": {}, \"center_y\": {}, \"width\": {}, \"height\": {}, \"variance\": {}, \"dominant_color\": \"{}\"}}",
        b.center_x,
        b.center_y,
        b.width,
        b.height,
        json_float(b.variance),
        b.dominant_color
    )
}

const NOT_FOUND_JSON: &str = "{\"error\": \"No region found under the threshold.\"}";

/// Runs the script for `args` (argv after the script name): its stdout and exit status.
/// argparse errors exit 2 and an exception (e.g. unreadable image) exits 1, with whatever was
/// printed before it; the messages themselves go to stderr in the script and are not reproduced.
pub fn cli(args: &[String]) -> (String, i32) {
    crate::pool::scoped(|| cli_on_pool(args))
}

fn cli_on_pool(args: &[String]) -> (String, i32) {
    let a = match parse_args(args) {
        Ok(a) => a,
        Err(code) => return (String::new(), code),
    };
    let mut out = String::new();
    match run(&a, &mut out) {
        Ok(Some(b)) => {
            out += &to_json(&b);
            out.push('\n');
            (out, 0)
        }
        Ok(None) => {
            out += NOT_FOUND_JSON;
            out.push('\n');
            (out, 0)
        }
        Err(()) => (out, 1),
    }
}

// ---------------------------------------------------------------------------------------------
// Python float formatting

/// `json.dumps` of a float: `float.__repr__`, with `NaN` / `Infinity` / `-Infinity`.
pub fn json_float(v: f64) -> String {
    if v.is_nan() {
        "NaN".into()
    } else if v.is_infinite() {
        if v > 0.0 {
            "Infinity".into()
        } else {
            "-Infinity".into()
        }
    } else {
        py_float_repr(v)
    }
}

/// Python's `repr(float)`: the shortest round-trip digits, positional when the decimal point
/// position is in (-4, 16], scientific (`1e+16`, `1.5e-05`) otherwise.
pub fn py_float_repr(v: f64) -> String {
    if v == 0.0 {
        return if v.is_sign_negative() {
            "-0.0".into()
        } else {
            "0.0".into()
        };
    }
    let e = format!("{:e}", v.abs());
    let (mant, exp) = e.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    let n = digits.len() as i32;
    let decpt = exp + 1;
    let mut s = String::new();
    if v < 0.0 {
        s.push('-');
    }
    if decpt > -4 && decpt <= 16 {
        if decpt <= 0 {
            s += "0.";
            s.extend(std::iter::repeat_n('0', (-decpt) as usize));
            s += &digits;
        } else if decpt >= n {
            s += &digits;
            s.extend(std::iter::repeat_n('0', (decpt - n) as usize));
            s += ".0";
        } else {
            s += &digits[..decpt as usize];
            s.push('.');
            s += &digits[decpt as usize..];
        }
    } else {
        s += &digits[..1];
        if n > 1 {
            s.push('.');
            s += &digits[1..];
        }
        s += &format!("e{}{:02}", if exp < 0 { '-' } else { '+' }, exp.abs());
    }
    s
}

// ---------------------------------------------------------------------------------------------
// cv2.imread(path, IMREAD_GRAYSCALE)

/// `cv::imdecode(buf, IMREAD_GRAYSCALE)`.
pub fn imdecode_gray(b: &[u8]) -> Option<Gray> {
    let r = gray_rows(b)?;
    let (w, h) = (r.w, r.h);
    Some(Gray {
        w,
        h,
        px: r.collect()?,
    })
}

/// [`imdecode_gray`] as rows: PNGs are streamed, other formats decoded whole.
fn gray_rows(b: &[u8]) -> Option<Rows<'_>> {
    if b.len() > 3 && b[0] == 0xff && b[1] == 0xd8 && b[2] == 0xff {
        // grfmt_jpeg.cpp: out_color_space = JCS_GRAYSCALE, i.e. the Y plane; CMYK is converted.
        let img = cv::jpeg(b, turbojpeg::PixelFormat::GRAY).ok()?;
        let px = if img.pitch == img.width {
            img.pixels
        } else {
            img.pixels
                .chunks(img.pitch)
                .flat_map(|r| r[..img.width].iter().copied())
                .collect()
        };
        return Rows::mem(img.width, img.height, 1, px).oriented(cv::jpeg_orientation(b));
    }
    if cv::is_png(b) {
        return png_gray(b);
    }
    // The BMP and TIFF decoders convert their rows with icvCvt_BGR2Gray_8u_C3C1R (14-bit fixed
    // point: 1868, 9617, 4899); the WebP, GIF and AVIF ones decode to BGR and call
    // cvtColor(COLOR_BGR2GRAY) (15-bit: 3735, 19235, 9798).
    let (cb, cg, cr, shift) = if b.starts_with(b"BM") || cv::is_tiff(b) {
        (1868, 9617, 4899, 14)
    } else {
        (3735, 19235, 9798, 15)
    };
    let c = cv::imdecode_bgr(b)?;
    let px =
        c.px.as_chunks::<3>()
            .0
            .iter()
            .map(|p| {
                ((p[0] as u32 * cb + p[1] as u32 * cg + p[2] as u32 * cr + (1 << (shift - 1)))
                    >> shift) as u8
            })
            .collect();
    Some(Rows::mem(c.w, c.h, 1, px))
}

const PNG_FP_1: i32 = 100_000;
const PNG_GAMMA_THRESHOLD_FIXED: i32 = 5000;
const PNG_GAMMA_SRGB_INVERSE: i32 = 45455;
const PNG_MAX_GAMMA_8: u32 = 11;
// png_set_rgb_to_gray_fixed(.., png_fixed(0.299), png_fixed(0.587)): (c * 32768) / 100000.
const RC: u32 = 29_900 * 32_768 / 100_000;
const GC: u32 = 58_700 * 32_768 / 100_000;
const BC: u32 = 32_768 - RC - GC;

fn gamma_significant(g: i32) -> bool {
    !(PNG_FP_1 - PNG_GAMMA_THRESHOLD_FIXED..=PNG_FP_1 + PNG_GAMMA_THRESHOLD_FIXED).contains(&g)
}

fn png_reciprocal(a: i32) -> i32 {
    let r = (1e10 / a as f64 + 0.5).floor();
    if (-2147483648.0..=2147483647.0).contains(&r) {
        r as i32
    } else {
        0
    }
}

fn png_reciprocal2(a: i32, b: i32) -> i32 {
    if a != 0 && b != 0 {
        let r = ((1e15 / a as f64) / b as f64 + 0.5).floor();
        if (-2147483648.0..=2147483647.0).contains(&r) {
            return r as i32;
        }
    }
    0
}

fn pow(x: f64, y: f64) -> f64 {
    // libpng calls libm pow(); keep LLVM from folding special exponents.
    std::hint::black_box(x).powf(std::hint::black_box(y))
}

fn table8(g: i32) -> [u8; 256] {
    std::array::from_fn(|i| {
        if gamma_significant(g) && i > 0 && i < 255 {
            (255.0 * pow(i as f64 / 255.0, g as f64 * 0.00001) + 0.5).floor() as u8
        } else {
            i as u8
        }
    })
}

fn gamma16_correct(v: u32, g: i32) -> u32 {
    if v > 0 && v < 65535 {
        (65535.0 * pow(v as f64 / 65535.0, g as f64 * 0.00001) + 0.5).floor() as u32
    } else {
        v
    }
}

/// libpng 16-bit tables, `t[(v & 0xff) >> shift][v >> 8]`.
struct Table16 {
    shift: u32,
    t: Vec<[u16; 256]>,
}

impl Table16 {
    fn get(&self, v: u16) -> u16 {
        self.t[((v & 0xff) as u32 >> self.shift) as usize][(v >> 8) as usize]
    }

    /// png_build_16bit_table (floating-point build).
    fn build(shift: u32, g: i32) -> Self {
        let num = 1usize << (8 - shift);
        let fmax = 1.0 / (((1i32 << (16 - shift)) - 1) as f64);
        let max = (1u32 << (16 - shift)) - 1;
        let max_by_2 = 1u32 << (15 - shift);
        let t = (0..num as u32)
            .map(|i| {
                std::array::from_fn(|j| {
                    let ig = ((j as u32) << (8 - shift)) + i;
                    if gamma_significant(g) {
                        (65535.0 * pow(ig as f64 * fmax, g as f64 * 0.00001) + 0.5).floor() as u16
                    } else if shift != 0 {
                        ((ig * 65535 + max_by_2) / max) as u16
                    } else {
                        ig as u16
                    }
                })
            })
            .collect();
        Table16 { shift, t }
    }

    /// png_build_16to8_table; `g` is the inverse of the overall correction.
    fn build_16to8(shift: u32, g: i32) -> Self {
        let num = 1u32 << (8 - shift);
        let max = (1u32 << (16 - shift)) - 1;
        let mut t = vec![[0u16; 256]; num as usize];
        let mut last = 0u32;
        let mut put = |last: u32, v: u16| {
            t[(last & (0xff >> shift)) as usize][(last >> (8 - shift)) as usize] = v
        };
        for i in 0..255u32 {
            let out = i * 257;
            let bound = gamma16_correct(out + 128, g);
            let bound = (bound as u64 * max as u64 + 32768) / 65535 + 1;
            while (last as u64) < bound {
                put(last, out as u16);
                last += 1;
            }
        }
        while last < (num << 8) {
            put(last, 65535);
            last += 1;
        }
        Table16 { shift, t }
    }
}

/// OpenCV's PngDecoder with a 1-channel destination: strip_16 (chop), strip_alpha,
/// palette_to_rgb, expand_gray_1_2_4_to_8, rgb_to_gray(1, 0.299, 0.587), on libpng 1.6.
fn png_gray(b: &[u8]) -> Option<Rows<'_>> {
    cv::png_rows(b, 1, png_gray_conv(b)?)
}

/// A PNG decoded once into interleaved `[grey, b, g, r]` rows: [`png_gray`] and the PNG branch
/// of [`cv::imdecode_bgr`] side by side. Lanczos resizes each channel on its own, so one
/// 4-channel resize gives both of the script's images.
fn png_gray_bgr(b: &[u8]) -> Option<Rows<'_>> {
    let gray = png_gray_conv(b)?;
    cv::png_rows(b, 4, move |oc, od| {
        let (gray, bgr) = (gray(oc, od)?, cv::bgr_conv(oc, od)?);
        let conv: cv::Conv = Box::new(move |row, out| {
            let n = out.len() / 4;
            let (mut g, mut c) = (vec![0u8; n], vec![0u8; 3 * n]);
            gray(row, &mut g);
            bgr(row, &mut c);
            for ((o, &g), c) in out
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(&g)
                .zip(c.as_chunks::<3>().0)
            {
                *o = [g, c[0], c[1], c[2]];
            }
        });
        Some(conv)
    })
}

/// The per-row conversion of [`png_gray`], given the file's chunks.
fn png_gray_conv(
    b: &[u8],
) -> Option<impl FnOnce(png::ColorType, png::BitDepth) -> Option<cv::Conv>> {
    let ihdr = b.get(16..29)?;
    let (bit_depth, color_type) = (ihdr[8], ihdr[9]);
    let channels = match color_type {
        0 | 3 => 1,
        2 => 3,
        4 => 2,
        6 => 4,
        _ => return None,
    };
    // Chunk state as libpng's readers leave it (PNGv3 gamma precedence: cICP, sRGB, gAMA).
    let (mut chunk_gamma, mut has_cicp, mut sig_bit) = (0i32, false, [0u8; 3]);
    let (mut before_data, mut seen_gama, mut seen_srgb, mut seen_sbit) =
        (true, false, false, false);
    cv::png_chunks(b, |tag, d| match tag {
        b"PLTE" | b"IDAT" => before_data = false,
        b"cICP" if before_data && d.len() == 4 => has_cicp = true,
        b"gAMA" if before_data && !seen_gama && d.len() == 4 => {
            seen_gama = true;
            let g = u32::from_be_bytes([d[0], d[1], d[2], d[3]]);
            if g <= i32::MAX as u32 && chunk_gamma == 0 {
                chunk_gamma = g as i32;
            }
        }
        b"sRGB" if before_data && !seen_srgb && d.len() == 1 && d[0] <= 3 => {
            seen_srgb = true;
            if !has_cicp || chunk_gamma == 0 {
                chunk_gamma = PNG_GAMMA_SRGB_INVERSE;
            }
        }
        b"sBIT" if before_data && !seen_sbit => {
            seen_sbit = true;
            let (truelen, depth) = if color_type == 3 {
                (3, 8)
            } else {
                (channels, bit_depth)
            };
            if d.len() == truelen && d.iter().all(|&v| v != 0 && v <= depth) {
                sig_bit = if color_type & 2 != 0 {
                    [d[0], d[1], d[2]]
                } else {
                    [d[0]; 3]
                };
            }
        }
        _ => {}
    });

    Some(move |oc: png::ColorType, od: png::BitDepth| {
        let ocn = oc.samples();
        let wide = od == png::BitDepth::Sixteen;
        let colour = matches!(oc, png::ColorType::Rgb | png::ColorType::Rgba);
        // png_init_gamma_values / png_init_read_transformations: no screen gamma is set, so
        // tables exist only for RGB_TO_GRAY when the file gamma is significant.
        let (file_gamma, screen_gamma) = if chunk_gamma > 0 {
            (chunk_gamma, png_reciprocal(chunk_gamma))
        } else {
            (PNG_FP_1, PNG_FP_1)
        };
        let tables = gamma_significant(file_gamma) || gamma_significant(screen_gamma);
        let file_to_linear = png_reciprocal(file_gamma);
        let (linear_to_screen, correction) = if screen_gamma > 0 {
            (
                png_reciprocal(screen_gamma),
                png_reciprocal2(screen_gamma, file_gamma),
            )
        } else {
            (file_gamma, PNG_FP_1)
        };
        let sample = move |row: &[u8], i: usize| -> u16 {
            if wide {
                u16::from_be_bytes([row[2 * i], row[2 * i + 1]])
            } else {
                row[i] as u16
            }
        };
        let conv: cv::Conv = if !colour {
            Box::new(move |row, out| {
                for (x, o) in out.iter_mut().enumerate() {
                    let v = sample(row, x * ocn);
                    *o = if wide { (v >> 8) as u8 } else { v as u8 };
                }
            })
        } else if !wide {
            let lut = tables.then(|| {
                (
                    table8(file_to_linear),
                    table8(linear_to_screen),
                    table8(correction),
                )
            });
            Box::new(move |row, out| {
                for (x, o) in out.iter_mut().enumerate() {
                    let (r, g, b_) = (
                        row[x * ocn] as u32,
                        row[x * ocn + 1] as u32,
                        row[x * ocn + 2] as u32,
                    );
                    *o = match &lut {
                        Some((to1, from1, gt)) => {
                            if r != g || r != b_ {
                                let (r, g, b_) = (
                                    to1[r as usize] as u32,
                                    to1[g as usize] as u32,
                                    to1[b_ as usize] as u32,
                                );
                                from1[((RC * r + GC * g + BC * b_ + 16384) >> 15) as usize]
                            } else {
                                gt[r as usize]
                            }
                        }
                        None if r != g || r != b_ => ((RC * r + GC * g + BC * b_) >> 15) as u8,
                        None => r as u8,
                    };
                }
            })
        } else {
            let lut = tables.then(|| {
                let sb = sig_bit.iter().copied().max().unwrap() as u32;
                let shift = if sb > 0 && sb < 16 { 16 - sb } else { 0 };
                let shift = shift.clamp(16 - PNG_MAX_GAMMA_8, 8);
                (
                    Table16::build(shift, file_to_linear),
                    Table16::build(shift, linear_to_screen),
                    Table16::build_16to8(shift, png_reciprocal(correction)),
                )
            });
            Box::new(move |row, out| {
                for (x, o) in out.iter_mut().enumerate() {
                    let (r, g, b_) = (
                        sample(row, x * ocn),
                        sample(row, x * ocn + 1),
                        sample(row, x * ocn + 2),
                    );
                    let v = match &lut {
                        Some((to1, from1, gt)) => {
                            if r == g && r == b_ {
                                gt.get(r)
                            } else {
                                let (r1, g1, b1) =
                                    (to1.get(r) as u32, to1.get(g) as u32, to1.get(b_) as u32);
                                from1.get(((RC * r1 + GC * g1 + BC * b1 + 16384) >> 15) as u16)
                            }
                        }
                        None => {
                            ((RC * r as u32 + GC * g as u32 + BC * b_ as u32 + 16384) >> 15) as u16
                        }
                    };
                    *o = (v >> 8) as u8;
                }
            })
        };
        Some(conv)
    })
}

// ---------------------------------------------------------------------------------------------
// Scaling and numpy helpers

/// Python `a // b`.
fn floordiv(a: i64, b: i64) -> i64 {
    let q = a / b;
    if (a % b != 0) && ((a < 0) != (b < 0)) {
        q - 1
    } else {
        q
    }
}

/// Python `int(x)` of a float (`Err` for inf/nan, which raise).
fn py_int(x: f64) -> Result<i64, ()> {
    if x.is_finite() {
        Ok(x.trunc() as i64)
    } else {
        Err(())
    }
}

/// numpy `a[start:stop]` bounds for a step-1 slice of an axis of length `n`.
fn py_slice(start: i64, stop: i64, n: i64) -> (usize, usize) {
    let norm = |i: i64| if i < 0 { (i + n).max(0) } else { i.min(n) };
    let (s, e) = (norm(start), norm(stop));
    (s as usize, e.max(s) as usize)
}

/// An interleaved 8-bit image (grey, or BGR / grey+BGR where noted).
struct Img {
    w: usize,
    h: usize,
    px: Vec<u8>,
}

/// Where the script's scale-to-screen (`cv2.resize` INTER_LANCZOS4) and `center_crop` put a
/// `ow`x`oh` image: the `nw`x`nh` resize and the kept `win` of it.
struct Geom {
    nw: usize,
    nh: usize,
    win: Window,
}

fn scale_geom(ow: usize, oh: usize, a: &BusyArgs, log: Option<&mut String>) -> Result<Geom, ()> {
    let (ow, oh) = (ow as i64, oh as i64);
    let (sw, sh) = (a.screen_width, a.screen_height);
    let scale_w = sw as f64 / ow as f64;
    let scale_h = sh as f64 / oh as f64;
    let scale = if !a.fit {
        if scale_h > scale_w { scale_h } else { scale_w }
    } else if scale_h < scale_w {
        scale_h
    } else {
        scale_w
    };
    let new_w = py_int(ow as f64 * scale)?;
    let new_h = py_int(oh as f64 * scale)?;
    if let Some(log) = log {
        let mode = if a.fit { "fit" } else { "fill" };
        *log += &format!(
            "Scaling image from {ow}x{oh} to {new_w}x{new_h} (scale: {scale:.3}, mode: {mode})\n"
        );
    }
    if new_w <= 0 || new_h <= 0 {
        return Err(()); // cv2.resize: !dsize.empty() assertion
    }
    let x1 = floordiv(new_w - sw, 2).max(0);
    let y1 = floordiv(new_h - sh, 2).max(0);
    let (x0, x1) = py_slice(x1, x1 + sw, new_w);
    let (y0, y1) = py_slice(y1, y1 + sh, new_h);
    Ok(Geom {
        nw: new_w as usize,
        nh: new_h as usize,
        win: Window { x0, x1, y0, y1 },
    })
}

/// Scale-to-screen and `center_crop` of a decoded image, resizing only the kept window as its
/// rows arrive. The scaling message goes to `log` unless decoding fails.
fn scale_crop(mut r: Rows<'_>, a: &BusyArgs, log: Option<&mut String>) -> Result<Img, ()> {
    let mut msg = String::new();
    let geom = scale_geom(r.w, r.h, a, log.is_some().then_some(&mut msg));
    let px = geom.as_ref().ok().and_then(|&Geom { nw, nh, win }| {
        resize_lanczos4_rows(r.w, r.h, r.cn, nw, nh, win, &mut |row| (r.next)(Some(row)))
    });
    if let Some(log) = log
        && (geom.is_err() || px.is_some())
    {
        *log += &msg;
    }
    let (win, px) = (geom?.win, px.ok_or(())?);
    Ok(Img {
        w: win.x1 - win.x0,
        h: win.y1 - win.y0,
        px,
    })
}

#[doc(hidden)]
#[allow(clippy::type_complexity)]
pub fn scaled_for_test(
    w: usize,
    h: usize,
    cn: usize,
    px: &[u8],
    sw: i64,
    sh: i64,
    fit: bool,
) -> (usize, usize, Vec<u8>) {
    let a = BusyArgs {
        screen_width: sw,
        screen_height: sh,
        fit,
        ..Default::default()
    };
    let i = scale_crop(Rows::mem(w, h, cn, px.to_vec()), &a, None).unwrap();
    (i.w, i.h, i.px)
}

/// `cv2.integral(arr)[1:, 1:]` of `arr` and `arr**2`. The float64 sums hold integers below
/// 2^53, so exact integer accumulation gives the same values.
struct Integral {
    w: i64,
    h: i64,
    s: Vec<i64>,
    s2: Vec<i64>,
}

impl Integral {
    fn new(g: &Img) -> Self {
        let (w, h) = (g.w, g.h);
        let (mut s, mut s2) = (vec![0i64; w * h], vec![0i64; w * h]);
        for y in 0..h {
            let (mut r, mut r2) = (0i64, 0i64);
            for x in 0..w {
                let v = g.px[y * w + x] as i64;
                r += v;
                r2 += v * v;
                let up = if y > 0 { s[(y - 1) * w + x] } else { 0 };
                let up2 = if y > 0 { s2[(y - 1) * w + x] } else { 0 };
                s[y * w + x] = up + r;
                s2[y * w + x] = up2 + r2;
            }
        }
        Integral {
            w: w as i64,
            h: h as i64,
            s,
            s2,
        }
    }

    /// `ii[y, x]` with numpy negative-index wrap-around (`Err` = IndexError).
    fn at(&self, sq: bool, y: i64, x: i64) -> Result<i64, ()> {
        let y = if y < 0 { y + self.h } else { y };
        let x = if x < 0 { x + self.w } else { x };
        if y < 0 || x < 0 || y >= self.h || x >= self.w {
            return Err(());
        }
        let i = (y * self.w + x) as usize;
        Ok(if sq { self.s2[i] } else { self.s[i] })
    }

    /// The script's `region_sum`.
    fn region_sum(&self, sq: bool, x1: i64, y1: i64, x2: i64, y2: i64) -> Result<f64, ()> {
        let mut t = self.at(sq, y2, x2)?;
        if x1 > 0 {
            t -= self.at(sq, y2, x1 - 1)?;
        }
        if y1 > 0 {
            t -= self.at(sq, y1 - 1, x2)?;
        }
        if x1 > 0 && y1 > 0 {
            t += self.at(sq, y1 - 1, x1 - 1)?;
        }
        Ok(t as f64)
    }

    /// `(s2 / area) - (mean ** 2)` in numpy float64 scalar arithmetic.
    fn variance(&self, x: i64, y: i64, rw: i64, rh: i64) -> Result<f64, ()> {
        let (x2, y2) = (x + rw - 1, y + rh - 1);
        let s = self.region_sum(false, x, y, x2, y2)?;
        let s2 = self.region_sum(true, x, y, x2, y2)?;
        Ok(variance_of(s, s2, rw, rh))
    }
}

fn variance_of(s: f64, s2: f64, rw: i64, rh: i64) -> f64 {
    let area = (rw * rh) as f64;
    let mean = s / area;
    (s2 / area) - pow(mean, 2.0)
}

/// Sums of `px` and `px**2` over each `rw`x`rh` window of the rows the scan is at, from
/// per-column sums slid down the image: the integral's exact sums in `O(w)` memory.
struct Windows<'a> {
    g: &'a Img,
    rh: usize,
    y: Option<usize>,
    col: Vec<u64>,
    col2: Vec<u64>,
    pre: Vec<u64>,
    pre2: Vec<u64>,
}

impl<'a> Windows<'a> {
    fn new(g: &'a Img, rh: usize) -> Self {
        let w = g.w;
        Windows {
            g,
            rh,
            y: None,
            col: vec![0; w],
            col2: vec![0; w],
            pre: vec![0; w + 1],
            pre2: vec![0; w + 1],
        }
    }

    fn add_row(&mut self, y: usize, sub: bool) {
        let row = &self.g.px[y * self.g.w..(y + 1) * self.g.w];
        for ((c, c2), &v) in self.col.iter_mut().zip(&mut self.col2).zip(row) {
            let v = v as u64;
            if sub {
                *c -= v;
                *c2 -= v * v;
            } else {
                *c += v;
                *c2 += v * v;
            }
        }
    }

    /// Moves the windows to rows `y..y + rh` (`y` only increases).
    fn seek(&mut self, y: usize) {
        match self.y {
            Some(p) if y - p < self.rh => {
                for r in p..y {
                    self.add_row(r, true);
                    self.add_row(r + self.rh, false);
                }
            }
            _ => {
                self.col.fill(0);
                self.col2.fill(0);
                for r in y..y + self.rh {
                    self.add_row(r, false);
                }
            }
        }
        self.y = Some(y);
        for x in 0..self.g.w {
            self.pre[x + 1] = self.pre[x] + self.col[x];
            self.pre2[x + 1] = self.pre2[x] + self.col2[x];
        }
    }

    fn variance(&self, x: usize, rw: usize) -> f64 {
        let s = self.pre[x + rw] - self.pre[x];
        let s2 = self.pre2[x + rw] - self.pre2[x];
        variance_of(s as f64, s2 as f64, rw as i64, self.rh as i64)
    }
}

fn padding_fit(hp: i64, vp: i64, w: i64, h: i64) -> (i64, i64) {
    if hp * 2 >= w || vp * 2 >= h {
        (
            hp.min(floordiv(w - 1, 2)).max(0),
            vp.min(floordiv(h - 1, 2)).max(0),
        )
    } else {
        (hp, vp)
    }
}

/// `find_least_busy_region`: top-left corner and variance.
fn find_least_busy(g: &Img, a: &BusyArgs, log: &mut String) -> Result<((i64, i64), f64), ()> {
    let (w, h) = (g.w as i64, g.h as i64);
    let stride = a.stride.max(1);
    let (hp, vp) = padding_fit(a.horizontal_padding, a.vertical_padding, w, h);
    let (max_w, max_h) = (w - 2 * hp, h - 2 * vp);
    if max_w <= 0 || max_h <= 0 {
        return Err(()); // ValueError("Image too small for the specified padding.")
    }
    let (mut rw, mut rh) = (a.width, a.height);
    if rw > max_w {
        if a.verbose {
            *log += &format!("Requested region_width {rw} too large; clamping to {max_w}\n");
        }
        rw = max_w;
    }
    if rh > max_h {
        if a.verbose {
            *log += &format!("Requested region_height {rh} too large; clamping to {max_h}\n");
        }
        rh = max_h;
    }
    // Non-negative paddings and sizes never index outside the image, so every window can come
    // from sliding sums; otherwise numpy's negative-index wrap-around needs the full integral.
    let slide = hp >= 0 && vp >= 0 && rw > 0 && rh > 0;
    let ri = (!slide).then(|| Integral::new(g));
    let mut win = slide.then(|| Windows::new(g, rh as usize));
    let (mut min_var, mut max_var) = (None::<f64>, None::<f64>);
    let (mut min_c, mut max_c) = ((hp, vp), (hp, vp));
    let (xs, ys) = (hp, vp);
    let xe = (w - rw - hp + 1).max(xs);
    let ye = (h - rh - vp + 1).max(ys);
    let mut y = ys;
    while y <= ye {
        if let Some(win) = win.as_mut()
            && y + rh - 1 < h
        {
            win.seek(y as usize);
        }
        let mut x = xs;
        while x <= xe {
            if x + rw - 1 < w && y + rh - 1 < h {
                let var = match (&ri, &win) {
                    (Some(ri), _) => ri.variance(x, y, rw, rh)?,
                    (None, Some(win)) => win.variance(x as usize, rw as usize),
                    (None, None) => unreachable!(),
                };
                if min_var.is_none_or(|m| var < m) {
                    min_var = Some(var);
                    min_c = (x, y);
                }
                if max_var.is_none_or(|m| var > m) {
                    max_var = Some(var);
                    max_c = (x, y);
                }
            }
            x += stride;
        }
        y += stride;
    }
    // At least the (hp, vp) window always fits, so the variances are set.
    Ok(if a.busiest {
        (max_c, max_var.ok_or(())?)
    } else {
        (min_c, min_var.ok_or(())?)
    })
}

/// Python `round(x)` to int (banker's rounding; `Err` for inf/nan).
fn py_round(x: f64) -> Result<i64, ()> {
    py_int(x.round_ties_even())
}

/// `find_largest_region`: center, size and variance, or `None` when nothing is under the
/// threshold.
#[allow(clippy::type_complexity)]
fn find_largest(g: &Img, a: &BusyArgs) -> Result<Option<((i64, i64), (i64, i64), f64)>, ()> {
    let (w, h) = (g.w as i64, g.h as i64);
    if w == 0 || h == 0 {
        return Err(()); // not reachable with a positive screen size
    }
    let stride = a.stride.max(1);
    let threshold = if a.variance_threshold > 0.0 {
        a.variance_threshold
    } else {
        0.0
    };
    let (hp, vp) = padding_fit(a.horizontal_padding, a.vertical_padding, w, h);
    let ri = Integral::new(g);
    let (ew, eh) = (w - 2 * hp, h - 2 * vp);
    if ew <= 0 || eh <= 0 {
        return Ok(None);
    }
    let ar = a.aspect_ratio;
    let mut min_size = 10i64;
    let mut max_size = if ar >= 1.0 {
        eh.min(py_int(ew as f64 / ar)?)
    } else {
        py_int(eh as f64 * ar)?.min(ew)
    };
    if max_size < min_size {
        min_size = 1;
        max_size = max_size.max(1);
    }
    let mut best = None;
    while min_size <= max_size {
        let mid = floordiv(min_size + max_size, 2);
        let (rw, rh) = if ar >= 1.0 {
            (py_round(mid as f64 * ar)?, mid)
        } else {
            (
                mid,
                py_round(if ar != 0.0 {
                    mid as f64 / ar
                } else {
                    mid as f64
                })?,
            )
        };
        if rw <= 0 || rh <= 0 {
            break;
        }
        if rw > ew || rh > eh {
            max_size = mid - 1;
            continue;
        }
        let mut found = false;
        let (xe, ye) = (w - rw - hp, h - rh - vp);
        let mut y = vp;
        'scan: while y <= ye {
            let mut x = hp;
            while x <= xe {
                if x + rw - 1 < w && y + rh - 1 < h {
                    let var = ri.variance(x, y, rw, rh)?;
                    if var <= threshold {
                        found = true;
                        best = Some((x, y, rw, rh, var));
                        break 'scan;
                    }
                }
                x += stride;
            }
            y += stride;
        }
        if found {
            min_size = mid + 1;
        } else {
            max_size = mid - 1;
        }
    }
    Ok(best.map(|(x, y, rw, rh, var)| ((x + floordiv(rw, 2), y + floordiv(rh, 2)), (rw, rh), var)))
}

// ---------------------------------------------------------------------------------------------
// get_dominant_color: cv2.kmeans(region, K, None, (EPS + MAX_ITER, 10, 1.0), 10,
// KMEANS_RANDOM_CENTERS), modules/core/src/kmeans.cpp.

/// `hal::normL2Sqr_` for 3 floats (the SIMD loop needs >= 16 elements; the tail is scalar).
fn norm_l2_sqr(a: &[f32; 3], b: &[f32; 3]) -> f32 {
    let mut d = 0f32;
    for j in 0..3 {
        let t = a[j] - b[j];
        d += t * t;
    }
    d
}

/// `cv::sum` of a CV_64F row: `sum_<double, double>` unrolled by 4.
fn cv_sum(v: &[f64]) -> f64 {
    let mut s0 = 0f64;
    let mut i = 0;
    while i + 4 <= v.len() {
        s0 += v[i] + v[i + 1] + v[i + 2] + v[i + 3];
        i += 4;
    }
    for &x in &v[i..] {
        s0 += x;
    }
    s0
}

/// `cv::kmeans` with KMEANS_RANDOM_CENTERS on 3-dim data; returns (labels, centers).
fn kmeans(
    data: &[[f32; 3]],
    k: usize,
    attempts: usize,
    max_count: usize,
    eps: f64,
    rng: &mut Rng,
) -> (Vec<usize>, Vec<[f32; 3]>) {
    let n = data.len();
    let (attempts, max_count) = if k == 1 {
        (1, 2)
    } else {
        (attempts.max(1), max_count.clamp(2, 100))
    };
    let eps = eps.max(0.0) * eps.max(0.0);
    let mut bx = [[0f32; 2]; 3];
    for j in 0..3 {
        bx[j] = [data[0][j], data[0][j]];
    }
    for s in &data[1..] {
        for j in 0..3 {
            if s[j] < bx[j][0] {
                bx[j][0] = s[j];
            }
            if bx[j][1] < s[j] {
                bx[j][1] = s[j];
            }
        }
    }
    let mut labels = vec![0usize; n];
    let mut dists = vec![0f64; n];
    let mut centers = vec![[0f32; 3]; k];
    let mut old_centers = vec![[0f32; 3]; k];
    let mut counters = vec![0i32; k];
    let mut best = (f64::MAX, Vec::new(), Vec::new());
    for _ in 0..attempts {
        let compactness;
        let mut iter = 0;
        loop {
            let mut max_shift = if iter == 0 { f64::MAX } else { 0.0 };
            std::mem::swap(&mut centers, &mut old_centers);
            if iter == 0 {
                let margin = 1f32 / 3f32;
                for c in centers.iter_mut() {
                    for j in 0..3 {
                        let r = rng.next() as f32 * 2.328_306_4e-10_f32;
                        c[j] = (r * (1f32 + margin * 2f32) - margin) * (bx[j][1] - bx[j][0])
                            + bx[j][0];
                    }
                }
            } else {
                for c in centers.iter_mut() {
                    *c = [0.0; 3];
                }
                counters.iter_mut().for_each(|c| *c = 0);
                for (s, &l) in data.iter().zip(&labels) {
                    for j in 0..3 {
                        centers[l][j] += s[j];
                    }
                    counters[l] += 1;
                }
                for kk in 0..k {
                    if counters[kk] != 0 {
                        continue;
                    }
                    let mut max_k = 0;
                    for k1 in 1..k {
                        if counters[max_k] < counters[k1] {
                            max_k = k1;
                        }
                    }
                    let scale = 1f32 / counters[max_k] as f32;
                    let base: [f32; 3] = std::array::from_fn(|j| centers[max_k][j] * scale);
                    let (mut max_dist, mut far) = (0f64, usize::MAX);
                    for i in 0..n {
                        if labels[i] != max_k {
                            continue;
                        }
                        let d = norm_l2_sqr(&data[i], &base) as f64;
                        if max_dist <= d {
                            max_dist = d;
                            far = i;
                        }
                    }
                    counters[max_k] -= 1;
                    counters[kk] += 1;
                    labels[far] = kk;
                    for j in 0..3 {
                        centers[max_k][j] -= data[far][j];
                        centers[kk][j] += data[far][j];
                    }
                }
                for kk in 0..k {
                    let scale = 1f32 / counters[kk] as f32;
                    for v in centers[kk].iter_mut() {
                        *v *= scale;
                    }
                    if iter > 0 {
                        let mut d = 0f64;
                        for j in 0..3 {
                            let t = (centers[kk][j] - old_centers[kk][j]) as f64;
                            d += t * t;
                        }
                        if max_shift < d {
                            max_shift = d;
                        }
                    }
                }
            }
            iter += 1;
            if iter == max_count || max_shift <= eps {
                dists
                    .par_iter_mut()
                    .zip(data.par_iter().zip(labels.par_iter()))
                    .for_each(|(d, (s, &l))| {
                        *d = norm_l2_sqr(s, &centers[l]) as f64;
                    });
                compactness = cv_sum(&dists);
                break;
            }
            // KMeansDistanceComputer<false>: per-sample, so the parallel split is exact.
            dists
                .par_iter_mut()
                .zip(labels.par_iter_mut())
                .zip(data.par_iter())
                .for_each(|((d, l), s)| {
                    let (mut kb, mut md) = (0, f64::MAX);
                    for (kk, c) in centers.iter().enumerate() {
                        let dist = norm_l2_sqr(s, c) as f64;
                        if md > dist {
                            md = dist;
                            kb = kk;
                        }
                    }
                    *d = md;
                    *l = kb;
                });
        }
        if compactness < best.0 {
            best = (compactness, labels.clone(), centers.clone());
        }
    }
    (best.1, best.2)
}

/// The region `get_dominant_color` takes of a `cw`x`ch` image: column and row ranges.
fn color_bounds(
    cw: usize,
    ch: usize,
    x: i64,
    y: i64,
    w: i64,
    h: i64,
) -> (usize, usize, usize, usize) {
    let (cw, ch) = (cw as i64, ch as i64);
    let (x, y) = (x.max(0), y.max(0));
    let w = w.min(cw - x).max(1);
    let h = h.min(ch - y).max(1);
    let (xs, xe) = py_slice(x, x + w, cw);
    let (ys, ye) = py_slice(y, y + h, ch);
    (xs, xe, ys, ye)
}

/// `get_dominant_color` of a BGR region as `#rrggbb`.
fn dominant_color(region: &[[u8; 3]]) -> String {
    let rgb = if region.is_empty() {
        [0, 0, 0]
    } else {
        let mut pts: Vec<[f32; 3]> = region
            .iter()
            .filter(|p| p.iter().any(|&v| v > 10))
            .map(|p| p.map(|v| v as f32))
            .collect();
        if pts.is_empty() {
            pts = region.iter().map(|p| p.map(|v| v as f32)).collect();
        }
        if pts.len() < 3 {
            // The script returns this mean without its BGR -> RGB reversal.
            let n = pts.len() as f32;
            let m: [i64; 3] =
                std::array::from_fn(|j| (pts.iter().fold(0f32, |s, p| s + p[j]) / n) as i64);
            return format!("#{:02x}{:02x}{:02x}", m[0], m[1], m[2]);
        }
        let bgr = {
            let k = pts.len().min(3);
            let (labels, centers) = kmeans(&pts, k, 10, 10, 1.0, &mut Rng::default());
            let mut counts = vec![0usize; k];
            for &l in &labels {
                counts[l] += 1;
            }
            let mut arg = 0;
            for (i, &cnt) in counts.iter().enumerate() {
                if cnt > counts[arg] {
                    arg = i;
                }
            }
            centers[arg].map(|v| v as i64)
        };
        [bgr[2], bgr[1], bgr[0]]
    };
    format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2])
}

// ---------------------------------------------------------------------------------------------
// main()

/// The script's color image as far as it is needed: scaled and cropped already, or decoded
/// (or failed to) with only the dominant-color region still to be resized.
enum Color {
    Cropped(Img),
    Decoded(Option<cv::Bgr>),
}

fn run(a: &BusyArgs, log: &mut String) -> Result<Option<Busy>, ()> {
    let bytes = std::fs::read(&a.image).map_err(|_| ())?;
    let (g, color) = if cv::is_png(&bytes) {
        // One streamed decode feeds both images; only the 4-channel crop is ever whole.
        let mut c = scale_crop(
            png_gray_bgr(&bytes).ok_or(())?,
            a,
            a.verbose.then_some(&mut *log),
        )?;
        let n = c.w * c.h;
        let g = c.px.iter().step_by(4).copied().collect();
        for i in 0..n {
            c.px.copy_within(4 * i + 1..4 * i + 4, 3 * i);
        }
        c.px.truncate(3 * n);
        c.px.shrink_to_fit();
        let (w, h) = (c.w, c.h);
        (Img { w, h, px: g }, Color::Cropped(c))
    } else {
        // The color image is decoded alongside the grey pipeline, as the script's two are
        // independent; only its dominant-color region is resized later.
        let mut glog = String::new();
        let (g, c) = rayon::join(
            || {
                gray_rows(&bytes)
                    .ok_or(())
                    .and_then(|r| scale_crop(r, a, a.verbose.then_some(&mut glog)))
            },
            || cv::imdecode_bgr(&bytes),
        );
        *log += &glog;
        (g?, Color::Decoded(c))
    };
    if a.verbose {
        *log += &format!("Cropped image to {}x{}\n", a.screen_width, a.screen_height);
    }
    let color = |x: i64, y: i64, w: i64, h: i64| -> Result<String, ()> {
        let px = match &color {
            Color::Cropped(c) => {
                let (xs, xe, ys, ye) = color_bounds(c.w, c.h, x, y, w, h);
                let mut px = Vec::with_capacity((xe - xs) * (ye - ys) * 3);
                for yy in ys..ye {
                    px.extend_from_slice(&c.px[(yy * c.w + xs) * 3..(yy * c.w + xe) * 3]);
                }
                px
            }
            Color::Decoded(c) => {
                let c = c.as_ref().ok_or(())?;
                let Geom { nw, nh, win } = scale_geom(c.w, c.h, a, None)?;
                let (xs, xe, ys, ye) = color_bounds(win.x1 - win.x0, win.y1 - win.y0, x, y, w, h);
                let region = Window {
                    x0: win.x0 + xs,
                    x1: win.x0 + xe,
                    y0: win.y0 + ys,
                    y1: win.y0 + ye,
                };
                let (line, mut y) = (c.w * 3, 0);
                let mut next = |row: &mut [u8]| {
                    row.copy_from_slice(&c.px[y * line..(y + 1) * line]);
                    y += 1;
                    true
                };
                resize_lanczos4_rows(c.w, c.h, 3, nw, nh, region, &mut next).ok_or(())?
            }
        };
        Ok(dominant_color(px.as_chunks::<3>().0))
    };
    if a.largest_region {
        let Some((center, size, var)) = find_largest(&g, a)? else {
            return Ok(None);
        };
        let x1 = center.0 - floordiv(size.0, 2);
        let y1 = center.1 - floordiv(size.1, 2);
        let dominant_color = color(x1, y1, size.0, size.1)?;
        return Ok(Some(Busy {
            center_x: center.0,
            center_y: center.1,
            width: size.0,
            height: size.1,
            variance: var,
            dominant_color,
        }));
    }
    let ((x, y), variance) = find_least_busy(&g, a, log)?;
    Ok(Some(Busy {
        center_x: x + floordiv(a.width, 2),
        center_y: y + floordiv(a.height, 2),
        width: a.width,
        height: a.height,
        variance,
        dominant_color: color(x, y, a.width, a.height)?,
    }))
}

/// Python `int(str)`.
fn py_parse_int(s: &str) -> Option<i64> {
    let t = s.trim();
    let (neg, d) = match t.as_bytes().first()? {
        b'-' => (true, &t[1..]),
        b'+' => (false, &t[1..]),
        _ => (false, t),
    };
    if d.is_empty() || d.starts_with('_') || d.ends_with('_') || d.contains("__") {
        return None;
    }
    let d: String = d.chars().filter(|&c| c != '_').collect();
    if !d.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let v: i64 = d.parse().ok()?;
    Some(if neg { -v } else { v })
}

/// Python `float(str)`.
fn py_parse_float(s: &str) -> Option<f64> {
    let t = s.trim();
    if t.contains("__") || t.starts_with('_') || t.ends_with('_') {
        return None;
    }
    let t: String = t.chars().filter(|&c| c != '_').collect();
    let l = t.to_ascii_lowercase();
    let body = l.trim_start_matches(['+', '-']);
    if body.starts_with("in") || body.starts_with("na") {
        return match body {
            "inf" | "infinity" | "nan" => t.parse().ok(),
            _ => None,
        };
    }
    t.parse().ok()
}

/// argparse's `_negative_number_matcher` up to Python 3.13: `^-\d+$|^-\d*\.\d+$`. Python 3.14
/// takes anything that starts as `-\.?\d` (`-1e3` too); the shell passes no such argument.
fn looks_negative(s: &str) -> bool {
    let Some(r) = s.strip_prefix('-') else {
        return false;
    };
    let digits = |x: &str| x.bytes().all(|c| c.is_ascii_digit());
    match r.split_once('.') {
        None => !r.is_empty() && digits(r),
        Some((a, b)) => digits(a) && !b.is_empty() && digits(b),
    }
}

const OPTS: &[(&str, bool)] = &[
    ("--width", true),
    ("--height", true),
    ("-v", false),
    ("--visual-output", false),
    ("--screen-width", true),
    ("--screen-height", true),
    ("--stride", true),
    ("--screen-mode", true),
    ("--verbose", false),
    ("-l", false),
    ("--largest-region", false),
    ("-t", true),
    ("--variance-threshold", true),
    ("--aspect-ratio", true),
    ("--horizontal-padding", true),
    ("-hp", true),
    ("--vertical-padding", true),
    ("-vp", true),
    ("--busiest", false),
    ("-h", false),
    ("--help", false),
];

/// The script's argparse parser (`Err(exit code)`). Supported: exact options, unique `--`
/// prefixes, `--opt=value`, `--`. `--help` text and `-v` (writes output.png) are not reproduced.
fn parse_args(args: &[String]) -> Result<BusyArgs, i32> {
    let mut a = BusyArgs::default();
    let mut image = None;
    let mut i = 0;
    let mut only_pos = false;
    while i < args.len() {
        let arg = &args[i];
        i += 1;
        if only_pos || !arg.starts_with('-') || arg == "-" || looks_negative(arg) {
            if image.replace(arg.clone()).is_some() {
                return Err(2); // unrecognized arguments
            }
            continue;
        }
        if arg == "--" {
            only_pos = true;
            continue;
        }
        let (name, inline) = match arg.split_once('=') {
            Some((n, v)) if n.starts_with("--") => (n, Some(v.to_string())),
            _ => (arg.as_str(), None),
        };
        let opt = match OPTS.iter().find(|(o, _)| *o == name) {
            Some(o) => *o,
            None if name.starts_with("--") => {
                let m: Vec<_> = OPTS
                    .iter()
                    .filter(|(o, _)| o.starts_with("--") && o.starts_with(name))
                    .collect();
                if m.len() != 1 {
                    return Err(2);
                }
                *m[0]
            }
            None => return Err(2),
        };
        let val = if opt.1 {
            match inline {
                Some(v) => v,
                None => {
                    let v = args.get(i).ok_or(2)?;
                    if v.starts_with('-') && !looks_negative(v) {
                        return Err(2);
                    }
                    i += 1;
                    v.clone()
                }
            }
        } else {
            if inline.is_some() {
                return Err(2);
            }
            String::new()
        };
        let int = || py_parse_int(&val).ok_or(2);
        match opt.0 {
            "--width" => a.width = int()?,
            "--height" => a.height = int()?,
            "--screen-width" => a.screen_width = int()?,
            "--screen-height" => a.screen_height = int()?,
            "--stride" => a.stride = int()?,
            "--horizontal-padding" | "-hp" => a.horizontal_padding = int()?,
            "--vertical-padding" | "-vp" => a.vertical_padding = int()?,
            "--screen-mode" => {
                a.fit = match val.as_str() {
                    "fill" => false,
                    "fit" => true,
                    _ => return Err(2),
                }
            }
            "-t" | "--variance-threshold" => {
                a.variance_threshold = py_parse_float(&val).ok_or(2)?
            }
            "--aspect-ratio" => a.aspect_ratio = py_parse_float(&val).ok_or(2)?,
            "--verbose" => a.verbose = true,
            "-l" | "--largest-region" => a.largest_region = true,
            "--busiest" => a.busiest = true,
            "-h" | "--help" => return Err(0),
            _ => {} // -v / --visual-output
        }
    }
    a.image = image.ok_or(2)?;
    Ok(a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_repr_matches_python() {
        for (v, s) in [
            (0.0, "0.0"),
            (-0.0, "-0.0"),
            (1.0, "1.0"),
            (1e16, "1e+16"),
            (1e15, "1000000000000000.0"),
            (123456789012345680.0, "1.2345678901234568e+17"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (1.5e-7, "1.5e-07"),
            (-1.1368683772161603e-13, "-1.1368683772161603e-13"),
            (1234.5678, "1234.5678"),
            (0.1 + 0.2, "0.30000000000000004"),
            (5e-324, "5e-324"),
            (1.7976931348623157e308, "1.7976931348623157e+308"),
            (2.5e100, "2.5e+100"),
        ] {
            assert_eq!(py_float_repr(v), s, "{v:e}");
        }
        assert_eq!(json_float(f64::NAN), "NaN");
        assert_eq!(json_float(f64::NEG_INFINITY), "-Infinity");
    }

    #[test]
    fn rgb_to_gray_coefficients() {
        assert_eq!((RC, GC, BC), (9797, 19234, 3737));
    }

    #[test]
    fn python_int_parsing() {
        assert_eq!(py_parse_int(" 12 "), Some(12));
        assert_eq!(py_parse_int("1_000"), Some(1000));
        assert_eq!(py_parse_int("+7"), Some(7));
        assert_eq!(py_parse_int("1.0"), None);
        assert_eq!(py_parse_int("_1"), None);
    }
}
