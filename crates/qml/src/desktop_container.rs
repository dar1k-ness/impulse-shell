//! `Impulse/DesktopContainer`: the desktop container of the edit-mode layout on one monitor.
//! It publishes the container's widgets (`widgets`: `{ id, type, x, y, width, height,
//! settings, settingErrors, guest }` in logical pixels, relative to the monitor; `settings`
//! are resolved against the type's schema; `guest` is the connector of an
//! unplugged monitor whose widget shows here until it comes back) and where each shows
//! (`shown`), fitted through [`crate::layout_model::desktop`] so widgets stay on the monitor and
//! off each other. It takes edits from the desktop surface in edit mode: `placeWidget`,
//! `resizeWidget`, `addWidget`, `removeWidget`. Those go through
//! [`crate::layout_model::desktop`] too, so widgets stay on the monitor, off each other, and on
//! the grid when `gridSize` is set; every accepted edit is one undo step written to
//! `layout.toml`.

use crate::layout_model::desktop::{self, Bounds, Limits, Natural, Rect, Shows, Snap};
use crate::layout_model::{Container, ContainerKind, Edit, MonitorId, Place, Widget};
use crate::runtime::World;
use crate::value::Value;
use crate::widgets::{Placement, Registry};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

pub(crate) const TYPE: &str = "DesktopContainer";

fn is_container(w: &World, item: usize) -> bool {
    w.items
        .get(item)
        .is_some_and(|i| i.types.iter().any(|t| t == TYPE))
}

fn text(w: &mut World, item: usize, k: &str) -> String {
    match w.read(item, k) {
        Some(Value::Str(s)) => s.to_string(),
        _ => String::new(),
    }
}

fn num(w: &mut World, item: usize, k: &str) -> f64 {
    w.read(item, k).map_or(0.0, |v| v.to_number())
}

/// The container a `DesktopContainer` shows: `containerId` when set, else the monitor's first
/// desktop container.
pub(crate) fn resolve<'a>(w: &'a World, connector: &str, id: &str) -> Option<&'a Container> {
    let l = w.layout.as_ref()?;
    match id {
        "" => l.container(connector, "desktop"),
        id => l.container_by_id(connector, "desktop", id),
    }
}

fn obj(pairs: Vec<(&str, Value)>) -> Value {
    let o = crate::eval::new_obj();
    for (k, v) in pairs {
        crate::eval::set_own(&o, k, v);
    }
    Value::Obj(o)
}

