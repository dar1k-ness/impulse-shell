//! Qt Quick `Text { renderType: Text.NativeRendering }` replica: FreeType glyphs + HarfBuzz shaping
//! following QFontEngineFT / QTextEngine / QSGTextMaskMaterial (Format_A8).

use fontconfig_sys as fc;
use freetype_sys as ft;
use harfbuzz_sys as hb;
use std::ffi::c_void;
use std::ptr;

use crate::raster::Canvas;

/// The process-wide FreeType library, shared by every thread under a mutex: the FreeType API
/// reference (`FT_Library`, since 2.5.6; `FT_Face`) allows one library across threads as long
/// as face creation and destruction (`FT_New_Face`, `FT_Done_Face`) are serialised. Each face is
/// used only by the thread that made it (`Font` is `!Send`), which is all FreeType asks of the
/// glyph functions.
struct Lib(ft::FT_Library);
// SAFETY: the handle is only passed to FreeType from inside `with_library`, i.e. while the
// mutex is held, which is the cross-thread use FreeType documents as safe.
unsafe impl Send for Lib {}

/// Runs `f` with the library locked; every call that creates or destroys a face (or frees
/// library-owned memory such as `FT_MM_Var`) goes through here.
fn with_library<R>(f: impl FnOnce(ft::FT_Library) -> R) -> R {
    static LIB: std::sync::OnceLock<std::sync::Mutex<Lib>> = std::sync::OnceLock::new();
    let lib = LIB.get_or_init(|| {
        let mut l = ptr::null_mut();
        // SAFETY: `l` is a valid out pointer; on failure it stays null and every later FreeType
        // call with it fails with `FT_Err_Invalid_Library_Handle`.
        unsafe { ft::FT_Init_FreeType(&mut l) };
        std::sync::Mutex::new(Lib(l))
    });
    let lib = lib
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    f(lib.0)
}

/// `len` items at `p`, or an empty slice when `len` is 0 (C APIs may then pass null).
///
/// # Safety
/// For `len > 0`, `p` must point to `len` initialised items that stay valid and unchanged for `'a`.
pub(crate) unsafe fn c_slice<'a, T>(p: *const T, len: usize) -> &'a [T] {
    if len == 0 {
        return &[];
    }
    // SAFETY: per the caller.
    unsafe { std::slice::from_raw_parts(p, len) }
}

/// How the bytes of a row of a glyph's bitmap are its pixels: the FT_Pixel_Mode values read.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Pixels {
    /// 1 bit a pixel, the most significant bit first (FT_PIXEL_MODE_MONO).
    Mono,
    /// A byte of coverage a pixel (FT_PIXEL_MODE_GRAY).
    Gray,
    /// Premultiplied blue, green, red, alpha (FT_PIXEL_MODE_BGRA).
    Bgra,
}

/// The rows of a glyph's bitmap. `Rows::of` is the one place that reads `FT_Bitmap::buffer`
/// (held by the test `one_place_reads_a_bitmaps_buffer`): every bitmap drawn goes through it,
/// so a row is never longer than its pixel mode and `pitch` say.
struct Rows<'a> {
    pixels: Pixels,
    width: usize,
    rows: usize,
    /// The bytes of a row that hold its `width` pixels; the rest of `pitch` is padding.
    row_bytes: usize,
    pitch: usize,
    /// A negative `FT_Bitmap::pitch`: the top row is the last one in `bytes`.
    up: bool,
    /// The whole buffer, `rows * pitch` bytes.
    bytes: &'a [u8],
}

impl<'a> Rows<'a> {
    /// `None` for the pixel modes not read (2- and 4-bit grey, LCD, none), for rows that
    /// `pitch` says are shorter than `width` pixels of the mode, and for rows with no buffer.
    ///
    /// # Safety
    /// `bm` must be a live FreeType bitmap: `rows` rows of `|pitch|` bytes, `buffer` at the
    /// top one.
    unsafe fn of(bm: &'a ft::FT_Bitmap) -> Option<Self> {
        let width = usize::try_from(bm.width).ok()?;
        let rows = usize::try_from(bm.rows).ok()?;
        let (pixels, row_bytes) = match bm.pixel_mode as u32 {
            ft::FT_PIXEL_MODE_MONO => (Pixels::Mono, width.div_ceil(8)),
            ft::FT_PIXEL_MODE_GRAY => (Pixels::Gray, width),
            ft::FT_PIXEL_MODE_BGRA => (Pixels::Bgra, width.checked_mul(4)?),
            _ => return None,
        };
        let pitch = bm.pitch.unsigned_abs() as usize;
        let len = rows.checked_mul(pitch)?;
        let top = bm.buffer.cast_const();
        if rows != 0 && (pitch < row_bytes || (pitch != 0 && top.is_null())) {
            return None;
        }
        let up = bm.pitch < 0;
        // With a negative pitch the buffer starts `rows - 1` rows before the top row.
        let start = match up {
            true => top.wrapping_sub(len - pitch.min(len)),
            false => top,
        };
        // SAFETY: FT_Bitmap: the buffer is `rows` rows of `|pitch|` bytes from `start`.
        let bytes = unsafe { c_slice(start, len) };
        Some(Rows {
            pixels,
            width,
            rows,
            row_bytes,
            pitch,
            up,
            bytes,
        })
    }

    /// The rows from the top one, each the `row_bytes` bytes of its pixels.
    fn iter(&self) -> impl Iterator<Item = &'a [u8]> + use<'a> {
        let (rows, pitch, row_bytes, up, bytes) =
            (self.rows, self.pitch, self.row_bytes, self.up, self.bytes);
        (0..rows).map(move |row| {
            let at = if up { rows - 1 - row } else { row } * pitch;
            &bytes[at..at + row_bytes]
        })
    }
}

/// A glyph bitmap as the 8-bit coverage QFontEngineFT::loadGlyph makes of it for Format_A8: a
/// 1-bit row (what FreeType gives for a bitmap font: BDF, PCF, an OpenType bitmap strike) becomes
/// 0xff or 0x00 per pixel, an 8-bit grey row is copied. `None` for every other pixel mode, which
/// Qt does not draw as coverage either (2- and 4-bit grey: "unknown pixel_mode"; LCD and BGRA
/// belong to other glyph formats), and for rows that `pitch` says are shorter than that.
///
/// # Safety
/// As `Rows::of`.
unsafe fn coverage(bm: &ft::FT_Bitmap) -> Option<Vec<u8>> {
    // SAFETY: per the caller.
    let rows = unsafe { Rows::of(bm) }?;
    let width = rows.width;
    let mut buf = Vec::with_capacity(width * rows.rows);
    match rows.pixels {
        Pixels::Mono => {
            for line in rows.iter() {
                buf.extend((0..width).map(|x| match line[x >> 3] & (0x80 >> (x & 7)) {
                    0 => 0x00,
                    _ => 0xff,
                }));
            }
        }
        Pixels::Gray => rows.iter().for_each(|line| buf.extend_from_slice(line)),
        Pixels::Bgra => return None,
    }
    Some(buf)
}

/// The glyph a rendered slot holds, its bitmap as `coverage`.
///
/// # Safety
/// `slot` must be a face's live glyph slot, a glyph loaded into it.
unsafe fn coverage_glyph(slot: ft::FT_GlyphSlot) -> Option<Glyph> {
    // SAFETY: per the caller.
    let slot = unsafe { &*slot };
    // SAFETY: the slot's bitmap, live until the face loads another glyph.
    let buf = unsafe { coverage(&slot.bitmap) }?;
    Some(Glyph {
        width: slot.bitmap.width as usize,
        rows: slot.bitmap.rows as usize,
        left: slot.bitmap_left as i64,
        top: slot.bitmap_top as i64,
        buf,
        color: false,
        key: Default::default(),
    })
}

/// The face's glyph slot after loading `glyph` with `flags`, or `None` if FreeType fails.
///
/// # Safety
/// `face` must be a live face that no other thread is using.
pub(crate) unsafe fn load_glyph(
    face: ft::FT_Face,
    glyph: u32,
    flags: i32,
) -> Option<ft::FT_GlyphSlot> {
    // SAFETY: per the caller.
    if unsafe { ft::FT_Load_Glyph(face, glyph, flags) } != 0 {
        return None;
    }
    // SAFETY: a live face's `glyph` is its (always allocated) glyph slot.
    Some(unsafe { (*face).glyph })
}

/// Distance-field tile cache key: glyph and everything `DfPlace` and the atlas give the
/// picture (pen, item translation, matrix, viewport, colour and material as bits; atlas state).
type DfShared = std::rc::Rc<std::cell::RefCell<crate::dftext::DfAtlas>>;
type DfKey = (
    i32,
    u32,
    [u64; 2],
    [u32; 2],
    Option<[u32; 9]>,
    [u32; 5],
    [u32; 4],
    [u32; 6],
    (usize, usize, usize),
);
/// Placed QtRendering tiles kept per font; least recently drawn go first.
const DF_TILES: std::num::NonZeroUsize = std::num::NonZeroUsize::new(512).unwrap();

/// Rendered glyphs by (glyph id, subpixel offset); `None` for a glyph with no bitmap.
type GlyphCache = std::collections::HashMap<(u32, i64), Option<std::rc::Rc<Glyph>>>;
/// Glyph boxes by (glyph id, load flags).
type GlyphBoxes = std::collections::HashMap<(u32, i32), (i64, i64, i64)>;

pub struct Font {
    _data: std::rc::Rc<FontData>,
    face: ft::FT_Face,
    hb_font: *mut hb::hb_font_t,
    pub pixel_size: u32,
    pub load_flags: i32,
    /// 26.6 fixed
    pub ascent: i64,
    pub descent: i64,
    pub leading: i64,
    /// QFontEngineFT synthetic bold (FT_GlyphSlot_Embolden on every loaded glyph).
    pub embolden: bool,
    /// QFontEngineFT synthetic italic (FT_GlyphSlot_Oblique): an italic request whose matched
    /// face is not italic by Qt's test (named instance `ital` = 1, else FT_STYLE_FLAG_ITALIC).
    pub oblique: bool,
    /// Colour bitmap font (CBDT/sbix, e.g. Noto Color Emoji): strike-to-pixel-size scale, as
    /// QFontEngineFT::scalableBitmapScaleFactor; 0 for outline fonts.
    pub color_scale: f64,
    /// A face that is not scalable and has no colour (BDF, PCF, an OpenType file with bitmaps
    /// only): its glyphs are one strike's bitmaps, drawn as the file has them.
    pub strike: bool,
    /// The QFont's weight (the one asked for, not the matched face's).
    request_weight: f32,
    /// QFont::letterSpacing (absolute), 26.6: added to the advance of each character's last glyph.
    letter_spacing: i64,
    /// Rendered A8 glyph bitmaps by (glyph id, subpixel offset), like QFontEngineFT's glyph cache.
    glyphs: std::cell::RefCell<GlyphCache>,
    /// The scale of the glyph cache this font's bitmaps are for (QSGTextMaskMaterial: the
    /// window's devicePixelRatio): they are drawn through that transform, unhinted. 1.0 for
    /// none.
    raster: f64,
    /// `glyph_box` results by (glyph id, load flags).
    boxes: std::cell::RefCell<GlyphBoxes>,
    /// Distance fields for `Text.QtRendering` and where Qt's glyph cache puts them: one per
    /// face and style whatever the size, as QSGRenderContext keeps them.
    df: std::rc::Rc<std::cell::RefCell<crate::dftext::DfAtlas>>,
    /// The caches of the render type qualities asked of this font, and what names its face.
    df_q: std::cell::RefCell<Vec<(i32, DfShared)>>,
    df_face: (String, i32, Option<usize>, f32),
    /// Placed QtRendering coverage tiles.
    df_tiles: std::cell::RefCell<lru::LruCache<DfKey, Option<crate::dftext::DfTile>>>,
    /// The named instance of a variable font this face is set to.
    named: Option<usize>,
}

/// A8 coverage bitmap of one glyph, or premultiplied RGBA for colour glyphs.
pub struct Glyph {
    pub width: usize,
    pub rows: usize,
    left: i64,
    top: i64,
    pub buf: Vec<u8>,
    pub color: bool,
    /// Atlas slots and draw signatures name the bitmap by this.
    pub key: crate::raster::TexKey,
}

/// A glyph bitmap placed at an integer device position.
pub type GlyphQuad = (std::rc::Rc<Glyph>, i64, i64);
/// A glyph at its unsnapped scene position (drawn through a transform, snapped afterwards).
pub type GlyphQuadF = (std::rc::Rc<Glyph>, f64, f64);

/// QFontEngineFT with HintNone (`Font.PreferNoHinting`).
const NO_HINT_FLAGS: i32 = ft::FT_LOAD_DEFAULT | ft::FT_LOAD_NO_HINTING | ft::FT_LOAD_NO_BITMAP;

const LOAD_FLAGS: i32 = ft::FT_LOAD_DEFAULT | ft::FT_LOAD_TARGET_LIGHT | ft::FT_LOAD_NO_BITMAP;

/// QFontEngine::maxCachedGlyphSize() in 26.6 pixels: above it QFontEngineFT does not hint.
const MAX_HINTED_SIZE64: i64 = 64 << 6;

/// A face that is not scalable (a bitmap strike without colour): its bitmaps as the file has
/// them. Hinting preferences and subpixel positions mean nothing to it.
const STRIKE_FLAGS: i32 = ft::FT_LOAD_DEFAULT;

/// QFontEngineFT with HintFull (`Font.PreferFullHinting`): FT_LOAD_TARGET_NORMAL, and the
/// advances are the hinted ones rounded to whole pixels (no design metrics).
const FULL_HINT_FLAGS: i32 = ft::FT_LOAD_DEFAULT | ft::FT_LOAD_NO_BITMAP;

unsafe extern "C" fn h_advance_full(
    _f: *mut hb::hb_font_t,
    data: *mut c_void,
    glyph: hb::hb_codepoint_t,
    _u: *mut c_void,
) -> hb::hb_position_t {
    // SAFETY: HarfBuzz passes the `AdvCtx` that `hb_font_set_funcs` installed as `data`.
    unsafe {
        AdvCtx::advance(data, glyph, FULL_HINT_FLAGS, true, |s| {
            ((s.advance.x + 32) & -64) as hb::hb_position_t
        })
    }
}

/// HarfBuzz font data: the face plus a per-glyph advance cache, so shaping the same text again
/// (every layout pass) does not reload and rehint every glyph through FreeType.
struct AdvCtx {
    face: ft::FT_Face,
    /// The FreeType load flags of the font's glyphs (`Font::load_flags`).
    flags: i32,
    cache: std::cell::RefCell<std::collections::HashMap<u32, hb::hb_position_t>>,
}

impl AdvCtx {
    fn boxed(face: ft::FT_Face, flags: i32) -> *mut c_void {
        Box::into_raw(Box::new(AdvCtx {
            face,
            flags,
            cache: Default::default(),
        })) as *mut c_void
    }

    /// The advance `adv` reads from the slot of `glyph` loaded with `flags` (untransformed if
    /// `reset_transform`), cached per glyph; 0 if FreeType cannot load it.
    ///
    /// # Safety
    /// `data` must point to a live `AdvCtx` (the `hb_font_set_funcs` user data).
    unsafe fn advance(
        data: *mut c_void,
        glyph: u32,
        flags: i32,
        reset_transform: bool,
        adv: impl Fn(&ft::FT_GlyphSlotRec) -> hb::hb_position_t,
    ) -> hb::hb_position_t {
        // SAFETY: the caller passes the live `AdvCtx`; it is only read here.
        let ctx = unsafe { &*(data as *const AdvCtx) };
        if let Some(&v) = ctx.cache.borrow().get(&glyph) {
            return v;
        }
        if reset_transform {
            // SAFETY: the font owns the face, which only this thread uses (`Font` is `!Send`).
            unsafe { ft::FT_Set_Transform(ctx.face, ptr::null_mut(), ptr::null_mut()) };
        }
        // SAFETY: as above.
        let slot = unsafe { load_glyph(ctx.face, glyph, flags) };
        // SAFETY: the slot was just filled and nothing touches it before `adv` returns.
        let v = slot.map_or(0, |s| adv(unsafe { &*s }));
        ctx.cache.borrow_mut().insert(glyph, v);
        v
    }
}

unsafe extern "C" fn adv_destroy(data: *mut c_void) {
    // SAFETY: HarfBuzz destroys the user data once, with the `Box` `AdvCtx::boxed` leaked.
    drop(unsafe { Box::from_raw(data as *mut AdvCtx) });
}

unsafe extern "C" fn h_advance(
    _f: *mut hb::hb_font_t,
    data: *mut c_void,
    glyph: hb::hb_codepoint_t,
    _u: *mut c_void,
) -> hb::hb_position_t {
    // SAFETY: HarfBuzz passes the `AdvCtx` that `hb_font_set_funcs` installed as `data`.
    unsafe {
        AdvCtx::advance(data, glyph, LOAD_FLAGS, true, |s| {
            (s.linearHoriAdvance >> 10) as hb::hb_position_t
        })
    }
}

/// QFontEngineFT with HintFull under design metrics (Text.QtRendering): the unhinted advance
/// (`shape` rounds it with its kerning to whole pixels, as the engine has no subpixel positions).
unsafe extern "C" fn h_advance_design(
    _f: *mut hb::hb_font_t,
    data: *mut c_void,
    glyph: hb::hb_codepoint_t,
    _u: *mut c_void,
) -> hb::hb_position_t {
    // SAFETY: HarfBuzz passes the `AdvCtx` that `hb_font_set_funcs` installed as `data`.
    unsafe {
        AdvCtx::advance(data, glyph, NO_HINT_FLAGS, true, |s| {
            (s.linearHoriAdvance >> 10) as hb::hb_position_t
        })
    }
}

/// Colour bitmap strike advance scaled to the requested size (QFontEngineFT::scalableBitmapScaleFactor).
unsafe extern "C" fn h_advance_color(
    _f: *mut hb::hb_font_t,
    data: *mut c_void,
    glyph: hb::hb_codepoint_t,
    scale: *mut c_void,
) -> hb::hb_position_t {
    // SAFETY: `scale` is the boxed `f64` installed with this function (freed by `drop_scale`).
    let s = unsafe { *(scale as *const f64) };
    // SAFETY: HarfBuzz passes the `AdvCtx` that `hb_font_set_funcs` installed as `data`.
    unsafe {
        AdvCtx::advance(data, glyph, ft::FT_LOAD_COLOR, false, |slot| {
            // QFontEngineFT::recalcAdvances: the strike's whole-pixel advance times the factor.
            let a = ((slot.advance.x + 32) & -64) as f64 * s;
            a.round() as hb::hb_position_t
        })
    }
}

/// QFontEngineFT::recalcAdvances for a face that is not scalable (a bitmap strike: BDF, PCF, an
/// OpenType file with bitmaps only): the strike's advance in whole pixels, never design metrics.
unsafe extern "C" fn h_advance_strike(
    _f: *mut hb::hb_font_t,
    data: *mut c_void,
    glyph: hb::hb_codepoint_t,
    _u: *mut c_void,
) -> hb::hb_position_t {
    // SAFETY: HarfBuzz passes the `AdvCtx` that `hb_font_set_funcs` installed as `data`.
    unsafe {
        AdvCtx::advance(data, glyph, ft::FT_LOAD_DEFAULT, false, |s| {
            ((s.advance.x + 32) & -64) as hb::hb_position_t
        })
    }
}

/// QFontEngineFT::glyphIndex: the glyph FreeType's charmap has for the character. A BDF or PCF
/// file has no OpenType `cmap` for HarfBuzz to read.
unsafe extern "C" fn nominal_glyph(
    _f: *mut hb::hb_font_t,
    data: *mut c_void,
    unicode: hb::hb_codepoint_t,
    glyph: *mut hb::hb_codepoint_t,
    _u: *mut c_void,
) -> hb::hb_bool_t {
    // SAFETY: HarfBuzz passes the `AdvCtx` that `hb_font_set_funcs` installed as `data`.
    let ctx = unsafe { &*(data as *const AdvCtx) };
    // SAFETY: the font owns the face, which only this thread uses (`Font` is `!Send`).
    let gid = unsafe { ft::FT_Get_Char_Index(ctx.face, unicode as _) };
    // SAFETY: HarfBuzz's out value.
    unsafe { *glyph = gid };
    (gid != 0) as hb::hb_bool_t
}

unsafe extern "C" fn drop_scale(p: *mut c_void) {
    // SAFETY: HarfBuzz destroys the user data once, with the leaked `Box<f64>` it was given.
    drop(unsafe { Box::from_raw(p as *mut f64) });
}

/// _hb_qt_font_get_glyph_contour_point: the point of a glyph's outline that a GPOS anchor names
/// (AnchorFormat2, as DejaVu Sans attaches its combining marks), from the glyph as FreeType
/// loads it for drawing (QFontEngineFT::getPointInOutline). Its y is FreeType's, pointing up,
/// and Qt hands it unchanged to a font whose y scale points down: HarfBuzz reads the point
/// mirrored about the baseline, and the mark lands above the letter by twice the point's
/// height (both marks of `x̂́` at one height, clear of the `x`). The same here, so that the
/// glyph is where Qt draws it.
unsafe extern "C" fn contour_point(
    _f: *mut hb::hb_font_t,
    data: *mut c_void,
    glyph: hb::hb_codepoint_t,
    point: u32,
    x: *mut hb::hb_position_t,
    y: *mut hb::hb_position_t,
    _u: *mut c_void,
) -> hb::hb_bool_t {
    // SAFETY: `data` is the `AdvCtx` the font was given in `set_advance_func`.
    let ctx = unsafe { &*(data as *const AdvCtx) };
    // SAFETY: the font's live face, used by this thread only.
    unsafe { ft::FT_Set_Transform(ctx.face, ptr::null_mut(), ptr::null_mut()) };
    // SAFETY: as above.
    let slot = unsafe { load_glyph(ctx.face, glyph, ctx.flags) };
    // SAFETY: the slot just loaded.
    let Some(slot) = slot.map(|s| unsafe { &*s }) else {
        return 0;
    };
    if slot.format != ft::FT_GLYPH_FORMAT_OUTLINE || point >= slot.outline.n_points as u32 {
        return 0;
    }
    // SAFETY: the outline's `n_points` points, live until the slot's next load.
    let points = unsafe { std::slice::from_raw_parts(slot.outline.points, point as usize + 1) };
    let p = points[point as usize];
    // SAFETY: HarfBuzz's out values.
    unsafe { *x = p.x as hb::hb_position_t };
    // SAFETY: as above.
    unsafe { *y = p.y as hb::hb_position_t };
    1
}

/// Makes `font` take its glyph advances from `func` over `face`, with `scale` as the function's
/// user data (`h_advance_color`), its outline points from the glyphs loaded with `flags`, and
/// with `charmap` also its glyphs from FreeType's charmap.
///
/// # Safety
/// `font` must be a live HarfBuzz font and `face` a live face that outlives it.
unsafe fn set_advance_func(
    font: *mut hb::hb_font_t,
    face: ft::FT_Face,
    func: hb::hb_font_get_glyph_h_advance_func_t,
    scale: Option<f64>,
    flags: i32,
    charmap: bool,
) {
    let (user, destroy): (*mut c_void, hb::hb_destroy_func_t) = match scale {
        Some(s) => (Box::into_raw(Box::new(s)) as *mut c_void, Some(drop_scale)),
        None => (ptr::null_mut(), None),
    };
    // SAFETY: creates an empty, mutable funcs object.
    let funcs = unsafe { hb::hb_font_funcs_create() };
    // SAFETY: `funcs` is ours; HarfBuzz frees `user` with `destroy`.
    unsafe { hb::hb_font_funcs_set_glyph_h_advance_func(funcs, func, user, destroy) };
    // SAFETY: `funcs` is ours; the function reads only the font's `AdvCtx`.
    unsafe {
        hb::hb_font_funcs_set_glyph_contour_point_func(
            funcs,
            Some(contour_point),
            ptr::null_mut(),
            None,
        )
    };
    if charmap {
        // SAFETY: `funcs` is ours; the function has no user data.
        unsafe {
            hb::hb_font_funcs_set_nominal_glyph_func(
                funcs,
                Some(nominal_glyph),
                ptr::null_mut(),
                None,
            )
        };
    }
    // SAFETY: per the caller; HarfBuzz frees the new `AdvCtx` with `adv_destroy`.
    unsafe { hb::hb_font_set_funcs(font, funcs, AdvCtx::boxed(face, flags), Some(adv_destroy)) };
    // SAFETY: drops this function's funcs reference (the font keeps its own).
    unsafe { hb::hb_font_funcs_destroy(funcs) };
}

/// QFontEngineFT::init synthetic bold. The fontconfig database leaves FaceId::instanceIndex at -1,
/// so calculateActualWeight falls back to the static OS/2 usWeightClass even for named instances.
/// # Safety
/// `face` must be a live face.
unsafe fn synth_bold(face: ft::FT_Face, weight: f32, pixel_size: u32) -> bool {
    if weight < 700.0 || pixel_size >= 64 || std::env::var_os("QT_NO_SYNTHESIZED_BOLD").is_some() {
        return false;
    }
    // SAFETY: the caller passes a live face.
    let rec = unsafe { &*face };
    let (style, flags) = (rec.style_flags, rec.face_flags);
    if style & ft::FT_STYLE_FLAG_BOLD != 0 || flags & ft::FT_FACE_FLAG_FIXED_WIDTH != 0 {
        return false;
    }
    // SAFETY: the caller passes a live face.
    let actual = unsafe { sfnt_table(face, b"OS/2") }
        .filter(|t| t.len() >= 6)
        .map_or(700, |t| u16::from_be_bytes([t[4], t[5]]));
    actual < 700
}

/// QFontEngineFT::getSfntTableData: FT_Load_Sfnt_Table reads the table of this face, also
/// for a face inside a TrueType collection (`.ttc`), where the file starts with the `ttcf`
/// header instead of the face's table directory.
/// # Safety
/// `face` must be a live face.
unsafe fn sfnt_table(face: ft::FT_Face, tag: &[u8; 4]) -> Option<Vec<u8>> {
    let tag = u32::from_be_bytes(*tag) as ft::FT_ULong;
    let mut len: ft::FT_ULong = 0;
    // SAFETY: a live face per the caller; a null buffer only asks for the length.
    if unsafe { ft::FT_Load_Sfnt_Table(face, tag, 0, ptr::null_mut(), &mut len) } != 0 {
        return None;
    }
    let mut out = vec![0u8; len as usize];
    // SAFETY: `out` has room for the `len` bytes FreeType writes.
    if unsafe { ft::FT_Load_Sfnt_Table(face, tag, 0, out.as_mut_ptr(), &mut len) } != 0 {
        return None;
    }
    Some(out)
}

