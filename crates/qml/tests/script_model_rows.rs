//! Quickshell's ScriptModel (0.3.2, `ScriptModel::updateValuesUnique`): a `values` write turns
//! into row inserts, removes and moves; a row stays while its value compares equal by
//! `objectProp`, or by structure (`ObjectComparison.Structure`, the default) or identity. A kept
//! row in place takes the new value, a moved one keeps its old one. The views over the model
//! follow inside the write, and `valuesChanged` comes after them. The expected logs are what
//! real Quickshell 0.3.2 printed for the audit's example files (`qs/sm3.qml`, `qs/sm5.qml`)
//! and for `sm6.qml`, less the lines of a delegate's own creation.
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

/// Finding 22: `objectProp` keys the rows, so fresh objects with the same key keep their
/// delegates, which read the new object; without it QObjects compare by identity.
#[test]
fn object_prop_keeps_delegates_of_fresh_objects() {
    let log = run(
        r##"import QtQuick
import Quickshell
Scope {
    id: root
    property string log: ""
    function say(...a) { log += a.map(x => String(x)).join(" ") + "\n" }
    function step(i) { say("--", steps[i][0]); steps[i][1]() }
    property int gen: 0
    property var ids: ["firefox", "kitty"]
    property list<var> apps: {
        var out = []
        for (var i = 0; i < ids.length; i++) out.push(entryComp.createObject(null, { appId: ids[i], gen: root.gen }))
        return out
    }
    Component { id: entryComp; QtObject { property string appId; property int gen } }
    property int serial: 0
    Item {
        Repeater {
            model: ScriptModel { id: keyed; objectProp: "appId"; values: root.apps }
            delegate: Item { required property var modelData; required property int index; property int born: -1
                Component.onCompleted: { born = root.serial++; root.say("keyed +", modelData.appId, modelData.gen, index, "born", born) }
                Component.onDestruction: root.say("keyed -", modelData ? modelData.appId : modelData, "born", born)
                onModelDataChanged: if (born >= 0) root.say("keyed data", modelData.appId, modelData.gen, "born", born) }
        }
        Repeater {
            model: ScriptModel { id: plain; values: root.apps }
            delegate: Item { required property var modelData; required property int index; property int born: -1
                Component.onCompleted: { born = root.serial++; root.say("plain +", modelData.appId, modelData.gen, index, "born", born) }
                Component.onDestruction: root.say("plain -", modelData ? modelData.appId : modelData, "born", born)
                onModelDataChanged: if (born >= 0) root.say("plain data", modelData.appId, modelData.gen, "born", born) }
        }
    }
    property var steps: [
        ["recompute same ids", () => root.gen = 1],
        ["add app", () => root.ids = ["firefox", "kitty", "mpv"]],
        ["remove first", () => root.ids = ["kitty", "mpv"]],
        ["reorder", () => root.ids = ["mpv", "kitty"]]
    ]
    function values() { say("values", keyed.values.map(v => v.appId + v.gen).join(",")) }
}"##,
        4,
        Some("values"),
    );
    same(
        &log,
        "\
         plain + firefox 0 0 born 0\n\
         plain + kitty 0 1 born 1\n\
         keyed + firefox 0 0 born 2\n\
         keyed + kitty 0 1 born 3\n\
         -- recompute same ids\n\
         plain + firefox 1 0 born 4\n\
         plain + kitty 1 1 born 5\n\
         plain - firefox born 0\n\
         plain - kitty born 1\n\
         keyed data firefox 1 born 2\n\
         keyed data kitty 1 born 3\n\
         values firefox1,kitty1\n\
         -- add app\n\
         plain + firefox 1 0 born 6\n\
         plain + kitty 1 1 born 7\n\
         plain + mpv 1 2 born 8\n\
         plain - firefox born 4\n\
         plain - kitty born 5\n\
         keyed data firefox 1 born 2\n\
         keyed data kitty 1 born 3\n\
         keyed + mpv 1 2 born 9\n\
         values firefox1,kitty1,mpv1\n\
         -- remove first\n\
         plain + kitty 1 0 born 10\n\
         plain + mpv 1 1 born 11\n\
         plain - firefox born 6\n\
         plain - kitty born 7\n\
         plain - mpv born 8\n\
         keyed - firefox born 2\n\
         keyed data kitty 1 born 3\n\
         keyed data mpv 1 born 9\n\
         values kitty1,mpv1\n\
         -- reorder\n\
         plain + mpv 1 0 born 12\n\
         plain + kitty 1 1 born 13\n\
         plain - kitty born 10\n\
         plain - mpv born 11\n\
         keyed data kitty 1 born 3\n\
         values mpv1,kitty1\n",
    );
}

