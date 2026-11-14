//! QPainter on the raster paint engine, as far as QQuickContext2DCommandBuffer::replay uses it:
//! a world transform, opacity, a composition mode, an antialiased clip, and fills and strokes
//! with solid, gradient, texture and pattern brushes on ARGB32_Premultiplied.
//!
//! Spans come from the existing ports (qgrayraster, QRasterizer::rasterizeLine, QStroker,
//! QCosmeticStroker); this module ports what turns a span into pixels: QSpanData
//! (qpaintengine_raster.cpp), the gradient fetchers (qdrawhelper.cpp) and the composition
//! functions (qcompositionfunctions.cpp).

use crate::canvas::{self, Cap, Join, Path};
use crate::raster::Canvas;
use mul255::{qt as byte_mul, qt_div_255 as div_255};
use std::rc::Rc;

/// A gradient stop: position and QColor::rgba64 (straight alpha).
pub type Stop = (f64, [u16; 4]);
pub type M = [f64; 6];

const IDENTITY: M = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
const TABLE: usize = 1024;

/// QBrush.
#[derive(Clone)]
pub enum Brush {
    /// Qt::NoBrush.
    None,
    /// Straight RGBA.
    Solid([u8; 4]),
    /// QLinearGradient(x0, y0, x1, y1).
    Linear([f64; 4], Vec<Stop>),
    /// QRadialGradient: centre x, y, radius, focal x, y, radius.
    Radial([f64; 6], Vec<Stop>),
    /// QConicalGradient(x, y, angle in degrees).
    Conical([f64; 3], Vec<Stop>),
    /// A tiled image (premultiplied).
    Texture(Rc<Canvas>),
    /// QBrush(color, Qt::BrushStyle) for Dense1Pattern..DiagCrossPattern (2..=14).
    Pattern([u8; 4], u8),
}

/// QPen with a non-cosmetic width.
#[derive(Clone)]
pub struct Pen {
    pub brush: Brush,
    pub width: f64,
    pub cap: Cap,
    pub join: Join,
    pub miter_limit: f64,
    /// In pen widths, as QPen keeps it; empty for a solid line.
    pub dash: Vec<f64>,
    pub dash_offset: f64,
    /// Qt::CustomDashLine with an empty pattern (QPen::setDashOffset on a solid pen): the
    /// cosmetic stroker draws a solid line, QDashStroker draws nothing.
    pub no_dashes: bool,
}

/// INTERPOLATE_PIXEL_255 on one channel.
fn interp(x: u32, a: u32, y: u32, b: u32) -> u32 {
    let t = x * a + y * b;
    ((t + ((t >> 8) & 0xff) + 0x80) >> 8) & 0xff
}

type Px = [u32; 4];

fn each(f: impl Fn(usize) -> u32) -> Px {
    [f(0), f(1), f(2), f(3)]
}

/// comp_func_<mode>(dest, src, 1, const_alpha) on premultiplied pixels; `mode` is
/// QPainter::CompositionMode.
fn comp(mode: u8, d: Px, s: Px, ca: u32) -> Px {
    comp_px(mode, d, s, ca, false)
}

fn comp_px(mode: u8, d: Px, s: Px, ca: u32, solid: bool) -> Px {
    let cia = 255 - ca;
    let scaled = |s: Px| {
        if ca == 255 {
            s
        } else {
            s.map(|v| byte_mul(v, ca))
        }
    };
    match mode {
        // SourceOver
        0 => {
            let s = scaled(s);
            let ia = 255 - s[3];
            each(|k| s[k] + byte_mul(d[k], ia))
        }
        // DestinationOver
        1 => {
            let s = scaled(s);
            let ia = 255 - d[3];
            each(|k| d[k] + byte_mul(s[k], ia))
        }
        // Clear
        2 => {
            if ca == 255 {
                [0; 4]
            } else {
                d.map(|v| byte_mul(v, cia))
            }
        }
        // Source
        3 => {
            if ca == 255 {
                s
            } else {
                each(|k| interp(s[k], ca, d[k], cia))
            }
        }
        // Destination
        4 => d,
        // SourceIn
        5 => {
            if ca == 255 {
                s.map(|v| byte_mul(v, d[3]))
            } else {
                let s = scaled(s);
                each(|k| interp(s[k], d[3], d[k], cia))
            }
        }
        // DestinationIn
        6 => {
            let a = if ca == 255 {
                s[3]
            } else {
                byte_mul(s[3], ca) + cia
            };
            d.map(|v| byte_mul(v, a))
        }
        // SourceOut
        7 => {
            if ca == 255 {
                s.map(|v| byte_mul(v, 255 - d[3]))
            } else {
                let s = scaled(s);
                each(|k| interp(s[k], 255 - d[3], d[k], cia))
            }
        }
        // DestinationOut
        8 => {
            let a = if ca == 255 {
                255 - s[3]
            } else {
                byte_mul(255 - s[3], ca) + cia
            };
            d.map(|v| byte_mul(v, a))
        }
        // SourceAtop
        9 => {
            let s = scaled(s);
            each(|k| interp(s[k], d[3], d[k], 255 - s[3]))
        }
        // DestinationAtop
        10 => {
            if ca == 255 {
                each(|k| interp(s[k], 255 - d[3], d[k], s[3]))
            } else {
                let s = scaled(s);
                let a = s[3] + cia;
                each(|k| interp(s[k], 255 - d[3], d[k], a))
            }
        }
        // Xor
        11 => {
            let s = scaled(s);
            each(|k| interp(s[k], 255 - d[3], d[k], 255 - s[3]))
        }
        // Plus
        12 => {
            let r = each(|k| (s[k] + d[k]).min(255));
            if ca == 255 {
                r
            } else {
                each(|k| interp(r[k], ca, d[k], cia))
            }
        }
        13..=23 => {
            let (da, sa) = (d[3] as i64, s[3] as i64);
            let op = |d: i64, s: i64| -> i64 {
                let temp = s * (255 - da) + d * (255 - sa);
                match mode {
                    13 => div_255(s * d + temp),
                    14 => 255 - div_255((255 - d) * (255 - s)),
                    15 => {
                        if 2 * d < da {
                            div_255(2 * s * d + temp)
                        } else {
                            div_255(sa * da - 2 * (da - d) * (sa - s) + temp)
                        }
                    }
                    16 => div_255((s * da).min(d * sa) + temp),
                    17 => div_255((s * da).max(d * sa) + temp),
                    18 => {
                        if s * da + d * sa > sa * da {
                            div_255(sa * da + temp)
                        } else if s == sa || sa == 0 {
                            div_255(temp)
                        } else {
                            div_255(255 * d * sa / (255 - 255 * s / sa) + temp)
                        }
                    }
                    19 => {
                        if s * da + d * sa < sa * da {
                            div_255(temp)
                        } else if s == 0 {
                            div_255(d * sa + temp)
                        } else {
                            div_255(sa * (s * da + d * sa - sa * da) / s + temp)
                        }
                    }
                    20 => {
                        if 2 * s < sa {
                            div_255(2 * s * d + temp)
                        } else {
                            div_255(sa * da - 2 * (da - d) * (sa - s) + temp)
                        }
                    }
                    21 => {
                        let s2 = s << 1;
                        let dnp = if da != 0 { 255 * d / da } else { 0 };
                        let temp = temp * 255;
                        if s2 < sa {
                            (d * (sa * 255 + (s2 - sa) * (255 - dnp)) + temp) / 65025
                        } else if 4 * d <= da {
                            (d * sa * 255
                                + da * (s2 - sa)
                                    * ((((16 * dnp - 12 * 255) * dnp + 3 * 65025) * dnp) / 65025)
                                + temp)
                                / 65025
                        } else {
                            (d * sa * 255
                                + da * (s2 - sa) * (((dnp * 255) as f64).sqrt() as i64 - dnp)
                                + temp)
                                / 65025
                        }
                    }
                    22 => s + d - div_255(2 * (s * da).min(d * sa)),
                    // comp_func_solid_Exclusion and comp_func_Exclusion round differently.
                    _ if solid => s + d - div_255(2 * (s * d)),
                    _ => s + d - ((s * d) >> 7),
                }
            };
            let a = 255 - div_255((255 - sa) * (255 - da));
            // qRgba() packs each channel into its byte.
            let r = [
                (op(d[0] as i64, s[0] as i64) & 0xff) as u32,
                (op(d[1] as i64, s[1] as i64) & 0xff) as u32,
                (op(d[2] as i64, s[2] as i64) & 0xff) as u32,
                (a & 0xff) as u32,
            ];
            if ca == 255 {
                r
            } else {
                each(|k| interp(r[k], ca, d[k], cia))
            }
        }
        _ => d,
    }
}

/// comp_func_solid_<mode>, where it rounds differently from the per-pixel function.
fn comp_solid(mode: u8, d: Px, c: Px, ca: u32) -> Px {
    if mode == 3 && ca != 255 {
        let cia = 255 - ca;
        return each(|k| byte_mul(c[k], ca) + byte_mul(d[k], cia));
    }
    comp_px(mode, d, c, ca, true)
}