/// `{ id, key, type, x, y, width, height, natural, settings, settingErrors, guest }` for a
/// desktop widget: `key` names it in `shown` and `setNaturalSize`, `settings` are resolved
/// against its type's schema ([`Registry::resolve`]), `guest` is the unplugged monitor it
/// belongs to (`""` for the container's own).
fn widget_value(
    wd: &Widget,
    natural: Natural,
    key: &str,
    guest: &str,
    reg: &Registry,
) -> Option<Value> {
    let Place::Desktop { x, y, size } = wd.place else {
        return None;
    };
    let (w, h) = size.unwrap_or_else(|| natural(wd));
    let settings = reg.resolve(&wd.r#type, &wd.settings);
    let enabled_by = reg
        .get(&wd.r#type)
        .and_then(|t| t.variant(Placement::Desktop))
        .and_then(|v| v.enabled_by.as_deref());
    Some(obj(vec![
        ("id", Value::str(&wd.id)),
        ("key", Value::str(key)),
        ("type", Value::str(&wd.r#type)),
        ("x", Value::Num(x as f64)),
        ("y", Value::Num(y as f64)),
        ("width", Value::Num(w as f64)),
        ("height", Value::Num(h as f64)),
        // No size in the layout: the widget's view sizes it to its natural size.
        ("natural", Value::Bool(size.is_none())),
        ("settings", settings.to_qml()),
        ("settingErrors", settings.errors_qml()),
        // The config flag the widget's type is enabled by (widgets.toml `enabledBy`), or "".
        ("enabledBy", Value::str(enabled_by.unwrap_or_default())),
        ("guest", Value::str(guest)),
    ]))
}

/// The containers of unplugged monitors a DesktopContainer shows until they come back
/// ([`crate::layout_model::Live::guests`]), each with the identity its monitor's layout is
/// stored under.
pub(crate) type Guests = Vec<(MonitorId, Container)>;

/// The guests of the DesktopContainer for `connector` and container `id`: a container named
/// by id is that one alone.
pub(crate) fn guests(w: &World, connector: &str, id: &str) -> Guests {
    match (&w.layout, id) {
        (Some(l), "") => l
            .guests(connector, "desktop")
            .into_iter()
            .map(|(m, c)| (MonitorId::of(m), c.clone()))
            .collect(),
        _ => Vec::new(),
    }
}

/// A guest widget's key in `shown` and `setNaturalSize`: its monitor's identity, then `/id`.
/// It stays the widget's own while other monitors come and go, and no widget's own id can be
/// one (ids never hold a `/`). The identity's strings are quoted (`{:?}` escapes quotes), so
/// two monitors never share a key.
fn guest_key(m: &MonitorId, id: &str) -> String {
    match &m.edid {
        None => format!("{:?}/{id}", m.connector),
        Some(e) => format!(
            "{:?} {:?} {:?} {:?}/{id}",
            m.connector, e.make, e.model, e.serial
        ),
    }
}

/// What a DesktopContainer shows: the container it found and its guests.
pub(crate) type Desktop<'a> = (Option<&'a Container>, &'a [(MonitorId, Container)]);

/// Each widget of a container and of its guests with its key ([`widget_value`]).
fn keyed<'a>(
    c: Option<&'a Container>,
    guests: &'a [(MonitorId, Container)],
) -> impl Iterator<Item = (String, &'a Widget)> {
    let own = c
        .into_iter()
        .flat_map(|c| &c.widgets)
        .map(|wd| (wd.id.clone(), wd));
    let visiting = guests
        .iter()
        .flat_map(|(m, g)| g.widgets.iter().map(move |wd| (guest_key(m, &wd.id), wd)));
    own.chain(visiting)
}

/// The keys whose widget the view still shows when its desktop goes from `prev` to `now`: one
/// of the same type and settings under the same key, whose natural size is what it was.
/// Without a `prev` (the view's first desktop) every key of `now`.
pub(crate) fn kept_keys(prev: Option<Desktop>, now: Desktop) -> HashSet<String> {
    let before: Option<HashMap<String, &Widget>> = prev.map(|(c, g)| keyed(c, g).collect());
    keyed(now.0, now.1)
        .filter(|(key, wd)| {
            before.as_ref().is_none_or(|b| {
                b.get(key)
                    .is_some_and(|old| old.r#type == wd.r#type && old.settings == wd.settings)
            })
        })
        .map(|(key, _)| key)
        .collect()
}

/// Forgets what the view reported ([`NaturalSizes`]) for every key but `kept`
/// ([`kept_keys`]): a widget that is gone, or another one now under its key, takes nothing of
/// it. The layout change that made the view show other widgets already moved on what
/// [`fitted`] is cached by.
pub(crate) fn forget_replaced(w: &mut World, item: usize, kept: &HashSet<String>) {
    if let Some(n) = w.desktop_natural.get_mut(&item) {
        n.sizes.retain(|key, _| kept.contains(key));
        n.changed.retain(|key, _| kept.contains(key));
        n.self_placed.retain(|key| kept.contains(key));
        n.hidden.retain(|key| kept.contains(key));
        n.placed_at.retain(|key, _| kept.contains(key));
    }
}

/// A widget's natural size by its key ([`natural`]).
pub(crate) type Keyed<'a> = &'a dyn Fn(&str, &Widget) -> (u32, u32);

/// When a widget's natural size last changed, by its key ([`changed`]).
type KeyedChanged<'a> = &'a dyn Fn(&str) -> u64;

/// Whether a widget places itself, or is hidden, by its key ([`self_placed`], [`hidden`]).
type KeyedPlaced<'a> = &'a dyn Fn(&str) -> bool;

/// Where a widget that places itself draws, by its key ([`placed_at`]).
type KeyedAt<'a> = &'a dyn Fn(&str) -> Option<Rect>;

/// `widgets` for a container (empty without one): the layout's entries, as laid out, then
/// those of its `guests`. A monitor size change leaves it alone (views keep their
/// delegates); see [`shown_value`].
pub(crate) fn widgets_value(
    c: Option<&Container>,
    guests: &[(MonitorId, Container)],
    natural: Keyed,
    reg: &Registry,
) -> Value {
    let own = c
        .into_iter()
        .flat_map(|c| &c.widgets)
        .filter_map(|wd| widget_value(wd, &|wd| natural(&wd.id, wd), &wd.id, "", reg));
    let visiting = guests.iter().flat_map(|(m, g)| {
        g.widgets.iter().filter_map(move |wd| {
            let key = guest_key(m, &wd.id);
            widget_value(wd, &|wd| natural(&key, wd), &key, &m.connector, reg)
        })
    });
    Value::array(own.chain(visiting).collect())
}

/// Logs the settings errors of the desktop widgets [`widgets_value`] publishes.
pub(crate) fn report_settings(
    c: Option<&Container>,
    guests: &[(MonitorId, Container)],
    reg: &Registry,
    log: &mut impl Extend<String>,
) {
    let guests = guests.iter().flat_map(|(_, g)| &g.widgets);
    for wd in c.into_iter().flat_map(|c| &c.widgets).chain(guests) {
        if matches!(wd.place, Place::Desktop { .. }) {
            reg.resolve(&wd.r#type, &wd.settings).report(&wd.id, log);
        }
    }
}

/// Where each widget shows on the container's `bounds`, by key ([`widget_value`]), but for the
/// hidden ones ([`set_hidden`]), which show nowhere and so take no place: the container's own ([`desktop::fit_all`]: on the monitor and off each other after a
/// resolution or scale change or a natural size change, unchanged where the layout fits), then
/// its guests', each as near its own place as it fits off all of those
/// ([`desktop::guest_places`]). `live`: a widget mid-drag in edit mode, where the drag would
/// put it.
#[allow(clippy::too_many_arguments)]
fn shown_rects(
    c: Option<&Container>,
    guests: &[(MonitorId, Container)],
    bounds: Bounds,
    natural: Keyed,
    changed: KeyedChanged,
    placed: KeyedPlaced,
    at: KeyedAt,
    hidden: KeyedPlaced,
    live: Option<&(String, Rect, bool)>,
) -> Vec<(String, Rect)> {
    let mut out = vec![];
    // A widget that places itself ([`set_self_placed`]) is not where the layout puts it: the
    // others do not keep off that place. It shows where its view said it draws
    // ([`set_placed_at`]); until it has, at its place in the layout kept on the monitor.
    let on_monitor = |want: Rect| desktop::fit(want, &[], bounds);
    if let Some(c) = c {
        let own = |wd: &Widget| natural(&wd.id, wd);
        let own_changed = |wd: &Widget| changed(&wd.id);
        let (selves, laid): (Vec<&Widget>, Vec<&Widget>) = c
            .widgets
            .iter()
            .filter(|wd| !hidden(&wd.id))
            .partition(|wd| placed(&wd.id));
        let laid: Vec<Widget> = laid.into_iter().cloned().collect();
        let fitted = desktop::fit_all(&laid, bounds, &own, &own_changed);
        let dragged = |wd: &Widget| live.filter(|(id, ..)| *id == wd.id).map(|(_, r, _)| *r);
        for (wd, r) in laid.iter().zip(fitted) {
            if let Some(r) = dragged(wd).or(r) {
                out.push((wd.id.clone(), r));
            }
        }
        for wd in selves {
            let drawn = || Rect::of(wd, &own).map(|r| at(&wd.id).unwrap_or_else(|| on_monitor(r)));
            if let Some(r) = dragged(wd).or_else(drawn) {
                out.push((wd.id.clone(), r));
            }
        }
    }
    let taken: Vec<Rect> = out
        .iter()
        .filter(|(key, _)| !placed(key))
        .map(|(_, r)| *r)
        .collect();
    let (mut keys, mut wanted) = (vec![], vec![]);
    for (m, g) in guests {
        for wd in &g.widgets {
            let key = guest_key(m, &wd.id);
            if hidden(&key) {
                continue;
            }
            if let Some(r) = Rect::of(wd, &|wd| natural(&key, wd)) {
                if placed(&key) {
                    let r = at(&key).unwrap_or_else(|| on_monitor(r));
                    out.push((key, r));
                } else {
                    wanted.push(r);
                    keys.push(key);
                }
            }
        }
    }
    for (key, r) in keys
        .into_iter()
        .zip(desktop::guest_places(&taken, &wanted, bounds))
    {
        if let Some(r) = r {
            out.push((key, r));
        }
    }
    out
}

/// `shown`: widget key -> `{x, y, width, height}` where it shows ([`fitted`]); the widget shown
/// mid-resize in edit mode has `sized` too.
pub(crate) fn shown_value(
    w: &mut World,
    item: usize,
    connector: &str,
    id: &str,
    bounds: Bounds,
) -> Value {
    let o = crate::eval::new_obj();
    let sized = live(w, connector, id).filter(|l| l.2).map(|l| l.0);
    for (key, r) in fitted(w, item, connector, id, bounds).iter() {
        let mut shown = vec![
            ("x", Value::Num(r.x as f64)),
            ("y", Value::Num(r.y as f64)),
            ("width", Value::Num(r.w as f64)),
            ("height", Value::Num(r.h as f64)),
        ];
        if sized.as_deref() == Some(key.as_str()) {
            // Mid-resize in edit mode: shown at this size, not its natural one.
            shown.push(("sized", Value::Bool(true)));
        }
        crate::eval::set_own(&o, key, obj(shown));
    }
    Value::Obj(o)
}

/// The container's own widget shown mid-drag in edit mode: its id, where the drag would put
/// it, and whether it is a resize ([`crate::edit_mode::live_desktop`]).
pub(crate) fn live(w: &World, connector: &str, id: &str) -> Option<(String, Rect, bool)> {
    resolve(w, connector, id).and_then(|c| crate::edit_mode::live_desktop(w, c))
}

/// What [`fitted`] last computed for a container, with everything it was computed from.
struct Fitted {
    generation: Option<u64>,
    connector: String,
    id: String,
    bounds: Bounds,
    revision: u64,
    guests: Guests,
    widgets: u64,
    live: Option<(String, Rect, bool)>,
    rects: Rc<[(String, Rect)]>,
}

/// Where each widget of the DesktopContainer for `connector` and container `id` shows on
/// `bounds` ([`shown_rects`]), computed once per layout generation, size, natural sizes,
/// guests and edit-mode drag: `shown` and every `fitWidget` call of its view read the same result.
fn fitted(
    w: &mut World,
    item: usize,
    connector: &str,
    id: &str,
    bounds: Bounds,
) -> Rc<[(String, Rect)]> {
    let generation = w.layout.as_ref().map(|l| l.generation());
    let revision = w.desktop_natural.get(&item).map_or(0, |n| n.revision);
    let guests = guests(w, connector, id);
    let live = live(w, connector, id);
    let cached = w.desktop_natural.get(&item).and_then(|n| n.fitted.as_ref());
    if let Some(f) = cached.filter(|f| {
        f.generation == generation
            && f.connector == connector
            && f.id == id
            && f.bounds == bounds
            && f.revision == revision
            && f.guests == guests
            && f.widgets == w.widgets_revision
            && f.live == live
    }) {
        return f.rects.clone();
    }
    let natural = natural(w, item);
    let changed = changed(w, item);
    let placed = self_placed(w, item);
    let at = placed_at(w, item);
    let hidden = hidden(w, item);
    let rects: Rc<[(String, Rect)]> = shown_rects(
        resolve(w, connector, id),
        &guests,
        bounds,
        &natural,
        &changed,
        &placed,
        &at,
        &hidden,
        live.as_ref(),
    )
    .into();
    drop((natural, changed, placed, at, hidden));
    let widgets = w.widgets_revision;
    w.desktop_natural.entry(item).or_default().fitted = Some(Fitted {
        generation,
        connector: connector.to_string(),
        id: id.to_string(),
        bounds,
        revision,
        guests,
        widgets,
        live,
        rects: rects.clone(),
    });
    rects
}

/// The natural size of a widget without a size in the layout, by its key in the view
/// ([`widget_value`]): what the view reported (`setNaturalSize`), the type's default size
/// (`widgets.toml`) until it has.
pub(crate) fn natural(w: &World, item: usize) -> impl Fn(&str, &Widget) -> (u32, u32) + '_ {
    let measured = w.desktop_natural.get(&item).map(|n| &n.sizes);
    move |key, wd| {
        measured
            .and_then(|m| m.get(key))
            .copied()
            .unwrap_or_else(|| {
                let px = |v: f64| v.round().clamp(1.0, u32::MAX as f64) as u32;
                w.widgets
                    .get(&wd.r#type)
                    .and_then(|t| t.variant(Placement::Desktop))
                    .map_or((1, 1), |v| (px(v.size.default.w), px(v.size.default.h)))
            })
    }
}

/// [`natural`] for the container's own widgets, by their ids.
fn own_natural(w: &World, item: usize) -> impl Fn(&Widget) -> (u32, u32) + '_ {
    let natural = natural(w, item);
    move |wd| natural(&wd.id, wd)
}

/// When each widget's natural size last changed, by key: the [`NaturalSizes::revision`] its
/// last change made, 0 for one never measured ([`desktop::Changed`]).
fn changed(w: &World, item: usize) -> impl Fn(&str) -> u64 + '_ {
    let changed = w.desktop_natural.get(&item).map(|n| &n.changed);
    move |key| changed.and_then(|c| c.get(key)).copied().unwrap_or(0)
}

/// Whether a widget places itself, by key: its view said so ([`set_self_placed`]).
fn self_placed(w: &World, item: usize) -> impl Fn(&str) -> bool + '_ {
    let placed = w.desktop_natural.get(&item).map(|n| &n.self_placed);
    move |key| placed.is_some_and(|p| p.contains(key))
}

/// Where a widget that places itself draws, by key: where its view said ([`set_placed_at`]).
fn placed_at(w: &World, item: usize) -> impl Fn(&str) -> Option<Rect> + '_ {
    let at = w.desktop_natural.get(&item).map(|n| &n.placed_at);
    move |key| at.and_then(|m| m.get(key)).copied()
}

/// Where each of the container's own widgets shows, for an edit that must keep off it
/// ([`NaturalSizes::shows`]).
fn own_shows(w: &World, item: usize) -> impl Fn(&Widget) -> Shows + '_ {
    let n = w.desktop_natural.get(&item);
    move |wd| n.map_or(Shows::Laid, |n| n.shows(&wd.id))
}

