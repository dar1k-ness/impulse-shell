//! The original's bar as the original's layout imports it (the standard seed), outside edit mode. On a vertical
//! (left) panel every run is as long as its widgets at their natural lengths (a stretching
//! widget's least length never pushes it past them), so the runs sit where the original's
//! VerticalBarContent puts them; on a 1280 logical top bar everything shows.
#![cfg(feature = "interp")]

use qml::runtime::{Runtime, World};
use qml::value::Value;
use shell::scene;
use std::path::Path;

thread_local!(static CONSOLE: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) });

const LAYOUT: &str = r#"version = 5
[[monitor]]
connector = "DP-1"
primary = true

[[monitor.container]]
id = "panel"
kind = "panel"
"#;

const WIDGETS: [(&str, &str, &str); 13] = [
    ("left-sidebar-button", "start", "auto"),
    ("active-window", "start", "stretch"),
    ("resources", "center", "auto"),
    ("media", "center", "stretch"),
    ("separator-1", "center", "auto"),
    ("workspaces", "center", "auto"),
    ("separator-2", "center", "auto"),
    ("clock", "center", "stretch"),
    ("util-buttons", "center", "auto"),
    ("battery", "center", "auto"),
    ("spacer", "end", "stretch"),
    ("systray", "end", "auto"),
    ("indicators", "end", "auto"),
];

/// A bar of `window` logical px on a `screen`, the panel at `edge` ("rusty" for the original's top bar,
/// with `thickness` for a vertical one).
struct Bar {
    edge: &'static str,
    /// The original's bar weather option: the weather widget before the end zone's spacer.
    weather: bool,
    window: [f64; 2],
    screen: [f64; 2],
}

const VERTICAL: Bar = Bar {
    edge: "\"left\"\nthickness = 46",
    weather: false,
    window: [46.0, 1080.0],
    screen: [1920.0, 1080.0],
};

/// A shown run of the panel: its first widget's layout id, where it starts along the panel and
/// how long it is in the window, and the length its widgets take at their natural lengths.
#[derive(Debug)]
struct Run {
    first: String,
    at: f64,
    length: f64,
    natural: f64,
}

