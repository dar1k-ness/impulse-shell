//! QCosmeticStroker (qtbase/src/gui/painting/qcosmeticstroker.cpp): the raster engine's
//! `fast_pen` path for pens of width <= 1, antialiased, FastDraw onto ARGB32_Premultiplied.

use crate::canvas::{El, Path};
use crate::raster::Canvas;
use mul255::qt as byte_mul;

bits::bit_flags! {
const CAP_BEGIN: i32 = 1;
const CAP_END: i32 = 2;
}

/// QColor (straight 8-bit) → premultiplied QRgba64 → multiplyAlpha256(opacity) → toArgb32.
fn pen_color(c: [u8; 4], opacity: u32) -> [u32; 4] {
    let a16 = c[3] as u64 * 257;
    let pm = |v: u8| -> u64 {
        let v = v as u64 * 257;
        if a16 == 65535 {
            v
        } else if a16 == 0 {
            0
        } else {
            let x = v * a16;
            (x + (x >> 16) + 0x8000) >> 16
        }
    };
    let ch = [pm(c[0]), pm(c[1]), pm(c[2]), a16];
    ch.map(|v| {
        let v = if opacity >= 256 {
            v
        } else {
            (v * opacity as u64) >> 8
        };
        ((v - (v >> 8) + 0x80) >> 8) as u32
    })
}

struct Dasher {
    rev: bool,
    offset: i32,
    index: usize,
    on_bit: usize,
}

struct Stroker<'a> {
    w: i64,
    h: i64,
    /// drawPixel: device pixel and its coverage.
    plot: &'a mut dyn FnMut(i64, i64, u32),
    pattern: Vec<i32>,
    reverse: Vec<i32>,
    pattern_length: i32,
    pattern_offset: i32,
    xmin: f64,
    xmax: f64,
    ymin: f64,
    ymax: f64,
}