/// Findings 26 and 37: an Instantiator over a ScriptModel creates and releases only the rows
/// that changed (a removed row ends the pass), and `valuesChanged` follows them.
#[test]
fn instantiator_follows_the_rows_and_values_changed_comes_last() {
    let log = run(
        r##"import QtQuick
import Quickshell
Scope {
    id: root
    property string log: ""
    function say(...a) { log += a.map(x => String(x)).join(" ") + "\n" }
    function step(i) { say("--", steps[i][0]); steps[i][1]() }
    property var arr: ["a", "b", "c"]
    property var objs: [{id: 1, n: "x"}, {id: 2, n: "y"}]
    ScriptModel { id: sm; values: root.arr; onValuesChanged: root.say("valuesChanged", JSON.stringify(values)) }
    ScriptModel { id: so; values: root.objs; objectProp: "id" }
    Instantiator { model: sm; delegate: QtObject { required property var modelData; required property int index; property bool up: false
            Component.onCompleted: { up = true; root.say("ins +", modelData, index) }
            Component.onDestruction: root.say("ins -", modelData)
            onIndexChanged: if (up) root.say("ins idx", modelData, index)
            onModelDataChanged: if (up) root.say("ins data", modelData, index) } }
    property var steps: [
        ["same content new array", () => root.arr = ["a", "b", "c"]],
        ["append", () => root.arr = ["a", "b", "c", "d"]],
        ["remove middle", () => root.arr = ["a", "c", "d"]],
        ["move", () => root.arr = ["d", "a", "c"]],
        ["replace one", () => root.arr = ["d", "X", "c"]],
        ["dups", () => root.arr = ["d", "d", "c", "d"]],
        ["clear", () => root.arr = []],
        ["numbers", () => root.arr = [1, 2, 3]],
        ["direct write", () => { sm.values = [3, 2, 1] }],
        ["objs same ids changed field", () => root.objs = [{id: 1, n: "x2"}, {id: 2, n: "y"}]],
        ["objs reorder + new", () => root.objs = [{id: 2, n: "y"}, {id: 3, n: "z"}, {id: 1, n: "x2"}]],
        ["objs same refs", () => { var o = root.objs; root.objs = [o[1], o[0], o[2]] }],
        ["undefined", () => root.arr = undefined]
    ]
    function values() { say("values", JSON.stringify(sm.values), sm.values ? sm.values.length : "no-length", JSON.stringify(so.values)) }
}"##,
        13,
        Some("values"),
    );
    same(
        &log,
        "\
         valuesChanged [\"a\",\"b\",\"c\"]\n\
         ins + a 0\n\
         ins + b 1\n\
         ins + c 2\n\
         -- same content new array\n\
         values [\"a\",\"b\",\"c\"] 3 [{\"id\":1,\"n\":\"x\"},{\"id\":2,\"n\":\"y\"}]\n\
         -- append\n\
         ins + d 3\n\
         valuesChanged [\"a\",\"b\",\"c\",\"d\"]\n\
         values [\"a\",\"b\",\"c\",\"d\"] 4 [{\"id\":1,\"n\":\"x\"},{\"id\":2,\"n\":\"y\"}]\n\
         -- remove middle\n\
         ins idx b -1\n\
         ins idx c 1\n\
         ins idx d 2\n\
         ins - b\n\
         valuesChanged [\"a\",\"c\",\"d\"]\n\
         values [\"a\",\"c\",\"d\"] 3 [{\"id\":1,\"n\":\"x\"},{\"id\":2,\"n\":\"y\"}]\n\
         -- move\n\
         ins idx a 1\n\
         ins idx c 2\n\
         ins idx d 0\n\
         valuesChanged [\"d\",\"a\",\"c\"]\n\
         values [\"d\",\"a\",\"c\"] 3 [{\"id\":1,\"n\":\"x\"},{\"id\":2,\"n\":\"y\"}]\n\
         -- replace one\n\
         ins idx a 2\n\
         ins idx c 3\n\
         ins + X 1\n\
         ins idx a -1\n\
         ins idx c 2\n\
         ins - a\n\
         valuesChanged [\"d\",\"X\",\"c\"]\n\
         values [\"d\",\"X\",\"c\"] 3 [{\"id\":1,\"n\":\"x\"},{\"id\":2,\"n\":\"y\"}]\n\
         -- dups\n\
         ins idx X 2\n\
         ins idx c 3\n\
         ins + d 1\n\
         ins idx X -1\n\
         ins idx c 2\n\
         ins - X\n\
         ins + d 3\n\
         valuesChanged [\"d\",\"d\",\"c\",\"d\"]\n\
         values [\"d\",\"d\",\"c\",\"d\"] 4 [{\"id\":1,\"n\":\"x\"},{\"id\":2,\"n\":\"y\"}]\n\
         -- clear\n\
         ins idx d -1\n\
         ins idx d -1\n\
         ins idx c -1\n\
         ins idx d -1\n\
         ins - d\n\
         ins - d\n\
         ins - c\n\
         ins - d\n\
         valuesChanged []\n\
         values [] 0 [{\"id\":1,\"n\":\"x\"},{\"id\":2,\"n\":\"y\"}]\n\
         -- numbers\n\
         ins + 1 0\n\
         ins + 2 1\n\
         ins + 3 2\n\
         valuesChanged [1,2,3]\n\
         values [1,2,3] 3 [{\"id\":1,\"n\":\"x\"},{\"id\":2,\"n\":\"y\"}]\n\
         -- direct write\n\
         ins idx 1 1\n\
         ins idx 2 2\n\
         ins idx 3 0\n\
         ins idx 1 2\n\
         ins idx 2 1\n\
         valuesChanged [3,2,1]\n\
         values [3,2,1] 3 [{\"id\":1,\"n\":\"x\"},{\"id\":2,\"n\":\"y\"}]\n\
         -- objs same ids changed field\n\
         values [3,2,1] 3 [{\"id\":1,\"n\":\"x2\"},{\"id\":2,\"n\":\"y\"}]\n\
         -- objs reorder + new\n\
         values [3,2,1] 3 [{\"id\":2,\"n\":\"y\"},{\"id\":3,\"n\":\"z\"},{\"id\":1,\"n\":\"x2\"}]\n\
         -- objs same refs\n\
         values [3,2,1] 3 [{\"id\":3,\"n\":\"z\"},{\"id\":2,\"n\":\"y\"},{\"id\":1,\"n\":\"x2\"}]\n\
         -- undefined\n\
         values [3,2,1] 3 [{\"id\":3,\"n\":\"z\"},{\"id\":2,\"n\":\"y\"},{\"id\":1,\"n\":\"x2\"}]\n",
    );
}

