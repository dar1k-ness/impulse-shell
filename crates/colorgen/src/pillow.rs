//! Pillow 13.0 `Image.resize(size, Resampling.BICUBIC)` for 8-bit images
//! (libImaging/Resample.c, and the RGBA -> RGBa -> RGBA round trip from Image.py/Convert.c).

const PRECISION_BITS: u32 = 32 - 8 - 2;
const SUPPORT: f64 = 2.0;

fn bicubic(x: f64) -> f64 {
    const A: f64 = -0.5;
    let x = x.abs();
    if x < 1.0 {
        ((A + 2.0) * x - (A + 3.0)) * x * x + 1.0
    } else if x < 2.0 {
        (((x - 5.0) * x + 8.0) * x - 4.0) * A
    } else {
        0.0
    }
}

struct Coeffs {
    ksize: usize,
    bounds: Vec<(usize, usize)>,
    kk: Vec<i32>,
}

fn precompute(in_size: usize, out_size: usize) -> Coeffs {
    let (in0, in1) = (0.0f32, in_size as f32);
    let scale = (in1 - in0) as f64 / out_size as f64;
    let filterscale = scale.max(1.0);
    let support = SUPPORT * filterscale;
    let ksize = support.ceil() as usize * 2 + 1;
    let mut kk = vec![0i32; out_size * ksize];
    let mut bounds = Vec::with_capacity(out_size);
    let mut k = vec![0f64; ksize];
    for xx in 0..out_size {
        let center = in0 as f64 + (xx as f64 + 0.5) * scale;
        let ss = 1.0 / filterscale;
        let xmin = ((center - support + 0.5) as i32).max(0) as usize;
        let xmax = ((center + support + 0.5) as i32).min(in_size as i32) as usize - xmin;
        let mut ww = 0.0;
        for (x, kx) in k.iter_mut().enumerate().take(xmax) {
            let w = bicubic((x as f64 + xmin as f64 - center + 0.5) * ss);
            *kx = w;
            ww += w;
        }
        for (x, kx) in k.iter_mut().enumerate().take(xmax) {
            if ww != 0.0 {
                *kx /= ww;
            }
            let v = *kx * (1u32 << PRECISION_BITS) as f64;
            kk[xx * ksize + x] = if *kx < 0.0 {
                (-0.5 + v) as i32
            } else {
                (0.5 + v) as i32
            };
        }
        bounds.push((xmin, xmax));
    }
    Coeffs { ksize, bounds, kk }
}

fn clip8(v: i32) -> u8 {
    (v >> PRECISION_BITS).clamp(0, 255) as u8
}

/// Resizes interleaved `ch`-channel pixels (`ch` = 3 or 4, alpha already premultiplied
/// where Pillow does it) from `w`x`h` to `nw`x`nh`.
fn resample(px: &[u8], ch: usize, w: usize, h: usize, nw: usize, nh: usize) -> Vec<u8> {
    let horiz = precompute(w, nw);
    let mut vert = precompute(h, nh);
    let need_h = nw != w;
    let need_v = nh != h;
    let mut cur = px.to_vec();
    let (mut cw, mut chh) = (w, h);
    if need_h {
        let first = vert.bounds[0].0;
        let last = vert.bounds[nh - 1].0 + vert.bounds[nh - 1].1;
        for b in &mut vert.bounds {
            b.0 -= first;
        }
        let rows = last - first;
        let mut out = vec![0u8; nw * rows * ch];
        for yy in 0..rows {
            let row = &cur[(yy + first) * w * ch..];
            for xx in 0..nw {
                let (xmin, xmax) = horiz.bounds[xx];
                let k = &horiz.kk[xx * horiz.ksize..];
                for c in 0..ch {
                    let mut ss = 1i32 << (PRECISION_BITS - 1);
                    for x in 0..xmax {
                        ss = ss.wrapping_add(row[(x + xmin) * ch + c] as i32 * k[x]);
                    }
                    out[(yy * nw + xx) * ch + c] = clip8(ss);
                }
            }
        }
        cur = out;
        cw = nw;
        chh = rows;
    }
    if need_v {
        let mut out = vec![0u8; cw * nh * ch];
        for yy in 0..nh {
            let (ymin, ymax) = vert.bounds[yy];
            let k = &vert.kk[yy * vert.ksize..];
            for xx in 0..cw {
                for c in 0..ch {
                    let mut ss = 1i32 << (PRECISION_BITS - 1);
                    for y in 0..ymax {
                        ss = ss.wrapping_add(cur[((y + ymin) * cw + xx) * ch + c] as i32 * k[y]);
                    }
                    out[(yy * cw + xx) * ch + c] = clip8(ss);
                }
            }
        }
        cur = out;
        chh = nh;
    }
    debug_assert_eq!(cur.len(), cw * chh * ch);
    cur
}

fn muldiv255(a: u32, b: u32) -> u8 {
    let t = a * b + 128;
    (((t >> 8) + t) >> 8) as u8
}

/// `Image.resize((nw, nh), BICUBIC)` for mode "RGB" (`ch` = 3) or "RGBA" (`ch` = 4).
pub fn resize_bicubic(px: &[u8], ch: usize, w: usize, h: usize, nw: usize, nh: usize) -> Vec<u8> {
    if (w, h) == (nw, nh) {
        return px.to_vec();
    }
    if ch != 4 {
        return resample(px, ch, w, h, nw, nh);
    }
    let pre: Vec<u8> = px
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| {
            let a = p[3] as u32;
            [
                muldiv255(p[0] as u32, a),
                muldiv255(p[1] as u32, a),
                muldiv255(p[2] as u32, a),
                p[3],
            ]
        })
        .collect();
    let mut out = resample(&pre, 4, w, h, nw, nh);
    for p in out.as_chunks_mut::<4>().0 {
        let a = p[3] as u32;
        if a != 0 && a != 255 {
            for c in &mut p[..3] {
                *c = ((255 * *c as u32) / a).min(255) as u8;
            }
        }
    }
    out
}

/// `calculate_optimal_size` from generate_colors_material.py.
pub fn optimal_size(w: usize, h: usize, bitmap: usize) -> (usize, usize) {
    let ia = (w * h) as f64;
    let ba = (bitmap * bitmap) as f64;
    let scale = if ia > ba { (ba / ia).sqrt() } else { 1.0 };
    let r = |v: f64| (v.round_ties_even() as usize).max(1);
    (r(w as f64 * scale), r(h as f64 * scale))
}
