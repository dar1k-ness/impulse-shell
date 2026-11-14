//! QtQuick.Effects MultiEffect (Qt 6.12 qquickmultieffect.cpp + multieffect.frag): blur (the
//! BlurItem chain and its weights, with the source's padding, [`pads`]), contrast, brightness,
//! saturation, colorization, shadow and mask.

use crate::layer::{Tap, blend};
use crate::raster::Canvas;

fn blur_weight(v: f32) -> f32 {
    (1.0 - v * 2.0).clamp(0.0, 1.0)
}

/// getBlurWeights(calculateLod(blur)) → six normalized weights (src, blurSrc1..5).
pub fn weights(blur: f32, blur_max: i32) -> [f32; 6] {
    let lod = (blur * blur_max as f32 / 64.0).sqrt() * 1.2 - 0.2;
    let bw: [f32; 6] = std::array::from_fn(|k| blur_weight((lod - (0.1 + 0.2 * k as f32)).abs()));
    let sum: f32 = bw.iter().sum();
    bw.map(|b| b / sum)
}

/// Texture as the shaders read it: 8-bit channels.
struct Tex {
    w: usize,
    h: usize,
    px: Vec<[u8; 4]>,
}

impl Tex {
    fn of(c: &Canvas) -> Tex {
        Tex {
            w: c.w,
            h: c.h,
            px: c.px.clone(),
        }
    }
}

/// `layer::linear` with its two axes precomputed per row/column.
#[inline]
fn sample(t: &Tex, x: Tap, y: Tap) -> [f32; 4] {
    let (r0, r1) = (y.i0 * t.w, y.i1 * t.w);
    blend(
        t.px[r0 + x.i0],
        t.px[r0 + x.i1],
        t.px[r1 + x.i0],
        t.px[r1 + x.i1],
        x,
        y,
    )
}

/// Pixel range `layer::cover` shades for a quad at `x` of size `w` on an axis of `len` pixels.
fn span(x: f64, w: f64, len: usize) -> (usize, usize) {
    let a = (x - 0.5).ceil().max(0.0) as usize;
    let b = ((x + w - 0.5).ceil().max(0.0) as usize).min(len);
    (a, b.max(a))
}

/// BlurItem.qml: item size may be fractional; its layer texture is round(size) (QSizeF::toSize) and the
/// item's (0..1) texture coordinates span the whole texture. The result is stored 8-bit like the FBO.
fn blur_item(src: &Tex, w: f64, h: f64, multiplier: f32) -> Tex {
    let (tw, th) = (w.round().max(1.0) as usize, h.round().max(1.0) as usize);
    let (ox, oy) = ((1.0 + multiplier) / w as f32, (1.0 + multiplier) / h as f32);
    let d = 0.33f32;
    let taps = [(ox, oy * d), (ox * d, -oy), (-ox * d, oy), (-ox, -oy * d)];
    let cols: Vec<[Tap; 4]> = (0..tw)
        .map(|px| {
            let u = (px as f64 + 0.5) / tw as f64;
            taps.map(|(dx, _)| Tap::linear(u + dx as f64, src.w))
        })
        .collect();
    let mut px = Vec::with_capacity(tw * th);
    for py in 0..th {
        let v = (py as f64 + 0.5) / th as f64;
        let rows = taps.map(|(_, dy)| Tap::linear(v + dy as f64, src.h));
        for col in &cols {
            let mut c = [0.0f32; 4];
            for k in 0..4 {
                let s = sample(src, col[k], rows[k]);
                for ch in 0..4 {
                    c[ch] += s[ch];
                }
            }
            px.push(crate::raster::bytes(c.map(|k| k / 4.0)));
        }
    }
    Tex { w: tw, h: th, px }
}

