//! `cv::resize(src, dst, dsize, 0, 0, INTER_LANCZOS4)` for 8-bit images, ported from OpenCV
//! `modules/imgproc/src/resize.cpp` (generic path: `interpolateLanczos4`, `HResizeLanczos4`,
//! `VResizeLanczos4`, `FixedPtCast<int, uchar, 22>`). IPP gives identical output for this case
//! and the HAL has no x86 override, so the generic code is what `cv2.resize` runs.

use rayon::prelude::*;

const COEF_SCALE: f32 = 2048.0; // INTER_RESIZE_COEF_SCALE
const CAST_SHIFT: i32 = 22; // INTER_RESIZE_COEF_BITS * 2

fn lanczos4(x: f32) -> [f32; 8] {
    const S45: f64 = std::f64::consts::FRAC_1_SQRT_2;
    const CS: [[f64; 2]; 8] = [
        [1.0, 0.0],
        [-S45, -S45],
        [0.0, 1.0],
        [S45, -S45],
        [-1.0, 0.0],
        [S45, S45],
        [0.0, -1.0],
        [-S45, S45],
    ];
    let mut c = [0f32; 8];
    let mut sum = 0f32;
    let y0 = -((x + 3.0) as f64) * std::f64::consts::PI * 0.25;
    let (s0, c0) = (y0.sin(), y0.cos());
    for (i, ci) in c.iter_mut().enumerate() {
        let y0_ = x + 3.0 - i as f32;
        *ci = if y0_.abs() >= 1e-6 {
            let y = -(y0_ as f64) * std::f64::consts::PI * 0.25;
            ((CS[i][0] * s0 + CS[i][1] * c0) / (y * y)) as f32
        } else {
            1e30
        };
        sum += *ci;
    }
    let inv = 1.0 / sum;
    for ci in c.iter_mut() {
        *ci *= inv;
    }
    c
}

/// `saturate_cast<short>(v * INTER_RESIZE_COEF_SCALE)` (cvRound = round half to even).
fn fixed(c: [f32; 8]) -> [i32; 8] {
    c.map(|v| ((v * COEF_SCALE).round_ties_even() as i32).clamp(i16::MIN as i32, i16::MAX as i32))
}

/// `cvFloor` of the source coordinate and its fractional part, as resize.cpp computes them.
fn src_pos(d: usize, scale: f64) -> (i32, f32) {
    let f = ((d as f64 + 0.5) * scale - 0.5) as f32;
    let s = f.floor() as i32;
    (s, f - s as f32)
}

/// Destination rectangle a resize produces: columns `x0..x1`, rows `y0..y1` of the full
/// `dw`x`dh` result. Pixels outside it are never computed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window {
    pub x0: usize,
    pub x1: usize,
    pub y0: usize,
    pub y1: usize,
}

impl Window {
    pub fn full(w: usize, h: usize) -> Self {
        Window {
            x0: 0,
            x1: w,
            y0: 0,
            y1: h,
        }
    }
}

/// Resizes an interleaved `cn`-channel 8-bit image of `sw`x`sh` to `dw`x`dh`.
pub fn resize_lanczos4(
    src: &[u8],
    sw: usize,
    sh: usize,
    cn: usize,
    dw: usize,
    dh: usize,
) -> Vec<u8> {
    assert!(src.len() == sw * sh * cn);
    let line = sw * cn;
    let mut y = 0;
    resize_lanczos4_rows(sw, sh, cn, dw, dh, Window::full(dw, dh), &mut |row| {
        row.copy_from_slice(&src[y * line..(y + 1) * line]);
        y += 1;
        true
    })
    .unwrap()
}

/// Destination rows resized per batch; bounds the horizontal-pass cache to about
/// `BATCH * scale + 8` rows instead of one per source row.
const BATCH: usize = 32;

