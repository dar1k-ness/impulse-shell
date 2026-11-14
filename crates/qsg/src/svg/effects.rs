//! The buffers QtSvg draws a node into before a filter, a mask or group opacity is applied
//! (QSvgNode::draw), and the filter primitives of qsvgfilter.cpp, as Qt 6.12 has them. A buffer
//! is the QImage of that code: premultiplied pixels at a device offset (QImage::offset).

use std::collections::HashMap;
use std::f64::consts::{FRAC_1_SQRT_2, FRAC_2_SQRT_PI};
use std::rc::Rc;

use super::gradient::{q_round, source_over};
use super::{Elem, M, num_prefix, nums, parse_color};
use crate::canvas::{Context, El, Path};
use crate::raster::Canvas;
use mul255::qt as byte_mul;

/// QRectF as x, y, width, height.
pub type RectF = [f64; 4];

/// QRect.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct IRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl IRect {
    pub fn is_empty(&self) -> bool {
        self.w <= 0 || self.h <= 0
    }

    pub fn to_f(self) -> RectF {
        [self.x as f64, self.y as f64, self.w as f64, self.h as f64]
    }
}

/// QRectF::toRect.
pub fn to_rect(r: RectF) -> IRect {
    let (x, y) = (q_round(r[0]), q_round(r[1]));
    IRect {
        x,
        y,
        w: q_round(r[2] + (r[0] - x as f64) / 2.0),
        h: q_round(r[3] + (r[1] - y as f64) / 2.0),
    }
}

/// QRectF::toAlignedRect.
pub fn to_aligned_rect(r: RectF) -> IRect {
    let (x, y) = (r[0].floor() as i32, r[1].floor() as i32);
    IRect {
        x,
        y,
        w: (r[0] + r[2]).ceil() as i32 - x,
        h: (r[1] + r[3]).ceil() as i32 - y,
    }
}

/// QTransform::mapRect.
pub fn map_rect(m: &M, r: RectF) -> RectF {
    if m[1] == 0.0 && m[2] == 0.0 {
        let (mut x, mut y) = (m[0] * r[0] + m[4], m[3] * r[1] + m[5]);
        let (mut w, mut h) = (m[0] * r[2], m[3] * r[3]);
        if w < 0.0 {
            w = -w;
            x -= w;
        }
        if h < 0.0 {
            h = -h;
            y -= h;
        }
        return [x, y, w, h];
    }
    let p = |x: f64, y: f64| [m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]];
    let c = [
        p(r[0], r[1]),
        p(r[0] + r[2], r[1]),
        p(r[0] + r[2], r[1] + r[3]),
        p(r[0], r[1] + r[3]),
    ];
    let lo = |k: usize| c.iter().map(|q| q[k]).fold(f64::INFINITY, f64::min);
    let hi = |k: usize| c.iter().map(|q| q[k]).fold(f64::NEG_INFINITY, f64::max);
    [lo(0), lo(1), hi(0) - lo(0), hi(1) - lo(1)]
}

fn edges(r: RectF) -> [f64; 4] {
    let (mut l, mut rt, mut t, mut b) = (r[0], r[0], r[1], r[1]);
    if r[2] < 0.0 {
        l += r[2];
    } else {
        rt += r[2];
    }
    if r[3] < 0.0 {
        t += r[3];
    } else {
        b += r[3];
    }
    [l, rt, t, b]
}

/// QRectF::operator|: a null rectangle (no width and no height) adds nothing.
pub fn united(a: RectF, b: RectF) -> RectF {
    let null = |r: RectF| r[2] == 0.0 && r[3] == 0.0;
    if null(a) {
        return b;
    }
    if null(b) {
        return a;
    }
    let ([l1, r1, t1, b1], [l2, r2, t2, b2]) = (edges(a), edges(b));
    let (l, t) = (l1.min(l2), t1.min(t2));
    [l, t, r1.max(r2) - l, b1.max(b2) - t]
}

