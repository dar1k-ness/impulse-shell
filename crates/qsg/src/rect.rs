//! Port of Qt 6.12 QSGBasicInternalRectangleNode::updateGeometry (no gradient stops).
//! Produces the same triangle-strip geometry Qt Quick feeds to the smoothcolor shader.

use crate::qmath::{q_fast_cos, q_fast_sin};

#[derive(Clone, Copy, Debug, Default)]
pub struct SmoothVertex {
    pub x: f32,
    pub y: f32,
    pub color: [u8; 4],
    pub dx: f32,
    pub dy: f32,
}

impl SmoothVertex {
    fn set(&mut self, primary: f32, secondary: f32, c: [u8; 4], dp: f32, ds: f32, vertical: bool) {
        if vertical {
            self.x = secondary;
            self.y = primary;
            self.dx = ds;
            self.dy = dp;
        } else {
            self.x = primary;
            self.y = secondary;
            self.dx = dp;
            self.dy = ds;
        }
        self.color = c;
    }
}

#[derive(Clone, Debug)]
pub struct RectParams {
    /// The rectangle's left and top in its node (QSGInternalRectangleNode::setRect). The
    /// antialiased edge of a straight rectangle is measured from the node's origin, not the
    /// rectangle's, so one set away from it is not the same rectangle moved.
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub color: [u8; 4],
    pub border_color: [u8; 4],
    pub pen_width: f32,
    pub radius: f32,
    pub tl: Option<f32>,
    pub tr: Option<f32>,
    pub bl: Option<f32>,
    pub br: Option<f32>,
    pub antialiasing: bool,
    pub aligned: bool,
    pub vertical: bool,
    /// Gradient stops (position, RGBA), sorted.
    pub stops: Vec<(f32, [u8; 4])>,
}

impl Default for RectParams {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
            color: [255, 255, 255, 255],
            border_color: [0, 0, 0, 255],
            pen_width: 0.0,
            radius: 0.0,
            tl: None,
            tr: None,
            bl: None,
            br: None,
            antialiasing: false,
            aligned: true,
            vertical: true,
            stops: Vec::new(),
        }
    }
}

/// QColor -> premultiplied Color4ub exactly like colorToColor4ub().
pub fn color4ub(rgba: [u8; 4]) -> [u8; 4] {
    let a = rgba[3] as f32 / 255.0;
    let f = |c: u8| ((c as f32 / 255.0) * a * 255.0).round_half_away_qt() as u8;
    [
        f(rgba[0]),
        f(rgba[1]),
        f(rgba[2]),
        (a * 255.0).round_half_away_qt() as u8,
    ]
}

trait QRound {
    fn round_half_away_qt(self) -> f32;
}
impl QRound for f32 {
    // qRound(float): d >= 0 ? int(d + 0.5f) : int(d - 0.5f) (Qt 6 uses std::round semantics)
    fn round_half_away_qt(self) -> f32 {
        self.round()
    }
}

pub struct Geometry {
    pub vertices: Vec<SmoothVertex>,
    pub indices: Vec<u16>,
}