pub struct Params {
    pub blur_enabled: bool,
    pub blur: f32,
    pub blur_max: i32,
    pub blur_multiplier: f32,
    pub saturation: f32,
    pub contrast: f32,
    pub brightness: f32,
    pub colorization: f32,
    /// The QColor's 16 bits a channel.
    pub colorization_color: [u16; 4],
    pub auto_padding: bool,
    /// `paddingRect`: x and y pad the left and the top, width and height the right and bottom.
    pub padding: [f64; 4],
    pub mask: Option<Canvas>,
    /// The mask texture is read linearly (the mask item's `layer.smooth`, or its `smooth`
    /// without a layer); otherwise at its nearest pixel.
    pub mask_smooth: bool,
    pub mask_threshold_min: f32,
    pub mask_spread_at_min: f32,
    pub mask_threshold_max: f32,
    pub mask_spread_at_max: f32,
    pub mask_inverted: bool,
    pub shadow_enabled: bool,
    pub shadow_opacity: f32,
    pub shadow_blur: f32,
    pub shadow_horizontal_offset: f32,
    pub shadow_vertical_offset: f32,
    /// QColor's 16-bit channels, not premultiplied.
    pub shadow_color: [u16; 4],
    pub shadow_scale: f32,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            blur_enabled: false,
            blur: 0.0,
            blur_max: 32,
            blur_multiplier: 0.0,
            saturation: 0.0,
            contrast: 0.0,
            brightness: 0.0,
            colorization: 0.0,
            colorization_color: [65535, 0, 0, 65535],
            auto_padding: true,
            padding: [0.0; 4],
            mask: None,
            mask_smooth: true,
            mask_threshold_min: 0.0,
            mask_spread_at_min: 0.0,
            mask_threshold_max: 1.0,
            mask_spread_at_max: 0.0,
            mask_inverted: false,
            shadow_enabled: false,
            shadow_opacity: 1.0,
            shadow_blur: 1.0,
            shadow_horizontal_offset: 0.0,
            shadow_vertical_offset: 0.0,
            shadow_color: [0, 0, 0, 65535],
            shadow_scale: 1.0,
        }
    }
}

/// The shadow's uniforms (multieffect.vert/.frag SHADOW): the shader reads the source alpha at
/// `(texCoord - offset) * scale + center` and puts `color` under the item where that is opaque.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    /// 1 / shadowScale.
    pub scale: f32,
    /// The shadow offsets as fractions of the effect's size.
    pub offset: [f32; 2],
    pub center: f32,
    /// shadowColor times shadowOpacity (updateShadowColor).
    pub color: [f32; 4],
    /// Weights of the source and the blur levels there are; the source alone without levels.
    pub weights: [f32; 6],
}

/// The shadow of an effect `w` x `h` large (the source and its `pad` on each side); None
/// without `shadowEnabled`.
pub fn shadow(p: &Params, w: f64, h: f64) -> Option<Shadow> {
    if !p.shadow_enabled {
        return None;
    }
    let scale = (1.0 / p.shadow_scale as f64) as f32;
    let alpha = p.shadow_opacity.clamp(0.0, 1.0);
    let levels = blur_items(p);
    let all = weights(p.shadow_blur, p.blur_max);
    Some(Shadow {
        scale,
        offset: [
            (p.shadow_horizontal_offset as f64 / w) as f32,
            (p.shadow_vertical_offset as f64 / h) as f32,
        ],
        center: (1.0 - scale) * 0.5,
        color: p.shadow_color.map(|k| k as f32 / 65535.0 * alpha),
        weights: if levels == 0 {
            [1.0, 0.0, 0.0, 0.0, 0.0, 0.0]
        } else {
            level_weights(all, levels)
        },
    })
}

/// `all` (`weights`) with the weights of blur levels beyond the `levels` there are zeroed: the
/// shader variant for fewer levels leaves those terms out.
pub fn level_weights(all: [f32; 6], levels: usize) -> [f32; 6] {
    std::array::from_fn(|k| if k <= levels { all[k] } else { 0.0 })
}

