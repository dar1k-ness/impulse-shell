//! Material dynamic colors exactly as `materialyoucolor` 2.0.10 computes them (the python
//! `generate_colors_material.py` path), quirks included. HCT, temperature and the solver come
//! from `material-colors`; the token rules, contrast math and key-color search are the python
//! ones, which differ from `material-colors`' own `DynamicScheme`.

use material_colors::color::Argb;
use material_colors::hct::Hct;
use material_colors::temperature::TemperatureCache;
use material_colors::utils::math::sanitize_degrees_double;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Variant {
    Monochrome,
    Neutral,
    TonalSpot,
    Vibrant,
    Expressive,
    Fidelity,
    Content,
    Rainbow,
    FruitSalad,
}

impl Variant {
    /// `--scheme` value -> class picked by generate_colors_material.py (unknown -> tonal spot).
    pub fn from_scheme_name(name: &str) -> Variant {
        match name {
            "scheme-fruit-salad" => Variant::FruitSalad,
            "scheme-expressive" => Variant::Expressive,
            "scheme-monochrome" => Variant::Monochrome,
            "scheme-rainbow" => Variant::Rainbow,
            "scheme-neutral" => Variant::Neutral,
            "scheme-fidelity" => Variant::Fidelity,
            "scheme-content" => Variant::Content,
            "scheme-vibrant" => Variant::Vibrant,
            _ => Variant::TonalSpot,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub hue: f64,
    pub chroma: f64,
    pub key_tone: f64,
}

impl Palette {
    pub fn from_hct(h: Hct) -> Palette {
        Palette {
            hue: h.get_hue(),
            chroma: h.get_chroma(),
            key_tone: h.py_tone(),
        }
    }

    /// `TonalPalette.from_hue_and_chroma` with python's `KeyColor.create` binary search.
    pub fn from_hue_and_chroma(hue: f64, chroma: f64) -> Palette {
        let mut cache = [f64::NAN; 102];
        let mut max_chroma = |t: i32| {
            let c = &mut cache[t as usize];
            if c.is_nan() {
                *c = Hct::from(hue, 200.0, t as f64).get_chroma();
            }
            *c
        };
        let (pivot, step, eps) = (50i32, 1i32, 0.01);
        let (mut lo, mut hi) = (0i32, 100i32);
        let mut key = None;
        while lo < hi {
            let mid = (lo + hi).div_euclid(2);
            let ascending = max_chroma(mid) < max_chroma(mid + step);
            if max_chroma(mid) >= chroma - eps {
                if (lo - pivot).abs() < (hi - pivot).abs() {
                    hi = mid;
                } else {
                    if lo == mid {
                        key = Some(lo);
                        break;
                    }
                    lo = mid;
                }
            } else if ascending {
                lo = mid + step;
            } else {
                hi = mid;
            }
        }
        let key = Hct::from(hue, chroma, key.unwrap_or(lo) as f64);
        Palette {
            hue,
            chroma,
            key_tone: key.py_tone(),
        }
    }

    pub fn argb(&self, tone: f64) -> Argb {
        Argb::from(Hct::from(self.hue, self.chroma, tone))
    }
}

pub struct Scheme {
    pub variant: Variant,
    pub dark: bool,
    pub contrast: f64,
    pub source: Hct,
    pub primary: Palette,
    pub secondary: Palette,
    pub tertiary: Palette,
    pub neutral: Palette,
    pub neutral_variant: Palette,
    pub error: Palette,
}

fn rotated_hue(source: &Hct, hues: &[f64], rotations: &[f64]) -> f64 {
    let sh = source.get_hue();
    if rotations.len() == 1 {
        return sanitize_degrees_double(sh + rotations[0]);
    }
    for (bounds, rotation) in hues.windows(2).zip(rotations) {
        if bounds[0] < sh && sh < bounds[1] {
            return sanitize_degrees_double(sh + rotation);
        }
    }
    sh
}

#[derive(Debug)]
pub struct ZeroTemperatureRange;

impl Scheme {
    pub fn new(
        source: Hct,
        variant: Variant,
        dark: bool,
        contrast: f64,
    ) -> Result<Scheme, ZeroTemperatureRange> {
        let tp = Palette::from_hue_and_chroma;
        let (h, c) = (source.get_hue(), source.get_chroma());
        let [p, s, t, n, nv] = match variant {
            Variant::TonalSpot => [
                tp(h, 36.0),
                tp(h, 16.0),
                tp(sanitize_degrees_double(h + 60.0), 24.0),
                tp(h, 6.0),
                tp(h, 8.0),
            ],
            Variant::Neutral => [tp(h, 12.0), tp(h, 8.0), tp(h, 16.0), tp(h, 2.0), tp(h, 2.0)],
            Variant::Monochrome => [tp(h, 0.0), tp(h, 0.0), tp(h, 0.0), tp(h, 0.0), tp(h, 0.0)],
            Variant::Rainbow => [
                tp(h, 48.0),
                tp(h, 16.0),
                tp(sanitize_degrees_double(h + 60.0), 24.0),
                tp(h, 0.0),
                tp(h, 0.0),
            ],
            Variant::FruitSalad => {
                let r = sanitize_degrees_double(h - 50.0);
                [
                    tp(r, 48.0),
                    tp(r, 36.0),
                    tp(h, 36.0),
                    tp(h, 10.0),
                    tp(h, 16.0),
                ]
            }
            Variant::Expressive => {
                const HUES: [f64; 9] = [0.0, 21.0, 51.0, 121.0, 151.0, 191.0, 271.0, 321.0, 360.0];
                const SEC: [f64; 9] = [45.0, 95.0, 45.0, 20.0, 45.0, 90.0, 45.0, 45.0, 45.0];
                const TER: [f64; 9] = [120.0, 120.0, 20.0, 45.0, 20.0, 15.0, 20.0, 120.0, 120.0];
                [
                    tp(sanitize_degrees_double(h + 240.0), 40.0),
                    tp(rotated_hue(&source, &HUES, &SEC), 24.0),
                    tp(rotated_hue(&source, &HUES, &TER), 32.0),
                    tp(h + 15.0, 8.0),
                    tp(h + 15.0, 12.0),
                ]
            }
            Variant::Vibrant => {
                const HUES: [f64; 9] = [0.0, 41.0, 61.0, 101.0, 131.0, 181.0, 251.0, 301.0, 360.0];
                const SEC: [f64; 9] = [18.0, 15.0, 10.0, 12.0, 15.0, 18.0, 15.0, 12.0, 12.0];
                const TER: [f64; 9] = [35.0, 30.0, 20.0, 25.0, 30.0, 35.0, 30.0, 25.0, 25.0];
                [
                    tp(h, 200.0),
                    tp(rotated_hue(&source, &HUES, &SEC), 24.0),
                    tp(rotated_hue(&source, &HUES, &TER), 32.0),
                    tp(h, 10.0),
                    tp(h, 12.0),
                ]
            }
            Variant::Content | Variant::Fidelity => {
                let mut cache = TemperatureCache::new(source);
                let tert = if variant == Variant::Content {
                    cache.analogous(Some(3), Some(6))[2]
                } else {
                    let w = cache.warmest();
                    let co = cache.coldest();
                    let temps = cache.temps_by_hct();
                    if temps[&w] - temps[&co] == 0.0 {
                        return Err(ZeroTemperatureRange);
                    }
                    cache.complement()
                };
                let tert = Hct::new(Argb::from(fix_if_disliked(tert)));
                [
                    tp(h, c),
                    tp(h, (c - 32.0).max(c * 0.5)),
                    Palette::from_hct(tert),
                    tp(h, c / 8.0),
                    tp(h, c / 8.0 + 4.0),
                ]
            }
        };
        Ok(Scheme {
            variant,
            dark,
            contrast,
            source,
            primary: p,
            secondary: s,
            tertiary: t,
            neutral: n,
            neutral_variant: nv,
            error: tp(25.0, 84.0),
        })
    }

    fn fidelity(&self) -> bool {
        matches!(self.variant, Variant::Fidelity | Variant::Content)
    }

    fn mono(&self) -> bool {
        self.variant == Variant::Monochrome
    }
}

/// python `Hct.tone`: `lstar_from_argb`, computed without fused multiply-add.
pub trait PyTone {
    fn py_tone(&self) -> f64;
}

impl PyTone for Hct {
    fn py_tone(&self) -> f64 {
        lstar_from_argb(Argb::from(*self))
    }
}

fn linearized(c: u8) -> f64 {
    let n = c as f64 / 255.0;
    if n <= 0.040449936 {
        n / 12.92 * 100.0
    } else {
        ((n + 0.055) / 1.055).powf(2.4) * 100.0
    }
}

pub fn lstar_from_argb(a: Argb) -> f64 {
    let y = 0.2126 * linearized(a.red) + 0.7152 * linearized(a.green) + 0.0722 * linearized(a.blue);
    116.0 * lab_f(y / 100.0) - 16.0
}

fn pyround(x: f64) -> f64 {
    x.round_ties_even()
}

pub fn fix_if_disliked(h: Hct) -> Hct {
    let hue = pyround(h.get_hue());
    if (90.0..=111.0).contains(&hue)
        && pyround(h.get_chroma()) > 16.0
        && pyround(h.py_tone()) < 65.0
    {
        Hct::from(h.get_hue(), h.get_chroma(), 70.0)
    } else {
        h
    }
}

// --- python Contrast (no fused multiply-add) ---

fn lab_f(t: f64) -> f64 {
    let e = 216.0 / 24389.0;
    let kappa = 24389.0 / 27.0;
    if t > e {
        t.powf(1.0 / 3.0)
    } else {
        (kappa * t + 16.0) / 116.0
    }
}

fn lab_invf(ft: f64) -> f64 {
    let e = 216.0 / 24389.0;
    let kappa = 24389.0 / 27.0;
    let ft3 = ft * ft * ft;
    if ft3 > e {
        ft3
    } else {
        (116.0 * ft - 16.0) / kappa
    }
}

fn y_from_lstar(l: f64) -> f64 {
    100.0 * lab_invf((l + 16.0) / 116.0)
}

fn lstar_from_y(y: f64) -> f64 {
    lab_f(y / 100.0) * 116.0 - 16.0
}

fn ratio_of_ys(y1: f64, y2: f64) -> f64 {
    let lighter = if y1 > y2 { y1 } else { y2 };
    let darker = if lighter == y2 { y1 } else { y2 };
    (lighter + 5.0) / (darker + 5.0)
}

pub fn ratio_of_tones(a: f64, b: f64) -> f64 {
    ratio_of_ys(
        y_from_lstar(a.clamp(0.0, 100.0)),
        y_from_lstar(b.clamp(0.0, 100.0)),
    )
}

fn lighter(tone: f64, ratio: f64) -> f64 {
    if !(0.0..=100.0).contains(&tone) {
        return -1.0;
    }
    let dark_y = y_from_lstar(tone);
    let light_y = ratio * (dark_y + 5.0) - 5.0;
    let real = ratio_of_ys(light_y, dark_y);
    if real < ratio && (real - ratio).abs() > 0.04 {
        return -1.0;
    }
    let v = lstar_from_y(light_y) + 0.4;
    if !(0.0..=100.0).contains(&v) { -1.0 } else { v }
}

fn darker(tone: f64, ratio: f64) -> f64 {
    if !(0.0..=100.0).contains(&tone) {
        return -1.0;
    }
    let light_y = y_from_lstar(tone);
    let dark_y = (light_y + 5.0) / ratio - 5.0;
    let real = ratio_of_ys(light_y, dark_y);
    if real < ratio && (real - ratio).abs() > 0.04 {
        return -1.0;
    }
    let v = lstar_from_y(dark_y) - 0.4;
    if !(0.0..=100.0).contains(&v) { -1.0 } else { v }
}

fn lighter_unsafe(t: f64, r: f64) -> f64 {
    let v = lighter(t, r);
    if v < 0.0 { 100.0 } else { v }
}

fn darker_unsafe(t: f64, r: f64) -> f64 {
    let v = darker(t, r);
    if v < 0.0 { 0.0 } else { v }
}

fn prefers_light(t: f64) -> bool {
    pyround(t) < 60.0
}

pub fn foreground_tone(bg: f64, ratio: f64) -> f64 {
    let lt = lighter_unsafe(bg, ratio);
    let dt = darker_unsafe(bg, ratio);
    let lr = ratio_of_tones(lt, bg);
    let dr = ratio_of_tones(dt, bg);
    if prefers_light(bg) {
        let negligible = (lr - dr).abs() < 0.1 && lr < ratio && dr < ratio;
        if lr >= ratio || lr >= dr || negligible {
            lt
        } else {
            dt
        }
    } else if dr >= ratio || dr >= lr {
        dt
    } else {
        lt
    }
}

fn curve(c: [f64; 4], level: f64) -> f64 {
    let lerp = |a: f64, b: f64, t: f64| (1.0 - t) * a + t * b;
    if level <= -1.0 {
        c[0]
    } else if level < 0.0 {
        lerp(c[0], c[1], level + 1.0)
    } else if level < 0.5 {
        lerp(c[1], c[2], level / 0.5)
    } else if level < 1.0 {
        lerp(c[2], c[3], (level - 0.5) / 0.5)
    } else {
        c[3]
    }
}

macro_rules! tokens {
    ($($name:ident),* $(,)?) => {
        #[allow(non_camel_case_types)]
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        pub enum Tok { $($name),* }
        impl Tok {
            /// In `vars(MaterialDynamicColors)` order, i.e. the SCSS order.
            pub const ALL: &[Tok] = &[$(Tok::$name),*];
            pub fn name(self) -> &'static str {
                match self { $(Tok::$name => stringify!($name)),* }
            }
        }
    };
}

tokens!(
    primary_paletteKeyColor,
    secondary_paletteKeyColor,
    tertiary_paletteKeyColor,
    neutral_paletteKeyColor,
    neutral_variant_paletteKeyColor,
    background,
    onBackground,
    surface,
    surfaceDim,
    surfaceBright,
    surfaceContainerLowest,
    surfaceContainerLow,
    surfaceContainer,
    surfaceContainerHigh,
    surfaceContainerHighest,
    onSurface,
    surfaceVariant,
    onSurfaceVariant,
    inverseSurface,
    inverseOnSurface,
    outline,
    outlineVariant,
    shadow,
    scrim,
    surfaceTint,
    primary,
    onPrimary,
    primaryContainer,
    onPrimaryContainer,
    inversePrimary,
    secondary,
    onSecondary,
    secondaryContainer,
    onSecondaryContainer,
    tertiary,
    onTertiary,
    tertiaryContainer,
    onTertiaryContainer,
    error,
    onError,
    errorContainer,
    onErrorContainer,
    primaryFixed,
    primaryFixedDim,
    onPrimaryFixed,
    onPrimaryFixedVariant,
    secondaryFixed,
    secondaryFixedDim,
    onSecondaryFixed,
    onSecondaryFixedVariant,
    tertiaryFixed,
    tertiaryFixedDim,
    onTertiaryFixed,
    onTertiaryFixedVariant,
);

#[derive(Clone, Copy, PartialEq)]
enum Polarity {
    Nearer,
    Lighter,
}

#[derive(Clone, Copy)]
enum Pal {
    P,
    S,
    T,
    N,
    NV,
    E,
}

struct Spec {
    pal: Pal,
    is_bg: bool,
    bg: Option<Tok>,
    bg2: Option<Tok>,
    curve: Option<[f64; 4]>,
    pair: Option<(Tok, Tok, f64, Polarity, bool)>,
}

fn find_desired_chroma_by_tone(hue: f64, chroma: f64, tone: f64, by_decreasing: bool) -> f64 {
    let mut answer = tone;
    let mut closest = Hct::from(hue, chroma, tone);
    if closest.get_chroma() < chroma {
        let mut peak = closest.get_chroma();
        while closest.get_chroma() < chroma {
            answer += if by_decreasing { -1.0 } else { 1.0 };
            let pot = Hct::from(hue, chroma, answer);
            if peak > pot.get_chroma() {
                break;
            }
            if (pot.get_chroma() - chroma).abs() < 0.4 {
                break;
            }
            if (pot.get_chroma() - chroma).abs() < (closest.get_chroma() - chroma).abs() {
                closest = pot;
            }
            peak = peak.max(pot.get_chroma());
        }
    }
    answer
}

impl Scheme {
    fn highest_surface(&self) -> Tok {
        if self.dark {
            Tok::surfaceBright
        } else {
            Tok::surfaceDim
        }
    }

