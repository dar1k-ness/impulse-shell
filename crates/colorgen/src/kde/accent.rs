//! The accent colour half of Plasma's `applyScheme` (plasma-workspace 6.8 `kcms/colors/
//! colorsapplicator.cpp`), which `plasma-apply-colorscheme` runs when kdeglobals has
//! `[General] AccentColor`: accent on the active/link/focus/hover colours (links made readable),
//! `TintFactor` OKLab tinting, accented selection/button backgrounds and titlebars. Kept
//! bit-exact with what Plasma writes, so the float types and roundings of `QColor`,
//! `QColorTransform` (sRGB <-> linear sRGB) and `KColorUtils` (HCY) are reproduced here as Qt 6
//! and KGuiAddons 6 do them.

use super::apply::{COLOR_KEYS, Notes};
use super::kconfig::Doc;

/// A `QColor` as Qt 6 stores it: 16-bit channels, or half floats for `ExtendedRgb`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum QColor {
    Invalid,
    /// r, g, b, a.
    Rgb([u16; 4]),
    /// r, g, b, a, each a value a `qfloat16` holds.
    Ext([f32; 4]),
}

const MAX: f32 = 65535.0;

/// `qRound(float)` on x86-64: add a signed half, truncate.
fn q_round(f: f32) -> i32 {
    (f + 0.5f32.copysign(f)) as i32
}

/// `qt_div_257` (qcolor_p.h).
fn div_257(x: u16) -> u8 {
    let x = x as u32 + 128;
    ((x - (x >> 8)) >> 8) as u8
}

fn f16(x: f32) -> f32 {
    half::f16::from_f32(x).to_f32()
}

impl QColor {
    pub fn rgb(r: u8, g: u8, b: u8) -> QColor {
        QColor::Rgb([r as u16 * 0x101, g as u16 * 0x101, b as u16 * 0x101, 0xffff])
    }

    /// `QColor::fromRgb(int, int, int, int)`.
    fn from_rgb(r: i32, g: i32, b: i32, a: i32) -> QColor {
        if [r, g, b, a].iter().any(|v| !(0..=255).contains(v)) {
            return QColor::Invalid;
        }
        QColor::Rgb([r, g, b, a].map(|v| v as u16 * 0x101))
    }

    /// `QColor::fromRgbF` / `setRgbF` on a fresh colour.
    pub fn from_rgb_f(r: f32, g: f32, b: f32, a: f32) -> QColor {
        if a < 0.0 || a > 1.0 {
            return QColor::Invalid;
        }
        if [r, g, b].iter().any(|&v| v < 0.0 || v > 1.0) {
            return QColor::Ext([f16(r), f16(g), f16(b), f16(a)]);
        }
        QColor::Rgb([r, g, b, a].map(|v| q_round(v * MAX) as u16))
    }

    pub fn is_valid(self) -> bool {
        self != QColor::Invalid
    }

    fn channel_f(self, i: usize) -> f32 {
        match self {
            QColor::Invalid => [0.0, 0.0, 0.0, 1.0][i],
            QColor::Rgb(c) => c[i] as f32 / MAX,
            QColor::Ext(c) => c[i],
        }
    }

    pub fn red_f(self) -> f32 {
        self.channel_f(0)
    }
    pub fn green_f(self) -> f32 {
        self.channel_f(1)
    }
    pub fn blue_f(self) -> f32 {
        self.channel_f(2)
    }
    pub fn alpha_f(self) -> f32 {
        self.channel_f(3)
    }

    pub fn to_rgb(self) -> QColor {
        match self {
            QColor::Ext([r, g, b, a]) => {
                let ch = |v: f32| q_round(MAX * v.clamp(0.0, 1.0)) as u16;
                QColor::Rgb([ch(r), ch(g), ch(b), q_round(MAX * a) as u16])
            }
            c => c,
        }
    }

    /// `red()`, `green()`, `blue()`, `alpha()`.
    pub fn rgba8(self) -> [u8; 4] {
        match self {
            QColor::Invalid => [0, 0, 0, 255],
            QColor::Rgb(c) => c.map(div_257),
            QColor::Ext([.., a]) => {
                let [r, g, b, _] = self.to_rgb().rgba8();
                [r, g, b, q_round(a * 255.0) as u8]
            }
        }
    }

    fn set_alpha_f(&mut self, alpha: f32) {
        match self {
            QColor::Ext(c) => c[3] = f16(alpha),
            QColor::Rgb(c) => c[3] = q_round(alpha * MAX) as u16,
            QColor::Invalid => {}
        }
    }

    /// `QColor(Qt::transparent)` equality.
    fn is_transparent(self) -> bool {
        self == QColor::Rgb([0; 4])
    }

    /// What `KConfigGroup::writeEntry(key, QColor)` writes.
    pub fn kconfig(self) -> String {
        if !self.is_valid() {
            return "invalid".into();
        }
        let [r, g, b, a] = self.rgba8();
        if a == 255 {
            format!("{r},{g},{b}")
        } else {
            format!("{r},{g},{b},{a}")
        }
    }

    /// `KConfigGroup::readEntry(key, QColor())`: `r,g,b[,a]`, `#hex` or an SVG colour name.
    pub fn read(v: Option<&str>) -> QColor {
        let Some(v) = v else { return QColor::Invalid };
        if v.is_empty() || v == "invalid" {
            return QColor::Invalid;
        }
        if let Some(h) = v.strip_prefix('#') {
            return from_hex(h).unwrap_or(QColor::Invalid);
        }
        if !v.contains(',') {
            return from_name(v).unwrap_or(QColor::Invalid);
        }
        let parts: Vec<Option<i32>> = v.split(',').map(|p| p.trim().parse::<i32>().ok()).collect();
        if !(3..=4).contains(&parts.len()) {
            return QColor::Invalid;
        }
        let mut c = [255u16; 4];
        for (i, p) in parts.iter().enumerate() {
            match p {
                Some(x) if (0..=255).contains(x) => c[i] = *x as u16,
                _ => return QColor::Invalid,
            }
        }
        QColor::Rgb(c.map(|x| x * 0x101))
    }
}

