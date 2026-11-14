//! Test-only reference: the hand-written SVG reader `svg.rs` had before the roxmltree/svgtypes
//! switch (commit 642a481), which `svg::equiv` compares today's reader with: the
//! XML tree, numbers, colors, transforms, path data, `url()` references and the document size.
//! Its drawing is gone: `svg.rs` draws as Qt does and is compared with Qt itself
//! (`tests/svg_stroke_qt.rs`).
#![allow(clippy::all)]
use crate::canvas::El;

/// a b c d e f: x' = a x + c y + e, y' = b x + d y + f.
pub(super) type M = [f64; 6];
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

pub(super) struct Elem {
    pub(super) name: String,
    pub(super) attrs: Vec<(String, String)>,
    pub(super) children: Vec<Elem>,
}

impl Elem {
    fn attr(&self, k: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(a, _)| a == k)
            .map(|(_, v)| v.as_str())
    }
}

pub(super) fn unescape(s: &str) -> String {
    s.replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

pub(super) fn parse_xml(src: &str) -> Option<Elem> {
    let b = src.as_bytes();
    let mut i = 0;
    let mut stack: Vec<Elem> = vec![Elem {
        name: String::new(),
        attrs: vec![],
        children: vec![],
    }];
    while i < b.len() {
        if b[i] != b'<' {
            i += 1;
            continue;
        }
        if src[i..].starts_with("<!--") {
            i += src[i..].find("-->")? + 3;
            continue;
        }
        if src[i..].starts_with("<?") || src[i..].starts_with("<!") {
            i += src[i..].find('>')? + 1;
            continue;
        }
        if src[i..].starts_with("</") {
            i += src[i..].find('>')? + 1;
            let e = stack.pop()?;
            stack.last_mut()?.children.push(e);
            continue;
        }
        i += 1;
        let st = i;
        while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'>' && b[i] != b'/' {
            i += 1;
        }
        let mut e = Elem {
            name: src[st..i].to_string(),
            attrs: vec![],
            children: vec![],
        };
        loop {
            while i < b.len() && b[i].is_ascii_whitespace() {
                i += 1;
            }
            if i >= b.len() {
                return None;
            }
            if b[i] == b'/' {
                i += src[i..].find('>')? + 1;
                stack.last_mut()?.children.push(e);
                break;
            }
            if b[i] == b'>' {
                i += 1;
                stack.push(e);
                break;
            }
            let ks = i;
            while i < b.len() && b[i] != b'=' && !b[i].is_ascii_whitespace() {
                i += 1;
            }
            let k = src[ks..i].to_string();
            while i < b.len() && b[i] != b'"' && b[i] != b'\'' {
                i += 1;
            }
            let q = b[i];
            i += 1;
            let vs = i;
            while i < b.len() && b[i] != q {
                i += 1;
            }
            e.attrs.push((k, unescape(&src[vs..i])));
            i += 1;
        }
    }
    let mut root = stack.pop()?;
    root.children.pop()
}

pub(super) fn num_prefix(s: &str) -> f64 {
    let end = s
        .find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E')))
        .unwrap_or(s.len());
    s[..end].parse().unwrap_or(0.0)
}

pub(super) fn parse_color(v: &str) -> Option<Option<[u8; 4]>> {
    let v = v.trim();
    if v == "none" || v == "transparent" {
        return Some(None);
    }
    if let Some(h) = v.strip_prefix('#') {
        let p = |s: &str| u8::from_str_radix(s, 16).ok();
        return match h.len() {
            3 => {
                let d = |i: usize| p(&h[i..i + 1]).map(|x| x * 17);
                Some(Some([d(0)?, d(1)?, d(2)?, 255]))
            }
            6 => Some(Some([p(&h[0..2])?, p(&h[2..4])?, p(&h[4..6])?, 255])),
            _ => None,
        };
    }
    if let Some(r) = v.strip_prefix("rgb(") {
        let c: Vec<u8> = r
            .trim_end_matches(')')
            .split(',')
            .map(|t| t.trim().parse::<f64>().unwrap_or(0.0).clamp(0.0, 255.0) as u8)
            .collect();
        return Some(Some([*c.first()?, *c.get(1)?, *c.get(2)?, 255]));
    }
    Some(Some(match v {
        "black" | "currentColor" => [0, 0, 0, 255],
        "white" => [255, 255, 255, 255],
        "red" => [255, 0, 0, 255],
        "green" => [0, 128, 0, 255],
        "blue" => [0, 0, 255, 255],
        "gray" | "grey" => [128, 128, 128, 255],
        _ => {
            if v.starts_with("url(") {
                elog::eprintln!("qsg::svg: paint servers not supported yet ({v})");
            }
            return None;
        }
    }))
}

pub(super) fn nums(s: &str) -> Vec<f64> {
    let mut out = vec![];
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_digit() || c == b'.' || c == b'-' || c == b'+' {
            let st = i;
            i += 1;
            let mut dot = c == b'.';
            while i < b.len() {
                let d = b[i];
                if d.is_ascii_digit() {
                    i += 1;
                } else if d == b'.' && !dot {
                    dot = true;
                    i += 1;
                } else if (d == b'e' || d == b'E')
                    && i + 1 < b.len()
                    && (b[i + 1].is_ascii_digit() || b[i + 1] == b'-' || b[i + 1] == b'+')
                {
                    i += 2;
                } else {
                    break;
                }
            }
            out.push(s[st..i].parse().unwrap_or(0.0));
        } else {
            i += 1;
        }
    }
    out
}

