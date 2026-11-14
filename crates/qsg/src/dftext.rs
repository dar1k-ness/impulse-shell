//! Qt Quick `Text { renderType: Text.QtRendering }`: QDistanceField glyphs (54 px base, radius 5)
//! drawn like QSGHiQSubPixelDistanceFieldTextMaterial (subpixel AA, ConstantColor blending).

use freetype_sys as ft;
use std::rc::Rc;

use crate::pathsimplify::PathEl;
use crate::raster::Canvas;

/// QT_DISTANCEFIELD_DEFAULT_BASEFONTSIZE / _SCALE / _RADIUS.
pub const BASE: f64 = 54.0;
const SCALE: f64 = 16.0;
const MARGIN: f64 = 80.0 / 16.0;

/// Distance field of one glyph at the 54 px base size.
pub struct DfGlyph {
    pub w: usize,
    pub h: usize,
    pub data: Vec<u8>,
    /// Path bounding rect in base-size px, y down from the baseline.
    pub bx: f64,
    pub by: f64,
    pub bw: f64,
    pub bh: f64,
}

/// Per-pixel subpixel coverage (r, g, b, a) of a placed glyph, top-left at a device pixel.
pub type DfTile = (Rc<Canvas>, i64, i64);

/// QFreetypeFace::addGlyphToPath of the loaded outline (conics as cubics, y down), in px.
/// # Safety
/// `g` must be a live glyph slot holding an outline.
unsafe fn glyph_path(g: ft::FT_GlyphSlot) -> Vec<PathEl> {
    // SAFETY: the caller passes a live slot.
    let o = unsafe { &(*g).outline };
    // SAFETY: FT_Outline: `points` and `tags` hold `n_points` entries, `contours` holds
    // `n_contours`.
    let pts = unsafe { crate::text::c_slice(o.points, o.n_points as usize) };
    // SAFETY: see `pts`.
    let tags = unsafe { crate::text::c_slice(o.tags, o.n_points as usize) };
    // SAFETY: see `pts`.
    let ends = unsafe { crate::text::c_slice(o.contours, o.n_contours as usize) };
    let at = |i: usize| (pts[i].x as f64 / 64.0, -(pts[i].y as f64) / 64.0);
    let mix = |a: (f64, f64), b: (f64, f64), ka: f64, kb: f64, d: f64| {
        ((a.0 * ka + b.0 * kb) / d, (a.1 * ka + b.1 * kb) / d)
    };
    let mut path = vec![];
    let mut i: isize = 0;
    for &last in ends {
        let last = last as usize;
        let mut start = at(i as usize);
        if tags[i as usize] & 1 == 0 {
            start = if tags[last] & 1 == 0 {
                mix(at(last), start, 1.0, 1.0, 2.0)
            } else {
                at(last)
            };
            i -= 1;
        }
        path.push(PathEl::Move(start.0, start.1));
        let mut c = [start; 4];
        let mut n = 1;
        while i < last as isize {
            i += 1;
            c[n] = at(i as usize);
            n += 1;
            match tags[i as usize] & 3 {
                2 => {
                    if n < 4 {
                        continue;
                    }
                    c[3] = mix(c[3], c[2], 1.0, 1.0, 2.0);
                    i -= 1;
                }
                0 => {
                    if n < 3 {
                        continue;
                    }
                    c[3] = mix(c[1], c[2], 1.0, 1.0, 2.0);
                    c[2] = mix(c[1], c[3], 2.0, 1.0, 3.0);
                    c[1] = mix(c[1], c[0], 2.0, 1.0, 3.0);
                    i -= 1;
                }
                _ => {
                    if n == 2 {
                        path.push(PathEl::Line(c[1].0, c[1].1));
                        c[0] = c[1];
                        n = 1;
                        continue;
                    } else if n == 3 {
                        c[3] = c[2];
                        c[2] = mix(c[1], c[3], 2.0, 1.0, 3.0);
                        c[1] = mix(c[1], c[0], 2.0, 1.0, 3.0);
                    }
                }
            }
            path.push(PathEl::Cubic(
                c[1].0, c[1].1, c[2].0, c[2].1, c[3].0, c[3].1,
            ));
            c[0] = c[3];
            n = 1;
        }
        if n != 1 {
            c[3] = start;
            if n == 2 {
                c[2] = mix(c[1], c[3], 2.0, 1.0, 3.0);
                c[1] = mix(c[1], c[0], 2.0, 1.0, 3.0);
            }
            path.push(PathEl::Cubic(
                c[1].0, c[1].1, c[2].0, c[2].1, c[3].0, c[3].1,
            ));
        }
        i += 1;
    }
    path
}

