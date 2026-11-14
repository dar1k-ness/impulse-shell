//! Port of Qt5Compat RadialGradient: the texture of a 16x256 Rectangle gradient sampled by
//! `2 * distance(tc1, center * ratio)` (radialgradient.vert / radialgradient_nomask.frag).
use crate::layer;
use crate::raster::Canvas;
use crate::rect::{self, RectParams};

/// The texture RadialGradient.qml reads its colours from: a 16 x 256 Rectangle with the
/// Gradient's `stops` (plain white without any) in its orientation, in a ShaderEffectSource
/// of no `textureSize`: its size times the window's devicePixelRatio `dpr`, the Rectangle
/// drawn through that scale (20 x 320 at 1.25, 32 x 512 at 2).
pub fn gradient_texture(stops: &[(f32, [u8; 4])], vertical: bool, dpr: f64) -> Canvas {
    let mut tex = layer::texture(16.0 * dpr, 256.0 * dpr);
    let g = rect::build(&RectParams {
        width: 16.0,
        height: 256.0,
        stops: stops.to_vec(),
        vertical,
        ..Default::default()
    });
    let all = [0, 0, tex.w as i64, tex.h as i64];
    tex.draw_smooth_scaled(&g, [dpr as f32; 2], 0.0, 0.0, 1.0, all);
    tex
}

/// What RadialGradient.qml gives its ShaderEffect for an item of `w` x `h`: `center`,
/// `matrixData` (the sine and cosine of `-angle`) and the horizontal and vertical ratio.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Radial {
    pub center: [f32; 2],
    pub sin: f32,
    pub cos: f32,
    pub ratio: [f32; 2],
}

impl Radial {
    pub fn new(
        w: f64,
        h: f64,
        horizontal_offset: f64,
        vertical_offset: f64,
        horizontal_radius: f64,
        vertical_radius: f64,
        angle: f64,
    ) -> Radial {
        let ratio = |size: f64, radius: f64| match radius > 0.0 {
            true => size / (2.0 * radius),
            false => size * 16384.0,
        };
        let a = -angle / 360.0 * 2.0 * std::f64::consts::PI;
        Radial {
            center: [
                (0.5 + horizontal_offset / w) as f32,
                (0.5 + vertical_offset / h) as f32,
            ],
            sin: a.sin() as f32,
            cos: a.cos() as f32,
            ratio: [
                ratio(w, horizontal_radius) as f32,
                ratio(h, vertical_radius) as f32,
            ],
        }
    }

    /// Where down the gradient texture the item's point (u, v) reads: radialgradient.vert's
    /// `qt_TexCoord1` and `centerPoint`, and the fragment shaders' `2.0 * distance` of them.
    pub fn at(&self, u: f32, v: f32) -> f32 {
        let [cx, cy] = self.center;
        let [hr, vr] = self.ratio;
        let (dx, dy) = (u - cx, v - cy);
        let tx = (dx * self.cos - dy * self.sin + cx) * hr;
        let ty = (dx * self.sin + dy * self.cos + cy) * vr;
        2.0 * ((tx - cx * hr).powi(2) + (ty - cy * vr).powi(2)).sqrt()
    }
}

/// Qt5Compat RadialGradient over the scene rect (x, y, w, h): the gradient texture `tex`
/// (`gradient_texture`) read at `r.at`, times the alpha of `mask` (the `source` item's
/// texture, read linearly over the item) when there is one, times `opacity`
/// (radialgradient_nomask.frag, radialgradient_mask.frag).
#[allow(clippy::too_many_arguments)]
pub fn radial(
    cv: &mut Canvas,
    tex: &Canvas,
    r: &Radial,
    mask: Option<&Canvas>,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    opacity: f32,
) {
    layer::cover(cv, x, y, w, h, |u, v| {
        let k = mask.map_or(1.0, |m| layer::linear(m, u, v)[3]) * opacity;
        if k == 0.0 {
            return [0.0; 4];
        }
        layer::linear(tex, 0.0, r.at(u as f32, v as f32) as f64).map(|c| c * k)
    });
}
