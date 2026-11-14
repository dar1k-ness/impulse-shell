//! Qt5Compat GaussianBlur (GaussianBlur.qml + qgfxshaderbuilder.cpp optimized two-pass shader).

use crate::layer::{Interpolated, Tap, linear, unorm};
use crate::raster::Canvas;
use crate::tri::Tri;
use std::sync::atomic::{AtomicU64, Ordering};

/// Texels the blur passes have read since the process started: what a blur costs, whatever
/// the machine. A tap reads two texels of a pixel, one where it is on a texel's centre.
static TEXEL_READS: AtomicU64 = AtomicU64::new(0);

/// See [`TEXEL_READS`].
pub fn texel_reads() -> u64 {
    TEXEL_READS.load(Ordering::Relaxed)
}

fn gaussian(x: f64, d: f64) -> f64 {
    (-x * x / (2.0 * d * d)).exp()
}

/// qgfx_buildGaussSamplePoints → (offset in texels, normalized weight) per tap, each as the
/// generated shader holds it: written with `QByteArray::number` (6 significant digits) and
/// read back as a float literal.
pub fn taps(kernel_radius: i32, deviation: f64) -> Vec<(f32, f32)> {
    let req = kernel_radius * 2 + 1;
    let samples = (1 + req / 2) as usize;
    let radius = (req / 4) as usize;
    let mut p = vec![(0.0f64, 0.0f64); samples];
    let pair = |p0: f64, p1: f64| {
        let (w0, w1) = (gaussian(p0, deviation), gaussian(p1, deviation));
        let w = w0 + w1;
        let s = (p0 * w0 + p1 * w1) / w;
        if s.is_nan() { (0.0, 0.0) } else { (s, w) }
    };
    if samples % 2 == 1 {
        p[radius] = (0.0, 1.0);
        for i in 0..radius {
            let (s, w) = pair(((i + 1) * 2 - 1) as f64, ((i + 1) * 2) as f64);
            p[radius - i - 1] = (s, w);
            p[radius + i + 1] = (-s, w);
        }
    } else {
        let (wl, wc) = (gaussian(-1.0, deviation), gaussian(0.0, deviation));
        let w = wl + wc;
        p[radius] = (-wl / w, w);
        p[radius + 1] = (1.0, wl);
        for i in 0..radius {
            let (s, w) = pair(((i + 1) * 2) as f64, ((i + 1) * 2 + 1) as f64);
            p[radius - i - 1] = (s, w);
            p[radius + i + 2] = (-s, w);
        }
    }
    let sum: f64 = p.iter().map(|t| t.1).sum();
    p.iter().map(|&(s, w)| (baked(s), baked(w / sum))).collect()
}

pub const MAX_BLUR_SAMPLES: i32 = 64;

/// The widest blur drawn: `radius` and the kernel radius (`samples` / 2) are clamped to it.
/// Both come from QML (the original's lock screen takes them from the user's config, 100 by default)
/// and size the tap list, the transparent border and every pass's work; Qt has no limit and
/// builds a shader of as many taps as asked, so an absurd value would end the process or
/// stall the frame. 513 taps also fit the uniform array of the GPU's looped shader.
pub const MAX_BLUR_RADIUS: i32 = 256;

/// The transparent border a blur of `radius` grows by on each side with `transparentBorder`:
/// GaussianBlur.qml's `radius`, a real (the effect is drawn at -radius).
pub fn border(radius: f64) -> f64 {
    if radius.is_nan() {
        return 0.0;
    }
    radius.clamp(0.0, MAX_BLUR_RADIUS as f64)
}

/// GaussianBlur.qml `_paddedTexWidth`/`_paddedTexHeight` of an effect `size` wide with the
/// transparent border `r`: `property int: width + 2 * radius`, which QML truncates.
pub fn padded(size: f64, r: f64) -> usize {
    (size + 2.0 * r).max(0.0) as usize
}

/// QByteArray::number(double) — 6 significant digits, as baked into the generated shader.
fn baked(v: f64) -> f32 {
    format!("{:.5e}", v).parse().unwrap()
}

/// qgfx_fallbackFragmentShader's taps, one per texel offset r in -kr..=kr, and its `wSum`:
/// the shader adds the taps with the weights as they are and divides by `wSum` once, in the
/// colour it writes (`(qt_Opacity / wSum) * result`).
pub fn fallback_taps(kernel_radius: i32, deviation: f64) -> (Vec<(f32, f32)>, f32) {
    let mut w_sum = 0.0f32;
    let ws = (-kernel_radius..=kernel_radius)
        .map(|r| {
            let w = gaussian(r as f64, deviation) as f32;
            w_sum += w;
            (r as f32, baked(w as f64))
        })
        .collect();
    (ws, baked(w_sum as f64))
}

/// What each tap of a pass reads, when `src` is the destination's size, `dir` is one texel
/// along an axis and a pixel's centre is a texel's centre across it: a tap's linear read is
/// then two texels of one row or column.
enum Reads {
    /// A pass down: per tap, the two rows each row of pixels reads.
    Down(Vec<Vec<Tap>>),
    /// A pass along a row, the row padded with `pad` copies of each edge texel: per tap, the
    /// runs of pixels that read neighbouring texels with one weight.
    Across { runs: Vec<Vec<Run>>, pad: usize },
}

/// `len` pixels from `at` on: the first reads `tap`, each next one a texel further.
struct Run {
    at: usize,
    len: usize,
    tap: Tap,
}

/// The [`Reads`] of a pass; `None` for a pass that is not one of those, which samples each
/// tap in each pixel.
fn taps_along(
    dst: &Canvas,
    src: &Canvas,
    taps: &[(f32, f32)],
    spread: f32,
    dir: (f32, f32),
) -> Option<Reads> {
    let down = if dir == (1.0 / src.w as f32, 0.0) {
        false
    } else if dir == (0.0, 1.0 / src.h as f32) {
        true
    } else {
        return None;
    };
    if (src.w, src.h) != (dst.w, dst.h) {
        return None;
    }
    // The coordinates as `sampled_pass` has them.
    let (along, across) = if down { (src.h, src.w) } else { (src.w, src.h) };
    let centre = |i: usize, n: usize| ((i as f64 + 0.5) / n as f64) as f32;
    let step = if down { dir.1 } else { dir.0 };
    if !(0..across).all(|i| Tap::linear(centre(i, across) as f64, across).is_texel(i)) {
        return None;
    }
    let at = |i: usize, pos: f32| (centre(i, along) + spread * step * pos) as f64;
    if down {
        let reads =
            |&(pos, _): &(f32, f32)| (0..along).map(|i| Tap::linear(at(i, pos), along)).collect();
        return Some(Reads::Down(taps.iter().map(reads).collect()));
    }
    let reach = taps.iter().fold(0.0f32, |m, t| m.max((spread * t.0).abs()));
    let pad = reach.ceil() as usize + 2;
    let runs = |&(pos, _): &(f32, f32)| {
        let mut runs = Vec::<Run>::new();
        let mut prev = None;
        for i in 0..along {
            let tap = Tap::padded(at(i, pos), along, pad);
            match runs.last_mut() {
                Some(run) if prev.is_some_and(|p| tap.follows(p)) => run.len += 1,
                _ => runs.push(Run { at: i, len: 1, tap }),
            }
            prev = Some(tap);
        }
        runs
    };
    Some(Reads::Across {
        runs: taps.iter().map(runs).collect(),
        pad,
    })
}

/// `acc += wt * texel`, per channel, the texel as the shader reads it.
fn add_row(acc: &mut [f32], wt: f32, row: &[u8]) {
    for (a, &s) in acc.iter_mut().zip(row) {
        *a += wt * unorm(s);
    }
}

