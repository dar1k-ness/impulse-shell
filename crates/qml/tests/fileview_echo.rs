//! A FileView's own write must not come back as `fileChanged`: the original's Config.qml reloads on
//! fileChanged, so the echo of a debounced write reverted text typed since that write
//! (Settings → Services → AI system prompt kept `XY` of ` XYZ`). Quickshell has this flaw;
//! the native port reports only changes made by others.
mod event_loop;
use event_loop::Loop;
use qml::runtime::Runtime;
use qml::value::Value;
use std::path::Path;

const QML: &str = r#"import QtQuick
import Rustshell
import Rustshell.Io
Item {
    id: root
    property alias s: ad.s
    property int changed: 0
    FileView {
        id: fv
        path: "@PATH@"
        watchChanges: true
        onFileChanged: { root.changed++; reloadTimer.restart() }
        onAdapterUpdated: writeTimer.restart()
        JsonAdapter { id: ad; property string s: "" }
    }
    Timer { id: reloadTimer; interval: 50; onTriggered: fv.reload() }
    Timer { id: writeTimer; interval: 50; onTriggered: fv.writeAdapter() }
    TextInput {
        id: field
        text: ad.s
        onTextChanged: Qt.callLater(() => { ad.s = text })
    }
}
"#;

fn type_into_config(gap_ms: u64) -> (String, String, String) {
    let base = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("fileview-echo-{}-{gap_ms}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let path = base.join("config.json");
    std::fs::write(&path, "{\"s\": \"abc\"}").unwrap();
    let cfg = base.join("cfg");
    std::fs::create_dir_all(&cfg).unwrap();
    std::fs::write(
        cfg.join("shell.qml"),
        QML.replace("@PATH@", path.to_str().unwrap()),
    )
    .unwrap();
    let mut rt = Runtime::new(Some(cfg.clone()));
    let root = rt.load_file(&cfg.join("shell.qml")).unwrap();
    rt.settle();
    let mut lp = Loop::new(&mut rt);
    lp.advance(&mut rt, 300.0);
    let field = rt.id("field").unwrap();
    let mut text = rt.get(field, "text").to_js_string();
    assert_eq!(text, "abc");
    for c in " XYZ wxyz".chars() {
        text.push(c);
        rt.set(field, "text", Value::str(&text));
        lp.advance(&mut rt, gap_ms as f64);
    }
    lp.advance(&mut rt, 600.0);
    let disk = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_dir_all(&base);
    (
        rt.get(field, "text").to_js_string(),
        rt.get(root, "s").to_js_string(),
        disk,
    )
}

#[test]
fn own_write_is_no_file_change() {
    for gap in [5, 30, 55, 70, 90] {
        let (field, s, disk) = type_into_config(gap);
        assert_eq!(field, "abc XYZ wxyz", "gap {gap}ms");
        assert_eq!(s, "abc XYZ wxyz", "gap {gap}ms");
        let v: serde_json::Value = serde_json::from_str(&disk).unwrap();
        assert_eq!(v["s"], "abc XYZ wxyz", "gap {gap}ms: {disk}");
    }
}

/// Another process putting back exactly what this view last wrote is a change once the view
/// loaded something else since: the settings window turning an option back (bar on top again,
/// transparency off) after the shell itself had saved config.json was taken for the shell's
/// own echo and ignored until a restart.
#[test]
fn others_restoring_own_last_write_is_a_file_change() {
    let base = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("fileview-echo-restore-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let path = base.join("config.json");
    std::fs::write(&path, "{\"s\": \"abc\"}").unwrap();
    let cfg = base.join("cfg");
    std::fs::create_dir_all(&cfg).unwrap();
    std::fs::write(
        cfg.join("shell.qml"),
        QML.replace("@PATH@", path.to_str().unwrap()),
    )
    .unwrap();
    let mut rt = Runtime::new(Some(cfg.clone()));
    let root = rt.load_file(&cfg.join("shell.qml")).unwrap();
    rt.settle();
    let mut lp = Loop::new(&mut rt);
    let is = |want: &'static str| move |rt: &mut Runtime| rt.get(root, "s").to_js_string() == want;
    assert!(lp.until(&mut rt, is("abc")), "the view never loaded");
    rt.set(root, "s", Value::str("top"));
    let wrote = lp.until(&mut rt, |_| {
        std::fs::read_to_string(&path).is_ok_and(|t| t.contains("\"top\""))
    });
    assert!(wrote, "the view never wrote its change");
    let own = std::fs::read_to_string(&path).unwrap();
    assert!(own.contains("\"top\""), "{own}");
    // Another process saves the way Config.qml does (atomic replace).
    let save = |text: &str| {
        let tmp = base.join("config.json.new");
        std::fs::write(&tmp, text).unwrap();
        std::fs::rename(&tmp, &path).unwrap();
    };
    save(&own.replace("\"top\"", "\"bottom\""));
    lp.until(&mut rt, is("bottom"));
    assert_eq!(rt.get(root, "s").to_js_string(), "bottom");
    save(&own);
    lp.until(&mut rt, is("top"));
    let s = rt.get(root, "s").to_js_string();
    let _ = std::fs::remove_dir_all(&base);
    assert_eq!(s, "top");
}

const TWO_WRITERS: &str = r#"import QtQuick
import Rustshell
import Rustshell.Io
Item {
    id: root
    property string all: ad.a + " " + ad.b + " " + ad.c
    function edit() { ad.b = "mine"; ad.c = "mine" }
    FileView {
        id: fv
        path: "@PATH@"
        watchChanges: @WATCH@
        onFileChanged: reloadTimer.restart()
        onAdapterUpdated: writeTimer.restart()
        JsonAdapter { id: ad; property string a: ""; property string b: ""; property string c: "" }
    }
    Timer { id: reloadTimer; interval: 50; onTriggered: fv.reload() }
    Timer { id: writeTimer; interval: 50; onTriggered: fv.writeAdapter() }
}
"#;

/// The view edits `b` and `c`; before its delayed save another process saves `a` and `c`.
/// Returns the adapter's and the file's `a b c` once both have settled.
fn save_after_another_writer(watch: bool) -> (String, String) {
    let base = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "fileview-two-writers-{}-{watch}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let path = base.join("config.json");
    std::fs::write(&path, r#"{"a": "a0", "b": "b0", "c": "c0"}"#).unwrap();
    let cfg = base.join("cfg");
    std::fs::create_dir_all(&cfg).unwrap();
    std::fs::write(
        cfg.join("shell.qml"),
        TWO_WRITERS
            .replace("@PATH@", path.to_str().unwrap())
            .replace("@WATCH@", &watch.to_string()),
    )
    .unwrap();
    let mut rt = Runtime::new(Some(cfg.clone()));
    let root = rt.load_file(&cfg.join("shell.qml")).unwrap();
    rt.settle();
    let mut lp = Loop::new(&mut rt);
    lp.advance(&mut rt, 300.0);
    assert_eq!(rt.get(root, "all").to_js_string(), "a0 b0 c0");
    rt.call_method(root, "edit", vec![]).unwrap().unwrap();
    let tmp = base.join("config.json.new");
    std::fs::write(&tmp, r#"{"a": "other", "b": "b0", "c": "other"}"#).unwrap();
    std::fs::rename(&tmp, &path).unwrap();
    lp.advance(&mut rt, 600.0);
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let _ = std::fs::remove_dir_all(&base);
    let s = |k: &str| v[k].as_str().unwrap().to_string();
    (
        rt.get(root, "all").to_js_string(),
        [s("a"), s("b"), s("c")].join(" "),
    )
}

/// A watching view's save keeps what another process saved since the view last read the
/// file, except where the view itself has an edit: the original's shell and settings window each saved
/// their whole copy of config.json, and the later one took the other's fresh option back.
#[test]
fn save_keeps_what_another_process_just_saved() {
    let want = "other mine mine".to_string();
    assert_eq!(save_after_another_writer(true), (want.clone(), want));
}

/// Without `watchChanges` the view does not follow the file: its save is its own copy, as in
/// Quickshell.
#[test]
fn save_of_a_view_that_does_not_watch_is_its_own_copy() {
    let want = "a0 mine mine".to_string();
    assert_eq!(save_after_another_writer(false), (want.clone(), want));
}
