//! QtQuick `Canvas` 2D context (qquickcontext2d.cpp, Qt 6.12): the JS object, its state, and
//! the command buffer the renderer replays (QQuickContext2DCommandBuffer).
//!
//! As in Qt the state machine runs when JS calls it: transforms, the state stack and the
//! current path live here and the buffer only holds absolute commands. The current path is
//! kept as the calls that built it, in user space; the renderer turns them into geometry.

use crate::color::Color;
use crate::eval::{Callable, Ex, Interp, NativeFn, R};
use crate::runtime::{World, world_of};
use crate::value::Value;
use std::cell::RefCell;
use std::rc::Rc;

/// Affine QTransform: `[m11, m12, m21, m22, dx, dy]` (`x' = m11 x + m21 y + dx`).
pub type M = [f64; 6];
pub const IDENTITY: M = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

fn fuzzy_null(v: f64) -> bool {
    v.abs() <= 1e-12
}

/// QTransform::type() of an affine matrix: 0 none, 1 translate, 2 scale, 3 rotate/shear.
fn m_type(m: &M) -> u8 {
    if !fuzzy_null(m[1]) || !fuzzy_null(m[2]) {
        3
    } else if !fuzzy_null(m[0] - 1.0) || !fuzzy_null(m[3] - 1.0) {
        2
    } else if !fuzzy_null(m[4]) || !fuzzy_null(m[5]) {
        1
    } else {
        0
    }
}

fn m_det(m: &M) -> f64 {
    m[0] * m[3] - m[1] * m[2]
}

/// QTransform::isInvertible.
fn m_invertible(m: &M) -> bool {
    !fuzzy_null(m_det(m))
}

/// `a * b` of QTransform: `a` applied first.
fn m_mul(a: &M, b: &M) -> M {
    [
        a[0] * b[0] + a[1] * b[2],
        a[0] * b[1] + a[1] * b[3],
        a[2] * b[0] + a[3] * b[2],
        a[2] * b[1] + a[3] * b[3],
        a[4] * b[0] + a[5] * b[2] + b[4],
        a[4] * b[1] + a[5] * b[3] + b[5],
    ]
}

/// QTransform::inverted (the caller checked `m_invertible`).
fn m_inverted(m: &M) -> M {
    match m_type(m) {
        0 => *m,
        1 => [m[0], m[1], m[2], m[3], -m[4], -m[5]],
        2 => {
            let (a, d) = (1.0 / m[0], 1.0 / m[3]);
            [a, 0.0, 0.0, d, -m[4] * a, -m[5] * d]
        }
        _ => {
            let det = m_det(m);
            if det == 0.0 {
                return IDENTITY;
            }
            let inv = 1.0 / det;
            [
                m[3] * inv,
                -m[1] * inv,
                -m[2] * inv,
                m[0] * inv,
                (m[2] * m[5] - m[3] * m[4]) * inv,
                (m[1] * m[4] - m[0] * m[5]) * inv,
            ]
        }
    }
}

fn m_scale(m: &M, sx: f64, sy: f64) -> M {
    if sx == 1.0 && sy == 1.0 {
        return *m;
    }
    [m[0] * sx, m[1] * sx, m[2] * sy, m[3] * sy, m[4], m[5]]
}

fn m_translate(m: &M, dx: f64, dy: f64) -> M {
    if dx == 0.0 && dy == 0.0 {
        return *m;
    }
    [
        m[0],
        m[1],
        m[2],
        m[3],
        m[4] + dx * m[0] + dy * m[2],
        m[5] + dy * m[3] + dx * m[1],
    ]
}

/// QTransform::rotate(degrees).
fn m_rotate(m: &M, a: f64) -> M {
    if a == 0.0 {
        return *m;
    }
    let (mut sina, mut cosa) = (0.0, 0.0);
    if a == 90.0 || a == -270.0 {
        sina = 1.0;
    } else if a == 270.0 || a == -90.0 {
        sina = -1.0;
    } else if a == 180.0 {
        cosa = -1.0;
    } else {
        let b = a * (std::f64::consts::PI / 180.0);
        sina = b.sin();
        cosa = b.cos();
    }
    [
        cosa * m[0] + sina * m[2],
        cosa * m[1] + sina * m[3],
        -sina * m[0] + cosa * m[2],
        -sina * m[1] + cosa * m[3],
        m[4],
        m[5],
    ]
}

fn m_shear(m: &M, sh: f64, sv: f64) -> M {
    if sh == 0.0 && sv == 0.0 {
        return *m;
    }
    [
        m[0] + sv * m[2],
        m[1] + sv * m[3],
        m[2] + sh * m[0],
        m[3] + sh * m[1],
        m[4],
        m[5],
    ]
}

/// A QFont as Context2D's `font` property sets it up (qt_font_from_string).
#[derive(Clone, Debug, PartialEq)]
pub struct CanvasFont {
    pub family: String,
    /// `setPixelSize`; -1 when the size was given in points.
    pub pixel_size: i32,
    /// `setPointSize`; -1 when the size was given in pixels.
    pub point_size: i32,
    /// QFont::Weight (400 normal, 700 bold).
    pub weight: i32,
    /// 0 normal, 1 italic, 2 oblique.
    pub style: u8,
    pub small_caps: bool,
}

impl CanvasFont {
    /// `QFont("sans-serif")` with `setPixelSize(10)`: QQuickContext2D::State's font.
    fn initial() -> Self {
        CanvasFont {
            family: "sans-serif".into(),
            pixel_size: 10,
            point_size: -1,
            weight: 400,
            style: 0,
            small_caps: false,
        }
    }

    /// QFont::toString().
    fn qt_string(&self) -> String {
        format!(
            "{},{},{},5,{},{},0,0,0,0,{},0,0,0,0,1,,0,0",
            self.family,
            self.point_size,
            self.pixel_size,
            self.weight,
            self.style,
            if self.small_caps { 3 } else { 0 }
        )
    }
}

/// One call that built the current path (QQuickContext2D::m_path), in user space.
#[derive(Clone, Debug, PartialEq)]
pub enum PathOp {
    MoveTo(f64, f64),
    LineTo(f64, f64),
    Cubic([f64; 6]),
    Quad([f64; 4]),
    Arc([f64; 5], bool),
    ArcTo([f64; 5]),
    Rect([f64; 4]),
    RoundedRect([f64; 6]),
    Ellipse([f64; 4]),
    /// `QPainterPath::addText` of `text` in `font`. `align` (0 start, 1 end, 2 left, 3 right,
    /// 4 center) and `baseline` (0 alphabetic, 1 top, 2 middle, 3 bottom, 4 hanging) move it
    /// as QQuickContext2D::createTextGlyphs does; [`RAW_TEXT`] as `align` puts the baseline
    /// origin at `(x, y)` (`ctx.text()`).
    Text {
        text: Rc<str>,
        x: f64,
        y: f64,
        font: Rc<CanvasFont>,
        align: u8,
        baseline: u8,
    },
    Close,
    /// `m_path = QTransform(m).map(m_path)`: Qt re-maps the path whenever the CTM changes.
    Map(M),
    /// `ctx.path = "M 0 0 L ..."`: an SVG path string replaces the path.
    Svg(Rc<str>),
}

pub const RAW_TEXT: u8 = 255;

pub type CanvasPath = Rc<Vec<PathOp>>;

/// Pixels of an image a context draws: straight (non-premultiplied) RGBA bytes, row by row.
#[derive(Debug, PartialEq)]
pub struct CanvasImage {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
}

/// A CanvasGradient or CanvasPattern as assigned to `fillStyle` / `strokeStyle` (a QBrush).
/// Stop colours are straight 16-bit RGBA (QColor's precision).
#[derive(Clone, Debug, PartialEq)]
pub enum CanvasBrush {
    /// A pattern that got no image: Qt::NoBrush.
    None,
    /// QLinearGradient(x0, y0, x1, y1).
    Linear([f64; 4], Vec<(f64, [u16; 4])>),
    /// QRadialGradient(center, radius, focal, focalRadius) as `[cx, cy, r, fx, fy, fr]`.
    Radial([f64; 6], Vec<(f64, [u16; 4])>),
    /// QConicalGradient(x, y, degrees).
    Conical([f64; 3], Vec<(f64, [u16; 4])>),
    /// QBrush(color, Qt::BrushStyle).
    Pattern([u8; 4], u8),
    /// QBrush(image).
    Texture(Rc<CanvasImage>),
}

/// One command of a Canvas' buffer (QQuickContext2D::PaintCommand), replayed by the renderer
/// onto the texture's persistent state. Every command is absolute, as in Qt: state changes
/// are recorded only when the JS-side state changes.
#[derive(Clone, Debug)]
pub enum CanvasOp {
    /// UpdateMatrix.
    Matrix(M),
    ClearRect([f64; 4]),
    FillRect([f64; 4]),
    /// The path and whether it fills by the odd-even rule.
    Fill(CanvasPath, bool),
    Stroke(CanvasPath),
    /// strokeRect: strokes `QPainterPath::addRect`.
    StrokeRect([f64; 4]),
    /// Clip on/off and the closed paths (with their fill rule) whose intersection clips.
    Clip(bool, Rc<Vec<(CanvasPath, bool)>>),
    /// A colour (`Value::Color` or a colour string).
    SetStroke(Value),
    SetFill(Value),
    SetStrokeBrush(Rc<CanvasBrush>),
    SetFillBrush(Rc<CanvasBrush>),
    SetLineWidth(f64),
    /// 0 flat, 1 square, 2 round.
    SetCap(u8),
    /// 1 svg-miter, 2 round, 3 bevel.
    SetJoin(u8),
    /// setLineDash pattern (even length; empty = solid).
    SetDash(Vec<f64>),
    SetDashOffset(f64),
    SetMiterLimit(f64),
    SetGlobalAlpha(f64),
    /// QPainter::CompositionMode.
    SetComposite(u8),
    SetShadowBlur(f64),
    /// Straight RGBA.
    SetShadowColor([u8; 4]),
    SetShadowOffsetX(f64),
    SetShadowOffsetY(f64),
    /// qt_drawImage(image, source rect, destination rect).
    DrawImage(Rc<CanvasImage>, [f64; 4], [f64; 4]),
    /// QQuickContext2D::flush: the buffer ends here. The next replay starts from the
    /// texture's state: its pen is `makePen(state)` and its clip is set under the matrix of
    /// that moment.
    Flush,
    /// The painter's pen dash where it is not what the commands before it leave (only
    /// [`canvas_fold`] writes it).
    SetPenDash(PenDash),
}