/// QRectF::operator&.
fn intersected(a: RectF, b: RectF) -> RectF {
    let ([l1, r1, t1, b1], [l2, r2, t2, b2]) = (edges(a), edges(b));
    if l1 == r1 || l2 == r2 || l1 >= r2 || l2 >= r1 {
        return [0.0; 4];
    }
    if t1 == b1 || t2 == b2 || t1 >= b2 || t2 >= b1 {
        return [0.0; 4];
    }
    let (x, y) = (l1.max(l2), t1.max(t2));
    [x, y, r1.min(r2) - x, b1.min(b2) - y]
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Units {
    Unknown,
    ObjectBoundingBox,
    UserSpaceOnUse,
}
use Units::{ObjectBoundingBox, Unknown, UserSpaceOnUse};

/// QSvgRectF: a rectangle whose four lengths each carry their unit.
#[derive(Clone, Copy)]
pub struct SvgRect {
    pub r: RectF,
    pub units: [Units; 4],
}

impl SvgRect {
    /// QSvgRectF::resolveRelativeLengths; `units` is `Unknown` for the overload without it.
    pub fn resolve(&self, local: RectF, units: Units) -> RectF {
        let obb = |i: usize| units == ObjectBoundingBox || self.units[i] == ObjectBoundingBox;
        let pos = |i: usize| {
            if obb(i) {
                local[i] + self.r[i] * local[i + 2]
            } else {
                self.r[i]
            }
        };
        let len = |i: usize| {
            if obb(i) {
                local[i] * self.r[i]
            } else {
                self.r[i]
            }
        };
        [pos(0), pos(1), len(2), len(3)]
    }
}

enum Len {
    Number(f64),
    Percent(f64),
    Pt(f64),
}

/// QGuiSvg::parseLength with convertToPixels for every unit but `pt` and `%`.
fn length(v: &str) -> Option<Len> {
    use svgtypes::LengthUnit as U;
    let l: svgtypes::Length = v.trim().parse().ok()?;
    Some(match l.unit {
        U::Percent => Len::Percent(l.number),
        U::Pt => Len::Pt(l.number),
        U::Mm => Len::Number(l.number * 3.543307),
        U::Cm => Len::Number(l.number * 35.43307),
        U::In => Len::Number(l.number * 90.0),
        _ => Len::Number(l.number),
    })
}

/// parseFilterBounds. A percentage of `y`, `width` or `height` marks `x` as relative to the
/// bounding box and leaves its own length in user units: that is what the Qt code does.
fn filter_bounds(e: &Elem, rect: &mut SvgRect) {
    for (i, k) in ["x", "y", "width", "height"].into_iter().enumerate() {
        let Some(v) = e.attr(k).filter(|v| !v.is_empty()) else {
            continue;
        };
        match length(v) {
            Some(Len::Pt(n)) => rect.r[i] = n,
            Some(Len::Percent(n)) => {
                rect.units[i] = UserSpaceOnUse;
                rect.units[0] = ObjectBoundingBox;
                rect.r[i] = n / 100.0;
            }
            Some(Len::Number(n)) => {
                rect.units[i] = UserSpaceOnUse;
                rect.r[i] = n;
            }
            None => {
                rect.units[i] = UserSpaceOnUse;
                rect.r[i] = 0.0;
            }
        }
    }
}

/// QSvgMask as createMaskNode builds it; `None` where Qt makes no node (empty rectangle).
pub struct Mask {
    pub rect: SvgRect,
    pub content_obb: bool,
}

impl Mask {
    pub fn parse(e: &Elem, view_box: RectF) -> Option<Mask> {
        let has = |k: &str, v: &str| e.attr(k).is_some_and(|a| a.contains(v));
        let mu = if has("maskUnits", "userSpaceOnUse") {
            UserSpaceOnUse
        } else {
            ObjectBoundingBox
        };
        let mut rect = SvgRect {
            r: [-0.1, -0.1, 1.2, 1.2],
            units: [ObjectBoundingBox; 4],
        };
        for (i, k) in ["x", "y", "width", "height"].into_iter().enumerate() {
            let len = e.attr(k).filter(|v| !v.is_empty()).and_then(length);
            let Some(len) = len else {
                continue;
            };
            rect.units[i] = mu;
            rect.r[i] = match len {
                Len::Percent(n) if mu == UserSpaceOnUse => n / 100.0 * view_box[2 + i % 2],
                Len::Percent(n) => n / 100.0,
                Len::Pt(n) => n * 1.25,
                Len::Number(n) => n,
            };
        }
        (rect.r[2] > 0.0 && rect.r[3] > 0.0).then_some(Mask {
            rect,
            content_obb: has("maskContentUnits", "objectBoundingBox"),
        })
    }
}

/// qUnpremultiply.
fn unpremultiply(p: [u8; 4]) -> [u32; 4] {
    let a = p[3] as u32;
    match a {
        255 => p.map(u32::from),
        0 => [0; 4],
        _ => {
            let inv = 0x00ff_00ff / a;
            let c = |v: u8| (v as u32 * inv + 0x8000) >> 16;
            [c(p[0]), c(p[1]), c(p[2]), a]
        }
    }
}

/// qPremultiply.
fn premultiply(c: [u32; 4]) -> [u8; 4] {
    let a = c[3];
    [
        byte_mul(c[0], a) as u8,
        byte_mul(c[1], a) as u8,
        byte_mul(c[2], a) as u8,
        a as u8,
    ]
}

/// Coverage of what lies outside `local` mapped by `m`, in a `w`×`h` image at device offset
/// (`x`, `y`): the odd-even path of the grown image rectangle and the mapped rectangle that
/// QSvgMask::createMask and clipToTransformedBounds fill.
fn outside_coverage(rect: IRect, m: &M, local: RectF) -> Vec<u8> {
    let (w, h) = (rect.w as f64, rect.h as f64);
    let p = |x: f64, y: f64| {
        [
            m[0] * x + m[2] * y + m[4] - rect.x as f64,
            m[1] * x + m[3] * y + m[5] - rect.y as f64,
        ]
    };
    let (x0, y0, x1, y1) = (local[0], local[1], local[0] + local[2], local[1] + local[3]);
    let path = Path::from_el(vec![
        El::Move([-10.0, -10.0]),
        El::Line([w + 20.0, -10.0]),
        El::Line([w + 20.0, h + 20.0]),
        El::Line([-10.0, h + 20.0]),
        El::Line([-10.0, -10.0]),
        El::Move(p(x0, y0)),
        El::Line(p(x1, y0)),
        El::Line(p(x1, y1)),
        El::Line(p(x0, y1)),
        El::Line(p(x0, y0)),
    ]);
    let mut c = Context::new(rect.w as usize, rect.h as usize);
    c.fill_opacity(&path, [255; 4], true, 256, None);
    c.img.px.iter().map(|p| p[3]).collect()
}

/// The mask image QSvgMask::createMask returns, from its drawn content: alpha is what the mask
/// takes away. The image is Format_RGBA8888, which qRed and qBlue read the other way round, so
/// red weighs 0.0721 and blue 0.2125; outside the mask rectangle everything is taken away.
pub fn mask_alpha(content: &[[u8; 4]], rect: IRect, device: &M, clip: RectF) -> Vec<u8> {
    let outside = outside_coverage(rect, device, clip);
    content
        .iter()
        .zip(outside)
        .map(|(p, cov)| {
            let [r, g, b, a] = unpremultiply(*p).map(f64::from);
            let alpha = (255.0 - (b * 0.2125 + g * 0.7154 + r * 0.0721) * a / 255.0) as i32 as u32;
            let cov = cov as u32;
            (cov + byte_mul(alpha & 0xff, 255 - cov)) as u8
        })
        .collect()
}

#[derive(Clone)]
pub struct Buf {
    pub rect: IRect,
    pub px: Vec<[u8; 4]>,
}

type Op = fn(&mut [u8; 4], [u8; 4]);

fn over(d: &mut [u8; 4], s: [u8; 4]) {
    source_over(d, s, 255);
}

fn destination_over(d: &mut [u8; 4], s: [u8; 4]) {
    let ia = 255 - d[3] as u32;
    for k in 0..4 {
        d[k] = (d[k] as u32 + byte_mul(s[k] as u32, ia)) as u8;
    }
}

fn destination_in(d: &mut [u8; 4], s: [u8; 4]) {
    *d = d.map(|v| byte_mul(v as u32, s[3] as u32) as u8);
}

fn destination_out(d: &mut [u8; 4], s: [u8; 4]) {
    *d = d.map(|v| byte_mul(v as u32, 255 - s[3] as u32) as u8);
}

/// INTERPOLATE_PIXEL_255 for one channel.
fn interpolate(x: u8, a: u32, y: u8, b: u32) -> u8 {
    mul255::qt_div_255((x as u32 * a + y as u32 * b) as i64) as u8
}

fn destination_atop(d: &mut [u8; 4], s: [u8; 4]) {
    let (sa, ida) = (s[3] as u32, 255 - d[3] as u32);
    for k in 0..4 {
        d[k] = interpolate(d[k], sa, s[k], ida);
    }
}

fn xor(d: &mut [u8; 4], s: [u8; 4]) {
    let (isa, ida) = (255 - s[3] as u32, 255 - d[3] as u32);
    for k in 0..4 {
        d[k] = interpolate(s[k], ida, d[k], isa);
    }
}

fn div_255(x: u32) -> u32 {
    mul255::qt_div_255(x as i64) as u32
}

/// comp_func_Lighten.
fn lighten(d: &mut [u8; 4], s: [u8; 4]) {
    let (da, sa) = (d[3] as u32, s[3] as u32);
    for k in 0..3 {
        let (dc, sc) = (d[k] as u32, s[k] as u32);
        d[k] = div_255((sc * da).max(dc * sa) + sc * (255 - da) + dc * (255 - sa)) as u8;
    }
    d[3] = (255 - div_255((255 - sa) * (255 - da))) as u8;
}

impl Buf {
    /// A transparent image; `None` where QImageIOHandler::allocateImage refuses the size.
    pub fn new(rect: IRect) -> Option<Buf> {
        if rect.is_empty() || crate::image::check_size(rect.w as usize, rect.h as usize).is_err() {
            return None;
        }
        Some(Buf {
            rect,
            px: vec![[0; 4]; rect.w as usize * rect.h as usize],
        })
    }

    /// The pixel at a device position, if the image covers it.
    fn get(&self, x: i32, y: i32) -> Option<[u8; 4]> {
        let (lx, ly) = (x - self.rect.x, y - self.rect.y);
        (lx >= 0 && ly >= 0 && lx < self.rect.w && ly < self.rect.h)
            .then(|| self.px[(ly * self.rect.w + lx) as usize])
    }

    /// QImage::copy of a device rectangle.
    fn copy(&self, rect: IRect) -> Option<Buf> {
        let mut out = Buf::new(rect)?;
        out.draw(self, self.rect.x, self.rect.y, |d, s| *d = s);
        Some(out)
    }

    /// QPainter::drawImage of `src` with its top left at device position (`x`, `y`).
    fn draw(&mut self, src: &Buf, x: i32, y: i32, op: Op) {
        let r = self.rect;
        for sy in 0..src.rect.h {
            let ly = y + sy - r.y;
            if ly < 0 || ly >= r.h {
                continue;
            }
            for sx in 0..src.rect.w {
                let lx = x + sx - r.x;
                if lx >= 0 && lx < r.w {
                    op(
                        &mut self.px[(ly * r.w + lx) as usize],
                        src.px[(sy * src.rect.w + sx) as usize],
                    );
                }
            }
        }
    }

    /// QSvgNode::applyMaskToBuffer: the mask image over the buffer in DestinationOut.
    pub fn mask_out(&mut self, alpha: &[u8]) {
        for (p, a) in self.px.iter_mut().zip(alpha) {
            *p = p.map(|v| byte_mul(v as u32, 255 - *a as u32) as u8);
        }
    }

    /// QSvgFeFilterPrimitive::clipToTransformedBounds.
    fn clip(&mut self, m: &M, local: RectF) {
        let outside = outside_coverage(self.rect, m, local);
        self.mask_out(&outside);
    }
}

#[derive(Clone, Copy)]
enum Operator {
    Over,
    In,
    Out,
    Atop,
    Xor,
    Lighter,
    Arithmetic,
}

#[derive(Clone, Copy)]
enum Mode {
    Normal,
    Multiply,
    Screen,
    Darken,
    Lighten,
}

enum Kind {
    /// Rows for red, green, blue and alpha over the columns red, green, blue, alpha and 255.
    ColorMatrix([[f64; 5]; 4]),
    Blur(f64, f64),
    Offset(f64, f64),
    Merge(Vec<String>),
    Composite(String, Operator, [f64; 4]),
    /// Premultiplied.
    Flood([u8; 4]),
    Blend(String, Mode),
}

struct Primitive {
    input: String,
    result: String,
    rect: SvgRect,
    kind: Kind,
}

/// QSvgFilterContainer with its primitives.
pub struct Filter {
    rect: SvgRect,
    filter_units: Units,
    primitive_units: Units,
    prims: Vec<Primitive>,
}

/// The primitives QSvgHandler makes a QSvgFeUnsupported node of.
const UNSUPPORTED: [&str; 14] = [
    "feComponentTransfer",
    "feConvolveMatrix",
    "feDiffuseLighting",
    "feDisplacementMap",
    "feDropShadow",
    "feFuncA",
    "feFuncB",
    "feFuncG",
    "feFuncR",
    "feImage",
    "feMorphology",
    "feSpecularLighting",
    "feTile",
    "feTurbulence",
];

/// QSvgFeColorMatrix: the matrix its constructor leaves for each `type`.
fn color_matrix(e: &Elem) -> [[f64; 5]; 4] {
    const M1: [f64; 9] = [
        0.213, 0.213, 0.213, 0.715, 0.715, 0.715, 0.072, 0.072, 0.072,
    ];
    const M2: [f64; 9] = [
        0.787, -0.213, -0.213, -0.715, 0.285, -0.715, -0.072, -0.072, 0.928,
    ];
    const M3: [f64; 9] = [
        -0.213, 0.143, -0.787, -0.715, 0.140, 0.715, 0.928, -0.283, 0.072,
    ];
    let mut m = [[0.0; 5]; 4];
    let mut first = 1.0;
    match e.attr("values").filter(|v| !v.is_empty()) {
        Some(v) => {
            let list = nums(v);
            first = list.first().copied().unwrap_or(0.0);
            for (i, n) in list.into_iter().take(20).enumerate() {
                m[i / 5][i % 5] = n;
            }
        }
        None => {
            for (i, row) in m.iter_mut().enumerate() {
                row[i] = 1.0;
            }
        }
    }
    let ty = e.attr("type").unwrap_or("");
    let mix = |c: f64, s: f64| {
        let mut m = [[0.0; 5]; 4];
        for (r, row) in m.iter_mut().take(3).enumerate() {
            for (col, v) in row.iter_mut().take(3).enumerate() {
                let i = col * 3 + r;
                *v = M1[i] + M2[i] * c + M3[i] * s;
            }
        }
        m[3][3] = 1.0;
        m
    };
    if ty.starts_with("saturate") {
        mix(first.clamp(0.0, 1.0), 0.0)
    } else if ty.starts_with("hueRotate") {
        let angle = first / 180.0 * std::f64::consts::PI;
        mix(angle.cos(), angle.sin())
    } else if ty.starts_with("luminanceToAlpha") {
        let mut m = [[0.0; 5]; 4];
        m[3][..3].copy_from_slice(&[0.2125, 0.7154, 0.0721]);
        m
    } else {
        m
    }
}

/// QColor::rgba of a colour whose alpha was set with setAlphaF, premultiplied.
fn flood_color(e: &Elem) -> [u8; 4] {
    let opacity = e.attr("flood-opacity").filter(|v| !v.is_empty());
    let rgb = match e.attr("flood-color").map(parse_color) {
        Some(Some(Some(c))) => c,
        Some(Some(None)) => [0; 4],
        _ => [0, 0, 0, 255],
    };
    let mut a = rgb[3] as u32 * 257;
    if let Some(op) = opacity {
        a = q_round(num_prefix(op).clamp(0.0, 1.0) * (a as f64 / 65535.0) * 65535.0) as u32;
    }
    let a8 = (a - (a >> 8) + 0x80) >> 8;
    premultiply([rgb[0] as u32, rgb[1] as u32, rgb[2] as u32, a8])
}

/// The length attributes of feOffset: `pt` is left unconverted.
fn offset_length(e: &Elem, k: &str) -> f64 {
    match e.attr(k).filter(|v| !v.is_empty()).and_then(length) {
        Some(Len::Number(n) | Len::Percent(n) | Len::Pt(n)) => n,
        None => 0.0,
    }
}

struct Run<'a> {
    m: &'a M,
    item: RectF,
    region: RectF,
    units: Units,
}