/// `QColor::fromString` for `#rgb`, `#rrggbb`, `#aarrggbb`, `#rrrgggbbb`, `#rrrrggggbbbb`.
fn from_hex(h: &str) -> Option<QColor> {
    if !h.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let x = |i: usize, n: usize| u32::from_str_radix(&h[i..i + n], 16).ok();
    let [r, g, b, a] = match h.len() {
        12 => [x(0, 4)?, x(4, 4)?, x(8, 4)?, 0xffff],
        9 => [x(0, 3)?, x(3, 3)?, x(6, 3)?]
            .map(|v| (v << 4) | (v >> 8))
            .into_iter()
            .chain([0xffff])
            .collect::<Vec<_>>()
            .try_into()
            .ok()?,
        8 => [
            x(2, 2)? * 0x101,
            x(4, 2)? * 0x101,
            x(6, 2)? * 0x101,
            x(0, 2)? * 0x101,
        ],
        6 => [x(0, 2)? * 0x101, x(2, 2)? * 0x101, x(4, 2)? * 0x101, 0xffff],
        3 => [
            x(0, 1)? * 0x1111,
            x(1, 1)? * 0x1111,
            x(2, 1)? * 0x1111,
            0xffff,
        ],
        _ => return None,
    };
    Some(QColor::Rgb([r as u16, g as u16, b as u16, a as u16]))
}

fn from_name(name: &str) -> Option<QColor> {
    let n: String = name
        .chars()
        .filter(|c| *c != ' ' && *c != '\t')
        .map(|c| c.to_ascii_lowercase())
        .collect();
    if n == "transparent" {
        return Some(QColor::Rgb([0; 4]));
    }
    let mut it = NAMES.split_ascii_whitespace();
    while let (Some(k), Some(v)) = (it.next(), it.next()) {
        if k == n {
            return from_hex(v);
        }
    }
    None
}

// ---- KColorUtils (KGuiAddons `kcolorutils.cpp`, `kcolorspaces.cpp`) ----

fn normalize(a: f64) -> f64 {
    if a < 1.0 {
        if a > 0.0 { a } else { 0.0 }
    } else {
        1.0
    }
}

fn q_max(a: f64, b: f64) -> f64 {
    if a < b { b } else { a }
}

fn q_min(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}

const YC: [f64; 3] = [0.2126, 0.7152, 0.0722];

fn gamma(n: f64) -> f64 {
    normalize(n).powf(2.2)
}

fn igamma(n: f64) -> f64 {
    normalize(n).powf(1.0 / 2.2)
}

fn lumag(r: f64, g: f64, b: f64) -> f64 {
    r * YC[0] + g * YC[1] + b * YC[2]
}

/// `KColorSpaces::KHCY`.
struct Hcy {
    h: f64,
    c: f64,
    y: f64,
    a: f64,
}

impl Hcy {
    fn new(color: QColor) -> Hcy {
        let r = gamma(color.red_f() as f64);
        let g = gamma(color.green_f() as f64);
        let b = gamma(color.blue_f() as f64);
        let a = color.alpha_f() as f64;
        let y = lumag(r, g, b);
        let p = q_max(q_max(r, g), b);
        let n = q_min(q_min(r, g), b);
        let d = 6.0 * (p - n);
        let h = if n == p {
            0.0
        } else if r == p {
            (g - b) / d
        } else if g == p {
            ((b - r) / d) + (1.0 / 3.0)
        } else {
            ((r - g) / d) + (2.0 / 3.0)
        };
        let c = if r == g && g == b {
            0.0
        } else {
            q_max((y - n) / y, (p - y) / (1.0 - y))
        };
        Hcy { h, c, y, a }
    }

    fn color(&self) -> QColor {
        let wrap = |a: f64| {
            let r = a % 1.0;
            if r < 0.0 {
                1.0 + r
            } else if r > 0.0 {
                r
            } else {
                0.0
            }
        };
        let h = wrap(self.h);
        let c = normalize(self.c);
        let y = normalize(self.y);
        let hs = h * 6.0;
        let (th, tm) = if hs < 1.0 {
            (hs, YC[0] + YC[1] * hs)
        } else if hs < 2.0 {
            let th = 2.0 - hs;
            (th, YC[1] + YC[0] * th)
        } else if hs < 3.0 {
            let th = hs - 2.0;
            (th, YC[1] + YC[2] * th)
        } else if hs < 4.0 {
            let th = 4.0 - hs;
            (th, YC[2] + YC[1] * th)
        } else if hs < 5.0 {
            let th = hs - 4.0;
            (th, YC[2] + YC[0] * th)
        } else {
            let th = 6.0 - hs;
            (th, YC[0] + YC[2] * th)
        };
        let (tp, to, tn) = if tm >= y {
            (
                y + y * c * (1.0 - tm) / tm,
                y + y * c * (th - tm) / tm,
                y - (y * c),
            )
        } else {
            (
                y + (1.0 - y) * c,
                y + (1.0 - y) * c * (th - tm) / (1.0 - tm),
                y - (1.0 - y) * c * tm / (1.0 - tm),
            )
        };
        let (p, o, n) = (igamma(tp) as f32, igamma(to) as f32, igamma(tn) as f32);
        let a = self.a as f32;
        if hs < 1.0 {
            QColor::from_rgb_f(p, o, n, a)
        } else if hs < 2.0 {
            QColor::from_rgb_f(o, p, n, a)
        } else if hs < 3.0 {
            QColor::from_rgb_f(n, p, o, a)
        } else if hs < 4.0 {
            QColor::from_rgb_f(n, o, p, a)
        } else if hs < 5.0 {
            QColor::from_rgb_f(o, n, p, a)
        } else {
            QColor::from_rgb_f(p, n, o, a)
        }
    }
}

pub fn luma(c: QColor) -> f64 {
    lumag(
        gamma(c.red_f() as f64),
        gamma(c.green_f() as f64),
        gamma(c.blue_f() as f64),
    )
}