/// The dash of the painter's QPen. It is not always the context's `lineDash` and
/// `lineDashOffset`: QPen::setDashOffset turns a solid pen into a custom dash with an empty
/// pattern (which strokes nothing wider than a cosmetic line), and QPen::setStyle(SolidLine)
/// also zeroes the offset.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PenDash {
    /// Qt::CustomDashLine (otherwise Qt::SolidLine).
    pub custom: bool,
    pub pattern: Vec<f64>,
    pub offset: f64,
}

impl PenDash {
    /// The LineDash command: QPen::setDashPattern, or setStyle(Qt::SolidLine) for no dashes.
    pub fn set_pattern(&mut self, d: &[f64]) {
        if d.is_empty() {
            if self.custom {
                *self = PenDash::default();
            }
            return;
        }
        self.custom = true;
        self.pattern = d.to_vec();
        if d.len() % 2 == 1 {
            self.pattern.push(1.0);
        }
    }

    /// QPen::setDashOffset.
    pub fn set_offset(&mut self, o: f64) {
        // qFuzzyCompare(offset, d->dashOffset).
        if (o - self.offset).abs() * 1e12 <= o.abs().min(self.offset.abs()) {
            return;
        }
        self.offset = o;
        self.custom = true;
    }

    /// QQuickContext2DCommandBuffer::makePen for a state's lineDash and lineDashOffset.
    pub fn make(dash: &[f64], offset: f64) -> PenDash {
        let mut p = PenDash::default();
        p.set_pattern(dash);
        p.set_offset(offset);
        p
    }
}

impl PartialEq for CanvasOp {
    fn eq(&self, o: &Self) -> bool {
        use CanvasOp::*;
        match (self, o) {
            (Matrix(a), Matrix(b)) => a == b,
            (ClearRect(a), ClearRect(b))
            | (FillRect(a), FillRect(b))
            | (StrokeRect(a), StrokeRect(b)) => a == b,
            (Fill(a, x), Fill(b, y)) => x == y && a == b,
            (Stroke(a), Stroke(b)) => a == b,
            (Clip(x, a), Clip(y, b)) => x == y && a == b,
            (SetStroke(a), SetStroke(b)) | (SetFill(a), SetFill(b)) => a.strict_eq(b),
            (SetStrokeBrush(a), SetStrokeBrush(b)) | (SetFillBrush(a), SetFillBrush(b)) => {
                Rc::ptr_eq(a, b) || a == b
            }
            (SetLineWidth(a), SetLineWidth(b))
            | (SetDashOffset(a), SetDashOffset(b))
            | (SetMiterLimit(a), SetMiterLimit(b))
            | (SetGlobalAlpha(a), SetGlobalAlpha(b))
            | (SetShadowBlur(a), SetShadowBlur(b))
            | (SetShadowOffsetX(a), SetShadowOffsetX(b))
            | (SetShadowOffsetY(a), SetShadowOffsetY(b)) => a == b,
            (SetCap(a), SetCap(b))
            | (SetJoin(a), SetJoin(b))
            | (SetComposite(a), SetComposite(b)) => a == b,
            (SetDash(a), SetDash(b)) => a == b,
            (SetShadowColor(a), SetShadowColor(b)) => a == b,
            (DrawImage(a, s, d), DrawImage(b, t, e)) => {
                s == t && d == e && (Rc::ptr_eq(a, b) || a == b)
            }
            (Flush, Flush) => true,
            (SetPenDash(a), SetPenDash(b)) => a == b,
            _ => false,
        }
    }
}

const STATE_KINDS: usize = 16;

impl CanvasOp {
    fn state_kind(&self) -> Option<usize> {
        Some(match self {
            CanvasOp::SetStroke(_) | CanvasOp::SetStrokeBrush(_) => 0,
            CanvasOp::SetFill(_) | CanvasOp::SetFillBrush(_) => 1,
            CanvasOp::SetLineWidth(_) => 2,
            CanvasOp::SetCap(_) => 3,
            CanvasOp::SetJoin(_) => 4,
            CanvasOp::SetDash(_) => 5,
            CanvasOp::SetDashOffset(_) => 6,
            CanvasOp::Matrix(_) => 7,
            CanvasOp::Clip(..) => 8,
            CanvasOp::SetMiterLimit(_) => 9,
            CanvasOp::SetGlobalAlpha(_) => 10,
            CanvasOp::SetComposite(_) => 11,
            CanvasOp::SetShadowBlur(_) => 12,
            CanvasOp::SetShadowColor(_) => 13,
            CanvasOp::SetShadowOffsetX(_) => 14,
            CanvasOp::SetShadowOffsetY(_) => 15,
            _ => return None,
        })
    }
}

pub(crate) fn canvas_ops_eq(a: Option<&Vec<CanvasOp>>, b: Option<&Vec<CanvasOp>>) -> bool {
    a.map(|v| v.as_slice()) == b.map(|v| v.as_slice())
}

/// Drop recorded drawing but keep the texture state it leaves behind (a fresh image keeps
/// QQuickContext2DTexture::m_state) and what the painter holds apart from that state: a clip
/// stays under the matrix it was set with, the pen keeps its dash.
pub fn canvas_fold(ops: &mut Vec<CanvasOp>) {
    let mut last: [Option<CanvasOp>; STATE_KINDS] = Default::default();
    let (mut m, mut clip_m) = (IDENTITY, IDENTITY);
    let (mut dash, mut offset, mut pen) = (Vec::new(), 0.0, PenDash::default());
    for o in ops.drain(..) {
        match &o {
            CanvasOp::Matrix(x) => m = *x,
            CanvasOp::Clip(..) => clip_m = m,
            CanvasOp::SetDash(d) => {
                pen.set_pattern(d);
                dash = d.clone();
            }
            CanvasOp::SetDashOffset(v) => {
                pen.set_offset(*v);
                offset = *v;
            }
            CanvasOp::SetPenDash(p) => pen = p.clone(),
            CanvasOp::Flush => {
                clip_m = m;
                pen = PenDash::make(&dash, offset);
            }
            _ => {}
        }
        if let Some(k) = o.state_kind() {
            last[k] = Some(o);
        }
    }
    let (matrix, clip) = (last[7].take(), last[8].take());
    ops.extend(last.into_iter().flatten());
    if pen != PenDash::make(&dash, offset) {
        ops.push(CanvasOp::SetPenDash(pen));
    }
    match clip {
        Some(c @ CanvasOp::Clip(true, _)) if clip_m != m => {
            ops.extend([CanvasOp::Matrix(clip_m), c, CanvasOp::Matrix(m)]);
        }
        c => {
            ops.extend(matrix);
            ops.extend(c);
        }
    }
}

/// QQuickContext2D::flush after the commands recorded so far.
pub fn canvas_flush(ops: &mut Vec<CanvasOp>) {
    if !matches!(ops.last(), None | Some(CanvasOp::Flush)) {
        ops.push(CanvasOp::Flush);
    }
}

/// What a context needs from the renderer while JS runs: text metrics, images and pixels.
/// The shell installs one per thread; without it text measures 0 and images do not load.
pub trait CanvasHost {
    /// `QFontMetrics(font).horizontalAdvance(text)`.
    fn measure_text(&self, font: &CanvasFont, text: &str) -> f64;
    /// QFontDatabase::hasFamily.
    fn has_family(&self, family: &str) -> bool;
    /// `QFont::defaultFamily()` for a style hint named as in CSS (`serif`, `monospace`, ...).
    fn default_family(&self, generic: &str) -> String;
    /// QQuickContext2D::isPointInPath on the path these calls build; `p` is already in the
    /// path's coordinates.
    fn point_in_path(&self, path: &[PathOp], odd_even: bool, p: [f64; 2]) -> bool;
    /// The decoded image file at `path`.
    fn load_image(&self, path: &str) -> Option<Rc<CanvasImage>>;
    /// The canvas these commands paint, `w` x `h` item pixels.
    fn render(&self, ops: &[CanvasOp], w: f64, h: f64) -> Option<CanvasImage>;
}

thread_local! {
    static HOST: RefCell<Option<Rc<dyn CanvasHost>>> = const { RefCell::new(None) };
}

pub fn set_host(h: Rc<dyn CanvasHost>) {
    HOST.with(|c| *c.borrow_mut() = Some(h));
}

pub fn has_host() -> bool {
    HOST.with(|c| c.borrow().is_some())
}

fn host() -> Option<Rc<dyn CanvasHost>> {
    HOST.with(|c| c.borrow().clone())
}

/// A QBrush held by `fillStyle` / `strokeStyle`.
#[derive(Clone, Debug, PartialEq)]
enum Style {
    /// A colour brush; `Color::INVALID` is `QBrush(QColor())`.
    Color(Color),
    Brush(Rc<CanvasBrush>),
}

impl Style {
    /// `brush.color()` when it is valid: what the style getters turn into a string.
    fn color(&self) -> Option<[u8; 4]> {
        match self {
            Style::Color(c) if *c != Color::INVALID => Some(c.to_rgb().rgba8()),
            Style::Color(_) => None,
            Style::Brush(b) => match &**b {
                CanvasBrush::Pattern(c, _) => Some(*c),
                // QBrush() + setTextureImage keeps the default colour, black.
                CanvasBrush::Texture(_) | CanvasBrush::None => Some([0, 0, 0, 255]),
                _ => None,
            },
        }
    }
}

type ClipPath = Rc<Vec<(CanvasPath, bool)>>;

/// QQuickContext2D::State.
#[derive(Clone, PartialEq)]
struct State {
    matrix: M,
    invertible: bool,
    stroke: Style,
    fill: Style,
    odd_even: bool,
    global_alpha: f64,
    line_width: f64,
    cap: u8,
    join: u8,
    dash: Vec<f64>,
    dash_offset: f64,
    miter_limit: f64,
    shadow_ox: f64,
    shadow_oy: f64,
    shadow_blur: f64,
    shadow_color: [u8; 4],
    composite: u8,
    font: Rc<CanvasFont>,
    text_align: u8,
    text_baseline: u8,
    clip: bool,
    clip_path: ClipPath,
}

