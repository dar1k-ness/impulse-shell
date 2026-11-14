//! qml's panel zones on a real original bar (the layout merge train's guest seeds: every quick
//! toggle at the end, before the tray and indicators) at 1920 and at 1280 logical (1920 at
//! 1.5x): every widget keeps its natural size (Qt Quick Layouts: a widget's minimum is its
//! implicit size, the spacer's 0), and a zone without room for them all hides the ones
//! nearest the bar's middle with one log line, so nothing is laid out outside the bar and the
//! other zones keep their place.
#![cfg(feature = "interp")]

use qml::runtime::{Runtime, World};
use qml::value::Value;
use shell::scene;
use std::path::{Path, PathBuf};
#[path = "support/frames.rs"]
mod frames;

thread_local!(static CONSOLE: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) });

const TOGGLES: [&str; 17] = [
    "antiFlashbang",
    "audio",
    "bluetooth",
    "cloudflareWarp",
    "colorPicker",
    "darkMode",
    "easyEffects",
    "gameMode",
    "idleInhibitor",
    "mic",
    "musicRecognition",
    "network",
    "nightLight",
    "notifications",
    "onScreenKeyboard",
    "powerProfile",
    "screenSnip",
];

/// The original's bar as the layout imports it, with `toggles` before the tray and indicators.
fn layout(toggles: &[&str]) -> String {
    let mut s = String::from(
        "version = 5\n[[monitor]]\nconnector = \"DP-1\"\nprimary = true\n\n\
         [[monitor.container]]\nid = \"panel\"\nkind = \"panel\"\nedge = \"rusty\"\n",
    );
    let mut w = |id: &str, ty: &str, zone: &str, width: &str| {
        s += &format!(
            "\n[[monitor.container.widget]]\nid = \"{id}\"\ntype = \"{ty}\"\nzone = \"{zone}\"\nwidth = \"{width}\"\n"
        );
    };
    for (id, zone, width) in [
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
    ] {
        let ty = if id.starts_with("separator") {
            "separator"
        } else {
            id
        };
        w(id, ty, zone, width);
    }
    for t in toggles {
        w(&format!("t-{t}"), t, "end", "auto");
    }
    w("systray", "systray", "end", "auto");
    w("indicators", "indicators", "end", "auto");
    s
}

/// The original's bar content on a `width` wide screen.
fn main_qml(width: u32) -> String {
    format!(
        r#"import QtQuick
import ri.modules.rusty.bar.panel
Item {{
    width: {width}
    height: 40
    PanelContent {{
        anchors.fill: parent
        screen: ({{ name: "DP-1", width: {width}, height: 1080 }})
    }}
}}
"#
    )
}

