//! Software emulation of Qt Quick's GL pipeline as executed by Mesa llvmpipe:
//! smoothcolor.vert, fixed-point triangle setup, unorm8 premultiplied blending.

use crate::rect::Geometry;

/// Identity of a pixel buffer for texture caches and draw signatures, like
/// `QSGTexture::comparisonKey`: drawn from a process-wide counter and never reused, so a key
/// can't name a later buffer the way a freed address can. A clone gets a new key.
///
/// It also says whether the buffer has an alpha channel, as `QSGTexture::hasAlphaChannel`
/// does; a clone keeps that.
#[derive(PartialEq, Eq, Hash, Debug)]
pub struct TexKey(u64, bool);

impl TexKey {
    pub fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        TexKey(
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            true,
        )
    }

    pub fn get(&self) -> u64 {
        self.0
    }

    /// False for a picture of a format with no alpha channel (`QImage::Format_RGB32`) whose
    /// alpha bytes are not all 255 all the same: Qt Quick draws it without blending.
    pub fn has_alpha(&self) -> bool {
        self.1
    }
}

impl Default for TexKey {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for TexKey {
    fn clone(&self) -> Self {
        TexKey(Self::new().0, self.1)
    }
}

/// Premultiplied RGBA pixels. A canvas shared through an `Rc` is not written again: its
/// `key` stands for its pixels wherever it is drawn.
#[derive(Clone)]
pub struct Canvas {
    pub w: usize,
    pub h: usize,
    pub px: Vec<[u8; 4]>,
    pub key: TexKey,
}

const SUB: f32 = 256.0;

thread_local! {
    /// The key of the canvas a window is being painted on, 0 when there is none.
    static WINDOW: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Until it is dropped, `cv` is a window's canvas: see [`Canvas::rows`].
pub struct Window(u64);

impl Window {
    pub fn new(cv: &Canvas) -> Self {
        Window(WINDOW.with(|w| w.replace(cv.key.get())))
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        WINDOW.with(|w| w.set(self.0));
    }
}

/// An edge at `v` where llvmpipe rasterizes it: on its grid of 1/256 of a pixel
/// (lp_setup_tri.c's subpixel_snap), so an edge a float's error off a pixel's centre is on it.
pub fn snapped(v: f64) -> f64 {
    (v * SUB as f64).round_ties_even() / SUB as f64
}

/// The columns `[x0, x1)` of `len` whose centres are in a shape from `x`, `w` wide: a centre
/// on the left edge is in, one on the right edge is out.
pub fn cols(x: f64, w: f64, len: usize) -> (usize, usize) {
    let edge = |v: f64| (snapped(v) - 0.5).ceil().max(0.0) as usize;
    (edge(x), edge(x + w).min(len))
}

/// A vertex in window pixels and its premultiplied colour.
type Vertex = ([f32; 2], [f32; 4]);

impl Canvas {
    /// Qt Quick draws a window through OpenGL bottom row first, and a layer's texture top row
    /// first. A row whose centre is exactly on a horizontal edge goes to the shape that is
    /// later in that order: the one above the edge in a window, the one below in a layer.
    pub fn is_window(&self) -> bool {
        WINDOW.with(|w| w.get()) == self.key.get()
    }

    /// The rows `[y0, y1)` of the canvas whose centres are in a shape from `y`, `h` tall.
    pub fn rows(&self, y: f64, h: f64) -> (usize, usize) {
        let edge = |v: f64| {
            let v = snapped(v);
            let r = if self.is_window() {
                (v - 0.5).floor() + 1.0
            } else {
                (v - 0.5).ceil()
            };
            r.max(0.0) as usize
        };
        (edge(y), edge(y + h).min(self.h))
    }

    /// The columns `[x0, x1)` of the canvas whose centres are in a shape from `x`, `w` wide.
    pub fn cols(&self, x: f64, w: f64) -> (usize, usize) {
        cols(x, w, self.w)
    }

    pub fn new(w: usize, h: usize, clear: [u8; 4]) -> Self {
        Self::from_px(w, h, vec![clear; w * h])
    }

    /// The canvas as a picture with no alpha channel (`TexKey::has_alpha`).
    pub fn without_alpha(mut self) -> Self {
        self.key.1 = false;
        self
    }

    pub fn from_px(w: usize, h: usize, px: Vec<[u8; 4]>) -> Self {
        Self {
            w,
            h,
            px,
            key: TexKey::new(),
        }
    }

    pub fn save_png(&self, path: &str) -> std::io::Result<()> {
        std::fs::write(path, self.png()?)
    }

    /// The canvas as a PNG file.
    pub fn png(&self) -> std::io::Result<Vec<u8>> {
        let mut out = Vec::new();
        self.write_png(&mut out)?;
        Ok(out)
    }

    fn write_png(&self, f: &mut Vec<u8>) -> std::io::Result<()> {
        let mut enc = png::Encoder::new(f, self.w as u32, self.h as u32);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut wr = enc.write_header().map_err(std::io::Error::other)?;
        let data: Vec<u8> = self.px.iter().flatten().copied().collect();
        wr.write_image_data(&data).map_err(std::io::Error::other)?;
        Ok(())
    }

    /// Draw smooth-color geometry translated by (tx, ty) in item space.
    pub fn draw_smooth(&mut self, g: &Geometry, tx: f32, ty: f32, opacity: f32) {
        let all = [0, 0, self.w as i64, self.h as i64];
        self.draw_smooth_scaled(g, [1.0, 1.0], tx, ty, opacity, all);
    }

    /// `draw_smooth` of an item at `place`, inside the scissor `clip` (x1 y1 x2 y2).
    pub fn draw_smooth_placed(
        &mut self,
        g: &Geometry,
        place: &Place,
        opacity: f32,
        clip: [i64; 4],
    ) {
        let (w, h) = (self.w as f32, self.h as f32);
        let verts = smooth_vertices_placed(g, place, opacity, [0.0, 0.0, w as f64, h as f64], w, h);
        for [a, b, c] in smooth_triangles(g) {
            self.tri(verts[a], verts[b], verts[c], clip);
        }
    }

    /// `draw_smooth` through the matrix `m` of [`smooth_vertices_matrix`], inside the scissor
    /// `clip` (x1 y1 x2 y2).
    pub fn draw_smooth_matrix(&mut self, g: &Geometry, m: [f32; 6], opacity: f32, clip: [i64; 4]) {
        let (w, h) = (self.w as f32, self.h as f32);
        let verts = smooth_vertices_matrix(g, m, opacity, w, h);
        for [a, b, c] in smooth_triangles(g) {
            self.tri(verts[a], verts[b], verts[c], clip);
        }
    }

    /// `draw_smooth` through a scale `s` of the scene (pixel = s * (item + t)), as a layer
    /// texture that is not its source rect's size is rendered, inside the scissor `clip`
    /// (x1 y1 x2 y2).
    pub fn draw_smooth_scaled(
        &mut self,
        g: &Geometry,
        s: [f32; 2],
        tx: f32,
        ty: f32,
        opacity: f32,
        clip: [i64; 4],
    ) {
        let (w, h) = (self.w as f32, self.h as f32);
        // An item that is only moved is merged into a batch (`smooth_vertices_over`).
        let verts = if s == [1.0, 1.0] {
            smooth_vertices(g, tx, ty, opacity, w, h)
        } else {
            smooth_vertices_scaled(g, s[0], s[1], s[0] * tx, s[1] * ty, opacity, w, h)
        };
        for [a, b, c] in smooth_triangles(g) {
            self.tri(verts[a], verts[b], verts[c], clip);
        }
    }

    /// The triangle inside the scissor `clip` (x1 y1 x2 y2).
    fn tri(&mut self, a: Vertex, b: Vertex, c: Vertex, clip: [i64; 4]) {
        #[cfg(target_arch = "x86_64")]
        if std::arch::is_x86_feature_detected!("fma") {
            // SAFETY: this processor has the FMA instructions `tri_fused` is compiled with.
            return unsafe { self.tri_fused(a, b, c, clip) };
        }
        self.tri_plain(a, b, c, clip);
    }

    /// `tri` on a processor with no fused multiply-add of its own.
    fn tri_plain(&mut self, a: Vertex, b: Vertex, c: Vertex, clip: [i64; 4]) {
        self.tri_with(a, b, c, clip, |p, x, y| {
            [0, 1, 2, 3].map(|ch| unorm(fma(p[2][ch], y, fma(p[1][ch], x, p[0][ch]))))
        });
    }

    /// `tri` with the processor's own fused multiply-add, the four channels at once.
    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "fma")]
    fn tri_fused(&mut self, a: Vertex, b: Vertex, c: Vertex, clip: [i64; 4]) {
        use std::arch::x86_64::*;
        self.tri_with(a, b, c, clip, |p, x, y| {
            let [at0, dx, dy] = p.map(|c| _mm_setr_ps(c[0], c[1], c[2], c[3]));
            let v = _mm_fmadd_ps(dy, _mm_set1_ps(y), _mm_fmadd_ps(dx, _mm_set1_ps(x), at0));
            // `unorm`: the whole part, and one more where the fraction left is a half or over.
            let v = _mm_min_ps(_mm_max_ps(v, _mm_setzero_ps()), _mm_set1_ps(1.0));
            let v = _mm_mul_ps(v, _mm_set1_ps(255.0));
            let whole = _mm_cvttps_epi32(v);
            let up = _mm_cmpge_ps(_mm_sub_ps(v, _mm_cvtepi32_ps(whole)), _mm_set1_ps(0.5));
            let n = _mm_sub_epi32(whole, _mm_castps_si128(up));
            let n = _mm_packs_epi32(n, n);
            _mm_cvtsi128_si32(_mm_packus_epi16(n, n)).to_le_bytes()
        });
    }