/// [`resize_lanczos4`] of only `win`, pulling source rows top to bottom from `next_row` (which
/// fills one `sw*cn` row and returns false on a decode error, giving `None`). Rows after the
/// last one `win` depends on are never requested, so the source never has to exist as a whole:
/// at most one batch of horizontally resized rows and the output are held. Every pixel is
/// computed exactly as OpenCV computes it.
pub fn resize_lanczos4_rows(
    sw: usize,
    sh: usize,
    cn: usize,
    dw: usize,
    dh: usize,
    win: Window,
    next_row: &mut (dyn FnMut(&mut [u8]) -> bool + Send),
) -> Option<Vec<u8>> {
    assert!(sw > 0 && sh > 0 && dw > 0 && dh > 0);
    assert!(win.x0 <= win.x1 && win.x1 <= dw && win.y0 <= win.y1 && win.y1 <= dh);
    let line = sw * cn;
    let (ow, oh) = (win.x1 - win.x0, win.y1 - win.y0);
    let wc = ow * cn;
    let mut dst = vec![0u8; oh * wc];
    if oh == 0 || ow == 0 {
        return Some(dst);
    }
    if (sw, sh) == (dw, dh) {
        let mut row = vec![0u8; line];
        for y in 0..win.y1 {
            if !next_row(&mut row) {
                return None;
            }
            if y >= win.y0 {
                let o = (y - win.y0) * wc;
                dst[o..o + wc].copy_from_slice(&row[win.x0 * cn..win.x1 * cn]);
            }
        }
        return Some(dst);
    }
    let scale_x = 1.0 / (dw as f64 / sw as f64);
    let scale_y = 1.0 / (dh as f64 / sh as f64);

    let cols: Vec<(i32, [i32; 8])> = (win.x0..win.x1)
        .map(|dx| {
            let (sx, fx) = src_pos(dx, scale_x);
            (sx, fixed(lanczos4(fx)))
        })
        .collect();
    let rows_y: Vec<(i32, [i32; 8])> = (win.y0..win.y1)
        .map(|dy| {
            let (sy, fy) = src_pos(dy, scale_y);
            (sy, fixed(lanczos4(fy)))
        })
        .collect();
    let src_row = |sy: i32, k: usize| (sy - 3 + k as i32).clamp(0, sh as i32 - 1) as usize;

    // Horizontal pass of one source row into `wc` ints (HResizeLanczos4; out-of-range taps
    // step back by whole pixels, as its border loop does).
    let (swc, cni) = (line as i32, cn as i32);
    let hresize = |row: &[u8], out: &mut [i32]| {
        for (o, &(sxo, a)) in out.chunks_exact_mut(cn).zip(&cols) {
            for (k, ok) in o.iter_mut().enumerate() {
                let sx = sxo * cni + k as i32 - cni * 3;
                let mut v = 0i32;
                for (j, &aj) in a.iter().enumerate() {
                    let mut sxj = sx + j as i32 * cni;
                    while sxj < 0 {
                        sxj += cni;
                    }
                    while sxj >= swc {
                        sxj -= cni;
                    }
                    v = v.wrapping_add((row[sxj as usize] as i32).wrapping_mul(aj));
                }
                *ok = v;
            }
        }
    };

    // Source rows each batch of destination rows reads, and those not already cached by the
    // previous batch (all past its last row, as windows only move down).
    let needed: Vec<Vec<usize>> = rows_y
        .chunks(BATCH)
        .map(|batch| {
            let mut n: Vec<usize> = batch
                .iter()
                .flat_map(|&(sy, _)| (0..8).map(move |k| src_row(sy, k)))
                .collect();
            n.sort_unstable();
            n.dedup();
            n
        })
        .collect();
    let fresh: Vec<&[usize]> = needed
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let prev = i.checked_sub(1).and_then(|p| needed[p].last());
            &n[n.partition_point(|&r| prev.is_some_and(|&p| r <= p))..]
        })
        .collect();

    let mut next_src = 0usize;
    let mut skip = vec![0u8; line];
    let mut read = |rows: &[usize], raw: &mut Vec<u8>| -> bool {
        raw.resize(rows.len() * line, 0);
        for (&r, out) in rows.iter().zip(raw.chunks_exact_mut(line)) {
            while next_src < r {
                if !next_row(&mut skip) {
                    return false;
                }
                next_src += 1;
            }
            if !next_row(out) {
                return false;
            }
            next_src += 1;
        }
        true
    };

    // Horizontally resized source rows, sorted by row index; rows the current batch does not
    // need are recycled through `free`. Decoding the next batch's rows overlaps resizing this
    // one's.
    let mut cache: Vec<(usize, Vec<i32>)> = Vec::new();
    let mut free: Vec<Vec<i32>> = Vec::new();
    let (mut raw, mut ahead) = (Vec::new(), Vec::new());
    if !read(fresh[0], &mut raw) {
        return None;
    }
    let batches = dst.chunks_mut(BATCH * wc).zip(rows_y.chunks(BATCH));
    for (i, (out, batch)) in batches.enumerate() {
        let (ok, ()) = rayon::join(
            || fresh.get(i + 1).is_none_or(|f| read(f, &mut ahead)),
            || {
                cache.retain_mut(|(r, v)| {
                    let keep = needed[i].binary_search(r).is_ok();
                    if !keep {
                        free.push(std::mem::take(v));
                    }
                    keep
                });
                let mut bufs: Vec<Vec<i32>> = fresh[i]
                    .iter()
                    .map(|_| {
                        let mut v = free.pop().unwrap_or_default();
                        v.resize(wc, 0);
                        v
                    })
                    .collect();
                bufs.par_iter_mut()
                    .zip(raw.par_chunks(line))
                    .for_each(|(o, row)| hresize(row, o));
                cache.extend(fresh[i].iter().copied().zip(bufs));

                let cache = &cache;
                let get = |r: usize| -> &[i32] {
                    let i = cache.binary_search_by_key(&r, |&(k, _)| k).unwrap();
                    &cache[i].1
                };
                out.par_chunks_mut(wc)
                    .zip(batch.par_iter())
                    .for_each(|(o, &(sy, beta))| {
                        let rows: [&[i32]; 8] = std::array::from_fn(|k| get(src_row(sy, k)));
                        for (x, d) in o.iter_mut().enumerate() {
                            let mut v = 0i32;
                            for k in 0..8 {
                                v = v.wrapping_add(rows[k][x].wrapping_mul(beta[k]));
                            }
                            *d = (v.wrapping_add(1 << (CAST_SHIFT - 1)) >> CAST_SHIFT).clamp(0, 255)
                                as u8;
                        }
                    });
            },
        );
        if !ok {
            return None;
        }
        std::mem::swap(&mut raw, &mut ahead);
    }
    Some(dst)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noise(n: usize) -> Vec<u8> {
        let mut s = 0x2545_f491_4f6c_dd1du64;
        (0..n)
            .map(|_| {
                s ^= s << 13;
                s ^= s >> 7;
                s ^= s << 17;
                s as u8
            })
            .collect()
    }

    /// Every window of the streamed resize equals the same pixels of the whole resize, and rows
    /// past the window's last source row are never pulled.
    #[test]
    fn windows_match_full_resize() {
        for &(sw, sh, cn, dw, dh) in &[
            (97, 211, 1, 40, 70),
            (300, 200, 3, 1001, 667),
            (640, 480, 4, 64, 48),
            (33, 1000, 3, 33, 9),
            (5, 7, 1, 5, 7),
        ] {
            let src = noise(sw * sh * cn);
            let full = resize_lanczos4(&src, sw, sh, cn, dw, dh);
            for win in [
                Window::full(dw, dh),
                Window {
                    x0: dw / 3,
                    x1: dw - dw / 4,
                    y0: dh / 5,
                    y1: dh / 2 + 1,
                },
                Window {
                    x0: 0,
                    x1: 1,
                    y0: dh - 1,
                    y1: dh,
                },
            ] {
                let mut y = 0;
                let out = resize_lanczos4_rows(sw, sh, cn, dw, dh, win, &mut |row| {
                    row.copy_from_slice(&src[y * sw * cn..(y + 1) * sw * cn]);
                    y += 1;
                    true
                })
                .unwrap();
                let wc = (win.x1 - win.x0) * cn;
                for (i, r) in out.chunks(wc).enumerate() {
                    let o = ((win.y0 + i) * dw + win.x0) * cn;
                    assert_eq!(r, &full[o..o + wc], "{sw}x{sh}x{cn}->{dw}x{dh} {win:?}");
                }
                let last =
                    ((win.y1 as f64 - 0.5) * sh as f64 / dh as f64 - 0.5).floor() as usize + 5;
                assert!(y <= last.min(sh), "read {y} rows for {win:?}");
            }
        }
    }

    #[test]
    fn decode_error_stops_the_resize() {
        let mut n = 0;
        let r = resize_lanczos4_rows(64, 64, 1, 32, 32, Window::full(32, 32), &mut |_| {
            n += 1;
            n < 10
        });
        assert!(r.is_none());
    }
}