pub(super) fn parse_transform(s: &str) -> M {
    let mut acc = ID;
    for part in s.split(')') {
        let Some((name, args)) = part.split_once('(') else {
            continue;
        };
        let a = nums(args);
        let g = |i: usize, d: f64| a.get(i).copied().unwrap_or(d);
        let t = match name.trim().trim_start_matches(',').trim() {
            "matrix" => [g(0, 1.), g(1, 0.), g(2, 0.), g(3, 1.), g(4, 0.), g(5, 0.)],
            "translate" => [1., 0., 0., 1., g(0, 0.), g(1, 0.)],
            "scale" => [g(0, 1.), 0., 0., g(1, g(0, 1.)), 0., 0.],
            "rotate" => {
                let r = g(0, 0.).to_radians();
                let (s, c) = r.sin_cos();
                let (cx, cy) = (g(1, 0.), g(2, 0.));
                compose(
                    compose([1., 0., 0., 1., -cx, -cy], [c, s, -s, c, 0., 0.]),
                    [1., 0., 0., 1., cx, cy],
                )
            }
            "skewX" => [1., 0., g(0, 0.).to_radians().tan(), 1., 0., 0.],
            "skewY" => [1., g(0, 0.).to_radians().tan(), 0., 1., 0., 0.],
            _ => ID,
        };
        acc = compose(t, acc);
    }
    acc
}

/// User-space QPainterPath elements (Move/Line/Cubic, closes as Line back to start).
pub(super) type UPath = Vec<El>;

fn arc_segment(p: &mut UPath, xc: f64, yc: f64, th0: f64, th1: f64, rx: f64, ry: f64, xrot: f64) {
    let (sin_th, cos_th) = xrot.to_radians().sin_cos();
    let (a00, a01, a10, a11) = (cos_th * rx, -sin_th * ry, sin_th * rx, cos_th * ry);
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
    for i in 0..n_segs {
        let a = th0 + i as f64 * th_arc / n_segs as f64;
        let b = th0 + (i + 1) as f64 * th_arc / n_segs as f64;
        arc_segment(p, xc, yc, a, b, rx, ry, xrot);
    }
}

