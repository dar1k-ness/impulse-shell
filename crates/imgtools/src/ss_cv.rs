//! OpenCV 4.13 imgproc primitives used by selective search, reproduced bit for bit for its
//! x86-64 code paths (IPP off for these calls). The Gaussian blur is the one whose result
//! depends on the CPU: see [`filter_fma`].

/// `cvRound(float)`/`cvRound(double)`: round half to even, `INT_MIN` when out of range or NaN.
pub fn cv_round(x: f64) -> i32 {
    if x.is_nan() || x >= 2_147_483_647.5 || x < -2_147_483_648.5 {
        i32::MIN
    } else {
        x.round_ties_even() as i32
    }
}

pub fn sat_u8(v: i32) -> u8 {
    v.clamp(0, 255) as u8
}

/// `saturate_cast<uchar>(float)`.
pub fn sat_u8_f32(v: f32) -> u8 {
    sat_u8(cv_round(v as f64))
}

/// `borderInterpolate(p, len, BORDER_REFLECT_101)`.
pub fn reflect101(mut p: isize, len: usize) -> usize {
    let len = len as isize;
    if (p as usize) < len as usize && p >= 0 {
        return p as usize;
    }
    if len == 1 {
        return 0;
    }
    loop {
        if p < 0 {
            p = -p;
        } else {
            p = len - 1 - (p - len) - 1;
        }
        if p >= 0 && p < len {
            return p as usize;
        }
    }
}

/// The script's `--resize-factor` default, the only one the shell runs it with.
pub const RESIZE_FACTOR: f64 = 0.1;

/// `cv2.resize(img, (int(w * 0.1), int(h * 0.1)), interpolation=cv2.INTER_AREA)` of an 8-bit
/// BGR image and its new size; `None` where that size is empty and OpenCV raises. Nothing
/// else is ported: the factors between source and result are 10 and up here, and OpenCV has
/// other code for smaller ones (a block of 2x2 is averaged in integers).
pub fn resize_area_tenth(src: &[u8], w: usize, h: usize) -> Option<(Vec<u8>, usize, usize)> {
    let cn = 3;
    let dw = (w as f64 * RESIZE_FACTOR) as usize;
    let dh = (h as f64 * RESIZE_FACTOR) as usize;
    if dw == 0 || dh == 0 {
        return None;
    }
    let inv_x = dw as f64 / w as f64;
    let inv_y = dh as f64 / h as f64;
    let sx = 1.0 / inv_x;
    let sy = 1.0 / inv_y;
    let isx = cv_round(sx);
    let isy = cv_round(sy);
    let fast = (sx - isx as f64).abs() < f64::EPSILON && (sy - isy as f64).abs() < f64::EPSILON;
    let mut dst = vec![0u8; dw * dh * cn];
    if fast {
        let (isx, isy) = (isx as usize, isy as usize);
        let scale = 1.0f32 / (isx * isy) as f32;
        for dy in 0..dh {
            for dx in 0..dw {
                for c in 0..cn {
                    let mut sum = 0i32;
                    for yy in 0..isy {
                        let row = (dy * isy + yy) * w * cn;
                        for xx in 0..isx {
                            sum += src[row + (dx * isx + xx) * cn + c] as i32;
                        }
                    }
                    dst[(dy * dw + dx) * cn + c] = sat_u8_f32(sum as f32 * scale);
                }
            }
        }
        return Some((dst, dw, dh));
    }
    let xtab = area_tab(w, dw, cn, sx);
    let ytab = area_tab(h, dh, 1, sy);
    let dwc = dw * cn;
    let mut buf = vec![0f32; dwc];
    let mut sum = vec![0f32; dwc];
    let mut prev_dy = ytab[0].1;
    for &(si, di, beta) in &ytab {
        let s = &src[si * w * cn..];
        buf.iter_mut().for_each(|b| *b = 0.0);
        for &(sxn, dxn, alpha) in &xtab {
            for c in 0..cn {
                buf[dxn + c] += s[sxn + c] as f32 * alpha;
            }
        }
        if di != prev_dy {
            store_area(&sum, &mut dst[prev_dy * dwc..(prev_dy + 1) * dwc]);
            for i in 0..dwc {
                sum[i] = buf[i] * beta;
            }
            prev_dy = di;
        } else {
            for i in 0..dwc {
                sum[i] += buf[i] * beta;
            }
        }
    }
    store_area(&sum, &mut dst[prev_dy * dwc..(prev_dy + 1) * dwc]);
    Some((dst, dw, dh))
}