pub fn contrast_ratio(c1: QColor, c2: QColor) -> f64 {
    let (y1, y2) = (luma(c1), luma(c2));
    if y1 > y2 {
        (y1 + 0.05) / (y2 + 0.05)
    } else {
        (y2 + 0.05) / (y1 + 0.05)
    }
}

/// `KColorUtils::lighten(color)` (amount 0.5, chroma gain 1).
pub fn lighten(color: QColor) -> QColor {
    let (ky, kc) = (0.5, 1.0);
    let mut c = Hcy::new(color);
    c.y = 1.0 - normalize((1.0 - c.y) * (1.0 - ky));
    c.c = 1.0 - normalize((1.0 - c.c) * kc);
    c.color()
}

/// `KColorUtils::darken(color)` (amount 0.5, chroma gain 1).
pub fn darken(color: QColor) -> QColor {
    let (ky, kc) = (0.5, 1.0);
    let mut c = Hcy::new(color);
    c.y = normalize(c.y * (1.0 - ky));
    c.c = normalize(c.c * kc);
    c.color()
}

// ---- QColorTransform between sRGB and linear sRGB (qcolortransform.cpp, qcolorspace.cpp) ----

/// `QColorTransferFunction` (ICC parametric curve type 4).
#[derive(Clone, Copy)]
struct Trc {
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    e: f32,
    f: f32,
    g: f32,
}

const LINEAR: Trc = Trc {
    a: 1.0,
    b: 0.0,
    c: 1.0,
    d: 0.0,
    e: 0.0,
    f: 0.0,
    g: 1.0,
};

fn srgb_trc() -> Trc {
    Trc {
        a: 1.0 / 1.055,
        b: 0.055 / 1.055,
        c: 1.0 / 12.92,
        d: 0.04045,
        e: 0.0,
        f: 0.0,
        g: 2.4,
    }
}

impl Trc {
    fn apply(&self, x: f32) -> f32 {
        if x < self.d {
            return self.c * x + self.f;
        }
        let t = (self.a * x + self.b).powf(self.g);
        if t.is_finite() && t > i16::MIN as f32 && t < i16::MAX as f32 {
            t + self.e
        } else if t > 0.0 {
            1.0
        } else {
            0.0
        }
    }

    fn inverted(&self) -> Trc {
        let d = self.c * self.d + self.f;
        let (c, f) = if self.c.is_normal() {
            (1.0 / self.c, -self.f / self.c)
        } else {
            (0.0, 0.0)
        };
        let mut valid = self.a.is_normal() && self.g.is_normal();
        let mut a = 0.0;
        if valid {
            a = (1.0 / self.a).powf(self.g);
            valid = a.is_finite();
        }
        let (a, b, e, g) = if valid {
            (a, -a * self.e, -self.b / self.a, 1.0 / self.g)
        } else {
            (0.0, 0.0, 1.0, 1.0)
        };
        Trc {
            a,
            b,
            c,
            d,
            e,
            f,
            g,
        }
    }

    fn apply_extended(&self, x: f32) -> f32 {
        self.apply(x.abs()).copysign(x)
    }

    fn apply_inverse_extended(&self, x: f32) -> f32 {
        self.inverted().apply(x.abs()).copysign(x)
    }
}

/// `QColorMatrix`, stored transposed like Qt's (r, g, b columns).
#[derive(Clone, Copy)]
struct Mat {
    r: [f32; 3],
    g: [f32; 3],
    b: [f32; 3],
}

impl Mat {
    fn det(&self) -> f32 {
        let (r, g, b) = (self.r, self.g, self.b);
        r[0] * (b[2] * g[1] - g[2] * b[1]) - r[1] * (b[2] * g[0] - g[2] * b[0])
            + r[2] * (b[1] * g[0] - g[1] * b[0])
    }

    fn inverted(&self) -> Mat {
        let det = 1.0 / self.det();
        let (r, g, b) = (self.r, self.g, self.b);
        Mat {
            r: [
                (g[1] * b[2] - b[1] * g[2]) * det,
                (b[1] * r[2] - r[1] * b[2]) * det,
                (r[1] * g[2] - g[1] * r[2]) * det,
            ],
            g: [
                (b[0] * g[2] - g[0] * b[2]) * det,
                (r[0] * b[2] - b[0] * r[2]) * det,
                (g[0] * r[2] - r[0] * g[2]) * det,
            ],
            b: [
                (g[0] * b[1] - b[0] * g[1]) * det,
                (b[0] * r[1] - r[0] * b[1]) * det,
                (r[0] * g[1] - g[0] * r[1]) * det,
            ],
        }
    }

    fn mul(&self, o: &Mat) -> Mat {
        let a = self;
        let row = |i: usize, v: [f32; 3]| a.r[i] * v[0] + a.g[i] * v[1] + a.b[i] * v[2];
        Mat {
            r: [row(0, o.r), row(1, o.r), row(2, o.r)],
            g: [row(0, o.g), row(1, o.g), row(2, o.g)],
            b: [row(0, o.b), row(1, o.b), row(2, o.b)],
        }
    }

    fn map(&self, c: [f32; 3]) -> [f32; 3] {
        let (r, g, b) = (self.r, self.g, self.b);
        [
            c[0] * r[0] + c[1] * g[0] + c[2] * b[0],
            c[0] * r[1] + c[1] * g[1] + c[2] * b[1],
            c[0] * r[2] + c[1] * g[2] + c[2] * b[2],
        ]
    }

    fn scale(v: [f32; 3]) -> Mat {
        Mat {
            r: [v[0], 0.0, 0.0],
            g: [0.0, v[1], 0.0],
            b: [0.0, 0.0, v[2]],
        }
    }
}

/// `QColorVector::fromXYChromaticity`.
fn from_xy(x: f64, y: f64) -> [f32; 3] {
    [(x / y) as f32, 1.0, ((1.0 - x - y) / y) as f32]
}