fn tag(s: &str) -> u32 {
    let b = s.as_bytes();
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

/// A face's name string (`family_name`, `style_name`); empty when the face has none.
/// # Safety
/// `p` must be null or a NUL-terminated string that is live for the call.
unsafe fn face_string(p: *const std::ffi::c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    // SAFETY: per the caller.
    unsafe { std::ffi::CStr::from_ptr(p) }
        .to_string_lossy()
        .into_owned()
}

impl Font {
    /// `weight` is the QFont weight used by fontconfig to pick the named instance (Qt default 400).
    pub fn open(
        path: &str,
        pixel_size: u32,
        weight: f32,
        axes: &[(&str, f32)],
    ) -> std::io::Result<Font> {
        Self::open_index(path, 0, pixel_size, weight, axes)
    }

    pub fn open_index(
        path: &str,
        index: i32,
        pixel_size: u32,
        weight: f32,
        axes: &[(&str, f32)],
    ) -> std::io::Result<Font> {
        Self::open_styled(path, index, pixel_size, weight, weight, axes)
    }

    /// `open_index` for a face QFontDatabase picked: `weight` selects the named instance (the
    /// picked style's weight) while synthetic bold follows the QFont's own `request_weight`, as
    /// QFontEngineFT::init tests fontDef.weight.
    pub fn open_styled(
        path: &str,
        index: i32,
        pixel_size: u32,
        weight: f32,
        request_weight: f32,
        axes: &[(&str, f32)],
    ) -> std::io::Result<Font> {
        Self::open_sized(
            path,
            index,
            (pixel_size as i64) << 6,
            weight,
            request_weight,
            axes,
        )
    }

    /// `open_styled` at a 26.6 pixel size: QFontEngineFT at a fractional device pixel ratio
    /// (pixelSize * devicePixelRatio) sizes the face in 26.6 too.
    pub fn open_sized(
        path: &str,
        index: i32,
        sz: i64,
        weight: f32,
        request_weight: f32,
        axes: &[(&str, f32)],
    ) -> std::io::Result<Font> {
        let (mut sz, mut pixel_size) = (sz, ((sz + 32) >> 6).max(1) as u32);
        let data = font_file(path)?;
        let mut face = ptr::null_mut();
        let opened = with_library(|lib| {
            // SAFETY: `data` stays alive in the returned `Font` (`_data`) for as long as the face
            // reads it; `face` is an out pointer.
            unsafe {
                ft::FT_New_Memory_Face(
                    lib,
                    data.as_ptr(),
                    data.len() as _,
                    (index & 0xffff) as _,
                    &mut face,
                )
            }
        });
        if opened != 0 {
            return Err(std::io::Error::other("FT_New_Memory_Face"));
        }
        let mut mm: *mut ft::FT_MM_Var = ptr::null_mut();
        let mut variations = Vec::new();
        let mut named: Option<usize> = None;
        // SAFETY: the face just opened.
        let mut actual_italic = unsafe { (*face).style_flags } & 1 != 0;
        let italic_req = axes.iter().any(|(a, v)| *a == "ital" && *v >= 1.0);
        // SAFETY: a live face; `mm` is an out pointer.
        if unsafe { ft::FT_Get_MM_Var(face, &mut mm) } == 0 {
            // SAFETY: FreeType filled `mm`; it stays valid until `FT_Done_MM_Var` below.
            let var = unsafe { &*mm };
            let n = var.num_axis as usize;
            // SAFETY: `axis` holds `num_axis` entries.
            let var_axes = unsafe { c_slice(var.axis, n) };
            // SAFETY: `namedstyle` holds `num_namedstyles` entries.
            let styles = unsafe { c_slice(var.namedstyle, var.num_namedstyles as usize) };
            // SAFETY: each named style's `coords` holds `num_axis` values.
            let style_coords = |ns: &ft::FT_Var_Named_Style| unsafe { c_slice(ns.coords, n) };
            let mut coords = vec![0 as ft::FT_Fixed; n];
            // SAFETY: `coords` has room for the `n` values FreeType writes.
            unsafe { ft::FT_Get_Var_Design_Coordinates(face, n as _, coords.as_mut_ptr()) };
            // fontconfig exposes named instances as faces; QFont matching picks the one closest
            // to the requested weight (upright, normal width).
            let axis_tag = |i: usize| var_axes[i].tag as u32;
            let mut best: Option<(f64, usize)> = None;
            for (j, ns) in styles.iter().enumerate() {
                let mut score = 0.0;
                for (i, &c) in style_coords(ns).iter().enumerate() {
                    let v = c as f64 / 65536.0;
                    score += match axis_tag(i) {
                        t if t == tag("wght") => (v - weight as f64).abs(),
                        t if t == tag("wdth") => (v - 100.0).abs() * 10.0,
                        // Italic requests (<i>) match the italic named instance, as fontconfig does.
                        t if t == tag("slnt") || t == tag("ital") => {
                            let want = axes
                                .iter()
                                .find(|(a, _)| tag(a) == t)
                                .map_or(0.0, |(_, w)| *w as f64);
                            (v - want).abs() * 1000.0
                        }
                        _ => 0.0,
                    };
                }
                if best.is_none_or(|(b, _)| score < b) {
                    best = Some((score, j));
                }
            }
            if let Some((_, j)) = best {
                named = Some(j);
                let ns_coords = style_coords(&styles[j]);
                if let Some(i) = (0..n).find(|&i| axis_tag(i) == tag("ital")) {
                    actual_italic = (ns_coords[i] >> 16) == 1;
                }
                coords.copy_from_slice(ns_coords);
            }
            for (coord, ax) in coords.iter_mut().zip(var_axes) {
                if let Some((t, v)) = axes.iter().find(|(t, _)| ax.tag == tag(t) as ft::FT_ULong) {
                    *coord = (*v as f64 * 65536.0) as ft::FT_Fixed;
                    variations.push(hb::hb_variation_t {
                        tag: tag(t),
                        value: *v,
                    });
                }
            }
            // SAFETY: `coords` holds the face's `n` axis values.
            unsafe { ft::FT_Set_Var_Design_Coordinates(face, n as _, coords.as_mut_ptr()) };
            // SAFETY: `mm` came from this library's `FT_Get_MM_Var` and is freed once; nothing
            // borrowed from it is used afterwards.
            with_library(|lib| unsafe { ft::FT_Done_MM_Var(lib, mm) });
        }
        // SAFETY: a live face; the fields are copied out before FreeType sizes it below.
        let rec = unsafe { &*face };
        let (flags, units_per_em) = (rec.face_flags, rec.units_per_EM);
        let (strikes, num_strikes) = (rec.available_sizes, rec.num_fixed_sizes);
        let mut color_scale = 0.0;
        // QFreetypeFace::computeSize: a face that is not scalable is set to one of its strikes.
        let fixed = flags & ft::FT_FACE_FLAG_SCALABLE == 0 && num_strikes > 0;
        // A strike without colour is drawn as it is, at its own size, whatever size is asked.
        let strike = fixed && flags & ft::FT_FACE_FLAG_COLOR == 0;
        if fixed {
            // SAFETY: `available_sizes` holds `num_fixed_sizes` entries.
            let sizes = unsafe { c_slice(strikes, num_strikes as usize) };
            let best = if strike {
                // The strike closest in height, then in width; the first of equally close ones.
                let off = |ppem: ft::FT_Pos| (sz - ppem).abs();
                (0..sizes.len()).min_by_key(|&i| (off(sizes[i].y_ppem), off(sizes[i].x_ppem)))
            } else {
                // The smallest strike not below the request, else the largest.
                (0..sizes.len())
                    .filter(|&i| sizes[i].y_ppem >= sz as ft::FT_Pos)
                    .min_by_key(|&i| sizes[i].y_ppem)
                    .or_else(|| (0..sizes.len()).max_by_key(|&i| sizes[i].y_ppem))
            }
            .unwrap();
            // SAFETY: a live face; `best` indexes its strikes.
            unsafe { ft::FT_Select_Size(face, best as _) };
            if strike {
                // The font is the strike's size from here on (QFontInfo::pixelSize, the size
                // its fallback fonts are opened at): fontconfig's match names that size.
                sz = sizes[best].y_ppem;
                pixel_size = ((sz + 32) >> 6).max(1) as u32;
            } else {
                // QFixed(pixelSize) / QFixed(strike height): a truncated 26.6 quotient.
                color_scale = ((sz << 6) / ((sizes[best].height as i64) << 6)) as f64 / 64.0;
            }
        } else {
            // SAFETY: a live face.
            unsafe { ft::FT_Set_Char_Size(face, sz as _, sz as _, 0, 0) };
        }
        // SAFETY: a live face.
        let size = unsafe { (*face).size };
        // SAFETY: the active size, just selected or set.
        let m = unsafe { (*size).metrics };
        let ms = |v: i64| {
            if color_scale > 0.0 {
                (v as f64 * color_scale) as i64
            } else {
                v
            }
        };
        let (mut ascent, mut descent, mut leading) = (
            ms(m.ascender as i64),
            ms(-(m.descender as i64)),
            ms((m.height - m.ascender + m.descender) as i64),
        );
        // QFontEngine::initializeHeightMetrics: hhea, then OS/2 (typo if USE_TYPO_METRICS else win), unrounded.
        // FreeType leaves units_per_EM at 0 for bitmap-only faces: read it from `head`.
        let upem = match units_per_em {
            // SAFETY: a live face.
            0 => unsafe { sfnt_table(face, b"head") }
                .filter(|t| t.len() >= 20)
                .map_or(2048.0, |t| u16::from_be_bytes([t[18], t[19]]) as f64),
            u => u as f64,
        };
        // QFixed::fromReal truncates toward zero.
        let fx = |v: f64| (v * sz as f64 / 64.0 / upem * 64.0) as i64;
        let be16 = |t: &[u8], o: usize| i16::from_be_bytes([t[o], t[o + 1]]) as f64;
        let beu16 = |t: &[u8], o: usize| u16::from_be_bytes([t[o], t[o + 1]]) as f64;
        // QFontEngine::initializeHeightMetrics keeps FreeType's metrics (those above) for a
        // file with bitmap strikes, colour or not.
        let bitmaps = [b"EBLC", b"CBLC", b"bdat"].iter().any(|tag| {
            let tag = u32::from_be_bytes(**tag) as ft::FT_ULong;
            let mut len: ft::FT_ULong = 0;
            // SAFETY: a live face; a null buffer only asks for the table's length.
            unsafe {
                ft::FT_Load_Sfnt_Table(face, tag, 0, ptr::null_mut(), &mut len) == 0 && len > 0
            }
        });
        // SAFETY: a live face.
        if let Some(t) = unsafe { sfnt_table(face, b"hhea") }.filter(|t| !bitmaps && t.len() >= 10)
            && !(be16(&t, 4) == 0.0 && be16(&t, 6) == 0.0)
        {
            (ascent, descent, leading) = (fx(be16(&t, 4)), -fx(be16(&t, 6)), fx(be16(&t, 8)));
        }
        // SAFETY: a live face.
        if let Some(t) = unsafe { sfnt_table(face, b"OS/2") }.filter(|t| !bitmaps && t.len() >= 78)
        {
            if beu16(&t, 62) as u16 & 0x80 != 0 {
                if !(be16(&t, 68) == 0.0 && be16(&t, 70) == 0.0) {
                    (ascent, descent, leading) =
                        (fx(be16(&t, 68)), -fx(be16(&t, 70)), fx(be16(&t, 72)));
                }
            } else if !(beu16(&t, 74) == 0.0 && beu16(&t, 76) == 0.0) {
                (ascent, descent, leading) = (fx(beu16(&t, 74)), fx(beu16(&t, 76)), 0);
            }
        }

        // SAFETY: `data` outlives the blob (the `Font` keeps it); read-only, no destroy callback.
        let blob = unsafe {
            hb::hb_blob_create(
                data.as_ptr() as *const _,
                data.len() as _,
                hb::HB_MEMORY_MODE_READONLY,
                ptr::null_mut(),
                None,
            )
        };
        // SAFETY: a live blob; the face takes its own reference.
        let hface = unsafe { hb::hb_face_create(blob, (index & 0xffff) as u32) };
        // SAFETY: drops this function's blob reference.
        unsafe { hb::hb_blob_destroy(blob) };
        // SAFETY: a live face; the font takes its own reference.
        let parent = unsafe { hb::hb_font_create(hface) };
        // SAFETY: drops this function's face reference.
        unsafe { hb::hb_face_destroy(hface) };
        // SAFETY: a live font.
        unsafe { hb::hb_ot_font_set_funcs(parent) };
        // SAFETY: a live font.
        unsafe { hb::hb_font_set_scale(parent, sz as i32, -(sz as i32)) };
        // SAFETY: a live font.
        unsafe { hb::hb_font_set_ppem(parent, pixel_size, pixel_size) };
        // Qt's hb face carries the fontconfig index (named instance in the high bits), so hb_font_create
        // applies the matched named instance; the QFont variableAxes overrides never reach HarfBuzz.
        let _ = &variations;
        if let Some(j) = named {
            // SAFETY: a live font; `j` is one of the face's named instances.
            unsafe { hb::hb_font_set_var_named_instance(parent, j as u32) };
        }
        // SAFETY: a live font; the sub-font takes its own reference.
        let font = unsafe { hb::hb_font_create_sub_font(parent) };
        // SAFETY: drops this function's parent reference.
        unsafe { hb::hb_font_destroy(parent) };
        if strike {
            // SAFETY: `font` was just created over `face`, which the returned `Font` owns.
            unsafe {
                set_advance_func(font, face, Some(h_advance_strike), None, STRIKE_FLAGS, true)
            };
        } else if color_scale == 0.0 {
            // SAFETY: as above.
            unsafe { set_advance_func(font, face, Some(h_advance), None, LOAD_FLAGS, false) };
        } else {
            // SAFETY: as above.
            let scale = Some(color_scale);
            // SAFETY: as above.
            unsafe {
                set_advance_func(font, face, Some(h_advance_color), scale, LOAD_FLAGS, false)
            };
        }
        // QFontEngineFT::init makes bold and slanted glyphs only of a scalable face's outlines.
        // SAFETY: a live face.
        let embolden = !fixed && unsafe { synth_bold(face, request_weight, pixel_size) };
        let oblique = !fixed && italic_req && !actual_italic;
        let mut font = Font {
            _data: data,
            load_flags: if strike { STRIKE_FLAGS } else { LOAD_FLAGS },
            face,
            hb_font: font,
            pixel_size,
            ascent,
            descent,
            leading,
            glyphs: Default::default(),
            raster: 1.0,
            boxes: Default::default(),
            df: df_atlas(path, index, named, embolden, oblique, request_weight, 0),
            df_q: Default::default(),
            df_face: (path.to_string(), index, named, request_weight),
            df_tiles: std::cell::RefCell::new(lru::LruCache::new(DF_TILES)),
            embolden,
            oblique,
            color_scale,
            strike,
            request_weight,
            letter_spacing: 0,
            named,
        };
        // QFreetypeFace::computeSize: a scalable face without colour, asked for more than
        // QFontEngine::maxCachedGlyphSize() (64) pixels, is `outline_drawing`, and
        // QFontEngineFT::loadFlags loads such a glyph set with FT_LOAD_NO_HINTING.
        if !fixed && flags & ft::FT_FACE_FLAG_COLOR == 0 && sz > MAX_HINTED_SIZE64 {
            font.set_no_hinting();
        }
        Ok(font)
    }

    /// Whether glyphs are outlines (what a distance field is made of).
    fn has_outlines(&self) -> bool {
        self.color_scale == 0.0 && !self.strike
    }

    fn set_full_hinting(&mut self, design: bool) {
        if self.color_scale > 0.0 || self.strike {
            return;
        }
        self.load_flags = FULL_HINT_FLAGS;
        // HintFull rounds the height metrics to whole pixels (FontMetrics: 14.48/4.28 -> 14/4).
        let r = |v: i64| (v + 32) & -64;
        let (a, d, l) = (self.ascent, self.descent, self.leading);
        (self.ascent, self.descent) = (r(a), r(d));
        self.leading = r(a + d + l) - self.ascent - self.descent;
        let func = if design {
            h_advance_design
        } else {
            h_advance_full
        };
        // SAFETY: `hb_font` and `face` are this font's; `Drop` destroys the font before the face.
        unsafe {
            set_advance_func(
                self.hb_font,
                self.face,
                Some(func),
                None,
                FULL_HINT_FLAGS,
                false,
            )
        };
    }

    /// Glyphs loaded without hinting, for drawing and for the outline points HarfBuzz asks for.
    fn set_no_hinting(&mut self) {
        // A strike is drawn as it is (STRIKE_FLAGS), with its own advances.
        if self.strike {
            return;
        }
        self.load_flags = NO_HINT_FLAGS;
        if self.color_scale > 0.0 {
            return;
        }
        // SAFETY: `hb_font` and `face` are this font's; `Drop` destroys the font before the face.
        unsafe {
            set_advance_func(
                self.hb_font,
                self.face,
                Some(h_advance),
                None,
                NO_HINT_FLAGS,
                false,
            )
        };
    }

    /// QFontEngineFT::boundingBox(glyph), metrics only: integer-pixel (left, width, advance).
    pub fn glyph_box(&self, gid: u32) -> (i64, i64, i64) {
        let key = (gid, self.load_flags);
        if let Some(&b) = self.boxes.borrow().get(&key) {
            return b;
        }
        let b = self.load_glyph_box(gid);
        self.boxes.borrow_mut().insert(key, b);
        b
    }

    fn load_glyph_box(&self, gid: u32) -> (i64, i64, i64) {
        if self.color_scale > 0.0 {
            return self.color_glyph_box(gid);
        }
        // SAFETY: `self.face` is this font's live face, used by this thread only (`Font` is `!Send`).
        let Some(slot) = (unsafe { load_glyph(self.face, gid, self.load_flags) }) else {
            return (0, 0, 0);
        };
        if self.embolden {
            // SAFETY: the slot just loaded.
            unsafe { ft::FT_GlyphSlot_Embolden(slot) };
        }
        if self.oblique {
            // SAFETY: the slot just loaded.
            unsafe { ft::FT_GlyphSlot_Oblique(slot) };
        }
        // SAFETY: the slot just loaded (and emboldened or sheared).
        let slot = unsafe { &*slot };
        let m = slot.metrics;
        let (mut left, mut right) = (m.horiBearingX, (m.horiBearingX + m.width));
        if self.oblique {
            // QFontEngineFT::loadGlyph: oblique leaves the slot metrics alone, so Qt
            // shears the bounding box itself with xy = 0x6000.
            let (top, bottom) = (m.horiBearingY, (m.horiBearingY - m.height));
            let shear = |x: i64, y: i64| x + ((y * 0x6000) >> 16);
            left = shear(left, top).min(shear(left, bottom));
            right = shear(right, top).max(shear(right, bottom));
        }
        let left = left & -64;
        let right = (right + 63) & -64;
        let adv = (slot.advance.x + 32) >> 6;
        (left >> 6, (right - left) >> 6, adv)
    }

    /// The glyph's box in whole pixels from its metrics, (left, top, width, height), as
    /// QFontEngineFT::loadGlyph(fetchMetricsOnly) keeps it: a colour strike's in the strike's
    /// own pixels.
    fn metrics_box(&self, gid: u32) -> Option<(i64, i64, i64, i64)> {
        let flags = if self.color_scale > 0.0 {
            ft::FT_LOAD_COLOR
        } else {
            self.load_flags
        };
        // SAFETY: `self.face` is this font's live face, used by this thread only (`Font` is `!Send`).
        let slot = unsafe { load_glyph(self.face, gid, flags) }?;
        if self.embolden {
            // SAFETY: the slot just loaded.
            unsafe { ft::FT_GlyphSlot_Embolden(slot) };
        }
        // SAFETY: the slot just loaded (and emboldened).
        let m = unsafe { &*slot }.metrics;
        let (mut left, mut right) = (m.horiBearingX, m.horiBearingX + m.width);
        let (top, bottom) = (m.horiBearingY, m.horiBearingY - m.height);
        if self.oblique {
            // transformBoundingBox with xy = 0x6000, as in `load_glyph_box`.
            let shear = |x: i64, y: i64| x + ((y * 0x6000) >> 16);
            (left, right) = (
                shear(left, top).min(shear(left, bottom)),
                shear(right, top).max(shear(right, bottom)),
            );
        }
        let (left, right) = (left & -64, (right + 63) & -64);
        let (top, bottom) = ((top + 63) & -64, bottom & -64);
        Some((
            left >> 6,
            top >> 6,
            (right - left) >> 6,
            (top - bottom) >> 6,
        ))
    }

    /// QFontEngineFT::boundingBox(glyphs) of one shaped run (`shape_run`), 26.6: the line's
    /// height at least, the glyphs' boxes at their whole pixels, and the run's advance.
    fn bounding_box(&self, glyphs: &[RunGlyph], advance: i64) -> GlyphBox {
        let color = self.color_scale > 0.0;
        let (ascent, descent) = if color {
            // SAFETY: a live face.
            let size = unsafe { (*self.face).size };
            // SAFETY: the face's size object, with a strike selected (`open_sized`).
            let m = unsafe { &(*size).metrics };
            (m.ascender, -m.descender)
        } else {
            (self.ascent, self.descent)
        };
        let mut b = GlyphBox {
            y: -ascent,
            height: ascent + descent,
            xoff: advance,
            ..GlyphBox::default()
        };
        let (mut xmax, mut ymax) = (0i64, 0i64);
        for &(gid, x, y, adv) in glyphs {
            // What shaping left without an advance is not counted.
            if adv == 0 {
                continue;
            }
            let Some((left, top, width, height)) = self.metrics_box(gid) else {
                continue;
            };
            let (x, y) = (x + (left << 6), y - (top << 6));
            b.x = b.x.min(x);
            b.y = b.y.min(y);
            xmax = xmax.max(((x + 63) & -64) + (width << 6));
            ymax = ymax.max(((y + 63) & -64) + (height << 6));
        }
        b.height = b.height.max(ymax - b.y);
        b.width = xmax - b.x;
        if color {
            // QFontEngineFT::scaledBitmapMetrics: the box and the advance times the strike's
            // scale; the advance already is in scaled pixels and is scaled once more.
            let s = |v: i64| (v as f64 / 64.0 * self.color_scale * 64.0) as i64;
            b = GlyphBox {
                x: s(b.x),
                y: s(b.y),
                width: s(b.width),
                height: s(b.height),
                xoff: s(b.xoff),
            };
        }
        b
    }

    fn color_glyph_box(&self, gid: u32) -> (i64, i64, i64) {
        // SAFETY: `self.face` is this font's live face, used by this thread only (`Font` is `!Send`).
        let Some(slot) = (unsafe { load_glyph(self.face, gid, ft::FT_LOAD_COLOR) }) else {
            return (0, 0, 0);
        };
        // SAFETY: the slot just loaded.
        let slot = unsafe { &*slot };
        let s = self.color_scale / 64.0;
        let m = slot.metrics;
        let left = (m.horiBearingX as f64 * s).floor() as i64;
        let right = ((m.horiBearingX + m.width) as f64 * s).ceil() as i64;
        let adv = (slot.advance.x as f64 * s).round() as i64;
        (left, right - left, adv)
    }

    /// QFontEngineFT::bitmapForGlyph for scalable bitmap fonts: the strike's BGRA bitmap
    /// smoothly scaled by `color_scale`, premultiplied RGBA.
    fn color_glyph(&self, gid: u32) -> Option<Glyph> {
        // SAFETY: `self.face` is this font's live face, used by this thread only (`Font` is `!Send`).
        let slot = unsafe { load_glyph(self.face, gid, ft::FT_LOAD_COLOR) }?;
        // SAFETY: the slot just loaded.
        let slot = unsafe { &*slot };
        // SAFETY: the slot's bitmap, live until the face loads another glyph.
        let bm = unsafe { Rows::of(&slot.bitmap) }?;
        if bm.pixels != Pixels::Bgra {
            return None;
        }
        let (w, h) = (bm.width, bm.rows);
        let mut src = Vec::with_capacity(w * h * 4);
        for line in bm.iter() {
            for px in line.as_chunks::<4>().0 {
                src.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
            }
        }
        let s = self.color_scale;
        let (dw, dh) = (
            ((w as f64 * s).round() as usize).max(1),
            ((h as f64 * s).round() as usize).max(1),
        );
        // QTextureGlyphCache::populate places the bitmap at the whole pixels of
        // alphaMapBoundingBox (scaledBitmapMetrics): QFixed::fromReal, then truncate(), which
        // rounds down, so a scaled bitmap never starts below or right of its place.
        let whole = |v: f64| ((v * 64.0) as i64) >> 6;
        Some(Glyph {
            width: dw,
            rows: dh,
            left: whole(slot.bitmap_left as f64 * s),
            top: -whole(-(slot.bitmap_top as f64) * s),
            buf: scale_rgba(&src, w, h, dw, dh),
            color: true,
            key: Default::default(),
        })
    }

    pub fn has_char(&self, c: char) -> bool {
        // SAFETY: a live face.
        unsafe { ft::FT_Get_Char_Index(self.face, c as _) != 0 }
    }

    /// QFontEngine::supportsScript for a script that needs OpenType shaping: the face has an
    /// AAT `mort` or `morx` table, or its `GSUB` has the script.
    fn supports_script(&self, script: hb::hb_script_t) -> bool {
        // SAFETY: a live face.
        let has = |t: &[u8; 4]| unsafe { sfnt_table(self.face, t) }.is_some();
        if has(b"mort") || has(b"morx") {
            return true;
        }
        let mut tags = [0 as hb::hb_tag_t; 3];
        let mut count = tags.len() as u32;
        // SAFETY: `tags` has room for the `count` tags HarfBuzz may write; no language asked.
        unsafe {
            hb::hb_ot_tags_from_script_and_language(
                script,
                ptr::null(),
                &mut count,
                tags.as_mut_ptr(),
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };
        let gsub = u32::from_be_bytes(*b"GSUB");
        // SAFETY: this font's live `hb_font`.
        let face = unsafe { hb::hb_font_get_face(self.hb_font) };
        // SAFETY: the face of a live font; `count` tags of `tags` are set.
        unsafe {
            hb::hb_ot_layout_table_select_script(
                face,
                gsub,
                count,
                tags.as_ptr(),
                ptr::null_mut(),
                ptr::null_mut(),
            ) != 0
        }
    }

    pub fn family(&self) -> String {
        // SAFETY: a live face.
        let p = unsafe { (*self.face).family_name };
        // SAFETY: `family_name` of a live face.
        unsafe { face_string(p) }
    }

    /// FT_Face::style_name of Qt's face, which QFontEngineFT::init copies into fontDef.styleName.
    /// Qt opens a variable font at the named instance, whose name FreeType then gives; ours is
    /// opened at the default instance, so the name is read from a face opened as Qt's.
    fn style_name(&self) -> String {
        // SAFETY: a live face.
        let rec = unsafe { &*self.face };
        // SAFETY: `style_name` of a live face.
        let own = || unsafe { face_string(rec.style_name) };
        let Some(j) = self.named else {
            return own();
        };
        let index = (rec.face_index & 0xffff) | ((j as ft::FT_Long + 1) << 16);
        let mut face = ptr::null_mut();
        let opened = with_library(|lib| {
            // SAFETY: `_data` outlives the face, which is closed below; `face` is an out pointer.
            unsafe {
                ft::FT_New_Memory_Face(
                    lib,
                    self._data.as_ptr(),
                    self._data.len() as _,
                    index,
                    &mut face,
                )
            }
        });
        if opened != 0 {
            return own();
        }
        // SAFETY: the face just opened.
        let p = unsafe { (*face).style_name };
        // SAFETY: `style_name` of a live face.
        let name = unsafe { face_string(p) };
        // SAFETY: the face opened above, closed once under the library lock.
        with_library(|_| unsafe { ft::FT_Done_Face(face) });
        name
    }

    /// QFontMetricsF::height() rounded up like QTextLine.
    pub fn line_height(&self) -> f64 {
        ((self.ascent + self.descent + 63) >> 6) as f64
    }

    /// QFontEngineFT::boundingBox(glyph), 26.6: the glyph's box in whole pixels from the pen on
    /// the baseline, and its advance in whole pixels; a colour strike's times its scale.
    fn glyph_metrics(&self, gid: u32) -> Option<GlyphBox> {
        let (left, top, width, height) = self.metrics_box(gid)?;
        if self.color_scale > 0.0 {
            // SAFETY: `self.face` is this font's live face, used by this thread only.
            let slot = unsafe { load_glyph(self.face, gid, ft::FT_LOAD_COLOR) }?;
            // SAFETY: the slot just loaded.
            let advance = (unsafe { &*slot }.advance.x + 32) >> 6;
            // scaledBitmapMetrics: QFixed::fromReal of the scaled reals.
            let s = |v: i64| (v as f64 * self.color_scale * 64.0) as i64;
            return Some(GlyphBox {
                x: s(left),
                y: s(-top),
                width: s(width),
                height: s(height),
                xoff: s(advance),
            });
        }
        Some(GlyphBox {
            x: left << 6,
            y: -top << 6,
            width: width << 6,
            height: height << 6,
            xoff: self.glyph_box(gid).2 << 6,
        })
    }

    fn char_metrics(&self, c: char) -> GlyphBox {
        // SAFETY: a live face.
        let gid = unsafe { ft::FT_Get_Char_Index(self.face, c as _) };
        self.glyph_metrics(gid).unwrap_or(GlyphBox {
            x: 0,
            y: 0,
            ..GlyphBox::default()
        })
    }

    /// The face's `OS/2` table when it has the fields of version 2 (sxHeight, sCapHeight).
    fn os2(&self) -> Option<Vec<u8>> {
        // SAFETY: a live face.
        unsafe { sfnt_table(self.face, b"OS/2") }.filter(|t| t.len() >= 90)
    }

    /// (x_ppem, y_ppem, y_scale, max_advance) of the face's size, and QFontEngineFT::
    /// emSquareSize: the design units of an em, or the strike's pixels of a face without outlines.
    fn size_metrics(&self) -> ((i64, i64, i64, i64), i64) {
        // SAFETY: a live face.
        let f = unsafe { &*self.face };
        // SAFETY: the face's active size, set when the font was opened.
        let m = unsafe { &(*f.size).metrics };
        let em = if f.face_flags & ft::FT_FACE_FLAG_SCALABLE != 0 {
            f.units_per_EM as i64
        } else {
            m.y_ppem as i64
        };
        (
            (m.x_ppem as i64, m.y_ppem as i64, m.y_scale, m.max_advance),
            em,
        )
    }

    /// QFontEngineFT::xHeight in pixels: `OS/2` sxHeight, or the height of the glyph of 'x'.
    pub fn x_height(&self) -> f64 {
        let ((_, y_ppem, ..), em) = self.size_metrics();
        let sx = self
            .os2()
            .map_or(0, |t| i16::from_be_bytes([t[86], t[87]]) as i64);
        if sx != 0 && em != 0 {
            return qfixed_div(sx * y_ppem * 64, em * 64) as f64 / 64.0;
        }
        self.char_metrics('x').height as f64 / 64.0
    }

    /// QFontEngineFT::capHeight in pixels: `OS/2` sCapHeight, or the height of the glyph of 'H'.
    pub fn cap_height(&self) -> f64 {
        let ((_, _, y_scale, _), _) = self.size_metrics();
        match self.os2() {
            Some(t) => {
                let cap = i16::from_be_bytes([t[88], t[89]]) as i64;
                // SAFETY: plain arithmetic of FreeType.
                unsafe { ft::FT_MulFix(cap, y_scale) as f64 / 64.0 }
            }
            None => self.char_metrics('H').height as f64 / 64.0,
        }
    }

    /// QFontEngineFT::averageCharWidth in pixels: `OS/2` xAvgCharWidth, or the advance of 'x'.
    pub fn average_char_width(&self) -> f64 {
        let ((x_ppem, ..), em) = self.size_metrics();
        // SAFETY: a live face.
        let avg = unsafe { sfnt_table(self.face, b"OS/2") }
            .filter(|t| t.len() >= 4)
            .map_or(0, |t| i16::from_be_bytes([t[2], t[3]]) as i64);
        if avg != 0 && em != 0 {
            return qfixed_div(avg * x_ppem * 64, em * 64) as f64 / 64.0;
        }
        self.char_metrics('x').xoff as f64 / 64.0
    }

    /// QFontEngineFT::maxCharWidth in pixels.
    pub fn max_char_width(&self) -> f64 {
        let ((.., max_advance), _) = self.size_metrics();
        if self.color_scale > 0.0 {
            return ((max_advance as f64 * self.color_scale) as i64) as f64 / 64.0;
        }
        max_advance as f64 / 64.0
    }

    /// QFontEngine::minLeftBearing and minRightBearing in pixels: those `hhea` names, or the
    /// least of a few characters that tend to reach past their advance.
    pub fn min_bearings(&self) -> (f64, f64) {
        let (_, em) = self.size_metrics();
        let (mut left, mut right) = (None, None);
        // SAFETY: a live face.
        if let Some(t) = unsafe { sfnt_table(self.face, b"hhea") }.filter(|t| t.len() >= 16)
            && em != 0
        {
            let factor = self.pixel_size as f64 / em as f64;
            let value = |o: usize| {
                let v = i16::from_be_bytes([t[o], t[o + 1]]) as i64;
                (v.abs() < 4 * em).then_some(v as f64 * factor)
            };
            (left, right) = (value(12), value(14));
        }
        if let (Some(left), Some(right)) = (left, right) {
            return (left, right);
        }
        let (mut left, mut right) = (f64::MAX, f64::MAX);
        const SUBSET: [u32; 19] = [
            40, 67, 70, 75, 86, 88, 89, 93, 95, 102, 114, 124, 127, 205, 645, 884, 922, 1070, 12386,
        ];
        for c in SUBSET {
            // SAFETY: a live face.
            let gid = unsafe { ft::FT_Get_Char_Index(self.face, c as _) };
            if gid == 0 {
                continue;
            }
            let Some(m) = self.glyph_metrics(gid) else {
                continue;
            };
            if m.width == 0 || m.height == 0 {
                continue;
            }
            left = left.min(m.x as f64 / 64.0);
            right = right.min((m.xoff - m.x - m.width) as f64 / 64.0);
        }
        (left, right)
    }

    /// QFontEngineFT::init: (underlinePosition, lineThickness) in 26.6.
    pub fn underline(&self) -> (i64, i64) {
        // SAFETY: a live face.
        let f = unsafe { &*self.face };
        if f.face_flags & ft::FT_FACE_FLAG_SCALABLE == 0
            || f.face_flags & ft::FT_FACE_FLAG_COLOR != 0
        {
            // A face without outlines, or with colour, names no underline: Qt's "ad hoc
            // algorithm" of the weight and the pixel size asked for, in whole pixels.
            let score = self.request_weight as i64 * self.pixel_size as i64;
            let th = match score / 7000 {
                th if th < 2 && score >= 1050 => 2,
                th => th,
            };
            // QFixed / int truncates to 1/64.
            return (((th * 2 + 3) << 6) / 6, th.max(1) << 6);
        }
        // SAFETY: the face's active size, set when the font was opened.
        let ys = unsafe { (*f.size).metrics.y_scale };
        // SAFETY: plain arithmetic of FreeType.
        let mul = |v: i64| unsafe { ft::FT_MulFix(v as _, ys) } as i64;
        let th = mul(f.underline_thickness as i64);
        let center = -mul(f.underline_position as i64);
        (center - th / 2, th.max(64))
    }

    /// Shape a line set in this font alone; returns (glyph, x_pos 26.6 accumulated, y_offset
    /// 26.6) in visual order and the total advance.
    pub fn shape(&self, text: &str) -> (Vec<(u32, i64, i64)>, i64) {
        let runs = itemize(text, None, |_, _, _, _| 0);
        let (glyphs, _, advance) = place_runs(text, &runs, |_| self);
        let glyphs = glyphs.into_iter().map(|(_, g, x, y)| (g, x, y)).collect();
        (glyphs, advance)
    }

    /// QTextEngine::shapeTextWithHarfbuzzNG for one run of the line `text`: HarfBuzz gets the
    /// run's script and direction, and the whole line as the context around the run (what joins
    /// Arabic letters across a change of font). The glyphs come out left to right, each at
    /// its (x, y) from the run's left end and with its advance, with the number of glyphs of
    /// the run's last character. `clusters` gets, for every glyph from the left, the byte its
    /// HarfBuzz cluster starts at, its advance and the glyph.
    fn shape_run(
        &self,
        text: &str,
        run: &Run,
        clusters: Option<&mut Vec<(usize, i64, u32)>>,
    ) -> (Vec<RunGlyph>, usize, i64) {
        let (glyphs, _, last_cluster, advance) = self.shape_run_clusters(text, run, clusters);
        (glyphs, last_cluster, advance)
    }

    /// `shape_run`, and after the glyphs the byte of `text` each glyph's cluster starts at.
    fn shape_run_clusters(
        &self,
        text: &str,
        run: &Run,
        mut clusters: Option<&mut Vec<(usize, i64, u32)>>,
    ) -> (Vec<RunGlyph>, Vec<u32>, usize, i64) {
        // SAFETY: creates an empty buffer.
        let buf = unsafe { hb::hb_buffer_create() };
        // SAFETY: `buf` is ours and the functions live for the whole process. The engine's
        // Unicode data in place of HarfBuzz's own, as QTextEngine sets Qt's.
        unsafe { hb::hb_buffer_set_unicode_funcs(buf, crate::hb_unicode::funcs()) };
        // SAFETY: `buf` is ours; HarfBuzz copies the run's bytes and up to five characters on
        // each side of it, all inside the `text.len()` UTF-8 bytes of `text`.
        unsafe {
            hb::hb_buffer_add_utf8(
                buf,
                text.as_ptr() as *const _,
                text.len() as i32,
                run.start as u32,
                (run.end - run.start) as i32,
            )
        };
        let direction = if run.level.is_rtl() {
            hb::HB_DIRECTION_RTL
        } else {
            hb::HB_DIRECTION_LTR
        };
        // SAFETY: `buf` is ours.
        unsafe { hb::hb_buffer_set_direction(buf, direction) };
        // SAFETY: `buf` is ours.
        unsafe { hb::hb_buffer_set_script(buf, run.script) };
        // SAFETY: the process's default language, a constant of HarfBuzz.
        let language = unsafe { hb::hb_language_get_default() };
        // SAFETY: `buf` is ours.
        unsafe { hb::hb_buffer_set_language(buf, language) };
        let feature = |name: &str, value: u32| hb::hb_feature_t {
            tag: tag(name),
            value,
            start: 0,
            end: u32::MAX,
        };
        let mut features = vec![feature("kern", 1)];
        // QTextEngine::shapeTextWithHarfbuzzNG: letters set apart are not joined into
        // ligatures, except in the scripts that cannot be written without them.
        if self.letter_spacing != 0 && !opentype_script(run.script) {
            features.extend(["liga", "clig", "dlig", "hlig"].map(|name| feature(name, 0)));
        }
        // SAFETY: this font's live `hb_font`, our buffer and its features.
        unsafe { hb::hb_shape(self.hb_font, buf, features.as_ptr(), features.len() as u32) };
        let mut n = 0u32;
        // SAFETY: `buf` is ours; `n` is an out value.
        let info = unsafe { hb::hb_buffer_get_glyph_infos(buf, &mut n) };
        // SAFETY: `info` holds the buffer's `n` glyphs until the buffer is destroyed.
        let info = unsafe { c_slice(info, n as usize) };
        // SAFETY: as for `info`.
        let pos = unsafe { hb::hb_buffer_get_glyph_positions(buf, &mut n) };
        // SAFETY: as for `info`.
        let pos = unsafe { c_slice(pos, n as usize) };
        let mut out = Vec::with_capacity(n as usize);
        let mut x = 0i64;
        for (k, (gi, &p)) in info.iter().zip(pos).enumerate() {
            let mut p = p;
            // QTextEngine::shapeGlyphsWithHarfBuzz: engines without horizontal subpixel
            // positions (HintFull) round advances and x offsets to whole pixels.
            if self.load_flags == FULL_HINT_FLAGS {
                p.x_advance = (p.x_advance + 32) & -64;
                p.x_offset = (p.x_offset + 32) & -64;
            }
            // The font's y scale is negative, so y offsets already point down, as Qt keeps them.
            let at = (x + p.x_offset as i64, p.y_offset as i64);
            x += p.x_advance as i64;
            // QTextEngine::shapeText: the spacing follows the last glyph of every character
            // (cluster) in reading order; HarfBuzz lists a right-to-left run's glyphs reversed.
            let after = if run.level.is_rtl() {
                k.checked_sub(1)
            } else {
                Some(k + 1)
            };
            if self.letter_spacing != 0
                && after.and_then(|a| info.get(a)).map(|a| a.cluster) != Some(gi.cluster)
            {
                x += self.letter_spacing;
            }
            let advance = x - (at.0 - p.x_offset as i64);
            if let Some(clusters) = clusters.as_deref_mut() {
                clusters.push((gi.cluster as usize, advance, gi.codepoint));
            }
            out.push((gi.codepoint, at.0, at.1, advance));
        }
        // The glyphs of the run's last character: HarfBuzz lists the glyphs from the left, so
        // they come last in text set left to right and first in text set right to left.
        let same = |a: Option<&hb::hb_glyph_info_t>, b: &hb::hb_glyph_info_t| {
            a.is_some_and(|a| a.cluster == b.cluster)
        };
        let last = if run.level.is_rtl() {
            info.first()
        } else {
            info.last()
        };
        // QTextLine::layout_helper: a cluster that starts with a space that breaks is space
        // as a whole, the marks on it too, and a line that ends in space has no bearing to
        // add (whiteSpaceOrObject); none of its glyphs is the last character's then.
        let space = last
            .and_then(|i| text.get(i.cluster as usize..)?.chars().next())
            .is_some_and(|c| c.is_whitespace() && !matches!(c, '\u{A0}' | '\u{2007}' | '\u{202F}'));
        let last_cluster = if space {
            0
        } else if run.level.is_rtl() {
            info.iter().take_while(|i| same(last, i)).count()
        } else {
            info.iter().rev().take_while(|i| same(last, i)).count()
        };
        let starts = info.iter().map(|i| i.cluster).collect();
        // SAFETY: ours, destroyed once; `info` and `pos` are not used past here.
        unsafe { hb::hb_buffer_destroy(buf) };
        (out, starts, last_cluster, x)
    }

    /// QFontEngine::subPixelPositionFor with 4 positions, in 26.6.
    pub(crate) fn subpixel(x: i64) -> i64 {
        if x == 0 {
            return 0;
        }
        let v = x - (x & -64) + 1;
        let fraction = (v * 4) & -64;
        fraction / 4
    }

    /// Draw text whose line origin is at local (x, y) inside a node translated by (tx, ty).
    /// Glyph quads are built in node space, then snapped like textmask.vert: floor(p + 0.5).
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &self,
        cv: &mut Canvas,
        text: &str,
        tx: f64,
        ty: f64,
        x: f64,
        y: f64,
        color: [u8; 4],
        opacity: f32,
    ) {
        let (glyphs, _) = self.shape(text);
        self.draw_glyphs(
            cv,
            &glyphs,
            tx,
            ty,
            x,
            y + self.ascent as f64 / 64.0,
            color,
            opacity,
        );
    }

    /// Draw shaped glyphs (x/y offsets 26.6, relative to `x`/`baseline` in node space).
    #[allow(clippy::too_many_arguments)]
    pub fn draw_glyphs(
        &self,
        cv: &mut Canvas,
        glyphs: &[(u32, i64, i64)],
        tx: f64,
        ty: f64,
        x: f64,
        baseline: f64,
        color: [u8; 4],
        opacity: f32,
    ) {
        let a = color[3] as f32 / 255.0 * opacity;
        let c = [
            color[0] as f32 / 255.0 * a,
            color[1] as f32 / 255.0 * a,
            color[2] as f32 / 255.0 * a,
            a,
        ];
        let mut quads = Vec::new();
        self.glyph_quads(glyphs, tx, ty, x, baseline, &mut quads);
        blit_glyphs(cv, &quads, c);
    }

    /// Device positions of shaped glyphs, exactly as `draw_glyphs` places them.
    #[allow(clippy::too_many_arguments)]
    pub fn glyph_quads(
        &self,
        glyphs: &[(u32, i64, i64)],
        tx: f64,
        ty: f64,
        x: f64,
        baseline: f64,
        out: &mut Vec<GlyphQuad>,
    ) {
        for &(gid, gx, gy) in glyphs {
            let px = x + gx as f64 / 64.0;
            let py = baseline + gy as f64 / 64.0;
            let fx = (px * 64.0).round() as i64;
            let Some(g) = self.glyph(gid, Self::subpixel(fx)) else {
                continue;
            };
            let lx = (fx as f64 / 64.0).floor() + g.left as f64;
            let ly = qround(py) - g.top as f64;
            let x0 = ((lx + tx) as f32 + 0.5).floor() as i64;
            let y0 = ((ly + ty) as f32 + 0.5).floor() as i64;
            out.push((g, x0, y0));
        }
    }

    /// `glyph_quads` before the final snap to device pixels.
    pub fn glyph_quads_f(
        &self,
        glyphs: &[(u32, i64, i64)],
        tx: f64,
        ty: f64,
        x: f64,
        baseline: f64,
        out: &mut Vec<GlyphQuadF>,
    ) {
        for &(gid, gx, gy) in glyphs {
            let px = x + gx as f64 / 64.0;
            let py = baseline + gy as f64 / 64.0;
            let fx = (px * 64.0).round() as i64;
            let Some(g) = self.glyph(gid, Self::subpixel(fx)) else {
                continue;
            };
            let lx = (fx as f64 / 64.0).floor() + g.left as f64;
            let ly = qround(py) - g.top as f64;
            out.push((g, lx + tx, ly + ty));
        }
    }

    /// QFontEngineFT::addGlyphsToPath: a glyph's outline in pixels, y down from the baseline.
    pub fn outline(&self, gid: u32) -> Vec<crate::pathsimplify::PathEl> {
        if self.color_scale > 0.0 {
            return vec![];
        }
        // SAFETY: a live face, used by this thread only; `outline` restores its char size.
        unsafe {
            crate::dftext::outline(self.face, gid, self.pixel_size, self.embolden, self.oblique)
        }
    }

    /// The glyph cache of a render type quality (QSGRenderContext::distanceFieldGlyphCache:
    /// one per font and quality; none asked for is 0).
    fn df_of(&self, quality: i32) -> DfShared {
        if quality <= 0 {
            return self.df.clone();
        }
        let mut q = self.df_q.borrow_mut();
        if let Some(a) = q.iter().find(|a| a.0 == quality) {
            return a.1.clone();
        }
        let (path, index, named, weight) = &self.df_face;
        let a = df_atlas(
            path,
            *index,
            *named,
            self.embolden,
            self.oblique,
            *weight,
            quality,
        );
        q.push((quality, a.clone()));
        a
    }

    /// QSGDistanceFieldGlyphCache::populate for one glyph node's glyphs, in run order.
    pub fn df_populate(&self, gids: &[u32], quality: i32) {
        let df = self.df_of(quality);
        let mut df = df.borrow_mut();
        let base = df.base();
        for &g in gids {
            // SAFETY: a live face, used by this thread only; `build` restores its char size.
            let build = || unsafe { crate::dftext::build(self.face, g, self.pixel_size, base) };
            df.field(g, build);
        }
        df.populate(gids);
    }

    /// Glyph `gid` has a distance field to draw: an outline with an area.
    pub fn df_has_field(&self, gid: u32, quality: i32) -> bool {
        let df = self.df_of(quality);
        let mut df = df.borrow_mut();
        let base = df.base();
        // SAFETY: a live face, used by this thread only; `build` restores its char size.
        let build = || unsafe { crate::dftext::build(self.face, gid, self.pixel_size, base) };
        df.field(gid, build).is_some_and(|f| f.w > 0)
    }

    /// `Text.QtRendering` tiles of shaped glyphs (unsnapped positions, like
    /// QSGDistanceFieldGlyphNode), placed by `df_populate`. `tx`/`ty` is the item's
    /// translation, `x`/`baseline` the pen in the item, `view` the size drawn into.
    #[allow(clippy::too_many_arguments)]
    pub fn df_tiles(
        &self,
        glyphs: &[(u32, i64, i64)],
        quality: i32,
        tx: f64,
        ty: f64,
        x: f64,
        baseline: f64,
        color: [f32; 4],
        look: crate::dftext::DfLook,
        turn: Option<crate::dftext::DfTurn>,
        view: crate::dftext::DfView,
        out: &mut Vec<crate::dftext::DfTile>,
    ) {
        let df = self.df_of(quality);
        let df = df.borrow();
        let shift = [tx as f32, ty as f32];
        for &(gid, gx, gy) in glyphs {
            let place = crate::dftext::DfPlace {
                px: self.pixel_size as f64,
                pen: [x + gx as f64 / 64.0, baseline + gy as f64 / 64.0],
                shift,
                turn,
                view,
                color,
                look,
            };
            if !df.in_view(gid, &place) {
                continue;
            }
            let key = (
                quality.max(0),
                gid,
                place.pen.map(f64::to_bits),
                shift.map(f32::to_bits),
                turn.map(|t| t.bits()),
                view.bits(),
                color.map(f32::to_bits),
                look.bits(),
                df.state(),
            );
            let hit = self.df_tiles.borrow_mut().get(&key).cloned();
            let tile = match hit {
                Some(t) => t,
                None => {
                    let t = df.render(gid, &place);
                    self.df_tiles.borrow_mut().put(key, t.clone());
                    t
                }
            };
            out.extend(tile);
        }
    }

    /// Glyphs that are the file's bitmaps (a colour font, a bitmap strike), not outlines.
    fn bitmaps(&self) -> bool {
        self.color_scale > 0.0 || self.strike
    }

    fn glyph(&self, gid: u32, sp: i64) -> Option<std::rc::Rc<Glyph>> {
        if let Some(g) = self.glyphs.borrow().get(&(gid, sp)) {
            return g.clone();
        }
        // A bitmap strike has one bitmap per glyph, drawn at whole pixels.
        if (self.color_scale > 0.0 || self.strike) && sp != 0 {
            return self.glyph(gid, 0);
        }
        let g = if self.color_scale > 0.0 {
            self.color_glyph(gid).map(std::rc::Rc::new)
        } else if self.raster != 1.0 {
            // QTransformToFTMatrix(QTransform::fromScale(dpr, dpr)); a transform that scales
            // turns hinting off (QFontEngineFT::loadGlyphFor).
            let k = (self.raster * 65536.0) as i64;
            self.render_xf(gid, sp, [k, 0, 0, k], NO_HINT_FLAGS)
                .map(std::rc::Rc::new)
        } else {
            let mut v = ft::FT_Vector { x: sp as _, y: 0 };
            // SAFETY: `self.face` is this font's live face, used by this thread only (`Font` is `!Send`); FreeType copies the delta.
            unsafe { ft::FT_Set_Transform(self.face, ptr::null_mut(), &mut v) };
            // SAFETY: as above.
            let slot = unsafe { load_glyph(self.face, gid, self.load_flags) };
            let slot = slot.filter(|&slot| {
                if self.embolden {
                    // SAFETY: the slot just loaded.
                    unsafe { ft::FT_GlyphSlot_Embolden(slot) };
                }
                if self.oblique {
                    // SAFETY: the slot just loaded.
                    unsafe { ft::FT_GlyphSlot_Oblique(slot) };
                }
                // SAFETY: the slot just loaded.
                unsafe { ft::FT_Render_Glyph(slot, ft::FT_RENDER_MODE_LIGHT) == 0 }
            });
            // SAFETY: the slot just rendered.
            slot.and_then(|slot| unsafe { coverage_glyph(slot) })
                .map(std::rc::Rc::new)
        };
        self.glyphs.borrow_mut().insert((gid, sp), g.clone());
        g
    }

    /// QFontEngineFT::loadGlyphFor under a painter transform: the A8 bitmap of `gid` drawn
    /// through `m` (FT_Matrix xx, xy, yx, yy in 16.16, as QTransformToFTMatrix makes it) at the
    /// horizontal subpixel offset `sp`. Not cached.
    /// A bitmap font's glyph comes as its strike has it, 1 bit a pixel: FreeType transforms
    /// outlines only.
    pub fn glyph_xf(&self, gid: u32, sp: i64, m: [i64; 4]) -> Option<Glyph> {
        if self.color_scale > 0.0 {
            return None;
        }
        self.render_xf(gid, sp, m, self.load_flags)
    }

    fn render_xf(&self, gid: u32, sp: i64, m: [i64; 4], load_flags: i32) -> Option<Glyph> {
        let mut matrix = ft::FT_Matrix {
            xx: m[0] as _,
            xy: m[1] as _,
            yx: m[2] as _,
            yy: m[3] as _,
        };
        let transform = m != [0x10000, 0, 0, 0x10000];
        if self.oblique && transform {
            // FT_Matrix_Multiply(&matrix, &slant) with slant = (1, 0x366A; 0, 1).
            // SAFETY: FreeType's own arithmetic on plain numbers.
            matrix.xy += unsafe { ft::FT_MulFix(matrix.xx, 0x0366A) };
            // SAFETY: as above.
            matrix.yy += unsafe { ft::FT_MulFix(matrix.yx, 0x0366A) };
        }
        let mut v = ft::FT_Vector { x: sp as _, y: 0 };
        // SAFETY: `self.face` is this font's live face, used by this thread only; FreeType
        // copies the matrix and the delta.
        unsafe { ft::FT_Set_Transform(self.face, &mut matrix, &mut v) };
        // SAFETY: as above.
        let slot = unsafe { load_glyph(self.face, gid, load_flags) };
        let slot = slot.filter(|&slot| {
            if self.embolden {
                // SAFETY: the slot just loaded.
                unsafe { ft::FT_GlyphSlot_Embolden(slot) };
            }
            if self.oblique && !transform {
                // SAFETY: the slot just loaded.
                unsafe { ft::FT_GlyphSlot_Oblique(slot) };
            }
            // SAFETY: the slot just loaded.
            unsafe { ft::FT_Render_Glyph(slot, ft::FT_RENDER_MODE_NORMAL) == 0 }
        });
        // SAFETY: the slot just rendered.
        let g = slot.and_then(|slot| unsafe { coverage_glyph(slot) });
        // SAFETY: as above; the other loads of this face expect no transform.
        unsafe { ft::FT_Set_Transform(self.face, ptr::null_mut(), ptr::null_mut()) };
        g
    }
}

impl Glyph {
    /// The bitmap's left edge from the pen position and its top edge above the baseline
    /// (FT_GlyphSlot bitmap_left / bitmap_top).
    pub fn origin(&self) -> (i64, i64) {
        (self.left, self.top)
    }

    /// A bitmap of `width` by `rows` bytes of coverage whose first pixel is `left` right of
    /// the pen and `top` above the baseline.
    pub fn coverage(width: usize, rows: usize, left: i64, top: i64, buf: Vec<u8>) -> Glyph {
        assert_eq!(buf.len(), width * rows);
        Glyph {
            width,
            rows,
            left,
            top,
            buf,
            color: false,
            key: Default::default(),
        }
    }
}

/// QImage::smoothScaled making a premultiplied picture smaller both ways, which is how
/// QFontEngineFT::bitmapForGlyph fits a colour strike to the size asked for: Qt's area
/// sampling in whole numbers (qt_qimageScaleAARGBA_down_xy), a pixel's share in 1/16384,
/// every sum cut off, never rounded.
fn scale_rgba_down(src: &[u8], w: usize, h: usize, dw: usize, dh: usize) -> Vec<u8> {
    // QImageScale::qimageCalcXPoints and qimageCalcApoints: per destination index the first
    // source index, that pixel's share and the share of a whole pixel.
    let points = |n: usize, dn: usize| -> Vec<(usize, u32, u32)> {
        let inc = ((n as u64) << 16) / dn as u64;
        let whole = ((dn as u64) << 14).div_ceil(n as u64);
        (0..dn as u64)
            .map(|d| {
                let val = d * inc;
                let first = ((0x10000 - (val & 0xffff)) * whole) >> 16;
                ((val >> 16) as usize, first as u32, whole as u32)
            })
            .collect()
    };
    let (xs, ys) = (points(w, dw), points(h, dh));
    // Qt reads on past the end of a row for a share that is nothing or next to it.
    let at = |i: usize, c: usize| src[(i * 4).min(src.len() - 4) + c] as u32;
    // qt_qimageScaleAARGBA_helper: one source row's pixels under the destination pixel.
    let across = |row: usize, (x0, first, whole): (usize, u32, u32), c: usize| -> u32 {
        let mut i = row * w + x0;
        let mut sum = at(i, c) * first;
        let mut left = (1 << 14) - first;
        while left > whole {
            i += 1;
            sum += at(i, c) * whole;
            left -= whole;
        }
        sum + at(i + 1, c) * left
    };
    let mut out = vec![0u8; dw * dh * 4];
    for (dy, &(y0, first, whole)) in ys.iter().enumerate() {
        for (dx, &x) in xs.iter().enumerate() {
            for c in 0..4 {
                let mut row = y0;
                let mut sum = (across(row, x, c) >> 4).wrapping_mul(first);
                let mut left = (1 << 14) - first;
                while left > whole {
                    row += 1;
                    sum = sum.wrapping_add((across(row, x, c) >> 4).wrapping_mul(whole));
                    left -= whole;
                }
                sum = sum.wrapping_add((across(row + 1, x, c) >> 4).wrapping_mul(left));
                out[(dy * dw + dx) * 4 + c] = (sum >> 24) as u8;
            }
        }
    }
    out
}

/// Box-filtered resample of premultiplied RGBA (QImage::scaled with SmoothTransformation).
fn scale_rgba(src: &[u8], w: usize, h: usize, dw: usize, dh: usize) -> Vec<u8> {
    if dw < w && dh < h {
        return scale_rgba_down(src, w, h, dw, dh);
    }
    // Per destination index: (first source index, weights) over the source span it covers.
    let spans = |n: usize, dn: usize| -> Vec<(usize, Vec<f32>)> {
        let r = n as f64 / dn as f64;
        (0..dn)
            .map(|d| {
                let (a, b) = (d as f64 * r, ((d + 1) as f64 * r).min(n as f64));
                let s0 = a.floor() as usize;
                let ws = (s0..(b.ceil() as usize).max(s0 + 1).min(n))
                    .map(|s| ((b.min(s as f64 + 1.0) - a.max(s as f64)) / (b - a)).max(0.0) as f32)
                    .collect();
                (s0, ws)
            })
            .collect()
    };
    let (sx, sy) = (spans(w, dw), spans(h, dh));
    let mut tmp = vec![0f32; dw * h * 4];
    for y in 0..h {
        for (dx, (s0, ws)) in sx.iter().enumerate() {
            for (k, wt) in ws.iter().enumerate() {
                let p = (y * w + s0 + k) * 4;
                for c in 0..4 {
                    tmp[(y * dw + dx) * 4 + c] += src[p + c] as f32 * wt;
                }
            }
        }
    }
    let mut out = vec![0u8; dw * dh * 4];
    for (dy, (s0, ws)) in sy.iter().enumerate() {
        for x in 0..dw {
            for c in 0..4 {
                let v: f32 = ws
                    .iter()
                    .enumerate()
                    .map(|(k, wt)| tmp[((s0 + k) * dw + x) * 4 + c] * wt)
                    .sum();
                out[(dy * dw + x) * 4 + c] = v.round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    out
}

/// One texel of a glyph's bitmap blended at (`xx`, `yy`) in the premultiplied colour `c`.
#[inline]
fn glyph_px(cv: &mut Canvas, g: &Glyph, c: [f32; 4], col: usize, row: usize, xx: usize, yy: usize) {
    if g.color {
        let p = (row * g.width + col) * 4;
        if g.buf[p + 3] == 0 {
            return;
        }
        let s = [0, 1, 2, 3].map(|k| (g.buf[p + k] as f32 * c[3]).round().clamp(0.0, 255.0) as u32);
        return cv.blend_px(xx, yy, s);
    }
    let m = g.buf[row * g.width + col];
    if m == 0 {
        return;
    }
    let mf = m as f32 / 255.0;
    let s = c.map(|ch| ((ch * mf).clamp(0.0, 1.0) * 255.0).round() as u32);
    cv.blend_px(xx, yy, s);
}

fn blit_glyphs(cv: &mut Canvas, quads: &[GlyphQuad], c: [f32; 4]) {
    for (g, x0, y0) in quads {
        for row in 0..g.rows as i64 {
            let yy = y0 + row;
            if yy < 0 || yy >= cv.h as i64 {
                continue;
            }
            for col in 0..g.width as i64 {
                let xx = x0 + col;
                if xx < 0 || xx >= cv.w as i64 {
                    continue;
                }
                glyph_px(
                    cv,
                    g,
                    c,
                    col as usize,
                    row as usize,
                    xx as usize,
                    yy as usize,
                );
            }
        }
    }
}

/// A 26.6 pixel size on a device `dpr` times finer.
fn device_size64(size64: i64, dpr: f64) -> i64 {
    (size64 as f64 * dpr).round().max(64.0) as i64
}

/// textmask.vert: a corner of a glyph's quad at the nearest pixel of the screen, half a pixel
/// going up there. On an axis that runs `against` the screen's (a texture of a subtree turned
/// upside down) that is down.
pub fn snap_px(v: f32, against: bool) -> f32 {
    if against {
        (v - 0.5).ceil()
    } else {
        (v + 0.5).floor()
    }
}

/// qRound for qreal.
fn qround(v: f64) -> f64 {
    if v >= 0.0 {
        (v + 0.5).floor()
    } else {
        (v - 0.5).ceil()
    }
}

impl Drop for Font {
    fn drop(&mut self) {
        // SAFETY: this font's, destroyed once (HarfBuzz frees its `AdvCtx` with it).
        unsafe { hb::hb_font_destroy(self.hb_font) };
        // SAFETY: this font's face, no longer used by HarfBuzz, closed once under the library lock.
        with_library(|_| unsafe { ft::FT_Done_Face(self.face) });
    }
}

/// Shaped lines one stack keeps.
const SHAPED_LINES: std::num::NonZeroUsize = std::num::NonZeroUsize::new(4096).unwrap();
const SHAPED_TEXTS: std::num::NonZeroUsize = std::num::NonZeroUsize::new(512).unwrap();

/// An empty cache of `cap` entries. `LruCache::new` allocates the table for all of them at
/// once (about 150 KiB a stack for [`SHAPED_LINES`], and most stacks shape a handful of lines);
/// this one grows with what is cached.
fn lazy_lru<V>(cap: std::num::NonZeroUsize) -> lru::LruCache<String, V> {
    let mut c = lru::LruCache::unbounded();
    c.resize(cap);
    c
}

/// QFontEngineMulti replica: primary font plus fontconfig fallbacks (FcFontSort order), loaded lazily.
pub struct FontStack {
    primary: Font,
    /// The fallback families' faces and the order each script looks through them.
    families: std::rc::Rc<FcFallbacks>,
    /// The font of every face in `families.faces`: font index n is `fallbacks[n - 1]`.
    fallbacks: Vec<std::cell::OnceCell<Option<Box<Font>>>>,
    /// Font for emoji presentation (fontconfig's `emoji` family), font index fallbacks + 1:
    /// Qt 6 QFontEngineMulti puts emoji sequences on QFontDatabase's emoji family first.
    emoji: Option<(String, std::cell::OnceCell<Option<Box<Font>>>)>,
    weight: f32,
    /// The QFont weight; `weight` is the picked face's, which differs when the family has no
    /// face of the requested weight or the face was matched by styleName.
    request_weight: f32,
    /// The primary face's style name (`Font::style_name`), which picks the fallback families'
    /// faces; read when the first fallback is loaded.
    face_style: std::cell::OnceCell<String>,
    axes: Vec<(String, f32)>,
    no_hint: bool,
    /// `Font::raster` of this stack's scalable fonts (`scaled`); 1.0 otherwise.
    raster: f64,
    full_hint: bool,
    /// Full hinting under design metrics (Text.QtRendering).
    design: bool,
    /// Shaped single lines by text: layout re-measures every Text each pass. Least recently
    /// used lines go first, so changing texts (a clock) never flush the ones still shown.
    shaped: std::cell::RefCell<lru::LruCache<String, ShapedLine>>,
    /// Texts shaped as a whole (`shape_text`), by text: the ones a Text wraps or elides.
    texts: std::cell::RefCell<lru::LruCache<String, std::rc::Rc<ShapedText>>>,
    /// The primary face's file, for `scaled`.
    path: String,
    /// Pixel size in 26.6: `pixel_size`, or pixel size times a device pixel ratio (`scaled`).
    size64: i64,
    /// The same of the pixel size asked for, which the emoji font is opened at: QFontPrivate
    /// asks for it by the QFont, while the fallbacks take the size of the first font's engine
    /// (a bitmap-only font's strike).
    request_size64: i64,
    /// `Font::letter_spacing` of every font of the stack.
    spacing: i64,
    /// Twins of this stack rasterizing at a device pixel ratio, by the ratio's bits.
    scaled: std::cell::RefCell<Vec<(u64, std::rc::Rc<FontStack>)>>,
    /// A script's own stack (one of `subs`), which has none below it.
    is_sub: bool,
    /// The stack of each script of `SCRIPT_LANGUAGES` whose base font is not the primary one
    /// (`script_stack`). Its fonts have the indices from `SUB0 + slot * SUB_STRIDE` on.
    subs: [std::cell::OnceCell<Option<Box<FontStack>>>; SCRIPT_LANGUAGES.len()],
    /// The QFont's family when no installed family has that name (`requested`).
    missing: Option<String>,
}

/// Font indices from here on are the fonts of the scripts' own stacks (`FontStack::subs`).
const SUB0: usize = 1 << 20;
const SUB_STRIDE: usize = 1 << 16;

/// A glyph bitmap on its device rect (x1 y1 x2 y2).
pub type GlyphRect = (std::rc::Rc<Glyph>, [f32; 4]);

#[derive(Clone)]
pub struct ShapedLine {
    /// (font index: 0 = primary, n = fallback n-1; glyph; x; y) in 26.6
    pub glyphs: Vec<(usize, u32, i64, i64)>,
    /// Where in `glyphs` the cluster of the line's last character starts: the glyph whose
    /// right bearing counts in `FontStack::natural_width`.
    pub last_cluster: usize,
    pub advance: i64,
    pub ascent: i64,
    pub descent: i64,
    pub leading: i64,
}

impl ShapedLine {
    /// QTextDocumentLayout's QTextLine::height() (TextEdit): leading included.
    pub fn doc_height(&self) -> f64 {
        ((self.ascent + self.descent + self.leading.max(0) + 63) >> 6) as f64
    }

    /// QTextLine::height(): ceil(ascent + descent).
    pub fn height(&self) -> f64 {
        ((self.ascent + self.descent + 63) >> 6) as f64
    }
}

/// A text shaped as a whole, as QTextEngine keeps it to break it into lines: a line's width
/// is the sum of the advances of its clusters as shaped in the text (the last one's includes
/// the kerning with the next line's first), a line ends only between clusters (never inside a
/// ligature), and it is as tall as the script items it has a part of.
pub struct ShapedText {
    /// Advance in 26.6 of the clusters that start before each byte; one more for the end.
    before: Vec<i64>,
    /// The bytes a cluster starts at.
    starts: Vec<bool>,
    /// QScriptItem per run of one script and bidi level: byte range, and the ascent, descent
    /// and leading of the fonts its characters are set in.
    items: Vec<(usize, usize, [i64; 3])>,
    /// The ISO 15924 tag of the script of the item that starts at each byte.
    scripts: Vec<Option<[u8; 4]>>,
    /// How far past its advance, in 26.6, the first glyph of the cluster that starts at each
    /// byte reaches: its negative right bearing as LineBreakHelper::calculateRightBearing
    /// takes it, of the glyph the text is shaped with.
    overhangs: Vec<i64>,
    /// At the byte an item after the first starts at: how far past its advance that item's
    /// glyph reaches which is as far into it as the glyph of the character before is into its
    /// own item, -1 where the item has no such glyph. LineBreakHelper::saveCurrentGlyph reads
    /// that glyph when white space starts an item: qtbase 6.12 src/gui/text/qtextlayout.cpp,
    /// line 1736, takes `glyphs.glyphs[logClusters[currentPosition - 1]]` (currentGlyph, line
    /// 1728) with the glyphs and logClusters of the item that starts at currentPosition and
    /// the character before it, which is in the item before. Lines 2001 and 2022 call it,
    /// calculateRightBearingForPreviousGlyph (line 1766) reads what it kept.
    strays: Vec<Option<i64>>,
    /// How far past its advance each glyph of the first item reaches, and which glyph of the
    /// last item the last character has: for `stray_after`.
    first_item: Vec<i64>,
    last_glyph: usize,
}

impl ShapedText {
    /// What `strays` has for byte `at`.
    pub fn stray(&self, at: usize) -> Option<i64> {
        self.strays.get(at).copied().flatten()
    }

    /// `stray` of this text's first byte where it follows `before` in another format: its
    /// first item starts there.
    pub fn stray_after(&self, before: &ShapedText) -> Option<i64> {
        if self.first_item.is_empty() || before.first_item.is_empty() {
            return None;
        }
        Some(
            self.first_item
                .get(before.last_glyph)
                .copied()
                .unwrap_or(-1),
        )
    }

    /// The bytes of the text.
    pub fn len(&self) -> usize {
        self.before.len() - 1
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The advance of the clusters that start in `text[a..b]`.
    pub fn advance(&self, a: usize, b: usize) -> i64 {
        self.before[b] - self.before[a]
    }

    /// How far past its advance the first glyph of the cluster that starts at byte `at` reaches.
    pub fn overhang(&self, at: usize) -> i64 {
        self.overhangs.get(at).copied().unwrap_or(0)
    }

    /// A cluster starts at byte `at`, or the text ends there.
    pub fn cluster_start(&self, at: usize) -> bool {
        at + 1 == self.before.len() || self.starts[at]
    }

    /// The script, as its ISO 15924 tag, of the item that starts at byte `at`: QTextEngine
    /// tailors the line break opportunities of each of its items by the item's script.
    pub fn item_script(&self, at: usize) -> Option<[u8; 4]> {
        self.scripts.get(at).copied().flatten()
    }

    /// The largest ascent, descent and leading of the items that `text[a..b]` has a part of.
    pub fn metrics(&self, a: usize, b: usize) -> Option<[i64; 3]> {
        self.items
            .iter()
            .filter(|item| item.0 < b && a < item.1)
            .map(|item| item.2)
            .reduce(|m, n| [m[0].max(n[0]), m[1].max(n[1]), m[2].max(n[2])])
    }
}

/// `is_right_to_left`.
pub fn is_rtl(text: &str) -> bool {
    is_right_to_left(text)
}

/// QTextOption::tabStopDistance of a text nobody set one for, in 26.6.
pub const TAB_STOP: i64 = 80 * 64;

/// QTextEngine::calculateTabWidth of a text without tab positions: how wide a tab at `x` of
/// its line is, which is the way to the next tab stop. QFixed divides rounding to 1/64, so a
/// tab less than 1/128 of the distance before a stop goes to the stop after it.
pub fn tab_width(x: i64) -> i64 {
    let stops = (((x.abs() << 6) + (TAB_STOP >> 1)) / TAB_STOP) * x.signum();
    ((stops >> 6) + 1) * TAB_STOP - x
}

/// A QScriptItem of a `LineMap`: UTF-16 units `at..at + len` of the line, set at one bidi
/// level in one script.
struct MapItem {
    at: usize,
    len: usize,
    level: u8,
    /// A script whose ligatures the caret steps into (QTextEngine::positionInLigature).
    split: bool,
    /// QScriptAnalysis::Tab: one tab, which has no glyph; its one advance is the way to the
    /// next tab stop of the line it is in (`LineMap::shape_line`).
    tab: bool,
    /// The advance of every glyph, in the order of the text.
    advances: Vec<i64>,
    /// QGlyphAttributes::clusterStart of every glyph.
    starts: Vec<bool>,
    /// QTextEngine::logClusters: the first glyph of the cluster of every unit.
    clusters: Vec<usize>,
}

impl MapItem {
    fn reverse(&self) -> bool {
        self.level % 2 == 1
    }

    /// The first glyph of the cluster of unit `pos` of the item; past its last unit, the
    /// number of its glyphs.
    fn glyph(&self, pos: usize) -> usize {
        self.clusters
            .get(pos)
            .copied()
            .unwrap_or(self.advances.len())
    }
}

/// A shaped line as QTextLine::cursorToX and QTextLine::xToCursor read it: caret positions
/// are characters of the line, x is 26.6 from the left end of the line's text.
pub struct LineMap {
    /// QTextEngine::isRightToLeft: the direction of the line's paragraph.
    pub rtl: bool,
    /// QScriptLine::textWidth: the advance of the line, and what its last letter draws past it.
    width: i64,
    items: Vec<MapItem>,
    /// QCharAttributes::graphemeBoundary of every UTF-16 unit, and the end.
    bound: Vec<bool>,
    /// The UTF-16 unit of every character, and the end.
    units: Vec<usize>,
}

/// A QScriptLine of a `LineMap`: characters `from..from + len` of the paragraph and the
/// `trailing` white space after them the line was broken at.
pub struct MapLine {
    pub from: usize,
    pub len: usize,
    pub trailing: usize,
    /// QScriptLine::textWidth in 26.6.
    pub text_width: i64,
    /// QTextOption::IncludeTrailingSpaces, which a TextInput sets: the trailing white space
    /// counts as text of the line.
    pub keep_spaces: bool,
}

impl LineMap {
    fn char_at(&self, unit: usize) -> usize {
        self.units.partition_point(|&u| u < unit)
    }

    fn unit(&self, c: usize) -> usize {
        self.units[c.min(self.units.len() - 1)]
    }

    /// The paragraph as one line.
    fn whole(&self) -> MapLine {
        MapLine {
            from: 0,
            len: self.units.len() - 1,
            trailing: 0,
            text_width: self.width,
            keep_spaces: true,
        }
    }

    /// QTextEngine::shapeLine: the tabs of `line` get their widths, each from where the items
    /// before it in the text of the line end. A paragraph that is one line has them from
    /// the start; one broken into lines asks for each of them.
    pub fn shape_line(&mut self, line: &MapLine) {
        let from = self.unit(line.from);
        let end = self.unit(line.from + line.len + line.trailing);
        let mut x = 0;
        for si in &mut self.items {
            if si.at + si.len <= from {
                continue;
            }
            if si.at >= end {
                break;
            }
            if si.tab {
                si.advances[0] = tab_width(x);
            }
            let first = si.glyph(from.saturating_sub(si.at));
            x += si.advances[first..].iter().sum::<i64>();
        }
    }

    /// QTextEngine::findItem.
    fn find(&self, unit: usize) -> Option<usize> {
        self.items.iter().position(|i| unit < i.at + i.len)
    }

    /// The items of units `from..end` from the left (QTextEngine::bidiReorder over the items
    /// of the line), with the item `with` among them when it is after the line.
    fn visual(&self, from: usize, end: usize, with: Option<usize>) -> Vec<usize> {
        let (Some(first), Some(last)) = (self.find(from), self.find(end.saturating_sub(1))) else {
            return vec![];
        };
        // QTextLine::cursorToX looks for the line's last item from the caret's on.
        let last = last.max(with.unwrap_or(0));
        if last < first {
            return vec![];
        }
        let levels: Vec<unicode_bidi::Level> = self.items[first..=last]
            .iter()
            .map(|i| unicode_bidi::Level::new(i.level).unwrap())
            .collect();
        unicode_bidi::BidiInfo::reorder_visual(&levels)
            .into_iter()
            .map(|i| i + first)
            .collect()
    }

    /// The advance of the units `from..end` of the paragraph.
    fn width_of(&self, from: usize, end: usize) -> i64 {
        self.items
            .iter()
            .map(|si| {
                let (start, stop) = (from.max(si.at), end.min(si.at + si.len));
                if start >= stop {
                    return 0;
                }
                si.advances[si.glyph(start - si.at)..si.glyph(stop - si.at)]
                    .iter()
                    .sum()
            })
            .sum()
    }

    /// QTextEngine::width of the units `from..end`: a tab in them is as wide as if the units
    /// started a line, whatever QTextEngine::shapeLine made it in its own.
    fn width_from(&self, from: usize, end: usize) -> i64 {
        let mut w = 0;
        for si in &self.items {
            let (start, stop) = (from.max(si.at), end.min(si.at + si.len));
            if start >= stop {
                continue;
            }
            w += match si.tab {
                true => tab_width(w),
                false => si.advances[si.glyph(start - si.at)..si.glyph(stop - si.at)]
                    .iter()
                    .sum(),
            };
        }
        w
    }

    /// The advance of the characters `from..end` of the paragraph, its tabs as wide as the
    /// last `shape_line` made them.
    pub fn advance(&self, from: usize, end: usize) -> i64 {
        self.width_of(self.unit(from), self.unit(end))
    }

    /// QTextEngine::alignLine: where `line` starts in `width` under the alignment `align`,
    /// Qt::AlignRight (2) or Qt::AlignHCenter (4), else the left.
    pub fn align(&self, line: &MapLine, width: i64, align: u32) -> i64 {
        // QScriptLine::textAdvance is without the white space the line was broken at.
        let advance = self.width_of(self.unit(line.from), self.unit(line.from + line.len));
        match align {
            2 => width - advance,
            4 => (width - advance) / 2,
            _ => 0,
        }
    }

    /// QTextLine::cursorToX(pos, Leading) of the paragraph's only line.
    pub fn cursor_to_x(&self, pos: usize) -> i64 {
        self.cursor_to_x_in(&self.whole(), pos)
    }

    /// QTextLine::cursorToX(pos, Leading) of `line`, logical cursor movement, from where the
    /// line's alignment puts its text; not bound to the line's width, which Qt does to a
    /// line that wraps.
    pub fn cursor_to_x_in(&self, line: &MapLine, pos: usize) -> i64 {
        let from = self.unit(line.from);
        let text_end = self.unit(line.from + line.len);
        let end = self.unit(line.from + line.len + line.trailing);
        // QTextEngine::leadingSpaceWidth: the white space a right-to-left line was broken at
        // hangs out on its left.
        let mut x = if line.trailing > 0 && !line.keep_spaces && self.rtl {
            -self.width_from(text_end, end)
        } else {
            0
        };
        let mut pos = self.unit(pos).clamp(from, end);
        while pos < end && !self.bound[pos] {
            pos += 1;
        }
        // At the end of the line the caret stands at the line's last item.
        let last = if pos == end {
            pos.saturating_sub(1)
        } else {
            pos
        };
        let Some(mut itm) = self.find(last) else {
            return x;
        };
        let si = &self.items[itm];
        if si.reverse() != self.rtl {
            // Between two items of different directions the caret stands at the one set in
            // the direction of the paragraph.
            let mut neighbor = itm;
            if neighbor > 0 && si.at == pos {
                neighbor -= 1;
            } else if neighbor < self.items.len() - 1 && si.at + si.advances.len() == pos {
                neighbor += 1;
            }
            if neighbor != itm && si.level != self.items[neighbor].level {
                itm = neighbor;
            }
        }
        let si = &self.items[itm];
        let pos = pos.saturating_sub(si.at).min(si.len);
        let glyph_pos = si.glyph(pos);
        for item in self.visual(from, end, Some(itm)) {
            if item == itm {
                break;
            }
            let other = &self.items[item];
            let (start, stop) = (from.max(other.at), end.min(other.at + other.len));
            let gs = other.clusters[start - other.at];
            let ge = if stop == other.at + other.len {
                other.advances.len()
            } else {
                other.clusters[stop - other.at - 1] + 1
            };
            x += other.advances[gs..ge.max(gs)].iter().sum::<i64>();
        }
        let stop = end.min(si.at + si.len).saturating_sub(si.at);
        // QTextEngine::offsetInLigature: a caret inside a cluster stands at its share of the
        // advance of the cluster's first glyph.
        let inside = si.clusters[..pos]
            .iter()
            .rev()
            .take_while(|&&g| g == glyph_pos)
            .count();
        let offset = if inside > 0 {
            let length = si.clusters[pos - inside..stop.max(pos)]
                .iter()
                .take_while(|&&g| g == glyph_pos)
                .count();
            si.advances[glyph_pos] * inside as i64 / length.max(1) as i64
        } else {
            0
        };
        if si.reverse() {
            let glyph_end = si.glyph(stop);
            x += si.advances[glyph_pos.min(glyph_end)..glyph_end]
                .iter()
                .sum::<i64>()
                - offset;
        } else {
            let start = from.saturating_sub(si.at);
            let glyph_start = if start < si.len {
                si.clusters[start]
            } else {
                // The item is before the line: Qt reads the cluster of the line's first unit,
                // which is of the item after this one.
                self.find(from)
                    .map_or(0, |i| self.items[i].clusters[from - self.items[i].at])
            };
            x += si.advances[glyph_start.min(glyph_pos)..glyph_pos]
                .iter()
                .sum::<i64>()
                + offset;
        }
        x
    }

    /// QTextLine::xToCursor(x, CursorBetweenCharacters) of the paragraph's only line.
    pub fn x_to_cursor(&self, x: i64) -> usize {
        self.x_to_cursor_in(&self.whole(), x, true)
    }

    /// QTextLine::xToCursor(x, CursorBetweenCharacters) of `line`, logical cursor movement:
    /// `x` is from where the line's alignment puts its text, `last` tells the last line of
    /// the layout.
    pub fn x_to_cursor_in(&self, line: &MapLine, x: i64, last: bool) -> usize {
        let from = self.unit(line.from);
        let end = self.unit(line.from + line.len + line.trailing);
        if from == end {
            return line.from;
        }
        if x <= 0 {
            return if self.rtl {
                line.from + line.len + line.trailing
            } else {
                line.from
            };
        }
        if x < line.text_width {
            let mut pos = 0;
            for item in self.visual(from, end, None) {
                let si = &self.items[item];
                let start = from.saturating_sub(si.at);
                let stop = (end - si.at).min(si.len);
                let (gs, ge) = (si.clusters[start], si.glyph(stop));
                let item_width: i64 = si.advances[gs..ge.max(gs)].iter().sum();
                if pos + item_width < x {
                    pos += item_width;
                    continue;
                }
                if si.tab {
                    // The half of the tab the point is in: the caret before it or after it.
                    let left_half = x - pos < item_width / 2;
                    return self.char_at(si.at + (si.reverse() == left_half) as usize);
                }
                let (mut glyph_pos, mut edge, mut dist) = (None, 0, i64::MAX);
                if si.reverse() {
                    pos += item_width;
                }
                for g in gs..ge {
                    if si.starts[g] && (x - pos).abs() < dist {
                        (glyph_pos, edge, dist) = (Some(g), pos, (x - pos).abs());
                    }
                    pos += if si.reverse() {
                        -si.advances[g]
                    } else {
                        si.advances[g]
                    };
                }
                let unit = if (x - pos).abs() < dist {
                    self.position_in_ligature(si, stop, x, pos, None)
                } else {
                    self.position_in_ligature(si, stop, x, edge, glyph_pos)
                };
                return self.char_at(unit);
            }
        }
        let mut pos = if self.rtl { from } else { end };
        if !last {
            // QTextEngine::previousLogicalPosition: the caret after the line's last
            // character is the one before the next line's first.
            pos = pos.saturating_sub(1);
            while pos > 0 && !self.bound[pos] {
                pos -= 1;
            }
        }
        self.char_at(pos)
    }

    /// Where Qt Quick draws the selection of characters `a..b` of `line`, as (left, right) from
    /// where the line's alignment puts its text: one stretch for every run of one direction the
    /// characters are in (QTextLineItemIterator::getSelectionBounds, the bounding rectangle of
    /// a QGlyphRun of QTextLine::glyphRuns). The white space a line breaks at has none. Of a
    /// ligature it is the share of the characters in it.
    pub fn ranges(&self, line: &MapLine, a: usize, b: usize) -> Vec<(i64, i64)> {
        let from = self.unit(line.from);
        let end = self.unit(line.from + line.len);
        let (a, b) = (self.unit(a).max(from), self.unit(b).min(end));
        let mut x = 0;
        let mut out: Vec<(i64, i64)> = Vec::new();
        // The stretch before reaches the end of its glyphs, and tabs alone follow it so far.
        let (mut open, mut tabs) = (false, false);
        for item in self.visual(from, end, None) {
            let si = &self.items[item];
            let (start, stop) = (from.max(si.at) - si.at, end.min(si.at + si.len) - si.at);
            let (gs, ge) = (si.glyph(start), si.glyph(stop).max(si.glyph(start)));
            let sum = |a: usize, b: usize| -> i64 { si.advances[a..b.max(a)].iter().sum() };
            if si.tab {
                // A tab has no glyphs, so no stretch of its own.
                x += sum(gs, ge);
                tabs = true;
                continue;
            }
            // QTextEngine::offsetInLigature.
            let in_ligature = |pos: usize, max: usize, glyph: usize| -> i64 {
                let inside = si.clusters[..pos]
                    .iter()
                    .rev()
                    .take_while(|&&g| g == glyph)
                    .count();
                let length = si.clusters[pos - inside..max.max(pos)]
                    .iter()
                    .take_while(|&&g| g == glyph)
                    .count();
                if inside > 0 && length > 0 {
                    si.advances[glyph] * inside as i64 / length as i64
                } else {
                    0
                }
            };
            let (sa, sb) = (a.max(si.at + start), b.min(si.at + stop));
            if sa < sb {
                let (sa, sb) = (sa - si.at, sb - si.at);
                let (first, last) = (si.glyph(sa), si.glyph(sb));
                let before = if si.reverse() {
                    sum(last, ge)
                } else {
                    sum(gs, first)
                };
                let inside = in_ligature(sa, sb, first);
                let left = x + before + inside;
                let width = sum(first, last) - inside + in_ligature(sb, si.len, last);
                // QQuickTextNodeEngine::processCurrentLine unites the selected glyph runs
                // that follow each other in the line: the tabs between two of them are in
                // the stretch, a tab at either end of the selection is not.
                match out.last_mut() {
                    Some(before) if open && tabs && left == x => before.1 = left + width,
                    _ => out.push((left, left + width)),
                }
                open = left + width == x + sum(gs, ge);
            } else {
                open = false;
            }
            tabs = false;
            x += sum(gs, ge);
        }
        out
    }

    /// QTextEngine::positionInLigature for a caret between characters: the unit of the
    /// paragraph next to `x`, which is nearest to the edge at `edge` of the cluster of
    /// `glyph_pos`, or with no glyph to the far end of the item, whose units before `end`
    /// are in the line.
    fn position_in_ligature(
        &self,
        si: &MapItem,
        end: usize,
        x: i64,
        edge: i64,
        glyph_pos: Option<usize>,
    ) -> usize {
        if !si.split {
            return si.at
                + glyph_pos.map_or(end, |g| {
                    si.clusters.iter().position(|&c| c == g).unwrap_or(end)
                });
        }
        let glyph_pos = match glyph_pos {
            None if end > 0 => Some(si.clusters[end - 1]),
            None => None,
            Some(g) if x <= edge => g.checked_sub(1),
            g => g,
        };
        // QTextEngine::getClusterLength: the graphemes of the cluster and where it starts.
        let (mut start, mut length) = (0, 0);
        for i in 0..end {
            if Some(si.clusters[i]) == glyph_pos && self.bound[si.at + i] {
                if length == 0 {
                    start = i;
                }
                length += 1;
            } else if length > 0 {
                break;
            }
        }
        let Some(glyph_pos) = glyph_pos.filter(|_| length > 0) else {
            return si.at + end;
        };
        let glyph_width = si.advances[glyph_pos];
        let per_item = glyph_width / length;
        if per_item <= 0 {
            return si.at + start;
        }
        let left = if x > edge { edge } else { edge - glyph_width };
        // QFixed's division rounds to 1/64; floor() of it is the whole part.
        let n = ((((x - left) << 6) + (per_item >> 1)) / per_item).div_euclid(64);
        let dist = x - left - n * per_item;
        let closest = if dist > per_item / 2 { n + 1 } else { n };
        let mut pos = (start as i64 + closest).max(0) as usize;
        while pos < end && !self.bound[si.at + pos] {
            pos += 1;
        }
        si.at + pos
    }
}

/// Characters QTextEngine::shapeTextWithHarfbuzzNG leaves unprinted, and the tab, which is an
/// item of its own that is not shaped: none has an advance, and none is looked up in a
/// fallback font.
fn unprinted(c: char) -> bool {
    matches!(
        c,
        '\n' | '\u{c}' | '\r' | '\u{2028}' | '\u{2029}' | '\u{ad}' | '\t'
    )
}

impl FontStack {
    pub fn from_path(
        path: &str,
        pixel_size: u32,
        weight: f32,
        axes: &[(&str, f32)],
    ) -> std::io::Result<FontStack> {
        Self::from_path_styled(path, pixel_size, weight, weight, axes)
    }

    /// `from_path` with the QFont weight kept apart from the weight of the face QFontDatabase
    /// picked for it (see `Font::open_styled`).
    pub fn from_path_styled(
        path: &str,
        pixel_size: u32,
        weight: f32,
        request_weight: f32,
        axes: &[(&str, f32)],
    ) -> std::io::Result<FontStack> {
        let primary = Font::open_styled(path, 0, pixel_size, weight, request_weight, axes)?;
        let italic = axes.iter().any(|(a, v)| *a == "ital" && *v >= 1.0);
        let families = fc_fallbacks(&primary.family(), italic);
        let fallbacks = families.faces.iter().map(|_| Default::default()).collect();
        let emoji = fc_match_file("emoji")
            .filter(|p| p != path)
            .map(|p| (p, std::cell::OnceCell::new()));
        let size64 = (primary.pixel_size as i64) << 6;
        let request_size64 = (pixel_size.max(1) as i64) << 6;
        Ok(FontStack {
            primary,
            families,
            fallbacks,
            emoji,
            weight,
            request_weight,
            face_style: Default::default(),
            axes: axes.iter().map(|(t, v)| (t.to_string(), *v)).collect(),
            no_hint: false,
            raster: 1.0,
            full_hint: false,
            design: false,
            shaped: std::cell::RefCell::new(lazy_lru(SHAPED_LINES)),
            texts: std::cell::RefCell::new(lazy_lru(SHAPED_TEXTS)),
            path: path.to_string(),
            size64,
            request_size64,
            spacing: 0,
            scaled: Default::default(),
            is_sub: false,
            subs: std::array::from_fn(|_| Default::default()),
            missing: None,
        })
    }

    /// This stack for a glyph cache scaled by `dpr`, with the same faces and fallbacks (same
    /// font indices and glyph ids): what QFontEngineFT rasterizes NativeRendering glyphs with
    /// on a window of that devicePixelRatio. A scalable font keeps its size and draws its
    /// outlines through the scale, unhinted (QFontEngineFT::loadGlyphFor with the cache's
    /// transform); a font of bitmaps is opened at `dpr` times its size.
    pub fn scaled(&self, dpr: f64) -> Option<std::rc::Rc<FontStack>> {
        if !(dpr.is_finite() && dpr > 0.0) {
            return None;
        }
        let key = dpr.to_bits();
        if let Some((_, s)) = self.scaled.borrow().iter().find(|(k, _)| *k == key) {
            return Some(s.clone());
        }
        let (size64, request_size64) = (self.size64, self.request_size64);
        let axes: Vec<(&str, f32)> = self.axes.iter().map(|(t, v)| (t.as_str(), *v)).collect();
        let open = |size64| {
            Font::open_sized(
                &self.path,
                0,
                size64,
                self.weight,
                self.request_weight,
                &axes,
            )
        };
        let mut primary = open(size64).ok()?;
        if primary.bitmaps() {
            primary = open(device_size64(size64, dpr)).ok()?;
        } else {
            primary.raster = dpr;
        }
        if self.no_hint {
            primary.set_no_hinting();
        }
        let spacing = self.spacing;
        primary.letter_spacing = spacing;
        if self.full_hint {
            primary.set_full_hinting(self.design);
        }
        let st = std::rc::Rc::new(FontStack {
            primary,
            families: self.families.clone(),
            fallbacks: self.fallbacks.iter().map(|_| Default::default()).collect(),
            emoji: self
                .emoji
                .as_ref()
                .map(|(p, _)| (p.clone(), std::cell::OnceCell::new())),
            weight: self.weight,
            request_weight: self.request_weight,
            face_style: self.face_style.clone(),
            axes: self.axes.clone(),
            no_hint: self.no_hint,
            raster: dpr,
            full_hint: self.full_hint,
            design: self.design,
            shaped: std::cell::RefCell::new(lru::LruCache::new(
                std::num::NonZeroUsize::new(1).unwrap(),
            )),
            texts: std::cell::RefCell::new(lru::LruCache::new(
                std::num::NonZeroUsize::new(1).unwrap(),
            )),
            path: self.path.clone(),
            size64,
            request_size64,
            spacing,
            scaled: Default::default(),
            is_sub: false,
            subs: std::array::from_fn(|_| Default::default()),
            missing: self.missing.clone(),
        });
        let mut c = self.scaled.borrow_mut();
        // A window moving between outputs leaves a ratio or two behind.
        if c.len() >= 4 {
            c.remove(0);
        }
        c.push((key, st.clone()));
        Some(st)
    }

    /// NativeRendering glyphs of `line` (laid out by this stack, node at `tx`, `ty`, pen at
    /// `x`, `y`) on a device `dpr` times finer, through the scene transform `xf` (sx sy ox oy,
    /// device = s * p + o, which includes `dpr`): glyphs rasterized at the device size and
    /// placed at device pixels, as QSGTextMaskMaterial does on a high-DPI window. Any scale
    /// left in `xf` beyond `dpr` (an item `scale`) stretches the bitmaps.
    #[allow(clippy::too_many_arguments)]
    pub fn line_rects_dpr(
        &self,
        line: &ShapedLine,
        tx: f64,
        ty: f64,
        x: f64,
        y: f64,
        xf: [f64; 4],
        dpr: f64,
    ) -> Vec<GlyphRect> {
        match self.scaled(dpr) {
            Some(st) => st.place_dpr(line, tx, ty, x, y, [0.0; 2], xf, [false; 2], dpr, dpr),
            None => Vec::new(),
        }
    }

    /// `line_rects_dpr` on the stack `scaled(dpr)` returned; `against` is the axes of the
    /// target that run against the screen's (`snap_px`). `root` is where the node's batch root
    /// is when that is not the window (the clip node of a TextField's or a TextArea's text):
    /// the node is at `tx`, `ty` in it, the renderer adds that to the vertices and the shader
    /// the root's place, two float sums where a node of the window has one. `target` is the
    /// devicePixelRatio of the render target, whose pixels the shader snaps to: `dpr`, the
    /// glyphs' own, in a window.
    #[allow(clippy::too_many_arguments)]
    pub fn place_dpr(
        &self,
        line: &ShapedLine,
        tx: f64,
        ty: f64,
        x: f64,
        y: f64,
        root: [f64; 2],
        xf: [f64; 4],
        against: [bool; 2],
        dpr: f64,
        target: f64,
    ) -> Vec<GlyphRect> {
        let st = self;
        let [sx, sy, ox, oy] = xf;
        let (kx, ky) = (sx / dpr, sy / dpr);
        let baseline = y + line.ascent as f64 / 64.0;
        // The vertex shader (textmask.vert): the node's vertex through the scene transform in
        // floats, then to the nearest device pixel.
        let snap = |v: f64, t: f64, k: f64, o: f64, root: f64, against: bool| {
            let m = (o / dpr) as f32 + k as f32 * root as f32;
            let scene = (v as f32 + t as f32) * k as f32 + m;
            snap_px(scene * target as f32, against)
        };
        // glyphCacheInverseScale: Qt multiplies by it, and the last bit decides a tie.
        let inv = 1.0 / dpr;
        let mut out = Vec::with_capacity(line.glyphs.len());
        for &(f, gid, gx, gy) in &line.glyphs {
            let Some(font) = st.font(f) else {
                continue;
            };
            // QSGTextMaskMaterial::populate: the glyph's position in the node, scaled into the
            // glyph cache before it is floored (x), rounded (y) and split into a subpixel
            // offset; the node's own position only moves the result.
            let px = x + gx as f64 / 64.0;
            let py = baseline + gy as f64 / 64.0;
            // QFixed::fromReal cuts the fraction off, both times.
            let fx = ((px * 64.0).trunc() / 64.0 * dpr * 64.0).trunc() as i64;
            // QFontEngineFT::supportsHorizontalSubPixelPositions: not a fully hinted font.
            let sp = if st.full_hint { 0 } else { Font::subpixel(fx) };
            let Some(g) = font.glyph(gid, sp) else {
                continue;
            };
            let lx = (px * dpr).floor() * inv + g.left as f64 * inv;
            let ly = qround(py * dpr) * inv - g.top as f64 * inv;
            let (w, h) = (g.width as f64 * inv, g.rows as f64 * inv);
            let r = [
                snap(lx, tx, kx, ox, root[0], against[0]),
                snap(ly, ty, ky, oy, root[1], against[1]),
                snap(lx + w, tx, kx, ox, root[0], against[0]),
                snap(ly + h, ty, ky, oy, root[1], against[1]),
            ];
            out.push((g, r));
        }
        out
    }

    /// QFont::setLetterSpacing(QFont::AbsoluteSpacing, px).
    pub fn letter_spacing(mut self, px: f64) -> Self {
        self.spacing = (px * 64.0).round() as i64;
        self.primary.letter_spacing = self.spacing;
        self
    }

    /// What QTextEngine::elidedText puts where a line is cut off, and the width it leaves for
    /// it, in 26.6: the ellipsis of the stack's first font, so that eliding moves no line;
    /// three full stops when that font has none, and another font's ellipsis when it has no
    /// full stop either. The width is the glyph's own advance (recalcAdvances), without the
    /// letter spacing that follows it once it is laid out, and three times a full stop's.
    fn ellipsis_64(&self) -> (&'static str, i64) {
        let own = |t: &str| self.primary.shape(t).1 - self.primary.letter_spacing;
        if self.primary.has_char('\u{2026}') {
            ("\u{2026}", own("\u{2026}"))
        } else if self.primary.has_char('.') {
            ("...", own(".") * 3)
        } else {
            ("\u{2026}", self.shape("\u{2026}").advance - self.spacing)
        }
    }

    /// `ellipsis_64` in pixels.
    pub fn ellipsis(&self) -> (&'static str, f64) {
        let (text, width) = self.ellipsis_64();
        (text, width as f64 / 64.0)
    }

    /// The stack of a QFont whose family is `family`, the one its primary font was matched
    /// for: when no installed family has that name (QFontDatabasePrivate::match compares the
    /// names without their case), every script looks for its font by that name
    /// (`script_stack`).
    pub fn requested(mut self, family: &str) -> Self {
        let name = family.to_lowercase();
        let installed = fc_faces(fc::constants::FC_FAMILY, family)
            .iter()
            .any(|f| f.family.to_lowercase() == name);
        self.missing = (!family.is_empty() && !installed).then(|| family.to_string());
        self
    }

    pub fn no_hinting(mut self) -> Self {
        self.no_hint = true;
        self.primary.set_no_hinting();
        self
    }

    pub fn full_hinting(mut self) -> Self {
        self.full_hint = true;
        self.primary.set_full_hinting(false);
        self
    }

    /// `full_hinting` for Text.QtRendering, which lays out with design metrics.
    pub fn full_hinting_design(mut self) -> Self {
        self.full_hint = true;
        self.design = true;
        self.primary.set_full_hinting(true);
        self
    }

    /// QTextLine::naturalTextWidth: the advance plus the last glyph's negative right bearing (26.6).
    pub fn natural_width(&self, line: &ShapedLine) -> i64 {
        // LineBreakHelper::currentGlyph is glyphs[logClusters[last character]]: the first glyph
        // of the last cluster, so the base of a letter that ends the line with marks on it.
        let Some(&(fi, gid, _, _)) = line.glyphs.get(line.last_cluster) else {
            return line.advance;
        };
        let Some(font) = self.font(fi) else {
            return line.advance;
        };
        if font.color_scale > 0.0 {
            return line.advance;
        }
        let (left, wd, adv) = font.glyph_box(gid);
        line.advance + ((left + wd - adv).max(0) << 6)
    }

    pub fn font(&self, i: usize) -> Option<&Font> {
        if i == 0 {
            return Some(&self.primary);
        }
        if i >= SUB0 {
            // A twin at a device pixel ratio is asked for the fonts its stack itemized with.
            let (script, _) = SCRIPT_LANGUAGES.get((i - SUB0) / SUB_STRIDE)?;
            let (_, sub) = self.script_stack(u32::from_be_bytes(**script))?;
            return sub.font((i - SUB0) % SUB_STRIDE);
        }
        let emoji;
        let (path, idx, cell) = match self.fallbacks.get(i - 1) {
            Some(c) => {
                let (p, x) = &self.families.faces[i - 1];
                (p, x, c)
            }
            None if i == self.fallbacks.len() + 1 => {
                let (p, c) = self.emoji.as_ref()?;
                emoji = 0;
                (p, &emoji, c)
            }
            None => return None,
        };
        cell.get_or_init(|| {
            let axes: Vec<(&str, f32)> = self.axes.iter().map(|(t, v)| (t.as_str(), *v)).collect();
            let (path, idx, weight) = (i <= self.fallbacks.len())
                .then(|| {
                    let style = self.face_style.get_or_init(|| self.primary.style_name());
                    let italic = axes.iter().any(|(t, v)| *t == "ital" && *v >= 1.0);
                    fallback_face(path, *idx, self.request_weight as i32, style, italic)
                })
                .flatten()
                .map_or((path.clone(), *idx, self.weight), |(p, x, w)| {
                    (p, x, w as f32)
                });
            let size64 = if i > self.fallbacks.len() {
                self.request_size64
            } else {
                self.size64
            };
            let open = |size64| {
                Font::open_sized(&path, idx, size64, weight, self.request_weight, &axes).ok()
            };
            open(size64)
                .and_then(|mut f| {
                    if self.raster == 1.0 {
                        return Some(f);
                    }
                    if f.bitmaps() {
                        return open(device_size64(size64, self.raster));
                    }
                    f.raster = self.raster;
                    Some(f)
                })
                .map(|mut f| {
                    if self.no_hint {
                        f.set_no_hinting();
                    }
                    if self.full_hint {
                        f.set_full_hinting(self.design);
                    }
                    f.letter_spacing = self.spacing;
                    Box::new(f)
                })
        })
        .as_deref()
    }

    /// The font of a character in an item of `script`: the primary font, or the first of the
    /// script's fallback families that has it (QFontEngineMulti::glyphIndex, on the engine
    /// QFontPrivate::engineForScript keeps per script).
    fn font_for(&self, c: char, script: hb::hb_script_t) -> usize {
        if let Some((slot, sub)) = self.script_stack(script) {
            return SUB0 + slot * SUB_STRIDE + sub.font_for(c, script);
        }
        if self.primary.has_char(c) {
            return 0;
        }
        let has = |&i: &usize| self.font(i).is_some_and(|f| f.has_char(c));
        match script_language(script) {
            Some(lang) => self.families.order(lang).iter().copied().find(has),
            None => (1..=self.fallbacks.len().min(MAX_FALLBACKS)).find(has),
        }
        .unwrap_or(0)
    }

    /// The stack items of `script` are set in when their first font is not the primary one,
    /// with its slot in `subs`: Qt's QFontEngineMulti for the script
    /// (QFontPrivate::engineForScript). QFontDatabasePrivate::findFont goes through the
    /// fallback families fontconfig sorts for the script's language, of each it takes the face
    /// the QFont's weight and slant match, and the first that loads is the stack's first font,
    /// whatever characters it has. It does so in two cases. The script needs OpenType shaping
    /// and the primary font has no tables for it (QFontDatabasePrivate::loadSingleEngine
    /// refuses a font by QFontEngine::supportsScript, the fallback families' too): the families
    /// are the primary font's. No family has the QFont's name: the families are the ones
    /// sorted for that name and the language, so a script whose language's fonts fontconfig
    /// puts first gets another font than Latin text, for all of its item (the digits and
    /// spaces after Arabic letters too). The font is opened at the pixel size asked for (not
    /// a bitmap primary font's strike), and its fallback families are its own, with its
    /// face's weight.
    fn script_stack(&self, script: hb::hb_script_t) -> Option<(usize, &FontStack)> {
        if self.is_sub {
            return None;
        }
        let opentype = opentype_script(script);
        if !opentype && self.missing.is_none() {
            return None;
        }
        let slot = script_slot(script)?;
        let sub = self.subs[slot].get_or_init(|| {
            let lang = SCRIPT_LANGUAGES[slot].1;
            let axes: Vec<(&str, f32)> = self.axes.iter().map(|(t, v)| (t.as_str(), *v)).collect();
            let italic = axes.iter().any(|(t, v)| *t == "ital" && *v >= 1.0);
            let own = self.primary.family().to_lowercase();
            let families = match &self.missing {
                Some(family) => fc_fallbacks(family, italic),
                None if self.primary.supports_script(script) => return None,
                None => self.families.clone(),
            };
            let order = families.order(lang);
            let first = order.first().map(|&i| &families.names[i - 1]);
            if !opentype && first.is_none_or(|name| *name == own) {
                return None;
            }
            let primary = order.iter().find_map(|&i| {
                let (path, idx) = &families.faces[i - 1];
                let (path, idx, weight) =
                    fallback_face(path, *idx, self.request_weight as i32, "", italic)
                        .map_or((path.clone(), *idx, self.weight), |(p, x, w)| {
                            (p, x, w as f32)
                        });
                let font = Font::open_sized(
                    &path,
                    idx,
                    self.request_size64,
                    weight,
                    self.request_weight,
                    &axes,
                )
                .ok()?;
                (!opentype || font.supports_script(script)).then_some((font, path, idx, weight))
            });
            let (primary, path, idx, weight) = primary?;
            if primary.family().to_lowercase() == own {
                return None;
            }
            let mut sub = FontStack::around(primary, &path, self.request_size64, weight, &axes);
            if self.raster != 1.0 {
                // As `scaled` does for the stack itself.
                if sub.primary.bitmaps() {
                    let size64 = device_size64(sub.size64, self.raster);
                    sub.primary =
                        Font::open_sized(&path, idx, size64, weight, self.request_weight, &axes)
                            .ok()?;
                } else {
                    sub.primary.raster = self.raster;
                }
                sub.raster = self.raster;
            }
            if self.no_hint {
                sub = sub.no_hinting();
            }
            if self.full_hint {
                sub.full_hint = true;
                sub.design = self.design;
                sub.primary.set_full_hinting(self.design);
            }
            sub.spacing = self.spacing;
            sub.primary.letter_spacing = self.spacing;
            Some(Box::new(sub))
        });
        sub.as_deref().map(|s| (slot, s))
    }

    /// A stack whose first font is `primary`, opened from `path` at `request_size64`, with the
    /// fallback families of its family; they open with `weight`, the face's own.
    fn around(
        primary: Font,
        path: &str,
        request_size64: i64,
        weight: f32,
        axes: &[(&str, f32)],
    ) -> FontStack {
        let italic = axes.iter().any(|(a, v)| *a == "ital" && *v >= 1.0);
        let families = fc_fallbacks(&primary.family(), italic);
        let fallbacks = families.faces.iter().map(|_| Default::default()).collect();
        let emoji = fc_match_file("emoji")
            .filter(|p| p != path)
            .map(|p| (p, std::cell::OnceCell::new()));
        let size64 = if primary.strike || primary.color_scale > 0.0 {
            (primary.pixel_size as i64) << 6
        } else {
            request_size64
        };
        FontStack {
            primary,
            families,
            fallbacks,
            emoji,
            weight,
            request_weight: weight,
            face_style: Default::default(),
            axes: axes.iter().map(|(t, v)| (t.to_string(), *v)).collect(),
            no_hint: false,
            raster: 1.0,
            full_hint: false,
            design: false,
            shaped: std::cell::RefCell::new(lru::LruCache::new(SHAPED_LINES)),
            texts: std::cell::RefCell::new(lru::LruCache::new(SHAPED_TEXTS)),
            path: path.to_string(),
            size64,
            request_size64,
            spacing: 0,
            scaled: Default::default(),
            is_sub: true,
            subs: std::array::from_fn(|_| Default::default()),
            missing: None,
        }
    }

    /// QFontMetricsF::boundingRect(text), (x, y, width, height) from the start of the baseline:
    /// QTextEngine::boundingBox over the line's script items, each QFontEngineMulti::boundingBox
    /// over its runs of one font.
    pub fn bounding_rect(&self, text: &str) -> (f64, f64, f64, f64) {
        if text.is_empty() {
            return (0.0, 0.0, 0.0, 0.0);
        }
        let emoji_idx = self.fallbacks.len() + 1;
        let runs = self.runs(text, None);
        // An emoji sequence is an item of its own.
        let item = |r: &Run| (r.script, r.level, r.font == emoji_idx);
        let mut gm = GlyphBox::default();
        for runs in runs.chunk_by(|a, b| item(a) == item(b)) {
            let mut overall = GlyphBox::default();
            // QTextEngine keeps an item's glyphs in the order of its text: those of a
            // right-to-left item are boxed with the pen going from the first character on.
            let rtl = runs[0].level.is_rtl();
            for r in runs {
                let Some(font) = self.font(r.font) else {
                    continue;
                };
                let (mut glyphs, _, advance) = font.shape_run(text, r, None);
                if rtl {
                    let mut pen = 0;
                    for g in &mut glyphs {
                        g.1 -= pen;
                        pen += g.3;
                    }
                    glyphs.reverse();
                    pen = 0;
                    for g in &mut glyphs {
                        g.1 += pen;
                        pen += g.3;
                    }
                }
                let m = font.bounding_box(&glyphs, advance);
                overall.x = overall.x.min(m.x);
                overall.y = overall.y.min(m.y);
                overall.width = overall.xoff + m.width;
                overall.height =
                    (overall.height + overall.y).max(m.height + m.y) - overall.y.min(m.y);
                overall.xoff += m.xoff;
            }
            gm.x = gm.x.min(overall.x + gm.xoff);
            gm.y = gm.y.min(overall.y);
            gm.width = gm.width.max(overall.width + gm.xoff);
            gm.height = gm.height.max(overall.height);
            gm.xoff += overall.xoff;
        }
        let px = |v: i64| v as f64 / 64.0;
        (px(gm.x), px(gm.y), px(gm.width), px(gm.height))
    }

    /// The line's runs by script item: an emoji sequence is an item of its own.
    fn items<'a>(&self, runs: &'a [Run]) -> impl Iterator<Item = &'a [Run]> {
        let emoji_idx = self.fallbacks.len() + 1;
        let item = move |r: &Run| (r.script, r.level, r.font == emoji_idx);
        runs.chunk_by(move |a, b| item(a) == item(b))
    }

    /// QFontMetricsF::tightBoundingRect(text), (x, y, width, height) from the start of the
    /// baseline: QTextEngine::tightBoundingBox over the line's script items, each
    /// QFontEngine::tightBoundingBox over the item's printed glyphs.
    pub fn tight_bounding_rect(&self, text: &str) -> (f64, f64, f64, f64) {
        if text.is_empty() {
            return (0.0, 0.0, 0.0, 0.0);
        }
        let runs = self.runs(text, None);
        let mut gm = GlyphBox::default();
        for runs in self.items(&runs) {
            let mut overall = GlyphBox::default();
            let (mut xmax, mut ymax) = (0i64, 0i64);
            let rtl = runs[0].level.is_rtl();
            for k in 0..runs.len() {
                let r = &runs[if rtl { runs.len() - 1 - k } else { k }];
                let Some(font) = self.font(r.font) else {
                    continue;
                };
                let (glyphs, clusters, _, advance) = font.shape_run_clusters(text, r, None);
                for ((gid, x, y, _), c) in glyphs.into_iter().zip(clusters) {
                    // applyVisibilityRules: these characters' glyphs are not printed.
                    if text.get(c as usize..).is_some_and(|t| {
                        t.starts_with(['\n', '\u{c}', '\r', '\u{2028}', '\u{2029}', '\u{ad}'])
                    }) {
                        continue;
                    }
                    let Some(bb) = font.glyph_metrics(gid) else {
                        continue;
                    };
                    let (x, y) = (overall.xoff + x + bb.x, y + bb.y);
                    overall.x = overall.x.min(x);
                    overall.y = overall.y.min(y);
                    xmax = xmax.max(((x + 63) & -64) + bb.width);
                    ymax = ymax.max(((y + 63) & -64) + bb.height);
                }
                overall.xoff += advance;
            }
            overall.height = overall.height.max(ymax - overall.y);
            overall.width = xmax - overall.x;
            gm.x = gm.x.min(overall.x + gm.xoff);
            gm.y = gm.y.min(overall.y);
            gm.width = gm.width.max(overall.width + gm.xoff);
            gm.height = gm.height.max(overall.height);
            // What Qt 6.12 returns: the item after a right-to-left one starts where that one
            // started.
            if !rtl {
                gm.xoff += overall.xoff;
            }
        }
        let px = |v: i64| v as f64 / 64.0;
        (px(gm.x), px(gm.y), px(gm.width), px(gm.height))
    }

    /// The advance of every cluster of the line at the byte its first character starts at,
    /// 26.6 (QTextEngine gives a cluster's width to its first character), and where the
    /// line's script items end.
    fn cluster_advances(&self, text: &str) -> (Vec<i64>, Vec<usize>) {
        let runs = self.runs(text, None);
        let mut adv = vec![0i64; text.len() + 1];
        for r in &runs {
            let Some(font) = self.font(r.font) else {
                continue;
            };
            let (glyphs, clusters, ..) = font.shape_run_clusters(text, r, None);
            for (g, c) in glyphs.iter().zip(clusters) {
                adv[(c as usize).min(text.len())] += g.3;
            }
        }
        let ends = self.items(&runs).map(|r| r[r.len() - 1].end).collect();
        (adv, ends)
    }

    /// QFontMetricsF::horizontalAdvance(text) in pixels: the advance of the line up to the
    /// first U+009C (which parts a text's variants of different lengths) in it.
    pub fn advance_width(&self, text: &str) -> f64 {
        let Some(cut) = text.find(VARIANT_SEPARATOR) else {
            return self.shape(text).advance as f64 / 64.0;
        };
        let (adv, _) = self.cluster_advances(text);
        adv[..cut].iter().sum::<i64>() as f64 / 64.0
    }

    /// QFontMetricsF::elidedText(text, mode, width, flags). `mode` is a Qt::TextElideMode
    /// (ElideLeft 0, ElideRight 1, ElideMiddle 2, ElideNone 3) and `flags` are Qt::TextFlag
    /// bits, of which Qt reads TextShowMnemonic and TextLongestVariant here.
    pub fn elided_text(&self, text: &str, mode: i32, width: f64, flags: u32) -> String {
        bits::bit_flags! {
        const SHOW_MNEMONIC: u32 = 0x0800;
        const LONGEST_VARIANT: u32 = 0x80000;
        }
        let mut text = text;
        if flags & LONGEST_VARIANT == 0 {
            // The first variant that fits, or the last one elided.
            while let Some((portion, rest)) = text.split_once(VARIANT_SEPARATOR) {
                if self.variant_width(portion, flags & SHOW_MNEMONIC != 0) <= width {
                    return portion.to_string();
                }
                text = rest;
            }
        }
        self.elided(text, mode, width, flags & SHOW_MNEMONIC != 0)
    }

    /// QFontMetricsF::size(flags, text).width(): the widest of the text's lines, with the
    /// `&` that marks a mnemonic left out.
    fn variant_width(&self, text: &str, mnemonic: bool) -> f64 {
        let text = if mnemonic {
            let mut out = String::new();
            let mut chars = text.chars();
            while let Some(c) = chars.next() {
                match c {
                    '&' => out.extend(chars.next()),
                    c => out.push(c),
                }
            }
            std::borrow::Cow::Owned(out)
        } else {
            std::borrow::Cow::Borrowed(text)
        };
        text.split(['\n', '\u{2028}'])
            .map(|l| self.shape(l).advance as f64 / 64.0)
            .fold(0.0, f64::max)
    }

    /// QTextEngine::elidedText.
    fn elided(&self, text: &str, mode: i32, width: f64, mnemonic: bool) -> String {
        use unicode_segmentation::UnicodeSegmentation;
        let (mut adv, ends) = self.cluster_advances(text);
        // Where a character one may stop at starts (a grapheme boundary), and the end.
        let mut stops = vec![false; text.len() + 1];
        for (i, _) in text.grapheme_indices(true) {
            stops[i] = true;
        }
        stops[text.len()] = true;
        if mnemonic {
            // The `&` before a character is not drawn and makes one character with it; the
            // last character of a script item is not looked at.
            let mut chars = text.char_indices().peekable();
            while let Some((i, c)) = chars.next() {
                let Some(&(n, next)) = chars.peek() else {
                    break;
                };
                if c == '&' && !ends.contains(&n) && !next.is_whitespace() && stops[n] {
                    adv[i] = 0;
                    stops[n] = false;
                    if next == '&' {
                        chars.next();
                    }
                }
            }
        }
        let mut before = vec![0i64; text.len() + 2];
        for (i, a) in adv.iter().enumerate() {
            before[i + 1] = before[i] + a;
        }
        let w = |from: usize, to: usize| before[to] - before[from];
        let len = text.len();
        // QFixed::fromReal truncates.
        let width = (width * 64.0) as i64;
        if !(0..=2).contains(&mode) || w(0, len) <= width || text.encode_utf16().count() <= 1 {
            return text.to_string();
        }
        let (ellipsis, ellipsis_width) = self.ellipsis_64();
        let mut ellipsis = ellipsis.to_string();
        let available = width - ellipsis_width;
        if available < 0 {
            return String::new();
        }
        let next = |mut i: usize| {
            i += 1;
            while i < len && !stops[i] {
                i += 1;
            }
            i
        };
        let prev = |mut i: usize, least: usize| {
            i -= 1;
            while i > least && !stops[i] {
                i -= 1;
            }
            i
        };
        const ZWJ: char = '\u{200d}';
        // The bidirectional control characters of what is cut stay.
        let controls = |part: &str| -> String {
            let keep =
                |c: &char| matches!(*c as u32, 0x202a..=0x202e | 0x200e..=0x200f | 0x2066..=0x2069);
            part.chars().filter(keep).collect()
        };
        match mode {
            1 => {
                let (mut current, mut stop) = (0, 0);
                let mut pos;
                loop {
                    pos = stop;
                    stop = next(stop);
                    current += w(pos, stop);
                    if !(stop < len && current < available) {
                        break;
                    }
                }
                if next_char_joins(&text[pos..]) {
                    ellipsis.insert(0, ZWJ);
                }
                format!("{}{ellipsis}{}", &text[..pos], controls(&text[pos..]))
            }
            0 => {
                let (mut current, mut stop) = (0, len);
                let mut pos;
                loop {
                    pos = stop;
                    stop = prev(stop, 0);
                    current += w(stop, pos);
                    if !(stop > 0 && current < available) {
                        break;
                    }
                }
                if prev_char_joins(&text[..pos]) {
                    ellipsis.push(ZWJ);
                }
                format!("{}{ellipsis}{}", controls(&text[..pos]), &text[pos..])
            }
            _ => {
                let (mut left_width, mut right_width) = (0, 0);
                let (mut next_left, mut next_right) = (0, len);
                let (mut left, mut right);
                loop {
                    left = next_left;
                    right = next_right;
                    next_left = next(next_left);
                    next_right = prev(next_right, 0);
                    left_width += w(left, next_left);
                    right_width += w(next_right, right);
                    if !(next_left < len && next_right > 0 && left_width + right_width < available)
                    {
                        break;
                    }
                }
                if next_char_joins(&text[left..]) {
                    ellipsis.insert(0, ZWJ);
                }
                if prev_char_joins(&text[..right]) {
                    ellipsis.push(ZWJ);
                }
                format!("{}{ellipsis}{}", &text[..left], &text[right..])
            }
        }
    }

    /// ri::wstat: (shaped lines, glyph bitmaps, distance fields + tiles) cached by this stack.
    pub fn cache_stats(&self) -> (usize, usize, usize) {
        let mut r = (self.shaped.borrow().len(), 0, 0);
        let fonts = std::iter::once(&self.primary).chain(
            self.fallbacks
                .iter()
                .filter_map(|f| f.get().and_then(|f| f.as_deref())),
        );
        for f in fonts {
            r.1 += f.glyphs.borrow().len();
            r.2 += f.df.borrow().state().2 + f.df_tiles.borrow().len();
        }
        r
    }

    pub fn shape(&self, text: &str) -> ShapedLine {
        if let Some(l) = self.shaped.borrow_mut().get(text) {
            return l.clone();
        }
        let l = self.shape_uncached(text, None);
        self.shaped.borrow_mut().put(text.to_string(), l.clone());
        l
    }

    /// A line of a text read right to left (`rtl`) or left to right: QTextEngine resolves the
    /// bidi levels of all lines of a text from the direction of the whole text.
    pub fn shape_line(&self, text: &str, rtl: bool) -> ShapedLine {
        if is_right_to_left(text) == rtl {
            return self.shape(text);
        }
        // The key of a line shaped the other way than its own characters tell.
        if text.starts_with('\u{ffff}') {
            return self.shape_uncached(text, Some(rtl));
        }
        let key = format!("\u{ffff}{text}");
        if let Some(l) = self.shaped.borrow_mut().get(&key) {
            return l.clone();
        }
        let l = self.shape_uncached(text, Some(rtl));
        self.shaped.borrow_mut().put(key, l.clone());
        l
    }

    fn emoji_font_for(&self, c: char) -> Option<usize> {
        let i = self.fallbacks.len() + 1;
        self.font(i).filter(|f| f.has_char(c)).map(|_| i)
    }

    /// The font of a character of script `script` of a line, after the font of the character
    /// before it; `emoji` tells a character of an emoji sequence: 1 the first of one, 2 any
    /// after it.
    fn run_font(&self, last: Option<usize>, c: char, emoji: u8, script: hb::hb_script_t) -> usize {
        let emoji_idx = self.fallbacks.len() + 1;
        if emoji == 2 && last == Some(emoji_idx) {
            emoji_idx
        } else if emoji > 0
            && let Some(e) = self.emoji_font_for(c)
        {
            e
        } else if matches!(c, '\u{200C}' | '\u{200D}')
            && let Some(l) = last.filter(|&l| l > 0 && self.font(l).is_some_and(|f| f.has_char(c)))
        {
            // QFontEngineMulti::stringToCMap: a joiner is shaped with the fallback font of the
            // character before it. Nothing else follows that font: a combining mark is set
            // in the first font that has it, like any character.
            l
        } else if matches!(c as u32, 0xFE00..=0xFE0F) {
            // QFontEngine::isIgnorableChar: no fallback font is looked up for it.
            0
        } else {
            self.font_for(c, script)
        }
    }

    /// `text` shaped as a whole: what a Text that wraps or elides it measures its lines with.
    pub fn shape_text(&self, text: &str) -> std::rc::Rc<ShapedText> {
        if let Some(t) = self.texts.borrow_mut().get(text) {
            return t.clone();
        }
        let runs = itemize(text, None, |last, c, emoji, script| {
            if unprinted(c) {
                last.unwrap_or(0)
            } else {
                self.run_font(last, c, emoji, script)
            }
        });
        let runs = apart(text, runs);
        let mut clusters = Vec::new();
        let mut items: Vec<(usize, usize, [i64; 3])> = Vec::new();
        let mut scripts = vec![None; text.len()];
        let mut overhangs = vec![0; text.len()];
        // Per item, its glyphs as QTextEngine keeps them (each font's run as HarfBuzz lists
        // it): the byte each one's cluster starts at and how far past its advance it reaches.
        let mut item_glyphs: Vec<Vec<(usize, i64)>> = Vec::new();
        let mut last: Option<&(Run, bool)> = None;
        for piece in &runs {
            let (r, alone) = piece;
            let font = self.font(r.font).unwrap();
            let shaped = clusters.len();
            let (glyphs, at_bytes, _, _) = font.shape_run_clusters(text, r, Some(&mut clusters));
            let overhang = |at: usize, gid: u32| {
                if font.color_scale != 0.0 || text[at..].starts_with(unprinted) {
                    return 0;
                }
                let (left, wd, adv) = font.glyph_box(gid);
                (left + wd - adv).max(0) << 6
            };
            // The cluster's first glyph in reading order; HarfBuzz lists the glyphs of a
            // right-to-left run from the left, last to first.
            let mut seen = None;
            for &(at, _, gid) in &clusters[shaped..] {
                if r.level.is_rtl() || seen != Some(at) {
                    overhangs[at] = overhang(at, gid);
                }
                seen = Some(at);
            }
            let mut reaches: Vec<(usize, i64)> = glyphs
                .iter()
                .zip(&at_bytes)
                .map(|(g, &at)| (at as usize, overhang(at as usize, g.0)))
                .collect();
            // QTextEngine::shapeTextWithHarfbuzzNG reverses what HarfBuzz lists from the left.
            if r.level.is_rtl() {
                reaches.reverse();
            }
            let own = [font.ascent, font.descent, font.leading];
            let joins = |l: &(Run, bool)| {
                !l.1 && !alone
                    && l.0.script == r.script
                    && l.0.emoji == r.emoji
                    && l.0.level == r.level
            };
            match items.last_mut() {
                Some(item) if last.is_some_and(joins) => {
                    item.1 = r.end;
                    item.2 = [0, 1, 2].map(|k| item.2[k].max(own[k]));
                    item_glyphs.last_mut().unwrap().extend(reaches);
                }
                _ => {
                    item_glyphs.push(reaches);
                    let first = self.item_font(r);
                    let first = [first.ascent, first.descent, first.leading];
                    items.push((r.start, r.end, [0, 1, 2].map(|k| first[k].max(own[k]))));
                    scripts[r.start] = Some(r.script.to_be_bytes());
                }
            }
            last = Some(piece);
        }
        let mut advances = vec![0; text.len()];
        let mut starts = vec![false; text.len()];
        for (at, advance, _) in clusters {
            starts[at] = true;
            if !text[at..].starts_with(unprinted) {
                advances[at] += advance;
            }
        }
        let mut before = Vec::with_capacity(text.len() + 1);
        let mut sum = 0;
        before.push(0);
        for advance in advances {
            sum += advance;
            before.push(sum);
        }
        // logClusters of an item's last character: the first glyph of its last cluster.
        let last_glyph = |item: &[(usize, i64)]| {
            let last = item.iter().map(|g| g.0).max();
            item.iter().position(|g| Some(g.0) == last).unwrap_or(0)
        };
        let mut strays = vec![None; text.len()];
        for (before, item) in item_glyphs.iter().zip(item_glyphs.iter().skip(1)) {
            if let Some(&(at, _)) = item.iter().min_by_key(|g| g.0) {
                strays[at] = Some(item.get(last_glyph(before)).map_or(-1, |g| g.1));
            }
        }
        let reaches = |item: &Vec<(usize, i64)>| item.iter().map(|g| g.1).collect();
        let shaped = std::rc::Rc::new(ShapedText {
            before,
            starts,
            items,
            scripts,
            overhangs,
            strays,
            first_item: item_glyphs.first().map(reaches).unwrap_or_default(),
            last_glyph: item_glyphs.last().map_or(0, |item| last_glyph(item)),
        });
        self.texts
            .borrow_mut()
            .put(text.to_string(), shaped.clone());
        shaped
    }

    /// The bytes `part` of the paragraph `text` as a line of it: set in the paragraph's
    /// direction and shaped inside its words, as a QTextLine of the paragraph's layout is.
    pub fn shape_part(&self, text: &str, part: std::ops::Range<usize>) -> ShapedLine {
        self.shape_part_in(text, None, part)
    }

    /// `shape_part` of a paragraph whose direction is `rtl` whatever its text
    /// (QTextOption::setTextDirection), its text's own when None.
    pub fn shape_part_in(
        &self,
        text: &str,
        rtl: Option<bool>,
        part: std::ops::Range<usize>,
    ) -> ShapedLine {
        let rtl = rtl.filter(|&rtl| rtl != is_rtl(text));
        if part == (0..text.len()) && rtl.is_none() {
            return self.shape(text);
        }
        let key = format!("{text}\0{}\0{}\0{rtl:?}", part.start, part.end);
        if let Some(l) = self.shaped.borrow_mut().get(&key) {
            return l.clone();
        }
        let runs = self
            .runs(text, rtl)
            .into_iter()
            .filter(|r| r.start < part.end && part.start < r.end)
            .map(|mut r| {
                (r.start, r.end) = (r.start.max(part.start), r.end.min(part.end));
                r
            })
            .collect();
        let l = self.shape_runs(text, runs);
        self.shaped.borrow_mut().put(key, l.clone());
        l
    }

    /// The first engine of the item a run is part of: the base font of its script. An emoji
    /// sequence is an item of the emoji script (QFontDatabasePrivate::Script_Emoji), whose
    /// engine has the emoji font first.
    fn item_font(&self, r: &Run) -> &Font {
        if r.emoji
            && let Some(f) = self.font(self.fallbacks.len() + 1)
        {
            return f;
        }
        self.script_stack(r.script)
            .map_or(&self.primary, |(_, sub)| &sub.primary)
    }

    /// The line's runs: `itemize` with this stack's font for every character.
    fn runs(&self, text: &str, rtl: Option<bool>) -> Vec<Run> {
        itemize(text, rtl, |last, c, emoji, script| {
            self.run_font(last, c, emoji, script)
        })
    }

    /// Where the caret stands in the line `text` as `shape` sets it. `boundary` tells the bytes
    /// of `text` a grapheme starts at.
    pub fn line_map(&self, text: &str, boundary: &dyn Fn(usize) -> bool) -> LineMap {
        self.line_map_in(text, None, boundary)
    }

    /// `line_map` of a paragraph whose direction is `rtl` whatever its text
    /// (QTextOption::setTextDirection), its text's own when None.
    pub fn line_map_in(
        &self,
        text: &str,
        rtl: Option<bool>,
        boundary: &dyn Fn(usize) -> bool,
    ) -> LineMap {
        let mut unit_at = vec![0; text.len() + 1];
        let mut units = Vec::new();
        let mut bound = Vec::new();
        for (b, c) in text.char_indices() {
            units.push(bound.len());
            unit_at[b] = bound.len();
            bound.push(boundary(b));
            bound.extend(std::iter::repeat_n(false, c.len_utf16() - 1));
        }
        units.push(bound.len());
        unit_at[text.len()] = bound.len();
        bound.push(true);
        let mut items: Vec<MapItem> = Vec::new();
        let mut last: Option<Run> = None;
        for r in tabs_apart(text, self.runs(text, rtl)) {
            if is_tab(text, &r) {
                items.push(MapItem {
                    at: unit_at[r.start],
                    len: 1,
                    level: r.level.number(),
                    split: false,
                    tab: true,
                    advances: vec![0],
                    starts: vec![true],
                    clusters: vec![0],
                });
                last = None;
                continue;
            }
            let mut glyphs = Vec::new();
            self.font(r.font)
                .unwrap()
                .shape_run(text, &r, Some(&mut glyphs));
            if r.level.is_rtl() {
                glyphs.reverse();
            }
            let joins = |l: &Run| l.script == r.script && l.emoji == r.emoji && l.level == r.level;
            if !last.as_ref().is_some_and(joins) {
                const SPLIT: [&str; 7] = ["Zyyy", "Grek", "Latn", "Hira", "Kana", "Bopo", "Hani"];
                items.push(MapItem {
                    at: unit_at[r.start],
                    len: 0,
                    level: r.level.number(),
                    // An emoji sequence is an item of QChar::Script_Emoji.
                    split: !r.emoji && SPLIT.iter().any(|s| tag(s) == r.script),
                    tab: false,
                    advances: vec![],
                    starts: vec![],
                    clusters: vec![],
                });
            }
            let item = items.last_mut().unwrap();
            let mut cluster = None;
            for (at, advance, _) in glyphs {
                if cluster != Some(at) {
                    // The units up to this cluster belong to the one before it.
                    let before = item.starts.iter().rposition(|&s| s).unwrap_or(0);
                    item.clusters.resize(unit_at[at] - item.at, before);
                }
                item.starts.push(cluster != Some(at));
                item.advances.push(advance);
                cluster = Some(at);
            }
            item.len = unit_at[r.end] - item.at;
            let before = item.starts.iter().rposition(|&s| s).unwrap_or(0);
            item.clusters.resize(item.len, before);
            last = Some(r);
        }
        let mut map = LineMap {
            rtl: rtl.unwrap_or_else(|| is_rtl(text)),
            width: self.natural_width(&self.shape_part_in(text, rtl, 0..text.len())),
            items,
            bound,
            units,
        };
        let whole = map.whole();
        map.shape_line(&whole);
        map
    }

    /// The part `from..to` of the line `text`, a text read right to left (`rtl`) or left to
    /// right, where one format of the line holds (QTextEngine::addRequiredBoundaries ends an
    /// item at each end of a format): its bidi levels are those of the whole line and HarfBuzz
    /// gets the line as the context around it, so a letter joins its neighbour in another
    /// format.
    pub fn shape_within(&self, text: &str, from: usize, to: usize, rtl: bool) -> ShapedLine {
        let key = format!("\u{fffe}{}{from}:{to}:{text}", rtl as u8);
        if let Some(l) = self.shaped.borrow_mut().get(&key) {
            return l.clone();
        }
        let runs: Vec<Run> = self
            .runs(text, Some(rtl))
            .into_iter()
            .filter(|r| r.start < to && r.end > from)
            .map(|mut r| {
                (r.start, r.end) = (r.start.max(from), r.end.min(to));
                r
            })
            .collect();
        let l = self.shape_runs(text, runs);
        self.shaped.borrow_mut().put(key, l.clone());
        l
    }

    fn shape_uncached(&self, text: &str, rtl: Option<bool>) -> ShapedLine {
        self.shape_runs(text, self.runs(text, rtl))
    }

    fn shape_runs(&self, text: &str, runs: Vec<Run>) -> ShapedLine {
        // An item's height is its first engine's and that of every font it is set in
        // (QTextEngine::shapeText); a line without text has the primary font's.
        let (mut ascent, mut descent, mut leading) = (0, 0, 0);
        if runs.is_empty() {
            (ascent, descent) = (self.primary.ascent, self.primary.descent);
            leading = self.primary.leading;
        }
        let runs = tabs_apart(text, runs);
        for r in &runs {
            // QTextEngine::shape gives a tab the height of its item's engine alone.
            let own = (!is_tab(text, r)).then(|| self.font(r.font).unwrap());
            for font in std::iter::once(self.item_font(r)).chain(own) {
                ascent = ascent.max(font.ascent);
                descent = descent.max(font.descent);
                leading = leading.max(font.leading);
            }
        }
        let (glyphs, last_cluster, advance) = place_runs(text, &runs, |f| self.font(f).unwrap());
        ShapedLine {
            glyphs,
            last_cluster,
            advance,
            ascent,
            descent,
            leading,
        }
    }

    #[allow(clippy::too_many_arguments)]
    /// Glyph placements of a shaped line, matching `draw_line`.
    pub fn line_quads(
        &self,
        line: &ShapedLine,
        tx: f64,
        ty: f64,
        x: f64,
        y: f64,
    ) -> Vec<GlyphQuad> {
        let baseline = y + line.ascent as f64 / 64.0;
        let mut out = Vec::new();
        let mut i = 0;
        while i < line.glyphs.len() {
            let f = line.glyphs[i].0;
            let j = line.glyphs[i..]
                .iter()
                .position(|g| g.0 != f)
                .map_or(line.glyphs.len(), |k| i + k);
            let run: Vec<(u32, i64, i64)> =
                line.glyphs[i..j].iter().map(|g| (g.1, g.2, g.3)).collect();
            self.font(f)
                .unwrap()
                .glyph_quads(&run, tx, ty, x, baseline, &mut out);
            i = j;
        }
        out
    }

    /// `line_quads` at unsnapped positions, for drawing through a scale transform.
    pub fn line_quads_f(
        &self,
        line: &ShapedLine,
        tx: f64,
        ty: f64,
        x: f64,
        y: f64,
    ) -> Vec<GlyphQuadF> {
        let baseline = y + line.ascent as f64 / 64.0;
        let mut out = Vec::new();
        let mut i = 0;
        while i < line.glyphs.len() {
            let f = line.glyphs[i].0;
            let j = line.glyphs[i..]
                .iter()
                .position(|g| g.0 != f)
                .map_or(line.glyphs.len(), |k| i + k);
            let run: Vec<(u32, i64, i64)> =
                line.glyphs[i..j].iter().map(|g| (g.1, g.2, g.3)).collect();
            self.font(f)
                .unwrap()
                .glyph_quads_f(&run, tx, ty, x, baseline, &mut out);
            i = j;
        }
        out
    }

    /// The glyphs of a line that have no outline (a colour font's, a bitmap strike's): Qt draws
    /// them with a native glyph node even under `Text.QtRendering`
    /// (QFontEngine::hasUnreliableGlyphOutline, QFontEngine::smoothScalable).
    pub fn color_part(&self, line: &ShapedLine) -> Option<ShapedLine> {
        let glyphs: Vec<_> = line
            .glyphs
            .iter()
            .filter(|g| self.font(g.0).is_some_and(|f| !f.has_outlines()))
            .copied()
            .collect();
        (!glyphs.is_empty()).then_some(ShapedLine {
            glyphs,
            last_cluster: 0,
            advance: line.advance,
            ascent: line.ascent,
            leading: line.leading,
            descent: line.descent,
        })
    }

    /// The glyphs of a `Text.QtRendering` line enter Qt's glyph cache, one glyph node per
    /// font. Qt does this for every text before it draws any (see `line_df_tiles`).
    pub fn line_df_populate(&self, line: &ShapedLine, quality: i32) {
        self.lines_df_populate(std::slice::from_ref(line), quality);
    }

    /// The glyphs of a `Text.QtRendering` text enter Qt's glyph cache. Its lines are one
    /// glyph node per font (QQuickTextNodeEngine::mergeProcessedNodes joins the runs of one
    /// font and colour), the fonts in the order the text comes to them, and a node's new
    /// glyphs are placed together.
    pub fn lines_df_populate(&self, lines: &[ShapedLine], quality: i32) {
        let mut seen: Vec<usize> = vec![];
        for g in lines.iter().flat_map(|l| &l.glyphs) {
            if !seen.contains(&g.0) {
                seen.push(g.0);
                if let Some(f) = self.font(g.0).filter(|f| f.has_outlines()) {
                    let gids: Vec<u32> = lines
                        .iter()
                        .flat_map(|l| &l.glyphs)
                        .filter(|h| h.0 == g.0)
                        .map(|h| h.1)
                        .collect();
                    f.df_populate(&gids, quality);
                }
            }
        }
    }

    /// The glyph nodes of a `Text.QtRendering` text, as the number of quads of each: a glyph
    /// with no field has none (QSGDistanceFieldGlyphNode::updateGeometry skips it).
    pub fn lines_df_quads(&self, lines: &[ShapedLine], quality: i32) -> Vec<usize> {
        let mut seen: Vec<usize> = vec![];
        let mut out = vec![];
        for g in lines.iter().flat_map(|l| &l.glyphs) {
            if !seen.contains(&g.0) {
                seen.push(g.0);
                if let Some(f) = self.font(g.0).filter(|f| f.has_outlines()) {
                    let gids = lines.iter().flat_map(|l| &l.glyphs);
                    out.push(
                        gids.filter(|h| h.0 == g.0 && f.df_has_field(h.1, quality))
                            .count(),
                    );
                }
            }
        }
        out
    }

    /// `Text.QtRendering` tiles of a shaped line (see `line_quads`) drawn into a `view` wide
    /// and high. The line is one glyph node per font: its glyphs enter Qt's glyph cache
    /// together, the first time they are drawn.
    #[allow(clippy::too_many_arguments)]
    pub fn line_df_tiles(
        &self,
        line: &ShapedLine,
        quality: i32,
        tx: f64,
        ty: f64,
        x: f64,
        y: f64,
        color: [f32; 4],
        look: crate::dftext::DfLook,
        turn: Option<crate::dftext::DfTurn>,
        view: crate::dftext::DfView,
    ) -> Vec<crate::dftext::DfTile> {
        let baseline = y + line.ascent as f64 / 64.0;
        let outlined = |i: usize| self.font(i).filter(|f| f.has_outlines());
        self.line_df_populate(line, quality);
        let mut out = Vec::new();
        for g in &line.glyphs {
            if let Some(f) = outlined(g.0) {
                let one = [(g.1, g.2, g.3)];
                f.df_tiles(
                    &one, quality, tx, ty, x, baseline, color, look, turn, view, &mut out,
                );
            }
        }
        out
    }

    /// Draws `line` with its top-left at `at` in node space, translated by `t` (`draw_glyphs`).
    pub fn draw_line(
        &self,
        cv: &mut Canvas,
        line: &ShapedLine,
        t: [f64; 2],
        at: [f64; 2],
        color: [u8; 4],
        opacity: f32,
    ) {
        let ([tx, ty], [x, y]) = (t, at);
        let baseline = y + line.ascent as f64 / 64.0;
        let mut i = 0;
        while i < line.glyphs.len() {
            let f = line.glyphs[i].0;
            let j = line.glyphs[i..]
                .iter()
                .position(|g| g.0 != f)
                .map_or(line.glyphs.len(), |k| i + k);
            let run: Vec<(u32, i64, i64)> =
                line.glyphs[i..j].iter().map(|g| (g.1, g.2, g.3)).collect();
            self.font(f)
                .unwrap()
                .draw_glyphs(cv, &run, tx, ty, x, baseline, color, opacity);
            i = j;
        }
    }

    /// `draw_line` through the scale `s` of a layer texture that is not its scene rect's size:
    /// the corners of a glyph's quad are snapped after the scale (textmask.vert) and the
    /// bitmap is read at the nearest texel. `against` is the texture's axes that run against
    /// the screen's (`snap_px`).
    #[allow(clippy::too_many_arguments)]
    pub fn draw_line_scaled(
        &self,
        cv: &mut Canvas,
        line: &ShapedLine,
        t: [f64; 2],
        at: [f64; 2],
        color: [u8; 4],
        opacity: f32,
        s: [f64; 2],
        against: [bool; 2],
    ) {
        let a = color[3] as f32 / 255.0 * opacity;
        let c = [
            color[0] as f32 / 255.0 * a,
            color[1] as f32 / 255.0 * a,
            color[2] as f32 / 255.0 * a,
            a,
        ];
        let (sx, sy) = (s[0] as f32, s[1] as f32);
        let (ox, oy) = ((s[0] * t[0]) as f32, (s[1] * t[1]) as f32);
        let snap = |v: f32, k: usize| snap_px(v, against[k]) as i64;
        for (g, x, y) in self.line_quads_f(line, 0.0, 0.0, at[0], at[1]) {
            let (x, y) = (x as f32, y as f32);
            let (x0, y0) = (snap(sx * x + ox, 0), snap(sy * y + oy, 1));
            let x1 = snap(sx * (x + g.width as f32) + ox, 0);
            let y1 = snap(sy * (y + g.rows as f32) + oy, 1);
            for yy in y0.max(0)..y1.min(cv.h as i64) {
                let v = (yy - y0) as f64 + 0.5;
                let row = ((v / (y1 - y0) as f64 * g.rows as f64) as usize).min(g.rows - 1);
                for xx in x0.max(0)..x1.min(cv.w as i64) {
                    let u = (xx - x0) as f64 + 0.5;
                    let col = ((u / (x1 - x0) as f64 * g.width as f64) as usize).min(g.width - 1);
                    glyph_px(cv, &g, c, col, row, xx as usize, yy as usize);
                }
            }
        }
    }
}

/// QFontMetrics' separator of a text's variants of different lengths.
const VARIANT_SEPARATOR: char = '\u{9c}';

/// QTextEngine's nextCharJoins: the first character of `rest` that is no non-spacing mark
/// has a joining type other than none and transparent.
pub fn next_char_joins(rest: &str) -> bool {
    use icu_properties::props::JoiningType as J;
    let joining = icu_properties::CodePointMapData::<J>::new();
    rest.chars()
        .find(|&c| !nonspacing_mark(c))
        .is_some_and(|c| !matches!(joining.get(c), J::NonJoining | J::Transparent))
}

/// QTextEngine's prevCharJoins: the last character of `head` that is no non-spacing mark
/// joins on both sides or causes joining.
pub fn prev_char_joins(head: &str) -> bool {
    use icu_properties::props::JoiningType as J;
    let joining = icu_properties::CodePointMapData::<J>::new();
    head.chars()
        .rev()
        .find(|&c| !nonspacing_mark(c))
        .is_some_and(|c| matches!(joining.get(c), J::DualJoining | J::JoinCausing))
}

/// QChar::category() == QChar::Mark_NonSpacing.
fn nonspacing_mark(c: char) -> bool {
    use icu_properties::props::GeneralCategory as G;
    icu_properties::CodePointMapData::<G>::new().get(c) == G::NonspacingMark
}

/// QFixed / QFixed, both 26.6: rounded to the nearest 1/64, half away from zero.
fn qfixed_div(a: i64, b: i64) -> i64 {
    let neg = (a < 0) != (b < 0);
    let (a, b) = (a.abs(), b.abs());
    let r = ((a << 6) + (b >> 1)) / b;
    if neg { -r } else { r }
}

/// A glyph of a shaped run (`Font::shape_run`): its index, its (x, y) from the run's left end and
/// its advance, 26.6.
type RunGlyph = (u32, i64, i64, i64);

/// Qt's glyph_metrics_t in 26.6: a box and the advance of what it holds. A new one has no
/// extent and starts far to the right and below, so that the first box merged sets it.
#[derive(Clone, Copy)]
struct GlyphBox {
    x: i64,
    y: i64,
    width: i64,
    height: i64,
    xoff: i64,
}

impl Default for GlyphBox {
    fn default() -> Self {
        GlyphBox {
            x: 100000 << 6,
            y: 100000 << 6,
            width: 0,
            height: 0,
            xoff: 0,
        }
    }
}

/// A stretch of a line shaped in one HarfBuzz call: one font, one script, one bidi level.
struct Run {
    font: usize,
    /// Byte range in the line.
    start: usize,
    end: usize,
    script: hb::hb_script_t,
    /// An emoji sequence, or sequences in a row: an item of its own, shaped as Common script.
    emoji: bool,
    level: unicode_bidi::Level,
}

/// The character data the Unicode Bidirectional Algorithm reads, from the Unicode version of
/// the rest of the engine's data and of Qt 6.12 (`icu_properties`): `unicode_bidi` runs the
/// rules, its own tables are of another version and are not built in.
struct BidiData;

/// Bidi_Class, as QChar::direction gives it. A code point with no character has the class of
/// its block: Qt's table generator (util/unicode/main.cpp, `UnicodeData`) takes the blocks
/// from its own list, in which an Arabic letter's blocks come first and a right-to-left one's
/// second, and not from DerivedBidiClass.txt, which has given more blocks to Arabic letters
/// since.
fn bidi_class(c: char) -> unicode_bidi::BidiClass {
    use icu_properties::props::GeneralCategory;
    use unicode_bidi::BidiClass::{AL, R};
    const QT_AL: [std::ops::RangeInclusive<u32>; 6] = [
        0x0600..=0x07BF,
        0x08A0..=0x08FF,
        0xFB50..=0xFDCF,
        0xFDF0..=0xFDFF,
        0xFE70..=0xFEFF,
        0x1EE00..=0x1EEFF,
    ];
    const QT_R: [std::ops::RangeInclusive<u32>; 6] = [
        0x0590..=0x05FF,
        0x07C0..=0x089F,
        0xFB1D..=0xFB4F,
        0x10800..=0x10FFF,
        0x1E800..=0x1EDFF,
        0x1EF00..=0x1EFFF,
    ];
    let class = ucd_bidi_class(c);
    if !matches!(class, AL | R)
        || icu_properties::CodePointMapData::<GeneralCategory>::new().get(c)
            != GeneralCategory::Unassigned
    {
        return class;
    }
    let within =
        |ranges: &[std::ops::RangeInclusive<u32>]| ranges.iter().any(|r| r.contains(&u32::from(c)));
    if within(&QT_AL) {
        AL
    } else if within(&QT_R) {
        R
    } else {
        class
    }
}

/// Bidi_Class of the Unicode Character Database.
fn ucd_bidi_class(c: char) -> unicode_bidi::BidiClass {
    use icu_properties::props::BidiClass as B;
    use unicode_bidi::BidiClass as U;
    match icu_properties::CodePointMapData::<B>::new().get(c) {
        B::LeftToRight => U::L,
        B::RightToLeft => U::R,
        B::EuropeanNumber => U::EN,
        B::EuropeanSeparator => U::ES,
        B::EuropeanTerminator => U::ET,
        B::ArabicNumber => U::AN,
        B::CommonSeparator => U::CS,
        B::ParagraphSeparator => U::B,
        B::SegmentSeparator => U::S,
        B::WhiteSpace => U::WS,
        B::LeftToRightEmbedding => U::LRE,
        B::LeftToRightOverride => U::LRO,
        B::ArabicLetter => U::AL,
        B::RightToLeftEmbedding => U::RLE,
        B::RightToLeftOverride => U::RLO,
        B::PopDirectionalFormat => U::PDF,
        B::NonspacingMark => U::NSM,
        B::BoundaryNeutral => U::BN,
        B::FirstStrongIsolate => U::FSI,
        B::LeftToRightIsolate => U::LRI,
        B::RightToLeftIsolate => U::RLI,
        B::PopDirectionalIsolate => U::PDI,
        _ => U::ON,
    }
}

/// Bidi_Paired_Bracket of `c` and whether `c` opens the pair (BidiBrackets.txt): the brackets
/// rule N0 pairs. QTextEngine takes the same pairs from the mirrored character of an opening
/// or closing punctuation mark (resolveN0).
fn bidi_bracket(c: char) -> Option<(char, bool)> {
    use icu_properties::props::{BidiMirroringGlyph, BidiPairedBracketType as T};
    let b = icu_properties::CodePointMapData::<BidiMirroringGlyph>::new().get(c);
    match b.paired_bracket_type {
        T::Open => Some((b.mirroring_glyph?, true)),
        T::Close => Some((b.mirroring_glyph?, false)),
        _ => None,
    }
}

impl unicode_bidi::BidiDataSource for BidiData {
    fn bidi_class(&self, c: char) -> unicode_bidi::BidiClass {
        bidi_class(c)
    }

    fn bidi_matched_opening_bracket(
        &self,
        c: char,
    ) -> Option<unicode_bidi::data_source::BidiMatchedOpeningBracket> {
        let (other, is_open) = bidi_bracket(c)?;
        // U+2329 and U+232A are canonically U+3008 and U+3009 and pair with them (UAX #9,
        // BD16), the one such pair of brackets (QTextEngine's BracketStack::match).
        let opening = match if is_open { c } else { other } {
            '\u{2329}' => '\u{3008}',
            o => o,
        };
        Some(unicode_bidi::data_source::BidiMatchedOpeningBracket { opening, is_open })
    }
}

/// QString::isRightToLeft: the first letter with a direction outside an isolate is one read
/// right to left.
pub fn is_right_to_left(text: &str) -> bool {
    use unicode_bidi::BidiClass::{AL, FSI, L, LRI, PDI, R, RLI};
    let mut isolates = 0usize;
    for c in text.chars() {
        match bidi_class(c) {
            LRI | RLI | FSI => isolates += 1,
            PDI => isolates = isolates.saturating_sub(1),
            L if isolates == 0 => return false,
            R | AL if isolates == 0 => return true,
            _ => {}
        }
    }
    false
}

/// Embedding level of every byte of a line: the Unicode Bidirectional Algorithm (UAX #9) as
/// QTextEngine::itemize runs it, with rule L1 applied. `rtl` is the direction of the text the
/// line is one of; without it the line's own first strong character tells it
/// (Qt::LayoutDirectionAuto, QString::isRightToLeft). A line read left to right without a
/// character from U+0590 on has no levels to resolve (QBidiAlgorithm::checkForBidi).
fn bidi_levels(text: &str, rtl: Option<bool>) -> Vec<unicode_bidi::Level> {
    use unicode_bidi::BidiClass::{BN, LRE, LRO, PDF, RLE, RLO};
    if rtl != Some(true) && text.chars().all(|c| c < '\u{590}') {
        return vec![unicode_bidi::Level::ltr(); text.len()];
    }
    let base = rtl.map(|rtl| match rtl {
        true => unicode_bidi::Level::rtl(),
        false => unicode_bidi::Level::ltr(),
    });
    let info = unicode_bidi::ParagraphBidiInfo::new_with_data_source(&BidiData, text, base);
    let mut levels = info.reordered_levels(0..text.len());
    // UAX #9 drops the embedding controls and boundary neutrals (rule X9); Qt keeps them in
    // the line at the lower level of the text on either side (QBidiAlgorithm::process).
    let base = info.paragraph_level;
    let mut last = base;
    let mut ignored: Option<usize> = None;
    for (b, c) in text.char_indices() {
        if matches!(bidi_class(c), LRE | RLE | LRO | RLO | PDF | BN) {
            ignored.get_or_insert(b);
            levels[b..b + c.len_utf8()].fill(last);
            continue;
        }
        let level = levels[b];
        if let Some(from) = ignored.take()
            && level < last
        {
            levels[from..b].fill(level);
        }
        last = level;
    }
    if let Some(from) = ignored
        && base < last
    {
        levels[from..].fill(base);
    }
    levels
}

/// The script of every character's shaping item as (first byte, script), split as
/// QUnicodeTools::initScripts does: Common, Inherited and unassigned characters and combining
/// marks stay with the text before them, and such characters opening the line join the first
/// script after them.
fn script_items(text: &str) -> Vec<(usize, hb::hb_script_t)> {
    use icu_properties::props::GeneralCategory as G;
    let weak = |s| {
        matches!(
            s,
            hb::HB_SCRIPT_COMMON | hb::HB_SCRIPT_INHERITED | hb::HB_SCRIPT_UNKNOWN
        )
    };
    let categories = icu_properties::CodePointMapData::<G>::new();
    let mut items = vec![];
    let (mut start, mut script) = (0, hb::HB_SCRIPT_COMMON);
    for (b, c) in text.char_indices() {
        let next = crate::hb_unicode::script(c);
        if next == script || weak(next) {
            continue;
        }
        if script == hb::HB_SCRIPT_COMMON {
            script = next;
            continue;
        }
        if matches!(
            categories.get(c),
            G::NonspacingMark | G::SpacingMark | G::EnclosingMark
        ) {
            continue;
        }
        items.push((start, script));
        (start, script) = (b, next);
    }
    items.push((start, script));
    items
}

/// The stretches of one bidi level of the line `text`, a text read right to left (`rtl`) or
/// left to right: (start, end, level) in the order of the text.
pub fn level_runs(text: &str, rtl: bool) -> Vec<(usize, usize, u8)> {
    let levels = bidi_levels(text, Some(rtl));
    let mut runs: Vec<(usize, usize, u8)> = vec![];
    for (b, c) in text.char_indices() {
        let (level, end) = (levels[b].number(), b + c.len_utf8());
        match runs.last_mut() {
            Some(r) if r.2 == level => r.1 = end,
            _ => runs.push((b, end, level)),
        }
    }
    runs
}

/// UAX #9 rule L2 (QTextEngine::bidiReorder): the order from the left of a line's items of
/// these bidi levels.
pub fn visual_order(levels: &[u8]) -> Vec<usize> {
    let levels: Vec<unicode_bidi::Level> = levels
        .iter()
        .map(|&l| unicode_bidi::Level::new(l).unwrap_or(unicode_bidi::Level::ltr()))
        .collect();
    unicode_bidi::BidiInfo::reorder_visual(&levels)
}

/// QTextEngine::itemize and the per-font split of QTextEngine::shapeText for one line, in the
/// order of the text: a run ends where the bidi level, the script or the font changes.
/// `font_for` gets the font of the character before, the character, its place in an emoji
/// sequence (0 outside one, 1 its first character, 2 a later one) and the script of the
/// character's item, and answers the character's font. Qt gives emoji sequences an item of
/// their own, shaped as Common script.
fn itemize(
    text: &str,
    rtl: Option<bool>,
    mut font_for: impl FnMut(Option<usize>, char, u8, hb::hb_script_t) -> usize,
) -> Vec<Run> {
    let levels = bidi_levels(text, rtl);
    let scripts = script_items(text);
    let emojis = crate::emoji::sequences(text);
    let mut item = 0;
    let mut runs: Vec<Run> = vec![];
    for (b, c) in text.char_indices() {
        if scripts.get(item + 1).is_some_and(|s| s.0 <= b) {
            item += 1;
        }
        let emoji = emojis[b] > 0;
        let script = if emoji {
            hb::HB_SCRIPT_COMMON
        } else {
            scripts[item].1
        };
        let font = font_for(runs.last().map(|r| r.font), c, emojis[b], scripts[item].1);
        let (level, end) = (levels[b], b + c.len_utf8());
        match runs.last_mut() {
            Some(r)
                if r.font == font && r.script == script && r.emoji == emoji && r.level == level =>
            {
                r.end = end
            }
            _ => runs.push(Run {
                font,
                start: b,
                end,
                script,
                emoji,
                level,
            }),
        }
    }
    runs
}

/// The runs of a whole text with every tab and line separator in a run of its own, marked
/// `true`: QTextEngine::itemize gives each an item of its own (QScriptAnalysis::Tab,
/// LineOrParagraphSeparator), so nothing is shaped together with one.
fn apart(text: &str, runs: Vec<Run>) -> Vec<(Run, bool)> {
    let mut pieces = Vec::with_capacity(runs.len());
    for r in runs {
        let piece = |start: usize, end: usize| Run {
            font: r.font,
            start,
            end,
            script: r.script,
            emoji: r.emoji,
            level: r.level,
        };
        let mut from = r.start;
        for (k, c) in text[r.start..r.end].char_indices() {
            if matches!(c, '\t' | '\n' | '\u{2028}') {
                let at = r.start + k;
                if from < at {
                    pieces.push((piece(from, at), false));
                }
                from = at + c.len_utf8();
                pieces.push((piece(at, from), true));
            }
        }
        if from < r.end {
            pieces.push((piece(from, r.end), false));
        }
    }
    pieces
}

/// The runs of a line with every tab in a run of its own (QScriptAnalysis::Tab).
fn tabs_apart(text: &str, runs: Vec<Run>) -> Vec<Run> {
    if !text.contains('\t') {
        return runs;
    }
    let mut pieces = Vec::with_capacity(runs.len());
    for r in runs {
        let mut from = r.start;
        let piece = |start: usize, end: usize| Run { start, end, ..r };
        for (k, _) in text[r.start..r.end].match_indices('\t') {
            let at = r.start + k;
            if from < at {
                pieces.push(piece(from, at));
            }
            from = at + 1;
            pieces.push(piece(at, from));
        }
        if from < r.end {
            pieces.push(piece(from, r.end));
        }
    }
    pieces
}

/// A run of `tabs_apart` that is a tab.
fn is_tab(text: &str, r: &Run) -> bool {
    &text[r.start..r.end] == "\t"
}

/// (font, glyph, x, y): a glyph of a line, in 26.6 from the line's left end and baseline.
type PlacedGlyph = (usize, u32, i64, i64);

/// The runs shaped and set side by side from the left in visual order (UAX #9 rule L2, as
/// QTextEngine::bidiReorder orders a line's items): (font, glyph, x, y) in 26.6, where the
/// last character's cluster starts among them, and the line's advance. The glyphs are listed
/// in the order of the text, as Qt keeps them (QGlyphLayout).
fn place_runs<'a>(
    text: &str,
    runs: &[Run],
    font: impl Fn(usize) -> &'a Font,
) -> (Vec<PlacedGlyph>, usize, i64) {
    let levels: Vec<unicode_bidi::Level> = runs.iter().map(|r| r.level).collect();
    // QTextEngine::shapeLine: a tab is as wide as the way from the end of the runs before it
    // in the text to the next tab stop, and has no glyph.
    let mut at = 0i64;
    let mut shaped: Vec<_> = runs
        .iter()
        .map(|r| {
            let (mut g, last, adv) = match is_tab(text, r) {
                true => (vec![], 0, tab_width(at)),
                false => font(r.font).shape_run(text, r, None),
            };
            if r.level.is_rtl() {
                g.reverse();
            }
            at += adv;
            (g, last, adv)
        })
        .collect();
    let mut placed = vec![(0i64, vec![]); runs.len()];
    let mut x = 0i64;
    let mut last_cluster = 0;
    for i in unicode_bidi::BidiInfo::reorder_visual(&levels) {
        let (g, last, adv) = std::mem::take(&mut shaped[i]);
        if i + 1 == runs.len() {
            last_cluster = last;
        }
        placed[i] = (x, g);
        x += adv;
    }
    let glyphs: Vec<_> = runs
        .iter()
        .zip(placed)
        .flat_map(|(r, (x0, g))| {
            g.into_iter()
                .map(move |(gid, gx, gy, _)| (r.font, gid, x0 + gx, gy))
        })
        .collect();
    let last_cluster = glyphs.len().saturating_sub(last_cluster);
    (glyphs, last_cluster, x)
}

/// QFontconfigDatabase's specialLanguages: the language whose fonts come first among the
/// fallbacks of a script's text, by the script's ISO 15924 tag. Latin text is set with the
/// Common engine (QFontPrivate::engineForScript), and Common and Han have no language of
/// their own: their fallbacks are sorted for the system's language.
const SCRIPT_LANGUAGES: [(&[u8; 4], &str); 120] = [
    (b"Grek", "el"),
    (b"Cyrl", "ru"),
    (b"Armn", "hy"),
    (b"Hebr", "he"),
    (b"Arab", "ar"),
    (b"Syrc", "syr"),
    (b"Thaa", "dv"),
    (b"Deva", "hi"),
    (b"Beng", "bn"),
    (b"Guru", "pa"),
    (b"Gujr", "gu"),
    (b"Orya", "or"),
    (b"Taml", "ta"),
    (b"Telu", "te"),
    (b"Knda", "kn"),
    (b"Mlym", "ml"),
    (b"Sinh", "si"),
    (b"Thai", "th"),
    (b"Laoo", "lo"),
    (b"Tibt", "bo"),
    (b"Mymr", "my"),
    (b"Geor", "ka"),
    (b"Hang", "ko"),
    (b"Ethi", "am"),
    (b"Cher", "chr"),
    (b"Cans", "cr"),
    (b"Ogam", "sga"),
    (b"Runr", "non"),
    (b"Khmr", "km"),
    (b"Mong", "mn"),
    (b"Hira", "ja"),
    (b"Kana", "ja"),
    (b"Bopo", "zh-TW"),
    (b"Yiii", "ii"),
    (b"Ital", "ett"),
    (b"Goth", "got"),
    (b"Dsrt", "en"),
    (b"Tglg", "fil"),
    (b"Hano", "hnn"),
    (b"Buhd", "bku"),
    (b"Tagb", "tbw"),
    (b"Copt", "cop"),
    (b"Limb", "lif"),
    (b"Tale", "tdd"),
    (b"Linb", "grc"),
    (b"Ugar", "uga"),
    (b"Shaw", "en"),
    (b"Osma", "so"),
    (b"Cprt", "grc"),
    (b"Bugi", "bug"),
    (b"Talu", "khb"),
    (b"Glag", "cu"),
    (b"Tfng", "shi"),
    (b"Sylo", "syl"),
    (b"Xpeo", "peo"),
    (b"Khar", "pra"),
    (b"Bali", "ban"),
    (b"Xsux", "akk"),
    (b"Phnx", "phn"),
    (b"Phag", "lzh"),
    (b"Nkoo", "man"),
    (b"Sund", "su"),
    (b"Lepc", "lep"),
    (b"Olck", "sat"),
    (b"Vaii", "vai"),
    (b"Saur", "saz"),
    (b"Kali", "eky"),
    (b"Rjng", "rej"),
    (b"Lyci", "xlc"),
    (b"Cari", "xcr"),
    (b"Lydi", "xld"),
    (b"Cham", "cjm"),
    (b"Lana", "nod"),
    (b"Tavt", "blt"),
    (b"Avst", "ae"),
    (b"Egyp", "egy"),
    (b"Samr", "smp"),
    (b"Lisu", "lis"),
    (b"Bamu", "bax"),
    (b"Java", "jv"),
    (b"Mtei", "mni"),
    (b"Armi", "arc"),
    (b"Sarb", "xsa"),
    (b"Prti", "xpr"),
    (b"Phli", "pal"),
    (b"Orkh", "otk"),
    (b"Kthi", "bh"),
    (b"Batk", "bbc"),
    (b"Brah", "pra"),
    (b"Mand", "myz"),
    (b"Cakm", "ccp"),
    (b"Merc", "xmr"),
    (b"Mero", "xmr"),
    (b"Plrd", "hmd"),
    (b"Shrd", "sa"),
    (b"Sora", "srb"),
    (b"Takr", "doi"),
    (b"Aghb", "lez"),
    (b"Bass", "bsq"),
    (b"Dupl", "fr"),
    (b"Elba", "sq"),
    (b"Gran", "sa"),
    (b"Hmng", "hnj"),
    (b"Khoj", "sd"),
    (b"Lina", "lab"),
    (b"Mahj", "hi"),
    (b"Mani", "xmn"),
    (b"Mend", "men"),
    (b"Modi", "mr"),
    (b"Mroo", "mru"),
    (b"Narb", "xna"),
    (b"Nbat", "arc"),
    (b"Palm", "arc"),
    (b"Pauc", "ctd"),
    (b"Perm", "kv"),
    (b"Phlp", "pal"),
    (b"Sidd", "sa"),
    (b"Sind", "sd"),
    (b"Tirh", "mai"),
    (b"Wara", "hoc"),
];

/// QFontEngine::scriptRequiresOpenType: Syriac to Sinhala in QChar's order, Khmer and N'Ko.
const OPENTYPE_SCRIPTS: [&[u8; 4]; 14] = [
    b"Syrc", b"Thaa", b"Deva", b"Beng", b"Guru", b"Gujr", b"Orya", b"Taml", b"Telu", b"Knda",
    b"Mlym", b"Sinh", b"Khmr", b"Nkoo",
];

fn opentype_script(script: hb::hb_script_t) -> bool {
    OPENTYPE_SCRIPTS
        .iter()
        .any(|t| u32::from_be_bytes(**t) == script)
}

/// The place of `script` in `SCRIPT_LANGUAGES`.
fn script_slot(script: hb::hb_script_t) -> Option<usize> {
    let tag = script.to_be_bytes();
    SCRIPT_LANGUAGES.iter().position(|(t, _)| **t == tag)
}

fn script_language(script: hb::hb_script_t) -> Option<&'static str> {
    let tag = script.to_be_bytes();
    SCRIPT_LANGUAGES
        .iter()
        .find(|(t, _)| **t == tag)
        .map(|&(_, lang)| lang)
}

/// The fontconfig configuration every lookup uses, loaded once and never changed afterwards.
struct FcCfg(*mut fc::FcConfig);
// SAFETY: fontconfig is thread-safe since 2.10.91 (2.11 RC1, "Make fontconfig thread-safe":
// atomic refcounts, locked caches and object tables), so one `FcConfig` may be shared by
// threads. Only read-only queries (`FcConfigSubstitute`, `FcFontMatch`, `FcFontSort`,
// `FcFontList`) receive it; nothing rebuilds, rescans or destroys it.
unsafe impl Send for FcCfg {}
// SAFETY: see `Send`: concurrent read-only queries on one `FcConfig` are safe.
unsafe impl Sync for FcCfg {}

fn fc_config() -> *mut fc::FcConfig {
    static CFG: std::sync::OnceLock<FcCfg> = std::sync::OnceLock::new();
    // SAFETY: `FcInitLoadConfigAndFonts` has no preconditions.
    CFG.get_or_init(|| FcCfg(unsafe { fc::FcInitLoadConfigAndFonts() }))
        .0
}

/// The first `obj` string of the pattern `p`.
///
/// # Safety
/// `p` must be a live pattern.
unsafe fn fc_string(p: *mut fc::FcPattern, obj: &std::ffi::CStr) -> Option<String> {
    let mut c = ptr::null_mut();
    // SAFETY: per the caller; `c` is an out pointer.
    if unsafe { fc::FcPatternGetString(p, obj.as_ptr(), 0, &mut c) } != fc::FcResultMatch {
        return None;
    }
    // SAFETY: on a match `c` is the pattern's NUL-terminated value, alive as long as `p`.
    let v = unsafe { std::ffi::CStr::from_ptr(c as *const _) };
    Some(v.to_string_lossy().into_owned())
}

/// The first `obj` integer of the pattern `p`.
///
/// # Safety
/// `p` must be a live pattern.
unsafe fn fc_int(p: *mut fc::FcPattern, obj: &std::ffi::CStr) -> Option<i32> {
    let mut c = 0;
    // SAFETY: per the caller; `c` is an out value.
    let r = unsafe { fc::FcPatternGetInteger(p, obj.as_ptr(), 0, &mut c) };
    (r == fc::FcResultMatch).then_some(c)
}

/// The patterns of the font set `set`.
///
/// # Safety
/// `set` must be a live font set that outlives `'a`.
unsafe fn fc_fonts<'a>(set: *const fc::FcFontSet) -> &'a [*mut fc::FcPattern] {
    // SAFETY: per the caller.
    let set = unsafe { &*set };
    // SAFETY: `fonts` holds `nfont` patterns.
    unsafe { c_slice(set.fonts, set.nfont as usize) }
}

/// Font file fontconfig picks for `family` (FcFontMatch with config + default substitution).
pub fn fc_match_file(family: &str) -> Option<String> {
    thread_local! {
        static MATCHED: std::cell::RefCell<std::collections::HashMap<String, Option<String>>> =
            Default::default();
    }
    if let Some(v) = MATCHED.with(|m| m.borrow().get(family).cloned()) {
        return v;
    }
    let v = fc_match_file_uncached(family);
    MATCHED.with(|m| m.borrow_mut().insert(family.to_string(), v.clone()));
    v
}

/// `fc_match_file` for a QFont weight. QFontDatabase lists every face of the matched family and
/// takes the style closest in weight (of two equally close ones the first it listed, see
/// `styled_face`), so a static family gives the weight's own file. Variable fonts list one file
/// for all instances and keep it.
pub fn fc_match_file_weight(family: &str, weight: i32) -> Option<String> {
    fc_match_face(family, weight, "", false).map(|f| f.0)
}

/// `fc_match_file_weight` with a QFont styleName: QFontDatabase's bestStyle takes the face whose
/// style name matches before comparing weights, and for an `italic` QFont among the family's
/// italic faces when it has any. Returns the file and the picked face's weight.
pub fn fc_match_face(
    family: &str,
    weight: i32,
    style: &str,
    italic: bool,
) -> Option<(String, i32)> {
    /// (family, weight, style, italic) to (file, weight).
    type Matched = std::collections::HashMap<(String, i32, String, bool), Option<(String, i32)>>;
    thread_local! {
        static MATCHED: std::cell::RefCell<Matched> = Default::default();
    }
    let key = (family.to_string(), weight, style.to_string(), italic);
    if let Some(v) = MATCHED.with(|m| m.borrow().get(&key).cloned()) {
        return v;
    }
    let base = fc_match_file(family);
    let v = base
        .as_deref()
        .and_then(|b| face_for_weight(b, weight, style, italic))
        .or(base.map(|b| (b, weight)));
    MATCHED.with(|m| m.borrow_mut().insert(key, v.clone()));
    v
}

/// fontconfig weight → OpenType/QFont weight (FcWeightToOpenType).
fn fc_weight_to_ot(w: i32) -> f64 {
    const MAP: [(f64, f64); 11] = [
        (0.0, 100.0),
        (40.0, 200.0),
        (50.0, 300.0),
        (55.0, 350.0),
        (75.0, 380.0),
        (80.0, 400.0),
        (100.0, 500.0),
        (180.0, 600.0),
        (200.0, 700.0),
        (205.0, 800.0),
        (210.0, 900.0),
    ];
    let w = (w as f64).clamp(0.0, 215.0);
    for p in MAP.windows(2) {
        let ((a, oa), (b, ob)) = (p[0], p[1]);
        if w <= b {
            return oa + (w - a) * (ob - oa) / (b - a);
        }
    }
    1000.0
}

/// fontconfig weight → QFont weight as QFontDatabase registers it (qfontconfigdatabase.cpp
/// weightFromFcWeight): whole numbers, without fontconfig's DemiLight and Book steps.
fn fc_weight_to_qt(w: i32) -> i32 {
    const MAP: [(i32, i32); 10] = [
        (0, 100),
        (40, 200),
        (50, 300),
        (80, 400),
        (100, 500),
        (180, 600),
        (200, 700),
        (205, 800),
        (210, 900),
        (215, 1000),
    ];
    if w <= MAP[0].0 {
        return MAP[0].1;
    }
    for p in MAP.windows(2) {
        let ((a, qa), (b, qb)) = (p[0], p[1]);
        if w <= b {
            return qa + (w - a) * (qb - qa) / (b - a);
        }
    }
    1000
}

struct FcFace {
    family: String,
    file: String,
    index: i32,
    style: Option<String>,
    weight: i32,
    slant: i32,
    width: i32,
    spacing: i32,
}

/// FcFontList of the faces whose `obj` (FC_FILE or FC_FAMILY) is `val`, in the order
/// QFontDatabase registers them. FcFontList returns the fonts in the order of a hash over the
/// listed properties, so this lists exactly what QFontconfigDatabase::populateFontDatabase does:
/// its properties, and no variable face itself (its named instances are listed).
fn fc_faces(obj: &std::ffi::CStr, val: &str) -> Vec<FcFace> {
    use fc::constants::{
        FC_CAPABILITY, FC_CHARSET, FC_COLOR, FC_FAMILY, FC_FAMILYLANG, FC_FILE, FC_FOUNDRY,
        FC_INDEX, FC_LANG, FC_PIXEL_SIZE, FC_SCALABLE, FC_SLANT, FC_SPACING, FC_STYLE, FC_VARIABLE,
        FC_WEIGHT, FC_WIDTH,
    };
    let cfg = fc_config();
    let Ok(v) = std::ffi::CString::new(val) else {
        return Vec::new();
    };
    // SAFETY: creates an empty pattern.
    let pat = unsafe { fc::FcPatternCreate() };
    // SAFETY: our pattern; fontconfig copies the NUL-terminated value.
    unsafe { fc::FcPatternAddString(pat, obj.as_ptr(), v.as_ptr() as *const u8) };
    // SAFETY: our pattern.
    unsafe { fc::FcPatternAddBool(pat, FC_VARIABLE.as_ptr(), 0) };
    // SAFETY: creates an empty object set.
    let os = unsafe { fc::FcObjectSetCreate() };
    for o in [
        FC_FAMILY,
        FC_STYLE,
        FC_WEIGHT,
        FC_SLANT,
        FC_SPACING,
        FC_FILE,
        FC_INDEX,
        FC_LANG,
        FC_CHARSET,
        FC_FOUNDRY,
        FC_SCALABLE,
        FC_PIXEL_SIZE,
        FC_WIDTH,
        FC_FAMILYLANG,
        FC_CAPABILITY,
        FC_COLOR,
    ] {
        // SAFETY: our object set; `o` is a static NUL-terminated name.
        unsafe { fc::FcObjectSetAdd(os, o.as_ptr()) };
    }
    // SAFETY: a read-only query on the shared config (see `FcCfg`).
    let set = unsafe { fc::FcFontList(cfg, pat, os) };
    // SAFETY: ours, destroyed once.
    unsafe { fc::FcObjectSetDestroy(os) };
    // SAFETY: ours, destroyed once.
    unsafe { fc::FcPatternDestroy(pat) };
    let mut out = Vec::new();
    if set.is_null() {
        return out;
    }
    // SAFETY: `set` is live until it is destroyed below.
    for &p in unsafe { fc_fonts(set) } {
        // SAFETY: `p` belongs to the live `set`.
        let s = |o: &std::ffi::CStr| unsafe { fc_string(p, o) };
        // SAFETY: as above.
        let n = |o: &std::ffi::CStr| unsafe { fc_int(p, o) };
        if let (Some(family), Some(file), Some(weight)) = (s(FC_FAMILY), s(FC_FILE), n(FC_WEIGHT)) {
            out.push(FcFace {
                family,
                file,
                index: n(FC_INDEX).unwrap_or(0),
                style: s(FC_STYLE),
                weight,
                slant: n(FC_SLANT).unwrap_or(0),
                width: n(FC_WIDTH).unwrap_or(100),
                spacing: n(FC_SPACING).unwrap_or(fc::constants::FC_PROPORTIONAL),
            });
        }
    }
    // SAFETY: ours, destroyed once; nothing borrowed from it is used afterwards.
    unsafe { fc::FcFontSetDestroy(set) };
    out
}

/// QFontDatabase's pick within `me`'s family (bestStyle): the face named `style`, else at `me`'s
/// slant and width the first listed face with the least weight distance, which Qt counts in whole
/// tens, so 450 is as far from Regular as from Medium and the one listed first wins. Returns the
/// face and its weight, which selects its named instance. An `italic` QFont takes the family's
/// italic faces for `me`'s slant, else its oblique ones, else stays with `me`'s (bestStyle
/// scores the style before the weight; the upright face is then slanted, `Font::oblique`).
fn styled_face(me: &FcFace, weight: i32, style: &str, italic: bool) -> Option<(FcFace, i32)> {
    let faces = fc_faces(fc::constants::FC_FAMILY, &me.family);
    let named = (!style.is_empty())
        .then(|| faces.iter().position(|f| f.style.as_deref() == Some(style)))
        .flatten();
    let slant = [
        fc::constants::FC_SLANT_ITALIC,
        fc::constants::FC_SLANT_OBLIQUE,
    ]
    .into_iter()
    .filter(|_| italic)
    .find(|&s| faces.iter().any(|f| f.slant == s && f.width == me.width))
    .unwrap_or(me.slant);
    let pick = named.or_else(|| {
        (0..faces.len())
            .filter(|&i| faces[i].slant == slant && faces[i].width == me.width)
            .min_by_key(|&i| ((weight - fc_weight_to_qt(faces[i].weight)) / 10).abs())
    })?;
    let f = faces.into_iter().nth(pick)?;
    let w = fc_weight_to_ot(f.weight).round() as i32;
    Some((f, w))
}

fn face_for_weight(base: &str, weight: i32, style: &str, italic: bool) -> Option<(String, i32)> {
    let me = fc_faces(fc::constants::FC_FILE, base).into_iter().next()?;
    styled_face(&me, weight, style, italic).map(|(f, w)| (f.file, w))
}

/// A fallback family's face for the QFont, while FcFontSort lists its default face.
/// QFontEngineMulti::loadEngine asks QFontDatabase for each fallback family with its own fontDef,
/// a copy of the primary engine's: the QFont's weight, and as styleName the primary face's
/// (`Font::style_name`), not the QFont's. So a fallback family takes its face of that name, and
/// only without one the face closest to the QFont's weight.
fn fallback_face(
    file: &str,
    index: i32,
    weight: i32,
    style: &str,
    italic: bool,
) -> Option<(String, i32, i32)> {
    type Key = (String, i32, i32, String, bool);
    type Matched = std::collections::HashMap<Key, Option<(String, i32, i32)>>;
    thread_local! {
        static MATCHED: std::cell::RefCell<Matched> = Default::default();
    }
    let key = (file.to_string(), index, weight, style.to_string(), italic);
    if let Some(v) = MATCHED.with(|m| m.borrow().get(&key).cloned()) {
        return v;
    }
    let v = fc_faces(fc::constants::FC_FILE, file)
        .into_iter()
        .find(|f| f.index == index)
        .and_then(|me| styled_face(&me, weight, style, italic))
        .map(|(f, w)| (f.file, f.index, w));
    MATCHED.with(|m| m.borrow_mut().insert(key, v.clone()));
    v
}

fn fc_match_file_uncached(family: &str) -> Option<String> {
    let cfg = fc_config();
    let name = std::ffi::CString::new(family.replace('-', "\\-").replace(':', "\\:")).ok()?;
    // SAFETY: a NUL-terminated name; the parsed pattern is ours.
    let pat = unsafe { fc::FcNameParse(name.as_ptr() as *const u8) };
    // SAFETY: our pattern, substituted from the shared config (see `FcCfg`), which it only reads.
    unsafe { fc::FcConfigSubstitute(cfg, pat, fc::FcMatchPattern) };
    // SAFETY: our pattern.
    unsafe { fc::FcDefaultSubstitute(pat) };
    let mut res = fc::FcResultMatch;
    // SAFETY: a read-only query on the shared config; `res` is an out value.
    let m = unsafe { fc::FcFontMatch(cfg, pat, &mut res) };
    // SAFETY: ours, destroyed once.
    unsafe { fc::FcPatternDestroy(pat) };
    if m.is_null() {
        return None;
    }
    // SAFETY: `m` is live until it is destroyed below.
    let out = unsafe { fc_string(m, fc::constants::FC_FILE) };
    // SAFETY: ours, destroyed once.
    unsafe { fc::FcPatternDestroy(m) };
    out
}

/// Qt's distance-field glyph cache of a face in a style, shared by every size opened of it
/// while any is alive (QSGRenderContext::distanceFieldGlyphCache: by face, style and weight).
fn df_atlas(
    path: &str,
    index: i32,
    named: Option<usize>,
    embolden: bool,
    oblique: bool,
    weight: f32,
    quality: i32,
) -> DfShared {
    type Key = (String, i32, Option<usize>, bool, bool, u32, i32);
    type Atlas = std::cell::RefCell<crate::dftext::DfAtlas>;
    thread_local! {
        static ATLASES: std::cell::RefCell<std::collections::HashMap<Key, std::rc::Weak<Atlas>>> =
            Default::default();
    }
    let key = (
        path.to_string(),
        index,
        named,
        embolden,
        oblique,
        weight.to_bits(),
        quality,
    );
    ATLASES.with(|m| {
        let mut m = m.borrow_mut();
        if let Some(a) = m.get(&key).and_then(|w| w.upgrade()) {
            return a;
        }
        m.retain(|_, w| w.strong_count() > 0);
        let a = std::rc::Rc::new(Atlas::new(crate::dftext::DfAtlas::of_quality(quality)));
        m.insert(key, std::rc::Rc::downgrade(&a));
        a
    })
}

/// Font file bytes, shared by every size/instance opened from the same file while any is alive.
fn font_file(path: &str) -> std::io::Result<std::rc::Rc<FontData>> {
    thread_local! {
        static FILES: std::cell::RefCell<std::collections::HashMap<String, std::rc::Weak<FontData>>> =
            Default::default();
    }
    if let Some(d) = FILES.with(|f| f.borrow().get(path).and_then(|w| w.upgrade())) {
        return Ok(d);
    }
    let d = std::rc::Rc::new(FontData::load(path)?);
    FILES.with(|f| {
        let mut f = f.borrow_mut();
        f.retain(|_, w| w.strong_count() > 0);
        f.insert(path.to_string(), std::rc::Rc::downgrade(&d));
    });
    Ok(d)
}

/// A font file's sfnt bytes: the file mapped read-only (clean shared page-cache pages, nothing
/// copied to the heap), or the decompressed sfnt for WOFF/WOFF2.
enum FontData {
    Mapped(memmap2::Mmap),
    Owned(Box<[u8]>),
}

impl FontData {
    fn load(path: &str) -> std::io::Result<FontData> {
        let file = std::fs::File::open(path)?;
        // Package managers replace font files by rename, so the mapped inode stays intact.
        // SAFETY: font files are never written in place (package managers replace them by rename), so
        // the mapping stays valid for its lifetime.
        if let Ok(map) = unsafe { memmap2::Mmap::map(&file) }
            && !(map.starts_with(b"wOFF") || map.starts_with(b"wOF2"))
        {
            return Ok(FontData::Mapped(map));
        }
        let mut data = Vec::new();
        std::io::Read::read_to_end(&mut &file, &mut data)?;
        Ok(FontData::Owned(sfnt_bytes(data).into_boxed_slice()))
    }
}

impl std::ops::Deref for FontData {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match self {
            FontData::Mapped(m) => m,
            FontData::Owned(b) => b,
        }
    }
}

