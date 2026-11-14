//! `Impulse/WidgetSettings` is the model behind edit mode's widget settings dialog: one field per
//! setting the widget type declares, accepted values written to `layout.toml` as one undo step
//! each, rejected values kept in their field with an inline message and never written.
use qml::runtime::Runtime;
use qml::value::Value;
use std::path::{Path, PathBuf};

/// `Value` has no `PartialEq` (JS equality is not structural); compare the debug forms.
macro_rules! assert_val {
    ($a:expr, $b:expr $(, $msg:expr)?) => {
        assert_eq!(format!("{:?}", $a), format!("{:?}", $b) $(, $msg)?)
    };
}

const MANIFEST: &str = r##"
[[widget]]
type = "clock"
name = "Clock"
icon = "schedule"
placements = ["desktop"]
component = "widgets/Clock.qml"
size = { min = [100, 50], default = [300, 150] }

[[widget.setting]]
key = "showDate"
label = "Show date"
kind = "bool"
default = true

[[widget.setting]]
key = "columns"
label = "Columns"
kind = "int"
min = 1
max = 10
default = 2

[[widget.setting]]
key = "opacity"
label = "Opacity"
kind = "real"
min = 0.0
max = 1.0
default = 1.0

[[widget.setting]]
key = "format"
label = "Format"
kind = "string"
default = "hh:mm"

[[widget.setting]]
key = "accent"
label = "Accent"
kind = "color"
default = "#6750a4"

[[widget.setting]]
key = "style"
label = "Style"
kind = "choice"
options = ["digital", "analog", "words"]
default = "digital"

[[widget]]
type = "plain"
name = "Plain"
icon = "crop_square"
placements = ["desktop"]
component = "widgets/Clock.qml"
size = { min = [10, 10], default = [20, 20] }
"##;

const LAYOUT: &str = r#"version = 5

[[monitor]]
connector = "DP-1"
primary = true

[[monitor.container]]
id = "desktop"
kind = "desktop"

[[monitor.container.widget]]
id = "clock"
type = "clock"
x = 100
y = 100
w = 300
h = 150
settings = { style = "analog" }

[[monitor.container.widget]]
id = "plain"
type = "plain"
x = 600
y = 100
w = 20
h = 20
"#;

const MAIN: &str = r#"import QtQuick
import Impulse
WidgetSettings {
    connector: "DP-1"
    widgetId: "clock"
    function set(k: string, v: var): bool { return setValue(k, v) }
    function reset(k: string): bool { return resetValue(k) }
    function back(): bool { return undo() }
    function forward(): bool { return redo() }
}
"#;

