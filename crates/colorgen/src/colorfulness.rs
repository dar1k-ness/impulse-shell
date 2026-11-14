//! `scheme_for_image.py`: OpenCV decode + `INTER_AREA` downscale to 128 px, then Hasler and
//! Süsstrunk's colorfulness with numpy's summation order.

use crate::seed::SeedError;

/// BGR pixels as `cv2.imread(path)` (IMREAD_COLOR) returns them.
pub struct Bgr {
    pub data: Vec<u8>,
    pub width: usize,
    pub height: usize,
}

fn err(e: impl std::fmt::Display) -> SeedError {
    SeedError(e.to_string())
}

/// `cv2.imread(path)`: the pixels in BGR, 16-bit samples brought to 8 bits and the image
/// turned by its EXIF orientation (JPEG, PNG, WebP) or TIFF Orientation tag as OpenCV does.
/// OpenCV 4.13's `imread` refuses a TIFF whose Orientation swaps the sides (its `imdecode`
/// turns it); such a file is read here.
pub fn imread(bytes: &[u8]) -> Result<Bgr, SeedError> {
    let stored = if heif::is_heif(bytes) {
        imgtools::cv::heif_bgr(bytes).map_err(err)?
    } else if bytes.starts_with(&[0xFF, 0xD8]) {
        let img = imgtools::cv::jpeg(bytes, turbojpeg::PixelFormat::BGR).map_err(err)?;
        let mut px = Vec::with_capacity(img.width * img.height * 3);
        for row in img.pixels.chunks(img.pitch) {
            px.extend_from_slice(&row[..img.width * 3]);
        }
        imgtools::cv::Bgr {
            w: img.width,
            h: img.height,
            px,
        }
    } else if bytes.starts_with(b"GIF8") {
        let frame = crate::seed::gif_frames(bytes)?
            .next()
            .ok_or_else(|| err("empty GIF"))?
            .map_err(err)?;
        let buf = frame.into_buffer();
        let (w, h) = (buf.width() as usize, buf.height() as usize);
        let px = buf
            .into_raw()
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[2], p[1], p[0]])
            .collect();
        imgtools::cv::Bgr { w, h, px }
    } else {
        let img = imgtools::limit::image(bytes).map_err(err)?;
        imgtools::cv::image_bgr(bytes, img)
    };
    let (width, height, data) = imgtools::cv::orient(
        stored.w,
        stored.h,
        3,
        stored.px,
        imgtools::cv::orientation(bytes),
    );
    Ok(Bgr {
        data,
        width,
        height,
    })
}

/// `cvRound` + `saturate_cast<uchar>` of a float.
fn sat_u8(v: f32) -> u8 {
    v.round_ties_even().clamp(0.0, 255.0) as u8
}

struct Tab {
    di: usize,
    si: usize,
    alpha: f32,
}

fn area_tab(ssize: usize, dsize: usize, scale: f64) -> Vec<Tab> {
    let mut tab = Vec::new();
    for dx in 0..dsize {
        let fsx1 = dx as f64 * scale;
        let fsx2 = fsx1 + scale;
        let cell = scale.min(ssize as f64 - fsx1);
        let mut sx2 = fsx2.floor() as isize;
        sx2 = sx2.min(ssize as isize - 1);
        let sx1 = (fsx1.ceil() as isize).min(sx2);
        if sx1 as f64 - fsx1 > 1e-3 {
            tab.push(Tab {
                di: dx,
                si: (sx1 - 1) as usize,
                alpha: ((sx1 as f64 - fsx1) / cell) as f32,
            });
        }
        for sx in sx1..sx2 {
            tab.push(Tab {
                di: dx,
                si: sx as usize,
                alpha: (1.0 / cell) as f32,
            });
        }
        if fsx2 - sx2 as f64 > 1e-3 {
            tab.push(Tab {
                di: dx,
                si: sx2 as usize,
                alpha: ((fsx2 - sx2 as f64).min(1.0).min(cell) / cell) as f32,
            });
        }
    }
    tab
}