/// WOFF/WOFF2 files as the plain sfnt FreeType decompresses them to. QFontEngineFT feeds HarfBuzz
/// FT_Load_Sfnt_Table data, so web fonts fontconfig lists shape like their OTF/TTF twins.
fn sfnt_bytes(data: Vec<u8>) -> Vec<u8> {
    if !(data.starts_with(b"wOFF") || data.starts_with(b"wOF2")) {
        return data;
    }
    let mut out = Vec::new();
    let mut face = ptr::null_mut();
    let opened = with_library(|lib| {
        // SAFETY: `data` outlives the face (closed below); `face` is an out pointer.
        unsafe { ft::FT_New_Memory_Face(lib, data.as_ptr(), data.len() as _, 0, &mut face) }
    });
    if opened != 0 {
        return data;
    }
    let mut len: ft::FT_ULong = 0;
    // SAFETY: a live face; a null buffer only asks for the length.
    if unsafe { ft::FT_Load_Sfnt_Table(face, 0, 0, ptr::null_mut(), &mut len) } == 0 {
        out.resize(len as usize, 0);
        // SAFETY: `out` has room for the `len` bytes FreeType writes.
        if unsafe { ft::FT_Load_Sfnt_Table(face, 0, 0, out.as_mut_ptr(), &mut len) } != 0 {
            out.clear();
        }
    }
    // SAFETY: the face opened above, closed once under the library lock.
    with_library(|_| unsafe { ft::FT_Done_Face(face) });
    if out.is_empty() { data } else { out }
}