impl Filter {
    /// `None` where QtSvg has no supported filter: a child it knows but does not apply.
    pub fn parse(e: &Elem, view_box: RectF) -> Option<Filter> {
        let has = |k: &str, v: &str| e.attr(k).is_some_and(|a| a.contains(v));
        let filter_units = if has("filterUnits", "userSpaceOnUse") {
            UserSpaceOnUse
        } else {
            ObjectBoundingBox
        };
        let primitive_units = if has("primitiveUnits", "objectBoundingBox") {
            ObjectBoundingBox
        } else {
            UserSpaceOnUse
        };
        let mut rect = if filter_units == UserSpaceOnUse {
            let (w, h) = (view_box[2], view_box[3]);
            SvgRect {
                r: [-0.1 * w, -0.1 * h, 1.2 * w, 1.2 * h],
                units: [UserSpaceOnUse; 4],
            }
        } else {
            SvgRect {
                r: [-0.1, -0.1, 1.2, 1.2],
                units: [ObjectBoundingBox; 4],
            }
        };
        filter_bounds(e, &mut rect);
        let mut prims = Vec::new();
        for c in &e.children {
            let s = |k: &str| c.attr(k).unwrap_or("").to_string();
            let kind = match c.name.as_str() {
                "feColorMatrix" => Kind::ColorMatrix(color_matrix(c)),
                "feGaussianBlur" => {
                    let v = c.attr("stdDeviation").unwrap_or("");
                    let n = |t: Option<&str>| num_prefix(t.unwrap_or("")).max(0.0);
                    if v.contains(' ') {
                        Kind::Blur(n(v.split(' ').next()), n(v.split(' ').next_back()))
                    } else {
                        Kind::Blur(n(Some(v)), n(Some(v)))
                    }
                }
                "feOffset" => Kind::Offset(offset_length(c, "dx"), offset_length(c, "dy")),
                "feMerge" => Kind::Merge(
                    c.children
                        .iter()
                        .filter(|n| n.name == "feMergeNode")
                        .map(|n| n.attr("in").unwrap_or("").to_string())
                        .collect(),
                ),
                "feComposite" => {
                    let o = c.attr("operator").unwrap_or("");
                    let op = [
                        ("in", Operator::In),
                        ("out", Operator::Out),
                        ("atop", Operator::Atop),
                        ("xor", Operator::Xor),
                        ("lighter", Operator::Lighter),
                        ("arithmetic", Operator::Arithmetic),
                    ]
                    .into_iter()
                    .find(|(name, _)| o.starts_with(name))
                    .map_or(Operator::Over, |(_, op)| op);
                    // QVector4D holds floats.
                    let k = ["k1", "k2", "k3", "k4"].map(|k| {
                        c.attr(k)
                            .and_then(|v| v.trim().parse::<f64>().ok())
                            .unwrap_or(0.0) as f32 as f64
                    });
                    Kind::Composite(s("in2"), op, k)
                }
                "feFlood" => Kind::Flood(flood_color(c)),
                "feBlend" => {
                    let v = c.attr("mode").unwrap_or("");
                    let mode = [
                        ("multiply", Mode::Multiply),
                        ("screen", Mode::Screen),
                        ("darken", Mode::Darken),
                        ("lighten", Mode::Lighten),
                    ]
                    .into_iter()
                    .find(|(name, _)| v.starts_with(name))
                    .map_or(Mode::Normal, |(_, m)| m);
                    Kind::Blend(s("in2"), mode)
                }
                // A node of the filter that is no primitive QtSvg applies.
                "feMergeNode" => return None,
                n if UNSUPPORTED.contains(&n) => return None,
                _ => continue,
            };
            let mut rect = SvgRect {
                r: [0.0, 0.0, 1.0, 1.0],
                units: [Unknown; 4],
            };
            filter_bounds(c, &mut rect);
            prims.push(Primitive {
                input: s("in"),
                result: s("result"),
                rect,
                kind,
            });
        }
        Some(Filter {
            rect,
            filter_units,
            primitive_units,
            prims,
        })
    }