/// Rows `first..` of a pass with [`taps_along`] into `band`: what `sampled_pass` writes, a
/// row at a time. A tap's reads of a row are blended as bytes first, then weighed; a tap on
/// a texel's centre in every pixel of the row reads the texels as they are.
#[allow(clippy::too_many_arguments)]
fn convolve_rows(
    band: &mut [[u8; 4]],
    first: usize,
    src: &Canvas,
    taps: &[(f32, f32)],
    reads: &Reads,
    sum: f32,
    opacity: f32,
    alpha_only: Option<([f32; 4], f32)>,
) {
    let w = src.w;
    let mut acc = vec![0.0f32; 4 * w];
    let mut read = vec![0u8; 4 * w];
    let mut padded = match reads {
        Reads::Across { pad, .. } => vec![[0u8; 4]; pad + w + pad],
        Reads::Down(_) => Vec::new(),
    };
    let mut texels = 0;
    for (y, out) in band.chunks_exact_mut(w).enumerate() {
        let y = first + y;
        acc.fill(0.0);
        match reads {
            Reads::Down(rows) => {
                for (&(_, wt), rows) in taps.iter().zip(rows) {
                    let t = rows[y];
                    let (a, b) = (&src.px[t.i0 * w..][..w], &src.px[t.i1 * w..][..w]);
                    if t.is_whole() {
                        add_row(&mut acc, wt, a.as_flattened());
                        texels += w;
                        continue;
                    }
                    t.mix_bytes(&mut read, a.as_flattened(), b.as_flattened());
                    add_row(&mut acc, wt, &read);
                    texels += 2 * w;
                }
            }
            Reads::Across { runs, pad } => {
                let row = &src.px[y * w..][..w];
                padded[..*pad].fill(row[0]);
                padded[*pad..pad + w].copy_from_slice(row);
                padded[pad + w..].fill(row[w - 1]);
                for (&(_, wt), runs) in taps.iter().zip(runs) {
                    if let [run] = &runs[..]
                        && run.tap.is_whole()
                    {
                        add_row(&mut acc, wt, padded[run.tap.i0..][..w].as_flattened());
                        texels += w;
                        continue;
                    }
                    for run in runs {
                        let row = padded[run.tap.i0..][..run.len + 1].as_flattened();
                        let out = &mut read[4 * run.at..][..4 * run.len];
                        run.tap.mix_bytes(out, row, &row[4..]);
                    }
                    add_row(&mut acc, wt, &read);
                    texels += 2 * w;
                }
            }
        }
        for (px, c) in out.iter_mut().zip(acc.as_chunks::<4>().0) {
            let c = match alpha_only {
                Some((color, thickness)) => {
                    let k = ((c[3] / sum) / thickness).clamp(0.0, 1.0);
                    color.map(|v| v * k * opacity)
                }
                None => c.map(|v| (opacity / sum) * v),
            };
            *px = crate::raster::bytes(c);
        }
    }
    TEXEL_READS.fetch_add(texels as u64, Ordering::Relaxed);
}

/// Runs `rows(first row, band)` over `dst`'s rows. They are independent: big passes are split
/// into bands (a theme change recomputes every shadow at once, which stalled the frame for
/// ~90 ms on a slow core).
fn in_bands(dst: &mut Canvas, rows: impl Fn(usize, &mut [[u8; 4]]) + Sync) {
    let threads = if dst.w * dst.h >= 32 * 1024 {
        std::thread::available_parallelism()
            .map_or(1, |n| n.get())
            .min(8)
    } else {
        1
    };
    let per = dst.h.div_ceil(threads).max(1);
    let dw = dst.w.max(1);
    // The bands are taken from one queue by this thread and the ones it could start: where
    // the system refuses a thread (`EAGAIN`), the others shade its share.
    let bands = std::sync::Mutex::new(dst.px.chunks_mut(per * dw).enumerate());
    let shade_bands = || {
        loop {
            let next = bands.lock().unwrap_or_else(|e| e.into_inner()).next();
            let Some((k, band)) = next else { break };
            rows(k * per, band);
        }
    };
    std::thread::scope(|s| {
        for _ in 1..threads {
            if std::thread::Builder::new()
                .spawn_scoped(s, shade_bands)
                .is_err()
            {
                break;
            }
        }
        shade_bands();
    });
}

/// One blur pass of GaussianBlur.qml's shader: `dst` is the layer it renders into, each
/// pixel the weighted sum of `src` read linearly at the taps along `dir`.
#[allow(clippy::too_many_arguments)]
fn pass(
    dst: &mut Canvas,
    src: &Canvas,
    taps: &[(f32, f32)],
    spread: f32,
    dir: (f32, f32),
    rect: [f64; 4],
    sum: f32,
    opacity: f32,
    alpha_only: Option<([f32; 4], f32)>,
) {
    if rect == [0.0, 0.0, dst.w as f64, dst.h as f64]
        && let Some(reads) = taps_along(dst, src, taps, spread, dir)
    {
        return in_bands(dst, |first, band| {
            convolve_rows(band, first, src, taps, &reads, sum, opacity, alpha_only)
        });
    }
    sampled_pass(dst, src, taps, spread, dir, rect, sum, opacity, alpha_only);
}

/// `acc += wt * row`, per channel, a byte of `row` as the float the shader has of it: the
/// shader adding a tap it has read.
fn add_tap(acc: &mut [f32], wt: f32, row: &[[u8; 4]]) {
    for (a, &s) in acc.iter_mut().zip(row.as_flattened()) {
        *a += wt * unorm(s);
    }
}

/// One tap of the kernel at every column of a pass along a row.
struct TapRow {
    /// The weight of a column's tap, for each of the texel's four bytes.
    weights: Vec<u8>,
    /// `(column, columns, i0, i1, apart)`: columns whose taps read the texels `i0` and `i1`
    /// (`apart`: 0) or the texels one after another from them (1). A quad with a texel a
    /// pixel is one stretch of these, and one of those at each edge the taps are clamped to.
    stretches: Vec<(usize, usize, usize, usize, usize)>,
}

impl TapRow {
    fn new(taps: impl Iterator<Item = Tap>) -> TapRow {
        let mut row = TapRow {
            weights: Vec::new(),
            stretches: Vec::new(),
        };
        for (j, t) in taps.enumerate() {
            row.weights.extend([t.weight as u8; 4]);
            if let Some((_, n, i0, i1, apart)) = row.stretches.last_mut() {
                let step = if *n == 1 {
                    t.i0.wrapping_sub(*i0)
                } else {
                    *apart
                };
                if step <= 1 && (t.i0, t.i1) == (*i0 + *n * step, *i1 + *n * step) {
                    (*n, *apart) = (*n + 1, step);
                    continue;
                }
            }
            row.stretches.push((j, 1, t.i0, t.i1, 0));
        }
        row
    }

    /// The taps of the columns `part` read from the row `texels` into `out`.
    fn read(&self, out: &mut [[u8; 4]], texels: &[[u8; 4]], part: std::ops::Range<usize>) {
        for &(j, n, i0, i1, apart) in &self.stretches {
            let (from, to) = (j.max(part.start), (j + n).min(part.end));
            if from >= to {
                continue;
            }
            let (skip, n) = (from - j, to - from);
            let weights = &self.weights[4 * from..4 * to];
            let out = out[from..to].as_flattened_mut();
            if apart == 0 {
                for (out, &weight) in out
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut()
                    .zip(weights.as_chunks::<4>().0)
                {
                    let t = Tap {
                        i0,
                        i1,
                        weight: weight[0] as u16,
                    };
                    *out = std::array::from_fn(|ch| t.mix(texels[i0][ch], texels[i1][ch]));
                }
                continue;
            }
            let (i0, i1) = (i0 + skip, i1 + skip);
            for (((out, &weight), &a), &b) in (out.iter_mut())
                .zip(weights)
                .zip(texels[i0..i0 + n].as_flattened())
                .zip(texels[i1..i1 + n].as_flattened())
            {
                let t = Tap {
                    i0,
                    i1,
                    weight: weight as u16,
                };
                *out = t.mix(a, b);
            }
        }
    }
}

