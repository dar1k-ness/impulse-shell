//! OKLab (Björn Ottosson's definition) and k-means palettes.

use image::{DynamicImage, imageops::FilterType};
use rand::{Rng, SeedableRng, rngs::StdRng};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Lab {
    pub l: f32,
    pub a: f32,
    pub b: f32,
}

fn to_linear(c: u8) -> f32 {
    let c = f32::from(c) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn to_srgb(c: f32) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let v = if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (v * 255.0).round() as u8
}

impl Lab {
    pub(crate) fn from_rgb([r, g, b]: [u8; 3]) -> Self {
        let (r, g, b) = (to_linear(r), to_linear(g), to_linear(b));
        let l = (0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
        let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
        let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
        Self {
            l: 0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
            a: 1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
            b: 0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
        }
    }

    pub(crate) fn to_rgb(self) -> [u8; 3] {
        let l = (self.l + 0.396_337_78 * self.a + 0.215_803_76 * self.b).powi(3);
        let m = (self.l - 0.105_561_346 * self.a - 0.063_854_17 * self.b).powi(3);
        let s = (self.l - 0.089_484_18 * self.a - 1.291_485_5 * self.b).powi(3);
        [
            to_srgb(4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s),
            to_srgb(-1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s),
            to_srgb(-0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s),
        ]
    }

    pub(crate) fn chroma(self) -> f32 {
        self.a.hypot(self.b)
    }

    pub(crate) fn dist2(self, o: Self) -> f32 {
        (self.l - o.l).powi(2) + (self.a - o.a).powi(2) + (self.b - o.b).powi(2)
    }

    pub(crate) fn hex(self) -> String {
        let [r, g, b] = self.to_rgb();
        format!("#{r:02x}{g:02x}{b:02x}")
    }

    /// `#rrggbb` → Lab.
    pub(crate) fn from_hex(s: &str) -> Option<Self> {
        let h = s.strip_prefix('#')?;
        if h.len() != 6 || !h.is_ascii() {
            return None;
        }
        let c = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
        Some(Self::from_rgb([c(0)?, c(2)?, c(4)?]))
    }
}

pub(crate) fn lightness(p: &image::Rgb<u8>) -> f32 {
    Lab::from_rgb(p.0).l
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Swatch {
    pub lab: Lab,
    /// Fraction of the whole image.
    pub share: f32,
}

fn pixels(img: &DynamicImage) -> Vec<Lab> {
    img.resize(96, 96, FilterType::Triangle)
        .to_rgb8()
        .pixels()
        .map(|p| Lab::from_rgb(p.0))
        .collect()
}

/// Dominant colours (k-means++ in OKLab, fixed seed), largest share first.
pub(crate) fn palette(img: &DynamicImage, k: usize) -> Vec<Swatch> {
    let px = pixels(img);
    let n = px.len();
    cluster(&px, k, n)
}

/// Clusters only saturated pixels, so small vivid details (ribbons, eyes, outlines) are not
/// swallowed by large pastel areas; shares stay relative to the whole image.
pub(crate) fn vivid(img: &DynamicImage, k: usize) -> Vec<Swatch> {
    let px = pixels(img);
    let n = px.len();
    let sat: Vec<Lab> = px.into_iter().filter(|p| p.chroma() >= 0.09).collect();
    cluster(&sat, k, n)
}

fn nearest(p: Lab, cent: &[Lab]) -> usize {
    let mut best = (f32::MAX, 0);
    for (i, c) in cent.iter().enumerate() {
        let d = p.dist2(*c);
        if d < best.0 {
            best = (d, i);
        }
    }
    best.1
}

fn cluster(px: &[Lab], k: usize, total: usize) -> Vec<Swatch> {
    if px.is_empty() || k == 0 {
        return Vec::new();
    }
    let mut rng = StdRng::seed_from_u64(7);
    let mut cent = vec![px[rng.random_range(0..px.len())]];
    while cent.len() < k {
        let d: Vec<f32> = px
            .iter()
            .map(|p| p.dist2(cent[nearest(*p, &cent)]))
            .collect();
        let sum: f32 = d.iter().sum();
        if sum <= 0.0 {
            break;
        }
        let mut t = rng.random::<f32>() * sum;
        let idx = d
            .iter()
            .position(|v| {
                t -= v;
                t <= 0.0
            })
            .unwrap_or(d.len() - 1);
        cent.push(px[idx]);
    }
    let mut assign = vec![0usize; px.len()];
    for _ in 0..12 {
        for (a, p) in assign.iter_mut().zip(px) {
            *a = nearest(*p, &cent);
        }
        let mut sum = vec![(0f32, 0f32, 0f32, 0usize); cent.len()];
        for (a, p) in assign.iter().zip(px) {
            let s = &mut sum[*a];
            s.0 += p.l;
            s.1 += p.a;
            s.2 += p.b;
            s.3 += 1;
        }
        for (c, s) in cent.iter_mut().zip(&sum) {
            if s.3 > 0 {
                let n = s.3 as f32;
                *c = Lab {
                    l: s.0 / n,
                    a: s.1 / n,
                    b: s.2 / n,
                };
            }
        }
    }
    let mut counts = vec![0usize; cent.len()];
    for a in &assign {
        counts[*a] += 1;
    }
    let mut out: Vec<Swatch> = cent
        .into_iter()
        .zip(counts)
        .filter(|(_, n)| *n > 0)
        .map(|(lab, n)| Swatch {
            lab,
            share: n as f32 / total.max(1) as f32,
        })
        .collect();
    out.sort_by(|a, b| b.share.total_cmp(&a.share));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};

    #[test]
    fn oklab_reference_values() {
        // Ottosson's published values: white L=1, a=b=0; sRGB red ≈ (0.628, 0.225, 0.126)
        let w = Lab::from_rgb([255, 255, 255]);
        assert!(
            (w.l - 1.0).abs() < 1e-3 && w.a.abs() < 1e-3 && w.b.abs() < 1e-3,
            "{w:?}"
        );
        let r = Lab::from_rgb([255, 0, 0]);
        assert!(
            (r.l - 0.628).abs() < 2e-3
                && (r.a - 0.2249).abs() < 2e-3
                && (r.b - 0.1258).abs() < 2e-3,
            "{r:?}"
        );
        assert!(Lab::from_rgb([0, 0, 0]).l.abs() < 1e-6);
    }

    #[test]
    fn rgb_and_hex_round_trip() {
        for c in [
            [0, 0, 0],
            [255, 255, 255],
            [18, 52, 86],
            [250, 128, 114],
            [0, 200, 255],
        ] {
            assert_eq!(Lab::from_rgb(c).to_rgb(), c);
        }
        assert_eq!(Lab::from_hex("#123456").unwrap().hex(), "#123456");
        assert!(Lab::from_hex("123456").is_none() && Lab::from_hex("#12345").is_none());
    }

    #[test]
    fn palette_finds_areas_and_vivid_finds_small_details() {
        let mut img = RgbImage::from_pixel(100, 100, Rgb([200, 190, 185]));
        for y in 0..100 {
            for x in 0..30 {
                img.put_pixel(x, y, Rgb([20, 30, 60]));
            }
        }
        for y in 45..50 {
            for x in 60..65 {
                img.put_pixel(x, y, Rgb([230, 20, 120]));
            }
        }
        let img = DynamicImage::ImageRgb8(img);
        let pal = palette(&img, 4);
        assert!((pal[0].share - 0.6975).abs() < 0.03, "{pal:?}");
        assert!((pal[1].share - 0.30).abs() < 0.03, "{pal:?}");
        let v = vivid(&img, 3);
        // only the small magenta patch (and its resampled edge) is saturated
        assert!(
            !v.is_empty() && v.iter().map(|s| s.share).sum::<f32>() < 0.01,
            "{v:?}"
        );
        assert!(v[0].lab.chroma() > 0.15 && v[0].lab.a > 0.1, "{v:?}");
        // deterministic
        let again = palette(&img, 4);
        assert_eq!(
            pal.iter().map(|s| s.lab).collect::<Vec<_>>(),
            again.iter().map(|s| s.lab).collect::<Vec<_>>()
        );
    }
}