    /// QSvgFilterContainer::filterRegion.
    pub fn region(&self, item: RectF) -> RectF {
        self.rect.resolve(item, self.filter_units)
    }

    /// QSvgFilterContainer::applyFilter: `buffer` holds the node drawn under `m`, `item` is its
    /// bounding box in user space. `None` where Qt ends up with a null image.
    pub fn apply(&self, buffer: Option<Buf>, m: &M, item: RectF) -> Option<Rc<Buf>> {
        let buffer = buffer?;
        let region = self.region(item);
        let global = to_rect(map_rect(m, region));
        if global.is_empty() {
            return Some(Rc::new(buffer));
        }
        let Some(proxy) = buffer.copy(global) else {
            return Some(Rc::new(buffer));
        };
        let proxy = Rc::new(proxy);
        let mut sources: HashMap<String, Rc<Buf>> = HashMap::new();
        sources.insert(String::new(), proxy.clone());
        sources.insert("SourceGraphic".to_string(), proxy.clone());
        let wants_alpha = self.prims.iter().any(|p| {
            p.input == "SourceAlpha"
                || match &p.kind {
                    Kind::Composite(in2, ..) | Kind::Blend(in2, _) => in2 == "SourceAlpha",
                    Kind::Merge(inputs) => inputs.iter().any(|i| i == "SourceAlpha"),
                    _ => false,
                }
        });
        if wants_alpha {
            let mut alpha = (*proxy).clone();
            for p in &mut alpha.px {
                *p = [0, 0, 0, p[3]];
            }
            sources.insert("SourceAlpha".to_string(), Rc::new(alpha));
        }
        let run = Run {
            m,
            item,
            region,
            units: self.primitive_units,
        };
        let mut result = None;
        for p in &self.prims {
            result = p.apply(&sources, &run);
            if let Some(r) = &result {
                sources.insert(String::new(), r.clone());
                sources.insert(p.result.clone(), r.clone());
            }
        }
        result
    }
}