/// QPainterPath::boundingRect: end points plus the cubic extrema.
fn path_bounds(path: &[PathEl]) -> Option<(f64, f64, f64, f64)> {
    let mut b = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    fn add(b: &mut (f64, f64, f64, f64), x: f64, y: f64) {
        *b = (b.0.min(x), b.1.min(y), b.2.max(x), b.3.max(y));
    }
    let mut cur = (0.0, 0.0);
    for e in path {
        match *e {
            PathEl::Move(x, y) | PathEl::Line(x, y) => {
                add(&mut b, x, y);
                cur = (x, y);
            }
            PathEl::Cubic(x1, y1, x2, y2, x3, y3) => {
                add(&mut b, x3, y3);
                for (p0, p1, p2, p3, ax) in [(cur.0, x1, x2, x3, 0), (cur.1, y1, y2, y3, 1)] {
                    // B'(t) = a t^2 + b t + c
                    let a = 3.0 * (-p0 + 3.0 * p1 - 3.0 * p2 + p3);
                    let bb = 6.0 * (p0 - 2.0 * p1 + p2);
                    let c = 3.0 * (p1 - p0);
                    let mut ts = vec![];
                    if a.abs() < 1e-12 {
                        if bb.abs() > 1e-12 {
                            ts.push(-c / bb);
                        }
                    } else {
                        let disc = bb * bb - 4.0 * a * c;
                        if disc >= 0.0 {
                            let r = disc.sqrt();
                            ts.push((-bb + r) / (2.0 * a));
                            ts.push((-bb - r) / (2.0 * a));
                        }
                    }
                    for t in ts.into_iter().filter(|t| *t > 0.0 && *t < 1.0) {
                        let m = 1.0 - t;
                        let v = m * m * m * p0
                            + 3.0 * m * m * t * p1
                            + 3.0 * m * t * t * p2
                            + t * t * t * p3;
                        let (cx, cy) = if ax == 0 { (v, cur.1) } else { (cur.0, v) };
                        // Only the extremal coordinate widens the box.
                        if ax == 0 {
                            b.0 = b.0.min(cx);
                            b.2 = b.2.max(cx);
                        } else {
                            b.1 = b.1.min(cy);
                            b.3 = b.3.max(cy);
                        }
                    }
                }
                cur = (x3, y3);
            }
        }
    }
    (b.0 <= b.2).then_some(b)
}

/// QFontEngineFT::addGlyphsToPath for one glyph: the outline loaded at units_per_EM
/// (lockFace(Unscaled)) and brought to `px` pixels by scaleOutline, y down from the baseline.
///
/// # Safety
/// `face` must be a valid face; its char size is restored to `px` afterwards.
pub unsafe fn outline(
    face: ft::FT_Face,
    gid: u32,
    px: u32,
    embolden: bool,
    oblique: bool,
) -> Vec<PathEl> {
    // SAFETY: the caller passes a valid face.
    unsafe { outline_at(face, gid, px, px, embolden, oblique) }.unwrap_or_default()
}

/// `outline` at `px` pixels, the face's char size set to `restore_px` afterwards; `None` for a
/// glyph that is no outline.
///
/// # Safety
/// `face` must be a valid face.
unsafe fn outline_at(
    face: ft::FT_Face,
    gid: u32,
    px: u32,
    restore_px: u32,
    embolden: bool,
    oblique: bool,
) -> Option<Vec<PathEl>> {
    // SAFETY: the caller passes a valid face.
    let upem = unsafe { (*face).units_per_EM } as i64;
    if upem == 0 {
        return None;
    }
    // SAFETY: as above.
    unsafe { ft::FT_Set_Char_Size(face, (upem << 6) as _, (upem << 6) as _, 0, 0) };
    // SAFETY: as above.
    unsafe { ft::FT_Set_Transform(face, std::ptr::null_mut(), std::ptr::null_mut()) };
    // SAFETY: as above.
    let slot = unsafe { crate::text::load_glyph(face, gid, ft::FT_LOAD_NO_BITMAP) };
    // SAFETY: the slot just loaded.
    let slot = slot.filter(|&g| unsafe { (*g).format } == ft::FT_GLYPH_FORMAT_OUTLINE);
    let path = slot.map(|g| {
        if embolden {
            // SAFETY: the slot just loaded.
            unsafe { ft::FT_GlyphSlot_Embolden(g) };
        }
        if oblique {
            // SAFETY: the slot just loaded.
            unsafe { ft::FT_GlyphSlot_Oblique(g) };
        }
        // scaleOutline: FT_MulDiv(size, 1 << 10, units_per_EM), then FT_MulFix per coordinate.
        let size = (px as i64) << 6;
        let scale = (size * 1024 + upem / 2) / upem;
        let mul_fix = |v: i64| {
            let p = v * scale;
            if p >= 0 {
                (p + 0x8000) >> 16
            } else {
                -((-p + 0x8000) >> 16)
            }
        };
        // SAFETY: FT_Outline: `points` holds `n_points` entries of the slot just loaded.
        let o = unsafe { &mut (*g).outline };
        for i in 0..o.n_points as usize {
            // SAFETY: `i` is within `n_points`.
            let at = unsafe { o.points.add(i) };
            // SAFETY: a point of the outline, which nothing else refers to here.
            let pt = unsafe { &mut *at };
            pt.x = mul_fix(pt.x);
            pt.y = mul_fix(pt.y);
        }
        // SAFETY: an outline slot just loaded.
        unsafe { glyph_path(g) }
    });
    let rs = (restore_px as i64) << 6;
    // SAFETY: as above.
    unsafe { ft::FT_Set_Char_Size(face, rs as _, rs as _, 0, 0) };
    path
}

