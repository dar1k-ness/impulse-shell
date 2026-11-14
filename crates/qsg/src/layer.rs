//! Item layers (QQuickItemLayer → offscreen 8-bit texture) and layer effects (Qt5Compat OpacityMask).

use crate::raster::Canvas;

/// Offscreen premultiplied texture of an item, sized like QQuickShaderEffectSource (Qt 6.12):
/// `QSizeF(width, height).toSize()`, i.e. rounded, at least 1x1.
pub fn texture(w: f64, h: f64) -> Canvas {
    let (tw, th) = texture_size(w, h);
    Canvas::new(tw, th, [0, 0, 0, 0])
}

/// The layer texture size of an item of `w` x `h`: rounded, at least 1 (QSizeF::toSize).
pub fn texture_size(w: f64, h: f64) -> (usize, usize) {
    (w.round().max(1.0) as usize, h.round().max(1.0) as usize)
}

/// Source-over of `t`, content painted 1:1 on the scene rect of `w` x `h` a layer texture `o`
/// covers, onto that texture inside the scissor `clip` (x1 y1 x2 y2): sampled linearly at each
/// texel's place in the rect, for content that has no draw through the layer's scale.
pub fn draw_fitted(o: &mut Canvas, t: &Canvas, w: f64, h: f64, clip: [i64; 4]) {
    let (sx, sy) = (w / o.w as f64 / t.w as f64, h / o.h as f64 / t.h as f64);
    let cl = |v: i64, hi: usize| v.clamp(0, hi as i64) as usize;
    for y in cl(clip[1], o.h)..cl(clip[3], o.h) {
        for x in cl(clip[0], o.w)..cl(clip[2], o.w) {
            let c = linear(t, (x as f64 + 0.5) * sx, (y as f64 + 0.5) * sy);
            if c != [0.0; 4] {
                o.blend_f(x, y, c);
            }
        }
    }
}

/// `src` stretched over the scene rect (x, y, w, h) with bilinear filtering, as a smooth
/// ShaderEffect samples its source texture.
pub fn draw_linear(cv: &mut Canvas, src: &Canvas, x: f64, y: f64, w: f64, h: f64, opacity: f32) {
    cover(cv, x, y, w, h, |u, v| {
        linear(src, u, v).map(|c| c * opacity)
    });
}

pub(crate) fn nearest(t: &Canvas, u: f64, v: f64) -> [f32; 4] {
    nearest_bytes(t, u, v).map(unorm)
}

/// `t` at (u, v) through the linear filter llvmpipe has for a texture of bytes (Mesa 26.2.6,
/// src/gallium/auxiliary/gallivm/lp_bld_sample_aos.c:762, lp_build_sample_image_linear): the
/// texel coordinate is rounded to 1/256 (lines 786-800), the two texels are clamped to the
/// edge, and the three lerps are lp_build_lerp_simple's with a weight of 0..=255
/// (lp_bld_arit.c:1109) in 16 bits, the half rounded to even. Qt Quick through OpenGL
/// reads every stretched texture like this, and so does every smooth read of ours on the CPU
/// (tests/stretched_textures_qt.rs in the shell); exact weights and one rounding at the end
/// are 1 of 255 off in about a pixel of twelve. On a video card the driver filters.
pub(crate) fn linear(t: &Canvas, u: f64, v: f64) -> [f32; 4] {
    linear_bytes(t, u, v).map(unorm)
}

/// [`linear`] as the bytes the filter gives, before a shader reads them as floats.
pub(crate) fn linear_bytes(t: &Canvas, u: f64, v: f64) -> [u8; 4] {
    let (x, y) = (Tap::linear(u, t.w), Tap::linear(v, t.h));
    let p = |x: usize, y: usize| t.px[y * t.w + x];
    blend_bytes(
        p(x.i0, y.i0),
        p(x.i1, y.i0),
        p(x.i0, y.i1),
        p(x.i1, y.i1),
        x,
        y,
    )
}

