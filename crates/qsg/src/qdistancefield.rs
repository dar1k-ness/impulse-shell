//! qdistancefield.cpp makeDistanceField (Qt 6.12): scanline-rasterized 8:8 distance field.
use crate::pathsimplify::{self, END, P, PathEl};

#[derive(Clone, Copy, PartialEq)]
enum H {
    L2R,
    R2L,
}
#[derive(Clone, Copy, PartialEq)]
enum V {
    Down,
    Up,
}

#[inline]
fn upd(v: &mut i32, val: i32) {
    if val.wrapping_abs() < v.wrapping_abs() {
        *v = val;
    }
}

fn fill_line(clip: bool, dir: H, line: &mut [i32], lx: i32, rx: i32, d: i32, dd: i32) {
    let width = line.len() as i32;
    let (from, to) = if clip {
        (0.max(lx >> 8), width.min(rx >> 8))
    } else {
        (lx >> 8, rx >> 8)
    };
    let n = to - from;
    if n <= 0 {
        return;
    }
    if dir == H::L2R {
        let mut val = if clip {
            d.wrapping_add((((from << 8) + 0xff - lx).wrapping_mul(dd)) >> 8)
        } else {
            d.wrapping_add(((!lx & 0xff).wrapping_mul(dd)) >> 8)
        };
        for x in from..to {
            upd(&mut line[x as usize], val);
            val = val.wrapping_add(dd);
        }
    } else {
        let mut val = if clip {
            d.wrapping_add((((to << 8) + 0xff - rx).wrapping_mul(dd)) >> 8)
        } else {
            d.wrapping_add(((!rx & 0xff).wrapping_mul(dd)) >> 8)
        };
        let mut x = to;
        for _ in 0..n {
            val = val.wrapping_sub(dd);
            x -= 1;
            upd(&mut line[x as usize], val);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn fill_lines(
    clip: bool,
    vd: V,
    hd: H,
    bits: &mut [i32],
    width: i32,
    upper: i32,
    lower: i32,
    lx: &mut i32,
    ldx: i32,
    rx: &mut i32,
    rdx: i32,
    d: &mut i32,
    ddy: i32,
    ddx: i32,
) {
    let mut y = lower - upper;
    if y <= 0 {
        return;
    }
    let w = width as usize;
    if vd == V::Down {
        let mut row = upper;
        loop {
            let s = row as usize * w;
            fill_line(clip, hd, &mut bits[s..s + w], *lx, *rx, *d, ddx);
            *lx = lx.wrapping_add(ldx);
            *d = d.wrapping_add(ddy);
            *rx = rx.wrapping_add(rdx);
            row += 1;
            y -= 1;
            if y == 0 {
                break;
            }
        }
    } else {
        let mut row = lower;
        loop {
            *lx = lx.wrapping_sub(ldx);
            *d = d.wrapping_sub(ddy);
            *rx = rx.wrapping_sub(rdx);
            row -= 1;
            let s = row as usize * w;
            fill_line(clip, hd, &mut bits[s..s + w], *lx, *rx, *d, ddx);
            y -= 1;
            if y == 0 {
                break;
            }
        }
    }
}

fn yb(clip: bool, v: i32, h: i32) -> i32 {
    if clip { (v >> 8).clamp(0, h) } else { v >> 8 }
}
fn frac(clip: bool, yi: i32, v: i32) -> i32 {
    if clip {
        (yi << 8) + 0xff - v
    } else {
        !v & 0xff
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_triangle(
    clip: bool,
    bits: &mut [i32],
    width: i32,
    height: i32,
    c: P,
    v1: P,
    v2: P,
    value: i32,
) {
    let y1 = yb(clip, v1.1, height);
    let y2 = yb(clip, v2.1, height);
    let yc = yb(clip, c.1, height);
    let v1f = frac(clip, y1, v1.1);
    let v2f = frac(clip, y2, v2.1);
    let cf = frac(clip, yc, c.1);
    let (mut dx1, mut x1, mut dx2, mut x2) = (0, 0, 0, 0);
    if v1.1 != c.1 {
        dx1 = ((v1.0 - c.0) << 8) / (v1.1 - c.1);
        x1 = c.0 + cf * (v1.0 - c.0) / (v1.1 - c.1);
    }
    if v2.1 != c.1 {
        dx2 = ((v2.0 - c.0) << 8) / (v2.1 - c.1);
        x2 = c.0 + cf * (v2.0 - c.0) / (v2.1 - c.1);
    }
    let div = (v2.0 - c.0)
        .wrapping_mul(v1.1 - c.1)
        .wrapping_sub((v2.1 - c.1).wrapping_mul(v1.0 - c.0));
    let dd = if div != 0 {
        (((value as i64 * (v1.1 - v2.1) as i64) << 8) / div as i64) as i32
    } else {
        0
    };
    let mut val = value;
    if y2 < yc {
        if y1 < yc {
            if y2 < y1 {
                let mut d1 = cf * value / (v1.1 - c.1);
                let dd1 = (value << 8) / (v1.1 - c.1);
                fill_lines(
                    clip,
                    V::Up,
                    H::L2R,
                    bits,
                    width,
                    y1,
                    yc,
                    &mut x1,
                    dx1,
                    &mut x2,
                    dx2,
                    &mut d1,
                    dd1,
                    dd,
                );
                dx1 = ((v1.0 - v2.0) << 8) / (v1.1 - v2.1);
                x1 = v1.0 + v1f * (v1.0 - v2.0) / (v1.1 - v2.1);
                fill_lines(
                    clip,
                    V::Up,
                    H::L2R,
                    bits,
                    width,
                    y2,
                    y1,
                    &mut x1,
                    dx1,
                    &mut x2,
                    dx2,
                    &mut val,
                    0,
                    dd,
                );
            } else {
                let mut d2 = cf * value / (v2.1 - c.1);
                let dd2 = (value << 8) / (v2.1 - c.1);
                fill_lines(
                    clip,
                    V::Up,
                    H::R2L,
                    bits,
                    width,
                    y2,
                    yc,
                    &mut x1,
                    dx1,
                    &mut x2,
                    dx2,
                    &mut d2,
                    dd2,
                    dd,
                );
                if y1 != y2 {
                    dx2 = ((v1.0 - v2.0) << 8) / (v1.1 - v2.1);
                    x2 = v2.0 + v2f * (v1.0 - v2.0) / (v1.1 - v2.1);
                    fill_lines(
                        clip,
                        V::Up,
                        H::R2L,
                        bits,
                        width,
                        y1,
                        y2,
                        &mut x1,
                        dx1,
                        &mut x2,
                        dx2,
                        &mut val,
                        0,
                        dd,
                    );
                }
            }
        } else {
            let dx = ((v1.0 - v2.0) << 8) / (v1.1 - v2.1);
            let off = if clip {
                (yc << 8) + 0xff - v2.1
            } else {
                (c.1 | 0xff) - v2.1
            };
            let mut xup = v2.0 + off * (v1.0 - v2.0) / (v1.1 - v2.1);
            let mut xdn = xup;
            fill_lines(
                clip,
                V::Up,
                H::L2R,
                bits,
                width,
                y2,
                yc,
                &mut xup,
                dx,
                &mut x2,
                dx2,
                &mut val,
                0,
                dd,
            );
            if yc != y1 {
                let mut v = value;
                fill_lines(
                    clip,
                    V::Down,
                    H::L2R,
                    bits,
                    width,
                    yc,
                    y1,
                    &mut xdn,
                    dx,
                    &mut x1,
                    dx1,
                    &mut v,
                    0,
                    dd,
                );
            }
        }
    } else if y1 < yc {
        let dx = ((v1.0 - v2.0) << 8) / (v1.1 - v2.1);
        let off = if clip {
            (yc << 8) + 0xff - v1.1
        } else {
            (c.1 | 0xff) - v1.1
        };
        let mut xup = v1.0 + off * (v1.0 - v2.0) / (v1.1 - v2.1);
        let mut xdn = xup;
        fill_lines(
            clip,
            V::Up,
            H::R2L,
            bits,
            width,
            y1,
            yc,
            &mut x1,
            dx1,
            &mut xup,
            dx,
            &mut val,
            0,
            dd,
        );
        if yc != y2 {
            let mut v = value;
            fill_lines(
                clip,
                V::Down,
                H::R2L,
                bits,
                width,
                yc,
                y2,
                &mut x2,
                dx2,
                &mut xdn,
                dx,
                &mut v,
                0,
                dd,
            );
        }
    } else if y2 < y1 {
        if yc != y2 {
            let mut d2 = cf * value / (v2.1 - c.1);
            let dd2 = (value << 8) / (v2.1 - c.1);
            fill_lines(
                clip,
                V::Down,
                H::L2R,
                bits,
                width,
                yc,
                y2,
                &mut x2,
                dx2,
                &mut x1,
                dx1,
                &mut d2,
                dd2,
                dd,
            );
        }
        dx2 = ((v1.0 - v2.0) << 8) / (v1.1 - v2.1);
        x2 = v2.0 + v2f * (v1.0 - v2.0) / (v1.1 - v2.1);
        fill_lines(
            clip,
            V::Down,
            H::L2R,
            bits,
            width,
            y2,
            y1,
            &mut x2,
            dx2,
            &mut x1,
            dx1,
            &mut val,
            0,
            dd,
        );
    } else {
        if yc != y1 {
            let mut d1 = cf * value / (v1.1 - c.1);
            let dd1 = (value << 8) / (v1.1 - c.1);
            fill_lines(
                clip,
                V::Down,
                H::R2L,
                bits,
                width,
                yc,
                y1,
                &mut x2,
                dx2,
                &mut x1,
                dx1,
                &mut d1,
                dd1,
                dd,
            );
        }
        if y1 != y2 {
            dx1 = ((v1.0 - v2.0) << 8) / (v1.1 - v2.1);
            x1 = v1.0 + v1f * (v1.0 - v2.0) / (v1.1 - v2.1);
            fill_lines(
                clip,
                V::Down,
                H::R2L,
                bits,
                width,
                y1,
                y2,
                &mut x2,
                dx2,
                &mut x1,
                dx1,
                &mut val,
                0,
                dd,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_rectangle(
    clip: bool,
    bits: &mut [i32],
    width: i32,
    height: i32,
    int1: P,
    center1: P,
    ext1: P,
    int2: P,
    center2: P,
    ext2: P,
    ext_value: i32,
) {
    let (mut int1, mut center1, mut ext1, mut int2, mut center2, mut ext2, mut ext_value) =
        (int1, center1, ext1, int2, center2, ext2, ext_value);
    if center1.1 > center2.1 {
        std::mem::swap(&mut center1, &mut center2);
        std::mem::swap(&mut int1, &mut ext2);
        std::mem::swap(&mut ext1, &mut int2);
        ext_value = -ext_value;
    }
    let yc1 = yb(clip, center1.1, height);
    let yc2 = yb(clip, center2.1, height);
    let yi1 = yb(clip, int1.1, height);
    let yi2 = yb(clip, int2.1, height);
    let ye1 = yb(clip, ext1.1, height);
    let ye2 = yb(clip, ext2.1, height);
    let c1f = frac(clip, yc1, center1.1);
    let c2f = frac(clip, yc2, center2.1);
    let i1f = frac(clip, yi1, int1.1);
    let e1f = frac(clip, ye1, ext1.1);
    let (mut dxc, mut dxe, mut ddc) = (0, 0, 0);
    if ext1.1 != int1.1 {
        dxc = ((ext1.0 - int1.0) << 8) / (ext1.1 - int1.1);
        ddc = (ext_value << 9) / (ext1.1 - int1.1);
    }
    if ext1.1 != ext2.1 {
        dxe = ((ext1.0 - ext2.0) << 8) / (ext1.1 - ext2.1);
    }
    let div = (ext1.0 - int1.0)
        .wrapping_mul(ext2.1 - int1.1)
        .wrapping_sub((ext1.1 - int1.1).wrapping_mul(ext2.0 - int1.0));
    let dd = if div != 0 {
        (((ext_value as i64 * (ext2.1 - ext1.1) as i64) << 9) / div as i64) as i32
    } else {
        0
    };
    let int_value = -ext_value;
    let (mut ev, mut iv) = (ext_value, int_value);
    let w = width;
    if center2.0 < center1.0 {
        if int1.1 < ext2.1 {
            let mut xe1 = ext1.0 + e1f * (ext1.0 - ext2.0) / (ext1.1 - ext2.1);
            let mut xe2 = int1.0 + i1f * (ext1.0 - ext2.0) / (ext1.1 - ext2.1);
            if ye1 != yi1 {
                let mut xc2 = center1.0 + c1f * (ext1.0 - int1.0) / (ext1.1 - int1.1);
                xc2 += (ye1 - yc1) * dxc;
                fill_lines(
                    clip,
                    V::Down,
                    H::L2R,
                    bits,
                    w,
                    ye1,
                    yi1,
                    &mut xe1,
                    dxe,
                    &mut xc2,
                    dxc,
                    &mut ev,
                    0,
                    dd,
                );
            }
            if yi1 != ye2 {
                fill_lines(
                    clip,
                    V::Down,
                    H::L2R,
                    bits,
                    w,
                    yi1,
                    ye2,
                    &mut xe1,
                    dxe,
                    &mut xe2,
                    dxe,
                    &mut ev,
                    0,
                    dd,
                );
            }
            if ye2 != yi2 {
                let mut xc1 = center2.0 + c2f * (ext1.0 - int1.0) / (ext1.1 - int1.1);
                xc1 += (ye2 - yc2) * dxc;
                fill_lines(
                    clip,
                    V::Down,
                    H::R2L,
                    bits,
                    w,
                    ye2,
                    yi2,
                    &mut xc1,
                    dxc,
                    &mut xe2,
                    dxe,
                    &mut iv,
                    0,
                    dd,
                );
            }
        } else {
            let mut xc1 = center2.0 + c2f * (ext1.0 - int1.0) / (ext1.1 - int1.1);
            let mut xc2 = center1.0 + c1f * (ext1.0 - int1.0) / (ext1.1 - int1.1);
            xc1 += (ye2 - yc2) * dxc;
            xc2 += (ye1 - yc1) * dxc;
            if ye1 != ye2 {
                let mut xe1 = ext1.0 + e1f * (ext1.0 - ext2.0) / (ext1.1 - ext2.1);
                fill_lines(
                    clip,
                    V::Down,
                    H::L2R,
                    bits,
                    w,
                    ye1,
                    ye2,
                    &mut xe1,
                    dxe,
                    &mut xc2,
                    dxc,
                    &mut ev,
                    0,
                    dd,
                );
            }
            if ye2 != yi1 {
                let off = if clip {
                    (ye2 << 8) + 0xff - center2.1
                } else {
                    (ext2.1 | 0xff) - center2.1
                };
                let mut d = off * 2 * ext_value / (ext1.1 - int1.1);
                fill_lines(
                    clip,
                    V::Down,
                    H::L2R,
                    bits,
                    w,
                    ye2,
                    yi1,
                    &mut xc1,
                    dxc,
                    &mut xc2,
                    dxc,
                    &mut d,
                    ddc,
                    dd,
                );
            }
            if yi1 != yi2 {
                let mut xe2 = int1.0 + i1f * (ext1.0 - ext2.0) / (ext1.1 - ext2.1);
                fill_lines(
                    clip,
                    V::Down,
                    H::R2L,
                    bits,
                    w,
                    yi1,
                    yi2,
                    &mut xc1,
                    dxc,
                    &mut xe2,
                    dxe,
                    &mut iv,
                    0,
                    dd,
                );
            }
        }
    } else if ext1.1 < int2.1 {
        let mut xe1 = ext1.0 + e1f * (ext1.0 - ext2.0) / (ext1.1 - ext2.1);
        let mut xe2 = int1.0 + i1f * (ext1.0 - ext2.0) / (ext1.1 - ext2.1);
        if yi1 != ye1 {
            let mut xc1 = center1.0 + c1f * (ext1.0 - int1.0) / (ext1.1 - int1.1);
            xc1 += (yi1 - yc1) * dxc;
            fill_lines(
                clip,
                V::Down,
                H::R2L,
                bits,
                w,
                yi1,
                ye1,
                &mut xc1,
                dxc,
                &mut xe2,
                dxe,
                &mut iv,
                0,
                dd,
            );
        }
        if ye1 != yi2 {
            fill_lines(
                clip,
                V::Down,
                H::R2L,
                bits,
                w,
                ye1,
                yi2,
                &mut xe1,
                dxe,
                &mut xe2,
                dxe,
                &mut iv,
                0,
                dd,
            );
        }
        if yi2 != ye2 {
            let mut xc2 = center2.0 + c2f * (ext1.0 - int1.0) / (ext1.1 - int1.1);
            xc2 += (yi2 - yc2) * dxc;
            fill_lines(
                clip,
                V::Down,
                H::L2R,
                bits,
                w,
                yi2,
                ye2,
                &mut xe1,
                dxe,
                &mut xc2,
                dxc,
                &mut ev,
                0,
                dd,
            );
        }
    } else {
        let mut xc1 = center1.0 + c1f * (ext1.0 - int1.0) / (ext1.1 - int1.1);
        let mut xc2 = center2.0 + c2f * (ext1.0 - int1.0) / (ext1.1 - int1.1);
        xc1 += (yi1 - yc1) * dxc;
        xc2 += (yi2 - yc2) * dxc;
        if yi1 != yi2 {
            let mut xe2 = int1.0 + i1f * (ext1.0 - ext2.0) / (ext1.1 - ext2.1);
            fill_lines(
                clip,
                V::Down,
                H::R2L,
                bits,
                w,
                yi1,
                yi2,
                &mut xc1,
                dxc,
                &mut xe2,
                dxe,
                &mut iv,
                0,
                dd,
            );
        }
        if yi2 != ye1 {
            let off = if clip {
                (yi2 << 8) + 0xff - center2.1
            } else {
                (int2.1 | 0xff) - center2.1
            };
            let mut d = off * 2 * ext_value / (ext1.1 - int1.1);
            fill_lines(
                clip,
                V::Down,
                H::R2L,
                bits,
                w,
                yi2,
                ye1,
                &mut xc1,
                dxc,
                &mut xc2,
                dxc,
                &mut d,
                ddc,
                dd,
            );
        }
        if ye1 != ye2 {
            let mut xe1 = ext1.0 + e1f * (ext1.0 - ext2.0) / (ext1.1 - ext2.1);
            fill_lines(
                clip,
                V::Down,
                H::L2R,
                bits,
                w,
                ye1,
                ye2,
                &mut xe1,
                dxe,
                &mut xc2,
                dxc,
                &mut ev,
                0,
                dd,
            );
        }
    }
}

fn draw_polygons(bits: &mut [i32], width: i32, height: i32, verts: &[P], idx: &[u32], value: i32) {
    let mut scans: Vec<Vec<u16>> = vec![vec![]; height as usize];
    let mut first = 0usize;
    let mut i = 1usize;
    while i < idx.len() {
        let i1 = idx[i - 1];
        let mut i2 = idx[i];
        if i2 == END {
            i2 = idx[first];
            i += 1;
            first = i;
        }
        let (mut v1, mut v2) = (verts[i1 as usize], verts[i2 as usize]);
        if v2.1 < v1.1 {
            std::mem::swap(&mut v1, &mut v2);
        }
        let fy = 0.max(v1.1 >> 8);
        let ty = height.min(v2.1 >> 8);
        if fy < ty {
            let dx = ((v2.0 - v1.0) << 8) / (v2.1 - v1.1);
            let mut x = v1.0 + ((fy << 8) + 0xff - v1.1) * (v2.0 - v1.0) / (v2.1 - v1.1);
            for y in fy..ty {
                let c = (x >> 8) as u32;
                if c < width as u32 {
                    scans[y as usize].push(c as u16);
                }
                x += dx;
            }
        }
        i += 1;
    }
    for (row, s) in scans.iter_mut().enumerate() {
        s.sort_unstable();
        let line = &mut bits[row * width as usize..(row + 1) * width as usize];
        let mut j = 0;
        while j + 1 < s.len() {
            for x in s[j]..s[j + 1] {
                line[x as usize] = value;
            }
            j += 2;
        }
        if j < s.len()
            && let Some(rest) = line.get_mut(s[j] as usize..width as usize)
        {
            rest.fill(value);
        }
    }
}

/// makeDistanceField(data, path, dfScale, offs): `path` already translated to its bounding-rect
/// top-left (QDistanceField::setGlyph), WindingFill. Returns w*h bytes.
pub fn make_distance_field(path: &[PathEl], w: i32, h: i32, df_scale: i32, offs: i32) -> Vec<u8> {
    let n = (w * h) as usize;
    if path.is_empty() {
        return vec![0; n];
    }
    let (verts, idx) = pathsimplify::simplify(path, 1.0 / df_scale as f64, offs as f64);
    if verts.is_empty() {
        return vec![0; n];
    }
    const INTERIOR: i32 = -0x7f80;
    const EXTERIOR: i32 = 0x7f80;
    let mut bits = vec![EXTERIOR; n];
    let step = 15f64.to_radians();
    let qr = |v: f64| {
        if v >= 0.0 {
            (v + 0.5).floor() as i32
        } else {
            (v - 0.5).ceil() as i32
        }
    };
    let rot = (qr(step.cos() * 16384.0), qr(step.sin() * 16384.0));
    draw_polygons(&mut bits, w, h, &verts, &idx, INTERIOR);
    let mut index = 0usize;
    while index < idx.len() {
        let mut end = index;
        while idx[end] != END {
            end += 1;
        }
        let mut normals: Vec<P> = vec![];
        let mut vertices: Vec<P> = vec![];
        let mut clipv: Vec<bool> = vec![];
        let (mut next, mut prev) = (index, end - 1);
        while next < end {
            let from = verts[idx[prev] as usize];
            let to = verts[idx[next] as usize];
            let mut nn = (to.1 - from.1, from.0 - to.0);
            if !(nn.0 == 0 && nn.1 == 0) {
                let len = ((nn.0 as f64) * nn.0 as f64 + (nn.1 as f64) * nn.1 as f64).sqrt();
                let scale = qr(((offs << 16) as f64) / len);
                nn = (nn.0.wrapping_mul(scale) >> 8, nn.1.wrapping_mul(scale) >> 8);
                normals.push(nn);
                vertices.push((to.0 + 0x7f, to.1 + 0x7f));
                clipv.push(
                    to.0 < offs << 8
                        || to.0 >= (w - offs) << 8
                        || to.1 < offs << 8
                        || to.1 >= (h - offs) << 8,
                );
            }
            prev = next;
            next += 1;
        }
        let m = normals.len();
        let mut cps = vec![0i32; m];
        if m > 0 {
            let (mut nx, mut pv) = (0usize, m - 1);
            while nx < m {
                cps[pv] = normals[pv]
                    .0
                    .wrapping_mul(normals[nx].1)
                    .wrapping_sub(normals[pv].1.wrapping_mul(normals[nx].0));
                pv = nx;
                nx += 1;
            }
            let (mut nx, mut pv) = (0usize, m - 1);
            while nx < m {
                let mut nrm = normals[nx];
                let vp = vertices[pv];
                let vn = vertices[nx];
                let mut ext_prev = (vp.0 - nrm.0, vp.1 - nrm.1);
                let mut int_prev = (vp.0 + nrm.0, vp.1 + nrm.1);
                let ext_next = (vn.0 - nrm.0, vn.1 - nrm.1);
                let int_next = (vn.0 + nrm.0, vn.1 + nrm.1);
                let clip = clipv[pv] || clipv[nx];
                draw_rectangle(
                    clip, &mut bits, w, h, int_prev, vp, ext_prev, int_next, vn, ext_next, EXTERIOR,
                );
                let cp = cps[pv];
                let np = normals[pv];
                let tclip = clipv[pv];
                if cp < 0 {
                    loop {
                        nrm = (
                            (nrm.0 * rot.0 - nrm.1 * rot.1) >> 14,
                            (nrm.1 * rot.0 + nrm.0 * rot.1) >> 14,
                        );
                        if nrm.0 * np.1 - nrm.1 * np.0 <= 0 {
                            let p = (vp.0 - np.0, vp.1 - np.1);
                            draw_triangle(tclip, &mut bits, w, h, vp, ext_prev, p, EXTERIOR);
                            break;
                        }
                        let p = (vp.0 - nrm.0, vp.1 - nrm.1);
                        draw_triangle(tclip, &mut bits, w, h, vp, ext_prev, p, EXTERIOR);
                        ext_prev = p;
                    }
                } else if cp > 0 {
                    loop {
                        nrm = (
                            (nrm.0 * rot.0 + nrm.1 * rot.1) >> 14,
                            (nrm.1 * rot.0 - nrm.0 * rot.1) >> 14,
                        );
                        if nrm.0 * np.1 - nrm.1 * np.0 >= 0 {
                            let p = (vp.0 + np.0, vp.1 + np.1);
                            draw_triangle(tclip, &mut bits, w, h, vp, p, int_prev, INTERIOR);
                            break;
                        }
                        let p = (vp.0 + nrm.0, vp.1 + nrm.1);
                        draw_triangle(tclip, &mut bits, w, h, vp, p, int_prev, INTERIOR);
                        int_prev = p;
                    }
                }
                pv = nx;
                nx += 1;
            }
        }
        index = end + 1;
    }
    bits.iter().map(|&v| ((0x7f80 - v) >> 8) as u8).collect()
}
