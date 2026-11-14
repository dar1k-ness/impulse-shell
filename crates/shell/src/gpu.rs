//! OpenGL ES 2 renderer over EGL on Wayland surfaces (default); `wl_shm` raster stays as fallback.
//! Rectangles go to the GPU as Qt's smooth-color triangles, text as A8 glyph-atlas quads; items
//! without a GPU path yet are rasterized into small textures.

use crate::region::{self, Region};
use glow::HasContext;
use khronos_egl as egl;
use qml::valuetypes::Mat4;
use qsg::{
    raster::{Canvas, Place},
    rect::Geometry,
    text::{GlyphQuad, GlyphQuadF},
};
use std::{collections::HashMap, ffi::c_void, rc::Rc};
use wayland_client::{Proxy, protocol::wl_surface::WlSurface};

type Egl = egl::DynamicInstance<egl::EGL1_4>;

/// Largest glyph atlas side; atlases start at `ATLAS_MIN` and double on demand.
const ATLAS: i32 = 2048;
const ATLAS_MIN: i32 = 256;

/// The vertices (x y r g b a each) of one triangle draw call, as runs shared with the display
/// lists that recorded them: recording or replaying a subtree shares its runs, and only the
/// last run, while no recording holds it, grows in place.
type TrisRuns = Vec<Rc<Vec<f32>>>;

/// Fragment modes of the texture program.
const PLAIN: f32 = 0.0;
const MASK: f32 = 1.0;
const OVERLAY: f32 = 2.0;
/// Qt5Compat OpacityMask: texture(source) * texture(maskSource).a (k.x = invert).
const MASKED: f32 = 3.0;
/// Qt5Compat Desaturate (desaturate.frag): mix(rgb, (r + g + b) / 3, k.x).
const DESAT: f32 = 4.0;
/// PLAIN drawn with blending off: the texels replace what is under them, alpha and all
/// (`Frame::image_unblended`). The program sees PLAIN.
const UNBLENDED: f32 = 5.0;

#[derive(Clone)]
enum Cmd {
    Tris(TrisRuns),
    Tex {
        src: Rc<Canvas>,
        rect: [f32; 4],
        uv: [f32; 4],
        k: [f32; 4],
        op: f32,
        mode: f32,
        smooth: bool,
        /// `Image.mipmap`: shrunk, the texture is read through its chain of levels.
        mipmap: bool,
    },
    Glyphs([f32; 4], Rc<[GlyphQuad]>),
    /// Glyphs drawn through a scale transform: device rect (x1 y1 x2 y2) per glyph.
    GlyphsF([f32; 4], Rc<[(Rc<qsg::text::Glyph>, [f32; 4])]>),
    /// `Text.QtRendering` subpixel coverage tiles, blended with a constant colour.
    Df(Option<[f32; 3]>, Rc<[qsg::dftext::DfTile]>),
    Blit(Rc<Canvas>, i64, i64),
    /// Offscreen layers of a source and a mask subtree, composited as an OpacityMask.
    Masked {
        src: Rc<Frame>,
        mask: Rc<Frame>,
        rect: [f32; 4],
        /// Texture fraction the item covers: its layers are ceil(width) x ceil(height).
        crop: [f32; 2],
        invert: bool,
        op: f32,
        /// Nonzero: the source layer persists under this id and is redrawn only where it changed.
        id: u64,
    },
    /// MultiEffect: offscreen source (and mask) layers through the blur, colour, shadow and mask
    /// shader.
    Effect {
        src: Rc<Frame>,
        mask: Option<Rc<Frame>>,
        /// The mask is read linearly, else at its nearest pixel (`Params::mask_smooth`).
        mask_smooth: bool,
        rect: [f32; 4],
        /// `qsg::multieffect::shader_uniforms`.
        u: [f32; 12],
        op: f32,
        blur: Option<Rc<Blur>>,
        shadow: Option<qsg::multieffect::Shadow>,
    },
    /// Offscreen layer composited through an affine transform (texture px -> window px).
    Layer {
        src: Rc<Frame>,
        m: [f32; 6],
        /// The perspective row of a projective map: a texel (u, v) lands at `m`'s point
        /// divided by `p[0] * u + p[1] * v + p[2]`. `[0, 0, 1]` for an affine map.
        p: [f32; 3],
        op: f32,
        /// Texel rect (u0 v0 u1 v1) holding all drawn content; the rest is transparent.
        crop: [f32; 4],
    },
    /// Qt5Compat GaussianBlur/DropShadow: the source layer through the two separable blur
    /// passes (cached as one texture until the source changes), drawn over `rect`. The first
    /// pass fills a layer of `size` texels sampling `src` over `g.band` of it, as
    /// GaussianBlur.qml's horizontal ShaderEffect samples its SourceProxy texture of another
    /// size; the second samples that layer over `rect`.
    Gauss {
        src: Rc<Frame>,
        g: Rc<Gauss>,
        size: (usize, usize),
        rect: [f32; 4],
        qt: Option<GaussAt>,
        op: f32,
    },
    /// The fill of a CurveRenderer Shape, drawn by shapecurve.frag: `v` what the vertex shader
    /// hands over (`qsg::shape::fill_vertices`), `c` the fill colour's floats.
    Curve {
        v: Rc<Vec<f32>>,
        c: [f32; 4],
        op: f32,
    },
    /// QtQuick.Effects RectangularShadow evaluated per fragment (rectangularshadow.frag):
    /// `rect` the effect quad, `u` the shape (`qsg::shadow::Shader::uniforms`), `k` the corner
    /// radii, `c` premultiplied.
    RShadow {
        rect: [f32; 4],
        u: [f32; 5],
        k: [f32; 4],
        c: [f32; 4],
        op: f32,
    },
    /// Qt5Compat RadialGradient over `rect` (radialgradient_nomask.frag, radialgradient_mask.frag):
    /// the gradient texture `tex` read down its height at `r`'s distance, times the alpha of the
    /// `source` layer `mask` (of which the item covers `crop`) when there is one.
    Radial {
        tex: Rc<Canvas>,
        mask: Option<Rc<Frame>>,
        crop: [f32; 2],
        rect: [f32; 4],
        /// RadialGradient.qml's ShaderEffect as Qt draws it: see `GaussAt`.
        qt: Option<GaussAt>,
        r: qsg::gradient::Radial,
        op: f32,
    },
    /// Qt5Compat ColorOverlay (`k` premultiplied colour, OVERLAY), Desaturate (`k.x`, DESAT)
    /// or the layer as it is (PLAIN), of an offscreen source layer stretched over `rect`, read
    /// linearly when `smooth` and nearest otherwise.
    Tint {
        src: Rc<Frame>,
        rect: [f32; 4],
        crop: [f32; 2],
        k: [f32; 4],
        mode: f32,
        smooth: bool,
        op: f32,
    },
    /// Top-down x1 y1 x2 y2 scissor for `clip: true`; None disables it.
    Scissor(Option<[i64; 4]>),
}

/// Draw list of one window frame.
/// Smaller layers re-render whole for less than an extra persistent target costs.
const INC_MIN_AREA: usize = 128 * 128;

/// Cached layers ordered by when they were last drawn (`used` never decreases along the
/// order, as every hit promotes the entry), with their total bytes: eviction pops from the
/// cold end instead of collecting and sorting every layer each frame.
struct LayerCache<V> {
    map: lru::LruCache<u64, V, rustc_hash::FxBuildHasher>,
    bytes: i64,
}

trait Cached {
    fn bytes(&self) -> i64;
}

impl Cached for LayerTex {
    fn bytes(&self) -> i64 {
        self.size.0 as i64 * self.size.1 as i64 * 4
    }
}

impl Cached for IncLayer {
    fn bytes(&self) -> i64 {
        self.size.0 as i64 * self.size.1 as i64 * 4
    }
}

impl<V: Cached> LayerCache<V> {
    fn new() -> Self {
        LayerCache {
            map: lru::LruCache::unbounded_with_hasher(Default::default()),
            bytes: 0,
        }
    }

    /// The entry, now the most recently drawn (the caller sets its `used`).
    fn get_mut(&mut self, k: &u64) -> Option<&mut V> {
        self.map.get_mut(k)
    }

    /// Inserts `v` drawn this frame; a replaced entry is returned.
    fn insert(&mut self, k: u64, v: V) -> Option<V> {
        self.bytes += v.bytes();
        let old = self.map.put(k, v);
        if let Some(o) = &old {
            self.bytes -= o.bytes();
        }
        old
    }

    fn remove(&mut self, k: &u64) -> Option<V> {
        let v = self.map.pop(k)?;
        self.bytes -= v.bytes();
        Some(v)
    }

    /// The least recently drawn entry.
    fn oldest(&self) -> Option<&V> {
        self.map.peek_lru().map(|e| e.1)
    }

    /// The least recently drawn entry if `evict` says so.
    fn pop_oldest_if(&mut self, evict: impl FnOnce(&V, i64) -> bool) -> Option<V> {
        let (_, v) = self.map.peek_lru()?;
        if !evict(v, self.bytes) {
            return None;
        }
        let (_, v) = self.map.pop_lru()?;
        self.bytes -= v.bytes();
        Some(v)
    }

    /// Most recently drawn first.
    fn iter(&self) -> impl DoubleEndedIterator<Item = (&u64, &V)> {
        self.map.iter()
    }

    fn contains_key(&self, k: &u64) -> bool {
        self.map.contains(k)
    }

    /// The entry, left where it is in the order.
    fn peek(&self, k: &u64) -> Option<&V> {
        self.map.peek(k)
    }

    fn len(&self) -> usize {
        self.map.len()
    }

    fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    fn clear(&mut self) {
        self.map.clear();
        self.bytes = 0;
    }
}

/// Frames a layer may sit undrawn before it is freed.
const LAYER_IDLE: u64 = 600;
/// A layer left undrawn this long is freed as well, however few frames were drawn since.
///
/// A clock by design: an expiry, not a poll. Frames are no measure of time in a shell that
/// draws one every few seconds, where a closed panel's layers stayed for half an hour. It is
/// looked at in the loop turns there are (`Gpu::trim_idle`) and wakes none.
const LAYER_IDLE_TIME: std::time::Duration = std::time::Duration::from_secs(60);
/// `Gpu::stamps` keeps at most two frames per this long.
const STAMP_EVERY: std::time::Duration = std::time::Duration::from_secs(1);
/// Every this many frames all undrawn layers are checked for images nobody else holds (layers
/// undrawn since the previous frame are checked every frame).
const STALE_SWEEP: u64 = 64;

/// A layer kept across frames by its item and updated in place where its draw list changed.
struct IncLayer {
    _keep: Vec<Rc<dyn std::any::Any>>,
    size: (i32, i32),
    tex: glow::Texture,
    fb: glow::Framebuffer,
    digest: u64,
    sig: Vec<(u64, [i64; 4])>,
    used: u64,
}

struct LayerTex {
    /// The layer's address-hashed images, so the digest can't be reused by other pixels.
    _keep: Vec<Rc<dyn std::any::Any>>,
    size: (i32, i32),
    tex: glow::Texture,
    fb: glow::Framebuffer,
    used: u64,
}

#[derive(Clone)]
pub struct Frame {
    pub w: usize,
    pub h: usize,
    cmds: Vec<Cmd>,
    /// Window-space bounds of each command (x1 y1 x2 y2), parallel to `cmds`.
    boxes: Vec<[f32; 4]>,
    clips: Vec<[i64; 4]>,
    /// One (content hash, clipped pixel bounds) per drawn primitive: diffing two frames'
    /// signatures gives the damaged region.
    sig: Vec<(u64, [i64; 4])>,
    /// Shared images/glyphs drawn, kept alive (with their uploaded textures) until the next
    /// frame is diffed.
    keep: Vec<Rc<dyn std::any::Any>>,
    /// Index into `sig` of each command's entry, parallel to `cmds`.
    marks: Vec<Option<usize>>,
    /// Item scale (sx, sy, ox, oy) applied to everything recorded: window = s * p + o. Only
    /// layers (`scene::gpu_layer_dpr`) scale x and y differently.
    pub xf: Option<[f32; 4]>,
    /// Device pixels per scene unit (the window's devicePixelRatio): `w`/`h` are device
    /// pixels, `lw`/`lh` the scene size, and `xf` starts at this scale.
    pub dpr: f64,
    pub lw: usize,
    pub lh: usize,
    /// The size of the source rect a layer's texture is drawn over (`sized`).
    rect: Option<[f64; 2]>,
    /// The rect (x y w h, scene units) Qt's projection is over: a layer's source rect, whose
    /// corner is not the origin when the source is padded. None for a window, which is over
    /// its own size from the origin.
    pub over: Option<[f64; 4]>,
    /// `xf` as the frame was made: its own scale, with no item's on top.
    own: Option<[f32; 4]>,
    /// The frame's x and y run against the screen's: it is the texture of a subtree turned
    /// upside down or by a quarter (`qsg::text::snap_px`).
    pub against: [bool; 2],
    /// The frame is a window: Qt projects over whole scene units (`window_units`). A texture
    /// that is no window shows its pixels over the devicePixelRatio, a fraction too.
    whole: bool,
    /// Where Qt's renderer has each item being drawn, the innermost last (`push_place`), and
    /// which of `ops` is the `Op::Place` that put it there.
    places: Vec<(Place, Option<usize>)>,
    /// Translatable draw calls so far, and the count of draws recorded without one.
    ops: Vec<Op>,
    opaque: usize,
    opaque_at: Option<&'static std::panic::Location<'static>>,
    /// `shared_texs` memo, valid while `cmds` keeps the length it was taken at.
    shared: std::cell::RefCell<Option<(usize, Vec<u64>)>>,
    /// (command index, run count): the runs of that `Cmd::Tris` a recording started after,
    /// which later triangles must not grow (`Frame::begin`).
    seal: std::cell::Cell<(usize, usize)>,
}

/// Device pixels of a `lw` x `lh` surface at scale `dpr`: wp_fractional_scale_v1 rounds
/// (logical size * scale), half away from zero.
pub fn device_size(lw: usize, lh: usize, dpr: f64) -> (usize, usize) {
    (
        (lw as f64 * dpr).round() as usize,
        (lh as f64 * dpr).round() as usize,
    )
}

const ANY_BOX: [f32; 4] = [f32::MIN, f32::MIN, f32::MAX, f32::MAX];

/// MultiEffect's BlurItem chain (`qsg::multieffect::draw_with`) as offscreen GL passes.
#[derive(Clone, PartialEq)]
pub struct Blur {
    /// BlurItem sizes in scene units, each a pass over the previous level (the first over the
    /// source).
    pub sizes: Vec<(f64, f64)>,
    /// The window's devicePixelRatio: each BlurItem is a layer of its size times this.
    pub dpr: f64,
    pub multiplier: f32,
    /// multieffect.frag weights of the source and each level (unused levels 0); the source
    /// alone when the levels are there for the shadow only.
    pub weights: [f32; 6],
}

/// Cells per side of the grid a projected layer is drawn with.
const PROJ_GRID: usize = 48;
/// The smallest w of a projected point that is drawn: in front of the viewer.
const PROJ_NEAR: f32 = 1e-6;

/// A GaussianBlur/DropShadow pass program with its `s x d k t w` uniforms.
type GaussProg = (glow::Program, [Option<glow::UniformLocation>; 6]);

/// The most taps a blur shader has baked in, as qgfxshaderbuilder bakes them: the longest
/// kernel of its optimized shader (`qsg::blur::MAX_BLUR_SAMPLES`). A longer kernel is the
/// fallback one of a tap per texel (the original's lock screen blurs the wallpaper with 201, its default
/// radius of 100) and reads the same taps from a uniform array in a loop, whose picture
/// differs by at most 1/255 in a few pixels. Mesa's llvmpipe compiles a fragment shader on
/// the thread that first draws with it, a texture lookup at a time: the first pass with 201
/// baked taps took 8 to 12 s there (every later one 0.3 s) and the lock screen waited for
/// it, with the loop 0.4 to 0.6 s. Where the array does not fit the
/// context's fragment uniforms (`Gauss::looped`) the taps are baked whatever their number,
/// as Qt's are.
const BAKED_TAPS: usize = qsg::blur::MAX_BLUR_SAMPLES as usize;

/// The fragment uniform vectors a looped blur shader takes besides its array `w`, one vector
/// per element: the sampler `x`, `d`, `k` and `t` (GLSL ES 1.00 appendix A.7 counts a vec2,
/// a vec4, a float and a sampler as one vector each).
const LOOP_UNIFORMS: usize = 4;

/// The vertical pass of a blur as Qt draws it into its target: the vertices of the effect's own
/// rect `item` through `m`, the target's projection times the effect's node matrix (both
/// QMatrix4x4, multiplied as one), in a viewport of `target` device pixels.
#[derive(Clone, PartialEq)]
pub struct GaussAt {
    m: [f32; 16],
    item: [f32; 4],
    target: (i32, i32),
}

/// Qt5Compat GaussianBlur/DropShadow kernel (`qsg::blur::kernel`): one shader per tap set, its
/// offsets and weights baked in like qgfxshaderbuilder's generated shaders.
#[derive(Clone, PartialEq)]
pub struct Gauss {
    /// (offset in texels before `spread`, weight) per tap.
    pub taps: Vec<(f32, f32)>,
    pub spread: f32,
    /// What the shader divides the taps' sum by: the fallback shader's `wSum`, else 1.
    pub sum: f32,
    /// GaussianBlur.qml `_paddedTexWidth * _dpr` and `_paddedTexHeight * _dpr`: a tap is
    /// `spread` over these apart in texture coordinates, one device pixel when `spread` is 1.
    pub per: [f32; 2],
    /// Where the horizontal pass draws in its layer, as the fractions (top, height) of the
    /// layer's height: with transparentBorder the pass is as tall as the effect inside a layer
    /// `radius` taller on each side (`layer.sourceRect`), else it fills the layer, (0, 1).
    pub band: [f32; 2],
    /// DropShadow: both passes keep clamp(alpha / thickness), the second tinted with this
    /// premultiplied colour (`qsg::blur::drop_shadow`).
    pub shadow: Option<([f32; 4], f32)>,
    /// The horizontal pass as QSGRhiLayer projects it, in the effect's units: the pass's
    /// item (width, height) and its layer's rect (top, height), which is the item's own or
    /// `layer.sourceRect`. None draws `band` of the layer's texels.
    pub first: Option<[f32; 4]>,
}

impl Gauss {
    /// Whether the shader reads its taps from the uniform array `w` (see `BAKED_TAPS`) on a
    /// context with `uniforms` fragment uniform vectors (GL_MAX_FRAGMENT_UNIFORM_VECTORS, at
    /// least 16 in GLES 2.0): only when the array and the other uniforms fit them, or the
    /// program would not link (GLES 2.0 §2.10.4) and the blur would draw nothing.
    fn looped(&self, uniforms: usize) -> bool {
        self.taps.len() > BAKED_TAPS && self.taps.len() + LOOP_UNIFORMS <= uniforms
    }

    /// What tells one pass program from another: the text of `shader`. The spread and the
    /// shadow's colour and thickness are uniforms, so every shadow of one kernel shares a
    /// program whatever its colour.
    fn prog_key(&self, uniforms: usize) -> u64 {
        let h = mix(
            mix(13, self.taps.len() as u64),
            self.shadow.is_some() as u64,
        );
        if self.looped(uniforms) {
            return h;
        }
        self.taps.iter().fold(h, |h, t| mix_f(h, &[t.0, t.1]))
    }

    fn digest(&self) -> u64 {
        let h = self
            .taps
            .iter()
            .fold(mix(12, self.taps.len() as u64), |h, t| {
                mix_f(h, &[t.0, t.1])
            });
        let h = mix_f(h, &[self.spread, self.sum]);
        let h = mix_f(mix_f(h, &self.per), &self.band);
        let h = self.first.map_or(h, |f| mix_f(mix(h, 2), &f));
        match self.shadow {
            Some((c, t)) => mix_f(mix(h, 1), &[c[0], c[1], c[2], c[3], t]),
            None => h,
        }
    }

    /// The pass's fragment shader: `d` the tap step in texture coordinates, `k` the shadow
    /// tint, `t` the shadow's thickness and `sum`; `uniforms` as in `looped`. The taps' sum
    /// is divided as qgfx_fallbackFragmentShader divides it: `(qt_Opacity / wSum) * result`,
    /// and `(result / wSum) / thickness` of a shadow.
    fn shader(&self, uniforms: usize) -> String {
        let mut fs = String::from(
            "varying vec2 u; uniform sampler2D x; uniform vec2 d;
uniform vec4 k; uniform vec2 t;
void main() { vec4 c = vec4(0.0);\n",
        );
        if self.looped(uniforms) {
            let n = self.taps.len();
            fs = format!("uniform vec2 w[{n}];\n{fs}");
            fs += &format!(
                " for (int i = 0; i < {n}; i++) c += texture2D(x, u + d * w[i].x) * w[i].y;\n"
            );
        } else {
            for &(o, w) in &self.taps {
                fs += &format!(
                    " c += texture2D(x, u + d * {}) * {};\n",
                    glsl_f(o),
                    glsl_f(w)
                );
            }
        }
        fs += match self.shadow {
            Some(_) => " gl_FragColor = k * clamp((c.a / t.y) / t.x, 0.0, 1.0); }",
            None => " gl_FragColor = (1.0 / t.y) * c; }",
        };
        fs
    }
}

/// A GLSL float literal that parses back to exactly `v`.
fn glsl_f(v: f32) -> String {
    let s = format!("{v:?}");
    if s.contains(['.', 'e']) {
        s
    } else {
        format!("{s}.0")
    }
}

// GL calls issued so far (`ri::glcount` prints them, `take_gl_calls` reads them): glTexImage2D,
// glTexSubImage2D, framebuffer allocations, glUseProgram, glUniform*, glTexParameteri,
// vertex attrib enables/pointers, glBindBuffer, and the glTexSubImage2D into an atlas while a
// draw list runs. Per thread, like the GL context that issues them (parallel tests each count
// their own).
thread_local! {
    static GL_COUNTS: std::cell::Cell<[u64; 9]> = const { std::cell::Cell::new([0; 9]) };
    /// A draw list is being run (`Gpu::exec`).
    static DRAWING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}
const GL_PROGRAM: usize = 3;
const GL_UNIFORM: usize = 4;
const GL_TEXPARAM: usize = 5;
const GL_ATTRIB: usize = 6;
const GL_BINDBUF: usize = 7;
/// An upload into an atlas between the draws of a frame: the driver must first finish the draws
/// that sample the atlas (llvmpipe waits for its rasterizer threads each time).
const GL_MIDDRAW: usize = 8;

fn gl_count(k: usize) {
    GL_COUNTS.with(|c| {
        let mut v = c.get();
        v[k] += 1;
        c.set(v);
    });
}

thread_local! {
    /// Draw lists run into a layer's texture so far, whole or the changed part of one.
    static LAYER_DRAWS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Resets and returns how many times this thread drew into a layer's texture.
pub fn take_layer_draws() -> u64 {
    LAYER_DRAWS.with(|c| c.replace(0))
}

/// Resets and returns this thread's GL call counters, indexed as `GL_COUNTS`.
pub fn take_gl_calls() -> [u64; 9] {
    GL_COUNTS.with(|c| c.replace([0; 9]))
}

fn print_gl_counts() {
    static LAST: std::sync::Mutex<[u64; 9]> = std::sync::Mutex::new([0; 9]);
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    let c = GL_COUNTS.with(|c| c.get());
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    if *last != c {
        *last = c;
        tracing::debug!(
            target: "ri::glcount",
            "glcount ms={t} teximage={} subimage={} fbo={} program={} uniform={} texparam={} attrib={} bindbuf={} middraw={}",
            c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7], c[8]
        );
    }
}

/// Opacity of the frame a crossfade starts from, at progress `t` (0..=1) of the fade.
pub fn fade_opacity(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - t * t * (3.0 - 2.0 * t)
}

/// A uniform's last value, compared bitwise (so -0.0 and NaN never pass as unchanged).
#[derive(Clone, Copy, PartialEq)]
enum Uni {
    F([u32; 4], u8),
    I(i32),
}

impl Uni {
    fn f(v: &[f32]) -> Uni {
        let mut b = [0u32; 4];
        for (d, x) in b.iter_mut().zip(v) {
            *d = x.to_bits();
        }
        Uni::F(b, v.len() as u8)
    }
}

/// Programs with cached uniforms (`State::uni`).
const P_COLOR: usize = 0;
const P_TEX: usize = 1;
const P_EFFECT: usize = 2;
const P_BLUR: usize = 3;
const P_FADE: usize = 4;
const P_DIFF: usize = 5;
const P_RSHADOW: usize = 6;
const P_RADIAL: usize = 7;
const P_CURVE: usize = 8;

/// GL state as this renderer last set it, so an unchanged value is not sent again. Program,
/// uniform, vertex attrib and array buffer state is forgotten at every frame start (`reset`);
/// texture filters are object state and stay known until the texture is deleted (`del_tex`).
#[derive(Default)]
struct State {
    program: Option<glow::Program>,
    uni: [[Option<Uni>; 17]; 9],
    attrib_on: [Option<bool>; 3],
    /// (buffer, size, stride, offset) per attrib.
    attrib_ptr: [Option<(glow::Buffer, i32, i32, i32)>; 3],
    array_buf: Option<glow::Buffer>,
    /// TEXTURE_MIN_FILTER of each texture; its TEXTURE_MAG_FILTER is that without the
    /// mipmaps (`filter_set`).
    filters: HashMap<glow::Texture, i32>,
    /// The textures that have their chain of mipmap levels (glGenerateMipmap).
    mipped: std::collections::HashSet<glow::Texture>,
}

impl State {
    fn reset(&mut self) {
        self.program = None;
        self.uni = [[None; 17]; 9];
        self.attrib_on = [None; 3];
        self.attrib_ptr = [None; 3];
        self.array_buf = None;
    }
}

/// Deletes texture `t` and forgets its cached filter.
///
/// # Safety
/// The GL context `gl` was loaded for must be current on the calling thread.
unsafe fn del_tex(gl: &glow::Context, st: &StateCell, t: glow::Texture) {
    {
        let mut st = st.borrow_mut();
        st.filters.remove(&t);
        st.mipped.remove(&t);
    }
    // SAFETY: `gl` is current, as this fn requires.
    unsafe { gl.delete_texture(t) };
}

type StateCell = std::cell::RefCell<State>;

/// Sets uniform `k` of program `prog`, at `loc`, to `v`; with `cache`, only if the cached state has
/// another value.
///
/// # Safety
/// The GL context `gl` was loaded for must be current on the calling thread.
unsafe fn uni_set(
    gl: &glow::Context,
    st: &StateCell,
    cache: bool,
    prog: usize,
    k: usize,
    loc: Option<&glow::UniformLocation>,
    v: Uni,
) {
    {
        let mut st = st.borrow_mut();
        if cache && st.uni[prog][k] == Some(v) {
            return;
        }
        st.uni[prog][k] = Some(v);
    }
    gl_count(GL_UNIFORM);
    match v {
        // SAFETY: `gl` is current, as this fn requires.
        Uni::I(i) => unsafe { gl.uniform_1_i32(loc, i) },
        Uni::F(b, n) => {
            let f = b.map(f32::from_bits);
            match n {
                // SAFETY: `gl` is current, as this fn requires.
                1 => unsafe { gl.uniform_1_f32(loc, f[0]) },
                // SAFETY: `gl` is current, as this fn requires.
                2 => unsafe { gl.uniform_2_f32(loc, f[0], f[1]) },
                // SAFETY: `gl` is current, as this fn requires.
                3 => unsafe { gl.uniform_3_f32(loc, f[0], f[1], f[2]) },
                // SAFETY: `gl` is current, as this fn requires.
                _ => unsafe { gl.uniform_4_f32(loc, f[0], f[1], f[2], f[3]) },
            }
        }
    }
}

/// Sets the min filter of `t`, bound to the active unit, to `f`, and its mag filter to `f`
/// without the mipmaps (GL_LINEAR of GL_LINEAR_MIPMAP_LINEAR, GL_NEAREST of
/// GL_NEAREST_MIPMAP_LINEAR).
///
/// # Safety
/// The GL context `gl` was loaded for must be current on the calling thread.
unsafe fn filter_set(gl: &glow::Context, st: &StateCell, cache: bool, t: glow::Texture, f: i32) {
    if cache && st.borrow().filters.get(&t) == Some(&f) {
        return;
    }
    st.borrow_mut().filters.insert(t, f);
    gl_count(GL_TEXPARAM);
    gl_count(GL_TEXPARAM);
    let mag = match f as u32 {
        glow::LINEAR_MIPMAP_LINEAR => glow::LINEAR as i32,
        glow::NEAREST_MIPMAP_LINEAR => glow::NEAREST as i32,
        _ => f,
    };
    // SAFETY: `gl` is current, as this fn requires.
    unsafe { gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, f) };
    // SAFETY: `gl` is current, as this fn requires.
    unsafe { gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, mag) };
}

/// Deletes buffer `b` and forgets what the cached state says of it.
///
/// # Safety
/// The GL context `gl` was loaded for must be current on the calling thread.
unsafe fn del_buf(gl: &glow::Context, st: &StateCell, b: glow::Buffer) {
    {
        let mut st = st.borrow_mut();
        if st.array_buf == Some(b) {
            st.array_buf = None;
        }
        for p in st.attrib_ptr.iter_mut() {
            if p.is_some_and(|p| p.0 == b) {
                *p = None;
            }
        }
    }
    // SAFETY: `gl` is current, as this fn requires.
    unsafe { gl.delete_buffer(b) };
}

fn mix(h: u64, v: u64) -> u64 {
    (h.rotate_left(5) ^ v).wrapping_mul(0x517c_c1b7_2722_0a95)
}

fn mix_f(h: u64, v: &[f32]) -> u64 {
    v.iter().fold(h, |h, x| mix(h, x.to_bits() as u64))
}

thread_local! {
    /// Keys of the shared canvases with an uploaded texture.
    static UPLOADED: std::cell::RefCell<std::collections::HashSet<u64>> = Default::default();
    /// Uploaded images trading their pixels for a pixel-less stand-in (`resident`).
    static ADOPT: std::cell::RefCell<Vec<(Rc<Canvas>, std::rc::Weak<Canvas>)>> = Default::default();
}

/// A pixel-less stand-in for `img` drawing its already uploaded texture, so the CPU copy can
/// be freed; `None` while `img` has no texture. Only `Frame::image` can draw it (one GL context).
pub fn resident(img: &Rc<Canvas>) -> Option<Rc<Canvas>> {
    if img.px.len() < img.w * img.h || !UPLOADED.with_borrow(|u| u.contains(&img.key.get())) {
        return None;
    }
    let p = Rc::new(Canvas {
        w: img.w,
        h: img.h,
        px: Vec::new(),
        // A key of its own that says the same of the alpha channel.
        key: img.key.clone(),
    });
    ADOPT.with_borrow_mut(|a| a.push((img.clone(), Rc::downgrade(&p))));
    Some(p)
}

/// Content hash of a raster painted this frame, which has no identity to sign it by (a shared
/// image is signed by `Canvas::key`). Four independent lanes of two pixels each keep it bound
/// by memory bandwidth rather than by a serial multiply chain.
fn mix_px(h: u64, c: &Canvas) -> u64 {
    let h = mix(mix(h, c.w as u64), c.h as u64);
    let (chunks, rest) = as_bytes(&c.px).as_chunks::<32>();
    let mut lanes = [h, h ^ 1, h ^ 2, h ^ 3];
    for c in chunks {
        for (l, w) in lanes.iter_mut().zip(c.as_chunks::<8>().0) {
            *l = mix(*l, u64::from_ne_bytes(*w));
        }
    }
    let h = lanes.iter().fold(h, |h, &l| mix(h, l));
    rest.as_chunks::<4>()
        .0
        .iter()
        .fold(h, |h, p| mix(h, u32::from_ne_bytes(*p) as u64))
}

fn union(a: Option<[i64; 4]>, b: [i64; 4]) -> Option<[i64; 4]> {
    Some(match a {
        None => b,
        Some(a) => [
            a[0].min(b[0]),
            a[1].min(b[1]),
            a[2].max(b[2]),
            a[3].max(b[3]),
        ],
    })
}

fn intersect(a: [i64; 4], b: [i64; 4]) -> [i64; 4] {
    [
        a[0].max(b[0]),
        a[1].max(b[1]),
        a[2].min(b[2]),
        a[3].min(b[3]),
    ]
}

/// Region that differs between two frame signatures; `None` when they draw the same.
fn diff(prev: &[(u64, [i64; 4])], cur: &[(u64, [i64; 4])], full: [i64; 4]) -> Option<Region> {
    // Primitives equal at the same place from either end change no count and no relative
    // order: only the differing middle is counted (none at all for an unchanged frame).
    let pre = prev.iter().zip(cur).take_while(|(a, b)| a == b).count();
    let (prev, cur) = (&prev[pre..], &cur[pre..]);
    let suf = prev
        .iter()
        .rev()
        .zip(cur.iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let (prev, cur) = (&prev[..prev.len() - suf], &cur[..cur.len() - suf]);
    if prev.is_empty() && cur.is_empty() {
        return None;
    }
    let mut n: HashMap<(u64, [i64; 4]), i32> = HashMap::with_capacity(prev.len());
    for e in prev {
        *n.entry(*e).or_default() += 1;
    }
    for e in cur {
        *n.entry(*e).or_default() -= 1;
    }
    let mut boxes: Vec<[i64; 4]> = n
        .iter()
        .filter(|(_, c)| **c != 0)
        .map(|(e, _)| e.1)
        .collect();
    // Merging depends on the order rectangles come in: make it the same every run.
    boxes.sort_unstable();
    let mut d = Region::new();
    for b in boxes {
        region::add(&mut d, b);
    }
    // Same primitives in a different stacking order repaint everything they cover.
    let common = |l: &[(u64, [i64; 4])]| {
        l.iter()
            .filter(|e| n.get(*e) == Some(&0))
            .copied()
            .collect::<Vec<_>>()
    };
    let (a, b) = (common(prev), common(cur));
    if a != b {
        let r = reorder_damage(&a, &b);
        if qml::log_on!("ri::dmgdbg") {
            tracing::debug!(target: "ri::dmgdbg", "dmg order {r:?} of {}", a.len());
        }
        return Some(r.map_or_else(
            || vec![full],
            |r| {
                region::add(&mut d, r);
                d
            },
        ));
    }
    if qml::log_on!("ri::dmgdbg") {
        let mut v: Vec<_> = n
            .iter()
            .filter(|(_, c)| **c != 0)
            .map(|(e, c)| (e.1, *c))
            .collect();
        v.sort();
        tracing::debug!(target: "ri::dmgdbg", "dmg {} {:?}", v.len(), &v[..v.len().min(12)]);
    }
    (!d.is_empty()).then_some(d)
}

/// The area where two orderings of the same primitives stack differently: a pixel changes only
/// where two of them swapped, and every swapped pair has one primitive outside a common
/// subsequence of the two orders, so the boxes of those cover it.
fn reorder_damage(a: &[(u64, [i64; 4])], b: &[(u64, [i64; 4])]) -> Option<[i64; 4]> {
    let pre = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let (a, b) = (&a[pre..], &b[pre..]);
    let suf = a
        .iter()
        .rev()
        .zip(b.iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let (a, b) = (&a[..a.len() - suf], &b[..b.len() - suf]);
    let mut d = None;
    if a.len() * b.len() > 1 << 18 {
        for e in a.iter().chain(b) {
            d = union(d, e.1);
        }
        return d;
    }
    // Longest common subsequence; what lies outside it in `a` moved.
    let m = b.len() + 1;
    let mut t = vec![0u32; (a.len() + 1) * m];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            t[i * m + j] = if a[i] == b[j] {
                t[(i + 1) * m + j + 1] + 1
            } else {
                t[(i + 1) * m + j].max(t[i * m + j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        if a[i] == b[j] {
            i += 1;
            j += 1;
        } else if t[(i + 1) * m + j] >= t[i * m + j + 1] {
            d = union(d, a[i].1);
            i += 1;
        } else {
            j += 1;
        }
    }
    for e in &a[i..] {
        d = union(d, e.1);
    }
    d
}

/// Draws one subtree recorded into a frame, replayable at the same position and clip.
pub struct Seg {
    /// Vertex runs the subtree appended to a triangle batch that was already open, and their box.
    lead: Option<(TrisRuns, [f32; 4])>,
    /// The draw calls behind `cmds` when all of them can be re-emitted at another offset.
    ops: Option<Vec<Op>>,
    cmds: Vec<Cmd>,
    boxes: Vec<[f32; 4]>,
    marks: Vec<Option<usize>>,
    sig: Vec<(u64, [i64; 4])>,
    keep: Vec<Rc<dyn std::any::Any>>,
}

impl Seg {
    /// Whether the segment keeps alive a shared object at any of the addresses `ptrs`.
    pub fn holds_any(&self, ptrs: &rustc_hash::FxHashSet<usize>) -> bool {
        self.keep
            .iter()
            .any(|k| ptrs.contains(&(Rc::as_ptr(k) as *const () as usize)))
    }
}

/// Recording start (see `Frame::begin`).
pub struct SegStart {
    ops: usize,
    opaque: usize,
    cmds: usize,
    sig: usize,
    keep: usize,
    clips: usize,
    tris: Option<usize>,
}

/// A draw call kept with its source offset, so `Frame::replay_at` can redo it elsewhere
/// (vertices and glyph snapping recomputed exactly as a fresh draw would).
#[derive(Clone)]
enum Op {
    Smooth(Rc<Geometry>, f64, f64, f32),
    Glyphs(Rc<[GlyphQuadF]>, f64, f64, [u8; 4], f32),
    /// `Frame::text_dpr`: the device-scale stack, line, node and pen position, color, opacity.
    GlyphsDpr(
        Rc<qsg::text::FontStack>,
        Rc<qsg::text::ShapedLine>,
        [f64; 4],
        [u8; 4],
        f32,
    ),
    /// texture, rect, uv, opacity, smooth, mipmap, and the colour of `Frame::image_mask`
    Image(
        Rc<Canvas>,
        [f64; 4],
        [f64; 4],
        f32,
        bool,
        bool,
        Option<[u8; 4]>,
    ),
    Clip([f64; 4]),
    Unclip,
    /// `Frame::push_place` and `pop_place`: how far the parent is from the place above, the
    /// item's own position, and whether it clips.
    Place([f64; 2], [f64; 2], bool),
    Unplace,
    Masked(Rc<Frame>, Rc<Frame>, [f64; 4], bool, f32, u64),
}

impl Seg {
    pub fn translatable(&self) -> bool {
        self.ops.is_some()
    }

    pub fn sig(&self) -> &[(u64, [i64; 4])] {
        &self.sig
    }
}

impl Frame {
    /// Whether `self` draws what `other` draws: their signatures leave no damage between them.
    pub fn draws_same(&self, other: &Frame) -> bool {
        (self.w, self.h) == (other.w, other.h)
            && diff(&other.sig, &self.sig, [0, 0, self.w as i64, self.h as i64]).is_none()
    }

    /// Where `self` draws something else than `old` did, as rectangles that do not overlap,
    /// when that is a part of the window: at most half of it. None when more changed (a theme
    /// switch) or nothing did.
    pub fn changed_part(&self, old: &Frame) -> Option<Region> {
        let full = [0, 0, self.w as i64, self.h as i64];
        if (self.w, self.h) != (old.w, old.h) {
            return None;
        }
        let part = region::disjoint(&region::clip(&diff(&old.sig, &self.sig, full)?, full));
        let px: i64 = part.iter().map(|r| region::area(*r)).sum();
        (px > 0 && px * 2 <= region::area(full)).then_some(part)
    }

    pub fn clip_top(&self) -> Option<[i64; 4]> {
        self.clips.last().copied()
    }

    /// Draw signatures recorded so far (tests: the same texture drawn twice signs the same).
    #[cfg(test)]
    pub(crate) fn sigs(&self) -> &[(u64, [i64; 4])] {
        &self.sig
    }

    pub fn begin(&self) -> SegStart {
        SegStart {
            ops: self.ops.len(),
            opaque: self.opaque,
            cmds: self.cmds.len(),
            sig: self.sig.len(),
            keep: self.keep.len(),
            clips: self.clips.len(),
            tris: match self.cmds.last() {
                Some(Cmd::Tris(runs)) => {
                    self.seal.set((self.cmds.len() - 1, runs.len()));
                    Some(runs.len())
                }
                _ => None,
            },
        }
    }

    pub fn sig_since(&self, s: &SegStart) -> &[(u64, [i64; 4])] {
        &self.sig[s.sig.min(self.sig.len())..]
    }

    /// Everything drawn since `s`; `None` when the clip stack is unbalanced.
    pub fn record(&self, s: &SegStart) -> Option<Seg> {
        if self.clips.len() != s.clips || self.cmds.len() < s.cmds {
            return None;
        }
        let lead = match (s.tris, s.cmds.checked_sub(1).and_then(|k| self.cmds.get(k))) {
            (Some(off), Some(Cmd::Tris(runs))) if runs.len() > off => {
                let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
                let (w, h) = (self.w as f32, self.h as f32);
                for vx in runs[off..].iter().flat_map(|r| r.as_chunks::<6>().0) {
                    let [x, y] = qsg::raster::viewport([vx[0], vx[1]], w, h);
                    b = [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)];
                }
                Some((runs[off..].to_vec(), b))
            }
            _ => None,
        };
        Some(Seg {
            lead,
            ops: (self.opaque == s.opaque && self.plain()).then(|| self.ops[s.ops..].to_vec()),
            cmds: self.cmds[s.cmds..].to_vec(),
            boxes: self.boxes[s.cmds..].to_vec(),
            marks: self.marks[s.cmds..]
                .iter()
                .map(|m| m.map(|j| j - s.sig))
                .collect(),
            sig: self.sig[s.sig..].to_vec(),
            keep: self.keep[s.keep..].to_vec(),
        })
    }

    fn append_tris(&mut self, v: &[Rc<Vec<f32>>], b: [f32; 4]) {
        if let Some(Cmd::Tris(out)) = self.cmds.last_mut() {
            out.extend(v.iter().cloned());
            let last = self.boxes.last_mut().expect("tris box");
            *last = [
                last[0].min(b[0]),
                last[1].min(b[1]),
                last[2].max(b[2]),
                last[3].max(b[3]),
            ];
        } else {
            self.cmds.push(Cmd::Tris(v.to_vec()));
            self.boxes.push(b);
            self.marks.push(None);
        }
    }

    pub fn replay(&mut self, s: &Seg) {
        let base = self.sig.len();
        if let Some((v, b)) = &s.lead {
            self.append_tris(v, *b);
        }
        let mut rest = 0;
        if s.lead.is_none()
            && let Some(Cmd::Tris(v)) = s.cmds.first()
            && matches!(self.cmds.last(), Some(Cmd::Tris(_)))
        {
            self.append_tris(v, s.boxes[0]);
            rest = 1;
        }
        self.cmds.extend(s.cmds[rest..].iter().cloned());
        self.boxes.extend_from_slice(&s.boxes[rest..]);
        self.marks
            .extend(s.marks[rest..].iter().map(|m| m.map(|j| j + base)));
        self.sig.extend_from_slice(&s.sig);
        self.keep.extend(s.keep.iter().cloned());
        match &s.ops {
            Some(o) if self.plain() => self.ops.extend(o.iter().cloned()),
            _ => self.taint(),
        }
    }

    /// Redraws a translatable segment moved by (`dx`, `dy`); false (nothing drawn) otherwise.
    pub fn replay_at(&mut self, s: &Seg, dx: f64, dy: f64) -> bool {
        let Some(ops) = &s.ops else { return false };
        if !self.plain() {
            return false;
        }
        for op in ops {
            match op {
                Op::Smooth(g, x, y, o) => self.smooth(g, x + dx, y + dy, *o),
                Op::Glyphs(q, x, y, c, o) => self.glyphs(q.clone(), x + dx, y + dy, *c, *o),
                Op::GlyphsDpr(st, l, [x, y, px, py], c, o) => {
                    self.text_dpr(st.clone(), l.clone(), [x + dx, y + dy, *px, *py], *c, *o)
                }
                Op::Image(t, r, uv, o, sm, mip, tint) => {
                    let r = [r[0] + dx, r[1] + dy, r[2], r[3]];
                    self.image_filtered(t.clone(), r, *uv, *o, *sm, *mip, *tint)
                }
                Op::Clip(r) => self.push_clip_f([r[0] + dx, r[1] + dy, r[2], r[3]]),
                Op::Unclip => self.pop_clip(),
                Op::Place(extra, own, clip) => self.place_by(*extra, *own, *clip),
                Op::Unplace => self.pop_place(),
                Op::Masked(sf, mf, r, inv, o, id) => {
                    let r = [r[0] + dx, r[1] + dy, r[2], r[3]];
                    self.masked_rc(sf.clone(), mf.clone(), r, *inv, *o, *id)
                }
            }
        }
        true
    }

    /// An empty frame with this one's size, clip stack and transform (to check a replay).
    pub fn scratch(&self) -> Frame {
        let mut f = Frame::new(self.w, self.h);
        f.clips = self.clips.clone();
        f.places = self.places.iter().map(|p| (p.0, None)).collect();
        f.xf = self.xf;
        f.own = self.own;
        f.over = self.over;
        f.against = self.against;
        f.whole = self.whole;
        f.dpr = self.dpr;
        f.lw = self.lw;
        f.lh = self.lh;
        f
    }

    /// Marks a draw whose output depends on its absolute position (pixel-aligned rasters).
    #[track_caller]
    pub fn opaque_draw(&mut self) {
        self.taint();
    }

    #[track_caller]
    fn taint(&mut self) {
        self.opaque += 1;
        if qml::log_on!("ri::dldbg") {
            self.opaque_at = Some(std::panic::Location::caller());
        }
    }

    /// Where the last untranslatable draw was recorded (`ri::dldbg`).
    pub fn opaque_at(&self) -> Option<&'static std::panic::Location<'static>> {
        self.opaque_at
    }

    pub fn new(w: usize, h: usize) -> Self {
        Self::scaled(w, h, 1.0)
    }

    /// A frame of scene size `lw` x `lh` drawn on `dpr` times as many device pixels per axis
    /// (`device_size`), like a Qt Quick window at that devicePixelRatio.
    pub fn scaled(lw: usize, lh: usize, dpr: f64) -> Self {
        let dpr = if dpr.is_finite() && dpr > 0.0 {
            dpr
        } else {
            1.0
        };
        let (w, h) = device_size(lw, lh, dpr);
        Self {
            w,
            h,
            dpr,
            lw,
            lh,
            rect: None,
            cmds: Vec::new(),
            boxes: Vec::new(),
            clips: Vec::new(),
            sig: Vec::new(),
            keep: Vec::new(),
            marks: Vec::new(),
            xf: Self::window_scale(w, h, dpr),
            over: None,
            against: [false; 2],
            own: Self::window_scale(w, h, dpr),
            whole: true,
            places: Vec::new(),
            ops: Vec::new(),
            opaque: 0,
            opaque_at: None,
            shared: Default::default(),
            seal: Default::default(),
        }
    }

    /// `scaled` for a texture that is no window: every scene unit is `dpr` pixels, also where
    /// that leaves a fraction of a unit over.
    pub fn offscreen(lw: usize, lh: usize, dpr: f64) -> Self {
        let mut f = Self::scaled(lw, lh, dpr);
        f.xf = (f.dpr != 1.0).then_some([f.dpr as f32, f.dpr as f32, 0.0, 0.0]);
        f.own = f.xf;
        f.whole = false;
        f
    }

    /// What was recorded so far, as a frame of its own; this one goes on empty, with the same
    /// size, scale and clips. None when nothing was recorded.
    pub fn take_recorded(&mut self) -> Option<Frame> {
        if self.cmds.is_empty() {
            return None;
        }
        let mut fresh = Self::scaled(self.lw, self.lh, self.dpr);
        (fresh.w, fresh.h, fresh.xf) = (self.w, self.h, self.xf);
        (fresh.own, fresh.over, fresh.whole) = (self.own, self.over, self.whole);
        fresh.rect = self.rect;
        fresh.against = self.against;
        fresh.clips = self.clips.clone();
        fresh.places = self.places.iter().map(|p| (p.0, None)).collect();
        Some(std::mem::replace(self, fresh))
    }

    /// This frame on a buffer of `dev` device pixels: a window whose surface a screen's factor
    /// made a fraction of a scene unit larger than the window.
    pub fn onto(mut self, dev: (usize, usize)) -> Self {
        (self.w, self.h) = dev;
        self.xf = Self::window_scale(self.w, self.h, self.dpr);
        self.own = self.xf;
        self
    }

    /// The scene units a window of `px` device pixels along an axis shows: QQuickWindow
    /// projects over its pixel size divided by the devicePixelRatio as a QSize, which rounds
    /// (QQuickWindowPrivate::renderSceneGraph).
    fn window_units(px: usize, dpr: f64) -> f64 {
        (px as f64 / dpr).round()
    }

    /// The scale of a window of `w` x `h` device pixels at `dpr`, from scene units to pixels:
    /// the pixels over the whole units Qt projects over. That is `dpr` where the pixels are a
    /// whole number of times it, and a little more or less where a fractional scale or a
    /// screen's factor left a fraction of a unit over.
    fn window_scale(w: usize, h: usize, dpr: f64) -> Option<[f32; 4]> {
        let along = |px: usize| match Self::window_units(px, dpr) {
            units if units >= 1.0 => (px as f64 / units) as f32,
            _ => dpr as f32,
        };
        (dpr != 1.0).then(|| [along(w), along(h), 0.0, 0.0])
    }

    /// The scene units the frame's pixels show: whole ones in a window (`window_units`).
    pub fn units(&self) -> (f64, f64) {
        if self.whole {
            (
                Self::window_units(self.w, self.dpr),
                Self::window_units(self.h, self.dpr),
            )
        } else {
            (self.w as f64 / self.dpr, self.h as f64 / self.dpr)
        }
    }

    /// The texture of a QQuickShaderEffectSource over a source rect of `lw` x `lh` scene units
    /// in a window of devicePixelRatio `dpr`: (size * dpr) rounded once (QSizeF::toSize), at
    /// least 1 x 1, with the rect drawn over all of it as QSGLayer does.
    pub fn sized(lw: f64, lh: f64, dpr: f64) -> Self {
        let mut f = Self::offscreen(
            lw.ceil().max(1.0) as usize,
            lh.ceil().max(1.0) as usize,
            dpr,
        );
        let texels = |l: f64| (l * f.dpr).round().max(1.0).min(usize::MAX as f64) as usize;
        (f.w, f.h) = (texels(lw), texels(lh));
        if lw >= 1.0 && lh >= 1.0 {
            let x = [(f.w as f64 / lw) as f32, (f.h as f64 / lh) as f32, 0.0, 0.0];
            f.xf = (x != [1.0, 1.0, 0.0, 0.0]).then_some(x);
            f.rect = Some([lw, lh]);
            f.own = f.xf;
            f.over = Some([0.0, 0.0, lw, lh]);
        }
        f
    }

    /// A layer's texture as Qt's render target, when nothing but its own scale scales what is
    /// drawn now: the rectangle of its projection, which is the source rect's, and its
    /// devicePixelRatio, the larger of the texels to the unit along x and along y
    /// (QSGRhiLayer::grab).
    pub fn layer_target(&self) -> Option<([f32; 2], f32)> {
        let [lw, lh] = self.rect?;
        let (sx, sy) = (self.w as f64 / lw, self.h as f64 / lh);
        let own = [sx as f32, sy as f32, 0.0, 0.0];
        (self.xf.unwrap_or([1.0, 1.0, 0.0, 0.0]) == own)
            .then(|| ([lw as f32, lh as f32], sx.max(sy) as f32))
    }

    /// Draws are placed by the frame's own scale alone (no item transform): they replay
    /// elsewhere by recomputing them at another scene offset.
    fn plain(&self) -> bool {
        match self.xf {
            None => true,
            Some(x) => self.dpr != 1.0 && self.at_screen_scale(x),
        }
    }

    /// `x` is the screen's scale and nothing else: the devicePixelRatio, or what a window
    /// whose pixels are not a whole number of times it has for it (`window_scale`).
    fn at_screen_scale(&self, x: [f32; 4]) -> bool {
        let dpr = self.dpr as f32;
        x == [dpr, dpr, 0.0, 0.0] || (self.whole && Some(x) == self.own)
    }

    /// The scale of a window whose pixels are not a whole number of times its
    /// devicePixelRatio (`window_scale`), when it differs from that ratio: Qt has it in the
    /// projection, past what a vertex shader does with the ratio.
    fn past_snap(&self) -> Option<[f32; 2]> {
        let dpr = self.dpr as f32;
        match self.own {
            Some([kx, ky, ..]) if self.whole && [kx, ky] != [dpr, dpr] => Some([kx, ky]),
            _ => None,
        }
    }

    /// The scene transform (sx sy ox oy, device = s * p + o), the frame's scale included.
    pub fn xform(&self) -> [f64; 4] {
        self.xf.unwrap_or([1.0, 1.0, 0.0, 0.0]).map(|v| v as f64)
    }

    /// `rect` (x y w h) through `xf`.
    fn xf_rect(&self, r: [f64; 4]) -> [f64; 4] {
        match self.xf {
            Some([sx, sy, ox, oy]) => {
                let (sx, sy, ox, oy) = (sx as f64, sy as f64, ox as f64, oy as f64);
                [sx * r[0] + ox, sy * r[1] + oy, sx * r[2], sy * r[3]]
            }
            None => r,
        }
    }

    /// Pushes every shared texture the frame draws, nested layers included.
    pub fn shared_texs(&self, out: &mut Vec<u64>) {
        self.with_shared(|t| out.extend_from_slice(t));
    }

    /// Whether the shared image `t` is drawn, here or in a layer.
    pub fn draws(&self, t: &Rc<Canvas>) -> bool {
        self.draws_shared(t.key.get())
    }

    fn draws_shared(&self, t: u64) -> bool {
        self.with_shared(|ts| ts.contains(&t))
    }

    fn with_shared<R>(&self, f: impl FnOnce(&[u64]) -> R) -> R {
        let mut m = self.shared.borrow_mut();
        if m.as_ref().is_none_or(|(n, _)| *n != self.cmds.len()) {
            let mut v = Vec::new();
            self.collect_shared(&mut v);
            *m = Some((self.cmds.len(), v));
        }
        f(&m.as_ref().expect("memo").1)
    }

    fn collect_shared(&self, out: &mut Vec<u64>) {
        for c in &self.cmds {
            match c {
                Cmd::Tex { src: rc, .. } => out.push(rc.key.get()),
                Cmd::Masked { src, mask, .. } => {
                    src.collect_shared(out);
                    mask.collect_shared(out);
                }
                Cmd::Effect { src, mask, .. } => {
                    src.collect_shared(out);
                    if let Some(m) = mask {
                        m.collect_shared(out);
                    }
                }
                Cmd::Layer { src, .. } | Cmd::Tint { src, .. } | Cmd::Gauss { src, .. } => {
                    src.collect_shared(out)
                }
                Cmd::Radial { mask: Some(m), .. } => m.collect_shared(out),
                _ => {}
            }
        }
    }

    /// Points every top-level draw of the shared image `old` at `new` (same geometry), so a
    /// recorded frame can be shown again with a swapped texture. False when `old` isn't drawn.
    pub fn swap_tex(&mut self, old: &Rc<Canvas>, new: &Rc<Canvas>) -> bool {
        if !self.draws_shared(old.key.get()) {
            return false;
        }
        let mut found = false;
        for (c, m) in self.cmds.iter_mut().zip(&self.marks) {
            let hit = match c {
                Cmd::Tex { src: rc, .. } if Rc::ptr_eq(rc, old) => {
                    *rc = new.clone();
                    true
                }
                // Layer textures are cached by their frame's digest, which the swap changes.
                Cmd::Masked { src, mask, .. } => {
                    Rc::make_mut(src).swap_tex(old, new) | Rc::make_mut(mask).swap_tex(old, new)
                }
                Cmd::Effect { src, mask, .. } => {
                    Rc::make_mut(src).swap_tex(old, new)
                        | mask
                            .as_mut()
                            .is_some_and(|m| Rc::make_mut(m).swap_tex(old, new))
                }
                Cmd::Layer { src, .. } | Cmd::Tint { src, .. } | Cmd::Gauss { src, .. } => {
                    Rc::make_mut(src).swap_tex(old, new)
                }
                Cmd::Radial { mask: Some(m), .. } => Rc::make_mut(m).swap_tex(old, new),
                _ => false,
            };
            if hit {
                if let Some(&j) = m.as_ref() {
                    self.sig[j].0 = mix(self.sig[j].0, new.key.get());
                }
                found = true;
            }
        }
        for op in &mut self.ops {
            if let Op::Image(t, ..) = op
                && Rc::ptr_eq(t, old)
            {
                *t = new.clone();
            }
        }
        if let Some((_, ts)) = self.shared.get_mut() {
            for t in ts.iter_mut().filter(|t| **t == old.key.get()) {
                *t = new.key.get();
            }
        }
        if found {
            let o = Rc::as_ptr(old) as *const ();
            self.keep.retain(|k| Rc::as_ptr(k) as *const () != o);
            self.keep.push(new.clone());
        }
        found
    }

    /// Records one primitive's signature: its hash (with the active clip) and pixel bounds.
    fn mark(&mut self, h: u64, b: [f32; 4]) -> Option<usize> {
        let mut r = [
            b[0].floor() as i64 - 1,
            b[1].floor() as i64 - 1,
            b[2].ceil() as i64 + 1,
            b[3].ceil() as i64 + 1,
        ];
        let mut h = h;
        if let Some(c) = self.clips.last() {
            r = intersect(r, *c);
            h = c.iter().fold(h, |h, v| mix(h, *v as u64));
        }
        r = intersect(r, [0, 0, self.w as i64, self.h as i64]);
        if r[2] > r[0] && r[3] > r[1] {
            self.sig
                .push((mix(h, r.iter().fold(0, |h, v| mix(h, *v as u64))), r));
            return Some(self.sig.len() - 1);
        }
        None
    }

    /// How many primitives are drawn so far (see `drawn_over`).
    pub fn drawn(&self) -> usize {
        self.sig.len()
    }

    /// Whether a primitive drawn after the first `from` reaches into `r` (x1 y1 x2 y2).
    pub fn drawn_over(&self, from: usize, r: [i64; 4]) -> bool {
        // `mark` keeps a pixel around each primitive's bounds.
        self.sig
            .iter()
            .skip(from)
            .any(|(_, s)| s[0] + 1 < r[2] && s[2] - 1 > r[0] && s[1] + 1 < r[3] && s[3] - 1 > r[1])
    }

    /// A clip is being drawn under.
    pub fn clipped(&self) -> bool {
        !self.clips.is_empty()
    }

    fn push(&mut self, c: Cmd, b: [f32; 4], h: u64) {
        self.cmds.push(c);
        self.boxes.push(b);
        let m = self.mark(h, b);
        self.marks.push(m);
    }

    /// Hash of everything this frame draws (for nested layers).
    fn digest(&self) -> u64 {
        let h = mix(self.w as u64, self.h as u64);
        self.sig.iter().fold(h, |h, (x, r)| {
            r.iter().fold(mix(h, *x), |h, v| mix(h, *v as u64))
        })
    }

    /// Nested clips intersect, as QSGBatchRenderer's scissor stack does.
    pub fn push_clip(&mut self, r: [i64; 4]) {
        self.taint();
        self.push_scissor(r);
    }

    /// The place of what is drawn until `pop_place`: an item at `own` in a parent whose
    /// origin is at `origin` in this frame, with a clip node when it clips (`Place::step`).
    pub fn push_place(&mut self, origin: [f64; 2], own: [f64; 2], clip: bool) {
        let at = self.place().at;
        self.place_by([origin[0] - at[0], origin[1] - at[1]], own, clip);
    }

    fn place_by(&mut self, extra: [f64; 2], own: [f64; 2], clip: bool) {
        let op = self.plain().then(|| {
            self.ops.push(Op::Place(extra, own, clip));
            self.ops.len() - 1
        });
        self.places.push((self.place().step(extra, own, clip), op));
    }

    pub fn pop_place(&mut self) {
        match self.places.pop() {
            // Nothing was drawn there: the pair is not kept.
            Some((_, Some(k))) if k + 1 == self.ops.len() => {
                self.ops.pop();
            }
            Some((_, Some(_))) => self.ops.push(Op::Unplace),
            _ => {}
        }
    }

    /// The innermost place; Qt's root node, where the projection's rect has its origin, when
    /// none is pushed.
    pub fn place(&self) -> Place {
        match self.places.last() {
            Some(p) => p.0,
            None => Place::origin(self.over.map_or([0.0; 2], |o| [-o[0], -o[1]])),
        }
    }

    /// Clip to a `clip: true` item's scene rect (x y w h) with QSG's scissor of it.
    pub fn push_clip_f(&mut self, r: [f64; 4]) {
        if self.plain() {
            self.ops.push(Op::Clip(r));
        } else {
            self.taint();
        }
        let (w, h) = (r[2].max(0.0), r[3].max(0.0));
        let s = match self.xf {
            // The frame's pixels are not the scene's: the rect goes through the scale and is
            // rounded in pixels, not rounded in the scene and scaled.
            Some([sx, sy, ox, oy]) => qsg::raster::scissor_rect_scaled(
                r[0] as f32 + ox / sx,
                r[1] as f32 + oy / sy,
                w,
                h,
                [self.w as f32 / sx, self.h as f32 / sy],
                self.w,
                self.h,
            ),
            None => qsg::raster::scissor_rect(r[0] as f32, r[1] as f32, w, h, self.lw, self.lh),
        };
        self.push_device_scissor(s);
    }

    fn push_scissor(&mut self, r: [i64; 4]) {
        let r = match self.xf {
            Some([sx, sy, ox, oy]) => [
                (sx * r[0] as f32 + ox).round() as i64,
                (sy * r[1] as f32 + oy).round() as i64,
                (sx * r[2] as f32 + ox).round() as i64,
                (sy * r[3] as f32 + oy).round() as i64,
            ],
            None => r,
        };
        self.push_device_scissor(r);
    }

    /// `r` (x1 y1 x2 y2) in the frame's pixels.
    fn push_device_scissor(&mut self, r: [i64; 4]) {
        let r = match self.clips.last() {
            Some(p) => [
                r[0].max(p[0]),
                r[1].max(p[1]),
                r[2].min(p[2]),
                r[3].min(p[3]),
            ],
            None => r,
        };
        self.clips.push(r);
        self.cmds.push(Cmd::Scissor(Some(r)));
        self.boxes.push(ANY_BOX);
        self.marks.push(None);
    }

    pub fn pop_clip(&mut self) {
        if self.plain() {
            self.ops.push(Op::Unclip);
        } else {
            self.taint();
        }
        self.clips.pop();
        self.cmds.push(Cmd::Scissor(self.clips.last().copied()));
        self.boxes.push(ANY_BOX);
        self.marks.push(None);
    }

    /// The rect Qt's projection is over, while nothing but the frame's own scale places what
    /// is drawn: then a draw is Qt's, in scene units under the projection over this rect.
    pub fn projected(&self) -> Option<[f64; 4]> {
        (self.xf == self.own).then(|| {
            let (w, h) = self.units();
            self.over.unwrap_or([0.0, 0.0, w, h])
        })
    }

    pub fn smooth(&mut self, g: &Rc<Geometry>, tx: f64, ty: f64, opacity: f32) {
        if self.plain() {
            self.ops.push(Op::Smooth(g.clone(), tx, ty, opacity));
        } else {
            self.opaque += 1;
        }
        let (w, h) = (self.w as f32, self.h as f32);
        let v = match self.projected() {
            // The item is only moved, by where it is in that rect's item.
            Some(over) => {
                let scene = [tx + over[0], ty + over[1]];
                match self.places.last() {
                    // The item itself, where Qt's renderer has it to the last bit.
                    Some((p, _)) if p.is_at(scene) => {
                        qsg::raster::smooth_clip_placed(g, p, opacity, over, w, h)
                    }
                    _ => {
                        let (tx, ty) = (scene[0] as f32, scene[1] as f32);
                        qsg::raster::smooth_clip_over(g, tx, ty, opacity, over, w, h)
                    }
                }
            }
            None => {
                let [sx, sy, ox, oy] = self.xf.unwrap_or([1.0, 1.0, 0.0, 0.0]);
                let (tx, ty) = (tx as f32, ty as f32);
                qsg::raster::smooth_clip_scaled(
                    g,
                    sx,
                    sy,
                    sx * tx + ox,
                    sy * ty + oy,
                    opacity,
                    w,
                    h,
                )
            }
        };
        self.smooth_tris(g, v);
    }

    /// `smooth` through the matrix `m` of an item (m11 m12 m21 m22 dx dy, to the scene).
    pub fn smooth_matrix(&mut self, g: &Geometry, m: [f32; 6], opacity: f32) {
        self.opaque += 1;
        let (w, h) = (self.w as f32, self.h as f32);
        let v = match self.projected() {
            Some(over) => {
                let m = [
                    m[0],
                    m[1],
                    m[2],
                    m[3],
                    m[4] + over[0] as f32,
                    m[5] + over[1] as f32,
                ];
                qsg::raster::smooth_clip_matrix_over(g, m, opacity, over, w, h)
            }
            None => {
                let [sx, sy, ox, oy] = self.xf.unwrap_or([1.0, 1.0, 0.0, 0.0]);
                let m = [
                    sx * m[0],
                    sy * m[1],
                    sx * m[2],
                    sy * m[3],
                    sx * m[4] + ox,
                    sy * m[5] + oy,
                ];
                qsg::raster::smooth_clip_matrix(g, m, opacity, w, h)
            }
        };
        self.smooth_tris(g, v);
    }

    fn smooth_tris(&mut self, g: &Geometry, v: Vec<([f32; 2], [f32; 4])>) {
        if !matches!(self.cmds.last(), Some(Cmd::Tris(_))) {
            self.cmds.push(Cmd::Tris(Vec::new()));
            self.boxes.push([f32::MAX, f32::MAX, f32::MIN, f32::MIN]);
            self.marks.push(None);
        }
        let at = self.cmds.len() - 1;
        let Some(Cmd::Tris(runs)) = self.cmds.last_mut() else {
            return;
        };
        // A run shared with a recording, or sealed by one starting, stays as it is.
        let sealed = self.seal.get() == (at, runs.len());
        if sealed || runs.last_mut().and_then(Rc::get_mut).is_none() {
            runs.push(Rc::new(Vec::new()));
        }
        let Some(out) = runs.last_mut().and_then(Rc::get_mut) else {
            return;
        };
        let start = out.len();
        out.reserve(g.indices.len().saturating_sub(2) * 18);
        for t in qsg::raster::smooth_triangles(g) {
            for k in t {
                let (p, c) = v[k];
                out.extend_from_slice(&[p[0], p[1], c[0], c[1], c[2], c[3]]);
            }
        }
        let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
        let (w, h) = (self.w as f32, self.h as f32);
        for vx in out[start..].as_chunks::<6>().0 {
            let [x, y] = qsg::raster::viewport([vx[0], vx[1]], w, h);
            b = [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)];
        }
        if b[0] > b[2] {
            return;
        }
        let h = mix_f(1, &out[start..]);
        let last = self.boxes.last_mut().expect("tris box");
        *last = [
            last[0].min(b[0]),
            last[1].min(b[1]),
            last[2].max(b[2]),
            last[3].max(b[3]),
        ];
        self.mark(h, b);
    }

    /// NativeRendering glyphs at node-local positions `quads`, node at `tx`, `ty`, snapped to
    /// device pixels like `qsg::text::FontStack::line_quads`.
    pub fn glyphs(
        &mut self,
        quads: Rc<[GlyphQuadF]>,
        tx: f64,
        ty: f64,
        color: [u8; 4],
        opacity: f32,
    ) {
        if self.dpr != 1.0 {
            return self.glyphs_f(quads.to_vec(), tx, ty, color, opacity);
        }
        let [ax, ay] = self.against;
        let snapped = quads
            .iter()
            .map(|(g, lx, ly)| {
                let x0 = qsg::text::snap_px((lx + tx) as f32, ax) as i64;
                let y0 = qsg::text::snap_px((ly + ty) as f32, ay) as i64;
                (g.clone(), x0, y0)
            })
            .collect();
        if self.plain() {
            self.ops.push(Op::Glyphs(quads, tx, ty, color, opacity));
        } else {
            self.opaque += 1;
        }
        self.glyphs_snapped(snapped, color, opacity);
    }

    fn glyphs_snapped(&mut self, quads: Vec<GlyphQuad>, color: [u8; 4], opacity: f32) {
        let a = color[3] as f32 / 255.0 * opacity;
        let c = [
            color[0] as f32 / 255.0 * a,
            color[1] as f32 / 255.0 * a,
            color[2] as f32 / 255.0 * a,
            a,
        ];
        if !quads.is_empty() {
            let mut h = mix_f(2, &c);
            let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
            for (g, x, y) in &quads {
                h = mix(mix(mix(h, g.key.get()), *x as u64), *y as u64);
                let (x, y) = (*x as f32, *y as f32);
                b = [
                    b[0].min(x),
                    b[1].min(y),
                    b[2].max(x + g.width as f32),
                    b[3].max(y + g.rows as f32),
                ];
                self.keep.push(g.clone());
            }
            self.push(Cmd::Glyphs(c, quads.into()), b, h);
        }
    }

    /// NativeRendering glyphs (node-local positions, node at `tx`, `ty`) through `xf`, snapped
    /// like textmask.vert: modelView * local, then floor(p + 0.5); sampled nearest.
    pub fn glyphs_f(
        &mut self,
        quads: Vec<GlyphQuadF>,
        tx: f64,
        ty: f64,
        color: [u8; 4],
        opacity: f32,
    ) {
        self.taint();
        let [sx, sy, ox, oy] = self.xf.unwrap_or([1.0, 1.0, 0.0, 0.0]);
        let (ox, oy) = ((sx as f64 * tx) as f32 + ox, (sy as f64 * ty) as f32 + oy);
        let a = color[3] as f32 / 255.0 * opacity;
        let c = [
            color[0] as f32 / 255.0 * a,
            color[1] as f32 / 255.0 * a,
            color[2] as f32 / 255.0 * a,
            a,
        ];
        let mut h = mix_f(9, &c);
        let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
        let mut q = Vec::with_capacity(quads.len());
        for (g, x, y) in quads {
            let (x, y) = (x as f32, y as f32);
            let (x2, y2) = (x + g.width as f32, y + g.rows as f32);
            let [ax, ay] = self.against;
            let r = [
                qsg::text::snap_px(sx * x + ox, ax),
                qsg::text::snap_px(sy * y + oy, ay),
                qsg::text::snap_px(sx * x2 + ox, ax),
                qsg::text::snap_px(sy * y2 + oy, ay),
            ];
            h = mix_f(mix(h, g.key.get()), &r);
            b = [
                b[0].min(r[0]),
                b[1].min(r[1]),
                b[2].max(r[2]),
                b[3].max(r[3]),
            ];
            self.keep.push(g.clone());
            q.push((g, r));
        }
        if !q.is_empty() {
            self.push(Cmd::GlyphsF(c, q.into()), b, h);
        }
    }

    /// NativeRendering glyphs of `line` on this frame's device pixels: `st` is the line's
    /// stack `scaled(self.dpr)`, `at` the node (x y) and pen (x y) position in scene units.
    /// Translatable: a moved segment re-places and re-snaps them like a fresh draw.
    pub fn text_dpr(
        &mut self,
        st: Rc<qsg::text::FontStack>,
        line: Rc<qsg::text::ShapedLine>,
        at: [f64; 4],
        color: [u8; 4],
        opacity: f32,
    ) {
        let against = self.against;
        let xf = self.xform();
        let mut q = st.place_dpr(&line, at[0], at[1], at[2], at[3], xf, against, self.dpr);
        if let Some([kx, ky]) = self.past_snap() {
            // textmask.vert snaps a vertex to a pixel of the devicePixelRatio before the
            // projection, which in such a window is over a little more or less.
            let dpr = self.dpr;
            let [sx, sy, ox, oy] = self.xform();
            let (kx, ky) = (kx as f64, ky as f64);
            let view = [sx / kx * dpr, sy / ky * dpr, ox / kx * dpr, oy / ky * dpr];
            q = st.place_dpr(&line, at[0], at[1], at[2], at[3], view, against, dpr);
            let (kx, ky) = ((kx / dpr) as f32, (ky / dpr) as f32);
            for (_, r) in &mut q {
                *r = [r[0] * kx, r[1] * ky, r[2] * kx, r[3] * ky];
            }
        }
        if self.plain() {
            self.ops.push(Op::GlyphsDpr(st, line, at, color, opacity));
        } else {
            self.taint();
        }
        self.push_glyph_rects(q, color, opacity);
    }

    /// NativeRendering glyphs rasterized for this frame's `dpr`, on device rects
    /// (`qsg::text::FontStack::line_rects_dpr`).
    pub fn glyphs_dpr(&mut self, q: Vec<qsg::text::GlyphRect>, color: [u8; 4], opacity: f32) {
        self.taint();
        self.push_glyph_rects(q, color, opacity);
    }

    fn push_glyph_rects(&mut self, q: Vec<qsg::text::GlyphRect>, color: [u8; 4], opacity: f32) {
        let a = color[3] as f32 / 255.0 * opacity;
        let c = [
            color[0] as f32 / 255.0 * a,
            color[1] as f32 / 255.0 * a,
            color[2] as f32 / 255.0 * a,
            a,
        ];
        let mut h = mix_f(11, &c);
        let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
        for (g, r) in &q {
            h = mix_f(mix(h, g.key.get()), r);
            b = [
                b[0].min(r[0]),
                b[1].min(r[1]),
                b[2].max(r[2]),
                b[3].max(r[3]),
            ];
            self.keep.push(g.clone());
        }
        if !q.is_empty() {
            self.push(Cmd::GlyphsF(c, q.into()), b, h);
        }
    }

    /// The screen's scale when nothing else scales what is drawn now: the frame is then a
    /// render target of Qt's with this device pixel ratio.
    pub fn screen_scale(&self) -> Option<f32> {
        let dpr = self.dpr as f32;
        match self.xf {
            None => Some(1.0),
            Some(x) => self.at_screen_scale(x).then_some(dpr),
        }
    }

    /// Tiles are placed in device pixels (already through `xf`, see `line_df_tiles`).
    pub fn df_tiles(&mut self, tiles: Vec<qsg::dftext::DfTile>, k: Option<[u8; 3]>) {
        self.taint();
        if !tiles.is_empty() {
            let k = k.map(|k| k.map(|c| c as f32 / 255.0));
            let mut h = mix_f(mix(3, k.is_none() as u64), &k.unwrap_or_default());
            let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
            for (t, x, y) in &tiles {
                h = mix(mix(mix(h, t.key.get()), *x as u64), *y as u64);
                let (x, y) = (*x as f32, *y as f32);
                b = [
                    b[0].min(x),
                    b[1].min(y),
                    b[2].max(x + t.w as f32),
                    b[3].max(y + t.h as f32),
                ];
                self.keep.push(t.clone());
            }
            self.push(Cmd::Df(k, tiles.into()), b, h);
        }
    }

    /// Image texture (shared with the image cache) on `rect`, sampling `uv`.
    pub fn image(&mut self, tex: Rc<Canvas>, rect: [f64; 4], uv: [f64; 4], op: f32, smooth: bool) {
        self.image_filtered(tex, rect, uv, op, smooth, false, None)
    }

    /// [`Frame::image`] of an Image with `mipmap: true`: shrunk, it is read through the
    /// texture's chain of halved levels (GL_LINEAR_MIPMAP_LINEAR, or GL_NEAREST_MIPMAP_LINEAR
    /// when not `smooth`), as QSGTexture::Linear mipmap filtering has it.
    pub fn image_mipmapped(
        &mut self,
        tex: Rc<Canvas>,
        rect: [f64; 4],
        uv: [f64; 4],
        op: f32,
        smooth: bool,
    ) {
        self.image_filtered(tex, rect, uv, op, smooth, true, None)
    }

    /// [`Frame::image`] of a Kirigami.Icon with `isMask` (icon.frag, ENABLE_MASK): what the
    /// filter gives of the texture is tinted, `vec4(mask_color.xyz * color.a, color.a)` with
    /// the premultiplied `color`.
    pub fn image_mask(
        &mut self,
        tex: Rc<Canvas>,
        rect: [f64; 4],
        uv: [f64; 4],
        op: f32,
        smooth: bool,
        color: [u8; 4],
    ) {
        self.image_filtered(tex, rect, uv, op, smooth, false, Some(color))
    }

    #[allow(clippy::too_many_arguments)]
    fn image_filtered(
        &mut self,
        tex: Rc<Canvas>,
        rect: [f64; 4],
        uv: [f64; 4],
        op: f32,
        smooth: bool,
        mipmap: bool,
        tint: Option<[u8; 4]>,
    ) {
        if self.plain() {
            let op = Op::Image(tex.clone(), rect, uv, op, smooth, mipmap, tint);
            self.ops.push(op);
        } else {
            self.opaque += 1;
        }
        let rect = self.xf_rect(rect);
        let (rect, uv) = (rect.map(|v| v as f32), uv.map(|v| v as f32));
        let h = mix_f(mix_f(mix(4, tex.key.get()), &rect), &uv);
        let h = mix(mix_f(h, &[op]), smooth as u64 | (mipmap as u64) << 1);
        let (h, k, mode) = match tint {
            Some(c) => {
                let [r, g, b, a] = c.map(|v| v as f32 / 255.0);
                let h = mix(h, 1 + u32::from_le_bytes(c) as u64);
                (h, [r * a, g * a, b * a, 1.0], MASK)
            }
            None => (h, [0.0; 4], PLAIN),
        };
        self.keep.push(tex.clone());
        let b = [rect[0], rect[1], rect[0] + rect[2], rect[1] + rect[3]];
        self.push(
            Cmd::Tex {
                src: tex,
                rect,
                uv,
                k,
                op,
                mode,
                smooth,
                mipmap,
            },
            b,
            h,
        );
    }

    /// `image` for a picture with no alpha channel that nothing makes translucent: Qt Quick
    /// draws it with QSGOpaqueTextureMaterial, blending off, so the window gets each texel as
    /// the picture has it. That differs from `image` where a texel's alpha is not 255 (the
    /// pixels QGifHandler cleared in a Format_RGB32 frame).
    pub fn image_unblended(&mut self, tex: Rc<Canvas>, rect: [f64; 4], uv: [f64; 4], smooth: bool) {
        self.taint();
        let rect = self.xf_rect(rect);
        let (rect, uv) = (rect.map(|v| v as f32), uv.map(|v| v as f32));
        let h = mix_f(mix_f(mix(11, tex.key.get()), &rect), &uv);
        let h = mix(h, smooth as u64);
        self.keep.push(tex.clone());
        let b = [rect[0], rect[1], rect[0] + rect[2], rect[1] + rect[3]];
        self.push(
            Cmd::Tex {
                src: tex,
                rect,
                uv,
                k: [0.0; 4],
                op: 1.0,
                mode: UNBLENDED,
                smooth,
                mipmap: false,
            },
            b,
            h,
        );
    }

    /// Qt5Compat ColorOverlay (`desat` None) or Desaturate of the GPU layer `src` over `rect`:
    /// the source stays on the GPU instead of a CPU raster uploaded every repaint.
    /// `size` is the source item's: its layer is ceil-sized and stretches over `rect` whole.
    /// `smooth` is the source's `smooth` (its layer's for a `layer.effect`): both effects set
    /// their SourceProxy to linear interpolation for a smooth input and to nearest otherwise.
    #[allow(clippy::too_many_arguments)]
    pub fn tint(
        &mut self,
        src: Frame,
        size: [f64; 2],
        color: [u16; 4],
        desat: Option<f32>,
        smooth: bool,
        rect: [f64; 4],
        op: f32,
    ) {
        self.taint();
        let crop = if !src.plain() {
            [1.0, 1.0]
        } else {
            [
                (size[0] * src.dpr / src.w.max(1) as f64).min(1.0) as f32,
                (size[1] * src.dpr / src.h.max(1) as f64).min(1.0) as f32,
            ]
        };
        let rect = self.xf_rect(rect).map(|v| v as f32);
        let (k, mode) = match desat {
            Some(d) => ([d, 0.0, 0.0, 0.0], DESAT),
            None => (qsg::shadow::shader_color(color), OVERLAY),
        };
        let h = mix_f(mix_f(mix(9, src.digest()), &k), &[mode, crop[0], crop[1]]);
        let h = mix(
            mix_f(h, &[rect[0], rect[1], rect[2], rect[3], op]),
            smooth as u64,
        );
        self.keep.extend(src.keep.iter().cloned());
        let b = [rect[0], rect[1], rect[0] + rect[2], rect[1] + rect[3]];
        self.push(
            Cmd::Tint {
                src: Rc::new(src),
                rect,
                crop,
                k,
                mode,
                smooth,
                op,
            },
            b,
            h,
        );
    }

    /// The GPU layer `src`, all of it, over `rect` (x y w h): the image node of a
    /// QQuickShaderEffectSource, read linearly when `smooth` and nearest otherwise.
    pub fn source(&mut self, src: Frame, smooth: bool, rect: [f64; 4], op: f32) {
        self.taint();
        let rect = self.xf_rect(rect).map(|v| v as f32);
        let h = mix_f(mix(9, src.digest()), &[PLAIN]);
        let h = mix(
            mix_f(h, &[rect[0], rect[1], rect[2], rect[3], op]),
            smooth as u64,
        );
        self.keep.extend(src.keep.iter().cloned());
        let b = [rect[0], rect[1], rect[0] + rect[2], rect[1] + rect[3]];
        self.push(
            Cmd::Tint {
                src: Rc::new(src),
                rect,
                crop: [1.0, 1.0],
                k: [0.0; 4],
                mode: PLAIN,
                smooth,
                op,
            },
            b,
            h,
        );
    }

    /// `layer.effect: OpacityMask`: both layers render offscreen on the GPU (source sampled
    /// nearest like the item layer, mask linear like its ShaderEffectSource).
    ///
    /// With a nonzero `id`, a source drawn 1:1 at whole pixels is damaged per changed draw, not
    /// as one rect, and its layer is updated in place (`Renderer::inc_layer_tex`).
    #[allow(clippy::too_many_arguments)]
    pub fn masked(
        &mut self,
        src: Frame,
        mask: Frame,
        rect: [f64; 4],
        invert: bool,
        op: f32,
        id: u64,
    ) {
        self.masked_rc(Rc::new(src), Rc::new(mask), rect, invert, op, id);
    }

    /// A MultiEffect over `rect` (x y w h), sampling `src`/`mask` stretched across it.
    #[allow(clippy::too_many_arguments)]
    pub fn effect(
        &mut self,
        src: Frame,
        mask: Option<Frame>,
        mask_smooth: bool,
        rect: [f64; 4],
        u: [f32; 12],
        op: f32,
        blur: Option<Blur>,
        shadow: Option<qsg::multieffect::Shadow>,
    ) {
        self.taint();
        let rect = self.xf_rect(rect).map(|v| v as f32);
        let h = mix(
            mix(8, src.digest()),
            mask.as_ref().map_or(0, |m| m.digest()),
        );
        let mut h = mix_f(mix_f(h, &u), &[rect[0], rect[1], rect[2], rect[3], op]);
        h = mix(h, (mask.is_some() && mask_smooth) as u64);
        if let Some(b) = &blur {
            h = mix_f(mix(h, b.sizes.len() as u64), &b.weights);
            h = mix_f(h, &[b.multiplier, b.sizes[0].0 as f32, b.sizes[0].1 as f32]);
            h = mix(h, b.dpr.to_bits());
        }
        if let Some(s) = &shadow {
            h = mix_f(mix_f(mix(h, 1), &s.color), &s.weights);
            h = mix_f(h, &[s.scale, s.offset[0], s.offset[1], s.center]);
        }
        self.keep.extend(src.keep.iter().cloned());
        if let Some(m) = &mask {
            self.keep.extend(m.keep.iter().cloned());
        }
        let b = [rect[0], rect[1], rect[0] + rect[2], rect[1] + rect[3]];
        let (src, mask) = (Rc::new(src), mask.map(Rc::new));
        self.push(
            Cmd::Effect {
                src,
                mask,
                mask_smooth,
                rect,
                u,
                op,
                blur: blur.map(Rc::new),
                shadow,
            },
            b,
            h,
        );
    }

    /// Qt5Compat GaussianBlur/DropShadow over `rect` (x y w h): `src` (the effect's SourceProxy
    /// texture) blurred by `g`'s two passes on the GPU, as Qt's ShaderEffects: the horizontal
    /// one into a layer of `size` texels, the vertical one over `rect`.
    ///
    /// `node` is the effect's own rect and its node's matrix (QSGNode's combined one, to the
    /// item this frame draws from) when the vertical pass is the effect itself drawn into
    /// this frame, as in GaussianBlur.qml: its vertices then go through that matrix and Qt's
    /// projection, in single floats, as Qt's do. It is left out when the matrix does not put
    /// the rect where `rect` is.
    pub fn gauss(
        &mut self,
        src: Frame,
        g: Gauss,
        size: (usize, usize),
        rect: [f64; 4],
        node: Option<([f32; 4], Mat4)>,
        op: f32,
    ) {
        self.taint();
        let rect = self.xf_rect(rect).map(|v| v as f32);
        let qt = node.and_then(|(item, mv)| self.gauss_at(item, &mv, rect));
        let h = mix(
            mix(mix(mix(10, src.digest()), g.digest()), size.0 as u64),
            size.1 as u64,
        );
        let h = mix_f(h, &[rect[0], rect[1], rect[2], rect[3], op]);
        self.keep.extend(src.keep.iter().cloned());
        let b = [rect[0], rect[1], rect[0] + rect[2], rect[1] + rect[3]];
        self.push(
            Cmd::Gauss {
                src: Rc::new(src),
                g: Rc::new(g),
                size,
                rect,
                qt,
                op,
            },
            b,
            h,
        );
    }

    /// The fill `tris` of a CurveRenderer Shape at scene `x`, `y`, in `color`, drawn by Qt's
    /// shader straight into the target, each triangle blended as it comes. Only where nothing
    /// but the frame's own scale places the item (`projected`): elsewhere nothing is drawn.
    pub fn curve_fill(
        &mut self,
        tris: &[qsg::shape::FillTri],
        x: f64,
        y: f64,
        color: [u8; 4],
        op: f32,
    ) {
        let Some(over) = self.projected() else {
            return;
        };
        self.taint();
        let at = [(x + over[0]) as f32, (y + over[1]) as f32];
        let v = qsg::shape::fill_vertices(tris, over, at, self.dpr as f32);
        let (w, h) = (self.w as f32, self.h as f32);
        let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
        for vx in v.as_chunks::<10>().0 {
            let [x, y] = qsg::raster::viewport([vx[0], vx[1]], w, h);
            b = [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)];
        }
        // Nothing to draw, or a corner that is no number: the rasterizer drops such a triangle.
        if !b.iter().all(|c| c.is_finite()) || b[0] > b[2] {
            return;
        }
        // QColor::redF and the like: the byte over 255.
        let c = color.map(|c| c as f32 / 255.0);
        let hash = mix_f(mix_f(mix_f(17, &v), &c), &[op]);
        self.push(
            Cmd::Curve {
                v: Rc::new(v),
                c,
                op,
            },
            b,
            hash,
        );
    }

    /// Qt's matrix of an effect of the rect `item` whose node has the matrix `mv`, drawn into
    /// this frame: QSGRenderer::setProjectionMatrixToRect's of the rect the frame is over
    /// times `mv`. None unless it puts `item` over `rect` (device pixels).
    fn gauss_at(&self, item: [f32; 4], mv: &Mat4, rect: [f32; 4]) -> Option<GaussAt> {
        let [l, t, w, h] =
            self.over
                .unwrap_or([0.0, 0.0, self.w as f64 / self.dpr, self.h as f64 / self.dpr]);
        let p = Mat4::ortho(
            l as f32,
            (l + w) as f32,
            (t + h) as f32,
            t as f32,
            1.0,
            -1.0,
        );
        let m = p.mul(mv);
        let (tw, th) = (self.w as f32, self.h as f32);
        let px = |x: f32, y: f32| {
            let v = m.map3([x, y, 0.0]);
            [(v[0] + 1.0) * 0.5 * tw, (1.0 - v[1]) * 0.5 * th]
        };
        let (a, b) = (
            px(item[0], item[1]),
            px(item[0] + item[2], item[1] + item[3]),
        );
        let near = |a: f32, b: f32| (a - b).abs() < 0.01;
        let fits = near(a[0], rect[0])
            && near(a[1], rect[1])
            && near(b[0], rect[0] + rect[2])
            && near(b[1], rect[1] + rect[3]);
        fits.then(|| GaussAt {
            m: std::array::from_fn(|i| m.m[i / 4][i % 4]),
            item,
            target: (self.w as i32, self.h as i32),
        })
    }

    /// QtQuick.Effects RectangularShadow over the scene rect `rect`, computed per fragment
    /// from the shape uniforms `u` and corner radii `k` (`qsg::shadow::Shader`) and
    /// premultiplied colour `c`.
    pub fn rshadow(&mut self, rect: [f64; 4], u: [f32; 5], k: [f32; 4], c: [f32; 4], op: f32) {
        self.taint();
        let rect = self.xf_rect(rect).map(|v| v as f32);
        let h = mix_f(
            mix_f(mix_f(mix_f(13, &u), &k), &c),
            &[rect[0], rect[1], rect[2], rect[3], op],
        );
        let b = [rect[0], rect[1], rect[0] + rect[2], rect[1] + rect[3]];
        self.push(Cmd::RShadow { rect, u, k, c, op }, b, h);
    }

    /// Qt5Compat RadialGradient over the scene rect `rect`: the gradient texture `tex`
    /// (`qsg::gradient::gradient_texture`) read at `r`, times the alpha of the `source` item's
    /// layer `mask`, when it has one, stretched over the rect. `size` is that item's: its layer
    /// is ceil-sized.
    pub fn radial(
        &mut self,
        tex: Rc<Canvas>,
        r: qsg::gradient::Radial,
        mask: Option<(Frame, [f64; 2])>,
        rect: [f64; 4],
        node: Option<([f32; 4], Mat4)>,
        op: f32,
    ) {
        self.taint();
        let rect = self.xf_rect(rect).map(|v| v as f32);
        let qt = node.and_then(|(item, mv)| self.gauss_at(item, &mv, rect));
        let mut h = mix_f(
            mix(14, tex.key.get()),
            &[r.center[0], r.center[1], r.sin, r.cos],
        );
        h = mix_f(h, &[r.ratio[0], r.ratio[1]]);
        h = mix_f(h, &[rect[0], rect[1], rect[2], rect[3], op]);
        if let Some(q) = &qt {
            h = mix_f(mix_f(h, &q.m), &q.item);
        }
        let mut crop = [1.0, 1.0];
        let mask = mask.map(|(m, size)| {
            if m.plain() {
                crop = [
                    (size[0] * m.dpr / m.w.max(1) as f64).min(1.0) as f32,
                    (size[1] * m.dpr / m.h.max(1) as f64).min(1.0) as f32,
                ];
            }
            h = mix_f(mix(mix(h, 1), m.digest()), &crop);
            self.keep.extend(m.keep.iter().cloned());
            Rc::new(m)
        });
        self.keep.push(tex.clone());
        let b = [rect[0], rect[1], rect[0] + rect[2], rect[1] + rect[3]];
        let c = Cmd::Radial {
            tex,
            mask,
            crop,
            rect,
            qt,
            r,
            op,
        };
        self.push(c, b, h);
    }

    fn masked_rc(
        &mut self,
        src: Rc<Frame>,
        mask: Rc<Frame>,
        rect: [f64; 4],
        invert: bool,
        op: f32,
        id: u64,
    ) {
        if self.plain() {
            self.ops
                .push(Op::Masked(src.clone(), mask.clone(), rect, invert, op, id));
        } else {
            self.taint();
        }
        let inc = id != 0
            && self.xf.is_none()
            && rect[0].fract() == 0.0
            && rect[1].fract() == 0.0
            && rect[2] == src.w as f64
            && rect[3] == src.h as f64
            && src.w * src.h >= INC_MIN_AREA;
        // A scaled layer (`scene::gpu_layer_dpr`) already spans its whole texture.
        let crop = if !src.plain() {
            [1.0, 1.0]
        } else {
            [
                (rect[2] * src.dpr / src.w.max(1) as f64).min(1.0) as f32,
                (rect[3] * src.dpr / src.h.max(1) as f64).min(1.0) as f32,
            ]
        };
        let rect = self.xf_rect(rect);
        let rect = rect.map(|v| v as f32);
        let h = mix(mix(6, if inc { id } else { src.digest() }), mask.digest());
        let h = mix(mix(h, inc as u64), invert as u64);
        let h = mix_f(h, &[rect[0], rect[1], rect[2], rect[3], op]);
        self.keep.extend(src.keep.iter().cloned());
        self.keep.extend(mask.keep.iter().cloned());
        let b = [rect[0], rect[1], rect[0] + rect[2], rect[1] + rect[3]];
        let parts: Vec<(u64, [i64; 4])> = if inc {
            let (ox, oy) = (rect[0] as i64, rect[1] as i64);
            src.sig
                .iter()
                .map(|(k, r)| (mix(*k, h), [r[0] + ox, r[1] + oy, r[2] + ox, r[3] + oy]))
                .collect()
        } else {
            Vec::new()
        };
        self.push(
            Cmd::Masked {
                src,
                mask,
                rect,
                crop,
                invert,
                op,
                id: if inc { id } else { 0 },
            },
            b,
            h,
        );
        for (k, r) in parts {
            let (mut r, mut k) = (r, k);
            if let Some(c) = self.clips.last() {
                r = intersect(r, *c);
                k = c.iter().fold(k, |h, v| mix(h, *v as u64));
            }
            r = intersect(r, [0, 0, self.w as i64, self.h as i64]);
            if r[2] > r[0] && r[3] > r[1] {
                self.sig.push((k, r));
            }
        }
    }

    /// Item `rotation`/`scale`: the subtree's offscreen layer drawn through `m`.
    pub fn layer(&mut self, src: Frame, m: [f64; 6], op: f32) {
        self.taint();
        // `m` maps the layer's scene units; a layer drawn at its own `dpr` has finer texels.
        let k = 1.0 / src.dpr;
        let m = [m[0] * k, m[1] * k, m[2] * k, m[3] * k, m[4], m[5]];
        let m = match self.xf {
            Some([sx, sy, ox, oy]) => {
                let (sx, sy, ox, oy) = (sx as f64, sy as f64, ox as f64, oy as f64);
                [
                    m[0] * sx,
                    m[1] * sy,
                    m[2] * sx,
                    m[3] * sy,
                    m[4] * sx + ox,
                    m[5] * sy + oy,
                ]
            }
            None => m,
        };
        let m = m.map(|v| v as f32);
        let h = mix_f(mix_f(mix(7, src.digest()), &m), &[op]);
        self.keep.extend(src.keep.iter().cloned());
        let (tw, th) = (src.w as f32, src.h as f32);
        let mut c = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
        for (_, r) in &src.sig {
            if r[2] > r[0] && r[3] > r[1] {
                c = [
                    c[0].min(r[0] as f32),
                    c[1].min(r[1] as f32),
                    c[2].max(r[2] as f32),
                    c[3].max(r[3] as f32),
                ];
            }
        }
        if c[0] > c[2] {
            return;
        }
        // Margin for bilinear taps; transparent texels add nothing, so cropping is exact.
        let crop = [
            (c[0] - 2.0).max(0.0),
            (c[1] - 2.0).max(0.0),
            (c[2] + 2.0).min(tw),
            (c[3] + 2.0).min(th),
        ];
        let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
        for (u, v) in [
            (crop[0], crop[1]),
            (crop[2], crop[1]),
            (crop[0], crop[3]),
            (crop[2], crop[3]),
        ] {
            let (x, y) = (m[0] * u + m[2] * v + m[4], m[1] * u + m[3] * v + m[5]);
            b = [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)];
        }
        self.push(
            Cmd::Layer {
                src: Rc::new(src),
                m,
                p: [0.0, 0.0, 1.0],
                op,
                crop,
            },
            b,
            h,
        );
    }

    /// `layer` through a projective map: `t` is a QTransform's m11..m33 by rows (points as
    /// row vectors), from the layer's scene units to this frame's. An item under a Rotation
    /// about the x or y axis, or a Matrix4x4 with a perspective row.
    pub fn layer_projected(&mut self, src: Frame, t: [[f64; 3]; 3], op: f32) {
        self.taint();
        let k = 1.0 / src.dpr;
        // Texels to scene units, then this frame's own scale and offset on the result.
        let mut t = [t[0].map(|v| v * k), t[1].map(|v| v * k), t[2]];
        if let Some([sx, sy, ox, oy]) = self.xf {
            let (sx, sy, ox, oy) = (sx as f64, sy as f64, ox as f64, oy as f64);
            for r in &mut t {
                *r = [r[0] * sx + r[2] * ox, r[1] * sy + r[2] * oy, r[2]];
            }
        }
        let m = [t[0][0], t[0][1], t[1][0], t[1][1], t[2][0], t[2][1]].map(|v| v as f32);
        let p = [t[0][2], t[1][2], t[2][2]].map(|v| v as f32);
        let h = mix_f(mix_f(mix_f(mix(7, src.digest()), &m), &p), &[op]);
        self.keep.extend(src.keep.iter().cloned());
        let (tw, th) = (src.w as f32, src.h as f32);
        let mut c = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
        for (_, r) in &src.sig {
            if r[2] > r[0] && r[3] > r[1] {
                c = [
                    c[0].min(r[0] as f32),
                    c[1].min(r[1] as f32),
                    c[2].max(r[2] as f32),
                    c[3].max(r[3] as f32),
                ];
            }
        }
        if c[0] > c[2] {
            return;
        }
        let crop = [
            (c[0] - 2.0).max(0.0),
            (c[1] - 2.0).max(0.0),
            (c[2] + 2.0).min(tw),
            (c[3] + 2.0).min(th),
        ];
        let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
        for (u, v) in [
            (crop[0], crop[1]),
            (crop[2], crop[1]),
            (crop[0], crop[3]),
            (crop[2], crop[3]),
        ] {
            let w = p[0] * u + p[1] * v + p[2];
            if w <= PROJ_NEAR {
                // A corner behind the viewer shows towards infinity.
                b = [0.0, 0.0, self.w as f32, self.h as f32];
                break;
            }
            let (x, y) = (
                (m[0] * u + m[2] * v + m[4]) / w,
                (m[1] * u + m[3] * v + m[5]) / w,
            );
            b = [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)];
        }
        self.push(
            Cmd::Layer {
                src: Rc::new(src),
                m,
                p,
                op,
                crop,
            },
            b,
            h,
        );
    }

    /// Premultiplied pixels placed 1:1 at (x, y).
    pub fn blit(&mut self, img: Canvas, x: i64, y: i64) {
        self.taint();
        if self.xf.is_some() {
            return self.blit_at(img, x as f64, y as f64);
        }
        let h = mix(mix(mix_px(8, &img), x as u64), y as u64);
        let b = [
            x as f32,
            y as f32,
            (x + img.w as i64) as f32,
            (y + img.h as i64) as f32,
        ];
        self.push(Cmd::Blit(Rc::new(img), x, y), b, h);
    }

    /// CPU-rasterized content (Canvas, Shape, effects) at (x, y) through `xf`, sampled linearly
    /// like a texture drawn through the item transform.
    pub fn blit_at(&mut self, img: Canvas, x: f64, y: f64) {
        self.taint();
        let rect = self.xf_rect([x, y, img.w as f64, img.h as f64]);
        let rect = rect.map(|v| v as f32);
        let hp = mix_px(10, &img);
        // Shared per pixel content, so unchanged rasters keep their uploaded texture.
        thread_local! {
            static RASTERS: std::cell::RefCell<qml::bytelru::ByteLru<u64, Rc<Canvas>>> =
                std::cell::RefCell::new(
                    qml::bytelru::ByteLru::new(16 << 20)
                        .max_len(64)
                        .pinned(|t: &Rc<Canvas>| Rc::strong_count(t) > 1),
                );
        }
        let tex = RASTERS.with(|r| {
            let mut r = r.borrow_mut();
            if let Some(t) = r.get(&hp).filter(|t| t.w == img.w && t.h == img.h) {
                return t.clone();
            }
            let n = img.px.len() * 4;
            let t = Rc::new(img);
            r.insert(hp, t.clone(), n);
            t
        });
        let h = mix_f(hp, &rect);
        self.keep.push(tex.clone());
        let b = [rect[0], rect[1], rect[0] + rect[2], rect[1] + rect[3]];
        self.push(
            Cmd::Tex {
                src: tex,
                rect,
                uv: [0.0, 0.0, 1.0, 1.0],
                k: [0.0; 4],
                op: 1.0,
                mode: PLAIN,
                smooth: true,
                mipmap: false,
            },
            b,
            h,
        );
    }
}

pub struct Surface {
    egl: Rc<Egl>,
    display: egl::Display,
    es: egl::Surface,
    /// None for a `Gpu::pbuffer` surface.
    wl: Option<wayland_egl::WlEglSurface>,
    /// The window's wl_surface: an unchanged frame commits it without a new buffer.
    wl_surface: Option<WlSurface>,
    size: (i32, i32),
    /// Previous frame's signature and the images it drew.
    prev: Vec<(u64, [i64; 4])>,
    keep: Vec<Rc<dyn std::any::Any>>,
    /// Damage of the last swapped frames, newest first (for EGL_EXT_buffer_age repair).
    hist: Vec<Region>,
    /// Swap the next frame even if unchanged (`Surface::reswap`).
    reswap: bool,
    /// The crossfade's GL paths ran once on this window (`Gpu::warm_fade`).
    warm: bool,
    /// The previous frame fading out over the new ones (`Gpu::begin_fade`).
    fade: Option<Fade>,
    /// This frame's content without the fade over it: while it stays the same, a fade frame is
    /// two texture draws instead of the whole draw list.
    fresh: Option<(glow::Texture, glow::Framebuffer)>,
    /// The crossfade requests this window takes (`qml::native::FadeArm`).
    pub crossfade: qml::native::FadeArm,
    /// `Gpu::tris_bufs` key of the vertex buffers it draws with: its own, or another surface's
    /// (`share_buffers`).
    tris: usize,
    /// `Gpu::dead`: GL objects this surface owns are queued there when it is dropped.
    dead: Rc<std::cell::RefCell<Dead>>,
}

/// GL objects of dropped surfaces, freed by the next `Gpu::render`/`trim` with the context
/// current: a `Surface` can't reach the `glow::Context`, and dropping it (a closed panel, a
/// removed output) must not leave its window-sized fade/fresh targets allocated.
#[derive(Default)]
struct Dead {
    targets: Vec<(glow::Texture, glow::Framebuffer)>,
    /// `Gpu::tris_bufs` keys of dropped surfaces.
    surfaces: Vec<usize>,
}

struct Fade {
    tex: glow::Texture,
    fb: glow::Framebuffer,
    /// The part of the window that fades, as rectangles that do not overlap: only they hold
    /// the start image in `tex` and only they are repainted for the fade. None = the whole
    /// window.
    area: Option<Region>,
    dur: std::time::Duration,
    /// Fade time shown so far, on the animation clock (`Gpu::clock`) like every other
    /// animation: the shell's clock steps one vsync per presented frame (QSGDefaultAnimationDriver),
    /// so a slow first frame (the new theme's layout) or a compositor stall (Hyprland reloading
    /// its config for the new colors) delays the fade instead of skipping part of it.
    elapsed: std::time::Duration,
    /// Animation time of the first fade frame (None before it is drawn).
    start: Option<std::time::Instant>,
    /// The fade starts from the frame the window shows: its first frame, the start image at
    /// opacity 1 over the new frame, is on screen already and is not drawn again.
    shown: bool,
}

impl Fade {
    fn new(
        tex: glow::Texture,
        fb: glow::Framebuffer,
        area: Option<Region>,
        dur: std::time::Duration,
        shown: bool,
    ) -> Self {
        Fade {
            tex,
            fb,
            area,
            dur,
            elapsed: Default::default(),
            start: None,
            shown,
        }
    }

    /// Progress (0..=1) of the fade shown so far.
    fn progress(&self) -> f32 {
        (self.elapsed.as_secs_f32() / self.dur.as_secs_f32().max(0.001)).min(1.0)
    }

    /// Opacity of the start image in the last fade frame drawn.
    fn opacity(&self) -> f32 {
        fade_opacity(self.progress())
    }
}

impl Surface {
    pub fn size(&self) -> (i32, i32) {
        self.size
    }

    /// A crossfade is running: the window needs a frame every vsync until it ends.
    pub fn fading(&self) -> bool {
        self.fade.is_some()
    }

    /// Where a back buffer shown `age` swaps ago differs from the frame shown last: what the
    /// frames since then changed. None = anywhere (its pixels are unknown, or older than the
    /// kept history).
    fn stale(&self, age: i32) -> Option<Region> {
        if age <= 0 || age as usize - 1 > self.hist.len() {
            return None;
        }
        let mut stale = Region::new();
        for r in self.hist.iter().take(age as usize - 1).flatten() {
            region::add(&mut stale, *r);
        }
        Some(stale)
    }

    /// The compositor resends dma-buf feedback on output changes; Mesa queues it (a format-table
    /// fd each time) on the surface's private queue, which only drawing to the surface
    /// dispatches. Qt redraws its windows on screen changes; swapping once here does the same.
    pub fn reswap(&mut self) {
        self.reswap = true;
    }

    /// Draws with `of`'s vertex buffers: a frame `of` last drew (a crossfade's start frame on
    /// its veil) finds its triangles uploaded already.
    pub fn share_buffers(&mut self, of: &Surface) {
        self.tris = of.tris;
    }
}

/// `eglGetProcAddress(name)` as `Option<$ty>`. `$ty` must be the C signature the extension's
/// specification gives `name`: the returned address is only valid called through that.
macro_rules! ext_proc {
    ($egl:expr, $name:expr, $ty:ty) => {
        $egl.get_proc_address($name).map(|f| {
            // SAFETY: `f` is the entry point of `$name` and `$ty` its specified C signature;
            // both are non-null `extern "system"` function pointers of the same size.
            unsafe { std::mem::transmute::<extern "system" fn(), $ty>(f) }
        })
    };
}

type SwapDamage = unsafe extern "system" fn(*mut c_void, *mut c_void, *const i32, i32) -> u32;

/// EGL_EXT_create_context_robustness: the context's reset notification strategy.
const EGL_CONTEXT_OPENGL_RESET_NOTIFICATION_STRATEGY_EXT: egl::Int = 0x3138;
const EGL_LOSE_CONTEXT_ON_RESET_EXT: egl::Int = 0x31BF;

/// glGetGraphicsResetStatusEXT (GL_EXT_robustness): 0 (GL_NO_ERROR) until the GPU was reset.
type ResetStatus = unsafe extern "system" fn() -> u32;

/// glClipControlEXT (GL_EXT_clip_control): the clip volume's origin and depth mode.
type ClipControl = unsafe extern "system" fn(u32, u32);

/// How a blur pass lays the rows of its scene in its target.
#[derive(Clone, Copy, PartialEq)]
enum Rows {
    /// The scene's bottom row is the target's first: a window, as OpenGL has it.
    BottomFirst,
    /// The scene's top row is the target's first: a layer, which Qt mirrors.
    TopFirst,
    /// As Mesa keeps a window: top row first, with the clip volume's origin at the upper left
    /// (GL_EXT_clip_control; a driver without it has `BottomFirst`). Mesa then turns the
    /// viewport over itself and gives a pixel whose centre is on an edge to the side it gives
    /// it in a window (`bottom_edge_rule` in st_atom_rasterizer.c, Mesa 26.2.6); a matrix that
    /// turns the scene over gives it to the other.
    Window,
}

/// Frames in a row that may fail on their surface before the context counts as lost.
const SURFACE_FAILURES: u32 = 3;

/// Why `Gpu::render` did not show its frame (`Gpu::take_failure`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    /// The window surface is unusable (EGL_BAD_SURFACE, EGL_BAD_NATIVE_WINDOW: its output is
    /// gone). The context still draws: the frame is drawn again on a new surface.
    Surface,
    /// The context is lost for good (EGL_CONTEXT_LOST, a GPU reset, or `SURFACE_FAILURES`
    /// failed frames in a row): every GL object went with it, and this `Gpu` draws nothing
    /// any more. The shell drops it (`Gpu::retire`) and draws on with a new one or on the CPU.
    Context,
}

/// What the log says about a failed `call` (as Qt's QWaylandGLContext logs both calls with
/// the EGL error) and what the failure means.
fn failure(call: &str, e: egl::Error, failed: u32) -> (String, Failure) {
    let lost = e == egl::Error::ContextLost;
    let kind = if lost || failed >= SURFACE_FAILURES {
        Failure::Context
    } else {
        Failure::Surface
    };
    let what = match kind {
        Failure::Surface => "the frame is drawn again on a new surface".to_owned(),
        Failure::Context if lost => "the GPU context is lost".to_owned(),
        Failure::Context => format!("{failed} frames in a row failed, the GPU context is given up"),
    };
    let name = format!("{e:?}");
    (
        format!("gpu: {call} failed: {:#x} ({name}): {what}", e.native()),
        kind,
    )
}

/// `RI_DEBUG=gpuloss=<n>` is used up: only the first context of the process is lost.
static LOSS_INJECTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// eglCreateImageKHR (EGL_KHR_image_base).
type CreateImage = unsafe extern "system" fn(
    *mut c_void,
    *mut c_void,
    u32,
    *mut c_void,
    *const i32,
) -> *mut c_void;
/// glEGLImageTargetTexture2DOES (GL_OES_EGL_image).
type TargetTex = unsafe extern "system" fn(u32, *mut c_void);
/// eglDestroyImageKHR (EGL_KHR_image_base).
type DestroyImage = unsafe extern "system" fn(*mut c_void, *mut c_void) -> u32;

/// What importing a dmabuf as a texture calls, on a display and context that advertise all of
/// it (`dmabuf_import_exts`).
#[derive(Clone, Copy)]
struct EglImage {
    create: CreateImage,
    target: TargetTex,
    destroy: DestroyImage,
}

/// Whether a dmabuf can be imported as a `GL_TEXTURE_2D`: the display's EGL_EXTENSIONS
/// (`egl`) must name EGL_KHR_image_base (eglCreateImageKHR, eglDestroyImageKHR) and
/// EGL_EXT_image_dma_buf_import (the EGL_LINUX_DMA_BUF_EXT target), and the context's
/// GL_EXTENSIONS (`gl`) GL_OES_EGL_image (glEGLImageTargetTexture2DOES). The strings decide,
/// not the addresses: eglGetProcAddress may return one for a function the display or context
/// does not implement (EGL 1.5 §3.11). Qt 6.12's compositor-side dmabuf import
/// (linuxdmabufclientbufferintegration.cpp) checks only the EGL_EXT_image_dma_buf_import
/// string and takes the three functions by address; this check is the stricter one the
/// extension specifications ask for.
fn dmabuf_import_exts(egl: &str, gl: &std::collections::HashSet<String>) -> bool {
    has_ext(egl, "EGL_KHR_image_base")
        && has_ext(egl, "EGL_EXT_image_dma_buf_import")
        && gl.contains("GL_OES_EGL_image")
}

type CreateSync = unsafe extern "system" fn(*mut c_void, u32, *const i32) -> *mut c_void;
type ClientWaitSync = unsafe extern "system" fn(*mut c_void, *mut c_void, i32, u64) -> i32;
type DestroySync = unsafe extern "system" fn(*mut c_void, *mut c_void) -> u32;

/// eglDupNativeFenceFDANDROID (EGL_ANDROID_native_fence_sync).
type DupNativeFence = unsafe extern "system" fn(*mut c_void, *mut c_void) -> i32;

/// EGL_KHR_fence_sync: eglCreateSyncKHR, eglClientWaitSyncKHR, eglDestroySyncKHR, and
/// eglDupNativeFenceFDANDROID where EGL_ANDROID_native_fence_sync is supported.
#[derive(Clone, Copy)]
struct FenceSync {
    create: CreateSync,
    wait: ClientWaitSync,
    destroy: DestroySync,
    dup: Option<DupNativeFence>,
}

const EGL_SYNC_NATIVE_FENCE_ANDROID: u32 = 0x3144;
const EGL_SYNC_NATIVE_FENCE_FD_ANDROID: i32 = 0x3145;
const EGL_NO_NATIVE_FENCE_FD_ANDROID: i32 = -1;
const EGL_TIMEOUT_EXPIRED_KHR: i32 = 0x30F5;
const EGL_FOREVER_KHR: u64 = u64::MAX;
const BUFFER_AGE: i32 = 0x313D;

impl Drop for Gpu {
    /// Destroys the context, then (`_display_ref`, the last field) terminates the display once
    /// no other `Gpu` uses it: llvmpipe joins its rasterizer and pool threads, which otherwise
    /// still run while `exit` unloads the driver (a SIGSEGV at exit).
    fn drop(&mut self) {
        let _ = self.egl.make_current(self.display, None, None, None);
        let _ = self.egl.destroy_context(self.display, self.ctx);
    }
}

/// An EGL context `Gpu::on_display` is still setting up: dropped before `keep`, it is released
/// from the thread and destroyed (EGL 1.5 §3.7.1.2: GL objects made on an unshared context go
/// with it), so a renderer that fails to start leaves no context behind.
struct NewContext {
    egl: Rc<Egl>,
    display: egl::Display,
    ctx: Option<egl::Context>,
}

impl NewContext {
    fn keep(mut self) -> egl::Context {
        self.ctx.take().expect("context kept once")
    }
}

impl Drop for NewContext {
    fn drop(&mut self) {
        if let Some(ctx) = self.ctx.take() {
            let _ = self.egl.make_current(self.display, None, None, None);
            let _ = self.egl.destroy_context(self.display, ctx);
        }
    }
}

/// Live `Gpu`s per initialized EGL display. `eglInitialize` does not count
/// (EGL_KHR_display_reference is not in Mesa) and `eglTerminate` ends the display for every
/// `Gpu` on it, so the last `DisplayRef` of a display terminates it.
static DISPLAYS: std::sync::Mutex<Vec<(usize, usize)>> = std::sync::Mutex::new(Vec::new());

struct DisplayRef {
    egl: Rc<Egl>,
    display: egl::Display,
}

impl DisplayRef {
    fn acquire(egl: &Rc<Egl>, display: egl::Display) -> Option<DisplayRef> {
        let mut live = DISPLAYS.lock().unwrap_or_else(|e| e.into_inner());
        egl.initialize(display).ok()?;
        let key = display.as_ptr() as usize;
        match live.iter_mut().find(|(d, _)| *d == key) {
            Some((_, n)) => *n += 1,
            None => live.push((key, 1)),
        }
        Some(DisplayRef {
            egl: egl.clone(),
            display,
        })
    }
}

impl Drop for DisplayRef {
    fn drop(&mut self) {
        let mut live = DISPLAYS.lock().unwrap_or_else(|e| e.into_inner());
        let key = self.display.as_ptr() as usize;
        let Some(i) = live.iter().position(|(d, _)| *d == key) else {
            return;
        };
        live[i].1 -= 1;
        if live[i].1 == 0 {
            live.swap_remove(i);
            let _ = self.egl.terminate(self.display);
        }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        let mut d = self.dead.borrow_mut();
        let fade = self.fade.take().map(|f| (f.tex, f.fb));
        d.targets.extend(fade.into_iter().chain(self.fresh.take()));
        d.surfaces.push(self.es.as_ptr() as usize);
        drop(d);
        let _ = self.egl.destroy_surface(self.display, self.es);
    }
}

struct Atlas {
    tex: glow::Texture,
    /// Side of the allocated texture; 0 until the first bitmap.
    size: i32,
    /// Largest side it grows to.
    max: i32,
    /// RGBA texels (colour glyphs, distance-field tiles), else ALPHA (glyph masks).
    rgba: bool,
    /// Bitmaps in the atlas by key (`Glyph::key`, `Canvas::key`): (bitmap, x, y). The atlas
    /// holds a reference like a `QSGOpenGLAtlasTexture`: once it holds the last one, the area
    /// can be freed.
    slots: HashMap<u64, (Rc<dyn std::any::Any>, i32, i32)>,
    areas: qsg::areaalloc::AreaAllocator,
}

impl Atlas {
    fn new(tex: glow::Texture, rgba: bool) -> Self {
        Atlas {
            tex,
            size: 0,
            max: ATLAS,
            rgba,
            slots: HashMap::new(),
            areas: qsg::areaalloc::AreaAllocator::new(0, 0),
        }
    }

    /// Frees the areas of bitmaps nothing else holds any more; false if there were none.
    fn free_unused(&mut self) -> bool {
        let n = self.slots.len();
        let areas = &mut self.areas;
        self.slots.retain(|_, (b, x, y)| {
            let dead = Rc::strong_count(b) == 1;
            if dead {
                areas.deallocate(*x, *y);
            }
            !dead
        });
        self.slots.len() < n
    }

    /// Where the `w` x `h` bitmap `b` (keyed `key`) with texels `px` is, uploaded first if it is
    /// new (freeing unused areas when it doesn't fit); None when the atlas is full. Each area
    /// has a cleared texel row and column past the bitmap, which scaled glyph quads may sample.
    ///
    /// # Safety
    /// The GL context `gl` was loaded for must be current on the calling thread, with
    /// GL_UNPACK_ALIGNMENT 1 as `Gpu::on_display` sets it: GL reads rows of `w + 1` texels from a
    /// buffer that holds them unpadded (GLES 2.0 §3.6.2).
    #[allow(clippy::too_many_arguments)]
    unsafe fn place(
        &mut self,
        gl: &glow::Context,
        key: u64,
        b: &Rc<dyn std::any::Any>,
        w: i32,
        h: i32,
        px: &[u8],
    ) -> Option<(i32, i32)> {
        if let Some((_, x, y)) = self.slots.get(&key) {
            return Some((*x, *y));
        }
        let (x, y) = match self.areas.allocate(w + 1, h + 1) {
            Some(p) => p,
            None if self.free_unused() => self.areas.allocate(w + 1, h + 1)?,
            None => return None,
        };
        self.slots.insert(key, (b.clone(), x, y));
        let (fmt, bpp) = if self.rgba {
            (glow::RGBA, 4)
        } else {
            (glow::ALPHA, 1)
        };
        let (row, padded) = (w as usize * bpp, (w as usize + 1) * bpp);
        let mut buf = vec![0u8; padded * (h as usize + 1)];
        for (d, s) in buf.chunks_exact_mut(padded).zip(px.chunks_exact(row)) {
            d[..row].copy_from_slice(s);
        }
        // SAFETY: the context is current, as this fn requires.
        unsafe { gl.bind_texture(glow::TEXTURE_2D, Some(self.tex)) };
        gl_count(1);
        if DRAWING.get() {
            gl_count(GL_MIDDRAW);
        }
        // SAFETY: the context is current, as this fn requires.
        unsafe {
            gl.tex_sub_image_2d(
                glow::TEXTURE_2D,
                0,
                x,
                y,
                w + 1,
                h + 1,
                fmt,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(Some(&buf)),
            )
        };
        Some((x, y))
    }

    /// The side to start over at when a `w` x `h` bitmap did not fit: twice the size (up to
    /// `max`) or more for a bigger bitmap.
    fn next_size(&self, w: i32, h: i32) -> i32 {
        let mut size = (self.size * 2).clamp(ATLAS_MIN.min(self.max), self.max);
        while size < self.max && (w + 1 > size || h + 1 > size) {
            size *= 2;
        }
        size.min(self.max)
    }
}

/// Placeholder canvases by key: (canvas, imported texture, y-inverted).
type ExtTextures = HashMap<u64, (std::rc::Weak<Canvas>, glow::Texture, bool)>;

thread_local! {
    /// Placeholder canvases standing for imported dmabuf textures (texture, y-inverted).
    static EXT: std::cell::RefCell<ExtTextures> =
        Default::default();
}

/// Draws of the (pixel-less) canvas `rc` sample `tex` instead; `inv` flips it vertically.
pub fn register_ext(rc: &Rc<Canvas>, tex: glow::Texture, inv: bool) {
    EXT.with(|m| {
        let mut m = m.borrow_mut();
        m.retain(|_, e| e.0.strong_count() > 0);
        m.insert(rc.key.get(), (Rc::downgrade(rc), tex, inv));
    });
}

fn ext(rc: &Rc<Canvas>) -> Option<(glow::Texture, bool)> {
    EXT.with(|m| m.borrow().get(&rc.key.get()).map(|e| (e.1, e.2)))
}

/// Runs a scissored clear once on a new context, into a 1x1 target of the window format
/// (BGRA where GL_EXT_texture_format_BGRA8888 allows it, else RGBA). Mesa builds that path
/// lazily (llvmpipe: 4-20 ms), and the first partial redraw — a ListView's first scroll —
/// missed vblanks paying it. Elsewhere this costs a few microseconds at start-up.
///
/// # Safety
/// The GL context `gl` was loaded for must be current on the calling thread.
unsafe fn warm_up(gl: &glow::Context) {
    let fmt = if gl
        .supported_extensions()
        .contains("GL_EXT_texture_format_BGRA8888")
    {
        glow::BGRA
    } else {
        glow::RGBA
    };
    // SAFETY: `gl` is current, as this fn requires.
    let t = unsafe { gl.create_texture() };
    // SAFETY: `gl` is current, as this fn requires.
    let fb = unsafe { gl.create_framebuffer() };
    let (Ok(t), Ok(fb)) = (t, fb) else {
        return;
    };
    // SAFETY: `gl` is current, as this fn requires.
    unsafe { gl.bind_texture(glow::TEXTURE_2D, Some(t)) };
    let px = glow::PixelUnpackData::Slice(None);
    // SAFETY: `gl` is current, as this fn requires.
    unsafe {
        gl.tex_image_2d(
            glow::TEXTURE_2D,
            0,
            fmt as i32,
            1,
            1,
            0,
            fmt,
            glow::UNSIGNED_BYTE,
            px,
        )
    };
    // SAFETY: `gl` is current, as this fn requires.
    unsafe { gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fb)) };
    let tex = Some(t);
    // SAFETY: `gl` is current, as this fn requires.
    unsafe {
        gl.framebuffer_texture_2d(
            glow::FRAMEBUFFER,
            glow::COLOR_ATTACHMENT0,
            glow::TEXTURE_2D,
            tex,
            0,
        )
    };
    // SAFETY: `gl` is current, as this fn requires.
    if unsafe { gl.check_framebuffer_status(glow::FRAMEBUFFER) } == glow::FRAMEBUFFER_COMPLETE {
        // SAFETY: `gl` is current, as this fn requires.
        unsafe { gl.enable(glow::SCISSOR_TEST) };
        // SAFETY: `gl` is current, as this fn requires.
        unsafe { gl.scissor(0, 0, 1, 1) };
        // SAFETY: `gl` is current, as this fn requires.
        unsafe { gl.clear(glow::COLOR_BUFFER_BIT) };
        // SAFETY: `gl` is current, as this fn requires.
        unsafe { gl.disable(glow::SCISSOR_TEST) };
    }
    // SAFETY: `gl` is current, as this fn requires.
    unsafe { gl.bind_framebuffer(glow::FRAMEBUFFER, None) };
    // SAFETY: `gl` is current, as this fn requires.
    unsafe { gl.bind_texture(glow::TEXTURE_2D, None) };
    // SAFETY: `gl` is current, as this fn requires.
    unsafe { gl.delete_framebuffer(fb) };
    // SAFETY: `gl` is current, as this fn requires.
    unsafe { gl.delete_texture(t) };
}

/// A surface's (last used frame, `Cmd::Tris` buffers with their vertices).
type TrisBufs = (u64, Vec<(glow::Buffer, Vec<f32>)>);
/// A frame signature: (content hash, clipped pixel bounds) per drawn primitive.
type Signature = Vec<(u64, [i64; 4])>;

/// One EGL context and what it draws with.
///
/// GL commands act on the context that is current on the calling thread (EGL 1.5 §2.2) and are
/// undefined without one (GLES 2.0 §2.1). The `unsafe fn`s here issue them: each names in its
/// `# Safety` section what its caller must have set up, this context made current first of all.
pub struct Gpu {
    egl: Rc<Egl>,
    display: egl::Display,
    config: egl::Config,
    ctx: egl::Context,
    gl: glow::Context,
    color: glow::Program,
    tex: glow::Program,
    /// Uniform locations: `f` of `color`; `s k m o x y` of `tex`.
    color_f: Option<glow::UniformLocation>,
    tex_u: [Option<glow::UniformLocation>; 6],
    effect: glow::Program,
    effect_u: [Option<glow::UniformLocation>; 17],
    blur: glow::Program,
    blur_u: [Option<glow::UniformLocation>; 3],
    /// GaussianBlur/DropShadow pass programs by `Gauss::digest` (taps and mode), compiled on
    /// first use; None for a kernel whose shader failed to build.
    gauss: HashMap<u64, Option<GaussProg>>,
    rshadow: glow::Program,
    rshadow_u: [Option<glow::UniformLocation>; 7],
    curve: glow::Program,
    curve_u: [Option<glow::UniformLocation>; 3],
    radial: glow::Program,
    radial_u: [Option<glow::UniformLocation>; 7],
    radial_q: Option<glow::UniformLocation>,
    vbo: glow::Buffer,
    /// Next free byte of `vbo` (`stream`).
    stream_at: std::cell::Cell<usize>,
    /// Per window surface (last used frame, buffers): its frame's `Cmd::Tris` lists in draw
    /// order, re-uploaded only when they change (a frame re-rendered for a new screencopy).
    tris_bufs: HashMap<usize, TrisBufs>,
    /// Surface being rendered and the index of its next `Cmd::Tris`.
    tris_at: Option<(usize, usize)>,
    scratch: glow::Texture,
    /// Uploaded shared images by `Canvas::key`.
    images: HashMap<u64, (std::rc::Weak<Canvas>, glow::Texture)>,
    /// Whether a texture of any size can have mipmaps: GLES 3, or GL_OES_texture_npot.
    npot_mips: bool,
    /// GL_MAX_TEXTURE_SIZE: larger images upload scaled down to it (`texture`), larger layers
    /// render scaled down to it (`offscreen_full`).
    max_tex: usize,
    /// The layers and blurs past `max_tex` already said in the log (`say_clamped`), by what
    /// they are and the size asked for.
    clamped: std::collections::HashSet<(&'static str, i32, i32)>,
    /// GL_MAX_FRAGMENT_UNIFORM_VECTORS: a blur kernel's uniform array must fit it
    /// (`Gauss::looped`).
    max_frag_uniforms: usize,
    /// Texels per draw-space pixel of the offscreen target `exec` draws into: below 1 for a
    /// layer clamped to `max_tex` (`fit`), else 1.
    vp: (f64, f64),
    /// Offscreen layer targets kept across frames, keyed by size.
    pool: Vec<((i32, i32), glow::Texture, glow::Framebuffer)>,
    /// Rendered layers keyed by their draw list's digest, reused while the content is unchanged.
    layers: LayerCache<LayerTex>,
    inc: LayerCache<IncLayer>,
    /// `exec` into an offscreen target limited to `dmg`, like the window (`inc_layer_tex`).
    force_top: bool,
    /// The target being drawn holds its scene top row first (row 0 is the scene's top), as
    /// every offscreen layer does: on OpenGL QSGRhiLayer mirrors a layer vertically
    /// (`m_mirrorVertical`), so an edge exactly on a pixel centre falls to the other side than
    /// in the window. The window itself is drawn y-up.
    flip: bool,
    /// The next `offscreen` draws its target like the window (`check_damage`).
    upright: bool,
    frame_no: u64,
    /// When frames were drawn (number, time), oldest first: the latest and at most two per
    /// `STAMP_EVERY` before it, back to the newest drawn `LAYER_IDLE_TIME` ago.
    stamps: std::collections::VecDeque<(u64, std::time::Instant)>,
    atlas: Atlas,
    /// RGBA atlas for colour (emoji) glyphs.
    catlas: Atlas,
    /// RGBA atlas for distance-field text tiles (`Cmd::Df`).
    dfatlas: Atlas,
    pub renderer: String,
    /// Partial redraw: back buffers report their age, damage goes to the compositor.
    buffer_age: bool,
    swap_damage: Option<SwapDamage>,
    /// The EGLImage functions `import_dmabuf` needs; None where the display or the context
    /// does not advertise them.
    image: Option<EglImage>,
    /// Window-space region being repainted this frame (None = everything).
    dmg: Option<[i64; 4]>,
    pub last_damage: [i64; 4],
    /// Pixels of the window the last `begin_fade` fades.
    pub fade_px: i64,
    /// Pixels of the old frame the last `begin_fade` had to draw: those its window's back
    /// buffer did not hold already.
    pub fade_drawn: i64,
    /// `Cmd::Tris` vertex buffers the last `render` uploaded.
    pub last_uploads: usize,
    /// Set before `render`: an unchanged frame sends nothing, not even a commit without a
    /// buffer.
    pub may_skip: bool,
    /// When the last frame's GL work had finished and swap began (ri::stats only waits for it).
    pub swap_start: Option<std::time::Instant>,
    dbg: (Signature, Signature, i32),
    /// RI_DEBUG=skipcheck: draw commands that would be skipped as covering no pixel.
    no_skip: bool,
    /// `frame_changed`: diff/reduce/copy program and its `a b s m` uniforms.
    diff: Option<(glow::Program, [Option<glow::UniformLocation>; 4])>,
    /// Results of the compares `frame_compare` queued since the last `compare_read`, one pixel
    /// each, and their count.
    diff_row: Option<(glow::Texture, glow::Framebuffer, i32, i32)>,
    fade_prog: Option<(glow::Program, [Option<glow::UniformLocation>; 2])>,
    diff_queued: usize,
    fence: Option<FenceSync>,
    /// Fence after the queued compares (`compare_submit`): their readback waits for it.
    diff_sync: Option<*mut c_void>,
    #[cfg(test)]
    age_override: Option<i32>,
    /// Tests: the next swaps fail as eglSwapBuffers does for a lost surface.
    #[cfg(test)]
    swap_fails: bool,
    /// Tests: `surface`'s eglMakeCurrent fails (EGL_BAD_ACCESS) instead of being called.
    #[cfg(test)]
    make_current_fails: bool,
    /// Tests: `target` makes this many more targets, then fails as it does when GL has no
    /// memory for one.
    #[cfg(test)]
    targets_left: std::cell::Cell<Option<u32>>,
    /// Tests: `render`'s eglMakeCurrent fails with this error instead of being called.
    #[cfg(test)]
    bind_fails: Option<egl::Error>,
    /// `RI_DEBUG=gpuloss=<n>`: the `render` that reports EGL_CONTEXT_LOST, counted in `binds`.
    lose_at: Option<u64>,
    /// `render` calls that went on to draw.
    binds: u64,
    reset_status: Option<ResetStatus>,
    /// glClipControlEXT where the context has GL_EXT_clip_control (`Rows::Window`).
    clip_control: Option<ClipControl>,
    /// Why the last `render` showed nothing, until `take_failure`.
    failure: Option<Failure>,
    /// Frames that failed since the last one shown.
    failed: u32,
    /// Frames shown (swapped).
    shown: u64,
    /// The animation clock's time of the frame being rendered, which crossfades run on (the
    /// shell sets it from its animation driver; offscreen renders step it so their fade frames
    /// match). None = the wall clock.
    pub clock: Option<std::time::Instant>,
    dead: Rc<std::cell::RefCell<Dead>>,
    st: StateCell,
    /// Skip GL calls that repeat the cached state (off: RI_DEBUG=noglcache, A/B tests).
    pub gl_cache: bool,
    /// Dropped last: terminates the display after everything else of this `Gpu` is gone.
    _display_ref: DisplayRef,
}

impl Gpu {
    /// Makes `p` the program in use.
    ///
    /// # Safety
    /// This `Gpu`'s context must be current on the calling thread.
    unsafe fn use_prog(&self, p: glow::Program) {
        if self.gl_cache && self.st.borrow().program == Some(p) {
            return;
        }
        self.st.borrow_mut().program = Some(p);
        gl_count(GL_PROGRAM);
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.gl.use_program(Some(p)) };
    }

    /// Sets uniform `k` of program `prog`, at `loc`, to `v`.
    ///
    /// # Safety
    /// This `Gpu`'s context must be current on the calling thread.
    unsafe fn set_u(&self, prog: usize, k: usize, loc: Option<&glow::UniformLocation>, v: Uni) {
        // SAFETY: the context is current, as this fn requires.
        unsafe { uni_set(&self.gl, &self.st, self.gl_cache, prog, k, loc, v) }
    }

    /// Binds `b` as the array buffer.
    ///
    /// # Safety
    /// This `Gpu`'s context must be current on the calling thread.
    unsafe fn bind_array(&self, b: glow::Buffer) {
        if self.gl_cache && self.st.borrow().array_buf == Some(b) {
            return;
        }
        self.st.borrow_mut().array_buf = Some(b);
        gl_count(GL_BINDBUF);
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.gl.bind_buffer(glow::ARRAY_BUFFER, Some(b)) };
    }

    /// Enables or disables vertex attrib array `k`.
    ///
    /// # Safety
    /// This `Gpu`'s context must be current on the calling thread.
    unsafe fn attrib(&self, k: u32, on: bool) {
        if self.gl_cache && self.st.borrow().attrib_on[k as usize] == Some(on) {
            return;
        }
        self.st.borrow_mut().attrib_on[k as usize] = Some(on);
        gl_count(GL_ATTRIB);
        if on {
            // SAFETY: the context is current, as this fn requires.
            unsafe { self.gl.enable_vertex_attrib_array(k) };
        } else {
            // SAFETY: the context is current, as this fn requires.
            unsafe { self.gl.disable_vertex_attrib_array(k) };
        }
    }

    /// Points attrib `k` at `size` floats every `stride` bytes from `off` of the bound array
    /// buffer (`bind_array`).
    ///
    /// # Safety
    /// This `Gpu`'s context must be current on the calling thread, with an array buffer bound:
    /// without one GL takes `off` for an address in client memory, which a draw then reads (GLES
    /// 2.0 §2.8, §2.9.1).
    unsafe fn attrib_ptr(&self, k: u32, size: i32, stride: i32, off: i32) {
        {
            let mut st = self.st.borrow_mut();
            let want = st.array_buf.map(|b| (b, size, stride, off));
            if self.gl_cache && want.is_some() && st.attrib_ptr[k as usize] == want {
                return;
            }
            st.attrib_ptr[k as usize] = want;
        }
        gl_count(GL_ATTRIB);
        // SAFETY: the context is current, as this fn requires.
        unsafe {
            self.gl
                .vertex_attrib_pointer_f32(k, size, glow::FLOAT, false, stride, off)
        };
    }

    /// Sets the min and mag filter of `t`, bound to the active unit, to `f`.
    ///
    /// # Safety
    /// This `Gpu`'s context must be current on the calling thread.
    unsafe fn filter(&self, t: glow::Texture, f: i32) {
        // SAFETY: the context is current, as this fn requires.
        unsafe { filter_set(&self.gl, &self.st, self.gl_cache, t, f) }
    }

    /// Logs and clears the GL error flags (debug builds and RI_LOG=ri::glerror: glGetError
    /// can stall the pipeline on some drivers, so release builds don't poll it every frame).
    ///
    /// # Safety
    /// This `Gpu`'s context must be current on the calling thread.
    unsafe fn log_gl_errors(&self) {
        // SAFETY: the context is current, as this fn requires.
        unsafe { drain_gl_errors(&self.gl, "in a frame") };
    }

    fn reset_state(&self) {
        self.st.borrow_mut().reset();
    }

    /// Makes this `Gpu`'s context current on the calling thread unless it already is, on
    /// whatever surface it is bound to (none when it was not current): the safe `pub fn`s
    /// that issue GL outside `render` start with it, like QOpenGLContext::makeCurrent before
    /// any GL call of Qt's. Another `Gpu`, or a dropped one (`Drop` releases the thread's
    /// context), may have left a different context current, or none. False when EGL refuses.
    fn make_current(&self) -> bool {
        self.egl.get_current_context() == Some(self.ctx)
            || self
                .egl
                .make_current(self.display, None, None, Some(self.ctx))
                .is_ok()
    }

    /// glFinish: returns once the GPU has done everything issued so far.
    pub fn finish(&self) {
        if !self.make_current() {
            return;
        }
        // SAFETY: `make_current` above succeeded.
        unsafe { self.gl.finish() };
    }
}

/// Whether `exts`, a space-separated extension string (EGL 1.5 §3.3, GLES 2.0 §6.1.5), names
/// `name`.
fn has_ext(exts: &str, name: &str) -> bool {
    exts.split(' ').any(|e| e == name)
}

/// The eglSwapBuffersWithDamage function of the extension `exts` (the display's
/// EGL_EXTENSIONS) advertises, KHR first. eglGetProcAddress may answer for a name the display
/// does not implement (EGL 1.5 §3.11: a non-NULL return does not guarantee the function is
/// supported), so the string decides, as Qt asks `q_hasEglExtension` before it resolves an
/// extension's function (qwaylandeglclientbufferintegration.cpp: eglGetPlatformDisplayEXT).
fn swap_damage_name(exts: &str) -> Option<&'static str> {
    [
        (
            "EGL_KHR_swap_buffers_with_damage",
            "eglSwapBuffersWithDamageKHR",
        ),
        (
            "EGL_EXT_swap_buffers_with_damage",
            "eglSwapBuffersWithDamageEXT",
        ),
    ]
    .into_iter()
    .find_map(|(ext, f)| has_ext(exts, ext).then_some(f))
}

/// The config for `attrs`: exactly RGBA8888 with the depth and stencil buffers of Qt's default
/// surface format (24 and 8 bits) and no multisampling when there is one. eglChooseConfig
/// matches sizes as minimums and sorts deeper colour first (EGL 1.5 §3.4.1.2), so its first
/// config may carry 10-bit channels or samples that every window would allocate and never
/// use. Nothing here tests depth; Mesa's llvmpipe clips a triangle to the viewport only when
/// there is a depth buffer, as under Qt.
fn choose_config(egl: &Egl, display: egl::Display, attrs: &[egl::Int]) -> Option<egl::Config> {
    let n = egl.matching_config_count(display, attrs).ok()?;
    let mut configs = Vec::with_capacity(n);
    egl.choose_config(display, attrs, &mut configs).ok()?;
    let want = [
        (egl::RED_SIZE, 8),
        (egl::GREEN_SIZE, 8),
        (egl::BLUE_SIZE, 8),
        (egl::ALPHA_SIZE, 8),
        (egl::DEPTH_SIZE, 24),
        (egl::STENCIL_SIZE, 8),
        (egl::SAMPLES, 0),
    ];
    let exact = configs.iter().copied().find(|&c| {
        want.iter()
            .all(|&(a, v)| egl.get_config_attrib(display, c, a).is_ok_and(|x| x == v))
    });
    exact.or_else(|| configs.first().copied())
}

/// Put in front of every fragment shader. GLSL ES 1.00 makes highp optional in fragment shaders
/// (§4.5.4: GL_FRAGMENT_PRECISION_HIGH is defined where it is supported) and using it elsewhere
/// fails to compile (Mali-400, Adreno 2xx), so `HP` is highp where available, mediump otherwise.
/// A shader may still pick a lower default with its own `precision` statement.
/// Samplers are lowp by default (§4.5.3), and a texel has the precision of its sampler. On
/// llvmpipe (Mesa 26.2) a texel sampled that way is handed over as a 16-bit float
/// (lower_precision.cpp) and came out up to 1/2048 of itself short: half the bytes times an
/// opacity, and 8-bit glyph coverage times a colour, were 1 of 255 lower than Qt's. So samplers
/// are `HP` as well.
const FS_PRECISION: &str = "#ifdef GL_FRAGMENT_PRECISION_HIGH
#define HP highp
#else
#define HP mediump
#endif
precision HP float;
precision HP sampler2D;
";

/// Compiles and links a program of `vs` and `fs`, with `attrs` at locations 0, 1, ...; None
/// (logged) when it does not build.
///
/// # Safety
/// The GL context `gl` was loaded for must be current on the calling thread.
unsafe fn program(gl: &glow::Context, vs: &str, fs: &str, attrs: &[&str]) -> Option<glow::Program> {
    // An #extension directive comes before anything that is not one (GLSL ES 1.00 §3.3).
    let (head, fs) = match fs.starts_with("#extension") {
        true => fs.split_at(fs.find('\n').map_or(0, |n| n + 1)),
        false => ("", fs),
    };
    #[cfg(test)]
    let fs = &*precision_tests::emulate(fs);
    let fs = format!("{head}{FS_PRECISION}{fs}");
    // SAFETY: `gl` is current, as this fn requires.
    let p = unsafe { gl.create_program() }.ok()?;
    for (src, kind, what) in [
        (vs, glow::VERTEX_SHADER, "vertex"),
        (fs.as_str(), glow::FRAGMENT_SHADER, "fragment"),
    ] {
        // SAFETY: `gl` is current, as this fn requires.
        let Ok(s) = (unsafe { gl.create_shader(kind) }) else {
            // Frees the vertex shader already attached (flagged for deletion) with it.
            // SAFETY: `gl` is current, as this fn requires.
            unsafe { gl.delete_program(p) };
            return None;
        };
        // SAFETY: `gl` is current, as this fn requires.
        unsafe { gl.shader_source(s, src) };
        // SAFETY: `gl` is current, as this fn requires.
        unsafe { gl.compile_shader(s) };
        // SAFETY: `gl` is current, as this fn requires.
        let ok = unsafe { gl.get_shader_compile_status(s) };
        if !ok {
            // SAFETY: `gl` is current, as this fn requires.
            let log = unsafe { gl.get_shader_info_log(s) };
            tracing::error!("gpu: {what} shader does not compile: {log}\n{src}");
            // SAFETY: `gl` is current, as this fn requires.
            unsafe { gl.delete_shader(s) };
            // SAFETY: `gl` is current, as this fn requires.
            unsafe { gl.delete_program(p) };
            return None;
        }
        // SAFETY: `gl` is current, as this fn requires.
        unsafe { gl.attach_shader(p, s) };
        // Flagged for deletion; freed with the program.
        // SAFETY: `gl` is current, as this fn requires.
        unsafe { gl.delete_shader(s) };
    }
    for (k, a) in attrs.iter().enumerate() {
        // SAFETY: `gl` is current, as this fn requires.
        unsafe { gl.bind_attrib_location(p, k as u32, a) };
    }
    // SAFETY: `gl` is current, as this fn requires.
    unsafe { gl.link_program(p) };
    // SAFETY: `gl` is current, as this fn requires.
    if !unsafe { gl.get_program_link_status(p) } {
        // SAFETY: `gl` is current, as this fn requires.
        let log = unsafe { gl.get_program_info_log(p) };
        tracing::error!("gpu: program does not link: {log}");
        // SAFETY: `gl` is current, as this fn requires.
        unsafe { gl.delete_program(p) };
        return None;
    }
    Some(p)
}

/// Clears every GL error flag set so far, logging each (`what` says where they came from).
/// glGetError reports and clears one recorded flag per call, oldest of any call first
/// (ES 2.0 §2.5): a call checked with it must start from this clean state. A lost context
/// keeps returning an error, hence the bound.
///
/// # Safety
/// The GL context `gl` was loaded for must be current on the calling thread.
unsafe fn drain_gl_errors(gl: &glow::Context, what: &str) {
    for _ in 0..8 {
        // SAFETY: `gl` is current, as this fn requires.
        let e = unsafe { gl.get_error() };
        if e == glow::NO_ERROR {
            break;
        }
        tracing::error!("gpu: GL error {e:#06x} {what}");
    }
}

/// Screencopy compare counters for `ri::stats`: trusted skips, unchanged, changed, dropped.
pub static DIFF_STATS: [std::sync::atomic::AtomicU32; 4] =
    [const { std::sync::atomic::AtomicU32::new(0) }; 4];

pub fn take_diff_stats() -> [u32; 4] {
    DIFF_STATS
        .each_ref()
        .map(|a| a.swap(0, std::sync::atomic::Ordering::Relaxed))
}

/// Copy of a compositor-written frame plus its diff reduction targets (`Gpu::frame_changed`).
pub struct FrameCopy {
    size: (i32, i32),
    view: (i32, i32),
    chain: Vec<(glow::Texture, glow::Framebuffer, i32, i32)>,
    scratch: (glow::Texture, glow::Framebuffer, i32, i32),
    /// Frames still reported changed without comparing (see `frame_changed`).
    trust: u8,
    /// Length of the current run of trusted frames: grows while compares keep finding changes.
    run: u8,
}

/// After a frame compares changed, the next frames are reported changed without the compare and
/// readback: content that changes keeps changing. The run starts at `DIFF_TRUST` frames and grows
/// to `DIFF_TRUST_MAX` while every compare finds a change, so an unchanged frame costs at most that
/// many identical redraws before the compare stops the loop. The copy is refreshed on a run's last
/// frame, so each compare is against the frame just before it.
const DIFF_TRUST: u8 = 3;
const DIFF_TRUST_MAX: u8 = 15;
/// Compares one `compare_read` reads back at most.
const DIFF_ROW: i32 = 64;
/// Bytes of `Gpu::vbo`, filled front to back by `Gpu::stream` and orphaned when full.
const STREAM_SIZE: usize = 1 << 20;

/// The fading previous frame (premultiplied) at opacity `o`, pixel for pixel.
/// The coordinate comes from a highp varying: GLSL ES 1.00 declares gl_FragCoord mediump, which
/// is fp16 on his RX 580 and can't hold x + 0.5 past x = 1024, so columns there shifted.
const FADE_VS: &str = "attribute vec2 p; varying highp vec2 q;
void main() { q = p * 0.5 + 0.5; gl_Position = vec4(p, 0.0, 1.0); }";
const FADE_FS: &str = "varying HP vec2 q; uniform sampler2D a;
uniform float o; void main() { gl_FragColor = texture2D(a, q) * o; }";
const DIFF_VS: &str = "attribute vec2 p; varying highp vec2 q;
void main() { q = p * 0.5 + 0.5; gl_Position = vec4(p, 0.0, 1.0); }";
/// m: 2 copy (resample) `a`, 1 mark 8x8 blocks where `a` and `b` differ, 0 max-reduce 8x8 blocks of `a`.
/// `s` is the size of `a`; the target is that size (m 2) or its count of 8x8 blocks (m 1, 0).
/// The target pixel `f` comes from a highp varying, as in FADE_VS: from the fp16 gl_FragCoord
/// the copy took even columns and rows only past 1024, and a change in the others went unseen.
const DIFF_FS: &str = "varying HP vec2 q; uniform sampler2D a; uniform sampler2D b;
uniform vec2 s; uniform float m;
void main() { vec2 f = floor(q * (m > 1.5 ? s : ceil(s / 8.0)));
 if (m > 1.5) { gl_FragColor = texture2D(a, (f + 0.5) / s); return; }
 float d = 0.0;
 for (int i = 0; i < 8; i++) for (int j = 0; j < 8; j++) {
  vec2 q = f * 8.0 + vec2(float(i), float(j));
  if (q.x < s.x && q.y < s.y) { vec2 c = (q + 0.5) / s; vec4 x = texture2D(a, c);
   if (m > 0.5) { vec4 e = abs(x - texture2D(b, c)); d = max(d, max(max(e.r, e.g), max(e.b, e.a))); }
   else d = max(d, x.r); } }
 gl_FragColor = vec4(d > 0.001 ? 1.0 : 0.0); }";

// `p` is the gl_Position qsg::raster's smooth_clip_* worked out, as Qt's shader gives it: a
// round trip through window pixels would not give the same floats back. `f` is -1 where the
// target is drawn upside down, its top in row 0 (`Gpu::flip`), else 1.
const COLOR_VS: &str = "attribute vec2 p; attribute vec4 c; uniform float f; varying vec4 v;
void main() { v = c; gl_Position = vec4(p.x, f * p.y, 0.0, 1.0); }";
const COLOR_FS: &str = "precision mediump float; varying vec4 v; void main() { gl_FragColor = v; }";
// shapecurve.vert is worked out on the processor (`qsg::shape::fill_vertices`): `p` its
// gl_Position, `t` its qt_TexCoord, `d` its texCoordDerivative. `f` as in COLOR_VS.
const CURVE_VS: &str = "attribute vec2 p; attribute vec4 t; attribute vec4 d; uniform float f;
varying vec4 u; varying vec4 g;
void main() { u = t; g = d; gl_Position = vec4(p.x, f * p.y, 0.0, 1.0); }";
// shapecurve.frag for a solid colour `c` under the opacity `o`. With screen-space derivatives
// (GLES 2.0 has them as an extension), Qt's USE_DERIVATIVES variant, which Qt takes wherever it
// draws with a graphics card; without, its other variant.
const CURVE_FS: &str = "#extension GL_OES_standard_derivatives : enable
varying vec4 u; varying vec4 g; uniform vec4 c; uniform float o;
void main() {
 float f = u.z * (u.x * u.x - u.y) + (1.0 - abs(u.z)) * (u.x - u.y);
#ifdef GL_OES_standard_derivatives
 float df = length(vec2(dFdx(f), dFdy(f)));
#else
 float dx = u.z * (2.0 * u.x * g.x - g.y) + (1.0 - abs(u.z)) * (g.x - g.y);
 float dy = u.z * (2.0 * u.x * g.z - g.w) + (1.0 - abs(u.z)) * (g.z - g.w);
 float df = length(vec2(dx, dy));
#endif
 int m = 1 - int(u.w != 0.0) * (int(u.x < 0.0) + int(u.x > 1.0));
 float k = clamp(0.5 + f / df, 0.0, 1.0) * float(m);
 gl_FragColor = vec4(c.rgb, 1.0) * c.a * k * o; }";
// The position times 2 / size, as by Qt's orthographic matrix: p / size * 2 puts a vertex
// between device pixels a float apart, and a texel read nearest in the next one.
const TEX_VS: &str = "attribute vec2 p; attribute vec2 t; uniform vec2 s; varying vec2 u;
void main() { u = t; gl_Position = vec4(p.x * (2.0 / s.x) - 1.0, sign(s.y) * (1.0 - p.y * (2.0 / abs(s.y))), 0.0, 1.0); }";
const TEX_FS: &str = "varying vec2 u; uniform sampler2D x; uniform sampler2D y;
uniform vec4 k; uniform float m; uniform float o;
void main() { vec4 c = texture2D(x, u);
 if (m > 3.5) { gl_FragColor = vec4(mix(c.rgb, vec3((c.r + c.g + c.b) / 3.0), k.x), c.a) * o; }
 else if (m > 2.5) { float a = texture2D(y, u).a; gl_FragColor = c * (k.x > 0.5 ? 1.0 - a : a) * o; }
 else if (m > 1.5) { float ca = max(c.a, 0.00390625); float ka = max(k.a, 0.00390625);
  gl_FragColor = vec4(mix(c.rgb / ca, k.rgb / ka, k.a) * c.a, c.a) * o; }
 else gl_FragColor = (m > 0.5 ? k * c.a : c) * o; }";
/// multieffect.frag (see `qsg::multieffect::draw_with`): e contrast brightness saturation
/// colorization, c colorization rgb and has shadow, k mask edges, f (has mask, inverted, opacity,
/// has blur), w and v.xy weights of the source and blur levels b1..b5; of the shadow
/// (`qsg::multieffect::Shadow`) q (scale, offset, center),
/// n its colour, sw and v.zw its weights. Nine vectors and seven samplers: the 16 fragment
/// uniform vectors GLES 2.0 guarantees (`effect_shader_fits_the_least_fragment_uniforms`).
const EFFECT_FS: &str = "varying vec2 u; uniform sampler2D x;
uniform sampler2D y; uniform vec4 e; uniform vec4 c; uniform vec4 k; uniform vec4 f;
uniform vec4 w; uniform vec4 v; uniform vec4 q; uniform vec4 n; uniform vec4 sw;
uniform sampler2D b1; uniform sampler2D b2; uniform sampler2D b3; uniform sampler2D b4; uniform sampler2D b5;
void main() { float mf = 1.0;
 if (f.x > 0.5) { float a = texture2D(y, u).a;
  float mm = smoothstep(k.x, k.y, a) * smoothstep(k.z, k.w, 1.0 - a); mf = f.y > 0.5 ? 1.0 - mm : mm; }
 vec4 p = texture2D(x, u);
 if (f.w > 0.5) p = p * w.x + texture2D(b1, u) * w.y + texture2D(b2, u) * w.z + texture2D(b3, u) * w.w
  + texture2D(b4, u) * v.x + texture2D(b5, u) * v.y;
 p.rgb = (p.rgb - 0.5 * p.a) * (1.0 + e.x) + 0.5 * p.a + e.y * p.a;
 float g = dot(p.rgb, vec3(0.299, 0.587, 0.114));
 p.rgb = g * c.rgb * e.w + p.rgb * (1.0 - e.w);
 p.rgb = g + (p.rgb - g) * (1.0 + e.z);
 if (c.w > 0.5) { vec2 su = (u - q.yz) * q.x + q.w;
  float sa = texture2D(x, su).a * sw.x + texture2D(b1, su).a * sw.y + texture2D(b2, su).a * sw.z
   + texture2D(b3, su).a * sw.w + texture2D(b4, su).a * v.z + texture2D(b5, su).a * v.w;
  p = mix(n, p, min(1.0, p.a + (1.0 - p.a) * (1.0 - sa))); }
 gl_FragColor = p * (mf * f.z); }";

/// BlurItem (bluritems.frag): four taps around the texel, `d` the item-pixel offset.
const BLUR_FS: &str = "varying vec2 u; uniform sampler2D x; uniform vec2 d;
void main() { vec2 a = vec2(d.x, d.y * 0.33); vec2 q = vec2(d.x * 0.33, -d.y);
 gl_FragColor = (texture2D(x, u + a) + texture2D(x, u + q) + texture2D(x, u - q) + texture2D(x, u - a)) * 0.25; }";

/// rectangularshadow.frag and rectangularshadow4r.frag: `u` the fragment in effect-item
/// units, `r` the item size, `q` the reduced half size of the rounded rect, `b` the blur, `k`
/// the top left, top right, bottom left and bottom right radius (all the same in the first
/// shader), of which the fragment's quadrant picks one as `int(2.0 + 0.5 * sign(x) + sign(y))`.
const RSHADOW_FS: &str = "varying vec2 u; uniform vec2 r; uniform vec2 q;
uniform float b; uniform vec4 k; uniform vec4 c; uniform float o;
void main() { vec2 p = u - r * 0.5; float i = 2.0 + 0.5 * sign(p.x) + sign(p.y);
 float a = i < 1.0 ? k.x : (i < 2.0 ? k.y : (i < 3.0 ? k.z : k.w));
 float d = length(max(abs(p) - q + a, 0.0)) - a;
 float t = b > 0.0 ? clamp(d / b, 0.0, 1.0) : (d > 0.0 ? 1.0 : 0.0);
 float v = 1.0 - t * t * (3.0 - 2.0 * t); gl_FragColor = c * (v * v * o); }";

/// radialgradient.vert: `t` the vertex in the item (0..1), `c` the centre, `a` the sine and
/// cosine of the angle, `r` the horizontal and vertical ratio. Where the gradient is read
/// (`v`, with the centre `w`) is the vertex shader's, interpolated over the quad, as in Qt.
const RADIAL_VS: &str = "attribute vec2 p; attribute vec2 t; uniform mat4 q;
uniform vec2 c; uniform vec2 a; uniform vec2 r; varying vec2 u; varying vec2 v; varying vec2 w;
void main() { u = t; vec2 d = t - c;
 v = (vec2(d.x * a.y - d.y * a.x, d.x * a.x + d.y * a.y) + c) * r; w = c * r;
 gl_Position = q * vec4(p, 0.0, 1.0); }";

/// radialgradient_nomask.frag or radialgradient_mask.frag: `x` the gradient texture read down
/// its height, `y` the `source` layer (bottom-up) the item covers `m.xy` of, when `m.z` is
/// set.
const RADIAL_FS: &str = "varying vec2 u; varying vec2 v; varying vec2 w;
uniform sampler2D x; uniform sampler2D y; uniform vec3 m; uniform float o;
void main() { vec4 g = texture2D(x, vec2(0.0, 2.0 * distance(v, w)));
 float k = m.z > 0.5 ? texture2D(y, vec2(u.x * m.x, 1.0 - u.y * m.y)).a : 1.0;
 gl_FragColor = g * (k * o); }";

impl Gpu {
    /// ri::wstat: (uploaded images, cached layers, their MiB, pooled targets).
    pub fn stats(&self) -> (usize, usize, i64, usize) {
        let b = self.layers.bytes;
        (
            self.images.len(),
            self.layers.len(),
            b >> 20,
            self.pool.len(),
        )
    }
}

fn load_egl() -> Option<Egl> {
    // SAFETY: libEGL's initializers have no preconditions; `Egl` keeps the library loaded.
    unsafe { Egl::load_required_from_filename("libEGL.so.1") }.ok()
}

const PLATFORM_DEVICE: egl::Enum = 0x313F;
const PLATFORM_SURFACELESS: egl::Enum = 0x31DD;
const DRM_DEVICE_FILE: egl::Int = 0x3233;

type QueryDevices = unsafe extern "system" fn(egl::Int, *mut *mut c_void, *mut egl::Int) -> u32;
type QueryDeviceString = unsafe extern "system" fn(*mut c_void, egl::Int) -> *const i8;
type PlatformDisplay =
    unsafe extern "system" fn(egl::Enum, *mut c_void, *const egl::Int) -> *mut c_void;

/// The EGL devices (handle, DRM node or "", software) of EGL_EXT_device_enumeration.
fn egl_devices(egl: &Egl) -> Vec<(*mut c_void, String, bool)> {
    let (Some(q), Some(qs)) = (
        ext_proc!(egl, "eglQueryDevicesEXT", QueryDevices),
        ext_proc!(egl, "eglQueryDeviceStringEXT", QueryDeviceString),
    ) else {
        return Vec::new();
    };
    let mut devs = [std::ptr::null_mut(); 16];
    let mut n = 0;
    // SAFETY: EGL_EXT_device_enumeration: `devs` has room for the `max_devices` passed and
    // `n` is a valid out pointer.
    if unsafe { q(devs.len() as egl::Int, devs.as_mut_ptr(), &mut n) } == 0 {
        return Vec::new();
    }
    let string = |d, name| {
        // SAFETY: EGL_EXT_device_query: `d` is a handle `q` returned.
        let p = unsafe { qs(d, name) };
        if p.is_null() {
            String::new()
        } else {
            // SAFETY: a non-null string from eglQueryDeviceStringEXT is NUL-terminated and
            // owned by EGL for the display's lifetime.
            unsafe { std::ffi::CStr::from_ptr(p) }
                .to_string_lossy()
                .into_owned()
        }
    };
    devs[..n.max(0) as usize]
        .iter()
        .map(|&d| {
            let soft = string(d, egl::EXTENSIONS)
                .split(' ')
                .any(|e| e == "EGL_MESA_device_software");
            (d, string(d, DRM_DEVICE_FILE), soft)
        })
        .collect()
}

/// The display `Gpu::headless` renders on, and what it is.
fn headless_display(egl: &Egl, platform: &str) -> Result<(egl::Display, String), String> {
    let client = egl
        .query_string(None, egl::EXTENSIONS)
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_default();
    let has = |e: &str| client.split(' ').any(|x| x == e);
    let get = ext_proc!(egl, "eglGetPlatformDisplayEXT", PlatformDisplay);
    let open = |plat, native| -> Option<egl::Display> {
        // SAFETY: EGL_EXT_platform_base: a platform enum, its native display (or null) and an
        // EGL_NONE-terminated attribute list.
        let d = unsafe { get?(plat, native, [egl::NONE].as_ptr()) };
        // SAFETY: a non-null EGLDisplay just returned by EGL.
        (!d.is_null()).then(|| unsafe { egl::Display::from_ptr(d) })
    };
    // `want`: an index, or the first software (true) / hardware (false) device.
    let device = |want: Result<usize, bool>| -> Result<(egl::Display, String), String> {
        if !has("EGL_EXT_platform_device") {
            return Err("no EGL_EXT_platform_device".into());
        }
        let devs = egl_devices(egl);
        let k = match want {
            Ok(k) => k,
            Err(soft) => devs.iter().position(|d| d.2 == soft).ok_or_else(|| {
                let kind = if soft { "software" } else { "hardware" };
                format!("no {kind} EGL device among {}", devs.len())
            })?,
        };
        let (d, node, soft) = devs
            .get(k)
            .ok_or_else(|| format!("EGL device {k} of {}", devs.len()))?;
        let disp = open(PLATFORM_DEVICE, *d).ok_or("eglGetPlatformDisplayEXT(device) failed")?;
        let kind = if *soft { "software" } else { "hardware" };
        Ok((disp, format!("device:{k} {kind} {node}")))
    };
    let surfaceless = || -> Result<(egl::Display, String), String> {
        if !has("EGL_MESA_platform_surfaceless") {
            return Err("no EGL_MESA_platform_surfaceless".into());
        }
        let disp = open(PLATFORM_SURFACELESS, std::ptr::null_mut())
            .ok_or("eglGetPlatformDisplayEXT(surfaceless) failed")?;
        Ok((disp, "surfaceless".into()))
    };
    let default = || -> Result<(egl::Display, String), String> {
        // SAFETY: eglGetDisplay needs no current context, and EGL_DEFAULT_DISPLAY is a display
        // id every EGL takes: it returns a default display (EGL 1.5 §3.2).
        let disp =
            unsafe { egl.get_display(egl::DEFAULT_DISPLAY) }.ok_or("no EGL_DEFAULT_DISPLAY")?;
        Ok((disp, "default".into()))
    };
    match platform {
        "auto" | "" => device(Err(false))
            .or_else(|e| surfaceless().map_err(|e2| format!("{e}; {e2}")))
            .or_else(|e| default().map_err(|e2| format!("{e}; {e2}"))),
        "device" => device(Err(false)),
        "software" => device(Err(true)),
        "surfaceless" => surfaceless(),
        "default" => default(),
        p => match p.strip_prefix("device:").and_then(|k| k.parse().ok()) {
            Some(k) => device(Ok(k)),
            None => Err(format!("unknown EGL platform {p:?}")),
        },
    }
}

impl Gpu {
    /// EGL + GLES2 on the client's `wl_display`; `None` means fall back to the raster path.
    ///
    /// # Safety
    /// `wl_display` must be a live `wl_display*` that outlives the returned `Gpu`.
    pub unsafe fn new(wl_display: *mut c_void) -> Option<Gpu> {
        let egl = Rc::new(load_egl()?);
        // SAFETY: `wl_display` is a live `wl_display*`, as this fn requires: a native display
        // for eglGetDisplay to look at (EGL 1.5 §3.2; an invalid one is undefined, §3.1.2).
        // The call needs no current context.
        let display = unsafe { egl.get_display(wl_display) }?;
        Self::on_display(egl, display, egl::WINDOW_BIT)
    }

    /// GLES2 without any window system: offscreen targets and `pbuffer` surfaces only.
    /// `platform`: `device[:N]` (EGL_EXT_platform_device: NVIDIA without X/Wayland, or a DRM
    /// node), `software` (Mesa's llvmpipe device), `surfaceless` (EGL_MESA_platform_surfaceless),
    /// `default` (EGL_DEFAULT_DISPLAY), or `auto`: the first hardware device, else surfaceless,
    /// else default. Also returns a description of the display it picked.
    pub fn headless(platform: &str) -> Result<(Gpu, String), String> {
        let egl = Rc::new(load_egl().ok_or("libEGL.so.1 not loadable")?);
        let (display, what) = headless_display(&egl, platform)?;
        let gpu = Self::on_display(egl, display, egl::PBUFFER_BIT)
            .ok_or_else(|| format!("{what}: no GLES2 context with pbuffers"))?;
        Ok((gpu, what))
    }

    fn on_display(egl: Rc<Egl>, display: egl::Display, surface: egl::Int) -> Option<Gpu> {
        let display_ref = DisplayRef::acquire(&egl, display)?;
        egl.bind_api(egl::OPENGL_ES_API).ok()?;
        let attrs = [
            egl::RED_SIZE,
            8,
            egl::GREEN_SIZE,
            8,
            egl::BLUE_SIZE,
            8,
            egl::ALPHA_SIZE,
            8,
            egl::SURFACE_TYPE,
            surface,
            egl::RENDERABLE_TYPE,
            egl::OPENGL_ES2_BIT,
            egl::DEPTH_SIZE,
            0,
            egl::STENCIL_SIZE,
            0,
            egl::NONE,
        ];
        let config = choose_config(&egl, display, &attrs)?;
        let exts = egl
            .query_string(Some(display), egl::EXTENSIONS)
            .map(|e| e.to_string_lossy().into_owned())
            .unwrap_or_default();
        let has = |x: &str| has_ext(&exts, x);
        // A GPU reset (driver crash, a suspend that drops video memory) loses the context
        // instead of leaving it drawing nothing, and `glGetGraphicsResetStatusEXT` reports it
        // (GL_EXT_robustness; without the strategy it always answers GL_NO_ERROR).
        let robust = has("EGL_EXT_create_context_robustness")
            .then(|| {
                let attrs = [
                    egl::CONTEXT_CLIENT_VERSION,
                    2,
                    EGL_CONTEXT_OPENGL_RESET_NOTIFICATION_STRATEGY_EXT,
                    EGL_LOSE_CONTEXT_ON_RESET_EXT,
                    egl::NONE,
                ];
                egl.create_context(display, config, None, &attrs).ok()
            })
            .flatten();
        let ctx = match robust {
            Some(ctx) => ctx,
            None => egl
                .create_context(
                    display,
                    config,
                    None,
                    &[egl::CONTEXT_CLIENT_VERSION, 2, egl::NONE],
                )
                .ok()?,
        };
        // Until the `Gpu` owns it, an early return releases and destroys the context (and with
        // it every GL object made on it so far).
        let ctx_guard = NewContext {
            egl: egl.clone(),
            display,
            ctx: Some(ctx),
        };
        egl.make_current(display, None, None, Some(ctx)).ok()?;
        let partial = !qml::debug::on(qml::debug::Switch::NODAMAGE);
        let buffer_age = partial && has("EGL_EXT_buffer_age");
        let swap_damage = swap_damage_name(&exts)
            .filter(|_| partial)
            .and_then(|n| ext_proc!(egl, n, SwapDamage));
        let fence = has("EGL_KHR_fence_sync")
            .then(|| {
                Some(FenceSync {
                    create: ext_proc!(egl, "eglCreateSyncKHR", CreateSync)?,
                    wait: ext_proc!(egl, "eglClientWaitSyncKHR", ClientWaitSync)?,
                    destroy: ext_proc!(egl, "eglDestroySyncKHR", DestroySync)?,
                    dup: has("EGL_ANDROID_native_fence_sync")
                        .then(|| ext_proc!(egl, "eglDupNativeFenceFDANDROID", DupNativeFence))
                        .flatten(),
                })
            })
            .flatten();
        // SAFETY: `ctx` was made current above; eglGetProcAddress returns the GL entry points
        // of the current client API, which glow casts to their specified signatures.
        let gl = unsafe {
            glow::Context::from_loader_function(|s| {
                egl.get_proc_address(s)
                    .map_or(std::ptr::null(), |p| p as *const c_void)
            })
        };
        // `GLenum glGetGraphicsResetStatusEXT(void)` (GL_EXT_robustness), taken only where the
        // context advertises the extension: eglGetProcAddress may answer for a name the
        // context does not implement.
        let clip_control = gl
            .supported_extensions()
            .contains("GL_EXT_clip_control")
            .then(|| ext_proc!(egl, "glClipControlEXT", ClipControl))
            .flatten();
        if clip_control.is_none() {
            // Once for the process: a context made again after a loss has the same driver.
            static SAID: std::sync::Once = std::sync::Once::new();
            SAID.call_once(|| {
                tracing::warn!(
                    target: "ri::gpu",
                    "gpu: no GL_EXT_clip_control: a blur's pixels with their centre on its \
                     edge may be a level off Qt's"
                );
            });
        }
        let reset_status = (robust.is_some()
            && gl.supported_extensions().contains("GL_EXT_robustness"))
        .then(|| ext_proc!(egl, "glGetGraphicsResetStatusEXT", ResetStatus))
        .flatten();
        let image = dmabuf_import_exts(&exts, gl.supported_extensions())
            .then(|| {
                Some(EglImage {
                    create: ext_proc!(egl, "eglCreateImageKHR", CreateImage)?,
                    target: ext_proc!(egl, "glEGLImageTargetTexture2DOES", TargetTex)?,
                    destroy: ext_proc!(egl, "eglDestroyImageKHR", DestroyImage)?,
                })
            })
            .flatten();
        // GLES 2.0 §3.8.2: a texture with a side that is not a power of two is incomplete
        // with a mipmap filter, unless the extension lifts that; GLES 3 has no such rule.
        let npot_mips =
            gl.version().major >= 3 || gl.supported_extensions().contains("GL_OES_texture_npot");
        // SAFETY: `make_current` above succeeded.
        let renderer = unsafe { gl.get_parameter_string(glow::RENDERER) };
        // SAFETY: `make_current` above succeeded.
        let color = unsafe { program(&gl, COLOR_VS, COLOR_FS, &["p", "c"]) }?;
        // SAFETY: `make_current` above succeeded.
        let color_f = unsafe { gl.get_uniform_location(color, "f") };
        // SAFETY: `make_current` above succeeded.
        let tex = unsafe { program(&gl, TEX_VS, TEX_FS, &["p", "t"]) }?;
        let tex_u = ["s", "k", "m", "o", "x", "y"].map(|n| {
            // SAFETY: `make_current` above succeeded.
            unsafe { gl.get_uniform_location(tex, n) }
        });
        // SAFETY: `make_current` above succeeded.
        let effect = unsafe { program(&gl, TEX_VS, EFFECT_FS, &["p", "t"]) }?;
        let effect_u = [
            "s", "e", "c", "k", "f", "x", "y", "w", "v", "q", "n", "sw", "b1", "b2", "b3", "b4",
            "b5",
        ]
        .map(|n| {
            // SAFETY: `make_current` above succeeded.
            unsafe { gl.get_uniform_location(effect, n) }
        });
        // SAFETY: `make_current` above succeeded.
        let blur = unsafe { program(&gl, TEX_VS, BLUR_FS, &["p", "t"]) }?;
        let blur_u = ["s", "x", "d"].map(|n| {
            // SAFETY: `make_current` above succeeded.
            unsafe { gl.get_uniform_location(blur, n) }
        });
        // SAFETY: `make_current` above succeeded.
        let rshadow = unsafe { program(&gl, TEX_VS, RSHADOW_FS, &["p", "t"]) }?;
        let rshadow_u = ["s", "r", "q", "b", "k", "c", "o"].map(|n| {
            // SAFETY: `make_current` above succeeded.
            unsafe { gl.get_uniform_location(rshadow, n) }
        });
        // SAFETY: `make_current` above succeeded.
        let curve = unsafe { program(&gl, CURVE_VS, CURVE_FS, &["p", "t", "d"]) }?;
        let curve_u = ["f", "c", "o"].map(|n| {
            // SAFETY: `make_current` above succeeded.
            unsafe { gl.get_uniform_location(curve, n) }
        });
        // SAFETY: `make_current` above succeeded.
        let radial = unsafe { program(&gl, RADIAL_VS, RADIAL_FS, &["p", "t"]) }?;
        let radial_u = ["c", "a", "r", "m", "o", "x", "y"].map(|n| {
            // SAFETY: `make_current` above succeeded.
            unsafe { gl.get_uniform_location(radial, n) }
        });
        // SAFETY: `make_current` above succeeded.
        let radial_q = unsafe { gl.get_uniform_location(radial, "q") };
        // SAFETY: `make_current` above succeeded.
        let vbo = unsafe { gl.create_buffer() }.ok()?;
        let mk = |gl: &glow::Context| -> Option<glow::Texture> {
            // SAFETY: `make_current` above succeeded.
            let t = unsafe { gl.create_texture() }.ok()?;
            // SAFETY: `make_current` above succeeded.
            unsafe { gl.bind_texture(glow::TEXTURE_2D, Some(t)) };
            for (k, v) in [
                (glow::TEXTURE_MIN_FILTER, glow::NEAREST),
                (glow::TEXTURE_MAG_FILTER, glow::NEAREST),
                (glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE),
                (glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE),
            ] {
                // SAFETY: `make_current` above succeeded.
                unsafe { gl.tex_parameter_i32(glow::TEXTURE_2D, k, v as i32) };
            }
            Some(t)
        };
        let scratch = mk(&gl)?;
        let atlas = mk(&gl)?;
        let catlas = mk(&gl)?;
        let dfatlas = mk(&gl)?;
        // SAFETY: `make_current` above succeeded.
        let max_tex = unsafe { gl.get_parameter_i32(glow::MAX_TEXTURE_SIZE) }.max(64);
        // SAFETY: `make_current` above succeeded.
        let max_frag_uniforms =
            unsafe { gl.get_parameter_i32(glow::MAX_FRAGMENT_UNIFORM_VECTORS) }.max(16) as usize;
        let atlas_of = |tex, rgba| Atlas {
            max: ATLAS.min(max_tex),
            ..Atlas::new(tex, rgba)
        };
        // SAFETY: `make_current` above succeeded.
        unsafe { gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1) };
        // SAFETY: `make_current` above succeeded.
        unsafe { warm_up(&gl) };
        if qml::log_on!("ri::stats") {
            tracing::debug!(
                target: "ri::stats",
                "gpu: buffer_age {buffer_age} swap_damage {} fence_sync {} reset_status {}",
                swap_damage.is_some(),
                fence.is_some(),
                reset_status.is_some()
            );
        }
        let mut st = State::default();
        for t in [scratch, atlas, catlas, dfatlas] {
            st.filters.insert(t, glow::NEAREST as i32);
        }
        let ctx = ctx_guard.keep();
        Some(Gpu {
            egl,
            display,
            config,
            ctx,
            gl,
            color,
            color_f,
            tex,
            vbo,
            stream_at: std::cell::Cell::new(STREAM_SIZE),
            tris_bufs: HashMap::new(),
            dead: Default::default(),
            tris_at: None,
            scratch,
            images: HashMap::new(),
            npot_mips,
            max_tex: max_tex as usize,
            clamped: Default::default(),
            max_frag_uniforms,
            vp: (1.0, 1.0),
            tex_u,
            effect,
            effect_u,
            blur,
            blur_u,
            gauss: HashMap::new(),
            rshadow,
            rshadow_u,
            curve,
            curve_u,
            radial,
            radial_u,
            radial_q,
            pool: Vec::new(),
            layers: LayerCache::new(),
            frame_no: 0,
            stamps: Default::default(),
            atlas: atlas_of(atlas, false),
            catlas: atlas_of(catlas, true),
            dfatlas: atlas_of(dfatlas, true),
            renderer,
            buffer_age,
            swap_damage,
            image,
            dmg: None,
            inc: LayerCache::new(),
            force_top: false,
            flip: false,
            upright: false,
            last_damage: [0; 4],
            fade_px: 0,
            fade_drawn: 0,
            last_uploads: 0,
            may_skip: false,
            swap_start: None,
            dbg: (Vec::new(), Vec::new(), 0),
            no_skip: false,
            diff: None,
            diff_row: None,
            fade_prog: None,
            diff_queued: 0,
            fence,
            diff_sync: None,
            #[cfg(test)]
            age_override: None,
            #[cfg(test)]
            swap_fails: false,
            #[cfg(test)]
            make_current_fails: false,
            #[cfg(test)]
            targets_left: std::cell::Cell::new(None),
            #[cfg(test)]
            bind_fails: None,
            lose_at: (surface == egl::WINDOW_BIT)
                .then(|| qml::debug::opts().gpuloss)
                .flatten()
                .filter(|_| !LOSS_INJECTED.swap(true, std::sync::atomic::Ordering::Relaxed)),
            binds: 0,
            reset_status,
            clip_control,
            failure: None,
            failed: 0,
            shown: 0,
            clock: None,
            st: std::cell::RefCell::new(st),
            gl_cache: !qml::debug::on(qml::debug::Switch::NOGLCACHE),
            _display_ref: display_ref,
        })
    }

    /// Makes the context current on `es` for a frame: `render`'s eglMakeCurrent, the
    /// `make_current` its SAFETY comments name.
    fn bind(&mut self, es: egl::Surface) -> Result<(), egl::Error> {
        self.binds += 1;
        if self.lose_at.is_some_and(|n| self.binds >= n) {
            return Err(egl::Error::ContextLost);
        }
        #[cfg(test)]
        if let Some(e) = self.bind_fails {
            return Err(e);
        }
        self.egl
            .make_current(self.display, Some(es), Some(es), Some(self.ctx))
    }

    /// The frame being rendered is not shown because `call` failed with `e`: says so in the
    /// log and keeps what it means for `take_failure`.
    fn fail(&mut self, call: &str, e: egl::Error) {
        self.failed += 1;
        let (line, kind) = failure(call, e, self.failed);
        elog::eprintln!("{line}");
        self.failure = Some(kind);
    }

    /// Why the last `render` showed nothing, when an EGL call failed (a frame skipped as
    /// unchanged is no failure). Asked once per `render`.
    pub fn take_failure(&mut self) -> Option<Failure> {
        self.failure.take()
    }

    /// Lowers the size past which images and layers are scaled down (`fit`, `texture`) to
    /// `max` texels a side, never below the atlases' side nor above the driver's limit. For
    /// tests: a layer clamped to a software driver's 16384 texels a side is 1 GiB to fill.
    pub fn cap_texture_size(&mut self, max: usize) {
        self.max_tex = self.max_tex.min(max.max(ATLAS as usize));
    }

    /// Whether this context ever showed a frame.
    pub fn shown(&self) -> bool {
        self.shown > 0
    }

    /// Drops a `Gpu` whose context is lost, with everything the thread knew about its
    /// textures (the shell runs one `Gpu` per thread): the images that are uploaded, the ones
    /// trading their pixels for their texture and the imported dmabuf textures. Its surfaces
    /// are dropped before it.
    pub fn retire(self) {
        UPLOADED.with_borrow_mut(|u| u.clear());
        ADOPT.with_borrow_mut(|a| a.clear());
        EXT.with_borrow_mut(|m| m.clear());
    }

    pub fn surface(&self, s: &WlSurface, w: i32, h: i32) -> Option<Surface> {
        let wl = wayland_egl::WlEglSurface::new(s.id(), w, h).ok()?;
        // SAFETY: eglCreateWindowSurface needs no current context (EGL 1.5 §3.5.1), only a live
        // native window of the display's platform: `wl.ptr()` is the wl_egl_window `wl` owns,
        // created just above on a surface of this display's wl_display, and `wl` moves into the
        // returned `Surface`, whose drop destroys the EGLSurface before it.
        let es = unsafe {
            self.egl
                .create_window_surface(self.display, self.config, wl.ptr() as *mut c_void, None)
        }
        .ok()?;
        // Owned from here: on any early return its drop destroys the EGLSurface, then the
        // wl_egl_window it was created on, which must outlive it.
        let surf = self.own(es, Some(wl), Some(s.clone()), (w, h));
        let bind = || {
            self.egl
                .make_current(self.display, Some(es), Some(es), Some(self.ctx))
        };
        #[cfg(test)]
        let bind = || {
            if self.make_current_fails {
                Err(egl::Error::BadAccess)
            } else {
                bind()
            }
        };
        bind().ok()?;
        // Never block the event loop on frame callbacks of hidden surfaces.
        let _ = self.egl.swap_interval(self.display, 0);
        Some(surf)
    }

    fn own(
        &self,
        es: egl::Surface,
        wl: Option<wayland_egl::WlEglSurface>,
        wl_surface: Option<WlSurface>,
        size: (i32, i32),
    ) -> Surface {
        Surface {
            egl: self.egl.clone(),
            display: self.display,
            es,
            wl,
            wl_surface,
            size,
            prev: Vec::new(),
            keep: Vec::new(),
            hist: Vec::new(),
            reswap: false,
            warm: false,
            fade: None,
            fresh: None,
            crossfade: qml::native::FadeArm::new(),
            tris: es.as_ptr() as usize,
            dead: self.dead.clone(),
        }
    }

    /// A w x h pbuffer standing in for a window (`Gpu::headless`): `render` draws to it as to a
    /// window surface, fades included, and `read_surface` reads it back. Its size is fixed.
    pub fn pbuffer(&self, w: i32, h: i32) -> Option<Surface> {
        let attrs = [egl::WIDTH, w, egl::HEIGHT, h, egl::NONE];
        let es = self
            .egl
            .create_pbuffer_surface(self.display, self.config, &attrs)
            .ok()?;
        Some(self.own(es, None, None, (w, h)))
    }

    /// The pixels `surf` shows (premultiplied RGBA, top row first).
    pub fn read_surface(&self, surf: &Surface) -> Canvas {
        let (w, h) = surf.size;
        let mut px = vec![0u8; (w * h * 4) as usize];
        if self
            .egl
            .make_current(self.display, Some(surf.es), Some(surf.es), Some(self.ctx))
            .is_ok()
        {
            let gl = &self.gl;
            // SAFETY: `make_current` above succeeded.
            unsafe { gl.bind_framebuffer(glow::FRAMEBUFFER, None) };
            // SAFETY: `make_current` above succeeded.
            unsafe { gl.pixel_store_i32(glow::PACK_ALIGNMENT, 1) };
            // SAFETY: `make_current` above succeeded.
            unsafe {
                gl.read_pixels(
                    0,
                    0,
                    w,
                    h,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelPackData::Slice(Some(&mut px)),
                )
            };
        }
        // GL rows are bottom-up.
        let row = (w * 4).max(1) as usize;
        let mut out = Vec::with_capacity((w * h).max(0) as usize);
        for r in px.chunks_exact(row).rev() {
            out.extend_from_slice(r.as_chunks::<4>().0);
        }
        Canvas {
            w: w as usize,
            h: h as usize,
            px: out,
            key: Default::default(),
        }
    }

    /// How many swaps ago `surf`'s back buffer was shown (EGL_EXT_buffer_age); 0 = its pixels
    /// are unknown. `surf` must be current.
    fn age(&self, surf: &Surface) -> i32 {
        let age = if self.buffer_age {
            self.egl
                .query_surface(self.display, surf.es, BUFFER_AGE)
                .unwrap_or(0)
        } else {
            0
        };
        // A pbuffer keeps its pixels across swaps: tests repair it as a window with this age.
        #[cfg(test)]
        let age = self.age_override.unwrap_or(age);
        age
    }

    /// Draws `fr` and swaps; false when nothing changed (`may_skip`) and no commit happens.
    /// An unchanged frame that may not be skipped commits the surface without a new buffer.
    /// `before_commit` runs only right before the commit, so per-commit surface state (a frame
    /// callback) is never requested for a frame that is skipped.
    pub fn render(
        &mut self,
        surf: &mut Surface,
        fr: &mut Frame,
        before_commit: impl FnOnce(),
    ) -> bool {
        let (w, h) = (fr.w as i32, fr.h as i32);
        let t0 = std::time::Instant::now();
        let full = [0, 0, w as i64, h as i64];
        self.last_uploads = 0;
        if surf.size != (w, h) {
            if let Some(wl) = &surf.wl {
                wl.resize(w, h, 0, 0);
            }
            surf.size = (w, h);
            surf.prev.clear();
            surf.hist.clear();
            self.end_fade(surf);
        }
        let changed = if surf.prev.is_empty() {
            Some(vec![full])
        } else {
            diff(&surf.prev, &fr.sig, full)
                .map(|d| region::clip(&d, full))
                .filter(|d| !d.is_empty())
        };
        let fade_area = surf.fade.as_ref().map(|f| f.area.clone());
        let whole_fade = matches!(fade_area, Some(None));
        // A fade's first frame is its start image at opacity 1 (`fade_opacity`) over what
        // changed: the frame the window shows, when the fade started from it. Nothing is drawn
        // or swapped for it, so the frame a fade starts in costs the copy of the start image
        // and not a second draw of the window; the fade's clock starts here all the same.
        let first = !surf.reswap
            && surf.fade.as_ref().is_some_and(|f| {
                f.shown
                    && f.start.is_none()
                    && match (&changed, &f.area) {
                        (Some(c), Some(a)) => region::inside(c, a),
                        _ => true,
                    }
            });
        if first {
            let now = self.now();
            if let Some(f) = surf.fade.as_mut() {
                f.start = Some(now);
            }
            self.may_skip = false;
        }
        let reuse = whole_fade && changed.is_none() && surf.fresh.is_some();
        let changed = match &fade_area {
            // The fading frame covers everything: the whole window changes every frame.
            Some(None) => Some(vec![full]),
            // Only the fading part changes with the fade's opacity.
            Some(Some(area)) => {
                let mut c = changed.unwrap_or_default();
                for r in area {
                    region::add(&mut c, *r);
                }
                Some(c)
            }
            None => changed,
        };
        if (changed.is_none() || first) && !surf.reswap {
            // The shown buffer already has this frame: no swap. A frame that still has to
            // reach the compositor (a veil step, a fade's first frame) is a commit without a
            // new buffer.
            if !first {
                surf.keep = fr.keep.clone();
            }
            self.last_damage = [0; 4];
            if std::mem::take(&mut self.may_skip) {
                return false;
            }
            before_commit();
            if let Some(s) = &surf.wl_surface {
                s.commit();
            }
            return true;
        }
        self.may_skip = false;
        surf.reswap = false;
        if let Err(e) = self.bind(surf.es) {
            self.fail("eglMakeCurrent", e);
            return false;
        }
        // SAFETY: `f` is glGetGraphicsResetStatusEXT of this context, which advertises
        // GL_EXT_robustness (`on_display` takes the entry point only then) and which `bind`
        // above made current; the call takes no argument.
        let reset = self.reset_status.map_or(0, |f| unsafe { f() });
        if reset != 0 {
            elog::eprintln!("gpu: the GPU was reset ({reset:#x}): the GPU context is lost");
            self.failure = Some(Failure::Context);
            return false;
        }
        self.reset_state();
        self.free_dead();
        let age = self.age(surf);
        let t1 = std::time::Instant::now();
        // The back buffer still shows the frame from `age` swaps ago: repaint what changed since.
        let changed = changed.unwrap_or_default();
        let mut region = changed.clone();
        match surf.stale(age) {
            Some(stale) => {
                for r in stale {
                    region::add(&mut region, r);
                }
            }
            None => region = vec![full],
        }
        if qml::debug::on(qml::debug::Switch::DMGCHECK) {
            self.dbg = (surf.prev.clone(), fr.sig.clone(), age);
        }
        let mut region = region::clip(&region, full);
        if region.is_empty() {
            // `reswap` of an unchanged frame: the swap is what it is for (see there), and
            // EGL_KHR_swap_buffers_with_damage reads n_rects 0 as the whole surface.
            region.push([0, 0, 1.min(w as i64), 1.min(h as i64)]);
        }
        // One scissored pass per damage rectangle; None = the whole window.
        let passes: Vec<Option<[i64; 4]>> = if region == [full] {
            vec![None]
        } else {
            region.iter().copied().map(Some).collect()
        };
        self.last_damage = region::bounds(&region).unwrap_or([0; 4]);
        // Textures whose source image nobody else references any more.
        self.adopt_resident();
        let gl = &self.gl;
        self.images.retain(|k, (rc, t)| {
            let keep = rc.strong_count() > 0;
            if !keep {
                // SAFETY: `make_current` above succeeded.
                unsafe { del_tex(gl, &self.st, *t) };
                UPLOADED.with_borrow_mut(|u| u.remove(k));
            }
            keep
        });
        // SAFETY: `make_current` above succeeded.
        unsafe { gl.enable(glow::BLEND) };
        // SAFETY: `make_current` above succeeded.
        unsafe { gl.blend_func(glow::ONE, glow::ONE_MINUS_SRC_ALPHA) };
        // SAFETY: `make_current` above succeeded.
        unsafe { self.bind_array(self.vbo) };
        if !reuse {
            // SAFETY: `make_current` above succeeded.
            unsafe { self.place_new_glyphs(&fr.cmds, &fr.boxes) };
            // SAFETY: `make_current` above succeeded.
            unsafe { self.prepare_layers(&fr.cmds, &fr.boxes) };
        }
        let gl = &self.gl;
        // SAFETY: `make_current` above succeeded.
        unsafe { gl.bind_framebuffer(glow::FRAMEBUFFER, None) };
        // SAFETY: `make_current` above succeeded.
        unsafe { gl.viewport(0, 0, w, h) };
        // SAFETY: `make_current` above succeeded.
        unsafe { gl.clear_color(0.0, 0.0, 0.0, 0.0) };
        if qml::log_on!("ri::gpuprof") {
            // SAFETY: `make_current` above succeeded.
            unsafe { self.gl.finish() };
            tracing::debug!(target: "ri::gpuprof", "  preexec {}us", t1.elapsed().as_micros());
        }
        if qml::log_on!("ri::cmdstat") {
            let tris = fr.cmds.iter().filter(|c| matches!(c, Cmd::Tris(_))).count();
            let verts: usize = fr
                .cmds
                .iter()
                .map(|c| match c {
                    Cmd::Tris(runs) => runs.iter().map(|r| r.len() / 6).sum(),
                    _ => 0,
                })
                .sum();
            tracing::debug!(
                target: "ri::cmdstat",
                "cmdstat {w}x{h} cmds {} tris {tris} verts {verts} rects {}",
                fr.cmds.len(),
                passes.len()
            );
        }
        for &d in &passes {
            self.dmg = d;
            // SAFETY: `make_current` above succeeded.
            unsafe { self.scissor(None, h, true) };
            // SAFETY: `make_current` above succeeded.
            unsafe { self.gl.clear(glow::COLOR_BUFFER_BIT) };
            self.tris_at = Some((surf.tris, 0));
            if let Some((tex, _)) = surf.fresh.filter(|_| reuse) {
                // SAFETY: `make_current` above succeeded.
                unsafe { self.draw_fade(tex, w, h, 1.0, None) };
            } else {
                // SAFETY: `make_current` above succeeded.
                unsafe { self.exec(&fr.cmds, &fr.boxes, None, w, h) };
            }
        }
        self.dmg = None;
        if whole_fade && !reuse {
            // SAFETY: `make_current` above succeeded.
            unsafe { self.keep_fresh(surf, w, h) };
        }
        self.tris_at = None;
        let (gl, n) = (&self.gl, self.frame_no);
        self.tris_bufs.retain(|_, (used, bufs)| {
            let keep = *used + 600 > n;
            if !keep {
                for (b, _) in bufs.drain(..) {
                    // SAFETY: `make_current` above succeeded.
                    unsafe { del_buf(gl, &self.st, b) };
                }
            }
            keep
        });
        if !surf.warm && surf.wl.is_some() {
            surf.warm = true;
            // SAFETY: `make_current` above succeeded.
            unsafe { self.warm_fade(w, h) };
        }
        let mut fade_t = None;
        if let Some(f) = surf.fade.as_mut() {
            let now = self.now();
            let start = *f.start.get_or_insert(now);
            f.elapsed = f.elapsed.max(now.saturating_duration_since(start));
        }
        if let Some((tex, t)) = surf.fade.as_ref().map(|f| (f.tex, f.progress())) {
            fade_t = Some(t);
            let o = fade_opacity(t);
            if o > 0.0 {
                match fade_area.flatten() {
                    // SAFETY: `make_current` above succeeded.
                    None => unsafe { self.draw_fade(tex, w, h, o, None) },
                    // The rectangles do not overlap (`begin_fade`), so each pixel is blended
                    // once, and this frame repainted all of them (`changed` above).
                    Some(area) => {
                        for r in area {
                            // SAFETY: `make_current` above succeeded.
                            unsafe { self.draw_fade(tex, w, h, o, Some(r)) };
                        }
                    }
                }
            }
            if t >= 1.0 {
                self.end_fade(surf);
                self.free_dead();
            }
        }
        let t2 = std::time::Instant::now();
        self.stamp();
        // SAFETY: `make_current` above succeeded.
        unsafe { self.evict_layers() };
        self.frame_no += 1;
        if qml::log_on!("ri::glcount") {
            print_gl_counts();
        }
        // SAFETY: `make_current` above succeeded.
        unsafe { self.gl.disable(glow::SCISSOR_TEST) };
        if qml::debug::on(qml::debug::Switch::DMGCHECK) {
            // SAFETY: `make_current` above succeeded.
            unsafe { self.check_damage(fr, w, h) };
        }
        if cfg!(debug_assertions) || qml::log_on!("ri::glerror") {
            // SAFETY: `make_current` above succeeded.
            unsafe { self.log_gl_errors() };
        }
        if qml::log_on!("ri::gputime") {
            // SAFETY: `make_current` above succeeded.
            unsafe { self.gl.finish() };
            let t3 = std::time::Instant::now();
            if (t3 - t0).as_millis() > 8 {
                tracing::debug!(
                    target: "ri::gputime",
                    "  render age {age} acquire {}us exec {}us finish {}us cmds {}",
                    (t1 - t0).as_micros(),
                    (t2 - t1).as_micros(),
                    (t3 - t2).as_micros(),
                    fr.cmds.len()
                );
            }
        }
        if let Some(t) = fade_t
            && qml::log_on!("ri::fade")
        {
            // SAFETY: `make_current` above succeeded.
            unsafe { self.gl.finish() };
            tracing::debug!(target: "ri::fade", "frame {w}x{h} t {t:.2} {}us", t0.elapsed().as_micros());
        }
        before_commit();
        self.swap_start = Some(std::time::Instant::now());
        let swapped = if let (false, Some(f)) = (passes == [None], self.swap_damage) {
            // EGL damage rects are x, y, width, height, bottom-up.
            let r: Vec<i32> = region
                .iter()
                .flat_map(|d| {
                    [
                        d[0] as i32,
                        h - d[3] as i32,
                        (d[2] - d[0]) as i32,
                        (d[3] - d[1]) as i32,
                    ]
                })
                .collect();
            let n = region.len() as i32;
            // SAFETY: `make_current` above succeeded, `f` is the display's
            // eglSwapBuffersWithDamage and `r` holds the `n` rects (4 ints each) it reads.
            if unsafe { f(self.display.as_ptr(), surf.es.as_ptr(), r.as_ptr(), n) } != 0 {
                Ok(())
            } else {
                Err(self.egl.get_error().unwrap_or(egl::Error::BadSurface))
            }
        } else {
            self.egl.swap_buffers(self.display, surf.es)
        };
        #[cfg(test)]
        let swapped = match swapped {
            Ok(()) if self.swap_fails => Err(egl::Error::BadSurface),
            r => r,
        };
        if let Err(e) = swapped {
            self.fail("eglSwapBuffers", e);
        }
        let swapped = swapped.is_ok();
        if swapped {
            self.failed = 0;
            self.shown += 1;
            surf.hist.insert(0, changed);
            surf.hist.truncate(4);
            // The frame stays intact: it may be shown again with swapped textures (`swap_tex`).
            surf.prev = fr.sig.clone();
            surf.keep = fr.keep.clone();
        } else {
            // EGL_BAD_SURFACE/EGL_BAD_NATIVE_WINDOW (output gone), EGL_CONTEXT_LOST: this frame
            // was never shown and the buffers hold nothing known, so the next one is drawn whole.
            surf.prev.clear();
            surf.hist.clear();
            surf.keep.clear();
        }
        swapped
    }

    /// Runs a draw list on framebuffer `fb` (None = the window surface) of size w x h.
    ///
    /// # Safety
    /// This `Gpu`'s context must be current on the calling thread, with `vbo` its bound array
    /// buffer (`bind_array`): the vertices are written to the bound buffer and drawn from it.
    unsafe fn exec(
        &mut self,
        cmds: &[Cmd],
        boxes: &[[f32; 4]],
        fb: Option<glow::Framebuffer>,
        w: i32,
        h: i32,
    ) {
        let mut scissor: Option<[i64; 4]> = None;
        let top = fb.is_none() || self.force_top;
        let outer = DRAWING.replace(true);
        let prof = qml::log_on!("ri::gpuprof");
        let mut last: Option<(std::time::Instant, std::mem::Discriminant<Cmd>, [f32; 4])> = None;
        for (c, b) in cmds.iter().zip(boxes) {
            if prof {
                // SAFETY: the context is current, as this fn requires.
                unsafe { self.gl.finish() };
                if let Some((t, k, lb)) = last.take()
                    && t.elapsed().as_micros() > 150
                {
                    tracing::debug!(target: "ri::gpuprof", "  gpucmd {k:?} {lb:?} {}us", t.elapsed().as_micros());
                }
                last = Some((std::time::Instant::now(), std::mem::discriminant(c), *b));
            }
            if top && !matches!(c, Cmd::Scissor(_)) && self.outside_damage(*b) {
                // Each damage pass reuses the window's buffer of the same `Cmd::Tris`.
                if fb.is_none()
                    && matches!(c, Cmd::Tris(_))
                    && let Some((_, k)) = self.tris_at.as_mut()
                {
                    *k += 1;
                }
                continue;
            }
            if matches!(c, Cmd::Layer { .. } | Cmd::Tint { .. } | Cmd::Gauss { .. })
                && covers_no_pixel(*b)
                && !self.no_skip
            {
                // e.g. a popup scaling in from ~0: its texture would be rendered for nothing.
                if qml::debug::on(qml::debug::Switch::SKIPCHECK) {
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.check_skip(c, *b, fb, w, h, scissor, top) };
                }
                continue;
            }
            match c {
                // SAFETY: the context is current, as this fn requires.
                Cmd::Tris(v) => unsafe { self.draw_tris(v) },
                // SAFETY: the context is current, as this fn requires.
                Cmd::Glyphs(col, q) => unsafe { self.draw_glyphs(*col, q, w, h) },
                Cmd::GlyphsF(col, q) => {
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.draw_glyph_rects(*col, q.iter().map(|(g, r)| (g, *r)), w, h) }
                }
                Cmd::Df(k, tiles) => {
                    // QSGHiQSubPixelDistanceFieldTextMaterial: ConstantColor / OneMinusSrcColor;
                    // gray tiles hold their colour and blend as everything else does.
                    if let Some(k) = k {
                        // SAFETY: the context is current, as this fn requires.
                        unsafe { self.gl.blend_color(k[0], k[1], k[2], 1.0) };
                        // SAFETY: the context is current, as this fn requires.
                        unsafe {
                            self.gl.blend_func_separate(
                                glow::CONSTANT_COLOR,
                                glow::ONE_MINUS_SRC_COLOR,
                                glow::ONE,
                                glow::ONE_MINUS_SRC_ALPHA,
                            )
                        };
                    }
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.df_pass(tiles, w, h) };
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.gl.blend_func(glow::ONE, glow::ONE_MINUS_SRC_ALPHA) };
                }
                // SAFETY: the context is current, as this fn requires.
                Cmd::Blit(img, x, y) => unsafe { self.draw_blit(img, *x, *y, w, h) },
                Cmd::Scissor(r) => {
                    scissor = *r;
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.scissor(scissor, h, top) };
                }
                Cmd::Masked {
                    src,
                    mask,
                    rect,
                    crop,
                    invert,
                    op,
                    id,
                } => {
                    let a = if *id != 0 {
                        // SAFETY: the context is current, as this fn requires.
                        unsafe { self.inc_layer_tex(*id, src) }
                    } else {
                        // SAFETY: the context is current, as this fn requires.
                        unsafe { self.layer_tex(src) }
                    };
                    // SAFETY: the context is current, as this fn requires.
                    let b = unsafe { self.layer_tex(mask) };
                    let (Some(a), Some(b)) = (a, b) else {
                        continue;
                    };
                    let gl = &self.gl;
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { gl.bind_framebuffer(glow::FRAMEBUFFER, fb) };
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.viewport(w, h) };
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.scissor(scissor, h, top) };
                    let gl = &self.gl;
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { gl.active_texture(glow::TEXTURE1) };
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { gl.bind_texture(glow::TEXTURE_2D, Some(b)) };
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.filter(b, glow::LINEAR as i32) };
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { gl.active_texture(glow::TEXTURE0) };
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { gl.bind_texture(glow::TEXTURE_2D, Some(a)) };
                    // OpacityMask samples its source linearly (unlike a plain layer).
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.filter(a, glow::LINEAR as i32) };
                    let mut v = Vec::with_capacity(24);
                    // Like QSGLayer's sourceRect, the item spans its own width/height of the
                    // layer, not the whole ceil-sized texture.
                    quad(
                        &mut v,
                        rect[0],
                        rect[1],
                        rect[2],
                        rect[3],
                        [0.0, 0.0, crop[0], crop[1]],
                    );
                    let k = [if *invert { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0];
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.quads(&v, a, k, MASKED, *op, w, h) };
                }
                Cmd::Effect {
                    src,
                    mask,
                    mask_smooth,
                    rect,
                    u,
                    op,
                    blur,
                    shadow,
                } => {
                    let src = match blur {
                        // SAFETY: the context is current, as this fn requires.
                        Some(bl) => unsafe { self.blur_levels(src, bl) },
                        // SAFETY: the context is current, as this fn requires.
                        None => unsafe { self.layer_tex(src) }.map(|a| (a, Vec::new())),
                    };
                    let b = match mask {
                        // SAFETY: the context is current, as this fn requires.
                        Some(m) => match unsafe { self.layer_tex(m) } {
                            Some(b) => Some(b),
                            None => continue,
                        },
                        None => None,
                    };
                    let Some((a, levels)) = src else {
                        continue;
                    };
                    let gl = &self.gl;
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { gl.bind_framebuffer(glow::FRAMEBUFFER, fb) };
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.viewport(w, h) };
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.scissor(scissor, h, top) };
                    let gl = &self.gl;
                    let mut units = vec![(glow::TEXTURE1, b.unwrap_or(a))];
                    for k in 0..5 {
                        units.push((
                            glow::TEXTURE2 + k as u32,
                            levels.get(k).copied().unwrap_or(a),
                        ));
                    }
                    units.push((glow::TEXTURE0, a));
                    for (unit, t) in units {
                        // SAFETY: the context is current, as this fn requires.
                        unsafe { gl.active_texture(unit) };
                        // SAFETY: the context is current, as this fn requires.
                        unsafe { gl.bind_texture(glow::TEXTURE_2D, Some(t)) };
                        let nearest = unit == glow::TEXTURE1 && b.is_some() && !*mask_smooth;
                        let f = if nearest { glow::NEAREST } else { glow::LINEAR };
                        // SAFETY: the context is current, as this fn requires.
                        unsafe { self.filter(t, f as i32) };
                    }
                    let mut v = Vec::with_capacity(24);
                    // The source is stretched over the effect rect.
                    quad(
                        &mut v,
                        rect[0],
                        rect[1],
                        rect[2],
                        rect[3],
                        [0.0, 0.0, 1.0, 1.0],
                    );
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.use_prog(self.effect) };
                    let e = &self.effect_u;
                    let on = |b: bool| if b { 1.0 } else { 0.0 };
                    let wt = blur.as_ref().map_or([0.0; 6], |b| b.weights);
                    let sh = shadow.as_ref();
                    let sw = sh.map_or([0.0; 6], |s| s.weights);
                    let vals = [
                        Uni::f(&self.space(w, h)),
                        Uni::f(&u[0..4]),
                        Uni::f(&[u[4], u[5], u[6], on(sh.is_some())]),
                        Uni::f(&u[7..11]),
                        Uni::f(&[on(b.is_some()), u[11], *op, on(blur.is_some())]),
                        Uni::I(0),
                        Uni::I(1),
                        Uni::f(&wt[0..4]),
                        Uni::f(&[wt[4], wt[5], sw[4], sw[5]]),
                        Uni::f(
                            &sh.map_or([0.0; 4], |s| [s.scale, s.offset[0], s.offset[1], s.center]),
                        ),
                        Uni::f(&sh.map_or([0.0; 4], |s| s.color)),
                        Uni::f(&sw[0..4]),
                    ];
                    for (k, v) in vals.into_iter().enumerate() {
                        // SAFETY: the context is current, as this fn requires.
                        unsafe { self.set_u(P_EFFECT, k, e[k].as_ref(), v) };
                    }
                    for k in 0..5 {
                        // SAFETY: the context is current, as this fn requires.
                        unsafe {
                            self.set_u(P_EFFECT, 12 + k, e[12 + k].as_ref(), Uni::I(2 + k as i32))
                        };
                    }
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.draw_tex_quads(&v) };
                }
                Cmd::Gauss {
                    src,
                    g,
                    size,
                    rect,
                    qt,
                    op,
                } => {
                    // SAFETY: the context is current, as this fn requires.
                    let Some((a, rect, flipped)) =
                        (unsafe { self.gauss_tex(src, g, *size, *rect, qt.as_ref()) })
                    else {
                        continue;
                    };
                    let gl = &self.gl;
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { gl.bind_framebuffer(glow::FRAMEBUFFER, fb) };
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.viewport(w, h) };
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.scissor(scissor, h, top) };
                    let gl = &self.gl;
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { gl.active_texture(glow::TEXTURE0) };
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { gl.bind_texture(glow::TEXTURE_2D, Some(a)) };
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.filter(a, glow::LINEAR as i32) };
                    let mut v = Vec::with_capacity(24);
                    let uv = if flipped {
                        [0.0, 0.0, 1.0, 1.0]
                    } else {
                        [0.0, 1.0, 1.0, -1.0]
                    };
                    quad(&mut v, rect[0], rect[1], rect[2], rect[3], uv);
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.quads(&v, a, [0.0; 4], PLAIN, *op, w, h) };
                }
                Cmd::Curve { v, c, op } => {
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.gl.bind_framebuffer(glow::FRAMEBUFFER, fb) };
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.viewport(w, h) };
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.scissor(scissor, h, top) };
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.use_prog(self.curve) };
                    let f = if self.flip { -1.0 } else { 1.0 };
                    let vals = [Uni::f(&[f]), Uni::f(c), Uni::f(&[*op])];
                    for (i, val) in vals.into_iter().enumerate() {
                        // SAFETY: the context is current, as this fn requires.
                        unsafe { self.set_u(P_CURVE, i, self.curve_u[i].as_ref(), val) };
                    }
                    // SAFETY: the context is current, as this fn requires.
                    let o = unsafe { self.stream(as_bytes(v)) };
                    for (k, (size, off)) in [(2, 0), (4, 8), (4, 24)].into_iter().enumerate() {
                        // SAFETY: the context is current, as this fn requires.
                        unsafe { self.attrib(k as u32, true) };
                        // SAFETY: the context is current, as this fn requires.
                        unsafe { self.attrib_ptr(k as u32, size, 40, o + off) };
                    }
                    // SAFETY: the context is current, as this fn requires.
                    unsafe {
                        self.gl
                            .draw_arrays(glow::TRIANGLES, 0, (v.len() / 10) as i32)
                    };
                    // No other program has a third attribute.
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.attrib(2, false) };
                }
                Cmd::RShadow { rect, u, k, c, op } => {
                    let gl = &self.gl;
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { gl.bind_framebuffer(glow::FRAMEBUFFER, fb) };
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.viewport(w, h) };
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.scissor(scissor, h, top) };
                    let mut v = Vec::with_capacity(24);
                    // The varying runs over the effect item's own units.
                    let uv = [0.0, 0.0, u[0], u[1]];
                    quad(&mut v, rect[0], rect[1], rect[2], rect[3], uv);
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.use_prog(self.rshadow) };
                    let loc = &self.rshadow_u;
                    let vals = [
                        Uni::f(&self.space(w, h)),
                        Uni::f(&u[0..2]),
                        Uni::f(&u[2..4]),
                        Uni::f(&u[4..5]),
                        Uni::f(k),
                        Uni::f(c),
                        Uni::f(&[*op]),
                    ];
                    for (i, val) in vals.into_iter().enumerate() {
                        // SAFETY: the context is current, as this fn requires.
                        unsafe { self.set_u(P_RSHADOW, i, loc[i].as_ref(), val) };
                    }
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.draw_tex_quads(&v) };
                }
                Cmd::Radial {
                    tex,
                    mask,
                    crop,
                    rect,
                    qt,
                    r,
                    op,
                } => {
                    // SAFETY: the context is current, as this fn requires.
                    let Some(t) = (unsafe { self.texture(tex) }) else {
                        continue;
                    };
                    let m = match mask {
                        // SAFETY: the context is current, as this fn requires.
                        Some(m) => match unsafe { self.layer_tex(m) } {
                            Some(m) => Some(m),
                            None => continue,
                        },
                        None => None,
                    };
                    let gl = &self.gl;
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { gl.bind_framebuffer(glow::FRAMEBUFFER, fb) };
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.viewport(w, h) };
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.scissor(scissor, h, top) };
                    let gl = &self.gl;
                    if let Some(m) = m {
                        // SAFETY: the context is current, as this fn requires.
                        unsafe { gl.active_texture(glow::TEXTURE1) };
                        // SAFETY: the context is current, as this fn requires.
                        unsafe { gl.bind_texture(glow::TEXTURE_2D, Some(m)) };
                        // SAFETY: the context is current, as this fn requires.
                        unsafe { self.filter(m, glow::LINEAR as i32) };
                    }
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { gl.active_texture(glow::TEXTURE0) };
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { gl.bind_texture(glow::TEXTURE_2D, Some(t)) };
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.filter(t, glow::LINEAR as i32) };
                    let mut v = Vec::with_capacity(24);
                    grid(&mut v, qt.as_ref().map_or(*rect, |q| q.item));
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.use_prog(self.radial) };
                    // The target's pixels to its clip space, or Qt's own matrix; a target
                    // kept top row first turns either over.
                    let [sw, sh] = self.space(w, h);
                    #[rustfmt::skip]
                    let mut pm = qt.as_ref().map_or([
                        2.0 / sw, 0.0, 0.0, 0.0,
                        0.0, -2.0 / sh.abs(), 0.0, 0.0,
                        0.0, 0.0, 1.0, 0.0,
                        -1.0, 1.0, 0.0, 1.0,
                    ], |q| q.m);
                    if sh < 0.0 {
                        for k in [1, 5, 9, 13] {
                            pm[k] = -pm[k];
                        }
                    }
                    gl_count(GL_UNIFORM);
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { gl.uniform_matrix_4_f32_slice(self.radial_q.as_ref(), false, &pm) };
                    let loc = &self.radial_u;
                    let has = if m.is_some() { 1.0 } else { 0.0 };
                    let vals = [
                        Uni::f(&r.center),
                        Uni::f(&[r.sin, r.cos]),
                        Uni::f(&r.ratio),
                        Uni::f(&[crop[0], crop[1], has]),
                        Uni::f(&[*op]),
                        Uni::I(0),
                        Uni::I(1),
                    ];
                    for (i, val) in vals.into_iter().enumerate() {
                        // SAFETY: the context is current, as this fn requires.
                        unsafe { self.set_u(P_RADIAL, i, loc[i].as_ref(), val) };
                    }
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.draw_tex_quads(&v) };
                }
                Cmd::Tint {
                    src,
                    rect,
                    crop,
                    k,
                    mode,
                    smooth,
                    op,
                } => {
                    // SAFETY: the context is current, as this fn requires.
                    let Some(a) = (unsafe { self.layer_tex(src) }) else {
                        continue;
                    };
                    let gl = &self.gl;
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { gl.bind_framebuffer(glow::FRAMEBUFFER, fb) };
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.viewport(w, h) };
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.scissor(scissor, h, top) };
                    let gl = &self.gl;
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { gl.active_texture(glow::TEXTURE0) };
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { gl.bind_texture(glow::TEXTURE_2D, Some(a)) };
                    let f = if *smooth { glow::LINEAR } else { glow::NEAREST };
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.filter(a, f as i32) };
                    let mut v = Vec::with_capacity(24);
                    quad(
                        &mut v,
                        rect[0],
                        rect[1],
                        rect[2],
                        rect[3],
                        [0.0, 0.0, crop[0], crop[1]],
                    );
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.quads(&v, a, *k, *mode, *op, w, h) };
                }
                Cmd::Layer {
                    src,
                    m,
                    p: pr,
                    op,
                    crop,
                } => {
                    let (tw, th) = (src.w.max(1) as f32, src.h.max(1) as f32);
                    // SAFETY: the context is current, as this fn requires.
                    let Some(a) = (unsafe { self.layer_tex(src) }) else {
                        continue;
                    };
                    let gl = &self.gl;
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { gl.bind_framebuffer(glow::FRAMEBUFFER, fb) };
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.viewport(w, h) };
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.scissor(scissor, h, top) };
                    let gl = &self.gl;
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { gl.active_texture(glow::TEXTURE0) };
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { gl.bind_texture(glow::TEXTURE_2D, Some(a)) };
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.filter(a, glow::LINEAR as i32) };
                    let p =
                        |u: f32, v: f32| (m[0] * u + m[2] * v + m[4], m[1] * u + m[3] * v + m[5]);
                    let mut v = Vec::with_capacity(24);
                    let [u0, v0, u1, v1] = *crop;
                    if *pr == [0.0, 0.0, 1.0] {
                        for (u, t) in [(u0, v0), (u1, v0), (u0, v1), (u1, v0), (u1, v1), (u0, v1)] {
                            let (x, y) = p(u, t);
                            v.extend_from_slice(&[x, y, u / tw, t / th]);
                        }
                    } else {
                        // The vertices carry no w: a grid fine enough that straight texture
                        // steps inside a cell follow the perspective. Cells reaching behind
                        // the viewer are left out.
                        let at = |a: f32, b: f32| {
                            let (u, t) = (u0 + (u1 - u0) * a, v0 + (v1 - v0) * b);
                            let w = pr[0] * u + pr[1] * t + pr[2];
                            let (x, y) = p(u, t);
                            (w > PROJ_NEAR).then(|| [x / w, y / w, u / tw, t / th])
                        };
                        let n = PROJ_GRID as f32;
                        for gy in 0..PROJ_GRID {
                            for gx in 0..PROJ_GRID {
                                let (a0, b0) = (gx as f32 / n, gy as f32 / n);
                                let (a1, b1) = ((gx + 1) as f32 / n, (gy + 1) as f32 / n);
                                if let (Some(c00), Some(c10), Some(c01), Some(c11)) =
                                    (at(a0, b0), at(a1, b0), at(a0, b1), at(a1, b1))
                                {
                                    for c in [c00, c10, c01, c10, c11, c01] {
                                        v.extend_from_slice(&c);
                                    }
                                }
                            }
                        }
                    }
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.quads(&v, a, [0.0; 4], PLAIN, *op, w, h) };
                }
                Cmd::Tex {
                    src,
                    rect,
                    uv,
                    k,
                    op,
                    mode,
                    smooth,
                    mipmap,
                } => {
                    // SAFETY: the context is current, as this fn requires.
                    let Some(t) = (unsafe { self.texture(src) }) else {
                        continue;
                    };
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.gl.bind_texture(glow::TEXTURE_2D, Some(t)) };
                    // SAFETY: the context is current and `t` is bound, as this fn requires.
                    let f = match (*mipmap && unsafe { self.mipmapped(src, t) }, *smooth) {
                        (true, true) => glow::LINEAR_MIPMAP_LINEAR,
                        (true, false) => glow::NEAREST_MIPMAP_LINEAR,
                        (false, true) => glow::LINEAR,
                        (false, false) => glow::NEAREST,
                    } as i32;
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.filter(t, f) };
                    let uv = match ext(src) {
                        Some((_, true)) => [uv[0], 1.0 - uv[1], uv[2], -uv[3]],
                        _ => *uv,
                    };
                    let v = strip(rect[0], rect[1], rect[2], rect[3], uv);
                    if *mode == UNBLENDED {
                        // SAFETY: the context is current, as this fn requires.
                        unsafe { self.gl.disable(glow::BLEND) };
                        // SAFETY: the context is current, as this fn requires.
                        unsafe { self.quads(&v, t, *k, PLAIN, 1.0, w, h) };
                        // SAFETY: the context is current, as this fn requires.
                        unsafe { self.gl.enable(glow::BLEND) };
                        continue;
                    }
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.quads(&v, t, *k, *mode, *op, w, h) };
                }
            }
        }
        DRAWING.set(outer);
    }

    /// Debug (RI_DEBUG=skipcheck): a skipped command must draw nothing on its own.
    ///
    /// # Safety
    /// This `Gpu`'s context must be current on the calling thread, with `vbo` its bound array
    /// buffer (`bind_array`): the vertices are written to the bound buffer and drawn from it. The
    /// buffer glReadPixels fills (GLES 2.0 §4.3.1) is sized `w * h * 4`, which must not overflow an
    /// `i32`.
    #[allow(clippy::too_many_arguments)]
    unsafe fn check_skip(
        &mut self,
        c: &Cmd,
        b: [f32; 4],
        fb: Option<glow::Framebuffer>,
        w: i32,
        h: i32,
        scissor: Option<[i64; 4]>,
        top: bool,
    ) {
        let mut f = Frame::new(w as usize, h as usize);
        f.push(c.clone(), ANY_BOX, 0);
        self.no_skip = true;
        // SAFETY: the context is current, as this fn requires.
        let t = unsafe { self.offscreen(&f) };
        self.no_skip = false;
        let Some(t) = t else {
            return;
        };
        let mut px = vec![0u8; (w * h * 4) as usize];
        // SAFETY: the context is current, as this fn requires.
        unsafe {
            self.gl.read_pixels(
                0,
                0,
                w,
                h,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut px)),
            )
        };
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.release(t, (w, h)) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.gl.bind_framebuffer(glow::FRAMEBUFFER, fb) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.gl.viewport(0, 0, w, h) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.gl.enable(glow::BLEND) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.gl.blend_func(glow::ONE, glow::ONE_MINUS_SRC_ALPHA) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.scissor(scissor, h, top) };
        let n = px
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p.iter().any(|v| *v != 0))
            .count();
        tracing::warn!(target: "ri::check", "skipcheck {b:?} drawn {n}");
    }

    /// Debug: the partially repainted back buffer must equal a full repaint of the frame.
    ///
    /// # Safety
    /// This `Gpu`'s context must be current on the calling thread, with `vbo` its bound array
    /// buffer (`bind_array`): the vertices are written to the bound buffer and drawn from it. The
    /// buffer glReadPixels fills (GLES 2.0 §4.3.1) is sized `w * h * 4`, which must not overflow an
    /// `i32`.
    unsafe fn check_damage(&mut self, fr: &Frame, w: i32, h: i32) {
        let read = |gl: &glow::Context| {
            let mut px = vec![0u8; (w * h * 4) as usize];
            // SAFETY: the context is current, as this fn requires.
            unsafe {
                gl.read_pixels(
                    0,
                    0,
                    w,
                    h,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelPackData::Slice(Some(&mut px)),
                )
            };
            px
        };
        let part = read(&self.gl);
        self.upright = true;
        // SAFETY: the context is current, as this fn requires.
        let Some(t) = (unsafe { self.offscreen(fr) }) else {
            return;
        };
        let whole = read(&self.gl);
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.release(t, (w, h)) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.gl.bind_framebuffer(glow::FRAMEBUFFER, None) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.gl.viewport(0, 0, w, h) };
        let (mut bad, mut max, mut bb) = (0usize, 0u8, None);
        for (k, (a, b)) in part
            .as_chunks::<4>()
            .0
            .iter()
            .zip(whole.as_chunks::<4>().0)
            .enumerate()
        {
            let d = a
                .iter()
                .zip(b)
                .map(|(x, y)| x.abs_diff(*y))
                .max()
                .unwrap_or(0);
            if d > 2 {
                bad += 1;
                let (x, y) = ((k as i32 % w) as i64, (h - 1 - k as i32 / w) as i64);
                bb = union(bb, [x, y, x + 1, y + 1]);
            }
            max = max.max(d);
        }
        if bad > 0 && qml::log_on!("ri::dmgdump") {
            thread_local!(static N: std::cell::Cell<u32> = const { std::cell::Cell::new(0) });
            let n = N.with(|c| c.replace(c.get() + 1));
            if n < 3 {
                for (tag, px) in [("part", &part), ("whole", &whole)] {
                    let mut out = format!("P6 {w} {h} 255\n").into_bytes();
                    for row in (0..h).rev() {
                        for p in px[(row * w * 4) as usize..((row + 1) * w * 4) as usize].chunks(4)
                        {
                            out.extend_from_slice(&p[..3]);
                        }
                    }
                    crate::ipc::write_debug_file(&format!("dmg{n}_{tag}.ppm"), &out);
                }
            }
        }
        if bad > 0 {
            tracing::warn!(
                target: "ri::check",
                "dmgcheck {w}x{h} age {} damage {:?} bad {bad} max {max} at {bb:?}",
                self.dbg.2, self.last_damage
            );
            if let Some(b) = bb {
                for (tag, l) in [("prev", &self.dbg.0), ("cur", &self.dbg.1)] {
                    for e in l.iter().filter(|e| {
                        intersect(e.1, b)[2] > intersect(e.1, b)[0]
                            && intersect(e.1, b)[3] > intersect(e.1, b)[1]
                    }) {
                        tracing::warn!(target: "ri::check", "  {tag} {:016x} {:?}", e.0, e.1);
                    }
                }
            }
        }
    }

    /// Return an offscreen target to the pool. A full pool (8 targets, whatever their sizes)
    /// first frees the one at its front.
    ///
    /// # Safety
    /// This `Gpu`'s context must be current on the calling thread.
    unsafe fn release(&mut self, (t, f): (glow::Texture, glow::Framebuffer), size: (i32, i32)) {
        if self.pool.len() >= 8 {
            let (_, ot, of) = self.pool.remove(0);
            // SAFETY: the context is current, as this fn requires.
            unsafe { self.gl.delete_framebuffer(of) };
            // SAFETY: the context is current, as this fn requires.
            unsafe { del_tex(&self.gl, &self.st, ot) };
        }
        self.pool.push((size, t, f));
    }

    /// Top-down scissor rect `r` of an h-pixel-high draw space, in the bound target's texels.
    ///
    /// # Safety
    /// This `Gpu`'s context must be current on the calling thread.
    unsafe fn scissor(&self, r: Option<[i64; 4]>, h: i32, top: bool) {
        let r = match (top, self.dmg) {
            (true, Some(d)) => Some(r.map_or(d, |r| intersect(r, d))),
            _ => r,
        };
        match r {
            // SAFETY: the context is current, as this fn requires.
            None => unsafe { self.gl.disable(glow::SCISSOR_TEST) },
            Some(r) => {
                // SAFETY: the context is current, as this fn requires.
                unsafe { self.gl.enable(glow::SCISSOR_TEST) };
                let r = if self.vp == (1.0, 1.0) {
                    r
                } else {
                    // A clamped layer: the texels any part of the rect touches.
                    let (sx, sy) = self.vp;
                    [
                        (r[0] as f64 * sx).floor() as i64,
                        (r[1] as f64 * sy).floor() as i64,
                        (r[2] as f64 * sx).ceil() as i64,
                        (r[3] as f64 * sy).ceil() as i64,
                    ]
                };
                let th = (h as f64 * self.vp.1).round() as i64;
                let (sw, sh) = ((r[2] - r[0]).max(0), (r[3] - r[1]).max(0));
                let y = if self.flip { r[1] } else { th - r[3] };
                // SAFETY: the context is current, as this fn requires.
                unsafe { self.gl.scissor(r[0] as i32, y as i32, sw as i32, sh as i32) };
            }
        }
    }

    /// The `s` uniform of a w x h draw space on the target being drawn (`COLOR_VS`).
    fn space(&self, w: i32, h: i32) -> [f32; 2] {
        [w as f32, if self.flip { -h } else { h } as f32]
    }

    /// glViewport for a w x h draw space on the bound target: a layer clamped to
    /// MAX_TEXTURE_SIZE draws the same space into fewer texels (`vp`).
    ///
    /// # Safety
    /// This `Gpu`'s context must be current on the calling thread.
    unsafe fn viewport(&self, w: i32, h: i32) {
        let (sx, sy) = self.vp;
        let (tw, th) = (
            (w as f64 * sx).round() as i32,
            (h as f64 * sy).round() as i32,
        );
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.gl.viewport(0, 0, tw, th) };
    }

    /// The texel size of an offscreen target for a w x h draw space: like QSGRhiLayer, a layer
    /// past GL_MAX_TEXTURE_SIZE (GLES 2.0 §3.7.1: INVALID_VALUE, an incomplete framebuffer)
    /// is clamped to it and sampled scaled up over the same rect.
    fn fit(&self, (w, h): (i32, i32)) -> (i32, i32) {
        let m = self.max_tex as i32;
        (w.min(m), h.min(m))
    }

    /// Says in the log, once, that a `what` (a layer, a blur) asked for at `size` texels is
    /// past the texture size limit and is drawn at the size `fit` gives. The same `what` and
    /// size asked for again, by a later frame or by another item, says nothing more
    /// (docs/qt-differences.md).
    fn say_clamped(&mut self, what: &'static str, size: (i32, i32)) {
        let (w, h) = self.fit(size);
        if (w, h) != size && self.clamped.insert((what, size.0, size.1)) {
            tracing::warn!(
                target: "ri::gpu",
                "{what} of {}x{} is past the texture size limit {}: drawn at {w}x{h}",
                size.0,
                size.1,
                self.max_tex
            );
        }
    }

    /// The persistent layer `id`: only the area whose draws changed since it was last drawn
    /// is cleared and redrawn, as the window is with buffer age.
    ///
    /// # Safety
    /// This `Gpu`'s context must be current on the calling thread, with `vbo` its bound array
    /// buffer (`bind_array`): the vertices are written to the bound buffer and drawn from it.
    unsafe fn inc_layer_tex(&mut self, id: u64, fr: &Frame) -> Option<glow::Texture> {
        let size = (fr.w.max(1) as i32, fr.h.max(1) as i32);
        let digest = fr.digest();
        let now = self.frame_no;
        if let Some(e) = self.inc.get_mut(&id) {
            if e.size == size {
                e.used = now;
                let tex = e.tex;
                if e.digest == digest {
                    return Some(tex);
                }
                let full = [0, 0, size.0 as i64, size.1 as i64];
                let d = diff(&e.sig, &fr.sig, full).and_then(|d| region::bounds(&d));
                let fb = e.fb;
                e.digest = digest;
                e.sig = fr.sig.clone();
                e._keep = fr.keep.clone();
                if let Some(d) = d
                    .map(|d| intersect(d, full))
                    .filter(|d| d[2] > d[0] && d[3] > d[1])
                {
                    LAYER_DRAWS.with(|c| c.set(c.get() + 1));
                    let flip = std::mem::replace(&mut self.flip, true);
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.prepare_layers(&fr.cmds, &fr.boxes) };
                    let dmg = self.dmg.replace(d);
                    let ft = std::mem::replace(&mut self.force_top, true);
                    let vp = std::mem::replace(&mut self.vp, (1.0, 1.0));
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fb)) };
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.gl.viewport(0, 0, size.0, size.1) };
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.scissor(None, size.1, true) };
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.gl.clear_color(0.0, 0.0, 0.0, 0.0) };
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.gl.clear(glow::COLOR_BUFFER_BIT) };
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.exec(&fr.cmds, &fr.boxes, Some(fb), size.0, size.1) };
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.gl.disable(glow::SCISSOR_TEST) };
                    self.dmg = dmg;
                    self.force_top = ft;
                    self.vp = vp;
                    self.flip = flip;
                    if qml::debug::on(qml::debug::Switch::INCCHECK) {
                        // SAFETY: the context is current, as this fn requires.
                        unsafe { self.check_inc(fr, fb, d) };
                    }
                }
                return Some(tex);
            }
            if let Some(e) = self.inc.remove(&id) {
                // SAFETY: the context is current, as this fn requires.
                unsafe { self.release((e.tex, e.fb), e.size) };
            }
        }
        if self.fit(size) != size {
            // Past the texture size limit: drawn whole, scaled down, without in-place updates.
            // SAFETY: the context is current, as this fn requires.
            return unsafe { self.layer_tex(fr) };
        }
        if qml::log_on!("ri::gpuprof") {
            tracing::debug!(target: "ri::gpuprof", "  inc layer new {}x{} cmds {}", fr.w, fr.h, fr.cmds.len());
        }
        // SAFETY: the context is current, as this fn requires.
        let (tex, fb) = unsafe { self.offscreen(fr) }?;
        self.inc.insert(
            id,
            IncLayer {
                _keep: fr.keep.clone(),
                size,
                tex,
                fb,
                digest,
                sig: fr.sig.clone(),
                used: now,
            },
        );
        Some(tex)
    }

    /// Debug (RI_DEBUG=inccheck): an in-place layer update must match drawing the layer afresh.
    ///
    /// # Safety
    /// This `Gpu`'s context must be current on the calling thread, with `vbo` its bound array
    /// buffer (`bind_array`): the vertices are written to the bound buffer and drawn from it. The
    /// buffer glReadPixels fills (GLES 2.0 §4.3.1) is sized `fr.w * fr.h * 4`, which must not
    /// overflow an `i32`.
    unsafe fn check_inc(&mut self, fr: &Frame, fb: glow::Framebuffer, d: [i64; 4]) {
        let (w, h) = (fr.w.max(1) as i32, fr.h.max(1) as i32);
        let read = |gl: &glow::Context, f: glow::Framebuffer| {
            let mut px = vec![0u8; (w * h * 4) as usize];
            // SAFETY: the context is current, as this fn requires.
            unsafe { gl.bind_framebuffer(glow::FRAMEBUFFER, Some(f)) };
            // SAFETY: the context is current, as this fn requires.
            unsafe {
                gl.read_pixels(
                    0,
                    0,
                    w,
                    h,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelPackData::Slice(Some(&mut px)),
                )
            };
            px
        };
        let a = read(&self.gl, fb);
        // SAFETY: the context is current, as this fn requires.
        let Some(t) = (unsafe { self.offscreen(fr) }) else {
            return;
        };
        let b = read(&self.gl, t.1);
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.release(t, (w, h)) };
        let (mut bad, mut max, mut bb) = (0, 0, None);
        for (k, (p, q)) in a.chunks(4).zip(b.chunks(4)).enumerate() {
            let m = p
                .iter()
                .zip(q)
                .map(|(x, y)| x.abs_diff(*y))
                .max()
                .unwrap_or(0);
            if m > 0 {
                bad += 1;
                let (x, y) = ((k as i32 % w) as i64, (k as i32 / w) as i64);
                bb = union(bb, [x, y, x + 1, y + 1]);
            }
            max = max.max(m);
        }
        if bad > 0 {
            tracing::warn!(target: "ri::check", "inccheck {w}x{h} damage {d:?} bad {bad} max {max} at {bb:?}");
        } else {
            tracing::warn!(target: "ri::check", "inccheck ok {w}x{h} damage {d:?}");
        }
    }

    /// A layer's texture, re-rendered only when its draw list changed; None when GL cannot make
    /// its target (the draws sampling it are skipped).
    ///
    /// # Safety
    /// This `Gpu`'s context must be current on the calling thread, with `vbo` its bound array
    /// buffer (`bind_array`): the vertices are written to the bound buffer and drawn from it.
    unsafe fn layer_tex(&mut self, fr: &Frame) -> Option<glow::Texture> {
        let key = fr.digest();
        if qml::debug::on(qml::debug::Switch::NOLCACHE) {
            self.layers.clear();
        }
        if let Some(e) = self.layers.get_mut(&key) {
            e.used = self.frame_no;
            return Some(e.tex);
        }
        if qml::log_on!("ri::gpuprof") {
            tracing::debug!(target: "ri::gpuprof", "  layer miss {}x{} cmds {}", fr.w, fr.h, fr.cmds.len());
        }
        // SAFETY: the context is current, as this fn requires.
        let (tex, fb) = unsafe { self.offscreen(fr) }?;
        let size = self.fit((fr.w.max(1) as i32, fr.h.max(1) as i32));
        let e = LayerTex {
            _keep: fr.keep.clone(),
            size,
            tex,
            fb,
            used: self.frame_no,
        };
        self.layers.insert(key, e);
        Some(tex)
    }

    /// Starts a crossfade on `surf` from `old`, its previous frame: copied into a texture that
    /// the next frames draw over themselves with falling opacity for `dur`. The texture (and
    /// `fresh`) exist only while the fade runs: `end_fade` buries them, so an idle window holds
    /// no window-sized copies of itself. A request during a running fade starts from what the
    /// window shows, `old` with the running fade over it at its last opacity, so neither the
    /// fading image nor the new change jumps.
    ///
    /// `area` (`Frame::changed_part`) is where the new frame differs from `old`: only there is
    /// `old` drawn and copied, blended and repainted while the fade runs, and only that is
    /// damaged, so the rest of the window costs neither this process nor the compositor
    /// anything. None, or a fade already running, fades the whole window.
    pub fn begin_fade(
        &mut self,
        surf: &mut Surface,
        old: &Frame,
        dur: std::time::Duration,
        area: Option<&[[i64; 4]]>,
    ) {
        let t0 = std::time::Instant::now();
        let (w, h) = (old.w as i32, old.h as i32);
        if surf.size != (w, h) || w <= 0 || h <= 0 {
            tracing::debug!(target: "ri::fade", "skip: surface {:?} frame {w}x{h}", surf.size);
            return;
        }
        let running = surf
            .fade
            .as_ref()
            .map(|f| (f.tex, f.opacity(), f.area.clone()));
        let area = area
            .filter(|_| running.is_none())
            .map(|a| region::disjoint(&region::clip(a, [0, 0, w as i64, h as i64])));
        if area.as_ref().is_some_and(|a| a.is_empty()) {
            return;
        }
        let dur = surf
            .fade
            .as_ref()
            .map_or(dur, |f| dur.max(f.dur.saturating_sub(f.elapsed)));
        if self
            .egl
            .make_current(self.display, Some(surf.es), Some(surf.es), Some(self.ctx))
            .is_err()
        {
            return;
        }
        self.reset_state();
        // SAFETY: `make_current` above succeeded.
        let Some((tex, fb, ..)) = (unsafe { self.target(w, h) }) else {
            return;
        };
        // `old` is redrawn into the window's back buffer exactly as `render` drew it (same target,
        // orientation and format) and copied out: the fade starts from the pixels on screen,
        // not from an offscreen redraw that can round differently. The next `render` repaints
        // the whole window over it.
        let gl = &self.gl;
        // SAFETY: `make_current` above succeeded.
        unsafe { gl.enable(glow::BLEND) };
        // SAFETY: `make_current` above succeeded.
        unsafe { gl.blend_func(glow::ONE, glow::ONE_MINUS_SRC_ALPHA) };
        // SAFETY: `make_current` above succeeded.
        unsafe { self.bind_array(self.vbo) };
        let dmg = self.dmg.take();
        // One scissored pass per rectangle of `area`, as `render` repaints damage.
        let passes: Vec<Option<[i64; 4]>> = match &area {
            Some(a) => a.iter().copied().map(Some).collect(),
            None => vec![None],
        };
        // The back buffer of a window that showed `old` last already holds it, but for what
        // the frames since that buffer was shown changed: only that is drawn (`render` repairs
        // a buffer the same way), and the rest of `area` is copied as it is.
        let held = !surf.prev.is_empty() && surf.prev == old.sig;
        let draws: Vec<Option<[i64; 4]>> = match (&area, surf.stale(self.age(surf))) {
            (Some(a), Some(stale)) if held => a
                .iter()
                .flat_map(|&r| region::clip(&stale, r))
                .map(Some)
                .collect(),
            (None, Some(stale)) if held => stale.into_iter().map(Some).collect(),
            _ => passes.clone(),
        };
        self.fade_drawn = draws
            .iter()
            .map(|d| d.map_or(i64::from(w) * i64::from(h), region::area))
            .sum();
        if !draws.is_empty() {
            // SAFETY: `make_current` above succeeded.
            unsafe { self.prepare_layers(&old.cmds, &old.boxes) };
        }
        let gl = &self.gl;
        // SAFETY: `make_current` above succeeded.
        unsafe { gl.bind_framebuffer(glow::FRAMEBUFFER, None) };
        // SAFETY: `make_current` above succeeded.
        unsafe { gl.viewport(0, 0, w, h) };
        // SAFETY: `make_current` above succeeded.
        unsafe { gl.clear_color(0.0, 0.0, 0.0, 0.0) };
        for &d in &draws {
            self.dmg = d;
            // SAFETY: `make_current` above succeeded.
            unsafe { self.scissor(None, h, true) };
            // SAFETY: `make_current` above succeeded.
            unsafe { self.gl.clear(glow::COLOR_BUFFER_BIT) };
            // SAFETY: `make_current` above succeeded.
            unsafe { self.exec(&old.cmds, &old.boxes, None, w, h) };
        }
        self.dmg = None;
        if let Some((ftex, o, farea)) = running.as_ref().filter(|r| r.1 > 0.0) {
            for r in farea
                .as_ref()
                .map_or(vec![None], |a| a.iter().copied().map(Some).collect())
            {
                // SAFETY: `make_current` above succeeded.
                unsafe { self.draw_fade(*ftex, w, h, *o, r) };
            }
        }
        let gl = &self.gl;
        // SAFETY: `make_current` above succeeded.
        unsafe { gl.disable(glow::SCISSOR_TEST) };
        // SAFETY: `make_current` above succeeded.
        unsafe { gl.bind_framebuffer(glow::FRAMEBUFFER, None) };
        // SAFETY: `make_current` above succeeded.
        unsafe { gl.bind_texture(glow::TEXTURE_2D, Some(tex)) };
        for d in &passes {
            // GL rows count from the bottom.
            let [x, y, cw, ch] = d.map_or([0, 0, w, h], |r| {
                [
                    r[0] as i32,
                    h - r[3] as i32,
                    (r[2] - r[0]) as i32,
                    (r[3] - r[1]) as i32,
                ]
            });
            // SAFETY: `make_current` above succeeded.
            unsafe { gl.copy_tex_sub_image_2d(glow::TEXTURE_2D, 0, x, y, x, y, cw, ch) };
        }
        self.dmg = dmg;
        if running.is_some() {
            self.end_fade(surf);
        }
        let px: i64 = area.as_ref().map_or(i64::from(w) * i64::from(h), |a| {
            a.iter().map(|r| region::area(*r)).sum()
        });
        self.fade_px = px;
        surf.fade = Some(Fade::new(tex, fb, area, dur, held && running.is_none()));
        if qml::log_on!("ri::fade") {
            // SAFETY: `make_current` above succeeded.
            unsafe { self.gl.finish() };
            tracing::debug!(
                target: "ri::fade",
                "begin {w}x{h} area {px}px {}us",
                t0.elapsed().as_micros()
            );
        }
    }

    fn now(&self) -> std::time::Instant {
        self.clock.unwrap_or_else(std::time::Instant::now)
    }

    /// Runs what a crossfade starts with once on a window's first frame, changing no pixel: a
    /// copy out of the bound `w` x `h` window and a draw of the copy over it at opacity 0.
    /// Mesa builds both paths on first use (llvmpipe: 80 ms the copy, 55 ms the draw, per kind
    /// of window size), and the first fade of a window paid that in its first frame. Elsewhere
    /// this is a few microseconds while the window appears. See `warm_up`.
    ///
    /// # Safety
    /// This `Gpu`'s context must be current on the calling thread, drawing to the window.
    unsafe fn warm_fade(&mut self, w: i32, h: i32) {
        // llvmpipe builds a path per kind of texture side, a power of two or not.
        let side = |n: i32| {
            if n.count_ones() == 1 {
                n.min(2)
            } else {
                n.min(3)
            }
        };
        let (tw, th) = (side(w), side(h));
        // SAFETY: the context is current, as this fn requires.
        let Some((tex, fb, ..)) = (unsafe { self.target(tw, th) }) else {
            return;
        };
        let gl = &self.gl;
        // SAFETY: the context is current, as this fn requires.
        unsafe { gl.bind_framebuffer(glow::FRAMEBUFFER, None) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { gl.disable(glow::SCISSOR_TEST) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { gl.bind_texture(glow::TEXTURE_2D, Some(tex)) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { gl.copy_tex_sub_image_2d(glow::TEXTURE_2D, 0, 0, 0, 0, 0, tw, th) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.draw_fade(tex, w, h, 0.0, Some([0, 0, 1, 1])) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.gl.delete_framebuffer(fb) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { del_tex(&self.gl, &self.st, tex) };
    }

    /// Draws `tex`, a window's fading image, over the `w` x `h` window at opacity `o`: over
    /// all of it, or inside `clip` only.
    ///
    /// # Safety
    /// This `Gpu`'s context must be current on the calling thread.
    unsafe fn draw_fade(
        &mut self,
        tex: glow::Texture,
        w: i32,
        h: i32,
        o: f32,
        clip: Option<[i64; 4]>,
    ) {
        if self.fade_prog.is_none() {
            // SAFETY: the context is current, as this fn requires.
            let Some(p) = (unsafe { program(&self.gl, FADE_VS, FADE_FS, &["p"]) }) else {
                return;
            };
            let u = ["a", "o"].map(|n| {
                // SAFETY: the context is current, as this fn requires.
                unsafe { self.gl.get_uniform_location(p, n) }
            });
            self.fade_prog = Some((p, u));
        }
        let (p, u) = self.fade_prog.expect("fade program");
        let gl = &self.gl;
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.use_prog(p) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.scissor(clip, h, false) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { gl.enable(glow::BLEND) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { gl.blend_func(glow::ONE, glow::ONE_MINUS_SRC_ALPHA) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { gl.viewport(0, 0, w, h) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.bind_array(self.vbo) };
        // SAFETY: the context is current, as this fn requires.
        let off = unsafe { self.stream(as_bytes(&[-1f32, -1.0, 3.0, -1.0, -1.0, 3.0])) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.attrib(0, true) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.attrib(1, false) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.attrib_ptr(0, 2, 8, off) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { gl.active_texture(glow::TEXTURE0) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { gl.bind_texture(glow::TEXTURE_2D, Some(tex)) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.set_u(P_FADE, 0, u[0].as_ref(), Uni::I(0)) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.set_u(P_FADE, 1, u[1].as_ref(), Uni::f(&[o])) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { gl.draw_arrays(glow::TRIANGLES, 0, 3) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.attrib(1, true) };
    }

    /// Copies the whole window, as drawn before any fade over it, into `surf.fresh`.
    ///
    /// # Safety
    /// This `Gpu`'s context must be current on the calling thread.
    unsafe fn keep_fresh(&mut self, surf: &mut Surface, w: i32, h: i32) {
        if surf.fresh.is_none() {
            // SAFETY: the context is current, as this fn requires.
            surf.fresh = unsafe { self.target(w, h) }.map(|(t, f, ..)| (t, f));
        }
        let Some((tex, _)) = surf.fresh else {
            return;
        };
        let gl = &self.gl;
        // SAFETY: the context is current, as this fn requires.
        unsafe { gl.bind_framebuffer(glow::FRAMEBUFFER, None) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { gl.disable(glow::SCISSOR_TEST) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { gl.bind_texture(glow::TEXTURE_2D, Some(tex)) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { gl.copy_tex_sub_image_2d(glow::TEXTURE_2D, 0, 0, 0, 0, 0, w, h) };
    }

    /// Frees what dropped surfaces left in `dead`. Only for callers that made the context
    /// current: `render` (after `bind`) and `trim`.
    fn free_dead(&mut self) {
        let d = std::mem::take(&mut *self.dead.borrow_mut());
        if !d.surfaces.is_empty() {
            tracing::debug!(
                target: "ri::stats",
                "free {} targets of {} dropped surfaces",
                d.targets.len(),
                d.surfaces.len()
            );
        }
        for (t, f) in d.targets {
            // SAFETY: the context is current, as this fn's callers ensure (`render` after `bind`,
            // `trim` after `make_current`).
            unsafe { self.gl.delete_framebuffer(f) };
            // SAFETY: the context is current, as this fn's callers ensure (`render` after `bind`,
            // `trim` after `make_current`).
            unsafe { del_tex(&self.gl, &self.st, t) };
        }
        for k in d.surfaces {
            for (b, _) in self.tris_bufs.remove(&k).into_iter().flat_map(|e| e.1) {
                // SAFETY: the context is current, as this fn's callers ensure (`render` after `bind`,
                // `trim` after `make_current`).
                unsafe { del_buf(&self.gl, &self.st, b) };
            }
        }
    }

    /// Queues a finished or cut-short fade's window-sized targets in `dead` (freed by the next
    /// `free_dead`).
    fn end_fade(&self, surf: &mut Surface) {
        let fade = surf.fade.take().map(|f| (f.tex, f.fb));
        self.dead
            .borrow_mut()
            .targets
            .extend(fade.into_iter().chain(surf.fresh.take()));
    }

    /// Drop the persistent mask layers (`Cmd::Masked` ids, item + 1) of destroyed items.
    pub fn forget_items(&mut self, ids: &[usize]) {
        if self.inc.is_empty() {
            return;
        }
        let gone: Vec<u64> = ids
            .iter()
            .map(|&i| i as u64 + 1)
            .filter(|k| self.inc.contains_key(k))
            .collect();
        if gone.is_empty()
            || self
                .egl
                .make_current(self.display, None, None, Some(self.ctx))
                .is_err()
        {
            return;
        }
        for k in gone {
            if let Some(e) = self.inc.remove(&k) {
                // SAFETY: `make_current` above succeeded.
                unsafe { self.gl.delete_framebuffer(e.fb) };
                // SAFETY: `make_current` above succeeded.
                unsafe { del_tex(&self.gl, &self.st, e.tex) };
            }
        }
    }

    /// Frees layers and pooled targets not drawn in the last `keep` frames. Age eviction only
    /// runs while frames are drawn, so after a large teardown in an idle shell (e.g. clearing
    /// hundreds of notifications) the dead content's layers would otherwise stay resident.
    pub fn trim(&mut self, keep: u64) {
        if self
            .egl
            .make_current(self.display, None, None, Some(self.ctx))
            .is_err()
        {
            return;
        }
        self.free_dead();
        let now = self.frame_no;
        while let Some(e) = self.layers.pop_oldest_if(|e, _| now - e.used > keep) {
            // SAFETY: `make_current` above succeeded.
            unsafe { self.gl.delete_framebuffer(e.fb) };
            // SAFETY: `make_current` above succeeded.
            unsafe { del_tex(&self.gl, &self.st, e.tex) };
        }
        for (_, t, f) in self.pool.drain(..) {
            // SAFETY: `make_current` above succeeded.
            unsafe { self.gl.delete_framebuffer(f) };
            // SAFETY: `make_current` above succeeded.
            unsafe { del_tex(&self.gl, &self.st, t) };
        }
    }

    /// Records when the frame being drawn was.
    fn stamp(&mut self) {
        let (f, at) = (self.frame_no, self.now());
        let n = self.stamps.len();
        if n >= 2 && at.saturating_duration_since(self.stamps[n - 2].1) < STAMP_EVERY {
            self.stamps[n - 1] = (f, at);
        } else {
            self.stamps.push_back((f, at));
        }
        // Of the frames drawn `LAYER_IDLE_TIME` ago or earlier only the newest is asked for.
        while self
            .stamps
            .get(1)
            .is_some_and(|s| at.saturating_duration_since(s.1) >= LAYER_IDLE_TIME)
        {
            self.stamps.pop_front();
        }
    }

    /// The newest frame drawn `LAYER_IDLE_TIME` ago or earlier: a layer last drawn in it or
    /// before it has sat undrawn that long.
    fn idle_frame(&self) -> Option<u64> {
        let now = self.now();
        self.stamps
            .iter()
            .take_while(|s| now.saturating_duration_since(s.1) >= LAYER_IDLE_TIME)
            .last()
            .map(|s| s.0)
    }

    /// Frees the layers left undrawn for `LAYER_IDLE_TIME`, also when no frame is drawn: the
    /// host calls it every loop turn. Nothing but two comparisons until a layer is due.
    pub fn trim_idle(&mut self) {
        let Some(idle) = self.idle_frame() else {
            return;
        };
        let due = |used: Option<u64>| used.is_some_and(|u| u <= idle);
        if !due(self.layers.oldest().map(|e| e.used)) && !due(self.inc.oldest().map(|e| e.used)) {
            return;
        }
        if self
            .egl
            .make_current(self.display, None, None, Some(self.ctx))
            .is_err()
        {
            return;
        }
        // SAFETY: `make_current` above succeeded.
        unsafe { self.evict_idle(idle) };
        // The targets the incremental layers gave back.
        for (_, t, f) in self.pool.drain(..) {
            // SAFETY: `make_current` above succeeded.
            unsafe { self.gl.delete_framebuffer(f) };
            // SAFETY: `make_current` above succeeded.
            unsafe { del_tex(&self.gl, &self.st, t) };
        }
    }

    /// Frees the layers last drawn in frame `idle` or before it.
    unsafe fn evict_idle(&mut self, idle: u64) {
        while let Some(e) = self.inc.pop_oldest_if(|e, _| e.used <= idle) {
            // SAFETY: the context is current, as this fn requires.
            unsafe { self.release((e.tex, e.fb), e.size) };
        }
        while let Some(e) = self.layers.pop_oldest_if(|e, _| e.used <= idle) {
            // SAFETY: the context is current, as this fn requires.
            unsafe { self.gl.delete_framebuffer(e.fb) };
            // SAFETY: the context is current, as this fn requires.
            unsafe { del_tex(&self.gl, &self.st, e.tex) };
        }
    }

    /// Frees layers unused for a while, oldest first beyond a memory budget. Both caches are in
    /// drawing order, so this touches only the entries it frees and those drawn lately.
    ///
    /// # Safety
    /// This `Gpu`'s context must be current on the calling thread.
    unsafe fn evict_layers(&mut self) {
        const BUDGET: i64 = 96 << 20;
        let now = self.frame_no;
        if let Some(idle) = self.idle_frame().filter(|&f| f < now) {
            // SAFETY: the context is current, as this fn requires.
            unsafe { self.evict_idle(idle) };
        }
        while let Some(e) = self.inc.pop_oldest_if(|e, _| now - e.used > LAYER_IDLE) {
            // SAFETY: the context is current, as this fn requires.
            unsafe { self.release((e.tex, e.fb), e.size) };
        }
        // A layer holding the only reference to one of its images (e.g. a replaced screencopy
        // frame) can never be drawn again: recycle its target right away. Such a layer stopped
        // being drawn when its image was replaced; one whose image is dropped later is found by
        // the periodic sweep.
        let sweep = now.is_multiple_of(STALE_SWEEP);
        let stale: Vec<u64> = self
            .layers
            .iter()
            .skip_while(|(_, e)| e.used == now)
            .take_while(|(_, e)| sweep || e.used + 1 >= now)
            .filter(|(_, e)| e._keep.iter().any(|k| Rc::strong_count(k) == 1))
            .map(|(k, _)| *k)
            .collect();
        for k in stale {
            if let Some(e) = self.layers.remove(&k) {
                // SAFETY: the context is current, as this fn requires.
                unsafe { self.release((e.tex, e.fb), e.size) };
            }
        }
        while let Some(e) = self.layers.pop_oldest_if(|e, total| {
            e.used != now && (now - e.used >= LAYER_IDLE || total > BUDGET)
        }) {
            // SAFETY: the context is current, as this fn requires.
            unsafe { self.gl.delete_framebuffer(e.fb) };
            // SAFETY: the context is current, as this fn requires.
            unsafe { del_tex(&self.gl, &self.st, e.tex) };
        }
    }

    /// Moves the least recently drawn cached layer of `size` that sat out the last few frames
    /// into the pool: an animating subtree (e.g. a ListView add transition's opacity) misses
    /// every frame, and its superseded layers would otherwise each hold a target for 600 frames
    /// while every miss allocates a new one.
    fn recycle_layer(&mut self, size: (i32, i32)) {
        let now = self.frame_no;
        let old = self
            .layers
            .iter()
            .rev()
            .take_while(|(_, e)| e.used + 8 < now)
            .find(|(_, e)| e.size == size)
            .map(|(k, _)| *k);
        if let Some(e) = old.and_then(|k| self.layers.remove(&k)) {
            self.pool.push((size, e.tex, e.fb));
        }
    }

    /// Renders every layer texture `exec` will sample before the target is bound: switching
    /// render targets mid-frame makes llvmpipe flush and reload the target.
    ///
    /// # Safety
    /// This `Gpu`'s context must be current on the calling thread, with `vbo` its bound array
    /// buffer (`bind_array`): the vertices are written to the bound buffer and drawn from it.
    unsafe fn prepare_layers(&mut self, cmds: &[Cmd], boxes: &[[f32; 4]]) {
        for (c, b) in cmds.iter().zip(boxes) {
            match c {
                Cmd::Layer { src, .. } | Cmd::Tint { src, .. }
                    if self.no_skip || !covers_no_pixel(*b) =>
                {
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.layer_tex(src) };
                }
                Cmd::Radial { mask: Some(m), .. } => {
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.layer_tex(m) };
                }
                Cmd::Masked { src, mask, id, .. } => {
                    if *id != 0 {
                        // SAFETY: the context is current, as this fn requires.
                        unsafe { self.inc_layer_tex(*id, src) };
                    } else {
                        // SAFETY: the context is current, as this fn requires.
                        unsafe { self.layer_tex(src) };
                    }
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.layer_tex(mask) };
                }
                Cmd::Gauss {
                    src,
                    g,
                    size,
                    rect,
                    qt,
                    ..
                } if self.no_skip || !covers_no_pixel(*b) => {
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.gauss_tex(src, g, *size, *rect, qt.as_ref()) };
                }
                Cmd::Effect {
                    src, mask, blur, ..
                } => {
                    match blur {
                        Some(b) => {
                            // SAFETY: the context is current, as this fn requires.
                            unsafe { self.blur_levels(src, b) };
                        }
                        None => {
                            // SAFETY: the context is current, as this fn requires.
                            unsafe { self.layer_tex(src) };
                        }
                    }
                    if let Some(m) = mask {
                        // SAFETY: the context is current, as this fn requires.
                        unsafe { self.layer_tex(m) };
                    }
                }
                _ => {}
            }
        }
    }

    /// Uploads the glyph bitmaps and distance-field tiles a frame draws that are not in their
    /// atlas yet, those of the layers `prepare_layers` will render included, before anything of
    /// the frame is drawn. Uploaded where the draw list reaches them, each upload makes the
    /// driver finish the draws before it that sample the atlas: llvmpipe then waits for its
    /// rasterizer threads once per text item of a frame of new text (a panel's first frame).
    /// An atlas the new bitmaps do not fit is emptied and grown here, where nothing drawn
    /// refers to it; one at its largest size is left to `glyph_pass` and `df_pass`.
    ///
    /// # Safety
    /// This `Gpu`'s context must be current on the calling thread.
    unsafe fn place_new_glyphs(&mut self, cmds: &[Cmd], boxes: &[[f32; 4]]) {
        // Mono glyphs, colour glyphs, distance-field tiles: false once an atlas is full.
        let mut open = [true; 3];
        loop {
            let before = [self.atlas.size, self.catlas.size, self.dfatlas.size];
            // SAFETY: the context is current, as this fn requires.
            unsafe { self.place_cmds(cmds, boxes, &mut open) };
            if before == [self.atlas.size, self.catlas.size, self.dfatlas.size] {
                break;
            }
            // An atlas grew, forgetting what was placed in it: place the frame's bitmaps again.
        }
    }

    /// One pass of `place_new_glyphs` over a draw list and the layers under it.
    ///
    /// # Safety
    /// This `Gpu`'s context must be current on the calling thread.
    unsafe fn place_cmds(&mut self, cmds: &[Cmd], boxes: &[[f32; 4]], open: &mut [bool; 3]) {
        for (c, b) in cmds.iter().zip(boxes) {
            let drawn = self.no_skip || !covers_no_pixel(*b);
            // The layers `prepare_layers` renders: those it has no texture of.
            let (first, second): (Option<&Frame>, Option<&Frame>) = match c {
                Cmd::Glyphs(_, q) => {
                    for (g, ..) in q.iter() {
                        // SAFETY: the context is current, as this fn requires.
                        unsafe { self.place_early(g, open) };
                    }
                    (None, None)
                }
                Cmd::GlyphsF(_, q) => {
                    for (g, _) in q.iter() {
                        // SAFETY: the context is current, as this fn requires.
                        unsafe { self.place_early(g, open) };
                    }
                    (None, None)
                }
                Cmd::Df(_, tiles) => {
                    for (t, ..) in tiles.iter() {
                        // SAFETY: the context is current, as this fn requires.
                        unsafe { self.place_tile_early(t, open) };
                    }
                    (None, None)
                }
                Cmd::Layer { src, .. } | Cmd::Tint { src, .. } | Cmd::Gauss { src, .. }
                    if drawn =>
                {
                    (Some(src), None)
                }
                Cmd::Radial { mask: Some(m), .. } => (Some(m), None),
                Cmd::Masked { src, mask, .. } => (Some(src), Some(mask)),
                Cmd::Effect { src, mask, .. } => (Some(src), mask.as_deref()),
                _ => (None, None),
            };
            // A masked item's source is kept under the item's id, redrawn where it changed.
            let kept = match c {
                Cmd::Masked { id, .. } if *id != 0 => self.inc.peek(id).map(|e| e.digest),
                _ => None,
            };
            for (n, fr) in [first, second].into_iter().enumerate() {
                let Some(fr) = fr else { continue };
                let digest = fr.digest();
                let cached = match c {
                    Cmd::Masked { id, .. } if *id != 0 && n == 0 => kept == Some(digest),
                    _ => self.layers.contains_key(&digest),
                };
                if !cached {
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.place_cmds(&fr.cmds, &fr.boxes, open) };
                }
            }
        }
    }

    /// Glyph `g` into its atlas if it is not there, growing a full atlas that can grow.
    ///
    /// # Safety
    /// This `Gpu`'s context must be current on the calling thread.
    unsafe fn place_early(&mut self, g: &Rc<qsg::text::Glyph>, open: &mut [bool; 3]) {
        let k = g.color as usize;
        if !open[k] || g.width == 0 || g.rows == 0 {
            return;
        }
        // SAFETY: the context is current, as this fn requires.
        if unsafe { self.place(g) }.is_some() {
            return;
        }
        let at = if g.color { &self.catlas } else { &self.atlas };
        let size = at.next_size(g.width as i32, g.rows as i32);
        if size == at.size {
            open[k] = false;
            return;
        }
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.atlas_reset(g.color, size) };
        // SAFETY: the context is current, as this fn requires.
        open[k] = unsafe { self.place(g) }.is_some();
    }

    /// `place_early` for a distance-field tile; one larger than any atlas has its own texture.
    ///
    /// # Safety
    /// This `Gpu`'s context must be current on the calling thread.
    unsafe fn place_tile_early(&mut self, t: &Rc<Canvas>, open: &mut [bool; 3]) {
        let (tw, th) = (t.w as i32, t.h as i32);
        if !open[2] || tw >= self.dfatlas.max || th >= self.dfatlas.max {
            return;
        }
        let b: Rc<dyn std::any::Any> = t.clone();
        let px = as_bytes(&t.px);
        // SAFETY: the context is current, as this fn requires.
        if unsafe { self.dfatlas.place(&self.gl, t.key.get(), &b, tw, th, px) }.is_some() {
            return;
        }
        let size = self.dfatlas.next_size(tw, th);
        if size == self.dfatlas.size {
            open[2] = false;
            return;
        }
        // SAFETY: the context is current, as this fn requires.
        unsafe { Self::reset_atlas(&self.gl, &mut self.dfatlas, size) };
        // SAFETY: the context is current, as this fn requires.
        open[2] = unsafe { self.dfatlas.place(&self.gl, t.key.get(), &b, tw, th, px) }.is_some();
    }

    fn outside_damage(&self, b: [f32; 4]) -> bool {
        self.dmg.is_some_and(|d| {
            b[2] < d[0] as f32 - 1.0
                || b[0] > d[2] as f32 + 1.0
                || b[3] < d[1] as f32 - 1.0
                || b[1] > d[3] as f32 + 1.0
        })
    }

    /// Renders a layer's draw list into a fresh premultiplied RGBA texture.
    ///
    /// # Safety
    /// This `Gpu`'s context must be current on the calling thread, with `vbo` its bound array
    /// buffer (`bind_array`): the vertices are written to the bound buffer and drawn from it.
    unsafe fn offscreen(&mut self, fr: &Frame) -> Option<(glow::Texture, glow::Framebuffer)> {
        let ft = std::mem::replace(&mut self.force_top, false);
        // SAFETY: the context is current, as this fn requires.
        let r = unsafe { self.offscreen_full(fr) };
        self.force_top = ft;
        r
    }

    /// [`offscreen`](Self::offscreen) with `force_top` left as it is.
    ///
    /// # Safety
    /// This `Gpu`'s context must be current on the calling thread, with `vbo` its bound array
    /// buffer (`bind_array`): the vertices are written to the bound buffer and drawn from it.
    unsafe fn offscreen_full(&mut self, fr: &Frame) -> Option<(glow::Texture, glow::Framebuffer)> {
        let flip = !std::mem::take(&mut self.upright);
        let was = std::mem::replace(&mut self.flip, flip);
        // SAFETY: the context is current, as this fn requires.
        let r = unsafe { self.offscreen_flipped(fr) };
        self.flip = was;
        r
    }

    /// [`offscreen_full`](Self::offscreen_full) with `flip` already set for the target.
    ///
    /// # Safety
    /// This `Gpu`'s context must be current on the calling thread, with `vbo` its bound array
    /// buffer (`bind_array`): the vertices are written to the bound buffer and drawn from it.
    unsafe fn offscreen_flipped(
        &mut self,
        fr: &Frame,
    ) -> Option<(glow::Texture, glow::Framebuffer)> {
        LAYER_DRAWS.with(|c| c.set(c.get() + 1));
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.prepare_layers(&fr.cmds, &fr.boxes) };
        let (w, h) = (fr.w.max(1) as i32, fr.h.max(1) as i32);
        if qml::log_on!("ri::stats") && w as usize * h as usize > 200_000 {
            tracing::debug!(target: "ri::stats", "  offscreen {w}x{h} cmds {}", fr.cmds.len());
        }
        let (tw, th) = self.fit((w, h));
        self.say_clamped("layer", (w, h));
        // SAFETY: the context is current, as this fn requires.
        let (t, f) = unsafe { self.clear_target(tw, th) }?;
        let vp = std::mem::replace(&mut self.vp, (tw as f64 / w as f64, th as f64 / h as f64));
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.exec(&fr.cmds, &fr.boxes, Some(f), w, h) };
        self.vp = vp;
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.gl.disable(glow::SCISSOR_TEST) };
        Some((t, f))
    }

    /// A cleared, bound w x h offscreen target, pooled or new; None when GL cannot make one.
    ///
    /// # Safety
    /// This `Gpu`'s context must be current on the calling thread.
    unsafe fn clear_target(
        &mut self,
        w: i32,
        h: i32,
    ) -> Option<(glow::Texture, glow::Framebuffer)> {
        if !self.pool.iter().any(|p| p.0 == (w, h)) {
            self.recycle_layer((w, h));
        }
        if let Some(k) = self.pool.iter().position(|p| p.0 == (w, h)) {
            let (_, t, f) = self.pool.swap_remove(k);
            glcount(4, w as usize * h as usize);
            let gl = &self.gl;
            // SAFETY: the context is current, as this fn requires.
            unsafe { gl.bind_framebuffer(glow::FRAMEBUFFER, Some(f)) };
            // SAFETY: the context is current, as this fn requires.
            unsafe { gl.viewport(0, 0, w, h) };
            // SAFETY: the context is current, as this fn requires.
            unsafe { gl.disable(glow::SCISSOR_TEST) };
            // SAFETY: the context is current, as this fn requires.
            unsafe { gl.clear_color(0.0, 0.0, 0.0, 0.0) };
            // SAFETY: the context is current, as this fn requires.
            unsafe { gl.clear(glow::COLOR_BUFFER_BIT) };
            return Some((t, f));
        }
        glcount(3, w as usize * h as usize);
        gl_count(0);
        gl_count(2);
        // SAFETY: the context is current, as this fn requires.
        let (t, f, ..) = unsafe { self.target(w, h) }?;
        let gl = &self.gl;
        // SAFETY: the context is current, as this fn requires.
        unsafe { gl.viewport(0, 0, w, h) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { gl.disable(glow::SCISSOR_TEST) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { gl.clear_color(0.0, 0.0, 0.0, 0.0) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { gl.clear(glow::COLOR_BUFFER_BIT) };
        Some((t, f))
    }

    /// The source layer and its BlurItem levels, each cached by the source's digest and the chain.
    ///
    /// # Safety
    /// This `Gpu`'s context must be current on the calling thread, with `vbo` its bound array
    /// buffer (`bind_array`): the vertices are written to the bound buffer and drawn from it.
    unsafe fn blur_levels(
        &mut self,
        src: &Frame,
        b: &Blur,
    ) -> Option<(glow::Texture, Vec<glow::Texture>)> {
        // SAFETY: the context is current, as this fn requires.
        let first = unsafe { self.layer_tex(src) }?;
        let (mut prev, mut key) = (first, src.digest());
        let mut out = Vec::with_capacity(b.sizes.len());
        for &(iw, ih) in &b.sizes {
            key = mix_f(mix(key, 11), &[iw as f32, ih as f32, b.multiplier]);
            key = mix(key, b.dpr.to_bits());
            let tex = if let Some(e) = self.layers.get_mut(&key) {
                e.used = self.frame_no;
                e.tex
            } else {
                // A BlurItem's layer: its size times the devicePixelRatio, rounded once.
                let texels = |l: f64| (l * b.dpr).round().clamp(1.0, i32::MAX as f64) as i32;
                self.say_clamped("blur", (texels(iw), texels(ih)));
                let size = self.fit((texels(iw), texels(ih)));
                // SAFETY: the context is current, as this fn requires.
                let (t, f) = unsafe { self.clear_target(size.0, size.1) }?;
                let d = [
                    (1.0 + b.multiplier) / iw as f32,
                    (1.0 + b.multiplier) / ih as f32,
                ];
                let mut v = Vec::with_capacity(24);
                quad(
                    &mut v,
                    0.0,
                    0.0,
                    size.0 as f32,
                    size.1 as f32,
                    [0.0, 1.0, 1.0, -1.0],
                );
                let gl = &self.gl;
                // SAFETY: the context is current, as this fn requires.
                unsafe { gl.active_texture(glow::TEXTURE0) };
                // SAFETY: the context is current, as this fn requires.
                unsafe { gl.bind_texture(glow::TEXTURE_2D, Some(prev)) };
                // SAFETY: the context is current, as this fn requires.
                unsafe { self.filter(prev, glow::LINEAR as i32) };
                // SAFETY: the context is current, as this fn requires.
                unsafe { self.use_prog(self.blur) };
                let u = &self.blur_u;
                // SAFETY: the context is current, as this fn requires.
                unsafe {
                    self.set_u(
                        P_BLUR,
                        0,
                        u[0].as_ref(),
                        Uni::f(&[size.0 as f32, size.1 as f32]),
                    )
                };
                // SAFETY: the context is current, as this fn requires.
                unsafe { self.set_u(P_BLUR, 1, u[1].as_ref(), Uni::I(0)) };
                // SAFETY: the context is current, as this fn requires.
                unsafe { self.set_u(P_BLUR, 2, u[2].as_ref(), Uni::f(&d)) };
                // SAFETY: the context is current, as this fn requires.
                unsafe { gl.disable(glow::BLEND) };
                // SAFETY: the context is current, as this fn requires.
                unsafe { self.draw_tex_quads(&v) };
                // SAFETY: the context is current, as this fn requires.
                unsafe { gl.enable(glow::BLEND) };
                self.layers.insert(
                    key,
                    LayerTex {
                        _keep: src.keep.clone(),
                        size,
                        tex: t,
                        fb: f,
                        used: self.frame_no,
                    },
                );
                t
            };
            out.push(tex);
            prev = tex;
        }
        Some((first, out))
    }

    /// The blurred source layer of a GaussianBlur/DropShadow (`Cmd::Gauss`) and the device rect
    /// it is drawn over 1:1: a horizontal then a vertical pass, like GaussianBlur.qml's two
    /// ShaderEffects, cached by the source's digest and the kernel until either changes (Qt
    /// keeps the effect's FBOs until its inputs change). The first pass renders the layer of
    /// `size` texels, sampling `src` linearly over `g.band` of it; the second renders the
    /// device pixels `rect` touches, sampling that layer linearly over `rect`, as Qt draws the
    /// vertical pass in the window. The first pass is a layer, drawn top row first like every
    /// layer; the second is drawn the way the target being drawn is (`flip`), as Qt draws it
    /// straight into that target, and the bool tells which. None when `g`'s shader cannot be
    /// built (the effect draws nothing).
    ///
    /// # Safety
    /// This `Gpu`'s context must be current on the calling thread, with `vbo` its bound array
    /// buffer (`bind_array`): the vertices are written to the bound buffer and drawn from it.
    unsafe fn gauss_tex(
        &mut self,
        src: &Frame,
        g: &Gauss,
        size: (usize, usize),
        rect: [f32; 4],
        qt: Option<&GaussAt>,
    ) -> Option<(glow::Texture, [f32; 4], bool)> {
        // The whole device pixels the vertical pass covers, and where it lies among them.
        let (x0, y0) = (rect[0].floor(), rect[1].floor());
        let (x1, y1) = ((rect[0] + rect[2]).ceil(), (rect[1] + rect[3]).ceil());
        // With Qt's own vertices and matrix the pass is drawn as into its target: only the
        // pixels it has there.
        let [x0, y0, x1, y1] = match qt {
            Some(q) => {
                let (tw, th) = (q.target.0 as f32, q.target.1 as f32);
                [x0.max(0.0), y0.max(0.0), x1.min(tw), y1.min(th)]
            }
            None => [x0, y0, x1, y1],
        };
        if qt.is_some() && (x1 <= x0 || y1 <= y0) {
            return None;
        }
        let (ow, oh) = ((x1 - x0).max(1.0), (y1 - y0).max(1.0));
        let at = [rect[0] - x0, rect[1] - y0, rect[2], rect[3]];
        let over = [x0, y0, ow, oh];
        let key = mix(
            mix(mix(src.digest(), g.digest()), size.0 as u64),
            size.1 as u64,
        );
        // A target that stands for a window is drawn as Mesa draws into a window, whose rows
        // it keeps top first.
        let rows = match (self.flip, self.clip_control.is_some()) {
            (true, _) => Rows::TopFirst,
            (false, true) => Rows::Window,
            (false, false) => Rows::BottomFirst,
        };
        let flip = rows != Rows::BottomFirst;
        let key = mix(mix_f(key, &at), flip as u64);
        let qt = qt.filter(|_| self.fit((ow as i32, oh as i32)) == (ow as i32, oh as i32));
        let key = match qt {
            Some(q) => {
                let key = mix_f(key, &q.item);
                let (tw, th) = (q.target.0 as f32, q.target.1 as f32);
                let key = mix_f(key, &[tw, th, q.m[0], q.m[1], q.m[4], q.m[5]]);
                // Wholly inside the target, a blur that moved by whole pixels keeps its
                // texture.
                let inside = rect[0] >= 0.0
                    && rect[1] >= 0.0
                    && rect[0] + rect[2] <= tw
                    && rect[1] + rect[3] <= th;
                match inside {
                    true => key,
                    false => mix_f(key, &[x0, y0, ow, oh]),
                }
            }
            None => key,
        };
        if let Some(e) = self.layers.get_mut(&key) {
            e.used = self.frame_no;
            return Some((e.tex, over, flip));
        }
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.gauss_prog(g) }?;
        // SAFETY: the context is current, as this fn requires.
        let a = unsafe { self.layer_tex(src) }?;
        // Steps of the kernel's own texels (`g.per`), also when a target is clamped (`fit`).
        let (mid, out) = (
            (size.0.max(1) as i32, size.1.max(1) as i32),
            (ow as i32, oh as i32),
        );
        // One line for the blur: the size of the rect it covers if that is clamped, else of
        // its first pass.
        self.say_clamped("blur", if self.fit(out) != out { out } else { mid });
        let size = self.fit(mid);
        let (mw, mh) = (size.0 as f32, size.1 as f32);
        let band = [0.0, g.band[0] * mh, mw, g.band[1] * mh];
        let d = [g.spread / g.per[0], 0.0];
        let (band, first) = match g.first {
            // QSGRenderer::setProjectionMatrixToRect of the layer's rect, mirrored: its top
            // is the texture's first row.
            Some([iw, ih, top, tall]) => {
                let m = qsg::tri::ortho(0.0, iw, top, top + tall);
                ([0.0, 0.0, iw, ih], Some((m, [0, 0, size.0, size.1])))
            }
            None => (band, None),
        };
        // SAFETY: the context is current, as this fn requires.
        let mid = unsafe { self.gauss_pass(a, size, band, g, d, [1.0; 4], Rows::TopFirst, first) }?;
        let tint = g.shadow.map_or([1.0; 4], |s| s.0);
        let out_size = self.fit(out);
        let (kx, ky) = (out_size.0 as f32 / ow, out_size.1 as f32 / oh);
        let at = [at[0] * kx, at[1] * ky, at[2] * kx, at[3] * ky];
        let d = [0.0, g.spread / g.per[1]];
        let (at, qt) = match qt {
            Some(q) => {
                // The target's viewport, moved to where the pass's pixels are in it.
                let (mut m, (tw, th)) = (q.m, q.target);
                let y = match rows {
                    Rows::TopFirst => {
                        (0..4).for_each(|c| m[c * 4 + 1] = -m[c * 4 + 1]);
                        -(y0 as i32)
                    }
                    Rows::Window => -(y0 as i32),
                    Rows::BottomFirst => (y0 + oh) as i32 - th,
                };
                (q.item, Some((m, [-(x0 as i32), y, tw, th])))
            }
            None => (at, None),
        };
        // SAFETY: the context is current, as this fn requires.
        let out = unsafe { self.gauss_pass(mid.0, out_size, at, g, d, tint, rows, qt) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.release(mid, size) };
        let out = out?;
        self.layers.insert(
            key,
            LayerTex {
                _keep: src.keep.clone(),
                size: out_size,
                tex: out.0,
                fb: out.1,
                used: self.frame_no,
            },
        );
        Some((out.0, over, flip))
    }

    /// `g`'s blur program, built on first use; None (and logged once) if it does not build.
    ///
    /// # Safety
    /// This `Gpu`'s context must be current on the calling thread.
    unsafe fn gauss_prog(&mut self, g: &Gauss) -> Option<GaussProg> {
        let key = g.prog_key(self.max_frag_uniforms);
        if let Some(p) = self.gauss.get(&key) {
            return *p;
        }
        let fs = g.shader(self.max_frag_uniforms);
        // SAFETY: the context is current, as this fn requires.
        let p = unsafe { program(&self.gl, GAUSS_VS, &fs, &["p", "t"]) };
        let p = p.map(|p| {
            // SAFETY: the context is current, as this fn requires.
            (
                p,
                ["m", "x", "d", "k", "t", "w"]
                    .map(|n| unsafe { self.gl.get_uniform_location(p, n) }),
            )
        });
        if p.is_none() {
            tracing::error!("gpu: blur shader of {} taps does not build", g.taps.len());
        }
        self.gauss.insert(key, p);
        p
    }

    /// One blur pass into a fresh target of `size` texels, sampling `src` linearly over the
    /// quad `at` (x y w h in texels) of it; the rest stays transparent, its rows laid as `rows`
    /// says.
    ///
    /// # Safety
    /// This `Gpu`'s context must be current on the calling thread, with `vbo` its bound array
    /// buffer (`bind_array`): the vertices are written to the bound buffer and drawn from it.
    #[allow(clippy::too_many_arguments)]
    unsafe fn gauss_pass(
        &mut self,
        src: glow::Texture,
        size: (i32, i32),
        at: [f32; 4],
        g: &Gauss,
        d: [f32; 2],
        tint: [f32; 4],
        rows: Rows,
        qt: Option<([f32; 16], [i32; 4])>,
    ) -> Option<(glow::Texture, glow::Framebuffer)> {
        // SAFETY: the context is current, as this fn requires.
        let (p, u) = unsafe { self.gauss_prog(g) }?;
        // SAFETY: the context is current, as this fn requires.
        let (t, f) = unsafe { self.clear_target(size.0, size.1) }?;
        let mut v = Vec::with_capacity(24);
        grid(&mut v, at);
        let thickness = g.shadow.map_or(1.0, |s| s.1);
        let gl = &self.gl;
        // SAFETY: the context is current, as this fn requires.
        unsafe { gl.active_texture(glow::TEXTURE0) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { gl.bind_texture(glow::TEXTURE_2D, Some(src)) };
        // The effect's source proxy is a smooth ShaderEffectSource, clamped at its edges.
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.filter(src, glow::LINEAR as i32) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.use_prog(p) };
        for _ in 0..u.len() {
            gl_count(GL_UNIFORM);
        }
        // The matrix of `TEX_VS` over the target, or Qt's own in the viewport it is for.
        let sgn = if rows == Rows::TopFirst { -1.0f32 } else { 1.0 };
        let upper = self.clip_control.filter(|_| rows == Rows::Window);
        #[rustfmt::skip]
        let m = qt.map_or([
            2.0 / size.0 as f32, 0.0, 0.0, 0.0,
            0.0, -sgn * (2.0 / size.1 as f32), 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0,
            -1.0, sgn, 0.0, 1.0,
        ], |q| q.0);
        if let Some((_, [x, y, w, h])) = qt {
            // SAFETY: the context is current, as this fn requires.
            unsafe { gl.viewport(x, y, w, h) };
        }
        // SAFETY: the context is current, as this fn requires.
        unsafe { gl.uniform_matrix_4_f32_slice(u[0].as_ref(), false, &m) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { gl.uniform_1_i32(u[1].as_ref(), 0) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { gl.uniform_2_f32(u[2].as_ref(), d[0], d[1]) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { gl.uniform_4_f32(u[3].as_ref(), tint[0], tint[1], tint[2], tint[3]) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { gl.uniform_2_f32(u[4].as_ref(), thickness, g.sum) };
        if g.looped(self.max_frag_uniforms) {
            let w: Vec<f32> = g.taps.iter().flat_map(|t| [t.0, t.1]).collect();
            // SAFETY: the context is current, as this fn requires.
            unsafe { gl.uniform_2_f32_slice(u[5].as_ref(), &w) };
        }
        // SAFETY: the context is current, as this fn requires.
        unsafe { gl.disable(glow::BLEND) };
        if let Some(clip) = upper {
            // SAFETY: the context is current and advertises GL_EXT_clip_control.
            unsafe { clip(glow::UPPER_LEFT, glow::NEGATIVE_ONE_TO_ONE) };
        }
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.draw_tex_quads(&v) };
        if let Some(clip) = upper {
            // SAFETY: the context is current and advertises GL_EXT_clip_control.
            unsafe { clip(glow::LOWER_LEFT, glow::NEGATIVE_ONE_TO_ONE) };
        }
        // SAFETY: the context is current, as this fn requires.
        unsafe { gl.enable(glow::BLEND) };
        Some((t, f))
    }

    /// Writes `data` to a fresh range of the bound `vbo` and returns its byte offset: no draw
    /// in flight reads that range, so the driver neither stalls nor reallocates per draw.
    ///
    /// # Safety
    /// This `Gpu`'s context must be current on the calling thread, with `vbo` its bound array
    /// buffer (`bind_array`): the vertices are written to the bound buffer and drawn from it.
    unsafe fn stream(&self, data: &[u8]) -> i32 {
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.stream_parts(&[data]) }
    }

    /// Uploads `parts` back to back into the stream buffer; the offset of the first.
    ///
    /// # Safety
    /// This `Gpu`'s context must be current on the calling thread, with `vbo` its bound array
    /// buffer (`bind_array`): the vertices are written to the bound buffer and drawn from it.
    unsafe fn stream_parts(&self, parts: &[&[u8]]) -> i32 {
        let gl = &self.gl;
        let len: usize = parts.iter().map(|p| p.len()).sum();
        let n = (len + 255) & !255;
        let mut at = self.stream_at.get();
        if n > STREAM_SIZE {
            if let [data] = parts {
                // SAFETY: the context is current, as this fn requires.
                unsafe { gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, data, glow::STREAM_DRAW) };
            } else {
                // SAFETY: the context is current, as this fn requires.
                unsafe { gl.buffer_data_size(glow::ARRAY_BUFFER, len as i32, glow::STREAM_DRAW) };
                let mut o = 0;
                for p in parts {
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { gl.buffer_sub_data_u8_slice(glow::ARRAY_BUFFER, o as i32, p) };
                    o += p.len();
                }
            }
            self.stream_at.set(STREAM_SIZE);
            return 0;
        }
        if at + n > STREAM_SIZE {
            // SAFETY: the context is current, as this fn requires.
            unsafe {
                gl.buffer_data_size(glow::ARRAY_BUFFER, STREAM_SIZE as i32, glow::STREAM_DRAW)
            };
            at = 0;
        }
        let mut o = at;
        for p in parts {
            // SAFETY: the context is current, as this fn requires.
            unsafe { gl.buffer_sub_data_u8_slice(glow::ARRAY_BUFFER, o as i32, p) };
            o += p.len();
        }
        self.stream_at.set(at + n);
        at as i32
    }

    /// Draws the triangles of `v` (x y u v per vertex) with the program in use; four vertices
    /// are one rect as a triangle strip (`strip`).
    ///
    /// # Safety
    /// This `Gpu`'s context must be current on the calling thread, with `vbo` its bound array
    /// buffer (`bind_array`): the vertices are written to the bound buffer and drawn from it.
    unsafe fn draw_tex_quads(&self, v: &[f32]) {
        // SAFETY: the context is current, as this fn requires.
        let o = unsafe { self.stream(as_bytes(v)) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.attrib(0, true) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.attrib(1, true) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.attrib_ptr(0, 2, 16, o) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.attrib_ptr(1, 2, 16, o + 8) };
        // SAFETY: the context is current, as this fn requires.
        let primitive = if v.len() == 16 {
            glow::TRIANGLE_STRIP
        } else {
            glow::TRIANGLES
        };
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.gl.draw_arrays(primitive, 0, (v.len() / 4) as i32) };
    }

    /// Draws the triangles of `runs` (gl_Position x y, r g b a per vertex): from the
    /// surface's own buffer for this draw where it keeps one (`tris_at`), else streamed.
    ///
    /// # Safety
    /// This `Gpu`'s context must be current on the calling thread, with `vbo` its bound array
    /// buffer (`bind_array`): the vertices are written to the bound buffer and drawn from it.
    unsafe fn draw_tris(&mut self, runs: &[Rc<Vec<f32>>]) {
        let gl = &self.gl;
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.use_prog(self.color) };
        // The target can change from one call to the next within a frame (a layer, then the
        // window), so the sign is set at every call.
        let f = if self.flip { -1.0 } else { 1.0 };
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.set_u(P_COLOR, 0, self.color_f.as_ref(), Uni::f(&[f])) };
        let n: usize = runs.iter().map(|r| r.len()).sum();
        let streamed = if let Some((s, k)) = self.tris_at.as_mut() {
            let (s, at_k) = (*s, *k);
            *k += 1;
            let (used, bufs) = self.tris_bufs.entry(s).or_default();
            *used = self.frame_no;
            // Damage passes skip `Cmd::Tris` outside the damage but still count them, so `at_k`
            // can be past the end by more than one.
            while bufs.len() <= at_k {
                // SAFETY: the context is current, as this fn requires.
                unsafe { bufs.push((gl.create_buffer().expect("buffer"), Vec::new())) };
            }
            let (b, data) = &mut bufs[at_k];
            let mut at = 0;
            let same = data.len() == n
                && runs.iter().all(|r| {
                    at += r.len();
                    as_bytes(&data[at - r.len()..at]) == as_bytes(r)
                });
            // Changed triangles of the same size update the store in place instead of
            // re-specifying it.
            let respecify = !same && data.len() != n;
            if !same {
                data.clear();
                for r in runs {
                    data.extend_from_slice(r);
                }
            }
            let b = *b;
            // SAFETY: the context is current, as this fn requires.
            unsafe { self.bind_array(b) };
            if !same {
                self.last_uploads += 1;
                let data = as_bytes(&self.tris_bufs[&s].1[at_k].1);
                if respecify {
                    // SAFETY: the context is current, as this fn requires.
                    unsafe {
                        gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, data, glow::DYNAMIC_DRAW)
                    };
                } else {
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { gl.buffer_sub_data_u8_slice(glow::ARRAY_BUFFER, 0, data) };
                }
            }
            None
        } else {
            let parts: Vec<&[u8]> = runs.iter().map(|r| as_bytes(r)).collect();
            // SAFETY: the context is current, as this fn requires.
            Some(unsafe { self.stream_parts(&parts) })
        };
        let o = streamed.unwrap_or(0);
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.attrib(0, true) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.attrib(1, true) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.attrib_ptr(0, 2, 24, o) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.attrib_ptr(1, 4, 24, o + 8) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { gl.draw_arrays(glow::TRIANGLES, 0, (n / 6) as i32) };
        if streamed.is_none() {
            // SAFETY: the context is current, as this fn requires.
            unsafe { self.bind_array(self.vbo) };
        }
    }

    /// The explicit modifiers EGL can import `fourcc` dmabufs with as a `GL_TEXTURE_2D`
    /// (EGL_EXT_image_dma_buf_import_modifiers, external-only ones left out); empty without it.
    pub fn dmabuf_modifiers(&self, fourcc: u32) -> Vec<u64> {
        type Query =
            unsafe extern "system" fn(*mut c_void, i32, i32, *mut u64, *mut u32, *mut i32) -> u32;
        let ext = self
            .egl
            .query_string(Some(self.display), egl::EXTENSIONS)
            .is_ok_and(|e| {
                e.to_string_lossy()
                    .split(' ')
                    .any(|e| e == "EGL_EXT_image_dma_buf_import_modifiers")
            });
        let Some(q) = ext_proc!(self.egl, "eglQueryDmaBufModifiersEXT", Query).filter(|_| ext)
        else {
            return Vec::new();
        };
        let dpy = self.display.as_ptr();
        let mut n = 0;
        // SAFETY: EGL_EXT_image_dma_buf_import_modifiers: with max_modifiers 0 and null arrays
        // the call only writes the count to `n`.
        let counted = unsafe {
            q(
                dpy,
                fourcc as i32,
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut n,
            )
        };
        if counted == 0 || n <= 0 {
            return Vec::new();
        }
        let mut mods = vec![0u64; n as usize];
        let mut ext_only = vec![0u32; n as usize];
        // SAFETY: as above; `mods` and `ext_only` hold the `n` entries the call is told.
        if unsafe {
            q(
                dpy,
                fourcc as i32,
                n,
                mods.as_mut_ptr(),
                ext_only.as_mut_ptr(),
                &mut n,
            )
        } == 0
        {
            return Vec::new();
        }
        mods.truncate(n.max(0) as usize);
        mods.into_iter()
            .zip(ext_only)
            .filter(|&(_, e)| e == 0)
            .map(|(m, _)| m)
            .collect()
    }

    /// A texture over a dmabuf: (fd, offset, stride) per plane, all with `modifier`.
    pub fn import_dmabuf(
        &mut self,
        planes: &[(i32, u32, u32)],
        w: u32,
        h: u32,
        fourcc: u32,
        modifier: u64,
    ) -> Option<(glow::Texture, usize)> {
        /// EGL_DMA_BUF_PLANE{0..3}_{FD,OFFSET,PITCH}_EXT, _MODIFIER_{LO,HI}_EXT.
        const PLANE: [[i32; 5]; 4] = [
            [0x3272, 0x3273, 0x3274, 0x3443, 0x3444],
            [0x3275, 0x3276, 0x3277, 0x3445, 0x3446],
            [0x3278, 0x3279, 0x327A, 0x3447, 0x3448],
            [0x3440, 0x3441, 0x3442, 0x3449, 0x344A],
        ];
        if planes.is_empty() || planes.len() > PLANE.len() {
            return None;
        }
        let image = self.image?;
        if !self.make_current() {
            return None;
        }
        let mut a = vec![0x3057, w as i32, 0x3056, h as i32, 0x3271, fourcc as i32];
        for (&(fd, offset, stride), k) in planes.iter().zip(PLANE) {
            a.extend([k[0], fd, k[1], offset as i32, k[2], stride as i32]);
            if modifier != crate::dmabuf::MOD_INVALID {
                a.extend([k[3], modifier as u32 as i32, k[4], (modifier >> 32) as i32]);
            }
        }
        a.push(0x3038);
        // SAFETY: EGL_EXT_image_dma_buf_import(_modifiers): `a` is an EGL_NONE-terminated
        // attribute list describing the caller's dmabuf planes, made on this display with no
        // context; the image is released with its texture in `free_dmabuf`, or right away on
        // failure.
        let img = unsafe {
            (image.create)(
                self.display.as_ptr(),
                std::ptr::null_mut(),
                0x3270,
                std::ptr::null_mut(),
                a.as_ptr(),
            )
        };
        if img.is_null() {
            return None;
        }
        let gl = &self.gl;
        // Errors of earlier calls are not this import's: clear them before the call checked below.
        // SAFETY: `make_current` above succeeded.
        unsafe { drain_gl_errors(gl, "before a dmabuf import") };
        // SAFETY: `make_current` above succeeded.
        let Ok(t) = (unsafe { gl.create_texture() }) else {
            // SAFETY: `img` is the EGLImage made above on this display, released once.
            unsafe { (image.destroy)(self.display.as_ptr(), img) };
            return None;
        };
        // SAFETY: `make_current` above succeeded.
        unsafe { gl.bind_texture(glow::TEXTURE_2D, Some(t)) };
        // SAFETY: OES_EGL_image: `img` is a live EGLImage of this display and `t` is bound to
        // TEXTURE_2D of the context `make_current` made current above.
        unsafe { (image.target)(glow::TEXTURE_2D, img) };
        for p in [glow::TEXTURE_WRAP_S, glow::TEXTURE_WRAP_T] {
            // SAFETY: `make_current` above succeeded.
            unsafe { gl.tex_parameter_i32(glow::TEXTURE_2D, p, glow::CLAMP_TO_EDGE as i32) };
        }
        // SAFETY: `make_current` above succeeded.
        let e = unsafe { gl.get_error() };
        if e != glow::NO_ERROR {
            tracing::error!("gpu: dmabuf {w}x{h} {fourcc:#010x} import: GL error {e:#06x}");
            // SAFETY: `make_current` above succeeded.
            unsafe { drain_gl_errors(gl, "in a failed dmabuf import") };
            self.free_dmabuf(t, img as usize);
            return None;
        }
        Some((t, img as usize))
    }

    /// Frees what `import_dmabuf` returned: the texture, then its EGLImage. A context that
    /// cannot be made current is lost with its textures (`retire`); the image is the display's
    /// and is destroyed either way.
    pub fn free_dmabuf(&mut self, t: glow::Texture, img: usize) {
        EXT.with(|m| m.borrow_mut().retain(|_, e| e.1 != t));
        if self.make_current() {
            // SAFETY: `make_current` above succeeded; `t` comes from `import_dmabuf` on this
            // context and is released once, before its image.
            unsafe { del_tex(&self.gl, &self.st, t) };
        }
        if let Some(i) = self.image {
            // SAFETY: `img` is the EGLImage `import_dmabuf` made on this display, released once.
            unsafe { (i.destroy)(self.display.as_ptr(), img as *mut c_void) };
        }
    }

    /// Whether `tex` (a dmabuf the compositor rewrites in place) differs from the copy kept in
    /// `prev` as a `view`-sized preview samples it: compared exactly on the GPU at that size
    /// (bilinear, as the preview draws it; the whole frame when not scaled down) and reduced
    /// to one pixel; refreshes the copy. `None` when that needs the GPU's answer: it is queued
    /// for `compare_read`, whose result goes back to the copy through `compare_apply`.
    /// Makes the context current itself (`make_current`), as `compare_submit`, `compare_read`
    /// and `free_frame_copy` do.
    pub fn frame_compare(
        &mut self,
        tex: glow::Texture,
        w: i32,
        h: i32,
        view: (i32, i32),
        prev: &mut Option<FrameCopy>,
    ) -> Option<bool> {
        // Without its context a frame is not compared: it counts as changed and is shown.
        if !self.make_current() {
            return Some(true);
        }
        if self.diff.is_none() {
            // SAFETY: `make_current` above succeeded.
            let Some(p) = (unsafe { program(&self.gl, DIFF_VS, DIFF_FS, &["p"]) }) else {
                return Some(true);
            };
            let u = ["a", "b", "s", "m"].map(|n| {
                // SAFETY: `make_current` above succeeded.
                unsafe { self.gl.get_uniform_location(p, n) }
            });
            self.diff = Some((p, u));
        }
        let view = if view.0 > 0 && view.1 > 0 && view.0 <= w && view.1 <= h {
            view
        } else {
            (w, h)
        };
        if prev
            .as_ref()
            .is_some_and(|c| c.size != (w, h) || c.view != view)
        {
            self.free_frame_copy(prev.take());
        }
        // Mid-run of a trusted change: nothing to draw or read back.
        if let Some(c) = prev.as_mut().filter(|c| c.trust > 1) {
            c.trust -= 1;
            DIFF_STATS[0].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            return Some(true);
        }
        let first = prev.is_none();
        if self.diff_row.is_none() {
            // SAFETY: `make_current` above succeeded.
            self.diff_row = unsafe { self.target(DIFF_ROW, 1) };
        }
        // Without its targets a frame is not compared: it counts as changed and is shown.
        if self.diff_row.is_none() {
            return Some(true);
        }
        if prev.is_none() {
            let mut chain = Vec::new();
            let (mut cw, mut ch) = view;
            let scratch = loop {
                // SAFETY: `make_current` above succeeded.
                let Some(t) = (unsafe { self.target(cw, ch) }) else {
                    break None;
                };
                chain.push(t);
                if (cw, ch) == (1, 1) && chain.len() > 1 {
                    // SAFETY: `make_current` above succeeded.
                    break unsafe { self.target(view.0, view.1) };
                }
                (cw, ch) = ((cw + 7) / 8, (ch + 7) / 8);
            };
            let Some(scratch) = scratch else {
                for (t, f, ..) in chain {
                    // SAFETY: `make_current` above succeeded.
                    unsafe { self.gl.delete_framebuffer(f) };
                    // SAFETY: `make_current` above succeeded.
                    unsafe { del_tex(&self.gl, &self.st, t) };
                }
                return Some(true);
            };
            *prev = Some(FrameCopy {
                size: (w, h),
                view,
                chain,
                scratch,
                trust: 0,
                run: 0,
            });
        }
        let copy = prev.as_mut().expect("frame copy");
        let gl = &self.gl;
        let (st, cache) = (&self.st, self.gl_cache);
        let (p, u) = self.diff.as_ref().unwrap();
        // SAFETY: `make_current` above succeeded.
        unsafe { self.use_prog(*p) };
        // SAFETY: `make_current` above succeeded.
        unsafe { gl.disable(glow::BLEND) };
        // SAFETY: `make_current` above succeeded.
        unsafe { gl.disable(glow::SCISSOR_TEST) };
        // SAFETY: `make_current` above succeeded.
        unsafe { self.bind_array(self.vbo) };
        // SAFETY: `make_current` above succeeded.
        let o = unsafe { self.stream(as_bytes(&[-1f32, -1.0, 3.0, -1.0, -1.0, 3.0])) };
        // SAFETY: `make_current` above succeeded.
        unsafe { self.attrib(0, true) };
        // SAFETY: `make_current` above succeeded.
        unsafe { self.attrib(1, false) };
        // SAFETY: `make_current` above succeeded.
        unsafe { self.attrib_ptr(0, 2, 8, o) };
        // SAFETY: `make_current` above succeeded.
        unsafe { self.set_u(P_DIFF, 0, u[0].as_ref(), Uni::I(0)) };
        // SAFETY: `make_current` above succeeded.
        unsafe { self.set_u(P_DIFF, 1, u[1].as_ref(), Uni::I(1)) };
        let sample = |t: glow::Texture, unit: u32, filter: u32| {
            // SAFETY: `make_current` above succeeded.
            unsafe { gl.active_texture(unit) };
            // SAFETY: `make_current` above succeeded.
            unsafe { gl.bind_texture(glow::TEXTURE_2D, Some(t)) };
            // SAFETY: `make_current` above succeeded.
            unsafe { filter_set(gl, st, cache, t, filter as i32) };
        };
        let pass = |src: (i32, i32), dst: &(glow::Texture, glow::Framebuffer, i32, i32), m: f32| {
            // SAFETY: `make_current` above succeeded.
            unsafe { gl.bind_framebuffer(glow::FRAMEBUFFER, Some(dst.1)) };
            // SAFETY: `make_current` above succeeded.
            unsafe { gl.viewport(0, 0, dst.2, dst.3) };
            let size = Uni::f(&[src.0 as f32, src.1 as f32]);
            // SAFETY: `make_current` above succeeded.
            unsafe { uni_set(gl, st, cache, P_DIFF, 2, u[2].as_ref(), size) };
            // SAFETY: `make_current` above succeeded.
            unsafe { uni_set(gl, st, cache, P_DIFF, 3, u[3].as_ref(), Uni::f(&[m])) };
            // SAFETY: `make_current` above succeeded.
            unsafe { gl.draw_arrays(glow::TRIANGLES, 0, 3) };
        };
        sample(tex, glow::TEXTURE0, glow::LINEAR);
        let trusted = !first && copy.trust > 0;
        if trusted {
            copy.trust -= 1;
        }
        if first || (trusted && copy.trust == 0) {
            pass(view, &copy.chain[0], 2.0);
        } else if !trusted {
            pass(view, &copy.scratch, 2.0);
        }
        let decided = (first || trusted).then_some(true);
        if decided.is_none() {
            sample(copy.scratch.0, glow::TEXTURE0, glow::NEAREST);
            sample(copy.chain[0].0, glow::TEXTURE1, glow::NEAREST);
            pass(view, &copy.chain[1], 1.0);
            for k in 1..copy.chain.len() - 1 {
                let c = &copy.chain[k];
                sample(c.0, glow::TEXTURE0, glow::NEAREST);
                pass((c.2, c.3), &copy.chain[k + 1], 0.0);
            }
            // One readback per batch of compares, not per frame: each a pixel of the row.
            let row = self.diff_row.expect("diff row");
            // SAFETY: `make_current` above succeeded.
            unsafe { gl.active_texture(glow::TEXTURE0) };
            // SAFETY: `make_current` above succeeded.
            unsafe { gl.bind_texture(glow::TEXTURE_2D, Some(row.0)) };
            // SAFETY: `make_current` above succeeded.
            unsafe {
                gl.copy_tex_sub_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    self.diff_queued as i32,
                    0,
                    0,
                    0,
                    1,
                    1,
                )
            };
            self.diff_queued += 1;
        }
        // SAFETY: `make_current` above succeeded.
        unsafe { gl.active_texture(glow::TEXTURE0) };
        // SAFETY: `make_current` above succeeded.
        unsafe { gl.bind_framebuffer(glow::FRAMEBUFFER, None) };
        // SAFETY: `make_current` above succeeded.
        unsafe { self.attrib(1, true) };
        // SAFETY: `make_current` above succeeded.
        unsafe { gl.enable(glow::BLEND) };
        decided
    }

    /// Whether the next `frame_compare` needs a `compare_read` first.
    pub fn compare_full(&self) -> bool {
        self.diff_queued >= DIFF_ROW as usize
    }

    /// Ends a batch of `frame_compare`s whose answers can wait: a native fence follows them
    /// (EGL_ANDROID_native_fence_sync), and the returned sync file becomes readable once the
    /// GPU has finished them, so `compare_read` then gets them without stalling (ES2 has no
    /// pixel pack buffers to read into asynchronously). None when nothing can tell when:
    /// `compare_read` waits for the GPU then.
    pub fn compare_submit(&mut self) -> Option<std::os::fd::OwnedFd> {
        use std::os::fd::FromRawFd;
        if self.diff_queued == 0 || self.diff_sync.is_some() {
            return None;
        }
        let f = self.fence?;
        let dup = f.dup?;
        // The fence goes into the command stream of the current context, after the compares.
        if !self.make_current() {
            return None;
        }
        let attrs = [
            EGL_SYNC_NATIVE_FENCE_FD_ANDROID,
            EGL_NO_NATIVE_FENCE_FD_ANDROID,
            egl::NONE,
        ];
        // SAFETY: EGL_ANDROID_native_fence_sync: a native fence sync on this display, whose
        // context `make_current` made current above, with an
        // EGL_NONE-terminated attribute list asking for a new fence fd; it is destroyed once in
        // `drop_sync`.
        let s = unsafe {
            (f.create)(
                self.display.as_ptr(),
                EGL_SYNC_NATIVE_FENCE_ANDROID,
                attrs.as_ptr(),
            )
        };
        if s.is_null() {
            return None;
        }
        self.diff_sync = Some(s);
        // The fence fd exists once the fence command is flushed.
        // SAFETY: `make_current` above succeeded.
        unsafe { self.gl.flush() };
        // SAFETY: EGL_ANDROID_native_fence_sync: `s` is the live native fence sync made above.
        let fd = unsafe { dup(self.display.as_ptr(), s) };
        // SAFETY: eglDupNativeFenceFDANDROID returns a new fd the caller owns, or -1.
        (fd >= 0).then(|| unsafe { std::os::fd::OwnedFd::from_raw_fd(fd) })
    }

    /// The GPU has finished the compares `compare_submit` fenced (always true without a fence:
    /// the read then waits). A status query, made when the fence fd signalled or another
    /// event ran the loop.
    pub fn compare_ready(&self) -> bool {
        match (self.fence, self.diff_sync) {
            // SAFETY: EGL_KHR_fence_sync: `s` is a live sync of this display (`diff_sync` is
            // cleared when it is destroyed); a zero timeout only queries its status.
            (Some(f), Some(s)) => unsafe {
                (f.wait)(self.display.as_ptr(), s, 0, 0) != EGL_TIMEOUT_EXPIRED_KHR
            },
            _ => true,
        }
    }

    fn drop_sync(&mut self, wait: bool) {
        if let (Some(f), Some(s)) = (self.fence, self.diff_sync.take()) {
            if wait {
                // SAFETY: EGL_KHR_fence_sync: `s` is the live sync just taken from `diff_sync`.
                unsafe { (f.wait)(self.display.as_ptr(), s, 0, EGL_FOREVER_KHR) };
            }
            // SAFETY: as above; `s` was taken out of `diff_sync`, so it is destroyed once.
            unsafe { (f.destroy)(self.display.as_ptr(), s) };
        }
    }

    /// Whether each frame queued by `frame_compare` changed, in queue order.
    pub fn compare_read(&mut self) -> Vec<bool> {
        let n = std::mem::take(&mut self.diff_queued);
        self.drop_sync(true);
        let Some(row) = self.diff_row.filter(|_| n > 0) else {
            return Vec::new();
        };
        // Without its context the compares cannot be read: each frame counts as changed.
        if !self.make_current() {
            return vec![true; n];
        }
        let mut px = vec![0u8; n * 4];
        // SAFETY: `make_current` above succeeded.
        unsafe { self.gl.bind_framebuffer(glow::FRAMEBUFFER, Some(row.1)) };
        // SAFETY: `make_current` above succeeded.
        unsafe {
            self.gl.read_pixels(
                0,
                0,
                n as i32,
                1,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut px)),
            )
        };
        // SAFETY: `make_current` above succeeded.
        unsafe { self.gl.bind_framebuffer(glow::FRAMEBUFFER, None) };
        px.as_chunks::<4>().0.iter().map(|p| p[0] != 0).collect()
    }

    /// Forgets the queued compares unread, for frames shown anyway (`compare_drop`).
    pub fn compare_forget(&mut self) {
        self.diff_queued = 0;
        self.drop_sync(false);
    }

    /// Takes a queued compare's frame into its copy without its result, keeping the run.
    pub fn compare_drop(prev: &mut Option<FrameCopy>) {
        DIFF_STATS[3].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if let Some(copy) = prev.as_mut() {
            std::mem::swap(&mut copy.scratch, &mut copy.chain[0]);
        }
    }

    /// Takes a queued compare's result (`compare_read`) into its copy.
    pub fn compare_apply(prev: &mut Option<FrameCopy>, changed: bool) {
        DIFF_STATS[1 + changed as usize].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let Some(copy) = prev.as_mut() else {
            return;
        };
        if changed {
            std::mem::swap(&mut copy.scratch, &mut copy.chain[0]);
            copy.run = if copy.run == 0 {
                DIFF_TRUST
            } else {
                (copy.run * 2 + 1).min(DIFF_TRUST_MAX)
            };
            copy.trust = copy.run;
        } else {
            copy.run = 0;
        }
    }

    #[cfg(test)]
    fn frame_changed(
        &mut self,
        tex: glow::Texture,
        w: i32,
        h: i32,
        view: (i32, i32),
        prev: &mut Option<FrameCopy>,
    ) -> bool {
        if let Some(changed) = self.frame_compare(tex, w, h, view, prev) {
            return changed;
        }
        let changed = self.compare_read()[0];
        Self::compare_apply(prev, changed);
        changed
    }

    /// Gives `f`, the bound framebuffer of w x h, a depth buffer, which lives as long as it.
    ///
    /// # Safety
    /// This `Gpu`'s context must be current on the calling thread.
    unsafe fn depth(&self, f: glow::Framebuffer, w: i32, h: i32) {
        let gl = &self.gl;
        // SAFETY: the context is current, as this fn requires.
        let Ok(rb) = (unsafe { gl.create_renderbuffer() }) else {
            return;
        };
        // SAFETY: the context is current, as this fn requires.
        unsafe { gl.bind_renderbuffer(glow::RENDERBUFFER, Some(rb)) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { gl.renderbuffer_storage(glow::RENDERBUFFER, glow::DEPTH_COMPONENT16, w, h) };
        // SAFETY: the context is current, as this fn requires.
        unsafe {
            gl.framebuffer_renderbuffer(
                glow::FRAMEBUFFER,
                glow::DEPTH_ATTACHMENT,
                glow::RENDERBUFFER,
                Some(rb),
            )
        };
        // SAFETY: the context is current, as this fn requires.
        unsafe { gl.bind_renderbuffer(glow::RENDERBUFFER, None) };
        // A deleted renderbuffer stays attached, and alive until its framebuffer goes,
        // unless that framebuffer is the bound one (GLES 2.0 §4.4.3).
        // SAFETY: the context is current, as this fn requires.
        unsafe { gl.bind_framebuffer(glow::FRAMEBUFFER, None) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { gl.delete_renderbuffer(rb) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { gl.bind_framebuffer(glow::FRAMEBUFFER, Some(f)) };
    }

    /// A new w x h RGBA target, bound; None (logged) when GL cannot make a complete one: past
    /// GL_MAX_TEXTURE_SIZE or out of memory (GLES 2.0 §4.4.5).
    ///
    /// # Safety
    /// This `Gpu`'s context must be current on the calling thread.
    unsafe fn target(
        &self,
        w: i32,
        h: i32,
    ) -> Option<(glow::Texture, glow::Framebuffer, i32, i32)> {
        #[cfg(test)]
        match self.targets_left.get() {
            Some(0) => return None,
            n => self.targets_left.set(n.map(|n| n - 1)),
        }
        let gl = &self.gl;
        // SAFETY: the context is current, as this fn requires.
        let t = match unsafe { gl.create_texture() } {
            Ok(t) => t,
            Err(e) => {
                tracing::error!("gpu: cannot create a {w}x{h} target texture: {e}");
                return None;
            }
        };
        // SAFETY: the context is current, as this fn requires.
        unsafe { gl.bind_texture(glow::TEXTURE_2D, Some(t)) };
        for (k, v) in [
            (glow::TEXTURE_MIN_FILTER, glow::NEAREST),
            (glow::TEXTURE_MAG_FILTER, glow::NEAREST),
            (glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE),
            (glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE),
        ] {
            // SAFETY: the context is current, as this fn requires.
            unsafe { gl.tex_parameter_i32(glow::TEXTURE_2D, k, v as i32) };
        }
        self.st.borrow_mut().filters.insert(t, glow::NEAREST as i32);
        // SAFETY: the context is current, as this fn requires.
        unsafe {
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA as i32,
                w,
                h,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(None),
            )
        };
        // SAFETY: the context is current, as this fn requires.
        let f = match unsafe { gl.create_framebuffer() } {
            Ok(f) => f,
            Err(e) => {
                tracing::error!("gpu: cannot create a {w}x{h} framebuffer: {e}");
                // SAFETY: the context is current, as this fn requires.
                unsafe { del_tex(gl, &self.st, t) };
                return None;
            }
        };
        // SAFETY: the context is current, as this fn requires.
        unsafe { gl.bind_framebuffer(glow::FRAMEBUFFER, Some(f)) };
        // SAFETY: the context is current, as this fn requires.
        unsafe {
            gl.framebuffer_texture_2d(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(t),
                0,
            )
        };
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.depth(f, w, h) };
        let gl = &self.gl;
        // SAFETY: the context is current, as this fn requires.
        let status = unsafe { gl.check_framebuffer_status(glow::FRAMEBUFFER) };
        if status != glow::FRAMEBUFFER_COMPLETE {
            tracing::error!("gpu: {w}x{h} target incomplete (status {status:#x})");
            // SAFETY: the context is current, as this fn requires.
            unsafe { gl.bind_framebuffer(glow::FRAMEBUFFER, None) };
            // SAFETY: the context is current, as this fn requires.
            unsafe { gl.delete_framebuffer(f) };
            // SAFETY: the context is current, as this fn requires.
            unsafe { del_tex(gl, &self.st, t) };
            return None;
        }
        Some((t, f, w, h))
    }

    /// Frees a `frame_compare` copy's targets. A context that cannot be made current is lost
    /// with them.
    pub fn free_frame_copy(&mut self, c: Option<FrameCopy>) {
        if c.is_none() || !self.make_current() {
            return;
        }
        for (t, f, ..) in c
            .into_iter()
            .flat_map(|c| c.chain.into_iter().chain([c.scratch]))
        {
            // SAFETY: `make_current` above succeeded.
            unsafe { self.gl.delete_framebuffer(f) };
            // SAFETY: `make_current` above succeeded.
            unsafe { del_tex(&self.gl, &self.st, t) };
        }
    }

    /// Moves the textures of images that dropped their pixels to their stand-ins.
    fn adopt_resident(&mut self) {
        if ADOPT.with_borrow(|a| a.is_empty()) {
            return;
        }
        for (old, new) in ADOPT.with_borrow_mut(std::mem::take) {
            let k = old.key.get();
            let Some(n) = new.upgrade() else { continue };
            if let Some((_, t)) = self.images.remove(&k) {
                let nk = n.key.get();
                self.images.insert(nk, (new, t));
                UPLOADED.with_borrow_mut(|u| {
                    u.remove(&k);
                    u.insert(nk);
                });
            }
        }
    }

    /// Texture for a draw, cached per shared image (`Canvas::key`).
    ///
    /// None when there is nothing to sample: an image without pixels (a dmabuf frame whose
    /// texture is gone) or a texture GL could not create. The draw is skipped then, as Qt does
    /// when `updatePaintNode` has no texture.
    ///
    /// # Safety
    /// This `Gpu`'s context must be current on the calling thread.
    unsafe fn texture(&mut self, src: &Rc<Canvas>) -> Option<glow::Texture> {
        if let Some((t, _)) = ext(src) {
            return Some(t);
        }
        self.adopt_resident();
        let key = src.key.get();
        if let Some((_, t)) = self.images.get(&key) {
            return Some(*t);
        }
        let img = &**src;
        if img.px.len() < img.w * img.h {
            return None;
        }
        // QSGPlainTexture: an image past the texture size limit uploads scaled down to it, and
        // its draws (UVs 0..1) cover the same rect.
        let (mw, mh) = (img.w.min(self.max_tex), img.h.min(self.max_tex));
        let scaled = ((mw, mh) != (img.w, img.h)).then(|| {
            tracing::debug!(target: "ri::gpu", "texture {}x{} scaled to {mw}x{mh}", img.w, img.h);
            qsg::image::smooth_scale(img, mw, mh, true)
        });
        let img = scaled.as_ref().unwrap_or(img);
        if qml::log_on!("ri::stats") && img.w * img.h > 40_000 {
            tracing::debug!(target: "ri::stats", "  upload {}x{} shared", img.w, img.h);
        }
        glcount(0, img.w * img.h);
        let gl = &self.gl;
        // SAFETY: the context is current, as this fn requires.
        let t = unsafe {
            match gl.create_texture() {
                Ok(t) => t,
                Err(e) => {
                    tracing::error!("gpu: cannot create a texture: {e}");
                    return None;
                }
            }
        };
        // SAFETY: the context is current, as this fn requires.
        unsafe { gl.bind_texture(glow::TEXTURE_2D, Some(t)) };
        for p in [glow::TEXTURE_WRAP_S, glow::TEXTURE_WRAP_T] {
            // SAFETY: the context is current, as this fn requires.
            unsafe { gl.tex_parameter_i32(glow::TEXTURE_2D, p, glow::CLAMP_TO_EDGE as i32) };
        }
        gl_count(0);
        // SAFETY: the context is current, as this fn requires.
        unsafe {
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA as i32,
                img.w as i32,
                img.h as i32,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(Some(as_bytes(&img.px))),
            )
        };
        self.images.insert(key, (Rc::downgrade(src), t));
        UPLOADED.with_borrow_mut(|u| u.insert(key));
        Some(t)
    }

    /// Whether the texture `t` of the image `src` has its chain of mipmap levels, made here
    /// on the first ask (QSGPlainTexture: QRhiResourceUpdateBatch::generateMips). Not for a
    /// texture another process gave (a dmabuf frame), nor, without GL_OES_texture_npot on
    /// GLES 2, for a side that is not a power of two: the draw goes without mipmaps then.
    ///
    /// # Safety
    /// This `Gpu`'s context must be current on the calling thread, with `t` bound to
    /// TEXTURE_2D of the active unit.
    unsafe fn mipmapped(&mut self, src: &Rc<Canvas>, t: glow::Texture) -> bool {
        if self.st.borrow().mipped.contains(&t) {
            return true;
        }
        let pot = src.w.is_power_of_two() && src.h.is_power_of_two();
        if ext(src).is_some() || !(pot || self.npot_mips) {
            return false;
        }
        // SAFETY: the context is current and `t` is bound, as this fn requires.
        unsafe { self.gl.generate_mipmap(glow::TEXTURE_2D) };
        self.st.borrow_mut().mipped.insert(t);
        true
    }

    /// Draws the triangles of `v` (see `draw_tex_quads`) sampling `tex` in a `w` x `h` draw space,
    /// with the texture program's `k`, fragment `mode` and opacity `op`.
    ///
    /// # Safety
    /// This `Gpu`'s context must be current on the calling thread, with `vbo` its bound array
    /// buffer (`bind_array`): the vertices are written to the bound buffer and drawn from it.
    #[allow(clippy::too_many_arguments)]
    unsafe fn quads(
        &self,
        v: &[f32],
        tex: glow::Texture,
        k: [f32; 4],
        mode: f32,
        op: f32,
        w: i32,
        h: i32,
    ) {
        let gl = &self.gl;
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.use_prog(self.tex) };
        let u = &self.tex_u;
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.set_u(P_TEX, 0, u[0].as_ref(), Uni::f(&self.space(w, h))) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.set_u(P_TEX, 1, u[1].as_ref(), Uni::f(&k)) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.set_u(P_TEX, 2, u[2].as_ref(), Uni::f(&[mode])) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.set_u(P_TEX, 3, u[3].as_ref(), Uni::f(&[op])) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.set_u(P_TEX, 4, u[4].as_ref(), Uni::I(0)) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.set_u(P_TEX, 5, u[5].as_ref(), Uni::I(1)) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { gl.active_texture(glow::TEXTURE0) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { gl.bind_texture(glow::TEXTURE_2D, Some(tex)) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.draw_tex_quads(v) };
    }

    /// Uploads `img` to the scratch texture and draws it at (`x`, `y`) of a `w` x `h` draw space.
    ///
    /// # Safety
    /// This `Gpu`'s context must be current on the calling thread, with `vbo` its bound array
    /// buffer (`bind_array`): the vertices are written to the bound buffer and drawn from it.
    unsafe fn draw_blit(&self, img: &Canvas, x: i64, y: i64, w: i32, h: i32) {
        // GL reads `w * h` pixels from the buffer: one that is shorter is not drawn, as
        // `texture` skips it.
        if img.px.len() < img.w * img.h {
            return;
        }
        glcount(2, img.w * img.h);
        let gl = &self.gl;
        // SAFETY: the context is current, as this fn requires.
        unsafe { gl.bind_texture(glow::TEXTURE_2D, Some(self.scratch)) };
        gl_count(0);
        // SAFETY: the context is current, as this fn requires.
        unsafe {
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA as i32,
                img.w as i32,
                img.h as i32,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(Some(as_bytes(&img.px))),
            )
        };
        let mut v = Vec::with_capacity(24);
        quad(
            &mut v,
            x as f32,
            y as f32,
            img.w as f32,
            img.h as f32,
            [0.0, 0.0, 1.0, 1.0],
        );
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.filter(self.scratch, glow::NEAREST as i32) };
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.quads(&v, self.scratch, [0.0; 4], PLAIN, 1.0, w, h) };
    }

    /// Glyph bitmaps at their own size from each quad's position, in colour `col`.
    ///
    /// # Safety
    /// This `Gpu`'s context must be current on the calling thread, with `vbo` its bound array
    /// buffer (`bind_array`): the vertices are written to the bound buffer and drawn from it.
    unsafe fn draw_glyphs(&mut self, col: [f32; 4], q: &[GlyphQuad], w: i32, h: i32) {
        let rects = q.iter().map(|(g, x0, y0)| {
            let (x0, y0) = (*x0 as f32, *y0 as f32);
            (g, [x0, y0, x0 + g.width as f32, y0 + g.rows as f32])
        });
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.draw_glyph_rects(col, rects, w, h) }
    }

    /// Glyph bitmaps from the atlas stretched over device rects (x1 y1 x2 y2). Colour glyphs
    /// come from the RGBA atlas, drawn as premultiplied texels times the text alpha.
    ///
    /// # Safety
    /// This `Gpu`'s context must be current on the calling thread, with `vbo` its bound array
    /// buffer (`bind_array`): the vertices are written to the bound buffer and drawn from it.
    unsafe fn draw_glyph_rects<'a>(
        &mut self,
        col: [f32; 4],
        q: impl Iterator<Item = (&'a Rc<qsg::text::Glyph>, [f32; 4])>,
        w: i32,
        h: i32,
    ) {
        let (colored, mono): (Vec<_>, Vec<_>) = q.partition(|(g, _)| g.color);
        // SAFETY: the context is current, as this fn requires.
        unsafe { self.glyph_pass(false, col, &mono, w, h) };
        if !colored.is_empty() {
            // SAFETY: the context is current, as this fn requires.
            unsafe { self.glyph_pass(true, col, &colored, w, h) };
        }
    }

    /// The glyphs `q` of one atlas (the RGBA one if `color`), batched: a glyph not in the atlas is
    /// uploaded first, and a full atlas is drawn from, emptied and grown.
    ///
    /// # Safety
    /// This `Gpu`'s context must be current on the calling thread, with `vbo` its bound array
    /// buffer (`bind_array`): the vertices are written to the bound buffer and drawn from it.
    unsafe fn glyph_pass(
        &mut self,
        color: bool,
        col: [f32; 4],
        q: &[(&Rc<qsg::text::Glyph>, [f32; 4])],
        w: i32,
        h: i32,
    ) {
        let (mode, op) = if color { (PLAIN, col[3]) } else { (MASK, 1.0) };
        let tex = if color {
            self.catlas.tex
        } else {
            self.atlas.tex
        };
        let mut v = Vec::new();
        for &(g, r) in q {
            if g.width == 0 || g.rows == 0 {
                continue;
            }
            // SAFETY: the context is current, as this fn requires.
            let slot = match unsafe { self.place(g) } {
                Some(s) => s,
                None => {
                    // Atlas full of glyphs still in use: draw what is batched, then start over
                    // in a texture twice the size (up to ATLAS); glyphs re-upload as drawn.
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.quads(&v, tex, col, mode, op, w, h) };
                    v.clear();
                    let at = if color { &self.catlas } else { &self.atlas };
                    let size = at.next_size(g.width as i32, g.rows as i32);
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.atlas_reset(color, size) };
                    // SAFETY: the context is current, as this fn requires.
                    let Some(s) = (unsafe { self.place(g) }) else {
                        continue;
                    };
                    s
                }
            };
            let (gw, gh) = (g.width as f32, g.rows as f32);
            let a = if color {
                self.catlas.size
            } else {
                self.atlas.size
            } as f32;
            // A scaled quad can put pixel centres exactly on texel edges; the bias picks the
            // same texel whichever way the target is flipped (window vs offscreen layer).
            const TIE: f32 = 1.0 / 256.0;
            quad(
                &mut v,
                r[0],
                r[1],
                r[2] - r[0],
                r[3] - r[1],
                [
                    (slot.0 as f32 + TIE) / a,
                    (slot.1 as f32 + TIE) / a,
                    gw / a,
                    gh / a,
                ],
            );
        }
        if !v.is_empty() {
            // SAFETY: the context is current, as this fn requires.
            unsafe { self.quads(&v, tex, col, mode, op, w, h) };
        }
    }

    /// Empties a glyph atlas and (re)allocates its texture at `size` x `size`.
    ///
    /// # Safety
    /// This `Gpu`'s context must be current on the calling thread.
    unsafe fn atlas_reset(&mut self, color: bool, size: i32) {
        let at = if color {
            &mut self.catlas
        } else {
            &mut self.atlas
        };
        // SAFETY: the context is current, as this fn requires.
        unsafe { Self::reset_atlas(&self.gl, at, size) };
    }

    /// Empties `at` and (re)allocates its texture at `size` x `size`.
    ///
    /// # Safety
    /// The GL context `gl` was loaded for must be current on the calling thread.
    unsafe fn reset_atlas(gl: &glow::Context, at: &mut Atlas, size: i32) {
        at.slots.clear();
        at.areas = qsg::areaalloc::AreaAllocator::new(size, size);
        if at.size == size {
            return;
        }
        at.size = size;
        tracing::debug!(target: "ri::gpu", "atlas (rgba {}) {size}x{size}", at.rgba);
        let fmt = if at.rgba { glow::RGBA } else { glow::ALPHA };
        let tex = at.tex;
        // SAFETY: `gl` is current, as this fn requires.
        unsafe { gl.bind_texture(glow::TEXTURE_2D, Some(tex)) };
        gl_count(0);
        // SAFETY: `gl` is current, as this fn requires.
        unsafe {
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                fmt as i32,
                size,
                size,
                0,
                fmt,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(None),
            )
        };
    }

    /// Where glyph `g` is in its atlas, uploaded first if it is new ([`Atlas::place`]).
    ///
    /// # Safety
    /// This `Gpu`'s context must be current on the calling thread.
    unsafe fn place(&mut self, g: &Rc<qsg::text::Glyph>) -> Option<(i32, i32)> {
        let key = g.key.get();
        let at = if g.color {
            &mut self.catlas
        } else {
            &mut self.atlas
        };
        let b: Rc<dyn std::any::Any> = g.clone();
        // SAFETY: the context is current, as this fn requires.
        unsafe { at.place(&self.gl, key, &b, g.width as i32, g.rows as i32, &g.buf) }
    }

    /// Distance-field tiles (`Cmd::Df`, blend set) from the tile atlas in one draw, as the Qt
    /// renderer batches distance-field glyph nodes.
    ///
    /// # Safety
    /// This `Gpu`'s context must be current on the calling thread, with `vbo` its bound array
    /// buffer (`bind_array`): the vertices are written to the bound buffer and drawn from it.
    unsafe fn df_pass(&mut self, tiles: &[qsg::dftext::DfTile], w: i32, h: i32) {
        let mut v = Vec::with_capacity(tiles.len() * 24);
        for (t, x0, y0) in tiles {
            let (tw, th) = (t.w as i32, t.h as i32);
            let b: Rc<dyn std::any::Any> = t.clone();
            let px = as_bytes(&t.px);
            // SAFETY: the context is current, as this fn requires.
            let mut slot = unsafe { self.dfatlas.place(&self.gl, t.key.get(), &b, tw, th, px) };
            if slot.is_none() && (tw < self.dfatlas.max && th < self.dfatlas.max) {
                // Full of tiles still drawn: draw what is batched and start over (bigger).
                if !v.is_empty() {
                    // SAFETY: the context is current, as this fn requires.
                    unsafe { self.quads(&v, self.dfatlas.tex, [0.0; 4], PLAIN, 1.0, w, h) };
                    v.clear();
                }
                let size = self.dfatlas.next_size(tw, th);
                // SAFETY: the context is current, as this fn requires.
                unsafe { Self::reset_atlas(&self.gl, &mut self.dfatlas, size) };
                // SAFETY: the context is current, as this fn requires.
                slot = unsafe { self.dfatlas.place(&self.gl, t.key.get(), &b, tw, th, px) };
            }
            let Some((sx, sy)) = slot else {
                // Larger than any atlas: its own texture.
                // SAFETY: the context is current, as this fn requires.
                let Some(tex) = (unsafe { self.texture(t) }) else {
                    continue;
                };
                // SAFETY: the context is current, as this fn requires.
                unsafe { self.filter(tex, glow::NEAREST as i32) };
                let mut one = Vec::with_capacity(24);
                quad(
                    &mut one,
                    *x0 as f32,
                    *y0 as f32,
                    tw as f32,
                    th as f32,
                    [0.0, 0.0, 1.0, 1.0],
                );
                // SAFETY: the context is current, as this fn requires.
                unsafe { self.quads(&one, tex, [0.0; 4], PLAIN, 1.0, w, h) };
                continue;
            };
            let a = self.dfatlas.size as f32;
            quad(
                &mut v,
                *x0 as f32,
                *y0 as f32,
                tw as f32,
                th as f32,
                [sx as f32 / a, sy as f32 / a, tw as f32 / a, th as f32 / a],
            );
        }
        if !v.is_empty() {
            // SAFETY: the context is current, as this fn requires.
            unsafe { self.quads(&v, self.dfatlas.tex, [0.0; 4], PLAIN, 1.0, w, h) };
        }
    }
}

/// No pixel centre lies within the box (in x or y), so a primitive inside it rasterizes no
/// fragments. The margin covers vertex snapping to the rasterizer's sub-pixel grid.
fn covers_no_pixel(b: [f32; 4]) -> bool {
    const E: f32 = 1.0 / 64.0;
    let none = |lo: f32, hi: f32| (lo - E - 0.5).ceil() > (hi + E - 0.5).floor();
    none(b[0], b[2]) || none(b[1], b[3])
}

/// The four corners of a rect as a triangle strip, in the order Qt Quick gives them (top
/// left, bottom left, top right, bottom right; QSGGeometry::updateTexturedRectGeometry).
/// llvmpipe draws two separate triangles that make a rectangle as one rectangle, with one
/// plane per texture coordinate (lp_setup_rect.c), and a strip as two triangles with a plane
/// each: at a fractional size the coordinate at a pixel differs in its last bit, and a
/// sample that sits on a 1/256 step of a texel then lands on the other side of it.
fn strip(x: f32, y: f32, w: f32, h: f32, uv: [f32; 4]) -> [f32; 16] {
    let (u0, v0, u1, v1) = (uv[0], uv[1], uv[0] + uv[2], uv[1] + uv[3]);
    [
        x,
        y,
        u0,
        v0,
        x,
        y + h,
        u0,
        v1,
        x + w,
        y,
        u1,
        v0,
        x + w,
        y + h,
        u1,
        v1,
    ]
}

/// A blur pass's vertices: the effect's own, through the matrix `m` of its node, as a
/// ShaderEffect's vertex shader has them (`qt_Matrix * qt_Vertex`).
const GAUSS_VS: &str = "attribute vec4 p; attribute vec2 t; uniform mat4 m; varying vec2 u;
void main() { u = t; gl_Position = m * p; }";

/// A rect's two triangles in the order of Qt's four vertices (top left, bottom left, top
/// right, bottom right) drawn as a strip.
fn quad(v: &mut Vec<f32>, x: f32, y: f32, w: f32, h: f32, uv: [f32; 4]) {
    let (u0, v0, u1, v1) = (uv[0], uv[1], uv[0] + uv[2], uv[1] + uv[3]);
    for (px, py, u, t) in [
        (x, y, u0, v0),
        (x, y + h, u0, v1),
        (x + w, y, u1, v0),
        (x + w, y, u1, v0),
        (x, y + h, u0, v1),
        (x + w, y + h, u1, v1),
    ] {
        v.extend_from_slice(&[px, py, u, t]);
    }
}

/// The one cell of a ShaderEffect's mesh over `at` (x y w h) with the whole texture
/// (QQuickGridMesh::updateGeometry: a strip per row, bottom left first): its two triangles
/// share the diagonal from the top left, not a rect's from the top right.
fn grid(v: &mut Vec<f32>, [x, y, w, h]: [f32; 4]) {
    let (tl, bl) = ([x, y, 0.0, 0.0], [x, y + h, 0.0, 1.0]);
    let (tr, br) = ([x + w, y, 1.0, 0.0], [x + w, y + h, 1.0, 1.0]);
    for q in [tl, bl, br, tl, br, tr] {
        v.extend_from_slice(&q);
    }
}

/// A type whose every byte is initialized: no padding (so it can be read as `[u8]`).
///
/// # Safety
/// `Self` has no padding bytes and no uninitialized ones.
unsafe trait NoPadding: Copy {}
// SAFETY: a primitive number has no padding.
unsafe impl NoPadding for u8 {}
// SAFETY: a primitive number has no padding.
unsafe impl NoPadding for f32 {}
// SAFETY: an array is its elements laid out one after another, without padding between them
// (the element's size is a multiple of its alignment), and `T` has none inside.
unsafe impl<T: NoPadding, const N: usize> NoPadding for [T; N] {}

/// The bytes of `v`: numbers and `[u8; 4]` pixels. A `T` with padding (reading a padding byte
/// as `u8` is undefined) does not implement `NoPadding` and does not compile here
/// (`as_bytes_takes_no_padded_type`).
fn as_bytes<T: NoPadding>(v: &[T]) -> &[u8] {
    // SAFETY: the pointer and the length are those of the live slice `v`, borrowed for the
    // returned lifetime; `u8` has alignment 1, and `T: NoPadding` has every byte initialized.
    unsafe { std::slice::from_raw_parts(v.as_ptr() as *const u8, std::mem::size_of_val(v)) }
}

/// Per counter (calls, pixels), and when counting started.
type GlCounts = ([(u64, u64); 5], Option<std::time::Instant>);
thread_local!(static GLCOUNT: std::cell::RefCell<GlCounts> = Default::default());
fn glcount(k: usize, px: usize) {
    if !qml::log_on!("ri::glcount") {
        return;
    }
    GLCOUNT.with_borrow_mut(|(c, t)| {
        c[k].0 += 1;
        c[k].1 += px as u64;
        let now = std::time::Instant::now();
        let t0 = *t.get_or_insert(now);
        if now.duration_since(t0).as_secs_f32() >= 1.0 {
            tracing::debug!(
                target: "ri::glcount",
                "glcount shared {:?} owned {:?} blit {:?} newlayer {:?} poollayer {:?}",
                c[0], c[1], c[2], c[3], c[4]
            );
            *c = Default::default();
            *t = Some(now);
        }
    });
}

/// Ends a GPU test that lacks `what` (a Wayland compositor, an EGL display): unrun where
/// that is allowed, a failure where RI_REQUIRE_GL is set (the gate), so a test environment
/// that lost its compositor or its GL does not show these tests green.
#[cfg(test)]
fn skip_without(what: &str) {
    assert!(
        std::env::var_os("RI_REQUIRE_GL").is_none(),
        "no {what} (RI_REQUIRE_GL is set)"
    );
    elog::eprintln!("test skipped: no {what}");
}

#[cfg(test)]
#[path = "gpu_precision_tests.rs"]
mod precision_tests;

#[cfg(test)]
mod headless_tests {
    use super::*;

    fn draw(g: &mut Gpu, surf: &mut Surface) -> Canvas {
        let mut fr = Frame::new(64, 32);
        let red = Rc::new(Canvas::new(4, 4, [255, 0, 0, 255]));
        fr.image(
            red,
            [8.0, 8.0, 16.0, 16.0],
            [0.0, 0.0, 1.0, 1.0],
            1.0,
            false,
        );
        let half = Rc::new(Canvas::new(4, 4, [0, 0, 128, 128]));
        fr.image(
            half,
            [40.0, 4.0, 20.0, 24.0],
            [0.0, 0.0, 1.0, 1.0],
            1.0,
            false,
        );
        surf.reswap();
        g.render(surf, &mut fr, || {});
        g.read_surface(surf)
    }

    /// A picture under an opacity comes out of the graphics card as it does on the processor,
    /// for every byte: the texel is the byte times the float 1/255, and the half of
    /// `texel * opacity` goes to even. A sampler left at GLSL ES's default lowp gave the texel
    /// as a 16-bit float on llvmpipe, a step lower at 31 of the 256 bytes under 0.5.
    #[test]
    fn a_translucent_picture_rounds_on_the_card_as_on_the_processor() {
        let (mut g, _) = Gpu::headless("surfaceless").expect("surfaceless EGL (Mesa)");
        let mut surf = g.pbuffer(256, 4).expect("pbuffer");
        let mut t = Canvas::new(256, 1, [0; 4]);
        for (k, p) in t.px.iter_mut().enumerate() {
            *p = [k as u8, k as u8, k as u8, 255];
        }
        let t = Rc::new(t);
        for op in [0.5f32, 0.3, 0.7, 0.25, 0.999] {
            let mut fr = Frame::new(256, 4);
            fr.image(
                t.clone(),
                [0.0, 0.0, 256.0, 1.0],
                [0.0, 0.0, 1.0, 1.0],
                op,
                false,
            );
            surf.reswap();
            g.render(&mut surf, &mut fr, || {});
            let card = g.read_surface(&surf);
            let mut cpu = Canvas::new(256, 4, [0; 4]);
            let fill = qsg::image::FillMode::Stretch;
            qsg::image::draw(&mut cpu, &t, 0.0, 0.0, 256.0, 1.0, fill, false, op);
            let want = |c: usize| ((c as f32 * (1.0 / 255.0) * op) * 255.0).round_ties_even();
            for c in 0..256 {
                assert_eq!(
                    card.px[c][0],
                    want(c) as u8,
                    "byte {c} under {op}, the card"
                );
                assert_eq!(cpu.px[c], card.px[c], "byte {c} under {op}, the processor");
            }
        }
    }

    /// llvmpipe without any window system (tools/testenv has Mesa): a pbuffer window reads back
    /// top-down and premultiplied, and the surfaceless and software-device displays agree.
    #[test]
    fn llvmpipe_renders_to_pbuffer() {
        let (mut a, what) = Gpu::headless("surfaceless").expect("surfaceless EGL (Mesa)");
        elog::eprintln!("headless {what}: {}", a.renderer);
        let mut sa = a.pbuffer(64, 32).expect("pbuffer");
        let shot = draw(&mut a, &mut sa);
        assert_eq!((shot.w, shot.h), (64, 32));
        let at = |x: usize, y: usize| shot.px[y * shot.w + x];
        assert_eq!(at(10, 9), [255, 0, 0, 255]);
        assert_eq!(at(23, 23), [255, 0, 0, 255]);
        assert_eq!(at(7, 9), [0; 4]);
        assert_eq!(at(10, 24), [0; 4]);
        assert_eq!(at(50, 5), [0, 0, 128, 128]);
        assert_eq!(at(50, 2), [0; 4]);
        assert_eq!(draw(&mut a, &mut sa).px, shot.px, "a second frame differs");
        let (mut b, what) = Gpu::headless("software").expect("Mesa software EGL device");
        elog::eprintln!("headless {what}: {}", b.renderer);
        let mut sb = b.pbuffer(64, 32).expect("pbuffer");
        assert_eq!(draw(&mut b, &mut sb).px, shot.px);
    }

    /// A layer wider than GL_MAX_TEXTURE_SIZE (e.g. a wallpaper layer at dpr 2) renders
    /// into a target clamped to the limit and is sampled scaled up, not as the empty texture of
    /// an incomplete framebuffer, and leaves no GL error behind.
    #[test]
    fn layer_past_the_texture_size_limit_draws_scaled_down() {
        let Ok((mut g, _)) = Gpu::headless("surfaceless") else {
            assert!(
                std::env::var_os("RI_REQUIRE_GL").is_none(),
                "no surfaceless EGL"
            );
            return;
        };
        let mut surf = g.pbuffer(64, 32).expect("pbuffer");
        let w = g.max_tex + 1;
        let mut src = Frame::new(w, 2);
        for (c, x) in [([0, 255, 0, 255], 0.0), ([0, 0, 255, 255], (w / 2) as f64)] {
            src.image(
                Rc::new(Canvas::new(4, 2, c)),
                [x, 0.0, (w - w / 2) as f64, 2.0],
                [0.0, 0.0, 1.0, 1.0],
                1.0,
                false,
            );
        }
        let mut fr = Frame::new(64, 32);
        fr.layer(src, [64.0 / w as f64, 0.0, 0.0, 16.0, 0.0, 0.0], 1.0);
        surf.reswap();
        g.render(&mut surf, &mut fr, || {});
        let shot = g.read_surface(&surf);
        let at = |x: usize, y: usize| shot.px[y * shot.w + x];
        assert_eq!(at(8, 16), [0, 255, 0, 255]);
        assert_eq!(at(56, 16), [0, 0, 255, 255]);
        // SAFETY: `read_surface` left the context current.
        assert_eq!(unsafe { g.gl.get_error() }, glow::NO_ERROR);
    }

    /// A blit of a canvas whose pixel buffer is shorter than its size (the fields are public)
    /// uploads nothing: GL would read `w * h` pixels past the end of the buffer.
    #[test]
    fn blit_of_a_canvas_short_of_its_size_draws_nothing() {
        let Ok((mut g, _)) = Gpu::headless("surfaceless") else {
            assert!(
                std::env::var_os("RI_REQUIRE_GL").is_none(),
                "no surfaceless EGL"
            );
            return;
        };
        let mut surf = g.pbuffer(64, 32).expect("pbuffer");
        surf.reswap();
        g.render(&mut surf, &mut Frame::new(64, 32), || {});
        let empty = g.read_surface(&surf);
        let mut short = Canvas::new(64, 32, [255, 0, 0, 255]);
        short.px.truncate(1);
        let mut fr = Frame::new(64, 32);
        fr.blit(short, 0, 0);
        surf.reswap();
        g.render(&mut surf, &mut fr, || {});
        let shot = g.read_surface(&surf);
        assert!(shot.px == empty.px, "drew {:?}", &shot.px[..4]);
        // SAFETY: `read_surface` left the context current.
        assert_eq!(unsafe { g.gl.get_error() }, glow::NO_ERROR);
    }

    /// A frame whose swap failed (EGL_BAD_SURFACE, EGL_CONTEXT_LOST) was never shown: the
    /// next frame is drawn and damaged whole, not as a repair of buffers that lack it.
    #[test]
    fn failed_swap_leaves_no_damage_history() {
        let Ok((mut g, _)) = Gpu::headless("surfaceless") else {
            assert!(
                std::env::var_os("RI_REQUIRE_GL").is_none(),
                "no surfaceless EGL"
            );
            return;
        };
        let px = |c: [u8; 4]| Rc::new(Canvas::new(4, 4, c));
        let (bg, red, blue) = (
            px([0, 0, 0, 255]),
            px([255, 0, 0, 255]),
            px([0, 0, 255, 255]),
        );
        let frame = |a: &Rc<Canvas>| {
            let mut fr = Frame::new(64, 32);
            fr.image(
                bg.clone(),
                [0.0, 0.0, 64.0, 32.0],
                [0.0, 0.0, 1.0, 1.0],
                1.0,
                false,
            );
            fr.image(
                a.clone(),
                [4.0, 4.0, 8.0, 8.0],
                [0.0, 0.0, 1.0, 1.0],
                1.0,
                false,
            );
            fr
        };
        let mut surf = g.pbuffer(64, 32).expect("pbuffer");
        g.age_override = Some(1);
        assert!(g.render(&mut surf, &mut frame(&red), || {}));
        g.swap_fails = true;
        assert!(!g.render(&mut surf, &mut frame(&blue), || {}));
        assert!(surf.prev.is_empty() && surf.hist.is_empty());
        g.swap_fails = false;
        // The same frame again: it used to count as shown, so nothing was swapped.
        assert!(g.render(&mut surf, &mut frame(&blue), || {}));
        assert_eq!(g.last_damage, [0, 0, 64, 32], "drawn whole");
        assert_eq!(surf.hist.len(), 1);
        g.render(&mut surf, &mut frame(&red), || {});
        assert_eq!(g.last_damage, [3, 3, 13, 13], "then repaired as usual");
    }

    /// A frame whose eglMakeCurrent or swap failed says why: an unusable surface, on which the
    /// frame is drawn again, or a lost context (EGL_CONTEXT_LOST, or frame after frame failing
    /// with nothing shown in between), which the shell replaces. `render` used to return
    /// false for both, as it does for an unchanged frame it skips, and logged nothing.
    #[test]
    fn failed_frame_says_whether_the_context_is_lost() {
        let Ok((mut g, _)) = Gpu::headless("surfaceless") else {
            assert!(
                std::env::var_os("RI_REQUIRE_GL").is_none(),
                "no surfaceless EGL"
            );
            return;
        };
        let mut surf = g.pbuffer(64, 32).expect("pbuffer");
        let mut n = 0u8;
        let mut frame = || {
            n += 1;
            let mut fr = Frame::new(64, 32);
            fr.blit(Canvas::new(4, 4, [n, 0, 0, 255]), 0, 0);
            fr
        };
        assert!(g.render(&mut surf, &mut frame(), || {}));
        assert!(g.shown());
        assert_eq!(g.take_failure(), None);
        // An unchanged frame that is skipped did not fail.
        g.may_skip = true;
        let mut same = frame();
        assert!(g.render(&mut surf, &mut same, || {}));
        g.may_skip = true;
        assert!(!g.render(&mut surf, &mut same, || {}));
        assert_eq!(g.take_failure(), None);
        g.swap_fails = true;
        assert!(!g.render(&mut surf, &mut frame(), || {}));
        assert_eq!(g.take_failure(), Some(Failure::Surface));
        assert_eq!(g.take_failure(), None, "asked once");
        g.swap_fails = false;
        g.bind_fails = Some(egl::Error::BadNativeWindow);
        assert!(!g.render(&mut surf, &mut frame(), || {}));
        assert_eq!(g.take_failure(), Some(Failure::Surface));
        // A frame shown in between: the count starts again.
        g.bind_fails = None;
        assert!(g.render(&mut surf, &mut frame(), || {}));
        g.bind_fails = Some(egl::Error::BadSurface);
        for _ in 1..SURFACE_FAILURES {
            assert!(!g.render(&mut surf, &mut frame(), || {}));
            assert_eq!(g.take_failure(), Some(Failure::Surface));
        }
        assert!(!g.render(&mut surf, &mut frame(), || {}));
        assert_eq!(g.take_failure(), Some(Failure::Context), "given up");
        g.bind_fails = Some(egl::Error::ContextLost);
        assert!(!g.render(&mut surf, &mut frame(), || {}));
        assert_eq!(g.take_failure(), Some(Failure::Context));
    }

    /// What the log says about a failed frame: the call, the EGL error and what happens next.
    #[test]
    fn failed_frame_log_lines() {
        assert_eq!(
            failure("eglMakeCurrent", egl::Error::ContextLost, 1),
            (
                "gpu: eglMakeCurrent failed: 0x300e (ContextLost): the GPU context is lost"
                    .to_owned(),
                Failure::Context
            )
        );
        assert_eq!(
            failure("eglSwapBuffers", egl::Error::BadSurface, 2),
            (
                "gpu: eglSwapBuffers failed: 0x300d (BadSurface): the frame is drawn again on a \
                 new surface"
                    .to_owned(),
                Failure::Surface
            )
        );
        assert_eq!(
            failure("eglSwapBuffers", egl::Error::BadSurface, 3),
            (
                "gpu: eglSwapBuffers failed: 0x300d (BadSurface): 3 frames in a row failed, the \
                 GPU context is given up"
                    .to_owned(),
                Failure::Context
            )
        );
    }

    /// After its context is lost a `Gpu` is retired and a new one draws the same frame: the
    /// same pixels, from the same shared images, none of which counts as uploaded any more
    /// (an image that had traded its pixels for the lost texture would draw nothing).
    #[test]
    fn new_context_draws_the_frame_of_a_lost_one() {
        let Ok((mut g, _)) = Gpu::headless("surfaceless") else {
            assert!(
                std::env::var_os("RI_REQUIRE_GL").is_none(),
                "no surfaceless EGL"
            );
            return;
        };
        let img = Rc::new(Canvas::new(8, 8, [0, 200, 0, 255]));
        let frame = || {
            let mut fr = Frame::new(64, 32);
            fr.image(
                img.clone(),
                [4.0, 4.0, 40.0, 20.0],
                [0.0, 0.0, 1.0, 1.0],
                1.0,
                false,
            );
            fr
        };
        let mut surf = g.pbuffer(64, 32).expect("pbuffer");
        assert!(g.render(&mut surf, &mut frame(), || {}));
        let shown = g.read_surface(&surf);
        assert_eq!(shown.px[10 * 64 + 10], [0, 200, 0, 255]);
        assert!(resident(&img).is_some(), "the image is uploaded");
        g.bind_fails = Some(egl::Error::ContextLost);
        let mut next = frame();
        next.blit(Canvas::new(2, 2, [9, 9, 9, 255]), 60, 0);
        assert!(!g.render(&mut surf, &mut next, || {}));
        assert_eq!(g.take_failure(), Some(Failure::Context));
        drop(surf);
        g.retire();
        assert!(
            resident(&img).is_none(),
            "its texture went with the context"
        );
        let (mut g, _) = Gpu::headless("surfaceless").expect("a new context");
        let mut surf = g.pbuffer(64, 32).expect("pbuffer");
        assert!(g.render(&mut surf, &mut frame(), || {}));
        assert_eq!(g.take_failure(), None);
        assert!(g.read_surface(&surf).px == shown.px);
    }

    /// An image wider than GL_MAX_TEXTURE_SIZE (a panorama wallpaper without `sourceSize`)
    /// draws its pixels, scaled down to the limit like QSGPlainTexture, not the black of an
    /// incomplete texture, and leaves no GL error behind.
    #[test]
    fn image_past_the_texture_size_limit_draws_scaled_down() {
        let Ok((mut g, _)) = Gpu::headless("surfaceless") else {
            assert!(
                std::env::var_os("RI_REQUIRE_GL").is_none(),
                "no surfaceless EGL"
            );
            return;
        };
        let mut surf = g.pbuffer(64, 32).expect("pbuffer");
        let w = g.max_tex + 1;
        let mut img = Canvas::new(w, 2, [0, 255, 0, 255]);
        for y in 0..2 {
            for x in w / 2..w {
                img.px[y * w + x] = [0, 0, 255, 255];
            }
        }
        let mut fr = Frame::new(64, 32);
        fr.image(
            Rc::new(img),
            [0.0, 0.0, 64.0, 32.0],
            [0.0, 0.0, 1.0, 1.0],
            1.0,
            true,
        );
        surf.reswap();
        g.render(&mut surf, &mut fr, || {});
        let shot = g.read_surface(&surf);
        let at = |x: usize, y: usize| shot.px[y * shot.w + x];
        assert_eq!(at(8, 16), [0, 255, 0, 255]);
        assert_eq!(at(56, 16), [0, 0, 255, 255]);
        // SAFETY: `read_surface` left the context current.
        assert_eq!(unsafe { g.gl.get_error() }, glow::NO_ERROR);
    }

    /// Layers leave the cache oldest first, with the byte total kept up to date.
    #[test]
    fn layer_cache_evicts_least_recently_drawn_first() {
        struct L(u64, i64);
        impl Cached for L {
            fn bytes(&self) -> i64 {
                self.1
            }
        }
        let mut c = LayerCache::new();
        for k in 0..4u64 {
            c.insert(k, L(k, 10));
        }
        assert_eq!(c.bytes, 40);
        c.get_mut(&0).expect("cached").0 = 4;
        assert!(c.insert(2, L(5, 30)).is_some(), "replaced entry returned");
        assert_eq!(c.bytes, 60);
        let order: Vec<u64> = c.iter().rev().map(|(k, _)| *k).collect();
        assert_eq!(order, [1, 3, 0, 2], "cold end first");
        assert!(c.iter().rev().map(|(_, e)| e.0).is_sorted());
        assert!(c.pop_oldest_if(|e, _| e.0 > 1).is_none(), "oldest kept");
        let e = c.pop_oldest_if(|_, total| total > 50).expect("over budget");
        assert_eq!((e.0, c.bytes), (1, 50));
        assert!(c.remove(&0).is_some() && !c.contains_key(&0));
        assert_eq!((c.len(), c.bytes), (2, 40));
        c.clear();
        assert!(c.is_empty() && c.bytes == 0);
    }

    /// A full atlas frees the areas of bitmaps nothing else holds and reuses them, without
    /// starting over; bitmaps still held keep their place.
    #[test]
    fn atlas_reuses_areas_of_dropped_bitmaps() {
        let (mut g, _) = Gpu::headless("surfaceless").expect("surfaceless EGL (Mesa)");
        g.dfatlas.max = ATLAS_MIN;
        // SAFETY: `g` made its context current when created or rendering, and this test binds no other.
        unsafe { Gpu::reset_atlas(&g.gl, &mut g.dfatlas, ATLAS_MIN) };
        let tile = |v: u8| -> Rc<Canvas> { Rc::new(Canvas::new(100, 100, [v; 4])) };
        let px = vec![7u8; 100 * 100 * 4];
        // SAFETY: `g` made its context current when created or rendering, and this test binds no other.
        let place = |g: &mut Gpu, c: &Rc<Canvas>| unsafe {
            let b: Rc<dyn std::any::Any> = c.clone();
            g.dfatlas.place(&g.gl, c.key.get(), &b, 100, 100, &px)
        };
        let held: Vec<_> = (0..4).map(tile).collect();
        let at: Vec<_> = held
            .iter()
            .map(|b| place(&mut g, b).expect("fits"))
            .collect();
        let extra = tile(9);
        assert_eq!(place(&mut g, &extra), None, "four 101px areas fill 256px");
        assert_eq!(g.dfatlas.slots.len(), 4, "held bitmaps freed");
        let mut held = held;
        drop(held.remove(2));
        let _ = take_gl_calls();
        assert_eq!(place(&mut g, &extra), Some(at[2]), "freed area not reused");
        assert_eq!(take_gl_calls()[0], 0, "atlas reallocated");
        for (b, p) in held.iter().zip([at[0], at[1], at[3]]) {
            assert_eq!(place(&mut g, b), Some(p), "held bitmap moved");
        }
    }

    /// A frame whose text is new to the atlas uploads every glyph bitmap before its first
    /// draw, in the window and in its layers: an upload between two draws that sample the atlas
    /// makes the driver finish the first (llvmpipe waits for its threads once per text item).
    #[test]
    #[cfg_attr(
        not(feature = "interp"),
        ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
    )]
    fn new_glyphs_are_uploaded_before_the_first_draw() {
        let (mut g, _) = Gpu::headless("surfaceless").expect("surfaceless EGL (Mesa)");
        let mut rt = qml::runtime::Runtime::new(None);
        let win = rt
            .load_str(
                r##"import QtQuick
import Rustshell
import Qt5Compat.GraphicalEffects
FloatingWindow {
    width: 400; height: 200
    color: "white"
    Text { x: 2; y: 0; text: "ABCDEFGH"; font.pixelSize: 22; font.family: "sans-serif"; color: "black" }
    Rectangle { x: 2; y: 40; width: 100; height: 4; color: "red" }
    Text { x: 2; y: 50; text: "ijklmnop"; font.pixelSize: 22; font.family: "sans-serif"; color: "black" }
    Text { x: 2; y: 90; text: "0123"; font.pixelSize: 22; font.family: "sans-serif"; renderType: Text.QtRendering; color: "black" }
    Rectangle { x: 2; y: 125; width: 100; height: 4; color: "red" }
    Text { x: 2; y: 130; text: "4567"; font.pixelSize: 22; font.family: "sans-serif"; renderType: Text.QtRendering; color: "black" }
    Item {
        x: 200; y: 0; width: 200; height: 100
        layer.enabled: true
        layer.effect: ColorOverlay { color: "#0000ff" }
        Text { text: "QRSTUV"; font.pixelSize: 22; font.family: "sans-serif"; color: "black" }
        Rectangle { y: 40; width: 100; height: 4; color: "red" }
        Text { y: 50; text: "wxyz"; font.pixelSize: 22; font.family: "sans-serif"; color: "black" }
    }
}"##,
            )
            .unwrap();
        let mut fonts = crate::scene::Fonts::default();
        if fonts.get("sans-serif", 16, 400).is_none() {
            testenv::unavailable("no font for the family sans-serif");
            return;
        }
        crate::scene::layout_window(&mut rt, &mut fonts, win, [400.0, 200.0]);
        let mut fr = Frame::new(400, 200);
        crate::scene::paint_window(
            &mut rt,
            &mut fonts,
            win,
            &mut crate::scene::Target::Gpu(&mut fr),
        );
        let texts = |f: &Frame| {
            let n = |c: &Cmd| matches!(c, Cmd::Glyphs(..) | Cmd::Df(..)) as usize;
            f.cmds.iter().map(n).sum::<usize>()
        };
        let layer = fr
            .cmds
            .iter()
            .find_map(|c| match c {
                Cmd::Tint { src, .. } => Some(src.clone()),
                _ => None,
            })
            .expect("no layer in the frame");
        assert_eq!((texts(&fr), texts(&layer)), (4, 2), "text commands");
        let mut s = g.pbuffer(400, 200).expect("pbuffer");
        s.reswap();
        let _ = take_gl_calls();
        g.render(&mut s, &mut fr, || {});
        let c = take_gl_calls();
        assert!(c[1] >= 34, "{} uploads for 34 glyphs", c[1]);
        assert_eq!(c[GL_MIDDRAW], 0, "atlas uploads between the frame's draws");
    }

    /// The tiles of a distance-field text command draw from one atlas texture with the same
    /// pixels as one texture per tile.
    #[test]
    fn df_tiles_batch_through_the_atlas() {
        let (mut g, _) = Gpu::headless("surfaceless").expect("surfaceless EGL (Mesa)");
        let tiles: Vec<qsg::dftext::DfTile> = (0..12)
            .map(|k| {
                let (w, h) = (5 + k % 4, 7 + k % 3);
                let mut c = Canvas::new(w, h, [0; 4]);
                for (i, p) in c.px.iter_mut().enumerate() {
                    let a = ((i * 37 + k * 11) % 256) as u8;
                    *p = [a / 3, a / 2, a, a];
                }
                (Rc::new(c), 3 + 10 * k as i64 % 60, 2 + 9 * (k as i64 / 6))
            })
            .collect();
        let mut fr = Frame::new(64, 32);
        fr.df_tiles(tiles.clone(), Some([64, 128, 191]));
        let shot = |g: &mut Gpu| {
            let mut s = g.pbuffer(64, 32).expect("pbuffer");
            s.reswap();
            g.render(&mut s, &mut fr.clone(), || {});
            g.read_surface(&s).px
        };
        let batched = shot(&mut g);
        assert_eq!(g.dfatlas.slots.len(), tiles.len(), "tiles not in the atlas");
        assert!(
            tiles
                .iter()
                .all(|(t, ..)| !g.images.contains_key(&t.key.get())),
            "a tile got its own texture"
        );
        let (mut alone, _) = Gpu::headless("surfaceless").expect("second context");
        alone.dfatlas.max = 0;
        assert_eq!(shot(&mut alone), batched, "atlas draws other pixels");
        assert_eq!(alone.dfatlas.size, 0);
        assert!(batched.iter().any(|p| p[0] != 0), "nothing drawn");
    }

    #[test]
    fn resident_images_draw_their_texture_without_cpu_pixels() {
        let (mut g, _) = Gpu::headless("surfaceless").expect("surfaceless EGL (Mesa)");
        let mut s = g.pbuffer(32, 32).expect("pbuffer");
        let shot = |g: &mut Gpu, s: &mut Surface, t: &Rc<Canvas>| {
            let mut fr = Frame::new(32, 32);
            fr.image(
                t.clone(),
                [4.0, 4.0, 24.0, 24.0],
                [0.0, 0.0, 1.0, 1.0],
                1.0,
                true,
            );
            s.reswap();
            g.render(s, &mut fr, || {});
            g.read_surface(s)
        };
        let mut img = Canvas::new(8, 8, [0; 4]);
        for (k, p) in img.px.iter_mut().enumerate() {
            *p = [(k * 4) as u8, 255 - (k * 4) as u8, 0, 255];
        }
        let img = Rc::new(img);
        assert!(resident(&img).is_none(), "no texture yet");
        let want = shot(&mut g, &mut s, &img);
        let p = resident(&img).expect("uploaded");
        assert!(p.px.is_empty() && (p.w, p.h) == (8, 8));
        let pixels = Rc::downgrade(&img);
        drop(img);
        assert_eq!(
            shot(&mut g, &mut s, &p).px,
            want.px,
            "stand-in draws the same texture"
        );
        assert!(pixels.upgrade().is_none(), "CPU pixels freed");
        assert_eq!(shot(&mut g, &mut s, &p).px, want.px);
        let key = p.key.get();
        assert!(g.images.contains_key(&key));
        drop(p);
        let other = Rc::new(Canvas::new(2, 2, [9, 9, 9, 255]));
        shot(&mut g, &mut s, &other);
        shot(&mut g, &mut s, &other);
        assert!(
            !g.images.contains_key(&key),
            "texture deleted with its stand-in"
        );
        let plain = Rc::new(Canvas::new(8, 8, [1, 2, 3, 0]).without_alpha());
        shot(&mut g, &mut s, &plain);
        let p = resident(&plain).expect("uploaded");
        assert!(
            !p.key.has_alpha(),
            "no alpha channel in the stand-in either"
        );
    }

    /// Glyph atlases take VRAM only for what is drawn: none until the first glyph, then a
    /// texture that doubles as it fills, with the same pixels as a full-size atlas.
    #[test]
    #[cfg_attr(
        not(feature = "interp"),
        ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
    )]
    fn glyph_atlas_grows_on_demand() {
        let Ok((mut g, _)) = Gpu::headless("surfaceless") else {
            assert!(
                std::env::var_os("RI_REQUIRE_GL").is_none(),
                "no surfaceless EGL/GLES2 context (RI_REQUIRE_GL is set)"
            );
            return;
        };
        assert_eq!(
            (g.atlas.size, g.catlas.size),
            (0, 0),
            "atlas allocated before any text"
        );
        let mut rt = qml::runtime::Runtime::new(None);
        let win = rt
            .load_str(
                r#"import QtQuick
import Rustshell
FloatingWindow {
    width: 1000; height: 240
    color: "white"
    Text { x: 2; y: 0; text: "ABCDEFGHIJKLMNOPQRSTUVWXYZ"; font.pixelSize: 64; font.family: "sans-serif"; color: "black"; renderType: Text.NativeRendering }
    Text { x: 2; y: 80; text: "abcdefghijklmnopqrstuvwxyz0123456789"; font.pixelSize: 22; font.family: "sans-serif"; color: "black"; renderType: Text.NativeRendering }
    Text { x: 2; y: 120; text: "The quick brown fox"; font.pixelSize: 13; font.family: "sans-serif"; color: "black"; renderType: Text.NativeRendering }
    Text { x: 2; y: 150; text: "abcdefghijklmnopqrstuvwxyz"; font.pixelSize: 64; font.family: "sans-serif"; color: "black"; renderType: Text.NativeRendering }
}"#,
            )
            .unwrap();
        let mut fonts = crate::scene::Fonts::default();
        if fonts.get("sans-serif", 16, 400).is_none() {
            testenv::unavailable("no font for the family sans-serif");
            return;
        }
        crate::scene::layout_window(&mut rt, &mut fonts, win, [1000.0, 240.0]);
        let mut fr = Frame::new(1000, 240);
        crate::scene::paint_window(
            &mut rt,
            &mut fonts,
            win,
            &mut crate::scene::Target::Gpu(&mut fr),
        );
        let shot = |g: &mut Gpu| {
            let mut s = g.pbuffer(1000, 240).expect("pbuffer");
            s.reswap();
            g.render(&mut s, &mut fr.clone(), || {});
            g.read_surface(&s).px
        };
        let lazy = shot(&mut g);
        assert!(
            g.atlas.size > ATLAS_MIN,
            "atlas never grew: {}",
            g.atlas.size
        );
        assert!(
            g.atlas.size < ATLAS,
            "atlas {} for three lines",
            g.atlas.size
        );
        assert_eq!(
            g.catlas.size, 0,
            "colour atlas allocated without colour glyphs"
        );
        let (mut full, _) = Gpu::headless("surfaceless").expect("second context");
        // SAFETY: `Gpu::headless` just made `full`'s context current (`on_display`), after
        // `g`'s, and nothing ran since.
        unsafe { full.atlas_reset(false, ATLAS) };
        assert_eq!(shot(&mut full), lazy, "grown atlas draws other pixels");
        assert!(lazy.iter().any(|p| p[0] < 128), "no text drawn");
    }

    fn gradient() -> Rc<Canvas> {
        let mut img = Canvas::new(8, 8, [0; 4]);
        for (k, p) in img.px.iter_mut().enumerate() {
            *p = [(k * 4) as u8, 255 - (k * 4) as u8, (k * 7 % 256) as u8, 255];
        }
        Rc::new(img)
    }

    /// Plain and smooth images, a layer, an opacity mask and a blurred effect: every program
    /// and both texture filters.
    fn mixed(red: &Rc<Canvas>, grad: &Rc<Canvas>) -> Frame {
        let uv = [0.0, 0.0, 1.0, 1.0];
        let mut fr = Frame::new(96, 64);
        fr.image(red.clone(), [2.0, 2.0, 20.0, 20.0], uv, 1.0, false);
        fr.image(grad.clone(), [24.0, 2.0, 30.0, 20.0], uv, 0.5, true);
        fr.image(grad.clone(), [56.0, 2.0, 30.0, 20.0], uv, 1.0, false);
        let mut inner = Frame::new(32, 32);
        inner.image(grad.clone(), [0.0, 0.0, 32.0, 32.0], uv, 1.0, true);
        fr.layer(inner, [1.0, 0.0, 0.0, 1.0, 2.0, 26.0], 0.8);
        let mut src = Frame::new(24, 24);
        src.image(red.clone(), [0.0, 0.0, 24.0, 24.0], uv, 1.0, false);
        let mut mask = Frame::new(24, 24);
        mask.image(grad.clone(), [4.0, 4.0, 16.0, 16.0], uv, 1.0, true);
        fr.masked(src, mask, [40.0, 28.0, 24.0, 24.0], false, 1.0, 7);
        let mut esrc = Frame::new(24, 24);
        esrc.image(grad.clone(), [0.0, 0.0, 24.0, 24.0], uv, 1.0, true);
        let blur = Blur {
            sizes: vec![(12.0, 12.0), (6.0, 6.0)],
            dpr: 1.0,
            multiplier: 1.0,
            weights: [0.2, 0.5, 0.3, 0.0, 0.0, 0.0],
        };
        let u = [1.0, 1.0, 1.0, 0.0, 0.1, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 0.0];
        fr.effect(
            esrc,
            None,
            true,
            [68.0, 28.0, 24.0, 24.0],
            u,
            0.9,
            Some(blur),
            None,
        );
        fr
    }

    /// The GL state cache changes which calls reach the driver, never the pixels: a mixed
    /// scene renders byte-identically with the cache on and off.
    #[test]
    fn gl_state_cache_draws_identical_pixels() {
        let (mut g, _) = Gpu::headless("surfaceless").expect("surfaceless EGL (Mesa)");
        let mut s = g.pbuffer(96, 64).expect("pbuffer");
        let (red, grad) = (Rc::new(Canvas::new(4, 4, [255, 0, 0, 255])), gradient());
        let shot = |g: &mut Gpu, s: &mut Surface| {
            let mut fr = mixed(&red, &grad);
            s.reswap();
            g.render(s, &mut fr, || {});
            g.read_surface(s)
        };
        g.gl_cache = false;
        let plain = shot(&mut g, &mut s);
        assert_eq!(shot(&mut g, &mut s).px, plain.px, "uncached frames differ");
        let _ = take_gl_calls();
        shot(&mut g, &mut s);
        let off = take_gl_calls();
        g.gl_cache = true;
        assert_eq!(
            shot(&mut g, &mut s).px,
            plain.px,
            "first cached frame differs"
        );
        let _ = take_gl_calls();
        assert_eq!(
            shot(&mut g, &mut s).px,
            plain.px,
            "warm cached frame differs"
        );
        let on = take_gl_calls();
        assert!(
            plain.px.iter().filter(|p| p[3] != 0).count() > 1000,
            "scene drew nothing"
        );
        for (k, name) in [
            (GL_PROGRAM, "use_program"),
            (GL_UNIFORM, "uniform"),
            (GL_TEXPARAM, "tex_parameter"),
            (GL_ATTRIB, "vertex attrib"),
        ] {
            assert!(
                on[k] < off[k],
                "{name}: {} cached vs {} uncached",
                on[k],
                off[k]
            );
        }
    }

    /// A frame of N textured quads sends the program once, its uniforms once and no texture
    /// filter the driver already has.
    #[test]
    fn gl_state_cache_skips_repeated_calls_for_quads() {
        const N: usize = 64;
        let (mut g, _) = Gpu::headless("surfaceless").expect("surfaceless EGL (Mesa)");
        let mut s = g.pbuffer(128, 128).expect("pbuffer");
        let imgs: Vec<Rc<Canvas>> = (0..N)
            .map(|k| Rc::new(Canvas::new(2, 2, [k as u8, 0, 255 - k as u8, 255])))
            .collect();
        let shot = |g: &mut Gpu, s: &mut Surface| {
            let mut fr = Frame::new(128, 128);
            for (k, img) in imgs.iter().enumerate() {
                let (x, y) = ((k % 8) as f64 * 16.0, (k / 8) as f64 * 16.0);
                fr.image(
                    img.clone(),
                    [x, y, 12.0, 12.0],
                    [0.0, 0.0, 1.0, 1.0],
                    1.0,
                    k % 2 == 0,
                );
            }
            s.reswap();
            g.render(s, &mut fr, || {});
            g.read_surface(s)
        };
        g.gl_cache = false;
        let plain = shot(&mut g, &mut s);
        let _ = take_gl_calls();
        shot(&mut g, &mut s);
        let off = take_gl_calls();
        g.gl_cache = true;
        shot(&mut g, &mut s);
        let _ = take_gl_calls();
        assert_eq!(shot(&mut g, &mut s).px, plain.px);
        let on = take_gl_calls();
        elog::eprintln!("{N} quads: uncached {off:?}, cached {on:?}");
        assert!(off[GL_PROGRAM] >= N as u64 && off[GL_UNIFORM] >= 6 * N as u64);
        assert!(off[GL_TEXPARAM] >= 2 * N as u64 && off[GL_ATTRIB] >= 4 * N as u64);
        assert_eq!(on[GL_PROGRAM], 1);
        assert!(on[GL_UNIFORM] <= 6, "uniforms: {}", on[GL_UNIFORM]);
        assert_eq!(on[GL_TEXPARAM], 0);
        assert!(
            on[GL_ATTRIB] <= 2 + 2 * N as u64,
            "attribs: {}",
            on[GL_ATTRIB]
        );
    }

    fn solid(rgba: [u8; 4]) -> Frame {
        let mut fr = Frame::new(64, 32);
        let c = Rc::new(Canvas::new(4, 4, rgba));
        fr.image(c, [0.0, 0.0, 64.0, 32.0], [0.0, 0.0, 1.0, 1.0], 1.0, false);
        fr
    }

    /// An unchanged frame is no new buffer: nothing is drawn or swapped (no buffer-age
    /// history entry). One that has something else to send (a veil's step) is still committed;
    /// any other sends nothing.
    #[test]
    fn an_unchanged_frame_is_not_swapped_and_is_sent_only_when_it_must_be() {
        let Ok((mut g, _)) = Gpu::headless("surfaceless") else {
            assert!(
                std::env::var_os("RI_REQUIRE_GL").is_none(),
                "no surfaceless EGL"
            );
            return;
        };
        let mut surf = g.pbuffer(64, 32).expect("pbuffer");
        let red = solid([255, 0, 0, 255]);
        assert!(g.render(&mut surf, &mut red.clone(), || {}));
        let (hist, frames) = (surf.hist.len(), g.frame_no);
        let mut asked = false;
        g.may_skip = false;
        assert!(g.render(&mut surf, &mut red.clone(), || asked = true));
        assert!(asked, "the frame callback was not requested");
        assert_eq!(
            (surf.hist.len(), g.frame_no),
            (hist, frames),
            "an unchanged frame was swapped"
        );
        g.may_skip = true;
        assert!(!g.render(&mut surf, &mut red.clone(), || panic!(
            "skipped frame committed"
        )));
        assert_eq!(g.read_surface(&surf).px[0], [255, 0, 0, 255]);
    }

    /// Two changes at opposite ends repaint and damage two rectangles, not their bounding box:
    /// the pixels between them are left as they were.
    #[test]
    fn distant_changes_repaint_only_their_rects() {
        let Ok((mut g, _)) = Gpu::headless("surfaceless") else {
            assert!(
                std::env::var_os("RI_REQUIRE_GL").is_none(),
                "no surfaceless EGL"
            );
            return;
        };
        // Images hash by address: the same Rc is the same content.
        let px = |c: [u8; 4]| Rc::new(Canvas::new(4, 4, c));
        let bg = px([0, 0, 255, 255]);
        let frame = |a: &Rc<Canvas>, b: &Rc<Canvas>| {
            let mut fr = Frame::new(256, 32);
            fr.image(
                bg.clone(),
                [0.0, 0.0, 256.0, 32.0],
                [0.0, 0.0, 1.0, 1.0],
                1.0,
                false,
            );
            for (c, x) in [(a, 2.0), (b, 246.0)] {
                fr.image(
                    c.clone(),
                    [x, 2.0, 8.0, 8.0],
                    [0.0, 0.0, 1.0, 1.0],
                    1.0,
                    false,
                );
            }
            fr
        };
        let (red, yellow, cyan) = (
            px([255, 0, 0, 255]),
            px([255, 255, 0, 255]),
            px([0, 255, 255, 255]),
        );
        let mut surf = g.pbuffer(256, 32).expect("pbuffer");
        g.age_override = Some(1);
        g.render(&mut surf, &mut frame(&red, &red), || {});
        // Paint the middle green behind the renderer's back: a repaint there would undo it.
        // SAFETY: `g` made its context current when created or rendering, and this test binds no other.
        unsafe { g.gl.enable(glow::SCISSOR_TEST) };
        // SAFETY: `g` made its context current when created or rendering, and this test binds no other.
        unsafe { g.gl.scissor(100, 0, 56, 32) };
        // SAFETY: `g` made its context current when created or rendering, and this test binds no other.
        unsafe { g.gl.clear_color(0.0, 1.0, 0.0, 1.0) };
        // SAFETY: `g` made its context current when created or rendering, and this test binds no other.
        unsafe { g.gl.clear(glow::COLOR_BUFFER_BIT) };
        // SAFETY: `g` made its context current when created or rendering, and this test binds no other.
        unsafe { g.gl.disable(glow::SCISSOR_TEST) };
        g.render(&mut surf, &mut frame(&yellow, &cyan), || {});
        // Signature boxes reach a pixel past each image (antialiased edges).
        assert_eq!(g.last_damage, [1, 1, 255, 11], "bounds of the damage");
        assert_eq!(surf.hist[0].len(), 2, "damage {:?}", surf.hist[0]);
        let shot = g.read_surface(&surf);
        let at = |x: usize, y: usize| shot.px[y * shot.w + x];
        assert_eq!(at(5, 5), [255, 255, 0, 255]);
        assert_eq!(at(250, 5), [0, 255, 255, 255]);
        assert_eq!(at(128, 5), [0, 255, 0, 255], "the middle was repainted");
        assert_eq!(at(20, 20), [0, 0, 255, 255]);
    }

    /// A damage pass counts the `Cmd::Tris` it skips, so the first batch it draws can be
    /// several past the window's buffers: each one gets a buffer of its own.
    #[test]
    fn damage_pass_draws_a_batch_past_the_window_buffers() {
        let Ok((mut g, _)) = Gpu::headless("surfaceless") else {
            assert!(
                std::env::var_os("RI_REQUIRE_GL").is_none(),
                "no surfaceless EGL"
            );
            return;
        };
        let rect = Rc::new(qsg::rect::build(&qsg::rect::RectParams {
            width: 10.0,
            height: 10.0,
            color: [255, 0, 0, 255],
            ..Default::default()
        }));
        let dot = Rc::new(Canvas::new(2, 2, [0, 255, 0, 255]));
        let mut surf = g.pbuffer(512, 32).expect("pbuffer");
        g.age_override = Some(1);
        // One batch: a single triangle buffer.
        let mut fr = Frame::new(512, 32);
        for x in [0.0, 100.0, 200.0] {
            fr.smooth(&rect, x, 10.0, 1.0);
        }
        g.render(&mut surf, &mut fr, || {});
        // The same rects split into batches by new images, and a new rect far right: the
        // damage pass there skips three batches before drawing the fourth.
        let mut fr = Frame::new(512, 32);
        for x in [0.0, 100.0, 200.0] {
            fr.smooth(&rect, x, 10.0, 1.0);
            fr.image(
                dot.clone(),
                [x + 50.0, 10.0, 2.0, 2.0],
                [0.0, 0.0, 1.0, 1.0],
                1.0,
                false,
            );
        }
        fr.smooth(&rect, 400.0, 10.0, 1.0);
        g.render(&mut surf, &mut fr, || {});
        assert!(surf.hist[0].len() > 1, "damage {:?}", surf.hist[0]);
        let shot = g.read_surface(&surf);
        let at = |x: usize, y: usize| shot.px[y * shot.w + x];
        assert_eq!(at(405, 15), [255, 0, 0, 255]);
        assert_eq!(at(105, 15), [255, 0, 0, 255]);
        assert_eq!(at(250, 10), [0, 255, 0, 255]);
    }

    /// The crossfade runs on the animation clock: a frame drawn 80 ms after the first shows
    /// 80 ms of the fade however few frames came between.
    #[test]
    fn fade_follows_the_animation_clock() {
        let Ok((mut g, _)) = Gpu::headless("surfaceless") else {
            assert!(
                std::env::var_os("RI_REQUIRE_GL").is_none(),
                "no surfaceless EGL"
            );
            return;
        };
        let mut surf = g.pbuffer(64, 32).expect("pbuffer");
        let t0 = std::time::Instant::now();
        g.clock = Some(t0);
        let (red, blue) = (solid([255, 0, 0, 255]), solid([0, 0, 255, 255]));
        g.render(&mut surf, &mut red.clone(), || {});
        g.begin_fade(&mut surf, &red, std::time::Duration::from_millis(200), None);
        g.render(&mut surf, &mut blue.clone(), || {});
        g.clock = Some(t0 + std::time::Duration::from_millis(80));
        g.render(&mut surf, &mut blue.clone(), || {});
        let elapsed = surf.fade.as_ref().expect("fading").elapsed;
        assert_eq!(elapsed, std::time::Duration::from_millis(80));
        // A clock that steps back (a driver restart) never rewinds the fade.
        g.clock = Some(t0 + std::time::Duration::from_millis(70));
        g.render(&mut surf, &mut blue.clone(), || {});
        assert_eq!(surf.fade.as_ref().expect("fading").elapsed, elapsed);
    }

    /// A closed panel's layers go a minute after their last frame, whether the shell draws
    /// a frame every few seconds since (600 of those took half an hour) or none at all.
    #[test]
    fn layers_left_undrawn_are_freed_by_the_clock() {
        let Ok((mut g, _)) = Gpu::headless("surfaceless") else {
            assert!(
                std::env::var_os("RI_REQUIRE_GL").is_none(),
                "no surfaceless EGL"
            );
            return;
        };
        let mut surf = g.pbuffer(64, 32).expect("pbuffer");
        let panel = |rgba: [u8; 4]| {
            let c = Rc::new(Canvas::new(4, 4, rgba));
            let uv = [0.0, 0.0, 1.0, 1.0];
            let mut fr = Frame::new(64, 32);
            let mut inner = Frame::new(16, 16);
            inner.image(c.clone(), [0.0, 0.0, 16.0, 16.0], uv, 1.0, false);
            fr.layer(inner, [1.0, 0.0, 0.0, 1.0, 2.0, 2.0], 0.8);
            let (mut src, mut mask) = (Frame::new(16, 16), Frame::new(16, 16));
            src.image(c.clone(), [0.0, 0.0, 16.0, 16.0], uv, 1.0, false);
            mask.image(c, [4.0, 4.0, 8.0, 8.0], uv, 1.0, false);
            fr.masked(src, mask, [30.0, 2.0, 16.0, 16.0], false, 1.0, 7);
            fr
        };
        let held = |g: &Gpu| (g.layers.len(), g.inc.len(), g.pool.len());
        let s = std::time::Duration::from_secs;
        let t0 = std::time::Instant::now();
        g.clock = Some(t0);
        g.render(&mut surf, &mut panel([255, 0, 0, 255]), || {});
        let all = held(&g);
        assert!(all.0 >= 1, "{all:?}");
        // No frame since: the host's next loop turn frees them.
        g.clock = Some(t0 + LAYER_IDLE_TIME - s(1));
        g.trim_idle();
        assert_eq!(held(&g), all, "freed early");
        g.clock = Some(t0 + LAYER_IDLE_TIME);
        g.trim_idle();
        assert_eq!(held(&g), (0, 0, 0));

        // The bar draws every few seconds: the panel's layers go at their minute, in the
        // frame drawn then, and a layer drawn in between starts its minute over.
        let t0 = t0 + s(100);
        g.clock = Some(t0);
        g.render(&mut surf, &mut panel([0, 255, 0, 255]), || {});
        assert_eq!(held(&g), all);
        let blue = panel([0, 0, 255, 255]);
        for at in [3, 6, 30, 57] {
            g.clock = Some(t0 + s(at));
            let mut fr = if at == 30 {
                blue.clone()
            } else {
                solid([9, 9, 9, 255])
            };
            g.render(&mut surf, &mut fr, || {});
        }
        assert_eq!(held(&g).0, 2 * all.0, "{:?}", held(&g));
        g.clock = Some(t0 + s(60));
        g.render(&mut surf, &mut solid([8, 8, 8, 255]), || {});
        assert_eq!(
            (held(&g).0, held(&g).1),
            (all.0, all.1),
            "the green panel's are gone"
        );
        g.clock = Some(t0 + s(89));
        g.render(&mut surf, &mut blue.clone(), || {});
        assert_eq!(
            (held(&g).0, held(&g).1),
            (all.0, all.1),
            "drawn again in time"
        );
        g.clock = Some(t0 + s(140));
        g.trim_idle();
        assert_eq!(
            (held(&g).0, held(&g).1),
            (all.0, all.1),
            "a minute from its last frame"
        );
        g.clock = Some(t0 + s(149));
        g.trim_idle();
        assert_eq!(held(&g), (0, 0, 0));
        // The record of frame times is a minute long, two frames a second at most.
        for k in 0..5000 {
            g.clock = Some(t0 + s(200) + std::time::Duration::from_millis(100 * k));
            g.frame_no += 1;
            g.stamp();
        }
        assert!(g.stamps.len() <= 122, "{}", g.stamps.len());
    }

    /// A crossfade requested while one runs starts from what the window shows: the first
    /// image's share of the screen only falls, through the second request and after it, and
    /// the second change fades in for its full duration.
    #[test]
    fn extending_a_fade_never_brightens_the_old_frame() {
        let Ok((mut g, _)) = Gpu::headless("surfaceless") else {
            assert!(
                std::env::var_os("RI_REQUIRE_GL").is_none(),
                "no surfaceless EGL"
            );
            return;
        };
        let mut surf = g.pbuffer(64, 32).expect("pbuffer");
        let ms = std::time::Duration::from_millis;
        let t0 = std::time::Instant::now();
        let (red, blue, green) = (
            solid([255, 0, 0, 255]),
            solid([0, 0, 255, 255]),
            solid([0, 255, 0, 255]),
        );
        let draw = |g: &mut Gpu, surf: &mut Surface, fr: &Frame, at: u64| {
            g.clock = Some(t0 + ms(at));
            surf.reswap();
            g.render(surf, &mut fr.clone(), || {});
            g.read_surface(surf).px[16 * 64 + 32]
        };
        draw(&mut g, &mut surf, &red, 0);
        g.begin_fade(&mut surf, &red, ms(200), None);
        let mut shown: Vec<(u64, [u8; 4])> = [0, 50, 100]
            .map(|t| (t, draw(&mut g, &mut surf, &blue, t)))
            .into();
        g.begin_fade(&mut surf, &blue, ms(200), None);
        shown.extend([100, 150, 200, 250, 299].map(|t| (t, draw(&mut g, &mut surf, &green, t))));
        for w in shown.windows(2) {
            assert!(
                w[1].1[0] <= w[0].1[0],
                "the red frame came back from {:?} to {:?}: {shown:?}",
                w[0],
                w[1]
            );
        }
        assert_eq!(shown[0].1[0], 255, "{shown:?}");
        let (_, at100) = shown[3];
        assert!(at100[1] < 4, "green before its fade: {shown:?}");
        assert!(shown[7].1[1] > 200, "green late in its fade: {shown:?}");
        assert_eq!(draw(&mut g, &mut surf, &green, 300), [0, 255, 0, 255]);
        assert!(surf.fade.is_none());
    }

    /// Submitted compares are read back once their fence has signalled, with the same answers
    /// as an immediate read.
    #[test]
    fn submitted_compares_read_after_their_fence() {
        let Ok((mut g, _)) = Gpu::headless("surfaceless") else {
            assert!(
                std::env::var_os("RI_REQUIRE_GL").is_none(),
                "no surfaceless EGL"
            );
            return;
        };
        let (w, h) = (300, 200);
        let mut px: Vec<u8> = (0..w * h * 4).map(|i| (i * 7 % 251) as u8).collect();
        let up = |g: &Gpu, t: glow::Texture, px: &[u8]| {
            // SAFETY: `g` made its context current when created or rendering, and this test binds no other.
            unsafe { g.gl.bind_texture(glow::TEXTURE_2D, Some(t)) };
            // SAFETY: as above.
            unsafe {
                g.gl.tex_sub_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    0,
                    0,
                    w,
                    h,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(Some(px)),
                )
            };
        };
        // SAFETY: `g` made its context current when created or rendering, and this test binds no other.
        let (t1, ..) = unsafe { g.target(w, h) }.expect("target");
        // SAFETY: `g` made its context current when created or rendering, and this test binds no other.
        let (t2, ..) = unsafe { g.target(w, h) }.expect("target");
        up(&g, t1, &px);
        up(&g, t2, &px);
        let mut copies = [None, None];
        for (t, c) in [t1, t2].into_iter().zip(&mut copies) {
            assert_eq!(g.frame_compare(t, w, h, (w, h), c), Some(true));
        }
        px[11] ^= 1;
        up(&g, t2, &px);
        for (t, c) in [t1, t2].into_iter().zip(&mut copies) {
            assert_eq!(g.frame_compare(t, w, h, (w, h), c), None);
        }
        let native = g.fence.is_some_and(|f| f.dup.is_some());
        let fd = g.compare_submit();
        assert_eq!(
            fd.is_some(),
            native,
            "a sync file exactly with native fences"
        );
        if let Some(fd) = fd {
            use std::os::fd::AsRawFd;
            // A sync file polls readable once its fence signals (sync_file(4)).
            let mut p = libc::pollfd {
                fd: fd.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            // SAFETY: `p` is one valid pollfd for the open fd.
            assert_eq!(unsafe { libc::poll(&mut p, 1, -1) }, 1);
            assert!(g.compare_ready(), "readable sync file, unsignalled fence");
        }
        assert_eq!(g.compare_read(), [false, true]);
        assert!(g.diff_sync.is_none());
        for c in copies {
            g.free_frame_copy(c);
        }
    }

    /// A crossfade of a change to a part of the window (a page of the settings window) repaints
    /// and damages that part only, frame after frame: the rest of the window is neither drawn
    /// by the shell nor blended by the compositor. The part itself blends from the old frame
    /// to the new one like a whole-window fade.
    #[test]
    fn a_fade_of_a_part_damages_only_that_part() {
        let Ok((mut g, _)) = Gpu::headless("surfaceless") else {
            assert!(
                std::env::var_os("RI_REQUIRE_GL").is_none(),
                "no surfaceless EGL"
            );
            return;
        };
        let px = |c: [u8; 4]| Rc::new(Canvas::new(4, 4, c));
        let (bg, red, blue) = (
            px([0, 255, 0, 255]),
            px([255, 0, 0, 255]),
            px([0, 0, 255, 255]),
        );
        let frame = |a: &Rc<Canvas>| {
            let mut fr = Frame::new(64, 32);
            let uv = [0.0, 0.0, 1.0, 1.0];
            fr.image(bg.clone(), [0.0, 0.0, 64.0, 32.0], uv, 1.0, false);
            fr.image(a.clone(), [4.0, 4.0, 8.0, 8.0], uv, 1.0, false);
            fr
        };
        let ms = std::time::Duration::from_millis;
        let mut surf = g.pbuffer(64, 32).expect("pbuffer");
        g.age_override = Some(1);
        let t0 = std::time::Instant::now();
        g.clock = Some(t0);
        let (old, new) = (frame(&red), frame(&blue));
        g.render(&mut surf, &mut old.clone(), || {});
        let part = new.changed_part(&old).expect("a part changed");
        // The changed image's box, a pixel wider each way as every signature is.
        assert_eq!(part, [[3, 3, 13, 13]]);
        assert_eq!(
            solid([0, 0, 255, 255]).changed_part(&old),
            None,
            "all changed"
        );
        g.begin_fade(&mut surf, &old, ms(200), Some(&part));
        assert_eq!(g.fade_px, 100, "pixels that fade");
        assert_eq!(g.fade_drawn, 0, "the buffer shows the old frame already");
        let mut inside = Vec::new();
        for i in 0..=10 {
            g.clock = Some(t0 + ms(i * 20));
            g.render(&mut surf, &mut new.clone(), || {});
            match i {
                // The start image at opacity 1 over the new frame is the frame on screen.
                0 => assert_eq!((g.last_damage, g.shown), ([0; 4], 1), "first frame drawn"),
                1..10 => assert_eq!(g.last_damage, [3, 3, 13, 13], "fade frame {i} damages more"),
                _ => {}
            }
            let shown = g.read_surface(&surf);
            assert_eq!(shown.px[0], [0, 255, 0, 255], "outside the part");
            assert_eq!(shown.px[13 * 64 + 13], [0, 255, 0, 255], "outside the part");
            inside.push(shown.px[8 * 64 + 8]);
        }
        assert_eq!(
            inside[0],
            [255, 0, 0, 255],
            "the fade starts from the old frame"
        );
        assert!(
            inside[5][0] > 20 && inside[5][2] > 20,
            "no blend halfway: {inside:?}"
        );
        assert_eq!(
            inside[10],
            [0, 0, 255, 255],
            "the fade ends on the new frame"
        );
        assert!(surf.fade.is_none(), "the fade never ended");
        // A fade requested while one runs takes the whole window: it starts from what is shown.
        g.begin_fade(&mut surf, &new, ms(200), Some(&part));
        g.begin_fade(&mut surf, &new, ms(200), Some(&part));
        g.render(&mut surf, &mut old.clone(), || {});
        assert_eq!(g.last_damage, [0, 0, 64, 32]);
        assert_eq!(g.read_surface(&surf).px[8 * 64 + 8], [0, 0, 255, 255]);
    }

    /// The fade of a part starts from the window's back buffer where that already shows the
    /// old frame, and draws the old frame again only where the buffer is older (what the
    /// frames since it was shown changed), or everywhere in the part when its pixels are
    /// unknown.
    #[test]
    fn a_fade_of_a_part_draws_only_what_its_buffer_lacks() {
        let Ok((mut g, _)) = Gpu::headless("surfaceless") else {
            assert!(
                std::env::var_os("RI_REQUIRE_GL").is_none(),
                "no surfaceless EGL"
            );
            return;
        };
        let px = |c: [u8; 4]| Rc::new(Canvas::new(4, 4, c));
        let (bg, red, blue) = (
            px([0, 255, 0, 255]),
            px([255, 0, 0, 255]),
            px([0, 0, 255, 255]),
        );
        let frame = |a: &Rc<Canvas>, b: &Rc<Canvas>| {
            let mut fr = Frame::new(64, 32);
            let uv = [0.0, 0.0, 1.0, 1.0];
            fr.image(bg.clone(), [0.0, 0.0, 64.0, 32.0], uv, 1.0, false);
            fr.image(a.clone(), [4.0, 4.0, 8.0, 8.0], uv, 1.0, false);
            fr.image(b.clone(), [40.0, 4.0, 8.0, 8.0], uv, 1.0, false);
            fr
        };
        let ms = std::time::Duration::from_millis;
        let (first, old, new) = (frame(&red, &red), frame(&red, &blue), frame(&blue, &red));
        let part = new.changed_part(&old).expect("a part changed");
        assert_eq!(part, [[3, 3, 13, 13], [39, 3, 49, 13]]);
        // (buffer age, pixels of the old frame drawn): a buffer two swaps old lacks what the
        // last frame changed, the second image.
        for (age, drawn) in [(1, 0), (2, 100), (0, 200), (3, 200)] {
            let mut surf = g.pbuffer(64, 32).expect("pbuffer");
            g.age_override = Some(1);
            g.render(&mut surf, &mut first.clone(), || {});
            g.render(&mut surf, &mut old.clone(), || {});
            g.age_override = Some(age);
            g.begin_fade(&mut surf, &old, ms(200), Some(&part));
            assert_eq!((g.fade_px, g.fade_drawn), (200, drawn), "age {age}");
            g.clock = Some(std::time::Instant::now());
            g.render(&mut surf, &mut new.clone(), || {});
            let shown = g.read_surface(&surf);
            assert_eq!(shown.px[8 * 64 + 8], [255, 0, 0, 255], "age {age}");
            assert_eq!(shown.px[8 * 64 + 44], [0, 0, 255, 255], "age {age}");
        }
        // A frame other than the one the window shows is drawn in full.
        let mut surf = g.pbuffer(64, 32).expect("pbuffer");
        g.age_override = Some(1);
        g.render(&mut surf, &mut first.clone(), || {});
        g.begin_fade(&mut surf, &old, ms(200), Some(&part));
        assert_eq!(g.fade_drawn, 200, "another frame");
    }

    /// Scrolling a ListView or a Flickable (with or without a ScrollBar) by `contentY` damages
    /// the view's own box and nothing else of the window, when the buffer's age is known.
    #[test]
    #[cfg_attr(
        not(feature = "interp"),
        ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
    )]
    fn scrolling_a_view_damages_only_the_view() {
        const WIN: [f64; 2] = [300.0, 400.0];
        const VIEW: [i64; 4] = [50, 100, 250, 300];
        let list = r##"import QtQuick
import QtQuick.Controls
import Rustshell
FloatingWindow {
    width: 300; height: 400
    color: "white"
    property var view: v
    ListView {
        id: v
        x: 50; y: 100; width: 200; height: 200
        clip: true
        model: 40
        BAR
        delegate: Rectangle {
            width: v.width; height: 40
            color: index % 2 ? "#335577" : "#775533"
        }
    }
}"##;
        let flick = r##"import QtQuick
import QtQuick.Controls
import Rustshell
FloatingWindow {
    width: 300; height: 400
    color: "white"
    property var view: v
    Flickable {
        id: v
        x: 50; y: 100; width: 200; height: 200
        clip: true
        contentWidth: width; contentHeight: col.height
        BAR
        Column {
            id: col
            Repeater {
                model: 40
                Rectangle {
                    width: 200; height: 40
                    color: index % 2 ? "#335577" : "#775533"
                }
            }
        }
    }
}"##;
        let bar = "ScrollBar.vertical: ScrollBar { }";
        for (name, src) in [
            ("ListView", list.replace("BAR", "")),
            ("ListView with a ScrollBar", list.replace("BAR", bar)),
            ("Flickable", flick.replace("BAR", "")),
            ("Flickable with a ScrollBar", flick.replace("BAR", bar)),
        ] {
            // A thread per case: the scene keeps per-item caches in thread locals.
            let case = std::thread::spawn(move || {
                let Ok((mut g, _)) = Gpu::headless("surfaceless") else {
                    assert!(
                        std::env::var_os("RI_REQUIRE_GL").is_none(),
                        "no surfaceless EGL/GLES2 context (RI_REQUIRE_GL is set)"
                    );
                    return;
                };
                let mut rt = qml::runtime::Runtime::new(None);
                let win = rt.load_str(&src).unwrap();
                let mut fonts = crate::scene::Fonts::default();
                rt.settle();
                crate::scene::layout_window(&mut rt, &mut fonts, win, WIN);
                rt.settle();
                let Some(qml::value::Value::Item(view)) = rt.world.borrow_mut().read(win, "view")
                else {
                    panic!("{name}: no view");
                };
                let mut surf = g.pbuffer(300, 400).expect("pbuffer");
                g.age_override = Some(1);
                let mut laid = rt.world.borrow().layout_rev;
                // One frame as the shell draws it; returns the bounds of what it repainted.
                let mut frame = |rt: &mut qml::runtime::Runtime, g: &mut Gpu| {
                    rt.settle();
                    for pass in 0..4 {
                        let rs = std::mem::take(&mut rt.world.borrow_mut().resized);
                        let mut w = rt.world.borrow_mut();
                        if w.layout_rev != laid
                            || !rs.iter().all(|&i| crate::scene::is_local_resize(&mut w, i))
                        {
                            drop(w);
                            crate::scene::layout_window(rt, &mut fonts, win, WIN);
                            laid = rt.world.borrow().layout_rev;
                            rt.world.borrow_mut().resized.clear();
                            break;
                        }
                        if rs.is_empty() {
                            break;
                        }
                        let rev = w.layout_rev;
                        crate::scene::relayout_targets(&mut w, &mut fonts, &rs, pass);
                        w.layout_rev = rev;
                        drop(w);
                        rt.settle();
                    }
                    {
                        let mut w = rt.world.borrow_mut();
                        for f in std::mem::take(&mut w.scrolled) {
                            crate::scene::flick_flags(&mut w, f);
                        }
                        let touched = std::mem::take(&mut w.touched);
                        let moved = std::mem::take(&mut w.moved);
                        crate::scene::invalidate_layers(&w, &touched);
                        crate::scene::invalidate_moved(&w, &moved);
                    }
                    let mut fr = Frame::new(300, 400);
                    crate::scene::paint_window(
                        rt,
                        &mut fonts,
                        win,
                        &mut crate::scene::Target::Gpu(&mut fr),
                    );
                    g.render(&mut surf, &mut fr, || {});
                    g.last_damage
                };
                assert_eq!(
                    frame(&mut rt, &mut g),
                    [0, 0, 300, 400],
                    "{name}: first frame"
                );
                frame(&mut rt, &mut g);
                assert_eq!(frame(&mut rt, &mut g), [0; 4], "{name}: nothing changed");
                for step in 1..=6 {
                    let y = f64::from(step) * 10.0;
                    rt.world
                        .borrow_mut()
                        .write(view, "contentY", qml::value::Value::Num(y));
                    assert_eq!(frame(&mut rt, &mut g), VIEW, "{name}: contentY {y}");
                }
            });
            if let Err(e) = case.join() {
                std::panic::resume_unwind(e);
            }
        }
    }

    /// An idle window holds no window-sized copy of itself: the crossfade's start image and its
    /// `fresh` copy exist only while the fade runs, the first fade frame shows exactly the old
    /// frame, and the targets are deleted as soon as the fade ends.
    #[test]
    fn fade_targets_live_only_while_fading() {
        let Ok((mut g, _)) = Gpu::headless("surfaceless") else {
            assert!(
                std::env::var_os("RI_REQUIRE_GL").is_none(),
                "no surfaceless EGL/GLES2 context (RI_REQUIRE_GL is set)"
            );
            return;
        };
        let mut surf = g.pbuffer(64, 32).expect("pbuffer");
        let t0 = std::time::Instant::now();
        g.clock = Some(t0);
        let (red, blue) = (solid([255, 0, 0, 255]), solid([0, 0, 255, 255]));
        g.render(&mut surf, &mut red.clone(), || {});
        let old = g.read_surface(&surf).px;
        assert!(
            surf.fade.is_none() && surf.fresh.is_none(),
            "idle window keeps a copy"
        );
        g.begin_fade(&mut surf, &red, std::time::Duration::from_millis(100), None);
        g.render(&mut surf, &mut blue.clone(), || {});
        assert_eq!(
            g.read_surface(&surf).px,
            old,
            "first fade frame is not the old frame"
        );
        assert_eq!(g.shown, 1, "the frame on screen was drawn again");
        g.clock = Some(t0 + std::time::Duration::from_millis(10));
        g.render(&mut surf, &mut blue.clone(), || {});
        assert_eq!(g.shown, 2, "the fade's second frame was not shown");
        let fade = surf.fade.as_ref().map(|f| (f.tex, f.fb));
        let targets: Vec<_> = fade.into_iter().chain(surf.fresh).collect();
        assert_eq!(targets.len(), 2);
        let alive = |g: &Gpu| {
            targets.iter().any(|&(t, f)| {
                // SAFETY: `g` made its context current when created or rendering, and this test binds no other.
                let tex = unsafe { g.gl.is_texture(t) };
                // SAFETY: as above.
                tex || unsafe { g.gl.is_framebuffer(f) }
            })
        };
        assert!(alive(&g));
        let mut frames = 1;
        while surf.fading() {
            frames += 1;
            assert!(frames < 20, "fade never ended");
            g.clock = Some(t0 + std::time::Duration::from_millis(30 * frames));
            g.render(&mut surf, &mut blue.clone(), || {});
        }
        assert!(surf.fresh.is_none());
        assert!(
            g.dead.borrow().targets.is_empty(),
            "ended fade's targets left queued"
        );
        assert!(!alive(&g), "ended fade's targets still allocated");
        assert_eq!(g.read_surface(&surf).px[0], [0, 0, 255, 255]);
    }
}

#[cfg(test)]
mod frame_changed_tests {
    use super::*;

    #[test]
    fn detects_single_pixel_changes() {
        let Ok(conn) = wayland_client::Connection::connect_to_env() else {
            return skip_without("Wayland compositor");
        };
        // SAFETY: `conn` outlives `g`.
        let Some(mut g) = (unsafe { Gpu::new(conn.backend().display_ptr() as *mut _) }) else {
            return skip_without("EGL/GLES2 context on the Wayland display");
        };
        elog::eprintln!("frame_changed test on {}", g.renderer);
        let (w, h) = (949, 1028);
        // SAFETY: `g` made its context current when created or rendering, and this test binds no other.
        let (t, ..) = unsafe { g.target(w, h) }.expect("target");
        let mut px: Vec<u8> = (0..w * h * 4).map(|i| (i * 7 % 251) as u8).collect();
        let up = |g: &Gpu, px: &[u8]| {
            // SAFETY: `g` made its context current when created or rendering, and this test binds no other.
            unsafe { g.gl.bind_texture(glow::TEXTURE_2D, Some(t)) };
            // SAFETY: as above.
            unsafe {
                g.gl.tex_sub_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    0,
                    0,
                    w,
                    h,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(Some(px)),
                )
            };
        };
        let mut copy = None;
        up(&g, &px);
        assert!(g.frame_changed(t, w, h, (w, h), &mut copy));
        assert!(!g.frame_changed(t, w, h, (w, h), &mut copy));
        for at in [
            0,
            (w * h - 1) * 4 + 2,
            (500 * w + 948) * 4 + 3,
            (1027 * w + 3) * 4,
        ] {
            px[at as usize] ^= 1;
            up(&g, &px);
            assert!(
                g.frame_changed(t, w, h, (w, h), &mut copy),
                "pixel byte {at}"
            );
            // A change starts a run reported changed without comparing.
            for _ in 0..DIFF_TRUST {
                assert!(g.frame_changed(t, w, h, (w, h), &mut copy));
            }
            assert!(
                !g.frame_changed(t, w, h, (w, h), &mut copy),
                "pixel byte {at} again"
            );
        }
        assert!(g.frame_changed(t, w - 1, h, (w - 1, h), &mut copy));
        g.free_frame_copy(copy);

        // Compares read back together keep their own results.
        // SAFETY: `g` made its context current when created or rendering, and this test binds no other.
        let (t2, ..) = unsafe { g.target(w, h) }.expect("target");
        let mut copies = [None, None];
        for (t, c) in [t, t2].into_iter().zip(&mut copies) {
            assert_eq!(g.frame_compare(t, w, h, (w, h), c), Some(true));
        }
        px[7] ^= 1;
        up(&g, &px);
        for (t, c) in [t2, t].into_iter().zip(copies.iter_mut().rev()) {
            assert_eq!(g.frame_compare(t, w, h, (w, h), c), None);
        }
        assert_eq!(g.compare_read(), [false, true]);
        for c in copies {
            g.free_frame_copy(c);
        }
    }
}

#[cfg(test)]
mod reorder_tests {
    use super::*;

    #[test]
    fn reorder_damages_only_the_moved_primitive() {
        let p = |k: u64, x: i64| (k, [x, 0, x + 10, 10]);
        let prev = [p(1, 0), p(2, 20), p(3, 40), p(4, 60)];
        let cur = [p(1, 0), p(3, 40), p(2, 20), p(4, 60)];
        let d = diff(&prev, &cur, [0, 0, 100, 10]).unwrap();
        assert!(d == [[20, 0, 30, 10]] || d == [[40, 0, 50, 10]], "{d:?}");
        assert_eq!(diff(&prev, &prev, [0, 0, 100, 10]), None);
        let moved = [p(4, 60), p(1, 0), p(2, 20), p(3, 40)];
        assert_eq!(
            diff(&prev, &moved, [0, 0, 100, 10]),
            Some(vec![[60, 0, 70, 10]])
        );
    }
}

#[cfg(test)]
mod dpr_tests {
    use super::*;

    #[test]
    fn scaled_frame_is_device_sized() {
        assert_eq!(device_size(101, 51, 1.5), (152, 77));
        assert_eq!(device_size(1920, 40, 1.25), (2400, 50));
        // A layer's texture is its size times the scale, rounded once (QSizeF::toSize).
        let l = Frame::sized(40.4, 30.6, 1.5);
        assert_eq!((l.w, l.h), (61, 46));
        assert_eq!(
            l.xf,
            Some([(61.0 / 40.4) as f32, (46.0 / 30.6) as f32, 0.0, 0.0])
        );
        let l = Frame::sized(0.2, 0.2, 2.0);
        assert_eq!((l.w, l.h, l.xf), (1, 1, Some([2.0, 2.0, 0.0, 0.0])));
        let f = Frame::scaled(101, 51, 1.5);
        assert_eq!((f.w, f.h, f.lw, f.lh), (152, 77, 101, 51));
        // 152 by 77 pixels are not 101 by 51 units times 1.5: Qt projects over whole units
        // (QQuickWindowPrivate::renderSceneGraph), so a unit is 152 / 101 by 77 / 51 pixels.
        let own = [(152.0 / 101.0) as f32, (77.0 / 51.0) as f32, 0.0, 0.0];
        assert_eq!(f.xf, Some(own));
        assert_eq!(f.units(), (101.0, 51.0));
        assert!(f.plain());
        let f = Frame::scaled(100, 50, 1.5);
        assert_eq!(f.xf, Some([1.5, 1.5, 0.0, 0.0]));
        // A texture that is no window keeps the devicePixelRatio.
        let f = Frame::offscreen(101, 51, 1.5);
        assert_eq!(f.xf, Some([1.5, 1.5, 0.0, 0.0]));
        assert!(f.plain());
        let f = Frame::scaled(10, 10, 1.0);
        assert!(f.xf.is_none());
        assert_eq!((f.w, f.lw), (10, 10));
    }

    #[test]
    fn clips_map_scene_to_device_pixels() {
        let mut f = Frame::scaled(100, 50, 2.0);
        f.push_clip_f([10.0, 5.0, 80.0, 40.0]);
        assert_eq!(f.clip_top(), Some([20, 10, 180, 90]));
        f.pop_clip();
        f.push_clip_f([50.0, 0.0, 500.0, 500.0]);
        assert_eq!(f.clip_top(), Some([100, 0, 1100, 1000]));
    }

    /// Qt rasterizes NativeRendering glyphs at pixelSize * devicePixelRatio; stretching 1x
    /// bitmaps (the transform path) is the blur the audit found.
    #[test]
    fn glyphs_rasterize_at_device_size() {
        let mut fonts = crate::scene::Fonts::default();
        let Some(st) = fonts.get("sans-serif", 14, 400) else {
            testenv::unavailable("no font for the family sans-serif");
            return;
        };
        let line = st.shape("Hg");
        let one = st.line_rects_dpr(&line, 0.0, 0.0, 3.0, 4.0, [1.0, 1.0, 0.0, 0.0], 1.0);
        let two = st.line_rects_dpr(&line, 0.0, 0.0, 3.0, 4.0, [2.0, 2.0, 0.0, 0.0], 2.0);
        assert!(!one.is_empty());
        assert_eq!(one.len(), two.len());
        for ((g1, r1), (g2, r2)) in one.iter().zip(&two) {
            assert!(g1.rows > 4);
            assert!(
                (g2.rows as i64 - 2 * g1.rows as i64).abs() <= 2,
                "{} rows at 2x vs {} at 1x",
                g2.rows,
                g1.rows
            );
            // One texel per device pixel.
            assert_eq!(r2[2] - r2[0], g2.width as f32);
            assert_eq!(r2[3] - r2[1], g2.rows as f32);
            assert!((r2[0] - 2.0 * r1[0]).abs() <= 2.0, "{r1:?} {r2:?}");
            assert!((r2[1] - 2.0 * r1[1]).abs() <= 2.0, "{r1:?} {r2:?}");
        }
        // Fractional scales too (14 * 1.5 = 21px).
        let st15 = st.scaled(1.5).unwrap();
        assert!(std::rc::Rc::ptr_eq(&st15, &st.scaled(1.5).unwrap()));
        let r15 = st.line_rects_dpr(&line, 0.0, 0.0, 3.0, 4.0, [1.5, 1.5, 0.0, 0.0], 1.5);
        for ((g1, _), (g15, _)) in one.iter().zip(&r15) {
            let want = g1.rows as f64 * 1.5;
            assert!(
                (g15.rows as f64 - want).abs() <= 2.0,
                "{} vs {want}",
                g15.rows
            );
        }
    }

    /// A Text in a window drawn at devicePixelRatio 2: device-sized frame, glyph bitmaps
    /// twice as tall as at 1, and no stretched glyphs.
    #[test]
    #[cfg_attr(
        not(feature = "interp"),
        ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
    )]
    fn scene_text_in_hidpi_window() {
        let mut rt = qml::runtime::Runtime::new(None);
        let win = rt
            .load_str(
                r#"import QtQuick
import Rustshell
FloatingWindow {
    width: 120; height: 40
    color: "transparent"
    Text { x: 3; y: 2; text: "Hg"; font.pixelSize: 16; font.family: "sans-serif"; renderType: Text.NativeRendering }
}"#,
            )
            .unwrap();
        let mut fonts = crate::scene::Fonts::default();
        if fonts.get("sans-serif", 16, 400).is_none() {
            testenv::unavailable("no font for the family sans-serif");
            return;
        }
        crate::scene::layout_window(&mut rt, &mut fonts, win, [120.0, 40.0]);
        let rows = |dpr: f64, rt: &mut qml::runtime::Runtime, fonts: &mut crate::scene::Fonts| {
            let mut fr = Frame::scaled(120, 40, dpr);
            crate::scene::paint_window(rt, fonts, win, &mut crate::scene::Target::Gpu(&mut fr));
            assert_eq!((fr.w, fr.h), crate::gpu::device_size(120, 40, dpr));
            let mut max = 0;
            for c in &fr.cmds {
                match c {
                    Cmd::GlyphsF(_, q) => {
                        assert!(dpr != 1.0);
                        for (g, r) in q.iter() {
                            assert_eq!(r[3] - r[1], g.rows as f32, "stretched glyph");
                            max = max.max(g.rows);
                        }
                    }
                    Cmd::Glyphs(_, q) => {
                        for (g, ..) in q.iter() {
                            max = max.max(g.rows);
                        }
                    }
                    _ => {}
                }
            }
            max
        };
        let r1 = rows(1.0, &mut rt, &mut fonts);
        let r2 = rows(2.0, &mut rt, &mut fonts);
        assert!(r1 > 4, "no text drawn");
        assert!((r2 as i64 - 2 * r1 as i64).abs() <= 2, "{r2} vs {r1}");
    }

    /// ColorOverlay/Desaturate keep their source on the GPU: no CPU raster is uploaded per
    /// repaint, and a recolour only changes the tint, not the source layer.
    #[test]
    #[cfg_attr(
        not(feature = "interp"),
        ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
    )]
    fn colour_effects_composite_gpu_layers() {
        let mut rt = qml::runtime::Runtime::new(None);
        let win = rt
            .load_str(
                r##"import QtQuick
import Qt5Compat.GraphicalEffects
import Rustshell
FloatingWindow {
    width: 60; height: 30
    color: "transparent"
    property color tint: "#ff0000"
    Rectangle { id: r; width: 20; height: 20; color: "#336699"; visible: false }
    ColorOverlay { width: 20; height: 20; source: r; color: tint }
    Desaturate { x: 30; width: 20; height: 20; source: r; desaturation: 0.5 }
}"##,
            )
            .unwrap();
        let mut fonts = crate::scene::Fonts::default();
        crate::scene::layout_window(&mut rt, &mut fonts, win, [60.0, 30.0]);
        let tints = |rt: &mut qml::runtime::Runtime, fonts: &mut crate::scene::Fonts| {
            let mut fr = Frame::scaled(60, 30, 1.0);
            crate::scene::paint_window(rt, fonts, win, &mut crate::scene::Target::Gpu(&mut fr));
            let mut out = Vec::new();
            for c in &fr.cmds {
                match c {
                    Cmd::Tint {
                        src, k, mode, crop, ..
                    } => out.push((src.digest(), *k, *mode, *crop)),
                    Cmd::Blit(..) => panic!("CPU raster uploaded"),
                    _ => {}
                }
            }
            out
        };
        let a = tints(&mut rt, &mut fonts);
        assert_eq!(a.len(), 2, "{a:?}");
        assert_eq!((a[0].1, a[0].2), ([1.0, 0.0, 0.0, 1.0], OVERLAY));
        assert_eq!((a[1].1, a[1].2), ([0.5, 0.0, 0.0, 0.0], DESAT));
        assert_eq!(a[0].0, a[1].0, "same source layer");
        let blue = qml::color::Color::from_rgb_f(0.0, 0.0, 1.0, 1.0);
        rt.set(win, "tint", qml::value::Value::Color(blue));
        let b = tints(&mut rt, &mut fonts);
        assert_eq!(b[0].0, a[0].0, "recolour re-rendered the source");
        assert_eq!(b[0].1, [0.0, 0.0, 1.0, 1.0]);
    }

    /// The GPU ColorOverlay/Desaturate composite draws what the CPU raster path draws.
    #[test]
    #[cfg_attr(
        not(feature = "interp"),
        ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
    )]
    fn colour_effects_match_the_cpu_raster() {
        let Ok((mut g, _)) = Gpu::headless("surfaceless") else {
            assert!(
                std::env::var_os("RI_REQUIRE_GL").is_none(),
                "no surfaceless EGL/GLES2 context (RI_REQUIRE_GL is set)"
            );
            return;
        };
        let mut rt = qml::runtime::Runtime::new(None);
        let win = rt
            .load_str(
                r##"import QtQuick
import Qt5Compat.GraphicalEffects
import Rustshell
FloatingWindow {
    width: 96; height: 40
    color: "#202020"
    Item {
        id: src; width: 30; height: 30; visible: false
        Rectangle { anchors.fill: parent; radius: 9; color: "#cc3366ff" }
        Rectangle { x: 6; y: 6; width: 12; height: 12; color: "#ffee8800" }
    }
    ColorOverlay { x: 2; y: 2; width: 30; height: 30; source: src; color: "#c07fd0a0" }
    Desaturate { x: 34; y: 3; width: 30; height: 30; source: src; desaturation: 0.7 }
    ColorOverlay { x: 66.5; y: 4; width: 24; height: 24; source: src; color: "#ffffff"; opacity: 0.6 }
}"##,
            )
            .unwrap();
        let mut fonts = crate::scene::Fonts::default();
        crate::scene::layout_window(&mut rt, &mut fonts, win, [96.0, 40.0]);
        let mut cpu = Canvas::new(96, 40, [0; 4]);
        crate::scene::paint_window(
            &mut rt,
            &mut fonts,
            win,
            &mut crate::scene::Target::Cpu(&mut cpu),
        );
        let mut fr = Frame::new(96, 40);
        crate::scene::paint_window(
            &mut rt,
            &mut fonts,
            win,
            &mut crate::scene::Target::Gpu(&mut fr),
        );
        assert!(fr.cmds.iter().any(|c| matches!(c, Cmd::Tint { .. })));
        let mut s = g.pbuffer(96, 40).expect("pbuffer");
        s.reswap();
        g.render(&mut s, &mut fr, || {});
        let gpu = g.read_surface(&s);
        let mut worst = (0, 0);
        for (k, (a, b)) in gpu.px.iter().zip(&cpu.px).enumerate() {
            let d = (0..4).map(|c| a[c].abs_diff(b[c])).max().unwrap_or(0);
            if d > worst.0 {
                worst = (d, k);
            }
        }
        let (d, k) = worst;
        assert!(
            d <= 3,
            "pixel ({}, {}): gpu {:?} cpu {:?}",
            k % 96,
            k / 96,
            gpu.px[k],
            cpu.px[k]
        );
    }

    /// Qt5Compat GaussianBlur/DropShadow run as GPU passes over the source layer and an uncached
    /// RectangularShadow as a fragment shader: no CPU raster is uploaded, the pixels match the
    /// CPU path, and an unchanged source reuses its blurred texture instead of blurring again.
    #[test]
    #[cfg_attr(
        not(feature = "interp"),
        ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
    )]
    fn blur_and_shadow_effects_run_on_the_gpu_like_the_cpu_raster() {
        let Ok((mut g, _)) = Gpu::headless("surfaceless") else {
            assert!(
                std::env::var_os("RI_REQUIRE_GL").is_none(),
                "no surfaceless EGL/GLES2 context (RI_REQUIRE_GL is set)"
            );
            return;
        };
        let mut rt = qml::runtime::Runtime::new(None);
        let win = rt
            .load_str(
                r##"import QtQuick
import QtQuick.Effects
import Qt5Compat.GraphicalEffects
import Rustshell
FloatingWindow {
    width: 160; height: 64
    color: "#203040"
    property real sx: 112.25
    Item {
        id: src; width: 30; height: 24; visible: false
        Rectangle { anchors.fill: parent; radius: 8; color: "#cc3366ff" }
        Rectangle { x: 5; y: 4; width: 10; height: 9; color: "#ffee8800" }
    }
    GaussianBlur { x: 8; y: 8; width: 30; height: 24; source: src; radius: 6; samples: 13; transparentBorder: true }
    GaussianBlur { x: 8; y: 38; width: 30; height: 24; source: src; radius: 5; samples: 8 }
    DropShadow { x: 58; y: 12; width: 30; height: 24; source: src; radius: 8; samples: 17
        color: "#a0000000"; spread: 0.2; horizontalOffset: 1.4; verticalOffset: 3; transparentBorder: true }
    RectangularShadow { x: sx; y: 14.5; width: 30; height: 30; radius: 7; blur: 9; spread: 2
        offset: Qt.vector2d(0.5, 2); color: "#c0102030" }
}"##,
            )
            .unwrap();
        let mut fonts = crate::scene::Fonts::default();
        crate::scene::layout_window(&mut rt, &mut fonts, win, [160.0, 64.0]);
        let mut cpu = Canvas::new(160, 64, [0; 4]);
        crate::scene::paint_window(
            &mut rt,
            &mut fonts,
            win,
            &mut crate::scene::Target::Cpu(&mut cpu),
        );
        let frame = |rt: &mut qml::runtime::Runtime, fonts: &mut crate::scene::Fonts| {
            let mut fr = Frame::new(160, 64);
            crate::scene::paint_window(rt, fonts, win, &mut crate::scene::Target::Gpu(&mut fr));
            let n = |f: fn(&Cmd) -> bool| fr.cmds.iter().filter(|c| f(c)).count();
            assert_eq!(
                n(|c| matches!(c, Cmd::Gauss { .. })),
                3,
                "two blurs and a shadow"
            );
            assert_eq!(n(|c| matches!(c, Cmd::RShadow { .. })), 1);
            assert_eq!(n(|c| matches!(c, Cmd::Blit(..))), 0, "CPU raster");
            fr
        };
        let mut fr = frame(&mut rt, &mut fonts);
        let mut s = g.pbuffer(160, 64).expect("pbuffer");
        s.reswap();
        g.render(&mut s, &mut fr, || {});
        let gpu = g.read_surface(&s);
        let mut worst = (0, 0);
        for (k, (a, b)) in gpu.px.iter().zip(&cpu.px).enumerate() {
            let d = (0..4).map(|c| a[c].abs_diff(b[c])).max().unwrap_or(0);
            if d > worst.0 {
                worst = (d, k);
            }
        }
        let (d, k) = worst;
        assert!(
            d <= 2,
            "pixel ({}, {}): gpu {:?} cpu {:?}",
            k % 160,
            k / 160,
            gpu.px[k],
            cpu.px[k]
        );
        // Blurred textures are cached by source and kernel: a repaint (here with the shadow moved
        // by a subpixel) runs no blur pass and creates no texture.
        let layers: std::collections::BTreeSet<u64> =
            g.layers.map.iter().map(|(k, _)| *k).collect();
        rt.set(win, "sx", qml::value::Value::Num(112.6));
        let mut fr = frame(&mut rt, &mut fonts);
        take_gl_calls();
        s.reswap();
        g.render(&mut s, &mut fr, || {});
        let calls = take_gl_calls();
        assert_eq!(
            (calls[0], calls[2]),
            (0, 0),
            "texture uploads / new targets: {calls:?}"
        );
        assert_eq!(
            g.layers
                .map
                .iter()
                .map(|(k, _)| *k)
                .collect::<std::collections::BTreeSet<u64>>(),
            layers
        );
        let mut cpu = Canvas::new(160, 64, [0; 4]);
        crate::scene::paint_window(
            &mut rt,
            &mut fonts,
            win,
            &mut crate::scene::Target::Cpu(&mut cpu),
        );
        let moved = g.read_surface(&s);
        let d = moved
            .px
            .iter()
            .zip(&cpu.px)
            .flat_map(|(a, b)| (0..4).map(|c| a[c].abs_diff(b[c])));
        assert!(
            d.max().unwrap_or(0) <= 2,
            "moved shadow differs from the CPU raster"
        );
    }

    /// GaussianBlur.qml's SourceProxy: without transparentBorder the first pass samples the
    /// source texture over the effect whatever their sizes, with it the source is cropped 1:1 to
    /// sourceRect (-r, 0, w + 2r, h). Fractional and mismatched sources stay on the GPU and draw
    /// what the CPU raster draws, which takes Rectangle geometry through the layer's scale as
    /// the GPU does.
    #[test]
    #[cfg_attr(
        not(feature = "interp"),
        ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
    )]
    fn blur_and_shadow_resample_mismatched_sources_on_the_gpu_like_the_cpu_raster() {
        let Ok((mut g, _)) = Gpu::headless("surfaceless") else {
            assert!(
                std::env::var_os("RI_REQUIRE_GL").is_none(),
                "no surfaceless EGL/GLES2 context (RI_REQUIRE_GL is set)"
            );
            return;
        };
        let mut rt = qml::runtime::Runtime::new(None);
        let win = rt
            .load_str(
                r##"import QtQuick
import Qt5Compat.GraphicalEffects
import Rustshell
FloatingWindow {
    width: 200; height: 72
    color: "#203040"
    Item {
        id: frac; width: 22.5; height: 19.25; visible: false
        Rectangle { x: 1; y: 1; width: 19; height: 16; radius: 6; color: "#cc3366ff" }
        Rectangle { x: 4; y: 3; width: 9; height: 8; color: "#ffee8800" }
    }
    Item {
        id: big; width: 38; height: 31; visible: false
        Rectangle { anchors.fill: parent; radius: 9; color: "#e022aa44" }
        Rectangle { x: 26; y: 5; width: 10; height: 22; color: "#ff1040ff" }
    }
    GaussianBlur { x: 8; y: 8; width: 30; height: 24; source: frac; radius: 6; samples: 13; transparentBorder: true }
    GaussianBlur { x: 8; y: 42; width: 30; height: 24; source: frac; radius: 5; samples: 11 }
    GaussianBlur { x: 56; y: 8; width: 26; height: 20; source: big; radius: 4; samples: 9 }
    GaussianBlur { x: 56; y: 42; width: 30; height: 24; source: big; radius: 4; samples: 9; transparentBorder: true }
    DropShadow { x: 106; y: 10; width: 30; height: 24; source: big; radius: 6; samples: 13
        color: "#a0000000"; horizontalOffset: 2; verticalOffset: 3; transparentBorder: true }
    DropShadow { x: 150; y: 10; width: 30; height: 24; source: frac; radius: 5; samples: 11
        color: "#c0000000"; spread: 0.3; verticalOffset: 2; transparentBorder: true }
    DropShadow { x: 150; y: 42; width: 24; height: 20; source: big; radius: 3; samples: 7
        color: "#80000000"; horizontalOffset: -2 }
}"##,
            )
            .unwrap();
        let mut fonts = crate::scene::Fonts::default();
        crate::scene::layout_window(&mut rt, &mut fonts, win, [200.0, 72.0]);
        let mut cpu = Canvas::new(200, 72, [0; 4]);
        crate::scene::paint_window(
            &mut rt,
            &mut fonts,
            win,
            &mut crate::scene::Target::Cpu(&mut cpu),
        );
        let mut fr = Frame::new(200, 72);
        crate::scene::paint_window(
            &mut rt,
            &mut fonts,
            win,
            &mut crate::scene::Target::Gpu(&mut fr),
        );
        let n = |f: fn(&Cmd) -> bool| fr.cmds.iter().filter(|c| f(c)).count();
        assert_eq!(
            n(|c| matches!(c, Cmd::Gauss { .. })),
            7,
            "four blurs, three shadows"
        );
        assert_eq!(n(|c| matches!(c, Cmd::Blit(..))), 0, "CPU raster");
        let mut s = g.pbuffer(200, 72).expect("pbuffer");
        s.reswap();
        g.render(&mut s, &mut fr, || {});
        let gpu = g.read_surface(&s);
        let mut worst = (0, 0);
        for (k, (a, b)) in gpu.px.iter().zip(&cpu.px).enumerate() {
            let d = (0..4).map(|c| a[c].abs_diff(b[c])).max().unwrap_or(0);
            if d > worst.0 {
                worst = (d, k);
            }
        }
        let (d, k) = worst;
        assert!(
            d <= 2,
            "pixel ({}, {}): gpu {:?} cpu {:?}",
            k % 200,
            k / 200,
            gpu.px[k],
            cpu.px[k]
        );
    }

    /// A Shape in a window at devicePixelRatio 1.5 rasterizes on device pixels: a rectangle on
    /// whole device pixels has no blended edge column or row, which an upscaled logical raster
    /// (texel edges between device pixels, sampled linearly) always had.
    #[test]
    #[cfg_attr(
        not(feature = "interp"),
        ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
    )]
    fn scene_shape_edges_are_crisp_in_hidpi_window() {
        let mut rt = qml::runtime::Runtime::new(None);
        let win = rt
            .load_str(
                r##"import QtQuick
import QtQuick.Shapes
import Rustshell
FloatingWindow {
    width: 40; height: 20
    color: "transparent"
    Shape {
        width: 30; height: 20
        ShapePath {
            fillColor: "#ff0000"; strokeColor: "transparent"
            startX: 4; startY: 4
            PathLine { x: 24; y: 4 } PathLine { x: 24; y: 14 } PathLine { x: 4; y: 14 }
        }
    }
}"##,
            )
            .unwrap();
        let mut fonts = crate::scene::Fonts::default();
        crate::scene::layout_window(&mut rt, &mut fonts, win, [40.0, 20.0]);
        let mut fr = Frame::scaled(40, 20, 1.5);
        crate::scene::paint_window(
            &mut rt,
            &mut fonts,
            win,
            &mut crate::scene::Target::Gpu(&mut fr),
        );
        assert_eq!((fr.w, fr.h), (60, 30));
        let (mut g, _) = Gpu::headless("surfaceless").expect("surfaceless EGL (Mesa)");
        let mut surf = g.pbuffer(60, 30).expect("pbuffer");
        surf.reswap();
        g.render(&mut surf, &mut fr, || {});
        let shot = g.read_surface(&surf);
        assert_eq!((shot.w, shot.h), (60, 30));
        let at = |x: usize, y: usize| shot.px[y * shot.w + x];
        // (4, 4)-(24, 14) logical is device x 6..36, y 6..21.
        let row: Vec<[u8; 4]> = (0..60).map(|x| at(x, 10)).collect();
        let col: Vec<[u8; 4]> = (0..30).map(|y| at(20, y)).collect();
        assert_eq!(row[5], [0; 4], "{row:?}");
        assert_eq!(row[6], [255, 0, 0, 255], "{row:?}");
        assert_eq!(row[35], [255, 0, 0, 255], "{row:?}");
        assert_eq!(row[36], [0; 4], "{row:?}");
        assert_eq!(col[5], [0; 4], "{col:?}");
        assert_eq!(col[6], [255, 0, 0, 255], "{col:?}");
        assert_eq!(col[20], [255, 0, 0, 255], "{col:?}");
        assert_eq!(col[21], [0; 4], "{col:?}");
        let blended = |px: &[[u8; 4]]| px.iter().filter(|p| p[3] != 0 && p[3] != 255).count();
        assert_eq!(blended(&row), 0, "{row:?}");
        assert_eq!(blended(&col), 0, "{col:?}");
    }
}

#[cfg(test)]
mod warm_tests {
    use super::*;

    /// The context warm-up clears into its own target and leaves GL as a fresh context has it:
    /// no framebuffer or texture bound, no scissor and no error.
    #[test]
    fn warm_up_leaves_default_state() {
        let Ok((g, _)) = Gpu::headless("surfaceless") else {
            assert!(
                std::env::var_os("RI_REQUIRE_GL").is_none(),
                "no surfaceless EGL/GLES2"
            );
            return;
        };
        // SAFETY: `g` made its context current when created or rendering, and this test binds no other.
        unsafe { warm_up(&g.gl) };
        let gl = &g.gl;
        // SAFETY: `g` made its context current when created or rendering, and this test binds no other.
        unsafe { assert_eq!(gl.get_error(), glow::NO_ERROR) };
        // SAFETY: `g` made its context current when created or rendering, and this test binds no other.
        unsafe { assert!(!gl.is_enabled(glow::SCISSOR_TEST)) };
        // SAFETY: `g` made its context current when created or rendering, and this test binds no other.
        unsafe { assert_eq!(gl.get_parameter_i32(glow::FRAMEBUFFER_BINDING), 0) };
        // SAFETY: `g` made its context current when created or rendering, and this test binds no other.
        unsafe { assert_eq!(gl.get_parameter_i32(glow::TEXTURE_BINDING_2D), 0) };
    }

    /// Errors left by earlier calls are drained before a call checked with glGetError, so the
    /// check sees only that call: glGetError reports one recorded flag per call, oldest first.
    #[test]
    fn drain_gl_errors_clears_every_recorded_flag() {
        let Ok((g, _)) = Gpu::headless("surfaceless") else {
            assert!(
                std::env::var_os("RI_REQUIRE_GL").is_none(),
                "no surfaceless EGL/GLES2"
            );
            return;
        };
        let gl = &g.gl;
        // SAFETY: `g` made its context current when created, and this test binds no other.
        unsafe { gl.enable(0x1234) };
        // SAFETY: as above; a negative width records GL_INVALID_VALUE (ES 2.0 §3.7.1).
        unsafe {
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA as i32,
                -1,
                1,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(None),
            )
        };
        // SAFETY: as above.
        unsafe { drain_gl_errors(gl, "in the test") };
        // SAFETY: as above.
        unsafe { assert_eq!(gl.get_error(), glow::NO_ERROR) };
    }
}

#[cfg(test)]
mod surface_drop_tests {
    use super::*;
    use wayland_client::globals::{GlobalListContents, registry_queue_init};
    use wayland_client::protocol::{wl_compositor::WlCompositor, wl_registry::WlRegistry};
    use wayland_client::{Connection, Dispatch, QueueHandle, delegate_noop};

    struct St;
    impl Dispatch<WlRegistry, GlobalListContents> for St {
        fn event(
            _: &mut Self,
            _: &WlRegistry,
            _: wayland_client::protocol::wl_registry::Event,
            _: &GlobalListContents,
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }
    delegate_noop!(St: WlCompositor);
    delegate_noop!(St: ignore WlSurface);

    /// A window closed mid-fade (a panel dropped while the theme crossfades) still holds its
    /// window-sized `fade`/`fresh` targets. `Surface::drop` must queue them in `Gpu::dead` for
    /// `free_dead` (run by `trim`/`render`); each session-screen open/close used to leak a
    /// 1920x1080 texture (+8 MB RssAnon on llvmpipe).
    #[test]
    fn dropped_surface_frees_its_window_textures() {
        let Ok(conn) = Connection::connect_to_env() else {
            return skip_without("Wayland compositor");
        };
        // SAFETY: `conn` outlives `g`.
        let Some(mut g) = (unsafe { Gpu::new(conn.backend().display_ptr() as *mut _) }) else {
            return skip_without("EGL/GLES2 context on the Wayland display");
        };
        let (globals, queue) = registry_queue_init::<St>(&conn).unwrap();
        let qh = queue.handle();
        let comp: WlCompositor = globals.bind(&qh, 1..=4, ()).unwrap();
        let (w, h) = (64, 48);
        let wl = comp.create_surface(&qh, ());
        let mut surf = g.surface(&wl, w as i32, h as i32).expect("egl surface");
        let mut fr = Frame::new(w, h);
        assert!(g.render(&mut surf, &mut fr, || {}));
        g.begin_fade(&mut surf, &fr, std::time::Duration::from_secs(10), None);
        assert!(g.render(&mut surf, &mut fr, || {}));
        let fade = surf.fade.as_ref().map(|f| (f.tex, f.fb));
        let targets: Vec<_> = fade.into_iter().chain(surf.fresh).collect();
        assert_eq!(
            targets.len(),
            2,
            "a fading window holds its fade and fresh targets"
        );
        let alive = |g: &Gpu| {
            targets.iter().any(|&(t, f)| {
                // SAFETY: `g` made its context current when created or rendering, and this test binds no other.
                let tex = unsafe { g.gl.is_texture(t) };
                // SAFETY: as above.
                tex || unsafe { g.gl.is_framebuffer(f) }
            })
        };
        assert!(alive(&g));
        drop(surf);
        wl.destroy();
        assert_eq!(
            g.dead.borrow().surfaces.len(),
            1,
            "dropped surface queued in Gpu::dead"
        );
        g.trim(60);
        assert!(g.dead.borrow().targets.is_empty(), "trim runs free_dead");
        assert!(!alive(&g), "closed window's fade textures are still alive");
    }

    /// `surface` used to return early when eglMakeCurrent failed, leaking the new EGLSurface
    /// and destroying the wl_egl_window it was still created on.
    #[test]
    fn failed_make_current_destroys_the_window_surface() {
        let Ok(conn) = Connection::connect_to_env() else {
            return skip_without("Wayland compositor");
        };
        // SAFETY: `conn` outlives `g`.
        let Some(mut g) = (unsafe { Gpu::new(conn.backend().display_ptr() as *mut _) }) else {
            return skip_without("EGL/GLES2 context on the Wayland display");
        };
        let (globals, queue) = registry_queue_init::<St>(&conn).unwrap();
        let qh = queue.handle();
        let comp: WlCompositor = globals.bind(&qh, 1..=4, ()).unwrap();
        let wl = comp.create_surface(&qh, ());
        g.make_current_fails = true;
        assert!(g.surface(&wl, 64, 48).is_none());
        let dead = std::mem::take(&mut g.dead.borrow_mut().surfaces);
        assert_eq!(
            dead.len(),
            1,
            "the unbound surface was dropped as a Surface"
        );
        // SAFETY: only queried, and EGL validates the handle (EGL_BAD_SURFACE once destroyed).
        let es = unsafe { egl::Surface::from_ptr(dead[0] as *mut c_void) };
        assert_eq!(
            g.egl.query_surface(g.display, es, egl::WIDTH),
            Err(egl::Error::BadSurface),
            "the EGLSurface is destroyed"
        );
        g.make_current_fails = false;
        let mut surf = g
            .surface(&wl, 64, 48)
            .expect("the wl_surface takes a new EGL window");
        assert!(g.render(&mut surf, &mut Frame::new(64, 48), || {}));
        drop(surf);
        wl.destroy();
    }
}

#[cfg(test)]
mod gl_state_tests {
    use super::*;

    fn gpu(w: i32, h: i32) -> Option<(Gpu, Surface)> {
        let Ok((g, _)) = Gpu::headless("surfaceless") else {
            assert!(
                std::env::var_os("RI_REQUIRE_GL").is_none(),
                "no surfaceless EGL/GLES2 context (RI_REQUIRE_GL is set)"
            );
            return None;
        };
        let s = g.pbuffer(w, h).expect("pbuffer");
        Some((g, s))
    }

    fn shot(g: &mut Gpu, s: &mut Surface, mut fr: Frame) -> Canvas {
        s.reswap();
        g.render(s, &mut fr, || {});
        g.read_surface(s)
    }

    /// Every texture filter the state cache believes in is the one GL has: a colour overlay
    /// samples its layer smoothly through the cache, so a later nearest draw of the same
    /// layer texture is not skipped as "already set".
    #[test]
    fn filter_cache_matches_gl_after_tint() {
        let Some((mut g, mut s)) = gpu(64, 32) else {
            return;
        };
        let mut fr = Frame::new(64, 32);
        let mut src = Frame::new(16, 16);
        let grad = {
            let mut c = Canvas::new(4, 4, [0; 4]);
            for (k, p) in c.px.iter_mut().enumerate() {
                *p = [(k * 16) as u8, 0, 255 - (k * 16) as u8, 255];
            }
            Rc::new(c)
        };
        src.image(
            grad,
            [0.0, 0.0, 16.0, 16.0],
            [0.0, 0.0, 1.0, 1.0],
            1.0,
            false,
        );
        fr.tint(
            src,
            [16.0, 16.0],
            [0, 65535, 0, 32896],
            None,
            true,
            [4.0, 4.0, 40.0, 20.0],
            1.0,
        );
        shot(&mut g, &mut s, fr);
        let filters = g.st.borrow().filters.clone();
        assert!(!filters.is_empty());
        for (t, f) in filters {
            // SAFETY: `g` made its context current when created or rendering, and this test binds no other.
            unsafe { g.gl.bind_texture(glow::TEXTURE_2D, Some(t)) };
            for p in [glow::TEXTURE_MIN_FILTER, glow::TEXTURE_MAG_FILTER] {
                // SAFETY: `g` made its context current when created or rendering, and this test binds no other.
                unsafe { assert_eq!(g.gl.get_tex_parameter_i32(glow::TEXTURE_2D, p), f, "{t:?}") };
            }
        }
    }

    /// An image without pixels (a dmabuf frame whose texture is gone) draws nothing instead of
    /// whatever the scratch texture last held.
    #[test]
    fn pixelless_image_draws_nothing() {
        let Some((mut g, mut s)) = gpu(64, 32) else {
            return;
        };
        let mut fr = Frame::new(64, 32);
        fr.image(
            Rc::new(Canvas::new(4, 4, [0, 255, 0, 255])),
            [0.0, 0.0, 16.0, 16.0],
            [0.0, 0.0, 1.0, 1.0],
            1.0,
            false,
        );
        let gone = Rc::new(Canvas {
            w: 4,
            h: 4,
            px: Vec::new(),
            key: Default::default(),
        });
        fr.image(
            gone,
            [32.0, 0.0, 16.0, 16.0],
            [0.0, 0.0, 1.0, 1.0],
            1.0,
            false,
        );
        let out = shot(&mut g, &mut s, fr);
        assert_eq!(out.px[8 * 64 + 8], [0, 255, 0, 255], "image drawn");
        assert!(
            (0..16).all(|y| (32..48).all(|x| out.px[y * 64 + x] == [0; 4])),
            "pixel-less image sampled stale texture"
        );
    }

    /// Triangles that change in place (same vertex count) reach the GL buffer: the frame
    /// matches one rendered by a fresh context.
    #[test]
    fn changed_triangles_update_their_buffer() {
        let Some((mut g, mut s)) = gpu(64, 32) else {
            return;
        };
        let geo = Rc::new(qsg::rect::build(&qsg::rect::RectParams {
            width: 40.0,
            height: 20.0,
            color: [200, 40, 90, 255],
            radius: 6.0,
            antialiasing: true,
            ..Default::default()
        }));
        let frame = |op: f32| {
            let mut fr = Frame::new(64, 32);
            fr.smooth(&geo, 8.0, 6.0, op);
            fr
        };
        shot(&mut g, &mut s, frame(1.0));
        let half = shot(&mut g, &mut s, frame(0.5));
        let (mut f, mut fs) = gpu(64, 32).expect("second context");
        let want = shot(&mut f, &mut fs, frame(0.5));
        assert!(want.px.iter().any(|p| p[3] != 0), "drew nothing");
        assert_eq!(half.px, want.px);
    }

    /// A blur that gets no target for one of its steps (GL out of memory) draws nothing and
    /// leaves no target behind once its layers are trimmed: the first pass's target is given
    /// back when the second pass has none to draw into.
    #[test]
    fn blur_short_of_targets_leaves_none_behind() {
        let Some((mut g, mut s)) = gpu(64, 32) else {
            return;
        };
        let (taps, spread, sum) = qsg::blur::kernel(8.0, 17, None);
        let white = Rc::new(Canvas::new(4, 4, [255; 4]));
        let framebuffers = |g: &Gpu| {
            (1..=64)
                .filter_map(std::num::NonZeroU32::new)
                // SAFETY: `g` made its context current when created or rendering, and this test binds no other.
                .filter(|&n| unsafe { g.gl.is_framebuffer(glow::NativeFramebuffer(n)) })
                .count()
        };
        // The source layer, the horizontal pass and the vertical pass each take a new target.
        for (made, drawn) in [(3, true), (2, false), (1, false), (0, false)] {
            let mut src = Frame::new(16, 16);
            src.image(
                white.clone(),
                [4.0, 4.0, 8.0, 8.0],
                [0.0, 0.0, 1.0, 1.0],
                1.0,
                false,
            );
            let blur = Gauss {
                taps: taps.clone(),
                spread,
                sum,
                per: [16.0; 2],
                band: [0.0, 1.0],
                shadow: None,
                first: None,
            };
            let mut fr = Frame::new(64, 32);
            fr.gauss(src, blur, (16, 16), [8.0, 8.0, 16.0, 16.0], None, 1.0);
            g.targets_left.set(Some(made));
            let px = shot(&mut g, &mut s, fr).px[16 * 64 + 16];
            assert_eq!(
                g.targets_left.take(),
                Some(0),
                "{made} targets: all asked for"
            );
            assert_eq!(px != [0; 4], drawn, "{made} targets: {px:?}");
            assert_eq!(framebuffers(&g) != 0, made != 0, "{made} targets: seen");
            shot(&mut g, &mut s, Frame::new(64, 32));
            g.trim(0);
            assert_eq!(framebuffers(&g), 0, "{made} targets: framebuffers left");
        }
    }

    /// A layer and a blur past the texture size limit are each said once in the log, with the
    /// size asked for and the size they are drawn at: not again by a later frame, also one
    /// that draws them anew because what is in them changed.
    #[test]
    fn layer_and_blur_past_the_texture_size_limit_are_said_once() {
        #[derive(Clone, Default)]
        struct Log(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
        impl std::io::Write for Log {
            fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(b);
                Ok(b.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let Some((mut g, mut s)) = gpu(64, 32) else {
            return;
        };
        g.cap_texture_size(0);
        let m = g.max_tex;
        let (taps, spread, sum) = qsg::blur::kernel(8.0, 17, None);
        let log = Log::default();
        let to = log.clone();
        let sub = qml::debug::subscriber(qml::debug::filter(None), move || to.clone());
        tracing::subscriber::with_default(sub, || {
            for (frame, colour) in [[255; 4], [255; 4], [0, 255, 0, 255]]
                .into_iter()
                .enumerate()
            {
                let pic = Rc::new(Canvas::new(4, 4, colour));
                let mut layer = Frame::new(m + 1, 2);
                layer.image(
                    pic.clone(),
                    [0.0, 0.0, (m + 1) as f64, 2.0],
                    [0.0, 0.0, 1.0, 1.0],
                    1.0,
                    false,
                );
                let mut src = Frame::new(16, 16);
                src.image(
                    pic,
                    [0.0, 0.0, 16.0, 16.0],
                    [0.0, 0.0, 1.0, 1.0],
                    1.0,
                    false,
                );
                let blur = Gauss {
                    taps: taps.clone(),
                    spread,
                    sum,
                    per: [16.0; 2],
                    band: [0.0, 1.0],
                    shadow: None,
                    first: None,
                };
                let mut fr = Frame::new(64, 32);
                fr.layer(layer, [64.0 / (m + 1) as f64, 0.0, 0.0, 8.0, 0.0, 0.0], 1.0);
                fr.gauss(
                    src,
                    blur,
                    (m + 2, 16),
                    [0.0, 16.0, (m + 2) as f64, 16.0],
                    None,
                    1.0,
                );
                let shot = shot(&mut g, &mut s, fr);
                assert_eq!(shot.px[4 * 64 + 32], colour, "frame {frame}: the layer");
                assert_ne!(shot.px[24 * 64 + 2], [0; 4], "frame {frame}: the blur");
            }
        });
        let log = String::from_utf8(log.0.lock().unwrap().clone()).unwrap();
        assert_eq!(
            log,
            format!(
                "layer of {}x2 is past the texture size limit {m}: drawn at {m}x2\n\
                 blur of {}x16 is past the texture size limit {m}: drawn at {m}x16\n",
                m + 1,
                m + 2
            )
        );
    }

    /// Shadows of one kernel share one pass program whatever their colour and thickness
    /// (both are uniforms), and each still comes out in its own colour.
    #[test]
    fn shadow_colours_share_one_blur_program() {
        let Some((mut g, mut s)) = gpu(64, 32) else {
            return;
        };
        let (taps, spread, sum) = qsg::blur::kernel(8.0, 17, None);
        let white = Rc::new(Canvas::new(4, 4, [255; 4]));
        let mut frame = |colour: [f32; 4], thickness: f32| {
            let mut src = Frame::new(16, 16);
            src.image(
                white.clone(),
                [4.0, 4.0, 8.0, 8.0],
                [0.0, 0.0, 1.0, 1.0],
                1.0,
                false,
            );
            let mut fr = Frame::new(64, 32);
            let shadow = Gauss {
                taps: taps.clone(),
                spread,
                sum,
                per: [16.0; 2],
                band: [0.0, 1.0],
                shadow: Some((colour, thickness)),
                first: None,
            };
            fr.gauss(src, shadow, (16, 16), [8.0, 8.0, 16.0, 16.0], None, 1.0);
            shot(&mut g, &mut s, fr).px[16 * 64 + 16]
        };
        for k in 0..40 {
            let v = k as f32 / 40.0;
            frame([v, 1.0 - v, 0.5, 1.0], 0.2 + v);
        }
        let (red, blue) = (
            frame([1.0, 0.0, 0.0, 1.0], 0.1),
            frame([0.0, 0.0, 1.0, 1.0], 0.1),
        );
        assert_eq!((red, blue), ([255, 0, 0, 255], [0, 0, 255, 255]));
        assert_eq!(g.gauss.len(), 1, "one program per shadow colour");
        let mut fr = Frame::new(64, 32);
        let blur = Gauss {
            taps,
            spread,
            sum,
            per: [16.0; 2],
            band: [0.0, 1.0],
            shadow: None,
            first: None,
        };
        fr.gauss(
            Frame::new(16, 16),
            blur,
            (16, 16),
            [8.0, 8.0, 16.0, 16.0],
            None,
            1.0,
        );
        shot(&mut g, &mut s, fr);
        assert_eq!(g.gauss.len(), 2, "a blur is not a shadow");
    }

    /// Fragment shaders take their float precision from FS_PRECISION only, so they compile
    /// where GLSL ES 1.00 fragment highp is unsupported (§4.5.4).
    #[test]
    fn fragment_shaders_leave_highp_to_the_prefix() {
        let gauss = |shadow| {
            let (taps, spread, sum) = qsg::blur::kernel(8.0, 17, None);
            Gauss {
                taps,
                spread,
                sum,
                per: [16.0; 2],
                band: [0.0, 1.0],
                shadow,
                first: None,
            }
            .shader(usize::MAX)
        };
        let (blur, shadow) = (gauss(None), gauss(Some(([0.0, 0.0, 0.0, 1.0], 1.0))));
        // More than BAKED_TAPS taps: the shader that reads them from a uniform array.
        let (taps, spread, sum) = qsg::blur::kernel(120.0, 241, None);
        let looped = Gauss {
            taps,
            spread,
            sum,
            per: [16.0; 2],
            band: [0.0, 1.0],
            shadow: Some(([0.0, 0.0, 0.0, 1.0], 1.0)),
            first: None,
        }
        .shader(usize::MAX);
        assert!(looped.contains("uniform vec2 w[241];"), "{looped}");
        for fs in [
            COLOR_FS, TEX_FS, EFFECT_FS, BLUR_FS, FADE_FS, DIFF_FS, RSHADOW_FS, RADIAL_FS, &blur,
            &shadow, &looped,
        ] {
            assert!(!fs.contains("highp"), "{fs}");
        }
        assert!(FS_PRECISION.contains("GL_FRAGMENT_PRECISION_HIGH"));
    }

    /// Windows get an RGBA8888 config with Qt's depth and stencil and no samples on Mesa.
    #[test]
    fn config_has_qts_depth_and_stencil_and_no_samples() {
        let Some(egl) = load_egl() else {
            return skip_without("libEGL.so.1");
        };
        let Ok((d, _)) = headless_display(&egl, "surfaceless") else {
            return skip_without("surfaceless EGL display");
        };
        egl.initialize(d).expect("eglInitialize");
        let attrs = [
            egl::SURFACE_TYPE,
            egl::PBUFFER_BIT,
            egl::RED_SIZE,
            8,
            egl::GREEN_SIZE,
            8,
            egl::BLUE_SIZE,
            8,
            egl::ALPHA_SIZE,
            8,
            egl::RENDERABLE_TYPE,
            egl::OPENGL_ES2_BIT,
            egl::DEPTH_SIZE,
            0,
            egl::STENCIL_SIZE,
            0,
            egl::NONE,
        ];
        let Some(c) = choose_config(&egl, d, &attrs) else {
            return skip_without("RGBA8888 GLES2 pbuffer config");
        };
        for (a, v) in [
            (egl::RED_SIZE, 8),
            (egl::ALPHA_SIZE, 8),
            (egl::DEPTH_SIZE, 24),
            (egl::STENCIL_SIZE, 8),
            (egl::SAMPLES, 0),
        ] {
            assert_eq!(egl.get_config_attrib(d, c, a).unwrap(), v, "{a:#x}");
        }
    }
}

#[cfg(test)]
mod display_list_tests {
    use super::*;

    fn rect(w: f32, h: f32) -> Rc<Geometry> {
        Rc::new(qsg::rect::build(&qsg::rect::RectParams {
            width: w,
            height: h,
            color: [255, 0, 0, 255],
            ..Default::default()
        }))
    }

    fn vertices(f: &Frame) -> Vec<Vec<f32>> {
        f.cmds
            .iter()
            .filter_map(|c| match c {
                Cmd::Tris(r) => Some(r.iter().flat_map(|r| r.iter().copied()).collect()),
                _ => None,
            })
            .collect()
    }

    /// Recording a subtree and replaying it share its vertex runs and rasters (no copies) and
    /// draw exactly what painting it again does, in one batch.
    #[test]
    fn replayed_subtrees_share_their_draws() {
        let g = rect(10.0, 10.0);
        let paint = |f: &mut Frame, sub: Option<&Seg>, img: &Canvas| -> Option<Seg> {
            f.smooth(&g, 0.0, 0.0, 1.0);
            let st = f.begin();
            let seg = match sub {
                Some(s) => {
                    f.replay(s);
                    None
                }
                None => {
                    f.smooth(&g, 20.0, 0.0, 1.0);
                    f.smooth(&g, 40.0, 0.0, 1.0);
                    f.blit(img.clone(), 5, 5);
                    f.smooth(&g, 50.0, 0.0, 1.0);
                    f.record(&st)
                }
            };
            f.smooth(&g, 60.0, 0.0, 1.0);
            seg
        };
        let img = Canvas::new(2, 2, [9, 9, 9, 255]);
        let mut fresh = Frame::new(100, 100);
        let seg = paint(&mut fresh, None, &img).expect("recorded");
        let mut again = Frame::new(100, 100);
        paint(&mut again, Some(&seg), &img);
        assert_eq!(vertices(&again), vertices(&fresh));
        assert_eq!(vertices(&fresh).len(), 2, "tris, blit, tris");
        assert_eq!(again.sig, fresh.sig);
        let (lead, _) = seg.lead.as_ref().expect("subtree started in an open batch");
        for f in [&fresh, &again] {
            let Some(Cmd::Tris(runs)) = f.cmds.first() else {
                panic!("no batch")
            };
            assert!(lead.iter().all(|l| runs.iter().any(|r| Rc::ptr_eq(l, r))));
            let blit = |f: &Frame| {
                f.cmds.iter().find_map(|c| match c {
                    Cmd::Blit(c, ..) => Some(c.clone()),
                    _ => None,
                })
            };
            assert!(Rc::ptr_eq(
                &blit(f).unwrap(),
                &blit(&seg_frame(&seg)).unwrap()
            ));
        }
    }

    /// A recorded subtree redrawn moved (`replay_at`) has the vertices painting it there has:
    /// the places of the items inside it are added up again from where its item is now, the
    /// way Qt's renderer adds them, which is not the old sum moved.
    #[test]
    fn a_moved_replay_adds_the_places_up_again() {
        let g = Rc::new(qsg::rect::build(&qsg::rect::RectParams {
            width: 36.3,
            height: 24.6,
            radius: 6.0,
            color: [255, 0, 0, 255],
            ..Default::default()
        }));
        // An item at `at` holding a clipping item with a Rectangle in it, and a Rectangle.
        let paint = |f: &mut Frame, at: [f64; 2], sub: Option<(&Seg, [f64; 2])>, placed: bool| {
            let place = |f: &mut Frame, origin: [f64; 2], own: [f64; 2], clip: bool| {
                if placed {
                    f.push_place(origin, own, clip);
                }
                [origin[0] + own[0], origin[1] + own[1]]
            };
            let item = place(f, [0.0; 2], at, false);
            let st = f.begin();
            let seg = match sub {
                Some((s, d)) => {
                    assert!(f.replay_at(s, d[0], d[1]));
                    None
                }
                None => {
                    let c = place(f, item, [7.1, 3.3], true);
                    f.push_clip_f([c[0], c[1], 70.0, 50.0]);
                    let r = place(f, c, [4.7, 5.3], false);
                    f.smooth(&g, r[0], r[1], 1.0);
                    f.pop_place();
                    f.pop_clip();
                    f.pop_place();
                    let r = place(f, item, [0.3, 60.7], false);
                    f.smooth(&g, r[0], r[1], 1.0);
                    f.pop_place();
                    f.record(&st)
                }
            };
            f.pop_place();
            seg
        };
        let (from, to) = ([3.3, 2.7], [13.9, 8.2]);
        let mut first = Frame::new(200, 200);
        let seg = paint(&mut first, from, None, true).expect("recorded");
        assert!(seg.translatable());
        let mut fresh = Frame::new(200, 200);
        paint(&mut fresh, to, None, true);
        let mut again = Frame::new(200, 200);
        paint(
            &mut again,
            to,
            Some((&seg, [to[0] - from[0], to[1] - from[1]])),
            true,
        );
        assert_eq!(vertices(&again), vertices(&fresh));
        assert_eq!(again.sig, fresh.sig);
        assert!(again.places.is_empty() && fresh.places.is_empty());
        // The places are what makes these vertices: the positions summed in doubles are not.
        let mut summed = Frame::new(200, 200);
        paint(&mut summed, to, None, false);
        assert_ne!(vertices(&summed), vertices(&fresh));
        // An item that draws nothing leaves nothing to replay.
        let mut empty = Frame::new(200, 200);
        let st = empty.begin();
        empty.push_place([0.0; 2], [1.5, 2.5], true);
        empty.pop_place();
        assert!(empty.record(&st).expect("balanced").ops.unwrap().is_empty());
    }

    /// The frame that goes on after `take_recorded` has the items being drawn where they were.
    #[test]
    fn a_frame_whose_draws_were_taken_goes_on_in_the_same_places() {
        let g = Rc::new(qsg::rect::build(&qsg::rect::RectParams {
            width: 36.3,
            height: 24.6,
            radius: 6.0,
            color: [255, 0, 0, 255],
            ..Default::default()
        }));
        let paint = |f: &mut Frame, take: bool| {
            f.push_place([0.0; 2], [3.3, 2.7], false);
            f.push_place([3.3, 2.7], [7.1, 3.3], true);
            f.smooth(&g, 10.4, 6.0, 1.0);
            let taken = if take { f.take_recorded() } else { None };
            f.push_place([10.4, 6.0], [4.7, 5.3], false);
            f.smooth(&g, 15.1, 11.3, 1.0);
            for _ in 0..3 {
                f.pop_place();
            }
            assert!(f.places.is_empty());
            taken
        };
        let mut whole = Frame::new(200, 200);
        paint(&mut whole, false);
        let mut rest = Frame::new(200, 200);
        let taken = paint(&mut rest, true).expect("something was drawn");
        assert_eq!(
            [vertices(&taken), vertices(&rest)].concat().concat(),
            vertices(&whole).concat()
        );
    }

    /// Places add up item by item in floats, and an item that clips takes the sum so far
    /// into its root's matrix: neither is the sum in doubles rounded once.
    #[test]
    fn places_add_up_as_qt_adds_them() {
        let p = Place::origin([0.0; 2])
            .step([0.0; 2], [3.3, 2.7], false)
            .step([0.0; 2], [9.5, 9.4], false);
        assert_eq!(p.rel, [3.3f32 + 9.5f32, 2.7f32 + 9.4f32]);
        assert_eq!(p.root, [0.0; 2]);
        assert_eq!(p.at, [3.3 + 9.5, 2.7 + 9.4]);
        assert_ne!(p.rel[1], (2.7 + 9.4) as f32);
        assert!(p.is_at(p.at) && !p.is_at([p.at[0] + 0.01, p.at[1]]));
        // A node between the two (a Flickable's contentItem) is one more float added.
        let q = Place::origin([0.0; 2])
            .step([0.0; 2], [3.3, 2.7], true)
            .step([-1.3, 0.0], [9.5, 9.4], false);
        assert_eq!(q.root, [3.3f32, 2.7f32]);
        assert_eq!(q.rel, [-1.3f32 + 9.5f32, 9.4f32]);
        // Under a layer's padded source rect Qt's root is not at the target's corner.
        let l = Place::origin([-2.0, -2.0]).step([0.0; 2], [2.0, 2.0], false);
        assert_eq!((l.rel, l.at), ([2.0; 2], [0.0; 2]));
    }

    fn seg_frame(s: &Seg) -> Frame {
        let mut f = Frame::new(100, 100);
        f.replay(s);
        f
    }

    /// A canvas allocated where a dropped one lived is a different texture to every cache.
    #[test]
    fn reused_address_signs_differently() {
        let sign = |c: Rc<Canvas>| {
            let mut f = Frame::new(4, 4);
            f.image(c, [0.0, 0.0, 4.0, 4.0], [0.0, 0.0, 1.0, 1.0], 1.0, false);
            f.sig[0].0
        };
        let mut seen = std::collections::HashSet::new();
        for k in 0..64u8 {
            let c = Rc::new(Canvas::new(4, 4, [k; 4]));
            assert!(seen.insert(sign(c)), "signature reused");
        }
        let c = Canvas::new(1, 1, [0; 4]);
        assert_ne!(c.clone().key, c.key, "a clone is another buffer");
    }

    /// Fresh rasters sign by content: equal pixels sign alike, any changed pixel (in a lane or
    /// in the tail) or another size signs differently.
    #[test]
    fn raster_content_hash() {
        let c = Canvas::new(13, 3, [1, 2, 3, 4]);
        assert_eq!(mix_px(8, &c), mix_px(8, &c.clone()));
        assert_ne!(mix_px(8, &c), mix_px(8, &Canvas::new(3, 13, [1, 2, 3, 4])));
        for i in 0..c.px.len() {
            let mut d = c.clone();
            d.px[i][i % 4] ^= 1;
            assert_ne!(mix_px(8, &c), mix_px(8, &d), "pixel {i}");
        }
    }

    /// The damage covers every pixel whose stack of primitives differs between two frames.
    #[test]
    fn diff_covers_every_restacked_pixel() {
        fn rnd(s: &mut u64, n: u64) -> u64 {
            *s ^= *s << 13;
            *s ^= *s >> 7;
            *s ^= *s << 17;
            *s % n
        }
        fn pick(s: &mut u64, pool: &[(u64, [i64; 4])], n: u64) -> Vec<(u64, [i64; 4])> {
            (0..rnd(s, n)).map(|_| pool[rnd(s, 6) as usize]).collect()
        }
        let s = &mut 0x2545_f491_4f6c_dd1du64;
        let full = [0, 0, 24, 4];
        for _ in 0..4000 {
            let pool: Vec<(u64, [i64; 4])> = (0..6)
                .map(|k| {
                    let x = rnd(s, 20) as i64;
                    (k, [x, 0, x + 1 + rnd(s, 4) as i64, 4])
                })
                .collect();
            let (pre, suf) = (pick(s, &pool, 5), pick(s, &pool, 5));
            let a = pick(s, &pool, 6);
            let mut b = a.clone();
            match rnd(s, 3) {
                0 if b.len() > 1 => {
                    let (i, j) = (
                        rnd(s, b.len() as u64) as usize,
                        rnd(s, b.len() as u64) as usize,
                    );
                    b.swap(i, j);
                }
                1 => b = pick(s, &pool, 6),
                _ => {}
            }
            let prev = [&pre[..], &a, &suf].concat();
            let cur = [&pre[..], &b, &suf].concat();
            let d = diff(&prev, &cur, full);
            let stack = |l: &[(u64, [i64; 4])], x: i64| -> Vec<u64> {
                l.iter()
                    .filter(|e| e.1[0] <= x && x < e.1[2])
                    .map(|e| e.0)
                    .collect()
            };
            for x in 0..24 {
                if stack(&prev, x) != stack(&cur, x) {
                    let d = d
                        .as_ref()
                        .unwrap_or_else(|| panic!("no damage: {prev:?} -> {cur:?}"));
                    assert!(
                        d.iter().any(|r| r[0] <= x && x < r[2]),
                        "{x} outside {d:?}: {prev:?} -> {cur:?}"
                    );
                }
            }
            assert_eq!(diff(&prev, &prev, full), None);
        }
    }
}

#[cfg(test)]
mod init_failure_tests {
    use super::*;

    /// A context `on_display` gives up on is released and destroyed, not left current.
    #[test]
    fn abandoned_new_context_is_released_and_destroyed() {
        let Ok((g, _)) = Gpu::headless("surfaceless") else {
            assert!(
                std::env::var_os("RI_REQUIRE_GL").is_none(),
                "no surfaceless EGL/GLES2"
            );
            return;
        };
        let attrs = [egl::CONTEXT_CLIENT_VERSION, 2, egl::NONE];
        let ctx = g
            .egl
            .create_context(g.display, g.config, None, &attrs)
            .expect("context");
        let guard = NewContext {
            egl: g.egl.clone(),
            display: g.display,
            ctx: Some(ctx),
        };
        g.egl
            .make_current(g.display, None, None, Some(ctx))
            .expect("current");
        drop(guard);
        assert!(g.egl.get_current_context().is_none(), "left current");
        assert!(
            g.egl
                .make_current(g.display, None, None, Some(ctx))
                .is_err(),
            "not destroyed"
        );
    }
}

/// What a `Gpu` takes from its display and context: extension functions only where their
/// extension is advertised, a blur shader that fits the fragment uniforms, and the context
/// itself, made current by every safe `pub fn` that issues GL.
#[cfg(test)]
mod capability_tests {
    use super::*;

    fn gpu() -> Option<Gpu> {
        match Gpu::headless("surfaceless") {
            Ok((g, _)) => Some(g),
            Err(_) => {
                skip_without("surfaceless EGL/GLES2 context");
                None
            }
        }
    }

    fn current(g: &Gpu) {
        g.egl
            .make_current(g.display, None, None, Some(g.ctx))
            .expect("eglMakeCurrent");
    }

    fn is_current(g: &Gpu) -> bool {
        g.egl.get_current_context() == Some(g.ctx)
    }

    fn display_exts(g: &Gpu) -> String {
        g.egl
            .query_string(Some(g.display), egl::EXTENSIONS)
            .expect("EGL_EXTENSIONS")
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn swap_with_damage_function_follows_the_extension_string() {
        const KHR: &str = "eglSwapBuffersWithDamageKHR";
        const EXT: &str = "eglSwapBuffersWithDamageEXT";
        for (exts, want) in [
            (
                "EGL_EXT_buffer_age EGL_KHR_swap_buffers_with_damage",
                Some(KHR),
            ),
            (
                "EGL_EXT_swap_buffers_with_damage EGL_EXT_buffer_age",
                Some(EXT),
            ),
            (
                "EGL_EXT_swap_buffers_with_damage EGL_KHR_swap_buffers_with_damage",
                Some(KHR),
            ),
            // A whole name, not a prefix of one.
            ("EGL_KHR_swap_buffers_with_damage2 EGL_EXT_buffer_age", None),
            ("EGL_EXT_buffer_age", None),
            ("", None),
        ] {
            assert_eq!(swap_damage_name(exts), want, "{exts:?}");
        }
    }

    /// Mesa's eglGetProcAddress answers for eglSwapBuffersWithDamage{KHR,EXT} on every
    /// display, also on one (surfaceless) whose extension string names neither: the `Gpu` has
    /// the function exactly when the string names its extension.
    #[test]
    fn swap_with_damage_is_taken_only_where_the_display_advertises_it() {
        let Some(g) = gpu() else { return };
        let exts = display_exts(&g);
        let advertised = exts.split(' ').any(|e| {
            e == "EGL_KHR_swap_buffers_with_damage" || e == "EGL_EXT_swap_buffers_with_damage"
        });
        assert_eq!(
            g.swap_damage.is_some(),
            advertised && !qml::debug::on(qml::debug::Switch::NODAMAGE),
            "{exts}"
        );
    }

    #[test]
    fn dmabuf_import_needs_all_three_extensions() {
        let gl = |names: &[&str]| -> std::collections::HashSet<String> {
            names.iter().map(|n| n.to_string()).collect()
        };
        let egl = "EGL_KHR_image_base EGL_EXT_image_dma_buf_import EGL_KHR_fence_sync";
        assert!(dmabuf_import_exts(egl, &gl(&["GL_OES_EGL_image"])));
        assert!(!dmabuf_import_exts(
            egl,
            &gl(&["GL_OES_EGL_image_external"])
        ));
        for egl in [
            "EGL_KHR_image_base EGL_EXT_image_dma_buf_import_modifiers",
            "EGL_KHR_image EGL_EXT_image_dma_buf_import",
            "EGL_EXT_image_dma_buf_import",
            "EGL_KHR_image_base",
            "",
        ] {
            assert!(
                !dmabuf_import_exts(egl, &gl(&["GL_OES_EGL_image"])),
                "{egl:?}"
            );
        }
    }

    /// The `Gpu` holds the EGLImage functions exactly when its display and context advertise
    /// their extensions, and without them an import is refused before any of them is called.
    #[test]
    fn dmabuf_functions_are_taken_only_where_advertised() {
        let Some(mut g) = gpu() else { return };
        let exts = display_exts(&g);
        let advertised = dmabuf_import_exts(&exts, g.gl.supported_extensions());
        assert_eq!(g.image.is_some(), advertised, "{exts}");
        g.image = None;
        let fd = std::fs::File::open("/dev/null").expect("/dev/null");
        use std::os::fd::AsRawFd;
        assert!(
            g.import_dmabuf(&[(fd.as_raw_fd(), 0, 16)], 4, 4, 0x34325258, 0)
                .is_none()
        );
    }

    /// GLES 2.0 guarantees 16 fragment uniform vectors, counted as `LOOP_UNIFORMS` counts them
    /// (one per uniform, a sampler too): EFFECT_FS, whose shadow added to them, declares no
    /// more, or its program would not link where there are only 16.
    #[test]
    fn effect_shader_fits_the_least_fragment_uniforms() {
        let uniforms = EFFECT_FS.matches("uniform ").count();
        assert!(
            !EFFECT_FS.contains('['),
            "an array takes a vector per element"
        );
        assert_eq!(uniforms, 16, "{EFFECT_FS}");
    }

    /// GLES 2.0 guarantees 16 fragment uniform vectors: a kernel whose array does not fit the
    /// context's keeps its taps baked in the shader, as qgfxshaderbuilder writes them, and
    /// draws what the loop draws.
    #[test]
    fn blur_kernel_past_the_uniform_limit_is_baked() {
        let (taps, spread, sum) = qsg::blur::kernel(101.0, 203, None);
        assert_eq!(taps.len(), 203);
        let blur = Gauss {
            taps,
            spread,
            sum,
            per: [16.0; 2],
            band: [0.0, 1.0],
            shadow: None,
            first: None,
        };
        assert!(blur.looped(203 + LOOP_UNIFORMS) && !blur.looped(203 + LOOP_UNIFORMS - 1));
        assert!(blur.shader(4096).contains("uniform vec2 w[203];"));
        let baked = blur.shader(16);
        assert!(!baked.contains("uniform vec2 w["), "{baked}");
        assert_eq!(baked.matches("texture2D(").count(), 203);
        assert_ne!(blur.prog_key(16), blur.prog_key(4096));
        let (short, spread, sum) = qsg::blur::kernel(63.0, 127, None);
        assert_eq!(short.len(), BAKED_TAPS);
        let short = Gauss {
            taps: short,
            spread,
            sum,
            per: [16.0; 2],
            band: [0.0, 1.0],
            shadow: None,
            first: None,
        };
        assert!(!short.shader(4096).contains("uniform vec2 w["));

        let draw = |uniforms: Option<usize>| -> Option<(Canvas, bool)> {
            let mut g = gpu()?;
            assert!(g.max_frag_uniforms >= 16, "{}", g.max_frag_uniforms);
            if let Some(u) = uniforms {
                g.max_frag_uniforms = u;
            }
            let looped = blur.looped(g.max_frag_uniforms);
            let mut s = g.pbuffer(64, 32).expect("pbuffer");
            let mut src = Frame::new(64, 32);
            src.image(
                Rc::new(Canvas::new(4, 4, [255, 128, 0, 255])),
                [8.0, 4.0, 24.0, 20.0],
                [0.0, 0.0, 1.0, 1.0],
                1.0,
                false,
            );
            let mut fr = Frame::new(64, 32);
            fr.gauss(
                src,
                blur.clone(),
                (64, 32),
                [0.0, 0.0, 64.0, 32.0],
                None,
                1.0,
            );
            s.reswap();
            g.render(&mut s, &mut fr, || {});
            assert!(
                g.gauss.len() == 1 && g.gauss.values().all(Option::is_some),
                "the blur program of {} uniforms did not build",
                g.max_frag_uniforms
            );
            Some((g.read_surface(&s), looped))
        };
        let Some((own, looped)) = draw(None) else {
            return;
        };
        assert!(looped, "llvmpipe has room for 203 taps");
        let (baked, looped) = draw(Some(16)).expect("second context");
        assert!(!looped);
        assert!(baked.px.iter().any(|p| p[3] > 0), "nothing drawn");
        let worst = own
            .px
            .iter()
            .zip(&baked.px)
            .flat_map(|(a, b)| (0..4).map(move |i| a[i].abs_diff(b[i])))
            .max()
            .unwrap();
        assert!(worst <= 1, "baked and looped taps differ by {worst}");
    }

    /// The lock screen's wallpaper blur (Background.qml: GaussianBlur of `lock.blur.radius`,
    /// `samples: radius * 2 + 1`) draws with one texture lookup in a loop, at Config.qml's
    /// default radius and at every wider one: a lookup per tap is what llvmpipe compiled for
    /// seconds before the lock screen could show (see `BAKED_TAPS`).
    #[test]
    fn lock_screen_blur_shader_loops_over_its_taps() {
        let config = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../qml/modules/common/Config.qml"
        );
        let config = std::fs::read_to_string(config).expect("Config.qml");
        let radius = config
            .split_once("property JsonObject lock:")
            .and_then(|(_, lock)| lock.split_once("property JsonObject blur:"))
            .and_then(|(_, blur)| blur.split_once("property real radius:"))
            .and_then(|(_, r)| r.lines().next()?.trim().parse::<i32>().ok())
            .expect("options.lock.blur.radius");
        for radius in [radius, radius + 1, qsg::blur::MAX_BLUR_RADIUS] {
            let (taps, spread, sum) = qsg::blur::kernel(radius as f64, radius * 2 + 1, None);
            let n = taps.len();
            assert!(n > BAKED_TAPS, "radius {radius}: {n} taps");
            let blur = Gauss {
                taps,
                spread,
                sum,
                per: [16.0; 2],
                band: [0.0, 1.0],
                shadow: None,
                first: None,
            };
            let fs = blur.shader(n + LOOP_UNIFORMS);
            assert_eq!(fs.matches("texture2D(").count(), 1, "radius {radius}: {fs}");
            assert!(fs.contains(&format!("uniform vec2 w[{n}];")), "{fs}");
        }
        let Some(g) = gpu() else {
            return;
        };
        let n = qsg::blur::kernel(radius as f64, radius * 2 + 1, None)
            .0
            .len();
        assert!(
            n + LOOP_UNIFORMS <= g.max_frag_uniforms,
            "{n} taps do not fit {} fragment uniform vectors",
            g.max_frag_uniforms
        );
    }

    /// The lock screen's kernel (`kernel(100, 201)`) drawn by the loop and by the shader with
    /// its 201 taps baked, the one it had before `BAKED_TAPS` was lowered, over a picture
    /// whose every texel differs from its neighbours: no pixel differs by more than one level
    /// of 255 (llvmpipe: 110 of 24576 pixels by one level, the rest by none).
    /// tests/effects_qt.rs `lock_screen_blur` compares the loop's picture with Qt's.
    #[test]
    fn lock_screen_blur_loop_draws_what_its_baked_taps_drew() {
        let (taps, spread, sum) = qsg::blur::kernel(100.0, 201, None);
        let (w, h) = (256usize, 96usize);
        let blur = Gauss {
            taps,
            spread,
            sum,
            per: [w as f32, h as f32],
            band: [0.0, 1.0],
            shadow: None,
            first: None,
        };
        let mut pic = Canvas::new(64, 24, [0, 0, 0, 255]);
        for (i, p) in pic.px.iter_mut().enumerate() {
            let (x, y) = (i % 64, i / 64);
            *p = [(x * 37 + y * 11) as u8, (x * 4) as u8, (y * 10) as u8, 255];
        }
        let pic = Rc::new(pic);
        let (wf, hf) = (w as f64, h as f64);
        let draw = |uniforms: Option<usize>| -> Option<(Canvas, bool)> {
            let mut g = gpu()?;
            if let Some(u) = uniforms {
                g.max_frag_uniforms = u;
            }
            let looped = blur.looped(g.max_frag_uniforms);
            let mut s = g.pbuffer(w as i32, h as i32).expect("pbuffer");
            let mut src = Frame::new(w, h);
            src.image(
                pic.clone(),
                [0.0, 0.0, wf, hf],
                [0.0, 0.0, 1.0, 1.0],
                1.0,
                false,
            );
            let mut fr = Frame::new(w, h);
            fr.gauss(src, blur.clone(), (w, h), [0.0, 0.0, wf, hf], None, 1.0);
            s.reswap();
            g.render(&mut s, &mut fr, || {});
            assert!(
                g.gauss.len() == 1 && g.gauss.values().all(Option::is_some),
                "the blur program of {} uniforms did not build",
                g.max_frag_uniforms
            );
            Some((g.read_surface(&s), looped))
        };
        let Some((own, looped)) = draw(None) else {
            return;
        };
        assert!(looped, "llvmpipe has room for 201 taps");
        let (baked, looped) = draw(Some(16)).expect("second context");
        assert!(!looped);
        let colours: std::collections::HashSet<_> = own.px.iter().collect();
        assert!(
            colours.len() > 1000,
            "a picture of {} colours",
            colours.len()
        );
        let mut off = [0usize; 256];
        for (a, b) in own.px.iter().zip(&baked.px) {
            off[(0..4).map(|i| a[i].abs_diff(b[i])).max().unwrap() as usize] += 1;
        }
        let off: Vec<_> = off.iter().enumerate().filter(|o| *o.1 > 0).collect();
        elog::eprintln!("loop and baked taps, (levels apart, pixels): {off:?}");
        assert!(
            off.iter().all(|o| o.0 <= 1),
            "baked and looped taps, (levels apart, pixels): {off:?}"
        );
    }

    /// `(u8, u32)` has three padding bytes and `[(u8, u16); 2]` two: neither is `NoPadding`,
    /// so `as_bytes` does not take them. (The `Padded<_>` below is inferred only for a type
    /// with one of the two impls: this test stops compiling if such a type gets `NoPadding`.)
    #[test]
    fn as_bytes_takes_no_padded_type() {
        trait Padded<A> {
            fn check() {}
        }
        impl<T> Padded<()> for T {}
        impl<T: NoPadding> Padded<u8> for T {}
        <(u8, u32) as Padded<_>>::check();
        <[(u8, u16); 2] as Padded<_>>::check();
        <bool as Padded<_>>::check();
        assert_eq!(
            as_bytes(&[[1u8, 2, 3, 4], [5, 6, 7, 8]]),
            [1, 2, 3, 4, 5, 6, 7, 8]
        );
        assert_eq!(as_bytes(&[1.5f32, -2.0])[..4], 1.5f32.to_ne_bytes());
        assert_eq!(as_bytes::<f32>(&[]), [0u8; 0]);
    }

    /// Two `Gpu`s on one thread, the other one's context current before each call: every safe
    /// `pub fn` that issues GL makes its own context current first, so it reads and frees its
    /// own objects and leaves the other context's objects of the same names alone.
    #[test]
    fn safe_gl_calls_make_their_own_context_current() {
        let Some(mut a) = gpu() else { return };
        let Some(b) = gpu() else { return };
        let (w, h) = (64, 48);
        // The other context's objects, under the names `a`'s get below.
        current(&b);
        // SAFETY: `current` made `b`'s context current.
        let theirs: Vec<_> = (0..24)
            .map(|_| unsafe { b.target(w, h) }.expect("target"))
            .collect();
        current(&a);
        // SAFETY: `current` made `a`'s context current.
        let (t, ..) = unsafe { a.target(w, h) }.expect("target");
        let mut px: Vec<u8> = (0..w * h * 4).map(|i| (i * 7 % 251) as u8).collect();
        let up = |a: &Gpu, px: &[u8]| {
            current(a);
            // SAFETY: `current` made `a`'s context current.
            unsafe { a.gl.bind_texture(glow::TEXTURE_2D, Some(t)) };
            // SAFETY: as above; `px` has w * h RGBA pixels.
            unsafe {
                a.gl.tex_sub_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    0,
                    0,
                    w,
                    h,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(Some(px)),
                )
            };
        };
        up(&a, &px);

        current(&b);
        a.finish();
        assert!(is_current(&a), "finish");
        let mut copy = None;
        current(&b);
        assert_eq!(a.frame_compare(t, w, h, (w, h), &mut copy), Some(true));
        assert!(is_current(&a), "frame_compare");
        current(&b);
        assert_eq!(a.frame_compare(t, w, h, (w, h), &mut copy), None);
        current(&b);
        a.compare_submit();
        current(&b);
        assert_eq!(a.compare_read(), [false], "an unchanged frame");
        assert!(is_current(&a), "compare_read");
        px[4 * (w * 20 + 30) as usize] ^= 1;
        up(&a, &px);
        current(&b);
        assert_eq!(a.frame_compare(t, w, h, (w, h), &mut copy), None);
        current(&b);
        assert_eq!(a.compare_read(), [true], "one changed byte");
        current(&b);
        a.free_frame_copy(copy);
        assert!(is_current(&a), "free_frame_copy");
        current(&b);
        // No image to import: refused by EGL or for lack of the extensions, on `a` either way.
        assert!(a.import_dmabuf(&[], 4, 4, 0x34325258, 0).is_none());
        current(&b);
        a.free_dmabuf(t, 0);
        assert!(is_current(&a), "free_dmabuf");
        // SAFETY: `a`'s context is current (asserted above).
        let left = unsafe { a.gl.is_texture(t) };
        assert!(!left, "free_dmabuf frees its texture");
        // SAFETY: as above.
        assert_eq!(unsafe { a.gl.get_error() }, glow::NO_ERROR);

        current(&b);
        // SAFETY: `current` made `b`'s context current.
        assert_eq!(unsafe { b.gl.get_error() }, glow::NO_ERROR);
        for (tex, fb, ..) in &theirs {
            // SAFETY: as above.
            let kept = unsafe { b.gl.is_texture(*tex) };
            assert!(kept, "the other context lost {tex:?}");
            // SAFETY: as above.
            let kept = unsafe { b.gl.is_framebuffer(*fb) };
            assert!(kept, "the other context lost {fb:?}");
        }
    }
}