/// The texel (u, v) is in: the coordinate times the size, in f32, rounded down
/// (lp_build_sample_image_nearest).
fn nearest_bytes(t: &Canvas, u: f64, v: f64) -> [u8; 4] {
    let at = |c: f64, n: usize| ((c as f32 * n as f32).floor().max(0.0) as usize).min(n - 1);
    t.px[at(v, t.h) * t.w + at(u, t.w)]
}

/// The levels after the first of `t`'s mipmap chain, down to 1x1, as glGenerateMipmap of
/// llvmpipe makes them (Mesa 26.2.6, util_gen_mipmap in src/gallium/auxiliary/util/
/// u_gen_mipmap.c: each level is a blit of the level before it through the linear filter):
/// half the size, not under 1, each texel [`linear`] at its centre. QSGPlainTexture asks
/// for the chain (QRhiResourceUpdateBatch::generateMips) when `mipmap` is set.
pub fn mip_levels(t: &Canvas) -> Vec<Canvas> {
    let mut levels: Vec<Canvas> = Vec::new();
    loop {
        let prev = levels.last().unwrap_or(t);
        if prev.w <= 1 && prev.h <= 1 {
            return levels;
        }
        let (w, h) = ((prev.w / 2).max(1), (prev.h / 2).max(1));
        let centre = |i: usize, n: usize| ((i as f32 + 0.5) / n as f32) as f64;
        let px = (0..w * h)
            .map(|n| linear_bytes(prev, centre(n % w, w), centre(n / w, h)))
            .collect();
        levels.push(Canvas::from_px(w, h, px));
    }
}

/// The levels of a mipmap chain a draw reads, as llvmpipe chooses them for
/// GL_*_MIPMAP_LINEAR (Mesa 26.2.6, src/gallium/auxiliary/gallivm/lp_bld_sample.c:
/// lp_build_rho, lp_build_lod_selector, lp_build_linear_mip_levels): `level` and the one
/// after it, the second with `weight` of 256.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Lod {
    pub level: usize,
    weight: u16,
}

impl Lod {
    /// For a texture with `levels` levels (the first included) drawn with `across` texels
    /// of the first level to a pixel along x and `down` along y. rho squared is the larger
    /// of the two squares (lp_bld_sample.c:704), the level of detail half of
    /// lp_build_fast_log2 of it (lp_bld_arit.c:3405: the exponent plus the mantissa less
    /// 1, a straight line between the powers of two), all in f32; its whole part is the
    /// level, its fraction in 1/256, rounded down, the weight of the next
    /// (lp_build_ifloor_fract, then line 1399). Under the first level, or from the last
    /// on, one level is read alone.
    pub fn choose(across: f64, down: f64, levels: usize) -> Lod {
        let (rx, ry) = (across as f32, down as f32);
        let rho2 = (rx * rx).max(ry * ry);
        if !rho2.is_normal() || rho2 <= 0.0 {
            return Lod {
                level: 0,
                weight: 0,
            };
        }
        let bits = rho2.to_bits();
        let exponent = ((bits >> 23) & 0xff) as i32 - 127;
        let mantissa = f32::from_bits((bits & 0x007f_ffff) | 0x3f80_0000);
        let lod = 0.5 * (exponent as f32 + (mantissa - 1.0));
        let whole = lod.floor();
        let last = levels.saturating_sub(1);
        if whole < 0.0 {
            return Lod {
                level: 0,
                weight: 0,
            };
        }
        if whole as usize >= last {
            return Lod {
                level: last,
                weight: 0,
            };
        }
        Lod {
            level: whole as usize,
            weight: ((lod - whole) * 256.0) as u16,
        }
    }

    /// Whether the first level is read alone: the draw is the one without mipmaps.
    pub fn is_base(self) -> bool {
        self == Lod {
            level: 0,
            weight: 0,
        }
    }

    /// The last level the draw reads.
    pub fn deepest(self) -> usize {
        self.level + (self.weight > 0) as usize
    }
}