pub(super) fn parse_path(d: &str) -> UPath {
    let mut p: UPath = vec![];
    let b = d.as_bytes();
    let (mut cur, mut start) = ([0.0f64; 2], [0.0f64; 2]);
    let mut last_ctrl: Option<[f64; 2]> = None;
    let mut last_q: Option<[f64; 2]> = None;
    let mut i = 0;
    let mut cmd = b' ';
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_alphabetic() && c != b'e' && c != b'E' {
            cmd = c;
            i += 1;
            if cmd == b'z' || cmd == b'Z' {
                p.push(El::Line(start));
                cur = start;
                last_ctrl = None;
                last_q = None;
            }
            continue;
        }
        if !(c.is_ascii_digit() || c == b'-' || c == b'+' || c == b'.') {
            i += 1;
            continue;
        }
        let argc = match cmd.to_ascii_lowercase() {
            b'm' | b'l' | b't' => 2,
            b'h' | b'v' => 1,
            b'c' => 6,
            b's' | b'q' => 4,
            b'a' => 7,
            _ => return p,
        };
        let mut a = [0.0f64; 7];
        for (j, slot) in a.iter_mut().enumerate().take(argc) {
            let is_flag = argc == 7 && (j == 3 || j == 4);
            while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b',') {
                i += 1;
            }
            let st = i;
            if i < b.len() && (b[i] == b'-' || b[i] == b'+') {
                i += 1;
            }
            let mut dot = false;
            let mut digits = 0;
            while i < b.len() {
                let d = b[i];
                if d.is_ascii_digit() {
                    i += 1;
                    digits += 1;
                    // arc flags are single digits that may be glued together ("a1 1 0 015 5")
                    if is_flag {
                        break;
                    }
                } else if d == b'.' && !dot {
                    dot = true;
                    i += 1;
                } else if (d == b'e' || d == b'E') && digits > 0 {
                    i += 1;
                    if i < b.len() && (b[i] == b'-' || b[i] == b'+') {
                        i += 1;
                    }
                } else {
                    break;
                }
            }
            *slot = d[st..i].parse().unwrap_or(0.0);
        }
        let rel = cmd.is_ascii_lowercase();
        let o = if rel { cur } else { [0.0, 0.0] };
        let pt = |x: f64, y: f64| [o[0] + x, o[1] + y];
        let mut ctrl = None;
        let mut q = None;
        match cmd.to_ascii_lowercase() {
            b'm' => {
                cur = pt(a[0], a[1]);
                start = cur;
                p.push(El::Move(cur));
                cmd = if rel { b'l' } else { b'L' };
            }
            b'l' => {
                cur = pt(a[0], a[1]);
                p.push(El::Line(cur));
            }
            b'h' => {
                cur = [if rel { cur[0] + a[0] } else { a[0] }, cur[1]];
                p.push(El::Line(cur));
            }
            b'v' => {
                cur = [cur[0], if rel { cur[1] + a[0] } else { a[0] }];
                p.push(El::Line(cur));
            }
            b'c' => {
                let (c1, c2, e) = (pt(a[0], a[1]), pt(a[2], a[3]), pt(a[4], a[5]));
                p.push(El::Cubic(c1, c2, e));
                ctrl = Some(c2);
                cur = e;
            }
            b's' => {
                let c1 = last_ctrl.map_or(cur, |c| [2.0 * cur[0] - c[0], 2.0 * cur[1] - c[1]]);
                let (c2, e) = (pt(a[0], a[1]), pt(a[2], a[3]));
                p.push(El::Cubic(c1, c2, e));
                ctrl = Some(c2);
                cur = e;
            }
            b'q' | b't' => {
                let qc = if cmd.to_ascii_lowercase() == b'q' {
                    pt(a[0], a[1])
                } else {
                    last_q.map_or(cur, |c| [2.0 * cur[0] - c[0], 2.0 * cur[1] - c[1]])
                };
                let e = if cmd.to_ascii_lowercase() == b'q' {
                    pt(a[2], a[3])
                } else {
                    pt(a[0], a[1])
                };
                let c1 = [
                    cur[0] + 2.0 / 3.0 * (qc[0] - cur[0]),
                    cur[1] + 2.0 / 3.0 * (qc[1] - cur[1]),
                ];
                let c2 = [
                    e[0] + 2.0 / 3.0 * (qc[0] - e[0]),
                    e[1] + 2.0 / 3.0 * (qc[1] - e[1]),
                ];
                p.push(El::Cubic(c1, c2, e));
                q = Some(qc);
                cur = e;
            }
            b'a' => {
                let e = pt(a[5], a[6]);
                path_arc(
                    &mut p,
                    a[0],
                    a[1],
                    a[2],
                    a[3] != 0.0,
                    a[4] != 0.0,
                    e[0],
                    e[1],
                    cur[0],
                    cur[1],
                );
                cur = e;
            }
            _ => {}
        }
        last_ctrl = ctrl;
        last_q = q;
    }
    p
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

pub(super) fn url_id(v: &str) -> Option<String> {
    let v = v.trim().strip_prefix("url(")?;
    let v = v
        .split(')')
        .next()?
        .trim()
        .trim_matches(|c| c == '"' || c == '\'');
    v.strip_prefix('#').map(str::to_string)
}

pub struct Doc {
    width: f64,
    height: f64,
}

impl Doc {
    /// `load` with KIconLoader's recoloring: the text of `<style id="current-color-scheme">`
    /// is replaced by `scheme` (KIconLoaderPrivate::processSvg).
    pub fn load_styled(path: &str, scheme: Option<&str>) -> std::io::Result<Doc> {
        let mut src = std::fs::read_to_string(path)?;
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
        Ok(Doc { width, height })
    }

    /// QSvgRenderer::defaultSize (document size truncated to int).
    pub fn default_size(&self) -> (i32, i32) {
        (self.width as i32, self.height as i32)
    }
}