/// The bar's shown runs in order along it, and the panel's log lines.
fn lay_out(bar: &Bar) -> (Vec<Run>, Vec<String>) {
    let vertical = bar.window[0] < bar.window[1];
    let ri = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../qml")
        .canonicalize()
        .unwrap();
    let d = std::env::temp_dir().join(format!(
        "panel-seed-{}-{}-{}",
        std::process::id(),
        bar.window[0],
        bar.weather
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    for e in std::fs::read_dir(&ri).unwrap() {
        let e = e.unwrap();
        std::os::unix::fs::symlink(e.path(), d.join(e.file_name())).unwrap();
    }
    let mut layout = format!("{LAYOUT}edge = {}\n", bar.edge);
    for (id, zone, width) in WIDGETS {
        if id == "spacer" && bar.weather {
            layout += "\n[[monitor.container.widget]]\nid = \"weather\"\ntype = \"weather\"\nzone = \"end\"\n";
        }
        let ty = if id.starts_with("separator") {
            "separator"
        } else {
            id
        };
        layout += &format!(
            "\n[[monitor.container.widget]]\nid = \"{id}\"\ntype = \"{ty}\"\nzone = \"{zone}\"\nwidth = \"{width}\"\n"
        );
    }
    std::fs::write(d.join("layout.toml"), layout).unwrap();
    let [w, h] = bar.window;
    let [sw, sh] = bar.screen;
    std::fs::write(
        d.join("main.qml"),
        format!(
            "import QtQuick\nimport ri.modules.common\nimport ri.modules.rusty.bar.panel\nItem {{\n    width: {w}\n    height: {h}\n    \
             Component.onCompleted: Config.options.bar.weather.enable = {weather}\n    \
             PanelContent {{\n        anchors.fill: parent\n        \
             screen: ({{ name: \"DP-1\", width: {sw}, height: {sh} }})\n    }}\n}}\n",
            weather = bar.weather
        ),
    )
    .unwrap();
    qml::builtins::CONSOLE_SINK
        .get_or_init(|| |_, line| CONSOLE.with_borrow_mut(|c| c.push(line.to_owned())));
    CONSOLE.with_borrow_mut(Vec::clear);
    let mut rt = Runtime::new(Some(d.clone()));
    rt.load_singletons(&[
        d.join("modules/common"),
        d.join("modules/common/functions"),
        d.join("services"),
        d.clone(),
    ]);
    rt.world.borrow_mut().open_layout(d.join("layout.toml"));
    rt.settle();
    let main = rt.load_file(&d.join("main.qml")).unwrap();
    rt.settle();
    let mut fonts = scene::Fonts::default();
    let mut now = 1000.0;
    // Frames enough for the widgets' loaders and their entry animations.
    for _ in 0..200 {
        now += 16.0;
        rt.tick(now);
        rt.settle();
        scene::layout_window(&mut rt, &mut fonts, main, bar.window);
        rt.settle();
    }
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    let log: Vec<String> = CONSOLE.with_borrow(|c| {
        c.iter()
            .filter(|l| l.starts_with("[Panel]"))
            .cloned()
            .collect()
    });
    let (length, natural) = if vertical {
        ("height", "implicitHeight")
    } else {
        ("width", "implicitWidth")
    };
    let mut w = rt.world.borrow_mut();
    let num =
        |w: &mut World, i: usize, k: &str| w.read(i, k).unwrap_or(Value::Num(0.0)).to_number();
    let ids: Vec<usize> = (0..w.items.len())
        .filter(|&i| !w.dead.contains(&i) && w.items[i].types.iter().any(|t| t == "PanelRun"))
        .collect();
    let mut out = vec![];
    for r in ids {
        if !w.read(r, "visible").is_some_and(|v| v.truthy()) || num(&mut w, r, length) <= 0.0 {
            continue;
        }
        let first = match w.read(r, "modelData") {
            Some(Value::Obj(o)) => match o.borrow().props.get("entries") {
                Some(Value::Obj(a)) => {
                    a.borrow()
                        .array
                        .as_ref()
                        .and_then(|v| v.first())
                        .and_then(|e| match e {
                            Value::Obj(e) => e.borrow().props.get("id").map(|v| v.to_js_string()),
                            _ => None,
                        })
                }
                _ => None,
            },
            _ => None,
        }
        .unwrap_or_default();
        let pos = w.scene_pos(r, None);
        out.push(Run {
            first,
            at: if vertical { pos.1 } else { pos.0 },
            length: num(&mut w, r, length),
            natural: num(&mut w, r, natural),
        });
    }
    drop(w);
    std::fs::remove_dir_all(&d).unwrap();
    out.sort_by(|a, b| a.at.total_cmp(&b.at));
    (out, log)
}

/// Where the original's VerticalBarContent puts each group on a 1080 px left bar, as `qs -c rusty-impulse` and the
/// base lay them out: media's least length (24) is more than it takes with nothing playing, and
/// asking for it would move the top group up and the groups below it down.
#[test]
fn a_vertical_bar_puts_its_groups_where_the_original_does() {
    let (runs, log) = lay_out(&VERTICAL);
    let report = format!("{runs:#?} {log:?}");
    let at: Vec<(&str, f64, f64)> = runs
        .iter()
        .map(|r| (r.first.as_str(), r.at, r.length))
        .collect();
    assert_eq!(
        at,
        [
            ("left-sidebar-button", 5.0, 39.0),
            ("resources", 295.0, 135.0),
            ("workspaces", 434.0, 272.0),
            ("clock", 710.0, 74.0),
            ("spacer", 784.0, 238.0),
            ("indicators", 1026.0, 31.0),
        ],
        "{report}"
    );
    for r in runs.iter().filter(|r| r.first != "spacer") {
        assert_eq!(
            r.length, r.natural,
            "{} is as long as its widgets: {report}",
            r.first
        );
    }
    assert!(log.is_empty(), "{report}");
}

/// Shown runs along the bar, each within it and clear of the next.
fn assert_laid_out(runs: &[Run], length: f64, report: &str) {
    for r in runs {
        assert!(
            r.at >= 0.0 && r.at + r.length <= length,
            "{} on the bar: {report}",
            r.first
        );
    }
    for p in runs.windows(2) {
        assert!(
            p[0].at + p[0].length <= p[1].at,
            "{} clear of {}: {report}",
            p[0].first,
            p[1].first
        );
    }
}

/// The original's top bar on a 1920 px monitor at 1.5x.
fn standard_bar_at_1280_logical(weather: bool) -> (Vec<Run>, Vec<String>, String) {
    standard_bar(1280.0, weather)
}

/// The original's top bar on a monitor `width` logical px wide.
fn standard_bar(width: f64, weather: bool) -> (Vec<Run>, Vec<String>, String) {
    let (runs, log) = lay_out(&Bar {
        edge: "\"rusty\"",
        weather,
        window: [width, 40.0],
        screen: [width, 720.0],
    });
    let report = format!("{runs:#?} {log:?}");
    assert_laid_out(&runs, width, &report);
    (runs, log, report)
}

fn firsts(runs: &[Run]) -> Vec<&str> {
    runs.iter().map(|r| r.first.as_str()).collect()
}

#[test]
fn the_standard_bar_at_1280_logical_shows_everything() {
    let (runs, log, report) = standard_bar_at_1280_logical(false);
    assert!(log.is_empty(), "{report}");
    assert_eq!(
        firsts(&runs),
        [
            "left-sidebar-button",
            "active-window",
            "resources",
            "workspaces",
            "clock",
            "spacer",
            "indicators"
        ],
        "{report}"
    );
}

/// The parity harness's seed, the original's bar weather on, has room at 1280 logical for all it
/// holds, the weather between the clock group and the right sidebar's indicators.
#[test]
fn the_standard_bar_with_weather_at_1280_logical_shows_everything() {
    let (runs, log, report) = standard_bar_at_1280_logical(true);
    assert!(log.is_empty(), "{report}");
    assert_eq!(
        firsts(&runs),
        [
            "left-sidebar-button",
            "active-window",
            "resources",
            "workspaces",
            "clock",
            "weather",
            "spacer",
            "indicators"
        ],
        "{report}"
    );
}

/// The same seed is longer than a bar of 1260 logical: each side's innermost widget gives way
/// (the end zone's weather, the start zone's window title, whose stretching run stays as free
/// space) and the sidebar buttons at the bar's edges stay.
#[test]
fn the_standard_bar_with_weather_at_1260_logical_keeps_its_sidebar_buttons() {
    let (runs, log, report) = standard_bar(1260.0, true);
    assert_eq!(
        log,
        [
            "[Panel] DP-1 start zone has no room for active-window: hidden",
            "[Panel] DP-1 end zone has no room for weather: hidden"
        ],
        "{report}"
    );
    assert_eq!(
        firsts(&runs),
        [
            "left-sidebar-button",
            "active-window",
            "resources",
            "workspaces",
            "clock",
            "spacer",
            "indicators"
        ],
        "{report}"
    );
}

/// A bar too short for the original's center groups: the center gives way, from its end, before either
/// side's widget at the bar's edge (the sidebar buttons); nothing leaves the bar or overlaps.
#[test]
fn a_short_bar_keeps_its_edge_widgets_and_the_center_gives_way() {
    let (runs, log) = lay_out(&Bar {
        edge: "\"rusty\"",
        weather: true,
        window: [1000.0, 40.0],
        screen: [1280.0, 720.0],
    });
    let report = format!("{runs:#?} {log:?}");
    assert_laid_out(&runs, 1000.0, &report);
    assert_eq!(
        runs.first().map(|r| r.first.as_str()),
        Some("left-sidebar-button"),
        "{report}"
    );
    assert_eq!(
        runs.last().map(|r| r.first.as_str()),
        Some("indicators"),
        "{report}"
    );
    assert!(
        log.iter()
            .any(|l| l.contains("center zone has no room for")),
        "{report}"
    );
    assert!(
        !log.iter()
            .any(|l| l.contains("left-sidebar-button") || l.contains("indicators")),
        "{report}"
    );
}
