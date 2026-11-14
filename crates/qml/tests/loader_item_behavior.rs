//! QQuickLoader sizes the item it creates before it tells of it, and the item's Behaviors are
//! finalized by then: what `onActiveChanged` (or `onLoaded`) writes to the item animates. The
//! shell lays the item out through `World::loaded_layout` before the Loader's handlers run, so
//! the item is no longer `fresh` (not yet laid out, where a Behavior does not intercept).

use qml::runtime::Runtime;
use qml::value::Value;

fn doc(handler: &str) -> String {
    format!(
        r#"import QtQuick
Item {{
    id: root
    property bool shown: false
    property real h: ld.item ? ld.item.h : -1
    property string log: ""
    Loader {{
        id: ld
        readonly property bool shown: root.shown
        onShownChanged: if (shown) ld.active = true
        active: shown
        onItemChanged: root.log += "item "
        onStatusChanged: root.log += "status "
        {handler}
        sourceComponent: Item {{
            width: 10
            property bool show: false
            property real h: show ? 100 : 0
            Behavior on h {{ NumberAnimation {{ duration: 100 }} }}
        }}
    }}
}}"#
    )
}

/// The value of `h` at the load and 16, 50 and 100 ms after it, and the order of the signals
/// (`active: false` at creation is the first activeChanged).
fn load(handler: &str, hook: bool) -> ([f64; 4], String) {
    let mut rt = Runtime::new(None);
    rt.world.borrow_mut().track_fresh = true;
    if hook {
        rt.world.borrow_mut().loaded_layout = Some(Box::new(|w, i, done| {
            if done {
                let mut st = vec![i];
                while let Some(x) = st.pop() {
                    w.fresh.remove(&x);
                    st.extend(w.items[x].children.iter().copied());
                }
            }
            true
        }));
    }
    let root = rt.load_str(&doc(handler)).unwrap();
    rt.settle();
    rt.tick(0.0);
    rt.tick(1000.0);
    // The window's first layout pass.
    rt.world.borrow_mut().fresh.clear();
    rt.set(root, "shown", Value::Bool(true));
    rt.settle();
    let h = [1000.0, 1016.0, 1050.0, 1100.0].map(|t| {
        rt.tick(t);
        rt.get(root, "h").to_number()
    });
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    (h, rt.get(root, "log").to_js_string())
}

const ACTIVE: &str = r#"onActiveChanged: { root.log += "active "; if (active) item.show = true }"#;
const LOADED: &str = r#"onLoaded: { root.log += "loaded "; item.show = true }"#;

#[test]
fn write_in_active_changed_animates() {
    let (h, log) = load(ACTIVE, true);
    assert_eq!(h[0], 0.0, "{h:?}");
    assert!(h[1] > 0.0 && h[1] < h[2] && h[2] < 100.0, "{h:?}");
    assert_eq!(h[3], 100.0, "{h:?}");
    assert_eq!(log, "active item status active ");
}

#[test]
fn write_in_loaded_animates() {
    let (h, log) = load(LOADED, true);
    assert_eq!(h[0], 0.0, "{h:?}");
    assert!(h[1] > 0.0 && h[1] < h[2] && h[2] < 100.0, "{h:?}");
    assert_eq!(h[3], 100.0, "{h:?}");
    assert_eq!(log, "item status loaded ");
}

/// Without a layout to ask (no shell) the signals come in the same order.
#[test]
fn signal_order_without_layout() {
    assert_eq!(load(ACTIVE, false).1, "active item status active ");
    assert_eq!(load(LOADED, false).1, "item status loaded ");
}
