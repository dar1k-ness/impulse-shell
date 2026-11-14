//! Port of `QEasingCurve` (qtbase `qeasingcurve.cpp` + `3rdparty/easing/easing.cpp`), including
//! Qt's approximate Cardano solver for `BezierSpline`, so animation progress matches Qt exactly.

#![allow(clippy::neg_cmp_op_on_partial_ord)] // NaN handling mirrors Qt

use std::f64::consts::{FRAC_PI_2, PI};

#[derive(Clone, Debug, PartialEq)]
pub struct Easing {
    pub ty: i32,
    pub amplitude: f64,
    pub period: f64,
    pub overshoot: f64,
    /// Control points (c1, c2, end) triples, flattened as x, y pairs.
    pub bezier: Vec<(f64, f64)>,
}

impl Default for Easing {
    fn default() -> Self {
        Easing {
            ty: 0,
            amplitude: 1.0,
            period: 0.3,
            overshoot: 1.70158,
            bezier: Vec::new(),
        }
    }
}

pub const BEZIER_SPLINE: i32 = 45;

impl Easing {
    /// `QEasingCurve::valueForProgress`.
    pub fn value(&self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        let (a, p, o) = (self.amplitude, self.period, self.overshoot);
        match self.ty {
            0 => t,
            1 => t * t,
            2 => -t * (t - 2.0),
            3 => in_out_quad(t),
            4 => out_in(t, |t| -t * (t - 2.0), |t| t * t),
            5 => t * t * t,
            6 => out_cubic(t),
            7 => {
                let t = t * 2.0;
                if t < 1.0 {
                    0.5 * t * t * t
                } else {
                    let t = t - 2.0;
                    0.5 * (t * t * t + 2.0)
                }
            }
            8 => out_in(t, out_cubic, |t| t * t * t),
            9 => t * t * t * t,
            10 => out_quart(t),
            11 => {
                let t = t * 2.0;
                if t < 1.0 {
                    0.5 * t * t * t * t
                } else {
                    let t = t - 2.0;
                    -0.5 * (t * t * t * t - 2.0)
                }
            }
            12 => out_in(t, out_quart, |t| t * t * t * t),
            13 => t * t * t * t * t,
            14 => out_quint(t),
            15 => {
                let t = t * 2.0;
                if t < 1.0 {
                    0.5 * t * t * t * t * t
                } else {
                    let t = t - 2.0;
                    0.5 * (t * t * t * t * t + 2.0)
                }
            }
            16 => out_in(t, out_quint, |t| t * t * t * t * t),
            17 => in_sine(t),
            18 => (t * FRAC_PI_2).sin(),
            19 => -0.5 * ((PI * t).cos() - 1.0),
            20 => out_in(t, |t| (t * FRAC_PI_2).sin(), in_sine),
            21 => in_expo(t),
            22 => out_expo(t),
            23 => {
                if t == 0.0 {
                    return 0.0;
                }
                if t == 1.0 {
                    return 1.0;
                }
                let t = t * 2.0;
                if t < 1.0 {
                    0.5 * 2f64.powf(10.0 * (t - 1.0)) - 0.0005
                } else {
                    0.5 * 1.0005 * (-(2f64.powf(-10.0 * (t - 1.0))) + 2.0)
                }
            }
            24 => out_in(t, out_expo, in_expo),
            25 => in_circ(t),
            26 => out_circ(t),
            27 => {
                let t = t * 2.0;
                if t < 1.0 {
                    -0.5 * ((1.0 - t * t).sqrt() - 1.0)
                } else {
                    let t = t - 2.0;
                    0.5 * ((1.0 - t * t).sqrt() + 1.0)
                }
            }
            28 => out_in(t, out_circ, in_circ),
            29..=32 => {
                let p = if p < 0.0 { 0.3 } else { p };
                let a = if a < 0.0 { 1.0 } else { a };
                match self.ty {
                    29 => in_elastic_h(t, 0.0, 1.0, 1.0, a, p),
                    30 => out_elastic_h(t, 0.0, 1.0, 1.0, a, p),
                    31 => in_out_elastic(t, a, p),
                    _ => {
                        if t < 0.5 {
                            out_elastic_h(t * 2.0, 0.0, 0.5, 1.0, a, p)
                        } else {
                            in_elastic_h(2.0 * t - 1.0, 0.5, 0.5, 1.0, a, p)
                        }
                    }
                }
            }
            33..=36 => {
                if !(t > 0.0) {
                    return 0.0;
                }
                if !(t < 1.0) {
                    return 1.0;
                }
                let s = if o < 0.0 { 1.70158 } else { o };
                match self.ty {
                    33 => in_back(t, s),
                    34 => out_back(t, s),
                    35 => {
                        let t = t * 2.0;
                        let s = s * 1.525f32 as f64;
                        if t < 1.0 {
                            0.5 * (t * t * ((s + 1.0) * t - s))
                        } else {
                            let t = t - 2.0;
                            0.5 * (t * t * ((s + 1.0) * t + s) + 2.0)
                        }
                    }
                    _ => {
                        if t < 0.5 {
                            out_back(2.0 * t, s) / 2.0
                        } else {
                            in_back(2.0 * t - 1.0, s) / 2.0 + 0.5
                        }
                    }
                }
            }
            37..=40 => {
                let a = if a < 0.0 { 1.0 } else { a };
                match self.ty {
                    37 => 1.0 - out_bounce_h(1.0 - t, 1.0, a),
                    38 => out_bounce_h(t, 1.0, a),
                    39 => {
                        if t < 0.5 {
                            (1.0 - out_bounce_h(1.0 - 2.0 * t, 1.0, a)) / 2.0
                        } else if t == 1.0 {
                            1.0
                        } else {
                            out_bounce_h(2.0 * t - 1.0, 1.0, a) / 2.0 + 0.5
                        }
                    }
                    _ => {
                        if t < 0.5 {
                            out_bounce_h(t * 2.0, 0.5, a)
                        } else {
                            1.0 - out_bounce_h(2.0 - 2.0 * t, 0.5, a)
                        }
                    }
                }
            }
            41 => {
                let mix = smooth_mix(t);
                sin_progress(t) * mix + t * (1.0 - mix)
            }
            42 => {
                let mix = smooth_mix(1.0 - t);
                sin_progress(t) * mix + t * (1.0 - mix)
            }
            43 => (((t * PI * 2.0) - FRAC_PI_2).sin() + 1.0) / 2.0,
            44 => (((t * PI * 2.0) - FRAC_PI_2).cos() + 1.0) / 2.0,
            BEZIER_SPLINE => bezier_value(&self.bezier, t),
            _ => t,
        }
    }
}