impl Primitive {
    /// QSvgFeFilterPrimitive::localSubRegion.
    fn sub_region(&self, run: &Run) -> RectF {
        let mut c = self.rect.resolve(run.item, run.units);
        // QRectF::setX and setY keep the right and bottom edges.
        let unknown = self.rect.units.map(|u| u == Unknown);
        if unknown[0] {
            c[2] -= run.region[0] - c[0];
            c[0] = run.region[0];
        }
        if unknown[1] {
            c[3] -= run.region[1] - c[1];
            c[1] = run.region[1];
        }
        if unknown[2] {
            c[2] = run.region[2];
        }
        if unknown[3] {
            c[3] = run.region[3];
        }
        intersected(c, run.region)
    }

    fn apply(&self, sources: &HashMap<String, Rc<Buf>>, run: &Run) -> Option<Rc<Buf>> {
        let sub = self.sub_region(run);
        let global = to_rect(map_rect(run.m, sub));
        let mut out = match &self.kind {
            Kind::ColorMatrix(matrix) => {
                let src = sources.get(&self.input)?;
                let mut out = Buf::new(global)?;
                for (i, d) in out.px.iter_mut().enumerate() {
                    let (x, y) = (
                        i as i32 % global.w + global.x,
                        i as i32 / global.w + global.y,
                    );
                    let Some(s) = src.get(x, y) else {
                        continue;
                    };
                    let [r, g, b, a] = unpremultiply(s).map(f64::from);
                    let v = [r, g, b, a, 255.0];
                    let row = |k: usize| {
                        let mut sum = 0.0;
                        for (j, v) in v.iter().enumerate() {
                            sum += matrix[k][j] * v;
                        }
                        (sum as i32).clamp(0, 255) as u32
                    };
                    *d = premultiply([row(0), row(1), row(2), row(3)]);
                }
                out
            }
            Kind::Blur(sx, sy) => {
                let src = sources.get(&self.input)?;
                if *sx == 0.0 && *sy == 0.0 {
                    return Some(src.clone());
                }
                blur(src, *sx, *sy, run, sub)?
            }
            Kind::Offset(dx, dy) => {
                let src = sources.get(&self.input)?;
                let (mut ox, mut oy) = (*dx as i32, *dy as i32);
                if run.units == ObjectBoundingBox {
                    ox = (dx * run.item[2]) as i32;
                    oy = (dy * run.item[3]) as i32;
                }
                // QTransform::map(QPoint).
                let m = run.m;
                let map = |x: f64, y: f64| {
                    (
                        q_round(m[0] * x + m[2] * y + m[4]),
                        q_round(m[1] * x + m[3] * y + m[5]),
                    )
                };
                let (a, b) = (map(ox as f64, oy as f64), map(0.0, 0.0));
                let mut out = Buf::new(global)?;
                out.draw(src, src.rect.x + a.0 - b.0, src.rect.y + a.1 - b.1, over);
                out
            }
            Kind::Merge(inputs) => {
                let mut out = Buf::new(global)?;
                for i in inputs.iter().filter_map(|i| sources.get(i)) {
                    out.draw(i, i.rect.x, i.rect.y, over);
                }
                out
            }
            Kind::Composite(in2, op, k) => {
                let (s1, s2) = (sources.get(&self.input)?, sources.get(in2)?);
                let mut out = Buf::new(global)?;
                let mode: Op = match op {
                    Operator::Arithmetic => {
                        for (i, d) in out.px.iter_mut().enumerate() {
                            let (x, y) = (
                                i as i32 % global.w + global.x,
                                i as i32 / global.w + global.y,
                            );
                            let a = s1.get(x, y).unwrap_or([0; 4]).map(f64::from);
                            let b = s2.get(x, y).unwrap_or([0; 4]).map(f64::from);
                            let v = |c: usize| {
                                (k[0] * a[c] * b[c] / 255.0
                                    + k[1] * a[c]
                                    + k[2] * b[c]
                                    + k[3] * 255.0) as i32
                            };
                            let alpha = v(3).clamp(0, 255);
                            let c = |c: usize| v(c).clamp(0, alpha) as u8;
                            *d = [c(0), c(1), c(2), alpha as u8];
                        }
                        return Some(Rc::new(clipped(out, run.m, sub)));
                    }
                    Operator::In => destination_in,
                    Operator::Out => destination_out,
                    Operator::Xor => xor,
                    Operator::Lighter => lighten,
                    Operator::Atop => destination_atop,
                    Operator::Over => destination_over,
                };
                out.draw(s1, s1.rect.x, s1.rect.y, over);
                out.draw(s2, s2.rect.x, s2.rect.y, mode);
                out
            }
            Kind::Flood(color) => {
                let mut out = Buf::new(global)?;
                out.px.fill(*color);
                out
            }
            Kind::Blend(in2, mode) => {
                let (s1, s2) = (sources.get(&self.input)?, sources.get(in2)?);
                let mut out = Buf::new(global)?;
                for (i, d) in out.px.iter_mut().enumerate() {
                    let (x, y) = (
                        i as i32 % global.w + global.x,
                        i as i32 / global.w + global.y,
                    );
                    let p1 = s1.get(x, y).unwrap_or([0; 4]).map(f64::from);
                    let p2 = s2.get(x, y).unwrap_or([0; 4]).map(f64::from);
                    let (a1, a2) = (p1[3] / 255.0, p2[3] / 255.0);
                    let a = (255.0 - (1.0 - a1) * (1.0 - a2) * 255.0).clamp(0.0, 255.0);
                    let c = |k: usize| {
                        let (c1, c2) = (p1[k], p2[k]);
                        let v = match mode {
                            Mode::Normal => (1.0 - a1) * c2 + c1,
                            Mode::Multiply => (1.0 - a1) * c2 + (1.0 - a2) * c1 + c1 * c2 / 255.0,
                            Mode::Screen => c2 + c1 - c1 * c2 / 255.0,
                            Mode::Darken => ((1.0 - a1) * c2 + c1).min((1.0 - a2) * c1 + c2),
                            Mode::Lighten => ((1.0 - a1) * c2 + c1).max((1.0 - a2) * c1 + c2),
                        };
                        v.clamp(0.0, a) as u8
                    };
                    *d = [c(0), c(1), c(2), a as u8];
                }
                out
            }
        };
        out.clip(run.m, sub);
        Some(Rc::new(out))
    }
}

