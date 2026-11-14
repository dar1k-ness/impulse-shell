//! Where the desktop clock goes: the calmest spot of the wallpaper as shown on screen, away from
//! the bar, the edges and the subject.

use crate::color::lightness;
use image::{DynamicImage, imageops::FilterType};

/// Grid cell in logical px.
const CELL: u32 = 20;
/// Kept free under the top bar and at the other edges, in cells.
const TOP: usize = 5;
const MARGIN: usize = 3;
pub(crate) const GRID: (usize, usize) = (4, 3);

/// Edge density per cell of the cover-cropped wallpaper, as an integral image.
pub(crate) struct Busy {
    w: usize,
    h: usize,
    sum: Vec<f32>,
}

impl Busy {
    pub(crate) fn new(img: &DynamicImage, screen: (u32, u32)) -> Self {
        let w = (screen.0 / CELL).max(8) as usize;
        let h = (screen.1 / CELL).max(6) as usize;
        let small = img
            .resize_to_fill(w as u32 * 2, h as u32 * 2, FilterType::Triangle)
            .to_rgb8();
        let (sw, sh) = small.dimensions();
        let l = |x: u32, y: u32| lightness(small.get_pixel(x.min(sw - 1), y.min(sh - 1)));
        let mut sum = vec![0f32; (w + 1) * (h + 1)];
        for y in 0..h {
            for x in 0..w {
                let mut g = 0.0;
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let (px, py) = (x as u32 * 2 + dx, y as u32 * 2 + dy);
                    let c = l(px, py);
                    g += (c - l(px + 1, py)).abs() + (c - l(px, py + 1)).abs();
                }
                sum[(y + 1) * (w + 1) + x + 1] =
                    g / 4.0 + sum[y * (w + 1) + x + 1] + sum[(y + 1) * (w + 1) + x]
                        - sum[y * (w + 1) + x];
            }
        }
        Self { w, h, sum }
    }

    fn mean(&self, x: usize, y: usize, bw: usize, bh: usize) -> f32 {
        let s = |x: usize, y: usize| self.sum[y * (self.w + 1) + x];
        (s(x + bw, y + bh) - s(x, y + bh) - s(x + bw, y) + s(x, y)) / (bw * bh).max(1) as f32
    }
}

/// Normalises a 4x3 subject heat map to 0..1, spreads it down its column (a figure spans its
/// column: face on top, body below) and marks the half where the subject stands (`side` > 0:
/// right is stronger).
pub(crate) fn heat_map(raw: &[f32], side: f32) -> Vec<f32> {
    let (lo, hi) = raw
        .iter()
        .fold((f32::MAX, f32::MIN), |a, &v| (a.0.min(v), a.1.max(v)));
    let norm: Vec<f32> = raw
        .iter()
        .map(|v| {
            if hi - lo > 1e-4 {
                (v - lo) / (hi - lo)
            } else {
                0.0
            }
        })
        .collect();
    let (cols, rows) = GRID;
    (0..norm.len())
        .map(|i| {
            let col_max = (0..rows)
                .filter_map(|r| norm.get(r * cols + i % cols))
                .fold(0f32, |a, &v| a.max(v));
            let mut v = 0.5 * norm[i] + 0.5 * col_max;
            if side.abs() > 0.01 && ((i % cols) >= cols / 2) == (side > 0.0) {
                v += 1.0;
            }
            v
        })
        .collect()
}

/// Top-left corner (logical px) of the calmest `clock`-sized box, avoiding `heat` (4x3, row by row).
pub(crate) fn calm_spot(
    busy: &Busy,
    screen: (u32, u32),
    clock: (u32, u32),
    heat: &[f32],
) -> (f64, f64) {
    let bw = (clock.0.div_ceil(CELL) as usize).clamp(1, busy.w);
    let bh = (clock.1.div_ceil(CELL) as usize).clamp(1, busy.h);
    let fit = |len: usize, b: usize, lo: usize, hi: usize| -> (usize, usize) {
        // the range of starts that keeps the margins, or the full range if the box is too big
        if len >= b + lo + hi {
            (lo, len - b - hi)
        } else {
            (0, len.saturating_sub(b))
        }
    };
    let (x0, x1) = fit(busy.w, bw, MARGIN, MARGIN);
    let (y0, y1) = fit(busy.h, bh, TOP, MARGIN);
    let (cols, rows) = GRID;
    let mut best = (f32::MAX, x0, y0);
    for y in y0..=y1 {
        for x in x0..=x1 {
            let cx = (x as f32 + bw as f32 / 2.0) / busy.w as f32;
            let cy = (y as f32 + bh as f32 / 2.0) / busy.h as f32;
            let cell = ((cy * rows as f32) as usize).min(rows - 1) * cols
                + ((cx * cols as f32) as usize).min(cols - 1);
            // a slight preference for the sides over the dead centre
            let s = busy.mean(x, y, bw, bh)
                + 0.004 * (1.0 - (cx - 0.5).abs() * 2.0)
                + 0.12 * heat.get(cell).copied().unwrap_or(0.0);
            if s < best.0 {
                best = (s, x, y);
            }
        }
    }
    let px = |cells: usize, size: u32, screen: u32| {
        f64::from((cells as u32 * CELL).min(screen.saturating_sub(size)))
    };
    (px(best.1, clock.0, screen.0), px(best.2, clock.1, screen.1))
}

