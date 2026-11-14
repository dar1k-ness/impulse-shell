//! The first layout of a user coming from the original: built from the original's own configuration
//! (`~/.config/rusty-impulse/config.json`, the original's `Config.options`) so the shell looks
//! exactly as it did before there was a layout. The panel holds the original's bar modules in the original's order
//! on the original's bar edge, which `bar.vertical` / `bar.bottom` keep choosing ([`PanelPosition::RustyBar`]);
//! the widgets keep reading the `bar.*` options they always read (verbose, borderless, weather,
//! utility buttons...), so those stay live settings. The desktop holds the original's background widgets
//! where the original puts them, and each shows while its `background.widgets.*.enable` is on (also a
//! live setting, read by `DesktopLayoutContent`), and the sidebar holds the quick sliders, the original's quick
//! panel (its toggles, order and sizes stay the original's options, edited by the original's own edit mode), the
//! notifications and the calendar.
//! An option missing from the file has the original's default (`modules/common/Config.qml`).

use std::path::{Path, PathBuf};

use serde_json::Value as Json;

use super::{
    Align, Container, ContainerKind, Edge, Layout, Length, MonitorLayout, PanelPosition,
    PanelProps, Place, Side, SidebarProps, Widget, Width, Zone,
};

/// The original's options file: `$XDG_CONFIG_HOME/rusty-impulse/config.json`.
pub fn config_path() -> PathBuf {
    crate::xdg::config_home()
        .join("rusty-impulse")
        .join("config.json")
}

/// The original's options from `path`: `{}` (every option at its default) when the file does not exist.
pub fn read_options(path: &Path) -> Result<Json, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Json::Object(Default::default())),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// The monitor the first layout is made on (it becomes the primary layout, which every other
/// monitor copies until edited), with its size in logical pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct Monitor {
    pub connector: String,
}

// The original's `Appearance.sizes.baseBarHeight` / `baseVerticalBarWidth` / `hyprlandGapsOut`.
const BAR_HEIGHT: u32 = 40;
const VERTICAL_BAR_WIDTH: u32 = 46;
const GAPS_OUT: u32 = 5;

/// The edge the original's settings put its bar on: `bar.vertical` picks the orientation, `bar.bottom`
/// the far edge of it (the original's `Bar` / `VerticalBar`).
pub fn bar_edge(vertical: bool, bottom: bool) -> Edge {
    match (vertical, bottom) {
        (false, false) => Edge::Top,
        (false, true) => Edge::Bottom,
        (true, false) => Edge::Left,
        (true, true) => Edge::Right,
    }
}

/// The original's bar size across `edge` (`Appearance.sizes.barHeight` / `verticalBarWidth`): the
/// floating bar (`bar.cornerStyle` 1) keeps Hyprland's outer gap on both sides of it.
pub fn bar_thickness(edge: Edge, floating: bool) -> u32 {
    let base = match edge {
        Edge::Top | Edge::Bottom => BAR_HEIGHT,
        Edge::Left | Edge::Right => VERTICAL_BAR_WIDTH,
    };
    if floating { base + 2 * GAPS_OUT } else { base }
}
// The original's AndroidQuickPanel default (`sidebar.quickToggles.android.columns`).
const DEFAULT_COLUMNS: u32 = 5;

fn at<'a>(o: &'a Json, path: &str) -> Option<&'a Json> {
    path.split('.').try_fold(o, |v, k| v.get(k))
}

fn number(o: &Json, path: &str, default: f64) -> f64 {
    at(o, path).and_then(Json::as_f64).unwrap_or(default)
}

fn text<'a>(o: &'a Json, path: &str, default: &'a str) -> &'a str {
    at(o, path).and_then(Json::as_str).unwrap_or(default)
}

/// The layout the original's options describe, on `monitor`.
pub fn from_options(options: &Json, monitor: &Monitor) -> Layout {
    let mut containers = vec![panel(options), desktop(options)];
    containers.extend(sidebar(options));
    let mut layout = Layout {
        monitors: vec![MonitorLayout {
            connector: monitor.connector.clone(),
            edid: None,
            primary: true,
            containers,
        }],
    };
    if let Some(parts) = bar_layout(options) {
        super::arrange::bar_groups(&mut layout, [&parts[0], &parts[1], &parts[2]]);
    }
    layout
}

