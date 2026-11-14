//! An attached object nothing in the document set is made by the first read of a script
//! (qmlAttachedPropertiesObject with `create`), one for an object and an attached type: a
//! document at rest has none, and reading it again makes no more.

use qml::runtime::Runtime;
use qml::value::Value;

const SRC: &str = r#"import QtQuick
import QtQuick.Layouts
import QtQuick.Controls
Item {
    id: root
    Item { id: plain }
    Item { id: other }
    function read() {
        return [plain.Layout.leftMargin, plain.Layout.fillWidth, plain.Keys.enabled, plain.ToolTip.visible,
                plain.Layout === plain.Layout, plain.Layout === other.Layout].join(" ")
    }
}
"#;

fn objects(rt: &Runtime) -> usize {
    let w = rt.world.borrow();
    (0..w.items.len()).filter(|&i| w.items.holds(i)).count()
}

#[test]
fn an_attached_object_is_made_once_by_the_first_read() {
    let mut rt = Runtime::new(None);
    let root = rt.load_str(SRC).unwrap();
    rt.settle();
    let loaded = objects(&rt);
    rt.settle();
    assert_eq!(objects(&rt), loaded, "at rest");
    let read = |rt: &mut Runtime| {
        let v = match rt.call_method(root, "read", vec![]) {
            Some(Ok(Value::Str(s))) => s.to_string(),
            v => panic!("read() gave {v:?}"),
        };
        rt.settle();
        v
    };
    // Qt 6.12: `0 false true false true false`.
    assert_eq!(read(&mut rt), "0 false true false true false");
    // `plain.Layout`, `plain.Keys`, `plain.ToolTip` and `other.Layout`.
    assert_eq!(objects(&rt), loaded + 4, "after the first read");
    for _ in 0..50 {
        read(&mut rt);
    }
    assert_eq!(objects(&rt), loaded + 4, "after 50 more reads");
    assert_eq!(rt.errors(), Vec::<String>::new());
}
