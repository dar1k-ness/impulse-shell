//! QtQuick.Effects RectangularShadow (Qt 6.12 qquickrectangularshadow.cpp + rectangularshadow.frag).

use crate::layer::unorm;
use crate::raster::Canvas;

/// A colour property as a ShaderEffect's shader gets it (qsgrhishadereffectnode.cpp,
/// qsg_premultiply_color): the floats of QColor's 16-bit channels times the alpha, made a QColor
/// again (16 bits each, rounded) and read as floats. `c` is red, green, blue and alpha.
pub fn shader_color(c: [u16; 4]) -> [f32; 4] {
    let f = c.map(|k| k as f32 / 65535.0);
    let q = |v: f32| (v * 65535.0 + 0.5).floor() / 65535.0;
    [q(f[0] * f[3]), q(f[1] * f[3]), q(f[2] * f[3]), f[3]]
}

#[derive(Clone, Debug)]
pub struct ShadowParams {
    pub width: f64,
    pub height: f64,
    pub radius: f64,
    /// The top left, top right, bottom left and bottom right radius when QML set one of
    /// them (useIndividualRadius(): rectangularshadow4r.frag draws the shadow).
    pub corners: Option<[f64; 4]>,
    pub blur: f64,
    pub spread: f64,
    pub offset: (f64, f64),
    /// QColor's 16-bit channels, not premultiplied.
    pub color: [u16; 4],
    pub antialiasing: bool,
    pub cached: bool,
}

impl ShadowParams {
    fn clamped_radius(&self, radius: f64) -> f64 {
        let mut max_radius = self.width.min(self.height) * 0.5 + self.spread * 2.0;
        let mut spread_radius = radius + self.spread;
        if radius < self.spread && self.spread.abs() > 1e-12 {
            let r = radius / self.spread - 1.0;
            spread_radius = radius + self.spread * (1.0 + r * r * r);
        }
        let blur_reduce = self.blur * 0.75;
        max_radius -= blur_reduce;
        (spread_radius - blur_reduce).max(0.0).min(max_radius)
    }

    /// clampedRadius4R(), or clampedRadius() for every corner.
    fn radii(&self) -> [f32; 4] {
        match self.corners {
            Some(c) => c.map(|r| self.clamped_radius(r) as f32),
            None => [self.clamped_radius(self.radius) as f32; 4],
        }
    }

