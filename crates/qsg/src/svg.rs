//! SVG icons as QtSvg draws them for QImageReader (QSvgIOHandler): ARGB32_Premultiplied target,
//! QPainter Antialiasing, viewBox stretched like QSvgDocument::mapSourceToTarget (IgnoreAspectRatio).
//! Supported subset (the original's assets and the icon themes `image://icon` draws):
//! svg/g/path/rect/circle/ellipse/line/polyline/polygon, transform,
//! fill/fill-opacity/fill-rule/opacity/display, strokes with
//! stroke-linecap/-linejoin/-miterlimit/-dasharray/-dashoffset (QSvgHandler's parsePen),
//! linear/radial gradient fills (pad spread) for fills and strokes, masks (QSvgMask).
//! clipPath/filter are skipped.
use crate::canvas::{Cap, Context, El, Join, Path, stroke_outline};
use crate::image::FillMode;
use crate::raster::Canvas;
use effects::{Buf, Filter, IRect, Mask, RectF, Units, map_rect, to_aligned_rect, to_rect, united};
use gradient::{Geometry, Rgba64, Spread};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

mod effects;
#[cfg(test)]
mod equiv;
mod glyphs;
mod gradient;
#[cfg(test)]
mod legacy;
mod picture;

/// a b c d e f: x' = a x + c y + e, y' = b x + d y + f.
type M = [f64; 6];
const ID: M = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// Point maps through `t` first, then `w`.
fn compose(t: M, w: M) -> M {
    [
        w[0] * t[0] + w[2] * t[1],
        w[1] * t[0] + w[3] * t[1],
        w[0] * t[2] + w[2] * t[3],
        w[1] * t[2] + w[3] * t[3],
        w[0] * t[4] + w[2] * t[5] + w[4],
        w[1] * t[4] + w[3] * t[5] + w[5],
    ]
}

struct Elem {
    name: String,
    attrs: Vec<(String, String)>,
    children: Vec<Elem>,
}

impl Elem {
    fn attr(&self, k: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(a, _)| a == k)
            .map(|(_, v)| v.as_str())
    }
}

/// Element tree from roxmltree. Elements go by their local name: QSvgHandler::parse reads
/// with namespace processing off and hands `xml->name()` to startElement, so `svg:path` is a
/// path whatever its prefix is bound to. Attributes keep their qualified name
/// (QSvgAttributes reads `qualifiedName()`); `xmlns` declarations are not attributes.
fn parse_xml(src: &str) -> Option<Elem> {
    let mut root = read_xml(src)?;
    root.children = attach(std::mem::take(&mut root.children), true);
    Some(root)
}

/// The elements as the file nests them.
fn read_xml(src: &str) -> Option<Elem> {
    let opt = roxmltree::ParsingOptions {
        allow_dtd: true,
        ..Default::default()
    };
    let doc = roxmltree::Document::parse_with_options(src, opt).ok()?;
    Some(elem(src, doc.root_element()))
}

/// What QSvgHandler::startElement makes of an element name.
#[derive(PartialEq)]
enum Kind {
    /// A structure node that takes child nodes.
    Container,
    /// Any other node.
    Leaf,
    /// No node: a paint server, `style`, `metadata`, a name QtSvg does not know.
    None,
}

fn kind(name: &str) -> Kind {
    match name {
        "g" | "defs" | "switch" | "mask" | "symbol" | "marker" | "pattern" => Kind::Container,
        "circle" | "ellipse" | "image" | "line" | "path" | "polygon" | "polyline" | "rect"
        | "text" | "textArea" | "tspan" | "use" | "video" | "filter" => Kind::Leaf,
        _ => Kind::None,
    }
}

/// The node tree QSvgHandler::startElement builds from `children`. An element that makes no
/// node does not hide what is inside it: the nodes there are added to the node being read
/// (`m_nodes.top()`), so the paths of a `clipPath` outside `defs` are drawn where it stands,
/// without its attributes. A nested `svg` is skipped with everything in it, and only structure
/// nodes take children (`accepts`); `text` and `filter` keep theirs for their own reading.
fn attach(children: Vec<Elem>, accepts: bool) -> Vec<Elem> {
    let mut out = Vec::new();
    for mut c in children {
        let inner = std::mem::take(&mut c.children);
        match kind(&c.name) {
            _ if c.name == "svg" => {}
            Kind::Container if accepts => {
                c.children = attach(inner, true);
                out.push(c);
            }
            Kind::Leaf if accepts => {
                if matches!(c.name.as_str(), "text" | "textArea" | "filter") {
                    c.children = inner;
                }
                out.push(c);
            }
            Kind::Container | Kind::Leaf => {}
            Kind::None => {
                let (nodes, rest): (Vec<Elem>, Vec<Elem>) = attach(inner, accepts)
                    .into_iter()
                    .partition(|e| kind(&e.name) != Kind::None);
                c.children = rest;
                out.push(c);
                out.extend(nodes);
            }
        }
    }
    out
}

fn elem(src: &str, n: roxmltree::Node) -> Elem {
    Elem {
        name: n.tag_name().name().to_string(),
        attrs: n
            .attributes()
            .map(|a| {
                let k = src
                    .get(a.range_qname())
                    .filter(|q| q.ends_with(a.name()))
                    .unwrap_or(a.name());
                (k.to_string(), a.value().to_string())
            })
            .collect(),
        // Character data is kept where QSvgHandler::characters keeps it: in `text` and `tspan`.
        children: n
            .children()
            .filter_map(|c| {
                let name = n.tag_name().name();
                if c.is_element() {
                    Some(elem(src, c))
                } else if c.is_text() && matches!(name, "text" | "textArea" | "tspan") {
                    Some(Elem {
                        name: "#text".to_string(),
                        attrs: vec![(String::new(), c.text().unwrap_or("").to_string())],
                        children: Vec::new(),
                    })
                } else {
                    None
                }
            })
            .collect(),
    }
}

/// Leading number of an attribute value, unit or garbage after it ignored (QSvg toDouble).
fn num_prefix(s: &str) -> f64 {
    svgtypes::NumberListParser::from(s)
        .next()
        .and_then(Result::ok)
        .unwrap_or(0.0)
}

/// QSvgHandler resolveColor: `None` keeps the inherited paint, `Some(None)` paints nothing.
fn parse_color(v: &str) -> Option<Option<[u8; 4]>> {
    let v = v.trim();
    match v {
        "none" | "transparent" => return Some(None),
        "currentColor" => return Some(Some([0, 0, 0, 255])),
        _ => {}
    }
    if let Some(h) = v.strip_prefix('#') {
        // qsvg_get_hex_rgb has no alpha forms (#rgba, #rrggbbaa).
        if !matches!(h.len(), 3 | 6) {
            return None;
        }
    } else if let Some(inner) = v.strip_prefix("rgb(") {
        let inner = inner.strip_suffix(')')?;
        let mut c: Vec<f64> = svgtypes::NumberListParser::from(inner)
            .map_while(Result::ok)
            .collect();
        // A single number means the list stopped at '%': percentages scaled by 2.55.
        if c.len() == 1 {
            c = svgtypes::LengthListParser::from(inner)
                .map_while(Result::ok)
                .map(|l| l.number * 2.55)
                .collect();
        }
        let &[r, g, b] = c.as_slice() else {
            return None;
        };
        let u = |x: f64| x.clamp(0.0, 255.0) as u8;
        return Some(Some([u(r), u(g), u(b), 255]));
    } else if !v.bytes().all(|b| b.is_ascii_alphabetic()) {
        // QColor::fromString: only #hex and SVG color keywords (no rgba()/hsl()).
        if v.starts_with("url(") {
            elog::eprintln!("qsg::svg: paint servers not supported yet ({v})");
        }
        return None;
    }
    // CSS4 `rebeccapurple` is not among QColor's SVG keywords.
    if v.eq_ignore_ascii_case("rebeccapurple") {
        return None;
    }
    let c = v.parse::<svgtypes::Color>().ok()?;
    Some(Some([c.red, c.green, c.blue, 255]))
}

fn nums(s: &str) -> Vec<f64> {
    svgtypes::NumberListParser::from(s)
        .map_while(Result::ok)
        .collect()
}

/// `stroke-dasharray` as QSvgHandler's parsePen reads it: `none`, only zeros or a negative
/// value is a solid line; an odd count is repeated to make it even.
fn parse_dashes(v: &str) -> Vec<f64> {
    let mut d = nums(v);
    if d.iter().all(|x| x.abs() <= 1e-12) || d.iter().any(|x| *x < 0.0) {
        return Vec::new();
    }
    if d.len() % 2 == 1 {
        d.extend_from_within(..);
    }
    d
}

/// Transform list composed left to right like QSvgHandler's QTransform calls; `rotate(a cx cy)`
/// arrives as translate/rotate/translate.
fn parse_transform(s: &str) -> M {
    use svgtypes::TransformListToken as T;
    let mut acc = ID;
    for tok in svgtypes::TransformListParser::from(s).map_while(Result::ok) {
        let t = match tok {
            T::Matrix { a, b, c, d, e, f } => [a, b, c, d, e, f],
            T::Translate { tx, ty } => [1., 0., 0., 1., tx, ty],
            T::Scale { sx, sy } => [sx, 0., 0., sy, 0., 0.],
            T::Rotate { angle } => {
                let (s, c) = angle.to_radians().sin_cos();
                [c, s, -s, c, 0., 0.]
            }
            T::SkewX { angle } => [1., 0., angle.to_radians().tan(), 1., 0., 0.],
            T::SkewY { angle } => [1., angle.to_radians().tan(), 0., 1., 0., 0.],
        };
        acc = compose(t, acc);
    }
    acc
}