/// Whether a widget is hidden, by key: its view said so ([`set_hidden`]).
fn hidden(w: &World, item: usize) -> impl Fn(&str) -> bool + '_ {
    let hidden = w.desktop_natural.get(&item).map(|n| &n.hidden);
    move |key| hidden.is_some_and(|h| h.contains(key))
}

/// What a container's view reported (the natural sizes, when each last changed, the widgets
/// that place themselves, the hidden ones), how often that changed (part of what `shown` was computed from),
/// and where its widgets show ([`fitted`]).
#[derive(Default)]
pub struct NaturalSizes {
    pub revision: u64,
    pub sizes: HashMap<String, (u32, u32)>,
    changed: HashMap<String, u64>,
    self_placed: HashSet<String>,
    placed_at: HashMap<String, Rect>,
    hidden: HashSet<String>,
    fitted: Option<Fitted>,
}

impl NaturalSizes {
    /// Where the view's widget `key` shows ([`Shows`]): nowhere when hidden, where it draws
    /// when it places itself and said where, else as laid out.
    pub(crate) fn shows(&self, key: &str) -> Shows {
        if self.hidden.contains(key) {
            return Shows::Nowhere;
        }
        match self.placed_at.get(key) {
            Some(r) if self.self_placed.contains(key) => Shows::At(*r),
            _ => Shows::Laid,
        }
    }