pub fn build(p: &RectParams) -> Geometry {
    let width = p.width;
    let height = p.height;
    // QQuickRectangle::updatePaintNode makes no node for an empty item. The vertex counts
    // below also assume an area: with none, the pen width is 0 and the gradient length
    // divides by zero.
    if !(width > 0.0 && height > 0.0) {
        return Geometry {
            vertices: Vec::new(),
            indices: Vec::new(),
        };
    }
    let mut pen_width = (width.min(height) * 0.5).min(p.pen_width);
    if p.aligned {
        pen_width = pen_width.round();
    }
    let mut fill_color = color4ub(p.color);
    let border_color = color4ub(p.border_color);
    let transparent = [0u8; 4];
    let vert = p.vertical;
    let (left, top, right, bottom) = (p.x, p.y, p.x + width, p.y + height);
    let gradient_start = if vert { top } else { left };
    let gradient_length = if vert { height } else { width };
    let secondary_length = if vert { width } else { height };
    let stops: Vec<(f32, [u8; 4])> = p.stops.iter().map(|&(t, c)| (t, color4ub(c))).collect();
    let has_fill = p.color[3] > 0 || !stops.is_empty();
    let mut next_stop: isize = 0;
    {
        let gp = pen_width / gradient_length;
        while (next_stop as usize) < stops.len() && stops[next_stop as usize].0 <= gp {
            next_stop += 1;
        }
    }
    let mut last_stop: isize = stops.len() as isize - 1;
    {
        let lp = 1.0 - pen_width / gradient_length;
        while last_stop >= next_stop && stops[last_stop as usize].0 >= lp {
            last_stop -= 1;
        }
    }
    let intersections = (last_stop - next_stop + 1) as usize;
    let lerp4 = |a: [u8; 4], b: [u8; 4], t: f64| -> [u8; 4] {
        std::array::from_fn(|k| {
            ((a[k] as f64 * (1.0 - t)) as u8).wrapping_add((b[k] as f64 * t) as u8)
        })
    };
    let fill_color_from_x = |x: f32| -> [u8; 4] {
        let mut t = ((x - gradient_start) / gradient_length).clamp(0.0, 1.0);
        if t < stops[0].0 {
            return stops[0].1;
        }
        for i in 1..stops.len() {
            let (a, b) = (stops[i - 1], stops[i]);
            if a.0 <= t && b.0 > t {
                t = (t - a.0) / (b.0 - a.0);
                return lerp4(a.1, b.1, t as f64);
            }
        }
        stops[stops.len() - 1].1
    };
    let aa = p.antialiasing;
    let pen = pen_width != 0.0;

    let any_radius =
        p.radius > 0.0 || p.tl.is_some() || p.tr.is_some() || p.bl.is_some() || p.br.is_some();
    if any_radius {
        let lim = width.min(height) * 0.4999f32;
        let mut radius_tl = lim.min(p.tl.unwrap_or(p.radius));
        let mut radius_tr = lim.min(p.tr.unwrap_or(p.radius));
        let mut radius_bl = lim.min(p.bl.unwrap_or(p.radius));
        let mut radius_br = lim.min(p.br.unwrap_or(p.radius));
        for r in [
            &mut radius_tl,
            &mut radius_tr,
            &mut radius_bl,
            &mut radius_br,
        ] {
            if *r <= 0.5 {
                *r = 0.0;
            }
        }
        let inner = |r: f32| ((r - pen_width * 1.0f32) as f64).max(0.01) as f32;
        let inner_tl = inner(radius_tl);
        let inner_tr = inner(radius_tr);
        let inner_bl = inner(radius_bl);
        let inner_br = inner(radius_br);
        let (outer_tl, outer_tr, outer_bl, outer_br) = (radius_tl, radius_tr, radius_bl, radius_br);
        let delta = width.min(height) * 0.5f32;
        let segs = |r: f32| -> i32 {
            if r == 0.0 {
                0
            } else {
                ((r as f64 * (std::f64::consts::PI / 6.0)).ceil() as i32).clamp(3, 18)
            }
        };
        let (mut seg_tl, mut seg_tr, mut seg_bl, mut seg_br) = (
            segs(radius_tl),
            segs(radius_tr),
            segs(radius_bl),
            segs(radius_br),
        );
        if vert {
            if inner_tl == inner_tr {
                if seg_tl <= seg_tr {
                    seg_tl = 0
                } else {
                    seg_tr = 0
                }
            }
            if inner_bl == inner_br {
                if seg_bl <= seg_br {
                    seg_bl = 0
                } else {
                    seg_br = 0
                }
            }
        } else {
            if inner_tl == inner_bl {
                if seg_tl <= seg_bl {
                    seg_tl = 0
                } else {
                    seg_bl = 0
                }
            }
            if inner_tr == inner_br {
                if seg_tr <= seg_br {
                    seg_tr = 0
                } else {
                    seg_br = 0
                }
            }
        }
        let sum = seg_tl + seg_tr + seg_bl + seg_br;
        let inner_vc = ((sum + 4) * 2) as usize + intersections * 2;
        let outer_vc = ((sum + 4) * 2) as usize;
        let mut vc = inner_vc;
        if aa || pen {
            vc += inner_vc;
        }
        if pen {
            vc += outer_vc;
        }
        if aa && pen {
            vc += outer_vc;
        }
        let fill_ic = inner_vc;
        let inner_aa_ic = inner_vc * 2 + 2;
        let border_ic = inner_vc * 2 + 2;
        let outer_aa_ic = outer_vc * 2 + 2;
        let mut ic = 0usize;
        let mut fill_head = 0usize;
        let (mut iaa_h, mut iaa_t, mut b_h, mut b_t, mut oaa_h, mut oaa_t) =
            (0usize, 0usize, 0usize, 0usize, 0usize, 0usize);
        if has_fill {
            ic += fill_ic;
        }
        if aa {
            iaa_h = ic + (inner_aa_ic >> 1) + 1;
            iaa_t = iaa_h;
            ic += inner_aa_ic;
        }
        if pen {
            b_h = ic + (border_ic >> 1) + 1;
            b_t = b_h;
            ic += border_ic;
        }
        if aa && pen {
            oaa_h = ic + (outer_aa_ic >> 1) + 1;
            oaa_t = oaa_h;
            ic += outer_aa_ic;
        }
        let mut v = vec![SmoothVertex::default(); vc];
        let mut idx = vec![0u16; ic];
        let mut index: usize = 0;

        let mut inner_x_prev = 0.0f32;
        let mut inner_y_left_prev = 0.0f32;
        let mut inner_y_right_prev = 0.0f32;
        let step = |s: i32| {
            let a = 0.5f32 * std::f32::consts::PI / s as f32;
            (q_fast_cos(a as f64) as f32, q_fast_sin(a as f64) as f32)
        };
        let (cos_tl, sin_tl) = step(seg_tl);
        let (cos_tr, sin_tr) = step(seg_tr);
        let (cos_bl, sin_bl) = step(seg_bl);
        let (cos_br, sin_br) = step(seg_br);

        let outer_x_center = [
            [
                if vert {
                    top + radius_tl
                } else {
                    left + radius_tl
                },
                if vert {
                    top + radius_tr
                } else {
                    left + radius_bl
                },
            ],
            [
                if vert {
                    bottom - radius_bl
                } else {
                    right - radius_tr
                },
                if vert {
                    bottom - radius_br
                } else {
                    right - radius_br
                },
            ],
        ];
        let outer_y_center = [
            [
                if !vert {
                    top + outer_tl
                } else {
                    left + outer_tl
                },
                if !vert {
                    bottom - outer_bl
                } else {
                    right - outer_tr
                },
            ],
            [
                if !vert {
                    top + outer_tr
                } else {
                    left + outer_bl
                },
                if !vert {
                    bottom - outer_br
                } else {
                    right - outer_br
                },
            ],
        ];
        let inner_x_center = [
            [
                if vert {
                    top + inner_tl + pen_width
                } else {
                    left + inner_tl + pen_width
                },
                if vert {
                    top + inner_tr + pen_width
                } else {
                    left + inner_bl + pen_width
                },
            ],
            [
                if vert {
                    bottom - inner_bl - pen_width
                } else {
                    right - inner_tr - pen_width
                },
                if vert {
                    bottom - inner_br - pen_width
                } else {
                    right - inner_br - pen_width
                },
            ],
        ];
        let inner_y_center = [
            [
                if !vert {
                    top + inner_tl + pen_width
                } else {
                    left + inner_tl + pen_width
                },
                if !vert {
                    bottom - inner_bl - pen_width
                } else {
                    right - inner_tr - pen_width
                },
            ],
            [
                if !vert {
                    top + inner_tr + pen_width
                } else {
                    left + inner_bl + pen_width
                },
                if !vert {
                    bottom - inner_br - pen_width
                } else {
                    right - inner_br - pen_width
                },
            ],
        ];
        let inner_radius = [
            [inner_tl, if !vert { inner_bl } else { inner_tr }],
            [if !vert { inner_tr } else { inner_bl }, inner_br],
        ];
        let outer_radius = [
            [outer_tl, if !vert { outer_bl } else { outer_tr }],
            [if !vert { outer_tr } else { outer_bl }, outer_br],
        ];
        let segments = [
            [seg_tl, if !vert { seg_bl } else { seg_tr }],
            [if !vert { seg_tr } else { seg_bl }, seg_br],
        ];
        let cos_step = [
            [cos_tl, if !vert { cos_bl } else { cos_tr }],
            [if !vert { cos_tr } else { cos_bl }, cos_br],
        ];
        let sin_step = [
            [sin_tl, if !vert { sin_bl } else { sin_tr }],
            [if !vert { sin_tr } else { sin_bl }, sin_br],
        ];

        for part in 0..2usize {
            let mut cos_l: f32 = (1.0f64 - part as f64) as f32;
            let mut sin_l: f32 = part as f32;
            let mut cos_r: f32 = (1.0f64 - part as f64) as f32;
            let mut sin_r: f32 = part as f32;
            let (mut i_left, mut i_right) = (0i32, 0i32);
            while i_left <= segments[part][0] || i_right <= segments[part][1] {
                let mut x_left = inner_x_center[part][0] - inner_radius[part][0] * cos_l;
                let mut x_right = inner_x_center[part][1] - inner_radius[part][1] * cos_r;
                let mut y_left = inner_y_center[part][0] - inner_radius[part][0] * sin_l;
                let mut y_right = inner_y_center[part][1] + inner_radius[part][1] * sin_r;
                let advance_left = (i_left <= segments[part][0] && x_left <= x_right)
                    || i_right > segments[part][1];
                let (sin_al, cos_al, sin_ar, cos_ar): (f32, f32, f32, f32);
                let corner =
                    |part: usize| -> (f32, f32) { (1.0, if part != 0 { -1.0 } else { 1.0 }) };
                if inner_radius[part][0] == inner_radius[part][1] {
                    if advance_left {
                        let (s, c) = if outer_radius[part][0] == 0.0 {
                            corner(part)
                        } else {
                            (sin_l, cos_l)
                        };
                        sin_al = s;
                        cos_al = c;
                        let (s, c) = if outer_radius[part][1] == 0.0 {
                            corner(part)
                        } else {
                            (sin_l, cos_l)
                        };
                        sin_ar = s;
                        cos_ar = c;
                        x_right = x_left;
                        y_right = inner_y_center[part][1] + inner_radius[part][1] * sin_ar;
                    } else {
                        let (s, c) = if outer_radius[part][0] == 0.0 {
                            corner(part)
                        } else {
                            (sin_r, cos_r)
                        };
                        sin_al = s;
                        cos_al = c;
                        let (s, c) = if outer_radius[part][1] == 0.0 {
                            corner(part)
                        } else {
                            (sin_r, cos_r)
                        };
                        sin_ar = s;
                        cos_ar = c;
                        x_left = x_right;
                        y_left = inner_y_center[part][0] - inner_radius[part][0] * sin_al;
                    }
                } else if advance_left {
                    let (s, c) = if outer_radius[part][0] == 0.0 {
                        corner(part)
                    } else {
                        (sin_l, cos_l)
                    };
                    sin_al = s;
                    cos_al = c;
                    if outer_radius[part][1] == 0.0 {
                        let (s, c) = corner(part);
                        sin_ar = s;
                        cos_ar = c;
                        x_right = x_left;
                        y_right = inner_y_center[part][1] + inner_radius[part][1] * sin_ar;
                    } else if x_left >= inner_x_center[0][1] && x_left <= inner_x_center[1][1] {
                        sin_ar = 1.0;
                        cos_ar = 0.0;
                        x_right = x_left;
                        y_right = inner_y_center[part][1] + inner_radius[part][1] * sin_ar;
                    } else {
                        if x_right != inner_x_prev {
                            let t = (x_left - inner_x_prev) / (x_right - inner_x_prev);
                            y_right = (inner_y_right_prev as f64 * (1.0 - t as f64)
                                + (y_right * t) as f64)
                                as f32;
                            x_right = x_left;
                        }
                        sin_ar = (y_right - inner_y_center[part][1]) / inner_radius[part][1];
                        cos_ar = -(x_right - inner_x_center[part][1]) / inner_radius[part][1];
                    }
                } else {
                    let (s, c) = if outer_radius[part][1] == 0.0 {
                        corner(part)
                    } else {
                        (sin_r, cos_r)
                    };
                    sin_ar = s;
                    cos_ar = c;
                    if outer_radius[part][0] == 0.0 {
                        let (s, c) = corner(part);
                        sin_al = s;
                        cos_al = c;
                        x_left = x_right;
                        y_left = inner_y_center[part][0] - inner_radius[part][0] * sin_al;
                    } else if x_right >= inner_x_center[0][0] && x_right <= inner_x_center[1][0] {
                        sin_al = 1.0;
                        cos_al = 0.0;
                        x_left = x_right;
                        y_left = inner_y_center[part][0] - inner_radius[part][0] * sin_al;
                    } else {
                        if x_left != inner_x_prev {
                            let t = (x_right - inner_x_prev) / (x_left - inner_x_prev);
                            y_left = (inner_y_left_prev as f64 * (1.0 - t as f64)
                                + (y_left * t) as f64) as f32;
                            x_left = x_right;
                        }
                        sin_al = -(y_left - inner_y_center[part][0]) / inner_radius[part][0];
                        cos_al = -(x_left - inner_x_center[part][0]) / inner_radius[part][0];
                    }
                }
                let outer_x_left = outer_x_center[part][0] - outer_radius[part][0] * cos_al;
                let outer_y_left = outer_y_center[part][0] - outer_radius[part][0] * sin_al;
                let outer_x_right = outer_x_center[part][1] - outer_radius[part][1] * cos_ar;
                let outer_y_right = outer_y_center[part][1] + outer_radius[part][1] * sin_ar;
                let gradient_pos = (x_left - gradient_start) / gradient_length;
                while next_stop <= last_stop && stops[next_stop as usize].0 <= gradient_pos {
                    let spos = stops[next_stop as usize].0;
                    let gx = gradient_start + spos * gradient_length;
                    let t = ((gx - inner_x_prev) / (x_left - inner_x_prev)) as f64;
                    let gyl = (inner_y_left_prev as f64 * (1.0 - t) + t * y_left as f64) as f32;
                    let gyr = (inner_y_right_prev as f64 * (1.0 - t) + t * y_right as f64) as f32;
                    let fc = fill_color_from_x(gx);
                    if has_fill {
                        idx[fill_head] = index as u16;
                        idx[fill_head + 1] = (index + 1) as u16;
                        fill_head += 2;
                    }
                    if pen {
                        b_h -= 1;
                        idx[b_h] = idx[b_h + 2];
                        b_h -= 1;
                        idx[b_h] = (index + 2) as u16;
                        idx[b_t] = (index + 3) as u16;
                        b_t += 1;
                        idx[b_t] = idx[b_t - 2];
                        b_t += 1;
                    }
                    if aa {
                        iaa_h -= 1;
                        idx[iaa_h] = (index + 2) as u16;
                        iaa_h -= 1;
                        idx[iaa_h] = index as u16;
                        idx[iaa_t] = (index + 1) as u16;
                        iaa_t += 1;
                        idx[iaa_t] = (index + 3) as u16;
                        iaa_t += 1;
                        let lower = spos > 0.5;
                        let dp = if lower {
                            0.0f32.min(gradient_length - gx - delta)
                        } else {
                            0.0f32.max(delta - gx)
                        };
                        v[index].set(gx, gyr, fc, dp, secondary_length - gyr - delta, vert);
                        v[index + 1].set(gx, gyl, fc, dp, delta - gyl, vert);
                        index += 2;
                        if pen {
                            let h = 0.49f32 * pen_width;
                            v[index].set(gx, gyr, border_color, -h * cos_ar, h * sin_ar, vert);
                            v[index + 1].set(gx, gyl, border_color, -h * cos_al, -h * sin_al, vert);
                        } else {
                            let d = if lower { delta } else { -delta };
                            v[index].set(gx, gyr, transparent, d, delta, vert);
                            v[index + 1].set(gx, gyl, transparent, d, -delta, vert);
                        }
                        index += 2;
                    } else {
                        v[index].set(gx, gyr, fc, 0.0, 0.0, vert);
                        v[index + 1].set(gx, gyl, fc, 0.0, 0.0, vert);
                        index += 2;
                        if pen {
                            v[index].set(gx, gyr, border_color, 0.0, 0.0, vert);
                            v[index + 1].set(gx, gyl, border_color, 0.0, 0.0, vert);
                            index += 2;
                        }
                    }
                    inner_x_prev = gx;
                    inner_y_left_prev = gyl;
                    inner_y_right_prev = gyr;
                    next_stop += 1;
                }
                if !stops.is_empty() {
                    fill_color = fill_color_from_x(x_left);
                }

                if has_fill {
                    idx[fill_head] = index as u16;
                    fill_head += 1;
                    idx[fill_head] = (index + 1) as u16;
                    fill_head += 1;
                }
                if pen {
                    b_h -= 1;
                    idx[b_h] = (index + 4) as u16;
                    b_h -= 1;
                    idx[b_h] = (index + 2) as u16;
                    idx[b_t] = (index + 3) as u16;
                    b_t += 1;
                    idx[b_t] = (index + 5) as u16;
                    b_t += 1;
                }
                if aa {
                    iaa_h -= 1;
                    idx[iaa_h] = (index + 2) as u16;
                    iaa_h -= 1;
                    idx[iaa_h] = index as u16;
                    idx[iaa_t] = (index + 1) as u16;
                    iaa_t += 1;
                    idx[iaa_t] = (index + 3) as u16;
                    iaa_t += 1;
                    let mut dp = if part != 0 {
                        0.0f32.min(gradient_length - x_right - delta)
                    } else {
                        0.0f32.max(delta - x_right)
                    };
                    v[index].set(
                        x_right,
                        y_right,
                        fill_color,
                        dp,
                        secondary_length - y_right - delta,
                        vert,
                    );
                    index += 1;
                    v[index].set(x_left, y_left, fill_color, dp, delta - y_left, vert);
                    index += 1;
                    dp = if part != 0 { delta } else { -delta };
                    if pen {
                        let h = 0.49f32 * pen_width;
                        v[index].set(
                            x_right,
                            y_right,
                            border_color,
                            -h * cos_ar,
                            h * sin_ar,
                            vert,
                        );
                        index += 1;
                        v[index].set(x_left, y_left, border_color, -h * cos_al, -h * sin_al, vert);
                        index += 1;
                        v[index].set(
                            outer_x_right,
                            outer_y_right,
                            border_color,
                            h * cos_ar,
                            -h * sin_ar,
                            vert,
                        );
                        index += 1;
                        v[index].set(
                            outer_x_left,
                            outer_y_left,
                            border_color,
                            h * cos_al,
                            h * sin_al,
                            vert,
                        );
                        index += 1;
                        v[index].set(outer_x_right, outer_y_right, transparent, dp, delta, vert);
                        index += 1;
                        v[index].set(outer_x_left, outer_y_left, transparent, dp, -delta, vert);
                        index += 1;
                        oaa_h -= 1;
                        idx[oaa_h] = (index - 2) as u16;
                        oaa_h -= 1;
                        idx[oaa_h] = (index - 4) as u16;
                        idx[oaa_t] = (index - 3) as u16;
                        oaa_t += 1;
                        idx[oaa_t] = (index - 1) as u16;
                        oaa_t += 1;
                    } else {
                        v[index].set(x_right, y_right, transparent, dp, delta, vert);
                        index += 1;
                        v[index].set(x_left, y_left, transparent, dp, -delta, vert);
                        index += 1;
                    }
                } else {
                    v[index].set(x_right, y_right, fill_color, 0.0, 0.0, vert);
                    index += 1;
                    v[index].set(x_left, y_left, fill_color, 0.0, 0.0, vert);
                    index += 1;
                    if pen {
                        v[index].set(x_right, y_right, border_color, 0.0, 0.0, vert);
                        index += 1;
                        v[index].set(x_left, y_left, border_color, 0.0, 0.0, vert);
                        index += 1;
                        v[index].set(outer_x_right, outer_y_right, border_color, 0.0, 0.0, vert);
                        index += 1;
                        v[index].set(outer_x_left, outer_y_left, border_color, 0.0, 0.0, vert);
                        index += 1;
                    }
                }
                inner_x_prev = x_left;
                inner_y_left_prev = y_left;
                inner_y_right_prev = y_right;
                if advance_left {
                    i_left += 1;
                    let tmp = cos_l as f64;
                    cos_l = cos_l * cos_step[part][0] - sin_l * sin_step[part][0];
                    sin_l = ((sin_l * cos_step[part][0]) as f64 + tmp * sin_step[part][0] as f64)
                        as f32;
                } else {
                    i_right += 1;
                    let tmp = cos_r as f64;
                    cos_r = cos_r * cos_step[part][1] - sin_r * sin_step[part][1];
                    sin_r = ((sin_r * cos_step[part][1]) as f64 + tmp * sin_step[part][1] as f64)
                        as f32;
                }
            }
        }
        debug_assert_eq!(index, vc);
        close_strips(
            &mut idx, aa, pen, &mut iaa_h, iaa_t, &mut b_h, b_t, &mut oaa_h, oaa_t,
        );
        Geometry {
            vertices: v,
            indices: idx,
        }
    } else {
        let (mut il, mut it, mut ir, mut ib) = (left, top, right, bottom);
        if pen {
            il += pen_width;
            it += pen_width;
            ir -= pen_width;
            ib -= pen_width;
        }
        let delta = width.min(height) * 0.5f32;
        let inner_vc = 4usize + intersections * 2;
        let outer_vc = 4usize;
        let mut vc = inner_vc;
        if aa || pen {
            vc += inner_vc;
        }
        if pen {
            vc += outer_vc;
        }
        if aa && pen {
            vc += outer_vc;
        }
        let (fill_ic, inner_aa_ic, border_ic, outer_aa_ic) = (
            inner_vc,
            inner_vc * 2 + 2,
            inner_vc * 2 + 2,
            outer_vc * 2 + 2,
        );
        let mut ic = 0usize;
        let mut fill_head = 0usize;
        let (mut iaa_h, mut iaa_t, mut b_h, mut b_t, mut oaa_h, mut oaa_t) =
            (0usize, 0usize, 0usize, 0usize, 0usize, 0usize);
        if has_fill {
            ic += fill_ic;
        }
        if aa {
            iaa_h = ic + (inner_aa_ic >> 1) + 1;
            iaa_t = iaa_h;
            ic += inner_aa_ic;
        }
        if pen {
            b_h = ic + (border_ic >> 1) + 1;
            b_t = b_h;
            ic += border_ic;
        }
        if aa && pen {
            oaa_h = ic + (outer_aa_ic >> 1) + 1;
            oaa_t = oaa_h;
            ic += outer_aa_ic;
        }
        let mut v = vec![SmoothVertex::default(); vc];
        let mut idx = vec![0u16; ic];
        let mut index = 0usize;
        let inner_start = if vert { it } else { il };
        let inner_end = if vert { ib } else { ir };
        let outer_start = if vert { top } else { left };
        let outer_end = if vert { bottom } else { right };
        let iss = if vert { il } else { it };
        let ise = if vert { ir } else { ib };
        let oss = if vert { left } else { top };
        let ose = if vert { right } else { bottom };
        for part in [-1i32, 1] {
            let inner_edge = if part == 1 { inner_end } else { inner_start };
            let outer_edge = if part == 1 { outer_end } else { outer_start };
            let pf = part as f32;
            let gradient_pos = (inner_edge - inner_start + pen_width) / gradient_length;
            while next_stop <= last_stop && stops[next_stop as usize].0 <= gradient_pos {
                let (spos, scol) = stops[next_stop as usize];
                let gp = (inner_start - pen_width) + spos * gradient_length;
                let fc = scol;
                if has_fill {
                    idx[fill_head] = index as u16;
                    idx[fill_head + 1] = (index + 1) as u16;
                    fill_head += 2;
                }
                if pen {
                    b_h -= 1;
                    idx[b_h] = idx[b_h + 2];
                    b_h -= 1;
                    idx[b_h] = (index + 2) as u16;
                    idx[b_t] = (index + 3) as u16;
                    b_t += 1;
                    idx[b_t] = idx[b_t - 2];
                    b_t += 1;
                }
                if aa {
                    iaa_h -= 1;
                    idx[iaa_h] = (index + 2) as u16;
                    iaa_h -= 1;
                    idx[iaa_h] = index as u16;
                    idx[iaa_t] = (index + 1) as u16;
                    iaa_t += 1;
                    idx[iaa_t] = (index + 3) as u16;
                    iaa_t += 1;
                    let lower = spos > 0.5;
                    let dp = if lower {
                        0.0f32.min(gradient_length - gp - delta)
                    } else {
                        0.0f32.max(delta - gp)
                    };
                    v[index].set(gp, ise, fc, dp, secondary_length - ise - delta, vert);
                    v[index + 1].set(gp, iss, fc, dp, delta - iss, vert);
                    index += 2;
                    if pen {
                        let h = 0.49f32 * pen_width;
                        let sx = if lower { h } else { -h };
                        v[index].set(gp, ise, border_color, sx, h, vert);
                        v[index + 1].set(gp, iss, border_color, sx, -h, vert);
                    } else {
                        let sx = if lower { delta } else { -delta };
                        v[index].set(gp, ise, transparent, sx, delta, vert);
                        v[index + 1].set(gp, iss, transparent, sx, -delta, vert);
                    }
                    index += 2;
                } else {
                    v[index].set(gp, ise, fc, 0.0, 0.0, vert);
                    v[index + 1].set(gp, iss, fc, 0.0, 0.0, vert);
                    index += 2;
                    if pen {
                        v[index].set(gp, ise, border_color, 0.0, 0.0, vert);
                        v[index + 1].set(gp, iss, border_color, 0.0, 0.0, vert);
                        index += 2;
                    }
                }
                next_stop += 1;
            }
            if !stops.is_empty() {
                let n = next_stop as usize;
                fill_color = if n == 0 {
                    stops[0].1
                } else if n == stops.len() {
                    stops[n - 1].1
                } else {
                    let (a, b) = (stops[n - 1], stops[n]);
                    let t = (gradient_pos - a.0) / (b.0 - a.0);
                    std::array::from_fn(|k| {
                        ((a.1[k] as f32 * (1.0 - t)) as u8).wrapping_add((b.1[k] as f32 * t) as u8)
                    })
                };
            }
            if has_fill {
                idx[fill_head] = index as u16;
                fill_head += 1;
                idx[fill_head] = (index + 1) as u16;
                fill_head += 1;
            }
            if pen {
                b_h -= 1;
                idx[b_h] = (index + 4) as u16;
                b_h -= 1;
                idx[b_h] = (index + 2) as u16;
                idx[b_t] = (index + 3) as u16;
                b_t += 1;
                idx[b_t] = (index + 5) as u16;
                b_t += 1;
            }
            if aa {
                iaa_h -= 1;
                idx[iaa_h] = (index + 2) as u16;
                iaa_h -= 1;
                idx[iaa_h] = index as u16;
                idx[iaa_t] = (index + 1) as u16;
                iaa_t += 1;
                idx[iaa_t] = (index + 3) as u16;
                iaa_t += 1;
                let dp = if part == 1 {
                    0.0f32.min(gradient_length - inner_edge - delta)
                } else {
                    0.0f32.max(delta - inner_edge)
                };
                v[index].set(
                    inner_edge,
                    ise,
                    fill_color,
                    dp,
                    secondary_length - ise - delta,
                    vert,
                );
                index += 1;
                v[index].set(inner_edge, iss, fill_color, dp, delta - iss, vert);
                index += 1;
                if pen {
                    let h = 0.49f32 * pen_width;
                    v[index].set(inner_edge, ise, border_color, h * pf, h, vert);
                    index += 1;
                    v[index].set(inner_edge, iss, border_color, h * pf, -h, vert);
                    index += 1;
                    v[index].set(outer_edge, ose, border_color, -h * pf, -h, vert);
                    index += 1;
                    v[index].set(outer_edge, oss, border_color, -h * pf, h, vert);
                    index += 1;
                    v[index].set(outer_edge, ose, transparent, delta * pf, delta, vert);
                    index += 1;
                    v[index].set(outer_edge, oss, transparent, delta * pf, -delta, vert);
                    index += 1;
                    oaa_h -= 1;
                    idx[oaa_h] = (index - 2) as u16;
                    oaa_h -= 1;
                    idx[oaa_h] = (index - 4) as u16;
                    idx[oaa_t] = (index - 3) as u16;
                    oaa_t += 1;
                    idx[oaa_t] = (index - 1) as u16;
                    oaa_t += 1;
                } else {
                    v[index].set(inner_edge, ise, transparent, delta * pf, delta, vert);
                    index += 1;
                    v[index].set(inner_edge, iss, transparent, delta * pf, -delta, vert);
                    index += 1;
                }
            } else {
                v[index].set(inner_edge, ise, fill_color, 0.0, 0.0, vert);
                index += 1;
                v[index].set(inner_edge, iss, fill_color, 0.0, 0.0, vert);
                index += 1;
                if pen {
                    v[index].set(inner_edge, ise, border_color, 0.0, 0.0, vert);
                    index += 1;
                    v[index].set(inner_edge, iss, border_color, 0.0, 0.0, vert);
                    index += 1;
                    v[index].set(outer_edge, ose, border_color, 0.0, 0.0, vert);
                    index += 1;
                    v[index].set(outer_edge, oss, border_color, 0.0, 0.0, vert);
                    index += 1;
                }
            }
        }
        debug_assert_eq!(index, vc);
        close_strips(
            &mut idx, aa, pen, &mut iaa_h, iaa_t, &mut b_h, b_t, &mut oaa_h, oaa_t,
        );
        Geometry {
            vertices: v,
            indices: idx,
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn close_strips(
    idx: &mut [u16],
    aa: bool,
    pen: bool,
    iaa_h: &mut usize,
    iaa_t: usize,
    b_h: &mut usize,
    b_t: usize,
    oaa_h: &mut usize,
    oaa_t: usize,
) {
    if aa {
        *iaa_h -= 1;
        idx[*iaa_h] = idx[iaa_t - 1];
        *iaa_h -= 1;
        idx[*iaa_h] = idx[iaa_t - 2];
    }
    if pen {
        *b_h -= 1;
        idx[*b_h] = idx[b_t - 1];
        *b_h -= 1;
        idx[*b_h] = idx[b_t - 2];
    }
    if aa && pen {
        *oaa_h -= 1;
        idx[*oaa_h] = idx[oaa_t - 1];
        *oaa_h -= 1;
        idx[*oaa_h] = idx[oaa_t - 2];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raster::Canvas;

    const RED: [u8; 4] = [255, 0, 0, 255];
    const BLUE: [u8; 4] = [0, 0, 255, 255];

    /// A rectangle without an area has no geometry, whatever its border, corners and gradient
    /// ask for (the vertex count asserted in `build` used to be off for these).
    #[test]
    fn empty_rectangle_has_no_geometry() {
        for (w, h) in [
            (40.0, 0.0),
            (0.0, 40.0),
            (0.0, 0.0),
            (40.0, -3.0),
            (f32::NAN, 40.0),
        ] {
            for pen_width in [0.0, 1.0, 4.0] {
                for radius in [0.0, 6.0] {
                    for antialiasing in [false, true] {
                        for vertical in [false, true] {
                            for stops in [vec![], vec![(0.0, RED), (0.5, BLUE), (1.0, RED)]] {
                                let g = build(&RectParams {
                                    width: w,
                                    height: h,
                                    color: RED,
                                    border_color: BLUE,
                                    pen_width,
                                    radius,
                                    antialiasing,
                                    vertical,
                                    stops,
                                    ..Default::default()
                                });
                                assert!(
                                    g.vertices.is_empty() && g.indices.is_empty(),
                                    "{w} x {h}, border {pen_width}, radius {radius}: {} vertices",
                                    g.vertices.len()
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    fn params(w: f32, h: f32) -> RectParams {
        RectParams {
            width: w,
            height: h,
            color: RED,
            ..Default::default()
        }
    }

    fn render(p: &RectParams, tx: f32, ty: f32, cw: usize, ch: usize) -> Canvas {
        let mut c = Canvas::new(cw, ch, [0; 4]);
        c.draw_smooth(&build(p), tx, ty, 1.0);
        c
    }

    fn at(c: &Canvas, x: usize, y: usize) -> [u8; 4] {
        c.px[y * c.w + x]
    }

    fn positions(g: &Geometry) -> Vec<(f32, f32)> {
        g.vertices.iter().map(|v| (v.x, v.y)).collect()
    }

    fn assert_well_formed(g: &Geometry, w: f32, h: f32) {
        for &i in &g.indices {
            assert!(
                (i as usize) < g.vertices.len(),
                "index {i} out of {}",
                g.vertices.len()
            );
        }
        for v in &g.vertices {
            assert!(
                v.x >= -1e-3 && v.x <= w + 1e-3 && v.y >= -1e-3 && v.y <= h + 1e-3,
                "{v:?}"
            );
        }
    }

    // colorToColor4ub: premultiplied, qRound(c * a * 255).
    #[test]
    fn color4ub_premultiplies() {
        assert_eq!(color4ub([255, 0, 0, 128]), [128, 0, 0, 128]);
        assert_eq!(color4ub([200, 100, 50, 255]), [200, 100, 50, 255]);
        assert_eq!(color4ub([255, 255, 255, 0]), [0, 0, 0, 0]);
        assert_eq!(color4ub([100, 51, 1, 51]), [20, 10, 0, 51]);
    }

    // Straight corners: innerVertexCount = 4 (+ inner AA, + border, + outer AA strips of 4);
    // index counts fill 4, each AA/border strip 4 * 2 + 2.
    #[test]
    fn straight_corner_vertex_and_index_counts() {
        for (aa, pen, verts, idx) in [
            (false, 0.0, 4, 4),
            (true, 0.0, 8, 14),
            (false, 2.0, 12, 14),
            (true, 2.0, 16, 34),
        ] {
            let g = build(&RectParams {
                antialiasing: aa,
                pen_width: pen,
                ..params(20.0, 10.0)
            });
            assert_eq!(
                (g.vertices.len(), g.indices.len()),
                (verts, idx),
                "aa={aa} pen={pen}"
            );
            assert_well_formed(&g, 20.0, 10.0);
        }
    }

    // hasFill = color.alpha() > 0 || gradient: a transparent fill drops the fill strip and
    // leaves the interior untouched.
    #[test]
    fn transparent_fill_has_no_fill_strip() {
        let p = RectParams {
            color: [255, 0, 0, 0],
            border_color: BLUE,
            pen_width: 2.0,
            ..params(20.0, 10.0)
        };
        let g = build(&p);
        assert_eq!(g.indices.len(), 10);
        let c = render(&p, 0.0, 0.0, 20, 10);
        assert_eq!(at(&c, 10, 5), [0; 4]);
        assert_eq!(at(&c, 0, 5), BLUE);
        assert_eq!(at(&c, 10, 1), BLUE);
    }

    // A non-AA, pixel-aligned rectangle covers exactly its pixels.
    #[test]
    fn plain_rect_covers_its_pixels() {
        let c = render(&params(6.0, 4.0), 2.0, 3.0, 12, 10);
        for y in 0..10 {
            for x in 0..12 {
                let inside = (2..8).contains(&x) && (3..7).contains(&y);
                assert_eq!(at(&c, x, y), if inside { RED } else { [0; 4] }, "({x},{y})");
            }
        }
    }

    // Border: outer ring of penWidth in border colour, fill inside innerRect = rect adjusted by pen.
    #[test]
    fn border_ring_and_fill() {
        let p = RectParams {
            border_color: BLUE,
            pen_width: 2.0,
            ..params(20.0, 12.0)
        };
        let c = render(&p, 0.0, 0.0, 20, 12);
        for (x, y) in [(0, 0), (1, 6), (19, 11), (10, 0), (10, 1), (18, 6)] {
            assert_eq!(at(&c, x, y), BLUE, "({x},{y})");
        }
        for (x, y) in [(2, 2), (10, 6), (17, 9)] {
            assert_eq!(at(&c, x, y), RED, "({x},{y})");
        }
    }

    // penWidth = min(min(w, h) * 0.5, pen): an oversized pen turns the whole rect into border.
    #[test]
    fn pen_width_clamped_to_half_size() {
        let p = RectParams {
            border_color: BLUE,
            pen_width: 100.0,
            ..params(10.0, 6.0)
        };
        let c = render(&p, 0.0, 0.0, 10, 6);
        for y in 0..6 {
            for x in 0..10 {
                assert_eq!(at(&c, x, y), BLUE, "({x},{y})");
            }
        }
    }

    // m_aligned: penWidth = qRound(penWidth); unaligned keeps the fractional width.
    #[test]
    fn aligned_pen_is_rounded() {
        for (pen, border_px) in [(1.4f32, 1usize), (1.6, 2)] {
            let p = RectParams {
                border_color: BLUE,
                pen_width: pen,
                ..params(10.0, 10.0)
            };
            let c = render(&p, 0.0, 0.0, 10, 10);
            for x in 0..border_px {
                assert_eq!(at(&c, x, 5), BLUE, "pen {pen} x {x}");
            }
            assert_eq!(at(&c, border_px, 5), RED, "pen {pen}");
        }
        let g = build(&RectParams {
            pen_width: 1.5,
            aligned: false,
            ..params(10.0, 10.0)
        });
        assert!(positions(&g).iter().any(|&(x, _)| x == 1.5));
    }

    // radius = min(min(w, h) * 0.4999, radius): any larger radius gives the same geometry.
    #[test]
    fn radius_is_clamped() {
        let a = build(&RectParams {
            radius: 1000.0,
            ..params(20.0, 10.0)
        });
        let b = build(&RectParams {
            radius: 10.0 * 0.4999,
            ..params(20.0, 10.0)
        });
        assert_eq!(positions(&a), positions(&b));
        assert_eq!(a.indices, b.indices);
        assert_well_formed(&a, 20.0, 10.0);
    }

    // radius <= 0.5 draws a normal corner.
    #[test]
    fn tiny_radius_is_square() {
        let square = render(&params(10.0, 8.0), 1.0, 1.0, 12, 10);
        for r in [0.3f32, 0.5] {
            let c = render(
                &RectParams {
                    radius: r,
                    ..params(10.0, 8.0)
                },
                1.0,
                1.0,
                12,
                10,
            );
            assert!(c.px == square.px, "radius {r}");
        }
        assert_eq!(at(&square, 1, 1), RED);
    }

    // Rounded corners cut the corner pixels outside the arc and keep the straight edges.
    #[test]
    fn rounded_corners() {
        let p = RectParams {
            radius: 8.0,
            ..params(40.0, 40.0)
        };
        let c = render(&p, 0.0, 0.0, 40, 40);
        for (x, y) in [(0, 0), (1, 1), (39, 0), (0, 39), (38, 38)] {
            assert_eq!(at(&c, x, y), [0; 4], "({x},{y})");
        }
        for (x, y) in [
            (3, 3),
            (20, 0),
            (0, 20),
            (39, 20),
            (20, 39),
            (36, 36),
            (20, 20),
        ] {
            assert_eq!(at(&c, x, y), RED, "({x},{y})");
        }
    }

    // QQuickRectangle per-corner radii (topLeftRadius, ...) override `radius` for that corner.
    #[test]
    fn per_corner_radius() {
        let p = RectParams {
            tl: Some(8.0),
            ..params(30.0, 20.0)
        };
        let c = render(&p, 0.0, 0.0, 30, 20);
        assert_eq!(at(&c, 0, 0), [0; 4]);
        for (x, y) in [(29, 0), (0, 19), (29, 19)] {
            assert_eq!(at(&c, x, y), RED, "({x},{y})");
        }
        let p = RectParams {
            radius: 8.0,
            br: Some(0.0),
            ..params(30.0, 20.0)
        };
        let c = render(&p, 0.0, 0.0, 30, 20);
        assert_eq!(at(&c, 29, 19), RED);
        for (x, y) in [(0, 0), (29, 0), (0, 19)] {
            assert_eq!(at(&c, x, y), [0; 4], "({x},{y})");
        }
    }

    // Antialiasing on a pixel-aligned rect: the half-pixel ramps meet at the edge, so edge
    // pixels stay fully covered and outside pixels empty.
    #[test]
    fn antialiased_aligned_rect_is_crisp() {
        let p = RectParams {
            antialiasing: true,
            ..params(6.0, 4.0)
        };
        let c = render(&p, 2.0, 2.0, 10, 8);
        assert_eq!(at(&c, 2, 2), RED);
        assert_eq!(at(&c, 7, 5), RED);
        assert_eq!(at(&c, 1, 3)[3], 0);
        assert_eq!(at(&c, 8, 3)[3], 0);
        assert_eq!(at(&c, 4, 1)[3], 0);
        assert_eq!(at(&c, 4, 6)[3], 0);
    }

    // Antialiasing on a half-pixel offset: edge pixels are half covered.
    #[test]
    fn antialiased_half_pixel_edge() {
        let p = RectParams {
            antialiasing: true,
            ..params(6.0, 4.0)
        };
        let c = render(&p, 2.5, 2.0, 10, 8);
        let a = at(&c, 2, 3)[3];
        assert!((120..=136).contains(&a), "{a}");
        assert_eq!(at(&c, 4, 3)[3], 255);
    }

    // Gradient stops run along y for a vertical gradient and along x for a horizontal one.
    #[test]
    fn gradient_orientation() {
        for vertical in [true, false] {
            let p = RectParams {
                vertical,
                stops: vec![(0.0, RED), (1.0, BLUE)],
                ..params(20.0, 20.0)
            };
            let c = render(&p, 0.0, 0.0, 20, 20);
            let (first, last) = if vertical {
                (at(&c, 10, 0), at(&c, 10, 19))
            } else {
                (at(&c, 0, 10), at(&c, 19, 10))
            };
            assert!(
                first[0] > 230 && first[2] < 25,
                "vertical={vertical} {first:?}"
            );
            assert!(
                last[2] > 230 && last[0] < 25,
                "vertical={vertical} {last:?}"
            );
            let (a, b) = if vertical {
                (at(&c, 0, 10), at(&c, 19, 10))
            } else {
                (at(&c, 10, 0), at(&c, 10, 19))
            };
            assert_eq!(a, b, "constant across the gradient axis");
        }
    }

    // All AA / pen / radius combinations index inside the vertex buffer and stay within the rect.
    #[test]
    fn geometry_well_formed_for_all_combinations() {
        for aa in [false, true] {
            for pen in [0.0f32, 1.0, 3.0] {
                for (radius, tl) in [(0.0f32, None), (5.0, None), (4.0, Some(9.0)), (100.0, None)] {
                    for vertical in [true, false] {
                        let p = RectParams {
                            antialiasing: aa,
                            pen_width: pen,
                            radius,
                            tl,
                            vertical,
                            ..params(30.0, 18.0)
                        };
                        assert_well_formed(&build(&p), 30.0, 18.0);
                    }
                }
            }
        }
    }
}
