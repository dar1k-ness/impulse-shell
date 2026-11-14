//! Port of QColor storage/conversion semantics used by QML (16-bit channels, Rgb/Hsv/Hsl specs).
#![allow(clippy::needless_range_loop)]
const UMAX: f32 = 65535.0;

fn qround(f: f32) -> u16 {
    (if f >= 0.0 {
        (f + 0.5).floor()
    } else {
        (f - 0.5).ceil()
    }) as i64 as u16
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spec {
    Invalid,
    Rgb,
    Hsv,
    Hsl,
    /// QColor::ExtendedRgb: what a red, green or blue outside 0..1 makes of a color.
    ExtendedRgb,
}

/// qfloat16 of `f`, rounded to the nearest, a tie to even.
fn f16_bits(f: f32) -> u16 {
    let x = f.to_bits();
    let sign = ((x >> 16) & 0x8000) as u16;
    let (exp, man) = (((x >> 23) & 0xff) as i32, x & 0x7f_ffff);
    if exp == 0xff {
        return sign | 0x7c00 | if man != 0 { 0x200 } else { 0 };
    }
    let e = exp - 127 + 15;
    if e >= 0x1f {
        return sign | 0x7c00;
    }
    let (half, rem, mid) = if e <= 0 {
        if e < -10 {
            return sign;
        }
        let (m, shift) = (man | 0x80_0000, (14 - e) as u32);
        (m >> shift, m & ((1 << shift) - 1), 1 << (shift - 1))
    } else {
        (((e as u32) << 10) | (man >> 13), man & 0x1fff, 0x1000)
    };
    let up = rem > mid || (rem == mid && half & 1 == 1);
    // A carry out of the mantissa is the next exponent, and past the last one infinity.
    sign | (half + u32::from(up)) as u16
}

/// The value of qfloat16 `b`.
fn f16_val(b: u16) -> f32 {
    let m = f32::from(b & 0x3ff);
    let v = match (b >> 10) & 0x1f {
        0 => m * 2f32.powi(-24),
        31 if m == 0.0 => f32::INFINITY,
        31 => f32::NAN,
        e => (1.0 + m / 1024.0) * 2f32.powi(i32::from(e) - 15),
    };
    if b & 0x8000 != 0 { -v } else { v }
}

/// qFuzzyCompare(float, float).
fn fuzzy(a: f32, b: f32) -> bool {
    (a - b).abs() * 100_000.0 <= a.abs().min(b.abs())
}

/// Components: Rgb → [r,g,b]; Hsv → [h,s,v]; Hsl → [h,s,l]. `a` alpha. Hue 0..35999 or 65535 (achromatic).
/// ExtendedRgb → [r,g,b] and `a` as qfloat16 bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Color {
    pub spec: Spec,
    pub a: u16,
    pub c: [u16; 3],
}

impl Color {
    pub const INVALID: Color = Color {
        spec: Spec::Invalid,
        a: 65535,
        c: [0; 3],
    };

    fn valid_f(v: &[f32]) -> bool {
        v.iter().all(|x| (0.0..=1.0).contains(x))
    }

    /// QColor::operator==: the same spec and components (a hue modulo 360°; HSL saturation
    /// and lightness within 50 of 65535); an extended RGB color and an RGB or extended RGB
    /// one by their fuzzily compared components. What a `color` property's write compares.
    pub fn same_qcolor(self, o: Color) -> bool {
        let rgb = |c: Color| matches!(c.spec, Spec::Rgb | Spec::ExtendedRgb);
        if (self.spec == Spec::ExtendedRgb || o.spec == Spec::ExtendedRgb) && rgb(self) && rgb(o) {
            return fuzzy(self.alpha_f(), o.alpha_f())
                && fuzzy(self.red_f(), o.red_f())
                && fuzzy(self.green_f(), o.green_f())
                && fuzzy(self.blue_f(), o.blue_f());
        }
        if self.spec != o.spec || self.a != o.a {
            return false;
        }
        let near = |a: u16, b: u16| a.abs_diff(b) < 50;
        match self.spec {
            Spec::Hsl => {
                let edge = |l: u16| l == 0 || l == 65535;
                self.c[0] % 36000 == o.c[0] % 36000
                    && (near(self.c[1], o.c[1]) || edge(self.c[2]) || edge(o.c[2]))
                    && near(self.c[2], o.c[2])
            }
            Spec::Hsv => {
                (self.c[0] % 36000 == o.c[0] % 36000 || self.c[0] == o.c[0])
                    && self.c[1..] == o.c[1..]
            }
            _ => self.c == o.c,
        }
    }