/// The matrix of `QColorSpace(SRgb).transformationToColorSpace(SRgbLinear)` and back: the same
/// primaries on both sides, so `toXyz⁻¹ · toXyz`, which in floats is not quite the identity.
fn srgb_linear_matrix() -> Mat {
    let mut to_xyz = Mat {
        r: from_xy(0.640, 0.330),
        g: from_xy(0.300, 0.600),
        b: from_xy(0.150, 0.060),
    };
    let white = from_xy(0.31271, 0.32902);
    let scale = to_xyz.inverted().map(white);
    to_xyz = to_xyz.mul(&Mat::scale(scale));
    // Bradford adaptation of the D65 white point to D50 (QColorMatrix::chromaticAdaptation).
    let abrad = Mat {
        r: [0.8951, -0.7502, 0.0389],
        g: [0.2664, 1.7135, -0.0685],
        b: [-0.1614, 0.0367, 1.0296],
    };
    let abradinv = Mat {
        r: [0.9869929, 0.4323053, -0.0085287],
        g: [-0.1470543, 0.5183603, 0.0400428],
        b: [0.1599627, 0.0492912, 0.9684867],
    };
    let src = abrad.map(white);
    let dst = abrad.map(from_xy(0.34567, 0.35850));
    let w = Mat::scale([dst[0] / src[0], dst[1] / src[1], dst[2] / src[2]]);
    let chad = abradinv.mul(&w.mul(&abrad));
    let to_xyz = chad.mul(&to_xyz);
    to_xyz.inverted().mul(&to_xyz)
}

/// `QColorTransform::map(const QColor &)` from `input` to `output` transfer curves.
fn transform(color: QColor, input: Trc, output: Trc) -> QColor {
    let c = [color.red_f(), color.green_f(), color.blue_f()].map(|x| input.apply_extended(x));
    let c = srgb_linear_matrix()
        .map(c)
        .map(|x| output.apply_inverse_extended(x));
    QColor::from_rgb_f(c[0], c[1], c[2], color.alpha_f())
}

// ---- colorsapplicator.cpp / colorsapplicator.h ----

const DEFAULT_TINT_FACTOR: f64 = 0.15;

/// `float lerp(float, float, float)`, with its double `1.0 - f`.
fn lerp(a: f64, b: f64, f: f64) -> f64 {
    let (a, b, f) = (a as f32 as f64, b as f32 as f64, f as f32 as f64);
    ((a * (1.0 - f)) + (b * f)) as f32 as f64
}

fn cube_root(n: f64) -> f64 {
    n.powf(1.0 / 3.0)
}

