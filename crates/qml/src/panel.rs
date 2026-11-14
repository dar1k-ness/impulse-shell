//! `Impulse/PanelContainer`: a panel of the layout (`layout.toml`) as data its QML lays out.
//! A panel is a one-dimensional flow of three zones (`start`, `center`, `end`); each zone is
//! a list of runs, and a run is either one widget or adjacent `pill` widgets that share one
//! group background (the original's `BarGroup`).

use crate::eval::{new_obj, set_own};
use crate::layout_model::{
    Align, Container, ContainerKind, Edge, Length, PanelPosition, Place, Shown, Widget, Width, Zone,
};
use crate::runtime::World;
use crate::value::Value;
use crate::widgets::Registry;

/// What a PanelContainer's outputs were computed from: its inputs (connector, container id,
/// widget registry revision, the original's bar edge), the panel they found and the unplugged monitors' panels
/// it shows meanwhile, each with its monitor's connector.
pub type Synced = Shown<(String, String, u64, String), Content>;
type Content = (Option<Container>, Vec<(String, Container)>);

/// Writes every PanelContainer's outputs again when its inputs, its panel or the registry
/// changed; an edit to another container leaves them be. Returns whether anything was written.
pub fn sync(w: &mut World) -> bool {
    let mut wrote = false;
    let generation = w.layout.as_ref().map(|l| l.generation());
    for id in w.of_type("PanelContainer") {
        let inputs = (
            str_prop(w, id, "connector"),
            str_prop(w, id, "containerId"),
            w.widgets_revision,
            str_prop(w, id, "rustyBarEdge"),
        );
        if Shown::fresh(w.panel_synced.get(&id), generation, &inputs) {
            continue;
        }
        let found = match inputs.1.as_str() {
            "" => w.layout_container(&inputs.0, "panel"),
            cid => w
                .layout
                .as_ref()
                .and_then(|l| l.container_by_id(&inputs.0, "panel", cid)),
        }
        .cloned();
        // A panel named by id is that one alone.
        let guests = match (&w.layout, inputs.1.as_str()) {
            (Some(l), "") => l
                .guests(&inputs.0, "panel")
                .into_iter()
                .map(|(m, c)| (m.connector.clone(), c.clone()))
                .collect(),
            _ => Vec::new(),
        };
        let found = (found, guests);
        let prev = w.panel_synced.remove(&id);
        if !Shown::same(prev.as_ref(), &inputs, &found) {
            // A zone whose entries did not change keeps its array, so its Repeater keeps its
            // delegates; the other outputs are values a same-value write leaves alone.
            let kept = prev.filter(|p| p.inputs == inputs).map(|p| p.content);
            let edge = edge_named(&inputs.3).unwrap_or(Edge::Top);
            let reg = w.widgets.clone();
            for (name, v) in outputs(found.0.as_ref(), &found.1, &reg, edge) {
                let zone = zone_named(name);
                if zone.is_some_and(|z| {
                    kept.as_ref()
                        .is_some_and(|k| zone_widgets(k, z) == zone_widgets(&found, z))
                }) {
                    continue;
                }
                w.write(id, name, v);
                // A zone's settings errors are logged when it is published, not on every pass.
                for (wd, _) in zone.map_or(vec![], |z| zone_widgets(&found, z)) {
                    reg.resolve(&wd.r#type, &wd.settings)
                        .report(&wd.id, &mut w.errors);
                }
            }
            wrote = true;
        }
        w.panel_synced.insert(
            id,
            Shown {
                generation,
                inputs,
                content: found,
            },
        );
    }
    wrote
}

fn zone_named(output: &str) -> Option<Zone> {
    match output {
        "start" => Some(Zone::Start),
        "center" => Some(Zone::Center),
        "end" => Some(Zone::End),
        _ => None,
    }
}

/// The widgets a zone output is built from, in order, with the monitor each comes from.
fn zone_widgets(content: &Content, z: Zone) -> Vec<(&Widget, &str)> {
    flow(content.0.as_ref(), &content.1).map_or(vec![], |(_, widgets)| {
        widgets
            .into_iter()
            .filter(|(wd, _)| matches!(&wd.place, Place::Panel { zone, .. } if *zone == z))
            .collect()
    })
}

/// The panel a PanelContainer shows and its widgets, each with the monitor it comes from
/// (`""` for the panel's own): its own panel, else the first guest's, with the other
/// guests' widgets after its own.
fn flow<'a>(
    c: Option<&'a Container>,
    guests: &'a [(String, Container)],
) -> Option<(&'a Container, Vec<(&'a Widget, &'a str)>)> {
    let (base, from, first) = match c {
        Some(c) => (c, "", 0),
        None => {
            let (m, g) = guests.first()?;
            (g, m.as_str(), 1)
        }
    };
    let mut widgets: Vec<(&Widget, &str)> = base.widgets.iter().map(|wd| (wd, from)).collect();
    for (m, g) in &guests[first..] {
        widgets.extend(g.widgets.iter().map(|wd| (wd, m.as_str())));
    }
    Some((base, widgets))
}