/// QDistanceField::setGlyph: the glyph path at `base`*16 px (54, or the render type quality) translated to its bounding rect,
/// rasterized by makeDistanceField into a ceil(w/16)+10 x ceil(h/16)+10 field.
///
/// # Safety
/// `face` must be a valid face; its char size is restored to `restore_px` afterwards.
pub unsafe fn build(face: ft::FT_Face, gid: u32, restore_px: u32, base: f64) -> Option<DfGlyph> {
    // QRawFont::pathForGlyph of the reference font (base * 16 px): QFontEngineFT::addGlyphsToPath.
    // SAFETY: the caller passes a valid face.
    let path = unsafe { outline_at(face, gid, (base * SCALE) as u32, restore_px, false, false) }?;
    let empty = DfGlyph {
        w: 0,
        h: 0,
        data: vec![],
        bx: 0.0,
        by: 0.0,
        bw: 0.0,
        bh: 0.0,
    };
    let Some((x0, y0, x1, y1)) = path_bounds(&path) else {
        return Some(empty);
    };
    let t = |x: f64, y: f64| (x - x0, y - y0);
    let moved: Vec<PathEl> = path
        .iter()
        .map(|e| match *e {
            PathEl::Move(x, y) => {
                let p = t(x, y);
                PathEl::Move(p.0, p.1)
            }
            PathEl::Line(x, y) => {
                let p = t(x, y);
                PathEl::Line(p.0, p.1)
            }
            PathEl::Cubic(a, b, c, d, e, f) => {
                let (p, q, r) = (t(a, b), t(c, d), t(e, f));
                PathEl::Cubic(p.0, p.1, q.0, q.1, r.0, r.1)
            }
        })
        .collect();
    let (bw, bh) = (x1 - x0, y1 - y0);
    let m = MARGIN as i32;
    let w = (bw / SCALE).ceil() as i32 + 2 * m;
    let h = (bh / SCALE).ceil() as i32 + 2 * m;
    let data = crate::qdistancefield::make_distance_field(&moved, w, h, SCALE as i32, m);
    Some(DfGlyph {
        w: w as usize,
        h: h as usize,
        data,
        bx: x0 / SCALE,
        by: y0 / SCALE,
        bw: bw / SCALE,
        bh: bh / SCALE,
    })
}

/// qsgdistancefieldglyphnode_p.cpp thresholdFunc / spreadFunc, in `float` as there:
/// (alphaMin, alphaMax) of a field drawn at `scale` = fontScale * matrixScale.
fn alpha_range(scale: f32) -> (f32, f32) {
    let (base, range) = threshold_spread(scale);
    ((base - range).max(0.0), (base + range).min(1.0))
}

/// thresholdFunc and spreadFunc.
fn threshold_spread(scale: f32) -> (f32, f32) {
    let s = scale.clamp(0.15, 0.3);
    let base = 0.5f32 - ((s - 0.15f32) / (0.3f32 - 0.15f32) * -0.065f32 + 0.065f32);
    (base, 0.06f32 / scale)
}

/// GLSL smoothstep as Mesa compiles it for llvmpipe: `t * (t * (3 - 2 t))`, every step a float.
fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    // fmax then fmin, as the shader has them: 0/0 gives 0, where `clamp` would give NaN.
    #[allow(clippy::manual_clamp)]
    let t = ((x - e0) / (e1 - e0)).max(0.0).min(1.0);
    t * (t * (3.0f32 - 2.0f32 * t))
}

/// llvmpipe's mix of two bytes by a weight of 0..=255 256ths (Mesa 26.2.6,
/// gallivm/lp_bld_arit.c `lp_build_lerp_simple`, "lerp_round_half_even"), in 16 bits.
fn lerp_u8(x: u16, v0: u16, v1: u16) -> u16 {
    let m = x.wrapping_mul(v1.wrapping_sub(v0));
    let m = m.wrapping_add(0x7f + ((m >> 8) & 1)) >> 8;
    (v0 + m) & 0xff
}

/// QHashPrivate::hash (qhashfunctions.h) of a glyph index under hash seed 0.
fn qhash(k: u32) -> u64 {
    const M: u64 = 0xd6e8_feb8_6659_fd93;
    let mut k = k as u64;
    k ^= k >> 32;
    k = k.wrapping_mul(M);
    k ^= k >> 32;
    k = k.wrapping_mul(M);
    k ^ (k >> 32)
}

/// The order a `QSet<glyph_t>` gives back `keys` put in in this order, under hash seed 0
/// (qhash.h: 128 buckets, twice the next power of two once half full, the hash's low bits,
/// the next free bucket on a clash).
fn qset_order(keys: &[u32]) -> Vec<u32> {
    fn slot(b: &[Option<u32>], k: u32) -> usize {
        let mut i = (qhash(k) & (b.len() as u64 - 1)) as usize;
        while b[i].is_some_and(|x| x != k) {
            i = (i + 1) % b.len();
        }
        i
    }
    let mut b: Vec<Option<u32>> = vec![None; 128];
    let mut size = 0usize;
    for &k in keys {
        let mut i = slot(&b, k);
        if b[i].is_some() {
            continue;
        }
        if size >= b.len() / 2 {
            let n = 1usize << (usize::BITS - (size + 1).leading_zeros() + 1);
            let old = std::mem::replace(&mut b, vec![None; n]);
            for x in old.into_iter().flatten() {
                let j = slot(&b, x);
                b[j] = Some(x);
            }
            i = slot(&b, k);
        }
        b[i] = Some(k);
        size += 1;
    }
    b.into_iter().flatten().collect()
}