    /// `shade`: the colour at a pixel of the planes (the value at the origin, across, down, of
    /// the four channels each), the same bytes whichever way they are made.
    #[inline(always)]
    fn tri_with(
        &mut self,
        a: Vertex,
        b: Vertex,
        c: Vertex,
        clip: [i64; 4],
        shade: impl Fn(&[[f32; 4]; 3], f32, f32) -> [u8; 4],
    ) {
        // lp_setup_tri.c's subpixel_snap of the position less the half pixel offset, put back
        // as the 128 the samples below stand at.
        let snap = |v: f32| ties_even((v - 0.5) * SUB) as i64 + 128;
        let fx = |p: [f32; 2]| [snap(p[0]), snap(p[1])];
        let (mut pa, mut pb, pc) = (fx(a.0), fx(b.0), fx(c.0));
        let area = (pb[0] - pa[0]) * (pc[1] - pa[1]) - (pb[1] - pa[1]) * (pc[0] - pa[0]);
        if area == 0 {
            return;
        }
        if area < 0 {
            std::mem::swap(&mut pa, &mut pb);
        }
        let pts = [pa, pb, pc];
        let (cx, cy) = (clip[2].min(self.w as i64), clip[3].min(self.h as i64));
        if cx <= clip[0].max(0) || cy <= clip[1].max(0) {
            return;
        }
        let minx = (pts.iter().map(|p| p[0]).min().unwrap() >> 8).max(clip[0].max(0)) as usize;
        let maxx = ((pts.iter().map(|p| p[0]).max().unwrap() >> 8) + 1)
            .min(cx - 1)
            .max(0) as usize;
        let miny = (pts.iter().map(|p| p[1]).min().unwrap() >> 8).max(clip[1].max(0)) as usize;
        let maxy = ((pts.iter().map(|p| p[1]).max().unwrap() >> 8) + 1)
            .min(cy - 1)
            .max(0) as usize;
        if minx > maxx || miny > maxy {
            return;
        }
        // Edge functions; ties resolved with a top-left rule in image space (y down), and a
        // bottom-left one in a window (`Canvas::rows`).
        let window = self.is_window();
        let edge = |p0: [i64; 2], p1: [i64; 2], x: i64, y: i64| {
            (p1[0] - p0[0]) * (y - p0[1]) - (p1[1] - p0[1]) * (x - p0[0])
        };
        let bias = |p0: [i64; 2], p1: [i64; 2]| {
            let dy = p1[1] - p0[1];
            let dx = p1[0] - p0[0];
            let top_left = dy < 0 || (dy == 0 && (dx > 0) != window);
            if top_left { 0 } else { -1 }
        };
        let biases = [
            bias(pts[1], pts[2]),
            bias(pts[2], pts[0]),
            bias(pts[0], pts[1]),
        ];
        // The plane of each channel as llvmpipe sets it up (lp_state_setup.c, calc_coef4): its
        // vertex order (the last one last, the first two so that its area is positive), the
        // differences times one reciprocal of the area, the value at the origin of pixel
        // centres. Qt's pixels at half coverage depend on these roundings.
        let (l0, l1, l2) = if area < 0 { (a, b, c) } else { (b, a, c) };
        let (dx01, dy01) = (l0.0[0] - l1.0[0], l0.0[1] - l1.0[1]);
        let (dx20, dy20) = (l2.0[0] - l0.0[0], l2.0[1] - l0.0[1]);
        let ooa = 1.0 / (dx01 * dy20 - dx20 * dy01);
        let (dx01, dy01, dx20, dy20) = (dx01 * ooa, dy01 * ooa, dx20 * ooa, dy20 * ooa);
        let (x0, y0) = (l0.0[0] - 0.5, l0.0[1] - 0.5);
        let (da01, da20): ([f32; 4], [f32; 4]) = (
            std::array::from_fn(|ch| l0.1[ch] - l1.1[ch]),
            std::array::from_fn(|ch| l2.1[ch] - l0.1[ch]),
        );
        let dadx: [f32; 4] = std::array::from_fn(|ch| da01[ch] * dy20 - da20[ch] * dy01);
        let dady: [f32; 4] = std::array::from_fn(|ch| da20[ch] * dx01 - da01[ch] * dx20);
        let at0 = std::array::from_fn(|ch| l0.1[ch] - (dadx[ch] * x0 + dady[ch] * y0));
        let planes = [at0, dadx, dady];
        // One colour all over (every triangle but those of an antialiased edge or a gradient).
        let flat = (planes[1] == [0.0; 4] && planes[2] == [0.0; 4]).then(|| planes[0].map(unorm));
        // An edge function goes up or down by the same step from one pixel to the next, so
        // the pixels of a row that are inside are one run, found by division.
        let edges = [(pts[1], pts[2]), (pts[2], pts[0]), (pts[0], pts[1])];
        let steps = edges.map(|(p0, p1)| -(p1[1] - p0[1]) * 256);
        let sx0 = minx as i64 * 256 + 128;
        for py in miny..=maxy {
            let sy = py as i64 * 256 + 128;
            let (mut lo, mut hi) = (0i64, (maxx - minx) as i64);
            for i in 0..3 {
                let e = edge(edges[i].0, edges[i].1, sx0, sy) + biases[i];
                match steps[i] {
                    0 if e < 0 => hi = -1,
                    0 => {}
                    step if step > 0 => lo = lo.max(-(e.div_euclid(step))),
                    step => hi = hi.min(e.div_euclid(-step)),
                }
            }
            if lo > hi {
                continue;
            }
            let (x1, x2) = (minx + lo as usize, minx + hi as usize);
            let row = &mut self.px[py * self.w + x1..=py * self.w + x2];
            match flat {
                Some(s) if s[3] == 255 => row.fill(s),
                Some(s) => row.iter_mut().for_each(|d| over(d, s)),
                // lp_bld_interp.c, interp_attrib_linear: two fused multiply-adds.
                None => {
                    for (d, x) in row.iter_mut().zip(x1..) {
                        over(d, shade(&planes, x as f32, py as f32));
                    }
                }
            }
        }
    }
}