fn store_area(sum: &[f32], d: &mut [u8]) {
    for (o, &s) in d.iter_mut().zip(sum) {
        *o = sat_u8_f32(s);
    }
}

/// `computeResizeAreaTab`: (src index, dst index, alpha).
fn area_tab(ssize: usize, dsize: usize, cn: usize, scale: f64) -> Vec<(usize, usize, f32)> {
    let mut tab = Vec::new();
    for dx in 0..dsize {
        let fsx1 = dx as f64 * scale;
        let fsx2 = fsx1 + scale;
        let cell = scale.min(ssize as f64 - fsx1);
        let mut sx1 = fsx1.ceil() as isize;
        let mut sx2 = fsx2.floor() as isize;
        sx2 = sx2.min(ssize as isize - 1);
        sx1 = sx1.min(sx2);
        if sx1 as f64 - fsx1 > 1e-3 {
            tab.push((
                ((sx1 - 1) as usize) * cn,
                dx * cn,
                ((sx1 as f64 - fsx1) / cell) as f32,
            ));
        }
        for sx in sx1..sx2 {
            tab.push((sx as usize * cn, dx * cn, (1.0 / cell) as f32));
        }
        if fsx2 - sx2 as f64 > 1e-3 {
            tab.push((
                sx2 as usize * cn,
                dx * cn,
                ((fsx2 - sx2 as f64).min(1.0).min(cell) / cell) as f32,
            ));
        }
    }
    tab
}

/// `cvtColor(BGR2HSV)` for 8-bit (H in 0..180).
pub fn bgr2hsv(src: &[u8]) -> Vec<u8> {
    const SHIFT: i32 = 12;
    let mut sdiv = [0i32; 256];
    let mut hdiv = [0i32; 256];
    for i in 1..256 {
        sdiv[i] = cv_round((255 << SHIFT) as f64 / i as f64);
        hdiv[i] = cv_round((180 << SHIFT) as f64 / (6.0 * i as f64));
    }
    let mut dst = vec![0u8; src.len()];
    for (s, d) in src
        .as_chunks::<3>()
        .0
        .iter()
        .zip(dst.as_chunks_mut::<3>().0.iter_mut())
    {
        let (b, g, r) = (s[0] as i32, s[1] as i32, s[2] as i32);
        let v = b.max(g).max(r);
        let vmin = b.min(g).min(r);
        let diff = v - vmin;
        let vr = if v == r { -1 } else { 0 };
        let vg = if v == g { -1 } else { 0 };
        let sv = (diff * sdiv[v as usize] + (1 << (SHIFT - 1))) >> SHIFT;
        let mut h =
            (vr & (g - b)) + (!vr & ((vg & (b - r + 2 * diff)) + (!vg & (r - g + 4 * diff))));
        h = (h * hdiv[diff as usize] + (1 << (SHIFT - 1))) >> SHIFT;
        if h < 0 {
            h += 180;
        }
        d[0] = sat_u8(h);
        d[1] = sv as u8;
        d[2] = v as u8;
    }
    dst
}

const SRGB2XYZ_D65: [u64; 9] = [
    0x3fda65a14488c60d,
    0x3fd6e297396d0918,
    0x3fc71819d2391d58,
    0x3fcb38cda6e75ff6,
    0x3fe6e297396d0918,
    0x3fb279aae6c8f755,
    0x3f93cc4ac6cdaf4b,
    0x3fbe836eb4e98138,
    0x3fee68427418d691,
];
const D65: [u64; 3] = [0x3fee6a22b3892ee8, 0x3ff0000000000000, 0x3ff16b8950763a19];