/// Findings 22 and 37: plain JS objects compare by structure (or by identity when asked to),
/// the rows and their delegates have changed when the write returns and when `valuesChanged`
/// runs, and a value that is no list is rejected.
#[test]
fn structural_rows_change_inside_the_write() {
    let log = run(
        r##"import QtQuick
import Quickshell
Scope {
    id: root
    property string log: ""
    function say(...a) { log += a.map(x => String(x)).join(" ") + "\n" }
    function step(i) { say("--", steps[i][0]); steps[i][1]() }
    property var arr: undefined
    property var objs: [{a: 1, b: [1, 2]}, {a: 2}]
    property int born: 0
    ScriptModel { id: su; values: root.arr; onValuesChanged: root.say("su changed", JSON.stringify(values), insU.count) }
    ScriptModel { id: sn; values: root.objs; onValuesChanged: root.say("sn changed", JSON.stringify(values), insN.count) }
    ScriptModel { id: si; values: root.objs; comparisonMode: ObjectComparison.Identity }
    Instantiator { id: insU; model: su; delegate: QtObject { required property var modelData; property int born: -1
            Component.onCompleted: { born = root.born; root.born += 1; root.say("u +", modelData, born) }
            Component.onDestruction: root.say("u -", modelData, born) } }
    Instantiator { id: insN; model: sn; delegate: QtObject { required property var modelData; property int born: -1
            Component.onCompleted: { born = root.born; root.born += 1; root.say("n +", JSON.stringify(modelData), born) }
            Component.onDestruction: root.say("n -", JSON.stringify(modelData), born)
            onModelDataChanged: if (born >= 0) root.say("n data", JSON.stringify(modelData), born) } }
    Instantiator { id: insI; model: si; delegate: QtObject { required property var modelData; property int born: -1
            Component.onCompleted: { born = root.born; root.born += 1; root.say("i +", JSON.stringify(modelData), born) }
            Component.onDestruction: root.say("i -", JSON.stringify(modelData), born)
            onModelDataChanged: if (born >= 0) root.say("i data", JSON.stringify(modelData), born) } }
    property var steps: [
        ["initial", () => root.say(JSON.stringify(su.values), su.values.length, su.objectProp === "", sn.comparisonMode, si.comparisonMode)],
        ["arr list", () => { root.arr = ["a", "b"]; root.say(JSON.stringify(su.values), insU.count) }],
        ["arr undefined", () => { root.arr = undefined; root.say(JSON.stringify(su.values), insU.count) }],
        ["arr null", () => { root.arr = ["c"]; root.arr = null; root.say(JSON.stringify(su.values), insU.count) }],
        ["equal objs", () => root.objs = [{a: 1, b: [1, 2]}, {a: 2}]],
        ["nested differs", () => root.objs = [{a: 1, b: [1, 3]}, {a: 2}]],
        ["key order", () => { root.objs = [{b: [1, 3], a: 1}, {a: 2}]
            root.say(insN.objectAt(0).modelData === root.objs[0], insI.objectAt(0).modelData === root.objs[0], sn.values[0] === root.objs[0], insN.objectAt(1).modelData === root.objs[1]) }],
        ["extra key", () => root.objs = [{b: [1, 3], a: 1, c: 0}, {a: 2}]],
        ["sync write", () => { sn.values = [{a: 5}, {a: 2}]; root.say(insN.count, insN.objectAt(0).modelData.a, insN.objectAt(1).born) }],
        ["dup write", () => { su.values = ["x", "y", "x"]; root.say(JSON.stringify(su.values), insU.count) }],
        ["dup shuffle", () => { su.values = ["y", "x", "x"]; root.say(JSON.stringify(su.values), insU.count) }],
        ["objectProp late", () => { sn.objectProp = "a"; sn.values = [{a: 2, z: 1}, {a: 5, z: 2}]; root.say(JSON.stringify(sn.values)) }]
    ]
}"##,
        12,
        None,
    );
    same(
        &log,
        "\
         sn changed [{\"a\":1,\"b\":[1,2]},{\"a\":2}] 0\n\
         i + {\"a\":1,\"b\":[1,2]} 0\n\
         i + {\"a\":2} 1\n\
         n + {\"a\":1,\"b\":[1,2]} 2\n\
         n + {\"a\":2} 3\n\
         -- initial\n\
         [] 0 true 1 0\n\
         -- arr list\n\
         u + a 4\n\
         u + b 5\n\
         su changed [\"a\",\"b\"] 2\n\
         [\"a\",\"b\"] 2\n\
         -- arr undefined\n\
         [\"a\",\"b\"] 2\n\
         -- arr null\n\
         u + c 6\n\
         u - a 4\n\
         u - b 5\n\
         su changed [\"c\"] 1\n\
         [\"c\"] 1\n\
         -- equal objs\n\
         i + {\"a\":1,\"b\":[1,2]} 7\n\
         i + {\"a\":2} 8\n\
         i - {\"a\":1,\"b\":[1,2]} 0\n\
         i - {\"a\":2} 1\n\
         n data {\"a\":1,\"b\":[1,2]} 2\n\
         n data {\"a\":2} 3\n\
         sn changed [{\"a\":1,\"b\":[1,2]},{\"a\":2}] 2\n\
         -- nested differs\n\
         i + {\"a\":1,\"b\":[1,3]} 9\n\
         i + {\"a\":2} 10\n\
         i - {\"a\":1,\"b\":[1,2]} 7\n\
         i - {\"a\":2} 8\n\
         n + {\"a\":1,\"b\":[1,3]} 11\n\
         n - {\"a\":1,\"b\":[1,2]} 2\n\
         n data {\"a\":2} 3\n\
         sn changed [{\"a\":1,\"b\":[1,3]},{\"a\":2}] 2\n\
         -- key order\n\
         i + {\"b\":[1,3],\"a\":1} 12\n\
         i + {\"a\":2} 13\n\
         i - {\"a\":1,\"b\":[1,3]} 9\n\
         i - {\"a\":2} 10\n\
         n data {\"b\":[1,3],\"a\":1} 11\n\
         n data {\"a\":2} 3\n\
         sn changed [{\"b\":[1,3],\"a\":1},{\"a\":2}] 2\n\
         true true true true\n\
         -- extra key\n\
         i + {\"b\":[1,3],\"a\":1,\"c\":0} 14\n\
         i + {\"a\":2} 15\n\
         i - {\"b\":[1,3],\"a\":1} 12\n\
         i - {\"a\":2} 13\n\
         n + {\"b\":[1,3],\"a\":1,\"c\":0} 16\n\
         n - {\"b\":[1,3],\"a\":1} 11\n\
         n data {\"a\":2} 3\n\
         sn changed [{\"b\":[1,3],\"a\":1,\"c\":0},{\"a\":2}] 2\n\
         -- sync write\n\
         n + {\"a\":5} 17\n\
         n - {\"b\":[1,3],\"a\":1,\"c\":0} 16\n\
         n data {\"a\":2} 3\n\
         sn changed [{\"a\":5},{\"a\":2}] 2\n\
         2 5 3\n\
         -- dup write\n\
         u + x 18\n\
         u + y 19\n\
         u + x 20\n\
         u - c 6\n\
         su changed [\"x\",\"y\",\"x\"] 3\n\
         [\"x\",\"y\",\"x\"] 3\n\
         -- dup shuffle\n\
         su changed [\"y\",\"x\",\"x\"] 3\n\
         [\"y\",\"x\",\"x\"] 3\n\
         -- objectProp late\n\
         n data {\"a\":5,\"z\":2} 17\n\
         sn changed [{\"a\":2},{\"a\":5,\"z\":2}] 2\n\
         [{\"a\":2},{\"a\":5,\"z\":2}]\n",
    );
}

