//! Triangles as llvmpipe draws them when the target has a depth buffer, which Qt's always
//! has (Mesa 26.2.6): cut at the viewport's edges in clip space (draw_pipe_clip.c), each
//! piece with its own edges in 1/256 of a pixel (lp_setup_tri.c) and its own planes of the
//! texture coordinates (`Plane`).

use crate::layer::Plane;

/// A vertex a vertex shader gave: `gl_Position` and the texture coordinates.
#[derive(Clone, Copy, Debug)]
pub struct Vert {
    pub pos: [f32; 4],
    pub uv: [f32; 2],
}

/// The viewport's transform (_mesa_get_viewport_xform, st_update_viewport) and the fill
/// rule that goes with the target's orientation (st_atom_rasterizer.c).
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub scale: [f32; 2],
    pub trans: [f32; 2],
    pub bottom_rule: bool,
}

/// A vertex in the target: its place in pixels and the coordinates.
#[derive(Clone, Copy)]
struct Placed {
    clip: [f32; 4],
    at: [f32; 2],
    uv: [f32; 2],
}

/// One triangle to fill.
pub struct Tri {
    c: [i64; 3],
    dcdx: [i64; 3],
    dcdy: [i64; 3],
    u: Plane,
    v: Plane,
    /// The pixels its corners span: first column and row, then one past the last.
    pub span: [i64; 4],
}

impl Tri {
    /// Whether the centre of the pixel (x, y) is filled (lp_rast_tri_tmp.h).
    pub fn covers(&self, x: i64, y: i64) -> bool {
        (0..3).all(|i| self.c[i] + self.dcdy[i] * (y * 256) - self.dcdx[i] * (x * 256) > 0)
    }

    /// The coordinates at the centre of the pixel (x, y).
    pub fn uv(&self, x: usize, y: usize) -> (f32, f32) {
        (self.u.at(x, y), self.v.at(x, y))
    }

    /// Whether u is one number down a column.
    pub fn upright(&self) -> bool {
        self.u.dady == 0.0
    }
}

const PLANES: [[f32; 4]; 4] = [
    [-1.0, 0.0, 0.0, 1.0],
    [1.0, 0.0, 0.0, 1.0],
    [0.0, -1.0, 0.0, 1.0],
    [0.0, 1.0, 0.0, 1.0],
];

fn dist(p: [f32; 4], plane: [f32; 4]) -> f32 {
    p[0] * plane[0] + p[1] * plane[1] + p[2] * plane[2] + p[3] * plane[3]
}

/// Mesa's `!(d >= 0)`: a distance that is not a number is outside too.
fn outside(d: f32) -> bool {
    d < 0.0 || d.is_nan()
}

/// The clip mask of draw_cliptest_tmp.h for the x and y planes.
fn mask(p: [f32; 4]) -> u8 {
    let mut m = 0;
    if outside(-p[0] + p[3]) {
        m |= 1;
    }
    if outside(p[0] + p[3]) {
        m |= 2;
    }
    if outside(-p[1] + p[3]) {
        m |= 4;
    }
    if outside(p[1] + p[3]) {
        m |= 8;
    }
    m
}

/// draw_pipe_clip.c's `interp`: the new vertex `t` of the way from `out` to `inside`.
fn interp(t: f32, out: &Placed, inside: &Placed, view: &View) -> Placed {
    let l = |o: f32, i: f32| o + t * (i - o);
    let clip: [f32; 4] = std::array::from_fn(|k| l(out.clip[k], inside.clip[k]));
    let oow = 1.0 / clip[3];
    Placed {
        clip,
        at: [
            clip[0] * oow * view.scale[0] + view.trans[0],
            clip[1] * oow * view.scale[1] + view.trans[1],
        ],
        uv: [l(out.uv[0], inside.uv[0]), l(out.uv[1], inside.uv[1])],
    }
}

/// do_clip_tri and emit_poly: the triangles left of one cut at the planes of `bits`.
fn clip(tri: [Placed; 3], bits: u8, view: &View, out: &mut Vec<[Placed; 3]>) {
    let mut list = tri.to_vec();
    for (k, plane) in PLANES.iter().enumerate() {
        if bits & (1 << k) == 0 {
            continue;
        }
        if list.len() < 3 {
            return;
        }
        let n = list.len();
        let mut next = Vec::with_capacity(n + 1);
        let mut prev = list[0];
        let mut dp_prev = dist(prev.clip, *plane);
        for i in 1..=n {
            let vert = list[i % n];
            let dp = dist(vert.clip, *plane);
            let different = if dp_prev >= 0.0 {
                next.push(prev);
                dp < 0.0
            } else {
                dp >= 0.0 || dp.is_nan()
            };
            if different {
                let denom = dp - dp_prev;
                let new = if dp < 0.0 {
                    if -dp < dp_prev {
                        interp(dp / denom, &vert, &prev, view)
                    } else {
                        interp(-dp_prev / denom, &prev, &vert, view)
                    }
                } else if -dp_prev < dp {
                    interp(-dp_prev / denom, &prev, &vert, view)
                } else {
                    interp(dp / denom, &vert, &prev, view)
                };
                next.push(new);
            }
            prev = vert;
            dp_prev = dp;
        }
        list = next;
    }
    for i in 2..list.len() {
        out.push([list[i - 1], list[i], list[0]]);
    }
}