/// User-space QPainterPath elements (Move/Line/Cubic, closes as Line back to start).
type UPath = Vec<El>;

/// `m` (a00 a01 a10 a11) maps the unit circle onto the ellipse: radii rx, ry rotated by xrot.
fn arc_segment(p: &mut UPath, xc: f64, yc: f64, th0: f64, th1: f64, m: [f64; 4]) {
    let [a00, a01, a10, a11] = m;
    let th_half = 0.5 * (th1 - th0);
    let t = (8.0 / 3.0) * (th_half * 0.5).sin() * (th_half * 0.5).sin() / th_half.sin();
    let x1 = xc + th0.cos() - t * th0.sin();
    let y1 = yc + th0.sin() + t * th0.cos();
    let x3 = xc + th1.cos();
    let y3 = yc + th1.sin();
    let x2 = x3 + t * th1.sin();
    let y2 = y3 - t * th1.cos();
    p.push(El::Cubic(
        [a00 * x1 + a01 * y1, a10 * x1 + a11 * y1],
        [a00 * x2 + a01 * y2, a10 * x2 + a11 * y2],
        [a00 * x3 + a01 * y3, a10 * x3 + a11 * y3],
    ));
}

#[allow(clippy::too_many_arguments)]
fn path_arc(
    p: &mut UPath,
    mut rx: f64,
    mut ry: f64,
    xrot: f64,
    large: bool,
    sweep: bool,
    x: f64,
    y: f64,
    cx: f64,
    cy: f64,
) {
    rx = rx.abs();
    ry = ry.abs();
    let (sin_th, cos_th) = xrot.to_radians().sin_cos();
    let dx = (cx - x) / 2.0;
    let dy = (cy - y) / 2.0;
    let dx1 = cos_th * dx + sin_th * dy;
    let dy1 = -sin_th * dx + cos_th * dy;
    let check = (dx1 * dx1) / (rx * rx) + (dy1 * dy1) / (ry * ry);
    if check > 1.0 {
        rx *= check.sqrt();
        ry *= check.sqrt();
    }
    let (a00, a01, a10, a11) = (cos_th / rx, sin_th / rx, -sin_th / ry, cos_th / ry);
    let x0 = a00 * cx + a01 * cy;
    let y0 = a10 * cx + a11 * cy;
    let x1 = a00 * x + a01 * y;
    let y1 = a10 * x + a11 * y;
    let d = (x1 - x0) * (x1 - x0) + (y1 - y0) * (y1 - y0);
    if d == 0.0 {
        return;
    }
    let mut sfactor_sq = 1.0 / d - 0.25;
    if sfactor_sq < 0.0 {
        sfactor_sq = 0.0;
    }
    let mut sfactor = sfactor_sq.sqrt();
    if sweep == large {
        sfactor = -sfactor;
    }
    let xc = 0.5 * (x0 + x1) - sfactor * (y1 - y0);
    let yc = 0.5 * (y0 + y1) + sfactor * (x1 - x0);
    let th0 = (y0 - yc).atan2(x0 - xc);
    let th1 = (y1 - yc).atan2(x1 - xc);
    let mut th_arc = th1 - th0;
    if th_arc < 0.0 && sweep {
        th_arc += 2.0 * std::f64::consts::PI;
    } else if th_arc > 0.0 && !sweep {
        th_arc -= 2.0 * std::f64::consts::PI;
    }
    let n_segs = (th_arc / (std::f64::consts::FRAC_PI_2 + 0.001))
        .abs()
        .ceil() as i32;
    let m = [cos_th * rx, -sin_th * ry, sin_th * rx, cos_th * ry];
    for i in 0..n_segs {
        let a = th0 + i as f64 * th_arc / n_segs as f64;
        let b = th0 + (i + 1) as f64 * th_arc / n_segs as f64;
        arc_segment(p, xc, yc, a, b, m);
    }
}

/// qFuzzyCompare of two points as QPointF::operator== does it.
fn fuzzy_pt(a: [f64; 2], b: [f64; 2]) -> bool {
    let eq = |a: f64, b: f64| {
        if a == 0.0 || b == 0.0 {
            (a - b).abs() <= 1e-12
        } else {
            fuzzy(a, b)
        }
    };
    eq(a[0], b[0]) && eq(a[1], b[1])
}

/// qFuzzyCompare.
fn fuzzy(a: f64, b: f64) -> bool {
    (a - b).abs() * 1e12 <= a.abs().min(b.abs())
}

/// A QPainterPath under construction: what moveTo, lineTo, cubicTo and closeSubpath keep.
struct Builder {
    el: UPath,
    /// Index of the MoveTo that starts the current subpath.
    start: usize,
    /// QPainterPathPrivate::require_moveTo: the subpath was closed.
    closed: bool,
}

impl Builder {
    fn last(&self) -> [f64; 2] {
        match self.el.last() {
            Some(El::Move(p) | El::Line(p) | El::Cubic(_, _, p)) => *p,
            None => [0.0, 0.0],
        }
    }

    fn first(&self) -> [f64; 2] {
        match self.el.get(self.start) {
            Some(El::Move(p)) => *p,
            _ => [0.0, 0.0],
        }
    }

    /// QPainterPathPrivate::maybeMoveTo, and the MoveTo(0, 0) an empty path starts with.
    fn begin(&mut self) {
        if self.el.is_empty() || self.closed {
            let p = self.first();
            self.closed = false;
            self.start = self.el.len();
            self.el.push(El::Move(p));
        }
    }

    fn move_to(&mut self, p: [f64; 2]) {
        self.closed = false;
        if let Some(El::Move(last)) = self.el.last_mut() {
            *last = p;
        } else {
            self.start = self.el.len();
            self.el.push(El::Move(p));
        }
    }

    fn line_to(&mut self, p: [f64; 2]) {
        self.begin();
        if !fuzzy_pt(p, self.last()) {
            self.el.push(El::Line(p));
        }
    }

    fn cubic_to(&mut self, c1: [f64; 2], c2: [f64; 2], e: [f64; 2]) {
        if !self.el.is_empty() && fuzzy_pt(self.last(), c1) && fuzzy_pt(c1, c2) && fuzzy_pt(c2, e) {
            return;
        }
        self.begin();
        self.el.push(El::Cubic(c1, c2, e));
    }

    /// QPainterPathPrivate::close: an end that is all but the start is moved onto it.
    fn close(&mut self) {
        if self.el.is_empty() {
            return;
        }
        self.closed = true;
        let (first, last) = (self.first(), self.last());
        if first == last {
            return;
        }
        if fuzzy(first[0], last[0]) && fuzzy(first[1], last[1]) {
            if let Some(El::Move(p) | El::Line(p) | El::Cubic(_, _, p)) = self.el.last_mut() {
                *p = first;
            }
        } else {
            self.el.push(El::Line(first));
        }
    }
}

/// QQuickSvgParser::parsePathDataFast: the QPainterPath of an SVG path string.
pub fn path_data(d: &str) -> Path {
    Path::from_el(parse_path(d))
}

fn parse_path(d: &str) -> UPath {
    use svgtypes::PathSegment as S;
    let mut p = Builder {
        el: vec![],
        start: 0,
        closed: false,
    };
    let (mut cur, mut start) = ([0.0f64; 2], [0.0f64; 2]);
    let mut last_ctrl: Option<[f64; 2]> = None;
    let mut last_q: Option<[f64; 2]> = None;
    // QGuiSvg::parsePath keeps the path parsed up to the first error.
    for seg in svgtypes::PathParser::from(d).map_while(Result::ok) {
        let o = match seg {
            S::MoveTo { abs: false, .. }
            | S::LineTo { abs: false, .. }
            | S::CurveTo { abs: false, .. }
            | S::SmoothCurveTo { abs: false, .. }
            | S::Quadratic { abs: false, .. }
            | S::SmoothQuadratic { abs: false, .. }
            | S::EllipticalArc { abs: false, .. } => cur,
            _ => [0.0, 0.0],
        };
        let pt = |x: f64, y: f64| [o[0] + x, o[1] + y];
        let reflect =
            |c: Option<[f64; 2]>| c.map_or(cur, |c| [2.0 * cur[0] - c[0], 2.0 * cur[1] - c[1]]);
        let mut ctrl = None;
        let mut q = None;
        match seg {
            S::MoveTo { x, y, .. } => {
                cur = pt(x, y);
                start = cur;
                p.move_to(cur);
            }
            S::LineTo { x, y, .. } => {
                cur = pt(x, y);
                p.line_to(cur);
            }
            S::HorizontalLineTo { abs, x } => {
                cur = [if abs { x } else { cur[0] + x }, cur[1]];
                p.line_to(cur);
            }
            S::VerticalLineTo { abs, y } => {
                cur = [cur[0], if abs { y } else { cur[1] + y }];
                p.line_to(cur);
            }
            S::CurveTo {
                x1,
                y1,
                x2,
                y2,
                x,
                y,
                ..
            } => {
                let (c1, c2, e) = (pt(x1, y1), pt(x2, y2), pt(x, y));
                p.cubic_to(c1, c2, e);
                ctrl = Some(c2);
                cur = e;
            }
            S::SmoothCurveTo { x2, y2, x, y, .. } => {
                let c1 = reflect(last_ctrl);
                let (c2, e) = (pt(x2, y2), pt(x, y));
                p.cubic_to(c1, c2, e);
                ctrl = Some(c2);
                cur = e;
            }
            S::Quadratic { .. } | S::SmoothQuadratic { .. } => {
                let (qc, e) = match seg {
                    S::Quadratic { x1, y1, x, y, .. } => (pt(x1, y1), pt(x, y)),
                    S::SmoothQuadratic { x, y, .. } => (reflect(last_q), pt(x, y)),
                    _ => unreachable!(),
                };
                let c1 = [
                    cur[0] + 2.0 / 3.0 * (qc[0] - cur[0]),
                    cur[1] + 2.0 / 3.0 * (qc[1] - cur[1]),
                ];
                let c2 = [
                    e[0] + 2.0 / 3.0 * (qc[0] - e[0]),
                    e[1] + 2.0 / 3.0 * (qc[1] - e[1]),
                ];
                p.cubic_to(c1, c2, e);
                q = Some(qc);
                cur = e;
            }
            S::EllipticalArc {
                rx,
                ry,
                x_axis_rotation,
                large_arc,
                sweep,
                x,
                y,
                ..
            } => {
                let e = pt(x, y);
                // pathArc: no arc between equal points, a line for a radius of nothing.
                if !fuzzy_pt(cur, e) {
                    if (rx * rx).abs() <= 1e-12 || (ry * ry).abs() <= 1e-12 {
                        p.line_to(e);
                    } else {
                        let mut arc = vec![];
                        path_arc(
                            &mut arc,
                            rx,
                            ry,
                            x_axis_rotation,
                            large_arc,
                            sweep,
                            e[0],
                            e[1],
                            cur[0],
                            cur[1],
                        );
                        for el in arc {
                            if let El::Cubic(c1, c2, end) = el {
                                p.cubic_to(c1, c2, end);
                            }
                        }
                    }
                }
                cur = e;
            }
            S::ClosePath { .. } => {
                p.close();
                cur = start;
            }
        }
        last_ctrl = ctrl;
        last_q = q;
    }
    p.el
}

