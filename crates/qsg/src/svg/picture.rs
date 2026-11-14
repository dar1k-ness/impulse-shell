//! `<image>`: the node QSvgHandler's createImageNode makes and QSvgImage::drawCommand's
//! QPainter::drawImage on the raster engine (SmoothPixmapTransform, Antialiasing).

use super::gradient::source_over;
use super::{M, compose, invert};
use crate::canvas::Context;
use crate::raster::Canvas;
use base64::Engine;
use base64::engine::DecodePaddingMode;
use base64::engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig};

/// The picture of an `href`, premultiplied; None where Qt makes no node. Only `data:` URLs
/// (qDecodeDataUrl): a document read from memory has no directory to resolve a file name in.
pub fn load(href: &str) -> Option<Canvas> {
    let url = href.trim().strip_prefix("data:")?.as_bytes();
    let comma = url.iter().position(|&b| b == b',')?;
    let head = percent_decode(&url[..comma]);
    let head = head.trim_ascii();
    let mut data = percent_decode(&url[comma + 1..]);
    if head.len() >= 7 && head[head.len() - 7..].eq_ignore_ascii_case(b";base64") {
        // QByteArray::fromBase64Encoding without AbortOnBase64DecodingErrors: what is not in
        // the alphabet is skipped, padding included, and leftover bits are dropped.
        data.retain(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/'));
        if data.len() % 4 == 1 {
            data.pop();
        }
        let lenient = GeneralPurposeConfig::new()
            .with_decode_padding_mode(DecodePaddingMode::RequireNone)
            .with_decode_allow_trailing_bits(true);
        data = GeneralPurpose::new(&base64::alphabet::STANDARD, lenient)
            .decode(data)
            .ok()?;
    }
    crate::image::decode_ex(&data).ok().map(|(cv, _)| cv)
}

fn percent_decode(s: &[u8]) -> Vec<u8> {
    let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        match (
            s[i],
            s.get(i + 1).copied().and_then(hex),
            s.get(i + 2).copied().and_then(hex),
        ) {
            (b'%', Some(h), Some(l)) => {
                out.push(h * 16 + l);
                i += 3;
            }
            (b, ..) => {
                out.push(b);
                i += 1;
            }
        }
    }
    out
}

/// QRasterPaintEngine::drawImage(r, img) under the painter matrix `m` and `opacity`, with
/// QPainter::SmoothPixmapTransform on (`smooth`) or off.
pub fn draw(ctx: &mut Context, img: &Canvas, r: [f64; 4], m: M, opacity: f64, smooth: bool) {
    let int_opacity = (opacity * 256.0) as u32;
    let (w, h) = (ctx.img.w as i64, ctx.img.h as i64);
    let (iw, ih) = (img.w as i64, img.h as i64);
    if iw == 0 || ih == 0 || r[2] <= 0.0 || r[3] <= 0.0 {
        return;
    }
    let null = |v: f64| v.abs() <= 1e-12;
    let scale_only = null(m[1]) && null(m[2]);
    let translate_only = scale_only && null(m[0] - 1.0) && null(m[3] - 1.0);
    if translate_only && r[2] == iw as f64 && r[3] == ih as f64 {
        // The untransformed texture over the rounded rectangle (fillRect_normalized,
        // blend_untransformed_argb).
        let round = |v: f64| (v + 0.5).floor() as i64;
        let (x1, y1) = (round(r[0] + m[4]), round(r[1] + m[5]));
        let (x2, y2) = (round(r[0] + m[4] + r[2]), round(r[1] + m[5] + r[3]));
        let alpha = (255 * int_opacity) >> 8;
        for y in y1.max(0)..y2.min(h).min(y1 + ih) {
            for x in x1.max(0)..x2.min(w).min(x1 + iw) {
                let s = img.px[((y - y1) * iw + (x - x1)) as usize];
                source_over(&mut ctx.img.px[(y * w + x) as usize], s, alpha);
            }
        }
        return;
    }
    // The texture brush (image_filler_xform) through the rectangle's coverage.
    let mut mask = Context::new(w as usize, h as usize);
    mask.fill_rect_xf(r, [255; 4], 256, m);
    let mut tex = m;
    tex[4] += r[0] * tex[0] + r[1] * tex[2];
    tex[5] += r[0] * tex[1] + r[1] * tex[3];
    let (sx, sy) = (r[2] / iw as f64, r[3] / ih as f64);
    tex = [
        tex[0] * sx,
        tex[1] * sx,
        tex[2] * sy,
        tex[3] * sy,
        tex[4],
        tex[5],
    ];
    // QSpanData::setupMatrix: a 1/65536 shift keeps the fixed point rounding right.
    const D: f64 = 1.0 / 65536.0;
    let Some(inv) = invert(&compose([1.0, 0.0, 0.0, 1.0, D, D], tex)) else {
        return;
    };
    let fetch = Fetch { img, inv, smooth };
    let mut src = vec![[0u8; 4]; w as usize];
    for y in 0..h {
        let row = &mask.img.px[(y * w) as usize..((y + 1) * w) as usize];
        let mut x = 0usize;
        while x < row.len() {
            if row[x][3] == 0 {
                x += 1;
                continue;
            }
            // handleSpans: adjacent spans are one fetch, at most BufferSize pixels long.
            let end = (x..row.len())
                .find(|&i| row[i][3] == 0)
                .unwrap_or(row.len());
            let end = end.min(x + 2048);
            fetch.row(x as i64, y, &mut src[x..end]);
            for i in x..end {
                let alpha = (row[i][3] as u32 * int_opacity) >> 8;
                source_over(&mut ctx.img.px[y as usize * row.len() + i], src[i], alpha);
            }
            x = end;
        }
    }
}

