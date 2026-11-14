//! The accent: a saturated mid-tone colour that is really on the wallpaper, steered by the
//! accent preferences. Colour names (English and Russian, with modifiers like "dark" or
//! "cold") are matched in OKLab directly; other phrases fall back to CLIP on flat swatches.

use crate::clip::Clip;
use crate::color::{self, Lab, Swatch};
use crate::error::Result;
use crate::prefs::Prefs;
use crate::score::{best, embed_groups};
use image::{DynamicImage, Rgb, RgbImage};

/// Swatches worth considering: palette areas and small vivid details, saturated and mid-tone.
pub(crate) fn candidates(img: &DynamicImage) -> Vec<Swatch> {
    let mut pal: Vec<Swatch> = color::palette(img, 14)
        .into_iter()
        .filter(|s| s.share >= 0.004)
        .collect();
    pal.extend(
        color::vivid(img, 4)
            .into_iter()
            .filter(|s| s.share >= 0.002),
    );
    pal.retain(|s| s.lab.chroma() >= 0.04 && (0.3..=0.85).contains(&s.lab.l));
    pal
}

const BASES: &[(&[&str], &str)] = &[
    (&["red", "красн"], "#d62828"),
    (&["crimson", "scarlet", "алый", "алая"], "#c0182e"),
    (&["burgundy", "maroon", "wine", "бордов"], "#7a1f33"),
    (&["orange", "оранж"], "#f28c28"),
    (&["amber", "янтар"], "#ffb000"),
    (&["yellow", "желт", "жёлт"], "#f2d42c"),
    (&["gold", "golden", "золот"], "#d4a72c"),
    (&["mustard", "горчичн"], "#c8a020"),
    (&["lime", "лайм"], "#9acd32"),
    (&["green", "зелен", "зелён"], "#3a9d4a"),
    (&["emerald", "изумруд"], "#1f9a6a"),
    (&["olive", "оливк", "оливков"], "#808a2e"),
    (&["mint", "мятн"], "#7fd8b4"),
    (&["teal"], "#138a8a"),
    (&["turquoise", "бирюз"], "#30c8b8"),
    (&["cyan", "aqua", "циан"], "#1fc8d8"),
    (&["sky", "azure", "голуб"], "#6ab4f0"),
    (&["blue", "cobalt", "син"], "#2f6fd8"),
    (&["navy"], "#1f2f6b"),
    (&["indigo", "индиго"], "#4b3a9a"),
    (&["purple", "фиолет", "лилов"], "#8a3ab9"),
    (&["violet"], "#8a5ae0"),
    (&["lavender", "лаванд"], "#b09ae0"),
    (&["lilac", "сирен"], "#c09ad8"),
    (&["mauve"], "#a07a9a"),
    (&["magenta", "fuchsia", "пурпур", "малин"], "#d02c9c"),
    (&["pink", "розов"], "#ef7fb4"),
    (&["rose"], "#e0607e"),
    (&["coral", "корал"], "#f07a5a"),
    (&["salmon", "лосос"], "#f08a78"),
    (&["peach", "персик"], "#f4b08a"),
    (&["brown", "коричн"], "#7a4a2a"),
    (&["beige", "беж"], "#d8c4a0"),
    (&["tan"], "#c09a6a"),
    (&["white", "бел"], "#f4f4f4"),
    (
        &["gray", "grey", "серый", "серая", "серое", "серые"],
        "#8a8a8a",
    ),
    (&["silver", "серебр"], "#b8bcc4"),
    (&["black", "черн", "чёрн"], "#161616"),
];

#[derive(Clone, Copy)]
enum Modifier {
    Dark,
    Light,
    Pastel,
    Vivid,
    Muted,
    Muddy,
    Soft,
    Cold,
    Warm,
}