fn ellipse(x: f64, y: f64, w: f64, h: f64) -> UPath {
    const K: f64 = 0.5522847498;
    let (w2, h2) = (w / 2.0, h / 2.0);
    let (w2k, h2k) = (w2 * K, h2 * K);
    vec![
        El::Move([x + w, y + h2]),
        El::Cubic(
            [x + w, y + h2 + h2k],
            [x + w2 + w2k, y + h],
            [x + w2, y + h],
        ),
        El::Cubic([x + w2 - w2k, y + h], [x, y + h2 + h2k], [x, y + h2]),
        El::Cubic([x, y + h2 - h2k], [x + w2 - w2k, y], [x + w2, y]),
        El::Cubic([x + w2 + w2k, y], [x + w, y + h2 - h2k], [x + w, y + h2]),
        El::Line([x + w, y + h2]),
    ]
}

/// QPaintEngineEx::drawRoundedRect, which QSvgRect ends in: createRectNode keeps the radii as
/// percentages of the half sides (Qt::RelativeSize), and the path starts after the top left
/// corner and keeps every line, also one of no length.
fn rounded_rect(x: f64, y: f64, w: f64, h: f64, rx: f64, ry: f64) -> UPath {
    const K: f64 = 0.5522847498;
    let (xr, yr) = (rx * (100.0 / (w / 2.0)), ry * (100.0 / (h / 2.0)));
    let (rx, ry) = ((xr * w / 200.0).min(w / 2.0), (yr * h / 200.0).min(h / 2.0));
    let (x1, y1, x2, y2) = (x, y, x + w, y + h);
    let (kx, ky) = ((1.0 - K) * rx, (1.0 - K) * ry);
    vec![
        El::Move([x1 + rx, y1]),
        El::Line([x2 - rx, y1]),
        El::Cubic([x2 - kx, y1], [x2, y1 + ky], [x2, y1 + ry]),
        El::Line([x2, y2 - ry]),
        El::Cubic([x2, y2 - ky], [x2 - kx, y2], [x2 - rx, y2]),
        El::Line([x1 + rx, y2]),
        El::Cubic([x1 + kx, y2], [x1, y2 - ky], [x1, y2 - ry]),
        El::Line([x1, y1 + ry]),
        El::Cubic([x1, y1 + ky], [x1 + kx, y1], [x1 + rx, y1]),
    ]
}

/// A `linearGradient`/`radialGradient` as QSvgGradientPaint holds it once its stops are
/// resolved: a QGradient and the brush transform.
struct Grad {
    geo: Geometry,
    /// QGradient::ObjectMode: coordinates are fractions of the painted path's bounding box.
    object_mode: bool,
    spread: Spread,
    transform: M,
    stops: Vec<(f64, Rgba64)>,
}

fn decls(text: &str) -> Vec<(String, String)> {
    text.split(';')
        .filter_map(|d| d.split_once(':'))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect()
}

/// `.Class { decl; ... }` rules from `<style>` blocks (KDE ColorScheme-* icons).
fn class_rules(src: &str) -> HashMap<String, Vec<(String, String)>> {
    let mut out: HashMap<String, Vec<(String, String)>> = HashMap::new();
    let mut rest = src;
    while let Some(a) = rest.find("<style") {
        let body = &rest[a..];
        let Some(open) = body.find('>') else { break };
        let end = body.find("</style>").unwrap_or(body.len());
        let css = body[open + 1..end.max(open + 1)]
            .replace("<![CDATA[", "")
            .replace("]]>", "");
        for rule in css.split('}') {
            if let Some((sel, body)) = rule.split_once('{') {
                for sel in sel.split(',') {
                    if let Some(c) = sel.trim().strip_prefix('.') {
                        out.entry(c.to_string()).or_default().extend(decls(body));
                    }
                }
            }
        }
        rest = &body[end.min(body.len())..];
        if end == body.len() {
            break;
        }
        rest = &rest[8..];
    }
    out
}

fn replace_scheme_style(src: &str, css: &str) -> String {
    let mut out = String::with_capacity(src.len() + css.len());
    let mut rest = src;
    while let Some(a) = rest.find("<style") {
        let body = &rest[a..];
        let Some(close) = body.find('>') else { break };
        let tag = &body[..close];
        let is_scheme = tag.contains("id=\"current-color-scheme\"")
            || tag.contains("id='current-color-scheme'");
        if !is_scheme {
            out.push_str(&rest[..a + close + 1]);
            rest = &body[close + 1..];
            continue;
        }
        out.push_str(&rest[..a]);
        let (open, after) = match tag.strip_suffix('/') {
            Some(t) => (t, &body[close + 1..]),
            None => match body.find("</style>") {
                Some(e) => (tag, &body[e + 8..]),
                None => break,
            },
        };
        out.push_str(open);
        out.push('>');
        out.push_str(css);
        out.push_str("</style>");
        rest = after;
    }
    out.push_str(rest);
    out
}

fn style_props(e: &Elem) -> Vec<(String, String)> {
    let mut props = e.attrs.clone();
    if let Some(style) = e.attr("style") {
        for d in style.split(';') {
            if let Some((k, v)) = d.split_once(':') {
                props.push((k.trim().to_string(), v.trim().to_string()));
            }
        }
    }
    props
}