/// The wallpaper cover-cropped to the screen: the 4x3 grid row by row, then the left and right halves.
pub(crate) fn crops(img: &DynamicImage, screen: (u32, u32)) -> Vec<DynamicImage> {
    let w = 768u32;
    let h = ((u64::from(w) * u64::from(screen.1.max(1)) / u64::from(screen.0.max(1))) as u32)
        .clamp(3, 4 * w);
    let cover = img.resize_to_fill(w, h, FilterType::Triangle);
    let (cols, rows) = (GRID.0 as u32, GRID.1 as u32);
    let (cw, ch) = (w / cols, h / rows);
    let mut out: Vec<DynamicImage> = (0..cols * rows)
        .map(|i| cover.crop_imm((i % cols) * cw, (i / cols) * ch, cw, ch))
        .collect();
    out.push(cover.crop_imm(0, 0, w / 2, h));
    out.push(cover.crop_imm(w / 2, 0, w / 2, h));
    out
}

pub(crate) const SUBJECT: [&str; 5] = [
    "a person",
    "a character's face",
    "an anime character",
    "an animal",
    "the main subject of the picture",
];
pub(crate) const BACKGROUND: [&str; 5] = [
    "empty sky",
    "a plain smooth background",
    "an out of focus background",
    "an empty wall",
    "calm water",
];

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};

    const SCREEN: (u32, u32) = (1920, 1080);
    const CLOCK: (u32, u32) = (260, 260);

    fn noisy_where(f: impl Fn(u32, u32) -> bool) -> DynamicImage {
        DynamicImage::ImageRgb8(RgbImage::from_fn(1920, 1080, |x, y| {
            if f(x, y) {
                let v = ((x.wrapping_mul(2654435761) ^ y.wrapping_mul(40503)) >> 7) as u8;
                Rgb([v, v, v])
            } else {
                Rgb([40, 60, 90])
            }
        }))
    }

    fn inside(p: (f64, f64)) {
        assert!(
            p.0 >= 0.0 && p.0 + f64::from(CLOCK.0) <= f64::from(SCREEN.0),
            "{p:?}"
        );
        assert!(
            p.1 >= f64::from(TOP as u32 * CELL) && p.1 + f64::from(CLOCK.1) <= f64::from(SCREEN.1),
            "{p:?}"
        );
    }

    #[test]
    fn avoids_busy_areas() {
        let img = noisy_where(|x, _| x < 1200);
        let p = calm_spot(&Busy::new(&img, SCREEN), SCREEN, CLOCK, &[]);
        inside(p);
        assert!(p.0 >= 1200.0, "{p:?}");
        let img = noisy_where(|x, y| x > 700 || y < 600);
        let p = calm_spot(&Busy::new(&img, SCREEN), SCREEN, CLOCK, &[]);
        inside(p);
        assert!(p.0 + 260.0 <= 760.0 && p.1 >= 580.0, "{p:?}");
    }

    #[test]
    fn avoids_the_subject_on_a_calm_image() {
        let img = noisy_where(|_, _| false);
        let busy = Busy::new(&img, SCREEN);
        // subject in the right column
        let raw = [0.1, 0.1, 0.2, 0.9, 0.1, 0.1, 0.2, 0.8, 0.1, 0.1, 0.1, 0.6];
        let heat = heat_map(&raw, 0.05);
        assert!(heat[3] > 1.9 && heat[0] < 0.2, "{heat:?}");
        let p = calm_spot(&busy, SCREEN, CLOCK, &heat);
        inside(p);
        assert!(p.0 + 260.0 <= 960.0, "{p:?}");
        let flipped = heat_map(
            &[0.9, 0.2, 0.1, 0.1, 0.8, 0.2, 0.1, 0.1, 0.6, 0.1, 0.1, 0.1],
            -0.05,
        );
        let p = calm_spot(&busy, SCREEN, CLOCK, &flipped);
        assert!(p.0 >= 960.0, "{p:?}");
    }

    #[test]
    fn oversized_clock_and_tiny_screens_stay_on_screen() {
        let img = noisy_where(|_, _| false);
        let small = (640, 360);
        let p = calm_spot(&Busy::new(&img, small), small, (600, 300), &[]);
        assert!(p.0 + 600.0 <= 640.0 && p.1 + 300.0 <= 360.0, "{p:?}");
        let p = calm_spot(&Busy::new(&img, (100, 50)), (100, 50), (400, 400), &[]);
        assert_eq!(p, (0.0, 0.0));
        assert_eq!(heat_map(&[0.3; 12], 0.0), vec![0.0; 12]);
    }

    #[test]
    fn crops_cover_the_screen_aspect() {
        let img = noisy_where(|_, _| true);
        let c = crops(&img, (2560, 1080));
        assert_eq!(c.len(), 14);
        assert_eq!((c[0].width(), c[0].height()), (192, 108));
        assert_eq!((c[12].width(), c[12].height()), (384, 324));
    }
}