/// `cv2.resize(img, (dw, dh), interpolation=cv2.INTER_AREA)` for a downscale of 3-channel u8.
pub fn resize_area(src: &Bgr, dw: usize, dh: usize) -> Bgr {
    const CN: usize = 3;
    let (sw, sh) = (src.width, src.height);
    let scale_x = 1.0 / (dw as f64 / sw as f64);
    let scale_y = 1.0 / (dh as f64 / sh as f64);
    let (ix, iy) = (scale_x.round() as usize, scale_y.round() as usize);
    let mut out = vec![0u8; dw * dh * CN];
    if (scale_x - ix as f64).abs() < f64::EPSILON && (scale_y - iy as f64).abs() < f64::EPSILON {
        let area = ix * iy;
        let scale = 1.0f32 / area as f32;
        // OpenCV averages a 2x2 block in integers, `(sum + 2) >> 2`: a half rounds up there,
        // and to even through `saturate_cast` for every other block.
        let half = ix == 2 && iy == 2;
        for dy in 0..dh {
            for dx in 0..dw {
                for c in 0..CN {
                    let mut sum: i32 = 0;
                    for y in dy * iy..dy * iy + iy {
                        let row = &src.data[y * sw * CN..];
                        for x in dx * ix..dx * ix + ix {
                            sum += row[x * CN + c] as i32;
                        }
                    }
                    out[(dy * dw + dx) * CN + c] = if half {
                        ((sum + 2) >> 2) as u8
                    } else {
                        sat_u8(sum as f32 * scale)
                    };
                }
            }
        }
        return Bgr {
            data: out,
            width: dw,
            height: dh,
        };
    }
    let xtab = area_tab(sw, dw, scale_x);
    let ytab = area_tab(sh, dh, scale_y);
    let mut buf = vec![0f32; dw * CN];
    let mut sum = vec![0f32; dw * CN];
    let mut prev_dy = ytab[0].di;
    for t in &ytab {
        let s = &src.data[t.si * sw * CN..(t.si + 1) * sw * CN];
        buf.iter_mut().for_each(|b| *b = 0.0);
        for x in &xtab {
            for c in 0..CN {
                let i = x.di * CN + c;
                buf[i] += s[x.si * CN + c] as f32 * x.alpha;
            }
        }
        if t.di != prev_dy {
            for (i, v) in sum.iter_mut().enumerate() {
                out[prev_dy * dw * CN + i] = sat_u8(*v);
                *v = t.alpha * buf[i];
            }
            prev_dy = t.di;
        } else {
            for (v, b) in sum.iter_mut().zip(&buf) {
                *v += t.alpha * b;
            }
        }
    }
    for (i, v) in sum.iter().enumerate() {
        out[prev_dy * dw * CN + i] = sat_u8(*v);
    }
    Bgr {
        data: out,
        width: dw,
        height: dh,
    }
}

/// `load_and_resize_image(path, 128)`.
pub fn load_and_resize(bytes: &[u8]) -> Result<Bgr, SeedError> {
    let img = imread(bytes)?;
    let m = img.width.max(img.height);
    if m <= 128 {
        return Ok(img);
    }
    let scale = 128.0 / m as f64;
    let (dw, dh) = (
        (img.width as f64 * scale) as usize,
        (img.height as f64 * scale) as usize,
    );
    if dw == 0 || dh == 0 {
        // A strip over 128 times longer than wide: `cv2.resize` raises on the empty size.
        return Err(err("the image scales to an empty size"));
    }
    Ok(resize_area(&img, dw, dh))
}

/// numpy's `pairwise_sum` for a contiguous float64 array.
fn pairwise(a: &[f64]) -> f64 {
    let n = a.len();
    if n < 8 {
        let mut res = 0.0;
        for v in a {
            res += v;
        }
        res
    } else if n <= 128 {
        let mut r = [0.0f64; 8];
        r.copy_from_slice(&a[..8]);
        let mut i = 8;
        while i < n - n % 8 {
            for k in 0..8 {
                r[k] += a[i + k];
            }
            i += 8;
        }
        let mut res = ((r[0] + r[1]) + (r[2] + r[3])) + ((r[4] + r[5]) + (r[6] + r[7]));
        while i < n {
            res += a[i];
            i += 1;
        }
        res
    } else {
        let mut n2 = n / 2;
        n2 -= n2 % 8;
        pairwise(&a[..n2]) + pairwise(&a[n2..])
    }
}

/// `np.add.reduce` over a contiguous float64 array, whatever its shape: one pairwise sum of
/// all its elements.
fn np_sum(a: &[f64]) -> f64 {
    pairwise(a)
}

fn mean(a: &[f64]) -> f64 {
    np_sum(a) / a.len() as f64
}