/// The gradients of the document as QSvgHandler builds them (parseBaseGradient,
/// createLinearGradientNode, createRadialGradientNode, parseStopNode), read in document order.
fn collect_grads(
    root: &Elem,
    classes: &HashMap<String, Vec<(String, String)>>,
) -> HashMap<String, Grad> {
    struct Raw {
        grad: Grad,
        /// QSvgGradientPaint::gradientStopsSet.
        stops_set: bool,
        /// QSvgGradientPaint::stopLink: a gradient not defined yet when this one was read.
        link: Option<String>,
    }
    type Classes = HashMap<String, Vec<(String, String)>>;
    fn number(v: &str) -> f64 {
        let v = v.trim();
        match v.strip_suffix('%') {
            Some(p) => num_prefix(p) / 100.0,
            None => num_prefix(v),
        }
    }
    fn stop(st: &Elem, classes: &Classes, mut color: [u8; 4], raw: &mut Raw) {
        let mut props = st.attrs.clone();
        for c in st.attr("class").unwrap_or("").split_whitespace() {
            if let Some(r) = classes.get(c) {
                props.extend(r.iter().cloned());
            }
        }
        if let Some(style) = st.attr("style") {
            props.extend(decls(style));
        }
        let get = |k: &str| {
            props
                .iter()
                .rev()
                .find(|(a, _)| a == k)
                .map(|(_, v)| v.trim())
                .filter(|v| !v.is_empty())
        };
        if let Some(Some(c)) = get("color").and_then(parse_color) {
            color = c;
        }
        let rgb = get("stop-color").and_then(|v| match v {
            "currentColor" => Some(color),
            "transparent" => Some([0; 4]),
            _ => parse_color(v).flatten(),
        });
        let mut c = gradient::rgba64(rgb.unwrap_or([0, 0, 0, 255]));
        if let Some(op) = get("stop-opacity") {
            // setAlpha: QColor::setAlphaF.
            c[3] = (num_prefix(op).clamp(0.0, 1.0) * 65535.0).round() as u16;
        }
        const EPS: f64 = f32::EPSILON as f64;
        let stops = &mut raw.grad.stops;
        let mut offset = get("offset").map_or(0.0, number).clamp(0.0, 1.0);
        if raw.stops_set
            && let Some(last) = stops.last()
            && offset <= last.0
        {
            offset = last.0 + EPS;
        }
        if offset > 1.0 {
            let n = stops.len();
            if (n == 1 || stops[n - 2].0 < 1.0 - EPS)
                && let Some(last) = stops.last_mut()
            {
                last.0 = 1.0 - EPS;
            }
            offset = 1.0;
        }
        // QGradient::setColorAt: a stop at the same offset is replaced.
        match stops.iter_mut().find(|s| s.0 == offset) {
            Some(s) => s.1 = c,
            None => {
                let at = stops.partition_point(|s| s.0 < offset);
                stops.insert(at, (offset, c));
            }
        }
        raw.stops_set = true;
    }
    fn walk(
        e: &Elem,
        classes: &Classes,
        mut color: [u8; 4],
        order: &mut Vec<String>,
        out: &mut HashMap<String, Raw>,
    ) {
        if let Some(Some(c)) = style_props(e)
            .iter()
            .rev()
            .find(|(k, _)| k == "color")
            .and_then(|(_, v)| parse_color(v))
        {
            color = c;
        }
        let radial = e.name == "radialGradient";
        if radial || e.name == "linearGradient" {
            let n = |k: &str, d: f64| e.attr(k).filter(|v| !v.is_empty()).map_or(d, number);
            let geo = if radial {
                let (cx, cy, r) = (n("cx", 0.5), n("cy", 0.5), n("r", 0.5));
                // No paint server at all for r <= 0: a fill that names it stays unresolved.
                if r <= 0.0 {
                    return;
                }
                Geometry::Radial([cx, cy, r, n("fx", cx), n("fy", cy)])
            } else {
                Geometry::Linear([n("x1", 0.0), n("y1", 0.0), n("x2", 1.0), n("y2", 0.0)])
            };
            let mut raw = Raw {
                grad: Grad {
                    geo,
                    object_mode: matches!(
                        e.attr("gradientUnits"),
                        None | Some("" | "objectBoundingBox")
                    ),
                    spread: match e.attr("spreadMethod") {
                        Some("reflect") => Spread::Reflect,
                        Some("repeat") => Spread::Repeat,
                        _ => Spread::Pad,
                    },
                    transform: ID,
                    stops: Vec::new(),
                },
                stops_set: false,
                link: None,
            };
            // Only `xlink:href`, and only the stops and the transform come over: from a gradient
            // read earlier right now, from a later one when the brush is first made.
            if let Some(link) = e
                .attr("xlink:href")
                .and_then(|h| h.trim().strip_prefix('#'))
                .filter(|l| !l.is_empty())
            {
                match out.get(link) {
                    Some(from) => {
                        if from.link.is_some() {
                            raw.link = from.link.clone();
                        } else {
                            raw.grad.stops = from.grad.stops.clone();
                            raw.stops_set = from.stops_set;
                        }
                        raw.grad.transform = from.grad.transform;
                    }
                    None => raw.link = Some(link.to_string()),
                }
            }
            if let Some(t) = e.attr("gradientTransform").filter(|t| !t.is_empty()) {
                raw.grad.transform = parse_transform(t);
            }
            for st in e.children.iter().filter(|c| c.name == "stop") {
                stop(st, classes, color, &mut raw);
            }
            // someId; QSvgDocument::addPaintServer keeps the first of a duplicate id.
            if let Some(id) = e
                .attr("id")
                .filter(|v| !v.is_empty())
                .or_else(|| e.attr("xml:id"))
                .filter(|v| !v.is_empty() && !out.contains_key(*v))
            {
                order.push(id.to_string());
                out.insert(id.to_string(), raw);
            }
            return;
        }
        for c in &e.children {
            walk(c, classes, color, order, out);
        }
    }
    /// QSvgGradientPaint::resolveStops_helper.
    fn resolve(id: &str, raws: &mut HashMap<String, Raw>, visited: &mut Vec<String>) {
        let Some(link) = raws.get_mut(id).and_then(|r| r.link.take()) else {
            return;
        };
        if raws.contains_key(&link) && !visited.contains(&link) {
            visited.push(link.clone());
            resolve(&link, raws, visited);
            let (stops, set) = (raws[&link].grad.stops.clone(), raws[&link].stops_set);
            if let Some(r) = raws.get_mut(id) {
                r.grad.stops = stops;
                r.stops_set = set;
            }
        }
    }
    let (mut order, mut raws) = (Vec::new(), HashMap::new());
    walk(root, classes, [0, 0, 0, 255], &mut order, &mut raws);
    for id in &order {
        resolve(id, &mut raws, &mut Vec::new());
    }
    raws.into_iter()
        .map(|(id, mut r)| {
            // QSvgGradientPaint::brush: a gradient without stops is transparent black.
            if !r.stops_set {
                r.grad.stops = vec![(0.0, [0; 4])];
            }
            (id, r.grad)
        })
        .collect()
}

fn url_id(v: &str) -> Option<String> {
    match svgtypes::Paint::from_str(v) {
        Ok(svgtypes::Paint::FuncIRI(id, _)) => Some(id.to_string()),
        _ => None,
    }
}

/// QPainterPath::boundingRect: the curves' extrema, not their control points.
fn path_bounds(up: &UPath) -> Option<[f64; 4]> {
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    let mut add = |p: [f64; 2]| {
        x0 = x0.min(p[0]);
        y0 = y0.min(p[1]);
        x1 = x1.max(p[0]);
        y1 = y1.max(p[1]);
    };
    let mut cur = [0.0; 2];
    for el in up {
        match *el {
            El::Move(p) | El::Line(p) => {
                add(p);
                cur = p;
            }
            El::Cubic(c1, c2, e) => {
                add(e);
                let at = |t: f64| -> [f64; 2] {
                    let u = 1.0 - t;
                    std::array::from_fn(|k| {
                        u * u * u * cur[k]
                            + 3.0 * u * u * t * c1[k]
                            + 3.0 * u * t * t * c2[k]
                            + t * t * t * e[k]
                    })
                };
                for k in 0..2 {
                    // Derivative roots: a t^2 + b t + c = 0.
                    let a = -cur[k] + 3.0 * c1[k] - 3.0 * c2[k] + e[k];
                    let b = 2.0 * (cur[k] - 2.0 * c1[k] + c2[k]);
                    let c = c1[k] - cur[k];
                    let mut root = |t: f64| {
                        if t > 0.0 && t < 1.0 {
                            add(at(t));
                        }
                    };
                    if a.abs() < 1e-12 {
                        if b.abs() > 1e-12 {
                            root(-c / b);
                        }
                    } else {
                        let d = b * b - 4.0 * a * c;
                        if d >= 0.0 {
                            let q = d.sqrt();
                            root((-b + q) / (2.0 * a));
                            root((-b - q) / (2.0 * a));
                        }
                    }
                }
                cur = e;
            }
        }
    }
    (x0 <= x1).then_some([x0, y0, x1 - x0, y1 - y0])
}

impl Grad {
    /// Paints the gradient through the coverage of `up` (user space, mapped by `m`).
    fn fill(
        &self,
        ctx: &mut Context,
        up: &UPath,
        rect: Option<[f64; 4]>,
        m: M,
        even_odd: bool,
        op: f64,
    ) {
        let (w, h) = (ctx.img.w, ctx.img.h);
        let mut mask = Context::new(w, h);
        match rect {
            // QPainter::drawRect: the rectangle the raster engine fills by its own rule.
            Some(r) => mask.fill_rect_xf(r, [255; 4], 256, m),
            None => mask.fill_opacity(
                &Path::from_el(up.clone()),
                [255, 255, 255, 255],
                even_odd,
                256,
                Some(m),
            ),
        }
        self.paint(ctx, &mask, up, m, op);
    }

    /// Paints the gradient through the alpha of `mask` as the raster engine paints a gradient
    /// brush under painter opacity `op`: QPainterPrivate::drawStretchedGradient maps an
    /// ObjectMode gradient onto the bounding box of `up`, and each run of covered pixels on a
    /// row is one fetch (handleSpans). An opaque gradient (QBrush::isOpaque, full opacity) is
    /// composed as Source instead (getOperator): a run then ends where full coverage meets
    /// partial coverage, and a partly covered pixel is interpolated. A vertical gradient that
    /// is not rotated is one colour a row, composed as SourceOver (qBlendGradient).
    fn paint(&self, ctx: &mut Context, mask: &Context, up: &UPath, m: M, op: f64) {
        let (w, h) = (ctx.img.w, ctx.img.h);
        let mut brush = self.transform;
        if self.object_mode {
            let Some([bx, by, bw, bh]) = path_bounds(up) else {
                return;
            };
            brush = compose(brush, [bw, 0.0, 0.0, bh, bx, by]);
        }
        let spans = gradient::Spans::new(
            &self.stops,
            (op * 256.0) as u32,
            self.spread,
            self.geo,
            compose(brush, m),
        );
        // qt_isExtendedRadialGradient: the focal point outside the circle.
        let extended = match self.geo {
            gradient::Geometry::Radial([cx, cy, r, fx, fy]) => {
                (fx - cx) * (fx - cx) + (fy - cy) * (fy - cy) > r * r
            }
            gradient::Geometry::Linear(_) => false,
        };
        let solid = !extended
            && (op * 256.0) as u32 == 256
            && self.stops.iter().all(|s| s.1[3] == u16::MAX);
        let covered = |y: &usize| mask.img.px[y * w..(y + 1) * w].iter().any(|p| p[3] != 0);
        let Some(first) = (0..h).find(covered) else {
            return;
        };
        let last = (0..h).rfind(covered).unwrap_or(first);
        let rows = spans.rows(first as i64, last as i64);
        let mut src = vec![[0u8; 4]; w];
        for y in first..=last {
            let row = &mask.img.px[y * w..(y + 1) * w];
            if let Some(colour) = &rows {
                let c = colour(y as i64);
                for (d, p) in ctx.img.px[y * w..(y + 1) * w].iter_mut().zip(row) {
                    if p[3] != 0 {
                        gradient::source_over(d, c, p[3] as u32);
                    }
                }
                continue;
            }
            let mut x = 0;
            while x < w {
                if row[x][3] == 0 {
                    x += 1;
                    continue;
                }
                let full = row[x][3] == 255;
                let end = (x..w)
                    .find(|&i| row[i][3] == 0 || (solid && (row[i][3] == 255) != full))
                    .unwrap_or(w)
                    .min(x + 2048);
                spans.fetch(x as i64, y as i64, &mut src[x..end]);
                for i in x..end {
                    let (d, coverage) = (&mut ctx.img.px[y * w + i], row[i][3] as u32);
                    if solid {
                        gradient::source(d, src[i], coverage);
                    } else {
                        gradient::source_over(d, src[i], coverage);
                    }
                }
                x = end;
            }
        }
    }
}