impl Default for State {
    fn default() -> Self {
        State {
            matrix: IDENTITY,
            invertible: true,
            stroke: Style::Color(Color::from_argb32(0xff00_0000)),
            fill: Style::Color(Color::from_argb32(0xff00_0000)),
            odd_even: false,
            global_alpha: 1.0,
            line_width: 1.0,
            cap: 0,
            join: 0,
            dash: Vec::new(),
            dash_offset: 0.0,
            miter_limit: 10.0,
            shadow_ox: 0.0,
            shadow_oy: 0.0,
            shadow_blur: 0.0,
            shadow_color: [0, 0, 0, 0],
            composite: 0,
            font: Rc::new(CanvasFont::initial()),
            text_align: 0,
            text_baseline: 0,
            clip: false,
            clip_path: Rc::new(Vec::new()),
        }
    }
}

struct Ctx {
    state: State,
    stack: Vec<State>,
    /// m_path, as the calls that built it.
    path: Vec<PathOp>,
    /// m_path.fillRule(): a new QPainterPath is odd-even; fill(), isPointInPath() and the
    /// fillRule setter give it the state's rule, and clip() uses the one it has.
    path_odd_even: bool,
    /// The JS values last assigned (QQuickContext2D::m_fillStyle, m_strokeStyle, m_v4path).
    fill_js: Value,
    stroke_js: Value,
    path_js: Value,
}

/// A Canvas 2D context object's native side (QQuickContext2D).
pub struct Context2d {
    item: usize,
    s: RefCell<Ctx>,
}

fn context2d(this: &Value) -> Option<Rc<Context2d>> {
    match this {
        Value::Obj(o) => o.borrow().native.clone()?.downcast::<Context2d>().ok(),
        _ => None,
    }
}

/// Appends buffer commands of the context's Canvas.
fn emit(it: &mut Interp, cx: &Context2d, ops: Vec<CanvasOp>) {
    if ops.is_empty() {
        return;
    }
    let Some(world) = world_of(it) else { return };
    let mut w = world.borrow_mut();
    w.canvas_ops.entry(cx.item).or_default().extend(ops);
    w.bump_canvas(cx.item);
}

fn canvas_size(w: &mut World, id: usize) -> (f64, f64) {
    (
        w.read(id, "width").map_or(0.0, |v| v.to_number()),
        w.read(id, "height").map_or(0.0, |v| v.to_number()),
    )
}

/// THROW_DOM: an Error with an own `code`.
fn throw_dom<T>(it: &Interp, code: u32, msg: &str) -> R<T> {
    let e = it.make_error("Error", Some(msg.into()));
    if let Value::Obj(o) = &e {
        crate::eval::set_own(o, "code", Value::Num(code as f64));
    }
    Err(Ex::Throw(e))
}

const INDEX_SIZE_ERR: u32 = 1;
const NOT_SUPPORTED_ERR: u32 = 9;
const SYNTAX_ERR: u32 = 12;
const TYPE_MISMATCH_ERR: u32 = 17;

fn num(a: &[Value], k: usize) -> f64 {
    a.get(k).map_or(f64::NAN, |v| v.to_number())
}

fn nums<const N: usize>(a: &[Value]) -> [f64; N] {
    std::array::from_fn(|k| num(a, k))
}

fn finite(v: &[f64]) -> bool {
    v.iter().all(|x| x.is_finite())
}

impl Ctx {
    fn map_path(&mut self, m: M) {
        if !self.path.is_empty() && m != IDENTITY {
            self.path.push(PathOp::Map(m));
        }
    }

    /// QQuickContext2D::transform.
    fn transform(&mut self, t: M, out: &mut Vec<CanvasOp>) {
        if !self.state.invertible || !finite(&t) {
            return;
        }
        let m = m_mul(&t, &self.state.matrix);
        if !m_invertible(&m) {
            self.state.invertible = false;
            return;
        }
        self.state.matrix = m;
        out.push(CanvasOp::Matrix(m));
        self.map_path(m_inverted(&t));
    }

    /// QQuickContext2D::setTransform.
    fn set_transform(&mut self, t: M, out: &mut Vec<CanvasOp>) {
        if !finite(&t) {
            return;
        }
        let ctm = self.state.matrix;
        if !m_invertible(&ctm) {
            return;
        }
        self.state.matrix = m_mul(&m_inverted(&ctm), &self.state.matrix);
        self.map_path(ctm);
        self.state.invertible = true;
        self.transform(t, out);
    }

    /// QQuickContext2D::popState.
    fn pop_state(&mut self, out: &mut Vec<CanvasOp>) {
        let Some(new) = self.stack.pop() else { return };
        let old = &self.state;
        if old.matrix != new.matrix {
            out.push(CanvasOp::Matrix(new.matrix));
        }
        if old.global_alpha != new.global_alpha {
            out.push(CanvasOp::SetGlobalAlpha(new.global_alpha));
        }
        if old.composite != new.composite {
            out.push(CanvasOp::SetComposite(new.composite));
        }
        if old.fill != new.fill {
            out.push(style_op(&new.fill, true));
        }
        if old.stroke != new.stroke {
            out.push(style_op(&new.stroke, false));
        }
        if old.line_width != new.line_width {
            out.push(CanvasOp::SetLineWidth(new.line_width));
        }
        if old.cap != new.cap {
            out.push(CanvasOp::SetCap(new.cap));
        }
        if old.join != new.join {
            out.push(CanvasOp::SetJoin(new.join));
        }
        if old.miter_limit != new.miter_limit {
            out.push(CanvasOp::SetMiterLimit(new.miter_limit));
        }
        if old.clip != new.clip || old.clip_path != new.clip_path {
            out.push(CanvasOp::Clip(new.clip, new.clip_path.clone()));
        }
        if old.shadow_blur != new.shadow_blur {
            out.push(CanvasOp::SetShadowBlur(new.shadow_blur));
        }
        if old.shadow_color != new.shadow_color {
            out.push(CanvasOp::SetShadowColor(new.shadow_color));
        }
        if old.shadow_ox != new.shadow_ox {
            out.push(CanvasOp::SetShadowOffsetX(new.shadow_ox));
        }
        if old.shadow_oy != new.shadow_oy {
            out.push(CanvasOp::SetShadowOffsetY(new.shadow_oy));
        }
        if old.dash != new.dash {
            out.push(CanvasOp::SetDash(new.dash.clone()));
        }
        // Qt restores the dash offset of the state but never tells the buffer.
        let back = self.state.matrix;
        self.map_path(back);
        self.state = new;
        let inv = m_inverted(&self.state.matrix);
        self.map_path(inv);
    }

    /// QQuickContext2D::beginPath: a path without elements is left as it is.
    fn begin_path(&mut self) {
        if !self.path.is_empty() {
            self.path.clear();
            self.path_odd_even = true;
        }
    }

    fn path_snapshot(&self) -> CanvasPath {
        Rc::new(self.path.clone())
    }
}

/// The buffer command that sets a fill (`fill`) or stroke style. An invalid QColor paints
/// as opaque black.
fn style_op(s: &Style, fill: bool) -> CanvasOp {
    let col = |c: &Color| {
        Value::Color(if *c == Color::INVALID {
            Color::from_argb32(0xff00_0000)
        } else {
            *c
        })
    };
    match (s, fill) {
        (Style::Color(c), true) => CanvasOp::SetFill(col(c)),
        (Style::Color(c), false) => CanvasOp::SetStroke(col(c)),
        (Style::Brush(b), true) => CanvasOp::SetFillBrush(b.clone()),
        (Style::Brush(b), false) => CanvasOp::SetStrokeBrush(b.clone()),
    }
}

fn qround(v: f64) -> i32 {
    if v >= 0.0 {
        (v + 0.5) as i32
    } else {
        (v - 0.5) as i32
    }
}

/// C `strtol(p, &p, 10)` on `b` from `*i`.
fn strtol(b: &[u8], i: &mut usize) -> i64 {
    let mut j = *i;
    while b.get(j).is_some_and(|c| c.is_ascii_whitespace()) {
        j += 1;
    }
    let neg = match b.get(j) {
        Some(b'-') => {
            j += 1;
            true
        }
        Some(b'+') => {
            j += 1;
            false
        }
        _ => false,
    };
    let start = j;
    let mut v: i64 = 0;
    while let Some(d) = b.get(j).filter(|c| c.is_ascii_digit()) {
        v = v.saturating_mul(10).saturating_add((d - b'0') as i64);
        j += 1;
    }
    if j == start {
        return 0;
    }
    *i = j;
    if neg { -v } else { v }
}

/// `qstrtod(p, &p, &ok)`: the longest decimal number at `*i`, 0 when there is none.
fn strtod(b: &[u8], i: &mut usize) -> f64 {
    let mut j = *i;
    if matches!(b.get(j), Some(b'-' | b'+')) {
        j += 1;
    }
    let digits = |j: &mut usize| {
        let s = *j;
        while b.get(*j).is_some_and(|c| c.is_ascii_digit()) {
            *j += 1;
        }
        *j - s
    };
    let mut n = digits(&mut j);
    if b.get(j) == Some(&b'.') {
        j += 1;
        n += digits(&mut j);
    }
    if n == 0 {
        return 0.0;
    }
    if matches!(b.get(j), Some(b'e' | b'E')) {
        let mut k = j + 1;
        if matches!(b.get(k), Some(b'-' | b'+')) {
            k += 1;
        }
        if digits(&mut k) > 0 {
            j = k;
        }
    }
    let v = std::str::from_utf8(&b[*i..j])
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);
    *i = j;
    v
}

