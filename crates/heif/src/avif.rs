//! AVIF decoding with the system libavif (`libavif.so.16`), loaded on first use with dlopen:
//! the decoder Pillow and OpenCV read AVIF with, set up as each of them sets it up, so the
//! pixels equal theirs (libheif upsamples chroma differently). Neither applies `irot`, `imir`
//! or `clap`, so the image comes as stored. Without the library, or with a release of it
//! whose structs were not compared with the ones here, every call fails with an `Unsupported`
//! error, callers fall back to libheif and the log gets one line saying so.

use std::ffi::{CStr, c_char, c_int};
use std::io::{Error, ErrorKind, Result};
use std::sync::OnceLock;

/// Whose `avifRGBImage` setup to repeat.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Reader {
    /// Pillow's `_avif.c`: mode "RGBA" with alpha, "L" for 4:0:0 (libavif 1.3 and newer), else
    /// "RGB"; always 8 bits.
    Pillow,
    /// OpenCV's `grfmt_avif.cpp` read as 8 bits: the luma plane itself for 4:0:0, else colour
    /// with the alpha the file has.
    OpenCv,
}

/// 8-bit rows, tightly packed: grey (`channels` 1), RGB (3) or RGBA (4, not premultiplied).
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub channels: usize,
    pub data: Vec<u8>,
}

/// An `ftyp` box naming the `avif` or `avis` brand, as major or compatible brand.
pub fn is_avif(data: &[u8]) -> bool {
    if data.len() < 16 || &data[4..8] != b"ftyp" {
        return false;
    }
    let size = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
    let end = size.clamp(16, data.len());
    let compatible = data[16..end].as_chunks::<4>().0.iter().map(|c| &c[..]);
    std::iter::once(&data[8..12])
        .chain(compatible)
        .any(|b| b == b"avif" || b == b"avis")
}

/// Whether libavif could be loaded.
pub fn available() -> bool {
    lib().is_ok()
}

// avif.h of libavif 1.0 (soname 16): the releases up to 1.4.2 only append fields to these
// structs ([`CHECKED`]).

/// The start of `avifImage`.
#[repr(C)]
struct AvifImage {
    width: u32,
    height: u32,
    depth: u32,
    yuv_format: c_int,
    yuv_range: c_int,
    yuv_chroma_sample_position: c_int,
    yuv_planes: [*mut u8; 3],
    yuv_row_bytes: [u32; 3],
    image_owns_yuv_planes: c_int,
    alpha_plane: *mut u8,
}

/// The start of `avifDecoder`.
#[repr(C)]
struct AvifDecoder {
    codec_choice: c_int,
    max_threads: c_int,
    requested_source: c_int,
    allow_progressive: c_int,
    allow_incremental: c_int,
    ignore_exif: c_int,
    ignore_xmp: c_int,
    image_size_limit: u32,
    image_dimension_limit: u32,
    image_count_limit: u32,
    strict_flags: u32,
    image: *mut AvifImage,
}

/// `avifRGBImage`, which the caller allocates.
#[repr(C)]
struct RgbImage {
    width: u32,
    height: u32,
    depth: u32,
    format: c_int,
    chroma_upsampling: c_int,
    chroma_downsampling: c_int,
    avoid_libyuv: c_int,
    ignore_alpha: c_int,
    alpha_premultiplied: c_int,
    is_float: c_int,
    max_threads: c_int,
    pixels: *mut u8,
    row_bytes: u32,
}

/// Room behind [`RgbImage`] for fields a newer libavif 16 may append and
/// `avifRGBImageSetDefaults` then writes.
#[repr(C)]
struct RgbStorage {
    rgb: RgbImage,
    _spare: [u64; 16],
}

const PIXEL_FORMAT_YUV400: c_int = 4;
const RGB_FORMAT_RGB: c_int = 0;
const RGB_FORMAT_RGBA: c_int = 1;
/// `AVIF_RGB_FORMAT_GRAY`, which libavif has since 1.3.0.
const RGB_FORMAT_GRAY: c_int = 7;
bits::bit_flags! {
const STRICT_PIXI_REQUIRED: u32 = 1;
const STRICT_CLAP_VALID: u32 = 2;
}