/// A node of QSGAreaAllocator's tree (qsgareaallocator.cpp).
#[derive(Default)]
struct AreaNode {
    kids: Option<Box<[AreaNode; 2]>>,
    split: i32,
    horizontal: bool,
    occupied: bool,
}

/// QSGAreaAllocator::allocateInNode: the top-left corner given to a `w` x `h` area in `rect`
/// (x, y, width, height).
fn allocate(n: &mut AreaNode, w: i32, h: i32, rect: [i32; 4]) -> Option<(i32, i32)> {
    let [x, y, rw, rh] = rect;
    if w > rw || h > rh {
        return None;
    }
    if n.kids.is_none() {
        if n.occupied {
            return None;
        }
        if w + 2 >= rw && h + 2 >= rh {
            n.occupied = true;
            return Some((x, y));
        }
        n.kids = Some(Default::default());
        n.horizontal = (rw - w) as i64 * (rh as i64) < (rh - h) as i64 * rw as i64;
        n.split = if n.horizontal { y + h } else { x + w };
    }
    let (left, right) = if n.horizontal {
        (
            [x, y, rw, n.split - y],
            [x, n.split, rw, rh - (n.split - y)],
        )
    } else {
        (
            [x, y, n.split - x, rh],
            [n.split, y, rw - (n.split - x), rh],
        )
    };
    let kids = n.kids.as_mut()?;
    allocate(&mut kids[0], w, h, left).or_else(|| allocate(&mut kids[1], w, h, right))
}

/// GL_MAX_TEXTURE_SIZE of llvmpipe, the side of the area QSGRhiDistanceFieldGlyphCache
/// allocates in (three textures high).
const MAX_TEXTURE: i32 = 16384;
/// QSG_RHI_DISTANCEFIELD_GLYPH_CACHE_PADDING: empty texels around every glyph.
const PADDING: i32 = 2;

/// QSGRhiDistanceFieldGlyphCache of one font: the glyphs' fields in one texture, placed in
/// the order Qt places them. Where a glyph lies and how large the texture is reach the
/// picture: texture coordinates are `float`s of texel / size.
#[derive(Default)]
pub struct DfAtlas {
    fields: std::collections::HashMap<u32, Option<Rc<DfGlyph>>>,
    /// Top-left texel of a glyph's field.
    at: std::collections::HashMap<u32, (i32, i32)>,
    root: AreaNode,
    w: usize,
    h: usize,
    tex: Vec<u8>,
    /// Text.renderTypeQuality when above zero: the cache's base font size.
    quality: i32,
}

/// The matrices of a text in a batch the renderer did not merge, as `float`s of Qt's
/// QMatrix4x4 arithmetic (the caller has the item tree they come from).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DfTurn {
    /// projection * model-view: m11 m21 / m12 m22 (x' = mvp[0] x + mvp[2] y + mvp[4]), then
    /// the translation.
    pub mvp: [f32; 6],
    /// RenderState::determinant() of the model-view matrix.
    pub det: f32,
    /// x and y of column 0 of the inverse of `mvp`, which the subpixel shader steps along.
    pub inv0: [f32; 2],
}

impl DfTurn {
    pub fn bits(&self) -> [u32; 9] {
        let [a, b, c, d, e, f] = self.mvp.map(f32::to_bits);
        let [g, h] = self.inv0.map(f32::to_bits);
        [a, b, c, d, e, f, self.det.to_bits(), g, h]
    }
}

/// What a text is drawn into: the render target as QQuickWindowPrivate::renderSceneGraph
/// hands it to the renderer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DfView {
    /// The viewport: the target's size in device pixels.
    pub size: [f32; 2],
    /// The rectangle of the projection matrix: that size over the device pixel ratio.
    pub proj: [f32; 2],
    /// RenderState::devicePixelRatio().
    pub dpr: f32,
}

impl DfView {
    /// A target of `w` by `h` pixels at device pixel ratio 1.
    pub fn plain(w: f32, h: f32) -> Self {
        Self {
            size: [w, h],
            proj: [w, h],
            dpr: 1.0,
        }
    }

    pub fn bits(&self) -> [u32; 5] {
        let ([a, b], [c, d]) = (self.size.map(f32::to_bits), self.proj.map(f32::to_bits));
        [a, b, c, d, self.dpr.to_bits()]
    }
}

/// Where and under what a glyph quad is drawn (QSGDistanceFieldGlyphNode::updateGeometry and
/// the batch renderer's matrix).
pub struct DfPlace {
    /// The font's pixel size.
    pub px: f64,
    /// Pen position in the item: x and the baseline.
    pub pen: [f64; 2],
    /// The item's translation from its batch root, which a merged batch adds to the vertices.
    pub shift: [f32; 2],
    /// Set when the shader's matrix is more than the projection: the item's whole matrix
    /// when the batch is not merged (one of its texts is turned or scaled; `shift` is then
    /// zero), the batch root's when a merged batch has one (an ancestor that clips, or that
    /// moved with enough under it).
    pub turn: Option<DfTurn>,
    pub view: DfView,
    /// The shader's `color`: premultiplied and times opacity; subpixel tiles read its alpha
    /// only (colour alpha times opacity), their colour is the blend constant.
    pub color: [f32; 4],
    pub look: DfLook,
}