fn str_prop(w: &World, id: usize, name: &str) -> String {
    match w.items[id].props.get(name).map(|p| &p.value) {
        Some(Value::Str(s)) => s.to_string(),
        _ => String::new(),
    }
}

/// The PanelContainer's readonly properties for `c` (a missing panel has none of its own),
/// with the widgets of the unplugged monitors' panels it shows until they come back
/// (`guests`, each with its monitor's connector) after its own ([`flow`]); `ri_bar` is the
/// edge the original's settings put the bar on (the `rustyBarEdge` input), where a
/// [`PanelPosition::RustyBar`] panel goes.
pub fn outputs(
    c: Option<&Container>,
    guests: &[(String, Container)],
    reg: &Registry,
    ri_bar: Edge,
) -> Vec<(&'static str, Value)> {
    let panel = flow(c, guests).and_then(|(c, widgets)| match &c.kind {
        ContainerKind::Panel(p) => Some((c, p, widgets)),
        _ => None,
    });
    let Some((c, p, widgets)) = panel else {
        let empty = || Value::array(vec![]);
        return vec![
            ("present", Value::Bool(false)),
            ("panelId", Value::str("")),
            ("edge", Value::str("top")),
            ("vertical", Value::Bool(false)),
            ("thickness", Value::Num(0.0)),
            ("lengthMode", Value::str("fill")),
            ("lengthValue", Value::Num(0.0)),
            ("align", Value::str("center")),
            ("floating", Value::Bool(false)),
            ("start", empty()),
            ("center", empty()),
            ("end", empty()),
            ("startStretch", Value::Bool(false)),
            ("centerStretch", Value::Bool(false)),
            ("endStretch", Value::Bool(false)),
        ];
    };
    let (mode, len) = match p.length {
        Length::Fill => ("fill", 0.0),
        Length::Percent(n) => ("percent", f64::from(n)),
        Length::Px(n) => ("px", f64::from(n)),
    };
    let (start, start_stretch) = zone(&widgets, Zone::Start, reg);
    let (center, center_stretch) = zone(&widgets, Zone::Center, reg);
    let (end, end_stretch) = zone(&widgets, Zone::End, reg);
    let (edge, thickness) = p.position.resolve(ri_bar, p.floating);
    vec![
        ("present", Value::Bool(true)),
        ("panelId", Value::str(&c.id)),
        ("edge", Value::str(edge_name(edge))),
        (
            "vertical",
            Value::Bool(matches!(edge, Edge::Left | Edge::Right)),
        ),
        ("thickness", Value::Num(f64::from(thickness))),
        ("lengthMode", Value::str(mode)),
        ("lengthValue", Value::Num(len)),
        ("align", Value::str(align_name(p.align))),
        ("floating", Value::Bool(p.floating)),
        ("start", start),
        ("center", center),
        ("end", end),
        ("startStretch", Value::Bool(start_stretch)),
        ("centerStretch", Value::Bool(center_stretch)),
        ("endStretch", Value::Bool(end_stretch)),
    ]
}

