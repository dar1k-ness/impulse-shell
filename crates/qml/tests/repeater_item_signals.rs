//! QQuickRepeater emits `itemAdded(index, item)` once each delegate is complete and parented and
//! `itemRemoved(index, item)` before each delegate is released: a reset releases last first,
//! a model row change per row (Qt 6.12, PySide6 probe). Compiled handlers get the same
//! arguments as interpreted ones.
#![cfg(feature = "interp")]

use qml::jsrt::{Aot, BodyKind, Cx, Entry, Env, FnKind, FnMeta};
use qml::runtime::Runtime;
use qml::value::Value;
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

fn log(rt: &Runtime, root: usize) -> String {
    rt.get(root, "log").to_js_string()
}

#[test]
fn number_model_resets_release_last_first_then_add() {
    let mut rt = Runtime::new(None);
    let root = rt
        .load_str(
            r##"import QtQuick
Item {
    id: root
    property string log: ""
    property int n: 3
    Repeater {
        id: rep
        model: root.n
        onCountChanged: root.log += "#" + count
        onItemAdded: (i, it) => root.log += "+" + i + (rep.itemAt(i) === it ? "a" : "x") + rep.count + (it.parent === root ? "p" : "q")
        onItemRemoved: (i, it) => root.log += "-" + i + it.k + (rep.itemAt(i) === it ? "a" : "x") + rep.count + (it.parent === root ? "p" : "q")
        delegate: Item {
            property int k: index
            Component.onCompleted: root.log += "c" + index
            Component.onDestruction: root.log += "d" + k
        }
    }
    function step(v) { log += "|"; n = v }
}"##,
        )
        .unwrap();
    rt.settle();
    for v in [1, 2, 0] {
        let _ = rt
            .call_method(root, "step", vec![Value::Num(v as f64)])
            .unwrap();
        rt.settle();
    }
    // Qt 6.12: #0c0+0a3pc1+1a3pc2+2a3p#3|-22a3pd2-11a3pd1-00a3pd0c0+0a1p#1|
    //          -00a1pd0c0+0a2pc1+1a2p#2|-11a2pd1-00a2pd0#0|
    assert_eq!(
        log(&rt, root),
        "#0c0+0a3pc1+1a3pc2+2a3p#3\
         |-22a3pd2-11a3pd1-00a3pd0c0+0a1p#1\
         |-00a1pd0c0+0a2pc1+1a2p#2\
         |-11a2pd1-00a2pd0#0"
    );
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
}

#[test]
fn model_rows_removed_and_inserted_signal_per_row() {
    let mut rt = Runtime::new(None);
    let root = rt
        .load_str(
            r##"import QtQuick
import Rustshell
Item {
    id: root
    property string log: ""
    property var vals: ["a", "b", "c"]
    Repeater {
        id: rep
        model: ScriptModel { values: root.vals }
        onCountChanged: root.log += "#" + count
        onItemAdded: (i, it) => root.log += "+" + i + it.k + (rep.itemAt(i) === it ? "a" : "x") + rep.count
        onItemRemoved: (i, it) => root.log += "-" + i + it.k + (rep.itemAt(i) === it ? "a" : "x") + rep.count
        delegate: Item {
            property string k: modelData
            Component.onCompleted: root.log += "c" + k
            Component.onDestruction: root.log += "d" + k
        }
    }
    function step(v) { log += "|"; vals = v }
}"##,
        )
        .unwrap();
    rt.settle();
    for v in [vec!["c"], vec!["x", "c", "y"], vec![]] {
        let v = Value::array(v.into_iter().map(Value::str).collect());
        let _ = rt.call_method(root, "step", vec![v]).unwrap();
        rt.settle();
    }
    // Qt 6.12 over a ListModel with the same row changes (remove(0, 2); insert(0, x) and
    // append(y); remove(0, 3)): #0ca+0aa3cb+1ba3cc+2ca3#3|-0ax1da-0bx1db#1|cx+0xa2#2cy+2ya3#3|
    // -0xx0dx-0cx0dc-0yx0dy#0
    assert_eq!(
        log(&rt, root),
        "#0ca+0aa3cb+1ba3cc+2ca3#3\
         |-0ax1da-0bx1db#1\
         |cx+0xa2#2cy+2ya3#3\
         |-0xx0dx-0cx0dc-0yx0dy#0"
    );
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
}