    pub fn from_rgb_f(r: f32, g: f32, b: f32, a: f32) -> Color {
        if !Self::valid_f(&[r, g, b, a]) {
            return Color::INVALID;
        }
        Color {
            spec: Spec::Rgb,
            a: qround(a * UMAX),
            c: [qround(r * UMAX), qround(g * UMAX), qround(b * UMAX)],
        }
    }

    pub fn from_argb32(v: u32) -> Color {
        let ch = |s: u32| ((v >> s) & 0xff) as u16 * 0x101;
        Color {
            spec: Spec::Rgb,
            a: ch(24),
            c: [ch(16), ch(8), ch(0)],
        }
    }

    pub fn from_hsv_f(h: f32, s: f32, v: f32, a: f32) -> Color {
        if !(h == -1.0 || (0.0..=1.0).contains(&h)) || !Self::valid_f(&[s, v, a]) {
            return Color::INVALID;
        }
        let hue = if h == -1.0 {
            65535
        } else {
            qround(h * 36000.0)
        };
        Color {
            spec: Spec::Hsv,
            a: qround(a * UMAX),
            c: [hue, qround(s * UMAX), qround(v * UMAX)],
        }
    }

    pub fn from_hsl_f(h: f32, s: f32, l: f32, a: f32) -> Color {
        if !(h == -1.0 || (0.0..=1.0).contains(&h)) || !Self::valid_f(&[s, l, a]) {
            return Color::INVALID;
        }
        let mut hue = if h == -1.0 {
            65535
        } else {
            qround(h * 36000.0)
        };
        if hue == 36000 {
            hue = 0;
        }
        Color {
            spec: Spec::Hsl,
            a: qround(a * UMAX),
            c: [hue, qround(s * UMAX), qround(l * UMAX)],
        }
    }

    fn hue_of(r: u16, g: u16, b: u16, max: u16, chroma: f32) -> u16 {
        let (r, g, b) = (r as f32, g as f32, b as f32);
        let mut hue = if max as f32 == r {
            let h = (g - b) / chroma;
            if h < 0.0 { h + 6.0 } else { h }
        } else if max as f32 == g {
            2.0 + (b - r) / chroma
        } else {
            4.0 + (r - g) / chroma
        };
        hue *= 6000.0;
        qround(hue)
    }

    pub fn to_hsv(self) -> Color {
        match self.spec {
            Spec::Invalid | Spec::Hsv => return self,
            Spec::Rgb => {}
            _ => return self.to_rgb().to_hsv(),
        }
        let [r, g, b] = self.c;
        let (min, max) = (r.min(g).min(b), r.max(g).max(b));
        if max == min {
            return Color {
                spec: Spec::Hsv,
                a: self.a,
                c: [65535, 0, max],
            };
        }
        let chroma = (max - min) as f32;
        Color {
            spec: Spec::Hsv,
            a: self.a,
            c: [
                Self::hue_of(r, g, b, max, chroma),
                qround(chroma / max as f32 * UMAX),
                max,
            ],
        }
    }

    pub fn to_hsl(self) -> Color {
        match self.spec {
            Spec::Invalid | Spec::Hsl => return self,
            Spec::Rgb => {}
            _ => return self.to_rgb().to_hsl(),
        }
        let [r, g, b] = self.c;
        let (min, max) = (r.min(g).min(b), r.max(g).max(b));
        if max == min {
            return Color {
                spec: Spec::Hsl,
                a: self.a,
                c: [65535, 0, max],
            };
        }
        let chroma = (max - min) as f32;
        let l = 0.5f32 * (max as u32 + min as u32) as f32;
        let s = 0.5f32 * chroma / l.min(UMAX - l);
        Color {
            spec: Spec::Hsl,
            a: self.a,
            c: [
                Self::hue_of(r, g, b, max, chroma),
                qround(s * UMAX),
                qround(l),
            ],
        }
    }

