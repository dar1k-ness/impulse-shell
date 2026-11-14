//! Quickshell's Variants (0.3.2, `Variants::setModel` / `updateVariants`): one instance per
//! distinct model value. A value equal to an earlier one of the list is skipped, an instance
//! stays while the model holds a value equal to its own (JS objects compare by content), a
//! model that is no list is ignored, and `instances` (kept instances first, then the new ones)
//! has changed when the write returns; a dropped instance is deleted later. The expected log
//! is what real Quickshell 0.3.2 printed for the audit's `qs/variants.qml`, less its
//! `Quickshell.screens` instance. `v1` and `v5` both read `root.m`: the binding of the object
//! created last runs again first, and each Variants makes its instances inside that write.
//! `instances` is a QQmlListProperty: array-like, and no Array.
#![cfg(feature = "interp")]

use qml::runtime::Runtime;
use qml::value::Value;

fn run(src: &str, steps: usize, after: Option<&str>) -> String {
    let mut rt = Runtime::new(None);
    let root = rt.load_str(src).unwrap();
    rt.settle();
    for i in 0..steps {
        let _ = rt
            .call_method(root, "step", vec![Value::Num(i as f64)])
            .unwrap();
        rt.settle();
        if let Some(f) = after {
            let _ = rt.call_method(root, f, vec![]).unwrap();
        }
    }
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    rt.get(root, "log").to_js_string()
}

/// Fails with the lines around the first one that differs.
fn same(log: &str, want: &str) {
    let (got, want): (Vec<&str>, Vec<&str>) = (log.lines().collect(), want.lines().collect());
    let at = (0..got.len().max(want.len())).find(|&k| got.get(k) != want.get(k));
    if let Some(at) = at {
        let from = at.saturating_sub(3);
        let show = |l: &[&str]| l[from.min(l.len())..(at + 6).min(l.len())].join("\n");
        panic!(
            "line {at} differs\n--- got\n{}\n--- want\n{}",
            show(&got),
            show(&want)
        );
    }
}

/// Finding 30: duplicates, a number as the model, equal fresh objects, `instances` inside the
/// write.
#[test]
fn variants_follow_the_distinct_values_inside_the_write() {
    let log = run(
        r##"import QtQuick
import Quickshell
Scope {
    id: root
    property string log: ""
    function say(...a) { log += a.map(x => String(x)).join(" ") + "\n" }
    function step(i) { say("--", steps[i][0]); steps[i][1]() }
    property var m: ["a", "b"]
    property var om: [{name: "x"}, {name: "y"}]
    Variants { id: v1; model: root.m
        QtObject { required property var modelData; Component.onCompleted: root.say("v1 +", modelData)
            Component.onDestruction: root.say("v1 -", modelData) } }
    Variants { id: v2; model: root.om
        delegate: Component { QtObject { required property var modelData; property string name: modelData.name; Component.onCompleted: root.say("v2 +", name)
            Component.onDestruction: root.say("v2 -", name) } } }
    Variants { id: v4; model: 3
        QtObject { required property var modelData; Component.onCompleted: root.say("v4 +", modelData) } }
    Variants { id: v5; model: root.m
        QtObject { property var modelData; property int index: -5; Component.onCompleted: root.say("v5 +", modelData, index) } }
    property var steps: [
        ["instances", () => root.say(v1.instances.length, v1.instances[0].modelData, v2.instances.length, v4.instances ? v4.instances.length : "none", typeof v1.instances, Array.isArray(v1.instances))],
        ["append", () => root.m = ["a", "b", "c"]],
        ["remove first", () => root.m = ["b", "c"]],
        ["reorder", () => { root.m = ["c", "b"]; root.say(v1.instances.map(i => i.modelData).join(",")) }],
        ["dups", () => { root.m = ["c", "c", "b"]; root.say(v1.instances.length) }],
        ["objects: equal content new objects", () => root.om = [{name: "x"}, {name: "y"}]],
        ["objects: same refs + one", () => { var o = root.om.slice(); o.push({name: "z"}); root.om = o }],
        ["empty", () => { root.m = []; root.say(v1.instances.length) }],
        ["undefined", () => { root.m = undefined; root.say(v1.instances.length) }]
    ]
}"##,
        9,
        None,
    );
    same(
        &log,
        "\
         v5 + a -5\n\
         v5 + b -5\n\
         v2 + x\n\
         v2 + y\n\
         v1 + a\n\
         v1 + b\n\
         -- instances\n\
         2 a 2 0 object false\n\
         -- append\n\
         v5 + c -5\n\
         v1 + c\n\
         -- remove first\n\
         v1 - a\n\
         -- reorder\n\
         b,c\n\
         -- dups\n\
         2\n\
         -- objects: equal content new objects\n\
         -- objects: same refs + one\n\
         v2 + z\n\
         -- empty\n\
         0\n\
         v1 - b\n\
         v1 - c\n\
         -- undefined\n\
         0\n",
    );
}