struct Lib {
    _lib: libloading::Library,
    /// (major, minor) of `avifVersion()`.
    version: (u32, u32),
    decoder_create: unsafe extern "C" fn() -> *mut AvifDecoder,
    decoder_destroy: unsafe extern "C" fn(*mut AvifDecoder),
    set_io_memory: unsafe extern "C" fn(*mut AvifDecoder, *const u8, usize) -> c_int,
    parse: unsafe extern "C" fn(*mut AvifDecoder) -> c_int,
    next_image: unsafe extern "C" fn(*mut AvifDecoder) -> c_int,
    rgb_set_defaults: unsafe extern "C" fn(*mut RgbImage, *const AvifImage),
    yuv_to_rgb: unsafe extern "C" fn(*const AvifImage, *mut RgbImage) -> c_int,
    result_to_string: unsafe extern "C" fn(c_int) -> *const c_char,
}

/// The file name libavif is opened by.
const SONAME: &str = "libavif.so.16";
/// The releases whose `avif.h` was compared with the structs below: the fields read here are in
/// the same places in all of them. A release outside is not used, since reading its structs by
/// this layout would be undefined behaviour if it moved them.
const CHECKED: std::ops::RangeInclusive<(u32, u32, u32)> = (1, 0, 0)..=(1, 4, 2);
const FALLBACK: &str = "AVIF is read with libheif, whose pixels differ from Pillow's and OpenCV's";

/// `avifVersion()`'s "major.minor.patch" when it is one of the [`CHECKED`] releases.
fn checked_version(version: &str) -> Option<(u32, u32, u32)> {
    let mut parts = version.split('.').map(|p| p.parse::<u32>().ok());
    let version = (parts.next()??, parts.next()??, parts.next()??);
    (parts.next().is_none() && CHECKED.contains(&version)).then_some(version)
}

/// Opens libavif at `name` and takes its functions, once its version is a checked one: the
/// error is the line for the log.
fn load(name: &str) -> std::result::Result<Lib, String> {
    // SAFETY: loading libavif runs no initialisers with preconditions.
    let lib = unsafe { libloading::Library::new(name) }
        .map_err(|_| format!("libavif ({SONAME}) is not installed: {FALLBACK}"))?;
    let missing = |_| format!("{name} is not a libavif this can use: {FALLBACK}");
    // SAFETY: `const char *avifVersion(void)` in every libavif.
    let version: unsafe extern "C" fn() -> *const c_char =
        *unsafe { lib.get(c"avifVersion") }.map_err(missing)?;
    // SAFETY: no preconditions.
    let version = unsafe { version() };
    // SAFETY: a static NUL-terminated "major.minor.patch" string.
    let version = unsafe { CStr::from_ptr(version) }.to_string_lossy();
    let Some((major, minor, _)) = checked_version(&version) else {
        let ((a, b, c), (d, e, f)) = CHECKED.into_inner();
        return Err(format!(
            "libavif {version} is not used, only {a}.{b}.{c} to {d}.{e}.{f} were checked: {FALLBACK}"
        ));
    };
    macro_rules! sym {
        ($name:literal) => {
            // SAFETY: the type each symbol is read as is avif.h's C signature for it in
            // every checked release; the library lives in the static next to the copied
            // function pointers, so they never dangle.
            *unsafe { lib.get($name) }.map_err(missing)?
        };
    }
    Ok(Lib {
        version: (major, minor),
        decoder_create: sym!(c"avifDecoderCreate"),
        decoder_destroy: sym!(c"avifDecoderDestroy"),
        set_io_memory: sym!(c"avifDecoderSetIOMemory"),
        parse: sym!(c"avifDecoderParse"),
        next_image: sym!(c"avifDecoderNextImage"),
        rgb_set_defaults: sym!(c"avifRGBImageSetDefaults"),
        yuv_to_rgb: sym!(c"avifImageYUVToRGB"),
        result_to_string: sym!(c"avifResultToString"),
        _lib: lib,
    })
}

fn lib() -> Result<&'static Lib> {
    static LIB: OnceLock<std::result::Result<Lib, String>> = OnceLock::new();
    LIB.get_or_init(|| {
        // Once per process: why its AVIF pixels are libheif's.
        load(SONAME).inspect_err(|why| elog::eprintln!("rusty-impulse: {why}"))
    })
    .as_ref()
    .map_err(|e| Error::new(ErrorKind::Unsupported, e.clone()))
}

fn check(lib: &Lib, result: c_int) -> Result<()> {
    if result == 0 {
        return Ok(());
    }
    // SAFETY: answers every value.
    let msg = unsafe { (lib.result_to_string)(result) };
    // SAFETY: a static NUL-terminated string.
    let msg = unsafe { CStr::from_ptr(msg) }.to_string_lossy();
    Err(Error::new(
        ErrorKind::InvalidData,
        format!("libavif: {msg}"),
    ))
}