/// fontconfig's FcValue holding a string. The bindings declare its union as `c_void`, which
/// cannot be passed by value.
#[repr(C)]
struct FcStringValue {
    kind: fc::FcType,
    u: FcValueUnion,
}

/// The members of FcValue's union that give it its size and alignment.
#[repr(C)]
union FcValueUnion {
    s: *const fc::FcChar8,
    _d: f64,
}

unsafe extern "C" {
    /// fontconfig's function of this name, with the value as C declares it.
    fn FcPatternAddWeak(
        p: *mut fc::FcPattern,
        object: *const std::ffi::c_char,
        value: FcStringValue,
        append: fc::FcBool,
    ) -> fc::FcBool;
}

/// QtFontFamily::fixedPitch: every face QFontDatabase registers sets it for its family
/// (qt_registerFont), from FC_SPACING, so the face listed last decides.
fn fc_fixed_pitch(family: &str) -> bool {
    fc_faces(fc::constants::FC_FAMILY, family)
        .last()
        .is_some_and(|f| f.spacing >= fc::constants::FC_MONO)
}

/// QFontEngineMulti::glyphIndex looks for a character in the engines below 256: the primary
/// font and the first 255 fallback families.
const MAX_FALLBACKS: usize = 255;

/// The fallback families of one family (QFontEngineMulti::ensureFallbackFamiliesQueried). Qt
/// keeps an engine per script, each with the families in the order fontconfig sorts them for
/// the script's language; here the families' faces are listed once, in the order for text of
/// no particular language, and every language has an order over that list.
struct FcFallbacks {
    family: String,
    /// The QFont is italic: fontconfig sorts the families for that slant.
    italic: bool,
    /// (file, face index) of every other family's first face in the sort order of scripts
    /// without a language of their own (Common, Latin, Han: the system's language).
    faces: Vec<(String, i32)>,
    /// The families of `faces`.
    names: Vec<String>,
    /// The font indices (position in `faces` + 1) a script language looks through, in order.
    orders: std::cell::RefCell<std::collections::HashMap<&'static str, std::rc::Rc<[usize]>>>,
}