fn out_in(t: f64, out: fn(f64) -> f64, inn: fn(f64) -> f64) -> f64 {
    if t < 0.5 {
        out(2.0 * t) / 2.0
    } else {
        inn(2.0 * t - 1.0) / 2.0 + 0.5
    }
}
fn in_out_quad(t: f64) -> f64 {
    let t = t * 2.0;
    if t < 1.0 {
        t * t / 2.0
    } else {
        let t = t - 1.0;
        -0.5 * (t * (t - 2.0) - 1.0)
    }
}
fn out_cubic(t: f64) -> f64 {
    let t = t - 1.0;
    t * t * t + 1.0
}
fn out_quart(t: f64) -> f64 {
    let t = t - 1.0;
    -(t * t * t * t - 1.0)
}
fn out_quint(t: f64) -> f64 {
    let t = t - 1.0;
    t * t * t * t * t + 1.0
}
fn in_sine(t: f64) -> f64 {
    if t == 1.0 {
        1.0
    } else {
        -(t * FRAC_PI_2).cos() + 1.0
    }
}
fn in_expo(t: f64) -> f64 {
    if t == 0.0 || t == 1.0 {
        t
    } else {
        2f64.powf(10.0 * (t - 1.0)) - 0.001
    }
}
fn out_expo(t: f64) -> f64 {
    if t == 1.0 {
        1.0
    } else {
        1.001 * (-(2.0f32 as f64).powf(-10.0 * t) + 1.0)
    }
}
fn in_circ(t: f64) -> f64 {
    -((1.0 - t * t).sqrt() - 1.0)
}
fn out_circ(t: f64) -> f64 {
    let t = t - 1.0;
    (1.0 - t * t).sqrt()
}
fn in_elastic_h(t: f64, b: f64, c: f64, d: f64, mut a: f64, p: f64) -> f64 {
    if t == 0.0 {
        return b;
    }
    let mut t_adj = t / d;
    if t_adj == 1.0 {
        return b + c;
    }
    let s = if a < c.abs() {
        a = c;
        p / 4.0f32 as f64
    } else {
        p / (2.0 * PI) * (c / a).asin()
    };
    t_adj -= 1.0;
    -(a * 2f64.powf(10.0 * t_adj) * ((t_adj * d - s) * (2.0 * PI) / p).sin()) + b
}
fn out_elastic_h(t: f64, _b: f64, c: f64, _d: f64, mut a: f64, p: f64) -> f64 {
    if t == 0.0 {
        return 0.0;
    }
    if t == 1.0 {
        return c;
    }
    let s = if a < c {
        a = c;
        p / 4.0
    } else {
        p / (2.0 * PI) * (c / a).asin()
    };
    a * 2f64.powf(-10.0 * t) * ((t - s) * (2.0 * PI) / p).sin() + c
}
fn in_out_elastic(t: f64, mut a: f64, p: f64) -> f64 {
    if t == 0.0 {
        return 0.0;
    }
    let t = t * 2.0;
    if t == 2.0 {
        return 1.0;
    }
    let s = if a < 1.0 {
        a = 1.0;
        p / 4.0
    } else {
        p / (2.0 * PI) * (1.0 / a).asin()
    };
    if t < 1.0 {
        return -0.5 * (a * 2f64.powf(10.0 * (t - 1.0)) * ((t - 1.0 - s) * (2.0 * PI) / p).sin());
    }
    a * 2f64.powf(-10.0 * (t - 1.0)) * ((t - 1.0 - s) * (2.0 * PI) / p).sin() * 0.5 + 1.0
}
fn in_back(t: f64, s: f64) -> f64 {
    t * t * ((s + 1.0) * t - s)
}
fn out_back(t: f64, s: f64) -> f64 {
    let t = t - 1.0;
    t * t * ((s + 1.0) * t + s) + 1.0
}
fn out_bounce_h(t: f64, c: f64, a: f64) -> f64 {
    if t == 1.0 {
        return c;
    }
    if t < 4.0 / 11.0 {
        c * (7.5625 * t * t)
    } else if t < 8.0 / 11.0 {
        let t = t - 6.0 / 11.0;
        -a * (1.0 - (7.5625 * t * t + 0.75)) + c
    } else if t < 10.0 / 11.0 {
        let t = t - 9.0 / 11.0;
        -a * (1.0 - (7.5625 * t * t + 0.9375)) + c
    } else {
        let t = t - 21.0 / 22.0;
        -a * (1.0 - (7.5625 * t * t + 0.984375)) + c
    }
}
fn sin_progress(v: f64) -> f64 {
    ((v * PI) - FRAC_PI_2).sin() / 2.0 + 0.5
}
fn smooth_mix(v: f64) -> f64 {
    (1.0 - v * 2.0 + 0.3).clamp(0.0, 1.0)
}