#[derive(Clone)]
struct State {
    m: M,
    fill: Option<[u8; 4]>,
    fill_grad: Option<String>,
    color: [u8; 4],
    fill_opacity: f64,
    even_odd: bool,
    opacity: f64,
    stroke: Option<[u8; 4]>,
    stroke_grad: Option<String>,
    stroke_width: f64,
    stroke_opacity: f64,
    cap: Cap,
    join: Join,
    miter_limit: f64,
    /// `stroke-dasharray` in user units (empty: a solid line) and `stroke-dashoffset`.
    dash: Vec<f64>,
    dash_offset: f64,
    /// The pen is Qt::NoPen (the fill pass of QSvgNode::fillThenStroke); the other pen fields
    /// are then what a style of a `tspan` goes on changing.
    pen_off: bool,
    font: glyphs::FontStyle,
}

impl State {
    /// QSvgHandler's initial style: black fill, no stroke, and its default pen (width 1,
    /// FlatCap, SvgMiterJoin, miter limit 4, solid).
    fn initial(m: M) -> State {
        State {
            m,
            fill: Some([0, 0, 0, 255]),
            fill_grad: None,
            color: [0, 0, 0, 255],
            fill_opacity: 1.0,
            even_odd: false,
            opacity: 1.0,
            stroke: None,
            stroke_grad: None,
            stroke_width: 1.0,
            stroke_opacity: 1.0,
            cap: Cap::Flat,
            join: Join::SvgMiter,
            miter_limit: 4.0,
            dash: Vec::new(),
            dash_offset: 0.0,
            pen_off: false,
            font: glyphs::FontStyle::initial(),
        }
    }
}

pub struct Doc {
    root: Elem,
    grads: HashMap<String, Grad>,
    classes: HashMap<String, Vec<(String, String)>>,
    width: f64,
    height: f64,
    view_box: Option<[f64; 4]>,
}

impl Doc {
    pub fn load(path: &str) -> std::io::Result<Doc> {
        Doc::load_styled(path, None)
    }

    /// `load` with KIconLoader's recoloring: the text of `<style id="current-color-scheme">`
    /// is replaced by `scheme` (KIconLoaderPrivate::processSvg).
    pub fn load_styled(path: &str, scheme: Option<&str>) -> std::io::Result<Doc> {
        Doc::from_data(&std::fs::read(path)?, scheme)
    }

    /// `load_styled` for a file already in memory. Gzip data (`.svgz`) is inflated first, as
    /// QSvgRenderer::load(QByteArray) does on the gzip magic number.
    pub fn from_data(data: &[u8], scheme: Option<&str>) -> std::io::Result<Doc> {
        let inflated;
        let data = if is_gzip(data) {
            inflated = inflate(data)?;
            &inflated[..]
        } else {
            data
        };
        let text = std::str::from_utf8(data).map_err(std::io::Error::other)?;
        let mut src = text.strip_prefix('\u{feff}').unwrap_or(text).to_string();
        if let Some(css) = scheme {
            src = replace_scheme_style(&src, css);
        }
        let root = parse_xml(&src)
            .filter(|e| e.name == "svg")
            .ok_or_else(|| std::io::Error::other("not an svg document"))?;
        let view_box = root
            .attr("viewBox")
            .map(nums)
            .filter(|v| v.len() == 4)
            .map(|v| [v[0], v[1], v[2], v[3]]);
        let len = |k: &str, d: f64| {
            root.attr(k)
                .filter(|v| !v.ends_with('%'))
                .map(num_prefix)
                .unwrap_or(d)
        };
        let width = len("width", view_box.map_or(100.0, |v| v[2]));
        let height = len("height", view_box.map_or(100.0, |v| v[3]));
        let classes = class_rules(&src);
        let grads = collect_grads(&root, &classes);
        Ok(Doc {
            root,
            grads,
            classes,
            width,
            height,
            view_box,
        })
    }

    /// QSvgRenderer::defaultSize (document size truncated to int); a negative or NaN length is 0.
    pub fn default_size(&self) -> (i32, i32) {
        (self.width.max(0.0) as i32, self.height.max(0.0) as i32)
    }

    /// Rasterises the document at `w`x`h`. `Err` when the target is over the decode limit
    /// ([`crate::image::check_size`], as QImageReader refuses it for QSvgIOHandler too) or the
    /// document runs out of the per-render [`Walk`] budget.
    pub fn render(&self, w: usize, h: usize) -> std::io::Result<Canvas> {
        crate::image::check_size(w, h)?;
        let mut ctx = Context::new(w, h);
        let (tw, th) = (w as f64, h as f64);
        let m = match self.view_box {
            None => {
                let (sx, sy) = (tw / self.width, th / self.height);
                [sx, 0.0, 0.0, sy, 0.0, 0.0]
            }
            // QSvgRenderer::aspectRatioMode defaults to IgnoreAspectRatio: viewBox is stretched.
            Some([vx, vy, vw, vh]) => {
                if [vx, vy, vw, vh] == [0.0, 0.0, tw, th] {
                    ID
                } else {
                    let (sx, sy) = (tw / vw, th / vh);
                    [sx, 0.0, 0.0, sy, -vx * sx, -vy * sy]
                }
            }
        };
        let st = State::initial(m);
        let mut ids = HashMap::new();
        collect_ids(&self.root, &mut ids);
        let walk = Walk {
            view_box: self.view_box.unwrap_or([0.0, 0.0, self.width, self.height]),
            ..Walk::default()
        };
        draw(
            &mut ctx,
            (&self.grads, &self.classes, &ids, &walk),
            &self.root,
            &st,
            true,
        );
        if walk.exhausted.get() {
            return Err(std::io::Error::other(
                "svg: element reference or mask layer budget exhausted",
            ));
        }
        Ok(ctx.img)
    }
}

/// The gzip magic number QSvgDocument tests for (an `.svgz` file).
pub fn is_gzip(data: &[u8]) -> bool {
    data.starts_with(&[0x1f, 0x8b])
}

/// Gzip `data` inflated; `Err` for a broken stream or one that inflates to more than the
/// decode limit ([`crate::image::MAX_DECODED_BYTES`]).
fn inflate(data: &[u8]) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let limit = crate::image::MAX_DECODED_BYTES;
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(data)
        .take(limit as u64 + 1)
        .read_to_end(&mut out)?;
    if out.len() > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::OutOfMemory,
            "svgz inflates to more than the decode limit",
        ));
    }
    Ok(out)
}

/// QSvgDocument::isLikelySvg, which QSvgIOHandler::canRead answers QImageReader with: the
/// first 4096 bytes (inflated when they are gzip) hold an SVG header. Whatever the file is
/// called.
pub fn is_svg(data: &[u8]) -> bool {
    const PEEK: usize = 4096;
    let head = &data[..data.len().min(PEEK)];
    if head.len() < 8 {
        return false;
    }
    if !is_gzip(head) {
        return has_svg_header(head);
    }
    // zlib gives what it inflated before the cut end of the peeked bytes.
    let mut out = Vec::new();
    let mut dec = flate2::read::GzDecoder::new(head);
    let mut buf = [0u8; PEEK];
    while out.len() < PEEK {
        match std::io::Read::read(&mut dec, &mut buf) {
            Ok(n) if n > 0 => out.extend_from_slice(&buf[..n]),
            _ => break,
        }
    }
    out.truncate(PEEK);
    out.len() >= 8 && has_svg_header(&out)
}

/// qsvgdocument.cpp's hasSvgHeader.
fn has_svg_header(head: &[u8]) -> bool {
    let text = String::from_utf8_lossy(head);
    let t = text.trim_start_matches('\u{feff}').trim();
    if t.starts_with("<svg") || t.starts_with("<!DOCTYPE svg") {
        return true;
    }
    (t.starts_with("<?xml") || t.starts_with("<!--"))
        && (t.contains("<!DOCTYPE svg") || t.contains("<svg"))
}

/// librsvg's MAX_REFERENCED_ELEMENTS (rsvg/src/limits.rs): `<use>` and `mask` dereferences one
/// render may make, so fan-out ("billion laughs") documents stop instead of running for hours.
const MAX_REFERENCES: u32 = 500_000;
/// Nesting of `<use>` inside the content another `<use>` draws.
const MAX_USE_DEPTH: u32 = 16;
/// Buffers and mask images (filters, masks, group opacity): their total pixels per render.
const MAX_LAYER_PIXELS: usize = 1 << 30;

/// Per-render traversal state: the work budget and the masks whose content is being drawn.
#[derive(Default)]
struct Walk {
    references: Cell<u32>,
    layer_pixels: Cell<usize>,
    use_depth: Cell<u32>,
    /// A mask referenced from its own content (directly or through other masks) is ignored
    /// there, so a recursive mask terminates instead of overflowing the stack.
    masks: RefCell<Vec<*const Elem>>,
    exhausted: Cell<bool>,
    /// QSvgDocument::viewBox, what the default filter region and mask percentages refer to.
    view_box: RectF,
}

