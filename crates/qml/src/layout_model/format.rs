//! The TOML file format: plain serde records with every kind-specific key optional, mapped
//! to and from the typed model so a key that doesn't belong to its kind is an error that
//! names it instead of being ignored.

use super::migrate::{self, Inputs, Migration};
use super::*;
use serde::de::{self, Visitor};
use serde::{Deserializer, Serializer};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLayout {
    version: u32,
    #[serde(default, rename = "monitor", skip_serializing_if = "Vec::is_empty")]
    monitors: Vec<RawMonitor>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMonitor {
    connector: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    primary: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    edid: Option<Edid>,
    #[serde(default, rename = "container", skip_serializing_if = "Vec::is_empty")]
    containers: Vec<RawContainer>,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Kind {
    Panel,
    Desktop,
    Sidebar,
}

/// A panel's `edge`: one of the four, or `"rusty"` for the original's bar ([`PanelPosition::RustyBar`]).
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum RawEdge {
    Top,
    Bottom,
    Left,
    Right,
    #[serde(alias = "ii")]
    Rusty,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawContainer {
    id: String,
    kind: Option<Kind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    edge: Option<RawEdge>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    thickness: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    length: Option<Length>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    align: Option<Align>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    floating: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    side: Option<Side>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    columns: Option<u32>,
    #[serde(default, rename = "widget", skip_serializing_if = "Vec::is_empty")]
    widgets: Vec<RawWidget>,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawWidget {
    id: String,
    #[serde(rename = "type")]
    r#type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    zone: Option<Zone>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    width: Option<Width>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    x: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    y: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    w: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    h: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    size: Option<u32>,
    #[serde(default, skip_serializing_if = "toml::Table::is_empty")]
    settings: toml::Table,
}

/// The layout and the schema version the text had.
pub(super) fn parse(
    text: &str,
    migrations: &[Migration],
    inputs: &Inputs,
) -> Result<(Layout, u32), LoadError> {
    let mut table: toml::Table = text
        .parse()
        .map_err(|e: toml::de::Error| LoadError::Syntax(e.to_string()))?;
    let found = migrate::run(&mut table, migrations, inputs)?;
    let raw: RawLayout = toml::Value::Table(table)
        .try_into()
        .map_err(|e: toml::de::Error| LoadError::Format(e.to_string()))?;
    let layout = from_raw(raw).map_err(LoadError::Schema)?;
    layout.validate().map_err(LoadError::Schema)?;
    Ok((layout, found))
}

pub(super) fn to_toml(layout: &Layout) -> String {
    toml::to_string(&to_raw(layout)).expect("a layout always serializes")
}

fn from_raw(raw: RawLayout) -> Result<Layout, SchemaError> {
    let mut monitors = Vec::with_capacity(raw.monitors.len());
    for (mi, m) in raw.monitors.into_iter().enumerate() {
        let at = format!("monitor {} ({})", mi + 1, m.connector);
        let mut containers = Vec::with_capacity(m.containers.len());
        for c in m.containers {
            containers.push(container(c, &at)?);
        }
        monitors.push(MonitorLayout {
            connector: m.connector,
            edid: m.edid,
            primary: m.primary,
            containers,
        });
    }
    Ok(Layout { monitors })
}

/// Errors with the first of `keys` that is set: a key of another container kind.
fn reject(at: &str, kind: &str, keys: &[(&str, bool)]) -> Result<(), SchemaError> {
    match keys.iter().find(|(_, set)| *set) {
        Some((k, _)) => Err(SchemaError::new(
            at,
            format!("`{k}` is not valid in a {kind}"),
        )),
        None => Ok(()),
    }
}

fn require<T>(v: Option<T>, at: &str, key: &str) -> Result<T, SchemaError> {
    v.ok_or_else(|| SchemaError::new(at, format!("missing `{key}`")))
}

fn container(c: RawContainer, at: &str) -> Result<Container, SchemaError> {
    let at = format!("{at} > container \"{}\"", c.id);
    let kind = require(c.kind, &at, "kind")?;
    let panel_keys = [
        ("edge", c.edge.is_some()),
        ("thickness", c.thickness.is_some()),
        ("length", c.length.is_some()),
        ("align", c.align.is_some()),
        ("floating", c.floating.is_some()),
    ];
    let sidebar_keys = [("side", c.side.is_some()), ("columns", c.columns.is_some())];
    let kind = match kind {
        Kind::Panel => {
            reject(&at, "panel", &sidebar_keys)?;
            let edge = match require(c.edge, &at, "edge")? {
                RawEdge::Rusty => {
                    // The original's bar size goes with the edge the original's settings choose.
                    reject(
                        &at,
                        "panel on the original's bar edge",
                        &[("thickness", c.thickness.is_some())],
                    )?;
                    None
                }
                RawEdge::Top => Some(Edge::Top),
                RawEdge::Bottom => Some(Edge::Bottom),
                RawEdge::Left => Some(Edge::Left),
                RawEdge::Right => Some(Edge::Right),
            };
            ContainerKind::Panel(PanelProps {
                position: match edge {
                    Some(edge) => PanelPosition::Edge {
                        edge,
                        thickness: require(c.thickness, &at, "thickness")?,
                    },
                    None => PanelPosition::RustyBar,
                },
                length: c.length.unwrap_or(Length::Fill),
                align: c.align.unwrap_or(Align::Center),
                floating: c.floating.unwrap_or(false),
            })
        }
        Kind::Desktop => {
            reject(&at, "desktop", &panel_keys)?;
            reject(&at, "desktop", &sidebar_keys)?;
            ContainerKind::Desktop
        }
        Kind::Sidebar => {
            reject(&at, "sidebar", &panel_keys)?;
            ContainerKind::Sidebar(SidebarProps {
                side: c.side.unwrap_or(Side::Right),
                columns: require(c.columns, &at, "columns")?,
            })
        }
    };
    let mut widgets = Vec::with_capacity(c.widgets.len());
    for w in c.widgets {
        widgets.push(widget(w, &kind, &at)?);
    }
    Ok(Container {
        id: c.id,
        kind,
        widgets,
    })
}

fn widget(w: RawWidget, kind: &ContainerKind, at: &str) -> Result<Widget, SchemaError> {
    let at = format!("{at} > widget \"{}\"", w.id);
    let panel_keys = [("zone", w.zone.is_some()), ("width", w.width.is_some())];
    let desktop_keys = [
        ("x", w.x.is_some()),
        ("y", w.y.is_some()),
        ("w", w.w.is_some()),
        ("h", w.h.is_some()),
    ];
    let sidebar_keys = [("size", w.size.is_some())];
    let name = kind.name();
    let place = match kind {
        ContainerKind::Panel(_) => {
            reject(&at, name, &desktop_keys)?;
            reject(&at, name, &sidebar_keys)?;
            Place::Panel {
                zone: require(w.zone, &at, "zone")?,
                width: w.width.unwrap_or(Width::Auto),
            }
        }
        ContainerKind::Desktop => {
            reject(&at, name, &panel_keys)?;
            reject(&at, name, &sidebar_keys)?;
            Place::Desktop {
                x: require(w.x, &at, "x")?,
                y: require(w.y, &at, "y")?,
                size: match (w.w, w.h) {
                    (None, None) => None,
                    (ww, wh) => Some((require(ww, &at, "w")?, require(wh, &at, "h")?)),
                },
            }
        }
        ContainerKind::Sidebar(_) => {
            reject(&at, name, &panel_keys)?;
            reject(&at, name, &desktop_keys)?;
            Place::Sidebar {
                size: w.size.unwrap_or(1),
            }
        }
    };
    Ok(Widget {
        id: w.id,
        r#type: w.r#type,
        place,
        settings: w.settings,
    })
}

fn to_raw(layout: &Layout) -> RawLayout {
    RawLayout {
        version: VERSION,
        monitors: layout
            .monitors
            .iter()
            .map(|m| RawMonitor {
                connector: m.connector.clone(),
                primary: m.primary,
                edid: m.edid.clone(),
                containers: m.containers.iter().map(raw_container).collect(),
            })
            .collect(),
    }
}

fn raw_container(c: &Container) -> RawContainer {
    let mut r = RawContainer {
        id: c.id.clone(),
        widgets: c.widgets.iter().map(raw_widget).collect(),
        ..Default::default()
    };
    match &c.kind {
        ContainerKind::Panel(p) => {
            r.kind = Some(Kind::Panel);
            match p.position {
                PanelPosition::Edge { edge, thickness } => {
                    r.edge = Some(match edge {
                        Edge::Top => RawEdge::Top,
                        Edge::Bottom => RawEdge::Bottom,
                        Edge::Left => RawEdge::Left,
                        Edge::Right => RawEdge::Right,
                    });
                    r.thickness = Some(thickness);
                }
                PanelPosition::RustyBar => r.edge = Some(RawEdge::Rusty),
            }
            r.length = Some(p.length);
            r.align = Some(p.align);
            r.floating = Some(p.floating);
        }
        ContainerKind::Desktop => r.kind = Some(Kind::Desktop),
        ContainerKind::Sidebar(s) => {
            r.kind = Some(Kind::Sidebar);
            r.side = Some(s.side);
            r.columns = Some(s.columns);
        }
    }
    r
}

/// `w` as an entry of a `[[monitor.container.widget]]` array.
pub(super) fn widget_value(w: &Widget) -> toml::Value {
    toml::Value::try_from(raw_widget(w)).expect("a widget always serializes")
}

fn raw_widget(w: &Widget) -> RawWidget {
    let mut r = RawWidget {
        id: w.id.clone(),
        r#type: w.r#type.clone(),
        settings: w.settings.clone(),
        ..Default::default()
    };
    match w.place {
        Place::Panel { zone, width } => {
            r.zone = Some(zone);
            r.width = (width != Width::Auto).then_some(width);
        }
        Place::Desktop { x, y, size } => {
            (r.x, r.y) = (Some(x), Some(y));
            (r.w, r.h) = (size.map(|s| s.0), size.map(|s| s.1));
        }
        Place::Sidebar { size } => r.size = Some(size),
    }
    r
}

/// A keyword or a pixel count (`length = "fill"`, `width = 120`), with `"NN%"` when
/// `percent` is allowed.
fn de_word_or_px<'de, D: Deserializer<'de>, T>(
    d: D,
    expecting: &'static str,
    word: fn(&str) -> Option<T>,
    px: fn(u32) -> T,
) -> Result<T, D::Error> {
    struct V<T> {
        expecting: &'static str,
        word: fn(&str) -> Option<T>,
        px: fn(u32) -> T,
    }
    impl<T> Visitor<'_> for V<T> {
        type Value = T;
        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str(self.expecting)
        }
        fn visit_str<E: de::Error>(self, s: &str) -> Result<T, E> {
            (self.word)(s).ok_or_else(|| E::invalid_value(de::Unexpected::Str(s), &self))
        }
        fn visit_i64<E: de::Error>(self, n: i64) -> Result<T, E> {
            u32::try_from(n)
                .map(self.px)
                .map_err(|_| E::invalid_value(de::Unexpected::Signed(n), &self))
        }
        fn visit_u64<E: de::Error>(self, n: u64) -> Result<T, E> {
            u32::try_from(n)
                .map(self.px)
                .map_err(|_| E::invalid_value(de::Unexpected::Unsigned(n), &self))
        }
    }
    d.deserialize_any(V {
        expecting,
        word,
        px,
    })
}

impl<'de> Deserialize<'de> for Length {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Length, D::Error> {
        de_word_or_px(
            d,
            "\"fill\", a percentage like \"60%\" or a length in pixels",
            |s| match s {
                "fill" => Some(Length::Fill),
                _ => s
                    .strip_suffix('%')
                    .and_then(|n| n.parse::<u8>().ok())
                    .map(Length::Percent),
            },
            Length::Px,
        )
    }
}

impl Serialize for Length {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Length::Fill => s.serialize_str("fill"),
            Length::Percent(n) => s.serialize_str(&format!("{n}%")),
            Length::Px(n) => s.serialize_u32(*n),
        }
    }
}

impl<'de> Deserialize<'de> for Width {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Width, D::Error> {
        de_word_or_px(
            d,
            "\"auto\", \"stretch\" or a width in pixels",
            |s| match s {
                "auto" => Some(Width::Auto),
                "stretch" => Some(Width::Stretch),
                _ => None,
            },
            Width::Px,
        )
    }
}

impl Serialize for Width {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Width::Auto => s.serialize_str("auto"),
            Width::Stretch => s.serialize_str("stretch"),
            Width::Px(n) => s.serialize_u32(*n),
        }
    }
}