/// qt_color_from_string: QColor::fromString plus CSS `rgb()`, `rgba()`, `hsl()`, `hsla()`.
fn color_from_string(s: &str) -> Color {
    let b = s.as_bytes();
    if b.len() > 255 || b.len() <= 7 {
        return crate::builtins::parse_color(s);
    }
    let at = |i: usize| b.get(i).copied().unwrap_or(0);
    let mut i = 0;
    while at(i).is_ascii_whitespace() {
        i += 1;
    }
    let rgb = b[i..].starts_with(b"rgb");
    if !rgb && !b[i..].starts_with(b"hsl") {
        return crate::builtins::parse_color(&s[i..]);
    }
    i += 3;
    let has_alpha = at(i) == b'a';
    i += 1;
    if has_alpha {
        i += 1;
    }
    let mut comp = [0i64; 3];
    for (k, c) in comp.iter_mut().enumerate() {
        while at(i).is_ascii_whitespace() {
            i += 1;
        }
        *c = strtol(b, &mut i);
        if at(i) == b'%' {
            *c = qround(*c as f64 / 100.0 * 255.0) as i64;
            i += 1;
        }
        if k < 2 {
            if at(i) != b',' {
                return Color::INVALID;
            }
            i += 1;
        }
    }
    let mut alpha = 255i64;
    if has_alpha {
        if at(i) != b',' {
            return Color::INVALID;
        }
        i += 1;
        while at(i).is_ascii_whitespace() {
            i += 1;
        }
        alpha = qround(strtod(b, &mut i) * 255.0) as i64;
    }
    if at(i) != b')' {
        return Color::INVALID;
    }
    let a = alpha.clamp(0, 255) as u16;
    if rgb {
        let c = comp.map(|v| v.clamp(0, 255) as u32);
        Color::from_argb32((a as u32) << 24 | c[0] << 16 | c[1] << 8 | c[2])
    } else {
        // QColor::fromHsl(h, s, l, a).
        Color {
            spec: crate::color::Spec::Hsl,
            a: a * 0x101,
            c: [
                comp[0].clamp(0, 359) as u16 * 100,
                comp[1].clamp(0, 255) as u16 * 0x101,
                comp[2].clamp(0, 255) as u16 * 0x101,
            ],
        }
    }
}

/// A JS value as a QColor: a `color` value, else a colour string.
fn value_color(v: &Value) -> Color {
    let v = &crate::valuetypes::plain(v.clone());
    match v {
        Value::Color(c) => *c,
        Value::Str(_) | Value::Str16(_) => color_from_string(&v.to_js_string()),
        _ => Color::INVALID,
    }
}

/// makeColorString of a valid colour.
fn color_string(c: [u8; 4]) -> String {
    if c[3] == 255 {
        return format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]);
    }
    // QString::number(alphaF, 'f') without trailing zeros, one decimal at least.
    let mut a = format!("{:.6}", (c[3] as f32 / 255.0) as f64);
    while a.ends_with('0') && !a.ends_with(".0") {
        a.pop();
    }
    format!("rgba({}, {}, {}, {})", c[0], c[1], c[2], a)
}

fn rgba16(c: Color) -> [u16; 4] {
    let c = c.to_rgb();
    [c.c[0], c.c[1], c.c[2], c.a]
}

/// qExtractFontFamiliesFromString.
fn font_families(s: &[char]) -> Vec<String> {
    let mut out = Vec::new();
    let mut quote: Option<usize> = None;
    let mut cur = String::new();
    for (i, &ch) in s.iter().enumerate() {
        if ch == '"' || ch == '\'' {
            match quote {
                None => quote = Some(i),
                Some(q) if s[q] == ch => {
                    out.push(s[q + 1..i].iter().collect());
                    cur.clear();
                    quote = None;
                }
                Some(_) => return Vec::new(),
            }
        } else if ch == ' ' && quote.is_none() {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
        } else {
            cur.push(ch);
        }
    }
    if !cur.is_empty() {
        if quote.is_some() {
            return Vec::new();
        }
        out.push(cur);
    }
    out
}

/// qt_font_from_string: the CSS `font` shorthand; `None` keeps the current font.
fn font_from_string(s: &str) -> Option<CanvasFont> {
    let c: Vec<char> = s.chars().collect();
    let find = |pat: [char; 2]| c.windows(2).position(|w| w == pat);
    let unit_at = find(['p', 'x']).or_else(|| find(['p', 't']))?;
    let size_start = c[..=unit_at.min(c.len() - 1)]
        .iter()
        .rposition(|&ch| ch == ' ')
        .map_or(0, |p| p + 1);
    let size_end = (unit_at + 3).min(c.len());
    // qSetFontSizeFromToken.
    let token: String = c[size_start..size_end].iter().collect();
    let token = token.trim();
    let tc: Vec<char> = token.chars().collect();
    if tc.len() < 2 {
        return None;
    }
    let unit: String = tc[tc.len() - 2..].iter().collect();
    let value: String = tc[..tc.len() - 2].iter().collect();
    let size = value.trim().parse::<f32>().ok().filter(|v| v.is_finite())? as i32;
    let mut font = CanvasFont {
        family: String::new(),
        pixel_size: -1,
        point_size: -1,
        weight: 400,
        style: 0,
        small_caps: false,
    };
    match unit.as_str() {
        "px" => font.pixel_size = size,
        "pt" => font.point_size = size,
        _ => return None,
    }
    // QFont::setPixelSize / setPointSize refuse sizes <= 0 and leave the default 12pt.
    if size <= 0 {
        font.pixel_size = -1;
        font.point_size = 12;
    }
    let families = font_families(&c[size_end..]);
    let host = host();
    font.family = families.iter().find_map(|f| {
        if host.as_ref().is_some_and(|h| h.has_family(f)) {
            return Some(f.clone());
        }
        matches!(
            f.as_str(),
            "serif" | "sans-serif" | "cursive" | "monospace" | "fantasy"
        )
        .then(|| match &host {
            Some(h) => h.default_family(f),
            None => f.clone(),
        })
    })?;
    let head: String = c[..size_start].iter().collect();
    let head = head.trim();
    if head.is_empty() {
        return Some(font);
    }
    let (mut style, mut variant, mut weight) = (false, false, false);
    let take = |used: &mut bool| !std::mem::replace(used, true);
    for t in head.split(' ') {
        match t {
            "normal" => {
                if !take(&mut style) && !take(&mut variant) && !take(&mut weight) {
                    return None;
                }
            }
            "bold" => {
                if !take(&mut weight) {
                    return None;
                }
                font.weight = 700;
            }
            "italic" | "oblique" => {
                if !take(&mut style) {
                    return None;
                }
                font.style = if t == "italic" { 1 } else { 2 };
            }
            "small-caps" => {
                if !take(&mut variant) {
                    return None;
                }
                font.small_caps = true;
            }
            _ => {
                let w = t.parse::<i32>().ok()?;
                if !take(&mut weight) {
                    return None;
                }
                // QFont::setWeight ignores a weight outside 1..=1000.
                if (1..=1000).contains(&w) {
                    font.weight = w;
                }
            }
        }
    }
    Some(font)
}

/// A CanvasGradient / CanvasPattern JS object's native side (QQuickContext2DStyle).
struct StyleObj {
    brush: RefCell<CanvasBrush>,
}

fn style_obj(v: &Value) -> Option<Rc<StyleObj>> {
    match v {
        Value::Obj(o) => o.borrow().native.clone()?.downcast::<StyleObj>().ok(),
        _ => None,
    }
}

fn native_fn(name: &'static str, f: NativeFn) -> Value {
    crate::eval::fn_value(Callable::Native(name, f))
}

fn new_style(brush: CanvasBrush, gradient: bool) -> Value {
    let o = crate::eval::new_obj();
    o.borrow_mut().native = Some(Rc::new(StyleObj {
        brush: RefCell::new(brush),
    }));
    if gradient {
        crate::eval::set_own(
            &o,
            "addColorStop",
            native_fn("addColorStop", add_color_stop),
        );
    }
    Value::Obj(o)
}

/// QQuickContext2DStyle::gradient_proto_addColorStop.
fn add_color_stop(it: &mut Interp, this: &Value, a: &[Value]) -> R<Value> {
    let Some(style) = style_obj(this) else {
        return it.throw("Error", "Not a CanvasGradient object");
    };
    if a.len() != 2 {
        return Ok(this.clone());
    }
    let mut brush = style.brush.borrow_mut();
    let stops = match &mut *brush {
        CanvasBrush::Linear(_, s) | CanvasBrush::Radial(_, s) | CanvasBrush::Conical(_, s) => s,
        _ => {
            return it.throw(
                "Error",
                "Not a valid CanvasGradient object, can't get the gradient information",
            );
        }
    };
    let pos = a[0].to_number();
    let color = value_color(&a[1]);
    if !(0.0..=1.0).contains(&pos) {
        return throw_dom(
            it,
            INDEX_SIZE_ERR,
            "CanvasGradient: parameter offset out of range",
        );
    }
    if color == Color::INVALID {
        return throw_dom(
            it,
            SYNTAX_ERR,
            "CanvasGradient: parameter color is not a valid color string",
        );
    }
    // QGradient::setColorAt.
    let i = stops.iter().take_while(|s| s.0 < pos).count();
    let c = rgba16(color);
    match stops.get_mut(i) {
        Some(s) if s.0 == pos => s.1 = c,
        _ => stops.insert(i, (pos, c)),
    }
    Ok(this.clone())
}

/// An ImageData JS object's native side: its pixels are the bytes of its `data` array.
struct ImageData {
    w: usize,
    h: usize,
    bytes: crate::typedarray::Bytes,
}

fn image_data(v: &Value) -> Option<Rc<ImageData>> {
    match v {
        Value::Obj(o) => o.borrow().native.clone()?.downcast::<ImageData>().ok(),
        _ => None,
    }
}

/// An ImageData of `w` x `h` holding `px` (straight RGBA).
fn new_image_data(it: &Interp, w: usize, h: usize, px: Vec<u8>) -> Value {
    let (buffer, bytes) = crate::typedarray::new_buffer(it, px);
    let data = crate::typedarray::new_view(
        it,
        crate::typedarray::Kind::U8C,
        buffer,
        bytes.clone(),
        0,
        w * h * 4,
    );
    let o = crate::eval::new_obj();
    o.borrow_mut().native = Some(Rc::new(ImageData { w, h, bytes }));
    crate::eval::set_own(&o, "width", Value::Num(w as f64));
    crate::eval::set_own(&o, "height", Value::Num(h as f64));
    crate::eval::set_own(&o, "data", Value::Obj(data));
    Value::Obj(o)
}