fn clipped(mut b: Buf, m: &M, local: RectF) -> Buf {
    b.clip(m, local);
    b
}

/// QSvgFeGaussianBlur::apply: three box blurs in the space scaled like the painter.
/// Whether QRasterPaintEngine::drawImage of an image at `target` under the matrix `m`, on a
/// painter that does not antialias, takes qt_transform_image: a rotation or a shear, and a
/// target of 16 pixels and more each way (QTBUG-93475).
fn fast_transform(m: &M, target: IRect) -> bool {
    let fuzzy = |v: f64| v.abs() <= 1e-12;
    let bounds = map_rect(m, target.to_f());
    !(fuzzy(m[1]) && fuzzy(m[2]))
        && m[0] < 512.0
        && m[3] < 512.0
        && bounds[2] >= 16.0
        && bounds[3] >= 16.0
}

/// qt_transform_image of `src`, lying at its own rectangle, under the matrix `m`, which takes
/// that rectangle to the pixels of `dst`: the mapped rectangle cut into three trapezoids, each
/// row of them running between its edges in 16.16, the source stepped in 16.16 from the
/// pixel centre of the first pixel. `dst` is transparent, so SourceOver is a copy.
fn transform_image(dst: &mut Buf, src: &Buf, m: &M) {
    #[derive(Clone, Copy)]
    struct Vertex {
        x: f64,
        y: f64,
        u: f64,
        v: f64,
    }
    let (ox, oy) = (dst.rect.x as f64, dst.rect.y as f64);
    let r = src.rect.to_f();
    let (sw, sh) = (src.rect.w as i64, src.rect.h as i64);
    let at = |x: f64, y: f64, u: f64, v: f64| Vertex {
        x: m[0] * x + m[2] * y + m[4] - ox,
        y: m[1] * x + m[3] * y + m[5] - oy,
        u,
        v,
    };
    // TopLeft, TopRight, BottomRight, BottomLeft.
    let mut v = [
        at(r[0], r[1], 0.0, 0.0),
        at(r[0] + r[2], r[1], r[2], 0.0),
        at(r[0] + r[2], r[1] + r[3], r[2], r[3]),
        at(r[0], r[1] + r[3], 0.0, r[3]),
    ];
    let mut topmost = 0;
    for i in 1..4 {
        if v[i].y < v[topmost].y {
            topmost = i;
        }
    }
    v.rotate_left(topmost);
    let (dx1, dy1) = (v[1].x - v[0].x, v[1].y - v[0].y);
    let (dx2, dy2) = (v[3].x - v[0].x, v[3].y - v[0].y);
    if dx1 * dy2 - dx2 * dy1 > 0.0 {
        v.swap(1, 3);
    }
    let u = (
        v[1].x - v[0].x,
        v[1].y - v[0].y,
        v[1].u - v[0].u,
        v[1].v - v[0].v,
    );
    let w = (
        v[2].x - v[0].x,
        v[2].y - v[0].y,
        v[2].u - v[0].u,
        v[2].v - v[0].v,
    );
    let det = u.0 * w.1 - u.1 * w.0;
    if det == 0.0 {
        return;
    }
    let inv = 1.0 / det;
    let m11 = (u.2 * w.1 - u.1 * w.2) * inv;
    let m12 = (u.0 * w.2 - u.2 * w.0) * inv;
    let m21 = (u.3 * w.1 - u.1 * w.3) * inv;
    let m22 = (u.0 * w.3 - u.3 * w.0) * inv;
    let mdx = v[0].u - m11 * v[0].x - m12 * v[0].y;
    let mdy = v[0].v - m21 * v[0].x - m22 * v[0].y;
    const ONE: f64 = 65536.0;
    let fixed = |f: f64| (f * ONE) as i32 as i64;
    let (dudx, dvdx, dudy, dvdy) = (fixed(m11), fixed(m21), fixed(m12), fixed(m22));
    let u0 = ((0.5 * m11 + 0.5 * m12 + mdx) * ONE).ceil() as i32 as i64 - 1;
    let v0 = ((0.5 * m21 + 0.5 * m22 + mdy) * ONE).ceil() as i32 as i64 - 1;
    let (dw, dh) = (dst.rect.w as i64, dst.rect.h as i64);
    // qt_transform_image_rasterize.
    let mut trapezoid = |tl: Vertex, bl: Vertex, tr: Vertex, br: Vertex, top: f64, bottom: f64| {
        let from_y = (q_round(top) as i64).max(0);
        let to_y = (q_round(bottom) as i64).min(dh);
        if from_y >= to_y {
            return;
        }
        let left = (bl.x - tl.x) / (bl.y - tl.y);
        let right = (br.x - tr.x) / (br.y - tr.y);
        let (dx_l, dx_r) = ((left * ONE) as i64, (right * ONE) as i64);
        let mut x_l = ((tl.x + (0.5 + from_y as f64 - tl.y) * left + 0.5) * ONE) as i64;
        let mut x_r = ((tr.x + (0.5 + from_y as f64 - tr.y) * right + 0.5) * ONE) as i64;
        for y in from_y..to_y {
            let (from_x, to_x) = ((x_l >> 16).max(0), (x_r >> 16).min(dw));
            for x in from_x..to_x {
                let su = ((x * dudx + y * dudy + u0) >> 16).clamp(0, sw - 1);
                let sv = ((x * dvdx + y * dvdy + v0) >> 16).clamp(0, sh - 1);
                over(
                    &mut dst.px[(y * dw + x) as usize],
                    src.px[(sv * sw + su) as usize],
                );
            }
            x_l += dx_l;
            x_r += dx_r;
        }
    };
    if v[1].y < v[3].y {
        trapezoid(v[0], v[1], v[0], v[3], v[0].y, v[1].y);
        trapezoid(v[1], v[2], v[0], v[3], v[1].y, v[3].y);
        trapezoid(v[1], v[2], v[3], v[2], v[3].y, v[2].y);
    } else {
        trapezoid(v[0], v[1], v[0], v[3], v[0].y, v[3].y);
        trapezoid(v[0], v[1], v[3], v[2], v[3].y, v[1].y);
        trapezoid(v[1], v[2], v[3], v[2], v[1].y, v[2].y);
    }
}

