//! `WidgetHost` loads the compiled-in widget a layout entry names by `type` from the root's
//! `widgets.toml` registry and hands it its settings, validated against the type's schema.
use qml::runtime::Runtime;
use qml::value::Value;
use std::path::PathBuf;

const MANIFEST: &str = r##"
[[widget]]
type = "clock"
name = "Clock"
icon = "schedule"
placements = ["panel", "desktop"]
component = "widgets/Clock.qml"
size = { min = [80, 24], max = [400, 200], default = [120, 32] }

[[widget.setting]]
key = "showSeconds"
label = "Show seconds"
kind = "bool"
default = false

[[widget.setting]]
key = "accent"
kind = "color"
default = "#ff8800"

[[widget.setting]]
key = "style"
kind = "choice"
options = ["digital", "analog"]
default = "digital"

[[widget.setting]]
key = "scale"
kind = "real"
min = 0.5
max = 3
default = 1

[[widget]]
type = "media"
name = "Media"
icon = "music_note"

[widget.panel]
component = "widgets/MediaPill.qml"
size = { min = [24, 24], default = [120, 24] }
pill = true

[widget.sidebar]
component = "widgets/MediaCard.qml"
size = { min = [2, 1], default = [4, 1] }

[[widget.setting]]
key = "showArtist"
kind = "bool"
default = true

[[widget]]
type = "battery"
name = "Battery"
icon = "battery_full"
placements = ["panel"]
component = "widgets/Battery.qml"
size = { min = [24, 24], default = [40, 24] }
"##;

const CLOCK: &str = r#"import QtQuick
Item {
    property bool showSeconds: true
    property color accent: "black"
    property string style: ""
    property real scale: 0
    property string kind: "clock"
}
"#;

const BATTERY: &str = r#"import QtQuick
Item { property string kind: "battery" }
"#;

const MEDIA_PILL: &str = r#"import QtQuick
Item {
    property bool showArtist: false
    property string kind: "pill"
}
"#;

const MEDIA_CARD: &str = r#"import QtQuick
Item {
    property bool showArtist: false
    property string kind: "card"
}
"#;