/// A colour channel as the byte the target keeps. It is at most 255.0 before it is rounded,
/// where adding a half in an `f64` and cutting the fraction off is `rnd`.
#[inline(always)]
fn unorm(v: f32) -> u8 {
    ((v.clamp(0.0, 1.0) * 255.0) as f64 + 0.5) as u8
}

/// Premultiplied `s` over `d`.
#[inline(always)]
fn over(d: &mut [u8; 4], s: [u8; 4]) {
    if s[3] == 255 {
        *d = s;
        return;
    }
    let inv = 255 - s[3] as u32;
    for ch in 0..4 {
        d[ch] = (s[ch] as u32 + mul255::nearest(d[ch] as u32, inv)).min(255) as u8;
    }
}

/// `f32::mul_add`, a fused multiply-add, without the libm call baseline x86-64 makes for it:
/// the product is exact in an `f64`, and the sum is rounded to odd there, so that the
/// rounding to `f32` is the only one.
#[inline(always)]
pub fn fma(a: f32, b: f32, c: f32) -> f32 {
    let (p, c) = (a as f64 * b as f64, c as f64);
    let s = p + c;
    let t = s - p;
    let err = (p - (s - t)) + (c - t);
    if err == 0.0 || s.to_bits() & 1 == 1 || !s.is_finite() {
        return s as f32;
    }
    let away = (err > 0.0) == (s > 0.0);
    f64::from_bits(if away {
        s.to_bits() + 1
    } else {
        s.to_bits() - 1
    }) as f32
}