// ---------------------------------------------------------------- QRgba64

type C64 = [u64; 4];

/// combineAlpha256 then QRgba64::premultiplied.
fn premul64(c: [u16; 4], opacity: u32) -> C64 {
    premul(combine(c, opacity))
}

fn combine(c: [u16; 4], opacity: u32) -> C64 {
    [
        c[0] as u64,
        c[1] as u64,
        c[2] as u64,
        (c[3] as u64 * opacity as u64) >> 8,
    ]
}

fn premul(c: C64) -> C64 {
    let a = c[3];
    if a == 65535 {
        return c;
    }
    if a == 0 {
        return [0; 4];
    }
    let f = |v: u64| {
        let x = v * a;
        ((x + (x >> 16) + 0x8000) >> 16) & 0xffff
    };
    [f(c[0]), f(c[1]), f(c[2]), a]
}

/// QRgba64::toArgb32, channel by channel.
fn to8(c: C64) -> [u8; 4] {
    c.map(|v| {
        let v = v + 0x80;
        ((v - (v >> 8)) >> 8) as u8
    })
}

fn qround(v: f64) -> i64 {
    if v >= 0.0 {
        (v + 0.5) as i64
    } else {
        (v - 0.5) as i64
    }
}

/// QGradientCache::generateGradientColorTable (ColorInterpolation) and the toArgb32 copy the
/// 32-bit fetchers read.
fn color_table(stops: &[Stop], opacity: u32) -> Vec<[u8; 4]> {
    const BW: [Stop; 2] = [(0.0, [0, 0, 0, 65535]), (1.0, [65535; 4])];
    let stops: &[Stop] = if stops.is_empty() { &BW } else { stops };
    let mut t: Vec<C64> = vec![[0; 4]; TABLE];
    let n = stops.len();
    if n == 2 {
        let mut first = premul64(stops[0].1, opacity);
        let mut second = premul64(stops[1].1, opacity);
        let (mut s0, mut s1) = (stops[0].0, stops[1].0);
        if s1 < s0 {
            std::mem::swap(&mut first, &mut second);
            std::mem::swap(&mut s0, &mut s1);
        }
        let first_index = qround(s0 * (TABLE - 1) as f64);
        let second_index = qround(s1 * (TABLE - 1) as f64);
        let mut cur = first.map(|v| (v as u32) << 16);
        let end = second.map(|v| (v as u32) << 16);
        let mut i = 0i64;
        while i <= first_index.min(TABLE as i64) && (i as usize) < TABLE {
            t[i as usize] = first;
            i += 1;
        }
        if i < second_index {
            let reciprocal = 1.0 / (second_index - first_index) as f64;
            let delta: [i32; 4] = std::array::from_fn(|k| {
                qround((end[k] as f64 - cur[k] as f64) * reciprocal) as i32
            });
            for c in &mut cur {
                *c = c.wrapping_add(1 << 15);
            }
            while i < second_index.min(TABLE as i64) {
                for k in 0..4 {
                    cur[k] = cur[k].wrapping_add(delta[k] as u32);
                }
                t[i as usize] = cur.map(|v| (v >> 16) as u64);
                i += 1;
            }
        }
        while (i as usize) < TABLE {
            t[i as usize] = second;
            i += 1;
        }
    } else if n == 1 {
        t.fill(premul64(stops[0].1, opacity));
    } else {
        let lerp = |x: C64, a: u64, y: C64, b: u64| -> C64 {
            std::array::from_fn(|k| (((x[k] * a) >> 8) + ((y[k] * b) >> 8)) & 0xffff)
        };
        let (begin_pos, end_pos) = (stops[0].0, stops[n - 1].0);
        let incr = 1.0 / TABLE as f64;
        let mut dpos = 1.5 * incr;
        let mut pos = 1usize;
        t[0] = premul64(stops[0].1, opacity);
        while dpos <= begin_pos && pos < TABLE {
            t[pos] = t[pos - 1];
            pos += 1;
            dpos += incr;
        }
        if dpos < end_pos {
            let mut cur = 0usize;
            while dpos > stops[cur + 1].0 {
                cur += 1;
            }
            let mut current = premul64(stops[cur].1, opacity);
            let mut next = premul64(stops[cur + 1].1, opacity);
            let slope = |cur: usize| {
                let diff = stops[cur + 1].0 - stops[cur].0;
                if diff == 0.0 { 0.0 } else { 256.0 / diff }
            };
            let mut c = slope(cur);
            let mut tt = (dpos - stops[cur].0) * c;
            let mut t_delta = incr * c;
            while pos < TABLE {
                let dist = qround(tt) as u64;
                t[pos] = lerp(current, 256u64.wrapping_sub(dist), next, dist);
                pos += 1;
                dpos += incr;
                if dpos >= end_pos {
                    break;
                }
                tt += t_delta;
                let mut skip = 0;
                while dpos > stops[cur + skip + 1].0 {
                    skip += 1;
                }
                if skip != 0 {
                    cur += skip;
                    current = if skip == 1 {
                        next
                    } else {
                        premul64(stops[cur].1, opacity)
                    };
                    next = premul64(stops[cur + 1].1, opacity);
                    c = slope(cur);
                    tt = (dpos - stops[cur].0) * c;
                    t_delta = incr * c;
                }
            }
        }
        let last = premul64(stops[n - 1].1, opacity);
        while pos < TABLE {
            t[pos] = last;
            pos += 1;
        }
        t[TABLE - 1] = last;
    }
    t.into_iter().map(to8).collect()
}

// ---------------------------------------------------------------- QSpanData

enum Kind {
    /// getLinearGradientValues: dx, dy, l, off.
    Linear([f64; 4]),
    /// Focal x, y, radius and getRadialGradientValues: dx, dy, dr, sqrfr, a, extended.
    Radial([f64; 3], [f64; 5], bool),
    /// Centre x, y and the angle in radians.
    Conical([f64; 3]),
}

struct Gradient {
    table: Vec<[u8; 4]>,
    kind: Kind,
    /// Device to brush space (QSpanData::m11..dy).
    inv: M,
}

struct Texture {
    img: Rc<Canvas>,
    inv: M,
    /// QTextureData::const_alpha, 0..=256.
    alpha: u32,
    tiled: bool,
    /// The painter transform is a translation: fetchUntransformed.
    plain: bool,
    bilinear: bool,
}

enum Source {
    Solid(Px),
    Gradient(Gradient),
    Texture(Texture),
}

fn fuzzy_null(v: f64) -> bool {
    v.abs() <= 0.000000000001
}

fn inverted(m: &M) -> Option<M> {
    let det = m[0] * m[3] - m[1] * m[2];
    if det == 0.0 || !det.is_finite() {
        return None;
    }
    let i = 1.0 / det;
    let (a, b, c, d) = (m[3] * i, -m[1] * i, -m[2] * i, m[0] * i);
    Some([a, b, c, d, -(m[4] * a + m[5] * c), -(m[4] * b + m[5] * d)])
}

/// QSpanData::setupMatrix: the inverse of `translate(1/65536, 1/65536) * matrix`.
fn span_matrix(m: &M) -> Option<M> {
    let d = 1.0 / 65536.0;
    inverted(&[
        m[0],
        m[1],
        m[2],
        m[3],
        m[0] * d + m[2] * d + m[4],
        m[1] * d + m[3] * d + m[5],
    ])
}

fn pad(ipos: i64) -> usize {
    ipos.clamp(0, TABLE as i64 - 1) as usize
}

