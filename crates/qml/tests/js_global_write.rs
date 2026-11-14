//! QML code cannot create globals: a write to a name declared nowhere throws
//! `Error: Invalid write to global property "x"` like QV4 (checked on Qt 6.12), instead of
//! creating a global that would then shadow QML names.
#![cfg(feature = "interp")]

use qml::runtime::Runtime;

#[test]
fn undeclared_write_throws_and_creates_no_global() {
    let mut rt = Runtime::new(None);
    let root = rt
        .load_str(
            r#"import QtQuick
Item {
    id: root
    property int known: 1
    property string log: ""
    function run() {
        try { known = 7; log += "known=" + known + " " } catch (e) { log += "bad " }
        try { typoName = 1; log += "created " } catch (e) { log += e.name + ": " + e.message + " " }
        try { for (i = 0; i < 2; i++) {} log += "loop " } catch (e) { log += "loop:" + e.message + " " }
        log += typeof typoName;
    }
}"#,
        )
        .unwrap();
    rt.call_method(root, "run", vec![]).unwrap().unwrap();
    assert_eq!(
        rt.get(root, "log").to_js_string(),
        "known=7 Error: Invalid write to global property \"typoName\" \
         loop:Invalid write to global property \"i\" undefined"
    );
}

/// QML freezes the JS global object (Qt docs, "JavaScript Environment Restrictions"): a write
/// to a builtin global throws QV4's error, whatever the assignment form or strictness, from a
/// QML function and from a `.js` resource alike, and leaves the builtin in place. Declaring a
/// variable of that name only shadows it. Expected strings are Qt 6.12's (PySide6 probe).
#[test]
fn builtin_global_write_throws_and_keeps_the_builtin() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("lib.js"),
        r#".pragma library
var log = "";
try { Math = 1; log += "lib-top:wrote " } catch (e) { log += "lib-top:" + e.message + " " }
function f() {
    try { console = 1; log += "lib-fn:wrote " } catch (e) { log += "lib-fn:" + e.message + " " }
    try { "use strict"; parseInt = 1; log += "lib-strict:wrote " } catch (e) { log += "lib-strict:" + e.name + " " }
    return log + typeof Math + " " + typeof console.log;
}
"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("plain.js"),
        r#"function g() {
    try { JSON = 1; return "plain:wrote" } catch (e) { return "plain:" + e.message + " " + typeof JSON.stringify }
}
"#,
    )
    .unwrap();
    let main = dir.path().join("Main.qml");
    std::fs::write(
        &main,
        r#"import QtQuick
import "lib.js" as L
import "plain.js" as P
Item {
    property string log: ""
    function t(label, fn) {
        try { fn(); log += label + ":ok " } catch (e) { log += label + ":" + e.name + ":" + e.message + " " }
    }
    function run() {
        t("Math", function() { Math = 1 })
        t("JSON", function() { JSON = 1 })
        t("Qt", function() { Qt = 1 })
        t("print", function() { print = 1 })
        t("qsTr", function() { qsTr = 1 })
        t("undefined", function() { undefined = 1 })
        t("compound", function() { Math += 1 })
        t("inc", function() { Math++ })
        t("destr", function() { [Math] = [1] })
        t("forin", function() { for (Math in { a: 1 }) {} })
        t("strict", function() { "use strict"; JSON = 1 })
        t("shadow", function() { var Math = 1; Math = 2; if (Math !== 2) throw new Error("lost") })
        log += "| " + L.f() + " | " + P.g() + " | " + typeof Math.abs + " " + typeof JSON.parse
    }
}
"#,
    )
    .unwrap();
    let mut rt = Runtime::new(None);
    let root = rt.load_file(&main).unwrap();
    rt.call_method(root, "run", vec![]).unwrap().unwrap();
    let w = |n: &str| format!("Error:Invalid write to global property \"{n}\"");
    assert_eq!(
        rt.get(root, "log").to_js_string(),
        format!(
            "Math:{} JSON:{} Qt:{} print:{} qsTr:{} undefined:{} compound:{} inc:{} destr:{} \
             forin:{} strict:{} shadow:ok \
             | lib-top:Invalid write to global property \"Math\" \
             lib-fn:Invalid write to global property \"console\" lib-strict:Error object function \
             | plain:Invalid write to global property \"JSON\" function | function function",
            w("Math"),
            w("JSON"),
            w("Qt"),
            w("print"),
            w("qsTr"),
            w("undefined"),
            w("Math"),
            w("Math"),
            w("Math"),
            w("Math"),
            w("JSON"),
        )
    );
}
