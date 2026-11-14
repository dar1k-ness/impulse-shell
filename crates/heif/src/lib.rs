//! AVIF and HEIC (HEIF) decoding with the system libheif, loaded on first use with dlopen.
//! Without `libheif.so.1` every call fails with an `Unsupported` error.

pub mod avif;

use std::ffi::{CString, c_char, c_int, c_void};
use std::io::{Error, ErrorKind, Result};
use std::sync::OnceLock;

/// An ISOBMFF `ftyp` box naming a HEIF brand (AVIF, HEIC and the generic MIAF/HEIF image ones)
/// as major or compatible brand, as libheif's `heif_check_filetype` accepts.
pub fn is_heif(data: &[u8]) -> bool {
    const BRANDS: [&[u8; 4]; 12] = [
        b"avif", b"avis", b"heic", b"heix", b"heim", b"heis", b"hevc", b"hevx", b"hevm", b"hevs",
        b"mif1", b"msf1",
    ];
    if data.len() < 16 || &data[4..8] != b"ftyp" {
        return false;
    }
    let size = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
    let end = size.clamp(16, data.len());
    let compatible = data[16..end].as_chunks::<4>().0.iter().map(|c| &c[..]);
    std::iter::once(&data[8..12])
        .chain(compatible)
        .any(|b| BRANDS.iter().any(|k| &k[..] == b))
}

/// The primary image after its transformations (`clap`, `irot`, `imir`), interleaved RGB or
/// RGBA (`alpha`), rows tightly packed.
#[derive(Clone, Debug, PartialEq)]
pub struct Decoded {
    pub width: usize,
    pub height: usize,
    pub alpha: bool,
    /// The file stores colour premultiplied by alpha (`prem` reference); libheif returns it as
    /// stored.
    pub premultiplied: bool,
    /// 8, or the luma bit depth when decoded with `high_depth` (`U16` samples, not rescaled).
    pub bits: u32,
    pub samples: Samples,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Samples {
    U8(Vec<u8>),
    U16(Vec<u16>),
}

impl Decoded {
    pub fn channels(&self) -> usize {
        3 + self.alpha as usize
    }

    /// Samples as 8 bits (deeper ones keep their top 8 bits).
    pub fn into_u8(self) -> Vec<u8> {
        match self.samples {
            Samples::U8(v) => v,
            Samples::U16(v) => {
                let shift = self.bits.saturating_sub(8);
                v.into_iter().map(|s| (s >> shift).min(255) as u8).collect()
            }
        }
    }

