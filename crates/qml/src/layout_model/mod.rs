//! The shell's widget layout: which panels, desktops and sidebars each monitor has and
//! which widgets sit where in them, as edited by the edit mode and read by the shell.
//!
//! The model is plain data. It is stored in `$XDG_CONFIG_HOME/impulse-shell/layout.toml`
//! ([`Store`]), versioned and migrated on load ([`migrate`]), kept per monitor and matched
//! to connected outputs by EDID and connector name ([`Layout::resolve`]), and changed only
//! through validated [`Edit`]s with undo/redo ([`Editor`]). Widget settings are opaque
//! TOML tables here: the widget registry owns their schema and validation.
//!
//! ```toml
//! version = 4
//!
//! [[monitor]]
//! connector = "DP-1"
//! primary = true
//! edid = { make = "Dell Inc.", model = "DELL U2720Q", serial = "F2V4X33" }
//!
//! [[monitor.container]]
//! id = "bar"
//! kind = "panel"
//! edge = "top"
//! thickness = 40
//! length = "fill"     # "fill", "60%" or a width in logical pixels
//! align = "center"
//! floating = false
//!
//! [[monitor.container.widget]]
//! id = "clock"
//! type = "clock"
//! zone = "center"
//! width = "auto"      # "auto", "stretch" or a width in logical pixels
//! settings = { seconds = true }
//! ```

pub mod arrange;
pub mod desktop;
mod edit;
mod format;
mod live;
pub mod migrate;
mod monitors;
pub mod rusty;
pub mod sidebar;
mod store;

pub use edit::{Edit, EditError, Editor};
pub use live::{Live, Shown};
pub use monitors::{MonitorId, MonitorMatch};
pub use store::{LoadSource, Loaded, Store};

use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

/// The schema version this build reads and writes.
pub const VERSION: u32 = 5;

/// The largest size or offset, in logical pixels, a layout may give: well past any monitor,
/// small enough that sums and products of them stay far from overflowing.
pub const MAX_PX: u32 = 1 << 15;
/// The most columns a sidebar grid may have.
pub const MAX_COLUMNS: u32 = 64;