    /// The widgets that do not show as laid out, each with where it shows ([`Self::shows`]).
    pub(crate) fn shown_elsewhere(&self) -> impl Iterator<Item = (&String, Shows)> {
        self.hidden
            .iter()
            .chain(self.self_placed.iter())
            .map(|key| (key, self.shows(key)))
    }
}

/// `setNaturalSize(key, width, height)`: the view's widget `key` ([`widget_value`]) measured
/// itself (its `implicitWidth` / `implicitHeight`); a widget without a size in the layout
/// takes it.
fn set_natural_size(w: &mut World, item: usize, args: &[Value]) -> Value {
    let id = args.first().map(|v| v.to_js_string()).unwrap_or_default();
    let px = |i: usize| {
        let v = args.get(i).map_or(0.0, |v| v.to_number());
        (v.is_finite() && v > 0.0).then(|| v.ceil().min(u32::MAX as f64) as u32)
    };
    let (Some(width), Some(height)) = (px(1), px(2)) else {
        return Value::Undefined;
    };
    let n = w.desktop_natural.entry(item).or_default();
    if n.sizes.get(&id) != Some(&(width, height)) {
        n.revision += 1;
        n.changed.insert(id.clone(), n.revision);
        n.sizes.insert(id, (width, height));
    }
    Value::Undefined
}