impl Walk {
    /// Takes one reference (and `pixels` of layers) from the budget; `false` once it is spent.
    fn take(&self, pixels: usize) -> bool {
        let refs = self.references.get() + 1;
        let px = self.layer_pixels.get().saturating_add(pixels);
        if self.exhausted.get() || refs > MAX_REFERENCES || px > MAX_LAYER_PIXELS {
            self.exhausted.set(true);
            return false;
        }
        self.references.set(refs);
        self.layer_pixels.set(px);
        true
    }

    fn mask_active(&self, mask: &Elem) -> bool {
        self.masks.borrow().contains(&(mask as *const Elem))
    }
}

type Defs<'a> = (
    &'a HashMap<String, Grad>,
    &'a HashMap<String, Vec<(String, String)>>,
    &'a HashMap<String, &'a Elem>,
    &'a Walk,
);

/// Elements by `id` (first one wins), for `mask` and `use` references.
fn collect_ids<'a>(e: &'a Elem, out: &mut HashMap<String, &'a Elem>) {
    if let Some(id) = e.attr("id") {
        out.entry(id.to_string()).or_insert(e);
    }
    for c in &e.children {
        collect_ids(c, out);
    }
}

/// What QSvgNode::applyStyle leaves on the painter for an element, and what the element refers to.
struct Styled {
    s: State,
    /// `display: none`.
    hidden: bool,
    mask: Option<String>,
    filter: Option<String>,
}

fn styled(defs: Defs, e: &Elem, parent: &State, is_root: bool) -> Styled {
    let gs = defs.0;
    let mut hidden = false;
    let mut s = parent.clone();
    let mut props = e.attrs.clone();
    for c in e.attr("class").unwrap_or("").split_whitespace() {
        if let Some(r) = defs.1.get(c) {
            props.extend(r.iter().cloned());
        }
    }
    if let Some(style) = e.attr("style") {
        props.extend(decls(style));
    }
    if let Some((_, v)) = props.iter().rev().find(|(k, _)| k == "color")
        && let Some(Some(c)) = parse_color(v)
    {
        s.color = c;
    }
    for (k, v) in &props {
        match k.as_str() {
            "fill" => {
                if let Some(id) = url_id(v) {
                    s.fill = None;
                    s.fill_grad = gs.contains_key(&id).then_some(id);
                } else if v.trim() == "currentColor" {
                    s.fill = Some(s.color);
                    s.fill_grad = None;
                } else if let Some(c) = parse_color(v) {
                    s.fill = c;
                    s.fill_grad = None;
                }
            }
            "fill-opacity" => s.fill_opacity = num_prefix(v).clamp(0.0, 1.0),
            "fill-rule" => s.even_odd = v == "evenodd",
            "opacity" => s.opacity = parent.opacity * num_prefix(v).clamp(0.0, 1.0),
            "stroke" => {
                s.stroke_grad = None;
                if let Some(id) = url_id(v) {
                    // The pen's brush is the gradient; the colour only says that there is a pen.
                    s.stroke = gs.contains_key(&id).then_some([0, 0, 0, 255]);
                    s.stroke_grad = s.stroke.is_some().then_some(id);
                } else if v.trim() == "currentColor" {
                    s.stroke = Some(s.color);
                } else if let Some(c) = parse_color(v) {
                    s.stroke = c;
                }
            }
            "stroke-width" => s.stroke_width = num_prefix(v),
            "stroke-opacity" => s.stroke_opacity = num_prefix(v).clamp(0.0, 1.0),
            "stroke-linecap" => match v.trim() {
                "butt" => s.cap = Cap::Flat,
                "square" => s.cap = Cap::Square,
                "round" => s.cap = Cap::Round,
                _ => {}
            },
            "stroke-linejoin" => match v.trim() {
                "miter" => s.join = Join::SvgMiter,
                "round" => s.join = Join::Round,
                "bevel" => s.join = Join::Bevel,
                _ => {}
            },
            "stroke-miterlimit" => s.miter_limit = num_prefix(v),
            "stroke-dasharray" if v != "inherit" => {
                s.dash = parse_dashes(v);
                // QSvgStrokeStyle::apply: setStyle(SolidLine) or setDashPattern, either of
                // which makes a pen of a Qt::NoPen.
                s.pen_off = false;
            }
            "stroke-dashoffset" => {
                s.dash_offset = num_prefix(v);
                // QPen::setDashOffset makes any pen that is not solid a CustomDashLine.
                s.pen_off &= s.dash_offset == 0.0;
            }
            "display" if v == "none" => hidden = true,
            _ => s.font.set(k, v),
        }
    }
    let url = |k: &str| {
        props
            .iter()
            .rev()
            .find(|(a, _)| a == k)
            .and_then(|(_, v)| url_id(v))
    };
    if !is_root && let Some(t) = e.attr("transform") {
        s.m = compose(parse_transform(t), s.m);
    }
    Styled {
        s,
        hidden,
        mask: url("mask"),
        filter: url("filter"),
    }
}

/// The outline of a shape element in its user space.
fn shape_path(e: &Elem) -> Option<UPath> {
    let g = |k: &str| e.attr(k).map(num_prefix).unwrap_or(0.0);
    match e.name.as_str() {
        "path" => e.attr("d").map(parse_path),
        "rect" => {
            let (x, y, w, h) = (g("x"), g("y"), g("width"), g("height"));
            let (rx, ry) = match (e.attr("rx").map(num_prefix), e.attr("ry").map(num_prefix)) {
                (Some(a), Some(b)) => (a, b),
                (Some(a), None) | (None, Some(a)) => (a, a),
                (None, None) => (0.0, 0.0),
            };
            let (rx, ry) = (rx.clamp(0.0, w / 2.0), ry.clamp(0.0, h / 2.0));
            if rx > 0.0 && ry > 0.0 {
                Some(rounded_rect(x, y, w, h, rx, ry))
            } else {
                Some(vec![
                    El::Move([x, y]),
                    El::Line([x + w, y]),
                    El::Line([x + w, y + h]),
                    El::Line([x, y + h]),
                    El::Line([x, y]),
                ])
            }
        }
        "circle" => {
            let r = g("r");
            Some(ellipse(g("cx") - r, g("cy") - r, 2.0 * r, 2.0 * r))
        }
        "ellipse" => {
            let (rx, ry) = (g("rx"), g("ry"));
            Some(ellipse(g("cx") - rx, g("cy") - ry, 2.0 * rx, 2.0 * ry))
        }
        "line" => Some(vec![
            El::Move([g("x1"), g("y1")]),
            El::Line([g("x2"), g("y2")]),
        ]),
        "polygon" | "polyline" => e.attr("points").map(|v| {
            let n = nums(v);
            let mut p: UPath = n
                .chunks(2)
                .filter(|c| c.len() == 2)
                .enumerate()
                .map(|(i, c)| {
                    if i == 0 {
                        El::Move([c[0], c[1]])
                    } else {
                        El::Line([c[0], c[1]])
                    }
                })
                .collect();
            if e.name == "polygon" && n.len() >= 2 {
                p.push(El::Line([n[0], n[1]]));
            }
            p
        }),
        _ => None,
    }
}

/// QSvgNode::bounds: the bounding box of a node under the painter state of its parent.
fn node_bounds(defs: Defs, e: &Elem, parent: &State, decorated: bool) -> RectF {
    if kind(&e.name) == Kind::None {
        return [0.0; 4];
    }
    let st = styled(defs, e, parent, false);
    internal_bounds(defs, e, &st, decorated)
}

/// QSvgNode::internalBounds of each node type, or decoratedInternalBounds: with the stroke's
/// joins as the pen has them and the region of the node's filter.
fn internal_bounds(defs: Defs, e: &Elem, st: &Styled, decorated: bool) -> RectF {
    let (s, walk) = (&st.s, defs.3);
    let g = |k: &str| e.attr(k).map(num_prefix).unwrap_or(0.0);
    let rect = match e.name.as_str() {
        "use" => {
            let Some(target) = use_target(defs, e) else {
                return [0.0; 4];
            };
            if walk.use_depth.get() >= MAX_USE_DEPTH || !walk.take(0) {
                return [0.0; 4];
            }
            walk.use_depth.set(walk.use_depth.get() + 1);
            let inner = State {
                m: compose([1.0, 0.0, 0.0, 1.0, g("x"), g("y")], s.m),
                ..s.clone()
            };
            let r = node_bounds(defs, target, &inner, decorated);
            walk.use_depth.set(walk.use_depth.get() - 1);
            return r;
        }
        "image" => map_rect(&s.m, [g("x"), g("y"), g("width"), g("height")]),
        "text" => glyphs::bounds(defs, e, s),
        _ if kind(&e.name) == Kind::Container => {
            return e.children.iter().fold([0.0; 4], |acc, c| {
                united(acc, node_bounds(defs, c, s, decorated))
            });
        }
        _ => {
            let Some(up) = shape_path(e) else {
                return [0.0; 4];
            };
            let path = Path::from_el(up);
            let width = if s.stroke.is_some() {
                s.stroke_width
            } else {
                0.0
            };
            // boundsOnStroke: a QPainterPathStroker as constructed (square caps, bevel joins).
            let outline = if width.abs() <= 1e-12 {
                path
            } else if decorated {
                stroke_outline(&path, width, Cap::Square, s.join, s.miter_limit)
            } else {
                stroke_outline(&path, width, Cap::Square, Join::Bevel, 2.0)
            };
            path_bounds(&outline.mapped(s.m).el).unwrap_or([0.0; 4])
        }
    };
    match filter_of(defs, st) {
        Some(f) if decorated => f.region(rect),
        _ => rect,
    }
}

