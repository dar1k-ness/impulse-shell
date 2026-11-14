//! The parts of OpenCV the scripts rely on, ported for bit-identical results.

/// An 8-bit 3-channel image in OpenCV's BGR order, row-major, no padding.
#[derive(Clone, Debug, PartialEq)]
pub struct Bgr {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
}

impl Bgr {
    pub fn at(&self, x: usize, y: usize) -> [u8; 3] {
        let i = (y * self.w + x) * 3;
        [self.px[i], self.px[i + 1], self.px[i + 2]]
    }
}

/// An 8-bit single-channel image.
#[derive(Clone, Debug, PartialEq)]
pub struct Gray {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
}

/// `cv::RNG` (multiply-with-carry), default-seeded like `cv::theRNG()` in a fresh process.
#[derive(Clone, Debug)]
pub struct Rng(pub u64);

impl Default for Rng {
    fn default() -> Self {
        Rng(0xffff_ffff)
    }
}

impl Rng {
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> u32 {
        self.0 = (self.0 as u32 as u64)
            .wrapping_mul(4_164_903_690)
            .wrapping_add(self.0 >> 32);
        self.0 as u32
    }
}

fn is_jpeg(b: &[u8]) -> bool {
    b.len() > 3 && b[0] == 0xff && b[1] == 0xd8 && b[2] == 0xff
}

/// Orientation tag of a TIFF-structured EXIF block (IFD0), as OpenCV's `ExifReader` reads it.
fn exif_orientation(t: &[u8]) -> u16 {
    tiff_tag(t, 0x0112).unwrap_or(1)
}

/// The first 16-bit value of entry `tag` in IFD0 of the TIFF structure `t`.
fn tiff_tag(t: &[u8], tag: u16) -> Option<u16> {
    if t.len() < 8 {
        return None;
    }
    let le = &t[..2] == b"II";
    let u16_at = |o: usize| -> u16 {
        t.get(o..o + 2).map_or(0, |s| {
            if le {
                u16::from_le_bytes([s[0], s[1]])
            } else {
                u16::from_be_bytes([s[0], s[1]])
            }
        })
    };
    let u32_at = |o: usize| -> u32 {
        t.get(o..o + 4).map_or(0, |s| {
            let a = [s[0], s[1], s[2], s[3]];
            if le {
                u32::from_le_bytes(a)
            } else {
                u32::from_be_bytes(a)
            }
        })
    };
    let ifd = u32_at(4) as usize;
    for k in 0..u16_at(ifd) as usize {
        let e = ifd + 2 + k * 12;
        if u16_at(e) == tag {
            return Some(u16_at(e + 8));
        }
    }
    None
}

/// EXIF orientation (1..=8) of a JPEG, as `cv::imread` applies it.
pub fn jpeg_orientation(b: &[u8]) -> u16 {
    let mut i = 2;
    while i + 4 <= b.len() && b[i] == 0xff {
        let m = b[i + 1];
        let len = u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize;
        if m == 0xda || len < 2 {
            break;
        }
        let seg = &b[(i + 4).min(b.len())..(i + 2 + len).min(b.len())];
        if m == 0xe1 && seg.len() > 14 && &seg[..6] == b"Exif\0\0" {
            return exif_orientation(&seg[6..]);
        }
        i += 2 + len;
    }
    1
}

pub(crate) fn is_png(b: &[u8]) -> bool {
    b.starts_with(b"\x89PNG\r\n\x1a\n")
}

/// Calls `f(tag, data)` for each PNG chunk in file order until IEND or truncation.
pub(crate) fn png_chunks<'a>(b: &'a [u8], mut f: impl FnMut(&'a [u8], &'a [u8])) {
    let mut i = 8;
    while i + 8 <= b.len() {
        let len = u32::from_be_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]) as usize;
        let tag = &b[i + 4..i + 8];
        let Some(data) = b.get(i + 8..i + 8 + len) else {
            break;
        };
        f(tag, data);
        if tag == b"IEND" {
            break;
        }
        i += 12 + len;
    }
}

/// EXIF orientation from a PNG's first `eXIf` chunk (OpenCV's PNG decoder reads it too).
pub(crate) fn png_orientation(b: &[u8]) -> u16 {
    let mut exif = None;
    png_chunks(b, |tag, data| {
        if tag == b"eXIf" && exif.is_none() {
            exif = Some(data);
        }
    });
    exif.map_or(1, exif_orientation)
}

