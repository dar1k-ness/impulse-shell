//! Port of `magick SRC -resize NxN OUT.png` from `generate-thumbnails-magick.sh`, matching
//! ImageMagick 7.1.2 Q16-HDRI bit for bit: its decoders' pixel values, the two-pass separable
//! resize of MagickCore/resize.c (Quantum = f32, f64 accumulation) and the PNG writer's export.

use rayon::prelude::*;
use std::path::Path;

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Result<T> = std::result::Result<T, Error>;

const QUANTUM_RANGE: f64 = 65535.0;
const QUANTUM_SCALE: f64 = 1.0 / 65535.0;
const EPSILON: f64 = 1.0e-12;

fn safe_reciprocal(x: f64) -> f64 {
    if x > -EPSILON && x < EPSILON {
        1.0 / EPSILON
    } else {
        1.0 / x
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Space {
    Gray,
    Rgb,
    Cmyk,
}

/// An IM image: interleaved f32 quantums (0..65535), colour channels then alpha.
struct Image {
    w: usize,
    h: usize,
    space: Space,
    alpha: bool,
    /// PseudoClass (palette, 8-bit grey PNG, grey JPEG): forces the Mitchell filter.
    pseudo: bool,
    depth16: bool,
    px: Vec<f32>,
}

impl Image {
    fn colors(&self) -> usize {
        match self.space {
            Space::Gray => 1,
            Space::Rgb => 3,
            Space::Cmyk => 4,
        }
    }

    fn channels(&self) -> usize {
        self.colors() + self.alpha as usize
    }
}

fn char_to_quantum(v: u8) -> f32 {
    257.0 * v as f32
}

/// Rows of f32 quantums (`w * channels` each), top to bottom; `None` checks the rest of the
/// stream once every row has been read.
type RowFn<'a> = Box<dyn FnMut(Option<&mut [f32]>) -> Result<()> + Send + 'a>;

/// Rows converted on the fly from a whole decoded buffer of `line` samples per row.
fn mem_rows<'a, T: Copy + Send + 'a>(
    px: Vec<T>,
    line: usize,
    conv: impl Fn(T) -> f32 + Send + 'a,
) -> RowFn<'a> {
    let mut y = 0;
    Box::new(move |out| {
        if let Some(out) = out {
            for (o, &v) in out.iter_mut().zip(&px[y * line..(y + 1) * line]) {
                *o = conv(v);
            }
            y += 1;
        }
        Ok(())
    })
}

/// The image's metadata (with no pixels) and its rows: the whole image never exists as f32.
fn decode(data: &[u8]) -> Result<(Image, RowFn<'_>)> {
    if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        decode_jpeg(data)
    } else if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        decode_png(data)
    } else if heif::is_heif(data) {
        decode_heif(data)
    } else {
        decode_other(data)
    }
}

/// IM reads JPEGs through libjpeg(-turbo) with its defaults (islow DCT, fancy upsampling), as
/// TurboJPEG does; EXIF orientation is ignored. CMYK samples are inverted, grey becomes a palette.
fn decode_jpeg(data: &[u8]) -> Result<(Image, RowFn<'_>)> {
    use turbojpeg::{Colorspace, PixelFormat};
    let mut d = turbojpeg::Decompressor::new()?;
    let hdr = d.read_header(data)?;
    crate::limit::check_size(hdr.width, hdr.height, 4)?;
    let (format, space) = match hdr.colorspace {
        Colorspace::Gray => (PixelFormat::GRAY, Space::Gray),
        Colorspace::CMYK | Colorspace::YCCK => (PixelFormat::CMYK, Space::Cmyk),
        _ => (PixelFormat::RGB, Space::Rgb),
    };
    let pitch = hdr.width * format.size();
    let mut buf = vec![0u8; pitch * hdr.height];
    d.decompress(
        data,
        turbojpeg::Image {
            pixels: &mut buf[..],
            width: hdr.width,
            pitch,
            height: hdr.height,
            format,
        },
    )?;
    let cmyk = space == Space::Cmyk;
    let rows = mem_rows(buf, pitch, move |v| {
        if cmyk {
            (QUANTUM_RANGE - char_to_quantum(v) as f64) as f32
        } else {
            char_to_quantum(v)
        }
    });
    let img = Image {
        w: hdr.width,
        h: hdr.height,
        space,
        alpha: false,
        pseudo: space == Space::Gray,
        depth16: false,
        px: Vec::new(),
    };
    Ok((img, rows))
}