const MODIFIERS: &[(&[&str], Modifier)] = &[
    (&["dark", "deep", "темн", "тёмн", "глубок"], Modifier::Dark),
    (&["light", "pale", "светл", "бледн"], Modifier::Light),
    (&["pastel", "пастел"], Modifier::Pastel),
    (
        &[
            "bright",
            "vivid",
            "neon",
            "electric",
            "saturated",
            "ярк",
            "неон",
            "насыщен",
        ],
        Modifier::Vivid,
    ),
    (
        &[
            "muted",
            "dusty",
            "dull",
            "desaturated",
            "greyish",
            "grayish",
            "приглуш",
            "пыльн",
        ],
        Modifier::Muted,
    ),
    (&["muddy", "dirty", "грязн"], Modifier::Muddy),
    (&["soft", "мягк", "нежн"], Modifier::Soft),
    (&["cold", "cool", "icy", "холодн", "ледян"], Modifier::Cold),
    (&["warm", "тепл", "тёпл"], Modifier::Warm),
];

/// Russian entries are stems (prefix match), English ones whole words.
fn matches(word: &str, keys: &[&str]) -> bool {
    keys.iter().any(|k| {
        if k.is_ascii() {
            word == *k || word.strip_suffix('s') == Some(*k) || word.strip_suffix("ish") == Some(*k)
        } else {
            word.starts_with(k)
        }
    })
}

fn words(s: &str) -> impl Iterator<Item = String> + '_ {
    s.split(|c: char| !c.is_alphabetic())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
}

/// The colour a phrase names ("cold blue", "тёмно-зелёный"), if it names one; the last colour
/// word wins ("blue green" → green-ish blue is not attempted, "green").
pub(crate) fn named(phrase: &str) -> Option<Lab> {
    let mut base = None;
    let mut mods = Vec::new();
    for w in words(phrase) {
        if let Some((_, hex)) = BASES.iter().find(|(k, _)| matches(&w, k)) {
            base = Lab::from_hex(hex);
        }
        if let Some((_, m)) = MODIFIERS.iter().find(|(k, _)| matches(&w, k)) {
            mods.push(*m);
        }
    }
    let mut c = base?;
    let toward = |c: &mut Lab, hex: &str, t: f32| {
        if let Some(o) = Lab::from_hex(hex) {
            c.a += (o.a - c.a) * t;
            c.b += (o.b - c.b) * t;
        }
    };
    let scale = |c: &mut Lab, k: f32| {
        c.a *= k;
        c.b *= k;
    };
    for m in mods {
        match m {
            Modifier::Dark => c.l -= 0.15,
            Modifier::Light => {
                c.l += 0.12;
                scale(&mut c, 0.8);
            }
            Modifier::Pastel => {
                c.l += 0.15;
                scale(&mut c, 0.55);
            }
            Modifier::Vivid => scale(&mut c, 1.3),
            Modifier::Muted => scale(&mut c, 0.5),
            Modifier::Muddy => {
                c.l -= 0.08;
                scale(&mut c, 0.5);
            }
            Modifier::Soft => scale(&mut c, 0.75),
            Modifier::Cold => toward(&mut c, "#2c8ce0", 0.3),
            Modifier::Warm => toward(&mut c, "#e8823a", 0.3),
        }
    }
    c.l = c.l.clamp(0.0, 1.0);
    Some(c)
}

/// Colour names in the wallpaper prompts ("girl with short pink hair" → "pink"): accent hints
/// when `accentLike` is empty.
pub(crate) fn colour_words(like: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for l in like {
        for w in words(l) {
            if BASES.iter().any(|(k, _)| matches(&w, k)) && !out.contains(&w) {
                out.push(w);
            }
        }
    }
    out
}

fn similarity(a: Lab, b: Lab) -> f32 {
    let d2 = (0.6 * (a.l - b.l)).powi(2) + (a.a - b.a).powi(2) + (a.b - b.b).powi(2);
    (-d2 / 0.01).exp()
}

fn nearest(lab: Lab, targets: &[Lab]) -> f32 {
    targets
        .iter()
        .map(|t| similarity(lab, *t))
        .fold(0.0, f32::max)
}