impl FcFallbacks {
    fn order(&self, lang: &'static str) -> std::rc::Rc<[usize]> {
        if let Some(o) = self.orders.borrow().get(lang) {
            return o.clone();
        }
        let index: std::collections::HashMap<&str, usize> =
            self.names.iter().map(String::as_str).zip(1..).collect();
        let order = fc_sorted_families(&self.family, self.italic, Some(lang))
            .iter()
            .take(MAX_FALLBACKS)
            .filter_map(|(name, _)| index.get(name.as_str()).copied())
            .collect::<std::rc::Rc<[usize]>>();
        self.orders.borrow_mut().insert(lang, order.clone());
        order
    }
}

/// A family's fallback families (memoized: the sort costs milliseconds).
fn fc_fallbacks(family: &str, italic: bool) -> std::rc::Rc<FcFallbacks> {
    type Sorted = std::collections::HashMap<(String, bool), std::rc::Rc<FcFallbacks>>;
    thread_local! {
        static SORTED: std::cell::RefCell<Sorted> = Default::default();
    }
    let key = (family.to_string(), italic);
    if let Some(v) = SORTED.with(|s| s.borrow().get(&key).cloned()) {
        return v;
    }
    let (names, faces) = fc_sorted_families(family, italic, None).into_iter().unzip();
    let v = std::rc::Rc::new(FcFallbacks {
        family: family.to_string(),
        italic,
        faces,
        names,
        orders: Default::default(),
    });
    SORTED.with(|s| s.borrow_mut().insert(key, v.clone()));
    v
}

