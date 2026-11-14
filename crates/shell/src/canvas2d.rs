//! QQuickContext2DCommandBuffer::replay: the commands a Canvas' context recorded, painted by
//! `qsg::painter::Painter` as Qt's replay paints them with QPainter, and what the context
//! asks of the renderer while JS runs (text metrics, images, pixels).

use crate::scene::Fonts;
use qml::context2d::{
    CanvasBrush, CanvasFont, CanvasHost, CanvasImage, CanvasOp, PathOp, PenDash, RAW_TEXT,
};
use qsg::canvas::{Cap, Join, Path};
use qsg::painter::{Brush, Painter, Pen, box_blur};
use qsg::pathsimplify::PathEl;
use qsg::raster::Canvas;
use qsg::text::FontStack;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

thread_local! {
    static FONTS: RefCell<Fonts> = RefCell::new(Fonts::default());
}

fn qround(v: f64) -> i64 {
    if v >= 0.0 {
        (v + 0.5) as i64
    } else {
        (v - 0.5) as i64
    }
}

/// The QFont's engine: pixel size, or point size at 96 dpi.
fn font_stack(f: &CanvasFont) -> Option<Rc<FontStack>> {
    let px = if f.pixel_size > 0 {
        f.pixel_size as f64
    } else {
        f.point_size as f64 * 96.0 / 72.0
    };
    let style = match f.style {
        0 => "",
        1 => "Italic",
        _ => "Oblique",
    };
    FONTS.with(|c| {
        c.borrow_mut().get_styled(
            &f.family,
            style,
            qround(px).max(1) as u32,
            f.weight,
            &[],
            (0, 0.0),
        )
    })
}

/// QFontMetrics: ascent(), height() and horizontalAdvance(text).
fn metrics(stack: &FontStack, text: &str) -> (i64, i64, i64) {
    let (asc, desc) = stack
        .font(0)
        .map(|f| (f.ascent, f.descent))
        .unwrap_or((0, 0));
    let r = |v: i64| qround(v as f64 / 64.0);
    (r(asc), r(asc) + r(desc), r(stack.shape(text).advance))
}

/// QPainterPath::addText(x, y, font, text): the glyph outlines of one line.
fn add_text(path: &mut Path, stack: &FontStack, text: &str, x: f64, y: f64) {
    if text.is_empty() {
        return;
    }
    // QFixed::fromReal.
    let (ox, oy) = ((x * 64.0) as i64, (y * 64.0) as i64);
    let line = stack.shape(text);
    let mut out = Path::default();
    for &(fi, gid, gx, gy) in &line.glyphs {
        let Some(font) = stack.font(fi) else {
            continue;
        };
        let (px, py) = ((ox + gx) as f64 / 64.0, (oy + gy) as f64 / 64.0);
        let mut open = false;
        for el in font.outline(gid) {
            match el {
                PathEl::Move(a, b) => {
                    if open {
                        out.close_subpath();
                    }
                    out.push_move(px + a, py + b);
                    open = true;
                }
                PathEl::Line(a, b) => out.push_line(px + a, py + b),
                PathEl::Cubic(a, b, c, d, e, f) => {
                    out.push_cubic([px + a, py + b], [px + c, py + d], [px + e, py + f])
                }
            }
        }
        if open {
            out.close_subpath();
        }
    }
    path.add_path(&out);
}

/// QQuickContext2D::createTextGlyphs, or `text()` with `align` [`RAW_TEXT`].
fn text_path(
    path: &mut Path,
    text: &str,
    x: f64,
    y: f64,
    font: &CanvasFont,
    align: u8,
    baseline: u8,
) {
    let Some(stack) = font_stack(font) else {
        return;
    };
    if align == RAW_TEXT {
        return add_text(path, &stack, text, x, y);
    }
    let (ascent, height, advance) = metrics(&stack, text);
    let yoffset = match baseline {
        1 | 4 => 0,
        2 => (ascent >> 1) + height - ascent,
        3 => height,
        _ => ascent,
    };
    let xoffset = match align {
        1 | 3 => advance,
        4 => advance / 2,
        _ => 0,
    };
    add_text(
        path,
        &stack,
        text,
        x - xoffset as f64,
        y - yoffset as f64 + ascent as f64,
    );
}