/// `setSelfPlaced(key, placed)`: the view's widget `key` ([`widget_value`]) places itself (the original's
/// wallpaper placement) or no longer does. It then shows wherever it puts itself, off the
/// others by [`fit_widget`], so its place in the layout is no place on the monitor: the other
/// widgets neither keep off it nor push it aside ([`shown_rects`]).
fn set_self_placed(w: &mut World, item: usize, args: &[Value]) -> Value {
    let id = args.first().map(|v| v.to_js_string()).unwrap_or_default();
    let placed = args.get(1).is_some_and(|v| v.truthy());
    mark(w, item, id, placed, |n| &mut n.self_placed);
    Value::Undefined
}

/// `setPlacedAt(key, x, y, width, height)`: the view's widget `key`, which places itself
/// ([`set_self_placed`]), draws there on the monitor: `shown` has it there, so its cell and
/// its edit-mode frame follow it, and an edit-mode drop keeps off it there.
/// `clearPlacedAt(key)`: it no longer says where (it is laid out again, or gone).
fn set_placed_at(w: &mut World, item: usize, args: &[Value], clear: bool) -> Value {
    let key = args.first().map(|v| v.to_js_string()).unwrap_or_default();
    let n = |i: usize| args.get(i).map_or(f64::NAN, |v| v.to_number());
    let at = (!clear)
        .then(|| (n(1), n(2), n(3), n(4)))
        .filter(|(x, y, ww, h)| {
            [*x, *y, *ww, *h].iter().all(|v| v.is_finite()) && *ww > 0.0 && *h > 0.0
        })
        .map(|(x, y, ww, h)| Rect {
            x: x.round().clamp(i32::MIN as f64, i32::MAX as f64) as i32,
            y: y.round().clamp(i32::MIN as f64, i32::MAX as f64) as i32,
            w: ww.ceil().min(u32::MAX as f64) as u32,
            h: h.ceil().min(u32::MAX as f64) as u32,
        });
    if !clear && at.is_none() {
        return Value::Undefined;
    }
    let Some(s) = (match at {
        Some(_) => Some(w.desktop_natural.entry(item).or_default()),
        None => w.desktop_natural.get_mut(&item),
    }) else {
        return Value::Undefined;
    };
    let changed = match at {
        Some(r) => s.placed_at.insert(key, r) != Some(r),
        None => s.placed_at.remove(&key).is_some(),
    };
    if changed {
        s.revision += 1;
    }
    Value::Undefined
}

