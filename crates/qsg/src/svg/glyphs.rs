//! `<text>`: the QTextLayout that QSvgText::draw_helper lays out and draws, with the glyphs
//! QFontEngineFT makes for it and the raster engine's two ways of painting them
//! (QRasterPaintEngine::drawTextItem: cached A8 glyphs, or the outline as a path).

use super::effects::{map_rect, to_aligned_rect};
use super::gradient::source_over;
use super::{Builder, Defs, Elem, M, State, UPath, compose, invert, styled};
use crate::canvas::{Context, Path, aliased_line, no_shear, solid_pm};
use crate::pathsimplify::PathEl;
use crate::text::{Font, FontStack, Glyph, fc_match_face};
use std::rc::Rc;

/// What QSvgFontStyle sets on the painter's font, and the white space mode of `xml:space`.
#[derive(Clone)]
pub struct FontStyle {
    /// None: the painter's own font (QGuiApplication::font(), "Sans Serif").
    pub family: Option<String>,
    /// QFont::pointSizeF, which draw_helper takes as the pixel size.
    pub size: f64,
    pub italic: bool,
    pub weight: i32,
    /// 0 start, 1 middle, 2 end.
    pub anchor: u8,
    pub preserve: bool,
}

impl FontStyle {
    pub fn initial() -> FontStyle {
        FontStyle {
            family: None,
            size: 9.0,
            italic: false,
            weight: 400,
            anchor: 0,
            preserve: false,
        }
    }

    /// One of the font properties parseFont reads, or `xml:space`.
    pub fn set(&mut self, k: &str, v: &str) {
        match k {
            "font-family" if !v.is_empty() && v != "inherit" => {
                let mut f = v.trim();
                if f.starts_with(['\'', '"']) {
                    let mut c = f.chars();
                    c.next();
                    c.next_back();
                    f = c.as_str();
                }
                self.family = Some(f.to_string());
            }
            "font-size" => {
                let named = [
                    "xx-small", "x-small", "small", "medium", "large", "x-large", "xx-large",
                ];
                let table = [6.9, 8.3, 10.0, 12.0, 14.4, 17.3, 20.7];
                if let Some(i) = named.iter().position(|n| *n == v) {
                    self.size = table[i];
                } else if let Some(px) = length_px(v) {
                    self.size = px.min(65535.0);
                }
            }
            "font-style" => match v {
                "normal" => self.italic = false,
                "italic" | "oblique" => self.italic = true,
                _ => {}
            },
            "font-weight" => match v {
                "normal" => self.weight = 400,
                "bold" => self.weight = 700,
                "bolder" => self.weight = self.weight.min(800) + 100,
                "lighter" => self.weight = self.weight.max(200) - 100,
                _ => {
                    if let Ok(n) = v.parse::<i32>() {
                        self.weight = n;
                    }
                }
            },
            "text-anchor" => match v {
                "start" => self.anchor = 0,
                "middle" => self.anchor = 1,
                "end" => self.anchor = 2,
                _ => {}
            },
            "xml:space" => self.preserve = v == "preserve",
            _ => {}
        }
    }
}

/// QGuiSvg::parseLength then convertToPixels; None where the number does not parse.
fn length_px(v: &str) -> Option<f64> {
    let s = v.trim();
    let units = [
        ("%", 1.0),
        ("px", 1.0),
        ("pc", 1.0),
        ("pt", 1.25),
        ("mm", 3.543307),
        ("cm", 35.43307),
        ("in", 90.0),
    ];
    let (num, k) = units
        .iter()
        .find_map(|(u, k)| s.strip_suffix(u).map(|n| (n, *k)))
        .unwrap_or((s, 1.0));
    num.trim().parse::<f64>().ok().map(|n| n * k)
}

/// The QFontEngineMulti of a tspan's QFont: pixel size, PreferNoHinting.
fn stack(f: &FontStyle) -> Option<Rc<FontStack>> {
    type Key = (Option<String>, u32, i32, bool);
    thread_local! {
        static STACKS: std::cell::RefCell<std::collections::HashMap<Key, Option<Rc<FontStack>>>> =
            Default::default();
    }
    // QFont::setPixelSize(int): a size of zero or less is refused.
    let px = f.size as i64;
    if px <= 0 {
        return None;
    }
    let weight = f.weight.clamp(100, 900);
    let key = (f.family.clone(), px as u32, weight, f.italic);
    if let Some(s) = STACKS.with(|m| m.borrow().get(&key).cloned()) {
        return s;
    }
    let family = f.family.as_deref().unwrap_or("Sans Serif");
    let face = fc_match_face(family, weight, "", f.italic);
    let axes: &[(&str, f32)] = if f.italic { &[("ital", 1.0)] } else { &[] };
    let s = face
        .and_then(|(path, face_weight)| {
            FontStack::from_path_styled(&path, px as u32, face_weight as f32, weight as f32, axes)
                .ok()
        })
        .map(|s| Rc::new(s.requested(family).no_hinting()));
    STACKS.with(|m| m.borrow_mut().insert(key, s.clone()));
    s
}