/// The original's `bar.layout` (`left`, `center`, `right` group lists), when the original's options set it.
fn bar_layout(o: &Json) -> Option<[Vec<String>; 3]> {
    at(o, "bar.layout")?.as_object()?;
    Some(["left", "center", "right"].map(|side| {
        at(o, &format!("bar.layout.{side}"))
            .and_then(Json::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Json::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    }))
}

fn widget(id: &str, r#type: &str, place: Place, settings: toml::Table) -> Widget {
    Widget {
        id: id.to_string(),
        r#type: r#type.to_string(),
        place,
        settings,
    }
}

/// The original's bar (`BarContent` / `VerticalBarContent`): the same modules in the same order on
/// either orientation; each module shows the form its orientation has, or none.
fn panel(o: &Json) -> Container {
    let at = |zone, width| Place::Panel { zone, width };
    let (start, center, end) = (Zone::Start, Zone::Center, Zone::End);
    let (auto, stretch) = (Width::Auto, Width::Stretch);
    let w = |id: &str, place| widget(id, id, place, toml::Table::new());
    let separator = |id: &str| widget(id, "separator", at(center, auto), toml::Table::new());
    Container {
        id: "panel".into(),
        kind: ContainerKind::Panel(PanelProps {
            position: PanelPosition::RustyBar,
            length: Length::Fill,
            align: Align::Center,
            floating: number(o, "bar.cornerStyle", 0.0) == 1.0,
        }),
        widgets: vec![
            w("left-sidebar-button", at(start, auto)),
            w("active-window", at(start, stretch)),
            // The original's left center group: as long as the center-side module, media takes the rest.
            w("resources", at(center, auto)),
            w("media", at(center, stretch)),
            separator("separator-1"),
            w("workspaces", at(center, auto)),
            separator("separator-2"),
            w("clock", at(center, stretch)),
            w("util-buttons", at(center, auto)),
            w("battery", at(center, auto)),
            w("weather", at(end, auto)),
            w("spacer", at(end, stretch)),
            w("systray", at(end, auto)),
            w("indicators", at(end, auto)),
        ],
    }
}

/// The original's background widgets in a desktop container ([`desktop_widgets`]).
fn desktop(o: &Json) -> Container {
    Container {
        id: "desktop".into(),
        kind: ContainerKind::Desktop,
        widgets: desktop_widgets(o),
    }
}

/// The original's background widgets, weather under the clock as the original stacks them, also the ones the original's
/// settings have off: those settings decide live whether each shows and how it is placed
/// (`placementStrategy`: by the wallpaper, or `free` at its position, which the layout keeps).
pub(super) fn desktop_widgets(o: &Json) -> Vec<Widget> {
    let mut widgets: Vec<Widget> = vec![];
    for (name, id, x, y) in [
        ("weather", "desktop-weather", 400.0, 100.0),
        ("clock", "desktop-clock", 100.0, 100.0),
    ] {
        let key = format!("background.widgets.{name}");
        // Drawn at their natural size, as the original draws them; the container keeps them on the
        // monitor and off each other.
        let place = Place::Desktop {
            x: number(o, &format!("{key}.x"), x).round() as i32,
            y: number(o, &format!("{key}.y"), y).round() as i32,
            size: None,
        };
        // Ids are unique on a monitor: the panel has a clock and a weather widget too.
        widgets.push(widget(id, name, place, toml::Table::new()));
    }
    widgets
}

/// The original's quick sliders in a sidebar of `columns` cells: a full-width block of its own.
pub(super) fn sliders_widget(columns: u32) -> Widget {
    widget(
        "sliders",
        "sliders",
        Place::Sidebar { size: columns },
        toml::Table::new(),
    )
}

/// The original's right sidebar below the system buttons: the quick sliders, the Android quick panel,
/// notifications and the calendar. The sliders are always there: whether they show, and which
/// of the three, is the original's live setting (`sidebar.quickSliders`), read by the widget. The original's
/// classic quick panel is a fixed row of round buttons, not a grid, so with it the sidebar
/// keeps the original's own content.
fn sidebar(o: &Json) -> Option<Container> {
    if text(o, "sidebar.quickToggles.style", "android") != "android" {
        return None;
    }
    let columns = match at(o, "sidebar.quickToggles.android.columns").and_then(Json::as_u64) {
        Some(c) if c >= 1 => c as u32,
        _ => DEFAULT_COLUMNS,
    };
    let full = || Place::Sidebar { size: columns };
    let mut widgets = vec![sliders_widget(columns)];
    // The original's quick panel itself: the toggles, their order and sizes stay in the original's options, where
    // the original's own edit mode for them keeps working.
    widgets.push(widget(
        "quickToggles",
        "quickToggles",
        full(),
        toml::Table::new(),
    ));
    widgets.push(widget(
        "notificationCenter",
        "notificationCenter",
        full(),
        toml::Table::new(),
    ));
    widgets.push(widget(
        "calendarTasks",
        "calendarTasks",
        full(),
        toml::Table::new(),
    ));
    Some(Container {
        id: "sidebar".into(),
        kind: ContainerKind::Sidebar(SidebarProps {
            side: Side::Right,
            columns,
        }),
        widgets,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn monitor() -> Monitor {
        Monitor {
            connector: "DP-1".into(),
        }
    }

    fn container<'a>(l: &'a Layout, kind: &str) -> Option<&'a Container> {
        l.monitors[0]
            .containers
            .iter()
            .find(|c| c.kind.name() == kind)
    }

    fn widgets(c: &Container) -> Vec<(&str, &str, &Place)> {
        c.widgets
            .iter()
            .map(|w| (w.id.as_str(), w.r#type.as_str(), &w.place))
            .collect()
    }

    fn panel_props(l: &Layout) -> &PanelProps {
        match &container(l, "panel").unwrap().kind {
            ContainerKind::Panel(p) => p,
            k => panic!("{k:?}"),
        }
    }

    fn sidebar_cells(l: &Layout) -> (u32, Vec<(&str, u32)>) {
        let c = container(l, "sidebar").unwrap();
        let ContainerKind::Sidebar(p) = &c.kind else {
            panic!()
        };
        let cells = c
            .widgets
            .iter()
            .map(|w| match w.place {
                Place::Sidebar { size } => (w.r#type.as_str(), size),
                _ => panic!(),
            })
            .collect();
        (p.columns, cells)
    }

    /// A real config.json (the original as installed by the end-4 dots, then customized).
    fn owner() -> Json {
        serde_json::from_str(include_str!("rusty_configs/owner.json")).unwrap()
    }

    /// The layout survives a write and a read, and the schema accepts it.
    fn round_trip(l: &Layout) {
        l.validate().unwrap();
        assert_eq!(&Layout::parse(&l.to_toml()).unwrap(), l);
    }

    #[test]
    fn fresh_install_is_the_original_default() {
        let l = from_options(&json!({}), &monitor());
        round_trip(&l);
        assert_eq!(l.monitors.len(), 1);
        assert!(l.monitors[0].primary);
        assert_eq!(l.monitors[0].connector, "DP-1");
        let p = panel_props(&l);
        assert_eq!(
            (p.position, p.length, p.align, p.floating),
            (PanelPosition::RustyBar, Length::Fill, Align::Center, false)
        );
        let panel = container(&l, "panel").unwrap();
        let (start, center, end) = (Zone::Start, Zone::Center, Zone::End);
        let at = |zone, width| Place::Panel { zone, width };
        let (auto, stretch) = (Width::Auto, Width::Stretch);
        assert_eq!(
            widgets(panel),
            vec![
                (
                    "left-sidebar-button",
                    "left-sidebar-button",
                    &at(start, auto)
                ),
                ("active-window", "active-window", &at(start, stretch)),
                ("resources", "resources", &at(center, auto)),
                ("media", "media", &at(center, stretch)),
                ("separator-1", "separator", &at(center, auto)),
                ("workspaces", "workspaces", &at(center, auto)),
                ("separator-2", "separator", &at(center, auto)),
                ("clock", "clock", &at(center, stretch)),
                ("util-buttons", "util-buttons", &at(center, auto)),
                ("battery", "battery", &at(center, auto)),
                ("weather", "weather", &at(end, auto)),
                ("spacer", "spacer", &at(end, stretch)),
                ("systray", "systray", &at(end, auto)),
                ("indicators", "indicators", &at(end, auto)),
            ]
        );
        // Background: the weather (shown only once the original's settings turn it on) where the original puts
        // it, the clock placed by the wallpaper.
        let desktop = container(&l, "desktop").unwrap();
        assert_eq!(
            widgets(desktop),
            vec![
                (
                    "desktop-weather",
                    "weather",
                    &Place::Desktop {
                        x: 400,
                        y: 100,
                        size: None
                    }
                ),
                (
                    "desktop-clock",
                    "clock",
                    &Place::Desktop {
                        x: 100,
                        y: 100,
                        size: None
                    }
                )
            ]
        );
        assert!(desktop.widgets.iter().all(|w| w.settings.is_empty()));
        // Sidebar: the quick sliders are there whatever the original's settings say (off by default).
        assert_eq!(
            sidebar_cells(&l),
            (
                5,
                vec![
                    ("sliders", 5),
                    ("quickToggles", 5),
                    ("notificationCenter", 5),
                    ("calendarTasks", 5),
                ]
            )
        );
    }

    #[test]
    fn owner_config() {
        let o = owner();
        let l = from_options(&o, &monitor());
        round_trip(&l);
        // These options equal the original's defaults for everything the layout holds, except the
        // weather and clock positions (its weather is off, its clock wallpaper-placed).
        let mut fresh = from_options(&json!({}), &monitor());
        let desktop = &mut fresh.monitors[0].containers[1].widgets;
        desktop[0].place = Place::Desktop {
            x: 358,
            y: 113,
            size: None,
        };
        desktop[1].place = Place::Desktop {
            x: 235,
            y: 545,
            size: None,
        };
        assert_eq!(l, fresh);
        // His quick toggles stay the original's panel, read from his options (order and sizes there),
        // so the original's edit mode for them keeps working: no copy of them in the layout.
        let (_, cells) = sidebar_cells(&l);
        assert_eq!(cells[..2], [("sliders", 5), ("quickToggles", 5)]);
        assert!(o["sidebar"]["quickToggles"]["android"]["toggles"].is_array());
        assert!(
            !cells
                .iter()
                .any(|(t, _)| ["network", "bluetooth", "audio"].contains(t))
        );
    }

    #[test]
    fn the_bar_edge_stays_the_original_s_setting() {
        // The panel is on the original's bar edge whatever the options say now: bar.vertical and
        // bar.bottom keep choosing it later (`PanelPosition::RustyBar`).
        for bar in [
            json!({}),
            json!({"vertical": true, "bottom": true, "cornerStyle": 1}),
            json!({"vertical": true}),
            json!({"bottom": true, "cornerStyle": 2}),
        ] {
            let l = from_options(&json!({ "bar": bar }), &monitor());
            round_trip(&l);
            assert_eq!(panel_props(&l).position, PanelPosition::RustyBar, "{bar}");
        }
        let l = from_options(&json!({"bar": {"cornerStyle": 1}}), &monitor());
        assert!(panel_props(&l).floating);
        // The original's barHeight / verticalBarWidth: base, plus hyprlandGapsOut on both sides when
        // floating (Appearance.qml).
        for (vertical, bottom, edge, thickness) in [
            (false, false, Edge::Top, 40),
            (false, true, Edge::Bottom, 40),
            (true, false, Edge::Left, 46),
            (true, true, Edge::Right, 46),
        ] {
            assert_eq!(
                PanelPosition::RustyBar.resolve(bar_edge(vertical, bottom), false),
                (edge, thickness)
            );
            assert_eq!(
                PanelPosition::RustyBar.resolve(bar_edge(vertical, bottom), true),
                (edge, thickness + 10)
            );
        }
        assert!(
            Layout::parse(
                &l.to_toml()
                    .replace("edge = \"rusty\"", "edge = \"rusty\"\nthickness = 40")
            )
            .is_err(),
            "the original's bar has the original's thickness"
        );
    }

    #[test]
    fn background_widgets_keep_their_places() {
        let l = from_options(
            &json!({"background": {"widgets": {
                "clock": {"enable": true, "placementStrategy": "free", "x": 1700.4, "y": 20},
                "weather": {"enable": true, "placementStrategy": "mostBusy", "x": 50, "y": 700}
            }}}),
            &monitor(),
        );
        round_trip(&l);
        let d = container(&l, "desktop").unwrap();
        assert_eq!(
            widgets(d),
            vec![
                (
                    "desktop-weather",
                    "weather",
                    &Place::Desktop {
                        x: 50,
                        y: 700,
                        size: None
                    }
                ),
                // The original's own place: the container keeps it on the monitor at the clock's
                // natural size, which only the clock knows.
                (
                    "desktop-clock",
                    "clock",
                    &Place::Desktop {
                        x: 1700,
                        y: 20,
                        size: None
                    }
                ),
            ]
        );
        // How each is placed stays the original's setting, read live by the widget.
        assert!(d.widgets.iter().all(|w| w.settings.is_empty()));
        // Turned off in the original's settings, they stay in the layout: the settings hide them, and
        // show them again when turned back on.
        let off = from_options(
            &json!({"background": {"widgets": {"clock": {"enable": false}}}}),
            &monitor(),
        );
        assert_eq!(
            container(&off, "desktop").unwrap(),
            container(&from_options(&json!({}), &monitor()), "desktop").unwrap()
        );
    }

    #[test]
    fn quick_sliders_and_the_original_s_quick_panel() {
        let l = from_options(
            &json!({"sidebar": {
                "quickSliders": {"enable": true, "showMic": true, "showVolume": false},
                "quickToggles": {"style": "android", "android": {"columns": 4, "toggles": [
                    {"type": "darkMode", "size": 1},
                    {"type": "network", "size": 2},
                    {"type": "darkMode", "size": 1},
                    {"type": "gameMode", "size": 9},
                    {"size": 1}
                ]}}
            }}),
            &monitor(),
        );
        round_trip(&l);
        assert_eq!(
            sidebar_cells(&l),
            (
                4,
                vec![
                    ("sliders", 4),
                    ("quickToggles", 4),
                    ("notificationCenter", 4),
                    ("calendarTasks", 4),
                ]
            )
        );
        // Which sliders show is the original's live setting: the layout keeps no copy of it.
        let c = container(&l, "sidebar").unwrap();
        assert!(c.widgets[0].settings.is_empty());
        // Sliders the original's settings have off, or with every slider hidden, are in the layout too:
        // turning them on in the original's settings shows them.
        for sliders in [
            json!({"enable": false}),
            json!({"enable": true, "showBrightness": false, "showVolume": false, "showMic": false}),
        ] {
            let l = from_options(&json!({"sidebar": {"quickSliders": sliders}}), &monitor());
            assert_eq!(sidebar_cells(&l).1[0], ("sliders", 5));
        }
    }

    #[test]
    fn classic_quick_panel_keeps_the_original_sidebar() {
        let l = from_options(
            &json!({"sidebar": {"quickToggles": {"style": "classic"}}}),
            &monitor(),
        );
        round_trip(&l);
        assert!(container(&l, "sidebar").is_none());
        assert!(container(&l, "panel").is_some());
    }

    #[test]
    fn missing_file_reads_as_defaults_and_broken_file_is_an_error() {
        let dir = std::env::temp_dir().join(format!("ri-layout-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(read_options(&dir.join("none.json")).unwrap(), json!({}));
        std::fs::write(dir.join("broken.json"), "{ bar: ").unwrap();
        assert!(read_options(&dir.join("broken.json")).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ri-seed-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("layout.toml")
    }

    #[test]
    fn first_run_writes_the_migrated_layout_once() {
        let path = scratch("first");
        let layout = from_options(&owner(), &monitor());
        let mut live = super::super::Live::open(&path);
        assert!(live.first_run());
        assert!(live.seed(layout.clone()).unwrap());
        assert_eq!(live.editor().layout(), &layout);
        assert!(!live.first_run());
        // The next start reads it back; seeding again changes nothing.
        let mut again = super::super::Live::open(&path);
        assert_eq!(again.source(), super::super::LoadSource::Main);
        assert_eq!(again.editor().layout(), &layout);
        assert!(
            !again
                .seed(from_options(
                    &json!({"bar": {"vertical": true}}),
                    &monitor()
                ))
                .unwrap()
        );
        assert_eq!(super::super::Live::open(&path).editor().layout(), &layout);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn an_existing_or_unreadable_layout_is_never_replaced() {
        for (name, main, prev) in [
            ("user", Some("version = 1\n"), None),
            ("broken", Some("this is not toml ["), None),
            ("future", Some("version = 999\n"), None),
            ("prev", None, Some("version = 1\n")),
        ] {
            let path = scratch(name);
            let store = super::super::Store::open(&path).0;
            if let Some(t) = main {
                std::fs::write(&path, t).unwrap();
            }
            if let Some(t) = prev {
                std::fs::write(store.previous_path(), t).unwrap();
            }
            let mut live = super::super::Live::open(&path);
            assert!(!live.first_run(), "{name}");
            assert!(
                !live.seed(from_options(&json!({}), &monitor())).unwrap(),
                "{name}"
            );
            assert_eq!(
                std::fs::read_to_string(&path).ok().as_deref(),
                main,
                "{name}"
            );
            std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
        }
    }
}