/// The triangles llvmpipe fills for `tris` through `view`.
pub fn setup(tris: &[[Vert; 3]], view: &View) -> Vec<Tri> {
    let place = |v: &Vert| {
        let oow = 1.0 / v.pos[3];
        // generate_viewport of draw_llvm.c: the product is not rounded before the sum.
        let at = |k: usize| (v.pos[k] * oow).mul_add(view.scale[k], view.trans[k]);
        Placed {
            clip: v.pos,
            at: [at(0), at(1)],
            uv: v.uv,
        }
    };
    let mut pieces = Vec::new();
    for t in tris {
        let masks = t.map(|v| mask(v.pos));
        let (any, all) = (
            masks[0] | masks[1] | masks[2],
            masks[0] & masks[1] & masks[2],
        );
        let t = [place(&t[0]), place(&t[1]), place(&t[2])];
        if any == 0 {
            pieces.push(t);
        } else if all == 0 {
            clip(t, any, view, &mut pieces);
        }
    }
    let fixed = |c: f32| ((c - 0.5) * 256.0).round_ties_even() as i64;
    let mut out = Vec::new();
    for mut t in pieces {
        let pos = |t: &[Placed; 3]| t.map(|v| [fixed(v.at[0]), fixed(v.at[1])]);
        let area = |p: [[i64; 2]; 3]| {
            (p[0][0] - p[1][0]) * (p[2][1] - p[0][1]) - (p[2][0] - p[0][0]) * (p[0][1] - p[1][1])
        };
        let a = area(pos(&t));
        if a == 0 {
            continue;
        }
        if a < 0 {
            t.swap(0, 1);
        }
        let p = pos(&t);
        let (mut c, mut dcdx, mut dcdy) = ([0; 3], [0; 3], [0; 3]);
        for i in 0..3 {
            let j = (i + 1) % 3;
            dcdy[i] = p[i][0] - p[j][0];
            dcdx[i] = p[i][1] - p[j][1];
            c[i] = dcdx[i] * p[i][0] - dcdy[i] * p[i][1];
            let left = dcdx[i] < 0;
            let flat = dcdx[i] == 0
                && if view.bottom_rule {
                    dcdy[i] < 0
                } else {
                    dcdy[i] > 0
                };
            if left || flat {
                c[i] += 1;
            }
        }
        let xy = t.map(|v| v.at);
        let lo = |k: usize| p.iter().map(|v| v[k]).min().unwrap_or(0) >> 8;
        let hi = |k: usize| (p.iter().map(|v| v[k]).max().unwrap_or(0) >> 8) + 1;
        out.push(Tri {
            c,
            dcdx,
            dcdy,
            u: Plane::new(xy, t.map(|v| v.uv[0])),
            v: Plane::new(xy, t.map(|v| v.uv[1])),
            span: [lo(0), lo(1), hi(0) + 1, hi(1) + 1],
        });
    }
    out
}

/// QMatrix4x4::ortho of the rect from (`left`, `bottom`) to (`right`, `top`), column major:
/// QSGRenderer::setProjectionMatrixToRect of a layer's mirrored rect, whose `bottom` is the
/// rect's top.
pub fn ortho(left: f32, right: f32, bottom: f32, top: f32) -> [f32; 16] {
    let (width, invheight) = (right - left, top - bottom);
    let mut m = [0.0; 16];
    m[0] = 2.0 / width;
    m[5] = 2.0 / invheight;
    m[10] = 1.0;
    m[12] = -(left + right) / width;
    m[13] = -(top + bottom) / invheight;
    m[15] = 1.0;
    m
}

/// A ShaderEffect's one cell over `rect` (x y w h) with the whole texture, as
/// QQuickGridMesh's strip gives its two triangles, through the matrix `m` (column major)
/// of `gl_Position = m * p`.
pub fn grid(m: &[f32], [x, y, w, h]: [f32; 4]) -> [[Vert; 3]; 2] {
    let v = |x: f32, y: f32, u: f32, v: f32| {
        let row = |r: usize| ((m[r] * x + m[4 + r] * y) + m[8 + r] * 0.0) + m[12 + r];
        Vert {
            pos: [row(0), row(1), row(2), row(3)],
            uv: [u, v],
        }
    };
    let (tl, bl) = (v(x, y, 0.0, 0.0), v(x, y + h, 0.0, 1.0));
    let (tr, br) = (v(x + w, y, 1.0, 0.0), v(x + w, y + h, 1.0, 1.0));
    [[tl, bl, br], [tl, br, tr]]
}