/// Non-interlaced PNGs are streamed row by row; Adam7 ones are decoded whole as bytes.
fn decode_png(data: &[u8]) -> Result<(Image, RowFn<'_>)> {
    use png::{BitDepth, ColorType};
    let mut dec = png::Decoder::new(std::io::Cursor::new(data));
    dec.set_transformations(png::Transformations::EXPAND);
    let mut r = dec.read_info()?;
    let (src_color, trns) = {
        let i = r.info();
        (i.color_type, i.trns.is_some())
    };
    let frame = crate::limit::png_buffer(&r)?;
    let (color, depth) = r.output_color_type();
    let (w, h) = r.info().size();
    let depth16 = depth == BitDepth::Sixteen;
    let conv = move |row: &[u8], out: &mut [f32]| {
        if depth16 {
            for (o, b) in out.iter_mut().zip(row.as_chunks::<2>().0) {
                *o = u16::from_be_bytes(*b) as f32;
            }
        } else {
            for (o, &v) in out.iter_mut().zip(row) {
                *o = char_to_quantum(v);
            }
        }
    };
    let rows: RowFn<'_> = if r.info().interlaced {
        let mut buf = vec![0u8; frame];
        let line = r.next_frame(&mut buf)?.line_size;
        let mut y = 0;
        Box::new(move |out| {
            if let Some(out) = out {
                conv(&buf[y * line..(y + 1) * line], out);
                y += 1;
            }
            Ok(())
        })
    } else {
        let mut raw = vec![0u8; r.output_line_size(w).ok_or("PNG row too long")?];
        Box::new(move |out| match out {
            Some(out) => {
                r.read_row(&mut raw)?.ok_or("PNG ended early")?;
                conv(&raw, out);
                Ok(())
            }
            // Finishes the IDAT stream, as `next_frame` does.
            None => match r.read_row(&mut raw)? {
                None => Ok(()),
                Some(_) => Err("PNG has extra rows".into()),
            },
        })
    };
    let gray = matches!(color, ColorType::Grayscale | ColorType::GrayscaleAlpha);
    let pseudo =
        src_color == ColorType::Indexed || (src_color == ColorType::Grayscale && !depth16 && !trns);
    let img = Image {
        w: w as usize,
        h: h as usize,
        space: if gray { Space::Gray } else { Space::Rgb },
        alpha: matches!(color, ColorType::GrayscaleAlpha | ColorType::Rgba),
        pseudo,
        depth16,
        px: Vec::new(),
    };
    Ok((img, rows))
}

/// IM's heic coder (ReadHEICImageHandle): libheif's interleaved RGB(A) with the transformations
/// applied, at 8 bits or (luma deeper than 8) 16-bit little-endian samples shifted to the top.
fn decode_heif(data: &[u8]) -> Result<(Image, RowFn<'_>)> {
    let d = crate::limit::heif(data, true)?;
    let line = d.width * d.channels();
    let img = Image {
        w: d.width,
        h: d.height,
        space: Space::Rgb,
        alpha: d.alpha,
        pseudo: false,
        depth16: d.bits > 8,
        px: Vec::new(),
    };
    let rows = match d.samples {
        heif::Samples::U8(v) => mem_rows(v, line, char_to_quantum),
        heif::Samples::U16(v) => {
            let shift = 16 - d.bits.min(16);
            mem_rows(v, line, move |v| (v << shift) as f32)
        }
    };
    Ok((img, rows))
}