/// QFontconfigDatabase::fallbacksForFamily: the families after `family` in fontconfig's sort
/// order for text in `lang` (the system's language when the script has none), each by its
/// case-folded name with the (file, face index) of its first face. An `italic` QFont asks
/// for that slant, which puts the families that have it first.
fn fc_sorted_families(
    family: &str,
    italic: bool,
    lang: Option<&str>,
) -> Vec<(String, (String, i32))> {
    let cfg = fc_config();
    let mut out = vec![];
    let mut seen = std::collections::HashSet::from([family.to_lowercase()]);
    let name = std::ffi::CString::new(family.replace('-', "\\-").replace(':', "\\:")).unwrap();
    // SAFETY: a NUL-terminated name; the parsed pattern is ours.
    let pat = unsafe { fc::FcNameParse(name.as_ptr() as *const u8) };
    if let Some(lang) = lang {
        let lang = std::ffi::CString::new(lang).unwrap();
        // SAFETY: an empty language set of ours.
        let set = unsafe { fc::FcLangSetCreate() };
        // SAFETY: our set and a NUL-terminated language tag, which fontconfig copies.
        unsafe { fc::FcLangSetAdd(set, lang.as_ptr() as *const fc::FcChar8) };
        // SAFETY: our pattern and set; the pattern takes a copy of the set.
        unsafe { fc::FcPatternAddLangSet(pat, fc::constants::FC_LANG.as_ptr(), set) };
        // SAFETY: ours, destroyed once.
        unsafe { fc::FcLangSetDestroy(set) };
    }
    if italic {
        // SAFETY: our pattern and a NUL-terminated property name.
        unsafe {
            fc::FcPatternAddInteger(
                pat,
                fc::constants::FC_SLANT.as_ptr(),
                fc::constants::FC_SLANT_ITALIC,
            )
        };
    }
    // QFontEngineMulti::ensureFallbackFamiliesQueried asks for a fixed-pitch family's fallbacks
    // with the TypeWriter style hint, which QFontconfigDatabase::fallbacksForFamily adds to the
    // pattern as the weak family "monospace": monospace families then come first.
    if fc_fixed_pitch(family) {
        let value = FcStringValue {
            kind: fc::FcTypeString,
            u: FcValueUnion {
                s: c"monospace".as_ptr() as *const fc::FcChar8,
            },
        };
        // SAFETY: our pattern; fontconfig copies the NUL-terminated static string.
        unsafe { FcPatternAddWeak(pat, fc::constants::FC_FAMILY.as_ptr(), value, 1) };
    }
    // SAFETY: our pattern, substituted from the shared config (see `FcCfg`), which it only reads.
    unsafe { fc::FcConfigSubstitute(cfg, pat, fc::FcMatchPattern) };
    // SAFETY: our pattern.
    unsafe { fc::FcDefaultSubstitute(pat) };
    let mut res = fc::FcResultMatch;
    // SAFETY: a read-only query on the shared config; `res` is an out value, no charset wanted.
    // Untrimmed, as Qt asks: every family is listed, whatever the ones before it cover.
    let set = unsafe { fc::FcFontSort(cfg, pat, 0, ptr::null_mut(), &mut res) };
    if !set.is_null() {
        // SAFETY: `set` is live until it is destroyed below.
        for &p in unsafe { fc_fonts(set) } {
            // SAFETY: `p` belongs to the live `set`.
            let Some(file) = (unsafe { fc_string(p, fc::constants::FC_FILE) }) else {
                continue;
            };
            // SAFETY: as above.
            let Some(fam) = (unsafe { fc_string(p, fc::constants::FC_FAMILY) }) else {
                continue;
            };
            // SAFETY: as above.
            let idx = unsafe { fc_int(p, fc::constants::FC_INDEX) }.unwrap_or(0);
            let fam = fam.to_lowercase();
            if seen.insert(fam.clone()) {
                out.push((fam, (file, idx)));
            }
        }
        // SAFETY: ours, destroyed once; nothing borrowed from it is used afterwards.
        unsafe { fc::FcFontSetDestroy(set) };
    }
    // SAFETY: ours, destroyed once.
    unsafe { fc::FcPatternDestroy(pat) };
    out
}