/// QQuickContext2D::m_path after these calls.
pub fn build_path(ops: &[PathOp]) -> Path {
    let mut p = Path::default();
    for op in ops {
        match op {
            PathOp::MoveTo(x, y) => p.move_to(*x, *y),
            PathOp::LineTo(x, y) => p.line_to(*x, *y),
            PathOp::Cubic(c) => p.bezier_curve_to(c[0], c[1], c[2], c[3], c[4], c[5]),
            PathOp::Quad(q) => p.quad_to(q[0], q[1], q[2], q[3]),
            PathOp::Arc(a, ccw) => p.arc(a[0], a[1], a[2], a[3], a[4], *ccw),
            PathOp::ArcTo(a) => p.arc_to_points(a[0], a[1], a[2], a[3], a[4]),
            PathOp::Rect(r) => {
                if r[2] == 0.0 && r[3] == 0.0 {
                    p.move_to(r[0], r[1]);
                } else {
                    p.add_rect(r[0], r[1], r[2], r[3]);
                }
            }
            PathOp::RoundedRect(r) => {
                if r[2] == 0.0 && r[3] == 0.0 {
                    p.move_to(r[0], r[1]);
                } else {
                    p.add_rounded_rect(r[0], r[1], r[2], r[3], r[4], r[5]);
                }
            }
            PathOp::Ellipse(r) => {
                if r[2] == 0.0 && r[3] == 0.0 {
                    p.move_to(r[0], r[1]);
                } else {
                    p.add_ellipse(r[0], r[1], r[2], r[3]);
                }
            }
            PathOp::Text {
                text,
                x,
                y,
                font,
                align,
                baseline,
            } => text_path(&mut p, text, *x, *y, font, *align, *baseline),
            PathOp::Close => p.close_path(),
            PathOp::Map(m) => p = p.mapped(*m),
            PathOp::Svg(d) => p = qsg::svg::path_data(d),
        }
    }
    p
}

fn premultiplied(img: &CanvasImage) -> Canvas {
    let px = img
        .px
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| {
            let a = c[3] as u32;
            // qPremultiply.
            let f = |v: u8| mul255::qt(v as u32, a) as u8;
            [f(c[0]), f(c[1]), f(c[2]), c[3]]
        })
        .collect();
    Canvas::from_px(img.w, img.h, px)
}

fn unpremultiplied(c: &Canvas) -> CanvasImage {
    let mut px = Vec::with_capacity(c.px.len() * 4);
    for p in &c.px {
        let a = p[3] as u32;
        // qUnpremultiply: (c * (0x00ff00ff / alpha) + 0x8000) >> 16.
        let f = |v: u8| {
            if a == 0 {
                0
            } else if a == 255 {
                v
            } else {
                ((v as u32 * (0x00ff_00ff / a) + 0x8000) >> 16) as u8
            }
        };
        px.extend([f(p[0]), f(p[1]), f(p[2]), p[3]]);
    }
    CanvasImage { w: c.w, h: c.h, px }
}

/// QImage::copy(x, y, w, h): pixels outside the image are transparent.
fn copy(img: &Canvas, x: i64, y: i64, w: i64, h: i64) -> Canvas {
    let mut out = Canvas::new(w.max(0) as usize, h.max(0) as usize, [0; 4]);
    for yy in 0..h.max(0) {
        let sy = y + yy;
        if sy < 0 || sy >= img.h as i64 {
            continue;
        }
        for xx in 0..w.max(0) {
            let sx = x + xx;
            if sx >= 0 && sx < img.w as i64 {
                out.px[(yy * w + xx) as usize] = img.px[sy as usize * img.w + sx as usize];
            }
        }
    }
    out
}

/// QImage::scaled(w, h) (IgnoreAspectRatio, FastTransformation): qt_xForm_helper.
fn scaled(img: &Canvas, dw: i64, dh: i64) -> Canvas {
    let mut out = Canvas::new(dw.max(0) as usize, dh.max(0) as usize, [0; 4]);
    if img.w == 0 || img.h == 0 || dw <= 0 || dh <= 0 {
        return out;
    }
    let m11 = (img.w as f64 / dw as f64 * 4096.0) as i64;
    let m22 = (img.h as f64 / dh as f64 * 4096.0) as i64;
    let (maxws, maxhs) = ((img.w as i64) << 12, (img.h as i64) << 12);
    let mut trigy = m22 / 2;
    for y in 0..dh {
        let mut trigx = m11 / 2;
        if trigy >= 0 && trigy < maxhs {
            for x in 0..dw {
                if trigx >= 0 && trigx < maxws {
                    out.px[(y * dw + x) as usize] =
                        img.px[(trigy >> 12) as usize * img.w + (trigx >> 12) as usize];
                }
                trigx += m11;
            }
        }
        trigy += m22;
    }
    out
}