    pub fn to_rgb(self) -> Color {
        let mut out = Color {
            spec: Spec::Rgb,
            a: self.a,
            c: [0; 3],
        };
        match self.spec {
            Spec::Invalid | Spec::Rgb => return self,
            Spec::ExtendedRgb => {
                out.a = qround(UMAX * f16_val(self.a));
                out.c = self.c.map(|x| qround(UMAX * f16_val(x).clamp(0.0, 1.0)));
            }
            Spec::Hsv => {
                let [hue, sat, val] = self.c;
                if sat == 0 || hue == 65535 {
                    out.c = [val; 3];
                    return out;
                }
                let h = if hue == 36000 {
                    0.0
                } else {
                    hue as f32 / 6000.0
                };
                let s = sat as f32 / UMAX;
                let v = val as f32 / UMAX;
                let i = h as i32;
                let f = h - i as f32;
                let p = v * (1.0 - s);
                let q = v * (1.0 - s * f);
                let t = v * (1.0 - s * (1.0 - f));
                let (r, g, b) = match i {
                    0 => (v, t, p),
                    1 => (q, v, p),
                    2 => (p, v, t),
                    3 => (p, q, v),
                    4 => (t, p, v),
                    _ => (v, p, q),
                };
                out.c = [qround(r * UMAX), qround(g * UMAX), qround(b * UMAX)];
            }
            Spec::Hsl => {
                let [hue, sat, lig] = self.c;
                if sat == 0 || hue == 65535 {
                    out.c = [lig; 3];
                } else if lig == 0 {
                    out.c = [0; 3];
                } else {
                    let h = if hue == 36000 {
                        0.0
                    } else {
                        hue as f32 / 36000.0
                    };
                    let s = sat as f32 / UMAX;
                    let l = lig as f32 / UMAX;
                    let t2 = if l < 0.5 {
                        l * (1.0 + s)
                    } else {
                        l + s - l * s
                    };
                    let t1 = 2.0 * l - t2;
                    let mut t3 = [h + 1.0 / 3.0, h, h - 1.0 / 3.0];
                    for i in 0..3 {
                        if t3[i] < 0.0 {
                            t3[i] += 1.0;
                        } else if t3[i] > 1.0 {
                            t3[i] -= 1.0;
                        }
                        let six = t3[i] * 6.0;
                        let v = if six < 1.0 {
                            t1 + (t2 - t1) * six
                        } else if t3[i] * 2.0 < 1.0 {
                            t2
                        } else if t3[i] * 3.0 < 2.0 {
                            t1 + (t2 - t1) * (2.0 / 3.0 - t3[i]) * 6.0
                        } else {
                            t1
                        };
                        out.c[i] = qround(v * UMAX);
                    }
                    for ch in &mut out.c {
                        if *ch == 1 {
                            *ch = 0;
                        }
                    }
                }
            }
        }
        out
    }

    fn rgb_f(self, i: usize) -> f32 {
        match self.spec {
            Spec::ExtendedRgb => f16_val(self.c[i]),
            _ => self.to_rgb().c[i] as f32 / UMAX,
        }
    }
    pub fn red_f(self) -> f32 {
        self.rgb_f(0)
    }
    pub fn green_f(self) -> f32 {
        self.rgb_f(1)
    }
    pub fn blue_f(self) -> f32 {
        self.rgb_f(2)
    }
    pub fn alpha_f(self) -> f32 {
        match self.spec {
            Spec::ExtendedRgb => f16_val(self.a),
            _ => self.a as f32 / UMAX,
        }
    }

    /// QColor::setAlphaF: `a` bounded to 0..1, a NaN being 0.
    pub fn with_alpha_f(self, a: f32) -> Color {
        let a = if a.is_nan() { 0.0 } else { a.clamp(0.0, 1.0) };
        let a = match self.spec {
            Spec::ExtendedRgb => f16_bits(a),
            _ => qround(a * UMAX),
        };
        Color { a, ..self }
    }