/// A write to a member of a JS object that is an element of a `list<var>` property: Qt holds
/// the element as a QVariantMap, a copy, so the write goes back into the list and a model
/// keyed by `objectProp` gets a row that differs from the one it has, and tells its delegate
/// (dataChanged). The rows of the original's quick toggles are such elements, and the size of a toggle
/// is written that way. The log is Quickshell 0.3.2's on Qt 6.12, from its first step on
/// (a QVariantMap lists its members by name; here they are written in that order).
#[test]
fn member_write_of_a_list_element_changes_its_row() {
    let log = run(
        r##"import QtQuick
import Quickshell
Scope {
    id: root
    property string log: ""
    function say(...a) { log += a.map(x => String(x)).join(" ") + "\n" }
    function step(i) { say("--", steps[i][0]); steps[i][1]() }
    property list<var> src: [{size: 2, type: "a"}, {size: 1, type: "b"}]
    function rowsFor(l) { const r = [[]]; for (let i = 0; i < l.length; i++) r[0].push(l[i]); return r }
    readonly property list<var> rows: rowsFor(src)
    ScriptModel { id: keyed; objectProp: "type"; values: root.rows[0] ?? []; onValuesChanged: root.say("keyed changed", JSON.stringify(values)) }
    ScriptModel { id: direct; objectProp: "type"; values: root.src; onValuesChanged: root.say("direct changed", JSON.stringify(values)) }
    Instantiator {
        model: keyed
        delegate: QtObject {
            required property var modelData
            property string tag: "k"
            property string first
            property int size: modelData.size
            Component.onCompleted: { first = modelData.type; root.say(tag, "+", JSON.stringify(modelData)) }
            Component.onDestruction: root.say(tag, "-", first)
            onModelDataChanged: if (first) root.say(tag, "data", JSON.stringify(modelData), first)
            onSizeChanged: if (first) root.say(tag, "size", size, first)
        }
    }
    Instantiator {
        model: direct
        delegate: QtObject {
            required property var modelData
            property string tag: "d"
            property string first
            property int size: modelData.size
            Component.onCompleted: { first = modelData.type; root.say(tag, "+", JSON.stringify(modelData)) }
            Component.onDestruction: root.say(tag, "-", first)
            onModelDataChanged: if (first) root.say(tag, "data", JSON.stringify(modelData), first)
            onSizeChanged: if (first) root.say(tag, "size", size, first)
        }
    }
    property var steps: [
        ["start", () => root.log = ""],
        ["member", () => { const l = root.src; l[0].size = 3 - l[0].size; root.say(JSON.stringify(root.src)) }],
        ["push", () => { const l = root.src; l.push({size: 1, type: "c"}); root.say(JSON.stringify(root.src)) }],
        ["element in a variable", () => { const e = root.src[1]; e.size = 2; root.say(JSON.stringify(root.src)) }],
        ["member of the last", () => { root.src[2].size = 2; root.say(JSON.stringify(root.src)) }],
        ["two members", () => { const l = root.src; l[0].size = 2; l[2].size = 1; root.say(JSON.stringify(root.src)) }]
    ]
}"##,
        6,
        None,
    );
    same(
        &log,
        "\
         -- member\n\
         d data {\"size\":1,\"type\":\"a\"} a\n\
         d size 1 a\n\
         direct changed [{\"size\":1,\"type\":\"a\"},{\"size\":1,\"type\":\"b\"}]\n\
         k data {\"size\":1,\"type\":\"a\"} a\n\
         k size 1 a\n\
         keyed changed [{\"size\":1,\"type\":\"a\"},{\"size\":1,\"type\":\"b\"}]\n\
         [{\"size\":1,\"type\":\"a\"},{\"size\":1,\"type\":\"b\"}]\n\
         -- push\n\
         d + {\"size\":1,\"type\":\"c\"}\n\
         direct changed [{\"size\":1,\"type\":\"a\"},{\"size\":1,\"type\":\"b\"},{\"size\":1,\"type\":\"c\"}]\n\
         k + {\"size\":1,\"type\":\"c\"}\n\
         keyed changed [{\"size\":1,\"type\":\"a\"},{\"size\":1,\"type\":\"b\"},{\"size\":1,\"type\":\"c\"}]\n\
         [{\"size\":1,\"type\":\"a\"},{\"size\":1,\"type\":\"b\"},{\"size\":1,\"type\":\"c\"}]\n\
         -- element in a variable\n\
         d data {\"size\":2,\"type\":\"b\"} b\n\
         d size 2 b\n\
         direct changed [{\"size\":1,\"type\":\"a\"},{\"size\":2,\"type\":\"b\"},{\"size\":1,\"type\":\"c\"}]\n\
         k data {\"size\":2,\"type\":\"b\"} b\n\
         k size 2 b\n\
         keyed changed [{\"size\":1,\"type\":\"a\"},{\"size\":2,\"type\":\"b\"},{\"size\":1,\"type\":\"c\"}]\n\
         [{\"size\":1,\"type\":\"a\"},{\"size\":2,\"type\":\"b\"},{\"size\":1,\"type\":\"c\"}]\n\
         -- member of the last\n\
         d data {\"size\":2,\"type\":\"c\"} c\n\
         d size 2 c\n\
         direct changed [{\"size\":1,\"type\":\"a\"},{\"size\":2,\"type\":\"b\"},{\"size\":2,\"type\":\"c\"}]\n\
         k data {\"size\":2,\"type\":\"c\"} c\n\
         k size 2 c\n\
         keyed changed [{\"size\":1,\"type\":\"a\"},{\"size\":2,\"type\":\"b\"},{\"size\":2,\"type\":\"c\"}]\n\
         [{\"size\":1,\"type\":\"a\"},{\"size\":2,\"type\":\"b\"},{\"size\":2,\"type\":\"c\"}]\n\
         -- two members\n\
         d data {\"size\":2,\"type\":\"a\"} a\n\
         d size 2 a\n\
         direct changed [{\"size\":2,\"type\":\"a\"},{\"size\":2,\"type\":\"b\"},{\"size\":2,\"type\":\"c\"}]\n\
         k data {\"size\":2,\"type\":\"a\"} a\n\
         k size 2 a\n\
         keyed changed [{\"size\":2,\"type\":\"a\"},{\"size\":2,\"type\":\"b\"},{\"size\":2,\"type\":\"c\"}]\n\
         d data {\"size\":1,\"type\":\"c\"} c\n\
         d size 1 c\n\
         direct changed [{\"size\":2,\"type\":\"a\"},{\"size\":2,\"type\":\"b\"},{\"size\":1,\"type\":\"c\"}]\n\
         k data {\"size\":1,\"type\":\"c\"} c\n\
         k size 1 c\n\
         keyed changed [{\"size\":2,\"type\":\"a\"},{\"size\":2,\"type\":\"b\"},{\"size\":1,\"type\":\"c\"}]\n\
         [{\"size\":2,\"type\":\"a\"},{\"size\":2,\"type\":\"b\"},{\"size\":1,\"type\":\"c\"}]\n\
         ",
    );
}