fn cap(k: u8) -> Cap {
    [Cap::Flat, Cap::Square, Cap::Round][(k as usize).min(2)]
}

fn join(k: u8) -> Join {
    [Join::Miter, Join::SvgMiter, Join::Round, Join::Bevel][(k as usize).min(3)]
}

struct Shadow {
    blur: f64,
    color: [u8; 4],
    x: f64,
    y: f64,
}

impl Shadow {
    /// HAS_SHADOW.
    fn on(&self) -> bool {
        self.color[3] != 0 && (self.blur != 0.0 || self.x != 0.0 || self.y != 0.0)
    }
}

/// ShadowImageMaker::paintShapeAndShadow: the shape painted on its own image, blurred and
/// tinted with the shadow colour, drawn under the shape. `bbox` is x, y, w, h in user space.
fn with_shadow(p: &mut Painter, s: &Shadow, bbox: [f64; 4], paint: &dyn Fn(&mut Painter)) {
    let b = 2.0 * s.blur;
    let (l, t) = (bbox[0] + s.x - b, bbox[1] + s.y - b);
    let (r, bt) = (bbox[0] + bbox[2] + s.x + b, bbox[1] + bbox[3] + s.y + b);
    if [l, t, r, bt].iter().all(|v| v.is_finite()) {
        // QRectF::toAlignedRect.
        let (x0, y0, x1, y1) = (
            l.floor() as i64,
            t.floor() as i64,
            r.ceil() as i64,
            bt.ceil() as i64,
        );
        let (w, h) = (x1 - x0, y1 - y0);
        if w > 0 && h > 0 && w * h <= 16_000_000 {
            let mut sp = Painter::new(w as usize, h as usize);
            sp.smooth = p.smooth;
            sp.set_matrix([1.0, 0.0, 0.0, 1.0, s.x - x0 as f64, s.y - y0 as f64]);
            paint(&mut sp);
            if s.blur > 0.0 {
                box_blur(&mut sp.img, qround(s.blur / 2.0).max(1));
            }
            sp.set_matrix([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
            // CompositionMode_SourceIn.
            sp.set_composition_mode(5);
            sp.fill_rect([0.0, 0.0, w as f64, h as f64], &Brush::Solid(s.color));
            p.draw_image(&Rc::new(sp.img), x0 as f64, y0 as f64);
        }
    }
    paint(p);
}

/// Replays `ops` onto a transparent `cw` x `ch` image.
pub fn replay(ops: &[CanvasOp], cw: usize, ch: usize) -> Canvas {
    let mut p = Painter::new(cw, ch);
    let black = Brush::Solid([0, 0, 0, 255]);
    let mut fill = black.clone();
    let mut pen = Pen {
        brush: black,
        width: 1.0,
        cap: Cap::Flat,
        join: Join::Miter,
        miter_limit: 10.0,
        dash: Vec::new(),
        dash_offset: 0.0,
        no_dashes: false,
    };
    let mut shadow = Shadow {
        blur: 0.0,
        color: [0; 4],
        x: 0.0,
        y: 0.0,
    };
    // The state's lineDash and lineDashOffset, the pen's own dash, and the state's clip.
    let (mut dash, mut dash_offset, mut pen_dash) = (Vec::new(), 0.0, PenDash::default());
    let mut clip: Option<(Path, bool)> = None;
    let mut clipping = false;
    let set_dash = |pen: &mut Pen, d: &PenDash| {
        pen.dash = d.pattern.clone();
        pen.dash_offset = d.offset;
        pen.no_dashes = d.custom && d.pattern.is_empty();
    };
    // Images already converted, by the address of the context's snapshot.
    let mut images: HashMap<*const CanvasImage, Rc<Canvas>> = HashMap::new();
    let mut image = |i: &Rc<CanvasImage>| {
        images
            .entry(Rc::as_ptr(i))
            .or_insert_with(|| Rc::new(premultiplied(i)))
            .clone()
    };
    let brush = |b: &CanvasBrush, image: &mut dyn FnMut(&Rc<CanvasImage>) -> Rc<Canvas>| match b {
        CanvasBrush::None => Brush::None,
        CanvasBrush::Linear(g, stops) => Brush::Linear(*g, stops.clone()),
        CanvasBrush::Radial(g, stops) => Brush::Radial(*g, stops.clone()),
        CanvasBrush::Conical(g, stops) => Brush::Conical(*g, stops.clone()),
        CanvasBrush::Pattern(c, style) => Brush::Pattern(*c, *style),
        CanvasBrush::Texture(i) => Brush::Texture(image(i)),
    };
    for op in ops {
        match op {
            CanvasOp::Matrix(m) => p.set_matrix(*m),
            CanvasOp::ClearRect(r) => p.clear_rect(*r),
            CanvasOp::FillRect(r) => {
                if shadow.on() {
                    let n = [
                        r[0].min(r[0] + r[2]),
                        r[1].min(r[1] + r[3]),
                        r[2].abs(),
                        r[3].abs(),
                    ];
                    with_shadow(&mut p, &shadow, n, &|p| p.fill_rect(*r, &fill));
                } else {
                    p.fill_rect(*r, &fill);
                }
            }
            CanvasOp::Fill(path, odd_even) => {
                let mut path = build_path(path);
                path.close_subpath();
                if shadow.on() {
                    let bbox = path.bounding_rect();
                    with_shadow(&mut p, &shadow, bbox, &|p| {
                        p.fill_path(&path, &fill, *odd_even)
                    });
                } else {
                    p.fill_path(&path, &fill, *odd_even);
                }
            }
            CanvasOp::Stroke(_) | CanvasOp::StrokeRect(_) => {
                let path = match op {
                    CanvasOp::Stroke(path) => build_path(path),
                    CanvasOp::StrokeRect(r) => {
                        let mut path = Path::default();
                        path.add_rect(r[0], r[1], r[2], r[3]);
                        path
                    }
                    _ => unreachable!(),
                };
                if shadow.on() {
                    let d = pen.width.max(1.0);
                    let b = path.bounding_rect();
                    let bbox = [b[0] - d, b[1] - d, b[2] + 2.0 * d, b[3] + 2.0 * d];
                    with_shadow(&mut p, &shadow, bbox, &|p| p.stroke_path(&path, &pen));
                } else {
                    p.stroke_path(&path, &pen);
                }
            }
            CanvasOp::Clip(on, paths) => {
                clipping = *on;
                if *on {
                    clip = None;
                    for (path, odd_even) in paths.iter() {
                        // clip(): clipPath.closeSubpath(), then intersected(state.clipPath).
                        let mut path = build_path(path);
                        path.close_subpath();
                        clip = Some(match &clip {
                            Some((old, old_rule)) => path.intersected(*odd_even, old, *old_rule),
                            None => (path, *odd_even),
                        });
                    }
                    p.set_clip(clip.as_ref());
                } else {
                    clip = None;
                    p.clear_clip();
                }
            }
            CanvasOp::Flush => {
                pen_dash = PenDash::make(&dash, dash_offset);
                set_dash(&mut pen, &pen_dash);
                if clipping {
                    p.set_clip(clip.as_ref());
                }
            }
            CanvasOp::SetPenDash(d) => {
                pen_dash = d.clone();
                set_dash(&mut pen, &pen_dash);
            }
            CanvasOp::SetStroke(v) => pen.brush = Brush::Solid(crate::scene::rgba(v)),
            CanvasOp::SetFill(v) => fill = Brush::Solid(crate::scene::rgba(v)),
            CanvasOp::SetStrokeBrush(b) => pen.brush = brush(b, &mut image),
            CanvasOp::SetFillBrush(b) => fill = brush(b, &mut image),
            CanvasOp::SetLineWidth(v) => pen.width = *v,
            CanvasOp::SetCap(k) => pen.cap = cap(*k),
            CanvasOp::SetJoin(k) => pen.join = join(*k),
            CanvasOp::SetDash(d) => {
                dash = d.clone();
                pen_dash.set_pattern(d);
                set_dash(&mut pen, &pen_dash);
            }
            CanvasOp::SetDashOffset(v) => {
                dash_offset = *v;
                pen_dash.set_offset(*v);
                set_dash(&mut pen, &pen_dash);
            }
            CanvasOp::SetMiterLimit(v) => pen.miter_limit = *v,
            CanvasOp::SetGlobalAlpha(v) => p.set_opacity(*v),
            CanvasOp::SetComposite(m) => p.set_composition_mode(*m),
            CanvasOp::SetShadowBlur(v) => shadow.blur = *v,
            CanvasOp::SetShadowColor(c) => shadow.color = *c,
            CanvasOp::SetShadowOffsetX(v) => shadow.x = *v,
            CanvasOp::SetShadowOffsetY(v) => shadow.y = *v,
            CanvasOp::DrawImage(img, sr, dr) => {
                if img.w == 0 || img.h == 0 || !sr.iter().chain(dr).all(|v| v.is_finite()) {
                    continue;
                }
                // qt_drawImage.
                let mut im = image(img);
                let (mut sw, mut sh) = (sr[2], sr[3]);
                if sw == -1.0 || sh == -1.0 {
                    sw = im.w as f64;
                    sh = im.h as f64;
                }
                if sr[0] != 0.0 || sr[1] != 0.0 || sw != im.w as f64 || sh != im.h as f64 {
                    im = Rc::new(copy(&im, sr[0] as i64, sr[1] as i64, sw as i64, sh as i64));
                }
                if sw != dr[2] || sh != dr[3] {
                    im = Rc::new(scaled(&im, dr[2] as i64, dr[3] as i64));
                }
                if im.w == 0 || im.h == 0 || im.w * im.h > 64_000_000 {
                    continue;
                }
                if shadow.on() {
                    let bbox = [dr[0], dr[1], im.w as f64, im.h as f64];
                    with_shadow(&mut p, &shadow, bbox, &|p| p.draw_image(&im, dr[0], dr[1]));
                } else {
                    p.draw_image(&im, dr[0], dr[1]);
                }
            }
        }
    }
    p.img
}

struct Host;

impl CanvasHost for Host {
    fn measure_text(&self, font: &CanvasFont, text: &str) -> f64 {
        font_stack(font).map_or(0.0, |s| metrics(&s, text).2 as f64)
    }

    fn has_family(&self, family: &str) -> bool {
        let f = CanvasFont {
            family: family.to_string(),
            pixel_size: 10,
            point_size: -1,
            weight: 400,
            style: 0,
            small_caps: false,
        };
        font_stack(&f)
            .and_then(|s| s.font(0).map(|f| f.family()))
            .is_some_and(|n| n.eq_ignore_ascii_case(family))
    }

    fn default_family(&self, generic: &str) -> String {
        let f = CanvasFont {
            family: generic.to_string(),
            pixel_size: 10,
            point_size: -1,
            weight: 400,
            style: 0,
            small_caps: false,
        };
        font_stack(&f)
            .and_then(|s| s.font(0).map(|f| f.family()))
            .unwrap_or_else(|| generic.to_string())
    }

    fn point_in_path(&self, path: &[PathOp], odd_even: bool, p: [f64; 2]) -> bool {
        let path = build_path(path);
        if path.is_empty() {
            return false;
        }
        if path.contains(p, odd_even) {
            return true;
        }
        // The point may lie on the border.
        let fuzzy = |a: f64, b: f64| (a - b).abs() * 1e12 <= a.abs().min(b.abs());
        let within = |v: f64, a: f64, b: f64| (v >= a && v <= b) || (v >= b && v <= a);
        let border = path.to_fill_polygon();
        border.windows(2).any(|w| {
            let (p1, p2) = (w[0], w[1]);
            fuzzy(
                (p2[1] - p1[1]) * (p[0] - p1[0]),
                (p[1] - p1[1]) * (p2[0] - p1[0]),
            ) && if (p2[0] - p1[0]).abs() > (p2[1] - p1[1]).abs() {
                within(p[0], p1[0], p2[0])
            } else {
                within(p[1], p1[1], p2[1])
            }
        })
    }

    fn load_image(&self, path: &str) -> Option<Rc<CanvasImage>> {
        let (c, _) = qsg::image::load_ex(path).ok()?;
        Some(Rc::new(unpremultiplied(&c)))
    }

    fn render(&self, ops: &[CanvasOp], w: f64, h: f64) -> Option<CanvasImage> {
        if !(w.is_finite() && h.is_finite()) || w < 1.0 || h < 1.0 || w * h > 64_000_000.0 {
            return None;
        }
        Some(unpremultiplied(&replay(
            ops,
            w.ceil() as usize,
            h.ceil() as usize,
        )))
    }
}

/// Gives this thread's contexts their renderer.
pub fn install() {
    if !qml::context2d::has_host() {
        qml::context2d::set_host(Rc::new(Host));
    }
}
