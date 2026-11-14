//! Every script of one engine runs against one set of intrinsics: a `.pragma library` shares
//! the built-in constructors and prototypes of the QML engine importing it (and of every other
//! library), while its own declarations stay its own. Expected values are Qt 6.12's (PySide6).
#![cfg(feature = "interp")]

use qml::runtime::Runtime;

const A: &str = r#".pragma library
var libvar = 7;
String.prototype.shout = function () { return this + "!"; };
Array.prototype.second = function () { return this[1]; };
function arr() { return [1, 2]; }
function obj() { return {}; }
function isArr(x) { return x instanceof Array; }
function isObj(x) { return x instanceof Object; }
function err() { try { null.x; } catch (e) { return e; } }
function date() { return new Date(0); }
function fnScope() { return Function("return typeof libvar")(); }
function sameArray(c) { return c === Array; }
function protoSame(p) { return p === Object.prototype; }
function seesQmlExt() { return typeof (5).qmlExt; }
"#;

const B: &str = r#".pragma library
function useSecond() { return [3, 4].second(); }
function shoutB() { return "b".shout(); }
function libvarSeen() { return typeof libvar; }
"#;

const MAIN: &str = r#"import QtQuick
import "a.js" as A
import "b.js" as B
Item {
    property string log: ""
    function run() {
        Number.prototype.qmlExt = function () { return 1; };
        log += "shout " + typeof "a".shout + " " + "a".shout() + "\n";
        log += "second " + [5, 6].second() + " " + B.useSecond() + " " + B.shoutB() + "\n";
        log += "instanceof " + (A.arr() instanceof Array) + " " + (A.obj() instanceof Object)
            + " " + A.isArr([]) + " " + A.isObj({}) + "\n";
        log += "err " + (A.err() instanceof TypeError) + " " + (A.err() instanceof Error) + "\n";
        log += "date " + (A.date() instanceof Date) + "\n";
        log += "fnScope " + A.fnScope() + "\n";
        log += "same " + A.sameArray(Array) + " " + A.protoSame(Object.prototype) + "\n";
        log += "qmlExt " + A.seesQmlExt() + "\n";
        log += "libvar " + B.libvarSeen() + " " + typeof libvar + " " + A.libvar;
    }
}
"#;

#[test]
fn pragma_libraries_share_the_engine_intrinsics() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.js"), A).unwrap();
    std::fs::write(dir.path().join("b.js"), B).unwrap();
    let main = dir.path().join("Main.qml");
    std::fs::write(&main, MAIN).unwrap();

    let mut rt = Runtime::new(None);
    let root = rt.load_file(&main).unwrap();
    assert!(
        rt.world.borrow().errors.is_empty(),
        "{:?}",
        rt.world.borrow().errors
    );
    rt.call_method(root, "run", vec![]).unwrap().unwrap();
    assert_eq!(
        rt.get(root, "log").to_js_string(),
        "shout function a!\n\
         second 6 4 b!\n\
         instanceof true true true true\n\
         err true true\n\
         date true\n\
         fnScope undefined\n\
         same true true\n\
         qmlExt function\n\
         libvar undefined undefined 7"
    );
}