/// A glyph's quad on its way to the rasterizer (`DfAtlas::quad`).
struct DfQuad {
    /// The font's pixel size over the size the fields were built at.
    fs: f64,
    /// Left and right, top and bottom of the quad, as the vertex shader gets them.
    cx: [f32; 2],
    cy: [f32; 2],
    /// The same of its texels in the atlas.
    tx: [f32; 2],
    ty: [f32; 2],
    /// The vertex shader's matrix, its determinant and vecDelta.
    m: [f32; 6],
    det: f32,
    vd: [f32; 2],
    /// The viewport.
    size: [f32; 2],
}

impl DfQuad {
    /// Where the viewport puts the vertex (`x`, `y`).
    fn window(&self, x: f32, y: f32) -> [f32; 2] {
        let (m, [vw, vh]) = (&self.m, self.size);
        let (px, py) = (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]);
        [
            px.mul_add(vw / 2.0, vw / 2.0),
            py.mul_add(-vh / 2.0, vh / 2.0),
        ]
    }
}

/// lp_setup_tri.c: a window position snapped to 1/256 of a pixel.
fn snap(v: f32) -> i64 {
    ((v - 0.5f32) * 256.0f32).round_ties_even() as i64
}

/// The material of a glyph node (QSGDistanceFieldGlyphNode::updateMaterial). All but
/// `Subpixel` give tiles that hold their premultiplied colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DfLook {
    /// hiqsubpixeldistancefieldtext: a node that has not been recycled.
    Subpixel,
    /// distancefieldtext: a recycled node (Qt resets it to GrayAntialiasing and never
    /// restores the preferred mode), and the fill of outlined text.
    Gray,
    /// Text.Outline, distancefieldoutlinetext, with the style colour (premultiplied, times
    /// opacity).
    Outline([f32; 4]),
    /// Text.Raised (1.0) and Text.Sunken (-1.0), distancefieldshiftedtext: the style colour
    /// and the material's shift down.
    Shifted([f32; 4], f32),
}

impl DfLook {
    pub fn bits(&self) -> [u32; 6] {
        let (kind, c, dy) = match *self {
            DfLook::Subpixel => (0, [0.0; 4], 0.0),
            DfLook::Gray => (1, [0.0; 4], 0.0),
            DfLook::Outline(c) => (2, c, 0.0),
            DfLook::Shifted(c, dy) => (3, c, dy),
        };
        let [r, g, b, a] = c.map(f32::to_bits);
        [kind, r, g, b, a, dy.to_bits()]
    }
}

impl DfAtlas {
    pub fn of_quality(quality: i32) -> Self {
        DfAtlas {
            quality,
            ..Default::default()
        }
    }

    /// QSGDistanceFieldGlyphCache::baseFontSize: the pixel size the fields are of, over
    /// QT_DISTANCEFIELD_SCALE.
    pub fn base(&self) -> f64 {
        if self.quality > 0 {
            self.quality as f64
        } else {
            BASE
        }
    }

    /// The field of `gid`, built once per font.
    pub fn field(
        &mut self,
        gid: u32,
        build: impl FnOnce() -> Option<DfGlyph>,
    ) -> Option<Rc<DfGlyph>> {
        self.fields
            .entry(gid)
            .or_insert_with(|| build().map(Rc::new))
            .clone()
    }

    /// (texture width, height, glyphs placed): what a cached tile depends on.
    pub fn state(&self) -> (usize, usize, usize) {
        (self.w, self.h, self.at.len())
    }

    /// QSGDistanceFieldGlyphCache::populate for the glyphs of one glyph node, in run order
    /// (each already has its field): the new ones are placed in QSet order.
    pub fn populate(&mut self, gids: &[u32]) {
        let new: Vec<u32> = gids
            .iter()
            .copied()
            .filter(|g| {
                !self.at.contains_key(g) && matches!(self.fields.get(g), Some(Some(f)) if f.w > 0)
            })
            .collect();
        if new.is_empty() {
            return;
        }
        for g in qset_order(&new) {
            let Some(Some(f)) = self.fields.get(&g) else {
                continue;
            };
            let (w, h) = (f.w as i32 + 2 * PADDING, f.h as i32 + 2 * PADDING);
            let area = [0, 0, MAX_TEXTURE, 3 * MAX_TEXTURE];
            let Some((x, y)) = allocate(&mut self.root, w, h, area) else {
                continue;
            };
            self.at.insert(g, (x + PADDING, y + PADDING));
            self.w = self.w.max((x + w) as usize);
            self.h = self.h.max((y + h) as usize);
        }
        self.tex = vec![0; self.w * self.h];
        for (g, &(x, y)) in &self.at {
            let Some(Some(f)) = self.fields.get(g) else {
                continue;
            };
            for j in 0..f.h {
                let to = (y as usize + j) * self.w + x as usize;
                self.tex[to..to + f.w].copy_from_slice(&f.data[j * f.w..(j + 1) * f.w]);
            }
        }
    }