pub fn is_tiff(b: &[u8]) -> bool {
    b.starts_with(b"II*\0") || b.starts_with(b"MM\0*")
}

fn is_webp(b: &[u8]) -> bool {
    b.len() >= 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP"
}

/// EXIF orientation from a WebP's first `EXIF` chunk (OpenCV's WebP decoder hands it to the
/// same `ExifReader`).
fn webp_orientation(b: &[u8]) -> u16 {
    let mut i = 12;
    while let Some(head) = b.get(i..i + 8) {
        let len = u32::from_le_bytes([head[4], head[5], head[6], head[7]]) as usize;
        let Some(data) = b.get(i + 8..i + 8 + len) else {
            break;
        };
        if &head[..4] == b"EXIF" {
            return exif_orientation(data.strip_prefix(b"Exif\0\0").unwrap_or(data));
        }
        i += 8 + len + (len & 1);
    }
    1
}

/// Orientation (1..=8) of the EXIF block a JPEG, PNG or WebP carries. ImageMagick copies that
/// block into the PNG it writes, so a crop made by `magick` keeps it.
pub(crate) fn exif_block_orientation(b: &[u8]) -> u16 {
    if is_jpeg(b) {
        jpeg_orientation(b)
    } else if is_png(b) {
        png_orientation(b)
    } else if is_webp(b) {
        webp_orientation(b)
    } else {
        1
    }
}

/// The orientation `cv::imdecode` turns an image by: the EXIF block of a JPEG, PNG or WebP,
/// and a TIFF's own Orientation tag (IFD0 of the file), which OpenCV's TIFF decoder applies.
pub fn orientation(b: &[u8]) -> u16 {
    if is_tiff(b) {
        exif_orientation(b)
    } else {
        exif_block_orientation(b)
    }
}

/// Applies an EXIF orientation to interleaved pixels with `c` channels.
pub fn orient(w: usize, h: usize, c: usize, px: Vec<u8>, o: u16) -> (usize, usize, Vec<u8>) {
    if !(2..=8).contains(&o) {
        return (w, h, px);
    }
    let swap = o >= 5;
    let (nw, nh) = if swap { (h, w) } else { (w, h) };
    let mut out = vec![0u8; px.len()];
    for y in 0..nh {
        for x in 0..nw {
            let (sx, sy) = match o {
                2 => (w - 1 - x, y),
                3 => (w - 1 - x, h - 1 - y),
                4 => (x, h - 1 - y),
                5 => (y, x),
                6 => (y, h - 1 - x),
                7 => (w - 1 - y, h - 1 - x),
                _ => (w - 1 - y, x),
            };
            let (d, s) = ((y * nw + x) * c, (sy * w + sx) * c);
            out[d..d + c].copy_from_slice(&px[s..s + c]);
        }
    }
    (nw, nh, out)
}

/// A JPEG as OpenCV's `grfmt_jpeg.cpp` reads it into `format` (`RGB`, `BGR` or `GRAY`), after
/// the size check of [`crate::limit`]: libjpeg-turbo's own conversion, except for a
/// four-component file (CMYK, YCCK), which OpenCV reads as CMYK and converts itself with
/// `icvCvt_CMYK2BGR_8u_C4C3R` / `icvCvt_CMYK2Gray_8u_C4C1R`.
pub fn jpeg(
    b: &[u8],
    format: turbojpeg::PixelFormat,
) -> std::io::Result<turbojpeg::Image<Vec<u8>>> {
    use turbojpeg::PixelFormat::{BGR, GRAY, RGB};
    let Some(cmyk) = crate::limit::jpeg_cmyk(b)? else {
        return crate::limit::jpeg(b, format);
    };
    if !matches!(format, RGB | BGR | GRAY) {
        return Err(std::io::Error::other(
            "CMYK JPEG: unsupported output format",
        ));
    }
    let size = format.size();
    let mut pixels = Vec::with_capacity(cmyk.width * cmyk.height * size);
    for row in cmyk.pixels.chunks(cmyk.pitch) {
        for p in row[..cmyk.width * 4].as_chunks::<4>().0 {
            let k = u32::from(p[3]);
            let [c, m, y] = [p[0], p[1], p[2]].map(|v| k - (((255 - u32::from(v)) * k) >> 8));
            match format {
                RGB => pixels.extend([c as u8, m as u8, y as u8]),
                BGR => pixels.extend([y as u8, m as u8, c as u8]),
                // cB, cG, cR of utils.cpp, 14-bit fixed point.
                _ => pixels.push(((y * 1868 + m * 9617 + c * 4899 + (1 << 13)) >> 14) as u8),
            }
        }
    }
    Ok(turbojpeg::Image {
        pixels,
        width: cmyk.width,
        pitch: cmyk.width * size,
        height: cmyk.height,
        format,
    })
}