#[test]
fn fma_matches_mul_add() {
    for (a, b, c) in [
        (1.0e-15, 1.0e-15, 1.0f32),
        (1.0e-15, -1.0e-15, 1.0),
        (-1.0e-15, 1.0e-15, -1.0),
        (0.0, 5.0, -0.0),
        (3.0, 1.0 / 3.0, -1.0),
        (f32::MAX, 2.0, 1.0),
        (f32::MAX, 2.0, f32::NEG_INFINITY),
    ] {
        assert_eq!(
            fma(a, b, c).to_bits(),
            a.mul_add(b, c).to_bits(),
            "{a} {b} {c}"
        );
    }
    let mut seed = 0x9E37_79B9_7F4A_7C15u64;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let any = |v: u64, exps: u64| {
        let m = f32::from_bits((v as u32 & 0x007f_ffff) | 0x3f80_0000);
        let e = 2f32.powi(((v >> 32) % exps) as i32 - (exps / 2) as i32);
        if v >> 63 == 0 { m * e } else { -m * e }
    };
    // A product a little under half of the last place of `c`: an `f64` cannot hold the
    // difference beside `c`, so the sum rounded there first is a tie, and goes wrong.
    let mut twice_rounded = 0;
    for k in 1..=64 {
        let e = k as f32 * 2f32.powi(-35);
        let (a, b) = (2f32.powi(-12) + e, 2f32.powi(-12) - e);
        for _ in 0..2000 {
            let c = any(next(), 1).abs();
            twice_rounded += ((a as f64 * b as f64 + c as f64) as f32 != c) as u32;
            for (a, c) in [(a, c), (-a, -c), (a, -c), (-a, c)] {
                assert_eq!(
                    fma(a, b, c).to_bits(),
                    a.mul_add(b, c).to_bits(),
                    "{a:e} {b:e} {c:e}"
                );
            }
        }
    }
    assert!(twice_rounded > 10_000, "{twice_rounded}");
    for _ in 0..2_000_000 {
        let (a, b, c) = (any(next(), 60), any(next(), 60), any(next(), 120));
        assert_eq!(
            fma(a, b, c).to_bits(),
            a.mul_add(b, c).to_bits(),
            "{a:e} {b:e} {c:e}"
        );
    }
}

/// The second piece of the running bar of crates/shell/tests/progress_bar_qt/running.txt 653
/// ms after it was shown: 4 high from y 29.5, so its first and last rows are half covered.
/// Qt (llvmpipe) rounds the last one down right of the diagonal of its two triangles, whose
/// plane has a slope of 0.99999994 in place of 1, and nowhere else.
#[test]
fn a_half_covered_row_rounds_as_llvmpipe_rounds_it() {
    let v = 1.0 - (1.0 - (653.0 - 520.0) / 1240.0f64).powi(3);
    let at = v * 240.0;
    let g = crate::rect::build(&crate::rect::RectParams {
        y: -0.5,
        width: (v * (240.0 - at)) as f32,
        height: 4.0,
        color: [0x80, 0xc0, 0xff, 0xff],
        antialiasing: true,
        ..Default::default()
    });
    let mut cv = Canvas::new(300, 64, [0x20, 0x20, 0x20, 0xff]);
    cv.draw_smooth(&g, (10.0 + at) as f32, 30.0, 1.0);
    let row = |y: usize, xs: std::ops::RangeInclusive<usize>| -> Vec<[u8; 4]> {
        xs.map(|x| cv.px[y * 300 + x]).collect()
    };
    assert_eq!(row(29, 80..=127), vec![[0x50, 0x70, 0x90, 0xff]; 48]);
    assert_eq!(row(31, 80..=127), vec![[0x80, 0xc0, 0xff, 0xff]; 48]);
    assert_eq!(row(33, 80..=102), vec![[0x50, 0x70, 0x90, 0xff]; 23]);
    assert_eq!(row(33, 103..=127), vec![[0x50, 0x70, 0x8f, 0xff]; 25]);
}

