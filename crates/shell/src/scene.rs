//! Geometry (implicit sizes, anchors, layouts, positioners) and software painting of the
//! runtime item tree under a window.

use rustc_hash::FxHashMap as HashMap;
use std::rc::Rc;

use qml::bytelru::ByteLru;
use qml::layout::{self, Attached, Child};
use qml::runtime::{Runtime, SCROLL_BARS, SCROLL_MADE, ViewTrack, World};
use qml::transform::Xf;
use qml::value::Value;
use qml::valuetypes::Mat4;
use qsg::raster::Canvas;
use qsg::text::FontStack;

use crate::text_layout::{self, Bounds, Breaks, Cut, Wrap};

type FontKey = (String, u32, i32);

/// Font stacks by family, pixel size, weight and axes. Animated sizes and axes (a scaling
/// label, MaterialSymbol's FILL/weight morph) mint a stack per step; one no text holds and
/// nothing asked for in [`FONTS_IDLE`] lookups is dropped once more than [`FONTS_KEPT`]
/// exist, instead of every step staying resident with its FreeType faces and glyph caches.
#[derive(Default)]
pub struct Fonts {
    cache: HashMap<FontKey, (Option<Rc<FontStack>>, u64)>,
    lookups: u64,
}

const FONTS_KEPT: usize = 32;
const FONTS_IDLE: u64 = 4096;

impl Fonts {
    /// ri::wstat: font stacks and their summed caches.
    pub fn stats(&self) -> String {
        let mut t = (0, 0, 0);
        for f in self.cache.values().filter_map(|e| e.0.as_ref()) {
            let s = f.cache_stats();
            t = (t.0 + s.0, t.1 + s.1, t.2 + s.2);
        }
        format!(
            "stacks={} shaped={} glyphs={} df={}",
            self.cache.len(),
            t.0,
            t.1,
            t.2
        )
    }

    pub fn get(&mut self, family: &str, px: u32, weight: i32) -> Option<Rc<FontStack>> {
        self.get_axes(family, px, weight, &[], 0)
    }

    /// `font.variableAxes` reach FreeType (outlines), as in QFontEngineFT.
    pub fn get_axes(
        &mut self,
        family: &str,
        px: u32,
        weight: i32,
        axes: &[(String, f32)],
        hinting: i32,
    ) -> Option<Rc<FontStack>> {
        self.get_styled(family, "", px, weight, axes, (hinting, 0.0))
    }

    /// `get_axes` with a QFont styleName, which picks the face before the weight does, and
    /// with `set`: the hinting preference and `font.letterSpacing` in pixels.
    pub fn get_styled(
        &mut self,
        family: &str,
        style: &str,
        px: u32,
        weight: i32,
        axes: &[(String, f32)],
        set: (i32, f64),
    ) -> Option<Rc<FontStack>> {
        let (hinting, letter_spacing) = set;
        let mut key: String = axes.iter().map(|(t, v)| format!("{t}={v};")).collect();
        key += &hinting.to_string();
        if letter_spacing != 0.0 {
            key += &format!(";{letter_spacing}");
        }
        let italic = axes.iter().any(|(t, v)| t == "ital" && *v >= 1.0);
        self.lookups += 1;
        let key = (format!("{family}\0{style}\0{key}"), px, weight);
        if !self.cache.contains_key(&key) {
            self.evict_idle();
        }
        let now = self.lookups;
        let e = self.cache.entry(key).or_insert_with(|| {
            let stack = (|| {
                let (path, face_weight) = qsg::text::fc_match_face(family, weight, style, italic)?;
                let ax: Vec<(&str, f32)> = axes.iter().map(|(t, v)| (t.as_str(), *v)).collect();
                FontStack::from_path_styled(
                    &path,
                    px.max(1),
                    face_weight as f32,
                    weight as f32,
                    &ax,
                )
                .ok()
                .map(|f| f.requested(family))
                // QFont::PreferNoHinting / PreferFullHinting
                .map(|f| match hinting {
                    1 => f.no_hinting(),
                    3 => f.full_hinting(),
                    4 => f.full_hinting_design(),
                    _ => f,
                })
                .map(|f| f.letter_spacing(letter_spacing))
                .map(Rc::new)
            })();
            (stack, now)
        });
        e.1 = now;
        e.0.clone()
    }

    fn evict_idle(&mut self) {
        if self.cache.len() < FONTS_KEPT {
            return;
        }
        let now = self.lookups;
        self.cache.retain(|_, (f, used)| {
            now - *used < FONTS_IDLE || f.as_ref().is_some_and(|f| Rc::strong_count(f) > 1)
        });
    }
}

/// Kirigami.Icon: the pixmap of a QIcon (a file or a theme icon name), fitted to the item.
fn is_icon(w: &World, i: usize) -> bool {
    is(w, i, "Kirigami.Icon")
}

/// The `fillMode` of an [`ImageKey`] that loads a theme icon's file as QIconLoaderEngine's
/// pixmap of the key's size, which is in pixels: the file fitted into it once
/// ([`qsg::image::qicon_fit`]). No Image.fillMode is negative.
const LOADER_FILL: i32 = -1;

/// The `fillMode` of an [`ImageKey`] whose source an icon provider answers (`image://icon/`,
/// the tray's `image://service.sni/`): the key's size is what the Image requests, and the
/// pixmap is loaded by [`provider_pixels`], whatever the Image's own fillMode. No
/// Image.fillMode is 7.
const PROVIDER_FILL: i32 = 7;

fn provider_source(src: &str) -> bool {
    src.starts_with("image://icon/") || src.starts_with("image://service.sni/")
}

/// IconImageProvider::requestPixmap's size for a request of `ss`: 100x100 for one without a
/// size, 2x2 for one with a zero side.
fn provider_target(ss: (i32, i32)) -> (i32, i32) {
    match ss {
        (w, h) if w > 0 && h > 0 => (w, h),
        (w, h) if w <= 0 && h <= 0 => (100, 100),
        _ => (2, 2),
    }
}

/// The size in pixels `QIcon::pixmap(w, h)` fits a theme icon into for a request of `ss`: the
/// requested size times QGuiApplication::devicePixelRatio(), each side rounded (QSize times
/// a qreal in QIconLoaderEngineEntry::pixmap; `QT_SCALE_FACTOR` makes the ratio any number).
fn provider_pixels(ss: (i32, i32)) -> (i32, i32) {
    let ((w, h), app) = (provider_target(ss), screens_ratio());
    let px = |side: i32| (side as f64 * app).round() as i32;
    (px(w), px(h))
}

/// The `fillMode` of an [`ImageKey`] that loads a file as QIcon(file)'s pixmap
/// ([`qsg::image::qicon_size`]) instead of an Image's `sourceSize`, in a window of
/// devicePixelRatio `dpr` of an application of ratio `app`: the key's size is the item's, and
/// the mode, below [`LOADER_FILL`], keeps the window's ratio (wp_fractional_scale_v1 gives it
/// in 120ths), whether the file is a theme's Fallback entry (`nested`) and the application's
/// (in 120ths as well: `QT_SCALE_FACTOR` makes it any number).
fn qicon_fill(dpr: f64, app: f64, nested: bool) -> i32 {
    let ratio = ((dpr * 120.0).round() as i32).clamp(1, QICON_RATIO);
    let app = ((app * 120.0).round() as i32).clamp(1, QICON_APP * 120) * QICON_NESTED * 2;
    LOADER_FILL - (ratio | if nested { QICON_NESTED } else { 0 } | app)
}

/// The `fillMode` of an [`ImageKey`] that loads a theme icon's file as the pixmap
/// plasma-integration's KIconEngine gives for it ([`kicon_pixmap`]) in a window of
/// devicePixelRatio `dpr`: the key's size is the one asked for, and the mode, between
/// [`LOADER_FILL`] and every [`qicon_fill`], keeps the ratio in 120ths (KIconEngine::
/// createPixmap takes one below 1 for 1).
fn kicon_fill(dpr: f64) -> i32 {
    LOADER_FILL - ((dpr * 120.0).round() as i32).clamp(120, QICON_RATIO)
}

/// The window's ratio of a [`kicon_fill`]; `None` for any other mode.
fn kicon_ratio(fill: i32) -> Option<f64> {
    let code = LOADER_FILL - fill;
    (1..=QICON_RATIO)
        .contains(&code)
        .then(|| code as f64 / 120.0)
}

/// The bits of the window's ratio in [`qicon_fill`]: up to 8.5.
const QICON_RATIO: i32 = 1023;
/// The bit of `nested` in [`qicon_fill`]; the application's ratio is above it.
const QICON_NESTED: i32 = QICON_RATIO + 1;
/// The highest application ratio [`qicon_fill`] keeps.
const QICON_APP: i32 = 15;

thread_local! {
    /// The highest QScreen::devicePixelRatio() of all screens; 0 while none is known.
    static SCREENS_SCALE: std::cell::Cell<f64> = const { std::cell::Cell::new(0.0) };
}

/// Tells icons the highest wl_output.scale of all outputs (0: none is known).
pub fn set_screens_scale(scale: i32) {
    set_screens_ratio(scale as f64);
}

/// Tells icons the highest QScreen::devicePixelRatio() of all screens (0: none is known):
/// the output's wl_output.scale times the screen's factor ([`crate::hidpi`]).
pub fn set_screens_ratio(ratio: f64) {
    SCREENS_SCALE.set(ratio.max(0.0));
}

/// QGuiApplication::devicePixelRatio() for what asks without a window: the highest
/// QScreen::devicePixelRatio() of all screens, 1 while none is known.
fn screens_ratio() -> f64 {
    match SCREENS_SCALE.get() {
        0.0 => 1.0,
        s => s,
    }
}

/// The scale of the theme directories an icon asked without a window is looked up in:
/// [`screens_ratio`] rounded up (QIconLoaderEngine::scaledPixmap's `qCeil(scale)`).
fn screens_scale() -> i32 {
    screens_ratio().ceil() as i32
}

/// The window of item `win` is there, on an output of wl_output.scale `scale`: an Image made
/// from now on asks its icons at the window's ratio ([`provider_ratio`]), which until the
/// compositor gives the window a scale is its output's (QWaylandWindow::initWindow takes
/// mScale from its QWaylandScreen).
pub fn window_made(w: &mut World, win: usize, scale: i32) {
    let n = w.items.len();
    let mut scales = image_loads(w).window_scales.borrow_mut();
    if scales.contains_key(&win) {
        return;
    }
    scales.insert(win, (scale.max(1) as f64, n));
    drop(scales);
    purge_provider_pixmaps(w);
}

/// What the Image `i` of icon provider source `src` asks the provider for, and the Image's own
/// ratio (QQuickImageBase::loadPixmap: `sourceSize` times the ratio).
fn provider_request(w: &mut World, i: usize, src: &str) -> ((i32, i32), f64) {
    // QSizeF -> QSize rounds; a sourceSize that was not set is no size at all, and the
    // Image's ratio stays 1.
    let ss = w
        .source_size_set(i)
        .map(|v| v.unwrap_or(0.0).round() as i32);
    let ss = (ss[0], ss[1]);
    if ss.0 <= 0 && ss.1 <= 0 {
        return ((0, 0), 1.0);
    }
    let ratio = provider_ratio(w, i, src, ss);
    let times = |v: i32| (v.max(0) as f64 * ratio).round() as i32;
    ((times(ss.0), times(ss.1)), ratio)
}

/// QQuickPixmap::purgeCache as it runs when Quickshell 0.3.2 loads a panel: the icon provider's
/// pixmaps no Image has are forgotten, and the next Image to ask for one gets it at the scale
/// the application has then. WlrLayershell::retrieveWindow makes a QQuickWindow,
/// ProxyWindowBase::ensureQWindow deletes it for its surface format and makes another, and
/// ~QQuickWindow purges; this is after the panel's Component.onCompleted and before what it
/// gave Qt.callLater. A panel going with its output was measured to leave them.
fn purge_provider_pixmaps(w: &mut World) {
    let mut held = std::collections::HashSet::new();
    for i in w.of_type("Image") {
        if is_icon(w, i) {
            continue;
        }
        let src = w.read(i, "source").map(|v| v.to_js_string());
        let src = w.resolved_url(i, &src.unwrap_or_default());
        if provider_source(&src) {
            let (ss, _) = provider_request(w, i, &src);
            held.insert((src, ss));
        }
    }
    IMAGES.with_borrow_mut(|c| {
        c.retain(|k, _| k.2 != PROVIDER_FILL || held.contains(&(k.0.clone(), k.1)))
    });
}

/// The compositor gave the window of item `win` the scale `dpr` (again, or for the first
/// time): the Images there from then on ask their icons at it ([`provider_ratio`]), and those
/// it changes the ratio of are laid out again, their implicit size being their pixmap's over
/// it.
pub fn window_scale(w: &mut World, win: usize, dpr: f64) {
    let n = w.items.len();
    let was = {
        let mut scales = image_loads(w).window_scales.borrow_mut();
        let s = scales.entry(win).or_insert((f64::NAN, n));
        std::mem::replace(&mut s.0, dpr)
    };
    if was == dpr {
        return;
    }
    for i in w.of_type("Image") {
        let src = w.read(i, "source").map(|v| v.to_js_string());
        if src.is_some_and(|s| provider_source(&s)) && qml::runtime::window_of(w, i) == Some(win) {
            w.memo_invalidate(i);
            w.resized.push(i);
            w.touched.push(i);
        }
    }
}

/// Drops what [`provider_ratio`], [`window_scale`] and [`polish_icons`] keep for reaped items.
pub fn forget_ratios(w: &World, ids: &[usize]) {
    let loads = image_loads(w);
    for i in ids {
        loads.ratios.borrow_mut().remove(i);
        loads.window_scales.borrow_mut().remove(i);
        loads.icons.borrow_mut().remove(i);
    }
}

/// QQuickImageBasePrivate::devicePixelRatio of an Image asking an icon provider.
struct ProviderRatio {
    ratio: f64,
    /// The scale its window had when this was last looked at; `None` before the compositor
    /// gave one.
    window: Option<f64>,
    /// Hash of the source and `sourceSize` it is for.
    request: u64,
}

/// The devicePixelRatio of Image `i`, which asks an icon provider for `src` with a
/// `sourceSize` of `ss` (QQuickImageBase::loadPixmap requests `sourceSize` times it, and the
/// implicit size is the pixmap's over it). Measured on Qt 6.12 with Quickshell 0.3.2: an
/// Image loads at its window's ratio, or at the application's when it is loaded before its
/// window exists, as what a shell declares is; and it loads again when its window's ratio
/// changes (QQuickImageBase::itemChange), which for a Qt window that starts at its output's
/// whole scale is the arrival of a fractional one, or any later change. An output coming or
/// going changes only the application's ratio and reloads nothing.
fn provider_ratio(w: &World, i: usize, src: &str, ss: (i32, i32)) -> f64 {
    use std::hash::{Hash, Hasher};
    let loads = image_loads(w);
    let reported = qml::runtime::window_of(w, i)
        .and_then(|win| loads.window_scales.borrow().get(&win).copied());
    let (now, app) = (reported.map(|r| r.0), screens_ratio());
    let mut h = rustc_hash::FxHasher::default();
    (src, ss).hash(&mut h);
    let request = h.finish();
    let mut ratios = loads.ratios.borrow_mut();
    match ratios.get_mut(&i) {
        // A new source or sourceSize is a new load.
        Some(r) if r.request != request => {
            *r = ProviderRatio {
                ratio: now.unwrap_or(app),
                window: now,
                request,
            };
            r.ratio
        }
        Some(r) => {
            if let Some(n) = now {
                if r.window.map_or(n.fract() != 0.0, |was| was != n) {
                    r.ratio = n;
                }
                r.window = now;
            }
            r.ratio
        }
        None => {
            let ratio = match reported {
                None => app,
                // Made before its window: loaded like one seen before then.
                Some((n, made)) if i < made && n.fract() == 0.0 => app,
                Some((n, _)) => n,
            };
            ratios.insert(
                i,
                ProviderRatio {
                    ratio,
                    window: now,
                    request,
                },
            );
            ratio
        }
    }
}

/// QGuiApplication::devicePixelRatio() for a window of devicePixelRatio `dpr`: the highest
/// QScreen::devicePixelRatio() of all screens, which on Wayland is the output's wl_output.scale
/// (QWaylandScreen::devicePixelRatio) times the screen's factor. Without outputs, the window's
/// ratio rounded up, the scale of the one output it would be on.
fn app_dpr(dpr: f64) -> f64 {
    match SCREENS_SCALE.get() {
        0.0 => dpr.ceil().max(1.0),
        s => s,
    }
}

/// Size of the pixmap an icon `fill` gives a file of `ow`x`oh` asked for `ss`.
fn icon_pixmap_size(fill: i32, ow: i32, oh: i32, ss: (i32, i32), svg: bool) -> Option<(i32, i32)> {
    match fill {
        LOADER_FILL => Some(qsg::image::qicon_fit(ow, oh, ss.0, ss.1, svg)),
        ..LOADER_FILL => {
            let code = LOADER_FILL - fill;
            let dpr = (code & QICON_RATIO) as f64 / 120.0;
            let nested = code & QICON_NESTED != 0;
            let app = (code / (QICON_NESTED * 2)) as f64 / 120.0;
            Some(qsg::image::qicon_size(
                (ow, oh),
                ss,
                (app, dpr),
                nested,
                svg,
            ))
        }
        _ => None,
    }
}

/// Kirigami's IconSizes::roundedIconSize.
fn rounded_icon_size(size: i32) -> i32 {
    match size {
        ..16 => size,
        16..22 => 16,
        22..32 => 22,
        32..48 => 32,
        48..64 => 48,
        _ => size,
    }
}

/// Whether a Kirigami.Icon of `w`x`h` is a square of a standard icon size, snapped to whole
/// pixels of a window of devicePixelRatio `dpr`, and that size: Icon::updatePaintedGeometry
/// fills such an item whole.
fn icon_standard(w: f64, h: f64, dpr: f64) -> (bool, f64) {
    let std = (rounded_icon_size(w.min(h) as i32) as f64 * dpr).round() / dpr;
    ((w, h) == (std, std), std)
}

/// Where a Kirigami.Icon of `w`x`h` draws its `tw`x`th` pixmap, stretched (x, y, width,
/// height from the item's corner), in a window of devicePixelRatio `dpr`:
/// Icon::updatePaintedGeometry fills a square item of a standard icon size
/// ([`icon_standard`]) whole; with `roundToIconSize` (`round`) the pixmap's longer side is
/// the standard size the item's shorter side rounds down to; without, it takes the pixmap as
/// `tw / dpr` x `th / dpr` units, fits it into the item and snaps the size to whole pixels.
/// Icon::calculateNodeRect centres it without rounding (Kirigami 6.31; [`icon_corner`] is
/// where it lands). A side of the item that was not set (`valid`: widthValid(),
/// heightValid()) counts as the pixmap's own, so a pixmap smaller than an item that only has
/// an implicit size is not stretched.
fn icon_rect(
    (tw, th): (usize, usize),
    (w, h): (f64, f64),
    valid: (bool, bool),
    dpr: f64,
    round: bool,
) -> [f64; 4] {
    let snap = |v: f64| (v * dpr).round() / dpr;
    let (standard, std) = icon_standard(w, h, dpr);
    let (pw, ph) = if standard {
        (w, h)
    } else if round {
        let (tw, th) = (tw as f64, th as f64);
        match tw.total_cmp(&th) {
            std::cmp::Ordering::Greater => (std, th * (std / tw)),
            std::cmp::Ordering::Less => (tw * (std / th), std),
            std::cmp::Ordering::Equal => (std, std),
        }
    } else {
        let (tw, th) = (tw as f64 / dpr, th as f64 / dpr);
        let (fw, fh) = (if valid.0 { w } else { tw }, if valid.1 { h } else { th });
        let (ws, hs) = (fw / tw, fh / th);
        let (pw, ph) = if ws <= hs {
            (fw, ws * th)
        } else {
            (hs * tw, fh)
        };
        (snap(pw), snap(ph))
    };
    [(w - pw) / 2.0, (h - ph) / 2.0, pw, ph]
}

/// The corner `x`, `y` of an icon's rectangle in the window as Kirigami's icon.vert moves
/// it: each vertex goes to the whole pixel `round(pixel + 0.0001)` of the framebuffer, whose
/// rows OpenGL counts from the bottom, so half a pixel goes right and up. `dpr` is how many
/// pixels a unit of `x` and `y` is.
fn icon_corner(x: f64, y: f64, dpr: f64) -> (f64, f64) {
    (
        (x * dpr + 0.5001).floor() / dpr,
        (y * dpr - 0.5001).ceil() / dpr,
    )
}

/// QQuickItem::widthValid/heightValid of the Kirigami.Icon `i`: set explicitly, by anchors
/// or by a layout.
fn icon_size_valid(w: &mut World, i: usize) -> (bool, bool) {
    let in_layout = truthy(w, i, "visible", true)
        && w.items[i]
            .parent
            .is_some_and(|p| linear_axis(w, p).is_some() || is(w, p, "GridLayout"));
    let (mw, mh) = if in_layout {
        // QQuickGridLayoutItem::setGeometry calls setSize, which marks both sides, only when
        // the cell's size differs from the item's: an icon left at its implicit size is not set.
        let sized = num(w, i, "width") != num(w, i, "implicitWidth")
            || num(w, i, "height") != num(w, i, "implicitHeight");
        (sized, sized)
    } else {
        size_managed(w, i)
    };
    (
        mw || explicit(w, i, "width"),
        mh || explicit(w, i, "height"),
    )
}

/// What Kirigami's Icon keeps from one polish to the next.
#[derive(Clone, Default)]
struct IconState {
    /// What the icon was last polished for: its pixmap's key, its size and the window's
    /// devicePixelRatio, and whether its remote source was still on its way.
    polished: Option<(ImageKey, [u64; 3], bool)>,
    /// `m_icon`: the pixmap's sides in pixels and how many of them are a unit.
    pixmap: (usize, usize, f64),
    /// `m_paintedSize`.
    painted: (f64, f64),
    /// Whether the theme has nothing for the source itself: Icon::findIcon is then Error,
    /// also when it draws the fallback.
    missed: bool,
    /// `roundToIconSize` as the last polish found it: Icon::setRoundToIconSize does not ask
    /// for a polish, so the pixmap stays the one loaded for the old value until something
    /// else asks.
    round: Option<bool>,
}

/// Whether `source` of the Kirigami.Icon `i` is a null QVariant: nothing was assigned.
fn icon_source_null(w: &mut World, i: usize) -> bool {
    matches!(
        w.read(i, "source"),
        None | Some(Value::Undefined | Value::Null)
    )
}

/// Kirigami Icon::updatePaintedGeometry of the icon `i`: `paintedWidth` and `paintedHeight`
/// are the rounded sides of the rectangle its pixmap (the one its last polish loaded) is
/// drawn in at the item's size now ([`icon_rect`]), 0 without a pixmap. Both properties have
/// the one change signal `paintedAreaChanged`, which comes when the unrounded size changed,
/// and every time for a square item of a standard icon size. Returns whether the size
/// changed.
fn icon_geometry(w: &mut World, i: usize) -> bool {
    let state = image_loads(w).icons.borrow().get(&i).cloned();
    let state = state.unwrap_or_default();
    let (tw, th, ratio) = state.pixmap;
    let (wd, ht) = (num(w, i, "width"), num(w, i, "height"));
    let (painted, always) = if tw == 0 || th == 0 {
        ((0.0, 0.0), false)
    } else {
        let (valid, round) = (icon_size_valid(w, i), truthy(w, i, "roundToIconSize", true));
        let r = icon_rect((tw, th), (wd, ht), valid, ratio, round);
        ((r[2], r[3]), icon_standard(wd, ht, ratio).0)
    };
    let changed = painted != state.painted;
    if !changed && !always {
        return false;
    }
    let loads = image_loads(w);
    loads.icons.borrow_mut().entry(i).or_default().painted = painted;
    let names = ["paintedWidth", "paintedHeight"];
    let now = [painted.0.round(), painted.1.round()];
    let was = names.map(|n| w.read(i, n).map_or(0.0, |v| v.to_number()));
    for k in 0..2 {
        if was[k] != now[k] {
            w.write(i, names[k], Value::Num(now[k]));
        } else {
            qml::native::emit(w, i, &format!("{}Changed", names[k]), &[], vec![]);
        }
    }
    qml::native::emit(w, i, "paintedAreaChanged", &[], vec![]);
    changed
}

/// `World::icon_changed`, what Kirigami's Icon does in a setter: Icon::setSource makes it
/// Loading, Icon::geometryChange and Icon::setRoundToIconSize tell the painted size of the
/// pixmap it has at once (setRoundToIconSize tells a change twice); the pixmap itself is
/// loaded when the window polishes its items ([`polish_icons`]).
fn icon_changed(w: &mut World, i: usize, name: &str) {
    if name != "roundToIconSize" {
        icon_asks(w, i);
    }
    match name {
        // An icon nothing polished yet reads as Loading without being told.
        "source" if w.items[i].props.contains_key("status") => set(w, i, "status", 2.0),
        "source" => {}
        "roundToIconSize" => {
            if icon_geometry(w, i) {
                qml::native::emit(w, i, "paintedAreaChanged", &[], vec![]);
            }
        }
        _ => {
            icon_geometry(w, i);
        }
    }
}

/// How many times a Kirigami.Icon asked to be polished; and of each that is not polished yet,
/// how many items there were when it asked and which ask it was.
type IconAsks = (u64, HashMap<usize, (usize, u64)>);

/// QQuickItem::polish of the Kirigami.Icon `i`: it is put last in the list of the items its
/// window polishes before the next frame, unless it is in the list already.
fn icon_asks(w: &mut World, i: usize) {
    let made = w.items.len();
    let mut asked = image_loads(w).asked.borrow_mut();
    asked.0 += 1;
    let nth = asked.0;
    asked.1.entry(i).or_insert((made, nth));
}

/// Where the icon `i` stands in QQuickWindowPrivate::itemsToPolish, the first to ask first.
/// An icon made since the last polish asked when it was made (its size and source are set
/// then), and [`icon_asks`] keeps how many items there were when an older icon asked. An icon
/// that did not ask (a new devicePixelRatio, a remote source that came) stands before both.
fn icon_asked(w: &World, i: usize) -> (usize, u8, u64) {
    let loads = image_loads(w);
    match loads.asked.borrow().1.get(&i) {
        Some(&(made, nth)) => (made, 1, nth),
        None if (loads.icons.borrow().get(&i)).is_some_and(|s| s.polished.is_some()) => {
            (0, 0, i as u64)
        }
        None => (i, 2, 0),
    }
}

/// QQuickWindowPrivate::polishItems for the Kirigami.Icons of window `win`, drawn or not,
/// before a frame of devicePixelRatio `dpr` is drawn: Icon::updatePolish of each whose
/// source, size or pixmap is not what it was last polished for, the one that asked last
/// first (`itemsToPolish.takeLast()`). What follows an icon's `status` and its painted size
/// runs before the next icon is polished, and an icon that asks there is the next one.
pub fn polish_icons(rt: &mut Runtime, win: usize, dpr: f64) {
    rt.world.borrow_mut().icon_changed = Some(icon_changed);
    // QQuickWindowPrivate::polishItems gives up on items that keep asking each other.
    for _ in 0..POLISH_ROUNDS {
        let (mut icons, before) = {
            let mut w = rt.world.borrow_mut();
            let mut icons = w.of_type("Kirigami.Icon");
            icons.retain(|&i| {
                let mut at = Some(i);
                while at.is_some_and(|a| a != win) {
                    at = w.items[at.unwrap()].parent;
                }
                at.is_some()
            });
            icons.sort_by_cached_key(|&i| icon_asked(&w, i));
            (icons, polish_asks(&w))
        };
        let mut asked = false;
        while let Some(i) = icons.pop() {
            {
                let mut w = rt.world.borrow_mut();
                let asked = image_loads(&w).asked.borrow_mut().1.remove(&i).is_some();
                if !polish_icon(&mut w, i, dpr, asked) {
                    continue;
                }
            }
            rt.settle();
            icon_geometry(&mut rt.world.borrow_mut(), i);
            rt.settle();
            if polish_asks(&rt.world.borrow()) != before {
                asked = true;
                break;
            }
        }
        if !asked {
            break;
        }
    }
}

const POLISH_ROUNDS: usize = 64;

/// What changes when an icon asks to be polished or one is made.
fn polish_asks(w: &World) -> (u64, usize) {
    (image_loads(w).asked.borrow().0, w.items.len())
}

/// Kirigami Icon::updatePolish of the icon `i` up to its pixmap: without a source it is
/// Ready; an item with a size loads its pixmap, a transparent one of the item's size in
/// whole units when nothing is found, and is Ready or Error (Icon::findIcon). Returns
/// whether it was polished: its painted size is to be told ([`icon_geometry`]). `asked` is
/// whether the icon asked for this polish itself; one that did not is polished when what it
/// would load with the `roundToIconSize` of its last polish is not what it loaded then.
fn polish_icon(w: &mut World, i: usize, dpr: f64, asked: bool) -> bool {
    let (wd, ht) = (num(w, i, "width"), num(w, i, "height"));
    let null = icon_source_null(w, i);
    let sized = wd as i32 != 0 && ht as i32 != 0;
    let state = image_loads(w).icons.borrow().get(&i).cloned();
    let state = state.unwrap_or_default();
    let key = |w: &mut World| {
        let (src, ss, fk) = match !null && sized {
            true => {
                let (src, ss, fk, _) = image_src(w, i, wd, ht, dpr);
                (src, ss, fk)
            }
            false => (String::new(), (0, 0), 0),
        };
        ((src, ss, fk), [wd.to_bits(), ht.to_bits(), dpr.to_bits()])
    };
    let pending = |key: &(ImageKey, [u64; 3])| match &state.polished {
        Some(p) if (&p.0, &p.1) == (&key.0, &key.1) => p.2,
        _ => true,
    };
    let mut key_now = key(w);
    let round = truthy(w, i, "roundToIconSize", true);
    if state.round != Some(round) && (asked || pending(&key_now)) {
        let mut icons = image_loads(w).icons.borrow_mut();
        icons.entry(i).or_default().round = Some(round);
        drop(icons);
        key_now = key(w);
    }
    let key = key_now;
    let (src, ss, fk) = (&key.0.0, key.0.1, key.0.2);
    if !pending(&key) || state.polished.is_some() && remote_pending(image_loads(w), src) {
        return false;
    }
    let mut pixmap = state.pixmap;
    if null {
        set(w, i, "status", 1.0);
    } else if sized {
        let assigned = w.sourced.remove(&i);
        remote_request(image_loads(w), i, src, assigned);
        let named = !w
            .read(i, "source")
            .is_some_and(|v| v.to_js_string().is_empty());
        match image_texture(image_loads(w), src, ss, fk, false) {
            Some(t) => {
                let missed = image_loads(w)
                    .icons
                    .borrow()
                    .get(&i)
                    .is_some_and(|s| s.missed);
                set(w, i, "status", if missed { 3.0 } else { 1.0 });
                // A QIcon's pixmap is in pixels; what is loaded like an Image is in units.
                pixmap = (t.w, t.h, if fk < 0 { dpr } else { 1.0 });
            }
            None => {
                if named && !(is_remote(src) && remote_pending(image_loads(w), src)) {
                    set(w, i, "status", 3.0);
                }
                pixmap = (wd as usize, ht as usize, dpr);
            }
        }
    }
    let pending = is_remote(src) && remote_pending(image_loads(w), src);
    let mut icons = image_loads(w).icons.borrow_mut();
    let state = icons.entry(i).or_default();
    state.polished = Some((key.0, key.1, pending));
    state.pixmap = pixmap;
    true
}

pub(crate) fn is(w: &World, i: usize, ty: &str) -> bool {
    w.items[i].types.iter().any(|t| t == ty)
}

/// Loader, or Impulse WidgetHost, which loads its widget and sizes it the same way.
fn is_loader(w: &World, i: usize) -> bool {
    is(w, i, "Loader") || is(w, i, "WidgetHost")
}

/// A QQuickPopup (Popup, Menu, Dialog, ...) other than ToolTip: it lives in QQuickOverlay.
pub(crate) fn is_popup(w: &World, i: usize) -> bool {
    !is(w, i, "ToolTip") && qml::types::is_popup(&w.items[i].types)
}

pub const EFFECT_ITEMS: &[&str] = &[
    "MultiEffect",
    "OpacityMask",
    "ColorOverlay",
    "Desaturate",
    "DropShadow",
    "GaussianBlur",
    "FastBlur",
    "RadialGradient",
    "LinearGradient",
];

fn is_item(w: &World, i: usize) -> bool {
    !w.dead.contains(&i) && type_bits(w, i) & ITEM_TYPE != 0
}

/// Objects assigned to a plain property (e.g. `property Item mask: Item {}`) are not
/// visual children, except a Control's `background`/`contentItem`/`indicator`/`handle`.
fn in_scene(w: &mut World, parent: usize, c: usize) -> bool {
    if !w.items[c].props.contains_key("\x01propval") || attached_scrollbar(w, parent, c) {
        return true;
    }
    if w.items[parent].prop_info("background").is_none() {
        return false;
    }
    // A ComboBox's `popup` is shown in place (QQuickOverlay stacking is approximated by z).
    [
        "background",
        "contentItem",
        "indicator",
        "handle",
        "popup",
        "up.indicator",
        "down.indicator",
    ]
    .iter()
    .any(|p| matches!(w.read(parent, p), Some(Value::Item(x)) if x == c))
}

pub fn visual_kids(w: &mut World, i: usize) -> Vec<usize> {
    let mut k = kids(w, i);
    k.retain(|&c| in_scene(w, i, c));
    k
}

/// Visual children in stacking order: ascending `z`, declaration order among equals.
pub fn stacked_kids(w: &mut World, i: usize) -> Vec<usize> {
    let v = visual_kids(w, i);
    // Uniform z (the common case): the stable sort below would keep document order.
    let Some(&first) = v.first() else { return v };
    let z0 = num(w, first, "z").to_bits();
    if v[1..].iter().all(|&c| num(w, c, "z").to_bits() == z0) {
        return v;
    }
    let mut ch: Vec<(f64, usize)> = v.into_iter().map(|c| (num(w, c, "z"), c)).collect();
    ch.sort_by(|a, b| a.0.total_cmp(&b.0));
    ch.into_iter().map(|x| x.1).collect()
}

/// What a line prints of its characters: Qt leaves out a carriage return, a form feed and a
/// paragraph separator, and prints a soft hyphen only where the line was broken at it.
fn printed(s: &str, hyphen: bool) -> std::borrow::Cow<'_, str> {
    let hidden = |c: char| matches!(c, '\r' | '\u{c}' | '\u{2029}');
    if !hyphen && !s.contains(hidden) {
        return s.into();
    }
    let mut out: String = s.chars().filter(|&c| !hidden(c)).collect();
    if hyphen && out.ends_with(text_layout::SOFT_HYPHEN) {
        out.pop();
        out.push('-');
    }
    out.into()
}

/// A line shaped with its tabs, each as wide as the way to the next tab stop
/// (QTextEngine::calculateTabWidth).
fn shape_tabbed(f: &FontStack, s: &str, rtl: bool) -> qsg::text::ShapedLine {
    if !s.contains('\t') {
        return f.shape_line(s, rtl);
    }
    let mut out = f.shape("");
    let mut x = 0;
    for (k, piece) in s.split('\t').enumerate() {
        if k > 0 {
            x += qsg::text::tab_width(x);
        }
        let l = f.shape_line(piece, rtl);
        out.glyphs.extend(
            l.glyphs
                .iter()
                .map(|&(font, g, gx, gy)| (font, g, gx + x, gy)),
        );
        out.ascent = out.ascent.max(l.ascent);
        out.descent = out.descent.max(l.descent);
        out.leading = out.leading.max(l.leading);
        x += l.advance;
    }
    out.advance = x;
    out
}

/// The lines a Text shows and what QQuickText reports of them.
struct Laid<L> {
    lines: Vec<L>,
    /// Where in the text each line starts, in bytes.
    starts: Vec<usize>,
    /// `Text.truncated`: lines were cut off or elided.
    truncated: bool,
    /// The advance and the natural width of each line QQuickText counts in its content
    /// width: those shown, and the one elided for the height as it was before.
    spans: Vec<(f64, f64)>,
    /// The implicit size: QTextLayout::maximumWidth, and the height of the lines up to
    /// `maximumLineCount` when none is cut off by the item's height.
    implicit_width: f64,
    implicit_height: f64,
}

impl<L> Laid<L> {
    /// QQuickText's content width: how wide its lines' rectangles are together
    /// (QTextLine::naturalTextRect), each put in a line `width` wide as the alignment says
    /// (QTextEngine::alignLine; AlignRight 2, AlignHCenter 4).
    fn content_width(&self, align: u32, width: f64) -> f64 {
        let width = (width * 64.0) as i64;
        let (mut left, mut right) = (f64::INFINITY, f64::NEG_INFINITY);
        for &(advance, natural) in &self.spans {
            let room = width - (advance * 64.0).round() as i64;
            let x = match align {
                2 => room,
                4 => room / 2,
                _ => 0,
            } as f64
                / 64.0;
            (left, right) = (left.min(x), right.max(x + natural));
        }
        if self.spans.is_empty() {
            0.0
        } else {
            right - left
        }
    }
}

/// QQuickText's `horizontalAlignment` and `effectiveHorizontalAlignment` (AlignLeft 1,
/// AlignRight 2, AlignHCenter 4, AlignJustify 8). One that is not set follows the direction
/// of the text (QQuickTextPrivate::determineHorizontalAlignment); one that is set has left
/// and right swapped under `LayoutMirroring` (QQuickText::effectiveHAlign).
fn text_halign(w: &mut World, i: usize, text: &str) -> (u32, u32) {
    if !explicit(w, i, "horizontalAlignment") {
        let align = 1 + qsg::text::is_right_to_left(text) as u32;
        return (align, align);
    }
    let align = num(w, i, "horizontalAlignment") as u32;
    let effective = match align {
        1 | 2 if w.mirrored(i) => 3 - align,
        _ => align,
    };
    (align, effective)
}

/// A text as QTextEngine shapes it, each run of one font and format as a whole
/// (`FontStack::shape_text`): what the lines it is broken into measure.
struct Widths {
    /// Advance in 26.6 of the clusters that start at each byte, then (`sum`) of those before it.
    before: Vec<i64>,
    /// The bytes a cluster starts at.
    starts: Vec<bool>,
    /// The script (ISO 15924 tag) of the QScriptItem that starts at each byte.
    items: Vec<Option<[u8; 4]>>,
    tabs: Vec<usize>,
    /// How far past its advance, in 26.6, the first glyph of the cluster at each byte reaches.
    overhangs: Vec<i64>,
    /// `ShapedText::stray` of each byte.
    strays: Vec<Option<i64>>,
    /// The runs, by the byte they start at.
    runs: Vec<(usize, Rc<qsg::text::ShapedText>)>,
}

impl Widths {
    fn new(text: &str) -> Widths {
        Widths {
            before: vec![0; text.len() + 1],
            starts: vec![false; text.len() + 1],
            items: vec![None; text.len() + 1],
            tabs: text.match_indices('\t').map(|(at, _)| at).collect(),
            overhangs: vec![0; text.len() + 1],
            strays: vec![None; text.len() + 1],
            runs: Vec::new(),
        }
    }

    /// `run`, at byte `at` of the text, is set in `f`.
    fn run(&mut self, at: usize, run: &str, f: &FontStack) {
        let shaped = f.shape_text(run);
        for k in 0..run.len() {
            self.starts[at + k] = shaped.cluster_start(k);
            self.items[at + k] = shaped.item_script(k);
            self.before[at + k] = shaped.advance(k, k + 1);
            self.overhangs[at + k] = shaped.overhang(k);
            self.strays[at + k] = shaped.stray(k);
        }
        // A run in another format is another item.
        if let Some((start, before)) = self.runs.last()
            && start + before.len() == at
        {
            self.strays[at] = shaped.stray_after(before);
        }
        self.runs.push((at, shaped));
    }

    /// The bytes `at..at + len` are one cluster `advance` wide; nothing with `advance` 0.
    fn object(&mut self, at: usize, len: usize, advance: i64) {
        self.before[at..at + len].fill(0);
        self.before[at] = advance;
        self.overhangs[at..at + len].fill(0);
        self.strays[at..at + len].fill(None);
        self.starts[at..at + len].fill(false);
        self.starts[at] = true;
        // An object is an item of its own: what follows it in its run is an item again.
        let around = self.items[..=at].iter().rev().find_map(|item| *item);
        self.items[at..at + len].fill(None);
        self.items[at] = Some(*b"Zyyy");
        if at + len < self.items.len() - 1 {
            self.items[at + len] = self.items[at + len].or(around);
        }
    }

    /// Once every run is in: the advances become the sums before each byte.
    fn sum(mut self) -> Widths {
        let mut sum = 0;
        for at in &mut self.before {
            sum += std::mem::replace(at, sum);
        }
        *self.starts.last_mut().unwrap() = true;
        self
    }

    /// The width of `text[a..b]` set from the start of a line, each tab as wide as the way to
    /// the next tab stop (QTextEngine::calculateTabWidth).
    fn advance(&self, a: usize, b: usize) -> i64 {
        let first = self.tabs.partition_point(|&t| t < a);
        let (mut x, mut from) = (0, a);
        for &tab in self.tabs[first..].iter().take_while(|&&t| t < b) {
            x += self.before[tab] - self.before[from];
            x += qsg::text::tab_width(x);
            from = tab + 1;
        }
        x + self.before[b] - self.before[from]
    }

    /// How far past the advance of `text[a..b]` the glyph of its last character reaches, as
    /// the text is shaped: LineBreakHelper::currentGlyph is the first glyph of that cluster.
    fn overhang(&self, a: usize, b: usize) -> i64 {
        if b <= a {
            return 0;
        }
        self.overhangs[self.last_cluster(a, b)]
    }

    /// What a line that was broken to fit reaches past its advance. LineBreakHelper keeps
    /// the glyph of the last character it took (saveCurrentGlyph) and reads it again where
    /// white space or the next word starts, from the glyphs of the item it is in by then:
    /// where an item starts there that is a glyph of the wrong item (`ShapedText::stray`),
    /// and where the item has no such glyph the bearing stays that of the word that did not
    /// fit.
    fn wrapped_overhang(&self, text: &str, l: &text_layout::Line) -> Option<i64> {
        let (a, end, next) = (l.start, l.end, l.next);
        if end <= a || l.hyphen || l.hard {
            return None;
        }
        let read = |at: usize| match self.strays[at] {
            Some(stray) => (stray >= 0).then_some(stray),
            None => Some(self.overhang(a, at)),
        };
        let kept = match next > end {
            // After white space the glyph kept before it stays, if there is one.
            true => read(end).or_else(|| self.strays[next].map_or(Some(0), |_| read(next))),
            false => read(next),
        };
        kept.or_else(|| {
            let word = text[next..]
                .char_indices()
                .skip(1)
                .find(|&(at, c)| c.is_whitespace() || self.strays[next + at].is_some())
                .map_or(text.len(), |(at, _)| next + at);
            Some(self.overhang(next, word))
        })
    }

    /// Where the last cluster of `text[a..b]` starts.
    fn last_cluster(&self, a: usize, b: usize) -> usize {
        (a..b).rev().find(|&at| self.starts[at]).unwrap_or(a)
    }

    /// The largest ascent, descent and leading of the script items `text[a..b]` has a part of
    /// (QTextLine::layout_helper gives a line the metrics of each item it takes text from).
    fn metrics(&self, a: usize, b: usize) -> Option<[i64; 3]> {
        self.runs
            .iter()
            .filter(|run| run.0 < b)
            .filter_map(|(at, shaped)| shaped.metrics(a.saturating_sub(*at), b - at))
            .reduce(|m, n| [m[0].max(n[0]), m[1].max(n[1]), m[2].max(n[2])])
    }
}

/// QQuickText's lines of plain text: broken at line separators and, wrapped, at the Unicode
/// line break opportunities; elided and cut off as `bounds` say.
fn plain_laid(f: &FontStack, text: &str, bounds: &Bounds) -> Laid<qsg::text::ShapedLine> {
    let widths = std::cell::OnceCell::new();
    let widths = || {
        widths.get_or_init(|| {
            let mut widths = Widths::new(text);
            widths.run(0, text, f);
            widths.sum()
        })
    };
    let hyphen = |on: bool| if on { f.shape("-").advance } else { 0 };
    let measure = |a: usize, b: usize, h: bool| (widths().advance(a, b) + hyphen(h)) as f64 / 64.0;
    // A wrapped line measures what its part of the whole text does; one that only line
    // separators end is shaped on its own.
    let wrapped = bounds.wrap != Wrap::None;
    // QQuickText lays all its lines out as one paragraph (a `\n` is a line separator in
    // it): a line of text that has right-to-left characters is set in the direction of the
    // whole text, its characters at the levels they have in it.
    let bidi = text.chars().any(|c| c >= '\u{590}');
    // QTextEngine::itemize: every line has the direction of the whole text.
    let rtl = qsg::text::is_right_to_left(text);
    let whole = |l: &text_layout::Line| {
        let print = printed(&text[l.start..l.end], l.hyphen);
        let part = bidi && matches!(print, std::borrow::Cow::Borrowed(_)) && !print.contains('\t');
        let mut line = match part {
            true => f.shape_part(text, l.start..l.end),
            false => shape_tabbed(f, &print, rtl),
        };
        if wrapped {
            line.advance = widths().advance(l.start, l.end) + hyphen(l.hyphen);
            if let Some([ascent, descent, leading]) = widths().metrics(l.start, l.end) {
                (line.ascent, line.descent, line.leading) = (ascent, descent, leading);
            }
        }
        line
    };
    let natural = |l: &text_layout::Line| {
        let line = whole(l);
        let broken = wrapped
            .then(|| widths().wrapped_overhang(text, l))
            .flatten();
        broken.map_or_else(|| f.natural_width(&line), |reach| line.advance + reach) as f64 / 64.0
    };
    let overhang = |a: usize, b: usize| widths().overhang(a, b) as f64 / 64.0;
    let mut flow = text_layout::flow(
        text,
        &[],
        bounds,
        &text_layout::Measures {
            shaped: &|at| widths().starts[at],
            item: &|at| widths().items[at],
            measure: &measure,
            overhang: &overhang,
            natural: &natural,
            height: &|l| whole(l).height(),
        },
    );
    let span = |l: qsg::text::ShapedLine| {
        (
            (l.advance as f64 / 64.0, f.natural_width(&l) as f64 / 64.0),
            l,
        )
    };
    let (mut spans, lines): (Vec<_>, Vec<qsg::text::ShapedLine>) = flow
        .rows
        .iter()
        .map(|row| {
            let l = &row.line;
            let Some(mode) = elide_mode(text, row, bounds.width, &measure) else {
                let line = whole(l);
                return ((line.advance as f64 / 64.0, natural(l)), line);
            };
            let breaks = flow.breaks.get_or_insert_with(|| Breaks::new(text, &[]));
            let (ellipsis, room) = f.ellipsis();
            let kept = text_layout::elide(
                breaks,
                mode,
                l.start,
                l.next,
                bounds.width,
                room,
                &|a, b| measure(a, b, false),
            );
            let Some((head, tail)) = kept else {
                return span(f.shape(""));
            };
            let elided = format!(
                "{}{}{}",
                printed(&text[l.start..head], false),
                text_layout::ellipsis_between(
                    text,
                    mode,
                    (l.start, l.next),
                    (head, tail),
                    ellipsis
                ),
                printed(
                    text[tail..l.next].trim_end_matches(text_layout::is_separator),
                    false
                )
            );
            if wrapped {
                // QQuickTextPrivate::setupTextLayout lays the elided text out as a line of
                // the same width and wrap mode: what is still too wide, as three full stops
                // with the letter spacing after each are, wraps, and its first line is shown.
                let again = Bounds::wrapped(bounds.wrap, bounds.width);
                let again = plain_laid(f, &elided, &again);
                if let (Some(first), Some(&span)) =
                    (again.lines.into_iter().next(), again.spans.first())
                {
                    return (span, first);
                }
            }
            span(shape_tabbed(
                f,
                &elided,
                qsg::text::is_right_to_left(&elided),
            ))
        })
        .unzip();
    if let (true, Some(row)) = (flow.height_cut, flow.rows.last()) {
        // QQuickText counts the line it elides for the height unelided too.
        let l = &row.line;
        spans.push((whole(l).advance as f64 / 64.0, natural(l)));
    }
    Laid {
        lines,
        starts: flow.rows.iter().map(|row| row.line.start).collect(),
        truncated: flow.truncated,
        spans,
        implicit_width: flow.max_width,
        implicit_height: flow.lines.iter().map(|l| whole(l).height()).sum(),
    }
}

/// How a row is elided, as `Text.elide` (ElideLeft 0, ElideRight 1, ElideMiddle 2), if it is.
/// QTextEngine::elidedText leaves alone a line of one character, and one from which the rest
/// of the text, were it one line, fits the width.
fn elide_mode(
    text: &str,
    row: &text_layout::Row,
    width: f64,
    measure: &dyn Fn(usize, usize, bool) -> f64,
) -> Option<u32> {
    let mode = match row.cut? {
        Cut::Width(mode) => mode,
        Cut::Rest => 1,
    };
    let line = &row.line;
    let mut rest = 0.0;
    let mut at = line.start;
    for l in text[line.start..].split(text_layout::is_separator) {
        rest += measure(at, at + l.len(), false);
        at += l.len()
            + text[at + l.len()..]
                .chars()
                .next()
                .map_or(0, char::len_utf8);
    }
    let one = text[line.start..line.next].chars().nth(1).is_none();
    (rest > width && !one).then_some(mode)
}

/// Lines broken at line separators only.
fn text_lines(f: &FontStack, text: &str) -> Vec<qsg::text::ShapedLine> {
    text_line_strs(f, text, None)
        .iter()
        .map(|l| f.shape(l))
        .collect()
}

/// Editor lines as painted (breaks at spaces only, so caret indices stay one char per break).
fn edit_lines(f: &FontStack, text: &str, wrap: Option<f64>) -> Vec<qsg::text::ShapedLine> {
    text_line_strs(f, text, wrap)
        .iter()
        .map(|l| f.shape(l))
        .collect()
}

/// Lines of an editor's `text` after word wrap; a break at a space consumes it ('\n' too).
fn text_line_strs(f: &FontStack, text: &str, wrap: Option<f64>) -> Vec<String> {
    text.split(['\n', '\u{2028}'])
        .flat_map(|para| para_lines(f, para, wrap))
        .map(|l| l.text)
        .collect()
}

/// A line of a wrapped paragraph of an editor.
struct ParaLine {
    /// Its characters, the white space it ends in too.
    text: String,
    /// How many characters at its end are white space it was broken at: the spaces, or the
    /// tab that did not fit.
    hung: usize,
    /// The space after it that the break consumed: 1, or 0 after a tab and at the end.
    gap: usize,
}

/// The lines of the paragraph `para` wrapped in `wrap` px: broken at spaces and after tabs
/// (QTextLine::layout_helper).
fn para_lines(f: &FontStack, para: &str, wrap: Option<f64>) -> Vec<ParaLine> {
    let whole = |text: String| ParaLine {
        text,
        hung: 0,
        gap: 0,
    };
    let Some(maxw) = wrap else {
        return vec![whole(para.to_string())];
    };
    let adv = |t: &str| f.shape(t).advance as f64 / 64.0;
    let at_space = |text: String| ParaLine {
        hung: text.len() - text.trim_end_matches(' ').len(),
        text,
        gap: 1,
    };
    let mut out = Vec::new();
    let mut cur = String::new();
    // Leading spaces stay on the paragraph's first line (` | x` keeps its space).
    let (mut started, mut broke) = (false, false);
    for word in para.split(' ') {
        for (k, piece) in word.split('\t').enumerate() {
            if k > 0 {
                // A tab that does not fit in a line with text ends it, as white space does.
                if !cur.is_empty() && adv(&format!("{cur}\t")) > maxw {
                    out.push(ParaLine {
                        text: std::mem::take(&mut cur) + "\t",
                        hung: 1,
                        gap: 0,
                    });
                    cur = piece.to_string();
                    broke = false;
                    continue;
                }
                // One that fits is text of the line, and the line can end after it.
                cur.push('\t');
                if !piece.is_empty() && adv(&format!("{cur}{piece}")) > maxw {
                    out.push(whole(std::mem::replace(&mut cur, piece.to_string())));
                    broke = false;
                } else {
                    cur.push_str(piece);
                }
                continue;
            }
            let cand = if !started || (cur.is_empty() && broke) {
                piece.to_string()
            } else {
                format!("{cur} {piece}")
            };
            started = true;
            // The spaces a line ends in take no room in it (QTextLine: they hang past its end).
            if !cur.is_empty() && adv(cand.trim_end_matches(' ')) > maxw {
                out.push(at_space(std::mem::replace(&mut cur, piece.to_string())));
                broke = true;
            } else {
                cur = cand;
            }
        }
    }
    // No line follows a tab that ends the paragraph: there is no text left to lay out.
    let ended = cur.is_empty() && out.last().is_some_and(|l| l.hung == 1 && l.gap == 0);
    if !ended {
        out.push(whole(cur));
    }
    out
}

/// The width (26.6) of an editor's `text` wrapped in `room`: of its widest line. The spaces a
/// line was broken at are not in it, and the ones a paragraph ends in are when they fit in
/// `room` with its last line (QTextLine::layout_helper).
fn edit_wrapped_width(f: &FontStack, text: &str, room: f64) -> i64 {
    let mut wd = 0;
    for para in text.split(['\n', '\u{2028}']) {
        let strs = para_lines(f, para, Some(room));
        for (n, l) in strs.iter().enumerate() {
            let s = &l.text;
            let mut line = f.shape(s);
            if n + 1 < strs.len() || line.advance as f64 / 64.0 > room {
                let inked = match l.hung {
                    0 => s.trim_end_matches(' '),
                    hung => &s[..s.len() - hung],
                };
                if inked.len() < s.len() {
                    line = f.shape(inked);
                }
            }
            wd = wd.max(f.natural_width(&line));
        }
    }
    wd
}

/// The text caret's blink: whether it shows now, and in how many ms that changes (never, when
/// it does not blink). Restarted visible on every edit, move and focus change.
pub fn caret_phase(w: &World) -> (bool, Option<f64>) {
    let ms = w
        .caret_reset
        .map_or(0.0, |t| t.elapsed().as_secs_f64() * 1000.0);
    caret_blink(qml::colorscheme::cursor_flash_time(), ms)
}

/// QQuickTextInputPrivate::updateCursorBlinking and QQuickTextControlPrivate::
/// updateCursorFlashTime: a timer of half of QStyleHints::cursorFlashTime toggles the caret,
/// starting visible; a flash time under 2 ms starts no timer and the caret stays on.
fn caret_blink(flash_ms: i32, since_restart_ms: f64) -> (bool, Option<f64>) {
    if flash_ms < 2 {
        return (true, None);
    }
    let half = f64::from(flash_ms / 2);
    let visible = ((since_restart_ms / half) as u64).is_multiple_of(2);
    (visible, Some(half - since_restart_ms % half))
}

/// NativeRendering glyphs of a shaped line on the GPU (through the frame's scale, if any).
#[allow(clippy::too_many_arguments)]
fn gpu_glyphs(
    fr: &mut crate::gpu::Frame,
    f: &FontStack,
    line: &qsg::text::ShapedLine,
    x: f64,
    y: f64,
    dx: f64,
    dy: f64,
    col: [u8; 4],
    op: f32,
) {
    if fr.dpr != 1.0 {
        if let Some(st) = f.scaled(fr.dpr) {
            fr.text_dpr(st, Rc::new(line.clone()), [x, y, dx, dy], col, op);
        }
    } else if fr.xf.is_some() {
        fr.glyphs_f(f.line_quads_f(line, 0.0, 0.0, dx, dy), x, y, col, op);
    } else {
        fr.glyphs(f.line_quads_f(line, 0.0, 0.0, dx, dy).into(), x, y, col, op);
    }
}

fn edit_line(
    cv: &mut Target,
    f: &FontStack,
    line: &qsg::text::ShapedLine,
    t: [f64; 2],
    at: [f64; 2],
    col: [u8; 4],
    op: f32,
) {
    let ([x, y], [dx, dy]) = (t, at);
    match cv {
        Target::Cpu(c) => cpu_line(c, f, line, t, at, col, op),
        Target::Gpu(fr) => gpu_glyphs(fr, f, line, x, y, dx, dy, col, op),
    }
}

/// How a Text.QtRendering text is drawn.
#[derive(Clone, Copy, Default)]
struct DfText {
    /// Its glyph node has been recycled: gray antialiasing (`df_node_recycled`).
    gray: bool,
    /// Text.renderTypeQuality, 0 when not asked for.
    quality: i32,
    /// Text.style (QQuickText::TextStyle: Outline 1, Raised 2, Sunken 3) and styleColor.
    style: u32,
    style_color: [u8; 4],
    /// The text is under a clip node of its own, which is a batch root of the renderer (the
    /// text of a TextField or a TextArea, selected text over its selection): what of its
    /// place in the item is in the root's matrix, and what the node is moved by in the root.
    /// The vertex shader gets the root's matrix and the vertices are in the root.
    root: Option<([f64; 2], [f64; 2])>,
}

fn df_text(w: &mut World, i: usize, gray: bool) -> DfText {
    DfText {
        gray,
        quality: opt_num(w, i, "renderTypeQuality").map_or(0, |q| (q as i32).max(0)),
        style: opt_num(w, i, "style").map_or(0, |s| s as u32),
        style_color: color(w, i, "styleColor", [0, 0, 0, 255]),
        root: None,
    }
}

/// `edit_line`, or distance-field glyphs for Text.QtRendering.
#[allow(clippy::too_many_arguments)]
fn rt_line(
    cv: &mut Target,
    f: &FontStack,
    line: &qsg::text::ShapedLine,
    x: f64,
    y: f64,
    dx: f64,
    dy: f64,
    col: [u8; 4],
    op: f32,
    df: Option<DfText>,
) {
    use qsg::dftext::DfLook;
    let Some(df) = df else {
        return edit_line(cv, f, line, [x, y], [dx, dy], col, op);
    };
    // QColor::getRgbF, then QSGDistanceFieldTextMaterial::setColor (premultiplied) or the
    // subpixel material's (not), times the opacity in updateUniformData.
    let premul = |c: [u8; 4]| {
        let [r, g, b, a] = c.map(|c| (c as u16 * 257) as f32 / 65535.0);
        [r * a, g * a, b * a, a].map(|c| c * op)
    };
    let look = match df.style {
        1 => DfLook::Outline(premul(df.style_color)),
        2 => DfLook::Shifted(premul(df.style_color), 1.0),
        3 => DfLook::Shifted(premul(df.style_color), -1.0),
        _ if df.gray => DfLook::Gray,
        _ => DfLook::Subpixel,
    };
    let color = if look == DfLook::Subpixel {
        let [r, g, b, a] = col.map(|c| (c as u16 * 257) as f32 / 65535.0);
        [r, g, b, a * op]
    } else {
        premul(col)
    };
    let k = (look == DfLook::Subpixel).then_some([col[0], col[1], col[2]]);
    // QSGInternalTextNode::addGlyphs: outlined text is the outline node, then a gray node of
    // the text itself over it.
    let fill = df.style == 1 && col[3] > 0 && df.style_color != col;
    use qsg::dftext::DfView;
    // Qt's render target: a viewport of device pixels, a projection over that size divided
    // by the screen's scale (QQuickWindowPrivate::renderSceneGraph). A layer's texture is one
    // too: the projection is over its source rect and the scale is the larger of its two
    // (QSGRhiLayer::grab), so a rect of a size that is not whole is not in the text's matrix.
    // Under any other scale (a scaled parent) the text is placed in device pixels.
    let (view, screen, xf) = match cv {
        Target::Cpu(c) => {
            let scaled = cpu_scaled(c, |l| ([l.out.w, l.out.h], l.s, l.layer.then_some(l.size)));
            match scaled {
                Some(([w, h], s, Some(rect))) => {
                    let view = DfView {
                        size: [w as f32, h as f32],
                        proj: rect.map(|v| v as f32),
                        dpr: s[0].max(s[1]) as f32,
                    };
                    (view, false, None)
                }
                Some(([w, h], [sx, sy], None)) => {
                    let xf = Some([sx as f32, sy as f32, 0.0, 0.0]);
                    (DfView::plain(w as f32, h as f32), false, xf)
                }
                None => (DfView::plain(c.w as f32, c.h as f32), true, None),
            }
        }
        Target::Gpu(fr) => {
            let size = [fr.w as f32, fr.h as f32];
            match (fr.layer_target(), fr.screen_scale()) {
                (Some((proj, dpr)), screen) => (DfView { size, proj, dpr }, screen.is_some(), None),
                (None, Some(dpr)) => {
                    let proj = [fr.lw as f32, fr.lh as f32];
                    (DfView { size, proj, dpr }, true, None)
                }
                (None, None) => (DfView::plain(size[0], size[1]), false, fr.xf),
            }
        }
    };
    // The text's node as Qt's scene graph has it, when this is where that node draws.
    let node = DF_AT
        .with(|c| c.get())
        .and_then(|i| DF_ITEMS.with(|m| m.borrow().get(&i).copied()))
        .filter(|d| screen && (d.pos[0] - x).abs() < 1e-6 && (d.pos[1] - y).abs() < 1e-6);
    let rooted = df.root.filter(|_| node.is_none() && xf.is_none());
    let place = |merged: fn(&DfNode) -> bool| match (node, xf, rooted) {
        // A merged batch: the vertices are moved to its root, whose matrix the shader has.
        (Some(d), ..) if merged(&d) => (
            [d.local.m[3][0] as f64, d.local.m[3][1] as f64],
            d.rooted().then(|| df_turn(&d.root, view.proj)),
        ),
        // Not merged: the shader has the root's matrix times the node's.
        (Some(d), ..) if d.rooted() => ([0.0; 2], Some(df_turn(&d.root.mul(&d.local), view.proj))),
        (Some(d), ..) => ([0.0; 2], Some(df_turn(&d.mat, view.proj))),
        // The scale of an item above the text is in its matrix.
        (None, Some([sx, sy, ox, oy]), _) => {
            let mut m = Mat4::identity();
            m.translate(sx * x as f32 + ox, sy * y as f32 + oy, 0.0);
            m.scale(sx, sy, 1.0);
            ([0.0; 2], Some(df_turn(&m, view.proj)))
        }
        (None, None, Some((in_root, moved))) => {
            let mut m = Mat4::identity();
            m.translate((x + in_root[0]) as f32, (y + in_root[1]) as f32, 0.0);
            (moved, Some(df_turn(&m, view.proj)))
        }
        (None, None, None) => ([x, y], None),
    };
    let pen = match rooted {
        Some((a, b)) => [dx - a[0] - b[0], dy - a[1] - b[1]],
        None => [dx, dy],
    };
    let mut draw = |look, merged: fn(&DfNode) -> bool| {
        let (at, turn) = place(merged);
        let (q, [px, py]) = (df.quality, pen);
        let tiles = f.line_df_tiles(line, q, at[0], at[1], px, py, color, look, turn, view);
        let k = k.filter(|_| look == DfLook::Subpixel);
        match cv {
            Target::Cpu(c) => cpu_direct(c, |c, _| qsg::dftext::blit(c, &tiles, k)),
            Target::Gpu(fr) => fr.df_tiles(tiles, k),
        }
    };
    draw(look, |d| d.merged);
    if fill {
        draw(DfLook::Gray, |d| d.fill_merged);
    }
    if let Some(cl) = f.color_part(line) {
        edit_line(cv, f, &cl, [x, y], [dx, dy], col, op);
    }
}

/// What Qt's scene graph holds for a QtRendering Text in this frame (`df_nodes`).
#[derive(Clone, Copy)]
struct DfNode {
    /// The geometry node's matrix: the item's and every ancestor's, multiplied as
    /// QSGNodeUpdater multiplies QMatrix4x4s.
    mat: Mat4,
    /// The matrix of its batch root, the clip node of the nearest ancestor that clips
    /// (Updater::visitClipNode: every clip node is one), and its own from there. The
    /// renderer moves the vertices of a merged batch by `local` and hands the shader `root`;
    /// both are `float`s, so the sum is not `mat`'s to the last bit.
    root: Mat4,
    local: Mat4,
    /// Which node that root is: batches do not cross roots.
    root_of: (usize, u8),
    /// Where `paint` draws the item when only moves lead to it.
    pos: [f64; 2],
    /// Only moves lead to its parent.
    plain: bool,
    /// No clip between it and the first turned or scaled item above it.
    free: bool,
    /// The opacity its parent hands it.
    opacity: f32,
    /// The renderer merges its batch: every text in it is only moved.
    merged: bool,
    /// The same of the fill node of outlined text.
    fill_merged: bool,
}

impl DfNode {
    /// A clip node lies above it: its batch has a root other than the window's.
    fn rooted(&self) -> bool {
        self.root.flags != qml::valuetypes::IDENTITY
    }
}

thread_local! {
    static DF_ITEMS: std::cell::RefCell<HashMap<usize, DfNode>> = Default::default();
    /// The Text whose lines are being drawn.
    static DF_AT: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
    /// Where `paint` draws the topmost turned or scaled items of the window.
    static DF_TURNED: std::cell::RefCell<HashMap<usize, [f64; 2]>> = Default::default();
    /// The QtRendering texts under the turned item being drawn, left out of its layer.
    static DF_LATER: std::cell::RefCell<Option<DfLater>> = const { std::cell::RefCell::new(None) };
    /// The text `df_later` is drawing through its node's matrix.
    static DF_NOW: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

/// The matrices of a text in a batch that is not merged: Renderer's projection
/// (QMatrix4x4::ortho(0, w, h, 0, 1, -1)) times the node's matrix `mv`.
fn df_turn(mv: &Mat4, [vw, vh]: [f32; 2]) -> qsg::dftext::DfTurn {
    let mut p = Mat4::identity();
    p.translate(-(0.0f32 + vw) / vw, -(0.0f32 + vh) / (0.0f32 - vh), 0.0);
    p.scale(1.0, 1.0, 1.0);
    p.m[0][0] = 2.0f32 / vw;
    p.m[1][1] = 2.0f32 / (0.0f32 - vh);
    let mvp = p.mul(mv);
    let inv = mvp.inverted();
    qsg::dftext::DfTurn {
        mvp: [
            mvp.m[0][0],
            mvp.m[0][1],
            mvp.m[1][0],
            mvp.m[1][1],
            mvp.m[3][0],
            mvp.m[3][1],
        ],
        det: mv.determinant() as f32,
        inv0: [inv.m[0][0], inv.m[0][1]],
    }
}

/// QQuickWindow::textRenderType(), one for the process as in Qt: what a text whose
/// `renderType` is not set draws with. Qt's is QtTextRendering; quickshell makes it native
/// only under `//@ pragma NativeTextRendering`.
static NATIVE_TEXT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// QQuickWindow::setTextRenderType(NativeTextRendering) when `on`.
pub fn set_native_text_rendering(on: bool) {
    NATIVE_TEXT.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// The text draws with distance fields: Text.QtRendering, given or the window's default.
fn qt_rendering(w: &mut World, i: usize) -> bool {
    if explicit(w, i, "renderType") {
        num(w, i, "renderType") == 0.0
    } else {
        !NATIVE_TEXT.load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// An underline or a strike-out of a line of rich text: its (offset from the baseline,
/// lineThickness) and the line's ascent in 26.6, its width and left end and the line's top.
/// QQuickTextNodeEngine::addToSceneGraph adds the lines after the glyphs of the whole text.
struct Underline {
    metrics: (f64, i64),
    ascent: i64,
    width: f64,
    x: f64,
    top: f64,
    color: [u8; 4],
}

/// An underline or a strike-out, drawn after all the glyphs of the text as Qt draws them.
/// QQuickTextNodeEngine::addTextDecorations and QSGRhiInternalTextNode::addDecorationNode:
/// the line is `lineThickness` high from round(lineTop + ascent + its offset) down,
/// and is drawn as a stroke of that width along its middle (the stroke of Qt Quick Shapes'
/// curve renderer, flat ends), so its edges are as soft as a Shape's.
fn underline(cv: &mut Target, u: &Underline, op: f32) {
    use qsg::shape::{self, Cap, Join, PathCmd, Stroke};
    let &Underline {
        metrics: (pos, th),
        ascent,
        width: wd,
        x,
        top,
        color: col,
    } = u;
    let th = th as f64 / 64.0;
    if wd <= 0.0 || th <= 0.0 {
        return;
    }
    let y = (top + (ascent as f64 + pos) / 64.0).round();
    let stroke = Stroke {
        width: th as f32,
        color: col,
        cap: Cap::Flat,
        join: Join::Miter,
        miter_limit: 2.0,
    };
    let middle = shape::stroke_elements(&[PathCmd::Line(wd, th / 2.0)], (0.0, th / 2.0));
    let tris = shape::stroke_tris(&middle, &stroke);
    let paint = |c: &mut Canvas, ox: f64, oy: f64| {
        shape::draw_stroke(c, &tris, &stroke, (x - ox) as f32, (y - oy) as f32, op)
    };
    let b = (x.floor() - 2.0, y - 2.0, wd.ceil() + 5.0, th.ceil() + 4.0);
    match cv {
        // The line reaches the picture as 8 bits a channel, as it does through a texture:
        // where a glyph crosses the line's soft edge Qt's pixels are the 8-bit blend.
        Target::Cpu(cv) => {
            let mut c = Canvas::new(b.2 as usize, b.3 as usize, [0, 0, 0, 0]);
            paint(&mut c, b.0, b.1);
            qsg::layer::draw(cv, &c, b.0, b.1, b.2, b.3, 1.0);
        }
        Target::Gpu(_) => raster(cv, b, paint),
    }
}

/// QQuickTextNodeEngine::addToSceneGraph: the background of a run fills the line's height
/// under it.
fn run_background(
    cv: &mut Target,
    line: &RichLine,
    run: &qsg::text::ShapedLine,
    x: f64,
    top: f64,
    deco: &Deco,
    op: f32,
) {
    if let Some(col) = deco.3 {
        solid_rect(
            cv,
            x,
            top,
            run.advance as f64 / 64.0,
            line.height(),
            col,
            op,
        );
    }
}

fn solid_rect(cv: &mut Target, x: f64, y: f64, wd: f64, ht: f64, col: [u8; 4], op: f32) {
    if wd <= 0.0 || ht <= 0.0 {
        return;
    }
    let g = qsg::rect::build(&qsg::rect::RectParams {
        width: wd as f32,
        height: ht as f32,
        color: col,
        ..Default::default()
    });
    match cv {
        Target::Cpu(c) => cpu_smooth(c, &g, x, y, op),
        Target::Gpu(fr) => fr.smooth(&Rc::new(g), x, y, op),
    }
}

/// UTF-16 position `u` of editor `i` (the units of QML's cursorPosition) as a char index
/// into the text it lays out: displayText with an inputMask, else text.
pub fn edit_char(w: &mut World, i: usize, u: usize) -> usize {
    let shown = if w.input_mask(i).is_some() {
        "displayText"
    } else {
        "text"
    };
    match w.read(i, shown).as_ref().and_then(Value::host_str) {
        Some(s) => {
            let cs: Vec<char> = s.chars().collect();
            qml::textpos::to_char(&cs, u)
        }
        None => 0,
    }
}

/// TextInput/TextEdit (TextField/TextArea): both expose cursorPosition.
/// A Text, or a type derived from it (Controls Label).
pub(crate) fn is_text(w: &World, i: usize) -> bool {
    w.items[i].inherits("Text")
}

pub(crate) fn is_text_edit(w: &World, i: usize) -> bool {
    !is_text(w, i) && w.items[i].prop_info("cursorPosition").is_some()
}

/// How far `clipRect()` of an editor reaches past its right edge, for the cursor: the width of
/// the cursor (QQuickTextInput::clipRect), 1 px as no `cursorDelegate` is made into an item
/// here, and 3 px more in a TextEdit with text (QQuickTextEdit::clipRect).
pub(crate) fn edit_clip_extra(w: &mut World, i: usize) -> f64 {
    if !is_text_edit(w, i) {
        return 0.0;
    }
    let gap =
        is_text_doc(w, i) && matches!(w.read(i, "text"), Some(Value::Str(s)) if !s.is_empty());
    if gap { 4.0 } else { 1.0 }
}

/// A QTextDocument-backed editor (TextEdit / TextArea), not a TextInput.
pub(crate) fn is_text_doc(w: &World, i: usize) -> bool {
    is(w, i, "TextEdit") || is(w, i, "TextArea")
}

/// `horizontalAlignment` of an editor. One nobody set follows the editor's text: AlignRight
/// when its first letter with a direction is of a right-to-left script, else AlignLeft
/// (determineHorizontalAlignment of QQuickTextInputPrivate and QQuickTextEditPrivate).
fn edit_halign(w: &mut World, i: usize) -> u32 {
    if explicit(w, i, "horizontalAlignment") {
        return num(w, i, "horizontalAlignment") as u32;
    }
    if edit_rtl(w, i) { 2 } else { 1 }
}

/// Whether the text of an editor is set right to left: the direction of its first letter
/// that has one.
fn edit_rtl(w: &mut World, i: usize) -> bool {
    matches!(w.read(i, "text"), Some(Value::Str(s)) if qsg::text::is_rtl(&s))
}

/// Shown string of an editor: its text, else the Controls placeholderText.
fn edit_style(w: &mut World, fonts: &mut Fonts, i: usize) -> (TextStyle, bool) {
    let mut ts = text_style(w, fonts, i);
    // A masked TextInput shows m_text (blanks and separators), not the stripped `text`.
    if w.items[i].prop_info("inputMask").is_some()
        && matches!(w.read(i, "inputMask"), Some(Value::Str(m)) if !m.is_empty())
        && let Some(d) = w.read(i, "displayText").as_ref().and_then(Value::host_str)
    {
        ts.text = d.into_owned();
    }
    // An input method's preedit string is shown at the cursor (QQuickTextInputPrivate::
    // m_textLayout preedit area).
    if let Some((pre, _)) = crate::keys::preedit(i) {
        let b = qml::textpos::to_byte(&ts.text, num(w, i, "cursorPosition").max(0.0) as usize);
        ts.text.insert_str(b, &pre);
    }
    let placeholder = ts.text.is_empty();
    if placeholder && !material_field(w, i) {
        ts.text = match w.read(i, "placeholderText") {
            Some(Value::Str(s)) => s.to_string(),
            _ => String::new(),
        };
    }
    // QQuickTextInput::EchoMode: NoEcho shows nothing, Password/PasswordEchoOnEdit mask.
    if !placeholder && w.items[i].prop_info("echoMode").is_some() {
        match num(w, i, "echoMode") as i32 {
            1 => ts.text.clear(),
            2 | 3 => {
                let mask = match w.read(i, "passwordCharacter") {
                    Some(Value::Str(c)) if !c.is_empty() => c.chars().next().unwrap(),
                    _ => '\u{25cf}',
                };
                ts.text = std::iter::repeat_n(mask, ts.text.chars().count()).collect();
            }
            _ => {}
        }
    }
    (ts, placeholder)
}

/// A plain-text editor line as painted: chars `start..start + text.chars().count()` (a wrap at a
/// space or '\n' consumes that char), its text set from (x, y) in the item, `h` high.
pub(crate) struct EditLine {
    pub start: usize,
    pub text: String,
    pub x: f64,
    pub y: f64,
    pub h: f64,
    /// The line as drawn: set in the direction of its paragraph.
    pub shaped: qsg::text::ShapedLine,
    /// Where the caret stands in the line's paragraph, and the line in it.
    map: Rc<qsg::text::LineMap>,
    line: qsg::text::MapLine,
    /// The last line of its paragraph.
    last: bool,
    /// The item x a wrapped line is between (QTextLine::cursorToX keeps the caret in them).
    bound: Option<(f64, f64)>,
}

impl EditLine {
    /// QTextLine::cursorToX: the item x of the caret before char `c` of the line.
    fn cursor_to_x(&self, c: usize) -> f64 {
        let x = self.x + self.map.cursor_to_x_in(&self.line, self.line.from + c) as f64 / 64.0;
        match self.bound {
            Some((a, b)) => x.max(a).min(b),
            None => x,
        }
    }

    /// QTextLine::xToCursor(CursorBetweenCharacters): the caret of the text nearest item x
    /// `px` on this line. Right of a wrapped right-to-left line that is the caret before the
    /// line's first char. QTextLine::xToCursor works in QFixed.
    fn x_to_cursor(&self, px: f64) -> usize {
        let x = ((px - self.x) * 64.0).round() as i64;
        let pos = self.map.x_to_cursor_in(&self.line, x, self.last);
        (self.start + pos).saturating_sub(self.line.from)
    }

    /// Where chars `a..b` of the line are drawn, as (left, right) from `x`: a stretch for
    /// every run of one direction they are in.
    fn ranges(&self, a: usize, b: usize) -> Vec<(f64, f64)> {
        let from = self.line.from;
        self.map
            .ranges(&self.line, from + a, from + b)
            .into_iter()
            .map(|(l, r)| (l as f64 / 64.0, r as f64 / 64.0))
            .collect()
    }
}

/// Lines of a plain-text TextInput/TextEdit of size `wd` x `ht` showing `text`, as `paint_body`
/// lays them out. `text` is the editor's own, or with `placeholder` its Controls placeholder,
/// which is a Text item over the editor (PlaceholderText in TextArea.qml), not its
/// QTextDocument: its lines have no leading.
fn edit_layout(
    w: &mut World,
    f: &FontStack,
    i: usize,
    text: &str,
    placeholder: bool,
    wd: f64,
    ht: f64,
) -> Vec<EditLine> {
    let [l, t, r, b] = edit_padding(w, i);
    let ww = edit_wrap_width(w, i, l + r);
    let ha = edit_halign(w, i);
    let k = edit_valign(w, i);
    let doc = is_text_doc(w, i) && !placeholder;
    // Neither QQuickTextInput's layout nor a QTextDocument's has
    // QTextOption::IncludeTrailingSpaces.
    let keep_spaces = false;
    let mut lines = Vec::new();
    // Every paragraph of a QTextDocument is set in the direction of the document's text
    // (QQuickTextEditPrivate::contentDirection, the default text option's direction).
    let dir = doc.then(|| qsg::text::is_rtl(text));
    let mut start = 0;
    for para in text.split(['\n', '\u{2028}']) {
        let graphemes: std::collections::HashSet<usize> =
            icu_segmenter::GraphemeClusterSegmenter::new()
                .segment_str(para)
                .collect();
        let mut map = f.line_map_in(para, dir, &|b| graphemes.contains(&b));
        let bytes: Vec<usize> = para.char_indices().map(|c| c.0).collect();
        let byte = |c: usize| bytes.get(c).copied().unwrap_or(para.len());
        let strs = para_lines(f, para, ww);
        let mut from = 0;
        let mut laid = Vec::with_capacity(strs.len());
        for (n, l) in strs.iter().enumerate() {
            let s = &l.text;
            let last = n + 1 == strs.len();
            let chars = s.chars().count();
            // The white space a line was broken at hangs out of it; what the paragraph ends
            // with is text of its last line, and so is a tab the line fits.
            let (len, trailing) = (chars - l.hung, l.hung + l.gap);
            let inked = if keep_spaces { len + trailing } else { len };
            let shaped = f.shape_part_in(para, dir, byte(from)..byte(from + inked));
            let spaces = shaped.advance
                - f.shape_part_in(para, dir, byte(from)..byte(from + len))
                    .advance;
            let line = qsg::text::MapLine {
                from,
                len,
                trailing,
                text_width: f.natural_width(&shaped).max(shaped.advance - spaces) + spaces,
                keep_spaces,
            };
            // The tabs of a line are as wide as their place in that line makes them.
            map.shape_line(&line);
            laid.push((s, shaped, line, last));
            from += chars + l.gap;
        }
        let map = Rc::new(map);
        for (s, shaped, line, last) in laid {
            let mut room = ((wd - l - r) * 64.0).round() as i64;
            if doc {
                // QTextDocumentLayoutPrivate::layoutBlock: a line wider than the document is
                // as wide as its text.
                room = room.max(line.text_width);
            }
            let h = if doc {
                shaped.doc_height()
            } else {
                shaped.height()
            };
            lines.push(EditLine {
                start: start + line.from,
                text: s.clone(),
                x: l + map.align(&line, room, ha) as f64 / 64.0,
                y: 0.0,
                h,
                shaped,
                map: map.clone(),
                line,
                last,
                // QTextLine::cursorToX keeps the caret inside a line that wraps; a
                // QQuickTextInput's layout is back at QTextOption::NoWrap once laid out.
                bound: ww.filter(|_| doc).map(|_| (l, l + room as f64 / 64.0)),
            });
        }
        start += para.chars().count() + 1;
    }
    let mut dy = if k > 0.0 {
        let th: f64 = lines.iter().map(|l| l.shaped.height()).sum();
        t + k * (ht - t - b - th)
    } else {
        t
    };
    for el in &mut lines {
        el.y = dy;
        dy += el.h;
    }
    if !placeholder && !is_text_doc(w, i) {
        let scroll = input_scroll(w, i, text, &lines, l, wd - l - r);
        for el in &mut lines {
            el.x -= scroll;
        }
    }
    lines
}

/// What decided a TextInput's `hscroll`, and the scroll.
#[derive(PartialEq, Clone, Copy)]
struct InputScroll {
    text: u64,
    cursor: usize,
    /// The caret in the laid-out line, the width the line uses and the room for it.
    at: f64,
    used: f64,
    room: f64,
    scroll: f64,
}

thread_local! {
    static INPUT_SCROLL: std::cell::RefCell<HashMap<usize, InputScroll>> = Default::default();
}

/// QQuickTextInputPrivate::updateHorizontalScroll: how far the text of TextInput `i` is moved
/// left for its caret to be seen in the `room` between its paddings. `lines` are laid out
/// without the scroll. The scroll starts from the one before, so it is kept by the item and
/// moved once for every change of the text, the caret or the room.
fn input_scroll(
    w: &mut World,
    i: usize,
    text: &str,
    lines: &[EditLine],
    left: f64,
    room: f64,
) -> f64 {
    let room = room.max(0.0);
    if !truthy(w, i, "autoScroll", true)
        || (w.items[i].prop_info("echoMode").is_some() && num(w, i, "echoMode") as i32 == 1)
    {
        INPUT_SCROLL.with_borrow_mut(|m| m.remove(&i));
        return 0.0;
    }
    let chars: Vec<char> = text.chars().collect();
    let cursor = qml::textpos::to_char(&chars, num(w, i, "cursorPosition").max(0.0) as usize);
    let Some(line) = lines
        .iter()
        .rev()
        .find(|l| l.start <= cursor)
        .or(lines.first())
    else {
        return 0.0;
    };
    let at = line.cursor_to_x((cursor - line.start).min(line.text.chars().count())) - left;
    let caret = if at >= 0.0 { at } else { room - at };
    let used = (line.line.text_width as f64 / 64.0).max(caret);
    let hash = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        text.hash(&mut h);
        h.finish()
    };
    let before = INPUT_SCROLL.with_borrow(|m| m.get(&i).copied());
    let mut now = InputScroll {
        text: hash,
        cursor,
        at,
        used,
        room,
        scroll: before.map_or(0.0, |b| b.scroll),
    };
    if before == Some(now) {
        return now.scroll;
    }
    let s = now.scroll;
    now.scroll = if used <= room {
        0.0
    } else if at - s >= room {
        // The caret is right of the field.
        at - room
    } else if at - s < 0.0 && s < used {
        // The caret is left of the field.
        at
    } else if used - s < room {
        // Text was removed: fill the room the scroll left on the right.
        used - room
    } else if room - s > used {
        room - used
    } else {
        s
    };
    INPUT_SCROLL.with_borrow_mut(|m| m.insert(i, now));
    now.scroll
}

/// QQuickTextUtil::alignedY of a TextInput/TextEdit as a share of the free height: 0 for
/// AlignTop, 1/2 for AlignVCenter, 1 for AlignBottom. Basic and Material TextField.qml set
/// AlignVCenter; TextInput and TextArea keep AlignTop.
fn edit_valign(w: &mut World, i: usize) -> f64 {
    let va = if explicit(w, i, "verticalAlignment") || !is(w, i, "TextField") {
        num(w, i, "verticalAlignment") as u32
    } else {
        128
    };
    match va {
        128 => 0.5,
        64 => 1.0,
        _ => 0.0,
    }
}

/// The laid-out lines of a plain-text editor's own text (none for rich text or no font).
fn edit_text_lines(
    w: &mut World,
    fonts: &mut Fonts,
    i: usize,
) -> Option<(Rc<FontStack>, Vec<EditLine>)> {
    let (ts, placeholder) = edit_style(w, fonts, i);
    if ts.spans.is_some() {
        return None;
    }
    let f = ts.font?;
    let text = if placeholder { "" } else { ts.text.as_str() };
    let (wd, ht) = (num(w, i, "width"), num(w, i, "height"));
    let lines = edit_layout(w, &f, i, text, false, wd, ht);
    Some((f, lines))
}

/// QQuickTextInput::positionAt / QQuickTextEdit::positionAt(x, y) in item coordinates: the line
/// under `y` (clamped to the first/last), then the nearest caret position on it. A TextEdit's
/// document has no line outside its text (QTextDocumentLayout::hitTest): a point above it is
/// the start of the text, one at its bottom edge or under it the end.
pub fn edit_position_at(
    w: &mut World,
    fonts: &mut Fonts,
    i: usize,
    px: f64,
    py: f64,
) -> Option<usize> {
    let (_, lines) = edit_text_lines(w, fonts, i)?;
    if is_text_doc(w, i) {
        let (first, last) = (lines.first()?, lines.last()?);
        if py < first.y {
            return Some(0);
        }
        if py >= last.y + last.h {
            return Some(last.start + last.text.chars().count());
        }
    }
    let el = lines.iter().find(|l| py < l.y + l.h).or(lines.last())?;
    Some(el.x_to_cursor(px))
}

/// QTextCursor::Up / Down from caret `pos`: the caret of the visual line above (below) nearest
/// item x `x`, the caret's own x when none is kept from an earlier vertical move, and that x.
/// None on the first (last) line.
pub fn edit_line_step(
    w: &mut World,
    fonts: &mut Fonts,
    i: usize,
    pos: usize,
    down: bool,
    x: Option<f64>,
) -> Option<(usize, f64)> {
    let (_, lines) = edit_text_lines(w, fonts, i)?;
    let k = lines
        .iter()
        .position(|l| (l.start..=l.start + l.text.chars().count()).contains(&pos))?;
    let x = x.unwrap_or_else(|| lines[k].cursor_to_x(pos - lines[k].start));
    let to = if down {
        lines.get(k + 1)?
    } else {
        lines.get(k.checked_sub(1)?)?
    };
    Some((to.x_to_cursor(x), x))
}

/// The visual (wrapped) line holding caret position `pos`: its first and end positions
/// (QTextCursor StartOfLine / EndOfLine; a soft break's consumed space stays after the end).
pub fn edit_visual_line(
    w: &mut World,
    fonts: &mut Fonts,
    i: usize,
    pos: usize,
) -> Option<(usize, usize)> {
    let (_, lines) = edit_text_lines(w, fonts, i)?;
    lines
        .iter()
        .map(|l| (l.start, l.start + l.text.chars().count()))
        .find(|&(a, b)| (a..=b).contains(&pos))
}

/// TextField/TextArea of the Material style with `Material.containerStyle` (the original's MaterialTextField):
/// QQuickMaterialTextContainer outline plus FloatingPlaceholderText.
/// Editor text colour; Material TextField/TextArea default to
/// `enabled ? Material.foreground : Material.hintTextColor`.
fn edit_color(w: &mut World, i: usize) -> [u8; 4] {
    let control = is(w, i, "TextField") || is(w, i, "TextArea");
    if !control || explicit(w, i, "color") || !w.items[i].props.contains_key("Material.foreground")
    {
        return color(w, i, "color", [0, 0, 0, 255]);
    }
    let fg = color(w, i, "Material.foreground", [0, 0, 0, 0xdd]);
    if truthy(w, i, "enabled", true) {
        return fg;
    }
    // Material hintTextColor: 38% black on light, 30% white on dark foregrounds.
    let light_fg = (fg[0] as u32 + fg[1] as u32 + fg[2] as u32) > 3 * 128;
    if light_fg {
        [255, 255, 255, 0x4d]
    } else {
        [0, 0, 0, 0x61]
    }
}

fn material_field(w: &World, i: usize) -> bool {
    w.items[i].props.contains_key("Material.containerStyle") && is_text_edit(w, i)
}

/// `Material.containerStyle: Material.Filled` (ContainerStyle::Filled = 0).
fn material_filled(w: &mut World, i: usize) -> bool {
    material_field(w, i) && num(w, i, "Material.containerStyle") as i32 == 0
}

/// A Control's own `background` item (MaterialTextArea's Rectangle) replacing the style's.
fn custom_background(w: &mut World, i: usize) -> Option<usize> {
    match w.read(i, "background") {
        Some(Value::Item(b)) if w.items[i].prop_info("background").is_some() => Some(b),
        _ => None,
    }
}

/// An editor's background painted below its text: QQuickControl gives a background at z 0 z -1.
fn edit_background(w: &mut World, i: usize) -> Option<usize> {
    if !is_text_edit(w, i) {
        return None;
    }
    custom_background(w, i).filter(|&b| num(w, b, "z") == 0.0)
}

thread_local! {
    /// FloatingPlaceholderText::largestHeight of Material TextAreas, set while measuring.
    static MAT_LH: std::cell::RefCell<HashMap<usize, f64>> = std::cell::RefCell::new(HashMap::default());
}

/// Material TextArea `topPadding`: below the floating placeholder when Filled with text or
/// focus, else centring a line in the implicit background height (plus topInset).
fn material_area_top(w: &mut World, i: usize) -> Option<f64> {
    if !is(w, i, "TextArea") || !material_field(w, i) || explicit(w, i, "topPadding") {
        return None;
    }
    let lh = MAT_LH.with(|m| m.borrow().get(&i).copied())?;
    let has_ph = matches!(w.read(i, "placeholderText"), Some(Value::Str(s)) if !s.is_empty());
    let has_text = w
        .read(i, "text")
        .and_then(|v| v.js_str().map(|s| !s.is_empty()))
        .unwrap_or(false);
    let up = has_ph && (has_text || truthy(w, i, "activeFocus", false));
    Some(if material_filled(w, i) && up {
        8.0 + lh
    } else {
        let bgh = custom_background(w, i).map_or(56.0, |b| num(w, b, "implicitHeight"));
        (bgh - lh) / 2.0 + num(w, i, "topInset")
    })
}

thread_local! {
    /// Floating-placeholder state per field: (target, focus, from, start).
    static MAT_ANIM: std::cell::RefCell<HashMap<usize, (bool, bool, f64, std::time::Instant)>> =
        std::cell::RefCell::new(HashMap::default());
}

/// Float progress of the placeholder/outline gap: 300 ms when focus changes, else immediate.
fn mat_progress(i: usize, target: bool, focus: bool) -> f64 {
    const DUR: f64 = 300.0;
    let now = std::time::Instant::now();
    let past = now
        .checked_sub(std::time::Duration::from_secs(1))
        .unwrap_or(now);
    let val = |e: &(bool, bool, f64, std::time::Instant)| {
        let t = (e.3.elapsed().as_secs_f64() * 1000.0 / DUR).min(1.0);
        e.2 + (e.0 as u8 as f64 - e.2) * t
    };
    MAT_ANIM.with(|m| {
        let mut m = m.borrow_mut();
        let e = m
            .entry(i)
            .or_insert((target, focus, target as u8 as f64, past));
        if e.0 != target {
            let cur = val(e);
            *e = if e.1 != focus {
                (target, focus, cur, now)
            } else {
                (target, focus, target as u8 as f64, past)
            };
        }
        e.1 = focus;
        if e.3.elapsed().as_secs_f64() * 1000.0 < DUR {
            ANIMATING.with(|a| a.set(true));
        }
        val(e)
    })
}

fn clipped(cv: &mut Target, r: [f64; 4], draw: &dyn Fn(&mut Target)) {
    match cv {
        Target::Cpu(_) => cpu_clipped(cv, r, draw),
        Target::Gpu(f) => {
            f.push_clip_f(r);
            draw(cv);
            if let Target::Gpu(f) = cv {
                f.pop_clip();
            }
        }
    }
}

/// Pixel runs of a `w`x`h` canvas outside the scissor rect `r` (x1, y1, x2, y2).
fn outside_runs(w: usize, h: usize, r: [i64; 4]) -> impl Iterator<Item = std::ops::Range<usize>> {
    let cl = |v: i64, hi: usize| v.clamp(0, hi as i64) as usize;
    let (x1, y1) = (cl(r[0], w), cl(r[1], h));
    let (x2, y2) = (cl(r[2], w).max(x1), cl(r[3], h).max(y1));
    let rows = (y1..y2).flat_map(move |y| [y * w..y * w + x1, y * w + x2..(y + 1) * w]);
    std::iter::once(0..y1 * w)
        .chain(rows)
        .chain(std::iter::once(y2 * w..h * w))
        .filter(|r| !r.is_empty())
}

/// What a scissored CPU draw must leave untouched: the pixels outside `r`, row runs copied
/// (instead of a whole-canvas snapshot restored pixel by pixel).
fn save_outside(c: &Canvas, r: [i64; 4]) -> Vec<[u8; 4]> {
    let mut out = Vec::with_capacity(outside_runs(c.w, c.h, r).map(|r| r.len()).sum());
    for run in outside_runs(c.w, c.h, r) {
        out.extend_from_slice(&c.px[run]);
    }
    out
}

fn restore_outside(c: &mut Canvas, r: [i64; 4], saved: &[[u8; 4]]) {
    let mut k = 0;
    for run in outside_runs(c.w, c.h, r) {
        let n = run.len();
        c.px[run].copy_from_slice(&saved[k..k + n]);
        k += n;
    }
}

/// A clip pushed by `clip_push`: on the CPU the scissor rect and the pixels outside it.
type Clipped = Option<([i64; 4], Vec<[u8; 4]>)>;

fn clip_push(cv: &mut Target, r: [f64; 4]) -> Clipped {
    match cv {
        Target::Cpu(c) => {
            let (x, y) = (r[0] as f32, r[1] as f32);
            let s = qsg::raster::scissor_rect(x, y, r[2].max(0.0), r[3].max(0.0), c.w, c.h);
            Some((s, save_outside(c, s)))
        }
        Target::Gpu(f) => {
            f.push_clip_f(r);
            None
        }
    }
}

fn clip_pop(cv: &mut Target, saved: Clipped) {
    match (cv, saved) {
        (Target::Cpu(c), Some((r, px))) => restore_outside(c, r, &px),
        (Target::Gpu(f), _) => f.pop_clip(),
        _ => {}
    }
}

/// QQuickTextArea::updatePaintNode and QQuickTextField::updatePaintNode: the text of a field
/// is clipped to the field less its paddings, and that of a TextArea to the Flickable less the
/// paddings where a ScrollView scrolls it; and whether the text is inside that anyway.
fn field_clip(w: &mut World, i: usize, at: [f64; 4]) -> Option<([f64; 4], bool)> {
    let area = qml::types::inherits(&w.items[i].types, "QQuickTextArea");
    if !area && !qml::types::inherits(&w.items[i].types, "QQuickTextField") {
        return None;
    }
    let [x, y, wd, ht] = at;
    let flick = area_flickable(w, i).filter(|&f| area && scroll_text_area(w, f) == Some(i));
    let [cx, cy, cw, ch] = match flick {
        Some(f) => [
            num(w, f, "contentX"),
            num(w, f, "contentY"),
            num(w, f, "width"),
            num(w, f, "height"),
        ],
        // QQuickTextEdit::clipRect and QQuickTextInput::clipRect: the cursor has room past
        // the right edge, 1 wide, and in a TextArea 3 more past text.
        None => {
            let text = w
                .read(i, "text")
                .is_some_and(|v| !v.to_js_string().is_empty());
            let extra = if area && text { 4.0 } else { 1.0 };
            [0.0, 0.0, wd + extra, ht]
        }
    };
    let wraps = num(w, i, "wrapMode") != 0.0;
    let fits = cx == 0.0
        && cy == 0.0
        && (wraps || num(w, i, "implicitWidth") <= cw)
        && num(w, i, "implicitHeight") <= ch;
    let [l, t, r, b] = edit_padding(w, i);
    // QQuickTextArea: one more pixel for text aligned to the right.
    let r = if area && num(w, i, "horizontalAlignment") == 2.0 {
        r - 1.0
    } else {
        r
    };
    Some(([x + cx + l, y + cy + t, cw - l - r, ch - t - b], fits))
}

/// QQuickMaterialTextContainer::paint (outlined) and the FloatingPlaceholderText above it.
#[allow(clippy::too_many_arguments)]
fn paint_material_frame(
    w: &mut World,
    fonts: &mut Fonts,
    i: usize,
    cv: &mut Target,
    x: f64,
    y: f64,
    wd: f64,
    ht: f64,
    op: f32,
) {
    const HPAD: f64 = 16.0;
    let ph = match w.read(i, "placeholderText") {
        Some(Value::Str(s)) => s.to_string(),
        _ => String::new(),
    };
    let has_text = w
        .read(i, "text")
        .and_then(|v| v.js_str().map(|s| !s.is_empty()))
        .unwrap_or(false);
    let focus = truthy(w, i, "activeFocus", false);
    let p = mat_progress(i, !ph.is_empty() && (focus || has_text), focus);
    let top = num(w, i, "topInset");
    // Material.Dark, or Material.System under a dark QStyleHints::colorScheme.
    let dark = match num(w, i, "Material.theme") as i32 {
        1 => true,
        2 => qml::colorscheme::system_dark(),
        _ => false,
    };
    let pen = if focus { 2.0 } else { 1.0 };
    let col = if focus {
        color(w, i, "Material.accent", [0xE9, 0x1E, 0x63, 255])
    } else if truthy(w, i, "hovered", false) {
        if dark {
            [255, 255, 255, 255]
        } else {
            [0, 0, 0, 0xDD]
        }
    } else if dark {
        [255, 255, 255, 0x4C]
    } else {
        [0, 0, 0, 0x60]
    };
    let [l, _, r, _] = edit_padding(w, i);
    let base = text_style(w, fonts, i).font;
    let (lh, pw) = base.as_ref().map_or((0.0, 0.0), |f| {
        let line = f.shape(&ph);
        (line.height(), line.advance as f64 / 64.0)
    });
    let scale = 1.0 - 0.2 * p;
    let half = pw.min(wd - l - r) * scale / 2.0;
    // QQuickPaintedItem paints into a whole-pixel texture: the container height truncates.
    let (fy, fh) = (y + top, (ht - top).floor());
    let g = qsg::rect::build(&qsg::rect::RectParams {
        width: wd as f32,
        height: fh as f32,
        color: [0, 0, 0, 0],
        border_color: col,
        pen_width: pen as f32,
        radius: (4.0 + pen / 2.0) as f32,
        antialiasing: true,
        ..Default::default()
    });
    let g = Rc::new(g);
    let draw = |cv: &mut Target| match cv {
        Target::Cpu(c) => cpu_smooth(c, &g, x, fy, op),
        Target::Gpu(fr) => fr.smooth(&g, x, fy, op),
    };
    let custom = custom_background(w, i);
    if custom.is_none() {
        if p > 0.0 && !ph.is_empty() {
            let gc = x + pen / 2.0 + HPAD + half;
            let (a, b) = (gc - p * half - 4.0 - pen, gc + p * half + 4.0);
            clipped(cv, [x - 1.0, fy - 1.0, a - x + 1.0, fh + 2.0], &draw);
            clipped(cv, [b, fy - 1.0, x + wd + 1.0 - b, fh + 2.0], &draw);
            clipped(cv, [a, fy + pen, b - a, fh], &draw);
        } else {
            draw(cv);
        }
    }
    if ph.is_empty() {
        return;
    }
    let Some(f) = text_style_scaled(w, fonts, i, scale).font else {
        return;
    };
    let e = (p * std::f64::consts::FRAC_PI_2).sin();
    // QQuickMaterialPlaceholderText normalTargetY/floatingTargetY.
    let (rest, float) = if material_filled(w, i) {
        let bgh = custom.map_or(56.0, |b| num(w, b, "implicitHeight"));
        let rest = if is(w, i, "TextArea") && ht >= bgh * 2.0 {
            8.0
        } else {
            (bgh - lh) / 2.0
        };
        (rest, 8.0)
    } else {
        ((ht - lh) / 2.0, top - lh / 2.0)
    };
    let py = rest + (float - rest) * e;
    let line = f.shape(&ph);
    let ty = py + lh / 2.0 - line.height() / 2.0;
    let pc = color(w, i, "placeholderTextColor", [127, 127, 127, 255]);
    let px = l + (HPAD - l) * e;
    // QtRendering FloatingPlaceholderText: glyphs at the full font size through its `scale`
    // (QQuickMaterialPlaceholderText transformOrigin Left).
    if qt_rendering(w, i)
        && let (Target::Gpu(fr), Some(bf)) = (&mut *cv, &base)
    {
        let size = bf.font(0).map_or(0, |x| x.pixel_size) as f64;
        let gray = df_node_recycled(usize::MAX - i, &ph, pc, size, wd, ht);
        let line = bf.shape(&ph);
        let [sx0, sy0, ox0, oy0] = fr.xf.unwrap_or([1.0, 1.0, 0.0, 0.0]);
        let (sx0, sy0) = (sx0 as f64, sy0 as f64);
        let (gx, gy) = (x + px, y + py + lh / 2.0);
        let prev = fr.xf.replace([
            (sx0 * scale) as f32,
            (sy0 * scale) as f32,
            (sx0 * gx * (1.0 - scale)) as f32 + ox0,
            (sy0 * gy * (1.0 - scale)) as f32 + oy0,
        ]);
        rt_line(
            &mut Target::Gpu(fr),
            bf,
            &line,
            x,
            y,
            px,
            py,
            pc,
            op,
            Some(DfText {
                gray,
                ..Default::default()
            }),
        );
        fr.xf = prev;
        return;
    }
    edit_line(cv, &f, &line, [x, y], [px, ty], pc, op);
}

/// The plain-text editor lines as painted: (first char index, top, height, caret x of every
/// position), in item coordinates.
pub fn edit_geometry(
    w: &mut World,
    fonts: &mut Fonts,
    i: usize,
) -> Vec<(usize, f64, f64, Vec<f64>)> {
    let Some((_, lines)) = edit_text_lines(w, fonts, i) else {
        return Vec::new();
    };
    lines
        .into_iter()
        .map(|l| {
            let xs = (0..=l.text.chars().count())
                .map(|c| l.cursor_to_x(c))
                .collect();
            (l.start, l.y, l.h, xs)
        })
        .collect()
}

/// QQuickTextInput/QQuickTextEdit::positionToRectangle(pos): the text cursor at UTF-16 position
/// `pos` of editor `i`, `[x, y, 1, line height]` in item coordinates.
pub fn edit_cursor_rect(
    w: &mut World,
    fonts: &mut Fonts,
    i: usize,
    pos: usize,
) -> Option<[f64; 4]> {
    let pos = edit_char(w, i, pos);
    let geo = edit_geometry(w, fonts, i);
    let (start, y, h, xs) = geo
        .iter()
        .rev()
        .find(|(start, ..)| *start <= pos)
        .or(geo.first())?;
    let x = xs
        .get(pos - start.min(&pos))
        .or(xs.last())
        .copied()
        .unwrap_or(0.0);
    Some([x, *y, 1.0, *h])
}

/// What `edit_cursor_sync` keeps of a TextEdit between calls: the size it last saw.
const CURSOR_SIZE: [&str; 2] = ["\x01curw", "\x01curh"];
/// Set on an editor once `edit_cursor_sync` found it sized: by a layout, or by itself.
const CURSOR_SIZED: &str = "\x01cursized";
/// Set on an editor whose cursor item `edit_cursor_sync` moved since the last layout.
const CURSOR_MOVED: &str = "\x01curmoved";

/// QQuickTextInput::updateCursorRectangle / QQuickTextEdit::moveCursorDelegate: editor `i`'s
/// `cursorRectangle` as its text, cursor and geometry are now. When it changed, the cursor
/// item (`World::cursor_items`) goes to its position and takes its height; a TextEdit also
/// moves its cursor item when its own size changed (its updateSize), a TextInput does not.
///
/// Qt does this in the setter, after the editor took its new size. Here a cursor or text
/// write is followed at once (`laid` false: a read of the cursor item, or settle), and the
/// sizes come with the layout: the layout's call (`laid`) places the item again, over what
/// the item's bindings on those sizes gave in between.
pub fn edit_cursor_sync(w: &mut World, fonts: &mut Fonts, i: usize, laid: bool) {
    // Qt sizes an editor when its component completes: a read before the first layout (in
    // `Component.onCompleted`) finds the text placed in the editor's implicit size.
    if !laid && w.behaviors_live && w.items[i].props.get(CURSOR_SIZED).is_none() {
        measure_prof(w, fonts, i);
    }
    if w.behaviors_live && w.items[i].props.get(CURSOR_SIZED).is_none() {
        set(w, i, CURSOR_SIZED, 1.0);
    }
    let pos = num(w, i, "cursorPosition").max(0.0) as usize;
    let Some(r) = edit_cursor_rect(w, fonts, i, pos) else {
        return;
    };
    let old = match w.items[i].props.get("cursorRectangle").map(|p| &p.value) {
        Some(Value::Obj(o)) => {
            let o = o.borrow();
            Some(["x", "y", "width", "height"].map(|k| o.props.get(k).map(Value::to_number)))
        }
        _ => None,
    };
    let mut resized = false;
    if is_text_doc(w, i) {
        for (k, v) in CURSOR_SIZE
            .into_iter()
            .zip([num(w, i, "width"), num(w, i, "height")])
        {
            let seen = w.items[i].props.get(k).map(|p| p.value.to_number());
            if seen != Some(v) && (laid || seen.is_some()) {
                resized = seen.is_some();
                set(w, i, k, v);
            }
        }
    }
    let changed = old != Some(r.map(Some));
    if changed {
        w.write(i, "cursorRectangle", qml::inputmethod::rect_value(r));
    }
    let moved = num(w, i, CURSOR_MOVED) != 0.0;
    if laid && moved {
        set(w, i, CURSOR_MOVED, 0.0);
    } else if !laid && changed && !moved && old.is_some() {
        set(w, i, CURSOR_MOVED, 1.0);
    }
    if (changed || resized || (laid && moved))
        && let Some(&(_, item)) = w.cursor_items.get(&i)
        && !w.dead.contains(&item)
    {
        for (k, v) in [("x", r[0]), ("y", r[1]), ("height", r[3])] {
            // The item was made before the rectangle was first known: at its creation Qt sets
            // these before the item's own bindings run, which then stand.
            let bound = w.items[item]
                .props
                .get(k)
                .is_some_and(|p| p.binding.is_some());
            if old.is_some() || !bound {
                set(w, item, k, v);
            }
        }
    }
}

/// `edit_cursor_sync` for every editor whose cursor rectangle something follows.
fn edit_cursors_sync(w: &mut World, fonts: &mut Fonts) {
    let mut tracked: Vec<usize> = w.cursor_tracked.iter().copied().collect();
    tracked.sort_unstable();
    for i in tracked {
        if !w.dead.contains(&i) {
            edit_cursor_sync(w, fonts, i, true);
        }
    }
}

fn edit_padding(w: &mut World, i: usize) -> [f64; 4] {
    ["left", "top", "right", "bottom"].map(|k| pad_side(w, i, k))
}

/// QQuickText `maximumLineCount`.
fn line_limit(w: &mut World, i: usize) -> Option<usize> {
    let n = opt_num(w, i, "maximumLineCount")?;
    (n.is_finite() && (1.0..1e9).contains(&n)).then_some(n as usize)
}

/// QQuickItem::widthValid() and heightValid(): the size is set, or anchors or a layout own it.
fn size_valid(w: &mut World, i: usize) -> (bool, bool) {
    let (mw, mh) = size_managed(w, i);
    (
        mw || explicit(w, i, "width"),
        mh || explicit(w, i, "height"),
    )
}

/// What bounds a Text's lines, from its `wrapMode`, `elide`, `maximumLineCount` and size.
/// `natural` is given when measuring: the width of the unwrapped lines, which no narrower item
/// wraps. The width of an item that nothing sizes is then its implicit width as last measured,
/// which elides nothing. `doc`: the text is a QTextDocument (RichText, MarkdownText).
fn text_bounds(w: &mut World, i: usize, doc: bool, natural: Option<f64>) -> Bounds {
    let [pl, pt, pr, pb] = text_pad(w, i);
    let (width, height) = (num(w, i, "width") - pl - pr, num(w, i, "height") - pt - pb);
    let (width_valid, height_valid) = size_valid(w, i);
    let wrap = match text_wrap_width(w, i) {
        Some(ww) if natural.is_none_or(|iw| ww < iw.ceil()) => {
            Wrap::from_qml(num(w, i, "wrapMode") as i32)
        }
        _ => Wrap::None,
    };
    // Text.ElideLeft 0, ElideRight 1, ElideMiddle 2, ElideNone 3.
    let elide = opt_num(w, i, "elide")
        .map(|e| e as u32)
        .filter(|&e| e != 3 && (width_valid || natural.is_none()));
    // A QTextDocument (`doc`) is neither elided nor cut off.
    let elide = elide.filter(|_| !doc);
    let max_lines = line_limit(w, i).filter(|_| !doc);
    let multiline_elide = elide == Some(1) && (height_valid || max_lines.is_some());
    // Without room for a line Qt lays out none, or (`text_lines_kept`) the text in full: then
    // a width that is not above 0 bounds no line (QQuickTextPrivate::setupTextLayout's
    // lineWidth).
    let no_room = text_no_room(w, i);
    Bounds {
        wrap,
        width: if no_room && width <= 0.0 {
            f64::INFINITY
        } else {
            width
        },
        elide,
        multiline_elide,
        height: if height_valid { height } else { f64::INFINITY },
        max_lines,
        hidden: no_room && num(w, i, TEXT_ROOM[2]) != 2.0,
    }
}

fn text_wrap_width(w: &mut World, i: usize) -> Option<f64> {
    let [l, _, r, _] = text_pad(w, i);
    let wd = num(w, i, "width") - l - r;
    (num(w, i, "wrapMode") != 0.0 && wd > 0.0).then_some(wd)
}

/// The width a TextInput/TextEdit wraps its text at: its own less its left and right padding
/// (`pad`, of `edit_padding`), once.
fn edit_wrap_width(w: &mut World, i: usize, pad: f64) -> Option<f64> {
    let wd = num(w, i, "width") - pad;
    (num(w, i, "wrapMode") != 0.0 && wd > 0.0).then_some(wd)
}

/// QQuickText `leftPadding`/`topPadding`/`rightPadding`/`bottomPadding`, falling back to `padding`.
fn text_pad(w: &mut World, i: usize) -> [f64; 4] {
    if !w.items[i].props.contains_key("padding")
        && !["leftPadding", "topPadding", "rightPadding", "bottomPadding"]
            .iter()
            .any(|k| w.items[i].props.contains_key(k))
    {
        return [0.0; 4];
    }
    let p = num(w, i, "padding");
    ["left", "top", "right", "bottom"].map(|side| {
        let k = format!("{side}Padding");
        if explicit(w, i, &k) { num(w, i, &k) } else { p }
    })
}

/// A text item's first-line ascent and the height of its laid-out lines, kept by `measure`.
const TEXT_BASE: [&str; 2] = ["\x01basc", "\x01bth"];

fn keep_text_base(w: &mut World, i: usize, ascent: i64, th: f64) {
    set(w, i, TEXT_BASE[0], ascent as f64 / 64.0);
    set(w, i, TEXT_BASE[1], th);
}

/// QQuickTextPrivate::updateBaseline / QQuickTextInputPrivate::updateBaselineOffset: topPadding
/// plus the vertical alignment offset (as painted) plus the first line's ascent.
fn text_baseline(w: &mut World, i: usize) -> f64 {
    let (asc, th) = (num(w, i, TEXT_BASE[0]), num(w, i, TEXT_BASE[1]));
    let text = is_text(w, i);
    let (pt, pb) = if text {
        let [_, t, _, b] = text_pad(w, i);
        (t, b)
    } else {
        let [_, t, _, b] = edit_padding(w, i);
        (t, b)
    };
    // Only a set height moves the baseline (`if (q->heightValid())`): in an implicit one the
    // baseline is the first line's, whatever the alignment.
    let free = match explicit(w, i, "height") {
        true => num(w, i, "height") - pt - pb - th,
        false => 0.0,
    };
    let dy = match (text, num(w, i, "verticalAlignment") as u32) {
        (true, 128) => free / 2.0,
        (true, 64) => free,
        (true, _) => 0.0,
        // QQuickTextInputPrivate::updateBaselineOffset: only a height that was set is
        // aligned in.
        (false, _) if explicit(w, i, "height") || size_managed(w, i).1 => edit_valign(w, i) * free,
        (false, _) => 0.0,
    };
    pt + dy + asc
}

/// QQuickItem::baselineOffset: an assigned value, else the text's first baseline for
/// Text/TextInput/TextEdit (and Label, TextField, TextArea), else for a Control
/// `topPadding + contentItem.baselineOffset` wherever the contentItem sits
/// (QQuickControlPrivate::updateBaselineOffset), else 0. Keeps the property current for
/// bindings and returns it.
fn baseline_offset(w: &mut World, i: usize) -> f64 {
    if baseline_set(w, i) {
        return num(w, i, "baselineOffset");
    }
    let v = if is_text(w, i) || is_text_edit(w, i) {
        text_baseline(w, i)
    } else if w.items[i].prop_info("background").is_some()
        && let Some(Value::Item(ci)) = w.read(i, "contentItem")
        && ci != i
    {
        pad_side(w, i, "top") + baseline_offset(w, ci)
    } else if let Some(v) = style_baseline(w, i) {
        v
    } else {
        return num(w, i, "baselineOffset");
    };
    // A QQuickPopup is not an item and has no baseline to read.
    if !qml::types::is_popup(&w.items[i].types) {
        set(w, i, "baselineOffset", v);
        if w.items[i].props.contains_key(BASE_PUT) || explicit(w, i, "baselineOffset") {
            set(w, i, BASE_PUT, v);
        }
    }
    v
}

/// The baseline `baseline_offset` put into a `baselineOffset` that has a binding.
const BASE_PUT: &str = "\x01bput";

/// Whether `baselineOffset` is a value that was given to it (QQuickControl's
/// `hasBaselineOffset`) and not the one the item works out. A binding that gave `undefined`
/// reset it (QQuickControl::resetBaselineOffset): the property is then no longer an assigned
/// one and holds the 0 of the reset or the baseline put there since; another value is what
/// the binding gave after that.
fn baseline_set(w: &mut World, i: usize) -> bool {
    let (bound, alias) = match w.items[i].props.get("baselineOffset") {
        Some(p) => (p.binding.is_some(), p.alias.is_some()),
        None => (false, false),
    };
    if w.is_assigned(i, "baselineOffset") || alias {
        return true;
    }
    if !bound {
        return false;
    }
    let v = num(w, i, "baselineOffset");
    v != 0.0 && opt_num(w, i, BASE_PUT) != Some(v)
}

/// QQuickControlPrivate::resizeContent puts the contentItem at the paddings, unless it fills
/// the control by anchors (or is a ScrollBar's, placed along the bar).
fn places_content(w: &mut World, i: usize, ci: usize) -> bool {
    !is(w, i, "ScrollBar") && !matches!(w.read(ci, "anchors.fill"), Some(Value::Item(_)))
}

/// QQuickTextArea::itemChange: a TextArea whose parent is the Flickable of a ScrollView
/// attaches to that Flickable (QQuickTextAreaPrivate::attachFlickable). The TextArea of the
/// Flickable `f`, if `f` is one of a ScrollView.
fn scroll_text_area(w: &mut World, f: usize) -> Option<usize> {
    scroll_view_of(w, f)?;
    let content = flick_content(w, f);
    kids(w, content)
        .into_iter()
        .find(|&c| qml::types::inherits(&w.items[c].types, "QQuickTextArea"))
}

/// The contentItem of Flickable `f`, which holds what scrolls.
fn flick_content(w: &World, f: usize) -> usize {
    w.content_item_of.get(&f).copied().unwrap_or(f)
}

/// The Flickable in whose contentItem `a` is.
fn area_flickable(w: &World, a: usize) -> Option<usize> {
    w.flickable_of(w.items[a].parent?)
}

/// QQuickFlickable::setContentWidth / setContentHeight called by the ScrollView of Flickable
/// `f`: the size is set on the Flickable from then on, as one written in QML is.
fn give_content(w: &mut World, f: usize, size: &str, v: f64) {
    // Assigned first: the contentItem takes the size as the content size when it is stored.
    w.assign((f, qml::runtime::Sym::new(size)));
    set(w, f, size, v);
}

/// What `follow_cursors` keeps on the TextArea of a ScrollView: a hash of what its cursor
/// rectangle is made from, and the rectangle.
const CURSOR_FROM: &str = "\x01cursorFrom";
const CURSOR_AT: [&str; 4] = ["\x01cursorX", "\x01cursorY", "\x01cursorW", "\x01cursorH"];

/// QQuickTextEdit::positionToRectangle of char `pos` in item coordinates, from `edit_geometry`.
fn position_rect(geo: &[(usize, f64, f64, Vec<f64>)], pos: usize) -> Option<[f64; 4]> {
    let (start, y, h, xs) = geo
        .iter()
        .rev()
        .find(|(start, ..)| *start <= pos)
        .or(geo.first())?;
    let x = xs.get(pos.saturating_sub(*start)).or(xs.last())?;
    Some([*x, *y, 1.0, *h])
}

/// QQuickTextControlPrivate::cursorMoveKeyEvent: a key that moves the cursor updates its
/// rectangle (`updateCursorRectangle(true)`) where the cursor already is too, so the view of
/// TextArea `a` comes back to it.
pub fn cursor_key(w: &mut World, a: usize) {
    if w.items[a].props.contains_key(CURSOR_FROM) {
        set(w, a, CURSOR_FROM, f64::NAN);
        set(w, a, CURSOR_AT[0], f64::NAN);
    }
}

/// QQuickTextAreaPrivate::ensureCursorVisible for the TextAreas of the ScrollViews in window
/// `win`: when the cursor rectangle of one changes, its Flickable moves so that the cursor
/// and the padding next to it are in view. True when a Flickable was moved.
pub fn follow_cursors(w: &mut World, fonts: &mut Fonts, win: usize) -> bool {
    let mut moved = false;
    for a in w.of_type("TextArea") {
        let Some(f) = area_flickable(w, a) else {
            continue;
        };
        if scroll_text_area(w, f) != Some(a) || !crate::input::is_ancestor(w, win, a) {
            continue;
        }
        let text = match w.read(a, "text") {
            Some(Value::Str(s)) => s,
            _ => "".into(),
        };
        let at = num(w, a, "cursorPosition");
        let from = text_key(&(
            &*text,
            at.to_bits(),
            num(w, a, "width").to_bits(),
            num(w, a, "height").to_bits(),
        ));
        if opt_num(w, a, CURSOR_FROM) == Some(from) {
            continue;
        }
        let first = !w.items[a].props.contains_key(CURSOR_FROM);
        set(w, a, CURSOR_FROM, from);
        let pos = edit_char(w, a, at as usize);
        let geo = edit_geometry(w, fonts, a);
        let Some(cr) = position_rect(&geo, pos) else {
            continue;
        };
        let was = CURSOR_AT.map(|k| opt_num(w, a, k));
        for (k, v) in CURSOR_AT.into_iter().zip(cr) {
            set(w, a, k, v);
        }
        // The rectangle the cursor has when the TextArea is made is not a change of it.
        if first || was == cr.map(Some) {
            continue;
        }
        let [lp, tp, rp, bp] = edit_padding(w, a);
        let (cx, cy) = (num(w, f, "contentX"), num(w, f, "contentY"));
        let (fw, fh) = (num(w, f, "width"), num(w, f, "height"));
        let (left, top, right, bottom) = (cr[0], cr[1], cr[0] + cr[2], cr[1] + cr[3]);
        let x = if left <= cx + lp {
            Some(left - lp)
        } else {
            let next = (pos < text.chars().count())
                .then(|| position_rect(&geo, pos + 1))
                .flatten()
                .filter(|nr| nr[1] == top)
                .map(|nr| nr[0] + nr[2]);
            match next {
                Some(nr) if nr >= cx + lp + fw - rp => Some(nr - fw + rp),
                _ if right >= cx + lp + fw - rp => Some(right - fw + rp),
                _ => None,
            }
        };
        let y = if top <= cy + tp {
            Some(top - tp)
        } else if bottom >= cy + tp + fh - bp && bottom <= num(w, f, "contentHeight") {
            Some(bottom - fh + bp)
        } else {
            None
        };
        for (k, v, cur) in [("contentX", x, cx), ("contentY", y, cy)] {
            if let Some(v) = v
                && v != cur
            {
                w.write(f, k, Value::Num(v));
                moved = true;
            }
        }
    }
    moved
}

/// The ScrollView whose Flickable `f` is.
fn scroll_view_of(w: &mut World, f: usize) -> Option<usize> {
    let p = w.items[f].parent?;
    (scroll_flickable(w, p) == Some(f)).then_some(p)
}

/// What `pane_content` keeps of a pane between layouts: 1 once it has seen the pane, plus 2
/// (width) and 4 (height) for a ScrollView whose own Flickable holds that content size.
const PANE_SEEN: &str = "\x01pane";
/// What `pane_content` keeps of a ScrollView with a TextArea, per axis: the implicit size of
/// the TextArea it gave the Flickable, and the size the TextArea gets from a size of the
/// ScrollView that was given to the Flickable, while a binding may still answer it.
const AREA_GAVE: [&str; 2] = ["\x01areaGaveW", "\x01areaGaveH"];
const AREA_ECHO: [&str; 2] = ["\x01areaEchoW", "\x01areaEchoH"];
/// Set on the TextArea of a ScrollView once its Flickable has sized it.
const AREA_SIZED_BY_VIEW: &str = "\x01areaSized";
/// The size of the Flickable a TextArea is attached to, as the TextArea last saw it.
const AREA_VIEW: [&str; 2] = ["\x01areaViewW", "\x01areaViewH"];
/// 1 on a TextArea that was made as wide as its Flickable to break its lines, until the
/// Flickable has the size that gives it and places it again.
const AREA_REFIT: &str = "\x01areaRefit";
/// The content size a ScrollView last gave the Flickable it wraps.
const PANE_GAVE: [&str; 2] = ["\x01panew", "\x01paneh"];

/// The content size of a QQuickPane (Pane, Frame, GroupBox, Page, ScrollView), which is its
/// `implicitContentWidth`/`Height`, as QQuickPanePrivate::getContentWidth/Height give it:
/// nothing while the pane has no contentItem (Qt makes one for the first object declared in
/// the pane); the `contentWidth` that was set; else the contentItem's implicit size; else,
/// where that is 0, the implicit size of the only child. A ScrollView has the content size of
/// the Flickable it wraps instead, and gives it the one set on itself.
///
/// QQuickPanePrivate::updateContentWidth/Height: `contentWidth` takes this size when the size
/// changes and `contentWidth` is not a set one. So an empty Pane keeps the `contentWidth` it
/// had before a reset, and a ScrollView the -1 it starts with while its child has no implicit
/// size.
fn pane_content(w: &mut World, i: usize) -> [f64; 2] {
    const SIZE: [(&str, &str, &str); 2] = [
        ("contentWidth", "implicitContentWidth", "implicitWidth"),
        ("contentHeight", "implicitContentHeight", "implicitHeight"),
    ];
    let fin = |v: f64| if v.is_finite() { v } else { 0.0 };
    let scroll = is(w, i, "ScrollView");
    // The Flickable a ScrollView made holds what was declared in the ScrollView.
    let made = scroll_flickable(w, i).filter(|&f| w.items[f].props.contains_key(SCROLL_MADE));
    let own = match w.read(i, "contentItem") {
        Some(Value::Item(c)) if c != i && !scroll => Some(c),
        _ => None,
    };
    let from = made.map_or(i, |f| flick_content(w, f));
    // The objects declared in the pane, not the ones its properties hold (background, a
    // ScrollBar): Qt appends them to the contentItem.
    let declared =
        |w: &World, c: usize| !w.dead.contains(&c) && !w.items[c].props.contains_key("\x01propval");
    let any = w.items[from]
        .children
        .clone()
        .into_iter()
        .any(|c| declared(w, c));
    let mut items: Vec<usize> = own.map_or(vec![], |c| kids(w, c));
    items.extend(kids(w, from).into_iter().filter(|&c| declared(w, c)));
    let seen = opt_num(w, i, PANE_SEEN).unwrap_or(0.0) as u8;
    let mut now = seen | 1;
    // QQuickScrollViewPrivate::getContentWidth: the content size of a Flickable that has one
    // of its own, which is one that was declared, or one a TextArea is attached to
    // (QQuickTextAreaPrivate::resizeFlickableContent gives it the TextArea's implicit size).
    let area = made.and_then(|f| scroll_text_area(w, f));
    if let (Some(f), Some(a)) = (made, area)
        && opt_num(w, a, AREA_REFIT) == Some(1.0)
    {
        set(w, a, AREA_REFIT, 0.0);
        w.memo_invalidate(f);
    }
    let flick = match (made, area) {
        (Some(_), None) => None,
        _ => scroll_flickable(w, i),
    };
    let mut out = [0.0; 2];
    for (o, (size, implicit, of)) in SIZE.into_iter().enumerate() {
        let has = w.is_assigned(i, size);
        let cur = num(w, i, size);
        // QQuickScrollView::componentComplete makes the Flickable of a ScrollView nothing
        // was declared in, and takes the Flickable's content size (none: -1) from then on.
        if seen == 0 && scroll && !any && own.is_none() && !has {
            now |= 2 << o;
        }
        let v = if let Some(f) = flick {
            // QQuickScrollView::contentSizeChange: a size set on the ScrollView goes to the
            // Flickable when it changes.
            let gave = opt_num(w, i, PANE_GAVE[o]);
            // QQuickTextAreaPrivate::attachFlickable: the TextArea is given the size of the
            // Flickable first (resizeFlickableControl), so one that wraps has its lines
            // broken in the view before the Flickable takes its implicit size.
            if let Some(a) = area
                && !w.items[a].props.contains_key(AREA_SIZED_BY_VIEW)
            {
                code_view_made(w, i, a);
            } else if let Some(a) = area {
                code_view_made(w, i, a);
                // QQuickTextAreaPrivate::resizeFlickableContent: the Flickable takes the
                // implicit size of its TextArea whenever that changes, and the TextArea is
                // then resized to it (`place_inner`).
                let v = fin(num(w, a, of));
                let side = ["width", "height"][o];
                let at = num(w, a, side);
                if opt_num(w, i, AREA_GAVE[o]) != Some(v) {
                    // The size the ScrollView had before the TextArea was complete went
                    // to the Flickable first.
                    if has && opt_num(w, i, AREA_GAVE[o]).is_none() {
                        set(w, i, PANE_GAVE[o], cur);
                    }
                    set(w, i, AREA_GAVE[o], v);
                    give_content(w, f, size, v);
                    set(w, i, AREA_ECHO[o], f64::NAN);
                } else if has && gave != Some(cur) {
                    if opt_num(w, i, AREA_ECHO[o]) == Some(at)
                        && let Some(g) = gave
                    {
                        // The size of the ScrollView is bound to the one of the TextArea,
                        // which the last size given to the Flickable changed: Qt does not
                        // run a binding again inside its own update (a binding loop).
                        set(w, i, AREA_ECHO[o], f64::NAN);
                        set(w, i, size, g);
                    } else {
                        set(w, i, PANE_GAVE[o], cur);
                        give_content(w, f, size, cur);
                        let view = num(w, f, side);
                        let to = if o == 0 && num(w, a, "wrapMode") != 0.0 {
                            view
                        } else {
                            view.max(cur)
                        };
                        set(w, i, AREA_ECHO[o], if to != at { to } else { f64::NAN });
                    }
                }
            } else if has && gave != Some(cur) {
                set(w, i, PANE_GAVE[o], cur);
                give_content(w, f, size, cur);
            }
            match opt_num(w, f, size) {
                Some(v) if v.is_finite() => v,
                _ => -1.0,
            }
        } else if now & (2 << o) != 0 {
            cur
        } else if !(scroll || any || own.is_some()) {
            0.0
        } else if has {
            cur
        } else {
            match (own.map_or(0.0, |c| fin(num(w, c, of))), &items[..]) {
                (v, _) if v != 0.0 => v,
                (_, &[only]) => fin(num(w, only, of)),
                _ => 0.0,
            }
        };
        let mut old = opt_num(w, i, implicit).unwrap_or(0.0);
        // Qt appends the declared children one by one: while the first one was the only one,
        // the size was its implicit size, if that is a literal (a binding has no value yet).
        // A Repeater's delegates come after all of them.
        if seen == 0
            && scroll
            && flick.is_none()
            && made.is_some()
            && items.len() > 1
            && let Some(first) = items.iter().copied().find(|c| !w.inst_roots.contains(c))
            && w.items[first]
                .props
                .contains_key(qml::runtime::LITERAL_IMPLICIT[o])
        {
            old = fin(num(w, first, of));
        }
        if v != old && !has && cur != v {
            set(w, i, size, v);
        }
        // QQuickScrollView::contentSizeChange: the Flickable the ScrollView made has the
        // content size of the ScrollView.
        if let Some(f) = made
            && flick.is_none()
        {
            let cur = num(w, i, size);
            if opt_num(w, f, size) != Some(cur) {
                give_content(w, f, size, cur);
            }
        }
        out[o] = v;
    }
    if now != seen {
        set(w, i, PANE_SEEN, now as f64);
    }
    out
}

/// QQuickPopupPositioner::reposition with allowVerticalFlip (QQuickComboBox sets it on its
/// popup): a popup leaving the window (less non-negative margins) vertically is mirrored around
/// its parent when the mirrored rect fits, else moved inside. Returns the popup's scene y shift.
pub fn popup_flip_dy(w: &mut World, c: usize, win: usize) -> f64 {
    let dy = flip_dy(w, c, win);
    let old = if dy == 0.0 {
        w.popup_dy.remove(&c)
    } else {
        w.popup_dy.insert(c, dy)
    };
    // `World::scene_pos` (a PopupWindow anchored inside this popup) moved without a write.
    if old.unwrap_or(0.0) != dy {
        w.revision += 1;
    }
    dy
}

fn flip_dy(w: &mut World, c: usize, win: usize) -> f64 {
    let Some(p) = w.items[c].parent else {
        return 0.0;
    };
    if !is(w, p, "ComboBox") || is(w, c, "ToolTip") {
        return 0.0;
    }
    let wh = num(w, win, "height");
    if wh <= 0.0 {
        return 0.0;
    }
    let m = opt_num(w, c, "margins").unwrap_or(-1.0);
    let top = opt_num(w, c, "topMargin").unwrap_or(m).max(0.0);
    let bottom = wh - opt_num(w, c, "bottomMargin").unwrap_or(m).max(0.0);
    let (_, py) = w.scene_pos(p, Some(win));
    let (y, h, ph) = (num(w, c, "y"), num(w, c, "height"), num(w, p, "height"));
    if py + y >= top && py + y + h <= bottom {
        return 0.0;
    }
    let f = ph - y - h;
    if py + f >= top && py + f + h <= bottom {
        return f - y;
    }
    // Neither side fits: allowVerticalMove keeps it inside, the top edge winning.
    (py + y).min(bottom - h).max(top) - py - y
}

/// The Flickable of a ScrollView, which is its contentItem: the one declared first in it, else
/// the one it made (`World::scroll_view_complete`). QQuickControl::resizeContent fits it to
/// the padded rect.
fn scroll_flickable(w: &mut World, i: usize) -> Option<usize> {
    if !is(w, i, "ScrollView") {
        return None;
    }
    match w.read(i, "contentItem") {
        Some(Value::Item(f)) if f != i && !w.dead.contains(&f) && is_flickable(w, f) => Some(f),
        _ => None,
    }
}

pub(crate) fn is_flickable(w: &World, i: usize) -> bool {
    w.items[i].prop_info("contentY").is_some()
}

pub(crate) fn num(w: &mut World, i: usize, name: &str) -> f64 {
    w.read(i, name)
        .map(|v| v.to_number())
        .filter(|x| x.is_finite())
        .unwrap_or(0.0)
}

/// A `real` property as Qt stores it: NaN/Inf kept, absent reads 0.
fn num_nan(w: &mut World, i: usize, name: &str) -> f64 {
    w.read(i, name).map(|v| v.to_number()).unwrap_or(0.0)
}

fn opt_num(w: &mut World, i: usize, name: &str) -> Option<f64> {
    if !w.items[i].props.contains_key(name) {
        return None;
    }
    w.read(i, name)
        .map(|v| v.to_number())
        .filter(|x| x.is_finite())
}

/// `radius` and `samples` of a GaussianBlur or DropShadow. GaussianBlur.qml and
/// DropShadowBase.qml: `property int samples: 9`, `property real radius: Math.floor(samples / 2)`.
fn gauss_radius_samples(w: &mut World, i: usize) -> (f64, i32) {
    let samples = opt_num(w, i, "samples").unwrap_or(9.0) as i32;
    let radius = opt_num(w, i, "radius").unwrap_or((samples as f64 / 2.0).floor());
    (radius, samples)
}

pub(crate) fn truthy(w: &mut World, i: usize, name: &str, def: bool) -> bool {
    match w.read(i, name) {
        Some(Value::Undefined) | None => def,
        Some(v) => v.truthy(),
    }
}

/// Anchored item: QQuickControlPrivate::resizeBackground only resizes a background sitting at
/// x/y 0, so an anchored (e.g. centerIn) background keeps its implicit size.
fn bg_anchored(w: &mut World, i: usize) -> bool {
    [
        "anchors.fill",
        "anchors.centerIn",
        "anchors.left",
        "anchors.right",
        "anchors.top",
        "anchors.bottom",
        "anchors.horizontalCenter",
        "anchors.verticalCenter",
    ]
    .iter()
    .any(|k| !matches!(w.read(i, k), None | Some(Value::Null | Value::Undefined)))
}

pub(crate) fn explicit(w: &World, i: usize, name: &str) -> bool {
    w.is_assigned(i, name)
        || w.items[i]
            .props
            .get(name)
            .is_some_and(|p| p.binding.is_some() || p.alias.is_some())
}

const LMIN: [&str; 2] = ["\x01lminw", "\x01lminh"];
const LMAX: [&str; 2] = ["\x01lmaxw", "\x01lmaxh"];
/// Where a ListView rested when last laid out: its header's edge, or its bottom when reversed.
const VSTART: &str = "\x01vstart";
/// Set once a view took the margin before its content on the axis it does not scroll along.
const CROSS_START: &str = "\x01xstart";
/// The implicit size a layout last set over its own `implicitWidth`/`implicitHeight` binding.
const LIMP: [&str; 2] = ["\x01limpw", "\x01limph"];

fn set(w: &mut World, i: usize, name: &str, v: f64) {
    let cur = w.items[i].props.get(name).map(|p| p.value.to_number());
    if cur != Some(v) {
        w.write(i, name, Value::Num(v));
    }
}

/// QQuickControl `leftPadding` etc.: side, then `horizontalPadding`/`verticalPadding`, then `padding`.
pub(crate) fn pad_side(w: &mut World, i: usize, side: &str) -> f64 {
    let own = format!("{side}Padding");
    let axis = if matches!(side, "left" | "right") {
        "horizontalPadding"
    } else {
        "verticalPadding"
    };
    if side == "top"
        && let Some(t) = material_area_top(w, i)
    {
        return t;
    }
    // ComboBox.qml binds both sides to `padding`, and the one of the indicator to its width
    // and the spacing too: World::read gives them, and the wider padding after a reset.
    if matches!(side, "left" | "right") && qml::types::inherits(&w.items[i].types, "QQuickComboBox")
    {
        return num(w, i, &own);
    }
    // Basic SpinBox: the down/up indicator widths are part of the left/right padding.
    if is(w, i, "SpinBox") && matches!(side, "left" | "right") && !explicit(w, i, &own) {
        let base = if explicit(w, i, axis) {
            opt_num(w, i, axis)
        } else {
            opt_num(w, i, "padding")
        }
        .unwrap_or(0.0);
        let k = if side == "left" {
            "down.indicator"
        } else {
            "up.indicator"
        };
        return base + spin_indicator(w, i, k).map_or(0.0, |(_, iw)| iw);
    }
    if qml::types::basic_padding(&w.items[i].types).is_some() {
        // World::read resolves Basic-style paddings.
        Some(num(w, i, &own))
    } else if explicit(w, i, &own) {
        opt_num(w, i, &own)
    } else if explicit(w, i, axis) {
        opt_num(w, i, axis)
    } else {
        opt_num(w, i, &own).or_else(|| opt_num(w, i, "padding"))
    }
    .unwrap_or(0.0)
}

/// A SpinBox indicator and its width (explicit, else implicit).
pub fn spin_indicator(w: &mut World, i: usize, k: &str) -> Option<(usize, f64)> {
    let Some(Value::Item(ind)) = w.read(i, k) else {
        return None;
    };
    if ind == i || w.dead.contains(&ind) {
        return None;
    }
    let iw = if explicit(w, ind, "width") {
        num(w, ind, "width")
    } else {
        num(w, ind, "implicitWidth")
    };
    Some((ind, iw))
}

fn visible(w: &mut World, i: usize) -> bool {
    truthy(w, i, "visible", true)
}

/// Laid out by Row/Column/Layouts: visible and not `transparentForPositioner` (QQuickRepeater).
fn positioned(w: &mut World, i: usize) -> bool {
    visible(w, i) && !is(w, i, "Repeater")
}

/// QQuickBasePositioner::prePositioning: Row/Column/Grid/Flow also skip zero-sized children.
fn in_positioner(w: &mut World, i: usize) -> bool {
    positioned(w, i) && num(w, i, "width") != 0.0 && num(w, i, "height") != 0.0
}

/// QtQuick.Controls Basic `ToolTip`: centred above its parent (`y: -implicitHeight - 3`),
/// kept `margins` (6) inside the window like QQuickPopupPositioner.
fn place_tooltip(w: &mut World, parent: usize, c: usize) {
    let bound = |w: &World, n: &str| w.items[c].props.get(n).is_some_and(|p| p.binding.is_some());
    let (iw, ih) = (num(w, c, "implicitWidth"), num(w, c, "implicitHeight"));
    let x = if bound(w, "x") {
        num(w, c, "x")
    } else {
        (num(w, parent, "width") - iw) / 2.0
    };
    let y = if bound(w, "y") {
        num(w, c, "y")
    } else {
        -ih - 3.0
    };
    let mut top = parent;
    while !w.items[top]
        .types
        .iter()
        .any(|t| WINDOWS.contains(&t.as_str()))
    {
        match w.items[top].parent {
            Some(p) => top = p,
            None => break,
        }
    }
    let (px, py) = w.scene_pos(parent, Some(top));
    let (ww, wh) = (num(w, top, "width"), num(w, top, "height"));
    let m = opt_num(w, c, "margins")
        .filter(|v| *v >= 0.0)
        .unwrap_or(6.0);
    // QQuickToolTip allows flipping: QQuickPopupPositioner mirrors around the parent when
    // that shows more of the popup inside the margined window.
    let (pw, ph) = (num(w, parent, "width"), num(w, parent, "height"));
    let shown = |t: f64, sz: f64, lim: f64| ((t + sz).min(lim - m) - t.max(m)).max(0.0);
    let flip = |p0: f64, v: f64, sz: f64, psz: f64, lim: f64| {
        if lim > 0.0 && (p0 + v < m || p0 + v + sz > lim - m) {
            let f = psz - v - sz;
            if shown(p0 + f, sz, lim) > shown(p0 + v, sz, lim) {
                return f;
            }
        }
        v
    };
    let x = flip(px, x, iw, pw, ww);
    let y = flip(py, y, ih, ph, wh);
    let clamp = |p: f64, sz: f64, lim: f64| {
        if lim <= 0.0 {
            p
        } else if p + sz > lim - m {
            (lim - m - sz).max(m)
        } else {
            p.max(m)
        }
    };
    set(w, c, "x", clamp(px + x, iw, ww) - px);
    set(w, c, "y", clamp(py + y, ih, wh) - py);
}

/// Quickshell windows nested in a tree (e.g. a tooltip's PopupWindow) get their own surface.
const WINDOWS: &[&str] = &[
    "PopupWindow",
    "PanelWindow",
    "FloatingWindow",
    "ProxyWindowBase",
    "WlSessionLockSurface",
];

thread_local! {
    static VIEW_CURRENT: std::cell::RefCell<std::collections::HashMap<usize, usize>> = Default::default();
}

/// QQuickItemViewPrivate::updateCurrent focuses the current item only when it changes; the
/// focus scope then drops the previous holder.
fn current_takes_focus(w: &mut World, view: usize, c: usize) {
    if VIEW_CURRENT.with(|m| m.borrow_mut().insert(view, c)) != Some(c) {
        w.write(c, "focus", Value::Bool(true));
    }
}

pub(crate) fn kids(w: &mut World, i: usize) -> Vec<usize> {
    let ch = w.items[i].children.clone();
    ch.into_iter()
        .filter(|&c| is_item(w, c) && type_bits(w, c) & WINDOW_TYPE == 0 && !w.layer_part(c))
        .collect()
}

pub fn rgba(v: &Value) -> [u8; 4] {
    let c = match v {
        Value::Color(c) => *c,
        Value::Str(s) => qml::builtins::parse_color(s),
        _ => return [0, 0, 0, 0],
    };
    if c == qml::color::Color::INVALID {
        return [0, 0, 0, 0];
    }
    let c = c.to_rgb();
    let f = |x: f32| (x * 255.0).round() as u8;
    [f(c.red_f()), f(c.green_f()), f(c.blue_f()), f(c.alpha_f())]
}

/// A colour as the QColor holds it: red, green, blue and alpha of 16 bits each, which is what
/// an effect's shader is given (`rgba` is the 8 bits a rectangle's vertices take).
fn rgba16(v: &Value) -> [u16; 4] {
    let c = match v {
        Value::Color(c) => *c,
        Value::Str(s) => qml::builtins::parse_color(s),
        _ => return [0; 4],
    };
    if c == qml::color::Color::INVALID {
        return [0; 4];
    }
    let c = c.to_rgb();
    [c.c[0], c.c[1], c.c[2], c.a]
}

fn color16(w: &mut World, i: usize, name: &str, def: [u16; 4]) -> [u16; 4] {
    match w.read(i, name) {
        Some(v @ (Value::Color(_) | Value::Str(_))) => rgba16(&v),
        _ => def,
    }
}

fn color(w: &mut World, i: usize, name: &str, def: [u8; 4]) -> [u8; 4] {
    match w.read(i, name) {
        Some(v @ (Value::Color(_) | Value::Str(_))) => rgba(&v),
        _ => def,
    }
}

struct TextStyle {
    font: Option<Rc<FontStack>>,
    /// What picked `font`, as one number: the same for the same font however often it is loaded.
    font_key: f64,
    text: String,
    /// Rich text with character formats: (text, font, decoration) runs.
    spans: Option<Vec<(String, Rc<FontStack>, Deco)>>,
    /// Markdown link target of each span (QTextCharFormat::anchorHref).
    links: Vec<Option<String>>,
    /// Laid out as a QTextDocument (RichText, MarkdownText), which a Text shows in full.
    doc: bool,
}

/// Output a PanelWindow's layer surface is created on: its own `screen`'s name. An unset or
/// null `screen` (e.g. a binding that threw) leaves the output to the compositor (Hyprland: the
/// focused monitor), as quickshell does; reads of `screen` fall back to the first screen.
pub fn panel_output(w: &mut World, i: usize) -> Option<String> {
    let own = w.items[i].props.get("screen").map(|p| p.value.clone());
    match own {
        Some(Value::Item(sc)) => match w.read(sc, "name") {
            Some(Value::Str(n)) => Some(n.to_string()),
            _ => None,
        },
        _ => None,
    }
}

/// Qt::mightBeRichText: the first `<...>` before any newline names a known HTML element
/// (`<Direction>` stays plain), or `&lt;` precedes it.
fn might_be_rich_text(s: &str) -> bool {
    const ELEMENTS: &[&str] = &[
        "a",
        "address",
        "b",
        "big",
        "blockquote",
        "body",
        "br",
        "caption",
        "center",
        "cite",
        "code",
        "dd",
        "dfn",
        "div",
        "dl",
        "dt",
        "em",
        "font",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "head",
        "hr",
        "html",
        "i",
        "img",
        "kbd",
        "li",
        "link",
        "meta",
        "nobr",
        "ol",
        "p",
        "pre",
        "qt",
        "s",
        "samp",
        "script",
        "small",
        "span",
        "strong",
        "style",
        "sub",
        "sup",
        "table",
        "tbody",
        "td",
        "tfoot",
        "th",
        "thead",
        "title",
        "tr",
        "tt",
        "u",
        "ul",
        "var",
    ];
    let t = s.trim_start();
    if t.as_bytes()
        .get(..5)
        .is_some_and(|b| b.eq_ignore_ascii_case(b"<!doc"))
    {
        return true;
    }
    let Some(open) = t.find(['<', '\n']) else {
        return t.contains("&lt;");
    };
    if t[..open].contains("&lt;") {
        return true;
    }
    if !t[open..].starts_with('<') {
        return false;
    }
    let Some(close) = t[open..].find('>').map(|c| open + c) else {
        return false;
    };
    let inner: Vec<char> = t[open + 1..close].chars().collect();
    let mut tag = String::new();
    for (k, &c) in inner.iter().enumerate() {
        if c.is_alphanumeric() {
            tag.push(c);
        } else if !tag.is_empty() && (c.is_whitespace() || (c == '/' && k + 1 == inner.len())) {
            break;
        } else if !c.is_whitespace() && (!tag.is_empty() || c != '!') {
            return false;
        }
    }
    ELEMENTS.contains(&tag.to_lowercase().as_str())
}

bits::bit_flags! {
const RICH_BOLD: u8 = 1;
const RICH_ITALIC: u8 = 2;
const RICH_UNDERLINE: u8 = 4;
/// Markdown link span: QTextMarkdownImporter gives it the palette's link color.
const RICH_LINK: u8 = 8;
/// Markdown `![alt](src)`: one U+FFFC per image, the source in the href slot.
const RICH_IMAGE: u8 = 16;
/// A syntax highlighter's struck-out text (QTextCharFormat::fontStrikeOut).
const RICH_STRIKE: u8 = 32;
/// The run is a format range of a Text.StyledText (QQuickStyledText): one text item.
const RICH_RANGE: u8 = 64;
}
/// Character format beyond the font: (`<font color>`, `<u>`, inline image id, the
/// background a syntax highlighter gives the run, struck out).
type Deco = (
    Option<[u8; 4]>,
    bool,
    Option<u32>,
    Option<[u8; 4]>,
    bool,
    u32,
);

/// A QTextImageFormat image as QQuickTextImageHandler sizes it: the file at its natural size,
/// or a 16x16 space with nothing drawn when it can't be loaded. Unlike Qt (a synchronous load
/// during layout), it is decoded on the image pool: until it lands it takes the 16x16 space.
struct InlineImage {
    path: String,
    size: (usize, usize),
    tex: Option<Rc<Canvas>>,
    /// Modification time of a file that failed to load (retried once it changes).
    failed: Option<Option<std::time::SystemTime>>,
    used: u64,
    /// A decode is queued or running.
    loading: bool,
}

thread_local! {
    static INLINE_IMAGES: std::cell::RefCell<(HashMap<String, u32>, Vec<InlineImage>, u64)> =
        Default::default();
    /// Where inline decodes report (the world laying the text out).
    static INLINE_TX: std::cell::RefCell<Option<qml::svc::Sender<Loaded>>> = const {
        std::cell::RefCell::new(None)
    };
}

/// Decoded inline images kept at once; older ones are decoded again when next drawn.
const INLINE_IMAGES_KEPT: usize = 64;

fn queue_inline_decode(img: &mut InlineImage) {
    if img.loading {
        return;
    }
    let Some(tx) = INLINE_TX.with_borrow(|t| t.clone()) else {
        return;
    };
    img.loading = true;
    let path = img.path.clone();
    let job: DecodeJob = Box::new(move || {
        let tex = guarded(|| decode_file(&path, (0, 0), 0, None));
        let _ = tx.send(Loaded::Inline(path, tex));
    });
    submit_decode(job);
}

/// Inline images decoded since the last call are stored; `true` if any landed.
fn take_inline(loads: &ImageLoads) -> bool {
    loads.drain();
    let done = std::mem::take(&mut *loads.inline.borrow_mut());
    if done.is_empty() {
        return false;
    }
    forget_draws();
    INLINE_IMAGES.with_borrow_mut(|(ids, imgs, _)| {
        for (path, tex) in done {
            let Some(img) = ids.get(&path).and_then(|&id| imgs.get_mut(id as usize)) else {
                continue;
            };
            img.loading = false;
            match tex {
                Some(c) => {
                    img.size = (c.w, c.h);
                    img.tex = Some(Rc::new(c));
                    img.failed = None;
                }
                None => {
                    img.size = (16, 16);
                    img.tex = None;
                    img.failed = Some(file_mtime(&path));
                }
            }
        }
        while imgs.iter().filter(|i| i.tex.is_some()).count() > INLINE_IMAGES_KEPT
            && let Some(old) = imgs
                .iter_mut()
                .filter(|i| i.tex.is_some())
                .min_by_key(|i| i.used)
        {
            old.tex = None;
        }
    });
    true
}

/// Id of the inline image at the local path `path`, queuing its decode on first use.
fn inline_image(loads: &ImageLoads, path: &str) -> u32 {
    INLINE_TX.with_borrow_mut(|t| *t = Some(loads.tx.clone()));
    INLINE_IMAGES.with_borrow_mut(|(ids, imgs, clock)| {
        *clock += 1;
        let (id, fresh) = match ids.get(path) {
            Some(&id) => (id, false),
            None => {
                let id = imgs.len() as u32;
                ids.insert(path.to_string(), id);
                imgs.push(InlineImage {
                    path: path.to_string(),
                    size: (16, 16),
                    tex: None,
                    failed: Some(None),
                    used: 0,
                    loading: false,
                });
                (id, true)
            }
        };
        let img = &mut imgs[id as usize];
        img.used = *clock;
        if !img.loading && img.failed.is_some_and(|t| fresh || t != file_mtime(path)) {
            queue_inline_decode(img);
        }
        id
    })
}

fn inline_image_size(id: u32) -> (usize, usize) {
    INLINE_IMAGES.with_borrow(|(_, imgs, _)| imgs.get(id as usize).map_or((16, 16), |i| i.size))
}

fn inline_image_tex(id: u32) -> Option<Rc<Canvas>> {
    INLINE_IMAGES.with_borrow_mut(|(_, imgs, _)| {
        let img = imgs.get_mut(id as usize)?;
        if img.tex.is_none() && img.failed.is_none() {
            queue_inline_decode(img);
        }
        img.tex.clone()
    })
}

/// A rich-text segment as laid out: an inline image is an object of its own size whose bottom
/// sits on the baseline (QTextCharFormat::AlignNormal).
fn seg_shape(f: &FontStack, t: &str, d: &Deco) -> qsg::text::ShapedLine {
    match d.2 {
        Some(id) => {
            let (w, h) = inline_image_size(id);
            qsg::text::ShapedLine {
                glyphs: vec![],
                last_cluster: 0,
                advance: (w * 64 * t.chars().count()) as i64,
                ascent: (h * 64) as i64,
                descent: 0,
                leading: 0,
            }
        }
        None => f.shape(t),
    }
}

/// An inline image segment drawn at `x` with its bottom on `baseline`.
fn draw_inline_image(cv: &mut Target, id: u32, x: f64, baseline: f64, op: f32) {
    let Some(tex) = inline_image_tex(id) else {
        return;
    };
    let (w, h) = (tex.w as f64, tex.h as f64);
    let rect = [x, baseline - h, w, h];
    match cv {
        Target::Gpu(fr) => fr.image(tex, rect, [0.0, 0.0, 1.0, 1.0], op, true),
        Target::Cpu(c) => {
            let fill = qsg::image::FillMode::Stretch;
            cpu_image(c, &tex, rect, fill, true, op)
        }
    }
}

/// `<font color>`, read as QTextHtmlParser does with QColor::fromString; None when invalid.
fn font_color(s: &str) -> Option<[u8; 4]> {
    let c = qml::builtins::parse_color(s);
    (c != qml::color::Color::INVALID).then(|| rgba(&Value::Color(c)))
}

/// The HTML subset the original uses (b, i, u, a, font, br, p, entities) as (text, RICH_* flags) runs;
/// block tags become '\n'. `<a href>` is underlined in `link` (Text.linkColor or QPalette::Link).
/// `styled` is Text.StyledText: QQuickStyledText gives the text between two tags a format
/// range of its own while a tag is open (a RICH_RANGE run), and the ends of a range are ends
/// of text items (QTextEngine::addRequiredBoundaries), so letters of neighbouring tags are not
/// kerned with each other. A rich text document joins neighbouring fragments of one format.
fn html_runs(s: &str, link: [u8; 4], styled: bool) -> Vec<(String, u8, Option<[u8; 4]>)> {
    let mut runs: Vec<(String, u8, Option<[u8; 4]>)> = vec![];
    // QQuickStyledTextPrivate::parse: how many tags are open (formatStack) and whether one
    // opened or closed since the last range (formatChanged).
    let (mut open, mut changed) = (0i32, false);
    let (mut bold, mut italic, mut under) = (0i32, 0i32, 0i32);
    let mut colors: Vec<Option<[u8; 4]>> = vec![];
    let mut anchors: Vec<bool> = vec![];
    let mut rest = s;
    let mut space = false;
    let mut started = false;
    type Runs = Vec<(String, u8, Option<[u8; 4]>)>;
    let push = |runs: &mut Runs, (flags, col): (u8, Option<[u8; 4]>), c: char, new: bool| match runs
        .last_mut()
    {
        Some(r) if r.1 == flags && r.2 == col && !new => r.0.push(c),
        _ => runs.push((c.to_string(), flags, col)),
    };
    let ends_nl = |runs: &Runs| runs.last().is_none_or(|r| r.0.ends_with('\n'));
    while let Some(c) = rest.chars().next() {
        // Whether the letter about to be added starts a range.
        let range = |open: i32, changed: &mut bool| {
            let new = styled && open > 0 && *changed;
            *changed &= open == 0;
            new
        };
        let flags = (
            if bold > 0 { RICH_BOLD } else { 0 }
                | if italic > 0 { RICH_ITALIC } else { 0 }
                | if under > 0 { RICH_UNDERLINE } else { 0 }
                | if styled && open > 0 { RICH_RANGE } else { 0 },
            colors.last().copied().flatten(),
        );
        if c == '<'
            && let Some(end) = rest.find('>')
        {
            let tag = rest[1..end]
                .trim()
                .trim_end_matches('/')
                .trim()
                .to_ascii_lowercase();
            let name = tag.split_whitespace().next().unwrap_or("");
            // QTextHtmlParser: <style>/<script>/<head> content is not displayed.
            if matches!(name, "style" | "script" | "head") {
                let close = format!("</{name}");
                rest = match rest.to_ascii_lowercase().find(&close) {
                    Some(i) => rest[i..].find('>').map_or("", |j| &rest[i + j + 1..]),
                    None => "",
                };
                continue;
            }
            match name {
                "b" | "strong" | "i" | "em" | "u" | "font" | "a" => {
                    open += 1;
                    changed = true;
                }
                "/b" | "/strong" | "/i" | "/em" | "/u" | "/font" | "/a" if open > 0 => {
                    open -= 1;
                    changed = true;
                }
                _ => {}
            }
            match name {
                "b" | "strong" => bold += 1,
                "/b" | "/strong" => bold -= 1,
                "i" | "em" => italic += 1,
                "/i" | "/em" => italic -= 1,
                "u" => under += 1,
                "/u" => under -= 1,
                "font" => {
                    let col = tag
                        .split_once("color=")
                        .map(|(_, v)| v.trim_start_matches(['"', '\'']))
                        .and_then(|v| font_color(v.split(['"', '\'', ' ']).next().unwrap_or("")));
                    colors.push(col.or(colors.last().copied().flatten()));
                }
                "/font" => {
                    colors.pop();
                }
                "a" => {
                    let href = tag
                        .split_whitespace()
                        .skip(1)
                        .any(|a| a.starts_with("href"));
                    if href {
                        under += 1;
                        colors.push(Some(link));
                    }
                    anchors.push(href);
                }
                "/a" if anchors.pop() == Some(true) => {
                    under -= 1;
                    colors.pop();
                }
                _ => {}
            }
            if matches!(name, "br" | "p" | "/p" | "div" | "/div" | "li")
                && !(name == "p" && !started)
            {
                push(&mut runs, flags, '\n', range(open, &mut changed));
                started = true;
                space = false;
            }
            rest = &rest[end + 1..];
            continue;
        }
        if c == '&'
            && let Some(end) = rest[..floor_boundary(rest, 10)].find(';')
        {
            let ent = &rest[1..end];
            let r = match ent {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some('\u{a0}'),
                e if e.starts_with("#x") => u32::from_str_radix(&e[2..], 16)
                    .ok()
                    .and_then(char::from_u32),
                e if e.starts_with('#') => e[1..].parse().ok().and_then(char::from_u32),
                _ => None,
            };
            if let Some(r) = r {
                push(&mut runs, flags, r, range(open, &mut changed));
                started = true;
                space = false;
                rest = &rest[end + 1..];
                continue;
            }
        }
        if c.is_whitespace() {
            if !space && started && !ends_nl(&runs) {
                push(&mut runs, flags, ' ', range(open, &mut changed));
            }
            space = true;
        } else {
            push(&mut runs, flags, c, range(open, &mut changed));
            started = true;
            space = false;
        }
        rest = &rest[c.len_utf8()..];
    }
    runs
}

/// QTextMarkdownImporter subset the original's chat uses: paragraphs, ATX headings (bold), bullet and
/// ordered items, and inline strong/emphasis/code/links. Blocks are split by '\n'; a soft break
/// stays a line break (U+2028) where Qt joins it with a space, which runs "one number per
/// line" answers into one line.
fn markdown_runs(s: &str) -> Vec<(String, u8, Option<String>)> {
    let mut blocks: Vec<(String, u8)> = vec![];
    let mut para: Vec<&str> = vec![];
    // The open list item (index in `blocks`) and the next ordered number.
    let mut item: Option<usize> = None;
    let mut next: Option<u64> = None;
    let flush = |para: &mut Vec<&str>, blocks: &mut Vec<(String, u8)>| {
        if !para.is_empty() {
            blocks.push((para.join("\u{2028}"), 0));
            para.clear();
        }
    };
    for line in s.lines() {
        let t = line.trim();
        // A trailing backslash is a hard break; the line break is kept either way.
        let t = t.strip_suffix('\\').map_or(t, str::trim_end);
        if t.is_empty() {
            flush(&mut para, &mut blocks);
            item = None;
            continue;
        }
        let h = t.trim_start_matches('#');
        if t.starts_with('#') && (h.is_empty() || h.starts_with(' ')) && t.len() - h.len() <= 6 {
            flush(&mut para, &mut blocks);
            blocks.push((h.trim().to_string(), RICH_BOLD));
            (item, next) = (None, None);
            continue;
        }
        // QTextMarkdownImporter: `-` is ListDisc, `*` ListCircle, `+` ListSquare.
        if let Some((bullet, text)) = [("- ", "\u{2022}"), ("* ", "\u{25E6}"), ("+ ", "\u{25AA}")]
            .iter()
            .find_map(|(m, b)| Some((*b, t.strip_prefix(m)?)))
        {
            flush(&mut para, &mut blocks);
            let (mark, text) = task_item(text.trim_start());
            blocks.push((list_item(mark.unwrap_or(bullet), text), 0));
            (item, next) = (Some(blocks.len() - 1), None);
            continue;
        }
        if let Some((n, delim, text)) = ordered_item(t)
            // Only a list starting at 1 interrupts a paragraph (CommonMark).
            && (para.is_empty() || n == 1)
        {
            flush(&mut para, &mut blocks);
            let n = next.unwrap_or(n);
            let (mark, text) = task_item(text);
            let number = format!("{n}{delim}");
            blocks.push((list_item(mark.unwrap_or(&number), text), 0));
            (item, next) = (Some(blocks.len() - 1), Some(n + 1));
            continue;
        }
        if let Some(k) = item {
            blocks[k].0.push('\u{2028}');
            blocks[k].0.push_str(t);
            continue;
        }
        next = None;
        para.push(t);
    }
    flush(&mut para, &mut blocks);
    let mut out: Vec<(String, u8, Option<String>)> = vec![];
    for (k, (b, f)) in blocks.iter().enumerate() {
        if k > 0 {
            out.push(("\n".into(), 0, None));
        }
        md_inline(b, *f, None, &mut out);
    }
    let mut merged: Vec<(String, u8, Option<String>)> = vec![];
    for (t, f, h) in out {
        match merged.last_mut() {
            Some(l) if l.1 == f && l.2 == h && f & RICH_IMAGE == 0 => l.0.push_str(&t),
            _ => merged.push((t, f, h)),
        }
    }
    merged
}

/// QTextDocument::indentWidth: where a list item's text starts.
const LIST_INDENT: f64 = 40.0;
/// Brackets a list item's marker at the start of its block, for `rich_laid` to hang it.
const LIST_MARK: char = '\u{E000}';
const LIST_MARK_END: char = '\u{E001}';

fn list_item(marker: &str, text: &str) -> String {
    format!("{LIST_MARK}{marker}{LIST_MARK_END}{text}")
}

/// A task list item (md4c MD_FLAG_TASKLISTS): `[ ]`, `[x]` or `[X]` and white space before the
/// text. Its box stands where the item's bullet or number would (QQuickTextNodeEngine: U+2612
/// for a checked BlockMarker, U+2610 for an unchecked one).
fn task_item(text: &str) -> (Option<&'static str>, &str) {
    let mark = match text.get(..3) {
        Some("[ ]") => "\u{2610}",
        Some("[x]" | "[X]") => "\u{2612}",
        _ => return (None, text),
    };
    let rest = &text[3..];
    if rest.starts_with(|c: char| !c.is_whitespace()) {
        return (None, text);
    }
    (Some(mark), rest.trim_start())
}

/// `1. text` / `1) text`: the start number (at most 9 digits), the delimiter and the text.
fn ordered_item(t: &str) -> Option<(u64, char, &str)> {
    let digits = t.find(|c: char| !c.is_ascii_digit())?;
    if !(1..=9).contains(&digits) {
        return None;
    }
    let delim = t[digits..].chars().next()?;
    let rest = &t[digits + 1..];
    if !matches!(delim, '.' | ')') || !rest.starts_with([' ', '\t']) {
        return None;
    }
    Some((t[..digits].parse().ok()?, delim, rest.trim_start()))
}

/// `href`: the target of the enclosing `[label](url)`, as QTextCharFormat::anchorHref.
fn md_inline(s: &str, flags: u8, href: Option<&str>, out: &mut Vec<(String, u8, Option<String>)>) {
    let h = || href.map(str::to_string);
    let mut lit = String::new();
    let mut i = 0;
    'scan: while i < s.len() {
        let rest = &s[i..];
        let prev = s[..i].chars().next_back();
        if let Some(r) = rest.strip_prefix('\\')
            && let Some(c) = r.chars().next()
            && c.is_ascii_punctuation()
        {
            lit.push(c);
            i += 1 + c.len_utf8();
            continue;
        }
        if rest.starts_with("![")
            && let Some(mid) = rest.find("](")
            && let Some(end) = rest[mid..].find(')')
        {
            // QTextMarkdownImporter: an image object; the alt text is not shown.
            out.push((std::mem::take(&mut lit), flags, h()));
            let src = rest[mid + 2..mid + end]
                .split_whitespace()
                .next()
                .unwrap_or("");
            let src = src
                .strip_prefix('<')
                .and_then(|u| u.strip_suffix('>'))
                .unwrap_or(src);
            out.push(("\u{FFFC}".into(), flags | RICH_IMAGE, Some(src.to_string())));
            i += mid + end + 1;
            continue;
        }
        if rest.starts_with('[')
            && let Some(mid) = rest.find("](")
            && let Some(end) = rest[mid..].find(')')
        {
            out.push((std::mem::take(&mut lit), flags, h()));
            let url = &rest[mid + 2..mid + end];
            md_inline(&rest[1..mid], flags | RICH_LINK, Some(url), out);
            i += mid + end + 1;
            continue;
        }
        if let Some(n) = md_autolink(rest, prev) {
            out.push((std::mem::take(&mut lit), flags, h()));
            let url = &rest[..n];
            let url = url
                .strip_prefix('<')
                .and_then(|u| u.strip_suffix('>'))
                .unwrap_or(url);
            // md4c's permissive www. autolinks get an http:// href.
            let target = if url.get(..4).is_some_and(|p| p.eq_ignore_ascii_case("www.")) {
                format!("http://{url}")
            } else {
                url.to_string()
            };
            out.push((url.to_string(), flags | RICH_LINK, Some(target)));
            i += n;
            continue;
        }
        for (m, f) in [
            ("**", RICH_BOLD),
            ("__", RICH_BOLD),
            ("*", RICH_ITALIC),
            ("_", RICH_ITALIC),
            ("`", 0),
        ] {
            if !rest.starts_with(m)
                || (m.starts_with('_') && prev.is_some_and(char::is_alphanumeric))
            {
                continue;
            }
            let Some(end) = rest[m.len()..].find(m).filter(|&e| e > 0) else {
                continue;
            };
            out.push((std::mem::take(&mut lit), flags, h()));
            let inner = &rest[m.len()..m.len() + end];
            if m == "`" {
                out.push((inner.to_string(), flags, h()));
            } else {
                md_inline(inner, flags | f, href, out);
            }
            i += 2 * m.len() + end;
            continue 'scan;
        }
        let c = rest.chars().next().unwrap();
        lit.push(c);
        i += c.len_utf8();
    }
    out.push((lit, flags, h()));
}

/// Byte length of an autolink at the start of `rest`: `<scheme:...>` or a GitHub-dialect
/// permissive URL (`http://`, `https://`, `ftp://`, `www.`) after a word boundary, without the
/// trailing punctuation md4c leaves out.
fn md_autolink(rest: &str, prev: Option<char>) -> Option<usize> {
    if let Some(r) = rest.strip_prefix('<') {
        let end = r.find(['>', '<', ' ', '\n'])?;
        let (scheme, _) = r[..end].split_once(':')?;
        let ok = r[end..].starts_with('>')
            && (2..=32).contains(&scheme.len())
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c));
        return ok.then_some(end + 2);
    }
    if prev.is_some_and(|c| !(c.is_whitespace() || "*_~(".contains(c))) {
        return None;
    }
    let lower = rest.get(..8).unwrap_or(rest).to_ascii_lowercase();
    let start = ["http://", "https://", "ftp://", "www."]
        .iter()
        .find(|p| lower.starts_with(*p))?
        .len();
    let mut n = rest
        .find(|c: char| c.is_whitespace() || c == '<')
        .unwrap_or(rest.len());
    loop {
        let t = &rest[..n];
        let open = t.matches('(').count();
        if t.ends_with(['.', ',', ':', ';', '!', '?', '*', '_', '~', '"', '\''])
            || (t.ends_with(')') && t.matches(')').count() > open)
        {
            n -= 1;
        } else {
            break;
        }
    }
    (n > start).then_some(n)
}

/// A laid-out line of rich text: shaped segments in their own fonts, sharing one baseline.
struct RichLine {
    segs: Vec<(Rc<FontStack>, qsg::text::ShapedLine, f64, Deco)>,
    advance: f64,
    /// QTextLine::naturalTextWidth: advance plus the last glyph's negative right bearing.
    natural: f64,
    ascent: i64,
    descent: i64,
}

impl RichLine {
    fn height(&self) -> f64 {
        ((self.ascent + self.descent + 63) >> 6) as f64
    }

    /// QQuickTextNodeEngine::processCurrentLine: the line's underlines and strike-outs in the
    /// order Qt adds them, as (left end, width, (offset from the baseline, lineThickness),
    /// colour of the format). It goes over the line's segments from the left: an underline
    /// reaches to the next segment when that one is underlined too or starts before this one
    /// ends, joins the one before it of the same colour that ends within half a pixel, and all
    /// up to the next segment without one are drawn with the metrics of the font with the
    /// thickest line among them. A strike-out is a line for each run of glyphs of one font, a
    /// third of that font's ascent above the baseline, and is added when the next run is
    /// reached, after the underlines that end there.
    fn underlines(&self) -> Vec<UnderlineOf> {
        let on = |k: usize| self.segs[k].3.1 && self.segs[k].3.2.is_none();
        let struck = |k: usize| self.segs[k].3.4 && self.segs[k].3.2.is_none();
        let mut order: Vec<usize> = (0..self.segs.len())
            .filter(|&k| !self.segs[k].1.glyphs.is_empty() || self.segs[k].3.2.is_some())
            .collect();
        order.sort_by(|&a, &b| self.segs[a].2.total_cmp(&self.segs[b].2));
        let mut done = vec![];
        let mut pending: Vec<(f64, f64, Option<[u8; 4]>)> = vec![];
        let mut kept = (0, 0);
        let mut strike: Option<UnderlineOf> = None;
        for (n, &k) in order.iter().enumerate() {
            let (f, l, x, deco) = &self.segs[k];
            if !on(k) {
                let under = (kept.0 as f64, kept.1);
                done.extend(pending.drain(..).map(|(a, b, c)| (a, b - a, under, c)));
                kept = (0, 0);
            }
            done.extend(strike.take());
            if !on(k) && !struck(k) {
                continue;
            }
            let mut runs: Vec<(i64, usize)> = l.glyphs.iter().map(|g| (g.2, g.0)).collect();
            runs.sort_by_key(|r| r.0);
            let end = x + l.advance as f64 / 64.0;
            let next = order.get(n + 1).map(|&next| (next, self.segs[next].2));
            if struck(k) {
                // QQuickTextNodeEngine: a decorated run reaches to the next one when the two
                // share a decoration, a background being one.
                let shares = |next: usize| {
                    struck(next)
                        || (on(k) && on(next))
                        || (deco.3.is_some() && self.segs[next].3.3.is_some())
                };
                let right = match next {
                    Some((next, left)) if shares(next) || left < end => left,
                    _ => end,
                };
                let mut fonts: Vec<(f64, usize)> = vec![];
                for &(at, font) in &runs {
                    if fonts.last().is_none_or(|last| last.1 != font) {
                        let left = if fonts.is_empty() {
                            0.0
                        } else {
                            at as f64 / 64.0
                        };
                        fonts.push((x + left, font));
                    }
                }
                for (j, &(left, font)) in fonts.iter().enumerate() {
                    done.extend(strike.take());
                    let Some(font) = f.font(font) else {
                        continue;
                    };
                    let right = fonts.get(j + 1).map_or(right, |next| next.0);
                    let metrics = (font.ascent as f64 / -3.0, font.underline().1);
                    strike = Some((left, right - left, metrics, deco.0));
                }
            }
            if !on(k) {
                continue;
            }
            for (pos, th) in runs.iter().filter_map(|r| Some(f.font(r.1)?.underline())) {
                if th > kept.1 {
                    kept = (pos, th);
                }
            }
            let right = match next {
                Some((next, left)) if on(next) || left < end => left,
                _ => end,
            };
            match pending.last_mut() {
                Some(last) if last.2 == deco.0 && (last.1 - x).abs() < 0.5 => last.1 = right,
                _ => pending.push((*x, right, deco.0)),
            }
        }
        let under = (kept.0 as f64, kept.1);
        done.extend(pending.into_iter().map(|(a, b, c)| (a, b - a, under, c)));
        done.extend(strike);
        done
    }
}

/// An underline or a strike-out of a `RichLine`: see `RichLine::underlines`.
type UnderlineOf = (f64, f64, (f64, i64), Option<[u8; 4]>);

/// The lines of a TextEdit's text with formats, wrapped at `wrap` as its `wrapMode` says.
fn edit_rich_lines(
    w: &mut World,
    i: usize,
    f: &Rc<FontStack>,
    spans: &[(String, Rc<FontStack>, Deco)],
    wrap: Option<f64>,
) -> Vec<RichLine> {
    let bounds = match wrap {
        Some(width) => Bounds::wrapped(Wrap::from_qml(num(w, i, "wrapMode") as i32), width),
        None => Bounds::NATURAL,
    };
    let laid = rich_laid(f, spans, &bounds);
    let mut lines = laid.lines;
    // A QTextDocument's text that ends in a line separator has one more, empty, block.
    let len: usize = spans.iter().map(|s| s.0.len()).sum();
    let ends = spans.iter().rev().find(|s| !s.0.is_empty());
    if ends.is_some_and(|s| s.0.ends_with(text_layout::is_separator))
        && laid.starts.last() != Some(&len)
    {
        lines.push(rich_line(f, vec![], 0.0, None, false));
    }
    lines
}

/// A line's segments placed from `start`, each tab as wide as the way to the next tab stop.
/// With `marker` the first segment is the line's list marker, at that x.
fn rich_line(
    base: &FontStack,
    segs: Vec<(String, Rc<FontStack>, Deco)>,
    start: f64,
    marker: Option<f64>,
    rtl: bool,
) -> RichLine {
    let empty = base.shape("");
    let (mut ascent, mut descent) = (empty.ascent, empty.descent);
    let mut x = (start * 64.0).round() as i64;
    let mut natural = x;
    let mut placed = Vec::with_capacity(segs.len());
    let text_from = marker.is_some() as usize;
    if rtl
        || segs[text_from.min(segs.len())..]
            .iter()
            .any(|s| s.0.chars().any(|c| c >= '\u{590}'))
    {
        // QTextLine lays the line out once, its items in the order their bidi levels give;
        // a format only ends an item where it starts and ends.
        if let (Some(m), Some((t, f, deco))) = (marker, segs.first()) {
            let l = seg_shape(f, t, deco);
            (ascent, descent) = (ascent.max(l.ascent), descent.max(l.descent));
            placed.push((f.clone(), l, m, *deco));
        }
        let segs = &segs[text_from.min(segs.len())..];
        let line: String = segs.iter().map(|s| s.0.as_str()).collect();
        let levels = qsg::text::level_runs(&line, rtl);
        // (segment, start, end, level) in the order of the text.
        let mut parts: Vec<(usize, usize, usize, u8)> = vec![];
        let mut at = 0;
        for (k, (t, f, deco)) in segs.iter().enumerate() {
            let end = at + t.len();
            if t.is_empty() {
                let l = seg_shape(f, t, deco);
                (ascent, descent) = (ascent.max(l.ascent), descent.max(l.descent));
            }
            for &(a, b, level) in &levels {
                if a < end && b > at {
                    parts.push((k, a.max(at), b.min(end), level));
                }
            }
            at = end;
        }
        let shaped: Vec<qsg::text::ShapedLine> = parts
            .iter()
            .map(|&(k, a, b, _)| {
                let (_, f, deco) = &segs[k];
                let l = match deco.2 {
                    Some(_) => seg_shape(f, &line[a..b], deco),
                    None => f.shape_within(&line, a, b, rtl),
                };
                (ascent, descent) = (ascent.max(l.ascent), descent.max(l.descent));
                l
            })
            .collect();
        let mut xs = vec![0; parts.len()];
        let levels: Vec<u8> = parts.iter().map(|p| p.3).collect();
        for k in qsg::text::visual_order(&levels) {
            xs[k] = x;
            x += shaped[k].advance;
        }
        let overhang = match (parts.last(), shaped.last()) {
            (Some(&(k, ..)), Some(l)) => segs[k].1.natural_width(l) - l.advance,
            _ => 0,
        };
        for ((&(k, ..), l), at) in parts.iter().zip(shaped).zip(xs) {
            placed.push((segs[k].1.clone(), l, at as f64 / 64.0, segs[k].2));
        }
        return RichLine {
            segs: placed,
            advance: x as f64 / 64.0,
            natural: (x + overhang.max(0)) as f64 / 64.0,
            ascent,
            descent,
        };
    }
    for (k, (t, f, deco)) in segs.into_iter().enumerate() {
        if k == 0
            && let Some(m) = marker
        {
            let l = seg_shape(&f, &t, &deco);
            ascent = ascent.max(l.ascent);
            descent = descent.max(l.descent);
            placed.push((f, l, m, deco));
            continue;
        }
        for (j, piece) in t.split('\t').enumerate() {
            if j > 0 {
                x += qsg::text::tab_width(x);
            }
            let l = seg_shape(&f, piece, &deco);
            ascent = ascent.max(l.ascent);
            descent = descent.max(l.descent);
            if piece.is_empty() {
                continue;
            }
            let at = x as f64 / 64.0;
            natural = x + f.natural_width(&l);
            x += l.advance;
            placed.push((f.clone(), l, at, deco));
        }
    }
    RichLine {
        segs: placed,
        advance: x as f64 / 64.0,
        natural: natural.max(x) as f64 / 64.0,
        ascent,
        descent,
    }
}

/// QTextLayout over format ranges: lines broken at '\n' (a paragraph's end) and U+2028 and,
/// wrapped, at the Unicode line break opportunities; elided and cut off as `bounds` say. A
/// paragraph opening with a list marker is indented by LIST_INDENT, with the marker
/// right-aligned a space before the text (QTextDocumentLayout::drawListItem).
fn rich_laid(
    base: &Rc<FontStack>,
    spans: &[(String, Rc<FontStack>, Deco)],
    bounds: &Bounds,
) -> Laid<RichLine> {
    // A stretch of one format: what it prints, its font and format, the byte it starts at.
    type Piece = (String, Rc<FontStack>, Deco, usize);
    const PLAIN: Deco = (None, false, None, None, false, 0);
    let text: String = spans.iter().map(|s| s.0.as_str()).collect();
    let rtl = qsg::text::is_right_to_left(&text);
    // Span k is text[offs[k]..offs[k + 1]].
    let offs: Vec<usize> = std::iter::once(0)
        .chain(spans.iter().scan(0, |at, s| {
            *at += s.0.len();
            Some(*at)
        }))
        .collect();
    // Each inline image is a break opportunity of its own.
    let objects: Vec<(usize, usize)> = spans
        .iter()
        .zip(&offs)
        .filter(|(s, _)| s.2.2.is_some())
        .flat_map(|(s, &o)| {
            s.0.char_indices()
                .map(move |(k, c)| (o + k, o + k + c.len_utf8()))
        })
        .collect();
    // Per line a separator ends: where it starts, its indent, its list marker and where its
    // text starts.
    type Block = (usize, f64, Option<(usize, usize)>, usize);
    let mut blocks: Vec<Block> = Vec::new();
    let (mut at, mut indent) = (0, 0.0);
    loop {
        let rest = &text[at..];
        let (mut marker, mut content) = (None, at);
        if let Some(r) = rest.strip_prefix(LIST_MARK)
            && let Some(k) = r.find(LIST_MARK_END)
            && !r[..k].contains(text_layout::is_separator)
        {
            indent = LIST_INDENT;
            let from = at + LIST_MARK.len_utf8();
            marker = Some((from, from + k));
            content = from + k + LIST_MARK_END.len_utf8();
        }
        blocks.push((at, indent, marker, content));
        let Some((k, c)) = rest
            .char_indices()
            .find(|&(_, c)| text_layout::is_separator(c))
        else {
            break;
        };
        if c == '\n' {
            indent = 0.0;
        }
        at += k + c.len_utf8();
    }
    let block_at = |a: usize| blocks[blocks.partition_point(|block| block.0 <= a) - 1];
    let span_at = |pos: usize| offs.partition_point(|&o| o <= pos) - 1;
    // Neighbours of one font and format are shaped together, as one run of the text.
    let same = |a: &(String, Rc<FontStack>, Deco), f: &Rc<FontStack>, deco: &Deco| {
        deco.2.is_none() && Rc::ptr_eq(&a.1, f) && a.2 == *deco
    };
    let widths = std::cell::OnceCell::new();
    let widths = || {
        widths.get_or_init(|| {
            let mut widths = Widths::new(&text);
            let mut k = 0;
            while k < spans.len() {
                let (_, f, deco) = &spans[k];
                let mut end = k + 1;
                while end < spans.len() && same(&spans[end], f, deco) {
                    end += 1;
                }
                let (from, to) = (offs[k], offs[end]);
                match deco.2 {
                    Some(id) => {
                        let width = inline_image_size(id).0 as i64 * 64;
                        for (at, c) in text[from..to].char_indices() {
                            widths.object(from + at, c.len_utf8(), width);
                        }
                    }
                    None => widths.run(from, &text[from..to], f),
                }
                k = end;
            }
            // A list marker hangs in the indent: it takes no room in its line.
            for &(start, _, marker, content) in &blocks {
                if marker.is_some() {
                    widths.object(start, content - start, 0);
                }
            }
            widths.sum()
        })
    };
    // What text[a..b] prints, by format; with `tabs`, a tab ends a piece and is left out.
    let pieces = |a: usize, b: usize, hyphen: bool, tabs: bool| -> Vec<Piece> {
        let mut out: Vec<Piece> = Vec::new();
        // Where the last piece ends when the next one may continue it.
        let mut open = usize::MAX;
        let mut k = span_at(a);
        while k < spans.len() && offs[k] < b {
            let (_, f, deco) = &spans[k];
            let (from, to) = (a.max(offs[k]), b.min(offs[k + 1]));
            k += 1;
            let mut at = from;
            while at < to {
                let end = match text[at..to].find('\t').filter(|_| tabs) {
                    Some(tab) => at + tab,
                    None => to,
                };
                let piece = printed(&text[at..end], hyphen && end == b);
                if deco.2.is_some() {
                    let mut c_at = at;
                    for c in text[at..end].chars() {
                        out.push((c.to_string(), f.clone(), *deco, c_at));
                        c_at += c.len_utf8();
                    }
                } else if !piece.is_empty() {
                    match out.last_mut() {
                        Some(last)
                            if open == at
                                && same(&(String::new(), last.1.clone(), last.2), f, deco) =>
                        {
                            last.0.push_str(&piece)
                        }
                        _ => out.push((piece.into_owned(), f.clone(), *deco, at)),
                    }
                    open = end;
                }
                // Past the tab.
                at = end + (end < to) as usize;
            }
        }
        out
    };
    // The list marker of the line that starts at `a`: its segment and x, and where the
    // line's text starts.
    let marker_at = |a: usize| {
        let (start, indent, marker, content) = block_at(a);
        let (from, to) = marker.filter(|_| a == start)?;
        let (_, f, deco) = &spans[span_at(from)];
        let width = f.shape(&text[from..to]).advance + f.shape(" ").advance;
        let seg = (text[from..to].to_string(), f.clone(), *deco);
        Some((seg, indent - width as f64 / 64.0, content))
    };
    // A line shaped on its own, segment after segment: one that only separators end, and an
    // elided one, which Qt lays out anew: `cut` is where the text an ellipsis stands for
    // starts and ends, and what stands there.
    let alone = |a: usize, b: usize, cut: Option<((usize, usize), &str)>| {
        let marker = marker_at(a);
        let from = marker.as_ref().map_or(a, |m| m.2.min(b));
        let mut segs: Vec<(String, Rc<FontStack>, Deco)> = Vec::new();
        let mut add = |from: usize, to: usize, ellipsis: Option<&str>| {
            for (t, f, deco, _) in pieces(from, to, false, false) {
                match segs.last_mut() {
                    Some(last) if deco.2.is_none() && same(last, &f, &deco) => last.0.push_str(&t),
                    _ => segs.push((t, f, deco)),
                }
            }
            if let Some(ellipsis) = ellipsis {
                // QQuickTextPrivate::elideFormats: the formats cover the text kept, not the
                // ellipsis.
                match segs.last_mut() {
                    Some(last) if same(last, base, &PLAIN) => last.0.push_str(ellipsis),
                    _ => segs.push((ellipsis.to_string(), base.clone(), PLAIN)),
                }
            }
        };
        match cut {
            Some(((head, tail), ellipsis)) => {
                add(from, head.max(from), Some(ellipsis));
                let kept = text[tail..b].trim_end_matches(text_layout::is_separator);
                add(tail.max(from), tail + kept.len(), None);
            }
            None => add(from, b, None),
        }
        let x = marker.as_ref().map(|m| m.1);
        segs.splice(0..0, marker.map(|m| m.0));
        rich_line(base, segs, block_at(a).1, x, rtl)
    };
    let hyphen = |on: bool, at: usize| match on {
        true => spans[span_at(at.saturating_sub(1))].1.shape("-").advance,
        false => 0,
    };
    // A line's width counts from the item's edge: the indent is part of it.
    let x_at = |a: usize, b: usize| (block_at(a).1 * 64.0).round() as i64 + widths().advance(a, b);
    let measure = |a: usize, b: usize, h: bool| (x_at(a, b) + hyphen(h && b > a, b)) as f64 / 64.0;
    // A wrapped line measures what its part of the whole text does: each segment sits where
    // the text before it ends.
    let wrapped = bounds.wrap != Wrap::None;
    let whole = |l: &text_layout::Line| {
        if !wrapped {
            return alone(l.start, l.end, None);
        }
        let (a, b) = (l.start, l.end);
        let empty = base.shape("");
        let (mut ascent, mut descent) = (empty.ascent, empty.descent);
        let mut segs = Vec::new();
        let marker = marker_at(a);
        let from = marker.as_ref().map_or(a, |m| m.2.min(b));
        if let Some(((t, f, deco), x, _)) = marker {
            let shaped = seg_shape(&f, &t, &deco);
            (ascent, descent) = (ascent.max(shaped.ascent), descent.max(shaped.descent));
            segs.push((f, shaped, x, deco));
        }
        if let Some([up, down, _]) = widths().metrics(from, b) {
            (ascent, descent) = (ascent.max(up), descent.max(down));
        }
        let advance = x_at(a, b) + hyphen(l.hyphen && b > a, b);
        let broken = widths().wrapped_overhang(&text, l);
        let mut overhang = 0;
        for (t, f, deco, at) in pieces(from, b, l.hyphen, true) {
            let shaped = seg_shape(&f, &t, &deco);
            if deco.2.is_some() {
                ascent = ascent.max(shaped.ascent);
            }
            overhang = f.natural_width(&shaped) - shaped.advance;
            segs.push((f, shaped, x_at(a, at) as f64 / 64.0, deco));
        }
        RichLine {
            segs,
            advance: advance as f64 / 64.0,
            natural: (advance + broken.unwrap_or(overhang.max(0))) as f64 / 64.0,
            ascent,
            descent,
        }
    };
    let overhang = |a: usize, b: usize| widths().overhang(a, b) as f64 / 64.0;
    let mut flow = text_layout::flow(
        &text,
        &objects,
        bounds,
        &text_layout::Measures {
            shaped: &|at| widths().starts[at],
            item: &|at| widths().items[at],
            measure: &measure,
            overhang: &overhang,
            natural: &|l| whole(l).natural,
            height: &|l| whole(l).height(),
        },
    );
    let lines: Vec<RichLine> = flow
        .rows
        .iter()
        .map(|row| {
            let l = &row.line;
            let Some(mode) = elide_mode(&text, row, bounds.width, &measure) else {
                return whole(l);
            };
            let breaks = flow
                .breaks
                .get_or_insert_with(|| Breaks::new(&text, &objects));
            let (ellipsis, room) = base.ellipsis();
            let width = bounds.width - block_at(l.start).1;
            let kept = text_layout::elide(breaks, mode, l.start, l.next, width, room, &|a, b| {
                widths().advance(a, b) as f64 / 64.0
            });
            match kept {
                Some(cut) => {
                    let between = (l.start, l.next);
                    let ellipsis =
                        text_layout::ellipsis_between(&text, mode, between, cut, ellipsis);
                    alone(l.start, l.next, Some((cut, &ellipsis)))
                }
                None => rich_line(base, vec![], 0.0, None, rtl),
            }
        })
        .collect();
    let span = |l: &RichLine| (l.advance, l.natural);
    let mut spans: Vec<_> = lines.iter().map(span).collect();
    if let (true, Some(row)) = (flow.height_cut, flow.rows.last()) {
        // QQuickText counts the line it elides for the height unelided too.
        spans.push(span(&whole(&row.line)));
    }
    Laid {
        lines,
        starts: flow.rows.iter().map(|row| row.line.start).collect(),
        truncated: flow.truncated,
        spans,
        implicit_width: flow.max_width,
        implicit_height: flow.lines.iter().map(|l| whole(l).height()).sum(),
    }
}

/// Item whose `font.*` values a Text uses: follows whole-value `font: x.font` assignments.
fn font_source(w: &World, mut i: usize) -> usize {
    for _ in 0..8 {
        match w.items[i].props.get("font").map(|p| &p.value) {
            Some(Value::Item(g)) => match w.groups.get(g) {
                Some((o, n)) if n == "font" && *o != i => i = *o,
                _ => break,
            },
            _ => break,
        }
    }
    i
}

/// Text runs of highlighted byte spans (theme colour, bold, italic, underline, struck out;
/// the rest plain) and the background of each run.
#[allow(clippy::type_complexity)]
fn syntax_runs(
    text: &str,
    spans: &[qml::syntax::Span],
) -> (Vec<(String, u8, Option<[u8; 4]>)>, Vec<Option<[u8; 4]>>) {
    let mut out = vec![];
    let mut back = vec![];
    let mut at = 0;
    for s in spans {
        if s.start > at {
            out.push((text[at..s.start].to_string(), 0, None));
            back.push(None);
        }
        let f = [
            (s.bold, RICH_BOLD),
            (s.italic, RICH_ITALIC),
            (s.underline, RICH_UNDERLINE),
            (s.strike, RICH_STRIKE),
        ];
        let f = f.iter().filter(|f| f.0).fold(0, |all, f| all | f.1);
        out.push((text[s.start..s.end].to_string(), f, Some(s.color)));
        back.push(s.background);
        at = s.end;
    }
    if at < text.len() {
        out.push((text[at..].to_string(), 0, None));
        back.push(None);
    }
    (out, back)
}

/// An italic request as variable axes: the italic instance (`ital`), else the slanted one.
fn italic_axes(axes: &mut Vec<(String, f32)>) {
    for (tag, v) in [("ital", 1.0), ("slnt", -10.0)] {
        if !axes.iter().any(|a| a.0 == tag) {
            axes.push((tag.into(), v));
        }
    }
}

fn text_style(w: &mut World, fonts: &mut Fonts, i: usize) -> TextStyle {
    text_style_scaled(w, fonts, i, 1.0)
}

fn text_style_scaled(w: &mut World, fonts: &mut Fonts, i: usize, scale: f64) -> TextStyle {
    let text = w
        .read(i, "text")
        .map(|v| match v {
            Value::Undefined | Value::Null => String::new(),
            v => v.to_js_string(),
        })
        .unwrap_or_default();
    // Text.textFormat: PlainText 0, RichText 1, AutoText 2 (default), MarkdownText 3, StyledText 4.
    let fmt = if is(w, i, "TextMetrics") {
        // QQuickTextMetrics measures its string with QFontMetricsF: tags are letters there.
        0
    } else if explicit(w, i, "textFormat") {
        num(w, i, "textFormat") as i32
    } else {
        2
    };
    let mut hrefs: Vec<Option<String>> = vec![];
    let mut images: Vec<Option<u32>> = vec![];
    // The background of each run a syntax highlighter colours.
    let mut backgrounds: Vec<Option<[u8; 4]>> = vec![];
    let loads = image_loads(w);
    let runs = if fmt == 3 {
        Some(
            markdown_runs(&text)
                .into_iter()
                .map(|(t, f, href)| {
                    if f & RICH_IMAGE != 0 {
                        // QQuickTextDocumentWithImageResources::loadResource: the url as the
                        // Text's context resolves it.
                        images.push(
                            href.as_deref()
                                .map(|s| inline_image(loads, &w.resolved_url(i, s))),
                        );
                        hrefs.push(None);
                    } else {
                        images.push(None);
                        hrefs.push(href);
                    }
                    let link = (f & RICH_LINK != 0).then(qml::colorscheme::link_color);
                    (t, f & !RICH_LINK, link)
                })
                .collect(),
        )
    } else {
        (matches!(fmt, 1 | 4) || (fmt == 2 && might_be_rich_text(&text))).then(|| {
            // Text.linkColor applies to StyledText (and AutoText, parsed as StyledText); RichText
            // documents use the palette's link colour.
            let link = if fmt == 1 {
                qml::colorscheme::link_color()
            } else {
                color(w, i, "linkColor", [0, 0, 255, 255])
            };
            html_runs(&text, link, fmt != 1)
        })
    };
    // org.kde.syntaxhighlighting `SyntaxHighlighter` colours a plain TextEdit's text.
    let runs = runs.or_else(|| {
        let h = qml::syntax::spans_for(w, i, &text)?;
        let (runs, back) = syntax_runs(&text, &h.spans);
        backgrounds = back;
        Some(runs)
    });
    let text = match &runs {
        Some(r) => r.iter().map(|r| r.0.as_str()).collect(),
        None => text,
    };
    // `font: other.font` assigns the whole QFont value (replacing the component's own font.* bindings).
    let qt = qt_rendering(w, i);
    let i = font_source(w, i);
    // Unset font properties come from the application (platform theme) font.
    let sys = qml::colorscheme::system_font();
    let family = match w.read(i, "font.family") {
        Some(Value::Str(s)) if !s.is_empty() => s.to_string(),
        _ => sys.family.clone(),
    };
    let mut px = num(w, i, "font.pixelSize");
    if px <= 0.0 {
        let pt = num(w, i, "font.pointSize");
        px = if pt > 0.0 {
            pt * 96.0 / 72.0
        } else {
            sys.pixels()
        };
    }
    px *= scale;
    let mut weight = num(w, i, "font.weight") as i32;
    if weight <= 0 {
        // QFont::setBold: `font.bold` is the weight 700 (or 400) when no weight is given.
        weight = match w.read(i, "font.bold") {
            Some(Value::Bool(true)) => 700,
            Some(Value::Bool(false)) => 400,
            // DayOfWeekRow.qml: `font.bold: true`.
            _ if is(w, i, "DayOfWeekRow") => 700,
            _ => sys.weight,
        };
    }
    let style = match w.read(i, "font.styleName") {
        Some(Value::Str(s)) => s.to_string(),
        _ => sys.style_name.clone(),
    };
    let mut axes: Vec<(String, f32)> = match w.read(i, "font.variableAxes") {
        Some(Value::Obj(o)) => o
            .borrow()
            .props
            .iter()
            .filter(|(k, _)| k.len() == 4)
            .map(|(k, v)| (k.to_string(), v.to_number() as f32))
            .collect(),
        _ => vec![],
    };
    // QFont::setItalic: the family's italic face, or its upright one slanted, as for <i>.
    if matches!(w.read(i, "font.italic"), Some(Value::Bool(true))) {
        italic_axes(&mut axes);
    }
    axes.sort_by(|a, b| a.0.cmp(&b.0));
    // QFont::setLetterSpacing(QFont::AbsoluteSpacing): pixels added after every character.
    let spacing = match num(w, i, "font.letterSpacing") {
        v if v.is_finite() => v,
        _ => 0.0,
    };
    let hinting = match num(w, i, "font.hintingPreference") as i32 {
        3 if qt => 4,
        h => h,
    };
    let mut links = vec![];
    // The format ranges of a StyledText, numbered: neighbours of one format stay apart.
    let mut ranges = 0;
    let spans = runs
        .filter(|r| {
            r.iter()
                .any(|r| r.1 != 0 || r.2.is_some() || r.0.contains([LIST_MARK, '\u{2028}']))
        })
        .map(|r| {
            r.into_iter()
                .enumerate()
                .filter_map(|(k, (t, flags, col))| {
                    // <b>: QFont::Bold; <i>: the italic instance (slnt/ital axes of variable faces).
                    let wt = if flags & RICH_BOLD != 0 {
                        weight.max(700)
                    } else {
                        weight
                    };
                    let mut ax = axes.clone();
                    if flags & RICH_ITALIC != 0 {
                        italic_axes(&mut ax);
                        ax.sort_by(|a, b| a.0.cmp(&b.0));
                    }
                    let px = px.round() as u32;
                    let font =
                        fonts.get_styled(&family, &style, px, wt, &ax, (hinting, spacing))?;
                    links.push(hrefs.get(k).cloned().flatten());
                    let image = images.get(k).copied().flatten();
                    let back = backgrounds.get(k).copied().flatten();
                    let lines = (flags & RICH_UNDERLINE != 0, flags & RICH_STRIKE != 0);
                    let range = if flags & RICH_RANGE != 0 {
                        ranges += 1;
                        ranges
                    } else {
                        0
                    };
                    Some((t, font, (col, lines.0, image, back, lines.1, range)))
                })
                .collect()
        });
    let named: Vec<(&str, u32)> = axes.iter().map(|(t, v)| (&t[..], v.to_bits())).collect();
    TextStyle {
        font_key: text_key(&(
            (&family, &style, px.round() as u32, weight),
            (named, hinting, spacing.to_bits()),
        )),
        font: fonts.get_styled(
            &family,
            &style,
            px.round() as u32,
            weight,
            &axes,
            (hinting, spacing),
        ),
        text,
        spans,
        links,
        doc: matches!(fmt, 1 | 3),
    }
}

/// QQuickTextPrivate::anchorAt: the link target under item-local (x, y) of a rich Text, laid
/// out as `paint` does (exact hit on a link segment, nothing between glyph runs).
pub fn link_at(w: &mut World, fonts: &mut Fonts, i: usize, x: f64, y: f64) -> Option<String> {
    if !is_text(w, i) && !is_text_doc(w, i) {
        return None;
    }
    let ts = text_style(w, fonts, i);
    let (Some(f), Some(spans)) = (&ts.font, &ts.spans) else {
        return None;
    };
    if ts.links.iter().all(Option::is_none) {
        return None;
    }
    // A per-span color keeps rich_laid from merging spans, so each segment names its span.
    let tagged: Vec<_> = spans
        .iter()
        .enumerate()
        .map(|(k, (t, sf, d))| {
            (
                t.clone(),
                sf.clone(),
                (Some((k as u32).to_le_bytes()), d.1, d.2, d.3, d.4, d.5),
            )
        })
        .collect();
    let lines = rich_laid(f, &tagged, &text_bounds(w, i, ts.doc, None)).lines;
    let (wd, ht) = (num(w, i, "width"), num(w, i, "height"));
    let th: f64 = lines.iter().map(|l| l.height()).sum();
    let ha = text_halign(w, i, &ts.text).1;
    let [pl, pt, pr, pb] = text_pad(w, i);
    let (wd, ht) = (wd - pl - pr, ht - pt - pb);
    let mut dy = pt
        + match num(w, i, "verticalAlignment") as u32 {
            128 => (ht - th) / 2.0,
            64 => ht - th,
            _ => 0.0,
        };
    for line in &lines {
        if y >= dy && y < dy + line.height() {
            let dx = pl
                + match ha {
                    4 => center_offset(wd, (line.advance * 64.0).round() as i64),
                    2 => wd - line.advance,
                    _ => 0.0,
                };
            return line.segs.iter().find_map(|(_, sl, sx, deco)| {
                let x0 = dx + sx;
                (x >= x0 && x < x0 + sl.advance as f64 / 64.0)
                    .then(|| ts.links.get(u32::from_le_bytes(deco.0?) as usize)?.clone())
                    .flatten()
            });
        }
        dy += line.height();
    }
    None
}

/// QQuickImageBase::componentComplete of Image `i`: its picture is loaded, or its load
/// started, before any `Component.onCompleted` runs and before its Behaviors are enabled. One
/// the pixmap cache holds, or a synchronous one, is Ready with its size; another is Loading.
/// A picture's statusChanged is withheld: the caller emits it.
pub fn load_image(w: &mut World, fonts: &Rc<std::cell::RefCell<Fonts>>, i: usize) {
    if !is_item(w, i) || w.dead.contains(&i) {
        return;
    }
    if is_picture(w, i) {
        let (iw, ih) = picture_load(w, i, Load::Created);
        take_implicit(w, i, iw, ih);
        picture_painted(w, i);
    } else if let Ok(mut f) = fonts.try_borrow_mut() {
        measure(w, &mut f, i);
    }
}

/// Gives `w` the shell's `load_image`: without it an Image's picture waits for the layout.
pub fn image_hook(w: &mut World, fonts: &Rc<std::cell::RefCell<Fonts>>) {
    let f = fonts.clone();
    w.image_complete = Some(Box::new(move |w, i| load_image(w, &f, i)));
}

/// Gives `w` the shell's layout of what a Loader has just created (`World::loaded_layout`).
pub fn loaded_layout_hook(w: &mut World, fonts: &Rc<std::cell::RefCell<Fonts>>) {
    let fonts = fonts.clone();
    w.loaded_layout = Some(Box::new(move |w, i, done| {
        if done {
            clear_fresh(w, i);
            return true;
        }
        // Inside a layout pass the pass itself places the item.
        let Ok(mut f) = fonts.try_borrow_mut() else {
            return false;
        };
        relayout_local(w, &mut f, i);
        true
    }));
}

/// Implicit sizes of Layouts and positioners created from item `n0` on (outermost new ones
/// only): QQuickBasePositioner::componentComplete positions before `Component.onCompleted`.
pub fn measure_new(w: &mut World, fonts: &mut Fonts, n0: usize) -> bool {
    let lay = |w: &World, i: usize| {
        [
            "RowLayout",
            "ColumnLayout",
            "GridLayout",
            "Row",
            "Column",
            "Grid",
            "Flow",
        ]
        .iter()
        .any(|t| is(w, i, t))
    };
    let mut any = false;
    for i in n0..w.items.len() {
        if w.dead.contains(&i) || !lay(w, i) {
            continue;
        }
        let mut p = w.items[i].parent;
        while let Some(q) = p.filter(|&q| q >= n0 && !lay(w, q)) {
            p = w.items[q].parent;
        }
        if p.is_some_and(|q| q >= n0) {
            continue;
        }
        measure(w, fonts, i);
        any = true;
    }
    // QQuickTextArea::itemChange: a TextArea declared in a ScrollView is attached to its
    // Flickable as soon as it is its child, before any Component.onCompleted: the Flickable
    // sizes it, then takes its implicit size (QQuickTextAreaPrivate::attachFlickable), each
    // in a pass of its own. A text set by a handler finds the field as wide as it was empty.
    for i in n0..w.items.len() {
        if w.dead.contains(&i) || !is(w, i, "ScrollView") {
            continue;
        }
        let Some(f) = scroll_flickable(w, i) else {
            continue;
        };
        if scroll_text_area(w, f).is_none() {
            continue;
        }
        for _ in 0..2 {
            measure(w, fonts, i);
            place(w, i);
        }
        any = true;
    }
    // Qt sizes a window's content item before its children complete, so `width` bindings
    // read in Component.onCompleted see the real geometry (e.g. GridView.cellWidth). Also
    // the `rusty-impulse -p` app windows (settings.qml, welcome.qml).
    for i in n0..w.items.len() {
        let app = |w: &World, i| ["ApplicationWindow", "Window"].iter().any(|t| is(w, i, t));
        if w.dead.contains(&i) || !(WINDOWS.iter().any(|t| is(w, i, t)) || app(w, i)) {
            continue;
        }
        let dim = |w: &mut World, k: &str, ik: &str| {
            let v = num(w, i, k);
            if v > 0.0 { v } else { num(w, i, ik) }
        };
        let (ww, wh) = (
            dim(w, "width", "implicitWidth"),
            dim(w, "height", "implicitHeight"),
        );
        if ww <= 0.0 || wh <= 0.0 {
            continue;
        }
        set(w, i, "width", ww);
        set(w, i, "height", wh);
        for c in kids(w, i) {
            measure(w, fonts, c);
        }
        place(w, i);
        any = true;
    }
    any
}

// A layout gave a wrapping Text or a Flow a new width, so its implicitHeight changes: like Qt, which
// polishes the layout again, it needs another pass even if no binding reacted.
thread_local!(static WRAP_RESIZED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) });

thread_local!(static MEMO_OFF: std::cell::Cell<bool> = const { std::cell::Cell::new(false) });

thread_local!(static SWIPE_POS: std::cell::RefCell<HashMap<usize, f64>> = Default::default());

// QQuickSliderPrivate::position where the pointer left it, with the (value, from, to) it was
// left at: handleMove sets it after the stepped value, and only updatePosition() (a new
// value or range) puts it back on the value.
thread_local!(static SLIDER_POS: std::cell::RefCell<HashMap<usize, ([f64; 3], f64)>> = Default::default());

/// qFuzzyCompare: QQuickSlider::setValue takes such a value for the one it has.
pub(crate) fn fuzzy_eq(a: f64, b: f64) -> bool {
    a == b || (a - b).abs() * 1e12 <= a.abs().min(b.abs())
}

/// The position of Slider `i`: where the pointer left it while value and range are the ones
/// it was left at, `(value - from) / (to - from)` otherwise.
pub(crate) fn slider_position(w: &mut World, i: usize) -> f64 {
    let at = [num(w, i, "value"), num(w, i, "from"), num(w, i, "to")];
    let held = SLIDER_POS.with_borrow_mut(|m| match m.get(&i) {
        Some((k, p)) if fuzzy_eq(k[0], at[0]) && k[1..] == at[1..] => Some(*p),
        Some(_) => {
            m.remove(&i);
            None
        }
        None => None,
    });
    let [v, f, t] = at;
    held.unwrap_or(if t != f {
        ((v - f) / (t - f)).clamp(0.0, 1.0)
    } else {
        0.0
    })
}

/// QQuickSliderPrivate::setPosition from handleMove (`Some`), or updatePosition (`None`).
pub(crate) fn set_slider_position(w: &mut World, i: usize, pos: Option<f64>) {
    let at = [num(w, i, "value"), num(w, i, "from"), num(w, i, "to")];
    SLIDER_POS.with_borrow_mut(|m| match pos {
        Some(p) => m.insert(i, (at, p)),
        None => m.remove(&i),
    });
    let p = slider_position(w, i);
    set(w, i, "position", p);
    set(w, i, "visualPosition", p);
}

/// SwipeView page geometry: (vertical, page pitch, page size, page count) of its ListView.
pub fn swipe_metrics(w: &mut World, i: usize) -> (bool, f64, f64, usize) {
    let vertical = num(w, i, "orientation") == 2.0;
    let spacing = prop_num(w, i, "spacing").unwrap_or(0.0);
    let size = if vertical {
        num(w, i, "height") - pad_side(w, i, "top") - pad_side(w, i, "bottom")
    } else {
        num(w, i, "width") - pad_side(w, i, "left") - pad_side(w, i, "right")
    }
    .max(0.0);
    let ch = kids(w, i);
    let count = ch.into_iter().filter(|&c| positioned(w, c)).count();
    (vertical, size + spacing, size, count)
}

/// The content position (contentX/Y of its ListView) a swipe moved a SwipeView to, or `None`
/// to rest at currentIndex.
pub fn set_swipe_pos(w: &mut World, i: usize, pos: Option<f64>) {
    let old = SWIPE_POS.with(|m| match pos {
        Some(p) => m.borrow_mut().insert(i, p),
        None => m.borrow_mut().remove(&i),
    });
    if old != pos {
        w.memo_invalidate(i);
        w.revision += 1;
        // No geometry write reaches layout_rev: the frame's local pass re-places the pages.
        w.resized.push(i);
        // Nor does a property write say the window changed: it draws that frame for the view.
        w.touched.push(i);
        // The pages moved without a property write: recorded draws and layers holding them
        // (SidebarLeftContent's OpacityMask layer) go stale.
        let pages = kids(w, i);
        invalidate_moved(w, &pages);
    }
}

pub fn swipe_pos(i: usize) -> Option<f64> {
    SWIPE_POS.with(|m| m.borrow().get(&i).copied())
}

/// Layout memo: a subtree with no write since it was last measured/placed is skipped
/// (`RI_DEBUG=nolaymemo` disables it; `RI_DEBUG=memocheck` compares against a full layout).
fn memo_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| !qml::debug::on(qml::debug::Switch::NOLAYMEMO))
        && !MEMO_OFF.with(|m| m.get())
}

/// Debug reference layouts run without the memo.
pub fn set_memo_off(off: bool) {
    MEMO_OFF.with(|m| m.set(off));
}

fn measure(w: &mut World, fonts: &mut Fonts, i: usize) {
    if !memo_on() {
        measure_prof(w, fonts, i);
        return code_made_wide(w, fonts, i);
    }
    if w.memo_m.contains(&i) {
        return;
    }
    measure_prof(w, fonts, i);
    if kids(w, i).iter().all(|c| w.memo_m.contains(c)) {
        w.memo_m.insert(i);
    }
    code_made_wide(w, fonts, i);
}

/// 1 on a ScrollView whose TextArea does not wrap and whose `contentWidth` is set, once it
/// was seen whether a Layout it is in was as wide as its implicit width when it was made.
const CODE_MADE: &str = "\x01codeMade";

thread_local! {
    /// The ScrollViews `code_made_wide` has to see yet, each with the Layout it waits for.
    static CODE_VIEWS: std::cell::RefCell<Vec<(usize, usize)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The outermost Layout over `i` that Qt has as wide as its implicit width as it completes:
/// one in a Layout, with no size of its own, that has not been laid out yet.
fn layout_made_implicit(w: &mut World, i: usize) -> Option<usize> {
    let (mut p, mut outer) = (w.items[i].parent, None);
    while let Some(q) = p.filter(|q| w.fresh.contains(q)) {
        p = w.items[q].parent;
        if linear_axis(w, q).is_some()
            && p.is_some_and(|l| linear_axis(w, l).is_some())
            && !explicit(w, q, "width")
            && !explicit(w, q, "height")
            && !bg_anchored(w, q)
        {
            outer = Some(q);
        }
    }
    outer
}

/// `pane_content` of ScrollView `i` with the TextArea `a`: notes one `code_made_wide` is for.
fn code_view_made(w: &mut World, i: usize, a: usize) {
    if w.fresh.is_empty()
        || w.items[i].props.contains_key(CODE_MADE)
        || !w.is_assigned(i, "contentWidth")
        || num(w, a, "wrapMode") != 0.0
    {
        return;
    }
    set(w, i, CODE_MADE, 1.0);
    if let Some(l) = layout_made_implicit(w, i) {
        CODE_VIEWS.with_borrow_mut(|v| v.push((i, l)));
    }
}

/// QQuickGridLayoutBase::componentComplete: a Layout in a Layout has no size as it completes,
/// so it is as large as the implicit size it takes there (QQuickLayoutPrivate::applySizeHints)
/// and arranges its items in that; the last to do so is the outermost. A Layout with a
/// paragraph in it is as wide as the paragraph on one line, and so is, less the margins, a
/// ScrollView in it. A TextArea in that ScrollView that does not wrap is then as wide as the
/// Flickable (QQuickTextAreaPrivate::resizeFlickableControl), and a `contentWidth` bound to
/// its width keeps it that wide when the Layout gets its real width: the code of
/// MessageCodeBlock.qml under a long paragraph scrolls sideways and shows its bar.
///
/// Layout `l` has just been measured: the ScrollViews of that kind in it are laid out once in
/// its implicit size. The pass that measures `l` then gives it its real size.
fn code_made_wide(w: &mut World, fonts: &mut Fonts, l: usize) {
    if CODE_VIEWS.with_borrow(|v| v.iter().all(|&(_, of)| of != l)) {
        return;
    }
    let mut views = Vec::new();
    CODE_VIEWS.with_borrow_mut(|v| {
        v.retain(|&(view, of)| {
            if of == l {
                views.push(view);
            }
            of != l
        })
    });
    views.retain(|i| !w.dead.contains(i));
    let (lw, lh) = (num(w, l, "implicitWidth"), num(w, l, "implicitHeight"));
    if views.is_empty() || w.dead.contains(&l) || !(lw.is_finite() && lh.is_finite()) {
        return;
    }
    // QQuickTextAreaPrivate::attachFlickable came first, as in `measure_new`.
    for &i in &views {
        let sized = scroll_flickable(w, i)
            .and_then(|f| scroll_text_area(w, f))
            .is_some_and(|a| w.items[a].props.contains_key(AREA_SIZED_BY_VIEW));
        for _ in 0..if sized { 0 } else { 2 } {
            measure(w, fonts, i);
            place(w, i);
        }
    }
    set(w, l, "width", lw);
    set(w, l, "height", lh);
    place(w, l);
    // QQuickScrollView::contentSizeChange: the `contentWidth` goes to the Flickable.
    for &i in &views {
        measure(w, fonts, i);
    }
}

fn place(w: &mut World, i: usize) {
    if !memo_on() {
        return place_inner(w, i);
    }
    if w.memo_p.contains(&i) {
        return;
    }
    place_inner(w, i);
    // Arranged by the full pass at show, which must not find it memoised.
    if unarranged_hidden(w, i) {
        return;
    }
    // A shown ToolTip is placed in window coordinates, which ancestors moving change too.
    let ks = kids(w, i);
    if ks.iter().all(|c| w.memo_p.contains(c))
        && !ks
            .iter()
            .any(|&c| is(w, c, "ToolTip") && w.read(c, "visible").is_some_and(|v| v.truthy()))
    {
        w.memo_p.insert(i);
    }
}

thread_local! {
    /// Items measured and items placed since the last `take_layout_work` (memoised ones are
    /// not counted: nothing is done for them).
    static LAYOUT_WORK: std::cell::Cell<(u64, u64)> = const { std::cell::Cell::new((0, 0)) };
}

/// (items measured, items placed) since the last call: the work of the layout, to hold against
/// the number of items that changed.
pub fn take_layout_work() -> (u64, u64) {
    LAYOUT_WORK.with(|c| c.replace((0, 0)))
}

/// Bottom-up implicit sizes.
fn measure_prof(w: &mut World, fonts: &mut Fonts, i: usize) {
    LAYOUT_WORK.with(|c| c.set((c.get().0 + 1, c.get().1)));
    thread_local! {
        static PROF: Option<std::cell::Cell<usize>> =
            qml::log_on!("ri::mprof").then(|| std::cell::Cell::new(0));
    }
    let Some(depth) = PROF.with(|p| p.as_ref().map(|d| d.replace(d.get() + 1))) else {
        measure_inner(w, fonts, i);
        w.text_dirty.remove(&i);
        return;
    };
    let t = std::time::Instant::now();
    measure_inner(w, fonts, i);
    w.text_dirty.remove(&i);
    PROF.with(|p| p.as_ref().map(|d| d.set(depth)));
    let us = t.elapsed().as_micros();
    if us > 2000 {
        tracing::debug!(target: "ri::mprof", "  mprof {depth} {i} {:?} {us}us", w.items[i].types.first());
    }
}

/// Installs what the engine asks the fonts for: `World::text_measure`, `World::font_method`
/// and `World::text_method`. A measure or a font method that arrives while the fonts are in use
/// (a measure pass that runs a binding) is left out, as that pass measures the item itself. A
/// text method called then (a layout pass that runs `onWidthChanged`) has to answer: it shapes
/// with its own set of the same fonts.
pub fn text_hooks(w: &mut World, fonts: &Rc<std::cell::RefCell<Fonts>>) {
    let f = fonts.clone();
    w.text_measure = Some(Box::new(move |w, i| {
        if let Ok(mut f) = f.try_borrow_mut() {
            measure_text(w, &mut f, i);
        }
    }));
    let f = fonts.clone();
    w.font_method = Some(Box::new(move |w, i, name, args| match f.try_borrow_mut() {
        Ok(mut f) => font_method(w, &mut f, i, name, args),
        Err(_) => Value::Undefined,
    }));
    let f = fonts.clone();
    let mut spare = Fonts::default();
    w.text_method = Some(Box::new(move |w, i, name, args| match f.try_borrow_mut() {
        Ok(mut f) => text_method(w, &mut f, i, name, args),
        Err(_) => text_method(w, &mut spare, i, name, args),
    }));
}

/// `World::text_measure`: a Text or TextMetrics measured on the spot, as QQuickText and
/// QQuickTextMetrics update their size synchronously when written; of an editor, whose text
/// is laid out at layout, only the `cursorRectangle` a read asks for.
pub fn measure_text(w: &mut World, fonts: &mut Fonts, i: usize) {
    if is_text_edit(w, i) {
        return edit_cursor_sync(w, fonts, i, false);
    }
    if is(w, i, "TextMetrics") {
        text_metrics(w, fonts, i);
    } else if is(w, i, "FontMetrics") {
        font_metrics(w, fonts, i);
    } else if w.text_unfinished.contains_key(&i) {
        // QQuickText::updateSize waits for componentComplete(), setHAlign does not.
        let text = match w.read(i, "text") {
            None | Some(Value::Undefined | Value::Null) => String::new(),
            Some(v) => v.to_js_string(),
        };
        return text_aligned(w, i, &text);
    } else {
        measure_prof(w, fonts, i);
    }
    w.text_dirty.remove(&i);
}

/// QQuickFontMetrics' properties: QFontMetricsF of the item's font, whose engine for common
/// characters is the font's own face.
fn font_metrics(w: &mut World, fonts: &mut Fonts, c: usize) {
    let Some(stack) = text_style(w, fonts, c).font else {
        return;
    };
    let Some(f) = stack.font(0) else {
        return;
    };
    let px = |v: i64| v as f64 / 64.0;
    let (ascent, descent, leading) = (px(f.ascent), px(f.descent), px(f.leading));
    let (underline, thickness) = f.underline();
    let (left, right) = f.min_bearings();
    for (k, v) in [
        ("ascent", ascent),
        ("descent", descent),
        ("height", ascent + descent),
        ("leading", leading),
        ("lineSpacing", leading + ascent + descent),
        ("minimumLeftBearing", left),
        ("minimumRightBearing", right),
        ("maximumCharacterWidth", f.max_char_width()),
        ("xHeight", f.x_height()),
        ("averageCharacterWidth", f.average_char_width()),
        ("underlinePosition", px(underline)),
        ("overlinePosition", ascent + 1.0),
        ("strikeOutPosition", ascent / 3.0),
        ("lineWidth", px(thickness)),
        ("capitalHeight", f.cap_height()),
    ] {
        set(w, c, k, v);
    }
}

/// `World::text_method`: QQuickTextEdit::positionToRectangle(pos), the rectangle of the
/// cursor before UTF-16 position `pos` in item coordinates. A position outside the text is
/// the start of the text: QTextCursor::setPosition refuses it and the cursor stays at 0.
pub fn text_method(
    w: &mut World,
    fonts: &mut Fonts,
    i: usize,
    name: &str,
    args: &[Value],
) -> Value {
    if name != "positionToRectangle" {
        return Value::Undefined;
    }
    let len = match w.read(i, "text").as_ref().and_then(Value::host_str) {
        Some(s) => s.encode_utf16().count(),
        None => 0,
    };
    let pos = args.first().map_or(0.0, |v| v.to_number());
    let pos = match pos >= 0.0 && pos <= len as f64 {
        true => pos as usize,
        false => 0,
    };
    qml::inputmethod::rect_value(edit_cursor_rect(w, fonts, i, pos).unwrap_or_default())
}

/// `World::font_method`: QQuickFontMetrics' advanceWidth(text), boundingRect(text),
/// tightBoundingRect(text) and elidedText(text, mode, width, flags).
pub fn font_method(
    w: &mut World,
    fonts: &mut Fonts,
    i: usize,
    name: &str,
    args: &[Value],
) -> Value {
    let Some(f) = text_style(w, fonts, i).font else {
        return Value::Undefined;
    };
    let text = match args.first() {
        None | Some(Value::Undefined | Value::Null) => String::new(),
        Some(v) => v.to_js_string(),
    };
    let rect = |(x, y, width, height): (f64, f64, f64, f64)| {
        let o = qml::eval::new_obj();
        for (k, v) in [("x", x), ("y", y), ("width", width), ("height", height)] {
            qml::eval::set_own(&o, k, Value::Num(v));
        }
        Value::Obj(o)
    };
    let arg = |k: usize| args.get(k).map_or(0.0, |v| v.to_number());
    match name {
        "advanceWidth" => Value::Num(f.advance_width(&text)),
        "boundingRect" => rect(f.bounding_rect(&text)),
        "tightBoundingRect" => rect(f.tight_bounding_rect(&text)),
        "elidedText" => Value::Str(
            f.elided_text(&text, arg(1) as i32, arg(2), arg(3) as u32)
                .into(),
        ),
        _ => Value::Undefined,
    }
}

/// `World::popup_measure`: a popup about to run its enter transition gets its implicit size
/// (and its width/height when not set), which Qt's bindings already hold.
pub fn measure_popup(w: &mut World, fonts: &mut Fonts, i: usize) {
    // Content a hidden popup just created (its Loader activates on `visible`) is not measured yet.
    let mut todo = vec![i];
    while let Some(x) = todo.pop() {
        w.memo_m.remove(&x);
        todo.extend(w.items[x].children.iter().copied());
    }
    measure(w, fonts, i);
    for (k, ik) in [("width", "implicitWidth"), ("height", "implicitHeight")] {
        if !explicit(w, i, k) {
            let v = num(w, i, ik);
            if v.is_finite() {
                set(w, i, k, v);
            }
        }
    }
}

fn text_metrics(w: &mut World, fonts: &mut Fonts, c: usize) {
    let ts = text_style(w, fonts, c);
    if let Some(f) = ts.font {
        let line = f.shape(&ts.text);
        set(w, c, "advanceWidth", line.advance as f64 / 64.0);
        // QQuickTextMetrics: width and height are boundingRect's.
        let (x, y, width, height) = f.bounding_rect(&ts.text);
        set(w, c, "width", width);
        set(w, c, "height", height);
        let now = [("x", x), ("y", y), ("width", width), ("height", height)];
        let old = match w.read(c, "boundingRect") {
            Some(Value::Obj(o)) => {
                let o = o.borrow();
                let get = |k: &str| o.props.iter().find(|p| &**p.0 == k);
                now.map(|(k, _)| get(k).map(|p| p.1.to_number()))
            }
            _ => [None; 4],
        };
        if old != now.map(|(_, v)| Some(v)) {
            let o = qml::eval::new_obj();
            for (k, v) in now {
                qml::eval::set_own(&o, k, Value::Num(v));
            }
            w.write(c, "boundingRect", Value::Obj(o));
        }
    }
}

/// QQuickTextPrivate::setupTextLayout's "elided and zero width or height": a plain or styled
/// Text that elides has no room for a line while its set width leaves none (or, eliding right,
/// its set height leaves none).
fn text_no_room(w: &mut World, i: usize) -> bool {
    let mode = opt_num(w, i, "elide").unwrap_or(3.0);
    if mode == 3.0
        || (explicit(w, i, "textFormat") && matches!(num(w, i, "textFormat") as i32, 1 | 3))
    {
        return false;
    }
    // An empty text takes QQuickTextPrivate::updateSize's own branch: one line, not truncated.
    if w.read(i, "text").is_none_or(|v| match v {
        Value::Undefined | Value::Null => true,
        v => v.to_js_string().is_empty(),
    }) {
        return false;
    }
    // widthValid / heightValid: a size somebody set, the item itself or what sizes it.
    let (mw, mh) = size_managed(w, i);
    let valid =
        |k: &str, managed: bool| explicit(w, i, k) || (managed && w.items[i].props.contains_key(k));
    let (vw, vh) = (valid("width", mw), valid("height", mh));
    let [l, t, r, b] = text_pad(w, i);
    vw && (num(w, i, "width") - l - r <= 0.0
        || (mode == 1.0 && vh && num(w, i, "height") - t - b <= 0.0))
}

/// What `text_lines_kept` keeps of a Text between layouts: the text and font last laid out in
/// full (Qt's implicitWidthValid), what the choice made without room depended on, that choice
/// (0 room, 1 no line, 2 in full), and whether the implicit size was ever used.
const TEXT_ROOM: [&str; 4] = ["\x01tvalid", "\x01tfor", "\x01tfull", "\x01tused"];

/// QQuickTextPrivate::requireImplicitSize, set by the first read of implicitWidth or
/// implicitHeight: here a read by script, a binding on either, or a parent that sizes itself
/// from them.
fn text_implicit_used(w: &mut World, i: usize) -> bool {
    if w.text_implicit_read.contains(&i)
        || ["implicitWidth", "implicitHeight"].iter().any(|k| {
            w.dependents
                .get(&(i, qml::runtime::Sym::new(k)))
                .is_some_and(|d| !d.is_empty())
        })
    {
        return true;
    }
    let Some(p) = w.items[i].parent else {
        return false;
    };
    linear_axis(w, p).is_some()
        || is(w, p, "GridLayout")
        || is(w, p, "StackLayout")
        || (is_loader(w, p) && matches!(w.read(p, "item"), Some(Value::Item(c)) if c == i))
        || (w.items[p].prop_info("background").is_some()
            && matches!(w.read(p, "contentItem"), Some(Value::Item(c)) if c == i))
}

/// Whether Text `i` keeps its laid-out lines: always with room for them. Without room Qt lays
/// out no line, unless the implicit size is used and is not valid for this text and font (the
/// text changed without room, or never had any): then it lays the text out in full to find
/// that size, and shows it so until the next change of text, eliding or room.
fn text_lines_kept(w: &mut World, i: usize, key: f64) -> bool {
    let [valid, made_for, choice, used] = TEXT_ROOM;
    if !text_no_room(w, i) {
        set(w, i, valid, key);
        set(w, i, choice, 0.0);
        return true;
    }
    let read = num(w, i, used) == 1.0 || text_implicit_used(w, i);
    if read {
        set(w, i, used, 1.0);
    }
    let now = text_key(&(
        key.to_bits(),
        read,
        opt_num(w, i, "elide").map(f64::to_bits),
    ));
    if num(w, i, choice) != 0.0 && num(w, i, made_for) == now {
        return num(w, i, choice) == 2.0;
    }
    let full = read && num(w, i, valid) != key;
    set(w, i, made_for, now);
    set(w, i, choice, if full { 2.0 } else { 1.0 });
    if full {
        set(w, i, valid, key);
    }
    full
}

/// A hash as a number a property can hold exactly.
fn text_key(of: &impl std::hash::Hash) -> f64 {
    use std::hash::Hasher;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    of.hash(&mut h);
    ((h.finish() >> 12) + 1) as f64
}

/// QQuickTextInput and QQuickTextEdit contentWidth/contentHeight: the size of the text the
/// editor laid out.
fn edit_content(w: &mut World, i: usize, cw: f64, ch: f64) {
    for (k, v) in [("contentWidth", cw), ("contentHeight", ch)] {
        if num(w, i, k) != v {
            set(w, i, k, v);
        }
    }
}

/// `lineCount` of a TextEdit: the number of the lines the editor laid out, one in an empty
/// editor. `edit_content` has their `contentWidth`: the widest line as the editor wraps it,
/// not rounded (QQuickTextEdit::updateSize: the idealWidth of its document).
fn edit_laid(w: &mut World, i: usize, lines: usize) {
    if is_text_doc(w, i) && num(w, i, "lineCount") != lines.max(1) as f64 {
        set(w, i, "lineCount", lines.max(1) as f64);
    }
}

/// The widest line of an editor's `placeholderText` in the editor's font, not wrapped and
/// not rounded: implicitWidth of the PlaceholderText of TextArea.qml and TextField.qml,
/// which is that with the editor empty or not.
pub fn placeholder_width(w: &mut World, fonts: &mut Fonts, i: usize) -> f64 {
    let Some(Value::Str(hint)) = w.read(i, "placeholderText") else {
        return 0.0;
    };
    let Some(f) = text_style(w, fonts, i).font else {
        return 0.0;
    };
    let widest = text_lines(&f, &hint)
        .iter()
        .map(|l| f.natural_width(l))
        .max();
    widest.unwrap_or(0) as f64 / 64.0
}

/// QQuickText contentWidth/contentHeight/paintedSize/lineCount of the laid-out lines.
fn text_content(w: &mut World, i: usize, cw: f64, ch: f64, n: usize) {
    for (k, v) in [
        ("contentWidth", cw),
        ("contentHeight", ch),
        ("paintedWidth", cw),
        ("paintedHeight", ch),
        ("lineCount", n as f64),
    ] {
        set(w, i, k, v);
    }
}

/// QQuickPopup's own contentItem (QQuickContentItem) when none is set: the popup's visual
/// children live in it, inside the padding.
fn popup_content_item(w: &mut World, i: usize) {
    if !qml::types::is_popup(&w.items[i].types)
        || matches!(w.read(i, "contentItem"), Some(Value::Item(_)))
    {
        return;
    }
    let bg = match w.read(i, "background") {
        Some(Value::Item(b)) => Some(b),
        _ => None,
    };
    let moved: Vec<usize> = visual_kids(w, i)
        .into_iter()
        .filter(|&c| Some(c) != bg && !is(w, c, "ToolTip") && !is_popup(w, c))
        .collect();
    let ci = w.new_inst("Item", Some(i));
    w.write(i, "contentItem", Value::Item(ci));
    for c in moved {
        w.reparent(c, ci);
    }
}

/// A Text as measured: the natural width and the start of each line it shows, `lineCount`, the content
/// size, the first line's ascent, `truncated`, and the implicit size without the padding.
struct TextMeasure {
    widths: Vec<f64>,
    starts: Vec<usize>,
    line_count: usize,
    content: (f64, f64),
    ascent: i64,
    truncated: bool,
    implicit: (f64, f64),
}

/// QQuickText::setHAlign as the text's direction or `LayoutMirroring` has it: the alignment
/// of a Text that sets none, and the one its lines get.
fn text_aligned(w: &mut World, i: usize, text: &str) {
    let (align, effective) = text_halign(w, i, text);
    let name = "effectiveHorizontalAlignment";
    if !explicit(w, i, "horizontalAlignment") {
        set(w, i, "horizontalAlignment", align as f64);
    } else if w.items[i].props.get(name).is_none() {
        // Set with the Text: QQuickText::setHAlign ran while the object was created, when
        // no handler of its signals was connected yet.
        w.write_unheard(i, name, Value::Num(effective as f64));
    }
    set(w, i, name, effective as f64);
}

/// `decide`: this is the item's layout, which chooses what a Text without room keeps
/// (`text_lines_kept`); other callers measure what that layout chose.
fn text_measured(w: &mut World, fonts: &mut Fonts, i: usize, decide: bool) -> Option<TextMeasure> {
    let ts = text_style(w, fonts, i);
    if decide {
        text_aligned(w, i, &ts.text);
    }
    let f = ts.font?;
    if decide {
        text_lines_kept(w, i, text_key(&(&ts.text, ts.font_key.to_bits())));
    }
    // Unwrapped first: the item wraps its lines only when it is narrower than they are.
    let natural_bounds = Bounds {
        max_lines: line_limit(w, i).filter(|_| !ts.doc),
        ..Bounds::NATURAL
    };
    let bounds_of = |w: &mut World, natural_width: f64| {
        let bounds = text_bounds(w, i, ts.doc, Some(natural_width));
        let natural = Bounds {
            width: 0.0,
            ..bounds
        } == natural_bounds;
        (!natural).then_some(bounds)
    };
    // QQuickTextPrivate::setupTextLayout's lineWidth: lines are aligned in the item's width,
    // or in that of the widest one.
    let align = text_halign(w, i, &ts.text).1;
    let [pl, _, pr, _] = text_pad(w, i);
    let line_width = Some(num(w, i, "width"))
        .filter(|&width| size_valid(w, i).0 && width > 0.0)
        .map(|width| width - pl - pr);
    let mut m = match &ts.spans {
        Some(spans) => {
            let natural = rich_laid(&f, spans, &natural_bounds);
            let bounds = bounds_of(w, natural.implicit_width);
            let laid = match &bounds {
                Some(bounds) => rich_laid(&f, spans, bounds),
                None => natural,
            };
            let content_width =
                laid.content_width(align, line_width.unwrap_or(laid.implicit_width));
            TextMeasure {
                widths: laid.lines.iter().map(|l| l.natural).collect(),
                starts: laid.starts,
                line_count: laid.lines.len(),
                content: (content_width, laid.lines.iter().map(|l| l.height()).sum()),
                ascent: laid.lines.first().map_or(0, |l| l.ascent),
                truncated: laid.truncated,
                implicit: (laid.implicit_width, laid.implicit_height),
            }
        }
        None => {
            let natural = plain_laid(&f, &ts.text, &natural_bounds);
            let bounds = bounds_of(w, natural.implicit_width);
            let laid = match &bounds {
                Some(bounds) => plain_laid(&f, &ts.text, bounds),
                None => natural,
            };
            let content_width =
                laid.content_width(align, line_width.unwrap_or(laid.implicit_width));
            TextMeasure {
                widths: laid
                    .spans
                    .iter()
                    .take(laid.lines.len())
                    .map(|s| s.1)
                    .collect(),
                starts: laid.starts,
                line_count: laid.lines.len(),
                content: (content_width, laid.lines.iter().map(|l| l.height()).sum()),
                ascent: laid.lines.first().map_or(0, |l| l.ascent),
                truncated: laid.truncated,
                implicit: (laid.implicit_width, laid.implicit_height),
            }
        }
    };
    if m.widths.is_empty() {
        // Elided with no room: no lines, and QQuickText reports the font's height.
        let line = f.shape("");
        (m.content.1, m.ascent) = (line.height(), line.ascent);
    }
    if ts.doc {
        // QQuickText does not count a QTextDocument's lines, and its content is as wide as
        // the document: the width it wraps at, or a wider line.
        m.line_count = 1;
        if num(w, i, "wrapMode") != 0.0 && size_valid(w, i).0 {
            let [pl, _, pr, _] = text_pad(w, i);
            m.content.0 = m.content.0.max(num(w, i, "width") - pl - pr);
        }
        m.widths.clear();
    }
    Some(m)
}

/// The natural width (QTextLine::naturalTextWidth) of each line a Text shows.
pub fn text_line_widths(w: &mut World, fonts: &mut Fonts, i: usize) -> Vec<f64> {
    text_measured(w, fonts, i, false).map_or(vec![], |m| m.widths)
}

/// A line's advance and its glyphs as (family, glyph, x, y), in 1/64 px from the line's left
/// end and baseline.
pub type LineGlyphs = (i64, Vec<(String, u32, i64, i64)>);

/// The lines a Text of plain text draws.
pub fn text_line_glyphs(w: &mut World, fonts: &mut Fonts, i: usize) -> Vec<LineGlyphs> {
    let ts = text_style(w, fonts, i);
    let Some(f) = ts.font.filter(|_| ts.spans.is_none()) else {
        return vec![];
    };
    plain_laid(&f, &ts.text, &text_bounds(w, i, ts.doc, None))
        .lines
        .iter()
        .map(|l| {
            let family = |font| f.font(font).map_or(String::new(), |f| f.family());
            let glyphs = l
                .glyphs
                .iter()
                .map(|&(font, g, x, y)| (family(font), g, x, y));
            (l.advance, glyphs.collect())
        })
        .collect()
}

/// Where each line a Text shows starts in its text, in bytes.
pub fn text_line_starts(w: &mut World, fonts: &mut Fonts, i: usize) -> Vec<usize> {
    text_measured(w, fonts, i, false).map_or(vec![], |m| m.starts)
}

fn measure_inner(w: &mut World, fonts: &mut Fonts, i: usize) {
    popup_content_item(w, i);
    scrollbar_handle(w, i);
    // A ScrollView that was not made by a QML creation gets its Flickable and bars here.
    if is(w, i, "ScrollView") && scroll_flickable(w, i).is_none() {
        w.scroll_view_complete(i);
    }
    for c in kids(w, i) {
        measure(w, fonts, c);
    }
    let ch = visual_kids(w, i);
    for c in w.items[i].children.clone() {
        if is(w, c, "TextMetrics") {
            text_metrics(w, fonts, c);
        }
    }
    let mut status_told = false;
    let (iw, ih) = if is_text(w, i) {
        match text_measured(w, fonts, i, true) {
            Some(m) => {
                text_content(w, i, m.content.0, m.content.1, m.line_count);
                if w.read(i, "truncated").is_some_and(|v| v.truthy()) != m.truncated {
                    w.write(i, "truncated", Value::Bool(m.truncated));
                }
                keep_text_base(w, i, m.ascent, m.content.1);
                // QQuickText implicitWidth is the unrounded natural width (e.g. 79.5).
                let [pl, pt, pr, pb] = text_pad(w, i);
                (m.implicit.0 + pl + pr, m.implicit.1 + pt + pb)
            }
            None => (0.0, 0.0),
        }
    } else if is_text_edit(w, i) && material_field(w, i) {
        // Material TextField: background implicit 120x56 below a topInset of half the placeholder
        // height when clipped.
        let (ts, _) = edit_style(w, fonts, i);
        let lh = ts.font.as_ref().map_or(0.0, |f| f.shape("").height());
        MAT_LH.with(|m| m.borrow_mut().insert(i, lh));
        let ins = if truthy(w, i, "clip", false) {
            lh / 2.0
        } else {
            0.0
        };
        set(w, i, "topInset", ins);
        let [l, t, r, b] = edit_padding(w, i);
        let ww = edit_wrap_width(w, i, l + r);
        let n = ts
            .font
            .as_ref()
            .map_or(1, |f| edit_lines(f, &ts.text, ww).len().max(1));
        let _ = (l, r);
        let dl = if is_text_doc(w, i) {
            ts.font.as_ref().map_or(lh, |f| f.shape("").doc_height())
        } else {
            lh
        };
        if let Some(f) = &ts.font {
            keep_text_base(w, i, f.shape("").ascent, lh * n as f64);
        }
        (120.0, (56.0 + ins).max(dl * n as f64 + t + b))
    } else if is_text_edit(w, i) {
        let (ts, placeholder) = edit_style(w, fonts, i);
        let [l, _, r, _] = edit_padding(w, i);
        if !explicit(w, i, "horizontalAlignment") {
            let ha = edit_halign(w, i);
            set(w, i, "horizontalAlignment", ha as f64);
        }
        match ts.font {
            Some(f) if ts.spans.is_some() => {
                let spans = ts.spans.unwrap();
                let natural = edit_rich_lines(w, i, &f, &spans, None);
                let iw = natural.iter().map(|l| l.natural).fold(0.0, f64::max);
                let ww = edit_wrap_width(w, i, l + r);
                let lines = edit_rich_lines(w, i, &f, &spans, ww.filter(|&x| x < iw.ceil()));
                let h: f64 = lines.iter().map(|l| l.height()).sum();
                let asc = lines
                    .first()
                    .map_or_else(|| f.shape("").ascent, |l| l.ascent);
                keep_text_base(w, i, asc, h);
                let cw = lines.iter().map(|l| l.natural).fold(0.0, f64::max);
                edit_laid(w, i, lines.len());
                edit_content(w, i, cw, h);
                field_size(w, fonts, i, iw, cw, h)
            }
            Some(f) => {
                let natural = text_lines(&f, &ts.text);
                let iw = natural
                    .iter()
                    .map(|l| f.natural_width(l))
                    .max()
                    .unwrap_or(0) as f64
                    / 64.0;
                let ww = edit_wrap_width(w, i, l + r).filter(|&x| x < iw.ceil());
                let lines = edit_lines(&f, &ts.text, ww);
                // The editor's own layout holds no text while the placeholder shows.
                let (cw, n) = match (placeholder, ww) {
                    (true, _) => (0, 1),
                    (false, Some(room)) => (edit_wrapped_width(&f, &ts.text, room), lines.len()),
                    (false, None) => (
                        lines.iter().map(|l| f.natural_width(l)).max().unwrap_or(0),
                        lines.len(),
                    ),
                };
                let cw = cw as f64 / 64.0;
                edit_laid(w, i, n);
                // The editor's own layout holds no text while the placeholder shows.
                match lines.first().filter(|_| !placeholder) {
                    Some(first) => {
                        let th = lines.iter().map(|l| l.height()).sum();
                        keep_text_base(w, i, first.ascent, th);
                    }
                    None => {
                        let empty = f.shape("");
                        keep_text_base(w, i, empty.ascent, empty.height());
                    }
                }
                // The placeholder is a plain Text item, not the QTextDocument: no leading.
                let doc = is_text_doc(w, i) && !placeholder;
                let mut h: f64 = lines
                    .iter()
                    .map(|l| if doc { l.doc_height() } else { l.height() })
                    .sum();
                if placeholder {
                    // The editor's own layout is one empty line; `field_size` adds the
                    // placeholder's.
                    h = f.shape("").height();
                }
                edit_content(w, i, cw, h);
                field_size(w, fonts, i, iw, cw, h)
            }
            None => field_size(w, fonts, i, 0.0, 0.0, 0.0),
        }
    } else if is(w, i, "Shape") {
        shape_bounds(w, i)
    } else if is(w, i, "ScreencopyView") {
        // A QSize nothing set is (-1, -1): no frame yet.
        screencopy_implicit(
            [
                num(w, i, "sourceSize.width").max(0.0),
                num(w, i, "sourceSize.height").max(0.0),
            ],
            [
                num(w, i, "constraintSize.width"),
                num(w, i, "constraintSize.height"),
            ],
        )
    } else if is_picture(w, i) {
        let was = num(w, i, "status");
        let size = picture_load(w, i, Load::Layout);
        status_told = num(w, i, "status") != was;
        size
    } else if let Some(o) = linear_axis(w, i) {
        let lin = linear_children(w, i, &ch);
        let kids: Vec<Child> = lin.iter().map(|x| x.1.clone()).collect();
        let cells = layout_cells(w, i, o, &lin);
        let h = layout::Grid {
            children: &kids,
            cells: &cells,
            spacing: [layout_spacing(w, i, 0), layout_spacing(w, i, 1)],
            uniform: [uniform_cells(w, i, 0), uniform_cells(w, i, 1)],
        }
        .size_hints();
        // A nested layout's min/max hints bound how far its parent layout may resize it.
        for o in 0..2 {
            set(w, i, LMIN[o], h[0][o]);
            set(w, i, LMAX[o], h[2][o]);
        }
        (h[1][0], h[1][1])
    } else if is(w, i, "Flow") || is_positioner(w, i) {
        positioner_place(w, i, &ch, false)
    } else if w.items[i].prop_info("contentItem").is_some()
        && w.items[i].prop_info("background").is_some()
    {
        // QQuickControl: max(implicitBackground + insets, implicitContent + padding), as each
        // file of the Basic style binds implicitWidth and implicitHeight.
        let side = |w: &mut World, _k: &str, s: &str| pad_side(w, i, s);
        let ci = if is(w, i, "TabBar") {
            // Basic TabBar contentItem: horizontal ListView sized to contentWidth/contentHeight.
            let spacing = prop_num(w, i, "spacing").unwrap_or(1.0);
            let tabs = tab_items(w, i, &ch);
            let (mut cw, mut chh) = (0.0f64, 0.0f64);
            for &c in &tabs {
                cw += if explicit(w, c, "width") {
                    num(w, c, "width")
                } else {
                    num(w, c, "implicitWidth")
                };
                chh = chh.max(num(w, c, "implicitHeight"));
            }
            [cw + tabs.len().saturating_sub(1) as f64 * spacing, chh]
        } else if is(w, i, "SwipeView") {
            // QQuickSwipeViewPrivate::getContentWidth/Height: the largest page.
            let pages: Vec<usize> = ch.iter().copied().filter(|&c| positioned(w, c)).collect();
            pages.into_iter().fold([0.0f64, 0.0f64], |a, c| {
                [
                    a[0].max(num(w, c, "implicitWidth")),
                    a[1].max(num(w, c, "implicitHeight")),
                ]
            })
        } else if let Some(ci) = menu_list(w, i) {
            let [iw, ih] = menu_content_size(w, ci);
            set(w, ci, "implicitWidth", iw);
            set(w, ci, "implicitHeight", ih);
            [iw, ih]
        } else if qml::types::inherits(&w.items[i].types, "QQuickPane") {
            pane_content(w, i)
        } else if !explicit(w, i, "contentItem")
            && qml::types::inherits(&w.items[i].types, "QQuickToolTip")
        {
            // The contentItem here is the engine's own (`popup_content_item`), not the Text
            // ToolTip.qml makes.
            style_part(w, fonts, i, "contentItem").unwrap_or([0.0, 0.0])
        } else if qml::types::is_popup(&w.items[i].types)
            && let Some(Value::Item(ci)) = w.read(i, "contentItem")
        {
            // QQuickPanePrivate::getContentWidth/Height: the contentItem's implicit size, else
            // its only child's.
            let only = match visual_kids(w, ci)[..] {
                [c] => Some(c),
                _ => None,
            };
            ["implicitWidth", "implicitHeight"].map(|k| {
                let v = num(w, ci, k);
                let v = match only {
                    Some(c) if v == 0.0 || !v.is_finite() => num(w, c, k),
                    _ => v,
                };
                if v.is_finite() { v } else { 0.0 }
            })
        } else {
            part_size(w, fonts, i, "contentItem")
        };
        style_kind(w, fonts, i);
        let bg = part_size(w, fonts, i, "background");
        let ind = part_size(w, fonts, i, "indicator");
        let hd = part_size(w, fonts, i, "handle");
        for (k, v) in [
            ("implicitIndicatorWidth", ind[0]),
            ("implicitIndicatorHeight", ind[1]),
            ("implicitHandleWidth", hd[0]),
            ("implicitHandleHeight", hd[1]),
        ] {
            if w.items[i].prop_info(k).is_some() {
                set(w, i, k, v);
            }
        }
        for (k, v) in [
            ("implicitContentWidth", ci[0]),
            ("implicitContentHeight", ci[1]),
            ("implicitBackgroundWidth", bg[0]),
            ("implicitBackgroundHeight", bg[1]),
        ] {
            set(w, i, k, v);
        }
        let pad = [
            side(w, "Padding", "left") + side(w, "Padding", "right"),
            side(w, "Padding", "top") + side(w, "Padding", "bottom"),
        ];
        let ins = [
            opt_num(w, i, "leftInset").unwrap_or(0.0) + opt_num(w, i, "rightInset").unwrap_or(0.0),
            opt_num(w, i, "topInset").unwrap_or(0.0) + opt_num(w, i, "bottomInset").unwrap_or(0.0),
        ];
        // Slider.qml binds its size to the handle's and not to a content's; CheckBox.qml,
        // RadioButton.qml, Switch.qml, ComboBox.qml and MenuItem.qml add `implicitIndicatorHeight
        // + topPadding + bottomPadding` to the heights.
        let slider = qml::types::inherits(&w.items[i].types, "QQuickSlider");
        let tall = [
            "QQuickCheckBox",
            "QQuickRadioButton",
            "QQuickSwitch",
            "QQuickComboBox",
            "QQuickMenuItem",
        ]
        .iter()
        .any(|t| qml::types::inherits(&w.items[i].types, t));
        let ci = if slider { hd } else { ci };
        let ih = if tall { ci[1].max(ind[1]) } else { ci[1] };
        (
            (bg[0] + ins[0]).max(ci[0] + pad[0]),
            (bg[1] + ins[1]).max(ih + pad[1]),
        )
    } else if is_loader(w, i) {
        // QQuickLoaderPrivate::getImplicitWidth/Height: the item's implicit size once the
        // Loader's own size is set (explicitly, by anchors or a layout), else the item's size.
        match w.items[i].props.get("item").map(|p| &p.value) {
            Some(&Value::Item(c)) => {
                let (vw, vh) = loader_size_valid(w, i);
                // An item without an explicit size is sized to its implicit size.
                let iw = vw || !explicit(w, c, "width");
                let ih = vh || !explicit(w, c, "height");
                (
                    num(w, c, if iw { "implicitWidth" } else { "width" }),
                    num(w, c, if ih { "implicitHeight" } else { "height" }),
                )
            }
            _ => (0.0, 0.0),
        }
    } else {
        (f64::NAN, f64::NAN)
    };
    take_implicit(w, i, iw, ih);
    if is_picture(w, i) {
        picture_sized(w, i, status_told);
    }
    baseline_offset(w, i);
}

/// Item `i` takes the implicit size its measure found, and that size as its own where nothing
/// else sets it.
fn take_implicit(w: &mut World, i: usize, iw: f64, ih: f64) {
    // QQuickLayoutPrivate::applySizeHints: a layout's setImplicitSize overrides a binding, but a
    // binding that re-evaluated since (`implicitWidth: view.implicitWidth` after a capture frame)
    // is the last writer until the next polish, when its children have followed it.
    let layout = linear_axis(w, i).is_some();
    for (o, (k, v)) in [("implicitWidth", iw), ("implicitHeight", ih)]
        .into_iter()
        .enumerate()
    {
        if !v.is_finite() || !layout && explicit(w, i, k) {
            continue;
        }
        if layout && explicit(w, i, k) {
            let cur = num(w, i, k);
            if opt_num(w, i, LIMP[o]).is_some_and(|l| l != cur) {
                set(w, i, LIMP[o], cur);
                continue;
            }
            set(w, i, LIMP[o], v);
        }
        set(w, i, k, v);
    }
    // Anchors own the size of the items they manage: resetting it to the
    // implicit size here would fight the placement pass on every frame.
    let (mw, mh) = size_managed(w, i);
    if !mw && !explicit(w, i, "width") {
        let v = num(w, i, "implicitWidth");
        set(w, i, "width", v);
    }
    if !mh && !explicit(w, i, "height") {
        let v = num(w, i, "implicitHeight");
        set(w, i, "height", v);
    }
}

/// An Image or AnimatedImage that loads a picture (a Kirigami.Icon is polished instead).
fn is_picture(w: &World, i: usize) -> bool {
    (is(w, i, "Image") || is(w, i, "AnimatedImage")) && !is_icon(w, i)
}

/// What makes a picture load.
#[derive(Clone, Copy, PartialEq)]
enum Load {
    /// Its componentComplete.
    Created,
    /// A layout pass.
    Layout,
    /// The end of the load it waited for.
    Answered,
}

/// The size picture `i` asks its image at follows a binding. Such a size is often that of the
/// item (StyledImage), which a layout gives it: until the first one it is not the size asked.
fn source_size_bound(w: &World, i: usize) -> bool {
    ["sourceSize", "sourceSize.width", "sourceSize.height"]
        .iter()
        .any(|k| {
            let p = w.items[i].props.get(k);
            p.is_some_and(|p| p.binding.is_some() || p.alias.is_some())
        })
}

/// QQuickImageBase::load for picture `i`: its status (Null, Ready, Loading or Error) and
/// progress; returns its implicit size, that of the loaded pixmap. statusChanged is withheld
/// (`World::write_withheld`) for the caller to emit once the size is set.
///
/// A created asynchronous picture whose `sourceSize` is bound only looks its image up: one
/// that is stored is Ready, one that is not is Loading and its file is read at the first
/// layout.
fn picture_load(w: &mut World, i: usize, why: Load) -> (f64, f64) {
    let (wd, ht) = (num(w, i, "width"), num(w, i, "height"));
    let (src, ss, fk, ratio) = image_src(w, i, wd, ht, 1.0);
    let key = (src, ss, fk);
    kept_only(image_loads(w), i, &key);
    // Its load may have ended, but its Behaviors run only once it has been laid out: it has
    // the answer after that ([`take_finished_images`]).
    let fresh = w.fresh.contains(&i);
    let begins = match waiting(i, &key) {
        Some(true) if fresh => return (0.0, 0.0),
        Some(started) => !started,
        None => false,
    };
    let (src, ..) = key;
    let assigned = w.sourced.remove(&i);
    remote_request(image_loads(w), i, &src, assigned);
    let asynchronous = truthy(w, i, "asynchronous", false);
    // `cache` is on unless something sets it.
    let own = explicit(w, i, "cache") && !truthy(w, i, "cache", true);
    let ask = Ask {
        asynchronous,
        own,
        laid_out: !fresh,
    };
    let lookup = why == Load::Created && asynchronous && !src.is_empty() && source_size_bound(w, i);
    let mut started = true;
    let tex = if lookup {
        let key = (src.clone(), ss, fk);
        let stored = IMAGES.with_borrow(|c| c.peek(&key).is_some_and(|t| t.is_some()));
        started = stored || decode_pending(image_loads(w), &src);
        stored
            .then(|| picture_texture(image_loads(w), i, &src, ss, fk, ask))
            .flatten()
    } else {
        match animated_texture(w, i, &src, ss, asynchronous) {
            Some(t) => t,
            None if src.is_empty() => None,
            None => picture_texture(image_loads(w), i, &src, ss, fk, ask),
        }
    };
    // What began to load at this first layout and is stored by now ends after the layout.
    let tex = tex.filter(|_| !begins);
    // A Ready Image keeps its pixmap while another picture has the same file decoded again:
    // only a request of its own loads it anew.
    let request = format!("{src}\u{1}{ss:?}\u{1}{fk}");
    let kept = tex.is_none()
        && !assigned
        && !begins
        && is(w, i, "Image")
        && num(w, i, "status") == 1.0
        && w.images.get(&i).is_some_and(|s| s.request == request)
        && decode_pending(image_loads(w), &src);
    let status = match (&tex, src.is_empty()) {
        _ if kept => 1.0,
        (_, true) => 0.0,
        (Some(_), _) => 1.0,
        (None, _)
            if !started
                || begins
                || is_remote(&src) && remote_pending(image_loads(w), &src)
                || decode_pending(image_loads(w), &src)
                || own_pending(i) =>
        {
            2.0
        }
        (None, _) => 3.0,
    };
    let key = (src, ss, fk);
    waits(i, (status == 2.0).then_some(key.clone()), started);
    if fresh && (own_pending(i) || begins) {
        owe(i);
    } else if begins {
        let _ = image_loads(w).tx.send(Loaded::Own(i));
    }
    // QQuickImageBase::status starts at Null: no load is no change of it.
    let cur = w.items[i].props.get("status").map(|p| p.value.to_number());
    if cur.unwrap_or(0.0) != status {
        w.write_withheld(i, "status", Value::Num(status));
    } else if assigned && status == 2.0 && is(w, i, "Image") {
        // QQuickImageBase::loadPixmap of a new source emits statusChanged for Loading also
        // over a load that has not ended.
        w.notify(i, "status");
    }
    set(w, i, "progress", if status == 1.0 { 1.0 } else { 0.0 });
    let pix = tex.map(|t| (t.w as f64 / ratio, t.h as f64 / ratio));
    if !is(w, i, "Image") {
        return pix.unwrap_or((0.0, 0.0));
    }
    // While the new pixmap loads the old one stays (`pendingPix` beside `currentPix`).
    if status == 2.0 {
        let retain = truthy(w, i, "retainWhileLoading", false);
        current_retained(image_loads(w), i, retain);
    } else if !kept {
        current_is(image_loads(w), i, pix.map(|_| key));
    }
    let held = w.images.entry(i).or_default();
    if status != 2.0 && !kept {
        held.pix = pix.unwrap_or((0.0, 0.0));
        held.request = request;
    }
    let pix = held.pix;
    if status != 2.0 && w.image_ends(i) {
        picture_resized(w, i, pix);
    }
    pix
}

/// A picture that is Loading.
struct Wait {
    key: ImageKey,
    /// Numbers the pictures in the order they began to load
    /// (QQuickPixmapReply::finished reaches the pictures of one file in that order).
    seq: u64,
    /// Its answer waits for its first layout ([`clear_fresh`]).
    owed: bool,
    /// Its file is asked for; else it is at the first layout ([`picture_load`]).
    started: bool,
}

thread_local! {
    static WAITING: std::cell::RefCell<(u64, HashMap<usize, Wait>)> = Default::default();
}

/// Picture `i` is Loading the image of `key`, or (`None`) no longer loads.
fn waits(i: usize, key: Option<ImageKey>, started: bool) {
    WAITING.with_borrow_mut(|(n, m)| match key {
        None => {
            m.remove(&i);
        }
        Some(key) => match m.get_mut(&i).filter(|l| l.key == key) {
            Some(l) => l.started |= started,
            None => {
                *n += 1;
                let (seq, owed) = (*n, false);
                let wait = Wait {
                    key,
                    seq,
                    owed,
                    started,
                };
                m.insert(i, wait);
            }
        },
    });
}

/// Whether picture `i` is Loading the image of `key`, and then whether its file is asked for.
fn waiting(i: usize, key: &ImageKey) -> Option<bool> {
    WAITING.with_borrow(|(_, m)| m.get(&i).filter(|l| l.key == *key).map(|l| l.started))
}

/// The answer for picture `i` is given once it has been laid out.
fn owe(i: usize) {
    WAITING.with_borrow_mut(|(_, m)| {
        if let Some(l) = m.get_mut(&i) {
            l.owed = true;
        }
    });
}

/// Picture `i` has been laid out: the answer it is owed leaves now.
fn picture_laid_out(w: &World, i: usize) {
    let owed = WAITING
        .with_borrow_mut(|(_, m)| m.get_mut(&i).is_some_and(|l| std::mem::take(&mut l.owed)));
    if owed {
        let _ = image_loads(w).tx.send(Loaded::Own(i));
    }
}

/// QQuickImageBase::requestFinished of picture `i` once its size is set: the painted geometry,
/// then statusChanged, which the load withheld, then for an Image the end of the load
/// (sourceSizeChanged comes after statusChanged). `told` is whether the status changed.
fn picture_sized(w: &mut World, i: usize, told: bool) {
    picture_painted(w, i);
    w.release_notify((i, qml::runtime::Sym::new("status")));
    if is(w, i, "Image") {
        w.image_measured(i, told);
    }
}

/// QQuickImage::updatePaintedGeometry of picture `i` whose implicit size is set: told
/// whether or not it changed when a load ends here.
fn picture_painted(w: &mut World, i: usize) {
    if is(w, i, "Image") {
        let pix = w.images.get(&i).map_or((0.0, 0.0), |s| s.pix);
        let ends = w.image_ends(i);
        image_painted(w, i, pix, ends);
    }
}

/// QQuickItem::setImplicitSize of Image `i` as a load ends with a pixmap of `pix`: the size
/// follows (geometryChange, which updates the painted geometry) before the implicit size's
/// own signals.
fn picture_resized(w: &mut World, i: usize, pix: (f64, f64)) {
    let (mw, mh) = size_managed(w, i);
    let mut resized = false;
    for (managed, k, ik, v) in [
        (mw, "width", "implicitWidth", pix.0),
        (mh, "height", "implicitHeight", pix.1),
    ] {
        if managed || explicit(w, i, k) {
            continue;
        }
        let v = if explicit(w, i, ik) { num(w, i, ik) } else { v };
        if num(w, i, k) != v {
            set(w, i, k, v);
            resized = true;
        }
    }
    if resized {
        image_painted(w, i, pix, true);
    }
}

/// QQuickImage::updatePaintedGeometry: `paintedWidth` and `paintedHeight` of the pixmap in
/// the item's size, told (paintedGeometryChanged) when `always` or when they changed.
fn image_painted(w: &mut World, i: usize, pix: (f64, f64), always: bool) {
    let (wd, ht) = (num(w, i, "width"), num(w, i, "height"));
    let painted = match num(w, i, "fillMode") as i32 {
        // PreserveAspectFit and PreserveAspectCrop of no pixmap leave what was painted.
        1 | 2 if pix.0 == 0.0 || pix.1 == 0.0 => return,
        1 => {
            let (mw, mh) = size_managed(w, i);
            let wd = if mw || explicit(w, i, "width") {
                wd
            } else {
                pix.0
            };
            let ht = if mh || explicit(w, i, "height") {
                ht
            } else {
                pix.1
            };
            let (ws, hs) = (wd / pix.0, ht / pix.1);
            if ws <= hs {
                (wd, ws * pix.1)
            } else {
                (hs * pix.0, ht)
            }
        }
        2 => {
            let scale = (wd / pix.0).max(ht / pix.1);
            (scale * pix.0, scale * pix.1)
        }
        _ => (wd, ht),
    };
    let was = (num(w, i, "paintedWidth"), num(w, i, "paintedHeight"));
    set(w, i, "paintedWidth", painted.0);
    set(w, i, "paintedHeight", painted.1);
    if always || was != painted {
        w.notify(i, "paintedGeometry");
    }
}

/// Quickshell ScreencopyView's bImplicitSize binding: the captured size scaled to fit
/// `constraintSize` (QSizeF::scale KeepAspectRatio), including its odd one-axis formulas.
fn screencopy_implicit(src: [f64; 2], con: [f64; 2]) -> (f64, f64) {
    let [sw, sh] = src.map(|v| if v.is_finite() { v } else { 0.0 });
    let [cw, ch] = con.map(|v| if v.is_finite() { v } else { 0.0 });
    if cw != 0.0 && ch != 0.0 {
        if sw == 0.0 || sh == 0.0 {
            return (cw, ch);
        }
        let rw = ch * sw / sh;
        return if rw <= cw {
            (rw, ch)
        } else {
            (cw, cw * sh / sw)
        };
    }
    if cw != 0.0 {
        (cw, sh / cw)
    } else if ch != 0.0 {
        (sw / ch, ch)
    } else {
        (sw, sh)
    }
}

/// The tabs a QQuickTabBar keeps in its contentModel (its ListView contentItem's delegates).
fn tab_items(w: &mut World, i: usize, ch: &[usize]) -> Vec<usize> {
    let bg = match w.read(i, "background") {
        Some(Value::Item(b)) => Some(b),
        _ => None,
    };
    ch.iter()
        .copied()
        .filter(|&c| Some(c) != bg && !is(w, c, "Repeater") && positioned(w, c))
        .collect()
}

/// Whether the Loader `i` has a set width and height (QQuickItem::widthValid/heightValid):
/// set explicitly, by anchors or by a layout.
fn loader_size_valid(w: &mut World, i: usize) -> (bool, bool) {
    // A layout only calls setSize (setting widthValid) when its result differs from the
    // Loader's size, i.e. in practice for fill-sized cells.
    let in_layout = w.items[i]
        .parent
        .is_some_and(|p| linear_axis(w, p).is_some() || is(w, p, "GridLayout"));
    let (mw, mh) = if in_layout {
        (
            w.read(i, "Layout.fillWidth").is_some_and(|v| v.truthy()),
            w.read(i, "Layout.fillHeight").is_some_and(|v| v.truthy()),
        )
    } else {
        size_managed(w, i)
    };
    (
        mw || explicit(w, i, "width"),
        mh || explicit(w, i, "height"),
    )
}

/// `p` is a Loader with a set size on both axes and `i` its loaded item.
fn loader_sizes_item(w: &mut World, p: usize, i: usize) -> bool {
    if !is_loader(w, p) || !matches!(w.read(p, "item"), Some(Value::Item(c)) if c == i) {
        return false;
    }
    let (mw, mh) = size_managed(w, p);
    (mw || explicit(w, p, "width")) && (mh || explicit(w, p, "height"))
}

fn size_managed(w: &mut World, i: usize) -> (bool, bool) {
    if matches!(w.read(i, "anchors.fill"), Some(Value::Item(_))) {
        return (true, true);
    }
    // QQuickControl::resizeContent/resizeBackground size these from the control's geometry.
    if let Some(p) = w.items[i].parent
        && ["contentItem", "background"]
            .iter()
            .any(|k| matches!(w.read(p, k), Some(Value::Item(c)) if c == i && c != p))
        && w.items[p].prop_info("background").is_some()
    {
        let bg = matches!(w.read(p, "background"), Some(Value::Item(c)) if c == i);
        return if bg {
            if bg_anchored(w, i) {
                return (false, false);
            }
            (!explicit(w, i, "width"), !explicit(w, i, "height"))
        } else {
            (true, true)
        };
    }
    // QQuickScrollBarAttachedPrivate lays out the bar of a Flickable, Basic ScrollView.qml the
    // bars a ScrollView made.
    if let Some(p) = w.items[i].parent
        && attached_scrollbar(w, p, i)
        && (is_flickable(w, p) || w.items[i].props.contains_key(SCROLL_MADE))
    {
        return (true, true);
    }
    // QQuickTextAreaPrivate::resizeFlickableControl sizes the TextArea of a ScrollView.
    if let Some(f) = area_flickable(w, i)
        && scroll_text_area(w, f) == Some(i)
    {
        return (true, true);
    }
    // QQuickLayout owns the geometry of its visible children; writing the implicit size here
    // flips it against the layout's (pixel-snapped) result on every pass.
    if let Some(p) = w.items[i].parent
        && (linear_axis(w, p).is_some() || is(w, p, "GridLayout") || is(w, p, "SwipeView"))
        && truthy(w, i, "visible", true)
    {
        return (true, true);
    }
    // QQuickAnchorsPrivate::update{Horizontal,Vertical}Anchors bail out under centerIn.
    if matches!(w.read(i, "anchors.centerIn"), Some(Value::Item(_))) {
        return (false, false);
    }
    // QQuickLoaderPrivate::_q_updateSize: a Loader with a set size sizes its item.
    if let Some(p) = w.items[i].parent
        && is_loader(w, p)
        && matches!(w.read(p, "item"), Some(Value::Item(c)) if c == i)
    {
        let (pw, ph) = size_managed(w, p);
        return (
            pw || explicit(w, p, "width"),
            ph || explicit(w, p, "height"),
        );
    }
    // QQuickListView sizes its highlight to the current item.
    if let Some(p) = w.items[i].parent
        && w.view_decor
            .get(&p)
            .is_some_and(|d| d[2].is_some_and(|h| h.1 == i))
    {
        return (true, true);
    }
    let has = |w: &mut World, k: &str| anchor_ref(w, i, k).is_some();
    // Two anchors of an axis give the size: both edges, or an edge and the centre.
    let two = |w: &mut World, a: &str, b: &str, m: &str| {
        (has(w, a) as u8 + has(w, b) as u8 + has(w, m) as u8) >= 2
    };
    (
        two(w, "left", "right", "horizontalCenter"),
        two(w, "top", "bottom", "verticalCenter"),
    )
}

/// Grid/GridLayout flowing as a single row (`columns: -1`/`rows: 1`) or column (`columns: 1`).
fn prop_num(w: &World, i: usize, name: &str) -> Option<f64> {
    w.items[i]
        .props
        .get(name)
        .map(|p| p.value.to_number())
        .filter(|x| x.is_finite())
}

/// The cell of each child of layout `i`: one row or column along `o` for Row/ColumnLayout,
/// QQuickGridLayout::insertLayoutItems for GridLayout (children it cannot place get no cell).
fn layout_cells(w: &mut World, i: usize, o: usize, lin: &[(usize, Child)]) -> Vec<layout::Cell> {
    if !is(w, i, "GridLayout") {
        return (0..lin.len())
            .map(|k| {
                let mut pos = [0; 2];
                pos[o] = k;
                layout::Cell { pos, span: [1, 1] }
            })
            .collect();
    }
    let bound = |n: &str| prop_num(w, i, n).filter(|v| *v >= 0.0).map(|v| v as usize);
    let bound = [bound("columns"), bound("rows")];
    let flow = if prop_num(w, i, "flow") == Some(1.0) {
        1
    } else {
        0
    };
    let items: Vec<_> = lin
        .iter()
        .map(|&(c, _)| {
            let mut at = |n: &str| opt_num(w, c, n).map(|v| v as i32);
            let (col, row) = (at("Layout.column"), at("Layout.row"));
            let set = (col.is_some_and(|v| v >= 0) || row.is_some_and(|v| v >= 0))
                .then(|| [col.unwrap_or(0), row.unwrap_or(0)]);
            let span = [
                at("Layout.columnSpan").unwrap_or(1),
                at("Layout.rowSpan").unwrap_or(1),
            ];
            (set, span)
        })
        .collect();
    layout::grid_cells(&items, bound, flow)
}

/// RowLayout/ColumnLayout `uniformCellSizes`, or `uniformCellWidths`/`uniformCellHeights`.
fn uniform_cells(w: &mut World, i: usize, o: usize) -> bool {
    let along = if o == 0 {
        "uniformCellWidths"
    } else {
        "uniformCellHeights"
    };
    truthy(w, i, "uniformCellSizes", false) || truthy(w, i, along, false)
}

fn layout_spacing(w: &World, i: usize, o: usize) -> f64 {
    let grid = is(w, i, "GridLayout") || is(w, i, "Grid");
    // Qt Quick Layouts default to 5; positioners (Row, Column, Grid, Flow) to 0.
    let def = if ["RowLayout", "ColumnLayout", "GridLayout"]
        .iter()
        .any(|t| is(w, i, t))
    {
        5.0
    } else {
        0.0
    };
    if grid {
        let name = if o == 0 {
            "columnSpacing"
        } else {
            "rowSpacing"
        };
        // A Grid's own gap counts once it was written (QQuickGrid::m_useRowSpacing).
        let written = !is(w, i, "Grid")
            || qml::runtime::grid_gap_mark(name).is_some_and(|m| w.items[i].props.contains_key(m));
        prop_num(w, i, name)
            .filter(|_| written)
            .or_else(|| prop_num(w, i, "spacing"))
            .unwrap_or(def)
    } else {
        prop_num(w, i, "spacing").unwrap_or(def)
    }
}

/// `layoutDirection: Qt.RightToLeft` (Qt mirrors the final geometry, margins included).
fn mirrored(w: &mut World, i: usize) -> bool {
    opt_num(w, i, "layoutDirection") == Some(1.0)
}

/// A RowLayout or GridLayout laid out right to left.
fn layout_mirrored(w: &mut World, i: usize) -> bool {
    (is(w, i, "GridLayout") || is(w, i, "RowLayout")) && mirrored(w, i)
}

pub(crate) fn linear_axis(w: &World, i: usize) -> Option<usize> {
    if is(w, i, "GridLayout") || is(w, i, "RowLayout") {
        Some(0)
    } else if is(w, i, "ColumnLayout") {
        Some(1)
    } else {
        None
    }
}

/// The padding a Row or Column last moved its child by, `[left, top]`.
const PPAD: [&str; 2] = ["\x01ppadl", "\x01ppadt"];

/// Padding of positioner `i` on `side`: `leftPadding` and the like, else `padding`.
fn positioner_pad(w: &mut World, i: usize, side: &str) -> f64 {
    opt_num(w, i, &format!("{side}Padding"))
        .or_else(|| opt_num(w, i, "padding"))
        .unwrap_or(0.0)
}

/// QQuickRow/Column/Grid/Flow::doPositioning: the content size of positioner `i`; with `apply`
/// its children are moved to their places.
fn positioner_place(w: &mut World, i: usize, ch: &[usize], apply: bool) -> (f64, f64) {
    let items: Vec<usize> = ch
        .iter()
        .copied()
        .filter(|&c| in_positioner(w, c))
        .collect();
    let sz: Vec<[f64; 2]> = items
        .iter()
        .map(|&c| [num(w, c, "width"), num(w, c, "height")])
        .collect();
    let [l, t, r, b] = ["left", "top", "right", "bottom"].map(|s| positioner_pad(w, i, s));
    let (flow, grid, row) = (is(w, i, "Flow"), is(w, i, "Grid"), is(w, i, "Row"));
    let rtl = (flow || grid || row) && mirrored(w, i);
    // widthValid()/heightValid(): set explicitly, by anchors or by a layout.
    let managed = size_managed(w, i);
    let valid = [
        explicit(w, i, "width") || managed.0,
        explicit(w, i, "height") || managed.1,
    ];
    let own = [num(w, i, "width"), num(w, i, "height")];
    // Right to left, items start at the right padding and are flipped at the end.
    let (h1, h2) = if rtl { (r, l) } else { (l, r) };
    let n = items.len();
    let mut at: Vec<[Option<f64>; 2]> = vec![[None; 2]; n];
    let (mut cw, mut chh);
    if flow {
        // Wraps only at a valid size; otherwise one line that grows to fit.
        let o = if num(w, i, "flow") == 1.0 { 1 } else { 0 };
        let spacing = opt_num(w, i, "spacing").unwrap_or(0.0);
        let (mut ho, mut vo, mut line) = (h1, t, 0.0f64);
        (cw, chh) = (h1 + h2, t + b);
        for k in 0..n {
            let [iw, ih] = sz[k];
            if o == 0 {
                if valid[0] && own[0] > 0.0 && ho != h1 && ho + iw + h2 > own[0] {
                    ho = h1;
                    vo += line + spacing;
                    line = 0.0;
                }
            } else if valid[1] && own[1] > 0.0 && vo != t && vo + ih + b > own[1] {
                vo = t;
                ho += line + spacing;
                line = 0.0;
            }
            at[k] = [Some(ho), Some(vo)];
            cw = cw.max(ho + iw + h2);
            chh = chh.max(vo + ih + b);
            if o == 0 {
                ho += iw + spacing;
                line = line.max(ih);
            } else {
                vo += ih + spacing;
                line = line.max(iw);
            }
        }
    } else if grid {
        let count = |w: &World, k: &str| prop_num(w, i, k).filter(|v| *v > 0.0).map(|v| v as usize);
        let (cols, rows) = match (count(w, "columns"), count(w, "rows")) {
            (None, None) => (4, n.div_ceil(4)),
            (Some(c), None) => (c, n.div_ceil(c)),
            (None, Some(r)) => (n.div_ceil(r), r),
            (Some(c), Some(r)) => (c, r),
        };
        if rows == 0 || cols == 0 {
            return (l + r, t + b);
        }
        let top_to_bottom = prop_num(w, i, "flow") == Some(1.0);
        // The cell of the k-th item; items past rows * columns are not placed.
        let cell = |k: usize| {
            if top_to_bottom {
                (k / rows, k % rows)
            } else {
                (k % cols, k / cols)
            }
        };
        let (mut widths, mut heights) = (vec![0.0f64; cols], vec![0.0f64; rows]);
        let placed = n.min(rows * cols);
        for (k, s) in sz.iter().enumerate().take(placed) {
            let (c, r) = cell(k);
            widths[c] = widths[c].max(s[0]);
            heights[r] = heights[r].max(s[1]);
        }
        let spacing = [layout_spacing(w, i, 0), layout_spacing(w, i, 1)];
        cw = widths.iter().sum::<f64>() + spacing[0] * (cols - 1) as f64 + l + r;
        chh = heights.iter().sum::<f64>() + spacing[1] * (rows - 1) as f64 + t + b;
        // QQuickGrid keeps the right edge in an int.
        let end = (if valid[0] { own[0] } else { cw }).trunc();
        // Grid.AlignRight = 2, AlignHCenter = 4; AlignBottom = 0x40, AlignVCenter = 0x80.
        let halign = prop_num(w, i, "horizontalItemAlignment").unwrap_or(1.0) as u32;
        let valign = prop_num(w, i, "verticalItemAlignment").unwrap_or(32.0) as u32;
        // Left and right swap when laid out right to left.
        let right = if rtl { halign == 1 } else { halign == 2 };
        for k in 0..placed {
            let (c, row) = cell(k);
            let before = widths[..c].iter().sum::<f64>() + spacing[0] * c as f64;
            let mut x = if rtl {
                end - r - before - widths[c]
            } else {
                l + before
            };
            if right {
                x += widths[c] - sz[k][0];
            } else if halign == 4 {
                x += (widths[c] - sz[k][0]) / 2.0;
            }
            let mut y = t + heights[..row].iter().sum::<f64>() + spacing[1] * row as f64;
            if valign == 0x80 {
                y += (heights[row] - sz[k][1]) / 2.0;
            } else if valign == 0x40 {
                y += heights[row] - sz[k][1];
            }
            at[k] = [Some(x), Some(y)];
        }
    } else {
        // Row along x, Column along y; the other coordinate stays the child's own, moved by
        // the padding.
        let o = if row { 0 } else { 1 };
        let spacing = layout_spacing(w, i, o);
        let (start, stop, across) = if row { (h1, h2, t + b) } else { (t, b, l + r) };
        let (mut p, mut cross) = (start, across);
        for k in 0..n {
            at[k][o] = Some(p);
            cross = cross.max(sz[k][1 - o] + across);
            p += sz[k][o] + spacing;
        }
        if p != start {
            p -= spacing;
        }
        (cw, chh) = if row {
            (p + stop, cross)
        } else {
            (cross, p + stop)
        };
        if apply {
            let (pad, coord) = if row { (t, "y") } else { (l, "x") };
            for &c in &items {
                let was = opt_num(w, c, PPAD[1 - o]).unwrap_or(0.0);
                if was != pad {
                    let v = num(w, c, coord) + pad - was;
                    set(w, c, coord, v);
                    set(w, c, PPAD[1 - o], pad);
                }
            }
        }
    }
    if rtl && !grid {
        let end = if valid[0] { own[0] } else { cw };
        for k in 0..n {
            if let Some(x) = at[k][0] {
                at[k][0] = Some(end - x - sz[k][0]);
            }
        }
    }
    if apply {
        for (k, &c) in items.iter().enumerate() {
            if let Some(x) = at[k][0] {
                set(w, c, "x", x);
            }
            if let Some(y) = at[k][1] {
                set(w, c, "y", y);
            }
        }
    }
    (cw, chh)
}

fn is_positioner(w: &World, i: usize) -> bool {
    ["Grid", "Row", "Column"].iter().any(|t| is(w, i, t))
}

fn linear_children(w: &mut World, i: usize, ch: &[usize]) -> Vec<(usize, Child)> {
    let rtl = layout_mirrored(w, i);
    let mut out = vec![];
    for &c in ch {
        if !positioned(w, c) {
            continue;
        }
        let mut a = Attached::new();
        let names = [("minimum", 0usize), ("preferred", 1), ("maximum", 2)];
        for (n, k) in names {
            for (o, d) in [(0usize, "Width"), (1, "Height")] {
                let v = opt_num(w, c, &format!("Layout.{n}{d}")).filter(|v| *v >= 0.0);
                match k {
                    0 => a.min[o] = v,
                    1 => a.pref[o] = v,
                    _ => a.max[o] = v,
                }
            }
        }
        let m = opt_num(w, c, "Layout.margins").unwrap_or(0.0);
        for (k, n) in ["left", "top", "right", "bottom"].iter().enumerate() {
            a.margins[k] = opt_num(w, c, &format!("Layout.{n}Margin")).unwrap_or(m);
        }
        if rtl {
            a.margins.swap(0, 2);
        }
        let nested = linear_axis(w, c).is_some();
        if nested {
            for o in 0..2 {
                if let Some(v) = opt_num(w, c, LMIN[o]) {
                    a.implicit_min[o] = v;
                }
                if let Some(v) = opt_num(w, c, LMAX[o]) {
                    a.implicit_max[o] = v;
                }
            }
        }
        let imp = [num(w, c, "implicitWidth"), num(w, c, "implicitHeight")];
        // The fallback is the item's own size only when set explicitly: the size this layout
        // assigned on the previous pass must not feed back into its preferred size.
        let fb = [
            if explicit(w, c, "width") {
                num(w, c, "width")
            } else {
                imp[0]
            },
            if explicit(w, c, "height") {
                num(w, c, "height")
            } else {
                imp[1]
            },
        ];
        let hints = layout::effective_size_hints(&a, imp, fb);
        let fill = |w: &mut World, n: &str| truthy(w, c, n, nested);
        let pol = |f: bool| {
            if f { layout::PREFERRED } else { layout::FIXED }
        };
        let policy = [
            pol(fill(w, "Layout.fillWidth")),
            pol(fill(w, "Layout.fillHeight")),
        ];
        let st = |w: &mut World, n: &str| opt_num(w, c, n).map(|v| v as i32).unwrap_or(-1);
        let stretch = [
            st(w, "Layout.horizontalStretchFactor"),
            st(w, "Layout.verticalStretchFactor"),
        ];
        let align = opt_num(w, c, "Layout.alignment").unwrap_or(0.0) as u32;
        out.push((
            c,
            Child {
                hints,
                policy,
                stretch,
                align,
                margins: a.margins,
            },
        ));
    }
    out
}

/// Whether reading `name` of `i` can only fall through to the type default (never an object):
/// nothing assigned, aliased or grouped under it, no group/proxy object, not tracking reads.
fn plainly_unset(w: &mut World, i: usize, name: &str) -> bool {
    let Some(it) = w.items.get(i) else {
        return false;
    };
    w.reading.is_none()
        && !it.props.contains_key(name)
        && !w.groups.contains_key(&i)
        && !w.has_grouped(i, name)
}

fn anchor_ref(w: &mut World, c: usize, name: &str) -> Option<(usize, String)> {
    let key: std::borrow::Cow<str> = match name {
        "left" => "anchors.left".into(),
        "right" => "anchors.right".into(),
        "top" => "anchors.top".into(),
        "bottom" => "anchors.bottom".into(),
        "horizontalCenter" => "anchors.horizontalCenter".into(),
        "verticalCenter" => "anchors.verticalCenter".into(),
        "baseline" => "anchors.baseline".into(),
        _ => format!("anchors.{name}").into(),
    };
    if plainly_unset(w, c, &key) && !w.items[c].props.contains_key("anchors") {
        return None;
    }
    let (it, line) = qml::runtime::anchor_line_of(&w.read(c, &key)?)?;
    Some((it, line.to_string()))
}

/// Rect of `t` in the coordinate space of `parent`, as its child `c` anchors to it.
fn rect_in(w: &mut World, t: usize, parent: usize) -> [f64; 4] {
    let (wd, ht) = (num(w, t, "width"), num(w, t, "height"));
    if t == parent {
        [0.0, 0.0, wd, ht]
    } else {
        [num(w, t, "x"), num(w, t, "y"), wd, ht]
    }
}

/// Position of anchor line `line` of `t`, whose rect in the anchoring parent is `r`.
fn line_pos(w: &mut World, t: usize, r: [f64; 4], line: &str) -> f64 {
    match line {
        "left" => r[0],
        "right" => r[0] + r[2],
        "horizontalCenter" => r[0] + r[2] / 2.0,
        "top" => r[1],
        "baseline" => r[1] + baseline_offset(w, t),
        "bottom" => r[1] + r[3],
        _ => r[1] + r[3] / 2.0,
    }
}

/// QQuickAnchors `hcenter()`/`vcenter()`: with `alignWhenCentered` (default) an odd integral
/// part rounds the half size up, so centred items land on whole pixels.
fn qt_half(w: &mut World, i: usize, v: f64) -> f64 {
    if matches!(
        w.read(i, "anchors.alignWhenCentered"),
        Some(Value::Bool(false))
    ) {
        return v / 2.0;
    }
    // qquickanchors.cpp hcenter()/vcenter(): qRound(width / 2).
    (v / 2.0).round()
}

fn margin(w: &mut World, c: usize, side: &str) -> f64 {
    opt_num(w, c, &format!("anchors.{side}Margin"))
        .or_else(|| opt_num(w, c, "anchors.margins"))
        .unwrap_or(0.0)
}

/// No anchor is declared on `c`: every `anchors.*` read would yield its default.
fn no_anchors(w: &mut World, c: usize) -> bool {
    w.items
        .get(c)
        .is_some_and(|it| !it.props.contains_key("anchors"))
        && w.reading.is_none()
        && !w.groups.contains_key(&c)
        && !w.has_grouped(c, "anchors")
}

/// Anchor lines of axis `ax` (0 horizontal, 1 vertical): near edge, far edge, centre.
const AXIS_LINES: [[&str; 3]; 2] = [
    ["left", "right", "horizontalCenter"],
    ["top", "bottom", "verticalCenter"],
];

fn anchor_item(w: &mut World, c: usize, key: &str) -> Option<usize> {
    match w.read(c, key) {
        Some(Value::Item(t)) => Some(t),
        _ => None,
    }
}

/// Items `c`'s geometry on axis `ax` follows: its fill/centerIn target, else its anchor lines'.
fn axis_targets(w: &mut World, c: usize, ax: usize) -> Vec<usize> {
    if no_anchors(w, c) {
        return Vec::new();
    }
    if let Some(t) =
        anchor_item(w, c, "anchors.fill").or_else(|| anchor_item(w, c, "anchors.centerIn"))
    {
        return vec![t];
    }
    let mut out: Vec<usize> = AXIS_LINES[ax]
        .iter()
        .chain((ax == 1).then_some(&"baseline"))
        .filter_map(|l| anchor_ref(w, c, l).map(|(t, _)| t))
        .collect();
    out.dedup();
    out
}

/// QQuickAnchorsPrivate::updateHorizontalAnchors/updateVerticalAnchors (and fillChanged,
/// centerInChanged for their axis): place `c` on axis `ax` (0 x/width, 1 y/height) from the
/// current geometry of its anchor targets. One a running Transition holds (`anchor_holds`)
/// goes between where its old anchor lines had it and where the new ones put it.
fn apply_axis(w: &mut World, parent: usize, c: usize, ax: usize) {
    if no_anchors(w, c) {
        return;
    }
    // QQuickPopupPositioner::reposition places a popup that is open, and centers it in its
    // parent only ("Popup can only be centered within its immediate parent or
    // Overlay.overlay"): otherwise it stays where it is.
    if qml::types::is_popup(&w.items[c].types)
        && !(truthy(w, c, "visible", false)
            && anchor_item(w, c, "anchors.centerIn") == Some(parent))
    {
        return;
    }
    let (pos, size) = [("x", "width"), ("y", "height")][ax];
    let cur = (num(w, c, pos), num(w, c, size));
    let mut placed = axis_geom(w, parent, c, ax);
    if w.anchor_holds.get(&c).is_some_and(|h| h.axes[ax]) {
        let at = |g: (Option<f64>, Option<f64>)| (g.0.unwrap_or(cur.0), g.1.unwrap_or(cur.1));
        let to = at(placed);
        if w.anchor_holds[&c].from[ax].is_none() {
            let old = w.anchor_holds[&c].old.clone();
            let new = swap_lines(w, c, old);
            let from = at(axis_geom(w, parent, c, ax));
            swap_lines(w, c, new);
            w.anchor_holds.get_mut(&c).unwrap().from[ax] = Some(from);
        }
        let hold = w.anchor_holds.get_mut(&c).unwrap();
        let (from, to) = (hold.from[ax].unwrap(), *hold.to[ax].get_or_insert(to));
        let k = hold.k;
        let mix = |a: f64, b: f64| if k >= 1.0 { b } else { a + (b - a) * k };
        placed = (Some(mix(from.0, to.0)), Some(mix(from.1, to.1)));
    }
    if let Some(p) = placed.0 {
        set(w, c, pos, p);
    }
    if let Some(s) = placed.1 {
        // QQuickAnchorsPrivate::setItemWidth/Height is QQuickItem::setWidth/Height: the size
        // is a set one from then on, as one a script assigned, and stays when the anchor goes.
        w.assign((c, qml::runtime::Sym::new(size)));
        set(w, c, size, s);
    }
}

/// Puts `lines` (anchor line properties and their values) on `c` without telling anyone, and
/// returns what was there: how the layout pass reads where other anchor lines would place it.
fn swap_lines(
    w: &mut World,
    c: usize,
    lines: Vec<(&'static str, Value)>,
) -> Vec<(&'static str, Value)> {
    let mut was = Vec::with_capacity(lines.len());
    for (name, v) in lines {
        // An AnchorChanges assignment cleared the binding of what it changed; the other lines
        // are current, and the same in both sets.
        w.read(c, name);
        let Some(p) = w.items[c].props.get_mut(name) else {
            continue;
        };
        was.push((name, std::mem::replace(&mut p.value, v)));
    }
    was
}

/// Position and size anchoring gives `c` on axis `ax`; `None` where it leaves them alone.
fn axis_geom(w: &mut World, parent: usize, c: usize, ax: usize) -> (Option<f64>, Option<f64>) {
    let size = ["width", "height"][ax];
    let [a, b, mid] = AXIS_LINES[ax];
    if let Some(t) = anchor_item(w, c, "anchors.fill") {
        let r = rect_in(w, t, parent);
        let (ma, mb) = (margin(w, c, a), margin(w, c, b));
        return (Some(r[ax] + ma), Some(r[ax + 2] - ma - mb));
    }
    let off = ["horizontalCenterOffset", "verticalCenterOffset"][ax];
    if let Some(t) = anchor_item(w, c, "anchors.centerIn") {
        let r = rect_in(w, t, parent);
        let sz = num(w, c, size);
        let o = num(w, c, &format!("anchors.{off}"));
        // A popup's rectangle gets its centre there (`rect.moveCenter`), on a half pixel too.
        let hc = match qml::types::is_popup(&w.items[c].types) {
            true => sz / 2.0,
            false => qt_half(w, c, sz),
        };
        return (Some(r[ax] + qt_half(w, t, r[ax + 2]) - hc + o), None);
    }
    let ra = anchor_ref(w, c, a).map(|(t, l)| {
        let r = rect_in(w, t, parent);
        line_pos(w, t, r, &l)
    });
    let rb = anchor_ref(w, c, b).map(|(t, l)| {
        let r = rect_in(w, t, parent);
        line_pos(w, t, r, &l)
    });
    let rm = anchor_ref(w, c, mid).map(|(t, l)| {
        let r = rect_in(w, t, parent);
        match l.as_str() {
            "horizontalCenter" => r[0] + qt_half(w, t, r[2]),
            "verticalCenter" => r[1] + qt_half(w, t, r[3]),
            _ => line_pos(w, t, r, &l),
        }
    });
    let sz = num(w, c, size);
    match (ra, rb) {
        (Some(p), Some(q)) => {
            let (ma, mb) = (margin(w, c, a), margin(w, c, b));
            // QQuickAnchorsPrivate::setItemWidth/Height: crossed anchors give a negative size.
            (Some(p + ma), Some(q - mb - p - ma))
        }
        (Some(p), None) => {
            let ma = margin(w, c, a);
            // With the centre anchored too, the size is twice the near edge's distance to it.
            let size = rm.map(|m| {
                let o = num(w, c, &format!("anchors.{off}"));
                (m + o - p - ma) * 2.0
            });
            (Some(p + ma), size)
        }
        (None, Some(q)) => {
            let mb = margin(w, c, b);
            let size = rm.map(|m| {
                let o = num(w, c, &format!("anchors.{off}"));
                (q - mb - m - o) * 2.0
            });
            (Some(q - mb - size.unwrap_or(sz)), size)
        }
        (None, None) => {
            if let Some(m) = rm {
                let o = num(w, c, &format!("anchors.{off}"));
                let h = qt_half(w, c, sz);
                (Some(m - h + o), None)
            } else if ax == 1
                && let Some((t, l)) = anchor_ref(w, c, "baseline")
            {
                // QQuickAnchorsPrivate::updateVerticalAnchors: the baseline anchor applies
                // only without top/bottom/verticalCenter.
                let r = rect_in(w, t, parent);
                let line = line_pos(w, t, r, &l);
                let own = baseline_offset(w, c);
                let o = num(w, c, "anchors.baselineOffset");
                (Some(line - own + o), None)
            } else {
                (None, None)
            }
        }
    }
}

fn geom4(w: &mut World, c: usize) -> [f64; 4] {
    [
        num(w, c, "x"),
        num(w, c, "y"),
        num(w, c, "width"),
        num(w, c, "height"),
    ]
}

/// Anchoring of a set of siblings, resolved like QQuickAnchors: each item is placed after the
/// siblings it anchors to (depth-first over the anchor graph of each axis), and a geometry
/// change re-places the already placed items that follow it, as Qt's geometry-change listeners
/// do. Only anchor cycles reach those; Qt's re-entrancy limits end a real loop with its warning.
struct Anchoring {
    parent: usize,
    items: Vec<usize>,
    /// Per axis: for each of `items`, the indices of the siblings in `items` it anchors to.
    deps: [Vec<Vec<usize>>; 2],
    /// Per axis: for each of `items`, the indices of the siblings anchored to it.
    rdeps: [Vec<Vec<usize>>; 2],
    /// Whether `fill`/`centerIn` positions the item (Qt's lower re-entrancy limit applies).
    whole: Vec<bool>,
    done: [Vec<bool>; 2],
    depth: [Vec<u8>; 2],
}

impl Anchoring {
    fn new(w: &mut World, parent: usize, items: Vec<usize>) -> Self {
        let at: HashMap<usize, usize> = items.iter().enumerate().map(|(k, &c)| (c, k)).collect();
        let n = items.len();
        let mut deps = [vec![Vec::new(); n], vec![Vec::new(); n]];
        let mut rdeps = [vec![Vec::new(); n], vec![Vec::new(); n]];
        let mut whole = vec![false; n];
        for (k, &c) in items.iter().enumerate() {
            if no_anchors(w, c) {
                continue;
            }
            whole[k] = anchor_item(w, c, "anchors.fill").is_some()
                || anchor_item(w, c, "anchors.centerIn").is_some();
            for ax in 0..2 {
                for t in axis_targets(w, c, ax) {
                    if let Some(&j) = at.get(&t)
                        && !deps[ax][k].contains(&j)
                    {
                        deps[ax][k].push(j);
                        rdeps[ax][j].push(k);
                    }
                }
            }
        }
        Anchoring {
            parent,
            items,
            deps,
            rdeps,
            whole,
            done: [vec![false; n], vec![false; n]],
            depth: [vec![0; n], vec![0; n]],
        }
    }

    /// Items in `items` that anchor to `i`, directly or through other siblings, `i` first.
    fn followers(w: &mut World, parent: usize, i: usize, items: &[usize]) -> Vec<usize> {
        let a = Anchoring::new(w, parent, items.to_vec());
        let Some(start) = items.iter().position(|&c| c == i) else {
            return vec![i];
        };
        let mut seen = vec![false; items.len()];
        seen[start] = true;
        let mut stack = vec![start];
        while let Some(k) = stack.pop() {
            for ax in 0..2 {
                for &d in &a.rdeps[ax][k] {
                    if !seen[d] {
                        seen[d] = true;
                        stack.push(d);
                    }
                }
            }
        }
        let mut out = vec![i];
        out.extend(
            items
                .iter()
                .zip(&seen)
                .filter(|&(&c, &s)| s && c != i)
                .map(|(&c, _)| c),
        );
        out
    }

    /// Place every item: per axis, each after the siblings it anchors to.
    fn run(&mut self, w: &mut World) {
        let n = self.items.len();
        let mut order = Vec::with_capacity(2 * n);
        for ax in 0..2 {
            let mut mark = vec![0u8; n];
            for k in 0..n {
                self.post_order(ax, k, &mut mark, &mut order);
            }
        }
        for (ax, k) in order {
            self.settle(w, ax, k);
        }
    }

    fn post_order(&self, ax: usize, k: usize, mark: &mut [u8], out: &mut Vec<(usize, usize)>) {
        // 1: on the DFS path (an edge back to it closes a cycle, left to `settle`); 2: emitted.
        if mark[k] != 0 {
            return;
        }
        mark[k] = 1;
        let mut stack = vec![(k, 0usize)];
        while let Some((c, e)) = stack.last_mut() {
            let c = *c;
            if let Some(&t) = self.deps[ax][c].get(*e) {
                *e += 1;
                if mark[t] == 0 {
                    mark[t] = 1;
                    stack.push((t, 0));
                }
            } else {
                mark[c] = 2;
                out.push((ax, c));
                stack.pop();
            }
        }
    }

    fn settle(&mut self, w: &mut World, ax: usize, k: usize) {
        // qquickanchors.cpp: updatingHorizontalAnchor/updatingVerticalAnchor < 3,
        // updatingFill/updatingCenterIn < 2.
        let limit = if self.whole[k] { 2 } else { 3 };
        let c = self.items[k];
        if self.depth[ax][k] >= limit {
            let what = if anchor_item(w, c, "anchors.fill").is_some() {
                "fill"
            } else if anchor_item(w, c, "anchors.centerIn").is_some() {
                "centerIn"
            } else {
                ["horizontal anchor", "vertical anchor"][ax]
            };
            let key = (c, qml::runtime::Sym::new(&format!("anchors.\x01loop{ax}")));
            if w.loop_reported.insert(key) {
                let ty = w.items[c].types.first().map_or("Item", |t| t.as_str());
                tracing::warn!(target: "ri::anchors", "{ty} {c}: Possible anchor loop detected on {what}.");
            }
            return;
        }
        self.depth[ax][k] += 1;
        self.done[ax][k] = true;
        let before = geom4(w, c);
        apply_axis(w, self.parent, c, ax);
        let after = geom4(w, c);
        for a in 0..2 {
            if before[a] == after[a] && before[a + 2] == after[a + 2] {
                continue;
            }
            // Own size changed on the other axis (a binding across axes): its anchors there
            // follow it, as QQuickAnchors' own geometry listener does.
            if a != ax && self.done[a][k] {
                self.settle(w, a, k);
            }
            for d in self.rdeps[a][k].clone() {
                if self.done[a][d] {
                    self.settle(w, a, d);
                }
            }
        }
        self.depth[ax][k] -= 1;
    }
}

/// Anchor `items`, children of `parent`, in dependency order.
fn anchor_children(w: &mut World, parent: usize, items: Vec<usize>) {
    Anchoring::new(w, parent, items).run(w);
}

/// Top-down placement of `i`'s children.
/// QQuickControlPrivate::resizeBackground: the background spans the control minus its insets;
/// an explicit background size stands unless an inset on that axis is set.
fn resize_background(w: &mut World, i: usize, bg: usize, pw: f64, ph: f64) {
    let inset = |w: &mut World, s: &str| opt_num(w, i, &format!("{s}Inset")).unwrap_or(0.0);
    let (l, r, t, b) = (
        inset(w, "left"),
        inset(w, "right"),
        inset(w, "top"),
        inset(w, "bottom"),
    );
    set(w, bg, "x", l);
    set(w, bg, "y", t);
    if !explicit(w, bg, "width") || l != 0.0 || r != 0.0 {
        set(w, bg, "width", (pw - l - r).max(0.0));
    }
    if !explicit(w, bg, "height") || t != 0.0 || b != 0.0 {
        set(w, bg, "height", (ph - t - b).max(0.0));
    }
}

fn place_inner(w: &mut World, i: usize) {
    LAYOUT_WORK.with(|c| c.set((c.get().0, c.get().1 + 1)));
    // The parent just sized `i`: an aligned text's baseline follows its height.
    baseline_offset(w, i);
    let ch = visual_kids(w, i);
    let (pw, ph) = (num(w, i, "width"), num(w, i, "height"));
    if unarranged_hidden(w, i) {
        // Layouts and positioners arrange in updatePolish, which Qt runs only for a shown window.
    } else if let Some(o) = linear_axis(w, i) {
        let lin = linear_children(w, i, &ch);
        let children: Vec<Child> = lin.iter().map(|x| x.1.clone()).collect();
        let cells = layout_cells(w, i, o, &lin);
        let rects = layout::Grid {
            children: &children,
            cells: &cells,
            spacing: [layout_spacing(w, i, 0), layout_spacing(w, i, 1)],
            uniform: [uniform_cells(w, i, 0), uniform_cells(w, i, 1)],
        }
        .arrange([pw, ph]);
        let rtl = layout_mirrored(w, i);
        let mut arranged = vec![(i, [0.0, 0.0, pw, ph])];
        arranged.extend(lin.iter().zip(rects).map(|((c, _), r)| {
            let x = if rtl { pw - r[0] - r[2] } else { r[0] };
            (*c, [x, r[1], r[2], r[3]])
        }));
        // QQuickLayout::rearrange runs when the layout is invalidated or resized: until then a
        // child dragged by a MouseArea (`drag.target`) stays where the drag put it.
        let again = w.layout_arranged.get(&i) != Some(&arranged);
        for &(c, r) in &arranged[1..] {
            // A Flow re-wraps its children at the new width, so its implicitHeight changes too.
            if num(w, c, "width") != r[2]
                && ((is_text(w, c) && num(w, c, "wrapMode") != 0.0) || is(w, c, "Flow"))
            {
                WRAP_RESIZED.with(|f| f.set(true));
            }
            if again {
                w.layout_dragged.remove(&c);
            }
            if !w.layout_dragged.contains(&c) {
                set(w, c, "x", r[0]);
                set(w, c, "y", r[1]);
            }
            set(w, c, "width", r[2]);
            set(w, c, "height", r[3]);
        }
        if again {
            w.layout_arranged.insert(i, arranged);
        }
    } else if is(w, i, "Flow") {
        positioner_place(w, i, &ch, true);
    } else if is_positioner(w, i) {
        // A child sized by anchors (`anchors.left/right: parent.left/right`) only gets its size
        // here: placed before, a fresh one would be skipped as zero-sized and its siblings
        // stacked over it (QQuickBasePositioner positions after anchors resolve).
        anchor_children(w, i, ch.clone());
        let (iw, ih) = positioner_place(w, i, &ch, true);
        // The children measured differently than at placement: like Qt, which sets the
        // positioner's implicit size as it positions, measure again in another pass.
        let stale = |w: &mut World, k: &str, v: f64| !explicit(w, i, k) && num(w, i, k) != v;
        if stale(w, "implicitWidth", iw) || stale(w, "implicitHeight", ih) {
            WRAP_RESIZED.with(|f| f.set(true));
        }
    } else if is(w, i, "ListView") || is(w, i, "GridView") {
        let dels: Vec<usize> = w.delegated.get(&i).map(|d| d.1.clone()).unwrap_or_default();
        let grid = is(w, i, "GridView");
        // ListView.Horizontal = 1; GridView.FlowLeftToRight = 0.
        let horiz = !grid && num(w, i, "orientation") == 1.0;
        let spacing = if grid { 0.0 } else { num(w, i, "spacing") };
        let (cw, chh) = (num(w, i, "cellWidth"), num(w, i, "cellHeight"));
        let cols = if grid && cw > 0.0 {
            (pw / cw).floor().max(1.0)
        } else {
            1.0
        };
        // ListView.BottomToTop = 3: the first delegate ends at 0, the next ones go up from it.
        let reversed = !grid && !horiz && num(w, i, "verticalLayoutDirection") == 3.0;
        let up = |w: &mut World, c: usize, p: f64| {
            if reversed {
                -(p + num(w, c, "height"))
            } else {
                p
            }
        };
        // A ListView's rows start at its first realized row (`ViewRows`): back from the row
        // whose position is known by the sizes of the rows before it.
        let (first, start) = match w.view_rows.get(&i) {
            Some(v) if !grid => {
                let lead: Vec<usize> = (v.rows.iter())
                    .take(v.anchor.saturating_sub(v.first))
                    .flatten()
                    .copied()
                    .collect();
                let (first, mut start) = (v.first, v.pos);
                for c in lead {
                    if positioned(w, c) {
                        start -= num(w, c, if horiz { "width" } else { "height" }) + spacing;
                    }
                }
                if let Some(v) = w.view_rows.get_mut(&i) {
                    (v.anchor, v.pos) = (first, start);
                }
                (first as f64, start)
            }
            _ => (0.0, 0.0),
        };
        let (mut p, mut extent) = (start, start);
        let now = w.now;
        let mut disp = (w.view_displace.remove(&i)).filter(|d| d.at.is_none_or(|t| t == now));
        if let Some(d) = &mut disp {
            d.at = Some(now);
        }
        // QQuickItemView: after an insert/remove, the delegates past it that move run the
        // displaced transition towards their new position; one already moving is left to its
        // transition, unless this frame started it towards another place.
        let mut place = |w: &mut World, c: usize, x: Option<f64>, y: Option<f64>| {
            let (cx, cy) = (num(w, c, "x"), num(w, c, "y"));
            let (nx, ny) = (x.unwrap_or(cx), y.unwrap_or(cy));
            if w.view_moving.contains_key(&c) {
                if let Some(d) = &mut disp
                    && d.dest.get(&c).is_some_and(|&to| to != (nx, ny))
                {
                    d.dest.insert(c, (nx, ny));
                    w.start_view_transition(
                        d.trans,
                        qml::runtime::ViewTrans::Move,
                        c,
                        Some((nx, ny)),
                    );
                }
                return;
            }
            if let Some(d) = &mut disp
                && d.rows.contains(&c)
                && !w.view_fresh.contains(&c)
                && (nx != cx || ny != cy)
            {
                d.dest.insert(c, (nx, ny));
                w.start_view_transition(d.trans, qml::runtime::ViewTrans::Move, c, Some((nx, ny)));
                return;
            }
            if let Some(x) = x {
                set(w, c, "x", x);
            }
            if let Some(y) = y {
                set(w, c, "y", y);
            }
        };
        // InlineLabels sections: a header sits right before the delegate it heads (no spacing).
        let heads: HashMap<usize, usize> = w
            .view_sections
            .get(&i)
            .map(|v| v.iter().copied().collect())
            .unwrap_or_default();
        for (k, &c) in dels.iter().enumerate() {
            if !positioned(w, c) {
                continue;
            }
            if let Some(&h) = heads.get(&c).filter(|_| !grid) {
                if horiz {
                    place(w, h, Some(p), None);
                    p += num(w, h, "width");
                } else {
                    let y = up(w, h, p);
                    place(w, h, None, Some(y));
                    p += num(w, h, "height");
                }
            }
            if grid {
                let (col, row) = ((k as f64) % cols, ((k as f64) / cols).floor());
                place(w, c, Some(col * cw), Some(row * chh));
                extent = (row + 1.0) * chh;
            } else if horiz {
                place(w, c, Some(p), None);
                p += num(w, c, "width") + spacing;
                extent = p - spacing;
            } else {
                let y = up(w, c, p);
                place(w, c, None, Some(y));
                p += num(w, c, "height") + spacing;
                extent = p - spacing;
            }
        }
        for c in &dels {
            w.view_fresh.remove(c);
        }
        let k = if horiz {
            "contentWidth"
        } else {
            "contentHeight"
        };
        let n = dels.len() as f64;
        // Unrealized delegates are estimated at the realized ones' average size, as
        // QQuickListView does for its content size.
        let total = w.view_window.get(&i).map_or(n, |v| (v.1 as f64).max(n));
        // Only this view reads its extent (to realize more delegates): it alone is read again.
        if (w.view_extent.get(&i)).is_none_or(|e| e.0 != dels || e.1 != extent) {
            w.view_extent.insert(i, (dels.clone(), extent));
            w.stamp(i);
            if qml::debug::on(qml::debug::Switch::NODELEGLOCAL) {
                w.deleg_gen += 1;
            }
        }
        // QQuickListViewPrivate::originPosition, lastPosition: the rows before the first
        // realized one and after the last, at the average size (updateAverage rounds it).
        let step = if n > 0.0 && !grid {
            ((extent - start - (n - 1.0) * spacing) / n).round() + spacing
        } else {
            0.0
        };
        let head = if grid { 0.0 } else { start - first * step };
        let est = if total > n && n > 0.0 && !grid {
            extent + (total - first - n).max(0.0) * step
        } else {
            extent
        };
        // currentItem outside the rows (QQuickListViewPrivate::positionAt): before them it
        // counts with its own size, the rows between at the average.
        let held = w.view_rows.get_mut(&i).and_then(|v| {
            let h = v.held?;
            let sig = (v.first, v.rows.len(), h.0, step);
            (v.held_placed.replace(sig) != Some(sig)).then_some(h)
        });
        if let Some((row, c, _)) = held
            && n > 0.0
            && positioned(w, c)
        {
            let row = row as f64;
            let at = if row < first {
                let own = num(w, c, if horiz { "width" } else { "height" });
                start - (first - row - 1.0) * step - (own + spacing)
            } else {
                extent + spacing + (row - first - n) * step
            };
            if horiz {
                place(w, c, Some(at), None);
            } else {
                let y = up(w, c, at);
                place(w, c, None, Some(y));
            }
        }
        if let Some(d) = disp {
            w.view_displace.insert(i, d);
        }
        // QQuickListView, QQuickGridView: the header sits before the first delegate, the
        // footer after the last; both are part of the content, which starts at the header.
        let decor: [Option<usize>; 3] = match w.view_decor.get(&i) {
            Some(d) => d.map(|x| x.map(|x| x.1)),
            _ => [None; 3],
        };
        let (at, len) = if horiz {
            ("x", "width")
        } else {
            ("y", "height")
        };
        let est = est.max(head);
        let hs = decor[0].map_or(0.0, |h| num(w, h, len));
        let fs = decor[1].map_or(0.0, |f| num(w, f, len));
        if let Some(h) = decor[0] {
            set(w, h, at, if reversed { -head } else { head - hs });
        }
        if let Some(f) = decor[1] {
            set(w, f, at, if reversed { -(est + fs) } else { est });
        }
        if !explicit(w, i, k) {
            set(w, i, k, est - head + hs + fs);
        }
        {
            let origin = if reversed { -(est + fs) } else { head - hs };
            set(w, i, if horiz { "originX" } else { "originY" }, origin);
        }
        {
            // QQuickItemViewPrivate::contentStartOffset: the view rests at its start, the
            // header's edge less the margin before it, or the bottom when reversed. A margin
            // set later leaves the view where it is.
            let (before, after) = if horiz {
                ("leftMargin", "rightMargin")
            } else {
                ("topMargin", "bottomMargin")
            };
            let (start, margin) = if reversed {
                (hs - num(w, i, len), num(w, i, after))
            } else {
                (-hs, -num(w, i, before))
            };
            let was = opt_num(w, i, VSTART);
            if was != Some(start) {
                let k = if horiz { "contentX" } else { "contentY" };
                let rests = match was {
                    Some(was) => num(w, i, k) == was + margin,
                    None => num(w, i, k) == 0.0,
                };
                if rests && start + margin != num(w, i, k) && !w.flick_moving(i) {
                    let animating = std::mem::replace(&mut w.animating, true);
                    let after = w.view_after.contains(&i);
                    let following = w.view_track.get(&i).is_some_and(|t| t.following);
                    set(w, i, k, start + margin);
                    // Not a scroll: the rows keep the cache on both sides and the view goes on
                    // following its current row.
                    if !after {
                        w.view_after.remove(&i);
                    }
                    if let Some(t) = w.view_track.get_mut(&i) {
                        t.following = following;
                    }
                    w.animating = animating;
                }
                set(w, i, VSTART, start);
            }
        }
        // A view holding row changes for its polish already counts its model's rows.
        if num(w, i, "count") != total && !w.polish_held.contains(&i) {
            set(w, i, "count", total);
        }
        // positionViewAtIndex on a row that had no delegate: it has one now and its place.
        if let Some((row, mode, at)) = w.view_goto.remove(&i)
            && let Some(c) = w.view_rows.get(&i).and_then(|v| v.item(row))
        {
            let (y, h, size) = (num(w, c, "y"), num(w, c, "height"), num(w, i, "height"));
            let (lo, hi) = w.flick_extents(i, true);
            let to = qml::runtime::view_position(mode, y, h, at, size)
                .min(-hi)
                .max(-lo);
            let was = std::mem::replace(&mut w.animating, true);
            set(w, i, "contentY", to);
            w.animating = was;
        }
        // QQuickItemView lays out the rows inserted before `positionViewAtEnd` and then
        // positions; here they are sized by this layout, which takes the view to their end.
        if let Some(&(last, at)) = w.view_at_end.get(&i) {
            let (min, max) = w.flick_extents(i, !horiz);
            let k = if horiz { "contentX" } else { "contentY" };
            if !at.is_nan() && num(w, i, k) != at {
                w.view_at_end.remove(&i);
            } else if max != last {
                // The end margin stays out of sight (QQuickItemViewPrivate::positionViewAtIndex).
                let after = if horiz { "rightMargin" } else { "bottomMargin" };
                let end = (-max.min(min) - num(w, i, after)).max(-min);
                let was = std::mem::replace(&mut w.animating, true);
                set(w, i, k, end);
                w.animating = was;
                w.view_at_end.insert(i, (max, end));
            }
        }
        if !grid {
            w.view_laid_out(i);
            w.view_current_section(i);
        }
        w.view_settle_position(i);
        w.view_follow(i);
        w.view_strict_layout(i);
        w.view_resized_layout(i);
        // QQuickItemViewPrivate::updateCurrent: an index outside the model has no current item;
        // the current delegate takes focus inside the view's focus scope.
        // QQuickItemView::componentComplete: a view whose currentIndex nothing set takes its
        // first row once it has rows.
        let key = w.view_cur(i);
        if !dels.is_empty() && !w.items[i].props.contains_key("currentIndex") {
            w.view_first_row(i);
        }
        let set_cur = opt_num(w, i, key).unwrap_or(-1.0);
        let cur = if dels.is_empty() || set_cur >= total {
            -1.0
        } else {
            set_cur.max(-1.0)
        };
        // The delegate of the current row, which a ListView keeps outside its rows too.
        let cur_item = match w.view_rows.get(&i) {
            _ if cur < 0.0 => None,
            Some(v) if !grid => v.item(cur as usize),
            _ => dels.get(cur as usize).copied(),
        };
        if let Some(c) = cur_item {
            current_takes_focus(w, i, c);
        }
        let ci = cur_item.map_or(Value::Null, Value::Item);
        match w.items[i].props.get("currentItem") {
            Some(p) if p.value.strict_eq(&ci) => {}
            Some(_) => {
                w.write(i, "currentItem", ci);
            }
            None => {
                w.declare(i, "currentItem", "QtObject");
                w.write(i, "currentItem", ci);
            }
        }
        let follows = !explicit(w, i, "highlightFollowsCurrentItem")
            || truthy(w, i, "highlightFollowsCurrentItem", true);
        // The highlight (QQuickItemViewPrivate::highlight, an Item nothing sees when the view
        // declares none) is made at the current row and moves to the next one by
        // QSmoothedAnimation (QQuickListViewPrivate::updateHighlight).
        let t = (w.view_track.entry(i)).or_insert_with(|| ViewTrack::new(set_cur));
        let changed = t.index != set_cur;
        let first = !std::mem::replace(&mut t.laid, true);
        // QQuickItemView::componentComplete places the view at the current row it has then:
        // the first layout does it, for the row the view was complete with.
        let fresh = first && !changed;
        // A row chosen between the two: the highlight was made at the row before it.
        let before = (first && changed && t.index >= 0.0).then_some(t.index);
        let mut highlight_at = None;
        if let Some(c) = cur_item {
            let k = if horiz { "contentX" } else { "contentY" };
            let (content, size) = (num(w, i, k), num(w, i, len));
            let (y, h) = (num(w, c, at), num(w, c, len));
            let (lead, trail) = if horiz {
                ("leftMargin", "rightMargin")
            } else {
                ("topMargin", "bottomMargin")
            };
            let (lead, trail) = (num(w, i, lead), num(w, i, trail));
            // Positions along the flow: a BottomToTop view counts up from its bottom. The
            // content position is where the view starts: before row 0 by the leading margin
            // when that is in sight.
            let (view, mut row, ahead, behind) = if reversed {
                (-content - size, -(y + h), trail, lead)
            } else {
                (content, y, lead, trail)
            };
            let (velocity, duration) = if grid {
                (
                    200.0,
                    opt_num(w, i, "highlightMoveDuration").unwrap_or(150.0),
                )
            } else {
                (
                    opt_num(w, i, "highlightMoveVelocity").unwrap_or(400.0),
                    opt_num(w, i, "highlightMoveDuration").unwrap_or(-1.0),
                )
            };
            let now = w.now;
            let t = w.view_track.get_mut(&i).expect("made above");
            let made = t.to.is_nan();
            // On its way since an earlier layout: this one is a step of its move.
            let stepping = !grid && !t.settled;
            if let Some(b) = before.filter(|_| follows) {
                let step = if grid { h } else { h + spacing };
                let rows = (set_cur / cols).floor() - (b / cols).floor();
                t.place(row - rows * step);
                t.retarget(row, now, velocity, duration);
            } else if made || !follows {
                t.place(row);
            } else if t.to != row {
                t.retarget(row, now, velocity, duration);
            }
            t.following |= changed;
            t.index = set_cur;
            let (mut tracked, following) = (t.pos(now), t.following);
            highlight_at = Some(if reversed { -(tracked + h) } else { tracked });
            // QQuickItemView::trackedPositionChanged: the view moves to keep the highlight in
            // sight while it and the current row are beyond the same edge. It does once when
            // the view gets its first current row (componentComplete), and from a write of
            // currentIndex until something else moves the view.
            if follows && (fresh || following) {
                let mut pos = view;
                if let Some((begin, end)) = w.view_apply_range(i).filter(|_| !reversed) {
                    // The highlight stays inside the range, as far as the view can go.
                    if tracked > pos + end - h {
                        pos = tracked - end + h;
                    }
                    if tracked < pos + begin {
                        pos = tracked - begin;
                    }
                    let (min, max) = w.flick_extents(i, !horiz);
                    pos = pos.min(-max).max(-min);
                } else {
                    // showHeaderForIndex, showFooterForIndex: the first row brings the header and
                    // the margin before it in sight, the last one the footer and the margin after.
                    let shift = if cur < cols {
                        -(hs + ahead)
                    } else if cur >= total - cols {
                        fs + behind
                    } else {
                        0.0
                    };
                    tracked += shift;
                    row += shift;
                    let (tracked_end, row_end, end) = (tracked + h, row + h, view + size);
                    if tracked_end >= end && row_end >= end {
                        let (from, to) = if tracked_end <= row_end {
                            (tracked, tracked_end)
                        } else {
                            (row, row_end)
                        };
                        pos = if h > size { from } else { to - size };
                    }
                    if tracked < pos && row < pos {
                        pos = tracked.max(row);
                    }
                }
                if pos != view {
                    let pos = if reversed { -pos - size } else { pos };
                    let was = std::mem::replace(&mut w.animating, true);
                    set(w, i, k, pos);
                    w.animating = was;
                    // Its own move: the view goes on following.
                    if let Some(t) = w.view_track.get_mut(&i) {
                        t.following = following;
                    }
                }
                if pos == view && following && stepping {
                    // No move to refill after: bufferPause runs out.
                    w.view_refill(i);
                }
            }
        } else if let Some(t) = w.view_track.get_mut(&i) {
            // No current row: the highlight is gone.
            t.place(f64::NAN);
            if cur < 0.0 {
                t.following |= changed;
                t.index = set_cur;
            }
        }
        // QQuickListViewPrivate::updateHighlight: the highlight takes the current item's
        // place along the view and its size.
        if let Some(h) = decor[2] {
            let on = cur >= 0.0 && follows;
            if on && let Some(c) = cur_item {
                // QQuickGridViewPrivate::updateHighlight: the current cell's column and row.
                let across = if horiz { "y" } else { "x" };
                for k in [at, "width", "height"]
                    .into_iter()
                    .chain(grid.then_some(across))
                {
                    // Along the view it is where its move has brought it.
                    let v = match highlight_at {
                        Some(v) if k == at => v,
                        _ => num(w, c, k),
                    };
                    set(w, h, k, v);
                }
            } else if cur < 0.0 {
                set(w, h, "width", 0.0);
                set(w, h, "height", 0.0);
            }
        }
    } else if is(w, i, "TabBar") {
        // QQuickTabBarPrivate::updateLayout: tabs without an explicit width share what the
        // explicit ones leave; all take the tallest implicit height.
        let spacing = prop_num(w, i, "spacing").unwrap_or(1.0);
        let tabs = tab_items(w, i, &ch);
        let (l, r, t) = (
            pad_side(w, i, "left"),
            pad_side(w, i, "right"),
            pad_side(w, i, "top"),
        );
        let (mut reserved, mut resizable, mut tallest) = (0.0f64, 0usize, 0.0f64);
        for &c in &tabs {
            if explicit(w, c, "width") {
                reserved += num(w, c, "width");
            } else {
                resizable += 1;
            }
            tallest = tallest.max(num(w, c, "implicitHeight"));
        }
        let gaps = tabs.len().saturating_sub(1) as f64 * spacing;
        let each = (pw - l - r - reserved - gaps) / resizable.max(1) as f64;
        let mut x = l;
        for &c in &tabs {
            if !explicit(w, c, "width") {
                set(w, c, "width", each);
            }
            if !explicit(w, c, "height") {
                set(w, c, "height", tallest);
            }
            set(w, c, "x", x);
            set(w, c, "y", t);
            x += num(w, c, "width") + spacing;
        }
        // The tabs sit in a ListView contentItem: QQuickItemViewPrivate::updateCurrent gives the
        // current tab focus in that scope.
        let cur = opt_num(w, i, "currentIndex").unwrap_or(0.0);
        if cur >= 0.0
            && let Some(&c) = tabs.get(cur as usize)
        {
            current_takes_focus(w, i, c);
        }
    } else if is(w, i, "SwipeView") {
        // QQuickSwipeView: pages fill the view, laid out horizontally in a ListView that moves
        // to currentIndex (qml::runtime::Swipe); Qt.Vertical stacks them.
        let spacing = prop_num(w, i, "spacing").unwrap_or(0.0);
        let vertical = num(w, i, "orientation") == 2.0;
        let cur = opt_num(w, i, "currentIndex").unwrap_or(0.0).max(0.0);
        let target = cur;
        // The page ListView is the contentItem: inside the padding, availableWidth/Height.
        let (l, t) = (pad_side(w, i, "left"), pad_side(w, i, "top"));
        let aw = (pw - l - pad_side(w, i, "right")).max(0.0);
        let ah = (ph - t - pad_side(w, i, "bottom")).max(0.0);
        let pitch = if vertical { ah + spacing } else { aw + spacing };
        let now = w.now;
        // A swipe in progress places the pages at its content position instead; the
        // currentIndex move animation (QQuickSwipeView's 250 ms highlight move) starts from rest.
        let dragged = SWIPE_POS.with(|m| m.borrow().get(&i).copied());
        let s = w
            .swipes
            .entry(i)
            .or_insert_with(|| qml::runtime::Swipe::at(target));
        if dragged.is_some() {
            *s = qml::runtime::Swipe::at(target);
        } else if s.to != target {
            s.retarget(target, now, pitch);
        }
        let pos = dragged.unwrap_or(s.pos(now) * pitch);
        let mut k = 0.0;
        for &c in &ch {
            if !positioned(w, c) {
                continue;
            }
            let step = k * pitch - pos;
            set(w, c, "x", l + if vertical { 0.0 } else { step });
            set(w, c, "y", t + if vertical { step } else { 0.0 });
            set(w, c, "width", aw);
            set(w, c, "height", ah);
            // QQuickItemViewPrivate::updateCurrent: the current page takes focus in its scope.
            if k == cur {
                current_takes_focus(w, i, c);
            }
            k += 1.0;
        }
    } else if is(w, i, "StackView") {
        // QQuickStackElement: elements without an explicit size fill the view.
        for &c in &ch {
            set(w, c, "x", 0.0);
            set(w, c, "y", 0.0);
            set(w, c, "width", pw);
            set(w, c, "height", ph);
        }
    } else if is_loader(w, i) {
        for &c in &ch {
            if explicit(w, i, "width") {
                set(w, c, "width", pw);
            }
            if explicit(w, i, "height") {
                set(w, c, "height", ph);
            }
        }
    }
    if is_loader(w, i)
        && let Some(Value::Item(it)) = w.read(i, "item")
    {
        // An item whose size is set keeps it while the Loader's is not (it got it from the
        // Loader before a reset, QQuickLoaderPrivate::_q_updateSize): the Loader follows it.
        let valid = loader_size_valid(w, i);
        for (k, valid, sz) in [("width", valid.0, pw), ("height", valid.1, ph)] {
            if (valid || !explicit(w, it, k)) && sz > 0.0 && num(w, it, k) != sz {
                set(w, it, k, sz);
            }
        }
    }
    if is(w, i, "Slider") {
        let p = slider_position(w, i);
        set(w, i, "position", p);
        set(w, i, "visualPosition", p);
    }
    // TextField/TextArea (QQuickTextFieldPrivate::resizeBackground): background, no contentItem.
    if w.items[i].prop_info("contentItem").is_none()
        && w.items[i].prop_info("background").is_some()
        && let Some(Value::Item(bg)) = w.read(i, "background")
        && bg != i
        && !bg_anchored(w, bg)
    {
        if w.items[bg].parent != Some(i) {
            w.reparent(bg, i);
        }
        resize_background(w, i, bg, pw, ph);
    }
    if w.items[i].prop_info("contentItem").is_some() && w.items[i].prop_info("background").is_some()
    {
        let pad = |w: &mut World, side: &str| pad_side(w, i, side);
        let (l, r, t, b) = (
            pad(w, "left"),
            pad(w, "right"),
            pad(w, "top"),
            pad(w, "bottom"),
        );
        // QQuickControlPrivate::resizeBackground leaves an anchored background to its anchors.
        if let Some(Value::Item(bg)) = w.read(i, "background")
            && ch.contains(&bg)
            && !bg_anchored(w, bg)
        {
            resize_background(w, i, bg, pw, ph);
        }
        if let Some(Value::Item(ci)) = w.read(i, "contentItem")
            && ci != i
        {
            if w.items[ci].parent != Some(i) {
                w.reparent(ci, i);
            }
            // anchors.fill on the contentItem wins (applied below); placing it here as well
            // would flip its geometry on every pass.
            if is(w, i, "SpinBox") {
                for k in ["down.indicator", "up.indicator"] {
                    let Some((ind, iw)) = spin_indicator(w, i, k) else {
                        continue;
                    };
                    if w.items[ind].parent != Some(i) {
                        w.reparent(ind, i);
                    }
                    set(w, ind, "width", iw);
                    if !explicit(w, ind, "height") {
                        let ih = num(w, ind, "implicitHeight");
                        set(w, ind, "height", ih);
                    }
                }
            }
            if is(w, i, "ScrollBar") {
                scrollbar_content(w, i);
            } else if places_content(w, i, ci) {
                set(w, ci, "x", l);
                set(w, ci, "y", t);
                set(w, ci, "width", (pw - l - r).max(0.0));
                set(w, ci, "height", (ph - t - b).max(0.0));
            }
            // The bars of a ScrollView follow its Flickable and its own size.
            if scroll_flickable(w, i) == Some(ci) {
                attached_bars(w, i, ci);
            }
            if menu_list(w, i) == Some(ci) {
                menu_place(w, ci);
            }
            // QQuickDayOfWeekRowPrivate::resizeItems: seven equal cells filling the contentItem.
            if is(w, i, "DayOfWeekRow") {
                let sp = num(w, i, "spacing");
                let cell = (num(w, ci, "width") - 6.0 * sp) / 7.0;
                let h = num(w, ci, "height");
                for d in kids(w, ci) {
                    if !is(w, d, "Repeater") {
                        set(w, d, "width", cell);
                        set(w, d, "height", h);
                    }
                }
            }
        }
    }
    if is_flickable(w, i) {
        // QQuickFlickable::componentComplete: a Flickable with a margin before its content
        // and no position of its own starts at the margin's edge.
        if !is(w, i, "ListView") && !is(w, i, "GridView") && opt_num(w, i, VSTART).is_none() {
            set(w, i, VSTART, 0.0);
            for (k, margin) in [("contentX", "leftMargin"), ("contentY", "topMargin")] {
                let m = num(w, i, margin);
                if m != 0.0 && !explicit(w, i, k) {
                    set(w, i, k, -m);
                }
            }
        } else if (is(w, i, "ListView") || is(w, i, "GridView"))
            && opt_num(w, i, CROSS_START).is_none()
        {
            // The same for a view's other axis: its own start is set by the view's layout
            // (QQuickItemViewPrivate::contentStartOffset).
            set(w, i, CROSS_START, 0.0);
            let along_x = if is(w, i, "ListView") {
                num(w, i, "orientation") == 1.0
            } else {
                num(w, i, "flow") == 1.0
            };
            let (k, margin) = if along_x {
                ("contentY", "topMargin")
            } else {
                ("contentX", "leftMargin")
            };
            let m = num(w, i, margin);
            if m != 0.0 && !explicit(w, i, k) {
                set(w, i, k, -m);
            }
        }
        flick_flags(w, i);
    }
    let all = kids(w, i);
    anchor_children(w, i, all.clone());
    // QQuickTextAreaPrivate::resizeFlickableControl: a TextArea attached to the Flickable of a
    // ScrollView is as large as the Flickable or its content, whichever is larger, and as
    // wide as the Flickable when it wraps. Anchors on the TextArea have the last word.
    if let Some(c) = scroll_text_area(w, i) {
        if !w.items[c].props.contains_key(AREA_SIZED_BY_VIEW) {
            set(w, c, AREA_SIZED_BY_VIEW, 1.0);
            // The Flickable takes the implicit size of the TextArea in the next pass.
            WRAP_RESIZED.with(|f| f.set(true));
        }
        let (cw, chh) = (num(w, i, "contentWidth"), num(w, i, "contentHeight"));
        let wraps = num(w, c, "wrapMode") != 0.0;
        let fill = matches!(w.read(c, "anchors.fill"), Some(Value::Item(_)));
        let both = |w: &mut World, a: &str, b: &str| {
            fill || [a, b]
                .iter()
                .all(|k| !matches!(w.read(c, k), None | Some(Value::Null | Value::Undefined)))
        };
        if !both(w, "anchors.left", "anchors.right") {
            set(w, c, "width", if wraps { pw } else { pw.max(cw) });
        }
        if !both(w, "anchors.top", "anchors.bottom") {
            set(w, c, "height", ph.max(chh));
        }
    }
    for &c in &all {
        if is(w, c, "ToolTip") {
            place_tooltip(w, i, c);
        }
        place(w, c);
    }
    // QQuickTextAreaPrivate::itemGeometryChanged: the Flickable has a new size, so the
    // TextArea is given it again, anchored or not, and one that wraps breaks its lines in
    // the Flickable's width. The Flickable then takes the implicit size the TextArea has
    // with those lines (`pane_content`), and the anchors put the TextArea in it. The
    // TextArea is in the contentItem, placed just above: its anchors have run by now.
    if let Some(c) = scroll_text_area(w, i) {
        let wraps = num(w, c, "wrapMode") != 0.0;
        let seen = AREA_VIEW.map(|k| opt_num(w, c, k));
        set(w, c, AREA_VIEW[0], pw);
        set(w, c, AREA_VIEW[1], ph);
        if wraps && seen[0].is_some() && seen != [Some(pw), Some(ph)] && num(w, c, "width") != pw {
            set(w, c, "width", pw);
            set(w, c, AREA_REFIT, 1.0);
            WRAP_RESIZED.with(|f| f.set(true));
        }
    }
}

/// The style's ListView contentItem of a QQuickMenu (approximated by the popup's own
/// contentItem holding the menu's items), unless the menu sets its own contentItem.
fn menu_list(w: &mut World, i: usize) -> Option<usize> {
    if !qml::types::inherits(&w.items[i].types, "QQuickMenu") {
        return None;
    }
    match w.read(i, "contentItem") {
        Some(Value::Item(ci)) if ci != i && !w.items[ci].props.contains_key("\x01propval") => {
            Some(ci)
        }
        _ => None,
    }
}

/// Basic Menu.qml `contentItem: ListView { implicitHeight: contentHeight }` with
/// QQuickMenuPrivate::updateContentWidth: the widest visible item's implicitWidth.
fn menu_content_size(w: &mut World, ci: usize) -> [f64; 2] {
    let (mut mw, mut h) = (0.0f64, 0.0f64);
    for c in visual_kids(w, ci) {
        if truthy(w, c, "visible", true) {
            mw = mw.max(fin(num(w, c, "implicitWidth")));
        }
        h += fin(menu_item_height(w, c));
    }
    [mw, h]
}

fn menu_item_height(w: &mut World, c: usize) -> f64 {
    if explicit(w, c, "height") {
        num(w, c, "height")
    } else {
        num(w, c, "implicitHeight")
    }
}

/// ListView stacking of the menu's items; QQuickMenuPrivate::resizeItem gives an item without
/// an explicit width the contentItem's width.
fn menu_place(w: &mut World, ci: usize) {
    let cw = num(w, ci, "width");
    let mut y = 0.0;
    for c in visual_kids(w, ci) {
        set(w, c, "x", 0.0);
        set(w, c, "y", y);
        if !explicit(w, c, "width") {
            set(w, c, "width", cw);
        }
        let h = fin(menu_item_height(w, c));
        if !explicit(w, c, "height") {
            set(w, c, "height", h);
        }
        y += h;
    }
}

fn fin(v: f64) -> f64 {
    if v.is_finite() { v } else { 0.0 }
}

/// The edge flags of Flickable `i` and its attached bar (`World::flick_edges`), then
/// the bar's place in the Flickable.
pub fn flick_flags(w: &mut World, i: usize) {
    w.flick_edges(i);
    // The bars of a ScrollView are attached to the ScrollView.
    let owner = scroll_view_of(w, i).unwrap_or(i);
    attached_bars(w, owner, i);
}

/// Whether `c` is the `ScrollBar.vertical` or `ScrollBar.horizontal` of the Flickable or
/// ScrollView `f`: QQuickScrollBarAttached parents it to `f` itself, outside the contentItem,
/// so it does not scroll.
pub fn attached_scrollbar(w: &mut World, f: usize, c: usize) -> bool {
    is(w, c, "ScrollBar")
        && SCROLL_BARS
            .iter()
            .any(|(k, _)| matches!(w.read(f, k), Some(Value::Item(b)) if b == c))
}

/// The Flickable an attached ScrollBar scrolls: the one it is attached to, or the Flickable
/// of the ScrollView it is attached to.
pub fn scrollbar_flickable(w: &mut World, b: usize) -> Option<(usize, bool)> {
    let p = w.items[b].parent?;
    let vertical = SCROLL_BARS
        .iter()
        .find(|(k, _)| matches!(w.read(p, k), Some(Value::Item(x)) if x == b))?
        .1;
    let f = match scroll_flickable(w, p) {
        Some(f) => f,
        None => p,
    };
    is_flickable(w, f).then_some((f, vertical))
}

/// What `attached_bars` keeps of a bar: whether its `visible` is the style's to set.
const BAR_VISIBLE: &str = "\x01barVisible";
/// And of a ScrollView: what its content and bars were when it was made with nothing in it,
/// until one of them changes.
const BARS_AS_MADE: &str = "\x01barsAsMade";

/// QQuickScrollBarAttachedPrivate (initVertical/layoutVertical/activateVertical and the
/// horizontal three) and Basic ScrollView.qml: the bars attached to `owner`, a Flickable or a
/// ScrollView, follow the Flickable `f`. A bar attached to a Flickable spans its right or
/// bottom edge; the bars a ScrollView made span its padded rect; a bar QML gave a ScrollView
/// is placed by QML. Each is sized and positioned as the visible part of the content, and is
/// active while the Flickable moves or the bar is hovered or pressed.
fn attached_bars(w: &mut World, owner: usize, f: usize) {
    let mut bars = [None; 2];
    for (n, (key, _)) in SCROLL_BARS.into_iter().enumerate() {
        if let Some(Value::Item(b)) = w.read(owner, key)
            && is(w, b, "ScrollBar")
            && !w.dead.contains(&b)
        {
            bars[n] = Some(b);
        }
    }
    if bars == [None; 2] {
        return;
    }
    let moving = w.flick_moving(f);
    let (ow, oh) = (num(w, owner, "width"), num(w, owner, "height"));
    let (fw, fh) = (num(w, f, "width"), num(w, f, "height"));
    let pad = if owner == f {
        [0.0; 4]
    } else {
        ["left", "top", "right", "bottom"].map(|k| pad_side(w, owner, k))
    };
    let own = bars.map(|b| {
        b.is_some_and(|b| {
            moving || truthy(w, b, "hovered", false) || truthy(w, b, "pressed", false)
        })
    });
    // `ScrollBar.vertical.active: true` on the ScrollView: the value QML gave stays.
    let fixed = [0, 1].map(|n| {
        let key = format!("{}.active", SCROLL_BARS[n].0);
        (owner != f && w.items[owner].props.contains_key(&key))
            .then(|| truthy(w, owner, &key, false))
    });
    for (n, (_, vertical)) in SCROLL_BARS.into_iter().enumerate() {
        let Some(b) = bars[n] else { continue };
        // Above what scrolls, the vertical bar below the horizontal one.
        let last = w.items[owner]
            .children
            .iter()
            .rev()
            .take(2)
            .any(|&c| c == b);
        if w.items[b].parent != Some(owner) || !last {
            w.reparent(b, owner);
        }
        let made = w.items[b].props.contains_key(SCROLL_MADE);
        set(w, b, "orientation", if vertical { 2.0 } else { 1.0 });
        let policy = num(w, b, "policy");
        // Basic ScrollBar.qml: `visible: control.policy !== T.ScrollBar.AlwaysOff`.
        if opt_num(w, b, BAR_VISIBLE).is_none() {
            let ours = made || !explicit(w, b, "visible");
            set(w, b, BAR_VISIBLE, ours as u8 as f64);
        }
        if opt_num(w, b, BAR_VISIBLE) == Some(1.0) {
            let on = policy != 1.0;
            if w.read(b, "visible").map(|v| v.truthy()) != Some(on) {
                w.write(b, "visible", Value::Bool(on));
            }
        }
        let size = |w: &mut World, k: &str, implicit: &str| {
            if explicit(w, b, k) {
                num(w, b, k)
            } else {
                num(w, b, implicit)
            }
        };
        if vertical {
            let bw = size(w, "width", "implicitWidth");
            if owner == f {
                set(w, b, "width", bw);
                set(w, b, "height", fh);
                set(w, b, "x", fw - bw);
            } else if made {
                set(w, b, "width", bw);
                set(w, b, "x", ow - bw);
                set(w, b, "y", pad[1]);
                set(w, b, "height", (oh - pad[1] - pad[3]).max(0.0));
            }
        } else {
            let bh = size(w, "height", "implicitHeight");
            if owner == f {
                set(w, b, "height", bh);
                set(w, b, "width", fw);
                set(w, b, "y", fh - bh);
            } else if made {
                set(w, b, "height", bh);
                set(w, b, "x", pad[0]);
                set(w, b, "y", oh - bh);
                set(w, b, "width", (ow - pad[0] - pad[2]).max(0.0));
            }
        }
        // Basic ScrollView.qml: each of its bars is `active` while the other one is.
        let active = match (made, fixed[n]) {
            (true, Some(v)) => v,
            (true, None) => own[n] || fixed[1 - n].unwrap_or(own[1 - n]),
            _ => own[n],
        };
        if w.read(b, "active").map(|v| v.truthy()) != Some(active) {
            w.write(b, "active", Value::Bool(active));
        }
        scrollbar_content(w, b);
    }
    if owner == f {
        return;
    }
    // QQuickScrollViewPrivate::updateScrollBarWidth/Height run when the content size or a bar
    // changes: a ScrollView with nothing in it has had neither and keeps 0.
    let seen = [num(w, f, "contentWidth"), num(w, f, "contentHeight")]
        .into_iter()
        .chain(
            (bars.into_iter().flatten())
                .flat_map(|b| [num(w, b, "policy"), visible(w, b) as u8 as f64]),
        )
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join(" ");
    let kept = match w.read(owner, BARS_AS_MADE) {
        Some(Value::Str(s)) => *s == *seen,
        Some(_) => false,
        None => {
            let content = flick_content(w, f);
            let empty =
                w.items[f].props.contains_key(SCROLL_MADE) && visual_kids(w, content).is_empty();
            let v = if empty {
                Value::Str(seen.into())
            } else {
                Value::Bool(false)
            };
            w.write(owner, BARS_AS_MADE, v);
            empty
        }
    };
    if kept {
        return;
    }
    if !matches!(w.read(owner, BARS_AS_MADE), Some(Value::Bool(_))) {
        w.write(owner, BARS_AS_MADE, Value::Bool(false));
    }
    for (n, (k, of)) in [
        ("effectiveScrollBarWidth", "width"),
        ("effectiveScrollBarHeight", "height"),
    ]
    .into_iter()
    .enumerate()
    {
        let v = match bars[n] {
            Some(b) if visible(w, b) => num(w, b, of),
            _ => 0.0,
        };
        set(w, owner, k, v);
    }
}

/// Basic ScrollBar.qml gives a ScrollBar that has none a `contentItem`, a Rectangle 6 thick (2
/// when the bar is not interactive), and a `background`, a Rectangle shown only in high
/// contrast.
fn scrollbar_handle(w: &mut World, b: usize) {
    if !is(w, b, "ScrollBar") {
        return;
    }
    for key in ["background", "contentItem"] {
        if matches!(w.read(b, key), Some(Value::Item(_))) || explicit(w, b, key) {
            continue;
        }
        let part = w.new_inst("Rectangle", Some(b));
        w.scroll_made(part);
        w.write(b, key, Value::Item(part));
        if key == "background" {
            w.write(part, "visible", Value::Bool(false));
        } else {
            w.write(part, "opacity", Value::Num(0.0));
        }
    }
}

/// The ScrollBar's handle becomes its child, then `World::scrollbar_content` places it. Basic
/// ScrollBar.qml styles the handle a bar made itself.
fn scrollbar_content(w: &mut World, b: usize) {
    let Some(Value::Item(ci)) = w.read(b, "contentItem") else {
        w.scrollbar_content(b);
        return;
    };
    if ci == b {
        w.scrollbar_content(b);
        return;
    }
    if w.items[ci].parent != Some(b) {
        w.reparent(ci, b);
    }
    w.scrollbar_content(b);
    if !w.items[ci].props.contains_key(SCROLL_MADE) {
        return;
    }
    // Basic ScrollBar.qml: `radius: width / 2`, `color: control.pressed ? control.palette.dark
    // : control.palette.mid`, and `opacity: 0.75` in the `active` state, which is `policy ===
    // AlwaysOn || (active && size < 1.0)`, else 0.
    let thick = if !explicit(w, b, "interactive") || truthy(w, b, "interactive", true) {
        6.0
    } else {
        2.0
    };
    set(w, ci, "implicitWidth", thick);
    set(w, ci, "implicitHeight", thick);
    let radius = num(w, ci, "width") / 2.0;
    set(w, ci, "radius", radius);
    let color = if truthy(w, b, "pressed", false) {
        PALETTE_DARK
    } else {
        PALETTE_MID
    };
    if w.read(ci, "color").map(|v| rgba(&v)) != Some(rgba(&Value::Str(color.into()))) {
        w.write(ci, "color", Value::Str(color.into()));
    }
    let shown =
        num(w, b, "policy") == 2.0 || (truthy(w, b, "active", false) && num(w, b, "size") < 1.0);
    set(w, ci, "opacity", if shown { 0.75 } else { 0.0 });
}

/// `palette.mid` and `palette.dark` of a control of the Basic style (QQuickBasicTheme).
const PALETTE_MID: &str = "#bdbdbd";
const PALETTE_DARK: &str = "#353637";

/// Whether a size change of `i` only affects `i`'s own subtree and its anchoring in its
/// parent: the parent neither arranges nor sizes its children from theirs.
pub fn is_local_resize(w: &mut World, i: usize) -> bool {
    if w.items
        .get(i)
        .is_some_and(|it| it.prop_info("width").is_none())
    {
        // Transforms (Translate/Scale/Rotation) and other non-items only affect painting.
        return true;
    }
    let Some(p) = w.items.get(i).and_then(|it| it.parent) else {
        return false;
    };
    // Popups (ToolTip, Menu...) are placed on their own, never by the parent's layout.
    if w.dead.contains(&i) || w.dead.contains(&p) || w.items[i].prop_info("closePolicy").is_some() {
        return true;
    }
    if leaving_view(w, i) {
        return true;
    }
    if unlaid_row(w, i, p) {
        return false;
    }
    // A child of a Row/Column(Layout) re-places that layout, which is local if the layout's
    // own placement is.
    if defers_to_parent(w, i, p) {
        return is_local_resize(w, p);
    }
    !arranges_child(w, i, p)
}

/// Whether `i` is a row its ListView or GridView has made and not laid out yet, sized by a
/// binding of its implicit size. A full pass lays it out: what is bound to the sizes a measure
/// finds (`implicitHeight: line.implicitHeight`) answers before the view places the row, as it
/// does for a row made with the view.
fn unlaid_row(w: &World, i: usize, p: usize) -> bool {
    let Some(v) = (w.items[p].parent).filter(|v| w.content_item_of.get(v) == Some(&p)) else {
        return false;
    };
    let bound = |size: &str, implicit: &str| {
        !explicit(w, i, size)
            && (w.items[i].props.get(implicit)).is_some_and(|p| p.binding.is_some())
    };
    (is(w, v, "ListView") || is(w, v, "GridView"))
        && w.delegated.get(&v).is_some_and(|d| d.1.contains(&i))
        && w.view_extent.get(&v).is_none_or(|e| !e.0.contains(&i))
        && (bound("height", "implicitHeight") || bound("width", "implicitWidth"))
}

/// Diagnostic: the parent chain `is_local_resize` walks, and where it stops.
/// Geometry (x, y, width, height) of every live item under `root`, for layout checks.
pub fn geom_snapshot(w: &mut World, root: usize) -> HashMap<usize, [f64; 4]> {
    let mut out = HashMap::default();
    let mut stack = vec![root];
    while let Some(i) = stack.pop() {
        // Nested windows are laid out with their own surface.
        if w.dead.contains(&i) || (i != root && WINDOWS.iter().any(|t| is(w, i, t))) {
            continue;
        }
        // A hidden ToolTip is placed when it is shown (see paint_window's overlay pass).
        if is(w, i, "ToolTip") && !visible(w, i) {
            continue;
        }
        stack.extend(w.items[i].children.iter().copied());
        if w.items[i].prop_info("width").is_some() {
            let g = ["x", "y", "width", "height"].map(|k| num(w, i, k));
            out.insert(i, g);
        }
    }
    out
}

/// `p` is a layout/positioner, so a resize of its child `i` is handled by re-placing `p`.
fn defers_to_parent(w: &mut World, i: usize, p: usize) -> bool {
    // QQuickLoaderPrivate::_q_updateSize: a sized Loader imposes its size on the item and only
    // its implicit size follows the item's, so the item is placed with the Loader.
    linear_axis(w, p).is_some()
        || is_positioner(w, p)
        || loader_sizes_item(w, p, i)
        || control_slot(w, p, i)
        || view_delegate(w, p)
        // QQuickScrollView takes its content size from what is in its Flickable.
        || (w.flickable_of(p)).is_some_and(|f| scroll_view_of(w, f).is_some())
}

/// A delegate of a ListView: the view stacks its delegates like a Column, so a delegate
/// resize re-places the view (positions, contentHeight), local if the view's placement is.
fn view_delegate(w: &mut World, p: usize) -> bool {
    let p = w.flickable_of(p).unwrap_or(p);
    !qml::debug::on(qml::debug::Switch::NOVIEWLOCAL) && is(w, p, "ListView")
}

/// The ListView or GridView whose contentItem is `i`: what the view creates is a child of it.
fn view_of_content(w: &mut World, i: usize) -> Option<usize> {
    let v = w.flickable_of(i)?;
    (is(w, v, "ListView") || is(w, v, "GridView")).then_some(v)
}

/// QQuickControl places its contentItem and background itself (padding, insets) and takes its
/// implicit size from them, so a resize of either re-places the control as a whole.
fn control_slot(w: &mut World, p: usize, i: usize) -> bool {
    !qml::debug::on(qml::debug::Switch::NOCTRLLOCAL)
        && w.items[p].prop_info("background").is_some()
        && w.items[p].prop_info("contentY").is_none()
        && ["contentItem", "background"]
            .iter()
            .any(|k| matches!(w.read(p, k), Some(Value::Item(c)) if c == i))
}

/// Parents whose own layout depends on the child (views, Loader, Flickable, Control slots);
/// re-placing them locally diverges from a full layout, so their child resizes stay non-local.
fn arranges_child(w: &mut World, i: usize, p: usize) -> bool {
    let ty = |w: &World, n: &str| w.items[p].prop_info(n).is_some();
    [
        "Flow",
        "Grid",
        "GridLayout",
        "SwipeView",
        "StackView",
        "Loader",
        "WidgetHost",
        "StackLayout",
        "TabBar",
    ]
    .iter()
    .any(|t| is(w, p, t))
        || ty(w, "contentY") && !plain_flickable(w, p)
        // Other children of a Control sit on their own anchors, like in any Item.
        || ty(w, "background") && qml::debug::on(qml::debug::Switch::NOCTRLLOCAL)
        || ["contentItem", "background", "item"]
            .iter()
            .any(|k| matches!(w.read(p, k), Some(Value::Item(c)) if c == i))
}

/// A Flickable that is not a view: its children are anchored within the content rect
/// (contentWidth/Height or the view size), which a child's size never changes.
fn plain_flickable(w: &World, p: usize) -> bool {
    let scroll = w.items[p].parent.is_some_and(|v| {
        is(w, v, "ScrollView")
            && matches!(w.items[v].props.get("contentItem").map(|x| &x.value), Some(&Value::Item(f)) if f == p)
    });
    is(w, p, "Flickable")
        && !scroll
        && ![
            "ListView",
            "GridView",
            "PathView",
            "TableView",
            "ScrollView",
        ]
        .iter()
        .any(|t| is(w, p, t))
}

/// A removed delegate running its remove transition is no longer placed by its view
/// (QQuickItemView::releaseItem took it out of visibleItems): what the transition writes to it
/// re-places the delegate alone (its anchors, QQuickAnchorsPrivate::updateMe), not the view
/// and the layouts above it.
fn leaving_view(w: &World, i: usize) -> bool {
    w.view_removing.contains_key(&i) && !qml::debug::on(qml::debug::Switch::NOREMOVELOCAL)
}

/// The item `relayout_local(i)` actually re-places (children of Row/Column(Layout)s defer to it).
pub fn local_target(w: &mut World, i: usize) -> usize {
    match w.items[i]
        .parent
        .filter(|&p| !leaving_view(w, i) && defers_to_parent(w, i, p))
    {
        Some(p) => local_target(w, p),
        _ => i,
    }
}

/// Measure the local target `i`; true when it is a window root.
fn measure_local(w: &mut World, fonts: &mut Fonts, i: usize) -> bool {
    // A window root keeps the size its surface was given (implicit size is only a request).
    let win = w.items[i].parent.is_none() || WINDOWS.iter().any(|t| is(w, i, t));
    if win {
        // Only the content: writing the root's own size would re-queue it as resized.
        for c in kids(w, i) {
            measure(w, fonts, c);
        }
    } else {
        measure(w, fonts, i);
    }
    win
}

/// Re-place a locally resized item: its own subtree and its anchoring in the parent.
pub fn relayout_local(w: &mut World, fonts: &mut Fonts, i: usize) {
    if w.dead.contains(&i) || w.items[i].prop_info("width").is_none() {
        return;
    }
    let parent = w.items[i].parent;
    if let Some(p) = parent
        && !leaving_view(w, i)
        && defers_to_parent(w, i, p)
    {
        return relayout_local(w, fonts, p);
    }
    let win = measure_local(w, fonts, i);
    let Some(p) = parent.filter(|_| !win) else {
        return place(w, i);
    };
    // Siblings anchored to `i`, also through other siblings (`anchors.fill: target` shadows,
    // chains), follow its new geometry.
    let sibs = visual_kids(w, p);
    let moved = Anchoring::followers(w, p, i, &sibs);
    for &s in &moved[1..] {
        measure(w, fonts, s);
    }
    anchor_children(w, p, moved.clone());
    for s in moved {
        place(w, s);
    }
}

thread_local! {
    /// Per root: tree_gen at its last `clear_fresh`. Fresh items only come from new instances
    /// and enter a subtree only through tree edits, so no edit under it since means none.
    static FRESH_CLEAN: std::cell::RefCell<HashMap<usize, u64>> = Default::default();
}

/// Items under `root` have been laid out: later writes to them are ordinary changes. A hidden
/// pass leaves Layouts and positioners unarranged, so the geometry of items below them keeps
/// settling until the first shown pass (`World::settling`).
pub fn clear_fresh(w: &mut World, root: usize) {
    if w.fresh.is_empty() && w.settling.is_empty() {
        return;
    }
    let hidden = HIDDEN_PASS.with(|f| f.get());
    let sig = w.tree_gen;
    let clean = FRESH_CLEAN
        .with_borrow(|m| m.get(&root).copied())
        .is_some_and(|g| !w.tree_edited_under(root, g));
    let check = clean && qml::debug::on(qml::debug::Switch::MEMOCHECK);
    if clean && !check {
        return;
    }
    let mut settled = true;
    let mut under = false;
    let mut a = w.items[root].parent;
    while hidden
        && !under
        && let Some(x) = a
    {
        under = unarranged_hidden(w, x);
        a = w.items[x].parent;
    }
    let mut stack = vec![(root, under)];
    while let Some((i, under)) = stack.pop() {
        if w.fresh.remove(&i) {
            picture_laid_out(w, i);
            if check {
                tracing::warn!(target: "ri::check", "freshcheck {root}: stale fresh {i}");
            }
            if under {
                w.settling.insert(i);
                settled = false;
            }
        } else if !hidden {
            w.settling.remove(&i);
        } else if w.settling.contains(&i) {
            settled = false;
        }
        let under = under || unarranged_hidden(w, i);
        stack.extend(w.items[i].children.iter().map(|&c| (c, under)));
    }
    if settled {
        FRESH_CLEAN.with_borrow_mut(|m| m.insert(root, sig));
    }
}

thread_local!(static HIDDEN_PASS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) });

/// Layouts and positioners arrange in updatePolish, which Qt runs only for a shown window.
fn unarranged_hidden(w: &World, i: usize) -> bool {
    HIDDEN_PASS.with(|f| f.get())
        && (linear_axis(w, i).is_some() || is(w, i, "Flow") || is_positioner(w, i))
}

/// Lays out a window that is not shown: like Qt, Layouts and positioners leave their children
/// where they are (a never-shown window's at 0) until the window is shown and laid out in full.
pub fn layout_hidden_window(rt: &mut Runtime, fonts: &mut Fonts, win: usize, size: [f64; 2]) {
    HIDDEN_PASS.with(|f| f.set(true));
    layout_window(rt, fonts, win, size);
    HIDDEN_PASS.with(|f| f.set(false));
}

/// Run geometry passes for the window `win` of size `size` until values settle.
/// A platform theme settings change (`qml::colorscheme::poll`) restyles what was drawn with the
/// old scheme, link color, system font and icon colors: drop those draws and the decoded icons.
fn follow_theme() {
    thread_local!(static APPLIED: std::cell::Cell<u64> = const { std::cell::Cell::new(0) });
    let g = qml::colorscheme::generation();
    if APPLIED.replace(g) != g {
        forget_draws();
        forget_themed_images();
        KEYED.with_borrow_mut(|c| c.clear());
    }
}

/// Forgets the pixmaps the icon theme answers: another theme or scheme gives other files and
/// colors ([`icon_scheme`]). A picture from a file is the same under any theme and stays:
/// forgotten, it is missing from its window (the wallpaper's) until decoded again.
fn forget_themed_images() {
    IMAGES.with_borrow_mut(|c| {
        c.retain(|k, _| !(provider_source(&k.0) || k.0.starts_with("image://kicon/")))
    });
}

pub fn layout_window(rt: &mut Runtime, fonts: &mut Fonts, win: usize, size: [f64; 2]) {
    follow_theme();
    crate::canvas2d::install();
    if !qml::debug::on(qml::debug::Switch::MEMOCHECK) || !memo_on() {
        return layout_window_impl(rt, fonts, win, size);
    }
    layout_window_impl(rt, fonts, win, size);
    let before = geom_snapshot(&mut rt.world.borrow_mut(), win);
    set_memo_off(true);
    layout_window_impl(rt, fonts, win, size);
    set_memo_off(false);
    let mut w = rt.world.borrow_mut();
    let after = geom_snapshot(&mut w, win);
    // A hidden ToolTip is placed again when shown (its `visible` write re-places the parent).
    let hidden_tip: std::collections::HashSet<usize> = after
        .keys()
        .copied()
        .filter(|&i| is(&w, i, "ToolTip") && !w.read(i, "visible").is_some_and(|v| v.truthy()))
        .collect();
    let bad: Vec<String> = after
        .iter()
        .filter(|(i, g)| {
            **i != win
                && !hidden_tip.contains(i)
                && before
                    .get(i)
                    .is_none_or(|b| b.iter().zip(g.iter()).any(|(x, y)| (x - y).abs() > 0.01))
        })
        .take(6)
        .map(|(&i, g)| {
            format!(
                "{i}:{:?} {:?}->{g:?} in {}",
                w.items[i].types.first(),
                before.get(&i),
                type_chain(&w, i)
            )
        })
        .collect();
    tracing::warn!(target: "ri::check", "memocheck {win} {size:?} {bad:?}");
}

fn layout_window_impl(rt: &mut Runtime, fonts: &mut Fonts, win: usize, size: [f64; 2]) {
    rt.world.borrow_mut().stand_views();
    layout_window_passes(rt, fonts, win, size);
    if follow_cursors(&mut rt.world.borrow_mut(), fonts, win) {
        rt.settle();
        layout_window_passes(rt, fonts, win, size);
    }
    // A view sent to its end has its inserted rows sized by now: it stays where it is from
    // here on, as in Qt.
    let mut w = rt.world.borrow_mut();
    if !w.view_at_end.is_empty() {
        let views: Vec<usize> = w.view_at_end.keys().copied().collect();
        for v in views {
            let mut a = Some(v);
            while let Some(x) = a.filter(|&x| x != win) {
                a = w.items[x].parent;
            }
            if a == Some(win) {
                w.view_at_end.remove(&v);
            }
        }
    }
    drop(w);
    clear_fresh(&mut rt.world.borrow_mut(), win);
    rt.world.borrow_mut().release_relaid_views();
    if let Some(ty) = &qml::debug::opts().geom
        && qml::log_on!("ri::geom")
    {
        trace_geometry(&mut rt.world.borrow_mut(), ty);
    }
}

/// QTextEngine::alignLine for AlignHCenter: the QFixed line width minus the text advance, halved
/// with integer division (a 1/64 px difference moves glyphs across subpixel positions).
fn center_offset(width: f64, advance: i64) -> f64 {
    (((width * 64.0).round() as i64 - advance) / 2) as f64 / 64.0
}

/// Debug aid: print geometry changes of items of type `ty` (`RI_LOG=ri::geom RI_DEBUG=geom=GroupButton`).
fn trace_geometry(w: &mut World, ty: &str) {
    thread_local! {
        static SEEN: std::cell::RefCell<HashMap<usize, String>> = Default::default();
    }
    for i in w.items.ids().collect::<Vec<_>>() {
        if w.dead.contains(&i) || !is(w, i, ty) {
            continue;
        }
        let g = format!(
            "x {} y {} w {} h {} iw {} fill {} down {:?} pressed {:?} parent {:?}",
            num(w, i, "x"),
            num(w, i, "y"),
            num(w, i, "width"),
            num(w, i, "height"),
            num(w, i, "implicitWidth"),
            w.read(i, "Layout.fillWidth").is_some_and(|v| v.truthy()),
            w.read(i, "down").map(|v| v.truthy()),
            w.read(i, "pressed").map(|v| v.truthy()),
            w.items[i].parent
        );
        SEEN.with(|s| {
            if s.borrow().get(&i) != Some(&g) {
                tracing::debug!(target: "ri::geom", "geom {ty}#{i} {g}");
                s.borrow_mut().insert(i, g);
            }
        });
    }
}

/// Settle the resizes bindings made since `resized[mark]` with local passes. False when one
/// of them needs the full layout (nothing re-placed for it then).
fn settle_locally(rt: &mut Runtime, fonts: &mut Fonts, win: usize, mut mark: usize) -> bool {
    // The items of `win` resized since `mark`, if each is a local resize.
    let resized = |rt: &mut Runtime, mark: usize| {
        let mut w = rt.world.borrow_mut();
        let rs: Vec<usize> = w.resized[mark.min(w.resized.len())..]
            .iter()
            .copied()
            .filter(|&i| !w.dead.contains(&i) && w.items.get(i).is_some() && i != win)
            .filter(|&i| {
                let mut a = Some(i);
                while let Some(x) = a {
                    if x == win {
                        return true;
                    }
                    a = w.items[x].parent;
                }
                false
            })
            .collect();
        rs.iter().all(|&i| is_local_resize(&mut w, i)).then_some(rs)
    };
    for _ in 0..4 {
        let Some(rs) = resized(rt, mark) else {
            return false;
        };
        if rs.is_empty() {
            return true;
        }
        measure_targets(&mut rt.world.borrow_mut(), fonts, &rs);
        rt.settle();
        // With what the bindings on the measured sizes answered.
        let Some(rs) = resized(rt, mark) else {
            return false;
        };
        if qml::log_on!("ri::passdbg") {
            let w = rt.world.borrow();
            let d: Vec<String> = rs
                .iter()
                .map(|&i| format!("{i}:{:?}<{:?}", w.items[i].types.first(), w.items[i].parent))
                .collect();
            tracing::debug!(target: "ri::passdbg", "  settle-local {d:?}");
        }
        relayout_targets(&mut rt.world.borrow_mut(), fonts, &rs, 0);
        let before = rt.revision();
        mark = rt.world.borrow().resized.len();
        rt.settle();
        if rt.revision() == before {
            return true;
        }
    }
    false
}

/// Debug (RI_DEBUG=laycheck): local settling must leave the geometry a full pass computes.
/// Types of `i`'s first few ancestors, for check diagnostics.
fn type_chain(w: &World, i: usize) -> String {
    let mut out = Vec::new();
    let mut p = w.items[i].parent;
    while let Some(q) = p.filter(|_| out.len() < 5) {
        out.push(w.items[q].types.first().map_or("?", |t| t.as_str()));
        p = w.items[q].parent;
    }
    out.join("<")
}

fn check_local_settle(rt: &mut Runtime, fonts: &mut Fonts, win: usize, size: [f64; 2]) {
    let before = geom_snapshot(&mut rt.world.borrow_mut(), win);
    set_memo_off(true);
    {
        let mut w = rt.world.borrow_mut();
        for c in kids(&mut w, win) {
            measure(&mut w, fonts, c);
        }
        place(&mut w, win);
        layout_layer_roots(&mut w, fonts, win);
    }
    rt.settle();
    set_memo_off(false);
    let mut w = rt.world.borrow_mut();
    let after = geom_snapshot(&mut w, win);
    let bad: Vec<String> = after
        .iter()
        .filter(|(i, g)| {
            **i != win
                && before
                    .get(i)
                    .is_none_or(|b| b.iter().zip(g.iter()).any(|(x, y)| (x - y).abs() > 0.01))
        })
        .take(6)
        .map(|(&i, g)| {
            format!(
                "{i}:{:?} {:?}->{g:?} in {}",
                w.items[i].types.first(),
                before.get(&i),
                type_chain(&w, i)
            )
        })
        .collect();
    tracing::warn!(target: "ri::check", "laycheck-settle {size:?} {bad:?}");
}

/// The topmost local targets of the resized items `rs` (one per subtree).
fn topmost_targets(w: &mut World, rs: &[usize]) -> Vec<usize> {
    w.stand_views();
    let alive: Vec<usize> = rs
        .iter()
        .copied()
        .filter(|&i| !w.dead.contains(&i) && w.items.get(i).is_some())
        .collect();
    let mut ts: Vec<usize> = alive.into_iter().map(|i| local_target(w, i)).collect();
    ts.sort_unstable();
    ts.dedup();
    let set: std::collections::HashSet<usize> = ts.iter().copied().collect();
    ts.into_iter()
        .filter(|&i| {
            let mut a = w.items[i].parent;
            while let Some(x) = a {
                if set.contains(&x) {
                    return false;
                }
                a = w.items[x].parent;
            }
            true
        })
        .collect()
}

/// Measure what `relayout_targets(rs)` re-places, placing nothing. A local pass goes in the
/// full pass's order (`layout_window_passes`): measure, `settle`, then `relayout_targets`. Qt
/// notifies implicit sizes at once and lays out at polish, so a binding on one (a ListView row's
/// `implicitHeight: line.implicitHeight`) answers before the placement; placed first, the view
/// counts the rows it has with the new ones at height 0.
pub fn measure_targets(w: &mut World, fonts: &mut Fonts, rs: &[usize]) {
    for i in topmost_targets(w, rs) {
        if !w.dead.contains(&i) && w.items[i].prop_info("width").is_some() {
            measure_local(w, fonts, i);
        }
    }
}

/// Re-place the topmost local targets of the resized items `rs` (one pass per subtree).
pub fn relayout_targets(w: &mut World, fonts: &mut Fonts, rs: &[usize], pass: usize) {
    let todo = topmost_targets(w, rs);
    let stats = qml::log_on!("ri::stats");
    for i in todo {
        let t = stats.then(std::time::Instant::now);
        WRAP_RESIZED.with(|f| f.set(false));
        relayout_local(w, fonts, i);
        if WRAP_RESIZED.with(|f| f.replace(false)) {
            relayout_local(w, fonts, i);
        }
        clear_fresh(w, i);
        if let Some(t) = t
            && t.elapsed().as_micros() > 300
        {
            tracing::debug!(
                target: "ri::stats",
                "  local p{pass} {i} {:?} {}us",
                w.items[i].types.first(),
                t.elapsed().as_micros()
            );
        }
    }
    edit_cursors_sync(w, fonts);
    w.release_relaid_views();
}

thread_local! {
    /// Set when a full layout ran out of passes with bindings still answering it.
    static UNSETTLED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether the last `layout_window` ended unsettled (see `UNSETTLED`); clears the flag.
pub fn take_unsettled() -> bool {
    UNSETTLED.with(|u| u.replace(false))
}

/// What the bindings still resize after an unsettled layout, for the warning.
pub fn unsettled_items(w: &World) -> Vec<String> {
    let n = w.resized.len();
    w.resized[n.saturating_sub(6)..]
        .iter()
        .map(|&i| format!("{i}:{:?}", w.items.get(i).and_then(|it| it.types.first())))
        .collect()
}

fn layout_window_passes(rt: &mut Runtime, fonts: &mut Fonts, win: usize, size: [f64; 2]) {
    UNSETTLED.with(|u| u.set(false));
    // The window shows `win`: an Item laid out as one's content has a window from here on.
    rt.world.borrow_mut().shown_in_window(win);
    let mut prev = geometry(&rt.world.borrow());
    let stats = qml::log_on!("ri::stats");
    for pass in 0..4 {
        let t0 = std::time::Instant::now();
        {
            let mut w = rt.world.borrow_mut();
            w.image_shell = true;
            set(&mut w, win, "width", size[0]);
            set(&mut w, win, "height", size[1]);
            // The first loads of the window's Images, in the order Qt starts them.
            for i in w.images_to_load() {
                let mut p = Some(i);
                while let Some(q) = p.filter(|&q| q != win) {
                    p = (is_item(&w, q) && type_bits(&w, q) & WINDOW_TYPE == 0)
                        .then(|| w.items[q].parent)
                        .flatten();
                }
                if p.is_some() {
                    measure(&mut w, fonts, i);
                }
            }
            for c in kids(&mut w, win) {
                measure(&mut w, fonts, c);
            }
        }
        // Qt notifies implicit sizes at once and lays out at polish: bindings on them (ContentPage
        // `width: Math.max(baseWidth, implicitWidth)` over a Flow) answer before placement.
        // Placing first hands the Flow a width computed from its previous implicit size, and two
        // consistent layouts (wrapped / one row) then alternate every frame.
        rt.settle();
        {
            let mut w = rt.world.borrow_mut();
            for c in kids(&mut w, win) {
                measure(&mut w, fonts, c);
            }
            WRAP_RESIZED.with(|f| f.set(false));
            place(&mut w, win);
            layout_layer_roots(&mut w, fonts, win);
            edit_cursors_sync(&mut w, fonts);
        }
        let rewrap = WRAP_RESIZED.with(|f| f.replace(false));
        let t1 = std::time::Instant::now();
        let before = rt.revision();
        let mark = rt.world.borrow().resized.len();
        let dbg = qml::log_on!("ri::passdbg");
        if dbg {
            tracing::debug!(target: "ri::passdbg", "  settle {pass}");
            rt.world.borrow_mut().trace_writes = true;
        }
        rt.settle();
        if dbg {
            rt.world.borrow_mut().trace_writes = false;
        }
        let settled = rt.revision() == before;
        if stats {
            tracing::debug!(
                target: "ri::passdbg",
                "  pass {pass} place {}us settle {}us quiet {settled}",
                (t1 - t0).as_micros(),
                t1.elapsed().as_micros()
            );
        }
        // Converged: a pass that ends where it started changes nothing for bindings either,
        // and if bindings wrote nothing, measuring again would reproduce this exact placement.
        let now = geometry(&rt.world.borrow());
        // A `visible` flip queues a resize without moving anything yet.
        let unplaced = rt.world.borrow().resized.len() > mark;
        if (settled || now == prev && !unplaced) && !rewrap {
            return;
        }
        // Bindings answered the placement with size changes of a few items: like Qt, which
        // only polishes the layouts those items invalidated, re-place just their subtrees.
        if !rewrap && settle_locally(rt, fonts, win, mark) {
            if qml::debug::on(qml::debug::Switch::LAYCHECK) {
                check_local_settle(rt, fonts, win, size);
            }
            return;
        }
        prev = geometry(&rt.world.borrow());
    }
    // Layouts own the geometry of their children: the final pass wins over bindings.
    UNSETTLED.with(|u| u.set(true));
    let mut w = rt.world.borrow_mut();
    for c in kids(&mut w, win) {
        measure(&mut w, fonts, c);
    }
    place(&mut w, win);
    layout_layer_roots(&mut w, fonts, win);
}

fn geometry(w: &World) -> (u64, usize) {
    (w.geom_rev, w.items.len())
}

bits::bit_flags! {
const SRC_TYPE: u8 = 1;
const MASK_SRC_TYPE: u8 = 2;
/// A visual item by type (an Item, effect or Popup; see `is_item`).
const ITEM_TYPE: u8 = 4;
/// A window type (`WINDOWS`).
const WINDOW_TYPE: u8 = 8;
}

thread_local! {
    /// Per instance: facts that depend only on its types, 0x80 = computed (types are fixed at
    /// creation and ids are never reused).
    static TYPE_BITS: std::cell::RefCell<qml::runtime::ChunkVec<u8>> = const { std::cell::RefCell::new(qml::runtime::ChunkVec::new()) };
}

fn type_bits(w: &World, i: usize) -> u8 {
    let cached = TYPE_BITS.with_borrow(|v| v.get(i));
    if cached != 0 {
        return cached;
    }
    let t = &w.items[i].types;
    let item = t.first().is_some_and(|t| !t.starts_with('\x01'))
        && (qml::types::find_prop(t, "anchors").is_some()
            || t.iter().any(|t| EFFECT_ITEMS.contains(&t.as_str()))
            || qml::types::is_popup(t));
    let bit = |on: bool, b: u8| if on { b } else { 0 };
    let v = 0x80
        | bit(qml::types::find_prop(t, "source").is_some(), SRC_TYPE)
        | bit(
            qml::types::find_prop(t, "maskSource").is_some(),
            MASK_SRC_TYPE,
        )
        | bit(item, ITEM_TYPE)
        | bit(t.iter().any(|t| WINDOWS.contains(&t.as_str())), WINDOW_TYPE);
    // A released id is not cached: `forget_chunks` already dropped what it had.
    if w.items.holds(i) {
        TYPE_BITS.with_borrow_mut(|c| *c.get_mut(i) = v);
    }
    v
}

thread_local! {
    /// Per instance: (property count when computed, bits): SRC_TYPE/MASK_SRC_TYPE = a property
    /// is assigned or grouped under `source`/`maskSource`, 0x80 = computed. Item properties only
    /// grow (or are dropped wholesale on destroy), so an unchanged count means unchanged keys.
    static SRC_KEYS: std::cell::RefCell<qml::runtime::ChunkVec<(u32, u8)>> = const { std::cell::RefCell::new(qml::runtime::ChunkVec::new()) };
}

fn src_keys(w: &mut World, i: usize) -> u8 {
    let n = w.items[i].props.len() as u32;
    let cur = SRC_KEYS.with_borrow(|v| v.get(i));
    if cur.1 != 0 && cur.0 == n {
        return cur.1;
    }
    let mut b = 0x80;
    for (bit, k) in [(SRC_TYPE, "source"), (MASK_SRC_TYPE, "maskSource")] {
        if w.items[i].props.contains_key(k) || w.has_grouped(i, k) {
            b |= bit;
        }
    }
    if w.items.holds(i) {
        SRC_KEYS.with_borrow_mut(|v| *v.get_mut(i) = (n, b));
    }
    b
}

/// Items under `i` whose `source`/`maskSource` (mask bits) may hold an item: declared, assigned
/// or grouped under the name, or a group/proxy object (see `plainly_unset`).
fn layer_cands(w: &mut World, i: usize, out: &mut Vec<(usize, u8)>) {
    let bits = (type_bits(w, i) | src_keys(w, i)) & (SRC_TYPE | MASK_SRC_TYPE);
    let plain = w.reading.is_none() && !w.groups.contains_key(&i);
    let mask = if plain {
        bits
    } else {
        SRC_TYPE | MASK_SRC_TYPE
    };
    if mask != 0 {
        out.push((i, mask));
    }
    let mut j = 0;
    while let Some(&c) = w.items[i].children.get(j) {
        layer_cands(w, c, out);
        j += 1;
    }
}

/// Per window: (tree_gen, reading) the candidates were found at, candidates.
type LayerCands = HashMap<usize, ((u64, bool), Rc<Vec<(usize, u8)>>)>;

thread_local! {
    /// Per window: (tree_gen, reading) the candidates were found at, candidates.
    static LAYER_CANDS: std::cell::RefCell<LayerCands> =
        Default::default();
}

/// Effect sources outside the visual tree (e.g. an Item held only by `maskSource`).
fn layer_roots(w: &mut World, win: usize, out: &mut Vec<usize>) {
    let sig = (w.tree_gen, w.reading.is_none());
    let cached = LAYER_CANDS.with_borrow(|m| {
        m.get(&win)
            .filter(|e| e.0.1 == sig.1 && !w.tree_edited_under(win, e.0.0))
            .map(|e| e.1.clone())
    });
    let cands = match cached {
        Some(c) if !qml::debug::on(qml::debug::Switch::MEMOCHECK) => c,
        cached => {
            let mut v = vec![];
            layer_cands(w, win, &mut v);
            if let Some(c) = cached
                && *c != v
            {
                tracing::warn!(
                    target: "ri::check",
                    "rootcheck {win} stale candidates {} vs {}",
                    c.len(),
                    v.len()
                );
            }
            let v = Rc::new(v);
            LAYER_CANDS.with_borrow_mut(|m| m.insert(win, (sig, v.clone())));
            v
        }
    };
    for &(i, mask) in cands.iter() {
        for (bit, k) in [(SRC_TYPE, "source"), (MASK_SRC_TYPE, "maskSource")] {
            if mask & bit == 0 {
                continue;
            }
            if let Some(Value::Item(s)) = w.read(i, k)
                && !w.dead.contains(&s)
                && (w.items[s].parent.is_none() || w.items[s].props.contains_key("\x01propval"))
                && !out.contains(&s)
            {
                out.push(s);
            }
        }
    }
}

fn layout_layer_roots(w: &mut World, fonts: &mut Fonts, win: usize) {
    let mut roots = vec![];
    layer_roots(w, win, &mut roots);
    for r in roots {
        measure(w, fonts, r);
        place(w, r);
    }
}

/// Paint destination: the raster canvas (fallback) or a GPU frame.
pub enum Target<'a> {
    Cpu(&'a mut Canvas),
    Gpu(&'a mut crate::gpu::Frame),
}

/// Run a raster painter for an item without a GPU path: directly on the canvas, or into a
/// texture covering `(x, y, w, h)` (clipped to the frame) that the GPU frame composites.
#[track_caller]
fn raster(cv: &mut Target, b: (f64, f64, f64, f64), f: impl FnOnce(&mut Canvas, f64, f64)) {
    if matches!(cv, Target::Gpu(_)) && qml::log_on!("ri::rastdbg") {
        tracing::debug!(
            target: "ri::rastdbg",
            "rastdbg {} {:.0}x{:.0}",
            std::panic::Location::caller(),
            b.2,
            b.3
        );
    }
    match cv {
        Target::Cpu(c) => f(cpu_plain(c), 0.0, 0.0),
        // Under a scale the raster is aligned to the item (like Canvas's own texture) and drawn
        // through the transform.
        Target::Gpu(fr) if fr.xf.is_some() => {
            let (w, h) = (b.2.ceil(), b.3.ceil());
            if !(w >= 1.0 && h >= 1.0 && w * h <= 16e6) {
                return;
            }
            let mut c = Canvas::new(w as usize, h as usize, [0, 0, 0, 0]);
            f(&mut c, b.0, b.1);
            if c.px.iter().any(|p| p[3] != 0) {
                fr.blit_at(c, b.0, b.1);
            }
        }
        Target::Gpu(fr) => {
            fr.opaque_draw();
            let x0 = b.0.floor().max(0.0) as i64;
            let y0 = b.1.floor().max(0.0) as i64;
            let x1 = ((b.0 + b.2).ceil() as i64).min(fr.lw as i64);
            let y1 = ((b.1 + b.3).ceil() as i64).min(fr.lh as i64);
            if x1 <= x0 || y1 <= y0 {
                return;
            }
            let mut c = Canvas::new((x1 - x0) as usize, (y1 - y0) as usize, [0, 0, 0, 0]);
            f(&mut c, x0 as f64, y0 as f64);
            if c.px.iter().any(|p| p[3] != 0) {
                fr.blit(c, x0, y0);
            }
        }
    }
}

/// `raster` whose output depends only on `key` and the pixel grid it lands on: an unchanged
/// item reuses its last raster instead of painting it again. The raster covers the frame's
/// pixels under `b` (device pixels under a scale) and `f` paints through `m` (sx sy tx ty,
/// canvas = s * p + t), so a scaled window rasterizes at device resolution, like Qt renders
/// Shape geometry, and the texture is drawn 1:1.
fn raster_keyed(
    cv: &mut Target,
    item: usize,
    key: u64,
    b: (f64, f64, f64, f64),
    f: impl FnOnce(&mut Canvas, [f64; 4]),
) {
    let fr = match cv {
        Target::Gpu(fr) if !qml::debug::on(qml::debug::Switch::NORASTERCACHE) => fr,
        _ => return raster(cv, b, |c, ox, oy| f(c, [1.0, 1.0, -ox, -oy])),
    };
    fr.opaque_draw();
    let [sx, sy, dx, dy] = fr.xform();
    let d = [sx * b.0 + dx, sy * b.1 + dy, sx * b.2, sy * b.3];
    let x0 = d[0].floor().max(0.0) as i64;
    let y0 = d[1].floor().max(0.0) as i64;
    let x1 = ((d[0] + d[2]).ceil() as i64).min(fr.w as i64);
    let y1 = ((d[1] + d[3]).ceil() as i64).min(fr.h as i64);
    if x1 <= x0 || y1 <= y0 || !cpu_texture_fits((x1 - x0) as f64, (y1 - y0) as f64) {
        return;
    }
    let key = if fr.xf.is_some() {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (key, sx.to_bits(), sy.to_bits()).hash(&mut h);
        h.finish()
    } else {
        key
    };
    let full = (
        key,
        (d[0] - x0 as f64).to_bits(),
        (d[1] - y0 as f64).to_bits(),
        x1 - x0,
        y1 - y0,
    );
    let hit = KEYED.with(|m| {
        m.borrow_mut()
            .get(&item)
            .filter(|e| e.0 == full)
            .map(|e| e.1.clone())
    });
    let out = match hit {
        Some(o) => o,
        None => {
            let mut c = Canvas::new((x1 - x0) as usize, (y1 - y0) as usize, [0, 0, 0, 0]);
            f(&mut c, [sx, sy, dx - x0 as f64, dy - y0 as f64]);
            let o = c.px.iter().any(|p| p[3] != 0).then(|| Rc::new(c));
            let n = opt_canvas_bytes(&o);
            KEYED.with(|m| m.borrow_mut().insert(item, (full, o.clone()), n));
            o
        }
    };
    if let Some(c) = out {
        // Shared, so the GPU keeps the uploaded texture while the raster is reused. The scene
        // rect maps back onto exactly the raster's device pixels: no filtering.
        let r = [
            (x0 as f64 - dx) / sx,
            (y0 as f64 - dy) / sy,
            c.w as f64 / sx,
            c.h as f64 / sy,
        ];
        fr.image(c, r, [0.0, 0.0, 1.0, 1.0], 1.0, false);
    }
}

type Keyed = ((u64, u64, u64, i64, i64), Option<Rc<Canvas>>);
type CanvasCached = ((u64, i64, i64, u64, u64), Option<Rc<Canvas>>);
type Replay = ((u64, usize, usize), Rc<Canvas>);
/// (Hash of a CurveRenderer Shape's paths, the fill triangles of each ShapePath).
type CurveTris = (u64, Rc<Vec<Vec<qsg::shape::FillTri>>>);

/// Pixel bytes of a cached raster: what the byte-bounded caches weigh.
fn canvas_bytes(c: &Canvas) -> usize {
    c.px.len() * 4
}

fn opt_canvas_bytes(c: &Option<Rc<Canvas>>) -> usize {
    c.as_deref().map_or(0, canvas_bytes)
}

/// Rasters shared with a recorded frame: evicting them frees nothing.
fn canvas_held(c: &Rc<Canvas>) -> bool {
    Rc::strong_count(c) > 1
}

thread_local! {
    /// Keyed CPU rasters (`raster_keyed`) by item: least recently drawn evicted first.
    static KEYED: std::cell::RefCell<ByteLru<usize, Keyed>> =
        std::cell::RefCell::new(ByteLru::new(16 << 20)
            .max_len(1024)
            .pinned(|e: &Keyed| e.1.as_ref().is_some_and(canvas_held)));
    /// Canvas items rasterized through their own texture (`canvas_raster`).
    static CANVASES: std::cell::RefCell<ByteLru<usize, CanvasCached>> =
        std::cell::RefCell::new(ByteLru::new(16 << 20)
            .max_len(512)
            .pinned(|e: &CanvasCached| e.1.as_ref().is_some_and(canvas_held)));
    /// Canvas op replays by item (`paint_canvas`).
    static REPLAYS: std::cell::RefCell<ByteLru<usize, Replay>> =
        std::cell::RefCell::new(ByteLru::new(16 << 20).max_len(512));
    /// The fill triangles of CurveRenderer Shapes the graphics card draws, by item.
    static CURVES: std::cell::RefCell<ByteLru<usize, CurveTris>> =
        std::cell::RefCell::new(ByteLru::new(4 << 20).max_len(512));
}

/// A Canvas through its own texture, like QQuickCanvasItem: the ops are rasterized once into a
/// canvas-window-sized image (reused until they change) that is drawn at the item's possibly
/// fractional position with the item's opacity and filtering.
#[allow(clippy::too_many_arguments)]
fn canvas_raster(
    w: &mut World,
    i: usize,
    cv: &mut Target,
    x: f64,
    y: f64,
    wd: f64,
    ht: f64,
    op: f32,
) {
    let fr = match cv {
        Target::Gpu(fr) if !qml::debug::on(qml::debug::Switch::NOCANVASCACHE) => fr,
        _ => {
            return raster(cv, (x, y, wd, ht), |c, ox, oy| {
                paint_canvas(w, i, c, x - ox, y - oy, wd, ht, op)
            });
        }
    };
    // Under a scale the texture is the ceil-sized item raster drawn through the transform
    // (like `raster`); at scale 1 it is the rounded size at the fractional position.
    let (cw, ch) = if fr.xf.is_some() {
        (wd.ceil() as i64, ht.ceil() as i64)
    } else {
        (wd.round() as i64, ht.round() as i64)
    };
    if cw <= 0 || ch <= 0 || cw * ch > 16_000_000 {
        return;
    }
    if x >= fr.lw as f64 || y >= fr.lh as f64 || x + wd <= 0.0 || y + ht <= 0.0 {
        // Not rasterized while off screen; the subtree must not be recorded without it, or a
        // recorded draw moved on screen (a sliding clock) would replay without the Canvas.
        no_cache();
        return;
    }
    let key = (
        w.canvas_gen.get(&i).copied().unwrap_or(0),
        cw,
        ch,
        wd.to_bits(),
        ht.to_bits(),
    );
    let hit = CANVASES.with(|m| {
        m.borrow_mut()
            .get(&i)
            .filter(|e| e.0 == key)
            .map(|e| e.1.clone())
    });
    let out = match hit {
        Some(o) => o,
        None => {
            if qml::log_on!("ri::rastdbg") {
                tracing::debug!(target: "ri::rastdbg", "rastdbg canvas {i} {wd:.0}x{ht:.0}");
            }
            let c = canvas_texture(w, i, cw as usize, ch as usize, wd, ht);
            let o = c.px.iter().any(|p| p[3] != 0).then(|| Rc::new(c));
            let n = opt_canvas_bytes(&o);
            CANVASES.with(|m| m.borrow_mut().insert(i, (key, o.clone()), n));
            o
        }
    };
    let live = canvas_live(i, key.0);
    SHOWN.with_borrow_mut(|m| m.remove(&i));
    if let Some(c) = out {
        let smooth = !explicit(w, i, "smooth") || truthy(w, i, "smooth", true);
        let rect = if fr.xf.is_some() {
            [x, y, c.w as f64, c.h as f64]
        } else {
            [x, y, wd, ht]
        };
        if live && lift_canvas(fr, i, key, &c, rect, op, smooth) {
            // The layer is recorded without it.
            no_cache();
            return;
        }
        if live {
            canvas_stays(i);
        }
        SHOWN.with_borrow_mut(|m| m.insert(i, (key, c.clone(), false)));
        fr.image(c, rect, [0.0, 0.0, 1.0, 1.0], op, smooth);
    }
}

/// What a Canvas texture is made for: the generation of its ops, its size in pixels and the
/// bits of the item's width and height.
type CanvasKey = (u64, i64, i64, u64, u64);

/// The Canvases of `repainted` (`World::repainted`) that changed in nothing but what they
/// draw: as often in `touched` as there, and not in `moved`.
pub fn only_repainted(touched: &[usize], moved: &[usize], repainted: &[usize]) -> Vec<usize> {
    let times = |l: &[usize], c: usize| l.iter().filter(|&&t| t == c).count();
    let mut only: Vec<usize> = repainted
        .iter()
        .copied()
        .filter(|&c| !moved.contains(&c) && times(touched, c) == times(repainted, c))
        .collect();
    only.sort_unstable();
    only.dedup();
    only
}

/// Canvas `c` painted something new and nothing else in its window changed since `fr`, the
/// frame the window shows, was painted. When `fr` draws the Canvas's last picture as one
/// image, it draws the new one there from now on and is the frame painting the window again
/// would give: true. False: the window is painted again, also once for a Canvas that turned
/// out to be repainted every frame while `fr` draws it into a layer, to draw it over the
/// layer where that is the same (`Lift`).
pub fn swap_repainted(w: &mut World, c: usize, fr: &mut crate::gpu::Frame) -> bool {
    let Some((was, old, lifted)) = SHOWN.with_borrow(|m| m.get(&c).cloned()) else {
        return false;
    };
    let lift = !lifted
        && CANVAS_RUNS.with_borrow(|m| {
            (m.get(&c)).is_some_and(|e| e.1.saturating_add(1) >= LIVE_RUN && !e.2 && !e.3)
        });
    if lift {
        return false;
    }
    let (wd, ht) = (num(w, c, "width"), num(w, c, "height"));
    let key = (
        w.canvas_gen.get(&c).copied().unwrap_or(0),
        was.1,
        was.2,
        was.3,
        was.4,
    );
    if (wd.to_bits(), ht.to_bits()) != (key.3, key.4) || !fr.draws(&old) {
        return false;
    }
    let tex = canvas_texture(w, c, key.1 as usize, key.2 as usize, wd, ht);
    if tex.px.iter().all(|p| p[3] == 0) {
        // Nothing to draw: the frame is one without the Canvas.
        return false;
    }
    let new = Rc::new(tex);
    if !fr.swap_tex(&old, &new) {
        return false;
    }
    let n = canvas_bytes(&new);
    CANVASES.with(|m| m.borrow_mut().insert(c, (key, Some(new.clone())), n));
    SHOWN.with_borrow_mut(|m| m.insert(c, (key, new, lifted)));
    CANVAS_RUNS.with_borrow_mut(|m| {
        if let Some(e) = m.get_mut(&c) {
            e.0 = key.0;
            e.1 = e.1.saturating_add(1);
        }
    });
    true
}

/// A Canvas's texture as a frame draws it: what it was made for, the texture, and whether it
/// is drawn over the Canvas's layer (`Lift`).
type Shown = (CanvasKey, Rc<Canvas>, bool);

/// A Canvas drawn over its layer instead of into it (`Lift`).
struct Lifted {
    item: usize,
    key: CanvasKey,
    tex: Rc<Canvas>,
    rect: [f64; 4],
    op: f32,
    smooth: bool,
    /// `Frame::drawn` of the layer when the Canvas was left out of it.
    at: usize,
}

/// The source layer of an OpacityMask being painted whose mask is one opaque rounded
/// Rectangle the size of the layer, drawn 1:1 at whole pixels with opacity 1. Where that mask
/// is 1 the effect shows the layer's pixels as they are, so a Canvas there with nothing of the
/// layer drawn over it blends the same onto the layer or onto the window after the layer. A
/// Canvas repainted frame after frame (the wave of a progress slider) is drawn the second way:
/// the layer does not change with it and its texture is not drawn again for every frame.
struct Lift {
    /// The layer's source item.
    root: usize,
    /// The address of the layer's frame while it is painted; 0 before.
    frame: usize,
    size: [f64; 2],
    /// The mask is 1 at least this far from its corners along both axes.
    radius: f64,
    items: Vec<Lifted>,
}

thread_local! {
    static LIFT: std::cell::RefCell<Option<Lift>> = const { std::cell::RefCell::new(None) };
    /// By Canvas: the op generation last shown, how many times in a row it was shown with a
    /// new one, whether it may not be lifted (something of its layer was drawn over it), and
    /// whether its last paint, of one repainted every frame, left it where it was.
    static CANVAS_RUNS: std::cell::RefCell<HashMap<usize, (u64, u8, bool, bool)>> =
        std::cell::RefCell::new(HashMap::default());
    /// By Canvas, as it was last painted (`swap_repainted`).
    static SHOWN: std::cell::RefCell<HashMap<usize, Shown>> =
        std::cell::RefCell::new(HashMap::default());
    /// Canvases drawn over their layer, for tests and `ri::rastdbg`.
    static LIFTS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Draws of a Canvas in a row with new ops that make it one repainted every frame.
const LIVE_RUN: u8 = 2;

/// Notes a draw of Canvas `i` at op generation `g`; true when it is repainted every frame.
fn canvas_live(i: usize, g: u64) -> bool {
    CANVAS_RUNS.with_borrow_mut(|m| {
        let e = m.entry(i).or_insert((g, 0, false, false));
        e.1 = if e.0 == g { 0 } else { e.1.saturating_add(1) };
        e.0 = g;
        e.3 = false;
        e.1 >= LIVE_RUN && !e.2
    })
}

/// Canvas `i`, repainted every frame, was painted where it is: not over a layer.
fn canvas_stays(i: usize) {
    CANVAS_RUNS.with_borrow_mut(|m| {
        if let Some(e) = m.get_mut(&i) {
            e.3 = true;
        }
    });
}

/// Canvases drawn over their layer so far (`Lift`).
pub fn canvas_lifts() -> u64 {
    LIFTS.with(|l| l.get())
}

/// Takes the Canvas out of the layer `fr` when `fr` is the layer of the `Lift` in effect and
/// the Canvas is where the mask is 1, unclipped and unscaled.
fn lift_canvas(
    fr: &crate::gpu::Frame,
    item: usize,
    key: CanvasKey,
    tex: &Rc<Canvas>,
    rect: [f64; 4],
    op: f32,
    smooth: bool,
) -> bool {
    LIFT.with_borrow_mut(|l| {
        let Some(l) = l.as_mut() else {
            return false;
        };
        let [x, y, wd, ht] = rect;
        let (r, [lw, lh]) = (l.radius, l.size);
        let inside = x >= 0.0 && y >= 0.0 && x + wd <= lw && y + ht <= lh;
        let core = (x >= r && x + wd <= lw - r) || (y >= r && y + ht <= lh - r);
        if l.frame != std::ptr::from_ref(fr) as usize
            || fr.xf.is_some()
            || fr.clipped()
            || !inside
            || !core
        {
            return false;
        }
        l.items.push(Lifted {
            item,
            key,
            tex: tex.clone(),
            rect,
            op,
            smooth,
            at: fr.drawn(),
        });
        true
    })
}

/// The `Lift` of an OpacityMask `i` over `src` with the mask item `mask`, drawn on `fr` at
/// `rect` (x y w h) with opacity `op`, when that is one.
fn lift_for(
    w: &mut World,
    fr: &crate::gpu::Frame,
    src: usize,
    mask: usize,
    rect: [f64; 4],
    invert: bool,
    op: f32,
) -> Option<Lift> {
    let [x, y, wd, ht] = rect;
    let whole = [x, y, wd, ht].iter().all(|v| v.fract() == 0.0);
    if invert || op != 1.0 || fr.xf.is_some() || fr.dpr != 1.0 || !whole {
        return None;
    }
    // An opaque Rectangle of one colour, as large as the effect, with four equal corners.
    let full = |w: &mut World, i: usize| {
        let corners = [
            "topLeftRadius",
            "topRightRadius",
            "bottomLeftRadius",
            "bottomRightRadius",
        ];
        is(w, i, "Rectangle")
            && num(w, i, "width") == wd
            && num(w, i, "height") == ht
            && color16(w, i, "color", [u16::MAX; 4])[3] == u16::MAX
            && num(w, i, "border.width") == 0.0
            && w.read(i, "gradient").is_none_or(|g| !g.truthy())
            && corners.iter().all(|c| !explicit(w, i, c))
    };
    // The source is one too: its texture is opaque wherever the mask is 1, so a Canvas blended
    // onto it and one blended onto the window over it round to the same bytes.
    if !full(w, mask) || !full(w, src) || !stacked_kids(w, mask).is_empty() {
        return None;
    }
    let radius = num(w, mask, "radius").max(num(w, src, "radius")).max(0.0);
    Some(Lift {
        root: src,
        frame: 0,
        size: [wd, ht],
        radius: radius.ceil() + 1.0,
        items: Vec::new(),
    })
}

/// `gpu_layer_dpr` of an OpacityMask's source under `lift`, and the Canvases left out of it:
/// each is to be drawn over the masked layer, at its place in it.
fn lifted_layer(
    w: &mut World,
    fonts: &mut Fonts,
    src: usize,
    dpr: f64,
    lift: Option<Lift>,
) -> (crate::gpu::Frame, Vec<Lifted>) {
    let outer = LIFT.replace(lift);
    let sf = gpu_layer_dpr(w, fonts, src, dpr);
    let items = LIFT.replace(outer).map_or(Vec::new(), |l| l.items);
    let px = |l: &Lifted| {
        let [x, y, wd, ht] = l.rect;
        [
            x.floor() as i64,
            y.floor() as i64,
            (x + wd).ceil() as i64,
            (y + ht).ceil() as i64,
        ]
    };
    let under: Vec<usize> = items
        .iter()
        .filter(|l| sf.drawn_over(l.at, px(l)))
        .map(|l| l.item)
        .collect();
    if under.is_empty() {
        LIFTS.with(|n| n.set(n.get() + items.len() as u64));
        SHOWN.with_borrow_mut(|m| {
            for l in &items {
                m.insert(l.item, (l.key, l.tex.clone(), true));
            }
        });
        return (sf, items);
    }
    // Something of the layer is over these: they are drawn into the layer from now on, and
    // this frame every Canvas is.
    CANVAS_RUNS.with_borrow_mut(|m| {
        for i in under {
            if let Some(e) = m.get_mut(&i) {
                e.2 = true;
            }
        }
    });
    (gpu_layer_dpr(w, fonts, src, dpr), Vec::new())
}

thread_local! {
    /// Canvas textures `canvas_texture` made, and how many of them it copied out of a replay.
    static CANVAS_TEXTURES: std::cell::Cell<[u64; 2]> = const { std::cell::Cell::new([0; 2]) };
}

/// The `cw` x `ch` texture of Canvas `i`, `wd` x `ht` in the scene: its ops replayed onto
/// transparent pixels. `paint_canvas` replays at the ceil of the item size and copies that 1:1
/// at the origin with opacity 1, which changes no byte, so a texture of that size is the replay
/// itself: a Canvas repainted every frame (a visualizer) is rasterized once per frame, not
/// rasterized and then copied.
fn canvas_texture(w: &mut World, i: usize, cw: usize, ch: usize, wd: f64, ht: f64) -> Canvas {
    let count = |copied: bool| {
        CANVAS_TEXTURES.with(|c| {
            let [made, copies] = c.get();
            c.set([made + 1, copies + copied as u64]);
        });
    };
    if (wd.ceil().max(1.0) as usize, ht.ceil().max(1.0) as usize) == (cw, ch) {
        count(false);
        return match w.canvas_ops.get(&i) {
            Some(ops) if !ops.is_empty() => crate::canvas2d::replay(ops, cw, ch),
            _ => Canvas::new(cw, ch, [0, 0, 0, 0]),
        };
    }
    count(true);
    let mut c = Canvas::new(cw, ch, [0, 0, 0, 0]);
    paint_canvas(w, i, &mut c, 0.0, 0.0, wd, ht, 1.0);
    c
}

/// The combined matrix of the item `i`'s node in its window: the product of its ancestors'
/// transform nodes' and its own (QSGNodeUpdater::enterTransformNode), each as
/// QQuickWindowPrivate::updateDirtyNode builds it, an identity one left out.
fn node_matrix(w: &mut World, i: usize) -> Mat4 {
    node_matrix_under(w, i, None)
}

/// `node_matrix` in the texture of the item `top`, a layer's or a source's: the nodes below
/// `top`'s own, which QSGLayer renders from.
fn node_matrix_under(w: &mut World, i: usize, top: Option<usize>) -> Mat4 {
    let mut chain = vec![i];
    while let Some(p) = w.items.get(*chain.last().unwrap()).and_then(|it| it.parent) {
        chain.push(p);
        if Some(p) == top {
            break;
        }
    }
    // Neither the window nor `top` has a node of its own here.
    chain.pop();
    let mut mat = Mat4::identity();
    for &c in chain.iter().rev() {
        let (x, y) = (num(w, c, "x"), num(w, c, "y"));
        let m = w.item_node_matrix(c, x, y);
        if m.flags != qml::valuetypes::IDENTITY {
            mat = mat.mul(&m);
        }
    }
    mat
}

/// A GaussianBlur.qml on the GPU: what `Frame::gauss` draws.
struct GpuGauss {
    /// The SourceProxy texture.
    proxy: crate::gpu::Frame,
    kernel: crate::gpu::Gauss,
    /// The texels of the horizontal pass's layer.
    layer: (usize, usize),
    /// The scene rect of the vertical pass.
    rect: [f64; 4],
    /// The scene rect `proxy` covers (`_outputRect`).
    output: [f64; 4],
}

/// GaussianBlur.qml of an effect at scene `at` (x y w h) over `src`, in a window of
/// devicePixelRatio `dpr`; None when it has no pixel to draw. Without transparentBorder the
/// proxy is the source's own layer, which the horizontal pass samples linearly over the effect
/// whatever their sizes; with it the proxy is the source 1:1 in sourceRect
/// (-radius, 0, `_paddedTexWidth`, height), and the horizontal pass's layer has sourceRect
/// (0, -radius, width, `_paddedTexHeight`). Every texture is its rect times `dpr`, and a tap
/// of the kernel is one device pixel (`dirstep`).
#[allow(clippy::too_many_arguments)]
fn gpu_gauss(
    w: &mut World,
    fonts: &mut Fonts,
    src: usize,
    [x, y, wd, ht]: [f64; 4],
    radius: f64,
    samples: i32,
    deviation: Option<f64>,
    tb: bool,
    dpr: f64,
    shadow: Option<([f32; 4], f32)>,
    direct: Option<Rc<Canvas>>,
) -> Option<GpuGauss> {
    let r = if tb { qsg::blur::border(radius) } else { 0.0 };
    // `_paddedTexWidth` and `_paddedTexHeight`: int properties, truncated.
    let (pw, ph) = (qsg::blur::padded(wd, r), qsg::blur::padded(ht, r));
    if pw == 0 || ph == 0 {
        return None;
    }
    let (pwf, phf) = (pw as f64, ph as f64);
    let (proxy, rect, output, band) = if tb {
        let proxy = gpu_source(w, fonts, src, [-r, 0.0, pwf, ht], dpr);
        let band = [(r / phf) as f32, (ht / phf) as f32];
        let rect = [x - r, y - r, pwf, phf];
        (proxy, rect, [x - r, y, pwf, ht], band)
    } else {
        let proxy = match direct {
            // The picture's texture: its texels one to one.
            Some(t) => {
                let mut f = crate::gpu::Frame::scaled(t.w, t.h, 1.0);
                let whole = [0.0, 0.0, t.w as f64, t.h as f64];
                f.image_unblended(t, whole, [0.0, 0.0, 1.0, 1.0], false);
                f
            }
            None => gpu_layer_dpr(w, fonts, src, dpr),
        };
        (proxy, [x, y, wd, ht], [x, y, wd, ht], [0.0, 1.0])
    };
    let layer = crate::gpu::Frame::sized(rect[2], rect[3], dpr);
    let (taps, spread, sum) = qsg::blur::kernel(radius, samples, deviation);
    let kernel = crate::gpu::Gauss {
        taps,
        spread,
        sum,
        per: [(pwf * dpr) as f32, (phf * dpr) as f32],
        band,
        shadow,
        first: Some(match tb {
            true => [pwf as f32, ht as f32, -r as f32, phf as f32],
            false => [wd as f32, ht as f32, 0.0, ht as f32],
        }),
    };
    Some(GpuGauss {
        proxy,
        kernel,
        layer: (layer.w, layer.h),
        rect,
        output,
    })
}

/// The window's devicePixelRatio when painting on the GPU; None on the CPU, whose canvas is 1x.
fn gpu_dpr(cv: &Target) -> Option<f64> {
    match cv {
        Target::Gpu(fr) => Some(fr.dpr),
        Target::Cpu(_) => None,
    }
}

/// The picture GaussianBlur `e` reads itself instead of a layer of its source `src`, as
/// `QGfxSourceProxy::updatePolish` has it for GaussianBlur.qml's proxy (linear, a sourceRect
/// only with transparentBorder): an Image with no children and no layer of its own, smooth
/// and of `fillMode: Stretch`, is the effect's source, a texture of the picture's own size.
fn gauss_direct(w: &mut World, e: usize, src: usize, tb: bool, cpu: bool) -> Option<Rc<Canvas>> {
    let own_layer = LAYER_SRC.with(|s| s.get()).is_some_and(|(le, _)| le == e);
    if tb || own_layer || !is(w, src, "Image") || truthy(w, src, "layer.enabled", false) {
        return None;
    }
    let smooth = !explicit(w, src, "smooth") || truthy(w, src, "smooth", true);
    if !smooth || truthy(w, src, "mipmap", false) || !visual_kids(w, src).is_empty() {
        return None;
    }
    let (wd, ht) = (num(w, src, "width"), num(w, src, "height"));
    let (file, ss, fk, _) = image_src(w, src, wd, ht, 1.0);
    if fk != 0 {
        return None;
    }
    let asynchronous = truthy(w, src, "asynchronous", false);
    let t = image_texture(image_loads(w), &file, ss, fk, asynchronous);
    match cpu {
        true => cpu_pixels(image_loads(w), &file, ss, fk, t),
        false => t,
    }
}

/// `gpu_gauss`'s proxy on the CPU: the source's layer `st` without transparentBorder, else
/// the source in sourceRect (-r, 0, `_paddedTexWidth`, height).
fn cpu_gauss_proxy(
    w: &mut World,
    fonts: &mut Fonts,
    st: &Layer,
    src: usize,
    wd: f64,
    ht: f64,
    r: f64,
) -> Rc<Canvas> {
    if r == 0.0 {
        return st.px.clone();
    }
    let pw = qsg::blur::padded(wd, r) as f64;
    Rc::new(cpu_source(w, fonts, src, [-r, 0.0, pw, ht]))
}

thread_local! {
    /// The size in pixels of the window the CPU raster is painting, outside its layers.
    static CPU_WINDOW: std::cell::Cell<Option<(usize, usize)>> = const { std::cell::Cell::new(None) };
}

/// `CPU_WINDOW` as it was, put back when dropped.
struct CpuWindow(Option<(usize, usize)>);

impl CpuWindow {
    fn set(size: Option<(usize, usize)>) -> Self {
        CpuWindow(CPU_WINDOW.with(|c| c.replace(size)))
    }
}

impl Drop for CpuWindow {
    fn drop(&mut self) {
        CPU_WINDOW.with(|c| c.set(self.0));
    }
}

/// Where Qt draws the last pass of the effect `i` with the border `r`, in the window the CPU
/// raster is painting: QSGRenderer's projection of the window times the node's matrix.
fn cpu_placed(w: &mut World, i: usize, r: f64) -> Option<qsg::blur::Placed> {
    let (tw, th) = CPU_WINDOW.with(|c| c.get())?;
    let mut mv = node_matrix(w, i);
    if r != 0.0 {
        let mut at = Mat4::identity();
        at.translate(-r as f32, -r as f32, 0.0);
        mv = mv.mul(&at);
    }
    let m = Mat4::ortho(0.0, tw as f32, th as f32, 0.0, 1.0, -1.0).mul(&mv);
    Some(qsg::blur::Placed {
        m: std::array::from_fn(|i| m.m[i / 4][i % 4]),
        target: (tw, th),
    })
}

/// Draws an effect of the size `b` on the CPU target through `f`, unless a texture of that size
/// cannot be (on the GPU target an effect is shader passes over its source's texture).
fn raster_cpu(cv: &mut Target, b: (f64, f64), f: impl FnOnce(&mut Canvas)) {
    if let Target::Cpu(c) = cv
        && cpu_texture_fits(b.0, b.1)
    {
        f(c);
    }
}

pub fn paint_window(rt: &mut Runtime, fonts: &mut Fonts, win: usize, cv: &mut Target) {
    follow_theme();
    let dpr = match cv {
        Target::Gpu(fr) => fr.dpr,
        Target::Cpu(_) => 1.0,
    };
    polish_icons(rt, win, dpr);
    let mut w = rt.world.borrow_mut();
    stop_hidden_progress(&mut w);
    OVERLAY.with(|o| o.borrow_mut().clear());
    let _window = match cv {
        Target::Cpu(c) => Some(qsg::raster::Window::new(c)),
        Target::Gpu(_) => None,
    };
    let size = match cv {
        Target::Cpu(c) => Some((c.w, c.h)),
        Target::Gpu(_) => None,
    };
    let _size = CpuWindow::set(size);
    // QsWindow.color: the window's clear color under its content.
    let bg = color(&mut w, win, "color", [0, 0, 0, 0]);
    if bg[3] > 0 {
        // All of the buffer, which a fractional scale or a screen's factor makes a fraction
        // of a scene unit larger or smaller than the window: the units Qt's projection is over.
        let (bw, bh) = match cv {
            Target::Cpu(_) => (num(&mut w, win, "width"), num(&mut w, win, "height")),
            Target::Gpu(f) => f.units(),
        };
        let g = qsg::rect::build(&qsg::rect::RectParams {
            width: bw as f32,
            height: bh as f32,
            color: bg,
            ..Default::default()
        });
        match cv {
            Target::Cpu(c) => c.draw_smooth(&g, 0.0, 0.0, 1.0),
            Target::Gpu(f) => f.smooth(&Rc::new(g), 0.0, 0.0, 1.0),
        }
    }
    df_prepare(&mut w, fonts, win);
    let world = std::sync::Arc::downgrade(&w.jobs_stop.0);
    if !DF_WORLD.with(|d| d.borrow().ptr_eq(&world)) {
        DF_WORLD.with(|d| *d.borrow_mut() = world);
        DF_MOVED.with(|m| m.borrow_mut().clear());
        SG_TEXTS.with(|t| t.borrow_mut().clear());
    }
    let pass = DF_PASS.with(|p| {
        let dpr = match cv {
            Target::Cpu(_) => 1.0,
            Target::Gpu(f) => f.dpr,
        };
        p.set((win, p.get().1 + 1, dpr));
        p.get().1
    });
    DF_ITEMS.with(|m| m.borrow_mut().clear());
    DF_TURNED.with(|m| m.borrow_mut().clear());
    let top = DfNode {
        mat: Mat4::identity(),
        root: Mat4::identity(),
        local: Mat4::identity(),
        root_of: (win, ROOT_WINDOW),
        pos: [0.0; 2],
        plain: true,
        free: true,
        opacity: 1.0,
        merged: true,
        fill_merged: true,
    };
    let mut texts = Vec::new();
    for c in kids(&mut w, win) {
        df_nodes(&mut w, fonts, c, &top, 1.0, None, &mut texts);
    }
    // A node that left the tree is another node when it comes back.
    DF_MOVED.with(|m| m.borrow_mut().retain(|_, n| n.win != win || n.pass == pass));
    df_batches(&texts);
    let on = DL_ON.with(|d| d.replace(matches!(cv, Target::Gpu(_))));
    DL_FRAME.with(|f| f.set(f.get() + 1));
    for c in kids(&mut w, win) {
        paint(&mut w, fonts, c, cv, 0.0, 0.0, 1.0, false);
    }
    DL_ON.with(|d| d.set(on));
    // QQuickOverlay: popups and tooltips are reparented to the window overlay, above every item and
    // outside ancestor clips, layers and opacity.
    // Popup items join the overlay when created: stacked by z, then creation order.
    // Popups inside a popup join the overlay too, after (above) the popup that holds them.
    let mut done = std::collections::HashSet::new();
    loop {
        let mut stack: Vec<(f64, usize)> = OVERLAY
            .with(|o| std::mem::take(&mut *o.borrow_mut()))
            .into_iter()
            .filter(|&c| done.insert(c))
            .map(|c| (num(&mut w, c, "z"), c))
            .collect();
        if stack.is_empty() {
            break;
        }
        stack.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        for (_, c) in stack {
            let Some(p) = w.items[c].parent else { continue };
            // QQuickPopupPositioner follows the parent's scene geometry while the popup is shown.
            if is(&w, c, "ToolTip") {
                place_tooltip(&mut w, p, c);
            }
            let (px, py) = w.scene_pos(p, Some(win));
            let py = py + popup_flip_dy(&mut w, c, win);
            IN_OVERLAY.with(|f| f.set(true));
            paint(&mut w, fonts, c, cv, px, py, 1.0, false);
            IN_OVERLAY.with(|f| f.set(false));
        }
    }
}

/// Qt builds the paint node of every dirty item before it draws one, and a QtRendering text
/// puts its glyphs into the font's glyph cache there. Where a glyph lies in that texture and
/// how large it has grown reach the pixels, so the glyphs of the texts under `i` enter the
/// cache here, in the order of QQuickWindowPrivate::updateDirtyNodes: the dirty list is
/// filled as items get their window (children, then the item) and is walked from its end.
fn df_prepare(w: &mut World, fonts: &mut Fonts, i: usize) {
    if is_text(w, i) && qt_rendering(w, i) {
        let ts = text_style(w, fonts, i);
        let q = df_text(w, i, false).quality;
        if let Some(f) = ts.font {
            let lines = plain_laid(&f, &ts.text, &text_bounds(w, i, ts.doc, None)).lines;
            f.lines_df_populate(&lines, q);
        }
    }
    for c in kids(w, i).into_iter().rev() {
        df_prepare(w, fonts, c);
    }
}

/// A QtRendering Text as the batch renderer sees it.
struct DfElement {
    item: usize,
    /// What QSGDistanceFieldTextMaterial::compare and prepareAlphaBatches tell texts apart
    /// by: font and size, quality, colour, material and style colour, inherited opacity, clip.
    key: (u64, i32, [u8; 4], u32, [u8; 4], u32, Option<usize>),
    /// The batch root it lies under.
    root: (usize, u8),
    /// The fill node of outlined text.
    fill: bool,
    /// Bounds in the window: left, top, right, bottom.
    bounds: [f32; 4],
    moved_only: bool,
}

/// The transform nodes of the items under `i` (QQuickWindowPrivate::updateDirtyNode: the
/// position, then scale and rotation about the origin, each a QMatrix4x4 call) and the
/// QtRendering texts among them, in drawing order.
#[allow(clippy::too_many_arguments)]
fn df_nodes(
    w: &mut World,
    fonts: &mut Fonts,
    i: usize,
    up: &DfNode,
    opacity: f32,
    clip: Option<usize>,
    out: &mut Vec<DfElement>,
) {
    if !visible(w, i) {
        return;
    }
    let (x, y) = (num(w, i, "x"), num(w, i, "y"));
    let m = w.item_node_matrix(i, x, y);
    // `paint_transformed` draws an item that its transform only moves where it lands.
    let moved = w
        .item_transform(i)
        .and_then(|t| t.as_translation())
        .unwrap_or((0.0, 0.0));
    let under = |up: &Mat4| {
        if m.flags == qml::valuetypes::IDENTITY {
            *up
        } else {
            up.mul(&m)
        }
    };
    let mut node = DfNode {
        mat: under(&up.mat),
        root: up.root,
        local: under(&up.local),
        root_of: up.root_of,
        pos: [up.pos[0] + x + moved.0, up.pos[1] + y + moved.1],
        plain: up.plain,
        free: up.free,
        opacity,
        merged: true,
        fill_merged: true,
    };
    // Its transform node is a batch root: what lies under it, its own paint node too, is
    // placed from there (Updater::visitTransformNode).
    if df_moved(w, fonts, i, &m) {
        node.root = up.root.mul(&up.local).mul(&m);
        node.local = Mat4::identity();
        node.root_of = (i, ROOT_ITEM);
    }
    if up.plain && m.flags > 1 {
        DF_TURNED.with(|t| t.borrow_mut().insert(i, [up.pos[0] + x, up.pos[1] + y]));
    }
    let opacity = opacity * num(w, i, "opacity") as f32;
    if is_text(w, i) && qt_rendering(w, i) {
        let ts = text_style(w, fonts, i);
        if let Some(f) = ts.font {
            let col = color(w, i, "color", [0, 0, 0, 255]);
            let (wd, ht) = (num(w, i, "width"), num(w, i, "height"));
            let px = f.font(0).map_or(0, |x| x.pixel_size) as f64;
            let gray = df_node_recycled(i, &ts.text, col, px, wd, ht);
            // The glyph quads reach a little past the item.
            let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
            for (cx, cy) in [
                (-3.0, -3.0),
                (wd + 3.0, -3.0),
                (-3.0, ht + 3.0),
                (wd + 3.0, ht + 3.0),
            ] {
                let [px, py, _] = node.mat.map3([cx as f32, cy as f32, 0.0]);
                b = [b[0].min(px), b[1].min(py), b[2].max(px), b[3].max(py)];
            }
            let d = df_text(w, i, gray);
            // 0 the subpixel material, 1 the gray one, then one per style.
            let kind = if d.style == 0 {
                gray as u32
            } else {
                d.style + 1
            };
            let style_color = if d.style == 0 { [0; 4] } else { d.style_color };
            let (font, op) = (ts.font_key.to_bits(), opacity.to_bits());
            let mut push = |kind, style_color, fill| {
                out.push(DfElement {
                    item: i,
                    key: (font, d.quality, col, kind, style_color, op, clip),
                    root: node.root_of,
                    fill,
                    bounds: b,
                    moved_only: node.mat.flags <= 1,
                })
            };
            push(kind, style_color, false);
            if d.style == 1 && col[3] > 0 && d.style_color != col {
                push(1, [0; 4], true);
            }
            DF_ITEMS.with(|m| m.borrow_mut().insert(i, node));
        }
    }
    let clip = if truthy(w, i, "clip", false) {
        Some(i)
    } else {
        clip
    };
    let plain = node.plain && m.flags <= 1;
    let mut down = DfNode {
        plain,
        free: node.free && (plain || clip.is_none_or(|c| c != i)),
        ..node
    };
    // The item's clip node, under its transform node, is the root of what it holds
    // (Updater::visitClipNode).
    if clip == Some(i) {
        down.root = node.root.mul(&node.local);
        down.local = Mat4::identity();
        down.root_of = (i, ROOT_CLIP);
    }
    for c in stacked_kids(w, i) {
        df_nodes(w, fonts, c, &down, opacity, clip, out);
    }
}

/// The window, an item's transform node, its clip node. A Flickable's contentItem is an item.
const ROOT_WINDOW: u8 = 0;
const ROOT_ITEM: u8 = 1;
const ROOT_CLIP: u8 = 2;

/// A transform node of the window as the batch renderer keeps it.
struct DfMoved {
    /// Its matrix when the window was last drawn.
    mat: [u32; 16],
    root: bool,
    win: usize,
    pass: u64,
}

thread_local! {
    /// The transform nodes of the windows' items while they stay in the scene.
    static DF_MOVED: std::cell::RefCell<HashMap<usize, DfMoved>> = Default::default();
    /// The World those are of: another one numbers its items from 0 again.
    static DF_WORLD: std::cell::RefCell<std::sync::Weak<qml::svc::Stop>> = Default::default();
    /// The window being drawn, how many have been, and its screen's scale.
    static DF_PASS: std::cell::Cell<(usize, u64, f64)> = const { std::cell::Cell::new((0, 0, 1.0)) };
}

/// QSG_RENDERER_BATCH_NODE_THRESHOLD and QSG_RENDERER_BATCH_VERTEX_THRESHOLD, unset.
const BATCH_NODES: usize = 64;
const BATCH_VERTICES: usize = 1024;

/// Renderer::nodeChanged: a transform node whose matrix changes while more than 64 geometry
/// nodes or more than 1024 vertices lie under it becomes a batch root, and stays one. The
/// renderer then hands the shader the matrix down to that node and moves the vertices only
/// from there, which rounds differently. `m` is the matrix of item `i`.
/// A node new to the scene gets its matrix before anything lies under it.
fn df_moved(w: &mut World, fonts: &mut Fonts, i: usize, m: &Mat4) -> bool {
    let mut mat = [0u32; 16];
    for (to, v) in mat.iter_mut().zip(m.m.iter().flatten()) {
        *to = v.to_bits();
    }
    let (win, pass, _) = DF_PASS.with(|p| p.get());
    let was = DF_MOVED.with(|n| n.borrow().get(&i).map(|n| (n.mat, n.root)));
    let root = match was {
        Some((_, true)) => true,
        Some((old, false)) if old != mat => {
            let mut held = (0, 0);
            sg_held(w, fonts, i, &mut held);
            sg_full(&held)
        }
        _ => false,
    };
    DF_MOVED.with(|n| {
        let node = DfMoved {
            mat,
            root,
            win,
            pass,
        };
        n.borrow_mut().insert(i, node)
    });
    root
}

thread_local! {
    /// The glyph nodes of the texts `sg_held` has counted, by what they were counted from.
    static SG_TEXTS: std::cell::RefCell<HashMap<usize, (u64, SgNodes)>> = Default::default();
}

/// Geometry nodes: how many of a kind, and the vertices of them all.
type SgNodes = Vec<(usize, usize)>;

/// The glyph nodes of text `i`, one entry a font. They are kept while the text, its font
/// and its size stay: a text that only moves is not laid out again for this.
fn sg_text(w: &mut World, fonts: &mut Fonts, i: usize, wd: f64, ht: f64) -> SgNodes {
    use std::hash::{Hash, Hasher};
    let ts = text_style(w, fonts, i);
    let Some(f) = ts.font else {
        return vec![];
    };
    let qt = qt_rendering(w, i);
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (
        &ts.text,
        ts.font_key.to_bits(),
        wd.to_bits(),
        ht.to_bits(),
        qt,
    )
        .hash(&mut h);
    let key = h.finish();
    let kept = SG_TEXTS.with(|t| {
        t.borrow()
            .get(&i)
            .filter(|k| k.0 == key)
            .map(|k| k.1.clone())
    });
    if let Some(kept) = kept {
        return kept;
    }
    let lines = plain_laid(&f, &ts.text, &text_bounds(w, i, ts.doc, None)).lines;
    let mut out = vec![];
    if qt {
        let d = df_text(w, i, false);
        let col = color(w, i, "color", [0, 0, 0, 255]);
        // QSGInternalTextNode::addGlyphs: outlined text has a fill node as well.
        let nodes = 1 + (d.style == 1 && col[3] > 0 && d.style_color != col) as usize;
        for quads in f.lines_df_quads(&lines, d.quality) {
            out.push((nodes, nodes * 4 * quads));
        }
    } else {
        let mut used: Vec<(usize, usize)> = vec![];
        for g in lines.iter().flat_map(|l| &l.glyphs) {
            match used.iter_mut().find(|u| u.0 == g.0) {
                Some(u) => u.1 += 1,
                None => used.push((g.0, 1)),
            }
        }
        out.extend(used.into_iter().map(|(_, glyphs)| (1, 4 * glyphs)));
    }
    SG_TEXTS.with(|t| {
        let mut t = t.borrow_mut();
        if t.len() > 4096 {
            t.clear();
        }
        t.insert(i, (key, out.clone()));
    });
    out
}

fn sg_full(held: &(usize, usize)) -> bool {
    held.0 > BATCH_NODES || held.1 > BATCH_VERTICES
}

/// Adds the geometry nodes of item `i` and of what it holds, and their vertices, to `held`,
/// until that is enough for a batch root. A Rectangle's and a QtRendering text's are Qt's
/// own; a NativeRendering text is four vertices a glyph, one node a font; anything else
/// that draws, and a layer, is taken for one node of four vertices.
fn sg_held(w: &mut World, fonts: &mut Fonts, i: usize, held: &mut (usize, usize)) {
    if sg_full(held) || !visible(w, i) {
        return;
    }
    let mut add = |nodes: usize, vertices: usize| *held = (held.0 + nodes, held.1 + vertices);
    // A layer's content is another renderer's; here it is the one textured node.
    if truthy(w, i, "layer.enabled", false) || external_source(w, i) {
        return add(1, 4);
    }
    let (wd, ht) = (num(w, i, "width"), num(w, i, "height"));
    if is(w, i, "Rectangle") {
        if wd > 0.0 && ht > 0.0 {
            let dpr = DF_PASS.with(|p| p.get()).2;
            let p = rect_params(w, i, wd, ht, dpr);
            // QQuickRectangle::updatePaintNode: nothing to fill or stroke, no node.
            if p.color[3] > 0 || p.pen_width > 0.0 || !p.stops.is_empty() {
                add(1, qsg::rect::build(&p).vertices.len());
            }
        }
    } else if is_text(w, i) {
        for (nodes, vertices) in sg_text(w, fonts, i, wd, ht) {
            add(nodes, vertices);
        }
    } else if ["Image", "Shape", "Canvas", "ShaderEffect"]
        .iter()
        .any(|t| w.items[i].inherits(t))
        && wd > 0.0
        && ht > 0.0
    {
        add(1, 4);
    }
    for c in visual_kids(w, i) {
        sg_held(w, fonts, c, held);
    }
}

/// Renderer::prepareAlphaBatches over the QtRendering texts of a window: a text joins the
/// batch of an earlier one like it unless something drawn between them lies over it. A batch
/// is merged, its vertices moved before the shader, only when every text in it is only
/// moved (QSGMaterial::RequiresFullMatrixExceptTranslate); otherwise each text's vertices go
/// through its whole matrix in the shader, which rounds differently.
fn df_batches(els: &[DfElement]) {
    let hit = |a: &[f32; 4], b: &[f32; 4]| b[0] < a[2] && b[2] > a[0] && b[1] < a[3] && b[3] > a[1];
    let mut batch: Vec<Option<usize>> = vec![None; els.len()];
    for i in 0..els.len() {
        if batch[i].is_some() {
            continue;
        }
        batch[i] = Some(i);
        let mut over = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
        let grow = |b: &[f32; 4], over: &mut [f32; 4]| {
            *over = [
                over[0].min(b[0]),
                over[1].min(b[1]),
                over[2].max(b[2]),
                over[3].max(b[3]),
            ];
        };
        for j in i + 1..els.len() {
            let b = &els[j].bounds;
            if els[j].root != els[i].root {
                break;
            }
            if batch[j].is_some() || els[j].key != els[i].key {
                grow(b, &mut over);
            } else if !hit(&over, b) || !(i + 1..j).any(|k| hit(&els[k].bounds, b)) {
                batch[j] = Some(i);
            } else {
                break;
            }
        }
    }
    DF_ITEMS.with(|m| {
        let mut m = m.borrow_mut();
        for (i, e) in els.iter().enumerate() {
            let merged = (0..els.len())
                .filter(|&j| batch[j] == batch[i])
                .all(|j| els[j].moved_only);
            match m.get_mut(&e.item) {
                Some(n) if e.fill => n.fill_merged = merged,
                Some(n) => n.merged = merged,
                None => {}
            }
        }
    });
}

thread_local! {
    /// QtRendering Text items: hash of what their paint node was last built from, and whether
    /// it has been rebuilt since (glyph nodes then come from the recycle bin).
    static DF_NODES: std::cell::RefCell<HashMap<usize, (u64, bool)>> = Default::default();
}

/// QSGDistanceFieldGlyphNode::recycle() resets the antialiasing mode to gray and
/// QSGInternalTextNode::findOrCreateGlyphNode() never re-applies the preferred subpixel mode,
/// so a QtRendering Text is subpixel-antialiased only until its first paint node update.
fn df_node_recycled(item: usize, text: &str, col: [u8; 4], px: f64, wd: f64, ht: f64) -> bool {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (text, col, px.to_bits(), wd.to_bits(), ht.to_bits()).hash(&mut h);
    let key = h.finish();
    DF_NODES.with(|m| {
        let mut m = m.borrow_mut();
        if m.len() > 4096 {
            m.clear();
        }
        let e = m.entry(item).or_insert((key, false));
        if e.0 != key {
            *e = (key, true);
        }
        e.1
    })
}

/// The glyph nodes QQuickTextNodeEngine::addToSceneGraph puts directly under the text node
/// of the plain-text `lines` whose chars `s0..s1` are selected, which are all the text in its
/// own colour: one for each font of what is not selected (mergeProcessedNodes merges it
/// across the text) and one for each font of each selected stretch of a line (which has a
/// clip node of its own), in the order they first appear. How many there are, and per line
/// the glyphs of each that has some there.
fn df_edit_nodes(
    lines: &[EditLine],
    s0: usize,
    s1: usize,
) -> (usize, Vec<Vec<(usize, qsg::text::ShapedLine)>>) {
    let mut nodes: Vec<(usize, Option<usize>)> = vec![];
    let mut stretches = 0;
    let per_line = lines
        .iter()
        .map(|el| {
            let len = el.text.chars().count();
            let (a, b) = (
                s0.clamp(el.start, el.start + len),
                s1.clamp(el.start, el.start + len),
            );
            let ranges = match a < b {
                true => el.ranges(a - el.start, b - el.start),
                false => vec![],
            };
            let mut parts: Vec<(usize, qsg::text::ShapedLine)> = vec![];
            let mut was = false;
            for g in &el.shaped.glyphs {
                let gx = g.2 as f64 / 64.0;
                let selected = ranges.iter().any(|(l, r)| (*l..*r).contains(&gx));
                stretches += (selected && !was) as usize;
                was = selected;
                let key = (g.0, selected.then_some(stretches));
                let node = nodes.iter().position(|k| *k == key).unwrap_or_else(|| {
                    nodes.push(key);
                    nodes.len() - 1
                });
                match parts.iter_mut().find(|(k, _)| *k == node) {
                    Some((_, part)) => part.glyphs.push(*g),
                    None => parts.push((
                        node,
                        qsg::text::ShapedLine {
                            glyphs: vec![*g],
                            last_cluster: 0,
                            advance: el.shaped.advance,
                            ascent: el.shaped.ascent,
                            descent: el.shaped.descent,
                            leading: el.shaped.leading,
                        },
                    )),
                }
            }
            parts
        })
        .collect();
    (nodes.len(), per_line)
}

/// What the text node of a TextInput is built from: QQuickTextInput::updatePaintNode builds
/// it again when its layout is dirty, which the text, the font, the width, the colour, the
/// scroll, the selection and, with a selection, its two colours make it.
fn df_edit_key(
    lines: &[EditLine],
    f: &FontStack,
    col: [u8; 4],
    wd: f64,
    sel: Option<(usize, usize, [u8; 4], [u8; 4])>,
) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let px = f.font(0).map_or(0, |x| x.pixel_size);
    (col, px, wd.to_bits(), sel).hash(&mut h);
    for el in lines {
        (&el.text, el.x.to_bits(), el.y.to_bits(), &el.shaped.glyphs).hash(&mut h);
    }
    h.finish()
}

thread_local! {
    /// QtRendering TextInputs: hash of what their text node was last built from
    /// (`df_edit_key`), and which of the glyph nodes directly under it were recycled ones.
    static DF_EDIT_NODES: std::cell::RefCell<HashMap<usize, (u64, Vec<bool>)>> = Default::default();
}

/// Which of the `count` glyph nodes directly under the text node of TextInput `item`, built
/// from `key`, are gray. QQuickTextInput::updatePaintNode puts the glyph nodes its text node
/// had in a recycle bin and QSGInternalTextNode::findOrCreateGlyphNode takes them from it in
/// order: QSGDistanceFieldGlyphNode::recycle() has made those gray, and a node made anew is
/// subpixel-antialiased. The nodes under the clip nodes of a selection are never recycled. A
/// TextEdit makes a new text node instead.
fn df_edit_recycled(item: usize, key: u64, count: usize) -> Vec<bool> {
    DF_EDIT_NODES.with(|m| {
        let mut m = m.borrow_mut();
        if m.len() > 4096 {
            m.clear();
        }
        let e = m.entry(item).or_insert((key, vec![false; count]));
        if e.0 != key {
            *e = (key, (0..count).map(|k| k < e.1.len()).collect());
        }
        e.1.clone()
    })
}

thread_local! {
    static OVERLAY: std::cell::RefCell<Vec<usize>> = const { std::cell::RefCell::new(Vec::new()) };
    static IN_OVERLAY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn trace_paint(w: &World, i: usize, x: f64, y: f64, wd: f64, ht: f64, op: f32) {
    thread_local! {
        static SEEN: std::cell::RefCell<HashMap<usize, String>> = Default::default();
    }
    let g = format!(
        "{:?} x {x:.2} y {y:.2} w {wd:.0} h {ht:.0} op {op:.2}",
        w.items[i].types
    );
    SEEN.with(|s| {
        if s.borrow().get(&i) != Some(&g) {
            tracing::debug!(target: "ri::paint", "paint #{i} {g}");
            s.borrow_mut().insert(i, g);
        }
    });
}

/// Recorded draws of item subtrees, by item: (where/how drawn, draws, epoch).
type DisplayLists = HashMap<usize, (DlKey, Rc<crate::gpu::Seg>, u64)>;

thread_local! {
    /// Recorded draws of item subtrees in the window frame, by item: (where/how drawn, draws).
    static DL: std::cell::RefCell<DisplayLists> = Default::default();
    /// Invalidation epoch: bumped whenever subtrees are invalidated (`DL_STAMP`).
    static DL_EPOCH: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    /// Per item: the epoch its subtree was last invalidated at. A recorded draw is stale if an
    /// ancestor-or-self was stamped after it was recorded (checked at lookup, not walked eagerly).
    static DL_STAMP: std::cell::RefCell<qml::runtime::ChunkVec<u64>> = const { std::cell::RefCell::new(qml::runtime::ChunkVec::new()) };
    /// Where a moving subtree was last painted: it is recorded again once it stops.
    static DL_MOVED: std::cell::RefCell<HashMap<usize, DlKey>> = Default::default();
    /// Painting a window's own frame (not an offscreen layer), so subtree draws can be reused.
    static DL_ON: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The subtree just painted drew something not covered by `touched` invalidation.
    static NOCACHE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Window frames painted so far (`DL_HOT` deadlines).
    static DL_FRAME: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    /// Recorded draws replayed at least once since recording.
    static DL_USED: std::cell::RefCell<rustc_hash::FxHashSet<usize>> = Default::default();
    /// Subtrees whose last recording went stale unused: not recorded again until the given frame,
    /// so an animating subtree (and its ancestors) is painted, not copied, every frame.
    static DL_HOT: std::cell::RefCell<HashMap<usize, u64>> = Default::default();
    static DLCHECK: bool = qml::debug::on(qml::debug::Switch::DLCHECK);
    static NODL: bool = qml::debug::on(qml::debug::Switch::NODL);
    /// (replayed, recorded, uncacheable) subtrees since the last `take_dl_stats`.
    static DL_STATS: std::cell::Cell<(u32, u32, u32)> = const { std::cell::Cell::new((0, 0, 0)) };
}

pub fn take_dl_stats() -> (u32, u32, u32, usize) {
    let (a, b, c) = DL_STATS.with(|s| s.replace((0, 0, 0)));
    (a, b, c, DL.with(|d| d.borrow().len()))
}

fn dl_stat(k: usize) {
    DL_STATS.with(|s| {
        let mut v = s.get();
        match k {
            0 => v.0 += 1,
            1 => v.1 += 1,
            _ => v.2 += 1,
        }
        s.set(v);
    });
}

thread_local! {
    /// Effect sources: item -> effects whose recorded draw samples it (outside their subtree).
    static DL_DEPS: std::cell::RefCell<HashMap<usize, Vec<usize>>> = Default::default();
}

fn dl_dep_on(src: usize, effect: usize) {
    if src == effect {
        return;
    }
    DL_DEPS.with(|d| {
        let mut d = d.borrow_mut();
        let v = d.entry(src).or_default();
        if !v.contains(&effect) {
            v.push(effect);
        }
    });
}

/// Records which items the effect `e` (drawn for item `owner`) samples.
fn dl_deps(w: &mut World, e: usize, owner: usize) {
    if e != owner {
        dl_dep_on(e, owner);
    }
    for k in ["source", "maskSource", "sourceItem"] {
        if let Some(Value::Item(s)) = w.read(e, k) {
            dl_dep_on(s, owner);
        }
    }
}

/// Drops recorded draws: `ups` with their ancestors, `downs` with their subtrees and ancestors,
/// and (transitively) effects sampling any dropped item.
fn dl_invalidate(w: &World, ups: &[usize], downs: &[usize]) {
    DL.with(|d| {
        let mut d = d.borrow_mut();
        if d.is_empty() {
            return;
        }
        DL_DEPS.with(|deps| {
            let deps = deps.borrow();
            let mut up: Vec<usize> = ups.to_vec();
            if !downs.is_empty() {
                let e = DL_EPOCH.with(|c| {
                    c.set(c.get() + 1);
                    c.get()
                });
                // A write to a destroyed instance touches it too (`World::write`), and by now
                // it may be released and its chunk dropped (`forget_chunks`): it has no subtree
                // to stamp, and a stamp would allocate that chunk again for good.
                DL_STAMP.with_borrow_mut(|st| {
                    for &t in downs.iter().filter(|&&t| w.items.holds(t)) {
                        *st.get_mut(t) = e;
                    }
                });
                up.extend_from_slice(downs);
                // Effects sampling an item inside a stamped subtree go stale with it.
                let set: rustc_hash::FxHashSet<usize> = downs.iter().copied().collect();
                for (&src, es) in deps.iter() {
                    let mut a = Some(src);
                    while let Some(x) = a {
                        if set.contains(&x) {
                            up.extend(es.iter().copied());
                            break;
                        }
                        a = w.items.get(x).and_then(|it| it.parent);
                    }
                }
            }
            let mut seen_up = rustc_hash::FxHashSet::default();
            while let Some(t) = up.pop() {
                let mut cur = Some(t);
                while let Some(i) = cur {
                    if !seen_up.insert(i) {
                        break;
                    }
                    if d.remove(&i).is_some() {
                        if DLDBG_ON.with(|d| *d) {
                            let ty = |k: usize| w.items.get(k).and_then(|it| it.types.first().cloned());
                            tracing::debug!(target: "ri::dldbg", "dldrop #{i} {:?} via #{t} {:?} down {}", ty(i), ty(t), downs.contains(&t));
                        }
                        dl_dropped(i);
                    }
                    if let Some(es) = deps.get(&i) {
                        up.extend(es.iter().copied());
                    }
                    cur = w.items.get(i).and_then(|it| it.parent);
                }
            }
        });
    });
}

/// Item position (scene x, y), opacity, clip, frame size, layer.
type DlKey = (
    u64,
    u64,
    u32,
    Option<[i64; 4]>,
    usize,
    usize,
    usize,
    [u32; 4],
);

thread_local!(static DLDBG_ON: bool = qml::log_on!("ri::dldbg"));

fn xlate_on() -> bool {
    !qml::debug::on(qml::debug::Switch::NOXLATE)
}

/// A recorded draw of `i` was dropped: if it was never replayed, recording it was wasted.
fn dl_dropped(i: usize) {
    if !DL_USED.with_borrow_mut(|u| u.remove(&i)) {
        let until = DL_FRAME.with(|f| f.get()) + 8;
        DL_HOT.with_borrow_mut(|h| h.insert(i, until));
    }
}

fn dl_hot(i: usize) -> bool {
    let now = DL_FRAME.with(|f| f.get());
    DL_HOT.with_borrow_mut(|h| match h.get(&i) {
        Some(&t) if t > now => true,
        Some(_) => {
            h.remove(&i);
            false
        }
        None => false,
    })
}

/// Whether a subtree of `draws` draws is recorded: two or more, or a single one (a text, a
/// rectangle) whose parent is painted every frame because something else under it animates.
/// Without it everything still beside a visualizer is laid out and built again every frame.
fn dl_worth(w: &World, i: usize, draws: usize) -> bool {
    draws >= 2 || draws == 1 && w.items[i].parent.is_some_and(dl_hot)
}

/// Whether `i` or an ancestor had its subtree invalidated after epoch `at`.
fn dl_stale(w: &World, i: usize, at: u64) -> bool {
    if DL_EPOCH.with(|c| c.get()) == at {
        return false;
    }
    DL_STAMP.with_borrow(|st| {
        let mut a = Some(i);
        while let Some(x) = a {
            if st.get(x) > at {
                return true;
            }
            a = w.items.get(x).and_then(|it| it.parent);
        }
        false
    })
}

fn no_cache() {
    NOCACHE.with(|n| n.set(true));
}

/// Drop every recorded subtree draw (state outside the item tree changed).
pub fn forget_draws() {
    DL.with(|d| d.borrow_mut().clear());
    DL_MOVED.with(|d| d.borrow_mut().clear());
    DL_DEPS.with(|d| d.borrow_mut().clear());
    DL_USED.with_borrow_mut(|u| u.clear());
    DL_HOT.with_borrow_mut(|h| h.clear());
}

/// Effects whose sources may live outside their own subtree.
fn external_source(w: &mut World, i: usize) -> bool {
    [
        "MultiEffect",
        "OpacityMask",
        "GaussianBlur",
        "DropShadow",
        "Desaturate",
        "ColorOverlay",
        "RadialGradient",
        "ShaderEffectSource",
    ]
    .iter()
    .any(|t| is(w, i, t))
}

thread_local! {
    /// `Frame::push_place` for the CPU raster: where Qt's renderer has each item being drawn,
    /// and the canvas it is drawn on (a layer's canvas starts from Qt's root again).
    static CPU_PLACES: std::cell::RefCell<Vec<(usize, qsg::raster::Place)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// The innermost place pushed for the canvas `c`.
fn cpu_top(c: &Canvas) -> Option<qsg::raster::Place> {
    let id = c as *const Canvas as usize;
    CPU_PLACES.with_borrow(|p| p.last().filter(|p| p.0 == id).map(|p| p.1))
}

/// The place Qt's renderer has what is drawn at (`x`, `y`) of the canvas `c`, if it is the
/// item being painted.
fn cpu_place(c: &Canvas, x: f64, y: f64) -> Option<qsg::raster::Place> {
    cpu_top(c).filter(|p| p.is_at([x, y]))
}

/// Paints the item `i`, whose parent's origin is at (`ox`, `oy`) of the target, and its
/// subtree, at the place Qt's renderer has each of them (`qsg::raster::Place`).
#[allow(clippy::too_many_arguments)]
fn paint(
    w: &mut World,
    fonts: &mut Fonts,
    i: usize,
    cv: &mut Target,
    ox: f64,
    oy: f64,
    opacity: f32,
    force: bool,
) {
    let own = [num(w, i, "x"), num(w, i, "y")];
    // The item again, drawn through its transform: where it is now, and no node of its own.
    let (origin, own, clip) = if XFORM_SKIP.with(|s| s.get()) == Some(i) {
        ([ox + own[0], oy + own[1]], [0.0; 2], false)
    } else {
        ([ox, oy], own, truthy(w, i, "clip", false))
    };
    match cv {
        Target::Gpu(f) => f.push_place(origin, own, clip),
        Target::Cpu(c) => {
            let top = cpu_top(c).unwrap_or(qsg::raster::Place::origin([0.0; 2]));
            let extra = [origin[0] - top.at[0], origin[1] - top.at[1]];
            let id = &**c as *const Canvas as usize;
            CPU_PLACES.with_borrow_mut(|p| p.push((id, top.step(extra, own, clip))));
        }
    }
    paint_recorded(w, fonts, i, cv, ox, oy, opacity, force);
    match cv {
        Target::Gpu(f) => f.pop_place(),
        Target::Cpu(_) => CPU_PLACES.with_borrow_mut(|p| {
            p.pop();
        }),
    }
}

/// `paint` with the draws of the subtree recorded, or replayed when nothing but its position
/// changed.
#[allow(clippy::too_many_arguments)]
fn paint_recorded(
    w: &mut World,
    fonts: &mut Fonts,
    i: usize,
    cv: &mut Target,
    ox: f64,
    oy: f64,
    opacity: f32,
    force: bool,
) {
    let key = match cv {
        Target::Gpu(fr)
            if !force
                && DL_ON.with(|d| d.get())
                && !NODL.with(|n| *n)
                && XFORM_SKIP.with(|s| s.get()) != Some(i)
                && (LAYER_DL.with(|s| s.get()).is_some()
                    || LAYER_SKIP.with(|s| s.get()).is_none()
                        && LAYER_SRC.with(|s| s.get()).is_none())
                && !IN_OVERLAY.with(|f| f.get()) =>
        {
            Some((
                (
                    (ox + num(w, i, "x")).to_bits(),
                    (oy + num(w, i, "y")).to_bits(),
                    opacity.to_bits(),
                    fr.clip_top(),
                    fr.w,
                    fr.h,
                    LAYER_DL.with(|s| s.get()).unwrap_or(usize::MAX),
                    // The last bits of where Qt's renderer has it, which the draws follow.
                    {
                        let p = fr.place();
                        [p.root[0], p.root[1], p.rel[0], p.rel[1]].map(f32::to_bits)
                    },
                ),
                fr.begin(),
            ))
        }
        _ => None,
    };
    let Some((key, start)) = key else {
        thread_local!(static DLDBG2: bool = qml::log_on!("ri::dldbg"));
        if DLDBG2.with(|d| *d) && matches!(cv, Target::Gpu(_)) {
            tracing::debug!(
                target: "ri::dldbg",
                "dlskip #{i} {:?} force {force} on {} x {:?} ls {:?} src {:?} ov {}",
                w.items[i].types.first(),
                DL_ON.with(|d| d.get()),
                XFORM_SKIP.with(|s| s.get()),
                LAYER_SKIP.with(|s| s.get()),
                LAYER_SRC.with(|s| s.get()),
                IN_OVERLAY.with(|f| f.get())
            );
        }
        return paint_timed(w, fonts, i, cv, ox, oy, opacity, force);
    };
    let mut shift = None;
    let why: String = if DLDBG_ON.with(|d| *d) {
        DL.with(|d| match d.borrow().get(&i) {
            None => "none".into(),
            Some(e) => format!(
                "stale {} tr {} dpos {} dop {} dclip {} dsz {} dlay {}",
                dl_stale(w, i, e.2),
                e.1.translatable(),
                (e.0.0, e.0.1) != (key.0, key.1),
                e.0.2 != key.2,
                e.0.3 != key.3,
                (e.0.4, e.0.5) != (key.4, key.5),
                e.0.6 != key.6
            ),
        })
    } else {
        String::new()
    };
    let (hit, moved_dl, stale) = DL.with(|d| match d.borrow().get(&i) {
        Some(e) if dl_stale(w, i, e.2) => (None, false, true),
        Some(e) if e.0 == key => (Some(e.1.clone()), false, false),
        // Only the position (and the enclosing clip, applied when redrawn) changed: redraw the
        // recorded calls there.
        Some(e)
            if xlate_on()
                && e.1.translatable()
                && (e.0.2, e.0.4, e.0.5, e.0.6) == (key.2, key.4, key.5, key.6) =>
        {
            let at = |k: &DlKey| (f64::from_bits(k.0), f64::from_bits(k.1));
            let ((x0, y0), (x1, y1)) = (at(&e.0), at(&key));
            shift = Some((x1 - x0, y1 - y0));
            (Some(e.1.clone()), false, false)
        }
        Some(_) => (None, true, false),
        None => (None, false, false),
    });
    if stale {
        DL.with(|d| d.borrow_mut().remove(&i));
        dl_dropped(i);
    }
    if let Some(seg) = &hit
        && !DLCHECK.with(|c| *c)
    {
        if let Target::Gpu(fr) = cv {
            match shift {
                Some((dx, dy)) => {
                    fr.replay_at(seg, dx, dy);
                }
                None => fr.replay(seg),
            }
        }
        DL_USED.with_borrow_mut(|u| u.insert(i));
        dl_stat(0);
        return;
    }
    let anim = ANIMATING.with(|a| a.replace(false));
    let nc = NOCACHE.with(|n| n.replace(false));
    paint_timed(w, fonts, i, cv, ox, oy, opacity, force);
    let anim2 = ANIMATING.with(|a| a.get());
    let nc2 = NOCACHE.with(|n| n.get()) || anim2;
    ANIMATING.with(|a| a.set(anim || anim2));
    NOCACHE.with(|n| n.set(nc || nc2));
    if nc2 {
        dl_stat(2);
    }
    let Target::Gpu(fr) = cv else { return };
    thread_local!(static DLDBG: bool = qml::log_on!("ri::dldbg"));
    if DLDBG.with(|d| *d) && hit.is_none() {
        tracing::debug!(
            target: "ri::dldbg",
            "dlmiss #{i} {:?} nc {nc2} sig {} rec {:?} why {why} at {:?}",
            w.items[i].types.first(),
            fr.sig_since(&start).len(),
            fr.record(&start).map(|s| s.translatable()),
            fr.opaque_at().map(|l| l.line())
        );
    }
    if let Some(seg) = &hit {
        let cached = match shift {
            Some((dx, dy)) => {
                let mut t = fr.scratch();
                let st = t.begin();
                t.replay_at(seg, dx, dy);
                t.sig_since(&st).to_vec()
            }
            None => seg.sig().to_vec(),
        };
        if fr.sig_since(&start) != cached {
            let par = w.items[i]
                .parent
                .and_then(|p| w.items[p].types.first().cloned());
            let props: Vec<String> = ["opacity", "color", "radius", "width", "height", "visible"]
                .iter()
                .map(|k| format!("{k}={:?}", w.read(i, k)))
                .collect();
            tracing::warn!(
                target: "ri::check",
                "dlcheck #{i} {:?} <{par:?} stale (shift {shift:?}): cached {:?} fresh {:?} {props:?}",
                w.items[i].types.first(),
                cached,
                fr.sig_since(&start)
            );
        }
        return;
    }
    // A subtree painted somewhere else than last time is animating: skip recording until it
    // is painted at the same place twice.
    let moved = moved_dl || DL_MOVED.with(|m| m.borrow().get(&i).is_some_and(|k| *k != key));
    let seg = (!nc2 && dl_worth(w, i, fr.sig_since(&start).len()) && !dl_hot(i))
        .then(|| fr.record(&start))
        .flatten();
    if DLDBG_ON.with(|d| *d) && seg.is_none() {
        tracing::debug!(
            target: "ri::dldbg",
            "dlnorec #{i} {:?} nc {nc2} sig {} hot {}",
            w.items[i].types.first(),
            fr.sig_since(&start).len(),
            dl_hot(i)
        );
    }
    // A translatable draw is kept while it moves: it is redrawn at each new position.
    if moved && !seg.as_ref().is_some_and(|s| xlate_on() && s.translatable()) {
        DL.with(|d| d.borrow_mut().remove(&i));
        DL_MOVED.with(|m| m.borrow_mut().insert(i, key));
        return;
    }
    if let Some(seg) = seg {
        DL_MOVED.with(|m| m.borrow_mut().remove(&i));
        DL_USED.with_borrow_mut(|u| u.remove(&i));
        dl_stat(1);
        DL.with(|d| {
            let mut d = d.borrow_mut();
            if d.len() > 20000 {
                d.clear();
            }
            d.insert(i, (key, Rc::new(seg), DL_EPOCH.with(|c| c.get())));
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_timed(
    w: &mut World,
    fonts: &mut Fonts,
    i: usize,
    cv: &mut Target,
    ox: f64,
    oy: f64,
    opacity: f32,
    force: bool,
) {
    thread_local!(static PPROF: bool = qml::log_on!("ri::pprof"));
    if !PPROF.with(|p| *p) {
        return paint_raw(w, fonts, i, cv, ox, oy, opacity, force);
    }
    // `ri::pprof`: items whose subtree took >= 3 ms (`RI_DEBUG=pprof_us=<us>`) to paint.
    let t = std::time::Instant::now();
    paint_raw(w, fonts, i, cv, ox, oy, opacity, force);
    thread_local!(static PPROF_US: u128 = qml::debug::opts().pprof_us.unwrap_or(3000));
    if t.elapsed().as_micros() >= PPROF_US.with(|v| *v) {
        tracing::debug!(
            target: "ri::pprof",
            "pprof {i} {:?} {}us",
            w.items[i].types.first(),
            t.elapsed().as_micros()
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_raw(
    w: &mut World,
    fonts: &mut Fonts,
    i: usize,
    cv: &mut Target,
    ox: f64,
    oy: f64,
    opacity: f32,
    force: bool,
) {
    if !force && !visible(w, i) {
        return;
    }
    // The texture of an item's own layer is its subtree under its opacity node (the root node
    // QQuickItemPrivate gives a referenced item): the opacity is the layer's, or its effect's.
    // A source's texture holds the item without its own opacity too (see `Untransformed`).
    let own = if LAYER_SKIP.with(|s| s.get()) == Some(i) || SOURCE_ROOT.with(|s| s.get()) == Some(i)
    {
        1.0
    } else {
        w.read(i, "opacity").map_or(1.0, |v| v.to_number() as f32)
    };
    let op = opacity * own;
    if op <= 0.0 {
        return;
    }
    let (x, y) = (ox + num(w, i, "x"), oy + num(w, i, "y"));
    let (wd, ht) = (num(w, i, "width"), num(w, i, "height"));
    thread_local!(static TRACE: bool = qml::log_on!("ri::paint"));
    if TRACE.with(|t| *t) {
        trace_paint(w, i, x, y, wd, ht, op);
    }
    if df_later(w, fonts, i, cv) {
        return;
    }
    if XFORM_SKIP.with(|s| s.get()) != Some(i) && paint_transformed(w, fonts, i, cv, x, y, opacity)
    {
        return;
    }
    if DL_ON.with(|d| d.get()) && external_source(w, i) {
        dl_deps(w, i, i);
    }
    // Item.layer.effect: the item renders into a texture that becomes the effect's source.
    if LAYER_SKIP.with(|s| s.get()) != Some(i)
        && truthy(w, i, "layer.enabled", false)
        && let Some(Value::Item(e)) = w.read(i, qml::runtime::LAYER_EFFECT)
        && ["OpacityMask", "MultiEffect", "DropShadow", "ColorOverlay"]
            .iter()
            .any(|t| is(w, e, t))
    {
        if DL_ON.with(|d| d.get()) {
            dl_deps(w, e, i);
        }
        let prev = LAYER_SKIP.with(|s| s.replace(Some(i)));
        let prev_src = LAYER_SRC.with(|s| s.replace(Some((e, i))));
        paint_body(w, fonts, e, cv, x, y, wd, ht, op);
        LAYER_SRC.with(|s| s.set(prev_src));
        LAYER_SKIP.with(|s| s.set(prev));
        return;
    }
    // Item.layer with no effect: QQuickItemLayer hides the item and shows its texture in a
    // QQuickShaderEffectSource of the item's place and size. An item painted as a source is
    // that texture's content already.
    if LAYER_SKIP.with(|s| s.get()) != Some(i)
        && !SOURCES.with(|s| s.borrow().last().is_some_and(|k| k.0 == i))
        && truthy(w, i, "layer.enabled", false)
        && !matches!(w.read(i, qml::runtime::LAYER_EFFECT), Some(Value::Item(_)))
    {
        let prev = LAYER_SKIP.with(|s| s.replace(Some(i)));
        paint_layer(w, fonts, i, cv, x, y, wd, ht, op);
        LAYER_SKIP.with(|s| s.set(prev));
        return;
    }
    if truthy(w, i, "clip", false) {
        let clip_wd = wd + edit_clip_extra(w, i);
        match cv {
            Target::Cpu(_) => cpu_clipped(cv, [x, y, clip_wd, ht], |cv| {
                paint_body(w, fonts, i, cv, x, y, wd, ht, op)
            }),
            Target::Gpu(f) => {
                f.push_clip_f([x, y, clip_wd, ht]);
                paint_body(w, fonts, i, cv, x, y, wd, ht, op);
                if let Target::Gpu(f) = cv {
                    f.pop_clip();
                }
            }
        }
        return;
    }
    paint_body(w, fonts, i, cv, x, y, wd, ht, op);
}

thread_local! {
    static ANIMATING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static EPOCH: std::time::Instant = std::time::Instant::now();
    static WALL: std::cell::Cell<Option<f64>> = const { std::cell::Cell::new(None) };
}

/// Pin the clock a running ProgressBar is drawn by to `ms` on this thread (`None`: the real
/// clock), for tests.
pub fn set_wall_clock(ms: Option<f64>) {
    WALL.with(|c| c.set(ms));
}

/// Milliseconds of the monotonic clock, from some moment of this thread's.
fn wall_ms() -> f64 {
    WALL.with(|c| c.get())
        .unwrap_or_else(|| EPOCH.with(|e| e.elapsed().as_secs_f64() * 1000.0))
}

/// Whether the last painted window holds a self-animating item (indeterminate ProgressBar).
pub fn take_animating() -> bool {
    ANIMATING.with(|a| a.replace(false))
}

/// What the label of a style part measures: its natural width, the height of its lines (an
/// empty text: none), the first line's ascent, the font's line height, and a key of the font.
struct StyleText {
    size: [f64; 2],
    ascent: f64,
    line: f64,
    key: f64,
}

/// The control's own `text` in the control's own `font`, as the label the style makes of it
/// (`text: control.text; font: control.font`) measures.
fn style_text(w: &mut World, fonts: &mut Fonts, i: usize, text: Option<&str>) -> StyleText {
    let ts = text_style(w, fonts, i);
    let text = text.map_or(ts.text, str::to_string);
    let Some(f) = ts.font else {
        return StyleText {
            size: [0.0, 0.0],
            ascent: 0.0,
            line: 0.0,
            key: 0.0,
        };
    };
    let empty = f.shape("");
    let (size, ascent) = if text.is_empty() {
        ([0.0, 0.0], empty.ascent)
    } else {
        let lines = text_lines(&f, &text);
        let iw = lines.iter().map(|l| f.natural_width(l)).max().unwrap_or(0) as f64 / 64.0;
        (
            [iw, lines.iter().map(|l| l.height()).sum::<f64>()],
            lines.first().map_or(empty.ascent, |l| l.ascent),
        )
    };
    StyleText {
        size,
        ascent: ascent as f64 / 64.0,
        line: empty.height(),
        key: ts.font_key,
    }
}

/// Keeps what the content the Basic style makes for a control is and what its label measures,
/// for `style_baseline`; nothing for a content of the document's own or none.
fn style_kind(w: &mut World, fonts: &mut Fonts, i: usize) {
    // A pane makes its content item itself (QQuickPanePrivate::getContentItem), a container
    // has the view its file of the style gives it (the ListView of TabBar.qml and
    // SwipeView.qml): items without a baseline.
    let ty = |cpp: &str| qml::types::inherits(&w.items[i].types, cpp);
    let made = if ty("QQuickPane") || ty("QQuickContainer") {
        1.0
    } else {
        0.0
    };
    let kind = match explicit(w, i, "contentItem") {
        false => style_content(w, fonts, i).map_or(made, |p| p.1),
        true => 0.0,
    };
    set(w, i, ST_KIND, kind);
    if kind < 2.0 {
        return;
    }
    let field = kind == 4.0;
    let t = style_text(w, fonts, i, field.then_some(""));
    let shown = !field
        && t.size[1] > 0.0
        && !(kind == 2.0 && opt_num(w, i, "display").is_some_and(|d| d as i32 == 0));
    let th = if shown { t.size[1] } else { t.line };
    for (k, v) in ST_LABEL
        .into_iter()
        .zip([t.ascent, th, t.key, f64::from(shown)])
    {
        set(w, i, k, v);
    }
}

/// `baselineOffset` of a control whose content the Basic style makes: `topPadding +
/// contentItem.baselineOffset` (QQuickControlPrivate::updateBaselineOffset), None for another
/// control. The content's own baseline is what Qt 6.12 leaves in it:
/// - an item that is not a text (ProgressBarImpl, the Row of a DayOfWeekRow): 0;
/// - an IconLabel: `label.y + label.baselineOffset` as its layout() reads them, 0 without a
///   label. The label is as high as its lines and centred in the content, and layout() runs
///   when the label's implicit size changes, which is before the label knows its new
///   baseline: a label that changes font keeps the baseline of the font before until the
///   IconLabel is laid out for another reason (it is resized, or makes a new label);
/// - a CheckLabel, a Text centred vertically: its first line's ascent below the centring
///   offset, which a Text without a text only works out when the text or the font changes
///   (QQuickText::geometryChange returns early), from the height it has then;
/// - the TextField of a ComboBox, centred vertically between its own paddings.
fn style_baseline(w: &mut World, i: usize) -> Option<f64> {
    let kind = w.items[i].props.get(ST_KIND).map(|p| p.value.to_number())?;
    if w.items[i].prop_info("background").is_none() || w.items[i].prop_info("contentItem").is_none()
    {
        return None;
    }
    // No content, by the document or by the style: QQuickControl leaves the baseline at 0.
    if explicit(w, i, "contentItem") || kind == 0.0 {
        return Some(0.0);
    }
    let top = pad_side(w, i, "top");
    if kind == 1.0 {
        return Some(top);
    }
    let [asc, th, key, shown] = ST_LABEL.map(|k| num(w, i, k));
    let shown = shown != 0.0;
    let avail = (num(w, i, "height") - top - pad_side(w, i, "bottom")).max(0.0);
    if kind == 4.0 {
        let pad = 6.0 - num(w, i, "padding");
        return Some(top + pad + (avail - 2.0 * pad - th) / 2.0 + asc);
    }
    // What was seen last: the label's own baseline, the content's height, the font and
    // whether there was a text.
    let had = w.items[i].props.contains_key(ST_SEEN[0]);
    let [was, room, pending] = ST_SEEN.map(|k| num(w, i, k));
    let (old_key, old_shown) = (num(w, i, "\x01stof"), num(w, i, "\x01stos") != 0.0);
    let relabel = !had || old_key != key || old_shown != shown;
    let resized = had && room != avail;
    let own = if kind == 2.0 {
        // The label's baseline within itself: its lines fill it unless the content is lower.
        let fresh = asc + (avail.min(th) - th) / 2.0;
        if !shown {
            0.0
        } else if !had || !old_shown || resized && pending == 0.0 && old_key == key {
            fresh
        } else {
            was
        }
    } else if shown {
        asc + (avail - th) / 2.0
    } else if !had {
        asc
    } else if relabel {
        asc + (room - th) / 2.0
    } else {
        was
    };
    // A label that was just made is first read where it is put, then with its baseline.
    if kind == 2.0 && shown && had && !old_shown {
        set(w, i, "baselineOffset", top + (avail - avail.min(th)) / 2.0);
    }
    set(w, i, ST_SEEN[0], own);
    set(w, i, ST_SEEN[1], avail);
    // The control follows its label's new size once; that resize is not another layout.
    set(w, i, ST_SEEN[2], f64::from(relabel && had));
    set(w, i, "\x01stof", key);
    set(w, i, "\x01stos", f64::from(shown));
    Some(
        top + if kind == 2.0 && shown {
            (avail - avail.min(th)) / 2.0 + own
        } else {
            own
        },
    )
}

/// The implicit size of a text editor whose text (or placeholder) is `iw` wide on one line
/// each and `ch` high, and `cw` wide as the editor wraps it (`contentWidth`).
/// A TextInput or TextEdit: the text not wrapped, rounded up, and the paddings. TextField.qml
/// and TextArea.qml of the Basic style bind it instead:
/// `implicitWidth: implicitBackgroundWidth + leftInset + rightInset || Math.max(contentWidth,
/// placeholder.implicitWidth) + leftPadding + rightPadding` (a TextArea: the widest of the
/// three) and `implicitHeight: Math.max(implicitBackgroundHeight + topInset + bottomInset,
/// contentHeight + topPadding + bottomPadding, placeholder.implicitHeight + ...)`.
fn field_size(w: &mut World, fonts: &mut Fonts, i: usize, iw: f64, cw: f64, ch: f64) -> (f64, f64) {
    let [l, t, r, b] = edit_padding(w, i);
    let area = qml::types::inherits(&w.items[i].types, "QQuickTextArea");
    if !area && !qml::types::inherits(&w.items[i].types, "QQuickTextField") {
        return (iw.ceil() + l + r, ch + t + b);
    }
    let bg = part_size(w, fonts, i, "background");
    set(w, i, "implicitBackgroundWidth", bg[0]);
    set(w, i, "implicitBackgroundHeight", bg[1]);
    let ins = |w: &mut World, a: &str, b: &str| {
        opt_num(w, i, a).unwrap_or(0.0) + opt_num(w, i, b).unwrap_or(0.0)
    };
    let bw = bg[0] + ins(w, "leftInset", "rightInset");
    let bh = bg[1] + ins(w, "topInset", "bottomInset");
    let text = cw.max(placeholder_width(w, fonts, i)) + l + r;
    let iw = match area {
        true => bw.max(text),
        false if bw != 0.0 => bw,
        false => text,
    };
    let ih = bh.max(ch.max(placeholder_height(w, fonts, i)) + t + b);
    if area {
        size_updated(w, i, [cw, ch], ih);
    }
    (iw, ih)
}

/// implicitHeight of the PlaceholderText of TextArea.qml and TextField.qml: the lines of the
/// `placeholderText` as the editor's `wrapMode` breaks them in the editor's width, however
/// high the editor is (the lines its height has no room for are only not painted). The
/// TextArea of a ScrollView is as wide as its implicit width in Qt, which the placeholder
/// fits in, and as wide as the view here: its placeholder is not wrapped.
fn placeholder_height(w: &mut World, fonts: &mut Fonts, i: usize) -> f64 {
    let Some(Value::Str(hint)) = w.read(i, "placeholderText") else {
        return 0.0;
    };
    let Some(f) = text_style(w, fonts, i).font else {
        return 0.0;
    };
    let view = w.items[i].parent;
    let [l, _, r, _] = edit_padding(w, i);
    let ww = match view
        .and_then(|p| w.flickable_of(p))
        .and_then(|f| scroll_text_area(w, f))
    {
        Some(c) if c == i => None,
        _ => edit_wrap_width(w, i, l + r),
    };
    edit_lines(&f, &hint, ww).iter().map(|l| l.height()).sum()
}

thread_local! {
    /// What `size_updated` last saw of each TextArea.
    static AREA_SIZED: std::cell::RefCell<HashMap<usize, [u64; 5]>> = std::cell::RefCell::new(HashMap::default());
}

/// QQuickTextEdit::updateSize, which a TextArea runs for a new text, font, wrap or size,
/// emits `implicitWidthChanged` each time while the area's width is set, changed or not:
/// it holds the implicit width back until `implicitWidth` is read through
/// QQuickTextEditPrivate::getImplicitWidth, and QQuickTextAreaPrivate has its own. `ih`, the
/// implicit height the area is about to have, is its next height unless one is set, and
/// that new size is one more run.
fn size_updated(w: &mut World, i: usize, content: [f64; 2], ih: f64) {
    let text = match w.read(i, "text") {
        Some(Value::Str(s)) => text_key(&&*s),
        _ => 0.0,
    };
    let (wide, high) = size_valid(w, i);
    let height = num(w, i, "height");
    let now = [
        text,
        num(w, i, "width"),
        if high { height } else { 0.0 },
        content[0],
        content[1],
    ]
    .map(f64::to_bits);
    let was = AREA_SIZED.with_borrow_mut(|m| m.insert(i, now));
    if wide && was.is_some_and(|was| was != now) {
        for _ in 0..1 + usize::from(!high && ih != height) {
            qml::native::emit(w, i, "implicitWidthChanged", &[], Vec::new());
        }
    }
}

/// What the content of a control of the Basic style is, for its baseline (`ST_KIND`): the
/// document's own item or none, an item without a baseline, an IconLabel, a CheckLabel, the
/// TextField of a ComboBox.
const ST_KIND: &str = "\x01stk";
/// The label of that content: the first line's ascent, the height of its lines (the font's
/// line when it has none), a key of its font, and whether it shows a text.
const ST_LABEL: [&str; 4] = ["\x01sta", "\x01sth", "\x01stf", "\x01stt"];
/// What the content last said its label's baseline was, the height it had then, and whether a
/// change of its label is still being followed by the control's size (`style_baseline`).
const ST_SEEN: [&str; 3] = ["\x01stb", "\x01stg", "\x01stp"];

/// The width a part takes of its control: its own, else its implicit one.
fn part_width(w: &mut World, c: usize) -> f64 {
    if explicit(w, c, "width") {
        num(w, c, "width")
    } else {
        num(w, c, "implicitWidth")
    }
}

/// The implicit size of `part` of a control: the document's own item's, nothing for a part
/// the document set to null, else what the QML of Qt 6.12's Basic style gives the part it
/// makes (`style_part`).
fn part_size(w: &mut World, fonts: &mut Fonts, i: usize, part: &str) -> [f64; 2] {
    if w.items[i].prop_info(part).is_none() {
        return [0.0, 0.0];
    }
    match w.read(i, part) {
        Some(Value::Item(c)) if c != i => [num(w, c, "implicitWidth"), num(w, c, "implicitHeight")]
            .map(|v| if v.is_finite() { v } else { 0.0 }),
        _ if explicit(w, i, part) => [0.0, 0.0],
        _ => style_part(w, fonts, i, part).unwrap_or([0.0, 0.0]),
    }
}

/// The width of the indicator next to the label of a CheckBox, RadioButton, Switch, MenuItem
/// or ComboBox, and the spacing after it: `control.indicator.width + control.spacing`, 0
/// without an indicator.
fn indicator_room(w: &mut World, i: usize) -> f64 {
    let iw = match w.read(i, "indicator") {
        Some(Value::Item(c)) if c != i => part_width(w, c),
        _ if explicit(w, i, "indicator") => return 0.0,
        _ => match style_box(w, i, "indicator") {
            Some(p) => p[0],
            None => return 0.0,
        },
    };
    iw + style_spacing(w, i)
}

/// The implicit size of a part the Basic style of Qt 6.12 makes for a control, and for a
/// contentItem what it is (`ST_KIND`); None for a part the style has none of. Each arm is the
/// part's own QML in qtdeclarative/src/quickcontrols/basic.
fn style_part(w: &mut World, fonts: &mut Fonts, i: usize, part: &str) -> Option<[f64; 2]> {
    match part {
        "contentItem" => style_content(w, fonts, i).map(|p| p.0),
        _ => style_box(w, i, part),
    }
}

/// Whether the control is the Material style's: the file that names its type (`ProgressBar` in
/// StyledIndeterminateProgressBar.qml, whatever file uses that one) imports
/// QtQuick.Controls.Material, which is where Qt takes the type from then.
fn material_style(w: &World, i: usize) -> bool {
    let it = &w.items[i];
    const MATERIAL: &str = "QtQuick.Controls.Material";
    // The document the object is written in: a file, or a text with its imports.
    let (mut c, mut file) = (it.ctx.map(|c| c as usize), None);
    while let Some(ci) = c {
        if let Some(f) = w.ctx_files.get(&ci) {
            file = Some(f.clone());
            break;
        }
        if let Some(uris) = w.ctx_imports.get(&ci) {
            return uris.iter().any(|u| u == MATERIAL);
        }
        c = w.ctxs.get(ci).and_then(|x| x.parent);
    }
    let Some(mut file) = file else {
        return false;
    };
    // From the file the object is written in down the files of its types to the one that
    // names a type no file defines (an inline component is in the file that names it).
    for t in &it.types {
        if let Some(r) = w.types_cache.get(&file).and_then(|m| m.get(t)) {
            file = r.file.to_path_buf();
        }
    }
    w.docs
        .get(&file)
        .is_some_and(|d| d.imports.iter().any(|i| i.uri == MATERIAL))
}

/// `style_part` for the background, the indicator and the handle.
fn style_box(w: &mut World, i: usize, part: &str) -> Option<[f64; 2]> {
    let ty = |w: &World, cpp: &str| qml::types::inherits(&w.items[i].types, cpp);
    let vertical = |w: &mut World| num(w, i, "orientation") as i32 == 2;
    let size = match part {
        "background" => {
            if ty(w, "QQuickRoundButton") || ty(w, "QQuickToolButton") {
                // RoundButton.qml, ToolButton.qml: `implicitWidth: 40; implicitHeight: 40`.
                [40.0, 40.0]
            } else if ty(w, "QQuickTabButton") {
                // TabButton.qml: `implicitHeight: 40`.
                [0.0, 40.0]
            } else if ty(w, "QQuickMenuItem") || ty(w, "QQuickMenu") {
                // MenuItem.qml, Menu.qml: `implicitWidth: 200; implicitHeight: 40`.
                [200.0, 40.0]
            } else if ty(w, "QQuickButton") || ty(w, "QQuickItemDelegate") {
                // Button.qml, ItemDelegate.qml: `implicitWidth: 100; implicitHeight: 40`.
                [100.0, 40.0]
            } else if ty(w, "QQuickSlider") {
                // Slider.qml: `implicitWidth: control.horizontal ? 200 : 6` and the height
                // the other way round.
                if vertical(w) {
                    [6.0, 200.0]
                } else {
                    [200.0, 6.0]
                }
            } else if ty(w, "QQuickProgressBar") {
                // ProgressBar.qml: `implicitWidth: 200; implicitHeight: 6`; Material's has
                // `implicitHeight: 4`.
                [200.0, if material_style(w, i) { 4.0 } else { 6.0 }]
            } else if ty(w, "QQuickComboBox") {
                // ComboBox.qml: `implicitWidth: 140; implicitHeight: 40`.
                [140.0, 40.0]
            } else if ty(w, "QQuickTextField") && !material_field(w, i) {
                // TextField.qml: `implicitWidth: 200; implicitHeight: 40`.
                [200.0, 40.0]
            } else {
                return None;
            }
        }
        "indicator" => {
            if ty(w, "QQuickSwitch") {
                // Switch.qml: `implicitWidth: 56; implicitHeight: 28`.
                [56.0, 28.0]
            } else if ty(w, "QQuickCheckBox") || ty(w, "QQuickRadioButton") {
                // CheckBox.qml, RadioButton.qml: `implicitWidth: 28; implicitHeight: 28`.
                [28.0, 28.0]
            } else if ty(w, "QQuickComboBox") {
                // ComboBox.qml: a ColorImage of images/double-arrow.png, 40x28.
                [40.0, 28.0]
            } else if ty(w, "QQuickMenuItem") {
                // MenuItem.qml: a ColorImage of images/check.png (28x28), `source` empty
                // unless `control.checkable`.
                if truthy(w, i, "checkable", false) {
                    [28.0, 28.0]
                } else {
                    [0.0, 0.0]
                }
            } else {
                return None;
            }
        }
        // Slider.qml: `implicitWidth: 28; implicitHeight: 28`.
        "handle" if ty(w, "QQuickSlider") => [28.0, 28.0],
        _ => return None,
    };
    Some(size)
}

/// `spacing` of a control: what was written to it, else what the file of the Basic style
/// sets: `spacing: 6` in Button.qml, RoundButton.qml, ToolButton.qml, TabButton.qml,
/// CheckBox.qml, RadioButton.qml, Switch.qml, MenuItem.qml and DayOfWeekRow.qml, `spacing: 8`
/// in ItemDelegate.qml, none in the others.
fn style_spacing(w: &mut World, i: usize) -> f64 {
    if w.items[i].props.contains_key("spacing") {
        return num(w, i, "spacing");
    }
    let ty = |cpp: &str| qml::types::inherits(&w.items[i].types, cpp);
    if ty("QQuickItemDelegate") {
        8.0
    } else if [
        "QQuickButton",
        "QQuickTabButton",
        "QQuickCheckBox",
        "QQuickRadioButton",
    ]
    .into_iter()
    .chain(["QQuickSwitch", "QQuickMenuItem", "QQuickDayOfWeekRow"])
    .any(ty)
    {
        6.0
    } else {
        0.0
    }
}

/// `style_part` for the contentItem.
fn style_content(w: &mut World, fonts: &mut Fonts, i: usize) -> Option<([f64; 2], f64)> {
    let ty = |w: &World, cpp: &str| qml::types::inherits(&w.items[i].types, cpp);
    Some(
        if ty(w, "QQuickCheckBox") || ty(w, "QQuickRadioButton") || ty(w, "QQuickSwitch") {
            // CheckLabel, a Text: `leftPadding: control.indicator && !control.mirrored ?
            // control.indicator.width + control.spacing : 0` and the same on the right when
            // mirrored. A Text without a text is as high as a line of its font.
            let t = style_text(w, fonts, i, None);
            let room = indicator_room(w, i);
            ([t.size[0] + room, t.size[1].max(t.line)], 3.0)
        } else if ty(w, "QQuickMenuItem") {
            // MenuItemIconLabel: `leftPadding: control.checkable && control.indicator ?
            // control.indicator.width + control.spacing : 0` (the right one when mirrored).
            let t = icon_label(w, fonts, i);
            let room = if truthy(w, i, "checkable", false) {
                indicator_room(w, i)
            } else {
                0.0
            };
            ([t[0] + room, t[1]], 2.0)
        } else if ty(w, "QQuickAbstractButton") {
            // Button.qml, RoundButton.qml, ToolButton.qml, TabButton.qml, ItemDelegate.qml:
            // an IconLabel of `control.text`.
            (icon_label(w, fonts, i), 2.0)
        } else if ty(w, "QQuickComboBox") {
            // ComboBox.qml: a T.TextField, `implicitHeight: contentHeight + topPadding +
            // bottomPadding`, `topPadding: 6 - control.padding` and the same at the bottom;
            // its implicitWidth is not bound and stays 0.
            let t = style_text(w, fonts, i, Some(""));
            let pad = 6.0 - num(w, i, "padding");
            ([0.0, t.line + 2.0 * pad], 4.0)
        } else if ty(w, "QQuickProgressBar") {
            // ProgressBar.qml: a ProgressBarImpl, `implicitHeight: 6; implicitWidth: 116`;
            // Material's has `implicitHeight: 4` and no width.
            match material_style(w, i) {
                true => ([0.0, 4.0], 1.0),
                false => ([116.0, 6.0], 1.0),
            }
        } else if ty(w, "QQuickScrollBar") {
            // ScrollBar.qml: `implicitWidth: control.interactive ? 6 : 2` and the same height.
            // QQuickScrollBarPrivate: `interactive` is true until it is written.
            let v = if !w.items[i].props.contains_key("interactive")
                || truthy(w, i, "interactive", true)
            {
                6.0
            } else {
                2.0
            };
            ([v, v], 1.0)
        } else if ty(w, "QQuickToolTip") {
            // ToolTip.qml: `contentItem: Text { text: control.text; font: control.font }`.
            let t = style_text(w, fonts, i, None);
            ([t.size[0], t.size[1].max(t.line)], 1.0)
        } else if ty(w, "QQuickDayOfWeekRow") {
            // DayOfWeekRow.qml: a Row (`spacing: control.spacing`) of the seven short day
            // names, each a Text in the control's font.
            let names = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
            let mut size = [6.0 * style_spacing(w, i), 0.0f64];
            for n in names {
                let t = style_text(w, fonts, i, Some(n));
                size[0] += t.size[0];
                size[1] = size[1].max(t.size[1]);
            }
            (size, 1.0)
        } else {
            return None;
        },
    )
}

/// The implicit size of an IconLabel without an icon: its label's (QQuickIconLabelPrivate::
/// updateImplicitSize), which it has while there is a text and `display` is not
/// AbstractButton.IconOnly (hasText).
fn icon_label(w: &mut World, fonts: &mut Fonts, i: usize) -> [f64; 2] {
    if opt_num(w, i, "display").is_some_and(|d| d as i32 == 0) {
        return [0.0, 0.0];
    }
    style_text(w, fonts, i, None).size
}

/// A ProgressBar without style items is drawn like QQuickMaterialProgressBar (the original imports the Material style).
fn bare_progress(w: &mut World, i: usize) -> bool {
    is(w, i, "ProgressBar")
        && !matches!(w.read(i, "contentItem"), Some(Value::Item(_)))
        && !matches!(w.read(i, "background"), Some(Value::Item(_)))
}

/// `background: null` on a ProgressBar the style draws: the style's track is taken away.
fn no_background(w: &mut World, i: usize) -> bool {
    w.items[i].props.contains_key("background")
        && matches!(w.read(i, "background"), Some(Value::Null))
}

thread_local! {
    /// When the timer of each running ProgressBar was started, in `wall_ms`
    /// (QQuickAnimatedNode::m_timer of its QQuickMaterialProgressBarNode).
    static RUNNING: std::cell::RefCell<HashMap<usize, f64>> = std::cell::RefCell::new(HashMap::default());
}

/// A bar runs while `control.visible && control.indeterminate` (Material ProgressBar.qml):
/// one that no longer does is stopped, and starts from the beginning when it runs again.
fn stop_hidden_progress(w: &mut World) {
    if RUNNING.with_borrow(|r| r.is_empty()) {
        return;
    }
    let mut ids: Vec<usize> = RUNNING.with_borrow(|r| r.keys().copied().collect());
    ids.retain(|&i| {
        let mut shown = is(w, i, "ProgressBar") && truthy(w, i, "indeterminate", false);
        let mut at = Some(i);
        while let Some(k) = at.filter(|_| shown) {
            shown = visible(w, k);
            at = w.items[k].parent;
        }
        !shown
    });
    RUNNING.with_borrow_mut(|r| r.retain(|i, _| !ids.contains(i)));
}

/// QQuickAnimatedNode::advance of the bar `i`: the whole milliseconds since its timer was
/// started, which the first frame that finds it running does; a frame at or past the 1760 ms
/// of a round is at 0 and starts the timer again.
fn running_ms(i: usize) -> f64 {
    const ROUND: f64 = 1760.0;
    let now = wall_ms();
    RUNNING.with_borrow_mut(|r| {
        let start = r.entry(i).or_insert(now);
        let t = (now - *start).trunc();
        if t < ROUND {
            return t;
        }
        *start = now;
        0.0
    })
}

/// The Material style's ProgressBar.qml: the track is a plain Rectangle 4 high, as wide as the
/// control less its insets, at `y: (control.height - height) / 2`; the bar is the contentItem
/// (QQuickMaterialProgressBarNode), antialiased rectangles 4 high in the middle of the
/// control less its padding.
#[allow(clippy::too_many_arguments)]
fn paint_progress(
    w: &mut World,
    i: usize,
    cv: &mut Target,
    x: f64,
    y: f64,
    wd: f64,
    ht: f64,
    op: f32,
) {
    const PAUSE: f64 = 520.0;
    const SLIDE: f64 = 1240.0;
    const BAR: f64 = 4.0;
    let accent = color(w, i, "Material.accent", [0xE9, 0x1E, 0x63, 255]);
    // (x, y) of the node in the window, and the rectangle [left, top, width, height] in it.
    let mut rect = |c: [u8; 4], antialiasing: bool, at: [f64; 2], r: [f64; 4]| {
        if r[2] <= 0.0 || r[3] <= 0.0 {
            return;
        }
        let g = qsg::rect::build(&qsg::rect::RectParams {
            x: r[0] as f32,
            y: r[1] as f32,
            width: r[2] as f32,
            height: r[3] as f32,
            color: c,
            antialiasing,
            ..Default::default()
        });
        match cv {
            Target::Cpu(cv) => cpu_smooth(cv, &g, at[0], at[1], op),
            Target::Gpu(f) => f.smooth(&Rc::new(g), at[0], at[1], op),
        }
    };
    if !no_background(w, i) {
        let track = [
            accent[0],
            accent[1],
            accent[2],
            (accent[3] as f64 * 0.25).round() as u8,
        ];
        let [left, top, right, bottom] = ["left", "top", "right", "bottom"]
            .map(|s| opt_num(w, i, &format!("{s}Inset")).unwrap_or(0.0));
        // QQuickControlPrivate::resizeBackground: an inset above or below makes the track as
        // high as the control less both, and its `y` binding then centres that.
        let th = if top != 0.0 || bottom != 0.0 {
            ht - top - bottom
        } else {
            BAR
        };
        let at = [x + left, y + (ht - th) / 2.0];
        rect(track, false, at, [0.0, 0.0, wd - left - right, th]);
    }
    // The contentItem fills the control less its padding; its node keeps the item's origin
    // and sets the rectangles (ht - 4) / 2 down in it.
    let pad = ["left", "top", "right", "bottom"].map(|s| pad_side(w, i, s));
    let (x, y) = (x + pad[0], y + pad[1]);
    let (wd, ht) = (wd - pad[0] - pad[2], ht - pad[1] - pad[3]);
    if !(wd > 0.0 && ht > 0.0) {
        return;
    }
    let top = (ht - BAR) / 2.0;
    if w.read(i, "indeterminate").is_some_and(|v| v.truthy()) {
        ANIMATING.with(|a| a.set(true));
        let t = running_ms(i);
        for p in [(t / SLIDE).min(1.0), ((t - PAUSE) / SLIDE).max(0.0)] {
            let v = 1.0 - (1.0 - p).powi(3);
            let bx = v * wd;
            rect(accent, true, [x + bx, y], [0.0, top, v * (wd - bx), BAR]);
        }
    } else {
        let f = num(w, i, "from");
        let t = num(w, i, "to");
        let p = if t != f {
            ((num(w, i, "value") - f) / (t - f)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        rect(accent, true, [x, y], [0.0, top, p * wd, BAR]);
    }
}

/// Rectangle's pen width as QQuickRectangle::updatePaintNode gives it to the node: an invalid
/// QQuickPen (transparent color, or a width that rounds below 1 unless `border.pixelAligned`
/// is off) draws no border and does not inset the fill, and a pixel-aligned one is a whole
/// number of device pixels wide.
fn rect_pen_width(width: f64, color: [u8; 4], aligned: bool, dpr: f64) -> f32 {
    if color[3] == 0 || !(width.round() >= 1.0 || (!aligned && width > 0.0)) {
        0.0
    } else if aligned {
        ((width * dpr).round() / dpr) as f32
    } else {
        width as f32
    }
}

/// What QQuickRectangle::updatePaintNode hands its node, for a Rectangle `wd` by `ht` on a
/// screen of scale `dpr`.
fn rect_params(w: &mut World, i: usize, wd: f64, ht: f64, dpr: f64) -> qsg::rect::RectParams {
    let corner = |w: &mut World, n: &str| opt_num(w, i, n).map(|v| v as f32);
    let border_color = color(w, i, "border.color", [0, 0, 0, 255]);
    let pen_width = rect_pen_width(
        num(w, i, "border.width"),
        border_color,
        truthy(w, i, "border.pixelAligned", true),
        dpr,
    );
    let mut p = qsg::rect::RectParams {
        width: wd as f32,
        height: ht as f32,
        color: color(w, i, "color", [255, 255, 255, 255]),
        border_color,
        pen_width,
        radius: num(w, i, "radius") as f32,
        tl: corner(w, "topLeftRadius"),
        tr: corner(w, "topRightRadius"),
        bl: corner(w, "bottomLeftRadius"),
        br: corner(w, "bottomRightRadius"),
        antialiasing: rect_antialiasing(w, i),
        // setAligned(false): the width is rounded above, in device pixels.
        aligned: false,
        ..Default::default()
    };
    // Rectangle.gradient: GradientStop children; Gradient.Horizontal = Qt.Horizontal (1).
    if let Some(Value::Item(g)) = w.read(i, "gradient") {
        let stops = gradient_stops(w, g);
        if !stops.is_empty() {
            p.vertical = num(w, g, "orientation") != 1.0;
            p.stops = stops;
        }
    }
    p
}

/// QQuickItem::antialiasing() of a Rectangle.
fn rect_antialiasing(w: &mut World, i: usize) -> bool {
    w.read(i, "antialiasing").is_some_and(|v| v.truthy())
}

/// The image node of the QQuickShaderEffectSource a layer with no effect is shown through
/// (QQuickShaderEffectSource::updatePaintNode, Qt 6.12): the item's texture, which is its size
/// in device pixels rounded, over a rect of that many device pixels and one more along an axis
/// where the item's own position is between device pixels. Nothing for an item of no size.
#[allow(clippy::too_many_arguments)]
fn paint_layer(
    w: &mut World,
    fonts: &mut Fonts,
    i: usize,
    cv: &mut Target,
    x: f64,
    y: f64,
    wd: f64,
    ht: f64,
    op: f32,
) {
    if !(wd > 0.0 && ht > 0.0) {
        return;
    }
    let dpr = match cv {
        Target::Cpu(_) => 1.0,
        Target::Gpu(f) => f.dpr,
    };
    let target = |at: f64, size: f64| {
        let d = at * dpr;
        ((d - d.floor()).ceil() + (size * dpr).round().max(1.0)) / dpr
    };
    let (tw, th) = (target(num(w, i, "x"), wd), target(num(w, i, "y"), ht));
    // A texture past the limit is not made, and nothing is shown.
    if !cpu_texture_fits(tw * dpr, th * dpr) {
        return;
    }
    let smooth = truthy(w, i, "layer.smooth", false);
    match cv {
        Target::Gpu(_) => {
            let sf = gpu_layer_dpr(w, fonts, i, dpr);
            if let Target::Gpu(fr) = cv {
                fr.source(sf, smooth, [x, y, tw, th], op);
            }
        }
        Target::Cpu(_) => {
            // Rendered for each frame, as the item was before it had a texture: `cached_layer`
            // is right only for a caller that names every change (`invalidate_layers`), and
            // nothing here needs the same texture twice.
            let st = render_layer(w, fonts, i);
            if let Target::Cpu(c) = cv {
                cpu_direct(c, |c, [sx, sy]| {
                    let [x, y, tw, th] =
                        as_llvmpipe_places([sx * x, sy * y, sx * tw, sy * th], (c.w, c.h));
                    if smooth {
                        qsg::layer::draw_linear(c, &st, x, y, tw, th, op)
                    } else {
                        qsg::layer::draw(c, &st, x, y, tw, th, op)
                    }
                });
            }
        }
    }
}

/// The rect (x y w h, pixels of a `target` of that size) between the corners llvmpipe draws
/// it with: QSGBatchRenderer moves the vertices of a node that is only moved before it
/// uploads them (f32), the vertex shader multiplies them by the projection (QMatrix4x4::ortho
/// over the target) and the viewport puts the result back in pixels. The corners are a few
/// f32 steps off the exact ones, which decides the texel of a pixel whose centre is on the
/// edge of two: a texture of 40 over 41 pixels from 20.3 has such a centre at 28.5.
fn as_llvmpipe_places([x, y, w, h]: [f64; 4], target: (usize, usize)) -> [f64; 4] {
    let (tw, th) = (target.0 as f32, target.1 as f32);
    let (l, t) = (x as f32, y as f32);
    let (r, b) = (l + w as f32, t + h as f32);
    let place = |x: f32, y: f32| {
        qsg::raster::viewport([2.0 / tw * x + -1.0, -(2.0 / th) * y + 1.0], tw, th)
    };
    let ([l, t], [r, b]) = (place(l, t), place(r, b));
    let (l, t) = (l as f64, t as f64);
    [l, t, r as f64 - l, b as f64 - t]
}

#[allow(clippy::too_many_arguments)]
fn paint_body(
    w: &mut World,
    fonts: &mut Fonts,
    i: usize,
    cv: &mut Target,
    x: f64,
    y: f64,
    wd: f64,
    ht: f64,
    op: f32,
) {
    if wd > 0.0 && bare_progress(w, i) {
        paint_progress(w, i, cv, x, y, wd, ht, op);
    } else if is(w, i, "Rectangle") && wd > 0.0 && ht > 0.0 {
        // QQuickItemPrivate::effectiveDevicePixelRatio(): the window's.
        let dpr = match cv {
            Target::Cpu(_) => 1.0,
            Target::Gpu(f) => f.dpr,
        };
        let p = rect_params(w, i, wd, ht, dpr);
        let g = qsg::rect::build(&p);
        // Under a QSGTransformNode the node's matrix is that one moved to the item.
        let through = NODE_XF
            .with(|m| m.get())
            .and_then(|m| Xf::translation(x, y).then(&m).affine())
            .map(|m| m.map(|v| v as f32));
        let through = match (through, NODE_TOP.with(|t| t.get())) {
            (Some(_), Some((top, at))) => {
                let mut base = qml::valuetypes::Mat4::identity();
                if at != [0.0, 0.0] {
                    base.translate(at[0], at[1], 0.0);
                }
                let m = w.combined_node_matrix(i, top, base).m;
                Some([m[0][0], m[0][1], m[1][0], m[1][1], m[3][0], m[3][1]])
            }
            (t, _) => t,
        };
        match (&mut *cv, through) {
            (Target::Cpu(c), None) => cpu_smooth(c, &g, x, y, op),
            (Target::Gpu(f), None) => f.smooth(&Rc::new(g), x, y, op),
            (Target::Cpu(c), Some(m)) => cpu_smooth_matrix(c, &g, m, op),
            (Target::Gpu(f), Some(m)) => f.smooth_matrix(&g, m, op),
        }
    } else if is_text(w, i) && text_no_room(w, i) && num(w, i, TEXT_ROOM[2]) != 2.0 {
        // No line is laid out, so there is nothing to draw.
    } else if is_text(w, i)
        && let ts = text_style(w, fonts, i)
        && let (Some(f), Some(spans)) = (&ts.font, &ts.spans)
    {
        let lines = rich_laid(f, spans, &text_bounds(w, i, ts.doc, None)).lines;
        let col = color(w, i, "color", [0, 0, 0, 255]);
        let th: f64 = lines.iter().map(|l| l.height()).sum();
        let ha = text_halign(w, i, &ts.text).1;
        let [pl, pt, pr, pb] = text_pad(w, i);
        let (wd, ht) = (wd - pl - pr, ht - pt - pb);
        let mut dy = pt
            + match num(w, i, "verticalAlignment") as u32 {
                128 => (ht - th) / 2.0,
                64 => ht - th,
                _ => 0.0,
            };
        let mut under_lines = vec![];
        for line in &lines {
            let dx = pl
                + match ha {
                    4 => center_offset(wd, (line.advance * 64.0).round() as i64),
                    2 => wd - line.advance,
                    _ => 0.0,
                };
            for (sf, sl, sx, deco) in &line.segs {
                if let Some(id) = deco.2 {
                    let baseline = y + dy + line.ascent as f64 / 64.0;
                    draw_inline_image(cv, id, x + dx + sx, baseline, op);
                    continue;
                }
                let sy = dy + (line.ascent - sl.ascent) as f64 / 64.0;
                let col = deco.0.unwrap_or(col);
                run_background(cv, line, sl, x + dx + sx, y + dy, deco, op);
                match cv {
                    Target::Cpu(c) => cpu_line(c, sf, sl, [x, y], [dx + sx, sy], col, op),
                    Target::Gpu(fr) => gpu_glyphs(fr, sf, sl, x, y, dx + sx, sy, col, op),
                }
            }
            for (sx, width, metrics, of) in line.underlines() {
                under_lines.push(Underline {
                    metrics,
                    ascent: line.ascent,
                    width,
                    x: x + dx + sx,
                    top: y + dy,
                    color: of.unwrap_or(col),
                });
            }
            dy += line.height();
        }
        for u in &under_lines {
            underline(cv, u, op);
        }
    } else if is_text(w, i) {
        let ts = text_style(w, fonts, i);
        if let Some(f) = ts.font {
            let lines = plain_laid(&f, &ts.text, &text_bounds(w, i, ts.doc, None)).lines;
            let col = color(w, i, "color", [0, 0, 0, 255]);
            let qt_rendering = qt_rendering(w, i);
            let df_gray = qt_rendering
                && df_node_recycled(
                    i,
                    &ts.text,
                    col,
                    f.font(0).map_or(0, |x| x.pixel_size) as f64,
                    wd,
                    ht,
                );
            let df = qt_rendering.then(|| df_text(w, i, df_gray));
            let th: f64 = lines.iter().map(|l| l.height()).sum();
            let ha = text_halign(w, i, &ts.text).1;
            let va = num(w, i, "verticalAlignment") as u32;
            let [pl, pt, pr, pb] = text_pad(w, i);
            let (wd, ht) = (wd - pl - pr, ht - pt - pb);
            let mut dy = pt
                + match va {
                    128 => (ht - th) / 2.0,
                    64 => ht - th,
                    _ => 0.0,
                };
            DF_AT.with(|c| c.set(qt_rendering.then_some(i)));
            for line in &lines {
                let lw = line.advance as f64 / 64.0;
                let dx = pl
                    + match ha {
                        4 => center_offset(wd, line.advance),
                        2 => wd - lw,
                        _ => 0.0,
                    };
                rt_line(cv, &f, line, x, y, dx, dy, col, op, df);
                dy += line.height();
            }
            DF_AT.with(|c| c.set(None));
        }
    } else if is_text_edit(w, i) {
        let mat = material_field(w, i);
        if let Some(b) = edit_background(w, i) {
            paint(w, fonts, b, cv, x, y, op, false);
        }
        if mat {
            paint_material_frame(w, fonts, i, cv, x, y, wd, ht, op);
        }
        let field_rect = field_clip(w, i, [x, y, wd, ht]);
        let clip = field_rect
            .filter(|(_, fits)| !fits)
            .map(|(r, _)| clip_push(cv, r));
        let (ts, placeholder) = edit_style(w, fonts, i);
        if let (Some(f), Some(spans)) = (&ts.font, &ts.spans) {
            let [l, t, r, b] = edit_padding(w, i);
            let ww = edit_wrap_width(w, i, l + r);
            let lines = edit_rich_lines(w, i, f, spans, ww);
            let col = edit_color(w, i);
            let ha = num(w, i, "horizontalAlignment") as u32;
            let th: f64 = lines.iter().map(|l| l.height()).sum();
            let mut dy = t + edit_valign(w, i) * (ht - t - b - th);
            let mut under_lines = vec![];
            for line in &lines {
                let dx = l + match ha {
                    4 => (wd - l - r - line.advance) / 2.0,
                    2 => wd - l - r - line.advance,
                    _ => 0.0,
                };
                for (sf, sl, sx, deco) in &line.segs {
                    if let Some(id) = deco.2 {
                        let baseline = y + dy + line.ascent as f64 / 64.0;
                        draw_inline_image(cv, id, x + dx + sx, baseline, op);
                        continue;
                    }
                    let sy = dy + (line.ascent - sl.ascent) as f64 / 64.0;
                    let col = deco.0.unwrap_or(col);
                    run_background(cv, line, sl, x + dx + sx, y + dy, deco, op);
                    edit_line(cv, sf, sl, [x, y], [dx + sx, sy], col, op);
                }
                for (sx, width, metrics, of) in line.underlines() {
                    under_lines.push(Underline {
                        metrics,
                        ascent: line.ascent,
                        width,
                        x: x + dx + sx,
                        top: y + dy,
                        color: of.unwrap_or(col),
                    });
                }
                dy += line.height();
            }
            for u in &under_lines {
                underline(cv, u, op);
            }
        } else if let Some(f) = ts.font {
            let lines = edit_layout(w, &f, i, &ts.text, placeholder, wd, ht);
            // The caret is the document's, which is empty while the placeholder shows.
            let empty = if placeholder {
                edit_layout(w, &f, i, "", false, wd, ht).into_iter().next()
            } else {
                None
            };
            let fg = edit_color(w, i);
            let col = if placeholder {
                color(w, i, "placeholderTextColor", [127, 127, 127, 255])
            } else {
                fg
            };
            let df = qt_rendering(w, i).then(DfText::default);
            // QQuickTextField and QQuickTextArea put their text node under a clip node, and
            // QQuickTextNodeEngine the selected text under one of the selection. A
            // QQuickTextEdit moves its text node by its paddings and its alignment.
            let base = match is(w, i, "TextEdit") || is(w, i, "TextArea") {
                true => [edit_padding(w, i)[0], lines.first().map_or(0.0, |l| l.y)],
                false => [0.0; 2],
            };
            let types = &w.items[i].types;
            let field = qml::types::inherits(types, "QQuickTextArea")
                || qml::types::inherits(types, "QQuickTextField");
            let df_own = df.map(|d| DfText {
                root: field.then_some(([0.0; 2], base)),
                ..d
            });
            let df_selected = df.map(|d| DfText {
                root: Some((base, [0.0; 2])),
                ..d
            });
            let df = df_own;
            let mut idx = |n: &str| {
                w.read(i, n)
                    .map(|v| v.to_number())
                    .filter(|v| v.is_finite())
                    .map(|v| v.max(0.0) as usize)
            };
            let (cursor, a0, a1) = (
                idx("cursorPosition"),
                idx("selectionStart"),
                idx("selectionEnd"),
            );
            let at = |w: &mut World, u: Option<usize>| u.map(|u| edit_char(w, i, u));
            let (cursor, a0, a1) = (at(w, cursor), at(w, a0), at(w, a1));
            // The input method's cursor in its preedit; a hidden one is at its start
            // (QInputMethodEvent::Cursor of length 0).
            let ime_cursor = crate::keys::preedit(i).map(|p| p.1);
            let cursor = cursor.map(|c| c + ime_cursor.flatten().unwrap_or(0));
            let (cursor, s0, s1) = if placeholder {
                (Some(0), 0, 0)
            } else {
                let (a, b) = (a0.unwrap_or(0), a1.unwrap_or(0));
                (cursor, a.min(b), a.max(b))
            };
            // QQuickTextInput/QQuickTextEdit paint their own cursor while `cursorVisible` and
            // they have no cursor item (`cursorDelegate`); it blinks only with active focus.
            let caret = truthy(w, i, "cursorVisible", false)
                && !w.cursor_items.contains_key(&i)
                && ime_cursor != Some(None)
                && (!truthy(w, i, "activeFocus", false) || caret_phase(w).0);
            let [sel_bg, sel_fg] = ["selectionColor", "selectedTextColor"].map(|n| {
                let styled = w.unset_style_colour(i, n);
                styled.or_else(|| w.read(i, n)).map_or([0; 4], |v| rgba(&v))
            });
            let (caret_w, caret_col) = if mat {
                (2.0, color(w, i, "Material.accent", [0xE9, 0x1E, 0x63, 255]))
            } else {
                (1.0, fg)
            };
            // The glyph nodes of distance fields a selection splits the text into, and which
            // of a TextInput's were taken from its recycle bin.
            let nodes = df.filter(|_| !placeholder && s0 < s1).map(|_| {
                let (count, per_line) = df_edit_nodes(&lines, s0, s1);
                (vec![false; count], per_line)
            });
            let nodes = match (df, is(w, i, "TextInput") || is(w, i, "TextField")) {
                (Some(_), true) => {
                    let sel = (s0 < s1).then_some((s0, s1, sel_bg, sel_fg));
                    let own: &[EditLine] = if placeholder { &[] } else { &lines };
                    let key = df_edit_key(own, &f, fg, wd, sel);
                    let (count, per_line) = match nodes {
                        Some((gray, per_line)) => (gray.len(), per_line),
                        None => df_edit_nodes(own, 0, 0),
                    };
                    let gray = df_edit_recycled(i, key, count);
                    (!placeholder).then_some((gray, per_line))
                }
                _ => nodes,
            };
            let mut caret_done = false;
            for (n, el) in lines.iter().enumerate() {
                let (start, dx, dy, lh) = (el.start, el.x, el.y, el.h);
                let line = &el.shaped;
                let len = el.text.chars().count();
                let (a, b) = (s0.clamp(start, start + len), s1.clamp(start, start + len));
                // As QQuickTextNodeEngine: the line in its colour, the selection over it, and
                // within the selection the line in the colour of selected text, so what a
                // glyph has outside the selection keeps the colour of the text.
                match &nodes {
                    Some((gray, per_line)) => {
                        for (node, part) in &per_line[n] {
                            let df = df.map(|d| DfText {
                                gray: gray[*node],
                                ..d
                            });
                            rt_line(cv, &f, part, x, y, dx, dy, col, op, df);
                        }
                    }
                    None => rt_line(cv, &f, line, x, y, dx, dy, col, op, df),
                }
                if a < b {
                    for (xa, xb) in el.ranges(a - start, b - start) {
                        // The rectangle of the selection is not antialiased and not rounded:
                        // it covers the pixels whose centres are in it.
                        let r = [x + dx + xa, y + dy, xb - xa, lh];
                        solid_rect(cv, r[0], r[1], r[2], r[3], sel_bg, op);
                        clipped(cv, r, &|cv| {
                            rt_line(cv, &f, line, x, y, dx, dy, sel_fg, op, df_selected)
                        });
                    }
                }
                if caret
                    && !caret_done
                    && let Some(c) = cursor
                    && (start..=start + len).contains(&c)
                {
                    caret_done = true;
                    let (cx, cy, ch) = match &empty {
                        Some(e) => (e.x, e.y, e.h),
                        None => (el.cursor_to_x(c - start), dy, lh),
                    };
                    // The text is inside the field's clip, the caret need not be: an empty
                    // TextArea is QFontMetricsF::height() high and its caret a document line,
                    // which has the font's leading too.
                    let (top, bottom) = (y + cy, y + cy + ch);
                    let cut = field_rect
                        .filter(|(r, fits)| {
                            *fits && (top < r[1] - 0.01 || bottom > r[1] + r[3] + 0.01)
                        })
                        .map(|(r, _)| clip_push(cv, r));
                    solid_rect(cv, (x + cx).round(), top, caret_w, ch, caret_col, op);
                    if let Some(c) = cut {
                        clip_pop(cv, c);
                    }
                }
            }
        }
        if let Some(c) = clip {
            clip_pop(cv, c);
        }
    } else if is(w, i, "RectangularShadow") && wd > 0.0 && ht > 0.0 {
        paint_rect_shadow(w, i, cv, x, y, wd, ht, op);
    } else if is(w, i, "RadialGradient") && wd > 0.0 && ht > 0.0 {
        paint_radial(w, fonts, i, cv, x, y, wd, ht, op);
    } else if is(w, i, "Canvas") && wd > 0.0 && ht > 0.0 {
        // Recorded ops changing touch the Canvas (Runtime::paint_canvases), like a property write.
        canvas_raster(w, i, cv, x, y, wd, ht, op);
    } else if is(w, i, "Shape") {
        let spec = shape_spec(w, i);
        // A CurveRenderer fill takes derivatives in 2 x 2 blocks of the target's pixels: a
        // raster made at an even position is not the one for an odd position.
        // A Shape with a layer of its own is drawn into that layer's texture, whose blocks
        // start at the item's corner.
        let own_layer = truthy(w, i, "layer.enabled", false);
        let parity = |v: f64| {
            if own_layer {
                0.0
            } else {
                (v.floor() as i64 & 1) as f64
            }
        };
        // The fills of a CurveRenderer Shape go to the graphics card as Qt's triangles under
        // Qt's shader, where the frame draws as Qt's renderer does: a texture of them made
        // here would blend overlapping triangles with each other before the target.
        let direct = match cv {
            Target::Gpu(fr) => {
                !own_layer
                    && spec.0
                    && spec.1.iter().all(|(sp, _)| sp.stroke.is_none())
                    && fr.projected().is_some()
            }
            Target::Cpu(_) => false,
        };
        if let Target::Gpu(fr) = cv
            && direct
        {
            let key = shape_key(&spec, &[]);
            let hit = CURVES.with_borrow_mut(|m| {
                let e = m.get(&i).filter(|e| e.0 == key);
                e.map(|e| e.1.clone())
            });
            let tris = hit.unwrap_or_else(|| {
                let fill = |sp: &ShapePathSpec| match sp.fc[3] {
                    0 => Vec::new(),
                    _ => qsg::shape::fill_tris(&sp.cmds, sp.start, sp.even_odd, 0),
                };
                let t: Vec<_> = spec.1.iter().map(|(sp, _)| fill(sp)).collect();
                let n = t.iter().map(|t| std::mem::size_of_val(&t[..])).sum();
                let t = Rc::new(t);
                CURVES.with_borrow_mut(|m| m.insert(i, (key, t.clone()), n));
                t
            });
            for ((sp, _), t) in spec.1.iter().zip(tris.iter()) {
                if !t.is_empty() {
                    fr.curve_fill(t, x, y, sp.fc, op);
                }
            }
        } else {
            let key = shape_key(
                &spec,
                &[
                    wd,
                    ht,
                    op as f64,
                    parity(x),
                    parity(y),
                    own_layer as u8 as f64,
                ],
            );
            // The frame Qt projects the fill onto, and its scale: the target itself, for a
            // Shape that is not a layer.
            let target = match cv {
                Target::Cpu(c) => Some(([0.0, 0.0, c.w as f64, c.h as f64], [c.w, c.h], [1.0; 2])),
                Target::Gpu(fr) => {
                    let [fx, fy, ..] = fr.xform();
                    fr.projected().map(|over| (over, [fr.w, fr.h], [fx, fy]))
                }
            };
            raster_keyed(
                cv,
                i,
                key,
                (x - 8.0, y - 8.0, wd + 16.0, ht + 16.0),
                |c, [sx, sy, tx, ty]| {
                    let origin = if own_layer {
                        (
                            (-(sx * x + tx)).round() as i64,
                            (-(sy * y + ty)).round() as i64,
                        )
                    } else {
                        ((-tx).round() as i64, (-ty).round() as i64)
                    };
                    let frame = target.filter(|t| !own_layer && t.2 == [sx, sy]).map(
                        |(over, [pw, ph], _)| qsg::shape::FillFrame {
                            over,
                            pixels: [pw as f32, ph as f32],
                            at: [x as f32, y as f32],
                        },
                    );
                    paint_shape(
                        &spec,
                        c,
                        sx * x + tx,
                        sy * y + ty,
                        wd,
                        ht,
                        op,
                        sx,
                        sy,
                        (origin, own_layer, frame.as_ref()),
                    )
                },
            );
        }
    } else if (is(w, i, "Image") || is(w, i, "AnimatedImage") || is_icon(w, i))
        && wd > 0.0
        && ht > 0.0
    {
        let dpr = match cv {
            Target::Gpu(fr) => fr.dpr,
            Target::Cpu(_) => 1.0,
        };
        let (src, ss, fk, _) = image_src(w, i, wd, ht, dpr);
        let fade = match is_icon(w, i) {
            true => icon_fade(w, i, &(src.clone(), ss, fk), (wd, ht)),
            false => None,
        };
        // An icon provider's pixmap is drawn by the Image's own fillMode.
        let mode = if fk == PROVIDER_FILL {
            num(w, i, "fillMode") as i32
        } else {
            fk
        };
        let fill = match mode {
            1 => qsg::image::FillMode::PreserveAspectFit,
            2 => qsg::image::FillMode::PreserveAspectCrop,
            _ => qsg::image::FillMode::Stretch,
        };
        let asynchronous = !is_icon(w, i) && truthy(w, i, "asynchronous", false);
        let tex = match animated_texture(w, i, &src, ss, asynchronous) {
            Some(t) => t,
            // A picture still Loading has no pixmap yet: it draws the one it retains.
            None if !is_icon(w, i) && num(w, i, "status") == 2.0 => {
                retained_texture(image_loads(w), i, matches!(cv, Target::Cpu(_)))
            }
            None => {
                // A Ready picture has its pixmap while its file is decoded for another one.
                let ready = !is_icon(w, i) && num(w, i, "status") == 1.0;
                let loads = image_loads(w);
                let t = match ready && decode_pending(loads, &src) {
                    true => {
                        let key = (src.clone(), ss, fk);
                        IMAGES.with(|c| c.borrow_mut().get(&key).cloned()).flatten()
                    }
                    false => image_texture(loads, &src, ss, fk, asynchronous),
                };
                if fade.is_some() || matches!(cv, Target::Cpu(_)) {
                    cpu_pixels(image_loads(w), &src, ss, fk, t)
                } else {
                    t
                }
            }
        };
        let tex = match fade {
            Some(((src, ss, fk), mix)) => {
                let old = image_texture(image_loads(w), &src, ss, fk, false);
                let old = cpu_pixels(image_loads(w), &src, ss, fk, old);
                icon_mix(old.as_deref(), tex.as_deref(), mix)
            }
            None => tex,
        };
        // icon.frag tints what the filter gives of the mixed pixmaps.
        let mask = (is_icon(w, i) && truthy(w, i, "isMask", false))
            .then(|| color(w, i, "color", [0, 0, 0, 255]));
        if tex.is_none() {
            no_cache();
        }
        if let Some(tex) = tex {
            let smooth = !explicit(w, i, "smooth") || truthy(w, i, "smooth", true);
            // An icon's pixmap is stretched over its painted rectangle.
            let (x, y, wd, ht, fill) = if is_icon(w, i) {
                let valid = icon_size_valid(w, i);
                // A QIcon's pixmap is in pixels; what is loaded like an Image is in units.
                let ratio = if fk < 0 { dpr } else { 1.0 };
                let round = truthy(w, i, "roundToIconSize", true);
                let r = icon_rect((tex.w, tex.h), (wd, ht), valid, ratio, round);
                let stretch = qsg::image::FillMode::Stretch;
                // Each vertex goes to a whole pixel on its own.
                let (x0, y0) = icon_corner(x + r[0], y + r[1], dpr);
                let (x1, y1) = icon_corner(x + r[0] + r[2], y + r[1] + r[3], dpr);
                (x0, y0, x1 - x0, y1 - y0, stretch)
            } else {
                (x, y, wd, ht, fill)
            };
            // QQuickImage::updatePaintNode: `mipmap` is a property of Image alone.
            let mipmap = !is_icon(w, i) && truthy(w, i, "mipmap", false);
            let unblended = !is_icon(w, i) && unblended_frame(w, i, &tex, op);
            match (&mut *cv, mask) {
                (Target::Gpu(fr), mask) => {
                    if let Some((r, uv)) = qsg::image::placement(tex.w, tex.h, wd, ht, fill) {
                        let rect = [x + r[0], y + r[1], r[2], r[3]];
                        match (mask, unblended, mipmap) {
                            (Some(k), ..) => fr.image_mask(tex, rect, uv, op, smooth, k),
                            (None, true, _) => fr.image_unblended(tex, rect, uv, smooth),
                            (None, false, true) => fr.image_mipmapped(tex, rect, uv, op, smooth),
                            (None, false, false) => fr.image(tex, rect, uv, op, smooth),
                        }
                    }
                }
                (Target::Cpu(c), Some(k)) => cpu_direct(c, |c, s| {
                    qsg::image::draw_mask(c, &tex, x, y, wd, ht, fill, smooth, op, k, s)
                }),
                (Target::Cpu(c), None) if unblended => cpu_direct(c, |c, s| {
                    qsg::image::draw_unblended_scaled(c, &tex, x, y, wd, ht, fill, smooth, s)
                }),
                (Target::Cpu(c), None) if mipmap => cpu_direct(c, |c, s| {
                    let levels = || mip_levels(&tex);
                    qsg::image::draw_mipmapped_scaled(
                        c, &tex, levels, x, y, wd, ht, fill, smooth, op, s,
                    )
                }),
                (Target::Cpu(c), None) => cpu_image(c, &tex, [x, y, wd, ht], fill, smooth, op),
            }
        }
    } else if is(w, i, "ScreencopyView") && wd > 0.0 && ht > 0.0 {
        no_cache();
        if let Some(tex) = SCREENCOPY.with(|c| c.borrow().get(&i).cloned()) {
            match cv {
                Target::Gpu(fr) => fr.image(tex, [x, y, wd, ht], [0.0, 0.0, 1.0, 1.0], op, true),
                // A dmabuf frame has no CPU pixels.
                Target::Cpu(_) if tex.px.len() != tex.w * tex.h => {}
                Target::Cpu(c) => {
                    let fill = qsg::image::FillMode::Stretch;
                    cpu_image(c, &tex, [x, y, wd, ht], fill, true, op)
                }
            }
        }
    } else if is(w, i, "MultiEffect") {
        if let Some(Value::Item(src)) = effect_src(w, i) {
            let mut p = qsg::multieffect::Params {
                blur_enabled: truthy(w, i, "blurEnabled", false),
                blur: num(w, i, "blur") as f32,
                blur_max: (opt_num(w, i, "blurMax").unwrap_or(32.0) as i32)
                    .min(qsg::blur::MAX_BLUR_RADIUS),
                blur_multiplier: num(w, i, "blurMultiplier") as f32,
                saturation: num(w, i, "saturation") as f32,
                contrast: num(w, i, "contrast") as f32,
                brightness: num(w, i, "brightness") as f32,
                colorization: num(w, i, "colorization") as f32,
                colorization_color: color16(w, i, "colorizationColor", [65535, 0, 0, 65535]),
                auto_padding: truthy(w, i, "autoPaddingEnabled", true),
                padding: ["x", "y", "width", "height"]
                    .map(|k| num(w, i, &format!("paddingRect.{k}"))),
                mask_threshold_min: num(w, i, "maskThresholdMin") as f32,
                mask_spread_at_min: num(w, i, "maskSpreadAtMin") as f32,
                mask_threshold_max: opt_num(w, i, "maskThresholdMax").unwrap_or(1.0) as f32,
                mask_spread_at_max: num(w, i, "maskSpreadAtMax") as f32,
                mask_inverted: truthy(w, i, "maskInverted", false),
                shadow_enabled: truthy(w, i, "shadowEnabled", false),
                shadow_opacity: opt_num(w, i, "shadowOpacity").unwrap_or(1.0) as f32,
                shadow_blur: opt_num(w, i, "shadowBlur").unwrap_or(1.0) as f32,
                shadow_horizontal_offset: num(w, i, "shadowHorizontalOffset") as f32,
                shadow_vertical_offset: num(w, i, "shadowVerticalOffset") as f32,
                shadow_color: color16(w, i, "shadowColor", [0, 0, 0, 65535]),
                shadow_scale: opt_num(w, i, "shadowScale").unwrap_or(1.0) as f32,
                ..Default::default()
            };
            let mask_src = match w.read(i, "maskSource") {
                Some(Value::Item(m)) if truthy(w, i, "maskEnabled", false) => Some(m),
                _ => None,
            };
            // The shader reads the mask's own texture: a layer's is smooth with `layer.smooth`
            // (false unless set), an Image's or ShaderEffectSource's with `smooth`.
            p.mask_smooth = mask_src.is_none_or(|m| {
                if truthy(w, m, "layer.enabled", false) {
                    truthy(w, m, "layer.smooth", false)
                } else {
                    !explicit(w, m, "smooth") || truthy(w, m, "smooth", true)
                }
            });
            // On the GPU the effect is shader passes over the source (and mask) layer textures,
            // as in Qt: no CPU shading or texture upload when its inputs change, whatever the
            // sizes of the source and of the effect (`gpu_source` draws the source's rect at
            // the screen's scale). The blur levels are there for the shadow as well.
            let blurred = qsg::multieffect::blur_items(&p) > 0;
            let (sw, sh) = (num(w, src, "width"), num(w, src, "height"));
            let gpu = gpu_dpr(cv);
            // updateSourcePadding(): the effect grows by the padding on each side, and its
            // ShaderEffectSource's sourceRect by the padding in the source item's units.
            let pads = qsg::multieffect::pads(&p);
            let [l, t, r, b] = pads.map(|k| k as f64);
            let (bw, mx) = if sw > 0.0 && wd > 0.0 {
                (sw, sw / wd)
            } else {
                (wd.max(sw), 1.0)
            };
            let (bh, my) = if sh > 0.0 && ht > 0.0 {
                (sh, sh / ht)
            } else {
                (ht.max(sh), 1.0)
            };
            let srect = [-l * mx, -t * my, bw + (l + r) * mx, bh + (t + b) * my];
            if let Some(dpr) = gpu {
                // The source and mask textures are their rects times the devicePixelRatio, as
                // QQuickShaderEffectSource sizes them; the BlurItems stay in scene units.
                let sf = gpu_source(w, fonts, src, srect, dpr);
                let mf = mask_src.map(|m| gpu_layer_dpr(w, fonts, m, dpr));
                let u = qsg::multieffect::shader_uniforms(&p);
                let rect = [x - l, y - t, wd + l + r, ht + t + b];
                let blur = blurred.then(|| {
                    // The first BlurItem is half the source rect, rounded up to 8.
                    let (pw, ph) = (srect[2].ceil() as usize, srect[3].ceil() as usize);
                    let sizes = qsg::multieffect::blur_sizes(&p, pw, ph);
                    let weights = if qsg::multieffect::blurs_source(&p) {
                        let all = qsg::multieffect::weights(p.blur, p.blur_max);
                        qsg::multieffect::level_weights(all, sizes.len())
                    } else {
                        [1.0, 0.0, 0.0, 0.0, 0.0, 0.0]
                    };
                    crate::gpu::Blur {
                        sizes,
                        dpr,
                        multiplier: p.blur_multiplier,
                        weights,
                    }
                });
                let shadow = qsg::multieffect::shadow(&p, rect[2], rect[3]);
                if let Target::Gpu(fr) = cv {
                    fr.effect(sf, mf, p.mask_smooth, rect, u, op, blur, shadow);
                }
                return;
            }
            let t_layer = qml::log_on!("ri::pprof").then(std::time::Instant::now);
            // Source and mask layers are cached until the item or a descendant changes, like the
            // layer textures Qt keeps between frames.
            let st = cached_layer(w, fonts, src);
            let t_layer = t_layer.map(|t| t.elapsed()).unwrap_or_default();
            let mask = mask_src.map(|m| cached_layer(w, fonts, m));
            // The effect draws inside its padded rect.
            let pad = qsg::multieffect::pads(&p)
                .into_iter()
                .max()
                .unwrap_or(0)
                .max(0) as f64
                + 1.0;
            raster_cpu(cv, (wd + 2.0 * pad, ht + 2.0 * pad), |c| {
                let began = std::time::Instant::now();
                // Read at the effect's texture coordinate, whatever its size.
                p.mask = mask.as_ref().map(|m| copy_canvas(&m.px));
                let rect = [x - l, y - t, wd + l + r, ht + t + b];
                // The source's rect grown by whole pixels is its texture inside a border.
                let unit = (bw, bh) == (wd, ht) && (st.px.w as f64, st.px.h as f64) == (wd, ht);
                let (st, border) = if pads == [0; 4] || unit {
                    (st.px.clone(), pads)
                } else {
                    (Rc::new(cpu_source(w, fonts, src, srect)), [0; 4])
                };
                cpu_direct(c, |c, s| {
                    qsg::multieffect::draw_source(c, &st, border, &p, rect, s, op)
                });
                if qml::log_on!("ri::pprof") {
                    tracing::debug!(
                        target: "ri::pprof",
                        "meffect {i} layer {:?} draw {:?} {}x{}",
                        t_layer,
                        began.elapsed(),
                        st.w,
                        st.h
                    );
                }
            });
        }
        return;
    } else if is(w, i, "GaussianBlur") {
        // Qt5Compat GaussianBlur; its own children draw on top of the result: GaussianBlur.qml's
        // two shader passes, on the GPU and on the CPU.
        if let Some(Value::Item(src)) = effect_src(w, i) {
            // `radius` is a real: a radius that is not whole blurs with the fallback shader.
            let (radius, samples) = gauss_radius_samples(w, i);
            // GaussianBlur.qml binds `deviation` to (radius + 1) / 3.3333 unless the QML sets it.
            let deviation = opt_num(w, i, "deviation");
            let tb = truthy(w, i, "transparentBorder", false);
            let r = if tb { qsg::blur::border(radius) } else { 0.0 };
            if let Some(dpr) = gpu_dpr(cv) {
                let at = [x, y, wd, ht];
                let direct = gauss_direct(w, i, src, tb, false);
                let b = gpu_gauss(
                    w, fonts, src, at, radius, samples, deviation, tb, dpr, None, direct,
                );
                if let (Some(b), Target::Gpu(fr)) = (b, &mut *cv) {
                    // GaussianBlur.qml's `verticalBlur`: a child at (-radius, -radius) with
                    // the border, the effect's own size without.
                    let mut mv = node_matrix(w, i);
                    if r != 0.0 {
                        let mut at = Mat4::identity();
                        at.translate(-r as f32, -r as f32, 0.0);
                        mv = mv.mul(&at);
                    }
                    let item = [0.0, 0.0, b.rect[2] as f32, b.rect[3] as f32];
                    fr.gauss(b.proxy, b.kernel, b.layer, b.rect, Some((item, mv)), op);
                }
            } else {
                let direct = gauss_direct(w, i, src, tb, true);
                let pad = if tb { r + 2.0 } else { 1.0 };
                raster_cpu(cv, (wd + 2.0 * pad, ht + 2.0 * pad), |c| {
                    let proxy = direct.unwrap_or_else(|| {
                        let st = cached_layer(w, fonts, src);
                        cpu_gauss_proxy(w, fonts, &st, src, wd, ht, r)
                    });
                    let (size, at) = ((wd, ht), [x, y]);
                    let placed = cpu_placed(w, i, r);
                    cpu_direct(c, |c, s| {
                        qsg::blur::gaussian_blur_proxy(
                            c, &proxy, size, radius, samples, deviation, tb, at[0], at[1], op, s,
                            placed,
                        );
                    });
                });
            }
        }
    } else if is(w, i, "DropShadow") {
        if let Some(Value::Item(src)) = effect_src(w, i) {
            let (radius, samples) = gauss_radius_samples(w, i);
            let col = color16(w, i, "color", [0, 0, 0, 65535]);
            let spread = opt_num(w, i, "spread").unwrap_or(0.0) as f32;
            let off = (
                opt_num(w, i, "horizontalOffset").unwrap_or(0.0),
                opt_num(w, i, "verticalOffset").unwrap_or(0.0),
            );
            // DropShadowBase.qml: `property bool transparentBorder: true`.
            let tb = truthy(w, i, "transparentBorder", true);
            if let Some(dpr) = gpu_dpr(cv) {
                // DropShadowBase.qml: the alpha-only blur at the rounded offset, its radius and
                // samples (an int, truncated) times the devicePixelRatio; then the source proxy
                // over `_outputRect`.
                let tint = qsg::shadow::shader_color(col);
                let shadow = Some((tint, qsg::blur::shadow_thickness(spread)));
                let at = [x + off.0.round(), y + off.1.round(), wd, ht];
                let (radius, samples) = (radius * dpr, (samples as f64 * dpr) as i32);
                let b = gpu_gauss(
                    w, fonts, src, at, radius, samples, None, tb, dpr, shadow, None,
                );
                if let (Some(b), Target::Gpu(fr)) = (b, &mut *cv) {
                    // `Frame::layer` maps the proxy's texels over its devicePixelRatio.
                    let [ox, oy, ow, oh] = b.output;
                    let (sx, sy) = (
                        ow * dpr / b.proxy.w.max(1) as f64,
                        oh * dpr / b.proxy.h.max(1) as f64,
                    );
                    let m = [sx, 0.0, 0.0, sy, ox - off.0.round(), oy - off.1.round()];
                    fr.gauss(b.proxy.clone(), b.kernel, b.layer, b.rect, None, op);
                    fr.layer(b.proxy, m, op);
                }
                return;
            }
            let r = if tb { qsg::blur::border(radius) } else { 0.0 };
            let st = cached_layer(w, fonts, src);
            let pad = qsg::blur::border(radius) + off.0.abs().max(off.1.abs()) + 2.0;
            raster_cpu(cv, (wd + 2.0 * pad, ht + 2.0 * pad), |c| {
                let proxy = cpu_gauss_proxy(w, fonts, &st, src, wd, ht, r);
                let (size, at) = ((wd, ht), [x, y]);
                cpu_direct(c, |c, s| {
                    qsg::blur::drop_shadow(
                        c, &proxy, size, radius, samples, col, spread, off, tb, at[0], at[1], op, s,
                    );
                });
            });
        }
        return;
    } else if is(w, i, "Desaturate") {
        // desaturate.frag mixes by `desaturation` as given: outside 0..1 it extrapolates.
        if let (Some(Value::Item(src)), Target::Gpu(fr)) = (effect_src(w, i), &mut *cv) {
            let d = num(w, i, "desaturation") as f32;
            let smooth = effect_src_smooth(w, i, src);
            let sz = [num(w, src, "width"), num(w, src, "height")];
            let sf = gpu_layer_dpr(w, fonts, src, fr.dpr);
            if let Target::Gpu(fr) = cv {
                fr.tint(sf, sz, [0; 4], Some(d), smooth, [x, y, wd, ht], op);
            }
        } else if let Some(Value::Item(src)) = effect_src(w, i) {
            let st = render_layer(w, fonts, src);
            let d = num(w, i, "desaturation") as f32;
            let smooth = effect_src_smooth(w, i, src);
            match cv {
                Target::Gpu(_) => unreachable!(),
                Target::Cpu(cv) => {
                    cpu_direct(cv, |c, [sx, sy]| {
                        let (x, y, wd, ht) = (sx * x, sy * y, sx * wd, sy * ht);
                        qsg::layer::draw_desaturate(c, &st, d, smooth, x, y, wd, ht, op)
                    });
                }
            }
        }
        return;
    } else if is(w, i, "ColorOverlay") {
        if let (Some(Value::Item(src)), Target::Gpu(fr)) = (effect_src(w, i), &mut *cv) {
            let c = color16(w, i, "color", [0; 4]);
            let smooth = effect_src_smooth(w, i, src);
            let sz = [num(w, src, "width"), num(w, src, "height")];
            let sf = gpu_layer_dpr(w, fonts, src, fr.dpr);
            if let Target::Gpu(fr) = cv {
                fr.tint(sf, sz, c, None, smooth, [x, y, wd, ht], op);
            }
        } else if let Some(Value::Item(src)) = effect_src(w, i) {
            let st = render_layer(w, fonts, src);
            let c = color16(w, i, "color", [0; 4]);
            let smooth = effect_src_smooth(w, i, src);
            match cv {
                Target::Gpu(_) => unreachable!(),
                Target::Cpu(cv) => {
                    cpu_direct(cv, |cv, [sx, sy]| {
                        let (x, y, wd, ht) = (sx * x, sy * y, sx * wd, sy * ht);
                        qsg::layer::draw_color_overlay(cv, &st, c, smooth, x, y, wd, ht, op)
                    });
                }
            }
        }
        return;
    } else if is(w, i, "OpacityMask") {
        if let (Some(Value::Item(src)), Some(Value::Item(mask))) =
            (effect_src(w, i), w.read(i, "maskSource"))
        {
            let inv = truthy(w, i, "invert", false);
            if let Target::Gpu(fr) = cv {
                let dpr = fr.dpr;
                let lift = lift_for(w, fr, src, mask, [x, y, wd, ht], inv, op);
                let (sf, lifted) = lifted_layer(w, fonts, src, dpr, lift);
                let mf = gpu_layer_dpr(w, fonts, mask, dpr);
                if let Target::Gpu(fr) = cv {
                    fr.masked(sf, mf, [x, y, wd, ht], inv, op, i as u64 + 1);
                    for l in lifted {
                        let [lx, ly, lw, lh] = l.rect;
                        let at = [x + lx, y + ly, lw, lh];
                        fr.image(l.tex, at, [0.0, 0.0, 1.0, 1.0], l.op, l.smooth);
                    }
                }
                return;
            }
            let st = cached_layer(w, fonts, src).px;
            let mt = cached_layer(w, fonts, mask).px;
            if let Target::Cpu(c) = cv {
                cpu_direct(c, |c, [sx, sy]| {
                    let (x, y, wd, ht) = (sx * x, sy * y, sx * wd, sy * ht);
                    qsg::layer::draw_opacity_mask(c, &st, &mt, inv, x, y, wd, ht, op)
                });
            }
        }
        return;
    }
    let edit_bg = edit_background(w, i);
    for c in stacked_kids(w, i) {
        if Some(c) == edit_bg {
            continue;
        }
        if is(w, c, "ToolTip") || is_popup(w, c) {
            // A hidden ToolTip draws nothing; showing it writes `visible`, which drops this draw.
            if is(w, c, "ToolTip") && !visible(w, c) {
                continue;
            }
            OVERLAY.with(|o| o.borrow_mut().push(c));
            no_cache();
            continue;
        }
        // QQuickItemView culls delegates outside the view, overflowing children included.
        if is(w, i, "SwipeView") {
            let (cx, cw) = if num(w, i, "orientation") == 2.0 {
                (num(w, c, "y"), num(w, c, "height"))
            } else {
                (num(w, c, "x"), num(w, c, "width"))
            };
            let extent = if num(w, i, "orientation") == 2.0 {
                "height"
            } else {
                "width"
            };
            if cx + cw <= 0.0 || cx >= num(w, i, extent) {
                continue;
            }
        }
        if view_culls(w, i, c) {
            continue;
        }
        paint(w, fonts, c, cv, x, y, op, false);
    }
}

/// QQuickItemPrivate::setCulled: a delegate `c` in the contentItem `i` of a view that lies
/// outside the view is neither painted nor hit.
pub(crate) fn view_culls(w: &mut World, i: usize, c: usize) -> bool {
    let Some(v) = view_of_content(w, i) else {
        return false;
    };
    // A pooled delegate, and an ObjectModel's item the view holds no row of.
    if w.view_pooled.contains(&c) {
        return true;
    }
    let held = w.view_rows.get(&v).and_then(|r| r.held).map(|h| h.1);
    if held != Some(c) && !w.delegated.get(&v).is_some_and(|d| d.1.contains(&c)) {
        return false;
    }
    let (ox, oy) = (num(w, i, "x"), num(w, i, "y"));
    !view_overlaps(w, v, c, -ox, -oy)
}

/// Whether view `i`'s child `c` (scrolled by `sx`, `sy`) intersects the view: QQuickItemView
/// only keeps delegates inside its visible area (plus cacheBuffer, which is never painted).
fn view_overlaps(w: &mut World, i: usize, c: usize, sx: f64, sy: f64) -> bool {
    let (cx, cy) = (num(w, c, "x") - sx, num(w, c, "y") - sy);
    let (cw, ch) = (num(w, c, "width"), num(w, c, "height"));
    let (vw, vh) = (num(w, i, "width"), num(w, i, "height"));
    cx + cw > 0.0 && cy + ch > 0.0 && cx < vw && cy < vh
}

/// The stops of Gradient `g`, sorted by position: its GradientStop children.
fn gradient_stops(w: &mut World, g: usize) -> Vec<(f32, [u8; 4])> {
    let mut stops: Vec<(f32, [u8; 4])> = vec![];
    for c in w.items[g].children.clone() {
        if !w.dead.contains(&c) && is(w, c, "GradientStop") {
            stops.push((
                num(w, c, "position") as f32,
                color(w, c, "color", [0, 0, 0, 255]),
            ));
        }
    }
    stops.sort_by(|a, b| a.0.total_cmp(&b.0));
    stops
}

thread_local! {
    /// RadialGradient colour textures by their stops and orientation.
    static RADIAL_TEX: std::cell::RefCell<ByteLru<u64, Rc<Canvas>>> =
        std::cell::RefCell::new(ByteLru::new(1 << 20).max_len(64));
}

/// Paints Qt5Compat RadialGradient `i` (scene x y, size wd ht) as RadialGradient.qml's
/// ShaderEffect does: its `gradient` (white to black when unset) as the texture of a 16 x 256
/// Rectangle at the window's devicePixelRatio (`qsg::gradient::gradient_texture`) read by
/// the distance from the centre, with the radii the item's size when unset, times the alpha
/// of the `source` item's texture when there is one.
#[allow(clippy::too_many_arguments)]
fn paint_radial(
    w: &mut World,
    fonts: &mut Fonts,
    i: usize,
    cv: &mut Target,
    x: f64,
    y: f64,
    wd: f64,
    ht: f64,
    op: f32,
) {
    let (stops, vertical) = match w.read(i, "gradient") {
        Some(Value::Item(g)) => (gradient_stops(w, g), num(w, g, "orientation") != 1.0),
        _ => (vec![(0.0, [255; 4]), (1.0, [0, 0, 0, 255])], true),
    };
    let dpr = match cv {
        Target::Gpu(fr) => fr.dpr,
        Target::Cpu(_) => 1.0,
    };
    let key = {
        use std::hash::{Hash, Hasher};
        let mut h = rustc_hash::FxHasher::default();
        for (p, c) in &stops {
            (p.to_bits(), c).hash(&mut h);
        }
        (vertical, dpr.to_bits()).hash(&mut h);
        h.finish()
    };
    let tex = RADIAL_TEX.with_borrow_mut(|m| {
        m.get(&key).cloned().unwrap_or_else(|| {
            let t = Rc::new(qsg::gradient::gradient_texture(&stops, vertical, dpr));
            m.insert(key, t.clone(), canvas_bytes(&t));
            t
        })
    });
    let r = qsg::gradient::Radial::new(
        wd,
        ht,
        opt_num(w, i, "horizontalOffset").unwrap_or(0.0),
        opt_num(w, i, "verticalOffset").unwrap_or(0.0),
        opt_num(w, i, "horizontalRadius").unwrap_or(wd),
        opt_num(w, i, "verticalRadius").unwrap_or(ht),
        opt_num(w, i, "angle").unwrap_or(0.0),
    );
    let src = match effect_src(w, i) {
        Some(Value::Item(src)) => Some(src),
        _ => None,
    };
    if let Target::Gpu(_) = cv {
        let mask = src.map(|src| {
            let sz = [num(w, src, "width"), num(w, src, "height")];
            (gpu_layer_dpr(w, fonts, src, dpr), sz)
        });
        // RadialGradient.qml's ShaderEffect fills the item: its node is the item's, in the
        // window or in the texture being drawn, whichever has the item here.
        let tops = [LAYER_SKIP.get(), SOURCE_ROOT.get(), None];
        let node = tops.into_iter().find_map(|top| {
            let m = node_matrix_under(w, i, top);
            let at = m.map3([0.0; 3]);
            let here = (at[0] as f64 - x).abs() < 0.01 && (at[1] as f64 - y).abs() < 0.01;
            here.then_some(([0.0, 0.0, wd as f32, ht as f32], m))
        });
        if let Target::Gpu(fr) = cv {
            fr.radial(tex, r, mask, [x, y, wd, ht], node, op);
        }
        return;
    }
    let mask = src.map(|src| cached_layer(w, fonts, src).px);
    raster(cv, (x, y, wd, ht), |c, ox, oy| {
        qsg::gradient::radial(c, &tex, &r, mask.as_deref(), x - ox, y - oy, wd, ht, op)
    });
}

/// Paints RectangularShadow `i` (scene x y, size wd ht) once Qt has shown its material.
#[allow(clippy::too_many_arguments)]
fn paint_rect_shadow(
    w: &mut World,
    i: usize,
    cv: &mut Target,
    x: f64,
    y: f64,
    wd: f64,
    ht: f64,
    op: f32,
) {
    let Some((antialiasing, corners)) = w.rect_shadow_drawn(i) else {
        return;
    };
    let off = match w.read(i, "offset") {
        Some(Value::Obj(o)) => {
            let o = o.borrow();
            let n = |k: &str| o.props.get(k).map(|v| v.to_number()).unwrap_or(0.0);
            (n("x"), n("y"))
        }
        _ => (0.0, 0.0),
    };
    let p = qsg::shadow::ShadowParams {
        width: wd,
        height: ht,
        radius: opt_num(w, i, "radius").unwrap_or(0.0),
        blur: opt_num(w, i, "blur").unwrap_or(10.0),
        spread: opt_num(w, i, "spread").unwrap_or(0.0),
        offset: off,
        color: color16(w, i, "color", [0, 0, 0, 65535]),
        antialiasing,
        corners,
        cached: truthy(w, i, "cached", false),
    };
    match cv {
        Target::Cpu(c) => qsg::shadow::draw(cpu_plain(c), &p, x, y, op),
        Target::Gpu(fr) if p.cached => {
            // A layer texture of the item's own geometry, its size times the window's
            // devicePixelRatio: independent of its scene position, so moving shadows reuse
            // it; drawn with nearest sampling like Qt's layer.
            let dpr = fr.dpr;
            let key = {
                use std::hash::{Hash, Hasher};
                let mut h = rustc_hash::FxHasher::default();
                for v in [
                    p.width, p.height, p.radius, p.blur, p.spread, off.0, off.1, dpr,
                ] {
                    v.to_bits().hash(&mut h);
                }
                (p.color, p.antialiasing, "layer").hash(&mut h);
                p.corners.map(|c| c.map(f64::to_bits)).hash(&mut h);
                h.finish()
            };
            let tex = LAYER_SHADOWS.with(|m| {
                let mut m = m.borrow_mut();
                m.get(&key).cloned().or_else(|| {
                    let t = qsg::shadow::cached_tex(&p, dpr)?;
                    let t = (Rc::new(t.0), t.1);
                    let n = canvas_bytes(&t.0);
                    m.insert(key, t.clone(), n);
                    Some(t)
                })
            });
            if let Some((tex, r)) = tex {
                fr.image(
                    tex,
                    [x + r[0], y + r[1], r[2], r[3]],
                    [0.0, 0.0, 1.0, 1.0],
                    op,
                    false,
                );
            }
        }
        // rectangularshadow.frag per fragment, as Qt draws an uncached RectangularShadow:
        // nothing is rasterized or uploaded when it moves or resizes.
        Target::Gpu(fr) => {
            if let Some(sh) = qsg::shadow::shader(&p, x, y) {
                fr.rshadow(sh.rect, sh.uniforms, sh.radii, sh.color, op);
            }
        }
    }
}

/// RectangularShadow layer textures and their item-relative rects.
type LayerShadows = ByteLru<u64, (Rc<Canvas>, [f64; 4])>;

thread_local! {
    /// `cached` RectangularShadow layer textures and their item-relative rects.
    static LAYER_SHADOWS: std::cell::RefCell<LayerShadows> =
        std::cell::RefCell::new(ByteLru::new(16 << 20).max_len(64));
    static XFORM_SKIP: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
    /// While a subtree is drawn through its item's transform: the combined matrix of the
    /// item's QSGTransformNode, from the painted coordinates to the scene, which every node
    /// drawn takes (`takes_matrix`).
    static NODE_XF: std::cell::Cell<Option<Xf>> = const { std::cell::Cell::new(None) };
    /// The topmost item drawn through `NODE_XF` and where its parent's origin is in the target.
    static NODE_TOP: std::cell::Cell<Option<(usize, [f32; 2])>> =
        const { std::cell::Cell::new(None) };
    static LAYER_SKIP: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
    /// (effect, layered item) while painting a `layer.effect`: the effect's implicit source.
    static LAYER_SRC: std::cell::Cell<Option<(usize, usize)>> = const { std::cell::Cell::new(None) };
    /// Layered item whose GPU source frame is being painted with recorded draws enabled.
    static LAYER_DL: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
    /// Items whose source texture (a layer or a blur proxy) is being painted, outermost first,
    /// each with whether it is painted bare (as its own layer.effect's source).
    static SOURCES: std::cell::RefCell<Vec<(usize, bool)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Item `i` is being painted into a source texture until this is dropped.
struct Sourcing;

/// Marks `i` as being painted into a source texture. None when it already is: an effect whose
/// source is itself or its own ancestor would paint that source without end, so inside its
/// own source such an effect has nothing to show (Qt's ShaderEffectSource does not render a
/// source into itself either unless it is `recursive`). An item with a layer.effect is
/// painted twice on purpose, through the effect and bare inside it as the effect's source.
fn sourcing(i: usize) -> Option<Sourcing> {
    let key = (i, LAYER_SKIP.with(|s| s.get()) == Some(i));
    SOURCES.with(|s| {
        let mut s = s.borrow_mut();
        if s.contains(&key) {
            return None;
        }
        s.push(key);
        Some(Sourcing)
    })
}

impl Drop for Sourcing {
    fn drop(&mut self) {
        SOURCES.with(|s| s.borrow_mut().pop());
    }
}

/// The most pixels of one texture painted on the CPU (a layer, an effect's output, a Shape's
/// fill): 64 megapixels, two 8K screens. The GPU clamps its layers to GL_MAX_TEXTURE_SIZE.
const MAX_CPU_TEXTURE: f64 = (1u64 << 26) as f64;

/// Whether a `w` x `h` texture is within `MAX_CPU_TEXTURE`. One past it is not painted: its
/// size comes from QML, and allocating it would end the process.
fn cpu_texture_fits(w: f64, h: f64) -> bool {
    w.ceil().max(1.0) * h.ceil().max(1.0) <= MAX_CPU_TEXTURE
}

/// An effect's `source`, or the item whose layer it is drawn for.
fn effect_src(w: &mut World, e: usize) -> Option<Value> {
    match LAYER_SRC.with(|s| s.get()) {
        Some((le, i)) if le == e => Some(Value::Item(i)),
        _ => w.read(e, "source"),
    }
}

/// Whether ColorOverlay or Desaturate `e` reads its source `src` linearly. Their SourceProxy
/// has `interpolation: input.smooth ? Linear : Nearest`, and QGfxSourceProxy makes the texture
/// it outputs follow that. The input is `src` itself, or for a `layer.effect` the layer's
/// ShaderEffectSource, whose `smooth` is `layer.smooth`.
fn effect_src_smooth(w: &mut World, e: usize, src: usize) -> bool {
    match LAYER_SRC.with(|s| s.get()) {
        Some((le, _)) if le == e => truthy(w, src, "layer.smooth", false),
        _ => !explicit(w, src, "smooth") || truthy(w, src, "smooth", true),
    }
}

/// Children extents of `i` in its own coordinates (at least its own rect): each child's rect
/// where its transform and those of the items between show it. `xf` maps the coordinates
/// (`ox`, `oy`) are in to `i`'s, when a transformed item lies between.
fn subtree_extent(w: &mut World, i: usize, ox: f64, oy: f64, xf: Option<&Xf>, e: &mut [f64; 4]) {
    for c in visual_kids(w, i) {
        if !visible(w, c) {
            continue;
        }
        let (x, y) = (ox + num(w, c, "x"), oy + num(w, c, "y"));
        let (cw, chh) = (num(w, c, "width"), num(w, c, "height"));
        let own = w
            .item_transform(c)
            .map(|t| t.then(&Xf::translation(x, y)))
            .map(|t| xf.map_or(t, |o| t.then(o)));
        let (r, at, through) = match &own {
            Some(t) => (t.map_rect([0.0, 0.0, cw, chh]), (0.0, 0.0), Some(t)),
            None => (
                xf.map_or([x, y, cw, chh], |t| t.map_rect([x, y, cw, chh])),
                (x, y),
                xf,
            ),
        };
        if r.iter().all(|v| v.is_finite()) {
            e[0] = e[0].min(r[0]);
            e[1] = e[1].min(r[1]);
            e[2] = e[2].max(r[0] + r[2]);
            e[3] = e[3].max(r[1] + r[3]);
        }
        if !truthy(w, c, "clip", false) {
            subtree_extent(w, c, at.0, at.1, through, e);
        }
    }
}

/// An item's own transform (`World::item_transform`): its `transform` list, then `scale` and
/// `rotation` about `transformOrigin`.
pub(crate) struct ItemXf(pub Xf);

impl ItemXf {
    /// The point `p` of the item's parent seen without this transform: where an untransformed
    /// item at `pos` has the content this one shows at `p` (QQuickItem::mapFromItem's inverse
    /// of the item transform). None for a transform without an inverse (a zero scale), which
    /// shows nothing.
    pub fn undo(&self, pos: (f64, f64), p: (f64, f64)) -> Option<(f64, f64)> {
        let (x, y) = self.0.inverse()?.map(p.0 - pos.0, p.1 - pos.1);
        (x.is_finite() && y.is_finite()).then_some((pos.0 + x, pos.1 + y))
    }
}

/// The transform of item `i`, or None when it has none.
pub(crate) fn item_xf(w: &mut World, i: usize) -> Option<ItemXf> {
    w.item_transform(i).map(ItemXf)
}

/// A subtree turned by whole quarter turns (and scaled) keeps the target's axes, so it can be
/// drawn offscreen on the target's own pixel grid and copied there texel for texel: the
/// geometry lands where Qt, drawing it through the transform, puts it.
struct QuarterTurn {
    /// Texels to the subtree's unit along the texture's x and y.
    s: [f64; 2],
    /// Where the subtree's origin is inside the texture's first texel.
    f: [f64; 2],
    /// Texture texels to target pixels.
    m: [f64; 6],
    /// The texture's x and y run against the screen's (`qsg::text::snap_px`).
    against: [bool; 2],
}

/// `xf` is the subtree's transform, `ts` the target's pixels to the scene unit, `t` the target
/// pixel the subtree's origin maps to. `None` for a transform that is not whole quarter turns
/// and one scale along both axes. `outer` is the target's axes that run against the screen's.
fn quarter_turn(xf: &Xf, ts: [f64; 2], t: [f64; 2], outer: [bool; 2]) -> Option<QuarterTurn> {
    let [a, b, c, d, ..] = xf.affine()?;
    let (odd, sc) = if b == 0.0 && c == 0.0 && a == d {
        (false, a)
    } else if a == 0.0 && d == 0.0 && b == -c {
        (true, b)
    } else {
        return None;
    };
    let sign = sc.signum();
    // The target's axis each of the texture's runs along: its scale there and where the
    // texture's origin lands on it, growing (1) or falling (-1).
    let (s, f) = if odd {
        (
            [ts[1], ts[0]].map(|v| v * sc.abs()),
            [sign * t[1], -sign * t[0]],
        )
    } else {
        (ts.map(|v| v * sc.abs()), [sign * t[0], sign * t[1]])
    };
    let f = f.map(|v| v.rem_euclid(1.0));
    let m = if odd {
        let (x, y) = (t[0] + sign * f[1], t[1] - sign * f[0]);
        [0.0, sign, -sign, 0.0, x.round(), y.round()]
    } else {
        let (x, y) = (t[0] - sign * f[0], t[1] - sign * f[1]);
        [sign, 0.0, 0.0, sign, x.round(), y.round()]
    };
    // The texture's x and y fall along the target's axis they run along when its factor in
    // `m` is negative.
    let against = if odd {
        [(sign < 0.0) != outer[1], (sign > 0.0) != outer[0]]
    } else {
        [(sign < 0.0) != outer[0], (sign < 0.0) != outer[1]]
    };
    Some(QuarterTurn { s, f, m, against })
}

/// The layers of turned items open under the topmost one, innermost last, while their
/// subtrees are painted: `base` is the target the topmost one is drawn on and `xf` its scale
/// there; each layer is its target and what was cut out of it so far.
struct DfLater {
    base: usize,
    xf: Option<[f32; 4]>,
    layers: Vec<(usize, Vec<DfPiece>)>,
}

/// A part of a turned item's picture, in the order Qt draws it: what the layer held up to a
/// QtRendering text (with the transform that places it and its opacity), or that text.
enum DfPiece {
    Layer(DfContent, Xf, f32),
    Text(usize),
}

enum DfContent {
    Cpu(Canvas),
    Gpu(Box<crate::gpu::Frame>),
}

fn target_id(cv: &Target) -> usize {
    match cv {
        Target::Cpu(c) => &raw const **c as usize,
        Target::Gpu(f) => &raw const **f as usize,
    }
}

/// What `cv` holds, leaving it empty; None when it holds nothing. Of the stage of a scaled
/// layer that is the layer's texture. The canvas stays the one it was (a window's keeps a
/// window's rows): its pixels are taken.
fn take_content(cv: &mut Target) -> Option<DfContent> {
    fn pixels(c: &mut Canvas) -> Option<DfContent> {
        if c.px.iter().all(|p| *p == [0; 4]) {
            return None;
        }
        let px = std::mem::replace(&mut c.px, vec![[0; 4]; c.w * c.h]);
        Some(DfContent::Cpu(Canvas::from_px(c.w, c.h, px)))
    }
    match cv {
        Target::Cpu(c) => match cpu_scaled(c, |l| pixels(&mut l.out)) {
            Some(held) => held,
            None => pixels(c),
        },
        Target::Gpu(f) => f.take_recorded().map(|f| DfContent::Gpu(Box::new(f))),
    }
}

/// Text `i` is a QtRendering text under the turned item being drawn: Qt draws its glyph
/// quads through the node's whole matrix, antialiased along the screen's x, not along the
/// item's, so it stays out of the item's layer. The layer is cut here: what it holds so far
/// is drawn, then the text, then what comes after (`df_place`), which is Qt's order.
fn df_later(w: &mut World, fonts: &mut Fonts, i: usize, cv: &mut Target) -> bool {
    if DF_NOW.with(|n| n.get()) == Some(i)
        || !SOURCES.with(|s| s.borrow().is_empty())
        || !DF_ITEMS.with(|m| m.borrow().get(&i).is_some_and(|d| !d.plain && d.free))
    {
        return false;
    }
    let at = target_id(cv);
    let cut = DF_LATER.with(|l| match &*l.borrow() {
        Some(s) => match s.layers.last() {
            Some((id, _)) if *id == at => Some(true),
            None if s.base == at => Some(false),
            _ => None,
        },
        None => None,
    });
    match cut {
        None => false,
        Some(false) => {
            df_text_now(w, fonts, i, cv);
            true
        }
        Some(true) => {
            let held = take_content(cv);
            DF_LATER.with(|l| {
                if let Some((_, pieces)) = l.borrow_mut().as_mut().and_then(|s| s.layers.last_mut())
                {
                    pieces.extend(held.map(|c| DfPiece::Layer(c, Xf::translation(0.0, 0.0), 1.0)));
                    pieces.push(DfPiece::Text(i));
                }
            });
            true
        }
    }
}

/// QtRendering text `t` of a turned item on `cv`, the target that item is drawn on, through
/// its node's matrix.
fn df_text_now(w: &mut World, fonts: &mut Fonts, t: usize, cv: &mut Target) {
    let Some(d) = DF_ITEMS.with(|m| m.borrow().get(&t).copied()) else {
        return;
    };
    let (tx, ty) = (num(w, t, "x"), num(w, t, "y"));
    let held = DF_LATER.with(|l| l.borrow_mut().take());
    let scale = match (&mut *cv, &held) {
        (Target::Gpu(fr), Some(s)) => Some(std::mem::replace(&mut fr.xf, s.xf)),
        _ => None,
    };
    let on = DL_ON.with(|d| d.replace(false));
    let prev = DF_NOW.with(|n| n.replace(Some(t)));
    paint(
        w,
        fonts,
        t,
        cv,
        d.pos[0] - tx,
        d.pos[1] - ty,
        d.opacity,
        false,
    );
    DF_NOW.with(|n| n.set(prev));
    DL_ON.with(|d| d.set(on));
    if let (Target::Gpu(fr), Some(xf)) = (&mut *cv, scale) {
        fr.xf = xf;
    }
    DF_LATER.with(|l| *l.borrow_mut() = held);
}

/// A turned item's layer `layer` is about to be painted, to be drawn on `on`: its texts are
/// cut out of it when `on` is where the item above it is drawn.
fn df_open(layer: usize, on: usize) -> bool {
    DF_LATER.with(|l| match l.borrow_mut().as_mut() {
        Some(s) if s.layers.last().map_or(s.base, |l| l.0) == on => {
            s.layers.push((layer, Vec::new()));
            true
        }
        _ => false,
    })
}

/// The pieces cut out of the layer opened last.
fn df_close() -> Vec<DfPiece> {
    DF_LATER.with(|l| {
        l.borrow_mut()
            .as_mut()
            .and_then(|s| s.layers.pop())
            .map_or(Vec::new(), |l| l.1)
    })
}

/// The pieces of a turned item's layer, placed by `m` with `opacity`, onto `cv` in order; or,
/// when `cv` is the layer of a turned item above, into that one's pieces.
fn df_place(
    w: &mut World,
    fonts: &mut Fonts,
    cv: &mut Target,
    pieces: Vec<DfPiece>,
    m: &Xf,
    opacity: f32,
) {
    let pieces = pieces.into_iter().map(|p| match p {
        DfPiece::Layer(c, xf, op) => DfPiece::Layer(c, xf.then(m), op * opacity),
        text => text,
    });
    if DF_LATER.with(|l| l.borrow().as_ref().is_some_and(|s| !s.layers.is_empty())) {
        let held = take_content(cv);
        DF_LATER.with(|l| {
            if let Some((_, above)) = l.borrow_mut().as_mut().and_then(|s| s.layers.last_mut()) {
                above.extend(held.map(|c| DfPiece::Layer(c, Xf::translation(0.0, 0.0), 1.0)));
                above.extend(pieces);
            }
        });
        return;
    }
    for p in pieces {
        match (p, &mut *cv) {
            (DfPiece::Text(t), cv) => df_text_now(w, fonts, t, cv),
            (DfPiece::Layer(DfContent::Gpu(sub), xf, op), Target::Gpu(fr)) => match xf.affine() {
                Some(a) => fr.layer(*sub, a, op),
                None => fr.layer_projected(*sub, xf.m, op),
            },
            // `xf` takes the texture's texels to those of the target.
            (DfPiece::Layer(DfContent::Cpu(tex), xf, op), Target::Cpu(cvs)) => {
                cpu_direct(cvs, |c, _| match xf.affine() {
                    Some(a) => qsg::layer::draw_transformed(c, &tex, a, op),
                    None => qsg::layer::draw_projected(c, &tex, xf.m, op),
                })
            }
            _ => {}
        }
    }
}

/// QQuickItem `rotation`/`scale` about `transformOrigin`, see `paint_turned`; the QtRendering
/// texts under the topmost such item are drawn outside its layer, each through its node's
/// matrix (`df_later`).
fn paint_transformed(
    w: &mut World,
    fonts: &mut Fonts,
    i: usize,
    cv: &mut Target,
    x: f64,
    y: f64,
    opacity: f32,
) -> bool {
    let whole = match cv {
        Target::Gpu(fr) => fr.screen_scale().is_some(),
        Target::Cpu(c) => cpu_scaled(c, |_| ()).is_none(),
    };
    let topmost = whole
        && SOURCES.with(|s| s.borrow().is_empty())
        && DF_LATER.with(|l| l.borrow().is_none())
        && DF_TURNED.with(|t| {
            t.borrow()
                .get(&i)
                .is_some_and(|p| (p[0] - x).abs() < 1e-6 && (p[1] - y).abs() < 1e-6)
        });
    if !topmost {
        return paint_turned(w, fonts, i, cv, x, y, opacity);
    }
    let later = DfLater {
        base: target_id(cv),
        xf: match cv {
            Target::Gpu(fr) => fr.xf,
            Target::Cpu(_) => None,
        },
        layers: Vec::new(),
    };
    DF_LATER.with(|l| *l.borrow_mut() = Some(later));
    let turned = paint_turned(w, fonts, i, cv, x, y, opacity);
    DF_LATER.with(|l| l.borrow_mut().take());
    turned
}

/// Whether every node of `i`'s subtree is drawn through a matrix (`NODE_XF`): Rectangle's
/// geometry, and items with no node of their own. A clip is a scissor and a layer is a
/// texture, upright ones; those and every other node go through a texture of the subtree.
fn takes_matrix(w: &mut World, i: usize) -> bool {
    let plain = w.items[i]
        .types
        .last()
        .is_some_and(|t| matches!(t.as_str(), "Item" | "Rectangle" | "MouseArea" | "Repeater"));
    if !plain || truthy(w, i, "clip", false) || truthy(w, i, "layer.enabled", false) {
        return false;
    }
    visual_kids(w, i).into_iter().all(|c| {
        !visible(w, c)
            || w.item_transform(c).is_none_or(|t| t.affine().is_some()) && takes_matrix(w, c)
    })
}

/// QQuickItem's transform (the `transform` list, `scale` and `rotation`), as a
/// QSGTransformNode over the item's subtree. A move paints the subtree where it lands, and
/// on the GPU so does an upright scale, through the frame's scale; anything else renders the
/// subtree untransformed into a texture and composites it through the transform. Returns
/// false when untransformed.
fn paint_turned(
    w: &mut World,
    fonts: &mut Fonts,
    i: usize,
    cv: &mut Target,
    x: f64,
    y: f64,
    opacity: f32,
) -> bool {
    let Some(t) = w.node_transform(i) else {
        return false;
    };
    // No inverse (a zero scale, or a NaN one from a binding over an undefined drag distance):
    // nothing shows.
    if t.inverse().is_none() {
        return true;
    }
    let (ix, iy) = (num(w, i, "x"), num(w, i, "y"));
    // A turned or scaled childless QtRendering Text (MaterialSymbol in OsdValueIndicator): Qt
    // draws its glyph quads through the item's matrix, subpixel antialiasing along the
    // screen's x, and so do we, with no offscreen layer.
    let whole = match cv {
        Target::Gpu(fr) => fr.screen_scale().is_some(),
        Target::Cpu(c) => cpu_scaled(c, |_| ()).is_none(),
    };
    let node = DF_ITEMS.with(|m| m.borrow().get(&i).copied());
    let now = DF_NOW.with(|n| n.get()) == Some(i);
    if whole
        && node.is_some_and(|d| {
            (d.plain || now) && (d.pos[0] - x).abs() < 1e-6 && (d.pos[1] - y).abs() < 1e-6
        })
        && visual_kids(w, i).is_empty()
    {
        let prev = XFORM_SKIP.with(|s| s.replace(Some(i)));
        let on = DL_ON.with(|d| d.replace(false));
        paint(w, fonts, i, cv, x - ix, y - iy, opacity, false);
        XFORM_SKIP.with(|s| s.set(prev));
        DL_ON.with(|d| d.set(on));
        return true;
    }
    if let Some((dx, dy)) = t.as_translation() {
        // The item's node holds a QMatrix4x4: its x and y and the move add up as floats
        // (QQuickWindowPrivate::updateDirtyNode), 40.4 and -45 to -4.599998.
        let by = |p: f64, d: f64| (p as f32 + d as f32) as f64 - p;
        let prev = XFORM_SKIP.with(|s| s.replace(Some(i)));
        paint(
            w,
            fonts,
            i,
            cv,
            x - ix + by(ix, dx),
            y - iy + by(iy, dy),
            opacity,
            false,
        );
        XFORM_SKIP.with(|s| s.set(prev));
        return true;
    }
    // An upright scale: like QSGTransformNode, draw the subtree directly through the
    // transform instead of an offscreen layer (RI_DEBUG=xflayer restores the layer).
    thread_local!(static XF_LAYER: bool = qml::debug::on(qml::debug::Switch::XFLAYER));
    // Any other transform that keeps lines parallel, over nodes that take a matrix: like
    // QSGTransformNode, each node is drawn through the matrix, so its edges are the node's own
    // (sharp unless it antialiases), not a texture's. Inside such a subtree every transform
    // goes this way.
    let outer = NODE_XF.with(|m| m.get());
    if t.affine().is_some()
        && (outer.is_some()
            || t.as_scale().is_none() && !XF_LAYER.with(|v| *v) && takes_matrix(w, i))
    {
        let m = t.then(&Xf::translation(x, y));
        NODE_XF.with(|n| n.set(Some(outer.map_or(m, |o| m.then(&o)))));
        if outer.is_none() {
            let at = [(x - ix) as f32, (y - iy) as f32];
            NODE_TOP.with(|t| t.set(Some((i, at))));
        }
        let prev = XFORM_SKIP.with(|s| s.replace(Some(i)));
        let on = DL_ON.with(|d| d.replace(false));
        paint(w, fonts, i, cv, -ix, -iy, opacity, false);
        XFORM_SKIP.with(|s| s.set(prev));
        DL_ON.with(|d| d.set(on));
        NODE_XF.with(|n| n.set(outer));
        if outer.is_none() {
            NODE_TOP.with(|t| t.set(None));
        }
        return true;
    }
    if let Target::Gpu(fr) = cv
        && let Some([sx, sy, dx, dy]) = t.as_scale()
        && !XF_LAYER.with(|v| *v)
    {
        let [sx0, sy0, ox0, oy0] = fr.xf.unwrap_or([1.0, 1.0, 0.0, 0.0]);
        let (sx0, sy0) = (sx0 as f64, sy0 as f64);
        // A point (x + lx) of the untransformed item shows at x + sx * lx + dx.
        let xf = [
            (sx0 * sx) as f32,
            (sy0 * sy) as f32,
            (sx0 * (x * (1.0 - sx) + dx)) as f32 + ox0,
            (sy0 * (y * (1.0 - sy) + dy)) as f32 + oy0,
        ];
        let prev_xf = fr.xf.replace(xf);
        let prev = XFORM_SKIP.with(|s| s.replace(Some(i)));
        let on = DL_ON.with(|d| d.replace(false));
        paint(
            w,
            fonts,
            i,
            &mut Target::Gpu(fr),
            x - ix,
            y - iy,
            opacity,
            false,
        );
        XFORM_SKIP.with(|s| s.set(prev));
        DL_ON.with(|d| d.set(on));
        fr.xf = prev_xf;
        return true;
    }
    let (wd, ht) = (num(w, i, "width").max(0.0), num(w, i, "height").max(0.0));
    let mut e = [0.0, 0.0, wd, ht];
    subtree_extent(w, i, 0.0, 0.0, None, &mut e);
    let (e0, e1) = ((e[0] - 2.0).floor(), (e[1] - 2.0).floor());
    let (tw, th) = (
        ((e[2] + 2.0).ceil() - e0).max(1.0),
        ((e[3] + 2.0).ceil() - e1).max(1.0),
    );
    let area = tw * th;
    if area.is_nan() || area > 64e6 {
        return true;
    }
    let (tw, th) = (tw as usize, th as usize);
    // texture px (u, v) -> item (u + e0, v + e1) -> through the transform -> placed at (x, y)
    let m = Xf::translation(e0, e1)
        .then(&t)
        .then(&Xf::translation(x, y));
    let (px, py) = (-ix - e0, -iy - e1);
    if let Target::Gpu(fr) = cv {
        let prev = XFORM_SKIP.with(|s| s.replace(Some(i)));
        let on = DL_ON.with(|d| d.replace(false));
        let [sx, sy, ox, oy] = fr.xform();
        let (dx, dy) = m.map(0.0, 0.0);
        let quarter = quarter_turn(&m, [sx, sy], [sx * dx + ox, sy * dy + oy], fr.against);
        let (mut sub, m) = match quarter {
            Some(q) => {
                let scene = |k: usize, n: usize| (n as f64 + 1.0 / q.s[k]).ceil() as usize;
                let mut sub = crate::gpu::Frame::offscreen(scene(0, tw), scene(1, th), fr.dpr);
                let texels = |k: usize, n: usize| (n as f64 * q.s[k]).ceil() as usize + 1;
                (sub.w, sub.h) = (texels(0, tw), texels(1, th));
                let xf = [q.s[0], q.s[1], q.f[0], q.f[1]].map(|v| v as f32);
                sub.xf = (xf != [1.0, 1.0, 0.0, 0.0]).then_some(xf);
                sub.against = q.against;
                // `Frame::layer` takes the matrix in the frame's scene units, of a texture
                // `dpr` texels to the unit.
                let d = sub.dpr;
                let m = Xf {
                    m: [
                        [q.m[0] * d / sx, q.m[1] * d / sy, 0.0],
                        [q.m[2] * d / sx, q.m[3] * d / sy, 0.0],
                        [(q.m[4] - ox) / sx, (q.m[5] - oy) / sy, 1.0],
                    ],
                };
                (sub, m)
            }
            // Render the subtree offscreen on the GPU and composite it through the transform.
            None => (crate::gpu::Frame::offscreen(tw, th, fr.dpr), m),
        };
        let cut = df_open(&raw const sub as usize, &raw const **fr as usize);
        paint(w, fonts, i, &mut Target::Gpu(&mut sub), px, py, 1.0, false);
        XFORM_SKIP.with(|s| s.set(prev));
        DL_ON.with(|d| d.set(on));
        let mut pieces = if cut { df_close() } else { Vec::new() };
        if !pieces.is_empty() {
            let rest = sub.take_recorded().map(|f| DfContent::Gpu(Box::new(f)));
            pieces.extend(rest.map(|c| DfPiece::Layer(c, Xf::translation(0.0, 0.0), 1.0)));
            df_place(w, fonts, cv, pieces, &m, opacity);
            return true;
        }
        match m.affine() {
            Some(a) => fr.layer(sub, a, opacity),
            None => fr.layer_projected(sub, m.m, opacity),
        }
        return true;
    }
    // In a layer of a size that is not whole the subtree is drawn at the layer's scale too.
    let on = target_id(cv);
    let Target::Cpu(c) = &mut *cv else {
        return true;
    };
    let scaled = cpu_scaled(c, |l| (l.s, cpu_against(&l.out)));
    let (ts, outer) = scaled.unwrap_or_else(|| ([1.0, 1.0], cpu_against(c)));
    let (dx, dy) = m.map(0.0, 0.0);
    // The subtree's texture and the texels of it to those of the target.
    let (tex, pieces, mm) = if let Some(q) = quarter_turn(&m, ts, [ts[0] * dx, ts[1] * dy], outer) {
        let prev = XFORM_SKIP.with(|s| s.replace(Some(i)));
        // Qt draws a scaled subtree of a window into the window itself: its rows are a
        // window's (`Canvas::rows`).
        let window = c.is_window() && t.as_scale().is_some();
        let (tex, pieces) = cpu_subtree(
            w,
            fonts,
            i,
            [px + q.f[0] / q.s[0], py + q.f[1] / q.s[1]],
            [tw as f64 + 1.0 / q.s[0], th as f64 + 1.0 / q.s[1]],
            q.s,
            window,
            q.against,
            on,
        );
        XFORM_SKIP.with(|s| s.set(prev));
        let k = q.m;
        let mm = Xf {
            m: [[k[0], k[1], 0.0], [k[2], k[3], 0.0], [k[4], k[5], 1.0]],
        };
        (tex, pieces, mm)
    } else {
        let prev = XFORM_SKIP.with(|s| s.replace(Some(i)));
        let size = [tw as f64, th as f64];
        let (tex, pieces) = cpu_subtree(w, fonts, i, [px, py], size, ts, false, [false; 2], on);
        XFORM_SKIP.with(|s| s.set(prev));
        // texture px -> the subtree's px -> through the transform -> the canvas's px
        let mut mm = m;
        for (row, s) in mm.m.iter_mut().zip([ts[0], ts[1], 1.0]) {
            *row = [ts[0] * (row[0] / s), ts[1] * (row[1] / s), row[2] / s];
        }
        (tex, pieces, mm)
    };
    let mut tex = tex;
    let mut pieces = pieces;
    if !pieces.is_empty() {
        let rest = take_content(&mut Target::Cpu(&mut tex));
        pieces.extend(rest.map(|c| DfPiece::Layer(c, Xf::translation(0.0, 0.0), 1.0)));
        df_place(w, fonts, cv, pieces, &mm, opacity);
        return true;
    }
    let Target::Cpu(c) = cv else { return true };
    cpu_direct(c, |c, _| match mm.affine() {
        Some(a) => qsg::layer::draw_transformed(c, &tex, a, opacity),
        None => qsg::layer::draw_projected(c, &tex, mm.m, opacity),
    });
    true
}

/// Bytes of decoded images no item draws any more kept for reuse; images shown (shared with
/// a recorded frame) are pinned on top of that.
const IMAGES_BYTES: usize = 32 << 20;

fn image_held(t: &Option<Rc<Canvas>>) -> bool {
    t.as_ref().is_some_and(canvas_held)
}

/// Cache weight of a decoded image (or remembered failure) and its key.
/// A GPU-resident image counts its texture: the budget bounds the VRAM it keeps too.
fn image_bytes(key: &ImageKey, t: &Option<Rc<Canvas>>) -> usize {
    t.as_ref().map_or(0, |c| c.w * c.h * 4) + key.0.len() + 64
}

thread_local! {
    static IMAGES: std::cell::RefCell<ByteLru<ImageKey, Option<std::rc::Rc<Canvas>>>> =
        std::cell::RefCell::new(
            ByteLru::new(IMAGES_BYTES).pinned(image_held).on_removed(image_evicted),
        );
}

fn cache_image(key: ImageKey, t: Option<Rc<Canvas>>) {
    let n = image_bytes(&key, &t);
    image_cached(&key, t.is_some());
    IMAGES.with(|c| c.borrow_mut().insert(key, t, n));
}

/// Image `source` URL, requested `sourceSize` and fillMode as the renderer loads them, and the
/// Image's own devicePixelRatio: its implicit size is the loaded pixmap's over it.
fn image_src(
    w: &mut World,
    i: usize,
    wd: f64,
    ht: f64,
    dpr: f64,
) -> (String, (i32, i32), i32, f64) {
    let icon = is_icon(w, i);
    let mut src = w
        .read(i, "source")
        .map(|v| v.to_js_string())
        .unwrap_or_default();
    // Kirigami Icon::findIcon: `QUrl(source).isLocalFile()` is the file `toLocalFile()` names.
    if icon {
        let url = qml::qurl::QUrl::parse(&src);
        if url.is_local_file() {
            src = url.to_local_file();
        }
    }
    // Kirigami Icon::iconSizeHint: with roundToIconSize a QIcon (a theme name or a file) is
    // asked for the square of the standard icon size the item's shorter side rounds down to.
    // The pixmap an icon has is the one its last polish loaded.
    let qicon = icon && (!src.contains("://") || src.starts_with("file://"));
    let round = |w: &mut World| {
        let polished = image_loads(w).icons.borrow().get(&i).and_then(|s| s.round);
        polished.unwrap_or_else(|| truthy(w, i, "roundToIconSize", true))
    };
    let (wd, ht) = match qicon && round(w) {
        true => {
            let side = rounded_icon_size(wd.min(ht) as i32) as f64;
            (side, side)
        }
        false => (wd, ht),
    };
    // The fill and size of a theme icon that QIconLoaderEngine answers.
    let mut loader = None;
    // Whether the icon's own source gave nothing, whatever its fallback gives.
    let mut missed = false;
    // Kirigami.Icon: a missing file or theme name tries `fallback`; nothing draws when both miss.
    if icon && !src.is_empty() && !src.contains("://") {
        let fallback = w
            .read(i, "fallback")
            .map(|v| v.to_js_string())
            .filter(|f| !f.is_empty() && f != "undefined");
        // Kirigami Icon::updatePolish: QIcon::pixmap at Icon::iconSizeHint.
        let sz = wd.min(ht) as i32;
        let kde_plugin = matches!(
            qml::colorscheme::platform_theme(),
            qml::colorscheme::PlatformTheme::KdePlugin
        );
        // QIcon::fromTheme: an absolute path is a plain QIcon, a theme name is the platform
        // theme's icon engine, marked by the native `image://kicon/`: plasma-integration's
        // KIconEngine (recolored), else Qt's own QIconLoaderEngine.
        let mut find = |n: &str| {
            if n.starts_with('/') {
                return qml::icons::lookup(n, sz);
            }
            if kde_plugin {
                // QIcon::actualSize: KIconEngine has the default actualSize, the size asked
                // for. Then KIconEngine::scaledPixmap of that size at the window's ratio.
                let item = (wd as i32, ht as i32);
                let item = qsg::image::qicon_actual(app_dpr(dpr), item.0, item.1, |w, h| (w, h));
                if let Some(f) = qml::icons::lookup_kde(n, sz, (dpr * 120.0).round() as i32) {
                    loader = Some((kicon_fill(dpr), item));
                    return Some(format!("image://kicon/{f}"));
                }
                // QIconLoader::iconEngine: a name KIconLoader does not have is asked of
                // Qt's own QIconLoaderEngine.
            }
            // QIcon::actualSize: QIconLoaderEngine::actualSize of the entry for the item's
            // size (times the application's ratio, and divided back); then scaledPixmap:
            // the entry for that size at the window's ratio rounded up gives a pixmap of
            // that size times the ratio. A file outside the theme directories is a QIcon.
            let item = (wd as i32, ht as i32);
            let mut f = None;
            let actual = qsg::image::qicon_actual(app_dpr(dpr), item.0, item.1, |rw, rh| {
                f = qml::icons::lookup_loader(n, rw.min(rh), 1);
                let a = f.as_ref().and_then(|f| qml::icons::loader_size(f, rw, rh));
                a.unwrap_or((0, 0))
            });
            let f = f?;
            let (fill, ss, f) = match qml::icons::loader_size(&f, item.0, item.1) {
                Some(_) => {
                    let side = actual.0.min(actual.1);
                    let again = qml::icons::lookup_loader(n, side, dpr.ceil() as i32);
                    let px = qsg::image::qsize_times(actual.0, actual.1, dpr);
                    (LOADER_FILL, px, again.unwrap_or(f))
                }
                None => (qicon_fill(dpr, app_dpr(dpr), true), item, f),
            };
            loader = Some((fill, ss));
            Some(format!("image://kicon/{f}"))
        };
        let found = find(&src);
        missed = found.is_none();
        src = found
            .or_else(|| fallback.and_then(|f| find(&f)))
            .unwrap_or_default();
    }
    if icon {
        let mut icons = image_loads(w).icons.borrow_mut();
        icons.entry(i).or_default().missed = missed;
    }
    // QQuickImageBase::loadPixmap: `qmlContext(this)->resolvedUrl(source)`.
    if !icon {
        src = w.resolved_url(i, &src);
    }
    if !icon && provider_source(&src) {
        let (ss, ratio) = provider_request(w, i, &src);
        return (src, ss, PROVIDER_FILL, ratio);
    }
    // A file is a plain QIcon; what a provider answers is fitted like an Image.
    let fk = if let Some((fill, _)) = loader {
        fill
    } else if icon && !src.contains("://") {
        qicon_fill(dpr, app_dpr(dpr), false)
    } else if icon {
        1
    } else {
        num(w, i, "fillMode") as i32
    };
    // Kirigami Icon::iconSizeHint: QSize(width(), height()) truncates.
    // QSizeF -> QSize rounds, as when a real binding feeds Image.sourceSize.
    let ss = if let Some((_, ss)) = loader {
        ss
    } else if icon {
        (wd as i32, ht as i32)
    } else {
        let ss = w
            .source_size_set(i)
            .map(|v| v.unwrap_or(0.0).round() as i32);
        (ss[0], ss[1])
    };
    (src, ss, fk, 1.0)
}

type ImageKey = (String, (i32, i32), i32);
type FileTime = Option<std::time::SystemTime>;

enum Loaded {
    /// An asynchronous (`Image.asynchronous`) local decode, with the file's modification time
    /// as the worker found it before reading the file.
    Decoded(ImageKey, Option<Canvas>, FileTime),
    /// A remote source fetched into the cache dir.
    Downloaded(String),
    /// An AnimatedImage's (item) source opened as a movie, with its first frame; `None` when
    /// it is not animated.
    Movie(usize, ImageKey, Option<Opened>),
    /// A Markdown inline image (path, decoded file).
    Inline(String, Option<Canvas>),
    /// The load of this picture is done: one of its own ([`picture_texture`]), or one that
    /// ended before the picture was laid out.
    Own(usize),
}

type Opened = (crate::movie::Movie, Canvas);

/// One AnimatedImage's movie and the frame it shows: the only frame kept.
struct Showing {
    movie: crate::movie::Movie,
    at: usize,
    frame: Rc<Canvas>,
    /// `Movie::raw` of `frame`.
    raw: bool,
}

impl Showing {
    fn new((movie, first): Opened) -> Self {
        Showing {
            raw: movie.raw(),
            movie,
            at: 0,
            frame: Rc::new(first),
        }
    }
}

/// QQuickImageBasePrivate::currentPix of an Image: the pixmap its last finished load gave it.
struct Current {
    key: ImageKey,
    /// That pixmap, held while a `retainWhileLoading` Image loads the next one
    /// (`pendingPix`): the Image draws it until the load ends, whatever the cache keeps.
    held: Option<Rc<Canvas>>,
}

/// The load of Image `i` ended with the pixmap of `key`, or with none.
fn current_is(loads: &ImageLoads, i: usize, key: Option<ImageKey>) {
    let mut current = loads.current.borrow_mut();
    match key {
        Some(key) => drop(current.insert(i, Current { key, held: None })),
        None => drop(current.remove(&i)),
    }
}

/// Image `i` is Loading: with `retainWhileLoading` it holds the pixmap it shows, without it
/// that pixmap is the one being loaded.
fn current_retained(loads: &ImageLoads, i: usize, retain: bool) {
    let mut current = loads.current.borrow_mut();
    if !retain {
        current.remove(&i);
    } else if let Some(c) = current.get_mut(&i).filter(|c| c.held.is_none()) {
        c.held = IMAGES.with_borrow(|m| m.peek(&c.key).cloned().flatten());
    }
}

/// The pixmap Image `i` holds while it is Loading, with pixels where `cpu` draws it.
fn retained_texture(loads: &ImageLoads, i: usize, cpu: bool) -> Option<Rc<Canvas>> {
    let (key, held) = {
        let current = loads.current.borrow();
        let c = current.get(&i)?;
        (c.key.clone(), c.held.clone()?)
    };
    if !cpu || held.px.len() >= held.w * held.h {
        return Some(held);
    }
    let (src, ss, fill) = key;
    let held = cpu_pixels(loads, &src, ss, fill, Some(held))?;
    if let Some(c) = loads.current.borrow_mut().get_mut(&i) {
        c.held = Some(held.clone());
    }
    Some(held)
}

/// One world's image work in flight on worker threads, which report on its own channel.
pub struct ImageLoads {
    tx: qml::svc::Sender<Loaded>,
    rx: std::sync::mpsc::Receiver<Loaded>,
    decoding: std::cell::RefCell<std::collections::HashSet<String>>,
    /// The still pictures among `decoding`, by what is asked of the file: a synchronous Image
    /// waits only for the very pixmap being loaded (QQuickPixmap::load shares the cache
    /// entry), and loads another size of the same file itself.
    decoding_keys: std::cell::RefCell<std::collections::HashSet<ImageKey>>,
    downloading: std::cell::RefCell<std::collections::HashSet<String>>,
    /// Remote sources whose last fetch failed: they read as Error and are not fetched again
    /// (each layout would start another download) until an Image asks anew
    /// ([`remote_request`]).
    fetch_failed: std::cell::RefCell<std::collections::HashSet<String>>,
    decoded: std::cell::RefCell<Vec<(ImageKey, Option<Canvas>, FileTime)>>,
    opened: std::cell::RefCell<Vec<(usize, ImageKey, Option<Opened>)>>,
    /// The source each AnimatedImage plays; `None` for one that is not animated.
    movies: std::cell::RefCell<HashMap<usize, (ImageKey, Option<Showing>)>>,
    downloaded: std::cell::RefCell<Vec<String>>,
    inline: std::cell::RefCell<Vec<(String, Option<Canvas>)>>,
    /// The decode that landed for a picture (item) and that no frame has drawn yet: its share
    /// keeps the image cached ([`image_held`]) however big it is. QQuickImage holds its
    /// pixmap itself; here the cache alone would, and an image bigger than what the cache
    /// keeps unheld was evicted before its picture had its answer, which decoded it again,
    /// for ever.
    kept: std::cell::RefCell<Vec<(usize, ImageKey, Rc<Canvas>)>>,
    /// Per Image, the pixmap it shows ([`Current`]).
    current: std::cell::RefCell<HashMap<usize, Current>>,
    own: std::cell::RefCell<Vec<usize>>,
    /// Each window's scale, by its item: its output's until the compositor gives it one. With
    /// it the number of items there were when the window was made.
    window_scales: std::cell::RefCell<HashMap<usize, (f64, usize)>>,
    /// What each Kirigami.Icon keeps between polishes ([`polish_icons`]).
    icons: std::cell::RefCell<HashMap<usize, IconState>>,
    /// The Kirigami.Icons that asked to be polished and were not yet ([`icon_asks`]).
    asked: std::cell::RefCell<IconAsks>,
    /// Each icon provider Image's own devicePixelRatio.
    ratios: std::cell::RefCell<HashMap<usize, ProviderRatio>>,
    /// Set when the world goes away: its downloads in flight stop.
    stop: qml::runtime::StopOnDrop,
}

impl ImageLoads {
    fn drain(&self) {
        for m in self.rx.try_iter() {
            match m {
                Loaded::Decoded(k, t, m) => self.decoded.borrow_mut().push((k, t, m)),
                Loaded::Downloaded(s) => self.downloaded.borrow_mut().push(s),
                Loaded::Movie(i, k, m) => self.opened.borrow_mut().push((i, k, m)),
                Loaded::Inline(p, t) => self.inline.borrow_mut().push((p, t)),
                Loaded::Own(i) => self.own.borrow_mut().push(i),
            }
        }
    }
}

/// The world's image loads, created with its host wakeup on first use.
pub fn image_loads(w: &World) -> &ImageLoads {
    w.host
        .get_or_init(|| {
            let (tx, rx) = w.waker.handle(qml::svc::Svc::Host).channel();
            Box::new(ImageLoads {
                tx,
                rx,
                decoding: Default::default(),
                decoding_keys: Default::default(),
                downloading: Default::default(),
                fetch_failed: Default::default(),
                decoded: Default::default(),
                opened: Default::default(),
                movies: Default::default(),
                kept: Default::default(),
                current: Default::default(),
                downloaded: Default::default(),
                inline: Default::default(),
                own: Default::default(),
                window_scales: Default::default(),
                icons: Default::default(),
                asked: Default::default(),
                ratios: Default::default(),
                stop: Default::default(),
            })
        })
        .downcast_ref()
        .expect("World::host holds the shell's ImageLoads")
}

type DecodeJob = Box<dyn FnOnce() + Send>;

const DECODE_WORKERS: usize = 2;

/// QQuickPixmapReader: a small fixed pool, so a long list of big images neither blocks the
/// frame loop nor fans out into one thread (and one full-size buffer) per image. A job that
/// panics (a decoder bug on a malformed file) costs only its own image: the worker survives.
fn decode_pool() -> &'static std::sync::mpsc::Sender<DecodeJob> {
    static POOL: std::sync::OnceLock<std::sync::mpsc::Sender<DecodeJob>> =
        std::sync::OnceLock::new();
    POOL.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<DecodeJob>();
        let rx = std::sync::Arc::new(std::sync::Mutex::new(rx));
        for n in 0..DECODE_WORKERS {
            let rx = rx.clone();
            let _ = std::thread::Builder::new()
                .name(format!("ri-decode{n}"))
                .spawn(move || {
                    loop {
                        let job = rx.lock().unwrap_or_else(|e| e.into_inner()).recv();
                        let Ok(f) = job else { break };
                        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).is_err() {
                            tracing::warn!("image decode job panicked");
                        }
                    }
                });
        }
        tx
    })
}

fn submit_decode(job: DecodeJob) {
    let _ = decode_pool().send(job);
}

/// `work` on a decode worker, a panic in it reported as a failed decode (`None`): the job
/// still reports, so the image leaves Loading instead of waiting forever.
fn guarded<T>(work: impl FnOnce() -> Option<T>) -> Option<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(work))
        .ok()
        .flatten()
}

/// plasma-integration's KIconEngine recolors every SVG theme icon (`image://icon/`, a
/// Kirigami.Icon theme name) while the user's icon theme follows the color scheme; a file from
/// an SNI `?path=` directory is loaded as a plain QIcon (no recoloring).
fn icon_scheme(src: &str, f: &str) -> Option<std::sync::Arc<str>> {
    let rest = src
        .strip_prefix("image://icon/")
        .or_else(|| src.strip_prefix("image://kicon/"))?;
    // KIconLoader recolors what it loads as SVG: a file named `.svg` or `.svgz`.
    if !(f.ends_with(".svg") || f.ends_with(".svgz")) {
        return None;
    }
    if let Some((_, p)) = rest.split_once("?path=")
        && f.starts_with(p.split("?fallback=").next().unwrap_or(p))
    {
        return None;
    }
    qml::icons::theme_follows_color_scheme()
        .then(qml::colorscheme::kde_icon_stylesheet)
        .flatten()
}

/// KIconEngine::createPixmap for the file `f` (its bytes are `data`) asked for `w`x`h` at a
/// devicePixelRatio of `scale`: a pixmap of that size times the ratio, each side rounded.
/// KIconLoaderPrivate::createIconImage reads the file at its own size fitted into that one
/// with its aspect kept (QImageReader::setScaledSize: a raster file is enlarged as well, an
/// SVG is drawn at that size). A file of another shape than the request does not fill the
/// pixmap: what was read is drawn again into the middle of a clear one, with
/// SmoothPixmapTransform, over its size in pixels fitted into the size asked for in units.
fn kicon_pixmap(
    f: &str,
    data: &[u8],
    (w, h): (i32, i32),
    scale: f64,
    scheme: Option<&str>,
) -> Option<Canvas> {
    if w <= 0 || h <= 0 {
        return None;
    }
    let wanted = qsg::image::qsize_times(w, h, scale);
    let size = |ow: i32, oh: i32| {
        let (fw, fh) = match (ow, oh) {
            (0, 0) => wanted,
            _ => qsg::image::qsize_fit(ow, oh, wanted.0, wanted.1),
        };
        (fw.max(0) as usize, fh.max(0) as usize)
    };
    let svg = |data: &[u8]| {
        let doc = qsg::svg::Doc::from_data(data, scheme).ok()?;
        let (dw, dh) = doc.default_size();
        let (fw, fh) = size(dw, dh);
        (fw > 0 && fh > 0)
            .then(|| doc.render(fw, fh).ok())
            .flatten()
    };
    let named_svg = std::path::Path::new(f)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("svg") || e.eq_ignore_ascii_case("svgz"));
    let pix = if qsg::svg::is_svg(data) && (named_svg || !qsg::svg::is_gzip(data)) {
        svg(data)
    } else {
        match qsg::image::decode_scaled(data, |ow, oh| size(ow as i32, oh as i32)) {
            Ok((t, _)) => Some(t),
            Err(_) if named_svg => svg(data),
            Err(_) => None,
        }
    };
    let (cw, ch) = (wanted.0.max(0) as usize, wanted.1.max(0) as usize);
    let pix = match pix {
        Some(p) if (p.w, p.h) == (cw, ch) => return Some(p),
        Some(p) => Rc::new(p),
        None => return Some(Canvas::new(cw, ch, [0, 0, 0, 0])),
    };
    let mut p = qsg::painter::Painter::new(cw, ch);
    p.set_matrix([scale, 0.0, 0.0, scale, 0.0, 0.0]);
    let (tw, th) = qsg::image::qsize_fit(pix.w as i32, pix.h as i32, w, h);
    let (tw, th) = (tw as f64, th as f64);
    let (cx, cy) = (cw as f64 / 2.0 / scale, ch as f64 / 2.0 / scale);
    p.draw_image_rect(&pix, [cx - tw / 2.0, cy - th / 2.0, tw, th]);
    Some(p.img)
}

/// A file decoded by what it holds, as Qt Quick's Image reads it (measured on Qt 6.12, see
/// `image_files_decode_by_content`): SVG under any name (`.SVG`, no extension, a wrong one),
/// gzip-compressed SVG only when it is named `.svg` or `.svgz` in any case, and every raster
/// format, XPM included, under any name.
fn decode_file(f: &str, ss: (i32, i32), fill: i32, scheme: Option<&str>) -> Option<Canvas> {
    let fm = match fill {
        1 => qsg::image::FillMode::PreserveAspectFit,
        2 => qsg::image::FillMode::PreserveAspectCrop,
        _ => qsg::image::FillMode::Stretch,
    };
    let data = std::fs::read(f).ok()?;
    if let Some(scale) = kicon_ratio(fill) {
        return kicon_pixmap(f, &data, ss, scale, scheme);
    }
    let svg = |data: &[u8]| {
        let doc = qsg::svg::Doc::from_data(data, scheme).ok()?;
        let (dw, dh) = doc.default_size();
        if let Some((lw, lh)) = icon_pixmap_size(fill, dw, dh, ss, true) {
            // QSvgIconEngine::pixmap: the document stretched over the size it answers.
            return (lw > 0 && lh > 0)
                .then(|| doc.render(lw as usize, lh as usize).ok())
                .flatten();
        }
        let (lw, lh) = if ss.0 > 0 || ss.1 > 0 {
            qsg::svg::load_size(dw, dh, ss.0, ss.1, fm)
        } else {
            (dw as usize, dh as usize)
        };
        doc.render(lw.max(1), lh.max(1)).ok()
    };
    let named_svg = std::path::Path::new(f)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("svg") || e.eq_ignore_ascii_case("svgz"));
    if qsg::svg::is_svg(&data) && (named_svg || !qsg::svg::is_gzip(&data)) {
        return svg(&data);
    }
    let sized = |w: usize, h: usize| match ss.0 > 0 || ss.1 > 0 {
        true => qsg::image::load_size(w, h, ss.0, ss.1, fm),
        false => (w, h),
    };
    // A GIF is read as QGifHandler reads it: what its first image does not cover and the
    // alpha bytes of a picture with no alpha channel are that reader's.
    // Only a GIF is opened again for it: any other file is read once, as QImageReader does.
    let icon = fill <= LOADER_FILL;
    let gif = data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a");
    if !icon
        && gif
        && let Some((tex, raw)) = crate::movie::gif_first(f, sized)
    {
        return Some(if raw { tex.without_alpha() } else { tex });
    }
    let decoded = match icon {
        true => qsg::image::decode_ex(&data),
        false => qsg::image::decode_scaled(&data, sized),
    };
    let (mut tex, alpha) = match decoded {
        Ok(t) => t,
        // A `.svg` whose header starts past the bytes Qt sniffs still goes to the SVG reader.
        Err(_) if named_svg => return svg(&data),
        Err(_) => return None,
    };
    if let Some((lw, lh)) = icon_pixmap_size(fill, tex.w as i32, tex.h as i32, ss, false) {
        // QPixmapIconEngine::pixmap and QIconLoader's PixmapEntry::pixmap:
        // QPixmap::scaled(IgnoreAspectRatio, SmoothTransformation).
        if lw <= 0 || lh <= 0 {
            return None;
        }
        if (lw as usize, lh as usize) != (tex.w, tex.h) {
            tex = qsg::image::smooth_scale(&tex, lw as usize, lh as usize, alpha);
        }
        return Some(tex);
    }
    Some(tex)
}

/// `file` as a movie with its first frame; `None` for what QMovie does not animate.
fn open_movie(file: &str, ss: (i32, i32)) -> Option<Opened> {
    let mut movie = crate::movie::Movie::open(file, ss)?;
    let first = movie.frame(0)?;
    Some((movie, first))
}

/// QQuickAnimatedImage: the frame `currentFrame` of an animated source, publishing
/// `frameCount`, the frame delays and the rounds of the file, by which the runtime plays it
/// (`qml::animated`: which frame comes when, `playing`, `loops`, `finished`). `None` when `i`
/// is not an AnimatedImage or its source is not animated (it then loads as a static Image).
///
/// The frame is read when `currentFrame` asks for it and only that one is kept, as QMovie
/// does with `cache: false`. Meant difference from Qt: `cache: true` (the default) keeps no
/// shown frames either, which costs Qt 61 MB a frame of a 4000x4000 GIF; the picture is the
/// same, a step back reads the file again from its start (docs/qt-differences.md).
fn animated_texture(
    w: &mut World,
    i: usize,
    src: &str,
    ss: (i32, i32),
    asynchronous: bool,
) -> Option<Option<Rc<Canvas>>> {
    if !is(w, i, "AnimatedImage") {
        return None;
    }
    let shown = movie_frame(w, i, src, ss, asynchronous);
    // No movie: the runtime stops playing the one before.
    if !matches!(shown, Some(Some(_))) && w.read(i, "\x01frameSrc").is_some_and(|v| v.truthy()) {
        w.write(i, "\x01frameSrc", Value::Str("".into()));
    }
    shown
}

fn movie_frame(
    w: &mut World,
    i: usize,
    src: &str,
    ss: (i32, i32),
    asynchronous: bool,
) -> Option<Option<Rc<Canvas>>> {
    if src.is_empty() {
        return None;
    }
    let loads = image_loads(w);
    // QMovie::setScaledSize takes `sourceSize` as it is, whatever the fill mode.
    let key = (src.to_string(), ss, 0);
    if loads.movies.borrow().get(&i).is_none_or(|m| m.0 != key) {
        if decode_pending(loads, src) {
            return Some(None);
        }
        let file = image_file(loads, src, ss)?;
        if asynchronous {
            loads.decoding.borrow_mut().insert(src.to_string());
            let tx = loads.tx.clone();
            let job: DecodeJob = Box::new(move || {
                let movie = guarded(|| open_movie(&file, key.1));
                let _ = tx.send(Loaded::Movie(i, key, movie));
            });
            submit_decode(job);
            return Some(None);
        }
        let movie = open_movie(&file, ss).map(Showing::new);
        loads.movies.borrow_mut().insert(i, (key, movie));
    }
    let (n, rounds, delays, late) = {
        let movies = loads.movies.borrow();
        let show = movies.get(&i)?.1.as_ref()?;
        (
            show.movie.frame_count(),
            show.movie.loops(),
            show.movie.delays().to_vec(),
            show.movie.counts_late(),
        )
    };
    // Published once per source, the name last: the runtime starts the movie when it sees it.
    let tag = Value::Str(format!("{src}\x01{n}").into());
    if w.read(i, "\x01frameSrc").as_ref().map(|v| v.to_js_string()) != Some(tag.to_js_string()) {
        let delays = delays.into_iter().map(Value::Num).collect();
        w.write(i, "\x01frameDelays", Value::array(delays));
        w.write(i, "\x01frameLoops", Value::Num(f64::from(rounds)));
        w.write(i, "\x01frameLate", Value::Bool(late));
        w.write(i, "frameCount", Value::Num(n as f64));
        w.write(i, "\x01frameSrc", tag);
    }
    let asked = num(w, i, "currentFrame");
    let loads = image_loads(w);
    let mut movies = loads.movies.borrow_mut();
    let show = movies.get_mut(&i)?.1.as_mut()?;
    // A frame the file does not have leaves the one shown.
    if asked >= 0.0
        && (asked as usize) < n
        && asked as usize != show.at
        && let Some(frame) = show.movie.frame(asked as usize)
    {
        show.at = asked as usize;
        show.frame = Rc::new(frame);
        show.raw = show.movie.raw();
    }
    Some(Some(show.frame.clone()))
}

/// Whether the frame AnimatedImage `i` shows is drawn with blending off at this opacity, as
/// Qt Quick draws a picture with no alpha channel while nothing makes it translucent
/// (QSGGeometryNode::activeMaterial: the opaque material above an inherited opacity of
/// 0.999). It is told from a blended one only where the frame's alpha bytes are not 255
/// (`Movie::raw`).
///
/// Under a `layer.enabled` with no effect Qt draws the frame so into the layer's texture and
/// blends the texture on the window. Such a layer is painted here straight on the window, so
/// the frame is blended: the same pixels, unless the layer has something under the frame.
fn unblended_frame(w: &mut World, i: usize, tex: &Canvas, op: f32) -> bool {
    let raw = image_loads(w)
        .movies
        .borrow()
        .get(&i)
        .and_then(|m| m.1.as_ref())
        .is_some_and(|show| show.raw)
        || !tex.key.has_alpha();
    if !raw || op <= 0.999 {
        return false;
    }
    let mut at = Some(i);
    while let Some(k) = at {
        let effect = match w.read(k, "layer.effect") {
            Some(Value::Item(e)) => ["OpacityMask", "MultiEffect", "DropShadow", "ColorOverlay"]
                .iter()
                .any(|t| is(w, e, t)),
            _ => false,
        };
        if truthy(w, k, "layer.enabled", false) && !effect {
            return false;
        }
        at = w.items.get(k).and_then(|it| it.parent);
    }
    true
}

/// Queues an asynchronous decode of a local file; `None` (status Loading) until it lands.
fn decode_async(loads: &ImageLoads, key: ImageKey, file: String) {
    if !loads.decoding.borrow_mut().insert(key.0.clone()) {
        return;
    }
    loads.decoding_keys.borrow_mut().insert(key.clone());
    let tx = loads.tx.clone();
    let job: DecodeJob = Box::new(move || {
        let read = file_mtime(&file);
        let tex = guarded(|| decode_file(&file, key.1, key.2, None));
        let _ = tx.send(Loaded::Decoded(key, tex, read));
    });
    submit_decode(job);
}

fn decode_pending(loads: &ImageLoads, src: &str) -> bool {
    loads.decoding.borrow().contains(src)
}

/// Whether an Image asking for `key` waits for a worker: an asynchronous one while the file
/// is being decoded at all, a synchronous one only while this very pixmap is.
fn still_pending(loads: &ImageLoads, key: &ImageKey, asynchronous: bool) -> bool {
    decode_pending(loads, &key.0) && (asynchronous || loads.decoding_keys.borrow().contains(key))
}

/// Remote image sources finished downloading since the last call (the host relayouts).
pub fn take_downloaded(w: &World) -> Vec<String> {
    prune_images();
    let loads = image_loads(w);
    loads.drain();
    std::mem::take(&mut *loads.downloaded.borrow_mut())
}

thread_local! {
    /// Modification time of local files whose decode failed, per image key.
    static FAILED: std::cell::RefCell<HashMap<ImageKey, FileTime>> = Default::default();
}

fn file_mtime(f: &str) -> Option<std::time::SystemTime> {
    std::fs::metadata(f).and_then(|m| m.modified()).ok()
}

/// The file of `key` did not decode. `read` is its modification time taken before the decode
/// read it, so the failure belongs to those bytes only: a file written or replaced after that
/// (the thumbnailer renames its PNG into place at any moment) has another time and is decoded
/// when the Image is laid out next. A source with no file yet is not noted at all.
fn note_failed(key: &ImageKey, read: FileTime) {
    FAILED.with(|m| m.borrow_mut().insert(key.clone(), read));
}

fn failed_unchanged(key: &ImageKey, f: &str) -> bool {
    FAILED.with(|m| m.borrow().get(key).is_some_and(|t| *t == file_mtime(f)))
}

/// Sources whose asynchronous decode landed since the last call, now cached.
pub fn take_decoded(w: &World) -> Vec<String> {
    take_landed(w).into_iter().map(|(k, _)| k.0).collect()
}

/// [`take_decoded`] with each decode's key and image, which the caller's share keeps cached.
fn take_landed(w: &World) -> Vec<(ImageKey, Option<Rc<Canvas>>)> {
    let loads = image_loads(w);
    loads.drain();
    let done = std::mem::take(&mut *loads.decoded.borrow_mut());
    let opened = std::mem::take(&mut *loads.opened.borrow_mut());
    if !done.is_empty() || !opened.is_empty() {
        forget_draws();
    }
    let mut out = Vec::new();
    loads.movies.borrow_mut().retain(|i, _| !w.dead.contains(i));
    loads
        .current
        .borrow_mut()
        .retain(|i, _| !w.dead.contains(i));
    settle_kept(w, loads);
    for (item, key, movie) in opened {
        loads.decoding.borrow_mut().remove(&key.0);
        out.push((key.clone(), None));
        if !w.dead.contains(&item) {
            let movie = movie.map(Showing::new);
            loads.movies.borrow_mut().insert(item, (key, movie));
        }
    }
    for (key, tex, read) in done {
        loads.decoding.borrow_mut().remove(&key.0);
        loads.decoding_keys.borrow_mut().remove(&key);
        // A file that was not there has no failed decode: it loads once it is.
        if tex.is_none()
            && let Some(read) = read
        {
            note_failed(&key, Some(read));
        }
        let tex = tex.map(Rc::new);
        out.push((key.clone(), tex.clone()));
        cache_image(key, tex);
    }
    out
}

/// Drops the shares of [`ImageLoads::kept`] that keep nothing more: the picture is gone, a
/// frame drew the image and holds it now, or the cache let it go.
fn settle_kept(w: &World, loads: &ImageLoads) {
    let mut kept = loads.kept.borrow_mut();
    if kept.is_empty() {
        return;
    }
    kept.retain(|(i, ..)| !w.dead.contains(i));
    // The cache's share, these and those of the Images that retain it: any other is a frame's.
    let current = loads.current.borrow();
    let retained = |t: &Rc<Canvas>| {
        let held = current.values().filter_map(|c| c.held.as_ref());
        held.filter(|h| Rc::ptr_eq(h, t)).count()
    };
    let done: Vec<bool> = kept
        .iter()
        .map(|(_, _, t)| {
            let shares = kept.iter().filter(|o| Rc::ptr_eq(&o.2, t)).count();
            Rc::strong_count(t) != shares + retained(t) + 1
        })
        .collect();
    let mut done = done.into_iter();
    kept.retain(|_| !done.next().unwrap());
}

/// The pictures in `hit` take their share of the decode in `landed` they wait for. One that
/// has not been laid out yet has its answer only after its first layout ([`owe`]): the share
/// keeps the image until then too.
fn keep_landed(w: &mut World, hit: &[usize], landed: Vec<(ImageKey, Option<Rc<Canvas>>)>) {
    if landed.iter().all(|(_, t)| t.is_none()) {
        return;
    }
    for &i in hit {
        if !is_picture(w, i) {
            continue;
        }
        let (wd, ht) = (num(w, i, "width"), num(w, i, "height"));
        let (src, ss, fk, _) = image_src(w, i, wd, ht, 1.0);
        let key = (src, ss, fk);
        let Some(tex) = landed
            .iter()
            .find(|(k, _)| *k == key)
            .and_then(|l| l.1.clone())
        else {
            continue;
        };
        let mut kept = image_loads(w).kept.borrow_mut();
        kept.retain(|o| o.0 != i);
        kept.push((i, key, tex));
    }
}

/// Picture `i` loads `key` now: its share of a decode of anything else is dropped.
fn kept_only(loads: &ImageLoads, i: usize, key: &ImageKey) {
    let mut kept = loads.kept.borrow_mut();
    if !kept.is_empty() {
        kept.retain(|(o, k, _)| *o != i || k == key);
    }
}

/// Drops cached textures of local files that are gone (cliphist previews are deleted when
/// their delegate is destroyed) and that no item still holds, evicts the least recently used
/// unheld ones beyond the byte budget (notification images, versioned tray icons, remote
/// covers), and forgets decode failures of images no longer cached.
/// Uploaded images at least this big keep only their texture (`gpu::resident`).
const RESIDENT_BYTES: usize = 64 << 10;

thread_local! {
    /// Images drawn by CPU paths: they keep their pixels.
    static CPU_IMAGES: std::cell::RefCell<std::collections::HashSet<ImageKey>> = Default::default();
}

/// A texture and the levels after the first of its mipmap chain.
type MipChain = (std::rc::Weak<Canvas>, Rc<[Canvas]>);

thread_local! {
    /// The mipmap chains of the textures the CPU raster drew shrunk with `mipmap: true`, by
    /// texture key, each held while its texture lives.
    static MIP_LEVELS: std::cell::RefCell<HashMap<u64, MipChain>> = Default::default();
}

/// `qsg::image::mip_levels` of `tex`, made once per texture.
fn mip_levels(tex: &Rc<Canvas>) -> Rc<[Canvas]> {
    MIP_LEVELS.with_borrow_mut(|m| {
        if let Some((_, levels)) = m.get(&tex.key.get()) {
            return levels.clone();
        }
        m.retain(|_, (t, _)| t.strong_count() > 0);
        let levels: Rc<[Canvas]> = qsg::image::mip_levels(tex).into();
        m.insert(tex.key.get(), (Rc::downgrade(tex), levels.clone()));
        levels
    })
}

/// `t` (the image `src`) with CPU pixels, decoded again if they were dropped for its texture.
fn cpu_pixels(
    loads: &ImageLoads,
    src: &str,
    ss: (i32, i32),
    fill: i32,
    t: Option<Rc<Canvas>>,
) -> Option<Rc<Canvas>> {
    let key = (src.to_string(), ss, fill);
    if !CPU_IMAGES.with_borrow(|c| c.contains(&key)) {
        CPU_IMAGES.with_borrow_mut(|c| c.insert(key.clone()));
    }
    match t {
        Some(t) if t.px.len() < t.w * t.h => {
            IMAGES.with_borrow_mut(|c| c.remove(&key));
            image_texture(loads, src, ss, fill, false)
        }
        t => t,
    }
}

/// Frees the CPU pixels of big images whose texture is uploaded and that no CPU path draws,
/// and those of the cached shadows.
fn drop_uploaded_pixels() {
    let mut shown = rustc_hash::FxHashSet::default();
    IMAGES.with_borrow_mut(|c| {
        c.update(|key, t| {
            let img = t.as_ref()?;
            if img.px.len() * 4 < RESIDENT_BYTES || CPU_IMAGES.with_borrow(|c| c.contains(key)) {
                return None;
            }
            let p = crate::gpu::resident(img)?;
            if canvas_held(img) {
                shown.insert(Rc::as_ptr(img) as *const () as usize);
            }
            *t = Some(p);
            Some(image_bytes(key, t))
        })
    });
    // A cached shadow is drawn only as a texture (`Frame::image`): once it is uploaded its
    // pixels, as big as the panel it belongs to, have no reader. Its weight stays, for the
    // texture.
    LAYER_SHADOWS.with_borrow_mut(|c| {
        c.update(|_, (t, _)| {
            let p = crate::gpu::resident(t)?;
            if canvas_held(t) {
                shown.insert(Rc::as_ptr(t) as *const () as usize);
            }
            *t = p;
            None
        })
    });
    // Recorded draws still reference the old canvases: re-record them with the stand-ins.
    forget_draws_holding(&shown);
}

/// The GPU context is lost, and with it the texture of every image that had dropped its
/// pixels for it (`drop_uploaded_pixels`): those are decoded again when next drawn, and no
/// recorded draw keeps one.
pub fn forget_textures() {
    IMAGES.with_borrow_mut(|c| {
        c.retain(|_, t| t.as_ref().is_none_or(|img| img.px.len() >= img.w * img.h))
    });
    forget_draws();
}

/// Drops the recorded draws that hold any of `ptrs` (addresses of shared images). A replay
/// carries the replayed draws' references into the recording around it, so every ancestor
/// recording that contains such a draw holds it too.
fn forget_draws_holding(ptrs: &rustc_hash::FxHashSet<usize>) {
    if ptrs.is_empty() {
        return;
    }
    DL.with_borrow_mut(|d| {
        d.retain(|&i, (_, seg, _)| {
            let stale = seg.holds_any(ptrs);
            if stale {
                dl_dropped(i);
            }
            !stale
        })
    });
}

/// Directories of cached decoded local images, watched for the files leaving them
/// (inotify(7)): a name unlinked or renamed away, or the directory itself deleted, moved or
/// unmounted. IN_IGNORED and IN_Q_OVERFLOW are always delivered.
const IMAGE_DIR_MASK: inotify::WatchMask = inotify::WatchMask::DELETE
    .union(inotify::WatchMask::MOVED_FROM)
    .union(inotify::WatchMask::DELETE_SELF)
    .union(inotify::WatchMask::MOVE_SELF)
    .union(inotify::WatchMask::ONLYDIR);

/// The file a local image source reads, if it is one.
fn image_path(src: &str) -> Option<&std::path::Path> {
    let p = std::path::Path::new(src);
    (!src.contains("://") && p.is_absolute()).then_some(p)
}

/// The requested `sourceSize` and fillMode an image is cached under.
type ImageVariant = ((i32, i32), i32);

type DirFiles = std::collections::HashMap<std::ffi::OsString, std::collections::HashSet<String>>;

/// Watch state shared by the UI thread, which adds and removes watches, and the reader thread,
/// which blocks on the inotify fd while any watch exists.
#[derive(Default)]
struct ImageDirs {
    /// Watched directory path: its watch and the cached sources in it by file name.
    dirs: std::collections::HashMap<std::path::PathBuf, (inotify::WatchDescriptor, DirFiles)>,
    /// A watch follows the directory inode: every path it was added by.
    wds: std::collections::HashMap<inotify::WatchDescriptor, Vec<std::path::PathBuf>>,
    /// The instance while no reader thread owns it.
    idle: Option<inotify::Inotify>,
    wake: Option<qml::svc::Handle>,
}

type GoneTx = std::sync::mpsc::Sender<std::collections::HashSet<String>>;

struct ImageWatch {
    shared: std::sync::Arc<std::sync::Mutex<ImageDirs>>,
    watches: Option<inotify::Watches>,
    tx: GoneTx,
    rx: std::sync::mpsc::Receiver<std::collections::HashSet<String>>,
    /// Cached decoded local sources and the size/fill variants each is cached under.
    files: std::collections::HashMap<String, std::collections::HashSet<ImageVariant>>,
    /// Watched files checked once on a decode worker: deleted before their watch was added.
    verify: Vec<String>,
    /// Sources reported gone whose entries a draw still holds.
    gone: std::collections::HashSet<String>,
    /// An entry left the cache since the last prune.
    removed: bool,
}

thread_local! {
    static IMAGE_WATCH: std::cell::RefCell<ImageWatch> = {
        let (tx, rx) = std::sync::mpsc::channel();
        std::cell::RefCell::new(ImageWatch {
            shared: Default::default(),
            watches: None,
            tx,
            rx,
            files: Default::default(),
            verify: Vec::new(),
            gone: Default::default(),
            removed: false,
        })
    };
}

fn lock_dirs(m: &std::sync::Mutex<ImageDirs>) -> std::sync::MutexGuard<'_, ImageDirs> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl ImageWatch {
    fn add(&mut self, (src, ss, fill): &ImageKey) {
        let Some(p) = image_path(src) else { return };
        let variants = self.files.entry(src.clone()).or_default();
        let first = variants.is_empty();
        variants.insert((*ss, *fill));
        let (Some(dir), Some(name)) = (p.parent(), p.file_name()) else {
            return;
        };
        if !first {
            return;
        }
        let mut s = lock_dirs(&self.shared);
        if let Some((_, names)) = s.dirs.get_mut(dir) {
            names
                .entry(name.to_owned())
                .or_default()
                .insert(src.clone());
            drop(s);
            self.verify.push(src.clone());
            return;
        }
        if self.watches.is_none() {
            match inotify::Inotify::init() {
                Ok(ino) => {
                    self.watches = Some(ino.watches());
                    s.idle = Some(ino);
                }
                Err(e) => {
                    tracing::warn!("image watcher: {e}");
                    return;
                }
            }
        }
        let Some(watches) = self.watches.as_mut() else {
            return;
        };
        let wd = match watches.add(dir, IMAGE_DIR_MASK) {
            Ok(wd) => wd,
            Err(e) => {
                if matches!(e.raw_os_error(), Some(libc::ENOENT | libc::ENOTDIR)) {
                    // Its directory went since the decode.
                    self.gone.insert(src.clone());
                } else {
                    tracing::debug!("image watcher: {}: {e}", dir.display());
                }
                return;
            }
        };
        let names = std::iter::once((name.to_owned(), std::iter::once(src.clone()).collect()));
        s.dirs.insert(dir.to_owned(), (wd.clone(), names.collect()));
        s.wds.entry(wd).or_default().push(dir.to_owned());
        if let Some(ino) = s.idle.take() {
            let (shared, tx) = (self.shared.clone(), self.tx.clone());
            if let Err(e) = std::thread::Builder::new()
                .name("ri-image-watch".into())
                .spawn(move || read_image_dirs(&shared, ino, &tx))
            {
                tracing::warn!("image watcher: {e}");
            }
        }
        drop(s);
        self.verify.push(src.clone());
    }

    fn forget(&mut self, (src, ss, fill): &ImageKey) {
        let Some(variants) = self.files.get_mut(src) else {
            return;
        };
        if !variants.remove(&(*ss, *fill)) || !variants.is_empty() {
            return;
        }
        self.files.remove(src);
        self.gone.remove(src);
        let Some(p) = image_path(src) else { return };
        let (Some(dir), Some(name)) = (p.parent(), p.file_name()) else {
            return;
        };
        let mut s = lock_dirs(&self.shared);
        let Some((wd, names)) = s.dirs.get_mut(dir) else {
            return;
        };
        if let Some(srcs) = names.get_mut(name) {
            srcs.remove(src);
            if srcs.is_empty() {
                names.remove(name);
            }
        }
        if !names.is_empty() {
            return;
        }
        let wd = wd.clone();
        s.dirs.remove(dir);
        let Some(paths) = s.wds.get_mut(&wd) else {
            return;
        };
        paths.retain(|d| d != dir);
        if paths.is_empty() {
            s.wds.remove(&wd);
            // The kernel may have dropped it already (IN_IGNORED is queued then).
            if let Some(w) = self.watches.as_mut() {
                let _ = w.remove(wd);
            }
        }
    }
}

impl Drop for ImageWatch {
    fn drop(&mut self) {
        let mut s = lock_dirs(&self.shared);
        s.dirs.clear();
        // Each removal queues IN_IGNORED: the reader wakes, finds nothing watched and ends.
        for (wd, _) in s.wds.drain() {
            if let Some(w) = self.watches.as_mut() {
                let _ = w.remove(wd);
            }
        }
    }
}

/// Reads the watch events and sends the cached sources gone from their paths, waking the
/// event loop; ends, parking the instance, once nothing is watched.
fn read_image_dirs(shared: &std::sync::Mutex<ImageDirs>, mut ino: inotify::Inotify, tx: &GoneTx) {
    use inotify::EventMask as M;
    let mut buf = [0u8; 4096];
    loop {
        let events = match ino.read_events_blocking(&mut buf) {
            Ok(events) => events,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => {
                tracing::warn!("image watcher: {e}");
                lock_dirs(shared).idle = Some(ino);
                return;
            }
        };
        let mut gone = std::collections::HashSet::new();
        let mut check = Vec::new();
        let mut s = lock_dirs(shared);
        for e in events {
            if e.mask.contains(M::Q_OVERFLOW) {
                // Events were lost: every watched file is checked.
                for (dir, (_, names)) in &s.dirs {
                    for (name, srcs) in names {
                        check.extend(srcs.iter().map(|src| (dir.join(name), src.clone())));
                    }
                }
                continue;
            }
            let Some(dirs) = s.wds.get(&e.wd).cloned() else {
                continue;
            };
            if e.mask.intersects(M::DELETE | M::MOVED_FROM) {
                let Some(name) = e.name else { continue };
                for dir in &dirs {
                    if let Some(srcs) = s.dirs.get(dir).and_then(|(_, n)| n.get(name)) {
                        gone.extend(srcs.iter().cloned());
                    }
                }
            } else if e
                .mask
                .intersects(M::DELETE_SELF | M::MOVE_SELF | M::UNMOUNT | M::IGNORED)
            {
                // The directory left its paths: so did its files.
                s.wds.remove(&e.wd);
                for dir in dirs {
                    if let Some((_, names)) = s.dirs.remove(&dir) {
                        gone.extend(names.into_values().flatten());
                    }
                }
                if !e.mask.contains(M::IGNORED) {
                    let _ = ino.watches().remove(e.wd);
                }
            }
        }
        let wake = s.wake.clone();
        let done = s.dirs.is_empty();
        drop(s);
        gone.extend(
            check
                .into_iter()
                .filter(|(p, _)| !p.exists())
                .map(|(_, src)| src),
        );
        if !gone.is_empty() && tx.send(gone).is_ok() {
            wake.inspect(qml::svc::Handle::wake);
        }
        if done {
            let mut s = lock_dirs(shared);
            if s.dirs.is_empty() {
                s.idle = Some(ino);
                return;
            }
        }
    }
}

/// Keeps the watch of a cached entry's directory in step with what is cached under the key.
fn image_cached(key: &ImageKey, decoded: bool) {
    IMAGE_WATCH.with_borrow_mut(|w| if decoded { w.add(key) } else { w.forget(key) });
}

fn image_evicted(key: &ImageKey) {
    let _ = IMAGE_WATCH.try_with(|w| {
        let mut w = w.borrow_mut();
        w.forget(key);
        w.removed = true;
    });
}

/// Drops the cached decoded images of local files reported gone that no draw holds
/// (cliphist previews are deleted when their delegate is destroyed), evicts the least
/// recently used unheld ones beyond the byte budget (notification images, versioned tray
/// icons, remote covers), and forgets decode failures of images no longer cached.
fn prune_images() {
    let (keys, removed) = IMAGE_WATCH.with_borrow_mut(|w| {
        let verify = std::mem::take(&mut w.verify);
        if !verify.is_empty() {
            let tx = w.tx.clone();
            let wake = lock_dirs(&w.shared).wake.clone();
            // A stat per new file: on a decode worker, a slow or cold disk stalls no frame.
            submit_decode(Box::new(move || {
                let gone: std::collections::HashSet<String> = guarded(|| {
                    Some(
                        verify
                            .into_iter()
                            .filter(|src| !image_path(src).is_some_and(std::path::Path::exists))
                            .collect(),
                    )
                })
                .unwrap_or_default();
                if !gone.is_empty() && tx.send(gone).is_ok() {
                    wake.inspect(qml::svc::Handle::wake);
                }
            }));
        }
        let reported: Vec<_> = w.rx.try_iter().flatten().collect();
        for src in reported {
            if w.files.contains_key(&src) {
                w.gone.insert(src);
            }
        }
        let keys: Vec<ImageKey> = w
            .gone
            .iter()
            .flat_map(|src| {
                w.files[src]
                    .iter()
                    .map(|&(ss, fill)| (src.clone(), ss, fill))
            })
            .collect();
        (keys, std::mem::take(&mut w.removed))
    });
    IMAGES.with_borrow_mut(|c| {
        for k in &keys {
            if c.peek(k).is_some_and(|t| !image_held(t)) {
                c.remove(k);
            }
        }
        c.evict(None);
    });
    if removed || !keys.is_empty() {
        IMAGES.with(|c| {
            let c = c.borrow();
            FAILED.with(|f| f.borrow_mut().retain(|k, _| c.contains(k)));
            CPU_IMAGES.with_borrow_mut(|k| k.retain(|k| c.contains(k)));
        });
    }
    drop_uploaded_pixels();
}

/// Async decodes and remote downloads landed since the last call: the Images showing them are
/// invalidated and returned (relaid out locally; a global layout_rev bump would relayout every
/// window once per finished image). `true` when a download finished: everything relayouts too.
pub fn take_finished_images(w: &mut World) -> (Vec<usize>, bool) {
    // Held until the Images waiting for them have their share: `take_downloaded` evicts.
    let landed = take_landed(w);
    let fetched = take_downloaded(w);
    let inline = take_inline(image_loads(w));
    // A finished download changes the Image's status too: a hidden `visible: status ===
    // Image.Ready` Image (StyledImage) is not reached by the relayout below.
    let srcs: std::collections::HashSet<String> = (landed.iter().map(|(k, _)| k.0.clone()))
        .chain(fetched.iter().cloned())
        .collect();
    let mut hit = take_own(w);
    if !srcs.is_empty() {
        // An AnimatedImage is not an Image by type: its movie opens on the same pool.
        for ty in ["Image", "AnimatedImage"] {
            for i in w.of_type(ty) {
                let src = w.read(i, "source").map(|v| v.to_js_string());
                if src.is_some_and(|s| srcs.contains(&w.resolved_url(i, &s))) && !hit.contains(&i) {
                    hit.push(i);
                }
            }
        }
    }
    if inline {
        // The Markdown texts showing an inline image take its size now.
        for ty in ["Text", "TextEdit"] {
            for i in w.of_type(ty) {
                if !hit.contains(&i)
                    && w.read(i, "text")
                        .is_some_and(|v| v.to_js_string().contains("!["))
                {
                    hit.push(i);
                }
            }
        }
    }
    keep_landed(w, &hit, landed);
    if !hit.is_empty() {
        for &i in &hit {
            w.memo_invalidate(i);
            w.resized.push(i);
            w.touched.push(i);
        }
    }
    if !fetched.is_empty() {
        finish_downloads(w, &fetched);
        w.layout_rev += 1;
        w.memo_m.clear();
        w.memo_p.clear();
    }
    // QQuickImageBase::requestFinished: each picture has its status and size as its load
    // ends, in the order the loads began, not when a layout next reaches it.
    let mut pictures: Vec<(u64, usize)> = WAITING.with_borrow_mut(|(_, m)| {
        m.retain(|i, _| !w.dead.contains(i));
        let at = |i: &usize| Some((m.get(i)?.seq, *i));
        hit.iter().filter_map(at).collect()
    });
    pictures.sort();
    pictures.dedup();
    for (_, i) in pictures {
        // Behaviors run only once the picture has been laid out: it has the answer then.
        if w.fresh.contains(&i) {
            owe(i);
            continue;
        }
        own_answered(i);
        let was = num(w, i, "status");
        let (iw, ih) = picture_load(w, i, Load::Answered);
        take_implicit(w, i, iw, ih);
        let told = num(w, i, "status") != was;
        picture_sized(w, i, told);
    }
    (hit, !fetched.is_empty())
}

/// Downloads done: no longer pending (a failed fetch now reads as Error).
pub fn finish_downloads(w: &World, srcs: &[String]) {
    forget_draws();
    let loads = image_loads(w);
    let mut d = loads.downloading.borrow_mut();
    let mut failed = loads.fetch_failed.borrow_mut();
    for s in srcs {
        d.remove(s);
        if is_remote(s) && !remote_cache_file(s).is_file() {
            failed.insert(s.clone());
        }
    }
}

fn is_remote(src: &str) -> bool {
    src.starts_with("http://") || src.starts_with("https://")
}

fn remote_pending(loads: &ImageLoads, src: &str) -> bool {
    loads.downloading.borrow().contains(src)
}

/// A network Image source over this many bytes is not kept (the Image errors).
pub const REMOTE_IMAGE_MAX: u64 = 64 << 20;

thread_local! {
    /// Per Image, the network source it last asked for ([`remote_request`]).
    static REMOTE_ASKED: std::cell::RefCell<HashMap<usize, String>> = Default::default();
}

/// Image `i` is laid out with `src`. A failed network source stays Error, as in QQuickImage,
/// until an Image asks for it anew: its `source` was assigned (`assigned`,
/// QQuickImageBase::setSource; `source = ""; source = url` is how QML retries) or it is an
/// Image that has not asked for it yet (a new delegate, QQuickImageBase::componentComplete).
/// Then the next [`remote_image`] fetches it again. No clock retries it.
fn remote_request(loads: &ImageLoads, i: usize, src: &str, assigned: bool) {
    if !is_remote(src) {
        REMOTE_ASKED.with_borrow_mut(|a| a.remove(&i));
        return;
    }
    let anew = REMOTE_ASKED.with_borrow_mut(|a| {
        let known = a.get(&i).is_some_and(|s| s == src);
        if !known {
            a.insert(i, src.to_string());
        }
        assigned || !known
    });
    if anew
        && !loads.downloading.borrow().contains(src)
        && loads.fetch_failed.borrow_mut().remove(src)
    {
        // The failure is cached as an image that did not load: forgotten, it loads again.
        IMAGES.with_borrow_mut(|c| c.retain(|k, t| k.0 != src || t.is_some()));
    }
}

/// Bytes of downloaded Image sources kept on disk; past it the oldest downloads go
/// (QQuickImage's QNetworkAccessManager keeps no disk cache at all).
pub const REMOTE_CACHE_MAX: u64 = 256 << 20;

/// Deletes the oldest files in `dir` until the rest fit `budget` bytes.
fn prune_net_cache(dir: &std::path::Path, budget: u64) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<_> = rd
        .flatten()
        .filter_map(|e| {
            let m = e.metadata().ok().filter(|m| m.is_file())?;
            Some((m.modified().ok()?, m.len(), e.path()))
        })
        .collect();
    files.sort_unstable_by_key(|f| std::cmp::Reverse(f.0));
    let mut kept = 0u64;
    for (_, len, path) in files {
        kept += len;
        if kept > budget {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn remote_cache_file(src: &str) -> std::path::PathBuf {
    use std::hash::{Hash, Hasher};
    let mut h = rustc_hash::FxHasher::default();
    src.hash(&mut h);
    qml::xdg::cache_home()
        .join("rusty-impulse/net")
        .join(format!("{:016x}", h.finish()))
}

/// QQuickImage network source: fetched once into the cache dir on the blocking pool, given up
/// when the server stalls ([`qml::http::STALL`]), the body passes [`REMOTE_IMAGE_MAX`] or the
/// world goes away.
fn remote_image(loads: &ImageLoads, src: &str) -> Option<String> {
    let f = remote_cache_file(src);
    if f.is_file() {
        return Some(f.to_string_lossy().into_owned());
    }
    let dir = f.parent()?.to_path_buf();
    let failed = loads.fetch_failed.borrow().contains(src);
    if !failed && loads.downloading.borrow_mut().insert(src.to_string()) {
        let (src, f, tx) = (src.to_string(), f.clone(), loads.tx.clone());
        let stop = loads.stop.0.clone();
        workpool::long(move || {
            let _ = std::fs::create_dir_all(&dir);
            let o = qml::http::Opts {
                follow: true,
                stall: true,
                ..Default::default()
            };
            // QQuickImage's QNetworkAccessManager sends Qt's default User-Agent.
            let ua = [("User-Agent".to_string(), "Mozilla/5.0".to_string())];
            let _ = qml::http::with_stop(&stop, || {
                qml::http::download_max(&src, &ua, &f, true, o, REMOTE_IMAGE_MAX)
            });
            prune_net_cache(&dir, REMOTE_CACHE_MAX);
            let _ = tx.send(Loaded::Downloaded(src));
        });
    }
    None
}

/// Local file behind an Image `source` requested at `ss`; like QImageReader, a missing suffix
/// is probed.
fn image_file(loads: &ImageLoads, src: &str, ss: (i32, i32)) -> Option<String> {
    if let Some(f) = src.strip_prefix("image://kicon/") {
        return std::path::Path::new(f).is_file().then(|| f.to_string());
    }
    if src.starts_with("image://icon/") {
        let (w, h) = provider_target(ss);
        // An own-theme (`?path=`) search finishing on a worker relays out like a download.
        let (tx, s) = (loads.tx.clone(), src.to_string());
        return qml::icons::resolve_url(src, w.min(h), screens_scale(), move || {
            let _ = tx.send(Loaded::Downloaded(s));
        });
    }
    if src.starts_with("http://") || src.starts_with("https://") {
        return remote_image(loads, src);
    }
    let p = src;
    if p.is_empty() {
        return None;
    }
    if std::path::Path::new(p).is_file() {
        return Some(p.to_string());
    }
    [
        "png", "svg", "jpg", "jpeg", "webp", "avif", "heic", "heif", "gif", "bmp",
    ]
    .iter()
    .map(|e| format!("{p}.{e}"))
    .find(|f| std::path::Path::new(f).is_file())
}

thread_local! {
    /// Latest captured frame per `ScreencopyView` item.
    static SCREENCOPY: std::cell::RefCell<HashMap<usize, std::rc::Rc<Canvas>>> = Default::default();
}

thread_local! {
    /// Frame size last installed per `ScreencopyView`: a same-size frame only swaps pixels.
    static SCREENCOPY_SIZE: std::cell::RefCell<HashMap<usize, (u32, u32)>> = Default::default();
}

pub fn screencopy_size(i: usize) -> Option<(u32, u32)> {
    SCREENCOPY_SIZE.with_borrow(|m| m.get(&i).copied())
}

/// Records the size of `i`'s installed frame; `true` when it differs from the previous one.
pub fn set_screencopy_size(i: usize, size: (u32, u32)) -> bool {
    SCREENCOPY_SIZE.with_borrow_mut(|m| m.insert(i, size) != Some(size))
}

pub fn screencopy(i: usize) -> Option<std::rc::Rc<Canvas>> {
    SCREENCOPY.with(|c| c.borrow().get(&i).cloned())
}

pub fn set_screencopy(i: usize, tex: Option<std::rc::Rc<Canvas>>) {
    SCREENCOPY.with(|c| match tex {
        Some(t) => {
            c.borrow_mut().insert(i, t);
        }
        None => {
            c.borrow_mut().remove(&i);
        }
    });
}

/// A load a picture makes for itself, past what is stored.
struct OwnLoad {
    key: ImageKey,
    /// Asked for and not answered yet: the picture is Loading.
    pending: bool,
    /// The picture has `cache` on: once answered, the stored image is in the pixmap cache.
    shares: bool,
}

thread_local! {
    /// Per picture, the load it made for itself ([`picture_texture`]).
    static OWN: std::cell::RefCell<HashMap<usize, OwnLoad>> = Default::default();
    /// Stored images only pictures with `cache: false` loaded: QQuickPixmap does not put
    /// those in its cache, so a picture with `cache` on does not find them there.
    static UNSHARED: std::cell::RefCell<std::collections::HashSet<ImageKey>> = Default::default();
}

/// Picture `i` waits for a load of its own.
fn own_pending(i: usize) -> bool {
    OWN.with_borrow(|o| o.get(&i).is_some_and(|l| l.pending))
}

/// The load of its own picture `i` waited for is done: it shows what is stored now.
fn own_answered(i: usize) {
    OWN.with_borrow_mut(|o| {
        if let Some(l) = o.get_mut(&i).filter(|l| l.pending) {
            l.pending = false;
            if l.shares {
                UNSHARED.with_borrow_mut(|u| u.remove(&l.key));
            }
        }
    });
}

/// How a picture asks for its image.
#[derive(Clone, Copy)]
struct Ask {
    asynchronous: bool,
    /// The picture has `cache: false`.
    own: bool,
    /// The picture has been laid out, so its Behaviors run.
    laid_out: bool,
}

/// [`image_texture`] for picture `i` as QQuickPixmap::load answers it. A picture with
/// `cache: false` never takes a stored image as it is and leaves none for the others: where
/// Qt reads the file again on its reader thread, an asynchronous one is Loading until the
/// event loop has turned once after its creation and first layout, then shows what is stored.
/// A picture with `cache` on does the same for an image only `cache: false` pictures loaded.
fn picture_texture(
    loads: &ImageLoads,
    i: usize,
    src: &str,
    ss: (i32, i32),
    fill: i32,
    ask: Ask,
) -> Option<std::rc::Rc<Canvas>> {
    let key = (src.to_string(), ss, fill);
    let stored = IMAGES.with_borrow(|c| c.peek(&key).is_some_and(|t| t.is_some()));
    let unshared = UNSHARED.with_borrow(|u| u.contains(&key));
    if !ask.own && !(unshared && stored) {
        if unshared {
            UNSHARED.with_borrow_mut(|u| u.remove(&key));
        }
        OWN.with_borrow_mut(|o| o.remove(&i));
        return image_texture(loads, src, ss, fill, ask.asynchronous);
    }
    let known = OWN.with_borrow(|o| o.get(&i).filter(|l| l.key == key).map(|l| l.pending));
    match known {
        Some(true) => return None,
        Some(false) => {}
        None => {
            let pending = stored && ask.asynchronous;
            if pending {
                if ask.laid_out {
                    let _ = loads.tx.send(Loaded::Own(i));
                }
            } else if !ask.own {
                UNSHARED.with_borrow_mut(|u| u.remove(&key));
            } else if !stored && !decode_pending(loads, src) {
                UNSHARED.with_borrow_mut(|u| u.insert(key.clone()));
            }
            let load = OwnLoad {
                key,
                pending,
                shares: !ask.own,
            };
            OWN.with_borrow_mut(|o| o.insert(i, load));
            if pending {
                return None;
            }
        }
    }
    image_texture(loads, src, ss, fill, ask.asynchronous)
}

/// The pictures whose load is done since the last call ([`Loaded::Own`]).
fn take_own(w: &World) -> Vec<usize> {
    let mut done = std::mem::take(&mut *image_loads(w).own.borrow_mut());
    if !done.is_empty() {
        done.retain(|i| !w.dead.contains(i));
        OWN.with_borrow_mut(|o| o.retain(|i, _| !w.dead.contains(i)));
    }
    done
}

/// Decoded (and `sourceSize`-scaled) texture of an Image, cached per source/size.
fn image_texture(
    loads: &ImageLoads,
    src: &str,
    ss: (i32, i32),
    fill: i32,
    asynchronous: bool,
) -> Option<std::rc::Rc<Canvas>> {
    let key = (src.to_string(), ss, fill);
    if still_pending(loads, &key, asynchronous) {
        return None;
    }
    if let Some(t) = IMAGES.with(|c| c.borrow_mut().get(&key).cloned()) {
        // A missing local file (thumbnail not generated yet) loads once it appears.
        let local = !src.contains("://");
        // A file that failed to decode is retried only once it changes.
        if t.is_some()
            || !local
            || image_file(loads, src, ss).is_none_or(|f| failed_unchanged(&key, &f))
        {
            return t;
        }
    }
    if qml::log_on!("ri::imgdbg") {
        tracing::debug!(target: "ri::imgdbg", "imgload {src} ss={ss:?} fill={fill}");
    }
    if src.starts_with("image://service.sni/") {
        // Not cached while a named layer's own-theme search runs: it loads once that lands.
        let t = Some(std::rc::Rc::new(tray_icon(loads, src, ss)?));
        cache_image(key, t.clone());
        return t;
    }
    let sni = src.starts_with("image://qsimage/").then(|| {
        let (w, h, px) = qml::notifications::pixmap(src)?;
        if w == 0 || h == 0 || w.checked_mul(h) != Some(px.len()) {
            return None;
        }
        let tex = Canvas::from_px(w, h, px);
        let (tw, th) = if ss.0 > 0 && ss.1 > 0 {
            (ss.0 as usize, ss.1 as usize)
        } else {
            (w, h)
        };
        Some(if (tw, th) != (w, h) {
            qsg::image::scale_decoded(&tex, tw, th, true)
        } else {
            tex
        })
    });
    if let Some(t) = sni {
        let t = Some(std::rc::Rc::new(t.unwrap_or_else(|| unresolved_icon(ss))));
        cache_image(key, t.clone());
        return t;
    }
    if src.starts_with("image://dbusmenu/") {
        // DBusMenuPngImage: the PNG at its own size, a null image when it fails to decode.
        let sized = |w, h| match ss.0 > 0 && ss.1 > 0 {
            true => (ss.0 as usize, ss.1 as usize),
            false => (w, h),
        };
        let t = qml::sni::menu_png(src)
            .and_then(|d| qsg::image::decode_scaled(&d, sized).ok())
            .map(|(tex, _)| std::rc::Rc::new(tex));
        cache_image(key, t.clone());
        return t;
    }
    let file = image_file(loads, src, ss);
    // QQuickPixmapReader's thread opens the file: one that is not there fails there too.
    if asynchronous && !src.starts_with("image://") && (file.is_some() || !is_remote(src)) {
        decode_async(loads, key, file.unwrap_or_else(|| src.to_string()));
        return None;
    }
    let read = file.as_deref().map(file_mtime);
    let tex = file.and_then(|f| {
        let scheme = icon_scheme(src, &f);
        if fill == PROVIDER_FILL {
            decode_file(&f, provider_pixels(ss), LOADER_FILL, scheme.as_deref())
        } else {
            decode_file(&f, ss, fill, scheme.as_deref())
        }
    });
    let tex = tex.or_else(|| {
        (src.starts_with("image://icon/") && !src.contains("?path=")).then(|| unresolved_icon(ss))
    });
    let tex = tex.map(std::rc::Rc::new);
    if tex.is_none()
        && let Some(read) = read
    {
        note_failed(&key, read);
    }
    let pending = if is_remote(src) {
        remote_pending(loads, src)
    } else {
        qml::icons::pending(src)
    };
    if tex.is_some() || !pending {
        cache_image(key, tex.clone());
    }
    tex
}

/// Icon::updatePolish when the Kirigami.Icon `i` is painted showing the image `key`: a written
/// source, color or isMask or a new size polishes it (`World::polish_icon`). While it fades,
/// the image it fades from and how far the fade is (Icon::m_oldIcon, m_animValue).
fn icon_fade(w: &mut World, i: usize, key: &ImageKey, size: (f64, f64)) -> Option<(ImageKey, f64)> {
    let resized = w.icons.get(&i).is_some_and(|f| f.size != size);
    if resized || w.icons_unpolished.contains_key(&i) {
        w.polish_icon(i, resized);
    }
    let icon = w.icons.entry(i).or_default();
    icon.size = size;
    if icon.shown.as_ref() != Some(key) {
        icon.shown = Some(key.clone());
    }
    let from = icon.from.clone()?;
    let mix = num(w, i, qml::runtime::ICON_MIX);
    (from != *key && mix < 1.0).then_some((from, mix))
}

/// Kirigami's icon.frag with ENABLE_MIX: `mix(old, new, m)` of the premultiplied pixels, the
/// old image stretched over the new one's rectangle. An image that is not there is transparent.
fn icon_mix(old: Option<&Canvas>, new: Option<&Canvas>, m: f64) -> Option<Rc<Canvas>> {
    let (w, h) = new.or(old).map(|c| (c.w, c.h))?;
    let at = |c: Option<&Canvas>, x: usize, y: usize| {
        c.map_or([0; 4], |c| c.px[(y * c.h / h) * c.w + x * c.w / w])
    };
    let mut out = Canvas::new(w, h, [0, 0, 0, 0]);
    for y in 0..h {
        for x in 0..w {
            let (o, n) = (at(old, x, y), at(new, x, y));
            out.px[y * w + x] = std::array::from_fn(|k| {
                (f64::from(o[k]) + (f64::from(n[k]) - f64::from(o[k])) * m).round() as u8
            });
        }
    }
    Some(Rc::new(out))
}

/// An `image://service.sni/` tray icon at `ss`; `None` while one of its named layers is still
/// searched in the item's own theme (the source is announced as loaded once it lands).
fn tray_icon(loads: &ImageLoads, src: &str, ss: (i32, i32)) -> Option<Canvas> {
    let Some(icon) = qml::sni::icon(src, ss.0, ss.1) else {
        return Some(unresolved_icon(ss));
    };
    let (tx, src) = (loads.tx.clone(), src.to_string());
    compose_tray(icon, ss, move || {
        let (tx, src) = (tx.clone(), src.clone());
        move || {
            let _ = tx.send(Loaded::Downloaded(src));
        }
    })
}

/// A tray icon's base at `ss` (at 100x100 when `ss` gives none, a pixmap as well as a named
/// icon: Quickshell's TrayImageHandle::requestPixmap) with its overlay painted over the whole
/// of it, as Quickshell 0.3.2 paints it (StatusNotifierItem::createPixmap: the overlay at the
/// base's size, `drawPixmap` into the base's rectangle). A base the item does not give draws
/// as Quickshell's missing picture, a missing overlay is left out; `None` while a named layer
/// is pending, `ready()` making what is called once it lands.
fn compose_tray<R: FnOnce() + Send + 'static>(
    icon: qml::sni::TrayIcon,
    ss: (i32, i32),
    ready: impl Fn() -> R,
) -> Option<Canvas> {
    let asked = match ss {
        (w, h) if w > 0 && h > 0 => (w as usize, h as usize),
        _ => (100, 100),
    };
    // A name the themes do not have draws as any unresolved icon; an item that gives neither
    // a name nor a pixmap that can be drawn gets Quickshell's missing picture
    // (TrayImageHandle::requestPixmap: a null pixmap from createPixmap).
    let mut out = match icon.base {
        Some(l) => {
            let named = matches!(l, qml::sni::Layer::Icon(_));
            match tray_layer(l, Some(asked), ready())? {
                Some(c) => c,
                None if named => unresolved_icon(ss),
                None => missing_pixmap(ss),
            }
        }
        None => missing_pixmap(ss),
    };
    if let Some(l) = icon.overlay
        && let Some(mut o) = tray_layer(l, Some((out.w, out.h)), ready())?
    {
        if (o.w, o.h) != (out.w, out.h) {
            // A named icon that is not square: stretched over the base.
            o = qsg::image::scale_decoded(&o, out.w, out.h, true);
        }
        paint_over(&mut out, &o, 0, 0);
    }
    Some(out)
}

/// One tray icon layer at `size` (without one, pixmaps: their own size, names: 100x100):
/// `Some(None)` when it cannot be drawn, `None` while its own-theme search runs.
fn tray_layer(
    l: qml::sni::Layer,
    size: Option<(usize, usize)>,
    ready: impl FnOnce() + Send + 'static,
) -> Option<Option<Canvas>> {
    match l {
        qml::sni::Layer::Pixels(w, h, px) => {
            if w == 0 || h == 0 || w.checked_mul(h) != Some(px.len()) {
                return Some(None);
            }
            // Scaled while its pixels are straight, a pixmap after (Quickshell's
            // StatusNotifierItem::createPixmap).
            let (tw, th) = size.unwrap_or((w, h));
            Some(Some(qsg::image::scale_argb32(&px, w, h, tw, th)))
        }
        qml::sni::Layer::Icon(url) => {
            let (w, h) = size.unwrap_or((100, 100));
            let ss = (w as i32, h as i32);
            match qml::icons::resolve_url(&url, ss.0.min(ss.1), screens_scale(), ready) {
                Some(f) => Some(decode_file(
                    &f,
                    provider_pixels(ss),
                    LOADER_FILL,
                    icon_scheme(&url, &f).as_deref(),
                )),
                None if qml::icons::pending(&url) => None,
                None => Some(None),
            }
        }
    }
}

/// Premultiplied source-over of `src` onto `dst` with its top-left at `x`, `y` (clipped), as
/// QPainter does it on the processor.
fn paint_over(dst: &mut Canvas, src: &Canvas, x: usize, y: usize) {
    for sy in 0..src.h.min(dst.h.saturating_sub(y)) {
        for sx in 0..src.w.min(dst.w.saturating_sub(x)) {
            let s = src.px[sy * src.w + sx];
            let d = &mut dst.px[(y + sy) * dst.w + x + sx];
            let keep = 255 - s[3] as u32;
            for c in 0..4 {
                d[c] = (s[c] as u32 + mul255::qt(d[c] as u32, keep)).min(255) as u8;
            }
        }
    }
}

/// The size of IconImageProvider::missingPixmap asked for at `ss`: the size of the request
/// (100x100 when none), rounded up to even.
fn missing_size(ss: (i32, i32)) -> (usize, usize) {
    let (w, h) = provider_target(ss);
    let even = |v: i32| (v + v % 2) as usize;
    (even(w), even(h))
}

/// Quickshell's IconImageProvider::missingPixmap: black, with magenta (`#d900d8`) in its top
/// right and bottom left quarters.
fn missing_pixmap(ss: (i32, i32)) -> Canvas {
    let (w, h) = missing_size(ss);
    let mut c = Canvas::new(w, h, [0, 0, 0, 255]);
    for (i, p) in c.px.iter_mut().enumerate() {
        if (i % w >= w / 2) != (i / w >= h / 2) {
            *p = [0xd9, 0x00, 0xd8, 255];
        }
    }
    c
}

/// Where Quickshell draws IconImageProvider::missingPixmap for a name its themes do not have:
/// the theme's generic icon as a request of that size finds it, in a transparent image of the
/// missing picture's size, so layouts keep the size without a missing-texture square.
fn unresolved_icon(ss: (i32, i32)) -> Canvas {
    let (w, h) = missing_size(ss);
    let mut out = Canvas::new(w, h, [0, 0, 0, 0]);
    let (w, h) = (w as i32, h as i32);
    let icon = qml::icons::generic_icon(w.min(h), screens_scale()).and_then(|f| {
        let scheme = icon_scheme("image://icon/", &f);
        decode_file(&f, (w, h), LOADER_FILL, scheme.as_deref())
    });
    if let Some(icon) = icon {
        let (x, y) = (
            out.w.saturating_sub(icon.w) / 2,
            out.h.saturating_sub(icon.h) / 2,
        );
        paint_over(&mut out, &icon, x, y);
    }
    out
}

/// Item subtree rendered into its own texture (layer / ShaderEffectSource), even if hidden.
/// An effect samples its source texture stretched over the effect item's rect.
fn copy_canvas(t: &Canvas) -> Canvas {
    let mut c = Canvas::new(t.w, t.h, [0, 0, 0, 0]);
    c.px.copy_from_slice(&t.px);
    c
}

/// A rendered layer source. Re-rendering an invalidated layer to the same pixels keeps it, so
/// the GPU keeps its texture.
#[derive(Clone)]
struct Layer {
    px: Rc<Canvas>,
}

impl Layer {
    fn new(px: Canvas) -> Layer {
        Layer { px: Rc::new(px) }
    }
}

struct LayerEntry {
    size: (i64, i64),
    layer: Layer,
    /// The item or a descendant changed since `layer` was rendered.
    stale: bool,
}

/// `LAYER_CACHE` key: the item, and whether it is rendered without its own `layer.effect`
/// (as that effect's implicit source) or through it (as another effect's source).
type LayerKey = (usize, bool);

thread_local! {
    /// Rendered layer sources by item and size; marked stale when the item or a descendant
    /// changes, and replaced by the next render whose pixels differ.
    static LAYER_CACHE: std::cell::RefCell<HashMap<LayerKey, LayerEntry>> = Default::default();
}

/// Mark the cached layers of `items` and all their ancestors stale.
fn stale_layers(w: &World, items: &[usize]) {
    LAYER_CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if c.is_empty() {
            return;
        }
        for &t in items {
            let mut cur = Some(t);
            while let Some(i) = cur {
                for bare in [false, true] {
                    if let Some(e) = c.get_mut(&(i, bare)) {
                        e.stale = true;
                    }
                }
                cur = w.items.get(i).and_then(|it| it.parent);
            }
        }
    });
}

/// Drop every per-item render cache entry of destroyed items.
/// ri::wstat: sizes of the per-item and image caches.
pub fn cache_stats() -> String {
    format!(
        "layer={} screencopy={} df_nodes={} mat_anim={} view_cur={} dl={} dl_deps={} images={} images_bytes={} keyed={} canvases={} replays={} shadows={} shadows_px={} id_chunks={:?}",
        LAYER_CACHE.with(|c| c.borrow().len()),
        SCREENCOPY.with(|c| c.borrow().len()),
        DF_NODES.with(|c| c.borrow().len()),
        MAT_ANIM.with(|c| c.borrow().len()),
        VIEW_CURRENT.with(|c| c.borrow().len()),
        DL.with(|c| c.borrow().len()),
        DL_DEPS.with(|c| c.borrow().len()),
        IMAGES.with(|c| c.borrow().len()),
        IMAGES.with(|c| c.borrow().bytes()),
        KEYED.with(|c| c.borrow().len()),
        CANVASES.with(|c| c.borrow().len()),
        REPLAYS.with(|c| c.borrow().len()),
        LAYER_SHADOWS.with(|c| c.borrow().len()),
        LAYER_SHADOWS.with(|c| c
            .borrow()
            .iter()
            .map(|(_, t)| t.0.px.len() * 4)
            .sum::<usize>()),
        // Chunks and listed values of the per-id tables: TYPE_BITS, SRC_KEYS, DL_STAMP.
        [
            TYPE_BITS.with_borrow(|v| v.allocated()),
            SRC_KEYS.with_borrow(|v| v.allocated()),
            DL_STAMP.with_borrow(|v| v.allocated()),
        ],
    )
}

/// Drops the render state of reaped items. Instance ids are never reused
/// (`qml::runtime::Arena`), so this only frees memory: state left behind can't be drawn for
/// another item.
pub fn forget_items(ids: &[usize]) {
    let gone: std::collections::HashSet<usize> = ids.iter().copied().collect();
    // Destroying an item already touched its parent, dropping the draws that contain it; only a
    // surviving effect sampling a destroyed source may still hold it.
    let stale_effect = DL_DEPS.with(|d| {
        d.borrow()
            .iter()
            .any(|(k, es)| gone.contains(k) && es.iter().any(|e| !gone.contains(e)))
    });
    if stale_effect {
        forget_draws();
    } else if !gone.is_empty() {
        DL.with(|d| d.borrow_mut().retain(|k, _| !gone.contains(k)));
        DL_MOVED.with(|d| d.borrow_mut().retain(|k, _| !gone.contains(k)));
        DL_DEPS.with(|d| {
            let mut d = d.borrow_mut();
            d.retain(|k, _| !gone.contains(k));
            for es in d.values_mut() {
                es.retain(|e| !gone.contains(e));
            }
        });
        DL_USED.with_borrow_mut(|u| u.retain(|k| !gone.contains(k)));
        DL_HOT.with_borrow_mut(|h| h.retain(|k, _| !gone.contains(k)));
    }
    LAYER_CACHE.with(|c| c.borrow_mut().retain(|k, _| !gone.contains(&k.0)));
    for &i in &gone {
        REMOTE_ASKED.with_borrow_mut(|c| c.remove(&i));
        KEYED.with_borrow_mut(|c| c.remove(&i));
        CANVASES.with_borrow_mut(|c| c.remove(&i));
        CANVAS_RUNS.with_borrow_mut(|c| c.remove(&i));
        SHOWN.with_borrow_mut(|c| c.remove(&i));
        REPLAYS.with_borrow_mut(|c| c.remove(&i));
        CURVES.with_borrow_mut(|c| c.remove(&i));
    }
    SCREENCOPY.with(|c| c.borrow_mut().retain(|k, _| !gone.contains(k)));
    SCREENCOPY_SIZE.with_borrow_mut(|c| c.retain(|k, _| !gone.contains(k)));
    MAT_LH.with_borrow_mut(|c| c.retain(|k, _| !gone.contains(k)));
    AREA_SIZED.with_borrow_mut(|c| c.retain(|k, _| !gone.contains(k)));
    INPUT_SCROLL.with_borrow_mut(|c| c.retain(|k, _| !gone.contains(k)));
    SWIPE_POS.with_borrow_mut(|c| c.retain(|k, _| !gone.contains(k)));
    SLIDER_POS.with_borrow_mut(|c| c.retain(|k, _| !gone.contains(k)));
    FRESH_CLEAN.with_borrow_mut(|c| c.retain(|k, _| !gone.contains(k)));
    DF_NODES.with(|c| c.borrow_mut().retain(|k, _| !gone.contains(k)));
    DF_MOVED.with(|c| c.borrow_mut().retain(|k, _| !gone.contains(k)));
    SG_TEXTS.with(|c| c.borrow_mut().retain(|k, _| !gone.contains(k)));
    MAT_ANIM.with(|c| c.borrow_mut().retain(|k, _| !gone.contains(k)));
    LAYER_CANDS.with(|c| c.borrow_mut().retain(|k, _| !gone.contains(k)));
    VIEW_CURRENT.with(|c| {
        c.borrow_mut()
            .retain(|k, v| !gone.contains(k) && !gone.contains(v))
    });
}

/// Drop the per-id render state of arena chunks whose instances are all destroyed.
pub fn forget_chunks(w: &World, chunks: &[usize]) {
    for &c in chunks {
        TYPE_BITS.with_borrow_mut(|v| v.keep_only(c, w.items.held_in(c)));
        SRC_KEYS.with_borrow_mut(|v| v.keep_only(c, w.items.held_in(c)));
        DL_STAMP.with_borrow_mut(|v| v.keep_only(c, w.items.held_in(c)));
    }
}

/// Drop the recorded draws of `items` and their ancestors (their child lists changed).
pub fn invalidate_parents(w: &World, items: &[usize]) {
    dl_invalidate(w, items, &[]);
}

/// `moved` items changed position only: their own draws are keyed by it, so just the draws
/// containing them (and effects sampling them) go stale.
pub fn invalidate_moved(w: &World, moved: &[usize]) {
    let mut ups: Vec<usize> = moved
        .iter()
        .filter_map(|&m| w.items.get(m).and_then(|it| it.parent))
        .collect();
    DL_DEPS.with(|deps| {
        let set: rustc_hash::FxHashSet<usize> = moved.iter().copied().collect();
        for (&src, es) in deps.borrow().iter() {
            let mut a = Some(src);
            while let Some(x) = a {
                if set.contains(&x) {
                    ups.extend(es.iter().copied());
                    break;
                }
                a = w.items.get(x).and_then(|it| it.parent);
            }
        }
    });
    dl_invalidate(w, &ups, &[]);
    stale_layers(w, moved);
}

/// The nearest strict ancestor of `t` below its window that is not drawn: one with
/// `visible: false`, or a delegate its view culls (`view_culls`). Qt drops such a subtree's
/// nodes from the scene graph (QQuickItemPrivate::setCulled counts a hide reference on the
/// delegate, as `hideSource` does on a source), so a change inside it alters no pixel of the
/// window.
pub fn hidden_ancestor(w: &mut World, t: usize) -> Option<usize> {
    let mut cur = w.items.get(t).and_then(|it| it.parent);
    while let Some(i) = cur {
        if type_bits(w, i) & WINDOW_TYPE != 0 {
            return None;
        }
        let up = w.items.get(i).and_then(|it| it.parent);
        if !visible(w, i) || up.is_some_and(|p| view_culls(w, p, i)) {
            return Some(i);
        }
        cur = up;
    }
    None
}

/// `QQuickItem::isTextureProvider`: images, canvases, ShaderEffectSources and layered items.
pub fn texture_provider(w: &mut World, i: usize) -> bool {
    [
        "Image",
        "AnimatedImage",
        "IconImage",
        "Canvas",
        "ShaderEffectSource",
    ]
    .iter()
    .any(|t| is(w, i, t))
        || truthy(w, i, "layer.enabled", false)
}

/// Items an effect, layer or ShaderEffectSource samples (drawn even when hidden).
pub fn sampled_items(w: &mut World) -> std::collections::HashSet<usize> {
    let mut s: std::collections::HashSet<usize> =
        DL_DEPS.with(|d| d.borrow().keys().copied().collect());
    for ty in EFFECT_ITEMS
        .iter()
        .chain(&["ShaderEffectSource", "ShaderEffect"])
    {
        for e in w.of_type(ty) {
            for k in ["source", "maskSource", "sourceItem"] {
                if let Some(Value::Item(x)) = w.read(e, k) {
                    s.insert(x);
                }
            }
        }
    }
    s
}

/// Invalidate cached layers of `touched` items and all their ancestors.
pub fn invalidate_layers(w: &World, touched: &[usize]) {
    if qml::log_on!("ri::touchdbg") {
        for &t in touched {
            if let Some(it) = w.items.get(t) {
                tracing::debug!(target: "ri::touchdbg", "touch {:?} in {}", it.types.first(), type_chain_dbg(w, t));
            }
        }
    }
    // A recorded draw goes stale with any change in its subtree, or above it (inherited state).
    dl_invalidate(w, &[], touched);
    stale_layers(w, touched);
}

/// `render_layer`, reused until the item or a descendant changes.
fn cached_layer(w: &mut World, fonts: &mut Fonts, i: usize) -> Layer {
    let size = (
        num(w, i, "width").round() as i64,
        num(w, i, "height").round() as i64,
    );
    let key = (i, LAYER_SKIP.with(|s| s.get()) == Some(i));
    let prev = LAYER_CACHE.with(|c| {
        c.borrow()
            .get(&key)
            .filter(|e| e.size == size)
            .map(|e| (e.stale, e.layer.clone()))
    });
    if let Some((false, l)) = prev {
        return l;
    }
    let t = render_layer(w, fonts, i);
    let layer = match prev {
        Some((_, l)) if l.px.w == t.w && l.px.px == t.px => l,
        _ => Layer::new(t),
    };
    let entry = LayerEntry {
        size,
        layer: layer.clone(),
        stale: false,
    };
    LAYER_CACHE.with(|c| c.borrow_mut().insert(key, entry));
    layer
}

/// GPU counterpart of `render_layer`: the subtree's draw list for an offscreen texture at the
/// window's devicePixelRatio. QQuickShaderEffectSource makes its texture item size * dpr, so
/// layered content (effect sources and masks) stays as sharp as the window.
fn gpu_layer_dpr(w: &mut World, fonts: &mut Fonts, i: usize, dpr: f64) -> crate::gpu::Frame {
    let (iw, ih) = (num(w, i, "width"), num(w, i, "height"));
    gpu_source(w, fonts, i, [0.0, 0.0, iw, ih], dpr)
}

/// The texture of a QQuickShaderEffectSource of item `i` with the sourceRect `rect` (x y w h
/// in the item's own coordinates) in a window of devicePixelRatio `dpr`: (rect size * dpr)
/// rounded once, at least 1 x 1 (`Frame::sized`), and QSGLayer renders the rect scaled onto it.
fn gpu_source(
    w: &mut World,
    fonts: &mut Fonts,
    i: usize,
    rect: [f64; 4],
    dpr: f64,
) -> crate::gpu::Frame {
    let mut f = crate::gpu::Frame::sized(rect[2], rect[3], dpr);
    if f.over.is_some() {
        f.over = Some(rect);
    }
    let Some(_sourcing) = sourcing(i) else {
        return f;
    };
    let (x, y) = (num(w, i, "x"), num(w, i, "y"));
    // A window's layer source keeps reusing its children's recorded draws, keyed by layer.
    let on = DL_ON.with(|d| d.get());
    let prev = LAYER_DL.with(|s| s.replace(on.then_some(i)));
    let (ox, oy) = (-rect[0] - x, -rect[1] - y);
    let _own = Untransformed::of(i);
    // A `Lift` is for the frame of its own layer: any other source is painted without one.
    let at = std::ptr::from_ref(&f) as usize;
    let own = LIFT.with_borrow_mut(|l| {
        let l = l.as_mut().filter(|l| l.root == i && l.frame == 0);
        l.map(|l| l.frame = at).is_some()
    });
    let outer = if own { None } else { LIFT.take() };
    paint(w, fonts, i, &mut Target::Gpu(&mut f), ox, oy, 1.0, true);
    if !own {
        LIFT.set(outer);
    }
    LAYER_DL.with(|s| s.set(prev));
    f
}

thread_local! {
    /// The item being painted as a source's texture, if any.
    static SOURCE_ROOT: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

/// Until this is dropped an item is painted without its own `rotation`, `scale` and
/// `opacity`: a source's texture holds the item in its own coordinates (QSGLayer renders from
/// the item's root node, which is below its transform node and its opacity node).
struct Untransformed([Option<usize>; 2]);

impl Untransformed {
    fn of(i: usize) -> Untransformed {
        Untransformed([
            XFORM_SKIP.with(|s| s.replace(Some(i))),
            SOURCE_ROOT.with(|s| s.replace(Some(i))),
        ])
    }
}

impl Drop for Untransformed {
    fn drop(&mut self) {
        XFORM_SKIP.with(|s| s.set(self.0[0]));
        SOURCE_ROOT.with(|s| s.set(self.0[1]));
    }
}

fn render_layer(w: &mut World, fonts: &mut Fonts, i: usize) -> Canvas {
    let (iw, ih) = (num(w, i, "width"), num(w, i, "height"));
    if !cpu_texture_fits(iw, ih) {
        return Canvas::new(1, 1, [0, 0, 0, 0]);
    }
    cpu_source(w, fonts, i, [0.0, 0.0, iw, ih])
}

/// `gpu_source` on the CPU raster, whose window has devicePixelRatio 1: the texture is the
/// rect's size rounded (QSizeF::toSize), at least 1 x 1, and the rect is drawn over all of it,
/// so content in a rect that is not a whole size is drawn through that scale.
fn cpu_source(w: &mut World, fonts: &mut Fonts, i: usize, rect: [f64; 4]) -> Canvas {
    let (tw, th) = qsg::layer::texture_size(rect[2], rect[3]);
    let mut out = Canvas::new(tw, th, [0, 0, 0, 0]);
    let Some(_sourcing) = sourcing(i) else {
        return out;
    };
    let (x, y) = (num(w, i, "x"), num(w, i, "y"));
    let (ox, oy) = (-rect[0] - x, -rect[1] - y);
    let _own = Untransformed::of(i);
    // A layer is a target of its own, with a projection of its own.
    let _size = CpuWindow::set(None);
    // `Frame::sized`: a rect under one unit is not stretched.
    let s = [tw as f64 / rect[2], th as f64 / rect[3]];
    if !(rect[2] >= 1.0 && rect[3] >= 1.0) || s == [1.0, 1.0] {
        paint(w, fonts, i, &mut Target::Cpu(&mut out), ox, oy, 1.0, true);
        return out;
    }
    let mut stage = Canvas::new(
        rect[2].ceil() as usize,
        rect[3].ceil() as usize,
        [0, 0, 0, 0],
    );
    let layer = ScaledCpu {
        stage: stage.key.get(),
        out,
        size: [rect[2], rect[3]],
        s,
        layer: true,
        clips: Vec::new(),
        dirty: false,
    };
    let prev = CPU_SCALED.with(|c| c.replace(Some(layer)));
    paint(w, fonts, i, &mut Target::Cpu(&mut stage), ox, oy, 1.0, true);
    let mut layer = CPU_SCALED
        .with(|c| c.replace(prev))
        .expect("the layer set above");
    layer.settle(&mut stage);
    layer.out
}

/// The subtree of item `i`, itself at `at`, over `size` on a texture of its own at the scale
/// `s` (x y): what `cpu_source` does for a layer's source. It is a turned item's layer, to be
/// drawn on the target `on`: with the texture come the pieces cut out of it (`df_open`).
/// `against` is the texture's axes that run against the screen's (`cpu_against`).
#[allow(clippy::too_many_arguments)]
fn cpu_subtree(
    w: &mut World,
    fonts: &mut Fonts,
    i: usize,
    at: [f64; 2],
    size: [f64; 2],
    s: [f64; 2],
    window: bool,
    against: [bool; 2],
    on: usize,
) -> (Canvas, Vec<DfPiece>) {
    let texels = |k: usize| ((size[k] * s[k]).ceil() as usize).max(1);
    let mut out = Canvas::new(texels(0), texels(1), [0, 0, 0, 0]);
    let _against = Against::new(&out, against);
    let _rows = window.then(|| qsg::raster::Window::new(&out));
    if s == [1.0, 1.0] {
        let cut = df_open(&raw const out as usize, on);
        paint(
            w,
            fonts,
            i,
            &mut Target::Cpu(&mut out),
            at[0],
            at[1],
            1.0,
            false,
        );
        return (out, if cut { df_close() } else { Vec::new() });
    }
    let size = [out.w as f64 / s[0], out.h as f64 / s[1]];
    let mut stage = Canvas::new(
        size[0].ceil() as usize,
        size[1].ceil() as usize,
        [0, 0, 0, 0],
    );
    let layer = ScaledCpu {
        stage: stage.key.get(),
        out,
        size,
        s,
        layer: false,
        clips: Vec::new(),
        dirty: false,
    };
    let prev = CPU_SCALED.with(|c| c.replace(Some(layer)));
    let cut = df_open(&raw const stage as usize, on);
    paint(
        w,
        fonts,
        i,
        &mut Target::Cpu(&mut stage),
        at[0],
        at[1],
        1.0,
        false,
    );
    let pieces = if cut { df_close() } else { Vec::new() };
    let mut layer = CPU_SCALED
        .with(|c| c.replace(prev))
        .expect("the layer set above");
    layer.settle(&mut stage);
    (layer.out, pieces)
}

/// A layer texture the CPU raster is painting whose size is not its source rect's (`cpu_source`).
/// Geometry, text, images, effects and clips go through the scale onto `out` (`cpu_direct`), as
/// QSGLayer's projection takes them. A draw that takes no scale (`cpu_plain`) lands 1:1 on the
/// stage canvas the painters are given, which is resampled onto `out` before the next scaled
/// draw so the stacking order holds.
struct ScaledCpu {
    /// `Canvas::key` of the stage: draws on any other canvas (a nested texture) are not scaled.
    stage: u64,
    out: Canvas,
    /// The source rect's size, which the stage is the ceiling of.
    size: [f64; 2],
    s: [f64; 2],
    /// It is a layer's texture (`cpu_source`), not that of a scaled subtree (`cpu_subtree`).
    layer: bool,
    /// The scissor stack in texels (x1 y1 x2 y2), each intersected with the one under it, with
    /// the texels outside it as they were when it was pushed (`save_outside`).
    clips: Vec<([i64; 4], Vec<[u8; 4]>)>,
    /// Whether the stage may hold a draw (`cpu_plain`).
    dirty: bool,
}

impl ScaledCpu {
    fn clip(&self) -> [i64; 4] {
        let all = [0, 0, self.out.w as i64, self.out.h as i64];
        self.clips.last().map_or(all, |c| c.0)
    }

    /// Scissor to `r` inside the current scissor until `pop_clip`.
    fn push_clip(&mut self, r: [i64; 4]) {
        let p = self.clip();
        let r = [
            r[0].max(p[0]),
            r[1].max(p[1]),
            r[2].min(p[2]),
            r[3].min(p[3]),
        ];
        let saved = save_outside(&self.out, r);
        self.clips.push((r, saved));
    }

    fn pop_clip(&mut self) {
        if let Some((r, saved)) = self.clips.pop() {
            restore_outside(&mut self.out, r, &saved);
        }
    }

    /// Resample what the stage holds onto the texture, when a draw landed on it.
    fn flush(&mut self, stage: &mut Canvas) {
        if self.dirty {
            self.settle(stage);
        }
    }

    /// `flush` without trusting `dirty`: the stage is read through.
    fn settle(&mut self, stage: &mut Canvas) {
        self.dirty = false;
        let drawn = |row: &[[u8; 4]]| row.iter().any(|p| *p != [0; 4]);
        let mut rows = stage.px.chunks(stage.w);
        let Some(top) = rows.position(drawn) else {
            return;
        };
        let bottom = top + rows.rposition(drawn).map_or(0, |k| k + 1);
        // The texels whose samples reach the rows drawn on: the others would add nothing.
        let mut clip = self.clip();
        let per_row = self.out.h as f64 / self.size[1];
        clip[1] = clip[1].max(((top as f64 - 1.0) * per_row).floor() as i64 - 1);
        clip[3] = clip[3].min(((bottom as f64 + 2.0) * per_row).ceil() as i64 + 1);
        qsg::layer::draw_fitted(&mut self.out, stage, self.size[0], self.size[1], clip);
        stage.px[top * stage.w..(bottom + 1) * stage.w].fill([0; 4]);
    }
}

thread_local! {
    static CPU_SCALED: std::cell::RefCell<Option<ScaledCpu>> = const { std::cell::RefCell::new(None) };
    /// The texture (`Canvas::key`) of a turned subtree being painted and its axes that run
    /// against the screen's (`qsg::text::snap_px`).
    static CPU_AGAINST: std::cell::Cell<(u64, [bool; 2])> = const { std::cell::Cell::new((0, [false; 2])) };
}

/// Until it is dropped, the axes of `cv` named run against the screen's.
struct Against((u64, [bool; 2]));

impl Against {
    fn new(cv: &Canvas, against: [bool; 2]) -> Self {
        Against(CPU_AGAINST.replace((cv.key.get(), against)))
    }
}

impl Drop for Against {
    fn drop(&mut self) {
        CPU_AGAINST.set(self.0);
    }
}

/// The axes of `c` that run against the screen's: `gpu::Frame::against`.
fn cpu_against(c: &Canvas) -> [bool; 2] {
    let (key, against) = CPU_AGAINST.get();
    if key == c.key.get() {
        against
    } else {
        [false; 2]
    }
}

/// Run `f` on the scaled layer `c` is the stage of, with what the stage holds flushed first;
/// None when `c` is any other canvas.
fn cpu_scaled<R>(c: &mut Canvas, f: impl FnOnce(&mut ScaledCpu) -> R) -> Option<R> {
    CPU_SCALED.with(|l| {
        let mut l = l.borrow_mut();
        let l = l.as_mut().filter(|l| l.stage == c.key.get())?;
        l.flush(c);
        Some(f(l))
    })
}

/// `c` for a draw that takes no scale: on the stage of a scaled layer it is resampled onto the
/// texture before the next scaled draw. Every draw on a painter's canvas goes through this or
/// `cpu_direct`.
fn cpu_plain(c: &mut Canvas) -> &mut Canvas {
    CPU_SCALED.with(|l| {
        if let Some(l) = l.borrow_mut().as_mut().filter(|l| l.stage == c.key.get()) {
            l.dirty = true;
        }
    });
    c
}

/// Run `f`, which draws quads through a scale (x y), on the CPU raster: on the texture of the
/// scaled layer `c` is the stage of with that layer's scale, else on `c` itself with 1. `f` must
/// not paint items.
fn cpu_direct(c: &mut Canvas, f: impl FnOnce(&mut Canvas, [f64; 2])) {
    let mut f = Some(f);
    let scaled = cpu_scaled(c, |l| f.take().expect("taken once")(&mut l.out, l.s));
    if scaled.is_none() {
        f.take().expect("not taken")(c, [1.0, 1.0]);
    }
}

/// NativeRendering glyphs on the CPU raster: `gpu_glyphs`.
fn cpu_line(
    c: &mut Canvas,
    f: &FontStack,
    line: &qsg::text::ShapedLine,
    t: [f64; 2],
    at: [f64; 2],
    col: [u8; 4],
    op: f32,
) {
    cpu_direct(c, |c, s| {
        let against = cpu_against(c);
        if s == [1.0, 1.0] && against == [false; 2] {
            f.draw_line(c, line, t, at, col, op)
        } else {
            f.draw_line_scaled(c, line, t, at, col, op, s, against)
        }
    });
}

/// An image on the CPU raster: `Frame::image`.
#[allow(clippy::too_many_arguments)]
fn cpu_image(
    c: &mut Canvas,
    tex: &Canvas,
    [x, y, w, h]: [f64; 4],
    fill: qsg::image::FillMode,
    smooth: bool,
    op: f32,
) {
    cpu_direct(c, |c, s| {
        qsg::image::draw_scaled(c, tex, x, y, w, h, fill, smooth, op, s)
    });
}

/// Rectangle geometry on the CPU raster: `Frame::smooth`.
fn cpu_smooth(c: &mut Canvas, g: &qsg::rect::Geometry, x: f64, y: f64, op: f32) {
    let place = cpu_place(c, x, y);
    let scaled = cpu_scaled(c, |l| {
        let (s, clip) = (l.s.map(|v| v as f32), l.clip());
        match place {
            Some(p) if s == [1.0, 1.0] => l.out.draw_smooth_placed(g, &p, op, clip),
            _ => l.out.draw_smooth_scaled(g, s, x as f32, y as f32, op, clip),
        }
    });
    if scaled.is_none() {
        match place {
            Some(p) => c.draw_smooth_placed(g, &p, op, [0, 0, c.w as i64, c.h as i64]),
            None => c.draw_smooth(g, x as f32, y as f32, op),
        }
    }
}

/// Rectangle geometry through the matrix `m` on the CPU raster: `Frame::smooth_matrix`.
fn cpu_smooth_matrix(c: &mut Canvas, g: &qsg::rect::Geometry, m: [f32; 6], op: f32) {
    let scaled = cpu_scaled(c, |l| {
        let (s, clip) = (l.s.map(|v| v as f32), l.clip());
        let m = [0, 1, 2, 3, 4, 5].map(|k| s[k % 2] * m[k]);
        l.out.draw_smooth_matrix(g, m, op, clip);
    });
    if scaled.is_none() {
        let all = [0, 0, c.w as i64, c.h as i64];
        c.draw_smooth_matrix(g, m, op, all);
    }
}

/// `draw` under QSG's scissor of a `clip: true` item's scene rect `r` (x y w h) on the CPU
/// raster: `Frame::push_clip_f` and `pop_clip`.
fn cpu_clipped(cv: &mut Target, r: [f64; 4], draw: impl FnOnce(&mut Target)) {
    let Target::Cpu(c) = cv else {
        return;
    };
    let (x, y, wd, ht) = (r[0] as f32, r[1] as f32, r[2].max(0.0), r[3].max(0.0));
    let pushed = cpu_scaled(c, |l| {
        let scene = l.size.map(|v| v as f32);
        let r = qsg::raster::scissor_rect_scaled(x, y, wd, ht, scene, l.out.w, l.out.h);
        l.push_clip(r);
    });
    if pushed.is_some() {
        draw(cv);
        if let Target::Cpu(c) = cv {
            cpu_scaled(c, |l| l.pop_clip());
        }
        return;
    }
    let r = qsg::raster::scissor_rect(x, y, wd, ht, c.w, c.h);
    let saved = save_outside(c, r);
    draw(cv);
    if let Target::Cpu(c) = cv {
        restore_outside(c, r, &saved);
    }
}

/// QQuickPathPolyline::addToPath: fewer than two points add nothing.
fn polyline(w: &mut World, e: usize) -> Vec<(f64, f64)> {
    let Some(Value::Obj(o)) = w.read(e, "path") else {
        return Vec::new();
    };
    let coord = |v: &Value, k: &str| match v {
        Value::Obj(p) => p.borrow().props.get(k).map_or(0.0, |x| x.to_number()),
        _ => 0.0,
    };
    let pts: Vec<_> = (o.borrow().array.iter().flatten())
        .map(|v| (coord(v, "x"), coord(v, "y")))
        .collect();
    if pts.len() < 2 { Vec::new() } else { pts }
}

/// QQuickShape implicit size: bottom-right of the paths' bounding rect.
fn shape_bounds(w: &mut World, i: usize) -> (f64, f64) {
    let (mut mx, mut my) = (0.0f64, 0.0f64);
    for sp in w.items[i].children.clone() {
        if !is(w, sp, "ShapePath") || w.dead.contains(&sp) {
            continue;
        }
        let mut pts = vec![(num(w, sp, "startX"), num(w, sp, "startY"))];
        for e in w.items[sp].children.clone() {
            if is(w, e, "PathAngleArc") {
                let (cx, cy) = (num(w, e, "centerX"), num(w, e, "centerY"));
                let (rx, ry) = (num(w, e, "radiusX"), num(w, e, "radiusY"));
                let (st, sw) = (num_nan(w, e, "startAngle"), num_nan(w, e, "sweepAngle"));
                // QPainterPath::arcTo ignores an arc with invalid angles.
                if !(st.is_finite() && sw.is_finite()) {
                    continue;
                }
                for k in 0..=64 {
                    let a = (st + sw * k as f64 / 64.0).to_radians();
                    pts.push((cx + rx * a.cos(), cy + ry * a.sin()));
                }
            } else if is(w, e, "PathLine") || is(w, e, "PathMove") {
                pts.push((num(w, e, "x"), num(w, e, "y")));
            } else if is(w, e, "PathPolyline") {
                pts.extend(polyline(w, e));
            }
        }
        for (x, y) in pts {
            mx = mx.max(x);
            my = my.max(y);
        }
    }
    ((mx * 1e6).round() / 1e6, (my * 1e6).round() / 1e6)
}

/// Replay the recorded Context2D ops of a `Canvas` (renderTarget Image) at item size.
#[allow(clippy::too_many_arguments)]
fn paint_canvas(
    w: &mut World,
    i: usize,
    cv: &mut Canvas,
    x: f64,
    y: f64,
    wd: f64,
    ht: f64,
    op: f32,
) {
    let ops = match w.canvas_ops.get(&i) {
        Some(o) if !o.is_empty() => o.clone(),
        _ => return,
    };
    let (cw, ch) = (wd.ceil().max(1.0) as usize, ht.ceil().max(1.0) as usize);
    // The replay only depends on the ops and the canvas size: layers re-rendered around an
    // unchanged Canvas (a DropShadow source) reuse it instead of re-rasterizing every path.
    let key = (w.canvas_gen.get(&i).copied().unwrap_or(0), cw, ch);
    let hit = REPLAYS.with(|r| {
        r.borrow_mut()
            .get(&i)
            .filter(|e| e.0 == key)
            .map(|e| e.1.clone())
    });
    if let Some(img) = hit {
        qsg::layer::draw(cv, &img, x, y, cw as f64, ch as f64, op);
        return;
    }
    let img = Rc::new(crate::canvas2d::replay(&ops, cw, ch));
    let n = canvas_bytes(&img);
    REPLAYS.with(|r| r.borrow_mut().insert(i, (key, img.clone()), n));
    qsg::layer::draw(cv, &img, x, y, cw as f64, ch as f64, op);
}

/// One ShapePath as the renderer draws it; `fill` is derived from `start` and `cmds`.
#[derive(Debug)]
struct ShapePathSpec {
    start: (f64, f64),
    cmds: Vec<qsg::shape::PathCmd>,
    fc: [u8; 4],
    even_odd: bool,
    stroke: Option<qsg::shape::Stroke>,
}

/// Everything a Shape's pixels depend on besides its size, offset and opacity.
fn shape_spec(w: &mut World, i: usize) -> (bool, Vec<(ShapePathSpec, qsg::canvas::Path)>) {
    use qsg::shape::{Cap, Join, PathCmd, Stroke};
    // Shape.CurveRenderer: fills take the curve renderer's analytic coverage.
    let curve = num(w, i, "preferredRendererType") == 4.0;
    let mut out = Vec::new();
    for sp in w.items[i].children.clone() {
        if !is(w, sp, "ShapePath") || w.dead.contains(&sp) {
            continue;
        }
        let start = (num(w, sp, "startX"), num(w, sp, "startY"));
        let mut cmds = Vec::new();
        let mut fill = qsg::canvas::Path::default();
        fill.move_to(start.0, start.1);
        for e in w.items[sp].children.clone() {
            if is(w, e, "PathAngleArc") {
                let (cx, cy) = (num(w, e, "centerX"), num(w, e, "centerY"));
                let (rx, ry) = (num(w, e, "radiusX"), num(w, e, "radiusY"));
                // NaN stays NaN (e.g. `position / length` with no player): Qt then drops the arc.
                let (st, sw) = (num_nan(w, e, "startAngle"), num_nan(w, e, "sweepAngle"));
                // QQuickPathAngleArc::moveToStart defaults to true.
                let move_to_start =
                    !explicit(w, e, "moveToStart") || truthy(w, e, "moveToStart", true);
                cmds.push(PathCmd::AngleArc {
                    cx,
                    cy,
                    rx,
                    ry,
                    start: st,
                    sweep: sw,
                    move_to_start,
                });
                let (a0, a1) = (st.to_radians(), (st + sw).to_radians());
                if move_to_start && a0.is_finite() {
                    fill.move_to(cx + rx * a0.cos(), cy + ry * a0.sin());
                }
                fill.arc(cx, cy, rx, a0, a1, sw < 0.0);
            } else if is(w, e, "PathLine") {
                let (px, py) = (num(w, e, "x"), num(w, e, "y"));
                cmds.push(PathCmd::Line(px, py));
                fill.line_to(px, py);
            } else if is(w, e, "PathMove") {
                let (px, py) = (num(w, e, "x"), num(w, e, "y"));
                cmds.push(PathCmd::Move(px, py));
                fill.move_to(px, py);
            } else if is(w, e, "PathPolyline") {
                for (k, (px, py)) in polyline(w, e).into_iter().enumerate() {
                    if k == 0 {
                        cmds.push(PathCmd::Move(px, py));
                        fill.move_to(px, py);
                    } else {
                        cmds.push(PathCmd::Line(px, py));
                        fill.line_to(px, py);
                    }
                }
            }
        }
        let fc = color(w, sp, "fillColor", [255, 255, 255, 255]);
        let even_odd = num(w, sp, "fillRule") == 0.0;
        let sc = color(w, sp, "strokeColor", [255, 255, 255, 255]);
        let sw = opt_num(w, sp, "strokeWidth").unwrap_or(1.0);
        let stroke = (sc[3] > 0 && sw > 0.0).then(|| {
            let cap = match opt_num(w, sp, "capStyle").unwrap_or(16.0) as u32 {
                0 => Cap::Flat,
                32 => Cap::Round,
                _ => Cap::Square,
            };
            let join = match opt_num(w, sp, "joinStyle").unwrap_or(64.0) as u32 {
                0 => Join::Miter,
                128 => Join::Round,
                _ => Join::Bevel,
            };
            Stroke {
                width: sw as f32,
                color: sc,
                cap,
                join,
                miter_limit: opt_num(w, sp, "miterLimit").unwrap_or(2.0) as f32,
            }
        });
        out.push((
            ShapePathSpec {
                start,
                cmds,
                fc,
                even_odd,
                stroke,
            },
            fill,
        ));
    }
    (curve, out)
}

fn shape_key(spec: &(bool, Vec<(ShapePathSpec, qsg::canvas::Path)>), extra: &[f64]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    spec.0.hash(&mut h);
    spec.1.len().hash(&mut h);
    for (p, _) in &spec.1 {
        p.hash(&mut h);
    }
    for v in extra {
        v.to_bits().hash(&mut h);
    }
    h.finish()
}

impl std::hash::Hash for ShapePathSpec {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        use qsg::shape::PathCmd;
        (self.start.0.to_bits(), self.start.1.to_bits()).hash(h);
        self.cmds.len().hash(h);
        for c in &self.cmds {
            match c {
                PathCmd::Move(x, y) => (0u8, x.to_bits(), y.to_bits()).hash(h),
                PathCmd::Line(x, y) => (1u8, x.to_bits(), y.to_bits()).hash(h),
                PathCmd::AngleArc {
                    cx,
                    cy,
                    rx,
                    ry,
                    start,
                    sweep,
                    move_to_start,
                } => (
                    2u8,
                    [cx, cy, rx, ry, start, sweep].map(|v| v.to_bits()),
                    move_to_start,
                )
                    .hash(h),
            }
        }
        (self.fc, self.even_odd).hash(h);
        match &self.stroke {
            None => 0u8.hash(h),
            Some(s) => (
                1u8,
                s.width.to_bits(),
                s.color,
                s.cap as u8,
                s.join as u8,
                s.miter_limit.to_bits(),
            )
                .hash(h),
        }
    }
}

/// Paints `spec` into `cv` with the item origin at (x, y) and its geometry scaled by (sx, sy):
/// the shape's canvas pixels per item unit (the device scale under a fractional output scale).
#[allow(clippy::too_many_arguments)]
fn paint_shape(
    spec: &(bool, Vec<(ShapePathSpec, qsg::canvas::Path)>),
    cv: &mut Canvas,
    x: f64,
    y: f64,
    wd: f64,
    ht: f64,
    op: f32,
    sx: f64,
    sy: f64,
    (origin, in_texture, frame): ((i64, i64), bool, Option<&qsg::shape::FillFrame>),
) {
    use qsg::shape;
    let curve = spec.0;
    let unit = sx == 1.0 && sy == 1.0;
    // The fill covers the item; of an item too large to rasterize, the part that reaches `cv`.
    let (mut fw, mut fh) = ((wd * sx).ceil().max(1.0), (ht * sy).ceil().max(1.0));
    if !cpu_texture_fits(fw, fh) {
        fw = fw.min((cv.w as f64 - x).ceil().max(1.0));
        fh = fh.min((cv.h as f64 - y).ceil().max(1.0));
    }
    let fills = cpu_texture_fits(fw, fh);
    let (cw, ch) = (fw as usize, fh as usize);
    for (sp, fill) in &spec.1 {
        let (fc, even_odd) = (sp.fc, sp.even_odd);
        let scaled_cmds;
        let (start, cmds) = if unit {
            (sp.start, &sp.cmds[..])
        } else {
            scaled_cmds = scale_cmds(&sp.cmds, sx, sy);
            ((sp.start.0 * sx, sp.start.1 * sy), &scaled_cmds[..])
        };
        if fills && fc[3] > 0 && curve {
            // Qt triangulates the path as it is and scales with the matrix.
            let tris = shape::fill_tris(&sp.cmds, sp.start, even_odd, 0);
            let (scale, at) = ([sx as f32, sy as f32], [x as f32, y as f32]);
            shape::draw_fill(cv, &tris, fc, scale, at, frame, op, origin, in_texture);
        } else if fills && fc[3] > 0 {
            let scaled_fill;
            let fill = if unit {
                fill
            } else {
                scaled_fill = fill.mapped([sx, 0.0, 0.0, sy, 0.0, 0.0]);
                &scaled_fill
            };
            let mut ctx = qsg::canvas::Context::new(cw, ch);
            ctx.fill(fill, fc, even_odd);
            qsg::layer::draw(cv, &ctx.img, x, y, ctx.img.w as f64, ctx.img.h as f64, op);
        }
        if let Some(st) = &sp.stroke {
            let scaled_stroke;
            let st = if unit {
                st
            } else {
                scaled_stroke = shape::Stroke {
                    width: (st.width as f64 * sx) as f32,
                    ..st.clone()
                };
                &scaled_stroke
            };
            let el = shape::stroke_elements(cmds, start);
            let tris = shape::stroke_tris(&el, st);
            shape::draw_stroke(cv, &tris, st, x as f32, y as f32, op);
        }
    }
}

/// `cmds` with every coordinate and radius scaled by (sx, sy); arc angles are parametric and
/// stay.
fn scale_cmds(cmds: &[qsg::shape::PathCmd], sx: f64, sy: f64) -> Vec<qsg::shape::PathCmd> {
    use qsg::shape::PathCmd;
    cmds.iter()
        .map(|c| match *c {
            PathCmd::Move(x, y) => PathCmd::Move(x * sx, y * sy),
            PathCmd::Line(x, y) => PathCmd::Line(x * sx, y * sy),
            PathCmd::AngleArc {
                cx,
                cy,
                rx,
                ry,
                start,
                sweep,
                move_to_start,
            } => PathCmd::AngleArc {
                cx: cx * sx,
                cy: cy * sy,
                rx: rx * sx,
                ry: ry * sy,
                start,
                sweep,
                move_to_start,
            },
        })
        .collect()
}

/// Windows (PanelWindow etc.) with the given layer namespace.
/// `PopupAnchor.adjustment` as xdg_positioner constraint_adjustment bits. Quickshell's default
/// is `PopupAdjustment.Slide` (no flip), and an explicit `PopupAdjustment.None` (0) is kept.
pub fn popup_adjustment(v: Option<Value>) -> u32 {
    match v {
        Some(Value::Num(v)) if v.is_finite() && v >= 0.0 => v as u32 & 63,
        _ => 3,
    }
}

pub fn find_windows(rt: &Runtime, namespace: &str) -> Vec<usize> {
    let mut w = rt.world.borrow_mut();
    let cands: Vec<usize> = w
        .items
        .ids()
        .filter(|&i| !w.dead.contains(&i) && is(&w, i, "PanelWindow"))
        .collect();
    cands
        .into_iter()
        .filter(|&i| {
            matches!(w.read(i, "WlrLayershell.namespace"), Some(Value::Str(s)) if &*s == namespace)
        })
        .collect()
}

/// Input rectangles of a window's `mask: Region { item; regions }` in window coordinates
/// (Quickshell PendingRegion); `None` when the window has no mask.
pub fn mask_rects(rt: &mut Runtime, win: usize) -> Option<Vec<[i32; 4]>> {
    let mut w = rt.world.borrow_mut();
    let Some(Value::Item(region)) = w.read(win, "mask") else {
        return None;
    };
    let mut out = Vec::new();
    let mut stack = vec![region];
    while let Some(r) = stack.pop() {
        match w.read(r, "item") {
            Some(Value::Item(it)) => {
                let (x, y) = w.scene_pos(it, Some(win));
                let (iw, ih) = (num(&mut w, it, "width"), num(&mut w, it, "height"));
                if iw > 0.0 && ih > 0.0 {
                    out.push([
                        x.floor() as i32,
                        y.floor() as i32,
                        (x + iw).ceil() as i32 - x.floor() as i32,
                        (y + ih).ceil() as i32 - y.floor() as i32,
                    ]);
                }
            }
            _ => {
                let (rw, rh) = (num(&mut w, r, "width"), num(&mut w, r, "height"));
                if rw > 0.0 && rh > 0.0 {
                    out.push([
                        num(&mut w, r, "x") as i32,
                        num(&mut w, r, "y") as i32,
                        rw.ceil() as i32,
                        rh.ceil() as i32,
                    ]);
                }
            }
        }
        if let Some(Value::Obj(o)) = w.read(r, "regions")
            && let Some(a) = o.borrow().array.as_ref()
        {
            stack.extend(a.iter().filter_map(|v| match v {
                Value::Item(i) => Some(*i),
                _ => None,
            }));
        }
        let kids: Vec<usize> = w.items[r].children.clone();
        stack.extend(
            kids.into_iter()
                .filter(|&c| w.items[c].types.iter().any(|t| t == "Region")),
        );
    }
    Some(out)
}

/// Largest char boundary of `s` at or below byte `n`.
fn floor_boundary(s: &str, n: usize) -> usize {
    (0..=n.min(s.len()))
        .rev()
        .find(|&k| s.is_char_boundary(k))
        .unwrap_or(0)
}

fn type_chain_dbg(w: &World, i: usize) -> String {
    let mut out = Vec::new();
    let mut p = w.items[i].parent;
    while let Some(q) = p.filter(|_| out.len() < 6) {
        out.push(w.items[q].types.first().map_or("?", |t| t.as_str()));
        p = w.items[q].parent;
    }
    out.join("<")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Material placeholder floats for 300 ms when the field's focus changes (an animation:
    /// its position is a function of the time since) and jumps when only the text changes.
    #[test]
    fn the_material_placeholder_floats_over_time_only_when_focus_changes() {
        let field = usize::MAX - 7;
        take_animating();
        assert_eq!(mat_progress(field, false, false), 0.0);
        assert!(!take_animating(), "a field first seen is where it belongs");
        let p = mat_progress(field, true, true);
        assert!((0.0..1.0).contains(&p), "focused: floating, at {p}");
        assert!(take_animating(), "asks for the next frame while it floats");
        assert_eq!(
            mat_progress(field, false, true),
            0.0,
            "text cleared: at once"
        );
        assert!(!take_animating());
        MAT_ANIM.with(|m| m.borrow_mut().remove(&field));
    }

    #[test]
    fn panicking_decode_reports_failure() {
        assert_eq!(guarded::<u8>(|| panic!("bad file")), None);
        assert_eq!(guarded(|| Some(7)), Some(7));
    }

    #[test]
    fn decode_pool_survives_panicking_jobs() {
        // More panics than workers: before, each killed its worker and later decodes never ran.
        for _ in 0..4 {
            submit_decode(Box::new(|| panic!("decoder bug")));
        }
        let (tx, rx) = std::sync::mpsc::channel();
        for k in 0..4 {
            let tx = tx.clone();
            submit_decode(Box::new(move || {
                let _ = tx.send(k);
            }));
        }
        let mut got: Vec<i32> = (0..4).map(|_| rx.recv().expect("decode ran")).collect();
        got.sort();
        assert_eq!(got, [0, 1, 2, 3]);
    }

    #[test]
    fn animated_font_sizes_do_not_keep_every_stack() {
        let mut fonts = Fonts::default();
        let held = fonts.get("sans-serif", 13, 400);
        // A label scaling through 8..208 px, then idling while other text lays out.
        for px in 8..208 {
            fonts.get("sans-serif", px, 400);
        }
        for _ in 0..FONTS_IDLE {
            fonts.get("sans-serif", 13, 400);
        }
        for px in 300..310 {
            fonts.get("sans-serif", px, 400);
        }
        assert!(
            fonts.cache.len() <= FONTS_KEPT,
            "{} stacks",
            fonts.cache.len()
        );
        let again = fonts.get("sans-serif", 13, 400);
        assert!(match (&held, &again) {
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        });
        // Stacks still in use by the lookups of the last FONTS_IDLE stay.
        assert!(
            fonts
                .cache
                .contains_key(&(["sans-serif", "", "0"].join("\0"), 309, 400))
        );
    }

    fn canvas(w: usize, h: usize) -> Rc<Canvas> {
        Rc::new(Canvas::new(w, h, [1, 2, 3, 255]))
    }

    #[test]
    fn tray_overlays_cover_the_whole_icon() {
        use qml::sni::{Layer, TrayIcon};
        let (red, blue) = ([255, 0, 0, 255], [0, 0, 255, 255]);
        let solid = |c: [u8; 4], n: usize| Layer::Pixels(n, n, vec![c; n * n]);
        // Blue in its right half, nothing in its left.
        let half = |n: usize| (0..n * n).map(move |i| if i % n < n / 2 { [0; 4] } else { blue });
        let at = |c: &Canvas, x: usize, y: usize| c.px[y * c.w + x];
        let never = || || panic!("nothing is pending");

        // Pixmaps asked for at their own size.
        let c = compose_tray(
            TrayIcon {
                base: Some(solid(red, 4)),
                overlay: Some(Layer::Pixels(4, 4, half(4).collect())),
            },
            (4, 4),
            never,
        )
        .unwrap();
        assert_eq!((c.w, c.h), (4, 4));
        assert_eq!([at(&c, 0, 0), at(&c, 1, 1), at(&c, 1, 3)], [red; 3]);
        assert_eq!([at(&c, 2, 0), at(&c, 2, 2), at(&c, 3, 3)], [blue; 3]);

        // Names from the item's own theme directory, drawn at the requested size.
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path().display();
        Canvas::new(16, 16, red)
            .save_png(&format!("{d}/tray-base.png"))
            .unwrap();
        Canvas::from_px(16, 16, half(16).collect())
            .save_png(&format!("{d}/tray-emblem.png"))
            .unwrap();
        let c = compose_tray(
            TrayIcon {
                base: Some(Layer::Icon(format!("image://icon/tray-base?path={d}"))),
                overlay: Some(Layer::Icon(format!("image://icon/tray-emblem?path={d}"))),
            },
            (8, 8),
            never,
        )
        .unwrap();
        assert_eq!((c.w, c.h), (8, 8));
        assert_eq!([at(&c, 0, 0), at(&c, 2, 3), at(&c, 2, 7)], [red; 3]);
        assert_eq!([at(&c, 5, 0), at(&c, 5, 4), at(&c, 7, 7)], [blue; 3]);

        // A half-transparent overlay blends; one that cannot be found is left out.
        let c = compose_tray(
            TrayIcon {
                base: Some(solid(red, 2)),
                overlay: Some(solid([0, 0, 255, 128], 2)),
            },
            (2, 2),
            never,
        )
        .unwrap();
        assert_eq!(c.px, vec![[127, 0, 128, 255]; 4]);
        // Asked for at no size, a pixmap is 100x100 like a named icon.
        let c = compose_tray(
            TrayIcon {
                base: Some(solid(red, 2)),
                overlay: None,
            },
            (0, 0),
            never,
        )
        .unwrap();
        assert_eq!((c.w, c.h, at(&c, 99, 99)), (100, 100, red));
        // One the item's theme does not have is waited for while its tree is searched, then
        // left out.
        let missing = || TrayIcon {
            base: Some(solid(red, 2)),
            overlay: Some(Layer::Icon(format!("image://icon/tray-none?path={d}"))),
        };
        let (tx, searched) = std::sync::mpsc::channel();
        let ready = move || {
            let tx = tx.clone();
            move || {
                let _ = tx.send(());
            }
        };
        if compose_tray(missing(), (2, 2), ready).is_none() {
            searched.recv().unwrap();
        }
        let c = compose_tray(missing(), (2, 2), never).unwrap();
        assert_eq!(c.px, vec![red; 4]);
    }

    #[test]
    fn unresolved_theme_icon_is_not_the_missing_texture() {
        for ss in [(20, 20), (0, 0), (33, 17)] {
            let c = unresolved_icon(ss);
            assert!(c.w > 0 && c.h > 0);
            assert!(!c.px.contains(&[0xd9, 0x00, 0xd8, 255]), "{ss:?}");
        }
    }

    #[test]
    fn scissor_restore_matches_full_snapshot() {
        let (w, h) = (37usize, 23usize);
        let base = Canvas {
            w,
            h,
            px: (0..w * h)
                .map(|k| [k as u8, (k >> 8) as u8, 7, 255])
                .collect(),
            key: Default::default(),
        };
        let rects = [
            [0, 0, 37, 23],
            [5, 3, 20, 11],
            [-4, -2, 50, 40],
            [30, 20, 60, 60],
            [10, 10, 5, 5],
            [0, 5, 37, 6],
            [36, 0, 37, 23],
            [40, 30, 50, 40],
        ];
        for r in rects {
            let mut a = base.clone();
            let mut b = base.clone();
            let snap = a.snapshot();
            let saved = save_outside(&b, r);
            for p in a.px.iter_mut().chain(b.px.iter_mut()) {
                *p = [9, 9, 9, 9];
            }
            a.apply_scissor(&snap, r);
            restore_outside(&mut b, r, &saved);
            assert!(a.px == b.px, "{r:?}");
        }
        // A 1920x1080 frame: whole-canvas snapshot + per-pixel restore vs. saving and
        // restoring only the runs outside the clip.
        let mut c = Canvas::new(1920, 1080, [1, 2, 3, 255]);
        let n = 200;
        for (name, r) in [
            ("full", [0, 0, 1920, 1080]),
            ("list", [200, 150, 1200, 900]),
            ("icon", [900, 500, 948, 548]),
        ] {
            for _round in 0..2 {
                let t = std::time::Instant::now();
                for _ in 0..n {
                    let s = c.snapshot();
                    c.apply_scissor(&s, r);
                    std::hint::black_box(&c);
                }
                let old = t.elapsed() / n;
                let t = std::time::Instant::now();
                for _ in 0..n {
                    let s = save_outside(&c, r);
                    restore_outside(&mut c, r, &s);
                    std::hint::black_box(&c);
                }
                let new = t.elapsed() / n;
                elog::eprintln!(
                    "scissor 1920x1080 {name} clip {r:?}: snapshot+apply {old:?}, runs {new:?}"
                );
            }
        }
    }

    #[test]
    fn forget_items_keeps_draws_unless_an_effect_survives_its_source() {
        DL_HOT.with_borrow_mut(|h| h.insert(900_001, 1));
        DL_DEPS.with_borrow_mut(|d| d.insert(900_010, vec![900_011]));
        forget_items(&[900_010, 900_011]);
        assert!(DL_HOT.with_borrow(|h| h.contains_key(&900_001)));
        assert!(!DL_DEPS.with_borrow(|d| d.contains_key(&900_010)));
        DL_DEPS.with_borrow_mut(|d| d.insert(900_020, vec![900_021]));
        forget_items(&[900_020]);
        assert!(!DL_HOT.with_borrow(|h| h.contains_key(&900_001)));
    }

    #[test]
    fn net_cache_prune_drops_oldest_past_budget() {
        let dir = std::env::temp_dir().join(format!("ri-net-prune-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let now = std::time::SystemTime::now();
        for k in 0..5u64 {
            let f = std::fs::File::create(dir.join(format!("f{k}"))).unwrap();
            f.set_len(100).unwrap();
            f.set_modified(now - std::time::Duration::from_secs(1000 - k * 10))
                .unwrap();
        }
        prune_net_cache(&dir, 250);
        let mut left: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().into_string().unwrap())
            .collect();
        left.sort();
        assert_eq!(left, ["f3", "f4"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn forget_items_drops_per_item_state_and_rasters() {
        let (gone, kept) = (1_000_001usize, 1_000_002usize);
        for i in [gone, kept] {
            MAT_LH.with_borrow_mut(|m| m.insert(i, 18.0));
            SWIPE_POS.with_borrow_mut(|m| m.insert(i, 40.0));
            FRESH_CLEAN.with_borrow_mut(|m| m.insert(i, 7));
            assert!(set_screencopy_size(i, (64, 48)));
            KEYED.with_borrow_mut(|m| m.insert(i, ((0, 0, 0, 4, 4), Some(canvas(4, 4))), 64));
            CANVASES.with_borrow_mut(|m| m.insert(i, ((0, 4, 4, 0, 0), Some(canvas(4, 4))), 64));
            REPLAYS.with_borrow_mut(|m| m.insert(i, ((0, 4, 4), canvas(4, 4)), 64));
        }
        forget_items(&[gone]);
        let has = |i: usize| {
            [
                MAT_LH.with_borrow(|m| m.contains_key(&i)),
                SWIPE_POS.with_borrow(|m| m.contains_key(&i)),
                FRESH_CLEAN.with_borrow(|m| m.contains_key(&i)),
                screencopy_size(i).is_some(),
                KEYED.with_borrow(|m| m.contains(&i)),
                CANVASES.with_borrow(|m| m.contains(&i)),
                REPLAYS.with_borrow(|m| m.contains(&i)),
            ]
        };
        assert_eq!(has(gone), [false; 7], "destroyed item's entries freed");
        assert_eq!(has(kept), [true; 7], "live item's entries kept");
        assert_eq!(KEYED.with_borrow(|m| m.bytes()), 64);
        forget_items(&[kept]);
        assert_eq!(has(kept), [false; 7]);
        assert_eq!(KEYED.with_borrow(|m| m.bytes()), 0);
    }

    /// qml/modules/rusty/sidebarLeft/aiChat/MessageCodeBlock.qml codeTextArea: the
    /// SyntaxHighlighter's spans become the TextEdit's coloured runs.
    #[cfg(feature = "interp")]
    #[test]
    fn syntax_highlighter_spans_colour_text_edit_runs() {
        let mut rt = qml::runtime::Runtime::new(None);
        let root = rt
            .load_str("import QtQuick\nTextEdit { text: \"return 1\" }\n")
            .unwrap();
        rt.settle();
        let w = &mut rt.world.borrow_mut();
        let spans = qml::syntax::highlight("return 1", "python", "Monokai");
        w.syntax.results.insert(
            root,
            std::sync::Arc::new(qml::syntax::Highlight {
                text: "return 1".into(),
                spans,
            }),
        );
        let ts = text_style(w, &mut Fonts::default(), root);
        let runs = ts.spans.expect("highlighted runs");
        assert_eq!(
            runs.iter().map(|r| r.0.as_str()).collect::<String>(),
            "return 1"
        );
        let kw = runs.iter().find(|r| r.0 == "return").expect("keyword run");
        assert_eq!(kw.2.0, Some([0xf9, 0x26, 0x72, 255]));
    }

    #[cfg(feature = "interp")]
    #[test]
    fn transparent_border_does_not_inset_the_rectangle_fill() {
        let mut rt = qml::runtime::Runtime::new(None);
        let root = rt
            .load_str(
                r##"import QtQuick
Item {
    width: 60; height: 20
    property Item clear: Rectangle { width: 20; height: 20; color: "#ff0000"; border.width: 2; border.color: "transparent" }
    property Item thin: Rectangle { width: 20; height: 20; color: "#ff0000"; border.width: 0.4; border.color: "#0000ff" }
    property Item solid: Rectangle { width: 20; height: 20; color: "#ff0000"; border.width: 2; border.color: "#0000ff" }
}"##,
            )
            .unwrap();
        rt.settle();
        let mut fonts = super::Fonts::default();
        let edge = |rt: &qml::runtime::Runtime, fonts: &mut super::Fonts, k: &str| {
            let qml::value::Value::Item(i) = rt.get(root, k) else {
                panic!("{k}");
            };
            let mut c = Canvas::new(20, 20, [0, 0, 0, 0]);
            let mut cv = Target::Cpu(&mut c);
            let w = &mut rt.world.borrow_mut();
            paint_body(w, fonts, i, &mut cv, 0.0, 0.0, 20.0, 20.0, 1.0);
            (c.px[20 + 10], c.px[10 * 20 + 10])
        };
        let red = [255, 0, 0, 255];
        assert_eq!(
            edge(&rt, &mut fonts, "clear"),
            (red, red),
            "transparent pen: full fill"
        );
        assert_eq!(
            edge(&rt, &mut fonts, "thin"),
            (red, red),
            "width rounding to 0: no pen"
        );
        assert_eq!(
            edge(&rt, &mut fonts, "solid"),
            ([0, 0, 255, 255], red),
            "opaque pen: border over the inset fill"
        );
    }

    /// The world's host wakeups: the image loads (`Loaded`) report on a channel that pings it.
    #[cfg(feature = "interp")]
    fn host_wakeups(rt: &qml::runtime::Runtime) -> testenv::Wakeups {
        let wake = testenv::Wakeups::new();
        rt.world.borrow().waker.set_ping(wake.ping());
        wake
    }

    /// Takes the image loads as they report until no Markdown inline image decode is in flight.
    /// The readiness is taken before they are drained: a report after the take pings.
    #[cfg(feature = "interp")]
    fn wait_inline_images(rt: &qml::runtime::Runtime, wake: &testenv::Wakeups) {
        loop {
            rt.world.borrow().waker.take();
            take_finished_images(&mut rt.world.borrow_mut());
            if INLINE_IMAGES.with_borrow(|(_, imgs, _)| imgs.iter().all(|i| !i.loading)) {
                return;
            }
            wake.wait(None, None);
        }
    }

    /// A Markdown inline image is decoded on the image pool: laying the text out does not wait
    /// for the file (here a FIFO nobody writes until the layout is done), and the text takes the
    /// image's size once it lands.
    #[cfg(feature = "interp")]
    #[test]
    fn markdown_inline_image_decodes_off_the_main_thread() {
        let dir = std::env::temp_dir().join(format!("ri-mdimage-slow-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let fifo = dir.join("slow.svg");
        let _ = std::fs::remove_file(&fifo);
        let c = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
        // SAFETY: `c` is a NUL-terminated path that outlives the call.
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        let (laid_out, released) = std::sync::mpsc::channel::<()>();
        let writer = {
            let fifo = fifo.clone();
            std::thread::spawn(move || {
                // Written once the layout is done (or the test gave up: the sender dropped).
                let held = released.recv().is_ok();
                std::fs::write(
                    &fifo,
                    r##"<svg xmlns="http://www.w3.org/2000/svg" width="30" height="40"><rect width="30" height="40" fill="#00ff00"/></svg>"##,
                )
                .unwrap();
                held
            })
        };
        let mut rt = qml::runtime::Runtime::new(None);
        let wake = host_wakeups(&rt);
        let root = rt
            .load_str(&format!(
                "import QtQuick\nItem {{ property Item md: TextEdit {{ textFormat: TextEdit.MarkdownText; font.pixelSize: 12; text: \"![x]({})\" }} }}",
                fifo.display()
            ))
            .unwrap();
        rt.settle();
        let qml::value::Value::Item(i) = rt.get(root, "md") else {
            panic!("md");
        };
        let mut fonts = super::Fonts::default();
        let width = |rt: &qml::runtime::Runtime, fonts: &mut super::Fonts| {
            let w = &mut rt.world.borrow_mut();
            super::measure(w, fonts, i);
            w.read(i, "implicitWidth").map_or(0.0, |v| v.to_number())
        };
        let before = width(&rt, &mut fonts);
        laid_out.send(()).unwrap();
        assert_eq!(before, 16.0, "16x16 space while loading");
        wait_inline_images(&rt, &wake);
        assert_eq!(width(&rt, &mut fonts), 30.0, "natural width once decoded");
        assert!(writer.join().unwrap(), "layout waited for the image file");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A GIF whose `frames` 1x1 images each composite onto a `w`x`h` canvas.
    fn gif_bomb(w: u16, h: u16, frames: usize) -> Vec<u8> {
        let mut g = b"GIF89a".to_vec();
        g.extend(w.to_le_bytes());
        g.extend(h.to_le_bytes());
        g.extend([0x80, 0, 0, 0, 0, 0, 255, 255, 255]);
        for _ in 0..frames {
            g.extend([0x21, 0xF9, 4, 0, 5, 0, 0, 0]);
            g.extend([0x2C, 0, 0, 0, 0, 1, 0, 1, 0, 0, 2, 2, 0x44, 1, 0]);
        }
        g.push(0x3B);
        g
    }

    /// GIFs of a few hundred bytes with large canvases or many frames. Qt 6.12 plays every one
    /// of these but `huge.gif`, whose canvas is over QImageReader's allocation limit; so does
    /// the shell, one frame at a time.
    #[test]
    fn animated_gif_plays_one_frame_at_a_time() {
        let dir = tempfile::tempdir().unwrap();
        let open = |name: &str, data: Vec<u8>, ss: (i32, i32)| {
            let f = dir.path().join(name);
            std::fs::write(&f, data).unwrap();
            crate::movie::Movie::open(f.to_str().unwrap(), ss)
        };
        // 2 x 16 GiB of canvas: refused from the header.
        assert!(open("huge.gif", gif_bomb(65535, 65535, 2), (0, 0)).is_none());
        assert!(open("huge-ss.gif", gif_bomb(65535, 65535, 2), (8, 8)).is_none());
        // One frame is a movie too: Qt plays it and stops.
        let one = open("one.gif", gif_bomb(2, 2, 1), (0, 0)).expect("one.gif plays");
        assert_eq!(one.frame_count(), 1);

        let mut big = open("big.gif", gif_bomb(4096, 4096, 8), (0, 0)).expect("big.gif plays");
        assert_eq!(big.frame_count(), 8);
        for k in [0, 1, 7, 3] {
            let c = big.frame(k).expect("frame");
            assert_eq!((c.w, c.h, c.px[0]), (4096, 4096, [0, 0, 0, 255]));
        }
        assert!(big.frame(8).is_none());

        let mut work = open("work.gif", gif_bomb(4000, 4000, 40), (8, 8)).expect("work.gif plays");
        assert_eq!(work.frame_count(), 40);
        assert_eq!(work.delays(), [50.0; 40]);
        let c = work.frame(39).expect("frame");
        assert_eq!((c.w, c.h), (8, 8));

        let mut many = open("many.gif", gif_bomb(2, 2, 5000), (0, 0)).expect("many.gif plays");
        assert_eq!(many.frame_count(), 5000);
        assert!(many.frame(4999).is_some());
        assert!(many.frame(0).is_some());
    }

    /// What Qt Quick's Image showed for the same files on Qt 6.12 (status Ready or Error).
    #[test]
    fn image_files_decode_by_content() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4"><rect width="4" height="4" fill="#ff0000"/></svg>"##;
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(svg.as_bytes()).unwrap();
        let svgz = gz.finish().unwrap();
        let xpm = b"/* XPM */\nstatic char *x[] = {\n\"4 4 1 1\",\n\"a c #ff0000\",\n\"aaaa\",\n\"aaaa\",\n\"aaaa\",\n\"aaaa\"\n};\n";
        let decode = |name: &str, data: &[u8]| {
            let f = dir.path().join(name);
            std::fs::write(&f, data).unwrap();
            super::decode_file(f.to_str().unwrap(), (0, 0), 0, None).map(|c| (c.w, c.h, c.px[5]))
        };
        let red = Some((4, 4, [255, 0, 0, 255]));
        for name in ["a.svg", "b.SVG", "noext", "wrong.png", "svg.xpm"] {
            assert_eq!(decode(name, svg.as_bytes()), red, "{name}");
        }
        for name in ["c.svgz", "c.SVGZ", "gz.svg"] {
            assert_eq!(decode(name, &svgz), red, "{name}");
        }
        // Qt Quick asks for the gzip reader only by the file's suffix.
        for name in ["gznoext", "gz.png"] {
            assert_eq!(decode(name, &svgz), None, "{name}");
        }
        for name in ["d.xpm", "e.XPM", "xpmnoext", "xpm.png", "xpm.svg"] {
            assert_eq!(decode(name, xpm), red, "{name}");
        }
        assert_eq!(decode("text.svg", b"not an image"), None);
    }

    /// An animated WebP of solid `frames` (colour, milliseconds): the container around the
    /// VP8L chunk `image` encodes for each frame.
    fn animated_webp(w: u32, h: u32, frames: &[([u8; 4], u32)]) -> Vec<u8> {
        let u24 = |v: u32| v.to_le_bytes()[..3].to_vec();
        let chunk = |tag: &[u8; 4], body: &[u8]| {
            let mut c = tag.to_vec();
            c.extend((body.len() as u32).to_le_bytes());
            c.extend(body);
            c.resize(c.len() + body.len() % 2, 0);
            c
        };
        let mut body = b"WEBP".to_vec();
        // VP8X: animation and alpha flags, canvas size minus one.
        body.extend(chunk(
            b"VP8X",
            &[vec![0x12, 0, 0, 0], u24(w - 1), u24(h - 1)].concat(),
        ));
        // ANIM: background colour, loop count 0 (forever).
        body.extend(chunk(b"ANIM", &[0; 6]));
        for (c, ms) in frames {
            let mut still = Vec::new();
            image::codecs::webp::WebPEncoder::new_lossless(&mut still)
                .encode(
                    &image::RgbaImage::from_pixel(w, h, image::Rgba(*c)),
                    w,
                    h,
                    image::ExtendedColorType::Rgba8,
                )
                .unwrap();
            assert_eq!(&still[12..16], b"VP8L");
            // ANMF: offset, size minus one, duration, no blending; then the frame's chunk.
            let head = [u24(0), u24(0), u24(w - 1), u24(h - 1), u24(*ms), vec![2]].concat();
            body.extend(chunk(b"ANMF", &[&head[..], &still[12..]].concat()));
        }
        let mut riff = b"RIFF".to_vec();
        riff.extend((body.len() as u32).to_le_bytes());
        riff.extend(body);
        riff
    }

    /// QMovie on Qt 6.12 animates GIF and WebP. A still WebP and a PNG are movies of one frame,
    /// shown for 0 and 1000 ms (`stills_are_movies_of_one_frame_as_in_qt` compares them with
    /// Qt); an SVG loads as a plain Image. Qt plays a GIF delay of 10 ms or less at 100 ms and
    /// a WebP delay as written.
    #[test]
    fn animated_image_formats_and_delays_follow_qmovie() {
        let dir = tempfile::tempdir().unwrap();
        let frames = |name: &str, data: Vec<u8>| {
            let f = dir.path().join(name);
            std::fs::write(&f, data).unwrap();
            let mut m = crate::movie::Movie::open(f.to_str().unwrap(), (0, 0))?;
            let delays = m.delays().to_vec();
            let px = |(k, ms)| (m.frame(k).expect("frame").px[0], ms);
            Some(delays.into_iter().enumerate().map(px).collect::<Vec<_>>())
        };
        let (red, blue) = ([255, 0, 0, 255], [0, 0, 255, 255]);
        let timed = [(red, 10), (blue, 120), (red, 20)];
        let webp = animated_webp(4, 4, &timed);
        let played = Some(vec![(red, 10.0), (blue, 120.0), (red, 20.0)]);
        assert_eq!(frames("a.webp", webp.clone()), played);
        // The content decides, not the name.
        assert_eq!(frames("a.gif", webp), played);
        assert_eq!(
            frames("one.webp", animated_webp(4, 4, &timed[..1])),
            Some(vec![(red, 10.0)])
        );

        let mut gif = Vec::new();
        {
            let mut enc = image::codecs::gif::GifEncoder::new(&mut gif);
            for (c, ms) in timed {
                let buf = image::RgbaImage::from_pixel(4, 4, image::Rgba(c));
                let d = image::Delay::from_numer_denom_ms(ms, 1);
                enc.encode_frame(image::Frame::from_parts(buf, 0, 0, d))
                    .unwrap();
            }
        }
        assert_eq!(
            frames("a.gif", gif),
            Some(vec![(red, 100.0), (blue, 120.0), (red, 20.0)])
        );

        let mut still = Vec::new();
        image::codecs::webp::WebPEncoder::new_lossless(&mut still)
            .encode(
                &[255, 0, 0, 255].repeat(16),
                4,
                4,
                image::ExtendedColorType::Rgba8,
            )
            .unwrap();
        assert_eq!(frames("still.webp", still), Some(vec![(red, 0.0)]));
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(4, 4, image::Rgba(blue))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        assert_eq!(frames("still.png", png), Some(vec![(blue, 1000.0)]));
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4"/>"#;
        assert_eq!(frames("a.svg", svg.into()), None);
    }

    #[cfg(feature = "interp")]
    #[test]
    fn animated_image_plays_gif_frames() {
        let dir = std::env::temp_dir().join(format!("ri-animimage-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let gif = dir.join("spin.gif");
        {
            let f = std::fs::File::create(&gif).unwrap();
            let mut enc = image::codecs::gif::GifEncoder::new(f);
            enc.set_repeat(image::codecs::gif::Repeat::Infinite)
                .unwrap();
            for (c, ms) in [([255u8, 0, 0, 255], 80u32), ([0, 0, 255, 255], 120)] {
                let buf = image::RgbaImage::from_pixel(4, 4, image::Rgba(c));
                let d = image::Delay::from_numer_denom_ms(ms, 1);
                enc.encode_frame(image::Frame::from_parts(buf, 0, 0, d))
                    .unwrap();
            }
        }
        let mut rt = qml::runtime::Runtime::new(None);
        let root = rt
            .load_str(&format!(
                "import QtQuick\nItem {{ property Item img: AnimatedImage {{ source: \"{}\" }} }}",
                gif.display()
            ))
            .unwrap();
        rt.settle();
        let qml::value::Value::Item(i) = rt.get(root, "img") else {
            panic!("img");
        };
        let mut fonts = super::Fonts::default();
        let px = |rt: &qml::runtime::Runtime, fonts: &mut super::Fonts| {
            let w = &mut rt.world.borrow_mut();
            super::measure(w, fonts, i);
            let mut c = Canvas::new(4, 4, [0, 0, 0, 0]);
            let mut cv = Target::Cpu(&mut c);
            paint_body(w, fonts, i, &mut cv, 0.0, 0.0, 4.0, 4.0, 1.0);
            c.px[5]
        };
        let red = |p: [u8; 4]| p[0] > 200 && p[2] < 50;
        let blue = |p: [u8; 4]| p[2] > 200 && p[0] < 50;
        assert!(red(px(&rt, &mut fonts)));
        assert_eq!(rt.get(i, "frameCount").to_number(), 2.0);
        assert_eq!(rt.get(i, "implicitWidth").to_number(), 4.0);
        rt.tick(0.0);
        rt.tick(85.0);
        assert_eq!(rt.get(i, "currentFrame").to_number(), 1.0);
        assert!(
            blue(px(&rt, &mut fonts)),
            "the second GIF frame shows after its delay"
        );
        rt.tick(200.0);
        assert!(red(px(&rt, &mut fonts)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An `asynchronous` AnimatedImage is laid out again when its movie, opened on the image
    /// pool, lands: its status goes from Loading to Ready and the first frame shows
    /// (FloatingImage.qml plays its GIF so).
    #[cfg(feature = "interp")]
    #[test]
    fn an_asynchronous_animated_image_shows_once_its_movie_opened() {
        let dir = std::env::temp_dir().join(format!("ri-animasync-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let gif = dir.join("spin.gif");
        {
            let f = std::fs::File::create(&gif).unwrap();
            let mut enc = image::codecs::gif::GifEncoder::new(f);
            for c in [[255u8, 0, 0, 255], [0, 0, 255, 255]] {
                let buf = image::RgbaImage::from_pixel(4, 4, image::Rgba(c));
                let d = image::Delay::from_numer_denom_ms(80, 1);
                enc.encode_frame(image::Frame::from_parts(buf, 0, 0, d))
                    .unwrap();
            }
        }
        let mut rt = qml::runtime::Runtime::new(None);
        let wake = host_wakeups(&rt);
        let root = rt
            .load_str(&format!(
                "import QtQuick\nItem {{ property Item img: AnimatedImage {{ asynchronous: true; source: \"{}\" }} }}",
                gif.display()
            ))
            .unwrap();
        rt.settle();
        let qml::value::Value::Item(i) = rt.get(root, "img") else {
            panic!("img");
        };
        let mut fonts = super::Fonts::default();
        super::measure(&mut rt.world.borrow_mut(), &mut fonts, i);
        assert_eq!(rt.get(i, "status").to_number(), 2.0, "Loading");
        let src = {
            let w = rt.world.borrow();
            w.resolved_url(i, gif.to_str().unwrap())
        };
        let mut again = Vec::new();
        loop {
            rt.world.borrow().waker.take();
            again.extend(take_finished_images(&mut rt.world.borrow_mut()).0);
            if !decode_pending(image_loads(&rt.world.borrow()), &src) {
                break;
            }
            wake.wait(None, None);
        }
        assert_eq!(again, [i], "the item is laid out again");
        let w = &mut rt.world.borrow_mut();
        super::measure(w, &mut fonts, i);
        assert_eq!(num(w, i, "status"), 1.0, "Ready");
        assert_eq!(num(w, i, "frameCount"), 2.0);
        let mut c = Canvas::new(4, 4, [0, 0, 0, 0]);
        paint_body(
            w,
            &mut fonts,
            i,
            &mut Target::Cpu(&mut c),
            0.0,
            0.0,
            4.0,
            4.0,
            1.0,
        );
        assert_eq!(c.px[5], [255, 0, 0, 255]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// What the shell does with `item` (QML, `id: it`, drawing the red `FILE`) in an 8x8 window:
    /// one full layout, then only the items [`take_finished_images`] names are laid out again.
    /// Returns the status before and after the loads landed, whether an item was named, and
    /// the pixel in the middle of the window.
    #[cfg(feature = "interp")]
    fn landed(tag: &str, imports: &str, item: &str) -> (f64, bool, f64, [u8; 4]) {
        let dir = std::env::temp_dir().join(format!("ri-landed-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("red.png");
        image::RgbaImage::from_pixel(8, 8, image::Rgba([255, 0, 0, 255]))
            .save(&file)
            .unwrap();
        let mut rt = qml::runtime::Runtime::new(Some(dir.clone()));
        let wake = host_wakeups(&rt);
        let item = item.replace("FILE", file.to_str().unwrap());
        std::fs::write(
            dir.join("Pic.qml"),
            "import QtQuick\nImage { asynchronous: true }\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("shell.qml"),
            format!(
                "import QtQuick\n{imports}\nItem {{ width: 8; height: 8; property alias img: it\n{item}\n}}"
            ),
        )
        .unwrap();
        let root = rt.load_file(&dir.join("shell.qml")).unwrap();
        rt.settle();
        let qml::value::Value::Item(i) = rt.get(root, "img") else {
            panic!("img");
        };
        let mut fonts = super::Fonts::default();
        layout_window(&mut rt, &mut fonts, root, [8.0, 8.0]);
        rt.settle();
        let before = rt.get(i, "status").to_number();
        let mut named = false;
        loop {
            rt.world.borrow().waker.take();
            let (hit, _) = take_finished_images(&mut rt.world.borrow_mut());
            if !hit.is_empty() {
                named = true;
                relayout_targets(&mut rt.world.borrow_mut(), &mut fonts, &hit, 0);
                rt.settle();
            }
            let busy = {
                let w = rt.world.borrow();
                let loads = image_loads(&w);
                !loads.decoding.borrow().is_empty() || !loads.downloading.borrow().is_empty()
            };
            if !busy {
                break;
            }
            wake.wait(None, None);
        }
        let after = rt.get(i, "status").to_number();
        let mut c = Canvas::new(8, 8, [0, 0, 0, 0]);
        paint_window(&mut rt, &mut fonts, root, &mut Target::Cpu(&mut c));
        let _ = std::fs::remove_dir_all(&dir);
        (before, named, after, c.px[4 * 8 + 4])
    }

    /// An Image keeps the decode it waited for, like QQuickImage its pixmap, however big: the
    /// wallpaper at scale 2 is 36 MiB, more than the cache keeps of images no frame draws, and
    /// it is hidden until Ready, so no frame draws it before it is laid out. Evicted when it
    /// landed, it was decoded again at that layout, for ever, and never shown.
    #[cfg(feature = "interp")]
    #[test]
    fn a_decode_bigger_than_the_cache_stays_for_its_image() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("red.png");
        image::RgbaImage::from_pixel(8, 8, image::Rgba([255, 0, 0, 255]))
            .save(&file)
            .unwrap();
        let mut rt = qml::runtime::Runtime::new(Some(dir.path().to_path_buf()));
        let wake = host_wakeups(&rt);
        std::fs::write(
            dir.path().join("shell.qml"),
            "import QtQuick\nItem { width: 8; height: 8; property alias img: it\n\
             Image { id: it; width: 8; height: 8; asynchronous: true; source: \"red.png\"\n\
             sourceSize: Qt.size(3200, 3200); fillMode: Image.PreserveAspectCrop\n\
             visible: opacity > 0; opacity: status === Image.Ready ? 1 : 0 } }",
        )
        .unwrap();
        let root = rt.load_file(&dir.path().join("shell.qml")).unwrap();
        rt.settle();
        let qml::value::Value::Item(i) = rt.get(root, "img") else {
            panic!("img");
        };
        let mut fonts = super::Fonts::default();
        layout_window(&mut rt, &mut fonts, root, [8.0, 8.0]);
        rt.settle();
        let mut landed = 0;
        while landed < 3 {
            rt.world.borrow().waker.take();
            let (hit, _) = take_finished_images(&mut rt.world.borrow_mut());
            if !hit.is_empty() {
                landed += 1;
                relayout_targets(&mut rt.world.borrow_mut(), &mut fonts, &hit, 0);
                rt.settle();
            }
            if image_loads(&rt.world.borrow()).decoding.borrow().is_empty() {
                break;
            }
            wake.wait(None, None);
        }
        assert_eq!(landed, 1, "decoded once");
        assert_eq!(rt.get(i, "status").to_number(), 1.0);
        // The frame loop turns before the window is painted (no frame callback yet).
        take_finished_images(&mut rt.world.borrow_mut());
        layout_window(&mut rt, &mut fonts, root, [8.0, 8.0]);
        assert!(
            image_loads(&rt.world.borrow()).decoding.borrow().is_empty(),
            "decoded again before it was painted"
        );
        assert_eq!(rt.get(i, "status").to_number(), 1.0);
        IMAGES.with_borrow(|c| assert!(c.bytes() > IMAGES_BYTES, "{}", c.bytes()));
        let mut c = Canvas::new(8, 8, [0, 0, 0, 0]);
        paint_window(&mut rt, &mut fonts, root, &mut Target::Cpu(&mut c));
        assert_eq!(c.px[4 * 8 + 4], [255, 0, 0, 255]);

        // An Image that shows something else keeps no decode.
        (rt.world.borrow_mut()).write(i, "source", qml::value::Value::Str("".into()));
        rt.settle();
        layout_window(&mut rt, &mut fonts, root, [8.0, 8.0]);
        assert!(image_loads(&rt.world.borrow()).kept.borrow().is_empty());
    }

    /// An Image with `retainWhileLoading` draws the pixmap it shows until the next one is
    /// loaded (QQuickImageBase swaps `pendingPix` into `currentPix` when its request ends),
    /// whatever the cache keeps meanwhile; one without it draws nothing while it loads. The
    /// wallpaper of Background.qml is such an Image, dimmed while it is not Ready: with no
    /// pixmap a change of wallpaper showed frames of the bare background.
    #[cfg(feature = "interp")]
    #[test]
    fn a_retaining_image_draws_its_pixmap_while_the_next_loads() {
        let dir = tempfile::tempdir().unwrap();
        for (name, color) in [
            ("red.png", [255, 0, 0, 255]),
            ("blue.png", [0, 0, 255, 255]),
        ] {
            image::RgbaImage::from_pixel(8, 8, image::Rgba(color))
                .save(dir.path().join(name))
                .unwrap();
        }
        let mut rt = qml::runtime::Runtime::new(Some(dir.path().to_path_buf()));
        let wake = host_wakeups(&rt);
        // The retained one is the wallpaper at scale 2: more than the cache keeps unheld.
        std::fs::write(
            dir.path().join("shell.qml"),
            "import QtQuick\nItem { width: 16; height: 8\n\
             property alias kept: a; property alias plain: b\n\
             Image { id: a; width: 8; height: 8; asynchronous: true; source: \"red.png\"\n\
             retainWhileLoading: true; sourceSize: Qt.size(3200, 3200) }\n\
             Image { id: b; x: 8; width: 8; height: 8; asynchronous: true; source: \"red.png\"\n\
             sourceSize: Qt.size(8, 8) } }",
        )
        .unwrap();
        let root = rt.load_file(&dir.path().join("shell.qml")).unwrap();
        rt.settle();
        let item = |rt: &qml::runtime::Runtime, name: &str| match rt.get(root, name) {
            qml::value::Value::Item(i) => i,
            _ => panic!("{name}"),
        };
        let (kept, plain) = (item(&rt, "kept"), item(&rt, "plain"));
        let mut fonts = super::Fonts::default();
        // Lays the window out, lets every decode under way land, and paints it: the pixels in
        // the middle of the two Images.
        let mut shown = |rt: &mut qml::runtime::Runtime, land: bool| {
            layout_window(rt, &mut fonts, root, [16.0, 8.0]);
            rt.settle();
            while land && !image_loads(&rt.world.borrow()).decoding.borrow().is_empty() {
                wake.wait(None, None);
                rt.world.borrow().waker.take();
                let (hit, _) = take_finished_images(&mut rt.world.borrow_mut());
                relayout_targets(&mut rt.world.borrow_mut(), &mut fonts, &hit, 0);
                rt.settle();
                layout_window(rt, &mut fonts, root, [16.0, 8.0]);
                rt.settle();
            }
            let mut c = Canvas::new(16, 8, [0, 0, 0, 0]);
            paint_window(rt, &mut fonts, root, &mut Target::Cpu(&mut c));
            let status = [kept, plain].map(|i| rt.get(i, "status").to_number());
            (status, [c.px[4 * 16 + 4], c.px[4 * 16 + 12]])
        };
        let (red, blue, none) = ([255, 0, 0, 255], [0, 0, 255, 255], [0, 0, 0, 0]);
        assert_eq!(shown(&mut rt, true), ([1.0, 1.0], [red, red]));

        for i in [kept, plain] {
            (rt.world.borrow_mut()).write(i, "source", qml::value::Value::Str("blue.png".into()));
        }
        rt.settle();
        assert_eq!(shown(&mut rt, false), ([2.0, 2.0], [red, none]), "Loading");
        // Another image is stored meanwhile: the cache lets go of what nothing holds.
        cache_image(("other".into(), (0, 0), 0), None);
        assert_eq!(shown(&mut rt, false), ([2.0, 2.0], [red, none]), "Loading");
        assert_eq!(shown(&mut rt, true), ([1.0, 1.0], [blue, blue]));
        let w = rt.world.borrow();
        let current = image_loads(&w).current.borrow();
        assert!(
            current.values().all(|c| c.held.is_none()),
            "held past the load"
        );
    }

    /// An Image that is Ready draws its pixmap while another Image has the same file decoded
    /// at another size: QQuickImage holds the pixmap it loaded.
    #[cfg(feature = "interp")]
    #[test]
    fn a_ready_image_draws_while_its_file_is_decoded_for_another() {
        let dir = tempfile::tempdir().unwrap();
        image::RgbaImage::from_pixel(8, 8, image::Rgba([255, 0, 0, 255]))
            .save(dir.path().join("red.png"))
            .unwrap();
        let mut rt = qml::runtime::Runtime::new(Some(dir.path().to_path_buf()));
        let wake = host_wakeups(&rt);
        std::fs::write(
            dir.path().join("shell.qml"),
            "import QtQuick\nItem { width: 16; height: 8\n\
             property alias first: a; property alias second: b\n\
             Image { id: a; width: 8; height: 8; asynchronous: true; source: \"red.png\"\n\
             sourceSize: Qt.size(8, 8) }\n\
             Image { id: b; x: 8; width: 8; height: 8; asynchronous: true\n\
             sourceSize: Qt.size(4, 4) } }",
        )
        .unwrap();
        let root = rt.load_file(&dir.path().join("shell.qml")).unwrap();
        rt.settle();
        let item = |rt: &qml::runtime::Runtime, name: &str| match rt.get(root, name) {
            qml::value::Value::Item(i) => i,
            _ => panic!("{name}"),
        };
        let (first, second) = (item(&rt, "first"), item(&rt, "second"));
        let mut fonts = super::Fonts::default();
        let mut shown = |rt: &mut qml::runtime::Runtime, land: bool| {
            layout_window(rt, &mut fonts, root, [16.0, 8.0]);
            rt.settle();
            while land && !image_loads(&rt.world.borrow()).decoding.borrow().is_empty() {
                wake.wait(None, None);
                rt.world.borrow().waker.take();
                let (hit, _) = take_finished_images(&mut rt.world.borrow_mut());
                relayout_targets(&mut rt.world.borrow_mut(), &mut fonts, &hit, 0);
                rt.settle();
                layout_window(rt, &mut fonts, root, [16.0, 8.0]);
                rt.settle();
            }
            let mut c = Canvas::new(16, 8, [0, 0, 0, 0]);
            paint_window(rt, &mut fonts, root, &mut Target::Cpu(&mut c));
            let status = [first, second].map(|i| rt.get(i, "status").to_number());
            (status, [c.px[4 * 16 + 4], c.px[4 * 16 + 12]])
        };
        let (red, none) = ([255, 0, 0, 255], [0, 0, 0, 0]);
        assert_eq!(shown(&mut rt, true), ([1.0, 0.0], [red, none]));
        (rt.world.borrow_mut()).write(second, "source", qml::value::Value::Str("red.png".into()));
        rt.settle();
        assert_eq!(shown(&mut rt, false), ([1.0, 2.0], [red, none]), "Loading");
        assert_eq!(shown(&mut rt, true), ([1.0, 1.0], [red, red]));
    }

    /// Every item that decodes its picture on the image pool is laid out again when the decode
    /// lands, whatever its type: it goes from Loading to Ready and draws. `take_finished_images`
    /// finds them by type name, so a type it does not name stays Loading for good (an
    /// AnimatedImage did).
    #[cfg(feature = "interp")]
    #[test]
    fn every_background_decode_is_laid_out_again() {
        let widgets = "import Rustshell.Widgets";
        let effects = "import Qt5Compat.GraphicalEffects";
        let cases: &[(&str, &str, &str)] = &[
            (
                "image",
                "",
                "Image { id: it; asynchronous: true; source: \"FILE\" }",
            ),
            (
                "url",
                "",
                "Image { id: it; asynchronous: true; source: \"file://FILE\" }",
            ),
            (
                "relative",
                "",
                "Image { id: it; asynchronous: true; source: \"red.png\" }",
            ),
            // A type of the tree with an Image for its root (StyledImage, CliphistImage).
            ("derived", "", "Pic { id: it; source: \"FILE\" }"),
            (
                "still-in-animated",
                "",
                "AnimatedImage { id: it; asynchronous: true; source: \"FILE\" }",
            ),
            // Quickshell's IconImage: an Item around an Image, `status` its alias.
            (
                "iconimage",
                widgets,
                "IconImage { id: it; implicitSize: 8; asynchronous: true; source: \"FILE\" }",
            ),
            (
                "layer",
                "",
                "Image { id: it; asynchronous: true; source: \"FILE\"; layer.enabled: true }",
            ),
            (
                "effect-source",
                effects,
                "Image { id: it; visible: false; asynchronous: true; source: \"FILE\" }\n\
                 Rectangle { id: m; width: 8; height: 8; visible: false }\n\
                 OpacityMask { width: 8; height: 8; source: it; maskSource: m }",
            ),
            // StyledImage: hidden until Ready, so nothing but the report lays it out.
            (
                "hidden-until-ready",
                "",
                "Image { id: it; asynchronous: true; source: \"FILE\"; visible: opacity > 0; \
                 opacity: status === Image.Ready ? 1 : 0 }",
            ),
            (
                "two-sizes",
                "",
                "Image { id: it; asynchronous: true; source: \"FILE\"; sourceSize: Qt.size(8, 8) }\n\
                 Image { asynchronous: true; source: \"FILE\"; sourceSize: Qt.size(4, 4) }",
            ),
        ];
        for (tag, imports, item) in cases {
            assert_eq!(
                landed(tag, imports, item),
                (2.0, true, 1.0, [255, 0, 0, 255]),
                "{tag}: Loading, then named, Ready and drawn"
            );
        }
    }

    /// A Kirigami.Icon has no status and decodes where it is drawn: its picture is there on
    /// the first frame, with nothing left in flight.
    #[cfg(feature = "interp")]
    #[test]
    fn a_kirigami_icon_draws_its_file_on_the_first_frame() {
        let kirigami = "import org.kde.kirigami as Kirigami";
        let (_, named, _, px) = landed(
            "kicon",
            kirigami,
            "Kirigami.Icon { id: it; width: 8; height: 8; source: \"FILE\" }",
        );
        assert_eq!((named, px), (false, [255, 0, 0, 255]));
        let (_, named, _, px) = landed(
            "kicon-mask",
            kirigami,
            "Kirigami.Icon { id: it; width: 8; height: 8; isMask: true; color: \"#00ff00\"; \
             source: \"FILE\" }",
        );
        assert_eq!((named, px), (false, [0, 255, 0, 255]));
    }

    #[cfg(feature = "interp")]
    #[test]
    fn markdown_inline_image_sits_on_the_baseline_at_its_natural_size() {
        let dir = std::env::temp_dir().join(format!("ri-mdimage-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let svg = dir.join("formula.svg");
        std::fs::write(
            &svg,
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="30" height="40" viewBox="0 0 30 40"><rect width="30" height="40" fill="#00ff00"/></svg>"##,
        )
        .unwrap();
        let svg = svg.to_str().unwrap();
        let edit = |text: &str| {
            format!(
                "TextEdit {{ textFormat: TextEdit.MarkdownText; color: \"#000000\"; font.pixelSize: 12; text: \"{text}\" }}"
            )
        };
        let mut rt = qml::runtime::Runtime::new(None);
        let wake = host_wakeups(&rt);
        let root = rt
            .load_str(&format!(
                "import QtQuick\nItem {{\n    property Item solo: {}\n    property Item inline: {}\n    property Item missing: {}\n    property Item plain: {}\n}}",
                edit(&format!("![latex]({svg})")),
                edit(&format!("Euler: ![latex](file://{svg}) inline.")),
                edit("![latex](/nonexistent/formula.svg)"),
                edit("Euler"),
            ))
            .unwrap();
        rt.settle();
        let mut fonts = super::Fonts::default();
        // (implicitWidth, implicitHeight, painted canvas)
        let render = |rt: &qml::runtime::Runtime, fonts: &mut super::Fonts, k: &str| {
            let qml::value::Value::Item(i) = rt.get(root, k) else {
                panic!("{k}");
            };
            let w = &mut rt.world.borrow_mut();
            super::measure(w, fonts, i);
            let iw = w.read(i, "implicitWidth").map_or(0.0, |v| v.to_number());
            let ih = w.read(i, "implicitHeight").map_or(0.0, |v| v.to_number());
            let mut c = Canvas::new(200, 60, [0, 0, 0, 0]);
            let mut cv = Target::Cpu(&mut c);
            paint_body(w, fonts, i, &mut cv, 0.0, 0.0, iw, ih, 1.0);
            (iw, ih, c)
        };
        for k in ["solo", "inline", "missing"] {
            render(&rt, &mut fonts, k);
        }
        wait_inline_images(&rt, &wake);
        let green = |p: [u8; 4]| p[1] >= 128 && p[0] < 64 && p[2] < 64;
        let ink = |p: [u8; 4]| p[3] >= 128 && !green(p);
        // (x0, y0, x1, y1) inclusive bounds of the pixels matching `f`.
        let bbox = |c: &Canvas, f: &dyn Fn([u8; 4]) -> bool| {
            let mut b: Option<(usize, usize, usize, usize)> = None;
            for (k, &p) in c.px.iter().enumerate() {
                if f(p) {
                    let (x, y) = (k % c.w, k / c.w);
                    b = Some(b.map_or((x, y, x, y), |(a, b, cc, d)| {
                        (a.min(x), b.min(y), cc.max(x), d.max(y))
                    }));
                }
            }
            b
        };

        let (_, plain_h, _) = render(&rt, &mut fonts, "plain");
        let (solo_w, solo_h, solo) = render(&rt, &mut fonts, "solo");
        let (x0, y0, x1, y1) = bbox(&solo, &green).expect("image drawn");
        assert_eq!((x1 - x0 + 1, y1 - y0 + 1), (30, 40), "natural size");
        assert!(
            solo.px.iter().all(|&p| p[3] == 0 || green(p)),
            "nothing but the image is drawn (no `!latex`)"
        );
        assert!(
            solo_h >= 40.0 && solo_h > plain_h,
            "implicitHeight {solo_h} fits the image"
        );

        let (_, inline_h, inline) = render(&rt, &mut fonts, "inline");
        assert_eq!(inline_h, solo_h, "one line as tall as the image line");
        let (gx0, gy0, gx1, gy1) = bbox(&inline, &green).expect("inline image drawn");
        assert!((30..=31).contains(&(gx1 - gx0 + 1)) && gy1 - gy0 + 1 == 40);
        let mut before = inline.clone();
        for (k, p) in before.px.iter_mut().enumerate() {
            if k % inline.w >= gx0 {
                *p = [0, 0, 0, 0];
            }
        }
        let left = bbox(&before, &ink);
        let (_, _, lx1, ly1) = left.expect("text before the image");
        assert!(lx1 < gx0, "text left of the image");
        assert!(
            ly1.abs_diff(gy1) <= 1,
            "image bottom {gy1} on the baseline (text {ly1})"
        );
        let right = (0..inline.px.len())
            .filter(|k| k % inline.w > gx1 && ink(inline.px[*k]))
            .count();
        assert!(right > 0, "text after the image");

        let (missing_w, _, missing) = render(&rt, &mut fonts, "missing");
        assert!(
            missing.px.iter().all(|p| p[3] == 0),
            "a missing image draws nothing"
        );
        assert_eq!(
            missing_w - 16.0,
            solo_w - 30.0,
            "a missing image takes 16px"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn screencopy_size_reports_changes_like_the_old_map() {
        let i = 2_000_000;
        assert_eq!(screencopy_size(i), None);
        assert!(set_screencopy_size(i, (10, 20)));
        assert!(!set_screencopy_size(i, (10, 20)));
        assert!(set_screencopy_size(i, (10, 21)));
        assert_eq!(screencopy_size(i), Some((10, 21)));
    }

    #[test]
    fn shape_keys_hash_contents_without_formatting() {
        use qsg::shape::{Cap, Join, PathCmd, Stroke};
        let spec = |fc: [u8; 4], y: f64, width: f32, even_odd: bool| ShapePathSpec {
            start: (1.5, 2.0),
            cmds: vec![
                PathCmd::Line(10.0, y),
                PathCmd::AngleArc {
                    cx: 5.0,
                    cy: 5.0,
                    rx: 4.0,
                    ry: 4.0,
                    start: 0.0,
                    sweep: 90.0,
                    move_to_start: false,
                },
            ],
            fc,
            even_odd,
            stroke: Some(Stroke {
                width,
                color: [0, 0, 0, 255],
                cap: Cap::Round,
                join: Join::Miter,
                miter_limit: 4.0,
            }),
        };
        let key = |p: ShapePathSpec, curve: bool, extra: &[f64]| {
            shape_key(&(curve, vec![(p, qsg::canvas::Path::default())]), extra)
        };
        let base = key(
            spec([255, 0, 0, 255], 3.0, 2.0, false),
            false,
            &[10.0, 6.0, 1.0],
        );
        assert_eq!(
            base,
            key(
                spec([255, 0, 0, 255], 3.0, 2.0, false),
                false,
                &[10.0, 6.0, 1.0]
            )
        );
        for other in [
            key(
                spec([0, 255, 0, 255], 3.0, 2.0, false),
                false,
                &[10.0, 6.0, 1.0],
            ),
            key(
                spec([255, 0, 0, 255], 3.5, 2.0, false),
                false,
                &[10.0, 6.0, 1.0],
            ),
            key(
                spec([255, 0, 0, 255], 3.0, 2.5, false),
                false,
                &[10.0, 6.0, 1.0],
            ),
            key(
                spec([255, 0, 0, 255], 3.0, 2.0, true),
                false,
                &[10.0, 6.0, 1.0],
            ),
            key(
                spec([255, 0, 0, 255], 3.0, 2.0, false),
                true,
                &[10.0, 6.0, 1.0],
            ),
            key(
                spec([255, 0, 0, 255], 3.0, 2.0, false),
                false,
                &[10.0, 6.0, 0.5],
            ),
        ] {
            assert_ne!(base, other);
        }
    }

    /// Cached local files go once inotify reports them gone, or for a file gone before its
    /// watch was added, a stat on a decode worker (none is made while every worker is busy);
    /// entries of existing files stay, and the directory watch goes with the last entry
    /// cached from it.
    #[test]
    fn prune_images_drops_files_reported_gone() {
        let dir = std::env::temp_dir().join(format!("ri-prune-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (early, gone, kept) = (
            dir.join("early.png"),
            dir.join("gone.png"),
            dir.join("kept.png"),
        );
        std::fs::write(&gone, b"x").unwrap();
        std::fs::write(&kept, b"x").unwrap();
        let key = |p: &std::path::Path| (p.to_string_lossy().into_owned(), (0, 0), 0);
        for p in [&early, &gone, &kept] {
            cache_image(key(p), Some(Rc::new(Canvas::new(1, 1, [0; 4]))));
        }
        let has = |p: &std::path::Path| IMAGES.with_borrow(|c| c.contains(&key(p)));
        let srcs = |ps: &[&std::path::PathBuf]| -> std::collections::HashSet<String> {
            ps.iter().map(|p| key(p).0).collect()
        };
        // Blocks on the report and hands it back to the channel `prune_images` drains.
        let report = || {
            IMAGE_WATCH.with_borrow(|w| {
                let r = w.rx.recv().unwrap();
                w.tx.send(r.clone()).unwrap();
                r
            })
        };
        // Every decode worker is held in a job of this test, so the stat cannot run on one
        // yet: an entry dropped or a report there when the prune returns was made by a stat
        // on this thread. (A free worker can report before the prune that queued its stat
        // reads the reports, which drops the entry in that same prune.)
        let (started_tx, started) = std::sync::mpsc::channel();
        let release: Vec<_> = (0..DECODE_WORKERS)
            .map(|_| {
                let (tx, held) = std::sync::mpsc::channel::<()>();
                let started_tx = started_tx.clone();
                submit_decode(Box::new(move || {
                    let _ = started_tx.send(());
                    // Ends when the test drops `release`, also by a failed assertion.
                    let _ = held.recv();
                }));
                tx
            })
            .collect();
        for _ in &release {
            started.recv().unwrap();
        }
        prune_images();
        assert!(has(&early), "stat ran on the UI thread");
        assert!(
            IMAGE_WATCH.with_borrow(|w| w.rx.try_recv().is_err()),
            "stat ran on the UI thread"
        );
        drop(release);
        assert_eq!(report(), srcs(&[&early]));
        prune_images();
        assert!(!has(&early), "file gone before its watch kept");
        std::fs::remove_file(&gone).unwrap();
        assert_eq!(report(), srcs(&[&gone]));
        prune_images();
        assert!(!has(&gone), "deleted file kept");
        assert!(has(&kept), "existing file pruned");
        let watched = || IMAGE_WATCH.with_borrow(|w| lock_dirs(&w.shared).dirs.contains_key(&dir));
        assert!(watched());
        IMAGES.with_borrow_mut(|c| c.remove(&key(&kept)));
        assert!(!watched(), "watch outlived the directory's last entry");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Swapping an image for its texture stand-in drops only the recorded draws holding it.
    #[test]
    fn replaced_images_drop_only_draws_holding_them() {
        let (a, b) = (
            Rc::new(Canvas::new(2, 2, [1; 4])),
            Rc::new(Canvas::new(2, 2, [2; 4])),
        );
        let rec = |t: &Rc<Canvas>| {
            let mut fr = crate::gpu::Frame::new(8, 8);
            let s = fr.begin();
            fr.image(
                t.clone(),
                [0.0, 0.0, 4.0, 4.0],
                [0.0, 0.0, 1.0, 1.0],
                1.0,
                false,
            );
            Rc::new(fr.record(&s).expect("recorded"))
        };
        DL.with_borrow_mut(|d| {
            d.insert(1, (Default::default(), rec(&a), 0));
            d.insert(2, (Default::default(), rec(&b), 0));
        });
        let ptrs = [Rc::as_ptr(&a) as *const () as usize].into_iter().collect();
        forget_draws_holding(&ptrs);
        DL.with_borrow(|d| {
            assert!(!d.contains_key(&1), "draw of the replaced image kept");
            assert!(d.contains_key(&2), "unrelated draw dropped");
        });
    }

    /// main.rs `mark_dirty` after a panel closes: the sweep releases the destroyed instances
    /// and `forget_chunks` drops their render state, then the frame's touched ids are
    /// invalidated, and a write that reached an instance after its destruction is among them.
    /// Its stamp allocated the dropped chunk again, and nothing dropped it a second time.
    #[test]
    #[cfg_attr(
        not(feature = "interp"),
        ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
    )]
    fn stamps_of_released_instances_do_not_bring_back_dropped_chunks() {
        use qml::runtime::CHUNK_BITS;
        let mut rt = qml::runtime::Runtime::new(None);
        let root = rt
            .load_str(
                "import QtQuick\nItem { id: root; property bool on: true\n Loader { active: root.on; sourceComponent: Item { Repeater { model: 5000; delegate: Item { width: 2; height: 2 } } } } }\n",
            )
            .unwrap();
        rt.settle();
        let gone: Vec<usize> = {
            let w = rt.world.borrow();
            let last = w.items.len() - 1;
            (root + 1..=last)
                .filter(|&i| i >> CHUNK_BITS != last >> CHUNK_BITS && i % 700 == 0)
                .collect()
        };
        assert!(gone.len() >= 3, "{gone:?}");
        // Something is recorded, so invalidation has draws to drop.
        let seg = {
            let mut fr = crate::gpu::Frame::new(8, 8);
            let s = fr.begin();
            fr.image(canvas(2, 2), [0.0; 4], [0.0, 0.0, 1.0, 1.0], 1.0, false);
            Rc::new(fr.record(&s).expect("recorded"))
        };
        DL.with_borrow_mut(|d| d.insert(root, (Default::default(), seg, 0)));
        rt.set(root, "on", Value::Bool(false));
        rt.settle();
        let w = &mut rt.world.borrow_mut();
        for &i in &gone {
            assert!(w.dead.contains(&i));
            w.write(i, "x", Value::Num(5.0));
        }
        let reaped = std::mem::take(&mut w.reaped);
        w.sweep_dead(&reaped);
        let freed = w.items.take_freed();
        assert!(freed.len() >= 2, "{freed:?}");
        forget_chunks(w, &freed);
        forget_items(&reaped);
        let touched = std::mem::take(&mut w.touched);
        assert!(gone.iter().all(|i| touched.contains(i)), "{touched:?}");
        invalidate_layers(w, &touched);
        let held = w.items.capacity() >> CHUNK_BITS;
        for (name, n) in [
            ("TYPE_BITS", TYPE_BITS.with_borrow(|v| v.allocated().0)),
            ("SRC_KEYS", SRC_KEYS.with_borrow(|v| v.allocated().0)),
            ("DL_STAMP", DL_STAMP.with_borrow(|v| v.allocated().0)),
        ] {
            assert!(n <= held, "{name}: {n} chunks for {held} of instances");
        }
        DL.with_borrow_mut(|d| d.clear());
    }

    /// A panel's cached shadow is a texture as big as the panel: its pixels stayed in the
    /// cache beside the texture for as long as the entry did.
    #[test]
    #[cfg_attr(
        not(feature = "interp"),
        ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
    )]
    fn cached_shadows_keep_only_their_texture_once_uploaded() {
        let (mut g, _) = crate::gpu::Gpu::headless("surfaceless").expect("surfaceless EGL (Mesa)");
        let mut s = g.pbuffer(96, 96).expect("pbuffer");
        let mut rt = qml::runtime::Runtime::new(None);
        let win = rt
            .load_str(
                r##"import QtQuick
import QtQuick.Effects
import Rustshell
FloatingWindow {
    width: 96; height: 96
    color: "transparent"
    RectangularShadow {
        x: 24; y: 24; width: 48; height: 48
        radius: 8; blur: 12; spread: 2; color: "#c0102030"; cached: true
    }
}"##,
            )
            .unwrap();
        let mut fonts = Fonts::default();
        layout_window(&mut rt, &mut fonts, win, [96.0, 96.0]);
        let mut shot = |rt: &mut qml::runtime::Runtime| {
            let mut fr = crate::gpu::Frame::new(96, 96);
            paint_window(rt, &mut fonts, win, &mut Target::Gpu(&mut fr));
            s.reswap();
            g.render(&mut s, &mut fr, || {});
            g.read_surface(&s).px
        };
        let pixels = || {
            LAYER_SHADOWS.with_borrow(|c| {
                let v: Vec<_> = c
                    .iter()
                    .map(|(_, (t, _))| (t.px.len(), t.w * t.h))
                    .collect();
                (v, c.bytes())
            })
        };
        let first = shot(&mut rt);
        assert!(first.iter().any(|p| p[3] != 0), "nothing drawn");
        let (v, weight) = pixels();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].0, v[0].1, "painted with its pixels");
        // The host's turn after the frame (`take_downloaded`).
        prune_images();
        let (v, after) = pixels();
        assert_eq!(v[0].0, 0, "pixels kept beside the texture");
        assert_eq!(after, weight, "the entry weighs its texture");
        // Painted again from the cache, it is the same texture.
        forget_draws();
        assert_eq!(shot(&mut rt), first);
        assert_eq!(pixels().0.len(), 1, "raster made again");
        LAYER_SHADOWS.with_borrow_mut(|c| c.clear());
    }

    /// A card with something repainted every frame (a visualizer, a wavy progress bar) is
    /// painted every frame, and so was every still text and rectangle on it: each a single
    /// draw, which was never recorded. They are replayed now, into the same pixels.
    #[test]
    #[cfg_attr(
        not(feature = "interp"),
        ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
    )]
    fn still_single_draws_beside_a_repainted_one_are_replayed() {
        let (mut g, _) = crate::gpu::Gpu::headless("surfaceless").expect("surfaceless EGL (Mesa)");
        let mut s = g.pbuffer(120, 60).expect("pbuffer");
        let mut rt = qml::runtime::Runtime::new(None);
        let win = rt
            .load_str(
                r##"import QtQuick
import Rustshell
FloatingWindow {
    id: win
    width: 120; height: 60
    color: "transparent"
    property color wave: "#102030"
    Rectangle {
        width: 120; height: 60; color: "#202020"
        Rectangle { x: 4; y: 4; width: 30; height: 30; radius: 6; color: "#c04040" }
        Text { x: 40; y: 4; text: "Bench Song"; color: "#ffffff" }
        Rectangle { x: 4; y: 40; width: 100; height: 8; color: win.wave }
    }
}"##,
            )
            .unwrap();
        let mut fonts = Fonts::default();
        layout_window(&mut rt, &mut fonts, win, [120.0, 60.0]);
        let mut shot = |rt: &mut qml::runtime::Runtime, k: usize, fresh: bool| {
            let wave = Value::Str(format!("#{:02x}40c0", 16 + 9 * (k % 20)).into());
            write_and_invalidate(rt, win, "wave", wave);
            if fresh {
                forget_draws();
            }
            take_dl_stats();
            let mut fr = crate::gpu::Frame::new(120, 60);
            paint_window(rt, &mut fonts, win, &mut Target::Gpu(&mut fr));
            s.reswap();
            g.render(&mut s, &mut fr, || {});
            (g.read_surface(&s).px, take_dl_stats().0)
        };
        // Every frame painted from nothing, then the same frames with what was recorded.
        let fresh: Vec<_> = (0..24).map(|k| shot(&mut rt, k, true).0).collect();
        assert_ne!(fresh[0], fresh[1], "the repainted rectangle did not change");
        forget_draws();
        for (k, want) in fresh.iter().enumerate() {
            let (px, replayed) = shot(&mut rt, k, false);
            assert!(px == *want, "frame {k} differs from its fresh paint");
            // Frame 0 records the card, frame 1 finds it stale and records the still draws.
            if k >= 2 {
                assert_eq!(replayed, 2, "frame {k}: the rectangle and the text");
            }
        }
        forget_draws();
    }

    /// Item `k` of `root`, and `rt.settle()` + main.rs's layer invalidation after writes.
    fn item_of(rt: &qml::runtime::Runtime, root: usize, k: &str) -> usize {
        let Value::Item(i) = rt.get(root, k) else {
            panic!("{k}");
        };
        i
    }

    fn write_and_invalidate(rt: &mut qml::runtime::Runtime, root: usize, k: &str, v: Value) {
        rt.world.borrow_mut().write(root, k, v);
        rt.settle();
        let w = &mut rt.world.borrow_mut();
        let touched = std::mem::take(&mut w.touched);
        invalidate_layers(w, &touched);
    }

    #[test]
    #[cfg_attr(
        not(feature = "interp"),
        ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
    )]
    fn layers_are_kept_until_their_pixels_change() {
        let mut rt = qml::runtime::Runtime::new(None);
        let root = rt
            .load_str(
                r##"import QtQuick
Item {
    width: 20; height: 20
    property bool alt: false
    property int tag: 0
    property Item src: Item {
        width: 20; height: 20
        Rectangle { objectName: "r" + tag; width: 20; height: 20; color: alt ? "#0000ff" : "#ff0000" }
    }
}"##,
            )
            .unwrap();
        rt.settle();
        let src = item_of(&rt, root, "src");
        let mut fonts = super::Fonts::default();
        let mut layer =
            |rt: &qml::runtime::Runtime| cached_layer(&mut rt.world.borrow_mut(), &mut fonts, src);
        let a = layer(&rt);
        assert_eq!(a.px.px[0], [255, 0, 0, 255]);
        let b = layer(&rt);
        assert!(Rc::ptr_eq(&a.px, &b.px), "cached");
        // A change inside the subtree that leaves its pixels as they were: re-rendered once,
        // the equal render dropped, the layer kept.
        write_and_invalidate(&mut rt, root, "tag", Value::Num(1.0));
        assert!(LAYER_CACHE.with_borrow(|c| c[&(src, false)].stale));
        let c = layer(&rt);
        assert!(Rc::ptr_eq(&a.px, &c.px));
        assert!(!LAYER_CACHE.with_borrow(|c| c[&(src, false)].stale));
        write_and_invalidate(&mut rt, root, "alt", Value::Bool(true));
        let d = layer(&rt);
        assert!(!Rc::ptr_eq(&a.px, &d.px), "new pixels, new layer");
        assert_eq!(d.px.px[0], [0, 0, 255, 255]);
        drop((b, c));
        assert_eq!(
            Rc::strong_count(&a.px),
            1,
            "the replaced layer is no longer cached"
        );
    }

    #[test]
    #[cfg_attr(
        not(feature = "interp"),
        ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
    )]
    fn layer_through_its_own_effect_is_cached_apart_from_the_bare_layer() {
        // An item with a layer.effect is rendered bare as that effect's implicit source, and
        // through the effect when another effect samples it: two different textures.
        let mut rt = qml::runtime::Runtime::new(None);
        let root = rt
            .load_str(
                r##"import QtQuick
import Qt5Compat.GraphicalEffects
Item {
    width: 20; height: 20
    property Item src: Item {
        width: 20; height: 20
        layer.enabled: true
        layer.effect: ColorOverlay { color: "#0000ff" }
        Rectangle { width: 10; height: 10; color: "#ff0000" }
    }
}"##,
            )
            .unwrap();
        rt.settle();
        let src = item_of(&rt, root, "src");
        let mut fonts = super::Fonts::default();
        let w = &mut rt.world.borrow_mut();
        let prev = LAYER_SKIP.with(|s| s.replace(Some(src)));
        let bare = cached_layer(w, &mut fonts, src);
        LAYER_SKIP.with(|s| s.set(prev));
        let through = cached_layer(w, &mut fonts, src);
        assert_eq!(bare.px.px[0], [255, 0, 0, 255], "bare source");
        assert_eq!(
            through.px.px[0],
            [0, 0, 255, 255],
            "sampled by another effect: drawn through its ColorOverlay"
        );
        assert_eq!(through.px.px[15 * 20 + 15][3], 0);
        assert!(!Rc::ptr_eq(&bare.px, &through.px));
    }

    #[test]
    #[cfg_attr(
        not(feature = "interp"),
        ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
    )]
    fn a_scale_that_is_one_as_a_float_is_no_transform() {
        // CookieClock's `scale: 1.4 - 0.4 * shown` is 0.9999999999999999 when shown, which is 1
        // in the float matrix Qt gives the node: no layer to draw the subtree through.
        let mut rt = qml::runtime::Runtime::new(None);
        let root = rt
            .load_str(
                r##"import QtQuick
Item {
    property real shown: 1
    property Item clock: Item { width: 10; height: 10; scale: 1.4 - 0.4 * shown }
    property Item spaces: Item { width: 10; height: 10; scale: 0.92 }
}"##,
            )
            .unwrap();
        rt.settle();
        let (clock, spaces) = (item_of(&rt, root, "clock"), item_of(&rt, root, "spaces"));
        let w = &mut rt.world.borrow_mut();
        assert_eq!(num(w, clock, "scale"), 0.9999999999999999);
        assert!(w.node_transform(clock).is_none(), "1 as a float");
        assert!(w.node_transform(spaces).is_some());
    }

    #[test]
    #[cfg_attr(
        not(feature = "interp"),
        ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
    )]
    fn scaled_window_reuses_keyed_rasters_and_canvases() {
        // At a fractional output scale the frame carries a transform; keyed rasters (Shape)
        // and Canvas textures were rasterized (and hashed pixel by pixel) again every frame.
        let item = 7_000_001;
        let mut fr = crate::gpu::Frame::scaled(64, 32, 1.5);
        let n = std::cell::Cell::new(0);
        for _ in 0..3 {
            raster_keyed(
                &mut Target::Gpu(&mut fr),
                item,
                11,
                (2.5, 3.25, 10.5, 6.0),
                |c, m| {
                    n.set(n.get() + 1);
                    // Device pixels 3..20 x 4..14 under (3.75, 4.875, 15.75, 9).
                    assert_eq!((c.w, c.h), (17, 10));
                    assert_eq!(m, [1.5, 1.5, -3.0, -4.0]);
                    c.px[0] = [255, 0, 0, 255];
                },
            );
        }
        assert_eq!(n.get(), 1, "rasterized once, then reused");
        assert!(KEYED.with_borrow(|m| m.contains(&item)));
        // A new key (a theme colour) rasterizes again; a new size too.
        raster_keyed(
            &mut Target::Gpu(&mut fr),
            item,
            12,
            (2.5, 3.25, 10.5, 6.0),
            |_, _| n.set(n.get() + 1),
        );
        raster_keyed(
            &mut Target::Gpu(&mut fr),
            item,
            12,
            (2.5, 3.25, 12.0, 6.0),
            |_, _| n.set(n.get() + 1),
        );
        assert_eq!(n.get(), 3);
        // The raster is drawn on its device pixels at the item's position.
        let draws = fr.sigs().len();
        assert!(draws >= 3, "{draws} draws");
        KEYED.with_borrow_mut(|m| m.remove(&item));
        // A Canvas keeps its ceil-sized texture across frames too.
        let mut rt = qml::runtime::Runtime::new(None);
        let c = rt
            .load_str("import QtQuick\nCanvas { width: 10.5; height: 6 }\n")
            .unwrap();
        let mut w = rt.world.borrow_mut();
        use qml::runtime::CanvasOp;
        w.canvas_ops.insert(
            c,
            vec![
                CanvasOp::SetFill(Value::Str("#ff0000".into())),
                CanvasOp::FillRect([0.0, 0.0, 10.5, 6.0]),
            ],
        );
        for _ in 0..2 {
            canvas_raster(
                &mut w,
                c,
                &mut Target::Gpu(&mut fr),
                2.5,
                3.25,
                10.5,
                6.0,
                1.0,
            );
        }
        let tex = CANVASES.with_borrow_mut(|m| m.get(&c).and_then(|e| e.1.clone()));
        let tex = tex.expect("canvas texture cached under a scale");
        assert_eq!((tex.w, tex.h), (11, 6));
        assert_eq!(tex.px[0], [255, 0, 0, 255]);
        let s = fr.sigs();
        assert_eq!(
            s[s.len() - 1].0,
            s[s.len() - 2].0,
            "both frames draw the same texture"
        );
        CANVASES.with_borrow_mut(|m| m.remove(&c));
    }

    /// A Canvas repainted every frame (the player panel's visualizer) was rasterized and then
    /// copied pixel by pixel into a second image of the same size for its texture. The texture
    /// is the replay itself, with the pixels the copy gave.
    #[test]
    #[cfg_attr(
        not(feature = "interp"),
        ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
    )]
    fn repainted_canvas_texture_is_its_replay_not_a_copy_of_it() {
        use qml::runtime::CanvasOp;
        let mut rt = qml::runtime::Runtime::new(None);
        let c = rt
            .load_str("import QtQuick\nCanvas { width: 42; height: 14 }\n")
            .unwrap();
        let mut w = rt.world.borrow_mut();
        let mut fr = crate::gpu::Frame::new(64, 32);
        let paint = |w: &mut World, k: usize| {
            w.canvas_ops.insert(
                c,
                vec![
                    CanvasOp::SetGlobalAlpha(0.6),
                    CanvasOp::SetFill(Value::Str("#40c0ee".into())),
                    CanvasOp::FillRect([k as f64 + 0.25, 1.5, 20.5, 9.0]),
                ],
            );
            w.bump_canvas(c);
        };
        let tex = || {
            CANVASES
                .with_borrow_mut(|m| m.get(&c).and_then(|e| e.1.clone()))
                .expect("cached")
        };
        CANVAS_TEXTURES.set([0; 2]);
        for k in 0..5 {
            paint(&mut w, k);
            canvas_raster(
                &mut w,
                c,
                &mut Target::Gpu(&mut fr),
                3.0,
                2.0,
                42.0,
                14.0,
                1.0,
            );
            let mut copied = Canvas::new(42, 14, [0, 0, 0, 0]);
            paint_canvas(&mut w, c, &mut copied, 0.0, 0.0, 42.0, 14.0, 1.0);
            let tex = tex();
            assert_eq!(tex.px, copied.px, "paint {k}");
            assert!(tex.px.iter().any(|p| p[3] != 0 && p[3] != 255), "paint {k}");
        }
        assert_eq!(CANVAS_TEXTURES.get(), [5, 0], "[textures, copies]");
        // A texture smaller than the ceil-sized replay is still cut out of it.
        paint(&mut w, 0);
        canvas_raster(
            &mut w,
            c,
            &mut Target::Gpu(&mut fr),
            3.0,
            2.0,
            41.25,
            14.0,
            1.0,
        );
        assert_eq!((tex().w, tex().h), (41, 14));
        assert_eq!(CANVAS_TEXTURES.get(), [6, 1], "[textures, copies]");
        CANVASES.with_borrow_mut(|m| m.remove(&c));
        REPLAYS.with_borrow_mut(|m| m.remove(&c));
    }

    /// Qt renders Shape geometry at device pixels; a logical raster upscaled through the
    /// window scale was soft. Keyed rasters cover device pixels and are drawn 1:1.
    #[test]
    #[cfg_attr(
        not(feature = "interp"),
        ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
    )]
    fn scaled_window_rasterizes_keyed_shapes_at_device_resolution() {
        let item = 7_000_002;
        let mut fr = crate::gpu::Frame::scaled(64, 32, 1.5);
        let mut m = None;
        raster_keyed(
            &mut Target::Gpu(&mut fr),
            item,
            21,
            (2.0, 4.0, 10.0, 6.0),
            |c, xf| {
                m = Some(xf);
                c.px.fill([255, 0, 0, 255]);
            },
        );
        // A 10x6 shape at 1.5 is a 15x9 texture, painted through the device transform.
        assert_eq!(m, Some([1.5, 1.5, -3.0, -6.0]));
        let tex = KEYED.with_borrow_mut(|m| m.get(&item).and_then(|e| e.1.clone()));
        assert_eq!(tex.map(|t| (t.w, t.h)), Some((15, 9)));
        let s = fr.sigs();
        // Draw signatures pad the device rect by one pixel on every side.
        assert_eq!(
            s[s.len() - 1].1,
            [2, 5, 19, 16],
            "drawn 1:1 on device pixels"
        );
        // A fractional device origin keeps the pixel grid (the phase is in the raster and key)
        // and a new scale is a new raster.
        let mut n = 0;
        raster_keyed(
            &mut Target::Gpu(&mut fr),
            item,
            21,
            (2.5, 4.0, 10.0, 6.0),
            |c, xf| {
                n += 1;
                assert_eq!((c.w, c.h), (16, 9));
                assert_eq!(xf, [1.5, 1.5, -3.0, -6.0]);
                c.px.fill([255, 0, 0, 255]);
            },
        );
        let s = fr.sigs();
        assert_eq!(s[s.len() - 1].1, [2, 5, 20, 16]);
        let mut fr2 = crate::gpu::Frame::scaled(64, 32, 2.0);
        raster_keyed(
            &mut Target::Gpu(&mut fr2),
            item,
            21,
            (2.5, 4.0, 10.0, 6.0),
            |c, xf| {
                n += 1;
                assert_eq!((c.w, c.h), (20, 12));
                assert_eq!(xf, [2.0, 2.0, -5.0, -8.0]);
                c.px.fill([255, 0, 0, 255]);
            },
        );
        assert_eq!(n, 2);
        KEYED.with_borrow_mut(|m| m.remove(&item));
        // A real Shape: a filled 20x10 rectangle at (4, 4) and a 4 wide stroke, in a window
        // at 1.5, land on whole device pixels with no blended edge column.
        let mut rt = qml::runtime::Runtime::new(None);
        let win = rt
            .load_str(
                r##"import QtQuick
import QtQuick.Shapes
import Rustshell
FloatingWindow {
    width: 40; height: 20
    color: "transparent"
    Shape {
        width: 30; height: 20
        ShapePath {
            fillColor: "#ff0000"; strokeColor: "transparent"
            startX: 4; startY: 4
            PathLine { x: 24; y: 4 } PathLine { x: 24; y: 14 } PathLine { x: 4; y: 14 }
        }
        ShapePath {
            fillColor: "transparent"; strokeColor: "#0000ff"; strokeWidth: 4
            capStyle: ShapePath.FlatCap
            startX: 30; startY: 4
            PathLine { x: 30; y: 14 }
        }
    }
}"##,
            )
            .unwrap();
        let mut fonts = Fonts::default();
        layout_window(&mut rt, &mut fonts, win, [40.0, 20.0]);
        let mut fr = crate::gpu::Frame::scaled(40, 20, 1.5);
        paint_window(&mut rt, &mut fonts, win, &mut Target::Gpu(&mut fr));
        let w = rt.world.borrow();
        let shape = w.items[win].children[0];
        let tex = KEYED.with_borrow_mut(|m| m.get(&shape).and_then(|e| e.1.clone()));
        let tex = tex.expect("shape raster cached");
        // (-8, -8, 46, 36) logical -> (-12, -12, 69, 54) device, clipped to the 60x30 window.
        assert_eq!((tex.w, tex.h), (57, 30));
        let at = |x: usize, y: usize| tex.px[y * tex.w + x];
        assert_eq!(at(5, 10), [0, 0, 0, 0]);
        assert_eq!(at(6, 10), [255, 0, 0, 255]);
        assert_eq!(at(35, 10), [255, 0, 0, 255]);
        assert_eq!(at(36, 10), [0, 0, 0, 0]);
        assert_eq!(at(20, 5), [0, 0, 0, 0]);
        assert_eq!(at(20, 6), [255, 0, 0, 255]);
        assert_eq!(at(20, 20), [255, 0, 0, 255]);
        assert_eq!(at(20, 21), [0, 0, 0, 0]);
        // The stroke is 4 logical = 6 device pixels wide (x 42..48), not 4 stretched ones.
        let cols: Vec<usize> = (36..57).filter(|&x| at(x, 15)[3] >= 128).collect();
        assert_eq!(cols, (42..48).collect::<Vec<_>>());
        assert!(
            at(42, 15)[3] >= 200 && at(47, 15)[3] >= 200,
            "{:?} {:?}",
            at(42, 15),
            at(47, 15)
        );
        assert_eq!(at(44, 15), [0, 0, 255, 255]);
        let s = fr.sigs();
        assert_eq!(s[s.len() - 1].1, [0, 0, 58, 30]);
        drop(w);
        KEYED.with_borrow_mut(|m| m.remove(&shape));
    }

    #[test]
    #[cfg_attr(
        not(feature = "interp"),
        ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
    )]
    fn scaled_window_keeps_rasters_apart_per_item() {
        // Two Shapes with the same key (the same colours) in different items, or one item at a
        // new geometry, are separate rasters under a scale: each draws its own pixels at its
        // own place and size, and each is rasterized once.
        let mut fr = crate::gpu::Frame::scaled(64, 32, 1.5);
        let (a, b) = (7_000_011, 7_000_012);
        let n = std::cell::Cell::new(0);
        for _ in 0..2 {
            raster_keyed(
                &mut Target::Gpu(&mut fr),
                a,
                11,
                (2.5, 3.25, 10.5, 6.0),
                |c, _| {
                    n.set(n.get() + 1);
                    c.px[0] = [255, 0, 0, 255];
                },
            );
            raster_keyed(
                &mut Target::Gpu(&mut fr),
                b,
                11,
                (20.0, 5.0, 8.0, 8.0),
                |c, _| {
                    n.set(n.get() + 1);
                    c.px[0] = [0, 0, 255, 255];
                },
            );
        }
        assert_eq!(n.get(), 2, "one raster per item, then reused");
        let cached = |i: usize| KEYED.with_borrow_mut(|m| m.get(&i).and_then(|e| e.1.clone()));
        let (ca, cb) = (cached(a).expect("a cached"), cached(b).expect("b cached"));
        // Device-pixel rasters: (3.75, 4.875, 15.75, 9) and (30, 7.5, 12, 12) at 1.5.
        assert_eq!(((ca.w, ca.h), ca.px[0]), ((17, 10), [255, 0, 0, 255]));
        assert_eq!(((cb.w, cb.h), cb.px[0]), ((12, 13), [0, 0, 255, 255]));
        let s = fr.sigs().to_vec();
        let l = &s[s.len() - 4..];
        assert_eq!(
            (l[0], l[1]),
            (l[2], l[3]),
            "the second frame draws the same textures"
        );
        assert_ne!(l[0], l[1], "different rasters");
        // Device boxes at 1.5x the logical positions (a 1px signature margin).
        assert!(l[0].1[0] <= 3 && l[0].1[2] >= 19, "{:?}", l[0]);
        assert!(l[1].1[0] >= 29 && l[1].1[2] >= 41, "{:?}", l[1]);
        // The same item at a new place (same device-pixel phase) keeps its raster but draws it there.
        raster_keyed(
            &mut Target::Gpu(&mut fr),
            b,
            11,
            (4.0, 21.0, 8.0, 8.0),
            |_, _| n.set(n.get() + 1),
        );
        assert_eq!(n.get(), 2);
        let s = fr.sigs().to_vec();
        assert!(s[s.len() - 1].1[1] >= 30, "{:?}", s[s.len() - 1]);
        KEYED.with_borrow_mut(|m| {
            m.remove(&a);
            m.remove(&b);
        });

        // Two Canvas items of one size with different drawings keep separate textures.
        let mut rt = qml::runtime::Runtime::new(None);
        let root = rt
            .load_str(
                "import QtQuick\nItem { Canvas { width: 10.5; height: 6 } Canvas { width: 10.5; height: 6 } }\n",
            )
            .unwrap();
        let mut w = rt.world.borrow_mut();
        let cs: Vec<usize> = w.items[root]
            .children
            .iter()
            .copied()
            .filter(|&c| w.items[c].types.iter().any(|t| t == "Canvas"))
            .collect();
        assert_eq!(cs.len(), 2);
        use qml::runtime::CanvasOp;
        for (c, col) in cs.iter().zip(["#ff0000", "#0000ff"]) {
            w.canvas_ops.insert(
                *c,
                vec![
                    CanvasOp::SetFill(Value::Str(col.into())),
                    CanvasOp::FillRect([0.0, 0.0, 10.5, 6.0]),
                ],
            );
        }
        for _ in 0..2 {
            canvas_raster(
                &mut w,
                cs[0],
                &mut Target::Gpu(&mut fr),
                2.5,
                3.25,
                10.5,
                6.0,
                1.0,
            );
            canvas_raster(
                &mut w,
                cs[1],
                &mut Target::Gpu(&mut fr),
                30.0,
                3.25,
                10.5,
                6.0,
                1.0,
            );
        }
        let tex = |c: usize| {
            CANVASES
                .with_borrow_mut(|m| m.get(&c).and_then(|e| e.1.clone()))
                .expect("cached")
        };
        assert_eq!(tex(cs[0]).px[0], [255, 0, 0, 255]);
        assert_eq!(tex(cs[1]).px[0], [0, 0, 255, 255]);
        let s = fr.sigs().to_vec();
        let l = &s[s.len() - 4..];
        assert_eq!((l[0], l[1]), (l[2], l[3]));
        assert_ne!(l[0].0, l[1].0);
        assert!(l[1].1[0] >= 44, "{:?}", l[1]);
        for c in cs {
            CANVASES.with_borrow_mut(|m| m.remove(&c));
        }
    }

    #[test]
    #[cfg_attr(
        not(feature = "interp"),
        ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
    )]
    fn offscreen_canvas_keeps_its_subtree_out_of_the_display_list() {
        // A Canvas below the window is skipped, which left its parent's recorded draw complete
        // without it: a clock sliding in from off screen replayed that draw and lost the Canvas
        // until something else repainted the subtree (seen at a live 1 -> 1.5 scale change).
        let mut rt = qml::runtime::Runtime::new(None);
        let c = rt
            .load_str("import QtQuick\nCanvas { width: 10; height: 10 }\n")
            .unwrap();
        let mut w = rt.world.borrow_mut();
        use qml::runtime::CanvasOp;
        w.canvas_ops.insert(
            c,
            vec![
                CanvasOp::SetFill(Value::Str("#ff0000".into())),
                CanvasOp::FillRect([0.0, 0.0, 10.0, 10.0]),
            ],
        );
        for dpr in [1.0, 1.5] {
            let mut fr = crate::gpu::Frame::scaled(64, 32, dpr);
            NOCACHE.with(|n| n.set(false));
            canvas_raster(
                &mut w,
                c,
                &mut Target::Gpu(&mut fr),
                2.0,
                40.0,
                10.0,
                10.0,
                1.0,
            );
            assert!(fr.sigs().is_empty(), "dpr {dpr}: nothing drawn off screen");
            assert!(
                NOCACHE.with(|n| n.get()),
                "dpr {dpr}: the subtree is not cacheable"
            );
            NOCACHE.with(|n| n.set(false));
            canvas_raster(
                &mut w,
                c,
                &mut Target::Gpu(&mut fr),
                2.0,
                4.0,
                10.0,
                10.0,
                1.0,
            );
            assert_eq!(fr.sigs().len(), 1, "dpr {dpr}: drawn on screen");
            assert!(
                !NOCACHE.with(|n| n.get()),
                "dpr {dpr}: an on-screen Canvas is cacheable"
            );
        }
        CANVASES.with_borrow_mut(|m| m.remove(&c));
    }

    #[test]
    fn raster_caches_evict_least_recent_instead_of_clearing() {
        // The old caches dropped every raster on the 1025th/513th entry.
        for i in 0..1025usize {
            KEYED.with_borrow_mut(|m| m.insert(i, ((0, 0, 0, 1, 1), None), 0));
            if i % 100 == 0 {
                assert!(
                    KEYED.with_borrow_mut(|m| m.get(&0).is_some()),
                    "kept while in use"
                );
            }
        }
        KEYED.with_borrow(|m| {
            assert_eq!(m.len(), 1024);
            assert!(m.contains(&0) && m.contains(&1024) && !m.contains(&1));
        });
        // Byte bound: 1 MiB canvases never exceed the 64 MiB budget.
        for i in 0..200usize {
            let c = canvas(512, 512);
            let n = canvas_bytes(&c);
            CANVASES.with_borrow_mut(|m| m.insert(i, ((0, 512, 512, 0, 0), Some(c)), n));
        }
        CANVASES.with_borrow(|m| {
            assert!(m.bytes() <= m.budget(), "{} bytes", m.bytes());
            assert!(m.contains(&199) && !m.contains(&0));
        });
    }

    #[test]
    fn a_theme_change_forgets_icons_and_keeps_pictures() {
        let wallpaper = ("/home/u/Pictures/wall.jpg".to_string(), (2055, 1160), 2);
        let remote = ("https://example.org/cover.png".to_string(), (0, 0), 0);
        let icons = [
            ("image://icon/folder".to_string(), (32, 32), PROVIDER_FILL),
            (
                "image://kicon//usr/share/icons/t/folder.svg".to_string(),
                (32, 32),
                1,
            ),
            (
                "image://service.sni/a/1".to_string(),
                (24, 24),
                PROVIDER_FILL,
            ),
        ];
        for k in [&wallpaper, &remote].into_iter().chain(&icons) {
            cache_image(k.clone(), Some(canvas(8, 8)));
        }
        forget_themed_images();
        IMAGES.with_borrow(|c| {
            assert!(c.contains(&wallpaper), "the wallpaper is not decoded again");
            assert!(c.contains(&remote));
            for k in &icons {
                assert!(!c.contains(k), "{} is asked of the new theme", k.0);
            }
        });
        IMAGES.with_borrow_mut(|c| c.clear());
    }

    #[test]
    fn unheld_images_are_evicted_by_bytes_and_failures_forgotten() {
        let key = |n: usize| (format!("image://qsimage/{n}"), (0, 0), 0);
        let shown = canvas(1024, 1024);
        cache_image(key(0), Some(shown.clone()));
        // Notification images and versioned tray icons nobody draws any more.
        for n in 1..40 {
            cache_image(key(n), Some(canvas(1024, 1024)));
        }
        IMAGES.with_borrow(|c| {
            assert!(c.bytes() <= IMAGES_BYTES + image_bytes(&key(0), &Some(shown.clone())));
            assert!(c.contains(&key(0)), "a drawn image stays cached");
            assert!(c.contains(&key(39)) && !c.contains(&key(1)));
        });
        FAILED.with_borrow_mut(|f| {
            f.insert(key(39), None);
            f.insert(key(1), None);
        });
        prune_images();
        FAILED.with_borrow(|f| {
            assert!(f.contains_key(&key(39)));
            assert!(
                !f.contains_key(&key(1)),
                "failure of an evicted image forgotten"
            );
        });
        drop(shown);
        IMAGES.with_borrow_mut(|c| c.evict(None));
        IMAGES.with_borrow(|c| assert!(c.bytes() <= IMAGES_BYTES));
    }

    /// A failed decode belongs to the bytes it read, and a source with no file has no failed
    /// decode. The file of an Image may be written while the Image loads (a thumbnail): noting
    /// the failure with the time the file has later, at a second lookup, marked the finished
    /// file as failed and the Image stayed at Error.
    #[test]
    fn a_failed_decode_is_noted_for_the_bytes_it_read() {
        let rt = qml::runtime::Runtime::new(None);
        let w = rt.world.borrow();
        let loads = image_loads(&w);
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("thumb.png");
        let src = file.to_str().unwrap().to_string();
        let key = (src.clone(), (0, 0), 0);
        let write = |data: &[u8], secs: u64| {
            std::fs::write(&file, data).unwrap();
            let t = std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs);
            let f = std::fs::File::options().write(true).open(&file).unwrap();
            f.set_modified(t).unwrap();
        };
        let load = || image_texture(loads, &src, (0, 0), 0, true);
        // The worker's result, taken as the frame loop takes it.
        let landed = || {
            let m = loads.rx.recv().unwrap();
            let Loaded::Decoded(k, t, read) = m else {
                panic!("not a decode");
            };
            loads.decoded.borrow_mut().push((k, t, read));
        };
        let png = {
            let p = dir.path().join("whole.png");
            canvas(2, 2).save_png(p.to_str().unwrap()).unwrap();
            std::fs::read(p).unwrap()
        };

        // No file yet: the reader's thread finds that out, the Image is Loading until then as
        // in Qt 6.12 (`written asynchronous, no such file` of tests/sync_load). Then Error
        // with nothing noted, so the file loads once it is there.
        assert!(load().is_none() && decode_pending(loads, &src));
        landed();
        assert_eq!(take_decoded(&w), std::slice::from_ref(&src));
        assert!(load().is_none() && !decode_pending(loads, &src));
        FAILED.with_borrow(|f| assert!(!f.contains_key(&key)));

        // Half of the file is there when the worker reads it; the writer ends before the
        // frame loop takes the result.
        write(&png[..png.len() / 2], 1_700_000_000);
        assert!(load().is_none() && decode_pending(loads, &src));
        landed();
        write(&png, 1_700_000_001);
        assert_eq!(take_decoded(&w), std::slice::from_ref(&src));
        assert!(
            load().is_none() && decode_pending(loads, &src),
            "the finished file is not decoded"
        );
        landed();
        assert_eq!(take_decoded(&w), std::slice::from_ref(&src));
        assert!(load().is_some());

        // A file that stays as it failed is not decoded again.
        IMAGES.with_borrow_mut(|c| c.remove(&key));
        write(&png[..png.len() / 2], 1_700_000_002);
        assert!(load().is_none() && decode_pending(loads, &src));
        landed();
        assert_eq!(take_decoded(&w), std::slice::from_ref(&src));
        assert!(load().is_none() && !decode_pending(loads, &src));
    }

    #[cfg(feature = "interp")]
    #[test]
    fn view_delegates_anchor_to_content_item_inside_margins() {
        let mut rt = qml::runtime::Runtime::new(None);
        let root = rt
            .load_str(
                r#"import QtQuick
Item {
    id: root
    width: 300; height: 200
    property Item del
    property Item plain: inner
    ListView {
        id: lv
        anchors.fill: parent
        leftMargin: 20; rightMargin: 30; topMargin: 5; bottomMargin: 7
        model: 1
        delegate: Rectangle {
            anchors { left: parent?.left; right: parent?.right }
            height: 10
            Component.onCompleted: root.del = this
        }
    }
    Flickable {
        anchors.fill: parent
        leftMargin: 4; rightMargin: 6; topMargin: 3; bottomMargin: 5
        Item { id: inner; anchors.fill: parent }
    }
}"#,
            )
            .unwrap();
        let mut fonts = super::Fonts::default();
        for _ in 0..3 {
            rt.poll_native();
            rt.settle();
            super::layout_window(&mut rt, &mut fonts, root, [300.0, 200.0]);
        }
        let item = |rt: &qml::runtime::Runtime, k: &str| match rt.get(root, k) {
            qml::value::Value::Item(i) => i,
            v => panic!("{k} {v:?}"),
        };
        let d = item(&rt, "del");
        assert_eq!(rt.get(d, "width").to_number(), 250.0);
        // The row starts inside the left and top margins, as the original volume mixer rows do.
        let (x, y) = rt.world.borrow_mut().scene_pos(d, Some(root));
        assert_eq!((x, y), (20.0, 5.0));
        let p = item(&rt, "plain");
        assert_eq!(rt.get(p, "width").to_number(), 290.0);
        assert_eq!(rt.get(p, "height").to_number(), 192.0);
    }

    /// Children declared in a ListView or GridView are children of the view, not of its
    /// contentItem (the original's "No applications" PagePlaceholder in the volume mixer's list): they
    /// anchor to the whole view, margins included, and do not scroll. Geometry as Qt 6.12
    /// reports it for the same document (`mapToItem(view, 0, 0)`, width, height).
    #[cfg(feature = "interp")]
    #[test]
    fn view_children_anchor_to_the_view_not_its_content_item() {
        let mut rt = qml::runtime::Runtime::new(None);
        let root = rt
            .load_str(
                r#"import QtQuick
Item {
    id: root
    width: 300; height: 200
    property Item emptyView: empty
    property Item emptyFill: ph
    property Item emptyCentre: mid
    property Item fullView: full
    property Item fullFill: ph2
    property Item fullPlain: plain2
    property Item gridView: grid
    property Item gridFill: ph3
    ListView {
        id: empty
        anchors.fill: parent
        leftMargin: 20; rightMargin: 30; topMargin: 14; bottomMargin: 12
        model: 0
        delegate: Item { height: 10; width: 10 }
        Item { id: ph; anchors.fill: parent }
        Item { id: mid; width: 100; height: 40; anchors.centerIn: parent }
    }
    ListView {
        id: full
        anchors.fill: parent
        leftMargin: 20; rightMargin: 30; topMargin: 14; bottomMargin: 12
        model: 50
        delegate: Item { height: 10; width: 10 }
        Item { id: ph2; anchors.fill: parent }
        Item { id: plain2; x: 3; y: 4; width: 10; height: 10 }
    }
    GridView {
        id: grid
        anchors.fill: parent
        leftMargin: 20; rightMargin: 30; topMargin: 14; bottomMargin: 12
        model: 0
        delegate: Item { height: 10; width: 10 }
        Item { id: ph3; anchors.fill: parent }
    }
}"#,
            )
            .unwrap();
        let mut fonts = super::Fonts::default();
        let mut layout = |rt: &mut qml::runtime::Runtime| {
            for _ in 0..3 {
                rt.poll_native();
                rt.settle();
                super::layout_window(rt, &mut fonts, root, [300.0, 200.0]);
            }
        };
        layout(&mut rt);
        let item = |rt: &qml::runtime::Runtime, k: &str| match rt.get(root, k) {
            qml::value::Value::Item(i) => i,
            v => panic!("{k} {v:?}"),
        };
        // [x, y, width, height] of `c` in its view, as drawn.
        let in_view = |rt: &qml::runtime::Runtime, view: &str, c: &str| {
            let (v, c) = (item(rt, view), item(rt, c));
            let g = |k: &str| rt.get(c, k).to_number();
            let (x, y) = rt.world.borrow_mut().scene_pos(c, Some(v));
            [x, y, g("width"), g("height")]
        };
        assert_eq!(
            in_view(&rt, "emptyView", "emptyFill"),
            [0.0, 0.0, 300.0, 200.0]
        );
        assert_eq!(
            in_view(&rt, "emptyView", "emptyCentre"),
            [100.0, 80.0, 100.0, 40.0]
        );
        assert_eq!(
            in_view(&rt, "fullView", "fullFill"),
            [0.0, 0.0, 300.0, 200.0]
        );
        assert_eq!(
            in_view(&rt, "fullView", "fullPlain"),
            [3.0, 4.0, 10.0, 10.0]
        );
        assert_eq!(
            in_view(&rt, "gridView", "gridFill"),
            [0.0, 0.0, 300.0, 200.0]
        );
        let full = item(&rt, "fullView");
        rt.world
            .borrow_mut()
            .write(full, "contentY", qml::value::Value::Num(86.0));
        layout(&mut rt);
        assert_eq!(
            in_view(&rt, "fullView", "fullFill"),
            [0.0, 0.0, 300.0, 200.0]
        );
        assert_eq!(
            in_view(&rt, "fullView", "fullPlain"),
            [3.0, 4.0, 10.0, 10.0]
        );
    }

    #[test]
    #[cfg_attr(
        not(feature = "interp"),
        ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
    )]
    fn path_polyline_moves_to_first_point_then_lines() {
        let mut rt = qml::runtime::Runtime::new(None);
        let root = rt
            .load_str(
                "import QtQuick\nimport QtQuick.Shapes\nShape { id: shape; property list<point> pts: []\n \
                 function add() { pts.push({x: 10, y: 20}); pts.push({x: 30, y: 40}) }\n \
                 ShapePath { PathPolyline { path: [Qt.point(50, 50), Qt.point(150, 80)] } }\n \
                 ShapePath { PathPolyline { path: shape.pts } } }",
            )
            .unwrap();
        rt.settle();
        rt.call_method(root, "add", Vec::new()).unwrap().unwrap();
        rt.settle();
        let mut w = rt.world.borrow_mut();
        let paths = w.items[root].children.clone();
        let line = |w: &mut qml::runtime::World, sp: usize| {
            let e = w.items[sp].children[0];
            super::polyline(w, e)
        };
        assert_eq!(line(&mut w, paths[0]), vec![(50.0, 50.0), (150.0, 80.0)]);
        assert_eq!(line(&mut w, paths[1]), vec![(10.0, 20.0), (30.0, 40.0)]);
        assert_eq!(super::shape_bounds(&mut w, root), (150.0, 80.0));
    }

    #[test]
    #[cfg_attr(
        not(feature = "interp"),
        ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
    )]
    fn column_skips_zero_width_gap() {
        let mut rt = qml::runtime::Runtime::new(None);
        let root = rt
            .load_str(
                "import QtQuick\nItem { width: 100; height: 100\n Column { spacing: 6\n \
                 Rectangle { width: 10; height: 10 }\n Item { implicitHeight: 20 }\n \
                 Rectangle { width: 10; height: 10 } } }",
            )
            .unwrap();
        rt.settle();
        super::layout_window(&mut rt, &mut super::Fonts::default(), root, [100.0, 100.0]);
        let mut w = rt.world.borrow_mut();
        let col = w.items[root].children[0];
        let last = *w.items[col].children.last().unwrap();
        assert_eq!(super::num(&mut w, last, "y"), 16.0);
        assert_eq!(super::num(&mut w, col, "height"), 26.0);
    }

    #[test]
    #[cfg_attr(
        not(feature = "interp"),
        ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
    )]
    fn popup_adjustment_defaults_to_slide_like_quickshell() {
        let dir = std::env::temp_dir().join(format!("ri-popadj-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("shell.qml");
        std::fs::write(
            &f,
            "import Rustshell\nimport QtQuick\nItem {\n\
             PopupWindow { objectName: \"unset\" }\n\
             PopupWindow { objectName: \"none\"; anchor.adjustment: PopupAdjustment.None }\n\
             PopupWindow { objectName: \"flip\"; anchor.adjustment: PopupAdjustment.Flip | PopupAdjustment.ResizeY }\n}\n",
        )
        .unwrap();
        let mut rt = qml::runtime::Runtime::new(Some(dir.clone()));
        rt.load_file(&f).expect("load");
        let mut got = Vec::new();
        {
            let mut w = rt.world.borrow_mut();
            for i in w.of_type("PopupWindow") {
                let name = match w.read(i, "objectName") {
                    Some(qml::value::Value::Str(s)) => s.to_string(),
                    _ => String::new(),
                };
                got.push((
                    name,
                    super::popup_adjustment(w.read(i, "anchor.adjustment")),
                ));
            }
        }
        got.sort();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            got,
            [
                ("flip".to_string(), 12 | 32),
                ("none".to_string(), 0),
                ("unset".to_string(), 3)
            ]
        );
    }

    #[test]
    fn html_entity_scan_respects_char_boundaries() {
        let runs = super::html_runs("A &B1234567…x", [0, 0, 255, 255], false);
        let text: String = runs.iter().map(|r| r.0.as_str()).collect();
        assert!(text.contains('…'));
        assert_eq!(super::floor_boundary("ab…", 3), 2);
    }

    /// welcome.qml/settings.qml ContentPage: a column `width: Math.max(base, implicitWidth)`
    /// over a Flow, whose implicit width depends on the width it wraps at, has two consistent
    /// layouts (wrapped at 600, one row at 750). It must settle on one, not alternate each pass.
    #[test]
    #[cfg_attr(
        not(feature = "interp"),
        ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
    )]
    fn flow_under_implicit_width_column_settles() {
        let mut rt = qml::runtime::Runtime::new(None);
        let root = rt
            .load_str(
                "import QtQuick\nimport QtQuick.Layouts\nItem {\n property var col: c\n property var flow: f\n\
                 ColumnLayout { id: c\n  width: Math.max(600, implicitWidth)\n\
                 Flow { id: f; Layout.fillWidth: true\n\
                 Item { width: 250; height: 20 }\n Item { width: 250; height: 20 }\n Item { width: 250; height: 20 } } } }\n",
            )
            .unwrap();
        rt.settle();
        let mut fonts = super::Fonts::default();
        let item =
            |rt: &mut qml::runtime::Runtime, k: &str| match rt.world.borrow_mut().read(root, k) {
                Some(qml::value::Value::Item(i)) => i,
                _ => panic!("no {k}"),
            };
        let (c, f) = (item(&mut rt, "col"), item(&mut rt, "flow"));
        let mut seen = Vec::new();
        for _ in 0..4 {
            super::layout_window(&mut rt, &mut fonts, root, [1000.0, 800.0]);
            rt.settle();
            seen.push((
                rt.get(c, "width").to_number(),
                rt.get(f, "height").to_number(),
            ));
        }
        assert!(
            seen.windows(2).all(|p| p[0] == p[1]),
            "layout never settles: {seen:?}"
        );
        assert_eq!(seen[0], (750.0, 20.0), "{seen:?}");
    }

    #[test]
    #[cfg_attr(
        not(feature = "interp"),
        ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
    )]
    fn layout_child_shown_by_settle_is_placed() {
        // SearchItem's "Open" label: `visible` flips in a settle after placement moved nothing.
        let x = |show: bool| {
            let mut rt = qml::runtime::Runtime::new(None);
            let root = rt
                .load_str(&format!(
                    "import QtQuick\nimport QtQuick.Layouts\nItem {{\n property bool show: {show}\n RowLayout {{\n  anchors.fill: parent\n  spacing: 10\n  Item {{ implicitWidth: 30; implicitHeight: 10 }}\n  Item {{ implicitWidth: 20; implicitHeight: 10; visible: show }}\n }}\n}}\n"
                ))
                .unwrap();
            rt.settle();
            let mut fonts = super::Fonts::default();
            super::layout_window(&mut rt, &mut fonts, root, [400.0, 50.0]);
            let show = qml::value::Value::Bool(true);
            rt.world.borrow_mut().write(root, "show", show);
            super::layout_window(&mut rt, &mut fonts, root, [400.0, 50.0]);
            let w = rt.world.borrow();
            let shown = w.items[w.items[root].children[0]].children[1];
            drop(w);
            assert!(rt.get(shown, "visible").truthy());
            rt.get(shown, "x").to_number()
        };
        let placed = x(true);
        assert!(placed > 30.0);
        assert_eq!(x(false), placed);
    }

    #[test]
    fn screencopy_implicit_size_follows_quickshell() {
        use super::screencopy_implicit as f;
        // No frame yet: QSizeF::scaled of an empty size returns the constraint.
        assert_eq!(f([0.0, 0.0], [300.0, 200.0]), (300.0, 200.0));
        assert_eq!(f([1920.0, 1080.0], [300.0, 200.0]), (300.0, 168.75));
        assert_eq!(f([800.0, 1000.0], [300.0, 200.0]), (160.0, 200.0));
        assert_eq!(f([100.0, 50.0], [300.0, 200.0]), (300.0, 150.0));
        assert_eq!(f([640.0, 480.0], [0.0, 0.0]), (640.0, 480.0));
        assert_eq!(f([640.0, 480.0], [300.0, 0.0]), (300.0, 1.6));
    }

    /// The lock screen on a 1920x1080 screen at scale 1.25 (Background.qml's `blurLoader`): the
    /// wallpaper blurred by `lock.blur.radius` in a Loader scaled by `lock.blur.extraZoom`.
    #[cfg(feature = "interp")]
    #[test]
    fn a_blur_in_a_scaled_subtree_reads_no_more_than_two_passes_over_its_pixels() {
        let mut rt = qml::runtime::Runtime::new(None);
        let root = rt
            .load_str(
                r##"import QtQuick
import Qt5Compat.GraphicalEffects
Item {
    width: 1536; height: 864
    Rectangle {
        id: wallpaper
        width: 1643.52; height: 924.48
        gradient: Gradient {
            GradientStop { position: 0; color: "#ff3366" }
            GradientStop { position: 1; color: "#3366ff" }
        }
    }
    Item {
        width: 1643.52; height: 924.48
        scale: 1.1
        GaussianBlur { width: 1643.52; height: 924.48; source: wallpaper; radius: 100; samples: 201 }
    }
}"##,
            )
            .unwrap();
        rt.settle();
        let mut fonts = Fonts::default();
        let w = &mut rt.world.borrow_mut();
        measure(w, &mut fonts, root);
        let mut c = Canvas::new(1536, 864, [0, 0, 0, 0]);
        let before = qsg::blur::texel_reads();
        let cv = &mut Target::Cpu(&mut c);
        paint(w, &mut fonts, root, cv, 0.0, 0.0, 1.0, false);
        let reads = qsg::blur::texel_reads() - before;
        // Two passes of 201 taps: the first on whole texels, a texel a tap; the second
        // between rows of texels, two a tap, as the filter of a texture reads them.
        let most = 1_352_083_344;
        assert!(
            reads > 0 && reads <= most,
            "{reads} texels read, {most} at most"
        );
    }

    /// DockApps' preview popup sizes its column from the ScreencopyView's implicit size.
    #[cfg(feature = "interp")]
    #[test]
    fn screencopy_view_sizes_its_layout() {
        let mut rt = qml::runtime::Runtime::new(None);
        let root = rt
            .load_str(
                "import QtQuick\nimport QtQuick.Layouts\nimport Quickshell\nimport Rustshell.Wayland\n\
                 FloatingWindow {\n implicitWidth: 800; implicitHeight: 600\n property var col: c\n property var view: v\n\
                 ColumnLayout { id: c\n implicitWidth: v.implicitWidth\n implicitHeight: v.implicitHeight\n\
                 Item { Layout.fillWidth: true; Layout.fillHeight: true\n implicitWidth: v.width; implicitHeight: v.height\n\
                 ScreencopyView { id: v; anchors.centerIn: parent\n constraintSize: Qt.size(300, 200) } } } }\n",
            )
            .unwrap();
        rt.settle();
        let mut fonts = super::Fonts::default();
        let size = |rt: &mut qml::runtime::Runtime, fonts: &mut super::Fonts| {
            super::layout_window(rt, fonts, root, [800.0, 600.0]);
            rt.settle();
            super::layout_window(rt, fonts, root, [800.0, 600.0]);
            let mut w = rt.world.borrow_mut();
            let Some(qml::value::Value::Item(v)) = w.read(root, "view") else {
                panic!("no view")
            };
            let Some(qml::value::Value::Item(c)) = w.read(root, "col") else {
                panic!("no col")
            };
            let g = |w: &mut qml::runtime::World, i, n| w.read(i, n).map_or(0.0, |v| v.to_number());
            [
                g(&mut w, v, "implicitWidth"),
                g(&mut w, v, "implicitHeight"),
                g(&mut w, v, "width"),
                g(&mut w, c, "implicitWidth"),
                g(&mut w, c, "implicitHeight"),
            ]
        };
        assert_eq!(
            size(&mut rt, &mut fonts),
            [300.0, 200.0, 300.0, 300.0, 200.0]
        );
        {
            let mut w = rt.world.borrow_mut();
            let Some(qml::value::Value::Item(v)) = w.read(root, "view") else {
                panic!("no view")
            };
            let sz = qml::eval::new_obj();
            qml::eval::set_own(&sz, "width", qml::value::Value::Num(1920.0));
            qml::eval::set_own(&sz, "height", qml::value::Value::Num(1080.0));
            w.write(v, "sourceSize", qml::value::Value::Obj(sz));
        }
        rt.settle();
        assert_eq!(
            size(&mut rt, &mut fonts),
            [300.0, 168.75, 300.0, 300.0, 169.0]
        );
        {
            let mut w = rt.world.borrow_mut();
            let Some(qml::value::Value::Item(v)) = w.read(root, "view") else {
                panic!("no view")
            };
            let sz = qml::eval::new_obj();
            qml::eval::set_own(&sz, "width", qml::value::Value::Num(800.0));
            qml::eval::set_own(&sz, "height", qml::value::Value::Num(1000.0));
            w.write(v, "sourceSize", qml::value::Value::Obj(sz));
        }
        rt.settle();
        assert_eq!(
            size(&mut rt, &mut fonts),
            [160.0, 200.0, 160.0, 160.0, 200.0]
        );
    }

    #[test]
    #[cfg_attr(
        not(feature = "interp"),
        ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
    )]
    fn layout_implicit_binding_wins_over_stale_children() {
        // Dock window preview: the entry's ColumnLayout binds `implicitWidth` to its capture,
        // while a child (ButtonGroup) is as wide as the layout. A portrait frame must shrink it.
        let mut rt = qml::runtime::Runtime::new(None);
        let root = rt
            .load_str(
                "import QtQuick\nimport QtQuick.Controls\nimport QtQuick.Layouts\nimport Quickshell\nimport Rustshell.Wayland\n\
                 FloatingWindow {\n implicitWidth: 800; implicitHeight: 600\n property var col: c\n property var view: v\n\
                 RowLayout { Button { padding: 0\n contentItem: ColumnLayout { id: c\n implicitWidth: v.implicitWidth\n implicitHeight: v.implicitHeight\n\
                 Rectangle { implicitWidth: parent.width; implicitHeight: 30 }\n\
                 Item { Layout.fillWidth: true; Layout.fillHeight: true\n implicitWidth: v.width; implicitHeight: v.height\n\
                 ScreencopyView { id: v; anchors.centerIn: parent\n constraintSize: Qt.size(300, 200) } } } } } }\n",
            )
            .unwrap();
        rt.settle();
        let mut fonts = super::Fonts::default();
        let item = |rt: &mut qml::runtime::Runtime, n| match rt.world.borrow_mut().read(root, n) {
            Some(qml::value::Value::Item(i)) => i,
            _ => panic!("no {n}"),
        };
        let (v, c) = (item(&mut rt, "view"), item(&mut rt, "col"));
        let size = |rt: &mut qml::runtime::Runtime, fonts: &mut super::Fonts| {
            for _ in 0..4 {
                super::layout_window(rt, fonts, root, [800.0, 600.0]);
                rt.settle();
            }
            let mut w = rt.world.borrow_mut();
            let mut g = |i, n| w.read(i, n).map_or(0.0, |v| v.to_number());
            [g(c, "implicitWidth"), g(c, "width"), g(v, "width")]
        };
        assert_eq!(size(&mut rt, &mut fonts), [300.0, 300.0, 300.0]);
        {
            let sz = qml::eval::new_obj();
            qml::eval::set_own(&sz, "width", qml::value::Value::Num(800.0));
            qml::eval::set_own(&sz, "height", qml::value::Value::Num(1000.0));
            rt.world
                .borrow_mut()
                .write(v, "sourceSize", qml::value::Value::Obj(sz));
        }
        rt.settle();
        assert_eq!(size(&mut rt, &mut fonts), [160.0, 160.0, 160.0]);
    }

    #[test]
    #[cfg_attr(
        not(feature = "interp"),
        ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
    )]
    fn panel_without_screen_leaves_output_to_compositor() {
        use qml::value::Value;
        let mut rt = qml::runtime::Runtime::new(None);
        let root = rt
            .load_str(
                r#"import QtQuick
                import Rustshell
                Item {
                    property var cfg: ({})
                    property var unset: PanelWindow {}
                    property var failed: PanelWindow { screen: cfg.forceMonitor.enable ? null : null }
                    property var second: PanelWindow { screen: Rustshell.screens[1] ?? null }
                }"#,
            )
            .unwrap();
        rt.set_screens(&[
            ("DP-1".into(), 0, 0, 1920, 1080),
            ("HDMI-A-1".into(), 1920, 0, 1920, 1080),
        ]);
        rt.settle();
        let win = |rt: &mut qml::runtime::Runtime, k: &str| match rt.get(root, k) {
            Value::Item(i) => i,
            v => panic!("{k}: {v:?}"),
        };
        let unset = win(&mut rt, "unset");
        let failed = win(&mut rt, "failed");
        let second = win(&mut rt, "second");
        let mut w = rt.world.borrow_mut();
        assert_eq!(super::panel_output(&mut w, unset), None);
        assert_eq!(super::panel_output(&mut w, failed), None);
        assert_eq!(
            super::panel_output(&mut w, second).as_deref(),
            Some("HDMI-A-1")
        );
    }

    /// `<font color>` takes any text: a value that is no colour (a non-ASCII digit, a sign)
    /// keeps the enclosing colour, as QColor::fromString's invalid colour does; hex and SVG
    /// names are QColor::fromString's.
    #[test]
    fn html_font_color_is_qcolor_from_string() {
        use super::html_runs;
        let col = |c: &str| {
            let runs = html_runs(
                &format!("<font color=\"{c}\">x</font>"),
                [0, 0, 255, 255],
                false,
            );
            assert_eq!(runs.len(), 1, "{c}: {runs:?}");
            runs[0].2
        };
        for c in [
            "#é1", "#1é", "#ééé", "#+ab", "#+abcde", "#-1", "#", "nocolor",
        ] {
            assert_eq!(col(c), None, "{c}");
        }
        assert_eq!(col("#f00"), Some([255, 0, 0, 255]));
        assert_eq!(col("#00ff00"), Some([0, 255, 0, 255]));
        assert_eq!(col("#800000ff"), Some([0, 0, 255, 128]));
        assert_eq!(col("red"), Some([255, 0, 0, 255]));
        let nested = html_runs(
            "<font color=\"#f00\">a<font color=\"#é1\">b</font></font>",
            [0; 4],
            false,
        );
        assert_eq!(nested, [("ab".to_string(), 0, Some([255, 0, 0, 255]))]);
    }

    /// Text.StyledText: the text between two tags is a run of its own while a tag is open,
    /// and so is what follows it; a rich text document joins runs of one format.
    #[test]
    fn styled_text_tags_end_runs() {
        use super::{RICH_RANGE as R, RICH_UNDERLINE, html_runs};
        const U: u8 = RICH_UNDERLINE | R;
        let runs = |s: &str, styled: bool| -> Vec<(String, u8)> {
            html_runs(s, [0; 4], styled)
                .into_iter()
                .map(|r| (r.0, r.1))
                .collect()
        };
        let own = |l: &[(&str, u8)]| -> Vec<(String, u8)> {
            l.iter().map(|r| (r.0.to_string(), r.1)).collect()
        };
        let s = "mo<u>r</u><u>o</u>ck &amp; <font>a</font>b<x>c</x>d";
        assert_eq!(
            runs(s, true),
            own(&[
                ("mo", 0),
                ("r", U),
                ("o", U),
                ("ck & ", 0),
                ("a", R),
                ("bcd", 0)
            ])
        );
        assert_eq!(
            runs(s, false),
            own(&[("mo", 0), ("ro", RICH_UNDERLINE), ("ck & abcd", 0)])
        );
        assert_eq!(
            runs("<u>a<b></b>b &lt;c</u>", true),
            own(&[("a", U), ("b <c", U)])
        );
    }

    #[test]
    fn html_anchors_are_underlined_in_link_color() {
        use super::{RICH_UNDERLINE, html_runs};
        let link = [1, 2, 3, 255];
        let runs = html_runs(
            "a <a href=\"https://x\">link</a> <a name=\"n\">b</a> &amp; c",
            link,
            false,
        );
        let anchor = runs.iter().find(|r| r.0 == "link").expect("link run");
        assert_eq!((anchor.1, anchor.2), (RICH_UNDERLINE, Some(link)));
        assert!(
            runs.iter()
                .filter(|r| r.0 != "link")
                .all(|r| r.1 == 0 && r.2.is_none())
        );
    }

    #[test]
    fn markdown_keeps_line_breaks_and_ordered_lists() {
        use super::{RICH_BOLD, markdown_runs};
        let text = |s: &str| -> String {
            let t: String = markdown_runs(s).into_iter().map(|r| r.0).collect();
            t.replace(super::LIST_MARK, "")
                .replace(super::LIST_MARK_END, " ")
                .replace('\u{2028}', "\n")
        };
        assert_eq!(text("Numbers:\n1\n2\n3"), "Numbers:\n1\n2\n3");
        assert_eq!(
            text("Steps:\n\n1. 1\n2. 2\n3. 3\n\nend."),
            "Steps:\n1. 1\n2. 2\n3. 3\nend."
        );
        assert_eq!(text("Steps:\n1. a\n1. b\n1. c"), "Steps:\n1. a\n2. b\n3. c");
        assert_eq!(text("3) x\n\n4) y"), "3) x\n4) y");
        assert_eq!(text("- a\n  more\n- b"), "\u{2022} a\nmore\n\u{2022} b");
        assert_eq!(text("* a\n+ b"), "\u{25E6} a\n\u{25AA} b");
        assert_eq!(
            text("- [x] a\n- [ ] b\n- [X] c\n- [x]d\n- [~] e"),
            "\u{2612} a\n\u{2610} b\n\u{2612} c\n\u{2022} [x]d\n\u{2022} [~] e"
        );
        assert_eq!(
            text("1. [x] a\n2. [ ] b\n3. c"),
            "\u{2612} a\n\u{2610} b\n3. c"
        );
        assert_eq!(text("hard\\\nbreak"), "hard\nbreak");
        let runs = markdown_runs("**two\nlines**");
        assert!(runs.contains(&("two\u{2028}lines".to_string(), RICH_BOLD, None)));
    }

    #[test]
    #[cfg_attr(
        not(feature = "interp"),
        ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
    )]
    fn markdown_list_items_hang_their_marker_in_the_indent() {
        let md = |extra: &str, t: &str| {
            format!(
                "TextEdit {{ textFormat: TextEdit.MarkdownText; font.pixelSize: 12; {extra} text: \"{t}\" }}"
            )
        };
        let mut rt = qml::runtime::Runtime::new(None);
        let root = rt
            .load_str(&format!(
                "import QtQuick\nItem {{\n    property Item item: {}\n    property Item plain: {}\n    property Item wrapped: {}\n}}",
                md("", "1. abc"),
                md("", "abc"),
                md("width: 90; wrapMode: TextEdit.Wrap;", "- aaa bbb ccc ddd eee fff\\nggg"),
            ))
            .unwrap();
        rt.settle();
        let mut fonts = super::Fonts::default();
        let [item, plain, wrapped] = ["item", "plain", "wrapped"].map(|k| {
            let qml::value::Value::Item(i) = rt.get(root, k) else {
                panic!("{k}");
            };
            i
        });
        let w = &mut rt.world.borrow_mut();
        for k in [item, plain, wrapped] {
            super::measure(w, &mut fonts, k);
        }
        let iw =
            |w: &mut World, k: usize| w.read(k, "implicitWidth").map_or(0.0, |v| v.to_number());
        // The marker sits in the indent; the text starts at QTextDocument::indentWidth.
        assert_eq!(iw(w, item), iw(w, plain) + super::LIST_INDENT);
        let ts = super::text_style(w, &mut fonts, wrapped);
        let lines = super::rich_laid(
            ts.font.as_ref().unwrap(),
            ts.spans.as_ref().unwrap(),
            &Bounds::wrapped(Wrap::WordOrAnywhere, 90.0),
        )
        .lines;
        assert!(lines.len() >= 3, "{} lines", lines.len());
        let (marker, text) = (&lines[0].segs[0], &lines[0].segs[1]);
        let space = ts.font.as_ref().unwrap().shape(" ").advance as f64 / 64.0;
        assert_eq!(
            marker.2 + marker.1.advance as f64 / 64.0 + space,
            super::LIST_INDENT
        );
        assert_eq!(text.2, super::LIST_INDENT);
        // Wrapped lines and the item's hard break keep the indent.
        assert!(lines[1..].iter().all(|l| l.segs[0].2 == super::LIST_INDENT));
    }

    #[test]
    fn markdown_links_are_link_runs() {
        use super::{RICH_LINK, markdown_runs};
        let runs = markdown_runs("See https://archlinux.org/. or [docs](x) and <mailto:a@b>");
        let links: Vec<&str> = runs
            .iter()
            .filter(|r| r.1 & RICH_LINK != 0)
            .map(|r| r.0.as_str())
            .collect();
        assert_eq!(links, ["https://archlinux.org/", "docs", "mailto:a@b"]);
        let hrefs: Vec<&str> = runs.iter().filter_map(|r| r.2.as_deref()).collect();
        assert_eq!(hrefs, ["https://archlinux.org/", "x", "mailto:a@b"]);
        assert_eq!(
            markdown_runs("www.x.org")
                .iter()
                .find_map(|r| r.2.clone())
                .as_deref(),
            Some("http://www.x.org")
        );
        assert!(
            markdown_runs("ahttps://x.org")
                .iter()
                .all(|r| r.1 & RICH_LINK == 0)
        );
    }

    #[test]
    #[cfg_attr(
        not(feature = "interp"),
        ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
    )]
    fn wrapped_placeholder_does_not_grow_text_area() {
        // AssistantChat's input: ScrollView { Layout.preferredHeight: ta.height; TextArea { id: ta;
        // anchors.fill: parent; wrapMode: Wrap } } laid out narrow first (detached sidebar).
        let mut rt = qml::runtime::Runtime::new(None);
        let win = rt
            .load_str(
                "import QtQuick\nimport QtQuick.Controls\nimport QtQuick.Layouts\nimport Quickshell\n\
                 FloatingWindow { width: 40; height: 600\n\
                 ColumnLayout { anchors.left: parent.left; anchors.right: parent.right; spacing: 0\n\
                 ScrollView { Layout.fillWidth: true; Layout.preferredHeight: ta.height\n\
                 TextArea { id: ta; anchors.fill: parent; wrapMode: TextArea.Wrap; padding: 10\n\
                 placeholderText: \"Message the assistant...\"; background: null } } } }",
            )
            .unwrap();
        let mut fonts = super::Fonts::default();
        let mut lay = |rt: &mut qml::runtime::Runtime, wd: f64| {
            rt.set(win, "width", qml::value::Value::Num(wd));
            for _ in 0..3 {
                rt.settle();
                super::layout_window(rt, &mut fonts, win, [wd, 600.0]);
            }
        };
        lay(&mut rt, 1000.0);
        let ta = rt.world.borrow_mut().of_type("TextArea")[0];
        let one = rt.get(ta, "height").to_number();
        lay(&mut rt, 40.0);
        assert_eq!(rt.get(ta, "implicitHeight").to_number(), one);
        lay(&mut rt, 1000.0);
        assert_eq!(rt.get(ta, "height").to_number(), one);
    }

    #[test]
    #[cfg_attr(
        not(feature = "interp"),
        ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
    )]
    fn swipe_view_moves_pages_like_smoothed_animation() {
        use qml::value::Value;
        let mut rt = qml::runtime::Runtime::new(None);
        let win = rt
            .load_str(
                "import QtQuick\nimport QtQuick.Controls\nimport Quickshell\n\
                 FloatingWindow { width: 400; height: 300\n\
                 SwipeView { anchors.fill: parent; Item {} Item {} } }",
            )
            .unwrap();
        rt.settle();
        let mut fonts = super::Fonts::default();
        let sv = rt.world.borrow_mut().of_type("SwipeView")[0];
        let page = rt.world.borrow().items[sv].children[1];
        let mut at = |rt: &mut qml::runtime::Runtime, t: f64| {
            rt.tick(t);
            super::layout_window(rt, &mut fonts, win, [400.0, 300.0]);
            rt.get(page, "x").to_number()
        };
        assert_eq!(at(&mut rt, 0.0), 400.0);
        rt.set(sv, "currentIndex", Value::Num(1.0));
        assert_eq!(at(&mut rt, 1000.0), 400.0);
        assert_eq!(at(&mut rt, 1062.5), 350.0);
        assert_eq!(at(&mut rt, 1125.0), 200.0);
        assert_eq!(at(&mut rt, 1250.0), 0.0);
        assert!(!rt.tick(1300.0));
    }
}
