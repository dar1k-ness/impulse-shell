//! V4's optional chains: every call on the chain's spine is optional (Qt 6.12 oracle results).

use qml::runtime::Runtime;
use qml::value::Value;

const QML: &str = r#"import QtQuick
Item {
    property var o: ({ temp: 0, s: "ab" })
    property string weather: o?.temp.substring(0, o?.temp.length - 1) ?? "--°"
    property var deep: o?.temp.nope().x
    property var before: o.nope()?.x
    property var bare: o?.nope()
    property string real: o?.s.substring(0, 1)
    property var skipped: o?.s.nope(1)
    property var plainThrows: o.nope()
    property var nonFunctionThrows: o?.s.length()
    property var nullReadThrows: o?.n.x
    property var parenThrows: (o?.temp).substring(0, 1)
}
"#;

#[test]
fn calls_on_an_optional_chain_short_circuit_like_v4() {
    let d = std::env::temp_dir().join(format!("ri-optchain-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("main.qml"), QML).unwrap();
    let mut rt = Runtime::new(Some(d.clone()));
    let id = rt.load_file(&d.join("main.qml")).unwrap();
    rt.settle();
    let r = |k: &str| rt.world.borrow_mut().read(id, k);
    assert_eq!(
        r("weather").map(|v| v.to_js_string()),
        Some("--°".to_string())
    );
    assert!(matches!(r("deep"), Some(Value::Undefined)));
    assert!(matches!(r("before"), Some(Value::Undefined)));
    assert!(matches!(r("bare"), Some(Value::Undefined)));
    assert_eq!(r("real").map(|v| v.to_js_string()), Some("a".to_string()));
    assert!(matches!(r("skipped"), Some(Value::Undefined)));
    let errs = rt.errors().join("\n");
    for k in [
        "plainThrows",
        "nonFunctionThrows",
        "nullReadThrows",
        "parenThrows",
    ] {
        assert!(errs.contains(k), "{k} should throw: {errs}");
    }
    assert_eq!(errs.lines().count(), 4, "{errs}");
}

const ARGS_QML: &str = r#"import QtQuick
Item {
    property var log: []
    function g(x) { log.push(x); return x }
    property var a: ({ b: {}, s: "abc", f: function (x) { return x + 1 } })
    property var nil: null
    property var calls: [a?.f(g("f")), a?.nope(g("nope")), a?.b.c(g("b.c")), nil?.f(g("z.f")),
        nil?.[g("z[]")], a?.[g("nopeKey")](g("nopeKeyArg")), a?.s.slice(g(1)), a?.s.nope(g("s.nope"))]
}
"#;

/// ECMA-262 OptionalChain: a short-circuited call skips its arguments and computed keys.
#[test]
fn short_circuit_skips_arguments() {
    let d = std::env::temp_dir().join(format!("ri-optargs-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("main.qml"), ARGS_QML).unwrap();
    let mut rt = Runtime::new(Some(d.clone()));
    let id = rt.load_file(&d.join("main.qml")).unwrap();
    rt.settle();
    let mut w = rt.world.borrow_mut();
    let calls = w.read(id, "calls").unwrap();
    let log = w.read(id, "log").unwrap();
    drop(w);
    assert_eq!(calls.to_js_string(), "f1,,,,,,bc,");
    assert_eq!(log.to_js_string(), "f,nopeKey,1");
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
}

const NATIVE_QML: &str = r#"import QtQuick
import Rustshell
PanelWindow {
    implicitWidth: 400
    implicitHeight: 40
    anchors { top: true; left: true }
    property string attached: String(t.QsWindow?.mapFromItem(t, 5, 0)?.x)
    property string attachedArgs: { var n = 0; var p = t.QsWindow?.mapFromItem(t, n++, 0); return p.x + ":" + n }
    property string toItem: String(t?.mapToItem(null, 1, 2)?.x)
    property string fromItem: String(t?.mapFromItem(null, 1, 2)?.x)
    property string str: String(t?.toString())
    property string missing: { var n = 0; var r = t?.noSuchMethod(n++); return String(r) + ":" + n }
    Item { id: t; x: 100; y: 5; width: 50; height: 20 }
}
"#;

/// Native methods (attached objects, QQuickItem, QObject::toString) are callees an optional
/// chain resolves, not missing properties that end it.
#[test]
fn native_methods_on_an_optional_chain() {
    let d = std::env::temp_dir().join(format!("ri-optnative-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("main.qml"), NATIVE_QML).unwrap();
    let mut rt = Runtime::new(Some(d.clone()));
    let id = rt.load_file(&d.join("main.qml")).unwrap();
    rt.settle();
    let r = |k: &str| {
        rt.world
            .borrow_mut()
            .read(id, k)
            .map(|v| v.to_js_string())
            .unwrap_or_default()
    };
    assert_eq!(r("attached"), "105");
    assert_eq!(r("attachedArgs"), "100:1");
    assert_eq!(r("toItem"), "101");
    assert_eq!(r("fromItem"), "-99");
    assert!(r("str").starts_with("QQuickItem(0x"), "{}", r("str"));
    assert_eq!(r("missing"), "undefined:0");
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
}