    /// Straight RGBA8 rows, unpremultiplying a premultiplied file.
    pub fn into_rgba8(self) -> Vec<u8> {
        let (alpha, prem) = (self.alpha, self.premultiplied);
        let px = self.into_u8();
        if !alpha {
            return px
                .as_chunks::<3>()
                .0
                .iter()
                .flat_map(|p| [p[0], p[1], p[2], 255])
                .collect();
        }
        let mut px = px;
        if prem {
            for p in px.as_chunks_mut::<4>().0 {
                let a = p[3] as u32;
                for c in &mut p[..3] {
                    *c = (*c as u32 * 255 + a / 2)
                        .checked_div(a)
                        .unwrap_or(0)
                        .min(255) as u8;
                }
            }
        }
        px
    }
}

#[repr(C)]
struct HeifError {
    code: c_int,
    subcode: c_int,
    message: *const c_char,
}

/// heif.h's `heif_decoding_options` up to its version-2 member (libheif 1.7). libheif appends
/// members in later struct versions and `heif_decoding_options_alloc` returns its own (newest)
/// version, so only members of `version` <= the returned one may be written.
#[repr(C)]
struct DecodingOptions {
    version: u8,
    ignore_transformations: u8,
    start_progress: *const c_void,
    on_progress: *const c_void,
    end_progress: *const c_void,
    progress_user_data: *mut c_void,
    convert_hdr_to_8bit: u8,
}

type Ctx = *mut c_void;
type Handle = *mut c_void;
type Image = *mut c_void;

struct Lib {
    _lib: libloading::Library,
    context_alloc: unsafe extern "C" fn() -> Ctx,
    context_free: unsafe extern "C" fn(Ctx),
    read_from_file: unsafe extern "C" fn(Ctx, *const c_char, *const c_void) -> HeifError,
    read_from_memory: unsafe extern "C" fn(Ctx, *const c_void, usize, *const c_void) -> HeifError,
    primary_handle: unsafe extern "C" fn(Ctx, *mut Handle) -> HeifError,
    handle_release: unsafe extern "C" fn(Handle),
    handle_width: unsafe extern "C" fn(Handle) -> c_int,
    handle_height: unsafe extern "C" fn(Handle) -> c_int,
    handle_has_alpha: unsafe extern "C" fn(Handle) -> c_int,
    handle_luma_bits: unsafe extern "C" fn(Handle) -> c_int,
    handle_premultiplied: Option<unsafe extern "C" fn(Handle) -> c_int>,
    options_alloc: unsafe extern "C" fn() -> *mut DecodingOptions,
    options_free: unsafe extern "C" fn(*mut DecodingOptions),
    decode_image:
        unsafe extern "C" fn(Handle, *mut Image, c_int, c_int, *const DecodingOptions) -> HeifError,
    image_release: unsafe extern "C" fn(Image),
    image_width: unsafe extern "C" fn(Image, c_int) -> c_int,
    image_height: unsafe extern "C" fn(Image, c_int) -> c_int,
    plane_readonly: unsafe extern "C" fn(Image, c_int, *mut c_int) -> *const u8,
}

const COLORSPACE_RGB: c_int = 1;
const CHROMA_RGB: c_int = 10;
const CHROMA_RGBA: c_int = 11;
const CHROMA_RRGGBB_LE: c_int = 14;
const CHROMA_RRGGBBAA_LE: c_int = 15;
const CHANNEL_INTERLEAVED: c_int = 10;

/// The oldest libheif whose ABI `Lib` and `DecodingOptions` match: `convert_hdr_to_8bit`
/// (version-2 decoding options) appeared in 1.7.0.
const MIN_VERSION: (c_int, c_int) = (1, 7);

fn lib() -> Result<&'static Lib> {
    static LIB: OnceLock<std::result::Result<Lib, String>> = OnceLock::new();
    LIB.get_or_init(|| {
        let missing = |_| "AVIF/HEIC need libheif (libheif.so.1), which is not available";
        // SAFETY: loading libheif runs no initialisers with preconditions.
        let lib = unsafe { libloading::Library::new("libheif.so.1") }.map_err(missing)?;
        macro_rules! sym {
            ($name:literal) => {
                // SAFETY: the type each symbol is read as is heif.h's C signature for it (stable
                // C ABI since 1.0); the library lives in the static next to the copied function
                // pointers, so they never dangle.
                *unsafe { lib.get($name) }.map_err(missing)?
            };
        }
        let major: unsafe extern "C" fn() -> c_int = sym!(c"heif_get_version_number_major");
        let minor: unsafe extern "C" fn() -> c_int = sym!(c"heif_get_version_number_minor");
        // SAFETY: `int heif_get_version_number_major(void)`: no preconditions.
        let major = unsafe { major() };
        // SAFETY: `int heif_get_version_number_minor(void)`: no preconditions.
        let minor = unsafe { minor() };
        if (major, minor) < MIN_VERSION {
            return Err(format!(
                "AVIF/HEIC need libheif {}.{} or newer, found {major}.{minor}",
                MIN_VERSION.0, MIN_VERSION.1
            ));
        }
        Ok(Lib {
            context_alloc: sym!(c"heif_context_alloc"),
            context_free: sym!(c"heif_context_free"),
            read_from_file: sym!(c"heif_context_read_from_file"),
            read_from_memory: sym!(c"heif_context_read_from_memory_without_copy"),
            primary_handle: sym!(c"heif_context_get_primary_image_handle"),
            handle_release: sym!(c"heif_image_handle_release"),
            handle_width: sym!(c"heif_image_handle_get_width"),
            handle_height: sym!(c"heif_image_handle_get_height"),
            handle_has_alpha: sym!(c"heif_image_handle_has_alpha_channel"),
            handle_luma_bits: sym!(c"heif_image_handle_get_luma_bits_per_pixel"),
            // SAFETY: as in `sym!`; `heif_image_handle_is_premultiplied_alpha` is libheif 1.12+.
            handle_premultiplied: unsafe { lib.get(c"heif_image_handle_is_premultiplied_alpha") }
                .ok()
                .map(|s| *s),
            options_alloc: sym!(c"heif_decoding_options_alloc"),
            options_free: sym!(c"heif_decoding_options_free"),
            decode_image: sym!(c"heif_decode_image"),
            image_release: sym!(c"heif_image_release"),
            image_width: sym!(c"heif_image_get_width"),
            image_height: sym!(c"heif_image_get_height"),
            plane_readonly: sym!(c"heif_image_get_plane_readonly"),
            _lib: lib,
        })
    })
    .as_ref()
    .map_err(|e| Error::new(ErrorKind::Unsupported, e.clone()))
}