fn decode_other(data: &[u8]) -> Result<(Image, RowFn<'_>)> {
    let img = crate::limit::image(data)?;
    let c = img.color();
    let (alpha, gray) = (c.has_alpha(), !c.has_color());
    let depth16 = c.bytes_per_pixel() / c.channel_count() > 1;
    let (w, h) = (img.width() as usize, img.height() as usize);
    let line = w * ((if gray { 1 } else { 3 }) + alpha as usize);
    let q = char_to_quantum;
    let rows = match (depth16, gray, alpha) {
        (false, true, false) => mem_rows(img.into_luma8().into_raw(), line, q),
        (false, true, true) => mem_rows(img.into_luma_alpha8().into_raw(), line, q),
        (false, false, false) => mem_rows(img.into_rgb8().into_raw(), line, q),
        (false, false, true) => mem_rows(img.into_rgba8().into_raw(), line, q),
        (true, true, false) => mem_rows(img.into_luma16().into_raw(), line, f32::from),
        (true, true, true) => mem_rows(img.into_luma_alpha16().into_raw(), line, f32::from),
        (true, false, false) => mem_rows(img.into_rgb16().into_raw(), line, f32::from),
        (true, false, true) => mem_rows(img.into_rgba16().into_raw(), line, f32::from),
    };
    let space = if gray { Space::Gray } else { Space::Rgb };
    let img = Image {
        w,
        h,
        space,
        alpha,
        pseudo: false,
        depth16,
        px: Vec::new(),
    };
    Ok((img, rows))
}

/// ParseMetaGeometry for `NxN`: fit inside the box keeping aspect, each side at least 1.
pub fn fit(w: usize, h: usize, size: u32) -> (usize, usize) {
    let n = size as f64;
    let mut s = n / w as f64;
    if s > n / h as f64 {
        s = n / h as f64;
    }
    let side = |d: usize| (s * d as f64 + 0.5).floor().max(1.0) as usize;
    (side(w), side(h))
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Lanczos,
    Mitchell,
}

struct Filter {
    kind: Kind,
    support: f64,
    window_scale: f64,
    coef: [f64; 7],
}

impl Filter {
    fn new(kind: Kind) -> Filter {
        let support = if kind == Kind::Lanczos { 3.0 } else { 2.0 };
        let (b, c) = (1.0 / 3.0, 1.0 / 3.0);
        let two_b = b + b;
        Filter {
            kind,
            support,
            window_scale: safe_reciprocal(support),
            coef: [
                1.0 - (1.0 / 3.0) * b,
                -3.0 + two_b + c,
                2.0 - 1.5 * b - c,
                (4.0 / 3.0) * b + 4.0 * c,
                -8.0 * c - two_b,
                b + 5.0 * c,
                (-1.0 / 6.0) * b - c,
            ],
        }
    }

    /// GetResizeFilterWeight (blur 1): Lanczos = SincFast windowed by SincFast, Mitchell = CubicBC.
    fn weight(&self, x: f64) -> f64 {
        let x = x.abs() * safe_reciprocal(1.0);
        match self.kind {
            Kind::Lanczos => sinc_fast(x * self.window_scale) * sinc_fast(x),
            Kind::Mitchell => 1.0 * self.cubic_bc(x),
        }
    }

    fn cubic_bc(&self, x: f64) -> f64 {
        let c = &self.coef;
        if x < 1.0 {
            return c[0] + x * (x * (c[1] + x * c[2]));
        }
        if x < 2.0 {
            return c[3] + x * (c[4] + x * (c[5] + x * c[6]));
        }
        0.0
    }
}

/// SincFast with the MAGICKCORE_QUANTUM_DEPTH == 16 Robidoux/Racette coefficients.
fn sinc_fast(x: f64) -> f64 {
    if x > 4.0 {
        let a = std::f64::consts::PI * x;
        return a.sin() / a;
    }
    // The C source's long-double literals rounded to double (checked bit for bit with gcc).
    const C: [u64; 10] = [
        0x3f5c71c712204843,
        0xbf392e7e30a21c51,
        0x3f04aae0f2f31f28,
        0xbec50d655760d6d4,
        0x3e7e09d9614d342a,
        0xbe300406665c46fb,
        0x3dda532c7dacb935,
        0xbd80778ff9cfd451,
        0x3d1c8b574f3d493c,
        0xbca98546b213e547,
    ];
    let c = C.map(f64::from_bits);
    let xx = x * x;
    let p = c[0]
        + xx * (c[1]
            + xx * (c[2]
                + xx * (c[3]
                    + xx * (c[4]
                        + xx * (c[5] + xx * (c[6] + xx * (c[7] + xx * (c[8] + xx * c[9]))))))));
    (xx - 1.0) * (xx - 4.0) * (xx - 9.0) * (xx - 16.0) * p
}