/// `chain` (the texture, then [`mip_levels`] of it) at (u, v) for the levels `lod` names,
/// each through the linear filter when `smooth` and from its nearest texel otherwise, the
/// two blended as the filter blends two texels (lp_build_sample_mipmap, lp_bld_sample_aos.c:
/// 8-bit weight, lp_build_lerp).
pub(crate) fn mip_sample(chain: &[&Canvas], lod: Lod, smooth: bool, u: f64, v: f64) -> [f32; 4] {
    let read = |t: &Canvas| {
        if smooth {
            linear_bytes(t, u, v)
        } else {
            nearest_bytes(t, u, v)
        }
    };
    let a = read(chain[lod.level]);
    if lod.weight == 0 {
        return a.map(unorm);
    }
    let b = read(chain[lod.level + 1]);
    std::array::from_fn(|i| unorm(lerp8(a[i], b[i], lod.weight)))
}

/// One axis of [`linear`]: the two texels a coordinate falls between and the weight of the
/// second, of 256.
#[derive(Clone, Copy)]
pub(crate) struct Tap {
    pub(crate) i0: usize,
    pub(crate) i1: usize,
    pub(crate) weight: u16,
}

impl Tap {
    pub(crate) fn linear(c: f64, n: usize) -> Tap {
        Tap::padded(c, n, 0)
    }

    /// [`Tap::linear`] with the texels numbered in a row that repeats each edge texel `pad`
    /// times before and after: a read `pad` texels past an edge needs no clamp.
    pub(crate) fn padded(c: f64, n: usize, pad: usize) -> Tap {
        let fixed = (c as f32 * (n << 8) as f32).round_ties_even() as i32 - 128;
        let pad = pad as i32;
        let at = |i: i32| (i.clamp(-pad, n as i32 - 1 + pad) + pad) as usize;
        Tap {
            i0: at(fixed >> 8),
            i1: at((fixed >> 8) + 1),
            weight: (fixed & 255) as u16,
        }
    }

    /// Whether this is `prev`'s read one texel further: two neighbours with the same weight.
    pub(crate) fn follows(self, prev: Tap) -> bool {
        (self.i0, self.i1, self.weight) == (prev.i0 + 1, prev.i0 + 2, prev.weight)
            && prev.i1 == prev.i0 + 1
    }

    /// The texels `a` (at `i0`) and `b` (at `i1`) blended by the weight, the half rounded
    /// to even.
    #[inline]
    pub(crate) fn mix(self, a: u8, b: u8) -> u8 {
        lerp8(a, b, self.weight)
    }

    /// [`Tap::mix`] of the bytes of `a` and `b` into `out`, one weight for all of them.
    pub(crate) fn mix_bytes(self, out: &mut [u8], a: &[u8], b: &[u8]) {
        for ((out, &a), &b) in out.iter_mut().zip(a).zip(b) {
            *out = self.mix(a, b);
        }
    }

    /// Whether the coordinate is the centre of texel `i`: the read is that texel.
    pub(crate) fn is_texel(self, i: usize) -> bool {
        self.i0 == i && self.is_whole()
    }

    /// Whether the read is the texel `i0` alone: [`Tap::mix`] gives `a` for any `b`.
    pub(crate) fn is_whole(self) -> bool {
        self.weight == 0
    }

    /// GL_NEAREST: the texel `c` falls in.
    pub(crate) fn nearest(c: f64, n: usize) -> Tap {
        let i = (c * n as f64).floor().clamp(0.0, (n - 1) as f64) as usize;
        Tap {
            i0: i,
            i1: i,
            weight: 0,
        }
    }
}

/// `a` and `b` blended with `weight` of 256 (0..=255) for `b`, in 16 bits, the half rounded
/// to even: lp_build_lerp_simple (lp_bld_arit.c:1109).
#[inline]
fn lerp8(a: u8, b: u8, weight: u16) -> u8 {
    let m = weight.wrapping_mul((b as u16).wrapping_sub(a as u16));
    a.wrapping_add((m.wrapping_add(0x7f + (m >> 8 & 1)) >> 8) as u8)
}

/// The four texels around a point (two of the upper row, two of the lower) blended as
/// [`linear`] does: along x, then along y.
#[inline]
pub(crate) fn blend(a: [u8; 4], b: [u8; 4], c: [u8; 4], d: [u8; 4], x: Tap, y: Tap) -> [f32; 4] {
    blend_bytes(a, b, c, d, x, y).map(unorm)
}