    fn palette(&self, p: Pal) -> &Palette {
        match p {
            Pal::P => &self.primary,
            Pal::S => &self.secondary,
            Pal::T => &self.tertiary,
            Pal::N => &self.neutral,
            Pal::NV => &self.neutral_variant,
            Pal::E => &self.error,
        }
    }

    fn spec(&self, t: Tok) -> Spec {
        use Pal::*;
        use Tok::*;
        let hs = Some(self.highest_surface());
        let plain = |pal, is_bg| Spec {
            pal,
            is_bg,
            bg: None,
            bg2: None,
            curve: None,
            pair: None,
        };
        // FromPaletteOptions defaults `is_background=bool` (the type, truthy) in python.
        let on = |pal, bg: Tok, c| Spec {
            pal,
            is_bg: true,
            bg: Some(bg),
            bg2: None,
            curve: Some(c),
            pair: None,
        };
        let on2 = |pal, bg: Tok, bg2: Tok, c| Spec {
            pal,
            is_bg: true,
            bg: Some(bg),
            bg2: Some(bg2),
            curve: Some(c),
            pair: None,
        };
        let paired = |pal, c, a: Tok, b: Tok, pol, st| Spec {
            pal,
            is_bg: true,
            bg: hs,
            bg2: None,
            curve: Some(c),
            pair: Some((a, b, 10.0, pol, st)),
        };
        const C_ON: [f64; 4] = [4.5, 7.0, 11.0, 21.0];
        const C_ACC: [f64; 4] = [3.0, 4.5, 7.0, 7.0];
        const C_CONT: [f64; 4] = [1.0, 1.0, 3.0, 4.5];
        const C_ONC: [f64; 4] = [3.0, 4.5, 7.0, 11.0];
        let nr = Polarity::Nearer;
        let lt = Polarity::Lighter;
        match t {
            primary_paletteKeyColor => plain(P, false),
            secondary_paletteKeyColor => plain(S, false),
            tertiary_paletteKeyColor => plain(T, false),
            neutral_paletteKeyColor => plain(N, false),
            neutral_variant_paletteKeyColor => plain(NV, false),
            background
            | surface
            | surfaceDim
            | surfaceBright
            | surfaceContainerLowest
            | surfaceContainerLow
            | surfaceContainer
            | surfaceContainerHigh
            | surfaceContainerHighest => plain(N, true),
            onBackground => on(N, background, [3.0, 3.0, 4.5, 7.0]),
            onSurface => on(N, self.highest_surface(), C_ON),
            surfaceVariant => plain(NV, true),
            onSurfaceVariant => on(NV, self.highest_surface(), C_ONC),
            inverseSurface => plain(N, false),
            inverseOnSurface => on(N, inverseSurface, C_ON),
            outline => on(NV, self.highest_surface(), [1.5, 3.0, 4.5, 7.0]),
            outlineVariant => on(NV, self.highest_surface(), [1.0, 1.0, 3.0, 4.5]),
            shadow | scrim => plain(N, false),
            surfaceTint => plain(P, true),
            primary => paired(P, C_ACC, primaryContainer, primary, nr, false),
            onPrimary => on(P, primary, C_ON),
            primaryContainer => paired(P, C_CONT, primaryContainer, primary, nr, false),
            onPrimaryContainer => on(P, primaryContainer, C_ONC),
            inversePrimary => on(P, inverseSurface, C_ACC),
            secondary => paired(S, C_ACC, secondaryContainer, secondary, nr, false),
            onSecondary => on(S, secondary, C_ON),
            secondaryContainer => paired(S, C_CONT, secondaryContainer, secondary, nr, false),
            onSecondaryContainer => on(S, secondaryContainer, C_ONC),
            tertiary => paired(T, C_ACC, tertiaryContainer, tertiary, nr, false),
            onTertiary => on(T, tertiary, C_ON),
            tertiaryContainer => paired(T, C_CONT, tertiaryContainer, tertiary, nr, false),
            onTertiaryContainer => on(T, tertiaryContainer, C_ONC),
            error => paired(E, C_ACC, errorContainer, error, nr, false),
            onError => on(E, error, C_ON),
            errorContainer => paired(E, C_CONT, errorContainer, error, nr, false),
            onErrorContainer => on(E, errorContainer, C_ONC),
            primaryFixed | primaryFixedDim => {
                paired(P, C_CONT, primaryFixed, primaryFixedDim, lt, true)
            }
            onPrimaryFixed => on2(P, primaryFixedDim, primaryFixed, C_ON),
            onPrimaryFixedVariant => on2(P, primaryFixedDim, primaryFixed, C_ONC),
            secondaryFixed | secondaryFixedDim => {
                paired(S, C_CONT, secondaryFixed, secondaryFixedDim, lt, true)
            }
            onSecondaryFixed => on2(S, secondaryFixedDim, secondaryFixed, C_ON),
            onSecondaryFixedVariant => on2(S, secondaryFixedDim, secondaryFixed, C_ONC),
            tertiaryFixed | tertiaryFixedDim => {
                paired(T, C_CONT, tertiaryFixed, tertiaryFixedDim, lt, true)
            }
            onTertiaryFixed => on2(T, tertiaryFixedDim, tertiaryFixed, C_ON),
            onTertiaryFixedVariant => on2(T, tertiaryFixedDim, tertiaryFixed, C_ONC),
        }
    }

