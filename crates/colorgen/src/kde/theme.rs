//! Everything kde-material-you-colors 1.10.1 derives from the seed colour (`m3_scheme_utils` +
//! `ThemeConfig`): the Material token sets, the Plasma `.colors` schemes, the KSyntaxHighlighting
//! themes and the Konsole palette. Pure; the files are written by [`super::apply`].

use super::color::{self, Rgb};
use crate::mdc::{PyTone, Scheme, Tok, Variant, ZeroTemperatureRange, fix_if_disliked};
use material_colors::color::Argb;
use material_colors::hct::Hct;

/// `-sv N` / `scheme_variant`: kde-material-you-colors' scheme class order.
pub const VARIANTS: [Variant; 9] = [
    Variant::Content,
    Variant::Expressive,
    Variant::Fidelity,
    Variant::Monochrome,
    Variant::Neutral,
    Variant::TonalSpot,
    Variant::Vibrant,
    Variant::Rainbow,
    Variant::FruitSalad,
];

/// Tokens whose `is_background is True`: their tone takes `tone_multiplier`.
const BACKGROUNDS: [&str; 25] = [
    "background",
    "surface",
    "surfaceDim",
    "surfaceBright",
    "surfaceContainerLowest",
    "surfaceContainerLow",
    "surfaceContainer",
    "surfaceContainerHigh",
    "surfaceContainerHighest",
    "surfaceVariant",
    "surfaceTint",
    "primary",
    "primaryContainer",
    "secondary",
    "secondaryContainer",
    "tertiary",
    "tertiaryContainer",
    "error",
    "errorContainer",
    "primaryFixed",
    "primaryFixedDim",
    "secondaryFixed",
    "secondaryFixedDim",
    "tertiaryFixed",
    "tertiaryFixedDim",
];

/// Breeze's text states, regenerated as vibrant schemes of their own (`base_text_states`).
const TEXT_STATES: [(&str, Rgb); 5] = [
    ("link", [0x29, 0x80, 0xb9]),
    ("visited", [0x9b, 0x59, 0xb6]),
    ("negative", [0xda, 0x44, 0x53]),
    ("neutral", [0xf6, 0x74, 0x00]),
    ("positive", [0x27, 0xae, 0x60]),
];

fn clip(v: f64, lo: f64, hi: f64) -> f64 {
    v.max(lo).min(hi)
}

fn rgb(a: Argb) -> Rgb {
    [a.red, a.green, a.blue]
}

fn argb(c: Rgb) -> Argb {
    Argb::new(255, c[0], c[1], c[2])
}

/// One scheme's tokens, in `Tok::ALL` order.
#[derive(Clone, Debug, PartialEq)]
pub struct Tokens(Vec<Rgb>);

impl Tokens {
    /// `getColors`: every token re-solved through `Hct` with the chroma/tone multipliers.
    fn new(s: &Scheme, chroma_mult: f64, tone_mult: f64) -> Tokens {
        let tone_k = clip(tone_mult, if s.dark { 0.0 } else { 0.5 }, 1.5);
        let chroma_k = clip(chroma_mult, 0.0, 10.0);
        Tokens(
            Tok::ALL
                .iter()
                .map(|&t| {
                    let mut h = Hct::new(s.argb(t));
                    if BACKGROUNDS.contains(&t.name()) {
                        h = Hct::from(h.get_hue(), h.get_chroma(), (h.py_tone() * tone_k).trunc());
                    }
                    h = Hct::from(h.get_hue(), h.get_chroma() * chroma_k, h.py_tone());
                    rgb(Argb::from(h))
                })
                .collect(),
        )
    }

    pub fn get(&self, name: &str) -> Option<Rgb> {
        Tok::ALL
            .iter()
            .position(|t| t.name() == name)
            .map(|i| self.0[i])
    }

    fn tok(&self, name: &str) -> Rgb {
        self.get(name)
            .unwrap_or_else(|| panic!("no Material token {name}"))
    }
}

