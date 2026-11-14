//! How well a candidate fits: CLIP similarity to the text preferences, quality and scene prompts,
//! the learnt taste, and penalties computed from the pixels.

use crate::clip::{Clip, dot, normalize};
use crate::color::lightness;
use crate::error::Result;
use crate::prefs::{Mode, Prefs};
use crate::taste::Taste;
use image::DynamicImage;
use rand::Rng;

pub(crate) const WALLPAPER: &str = "a desktop wallpaper of {}";

pub(crate) fn best(e: &[f32], set: &[Vec<f32>]) -> Option<f32> {
    set.iter().map(|t| dot(e, t)).reduce(f32::max)
}

/// Embeds every prompt of every group as written and through its template (`{}` = the prompt),
/// averaging the two; all groups go through one text-model pass.
pub(crate) fn embed_groups(
    clip: &Clip,
    groups: &[(&[String], &str)],
) -> Result<Vec<Vec<Vec<f32>>>> {
    let mut texts = Vec::new();
    for (prompts, template) in groups {
        for p in *prompts {
            texts.push(p.clone());
            texts.push(template.replace("{}", p));
        }
    }
    let e = clip.texts(&texts)?;
    let mut pairs = e
        .chunks(2)
        .map(|c| normalize(c[0].iter().zip(&c[1]).map(|(a, b)| a + b).collect()));
    Ok(groups
        .iter()
        .map(|(p, _)| pairs.by_ref().take(p.len()).collect())
        .collect())
}

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| (*s).to_string()).collect()
}

#[derive(Default)]
pub(crate) struct Scorer {
    pub like: Vec<Vec<f32>>,
    pub dislike: Vec<Vec<f32>>,
    pub good: Vec<Vec<f32>>,
    pub bad: Vec<Vec<f32>>,
    pub scene: Vec<Vec<f32>>,
    pub plain: Vec<Vec<f32>>,
}

impl Scorer {
    pub(crate) fn new(clip: &Clip, p: &Prefs) -> Result<Self> {
        let good = strings(&[
            "a beautiful high quality wallpaper",
            "stunning aesthetic composition, professional photo or artwork",
        ]);
        let bad = strings(&[
            "a low quality blurry jpeg",
            "an ugly amateur picture",
            "a screenshot with lots of text",
        ]);
        let scene = strings(&[
            "an illustration with a fully drawn detailed background scenery",
            "a character inside a rich colorful environment",
        ]);
        let plain = strings(&[
            "a character on a plain empty white background",
            "a character on a plain black background",
            "a sketch with no background",
        ]);
        let none: Vec<String> = Vec::new();
        let (scene, plain) = if p.full_scene {
            (&scene, &plain)
        } else {
            (&none, &none)
        };
        let mut g = embed_groups(
            clip,
            &[
                (&p.like, WALLPAPER),
                (&p.dislike, WALLPAPER),
                (&good, "{}"),
                (&bad, "{}"),
                (scene, "{}"),
                (plain, "{}"),
            ],
        )?
        .into_iter();
        let mut next = || g.next().unwrap_or_default();
        Ok(Self {
            like: next(),
            dislike: next(),
            good: next(),
            bad: next(),
            scene: next(),
            plain: next(),
        })
    }

    pub(crate) fn like(&self, e: &[f32]) -> f32 {
        best(e, &self.like).unwrap_or(0.0)
    }

    pub(crate) fn score(&self, e: &[f32], taste: &Taste) -> f32 {
        let sim = |set: &[Vec<f32>]| best(e, set).unwrap_or(0.0);
        let quality = sim(&self.good) - sim(&self.bad);
        let scene = sim(&self.scene) - sim(&self.plain);
        100.0 * (self.like(e) - sim(&self.dislike)) + 40.0 * quality + 60.0 * scene + taste.score(e)
    }
}

/// Penalises pictures that are mostly an empty flat backdrop around the subject.
pub(crate) fn flat_penalty(img: &DynamicImage) -> f32 {
    let small = img.thumbnail(96, 96).to_rgb8();
    let (w, h) = small.dimensions();
    if w < 2 || h < 2 {
        return 0.0;
    }
    let l = |x, y| lightness(small.get_pixel(x, y));
    let mut flat = 0usize;
    for y in 0..h - 1 {
        for x in 0..w - 1 {
            let c = l(x, y);
            if (c - l(x + 1, y)).abs() < 0.012 && (c - l(x, y + 1)).abs() < 0.012 {
                flat += 1;
            }
        }
    }
    let share = flat as f32 / ((w - 1) * (h - 1)) as f32;
    60.0 * (share - 0.2).max(0.0)
}

/// Penalises brightness that fights the theme: very dark pixels under a light theme, very bright
/// ones under a dark theme.
pub(crate) fn tone_penalty(img: &DynamicImage, mode: Mode) -> f32 {
    let small = img.thumbnail(64, 64).to_rgb8();
    let n = small.pixels().len().max(1) as f32;
    let share =
        |f: &dyn Fn(f32) -> bool| small.pixels().filter(|p| f(lightness(p))).count() as f32 / n;
    match mode {
        Mode::Light => 30.0 * share(&|l| l < 0.3),
        Mode::Dark => 30.0 * share(&|l| l > 0.85),
        Mode::Auto => 0.0,
    }
}

