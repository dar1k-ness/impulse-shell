//! QtQuick Image (QQuickImage::updatePaintNode + QSGDefaultInternalImageNode, antialiasing off).

pub use crate::layer::{Lod, mip_levels};
use crate::layer::{cover_uv, linear, mip_sample, nearest, unorm};
use crate::raster::Canvas;
use mul255::qt as byte_mul;

/// QImageReader::allocationLimit()'s default: an image whose 32-bit pixels would take more than
/// 256 MB is refused from its header, before anything is decoded or allocated, so a small file
/// (or a network Image source) claiming a huge size cannot exhaust memory.
pub const MAX_DECODED_BYTES: usize = 256 << 20;

/// `Err` (`OutOfMemory`) when a `w`x`h` image is over [`MAX_DECODED_BYTES`].
pub fn check_size(w: usize, h: usize) -> std::io::Result<()> {
    if w.checked_mul(h)
        .and_then(|n| n.checked_mul(4))
        .is_some_and(|n| n <= MAX_DECODED_BYTES)
    {
        return Ok(());
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::OutOfMemory,
        format!("{w}x{h} image is over the {MAX_DECODED_BYTES}-byte decode limit"),
    ))
}

/// A straight 8-bit pixel as ARGB32_Premultiplied.
fn premul_rgba8(p: [u8; 4]) -> [u8; 4] {
    let [r, g, b, a] = p;
    if a == 255 {
        return p;
    }
    let a32 = a as u32;
    [
        byte_mul(r as u32, a32) as u8,
        byte_mul(g as u32, a32) as u8,
        byte_mul(b as u32, a32) as u8,
        a,
    ]
}

/// A channel of 16 bits as Qt brings it to 8: rounded, not the high byte.
fn rgba64_to8(v: u16) -> u8 {
    ((v as u32 + 128) / 257) as u8
}

/// A straight pixel of 16 bits per channel (QImage::Format_RGBA64) as Qt converts it to
/// ARGB32_Premultiplied: alpha is multiplied in at 16 bits (`QRgba64::premultiplied`), then
/// each channel goes to 8 bits by [`rgba64_to8`]. Taking the high byte, or multiplying after
/// the cut to 8 bits, is off by up to 2.
fn premul_rgba64(p: [u16; 4]) -> [u8; 4] {
    let a = p[3] as u32;
    let mul = |c: u16| match a {
        65535 => c,
        _ => {
            let t = c as u32 * a;
            ((t + (t >> 16) + 0x8000) >> 16) as u16
        }
    };
    [mul(p[0]), mul(p[1]), mul(p[2]), p[3]].map(rgba64_to8)
}

/// The file is a TIFF whose ExtraSamples tag says associated alpha (1).
fn tiff_associated_alpha(data: &[u8]) -> bool {
    if !(data.starts_with(b"II*\0") || data.starts_with(b"MM\0*")) {
        return false;
    }
    tiff::decoder::Decoder::new(std::io::Cursor::new(data))
        .and_then(|mut d| d.find_tag_unsigned_vec::<u16>(tiff::tags::Tag::ExtraSamples))
        .is_ok_and(|v| v.is_some_and(|v| v.first() == Some(&1)))
}

/// Decode a PNG into a premultiplied texture (QImage → ARGB32_Premultiplied via qPremultiply).
pub fn load_png(path: &str) -> std::io::Result<Canvas> {
    load_png_ex(path).map(|(c, _)| c)
}

/// Decode by content like QImageReader: JPEG goes through libjpeg-turbo with QJpegHandler's
/// defaults (quality 75 >= HIGH_QUALITY_THRESHOLD: JDCT_ISLOW + fancy upsampling) to RGB32.
pub fn load_ex(path: &str) -> std::io::Result<(Canvas, bool)> {
    decode_ex(&std::fs::read(path)?)
}

/// `load_ex` for a file already in memory.
pub fn decode_ex(data: &[u8]) -> std::io::Result<(Canvas, bool)> {
    decode_as(data, false).map(|(c, alpha, _)| (c, alpha))
}

/// `decode_ex`; with `straight` the pixels of a Format_ARGB32 image (8 bits a channel, with
/// alpha) stay as the file has them, not premultiplied, which the last answer tells.
fn decode_as(data: &[u8], straight: bool) -> std::io::Result<(Canvas, bool, bool)> {
    if data.starts_with(&[0xFF, 0xD8]) {
        return decode_jpeg(data).map(|c| (c, false, false));
    }
    if data.starts_with(b"\x89PNG") {
        return decode_png(std::io::Cursor::new(data), straight);
    }
    if heif::is_heif(data) {
        return decode_heif(data).map(|(c, alpha)| (c, alpha, false));
    }
    decode_other(data, straight)
}

/// `decode_ex` read at another size, as QImageReader::setScaledSize has it read: `size`
/// answers the size wanted for the file's. A Format_ARGB32 image is scaled from the pixels
/// the file has, as QImage::scaled scales it, not from premultiplied ones.
pub fn decode_scaled(
    data: &[u8],
    size: impl FnOnce(usize, usize) -> (usize, usize),
) -> std::io::Result<(Canvas, bool)> {
    let (mut c, alpha, straight) = decode_as(data, true)?;
    let (w, h) = size(c.w, c.h);
    if straight {
        if (w, h) != (c.w, c.h) {
            return Ok((scale_straight(&c.px, c.w, c.h, w, h), alpha));
        }
        for p in &mut c.px {
            *p = premul_rgba8(*p);
        }
    } else if (w, h) != (c.w, c.h) {
        c = scale_decoded(&c, w, h, alpha);
    }
    Ok((c, alpha))
}

/// AVIF/HEIC (Qt reads them with kimageformats' plugins, qt6-imageformats has none) through libheif: the primary
/// image with its `irot`/`imir`/`clap` applied, premultiplied unless the file already is.
pub fn decode_heif(data: &[u8]) -> std::io::Result<(Canvas, bool)> {
    let (w, h) = heif::size(data)?;
    check_size(w, h)?;
    let d = heif::decode(data, false)?;
    let (w, h, alpha, prem) = (d.width, d.height, d.alpha, d.premultiplied);
    let s = d.into_u8();
    let px = if !alpha {
        s.as_chunks::<3>()
            .0
            .iter()
            .map(|p| [p[0], p[1], p[2], 255])
            .collect()
    } else {
        s.as_chunks::<4>()
            .0
            .iter()
            .map(|p| {
                let a = p[3];
                if prem {
                    [p[0].min(a), p[1].min(a), p[2].min(a), a]
                } else if a == 255 {
                    [p[0], p[1], p[2], a]
                } else {
                    let a32 = a as u32;
                    [
                        byte_mul(p[0] as u32, a32) as u8,
                        byte_mul(p[1] as u32, a32) as u8,
                        byte_mul(p[2] as u32, a32) as u8,
                        a,
                    ]
                }
            })
            .collect()
    };
    Ok((Canvas::from_px(w, h, px), alpha))
}