#[inline]
fn blend_bytes(a: [u8; 4], b: [u8; 4], c: [u8; 4], d: [u8; 4], x: Tap, y: Tap) -> [u8; 4] {
    std::array::from_fn(|i| y.mix(x.mix(a[i], b[i]), x.mix(c[i], d[i])))
}

/// A byte of a texture as the float a shader reads: llvmpipe multiplies by 1/255 where a
/// division rounds a few of the 256 values the other way.
#[inline]
pub(crate) fn unorm(c: u8) -> f32 {
    c as f32 * (1.0 / 255.0)
}

pub fn cover(
    cv: &mut Canvas,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    shade: impl FnMut(f64, f64) -> [f32; 4],
) {
    cover_uv(cv, x, y, w, h, [0.0, 0.0, 1.0, 1.0], shade)
}

/// [`cover`] with the texture coordinates `uv` (left, top, width, height) at the quad's
/// corners: `shade` gets the coordinates at each pixel's centre, as llvmpipe interpolates
/// them over the two triangles Qt Quick draws a rectangle with (a strip: top left, bottom
/// left, top right, bottom right; QSGGeometry::updateTexturedRectGeometry).
pub(crate) fn cover_uv(
    cv: &mut Canvas,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    uv: [f64; 4],
    mut shade: impl FnMut(f64, f64) -> [f32; 4],
) {
    let x0 = (x - 0.5).ceil().max(0.0) as usize;
    let x1 = ((x + w - 0.5).ceil().max(0.0) as usize).min(cv.w);
    let (y0, y1) = cv.rows(y, h);
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    let quad = Interpolated::new([x, y, x + w, y + h], uv, [x0, y0, x1, y1]);
    for py in y0..y1 {
        for px in x0..x1 {
            let (u, v) = quad.at(px, py);
            let c = shade(u, v);
            if c == [0.0; 4] {
                continue;
            }
            cv.blend_f(px, py, c);
        }
    }
}

/// A plane a0 + dadx * x + dady * y of one value over a triangle.
#[derive(Clone, Copy)]
pub(crate) struct Plane {
    pub(crate) a0: f32,
    pub(crate) dadx: f32,
    pub(crate) dady: f32,
}

impl Plane {
    /// llvmpipe's plane (Mesa 26.2.6, calc_coef4 in src/gallium/drivers/llvmpipe/
    /// lp_state_setup.c) of the value `a` at the three corners `p` of a triangle, the first
    /// corner the one it starts from: f32, each product rounded before it is added.
    pub(crate) fn new(p: [[f32; 2]; 3], a: [f32; 3]) -> Plane {
        let [[x0, y0], [x1, y1], [x2, y2]] = p;
        let (dx01, dy01, dx20, dy20) = (x0 - x1, y0 - y1, x2 - x0, y2 - y0);
        let ooa = 1.0 / (dx01 * dy20 - dx20 * dy01);
        let (da01, da20) = (a[0] - a[1], a[2] - a[0]);
        let dadx = da01 * (dy20 * ooa) - da20 * (dy01 * ooa);
        let dady = da20 * (dx01 * ooa) - da01 * (dx20 * ooa);
        Plane {
            a0: a[0] - (dadx * (x0 - 0.5) + dady * (y0 - 0.5)),
            dadx,
            dady,
        }
    }

    /// The value at the pixel (x, y): a0, then each product added without rounding it
    /// first (interp_attrib_linear in lp_bld_interp.c: lp_build_fmuladd).
    pub(crate) fn at(self, x: usize, y: usize) -> f32 {
        self.dady
            .mul_add(y as f32, self.dadx.mul_add(x as f32, self.a0))
    }
}

/// The texture coordinates at the pixel centres of a rectangle drawn as two triangles: top
/// left, bottom left, top right, and top right, bottom left, bottom right. Each triangle has
/// its own planes, so a coordinate is not the same number on the two sides of the diagonal.
pub(crate) struct Interpolated {
    /// By triangle: u of each column and v of each row, from the first pixel of the rect.
    u: [Vec<f32>; 2],
    v: [Vec<f32>; 2],
    first: [usize; 2],
    /// The diagonal from the bottom left corner to the top right one, in 1/256 of a pixel,
    /// from the first pixel's centre: llvmpipe's corners are rounded to that grid.
    diagonal: [i64; 4],
}