    /// QColor::setRedF, setGreenF and setBlueF for component `i`: an RGB color takes a value
    /// in 0..1; any other value, or any value in an extended RGB color, is kept as it is in an
    /// extended RGB color; a color of another spec becomes one of these two. A NaN is 0.
    pub fn with_rgb_f(self, i: usize, x: f32) -> Color {
        let x = if x.is_nan() { 0.0 } else { x };
        let mut f = [self.red_f(), self.green_f(), self.blue_f()];
        f[i] = x;
        if self.spec != Spec::ExtendedRgb && Self::valid_f(&f) {
            let mut rgb = self.to_rgb();
            rgb.spec = Spec::Rgb;
            rgb.c[i] = qround(x * UMAX);
            return rgb;
        }
        Color {
            spec: Spec::ExtendedRgb,
            a: f16_bits(self.alpha_f()),
            c: f.map(f16_bits),
        }
    }

    /// QColor::toExtendedRgb.
    fn to_extended(self) -> Color {
        match self.spec {
            Spec::Invalid | Spec::ExtendedRgb => self,
            _ => Color {
                spec: Spec::ExtendedRgb,
                a: f16_bits(self.alpha_f()),
                c: [self.red_f(), self.green_f(), self.blue_f()].map(f16_bits),
            },
        }
    }
    fn hue_f(h: u16) -> f32 {
        if h == 65535 { -1.0 } else { h as f32 / 36000.0 }
    }
    pub fn hsv_hue_f(self) -> f32 {
        Self::hue_f(self.to_hsv().c[0])
    }
    pub fn hsv_saturation_f(self) -> f32 {
        self.to_hsv().c[1] as f32 / UMAX
    }
    pub fn value_f(self) -> f32 {
        self.to_hsv().c[2] as f32 / UMAX
    }
    pub fn hsl_hue_f(self) -> f32 {
        Self::hue_f(self.to_hsl().c[0])
    }
    pub fn hsl_saturation_f(self) -> f32 {
        self.to_hsl().c[1] as f32 / UMAX
    }
    pub fn lightness_f(self) -> f32 {
        self.to_hsl().c[2] as f32 / UMAX
    }

    pub fn convert_to(self, s: Spec) -> Color {
        match s {
            Spec::Rgb => self.to_rgb(),
            Spec::Hsv => self.to_hsv(),
            Spec::Hsl => self.to_hsl(),
            Spec::ExtendedRgb => self.to_extended(),
            Spec::Invalid => self,
        }
    }

    pub fn lighter(self, factor: i32) -> Color {
        if factor <= 0 {
            return self;
        }
        if factor < 100 {
            return self.darker(10000 / factor);
        }
        let mut hsv = self.to_hsv();
        let mut s = hsv.c[1] as i64;
        let mut v = (factor as u64 * hsv.c[2] as u64 / 100) as i64;
        if v > 65535 {
            s = (s - (v - 65535)).max(0);
            v = 65535;
        }
        hsv.c[1] = s as u16;
        hsv.c[2] = v as u16;
        hsv.convert_to(self.spec)
    }

    pub fn darker(self, factor: i32) -> Color {
        if factor <= 0 {
            return self;
        }
        if factor < 100 {
            return self.lighter(10000 / factor);
        }
        let mut hsv = self.to_hsv();
        hsv.c[2] = (hsv.c[2] as u32 * 100 / factor as u32) as u16;
        hsv.convert_to(self.spec)
    }

    /// 8-bit premultiplication input as used by the renderer: [r, g, b, a] (QColor::rgba() rounding).
    pub fn rgba8(self) -> [u8; 4] {
        let c = self.to_rgb();
        let q = |v: u16| ((v as u32 + 0x80 - ((v as u32 + 0x80) >> 8)) >> 8) as u8;
        [q(c.c[0]), q(c.c[1]), q(c.c[2]), q(c.a)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip() {
        let c = Color::from_argb32(0xff3a7bd5);
        let h = c.to_hsl();
        assert_eq!(h.to_rgb().rgba8(), [0x3a, 0x7b, 0xd5, 0xff]);
        assert_eq!(c.to_hsv().to_rgb().rgba8(), [0x3a, 0x7b, 0xd5, 0xff]);
        assert_eq!(Color::from_rgb_f(0.5, 0.5, 0.5, 1.0).c, [32768; 3]);
    }
}
