//! `Qt.inputMethod`: the QInputMethod QML sees (Qt 6.12 `qinputmethod.cpp`), backed by the host's
//! text input. The host reports the focused editor's geometry ([`set_geometry`]) and serves the
//! requests QML makes ([`take_requests`]); the shell speaks zwp_text_input_v3 for both.

use crate::runtime::World;
use crate::value::Value;

/// `[x, y, width, height]` in window coordinates.
pub type Rect = [f64; 4];

bits::bit_flags! {
/// Qt::InputMethodQuery bits `QInputMethod::update` re-announces.
pub const IM_CURSOR_RECTANGLE: u32 = 0x2;
pub const IM_ANCHOR_RECTANGLE: u32 = 0x4000;
pub const IM_INPUT_ITEM_CLIP_RECTANGLE: u32 = 0x8000;
}

/// The input item's rectangles (`QInputMethod::cursorRectangle` and siblings): all empty while
/// no editor has the focus.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Geometry {
    pub cursor: Rect,
    pub anchor: Rect,
    pub clip: Rect,
}

/// A QInputMethod call the platform input context serves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Request {
    Show,
    Hide,
    /// `update(queries)`: the input item's state changed; re-send what `queries` names.
    Update(u32),
    /// `reset()`: drop the composition in progress.
    Reset,
    /// `commit()`: insert the composition in progress as typed text.
    Commit,
    /// `invokeAction(action, cursorPosition)`.
    InvokeAction(i32, i32),
}

/// World-side state of the `Qt.inputMethod` object.
#[derive(Default)]
pub struct State {
    /// The QInputMethod item, once the `Qt` object exists.
    pub item: Option<usize>,
    pub geometry: Geometry,
    pub requests: Vec<Request>,
}

pub fn rect_value(r: Rect) -> Value {
    let o = crate::eval::new_obj();
    for (k, v) in ["x", "y", "width", "height"].into_iter().zip(r) {
        crate::eval::set_own(&o, k, Value::Num(v));
    }
    Value::Obj(o)
}

/// Creates the QInputMethod item behind `Qt.inputMethod`.
pub(crate) fn make(w: &mut World) -> usize {
    let im = crate::types::native_object(w, "QInputMethod");
    let g = w.input_method.geometry;
    for (k, v) in [
        ("cursorRectangle", rect_value(g.cursor)),
        ("anchorRectangle", rect_value(g.anchor)),
        ("inputItemClipRectangle", rect_value(g.clip)),
        // No input panel: Qt's text-input-v3 backend reports the cursor rectangle here, which
        // is no keyboard; an empty rectangle is what QInputMethod documents for a hidden panel.
        ("keyboardRectangle", rect_value([0.0; 4])),
        ("visible", Value::Bool(false)),
        ("animating", Value::Bool(false)),
        ("locale", crate::builtins::locale_object(&Value::Undefined)),
        ("inputDirection", Value::Num(0.0)),
    ] {
        w.write_raw(im, k, v);
    }
    w.input_method.item = Some(im);
    im
}

/// The focused editor's rectangles changed: the matching `*Changed` signals fire for the
/// ones that differ.
pub fn set_geometry(w: &mut World, g: Geometry) {
    let old = std::mem::replace(&mut w.input_method.geometry, g);
    let Some(im) = w.input_method.item else {
        return;
    };
    for (k, o, n) in [
        ("cursorRectangle", old.cursor, g.cursor),
        ("anchorRectangle", old.anchor, g.anchor),
        ("inputItemClipRectangle", old.clip, g.clip),
    ] {
        if o != n {
            w.write(im, k, rect_value(n));
        }
    }
}

/// The calls QML made since the last drain, in order.
pub fn take_requests(w: &mut World) -> Vec<Request> {
    std::mem::take(&mut w.input_method.requests)
}