/// `setHidden(key, hidden)`: the view's widget `key` ([`widget_value`]) is hidden (the original's
/// settings turned it off) or shows again. A hidden widget shows nowhere, so it has no place
/// in `shown` and the others neither keep off it nor push it aside ([`shown_rects`]).
fn set_hidden(w: &mut World, item: usize, args: &[Value]) -> Value {
    let id = args.first().map(|v| v.to_js_string()).unwrap_or_default();
    let hidden = args.get(1).is_some_and(|v| v.truthy());
    mark(w, item, id, hidden, |n| &mut n.hidden);
    Value::Undefined
}

/// Puts `key` in (`on`) or takes it out of one of the view's key sets: a change of what
/// `shown` is computed from.
fn mark(
    w: &mut World,
    item: usize,
    key: String,
    on: bool,
    set: fn(&mut NaturalSizes) -> &mut HashSet<String>,
) {
    let n = if on {
        w.desktop_natural.entry(item).or_default()
    } else {
        match w.desktop_natural.get_mut(&item) {
            Some(n) => n,
            None => return,
        }
    };
    let changed = if on {
        set(n).insert(key)
    } else {
        set(n).remove(&key)
    };
    if changed {
        n.revision += 1;
    }
}

/// The container's area: its size, the monitor's.
pub(crate) fn bounds(w: &mut World, item: usize) -> Bounds {
    Bounds {
        w: num(w, item, "width").max(0.0) as u32,
        h: num(w, item, "height").max(0.0) as u32,
    }
}

/// `fitWidget(key, x, y, width, height)`: where widget `key`, which places itself (by the
/// wallpaper), shows when it wants (x, y) at that size: there when that is on the monitor and
/// off the other widgets as they show, otherwise the nearest such place ([`desktop::fit`]).
/// The other widgets that place themselves ([`set_self_placed`]) do not count: where the
/// layout puts them is not where they show. Asks only; nothing is written. Returns
/// `{x, y, width, height}`.
fn fit_widget(w: &mut World, item: usize, args: &[Value]) -> Value {
    let n = |i: usize| args.get(i).map_or(0.0, |v| v.to_number());
    let id = args.first().map(|v| v.to_js_string()).unwrap_or_default();
    let bounds = bounds(w, item);
    let (connector, cid) = (text(w, item, "connector"), text(w, item, "containerId"));
    let rects = fitted(w, item, &connector, &cid, bounds);
    let placed = self_placed(w, item);
    let others: Vec<Rect> = rects
        .iter()
        .filter(|(key, _)| *key != id && !placed(key))
        .map(|(_, r)| *r)
        .collect();
    drop(placed);
    let want = Rect {
        x: n(1).round().clamp(i32::MIN as f64, i32::MAX as f64) as i32,
        y: n(2).round().clamp(i32::MIN as f64, i32::MAX as f64) as i32,
        w: n(3).round().clamp(1.0, u32::MAX as f64) as u32,
        h: n(4).round().clamp(1.0, u32::MAX as f64) as u32,
    };
    let r = desktop::fit(want, &others, bounds);
    obj(vec![
        ("x", Value::Num(r.x as f64)),
        ("y", Value::Num(r.y as f64)),
        ("width", Value::Num(r.w as f64)),
        ("height", Value::Num(r.h as f64)),
    ])
}

