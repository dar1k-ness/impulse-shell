//! Creation plans replay functions declared in nested delegate templates: rebuilding the
//! outer delegates (their inner templates freed) must not leave a plan reading a freed AST.
use qml::runtime::Runtime;
use qml::value::Value;

const MAIN: &str = r#"import QtQuick
Item {
    id: root
    property int n: 2
    property int calls: 0
    Repeater {
        model: root.n
        delegate: Item {
            id: outer
            required property int index
            Cell { tag: "block" + outer.index }
            Repeater {
                model: 2
                delegate: Cell {
                    required property int index
                    tag: "cell" + outer.index + "." + index
                }
            }
        }
    }
    component Cell: Item {
        id: cell
        property string tag: ""
        Connections {
            target: cell
            function onTagChanged() {
                cell.count();
            }
        }
        function count() {
            root.calls += cell.tag.length > 0 ? 1 : 0;
        }
        Component.onCompleted: cell.count()
    }
}
"#;

#[test]
fn rebuilt_nested_delegates_replay_their_functions() {
    let d = std::env::temp_dir().join(format!("plan-freed-template-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("main.qml"), MAIN).unwrap();
    let mut rt = Runtime::new(Some(d.clone()));
    let root = rt.load_file(&d.join("main.qml")).unwrap();
    rt.settle();
    let calls = |rt: &Runtime| {
        rt.world
            .borrow_mut()
            .read(root, "calls")
            .unwrap_or_default()
            .to_js_string()
    };
    // 3 cells per outer delegate, each calling once on completion.
    let mut want = 2 * 3;
    assert_eq!(calls(&rt), want.to_string(), "{:?}", rt.errors());
    for n in [3, 1, 4, 2, 5, 3] {
        rt.world.borrow_mut().write(root, "n", Value::Num(n as f64));
        rt.settle();
        rt.world.borrow_mut().purge_dead();
        rt.settle();
        want += n * 3;
        assert_eq!(
            calls(&rt),
            want.to_string(),
            "after n = {n}: {:?}",
            rt.errors()
        );
    }
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
}