struct Contribution {
    start: usize,
    weights: Vec<f64>,
}

/// The per-destination contribution lists of HorizontalFilter / VerticalFilter.
fn contributions(f: &Filter, factor: f64, src_len: usize, dst_len: usize) -> Vec<Contribution> {
    let mut scale = (1.0 / factor + EPSILON).max(1.0);
    let mut support = scale * f.support;
    if support < 0.5 {
        support = 0.5;
        scale = 1.0;
    }
    scale = safe_reciprocal(scale);
    (0..dst_len)
        .map(|d| {
            let bisect = (d as f64 + 0.5) / factor + EPSILON;
            let start = (bisect - support + 0.5).max(0.0) as isize;
            let stop = (bisect + support + 0.5).min(src_len as f64) as isize;
            let mut weights: Vec<f64> = (start..stop)
                .map(|s| f.weight(scale * (s as f64 - bisect + 0.5)))
                .collect();
            let density: f64 = weights.iter().fold(0.0, |a, &w| a + w);
            if density != 0.0 && density != 1.0 {
                let r = safe_reciprocal(density);
                weights.iter_mut().for_each(|w| *w *= r);
            }
            Contribution {
                start: start.max(0) as usize,
                weights,
            }
        })
        .collect()
}

/// One output pixel of a filter pass: `px(s)` is the source pixel at sample `s` along the axis.
fn filter_px<'s>(
    alpha: Option<usize>,
    c: &Contribution,
    px: impl Fn(usize) -> &'s [f32],
    out: &mut [f32],
) {
    for (i, o) in out.iter_mut().enumerate() {
        let mut pixel = 0.0f64;
        match alpha {
            Some(a) if a != i => {
                let mut gamma = 0.0f64;
                for (j, &w) in c.weights.iter().enumerate() {
                    let p = px(c.start + j);
                    let al = w * QUANTUM_SCALE * p[a] as f64;
                    pixel += al * p[i] as f64;
                    gamma += al;
                }
                *o = (safe_reciprocal(gamma) * pixel) as f32;
            }
            _ => {
                for (j, &w) in c.weights.iter().enumerate() {
                    pixel += w * px(c.start + j)[i] as f64;
                }
                *o = pixel as f32;
            }
        }
    }
}

/// One filter pass along x (`horizontal`) or y; returns the new pixel buffer.
fn pass(img: &Image, f: &Filter, factor: f64, dst_len: usize, horizontal: bool) -> Vec<f32> {
    let nch = img.channels();
    let alpha = img.alpha.then(|| img.colors());
    let (sw, sh) = (img.w, img.h);
    let src_len = if horizontal { sw } else { sh };
    let contribs = contributions(f, factor, src_len, dst_len);
    let src = &img.px;
    if horizontal {
        let mut dst = vec![0f32; dst_len * sh * nch];
        dst.par_chunks_mut(dst_len * nch)
            .zip(src.par_chunks(sw * nch))
            .for_each(|(out, row)| hpass_row(alpha, nch, &contribs, row, out));
        dst
    } else {
        let mut dst = vec![0f32; sw * dst_len * nch];
        dst.par_chunks_mut(sw * nch)
            .zip(contribs.par_iter())
            .for_each(|(row, c)| {
                for (x, out) in row.chunks_exact_mut(nch).enumerate() {
                    filter_px(
                        alpha,
                        c,
                        |y| &src[(y * sw + x) * nch..(y * sw + x + 1) * nch],
                        out,
                    );
                }
            });
        dst
    }
}

fn hpass_row(
    alpha: Option<usize>,
    nch: usize,
    contribs: &[Contribution],
    row: &[f32],
    out: &mut [f32],
) {
    for (c, o) in contribs.iter().zip(out.chunks_exact_mut(nch)) {
        filter_px(alpha, c, |x| &row[x * nch..(x + 1) * nch], o);
    }
}