/// The best swatch: share, chroma and closeness to L=0.55 (seeds that keep reading as that colour
/// in a Material scheme) plus the preference term `pref(i)` for swatch `i`.
pub(crate) fn choose(cands: &[Swatch], pref: impl Fn(usize) -> f32) -> Option<Lab> {
    let mut best: Option<(f32, Lab)> = None;
    for (i, s) in cands.iter().enumerate() {
        let sc = 0.8 * s.share.sqrt() + 2.5 * (s.lab.chroma() / 0.12).min(1.0)
            - 3.0 * (s.lab.l - 0.55).abs()
            + pref(i);
        if best.is_none_or(|b| sc > b.0) {
            best = Some((sc, s.lab));
        }
    }
    best.map(|b| b.1)
}

fn swatch_image(c: Lab) -> DynamicImage {
    DynamicImage::ImageRgb8(RgbImage::from_pixel(64, 64, Rgb(c.to_rgb())))
}

/// The accent for `img`, `None` when nothing on it is colourful enough. `clip` is only touched
/// when a preference phrase names no known colour.
pub(crate) fn pick(img: &DynamicImage, p: &Prefs, clip: &Clip) -> Result<Option<Lab>> {
    let cands = candidates(img);
    if cands.is_empty() {
        return Ok(None);
    }
    let like = if p.accent_like.is_empty() {
        colour_words(&p.like)
    } else {
        p.accent_like.clone()
    };
    let split = |v: &[String]| -> (Vec<Lab>, Vec<String>) {
        let mut labs = Vec::new();
        let mut other = Vec::new();
        for s in v.iter().filter(|s| !s.trim().is_empty()) {
            match named(s) {
                Some(l) => labs.push(l),
                None => other.push(s.clone()),
            }
        }
        (labs, other)
    };
    let (like_lab, like_txt) = split(&like);
    let (dis_lab, dis_txt) = split(&p.accent_dislike);
    let clip_pref: Vec<f32> = if like_txt.is_empty() && dis_txt.is_empty() {
        vec![0.0; cands.len()]
    } else {
        let g = embed_groups(
            clip,
            &[(&like_txt, "the color {}"), (&dis_txt, "the color {}")],
        )?;
        let sw: Vec<DynamicImage> = cands.iter().map(|s| swatch_image(s.lab)).collect();
        let embs = clip.images(&sw, &mut |_| {})?;
        embs.iter()
            .map(|e| best(e, &g[0]).unwrap_or(0.0) - best(e, &g[1]).unwrap_or(0.0))
            .collect()
    };
    Ok(choose(&cands, |i| {
        let lab = cands[i].lab;
        1.5 * (nearest(lab, &like_lab) - nearest(lab, &dis_lab)) + 15.0 * clip_pref[i]
    }))
}

