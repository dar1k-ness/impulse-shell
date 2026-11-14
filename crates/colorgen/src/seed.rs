//! Image -> source color, exactly as generate_colors_material.py picks it: Pillow decode
//! (libjpeg-turbo for JPEG, GIF frame 1), BICUBIC downscale to ~128x128 pixels, Celebi
//! quantizer, `Score.score(...)[0]`.

use crate::{pillow, quantize};
use material_colors::color::Argb;
use material_colors::score::Score;
use std::io::Cursor;

type Map = indexmap::IndexMap<Argb, u32, core::hash::BuildHasherDefault<ahash::AHasher>>;

#[derive(Debug)]
pub struct SeedError(pub String);

/// Decoded pixels in the Pillow mode the script resizes in: 3 = RGB, 4 = RGBA.
pub struct Pixels {
    pub data: Vec<u8>,
    pub channels: usize,
    pub width: usize,
    pub height: usize,
}

fn err(e: impl std::fmt::Display) -> SeedError {
    SeedError(e.to_string())
}

/// The frames of a GIF whose canvas, which every frame is composed on, is within the decode
/// limit.
pub(crate) fn gif_frames(bytes: &[u8]) -> Result<image::Frames<'_>, SeedError> {
    use image::{AnimationDecoder, ImageDecoder};
    let dec = image::codecs::gif::GifDecoder::new(Cursor::new(bytes)).map_err(err)?;
    let (w, h) = dec.dimensions();
    imgtools::limit::check_size(w as usize, h as usize, 4).map_err(err)?;
    Ok(dec.into_frames())
}