impl Stroker<'_> {
    fn pixel(&mut self, x: i64, y: i64, coverage: i64) {
        if x < 0 || x >= self.w || y < 0 || y >= self.h {
            return;
        }
        (self.plot)(x, y, coverage as u32);
    }

    fn dasher(&mut self, rev: bool, start: i64, stop: i64) -> Option<Dasher> {
        if self.pattern.is_empty() {
            return None;
        }
        let delta = (stop - start) as i32;
        let st = ((start & 63) - 32) as i32;
        let pl = self.pattern_length;
        let (mut offset, on_bit) = if rev {
            (pl - self.pattern_offset - delta - st, 0)
        } else {
            (self.pattern_offset - st, 1)
        };
        offset %= pl;
        if offset < 0 {
            offset += pl;
        }
        let pat = if rev { &self.reverse } else { &self.pattern };
        let mut index = 0;
        while index < pat.len() - 1 && offset >= pat[index] {
            index += 1;
        }
        self.pattern_offset = (self.pattern_offset + delta) % pl;
        Some(Dasher {
            rev,
            offset,
            index,
            on_bit,
        })
    }

    fn on(d: &Option<Dasher>) -> bool {
        d.as_ref().is_none_or(|d| (d.index + d.on_bit) & 1 == 1)
    }

    fn adjust(&self, d: &mut Option<Dasher>) {
        if let Some(d) = d {
            let pat = if d.rev { &self.reverse } else { &self.pattern };
            d.offset += 64;
            if d.offset >= pat[d.index] {
                d.index = (d.index + 1) % pat.len();
            }
            d.offset %= self.pattern_length;
        }
    }

    fn clip_line(&self, x1: &mut f64, y1: &mut f64, x2: &mut f64, y2: &mut f64) -> bool {
        if ![*x1, *y1, *x2, *y2].iter().all(|v| v.is_finite()) {
            return true;
        }
        let (xmin, xmax, ymin, ymax) = (self.xmin, self.xmax, self.ymin, self.ymax);
        if *x1 < xmin {
            if *x2 <= xmin {
                return true;
            }
            *y1 += (*y2 - *y1) / (*x2 - *x1) * (xmin - *x1);
            *x1 = xmin;
        } else if *x1 > xmax {
            if *x2 >= xmax {
                return true;
            }
            *y1 += (*y2 - *y1) / (*x2 - *x1) * (xmax - *x1);
            *x1 = xmax;
        }
        if *x2 < xmin {
            *y2 += (*y2 - *y1) / (*x2 - *x1) * (xmin - *x2);
            *x2 = xmin;
        } else if *x2 > xmax {
            *y2 += (*y2 - *y1) / (*x2 - *x1) * (xmax - *x2);
            *x2 = xmax;
        }
        if *y1 < ymin {
            if *y2 <= ymin {
                return true;
            }
            *x1 += (*x2 - *x1) / (*y2 - *y1) * (ymin - *y1);
            *y1 = ymin;
        } else if *y1 > ymax {
            if *y2 >= ymax {
                return true;
            }
            *x1 += (*x2 - *x1) / (*y2 - *y1) * (ymax - *y1);
            *y1 = ymax;
        }
        if *y2 < ymin {
            *x2 += (*x2 - *x1) / (*y2 - *y1) * (ymin - *y2);
            *y2 = ymin;
        } else if *y2 > ymax {
            *x2 += (*x2 - *x1) / (*y2 - *y1) * (ymax - *y2);
            *y2 = ymax;
        }
        false
    }

    /// drawLineAA<drawPixelARGB32, Dasher|NoDasher>
    fn line_aa(&mut self, mut rx1: f64, mut ry1: f64, mut rx2: f64, mut ry2: f64, mut caps: i32) {
        if self.clip_line(&mut rx1, &mut ry1, &mut rx2, &mut ry2) {
            return;
        }
        let f26 = |v: f64| (v * 64.0) as i32 as i64;
        let (x1, y1, x2, y2) = (f26(rx1), f26(ry1), f26(rx2), f26(ry2));
        let (dx, dy) = (x2 - x1, y2 - y1);
        let swap_caps = |c: i32| ((c & CAP_BEGIN) << 1) | ((c & CAP_END) >> 1);
        // Major axis is "a" (y when steep), minor is "b"; pixels are emitted as (b, a) or (a, b).
        let steep = dx.abs() < dy.abs();
        if !steep && dx == 0 {
            return;
        }
        let (mut a1, mut a2, mut b1, mut b2, da, db) = if steep {
            (y1, y2, x1, x2, dy, dx)
        } else {
            (x1, x2, y1, y2, dx, dy)
        };
        let binc = db * (1 << 16) / da;
        let mut swapped = false;
        if a1 > a2 {
            std::mem::swap(&mut a1, &mut a2);
            std::mem::swap(&mut b1, &mut b2);
            swapped = true;
            caps = swap_caps(caps);
        }
        let mut b = (b1 - 32) * (1 << 10);
        b -= (((a1 & 63) - 32) * binc) >> 6;
        if caps & CAP_BEGIN != 0 {
            a1 -= 32;
            b -= binc >> 1;
        }
        if caps & CAP_END != 0 {
            a2 += 32;
        }
        let mut dasher = self.dasher(swapped, a1, a2);
        let mut a = a1 >> 6;
        let as_ = a2 >> 6;
        let (alpha_start, alpha_end) = if a == as_ {
            (a2 - a1, 0)
        } else {
            (64 - (a1 & 63), a2 & 63)
        };
        let plot = |s: &mut Self, a: i64, b: i64, scale: Option<i64>| {
            let alpha = (b >> 8) & 0xff;
            let (c0, c1) = match scale {
                Some(k) => (((255 - alpha) * k) >> 6, (alpha * k) >> 6),
                None => (255 - alpha, alpha),
            };
            if steep {
                s.pixel(b >> 16, a, c0);
                s.pixel((b >> 16) + 1, a, c1);
            } else {
                s.pixel(a, b >> 16, c0);
                s.pixel(a, (b >> 16) + 1, c1);
            }
        };
        if Self::on(&dasher) {
            plot(self, a, b, Some(alpha_start));
        }
        self.adjust(&mut dasher);
        b += binc;
        a += 1;
        while a < as_ {
            if Self::on(&dasher) {
                plot(self, a, b, None);
            }
            self.adjust(&mut dasher);
            b += binc;
            a += 1;
        }
        if alpha_end != 0 && Self::on(&dasher) {
            plot(self, a, b, Some(alpha_end));
        }
    }

    fn cubic(&mut self, pts: &mut [[f64; 2]], level: i32, caps: i32) {
        if level > 0 {
            let dx = pts[3][0] - pts[0][0];
            let dy = pts[3][1] - pts[0][1];
            let len = 0.25 * (dx.abs() + dy.abs());
            if (dx * (pts[0][1] - pts[2][1]) - dy * (pts[0][0] - pts[2][0])).abs() >= len
                || (dx * (pts[0][1] - pts[1][1]) - dy * (pts[0][0] - pts[1][0])).abs() >= len
            {
                // False positive: `k` picks the coordinate inside each point, not an element of `pts`.
                #[allow(clippy::needless_range_loop)]
                for k in 0..2 {
                    pts[6][k] = pts[3][k];
                    let c = pts[1][k];
                    let d = pts[2][k];
                    let a = (pts[0][k] + c) * 0.5;
                    pts[1][k] = a;
                    let b = (pts[3][k] + d) * 0.5;
                    pts[5][k] = b;
                    let c = (c + d) * 0.5;
                    let a = (a + c) * 0.5;
                    pts[2][k] = a;
                    let b = (b + c) * 0.5;
                    pts[4][k] = b;
                    pts[3][k] = (a + b) * 0.5;
                }
                self.cubic(&mut pts[3..], level - 1, caps & CAP_BEGIN);
                self.cubic(pts, level - 1, caps & CAP_END);
                return;
            }
        }
        self.line_aa(pts[3][0], pts[3][1], pts[0][0], pts[0][1], caps);
    }
}