struct LabTabs {
    gamma: [i32; 256],
    cbrt: Vec<i32>,
    coeffs: [i32; 9],
}

fn lab_tabs() -> &'static LabTabs {
    static T: std::sync::OnceLock<LabTabs> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        let mut gamma = [0i32; 256];
        let int_scale = 255.0f32 * 8.0;
        for (i, g) in gamma.iter_mut().enumerate() {
            let x = (i as f32 / 255.0f32) as f64;
            let v = if x <= 809.0 / 20000.0 {
                x / (323.0 / 25.0)
            } else {
                ((x + 11.0 / 200.0) / (1.0 + 11.0 / 200.0)).powf(12.0 / 5.0)
            };
            *g = cv_round((int_scale * v as f32) as f64);
        }
        let scale = 1.0f32 / (255.0f32 * 8.0);
        let lthresh = 216.0f32 / 24389.0;
        let lscale = 841.0f32 / 108.0;
        let lbias = 16.0f32 / 116.0;
        let cbrt = (0..256 * 3 / 2 * 8)
            .map(|i| {
                let x = scale * i as f32;
                let v = if x < lthresh {
                    x.mul_add(lscale, lbias)
                } else {
                    softfloat_cbrt(x)
                };
                cv_round((32768.0f32 * v) as f64)
            })
            .collect();
        let mut coeffs = [0i32; 9];
        for i in 0..3 {
            let wp = f64::from_bits(D65[i]);
            let c: Vec<f64> = (0..3)
                .map(|j| f64::from_bits(SRGB2XYZ_D65[i * 3 + j]))
                .collect();
            coeffs[i * 3 + 2] = cv_round(4096.0 * c[0] / wp);
            coeffs[i * 3 + 1] = cv_round(4096.0 * c[1] / wp);
            coeffs[i * 3] = cv_round(4096.0 * c[2] / wp);
        }
        LabTabs {
            gamma,
            cbrt,
            coeffs,
        }
    })
}

/// OpenCV's `softfloat cbrt` (`f32_cbrt`, core/src/softfloat.cpp): rational approximation in
/// double, mantissa truncated.
fn softfloat_cbrt(x: f32) -> f32 {
    if x.is_nan() || x.is_infinite() {
        return x;
    }
    let v = x.to_bits();
    let s = v >> 31;
    let mut ex = ((v >> 23) & 0xff) as i32 - 127;
    let mut shx = ex % 3;
    if shx >= 0 {
        shx -= 3;
    }
    ex = (ex - shx) / 3 - 1;
    let fr = f64::from_bits((((shx + 1023) as u64) << 52) | (((v & 0x7f_ffff) as u64) << 29));
    let a = [
        0x4046a09e6653ba70u64,
        0x406808f46c6116e0,
        0x405dca97439cae14,
        0x402add70d2827500,
        0x3fc4f15f83f55d2d,
        0x402d9e20660edb21,
        0x4062ff15c0285815,
        0x406510d06a8112ce,
        0x4040fecbc9e2c375,
    ]
    .map(f64::from_bits);
    let fr = ((((a[0] * fr + a[1]) * fr + a[2]) * fr + a[3]) * fr + a[4])
        / ((((a[5] * fr + a[6]) * fr + a[7]) * fr + a[8]) * fr + 1.0);
    if v & 0x7fff_ffff == 0 {
        return 0.0;
    }
    let frac = ((fr.to_bits() & 0xf_ffff_ffff_ffff) >> 29) as u32;
    f32::from_bits((s << 31) + (((ex + 127) as u32) << 23) + frac)
}