/// `moveWidget(id, x, y)`: the user dragged the container's own widget `id` so that its top
/// left corner is at `x`, `y` on the monitor (the original's "Draggable" placement of the background
/// clock and weather). The layout gets that place, at the size it had, and `layout.toml` is
/// written; where it then shows is `shown`'s, as for any place of the layout. A monitor that
/// showed the primary one's layout gets its own first, so the others keep theirs. `true` when
/// the layout changed; a guest's widget is its unplugged monitor's and stays.
fn move_widget(w: &mut World, item: usize, args: &[Value]) -> Value {
    let id = args.first().map(|v| v.to_js_string()).unwrap_or_default();
    let px = |i: usize| {
        let v = args.get(i).map_or(f64::NAN, |v| v.to_number());
        v.is_finite()
            .then(|| v.round().clamp(i32::MIN as f64, i32::MAX as f64) as i32)
    };
    let (Some(x), Some(y)) = (px(1), px(2)) else {
        return Value::Bool(false);
    };
    let (connector, cid) = (text(w, item, "connector"), text(w, item, "containerId"));
    let found = resolve(w, &connector, &cid).and_then(|c| {
        let wd = c.widgets.iter().find(|wd| wd.id == id)?;
        match wd.place {
            Place::Desktop { size, .. } => Some((c.id.clone(), size)),
            _ => None,
        }
    });
    let (Some((container, size)), Some(live)) = (found, w.layout.as_mut()) else {
        return Value::Bool(false);
    };
    let monitor = live.attach(&connector);
    let edit = Edit::MoveWidget {
        monitor,
        id,
        container,
        place: Place::Desktop { x, y, size },
        index: None,
    };
    Value::Bool(match live.apply(&[edit]) {
        Ok(Ok(changed)) => changed,
        // Applied, but not written: the shell shows it and says why the file does not.
        Ok(Err(save)) => {
            w.errors
                .push(format!("DesktopContainer.moveWidget: {save}"));
            true
        }
        Err(e) => {
            w.errors.push(format!("DesktopContainer.moveWidget: {e}"));
            false
        }
    })
}

/// What an edit needs: the monitor index (attached), the container id, the monitor area and
/// the snapping.
struct Target {
    monitor: usize,
    container: String,
    bounds: Bounds,
    snap: Snap,
}

fn target(w: &mut World, item: usize) -> Option<Target> {
    let connector = text(w, item, "connector");
    let cid = text(w, item, "containerId");
    let container = resolve(w, &connector, &cid)?.id.clone();
    let bounds = bounds(w, item);
    let grid = num(w, item, "gridSize");
    let snap = if grid >= 1.0 {
        Snap::Grid {
            cell: grid.round() as u32,
        }
    } else {
        Snap::Free
    };
    let monitor = w.layout.as_mut()?.attach(&connector);
    Some(Target {
        monitor,
        container,
        bounds,
        snap,
    })
}

fn limits(w: &World, ty: &str) -> Limits {
    let px = |v: f64| v.max(1.0).min(u32::MAX as f64) as u32;
    w.widgets
        .get(ty)
        .and_then(|t| t.variant(Placement::Desktop))
        .map_or(Limits::ANY, |v| Limits {
            min: (px(v.size.min.w), px(v.size.min.h)),
            max: (px(v.size.max.w), px(v.size.max.h)),
        })
}