fn filter_of(defs: Defs, st: &Styled) -> Option<Filter> {
    let node = defs.2.get(st.filter.as_ref()?)?;
    (node.name == "filter")
        .then(|| Filter::parse(node, defs.3.view_box))
        .flatten()
}

/// QSvgNode::drawIntoBuffer: the node drawn by a fresh painter (opacity 1) into an image that
/// covers the device rectangle `rect`.
fn draw_into_buffer(defs: Defs, e: &Elem, s: &State, rect: IRect) -> Option<Buf> {
    let mut buf = Buf::new(rect)?;
    if !defs.3.take(buf.px.len()) {
        return None;
    }
    let inner = State {
        m: compose(s.m, [1.0, 0.0, 0.0, 1.0, -rect.x as f64, -rect.y as f64]),
        opacity: 1.0,
        ..s.clone()
    };
    let mut c = Context::new(rect.w as usize, rect.h as usize);
    draw_command(&mut c, defs, e, &inner);
    buf.px = c.img.px;
    Some(buf)
}

/// QSvgNode::applyBufferToCanvas: the image drawn at its offset under the painter's opacity.
fn blit(ctx: &mut Context, buf: &Buf, opacity: f64) {
    let coverage = (255 * (opacity * 256.0) as u32) >> 8;
    let (w, h) = (ctx.img.w as i32, ctx.img.h as i32);
    let r = buf.rect;
    for y in r.y.max(0)..(r.y + r.h).min(h) {
        for x in r.x.max(0)..(r.x + r.w).min(w) {
            let s = buf.px[((y - r.y) * r.w + x - r.x) as usize];
            gradient::source_over(&mut ctx.img.px[(y * w + x) as usize], s, coverage);
        }
    }
}

/// QSvgMask::createMask for a target drawn under `m` whose bounding box in user space is
/// `local`: what the mask takes away from each pixel of the device rectangle `rect`.
fn create_mask(
    defs: Defs,
    node: &Elem,
    mask: &Mask,
    m: &M,
    local: RectF,
    rect: IRect,
) -> Option<Vec<u8>> {
    let walk = defs.3;
    let st = styled(defs, node, &State::initial(ID), false);
    // A mask on the mask stands in for it.
    if let Some(other) = st.mask.as_ref().and_then(|id| defs.2.get(id))
        && other.name == "mask"
        && !walk.mask_active(other)
        && let Some(other_mask) = Mask::parse(other, walk.view_box)
    {
        walk.masks.borrow_mut().push(node);
        let alpha = create_mask(defs, other, &other_mask, m, local, rect);
        walk.masks.borrow_mut().pop();
        return alpha;
    }
    if rect.is_empty() || !walk.take(rect.w as usize * rect.h as usize) {
        return None;
    }
    crate::image::check_size(rect.w as usize, rect.h as usize).ok()?;
    // The mask's own transform does not count, only where the target is.
    let device = compose(*m, [1.0, 0.0, 0.0, 1.0, -rect.x as f64, -rect.y as f64]);
    let mut s = st.s;
    s.m = if mask.content_obb {
        compose([local[2], 0.0, 0.0, local[3], local[0], local[1]], device)
    } else {
        device
    };
    let mut c = Context::new(rect.w as usize, rect.h as usize);
    walk.masks.borrow_mut().push(node);
    for child in &node.children {
        draw(&mut c, defs, child, &s, false);
    }
    walk.masks.borrow_mut().pop();
    let clip = mask.rect.resolve(local, Units::Unknown);
    let size = IRect { x: 0, y: 0, ..rect };
    Some(effects::mask_alpha(&c.img.px, size, &device, clip))
}

/// QSvgNode::draw: a node with a filter, a mask or (a group under) opacity is drawn into a
/// buffer first, anything else straight onto the canvas.
fn draw(ctx: &mut Context, defs: Defs, e: &Elem, parent: &State, is_root: bool) {
    let walk = defs.3;
    if walk.exhausted.get() {
        return;
    }
    // QSvgDefs, QSvgMask, QSvgPattern and QSvgFilterContainer are never drawn where they stand,
    // QSvgSymbol and QSvgMarker only from the node that uses them.
    if !is_root
        && (kind(&e.name) == Kind::None
            || matches!(
                e.name.as_str(),
                "defs" | "mask" | "symbol" | "pattern" | "filter" | "marker"
            ))
    {
        return;
    }
    let st = styled(defs, e, parent, is_root);
    if st.hidden {
        return;
    }
    let s = &st.s;
    if is_root {
        return draw_command(ctx, defs, e, s);
    }
    let mask = st
        .mask
        .as_ref()
        .and_then(|id| defs.2.get(id))
        .filter(|n| n.name == "mask" && !walk.mask_active(n))
        .and_then(|n| Some((*n, Mask::parse(n, walk.view_box)?)));
    let local = |decorated: bool| {
        let unmapped = Styled {
            s: State { m: ID, ..s.clone() },
            hidden: false,
            mask: None,
            filter: st.filter.clone(),
        };
        internal_bounds(defs, e, &unmapped, decorated)
    };
    if let Some(f) = filter_of(defs, &st) {
        let item = local(false);
        let rect = to_rect(map_rect(&s.m, f.region(item)));
        let Some(out) = f.apply(draw_into_buffer(defs, e, s, rect), &s.m, item) else {
            return;
        };
        let mut out = Rc::unwrap_or_clone(out);
        if let Some((node, mask)) = mask
            && let Some(inv) = invert(&s.m)
        {
            let rect = out.rect;
            let item = map_rect(&inv, rect.to_f());
            if let Some(alpha) = create_mask(defs, node, &mask, &s.m, item, rect) {
                out.mask_out(&alpha);
            }
        }
        blit(ctx, &out, s.opacity);
    } else if let Some((node, mask)) = mask {
        let item = local(false);
        let rect = to_aligned_rect(map_rect(&s.m, item));
        let Some(alpha) = create_mask(defs, node, &mask, &s.m, item, rect) else {
            return;
        };
        if let Some(mut buf) = draw_into_buffer(defs, e, s, rect) {
            buf.mask_out(&alpha);
            blit(ctx, &buf, s.opacity);
        }
    } else if (s.opacity - 1.0).abs() > 1e-12 && group_rendering(e) {
        let b = map_rect(&s.m, local(true));
        // "adding safety border needed because of the antialiazing effects"
        let (dx, dy) = ((b[2] * 0.1) as i32 as f64, (b[3] * 0.1) as i32 as f64);
        let rect = to_aligned_rect([b[0] - dx, b[1] - dy, b[2] + 2.0 * dx, b[3] + 2.0 * dy]);
        if let Some(buf) = draw_into_buffer(defs, e, s, rect) {
            blit(ctx, &buf, s.opacity);
        }
    } else {
        draw_command(ctx, defs, e, s);
    }
}

/// QSvgG::requiresGroupRendering: a group of more than one node.
fn group_rendering(e: &Elem) -> bool {
    e.name == "g"
        && e.children
            .iter()
            .filter(|c| kind(&c.name) != Kind::None)
            .count()
            > 1
}

fn invert(m: &M) -> Option<M> {
    let det = m[0] * m[3] - m[1] * m[2];
    if det.abs() < 1e-12 {
        return None;
    }
    let (a, b, c, d) = (m[3] / det, -m[1] / det, -m[2] / det, m[0] / det);
    Some([a, b, c, d, -(a * m[4] + c * m[5]), -(b * m[4] + d * m[5])])
}

/// The node's own drawing (drawCommand, or fillThenStroke) and, for a group, its children.
fn draw_command(ctx: &mut Context, defs: Defs, e: &Elem, s: &State) {
    let gs = defs.0;
    let g = |k: &str| e.attr(k).map(num_prefix).unwrap_or(0.0);
    if e.name == "use" {
        return draw_use(ctx, defs, e, s.clone(), g("x"), g("y"));
    }
    if e.name == "image" {
        // createImageNode: `xlink:href`, else `href`; no node without a size or a picture.
        let href = e.attr("xlink:href").filter(|h| !h.is_empty());
        let img = href.or_else(|| e.attr("href")).and_then(picture::load);
        let r = [g("x"), g("y"), g("width"), g("height")];
        if let Some(img) = img.filter(|_| r[2] > 0.0 && r[3] > 0.0) {
            picture::draw(ctx, &img, r, s.m, s.opacity, true);
        }
        return;
    }
    if e.name == "text" {
        // QSvgNode::fillThenStroke (QSvgText::separateFillStroke): once with the brush alone
        // under the fill opacity, once with the pen alone under the stroke opacity.
        if s.fill.is_some() || s.fill_grad.is_some() {
            let mut pass = s.clone();
            // QPen(Qt::NoPen).
            pass.pen_off = true;
            (pass.stroke, pass.stroke_grad) = (Some([0, 0, 0, 255]), None);
            (pass.stroke_width, pass.miter_limit) = (1.0, 2.0);
            (pass.cap, pass.join) = (Cap::Square, Join::Bevel);
            pass.dash = Vec::new();
            (pass.opacity, pass.fill_opacity) = (s.opacity * s.fill_opacity, 1.0);
            glyphs::draw(ctx, defs, e, &pass);
        }
        if s.stroke.is_some() && s.stroke_width != 0.0 {
            let mut pass = s.clone();
            (pass.fill, pass.fill_grad) = (None, None);
            (pass.opacity, pass.stroke_opacity) = (s.opacity * s.stroke_opacity, 1.0);
            glyphs::draw(ctx, defs, e, &pass);
        }
        return;
    }
    // QSvgRect::drawCommand: drawRect unless a radius is set.
    let radius = |k: &str| e.attr(k).map(num_prefix).is_some_and(|r| r != 0.0);
    let plain = (e.name == "rect" && !radius("rx") && !radius("ry"))
        .then(|| [g("x"), g("y"), g("width"), g("height")]);
    let shape = if plain.is_some() && s.stroke.is_none() && s.fill_grad.is_none() {
        if let Some(c) = s.fill {
            let op = s.opacity * s.fill_opacity;
            let r = [g("x"), g("y"), g("width"), g("height")];
            ctx.fill_rect_xf(r, c, (op * 256.0) as u32, s.m);
        }
        None
    } else {
        shape_path(e)
    };
    if let Some(up) = shape {
        let stroking = s.stroke.is_some() && s.stroke_width > 0.0;
        if let Some(g) = s.fill_grad.as_ref().and_then(|id| gs.get(id)) {
            g.fill(ctx, &up, plain, s.m, s.even_odd, s.opacity * s.fill_opacity);
        }
        // The brush keeps its colour; the painter's opacity is the node's times fill-opacity,
        // in QSvgNode::fillThenStroke and in QSvgPath::drawCommand's one pass alike.
        if let Some(c) = s.fill {
            ctx.fill_opacity(
                &Path::from_el(up.clone()),
                c,
                s.even_odd,
                (s.opacity * s.fill_opacity * 256.0) as u32,
                Some(s.m),
            );
        }
        if stroking {
            stroke_path(ctx, defs, &up, s);
        }
    }
    for c in &e.children {
        draw(ctx, defs, c, s, false);
    }
}