/// The main embedding sees the centre square only: rescore from the half where the liked subject
/// is stronger and reward subjects that sit there rather than in the middle.
pub(crate) fn side_score(
    s: &Scorer,
    taste: &Taste,
    whole: &[f32],
    left: &[f32],
    right: &[f32],
    base: f32,
) -> f32 {
    let side = if s.like(left) >= s.like(right) {
        left
    } else {
        right
    };
    let pixel_terms = base - s.score(whole, taste);
    s.score(side, taste) + 100.0 * (s.like(side) - s.like(whole)) + pixel_terms
}

/// Indices by descending score.
pub(crate) fn ranked(scores: &[f32]) -> Vec<usize> {
    let mut o: Vec<usize> = (0..scores.len()).collect();
    o.sort_by(|&a, &b| scores[b].total_cmp(&scores[a]));
    o
}

/// Softmax sampling among the 12 best with temperature `3 * randomness`; 0 = always the best.
/// `None` only for no candidates.
pub(crate) fn sample(
    scores: &[f32],
    allowed: &[usize],
    randomness: f32,
    rng: &mut impl Rng,
) -> Option<usize> {
    let mut order: Vec<usize> = ranked(scores)
        .into_iter()
        .filter(|i| allowed.contains(i))
        .collect();
    order.truncate(12);
    let first = *order.first()?;
    let t = randomness.clamp(0.0, 1.0) * 3.0;
    if t < 0.05 {
        return Some(first);
    }
    let w: Vec<f32> = order
        .iter()
        .map(|&i| ((scores[i] - scores[first]) / t).exp())
        .collect();
    let mut r = rng.random::<f32>() * w.iter().sum::<f32>();
    for (k, wk) in w.iter().enumerate() {
        r -= wk;
        if r <= 0.0 {
            return Some(order[k]);
        }
    }
    Some(first)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};
    use rand::{SeedableRng, rngs::StdRng};

    fn axis(i: usize) -> Vec<f32> {
        let mut v = vec![0f32; 8];
        v[i] = 1.0;
        v
    }

    fn mix(a: &[f32], b: &[f32], t: f32) -> Vec<f32> {
        normalize(
            a.iter()
                .zip(b)
                .map(|(x, y)| x * (1.0 - t) + y * t)
                .collect(),
        )
    }

    fn scorer() -> Scorer {
        Scorer {
            like: vec![axis(0)],
            dislike: vec![axis(1)],
            good: vec![axis(2)],
            bad: vec![axis(3)],
            ..Scorer::default()
        }
    }

    #[test]
    fn likes_beat_dislikes_and_quality_counts() {
        let s = scorer();
        let t = Taste::default();
        let liked = mix(&axis(0), &axis(4), 0.3);
        let disliked = mix(&axis(1), &axis(4), 0.3);
        assert!(s.score(&liked, &t) > s.score(&disliked, &t) + 50.0);
        let good = mix(&liked, &axis(2), 0.3);
        let bad = mix(&liked, &axis(3), 0.3);
        assert!(s.score(&good, &t) > s.score(&bad, &t));
        assert_eq!(Scorer::default().score(&liked, &t), 0.0);
    }

    #[test]
    fn side_rescoring_rewards_a_subject_at_the_side() {
        let s = scorer();
        let t = Taste::default();
        let whole = mix(&axis(0), &axis(5), 0.6);
        let strong = mix(&axis(0), &axis(5), 0.1);
        let base = s.score(&whole, &t) - 7.0;
        let side = side_score(&s, &t, &whole, &axis(5), &strong, base);
        assert!(side > base, "{side} vs {base}");
        // pixel penalties carry over unchanged when no half is better
        let same = side_score(&s, &t, &whole, &whole, &whole, base);
        assert!((same - base).abs() < 1e-4);
    }

    #[test]
    fn sampling_is_greedy_at_zero_and_seeded_otherwise() {
        let scores = [1.0, 5.0, 3.0, -2.0];
        let all = [0, 1, 2, 3];
        let mut rng = StdRng::seed_from_u64(9);
        assert_eq!(sample(&scores, &all, 0.0, &mut rng), Some(1));
        assert_eq!(sample(&scores, &[0, 2, 3], 0.0, &mut rng), Some(2));
        assert_eq!(sample(&scores, &[], 0.5, &mut rng), None);
        let draw = |seed| {
            let mut r = StdRng::seed_from_u64(seed);
            (0..50)
                .map(|_| sample(&scores, &all, 1.0, &mut r).unwrap())
                .collect::<Vec<_>>()
        };
        assert_eq!(draw(4), draw(4));
        let d = draw(4);
        assert!(
            d.iter().filter(|&&i| i == 1).count() > 20,
            "best wins most often: {d:?}"
        );
        assert!(d.iter().any(|&i| i != 1), "but not always: {d:?}");
        assert_eq!(ranked(&scores), [1, 2, 0, 3]);
    }

    #[test]
    fn pixel_penalties() {
        let flat = DynamicImage::ImageRgb8(RgbImage::from_pixel(64, 64, Rgb([240, 240, 240])));
        let noisy = DynamicImage::ImageRgb8(RgbImage::from_fn(64, 64, |x, y| {
            let v = ((x * 37 + y * 91) % 255) as u8;
            Rgb([v, 255 - v, v / 2])
        }));
        assert!((flat_penalty(&flat) - 48.0).abs() < 1e-3);
        assert_eq!(flat_penalty(&noisy), 0.0);
        assert!((tone_penalty(&flat, Mode::Dark) - 30.0).abs() < 1e-3);
        assert_eq!(tone_penalty(&flat, Mode::Light), 0.0);
        assert_eq!(tone_penalty(&flat, Mode::Auto), 0.0);
    }
}