impl Gradient {
    /// qt_fetch_{linear,radial,conical}_gradient into `buf` for device pixels x.., row y.
    fn fetch(&self, buf: &mut Vec<[u8; 4]>, x: i64, y: i64, len: usize) {
        buf.clear();
        let m = &self.inv;
        let (cx, cy) = (x as f64 + 0.5, y as f64 + 0.5);
        let rx = m[2] * cy + m[0] * cx + m[4];
        let ry = m[3] * cy + m[1] * cx + m[5];
        match &self.kind {
            Kind::Linear([dx, dy, l, off]) => {
                const FIXPT_MAX: f64 = (i32::MAX >> 9) as f64;
                let (mut t, mut inc) = (0.0, 0.0);
                if *l != 0.0 {
                    t = (dx * rx + dy * ry + off) * (TABLE - 1) as f64;
                    inc = (dx * m[0] + dy * m[1]) * (TABLE - 1) as f64;
                }
                let fixed = |v: i32| self.table[pad(((v as i64) + 128) >> 8)];
                let real = |v: f64| self.table[pad((v * (TABLE - 1) as f64 + 0.5) as i64)];
                if inc > -1e-5 && inc < 1e-5 {
                    let c = if t.abs() < FIXPT_MAX {
                        fixed((t * 256.0) as i32)
                    } else {
                        real(t / TABLE as f64)
                    };
                    buf.resize(len, c);
                } else if t.abs() < FIXPT_MAX
                    && inc.abs() < FIXPT_MAX
                    && (t + inc * len as f64).abs() < FIXPT_MAX
                {
                    let mut tf = (t * 256.0) as i32;
                    let incf = (inc * 256.0) as i32;
                    for _ in 0..len {
                        buf.push(fixed(tf));
                        tf = tf.wrapping_add(incf);
                    }
                } else {
                    for _ in 0..len {
                        buf.push(real(t / TABLE as f64));
                        t += inc;
                    }
                }
            }
            Kind::Radial(focal, [dx, dy, dr, sqrfr, a], extended) => {
                if fuzzy_null(*a) {
                    buf.resize(len, [0; 4]);
                    return;
                }
                let (rx, ry) = (rx - focal[0], ry - focal[1]);
                let mut inv_a = 1.0 / (2.0 * a);
                let (delta_rx, delta_ry) = (m[0], m[1]);
                let mut b = 2.0 * (dr * focal[2] + rx * dx + ry * dy);
                let mut delta_b = 2.0 * (delta_rx * dx + delta_ry * dy);
                let b_delta_b = 2.0 * b * delta_b;
                let delta_b_delta_b = 2.0 * delta_b * delta_b;
                let bb = b * b;
                let delta_bb = delta_b * delta_b;
                b *= inv_a;
                delta_b *= inv_a;
                let rxrxryry = rx * rx + ry * ry;
                let delta_rxrxryry = delta_rx * delta_rx + delta_ry * delta_ry;
                let rx_plus_ry = 2.0 * (rx * delta_rx + ry * delta_ry);
                let delta_rx_plus_ry = 2.0 * delta_rxrxryry;
                inv_a *= inv_a;
                let mut det = (bb - 4.0 * a * (sqrfr - rxrxryry)) * inv_a;
                let mut delta_det =
                    (b_delta_b + delta_bb + 4.0 * a * (rx_plus_ry + delta_rxrxryry)) * inv_a;
                let ddd = (delta_b_delta_b + 4.0 * a * delta_rx_plus_ry) * inv_a;
                if !((det as f32).is_finite()
                    && (delta_det as f32).is_finite()
                    && (ddd as f32).is_finite())
                {
                    buf.resize(len, [0; 4]);
                    return;
                }
                // QRadialFetchSimd: four pixels at a time in single precision.
                let (mut det_v, mut dd4_v, mut b_v) = ([0f32; 4], [0f32; 4], [0f32; 4]);
                for i in 0..4 {
                    det_v[i] = det as f32;
                    dd4_v[i] = (4.0 * delta_det) as f32;
                    b_v[i] = b as f32;
                    det += delta_det;
                    delta_det += ddd;
                    b += delta_b;
                }
                let (dd16, dd6, db4) = (
                    (16.0 * ddd) as f32,
                    (6.0 * ddd) as f32,
                    (4.0 * delta_b) as f32,
                );
                let (r0, drf) = (focal[2] as f32, *dr as f32);
                let max = (TABLE - 1) as f32;
                while buf.len() < len {
                    for i in 0..4 {
                        let local = det_v[i].max(0.0).sqrt() - b_v[i];
                        let index = local * max + 0.5;
                        let ok = det_v[i] >= 0.0 && r0 + drf * local >= 0.0;
                        let idx = index.max(0.0).min(max) as i32 as usize;
                        det_v[i] = det_v[i] + dd4_v[i] + dd6;
                        dd4_v[i] += dd16;
                        b_v[i] += db4;
                        if buf.len() < len {
                            buf.push(if !*extended || ok {
                                self.table[idx.min(TABLE - 1)]
                            } else {
                                [0; 4]
                            });
                        }
                    }
                }
            }
            Kind::Conical([ccx, ccy, angle]) => {
                let inv2pi = std::f64::consts::FRAC_1_PI / 2.0;
                let (mut rx, mut ry) = (rx - ccx, ry - ccy);
                for _ in 0..len {
                    let a = ry.atan2(rx) + angle;
                    let pos = 1.0 - a * inv2pi;
                    let mut ipos = (pos * (TABLE - 1) as f64 + 0.5) as i64;
                    if ipos < 0 || ipos >= TABLE as i64 {
                        ipos %= TABLE as i64;
                        if ipos < 0 {
                            ipos += TABLE as i64;
                        }
                    }
                    buf.push(self.table[ipos as usize]);
                    rx += m[0];
                    ry += m[1];
                }
            }
        }
    }
}

impl Texture {
    fn pixel(&self, x: i64, y: i64) -> Option<[u8; 4]> {
        let (w, h) = (self.img.w as i64, self.img.h as i64);
        let (x, y) = if self.tiled {
            (x.rem_euclid(w), y.rem_euclid(h))
        } else if x < 0 || y < 0 || x >= w || y >= h {
            return None;
        } else {
            (x, y)
        };
        Some(self.img.px[(y * w + x) as usize])
    }

    /// fetchUntransformed / fetchTransformed / fetchTransformedBilinear for ARGB32_PM.
    fn fetch(&self, buf: &mut Vec<[u8; 4]>, x: i64, y: i64, len: usize) {
        buf.clear();
        if self.img.w == 0 || self.img.h == 0 {
            buf.resize(len, [0; 4]);
            return;
        }
        let m = &self.inv;
        if self.plain {
            // xoff = -qRound(-dx): whole pixels only.
            let xoff = -qround(-m[4]);
            let yoff = -qround(-m[5]);
            for k in 0..len as i64 {
                buf.push(self.pixel(x + k + xoff, y + yoff).unwrap_or([0; 4]));
            }
            return;
        }
        let (cx, cy) = (x as f64 + 0.5, y as f64 + 0.5);
        const FIXED: f64 = 65536.0;
        if !self.bilinear {
            let mut fx = ((m[2] * cy + m[0] * cx + m[4]) * FIXED) as i64;
            let mut fy = ((m[3] * cy + m[1] * cx + m[5]) * FIXED) as i64;
            let (fdx, fdy) = ((m[0] * FIXED) as i64, (m[1] * FIXED) as i64);
            // fetchTransformed: the pixel centre rounding of the 16.16 position.
            fx -= 1;
            fy -= 1;
            for _ in 0..len {
                buf.push(self.pixel(fx >> 16, fy >> 16).unwrap_or([0; 4]));
                fx += fdx;
                fy += fdy;
            }
            return;
        }
        let half = 32768i64;
        let mut fx = ((m[2] * cy + m[0] * cx + m[4]) * FIXED) as i64 - half;
        let mut fy = ((m[3] * cy + m[1] * cx + m[5]) * FIXED) as i64 - half;
        let (fdx, fdy) = ((m[0] * FIXED) as i64, (m[1] * FIXED) as i64);
        let (w, h) = (self.img.w as i64, self.img.h as i64);
        for _ in 0..len {
            let (x1, y1) = (fx >> 16, fy >> 16);
            let (distx, disty) = (((fx & 0xffff) >> 8) as u32, ((fy & 0xffff) >> 8) as u32);
            // Outside the image the edge pixel repeats (a plain texture) or the tile wraps.
            let at = |px: i64, py: i64| -> [u8; 4] {
                if self.tiled {
                    self.img.px[(py.rem_euclid(h) * w + px.rem_euclid(w)) as usize]
                } else {
                    self.img.px[(py.clamp(0, h - 1) * w + px.clamp(0, w - 1)) as usize]
                }
            };
            let (tl, tr, bl, br) = (
                at(x1, y1),
                at(x1 + 1, y1),
                at(x1, y1 + 1),
                at(x1 + 1, y1 + 1),
            );
            // interpolate_4_pixels: horizontal blend of each row, then vertical.
            let (idx, idy) = (256 - distx, 256 - disty);
            let mut out = [0u8; 4];
            for k in 0..4 {
                let top = (tl[k] as u32 * idx + tr[k] as u32 * distx) >> 8;
                let bot = (bl[k] as u32 * idx + br[k] as u32 * distx) >> 8;
                out[k] = (((top & 0xff) * idy + (bot & 0xff) * disty) >> 8) as u8;
            }
            buf.push(out);
            fx += fdx;
            fy += fdy;
        }
    }
}

/// qt_patternForBrush: the 8x8 bitmaps of Dense1Pattern..DiagCrossPattern, a set bit paints.
fn pattern_rows(style: u8) -> [u8; 8] {
    match style {
        2 => [0xff, 0xbb, 0xff, 0xff, 0xff, 0xbb, 0xff, 0xff],
        3 => [0x77, 0xff, 0xdd, 0xff, 0x77, 0xff, 0xdd, 0xff],
        4 => [0x55, 0xbb, 0x55, 0xee, 0x55, 0xbb, 0x55, 0xee],
        5 => [0xaa, 0x55, 0xaa, 0x55, 0xaa, 0x55, 0xaa, 0x55],
        6 => [0xaa, 0x44, 0xaa, 0x11, 0xaa, 0x44, 0xaa, 0x11],
        7 => [0x88, 0x00, 0x22, 0x00, 0x88, 0x00, 0x22, 0x00],
        8 => [0x00, 0x44, 0x00, 0x00, 0x00, 0x44, 0x00, 0x00],
        9 => [0x00, 0x00, 0x00, 0xff, 0x00, 0x00, 0x00, 0x00],
        10 => [0x10; 8],
        11 => [0x10, 0x10, 0x10, 0xff, 0x10, 0x10, 0x10, 0x10],
        12 => [0x80, 0x40, 0x20, 0x10, 0x08, 0x04, 0x02, 0x01],
        13 => [0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80],
        14 => [0x81, 0x42, 0x24, 0x18, 0x18, 0x24, 0x42, 0x81],
        _ => [0; 8],
    }
}