/// An AVIF file as `cv::imread(path, IMREAD_COLOR)` reads it with libavif (`grfmt_avif.cpp`: a
/// 4:0:0 file is its luma plane, alpha is dropped, `irot` is not applied). HEIC, which OpenCV
/// does not read, and AVIF on a system without libavif go through libheif.
pub fn heif_bgr(b: &[u8]) -> std::io::Result<Bgr> {
    if let Some(a) = crate::limit::avif(b, heif::avif::Reader::OpenCv) {
        let px = match a.channels {
            1 => a.data.iter().flat_map(|&v| [v; 3]).collect(),
            c => a
                .data
                .chunks_exact(c)
                .flat_map(|p| [p[2], p[1], p[0]])
                .collect(),
        };
        return Ok(Bgr {
            w: a.width,
            h: a.height,
            px,
        });
    }
    let d = crate::limit::heif(b, false)?;
    let (w, h, c) = (d.width, d.height, d.channels());
    let px = d
        .into_u8()
        .chunks_exact(c)
        .flat_map(|p| [p[2], p[1], p[0]])
        .collect();
    Ok(Bgr { w, h, px })
}

/// `cv::imread(path, IMREAD_COLOR)`.
pub fn imread_bgr(path: &str) -> Option<Bgr> {
    imdecode_bgr(&std::fs::read(path).ok()?)
}

/// `cv::imdecode(buf, IMREAD_COLOR)`: JPEG through libjpeg-turbo (as OpenCV's bundled one),
/// anything else through `image`, alpha dropped, 16-bit samples brought to 8 bits and the
/// image turned by its orientation like OpenCV.
///
/// `cv::imread` of OpenCV 4.13 differs in one case: it refuses a TIFF whose Orientation swaps
/// the sides (5..=8), because its TIFF decoder then reallocates the destination.
pub fn imdecode_bgr(b: &[u8]) -> Option<Bgr> {
    let img = imdecode_bgr_stored(b, Via::OpenCv)?;
    let (w, h, px) = orient(img.w, img.h, 3, img.px, orientation(b));
    Some(Bgr { w, h, px })
}

/// Who reads the file itself, for [`imdecode_bgr_stored`].
#[derive(Clone, Copy)]
pub(crate) enum Via {
    /// OpenCV, with its decoder for the format.
    OpenCv,
    /// ImageMagick, which hands OpenCV a PNG of the same samples (`magick F png:-`): 16 bits
    /// stay 16 bits and alpha stays apart, whatever the format was.
    MagickPng,
}

/// [`imdecode_bgr`] before the turn: the pixels as the file stores them.
pub(crate) fn imdecode_bgr_stored(b: &[u8], via: Via) -> Option<Bgr> {
    if is_jpeg(b) {
        let img = jpeg(b, turbojpeg::PixelFormat::BGR).ok()?;
        let (w, h, pitch) = (img.width, img.height, img.pitch);
        let px = if pitch == w * 3 {
            img.pixels
        } else {
            img.pixels
                .chunks(pitch)
                .flat_map(|r| r[..w * 3].iter().copied())
                .collect()
        };
        return Some(Bgr { w, h, px });
    }
    if heif::is_heif(b) {
        return heif_bgr(b).ok();
    }
    if is_png(b) {
        let r = png_rows_stored(b, 3, bgr_conv)?;
        let (w, h) = (r.w, r.h);
        return Some(Bgr {
            w,
            h,
            px: r.collect()?,
        });
    }
    let img = crate::limit::image(b).ok()?;
    Some(match via {
        Via::OpenCv => image_bgr(b, img),
        Via::MagickPng => bgr8(img, None),
    })
}

/// What `cv::imdecode(buf, IMREAD_COLOR)` makes of the pixels `image` decoded from `b`, before
/// the turn by [`orientation`]: BGR, grey replicated, alpha dropped, 16-bit samples brought to
/// 8 bits.
pub fn image_bgr(b: &[u8], img: image::DynamicImage) -> Bgr {
    bgr8(img, is_tiff(b).then(|| tiff_extra_samples(b) == Some(2)))
}