    /// `texture()` at normalized (`s`, `t`) as llvmpipe filters an 8-bit texture (Mesa
    /// 26.2.6, gallivm/lp_bld_sample_aos.c `lp_build_sample_image_linear`): the coordinate
    /// times the size is rounded to 1/256 of a texel, the four texels are mixed in bytes
    /// (clamped to the edge), and the byte becomes a float by 1/255.
    fn sample(&self, s: f32, t: f32) -> f32 {
        let fix = |v: f32, size: usize| (v * (size << 8) as f32).round_ties_even() as i64 - 128;
        let (x, y) = (fix(s, self.w), fix(t, self.h));
        let (ix, iy) = (x >> 8, y >> 8);
        let (ax, ay) = ((x & 255) as u16, (y & 255) as u16);
        let at = |i: i64, j: i64| {
            let i = i.clamp(0, self.w as i64 - 1) as usize;
            let j = j.clamp(0, self.h as i64 - 1) as usize;
            self.tex[j * self.w + i] as u16
        };
        let top = lerp_u8(ax, at(ix, iy), at(ix + 1, iy));
        let bot = lerp_u8(ax, at(ix, iy + 1), at(ix + 1, iy + 1));
        lerp_u8(ay, top, bot) as f32 * (1.0f32 / 255.0f32)
    }

    /// The quad of glyph `gid` drawn at `p` (updateGeometry) and the matrix its vertices go
    /// through.
    fn quad(&self, gid: u32, p: &DfPlace) -> Option<DfQuad> {
        let g = self.fields.get(&gid)?.as_ref()?;
        let &(ax, ay) = self.at.get(&gid)?;
        // updateGeometry, in qreal.
        let fs = p.px / self.base();
        let (mut margin, mut tm) = (2.0, 2.0 / fs);
        if tm > MARGIN {
            tm = MARGIN;
            margin = MARGIN * fs;
        }
        let (x, y) = (
            p.pen[0] + (g.bx * fs - margin),
            p.pen[1] - (-g.by * fs + margin),
        );
        let (mw, mh) = (g.bw * fs + margin * 2.0, g.bh * fs + margin * 2.0);
        let tx1 = (ax as f64 + (MARGIN - tm)) as f32;
        let tx2 = (tx1 as f64 + (g.bw + tm * 2.0)) as f32;
        let ty1 = (ay as f64 + (MARGIN - tm)) as f32;
        let ty2 = (ty1 as f64 + (g.bh + tm * 2.0)) as f32;
        let (mut cx, mut cy) = ([x as f32, (x + mw) as f32], [y as f32, (y + mh) as f32]);
        let ([vw, _], [pw, ph]) = (p.view.size, p.view.proj);
        // A merged batch: the vertices carry the translation and the matrix is
        // QMatrix4x4::ortho(0, w, h, 0, 1, -1) alone, of the projection's rectangle.
        let k = (2.0f64 / vw as f64) as f32;
        let (m, det, vd) = match p.turn {
            None => {
                cx = cx.map(|v| v + p.shift[0]);
                cy = cy.map(|v| v + p.shift[1]);
                let (p00, p30) = (2.0f32 / pw, -(0.0f32 + pw) / pw);
                let (p11, p31) = (2.0f32 / (0.0f32 - ph), -(0.0f32 + ph) / (0.0f32 - ph));
                (
                    [p00, 0.0, 0.0, p11, p30, p31],
                    1.0f32,
                    [1.0f32 / p00 * k, 0.0],
                )
            }
            // vecDelta: column 0 of the inverse, times 2 / viewport width. A merged batch
            // under a batch root: the vertices carry the translation from the root.
            Some(t) => {
                if p.shift != [0.0; 2] {
                    cx = cx.map(|v| v + p.shift[0]);
                    cy = cy.map(|v| v + p.shift[1]);
                }
                (t.mvp, t.det, t.inv0.map(|v| v * k))
            }
        };
        Some(DfQuad {
            fs,
            cx,
            cy,
            tx: [tx1, tx2],
            ty: [ty1, ty2],
            m,
            det,
            vd,
            size: p.view.size,
        })
    }