    /// The `tone=` lambda of each token.
    fn raw_tone(&self, t: Tok) -> f64 {
        use Tok::*;
        let d = self.dark;
        let m = self.mono();
        let pick = |a: f64, b: f64| if d { a } else { b };
        let lvl = self.contrast;
        match t {
            primary_paletteKeyColor => self.primary.key_tone,
            secondary_paletteKeyColor => self.secondary.key_tone,
            tertiary_paletteKeyColor => self.tertiary.key_tone,
            neutral_paletteKeyColor => self.neutral.key_tone,
            neutral_variant_paletteKeyColor => self.neutral_variant.key_tone,
            background | surface => pick(6.0, 98.0),
            onBackground | onSurface => pick(90.0, 10.0),
            surfaceDim => {
                if d {
                    6.0
                } else {
                    curve([87.0, 87.0, 80.0, 75.0], lvl)
                }
            }
            surfaceBright => {
                if d {
                    curve([24.0, 24.0, 29.0, 34.0], lvl)
                } else {
                    98.0
                }
            }
            surfaceContainerLowest => {
                if d {
                    curve([4.0, 4.0, 2.0, 0.0], lvl)
                } else {
                    100.0
                }
            }
            surfaceContainerLow => {
                if d {
                    curve([10.0, 10.0, 11.0, 12.0], lvl)
                } else {
                    curve([96.0, 96.0, 96.0, 95.0], lvl)
                }
            }
            surfaceContainer => {
                if d {
                    curve([12.0, 12.0, 16.0, 20.0], lvl)
                } else {
                    curve([94.0, 94.0, 92.0, 90.0], lvl)
                }
            }
            surfaceContainerHigh => {
                if d {
                    curve([17.0, 17.0, 21.0, 25.0], lvl)
                } else {
                    curve([92.0, 92.0, 88.0, 85.0], lvl)
                }
            }
            surfaceContainerHighest => {
                if d {
                    curve([22.0, 22.0, 26.0, 30.0], lvl)
                } else {
                    curve([90.0, 90.0, 84.0, 80.0], lvl)
                }
            }
            surfaceVariant => pick(30.0, 90.0),
            onSurfaceVariant => pick(80.0, 30.0),
            inverseSurface => pick(90.0, 20.0),
            inverseOnSurface => pick(20.0, 95.0),
            outline => pick(60.0, 50.0),
            outlineVariant => pick(30.0, 80.0),
            shadow | scrim => 0.0,
            surfaceTint => pick(80.0, 40.0),
            primary => {
                if m {
                    100.0
                } else {
                    pick(80.0, 40.0)
                }
            }
            onPrimary => {
                if m {
                    10.0
                } else {
                    pick(20.0, 100.0)
                }
            }
            primaryContainer => {
                if self.fidelity() {
                    self.source.py_tone()
                } else if m {
                    85.0
                } else {
                    pick(30.0, 90.0)
                }
            }
            onPrimaryContainer => {
                if self.fidelity() {
                    foreground_tone(self.raw_tone(primaryContainer), 4.5)
                } else if m {
                    0.0
                } else {
                    pick(90.0, 30.0)
                }
            }
            inversePrimary => pick(40.0, 80.0),
            secondary => pick(80.0, 40.0),
            onSecondary => {
                if m {
                    10.0
                } else {
                    pick(20.0, 100.0)
                }
            }
            secondaryContainer => {
                let initial = pick(30.0, 90.0);
                if m {
                    pick(30.0, 85.0)
                } else if self.fidelity() {
                    initial
                } else {
                    find_desired_chroma_by_tone(
                        self.secondary.hue,
                        self.secondary.chroma,
                        initial,
                        !d,
                    )
                }
            }
            onSecondaryContainer => {
                if m {
                    pick(90.0, 10.0)
                } else if !self.fidelity() {
                    pick(90.0, 30.0)
                } else {
                    foreground_tone(self.raw_tone(secondaryContainer), 4.5)
                }
            }
            tertiary => {
                if m {
                    pick(90.0, 25.0)
                } else {
                    pick(80.0, 40.0)
                }
            }
            onTertiary => {
                if m {
                    pick(10.0, 90.0)
                } else {
                    pick(20.0, 100.0)
                }
            }
            // python: `if not is_monochrome(s): return 60 if dark else 49` (sic).
            tertiaryContainer => {
                if !m {
                    pick(60.0, 49.0)
                } else if !self.fidelity() {
                    pick(30.0, 90.0)
                } else {
                    let proposed = Hct::new(self.tertiary.argb(self.source.py_tone()));
                    fix_if_disliked(proposed).py_tone()
                }
            }
            onTertiaryContainer => {
                if !m {
                    pick(0.0, 100.0)
                } else if !self.fidelity() {
                    pick(90.0, 30.0)
                } else {
                    foreground_tone(self.raw_tone(tertiaryContainer), 4.5)
                }
            }
            error => pick(80.0, 40.0),
            onError => pick(20.0, 100.0),
            errorContainer => pick(30.0, 90.0),
            onErrorContainer => {
                if d {
                    90.0
                } else if m {
                    10.0
                } else {
                    30.0
                }
            }
            primaryFixed | tertiaryFixed => {
                if m {
                    40.0
                } else {
                    90.0
                }
            }
            primaryFixedDim | tertiaryFixedDim => {
                if m {
                    30.0
                } else {
                    80.0
                }
            }
            onPrimaryFixed | onTertiaryFixed => {
                if m {
                    100.0
                } else {
                    10.0
                }
            }
            onPrimaryFixedVariant | onTertiaryFixedVariant => {
                if m {
                    90.0
                } else {
                    30.0
                }
            }
            secondaryFixed => {
                if m {
                    80.0
                } else {
                    90.0
                }
            }
            secondaryFixedDim => {
                if m {
                    70.0
                } else {
                    80.0
                }
            }
            onSecondaryFixed => 10.0,
            onSecondaryFixedVariant => {
                if m {
                    25.0
                } else {
                    30.0
                }
            }
        }
    }