/// Appends `n` rows of `line` samples to `buf`.
fn read_rows(next: &mut RowFn<'_>, buf: &mut Vec<f32>, n: usize, line: usize) -> Result<()> {
    let s = buf.len();
    buf.resize(s + n * line, 0.0);
    for r in buf[s..].chunks_exact_mut(line) {
        next(Some(r))?;
    }
    Ok(())
}

/// Source rows decoded per batch of the streamed horizontal pass.
const HROWS: usize = 32;
/// Destination rows per batch of the streamed vertical pass.
const VROWS: usize = 4;

/// ResizeImage with the default filter (`filter` undefined). The first pass consumes the
/// decoder's rows as they come (a window of them for a vertical pass), so only the
/// intermediate image, already resized along one axis, is ever held.
fn resize(mut img: Image, mut next: RowFn<'_>, cols: usize, rows: usize) -> Result<Image> {
    let nch = img.channels();
    let line = img.w * nch;
    if cols == img.w && rows == img.h {
        let mut px = Vec::new();
        read_rows(&mut next, &mut px, img.h, line)?;
        next(None)?;
        img.px = px;
        return Ok(img);
    }
    let xf = cols as f64 * safe_reciprocal(img.w as f64);
    let yf = rows as f64 * safe_reciprocal(img.h as f64);
    let kind = if img.pseudo || img.alpha || xf * yf > 1.0 {
        Kind::Mitchell
    } else {
        Kind::Lanczos
    };
    let f = Filter::new(kind);
    let alpha = img.alpha.then(|| img.colors());
    if xf > yf {
        let contribs = contributions(&f, xf, img.w, cols);
        let mut mid = vec![0f32; cols * img.h * nch];
        let mut raw = Vec::new();
        for out in mid.chunks_mut(HROWS * cols * nch) {
            raw.clear();
            read_rows(&mut next, &mut raw, out.len() / (cols * nch), line)?;
            out.par_chunks_mut(cols * nch)
                .zip(raw.par_chunks(line))
                .for_each(|(o, row)| hpass_row(alpha, nch, &contribs, row, o));
        }
        next(None)?;
        img.px = mid;
        img.w = cols;
        img.px = pass(&img, &f, yf, rows, false);
        img.h = rows;
    } else {
        let contribs = contributions(&f, yf, img.h, rows);
        let mut mid = vec![0f32; line * rows];
        // Source rows `lo..lo + win.len() / line`; the next row to decode is always the one after.
        let (mut win, mut lo) = (Vec::<f32>::new(), 0usize);
        let mut skip = vec![0f32; line];
        for (out, cs) in mid.chunks_mut(VROWS * line).zip(contribs.chunks(VROWS)) {
            let start = cs[0].start;
            let stop = cs
                .iter()
                .map(|c| c.start + c.weights.len())
                .max()
                .unwrap_or(start);
            let drop = start.saturating_sub(lo).min(win.len() / line);
            win.drain(..drop * line);
            lo += drop;
            if win.is_empty() {
                while lo < start {
                    next(Some(&mut skip))?;
                    lo += 1;
                }
            }
            let have = lo + win.len() / line;
            if stop > have {
                read_rows(&mut next, &mut win, stop - have, line)?;
            }
            let (win, lo) = (&win, lo);
            out.par_chunks_mut(line)
                .zip(cs.par_iter())
                .for_each(|(o, c)| {
                    o.par_chunks_mut(64 * nch).enumerate().for_each(|(xc, o)| {
                        for (k, px) in o.chunks_exact_mut(nch).enumerate() {
                            let x = xc * 64 + k;
                            filter_px(
                                alpha,
                                c,
                                |y| {
                                    let at = (y - lo) * line + x * nch;
                                    &win[at..at + nch]
                                },
                                px,
                            );
                        }
                    });
                });
        }
        for _ in lo + win.len() / line..img.h {
            next(Some(&mut skip))?;
        }
        next(None)?;
        img.px = mid;
        img.h = rows;
        img.px = pass(&img, &f, xf, cols, true);
        img.w = cols;
    }
    img.pseudo = false;
    Ok(img)
}

fn quantum_to_char(q: f32) -> u8 {
    if q.is_nan() || q <= 0.0 {
        return 0;
    }
    let s = q / 257.0f32;
    if s >= 255.0 { 255 } else { (s + 0.5) as u8 }
}