/// `load_ex` for a consumer that scales the image down to fit `long_side` px anyway: JPEGs are
/// decoded at the smallest DCT scale (1/2, 1/4, 1/8) that still covers the target size, so a 4K
/// or 8K wallpaper never decodes at full size. Also returns the full image size.
pub fn load_scaled(path: &str, long_side: f64) -> std::io::Result<(Canvas, bool, (usize, usize))> {
    let data = std::fs::read(path)?;
    if data.starts_with(&[0xFF, 0xD8]) && long_side > 0.0 {
        let mut d = turbojpeg::Decompressor::new().map_err(std::io::Error::other)?;
        let hdr = d.read_header(&data).map_err(std::io::Error::other)?;
        let (w, h) = (hdr.width, hdr.height);
        let (tw, th) = fit_long_side(w, h, long_side);
        let factor = [
            turbojpeg::ScalingFactor::ONE_EIGHTH,
            turbojpeg::ScalingFactor::ONE_QUARTER,
            turbojpeg::ScalingFactor::ONE_HALF,
        ]
        .into_iter()
        .find(|f| {
            let s = hdr.scaled(*f);
            !hdr.is_lossless && s.width >= tw && s.height >= th
        })
        .unwrap_or(turbojpeg::ScalingFactor::ONE);
        d.set_scaling_factor(factor)
            .map_err(std::io::Error::other)?;
        let s = hdr.scaled(factor);
        check_size(s.width, s.height)?;
        let px = jpeg_pixels(&mut d, &data, &hdr, s.width, s.height)?;
        let c = Canvas {
            w: s.width,
            h: s.height,
            px,
            key: Default::default(),
        };
        return Ok((c, false, (w, h)));
    }
    let (c, alpha) = if data.starts_with(b"\x89PNG") {
        load_png_bytes(&data)?
    } else if data.starts_with(&[0xFF, 0xD8]) {
        (decode_jpeg(&data)?, false)
    } else if heif::is_heif(&data) {
        decode_heif(&data)?
    } else {
        decode_other(&data, false).map(|(c, alpha, _)| (c, alpha))?
    };
    let size = (c.w, c.h);
    Ok((c, alpha, size))
}

/// The size `QImage::scaled(s, s, Qt::KeepAspectRatio)` gives a `w`x`h` image: (0, 0), a null
/// image, for an empty target (`s < 1` once truncated to int).
pub fn fit_long_side(w: usize, h: usize, s: f64) -> (usize, usize) {
    let s = s.trunc();
    if w == 0 || h == 0 || s.is_nan() || s < 1.0 {
        return (0, 0);
    }
    let (dw, dh) = if w >= h {
        (s, (s * h as f64 / w as f64).round().max(1.0))
    } else {
        ((s * w as f64 / h as f64).round().max(1.0), s)
    };
    (dw as usize, dh as usize)
}

/// The first entry of an ICO file's directory, the image QImageReader gives (`image` picks the
/// largest one).
enum IcoEntry<'a> {
    /// The entry's PNG file, which QtIcoHandler hands to the PNG reader as it is.
    Png(&'a [u8]),
    /// An ICO file holding this BMP entry alone.
    Bmp(Vec<u8>),
}

/// `None` when `data` is no ICO file or its first entry lies outside it.
fn ico_first_entry(data: &[u8]) -> Option<IcoEntry<'_>> {
    // ICONDIR: reserved 0, type 1, count; then 16-byte ICONDIRENTRYs ending in the entry's
    // size and offset.
    if !data.starts_with(&[0, 0, 1, 0]) || data.len() < 22 {
        return None;
    }
    let u32_at = |at: usize| u32::from_le_bytes(data[at..at + 4].try_into().unwrap()) as usize;
    let (size, offset) = (u32_at(14), u32_at(18));
    let entry = data.get(offset..offset.checked_add(size)?)?;
    if entry.starts_with(b"\x89PNG") {
        return Some(IcoEntry::Png(entry));
    }
    let mut ico = Vec::with_capacity(22 + entry.len());
    ico.extend_from_slice(&[0, 0, 1, 0, 1, 0]);
    ico.extend_from_slice(&data[6..18]);
    ico.extend_from_slice(&22u32.to_le_bytes());
    ico.extend_from_slice(entry);
    Some(IcoEntry::Bmp(ico))
}

/// Whether a GIF's first frame names no transparent colour, which is when QGifHandler makes
/// an image without alpha (Format_RGB32); `image` reports every GIF as RGBA. `None` when
/// `data` is no GIF or ends before its first frame.
fn gif_first_frame_opaque(data: &[u8]) -> Option<bool> {
    if !data.starts_with(b"GIF8") {
        return None;
    }
    // Header and logical screen descriptor, then the global colour table if bit 7 is set.
    let flags = *data.get(10)?;
    let mut at = 13;
    if flags & 0x80 != 0 {
        at += 3 << ((flags & 7) + 1);
    }
    let mut opaque = true;
    loop {
        match *data.get(at)? {
            // Image descriptor: the first frame starts.
            0x2C => return Some(opaque),
            // Extension: a label, then sub-blocks up to an empty one. The graphic control
            // extension (0xF9) has the transparent-colour flag in bit 0 of its first byte.
            0x21 => {
                if *data.get(at + 1)? == 0xF9 && *data.get(at + 2)? >= 1 {
                    opaque = *data.get(at + 3)? & 1 == 0;
                }
                at += 2;
                loop {
                    let n = *data.get(at)? as usize;
                    at += 1 + n;
                    if n == 0 {
                        break;
                    }
                }
            }
            _ => return None,
        }
    }
}

/// The other formats Qt reads (its built-in GIF, ICO, PNM, BMP and XPM handlers, and
/// qt6-imageformats' WebP and TIFF), premultiplied.
fn decode_other(data: &[u8], straight: bool) -> std::io::Result<(Canvas, bool, bool)> {
    let gif_opaque = gif_first_frame_opaque(data);
    // QXpmHandler::canRead: the file starts with "/* XPM".
    let img = if data.starts_with(b"/* XPM") {
        use image::ImageDecoder;
        let dec = image_extras::xpm::XpmDecoder::new(std::io::Cursor::new(data))
            .map_err(std::io::Error::other)?;
        let (w, h) = dec.dimensions();
        check_size(w as usize, h as usize)?;
        image::DynamicImage::from_decoder(dec).map_err(std::io::Error::other)?
    } else {
        let first;
        let data = match ico_first_entry(data) {
            Some(IcoEntry::Png(png)) => return decode_png(std::io::Cursor::new(png), straight),
            Some(IcoEntry::Bmp(ico)) => {
                first = ico;
                &first[..]
            }
            None => data,
        };
        let reader = || image::ImageReader::new(std::io::Cursor::new(data)).with_guessed_format();
        let (w, h) = reader()?.into_dimensions().map_err(std::io::Error::other)?;
        check_size(w as usize, h as usize)?;
        let mut r = reader()?;
        let mut limits = image::Limits::default();
        limits.max_alloc = Some(MAX_DECODED_BYTES as u64);
        r.limits(limits);
        r.decode().map_err(std::io::Error::other)?
    };
    let has_alpha = img.color().has_alpha() && gif_opaque != Some(true);
    let (w, h) = (img.width() as usize, img.height() as usize);
    let sixteen = img.color().bits_per_pixel() / img.color().channel_count() as u16 == 16;
    // A TIFF with associated alpha holds colour already multiplied by it: Qt's TIFF handler
    // reads it into a premultiplied format as it is.
    let premultiplied = has_alpha && tiff_associated_alpha(data);
    let px = if sixteen {
        // A QImage of 16 bits per channel (qt6-imageformats' TIFF handler gives RGBX64,
        // RGBA64 or RGBA64_Premultiplied, Grayscale16).
        let rgba = img.into_rgba16().into_raw();
        let px = rgba.as_chunks::<4>().0.iter();
        if premultiplied {
            px.map(|p| p.map(rgba64_to8)).collect()
        } else {
            px.map(|p| premul_rgba64(*p)).collect()
        }
    } else if premultiplied || (straight && has_alpha) {
        img.into_rgba8().pixels().map(|p| p.0).collect()
    } else {
        img.into_rgba8()
            .pixels()
            .map(|p| premul_rgba8(p.0))
            .collect()
    };
    let straight = straight && has_alpha && !sixteen && !premultiplied;
    Ok((Canvas::from_px(w, h, px), has_alpha, straight))
}

pub fn decode_jpeg(data: &[u8]) -> std::io::Result<Canvas> {
    let mut d = turbojpeg::Decompressor::new().map_err(std::io::Error::other)?;
    let hdr = d.read_header(data).map_err(std::io::Error::other)?;
    check_size(hdr.width, hdr.height)?;
    Ok(Canvas {
        w: hdr.width,
        h: hdr.height,
        px: jpeg_pixels(&mut d, data, &hdr, hdr.width, hdr.height)?,
        key: Default::default(),
    })
}