fn blur(src: &Buf, sx: f64, sy: f64, run: &Run, sub: RectF) -> Option<Buf> {
    let m = run.m;
    let (scale_x, scale_y) = (m[0].hypot(m[2]), m[1].hypot(m[3]));
    if scale_x == 0.0 || scale_y == 0.0 {
        return None;
    }
    let (mut sigma_x, mut sigma_y) = (scale_x * sx, scale_y * sy);
    if run.units == ObjectBoundingBox {
        sigma_x *= run.item[2];
        sigma_y *= run.item[3];
    }
    const SD: f64 = 3.0 * FRAC_1_SQRT_2 / FRAC_2_SQRT_PI;
    let dx = (sigma_x * SD + 0.5).floor() as i64;
    let dy = (sigma_y * SD + 0.5).floor() as i64;
    // restXr: the painter transform without its scale.
    let rest: M = [
        m[0] / scale_x,
        m[1] / scale_x,
        m[2] / scale_y,
        m[3] / scale_y,
        m[4],
        m[5],
    ];
    let fuzzy = |v: f64| v.abs() <= 1e-12;
    let translation =
        fuzzy(rest[0] - 1.0) && fuzzy(rest[3] - 1.0) && fuzzy(rest[1]) && fuzzy(rest[2]);
    let scaled = to_rect(map_rect(&[scale_x, 0.0, 0.0, scale_y, 0.0, 0.0], sub));
    let mut temp = Buf::new(scaled)?;
    let det = rest[0] * rest[3] - rest[1] * rest[2];
    if translation {
        // The raster engine blits an untransformed image at the rounded position.
        let x = q_round(src.rect.x as f64 + (-rest[4] + -(scaled.x as f64)));
        let y = q_round(src.rect.y as f64 + (-rest[5] + -(scaled.y as f64)));
        temp.draw(src, scaled.x + x, scaled.y + y, over);
    } else if let Some(to_temp) = super::invert(&rest).filter(|t| fast_transform(t, src.rect)) {
        transform_image(&mut temp, src, &to_temp);
    } else {
        // A rotated or mirrored painter: the image is sampled at the pixel centres.
        for (i, d) in temp.px.iter_mut().enumerate() {
            let x = (i as i32 % scaled.w + scaled.x) as f64 + 0.5;
            let y = (i as i32 / scaled.w + scaled.y) as f64 + 0.5;
            let (gx, gy) = (
                rest[0] * x + rest[2] * y + rest[4],
                rest[1] * x + rest[3] * y + rest[5],
            );
            if let Some(s) = src.get(gx.floor() as i32, gy.floor() as i32) {
                *d = s;
            }
        }
    }
    let (w, h) = (scaled.w as usize, scaled.h as usize);
    let mut sums = vec![[0u64; 4]; w * h];
    let adjust = |d: i64, iteration: usize| {
        let d = d.max(1);
        if d % 2 == 1 {
            (d / 2 + 1, d / 2)
        } else {
            match iteration {
                0 => (d / 2 + 1, d / 2 - 1),
                1 => (d / 2, d / 2),
                _ => (d / 2 + 1, d / 2),
            }
        }
    };
    for iteration in 0..3 {
        let (left, right) = adjust(dx, iteration);
        let (top, bottom) = adjust(dy, iteration);
        for j in 0..h {
            for i in 0..w {
                let mut v = temp.px[i + j * w].map(u64::from);
                for k in 0..4 {
                    if i > 0 {
                        v[k] = v[k].wrapping_add(sums[i - 1 + j * w][k]);
                    }
                    if j > 0 {
                        v[k] = v[k].wrapping_add(sums[i + (j - 1) * w][k]);
                    }
                    if i > 0 && j > 0 {
                        v[k] = v[k].wrapping_sub(sums[i - 1 + (j - 1) * w][k]);
                    }
                }
                sums[i + j * w] = v;
            }
        }
        let div = (left + right) as u64 * (top + bottom) as u64;
        for j in 0..h {
            let j1 = (j as i64 - top).max(0) as usize;
            let j2 = (j as i64 + bottom).min(h as i64 - 1) as usize;
            for i in 0..w {
                let i1 = (i as i64 - left).max(0) as usize;
                let i2 = (i as i64 + right).min(w as i64 - 1) as usize;
                let mut v = [0u8; 4];
                for k in 0..4 {
                    let sum = sums[i2 + j2 * w][k]
                        .wrapping_sub(sums[i1 + j2 * w][k])
                        .wrapping_sub(sums[i2 + j1 * w][k])
                        .wrapping_add(sums[i1 + j1 * w][k]);
                    v[k] = (sum / div) as u8;
                }
                temp.px[i + j * w] = v;
            }
        }
    }
    let global = to_rect(map_rect(m, sub));
    let mut out = Buf::new(global)?;
    if translation {
        let x = q_round(scaled.x as f64 + (rest[4] + -(global.x as f64)));
        let y = q_round(scaled.y as f64 + (rest[5] + -(global.y as f64)));
        out.draw(&temp, global.x + x, global.y + y, over);
    } else if det != 0.0 {
        // An antialiasing painter without SmoothPixmapTransform: the image's rectangle
        // antialiased, each pixel of it the nearest of the image.
        let mut ctx = Context::new(global.w as usize, global.h as usize);
        let mut img = Canvas::new(scaled.w as usize, scaled.h as usize, [0; 4]);
        img.px = temp.px;
        let mut device = rest;
        device[4] -= global.x as f64;
        device[5] -= global.y as f64;
        let at = [
            scaled.x as f64,
            scaled.y as f64,
            scaled.w as f64,
            scaled.h as f64,
        ];
        super::picture::draw(&mut ctx, &img, at, device, 1.0, false);
        out.px = ctx.img.px;
    }
    Some(out)
}