/// A TIFF's ExtraSamples tag: what its first extra sample is (1 = associated alpha, colour
/// already multiplied by it; 2 = unassociated alpha).
pub fn tiff_extra_samples(b: &[u8]) -> Option<u16> {
    tiff_tag(b, 338)
}

/// [`image_bgr`] for a TIFF (`libtiff`: whether its alpha is unassociated, ExtraSamples = 2)
/// or for any other format.
///
/// OpenCV's PNG decoder has libpng drop the low byte of a 16-bit sample (`png_set_strip_16`)
/// and the alpha channel. Its TIFF decoder reads 8-bit BGR through libtiff's RGBA interface
/// (tif_getimage.c), which rounds a 16-bit colour sample (`Bitdepth16To8`: `(v + 128) / 257`),
/// takes the high byte of a 16-bit grey one, and multiplies colour by unassociated alpha
/// (`UaToAa`: `(c * a + 127) / 255`) before OpenCV drops the alpha.
fn bgr8(img: image::DynamicImage, libtiff: Option<bool>) -> Bgr {
    use image::DynamicImage::{ImageRgb16, ImageRgba8, ImageRgba16};
    let (w, h) = (img.width() as usize, img.height() as usize);
    let colour = matches!(img, ImageRgb16(_) | ImageRgba16(_) | ImageRgba8(_));
    let round = libtiff.is_some() && colour;
    let depth8 = |v: u16| {
        if round {
            ((v as u32 + 128) / 257) as u8
        } else {
            (v >> 8) as u8
        }
    };
    let sixteen = img.color().bits_per_pixel() / img.color().channel_count() as u16 == 16;
    let px = if libtiff == Some(true) && colour && img.color().has_alpha() {
        let rgba = if sixteen {
            img.to_rgba16().into_raw().into_iter().map(depth8).collect()
        } else {
            img.to_rgba8().into_raw()
        };
        let mul = |c: u8, a: u8| ((c as u32 * a as u32 + 127) / 255) as u8;
        rgba.as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [mul(p[2], p[3]), mul(p[1], p[3]), mul(p[0], p[3])])
            .collect()
    } else {
        let mut px = if sixteen {
            img.to_rgb16().into_raw().into_iter().map(depth8).collect()
        } else {
            img.to_rgb8().into_raw()
        };
        for p in px.as_chunks_mut::<3>().0 {
            p.swap(0, 2);
        }
        px
    };
    Bgr { w, h, px }
}

/// Converts one decoded PNG row into an output row.
pub(crate) type Conv = Box<dyn Fn(&[u8], &mut [u8]) + Send>;

/// One `w*cn` row of a [`Rows`] source, or `None` to finish (check the rest of the stream).
pub(crate) type NextRow<'a> = Box<dyn FnMut(Option<&mut [u8]>) -> bool + Send + 'a>;

/// A decoded `w`x`h` image with `cn` channels, delivered top to bottom one row per call, so
/// consumers that only need a few rows at a time never hold the whole image.
pub(crate) struct Rows<'a> {
    pub w: usize,
    pub h: usize,
    pub cn: usize,
    pub next: NextRow<'a>,
}

impl<'a> Rows<'a> {
    pub fn mem(w: usize, h: usize, cn: usize, px: Vec<u8>) -> Rows<'a> {
        let line = w * cn;
        let mut y = 0;
        let next = move |out: Option<&mut [u8]>| {
            if let Some(out) = out {
                out.copy_from_slice(&px[y * line..(y + 1) * line]);
                y += 1;
            }
            true
        };
        Rows {
            w,
            h,
            cn,
            next: Box::new(next),
        }
    }

    pub fn collect(mut self) -> Option<Vec<u8>> {
        let line = self.w * self.cn;
        let mut px = vec![0u8; line * self.h];
        for row in px.chunks_exact_mut(line.max(1)) {
            if !(self.next)(Some(row)) {
                return None;
            }
        }
        (self.next)(None).then_some(px)
    }

    /// The image with EXIF orientation `o` applied.
    pub fn oriented(self, o: u16) -> Option<Rows<'a>> {
        if !(2..=8).contains(&o) {
            return Some(self);
        }
        let (w, h, cn) = (self.w, self.h, self.cn);
        let (w, h, px) = orient(w, h, cn, self.collect()?, o);
        Some(Rows::mem(w, h, cn, px))
    }
}