/// A row whose DelegateChooser has no matching DelegateChoice gets no delegate
/// (QQuickRepeaterPrivate::deletables keeps a null slot): `count` stays the model's,
/// `itemAt` of that row is null and later rows keep their model index in `itemAt`,
/// itemAdded and itemRemoved.
const HOLES: &str = r##"import QtQuick
Item {
    id: root
    property string log: ""
    property var vals: [{ k: "x", t: "a" }, { k: "none", t: "b" }, { k: "x", t: "c" }]
    function at(i) { const it = rep.itemAt(i); return it === null ? "n" : it.t }
    function probe() { log += "|" + rep.count + at(0) + at(1) + at(2) + at(3) }
    Repeater {
        id: rep
        model: root.vals
        onItemAdded: (i, it) => root.log += "+" + i + it.t + (rep.itemAt(i) === it ? "a" : "x")
        onItemRemoved: (i, it) => root.log += "-" + i + it.t + (rep.itemAt(i) === it ? "a" : "x")
        delegate: DelegateChooser {
            role: "k"
            DelegateChoice {
                roleValue: "x"
                Item {
                    required property var modelData
                    property string t: modelData.t
                    Component.onDestruction: root.log += "d" + t
                }
            }
        }
    }
    function step(v) { log += "/"; vals = v }
}"##;

#[test]
fn rows_without_a_delegate_keep_later_rows_at_their_model_index() {
    let mut rt = Runtime::new(None);
    let root = rt.load_str(HOLES).unwrap();
    rt.settle();
    let _ = rt.call_method(root, "probe", vec![]).unwrap();
    let none = Value::array(vec![]);
    let _ = rt.call_method(root, "step", vec![none]).unwrap();
    rt.settle();
    let _ = rt.call_method(root, "probe", vec![]).unwrap();
    // Qt 6.12 (PySide6 6.12.0): +0aa+2ca|3ancn/-2cadc-0aada|0nnnn
    assert_eq!(log(&rt, root), "+0aa+2ca|3ancn/-2cadc-0aada|0nnnn");
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
}

/// A Repeater over a ScriptModel with a DelegateChooser keeps every surviving delegate when
/// `values` changes: rows are inserted, removed and moved one by one (a hole row, a choice
/// with no match, is removed with a null itemRemoved), never the whole view rebuilt.
const CHOOSER: &str = r##"import QtQuick
import Rustshell
Item {
    id: root
    property string log: ""
    property var a: ({ k: "x", t: "a" })
    property var b: ({ k: "none", t: "b" })
    property var c: ({ k: "x", t: "c" })
    property var d: ({ k: "y", t: "d" })
    property var e: ({ k: "none", t: "e" })
    property var vals: [a, b, c]
    function at(i) { const it = rep.itemAt(i); return it === null ? "n" : it.t + it.index }
    function probe() { let s = "|" + rep.count; for (let i = 0; i < rep.count + 1; i++) s += at(i); log += s }
    Repeater {
        id: rep
        model: ScriptModel { values: root.vals }
        onItemAdded: (i, it) => root.log += "+" + i + it.t + (rep.itemAt(i) === it ? "a" : "x")
        onItemRemoved: (i, it) => root.log += "-" + i + (it === null ? "N" : it.t) + (rep.itemAt(i) === it ? "a" : "x")
        delegate: DelegateChooser {
            role: "k"
            DelegateChoice {
                roleValue: "x"
                Item {
                    required property var modelData
                    required property int index
                    property string t: modelData.t
                    Component.onCompleted: root.log += "c" + t + index
                    Component.onDestruction: root.log += "d" + t
                }
            }
            DelegateChoice {
                roleValue: "y"
                Item {
                    required property var modelData
                    required property int index
                    property string t: modelData.t
                    Component.onCompleted: root.log += "C" + t + index
                    Component.onDestruction: root.log += "D" + t
                }
            }
        }
    }
    function step(v) { probe(); log += "/"; vals = v }
}"##;