fn tree(width: u32, toggles: &[&str]) -> PathBuf {
    let ri = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../qml")
        .canonicalize()
        .unwrap();
    let d = std::env::temp_dir().join(format!(
        "panel-overflow-{width}-{}-{}",
        toggles.len(),
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    for e in std::fs::read_dir(&ri).unwrap() {
        let e = e.unwrap();
        std::os::unix::fs::symlink(e.path(), d.join(e.file_name())).unwrap();
    }
    std::fs::write(d.join("layout.toml"), layout(toggles)).unwrap();
    std::fs::write(d.join("main.qml"), main_qml(width)).unwrap();
    d
}

/// A shown host's layout id, x in the window, width, its widget's implicit width and whether
/// it stretches.
#[derive(Debug, PartialEq)]
struct Host {
    id: String,
    x: f64,
    width: f64,
    natural: f64,
    stretch: bool,
}

/// The bar once Config and the widgets' loaders came in: its shown widgets left to right, the
/// room its end zone reports for one more widget and the panel's log lines.
struct Bar {
    hosts: Vec<Host>,
    /// The start, center and end zones: x and width.
    zones: [(f64, f64); 3],
    room: f64,
    log: Vec<String>,
}

fn bar(width: u32, toggles: &[&str]) -> Bar {
    let d = tree(width, toggles);
    qml::builtins::CONSOLE_SINK
        .get_or_init(|| |_, line| CONSOLE.with_borrow_mut(|c| c.push(line.to_owned())));
    CONSOLE.with_borrow_mut(Vec::clear);
    let mut rt = Runtime::new(Some(d.clone()));
    let mut frames = frames::Frames::new(&mut rt);
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
    let mut layout =
        |rt: &mut Runtime| scene::layout_window(rt, &mut fonts, main, [f64::from(width), 40.0]);
    frames.until_config(&mut rt, &mut layout);
    frames.frames(&mut rt, &mut layout, 60);
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    let mut w = rt.world.borrow_mut();
    let num =
        |w: &mut World, i: usize, k: &str| w.read(i, k).unwrap_or(Value::Num(0.0)).to_number();
    let room = w
        .of_type("PanelContainer")
        .into_iter()
        .map(|p| num(&mut w, p, "endRoom"))
        .next()
        .unwrap();
    let mut zones = [(f64::NAN, f64::NAN); 3];
    for z in w.of_type("GridLayout") {
        let label = w.read(z, "label").unwrap_or_default().to_js_string();
        if let Some(k) = ["DP-1 start", "DP-1 center", "DP-1 end"]
            .iter()
            .position(|l| *l == label)
        {
            zones[k] = (w.scene_pos(z, None).0, num(&mut w, z, "width"));
        }
    }
    let mut hosts = vec![];
    let ids: Vec<usize> = (0..w.items.len())
        .filter(|&i| !w.dead.contains(&i) && w.items[i].types.iter().any(|t| t == "WidgetHost"))
        .collect();
    for h in ids {
        if !w.read(h, "visible").is_some_and(|v| v.truthy()) {
            continue;
        }
        let Some(Value::Item(item)) = w.read(h, "item") else {
            continue;
        };
        hosts.push(Host {
            id: w.read(h, "layoutId").unwrap_or_default().to_js_string(),
            x: w.scene_pos(h, None).0,
            width: num(&mut w, h, "width"),
            natural: num(&mut w, item, "implicitWidth"),
            stretch: w
                .read(h, "entry")
                .and_then(|e| match e {
                    Value::Obj(o) => o.borrow().props.get("width").map(|v| v.to_js_string()),
                    _ => None,
                })
                .is_some_and(|v| v == "stretch"),
        });
    }
    drop(w);
    std::fs::remove_dir_all(&d).unwrap();
    hosts.sort_by(|a, b| a.x.total_cmp(&b.x));
    let log = CONSOLE.with_borrow(|c| {
        c.iter()
            .filter(|l| l.starts_with("[Panel]"))
            .cloned()
            .collect()
    });
    Bar {
        hosts,
        zones,
        room,
        log,
    }
}

/// Nothing outside the bar, nothing squashed, nothing overlapping; the start and center zones
/// where they are without the toggles; the end zone's hidden toggles the ones nearest the
/// middle, named by the zone's last log line; no room left for edit mode. Returns how many
/// toggles show.
fn check(width: u32) -> usize {
    let b = bar(width, &TOGGLES);
    let report = format!("{:#?}\nend room {}\nlog {:#?}", b.hosts, b.room, b.log);
    let w = f64::from(width);
    for h in &b.hosts {
        assert!(
            h.x >= 0.0 && h.x + h.width <= w,
            "{} outside the bar: {report}",
            h.id
        );
        if !h.stretch {
            assert!(h.width >= h.natural, "{} squashed: {report}", h.id);
        }
    }
    for p in b.hosts.windows(2) {
        assert!(
            p[1].x >= p[0].x + p[0].width,
            "{} overlaps {}: {report}",
            p[0].id,
            p[1].id
        );
    }
    // The zones keep their place: the center one centered, each widget inside its own zone,
    // the zones apart.
    let [start, center, end] = b.zones;
    assert!(
        (center.0 + center.1 / 2.0 - w / 2.0).abs() <= 0.5,
        "{:?}: {report}",
        b.zones
    );
    assert!(
        start.0 + start.1 <= center.0 && center.0 + center.1 <= end.0,
        "{:?}: {report}",
        b.zones
    );
    assert!(end.0 + end.1 <= w, "{:?}: {report}", b.zones);
    let zone = |id: &str| match id {
        "left-sidebar-button" | "active-window" => start,
        id if id.starts_with("t-") || ["spacer", "systray", "indicators"].contains(&id) => end,
        _ => center,
    };
    for h in &b.hosts {
        let z = zone(&h.id);
        assert!(
            h.x >= z.0 && h.x + h.width <= z.0 + z.1,
            "{} outside its zone {z:?}: {report}",
            h.id
        );
    }
    assert!(b.hosts.iter().any(|h| h.id == "indicators"), "{report}");
    let shown: Vec<&str> = TOGGLES
        .iter()
        .copied()
        .filter(|t| b.hosts.iter().any(|h| h.id == format!("t-{t}")))
        .collect();
    let hidden = TOGGLES.len() - shown.len();
    assert!(hidden > 0, "{report}");
    assert_eq!(
        shown,
        TOGGLES[hidden..],
        "the toggles nearest the middle wait: {report}"
    );
    let names: Vec<String> = TOGGLES[..hidden]
        .iter()
        .rev()
        .map(|t| format!("t-{t}"))
        .collect();
    let ends: Vec<&String> = b.log.iter().filter(|l| l.contains(" end zone ")).collect();
    assert_eq!(
        ends.last().map(|l| l.as_str()),
        Some(
            format!(
                "[Panel] DP-1 end zone has no room for {}: hidden",
                names.join(", ")
            )
            .as_str()
        ),
        "{report}"
    );
    assert_eq!(
        b.room, 0.0,
        "edit mode refuses another widget there: {report}"
    );
    shown.len()
}

#[test]
fn seventeen_toggles_on_a_1920_bar_stay_on_it_at_natural_size() {
    let shown = check(1920);
    // The spacer gave way first: what shows fills the end zone.
    assert!(shown >= 10, "{shown}");
}

#[test]
fn seventeen_toggles_on_a_1280_logical_bar_stay_on_it_at_natural_size() {
    check(1280);
}

#[test]
fn toggles_that_fit_all_show_and_leave_room() {
    let b = bar(1920, &TOGGLES[..4]);
    assert_eq!(b.hosts.iter().filter(|h| h.id.starts_with("t-")).count(), 4);
    assert!(
        !b.log.iter().any(|l| l.contains(" end zone has no room")),
        "{:?}",
        b.log
    );
    let spacer = b
        .hosts
        .iter()
        .find(|h| h.id == "spacer")
        .map_or(0.0, |s| s.width);
    assert!(
        b.room > 0.0 && b.room <= spacer,
        "spacer {spacer}, room {}",
        b.room
    );
}