/// A PNG decoded with `Transformations::EXPAND` (what both OpenCV's and `image`'s PNG paths
/// start from), each output row converted by `conv(decoded_row, out_row)` into `cn` channels.
/// `make_conv` gets the decoder's output colour type and depth. Non-interlaced images are
/// streamed row by row; Adam7 ones are decoded whole first. EXIF orientation is applied.
pub(crate) fn png_rows<'a>(
    b: &'a [u8],
    cn: usize,
    make_conv: impl FnOnce(png::ColorType, png::BitDepth) -> Option<Conv>,
) -> Option<Rows<'a>> {
    png_rows_stored(b, cn, make_conv)?.oriented(png_orientation(b))
}

/// [`png_rows`] without the turn: the rows as the file stores them.
pub(crate) fn png_rows_stored<'a>(
    b: &'a [u8],
    cn: usize,
    make_conv: impl FnOnce(png::ColorType, png::BitDepth) -> Option<Conv>,
) -> Option<Rows<'a>> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(b));
    dec.set_transformations(png::Transformations::EXPAND);
    let mut reader = dec.read_info().ok()?;
    let frame = crate::limit::png_buffer(&reader).ok()?;
    let (oc, od) = reader.output_color_type();
    let conv = make_conv(oc, od)?;
    let (w, h) = reader.info().size();
    let (w, h) = (w as usize, h as usize);
    let next: NextRow<'a> = if reader.info().interlaced {
        let mut buf = vec![0u8; frame];
        let line = reader.next_frame(&mut buf).ok()?.line_size;
        let mut y = 0;
        Box::new(move |out: Option<&mut [u8]>| {
            if let Some(out) = out {
                conv(&buf[y * line..(y + 1) * line], out);
                y += 1;
            }
            true
        })
    } else {
        let mut raw = vec![0u8; reader.output_line_size(w as u32)?];
        Box::new(move |out: Option<&mut [u8]>| match out {
            Some(out) => match reader.read_row(&mut raw) {
                Ok(Some(_)) => {
                    conv(&raw, out);
                    true
                }
                _ => false,
            },
            // After the last row this finishes the IDAT stream, as `next_frame` does.
            None => matches!(reader.read_row(&mut raw), Ok(None)),
        })
    };
    Some(Rows { w, h, cn, next })
}

/// The per-row conversion of the PNG branch of [`imdecode_bgr`]: `image`'s decode (EXPAND) and
/// `to_rgb8` / `to_rgb16 >> 8`, i.e. grey replicated, alpha dropped, 16-bit samples' high byte,
/// as BGR.
pub(crate) fn bgr_conv(oc: png::ColorType, od: png::BitDepth) -> Option<Conv> {
    if oc == png::ColorType::Indexed {
        return None;
    }
    let (n, step) = (
        oc.samples(),
        if od == png::BitDepth::Sixteen { 2 } else { 1 },
    );
    let gray = matches!(
        oc,
        png::ColorType::Grayscale | png::ColorType::GrayscaleAlpha
    );
    Some(Box::new(move |row: &[u8], out: &mut [u8]| {
        for (o, p) in out
            .as_chunks_mut::<3>()
            .0
            .iter_mut()
            .zip(row.chunks_exact(n * step))
        {
            if gray {
                o.fill(p[0]);
            } else {
                *o = [p[2 * step], p[step], p[0]];
            }
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rng_matches_cv_default_sequence() {
        // What OpenCV 4.13.0 draws from `cv::theRNG()` in a fresh process:
        // `cv2.randu(np.zeros(4, np.int32), 0, 1 << 30)` is the low 30 bits of the first four
        // values, `cv2.randu(np.zeros(4, np.uint16), 0, 1 << 16)` after it the low 16 bits of
        // the next four.
        let mut r = Rng::default();
        let low30: Vec<u32> = (0..4).map(|_| r.next() & 0x3fff_ffff).collect();
        assert_eq!(low30, [0x07c0_9cf6, 0x3302_a6a5, 0x26ab_d4e7, 0x107b_8a5d]);
        let low16: Vec<u32> = (0..4).map(|_| r.next() & 0xffff).collect();
        assert_eq!(low16, [0xf269, 0xaeae, 0x3686, 0x9319]);
    }

    #[test]
    fn orient_rotates_like_exif() {
        // 2x1 [a b] with orientation 6 (rotate 90 cw) -> 1x2 [a; b].
        let (w, h, p) = orient(2, 1, 1, vec![1, 2], 6);
        assert_eq!((w, h, p), (1, 2, vec![1, 2]));
    }
}