/// `get_material_you_colors` for a colour source.
#[derive(Clone, Debug)]
pub struct Material {
    pub seed: Rgb,
    pub light: Tokens,
    pub dark: Tokens,
    /// Tones 0..=99 of the light scheme's primary, secondary, tertiary, neutral,
    /// neutralVariant and error palettes.
    pub palettes: [Vec<Rgb>; 6],
    /// [`TEXT_STATES`] as (light, dark) token sets.
    pub text_states: Vec<(&'static str, Tokens, Tokens)>,
}

impl Material {
    pub fn new(
        seed: Rgb,
        variant: Variant,
        chroma_mult: f64,
        tone_mult: f64,
    ) -> Result<Material, ZeroTemperatureRange> {
        let src = Hct::new(argb(seed));
        let light = Scheme::new(src, variant, false, 0.0)?;
        let dark = Scheme::new(src, variant, true, 0.0)?;
        let tones = |p: &crate::mdc::Palette| (0..100).map(|t| rgb(p.argb(t as f64))).collect();
        let mut text_states = Vec::with_capacity(TEXT_STATES.len());
        for (name, c) in TEXT_STATES {
            let h = Hct::new(Argb::from(fix_if_disliked(Hct::new(argb(c)))));
            let l = Scheme::new(h, Variant::Vibrant, false, 0.0)?;
            let d = Scheme::new(h, Variant::Vibrant, true, 0.0)?;
            text_states.push((
                name,
                Tokens::new(&l, chroma_mult, tone_mult),
                Tokens::new(&d, chroma_mult, tone_mult),
            ));
        }
        Ok(Material {
            seed,
            light: Tokens::new(&light, chroma_mult, tone_mult),
            dark: Tokens::new(&dark, chroma_mult, tone_mult),
            palettes: [
                tones(&light.primary),
                tones(&light.secondary),
                tones(&light.tertiary),
                tones(&light.neutral),
                tones(&light.neutral_variant),
                tones(&light.error),
            ],
            text_states,
        })
    }

    pub fn tokens(&self, dark: bool) -> &Tokens {
        if dark { &self.dark } else { &self.light }
    }

    fn state(&self, name: &str, dark: bool) -> &Tokens {
        let (_, l, d) = self
            .text_states
            .iter()
            .find(|(n, ..)| *n == name)
            .unwrap_or_else(|| panic!("no text state {name}"));
        if dark { d } else { l }
    }