/// A gradient under rounded, antialiased corners, half see-through and off the pixel grid:
/// every pixel is the same with the processor's fused multiply-add and without it, and
/// `unorm` is `rnd` of every channel there is.
#[test]
fn both_ways_of_a_triangle_make_the_same_pixels() {
    let g = crate::rect::build(&crate::rect::RectParams {
        width: 180.0,
        height: 90.0,
        radius: 17.0,
        color: [0x80, 0xc0, 0xff, 0xff],
        antialiasing: true,
        stops: vec![
            (0.0, [255, 0, 0, 255]),
            (0.4, [0, 255, 0, 128]),
            (1.0, [0, 0, 255, 255]),
        ],
        ..Default::default()
    });
    let (w, h) = (200usize, 110usize);
    let mut one = Canvas::new(w, h, [0x20, 0x30, 0x40, 0xff]);
    let mut other = Canvas::new(w, h, [0x20, 0x30, 0x40, 0xff]);
    let all = [0, 0, w as i64, h as i64];
    for opacity in [1.0, 0.37] {
        let verts = smooth_vertices(&g, 9.3, 7.7, opacity, w as f32, h as f32);
        for [a, b, c] in smooth_triangles(&g) {
            one.tri(verts[a], verts[b], verts[c], all);
            other.tri_plain(verts[a], verts[b], verts[c], all);
        }
    }
    assert!(one.px == other.px);
    assert!(one.px.iter().any(|p| p[0] != p[2]));
    let mut v = 0.0f32;
    while v <= 1.0 {
        for v in [v, f32::from_bits(v.to_bits() + 1)] {
            assert_eq!(unorm(v), rnd(v.clamp(0.0, 1.0) * 255.0) as u8, "{v}");
        }
        v += 1.0 / 65536.0;
    }
    for v in [-1.0, 2.0, f32::NAN, 0.49999997 / 255.0, 0.5 / 255.0] {
        assert_eq!(unorm(v), rnd(v.clamp(0.0, 1.0) * 255.0) as u8, "{v}");
    }
}

/// `f32::round_ties_even` (cvtps2dq, as llvmpipe rounds) without the libm call baseline
/// x86-64 makes for it.
#[inline(always)]
fn ties_even(x: f32) -> f32 {
    if x.is_nan() || x.abs() >= 8_388_608.0 {
        return x;
    }
    let k = 8_388_608.0f32.copysign(x);
    (x + k) - k
}

/// `f32::round` (half away from zero) without the libm call baseline x86-64 makes for it.
#[inline(always)]
pub fn rnd(x: f32) -> f32 {
    if x.is_nan() || x.abs() >= 8_388_608.0 {
        return x.round();
    }
    let t = x as i32 as f32;
    let d = x - t;
    if d >= 0.5 {
        t + 1.0
    } else if d <= -0.5 {
        t - 1.0
    } else {
        t
    }
}

#[test]
fn rnd_matches_round() {
    let mut x = -70000.0f32;
    while x < 70000.0 {
        for v in [x, x + 0.5, x - 0.5, x + 0.49999997, x - 0.49999997] {
            assert_eq!(rnd(v), v.round(), "{v}");
        }
        x += 0.37;
    }
    for v in [0.0, -0.0, 1e9, -1e9, f32::NAN, 0.49999997, 254.5, 8388607.5] {
        assert!(rnd(v) == v.round() || v.is_nan(), "{v}");
    }
}

impl Canvas {
    /// [`blend_f`] onto the pixel at (x, y).
    pub fn blend_f(&mut self, x: usize, y: usize, s: [f32; 4]) {
        blend_f(&mut self.px[y * self.w + x], s);
    }

    pub fn blend_px(&mut self, x: usize, y: usize, s: [u32; 4]) {
        let d = &mut self.px[y * self.w + x];
        let inv = 255 - s[3];
        for ch in 0..4 {
            d[ch] = (s[ch] + mul255::nearest(d[ch] as u32, inv)).min(255) as u8;
        }
    }
}

/// A colour a shader computed, as llvmpipe writes it into a target of bytes: clamped, the
/// half rounded to even.
#[inline]
pub fn bytes(c: [f32; 4]) -> [u8; 4] {
    c.map(|c| (c.clamp(0.0, 1.0) * 255.0).round_ties_even() as u8)
}

/// Source-over of a premultiplied float color onto an 8-bit pixel, as llvmpipe blends: the
/// colour becomes bytes first, then the bytes are blended ([`mul255::nearest`]).
#[inline]
pub fn blend_f(d: &mut [u8; 4], s: [f32; 4]) {
    let s = bytes(s);
    let inv = 255 - s[3] as u32;
    for ch in 0..4 {
        d[ch] = (s[ch] as u32 + mul255::nearest(d[ch] as u32, inv)).min(255) as u8;
    }
}

/// QSGBatchRenderer scissor for `clip: true` on an axis-aligned item. `tx`/`ty` is the item's
/// accumulated translation as the float QMatrix4x4 holds it; projection and NDC math follow
/// Qt (float matrix, qreal mapping, qRound in GL bottom-up space). Returns top-down x1 y1 x2 y2.
pub fn scissor_rect(tx: f32, ty: f32, w: f64, h: f64, cw: usize, ch: usize) -> [i64; 4] {
    scissor_rect_scaled(tx, ty, w, h, [cw as f32, ch as f32], cw, ch)
}

