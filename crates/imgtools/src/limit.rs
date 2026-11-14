//! Decode size limit, checked from the file header before any pixel buffer is allocated, so a
//! tiny file claiming a huge size (a PNG with a 65535x65535 IHDR) is refused instead of aborting
//! the shell on a failed allocation. The original ran magick/python in a separate process; Qt refuses such
//! images through QImageReader::allocationLimit().

use std::io::{Error, ErrorKind, Result};

/// QImageReader::allocationLimit()'s default (as `qsg::image::MAX_DECODED_BYTES`).
pub const MAX_DECODED_BYTES: usize = 256 << 20;

/// `Err` (`OutOfMemory`) when a `w`x`h` image decoded at `bytes_per_pixel` (never counted below
/// Qt's 32-bit pixels) is over [`MAX_DECODED_BYTES`].
pub fn check_size(w: usize, h: usize, bytes_per_pixel: usize) -> Result<()> {
    if w.checked_mul(h)
        .and_then(|n| n.checked_mul(bytes_per_pixel.max(4)))
        .is_some_and(|n| n <= MAX_DECODED_BYTES)
    {
        return Ok(());
    }
    Err(Error::new(
        ErrorKind::OutOfMemory,
        format!("{w}x{h} image is over the {MAX_DECODED_BYTES}-byte decode limit"),
    ))
}

/// [`check_size`] for a PNG reader's output frame, whose buffer (16-bit samples included) must
/// also fit the limit; returns that buffer's size.
pub fn png_buffer<R: std::io::BufRead + std::io::Seek>(r: &png::Reader<R>) -> Result<usize> {
    let (w, h) = r.info().size();
    check_size(w as usize, h as usize, 4)?;
    match r.output_buffer_size() {
        Some(n) if n <= MAX_DECODED_BYTES => Ok(n),
        _ => Err(Error::new(
            ErrorKind::OutOfMemory,
            format!("{w}x{h} PNG frame is over the {MAX_DECODED_BYTES}-byte decode limit"),
        )),
    }
}

/// `turbojpeg::decompress` after [`check_size`] on the JPEG header.
pub fn jpeg(data: &[u8], format: turbojpeg::PixelFormat) -> Result<turbojpeg::Image<Vec<u8>>> {
    let hdr = turbojpeg::read_header(data).map_err(Error::other)?;
    check_size(hdr.width, hdr.height, format.size())?;
    turbojpeg::decompress(data, format).map_err(Error::other)
}

/// The samples of a four-component JPEG (CMYK, or YCCK converted to it) as libjpeg's `JCS_CMYK`
/// gives them, after [`check_size`]; `None` for any other JPEG. TurboJPEG converts no such file
/// to RGB, so each reader's own conversion starts here. Adobe writes the samples inverted (255 is
/// no ink) and every reader takes them as such, with or without the Adobe marker.
pub fn jpeg_cmyk(data: &[u8]) -> Result<Option<turbojpeg::Image<Vec<u8>>>> {
    use turbojpeg::Colorspace::{CMYK, YCCK};
    let hdr = turbojpeg::read_header(data).map_err(Error::other)?;
    if !matches!(hdr.colorspace, CMYK | YCCK) {
        return Ok(None);
    }
    check_size(hdr.width, hdr.height, 4)?;
    turbojpeg::decompress(data, turbojpeg::PixelFormat::CMYK)
        .map(Some)
        .map_err(Error::other)
}

/// `heif::decode` after [`check_size`] on the primary image's size in the file's metadata.
pub fn heif(data: &[u8], high_depth: bool) -> Result<heif::Decoded> {
    heif::decode_checked(data, high_depth, check_size)
}

/// An AVIF file's pixels as `reader` (Pillow or OpenCV) gets them from libavif, after
/// [`check_size`]; `None` when `data` is not AVIF or libavif is missing or refuses it, and the
/// caller reads the file through libheif ([`heif`]) instead.
pub fn avif(data: &[u8], reader: heif::avif::Reader) -> Option<heif::avif::Image> {
    if !heif::avif::is_avif(data) {
        return None;
    }
    heif::avif::decode(data, reader, check_size).ok()
}

/// `image::load_from_memory` after [`check_size`] on the header, with the decoder's own
/// allocations capped at [`MAX_DECODED_BYTES`].
pub fn image(data: &[u8]) -> image::ImageResult<image::DynamicImage> {
    decode(|| image::ImageReader::new(std::io::Cursor::new(data)).with_guessed_format())
}

/// `image::open` under the limits of [`image`]: the format comes from the content, or from the
/// file name for a format without a signature (TGA).
pub fn image_file(path: &std::path::Path) -> image::ImageResult<image::DynamicImage> {
    decode(|| image::ImageReader::open(path)?.with_guessed_format())
}

fn decode<R: std::io::BufRead + std::io::Seek>(
    reader: impl Fn() -> Result<image::ImageReader<R>>,
) -> image::ImageResult<image::DynamicImage> {
    let (w, h) = reader()?.into_dimensions()?;
    check_size(w as usize, h as usize, 4)?;
    let mut r = reader()?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(MAX_DECODED_BYTES as u64);
    r.limits(limits);
    r.decode()
}