/// One QTextLayout::FormatRange of the paragraph.
struct Range {
    text: String,
    s: State,
}

/// QFixed::fromReal: 26.6, truncated.
fn fixed(v: f64) -> i64 {
    (v * 64.0) as i64
}

/// One format range of the line, shaped.
struct Run {
    stack: Rc<FontStack>,
    glyphs: Vec<(usize, u32, i64, i64)>,
    x: i64,
    s: State,
}

/// The one QTextLine of a `text` element, positioned as draw_helper positions it.
struct Line {
    runs: Vec<Run>,
    /// The element's `x` and `y`.
    at: (f64, f64),
    /// QTextLine::setPosition's x: what `text-anchor` moves the line by.
    shift: f64,
    /// naturalTextWidth, ascent and descent, 26.6.
    width: i64,
    ascent: i64,
    descent: i64,
}

/// QSvgText::draw_helper for a `text` element whose style is `s`, in one of the two passes of
/// QSvgNode::fillThenStroke (`s` holds that pass's brush, pen and opacity).
pub fn draw(ctx: &mut Context, defs: Defs, e: &Elem, s: &State) {
    let line = layout(defs, e, s);
    // QTextLineItemIterator: x = fromReal(pos.x) + line.x; the baseline is fromReal(pos.y),
    // the line's y (minus its ascent) and its base cancelling out.
    let (x0, y0) = (fixed(line.at.0) + fixed(line.shift), fixed(line.at.1));
    for r in &line.runs {
        draw_run(ctx, defs, &r.stack, &r.glyphs, (x0 + r.x, y0), &r.s);
    }
}

/// QSvgText::internalBounds: the line's naturalTextRect under the painter's transform.
pub fn bounds(defs: Defs, e: &Elem, s: &State) -> [f64; 4] {
    let line = layout(defs, e, s);
    let rect = [
        line.at.0 + line.shift,
        line.at.1 - line.ascent as f64 / 64.0,
        line.width as f64 / 64.0,
        (line.ascent + line.descent) as f64 / 64.0,
    ];
    super::map_rect(&s.m, rect)
}

fn layout(defs: Defs, e: &Elem, s: &State) -> Line {
    let mut ranges: Vec<Range> = Vec::new();
    let mut paragraph_empty = true;
    let mut append_space = false;
    for c in &e.children {
        let is_tspan = c.name == "tspan";
        let (st, raw) = if is_tspan {
            let st = styled(defs, c, s, false);
            let raw: String = c
                .children
                .iter()
                .filter(|t| t.name == "#text")
                .filter_map(|t| t.attr(""))
                .collect();
            if st.hidden {
                continue;
            }
            let mut cs = st.s;
            // A tspan takes no transform (createTspanNode reads no attributes of its own,
            // and its QSvgTransformStyle is applied around no drawing).
            cs.m = s.m;
            (cs, raw)
        } else if c.name == "#text" {
            (s.clone(), c.attr("").unwrap_or("").to_string())
        } else {
            continue;
        };
        let default = !st.font.preserve;
        let mut text = raw.replace(['\t', '\n'], " ");
        let prepend =
            !append_space && !is_tspan && default && !paragraph_empty && text.starts_with(' ');
        let mut append_next = !is_tspan && default && text.ends_with(' ');
        if default {
            text = text.split_whitespace().collect::<Vec<_>>().join(" ");
            if text.is_empty() {
                append_next = false;
            }
        }
        if append_space {
            if let Some(last) = ranges.last_mut() {
                last.text.push(' ');
            }
        } else if prepend {
            text.insert(0, ' ');
        }
        if append_space || prepend || !text.is_empty() {
            paragraph_empty = false;
        }
        ranges.push(Range { text, s: st });
        append_space = append_next;
    }

    // The line: every range shaped with its own font, one after another.
    let mut runs = Vec::new();
    let mut x = 0i64;
    let (mut width, mut ascent, mut descent) = (0i64, 0i64, 0i64);
    for r in ranges {
        if r.text.is_empty() {
            continue;
        }
        let Some(stack) = stack(&r.s.font) else {
            continue;
        };
        let line = stack.shape(&r.text);
        (ascent, descent) = (ascent.max(line.ascent), descent.max(line.descent));
        // QTextLine::naturalTextWidth leaves the spaces at the end of the line out.
        if !r.text.trim_end_matches(' ').is_empty() {
            let trailing = r.text.len() - r.text.trim_end_matches(' ').len();
            let shown = if trailing == 0 {
                stack.natural_width(&line)
            } else {
                let cut = &r.text[..r.text.len() - trailing];
                stack.natural_width(&stack.shape(cut))
            };
            width = x + shown;
        }
        runs.push(Run {
            stack,
            glyphs: line.glyphs.clone(),
            x,
            s: r.s,
        });
        x += line.advance;
    }
    let g = |k: &str| e.attr(k).and_then(length_px).unwrap_or(0.0);
    let shift = match s.font.anchor {
        1 => -0.5 * (width as f64 / 64.0),
        2 => -(width as f64 / 64.0),
        _ => 0.0,
    };
    Line {
        runs,
        at: (g("x"), g("y")),
        shift,
        width,
        ascent,
        descent,
    }
}