    /// `DynamicColor.get_tone`.
    pub fn tone(&self, t: Tok) -> f64 {
        let spec = self.spec(t);
        let level = self.contrast;
        let decreasing = level < 0.0;
        if let Some((a, b, delta, pol, stay)) = spec.pair {
            let bg_tone = self.tone(spec.bg.unwrap());
            let a_nearer = pol == Polarity::Nearer || (pol == Polarity::Lighter && !self.dark);
            let (nearer, farther) = if a_nearer { (a, b) } else { (b, a) };
            let am_nearer = t == nearer;
            let dir = if self.dark { 1.0 } else { -1.0 };
            let nc = curve(self.spec(nearer).curve.unwrap(), level);
            let fc = curve(self.spec(farther).curve.unwrap(), level);
            let ni = self.raw_tone(nearer);
            let mut n = if ratio_of_tones(bg_tone, ni) >= nc {
                ni
            } else {
                foreground_tone(bg_tone, nc)
            };
            let fi = self.raw_tone(farther);
            let mut f = if ratio_of_tones(bg_tone, fi) >= fc {
                fi
            } else {
                foreground_tone(bg_tone, fc)
            };
            if decreasing {
                n = foreground_tone(bg_tone, nc);
                f = foreground_tone(bg_tone, fc);
            }
            // python moves the farther tone by `f - delta*dir` here (sic).
            if (f - n) * dir < delta {
                f = (f - delta * dir).clamp(0.0, 100.0);
            }
            if (50.0..60.0).contains(&n) {
                if dir > 0.0 {
                    f = f.max(n + delta * dir);
                    n = 60.0;
                } else {
                    f = f.min(n + delta * dir);
                    n = 49.0;
                }
            } else if (50.0..60.0).contains(&f) {
                if stay {
                    if dir > 0.0 {
                        f = f.max(n + delta * dir);
                        n = 60.0;
                    } else {
                        f = f.min(n + delta * dir);
                        n = 49.0;
                    }
                } else {
                    f = if dir > 0.0 { 60.0 } else { 49.0 };
                }
            }
            return if am_nearer { n } else { f };
        }
        let mut answer = self.raw_tone(t);
        let Some(bg) = spec.bg else { return answer };
        let bg_tone = self.tone(bg);
        let ratio = curve(spec.curve.unwrap(), level);
        if ratio_of_tones(bg_tone, answer) < ratio {
            answer = foreground_tone(bg_tone, ratio);
        }
        if decreasing {
            answer = foreground_tone(bg_tone, ratio);
        }
        if spec.is_bg && (50.0..60.0).contains(&answer) {
            answer = if ratio_of_tones(49.0, bg_tone) >= ratio {
                49.0
            } else {
                60.0
            };
        }
        if let Some(bg2) = spec.bg2 {
            let (t1, t2) = (bg_tone, self.tone(bg2));
            let (upper, lower) = (t1.max(t2), t1.min(t2));
            if ratio_of_tones(upper, answer) >= ratio && ratio_of_tones(lower, answer) >= ratio {
                return answer;
            }
            let light = lighter(upper, ratio);
            let dark = darker(lower, ratio);
            let pl = prefers_light(t1) || prefers_light(t2);
            // python returns `light if prefers_light and (light == -1 or dark == -1) else dark` (sic).
            return if pl && (light == -1.0 || dark == -1.0) {
                light
            } else {
                dark
            };
        }
        answer
    }

    pub fn argb(&self, t: Tok) -> Argb {
        self.palette(self.spec(t).pal).argb(self.tone(t))
    }
}

#[cfg(test)]
mod tests {
    //! Reference vectors, each tagged with its source:
    //! - MCU@194144d: github.com/material-foundation/material-color-utilities at
    //!   194144d75ed7d34f06264a625e086db81a7ab9aa, `dart/test/{contrast,dislike_analyzer,
    //!   dynamic_scheme,palettes,scheme_monochrome,dynamic_color}_test.dart`.
    //! - MCU@ab1be2d: the same repo at ab1be2d7073d032a744eb88df69a51291366516c, the last commit
    //!   with per-scheme value tests (`dart/test/scheme_*_test.dart`, removed in af27fa9).
    //! - PY: materialyoucolor 2.0.10 (PyPI), `tests/mdc/vectors.txt` written by `tests/mdc/gen.py`.
    //!
    //! MCU vectors are kept only where materialyoucolor 2.0.10 computes the same value; where the
    //! python quirks this module reproduces give another answer, PY is the reference.
    use super::*;
    use Variant::*;