/// Decompresses a JPEG into `w`x`h` opaque pixels (the header's size, or its size at the
/// decompressor's scaling factor). TurboJPEG converts no four-component file (CMYK, YCCK) to
/// RGB; QJpegHandler reads one as Format_CMYK8888, taking the stored samples as inverted (255
/// is no ink), and painting converts that with `(255 - ink) * (255 - black) / 255`, rounded.
fn jpeg_pixels(
    d: &mut turbojpeg::Decompressor,
    data: &[u8],
    hdr: &turbojpeg::DecompressHeader,
    w: usize,
    h: usize,
) -> std::io::Result<Vec<[u8; 4]>> {
    use turbojpeg::{Colorspace, PixelFormat};
    let cmyk = matches!(hdr.colorspace, Colorspace::CMYK | Colorspace::YCCK);
    let pitch = 4 * w;
    let mut buf = vec![0u8; pitch * h];
    d.decompress(
        data,
        turbojpeg::Image {
            pixels: &mut buf[..],
            width: w,
            pitch,
            height: h,
            format: if cmyk {
                PixelFormat::CMYK
            } else {
                PixelFormat::RGBA
            },
        },
    )
    .map_err(std::io::Error::other)?;
    let px = buf.as_chunks::<4>().0.iter();
    Ok(if cmyk {
        px.map(|p| {
            let k = p[3] as u32;
            let rgb = |v: u8| mul255::nearest(v as u32, k) as u8;
            [rgb(p[0]), rgb(p[1]), rgb(p[2]), 255]
        })
        .collect()
    } else {
        px.map(|p| [p[0], p[1], p[2], 255]).collect()
    })
}

/// As `load_png`, also reporting whether Qt's PNG handler yields an alpha format (ARGB32).
pub fn load_png_ex(path: &str) -> std::io::Result<(Canvas, bool)> {
    decode_png(std::io::BufReader::new(std::fs::File::open(path)?), false)
        .map(|(c, alpha, _)| (c, alpha))
}

/// `load_png_ex` for in-memory PNG data (`QImage::loadFromData(data, "PNG")`).
pub fn load_png_bytes(data: &[u8]) -> std::io::Result<(Canvas, bool)> {
    decode_png(std::io::Cursor::new(data), false).map(|(c, alpha, _)| (c, alpha))
}

fn decode_png<R: std::io::BufRead + std::io::Seek>(
    r: R,
    straight: bool,
) -> std::io::Result<(Canvas, bool, bool)> {
    let mut dec = png::Decoder::new(r);
    dec.set_transformations(png::Transformations::EXPAND);
    let mut r = dec.read_info().map_err(std::io::Error::other)?;
    check_size(r.info().width as usize, r.info().height as usize)?;
    let has_alpha = r.info().trns.is_some()
        || matches!(
            r.info().color_type,
            png::ColorType::Rgba | png::ColorType::GrayscaleAlpha
        );
    let (w, h) = (r.info().width as usize, r.info().height as usize);
    let (color, depth) = r.output_color_type();
    let sixteen = depth == png::BitDepth::Sixteen;
    // Bytes of one pixel in a decoded row.
    let ch = color.samples() * if sixteen { 2 } else { 1 };
    let mut cv = Canvas::new(w, h, [0, 0, 0, 0]);
    let straight = straight && has_alpha && !sixteen;
    let premul = |s: &[u8]| {
        if sixteen {
            // QPngHandler keeps 16 bits per channel (RGBX64, RGBA64, Grayscale16).
            let v = |i: usize| u16::from_be_bytes([s[2 * i], s[2 * i + 1]]);
            return premul_rgba64(match ch {
                2 => [v(0), v(0), v(0), 65535],
                4 => [v(0), v(0), v(0), v(1)],
                6 => [v(0), v(1), v(2), 65535],
                _ => [v(0), v(1), v(2), v(3)],
            });
        }
        let p = match ch {
            1 => [s[0], s[0], s[0], 255],
            2 => [s[0], s[0], s[0], s[1]],
            3 => [s[0], s[1], s[2], 255],
            _ => [s[0], s[1], s[2], s[3]],
        };
        match straight {
            true => p,
            false => premul_rgba8(p),
        }
    };
    if r.info().interlaced {
        let n = r
            .output_buffer_size()
            .ok_or_else(|| std::io::Error::other("PNG too large"))?;
        let mut buf = vec![0; n];
        r.next_frame(&mut buf).map_err(std::io::Error::other)?;
        for (o, s) in cv.px.iter_mut().zip(buf.chunks_exact(ch)) {
            *o = premul(s);
        }
    } else {
        // Rows go straight into the canvas: the decoded frame never exists on its own.
        for row in cv.px.chunks_exact_mut(w.max(1)) {
            let src = r.next_row().map_err(std::io::Error::other)?;
            let src = src.ok_or_else(|| std::io::Error::other("PNG ended early"))?;
            for (o, s) in row.iter_mut().zip(src.data().chunks_exact(ch)) {
                *o = premul(s);
            }
        }
        // Finishes the image data, as `next_frame` does.
        if r.next_row().map_err(std::io::Error::other)?.is_some() {
            return Err(std::io::Error::other("PNG has extra rows"));
        }
    }
    Ok((cv, has_alpha, straight))
}

/// Size QQuickPixmap loads a raster image at for Image.sourceSize (0 = unset), measured
/// against Qt 6.12: Stretch scales down only, by the smaller ratio; PreserveAspectFit/Crop
/// scale (also up) by the larger ratio. The other side is qRound(orig * ratio).
pub fn load_size(ow: usize, oh: usize, rw: i32, rh: i32, fill: FillMode) -> (usize, usize) {
    let (owf, ohf) = (ow as f64, oh as f64);
    let rx = (rw > 0).then(|| rw as f64 / owf);
    let ry = (rh > 0).then(|| rh as f64 / ohf);
    let ratio = match (rx, ry, fill) {
        (None, None, _) => return (ow, oh),
        (Some(a), None, _) | (None, Some(a), _) => a,
        (Some(a), Some(b), FillMode::Stretch) => a.min(b),
        (Some(a), Some(b), _) => a.max(b),
    };
    if fill == FillMode::Stretch && ratio >= 1.0 {
        return (ow, oh);
    }
    let w = if rx == Some(ratio) {
        rw as usize
    } else {
        (owf * ratio).round() as usize
    };
    let h = if ry == Some(ratio) {
        rh as usize
    } else {
        (ohf * ratio).round() as usize
    };
    (w.max(1), h.max(1))
}

/// QSize(w, h).scaled(bw, bh, Qt::KeepAspectRatio): whole numbers, the divided side truncated.
pub fn qsize_fit(w: i32, h: i32, bw: i32, bh: i32) -> (i32, i32) {
    if w == 0 || h == 0 {
        return (bw, bh);
    }
    let rw = bh as i64 * w as i64 / h as i64;
    if rw <= bw as i64 {
        (rw as i32, bh)
    } else {
        (bw, (bw as i64 * h as i64 / w as i64) as i32)
    }
}

/// Size of the pixmap one icon engine gives a file of `ow`x`oh` for `rw`x`rh`: an SVG's
/// default size is fitted into the request, also upwards (QSvgIconEngine); a raster file is
/// only shrunk (QPixmapIconEngine, QIconLoader's PixmapEntry).
pub fn qicon_fit(ow: i32, oh: i32, rw: i32, rh: i32, svg: bool) -> (i32, i32) {
    if svg || ow > rw || oh > rh {
        qsize_fit(ow, oh, rw, rh)
    } else {
        (ow, oh)
    }
}