/// Where Qt draws an effect's last pass: its matrix (projection times the node's, column
/// major, f32) and the size of the target it is drawn into, in pixels.
#[derive(Clone, Copy, Debug)]
pub struct Placed {
    pub m: [f32; 16],
    pub target: (usize, usize),
}

/// The quad of a pass as the pieces llvmpipe fills, each with its own planes of the
/// coordinates.
enum Shape<'a> {
    /// A quad with its sides along the axes as [`Interpolated`]'s two triangles.
    Quad(Interpolated),
    /// Triangles in a target whose pixel `.1` is the pixel (0, 0) of the pass's.
    Tris(&'a [Tri], (i64, i64)),
}

struct Pieces<'a> {
    shape: Shape<'a>,
    /// The columns and rows of the pass the pieces are in: one past the last.
    x0: usize,
    x1: usize,
    rows: (usize, usize),
}

impl<'a> Pieces<'a> {
    fn tris(dst: &Canvas, tris: &'a [Tri], origin: (i64, i64)) -> Pieces<'a> {
        let lo = |i: usize, o: i64| tris.iter().map(|t| t.span[i].max(0) - o).min();
        let hi = |i: usize, o: i64| tris.iter().map(|t| t.span[i] - o).max();
        let px = |v: Option<i64>, n: usize| v.unwrap_or(0).clamp(0, n as i64) as usize;
        let (x0, y0) = (px(lo(0, origin.0), dst.w), px(lo(1, origin.1), dst.h));
        let (x1, y1) = (px(hi(2, origin.0), dst.w), px(hi(3, origin.1), dst.h));
        Pieces {
            shape: Shape::Tris(tris, origin),
            x0,
            x1: x1.max(x0),
            rows: (y0, y1.max(y0)),
        }
    }

    fn count(&self) -> usize {
        match &self.shape {
            Shape::Quad(_) => 2,
            Shape::Tris(t, _) => t.len(),
        }
    }

    /// (u, v) of the piece `k` at the centre of the pass's pixel (x, y).
    fn uv(&self, k: usize, x: usize, y: usize) -> (f32, f32) {
        match &self.shape {
            Shape::Quad(q) => (q.u(k == 1, x), q.v(k == 1, y)),
            Shape::Tris(t, o) => t[k].uv((x as i64 + o.0) as usize, (y as i64 + o.1) as usize),
        }
    }

    /// The first and the last of the pass's rows the piece `k` may be in, None when u is
    /// the same in every row of a column.
    fn turned(&self, k: usize) -> Option<(usize, usize)> {
        let Shape::Tris(t, o) = &self.shape else {
            return None;
        };
        let row = |v: i64| ((v - o.1).max(self.rows.0 as i64) as usize).min(self.rows.1 - 1);
        (!t[k].upright()).then(|| (row(t[k].span[1]), row(t[k].span[3] - 1)))
    }

    /// The pieces of the row `y` into `out`: a piece and its columns, counted from `x0`.
    fn spans(&self, y: usize, out: &mut Vec<(usize, std::ops::Range<usize>)>) {
        out.clear();
        let cols = self.x1 - self.x0;
        let (t, o) = match &self.shape {
            Shape::Quad(q) => {
                let split = q.split(y, self.x0, self.x1) - self.x0;
                out.extend([(0, 0..split), (1, split..cols)]);
                out.retain(|(_, part)| !part.is_empty());
                return;
            }
            Shape::Tris(t, o) => (t, o),
        };
        let wy = y as i64 + o.1;
        let mut run: Option<(usize, usize)> = None;
        for j in 0..cols {
            let wx = (self.x0 + j) as i64 + o.0;
            let k = match run {
                Some((k, _)) if t[k].covers(wx, wy) => Some(k),
                _ => t.iter().position(|t| t.covers(wx, wy)),
            };
            if k != run.map(|r| r.0) {
                out.extend(run.map(|(k, from)| (k, from..j)));
                run = k.map(|k| (k, j));
            }
        }
        out.extend(run.map(|(k, from)| (k, from..cols)));
    }
}

/// The columns `part` as runs of one tap each into `out`. `tap` of a column is a plane's
/// value rounded, so its taps go one way along the row: a run ends where bisection finds
/// the next tap.
fn runs(
    part: std::ops::Range<usize>,
    tap: impl Fn(usize) -> Tap,
    out: &mut Vec<(std::ops::Range<usize>, Tap)>,
) {
    out.clear();
    let same = |a: Tap, b: Tap| (a.i0, a.i1, a.weight) == (b.i0, b.i1, b.weight);
    let (mut from, last) = (part.start, part.end - 1);
    let mut t = tap(from);
    while !same(t, tap(last)) {
        let (mut lo, mut hi) = (from, last);
        while hi - lo > 1 {
            let mid = lo + (hi - lo) / 2;
            if same(tap(mid), t) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        out.push((from..hi, t));
        (from, t) = (hi, tap(hi));
    }
    out.push((from..part.end, t));
}

/// [`pass`] for any source and any place in `dst`, `dir` along one axis: every pixel is the
/// sum of its taps, each read through [`linear`]'s filter (a byte per channel, as llvmpipe
/// hands the shader). The texels and weights a tap has along x are the same in every row and
/// those along y the same in every column, so they are found once: a pass down first filters
/// every row of the source along x for the columns it draws, and a tap is then one lerp of
/// two of those rows; a pass along a row lerps each tap's two texels of that row, and of the
/// row below when the pixel is not on a row of texels.
///
/// [`linear`]: crate::layer::linear
#[allow(clippy::too_many_arguments)]
fn sampled_pass(
    dst: &mut Canvas,
    src: &Canvas,
    taps: &[(f32, f32)],
    spread: f32,
    dir: (f32, f32),
    rect: [f64; 4],
    sum: f32,
    opacity: f32,
    alpha_only: Option<([f32; 4], f32)>,
) {
    let [x, y, w, h] = rect;
    // The pixels of the quad are those llvmpipe fills: it holds a corner in 256ths of a
    // pixel, so an edge a float's error off a pixel centre (6.5 of `scale: 1.1` is
    // 6.4999992 here) is on it.
    let snap = |v: f64| (v * 256.0).round() / 256.0;
    let (left, right, top, bottom) = (snap(x), snap(x + w), snap(y), snap(y + h));
    let x0 = (left - 0.5).ceil().max(0.0) as usize;
    let x1 = ((right - 0.5).ceil().max(0.0) as usize).min(dst.w);
    // The rows by the rule of `dst`, which a band of it does not know: a window's differs.
    let rows = dst.rows(top, bottom - top);
    if x1 <= x0 || rows.0 >= rows.1 {
        return;
    }
    // The coordinates the shader gets: llvmpipe's, over the quad's two triangles (`cover_uv`).
    let full = [0.0, 0.0, 1.0, 1.0];
    let quad = Interpolated::new([x, y, x + w, y + h], full, [x0, rows.0, x1, rows.1]);
    let pieces = Pieces {
        shape: Shape::Quad(quad),
        x0,
        x1,
        rows,
    };
    pieces_pass(
        dst, src, taps, spread, dir, &pieces, sum, opacity, alpha_only,
    );
}

/// [`sampled_pass`] over the triangles `tris` as llvmpipe fills them, the pixel (0, 0) of
/// `dst` the pixel `origin` of the target they are in.
#[allow(clippy::too_many_arguments)]
fn tris_pass(
    dst: &mut Canvas,
    src: &Canvas,
    taps: &[(f32, f32)],
    spread: f32,
    dir: (f32, f32),
    tris: &[Tri],
    origin: (i64, i64),
    sum: f32,
    opacity: f32,
    alpha_only: Option<([f32; 4], f32)>,
) {
    let pieces = Pieces::tris(dst, tris, origin);
    if pieces.x1 > pieces.x0 && pieces.rows.1 > pieces.rows.0 {
        pieces_pass(
            dst, src, taps, spread, dir, &pieces, sum, opacity, alpha_only,
        );
    }
}

/// [`sampled_pass`] over `geo`. Where a piece's planes are not along the axes (a triangle
/// cut at the target's edge has corners of its own), a tap is still the same over a run of
/// a row's columns ([`runs`]); a column whose own tap is not the same in every row of the
/// piece reads every tap by itself.
#[allow(clippy::too_many_arguments)]
fn pieces_pass(
    dst: &mut Canvas,
    src: &Canvas,
    taps: &[(f32, f32)],
    spread: f32,
    dir: (f32, f32),
    geo: &Pieces,
    sum: f32,
    opacity: f32,
    alpha_only: Option<([f32; 4], f32)>,
) {
    let down = dir.0 == 0.0;
    let (x0, x1, rows) = (geo.x0, geo.x1, geo.rows);
    let cols = x1 - x0;
    let pieces = geo.count();
    let at =
        |c: f32, pos: f32, step: f32, n: usize| Tap::linear((c + spread * step * pos) as f64, n);
    let same = |a: &Tap, b: &Tap| (a.i0, a.i1, a.weight) == (b.i0, b.i1, b.weight);
    // u of each column by piece, in the piece's first row.
    let us: Vec<Vec<f32>> = (0..pieces)
        .map(|k| {
            let row = geo.turned(k).map_or(rows.0, |r| r.0);
            (x0..x1).map(|px| geo.uv(k, px, row).0).collect()
        })
        .collect();
    // The columns whose taps are others in the piece's last row.
    let offsets: Vec<f32> = match down {
        true => vec![0.0],
        false => taps.iter().map(|t| t.0).collect(),
    };
    let loose: Vec<Vec<bool>> = (0..pieces)
        .map(|k| {
            let Some((_, last)) = geo.turned(k) else {
                return Vec::new();
            };
            (x0..x1)
                .map(|px| {
                    let (near, far) = (us[k][px - x0], geo.uv(k, px, last).0);
                    (offsets.iter())
                        .any(|&p| !same(&at(near, p, dir.0, src.w), &at(far, p, dir.0, src.w)))
                })
                .collect()
        })
        .collect();
    // Along x, by piece: a tap per column for a pass down, else the columns' taps, a tap
    // of the kernel after another.
    let along = |k: usize, pos: f32| {
        let us = &us[k];
        (0..cols).map(move |j| at(us[j], pos, dir.0, src.w))
    };
    let xs: Vec<Vec<Tap>> = (0..pieces)
        .map(|k| match down {
            true => along(k, 0.0).collect(),
            false => Vec::new(),
        })
        .collect();
    let tap_rows: Vec<Vec<TapRow>> = (0..pieces)
        .map(|k| match down {
            true => Vec::new(),
            false => (taps.iter())
                .map(|&(pos, _)| TapRow::new(along(k, pos)))
                .collect(),
        })
        .collect();
    let line = |sy: usize| &src.px[sy * src.w..(sy + 1) * src.w];
    // The source filtered along x at the columns, for a pass down: the first piece's, and
    // another's when a column's tap is another there. A column reads the texels its taps
    // are of once for all the pieces.
    let mut across: Vec<Vec<[u8; 4]>> = Vec::new();
    let mut table = vec![0; pieces];
    if down {
        let filter = |t: &Tap, row: &[[u8; 4]]| match t.weight {
            0 => row[t.i0],
            _ => std::array::from_fn(|ch| t.mix(row[t.i0][ch], row[t.i1][ch])),
        };
        let mut first = Vec::with_capacity(cols * src.h);
        for sy in 0..src.h {
            let row = line(sy);
            first.extend(xs[0].iter().map(|t| filter(t, row)));
        }
        across.push(first);
        for k in 1..pieces {
            let other: Vec<usize> = (0..cols).filter(|&j| !same(&xs[0][j], &xs[k][j])).collect();
            if other.is_empty() {
                continue;
            }
            let mut own = across[0].clone();
            for sy in 0..src.h {
                let row = line(sy);
                for &j in &other {
                    own[sy * cols + j] = filter(&xs[k][j], row);
                }
            }
            table[k] = across.len();
            across.push(own);
        }
        let texels = |j: usize| {
            let mut read: Vec<usize> = (xs.iter())
                .flat_map(|xs| {
                    let t = xs[j];
                    [t.i0, if t.weight == 0 { t.i0 } else { t.i1 }]
                })
                .collect();
            read.sort_unstable();
            read.dedup();
            read.len()
        };
        let reads = (0..cols).map(texels).sum::<usize>() * src.h;
        TEXEL_READS.fetch_add(reads as u64, Ordering::Relaxed);
    }
    let finish = |c: [f32; 4]| {
        if let Some((color, thickness)) = alpha_only {
            let k = ((c[3] / sum) / thickness).clamp(0.0, 1.0);
            return color.map(|v| v * k * opacity);
        }
        c.map(|k| (opacity / sum) * k)
    };
    // A pixel of the piece `k` reading every tap by itself, as the shader does.
    let alone = |k: usize, px: usize, py: usize| {
        let (u, v) = geo.uv(k, px, py);
        let mut c = [0.0f32; 4];
        for &(pos, wt) in taps {
            let (su, sv) = (u + spread * dir.0 * pos, v + spread * dir.1 * pos);
            let s = linear(src, su as f64, sv as f64);
            for (c, s) in c.iter_mut().zip(s) {
                *c += wt * s;
            }
        }
        c
    };
    let dw = dst.w.max(1);
    in_bands(dst, |first, band| {
        let mut c = Canvas::from_px(dw, band.len() / dw, band.to_vec());
        let (y0, y1) = (
            rows.0.clamp(first, first + c.h),
            rows.1.clamp(first, first + c.h),
        );
        let mut sum = vec![0.0f32; 4 * cols];
        let mut mixed = vec![[0u8; 4]; cols];
        let (mut above, mut below) = (mixed.clone(), mixed.clone());
        let (mut spans, mut same_tap) = (Vec::new(), Vec::new());
        let mut reads = 0;
        let put = |c: &mut Canvas, px: usize, py: usize, sum: &[f32]| {
            for (px, sum) in (px..).zip(sum.as_chunks::<4>().0) {
                let out = finish(*sum);
                if out != [0.0; 4] {
                    c.blend_f(px, py, out);
                }
            }
        };
        for py in y0..y1 {
            geo.spans(py, &mut spans);
            sum.fill(0.0);
            for (k, part) in spans.iter().cloned() {
                let v = |j: usize| geo.uv(k, x0 + j, py).1;
                if down {
                    let across = &across[table[k]];
                    for &(pos, wt) in taps {
                        runs(part.clone(), |j| at(v(j), pos, dir.1, src.h), &mut same_tap);
                        for (part, t) in same_tap.iter().cloned() {
                            let n = part.len();
                            let sum = &mut sum[4 * part.start..4 * part.end];
                            let a = &across[t.i0 * cols..][part.clone()];
                            if t.weight == 0 {
                                add_tap(sum, wt, a);
                                reads += n;
                                continue;
                            }
                            let b = &across[t.i1 * cols..][part.clone()];
                            let mixed = &mut mixed[part.clone()];
                            t.mix_bytes(
                                mixed.as_flattened_mut(),
                                a.as_flattened(),
                                b.as_flattened(),
                            );
                            add_tap(sum, wt, mixed);
                            reads += 2 * n;
                        }
                    }
                } else {
                    runs(part.clone(), |j| at(v(j), 0.0, 0.0, src.h), &mut same_tap);
                    for (part, t) in same_tap.iter().cloned() {
                        let n = part.len();
                        let sum = &mut sum[4 * part.start..4 * part.end];
                        let both = t.weight != 0;
                        reads += if both { 4 } else { 2 } * n * taps.len();
                        for (row, &(_, wt)) in tap_rows[k].iter().zip(taps) {
                            if both {
                                row.read(&mut above, line(t.i0), part.clone());
                                row.read(&mut below, line(t.i1), part.clone());
                                t.mix_bytes(
                                    mixed[part.clone()].as_flattened_mut(),
                                    above[part.clone()].as_flattened(),
                                    below[part.clone()].as_flattened(),
                                );
                            } else {
                                row.read(&mut mixed, line(t.i0), part.clone());
                            }
                            add_tap(sum, wt, &mixed[part.clone()]);
                        }
                    }
                }
                for j in part.clone().filter(|&j| loose[k].get(j) == Some(&true)) {
                    sum[4 * j..4 * j + 4].copy_from_slice(&alone(k, x0 + j, py));
                    reads += 4 * taps.len();
                }
            }
            put(&mut c, x0, py - first, &sum);
        }
        TEXEL_READS.fetch_add(reads as u64, Ordering::Relaxed);
        band.copy_from_slice(&c.px);
    });
}

/// GaussianBlur { radius; samples; deviation; transparentBorder } over a source texture of the
/// item size (w x h), drawn into `cv` at (x, y). Uses the fallback shader like Qt when needed.
/// `deviation` as in [`kernel`].
#[allow(clippy::too_many_arguments)]
pub fn gaussian_blur(
    cv: &mut Canvas,
    src: &Canvas,
    radius: f64,
    samples: i32,
    deviation: Option<f64>,
    transparent_border: bool,
    x: f64,
    y: f64,
    opacity: f32,
) {
    blur_impl(
        cv,
        src,
        (src.w as f64, src.h as f64),
        radius,
        samples,
        deviation,
        transparent_border,
        x,
        y,
        opacity,
        [1.0, 1.0],
        None,
    );
}

/// GaussianBlur { radius; samples; deviation; transparentBorder } of an effect of `size` (its
/// width and height, whole or not), fed its SourceProxy texture `proxy`: without
/// transparentBorder the source texture of any size, which the horizontal pass samples linearly
/// over the effect; with it either the effect-sized source or the proxy itself (sourceRect
/// (-r, 0, w + 2r, h): the source 1:1, cropped). `scale` is that of the texture `cv` when it is
/// a layer's that is not its scene rect's size: the last pass's quad is drawn through it.
#[allow(clippy::too_many_arguments)]
pub fn gaussian_blur_proxy(
    cv: &mut Canvas,
    proxy: &Canvas,
    size: (f64, f64),
    radius: f64,
    samples: i32,
    deviation: Option<f64>,
    transparent_border: bool,
    x: f64,
    y: f64,
    opacity: f32,
    scale: [f64; 2],
    placed: Option<Placed>,
) {
    blur_impl(
        cv,
        proxy,
        size,
        radius,
        samples,
        deviation,
        transparent_border,
        x,
        y,
        opacity,
        scale,
        placed,
    );
}

/// GaussianBlur fed directly with a texture (QGfxSourceProxy "direct" path: Image with
/// fillMode Stretch, no transparentBorder) — the effect is item-sized but samples `tex`.
#[allow(clippy::too_many_arguments)]
pub fn gaussian_blur_direct(
    cv: &mut Canvas,
    tex: &Canvas,
    item_w: usize,
    item_h: usize,
    radius: f64,
    samples: i32,
    x: f64,
    y: f64,
    opacity: f32,
) {
    blur_impl(
        cv,
        tex,
        (item_w as f64, item_h as f64),
        radius,
        samples,
        None,
        false,
        x,
        y,
        opacity,
        [1.0, 1.0],
        None,
    );
}

/// Qt5Compat DropShadow (DropShadowBase.qml: alpha-only GaussianBlur at rounded offset, _thickness = spread)
/// of an effect of `size`; `src` is its SourceProxy texture and `scale` the texture's it is
/// drawn into, as in `gaussian_blur_proxy`.
#[allow(clippy::too_many_arguments)]
pub fn drop_shadow(
    cv: &mut Canvas,
    src: &Canvas,
    size: (f64, f64),
    radius: f64,
    samples: i32,
    color: [u16; 4],
    spread: f32,
    offset: (f64, f64),
    transparent_border: bool,
    x: f64,
    y: f64,
    opacity: f32,
    scale: [f64; 2],
) {
    let pc = crate::shadow::shader_color(color);
    let thickness = shadow_thickness(spread);
    let alpha: Vec<u8> = src.px.iter().map(|p| p[3]).collect();
    let r = if transparent_border {
        border(radius)
    } else {
        0.0
    };
    let Some((at, quad)) = last_quad(x + offset.0.round(), y + offset.1.round(), r, size, scale)
    else {
        return;
    };
    let window = cv.is_window();
    let key = ShadowKey {
        window,
        size: (size.0.to_bits(), size.1.to_bits()),
        src: (src.w, src.h),
        radius: radius.to_bits(),
        quad: quad.map(f64::to_bits),
        samples,
        color,
        spread: spread.to_bits(),
        transparent_border,
        opacity: opacity.to_bits(),
    };
    let out = cached_shadow(key, alpha, || {
        blur_tex(
            src,
            size,
            radius,
            samples,
            None,
            transparent_border,
            quad,
            window,
            opacity,
            Some((pc, thickness)),
            None,
        )
    });
    crate::layer::draw(cv, &out, at.0, at.1, out.w as f64, out.h as f64, 1.0);
    // DropShadowBase.qml draws the SourceProxy output on top of the shadow over `_outputRect`:
    // the cropped proxy at -radius with transparentBorder (a smooth texture: between pixels
    // when the radius is not whole), else the source stretched over the effect.
    let (w, h) = crate::layer::texture_size(size.0, size.1);
    let [sx, sy] = scale;
    let whole = scale == [1.0, 1.0] && (size.0, size.1) == (w as f64, h as f64);
    if r > 0.0 && (src.w, src.h) == (padded(size.0, r), h) {
        let (pw, ph) = (src.w as f64, src.h as f64);
        if whole && (x - r).fract() == 0.0 && y.fract() == 0.0 {
            crate::layer::draw(cv, src, x - r, y, pw, ph, opacity);
        } else {
            let (qw, qh) = (sx * pw, sy * size.1);
            crate::layer::draw_linear(cv, src, sx * (x - r), sy * y, qw, qh, opacity);
        }
    } else if whole && (src.w, src.h) == (w, h) {
        crate::layer::draw(cv, src, x, y, w as f64, h as f64, opacity);
    } else {
        let (qw, qh) = (sx * size.0, sy * size.1);
        crate::layer::draw_linear(cv, src, sx * x, sy * y, qw, qh, opacity);
    }
}

/// DropShadowBase.qml `_thickness` of a DropShadow `spread`.
pub fn shadow_thickness(spread: f32) -> f32 {
    (1.0 - spread * 0.98).clamp(0.0, 0.98)
}

/// DropShadow parameters besides the source alpha: its output depends on nothing else.
#[derive(Clone, Copy, PartialEq, Eq)]
struct ShadowKey {
    /// Whether it is drawn into a window: its rows are a window's then (`Canvas::rows`).
    window: bool,
    /// Bits of the effect's size.
    size: (u64, u64),
    src: (usize, usize),
    radius: u64,
    /// Bits of the last pass's quad in the texture the blur is drawn from (`last_quad`).
    quad: [u64; 4],
    samples: i32,
    color: [u16; 4],
    spread: u32,
    transparent_border: bool,
    opacity: u32,
}

struct ShadowEntry {
    key: ShadowKey,
    alpha: Vec<u8>,
    out: std::rc::Rc<Canvas>,
}

/// Recent shadows, newest first, at most this many bytes of output and alpha.
const SHADOW_CACHE_BYTES: usize = 8 << 20;

thread_local! {
    static SHADOWS: std::cell::RefCell<Vec<ShadowEntry>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// A DropShadow reads only its source's alpha: when a theme recolours the source (every frame
/// of the colour animation) the blurred shadow is reused instead of blurred again.
fn cached_shadow(
    key: ShadowKey,
    alpha: Vec<u8>,
    blur: impl FnOnce() -> Canvas,
) -> std::rc::Rc<Canvas> {
    SHADOWS.with_borrow_mut(|c| {
        if let Some(i) = c.iter().position(|e| e.key == key && e.alpha == alpha) {
            let e = c.remove(i);
            let hit = e.out.clone();
            c.insert(0, e);
            return hit;
        }
        let out = blur();
        let out = std::rc::Rc::new(out);
        c.insert(
            0,
            ShadowEntry {
                key,
                alpha,
                out: out.clone(),
            },
        );
        let mut bytes = 0;
        c.retain(|e| {
            bytes += e.out.px.len() * 4 + e.alpha.len();
            bytes <= SHADOW_CACHE_BYTES || std::rc::Rc::ptr_eq(&e.out, &out)
        });
        out
    })
}

/// GaussianBlur.qml's default `deviation` for `radius`.
pub fn default_deviation(radius: f64) -> f64 {
    (radius + 1.0) / 3.3333
}

/// The taps (offset in texels before `spread`, weight) of one GaussianBlur pass, the `spread`
/// scaling their offsets and what the shader divides the taps' sum by (the fallback shader's
/// `wSum`; 1 for the other, whose weights are divided as they are baked): the shader
/// qgfxshaderbuilder generates for these properties. `deviation` is the item's property
/// (`None`: its default binding, [`default_deviation`]).
pub fn kernel(radius: f64, samples: i32, deviation: Option<f64>) -> (Vec<(f32, f32)>, f32, f32) {
    let radius = border(radius);
    // `property int _kernelRadius: Math.max(0, samples / 2)`, truncated.
    let kr = ((samples as f64 / 2.0).max(0.0) as i32).min(MAX_BLUR_RADIUS);
    // GaussianBlur.qml: Math.max(0.00001, deviation), which is NaN for a NaN deviation; f64::max
    // would drop the NaN, and a NaN kernel draws nothing in Qt and here.
    let deviation = deviation.unwrap_or_else(|| default_deviation(radius));
    let deviation = if deviation.is_nan() {
        deviation
    } else {
        deviation.max(0.00001)
    };
    // qgfxshaderbuilder: fallback shader when samples exceed the GL varying budget
    // (GL_MAX_VARYING_COMPONENTS / 2 = 64 on Mesa) or radius != kernelRadius, which every
    // radius that is not a whole number is.
    let fallback = kr + 1 > MAX_BLUR_SAMPLES || radius != kr as f64;
    let (t, sum) = if fallback {
        fallback_taps(kr, deviation)
    } else {
        (taps(kr, deviation), 1.0)
    };
    // No kernel (samples < 2): the single centre tap, not radius / 0.
    let spread = if kr == 0 {
        0.0
    } else {
        (radius / kr as f64) as f32
    };
    (t, spread, sum)
}

/// GaussianBlur.qml's vertical pass of an effect of `size` at (x, y): a quad of
/// `_paddedTexWidth` x `_paddedTexHeight` at (-r, -r), drawn through `scale`. The whole pixel
/// its corner lies in, and the quad from that pixel; None when it has no pixel.
fn last_quad(
    x: f64,
    y: f64,
    r: f64,
    size: (f64, f64),
    scale: [f64; 2],
) -> Option<((f64, f64), [f64; 4])> {
    let (pw, ph) = (padded(size.0, r), padded(size.1, r));
    if pw == 0 || ph == 0 {
        return None;
    }
    // `_paddedTexWidth` is the width itself without transparentBorder, but an int.
    let (qw, qh) = if r > 0.0 {
        (pw as f64, ph as f64)
    } else {
        size
    };
    let (cx, cy) = (scale[0] * (x - r), scale[1] * (y - r));
    let at = (cx.floor(), cy.floor());
    let quad = [cx - at.0, cy - at.1, scale[0] * qw, scale[1] * qh];
    Some((at, quad))
}

#[allow(clippy::too_many_arguments)]
fn blur_impl(
    cv: &mut Canvas,
    src: &Canvas,
    size: (f64, f64),
    radius: f64,
    samples: i32,
    deviation: Option<f64>,
    transparent_border: bool,
    x: f64,
    y: f64,
    opacity: f32,
    scale: [f64; 2],
    placed: Option<Placed>,
) {
    let r = if transparent_border {
        border(radius)
    } else {
        0.0
    };
    let Some((at, quad)) = last_quad(x, y, r, size, scale) else {
        return;
    };
    // The pixel of Qt's target the pixel (0, 0) of the pass's texture is: `cv` is that
    // target or a part of it a whole number of pixels from its corner.
    let placed = placed.and_then(|p| {
        let (tw, th) = (p.target.0 as f32, p.target.1 as f32);
        let corner = [
            (p.m[12] / p.m[15] + 1.0) * 0.5 * tw,
            (1.0 - p.m[13] / p.m[15]) * 0.5 * th,
        ];
        let off = [
            corner[0] as f64 - (at.0 + quad[0]),
            corner[1] as f64 - (at.1 + quad[1]),
        ];
        let whole = off.iter().all(|o| (o - o.round()).abs() < 0.01);
        let origin = (
            (at.0 + off[0].round()) as i64,
            (at.1 + off[1].round()) as i64,
        );
        whole.then_some((p, origin))
    });
    let out = blur_tex(
        src,
        size,
        radius,
        samples,
        deviation,
        transparent_border,
        quad,
        cv.is_window(),
        opacity,
        None,
        placed,
    );
    crate::layer::draw(cv, &out, at.0, at.1, out.w as f64, out.h as f64, 1.0);
}

/// The pixels of the blur of an effect of `size` (wd x ht) from the whole pixel its last
/// pass's quad `quad` (x y w h from that pixel, `last_quad`) lies in. As GaussianBlur.qml: the
/// horizontal pass is an item of the effect's size in its own layer, a texture of that size
/// rounded; with transparentBorder it is `_paddedTexWidth` wide in a layer of sourceRect
/// (0, -r, width, `_paddedTexHeight`). A tap is one of `_paddedTexWidth` (an int) steps. The
/// vertical pass samples that layer linearly over its quad.
#[allow(clippy::too_many_arguments)]
fn blur_tex(
    src: &Canvas,
    (wd, ht): (f64, f64),
    radius: f64,
    samples: i32,
    deviation: Option<f64>,
    transparent_border: bool,
    quad: [f64; 4],
    window: bool,
    opacity: f32,
    alpha: Option<([f32; 4], f32)>,
    placed: Option<(Placed, (i64, i64))>,
) -> Canvas {
    let (t, spread, sum) = kernel(radius, samples, deviation);
    let r = if transparent_border {
        border(radius)
    } else {
        0.0
    };
    let (w, h) = crate::layer::texture_size(wd, ht);
    let (pw, ph) = (padded(wd, r), padded(ht, r));
    // SourceProxy: sourceRect (-r, 0, pw, ht); `src` may already be that proxy.
    let padded_proxy;
    let proxy = if r == 0.0 || (src.w, src.h) == (pw, h) {
        src
    } else {
        assert_eq!((src.w, src.h), (w, h));
        let mut p = Canvas::new(pw, h, [0, 0, 0, 0]);
        if r.fract() == 0.0 && wd == w as f64 {
            let r = r as usize;
            for yy in 0..h {
                for xx in 0..w {
                    p.px[yy * pw + xx + r] = src.px[yy * w + xx];
                }
            }
        } else {
            crate::layer::draw_linear(&mut p, src, r, 0.0, wd, h as f64, 1.0);
        }
        padded_proxy = p;
        &padded_proxy
    };
    // horizontalBlur in its layer.
    let (hw, hh) = if r > 0.0 { (pw, ph) } else { (w, h) };
    let mut hb = Canvas::new(hw, hh, [0, 0, 0, 0]);
    let dir = (1.0 / pw as f32, 0.0);
    let mask = alpha.map(|(_, th)| ([1.0; 4], th));
    // The item's own rect through QSGRhiLayer's projection of the layer's rect, both in
    // the item's units, into the layer's texels: with transparentBorder `layer.sourceRect`
    // (0, -radius, width, `_paddedTexHeight`), else the item's rect.
    let (lw, lh) = (hw as f32, hh as f32);
    let (item, m) = if r > 0.0 {
        let (top, tall) = (-r as f32, ph as f32);
        let item = [0.0, 0.0, pw as f32, ht as f32];
        (item, crate::tri::ortho(0.0, pw as f32, top, top + tall))
    } else {
        let item = [0.0, 0.0, wd as f32, ht as f32];
        (item, crate::tri::ortho(0.0, wd as f32, 0.0, ht as f32))
    };
    let view = crate::tri::View {
        scale: [0.5 * lw, 0.5 * lh],
        trans: [0.5 * lw, 0.5 * lh],
        bottom_rule: false,
    };
    let tris = crate::tri::setup(&crate::tri::grid(&m, item), &view);
    tris_pass(
        &mut hb,
        proxy,
        &t,
        spread,
        dir,
        &tris,
        (0, 0),
        sum,
        1.0,
        mask,
    );
    let (ow, oh) = ((quad[0] + quad[2]).ceil(), (quad[1] + quad[3]).ceil());
    let mut out = Canvas::new(ow.max(1.0) as usize, oh.max(1.0) as usize, [0, 0, 0, 0]);
    let dir = (0.0, 1.0 / ph as f32);
    // Drawn into a window, the quad has the rows a window gives it.
    let _rows = window.then(|| crate::raster::Window::new(&out));
    if let Some((p, origin)) = placed {
        // The item's own rect through Qt's matrix into the target, whose rows go down.
        let (tw, th) = (p.target.0 as f32, p.target.1 as f32);
        let view = crate::tri::View {
            scale: [0.5 * tw, -(0.5 * th)],
            trans: [0.5 * tw, 0.5 * th],
            bottom_rule: true,
        };
        let (qw, qh) = if r > 0.0 {
            (pw as f32, ph as f32)
        } else {
            (wd as f32, ht as f32)
        };
        let tris = crate::tri::setup(&crate::tri::grid(&p.m, [0.0, 0.0, qw, qh]), &view);
        tris_pass(
            &mut out, &hb, &t, spread, dir, &tris, origin, sum, opacity, alpha,
        );
        return out;
    }
    pass(&mut out, &hb, &t, spread, dir, quad, sum, opacity, alpha);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::{cover, linear};

    /// [`pass`] as the shader runs it: every pixel reads every tap by itself.
    #[allow(clippy::too_many_arguments)]
    fn shaded_pass(
        dst: &mut Canvas,
        src: &Canvas,
        taps: &[(f32, f32)],
        spread: f32,
        dir: (f32, f32),
        [x, y, w, h]: [f64; 4],
        sum: f32,
        opacity: f32,
        alpha_only: Option<([f32; 4], f32)>,
    ) {
        let shade = |u: f64, v: f64| {
            let mut c = [0.0f32; 4];
            for &(pos, wt) in taps {
                let su = u as f32 + spread * dir.0 * pos;
                let sv = v as f32 + spread * dir.1 * pos;
                let s = linear(src, su as f64, sv as f64);
                for k in 0..4 {
                    c[k] += wt * s[k];
                }
            }
            if let Some((color, thickness)) = alpha_only {
                let k = ((c[3] / sum) / thickness).clamp(0.0, 1.0);
                return color.map(|v| v * k * opacity);
            }
            c.map(|k| (opacity / sum) * k)
        };
        let dw = dst.w.max(1);
        in_bands(dst, |first, band| {
            let mut c = Canvas::from_px(dw, band.len() / dw, band.to_vec());
            cover(&mut c, x, y - first as f64, w, h, &shade);
            band.copy_from_slice(&c.px);
        });
    }

    /// The four channels mixed at once are each `Tap::mix`'s, for every weight and bytes.
    #[test]
    fn a_sampled_pass_is_the_shader_reading_every_tap_of_every_pixel() {
        let mut src = Canvas::new(83, 61, [0, 0, 0, 0]);
        let mut seed = 0x2545_f491u32;
        for p in src.px.iter_mut() {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let a = (seed >> 24) as u8;
            *p = [(seed >> 8) as u8, (seed >> 16) as u8, seed as u8, 255].map(|c| c.min(a));
            p[3] = a;
        }
        for (radius, samples) in [(8.0, 17), (40.0, 81), (10.0, 9), (2.6, 9)] {
            let (taps, spread, sum) = kernel(radius, samples, None);
            for dir in [(1.0 / 82.0, 0.0), (0.0, 1.0 / 60.0), (1.0 / 83.0, 0.0)] {
                for alpha in [None, Some(([0.2, 0.4, 0.6, 0.8], 0.5))] {
                    for rect in [
                        [0.0, 0.0, 83.0, 61.0],
                        [0.24, 0.57, 91.3, 67.1],
                        [3.5, 2.5, 40.2, 70.0],
                        [-6.3, -4.5, 120.0, 30.4],
                    ] {
                        let mut ours = Canvas::new(96, 70, [0, 0, 0, 0]);
                        sampled_pass(&mut ours, &src, &taps, spread, dir, rect, sum, 0.75, alpha);
                        let mut shader = Canvas::new(96, 70, [0, 0, 0, 0]);
                        shaded_pass(
                            &mut shader,
                            &src,
                            &taps,
                            spread,
                            dir,
                            rect,
                            sum,
                            0.75,
                            alpha,
                        );
                        let other = (ours.px.iter().zip(&shader.px))
                            .filter(|(a, b)| a != b)
                            .count();
                        assert_eq!(other, 0, "radius {radius} dir {dir:?} rect {rect:?}");
                        assert!(ours.px.iter().any(|p| *p != [0, 0, 0, 0]));
                    }
                }
            }
        }
    }

    /// GaussianBlur.qml: `radius` is a real. One that is not whole keeps its border and
    /// spread and takes the fallback shader's taps; `_paddedTexWidth` is an int, truncated.
    #[test]
    fn fractional_radius_is_kept() {
        assert_eq!(border(2.6), 2.6);
        assert_eq!((padded(40.0, 2.6), padded(40.0, 2.5)), (45, 45));
        let (taps, spread, sum) = kernel(2.6, 9, None);
        assert_eq!(spread, (2.6f64 / 4.0) as f32);
        assert_eq!((taps, sum), fallback_taps(4, 3.6 / 3.3333));
        let (whole, spread, sum) = kernel(4.0, 9, None);
        assert_eq!((spread, whole.len(), sum), (1.0, 5, 1.0));
    }

    /// The system refuses the threads a big pass splits its rows over (the limit on threads
    /// is reached): the pass shades every band on the calling thread, instead of panicking,
    /// to the same pixels as with the threads.
    #[test]
    fn a_pass_shades_every_band_when_threads_are_refused() {
        let mut src = Canvas::new(256, 160, [0, 0, 0, 0]);
        for (i, p) in src.px.iter_mut().enumerate() {
            *p = [(i % 251) as u8, (i % 241) as u8, (i % 239) as u8, 255];
        }
        let blur = |dst: &mut Canvas| {
            let taps = [(-1.0, 0.25), (0.0, 0.5), (1.0, 0.25)];
            let dir = (1.0 / src.w as f32, 0.0);
            let rect = [0.0, 0.0, dst.w as f64, dst.h as f64];
            pass(dst, &src, &taps, 1.0, dir, rect, 1.0, 1.0, None);
        };
        let mut threaded = Canvas::new(src.w, src.h, [0, 0, 0, 0]);
        blur(&mut threaded);
        testenv::refuse_new_threads();
        let mut refused = Canvas::new(src.w, src.h, [0, 0, 0, 0]);
        blur(&mut refused);
        assert!(refused.px == threaded.px);
        assert!(refused.px.iter().any(|p| *p != [0, 0, 0, 0]));
    }

    /// A pass over a source of its own size is computed a row at a time; it is the pass the
    /// shader runs, where every pixel reads its taps linearly: the same pixels, for whole and
    /// fractional tap offsets, along both axes, at the clamped edges and for a shadow's alpha.
    #[test]
    fn a_pass_over_a_source_of_its_size_is_the_sampled_pass() {
        let mut src = Canvas::new(96, 70, [0, 0, 0, 0]);
        let mut seed = 0x2545_f491u32;
        for p in src.px.iter_mut() {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let a = (seed >> 24) as u8;
            *p = [(seed >> 8) as u8, (seed >> 16) as u8, seed as u8, 255].map(|c| c.min(a));
            p[3] = a;
        }
        // The optimized shader's paired taps, the fallback's one per texel, and a radius
        // unlike the kernel's (taps 2.5 texels apart).
        for (radius, samples) in [(8.0, 17), (40.0, 81), (10.0, 9)] {
            let (taps, spread, sum) = kernel(radius, samples, None);
            for dir in [(1.0 / src.w as f32, 0.0), (0.0, 1.0 / src.h as f32)] {
                for alpha in [None, Some(([0.2, 0.4, 0.6, 0.8], 0.5))] {
                    let mut conv = Canvas::new(src.w, src.h, [0, 0, 0, 0]);
                    assert!(taps_along(&conv, &src, &taps, spread, dir).is_some());
                    let rect = [0.0, 0.0, src.w as f64, src.h as f64];
                    pass(&mut conv, &src, &taps, spread, dir, rect, sum, 0.75, alpha);
                    let mut sampled = Canvas::new(src.w, src.h, [0, 0, 0, 0]);
                    sampled_pass(
                        &mut sampled,
                        &src,
                        &taps,
                        spread,
                        dir,
                        rect,
                        sum,
                        0.75,
                        alpha,
                    );
                    assert!(
                        conv.px == sampled.px,
                        "radius {radius} samples {samples} dir {dir:?}"
                    );
                    assert!(conv.px.iter().any(|p| *p != [0, 0, 0, 0]));
                }
            }
        }
        // A source of another size has no such kernel: each pixel samples it.
        let half = Canvas::new(48, 70, [0, 0, 0, 0]);
        let (taps, spread, _) = kernel(8.0, 17, None);
        assert!(taps_along(&half, &src, &taps, spread, (1.0 / 48.0, 0.0)).is_none());
    }

    fn disc(rgba: [u8; 4]) -> Canvas {
        let mut c = Canvas::new(24, 20, [0, 0, 0, 0]);
        for y in 0..c.h {
            for x in 0..c.w {
                let d = ((x as f64 - 11.5).powi(2) + (y as f64 - 9.5).powi(2)).sqrt();
                let a = (8.0 - d).clamp(0.0, 1.0);
                let k = |v: u8| (v as f64 * a).round() as u8;
                c.px[y * c.w + x] = [k(rgba[0]), k(rgba[1]), k(rgba[2]), k(rgba[3])];
            }
        }
        c
    }

    fn shadow(src: &Canvas, color: [u16; 4]) -> Canvas {
        let mut cv = Canvas::new(48, 44, [10, 20, 30, 255]);
        drop_shadow(
            &mut cv,
            src,
            (src.w as f64, src.h as f64),
            6.0,
            13,
            color,
            0.2,
            (1.4, 2.0),
            true,
            12.0,
            11.0,
            0.9,
            [1.0, 1.0],
        );
        cv
    }

    fn uncached(src: &Canvas, color: [u16; 4]) -> Canvas {
        SHADOWS.with_borrow_mut(|c| c.clear());
        shadow(src, color)
    }

    /// A recoloured source (same alpha) reuses the blurred shadow and draws the same pixels as a
    /// fresh blur; a new alpha or shadow colour blurs again.
    #[test]
    fn drop_shadow_cache_matches_fresh_blur() {
        let (red, blue) = (disc([200, 30, 40, 255]), disc([20, 60, 210, 255]));
        let black = [0, 0, 0, 120 * 257];
        let fresh_blue = uncached(&blue, black);
        uncached(&red, black);
        assert!(shadow(&blue, black).px == fresh_blue.px);
        assert_eq!(SHADOWS.with_borrow(|c| c.len()), 1);
        let grey = [90, 90, 90, 200].map(|k| k * 257);
        assert!(shadow(&blue, grey).px == uncached(&blue, grey).px);
        assert!(shadow(&blue, grey).px != fresh_blue.px);
        let mut half = disc([20, 60, 210, 255]);
        half.px.iter_mut().step_by(3).for_each(|p| *p = [0; 4]);
        let fresh_half = uncached(&half, black);
        shadow(&blue, black);
        assert!(shadow(&half, black).px == fresh_half.px);
        assert!(fresh_half.px != fresh_blue.px);
    }

    #[test]
    fn drop_shadow_cache_is_bounded() {
        SHADOWS.with_borrow_mut(|c| c.clear());
        let mut big = Canvas::new(1200, 1000, [0, 0, 0, 255]);
        for i in 0..4u8 {
            big.px[0] = [0, 0, 0, i];
            let mut cv = Canvas::new(8, 8, [0; 4]);
            drop_shadow(
                &mut cv,
                &big,
                (big.w as f64, big.h as f64),
                1.0,
                3,
                [0, 0, 0, 65535],
                0.0,
                (0.0, 0.0),
                false,
                0.0,
                0.0,
                1.0,
                [1.0, 1.0],
            );
        }
        let bytes: usize =
            SHADOWS.with_borrow(|c| c.iter().map(|e| e.out.px.len() * 4 + e.alpha.len()).sum());
        assert!(bytes <= SHADOW_CACHE_BYTES, "{bytes}");
        assert!(SHADOWS.with_borrow(|c| c.len()) >= 1);
    }
}
