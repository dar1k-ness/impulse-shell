//! The colour helpers kde-material-you-colors' palettes are built with (`utils/color_utils.py`),
//! kept bit-exact so Konsole and the Plasma schemes keep the same colours. Its Oklab blend uses
//! `1.055 * (x^(1/2.4) - 0.055)` as the sRGB transfer; that is part of the look, so it stays.

pub type Rgb = [u8; 3];

/// `#rgb` / `#rrggbb` / `#aarrggbb` (alpha dropped) or `r,g,b`.
pub fn parse(s: &str) -> Option<Rgb> {
    let s = s.trim();
    if let Some(h) = s.strip_prefix('#') {
        if !h.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let byte = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
        return match h.len() {
            3 => {
                let d = |i: usize| u8::from_str_radix(&h[i..i + 1], 16).ok().map(|v| v * 17);
                Some([d(0)?, d(1)?, d(2)?])
            }
            6 => Some([byte(0)?, byte(2)?, byte(4)?]),
            8 => Some([byte(2)?, byte(4)?, byte(6)?]),
            _ => None,
        };
    }
    let mut it = s.split(',').map(|p| p.trim().parse::<u8>().ok());
    let rgb = [it.next()??, it.next()??, it.next()??];
    it.next().is_none().then_some(rgb)
}

pub fn hex(c: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

/// Konsole / KConfig `r,g,b`.
pub fn triple(c: Rgb) -> String {
    format!("{},{},{}", c[0], c[1], c[2])
}

fn transfer_inv(x: f64) -> f64 {
    if x >= 0.04045 {
        ((x + 0.055) / 1.055).powf(2.4)
    } else {
        x / 12.92
    }
}

fn transfer(x: f64) -> f64 {
    if x >= 0.0031308 {
        1.055 * (x.powf(1.0 / 2.4) - 0.055)
    } else {
        12.92 * x
    }
}

fn to_int(c: f64) -> u8 {
    (transfer(c) * 255.0).round_ties_even().clamp(0.0, 255.0) as u8
}

fn linear(c: Rgb) -> [f64; 3] {
    c.map(|v| transfer_inv(v as f64 / 255.0))
}

/// `blendColors`: Oklab mix of `a` towards `b` by `ratio`.
pub fn blend(a: Rgb, b: Rgb, ratio: f64) -> Rgb {
    let lms = |c: Rgb| {
        let [r, g, b] = linear(c);
        [
            (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt(),
            (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt(),
            (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt(),
        ]
    };
    let (p, q, inv) = (lms(a), lms(b), 1.0 - ratio);
    let cube = |i: usize| {
        let x = p[i] * inv + q[i] * ratio;
        x * x * x
    };
    let (l, m, s) = (cube(0), cube(1), cube(2));
    [
        to_int(4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s),
        to_int(-1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s),
        to_int(-0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s),
    ]
}

pub fn luminance(c: Rgb) -> f64 {
    let [r, g, b] = linear(c);
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

pub fn contrast(lighter: Rgb, darker: Rgb) -> f64 {
    (luminance(lighter) + 0.05) / (luminance(darker) + 0.05)
}

/// `blend2contrast`: blend `c` towards `with` in 1% steps until it has `min` contrast against
/// `bg`. The original loops forever if `with` can never reach it; this stops at a full blend.
pub fn blend_to_contrast(c: Rgb, bg: Rgb, with: Rgb, min: f64, dark: bool) -> Rgb {
    let ratio_of = |x: Rgb| {
        if dark {
            contrast(x, bg)
        } else {
            contrast(bg, x)
        }
    };
    if ratio_of(c) >= min {
        return blend(c, with, 0.12);
    }
    let mut r = 0.0;
    loop {
        r += 0.01;
        let n = blend(c, with, r);
        if ratio_of(n) >= min || r >= 1.0 {
            return n;
        }
    }
}

fn rgb_to_hsv(c: Rgb) -> (f64, f64, f64) {
    let [r, g, b] = c.map(f64::from);
    let (maxc, minc) = (r.max(g).max(b), r.min(g).min(b));
    if minc == maxc {
        return (0.0, 0.0, maxc);
    }
    let range = maxc - minc;
    let (rc, gc, bc) = ((maxc - r) / range, (maxc - g) / range, (maxc - b) / range);
    let h = if r == maxc {
        bc - gc
    } else if g == maxc {
        2.0 + rc - bc
    } else {
        4.0 + gc - rc
    };
    ((h / 6.0).rem_euclid(1.0), range / maxc, maxc)
}

fn hsv_to_rgb(h: f64, s: f64, v: f64) -> Rgb {
    let px = |x: f64| x as u8;
    if s == 0.0 {
        return [px(v); 3];
    }
    let i = (h * 6.0) as i64;
    let f = h * 6.0 - i as f64;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    let (r, g, b) = match i.rem_euclid(6) {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    [px(r), px(g), px(b)]
}

/// `scale_saturation(c, amount)`.
pub fn with_saturation(c: Rgb, amount: f64) -> Rgb {
    let (h, _, v) = rgb_to_hsv(c);
    hsv_to_rgb(h, amount, v)
}

/// `lighteen_color`: lift a colour darker than luminance `min`, then blend 20% towards `with`.
pub fn lighten(c: Rgb, min: f64, with: Rgb) -> Rgb {
    let lum = luminance(c);
    let c = if lum < min {
        let (h, s, _) = rgb_to_hsv(c);
        hsv_to_rgb(h, s, 255.0 * (1.0 - lum))
    } else {
        c
    };
    blend(c, with, 0.2)
}

/// `hex2alpha`: `#AArrggbb` with `opacity` percent.
pub fn with_alpha(c: Rgb, opacity: f64) -> String {
    let a = (opacity * 2.55).ceil() as i64;
    format!("#{a:02x}{}", &hex(c)[1..])
}