/// QIconPrivate::pixmapDevicePixelRatio: the ratio a pixmap of `actual` pixels has when
/// `req` was asked for at `dpr`.
pub fn qicon_pixmap_dpr(dpr: f64, req: (i32, i32), actual: (i32, i32)) -> f64 {
    let t = qsize_times(req.0, req.1, dpr);
    if (actual.0 == t.0 && actual.1 <= t.1) || (actual.0 <= t.0 && actual.1 == t.1) {
        return dpr;
    }
    let scale = 0.5 * (actual.0 as f64 / t.0 as f64 + actual.1 as f64 / t.1 as f64);
    ((dpr * scale).max(1.0) * 100.0).round() / 100.0
}

/// QIcon::actualSize for `rw`x`rh` where QGuiApplication::devicePixelRatio() is `app`
/// (the highest of the screens' ratios; a Wayland screen has its output's whole scale) and
/// `engine` is the icon engine's actualSize: above 1 the engine is asked for the size times
/// `app` and its answer is divided by the ratio a pixmap of that size would have.
pub fn qicon_actual(
    app: f64,
    rw: i32,
    rh: i32,
    mut engine: impl FnMut(i32, i32) -> (i32, i32),
) -> (i32, i32) {
    if app <= 1.0 {
        return engine(rw, rh);
    }
    let (tw, th) = qsize_times(rw, rh, app);
    let e = engine(tw, th);
    qsize_times(e.0, e.1, 1.0 / qicon_pixmap_dpr(app, (rw, rh), e))
}

/// Size of the pixmap QIcon(file) gives for `rw`x`rh` in a window of devicePixelRatio `dpr`:
/// QIcon::actualSize ([`qicon_actual`]), then QIcon::pixmap of that size at `dpr`, as
/// Kirigami's Icon::iconPixmap asks. The engine fits its own answer once more, times `dpr`,
/// when the pixmap of that size is asked for. `nested`: the file is a Fallback entry of
/// QIconLoaderEngine, whose actualSize asks QIcon(file)::actualSize in turn.
pub fn qicon_size(
    (ow, oh): (i32, i32),
    (rw, rh): (i32, i32),
    (app, dpr): (f64, f64),
    nested: bool,
    svg: bool,
) -> (i32, i32) {
    let fit = |w, h| qicon_fit(ow, oh, w, h, svg);
    let (aw, ah) = if nested {
        qicon_actual(app, rw, rh, |w, h| qicon_actual(app, w, h, fit))
    } else {
        qicon_actual(app, rw, rh, fit)
    };
    let (dw, dh) = qsize_times(aw, ah, dpr);
    fit(dw, dh)
}