#[cfg(test)]
mod fc_value_tests {
    use super::*;
    use fc::constants::{FC_FAMILY, FC_SIZE};

    /// What fontconfig writes for `obj` of `pat` (FcPatternGet), read as our `FcStringValue`;
    /// fontconfig must leave the words after it alone.
    fn get(pat: *mut fc::FcPattern, obj: &std::ffi::CStr) -> FcStringValue {
        const UNTOUCHED: u64 = 0xa5a5_a5a5_a5a5_a5a5;
        #[repr(C)]
        struct Slot {
            value: std::mem::MaybeUninit<FcStringValue>,
            after: [u64; 2],
        }
        let mut slot = Slot {
            value: std::mem::MaybeUninit::zeroed(),
            after: [UNTOUCHED; 2],
        };
        let out = slot.value.as_mut_ptr() as *mut fc::FcValue;
        // SAFETY: a live pattern and a NUL-terminated name; `out` has room for an FcValue as
        // long as ours is no smaller than fontconfig's, and `after` takes what a larger one
        // would write past it.
        let res = unsafe { fc::FcPatternGet(pat, obj.as_ptr(), 0, out) };
        assert_eq!(res, fc::FcResultMatch);
        assert_eq!(slot.after, [UNTOUCHED; 2], "fontconfig's FcValue is larger");
        // SAFETY: zeroed at first and then filled by fontconfig; every bit pattern is valid.
        unsafe { slot.value.assume_init() }
    }

    /// `FcStringValue` is declared here, not by the bindings: it must have the layout of
    /// fontconfig's FcValue, which `FcPatternAddWeak` takes by value.
    #[test]
    fn our_fc_value_is_fontconfigs() {
        // SAFETY: creates an empty pattern.
        let pat = unsafe { fc::FcPatternCreate() };
        // SAFETY: our pattern; fontconfig copies the NUL-terminated string.
        unsafe { fc::FcPatternAddString(pat, FC_FAMILY.as_ptr(), c"Strong".as_ptr() as *const u8) };
        // SAFETY: our pattern.
        unsafe { fc::FcPatternAddDouble(pat, FC_SIZE.as_ptr(), 12.5) };

        // A value fontconfig wrote reads right through our declaration: the type, and both the
        // pointer and the double members of the union.
        let family = get(pat, FC_FAMILY);
        assert_eq!(family.kind, fc::FcTypeString);
        // SAFETY: a string value's union holds the string.
        let s = unsafe { family.u.s };
        // SAFETY: the pattern's NUL-terminated string, alive as long as `pat`.
        let s = unsafe { std::ffi::CStr::from_ptr(s as *const _) };
        assert_eq!(s, c"Strong");
        let size = get(pat, FC_SIZE);
        assert_eq!(size.kind, fc::FcTypeDouble);
        // SAFETY: a double value's union holds the double.
        assert_eq!(unsafe { size.u._d }, 12.5);

        // And a value we pass by value arrives: the weak family is the pattern's second one.
        let value = FcStringValue {
            kind: fc::FcTypeString,
            u: FcValueUnion {
                s: c"monospace".as_ptr() as *const fc::FcChar8,
            },
        };
        // SAFETY: our pattern; fontconfig copies the NUL-terminated static string.
        let added = unsafe { FcPatternAddWeak(pat, FC_FAMILY.as_ptr(), value, 1) };
        assert_ne!(added, 0);
        let mut second = ptr::null_mut();
        // SAFETY: our pattern; `second` is an out pointer.
        let res = unsafe { fc::FcPatternGetString(pat, FC_FAMILY.as_ptr(), 1, &mut second) };
        assert_eq!(res, fc::FcResultMatch);
        // SAFETY: on a match the pattern's NUL-terminated value, alive as long as `pat`.
        let second = unsafe { std::ffi::CStr::from_ptr(second as *const _) };
        assert_eq!(second, c"monospace");
        // SAFETY: ours, destroyed once; nothing borrowed from it is used afterwards.
        unsafe { fc::FcPatternDestroy(pat) };
    }
}

#[cfg(test)]
mod shaped_cache_tests {
    use super::*;

    #[test]
    fn changing_texts_keep_lines_still_in_use() {
        let Some(path) = fc_match_file("sans-serif") else {
            testenv::unavailable("no font for the family sans-serif");
            return;
        };
        let fs = FontStack::from_path(&path, 15, 400.0, &[]).unwrap();
        let label = fs.shape("Battery");
        // A clock label shaped once per second never flushes the lines drawn every frame.
        for n in 0..3 * SHAPED_LINES.get() {
            fs.shape(&format!("{:02}:{:02}:{:02}", n / 3600, n / 60 % 60, n % 60));
            let cached = fs.shaped.borrow().peek("Battery").map(|l| l.advance);
            assert_eq!(cached, Some(label.advance), "flushed after {n} clock texts");
            fs.shape("Battery");
        }
        assert_eq!(fs.shaped.borrow().len(), SHAPED_LINES.get());
    }
}

#[cfg(test)]
mod df_tile_cache_tests {
    use super::*;