impl Interpolated {
    /// `rect` is left, top, right, bottom; `uv` left, top, width, height; `px` the pixels
    /// the rect covers: first column and row, then one past the last.
    pub(crate) fn new(rect: [f64; 4], uv: [f64; 4], px: [usize; 4]) -> Interpolated {
        let [l, t, r, b] = rect.map(|c| c as f32);
        let (u0, v0, u1, v1) = (
            uv[0] as f32,
            uv[1] as f32,
            (uv[0] + uv[2]) as f32,
            (uv[1] + uv[3]) as f32,
        );
        let corners = [[[l, t], [l, b], [r, t]], [[r, t], [l, b], [r, b]]];
        let us = [[u0, u0, u1], [u1, u0, u1]];
        let vs = [[v0, v1, v0], [v0, v1, v1]];
        let along = |values: [[f32; 3]; 2], from: usize, to: usize, by_row: bool| {
            [0, 1].map(|i| {
                let plane = Plane::new(corners[i], values[i]);
                (from..to)
                    .map(|n| {
                        if by_row {
                            plane.at(px[0], n)
                        } else {
                            plane.at(n, px[1])
                        }
                    })
                    .collect::<Vec<f32>>()
            })
        };
        // subpixel_snap of lp_setup_tri.c: the corner less half a pixel, in 1/256.
        let snap = |c: f32, first: usize| {
            ((c - 0.5) * 256.0).round_ties_even() as i64 - first as i64 * 256
        };
        Interpolated {
            u: along(us, px[0], px[2], false),
            v: along(vs, px[1], px[3], true),
            first: [px[0], px[1]],
            diagonal: [
                snap(l, px[0]),
                snap(b, px[1]),
                snap(r, px[0]),
                snap(t, px[1]),
            ],
        }
    }

    /// (u, v) at the centre of the pixel (x, y). A centre on the diagonal is the second
    /// triangle's: the diagonal is its left edge.
    fn at(&self, x: usize, y: usize) -> (f64, f64) {
        let second = self.second(x, y);
        (self.u(second, x) as f64, self.v(second, y) as f64)
    }

    /// Whether the centre of the pixel (x, y) is the second triangle's.
    fn second(&self, x: usize, y: usize) -> bool {
        let (i, j) = (x - self.first[0], y - self.first[1]);
        let [lx, by, rx, ty] = self.diagonal;
        let (px, py) = (i as i64 * 256, j as i64 * 256);
        (rx - lx) * (py - by) - (ty - by) * (px - lx) >= 0
    }

    /// u at the column `x` on the first triangle or the second.
    pub(crate) fn u(&self, second: bool, x: usize) -> f32 {
        self.u[second as usize][x - self.first[0]]
    }

    /// v at the row `y` on the first triangle or the second.
    pub(crate) fn v(&self, second: bool, y: usize) -> f32 {
        self.v[second as usize][y - self.first[1]]
    }

    /// The first of the columns `x0..x1` that is the second triangle's in the row `y`, `x1`
    /// when none is: the columns before the diagonal are the first triangle's.
    pub(crate) fn split(&self, y: usize, x0: usize, x1: usize) -> usize {
        let (mut lo, mut hi) = (x0, x1);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if self.second(mid, y) {
                hi = mid;
            } else {
                lo = mid + 1;
            }
        }
        lo
    }
}