fn root(tag: &str, layout: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("widget-settings-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("widgets")).unwrap();
    std::fs::write(d.join("widgets.toml"), MANIFEST).unwrap();
    std::fs::write(d.join("widgets/Clock.qml"), "import QtQuick\nItem {}\n").unwrap();
    std::fs::write(d.join("main.qml"), MAIN).unwrap();
    std::fs::write(d.join("layout.toml"), layout).unwrap();
    d
}

fn start(d: &Path) -> (Runtime, usize) {
    let mut rt = Runtime::new(Some(d.to_path_buf()));
    rt.world.borrow_mut().open_layout(d.join("layout.toml"));
    let id = rt.load_file(&d.join("main.qml")).unwrap();
    rt.settle();
    (rt, id)
}

fn call(rt: &mut Runtime, id: usize, f: &str, args: Vec<Value>) -> bool {
    let v = rt.call_method(id, f, args).unwrap().unwrap();
    rt.settle();
    v.truthy()
}

fn set(rt: &mut Runtime, id: usize, k: &str, v: Value) -> bool {
    call(rt, id, "set", vec![Value::str(k), v])
}

fn read(rt: &Runtime, id: usize, k: &str) -> Value {
    rt.world.borrow_mut().read(id, k).unwrap_or_default()
}

#[derive(Debug)]
struct Field {
    kind: String,
    label: String,
    value: Value,
    min: Value,
    max: Value,
    options: Vec<String>,
    error: String,
    input: String,
    is_default: bool,
}

fn array(v: &Value) -> Vec<Value> {
    let Value::Obj(o) = v else {
        panic!("not an array: {v:?}")
    };
    o.borrow().array.clone().unwrap_or_default().to_vec()
}

fn fields(rt: &Runtime, id: usize) -> Vec<(String, Field)> {
    array(&read(rt, id, "fields"))
        .iter()
        .map(|f| {
            let Value::Obj(f) = f else { panic!() };
            let f = f.borrow();
            let st = |k: &str| f.props[k].to_js_string();
            (
                st("key"),
                Field {
                    kind: st("kind"),
                    label: st("label"),
                    value: f.props["value"].clone(),
                    min: f.props["min"].clone(),
                    max: f.props["max"].clone(),
                    options: array(&f.props["options"])
                        .iter()
                        .map(|o| o.to_js_string())
                        .collect(),
                    error: st("error"),
                    input: st("input"),
                    is_default: f.props["isDefault"].truthy(),
                },
            )
        })
        .collect()
}

fn field(rt: &Runtime, id: usize, key: &str) -> Field {
    fields(rt, id)
        .into_iter()
        .find(|(k, _)| k == key)
        .unwrap_or_else(|| panic!("no field {key}"))
        .1
}

/// The clock's `settings` table as written to `layout.toml`.
fn saved(d: &Path) -> toml::Table {
    let t: toml::Table = std::fs::read_to_string(d.join("layout.toml"))
        .unwrap()
        .parse()
        .unwrap();
    let w = t["monitor"][0]["container"][0]["widget"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["id"].as_str() == Some("clock"))
        .unwrap()
        .clone();
    w.get("settings")
        .and_then(|s| s.as_table().cloned())
        .unwrap_or_default()
}

#[test]
fn fields_follow_the_schema_in_declaration_order() {
    let d = root("schema", LAYOUT);
    let (rt, id) = start(&d);
    assert!(read(&rt, id, "valid").truthy());
    assert_eq!(read(&rt, id, "widgetType").to_js_string(), "clock");
    assert_eq!(read(&rt, id, "title").to_js_string(), "Clock");
    assert_eq!(read(&rt, id, "icon").to_js_string(), "schedule");
    let f = fields(&rt, id);
    let shape: Vec<(&str, &str)> = f
        .iter()
        .map(|(k, f)| (k.as_str(), f.kind.as_str()))
        .collect();
    assert_eq!(
        shape,
        [
            ("showDate", "bool"),
            ("columns", "int"),
            ("opacity", "real"),
            ("format", "str"),
            ("accent", "color"),
            ("style", "choice"),
        ]
    );
    assert_eq!(f[0].1.label, "Show date");
    assert_val!(f[1].1.min, Value::Num(1.0));
    assert_val!(f[1].1.max, Value::Num(10.0));
    assert_val!(f[3].1.min, Value::Null);
    assert_eq!(f[5].1.options, ["digital", "analog", "words"]);
    // The stored override wins over the default; only it counts as changed.
    assert_eq!(f[5].1.value.to_js_string(), "analog");
    assert!(!f[5].1.is_default);
    assert!(
        f.iter()
            .take(5)
            .all(|(_, f)| f.is_default && f.error.is_empty())
    );
    assert!(!read(&rt, id, "canUndo").truthy());
}

#[test]
fn bool_setting_writes_layout_and_undoes() {
    let d = root("bool", LAYOUT);
    let (mut rt, id) = start(&d);
    assert_val!(field(&rt, id, "showDate").value, Value::Bool(true));
    assert!(set(&mut rt, id, "showDate", Value::Bool(false)));
    assert_val!(field(&rt, id, "showDate").value, Value::Bool(false));
    assert_eq!(
        saved(&d).get("showDate"),
        Some(&toml::Value::Boolean(false))
    );
    assert!(read(&rt, id, "canUndo").truthy());
    // Re-entering the stored value is no edit and no undo step.
    assert!(!set(&mut rt, id, "showDate", Value::Bool(false)));
    assert!(call(&mut rt, id, "back", vec![]));
    assert_val!(field(&rt, id, "showDate").value, Value::Bool(true));
    assert_eq!(saved(&d).get("showDate"), None);
    assert!(read(&rt, id, "canRedo").truthy());
    assert!(call(&mut rt, id, "forward", vec![]));
    assert_eq!(
        saved(&d).get("showDate"),
        Some(&toml::Value::Boolean(false))
    );
    // A non-boolean is rejected inline.
    assert!(!set(&mut rt, id, "showDate", Value::str("yes")));
    assert_eq!(field(&rt, id, "showDate").error, "Must be on or off");
}

#[test]
fn int_setting_checks_bounds_and_type() {
    let d = root("int", LAYOUT);
    let (mut rt, id) = start(&d);
    assert!(set(&mut rt, id, "columns", Value::Num(7.0)));
    assert_eq!(saved(&d).get("columns"), Some(&toml::Value::Integer(7)));
    assert!(!set(&mut rt, id, "columns", Value::Num(11.0)));
    let f = field(&rt, id, "columns");
    assert_eq!(f.error, "Must be between 1 and 10");
    assert_eq!(f.input, "11");
    assert_val!(f.value, Value::Num(7.0), "the last accepted value stays");
    assert_eq!(saved(&d).get("columns"), Some(&toml::Value::Integer(7)));
    assert!(!set(&mut rt, id, "columns", Value::Num(2.5)));
    assert_eq!(field(&rt, id, "columns").error, "Must be a whole number");
    // Text from a number field is parsed; an accepted value clears the error.
    assert!(set(&mut rt, id, "columns", Value::str(" 3 ")));
    assert_eq!(field(&rt, id, "columns").error, "");
    assert_eq!(saved(&d).get("columns"), Some(&toml::Value::Integer(3)));
    // Two accepted edits are two undo steps.
    assert!(call(&mut rt, id, "back", vec![]));
    assert_eq!(saved(&d).get("columns"), Some(&toml::Value::Integer(7)));
}

#[test]
fn real_setting_checks_bounds_and_parses_text() {
    let d = root("real", LAYOUT);
    let (mut rt, id) = start(&d);
    assert!(set(&mut rt, id, "opacity", Value::Num(0.25)));
    assert_eq!(saved(&d).get("opacity"), Some(&toml::Value::Float(0.25)));
    assert!(!set(&mut rt, id, "opacity", Value::Num(1.5)));
    assert_eq!(field(&rt, id, "opacity").error, "Must be between 0 and 1");
    assert!(!set(&mut rt, id, "opacity", Value::str("half")));
    let f = field(&rt, id, "opacity");
    assert_eq!(f.error, "Must be a number");
    assert_eq!(f.input, "half");
    assert_eq!(saved(&d).get("opacity"), Some(&toml::Value::Float(0.25)));
    assert!(set(&mut rt, id, "opacity", Value::str("0.5")));
    assert_val!(field(&rt, id, "opacity").value, Value::Num(0.5));
}

#[test]
fn string_setting_writes_layout_and_undoes() {
    let d = root("str", LAYOUT);
    let (mut rt, id) = start(&d);
    assert!(set(&mut rt, id, "format", Value::str("hh:mm:ss")));
    assert_eq!(
        saved(&d).get("format").and_then(|v| v.as_str()),
        Some("hh:mm:ss")
    );
    assert_eq!(field(&rt, id, "format").value.to_js_string(), "hh:mm:ss");
    assert!(call(&mut rt, id, "back", vec![]));
    assert_eq!(saved(&d).get("format"), None);
    assert_eq!(field(&rt, id, "format").value.to_js_string(), "hh:mm");
}

#[test]
fn color_setting_rejects_non_colors_inline() {
    let d = root("color", LAYOUT);
    let (mut rt, id) = start(&d);
    assert!(!set(&mut rt, id, "accent", Value::str("blurple")));
    let f = field(&rt, id, "accent");
    assert_eq!(f.error, "\"blurple\" is not a color");
    assert_eq!(f.input, "blurple");
    assert_eq!(f.value.to_js_string(), "#6750a4");
    assert_eq!(saved(&d).get("accent"), None);
    assert!(
        !read(&rt, id, "canUndo").truthy(),
        "a rejected value is no undo step"
    );
    for c in ["#ff0000", "teal", "#80ff0000"] {
        assert!(set(&mut rt, id, "accent", Value::str(c)), "{c}");
        assert_eq!(saved(&d).get("accent").and_then(|v| v.as_str()), Some(c));
        assert_eq!(field(&rt, id, "accent").error, "");
    }
}

#[test]
fn choice_setting_accepts_only_declared_options() {
    let d = root("choice", LAYOUT);
    let (mut rt, id) = start(&d);
    assert!(set(&mut rt, id, "style", Value::str("words")));
    assert_eq!(
        saved(&d).get("style").and_then(|v| v.as_str()),
        Some("words")
    );
    assert!(!set(&mut rt, id, "style", Value::str("binary")));
    assert_eq!(
        field(&rt, id, "style").error,
        "\"binary\" is not one of the options"
    );
    assert_eq!(
        saved(&d).get("style").and_then(|v| v.as_str()),
        Some("words")
    );
}

#[test]
fn reset_drops_the_override_as_one_undo_step() {
    let d = root("reset", LAYOUT);
    let (mut rt, id) = start(&d);
    assert!(call(&mut rt, id, "reset", vec![Value::str("style")]));
    let f = field(&rt, id, "style");
    assert!(f.is_default);
    assert_eq!(f.value.to_js_string(), "digital");
    assert_eq!(saved(&d).get("style"), None);
    assert!(
        !call(&mut rt, id, "reset", vec![Value::str("style")]),
        "nothing left to reset"
    );
    assert!(call(&mut rt, id, "back", vec![]));
    assert_eq!(
        saved(&d).get("style").and_then(|v| v.as_str()),
        Some("analog")
    );
}

#[test]
fn invalid_saved_values_show_inline_and_fall_back() {
    let layout = LAYOUT.replace(
        "settings = { style = \"analog\" }",
        "settings = { columns = 40, accent = \"nope\" }",
    );
    let d = root("stored", &layout);
    let (rt, id) = start(&d);
    let f = field(&rt, id, "columns");
    assert_val!(f.value, Value::Num(2.0));
    assert_eq!(f.error, "Saved value ignored: Must be between 1 and 10");
    assert_eq!(
        field(&rt, id, "accent").error,
        "Saved value ignored: \"nope\" is not a color"
    );
}

#[test]
fn switching_widgets_clears_rejected_input() {
    let d = root("switch", LAYOUT);
    let (mut rt, id) = start(&d);
    assert!(!set(&mut rt, id, "columns", Value::Num(0.0)));
    assert!(!field(&rt, id, "columns").error.is_empty());
    rt.world
        .borrow_mut()
        .write(id, "widgetId", Value::str("plain"));
    rt.settle();
    assert!(read(&rt, id, "valid").truthy());
    assert_eq!(read(&rt, id, "title").to_js_string(), "Plain");
    assert!(fields(&rt, id).is_empty());
    rt.world
        .borrow_mut()
        .write(id, "widgetId", Value::str("clock"));
    rt.settle();
    assert_eq!(field(&rt, id, "columns").error, "");
    rt.world
        .borrow_mut()
        .write(id, "widgetId", Value::str("gone"));
    rt.settle();
    assert!(!read(&rt, id, "valid").truthy());
    assert!(!set(&mut rt, id, "columns", Value::Num(3.0)));
}

#[test]
fn edits_survive_a_restart() {
    let d = root("restart", LAYOUT);
    {
        let (mut rt, id) = start(&d);
        assert!(set(&mut rt, id, "columns", Value::Num(4.0)));
        assert!(set(&mut rt, id, "accent", Value::str("#00ff00")));
    }
    let (rt, id) = start(&d);
    assert_val!(field(&rt, id, "columns").value, Value::Num(4.0));
    assert_eq!(field(&rt, id, "accent").value.to_js_string(), "#00ff00");
}
