//! V4 enumerates a QObject's properties, signals and methods: `Object.keys`, `for in` and `Object.assign`
//! copy them (the waffle start menu copies each LauncherSearchResult with
//! `createObject(null, Object.assign({}, entry))`).
#![cfg(feature = "interp")]

use qml::runtime::Runtime;

#[test]
fn qobject_properties_are_enumerable() {
    let mut rt = Runtime::new(None);
    let r = rt
        .load_str(
            "import QtQuick\n\
             Item {\n\
             QtObject { id: src; property string name: \"Foot\"; property int type: 2; property var execute: () => 7; signal hit() }\n\
             Component { id: comp; QtObject { property string name; property int type; property var execute; property string category } }\n\
             function keys() { return Object.keys(src).join(',') }\n\
             function forIn() { let a = []; for (const k in src) a.push(k); return a.join(',') }\n\
             function assigned() { const c = Object.assign({}, src); return c.name + ':' + c.type + ':' + c.execute() }\n\
             function made() { const o = comp.createObject(null, Object.assign({}, src)); o.category = 'Best match'; return o.name + ':' + o.type + ':' + o.execute() + ':' + o.category + ':' + src.name }\n\
             }",
        )
        .unwrap();
    rt.settle();
    let call = |rt: &mut Runtime, f: &str| {
        rt.call_method(r, f, vec![])
            .unwrap()
            .unwrap()
            .to_js_string()
    };
    // Qt 6.12: properties, then signals and methods, each in declaration order.
    let all =
        "objectName,name,type,execute,objectNameChanged,nameChanged,typeChanged,executeChanged,hit";
    assert_eq!(call(&mut rt, "keys"), all);
    assert_eq!(call(&mut rt, "forIn"), all);
    assert_eq!(call(&mut rt, "assigned"), "Foot:2:7");
    assert_eq!(call(&mut rt, "made"), "Foot:2:7:Best match:Foot");
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
}

#[test]
fn for_in_on_a_qobject_walks_object_prototype() {
    let mut rt = Runtime::new(None);
    let r = rt
        .load_str(
            "import QtQuick\n\
             Item {\n\
             QtObject { id: src; property string name: \"Foot\"; signal hit() }\n\
             function forIn() { Object.prototype.op = 1; Object.prototype.name = 2; Object.prototype.hit = 3; \
             let a = []; for (const k in src) a.push(k); delete Object.prototype.op; delete Object.prototype.name; \
             delete Object.prototype.hit; return a.join(',') }\n\
             }",
        )
        .unwrap();
    rt.settle();
    let got = rt
        .call_method(r, "forIn", vec![])
        .unwrap()
        .unwrap()
        .to_js_string();
    assert_eq!(got, "objectName,name,objectNameChanged,nameChanged,hit,op");
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
}