fn std(a: &[f64]) -> f64 {
    let m = mean(a);
    let sq: Vec<f64> = a.iter().map(|v| (v - m) * (v - m)).collect();
    (np_sum(&sq) / a.len() as f64).sqrt()
}

pub fn colorfulness(img: &Bgr) -> f64 {
    let (mut rg, mut yb) = (
        Vec::with_capacity(img.data.len() / 3),
        Vec::with_capacity(img.data.len() / 3),
    );
    for p in img.data.as_chunks::<3>().0 {
        let (b, g, r) = (p[0] as f64, p[1] as f64, p[2] as f64);
        rg.push((r - g).abs());
        yb.push((0.5 * (r + g) - b).abs());
    }
    let (srg, syb, mrg, myb) = (std(&rg), std(&yb), mean(&rg), mean(&yb));
    (srg * srg + syb * syb).sqrt() + 0.3 * (mrg * mrg + myb * myb).sqrt()
}

/// `pick_scheme(colorfulness)`; any failure prints `scheme-tonal-spot` in the script.
pub fn scheme_for_image(path: &std::path::Path) -> &'static str {
    match std::fs::read(path)
        .map_err(err)
        .and_then(|b| load_and_resize(&b))
    {
        Ok(img) if colorfulness(&img) < 40.0 => "scheme-neutral",
        _ => "scheme-tonal-spot",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `s`x`s` block of channel `c` holds `q` and `q + 1` in equal numbers,
    /// `q = 5 * bx + 3 * by + c`: its mean is `q + 0.5`.
    fn halves(w: usize, h: usize, s: usize) -> Bgr {
        let mut data = Vec::with_capacity(w * h * 3);
        for y in 0..h {
            for x in 0..w {
                for c in 0..3 {
                    data.push((5 * (x / s) + 3 * (y / s) + c + x % 2) as u8);
                }
            }
        }
        Bgr {
            data,
            width: w,
            height: h,
        }
    }

    #[test]
    fn sums_over_8192_pixels_add_in_numpy_order() {
        // image_colorfulness() of scheme_for_image.py (numpy 2.3.5) on this 128x72 image: its
        // 9216 values are one pairwise sum, where 8192-element pieces give ...327.
        let (w, h) = (128, 72);
        let mut data = Vec::with_capacity(w * h * 3);
        for y in 0..h {
            for x in 0..w {
                for c in 0..3 {
                    data.push(((x * x * 6 + y * 91 + c * 53 * (x + 1) + x * y * 7) % 256) as u8);
                }
            }
        }
        let img = Bgr {
            data,
            width: w,
            height: h,
        };
        assert_eq!(colorfulness(&img), 113.84146954965328);
    }

    #[test]
    fn a_strip_that_scales_to_nothing_is_an_error() {
        // 1000x1, 1x1000 and 300x2 scale to a zero side, where the script's `cv2.resize`
        // raises; 256x2 is the thinnest that still has a row.
        for (w, h, ok) in [
            (1000, 1, false),
            (1, 1000, false),
            (300, 2, false),
            (256, 2, true),
        ] {
            let mut png = Vec::new();
            image::RgbImage::from_pixel(w, h, image::Rgb([10, 200, 30]))
                .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
                .unwrap();
            assert_eq!(load_and_resize(&png).is_ok(), ok, "{w}x{h}");
        }
    }

    #[test]
    fn area_halving_rounds_a_half_up() {
        // cv2.resize(halves(8, 4, 2), (4, 2), interpolation=cv2.INTER_AREA), OpenCV 4.13.0:
        // what a wallpaper whose long side is 256 goes through.
        assert_eq!(
            resize_area(&halves(8, 4, 2), 4, 2).data,
            [
                1, 2, 3, 6, 7, 8, 11, 12, 13, 16, 17, 18, 4, 5, 6, 9, 10, 11, 14, 15, 16, 19, 20,
                21
            ]
        );
    }

    #[test]
    fn area_quartering_rounds_a_half_to_even() {
        // cv2.resize(halves(16, 8, 4), (4, 2), interpolation=cv2.INTER_AREA), OpenCV 4.13.0.
        assert_eq!(
            resize_area(&halves(16, 8, 4), 4, 2).data,
            [
                0, 2, 2, 6, 6, 8, 10, 12, 12, 16, 16, 18, 4, 4, 6, 8, 10, 10, 14, 14, 16, 18, 20,
                20
            ]
        );
    }
}