fn chooser_steps(rt: &mut Runtime, root: usize) {
    rt.settle();
    let o = |n: &str| rt.get(root, n);
    let (a, b, c, d, e) = (o("a"), o("b"), o("c"), o("d"), o("e"));
    for v in [
        vec![a.clone(), b.clone(), d.clone(), c.clone()],
        vec![a.clone(), d, c.clone()],
        vec![c, a, e],
        vec![],
    ] {
        let _ = rt.call_method(root, "step", vec![Value::array(v)]).unwrap();
        rt.settle();
    }
    let _ = rt.call_method(root, "probe", vec![]).unwrap();
}

#[test]
fn script_model_rows_with_a_delegate_chooser_change_per_row() {
    let mut rt = Runtime::new(None);
    let root = rt.load_str(CHOOSER).unwrap();
    chooser_steps(&mut rt, root);
    // Qt 6.12 (PySide6 6.12.0) over a ListModel with the same row changes (insert(2, d);
    // remove(1, 1); move(2, 0, 1), insert(2, e), remove(3, 1); remove(0, 3)):
    // ca0+0aacc2+2ca|3a0nc2n/Cd2+2da|4a0nd2c3n/-1Nx|3a0d1c2n/-3dxDd|3c0a1nn/-0cxdc-0axda-0Na|0n
    assert_eq!(
        log(&rt, root),
        "ca0+0aacc2+2ca|3a0nc2n\
         /Cd2+2da|4a0nd2c3n\
         /-1Nx|3a0d1c2n\
         /-3dxDd|3c0a1nn\
         /-0cxdc-0axda-0Na|0n"
    );
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
}

thread_local! {
    static CALLS: RefCell<Vec<(char, f64, Option<usize>)>> = const { RefCell::new(vec![]) };
}

fn record(sign: char, args: &[Value]) {
    let index = args.first().map_or(f64::NAN, |v| v.to_number());
    let item = match args.get(1) {
        Some(Value::Item(i)) => Some(*i),
        _ => None,
    };
    CALLS.with(|c| c.borrow_mut().push((sign, index, item)));
}

fn added(_: &mut Cx, _: &Env, _: &Value, args: &[Value]) -> qml::eval::R<Value> {
    record('+', args);
    Ok(Value::Undefined)
}

fn removed(_: &mut Cx, _: &Env, _: &Value, args: &[Value]) -> qml::eval::R<Value> {
    record('-', args);
    Ok(Value::Undefined)
}

/// Compiles `onItemAdded`/`onItemRemoved` of `Main.qml` to [`added`]/[`removed`].
struct Handlers;

impl Aot for Handlers {
    fn doc_scripts(&self, file: &Path, doc: &qml::Document) -> Vec<(qml::NodeId, &'static Entry)> {
        let mut out = vec![];
        if !file.ends_with("Main.qml") {
            return out;
        }
        qml::walk::sites(doc, &mut |s| {
            let (member, f): (&'static str, qml::jsrt::CompiledFn) = match s.name.as_str() {
                "onItemAdded" => ("onItemAdded", added),
                "onItemRemoved" => ("onItemRemoved", removed),
                _ => return,
            };
            let e: &'static Entry = Box::leak(Box::new(Entry {
                file: "Main.qml",
                path: Box::leak(s.path.clone().into_boxed_str()),
                member,
                kind: BodyKind::Handler,
                f,
                meta: FnMeta {
                    name: "",
                    kind: FnKind::Normal,
                    nparams: 0,
                    nslots: 0,
                    strict: false,
                },
                hash: s.hash(),
            }));
            out.push((s.script.id, e));
        });
        out
    }
    fn library(&self, _: &Path) -> Option<&'static Entry> {
        None
    }
    fn script(&self, _: &Path) -> Option<&'static Entry> {
        None
    }
}