struct Fetch<'a> {
    img: &'a Canvas,
    /// Device to texture (QSpanData m11 m12 m21 m22 dx dy).
    inv: M,
    /// Bilinear (QSpanData::bilinear), else the nearest pixel.
    smooth: bool,
}

impl Fetch<'_> {
    /// fetchTransformedBilinearARGB32PM for `out.len()` pixels from (x, y).
    fn row(&self, x: i64, y: i64, out: &mut [[u8; 4]]) {
        const ONE: i64 = 1 << 16;
        let inv = &self.inv;
        let (cx, cy) = (x as f64 + 0.5, y as f64 + 0.5);
        let fdx = (inv[0] * ONE as f64) as i64;
        let fdy = (inv[1] * ONE as f64) as i64;
        let mut fx = ((inv[2] * cy + inv[0] * cx + inv[4]) * ONE as f64) as i64 - ONE / 2;
        let mut fy = ((inv[3] * cy + inv[1] * cx + inv[5]) * ONE as f64) as i64 - ONE / 2;
        let (iw, ih) = (self.img.w as i64, self.img.h as i64);
        if !self.smooth {
            // fetchTransformed: the pixel the centre falls in.
            let mut fx = ((inv[2] * cy + inv[0] * cx + inv[4]) * ONE as f64) as i64;
            let mut fy = ((inv[3] * cy + inv[1] * cx + inv[5]) * ONE as f64) as i64;
            for o in out {
                let (px, py) = ((fx >> 16).clamp(0, iw - 1), (fy >> 16).clamp(0, ih - 1));
                *o = self.img.px[(py * iw + px) as usize];
                (fx, fy) = (fx + fdx, fy + fdy);
            }
            return;
        }
        // fetchTransformedBilinear_pixelBounds
        let bounds = |v: i64, max: i64| {
            if v < 0 {
                (0, 0)
            } else if v >= max - 1 {
                (max - 1, max - 1)
            } else {
                (v, v + 1)
            }
        };
        let px = |x: i64, y: i64| self.img.px[(y * iw + x) as usize].map(u32::from);
        let corners = |fx: i64, fy: i64| {
            let ((x1, x2), (y1, y2)) = (bounds(fx >> 16, iw), bounds(fy >> 16, ih));
            [px(x1, y1), px(x2, y1), px(x1, y2), px(x2, y2)]
        };
        let dist = |f: i64| ((f & 0xffff) >> 8) as u32;
        // interpolate_4_pixels: down the two columns, then across.
        let fine = |fx: i64, fy: i64| -> [u8; 4] {
            let ([tl, tr, bl, br], dx, dy) = (corners(fx, fy), dist(fx), dist(fy));
            std::array::from_fn(|c| {
                let l = (tl[c] * (256 - dy) + bl[c] * dy) >> 8;
                let r = (tr[c] * (256 - dy) + br[c] * dy) >> 8;
                ((l * (256 - dx) + r * dx) >> 8) as u8
            })
        };
        // interpolate_4_pixels_16: weights of 4 bits.
        let coarse = |fx: i64, fy: i64| -> [u8; 4] {
            let ([tl, tr, bl, br], dx, dy) = (corners(fx, fy), dist(fx), dist(fy));
            let (dx, dy) = ((dx + 8) >> 4, (dy + 8) >> 4);
            let dxy = dx * dy;
            let k = [(16 - dx) * (16 - dy), 16 * dx - dxy, 16 * dy - dxy, dxy];
            std::array::from_fn(|c| {
                ((tl[c] * k[0] + tr[c] * k[1] + bl[c] * k[2] + br[c] * k[3]) >> 8) as u8
            })
        };
        // The helper Qt takes: the simple scale, upscale and rotate ones weigh with 8 bits
        // everywhere, the downscale and fast rotate ones (AVX2) with 4 bits in runs of 8
        // pixels whose neighbours are all inside the picture.
        let rotate = fdy != 0;
        let fast = if rotate {
            inv[0].abs() >= 0.125 && inv[3].abs() >= 0.125
        } else {
            fdx.abs() > 2 * ONE && inv[3].abs() >= 0.125
        };
        let n = out.len() as i64;
        let mut i = 0;
        if fast {
            let edge = |f: i64, max: i64| {
                let (a, b) = bounds(f >> 16, max);
                a == b
            };
            while i < n && (edge(fx, iw) || (rotate && edge(fy, ih))) {
                out[i as usize] = fine(fx, fy);
                (fx, fy, i) = (fx + fdx, fy + fdy, i + 1);
            }
            let reach = |f: i64, fd: i64, max: i64| match fd {
                0 => i64::MAX,
                fd if fd > 0 => ((max - 1) * ONE - f) / fd,
                fd => -f / fd,
            };
            let mut bounded = n.min(i.saturating_add(reach(fx, fdx, iw)));
            if rotate {
                bounded = bounded.min(i.saturating_add(reach(fy, fdy, ih)));
            }
            while i < bounded - 7 {
                for _ in 0..8 {
                    out[i as usize] = coarse(fx, fy);
                    (fx, fy, i) = (fx + fdx, fy + fdy, i + 1);
                }
            }
        }
        while i < n {
            out[i as usize] = fine(fx, fy);
            (fx, fy, i) = (fx + fdx, fy + fdy, i + 1);
        }
    }
}