/// Plain layer: draw the texture (layer.smooth: false → nearest).
pub fn draw(cv: &mut Canvas, src: &Canvas, x: f64, y: f64, w: f64, h: f64, opacity: f32) {
    // Pixel-aligned 1:1 copy (the common case): nearest sampling hits texel (px - x, py - y),
    // and transparent texels leave the destination untouched.
    if w == src.w as f64 && h == src.h as f64 && x.fract() == 0.0 && y.fract() == 0.0 {
        let (ox, oy) = (x as i64, y as i64);
        let x0 = ox.max(0) as usize;
        let y0 = oy.max(0) as usize;
        let x1 = ((ox + src.w as i64).max(0) as usize).min(cv.w);
        let y1 = ((oy + src.h as i64).max(0) as usize).min(cv.h);
        for py in y0..y1 {
            let srow = (py as i64 - oy) as usize * src.w;
            for px in x0..x1 {
                let t = src.px[srow + (px as i64 - ox) as usize];
                if t == [0, 0, 0, 0] {
                    continue;
                }
                cv.blend_f(px, py, t.map(|c| unorm(c) * opacity));
            }
        }
        return;
    }
    cover(cv, x, y, w, h, |u, v| {
        nearest(src, u, v).map(|c| c * opacity)
    });
}

/// OpacityMask — texture(source) * texture(maskSource).a * qt_Opacity (opacitymask.frag). Both
/// SourceProxy inputs are read linearly: with no `interpolation` set, QGfxSourceProxy makes its
/// ShaderEffectSource, or the layer of a `layer.effect`, smooth (qgfxsourceproxy.cpp).
#[allow(clippy::too_many_arguments)]
pub fn draw_opacity_mask(
    cv: &mut Canvas,
    src: &Canvas,
    mask: &Canvas,
    invert: bool,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    opacity: f32,
) {
    cover(cv, x, y, w, h, |u, v| {
        let s = linear(src, u, v);
        if s == [0.0; 4] {
            return s;
        }
        let m = linear(mask, u, v)[3];
        let k = if invert { 1.0 - m } else { m } * opacity;
        s.map(|c| c * k)
    });
}

/// Qt5Compat ColorOverlay (coloroverlay.frag); `src` is the texture its SourceProxy outputs, read
/// linearly when `smooth` (ColorOverlay.qml: the source item's `smooth`) and nearest otherwise.
/// `color` is the QColor's 16 bits a channel.
#[allow(clippy::too_many_arguments)]
pub fn draw_color_overlay(
    cv: &mut Canvas,
    src: &Canvas,
    color: [u16; 4],
    smooth: bool,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    opacity: f32,
) {
    let c = crate::shadow::shader_color(color);
    cover(cv, x, y, w, h, |u, v| {
        let p = if smooth {
            linear(src, u, v)
        } else {
            nearest(src, u, v)
        };
        let pa = p[3].max(0.00390625);
        let ca = c[3].max(0.00390625);
        let mut o = [0.0f32; 4];
        for i in 0..3 {
            let a = p[i] / pa;
            let b = c[i] / ca;
            o[i] = (a * (1.0 - c[3]) + b * c[3]) * p[3] * opacity;
        }
        o[3] = p[3] * opacity;
        o
    });
}

/// Qt5Compat Desaturate (desaturate.frag): `desaturation` is used as given, outside 0..1 too;
/// `smooth` as in [`draw_color_overlay`] (Desaturate.qml).
#[allow(clippy::too_many_arguments)]
pub fn draw_desaturate(
    cv: &mut Canvas,
    src: &Canvas,
    desaturation: f32,
    smooth: bool,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    opacity: f32,
) {
    cover(cv, x, y, w, h, |u, v| {
        let t = if smooth {
            linear(src, u, v)
        } else {
            nearest(src, u, v)
        };
        let g = (t[0] + t[1] + t[2]) / 3.0;
        let gray = [g, g, g, t[3]];
        std::array::from_fn(|i| (t[i] * (1.0 - desaturation) + gray[i] * desaturation) * opacity)
    });
}