/// `scissor_rect` on a render target of `cw` x `ch` pixels that shows `scene` units (a window
/// whose devicePixelRatio is not 1, a layer texture of a rect whose size is not whole): the
/// projection takes the item's rect to the target's pixels and Qt rounds there, once.
pub fn scissor_rect_scaled(
    tx: f32,
    ty: f32,
    w: f64,
    h: f64,
    [pw, ph]: [f32; 2],
    cw: usize,
    ch: usize,
) -> [i64; 4] {
    let (m00, m03) = (2.0 / pw, (2.0 / pw) * tx + -1.0);
    let (m11, m13) = (-2.0 / ph, (-2.0 / ph) * ty + 1.0);
    let r = |v: f64| (v + 0.5).floor() as i64;
    let (mut fx1, mut fx2) = (m03 as f64, w * m00 as f64 + m03 as f64);
    let (mut fy1, mut fy2) = (h * m11 as f64 + m13 as f64, m13 as f64);
    if fx1 > fx2 {
        std::mem::swap(&mut fx1, &mut fx2);
    }
    if fy1 > fy2 {
        std::mem::swap(&mut fy1, &mut fy2);
    }
    let (dw, dh) = (cw as f64, ch as f64);
    let ix1 = r((fx1 + 1.0) * dw * 0.5);
    let ix2 = r((fx2 + 1.0) * dw * 0.5);
    let iy1 = r((fy1 + 1.0) * dh * 0.5);
    let iy2 = r((fy2 + 1.0) * dh * 0.5);
    [ix1, ch as i64 - iy2, ix2, ch as i64 - iy1]
}

impl Canvas {
    /// Restores every pixel outside `rect` (x1, y1, x2, y2) from `saved`: scissor clipping.
    pub fn apply_scissor(&mut self, saved: &Canvas, rect: [i64; 4]) {
        for y in 0..self.h {
            for x in 0..self.w {
                let (xi, yi) = (x as i64, y as i64);
                if xi < rect[0] || xi >= rect[2] || yi < rect[1] || yi >= rect[3] {
                    self.px[y * self.w + x] = saved.px[y * self.w + x];
                }
            }
        }
    }

    pub fn snapshot(&self) -> Canvas {
        Canvas::from_px(self.w, self.h, self.px.clone())
    }
}

/// smoothcolor.vert on the CPU: window-space positions (pixels, y down) and premultiplied colors.
pub fn smooth_vertices(
    g: &Geometry,
    tx: f32,
    ty: f32,
    opacity: f32,
    w: f32,
    h: f32,
) -> Vec<([f32; 2], [f32; 4])> {
    smooth_vertices_over(g, tx, ty, opacity, [0.0, 0.0, w as f64, h as f64], w, h)
}

/// The vertices of an item that is only moved, by (`tx`, `ty`), drawn on `w` x `h` device
/// pixels where Qt's projection is over the rect `over` (x y w h, scene units): a window's
/// size from the origin, or a layer's source rect (QSGRhiLayer::grab), whose corner is not
/// the origin when the source is padded. QSGBatchRenderer merges such a node into a batch:
/// the vertices are moved before they are uploaded (`Pt::map`), and the shader's matrix is
/// the projection alone, which QMatrix4x4::ortho makes of the rect's sides.
pub fn smooth_vertices_over(
    g: &Geometry,
    tx: f32,
    ty: f32,
    opacity: f32,
    over: [f64; 4],
    w: f32,
    h: f32,
) -> Vec<([f32; 2], [f32; 4])> {
    in_window(smooth_clip_over(g, tx, ty, opacity, over, w, h), w, h)
}

/// `smooth_vertices_over` before the viewport, as [`smooth_clip_scaled`].
pub fn smooth_clip_over(
    g: &Geometry,
    tx: f32,
    ty: f32,
    opacity: f32,
    over: [f64; 4],
    w: f32,
    h: f32,
) -> Vec<([f32; 2], [f32; 4])> {
    smooth_clip_under(g, ortho(over), [tx, ty], opacity, w, h)
}

/// Where QSGBatchRenderer has an item: its batch root's matrix and its own matrix under that
/// root, both only translations here, in floats. An item's QSGTransformNode multiplies the
/// matrix above it by a translation to the item (`Updater::visitTransformNode`), so the places
/// of nested items add up one by one in floats; the QSGClipNode of an item that clips is a
/// batch root (`Updater::visitClipNode`): what was added up so far goes to the root's matrix,
/// which the shader gets with the projection, and the matrix below starts from identity,
/// which is what a merged batch adds to the vertices. The last bit of each decides how an
/// antialiased edge pixel rounds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Place {
    /// The translation of the batch root's matrix.
    pub root: [f32; 2],
    /// The translation from the batch root to the item.
    pub rel: [f32; 2],
    /// The same place in the target's coordinates, as the scene adds it up in doubles.
    pub at: [f64; 2],
}

impl Place {
    /// The place of Qt's root node, which is at `at` in the target.
    pub fn origin(at: [f64; 2]) -> Place {
        Place {
            root: [0.0; 2],
            rel: [0.0; 2],
            at,
        }
    }

    /// The place of an item at `own` in a parent that is `extra` away from this place (a
    /// node between them, like a Flickable's contentItem), with a clip node when it clips.
    pub fn step(self, extra: [f64; 2], own: [f64; 2], clip: bool) -> Place {
        let mut p = self;
        for k in 0..2 {
            p.rel[k] = p.rel[k] + extra[k] as f32 + own[k] as f32;
            p.at[k] = p.at[k] + extra[k] + own[k];
            if clip {
                p.root[k] += p.rel[k];
                p.rel[k] = 0.0;
            }
        }
        p
    }