/// Connector -> edge of each monitor's panel, plus `""` -> the primary monitor's, which is what
/// a monitor the layout does not name uses (`LayoutState.panelEdges`). A panel on the original's bar edge
/// is `""` too: the original's settings place it, as they do without a layout.
pub fn edges(w: &World) -> Vec<(String, &'static str)> {
    let mut out = vec![];
    if let Some(l) = w.layout.as_ref() {
        let layout = l.editor().layout();
        let primary = layout.primary();
        for (i, m) in layout.monitors.iter().enumerate() {
            let Some(edge) = m.containers.iter().find_map(|c| match &c.kind {
                ContainerKind::Panel(p) => Some(match p.position {
                    PanelPosition::Edge { edge, .. } => edge_name(edge),
                    PanelPosition::RustyBar => "",
                }),
                _ => None,
            }) else {
                continue;
            };
            out.push((m.connector.clone(), edge));
            if primary == Some(i) {
                out.push((String::new(), edge));
            }
        }
    }
    out
}

/// [`edges`] as the `panelEdges` object.
pub fn edges_value(edges: &[(String, &str)]) -> Value {
    let o = new_obj();
    for (connector, edge) in edges {
        set_own(&o, connector, Value::str(edge));
    }
    Value::Obj(o)
}

fn edge_named(name: &str) -> Option<Edge> {
    Some(match name {
        "top" => Edge::Top,
        "bottom" => Edge::Bottom,
        "left" => Edge::Left,
        "right" => Edge::Right,
        _ => return None,
    })
}

fn edge_name(e: Edge) -> &'static str {
    match e {
        Edge::Top => "top",
        Edge::Bottom => "bottom",
        Edge::Left => "left",
        Edge::Right => "right",
    }
}

fn align_name(a: Align) -> &'static str {
    match a {
        Align::Start => "start",
        Align::Center => "center",
        Align::End => "end",
    }
}

/// The runs of zone `z`, in layout order, and whether any of them stretches. Adjacent pills
/// share a run only when they belong to the same monitor.
fn zone(widgets: &[(&Widget, &str)], z: Zone, reg: &Registry) -> (Value, bool) {
    let mut runs: Vec<(bool, Vec<Value>, bool, &str)> = vec![];
    for &(wd, from) in widgets {
        let Place::Panel { zone, width } = &wd.place else {
            continue;
        };
        if *zone != z {
            continue;
        }
        let pill = reg.get(&wd.r#type).is_some_and(|t| t.pill);
        let stretch = matches!(width, Width::Stretch);
        let e = new_obj();
        set_own(&e, "id", Value::str(&wd.id));
        set_own(&e, "type", Value::str(&wd.r#type));
        let (mode, px) = match width {
            Width::Auto => ("auto", 0.0),
            Width::Stretch => ("stretch", 0.0),
            Width::Px(n) => ("px", f64::from(*n)),
        };
        set_own(&e, "width", Value::str(mode));
        set_own(&e, "px", Value::Num(px));
        // The settings as the widget gets them: schema defaults applied, invalid overrides
        // dropped and listed.
        let settings = reg.resolve(&wd.r#type, &wd.settings);
        set_own(&e, "settings", settings.to_qml());
        set_own(&e, "settingErrors", settings.errors_qml());
        set_own(&e, "guest", Value::str(from));
        match runs.last_mut() {
            Some((true, entries, s, f)) if pill && *f == from => {
                entries.push(Value::Obj(e));
                *s |= stretch;
            }
            _ => runs.push((pill, vec![Value::Obj(e)], stretch, from)),
        }
    }
    let stretches = runs.iter().any(|r| r.2);
    let runs = Value::array(
        runs.into_iter()
            .map(|(pill, entries, stretch, _)| {
                let r = new_obj();
                set_own(&r, "pill", Value::Bool(pill));
                set_own(&r, "stretch", Value::Bool(stretch));
                set_own(&r, "entries", Value::array(entries));
                Value::Obj(r)
            })
            .collect(),
    );
    (runs, stretches)
}