/// QRasterBuffer::colorizeBitmap(qt_imageForBrush(style, true), color).
fn pattern_image(color: [u8; 4], style: u8) -> Canvas {
    let fg = canvas::solid_pm(color, 256).map(|v| v as u8);
    let rows = pattern_rows(style);
    let mut px = Vec::with_capacity(64);
    for row in rows {
        for x in 0..8 {
            // Format_MonoLSB: bit 0 is the leftmost pixel.
            px.push(if row >> x & 1 == 1 { fg } else { [0; 4] });
        }
    }
    Canvas::from_px(8, 8, px)
}

/// qt_scaleForTransform: the scale and whether the transform keeps shapes (tx_noshear).
pub fn scale_for_transform(m: &M) -> (f64, bool) {
    let fuzzy = |a: f64, b: f64| (a - b).abs() * 1e12 <= a.abs().min(b.abs());
    if m[1] == 0.0 && m[2] == 0.0 {
        if m[0] == 1.0 && m[3] == 1.0 {
            return (1.0, true);
        }
        let (xs, ys) = (m[0].abs(), m[3].abs());
        return (xs.max(ys), fuzzy(xs, ys));
    }
    let (xs1, ys1) = (m[0] * m[0] + m[2] * m[2], m[1] * m[1] + m[3] * m[3]);
    let (xs2, ys2) = (m[0] * m[0] + m[1] * m[1], m[2] * m[2] + m[3] * m[3]);
    if (xs1 - ys1).abs() > (xs2 - ys2).abs() {
        (xs1.max(ys1).sqrt(), fuzzy(xs1, ys1))
    } else {
        (xs2.max(ys2).sqrt(), fuzzy(xs2, ys2))
    }
}

fn map(m: &M, p: [f64; 2]) -> [f64; 2] {
    [
        m[0] * p[0] + m[2] * p[1] + m[4],
        m[1] * p[0] + m[3] * p[1] + m[5],
    ]
}

/// QTransform::mapRect of x, y, w, h.
fn map_rect(m: &M, r: [f64; 4]) -> [f64; 4] {
    let ps = [
        map(m, [r[0], r[1]]),
        map(m, [r[0] + r[2], r[1]]),
        map(m, [r[0], r[1] + r[3]]),
        map(m, [r[0] + r[2], r[1] + r[3]]),
    ];
    let x0 = ps.iter().map(|p| p[0]).fold(f64::MAX, f64::min);
    let y0 = ps.iter().map(|p| p[1]).fold(f64::MAX, f64::min);
    let x1 = ps.iter().map(|p| p[0]).fold(f64::MIN, f64::max);
    let y1 = ps.iter().map(|p| p[1]).fold(f64::MIN, f64::max);
    [x0, y0, x1 - x0, y1 - y0]
}

type Span = (i64, i64, i64, u32);

/// A QPainter with Antialiasing and SmoothPixmapTransform on an ARGB32_Premultiplied image.
pub struct Painter {
    pub img: Canvas,
    matrix: M,
    /// QPainterState::intOpacity, 0..=256.
    opacity: u32,
    /// QPainter::CompositionMode.
    mode: u8,
    /// QClipData: the coverage of every device pixel.
    clip: Option<Vec<u8>>,
    /// QClipData::hasRectClip with a rectangle that is not empty.
    clip_rect: bool,
    /// QClipData's xmin, ymin, xmax and ymax: the pixels the clip leaves anything of, the
    /// last two one past. `None` with no clip.
    clip_bounds: Option<[i64; 4]>,
    /// QPainter::SmoothPixmapTransform.
    pub smooth: bool,
}

impl Painter {
    pub fn new(w: usize, h: usize) -> Self {
        Painter {
            img: Canvas::new(w, h, [0, 0, 0, 0]),
            matrix: IDENTITY,
            opacity: 256,
            mode: 0,
            clip: None,
            clip_rect: false,
            clip_bounds: None,
            smooth: true,
        }
    }

    pub fn matrix(&self) -> M {
        self.matrix
    }

    /// QPainter::setWorldTransform; a non-finite matrix is not taken.
    pub fn set_matrix(&mut self, m: M) {
        if m.iter().all(|v| v.is_finite()) {
            self.matrix = m;
        }
    }

    /// QPainter::setOpacity.
    pub fn set_opacity(&mut self, o: f64) {
        if o.is_finite() {
            // QRasterPaintEngine::opacityChanged truncates.
            self.opacity = (o.clamp(0.0, 1.0) * 256.0) as u32;
        }
    }

    /// QPainter::setCompositionMode.
    pub fn set_composition_mode(&mut self, mode: u8) {
        self.mode = mode;
    }

    pub fn composition_mode(&self) -> u8 {
        self.mode
    }

    /// QPainter::setClipping(false).
    pub fn clear_clip(&mut self) {
        self.clip = None;
        self.clip_rect = false;
        self.clip_bounds = None;
    }

    /// QPainter::setClipPath(ReplaceClip) under the current transform: a path and its odd-even
    /// fill rule. `None` clips everything away.
    pub fn set_clip(&mut self, path: Option<&(Path, bool)>) {
        let (w, h) = (self.img.w, self.img.h);
        self.clip_rect = false;
        let m = self.matrix;
        let mut mask = vec![0u8; w * h];
        if let (Some((p, _)), true) = (path, m[1] == 0.0 && m[2] == 0.0)
            && let Some(r) = p.as_rect()
        {
            // setClipRectInDeviceCoords(qt_mapFillRect): whole pixels, no coverage.
            let a = map(&m, [r[0], r[1]]).map(qround);
            let b = map(&m, [r[2], r[3]]).map(qround);
            let (x0, x1) = (a[0].min(b[0]).max(0), a[0].max(b[0]).min(w as i64));
            let (y0, y1) = (a[1].min(b[1]).max(0), a[1].max(b[1]).min(h as i64));
            for y in y0..y1 {
                for x in x0..x1 {
                    mask[y as usize * w + x as usize] = 255;
                }
            }
            self.clip_rect = x0 < x1 && y0 < y1;
        } else if let Some((p, even_odd)) = path {
            for (x, y, len, cov) in self.path_spans(p, *even_odd) {
                if y < 0 || y >= h as i64 {
                    continue;
                }
                for px in x.max(0)..(x + len).min(w as i64) {
                    mask[y as usize * w + px as usize] = cov as u8;
                }
            }
        }
        self.clip_bounds = Some(mask_bounds(&mask, w));
        self.clip = Some(mask);
    }

    /// QRasterPaintEnginePrivate::initializeRasterizer: the rectangle QRasterizer cuts its
    /// lines to, left, top, right and bottom with the last two inside it. The device, and
    /// under a clip the part of it the clip leaves anything of; `None` when that is nothing.
    fn rasterizer_clip(&self) -> Option<[i64; 4]> {
        let (w, h) = (self.img.w as i64, self.img.h as i64);
        let [x0, y0, x1, y1] = self.clip_bounds.unwrap_or([0, 0, w, h]);
        let (x0, y0, x1, y1) = (x0.max(0), y0.max(0), x1.min(w), y1.min(h));
        (x0 < x1 && y0 < y1).then_some([x0, y0, x1 - 1, y1 - 1])
    }

    fn path_spans(&self, p: &Path, even_odd: bool) -> Vec<Span> {
        let mut spans = Vec::new();
        if p.el.is_empty() {
            return spans;
        }
        let xf = (self.matrix != IDENTITY).then_some(self.matrix);
        canvas::gray_render(
            &canvas::outline_xf(p, 0.0, 0.0, xf),
            even_odd,
            self.img.w as i64,
            self.img.h as i64,
            |x, y, len, cov| spans.push((x, y, len, cov as u32)),
        );
        spans
    }