    /// Whether this is the place of what is drawn at `scene` (scene units): closer than
    /// llvmpipe's 1/256 of a pixel, so that only the last bits are taken from here.
    pub fn is_at(&self, scene: [f64; 2]) -> bool {
        (0..2).all(|k| (self.root[k] as f64 + self.rel[k] as f64 - scene[k]).abs() < 1.0 / 256.0)
    }
}

/// `smooth_vertices_over` of an item at `place`.
pub fn smooth_vertices_placed(
    g: &Geometry,
    place: &Place,
    opacity: f32,
    over: [f64; 4],
    w: f32,
    h: f32,
) -> Vec<([f32; 2], [f32; 4])> {
    in_window(smooth_clip_placed(g, place, opacity, over, w, h), w, h)
}

/// `smooth_clip_over` of an item at `place`: the shader's matrix is the projection times the
/// batch root's (QMatrix4x4's product of a scale and a translation), and the vertices are
/// moved by the rest.
pub fn smooth_clip_placed(
    g: &Geometry,
    place: &Place,
    opacity: f32,
    over: [f64; 4],
    w: f32,
    h: f32,
) -> Vec<([f32; 2], [f32; 4])> {
    let [m00, m11, m30, m31] = ortho(over);
    let m = [
        m00,
        m11,
        m30 + m00 * place.root[0],
        m31 + m11 * place.root[1],
    ];
    smooth_clip_under(g, m, place.rel, opacity, w, h)
}

/// QMatrix4x4::ortho over the rect `over` (x y w h), y down: m00, m11, m30, m31.
pub(crate) fn ortho(over: [f64; 4]) -> [f32; 4] {
    let (left, right) = (over[0] as f32, (over[0] + over[2]) as f32);
    let (top, bottom) = (over[1] as f32, (over[1] + over[3]) as f32);
    let (width, height) = (right - left, bottom - top);
    [
        2.0 / width,
        -(2.0 / height),
        -(left + right) / width,
        (top + bottom) / height,
    ]
}

/// `smooth_vertices` under an item scale `sx`, `sy` (smoothcolor.vert with the scaled matrix:
/// such a node is not merged, its matrix goes to the shader).
#[allow(clippy::too_many_arguments)]
pub fn smooth_vertices_scaled(
    g: &Geometry,
    sx: f32,
    sy: f32,
    tx: f32,
    ty: f32,
    opacity: f32,
    w: f32,
    h: f32,
) -> Vec<([f32; 2], [f32; 4])> {
    in_window(smooth_clip_scaled(g, sx, sy, tx, ty, opacity, w, h), w, h)
}

/// `smooth_vertices_scaled` before the viewport: what the vertex shader gives as gl_Position
/// (x right, y up, -1 to 1 over a target of `w` x `h`), for a GPU to map itself.
#[allow(clippy::too_many_arguments)]
pub fn smooth_clip_scaled(
    g: &Geometry,
    sx: f32,
    sy: f32,
    tx: f32,
    ty: f32,
    opacity: f32,
    w: f32,
    h: f32,
) -> Vec<([f32; 2], [f32; 4])> {
    let m = [
        2.0f32 / w * sx,
        -2.0f32 / h * sy,
        2.0f32 / w * tx + -1.0f32,
        -2.0f32 / h * ty + 1.0f32,
    ];
    smooth_clip_under(g, m, [0.0; 2], opacity, w, h)
}

/// smoothcolor.vert: the vertices moved by `moved`, then under the matrix (`m00`, `m11`,
/// `m30`, `m31`), as gl_Position; a pixel is one of `w` x `h` device pixels.
fn smooth_clip_under(
    g: &Geometry,
    [m00, m11, m30, m31]: [f32; 4],
    moved: [f32; 2],
    opacity: f32,
    w: f32,
    h: f32,
) -> Vec<([f32; 2], [f32; 4])> {
    let ps = [2.0f32 / w, 2.0f32 / h];
    g.vertices
        .iter()
        .map(|v| {
            let pos = [m00 * (v.x + moved[0]) + m30, m11 * (v.y + moved[1]) + m31];
            let mut gp = pos;
            for (axis, off) in [(0usize, v.dx), (1usize, v.dy)] {
                if off != 0.0 {
                    let delta = if axis == 0 {
                        [m00 * off, 0.0]
                    } else {
                        [0.0, m11 * off]
                    };
                    let dir = delta;
                    let q = [dir[0] / ps[0], dir[1] / ps[1]];
                    let len = (q[0] * q[0] + q[1] * q[1]).sqrt();
                    let ndir = [0.5 * ps[0] * (q[0] / len), 0.5 * ps[1] * (q[1] / len)];
                    let numerator = dir[0] * ndir[0] + dir[1] * ndir[1];
                    let scale = if numerator < 0.0 {
                        1.0
                    } else {
                        (numerator / (dir[0] * dir[0] + dir[1] * dir[1])).min(1.0)
                    };
                    gp[0] += scale * delta[0];
                    gp[1] += scale * delta[1];
                }
            }
            (gp, v.color.map(|c| crate::layer::unorm(c) * opacity))
        })
        .collect()
}