/// `cvtColor(BGR2Lab)` for 8-bit (`RGB2Lab_b`, sRGB).
pub fn bgr2lab(src: &[u8]) -> Vec<u8> {
    let t = lab_tabs();
    let c = &t.coeffs;
    let ds = |x: i32, n: i32| (x + (1 << (n - 1))) >> n;
    let lscale = (116 * 255 + 50) / 100;
    let lshift = -((16 * 255 * (1 << 15) + 50) / 100);
    let mut dst = vec![0u8; src.len()];
    for (s, d) in src
        .as_chunks::<3>()
        .0
        .iter()
        .zip(dst.as_chunks_mut::<3>().0.iter_mut())
    {
        let (r, g, b) = (
            t.gamma[s[0] as usize],
            t.gamma[s[1] as usize],
            t.gamma[s[2] as usize],
        );
        let fx = t.cbrt[ds(r * c[0] + g * c[1] + b * c[2], 12) as usize];
        let fy = t.cbrt[ds(r * c[3] + g * c[4] + b * c[5], 12) as usize];
        let fz = t.cbrt[ds(r * c[6] + g * c[7] + b * c[8], 12) as usize];
        d[0] = sat_u8(ds(lscale * fy + lshift, 15));
        d[1] = sat_u8(ds(500 * (fx - fy) + 128 * (1 << 15), 15));
        d[2] = sat_u8(ds(200 * (fy - fz) + 128 * (1 << 15), 15));
    }
    dst
}

/// `getGaussianKernel(n, sigma, CV_32F)`.
pub fn gaussian_kernel(n: usize, sigma: f64) -> Vec<f32> {
    let scale2x = -0.125 / (sigma * sigma);
    let n2 = (n - 1) / 2;
    let mut vals = vec![0f64; n2];
    let mut sum = 0f64;
    for (i, v) in vals.iter_mut().enumerate() {
        let x = 1 - n as i64 + 2 * i as i64;
        *v = ((x * x) as f64 * scale2x).exp();
        sum += *v;
    }
    sum *= 2.0;
    sum += 1.0;
    let mul1 = 1.0 / sum;
    let mut k = vec![0f32; n];
    for i in 0..n2 {
        let t = (vals[i] * mul1) as f32;
        k[i] = t;
        k[n - 1 - i] = t;
    }
    k[n2] = mul1 as f32;
    k
}

/// Whether OpenCV runs the AVX2 build of its separable filters on this CPU (its run-time
/// dispatch, `filter.dispatch.cpp`). That build is compiled with FMA3 and fuses most
/// multiply-adds; the SSE4.1 and baseline builds round each product first, so the two differ
/// in the last bits of a blurred float image.
pub fn filter_fma() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        std::arch::is_x86_feature_detected!("avx2") && std::arch::is_x86_feature_detected!("fma")
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        false
    }
}

/// The script's `--sigma` default, the only one the shell runs it with, as the `float` that
/// `switchToSelectiveSearchFast` takes.
pub const SIGMA: f32 = 0.6;