/// Texture drawn through an affine map `m` (texture px -> canvas px, [a b c d e f] like
/// QTransform m11 m12 m21 m22 dx dy): bilinear with a transparent border, as a smooth
/// QSGImageNode under a rotation/scale transform.
pub fn draw_transformed(cv: &mut Canvas, src: &Canvas, m: [f64; 6], opacity: f32) {
    let det = m[0] * m[3] - m[1] * m[2];
    if det.abs() < 1e-12 || src.w == 0 || src.h == 0 {
        return;
    }
    let inv = [
        m[3] / det,
        -m[1] / det,
        -m[2] / det,
        m[0] / det,
        (m[2] * m[5] - m[3] * m[4]) / det,
        (m[1] * m[4] - m[0] * m[5]) / det,
    ];
    let (sw, sh) = (src.w as f64, src.h as f64);
    let corners = [(0.0, 0.0), (sw, 0.0), (0.0, sh), (sw, sh)]
        .map(|(u, v)| (m[0] * u + m[2] * v + m[4], m[1] * u + m[3] * v + m[5]));
    let fx = |f: fn(f64, f64) -> f64, k: usize| {
        corners.iter().map(|c| if k == 0 { c.0 } else { c.1 }).fold(
            if f(1.0, 0.0) == 1.0 {
                f64::MIN
            } else {
                f64::MAX
            },
            f,
        )
    };
    let x0 = (fx(f64::min, 0).floor().max(0.0) as usize).min(cv.w);
    let x1 = (fx(f64::max, 0).ceil().max(0.0) as usize).min(cv.w);
    let y0 = (fx(f64::min, 1).floor().max(0.0) as usize).min(cv.h);
    let y1 = (fx(f64::max, 1).ceil().max(0.0) as usize).min(cv.h);
    let px = |x: i64, y: i64| -> [f32; 4] {
        if x < 0 || y < 0 || x >= src.w as i64 || y >= src.h as i64 {
            [0.0; 4]
        } else {
            src.px[y as usize * src.w + x as usize].map(|c| c as f32 / 255.0)
        }
    };
    for py in y0..y1 {
        for pxx in x0..x1 {
            let (cx, cy) = (pxx as f64 + 0.5, py as f64 + 0.5);
            let u = inv[0] * cx + inv[2] * cy + inv[4] - 0.5;
            let v = inv[1] * cx + inv[3] * cy + inv[5] - 0.5;
            if u < -1.0 || v < -1.0 || u > sw || v > sh {
                continue;
            }
            let (ux, vy) = (u.floor(), v.floor());
            let (ax, ay) = ((u - ux) as f32, (v - vy) as f32);
            let (ix, iy) = (ux as i64, vy as i64);
            let (a, b, c, d) = (
                px(ix, iy),
                px(ix + 1, iy),
                px(ix, iy + 1),
                px(ix + 1, iy + 1),
            );
            let col: [f32; 4] = std::array::from_fn(|k| {
                ((a[k] * (1.0 - ax) + b[k] * ax) * (1.0 - ay)
                    + (c[k] * (1.0 - ax) + d[k] * ax) * ay)
                    * opacity
            });
            if col[3] > 0.0 {
                cv.blend_f(pxx, py, col);
            }
        }
    }
}