/// `smooth_vertices` under an item matrix `m` (a QTransform's m11 m12 m21 m22 dx dy, to window
/// pixels), as QSGBatchRenderer draws a node under a QSGTransformNode. Geometry without
/// antialiasing offsets is QSGVertexColorMaterial's, which batches merge: the matrix maps its
/// vertices before the projection (`Renderer::uploadMergedElement`). Geometry with them is
/// QSGSmoothColorMaterial's, which needs the full matrix: smoothcolor.vert gets projection
/// times `m`.
pub fn smooth_vertices_matrix(
    g: &Geometry,
    m: [f32; 6],
    opacity: f32,
    w: f32,
    h: f32,
) -> Vec<([f32; 2], [f32; 4])> {
    in_window(smooth_clip_matrix(g, m, opacity, w, h), w, h)
}

/// `smooth_vertices_matrix` before the viewport, as [`smooth_clip_scaled`].
pub fn smooth_clip_matrix(
    g: &Geometry,
    m: [f32; 6],
    opacity: f32,
    w: f32,
    h: f32,
) -> Vec<([f32; 2], [f32; 4])> {
    smooth_clip_matrix_over(g, m, opacity, [0.0, 0.0, w as f64, h as f64], w, h)
}

/// `smooth_clip_matrix` on `w` x `h` device pixels where Qt's projection is over the rect
/// `over` (as [`smooth_vertices_over`]) and `m` is to that rect's scene units: the scale to
/// pixels is in neither, only the antialiasing's pixel size is of the target.
pub fn smooth_clip_matrix_over(
    g: &Geometry,
    m: [f32; 6],
    opacity: f32,
    over: [f64; 4],
    w: f32,
    h: f32,
) -> Vec<([f32; 2], [f32; 4])> {
    let [px, py, ox, oy] = ortho(over);
    if g.vertices.iter().any(|v| v.dx != 0.0 || v.dy != 0.0) {
        return smooth_shader(g, m, opacity, [px, py, ox, oy], [w, h]);
    }
    g.vertices
        .iter()
        .map(|v| {
            // Pt::map, then vertexcolor.vert with the projection.
            let x = v.x * m[0] + v.y * m[2] + m[4];
            let y = v.x * m[1] + v.y * m[3] + m[5];
            let gp = [px * x + ox, py * y + oy];
            (gp, v.color.map(|c| crate::layer::unorm(c) * opacity))
        })
        .collect()
}

/// Where the viewport of a `w` x `h` target puts a gl_Position, in pixels with y down:
/// llvmpipe's one fused multiply-add a coordinate (draw_llvm.c generate_viewport), y flipped
/// as in a window.
pub fn viewport(gp: [f32; 2], w: f32, h: f32) -> [f32; 2] {
    [
        fma(gp[0], w * 0.5, w * 0.5),
        fma(gp[1], -(h * 0.5), h * 0.5),
    ]
}

fn in_window(mut v: Vec<([f32; 2], [f32; 4])>, w: f32, h: f32) -> Vec<([f32; 2], [f32; 4])> {
    for (p, _) in &mut v {
        *p = viewport(*p, w, h);
    }
    v
}

/// smoothcolor.vert under projection times the matrix `m` (m11 m12 m21 m22 dx dy): gl_Position
/// and the colour. The projection is `ortho`'s, the pixels are `dev`'s.
fn smooth_shader(
    g: &Geometry,
    m: [f32; 6],
    opacity: f32,
    [px, py, ox, oy]: [f32; 4],
    dev: [f32; 2],
) -> Vec<([f32; 2], [f32; 4])> {
    // The columns of the matrix: where the item's x and y axes and its origin go.
    let cols = [[px * m[0], py * m[1]], [px * m[2], py * m[3]]];
    let m30 = px * m[4] + ox;
    let m31 = py * m[5] + oy;
    let ps = [2.0f32 / dev[0], 2.0f32 / dev[1]];
    g.vertices
        .iter()
        .map(|v| {
            let pos = [
                cols[0][0] * v.x + cols[1][0] * v.y + m30,
                cols[0][1] * v.x + cols[1][1] * v.y + m31,
            ];
            let mut gp = pos;
            for (axis, off) in [(0usize, v.dx), (1usize, v.dy)] {
                if off != 0.0 {
                    let delta = [cols[axis][0] * off, cols[axis][1] * off];
                    let dir = delta;
                    let q = [dir[0] / ps[0], dir[1] / ps[1]];
                    let len = (q[0] * q[0] + q[1] * q[1]).sqrt();
                    let ndir = [0.5 * ps[0] * (q[0] / len), 0.5 * ps[1] * (q[1] / len)];
                    let numerator = dir[0] * ndir[0] + dir[1] * ndir[1];
                    let scale = if numerator < 0.0 {
                        1.0
                    } else {
                        (numerator / (dir[0] * dir[0] + dir[1] * dir[1])).min(1.0)
                    };
                    gp[0] += scale * delta[0];
                    gp[1] += scale * delta[1];
                }
            }
            (gp, v.color.map(|c| crate::layer::unorm(c) * opacity))
        })
        .collect()
}

/// Non-degenerate triangles of the joined triangle strips.
pub fn smooth_triangles(g: &Geometry) -> impl Iterator<Item = [usize; 3]> + '_ {
    // Qt draws each section as its own triangle strip; the index buffer joins them with degenerate triangles.
    g.indices.windows(3).filter_map(|t| {
        let (a, b, c) = (t[0] as usize, t[1] as usize, t[2] as usize);
        (a != b && b != c && a != c).then_some([a, b, c])
    })
}