/// Every itemAdded (`+index`) and itemRemoved (`-index`, `=` when it names the delegate the
/// addition at that index announced) while `n` goes 3, 1, 0, the handlers compiled or not.
fn signals(compiled: bool) -> String {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("Main.qml"),
        "import QtQuick\nItem {\n    id: root\n    property int n: 3\n    property string log: \"\"\n    property var made: []\n    Repeater {\n        model: root.n\n        onItemAdded: { root.made[index] = item; root.log += \"+\" + index }\n        onItemRemoved: root.log += \"-\" + index + (root.made[index] === item ? \"=\" : \"!\")\n        Item {}\n    }\n}\n",
    )
    .unwrap();
    CALLS.with(|c| c.borrow_mut().clear());
    let mut rt = Runtime::new(Some(d.path().to_path_buf()));
    if compiled {
        rt.eng().set_compiled(Some(Rc::new(Handlers)));
    }
    let root = rt.load_file(&d.path().join("Main.qml")).unwrap();
    rt.settle();
    for n in [1, 0] {
        rt.set(root, "n", Value::Num(n as f64));
        rt.settle();
    }
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    if !compiled {
        return rt.get(root, "log").to_js_string();
    }
    let mut made = std::collections::HashMap::new();
    let mut out = String::new();
    for (sign, index, item) in CALLS.with(|c| c.borrow().clone()) {
        out.push(sign);
        out.push_str(&index.to_string());
        let item = item.expect("delegate argument");
        if sign == '+' {
            made.insert(index as usize, item);
        } else {
            out.push(if made.get(&(index as usize)) == Some(&item) {
                '='
            } else {
                '!'
            });
        }
    }
    out
}

#[test]
fn compiled_handlers_get_the_index_and_delegate() {
    assert_eq!(signals(false), "+0+1+2-2=-1=-0=+0-0=");
    assert_eq!(signals(true), "+0+1+2-2=-1=-0=+0-0=");
}

/// Compiled itemAdded/itemRemoved handlers of a Repeater whose middle row has no delegate get
/// the model index of each delegate, also when the reset releases them.
#[test]
fn compiled_handlers_get_model_indices_around_a_row_without_a_delegate() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("Main.qml"), HOLES).unwrap();
    CALLS.with(|c| c.borrow_mut().clear());
    let mut rt = Runtime::new(Some(d.path().to_path_buf()));
    rt.eng().set_compiled(Some(Rc::new(Handlers)));
    let root = rt.load_file(&d.path().join("Main.qml")).unwrap();
    rt.settle();
    rt.set(root, "vals", Value::array(vec![]));
    rt.settle();
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    let calls = CALLS.with(|c| c.borrow().clone());
    let signs: Vec<(char, f64)> = calls.iter().map(|&(s, i, _)| (s, i)).collect();
    assert_eq!(signs, [('+', 0.0), ('+', 2.0), ('-', 2.0), ('-', 0.0)]);
    assert_eq!(calls[0].2, calls[3].2);
    assert_eq!(calls[1].2, calls[2].2);
    assert_ne!(calls[0].2, calls[1].2);
}

/// Compiled itemAdded/itemRemoved handlers of a ScriptModel Repeater with a DelegateChooser get
/// one call per changed row: the surviving delegates are never released and re-added, and a
/// removed row without a delegate passes a null item.
#[test]
fn compiled_handlers_see_per_row_changes_through_a_delegate_chooser() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("Main.qml"), CHOOSER).unwrap();
    CALLS.with(|c| c.borrow_mut().clear());
    let mut rt = Runtime::new(Some(d.path().to_path_buf()));
    rt.eng().set_compiled(Some(Rc::new(Handlers)));
    let root = rt.load_file(&d.path().join("Main.qml")).unwrap();
    chooser_steps(&mut rt, root);
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    let calls = CALLS.with(|c| c.borrow().clone());
    let signs: Vec<(char, f64, bool)> = calls
        .iter()
        .map(|&(s, i, it)| (s, i, it.is_some()))
        .collect();
    assert_eq!(
        signs,
        [
            ('+', 0.0, true),
            ('+', 2.0, true),
            ('+', 2.0, true),
            ('-', 1.0, false),
            ('-', 3.0, true),
            ('-', 0.0, true),
            ('-', 0.0, true),
            ('-', 0.0, false),
        ]
    );
    // a, c, d: each delegate is announced once and removed once.
    let (a, c, d) = (calls[0].2, calls[1].2, calls[2].2);
    assert_eq!([calls[4].2, calls[5].2, calls[6].2], [d, c, a]);
}