/// `QSize * qreal`: each side rounded.
pub fn qsize_times(w: i32, h: i32, f: f64) -> (i32, i32) {
    ((w as f64 * f).round() as i32, (h as f64 * f).round() as i32)
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum FillMode {
    Stretch,
    PreserveAspectFit,
    PreserveAspectCrop,
}

/// Painted rectangle (relative to the item) and normalized source rectangle of an Image.
pub fn placement(
    tw: usize,
    th: usize,
    w: f64,
    h: f64,
    fill: FillMode,
) -> Option<([f64; 4], [f64; 4])> {
    let (tw, th) = (tw as f64, th as f64);
    let (pw, ph) = match fill {
        FillMode::PreserveAspectFit => {
            let (ws, hs) = (w / tw, h / th);
            if ws <= hs { (w, ws * th) } else { (hs * tw, h) }
        }
        _ => (tw, th),
    };
    let xoff = ((w - pw) / 2.0) as i32 as f64;
    let yoff = ((h - ph) / 2.0) as i32 as f64;
    let (target, source) = match fill {
        FillMode::Stretch => ([0.0, 0.0, w, h], [0.0, 0.0, tw, th]),
        FillMode::PreserveAspectFit => ([xoff, yoff, pw, ph], [0.0, 0.0, tw, th]),
        FillMode::PreserveAspectCrop => {
            let (ws, hs) = (w / tw, h / th);
            let src = if ws > hs {
                let s = ((hs / ws) * th) as i32 as f64;
                [0.0, ((th - s) / 2.0).ceil(), tw, s]
            } else {
                let s = ((ws / hs) * tw) as i32 as f64;
                [((tw - s) / 2.0).ceil(), 0.0, s, th]
            };
            ([0.0, 0.0, w, h], src)
        }
    };
    let ns = [
        source[0] / tw,
        source[1] / th,
        source[2] / tw,
        source[3] / th,
    ];
    if target[2] <= 0.0 || target[3] <= 0.0 || ns[2] <= 0.0 || ns[3] <= 0.0 {
        return None;
    }
    Some((target, ns))
}

/// Draw an Image item at (x, y) sized w x h (AlignHCenter/AlignVCenter, no mirror).
///
/// Rounded as the graphics card rounds it, which is what Qt Quick shows: the texel times the
/// opacity becomes a byte with its half going to even, and that byte is blended in bytes
/// (`raster::blend_f`).
#[allow(clippy::too_many_arguments)]
pub fn draw(
    cv: &mut Canvas,
    tex: &Canvas,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    fill: FillMode,
    smooth: bool,
    opacity: f32,
) {
    draw_scaled(cv, tex, x, y, w, h, fill, smooth, opacity, [1.0, 1.0]);
}

/// [`draw`] through the scale `s` of a layer texture `cv` that is not its scene rect's size.
#[allow(clippy::too_many_arguments)]
pub fn draw_scaled(
    cv: &mut Canvas,
    tex: &Canvas,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    fill: FillMode,
    smooth: bool,
    opacity: f32,
    s: [f64; 2],
) {
    shade(cv, tex, x, y, w, h, fill, smooth, s, |c| {
        c.map(|k| k * opacity)
    });
}

/// [`draw`] of a Kirigami.Icon with `isMask`, which Kirigami 6.31 does on the video card
/// (src/primitives/shaders/icon.frag, ENABLE_MASK): `vec4(mask_color.xyz * color.a, color.a)`,
/// the premultiplied `color` times the icon's alpha, and the icon's alpha itself as the alpha,
/// not multiplied by the colour's. The shader tints what the filter gives of the texture.
/// `s` is the scale of a layer texture `cv`, as in [`draw_scaled`].
#[allow(clippy::too_many_arguments)]
pub fn draw_mask(
    cv: &mut Canvas,
    tex: &Canvas,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    fill: FillMode,
    smooth: bool,
    opacity: f32,
    color: [u8; 4],
    s: [f64; 2],
) {
    shade(cv, tex, x, y, w, h, fill, smooth, s, |c| {
        let a = (c[3] * 255.0).round() as u32;
        // The shader multiplies floats and the frame takes the nearest byte:
        // `mul255::nearest(v, a)` for an opaque colour.
        let f = |v: u8| (2 * v as u32 * color[3] as u32 * a + 255 * 255) / (2 * 255 * 255);
        [f(color[0]), f(color[1]), f(color[2]), a].map(|k| unorm(k as u8) * opacity)
    });
}

/// `tex` placed as `fill` places it, each pixel what `shader` makes of the filtered texel.
#[allow(clippy::too_many_arguments)]
fn shade(
    cv: &mut Canvas,
    tex: &Canvas,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    fill: FillMode,
    smooth: bool,
    s: [f64; 2],
    shader: impl Fn([f32; 4]) -> [f32; 4],
) {
    let Some((target, ns)) = placement(tex.w, tex.h, w, h, fill) else {
        return;
    };
    cover_uv(
        cv,
        s[0] * (x + target[0]),
        s[1] * (y + target[1]),
        s[0] * target[2],
        s[1] * target[3],
        ns,
        |u, v| {
            shader(if smooth {
                linear(tex, u, v)
            } else {
                nearest(tex, u, v)
            })
        },
    );
}

/// `draw` with blending off: every pixel under the picture becomes the picture's, alpha and
/// all. Qt Quick draws so a picture with no alpha channel that nothing makes translucent
/// (QSGOpaqueTextureMaterial); it shows where such a picture's alpha bytes are not 255.
#[allow(clippy::too_many_arguments)]
pub fn draw_unblended(
    cv: &mut Canvas,
    tex: &Canvas,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    fill: FillMode,
    smooth: bool,
) {
    draw_unblended_scaled(cv, tex, x, y, w, h, fill, smooth, [1.0, 1.0]);
}

/// [`draw_unblended`] through the scale `s` of a layer texture `cv` that is not its scene
/// rect's size.
#[allow(clippy::too_many_arguments)]
pub fn draw_unblended_scaled(
    cv: &mut Canvas,
    tex: &Canvas,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    fill: FillMode,
    smooth: bool,
    s: [f64; 2],
) {
    let Some((target, ns)) = placement(tex.w, tex.h, w, h, fill) else {
        return;
    };
    let (x, y) = (s[0] * (x + target[0]), s[1] * (y + target[1]));
    let (w, h) = (s[0] * target[2], s[1] * target[3]);
    // The pixels `draw` shades: those whose centre is in the rectangle.
    let x0 = (x - 0.5).ceil().max(0.0) as usize;
    let x1 = ((x + w - 0.5).ceil().max(0.0) as usize).min(cv.w);
    let (y0, y1) = cv.rows(y, h);
    for py in y0..y1 {
        for px in x0..x1 {
            let (u, v) = ((px as f64 + 0.5 - x) / w, (py as f64 + 0.5 - y) / h);
            let (su, sv) = (ns[0] + u * ns[2], ns[1] + v * ns[3]);
            let c = if smooth {
                linear(tex, su, sv)
            } else {
                nearest(tex, su, sv)
            };
            cv.px[py * cv.w + px] = c.map(|k| crate::raster::rnd(k.clamp(0.0, 1.0) * 255.0) as u8);
        }
    }
}

/// [`draw`] of an Image with `mipmap: true` (QQuickImage sets QSGTexture::Linear mipmap
/// filtering): shrunk, it reads the levels [`Lod::choose`] names, and at its size or larger
/// it is the draw without mipmaps. `levels` gives [`mip_levels`] of `tex` and is asked only
/// when a level after the first is read.
#[allow(clippy::too_many_arguments)]
pub fn draw_mipmapped<L: std::ops::Deref<Target = [Canvas]>>(
    cv: &mut Canvas,
    tex: &Canvas,
    levels: impl FnOnce() -> L,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    fill: FillMode,
    smooth: bool,
    opacity: f32,
) {
    let s = [1.0, 1.0];
    draw_mipmapped_scaled(cv, tex, levels, x, y, w, h, fill, smooth, opacity, s);
}

/// [`draw_mipmapped`] through the scale `s` of a layer texture `cv` that is not its scene
/// rect's size.
#[allow(clippy::too_many_arguments)]
pub fn draw_mipmapped_scaled<L: std::ops::Deref<Target = [Canvas]>>(
    cv: &mut Canvas,
    tex: &Canvas,
    levels: impl FnOnce() -> L,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    fill: FillMode,
    smooth: bool,
    opacity: f32,
    s: [f64; 2],
) {
    let Some((target, ns)) = placement(tex.w, tex.h, w, h, fill) else {
        return;
    };
    let size = [s[0] * target[2], s[1] * target[3]];
    let lod = mip_lod(tex.w, tex.h, [target[0], target[1], size[0], size[1]], ns);
    if lod.is_base() {
        return draw_scaled(cv, tex, x, y, w, h, fill, smooth, opacity, s);
    }
    let levels = levels();
    let chain: Vec<&Canvas> = std::iter::once(tex).chain(levels.iter()).collect();
    cover_uv(
        cv,
        s[0] * (x + target[0]),
        s[1] * (y + target[1]),
        size[0],
        size[1],
        ns,
        |u, v| mip_sample(&chain, lod, smooth, u, v).map(|k| k * opacity),
    );
}

/// The levels a `tw` x `th` texture's draw reads when [`placement`] gave it `target` and `ns`.
pub fn mip_lod(tw: usize, th: usize, target: [f64; 4], ns: [f64; 4]) -> Lod {
    // 1x1 is the last level: one more than the halvings of the longer side.
    let levels = tw.max(th).max(1).ilog2() as usize + 1;
    Lod::choose(
        ns[2] / target[2] * tw as f64,
        ns[3] / target[3] * th as f64,
        levels,
    )
}

/// QImage::scaled(dw, dh, IgnoreAspectRatio, SmoothTransformation) → qSmoothScaleImage
/// (qimagescale.cpp, SSE4.1 kernels) on a premultiplied texture. `has_alpha == false` is the
/// Format_RGB32 path (qt_qimageScaleAARGB): alpha is set to 255 where a side gets smaller;
/// larger on both sides is the code of pictures with alpha, which keeps the alpha bytes the
/// picture has.
/// An empty target gives an empty canvas (QImage::scaled returns a null image).
pub fn smooth_scale(src: &Canvas, dw: usize, dh: usize, has_alpha: bool) -> Canvas {
    if dw == 0 || dh == 0 || src.w == 0 || src.h == 0 {
        return Canvas::new(dw, dh, [0, 0, 0, 0]);
    }
    let (sw, sh) = (src.w as i64, src.h as i64);
    let (dwi, dhi) = (dw as i64, dh as i64);
    let calc_points = |s: i64, d: i64| -> Vec<usize> {
        let up = d >= s;
        let mut val: i64 = if up { 0x8000 * s / d - 0x8000 } else { 0 };
        let inc = (s << 16) / d;
        (0..d)
            .map(|_| {
                let p = (val >> 16).max(0) as usize;
                val += inc;
                p
            })
            .collect()
    };
    let calc_apoints = |s: i64, d: i64, up: bool| -> Vec<i64> {
        let inc = (s << 16) / d;
        if up {
            let mut val: i64 = 0x8000 * s / d - 0x8000;
            (0..d)
                .map(|_| {
                    let pos = val >> 16;
                    let r = if pos < 0 || pos >= s - 1 {
                        0
                    } else {
                        (val >> 8) - ((val >> 8) & 0xffffff00)
                    };
                    val += inc;
                    r
                })
                .collect()
        } else {
            let mut val: i64 = 0;
            let cp = ((d << 14) + s - 1) / s;
            (0..d)
                .map(|_| {
                    let ap = ((0x10000 - (val & 0xffff)) * cp) >> 16;
                    val += inc;
                    ap | (cp << 16)
                })
                .collect()
        }
    };
    let (xup, yup) = (dwi >= sw, dhi >= sh);
    let xpoints = calc_points(sw, dwi);
    let ypoints = calc_points(sh, dhi);
    let xap = calc_apoints(sw, dwi, xup);
    let yap = calc_apoints(sh, dhi, yup);
    let sow = src.w;
    let px = |i: usize| -> [i64; 4] { src.px[i].map(|v| v as i64) };
    // qt_qimageScaleAARGBA_helper (vector form): weighted run of pixels along `step`.
    let helper = |mut i: usize, xyap: i64, c: i64, step: usize| -> [i64; 4] {
        let p = px(i);
        let mut v = p.map(|k| k * xyap);
        let mut j = (1 << 14) - xyap;
        while j > c {
            i += step;
            let p = px(i);
            for k in 0..4 {
                v[k] += p[k] * c;
            }
            j -= c;
        }
        i += step;
        let p = px(i);
        for k in 0..4 {
            v[k] += p[k] * j;
        }
        v
    };
    let pack = |v: [i64; 4]| -> [u8; 4] {
        let mut o = v.map(|k| k.clamp(0, 255) as u8);
        if !has_alpha && !(xup && yup) {
            o[3] = 255;
        }
        o
    };
    let mut out = Canvas::new(dw, dh, [0, 0, 0, 0]);
    for y in 0..dh {
        let row = ypoints[y] * sow;
        for x in 0..dw {
            let i = row + xpoints[x];
            let o = if xup && yup {
                let (xa, ya) = (xap[x], yap[y]);
                let tl = px(i);
                let tr = if xa > 0 { px(i + 1) } else { tl };
                let bl = if ya > 0 { px(i + sow) } else { tl };
                let br = if xa > 0 && ya > 0 {
                    px(i + sow + 1)
                } else {
                    tl
                };
                let mut o = [0i64; 4];
                for k in 0..4 {
                    o[k] = if ya > 0 && xa > 0 {
                        let l = (tl[k] * (256 - ya) + bl[k] * ya) >> 8;
                        let r = (tr[k] * (256 - ya) + br[k] * ya) >> 8;
                        (l * (256 - xa) + r * xa) >> 8
                    } else if ya > 0 {
                        (tl[k] * (256 - ya) + bl[k] * ya) >> 8
                    } else if xa > 0 {
                        (tl[k] * (256 - xa) + tr[k] * xa) >> 8
                    } else {
                        tl[k]
                    };
                }
                o
            } else if xup {
                // up_x_down_y
                let (cy, ya) = (yap[y] >> 16, yap[y] & 0xffff);
                let mut v = helper(i, ya, cy, sow);
                let xa = xap[x];
                if xa > 0 {
                    let r = helper(i + 1, ya, cy, sow);
                    for k in 0..4 {
                        v[k] = (v[k] * (256 - xa) + r[k] * xa) >> 8;
                    }
                }
                v.map(|k| k >> 14)
            } else if yup {
                // down_x_up_y
                let (cx, xa) = (xap[x] >> 16, xap[x] & 0xffff);
                let mut v = helper(i, xa, cx, 1);
                let ya = yap[y];
                if ya > 0 {
                    let r = helper(i + sow, xa, cx, 1);
                    for k in 0..4 {
                        v[k] = (v[k] * (256 - ya) + r[k] * ya) >> 8;
                    }
                }
                v.map(|k| k >> 14)
            } else {
                // down_xy
                let (cx, xa) = (xap[x] >> 16, xap[x] & 0xffff);
                let (cy, ya) = (yap[y] >> 16, yap[y] & 0xffff);
                let mut s = i;
                let mut r = helper(s, xa, cx, 1).map(|k| (k >> 4) * ya);
                let mut j = (1 << 14) - ya;
                while j > cy {
                    s += sow;
                    let v = helper(s, xa, cx, 1);
                    for k in 0..4 {
                        r[k] += (v[k] >> 4) * cy;
                    }
                    j -= cy;
                }
                s += sow;
                let v = helper(s, xa, cx, 1);
                for k in 0..4 {
                    r[k] += (v[k] >> 4) * j;
                }
                r.map(|k| k >> 24)
            };
            out.px[y * dw + x] = pack(o);
        }
    }
    out
}

/// A pixel as QPainter blends it on a Format_ARGB32 image: 16 bits a channel, premultiplied.
type Px64 = [u32; 4];

/// convertARGB32ToRGBA64PM (the AVX2 one): eight pixels at a time, the last ones of `src`
/// together. Eight clear pixels are zero, eight opaque ones are widened only, among any
/// other eight each is multiplied by its alpha.
fn argb_to_rgba64pm(src: &[[u8; 4]], out: &mut [Px64]) {
    for (src, out) in src.chunks(8).zip(out.chunks_mut(8)) {
        let clear = src.iter().all(|p| p[3] == 0);
        let opaque = src.iter().all(|p| p[3] == 255);
        for (p, o) in src.iter().zip(out) {
            let a = u32::from(p[3]) * 257;
            let mul = |c: u8| {
                let t = (u32::from(c) * 257 * a) >> 16;
                t + (t >> 15)
            };
            *o = match (clear, opaque) {
                (true, _) => [0; 4],
                (_, true) => p.map(|c| u32::from(c) * 257),
                _ => [mul(p[0]), mul(p[1]), mul(p[2]), a],
            };
        }
    }
}

/// storeARGB32FromRGBA64PM (the SSE4 one): back to 8 bits a channel, not premultiplied. It
/// divides by a translucent alpha in single floats, with the reciprocal `rcpps` gives.
fn argb_from_rgba64pm(p: Px64) -> [u8; 4] {
    let div_257 = |v: u32| (((v + 128) - ((v + 128) >> 8)) >> 8) as u8;
    match p[3] {
        0 => [0; 4],
        0xffff => p.map(div_257),
        a => {
            let a = a as f32;
            let ia = rcp(a);
            let ia = ((ia + ia) - ia * (ia * a)) * 255.0;
            let div = |c: u32| (c as f32 * ia).round_ties_even().clamp(0.0, 255.0) as u8;
            [div(p[0]), div(p[1]), div(p[2]), div_257(p[3])]
        }
    }
}

/// `1 / a` as the `rcpps` instruction of this CPU approximates it.
fn rcp(a: f32) -> f32 {
    #[cfg(target_arch = "x86_64")]
    {
        use std::arch::x86_64::{_mm_cvtss_f32, _mm_rcp_ss, _mm_set_ss};
        // SAFETY: every x86-64 CPU has SSE.
        let a = unsafe { _mm_set_ss(a) };
        // SAFETY: as above.
        let r = unsafe { _mm_rcp_ss(a) };
        // SAFETY: as above.
        unsafe { _mm_cvtss_f32(r) }
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        1.0 / a
    }
}

/// QImage::transformed's QPainter path for a Format_ARGB32 image of `ws` x `hs` pixels, not
/// premultiplied (a scale of no less than a half on both axes): the raster engine draws it
/// with SmoothPixmapTransform on an ARGB32 image, which Qt 6.12 does at 16 bits a channel
/// (processTextureSpansGeneric64): fetchTransformedBilinear64_uint32's simple scale,
/// SourceOver on a clear image, storeARGB32FromRGBA64PM. The canvas is premultiplied, as the
/// scaled QImage is when it is shown.
pub fn bilinear_scale_straight(
    px: &[[u8; 4]],
    ws: usize,
    hs: usize,
    dw: usize,
    dh: usize,
) -> Canvas {
    if dw == 0 || dh == 0 || ws == 0 || hs == 0 {
        return Canvas::new(dw, dh, [0, 0, 0, 0]);
    }
    const FS: f64 = 65536.0;
    // BufferSize of qdrawhelper: a span is fetched in runs of it, and blended in halves.
    const RUN: usize = 2048;
    // QSpanData::setupMatrix: the painter's scale, moved by 1/65536 of a pixel, inverted.
    let inv = |d: usize, s: usize| {
        let scale = d as f64 / s as f64;
        let m = 1.0 / scale;
        (m, -((1.0 / FS) * scale) * m)
    };
    let ((m11, dx), (m22, dy)) = (inv(dw, ws), inv(dh, hs));
    let fdx = (m11 * FS) as i32 as i64;
    let bounds = |v: i64, n: usize| match v {
        v if v < 0 => (0, 0),
        v if v >= n as i64 - 1 => (n - 1, n - 1),
        v => (v as usize, v as usize + 1),
    };
    let mut out = Canvas::new(dw, dh, [0, 0, 0, 0]);
    let (mut top, mut bottom) = (vec![[0; 4]; RUN], vec![[0; 4]; RUN]);
    let (mut top64, mut bottom64) = (vec![[0; 4]; RUN], vec![[0; 4]; RUN]);
    for y in 0..dh {
        let fy = ((m22 * (y as f64 + 0.5) + dy) * FS) as i32 as i64 - 32768;
        let (y1, y2) = bounds(fy >> 16, hs);
        let disty = (fy & 0xffff) as u32;
        for x0 in (0..dw).step_by(RUN) {
            let mut fx = ((m11 * (x0 as f64 + 0.5) + dx) * FS) as i32 as i64 - 32768;
            let run = &mut out.px[y * dw + x0..y * dw + dw.min(x0 + RUN)];
            for line in run.chunks_mut(RUN / 2) {
                let mut f = fx;
                for i in 0..line.len() {
                    let (x1, x2) = bounds(f >> 16, ws);
                    top[2 * i] = px[y1 * ws + x1];
                    top[2 * i + 1] = px[y1 * ws + x2];
                    bottom[2 * i] = px[y2 * ws + x1];
                    bottom[2 * i + 1] = px[y2 * ws + x2];
                    f += fdx;
                }
                let n = line.len() * 2;
                argb_to_rgba64pm(&top[..n], &mut top64[..n]);
                if disty != 0 {
                    argb_to_rgba64pm(&bottom[..n], &mut bottom64[..n]);
                }
                for (i, o) in line.iter_mut().enumerate() {
                    let distx = (fx & 0xffff) as u32;
                    let side = |k: usize| -> Px64 {
                        let t = top64[2 * i + k];
                        match disty {
                            0 => t,
                            _ => std::array::from_fn(|c| {
                                ((t[c] * (0x10000 - disty)) >> 16)
                                    + ((bottom64[2 * i + k][c] * disty) >> 16)
                            }),
                        }
                    };
                    let left = side(0);
                    let v = match distx {
                        0 => left,
                        _ => {
                            let right = side(1);
                            std::array::from_fn(|c| {
                                (((left[c] * (0x10000 - distx)) >> 16) + ((right[c] * distx) >> 16))
                                    & 0xffff
                            })
                        }
                    };
                    // comp_func_SourceOver_rgb64 on a clear image: the source, or nothing.
                    *o = premul_rgba8(argb_from_rgba64pm(v));
                    fx += fdx;
                }
            }
        }
    }
    out
}

/// 1/a as SSE's `rcpps` gives it: the approximation is the processor's own, so the very
/// instruction is run. Elsewhere Qt has no such path and this is only close.
fn rcpps(a: f32) -> f32 {
    #[cfg(target_arch = "x86_64")]
    {
        use core::arch::x86_64::{_mm_cvtss_f32, _mm_rcp_ss, _mm_set_ss};
        // SAFETY: SSE is part of x86_64.
        let a = unsafe { _mm_set_ss(a) };
        // SAFETY: as above.
        let r = unsafe { _mm_rcp_ss(a) };
        // SAFETY: as above.
        unsafe { _mm_cvtss_f32(r) }
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        1.0 / a
    }
}

/// QRgba64 → 8 bits (`toArgb32`'s `div_257`).
fn div_257(v: u16) -> u8 {
    let v = v as u32 + 128;
    ((v - (v >> 8)) >> 8) as u8
}

/// convertARGBToRGBA64PM_avx2: straight 8-bit pixels to premultiplied 16-bit ones, eight at a
/// time. Eight opaque ones are widened, any other eight are all multiplied in 16 bits.
fn rgba64pm_avx2(px: &[[u8; 4]], out: &mut Vec<[u16; 4]>) {
    for g in px.chunks(8) {
        let opaque = g.iter().all(|p| p[3] == 255);
        let clear = g.iter().all(|p| p[3] == 0);
        out.extend(g.iter().map(|p| {
            let wide = p.map(|c| c as u16 * 257);
            if opaque {
                return wide;
            }
            if clear {
                return [0; 4];
            }
            let a = wide[3] as u32;
            let m = |c: u16| {
                let v = (c as u32 * a) >> 16;
                (v + (v >> 15)) as u16
            };
            [m(wide[0]), m(wide[1]), m(wide[2]), wide[3]]
        }));
    }
}

/// convertARGBFromRGBA64PM_sse4: premultiplied 16-bit pixels to straight 8-bit ones, four at a
/// time: four opaque ones in integers, any other four through a float reciprocal of alpha; the
/// last one to three as QRgba64::unpremultiplied.
fn argb32_from_rgba64pm_sse4(px: &[[u16; 4]], out: &mut Vec<[u8; 4]>) {
    let (groups, rest) = px.as_chunks::<4>();
    for g in groups {
        let opaque = g.iter().all(|p| p[3] == 0xffff);
        out.extend(g.iter().map(|p| {
            if opaque {
                return p.map(div_257);
            }
            if p[3] == 0 {
                return [0; 4];
            }
            let a = p[3] as f32;
            let mut ia = rcpps(a);
            ia = (ia + ia) - ia * (ia * a);
            ia *= 255.0;
            let c = |c: u16| (c as f32 * ia).round_ties_even().clamp(0.0, 255.0) as u8;
            [c(p[0]), c(p[1]), c(p[2]), div_257(p[3])]
        }));
    }
    out.extend(rest.iter().map(|p| {
        let a = p[3] as u64;
        if a == 0xffff || a == 0 {
            return p.map(div_257);
        }
        let fa = (0xffff_0000_8000 + a / 2) / a;
        let c = |c: u16| div_257(((c as u64 * fa + 0x8000_0000) >> 32) as u16);
        [c(p[0]), c(p[1]), c(p[2]), div_257(p[3])]
    }));
}

/// The coverage QRasterizer::rasterizeLine (antialiased) gives the pixels of a `ws`×`hs` image
/// drawn scaled to `dw`×`dh`: of each column in 16.16 times 255, and the height of each row in
/// 16.16. Whole pixels but for what doubles leave at the edges.
fn argb32_coverage(ws: usize, hs: usize, dw: usize, dh: usize) -> (Vec<i64>, Vec<i64>) {
    const F: i64 = 65536;
    let fix = |x: f64| (x * 65536.0) as i64;
    let (sx, sy) = (dw as f64 / ws as f64, dh as f64 / hs as f64);
    let (ax, ay) = (0.0 * sx, (hs as f64 * 0.5) * sy);
    let bx = ws as f64 * sx;
    let fuzzy = (sx - sy).abs() * 1_000_000_000_000.0 <= sx.min(sy);
    let mut width = if fuzzy {
        hs as f64 / ws as f64
    } else {
        ((sy * hs as f64) / (sx * ws as f64)).abs()
    };
    let (pax, pbx) = (ax.max(0.0), bx.min(dw as f64));
    let (w0, w) = ((ax - bx) * (ax - bx), (pax - pbx) * (pax - pbx));
    width *= (w0 / w).sqrt();
    let (x, dx) = ((pax + pbx) * 0.5, (pbx - pax).abs() * 0.5);
    let dy = width * dx;
    let (pay, pby) = (ay - dy, ay + dy);
    let half = 0.5 * (1.0 / width) * (pby - pay);
    let left = (x - half).clamp(0.0, dw as f64);
    let right = (x + half).clamp(0.0, dw as f64);
    let (pay, pby) = (pay.clamp(0.0, dh as f64), pby.clamp(0.0, dh as f64));
    let (il, ir) = (left as i64, right as i64);
    let lw = ((il + 1) << 16) - fix(left);
    let rw = fix(right) - (ir << 16);
    let mut cols = vec![0i64; dw];
    let mut span = |x: i64, len: i64, c: i64| {
        for x in x.max(0)..(x + len).min(dw as i64) {
            cols[x as usize] = c;
        }
    };
    if il == ir {
        span(
            il,
            1,
            if lw == F {
                rw * 255
            } else {
                (rw + lw - F) * 255
            },
        );
    } else {
        if lw == F {
            span(il, ir - il, lw * 255);
        } else {
            span(il, 1, lw * 255);
            span(il + 1, ir - il - 1, 255 << 16);
        }
        if rw != 0 {
            span(ir, 1, rw * 255);
        }
    }
    let mut rows = vec![0i64; dh];
    let (top, bottom) = ((pay as i64) << 16, (pby as i64) << 16);
    let (ypa, ypb) = (fix(pay), fix(pby));
    let mut yfp = top;
    while yfp <= bottom && ((yfp >> 16) as usize) < dh {
        rows[(yfp >> 16) as usize] = (yfp + F).min(ypb) - yfp.max(ypa);
        yfp += F;
    }
    (cols, rows)
}

/// `QImage::scaled(dw, dh, Qt::IgnoreAspectRatio, Qt::SmoothTransformation)` of a
/// Format_ARGB32 picture no more than halved on a side, made a pixmap: `src` is straight RGBA,
/// what comes back is premultiplied. QImage::transformed paints the picture on an ARGB32 one,
/// and the raster engine does that in 16 bits a channel (`blend_src_generic_rgb64`): the
/// source goes to premultiplied RGBA64, is weighed with 16-bit weights
/// (fetchTransformedBilinear64_uint32) and goes back to straight ARGB32. Modelled as a
/// processor with AVX2 does it.
fn bilinear_scale_argb32(src: &[[u8; 4]], ws: usize, hs: usize, dw: usize, dh: usize) -> Canvas {
    let mut out = Canvas::new(dw, dh, [0, 0, 0, 0]);
    let (sx, sy) = (dw as f64 / ws as f64, dh as f64 / hs as f64);
    let (m11, m22) = (1.0 / sx, 1.0 / sy);
    // QSpanData::setupMatrix: the painter's matrix moved by 1/65536, inverted.
    let (tx, ty) = (-(sx * (1.0 / 65536.0)) * m11, -(sy * (1.0 / 65536.0)) * m22);
    let fdx = (m11 * 65536.0) as i64;
    let (cols, rows) = argb32_coverage(ws, hs, dw, dh);
    let pair = |f: i64, n: usize| match f >> 16 {
        i if i < 0 => (0, 0),
        i if i >= n as i64 - 1 => (n - 1, n - 1),
        i => (i as usize, i as usize + 1),
    };
    let weigh = |a: [u16; 4], b: [u16; 4], d: i64| -> [u16; 4] {
        std::array::from_fn(|k| {
            (((a[k] as i64 * (0x10000 - d)) >> 16) + ((b[k] as i64 * d) >> 16)) as u16
        })
    };
    let (mut s1, mut s2, mut dist) = (Vec::new(), Vec::new(), Vec::new());
    let (mut b1, mut b2) = (Vec::new(), Vec::new());
    let (mut line, mut stored) = (Vec::new(), Vec::new());
    for (y, row) in rows.iter().enumerate() {
        let cover: Vec<u8> = cols
            .iter()
            .map(|c| (((row * c) >> 16) >> 16) as u8)
            .collect();
        let cy = y as f64 + 0.5;
        let mut x = 0;
        while x < dw {
            if cover[x] == 0 {
                x += 1;
                continue;
            }
            // handleSpans: the spans that touch are one run, fetched and stored 2048 at a time.
            let run = cover[x..]
                .iter()
                .take(2048)
                .take_while(|c| **c != 0)
                .count();
            let cx = x as f64 + 0.5;
            let mut fx = ((0.0 * cy + m11 * cx + tx) * 65536.0) as i64 - 0x8000;
            let fy = ((m22 * cy + 0.0 * cx + ty) * 65536.0) as i64 - 0x8000;
            let (y1, y2) = pair(fy, hs);
            let disty = fy & 0xffff;
            line.clear();
            while line.len() < run {
                let len = (run - line.len()).min(1024);
                s1.clear();
                s2.clear();
                dist.clear();
                for _ in 0..len {
                    let (x1, x2) = pair(fx, ws);
                    s1.extend([src[y1 * ws + x1], src[y1 * ws + x2]]);
                    s2.extend([src[y2 * ws + x1], src[y2 * ws + x2]]);
                    dist.push(fx & 0xffff);
                    fx += fdx;
                }
                b1.clear();
                b2.clear();
                rgba64pm_avx2(&s1, &mut b1);
                if disty != 0 {
                    rgba64pm_avx2(&s2, &mut b2);
                }
                for (i, &distx) in dist.iter().enumerate() {
                    let (mut l, mut r) = (b1[2 * i], b1[2 * i + 1]);
                    if disty != 0 {
                        l = weigh(l, b2[2 * i], disty);
                        r = weigh(r, b2[2 * i + 1], disty);
                    }
                    if distx != 0 {
                        l = weigh(l, r, distx);
                    }
                    // comp_func_SourceOver_rgb64 over nothing, by the span's coverage.
                    let c = cover[x + line.len()] as u32;
                    if c != 255 {
                        l = if l[3] == 0 {
                            [0; 4]
                        } else {
                            l.map(|v| {
                                let t = v as u32 * (c * 257);
                                ((t + (t >> 16) + 0x8000) >> 16) as u16
                            })
                        };
                    }
                    line.push(l);
                }
            }
            stored.clear();
            argb32_from_rgba64pm_sse4(&line, &mut stored);
            for (i, p) in stored.iter().enumerate() {
                out.px[y * dw + x + i] = premul_rgba8(*p);
            }
            x += run;
        }
    }
    out
}

/// `QImage::scaled(dw, dh, Qt::IgnoreAspectRatio, Qt::SmoothTransformation)` of a
/// Format_ARGB32 picture, made a pixmap (QPixmap::fromImage): `src` is `ws`×`hs` straight
/// RGBA, what comes back is premultiplied.
pub fn scale_argb32(src: &[[u8; 4]], ws: usize, hs: usize, dw: usize, dh: usize) -> Canvas {
    if dw == 0 || dh == 0 || ws == 0 || hs == 0 {
        return Canvas::new(dw, dh, [0, 0, 0, 0]);
    }
    if (dw, dh) == (ws, hs) {
        return Canvas::from_px(ws, hs, src.iter().map(|p| premul_rgba8(*p)).collect());
    }
    if dh * 2 < hs || dw * 2 < ws || ws * hs >= (1 << 20) {
        let px = src.iter().map(|p| premul_rgba8(*p)).collect();
        return smooth_scale(&Canvas::from_px(ws, hs, px), dw, dh, true);
    }
    bilinear_scale_argb32(src, ws, hs, dw, dh)
}

/// `bilinear_scale_straight` for a premultiplied texture: its colours are divided by alpha
/// first, so what a translucent pixel had below its alpha's precision is not Qt's.
pub fn bilinear_scale(src: &Canvas, dw: usize, dh: usize) -> Canvas {
    let straight: Vec<[u8; 4]> = src
        .px
        .iter()
        .map(|p| match p[3] {
            0 => [0; 4],
            255 => *p,
            a => {
                let c = |v: u8| ((u32::from(v) * 255 + u32::from(a) / 2) / u32::from(a)).min(255);
                [c(p[0]) as u8, c(p[1]) as u8, c(p[2]) as u8, a]
            }
        })
        .collect();
    bilinear_scale_straight(&straight, src.w, src.h, dw, dh)
}

/// QImage::scaled(dw, dh, IgnoreAspectRatio, SmoothTransformation) of a Format_ARGB32 image:
/// `px` are its `ws` x `hs` pixels as the file has them, not premultiplied. The canvas is
/// premultiplied, as the scaled QImage is when it is shown.
pub fn scale_straight(px: &[[u8; 4]], ws: usize, hs: usize, dw: usize, dh: usize) -> Canvas {
    if paints_scale(ws, hs, dw, dh) {
        return bilinear_scale_straight(px, ws, hs, dw, dh);
    }
    let src = Canvas::from_px(ws, hs, px.iter().map(|p| premul_rgba8(*p)).collect());
    smooth_scale(&src, dw, dh, true)
}

/// QImage::transformed scales with a painter, not with qSmoothScaleImage: neither side goes
/// under a half and the image is under the size Qt scales on several threads.
fn paints_scale(ws: usize, hs: usize, dw: usize, dh: usize) -> bool {
    !(dh * 2 < hs || dw * 2 < ws) && ws * hs < (1 << 20)
}

/// QImage::scaled(dw, dh, IgnoreAspectRatio, SmoothTransformation) for a decoded PNG
/// (alpha PNGs decode to straight ARGB32, others to RGB32).
pub fn scale_decoded(src: &Canvas, dw: usize, dh: usize, has_alpha: bool) -> Canvas {
    if has_alpha && paints_scale(src.w, src.h, dw, dh) {
        bilinear_scale(src, dw, dh)
    } else {
        smooth_scale(src, dw, dh, has_alpha)
    }
}
