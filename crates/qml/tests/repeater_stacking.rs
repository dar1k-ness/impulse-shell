//! QQuickRepeater::initItem parents a new delegate (appended to the parent's children, which
//! emits childrenChanged) and then stacks it before the Repeater without a signal. A binding on
//! the parent's `children` therefore last ran with the newest delegate at the end: with one
//! delegate `children[0]` stays the Repeater until something else re-runs the binding. The original's
//! ToolbarTabBar sizes its active indicator from `children[currentIndex]`, so a bar with one
//! tab shows none. Values are what Qt 6.12 gives for the same document.
use qml::runtime::Runtime;
use qml::value::Value;

const DOC: &str = r#"import QtQuick
Item {
    id: root
    property int cur: 0
    property int step: 0
    component Bar: Item {
        id: bar
        property var list
        property Item target: row.children[root.cur]
        property real tw: row.children[root.cur]?.implicitWidth ?? 0
        property int seen: row.children.length
        property int changes: 0
        property string seenAs: (target === rep ? "repeater" : "tab") + " " + tw + " " + seen
        function real() { return (row.children[0] === rep ? "repeater" : "tab") + " " + row.children.length }
        Row {
            id: row
            onChildrenChanged: bar.changes++
            Repeater { id: rep; model: bar.list; delegate: Rectangle { implicitWidth: 50; implicitHeight: 20 } }
        }
    }
    Bar { id: none; list: root.step < 1 ? [] : ["x"] }
    Bar { id: one; list: root.step < 1 ? ["a"] : ["x", "y"] }
    Bar { id: two; list: root.step < 1 ? ["a", "b"] : ["x"] }
    property string log: none.seenAs + ", " + one.seenAs + ", " + two.seenAs
    function real() { return none.real() + ", " + one.real() + ", " + two.real() }
}"#;

#[test]
fn children_binding_sees_the_newest_delegate_after_the_repeater() {
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("main.qml");
    std::fs::write(&main, DOC).unwrap();
    let mut rt = Runtime::new(Some(dir.path().to_path_buf()));
    let root = rt.load_file(&main).unwrap();
    rt.settle();
    let log = |rt: &Runtime| rt.get(root, "log").to_js_string();
    let set = |rt: &mut Runtime, k: &str, v: f64| {
        rt.world.borrow_mut().write(root, k, Value::Num(v));
        rt.settle();
    };
    // One delegate: the binding ran when it was appended, behind the Repeater.
    assert_eq!(log(&rt), "repeater 0 1, repeater 0 2, tab 50 3");
    // The children themselves are in stacking order.
    let real = rt
        .call_method(root, "real", Vec::new())
        .unwrap()
        .unwrap()
        .to_js_string();
    assert_eq!(real, "repeater 1, tab 2, tab 3");

    // The models change: 0 -> 1, 1 -> 2 and 2 -> 1 delegates.
    set(&mut rt, "step", 1.0);
    assert_eq!(log(&rt), "repeater 0 2, tab 50 3, repeater 0 2");

    // Another dependency re-runs the binding, which reads the stacking order.
    set(&mut rt, "cur", 1.0);
    assert_eq!(log(&rt), "repeater 0 2, tab 50 3, repeater 0 2");
    set(&mut rt, "cur", 0.0);
    assert_eq!(log(&rt), "tab 50 2, tab 50 3, tab 50 2");
}

/// QQmlDelegateModel emits initItem from QQmlIncubator::setInitialState, before the delegate's
/// bindings are enabled: the delegate is already stacked when they first run, and the
/// childrenChanged its own parenting emitted is not for them. So `parent.children.indexOf(tab)`
/// is each delegate's place among its siblings, the newest one's too, and it changes at most
/// once (from the property's default 0). The original's GroupButton fills the width of the group when
/// this index is 0: the only button of a row was 3 px narrower than in Qt.
const OWN: &str = r#"import QtQuick
import Qt.labs.qmlmodels
Item {
    id: root
    property int step: 0
    component Tab: Rectangle {
        id: tab
        property int at: parent.children.indexOf(tab)
        property int moves: 0
        onAtChanged: moves++
    }
    component Bar: Item {
        id: bar
        property var list
        function log() {
            let s = [];
            for (const c of row.children)
                s.push(c === rep ? "R" : c.at + "/" + c.moves);
            return s.join(" ");
        }
        Row {
            id: row
            Repeater { id: rep; model: bar.list; delegate: Tab {} }
        }
    }
    component ChosenBar: Item {
        id: bar
        property var list
        function log() {
            let s = [];
            for (const c of row.children)
                s.push(c === rep ? "R" : c.at + "/" + c.moves);
            return s.join(" ");
        }
        Row {
            id: row
            Repeater {
                id: rep
                model: bar.list
                delegate: DelegateChooser {
                    DelegateChoice { Tab {} }
                }
            }
        }
    }
    Bar { id: one; list: root.step < 1 ? ["a"] : ["x", "y"] }
    Bar { id: three; list: root.step < 1 ? ["a", "b", "c"] : ["x"] }
    ChosenBar { id: chosen; list: root.step < 1 ? ["a"] : ["x", "y"] }
    function log() { return one.log() + ", " + three.log() + ", " + chosen.log() }
}"#;

#[test]
fn a_delegates_own_binding_reads_its_place_before_the_repeater() {
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("main.qml");
    std::fs::write(&main, OWN).unwrap();
    let mut rt = Runtime::new(Some(dir.path().to_path_buf()));
    let root = rt.load_file(&main).unwrap();
    rt.settle();
    let log = |rt: &mut Runtime| {
        rt.call_method(root, "log", Vec::new())
            .unwrap()
            .unwrap()
            .to_js_string()
    };
    // Each delegate as "index it read/times that index changed", in stacking order.
    assert_eq!(log(&mut rt), "0/0 R, 0/0 1/1 2/1 R, 0/0 R");

    // New models: every delegate is created again.
    rt.world.borrow_mut().write(root, "step", Value::Num(1.0));
    rt.settle();
    assert_eq!(log(&mut rt), "0/0 1/1 R, 0/0 R, 0/0 1/1 R");
    assert_eq!(rt.world.borrow().errors, Vec::<String>::new());
}