    /// QSpanData::setup for `brush` under the current transform and opacity.
    fn source(&self, brush: &Brush) -> Option<Source> {
        let op = self.opacity;
        let grad = |stops: &[Stop], kind: Kind| {
            Some(Source::Gradient(Gradient {
                table: color_table(stops, op),
                kind,
                inv: span_matrix(&self.matrix)?,
            }))
        };
        match brush {
            Brush::None => None,
            Brush::Solid(c) => {
                let pm = canvas::solid_pm(*c, op);
                (pm[3] != 0 || self.mode != 0).then_some(Source::Solid(pm))
            }
            Brush::Linear(p, stops) => {
                let (mut dx, mut dy) = (p[2] - p[0], p[3] - p[1]);
                let l = dx * dx + dy * dy;
                let mut off = 0.0;
                if l != 0.0 {
                    dx /= l;
                    dy /= l;
                    off = -dx * p[0] - dy * p[1];
                }
                grad(stops, Kind::Linear([dx, dy, l, off]))
            }
            Brush::Radial(p, stops) => {
                let (dx, dy, dr) = (p[0] - p[3], p[1] - p[4], p[2] - p[5]);
                let a = dr * dr - dx * dx - dy * dy;
                let extended = !fuzzy_null(p[5]) || a <= 0.0;
                grad(
                    stops,
                    Kind::Radial([p[3], p[4], p[5]], [dx, dy, dr, p[5] * p[5], a], extended),
                )
            }
            Brush::Conical(p, stops) => grad(stops, Kind::Conical([p[0], p[1], p[2].to_radians()])),
            Brush::Texture(img) => Some(self.texture(img.clone(), true)),
            Brush::Pattern(color, style) => match style {
                0 => None,
                2..=14 => Some(self.texture(Rc::new(pattern_image(*color, *style)), true)),
                _ => self.source(&Brush::Solid(*color)),
            },
        }
    }

    fn texture(&self, img: Rc<Canvas>, tiled: bool) -> Source {
        let m = &self.matrix;
        let plain = m[0] == 1.0 && m[1] == 0.0 && m[2] == 0.0 && m[3] == 1.0;
        Source::Texture(Texture {
            img,
            inv: span_matrix(m).unwrap_or(IDENTITY),
            alpha: self.opacity,
            tiled,
            plain,
            bilinear: self.smooth,
        })
    }