fn det(m: &M) -> f64 {
    m[0] * m[3] - m[1] * m[2]
}

/// The outlines of a run as QFontEngine::addOutlineToPath adds them: positions in 26.6 user
/// units from the run's origin.
fn outline(stack: &FontStack, glyphs: &[(usize, u32, i64, i64)], origin: (i64, i64)) -> UPath {
    let mut p = Builder {
        el: vec![],
        start: 0,
        closed: false,
    };
    for &(fi, gid, gx, gy) in glyphs {
        let Some(font) = stack.font(fi) else {
            continue;
        };
        let (cx, cy) = ((origin.0 + gx) as f64 / 64.0, (origin.1 + gy) as f64 / 64.0);
        // QFreetypeFace::addGlyphToPath ends a contour with closeSubpath, or with a curve
        // back to its start, where closing changes nothing.
        let mut open = false;
        for el in font.outline(gid) {
            match el {
                PathEl::Move(x, y) => {
                    if open {
                        p.close();
                    }
                    p.move_to([cx + x, cy + y]);
                    open = true;
                }
                PathEl::Line(x, y) => p.line_to([cx + x, cy + y]),
                PathEl::Cubic(a, b, c, d, x, y) => {
                    p.cubic_to([cx + a, cy + b], [cx + c, cy + d], [cx + x, cy + y]);
                }
            }
        }
        if open {
            p.close();
        }
    }
    p.el
}

/// A strike's glyph under the painter matrix `m`, as QImageTextureGlyphCache holds it for a
/// face that is not scalable: the bitmap itself transformed (QFontEngineFT::alphaMapForGlyph,
/// `img.transformed(t, Qt::FastTransformation)`), cut to the box of
/// QFontEngineFT::alphaMapBoundingBox (scaledBitmapMetrics). `None` where Qt takes another
/// way that is not followed here: a matrix with shear (the glyphs go as paths), and a turn
/// with two different scales (the picture's rectangle goes through the scan converter).
fn turned(g: &Glyph, m: &M) -> Option<Glyph> {
    let (w, h) = (g.width as i64, g.rows as i64);
    let (left, top) = g.origin();
    let null = |v: f64| v.abs() <= 1e-12;
    let rotates = |m: &M| !null(m[1]) || !null(m[2]);
    if w == 0 || h == 0 || (rotates(m) && !null(m[0] * m[2] + m[1] * m[3])) {
        return None;
    }
    // QImage::trueMatrix: the picture's mapped rectangle starts inside the first pixel.
    let (fw, fh) = (w as f64, h as f64);
    let at = to_aligned_rect(map_rect(m, [0.0, 0.0, fw, fh]));
    let mat = [
        m[0],
        m[1],
        m[2],
        m[3],
        m[4] - at.x as f64,
        m[5] - at.y as f64,
    ];
    let (wd, hd, pixels) = transformed(&g.buf, w, h, &mat)?;
    // scaledBitmapMetrics: the strike's box through the matrix without its translation,
    // then QTextureGlyphCache::populate's whole pixels of it.
    let b = map_rect(
        &[m[0], m[1], m[2], m[3], 0.0, 0.0],
        [left as f64, -top as f64, fw, fh],
    );
    let ceil = |v: f64| (fixed(v) + 63) >> 6;
    let (cw, ch) = (ceil(b[2]), ceil(b[3]));
    let mut buf = vec![0u8; (cw * ch) as usize];
    for y in 0..hd.min(ch) {
        let n = wd.min(cw) as usize;
        let (d, s) = ((y * cw) as usize, (y * wd) as usize);
        buf[d..d + n].copy_from_slice(&pixels[s..s + n]);
    }
    Some(Glyph::coverage(
        cw as usize,
        ch as usize,
        fixed(b[0]) >> 6,
        -(fixed(b[1]) >> 6),
        buf,
    ))
}