pub fn decode(bytes: &[u8]) -> Result<Pixels, SeedError> {
    if bytes.starts_with(&[0xFF, 0xD8]) {
        if let Some(img) = imgtools::limit::jpeg_cmyk(bytes).map_err(err)? {
            // Pillow reads mode "CMYK" (rawmode "CMYK;I": the stored samples inverted), which
            // the script neither converts nor premultiplies: it resizes the four bands one by
            // one and QuantizeCelebi takes the first three of each pixel, C, M and Y, as R, G
            // and B. K changes nothing, so it is dropped here.
            let mut data = Vec::with_capacity(img.width * img.height * 3);
            for row in img.pixels.chunks(img.pitch) {
                for p in row[..img.width * 4].as_chunks::<4>().0 {
                    data.extend([255 - p[0], 255 - p[1], 255 - p[2]]);
                }
            }
            return Ok(Pixels {
                data,
                channels: 3,
                width: img.width,
                height: img.height,
            });
        }
        let img = imgtools::limit::jpeg(bytes, turbojpeg::PixelFormat::RGB).map_err(err)?;
        let mut data = Vec::with_capacity(img.width * img.height * 3);
        for row in img.pixels.chunks(img.pitch) {
            data.extend_from_slice(&row[..img.width * 3]);
        }
        return Ok(Pixels {
            data,
            channels: 3,
            width: img.width,
            height: img.height,
        });
    }
    if let Some(a) = imgtools::limit::avif(bytes, heif::avif::Reader::Pillow) {
        // Pillow's own AVIF plugin (libavif); mode "L" is converted to "RGB" by the script.
        return Ok(Pixels {
            channels: a.channels.max(3),
            width: a.width,
            height: a.height,
            data: match a.channels {
                1 => a.data.iter().flat_map(|&v| [v; 3]).collect(),
                _ => a.data,
            },
        });
    }
    if heif::is_heif(bytes) {
        // HEIC, which Pillow does not read, and AVIF without libavif.
        let d = imgtools::limit::heif(bytes, false).map_err(err)?;
        let (width, height, alpha) = (d.width, d.height, d.alpha);
        return Ok(Pixels {
            data: if alpha { d.into_rgba8() } else { d.into_u8() },
            channels: 3 + alpha as usize,
            width,
            height,
        });
    }
    if bytes.starts_with(b"GIF8") {
        // `image.seek(1)`: a single-frame GIF raises EOFError in python.
        let frame = gif_frames(bytes)?
            .nth(1)
            .ok_or_else(|| err("GIF has no frame 1"))?
            .map_err(err)?;
        let buf = frame.into_buffer();
        let (w, h) = (buf.width() as usize, buf.height() as usize);
        let raw = buf.into_raw();
        if raw.as_chunks::<4>().0.iter().all(|p| p[3] == 255) {
            let data = raw
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|p| [p[0], p[1], p[2]])
                .collect();
            return Ok(Pixels {
                data,
                channels: 3,
                width: w,
                height: h,
            });
        }
        return Ok(Pixels {
            data: raw,
            channels: 4,
            width: w,
            height: h,
        });
    }
    let img = imgtools::limit::image(bytes).map_err(err)?;
    let (w, h) = (img.width() as usize, img.height() as usize);
    let png = bytes.starts_with(b"\x89PNG\r\n\x1a\n");
    // Pillow keeps a PNG's `tRNS` chunk aside (`info["transparency"]`): a palette, grey or RGB
    // PNG stays without alpha, and the script's `convert('RGB')` of mode P drops it. Only the
    // colour types with an alpha channel (IHDR colour type 4 and 6) give alpha.
    let alpha = img.color().has_alpha() && !(png && bytes.get(25).is_some_and(|t| t & 4 == 0));
    let channels = if alpha { 4 } else { 3 };
    // Pillow reads 16-bit RGB(A) samples of PNG and TIFF into its 8-bit modes by their high
    // byte (raw modes `RGB;16B`, `RGBA;16L`, ...); `image` would round them.
    let sixteen = img.color().bits_per_pixel() / img.color().channel_count() as u16 == 16;
    let mut data = match (sixteen, alpha) {
        (true, true) => high_bytes(img.to_rgba16().into_raw()),
        (true, false) => high_bytes(img.to_rgb16().into_raw()),
        (false, true) => img.to_rgba8().into_raw(),
        (false, false) => img.to_rgb8().into_raw(),
    };
    let tiff = imgtools::cv::is_tiff(bytes);
    if tiff && alpha && imgtools::cv::tiff_extra_samples(bytes) == Some(1) {
        // Associated alpha: Pillow's raw mode `RGBa` divides the colour by alpha again.
        for p in data.as_chunks_mut::<4>().0 {
            let a = p[3] as u32;
            for c in &mut p[..3] {
                *c = match a {
                    0 => 0,
                    255 => *c,
                    _ => (*c as u32 * 255 / a).min(255) as u8,
                };
            }
        }
    }
    // Pillow turns a TIFF by its Orientation tag when it loads it (TiffImagePlugin); for the
    // other formats the script never asks for the EXIF turn.
    let o = if tiff {
        imgtools::cv::orientation(bytes)
    } else {
        1
    };
    let (width, height, data) = imgtools::cv::orient(w, h, channels, data, o);
    Ok(Pixels {
        data,
        channels,
        width,
        height,
    })
}

fn high_bytes(v: Vec<u16>) -> Vec<u8> {
    v.into_iter().map(|v| (v >> 8) as u8).collect()
}

/// `QuantizeCelebi(list(image.getdata()), 128)` after the script's resize.
pub fn quantized(px: &Pixels, bitmap_size: usize) -> Vec<(u32, u32)> {
    let (nw, nh) = pillow::optimal_size(px.width, px.height, bitmap_size);
    let data = if nw < px.width || nh < px.height {
        pillow::resize_bicubic(&px.data, px.channels, px.width, px.height, nw, nh)
    } else {
        px.data.clone()
    };
    quantize::celebi(data.chunks_exact(px.channels), 128)
}

pub fn seed_from_bytes(bytes: &[u8]) -> Result<Argb, SeedError> {
    let px = decode(bytes)?;
    let q = quantized(&px, 128);
    let map: Map = q.into_iter().map(|(a, n)| (Argb::from_u32(a), n)).collect();
    Score::score(&map, None, None, None)
        .first()
        .copied()
        .ok_or_else(|| err("no colors"))
}

pub fn seed_from_path(path: &std::path::Path) -> Result<Argb, SeedError> {
    seed_from_bytes(&std::fs::read(path).map_err(err)?)
}