    const VECTORS: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/mdc/vectors.txt"
    ));

    fn rgb(x: u32) -> Argb {
        Argb::new(255, (x >> 16) as u8, (x >> 8) as u8, x as u8)
    }

    fn hex(a: Argb) -> u32 {
        (a.red as u32) << 16 | (a.green as u32) << 8 | a.blue as u32
    }

    fn scheme(v: Variant, seed: u32, dark: bool, contrast: f64) -> Scheme {
        Scheme::new(Hct::new(rgb(seed)), v, dark, contrast).unwrap()
    }

    fn variant(name: &str) -> Variant {
        [
            Monochrome, Neutral, TonalSpot, Vibrant, Expressive, Fidelity, Content, Rainbow,
            FruitSalad,
        ]
        .into_iter()
        .find(|v| format!("{v:?}") == name)
        .unwrap()
    }

    fn tok(name: &str) -> Tok {
        *Tok::ALL.iter().find(|t| t.name() == name).unwrap()
    }

    /// `Hct.tone` of the rendered colour, like MCU's `getHct(scheme).tone`.
    fn rendered_tone(s: &Scheme, t: Tok) -> f64 {
        Hct::new(s.argb(t)).get_tone()
    }

    fn vectors(kind: &str) -> impl Iterator<Item = Vec<&'static str>> + '_ {
        VECTORS
            .lines()
            .map(|l| l.split(' ').collect::<Vec<_>>())
            .filter(move |f| f[0] == kind)
    }

    fn num(s: &str) -> f64 {
        s.parse().unwrap()
    }

    // MCU@194144d contrast_test.dart.
    #[test]
    fn mcu_contrast_bounds() {
        let near = |a: f64, b: f64| (a - b).abs() < 0.001;
        assert!(near(ratio_of_tones(-10.0, 110.0), 21.0));
        assert!(near(lighter(90.0, 10.0), -1.0));
        assert!(near(lighter(110.0, 2.0), -1.0));
        assert!(near(lighter(-10.0, 2.0), -1.0));
        assert!(near(lighter_unsafe(100.0, 2.0), 100.0));
        assert!(near(darker(10.0, 20.0), -1.0));
        assert!(near(darker(110.0, 2.0), -1.0));
        assert!(near(darker(-10.0, 2.0), -1.0));
        assert!(near(darker_unsafe(0.0, 2.0), 0.0));
    }

    // MCU@194144d dislike_analyzer_test.dart (`isDisliked` == `fixIfDisliked` moves the colour).
    #[test]
    fn mcu_dislike_analyzer() {
        let monk_skin_tones = [
            0xf6ede4, 0xf3e7db, 0xf7ead0, 0xeadaba, 0xd7bd96, 0xa07e56, 0x825c43, 0x604134,
            0x3a312a, 0x292420,
        ];
        for c in monk_skin_tones {
            let h = Hct::new(rgb(c));
            assert_eq!(
                hex(Argb::from(fix_if_disliked(h))),
                c,
                "{c:06x} was disliked"
            );
        }
        for c in [0x95884B, 0x716B40, 0xB08E00, 0x4C4308, 0x464521] {
            let fixed = fix_if_disliked(Hct::new(rgb(c)));
            assert_ne!(hex(Argb::from(fixed)), c, "{c:06x} was likable");
            let again = fix_if_disliked(fixed);
            assert_eq!(
                Argb::from(again),
                Argb::from(fixed),
                "{c:06x} stayed disliked"
            );
        }
        let tone67 = Hct::from(100.0, 50.0, 67.0);
        assert_eq!(Argb::from(fix_if_disliked(tone67)), Argb::from(tone67));
    }

    // MCU@194144d dynamic_scheme_test.dart.
    #[test]
    fn mcu_rotated_hue() {
        let src = Hct::from(43.0, 16.0, 16.0);
        let near = |a: f64, b: f64| (a - b).abs() < 1.0;
        assert!(near(rotated_hue(&src, &[0.0], &[0.0]), 43.0));
        assert!(near(
            rotated_hue(&src, &[0.0, 42.0, 360.0], &[0.0, 15.0, 0.0]),
            58.0
        ));
        assert!(near(
            rotated_hue(&src, &[0.0, 42.0, 360.0], &[0.0, 480.0, 0.0]),
            163.0
        ));
    }

    // MCU@194144d dynamic_scheme_test.dart '0 length input'; materialyoucolor 2.0.10
    // `get_rotated_hue(c, [], [])` also returns the source hue.
    #[test]
    fn mcu_rotated_hue_empty_tables() {
        let src = Hct::from(43.0, 16.0, 16.0);
        assert!((rotated_hue(&src, &[], &[]) - 43.0).abs() < 1.0);
    }

    // MCU@194144d palettes_test.dart group 'KeyColor'.
    #[test]
    fn mcu_key_color() {
        let key = |hue, chroma| {
            let p = Palette::from_hue_and_chroma(hue, chroma);
            Hct::new(p.argb(p.key_tone))
        };
        let k = key(50.0, 60.0);
        assert!((k.get_hue() - 50.0).abs() < 10.0);
        assert!((k.get_chroma() - 60.0).abs() < 0.5);
        assert!(k.get_tone() > 0.0 && k.get_tone() < 100.0);
        // Hue 149 peaks at chroma 89.6 (tone 87.9).
        let k = key(149.0, 200.0);
        assert!((k.get_hue() - 149.0).abs() < 10.0);
        assert!(k.get_chroma() > 89.0);
        assert!(k.get_tone() > 0.0 && k.get_tone() < 100.0);
        let k = key(50.0, 3.0);
        assert!((k.get_hue() - 50.0).abs() < 10.0);
        assert!((k.get_chroma() - 3.0).abs() < 0.5);
        assert!((k.get_tone() - 50.0).abs() < 0.5);
    }

    // MCU@194144d palettes_test.dart 'tones of blue'.
    #[test]
    fn mcu_tones_of_blue() {
        let p = Palette::from_hct(Hct::new(rgb(0x0000ff)));
        let expected = [
            (0.0, 0x000000),
            (3.0, 0x00003c),
            (10.0, 0x00006e),
            (20.0, 0x0001ac),
            (30.0, 0x0000ef),
            (40.0, 0x343dff),
            (50.0, 0x5a64ff),
            (60.0, 0x7c84ff),
            (70.0, 0x9da3ff),
            (80.0, 0xbec2ff),
            (90.0, 0xe0e0ff),
            (95.0, 0xf1efff),
            (99.0, 0xfffbff),
            (100.0, 0xffffff),
        ];
        for (tone, want) in expected {
            assert_eq!(hex(p.argb(tone)), want, "tone {tone}");
        }
    }

    // MCU@194144d scheme_monochrome_test.dart (seed 0xff0000ff, contrast 0). Left out because
    // materialyoucolor 2.0.10 differs (its values are in the PY scheme grid): light primary,
    // onPrimary, primaryContainer, onPrimaryContainer, tertiaryContainer, onTertiaryContainer
    // (python keeps the dark-mode tones: 100/10/85/0 resp. its `not is_monochrome` branch) and
    // dark tertiaryContainer, onTertiaryContainer.
    #[test]
    fn mcu_monochrome_spec() {
        use Tok::*;
        let dark: &[(Tok, f64)] = &[
            (primary, 100.0),
            (onPrimary, 10.0),
            (primaryContainer, 85.0),
            (onPrimaryContainer, 0.0),
            (secondary, 80.0),
            (onSecondary, 10.0),
            (secondaryContainer, 30.0),
            (onSecondaryContainer, 90.0),
            (tertiary, 90.0),
            (onTertiary, 10.0),
        ];
        let light: &[(Tok, f64)] = &[
            (secondary, 40.0),
            (onSecondary, 100.0),
            (secondaryContainer, 85.0),
            (onSecondaryContainer, 10.0),
            (tertiary, 25.0),
            (onTertiary, 90.0),
        ];
        for (is_dark, want) in [(true, dark), (false, light)] {
            let s = scheme(Monochrome, 0x0000ff, is_dark, 0.0);
            for &(t, tone) in want {
                let got = rendered_tone(&s, t);
                assert!(
                    (got - tone).abs() < 1.0,
                    "dark={is_dark} {t:?}: {got} != {tone}"
                );
            }
        }
    }

    // MCU@194144d dynamic_color_test.dart 'Values are correct'.
    #[test]
    fn mcu_dynamic_color_values() {
        use Tok::*;
        let cases = [
            (onPrimaryContainer, Fidelity, 0xff0000, false, 0.5, 0xffffff),
            (
                onSecondaryContainer,
                Content,
                0x0000ff,
                false,
                0.5,
                0xffffff,
            ),
            (onTertiaryContainer, Content, 0xffff00, true, -0.5, 0x959b1a),
            (inverseSurface, Content, 0x0000ff, false, 0.0, 0x2f2f3b),
            // MCU expects 0xff422f; materialyoucolor 2.0.10 gives 0xff5540 (PY).
            (inversePrimary, Content, 0xff0000, false, -0.5, 0xff5540),
            (outlineVariant, Content, 0xffff00, true, 0.0, 0x484831),
        ];
        for (t, v, seed, dark, contrast, want) in cases {
            let got = hex(scheme(v, seed, dark, contrast).argb(t));
            assert_eq!(got, want, "{t:?} {v:?} {seed:06x} dark={dark} {contrast}");
        }
    }

    // MCU@194144d dynamic_color_test.dart: text/surface pairs keep 4.5:1 (3:1 at reduced
    // contrast). MCU checks Content, Monochrome, TonalSpot and Fidelity; materialyoucolor 2.0.10
    // satisfies it for all nine variants, so all nine are checked.
    #[test]
    fn mcu_text_surface_pairs_meet_contrast() {
        use Tok::*;
        let pairs = [
            (onPrimary, primary),
            (onPrimaryContainer, primaryContainer),
            (onSecondary, secondary),
            (onSecondaryContainer, secondaryContainer),
            (onTertiary, tertiary),
            (onTertiaryContainer, tertiaryContainer),
            (onError, error),
            (onErrorContainer, errorContainer),
            (onBackground, background),
            (onSurfaceVariant, surfaceBright),
            (onSurfaceVariant, surfaceDim),
            (inverseOnSurface, inverseSurface),
        ];
        let variants = [
            Monochrome, Neutral, TonalSpot, Vibrant, Expressive, Fidelity, Content, Rainbow,
            FruitSalad,
        ];
        for seed in [0xff0000, 0xffff00, 0x00ff00, 0x0000ff] {
            for contrast in [-1.0, -0.5, 0.0, 0.5, 1.0] {
                for dark in [false, true] {
                    for v in variants {
                        let s = scheme(v, seed, dark, contrast);
                        let min = if contrast >= 0.0 { 4.5 } else { 3.0 };
                        for (fg, bg) in pairs {
                            let r = ratio_of_tones(rendered_tone(&s, fg), rendered_tone(&s, bg));
                            assert!(
                                r >= min,
                                "{v:?} {seed:06x} {contrast} dark={dark} {fg:?}/{bg:?}: {r}"
                            );
                        }
                    }
                }
            }
        }
    }

    // MCU@194144d dynamic_color_test.dart 'fixed colors in ...' (seed 0xffff0000, contrast 0).
    // Monochrome onPrimaryFixed / onTertiaryFixed are left out: MCU expects tone 100,
    // materialyoucolor 2.0.10 gives 0 (PY scheme grid).
    #[test]
    fn mcu_fixed_colors() {
        use Tok::*;
        let fixed = [
            primaryFixed,
            primaryFixedDim,
            onPrimaryFixed,
            onPrimaryFixedVariant,
            secondaryFixed,
            secondaryFixedDim,
            onSecondaryFixed,
            onSecondaryFixedVariant,
            tertiaryFixed,
            tertiaryFixedDim,
            onTertiaryFixed,
            onTertiaryFixedVariant,
        ];
        let tonal = [
            90.0, 80.0, 10.0, 30.0, 90.0, 80.0, 10.0, 30.0, 90.0, 80.0, 10.0, 30.0,
        ];
        let mono = [
            40.0, 30.0, -1.0, 90.0, 80.0, 70.0, 10.0, 25.0, 40.0, 30.0, -1.0, 90.0,
        ];
        for (v, dark, want) in [
            (TonalSpot, true, tonal),
            (Monochrome, false, mono),
            (Monochrome, true, mono),
        ] {
            let s = scheme(v, 0xff0000, dark, 0.0);
            for (t, tone) in fixed.into_iter().zip(want) {
                if tone < 0.0 {
                    continue;
                }
                let got = rendered_tone(&s, t);
                assert!(
                    (got - tone).abs() < 1.0,
                    "{v:?} dark={dark} {t:?}: {got} != {tone}"
                );
            }
        }
    }

    // MCU@ab1be2d dart/test/scheme_*_test.dart: every expectation materialyoucolor 2.0.10 also
    // meets (216 of 335; the rest predate the python port's key-colour search, contrast curves and
    // token quirks and are covered by the PY scheme grid).
    #[test]
    fn mcu_scheme_values() {
        use Tok::*;
        #[rustfmt::skip]
        let cases: &[(Variant, u32, bool, f64, Tok, u32)] = &[
            (Content, 0x0000FF, false, 0.0, primary_paletteKeyColor, 0x080CFF),
            (Content, 0x0000FF, false, 0.0, secondary_paletteKeyColor, 0x656DD3),
            (Content, 0x0000FF, false, 0.0, tertiary_paletteKeyColor, 0x81009F),
            (Content, 0x0000FF, false, 0.0, neutral_paletteKeyColor, 0x767684),
            (Content, 0x0000FF, false, 0.0, neutral_variant_paletteKeyColor, 0x757589),
            (Content, 0x0000FF, false, -1.0, primary, 0x5660FF),
            (Content, 0x0000FF, false, -1.0, primaryContainer, 0xD5D6FF),
            (Content, 0x0000FF, false, -1.0, tertiaryContainer, 0xFAC9FF),
            (Content, 0x850096, false, -1.0, tertiaryContainer, 0xFFCCD7),
            (Content, 0x0000FF, false, 1.0, onPrimaryContainer, 0xFFFFFF),
            (Content, 0x0000FF, false, -1.0, surface, 0xFBF8FF),
            (Content, 0x0000FF, false, 0.0, surface, 0xFBF8FF),
            (Content, 0x0000FF, false, 1.0, surface, 0xFBF8FF),
            (Content, 0x0000FF, true, -1.0, primary, 0x7C84FF),
            (Content, 0x0000FF, true, 0.0, primary, 0xBEC2FF),
            (Content, 0x0000FF, true, -1.0, primaryContainer, 0x0001C9),
            (Content, 0x0000FF, true, 1.0, onTertiaryContainer, 0x000000),
            (Content, 0x0000FF, true, -1.0, surface, 0x12121D),
            (Content, 0x0000FF, true, 0.0, surface, 0x12121D),
            (Content, 0x0000FF, true, 1.0, surface, 0x12121D),
            (Expressive, 0x0000FF, false, 0.0, primary_paletteKeyColor, 0x35855F),
            (Expressive, 0x0000FF, false, 0.0, secondary_paletteKeyColor, 0x8C6D8C),
            (Expressive, 0x0000FF, false, 0.0, tertiary_paletteKeyColor, 0x806EA1),
            (Expressive, 0x0000FF, false, 0.0, neutral_paletteKeyColor, 0x79757F),
            (Expressive, 0x0000FF, false, 0.0, neutral_variant_paletteKeyColor, 0x7A7585),
            (Expressive, 0x0000FF, false, -1.0, primary, 0x32835D),
            (Expressive, 0x0000FF, false, 0.0, primary, 0x146C48),
            (Expressive, 0x0000FF, false, -1.0, primaryContainer, 0x99EABD),
            (Expressive, 0x0000FF, false, 0.0, primaryContainer, 0xA2F4C6),
            (Expressive, 0x0000FF, false, 1.0, onPrimaryContainer, 0xFFFFFF),
            (Expressive, 0x0000FF, false, -1.0, surface, 0xFDF7FF),
            (Expressive, 0x0000FF, false, 0.0, surface, 0xFDF7FF),
            (Expressive, 0x0000FF, false, 1.0, surface, 0xFDF7FF),
            (Expressive, 0x0000FF, true, -1.0, primary, 0x51A078),
            (Expressive, 0x0000FF, true, 0.0, primary, 0x87D7AB),
            (Expressive, 0x0000FF, true, -1.0, primaryContainer, 0x00432A),
            (Expressive, 0x0000FF, true, 0.0, primaryContainer, 0x005234),
            (Expressive, 0x0000FF, true, 0.0, onPrimaryContainer, 0xA2F4C6),
            (Expressive, 0x0000FF, true, -1.0, surface, 0x14121A),
            (Expressive, 0x0000FF, true, 0.0, surface, 0x14121A),
            (Expressive, 0x0000FF, true, 1.0, surface, 0x14121A),
            (Fidelity, 0x0000FF, false, 0.0, primary_paletteKeyColor, 0x080CFF),
            (Fidelity, 0x0000FF, false, 0.0, secondary_paletteKeyColor, 0x656DD3),
            (Fidelity, 0x0000FF, false, 0.0, tertiary_paletteKeyColor, 0x9D0002),
            (Fidelity, 0x0000FF, false, 0.0, neutral_paletteKeyColor, 0x767684),
            (Fidelity, 0x0000FF, false, 0.0, neutral_variant_paletteKeyColor, 0x757589),
            (Fidelity, 0x0000FF, false, -1.0, primary, 0x5660FF),
            (Fidelity, 0x0000FF, false, -1.0, primaryContainer, 0xD5D6FF),
            (Fidelity, 0x0000FF, false, -1.0, tertiaryContainer, 0xFFCDC6),
            (Fidelity, 0x850096, false, -1.0, tertiaryContainer, 0xEBD982),
            (Fidelity, 0x0000FF, false, 1.0, onPrimaryContainer, 0xFFFFFF),
            (Fidelity, 0x0000FF, false, -1.0, surface, 0xFBF8FF),
            (Fidelity, 0x0000FF, false, 0.0, surface, 0xFBF8FF),
            (Fidelity, 0x0000FF, false, 1.0, surface, 0xFBF8FF),
            (Fidelity, 0x0000FF, true, -1.0, primary, 0x7C84FF),
            (Fidelity, 0x0000FF, true, 0.0, primary, 0xBEC2FF),
            (Fidelity, 0x0000FF, true, -1.0, primaryContainer, 0x0001C9),
            (Fidelity, 0x0000FF, true, 1.0, onTertiaryContainer, 0x000000),
            (Fidelity, 0x0000FF, true, -1.0, surface, 0x12121D),
            (Fidelity, 0x0000FF, true, 0.0, surface, 0x12121D),
            (Fidelity, 0x0000FF, true, 1.0, surface, 0x12121D),
            (FruitSalad, 0x0000FF, false, 0.0, secondary_paletteKeyColor, 0x3A7E9E),
            (FruitSalad, 0x0000FF, false, 0.0, tertiary_paletteKeyColor, 0x6E72AC),
            (FruitSalad, 0x0000FF, false, 0.0, neutral_paletteKeyColor, 0x777682),
            (FruitSalad, 0x0000FF, false, 0.0, neutral_variant_paletteKeyColor, 0x75758B),
            (FruitSalad, 0x0000FF, false, -1.0, primary, 0x007EA7),
            (FruitSalad, 0x0000FF, false, 0.0, primary, 0x006688),
            (FruitSalad, 0x0000FF, false, -1.0, primaryContainer, 0xAAE0FF),
            (FruitSalad, 0x0000FF, false, 0.0, primaryContainer, 0xC2E8FF),
            (FruitSalad, 0x0000FF, false, -1.0, tertiaryContainer, 0xD5D6FF),
            (FruitSalad, 0x0000FF, false, 1.0, onPrimaryContainer, 0xFFFFFF),
            (FruitSalad, 0x0000FF, false, -1.0, surface, 0xFBF8FF),
            (FruitSalad, 0x0000FF, false, 0.0, surface, 0xFBF8FF),
            (FruitSalad, 0x0000FF, false, 1.0, surface, 0xFBF8FF),
            (FruitSalad, 0x0000FF, false, 0.0, secondary, 0x196584),
            (FruitSalad, 0x0000FF, true, -1.0, primary, 0x1E9BCB),
            (FruitSalad, 0x0000FF, true, 0.0, primary, 0x76D1FF),
            (FruitSalad, 0x0000FF, true, -1.0, primaryContainer, 0x003F56),
            (FruitSalad, 0x0000FF, true, 0.0, primaryContainer, 0x004D67),
            (FruitSalad, 0x0000FF, true, 0.0, onPrimaryContainer, 0xC2E8FF),
            (FruitSalad, 0x0000FF, true, 1.0, onTertiaryContainer, 0x000000),
            (FruitSalad, 0x0000FF, true, -1.0, surface, 0x12131C),
            (FruitSalad, 0x0000FF, true, 0.0, surface, 0x12131C),
            (FruitSalad, 0x0000FF, true, 1.0, surface, 0x12131C),
            (FruitSalad, 0x0000FF, true, 0.0, secondary, 0x8ECFF2),
            (Monochrome, 0x0000FF, false, -1.0, primary, 0x747474),
            (Monochrome, 0x0000FF, false, -1.0, primaryContainer, 0xD9D9D9),
            (Monochrome, 0x0000FF, false, 1.0, onPrimaryContainer, 0xFFFFFF),
            (Monochrome, 0x0000FF, false, -1.0, surface, 0xF9F9F9),
            (Monochrome, 0x0000FF, false, 0.0, surface, 0xF9F9F9),
            (Monochrome, 0x0000FF, false, 1.0, surface, 0xF9F9F9),
            (Monochrome, 0x0000FF, true, -1.0, primary, 0x919191),
            (Monochrome, 0x0000FF, true, 0.0, primary, 0xFFFFFF),
            (Monochrome, 0x0000FF, true, 1.0, primary, 0xFFFFFF),
            (Monochrome, 0x0000FF, true, -1.0, primaryContainer, 0x3A3A3A),
            (Monochrome, 0x0000FF, true, 0.0, primaryContainer, 0xD4D4D4),
            (Monochrome, 0x0000FF, true, 1.0, primaryContainer, 0xD4D4D4),
            (Monochrome, 0x0000FF, true, 0.0, onPrimaryContainer, 0x000000),
            (Monochrome, 0x0000FF, true, 1.0, onPrimaryContainer, 0x000000),
            (Monochrome, 0x0000FF, true, -1.0, surface, 0x131313),
            (Monochrome, 0x0000FF, true, 0.0, surface, 0x131313),
            (Monochrome, 0x0000FF, true, 1.0, surface, 0x131313),
            (Neutral, 0x0000FF, false, 0.0, primary_paletteKeyColor, 0x767685),
            (Neutral, 0x0000FF, false, 0.0, secondary_paletteKeyColor, 0x777680),
            (Neutral, 0x0000FF, false, 0.0, tertiary_paletteKeyColor, 0x75758B),
            (Neutral, 0x0000FF, false, 0.0, neutral_paletteKeyColor, 0x787678),
            (Neutral, 0x0000FF, false, 0.0, neutral_variant_paletteKeyColor, 0x787678),
            (Neutral, 0x0000FF, false, -1.0, primary, 0x737383),
            (Neutral, 0x0000FF, false, 0.0, primary, 0x5D5D6C),
            (Neutral, 0x0000FF, false, -1.0, primaryContainer, 0xD9D7E9),
            (Neutral, 0x0000FF, false, 0.0, primaryContainer, 0xE2E1F3),
            (Neutral, 0x0000FF, false, 1.0, onPrimaryContainer, 0xFFFFFF),
            (Neutral, 0x0000FF, false, -1.0, surface, 0xFCF8FA),
            (Neutral, 0x0000FF, false, 0.0, surface, 0xFCF8FA),
            (Neutral, 0x0000FF, false, 1.0, surface, 0xFCF8FA),
            (Neutral, 0x0000FF, true, -1.0, primary, 0x908F9F),
            (Neutral, 0x0000FF, true, 0.0, primary, 0xC6C5D6),
            (Neutral, 0x0000FF, true, -1.0, primaryContainer, 0x393947),
            (Neutral, 0x0000FF, true, 0.0, primaryContainer, 0x454654),
            (Neutral, 0x0000FF, true, 0.0, onPrimaryContainer, 0xE2E1F3),
            (Neutral, 0x0000FF, true, 1.0, onTertiaryContainer, 0x000000),
            (Neutral, 0x0000FF, true, -1.0, surface, 0x131315),
            (Neutral, 0x0000FF, true, 0.0, surface, 0x131315),
            (Neutral, 0x0000FF, true, 1.0, surface, 0x131315),
            (Rainbow, 0x0000FF, false, 0.0, primary_paletteKeyColor, 0x696FC4),
            (Rainbow, 0x0000FF, false, 0.0, secondary_paletteKeyColor, 0x75758B),
            (Rainbow, 0x0000FF, false, 0.0, tertiary_paletteKeyColor, 0x936B84),
            (Rainbow, 0x0000FF, false, -1.0, primary, 0x676DC1),
            (Rainbow, 0x0000FF, false, 0.0, primary, 0x5056A9),
            (Rainbow, 0x0000FF, false, -1.0, primaryContainer, 0xD5D6FF),
            (Rainbow, 0x0000FF, false, 0.0, primaryContainer, 0xE0E0FF),
            (Rainbow, 0x0000FF, false, -1.0, tertiaryContainer, 0xFBCBE7),
            (Rainbow, 0x0000FF, false, 1.0, onPrimaryContainer, 0xFFFFFF),
            (Rainbow, 0x0000FF, false, -1.0, surface, 0xF9F9F9),
            (Rainbow, 0x0000FF, false, 0.0, surface, 0xF9F9F9),
            (Rainbow, 0x0000FF, false, 1.0, surface, 0xF9F9F9),
            (Rainbow, 0x0000FF, false, 0.0, secondary, 0x5C5D72),
            (Rainbow, 0x0000FF, true, -1.0, primary, 0x8389E0),
            (Rainbow, 0x0000FF, true, 0.0, primary, 0xBEC2FF),
            (Rainbow, 0x0000FF, true, -1.0, primaryContainer, 0x2A3082),
            (Rainbow, 0x0000FF, true, 0.0, primaryContainer, 0x383E8F),
            (Rainbow, 0x0000FF, true, 0.0, onPrimaryContainer, 0xE0E0FF),
            (Rainbow, 0x0000FF, true, 1.0, onTertiaryContainer, 0x000000),
            (Rainbow, 0x0000FF, true, -1.0, surface, 0x131313),
            (Rainbow, 0x0000FF, true, 0.0, surface, 0x131313),
            (Rainbow, 0x0000FF, true, 1.0, surface, 0x131313),
            (Rainbow, 0x0000FF, true, 0.0, secondary, 0xC5C4DD),
            (Rainbow, 0x0000FF, true, 0.0, secondaryContainer, 0x444559),
            (TonalSpot, 0x0000FF, false, 0.0, primary_paletteKeyColor, 0x6E72AC),
            (TonalSpot, 0x0000FF, false, 0.0, secondary_paletteKeyColor, 0x75758B),
            (TonalSpot, 0x0000FF, false, 0.0, tertiary_paletteKeyColor, 0x936B84),
            (TonalSpot, 0x0000FF, false, 0.0, neutral_paletteKeyColor, 0x77767D),
            (TonalSpot, 0x0000FF, false, 0.0, neutral_variant_paletteKeyColor, 0x777680),
            (TonalSpot, 0x0000FF, false, -1.0, primary, 0x6C70AA),
            (TonalSpot, 0x0000FF, false, 0.0, primary, 0x555992),
            (TonalSpot, 0x0000FF, false, -1.0, primaryContainer, 0xD5D6FF),
            (TonalSpot, 0x0000FF, false, 0.0, primaryContainer, 0xE0E0FF),
            (TonalSpot, 0x0000FF, false, 1.0, onPrimaryContainer, 0xFFFFFF),
            (TonalSpot, 0x0000FF, false, -1.0, surface, 0xFBF8FF),
            (TonalSpot, 0x0000FF, false, 0.0, surface, 0xFBF8FF),
            (TonalSpot, 0x0000FF, false, 1.0, surface, 0xFBF8FF),
            (TonalSpot, 0x0000FF, false, -1.0, onSurface, 0x5F5E65),
            (TonalSpot, 0x0000FF, false, 0.0, onSurface, 0x1B1B21),
            (TonalSpot, 0x0000FF, false, 1.0, onSurface, 0x000000),
            (TonalSpot, 0x0000FF, false, -1.0, onSecondary, 0xFFFBFF),
            (TonalSpot, 0x0000FF, false, 0.0, onSecondary, 0xFFFFFF),
            (TonalSpot, 0x0000FF, false, 1.0, onSecondary, 0xFFFFFF),
            (TonalSpot, 0x0000FF, false, -1.0, onTertiary, 0xFFFBFF),
            (TonalSpot, 0x0000FF, false, 0.0, onTertiary, 0xFFFFFF),
            (TonalSpot, 0x0000FF, false, 1.0, onTertiary, 0xFFFFFF),
            (TonalSpot, 0x0000FF, false, -1.0, onError, 0xFFFBFF),
            (TonalSpot, 0x0000FF, false, 0.0, onError, 0xFFFFFF),
            (TonalSpot, 0x0000FF, false, 1.0, onError, 0xFFFFFF),
            (TonalSpot, 0x0000FF, true, -1.0, primary, 0x888CC8),
            (TonalSpot, 0x0000FF, true, 0.0, primary, 0xBEC2FF),
            (TonalSpot, 0x0000FF, true, -1.0, primaryContainer, 0x31356B),
            (TonalSpot, 0x0000FF, true, 0.0, primaryContainer, 0x3E4278),
            (TonalSpot, 0x0000FF, true, 0.0, onPrimaryContainer, 0xE0E0FF),
            (TonalSpot, 0x0000FF, true, 1.0, onTertiaryContainer, 0x000000),
            (TonalSpot, 0x0000FF, true, -1.0, onSecondary, 0x27283B),
            (TonalSpot, 0x0000FF, true, 0.0, onSecondary, 0x2E2F42),
            (TonalSpot, 0x0000FF, true, 1.0, onSecondary, 0x000000),
            (TonalSpot, 0x0000FF, true, -1.0, onTertiary, 0x3E1F34),
            (TonalSpot, 0x0000FF, true, 0.0, onTertiary, 0x46263B),
            (TonalSpot, 0x0000FF, true, 1.0, onTertiary, 0x000000),
            (TonalSpot, 0x0000FF, true, -1.0, onError, 0x5C0003),
            (TonalSpot, 0x0000FF, true, 0.0, onError, 0x690005),
            (TonalSpot, 0x0000FF, true, 1.0, onError, 0x000000),
            (TonalSpot, 0x0000FF, true, -1.0, surface, 0x131318),
            (TonalSpot, 0x0000FF, true, 0.0, surface, 0x131318),
            (TonalSpot, 0x0000FF, true, 1.0, surface, 0x131318),
            (TonalSpot, 0x0000FF, true, -1.0, onSurface, 0xA4A2A9),
            (TonalSpot, 0x0000FF, true, 0.0, onSurface, 0xE4E1E9),
            (TonalSpot, 0x0000FF, true, 1.0, onSurface, 0xFFFFFF),
            (Vibrant, 0x0000FF, false, 0.0, primary_paletteKeyColor, 0x080CFF),
            (Vibrant, 0x0000FF, false, 0.0, secondary_paletteKeyColor, 0x7B7296),
            (Vibrant, 0x0000FF, false, 0.0, tertiary_paletteKeyColor, 0x886C9D),
            (Vibrant, 0x0000FF, false, 0.0, neutral_paletteKeyColor, 0x777682),
            (Vibrant, 0x0000FF, false, 0.0, neutral_variant_paletteKeyColor, 0x767685),
            (Vibrant, 0x0000FF, false, -1.0, primary, 0x5660FF),
            (Vibrant, 0x0000FF, false, 0.0, primary, 0x343DFF),
            (Vibrant, 0x0000FF, false, -1.0, primaryContainer, 0xD5D6FF),
            (Vibrant, 0x0000FF, false, 0.0, primaryContainer, 0xE0E0FF),
            (Vibrant, 0x0000FF, false, 1.0, onPrimaryContainer, 0xFFFFFF),
            (Vibrant, 0x0000FF, false, -1.0, surface, 0xFBF8FF),
            (Vibrant, 0x0000FF, false, 0.0, surface, 0xFBF8FF),
            (Vibrant, 0x0000FF, false, 1.0, surface, 0xFBF8FF),
            (Vibrant, 0x0000FF, true, -1.0, primary, 0x7C84FF),
            (Vibrant, 0x0000FF, true, 0.0, primary, 0xBEC2FF),
            (Vibrant, 0x0000FF, true, -1.0, primaryContainer, 0x0001C9),
            (Vibrant, 0x0000FF, true, 0.0, primaryContainer, 0x0000EF),
            (Vibrant, 0x0000FF, true, 0.0, onPrimaryContainer, 0xE0E0FF),
            (Vibrant, 0x0000FF, true, 1.0, onTertiaryContainer, 0x000000),
            (Vibrant, 0x0000FF, true, -1.0, surface, 0x12131C),
            (Vibrant, 0x0000FF, true, 0.0, surface, 0x12131C),
            (Vibrant, 0x0000FF, true, 1.0, surface, 0x12131C),
        ];
        let bad: Vec<String> = cases
            .iter()
            .filter_map(|&(v, seed, dark, contrast, t, want)| {
                let got = hex(scheme(v, seed, dark, contrast).argb(t));
                (got != want).then(|| {
                    format!(
                        "{v:?} {seed:06x} dark={dark} {contrast} {t:?}: {got:06x} != {want:06x}"
                    )
                })
            })
            .collect();
        assert!(bad.is_empty(), "{}", bad.join("\n"));
    }

    // PY: vars(MaterialDynamicColors) order, the order generate_colors_material.py writes SCSS in.
    #[test]
    fn py_token_order() {
        let names: Vec<&str> = vectors("tokens").next().unwrap()[1..].to_vec();
        let ours: Vec<&str> = Tok::ALL.iter().map(|t| t.name()).collect();
        assert_eq!(ours, names);
    }

    // PY: every token of every variant, light and dark, contrast -1..1, three seeds.
    #[test]
    fn py_scheme_grid() {
        let names = vectors("tokens").next().unwrap()[1..].to_vec();
        let toks: Vec<Tok> = names.iter().map(|n| tok(n)).collect();
        let mut rows = 0;
        let mut bad = Vec::new();
        for f in vectors("scheme") {
            let (v, seed) = (
                variant(f[1]),
                u32::from_str_radix(f[2], 16).unwrap() & 0xffffff,
            );
            let (dark, contrast) = (f[3] == "1", num(f[4]));
            let s = scheme(v, seed, dark, contrast);
            assert_eq!(f[5].len(), 6 * toks.len());
            for (i, &t) in toks.iter().enumerate() {
                let want = u32::from_str_radix(&f[5][6 * i..6 * i + 6], 16).unwrap();
                let got = hex(s.argb(t));
                if got != want {
                    bad.push(format!(
                        "{v:?} {seed:06x} dark={dark} {contrast} {t:?}: {got:06x} != {want:06x}"
                    ));
                }
            }
            rows += 1;
        }
        assert_eq!(rows, 9 * 3 * 2 * 5);
        assert!(bad.is_empty(), "{} differ:\n{}", bad.len(), bad.join("\n"));
    }

    // PY: `KeyColor.create` tone for hue 0..345 x chroma 0..200.
    #[test]
    fn py_key_color_search() {
        let mut n = 0;
        for f in vectors("key") {
            let (hue, chroma, want) = (num(f[1]), num(f[2]), num(f[3]));
            let got = Palette::from_hue_and_chroma(hue, chroma).key_tone;
            assert!(
                (got - want).abs() < 1e-9,
                "hue {hue} chroma {chroma}: {got} != {want}"
            );
            n += 1;
        }
        assert_eq!(n, 24 * 11);
    }

    // PY: `Contrast.*` and `DynamicColor.foreground_tone` over a tone x ratio grid.
    #[test]
    fn py_contrast_functions() {
        let mut n = 0;
        for f in vectors("contrast") {
            let (a, b, want) = (num(f[2]), num(f[3]), num(f[4]));
            let got = match f[1] {
                "ratio_of_tones" => ratio_of_tones(a, b),
                "lighter" => lighter(a, b),
                "darker" => darker(a, b),
                "lighter_unsafe" => lighter_unsafe(a, b),
                "darker_unsafe" => darker_unsafe(a, b),
                "foreground_tone" => foreground_tone(a, b),
                other => panic!("{other}"),
            };
            assert!(
                (got - want).abs() < 1e-9,
                "{}({a}, {b}): {got} != {want}",
                f[1]
            );
            n += 1;
        }
        assert_eq!(n, 12 * 12 + 5 * 19 * 7);
    }

    // PY: `DislikeAnalyzer.fix_if_disliked` around the disliked hue band.
    #[test]
    fn py_fix_if_disliked() {
        let mut n = 0;
        for f in vectors("dislike") {
            let input = u32::from_str_radix(f[1], 16).unwrap();
            let want = u32::from_str_radix(f[2], 16).unwrap();
            let got = hex(Argb::from(fix_if_disliked(Hct::new(rgb(input)))));
            assert_eq!(got, want, "{input:06x}");
            n += 1;
        }
        assert_eq!(n, 9 * 4 * 5);
    }

    // PY: SchemeFidelity divides by the warmest-coldest temperature range (ZeroDivisionError for
    // black and white); SchemeContent does not. Tertiary values from materialyoucolor 2.0.10.
    #[test]
    fn py_zero_temperature_range() {
        for seed in [0x000000, 0xffffff] {
            assert!(Scheme::new(Hct::new(rgb(seed)), Fidelity, true, 0.0).is_err());
        }
        let cases = [
            (Fidelity, 0x808080, 0xccc5c3),
            (Content, 0x808080, 0xcac5c7),
            (Content, 0x000000, 0xc6c6c6),
            (Content, 0xffffff, 0xc6c6c7),
        ];
        for (v, seed, want) in cases {
            assert_eq!(
                hex(scheme(v, seed, true, 0.0).argb(Tok::tertiary)),
                want,
                "{v:?} {seed:06x}"
            );
        }
    }

    // generate_colors_material.py: `--scheme` -> scheme class.
    #[test]
    fn scheme_names() {
        let cases = [
            ("scheme-fruit-salad", FruitSalad),
            ("scheme-expressive", Expressive),
            ("scheme-monochrome", Monochrome),
            ("scheme-rainbow", Rainbow),
            ("scheme-tonal-spot", TonalSpot),
            ("scheme-neutral", Neutral),
            ("scheme-fidelity", Fidelity),
            ("scheme-content", Content),
            ("scheme-vibrant", Vibrant),
            ("vibrant", TonalSpot),
            ("", TonalSpot),
        ];
        for (name, v) in cases {
            assert_eq!(Variant::from_scheme_name(name), v, "{name}");
        }
    }
}
