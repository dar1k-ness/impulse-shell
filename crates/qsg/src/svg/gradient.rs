//! QGradient as Qt's raster engine paints it: the 1024-entry colour table of QGradientCache
//! (qpaintengine_raster.cpp) and the span fetchers of qdrawhelper.cpp.

/// GRADIENT_STOPTABLE_SIZE.
pub const TABLE: usize = 1024;

/// QRgba64: r, g, b, a, 16 bits each, not premultiplied unless said so.
pub type Rgba64 = [u16; 4];

/// QColor::rgba64 of an 8-bit colour (QColor keeps 16-bit channels, `v * 257`).
pub fn rgba64(c: [u8; 4]) -> Rgba64 {
    c.map(|v| v as u16 * 257)
}

/// combineAlpha256: alpha scaled by `alpha256 / 256`.
fn combine_alpha256(c: Rgba64, alpha256: u32) -> Rgba64 {
    [c[0], c[1], c[2], ((c[3] as u32 * alpha256) >> 8) as u16]
}

/// qPremultiply(QRgba64).
fn premultiply64(c: Rgba64) -> Rgba64 {
    let a = c[3] as u32;
    if a == 0xffff {
        return c;
    }
    if a == 0 {
        return [0; 4];
    }
    let m = |v: u16| {
        let t = v as u32 * a;
        ((t + (t >> 16) + 0x8000) >> 16) as u16
    };
    [m(c[0]), m(c[1]), m(c[2]), c[3]]
}

pub fn q_round(v: f64) -> i32 {
    if v >= 0.0 {
        (v + 0.5) as i32
    } else {
        (v - 0.5) as i32
    }
}

/// QGradientCache::generateGradientColorTable for ComponentInterpolation (what QtSvg sets):
/// premultiplied 16-bit colours, `opacity` (0..=256) folded into the stops' alpha.
/// `stops` is never empty and sorted by offset.
pub fn color_table(stops: &[(f64, Rgba64)], opacity: u32) -> Vec<Rgba64> {
    let mut table = vec![[0u16; 4]; TABLE];
    let n = stops.len();
    if n == 2 {
        let mut first = combine_alpha256(stops[0].1, opacity);
        let mut second = combine_alpha256(stops[1].1, opacity);
        let (mut first_stop, mut second_stop) = (stops[0].0, stops[1].0);
        if second_stop < first_stop {
            std::mem::swap(&mut first, &mut second);
            std::mem::swap(&mut first_stop, &mut second_stop);
        }
        let first_index = q_round(first_stop * (TABLE - 1) as f64);
        let second_index = q_round(second_stop * (TABLE - 1) as f64);
        let mut cur: [u32; 4] = first.map(|v| (v as u32) << 16);
        let end: [u32; 4] = second.map(|v| (v as u32) << 16);
        let mut i = 0i32;
        while i <= first_index.min(TABLE as i32) {
            // Qt writes colorTable[1024] here when first_index >= 1024; offsets are clamped to
            // 0..=1, so first_index <= 1023.
            table[i as usize] = premultiply64(first);
            i += 1;
        }
        if i < second_index {
            let reciprocal = 1.0 / (second_index - first_index) as f64;
            let delta: [i32; 4] =
                std::array::from_fn(|k| q_round((end[k] as f64 - cur[k] as f64) * reciprocal));
            for c in &mut cur {
                *c = c.wrapping_add(1 << 15);
            }
            while i < second_index.min(TABLE as i32) {
                for k in 0..4 {
                    cur[k] = cur[k].wrapping_add(delta[k] as u32);
                }
                table[i as usize] = premultiply64(cur.map(|v| (v >> 16) as u16));
                i += 1;
            }
        }
        while (i as usize) < TABLE {
            table[i as usize] = premultiply64(second);
            i += 1;
        }
        return table;
    }
    let mut current = combine_alpha256(stops[0].1, opacity);
    if n == 1 {
        table.fill(premultiply64(current));
        return table;
    }
    let size = TABLE;
    let begin_pos = stops[0].0;
    let end_pos = stops[n - 1].0;
    let mut pos = 0usize;
    let incr = 1.0 / size as f64;
    let mut dpos = 1.5 * incr;
    table[pos] = premultiply64(current);
    pos += 1;
    while dpos <= begin_pos {
        table[pos] = table[pos - 1];
        pos += 1;
        dpos += incr;
    }
    let mut cur_stop = 0usize;
    if dpos < end_pos {
        while dpos > stops[cur_stop + 1].0 {
            cur_stop += 1;
        }
        if cur_stop != 0 {
            current = combine_alpha256(stops[cur_stop].1, opacity);
        }
        let mut next = combine_alpha256(stops[cur_stop + 1].1, opacity);
        let rate = |i: usize| {
            let diff = stops[i + 1].0 - stops[i].0;
            if diff == 0.0 { 0.0 } else { 256.0 / diff }
        };
        let mut c = rate(cur_stop);
        let mut t = (dpos - stops[cur_stop].0) * c;
        let mut t_delta = incr * c;
        loop {
            let dist = q_round(t) as u32;
            let idist = 256u32.wrapping_sub(dist);
            table[pos] = premultiply64(interpolate256(current, idist, next, dist));
            pos += 1;
            dpos += incr;
            if dpos >= end_pos {
                break;
            }
            t += t_delta;
            let mut skip = 0;
            while dpos > stops[cur_stop + skip + 1].0 {
                skip += 1;
            }
            if skip != 0 {
                cur_stop += skip;
                current = if skip == 1 {
                    next
                } else {
                    combine_alpha256(stops[cur_stop].1, opacity)
                };
                next = combine_alpha256(stops[cur_stop + 1].1, opacity);
                c = rate(cur_stop);
                t = (dpos - stops[cur_stop].0) * c;
                t_delta = incr * c;
            }
        }
    }
    let last = premultiply64(combine_alpha256(stops[n - 1].1, opacity));
    while pos < size - 1 {
        table[pos] = last;
        pos += 1;
    }
    table[size - 1] = last;
    table
}