/// The pen of `s` along `up` (user space).
fn stroke_path(ctx: &mut Context, defs: Defs, up: &UPath, s: &State) {
    let Some(c) = s.stroke else {
        return;
    };
    // QPen keeps the dash pattern and its offset in pen widths (parsePen divides them).
    let dash: Vec<f64> = s.dash.iter().map(|d| d / s.stroke_width).collect();
    let offset = s.dash_offset / s.stroke_width;
    let user = Path::from_el(up.clone());
    let pen = |ctx: &mut Context, c: [u8; 4], int_opacity: u32| {
        ctx.stroke_xf_opacity(
            &user,
            c,
            s.stroke_width,
            s.cap,
            s.join,
            s.miter_limit,
            &dash,
            offset,
            s.m,
            int_opacity,
        );
    };
    if let Some(g) = s.stroke_grad.as_ref().and_then(|id| defs.0.get(id)) {
        let mut mask = Context::new(ctx.img.w, ctx.img.h);
        pen(&mut mask, [255; 4], 256);
        g.paint(ctx, &mask, up, s.m, s.stroke_opacity * s.opacity);
    } else {
        // The pen keeps its colour; the painter's opacity is the node's times stroke-opacity
        // (QSvgNode::fillThenStroke), QPainterState::intOpacity of it.
        pen(ctx, c, (s.opacity * s.stroke_opacity * 256.0) as u32);
    }
}

fn use_target<'a>(defs: Defs<'a>, e: &Elem) -> Option<&'a Elem> {
    e.attr("xlink:href")
        .or_else(|| e.attr("href"))
        .and_then(|v| v.trim().strip_prefix('#'))
        .and_then(|id| defs.2.get(id))
        .copied()
}

/// QSvgUse: the referenced node is drawn translated by x/y, inheriting the `use` element's style.
fn draw_use(ctx: &mut Context, defs: Defs, e: &Elem, mut s: State, x: f64, y: f64) {
    let walk = defs.3;
    let Some(target) = use_target(defs, e) else {
        return;
    };
    if walk.use_depth.get() >= MAX_USE_DEPTH || !walk.take(0) {
        return;
    }
    walk.use_depth.set(walk.use_depth.get() + 1);
    s.m = compose([1.0, 0.0, 0.0, 1.0, x, y], s.m);
    if matches!(target.name.as_str(), "symbol" | "svg") {
        for c in &target.children {
            draw(ctx, defs, c, &s, false);
        }
    } else {
        draw(ctx, defs, target, &s, false);
    }
    walk.use_depth.set(walk.use_depth.get() - 1);
}

/// QQuickImage load size for a vector source (QImageReader::setScaledSize is honoured exactly).
pub fn load_size(dw: i32, dh: i32, rw: i32, rh: i32, fill: FillMode) -> (usize, usize) {
    let (fw, fh) = (dw as f64, dh as f64);
    let r = |v: f64| (v + 0.5).floor() as usize;
    match (rw > 0, rh > 0) {
        (false, false) => (dw as usize, dh as usize),
        (true, false) => (rw as usize, r(fh * rw as f64 / fw)),
        (false, true) => (r(fw * rh as f64 / fh), rh as usize),
        (true, true) => {
            if matches!(fill, FillMode::Stretch) {
                (rw as usize, rh as usize)
            } else {
                let k = (rw as f64 / fw).max(rh as f64 / fh);
                (r(fw * k), r(fh * k))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn use_inherits_fill_and_offset() {
        let p = std::env::temp_dir().join(format!("ri-svg-use-{}.svg", std::process::id()));
        std::fs::write(
            &p,
            r##"<svg width="20" height="10" xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink">
 <defs><path id="c" d="m0 0h10v10h-10z"/></defs>
 <use fill="#ff0000" xlink:href="#c"/>
 <use fill="#0000ff" x="10" href="#c"/>
</svg>"##,
        )
        .unwrap();
        let img = Doc::load(p.to_str().unwrap())
            .unwrap()
            .render(20, 10)
            .unwrap();
        std::fs::remove_file(&p).ok();
        assert_eq!(img.px[5 * 20 + 5], [255, 0, 0, 255]);
        assert_eq!(img.px[5 * 20 + 15], [0, 0, 255, 255]);
    }

    fn draw(body: &str) -> Canvas {
        let src = format!(
            r##"<svg width="20" height="20" xmlns="http://www.w3.org/2000/svg" fill="none" stroke="#000" stroke-width="4">{body}</svg>"##
        );
        Doc::from_data(src.as_bytes(), None)
            .unwrap()
            .render(20, 20)
            .unwrap()
    }

    /// The pixel values here are QtSvg's (tests/svg_stroke_qt.rs compares whole images).
    #[test]
    fn line_and_stroke_properties_draw() {
        let at = |c: &Canvas, x: usize, y: usize| c.px[y * 20 + x][3];
        let line = draw(r#"<line x1="6" y1="10" x2="14" y2="10"/>"#);
        assert_eq!((at(&line, 10, 10), at(&line, 4, 10)), (255, 0));
        // Caps reach half the width past the ends: square fills the corner, round does not.
        let square = draw(r#"<line x1="6" y1="10" x2="14" y2="10" stroke-linecap="square"/>"#);
        assert_eq!((at(&square, 4, 10), at(&square, 4, 8)), (255, 255));
        let round = draw(r#"<line x1="6" y1="10" x2="14" y2="10" stroke-linecap="round"/>"#);
        assert_eq!((at(&round, 4, 10), at(&round, 4, 8)), (232, 76));
        // The outer corner of a right angle: a miter fills it, a bevel and a round join cut it.
        let corner = |join: &str| {
            let c = draw(&format!(
                r#"<path d="M4 16V6H16" stroke-linejoin="{join}"/>"#
            ));
            at(&c, 2, 4)
        };
        assert_eq!(corner("miter"), 255);
        assert_eq!(corner("bevel"), 0);
        assert_eq!(corner("round"), 76);
        // A sharp angle is a miter only within the limit.
        let tip = |limit: &str| {
            let c = draw(&format!(
                r#"<path d="M4 18L10 8L16 18" stroke-miterlimit="{limit}"/>"#
            ));
            at(&c, 10, 5)
        };
        assert_eq!((tip("1"), tip("10")), (0, 210));
        // Dashes of 4 and gaps of 4, then shifted by 4.
        let dash = draw(r#"<path d="M2 10H18" stroke-dasharray="4"/>"#);
        assert_eq!((at(&dash, 3, 10), at(&dash, 7, 10)), (255, 0));
        let shifted = draw(r#"<path d="M2 10H18" stroke-dasharray="4" stroke-dashoffset="4"/>"#);
        assert_eq!((at(&shifted, 3, 10), at(&shifted, 7, 10)), (0, 255));
    }

    #[test]
    fn svg_is_recognized_by_content_and_read_compressed() {
        use std::io::Write;
        let src = r##"<svg width="4" height="4" xmlns="http://www.w3.org/2000/svg"><rect width="4" height="4" fill="#ff0000"/></svg>"##;
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(src.as_bytes()).unwrap();
        let gz = gz.finish().unwrap();
        assert!(is_gzip(&gz) && !is_gzip(src.as_bytes()));
        for data in [
            src.as_bytes().to_vec(),
            gz,
            format!("\u{feff}<?xml version=\"1.0\"?>\n<!-- c -->\n{src}").into_bytes(),
        ] {
            assert!(is_svg(&data));
            let img = Doc::from_data(&data, None).unwrap().render(4, 4).unwrap();
            assert_eq!(img.px[5], [255, 0, 0, 255]);
        }
        assert!(!is_svg(b"\x89PNG\r\n\x1a\n"));
        assert!(!is_svg(b"/* XPM */\nstatic char *x[] = {"));
        assert!(Doc::from_data(b"<html/>", None).is_err());
    }
}