/// QImage::transformed(mat, Qt::FastTransformation) of an Alpha8 picture of `w` by `h`, `mat`
/// already its trueMatrix: the size of the new picture and its bytes. `None` for the turn Qt
/// draws through its scan converter.
fn transformed(src: &[u8], w: i64, h: i64, mat: &M) -> Option<(i64, i64, Vec<u8>)> {
    let null = |v: f64| v.abs() <= 1e-12;
    let round = |v: f64| (v + 0.5).floor() as i64;
    let (fw, fh) = (w as f64, h as f64);
    let at = |x: i64, y: i64| src[(y * w + x) as usize];
    let scale = null(mat[1]) && null(mat[2]);
    if scale && null(mat[0] - 1.0) && null(mat[3] - 1.0) {
        return Some((w, h, src.to_vec()));
    }
    // rotated180, rotated90 and rotated270 move the pixels as they are.
    let quarter = |wd: i64, hd: i64, from: &dyn Fn(i64, i64) -> (i64, i64)| {
        let px = (0..hd).flat_map(|y| (0..wd).map(move |x| (x, y)));
        let px = px.map(|(x, y)| from(x, y)).map(|(x, y)| at(x, y));
        Some((wd, hd, px.collect()))
    };
    if scale && mat[0] == -1.0 && mat[3] == -1.0 {
        return quarter(w, h, &|x, y| (w - 1 - x, h - 1 - y));
    }
    if !scale && mat[0] == 0.0 && mat[3] == 0.0 {
        if mat[1] == 1.0 && mat[2] == -1.0 {
            return quarter(h, w, &|x, y| (y, h - 1 - x));
        }
        if mat[1] == -1.0 && mat[2] == 1.0 {
            return quarter(h, w, &|x, y| (w - 1 - y, x));
        }
    }
    let rr = map_rect(mat, [0.0, 0.0, fw, fh]);
    let (wd, hd) = if scale {
        (round(mat[0].abs() * fw), round(mat[3].abs() * fh))
    } else {
        let r = to_aligned_rect(rr);
        (r.w as i64, r.h as i64)
    };
    let mut out = vec![0u8; (wd.max(0) * hd.max(0)) as usize];
    if wd <= 0 || hd <= 0 {
        return Some((0, 0, out));
    }
    // QSpanData::setupMatrix: a 1/65536 shift keeps the fixed point rounding right.
    const D: f64 = 1.0 / 65536.0;
    let inv = invert(&compose([1.0, 0.0, 0.0, 1.0, D, D], *mat))?;
    // QPainter::drawImage without Antialiasing and SmoothPixmapTransform: every pixel of a
    // span is the picture's pixel its centre falls in (fetchTransformed), a span fetched in
    // runs of BufferSize.
    let mut span = |x: i64, len: i64, y: i64| {
        const ONE: f64 = 65536.0;
        let (fdx, fdy) = ((inv[0] * ONE) as i64, (inv[1] * ONE) as i64);
        let cy = y as f64 + 0.5;
        for from in (x..x + len).step_by(2048) {
            let cx = from as f64 + 0.5;
            let mut fx = ((inv[2] * cy + inv[0] * cx + inv[4]) * ONE) as i64;
            let mut fy = ((inv[3] * cy + inv[1] * cx + inv[5]) * ONE) as i64;
            let run = (x + len - from).min(2048) as usize;
            for o in &mut out[(y * wd + from) as usize..][..run] {
                *o = at((fx >> 16).clamp(0, w - 1), (fy >> 16).clamp(0, h - 1));
                (fx, fy) = (fx + fdx, fy + fdy);
            }
        }
    };
    if scale {
        // fillRect_normalized over the rounded rectangle.
        let (x1, y1) = (round(rr[0]).max(0), round(rr[1]).max(0));
        let (x2, y2) = (round(rr[0] + rr[2]).min(wd), round(rr[1] + rr[3]).min(hd));
        for y in y1..y2 {
            if x2 > x1 {
                span(x1, x2 - x1, y);
            }
        }
    } else if no_shear(*mat) {
        let map = |x: f64, y: f64| {
            [
                mat[0] * x + mat[2] * y + mat[4],
                mat[1] * x + mat[3] * y + mat[5],
            ]
        };
        aliased_line(map(0.0, fh * 0.5), map(fw, fh * 0.5), fh / fw, wd, hd, span);
    } else {
        return None;
    }
    Some((wd, hd, out))
}