/// `$XDG_CONFIG_HOME/impulse-shell/layout.toml`.
pub fn default_path() -> PathBuf {
    crate::xdg::config_home()
        .join("impulse-shell")
        .join("layout.toml")
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layout {
    /// Every monitor the shell has seen, connected or not, so a monitor that comes back
    /// gets its own layout again.
    pub monitors: Vec<MonitorLayout>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MonitorLayout {
    /// The output name, as Hyprland's `monitors` reply calls it (`DP-1`, `HDMI-A-1`).
    pub connector: String,
    pub edid: Option<Edid>,
    /// The layout new, unknown monitors start from. At most one; without one, the first.
    pub primary: bool,
    pub containers: Vec<Container>,
}

/// The monitor identity from its EDID, as reported by the compositor
/// (Hyprland: `make`, `model`, `serial` of `hyprctl monitors -j`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Edid {
    #[serde(default)]
    pub make: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub serial: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Container {
    /// Unique among the containers and widgets of its monitor.
    pub id: String,
    pub kind: ContainerKind,
    /// In display order (for a panel: the order inside each zone).
    pub widgets: Vec<Widget>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ContainerKind {
    Panel(PanelProps),
    Desktop,
    Sidebar(SidebarProps),
}

#[derive(Clone, Debug, PartialEq)]
pub struct PanelProps {
    pub position: PanelPosition,
    pub length: Length,
    pub align: Align,
    pub floating: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SidebarProps {
    pub side: Side,
    pub columns: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Widget {
    /// Unique among the containers and widgets of its monitor; kept when the widget moves.
    pub id: String,
    /// The widget registry's type name.
    pub r#type: String,
    pub place: Place,
    /// Overrides of the type's settings, validated by the widget registry.
    pub settings: toml::Table,
}

/// Where a widget sits; the variant must match its container's kind.
#[derive(Clone, Debug, PartialEq)]
pub enum Place {
    Panel {
        zone: Zone,
        width: Width,
    },
    /// Logical pixels, relative to the monitor's top-left corner. `size` is the box a resize
    /// in edit mode gave the widget (`w`, `h`); without one the widget takes its natural size,
    /// as QML sizes an item by its `implicitWidth` / `implicitHeight`.
    Desktop {
        x: i32,
        y: i32,
        size: Option<(u32, u32)>,
    },
    /// Cells taken in the sidebar grid, `1..=columns`.
    Sidebar {
        size: u32,
    },
}

/// Where a panel sits and how thick it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelPosition {
    /// On `edge`, `thickness` logical pixels across it (also its exclusive zone).
    Edge { edge: Edge, thickness: u32 },
    /// The original's bar: on the edge the original's own settings choose (`bar.vertical`, `bar.bottom`), as thick
    /// as the original's bar is in that orientation. Those settings stay the one place the bar's edge is
    /// set, so the original's Settings and everything else that reads them keep agreeing with the panel.
    RustyBar,
}

impl PanelPosition {
    /// The edge and thickness, `ri_bar` being the edge the original's settings put its bar on and
    /// `floating` the panel's [`PanelProps::floating`].
    pub fn resolve(self, ri_bar: Edge, floating: bool) -> (Edge, u32) {
        match self {
            PanelPosition::Edge { edge, thickness } => (edge, thickness),
            PanelPosition::RustyBar => (ri_bar, rusty::bar_thickness(ri_bar, floating)),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Edge {
    Top,
    Bottom,
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Align {
    Start,
    Center,
    End,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Zone {
    Start,
    Center,
    End,
}

/// A panel's extent along its edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Length {
    Fill,
    /// `1..=100` percent of the edge.
    Percent(u8),
    Px(u32),
}

/// A panel widget's width mode: panels never take free pixel sizes for content-sized
/// widgets, so a panel stays aligned whatever its widgets do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Width {
    Auto,
    Stretch,
    Px(u32),
}

impl ContainerKind {
    pub fn name(&self) -> &'static str {
        match self {
            ContainerKind::Panel(_) => "panel",
            ContainerKind::Desktop => "desktop",
            ContainerKind::Sidebar(_) => "sidebar",
        }
    }

    fn accepts(&self, place: &Place) -> bool {
        matches!(
            (self, place),
            (ContainerKind::Panel(_), Place::Panel { .. })
                | (ContainerKind::Desktop, Place::Desktop { .. })
                | (ContainerKind::Sidebar(_), Place::Sidebar { .. })
        )
    }
}

impl MonitorLayout {
    pub fn container(&self, id: &str) -> Option<&Container> {
        self.containers.iter().find(|c| c.id == id)
    }

    /// The widget with this id and the index of its container.
    pub fn widget(&self, id: &str) -> Option<(usize, &Widget)> {
        self.containers
            .iter()
            .enumerate()
            .find_map(|(ci, c)| c.widgets.iter().find(|w| w.id == id).map(|w| (ci, w)))
    }

    /// `base`, else `base-2`, `base-3`, ...: an id no container or widget of this monitor uses.
    pub fn fresh_id(&self, base: &str) -> String {
        let used = |id: &str| self.container(id).is_some() || self.widget(id).is_some();
        if !used(base) {
            return base.to_string();
        }
        (2..)
            .map(|n| format!("{base}-{n}"))
            .find(|id| !used(id))
            .expect("unbounded")
    }
}

impl Layout {
    /// The layout unknown monitors copy: the one marked primary, else the first.
    pub fn primary(&self) -> Option<usize> {
        match self.monitors.iter().position(|m| m.primary) {
            Some(i) => Some(i),
            None if self.monitors.is_empty() => None,
            None => Some(0),
        }
    }

    /// The layout in `text`, an old one migrated with every original option at its default
    /// ([`migrate::Inputs`]; the store reads the user's).
    pub fn parse(text: &str) -> Result<Layout, LoadError> {
        format::parse(text, migrate::MIGRATIONS, &migrate::Inputs::default()).map(|(l, _)| l)
    }

    pub fn to_toml(&self) -> String {
        format::to_toml(self)
    }

    /// Checks what the types alone can't: ids, ranges, and that every widget's place fits
    /// its container.
    pub fn validate(&self) -> Result<(), SchemaError> {
        let mut primaries = 0;
        for (mi, m) in self.monitors.iter().enumerate() {
            let at = |rest: String| format!("monitor {} ({}){rest}", mi + 1, m.connector);
            if m.connector.is_empty() {
                return Err(SchemaError::new(at(String::new()), "empty `connector`"));
            }
            if let Some(dup) = self.monitors[..mi].iter().find(|o| {
                o.connector == m.connector && monitors::known(&o.edid) == monitors::known(&m.edid)
            }) {
                return Err(SchemaError::new(
                    at(String::new()),
                    format!(
                        "same connector and EDID as an earlier monitor ({})",
                        dup.connector
                    ),
                ));
            }
            primaries += m.primary as u32;
            let mut ids: Vec<&str> = Vec::new();
            for c in &m.containers {
                let at_c = |rest: String| at(format!(" > container \"{}\"{rest}", c.id));
                check_id(&c.id).map_err(|e| SchemaError::new(at_c(String::new()), e))?;
                if ids.contains(&c.id.as_str()) {
                    return Err(SchemaError::new(at_c(String::new()), "duplicate id"));
                }
                ids.push(&c.id);
                match &c.kind {
                    ContainerKind::Panel(p) => {
                        if let PanelPosition::Edge { thickness, .. } = p.position
                            && !(1..=MAX_PX).contains(&thickness)
                        {
                            return Err(SchemaError::new(
                                at_c(String::new()),
                                format!("`thickness` must be 1..={MAX_PX}"),
                            ));
                        }
                        match p.length {
                            Length::Percent(n) if !(1..=100).contains(&n) => {
                                return Err(SchemaError::new(
                                    at_c(String::new()),
                                    "`length` percent must be 1..=100",
                                ));
                            }
                            Length::Px(n) if !(1..=MAX_PX).contains(&n) => {
                                return Err(SchemaError::new(
                                    at_c(String::new()),
                                    format!("`length` must be 1..={MAX_PX}"),
                                ));
                            }
                            _ => {}
                        }
                    }
                    ContainerKind::Sidebar(s) if !(1..=MAX_COLUMNS).contains(&s.columns) => {
                        return Err(SchemaError::new(
                            at_c(String::new()),
                            format!("`columns` must be 1..={MAX_COLUMNS}"),
                        ));
                    }
                    _ => {}
                }
                for w in &c.widgets {
                    let at_w = || at_c(format!(" > widget \"{}\"", w.id));
                    check_id(&w.id).map_err(|e| SchemaError::new(at_w(), e))?;
                    if ids.contains(&w.id.as_str()) {
                        return Err(SchemaError::new(at_w(), "duplicate id"));
                    }
                    ids.push(&w.id);
                    if w.r#type.is_empty() {
                        return Err(SchemaError::new(at_w(), "empty `type`"));
                    }
                    if !c.kind.accepts(&w.place) {
                        return Err(SchemaError::new(
                            at_w(),
                            format!("placement does not fit a {} container", c.kind.name()),
                        ));
                    }
                    match (&c.kind, &w.place) {
                        (
                            _,
                            Place::Panel {
                                width: Width::Px(n),
                                ..
                            },
                        ) if !(1..=MAX_PX).contains(n) => {
                            return Err(SchemaError::new(
                                at_w(),
                                format!("`width` must be 1..={MAX_PX}"),
                            ));
                        }
                        (
                            _,
                            Place::Desktop {
                                size: Some((w, h)), ..
                            },
                        ) if !(1..=MAX_PX).contains(w) || !(1..=MAX_PX).contains(h) => {
                            return Err(SchemaError::new(
                                at_w(),
                                format!("`w` and `h` must be 1..={MAX_PX}"),
                            ));
                        }
                        (_, Place::Desktop { x, y, .. })
                            if x.unsigned_abs() > MAX_PX || y.unsigned_abs() > MAX_PX =>
                        {
                            return Err(SchemaError::new(
                                at_w(),
                                format!("`x` and `y` must be within ±{MAX_PX}"),
                            ));
                        }
                        (ContainerKind::Sidebar(s), Place::Sidebar { size })
                            if !(1..=s.columns).contains(size) =>
                        {
                            return Err(SchemaError::new(
                                at_w(),
                                format!("`size` must be 1..={}", s.columns),
                            ));
                        }
                        _ => {}
                    }
                }
            }
        }
        if primaries > 1 {
            return Err(SchemaError::new(
                "layout",
                "more than one monitor is `primary`",
            ));
        }
        Ok(())
    }
}

fn check_id(id: &str) -> Result<(), &'static str> {
    if id.is_empty() {
        return Err("empty `id`");
    }
    if !id
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    {
        return Err("`id` may only use ASCII letters, digits, `-`, `_` and `.`");
    }
    Ok(())
}

/// A layout that parses but breaks a rule, with where it is.
#[derive(Clone, Debug, PartialEq)]
pub struct SchemaError {
    pub at: String,
    pub msg: String,
}

impl SchemaError {
    fn new(at: impl Into<String>, msg: impl Into<String>) -> SchemaError {
        SchemaError {
            at: at.into(),
            msg: msg.into(),
        }
    }
}

impl fmt::Display for SchemaError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}: {}", self.at, self.msg)
    }
}

/// Why a layout file could not be used.
#[derive(Clone, Debug, PartialEq)]
pub enum LoadError {
    Io(String),
    /// Not valid TOML (the message carries line and column).
    Syntax(String),
    MissingVersion,
    BadVersion(String),
    /// Written by a newer build; never overwritten by this one.
    FutureVersion {
        found: u32,
        supported: u32,
    },
    Migration {
        from: u32,
        msg: String,
    },
    /// Valid TOML with unknown keys, wrong types or missing keys.
    Format(String),
    Schema(SchemaError),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            LoadError::Io(e) => write!(f, "cannot read the layout: {e}"),
            LoadError::Syntax(e) => write!(f, "invalid TOML: {e}"),
            LoadError::MissingVersion => write!(f, "missing `version`"),
            LoadError::BadVersion(v) => write!(f, "`version` must be a positive integer, not {v}"),
            LoadError::FutureVersion { found, supported } => write!(
                f,
                "layout version {found} is newer than this build supports ({supported})"
            ),
            LoadError::Migration { from, msg } => {
                write!(f, "cannot migrate the layout from version {from}: {msg}")
            }
            LoadError::Format(e) => write!(f, "{e}"),
            LoadError::Schema(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for LoadError {}

#[cfg(test)]
mod tests;