struct Decoder(&'static Lib, *mut AvifDecoder);

impl Drop for Decoder {
    fn drop(&mut self) {
        // SAFETY: created by this library, destroyed once.
        unsafe { (self.0.decoder_destroy)(self.1) }
    }
}

/// Decodes the first image of an AVIF file as `reader` does, once
/// `check_size(width, height, bytes per decoded pixel)` accepts the size its header gives.
pub fn decode(
    data: &[u8],
    reader: Reader,
    check_size: impl FnOnce(usize, usize, usize) -> Result<()>,
) -> Result<Image> {
    let lib = lib()?;
    // SAFETY: no preconditions; null when out of memory.
    let dec = unsafe { (lib.decoder_create)() };
    if dec.is_null() {
        return Err(Error::new(ErrorKind::OutOfMemory, "libavif: no decoder"));
    }
    let dec = Decoder(lib, dec);
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    // SAFETY: a live decoder, whose first fields are `AvifDecoder`'s; nothing else refers to
    // it yet.
    let settings = unsafe { &mut *dec.1 };
    settings.max_threads = threads.min(64) as c_int;
    match reader {
        Reader::Pillow => settings.strict_flags &= !(STRICT_CLAP_VALID | STRICT_PIXI_REQUIRED),
        Reader::OpenCv => settings.strict_flags = 0,
    }
    // SAFETY: `data` outlives the decoder, which only reads it.
    check(lib, unsafe {
        (lib.set_io_memory)(dec.1, data.as_ptr(), data.len())
    })?;
    // SAFETY: a live decoder with its input set.
    check(lib, unsafe { (lib.parse)(dec.1) })?;
    let image = |dec: &Decoder| -> Result<&AvifImage> {
        // SAFETY: after a successful parse `image` points to the decoder's image, which lives
        // as long as the decoder and starts with `AvifImage`'s fields.
        let image = unsafe { (*dec.1).image };
        // SAFETY: null or that image.
        unsafe { image.as_ref() }
            .ok_or_else(|| Error::new(ErrorKind::InvalidData, "libavif: no image"))
    };
    let img = image(&dec)?;
    let (w, h) = (img.width as usize, img.height as usize);
    if w == 0 || h == 0 {
        return Err(Error::new(ErrorKind::InvalidData, "libavif: empty image"));
    }
    check_size(w, h, 4)?;
    // SAFETY: a parsed decoder.
    check(lib, unsafe { (lib.next_image)(dec.1) })?;
    let img = image(&dec)?;
    let (alpha, gray) = (
        !img.alpha_plane.is_null(),
        img.yuv_format == PIXEL_FORMAT_YUV400,
    );
    let format = match reader {
        Reader::OpenCv if gray && alpha => {
            // channels_ is 2, which CopyToMat refuses.
            return Err(Error::new(
                ErrorKind::Unsupported,
                "OpenCV reads no grey AVIF with alpha",
            ));
        }
        Reader::OpenCv if gray => return luma(img, w, h),
        _ if alpha => RGB_FORMAT_RGBA,
        Reader::Pillow if gray && lib.version >= (1, 3) => RGB_FORMAT_GRAY,
        _ => RGB_FORMAT_RGB,
    };
    let channels = match format {
        RGB_FORMAT_GRAY => 1,
        RGB_FORMAT_RGBA => 4,
        _ => 3,
    };
    let mut out = vec![0u8; w * h * channels];
    // SAFETY: plain integers and a pointer, for which zero is a valid value.
    let mut store: RgbStorage = unsafe { std::mem::zeroed() };
    // SAFETY: `store` is at least an `avifRGBImage`; `img` is the decoded image.
    unsafe { (lib.rgb_set_defaults)(&mut store.rgb, img) };
    store.rgb.depth = 8;
    store.rgb.format = format;
    store.rgb.pixels = out.as_mut_ptr();
    store.rgb.row_bytes = (w * channels) as u32;
    // SAFETY: `pixels` holds `height` rows of `row_bytes` bytes, which is what libavif writes
    // for an 8-bit image of `channels` channels and the size `rgb_set_defaults` copied.
    check(lib, unsafe { (lib.yuv_to_rgb)(img, &mut store.rgb) })?;
    Ok(Image {
        width: w,
        height: h,
        channels,
        data: out,
    })
}

/// OpenCV's single-channel read: the luma plane as stored, deeper samples divided by
/// `1 << (depth - 8)` and rounded like `cvRound`.
fn luma(img: &AvifImage, w: usize, h: usize) -> Result<Image> {
    let (plane, stride) = (img.yuv_planes[0], img.yuv_row_bytes[0] as usize);
    let wide = img.depth > 8;
    let row = w * if wide { 2 } else { 1 };
    if plane.is_null() || stride < row {
        return Err(Error::new(ErrorKind::InvalidData, "libavif: no luma plane"));
    }
    // SAFETY: the plane holds `h` rows `stride` bytes apart, each at least `row` bytes long,
    // and lives as long as the decoder that `img` borrows from.
    let bytes = unsafe { std::slice::from_raw_parts(plane, stride * (h - 1) + row) };
    let rows = (0..h).map(|y| &bytes[y * stride..y * stride + row]);
    let data = if wide {
        let scale = 1.0 / f64::from(1u32 << (img.depth - 8));
        rows.flat_map(|r| r.as_chunks::<2>().0)
            .map(|b| {
                let v = f64::from(u16::from_ne_bytes(*b)) * scale;
                v.round_ties_even().clamp(0.0, 255.0) as u8
            })
            .collect()
    } else {
        rows.flatten().copied().collect()
    };
    Ok(Image {
        width: w,
        height: h,
        channels: 1,
        data,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_checked_releases_are_used() {
        for ok in ["1.0.0", "1.0.4", "1.3.0", "1.4.2"] {
            assert!(checked_version(ok).is_some(), "{ok}");
        }
        for not in [
            "0.11.1", "1.4.3", "1.5.0", "2.0.0", "1.4", "1.4.2.1", "1.x.0", "",
        ] {
            assert_eq!(checked_version(not), None, "{not:?}");
        }
    }

    /// A shared library named as libavif is, whose `avifVersion()` answers `version`; `None`
    /// (and the test failed, see `testenv::equiv_missing`) without a C compiler.
    fn fake_libavif(dir: &std::path::Path, version: &str) -> Option<std::path::PathBuf> {
        if !testenv::require("cc") {
            return None;
        }
        let (src, so) = (dir.join("avif.c"), dir.join(SONAME));
        let body = format!("const char *avifVersion(void) {{ return \"{version}\"; }}\n");
        std::fs::write(&src, body).unwrap();
        let cc = std::process::Command::new("cc")
            .args(["-shared", "-fPIC", "-o"])
            .args([&so, &src])
            .status()
            .unwrap();
        assert!(cc.success(), "cc -shared");
        Some(so)
    }

    #[test]
    fn a_release_that_was_not_checked_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let Some(so) = fake_libavif(dir.path(), "9.9.9") else {
            return;
        };
        // Refused by its version alone: the library has none of the decoder's functions, and
        // none is looked up.
        let why = load(so.to_str().unwrap()).err().expect("refused");
        assert_eq!(
            why,
            "libavif 9.9.9 is not used, only 1.0.0 to 1.4.2 were checked: AVIF is read with \
             libheif, whose pixels differ from Pillow's and OpenCV's"
        );
        let why = load("/nonexistent/libavif.so.16").err().expect("missing");
        assert_eq!(
            why,
            "libavif (libavif.so.16) is not installed: AVIF is read with libheif, whose pixels \
             differ from Pillow's and OpenCV's"
        );
    }

    /// With such a library first on the loader's path nothing is decoded with it, and the log
    /// gets one line, however often AVIF is asked for.
    #[test]
    fn a_refused_library_is_one_line_in_the_log() {
        const TEST: &str = concat!(module_path!(), "::a_refused_library_is_one_line_in_the_log");
        if testenv::in_own_process(TEST) {
            assert!(!available());
            assert!(!available());
            let e = decode(b"", Reader::Pillow, |_, _, _| Ok(())).unwrap_err();
            assert_eq!(e.kind(), ErrorKind::Unsupported);
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        if fake_libavif(dir.path(), "9.9.9").is_none() {
            return;
        }
        let out = testenv::own_process_command(TEST)
            .arg("--nocapture")
            .env("LD_LIBRARY_PATH", dir.path())
            .output()
            .unwrap();
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{err}");
        let lines: Vec<&str> = err.lines().filter(|l| l.contains("libavif")).collect();
        assert_eq!(
            lines,
            [
                "rusty-impulse: libavif 9.9.9 is not used, only 1.0.0 to 1.4.2 were checked: AVIF is read \
              with libheif, whose pixels differ from Pillow's and OpenCV's"
            ],
            "{err}"
        );
    }
}