/// Texture drawn through a projective map `m` (texture px -> canvas px; a QTransform's
/// m11..m33 by rows, points as row vectors): what `draw_transformed` does for an affine one,
/// under a Rotation about the x or y axis or a Matrix4x4 with a perspective row. The part of
/// the texture behind the viewer (w <= 0) is not drawn.
pub fn draw_projected(cv: &mut Canvas, src: &Canvas, m: [[f64; 3]; 3], opacity: f32) {
    let cof =
        |r0: usize, r1: usize, c0: usize, c1: usize| m[r0][c0] * m[r1][c1] - m[r0][c1] * m[r1][c0];
    let det = m[0][0] * cof(1, 2, 1, 2) - m[0][1] * cof(1, 2, 0, 2) + m[0][2] * cof(1, 2, 0, 1);
    if !det.is_finite() || det.abs() < 1e-12 || src.w == 0 || src.h == 0 {
        return;
    }
    // The adjugate maps canvas points back to the texture, up to a factor that divides out.
    let inv = [
        [cof(1, 2, 1, 2), -cof(0, 2, 1, 2), cof(0, 1, 1, 2)],
        [-cof(1, 2, 0, 2), cof(0, 2, 0, 2), -cof(0, 1, 0, 2)],
        [cof(1, 2, 0, 1), -cof(0, 2, 0, 1), cof(0, 1, 0, 1)],
    ];
    let (sw, sh) = (src.w as f64, src.h as f64);
    let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
    let mut behind = false;
    for (u, v) in [(0.0, 0.0), (sw, 0.0), (0.0, sh), (sw, sh)] {
        let w = m[0][2] * u + m[1][2] * v + m[2][2];
        if w <= 1e-6 {
            behind = true;
            continue;
        }
        let (x, y) = (
            (m[0][0] * u + m[1][0] * v + m[2][0]) / w,
            (m[0][1] * u + m[1][1] * v + m[2][1]) / w,
        );
        b = [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)];
    }
    // A corner behind the viewer shows towards infinity: every canvas pixel may be covered.
    if behind {
        b = [0.0, 0.0, cv.w as f64, cv.h as f64];
    }
    let x0 = (b[0].floor().max(0.0) as usize).min(cv.w);
    let x1 = (b[2].ceil().max(0.0) as usize).min(cv.w);
    let y0 = (b[1].floor().max(0.0) as usize).min(cv.h);
    let y1 = (b[3].ceil().max(0.0) as usize).min(cv.h);
    let px = |x: i64, y: i64| -> [f32; 4] {
        if x < 0 || y < 0 || x >= src.w as i64 || y >= src.h as i64 {
            [0.0; 4]
        } else {
            src.px[y as usize * src.w + x as usize].map(|c| c as f32 / 255.0)
        }
    };
    for py in y0..y1 {
        for pxx in x0..x1 {
            let (cx, cy) = (pxx as f64 + 0.5, py as f64 + 0.5);
            let k = inv[0][2] * cx + inv[1][2] * cy + inv[2][2];
            if k == 0.0 {
                continue;
            }
            let u = (inv[0][0] * cx + inv[1][0] * cy + inv[2][0]) / k;
            let v = (inv[0][1] * cx + inv[1][1] * cy + inv[2][1]) / k;
            if !(u >= -0.5 && v >= -0.5 && u <= sw + 0.5 && v <= sh + 0.5)
                || m[0][2] * u + m[1][2] * v + m[2][2] <= 1e-6
            {
                continue;
            }
            let (u, v) = (u - 0.5, v - 0.5);
            let (ux, vy) = (u.floor(), v.floor());
            let (ax, ay) = ((u - ux) as f32, (v - vy) as f32);
            let (ix, iy) = (ux as i64, vy as i64);
            let (a, b, c, d) = (
                px(ix, iy),
                px(ix + 1, iy),
                px(ix, iy + 1),
                px(ix + 1, iy + 1),
            );
            let col: [f32; 4] = std::array::from_fn(|k| {
                ((a[k] * (1.0 - ax) + b[k] * ax) * (1.0 - ay)
                    + (c[k] * (1.0 - ax) + d[k] * ax) * ay)
                    * opacity
            });
            if col[3] > 0.0 {
                cv.blend_f(pxx, py, col);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aligned_draw_matches_sampled() {
        let mut seed = 12345u32;
        let mut rnd = || {
            seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
            (seed >> 16) as u8
        };
        let mut src = Canvas::new(37, 23, [0, 0, 0, 0]);
        for p in src.px.iter_mut() {
            let a = if rnd() < 80 { 0 } else { rnd() };
            *p = [rnd().min(a), rnd().min(a), rnd().min(a), a];
        }
        let mut base = Canvas::new(60, 40, [0, 0, 0, 0]);
        for p in base.px.iter_mut() {
            *p = [rnd(), rnd(), rnd(), 255];
        }
        for (x, y, op) in [(5.0, 7.0, 1.0f32), (-3.0, 30.0, 0.6), (40.0, -4.0, 0.3)] {
            let mut a = Canvas::new(60, 40, [0, 0, 0, 0]);
            a.px.clone_from(&base.px);
            let mut b = Canvas::new(60, 40, [0, 0, 0, 0]);
            b.px.clone_from(&base.px);
            draw(&mut a, &src, x, y, 37.0, 23.0, op);
            cover(&mut b, x, y, 37.0, 23.0, |u, v| {
                nearest(&src, u, v).map(|c| c * op)
            });
            assert!(a.px == b.px, "mismatch at {x},{y}");
        }
    }
}