/// Number of BlurItems (updateBlurItemsAmount): they exist for blur and for the shadow.
pub fn blur_items(p: &Params) -> usize {
    if !((p.blur_enabled || p.shadow_enabled) && p.blur_max > 0) {
        0
    } else if p.blur_max > 32 {
        5
    } else if p.blur_max > 16 {
        4
    } else {
        3
    }
}

/// Whether the source itself is drawn blurred (the shader's BLUR).
pub fn blurs_source(p: &Params) -> bool {
    p.blur_enabled && p.blur_max > 0
}

/// `colorizationColor` as the shader gets it (QQuickMultiEffectPrivate::updateColorizationColor):
/// the QColor's red, green and blue as floats, not multiplied by the alpha, and the alpha times
/// `colorization`, clamped.
fn colorization_uniform(p: &Params) -> ([f32; 3], f32) {
    let c = p.colorization_color.map(|k| k as f32 / 65535.0);
    ([c[0], c[1], c[2]], (c[3] * p.colorization).clamp(0.0, 1.0))
}

/// The non-blur colour and mask stages of `draw_with` as shader uniforms: contrast, brightness,
/// saturation, colorization strength, colorization rgb, mask smoothstep edges, inverted.
pub fn shader_uniforms(p: &Params) -> [f32; 12] {
    let (cc, cca) = colorization_uniform(p);
    let (c0, c1) = (0.0001f32, 1.0 - 0.0001f32);
    let (mt1, ms1) = (p.mask_threshold_min + c0, p.mask_spread_at_min + 1.0);
    let (mt2, ms2) = (c1 - p.mask_threshold_max, p.mask_spread_at_max + 1.0);
    [
        p.contrast,
        p.brightness,
        p.saturation,
        cca,
        cc[0],
        cc[1],
        cc[2],
        mt1 * ms1 - (ms1 - c1),
        mt1 * ms1,
        mt2 * ms2 - (ms2 - c1),
        mt2 * ms2,
        if p.mask_inverted { 1.0 } else { 0.0 },
    ]
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Transparent border added to the left, top, right and bottom of the source
/// (updateSourcePadding): autoPaddingEnabled's with blur or shadow, plus `paddingRect`'s. A
/// negative one crops. The texture is in whole pixels, so are these.
pub fn pads(p: &Params) -> [i32; 4] {
    let auto = if p.auto_padding && blur_items(p) > 0 {
        (p.blur_max as f64 * (1.0 + p.blur_multiplier as f64)) as i32
    } else {
        0
    };
    p.padding.map(|k| (k + auto as f64).round() as i32)
}

/// `src` inside the border `pads` (left, top, right, bottom) of transparent pixels.
fn padded(src: &Canvas, pads: [i32; 4]) -> Tex {
    let [l, t, r, b] = pads.map(|k| k as isize);
    let w = (src.w as isize + l + r).max(1) as usize;
    let h = (src.h as isize + t + b).max(1) as usize;
    let mut px = vec![[0u8; 4]; w * h];
    for (y, row) in px.chunks_mut(w).enumerate() {
        let sy = y as isize - t;
        if sy < 0 || sy >= src.h as isize {
            continue;
        }
        for (x, d) in row.iter_mut().enumerate() {
            let sx = x as isize - l;
            if sx >= 0 && sx < src.w as isize {
                *d = src.px[sy as usize * src.w + sx as usize];
            }
        }
    }
    Tex { w, h, px }
}

/// BlurItem sizes of the blur chain for a (padded) source texture of `sw` x `sh`; empty without
/// blur items ([`blur_items`]).
pub fn blur_sizes(p: &Params, sw: usize, sh: usize) -> Vec<(f64, f64)> {
    let (iw, ih) = (
        (sw as f64 / 16.0).ceil() * 8.0,
        (sh as f64 / 16.0).ceil() * 8.0,
    );
    (0..blur_items(p) as i32)
        .map(|i| (iw * 0.5f64.powi(i), ih * 0.5f64.powi(i)))
        .collect()
}

/// QQuickMultiEffect: `src` is the source item texture placed at scene (x, y).
pub fn draw_with(cv: &mut Canvas, src: &Canvas, p: &Params, x: f64, y: f64, opacity: f32) {
    let pads = pads(p);
    let [l, t, r, b] = pads.map(|k| k as f64);
    let (w, h) = (
        (src.w as f64 + l + r).max(1.0),
        (src.h as f64 + t + b).max(1.0),
    );
    draw_source(cv, src, pads, p, [x - l, y - t, w, h], [1.0, 1.0], opacity);
}

/// QQuickMultiEffect whose ShaderEffect covers `rect` (x y w h: the effect grown by [`pads`])
/// and reads the texture of its ShaderEffectSource, whatever its size: `src` inside `border`
/// (as `pads`; none when `src` is that texture, of the source grown by the padding in its own
/// units). `scale` is that of the texture `cv` when it is a layer's that is not its scene
/// rect's size.
pub fn draw_source(
    cv: &mut Canvas,
    src: &Canvas,
    border: [i32; 4],
    p: &Params,
    rect: [f64; 4],
    scale: [f64; 2],
    opacity: f32,
) {
    let src = if border == [0; 4] {
        Tex::of(src)
    } else {
        padded(src, border)
    };
    let (x, y) = (scale[0] * rect[0], scale[1] * rect[1]);
    let sizes = blur_sizes(p, src.w, src.h);
    let blurs_source = blurs_source(p);
    let mut items: Vec<Tex> = Vec::new();
    for (i, &(iw, ih)) in sizes.iter().enumerate() {
        let it = blur_item(
            if i == 0 { &src } else { &items[i - 1] },
            iw,
            ih,
            p.blur_multiplier,
        );
        items.push(it);
    }
    let mask = p.mask.as_ref().map(Tex::of);
    let wt = weights(p.blur, p.blur_max);
    let (cc, cca) = colorization_uniform(p);
    let (c0, c1) = (0.0001f32, 1.0 - 0.0001f32);
    let (mt1, ms1) = (p.mask_threshold_min + c0, p.mask_spread_at_min + 1.0);
    let (mt2, ms2) = (c1 - p.mask_threshold_max, p.mask_spread_at_max + 1.0);
    let mk = [
        mt1 * ms1 - (ms1 - c1),
        mt1 * ms1,
        mt2 * ms2 - (ms2 - c1),
        mt2 * ms2,
    ];
    let shadow = shadow(p, rect[2], rect[3]);
    let (sw, sh) = (scale[0] * rect[2], scale[1] * rect[3]);
    let (x0, x1) = span(x, sw, cv.w);
    let (y0, y1) = cv.rows(y, sh);
    let y1 = y1.max(y0);
    // Textures in sampling order: source, blur levels, mask.
    let mut texs: Vec<&Tex> = vec![&src];
    texs.extend(items.iter());
    texs.extend(mask.iter());
    // The shadow reads the source and the blur levels at its own coordinate (multieffect.vert).
    let shadow_at =
        |s: &Shadow, t: f64, axis: usize| ((t as f32 - s.offset[axis]) * s.scale + s.center) as f64;
    let cols: Vec<(Vec<Tap>, Vec<Tap>)> = (x0..x1)
        .map(|px| {
            let u = (px as f64 + 0.5 - x) / sw;
            let mut at: Vec<Tap> = texs.iter().map(|t| Tap::linear(u, t.w)).collect();
            if let (Some(m), false) = (&mask, p.mask_smooth) {
                *at.last_mut().unwrap() = Tap::nearest(u, m.w);
            }
            let shadow_at = shadow.as_ref().map_or(Vec::new(), |s| {
                let u = shadow_at(s, u, 0);
                texs[..=items.len()]
                    .iter()
                    .map(|t| Tap::linear(u, t.w))
                    .collect()
            });
            (at, shadow_at)
        })
        .collect();
    let cw = cv.w;
    let shade_rows = |py0: usize, out: &mut [[u8; 4]]| {
        let none = Tap::nearest(0.0, 1);
        let mut rows = vec![none; texs.len()];
        let mut srows = vec![none; items.len() + 1];
        for (r, line) in out.chunks_mut(cw).enumerate() {
            let v = ((py0 + r) as f64 + 0.5 - y) / sh;
            for (r, t) in rows.iter_mut().zip(&texs) {
                *r = Tap::linear(v, t.h);
            }
            if let (Some(m), false) = (&mask, p.mask_smooth) {
                *rows.last_mut().unwrap() = Tap::nearest(v, m.h);
            }
            if let Some(s) = &shadow {
                let v = shadow_at(s, v, 1);
                for (r, t) in srows.iter_mut().zip(&texs) {
                    *r = Tap::linear(v, t.h);
                }
            }
            for (d, (col, scol)) in line[x0..x1].iter_mut().zip(&cols) {
                // Mask coverage first: a zero factor zeroes the (finite) colour, which blends as
                // a no-op, so the source and blur taps are skipped.
                let f = mask.as_ref().map(|m| {
                    let n = texs.len() - 1;
                    let a = sample(m, col[n], rows[n])[3];
                    let mm = smoothstep(mk[0], mk[1], a) * smoothstep(mk[2], mk[3], 1.0 - a);
                    if p.mask_inverted { 1.0 - mm } else { mm }
                });
                if f == Some(0.0) {
                    continue;
                }
                let mut c = sample(&src, col[0], rows[0]);
                if blurs_source {
                    c = c.map(|k| k * wt[0]);
                    for (i, it) in items.iter().enumerate() {
                        // A zero-weight level adds exactly +0 (samples are finite).
                        if wt[i + 1] == 0.0 {
                            continue;
                        }
                        let s = sample(it, col[i + 1], rows[i + 1]);
                        for k in 0..4 {
                            c[k] += s[k] * wt[i + 1];
                        }
                    }
                }
                // Fully transparent input stays transparent through every colour stage.
                let clear = c == [0.0; 4] && p.brightness == 0.0;
                if clear && shadow.is_none() {
                    continue;
                }
                if !clear {
                    for k in 0..3 {
                        c[k] = (c[k] - 0.5 * c[3]) * (1.0 + p.contrast) + 0.5 * c[3];
                        c[k] += p.brightness * c[3];
                    }
                    let gray = c[0] * 0.299 + c[1] * 0.587 + c[2] * 0.114;
                    for k in 0..3 {
                        c[k] = gray * cc[k] * cca + c[k] * (1.0 - cca);
                        c[k] = gray + (c[k] - gray) * (1.0 + p.saturation);
                    }
                }
                if let Some(s) = &shadow {
                    let mut a = 0.0f32;
                    for (i, t) in texs[..=items.len()].iter().enumerate() {
                        if s.weights[i] != 0.0 {
                            a += sample(t, scol[i], srows[i])[3] * s.weights[i];
                        }
                    }
                    let aa = (c[3] + (1.0 - c[3]) * (1.0 - a)).min(1.0);
                    for (c, sc) in c.iter_mut().zip(s.color) {
                        *c = sc * (1.0 - aa) + *c * aa;
                    }
                }
                if let Some(f) = f {
                    c = c.map(|k| k * f);
                }
                crate::raster::blend_f(d, c.map(|k| k * opacity));
            }
        }
    };
    if y1 > y0 && x1 > x0 {
        par_rows(
            &mut cv.px[y0 * cw..y1 * cw],
            cw,
            y0,
            (x1 - x0) * (y1 - y0),
            &shade_rows,
        );
    }
}

/// Run `f(first_row, rows)` over `px` (whole rows of width `w` starting at row `y0`), split across
/// threads when the work (`n` shaded pixels) is large enough to pay for them.
fn par_rows(
    px: &mut [[u8; 4]],
    w: usize,
    y0: usize,
    n: usize,
    f: &(dyn Fn(usize, &mut [[u8; 4]]) + Sync),
) {
    let rows = px.len() / w;
    let threads = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(8)
        .min(n / 8192)
        .min(rows)
        .max(1);
    if threads == 1 {
        return f(y0, px);
    }
    let per = rows.div_ceil(threads);
    // The chunks are taken from one queue by this thread and the ones it could start: where
    // the system refuses a thread (`EAGAIN`), the others run its share.
    let chunks = std::sync::Mutex::new(px.chunks_mut(per * w).enumerate());
    let run_chunks = || {
        loop {
            let next = chunks.lock().unwrap_or_else(|e| e.into_inner()).next();
            let Some((k, chunk)) = next else { break };
            f(y0 + k * per, chunk);
        }
    };
    std::thread::scope(|s| {
        for _ in 1..threads {
            if std::thread::Builder::new()
                .spawn_scoped(s, run_chunks)
                .is_err()
            {
                break;
            }
        }
        run_chunks();
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::{cover, linear};

    /// The system refuses the threads `par_rows` splits a big area over (the limit on threads
    /// is reached): every row is still run, on the calling thread, instead of a panic.
    #[test]
    fn par_rows_runs_every_row_when_threads_are_refused() {
        let (w, rows, y0) = (256, 256, 7);
        let mark = |y: usize, px: &mut [[u8; 4]]| {
            for (k, row) in px.chunks_mut(w).enumerate() {
                row.fill([(y + k) as u8, ((y + k) >> 8) as u8, 1, 255]);
            }
        };
        let mut want = vec![[0u8; 4]; w * rows];
        mark(y0, &mut want);
        testenv::refuse_new_threads();
        let mut px = vec![[0u8; 4]; w * rows];
        par_rows(&mut px, w, y0, w * rows, &mark);
        assert!(px == want);
    }

    /// BlurItem.qml: item size may be fractional; its layer texture is round(size) (QSizeF::toSize) and the
    /// item's (0..1) texture coordinates span the whole texture.
    fn ref_blur_item(src: &Canvas, w: f64, h: f64, multiplier: f32) -> Canvas {
        let (tw, th) = (w.round().max(1.0) as usize, h.round().max(1.0) as usize);
        let mut out = Canvas::new(tw, th, [0, 0, 0, 0]);
        let (ox, oy) = ((1.0 + multiplier) / w as f32, (1.0 + multiplier) / h as f32);
        let (w, h) = (tw, th);
        let d = 0.33f32;
        let taps = [(ox, oy * d), (ox * d, -oy), (-ox * d, oy), (-ox, -oy * d)];
        cover(&mut out, 0.0, 0.0, w as f64, h as f64, |u, v| {
            let mut c = [0.0f32; 4];
            for (dx, dy) in taps {
                let s = linear(src, u + dx as f64, v + dy as f64);
                for k in 0..4 {
                    c[k] += s[k];
                }
            }
            c.map(|k| k / 4.0)
        });
        out
    }

    /// QQuickMultiEffect: `src` is the source item texture placed at scene (x, y).
    fn ref_draw_with(cv: &mut Canvas, src: &Canvas, p: &Params, x: f64, y: f64, opacity: f32) {
        let blur_items = p.blur_enabled && p.blur_max > 0;
        let pad = if p.auto_padding && blur_items {
            (p.blur_max as f32 * (1.0 + p.blur_multiplier)) as usize
        } else {
            0
        };
        let padded;
        let src = if pad > 0 {
            let mut t = Canvas::new(src.w + 2 * pad, src.h + 2 * pad, [0, 0, 0, 0]);
            crate::layer::draw(
                &mut t,
                src,
                pad as f64,
                pad as f64,
                src.w as f64,
                src.h as f64,
                1.0,
            );
            padded = t;
            &padded
        } else {
            src
        };
        let (x, y) = (x - pad as f64, y - pad as f64);
        let level = if blur_items {
            if p.blur_max > 32 {
                3
            } else if p.blur_max > 16 {
                2
            } else {
                1
            }
        } else {
            0
        };
        let mut items: Vec<Canvas> = Vec::new();
        if level > 0 {
            let (mut iw, mut ih) = (
                ((src.w as f64) / 16.0).ceil() * 8.0,
                ((src.h as f64) / 16.0).ceil() * 8.0,
            );
            for i in 0..level + 2 {
                let it = ref_blur_item(
                    if i == 0 { src } else { &items[i - 1] },
                    iw,
                    ih,
                    p.blur_multiplier,
                );
                items.push(it);
                iw *= 0.5;
                ih *= 0.5;
            }
        }
        let wt = weights(p.blur, p.blur_max);
        let (cc, cca) = colorization_uniform(p);
        let (c0, c1) = (0.0001f32, 1.0 - 0.0001f32);
        let (mt1, ms1) = (p.mask_threshold_min + c0, p.mask_spread_at_min + 1.0);
        let (mt2, ms2) = (c1 - p.mask_threshold_max, p.mask_spread_at_max + 1.0);
        let mk = [
            mt1 * ms1 - (ms1 - c1),
            mt1 * ms1,
            mt2 * ms2 - (ms2 - c1),
            mt2 * ms2,
        ];
        cover(cv, x, y, src.w as f64, src.h as f64, |u, v| {
            let mut c = linear(src, u, v);
            if level > 0 {
                c = c.map(|k| k * wt[0]);
                for (i, it) in items.iter().enumerate() {
                    let s = linear(it, u, v);
                    for k in 0..4 {
                        c[k] += s[k] * wt[i + 1];
                    }
                }
            }
            for k in 0..3 {
                c[k] = (c[k] - 0.5 * c[3]) * (1.0 + p.contrast) + 0.5 * c[3];
                c[k] += p.brightness * c[3];
            }
            let gray = c[0] * 0.299 + c[1] * 0.587 + c[2] * 0.114;
            for k in 0..3 {
                c[k] = gray * cc[k] * cca + c[k] * (1.0 - cca);
                c[k] = gray + (c[k] - gray) * (1.0 + p.saturation);
            }
            if let Some(m) = &p.mask {
                let a = linear(m, u, v)[3];
                let mm = smoothstep(mk[0], mk[1], a) * smoothstep(mk[2], mk[3], 1.0 - a);
                let f = if p.mask_inverted { 1.0 - mm } else { mm };
                c = c.map(|k| k * f);
            }
            c.map(|k| k * opacity)
        });
    }

    fn noise(w: usize, h: usize, seed: u32) -> Canvas {
        let mut c = Canvas::new(w, h, [0, 0, 0, 0]);
        let mut s = seed;
        for p in c.px.iter_mut() {
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            let a = (s >> 24) as u8;
            let r = ((s >> 16) as u8).min(a);
            let g = ((s >> 8) as u8).min(a);
            *p = [r, g, a / 2, a];
        }
        c
    }

    #[test]
    fn fast_path_matches_reference() {
        for (k, (bm, blur, sat, x, y, masked)) in [
            (7, 1.0, 0.2, 8.0, 8.0, false),
            (100, 1.0, 0.2, 3.25, 5.5, false),
            (32, 0.6, -0.4, 0.0, 0.0, true),
            (20, 0.3, 0.0, -10.5, 2.0, false),
        ]
        .into_iter()
        .enumerate()
        {
            let src = noise(97, 41, k as u32 + 1);
            let p = Params {
                blur_enabled: true,
                blur,
                blur_max: bm,
                saturation: sat,
                contrast: 0.1,
                colorization: 0.3,
                mask: masked.then(|| noise(97, 41, 99)),
                ..Default::default()
            };
            let mut a = noise(160, 120, 7);
            let mut b = Canvas::new(a.w, a.h, [0, 0, 0, 0]);
            b.px = a.px.clone();
            draw_with(&mut a, &src, &p, x, y, 0.8);
            ref_draw_with(&mut b, &src, &p, x, y, 0.8);
            assert!(a.px == b.px, "case {k}");
        }
    }
}