fn snapshot(d: &ImageData) -> Rc<CanvasImage> {
    Rc::new(CanvasImage {
        w: d.w,
        h: d.h,
        px: d.bytes.borrow().clone(),
    })
}

/// The file an image URL names.
fn url_path(url: &str) -> &str {
    url.strip_prefix("file://").unwrap_or(url)
}

fn load_image(url: &str) -> Option<Rc<CanvasImage>> {
    host()?.load_image(url_path(url))
}

/// QQuickCanvasItem::toImage: what canvas `id` shows now.
fn canvas_image(it: &mut Interp, id: usize) -> Option<CanvasImage> {
    image_of(&world_of(it)?, id)
}

/// The same, of the canvas `id` of `world`.
pub(crate) fn image_of(world: &Rc<RefCell<World>>, id: usize) -> Option<CanvasImage> {
    let (ops, (w, h)) = {
        let mut w = world.borrow_mut();
        let size = canvas_size(&mut w, id);
        // QQuickContext2D::toImage flushes first.
        let ops = w.canvas_ops.entry(id).or_default();
        canvas_flush(ops);
        (ops.clone(), size)
    };
    host()?.render(&ops, w, h)
}

fn is_type(it: &mut Interp, id: usize, t: &str) -> bool {
    world_of(it).is_some_and(|w| {
        w.borrow()
            .items
            .get(id)
            .is_some_and(|i| i.types.iter().any(|x| x == t))
    })
}

/// Runs `f` on the context of `this` and records what it emits; returns `this` (Qt's
/// methods chain).
fn method(it: &mut Interp, this: &Value, f: impl FnOnce(&mut Ctx, &mut Vec<CanvasOp>)) -> R<Value> {
    let Some(cx) = context2d(this) else {
        return it.throw("Error", "Not a Context2D object");
    };
    let mut out = Vec::new();
    f(&mut cx.s.borrow_mut(), &mut out);
    emit(it, &cx, out);
    Ok(this.clone())
}

/// A property setter: like [`method`], returning undefined.
fn setter(it: &mut Interp, this: &Value, f: impl FnOnce(&mut Ctx, &mut Vec<CanvasOp>)) -> R<Value> {
    method(it, this, f).map(|_| Value::Undefined)
}

fn getter(it: &mut Interp, this: &Value, f: impl FnOnce(&Ctx) -> Value) -> R<Value> {
    match context2d(this) {
        Some(cx) => Ok(f(&cx.s.borrow())),
        None => it.throw("Error", "Not a Context2D object"),
    }
}

/// `(x, y, w, h)` covers the whole canvas under `m`, a translation and scale at most.
fn covers(m: &M, r: [f64; 4], cw: f64, ch: f64) -> bool {
    if m_type(m) > 2 {
        return false;
    }
    let (x0, x1) = (m[0] * r[0] + m[4], m[0] * (r[0] + r[2]) + m[4]);
    let (y0, y1) = (m[3] * r[1] + m[5], m[3] * (r[1] + r[3]) + m[5]);
    x0.min(x1) <= 0.0 && y0.min(y1) <= 0.0 && x0.max(x1) >= cw && y0.max(y1) >= ch
}

/// clearRect: a clear of the whole unclipped canvas also drops the drawing recorded so far.
fn clear_rect(it: &mut Interp, this: &Value, r: [f64; 4]) -> R<Value> {
    let Some(cx) = context2d(this) else {
        return it.throw("Error", "Not a Context2D object");
    };
    let (m, clip) = {
        let s = cx.s.borrow();
        if !s.state.invertible || !finite(&r) {
            return Ok(this.clone());
        }
        (s.state.matrix, s.state.clip)
    };
    let Some(world) = world_of(it) else {
        return Ok(this.clone());
    };
    let mut w = world.borrow_mut();
    let (cw, ch) = canvas_size(&mut w, cx.item);
    w.bump_canvas(cx.item);
    let ops = w.canvas_ops.entry(cx.item).or_default();
    if !clip && covers(&m, r, cw, ch) {
        canvas_fold(ops);
    } else {
        ops.push(CanvasOp::ClearRect(r));
    }
    Ok(this.clone())
}

fn unsupported(it: &mut Interp, name: &str) -> R<Value> {
    throw_dom(
        it,
        NOT_SUPPORTED_ERR,
        &format!("Context2D::{name} is not supported"),
    )
}

/// The image `v` names for drawImage / createPattern: a URL string, an Image or Canvas
/// item, or an ImageData. `Err` is Qt's type mismatch.
fn image_arg(it: &mut Interp, v: &Value) -> Result<Option<Rc<CanvasImage>>, ()> {
    match v {
        Value::Str(_) | Value::Str16(_) => Ok(load_image(&v.to_js_string())),
        Value::Item(id) => {
            if is_type(it, *id, "Canvas") {
                return Ok(canvas_image(it, *id).map(Rc::new));
            }
            let src = world_of(it)
                .and_then(|w| w.borrow_mut().read(*id, "source"))
                .map(|s| s.to_js_string())
                .unwrap_or_default();
            Ok(load_image(&src))
        }
        Value::Obj(_) => match image_data(v) {
            Some(d) => Ok(Some(snapshot(&d))),
            None => Ok(load_image(&v.to_js_string())),
        },
        _ => Err(()),
    }
}

/// QQuickJSContext2DPrototype::method_drawImage.
fn draw_image(it: &mut Interp, this: &Value, a: &[Value]) -> R<Value> {
    let Some(cx) = context2d(this) else {
        return it.throw("Error", "Not a Context2D object");
    };
    if a.is_empty() || !cx.s.borrow().state.invertible {
        return Ok(this.clone());
    }
    let Ok(img) = image_arg(it, &a[0]) else {
        return throw_dom(it, TYPE_MISMATCH_ERR, "drawImage(), type mismatch");
    };
    let Some(img) = img.filter(|i| i.w > 0 && i.h > 0) else {
        return Ok(this.clone());
    };
    let (iw, ih) = (img.w as f64, img.h as f64);
    let [sx, sy, sw, sh, dx, dy, dw, dh] = if a.len() >= 9 {
        std::array::from_fn(|k| num(a, k + 1))
    } else if a.len() >= 5 {
        [0.0, 0.0, iw, ih, num(a, 1), num(a, 2), num(a, 3), num(a, 4)]
    } else if a.len() >= 3 {
        [0.0, 0.0, iw, ih, num(a, 1), num(a, 2), iw, ih]
    } else {
        return Ok(this.clone());
    };
    if !finite(&[sx, sy, sw, sh, dx, dy, dw, dh]) {
        return Ok(this.clone());
    }
    if sx < 0.0
        || sy < 0.0
        || sw == 0.0
        || sh == 0.0
        || sx + sw > iw
        || sy + sh > ih
        || sx + sw < 0.0
        || sy + sh < 0.0
    {
        return throw_dom(it, INDEX_SIZE_ERR, "drawImage(), index size error");
    }
    emit(
        it,
        &cx,
        vec![CanvasOp::DrawImage(img, [sx, sy, sw, sh], [dx, dy, dw, dh])],
    );
    Ok(this.clone())
}

/// QRectF(x, y, w, h).toRect() as `[x, y, w, h]`.
fn to_rect(x: f64, y: f64, w: f64, h: f64) -> [i64; 4] {
    let (l, t) = (qround(x) as i64, qround(y) as i64);
    [l, t, qround(x + w) as i64 - l, qround(y + h) as i64 - t]
}

fn get_image_data(it: &mut Interp, this: &Value, a: &[Value]) -> R<Value> {
    let Some(cx) = context2d(this) else {
        return it.throw("Error", "Not a Context2D object");
    };
    if a.len() < 4 {
        return Ok(Value::Null);
    }
    let [x, y, w, h] = nums(a);
    if !finite(&[x, y, w, h]) {
        return throw_dom(it, NOT_SUPPORTED_ERR, "getImageData(): Invalid arguments");
    }
    if w <= 0.0 || h <= 0.0 {
        return throw_dom(it, INDEX_SIZE_ERR, "getImageData(): Invalid arguments");
    }
    let [rx, ry, rw, rh] = to_rect(x, y, w, h);
    let (rw, rh) = (rw.max(0) as usize, rh.max(0) as usize);
    let mut px = vec![0u8; rw * rh * 4];
    if let Some(img) = canvas_image(it, cx.item) {
        for j in 0..rh {
            let sy = ry + j as i64;
            if sy < 0 || sy >= img.h as i64 {
                continue;
            }
            for i in 0..rw {
                let sx = rx + i as i64;
                if sx < 0 || sx >= img.w as i64 {
                    continue;
                }
                let s = (sy as usize * img.w + sx as usize) * 4;
                let d = (j * rw + i) * 4;
                px[d..d + 4].copy_from_slice(&img.px[s..s + 4]);
            }
        }
    }
    Ok(new_image_data(it, rw, rh, px))
}

fn create_image_data(it: &mut Interp, this: &Value, a: &[Value]) -> R<Value> {
    if context2d(this).is_none() {
        return it.throw("Error", "Not a Context2D object");
    }
    match a {
        [v] => {
            if let Some(d) = image_data(v) {
                return Ok(new_image_data(it, d.w, d.h, vec![0; d.w * d.h * 4]));
            }
            if matches!(v, Value::Str(_) | Value::Str16(_)) {
                return Ok(match load_image(&v.to_js_string()) {
                    Some(i) => new_image_data(it, i.w, i.h, i.px.clone()),
                    None => new_image_data(it, 0, 0, Vec::new()),
                });
            }
            Ok(Value::Undefined)
        }
        [w, h] => {
            let (w, h) = (w.to_number(), h.to_number());
            if !finite(&[w, h]) {
                return throw_dom(
                    it,
                    NOT_SUPPORTED_ERR,
                    "createImageData(): invalid arguments",
                );
            }
            if w <= 0.0 || h <= 0.0 {
                return throw_dom(it, INDEX_SIZE_ERR, "createImageData(): invalid arguments");
            }
            let (w, h) = (qround(w).max(0) as usize, qround(h).max(0) as usize);
            Ok(new_image_data(it, w, h, vec![0; w * h * 4]))
        }
        _ => Ok(Value::Undefined),
    }
}