/// One text item of the line (QTextLine::draw_internal) at `origin` (26.6, user space).
fn draw_run(
    ctx: &mut Context,
    defs: Defs,
    stack: &FontStack,
    glyphs: &[(usize, u32, i64, i64)],
    origin: (i64, i64),
    s: &State,
) {
    let gs = defs.0;
    let int_opacity = (s.opacity * 256.0) as u32;
    let grad = s.fill_grad.as_ref().and_then(|id| gs.get(id));
    // draw_helper: the pen is the text outline unless it is NoPen or has no brush.
    let stroking = !s.pen_off && s.stroke.is_some() && s.stroke_width != 0.0;
    let px = stack.font(0).map_or(0, |f| f.pixel_size) as f64;
    let cached = !stroking && px * px * det(&s.m).abs() <= 64.0 * 64.0;
    if !cached {
        // The outline as a path: drawn with the foreground as its brush and the text outline
        // as its pen, or (too large to cache) filled with the pen's brush.
        let up = outline(stack, glyphs, origin);
        if up.is_empty() {
            return;
        }
        if let Some(g) = grad {
            g.fill(ctx, &up, None, s.m, false, s.opacity);
        } else if let Some(c) = s.fill {
            ctx.fill_opacity(&Path::from_el(up.clone()), c, false, int_opacity, Some(s.m));
        }
        if stroking {
            super::stroke_path(ctx, defs, &up, s);
        }
        return;
    }
    if grad.is_none() && s.fill.is_none() {
        return;
    }
    // drawCachedGlyphs: the glyph bitmaps of the painter's transform, at device positions.
    let (w, h) = (ctx.img.w as i64, ctx.img.h as i64);
    let m = s.m;
    let transform = m[0] != 1.0 || m[1] != 0.0 || m[2] != 0.0 || m[3] != 1.0;
    // QTransformToFTMatrix.
    let ft = [
        (m[0] * 65536.0) as i64,
        (-m[2] * 65536.0) as i64,
        (-m[1] * 65536.0) as i64,
        (m[3] * 65536.0) as i64,
    ];
    let pm = solid_pm(s.fill.unwrap_or([0; 4]), int_opacity).map(|v| v as u8);
    let mut mask = grad.map(|_| Context::new(ctx.img.w, ctx.img.h));
    // QFontEngine::getGlyphPositions under matrix.translate(p).
    let (ox, oy) = (origin.0 as f64 / 64.0, origin.1 as f64 / 64.0);
    let (dx, dy) = (m[0] * ox + m[2] * oy + m[4], m[1] * ox + m[3] * oy + m[5]);
    let (fx0, fy0) = if transform {
        (0, 0)
    } else {
        (fixed(dx), fixed(dy))
    };
    for &(fi, gid, gx, gy) in glyphs {
        let Some(font) = stack.font(fi) else {
            continue;
        };
        let (mut fx, mut fy) = (fx0 + gx, fy0 + gy);
        if transform {
            let (ux, uy) = (gx as f64 / 64.0, gy as f64 / 64.0);
            fx = fixed(m[0] * ux + m[2] * uy + dx);
            fy = fixed(m[1] * ux + m[3] * uy + dy);
        }
        let Some(mut glyph) = font.glyph_xf(gid, Font::subpixel(fx), ft) else {
            continue;
        };
        if font.strike && transform {
            // FreeType transforms outlines only: Qt turns the strike's picture itself.
            if let Some(t) = turned(&glyph, &m) {
                glyph = t;
            }
        }
        let (left, top) = glyph.origin();
        let (bx, by) = (fx.div_euclid(64) + left, ((fy + 32).div_euclid(64)) - top);
        for row in 0..glyph.rows as i64 {
            let y = by + row;
            if y < 0 || y >= h {
                continue;
            }
            for col in 0..glyph.width as i64 {
                let x = bx + col;
                let cov = glyph.buf[(row * glyph.width as i64 + col) as usize] as u32;
                if x < 0 || x >= w || cov == 0 {
                    continue;
                }
                let i = (y * w + x) as usize;
                match &mut mask {
                    Some(mask) => {
                        let a = &mut mask.img.px[i][3];
                        *a = (*a).max(cov as u8);
                    }
                    None => source_over(&mut ctx.img.px[i], pm, cov),
                }
            }
        }
    }
    if let (Some(g), Some(mask)) = (grad, mask) {
        let up = outline(stack, glyphs, origin);
        g.paint(
            ctx,
            &mask,
            &up,
            compose([1.0, 0.0, 0.0, 1.0, 0.0, 0.0], s.m),
            s.opacity,
        );
    }
}