/// `QInputMethod::update`: the platform re-reads the input item, and the rectangle signals
/// `queries` covers fire (Qt emits them whether or not the rectangle moved).
pub(crate) fn update(w: &mut World, queries: u32) {
    w.input_method.requests.push(Request::Update(queries));
    let Some(im) = w.input_method.item else {
        return;
    };
    for (bit, k) in [
        (IM_CURSOR_RECTANGLE, "cursorRectangle"),
        (IM_ANCHOR_RECTANGLE, "anchorRectangle"),
        (IM_INPUT_ITEM_CLIP_RECTANGLE, "inputItemClipRectangle"),
    ] {
        if queries & bit != 0 {
            w.notify(im, k);
        }
    }
}

#[cfg(all(test, feature = "interp"))]
mod tests {
    use super::*;
    use crate::runtime::Runtime;

    fn out(rt: &mut Runtime, root: usize) -> String {
        rt.get(root, "out").to_js_string()
    }

    #[test]
    fn geometry_changes_drive_bindings_and_signals() {
        let mut rt = Runtime::new(None);
        let root = rt
            .load_str(
                "import QtQuick\nItem {\n property var r: Qt.inputMethod.cursorRectangle\n \
                 property int n: 0\n property string out: [r.x, r.y, r.width, r.height, n,\n \
                 Qt.inputMethod.anchorRectangle.x, Qt.inputMethod.inputItemClipRectangle.width]\n \
                 .join()\n Connections { target: Qt.inputMethod\n \
                 function onCursorRectangleChanged() { n++ } }\n}",
            )
            .unwrap();
        rt.settle();
        assert_eq!(out(&mut rt, root), "0,0,0,0,0,0,0");
        let g = Geometry {
            cursor: [10.0, 20.0, 1.0, 16.0],
            anchor: [4.0, 20.0, 1.0, 16.0],
            clip: [0.0, 0.0, 300.0, 40.0],
        };
        set_geometry(&mut rt.world.borrow_mut(), g);
        rt.settle();
        assert_eq!(out(&mut rt, root), "10,20,1,16,1,4,300");
        set_geometry(&mut rt.world.borrow_mut(), g);
        rt.settle();
        assert_eq!(out(&mut rt, root), "10,20,1,16,1,4,300");
        set_geometry(&mut rt.world.borrow_mut(), Geometry::default());
        rt.settle();
        assert_eq!(out(&mut rt, root), "0,0,0,0,2,0,0");
    }

    #[test]
    fn calls_queue_requests_and_update_reannounces() {
        let mut rt = Runtime::new(None);
        let root = rt
            .load_str(
                "import QtQuick\nItem {\n property var seen: []\n \
                 Connections { target: Qt.inputMethod\n \
                 function onCursorRectangleChanged() { seen.push('c') }\n \
                 function onAnchorRectangleChanged() { seen.push('a') }\n \
                 function onInputItemClipRectangleChanged() { seen.push('k') } }\n \
                 function go() { const im = Qt.inputMethod; im.show(); im.hide(); im.reset();\n \
                 im.commit(); im.update(Qt.ImCursorRectangle | Qt.ImInputItemClipRectangle);\n \
                 im.invokeAction(InputMethod.Click, 3); im.update(Qt.ImQueryAll) }\n \
                 Component.onCompleted: go()\n}",
            )
            .unwrap();
        rt.settle();
        let Value::Obj(o) = rt.get(root, "seen") else {
            panic!("no output: {:?}", rt.errors())
        };
        let seen = o.borrow().array.clone().unwrap_or_default();
        let seen: Vec<String> = seen.iter().map(|v| v.to_js_string()).collect();
        assert_eq!(seen, ["c", "k", "c", "a", "k"]);
        assert_eq!(
            take_requests(&mut rt.world.borrow_mut()),
            [
                Request::Show,
                Request::Hide,
                Request::Reset,
                Request::Commit,
                Request::Update(IM_CURSOR_RECTANGLE | IM_INPUT_ITEM_CLIP_RECTANGLE),
                Request::InvokeAction(0, 3),
                Request::Update(u32::MAX),
            ]
        );
    }
}