/// QQuickJSContext2DPrototype::method_putImageData: Qt 6.12 does nothing with fewer than 7
/// arguments.
fn put_image_data(it: &mut Interp, this: &Value, a: &[Value]) -> R<Value> {
    let Some(cx) = context2d(this) else {
        return it.throw("Error", "Not a Context2D object");
    };
    if a.len() < 7 {
        return Ok(Value::Undefined);
    }
    if !matches!(a[0], Value::Obj(_)) {
        return throw_dom(
            it,
            TYPE_MISMATCH_ERR,
            "Context2D::putImageData, the image data type mismatch",
        );
    }
    let (dx, dy) = (num(a, 1), num(a, 2));
    if !finite(&[dx, dy]) {
        return throw_dom(it, NOT_SUPPORTED_ERR, "putImageData() : Invalid arguments");
    }
    let Some(d) = image_data(&a[0]) else {
        return Ok(Value::Undefined);
    };
    let (w, h) = (d.w as f64, d.h as f64);
    let [mut x, mut y, mut dw, mut dh] = [0.0, 0.0, w, h];
    if a.len() == 7 {
        [x, y, dw, dh] = std::array::from_fn(|k| num(a, k + 3));
        if !finite(&[x, y, dw, dh]) {
            return throw_dom(it, NOT_SUPPORTED_ERR, "putImageData() : Invalid arguments");
        }
        if dw < 0.0 {
            x += dw;
            dw = -dw;
        }
        if dh < 0.0 {
            y += dh;
            dh = -dh;
        }
        if x < 0.0 {
            dw += x;
            x = 0.0;
        }
        if y < 0.0 {
            dh += y;
            y = 0.0;
        }
        if x + dw > w {
            dw = w - x;
        }
        if y + dh > h {
            dh = h - y;
        }
        if dw <= 0.0 || dh <= 0.0 {
            return Ok(Value::Undefined);
        }
    }
    // QImage::copy(int x, int y, int w, int h): the arguments truncate.
    let (cx0, cy0, cw, chh) = (x as usize, y as usize, dw as usize, dh as usize);
    let src = d.bytes.borrow();
    let mut px = vec![0u8; cw * chh * 4];
    for j in 0..chh {
        for i in 0..cw {
            let (sx, sy) = (cx0 + i, cy0 + j);
            if sx < d.w && sy < d.h {
                let s = (sy * d.w + sx) * 4;
                let t = (j * cw + i) * 4;
                px[t..t + 4].copy_from_slice(&src[s..s + 4]);
            }
        }
    }
    drop(src);
    let img = Rc::new(CanvasImage { w: cw, h: chh, px });
    emit(
        it,
        &cx,
        vec![CanvasOp::DrawImage(img, [x, y, dw, dh], [dx, dy, dw, dh])],
    );
    Ok(this.clone())
}

const COMPOSITE: [&str; 24] = [
    "source-over",
    "destination-over",
    "qt-clear",
    "copy",
    "qt-destination",
    "source-in",
    "destination-in",
    "source-out",
    "destination-out",
    "source-atop",
    "destination-atop",
    "xor",
    "lighter",
    "qt-multiply",
    "qt-screen",
    "qt-overlay",
    "qt-darken",
    "qt-lighten",
    "qt-color-dodge",
    "qt-color-burn",
    "qt-hard-light",
    "qt-soft-light",
    "qt-difference",
    "qt-exclusion",
];

const TEXT_ALIGN: [&str; 5] = ["start", "end", "left", "right", "center"];
const TEXT_BASELINE: [&str; 5] = ["alphabetic", "top", "middle", "bottom", "hanging"];

/// A path call with all-finite arguments on an invertible CTM is recorded.
fn path_op<const N: usize>(
    it: &mut Interp,
    this: &Value,
    a: &[Value],
    f: impl FnOnce([f64; N]) -> PathOp,
) -> R<Value> {
    method(it, this, |c, _| {
        if a.len() < N || !c.state.invertible {
            return;
        }
        let v: [f64; N] = nums(a);
        if finite(&v) {
            c.path.push(f(v));
        }
    })
}

/// A transform call: `f` gives the new CTM and the matrix that maps the path back.
fn transform_op<const N: usize>(
    it: &mut Interp,
    this: &Value,
    a: &[Value],
    f: impl FnOnce(&M, [f64; N]) -> (M, M),
) -> R<Value> {
    method(it, this, |c, out| {
        if a.len() < N || !c.state.invertible {
            return;
        }
        let v: [f64; N] = nums(a);
        if !finite(&v) {
            return;
        }
        let (m, back) = f(&c.state.matrix, v);
        if !m_invertible(&m) {
            c.state.invertible = false;
            return;
        }
        c.state.matrix = m;
        out.push(CanvasOp::Matrix(m));
        c.map_path(back);
    })
}

fn negative_radius<T>(it: &Interp) -> R<T> {
    throw_dom(it, INDEX_SIZE_ERR, "Incorrect argument radius")
}

/// fillText / strokeText: the glyph path of `a[0]` placed by textAlign and textBaseline.
fn text_path(c: &Ctx, a: &[Value]) -> Option<CanvasPath> {
    let (x, y) = (num(a, 1), num(a, 2));
    if a.len() < 3 || !finite(&[x, y]) {
        return None;
    }
    Some(Rc::new(vec![PathOp::Text {
        text: a[0].to_js_string().into(),
        x,
        y,
        font: c.state.font.clone(),
        align: c.state.text_align,
        baseline: c.state.text_baseline,
    }]))
}

fn set_style(it: &mut Interp, this: &Value, a: &[Value], fill: bool) -> R<Value> {
    let v = crate::valuetypes::plain(a.first().cloned().unwrap_or_default());
    setter(it, this, |c, out| {
        let cur = if fill { &c.state.fill } else { &c.state.stroke };
        let new = match &v {
            Value::Str(_) | Value::Str16(_) => {
                let col = color_from_string(&v.to_js_string());
                if col == Color::INVALID || *cur == Style::Color(col) {
                    return;
                }
                Style::Color(col)
            }
            // A QColor is recorded even when it is the current one.
            Value::Color(col) if *col != Color::INVALID => Style::Color(*col),
            Value::Obj(_) | Value::Item(_) | Value::Color(_) => match style_obj(&v) {
                Some(s) => {
                    let b = s.brush.borrow();
                    if matches!(cur, Style::Brush(old) if **old == *b) {
                        return;
                    }
                    Style::Brush(Rc::new(b.clone()))
                }
                // Only strokeStyle falls back to QBrush(QColor()) for an unknown object.
                None if !fill && *cur != Style::Color(Color::INVALID) => {
                    Style::Color(Color::INVALID)
                }
                None => return,
            },
            _ => return,
        };
        out.push(style_op(&new, fill));
        if fill {
            c.state.fill = new;
            c.fill_js = v.clone();
        } else {
            c.state.stroke = new;
            c.stroke_js = v.clone();
        }
    })
}

fn get_style(it: &mut Interp, this: &Value, fill: bool) -> R<Value> {
    getter(it, this, |c| {
        let (s, js) = if fill {
            (&c.state.fill, &c.fill_js)
        } else {
            (&c.state.stroke, &c.stroke_js)
        };
        match s.color() {
            Some(col) => Value::str(&color_string(col)),
            None => js.clone(),
        }
    })
}

/// A numeric property that takes finite values `ok` accepts; `-1` stands for a missing
/// argument as in Qt.
fn set_num(
    it: &mut Interp,
    this: &Value,
    a: &[Value],
    ok: fn(f64) -> bool,
    field: fn(&mut State) -> &mut f64,
    op: fn(f64) -> Option<CanvasOp>,
) -> R<Value> {
    let v = a.first().map_or(-1.0, |v| v.to_number());
    setter(it, this, |c, out| {
        let f = field(&mut c.state);
        if v.is_finite() && ok(v) && *f != v {
            *f = v;
            out.extend(op(v));
        }
    })
}

/// A string-enumerated property: the index of the argument in `names` is stored.
fn set_enum(
    it: &mut Interp,
    this: &Value,
    a: &[Value],
    names: &[&str],
    field: fn(&mut State) -> &mut u8,
    op: fn(u8) -> Option<CanvasOp>,
) -> R<Value> {
    let Some(v) = a.first() else {
        return Ok(Value::Undefined);
    };
    let s = v.to_js_string();
    setter(it, this, |c, out| {
        let Some(k) = names.iter().position(|n| *n == s) else {
            return;
        };
        let f = field(&mut c.state);
        if *f != k as u8 {
            *f = k as u8;
            out.extend(op(k as u8));
        }
    })
}