    /// rectangularshadow4r.frag for the fragment at (fx, fy) of the effect item: the
    /// coverage of the rounded box whose corner the fragment's quadrant picks.
    fn coverage4(&self) -> impl Fn(f32, f32) -> f32 + use<> {
        let blur = self.blur.max(0.0);
        let aa = if self.antialiasing { 1.0 } else { 0.0 };
        let pad = blur * 2.0 + self.spread * 2.0;
        let res = [(self.width + pad) as f32, (self.height + pad) as f32];
        let blur_red = blur * 1.8 + aa;
        let rect = [
            ((self.width + pad) * 0.5 - blur_red) as f32,
            ((self.height + pad) * 0.5 - blur_red) as f32,
        ];
        let radii = self.radii();
        let sblur = (blur * 2.1 + aa) as f32;
        // GLSL sign(): 0 at 0.
        let sign = |v: f32| {
            if v > 0.0 {
                1.0
            } else if v < 0.0 {
                -1.0
            } else {
                0.0
            }
        };
        move |fx, fy| {
            let (cx, cy) = (fx - res[0] * 0.5, fy - res[1] * 0.5);
            let r = radii[(2.0 + 0.5 * sign(cx) + sign(cy)) as usize];
            let qx = (cx.abs() - rect[0] + r).max(0.0);
            let qy = (cy.abs() - rect[1] + r).max(0.0);
            let v = 1.0 - smoothstep(0.0, sblur, (qx * qx + qy * qy).sqrt() - r);
            v * v
        }
    }
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    // GLSL leaves equal edges undefined; Qt's shader on Mesa and ours on the GPU step there.
    if e1 <= e0 {
        return if x > e0 { 1.0 } else { 0.0 };
    }
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Draw the shadow of an item whose top-left is at scene (x, y).
/// `cached: true` (layer.enabled): the shadow rendered into its 8-bit layer texture, the
/// effect's size times the window's devicePixelRatio `dpr` rounded once
/// ([`crate::layer::texture_size`]), and the rect (relative to the item) Qt draws it into with
/// nearest sampling.
pub fn cached_tex(p: &ShadowParams, dpr: f64) -> Option<(Canvas, [f64; 4])> {
    if !p.cached || p.width <= 0.0 || p.height <= 0.0 {
        return None;
    }
    let pad = p.blur.max(0.0) * 2.0 + p.spread * 2.0;
    let (ew, eh) = (p.width + pad, p.height + pad);
    let (w, h, px) = layer_texels(p, dpr);
    let (ex, ey) = (
        (p.width - ew) * 0.5 + p.offset.0,
        (p.height - eh) * 0.5 + p.offset.1,
    );
    // QQuickShaderEffectSource::updatePaintNode: the layer is drawn over its texture's size,
    // one device pixel more where the effect starts inside a device pixel.
    let target = |at: f64, texels: usize| {
        let d = at * dpr;
        ((d - d.floor()).ceil() + texels as f64) / dpr
    };
    let r = [ex, ey, target(ex, w), target(ey, h)];
    Some((Canvas::from_px(w, h, px), r))
}

fn layer_texels(p: &ShadowParams, dpr: f64) -> (usize, usize, Vec<[u8; 4]>) {
    let blur = p.blur.max(0.0);
    let aa = if p.antialiasing { 1.0 } else { 0.0 };
    let pad = blur * 2.0 + p.spread * 2.0;
    let (ew, eh) = (p.width + pad, p.height + pad);
    let blur_red = blur * 1.8 + aa;
    let rect = [(ew * 0.5 - blur_red) as f32, (eh * 0.5 - blur_red) as f32];
    let radius = p.radii()[0];
    let sblur = (blur * 2.1 + aa) as f32;
    let res = [ew as f32, eh as f32];
    let col = shader_color(p.color);
    // The shape is separable: q depends on one axis each, and wherever one of them is 0 the
    // value is a function of the other axis only, so only corner pixels need the full formula.
    let qx_of = |fx: f32| ((fx - res[0] * 0.5).abs() - rect[0] + radius).max(0.0);
    let qy_of = |fy: f32| ((fy - res[1] * 0.5).abs() - rect[1] + radius).max(0.0);
    let kq = |qx: f32, qy: f32| {
        let bx = (qx * qx + qy * qy).sqrt() - radius;
        let v = 1.0 - smoothstep(0.0, sblur, bx);
        v * v
    };
    let (tw, th) = crate::layer::texture_size(ew * dpr, eh * dpr);
    let (sx, sy) = (ew / tw as f64, eh / th as f64);
    if p.corners.is_some() {
        let k = p.coverage4();
        let tex = (0..tw * th)
            .map(|n| {
                let k = k(
                    (((n % tw) as f64 + 0.5) * sx) as f32,
                    (((n / tw) as f64 + 0.5) * sy) as f32,
                );
                crate::raster::bytes([col[0] * k, col[1] * k, col[2] * k, col[3] * k])
            })
            .collect();
        return (tw, th, tex);
    }
    let qx: Vec<f32> = (0..tw)
        .map(|tx| qx_of(((tx as f64 + 0.5) * sx) as f32))
        .collect();
    let qy: Vec<f32> = (0..th)
        .map(|ty| qy_of(((ty as f64 + 0.5) * sy) as f32))
        .collect();
    let mut tex = vec![[0u8; 4]; tw * th];
    // Rows with qy == 0 all equal the first such row: compute it once, copy the rest.
    let flat = qy.iter().position(|&b| b == 0.0);
    let qy_sparse: Vec<f32> = qy
        .iter()
        .enumerate()
        .map(|(j, &b)| {
            if b == 0.0 && Some(j) != flat {
                f32::NAN
            } else {
                b
            }
        })
        .collect();
    grid(&qx, &qy_sparse, kq, |tx, ty, k| {
        if !qy_sparse[ty].is_nan() {
            let c = [col[0] * k, col[1] * k, col[2] * k, col[3] * k];
            tex[ty * tw + tx] = crate::raster::bytes(c);
        }
    });
    if let Some(f) = flat {
        for (ty, q) in qy_sparse.iter().enumerate().take(th) {
            if q.is_nan() {
                tex.copy_within(f * tw..(f + 1) * tw, ty * tw);
            }
        }
    }
    (tw, th, tex)
}

/// The uncached shadow as rectangularshadow.frag evaluates it per fragment: the effect quad
/// (scene x y w h) and its uniforms.
pub struct Shader {
    pub rect: [f64; 4],
    /// Item size, reduced half size of the rounded rect, blur.
    pub uniforms: [f32; 5],
    /// Top left, top right, bottom left and bottom right radius.
    pub radii: [f32; 4],
    /// Premultiplied colour.
    pub color: [f32; 4],
}

/// `draw`'s shape for an item at scene (x, y), for the GPU to evaluate per fragment.
pub fn shader(p: &ShadowParams, x: f64, y: f64) -> Option<Shader> {
    if p.width <= 0.0 || p.height <= 0.0 {
        return None;
    }
    let blur = p.blur.max(0.0);
    let aa = if p.antialiasing { 1.0 } else { 0.0 };
    let pad = blur * 2.0 + p.spread * 2.0;
    let (ew, eh) = (p.width + pad, p.height + pad);
    let ex = x + (p.width - ew) * 0.5 + p.offset.0;
    let ey = y + (p.height - eh) * 0.5 + p.offset.1;
    let blur_red = blur * 1.8 + aa;
    Some(Shader {
        rect: [ex, ey, ew, eh],
        uniforms: [
            ew as f32,
            eh as f32,
            (ew * 0.5 - blur_red) as f32,
            (eh * 0.5 - blur_red) as f32,
            (blur * 2.1 + aa) as f32,
        ],
        radii: p.radii(),
        color: shader_color(p.color),
    })
}

pub fn draw(cv: &mut Canvas, p: &ShadowParams, x: f64, y: f64, opacity: f32) {
    if p.width <= 0.0 || p.height <= 0.0 {
        return;
    }
    let blur = p.blur.max(0.0);
    let aa = if p.antialiasing { 1.0 } else { 0.0 };
    let pad = blur * 2.0 + p.spread * 2.0;
    let (ew, eh) = (p.width + pad, p.height + pad);
    let ex = x + (p.width - ew) * 0.5 + p.offset.0;
    let ey = y + (p.height - eh) * 0.5 + p.offset.1;
    let blur_red = blur * 1.8 + aa;
    let rect = [(ew * 0.5 - blur_red) as f32, (eh * 0.5 - blur_red) as f32];
    let radius = p.radii()[0];
    let sblur = (blur * 2.1 + aa) as f32;
    let res = [ew as f32, eh as f32];
    let col = shader_color(p.color);
    // The shape is separable: q depends on one axis each, and wherever one of them is 0 the
    // value is a function of the other axis only, so only corner pixels need the full formula.
    let qx_of = |fx: f32| ((fx - res[0] * 0.5).abs() - rect[0] + radius).max(0.0);
    let qy_of = |fy: f32| ((fy - res[1] * 0.5).abs() - rect[1] + radius).max(0.0);
    let kq = |qx: f32, qy: f32| {
        let bx = (qx * qx + qy * qy).sqrt() - radius;
        let v = 1.0 - smoothstep(0.0, sblur, bx);
        v * v
    };
    if p.cached {
        // layer.enabled: render into an 8-bit texture, then draw with nearest sampling.
        let (tw, th, tex) = layer_texels(p, 1.0);
        // The rect the layer is drawn over (`cached_tex`): its texture's size, one pixel more
        // where the effect starts inside a pixel.
        let target = |at: f64, texels: usize| (at - at.floor()).ceil() + texels as f64;
        let (ew, eh) = (target(ex - x, tw), target(ey - y, th));
        // Nearest sampling: texel floor(u * n). Exact .5 ties are GPU-dependent (llvmpipe rounds either way).
        let texel = |u: f64, n: usize| ((u * n as f64).floor().max(0.0) as usize).min(n - 1);
        // `cover` with the texel lookups hoisted: rows sampling the same texel row are identical.
        let (x0, x1) = cv.cols(ex, ew);
        let (y0, y1) = cv.rows(ey, eh);
        let cols: Vec<usize> = (x0..x1)
            .map(|px| texel((px as f64 + 0.5 - ex) / ew, tw))
            .collect();
        // A texel scaled by opacity, and that quantized (blend_f over a transparent pixel).
        let lf: Vec<f32> = (0..=255).map(|c| unorm(c) * opacity).collect();
        let lq: Vec<u8> = lf
            .iter()
            .map(|&v| crate::raster::bytes([v; 4])[0])
            .collect();
        let mut rowq: Vec<[u8; 4]> = Vec::with_capacity(cols.len());
        let mut last = usize::MAX;
        for py in y0..y1 {
            let ty = texel((py as f64 + 0.5 - ey) / eh, th);
            if ty != last {
                last = ty;
                let t = &tex[ty * tw..(ty + 1) * tw];
                rowq.clear();
                rowq.extend(cols.iter().map(|&tx| {
                    let c = t[tx];
                    [
                        lq[c[0] as usize],
                        lq[c[1] as usize],
                        lq[c[2] as usize],
                        lq[c[3] as usize],
                    ]
                }));
            }
            let base = py * cv.w + x0;
            let dst = &mut cv.px[base..base + rowq.len()];
            if dst.iter().all(|d| *d == [0; 4]) {
                dst.copy_from_slice(&rowq);
                continue;
            }
            for (i, &tx) in cols.iter().enumerate() {
                if cv.px[base + i] == [0; 4] {
                    cv.px[base + i] = rowq[i];
                } else {
                    let c = tex[ty * tw + tx];
                    let f = [
                        lf[c[0] as usize],
                        lf[c[1] as usize],
                        lf[c[2] as usize],
                        lf[c[3] as usize],
                    ];
                    cv.blend_f(x0 + i, py, f);
                }
            }
        }
    } else {
        let (x0, x1) = cv.cols(ex, ew);
        let (y0, y1) = cv.rows(ey, eh);
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        if p.corners.is_some() {
            let k = p.coverage4();
            for py in y0..y1 {
                for px in x0..x1 {
                    let k = k(
                        ((px as f64 + 0.5 - ex) / ew * ew) as f32,
                        ((py as f64 + 0.5 - ey) / eh * eh) as f32,
                    );
                    if k != 0.0 {
                        let c = [col[0] * k, col[1] * k, col[2] * k, col[3] * k];
                        cv.blend_f(px, py, c.map(|c| c * opacity));
                    }
                }
            }
            return;
        }
        let qx: Vec<f32> = (x0..x1)
            .map(|px| qx_of(((px as f64 + 0.5 - ex) / ew * ew) as f32))
            .collect();
        let qy: Vec<f32> = (y0..y1)
            .map(|py| qy_of(((py as f64 + 0.5 - ey) / eh * eh) as f32))
            .collect();
        grid(&qx, &qy, kq, |i, j, k| {
            if k != 0.0 {
                let c = [col[0] * k, col[1] * k, col[2] * k, col[3] * k];
                cv.blend_f(x0 + i, y0 + j, c.map(|c| c * opacity));
            }
        });
    }
}

/// Visit every (column, row) of a separable field `kq(qx[i], qy[j])`, computing it once per
/// axis where the other axis is 0.
fn grid(
    qx: &[f32],
    qy: &[f32],
    kq: impl Fn(f32, f32) -> f32,
    mut put: impl FnMut(usize, usize, f32),
) {
    let k00 = kq(0.0, 0.0);
    let kx: Vec<f32> = qx.iter().map(|&a| kq(a, 0.0)).collect();
    for (j, &b) in qy.iter().enumerate() {
        // NaN marks a row the caller fills itself.
        if b.is_nan() {
            continue;
        }
        let ky = kq(0.0, b);
        for (i, &a) in qx.iter().enumerate() {
            let k = match (a == 0.0, b == 0.0) {
                (true, true) => k00,
                (true, false) => ky,
                (false, true) => kx[i],
                (false, false) => kq(a, b),
            };
            put(i, j, k);
        }
    }
}

#[cfg(test)]
/// Rasterize an axis-aligned quad (pixel centers inside [x, x+w) x [y, y+h)), shading with normalized (u, v).
fn cover(
    cv: &mut Canvas,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    mut shade: impl FnMut(f64, f64) -> [f32; 4],
) {
    let (x0, x1) = cv.cols(x, w);
    let (y0, y1) = cv.rows(y, h);
    for py in y0..y1 {
        for px in x0..x1 {
            let c = shade((px as f64 + 0.5 - x) / w, (py as f64 + 0.5 - y) / h);
            cv.blend_f(px, py, c);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Qt.rgba(0, 0, 0, 0.3)` reaches the shader as 19661 / 65535, not as 77 / 255, and a
    /// channel times the alpha is rounded to 16 bits once more.
    #[test]
    fn shader_color_keeps_sixteen_bits() {
        let c = shader_color([65535, 32768, 0, 19661]);
        assert_eq!(c[3], 19661.0 / 65535.0);
        assert_ne!(c[3], 77.0 / 255.0);
        assert_eq!(c[0], c[3]);
        assert_eq!(c[1], 9831.0 / 65535.0);
        assert_eq!(shader_color([255 * 257; 4]), [1.0; 4]);
    }

    /// The straightforward per-pixel version the separable one must reproduce exactly.
    fn draw_ref(cv: &mut Canvas, p: &ShadowParams, x: f64, y: f64, opacity: f32) {
        if p.width <= 0.0 || p.height <= 0.0 {
            return;
        }
        let blur = p.blur.max(0.0);
        let aa = if p.antialiasing { 1.0 } else { 0.0 };
        let pad = blur * 2.0 + p.spread * 2.0;
        let (ew, eh) = (p.width + pad, p.height + pad);
        let ex = x + (p.width - ew) * 0.5 + p.offset.0;
        let ey = y + (p.height - eh) * 0.5 + p.offset.1;
        let blur_red = blur * 1.8 + aa;
        let rect = [(ew * 0.5 - blur_red) as f32, (eh * 0.5 - blur_red) as f32];
        let radius = p.radii()[0];
        let sblur = (blur * 2.1 + aa) as f32;
        let res = [ew as f32, eh as f32];
        let col = shader_color(p.color);
        let frag = |fx: f32, fy: f32| -> [f32; 4] {
            let (cx, cy) = (fx - res[0] * 0.5, fy - res[1] * 0.5);
            let qx = (cx.abs() - rect[0] + radius).max(0.0);
            let qy = (cy.abs() - rect[1] + radius).max(0.0);
            let bx = (qx * qx + qy * qy).sqrt() - radius;
            let v = 1.0 - smoothstep(0.0, sblur, bx);
            let k = v * v;
            [col[0] * k, col[1] * k, col[2] * k, col[3] * k]
        };
        if p.cached {
            // layer.enabled: render into an 8-bit texture, then draw with nearest sampling.
            let (tw, th) = crate::layer::texture_size(ew, eh);
            let (sx, sy) = (ew / tw as f64, eh / th as f64);
            let mut tex = vec![[0u8; 4]; tw * th];
            for ty in 0..th {
                for tx in 0..tw {
                    let c = frag(
                        ((tx as f64 + 0.5) * sx) as f32,
                        ((ty as f64 + 0.5) * sy) as f32,
                    );
                    tex[ty * tw + tx] = crate::raster::bytes(c);
                }
            }
            // Nearest sampling: texel floor(u * n). Exact .5 ties are GPU-dependent (llvmpipe rounds either way).
            let texel = |u: f64, n: usize| ((u * n as f64).floor().max(0.0) as usize).min(n - 1);
            cover(cv, ex, ey, ew, eh, |u, v| {
                let t = tex[texel(v, th) * tw + texel(u, tw)];
                t.map(|c| unorm(c) * opacity)
            });
        } else {
            cover(cv, ex, ey, ew, eh, |u, v| {
                frag((u * ew) as f32, (v * eh) as f32).map(|c| c * opacity)
            });
        }
    }

    /// The cache texture of a shadow whose effect item (size + 2 * (blur + spread)) has a
    /// fractional size is the size Qt 6.12 gives that item's layer, measured on real Qt
    /// (qquickshadereffectsource.cpp: `QSizeF::toSize().expandedTo(QSize(1, 1))`, which rounds;
    /// up to Qt 6.11 it was `qCeil`).
    #[test]
    fn cached_texture_size_is_qts_layer_size() {
        for (w, h, qt) in [
            (120.4, 80.6, (129, 90)),
            (120.5, 80.5, (130, 90)),
            (121.5, 81.5, (131, 91)),
            (0.3, 0.6, (9, 10)),
            (33.49, 33.51, (42, 43)),
        ] {
            let p = ShadowParams {
                width: w,
                height: h,
                radius: 4.0,
                corners: None,
                blur: 3.3,
                spread: 1.2,
                offset: (0.0, 0.0),
                color: [0, 0, 0, 255 * 257],
                antialiasing: true,
                cached: true,
            };
            let (tex, r) = cached_tex(&p, 1.0).unwrap();
            assert_eq!((tex.w, tex.h), qt, "{w}x{h}");
            // The same size as an item of the effect's size gets for its layer, which Qt
            // draws over that many pixels and one more: the effect starts at -4.5.
            let size = crate::layer::texture_size(w + 9.0, h + 9.0);
            assert_eq!((tex.w, tex.h), size);
            assert_eq!((r[2], r[3]), (size.0 as f64 + 1.0, size.1 as f64 + 1.0));
        }
    }

    #[test]
    fn separable_matches_reference() {
        for (i, &(w, h, r, b, sp, cached, op)) in [
            (515.0, 225.3, 16.0, 10.0, 0.0, false, 1.0f32),
            (300.0, 40.0, 20.0, 6.0, 2.0, true, 0.7),
            (120.5, 260.0, 4.0, 0.0, 0.0, false, 0.5),
            (64.0, 64.0, 32.0, 14.0, 1.0, false, 1.0),
            (64.0, 64.0, 32.0, 14.0, 1.0, true, 1.0),
        ]
        .iter()
        .enumerate()
        {
            let p = ShadowParams {
                width: w,
                height: h,
                radius: r,
                corners: None,
                blur: b,
                spread: sp,
                offset: (0.0, 2.0),
                color: [0, 0, 0, 120 * 257],
                antialiasing: i % 2 == 0,
                cached,
            };
            for bg in [[40, 30, 50, 255], [0, 0, 0, 0]] {
                let (mut a, mut c) = (Canvas::new(700, 400, bg), Canvas::new(700, 400, bg));
                draw(&mut a, &p, 60.3, 50.7, op);
                draw_ref(&mut c, &p, 60.3, 50.7, op);
                assert!(a.px == c.px, "case {i} bg {bg:?}");
            }
        }
    }
}

#[cfg(test)]
mod cache_tests {
    /// The cached path over arbitrary backgrounds and opacities equals blending the opacity-1
    /// texture (which over transparent is the texel itself) pixel by pixel.
    #[test]
    fn cached_cover_matches_per_pixel_blend() {
        for (op, off) in [
            (1.0f32, (0.0, 1.0)),
            (0.37, (2.5, -1.25)),
            (0.8, (0.0, 0.0)),
        ] {
            let p = super::ShadowParams {
                width: 61.0,
                height: 47.5,
                radius: 9.0,
                corners: None,
                blur: 6.0,
                spread: 1.0,
                offset: off,
                color: [10 * 257, 20 * 257, 200 * 257, 120 * 257],
                antialiasing: false,
                cached: true,
            };
            let (w, h) = (100, 90);
            let mut unit = crate::raster::Canvas::new(w, h, [0, 0, 0, 0]);
            super::draw(&mut unit, &p, 17.3, 19.6, 1.0);
            let mut bg = crate::raster::Canvas::new(w, h, [0, 0, 0, 0]);
            for y in 0..h {
                for x in 0..w {
                    if (x / 7 + y / 5) % 3 != 0 {
                        bg.px[y * w + x] = [(x * 2) as u8, (y * 2) as u8, 90, 200];
                    }
                }
            }
            let mut want = crate::raster::Canvas {
                w,
                h,
                px: bg.px.clone(),
                key: Default::default(),
            };
            for y in 0..h {
                for x in 0..w {
                    let c = unit.px[y * w + x];
                    if c != [0; 4] {
                        want.blend_f(x, y, c.map(|v| crate::layer::unorm(v) * op));
                    }
                }
            }
            let mut got = crate::raster::Canvas {
                w,
                h,
                px: bg.px.clone(),
                key: Default::default(),
            };
            super::draw(&mut got, &p, 17.3, 19.6, op);
            assert!(got.px == want.px, "opacity {op}");
        }
    }
}