    /// Whether the quad of glyph `gid` drawn at `p` reaches a pixel of the view: the
    /// viewport keeps the others out, so a line far longer than its window costs what its
    /// glyphs in the window cost.
    pub fn in_view(&self, gid: u32, p: &DfPlace) -> bool {
        let Some(q) = self.quad(gid, p) else {
            return false;
        };
        let (mut lo, mut hi) = ([i64::MAX; 2], [i64::MIN; 2]);
        for (x, y) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            let at = q.window(q.cx[x], q.cy[y]).map(snap);
            lo = [lo[0].min(at[0]), lo[1].min(at[1])];
            hi = [hi[0].max(at[0]), hi[1].max(at[1])];
        }
        let size = q.size.map(|v| v as i64);
        (0..2).all(|i| hi[i] >> 8 >= 0 && lo[i] >> 8 < size[i])
    }

    /// Coverage of glyph `gid` (placed by `populate`) drawn at `p`: the quad of
    /// QSGDistanceFieldGlyphNode through hiqsubpixeldistancefieldtext.vert/.frag, or
    /// distancefieldtext.vert/.frag when `gray` (a recycled glyph node: Qt resets it to
    /// GrayAntialiasing and never restores the preferred mode; all four channels equal).
    /// Every number is a `float` where Qt, the shaders and llvmpipe have one, in their order
    /// of operations: vertices, llvmpipe's plane equations (lp_state_setup.c), its
    /// interpolation (lp_bld_interp.c), the texture filter and the conversion to bytes.
    pub fn render(&self, gid: u32, p: &DfPlace) -> Option<DfTile> {
        let q = self.quad(gid, p)?;
        let DfQuad {
            fs,
            cx,
            cy,
            tx: [tx1, tx2],
            ty: [ty1, ty2],
            det,
            vd,
            ..
        } = q;
        let ts = [1.0f32 / self.w as f32, 1.0f32 / self.h as f32];
        let fscale = fs as f32;
        // m_matrixScale = qSqrt(qAbs(state.determinant())) * devicePixelRatio, a float.
        let combined = fscale * ((det.abs() as f64).sqrt() as f32 * p.view.dpr);
        let (amin, amax) = alpha_range(combined);
        // DistanceFieldOutlineTextMaterialRhiShader::updateUniformData: where the outline
        // fades out (the shader calls the pair outlineAlphaMax0 / 1).
        let outline = {
            let (base, range) = threshold_spread(combined);
            let limit = (base - 0.5f32 / MARGIN as f32 / fscale).max(0.2f32);
            ((limit - range).max(0.0f32), (limit + range).min(amin))
        };
        // DistanceFieldShiftedStyleTextMaterialRhiShader: the shift over the font scale.
        let shifted = match p.look {
            DfLook::Shifted(_, dy) => {
                Some([(1.0 / fs * 0.0) as f32, (1.0 / fs * dy as f64) as f32])
            }
            _ => None,
        };
        let (far, near) = (
            [vd[0] * 0.667f32, vd[1] * 0.667f32],
            [vd[0] * 0.333f32, vd[1] * 0.333f32],
        );
        let scale = [ts[0] / fscale, ts[1] / fscale];
        // Window position, then sampleCoord and the four neighbours' coordinates.
        let vertex = |x: f32, y: f32, tx: f32, ty: f32| -> ([f32; 2], [f32; 10]) {
            let win = q.window(x, y);
            let sc = [tx * ts[0], ty * ts[1]];
            let base = [sc[0] - scale[0] * x, sc[1] - scale[1] * y];
            let at = |dx: f32, dy: f32| [base[0] + scale[0] * dx, base[1] + scale[1] * dy];
            let (fl, nl) = (at(x - far[0], y - far[1]), at(x - near[0], y - near[1]));
            let (nr, fr) = (at(x + near[0], y + near[1]), at(x + far[0], y + far[1]));
            // shiftedSampleCoord takes the place of the first neighbour.
            let fl = shifted.map_or(fl, |s| [(tx - s[0]) * ts[0], (ty - s[1]) * ts[1]]);
            (
                win,
                [
                    sc[0], sc[1], fl[0], fl[1], nl[0], nl[1], nr[0], nr[1], fr[0], fr[1],
                ],
            )
        };
        let (tl, tr) = (
            vertex(cx[0], cy[0], tx1, ty1),
            vertex(cx[1], cy[0], tx2, ty1),
        );
        let (bl, br) = (
            vertex(cx[0], cy[1], tx1, ty2),
            vertex(cx[1], cy[1], tx2, ty2),
        );
        // lp_setup_tri.c: positions snapped to 1/256 of a pixel, three edge functions with
        // the bottom-left fill rule GL has in a window, and the planes of the varyings.
        struct Tri {
            edge: [[i64; 3]; 3],
            a0: [f32; 10],
            dx: [f32; 10],
            dy: [f32; 10],
        }
        let mut lo = [i64::MAX; 2];
        let mut hi = [i64::MIN; 2];
        let mut tris = vec![];
        for mut v in [[tl, bl, br], [br, tr, tl]] {
            let fixed = |v: &[([f32; 2], [f32; 10]); 3]| v.map(|q| [snap(q.0[0]), snap(q.0[1])]);
            let mut f = fixed(&v);
            let area = (f[0][0] - f[1][0]) * (f[2][1] - f[0][1])
                - (f[2][0] - f[0][0]) * (f[0][1] - f[1][1]);
            if area == 0 {
                continue;
            }
            if area < 0 {
                v.swap(0, 1);
                f = fixed(&v);
            }
            for q in f {
                lo = [lo[0].min(q[0]), lo[1].min(q[1])];
                hi = [hi[0].max(q[0]), hi[1].max(q[1])];
            }
            let edge = [0, 1, 2].map(|i| {
                let j = (i + 1) % 3;
                let (dcdx, dcdy) = (f[i][1] - f[j][1], f[i][0] - f[j][0]);
                let c = dcdx * f[i][0] - dcdy * f[i][1];
                [c + (dcdx < 0 || (dcdx == 0 && dcdy < 0)) as i64, dcdx, dcdy]
            });
            let [p0, p1, p2] = v.map(|q| q.0);
            let (x0, y0) = (p0[0] - 0.5f32, p0[1] - 0.5f32);
            let (dx01, dy01) = (p0[0] - p1[0], p0[1] - p1[1]);
            let (dx20, dy20) = (p2[0] - p0[0], p2[1] - p0[1]);
            let ooa = 1.0f32 / (dx01 * dy20 - dy01 * dx20);
            let (dy20, dy01, dx20, dx01) = (dy20 * ooa, dy01 * ooa, dx20 * ooa, dx01 * ooa);
            let mut t = Tri {
                edge,
                a0: [0.0; 10],
                dx: [0.0; 10],
                dy: [0.0; 10],
            };
            for k in 0..10 {
                let (a, b, c) = (v[0].1[k], v[1].1[k], v[2].1[k]);
                let (da01, da20) = (a - b, c - a);
                t.dx[k] = da01 * dy20 - da20 * dy01;
                t.dy[k] = da20 * dx01 - da01 * dx20;
                t.a0[k] = a - (t.dx[k] * x0 + t.dy[k] * y0);
            }
            tris.push(t);
        }
        if tris.is_empty() {
            return None;
        }
        let (ix0, iy0) = (lo[0] >> 8, lo[1] >> 8);
        let (w, h) = (
            ((hi[0] >> 8) + 1 - ix0) as usize,
            ((hi[1] >> 8) + 1 - iy0) as usize,
        );
        let byte = |v: f32| (v * 255.0f32).round_ties_even() as u8;
        let mut out = vec![[0u8; 4]; w * h];
        for j in 0..h {
            let py = iy0 + j as i64;
            for i in 0..w {
                let px = ix0 + i as i64;
                let inside = |t: &&Tri| {
                    t.edge
                        .iter()
                        .all(|e| e[0] + e[2] * (py << 8) - e[1] * (px << 8) > 0)
                };
                let Some(t) = tris.iter().find(inside) else {
                    continue;
                };
                let at = |k: usize| t.dy[k].mul_add(py as f32, t.dx[k].mul_add(px as f32, t.a0[k]));
                let tex = |k: usize| self.sample(at(k), at(k + 1));
                let c = tex(0);
                // mix() of values the shader computes is x + t (y - x) on llvmpipe
                // (nir_lower_flrp without fused multiply-add).
                let mix = |x: f32, y: f32, t: f32| x + t * (y - x);
                match p.look {
                    DfLook::Subpixel => {}
                    DfLook::Gray => {
                        let s = smoothstep(amin, amax, c);
                        out[j * w + i] = p.color.map(|k| byte(k * s));
                        continue;
                    }
                    DfLook::Outline(style) => {
                        let a = smoothstep(amin, amax, c);
                        let o = smoothstep(outline.0, outline.1, c);
                        out[j * w + i] =
                            [0, 1, 2, 3].map(|k| byte(mix(style[k], p.color[k], a) * o));
                        continue;
                    }
                    DfLook::Shifted(style, _) => {
                        let a = smoothstep(amin, amax, c);
                        let s = smoothstep(amin, amax, tex(2));
                        out[j * w + i] =
                            [0, 1, 2, 3].map(|k| byte(mix(style[k] * s, p.color[k], a)));
                        continue;
                    }
                }
                let n = [tex(2), tex(4), tex(6), tex(8)];
                let d = [
                    ((n[1] - n[0]).abs() * 2.0f32).min(0.67f32),
                    ((n[3] - n[2]).abs() * 2.0f32).min(0.67f32),
                ];
                let lo = d.map(|d| amin * (1.0f32 - d) + 0.5f32 * d);
                let hi = d.map(|d| amax * (1.0f32 - d) + 0.5f32 * d);
                let s = [0, 1, 2, 3].map(|k| smoothstep(lo[k / 2], hi[k / 2], n[k]));
                let c = smoothstep(lo[0] + lo[1], hi[0] + hi[1], 2.0f32 * c);
                out[j * w + i] = [
                    (s[0] + s[1] + c) * 0.333f32,
                    (s[1] + s[2] + c) * 0.333f32,
                    (s[2] + s[3] + c) * 0.333f32,
                    c,
                ]
                .map(|v| byte(v * p.color[3]));
            }
        }
        Some((Rc::new(Canvas::from_px(w, h, out)), ix0, iy0))
    }
}