/// `GaussianBlur(src32f, dst, Size(0,0), SIGMA)` for float images, `BORDER_REFLECT_101`, as
/// OpenCV's AVX2 build (`fma`) or its other x86-64 builds compute it, bit for bit. Nothing
/// else is ported: this sigma gives a kernel of 7 taps, and OpenCV filters kernels of 5 and
/// fewer with other code.
pub fn gaussian_blur_f32(src: &[f32], w: usize, h: usize, cn: usize, fma: bool) -> Vec<f32> {
    if !fma {
        return blur(src, w, h, cn, |a, b, c| a * b + c);
    }
    #[cfg(target_arch = "x86_64")]
    if filter_fma() {
        // SAFETY: `filter_fma` has just found AVX2 and FMA3 on this CPU.
        return unsafe { blur_fma(src, w, h, cn) };
    }
    blur(src, w, h, cn, f32::mul_add)
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
fn blur_fma(src: &[f32], w: usize, h: usize, cn: usize) -> Vec<f32> {
    blur(src, w, h, cn, |a, b, c| a.mul_add(b, c))
}

/// `sepFilter2D` with the Gaussian kernel: `RowFilter<float, float, RowVec_32f>` then
/// `SymmColumnFilter` with `SymmColumnVec_32f`. `mul_add(a, b, c)` is `a * b + c` as the
/// vector loops of the build compute it. The scalar loops that finish a row are the same in
/// every build, except that the AVX2 one's compiler fused some of their multiply-adds.
#[inline(always)]
fn blur(
    src: &[f32],
    w: usize,
    h: usize,
    cn: usize,
    mul_add: impl Fn(f32, f32, f32) -> f32,
) -> Vec<f32> {
    let sigma = SIGMA as f64;
    let n = (cv_round(sigma * 8.0 + 1.0) | 1) as usize;
    let rw = w * cn;

    // A one-pixel-wide image gets a one-tap kernel.
    let k = gaussian_kernel(if w == 1 { 1 } else { n }, sigma);
    let r = (k.len() / 2) as isize;
    // `RowVec_32f` takes 8 floats at a time and the unrolled loop after it 4, both fused in
    // the AVX2 build. In the last `rw % 4` floats that build multiplies the taps four at a
    // time, unfused, and fuses the ones left over.
    let vec_end = rw / 4 * 4;
    let unfused = (k.len() - 1) / 4 * 4;
    let mut rows = vec![0f32; rw * h];
    let xmap: Vec<usize> = (-r..w as isize + r).map(|x| reflect101(x, w)).collect();
    for y in 0..h {
        let s = &src[y * rw..(y + 1) * rw];
        let d = &mut rows[y * rw..(y + 1) * rw];
        for (i, d) in d.iter_mut().enumerate() {
            let (x, c) = (i / cn, i % cn);
            let mut acc = 0f32;
            for (j, &kj) in k.iter().enumerate() {
                let v = s[xmap[x + j] * cn + c];
                acc = if i >= vec_end && j <= unfused {
                    acc + v * kj
                } else {
                    mul_add(v, kj, acc)
                };
            }
            *d = acc;
        }
    }

    // A one-pixel-high image likewise.
    let k = gaussian_kernel(if h == 1 { 1 } else { n }, sigma);
    let r = k.len() / 2;
    // `SymmColumnVec_32f` takes 8 floats at a time; the scalar loop after it is unfused in
    // every build.
    let vec_end = rw / 8 * 8;
    let ymap: Vec<usize> = (-(r as isize)..(h + r) as isize)
        .map(|y| reflect101(y, h))
        .collect();
    let mut dst = vec![0f32; rw * h];
    for y in 0..h {
        let d = &mut dst[y * rw..(y + 1) * rw];
        let c0 = &rows[ymap[y + r] * rw..];
        for i in 0..rw {
            d[i] = c0[i] * k[r] + 0.0;
        }
        for j in 1..=r {
            let a = &rows[ymap[y + r + j] * rw..];
            let b = &rows[ymap[y + r - j] * rw..];
            let kj = k[r + j];
            for i in 0..vec_end {
                d[i] = mul_add(a[i] + b[i], kj, d[i]);
            }
            for i in vec_end..rw {
                d[i] += (a[i] + b[i]) * kj;
            }
        }
    }
    dst
}

/// `Scharr(src8u, dst, CV_32F, dx, dy)` with `BORDER_REFLECT_101`; exact integers.
pub fn scharr(src: &[u8], w: usize, h: usize, dx: bool) -> Vec<f32> {
    let (kx, ky): ([i32; 3], [i32; 3]) = if dx {
        ([-1, 0, 1], [3, 10, 3])
    } else {
        ([3, 10, 3], [-1, 0, 1])
    };
    let mut out = vec![0f32; w * h];
    for y in 0..h {
        let ys = [
            reflect101(y as isize - 1, h),
            y,
            reflect101(y as isize + 1, h),
        ];
        for x in 0..w {
            let xs = [
                reflect101(x as isize - 1, w),
                x,
                reflect101(x as isize + 1, w),
            ];
            let mut s = 0i32;
            for (a, &yy) in ys.iter().enumerate() {
                let mut rs = 0i32;
                for (b, &xx) in xs.iter().enumerate() {
                    rs += kx[b] * src[yy * w + xx] as i32;
                }
                s += ky[a] * rs;
            }
            out[y * w + x] = s as f32;
        }
    }
    out
}

/// `getRotationMatrix2D(center, angle, 1.0)`.
pub fn rotation_matrix(cx: f64, cy: f64, angle: f64) -> [f64; 6] {
    let a = angle * std::f64::consts::PI / 180.0;
    let alpha = a.cos();
    let beta = a.sin();
    [
        alpha,
        beta,
        (1.0 - alpha) * cx - beta * cy,
        -beta,
        alpha,
        beta * cx + (1.0 - alpha) * cy,
    ]
}

/// `RotatedRect(center, size, angle).boundingRect()` -> (x, y, w, h).
pub fn rotated_rect_bbox(cx: f32, cy: f32, sw: f32, sh: f32, angle: f64) -> (i32, i32, i32, i32) {
    let a_rad = angle * std::f64::consts::PI / 180.0;
    let b = a_rad.cos() as f32 * 0.5;
    let a = a_rad.sin() as f32 * 0.5;
    let p0 = (cx - a * sh - b * sw, cy + b * sh - a * sw);
    let p1 = (cx + a * sh - b * sw, cy - b * sh - a * sw);
    let p2 = (2.0 * cx - p0.0, 2.0 * cy - p0.1);
    let p3 = (2.0 * cx - p1.0, 2.0 * cy - p1.1);
    let minx = p0.0.min(p1.0).min(p2.0).min(p3.0);
    let miny = p0.1.min(p1.1).min(p2.1).min(p3.1);
    let maxx = p0.0.max(p1.0).max(p2.0).max(p3.0);
    let maxy = p0.1.max(p1.1).max(p2.1).max(p3.1);
    let x = minx.floor() as i32;
    let y = miny.floor() as i32;
    (x, y, maxx.ceil() as i32 - x + 1, maxy.ceil() as i32 - y + 1)
}

pub trait WarpPx: Copy + Default {
    fn interp(v: [Self; 4], wi: [i16; 4], wf: [f32; 4]) -> Self;
}

impl WarpPx for u8 {
    fn interp(v: [u8; 4], w: [i16; 4], _: [f32; 4]) -> u8 {
        let s = v[0] as i32 * w[0] as i32
            + v[1] as i32 * w[1] as i32
            + v[2] as i32 * w[2] as i32
            + v[3] as i32 * w[3] as i32;
        sat_u8((s + (1 << 14)) >> 15)
    }
}

impl WarpPx for f32 {
    fn interp(v: [f32; 4], _: [i16; 4], w: [f32; 4]) -> f32 {
        v[0] * w[0] + v[1] * w[1] + v[2] * w[2] + v[3] * w[3]
    }
}

/// `warpAffine(src, dst, M, (dw, dh))`: INTER_LINEAR, BORDER_CONSTANT 0, single channel.
pub fn warp_affine<T: WarpPx>(
    src: &[T],
    w: usize,
    h: usize,
    m: &[f64; 6],
    dw: usize,
    dh: usize,
) -> Vec<T> {
    let mut im = *m;
    let d = im[0] * im[4] - im[1] * im[3];
    let d = if d != 0.0 { 1.0 / d } else { 0.0 };
    let a11 = im[4] * d;
    let a22 = im[0] * d;
    im[0] = a11;
    im[1] *= -d;
    im[3] *= -d;
    im[4] = a22;
    let b1 = -im[0] * im[2] - im[1] * im[5];
    let b2 = -im[3] * im[2] - im[4] * im[5];
    im[2] = b1;
    im[5] = b2;
    const AB: f64 = 1024.0;
    let adelta: Vec<i32> = (0..dw).map(|x| cv_round(im[0] * x as f64 * AB)).collect();
    let bdelta: Vec<i32> = (0..dw).map(|x| cv_round(im[3] * x as f64 * AB)).collect();
    let mut dst = vec![T::default(); dw * dh];
    let (wi, hi) = (w as i32, h as i32);
    for y in 0..dh {
        let x0 = cv_round((im[1] * y as f64 + im[2]) * AB) + 16;
        let y0 = cv_round((im[4] * y as f64 + im[5]) * AB) + 16;
        for x in 0..dw {
            let xx = (x0 + adelta[x]) >> 5;
            let yy = (y0 + bdelta[x]) >> 5;
            let sx = (xx >> 5).clamp(i16::MIN as i32, i16::MAX as i32);
            let sy = (yy >> 5).clamp(i16::MIN as i32, i16::MAX as i32);
            let fx = (xx & 31) as f32 * (1.0 / 32.0);
            let fy = (yy & 31) as f32 * (1.0 / 32.0);
            let tx = [1.0 - fx, fx];
            let ty = [1.0 - fy, fy];
            let wf = [ty[0] * tx[0], ty[0] * tx[1], ty[1] * tx[0], ty[1] * tx[1]];
            let wq = wf.map(|v| cv_round((v * 32768.0) as f64).clamp(-32768, 32767) as i16);
            if sx >= wi || sx + 1 < 0 || sy >= hi || sy + 1 < 0 {
                continue;
            }
            let at = |px: i32, py: i32| {
                if px >= 0 && px < wi && py >= 0 && py < hi {
                    src[py as usize * w + px as usize]
                } else {
                    T::default()
                }
            };
            let v = [
                at(sx, sy),
                at(sx + 1, sy),
                at(sx, sy + 1),
                at(sx + 1, sy + 1),
            ];
            dst[y * dw + x] = T::interp(v, wq, wf);
        }
    }
    dst
}

/// `src.convertTo(dst, CV_8U, alpha, beta)` for float input.
pub fn convert_scale_u8(src: &[f32], alpha: f64, beta: f64) -> Vec<u8> {
    let (a, b) = (alpha as f32, beta as f32);
    src.iter().map(|&v| sat_u8_f32(v * a + b)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `s`x`s` block of channel `c` holds `q` and `q + 1` in equal numbers,
    /// `q = 5 * bx + 3 * by + c`: its mean is `q + 0.5`.
    fn halves(w: usize, h: usize, s: usize) -> Vec<u8> {
        let mut px = Vec::with_capacity(w * h * 3);
        for y in 0..h {
            for x in 0..w {
                for c in 0..3 {
                    px.push((5 * (x / s) + 3 * (y / s) + c + x % 2) as u8);
                }
            }
        }
        px
    }

    fn ramp(w: usize, h: usize) -> Vec<f32> {
        let mut px = Vec::with_capacity(w * h * 3);
        for y in 0..h {
            for x in 0..w {
                for c in 0..3 {
                    px.push(((x * 37 + y * 91 + c * 53 + x * y * 7) % 256) as f32);
                }
            }
        }
        px
    }

    fn ramp_u8(w: usize, h: usize) -> Vec<u8> {
        ramp(w, h).iter().map(|&v| v as u8).collect()
    }

    // cv2.resize(img, (int(w * 0.1), int(h * 0.1)), interpolation=cv2.INTER_AREA), OpenCV 4.13.0.
    #[test]
    fn area_by_exactly_ten_rounds_a_half_to_even() {
        assert_eq!(
            resize_area_tenth(&halves(40, 20, 10), 40, 20).unwrap(),
            (
                vec![
                    0, 2, 2, 6, 6, 8, 10, 12, 12, 16, 16, 18, 4, 4, 6, 8, 10, 10, 14, 14, 16, 18,
                    20, 20
                ],
                4,
                2
            )
        );
    }

    #[test]
    fn area_by_a_fraction_matches_opencv() {
        assert_eq!(
            resize_area_tenth(&ramp_u8(25, 13), 25, 13).unwrap(),
            (vec![127, 128, 132, 127, 129, 130], 2, 1)
        );
        assert_eq!(
            resize_area_tenth(&ramp_u8(37, 21), 37, 21).unwrap(),
            (
                vec![
                    127, 129, 131, 126, 126, 133, 130, 128, 130, 121, 134, 124, 122, 132, 119, 125,
                    129, 121
                ],
                3,
                2
            )
        );
    }

    #[test]
    fn area_to_an_empty_size_is_none() {
        assert_eq!(resize_area_tenth(&ramp_u8(9, 30), 9, 30), None);
        assert_eq!(resize_area_tenth(&ramp_u8(30, 9), 30, 9), None);
    }

    fn bits(v: &[f32]) -> Vec<u32> {
        v.iter().map(|f| f.to_bits()).collect()
    }

    // cv2.GaussianBlur(ramp(5, 3), (0, 0), s, s).view(np.uint32) of OpenCV 4.13.0, s being
    // float(np.float32(0.6)): as it runs on a CPU with AVX2, and with OPENCV_CPU_DISABLE=AVX2.
    // A row is 15 floats: 8 for the filters' vector loop, 4 for the unrolled scalar loop, 3
    // for the last one.
    const BLUR_AVX2: [u32; 45] = [
        0x4231fdb4, 0x42c21fef, 0x4314c153, 0x428cdc77, 0x42f4acd1, 0x4321177f, 0x42d90e3a,
        0x43134834, 0x4310054e, 0x4305afb8, 0x43023525, 0x43292de9, 0x42eb3359, 0x430e8b50,
        0x43431bf6, 0x42d399f5, 0x4310c596, 0x4328c38e, 0x43001a56, 0x43186cf8, 0x432a10b0,
        0x430f4f07, 0x4320f2b6, 0x42c85242, 0x4318296d, 0x42b5deac, 0x42e72d3b, 0x42bfdb54,
        0x42b878e3, 0x43105b6e, 0x4326a8c4, 0x432366d8, 0x42ce520c, 0x432b3c32, 0x42dd8cd5,
        0x42f33c8d, 0x42d5252d, 0x42ead4ad, 0x42e37152, 0x42e1fd14, 0x42da2a5f, 0x43140dd9,
        0x42cd31ce, 0x42ff14a9, 0x43341af8,
    ];
    const BLUR_NO_AVX2: [u32; 45] = [
        0x4231fdb5, 0x42c21fef, 0x4314c154, 0x428cdc77, 0x42f4acd1, 0x4321177f, 0x42d90e3a,
        0x43134833, 0x4310054e, 0x4305afb8, 0x43023525, 0x43292de9, 0x42eb3359, 0x430e8b50,
        0x43431bf6, 0x42d399f5, 0x4310c596, 0x4328c38e, 0x43001a55, 0x43186cf8, 0x432a10b0,
        0x430f4f06, 0x4320f2b6, 0x42c85241, 0x4318296d, 0x42b5deac, 0x42e72d3b, 0x42bfdb54,
        0x42b878e3, 0x43105b6e, 0x4326a8c4, 0x432366d8, 0x42ce520c, 0x432b3c32, 0x42dd8cd6,
        0x42f33c8d, 0x42d5252c, 0x42ead4ad, 0x42e37151, 0x42e1fd14, 0x42da2a5f, 0x43140dd9,
        0x42cd31ce, 0x42ff14a9, 0x43341af8,
    ];

    #[test]
    fn blur_matches_both_opencv_builds_bit_for_bit() {
        let src = ramp(5, 3);
        assert_eq!(bits(&gaussian_blur_f32(&src, 5, 3, 3, true)), BLUR_AVX2);
        assert_eq!(bits(&gaussian_blur_f32(&src, 5, 3, 3, false)), BLUR_NO_AVX2);
        assert_ne!(BLUR_AVX2, BLUR_NO_AVX2);
    }

    #[test]
    fn blur_of_a_one_pixel_side_filters_the_other_only() {
        // The same call on 1x3 and 3x1 images (OPENCV_CPU_DISABLE=AVX2): OpenCV gives the
        // one-pixel side a one-tap kernel.
        assert_eq!(
            bits(&gaussian_blur_f32(&ramp(1, 3), 1, 3, 3, false)),
            [
                0x41f87aa8, 0x42a81eaa, 0x4307bef8, 0x42b5ffff, 0x43100000, 0x431aa012, 0x4316f0ab,
                0x434bf0ab, 0x42ae01c6
            ]
        );
        assert_eq!(
            bits(&gaussian_blur_f32(&ramp(3, 1), 3, 1, 3, false)),
            [
                0x414a0f58, 0x428341eb, 0x42ed41ec, 0x42140000, 0x42b40000, 0x430f0000, 0x42757c29,
                0x42e4be14, 0x43275f09
            ]
        );
    }
}