/// interpolate256: `x * a1 / 256 + y * a2 / 256` per channel (wrapping like the 16-bit lanes).
fn interpolate256(x: Rgba64, a1: u32, y: Rgba64, a2: u32) -> Rgba64 {
    std::array::from_fn(|k| {
        (((x[k] as u32).wrapping_mul(a1) >> 8) as u16)
            .wrapping_add(((y[k] as u32).wrapping_mul(a2) >> 8) as u16)
    })
}

/// QRgba64::toArgb32 as channels r, g, b, a: the table an ARGB32_Premultiplied target reads.
pub fn to8(c: Rgba64) -> [u8; 4] {
    c.map(|v| {
        let v = v as u32 + 128;
        ((v - (v >> 8)) >> 8) as u8
    })
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Spread {
    Pad,
    Reflect,
    Repeat,
}

/// qt_gradient_clamp.
fn clamp(spread: Spread, ipos: i64) -> usize {
    let n = TABLE as i64;
    (match spread {
        Spread::Repeat => ipos.rem_euclid(n),
        Spread::Reflect => {
            let i = ipos.rem_euclid(2 * n);
            if i >= n { 2 * n - 1 - i } else { i }
        }
        Spread::Pad => ipos.clamp(0, n - 1),
    }) as usize
}

#[derive(Clone, Copy, Debug)]
pub enum Geometry {
    /// x1, y1, x2, y2.
    Linear([f64; 4]),
    /// cx, cy, r, fx, fy (focal radius 0, as QtSvg builds it).
    Radial([f64; 5]),
}

/// One gradient brush set up for a paint: QSpanData after `setup` and `setupMatrix`.
pub struct Spans {
    table: Vec<[u8; 4]>,
    spread: Spread,
    geo: Geometry,
    /// Device to gradient space: m11, m12, m21, m22, dx, dy.
    inv: [f64; 6],
}

const FIXPT_MAX: f64 = (i32::MAX >> 9) as f64;

impl Spans {
    /// `matrix` maps gradient space to the device (brush transform, then the painter's).
    /// QSpanData::setupMatrix inverts it behind a 1/65536 translation; a matrix that cannot be
    /// inverted gives the identity (QTransform::inverted).
    pub fn new(
        stops: &[(f64, Rgba64)],
        opacity: u32,
        spread: Spread,
        geo: Geometry,
        m: [f64; 6],
    ) -> Spans {
        let d = 1.0 / 65536.0;
        // delta * matrix: the translation maps through the matrix's linear part.
        let (tx, ty) = (d * m[0] + d * m[2] + m[4], d * m[1] + d * m[3] + m[5]);
        let det = m[0] * m[3] - m[1] * m[2];
        let inv = if det == 0.0 {
            [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]
        } else {
            let (a, b, c, e) = (m[3] / det, -m[1] / det, -m[2] / det, m[0] / det);
            [a, b, c, e, -(a * tx + c * ty), -(b * tx + e * ty)]
        };
        Spans {
            table: color_table(stops, opacity).into_iter().map(to8).collect(),
            spread,
            geo,
            inv,
        }
    }

    fn at(&self, ipos: i64) -> [u8; 4] {
        self.table[clamp(self.spread, ipos)]
    }

    /// qt_gradient_pixel: `int(pos * 1023 + 0.5)` truncates toward zero.
    fn pixel(&self, pos: f64) -> [u8; 4] {
        self.at((pos * (TABLE - 1) as f64 + 0.5) as i64)
    }

    /// qt_gradient_pixel_fixed.
    fn pixel_fixed(&self, fixed: i64) -> [u8; 4] {
        self.at((fixed + 128) >> 8)
    }

    /// The one colour of each row of a vertical gradient under a matrix that only scales, for
    /// spans from row `first` to row `last`: blend_vertical_gradient_argb, which gives up
    /// (`None`) where its fixed point factors do not fit (calculate_fixed_gradient_factors).
    pub fn rows(&self, first: i64, last: i64) -> Option<impl Fn(i64) -> [u8; 4] + '_> {
        let [_, m12, m21, m22, _, dy] = self.inv;
        let Geometry::Linear([x1, y1, x2, y2]) = self.geo else {
            return None;
        };
        let null = |v: f64| v.abs() <= 1e-12;
        if !null(m12) || !null(m21) || x1 != x2 {
            return None;
        }
        let (mut gx, mut gy) = (x2 - x1, y2 - y1);
        let l = gx * gx + gy * gy;
        let mut off = 0.0;
        if l != 0.0 {
            gx /= l;
            gy /= l;
            off = -gx * x1 - gy * y1;
        }
        let scale = (TABLE - 1) as f64 * 256.0;
        let yinc = gy * m22 * scale;
        let roff = (gy * (m22 * 0.5 + dy) + off) * scale;
        let limit = (i32::MAX - 256) as f64;
        let fits = yinc.abs() < limit
            && roff.abs() < limit
            && (yinc * first as f64 + roff).abs() < limit
            && (yinc * last as f64 + roff).abs() < limit;
        let (yinc, roff) = (yinc as i32, roff as i32);
        fits.then_some(move |y: i64| {
            self.pixel_fixed(yinc.wrapping_mul(y as i32).wrapping_add(roff) as i64)
        })
    }

    /// The source pixels (premultiplied RGBA) of the run `x..x + out.len()` on row `y`:
    /// qt_fetch_linear_gradient / qt_fetch_radial_gradient for one fetch of handleSpans.
    pub fn fetch(&self, x: i64, y: i64, out: &mut [[u8; 4]]) {
        let [m11, m12, m21, m22, dx, dy] = self.inv;
        let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
        let rx = m21 * py + m11 * px + dx;
        let ry = m22 * py + m12 * px + dy;
        match self.geo {
            Geometry::Linear([x1, y1, x2, y2]) => {
                let (mut gx, mut gy) = (x2 - x1, y2 - y1);
                let l = gx * gx + gy * gy;
                let (mut t, mut inc) = (0.0, 0.0);
                if l != 0.0 {
                    gx /= l;
                    gy /= l;
                    let off = -gx * x1 - gy * y1;
                    t = (gx * rx + gy * ry + off) * (TABLE - 1) as f64;
                    inc = (gx * m11 + gy * m12) * (TABLE - 1) as f64;
                }
                if inc > -1e-5 && inc < 1e-5 {
                    let c = if t.abs() < FIXPT_MAX {
                        self.pixel_fixed((t * 256.0) as i64)
                    } else {
                        self.pixel(t / TABLE as f64)
                    };
                    out.fill(c);
                } else if t.abs() < FIXPT_MAX
                    && inc.abs() < FIXPT_MAX
                    && (t + inc * out.len() as f64).abs() < FIXPT_MAX
                {
                    let mut t_fixed = (t * 256.0) as i32;
                    let inc_fixed = (inc * 256.0) as i32;
                    for o in out {
                        *o = self.pixel_fixed(t_fixed as i64);
                        t_fixed = t_fixed.wrapping_add(inc_fixed);
                    }
                } else {
                    for o in out {
                        *o = self.pixel(t / TABLE as f64);
                        t += inc;
                    }
                }
            }
            Geometry::Radial([cx, cy, r, fx, fy]) => {
                // getRadialGradientValues with focal radius 0.
                let (gdx, gdy, dr) = (cx - fx, cy - fy, r);
                let a = dr * dr - gdx * gdx - gdy * gdy;
                if a.abs() <= 0.000000000001 {
                    out.fill([0; 4]);
                    return;
                }
                let extended = a <= 0.0;
                let inv_a = 1.0 / (2.0 * a);
                for (i, o) in out.iter_mut().enumerate() {
                    let (qx, qy) = (rx + m11 * i as f64 - fx, ry + m12 * i as f64 - fy);
                    let b = 2.0 * (qx * gdx + qy * gdy);
                    // The fetcher works in float: det and b scaled by 1/(2a).
                    let det = ((b * b + 4.0 * a * (qx * qx + qy * qy)) * inv_a * inv_a) as f32;
                    let b = (b * inv_a) as f32;
                    let v = det.max(0.0).sqrt() - b;
                    let shown = det >= 0.0 && (dr as f32) * v >= 0.0;
                    let index = v * (TABLE - 1) as f32 + 0.5;
                    let c = match self.spread {
                        Spread::Pad => self.table[index.clamp(0.0, (TABLE - 1) as f32) as usize],
                        Spread::Repeat => self.table[(index as i32 & 0x3ff) as usize],
                        Spread::Reflect => {
                            let i = index as i32 & 0x7ff;
                            self.table[i.min(2047 - i) as usize]
                        }
                    };
                    *o = if extended || shown { c } else { [0; 4] };
                }
            }
        }
    }
}

/// comp_func_Source of one fetched pixel under `coverage` (0..=255): INTERPOLATE_PIXEL_255.
pub fn source(d: &mut [u8; 4], s: [u8; 4], coverage: u32) {
    if coverage == 255 {
        *d = s;
        return;
    }
    for k in 0..4 {
        let t = s[k] as u32 * coverage + d[k] as u32 * (255 - coverage);
        d[k] = mul255::qt_div_255(t as i64) as u8;
    }
}

/// comp_func_SourceOver of one fetched pixel under `coverage` (0..=255).
pub fn source_over(d: &mut [u8; 4], s: [u8; 4], coverage: u32) {
    let s = if coverage == 255 {
        s.map(u32::from)
    } else {
        s.map(|v| mul255::qt(v as u32, coverage))
    };
    let ia = 255 - s[3];
    for k in 0..4 {
        d[k] = (s[k] + mul255::qt(d[k] as u32, ia)) as u8;
    }
}