type Accessor = (&'static str, NativeFn, Option<NativeFn>);

const ACCESSORS: &[Accessor] = &[
    (
        "strokeStyle",
        |it, t, _| get_style(it, t, false),
        Some(|it, t, a| set_style(it, t, a, false)),
    ),
    (
        "fillStyle",
        |it, t, _| get_style(it, t, true),
        Some(|it, t, a| set_style(it, t, a, true)),
    ),
    (
        "fillRule",
        |it, t, _| {
            getter(it, t, |c| {
                Value::Num(if c.state.odd_even { 0.0 } else { 1.0 })
            })
        },
        Some(|it, t, a| {
            let v = a.first().cloned().unwrap_or_default();
            setter(it, t, |c, _| {
                match &v {
                    Value::Str(s) if &**s == "WindingFill" => c.state.odd_even = false,
                    Value::Str(s) if &**s == "OddEvenFill" => c.state.odd_even = true,
                    Value::Num(n) if *n == 1.0 => c.state.odd_even = false,
                    Value::Num(n) if *n == 0.0 => c.state.odd_even = true,
                    _ => {}
                }
                // QPainterPath::setFillRule gives a path without data its first element,
                // a move to (0, 0), and from then on elementCount() is not 0.
                if c.path.is_empty() {
                    c.path.push(PathOp::MoveTo(0.0, 0.0));
                }
                c.path_odd_even = c.state.odd_even;
            })
        }),
    ),
    (
        "globalAlpha",
        |it, t, _| getter(it, t, |c| Value::Num(c.state.global_alpha)),
        Some(|it, t, a| {
            let v = a.first().map_or(f64::NAN, |v| v.to_number());
            setter(it, t, |c, out| {
                if (0.0..=1.0).contains(&v) && c.state.global_alpha != v {
                    c.state.global_alpha = v;
                    out.push(CanvasOp::SetGlobalAlpha(v));
                }
            })
        }),
    ),
    (
        "globalCompositeOperation",
        |it, t, _| {
            getter(it, t, |c| {
                // qt_composite_mode_to_string names Lighten "lighter" too.
                Value::str(match c.state.composite {
                    17 => "lighter",
                    k => COMPOSITE[k as usize],
                })
            })
        },
        Some(|it, t, a| {
            let Some(v) = a.first() else {
                return it.throw("TypeError", "Type error");
            };
            let s = v.to_js_string();
            setter(it, t, |c, out| {
                let Some(k) = COMPOSITE.iter().position(|n| *n == s) else {
                    return;
                };
                if c.state.composite != k as u8 {
                    c.state.composite = k as u8;
                    out.push(CanvasOp::SetComposite(k as u8));
                }
            })
        }),
    ),
    (
        "lineWidth",
        |it, t, _| getter(it, t, |c| Value::Num(c.state.line_width)),
        Some(|it, t, a| {
            set_num(
                it,
                t,
                a,
                |v| v > 0.0,
                |s| &mut s.line_width,
                |v| Some(CanvasOp::SetLineWidth(v)),
            )
        }),
    ),
    (
        "lineCap",
        |it, t, _| {
            getter(it, t, |c| {
                Value::str(["butt", "square", "round"][c.state.cap as usize])
            })
        },
        Some(|it, t, a| {
            set_enum(
                it,
                t,
                a,
                &["butt", "square", "round"],
                |s| &mut s.cap,
                |k| Some(CanvasOp::SetCap(k)),
            )
        }),
    ),
    (
        "lineJoin",
        |it, t, _| {
            getter(it, t, |c| {
                Value::str(["miter", "miter", "round", "bevel"][c.state.join as usize])
            })
        },
        Some(|it, t, a| {
            // "miter" is Qt::SvgMiterJoin; the initial join is Qt::MiterJoin.
            set_enum(
                it,
                t,
                a,
                &["\0", "miter", "round", "bevel"],
                |s| &mut s.join,
                |k| Some(CanvasOp::SetJoin(k)),
            )
        }),
    ),
    (
        "lineDashOffset",
        |it, t, _| getter(it, t, |c| Value::Num(c.state.dash_offset)),
        Some(|it, t, a| {
            set_num(
                it,
                t,
                a,
                |_| true,
                |s| &mut s.dash_offset,
                |v| Some(CanvasOp::SetDashOffset(v)),
            )
        }),
    ),
    (
        "miterLimit",
        |it, t, _| getter(it, t, |c| Value::Num(c.state.miter_limit)),
        Some(|it, t, a| {
            set_num(
                it,
                t,
                a,
                |v| v > 0.0,
                |s| &mut s.miter_limit,
                |v| Some(CanvasOp::SetMiterLimit(v)),
            )
        }),
    ),
    (
        "shadowBlur",
        |it, t, _| getter(it, t, |c| Value::Num(c.state.shadow_blur)),
        Some(|it, t, a| {
            set_num(
                it,
                t,
                a,
                |v| v > 0.0,
                |s| &mut s.shadow_blur,
                |v| Some(CanvasOp::SetShadowBlur(v)),
            )
        }),
    ),
    (
        "shadowColor",
        |it, t, _| {
            getter(it, t, |c| {
                let [r, g, b, _] = c.state.shadow_color;
                Value::str(&format!("#{r:02x}{g:02x}{b:02x}"))
            })
        },
        Some(|it, t, a| {
            let col = a.first().map_or(Color::INVALID, value_color);
            setter(it, t, |c, out| {
                if col == Color::INVALID {
                    return;
                }
                let v = col.to_rgb().rgba8();
                if c.state.shadow_color != v {
                    c.state.shadow_color = v;
                    out.push(CanvasOp::SetShadowColor(v));
                }
            })
        }),
    ),
    (
        "shadowOffsetX",
        |it, t, _| getter(it, t, |c| Value::Num(c.state.shadow_ox)),
        Some(|it, t, a| {
            let v = a.first().map_or(f64::NAN, |v| v.to_number());
            setter(it, t, |c, out| {
                if v.is_finite() && c.state.shadow_ox != v {
                    c.state.shadow_ox = v;
                    out.push(CanvasOp::SetShadowOffsetX(v));
                }
            })
        }),
    ),
    (
        "shadowOffsetY",
        |it, t, _| getter(it, t, |c| Value::Num(c.state.shadow_oy)),
        Some(|it, t, a| {
            let v = a.first().map_or(f64::NAN, |v| v.to_number());
            setter(it, t, |c, out| {
                if v.is_finite() && c.state.shadow_oy != v {
                    c.state.shadow_oy = v;
                    out.push(CanvasOp::SetShadowOffsetY(v));
                }
            })
        }),
    ),
    (
        "font",
        |it, t, _| getter(it, t, |c| Value::str(&c.state.font.qt_string())),
        Some(|it, t, a| {
            let s = a.first().cloned().unwrap_or_default().to_js_string();
            let font = font_from_string(&s);
            setter(it, t, |c, _| {
                if let Some(f) = font.filter(|f| *f != *c.state.font) {
                    c.state.font = Rc::new(f);
                }
            })
        }),
    ),
    (
        "textAlign",
        |it, t, _| {
            getter(it, t, |c| {
                Value::str(TEXT_ALIGN[c.state.text_align as usize])
            })
        },
        Some(|it, t, a| {
            let v = [a.first().cloned().unwrap_or_default()];
            set_enum(it, t, &v, &TEXT_ALIGN, |s| &mut s.text_align, |_| None)
        }),
    ),
    (
        "textBaseline",
        |it, t, _| {
            getter(it, t, |c| {
                Value::str(TEXT_BASELINE[c.state.text_baseline as usize])
            })
        },
        Some(|it, t, a| {
            let v = [a.first().cloned().unwrap_or_default()];
            set_enum(
                it,
                t,
                &v,
                &TEXT_BASELINE,
                |s| &mut s.text_baseline,
                |_| None,
            )
        }),
    ),
    (
        "path",
        |it, t, _| getter(it, t, |c| c.path_js.clone()),
        Some(|it, t, a| {
            let v = a.first().cloned().unwrap_or_default();
            setter(it, t, |c, _| {
                c.begin_path();
                if !matches!(v, Value::Item(_)) {
                    let s = v.to_js_string();
                    if !s.trim().is_empty() {
                        c.path_odd_even = true;
                        c.path.push(PathOp::Svg(s.into()));
                    }
                }
                c.path_js = v.clone();
            })
        }),
    ),
    (
        "canvas",
        |it, t, _| match context2d(t) {
            Some(cx) => Ok(Value::Item(cx.item)),
            None => it.throw("Error", "Not a Context2D object"),
        },
        None,
    ),
];

const METHODS: &[(&str, NativeFn)] = &[
    ("save", |it, t, _| {
        method(it, t, |c, _| c.stack.push(c.state.clone()))
    }),
    ("restore", |it, t, _| {
        method(it, t, |c, out| c.pop_state(out))
    }),
    ("reset", |it, t, _| {
        let size = context2d(t)
            .zip(world_of(it))
            .map(|(cx, w)| canvas_size(&mut w.borrow_mut(), cx.item));
        method(it, t, |c, out| {
            c.path.clear();
            c.path_odd_even = true;
            c.stack = vec![State::default()];
            c.pop_state(out);
        })?;
        let (w, h) = size.unwrap_or_default();
        clear_rect(it, t, [0.0, 0.0, w, h])
    }),
    ("scale", |it, t, a| {
        transform_op(it, t, a, |m, [x, y]: [f64; 2]| {
            (m_scale(m, x, y), m_scale(&IDENTITY, 1.0 / x, 1.0 / y))
        })
    }),
    ("rotate", |it, t, a| {
        transform_op(it, t, a, |m, [r]: [f64; 1]| {
            let deg = r.to_degrees();
            (m_rotate(m, deg), m_rotate(&IDENTITY, -deg))
        })
    }),
    ("shear", |it, t, a| {
        transform_op(it, t, a, |m, [h, v]: [f64; 2]| {
            (m_shear(m, h, v), m_shear(&IDENTITY, -h, -v))
        })
    }),
    ("translate", |it, t, a| {
        transform_op(it, t, a, |m, [x, y]: [f64; 2]| {
            (m_translate(m, x, y), m_translate(&IDENTITY, -x, -y))
        })
    }),
    ("transform", |it, t, a| {
        method(it, t, |c, out| {
            if a.len() >= 6 {
                c.transform(nums(a), out);
            }
        })
    }),
    ("setTransform", |it, t, a| {
        method(it, t, |c, out| {
            if a.len() >= 6 {
                c.set_transform(nums(a), out);
            }
        })
    }),
    ("resetTransform", |it, t, _| {
        method(it, t, |c, out| c.set_transform(IDENTITY, out))
    }),
    ("beginPath", |it, t, _| method(it, t, |c, _| c.begin_path())),
    ("closePath", |it, t, _| {
        method(it, t, |c, _| {
            if !c.path.is_empty() {
                c.path.push(PathOp::Close);
            }
        })
    }),
    ("moveTo", |it, t, a| {
        path_op(it, t, a, |[x, y]: [f64; 2]| PathOp::MoveTo(x, y))
    }),
    ("lineTo", |it, t, a| {
        path_op(it, t, a, |[x, y]: [f64; 2]| PathOp::LineTo(x, y))
    }),
    ("bezierCurveTo", |it, t, a| path_op(it, t, a, PathOp::Cubic)),
    ("quadraticCurveTo", |it, t, a| {
        path_op(it, t, a, PathOp::Quad)
    }),
    ("arc", |it, t, a| {
        if a.len() < 5 {
            return method(it, t, |_, _| {});
        }
        let v: [f64; 5] = nums(a);
        if v[2].is_finite() && v[2] < 0.0 {
            return negative_radius(it);
        }
        let ccw = a.len() == 6 && a[5].truthy();
        method(it, t, |c, _| {
            if c.state.invertible && finite(&v) && v[3] != v[4] {
                c.path.push(PathOp::Arc(v, ccw));
            }
        })
    }),
    ("arcTo", |it, t, a| {
        if a.len() >= 5 && num(a, 4).is_finite() && num(a, 4) < 0.0 {
            return negative_radius(it);
        }
        path_op(it, t, a, PathOp::ArcTo)
    }),
    ("rect", |it, t, a| path_op(it, t, a, PathOp::Rect)),
    ("roundedRect", |it, t, a| {
        path_op(it, t, a, PathOp::RoundedRect)
    }),
    ("ellipse", |it, t, a| path_op(it, t, a, PathOp::Ellipse)),
    ("text", |it, t, a| {
        let (x, y) = (num(a, 1), num(a, 2));
        if a.len() >= 3 && !finite(&[x, y]) {
            return Ok(Value::Undefined);
        }
        method(it, t, |c, _| {
            let text = a.first().map(|v| v.to_js_string()).unwrap_or_default();
            if a.len() >= 3 && c.state.invertible && !text.is_empty() {
                c.path.push(PathOp::Text {
                    text: text.into(),
                    x,
                    y,
                    font: c.state.font.clone(),
                    align: RAW_TEXT,
                    baseline: 0,
                });
            }
        })
    }),
    ("fill", |it, t, _| {
        method(it, t, |c, out| {
            if c.state.invertible && !c.path.is_empty() {
                c.path_odd_even = c.state.odd_even;
                out.push(CanvasOp::Fill(c.path_snapshot(), c.state.odd_even));
            }
        })
    }),
    ("stroke", |it, t, _| {
        method(it, t, |c, out| {
            if c.state.invertible && !c.path.is_empty() {
                out.push(CanvasOp::Stroke(c.path_snapshot()));
            }
        })
    }),
    ("clip", |it, t, _| {
        method(it, t, |c, out| {
            if !c.state.invertible {
                return;
            }
            let mut paths = if c.state.clip {
                (*c.state.clip_path).clone()
            } else {
                Vec::new()
            };
            paths.push((c.path_snapshot(), c.path_odd_even));
            c.state.clip_path = Rc::new(paths);
            c.state.clip = true;
            out.push(CanvasOp::Clip(true, c.state.clip_path.clone()));
        })
    }),
    ("fillRect", |it, t, a| {
        method(it, t, |c, out| {
            let r: [f64; 4] = nums(a);
            if a.len() >= 4 && c.state.invertible && finite(&r) {
                out.push(CanvasOp::FillRect(r));
            }
        })
    }),
    ("strokeRect", |it, t, a| {
        method(it, t, |c, out| {
            let r: [f64; 4] = nums(a);
            if a.len() >= 4 && c.state.invertible && finite(&r) {
                out.push(CanvasOp::StrokeRect(r));
            }
        })
    }),
    ("clearRect", |it, t, a| {
        if a.len() < 4 {
            return method(it, t, |_, _| {});
        }
        clear_rect(it, t, nums(a))
    }),
    ("fillText", |it, t, a| {
        if a.len() >= 3 && !finite(&[num(a, 1), num(a, 2)]) {
            return Ok(Value::Undefined);
        }
        method(it, t, |c, out| {
            // The glyph path keeps QPainterPath's default fill rule, odd-even.
            out.extend(text_path(c, a).map(|p| CanvasOp::Fill(p, true)));
        })
    }),
    ("strokeText", |it, t, a| {
        method(it, t, |c, out| {
            if c.state.invertible {
                out.extend(text_path(c, a).map(CanvasOp::Stroke));
            }
        })
    }),
    ("measureText", |it, t, a| {
        let Some(text) = a.first() else {
            return Ok(Value::Undefined);
        };
        let font = getter(it, t, |_| Value::Undefined)
            .ok()
            .and_then(|_| context2d(t))
            .map(|cx| cx.s.borrow().state.font.clone());
        let Some(font) = font else {
            return it.throw("Error", "Not a Context2D object");
        };
        let width = host().map_or(0.0, |h| h.measure_text(&font, &text.to_js_string()));
        let o = crate::eval::new_obj();
        // Qt stores the advance in a `uint`.
        crate::eval::set_own(&o, "width", Value::Num(width as i32 as u32 as f64));
        Ok(Value::Obj(o))
    }),
    ("isPointInPath", |it, t, a| {
        let Some(cx) = context2d(t) else {
            return it.throw("Error", "Not a Context2D object");
        };
        let (x, y) = (num(a, 0), num(a, 1));
        let mut c = cx.s.borrow_mut();
        if a.len() < 2 || !c.state.invertible || c.path.is_empty() || !finite(&[x, y]) {
            return Ok(Value::Bool(false));
        }
        // Qt maps the point by the inverse CTM although the path is in user space already.
        let m = m_inverted(&c.state.matrix);
        let p = [m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]];
        if !finite(&p) {
            return Ok(Value::Bool(false));
        }
        c.path_odd_even = c.state.odd_even;
        Ok(Value::Bool(host().is_some_and(|h| {
            h.point_in_path(&c.path, c.state.odd_even, p)
        })))
    }),
    ("createLinearGradient", |it, t, a| {
        if a.len() < 4 {
            return Ok(t.clone());
        }
        let v: [f64; 4] = nums(a);
        if !finite(&v) {
            return throw_dom(
                it,
                NOT_SUPPORTED_ERR,
                "createLinearGradient(): Incorrect arguments",
            );
        }
        Ok(new_style(CanvasBrush::Linear(v, Vec::new()), true))
    }),
    ("createRadialGradient", |it, t, a| {
        if a.len() < 6 {
            return Ok(t.clone());
        }
        let [x0, y0, r0, x1, y1, r1]: [f64; 6] = nums(a);
        if !finite(&[x0, y0, r0, x1, y1, r1]) {
            return throw_dom(
                it,
                NOT_SUPPORTED_ERR,
                "createRadialGradient(): Incorrect arguments",
            );
        }
        if r0 < 0.0 || r1 < 0.0 {
            return throw_dom(
                it,
                INDEX_SIZE_ERR,
                "createRadialGradient(): Incorrect arguments",
            );
        }
        Ok(new_style(
            CanvasBrush::Radial([x1, y1, r1, x0, y0, r0], Vec::new()),
            true,
        ))
    }),
    ("createConicalGradient", |it, t, a| {
        if a.len() < 3 {
            return Ok(t.clone());
        }
        let [x, y, r]: [f64; 3] = nums(a);
        if !finite(&[x, y]) {
            return throw_dom(
                it,
                NOT_SUPPORTED_ERR,
                "createConicalGradient(): Incorrect arguments",
            );
        }
        if !r.is_finite() {
            return throw_dom(
                it,
                INDEX_SIZE_ERR,
                "createConicalGradient(): Incorrect arguments",
            );
        }
        Ok(new_style(
            CanvasBrush::Conical([x, y, r.to_degrees()], Vec::new()),
            true,
        ))
    }),
    ("createPattern", |_, _, a| {
        if a.len() < 2 {
            return Ok(Value::Undefined);
        }
        // toVariant(QColor): a colour, or a string QColor::fromString reads.
        let color = match &crate::valuetypes::plain(a[0].clone()) {
            Value::Color(c) => *c,
            Value::Str(_) | Value::Str16(_) => crate::builtins::parse_color(&a[0].to_js_string()),
            _ => Color::INVALID,
        };
        if color != Color::INVALID {
            // Qt::BrushStyle below LinearGradientPattern (15), else SolidPattern.
            let k = a[1].to_number();
            let k = if k.is_finite() { k.trunc() as i64 } else { 0 };
            let style = if (0..15).contains(&k) { k as u8 } else { 1 };
            return Ok(new_style(
                CanvasBrush::Pattern(color.to_rgb().rgba8(), style),
                false,
            ));
        }
        let img = match &a[0] {
            Value::Obj(_) => image_data(&a[0]).map(|d| snapshot(&d)),
            Value::Item(_) => None,
            v => load_image(&v.to_js_string()),
        };
        Ok(new_style(
            match img.filter(|i| i.w > 0 && i.h > 0) {
                Some(i) => CanvasBrush::Texture(i),
                None => CanvasBrush::None,
            },
            false,
        ))
    }),
    ("setLineDash", |it, t, a| {
        let dash = match a.first() {
            Some(Value::Obj(o)) => o.borrow().array.clone(),
            _ => None,
        };
        let Some(dash) = dash else {
            return Ok(Value::Undefined);
        };
        let mut d: Vec<f64> = dash.iter().map(|v| v.to_number()).collect();
        if d.iter().any(|n| !n.is_finite() || *n < 0.0) {
            return Ok(Value::Undefined);
        }
        if !d.len().is_multiple_of(2) {
            d.extend(d.clone());
        }
        setter(it, t, |c, out| {
            c.state.dash = d.clone();
            out.push(CanvasOp::SetDash(d));
        })
    }),
    ("getLineDash", |it, t, _| {
        getter(it, t, |c| {
            Value::array(c.state.dash.iter().map(|n| Value::Num(*n)).collect())
        })
    }),
    ("drawImage", draw_image),
    ("getImageData", get_image_data),
    ("createImageData", create_image_data),
    ("putImageData", put_image_data),
    ("drawFocusRing", |it, _, _| unsupported(it, "drawFocusRing")),
    ("setCaretSelectionRect", |it, _, _| {
        unsupported(it, "setCaretSelectionRect")
    }),
    ("caretBlinkRate", |it, _, _| {
        unsupported(it, "caretBlinkRate")
    }),
];

/// The context object `getContext("2d")` returns for Canvas `item`.
pub(crate) fn new_context(item: usize) -> Value {
    let o = crate::eval::new_obj();
    let black = Value::str("#000000");
    o.borrow_mut().native = Some(Rc::new(Context2d {
        item,
        s: RefCell::new(Ctx {
            state: State::default(),
            stack: Vec::new(),
            path: Vec::new(),
            path_odd_even: true,
            fill_js: black.clone(),
            stroke_js: black,
            path_js: Value::Undefined,
        }),
    }));
    for (k, get, set) in ACCESSORS {
        crate::eval::define_accessor(&o, k, true, native_fn(k, *get), false);
        if let Some(set) = set {
            crate::eval::define_accessor(&o, k, false, native_fn(k, *set), false);
        }
    }
    for (k, f) in METHODS {
        crate::eval::set_own(&o, k, native_fn(k, *f));
    }
    Value::Obj(o)
}