fn root(tag: &str, main: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("widget-host-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("widgets")).unwrap();
    std::fs::write(d.join("widgets.toml"), MANIFEST).unwrap();
    std::fs::write(d.join("widgets/Clock.qml"), CLOCK).unwrap();
    std::fs::write(d.join("widgets/Battery.qml"), BATTERY).unwrap();
    std::fs::write(d.join("widgets/MediaPill.qml"), MEDIA_PILL).unwrap();
    std::fs::write(d.join("widgets/MediaCard.qml"), MEDIA_CARD).unwrap();
    std::fs::write(d.join("main.qml"), main).unwrap();
    d
}

fn start(tag: &str, main: &str) -> (Runtime, usize) {
    let d = root(tag, main);
    let mut rt = Runtime::new(Some(d.clone()));
    let id = rt.load_file(&d.join("main.qml")).unwrap();
    rt.settle();
    (rt, id)
}

fn read(rt: &Runtime, id: usize, prop: &str) -> Value {
    rt.get(id, prop)
}

fn s(rt: &Runtime, id: usize, prop: &str) -> String {
    read(rt, id, prop).to_js_string()
}

fn item(rt: &Runtime, host: usize) -> Option<usize> {
    match read(rt, host, "item") {
        Value::Item(i) => Some(i),
        _ => None,
    }
}

fn set(rt: &mut Runtime, id: usize, prop: &str, v: Value) {
    rt.world.borrow_mut().write(id, prop, v);
    rt.settle();
}

fn obj(pairs: &[(&str, Value)]) -> Value {
    let o = qml::eval::new_obj();
    for (k, v) in pairs {
        qml::eval::set_own(&o, k, v.clone());
    }
    Value::Obj(o)
}

const HOST: &str = r#"import QtQuick
import Impulse
WidgetHost {
    widgetType: "clock"
    settings: ({ showSeconds: true, style: "analog" })
}
"#;

#[test]
fn loads_the_registered_widget_with_typed_settings() {
    let (rt, h) = start("load", HOST);
    let w = item(&rt, h).expect("clock loaded");
    assert_eq!(s(&rt, w, "kind"), "clock");
    assert_eq!(s(&rt, h, "status"), "1");
    assert_eq!(s(&rt, h, "errorString"), "");
    // Overrides win; settings the user never set get the schema default, not the
    // component's own initializer.
    assert_eq!(s(&rt, w, "showSeconds"), "true");
    assert_eq!(s(&rt, w, "style"), "analog");
    assert_eq!(s(&rt, w, "scale"), "1");
    assert_eq!(s(&rt, w, "accent"), "#ff8800");
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
}

#[test]
fn settings_changes_reach_the_loaded_widget_live() {
    let (mut rt, h) = start("live", HOST);
    let w = item(&rt, h).unwrap();
    set(&mut rt, h, "settings", obj(&[("scale", Value::Num(2.5))]));
    assert_eq!(
        item(&rt, h),
        Some(w),
        "a settings change must not reload the widget"
    );
    assert_eq!(s(&rt, w, "scale"), "2.5");
    // Keys dropped from the overrides fall back to their defaults.
    assert_eq!(s(&rt, w, "showSeconds"), "false");
    assert_eq!(s(&rt, w, "style"), "digital");
}

#[test]
fn invalid_settings_fall_back_to_defaults_and_are_reported() {
    let (mut rt, h) = start("invalid", HOST);
    let w = item(&rt, h).unwrap();
    set(
        &mut rt,
        h,
        "settings",
        obj(&[
            ("scale", Value::Num(9.0)),
            ("style", Value::str("sundial")),
            ("showSeconds", Value::str("yes")),
            ("accent", Value::str("not-a-color")),
            ("bogus", Value::Bool(true)),
        ]),
    );
    assert_eq!(s(&rt, w, "scale"), "1");
    assert_eq!(s(&rt, w, "style"), "digital");
    assert_eq!(s(&rt, w, "showSeconds"), "false");
    assert_eq!(s(&rt, w, "accent"), "#ff8800");
    let err = s(&rt, h, "errorString");
    for frag in [
        "\"bogus\"",
        "\"scale\" is out of range",
        "\"sundial\"",
        "\"showSeconds\" must be a bool",
        "\"not-a-color\"",
    ] {
        assert!(err.contains(frag), "{frag} missing from {err}");
    }
    assert_eq!(
        s(&rt, h, "status"),
        "1",
        "a bad setting must not unload the widget"
    );
}

/// The host resyncs when `settings` is assigned, as a `var` property notifies; mutating the
/// object it holds in place notifies nothing, so the widget keeps what it was given.
#[test]
fn settings_resync_on_assignment_not_on_every_pass() {
    let (mut rt, h) = start("fresh", HOST);
    let w = item(&rt, h).unwrap();
    let Value::Obj(o) = read(&rt, h, "settings") else {
        panic!("settings object")
    };
    qml::eval::set_own(&o, "style", Value::str("digital"));
    rt.settle();
    assert_eq!(s(&rt, w, "style"), "analog");
    set(
        &mut rt,
        h,
        "settings",
        obj(&[("style", Value::str("digital"))]),
    );
    assert_eq!(s(&rt, w, "style"), "digital");
}

#[test]
fn changing_type_swaps_the_widget() {
    let (mut rt, h) = start("swap", HOST);
    let clock = item(&rt, h).unwrap();
    set(&mut rt, h, "widgetType", Value::str("battery"));
    let bat = item(&rt, h).expect("battery loaded");
    assert_ne!(bat, clock);
    assert_eq!(s(&rt, bat, "kind"), "battery");
    assert!(
        rt.world.borrow().dead.contains(&clock),
        "old widget destroyed"
    );
}

#[test]
fn unknown_type_loads_nothing_and_reports_error() {
    let (mut rt, h) = start(
        "unknown",
        "import QtQuick\nimport Impulse\nWidgetHost { widgetType: \"weather\" }\n",
    );
    assert_eq!(item(&rt, h), None);
    assert_eq!(s(&rt, h, "status"), "2");
    assert_eq!(s(&rt, h, "errorString"), "unknown widget type \"weather\"");
    // A later fix of the type loads the widget.
    set(&mut rt, h, "widgetType", Value::str("battery"));
    assert!(item(&rt, h).is_some());
    assert_eq!(s(&rt, h, "status"), "1");
    assert_eq!(s(&rt, h, "errorString"), "");
}

#[test]
fn inactive_host_unloads() {
    let (mut rt, h) = start("inactive", HOST);
    let w = item(&rt, h).unwrap();
    set(&mut rt, h, "active", Value::Bool(false));
    assert_eq!(item(&rt, h), None);
    assert!(rt.world.borrow().dead.contains(&w));
    assert_eq!(s(&rt, h, "status"), "0");
}

#[test]
fn bad_manifest_entries_are_reported_and_skipped() {
    let d = root("badmanifest", HOST);
    std::fs::write(
        d.join("widgets.toml"),
        format!(
            "{MANIFEST}\n[[widget]]\ntype = \"clock\"\nname = \"Dup\"\nplacements = [\"panel\"]\ncomponent = \"x.qml\"\nsize = {{ min = [1, 1], default = [1, 1] }}\n"
        ),
    )
    .unwrap();
    let rt = Runtime::new(Some(d));
    assert_eq!(rt.widgets().iter().count(), 3);
    assert!(
        rt.errors()
            .iter()
            .any(|e| e.contains("\"clock\" is registered twice")),
        "{:?}",
        rt.errors()
    );
}

const MEDIA_HOST: &str = r#"import QtQuick
import Impulse
WidgetHost {
    widgetType: "media"
    surface: "sidebar"
    settings: ({ showArtist: false })
}
"#;

#[test]
fn the_host_loads_its_surfaces_variant_with_the_same_settings() {
    let (mut rt, h) = start("surface", MEDIA_HOST);
    let card = item(&rt, h).expect("sidebar variant loaded");
    assert_eq!(s(&rt, card, "kind"), "card");
    assert_eq!(s(&rt, card, "showArtist"), "false");
    assert_eq!(s(&rt, h, "status"), "1");
    // Moved to a panel, the widget becomes the panel variant and keeps its settings.
    set(&mut rt, h, "surface", Value::str("panel"));
    let pill = item(&rt, h).expect("panel variant loaded");
    assert_ne!(pill, card);
    assert_eq!(s(&rt, pill, "kind"), "pill");
    assert_eq!(s(&rt, pill, "showArtist"), "false");
    assert!(
        rt.world.borrow().dead.contains(&card),
        "old variant destroyed"
    );
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
}

#[test]
fn a_surface_the_type_has_no_variant_for_is_reported() {
    let (mut rt, h) = start("no-variant", MEDIA_HOST);
    set(&mut rt, h, "surface", Value::str("desktop"));
    assert_eq!(item(&rt, h), None);
    assert_eq!(s(&rt, h, "status"), "2");
    assert_eq!(
        s(&rt, h, "errorString"),
        "widget type \"media\" has no desktop variant"
    );
    // A host that does not say where it is cannot pick between several variants.
    set(&mut rt, h, "surface", Value::str(""));
    assert_eq!(item(&rt, h), None);
    assert_eq!(s(&rt, h, "status"), "2");
    // A type with one component on all its surfaces needs no `surface`, as HOST's clock.
    set(&mut rt, h, "surface", Value::str("sidebar"));
    assert!(item(&rt, h).is_some());
    assert_eq!(s(&rt, h, "status"), "1");
}