    fn palette(&self, name: &str) -> &[Rgb] {
        let i = [
            "primary",
            "secondary",
            "tertiary",
            "neutral",
            "neutralVariant",
            "error",
        ]
        .iter()
        .position(|p| *p == name)
        .unwrap_or_else(|| panic!("no palette {name}"));
        &self.palettes[i]
    }
}

/// A Konsole colour scheme (`pywal` palette in the original).
#[derive(Clone, Debug, PartialEq)]
pub struct Konsole {
    pub background: Rgb,
    pub background_intense: Rgb,
    pub background_faint: Rgb,
    pub foreground: Rgb,
    pub foreground_intense: Rgb,
    pub foreground_faint: Rgb,
    pub normal: [Rgb; 8],
    pub intense: [Rgb; 8],
    pub faint: [Rgb; 8],
}

impl Konsole {
    /// `ThemeConfig`'s pywal palettes: seven accents from `best` (the custom colours, else the
    /// seed) topped up from the primary/tertiary palettes, readable on the surface.
    fn new(m: &Material, best: &[Rgb], sort: bool, dark: bool) -> Konsole {
        let [p, s, t, n, ..] = &m.palettes;
        let bg = m.tokens(dark).tok("surface");
        let (ink, edge) = if dark { (s[90], n[99]) } else { (s[25], n[10]) };
        let readable = |c: Rgb| {
            if dark {
                color::blend_to_contrast(color::lighten(c, 0.2, n[99]), bg, edge, 2.5, true)
            } else {
                color::blend_to_contrast(c, bg, edge, 2.0, false)
            }
        };
        let mut accents: Vec<Rgb> = Vec::with_capacity(7);
        let mut tone = 50;
        for x in 0..7 {
            if accents.len() >= 7 {
                break;
            }
            if let Some(&c) = best.get(x) {
                accents.push(if dark {
                    readable(c)
                } else {
                    readable(color::lighten(color::with_saturation(c, 1.0), 0.2, n[99]))
                });
                continue;
            }
            for pal in [p, t] {
                if accents.len() < 7 {
                    let c = pal[tone];
                    accents.push(readable(if dark {
                        c
                    } else {
                        color::with_saturation(c, 1.0)
                    }));
                }
            }
            if tone < 91 {
                tone += 8;
            }
        }
        if sort {
            accents.sort_by(|a, b| color::luminance(*a).total_cmp(&color::luminance(*b)));
        }
        let mix = if dark { n[99] } else { n[1] };
        let mut normal = [bg; 8];
        let mut intense = [color::blend(bg, ink, 0.8); 8];
        let mut faint = [color::blend(bg, ink, 0.7); 8];
        for (i, &a) in accents.iter().enumerate() {
            normal[i + 1] = color::blend(mix, a, 0.95);
            intense[i + 1] = color::blend(mix, a, 0.82);
            faint[i + 1] = color::blend(bg, a, 0.7);
        }
        let primary = m.tokens(dark).tok("primary");
        Konsole {
            background: bg,
            background_intense: color::blend(n[8], primary, 0.0),
            background_faint: color::blend(n[8], primary, 0.35),
            foreground: color::blend(bg, ink, 0.98),
            foreground_intense: ink,
            foreground_faint: color::blend(bg, ink, 0.88),
            normal,
            intense,
            faint,
        }
    }
}

/// What gets written for one seed.
#[derive(Clone, Debug)]
pub struct Theme {
    pub material: Material,
    /// `MaterialYouLight.colors` / `MaterialYouDark.colors`.
    pub plasma_light: String,
    pub plasma_dark: String,
    /// `material-you-light.theme` / `material-you-dark.theme`.
    pub ksyntax_light: String,
    pub ksyntax_dark: String,
    pub konsole_light: Konsole,
    pub konsole_dark: Konsole,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Params {
    pub variant: Variant,
    pub chroma_mult: f64,
    pub tone_mult: f64,
    /// `custom_colors_list`: Konsole's accents instead of the seed.
    pub custom_colors: Option<Vec<Rgb>>,
    /// Header/titlebar background opacity, percent.
    pub toolbar_opacity: f64,
    pub toolbar_opacity_dark: f64,
}

impl Default for Params {
    fn default() -> Params {
        Params {
            variant: Variant::TonalSpot,
            chroma_mult: 1.0,
            tone_mult: 1.0,
            custom_colors: None,
            toolbar_opacity: 100.0,
            toolbar_opacity_dark: 100.0,
        }
    }
}

const PLASMA_LIGHT: &str = include_str!("light.colors.in");
const PLASMA_DARK: &str = include_str!("dark.colors.in");
const KSYNTAX_LIGHT: &str = include_str!("ksyntax-light.theme.in");
const KSYNTAX_DARK: &str = include_str!("ksyntax-dark.theme.in");

/// Fills `{K:a:b...}` placeholders (K an upper-case letter) through `f`.
fn render(tpl: &str, f: impl Fn(char, &[&str]) -> String) -> String {
    let mut out = String::with_capacity(tpl.len() + tpl.len() / 8);
    let mut rest = tpl;
    while let Some(i) = rest.find('{') {
        out += &rest[..i];
        let tail = &rest[i..];
        let b = tail.as_bytes();
        let end = (b.len() > 2 && b[1].is_ascii_uppercase() && b[2] == b':')
            .then(|| tail.find('}'))
            .flatten();
        match end {
            Some(e) => {
                let parts: Vec<&str> = tail[3..e].split(':').collect();
                out += &f(b[1] as char, &parts);
                rest = &tail[e + 1..];
            }
            None => {
                out.push('{');
                rest = &tail[1..];
            }
        }
    }
    out + rest
}

impl Theme {
    pub fn new(seed: Rgb, p: &Params) -> Result<Theme, ZeroTemperatureRange> {
        let m = Material::new(seed, p.variant, p.chroma_mult, p.tone_mult)?;
        let best = p.custom_colors.clone().unwrap_or_else(|| vec![seed]);
        let sort = p.custom_colors.is_none();
        let mode = |s: &str| s == "dark";
        let plasma = |tpl: &str, dark: bool| {
            let opacity = if dark {
                p.toolbar_opacity_dark
            } else {
                p.toolbar_opacity
            };
            let toks = m.tokens(dark);
            render(tpl, |k, a| match (k, a) {
                ('C', [t]) => color::hex(toks.tok(t)),
                ('A', [t]) => color::with_alpha(toks.tok(t), opacity),
                ('X', [n, md, t]) => color::hex(m.state(n, mode(md)).tok(t)),
                _ => panic!("bad placeholder {k}:{a:?}"),
            })
        };
        let ksyntax = |tpl: &str| {
            render(tpl, |k, a| match (k, a) {
                ('W', [md]) => color::hex(m.tokens(mode(md)).tok("surface")),
                ('T', [pal, i]) => color::hex(m.palette(pal)[i.parse::<usize>().unwrap()]),
                ('C', [md, t]) => color::hex(m.tokens(mode(md)).tok(t)),
                ('X', [n, md, t]) => color::hex(m.state(n, mode(md)).tok(t)),
                _ => panic!("bad placeholder {k}:{a:?}"),
            })
        };
        Ok(Theme {
            plasma_light: plasma(PLASMA_LIGHT, false),
            plasma_dark: plasma(PLASMA_DARK, true),
            ksyntax_light: ksyntax(KSYNTAX_LIGHT),
            ksyntax_dark: ksyntax(KSYNTAX_DARK),
            konsole_light: Konsole::new(&m, &best, sort, false),
            konsole_dark: Konsole::new(&m, &best, sort, true),
            material: m,
        })
    }

    pub fn plasma(&self, dark: bool) -> &str {
        if dark {
            &self.plasma_dark
        } else {
            &self.plasma_light
        }
    }

    pub fn konsole(&self, dark: bool) -> &Konsole {
        if dark {
            &self.konsole_dark
        } else {
            &self.konsole_light
        }
    }
}