fn widget_type(w: &World, monitor: usize, id: &str) -> Option<String> {
    let l = w.layout.as_ref()?.editor().layout();
    l.monitors.get(monitor)?.containers.iter().find_map(|c| {
        c.widgets
            .iter()
            .find(|wd| wd.id == id)
            .map(|wd| wd.r#type.clone())
    })
}

/// A widget id not used on the monitor yet: the type name, numbered from 2 on collision.
fn fresh_id(w: &World, monitor: usize, ty: &str) -> String {
    let Some(m) = w
        .layout
        .as_ref()
        .and_then(|l| l.editor().layout().monitors.get(monitor))
    else {
        return ty.to_string();
    };
    let taken = |id: &str| {
        m.containers
            .iter()
            .any(|c| c.id == id || c.widgets.iter().any(|wd| wd.id == id))
    };
    let mut id = ty.to_string();
    let mut n = 2;
    while taken(&id) {
        id = format!("{ty}-{n}");
        n += 1;
    }
    id
}

fn commit(w: &mut World, edit: Edit) -> bool {
    let Some(live) = w.layout.as_mut() else {
        return false;
    };
    match live.apply(&[edit]) {
        Ok(Ok(changed)) => changed,
        Ok(Err(save)) => {
            w.errors.push(format!("DesktopContainer: {save}"));
            true
        }
        Err(e) => {
            w.errors.push(format!("DesktopContainer: {e}"));
            false
        }
    }
}

/// The container's methods: `fitWidget`, `moveWidget`, `setNaturalSize`, `setSelfPlaced`,
/// `setPlacedAt`, `clearPlacedAt`, `setHidden`, and edit mode's `placeWidget`, `resizeWidget`, `addWidget`, `removeWidget`.
/// Each edit returns whether the layout changed; `addWidget` returns the new widget's id (""
/// when it fits nowhere or the type is not a desktop widget).
/// `args: None` only probes for the method ([`crate::eval::Host::native`]).
pub(crate) fn call(
    w: &mut World,
    item: usize,
    name: &str,
    args: Option<&[Value]>,
) -> Option<Value> {
    if name == "fitWidget" && is_container(w, item) {
        return Some(fit_widget(w, item, args.unwrap_or_default()));
    }
    if name == "moveWidget" && is_container(w, item) {
        return Some(move_widget(w, item, args.unwrap_or_default()));
    }
    if name == "setNaturalSize" && is_container(w, item) {
        return Some(set_natural_size(w, item, args.unwrap_or_default()));
    }
    if name == "setSelfPlaced" && is_container(w, item) {
        return Some(set_self_placed(w, item, args.unwrap_or_default()));
    }
    if matches!(name, "setPlacedAt" | "clearPlacedAt") && is_container(w, item) {
        return Some(set_placed_at(
            w,
            item,
            args.unwrap_or_default(),
            name == "clearPlacedAt",
        ));
    }
    if name == "setHidden" && is_container(w, item) {
        return Some(set_hidden(w, item, args.unwrap_or_default()));
    }
    if !matches!(
        name,
        "placeWidget" | "resizeWidget" | "addWidget" | "removeWidget"
    ) || !is_container(w, item)
    {
        return None;
    }
    let Some(args) = args else {
        return Some(Value::Undefined);
    };
    let arg_s = |i: usize| match args.get(i) {
        Some(Value::Str(s)) => s.to_string(),
        Some(v) => v.to_js_string(),
        None => String::new(),
    };
    let arg_n = |i: usize| args.get(i).map_or(0.0, |v| v.to_number());
    let px = |v: f64| v.round().clamp(i32::MIN as f64, i32::MAX as f64) as i32;
    let size = |v: f64| v.round().clamp(1.0, u32::MAX as f64) as u32;
    let Some(t) = target(w, item) else {
        return Some(if name == "addWidget" {
            Value::str("")
        } else {
            Value::Bool(false)
        });
    };
    let layout = || w.layout.as_ref().unwrap().editor().layout();
    let r = match name {
        // The edit-mode drop: the nearest free place, on the grid.
        "placeWidget" => {
            let id = arg_s(0);
            let edit = desktop::move_to(
                layout(),
                t.monitor,
                &id,
                px(arg_n(1)),
                px(arg_n(2)),
                t.bounds,
                t.snap,
                &own_natural(w, item),
                &own_shows(w, item),
            );
            match edit {
                Ok(e) => Value::Bool(commit(w, e)),
                Err(_) => Value::Bool(false),
            }
        }
        "resizeWidget" => {
            let id = arg_s(0);
            let lim = widget_type(w, t.monitor, &id).map_or(Limits::ANY, |ty| limits(w, &ty));
            let edit = desktop::resize_to(
                layout(),
                t.monitor,
                &id,
                size(arg_n(1)),
                size(arg_n(2)),
                lim,
                t.bounds,
                t.snap,
                &own_natural(w, item),
                &own_shows(w, item),
            );
            match edit {
                Ok(e) => Value::Bool(commit(w, e)),
                Err(_) => Value::Bool(false),
            }
        }
        "removeWidget" => {
            let id = arg_s(0);
            let here = layout().monitors[t.monitor]
                .containers
                .iter()
                .any(|c| c.id == t.container && c.widgets.iter().any(|wd| wd.id == id));
            Value::Bool(
                here && commit(
                    w,
                    Edit::RemoveWidget {
                        monitor: t.monitor,
                        id,
                    },
                ),
            )
        }
        _ => {
            let ty = arg_s(0);
            let Some(def) = w
                .widgets
                .get(&ty)
                .and_then(|d| d.variant(Placement::Desktop))
            else {
                w.errors.push(format!(
                    "DesktopContainer: \"{ty}\" is not a desktop widget"
                ));
                return Some(Value::str(""));
            };
            let (dw, dh) = (def.size.default.w, def.size.default.h);
            let lim = limits(w, &ty);
            let wanted = (
                size(dw).clamp(lim.min.0, lim.max.0.max(lim.min.0)),
                size(dh).clamp(lim.min.1, lim.max.1.max(lim.min.1)),
            );
            let widgets = &layout().monitors[t.monitor]
                .containers
                .iter()
                .find(|c| c.id == t.container && matches!(c.kind, ContainerKind::Desktop))
                .map(|c| c.widgets.clone())
                .unwrap_or_default();
            let Some(place) = desktop::place_new(
                widgets,
                px(arg_n(1)),
                px(arg_n(2)),
                wanted.0,
                wanted.1,
                t.bounds,
                t.snap,
                &own_natural(w, item),
                &own_shows(w, item),
            ) else {
                return Some(Value::str(""));
            };
            let id = fresh_id(w, t.monitor, &ty);
            let edit = Edit::AddWidget {
                monitor: t.monitor,
                container: t.container,
                widget: Widget {
                    id: id.clone(),
                    r#type: ty,
                    place,
                    settings: toml::Table::new(),
                },
                index: None,
            };
            if commit(w, edit) {
                Value::str(&id)
            } else {
                Value::str("")
            }
        }
    };
    Some(r)
}