type Seg = [f64; 8];

fn bezier_value(pts: &[(f64, f64)], x: f64) -> f64 {
    if pts.is_empty() {
        return x;
    }
    if !pts.len().is_multiple_of(3) || pts.last() != Some(&(1.0, 1.0)) {
        return x;
    }
    if !(x > 0.0) {
        return 0.0;
    }
    if !(x < 1.0) {
        return 1.0;
    }
    let n = pts.len() / 3;
    let mut seg: Seg = [0.0; 8];
    let mut found = n;
    for i in 0..n {
        if x <= pts[i * 3 + 2].0 {
            found = i;
            break;
        }
    }
    // Qt indexes one past the last segment when x exceeds every interval (never for x < 1).
    let i = found.min(n - 1);
    let p0 = if i == 0 { (0.0, 0.0) } else { pts[i * 3 - 1] };
    let (p1, p2, p3) = (pts[i * 3], pts[i * 3 + 1], pts[i * 3 + 2]);
    seg.copy_from_slice(&[p0.0, p0.1, p1.0, p1.1, p2.0, p2.1, p3.0, p3.1]);
    let t = find_t_for_x(&seg, x);
    let s = 1.0 - t;
    s * s * s * seg[1] + 3.0 * s * s * t * seg[3] + 3.0 * s * t * t * seg[5] + t * t * t * seg[7]
}