/// The most colourful palette colour: the accent of last resort for a grey wallpaper.
pub(crate) fn fallback(img: &DynamicImage) -> Lab {
    color::palette(img, 8)
        .into_iter()
        .max_by(|a, b| {
            (a.lab.chroma() + 0.01 * a.share).total_cmp(&(b.lab.chroma() + 0.01 * b.share))
        })
        .map_or(
            Lab {
                l: 0.55,
                a: 0.0,
                b: 0.0,
            },
            |s| s.lab,
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Horizontal bands of (colour, width in px) on a 200 px wide image.
    fn bands(spec: &[([u8; 3], u32)]) -> DynamicImage {
        let mut img = RgbImage::new(200, 100);
        let mut x0 = 0;
        for (c, w) in spec {
            for x in x0..(x0 + w).min(200) {
                for y in 0..100 {
                    img.put_pixel(x, y, Rgb(*c));
                }
            }
            x0 += w;
        }
        DynamicImage::ImageRgb8(img)
    }

    fn offline_pick(img: &DynamicImage, p: &Prefs) -> String {
        let cancel = crate::Cancel::new();
        let tmp = tempfile::tempdir().unwrap();
        // no model files and an unroutable endpoint: named colours must not need CLIP
        let clip = Clip::new(tmp.path(), "http://127.0.0.1:9", &cancel);
        pick(img, p, &clip).unwrap().unwrap().hex()
    }

    fn hue_deg(l: Lab) -> f32 {
        l.b.atan2(l.a).to_degrees().rem_euclid(360.0)
    }

    #[test]
    fn names_and_modifiers() {
        let blue = named("blue").unwrap();
        assert!((hue_deg(blue) - 263.0).abs() < 10.0, "{}", hue_deg(blue));
        assert!(named("dark blue").unwrap().l < blue.l);
        assert!(named("pastel pink").unwrap().chroma() < named("pink").unwrap().chroma());
        assert!(named("muddy olive").unwrap().chroma() < named("olive").unwrap().chroma());
        assert!(
            hue_deg(named("cold blue").unwrap()) < hue_deg(blue),
            "cold shifts toward cyan"
        );
        assert!(named("тёмно-зелёный").unwrap().l < named("зелёный").unwrap().l);
        assert_eq!(named("синий").unwrap(), blue);
        assert!(named("pinkish").is_some() && named("reds").is_some());
        assert!(named("the color of the sea at night").is_none());
        assert!(
            named("redemption").is_none(),
            "whole words only for English"
        );
    }

    #[test]
    fn colour_words_from_like_prompts() {
        let like = vec![
            "girl with short pink hair".to_string(),
            "Blue sky, pink clouds".into(),
            "mountains".into(),
        ];
        assert_eq!(colour_words(&like), ["pink", "blue", "sky"]);
    }

    #[test]
    fn candidates_skip_grey_and_extremes() {
        let img = bands(&[
            ([128, 128, 128], 100),
            ([250, 250, 250], 50),
            ([5, 5, 10], 40),
            ([40, 90, 220], 10),
        ]);
        let c = candidates(&img);
        assert!(
            !c.is_empty() && c.iter().all(|s| s.lab.chroma() >= 0.04),
            "{c:?}"
        );
        let grey = bands(&[([128, 128, 128], 200)]);
        assert!(candidates(&grey).is_empty());
        assert!(fallback(&grey).chroma() < 0.01);
    }

    #[test]
    fn prefers_liked_colour_even_when_small() {
        let img = bands(&[
            ([230, 120, 40], 150),
            ([40, 100, 220], 20),
            ([30, 30, 30], 30),
        ]);
        let p = Prefs {
            accent_like: vec!["blue".into()],
            accent_dislike: vec![],
            ..Prefs::default()
        };
        let hex = offline_pick(&img, &p);
        let lab = Lab::from_hex(&hex).unwrap();
        assert!((hue_deg(lab) - 263.0).abs() < 20.0, "{hex}");
        let p = Prefs {
            accent_like: vec!["orange".into()],
            ..p
        };
        let lab = Lab::from_hex(&offline_pick(&img, &p)).unwrap();
        assert!(hue_deg(lab) < 90.0, "{lab:?}");
    }

    #[test]
    fn disliked_colour_loses_and_like_hints_work() {
        let img = bands(&[([125, 80, 45], 140), ([60, 160, 80], 60)]);
        let p = Prefs {
            accent_like: vec![],
            like: vec![],
            accent_dislike: vec!["brown".into()],
            ..Prefs::default()
        };
        let lab = Lab::from_hex(&offline_pick(&img, &p)).unwrap();
        assert!(
            (100.0..180.0).contains(&hue_deg(lab)),
            "green expected, got {lab:?}"
        );

        let img = bands(&[([60, 160, 80], 150), ([240, 120, 180], 50)]);
        let p = Prefs {
            accent_like: vec![],
            like: vec!["girl with pink hair".into()],
            accent_dislike: vec![],
            ..Prefs::default()
        };
        let lab = Lab::from_hex(&offline_pick(&img, &p)).unwrap();
        assert!(
            hue_deg(lab) < 20.0 || hue_deg(lab) > 300.0,
            "pink expected, got {lab:?}"
        );
    }

    #[test]
    fn default_prefs_pick_cold_over_brown() {
        let img = bands(&[([130, 85, 50], 120), ([40, 120, 200], 80)]);
        let lab = Lab::from_hex(&offline_pick(&img, &Prefs::default())).unwrap();
        assert!((200.0..290.0).contains(&hue_deg(lab)), "{lab:?}");
    }
}