/// The pen's colour and how QCosmeticStroker::setup has its pixels written.
pub enum Pen {
    /// FastDraw (drawPixelARGB32), which setup takes only under a rectangular clip of the
    /// painter: the straight colour, scaled by the pen's width once.
    Fast([u8; 4]),
    /// drawPixel: spans of the pen's premultiplied colour (QSpanData::solidColor, the painter's
    /// opacity in it), each pixel's coverage scaled by the pen's width (blend_color_argb).
    Spans([u32; 4]),
}

/// QCosmeticStroker::drawPath for a QPainterPath (element-typed QVectorPath), identity matrix.
pub fn stroke(
    img: &mut Canvas,
    path: &Path,
    pen: Pen,
    width: f64,
    draw_caps: bool,
    dash: &[f64],
    dash_offset: f64,
) {
    let opacity = if width == 0.0 {
        256
    } else {
        ((256.0 * width) as i32).clamp(0, 256) as u32
    };
    let (color, scale) = match pen {
        Pen::Fast(c) => (pen_color(c, opacity), 256),
        Pen::Spans(pm) => (pm, opacity),
    };
    let w = img.w as i64;
    stroke_pixels(
        img.w,
        img.h,
        path,
        draw_caps,
        dash,
        dash_offset,
        &mut |x, y, cov| {
            let cov = (cov * scale) >> 8;
            let c = color.map(|v| byte_mul(v, cov));
            let d = &mut img.px[(y * w + x) as usize];
            let ia = 255 - c[3];
            for k in 0..4 {
                d[k] = (c[k] + byte_mul(d[k] as u32, ia)) as u8;
            }
        },
    );
}

/// The pixels QCosmeticStroker::drawPath covers on a `w` x `h` device, in drawing order, each
/// with its antialiasing coverage; `path` is already in device coordinates.
pub fn stroke_pixels(
    w: usize,
    h: usize,
    path: &Path,
    draw_caps: bool,
    dash: &[f64],
    dash_offset: f64,
    plot: &mut dyn FnMut(i64, i64, u32),
) {
    if !dash_offset.is_finite() || dash.iter().any(|v| !v.is_finite()) {
        return;
    }
    let mut pattern = Vec::new();
    let mut reverse = Vec::new();
    let mut pattern_length = 0;
    if !dash.is_empty() && dash.len() <= 1024 {
        let q = |v: f64| ((v * 64.0) as i32).clamp(1, 65536);
        for v in dash {
            pattern_length += q(*v);
            pattern.push(pattern_length);
        }
        let mut l = 0;
        for v in dash.iter().rev() {
            l += q(*v);
            reverse.push(l);
        }
    }
    let (iw, ih) = (w as i64, h as i64);
    let (w, h) = (w as f64, h as f64);
    let mut s = Stroker {
        w: iw,
        h: ih,
        plot,
        pattern,
        reverse,
        pattern_length,
        pattern_offset: 0,
        xmin: -1.0,
        xmax: w - 1.0 + 2.0,
        ymin: -1.0,
        ymax: h - 1.0 + 2.0,
    };
    let el = &path.el;
    let mut i = 0;
    while i < el.len() {
        let start = i;
        i += 1;
        while i < el.len() && !matches!(el[i], El::Move(_)) {
            i += 1;
        }
        let sub = &el[start..i];
        let first = match sub[0] {
            El::Move(p) | El::Line(p) | El::Cubic(_, _, p) => p,
        };
        let last = match sub[sub.len() - 1] {
            El::Move(p) | El::Line(p) | El::Cubic(_, _, p) => p,
        };
        s.pattern_offset = (dash_offset * 64.0) as i32;
        let closed = first == last;
        let mut caps = if !closed && draw_caps { CAP_BEGIN } else { 0 };
        let mut p = first;
        for (k, e) in sub.iter().enumerate().skip(1) {
            let is_last = k == sub.len() - 1;
            if !closed && draw_caps && is_last {
                caps |= CAP_END;
            }
            match *e {
                El::Move(_) => unreachable!(),
                El::Line(p2) => {
                    s.line_aa(p[0], p[1], p2[0], p2[1], caps);
                    p = p2;
                }
                El::Cubic(c1, c2, p4) => {
                    let mut pts = [[0.0; 2]; 3 * 6 + 4];
                    pts[3] = p;
                    pts[2] = c1;
                    pts[1] = c2;
                    pts[0] = p4;
                    s.cubic(&mut pts, 6, caps);
                    p = p4;
                }
            }
            caps = 0;
        }
    }
}