/// Tiles blended as llvmpipe blends bytes. `Some(k)`, subpixel tiles under the text colour
/// `k` (not premultiplied): ConstantColor / OneMinusSrcColor, alpha One / OneMinusSrcAlpha.
/// `None`, gray tiles, which hold the premultiplied colour: One / OneMinusSrcAlpha.
pub fn blit(cv: &mut Canvas, tiles: &[DfTile], k: Option<[u8; 3]>) {
    for (t, x0, y0) in tiles {
        for j in 0..t.h as i64 {
            let y = y0 + j;
            if y < 0 || y >= cv.h as i64 {
                continue;
            }
            for i in 0..t.w as i64 {
                let x = x0 + i;
                if x < 0 || x >= cv.w as i64 {
                    continue;
                }
                let s = t.px[(j * t.w as i64 + i) as usize].map(u32::from);
                if s == [0; 4] {
                    continue;
                }
                let d = &mut cv.px[y as usize * cv.w + x as usize];
                for ch in 0..4 {
                    let (src, keep) = match k {
                        Some(k) if ch < 3 => (mul255::nearest(s[ch], k[ch] as u32), 255 - s[ch]),
                        _ => (s[ch], 255 - s[3]),
                    };
                    d[ch] = (src + mul255::nearest(d[ch] as u32, keep)).min(255) as u8;
                }
            }
        }
    }
}