    #[test]
    fn rotating_icon_keeps_static_label_tiles() {
        let Some(path) = fc_match_file("sans-serif") else {
            testenv::unavailable("no font for the family sans-serif");
            return;
        };
        let fs = FontStack::from_path(&path, 15, 400.0, &[]).unwrap();
        let f = fs.font(0).unwrap();
        let run =
            |t: &str| -> Vec<_> { fs.shape(t).glyphs.iter().map(|g| (g.1, g.2, g.3)).collect() };
        let (label, icon) = (run("Wi-Fi"), run("@"));
        let draw = |glyphs: &[(u32, i64, i64)], turn| {
            let mut out = vec![];
            f.df_populate(&glyphs.iter().map(|g| g.0).collect::<Vec<_>>(), 0);
            let view = crate::dftext::DfView::plain(200.0, 100.0);
            f.df_tiles(
                glyphs,
                0,
                0.0,
                0.0,
                0.0,
                12.0,
                [0.0, 0.0, 0.0, 1.0],
                crate::dftext::DfLook::Subpixel,
                turn,
                view,
                &mut out,
            );
            out
        };
        // The window's projection times a turn by `a` about (20, 20).
        let turned = |a: f32| {
            let (s, c) = a.sin_cos();
            let (px, py) = (2.0 / 200.0, -2.0 / 100.0);
            crate::dftext::DfTurn {
                mvp: [
                    px * c,
                    py * s,
                    -px * s,
                    py * c,
                    px * (20.0 - 20.0 * c + 20.0 * s) - 1.0,
                    py * (20.0 - 20.0 * s - 20.0 * c) + 1.0,
                ],
                det: 1.0,
                inv0: [c / px, -s / px],
            }
        };
        // The icon's glyph enters the glyph cache first: a tile is of the cache as it is.
        draw(&icon, None);
        let first = draw(&label, None);
        assert!(!first.is_empty());
        // An animated rotation makes a new tile every frame; the label beside it stays cached.
        for n in 0..3 * DF_TILES.get() {
            draw(&icon, Some(turned(n as f32 * 0.001)));
            let again = draw(&label, None);
            for (x, y) in first.iter().zip(&again) {
                let kept = std::rc::Rc::ptr_eq(&x.0, &y.0);
                assert!(kept, "label re-rendered after {n} rotated frames");
            }
        }
        assert_eq!(f.df_tiles.borrow().len(), DF_TILES.get());
    }

    /// The tile past the limit pushes out the one used longest ago, not itself; that one,
    /// asked for again, is rendered anew with the pixels it had, so the limit costs time only.
    #[test]
    fn tile_past_the_limit_evicts_the_oldest_which_redraws_the_same() {
        let Some(path) = fc_match_file("sans-serif") else {
            testenv::unavailable("no font for the family sans-serif");
            return;
        };
        let fs = FontStack::from_path(&path, 15, 400.0, &[]).unwrap();
        let f = fs.font(0).unwrap();
        let glyph: Vec<_> = fs
            .shape("W")
            .glyphs
            .iter()
            .map(|g| (g.1, g.2, g.3))
            .collect();
        f.df_populate(&[glyph[0].0], 0);
        let draw = |x: f64| {
            let mut out = vec![];
            f.df_tiles(
                &glyph,
                0,
                0.0,
                0.0,
                x,
                12.0,
                [0.0, 0.0, 0.0, 1.0],
                crate::dftext::DfLook::Subpixel,
                None,
                crate::dftext::DfView::plain(200.0, 100.0),
                &mut out,
            );
            out
        };
        let at = |n: usize| 4.0 + n as f64 / 64.0;
        let same = |a: &[crate::dftext::DfTile], b: &[crate::dftext::DfTile]| {
            std::rc::Rc::ptr_eq(&a[0].0, &b[0].0)
        };
        let (first, second) = (draw(at(0)), draw(at(1)));
        assert_eq!((first.len(), second.len()), (1, 1));
        for n in 2..DF_TILES.get() {
            draw(at(n));
        }
        // The cache is full and holds the first tile; using it makes the second the oldest.
        assert_eq!(f.df_tiles.borrow().len(), DF_TILES.get());
        assert!(same(&first, &draw(at(0))));
        // The 513th: the cache keeps its size and holds the new tile and the first.
        let new = draw(at(DF_TILES.get()));
        assert_eq!(f.df_tiles.borrow().len(), DF_TILES.get());
        assert!(same(&new, &draw(at(DF_TILES.get()))));
        assert!(same(&first, &draw(at(0))));
        // The second is gone: it is rendered anew, at the same place with the same pixels.
        let again = draw(at(1));
        assert!(!same(&second, &again), "the oldest tile was not evicted");
        assert_eq!((again[0].1, again[0].2), (second[0].1, second[0].2));
        assert_eq!((again[0].0.w, again[0].0.h), (second[0].0.w, second[0].0.h));
        assert_eq!(
            again[0].0.px, second[0].0.px,
            "evicted tile redrawn differently"
        );
        assert_eq!(f.df_tiles.borrow().len(), DF_TILES.get());
    }

    /// A line far longer than its view, plain and turned: the tiles are those of the glyphs
    /// that reach a pixel of the view, as each is rendered alone; the rest of the line is
    /// neither rendered nor kept.
    #[test]
    fn glyphs_outside_the_view_give_no_tiles() {
        use crate::dftext::{DfLook, DfPlace, DfTurn, DfView};
        let Some(path) = fc_match_file("sans-serif") else {
            testenv::unavailable("no font for the family sans-serif");
            return;
        };
        let (vw, vh) = (200.0f32, 100.0f32);
        let view = DfView::plain(vw, vh);
        // ortho(0, vw, vh, 0) times a turn by 20 degrees about (30, 10).
        let (s, c) = 20.0f32.to_radians().sin_cos();
        let turned = DfTurn {
            mvp: [
                2.0 * c / vw,
                -2.0 * s / vh,
                -2.0 * s / vw,
                -2.0 * c / vh,
                2.0 * 30.0 / vw - 1.0,
                1.0 - 2.0 * 10.0 / vh,
            ],
            det: 1.0,
            inv0: [c * vw / 2.0, -s * vw / 2.0],
        };
        let (color, look) = ([0.0, 0.0, 0.0, 1.0], DfLook::Subpixel);
        for turn in [None, Some(turned)] {
            let fs = FontStack::from_path(&path, 15, 400.0, &[]).unwrap();
            let f = fs.font(0).unwrap();
            let line = fs.shape(&"Wij".repeat(1000));
            let got = fs.line_df_tiles(&line, 0, 3.0, 4.0, 0.0, 0.0, color, look, turn, view);
            let df = f.df_of(0);
            let df = df.borrow();
            let baseline = line.ascent as f64 / 64.0;
            let want: Vec<_> = line
                .glyphs
                .iter()
                .filter_map(|g| {
                    let place = DfPlace {
                        px: f.pixel_size as f64,
                        pen: [g.2 as f64 / 64.0, baseline + g.3 as f64 / 64.0],
                        shift: [3.0, 4.0],
                        turn,
                        view,
                        color,
                        look,
                    };
                    df.render(g.1, &place)
                })
                .filter(|(t, x, y)| {
                    *x < vw as i64 && x + t.w as i64 > 0 && *y < vh as i64 && y + t.h as i64 > 0
                })
                .collect();
            assert!(
                (10..100).contains(&want.len()),
                "{} of {} glyphs in the view",
                want.len(),
                line.glyphs.len()
            );
            assert_eq!(got.len(), want.len(), "turn {turn:?}");
            for (a, b) in got.iter().zip(&want) {
                assert_eq!((a.1, a.2), (b.1, b.2), "turn {turn:?}");
                assert_eq!(a.0.px, b.0.px, "turn {turn:?}");
            }
            assert_eq!(f.df_tiles.borrow().len(), got.len(), "turn {turn:?}");
        }
    }

    /// A list of QtRendering text scrolled by part of a pixel a frame: every glyph is at a
    /// new place every frame, so no tile is reused, and the cache stays at its limit.
    #[test]
    fn scrolled_list_keeps_tile_cache_bounded() {
        let Some(path) = fc_match_file("sans-serif") else {
            testenv::unavailable("no font for the family sans-serif");
            return;
        };
        let fs = FontStack::from_path(&path, 15, 400.0, &[]).unwrap();
        let f = fs.font(0).unwrap();
        let line = fs.shape("Battery 87% charging, 1 h 12 min");
        let rss = || {
            let statm = std::fs::read_to_string("/proc/self/statm").unwrap_or_default();
            statm
                .split(' ')
                .nth(1)
                .and_then(|p| p.parse::<usize>().ok())
                .unwrap_or(0)
                * 4096
        };
        let frame = |n: usize| {
            for row in 0..20 {
                let y = row as f64 * 24.0 - n as f64 * 0.37;
                let look = crate::dftext::DfLook::Subpixel;
                let color = [0.0, 0.0, 0.0, 1.0];
                fs.line_df_tiles(
                    &line,
                    0,
                    8.0,
                    y,
                    0.0,
                    0.0,
                    color,
                    look,
                    None,
                    crate::dftext::DfView::plain(400.0, 480.0),
                );
            }
        };
        let held = || -> usize {
            let tiles = f.df_tiles.borrow();
            tiles
                .iter()
                .flat_map(|t| t.1)
                .map(|t| t.0.px.len() * 4)
                .sum()
        };
        for n in 0..50 {
            frame(n);
        }
        let (before, bytes, start) = (rss(), held(), std::time::Instant::now());
        for n in 50..350 {
            frame(n);
        }
        let per_frame = start.elapsed() / 300;
        let grown = rss().saturating_sub(before);
        println!(
            "tiles {} of {}, {} KiB held, {per_frame:?} a frame of {} glyphs, rss grew {} KiB",
            f.df_tiles.borrow().len(),
            DF_TILES.get(),
            held() / 1024,
            20 * line.glyphs.len(),
            grown / 1024
        );
        assert_eq!(f.df_tiles.borrow().len(), DF_TILES.get());
        // 300 more frames of 640 new tiles each leave what is held where it was.
        assert!(
            held() < 2 * bytes && grown < 8 << 20,
            "{bytes} -> {}, rss +{grown}",
            held()
        );
    }
}

#[cfg(test)]
mod bitmap_glyph_tests {
    use super::*;

    /// One font as BDF, as PCF (rows padded to 4 bytes) and as an OpenType file with only a
    /// bitmap strike: tests/fixtures/bitmap/make.py.
    const FILES: [&str; 3] = [
        "impulse-bitmap-test.bdf",
        "impulse-bitmap-test.pcf",
        "impulse-bitmap-test.otb",
    ];

    fn fixture(file: &str) -> String {
        format!(
            "{}/tests/fixtures/bitmap/{file}",
            env!("CARGO_MANIFEST_DIR")
        )
    }

    /// Per glyph of the BDF file: its code point and its set pixels, (x right of the origin,
    /// y above the baseline).
    fn bdf_glyphs() -> Vec<(u32, Vec<(i64, i64)>)> {
        let text = std::fs::read_to_string(fixture(FILES[0])).unwrap();
        let mut lines = text.lines();
        let mut out = Vec::new();
        let (mut code, mut bbx) = (0, [0i64; 4]);
        while let Some(line) = lines.next() {
            let mut words = line.split(' ');
            match words.next() {
                Some("ENCODING") => code = words.next().unwrap().parse().unwrap(),
                Some("BBX") => bbx = [(); 4].map(|_| words.next().unwrap().parse().unwrap()),
                Some("BITMAP") => {
                    let [w, h, left, bottom] = bbx;
                    let mut set = Vec::new();
                    for (row, hex) in (0..h).zip(lines.by_ref()) {
                        let bits = u32::from_str_radix(hex, 16).unwrap();
                        let first = hex.len() as i64 * 4 - 1;
                        set.extend(
                            (0..w)
                                .filter(|x| bits >> (first - x) & 1 == 1)
                                .map(|x| (left + x, bottom + h - 1 - row)),
                        );
                    }
                    out.push((code, set));
                }
                _ => {}
            }
        }
        out
    }

    /// A bitmap font's glyph is 1 bit per pixel, a row of 13 pixels 2 bytes; read as a byte per
    /// pixel it was other rows' bits, and the last rows were memory past FreeType's buffer.
    #[test]
    fn bitmap_font_glyphs_are_the_bits_of_the_file() {
        let glyphs = bdf_glyphs();
        assert_eq!(glyphs.len(), 7);
        for file in FILES {
            let font = Font::open(&fixture(file), 16, 400.0, &[]).unwrap();
            for (code, set) in &glyphs {
                // SAFETY: a live face.
                let gid = unsafe { ft::FT_Get_Char_Index(font.face, *code as _) };
                assert_ne!(gid, 0, "{file} U+{code:04X}");
                let g = font.glyph(gid, 0).unwrap();
                assert_eq!(g.buf.len(), g.width * g.rows, "{file} U+{code:04X}");
                assert!(
                    g.buf.iter().all(|&v| v == 0x00 || v == 0xff),
                    "{file} U+{code:04X}"
                );
                let ours: Vec<(i64, i64)> = (0..g.buf.len())
                    .filter(|&i| g.buf[i] == 0xff)
                    .map(|i| {
                        (
                            g.left + (i % g.width) as i64,
                            g.top - 1 - (i / g.width) as i64,
                        )
                    })
                    .collect();
                assert_eq!(&ours, set, "{file} U+{code:04X}");
            }
        }
    }

    /// The bitmap SVG text takes (`glyph_xf`, under a painter's transform or none) is the same
    /// bits. Read as a byte per pixel, a row of `m` (19 pixels in 3 bytes, 4 in the PCF file)
    /// ran into the next rows, and the last rows ended past FreeType's buffer: the length of
    /// that read is checked here against the buffer's, `rows` rows of `pitch` bytes.
    #[test]
    fn bitmap_font_glyphs_under_a_transform_are_the_bits_of_the_file() {
        let glyphs = bdf_glyphs();
        let mut past_the_buffer = 0;
        for file in FILES {
            let font = Font::open(&fixture(file), 16, 400.0, &[]).unwrap();
            for (code, set) in &glyphs {
                // SAFETY: a live face.
                let gid = unsafe { ft::FT_Get_Char_Index(font.face, *code as _) };
                for m in [[0x10000, 0, 0, 0x10000], [0x20000, 0, 0, 0x20000]] {
                    let what = format!("{file} U+{code:04X} {m:x?}");
                    let g = font.glyph_xf(gid, 0, m).unwrap();
                    // SAFETY: the slot `glyph_xf` loaded, live until the face loads another.
                    let slot = unsafe { (*font.face).glyph };
                    // SAFETY: as above.
                    let bm = unsafe { &(*slot).bitmap };
                    assert_eq!(bm.pixel_mode as u32, ft::FT_PIXEL_MODE_MONO, "{what}");
                    let (rows, pitch) = (bm.rows as usize, bm.pitch.unsigned_abs() as usize);
                    assert_eq!((g.width, g.rows), (bm.width as usize, rows), "{what}");
                    // A byte per pixel: the last row would end here, of `rows * pitch` bytes.
                    let byte_per_pixel = rows.saturating_sub(1) * pitch + g.width;
                    past_the_buffer += (rows != 0 && byte_per_pixel > rows * pitch) as usize;
                    assert_eq!(g.buf.len(), g.width * g.rows, "{what}");
                    let ours: Vec<(i64, i64)> = (0..g.buf.len())
                        .filter(|&i| g.buf[i] != 0x00)
                        .map(|i| {
                            (
                                g.left + (i % g.width) as i64,
                                g.top - 1 - (i / g.width) as i64,
                            )
                        })
                        .collect();
                    assert_eq!(&ours, set, "{what}");
                    assert!(g.buf.iter().all(|&v| v == 0x00 || v == 0xff), "{what}");
                }
            }
        }
        // The fixture has glyphs whose rows are wider in pixels than in bytes.
        assert_ne!(past_the_buffer, 0);
    }

    /// No code of this crate but `Rows::of` reads the buffer of an `FT_Bitmap`: a second reader
    /// (`glyph_xf` was one) is a second place to get the pixel mode and the row length wrong.
    /// Counted in the source: every `.buffer` outside test modules and comments.
    #[test]
    fn one_place_reads_a_bitmaps_buffer() {
        fn sources(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            for e in std::fs::read_dir(dir).unwrap() {
                let p = e.unwrap().path();
                if p.is_dir() {
                    sources(&p, out);
                } else if p.extension().is_some_and(|e| e == "rs") {
                    out.push(p);
                }
            }
        }
        let mut files = Vec::new();
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        sources(&src, &mut files);
        assert!(files.len() > 10);
        let mut readers = Vec::new();
        for file in files {
            let text = std::fs::read_to_string(&file).unwrap();
            let mut in_fn = String::new();
            for (n, line) in text.lines().enumerate() {
                let code = line.split("//").next().unwrap();
                // Test modules are at the end of a file.
                if code.starts_with("#[cfg(test)]") {
                    break;
                }
                if let Some(at) = code.find("fn ") {
                    let name = &code[at + 3..];
                    in_fn = name[..name.find(['(', '<']).unwrap_or(name.len())].to_string();
                }
                // A field `buffer` of anything: no other struct of this crate has one.
                if code.contains(".buffer") {
                    let file = file.strip_prefix(&src).unwrap().display();
                    readers.push(format!("{file}:{} in {in_fn}: {}", n + 1, code.trim()));
                }
            }
        }
        assert_eq!(readers.len(), 1, "{readers:#?}");
        assert!(
            readers[0].starts_with("text.rs:") && readers[0].contains(" in of: "),
            "{readers:#?}"
        );
    }

    /// `rows` rows of `pitch` bytes, numbered from 1, behind an `FT_Bitmap` of `mode`.
    fn bitmap(mode: u32, width: i32, rows: i32, pitch: i32, bytes: &mut [u8]) -> ft::FT_Bitmap {
        assert_eq!(bytes.len(), (rows * pitch) as usize);
        ft::FT_Bitmap {
            rows,
            width,
            pitch,
            buffer: bytes.as_mut_ptr(),
            num_grays: 256,
            pixel_mode: mode as _,
            palette_mode: 0,
            palette: ptr::null_mut(),
        }
    }

    #[test]
    fn coverage_per_pixel_mode() {
        let numbered = |n: usize| (1..=n as u8).collect::<Vec<u8>>();
        // 8-bit grey: `width` bytes of each row, the padding up to `pitch` left out.
        let mut bytes = numbered(8);
        let bm = bitmap(ft::FT_PIXEL_MODE_GRAY, 3, 2, 4, &mut bytes);
        // SAFETY: `bm` is 2 rows of 4 bytes in `bytes`.
        assert_eq!(unsafe { coverage(&bm) }, Some(vec![1, 2, 3, 5, 6, 7]));
        // 1 bit: the most significant bit first, 0xff or 0x00 per pixel; 9 pixels are 2 bytes
        // of the 3 a row has here.
        let mut bytes = [0xa0, 0x80, 0xff, 0x01, 0x80, 0xff];
        let bm = bitmap(ft::FT_PIXEL_MODE_MONO, 9, 2, 3, &mut bytes);
        let (o, x) = (0x00, 0xff);
        // SAFETY: `bm` is 2 rows of 3 bytes in `bytes`.
        let got = unsafe { coverage(&bm) };
        assert_eq!(
            got,
            Some(vec![x, o, x, o, o, o, o, o, x, o, o, o, o, o, o, o, x, x])
        );
        // A negative pitch: `buffer` is the top row, which is the last one in memory.
        let mut bytes = numbered(6);
        let mut bm = bitmap(ft::FT_PIXEL_MODE_GRAY, 2, 3, 2, &mut bytes);
        (bm.buffer, bm.pitch) = (bm.buffer.wrapping_add(4), -2);
        // SAFETY: `bm` is 3 rows of 2 bytes in `bytes`, the first one last.
        assert_eq!(unsafe { coverage(&bm) }, Some(vec![5, 6, 3, 4, 1, 2]));
        // An empty glyph (a space) has no buffer.
        let bm = bitmap(ft::FT_PIXEL_MODE_MONO, 0, 0, 0, &mut []);
        // SAFETY: no rows to read.
        assert_eq!(unsafe { coverage(&bm) }, Some(vec![]));
        // The pixel modes Qt does not draw as coverage: nothing is read (the buffer is 1 row of
        // 1 byte, less than any of them needs for 8 pixels).
        for mode in [
            ft::FT_PIXEL_MODE_NONE,
            ft::FT_PIXEL_MODE_GRAY2,
            ft::FT_PIXEL_MODE_GRAY4,
            ft::FT_PIXEL_MODE_LCD,
            ft::FT_PIXEL_MODE_LCD_V,
            ft::FT_PIXEL_MODE_BGRA,
            ft::FT_PIXEL_MODE_MAX,
        ] {
            let bm = bitmap(mode, 8, 1, 1, &mut [0xff]);
            // SAFETY: `bm` is 1 row of 1 byte.
            assert_eq!(unsafe { coverage(&bm) }, None, "pixel mode {mode}");
        }
        // A pitch shorter than a row of `width` pixels.
        let bm = bitmap(ft::FT_PIXEL_MODE_GRAY, 8, 1, 7, &mut numbered(7));
        // SAFETY: `bm` is 1 row of 7 bytes.
        assert_eq!(unsafe { coverage(&bm) }, None);
        let bm = bitmap(ft::FT_PIXEL_MODE_MONO, 9, 1, 1, &mut [0xff]);
        // SAFETY: `bm` is 1 row of 1 byte.
        assert_eq!(unsafe { coverage(&bm) }, None);
        // Rows with no buffer.
        let mut bm = bitmap(ft::FT_PIXEL_MODE_GRAY, 2, 1, 2, &mut [1, 2]);
        bm.buffer = ptr::null_mut();
        // SAFETY: nothing is read through a null buffer.
        assert_eq!(unsafe { coverage(&bm) }, None);
    }

    /// A colour bitmap's rows are 4 bytes a pixel, checked against `pitch` as the others.
    #[test]
    fn bgra_rows() {
        let mut bytes: Vec<u8> = (1..=20).collect();
        let bm = bitmap(ft::FT_PIXEL_MODE_BGRA, 2, 2, 10, &mut bytes);
        // SAFETY: `bm` is 2 rows of 10 bytes in `bytes`.
        let rows = unsafe { Rows::of(&bm) }.unwrap();
        assert_eq!(rows.pixels, Pixels::Bgra);
        let got: Vec<&[u8]> = rows.iter().collect();
        assert_eq!(
            got,
            [
                &[1, 2, 3, 4, 5, 6, 7, 8][..],
                &[11, 12, 13, 14, 15, 16, 17, 18]
            ]
        );
        let mut bm = bitmap(ft::FT_PIXEL_MODE_BGRA, 2, 2, 10, &mut bytes);
        (bm.buffer, bm.pitch) = (bm.buffer.wrapping_add(10), -10);
        // SAFETY: `bm` is 2 rows of 10 bytes in `bytes`, the first one last.
        let rows = unsafe { Rows::of(&bm) }.unwrap();
        assert_eq!(
            rows.iter().next(),
            Some(&[11, 12, 13, 14, 15, 16, 17, 18][..])
        );
        // 2 pixels are 8 bytes: a pitch of 7 is not a row of them.
        let bm = bitmap(ft::FT_PIXEL_MODE_BGRA, 2, 1, 7, &mut bytes[..7]);
        // SAFETY: `bm` is 1 row of 7 bytes.
        assert!(unsafe { Rows::of(&bm) }.is_none());
    }
}

#[cfg(test)]
mod emoji_tests {
    use super::*;

    #[test]
    fn emoji_uses_colour_font() {
        let Some(path) = fc_match_file("sans-serif") else {
            testenv::unavailable("no font for the family sans-serif");
            return;
        };
        if fc_match_file("emoji").is_none() {
            testenv::unavailable("no font for the family emoji");
            return;
        }
        let fs = FontStack::from_path(&path, 15, 400.0, &[]).unwrap();
        let line = fs.shape("a\u{1F600}");
        let f = fs.font(line.glyphs[1].0).unwrap();
        assert!(f.ascent > 0 && f.ascent < 64 * 40, "ascent {}", f.ascent);
        assert!(f.color_scale > 0.0);
        let q = fs.line_quads(&line, 0.0, 0.0, 0.0, 0.0);
        let g = &q.last().unwrap().0;
        assert!(g.color && g.width > 10 && g.rows > 10);
        let l2 = fs.shape("\u{1F923}\u{1F923}");
        // Qt: two Noto Color Emoji advances at 15px, 136 * (15/128 as 26.6) each.
        assert_eq!(fs.natural_width(&l2), 2 * 136 * 7);
    }
}

#[cfg(test)]
mod bidi_data_tests {
    use super::*;
    use unicode_bidi::BidiDataSource;

    /// tests/qt_bidi/qt.txt: what Qt 6.12 holds (tests/qt_bidi/probe.cpp).
    const QT: &str = include_str!("../tests/qt_bidi/qt.txt");

    fn rows(kind: &str) -> impl Iterator<Item = Vec<&'static str>> {
        QT.lines()
            .map(|l| l.split(' ').collect::<Vec<_>>())
            .filter(move |w| w[0] == kind)
    }

    fn point(hex: &str) -> u32 {
        u32::from_str_radix(hex, 16).unwrap()
    }

    /// Every code point has the class QChar::direction gives it, the ones with no character
    /// too; the Unicode Character Database alone differs from Qt in those.
    #[test]
    fn every_code_point_has_qt_s_bidi_class() {
        let (mut seen, mut differ, mut ucd_differs) = (0, Vec::new(), 0);
        for w in rows("class") {
            let (from, to) = w[1].split_once("..").unwrap();
            for c in (point(from)..=point(to)).filter_map(char::from_u32) {
                seen += 1;
                if format!("{:?}", bidi_class(c)) != w[2] {
                    differ.push(format!(
                        "U+{:04X} {:?} Qt {}",
                        u32::from(c),
                        bidi_class(c),
                        w[2]
                    ));
                }
                if format!("{:?}", ucd_bidi_class(c)) != w[2] {
                    ucd_differs += 1;
                    assert_eq!(
                        (format!("{:?}", ucd_bidi_class(c)).as_str(), w[2]),
                        ("AL", "R")
                    );
                }
            }
        }
        assert_eq!(seen, 0x110000 - 0x800);
        assert!(
            differ.is_empty(),
            "{} differ: {:?}",
            differ.len(),
            &differ[..differ.len().min(20)]
        );
        assert_eq!(ucd_differs, 120);
    }

    /// Every code point is of the script QChar::script gives it: Qt numbers its scripts and
    /// HarfBuzz names them, so each number of Qt's goes with one name and each name with one
    /// number; the three scripts the split treats alike are pinned by name.
    #[test]
    fn every_code_point_has_qt_s_script() {
        let (mut seen, mut names, mut numbers) = (
            0,
            std::collections::BTreeMap::new(),
            std::collections::BTreeMap::new(),
        );
        let mut differ = vec![];
        for w in rows("script") {
            let (from, to) = w[1].split_once("..").unwrap();
            let qt: u32 = w[2].parse().unwrap();
            for c in (point(from)..=point(to)).filter_map(char::from_u32) {
                seen += 1;
                let ours = crate::hb_unicode::script(c);
                if *names.entry(qt).or_insert(ours) != ours
                    || *numbers.entry(ours).or_insert(qt) != qt
                {
                    differ.push(format!(
                        "U+{:04X} {:?} Qt {qt}",
                        u32::from(c),
                        String::from_utf8_lossy(&ours.to_be_bytes())
                    ));
                }
            }
        }
        assert_eq!(seen, 0x110000 - 0x800);
        assert!(
            differ.is_empty(),
            "{} differ: {:?}",
            differ.len(),
            &differ[..differ.len().min(30)]
        );
        assert_eq!(
            [names[&0], names[&1], names[&2]],
            [
                hb::HB_SCRIPT_UNKNOWN,
                hb::HB_SCRIPT_INHERITED,
                hb::HB_SCRIPT_COMMON
            ]
        );
    }

    /// The brackets rule N0 pairs are the ones QTextEngine pairs (resolveN0): an opening or
    /// closing punctuation mark of class ON with a mirrored character.
    #[test]
    fn paired_brackets_are_qt_s() {
        let qt: std::collections::BTreeMap<u32, (u32, bool)> = rows("open")
            .map(|w| (point(w[1]), (point(w[2]), true)))
            .chain(rows("close").map(|w| (point(w[1]), (point(w[2]), false))))
            .collect();
        assert_eq!(qt.len(), 128);
        let ours: std::collections::BTreeMap<u32, (u32, bool)> = (0..0x110000)
            .filter_map(char::from_u32)
            .filter_map(|c| bidi_bracket(c).map(|(o, open)| (u32::from(c), (u32::from(o), open))))
            .collect();
        assert_eq!(ours, qt);
        for (&c, &(other, open)) in &qt {
            let c = char::from_u32(c).unwrap();
            let m = BidiData.bidi_matched_opening_bracket(c).unwrap();
            let opening = if open { u32::from(c) } else { other };
            // U+2329 and U+232A pair with U+3008 and U+3009, their canonical equivalents.
            let opening = if opening == 0x2329 { 0x3008 } else { opening };
            assert_eq!((u32::from(m.opening), m.is_open), (opening, open));
        }
    }
}

#[cfg(test)]
mod webfont_tests {
    use super::*;

    fn webfonts(dir: &std::path::Path, out: &mut Vec<String>) {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                webfonts(&p, out);
            } else if p.extension().is_some_and(|x| x == "woff" || x == "woff2") {
                out.push(p.to_string_lossy().into_owned());
            }
        }
    }

    #[test]
    fn sfnt_files_are_mapped_not_copied() {
        let path = fc_match_file("DejaVu Sans").unwrap();
        let data = FontData::load(&path).unwrap();
        assert!(matches!(data, FontData::Mapped(_)), "{path}");
        assert_eq!(&*data, &std::fs::read(&path).unwrap()[..]);
        let mut files = vec![];
        webfonts(std::path::Path::new("/usr/share/fonts"), &mut files);
        for f in files {
            let data = FontData::load(&f).unwrap();
            assert!(matches!(data, FontData::Owned(_)), "{f}");
            assert!(!data.starts_with(b"wOF"), "{f}: not decompressed");
        }
    }

    /// A face of a TrueType collection gets the same hhea/OS/2 height metrics as the same face
    /// in a plain sfnt file (QFontEngineFT reads tables per face, the `ttcf` header aside).
    #[test]
    fn ttc_face_keeps_sfnt_height_metrics() {
        let path = fc_match_file("DejaVu Sans").unwrap();
        let ttf = std::fs::read(&path).unwrap();
        // TTC 1.0 header for one font (ttcTag, version, numFonts, tableDirectoryOffsets[0]),
        // then the font with its table offsets moved past the header.
        const HDR: u32 = 16;
        let mut ttc = b"ttcf\0\x01\0\0\0\0\0\x01".to_vec();
        ttc.extend(HDR.to_be_bytes());
        let mut font = ttf.clone();
        let n = u16::from_be_bytes([ttf[4], ttf[5]]) as usize;
        for i in 0..n {
            let o = 12 + i * 16 + 8;
            let off = u32::from_be_bytes(ttf[o..o + 4].try_into().unwrap()) + HDR;
            font[o..o + 4].copy_from_slice(&off.to_be_bytes());
        }
        ttc.extend(font);
        let dir = std::env::temp_dir().join(format!("qsg-ttc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ttc_path = dir.join("DejaVuSans.ttc");
        std::fs::write(&ttc_path, &ttc).unwrap();
        for px in [11, 13, 17] {
            let a = Font::open(&path, px, 400.0, &[]).unwrap();
            let b = Font::open(ttc_path.to_str().unwrap(), px, 400.0, &[]).unwrap();
            assert_eq!(
                (b.ascent, b.descent, b.leading),
                (a.ascent, a.descent, a.leading),
                "{px}px"
            );
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn woff_faces_shape_digits() {
        let mut files = vec![];
        webfonts(std::path::Path::new("/usr/share/fonts"), &mut files);
        for f in files {
            let font = Font::open(&f, 40, 400.0, &[]).unwrap();
            let (glyphs, adv) = font.shape("0123456789");
            assert!(glyphs.iter().all(|g| g.0 != 0), "{f}: notdef glyphs");
            assert!(adv > 0, "{f}");
        }
    }
}