fn almost_zero(v: f64) -> bool {
    v > -1e-3 && v < 1e-3
}

fn find_t_for_x(s: &Seg, x: f64) -> f64 {
    let (p0, p1, p2, p3) = (s[0], s[2], s[4], s[6]);
    let f3 = p3 - p0 + 3.0 * p1 - 3.0 * p2;
    let f2 = 3.0 * p0 - 6.0 * p1 + 3.0 * p2;
    let f1 = -3.0 * p0 + 3.0 * p1;
    let f0 = p0 - x;
    if almost_zero(f3) {
        if almost_zero(f2) {
            if almost_zero(f1) {
                return 0.0;
            }
            return -f0 / f1;
        }
        let d = f1 * f1 - 4.0 * f2 * f0;
        if d < 0.0 {
            return 0.0;
        }
        if d == 0.0 {
            return -f1 / (2.0 * f2);
        }
        let s1 = (-f1 + d.sqrt()) / (2.0 * f2);
        if (0.0..=1.0).contains(&s1) {
            return s1;
        }
        let s2 = (-f1 - d.sqrt()) / (2.0 * f2);
        if (0.0..=1.0).contains(&s2) {
            return s2;
        }
        return 0.0;
    }
    single_real_solution_for_cubic(f2 / f3, f1 / f3, f0 / f3)
}

fn fast_cbrt(d: f64) -> f64 {
    const B1: u32 = 715094163;
    let hx = (d.to_bits() >> 32) as u32;
    f64::from_bits(((hx / 3 + B1) as u64) << 32)
}

fn cbrt(d: f64) -> f64 {
    let sign = if d < 0.0 { -1.0 } else { 1.0 };
    let d = d * sign;
    let mut t = fast_cbrt(d);
    let tc = t * t * t;
    let f = tc + tc + d;
    if f != 0.0 {
        t = t * (tc + d + d) / f;
    }
    t * sign
}

fn in_range(f: f64) -> bool {
    (-0.01..=1.01).contains(&f)
}

fn single_real_solution_for_cubic(a: f64, b: f64, c: f64) -> f64 {
    if c < 0.000001 && c > -0.000001 {
        return 0.0;
    }
    let a_by3 = a / 3.0;
    let a_cubic = a * a * a;
    let p = b - a * a_by3;
    let q = 2.0 * a_cubic / 27.0 - a * b / 3.0 + c;
    let d = 0.25 * q * q + p * p * p / 27.0;
    if d >= 0.0 {
        let ds = d.sqrt();
        let u = cbrt(-q * 0.5 + ds);
        let v = cbrt(-q * 0.5 - ds);
        let t1 = u + v - a_by3;
        if in_range(t1) {
            return t1;
        }
        return -u - a_by3;
    }
    let pms = (-p).sqrt();
    let f = (4.0f64 / 3.0).sqrt() * pms;
    let sqrt_p = -3.0 * 3f64.sqrt() / (pms * p);
    let g = -q * 0.5 * sqrt_p;
    let (xs, xp1, om1) = (g * g, (1.0 + g).sqrt(), (1.0 - g).sqrt());
    let s1 = 0.463614 - 0.0347815 * g + 0.00218245 * xs + 0.402421 * xp1;
    let s3 = 0.463614 + 0.402421 * om1 + 0.0347815 * g + 0.00218245 * xs;
    let s2 = -0.401644 * om1 - 0.0686804 * g + 0.401644 * xp1;
    let t1 = -f * s2 - a_by3;
    if in_range(t1) {
        return t1;
    }
    let t2 = f * s1 - a_by3;
    if in_range(t2) {
        return t2;
    }
    -f * s3 - a_by3
}