fn to_oklab(c: QColor) -> [f64; 3] {
    let (r, g, b) = (c.red_f() as f64, c.green_f() as f64, c.blue_f() as f64);
    let l = cube_root(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
    let m = cube_root(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
    let s = cube_root(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
    [
        0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
        1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
        0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
    ]
}

fn from_oklab([ll, a, b]: [f64; 3]) -> QColor {
    let l_ = ll + 0.3963377774 * a + 0.2158037573 * b;
    let m_ = ll - 0.1055613458 * a - 0.0638541728 * b;
    let s_ = ll - 0.0894841775 * a - 1.2914855480 * b;
    let (l, m, s) = (l_ * l_ * l_, m_ * m_ * m_, s_ * s_ * s_);
    let r = 4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s;
    let g = -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s;
    let b = -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s;
    QColor::from_rgb_f(r as f32, g as f32, b as f32, 1.0).to_rgb()
}

/// `tintColor`: `base` with its OKLab a/b moved towards `with` by `factor`.
pub fn tint_color(base: QColor, with: QColor, factor: f64) -> QColor {
    let mut lab = to_oklab(transform(base, srgb_trc(), LINEAR));
    let w = to_oklab(transform(with, srgb_trc(), LINEAR));
    lab[1] = lerp(lab[1], w[1], factor);
    lab[2] = lerp(lab[2], w[2], factor);
    transform(from_oklab(lab), LINEAR, srgb_trc())
}

fn alpha_blend(fg: QColor, bg: QColor) -> QColor {
    let fa = fg.alpha_f();
    let inv = 1.0 - fa as f64;
    let ba = bg.alpha_f();
    if fa == 0.0 {
        return bg;
    }
    let [fr, fgr, fb, _] = fg.rgba8().map(|v| v as f32);
    let [br, bgr, bb, _] = bg.rgba8().map(|v| v as f64);
    let ch = |f: f32, b: f64, k: f64| ((fa * f) as f64 + k * b) as i32;
    if ba == 1.0 {
        QColor::from_rgb(ch(fr, br, inv), ch(fgr, bgr, inv), ch(fb, bb, inv), 0xff)
    } else {
        let inv_bg = ba as f64 * inv;
        let alpha = fa as f64 + inv_bg;
        QColor::from_rgb(
            ch(fr, br, inv_bg),
            ch(fgr, bgr, inv_bg),
            ch(fb, bb, inv_bg),
            alpha as i32,
        )
    }
}

pub fn accent_background(accent: QColor, background: QColor) -> QColor {
    let mut c = accent;
    c.set_alpha_f(0.7);
    alpha_blend(c, background)
}

pub fn accent_foreground(accent: QColor, active: bool) -> QColor {
    let white = QColor::rgb(255, 255, 255);
    let mut c = if contrast_ratio(accent, white) < 4.5 {
        QColor::rgb(0, 0, 0)
    } else {
        white
    };
    c.set_alpha_f(if active { 1.0 } else { 0.6 });
    alpha_blend(c, accent)
}

/// The accent used for `ForegroundLink` over `background`.
pub fn readable_link(accent: QColor, background: QColor) -> QColor {
    // WCAG's AA level is around 5.0.
    if contrast_ratio(background, accent) >= 5.0 {
        accent
    } else if luma(accent) > luma(background) {
        lighten(accent)
    } else {
        darken(accent)
    }
}

/// The accent of kdeglobals, if `applyScheme` would use one.
pub fn accent(kdeglobals: &Doc) -> Option<QColor> {
    let v = kdeglobals.get("General", "AccentColor")?;
    let c = QColor::read(Some(v));
    (c.is_valid() && !c.is_transparent()).then_some(c)
}

/// `KConfigGroup::readEntry(key, bool)`.
fn read_bool(v: Option<&str>, default: bool) -> bool {
    match v {
        None => default,
        Some(v) => !["false", "no", "off", "0"]
            .iter()
            .any(|n| v.eq_ignore_ascii_case(n)),
    }
}

const ACCENT_KEYS: [&str; 4] = [
    "ForegroundActive",
    "ForegroundLink",
    "DecorationFocus",
    "DecorationHover",
];

/// The scheme's accent settings.
pub struct Options {
    accent: QColor,
    titlebar: bool,
    tint: Option<f64>,
    tint_factor: f64,
}

impl Options {
    pub fn new(scheme: &Doc, accent: QColor) -> Options {
        let titlebar = read_bool(
            scheme.get("General", "TitlebarIsAccentColored"),
            read_bool(scheme.get("General", "accentActiveTitlebar"), false),
        );
        let factor = scheme.get("General", "TintFactor");
        let tint_factor = factor
            .and_then(|v| v.trim().parse::<f64>().ok())
            .unwrap_or(DEFAULT_TINT_FACTOR);
        Options {
            accent,
            titlebar,
            tint: factor.map(|_| tint_factor),
            tint_factor,
        }
    }
}

/// The `Colors:*` groups of `applyScheme` with an accent; kg's groups are already cleared.
pub(super) fn color_sets(kg: &mut Doc, scheme: &Doc, groups: &[&str], o: &Options) {
    let read = |g: &str, k: &str| QColor::read(scheme.get(g, k));
    let accent = o.accent;
    let window_bg = read("Colors:Window", "BackgroundNormal");
    for &g in groups {
        if g == "Colors:Header" && !scheme.has_group(g) {
            continue;
        }
        let inactive_name = format!("{g}][Inactive");
        let mut main: Vec<(&str, String)> = Vec::new();
        let mut inactive: Vec<(&str, String)> = Vec::new();
        let put = |out: &mut Vec<(&str, String)>, k: &'static str, v: String| match out
            .iter_mut()
            .find(|(x, _)| *x == k)
        {
            Some(e) => e.1 = v,
            None => out.push((k, v)),
        };
        for k in COLOR_KEYS {
            if k == "ForegroundLink" {
                put(
                    &mut main,
                    k,
                    readable_link(accent, read(g, "BackgroundNormal")).kconfig(),
                );
            } else if ACCENT_KEYS.contains(&k) {
                put(&mut main, k, accent.kconfig());
            } else if let Some(f) = o.tint {
                put(&mut main, k, tint_color(read(g, k), accent, f).kconfig());
            } else if let Some(v) = scheme.get(g, k) {
                put(&mut main, k, v.to_string());
            }
        }
        if g == "Colors:Selection" {
            let bg = accent_background(accent, read("Colors:View", "BackgroundNormal"));
            put(&mut main, "BackgroundNormal", bg.kconfig());
            put(&mut main, "BackgroundAlternate", bg.kconfig());
            let fg = accent_foreground(bg, true).kconfig();
            put(&mut main, "ForegroundNormal", fg.clone());
            put(&mut main, "ForegroundInactive", fg);
        }
        if g == "Colors:Button" {
            put(
                &mut main,
                "BackgroundAlternate",
                accent_background(accent, read("Colors:Button", "BackgroundNormal")).kconfig(),
            );
        }
        if scheme.has_group(&inactive_name) {
            for k in COLOR_KEYS {
                if let Some(f) = o.tint {
                    put(
                        &mut inactive,
                        k,
                        tint_color(read(&inactive_name, k), accent, f).kconfig(),
                    );
                } else {
                    // Inactive headers take the window colours (BUG:516396).
                    let from = if g == "Colors:Header" {
                        "Colors:Window"
                    } else {
                        inactive_name.as_str()
                    };
                    if let Some(v) = scheme.get(from, k) {
                        put(&mut inactive, k, v.to_string());
                    }
                }
            }
        }
        if g == "Colors:Header" && o.titlebar {
            let active = accent_background(accent, window_bg);
            let idle = tint_color(window_bg, accent, o.tint_factor);
            put(&mut main, "BackgroundNormal", active.kconfig());
            put(
                &mut main,
                "ForegroundNormal",
                accent_foreground(active, true).kconfig(),
            );
            put(&mut inactive, "BackgroundNormal", idle.kconfig());
            put(
                &mut inactive,
                "ForegroundNormal",
                accent_foreground(idle, false).kconfig(),
            );
        }
        // KConfig saves keys sorted.
        main.sort();
        inactive.sort();
        for (k, v) in main {
            kg.set(g, k, &v);
        }
        for (k, v) in inactive {
            kg.set(&inactive_name, k, &v);
        }
    }
}

/// The titlebar colours `applyScheme` puts in `[WM]` over the scheme's (written with the
/// Notify flag, so they are announced).
pub(super) fn window_manager(kg: &mut Doc, scheme: &Doc, o: &Options, notes: &mut Notes) {
    if o.tint.is_none() && !o.titlebar {
        return;
    }
    let window_bg = QColor::read(scheme.get("Colors:Window", "BackgroundNormal"));
    let mut set = |k: &str, c: QColor| {
        if kg.set("WM", k, &c.kconfig()) {
            notes.add("WM", k);
        }
    };
    if let Some(f) = o.tint {
        let tinted = tint_color(window_bg, o.accent, f);
        if !o.titlebar {
            set("activeBackground", tinted);
            set("activeForeground", accent_foreground(tinted, true));
        }
        set("inactiveBackground", tinted);
        set("inactiveForeground", accent_foreground(tinted, false));
    }
    if o.titlebar {
        let accented = accent_background(o.accent, window_bg);
        set("activeBackground", accented);
        set("activeForeground", accent_foreground(accented, true));
    }
}

const NAMES: &str = concat!(
    "aliceblue f0f8ff antiquewhite faebd7 aqua 00ffff aquamarine 7fffd4 azure f0ffff beige f5f5dc ",
    "bisque ffe4c4 black 000000 blanchedalmond ffebcd blue 0000ff blueviolet 8a2be2 brown a52a2a ",
    "burlywood deb887 cadetblue 5f9ea0 chartreuse 7fff00 chocolate d2691e coral ff7f50 ",
    "cornflowerblue 6495ed cornsilk fff8dc crimson dc143c cyan 00ffff darkblue 00008b darkcyan ",
    "008b8b darkgoldenrod b8860b darkgray a9a9a9 darkgreen 006400 darkgrey a9a9a9 darkkhaki bdb76b ",
    "darkmagenta 8b008b darkolivegreen 556b2f darkorange ff8c00 darkorchid 9932cc darkred 8b0000 ",
    "darksalmon e9967a darkseagreen 8fbc8f darkslateblue 483d8b darkslategray 2f4f4f darkslategrey ",
    "2f4f4f darkturquoise 00ced1 darkviolet 9400d3 deeppink ff1493 deepskyblue 00bfff dimgray 696969 ",
    "dimgrey 696969 dodgerblue 1e90ff firebrick b22222 floralwhite fffaf0 forestgreen 228b22 fuchsia ",
    "ff00ff gainsboro dcdcdc ghostwhite f8f8ff gold ffd700 goldenrod daa520 gray 808080 green 008000 ",
    "greenyellow adff2f grey 808080 honeydew f0fff0 hotpink ff69b4 indianred cd5c5c indigo 4b0082 ",
    "ivory fffff0 khaki f0e68c lavender e6e6fa lavenderblush fff0f5 lawngreen 7cfc00 lemonchiffon ",
    "fffacd lightblue add8e6 lightcoral f08080 lightcyan e0ffff lightgoldenrodyellow fafad2 ",
    "lightgray d3d3d3 lightgreen 90ee90 lightgrey d3d3d3 lightpink ffb6c1 lightsalmon ffa07a ",
    "lightseagreen 20b2aa lightskyblue 87cefa lightslategray 778899 lightslategrey 778899 ",
    "lightsteelblue b0c4de lightyellow ffffe0 lime 00ff00 limegreen 32cd32 linen faf0e6 magenta ",
    "ff00ff maroon 800000 mediumaquamarine 66cdaa mediumblue 0000cd mediumorchid ba55d3 mediumpurple ",
    "9370db mediumseagreen 3cb371 mediumslateblue 7b68ee mediumspringgreen 00fa9a mediumturquoise ",
    "48d1cc mediumvioletred c71585 midnightblue 191970 mintcream f5fffa mistyrose ffe4e1 moccasin ",
    "ffe4b5 navajowhite ffdead navy 000080 oldlace fdf5e6 olive 808000 olivedrab 6b8e23 orange ",
    "ffa500 orangered ff4500 orchid da70d6 palegoldenrod eee8aa palegreen 98fb98 paleturquoise ",
    "afeeee palevioletred db7093 papayawhip ffefd5 peachpuff ffdab9 peru cd853f pink ffc0cb plum ",
    "dda0dd powderblue b0e0e6 purple 800080 red ff0000 rosybrown bc8f8f royalblue 4169e1 saddlebrown ",
    "8b4513 salmon fa8072 sandybrown f4a460 seagreen 2e8b57 seashell fff5ee sienna a0522d silver ",
    "c0c0c0 skyblue 87ceeb slateblue 6a5acd slategray 708090 slategrey 708090 snow fffafa ",
    "springgreen 00ff7f steelblue 4682b4 tan d2b48c teal 008080 thistle d8bfd8 tomato ff6347 ",
    "turquoise 40e0d0 violet ee82ee wheat f5deb3 white ffffff whitesmoke f5f5f5 yellow ffff00 ",
    "yellowgreen 9acd32 ",
);

#[cfg(test)]
mod tests {
    //! Reference vectors from the real tools, in `tests/kde/accent_vectors.txt` (regenerate with
    //! `tools/reference/kmyc-golden/accent_vectors.sh`; versions in its header): Qt 6.12 `QColor` and
    //! `QColorTransform` (section `qcolor`, `transform`), KConfig 6.31 `KConfigGroup` colour
    //! entries (`kconfig`), KGuiAddons 6.31 `KColorUtils` (`kcolorutils`), Plasma 6.8
    //! `colorsapplicator.{h,cpp}` helpers compiled against them (`colorsapplicator`), and the
    //! `kdeglobals` the real `plasma-apply-colorscheme` 6.8 writes (`## <case>` sections) for the
    //! Material You schemes kde-material-you-colors made for the original's seed #5E7A91
    //! (`tests/kde/expected/seed-5e7a91-dark`, the matugen/end-4 flow that feeds `applyScheme`).
    use super::*;
    use crate::kde::apply::apply_scheme_text;
    use std::collections::BTreeMap;

    const VECTORS: &str = include_str!("../../tests/kde/accent_vectors.txt");
    const DARK: &str = include_str!(
        "../../tests/kde/expected/seed-5e7a91-dark/.local/share/color-schemes/MaterialYouDark.colors"
    );
    const LIGHT: &str = include_str!(
        "../../tests/kde/expected/seed-5e7a91-dark/.local/share/color-schemes/MaterialYouLight.colors"
    );

    /// The `name = value` lines of one `### section`.
    fn section(name: &str) -> Vec<(&'static str, &'static str)> {
        let mut out = Vec::new();
        let mut on = false;
        for line in VECTORS.lines() {
            if let Some(s) = line.strip_prefix("### ") {
                on = s == name;
            } else if line.starts_with("## ") {
                on = false;
            } else if on && !line.starts_with('#') {
                out.push(line.split_once(" = ").unwrap());
            }
        }
        assert!(!out.is_empty(), "no vectors in section {name}");
        out
    }

    enum V {
        C(QColor),
        N(f64),
    }

    impl V {
        fn c(self) -> QColor {
            match self {
                V::C(c) => c,
                V::N(_) => panic!("not a colour"),
            }
        }
    }

    /// Splits at `sep` outside parentheses.
    fn split_top(s: &str, sep: char) -> Vec<&str> {
        let (mut out, mut depth, mut start) = (Vec::new(), 0, 0);
        for (i, ch) in s.char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => depth -= 1,
                c if c == sep && depth == 0 => {
                    out.push(&s[start..i]);
                    start = i + 1;
                }
                _ => {}
            }
        }
        out.push(&s[start..]);
        out
    }

    fn nums<T: std::str::FromStr>(s: &str) -> Vec<T>
    where
        T::Err: std::fmt::Debug,
    {
        s.split(',').map(|n| n.trim().parse().unwrap()).collect()
    }

    /// `r,g,b` (`QColor(r, g, b)`), `r,g,b,aF` (then `setAlphaF(F)`) or an expression.
    fn color(e: &str) -> QColor {
        if e.contains('(') {
            return eval(e).c();
        }
        let parts: Vec<&str> = e.split(',').collect();
        let [r, g, b] = [0, 1, 2].map(|i| parts[i].parse::<u8>().unwrap());
        let mut c = QColor::rgb(r, g, b);
        if let Some(a) = parts.get(3) {
            c.set_alpha_f(a.strip_prefix('a').unwrap().parse().unwrap());
        }
        c
    }

    /// Evaluates a vector name, e.g. `tintColor(28,32,36;61,174,233;0.15)`, with the port.
    fn eval(e: &str) -> V {
        let chain = split_top(e, '.');
        // Rejoin decimal points: only `name(...)` heads and method names follow a top-level dot.
        let mut parts: Vec<String> = Vec::new();
        for p in chain {
            match parts.last_mut() {
                Some(last) if !p.starts_with(|c: char| c.is_ascii_alphabetic()) => {
                    last.push('.');
                    last.push_str(p);
                }
                _ => parts.push(p.to_string()),
            }
        }
        let head = &parts[0];
        let (f, inner) = match head.split_once('(') {
            Some((f, rest)) => (f, rest.strip_suffix(')').unwrap()),
            None => ("", head.as_str()),
        };
        let args = split_top(inner, ';');
        let mut v = match f {
            "" => V::C(color(inner)),
            "fromRgbF" => {
                let n: Vec<f32> = nums(inner);
                V::C(QColor::from_rgb_f(n[0], n[1], n[2], n[3]))
            }
            "QColor" => V::C(color(inner)),
            "QRgba64" => V::C(QColor::Rgb([inner.parse().unwrap(), 0, 0, 0xffff])),
            "read" => V::C(QColor::read((inner != "<absent>").then_some(inner))),
            "write" => V::C(match inner {
                "opaque" => QColor::rgb(61, 174, 233),
                "alpha" => color("61,174,233,a0.7"),
                "ext" => QColor::from_rgb_f(1.2, -0.1, 0.5, 0.6),
                "invalid" => QColor::Invalid,
                k => panic!("write({k})"),
            }),
            "toLinear" => V::C(transform(color(inner), srgb_trc(), LINEAR)),
            "fromLinear" => V::C(transform(color(inner), LINEAR, srgb_trc())),
            // `KColorUtils::hue` is `KHCY::hue`: the stored hue wrapped into [0, 1).
            "hue" => V::N(match Hcy::new(color(inner)).h % 1.0 {
                h if h < 0.0 => 1.0 + h,
                h if h > 0.0 => h,
                _ => 0.0,
            }),
            "chroma" => V::N(Hcy::new(color(inner)).c),
            "luma" => {
                let c = color(inner);
                assert_eq!(Hcy::new(c).y, luma(c), "KHCY y is KColorUtils::luma");
                V::N(luma(c))
            }
            "hcyColor" => V::C(match inner.strip_prefix("hcy(") {
                Some(c) => Hcy::new(color(c.strip_suffix(')').unwrap())).color(),
                None => {
                    let n: Vec<f64> = nums(inner);
                    Hcy {
                        h: n[0],
                        c: n[1],
                        y: n[2],
                        a: n[3],
                    }
                    .color()
                }
            }),
            "lighten" => V::C(lighten(color(inner))),
            "darken" => V::C(darken(color(inner))),
            "contrastRatio" => V::N(contrast_ratio(color(args[0]), color(args[1]))),
            "tintColor" => V::C(tint_color(
                color(args[0]),
                color(args[1]),
                args[2].parse().unwrap(),
            )),
            "readableLink" => V::C(readable_link(color(args[0]), color(args[1]))),
            "accentBackground" => V::C(accent_background(color(args[0]), color(args[1]))),
            "accentForeground" => {
                let (c, active) = inner.rsplit_once(',').unwrap();
                V::C(accent_foreground(color(c), active == "true"))
            }
            "alphaBlend" => V::C(alpha_blend(color(args[0]), color(args[1]))),
            f => panic!("no evaluator for {f}"),
        };
        for m in &parts[1..] {
            v = match m.as_str() {
                "toRgb()" => V::C(v.c().to_rgb()),
                "rgba8" => v,
                "red" => V::N(v.c().rgba8()[0] as f64),
                m => {
                    let a = m
                        .strip_prefix("setAlphaF(")
                        .and_then(|a| a.strip_suffix(')'))
                        .unwrap_or_else(|| panic!("no method {m}"));
                    let mut c = v.c();
                    c.set_alpha_f(a.parse().unwrap());
                    V::C(c)
                }
            };
        }
        v
    }

    /// `rgb r g b a` (16-bit channels), `ext r g b a` (qfloat16 values), `invalid`, a
    /// KConfig-written `r,g,b[,a]` or a number.
    fn matches(got: V, want: &str) -> bool {
        match got {
            V::N(n) => n == want.parse::<f64>().unwrap(),
            V::C(c) => match want.split_once(' ') {
                Some(("rgb", v)) => {
                    let v: Vec<u16> = v.split(' ').map(|x| x.parse().unwrap()).collect();
                    c == QColor::Rgb([v[0], v[1], v[2], v[3]])
                }
                Some(("ext", v)) => {
                    let v: Vec<f32> = v.split(' ').map(|x| x.parse().unwrap()).collect();
                    c == QColor::Ext([v[0], v[1], v[2], v[3]])
                }
                _ => c.kconfig() == want && (want != "invalid" || !c.is_valid()),
            },
        }
    }

    fn check(name: &str) {
        let bad: Vec<String> = section(name)
            .into_iter()
            .filter(|(e, want)| !matches(eval(e), want))
            .map(|(e, want)| match eval(e) {
                V::C(c) => format!("{e}: got {c:?} ({}), want {want}", c.kconfig()),
                V::N(n) => format!("{e}: got {n:?}, want {want}"),
            })
            .collect();
        assert!(bad.is_empty(), "{name}:\n{}", bad.join("\n"));
    }

    /// Qt 6.12: `fromRgbF` keeps in-range floats as `qRound(x * 65535)` 16-bit channels and
    /// out-of-range ones as `ExtendedRgb` qfloat16s (invalid alpha: invalid colour), `toRgb`
    /// clamps, 8-bit reads go through `qt_div_257`, `setAlphaF` rounds per storage.
    #[test]
    fn qcolor_storage_matches_qt() {
        check("qcolor");
    }

    /// KConfig 6.31 `readEntry(key, QColor())` / `writeEntry(key, QColor)`.
    #[test]
    fn kconfig_colour_entries_match_kconfig() {
        check("kconfig");
    }

    /// Qt 6.12 `QColorSpace(SRgb).transformationToColorSpace(SRgbLinear)` and back, on 16-bit,
    /// translucent and `ExtendedRgb` colours.
    #[test]
    fn srgb_linear_transforms_match_qcolortransform() {
        check("transform");
    }

    /// KGuiAddons 6.31 `KColorUtils`: `hue`/`chroma`/`luma` (KHCY), `hcyColor` (hue wrap, chroma
    /// and luma clamps), `lighten`, `darken`, `contrastRatio`.
    #[test]
    fn hcy_matches_kcolorutils() {
        check("kcolorutils");
    }

    /// Plasma 6.8 `tintColor` (OKLab, `TintFactor`), readable links (lighten/darken below
    /// contrast 5), `accentBackground`/`accentForeground`/`alphaBlend`.
    #[test]
    fn colorsapplicator_helpers_match_plasma() {
        check("colorsapplicator");
    }

    type Groups = BTreeMap<String, BTreeMap<String, String>>;

    /// `## <case> <scheme> <accent> [General lines;...]` sections and the `[Colors:*]`/`[WM]`
    /// entries `plasma-apply-colorscheme` wrote.
    fn cases() -> Vec<(Vec<&'static str>, Groups)> {
        let mut out: Vec<(Vec<&'static str>, Groups)> = Vec::new();
        for line in VECTORS.lines() {
            if let Some(h) = line.strip_prefix("## ") {
                out.push((h.split_whitespace().collect(), Groups::new()));
            } else if let (Some((_, groups)), Some(l)) = (out.last_mut(), line.strip_prefix('[')) {
                let (g, kv) = l.split_once("] ").unwrap();
                let (k, v) = kv.split_once('=').unwrap();
                groups
                    .entry(g.into())
                    .or_default()
                    .insert(k.into(), v.into());
            }
        }
        out
    }

    /// Ours on the case: kdeglobals with only `AccentColor`, the scheme with the case's
    /// `[General]` lines, as `accent_vectors.sh` built them.
    fn ours(case: &[&str]) -> Groups {
        let scheme = match case[1] {
            "MaterialYouDark" => DARK,
            "MaterialYouLight" => LIGHT,
            s => panic!("scheme {s}"),
        };
        let extra = case
            .get(3)
            .map_or(String::new(), |l| l.replace(';', "\n") + "\n");
        let text: String = scheme
            .lines()
            .map(|l| match l {
                "[General]" => format!("{l}\n{extra}"),
                l if l.starts_with("ColorScheme=") => "ColorScheme=Test\n".into(),
                l => format!("{l}\n"),
            })
            .collect();
        let mut kg = Doc::parse(&format!("[General]\nAccentColor={}\n", case[2]));
        apply_scheme_text(&mut kg, &text, "Test");
        let mut out = Groups::new();
        for g in kg.groups() {
            if g.starts_with("Colors:") || g == "WM" {
                for (k, v) in kg.entries(g) {
                    out.entry(g.into()).or_default().insert(k.into(), v.into());
                }
            }
        }
        out
    }

    fn check_schemes(prefix: &str) {
        let mut bad = Vec::new();
        let mut n = 0;
        for (case, want) in cases()
            .into_iter()
            .filter(|(c, _)| c[0].starts_with(prefix))
        {
            n += 1;
            let got = ours(&case);
            for g in want
                .keys()
                .chain(got.keys())
                .collect::<std::collections::BTreeSet<_>>()
            {
                let (w, o) = (want.get(g), got.get(g));
                for k in w
                    .into_iter()
                    .chain(o)
                    .flat_map(|m| m.keys())
                    .collect::<std::collections::BTreeSet<_>>()
                {
                    let (w, o) = (w.and_then(|m| m.get(k)), o.and_then(|m| m.get(k)));
                    if w != o {
                        bad.push(format!("{} [{g}] {k}: ours {o:?} plasma {w:?}", case[0]));
                    }
                }
            }
        }
        assert!(n >= 3, "too few {prefix} cases");
        assert!(bad.is_empty(), "{}", bad.join("\n"));
    }

    /// Accent on ForegroundActive/DecorationFocus/DecorationHover, readable ForegroundLink per
    /// group (kept, and lightened for a dark accent), accented selection and button
    /// backgrounds, `TintFactor` 0.3 tinting and accented titlebar (Header, Header][Inactive,
    /// WM) on the dark Material You scheme: `plasma-apply-colorscheme` 6.8.2.
    #[test]
    fn dark_scheme_matches_plasma_apply_colorscheme() {
        check_schemes("dark-");
    }

    /// The same on the light Material You scheme: link darkened for a light accent, accented
    /// titlebar with the default tint factor for the inactive header, `TintFactor` 0.15
    /// without accented titlebar (WM active colours from the tint): `plasma-apply-colorscheme`
    /// 6.8.2.
    #[test]
    fn light_scheme_matches_plasma_apply_colorscheme() {
        check_schemes("light-");
    }
}