fn quantum_to_short(q: f32) -> u16 {
    if q.is_nan() || q <= 0.0 {
        0
    } else if q >= 65535.0 {
        65535
    } else {
        (q + 0.5) as u16
    }
}

/// The PNG writer's TransformImageColorspace(sRGB) for CMYK.
fn cmyk_to_rgb(img: &mut Image) {
    let px: Vec<f32> = img
        .px
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| {
            let k = p[3] as f64;
            let conv = |c: f32| {
                (QUANTUM_RANGE - (QUANTUM_SCALE * c as f64 * (QUANTUM_RANGE - k) + k)) as f32
            };
            [conv(p[0]), conv(p[1]), conv(p[2])]
        })
        .collect();
    img.px = px;
    img.space = Space::Rgb;
}

fn encode(mut img: Image, out: &Path, text: &[(&str, &str)]) -> Result<()> {
    if img.space == Space::Cmyk {
        cmyk_to_rgb(&mut img);
    }
    let (nch, colors) = (img.channels(), img.colors());
    let mut depth16 = img.depth16;
    if depth16 {
        // LosslessReduceDepthOK: only the colour channels (and the white background) are checked.
        depth16 = !img.px.chunks_exact(nch).all(|p| {
            p[..colors]
                .iter()
                .all(|&q| char_to_quantum(quantum_to_char(q)) as f64 == q as f64)
        });
    }
    let gray_alpha = img.space == Space::Gray && img.alpha;
    let sample = |p: &[f32], i: usize| -> f32 {
        if gray_alpha && i == 0 {
            let g = p[0] as f64;
            (0.212656 * g + 0.715158 * g + 0.072186 * g) as f32
        } else {
            p[i]
        }
    };
    let mut data = Vec::with_capacity(img.px.len() * if depth16 { 2 } else { 1 });
    for p in img.px.chunks_exact(nch) {
        for i in 0..nch {
            let q = sample(p, i);
            if depth16 {
                data.extend_from_slice(&quantum_to_short(q).to_be_bytes());
            } else {
                data.push(quantum_to_char(q));
            }
        }
    }
    let file = std::io::BufWriter::new(std::fs::File::create(out)?);
    let mut enc = png::Encoder::new(file, img.w as u32, img.h as u32);
    enc.set_color(match (img.space, img.alpha) {
        (Space::Gray, false) => png::ColorType::Grayscale,
        (Space::Gray, true) => png::ColorType::GrayscaleAlpha,
        (_, false) => png::ColorType::Rgb,
        (_, true) => png::ColorType::Rgba,
    });
    enc.set_depth(if depth16 {
        png::BitDepth::Sixteen
    } else {
        png::BitDepth::Eight
    });
    enc.set_compression(png::Compression::Fast);
    for (k, v) in text {
        enc.add_text_chunk(k.to_string(), v.to_string())?;
    }
    let mut w = enc.write_header()?;
    w.write_image_data(&data)?;
    w.finish()?;
    Ok(())
}

/// `magick SRC -resize SIZExSIZE OUT` with OUT a PNG.
pub fn resize_to_png(src: &Path, size: u32, out: &Path) -> Result<()> {
    resize_to_png_text(src, size, out, &[])
}

/// [`resize_to_png`] with extra `tEXt` chunks (Freedesktop `Thumb::URI`, `Thumb::MTime`).
pub fn resize_to_png_text(src: &Path, size: u32, out: &Path, text: &[(&str, &str)]) -> Result<()> {
    crate::pool::scoped(|| {
        let data = std::fs::read(src)?;
        let (img, rows_of) = decode(&data)?;
        let (cols, rows) = fit(img.w, img.h, size);
        encode(resize(img, rows_of, cols, rows)?, out, text)
    })
}

/// The `tEXt` value for `key` among the chunks before a PNG's image data.
pub fn png_text(path: &Path, key: &str) -> Option<String> {
    let f = std::io::BufReader::new(std::fs::File::open(path).ok()?);
    let r = png::Decoder::new(f).read_info().ok()?;
    r.info()
        .uncompressed_latin1_text
        .iter()
        .find(|c| c.keyword == key)
        .map(|c| c.text.clone())
}