fn check(e: HeifError) -> Result<()> {
    if e.code == 0 {
        return Ok(());
    }
    let msg = if e.message.is_null() {
        String::new()
    } else {
        // SAFETY: libheif's messages are static NUL-terminated strings.
        unsafe { std::ffi::CStr::from_ptr(e.message) }
            .to_string_lossy()
            .into_owned()
    };
    Err(Error::new(
        ErrorKind::InvalidData,
        format!("libheif: {msg} ({}.{})", e.code, e.subcode),
    ))
}

/// A context with its primary image handle, both freed on drop.
struct Primary<'a> {
    lib: &'static Lib,
    ctx: Ctx,
    handle: Handle,
    _data: std::marker::PhantomData<&'a [u8]>,
}

impl Drop for Primary<'_> {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            // SAFETY: a handle from `primary_handle`, released once.
            unsafe { (self.lib.handle_release)(self.handle) };
        }
        // SAFETY: a context from `context_alloc`, freed once, after its handle.
        unsafe { (self.lib.context_free)(self.ctx) };
    }
}

impl<'a> Primary<'a> {
    fn open(read: impl FnOnce(&'static Lib, Ctx) -> HeifError) -> Result<Self> {
        let lib = lib()?;
        // SAFETY: `heif_context_alloc` has no preconditions; `Primary` frees the context (and
        // the handle) whatever fails later.
        let ctx = unsafe { (lib.context_alloc)() };
        if ctx.is_null() {
            return Err(Error::other("libheif: cannot allocate a context"));
        }
        let mut p = Primary {
            lib,
            ctx,
            handle: std::ptr::null_mut(),
            _data: std::marker::PhantomData,
        };
        check(read(lib, ctx))?;
        // SAFETY: a live context that has read a file; `p.handle` is a valid out pointer.
        check(unsafe { (lib.primary_handle)(ctx, &mut p.handle) })?;
        if p.handle.is_null() {
            return Err(Error::new(
                ErrorKind::InvalidData,
                "libheif: no primary image",
            ));
        }
        Ok(p)
    }

    fn from_bytes(data: &'a [u8]) -> Result<Self> {
        // SAFETY: `data` outlives the context (the `'a` of `Primary`), which reads it in place.
        Self::open(|lib, ctx| unsafe {
            (lib.read_from_memory)(ctx, data.as_ptr().cast(), data.len(), std::ptr::null())
        })
    }

    fn size(&self) -> (usize, usize) {
        // SAFETY: a live handle.
        let w = unsafe { (self.lib.handle_width)(self.handle) };
        // SAFETY: a live handle.
        let h = unsafe { (self.lib.handle_height)(self.handle) };
        (w.max(0) as usize, h.max(0) as usize)
    }

    /// Bytes one decoded pixel takes in [`Decoded::samples`].
    fn bytes_per_pixel(&self, high_depth: bool) -> usize {
        // SAFETY: a live handle.
        let alpha = unsafe { (self.lib.handle_has_alpha)(self.handle) } != 0;
        // SAFETY: a live handle.
        let luma = unsafe { (self.lib.handle_luma_bits)(self.handle) };
        (3 + alpha as usize) * if luma > 8 && high_depth { 2 } else { 1 }
    }

    fn decode(&self, high_depth: bool) -> Result<Decoded> {
        let lib = self.lib;
        // SAFETY: a live handle.
        let alpha = unsafe { (lib.handle_has_alpha)(self.handle) } != 0;
        let premultiplied = alpha
            && lib
                .handle_premultiplied
                // SAFETY: a live handle.
                .is_some_and(|f| unsafe { f(self.handle) } != 0);
        // SAFETY: a live handle.
        let luma = unsafe { (lib.handle_luma_bits)(self.handle) };
        let deep = luma > 8;
        let wide = deep && high_depth;
        let chroma = match (alpha, wide) {
            (false, false) => CHROMA_RGB,
            (true, false) => CHROMA_RGBA,
            (false, true) => CHROMA_RRGGBB_LE,
            (true, true) => CHROMA_RRGGBBAA_LE,
        };
        // SAFETY: `heif_decoding_options_alloc` has no preconditions.
        let opts = unsafe { (lib.options_alloc)() };
        // SAFETY: null or libheif's own options struct, exclusively ours until freed below;
        // only members of `version` <= the struct's own are touched.
        if let Some(o) = unsafe { opts.as_mut() }
            && o.version >= 2
            && deep
            && !wide
        {
            o.convert_hdr_to_8bit = 1;
        }
        let mut img: Image = std::ptr::null_mut();
        // SAFETY: a live handle, a valid out pointer, and null or allocated options.
        let r = unsafe { (lib.decode_image)(self.handle, &mut img, COLORSPACE_RGB, chroma, opts) };
        if !opts.is_null() {
            // SAFETY: allocated above, freed once.
            unsafe { (lib.options_free)(opts) };
        }
        check(r)?;
        if img.is_null() {
            return Err(Error::new(
                ErrorKind::InvalidData,
                "libheif: no image decoded",
            ));
        }
        struct Release(&'static Lib, Image);
        impl Drop for Release {
            fn drop(&mut self) {
                // SAFETY: decoded by this library, released once.
                unsafe { (self.0.image_release)(self.1) }
            }
        }
        let _img = Release(lib, img);
        // SAFETY: a live decoded image.
        let w = unsafe { (lib.image_width)(img, CHANNEL_INTERLEAVED) };
        // SAFETY: a live decoded image.
        let h = unsafe { (lib.image_height)(img, CHANNEL_INTERLEAVED) };
        let mut stride: c_int = 0;
        // SAFETY: a live decoded image; `stride` is a valid out pointer.
        let plane = unsafe { (lib.plane_readonly)(img, CHANNEL_INTERLEAVED, &mut stride) };
        if plane.is_null() || w <= 0 || h <= 0 {
            return Err(Error::new(ErrorKind::InvalidData, "libheif: empty image"));
        }
        let (w, h) = (w as usize, h as usize);
        let ch = 3 + alpha as usize;
        let row = w * ch * if wide { 2 } else { 1 };
        let stride = stride as usize;
        if stride < row {
            return Err(Error::new(ErrorKind::InvalidData, "libheif: bad stride"));
        }
        // SAFETY: the plane holds `h` rows `stride` bytes apart, each at least `row` bytes
        // long, and lives (unchanged) as long as `_img`.
        let bytes = unsafe { std::slice::from_raw_parts(plane, stride * (h - 1) + row) };
        let rows = (0..h).map(|y| &bytes[y * stride..y * stride + row]);
        let samples = if wide {
            Samples::U16(
                rows.flat_map(|r| r.as_chunks::<2>().0.iter().map(|b| u16::from_le_bytes(*b)))
                    .collect(),
            )
        } else {
            Samples::U8(rows.flatten().copied().collect())
        };
        Ok(Decoded {
            width: w,
            height: h,
            alpha,
            premultiplied,
            bits: if wide { luma as u32 } else { 8 },
            samples,
        })
    }
}

/// Decodes the primary image of an AVIF/HEIC file. `high_depth` keeps samples deeper than 8
/// bits as `U16` (as ImageMagick reads them); otherwise libheif converts them to 8 bits.
pub fn decode(data: &[u8], high_depth: bool) -> Result<Decoded> {
    Primary::from_bytes(data)?.decode(high_depth)
}

/// [`decode`] once `check(width, height, bytes per decoded pixel)` accepts the size the file's
/// metadata gives, so a caller can refuse a huge image before libheif allocates it.
pub fn decode_checked(
    data: &[u8],
    high_depth: bool,
    check: impl FnOnce(usize, usize, usize) -> Result<()>,
) -> Result<Decoded> {
    let p = Primary::from_bytes(data)?;
    let (w, h) = p.size();
    check(w, h, p.bytes_per_pixel(high_depth))?;
    p.decode(high_depth)
}

/// [`file_size`] of in-memory data.
pub fn size(data: &[u8]) -> Result<(usize, usize)> {
    Ok(Primary::from_bytes(data)?.size())
}

/// The primary image's size after its transformations, read from the file's metadata alone.
pub fn file_size(path: &str) -> Result<(usize, usize)> {
    let path = CString::new(path).map_err(|e| Error::new(ErrorKind::InvalidInput, e))?;
    // SAFETY: `path` is NUL-terminated and outlives the call.
    let p = Primary::open(|lib, ctx| unsafe {
        (lib.read_from_file)(ctx, path.as_ptr(), std::ptr::null())
    })?;
    Ok(p.size())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ftyp(major: &[u8; 4], compatible: &[&[u8; 4]]) -> Vec<u8> {
        let mut b = ((16 + 4 * compatible.len()) as u32).to_be_bytes().to_vec();
        b.extend_from_slice(b"ftyp");
        b.extend_from_slice(major);
        b.extend_from_slice(&[0; 4]);
        for c in compatible {
            b.extend_from_slice(*c);
        }
        b.extend_from_slice(&[0; 8]);
        b
    }

    #[test]
    fn brands() {
        assert!(is_heif(&ftyp(b"avif", &[b"mif1"])));
        assert!(is_heif(&ftyp(b"heic", &[])));
        assert!(is_heif(&ftyp(b"msf1", &[b"avis"])));
        assert!(is_heif(&ftyp(b"XXXX", &[b"isom", b"mif1"])));
        assert!(!is_heif(&ftyp(b"isom", &[b"mp41"])));
        assert!(!is_heif(&ftyp(b"mp42", &[b"isom", b"avc1"])));
        assert!(!is_heif(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR"));
        assert!(!is_heif(b"short"));
    }

    #[test]
    fn rgba8_unpremultiplies() {
        let d = Decoded {
            width: 2,
            height: 1,
            alpha: true,
            premultiplied: true,
            bits: 8,
            samples: Samples::U8(vec![0, 128, 0, 128, 9, 9, 9, 0]),
        };
        assert_eq!(d.into_rgba8(), [0, 255, 0, 128, 0, 0, 0, 0]);
    }
}