    /// The blend function of a QSpanData: every span through the clip, then composed.
    fn blend(&mut self, src: &Source, spans: &[Span]) {
        let (w, h) = (self.img.w as i64, self.img.h as i64);
        let mode = self.mode;
        let mut buf: Vec<[u8; 4]> = Vec::new();
        for &(x, y, len, cov) in spans {
            if y < 0 || y >= h {
                continue;
            }
            let (x0, x1) = (x.max(0), (x + len).min(w));
            if x0 >= x1 {
                continue;
            }
            // qt_intersect_spans: one piece per run of equal clip coverage.
            let mut a = x0;
            while a < x1 {
                let (mut b, mut c) = (x1, cov);
                if let Some(mask) = &self.clip {
                    let row = &mask[(y * w) as usize..((y + 1) * w) as usize];
                    let k = row[a as usize];
                    b = a + 1;
                    while b < x1 && row[b as usize] == k {
                        b += 1;
                    }
                    c = div_255(cov as i64 * k as i64) as u32;
                }
                if c != 0 {
                    self.blend_run(src, mode, &mut buf, a, y, (b - a) as usize, c);
                }
                a = b;
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn blend_run(
        &mut self,
        src: &Source,
        mode: u8,
        buf: &mut Vec<[u8; 4]>,
        x: i64,
        y: i64,
        len: usize,
        cov: u32,
    ) {
        let w = self.img.w;
        let row = y as usize * w;
        match src {
            Source::Solid(pm) => {
                if mode == 0 {
                    return canvas::blend_span(&mut self.img, *pm, x, y, len as i64, cov);
                }
                for d in &mut self.img.px[row + x as usize..row + x as usize + len] {
                    *d = comp_solid(mode, d.map(|v| v as u32), *pm, cov).map(|v| v as u8);
                }
            }
            Source::Gradient(_) | Source::Texture(_) => {
                let cov = match src {
                    Source::Texture(t) => (cov * t.alpha) >> 8,
                    _ => cov,
                };
                if cov == 0 {
                    return;
                }
                // blend_src_generic: BufferSize pixels per fetch.
                let (mut x, mut left) = (x, len);
                while left > 0 {
                    let l = left.min(2048);
                    match src {
                        Source::Gradient(g) => g.fetch(buf, x, y, l),
                        Source::Texture(t) => t.fetch(buf, x, y, l),
                        Source::Solid(_) => unreachable!(),
                    }
                    let dst = &mut self.img.px[row + x as usize..row + x as usize + l];
                    for (d, s) in dst.iter_mut().zip(buf.iter()) {
                        let s = s.map(|v| v as u32);
                        if mode == 0 && cov == 255 {
                            // comp_func_SourceOver at full coverage.
                            if s[3] == 255 {
                                *d = s.map(|v| v as u8);
                            } else if s != [0; 4] {
                                let ia = 255 - s[3];
                                for k in 0..4 {
                                    d[k] = (s[k] + byte_mul(d[k] as u32, ia)) as u8;
                                }
                            }
                        } else {
                            *d = comp(mode, d.map(|v| v as u32), s, cov).map(|v| v as u8);
                        }
                    }
                    x += l as i64;
                    left -= l;
                }
            }
        }
    }

    /// QPainter::fillPath: `path` (closed by the caller as Qt's caller does) under the
    /// transform.
    pub fn fill_path(&mut self, path: &Path, brush: &Brush, even_odd: bool) {
        let Some(src) = self.source(brush) else {
            return;
        };
        let spans = self.path_spans(path, even_odd);
        self.blend(&src, &spans);
    }

    fn rect_spans(&self, r: [f64; 4]) -> Vec<Span> {
        let mut spans = Vec::new();
        if !r.iter().all(|v| v.is_finite()) {
            return spans;
        }
        let (_, noshear) = scale_for_transform(&self.matrix);
        if noshear {
            // QRasterPaintEngine::fillRect: the line through the middle, as wide as the rect.
            let (x0, x1) = (r[0].min(r[0] + r[2]), r[0].max(r[0] + r[2]));
            let (y0, y1) = (r[1].min(r[1] + r[3]), r[1].max(r[1] + r[3]));
            if x1 > x0 && y1 > y0 {
                let a = map(&self.matrix, [(x0 + x0) * 0.5, (y0 + y1) * 0.5]);
                let b = map(&self.matrix, [(x1 + x1) * 0.5, (y0 + y1) * 0.5]);
                let clip = self.rasterizer_clip();
                rasterize_line(a, b, (y1 - y0) / (x1 - x0), false, clip, &mut spans);
            }
            return spans;
        }
        let mut p = Path::default();
        p.add_rect(r[0], r[1], r[2], r[3]);
        self.path_spans(&p, false)
    }

    /// QPainter::fillRect(QRectF, QBrush).
    pub fn fill_rect(&mut self, r: [f64; 4], brush: &Brush) {
        let Some(src) = self.source(brush) else {
            return;
        };
        let spans = self.rect_spans(r);
        self.blend(&src, &spans);
    }

    /// CompositionMode_Clear fillRect, the mode restored afterwards.
    pub fn clear_rect(&mut self, r: [f64; 4]) {
        let spans = self.rect_spans(r);
        let mode = std::mem::replace(&mut self.mode, 2);
        self.blend(&Source::Solid([255; 4]), &spans);
        self.mode = mode;
    }

    /// QPainter::strokePath.
    pub fn stroke_path(&mut self, path: &Path, pen: &Pen) {
        if path.el.is_empty() || !pen.width.is_finite() || !pen.miter_limit.is_finite() {
            return;
        }
        if !pen.dash_offset.is_finite() || pen.dash.iter().any(|v| !v.is_finite()) {
            return;
        }
        let (scale, noshear) = scale_for_transform(&self.matrix);
        let (w, h) = (self.img.w, self.img.h);
        if noshear && pen.width * scale <= 1.0 {
            // fast_pen: QCosmeticStroker on the mapped path.
            let Some(src) = self.source(&pen.brush) else {
                return;
            };
            let opacity = if pen.width == 0.0 {
                256
            } else {
                ((256.0 * pen.width * scale) as i64).clamp(0, 256) as u32
            };
            let mut pixels: Vec<(i64, i64, u32)> = Vec::new();
            crate::cosmetic::stroke_pixels(
                w,
                h,
                &path.mapped(self.matrix),
                pen.cap != Cap::Flat,
                &pen.dash,
                pen.dash_offset,
                &mut |x, y, cov| pixels.push((x, y, cov)),
            );
            if let (Source::Solid(_), 0, true, Brush::Solid(c)) = (
                &src,
                self.mode,
                self.clip.is_none() || self.clip_rect,
                &pen.brush,
            ) {
                // FastDraw: the 16-bit solid colour takes the pen opacity before coverage.
                let c16 = premul(combine(c.map(|v| v as u16 * 257), self.opacity));
                let color = to8(c16.map(|v| (v * opacity as u64) >> 8)).map(|v| v as u32);
                for (x, y, cov) in pixels {
                    let at = y as usize * w + x as usize;
                    if self.clip.as_ref().is_some_and(|mask| mask[at] == 0) {
                        continue;
                    }
                    let s = color.map(|v| byte_mul(v, cov));
                    let d = &mut self.img.px[at];
                    let ia = 255 - s[3];
                    for k in 0..4 {
                        d[k] = (s[k] + byte_mul(d[k] as u32, ia)) as u8;
                    }
                }
                return;
            }
            for (x, y, cov) in pixels {
                self.blend(&src, &[(x, y, 1, (cov * opacity) >> 8)]);
            }
            return;
        }
        let is_lines = path.el.iter().enumerate().all(|(i, e)| {
            matches!(
                (i % 2, e),
                (0, canvas::El::Move(_)) | (1, canvas::El::Line(_))
            )
        });
        if noshear && pen.cap != Cap::Round && is_lines {
            // non_complex_pen and QVectorPath::LinesHint: QRasterizer::rasterizeLine per line,
            // each dashed from the start of the pattern.
            let Some(src) = self.source(&pen.brush) else {
                return;
            };
            let m = self.matrix;
            let map = |p: [f64; 2]| {
                [
                    m[0] * p[0] + m[2] * p[1] + m[4],
                    m[1] * p[0] + m[3] * p[1] + m[5],
                ]
            };
            let width = pen.width * scale;
            let square = pen.cap == Cap::Square;
            // updatePen: a custom dash with no pattern is a solid line here.
            let pattern: &[f64] = if pen.no_dashes { &[] } else { &pen.dash };
            let pattern_length: f64 = pattern.iter().sum();
            let (mut dash_index, mut dash_offset, mut in_dash) = (0, pen.dash_offset, true);
            if pattern_length > 0.0 {
                dash_offset %= pattern_length;
                if dash_offset < 0.0 {
                    dash_offset += pattern_length;
                }
                while dash_offset >= pattern[dash_index] {
                    dash_offset -= pattern[dash_index];
                    dash_index = (dash_index + 1) % pattern.len();
                    in_dash = !in_dash;
                }
            }
            let mut spans: Vec<Span> = Vec::new();
            let clip = self.rasterizer_clip();
            for pair in path.el.as_chunks::<2>().0 {
                let (canvas::El::Move(a), canvas::El::Line(b)) = (&pair[0], &pair[1]) else {
                    continue;
                };
                let (mut a, b) = (map(*a), map(*b));
                if !(a[0].is_finite() && a[1].is_finite() && b[0].is_finite() && b[1].is_finite()) {
                    continue;
                }
                if a == b {
                    if pen.cap != Cap::Flat {
                        let d = width / 2.0;
                        rasterize_line(
                            [a[0] - d, a[1]],
                            [a[0] + d, a[1]],
                            1.0,
                            false,
                            clip,
                            &mut spans,
                        );
                    }
                    continue;
                }
                // QLineF::length is qHypot.
                let mut length = (b[0] - a[0]).hypot(b[1] - a[1]);
                if pattern.is_empty() {
                    rasterize_line(a, b, width / length, square, clip, &mut spans);
                    continue;
                }
                // rasterizeLine_dashed.
                if pattern_length <= 0.0 {
                    continue;
                }
                if length / (pattern_length * width) > canvas::DASH_REPETITION_LIMIT {
                    rasterize_line(a, b, width / length, square, clip, &mut spans);
                    continue;
                }
                let (mut di, mut doff, mut ind) = (dash_index, dash_offset, in_dash);
                while length > 0.0 {
                    let rasterize = ind;
                    let mut dash = (pattern[di] - doff) * width;
                    let line_len = (b[0] - a[0]).hypot(b[1] - a[1]);
                    let (l1, mut l2) = (a, b);
                    if dash >= length {
                        dash = line_len;
                        doff += dash / width;
                        length = 0.0;
                    } else {
                        doff = 0.0;
                        ind = !ind;
                        di = (di + 1) % pattern.len();
                        length -= dash;
                        // QLineF::setLength.
                        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
                        l2 = [a[0] + dash * (dx / line_len), a[1] + dash * (dy / line_len)];
                        a = l2;
                    }
                    if rasterize && dash > 0.0 {
                        rasterize_line(l1, l2, width / dash, square, clip, &mut spans);
                    }
                }
            }
            self.blend(&src, &spans);
            return;
        }
        // QPaintEngineEx::stroke: the outline in user space, filled under the transform.
        if pen.no_dashes {
            return;
        }
        let Some(inv) = inverted(&self.matrix) else {
            return;
        };
        let clip = map_rect(&inv, [0.0, 0.0, w as f64, h as f64]);
        let Some((outline, half)) = canvas::pen_outline(
            path,
            pen.width,
            pen.cap,
            pen.join,
            pen.miter_limit,
            &pen.dash,
            pen.dash_offset,
            clip,
            scale,
        ) else {
            return;
        };
        let brush = if half {
            // QPen::setColor: a solid pen of the brush colour with half its alpha.
            let mut c = match &pen.brush {
                Brush::Solid(c) | Brush::Pattern(c, _) => *c,
                _ => [0, 0, 0, 255],
            };
            c[3] /= 2;
            Brush::Solid(c)
        } else {
            pen.brush.clone()
        };
        self.fill_path(&outline, &brush, false);
    }

    /// QPainter::drawImage(QPointF, QImage) with a premultiplied image.
    pub fn draw_image(&mut self, img: &Rc<Canvas>, x: f64, y: f64) {
        if img.w == 0 || img.h == 0 || !x.is_finite() || !y.is_finite() {
            return;
        }
        let m = self.matrix;
        let (iw, ih) = (img.w as f64, img.h as f64);
        if m[0] == 1.0 && m[1] == 0.0 && m[2] == 0.0 && m[3] == 1.0 {
            // Translation: the image lands on whole pixels.
            let (px, py) = (qround(x + m[4]), qround(y + m[5]));
            let mut t = match self.texture(img.clone(), false) {
                Source::Texture(t) => t,
                _ => return,
            };
            t.inv = [1.0, 0.0, 0.0, 1.0, -(px as f64), -(py as f64)];
            let spans: Vec<Span> = (0..img.h as i64)
                .map(|k| (px, py + k, img.w as i64, 255))
                .collect();
            return self.blend(&Source::Texture(t), &spans);
        }
        // drawImage(QRectF, image, sr): the image rect filled with the transformed texture.
        let mut t = match self.texture(img.clone(), false) {
            Source::Texture(t) => t,
            _ => return,
        };
        let Some(inv) = span_matrix(&[
            m[0],
            m[1],
            m[2],
            m[3],
            m[0] * x + m[2] * y + m[4],
            m[1] * x + m[3] * y + m[5],
        ]) else {
            return;
        };
        t.inv = inv;
        let spans = self.rect_spans([x, y, iw, ih]);
        self.blend(&Source::Texture(t), &spans);
    }

    /// QPainter::drawPixmap(QRectF, pixmap, pixmap.rect()) of a painter without Antialiasing
    /// whose transform only scales and moves: `img` stretched over `r`
    /// (QRasterPaintEngine::drawImage). The pixels filled are the mapped rectangle with each
    /// edge rounded, and each takes the image where the transform puts it: as it is, on whole
    /// pixels, when the stretch undoes the painter's scale, else filtered as [`Painter::smooth`]
    /// says.
    pub fn draw_image_rect(&mut self, img: &Rc<Canvas>, r: [f64; 4]) {
        let m = self.matrix;
        if img.w == 0
            || img.h == 0
            || !r.iter().all(|v| v.is_finite())
            || r[2] <= 0.0
            || r[3] <= 0.0
        {
            return;
        }
        let (iw, ih) = (img.w as f64, img.h as f64);
        // `copy`: the image's pixels to the device.
        let (sx, sy) = (m[0] * (r[2] / iw), m[3] * (r[3] / ih));
        let copy = [sx, 0.0, 0.0, sy, m[0] * r[0] + m[4], m[3] * r[1] + m[5]];
        let Some(inv) = span_matrix(&copy) else {
            return;
        };
        // QTransform::type: qFuzzyIsNull of what a side differs from 1 by.
        let unit = |v: f64| (v - 1.0).abs() <= 1e-12;
        let t = Texture {
            img: img.clone(),
            inv,
            alpha: self.opacity,
            tiled: false,
            plain: unit(sx) && unit(sy),
            bilinear: self.smooth,
        };
        let (x1, y1) = (qround(copy[4]), qround(copy[5]));
        let (x2, y2) = (
            qround(m[0] * (r[0] + r[2]) + m[4]),
            qround(m[3] * (r[1] + r[3]) + m[5]),
        );
        let spans: Vec<Span> = (y1..y2).map(|y| (x1, y, x2 - x1, 255)).collect();
        self.blend(&Source::Texture(t), &spans);
    }
}

/// qt_image_boxblur(image, radius, quality = true) on premultiplied pixels: three passes of
/// qt_image_convolute_filter with a (2 radius + 1)^2 box.
pub fn box_blur(img: &mut Canvas, radius: i64) {
    let (w, h) = (img.w as i64, img.h as i64);
    let fs = 2 * radius + 1;
    let weight = 1.0 / (fs * fs) as f64;
    for _ in 0..3 {
        let src = img.px.clone();
        // Prefix sums of every channel: the box sum of a window clipped to the image.
        let mut sum = vec![[0u32; 4]; ((w + 1) * (h + 1)) as usize];
        for y in 0..h {
            let mut row = [0u32; 4];
            for x in 0..w {
                let p = src[(y * w + x) as usize];
                for k in 0..4 {
                    row[k] += p[k] as u32;
                    sum[((y + 1) * (w + 1) + x + 1) as usize][k] =
                        sum[(y * (w + 1) + x + 1) as usize][k] + row[k];
                }
            }
        }
        for y in 0..h {
            let (y0, y1) = ((y - radius).max(0), (y + radius + 1).min(h));
            for x in 0..w {
                let (x0, x1) = ((x - radius).max(0), (x + radius + 1).min(w));
                let at = |xx: i64, yy: i64| sum[(yy * (w + 1) + xx) as usize];
                let (a, b, c, d) = (at(x1, y1), at(x0, y1), at(x1, y0), at(x0, y0));
                let p = &mut img.px[(y * w + x) as usize];
                for k in 0..4 {
                    let s = (a[k] + d[k] - b[k] - c[k]) as f64;
                    p[k] = qround(s * weight).clamp(0, 255) as u8;
                }
            }
        }
    }
}

const SC: i64 = 65536;

fn sc_mul(x: i64, y: i64) -> i64 {
    (x * y) >> 16
}

/// qSafeFloatToQScFixed.
fn sc_fixed(x: f64) -> i64 {
    let tmp = x * SC as f64;
    let (lo, hi) = (i32::MIN as f64 * SC as f64, i32::MAX as f64 * SC as f64);
    if tmp > hi {
        i32::MAX as i64 * SC
    } else if tmp < lo {
        i32::MIN as i64 * SC
    } else {
        tmp as i64
    }
}

fn safe_div(x: f64, y: f64) -> f64 {
    if y == 0.0 {
        if x > 0.0 { 1e20 } else { -1e20 }
    } else {
        x / y
    }
}

fn dot6_eq(p1: f64, p2: f64) -> bool {
    ((p2 - p1) * 64.0) as i32 == 0
}

/// intersectPixelFP (qrasterizer.cpp).
fn intersect_pixel(x: i64, top: i64, bottom: i64, lix: i64, rix: i64, slope: i64, inv: i64) -> i64 {
    let left_x = x * SC;
    let right_x = left_x + SC;
    let ys = || {
        let l = if slope > 0 {
            top + sc_mul(left_x - lix, inv)
        } else {
            top + sc_mul(left_x - rix, inv)
        };
        (l, l + inv)
    };
    if lix >= left_x && rix <= right_x {
        sc_mul(bottom - top, lix - left_x + ((rix - lix) >> 1))
    } else if lix >= right_x {
        bottom - top
    } else if lix >= left_x {
        let (_, r) = ys();
        if slope > 0 {
            (bottom - top) - sc_mul((right_x - lix) >> 1, r - top)
        } else {
            (bottom - top) - sc_mul((right_x - lix) >> 1, bottom - r)
        }
    } else if rix <= left_x {
        0
    } else if rix <= right_x {
        let (l, _) = ys();
        if slope > 0 {
            sc_mul((rix - left_x) >> 1, bottom - l)
        } else {
            sc_mul((rix - left_x) >> 1, l - top)
        }
    } else {
        let (l, r) = ys();
        if slope > 0 {
            (bottom - r) + ((r - l) >> 1)
        } else {
            (r - top) + ((l - r) >> 1)
        }
    }
}

/// qClipLine: true when the whole line is clipped away. `clip` is left, top, right, bottom.
fn clip_line(p1: &mut [f64; 2], p2: &mut [f64; 2], clip: [f64; 4]) -> bool {
    let [xmin, ymin, xmax, ymax] = clip;
    let (mut x1, mut y1, mut x2, mut y2) = (p1[0], p1[1], p2[0], p2[1]);
    if ![x1, y1, x2, y2].iter().all(|v| v.is_finite()) {
        return true;
    }
    if x1 < xmin {
        if x2 <= xmin {
            return true;
        }
        y1 += (y2 - y1) / (x2 - x1) * (xmin - x1);
        x1 = xmin;
    } else if x1 > xmax {
        if x2 >= xmax {
            return true;
        }
        y1 += (y2 - y1) / (x2 - x1) * (xmax - x1);
        x1 = xmax;
    }
    if x2 < xmin {
        y2 += (y2 - y1) / (x2 - x1) * (xmin - x2);
        x2 = xmin;
    } else if x2 > xmax {
        y2 += (y2 - y1) / (x2 - x1) * (xmax - x2);
        x2 = xmax;
    }
    if y1 < ymin {
        if y2 <= ymin {
            return true;
        }
        x1 += (x2 - x1) / (y2 - y1) * (ymin - y1);
        y1 = ymin;
    } else if y1 > ymax {
        if y2 >= ymax {
            return true;
        }
        x1 += (x2 - x1) / (y2 - y1) * (ymax - y1);
        y1 = ymax;
    }
    if y2 < ymin {
        x2 += (x2 - x1) / (y2 - y1) * (ymin - y2);
        y2 = ymin;
    } else if y2 > ymax {
        x2 += (x2 - x1) / (y2 - y1) * (ymax - y2);
        y2 = ymax;
    }
    *p1 = [x1, y1];
    *p2 = [x2, y2];
    false
}

/// QClipData::fixup: xmin, ymin, xmax and ymax of the spans of a clip, all 0 without any.
fn mask_bounds(mask: &[u8], w: usize) -> [i64; 4] {
    let mut b: Option<[i64; 4]> = None;
    for (i, _) in mask.iter().enumerate().filter(|(_, c)| **c != 0) {
        let (x, y) = ((i % w) as i64, (i / w) as i64);
        b = Some(match b {
            None => [x, y, x + 1, y + 1],
            Some([x0, y0, x1, y1]) => [x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1)],
        });
    }
    b.unwrap_or([0; 4])
}

/// QRasterizer::rasterizeLine (antialiased): the line from `a` to `b`, `width` times its
/// length wide, as spans inside `clip_rect` (QRasterizer::setClipRect: left, top, right and
/// bottom, the last two inside it).
fn rasterize_line(
    a: [f64; 2],
    b: [f64; 2],
    mut width: f64,
    square_cap: bool,
    clip_rect: Option<[i64; 4]>,
    spans: &mut Vec<Span>,
) {
    let Some([cl, ct, cr, cb]) = clip_rect else {
        return;
    };
    if a == b || width.is_nan() || width <= 0.0 {
        return;
    }
    let mut add = |x: i64, len: i64, y: i64, cov: i64| {
        if cov != 0 && len != 0 {
            spans.push((x, y, len, cov.clamp(0, 255) as u32));
        }
    };
    let (mut pa, mut pb) = (a, b);
    if square_cap {
        let d = [pb[0] - pa[0], pb[1] - pa[1]];
        let k = 0.5 * width;
        pa = [pa[0] - k * d[0], pa[1] - k * d[1]];
        pb = [pb[0] + k * d[0], pb[1] + k * d[1]];
    }
    let offs = [
        (b[1] - a[1]).abs() * width * 0.5,
        (b[0] - a[0]).abs() * width * 0.5,
    ];
    // QRectF(topLeft - offs, bottomRight + (1, 1) + offs), bottomRight being right, bottom.
    // A QRectF keeps its width and height, so its right and bottom are sums.
    let (left, top) = (cl as f64 - offs[0], ct as f64 - offs[1]);
    let clip = [
        left,
        top,
        left + ((cr as f64 + 1.0 + offs[0]) - left),
        top + ((cb as f64 + 1.0 + offs[1]) - top),
    ];
    if clip_line(&mut pa, &mut pb, clip) {
        return;
    }
    {
        let d0 = [a[0] - b[0], a[1] - b[1]];
        let w0 = d0[0] * d0[0] + d0[1] * d0[1];
        let d = [pa[0] - pb[0], pa[1] - pb[1]];
        let w = d[0] * d[0] + d[1] * d[1];
        if w == 0.0 {
            return;
        }
        width *= (w0 / w).sqrt();
    }
    if dot6_eq(pa[1], pb[1]) {
        let x = (pa[0] + pb[0]) * 0.5;
        let dx = (pb[0] - pa[0]).abs() * 0.5;
        let y = pa[1];
        let dy = width * dx;
        pa = [x, y - dy];
        pb = [x, y + dy];
        width = 1.0 / width;
    }
    if dot6_eq(pa[0], pb[0]) {
        if pa[1] > pb[1] {
            std::mem::swap(&mut pa, &mut pb);
        }
        let dy = pb[1] - pa[1];
        let half = 0.5 * width * dy;
        let left = (pa[0] - half).clamp(cl as f64, cr as f64 + 1.0);
        let right = (pa[0] + half).clamp(cl as f64, cr as f64 + 1.0);
        pa[1] = pa[1].clamp(ct as f64, cb as f64 + 1.0);
        pb[1] = pb[1].clamp(ct as f64, cb as f64 + 1.0);
        if dot6_eq(left, right) || dot6_eq(pa[1], pb[1]) {
            return;
        }
        let (il, ir) = (left as i64, right as i64);
        let left_w = (il + 1) * SC - sc_fixed(left);
        let right_w = sc_fixed(right) - ir * SC;
        let mut cov = [0i64; 3];
        let mut xs = [0i64; 3];
        let mut len = [0i64; 3];
        let mut n = 1;
        if il == ir {
            cov[0] = if left_w == SC {
                right_w * 255
            } else {
                (right_w + left_w - SC) * 255
            };
            xs[0] = il;
            len[0] = 1;
        } else {
            cov[0] = left_w * 255;
            xs[0] = il;
            len[0] = 1;
            if left_w == SC {
                len[0] = ir - il;
            } else if ir - il > 1 {
                cov[1] = 255 * SC;
                xs[1] = il + 1;
                len[1] = ir - il - 1;
                n += 1;
            }
            if right_w != 0 {
                cov[n] = right_w * 255;
                xs[n] = ir;
                len[n] = 1;
                n += 1;
            }
        }
        let (itop, ibot) = ((pa[1] as i64) * SC, (pb[1] as i64) * SC);
        let (ypa, ypb) = (sc_fixed(pa[1]), sc_fixed(pb[1]));
        let mut yfp = itop;
        while yfp <= ibot {
            let row_h = (yfp + SC).min(ypb) - yfp.max(ypa);
            let y = yfp >> 16;
            if y > cb {
                break;
            }
            for i in 0..n {
                add(xs[i], len[i], y, sc_mul(row_h, cov[i]) >> 16);
            }
            yfp += SC;
        }
        return;
    }
    if pa[1] > pb[1] {
        std::mem::swap(&mut pa, &mut pb);
    }
    let delta = [
        (pb[0] - pa[0]) * (0.5 * width),
        (pb[1] - pa[1]) * (0.5 * width),
    ];
    let perp = [delta[1], -delta[0]];
    let plus = |p: [f64; 2]| [p[0] + perp[0], p[1] + perp[1]];
    let minus = |p: [f64; 2]| [p[0] - perp[0], p[1] - perp[1]];
    let (top, left, right, bottom) = if pa[0] < pb[0] {
        (plus(pa), minus(pa), plus(pb), minus(pb))
    } else {
        (minus(pa), minus(pb), plus(pa), plus(pb))
    };
    let snap = |p: [f64; 2]| [(p[0] * 64.0).floor() / 64.0, (p[1] * 64.0).floor() / 64.0];
    let (top, bottom, left, right) = (snap(top), snap(bottom), snap(left), snap(right));
    let top_bound = top[1].clamp(ct as f64, cb as f64);
    let bottom_bound = bottom[1].clamp(ct as f64, cb as f64);
    let edge = |a: [f64; 2], b: [f64; 2]| safe_div(a[0] - b[0], a[1] - b[1]);
    let tl_slope = edge(left, top);
    let bl_slope = edge(bottom, left);
    let tr_slope = edge(right, top);
    let br_slope = edge(bottom, right);
    let (tl_fp, tr_fp) = (sc_fixed(tl_slope), sc_fixed(tr_slope));
    let (bl_fp, br_fp) = (sc_fixed(bl_slope), sc_fixed(br_slope));
    let inv_tl = sc_fixed(safe_div(1.0, tl_slope));
    let inv_tr = sc_fixed(safe_div(1.0, tr_slope));
    let inv_bl = sc_fixed(safe_div(1.0, bl_slope));
    let inv_br = sc_fixed(safe_div(1.0, br_slope));

    let tb = top_bound as i64;
    let itop = tb * SC;
    let ileft = (left[1] as i64) * SC;
    let iright = (right[1] as i64) * SC;
    let ibottom = (bottom_bound as i64) * SC;
    let mut left_af = sc_fixed(top[0] + (tb as f64 - top[1]) * tl_slope);
    let mut right_af = sc_fixed(top[0] + (tb as f64 - top[1]) * tr_slope);
    let mut left_bf = 0;
    let mut right_bf = 0;
    if ileft < itop {
        left_bf = sc_fixed(left[0] + (tb as f64 - left[1]) * bl_slope);
    }
    if iright < itop {
        right_bf = sc_fixed(right[0] + (tb as f64 - right[1]) * br_slope);
    }
    let (ytop, yleft) = (sc_fixed(top[1]), sc_fixed(left[1]));
    let (yright, ybottom) = (sc_fixed(right[1]), sc_fixed(bottom[1]));
    let mut row_top = itop.max(ytop);
    let mut tl_af = left_af + sc_mul(tl_fp, row_top - itop);
    let mut tr_af = right_af + sc_mul(tr_fp, row_top - itop);
    let mut yfp = itop;
    while yfp <= ibottom {
        let row_bl = (yfp + SC).min(yleft);
        let row_br = (yfp + SC).min(yright);
        let row_tl = yfp.max(yleft);
        let row_tr = yfp.max(yright);
        let row_bottom = (yfp + SC).min(ybottom);
        let (tl_bf, bl_af);
        if yfp == ileft {
            let y = (yfp >> 16) as f64;
            left_bf = sc_fixed(left[0] + (y - left[1]) * bl_slope);
            tl_bf = left_bf + sc_mul(bl_fp, row_tl - yfp);
            bl_af = left_af + sc_mul(tl_fp, row_bl - yfp);
        } else {
            tl_bf = left_bf;
            bl_af = left_af + tl_fp;
        }
        let (tr_bf, br_af);
        if yfp == iright {
            let y = (yfp >> 16) as f64;
            right_bf = sc_fixed(right[0] + (y - right[1]) * br_slope);
            tr_bf = right_bf + sc_mul(br_fp, row_tr - yfp);
            br_af = right_af + sc_mul(tr_fp, row_br - yfp);
        } else {
            tr_bf = right_bf;
            br_af = right_af + tr_fp;
        }
        let (bl_bf, br_bf) = if yfp == ibottom {
            (
                left_bf + sc_mul(bl_fp, row_bottom - yfp),
                right_bf + sc_mul(br_fp, row_bottom - yfp),
            )
        } else {
            (left_bf + bl_fp, right_bf + br_fp)
        };
        let (mut left_min, mut left_max) = if yfp < ileft {
            (bl_af >> 16, tl_af >> 16)
        } else if yfp == ileft {
            (bl_af.max(tl_bf) >> 16, tl_af.max(bl_bf) >> 16)
        } else {
            (tl_bf >> 16, bl_bf >> 16)
        };
        left_min = left_min.clamp(cl, cr);
        left_max = left_max.clamp(cl, cr);
        let (mut right_min, mut right_max) = if yfp < iright {
            (tr_af >> 16, br_af >> 16)
        } else if yfp == iright {
            (tr_af.min(br_bf) >> 16, br_af.min(tr_bf) >> 16)
        } else {
            (br_bf >> 16, tr_bf >> 16)
        };
        right_min = right_min.clamp(cl, cr);
        right_max = right_max.clamp(cl, cr);
        if left_max > right_max {
            left_max = right_max;
        }
        if right_min < left_min {
            right_min = left_min;
        }
        let row_h = row_bottom - row_top;
        let y = yfp >> 16;
        let right_excluded = |x: i64| {
            let mut e = 0;
            if yfp <= iright && row_br > row_top {
                e += (row_br - row_top)
                    - intersect_pixel(x, row_top, row_br, tr_af, br_af, tr_fp, inv_tr);
            }
            if yfp >= iright && row_bottom > row_tr {
                e += (row_bottom - row_tr)
                    - intersect_pixel(x, row_tr, row_bottom, br_bf, tr_bf, br_fp, inv_br);
            }
            e
        };
        let mut x = left_min;
        while x <= left_max {
            let mut excluded = 0;
            if yfp <= ileft && row_bl > row_top {
                excluded += intersect_pixel(x, row_top, row_bl, bl_af, tl_af, tl_fp, inv_tl);
            }
            if yfp >= ileft && row_bottom > row_tl {
                excluded += intersect_pixel(x, row_tl, row_bottom, tl_bf, bl_bf, bl_fp, inv_bl);
            }
            if x >= right_min {
                excluded += right_excluded(x);
            }
            add(x, 1, y, (255 * (row_h - excluded)) >> 16);
            x += 1;
        }
        if x < right_min {
            add(x, right_min - x, y, (255 * row_h) >> 16);
            x = right_min;
        }
        while x <= right_max {
            add(x, 1, y, (255 * (row_h - right_excluded(x))) >> 16);
            x += 1;
        }
        left_af += tl_fp;
        left_bf += bl_fp;
        right_af += tr_fp;
        right_bf += br_fp;
        tl_af = left_af;
        tr_af = right_af;
        yfp += SC;
        row_top = yfp;
    }
}
