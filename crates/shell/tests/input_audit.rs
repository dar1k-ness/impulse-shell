//! Keyboard, focus and wheel behaviour measured on Qt 6.12 (qmltestrunner, offscreen, in the
//! tools/testenv image): each test replays the scenario and asserts the values Qt printed.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::{
    input::{Pointer, open_popup},
    keys::Keyboard,
    scene,
};

bits::bit_flags! {
const SHIFT: u32 = 0x0200_0000;
const CTRL: u32 = 0x0400_0000;
}

const BTN_LEFT: u32 = 0x110;

const XK_TAB: u32 = 0xff09;
const XK_ISO_LEFT_TAB: u32 = 0xfe20;
const XK_RETURN: u32 = 0xff0d;
const XK_KP_ENTER: u32 = 0xff8d;
const XK_BACKSPACE: u32 = 0xff08;
const XK_DELETE: u32 = 0xffff;
const XK_LEFT: u32 = 0xff51;
const XK_UP: u32 = 0xff52;
const XK_RIGHT: u32 = 0xff53;
const XK_DOWN: u32 = 0xff54;
const XK_SPACE: u32 = 0x20;

struct Win {
    rt: Runtime,
    root: usize,
    kb: Keyboard,
}

impl Win {
    fn new(qml: &str) -> Win {
        let mut rt = Runtime::new(None);
        let root = rt.load_str(qml).unwrap();
        rt.settle();
        let mut kb = Keyboard::default();
        kb.win = Some(root);
        let size = [rt.get(root, "width"), rt.get(root, "height")].map(|v| v.to_number());
        scene::layout_window(&mut rt, &mut kb.fonts.borrow_mut(), root, size);
        let mut w = Win { rt, root, kb };
        w.kb.sync(&mut w.rt);
        w.rt.settle();
        w
    }

    fn item(&mut self, name: &str) -> usize {
        match self.rt.world.borrow_mut().read(self.root, name) {
            Some(Value::Item(i)) => i,
            v => panic!("{name}: {v:?}"),
        }
    }

    fn focus(&mut self, name: &str) -> usize {
        let i = self.item(name);
        self.rt.world.borrow_mut().force_active_focus(i);
        self.kb.sync(&mut self.rt);
        self.rt.settle();
        i
    }

    /// A press and release of keysym `sym` with Qt modifiers `mods` held.
    fn tap(&mut self, sym: u32, mods: u32) {
        self.kb.mods = mods;
        let text = match sym {
            0x20..=0x7e if mods & CTRL == 0 => char::from_u32(sym).map(String::from),
            XK_TAB => Some("\t".into()),
            XK_RETURN | XK_KP_ENTER => Some("\r".into()),
            _ => None,
        };
        self.kb.key(&mut self.rt, sym, text.as_deref(), true, false);
        self.kb.key(&mut self.rt, sym, None, false, false);
        self.rt.settle();
        self.kb.mods = 0;
    }

    fn keys(&mut self, text: &str) {
        for c in text.chars() {
            self.tap(c as u32, 0);
        }
    }

    fn set(&mut self, i: usize, name: &str, v: Value) {
        self.rt.set(i, name, v);
        self.rt.settle();
    }

    fn num(&mut self, i: usize, name: &str) -> f64 {
        self.rt.get(i, name).to_number()
    }

    fn text(&mut self, i: usize, name: &str) -> String {
        self.rt.get(i, name).to_js_string()
    }

    fn on(&mut self, i: usize, name: &str) -> bool {
        self.rt.get(i, name).truthy()
    }

    /// Calls the root's QML function `name`.
    fn call(&mut self, name: &str, args: Vec<Value>) {
        self.rt.call_method(self.root, name, args).unwrap().unwrap();
        self.rt.settle();
    }

    /// The root's `log`, emptied.
    fn log(&mut self) -> String {
        let l = self.text(self.root, "log");
        self.set(self.root, "log", Value::Str("".into()));
        l
    }

    /// The names among `names` whose item has active focus.
    fn who(&mut self, names: &[&str]) -> String {
        let mut out = Vec::new();
        for n in names {
            let i = self.item(n);
            if self.on(i, "activeFocus") {
                out.push(*n);
            }
        }
        out.join(",")
    }
}

const EDITORS: &str = r#"
import QtQuick
import QtQuick.Controls
Item {
    id: root
    width: 400; height: 400
    property string log: ""
    property var ta: ta
    property var tf: tf
    property var plain: plain
    property var tf2: tf2
    property var ti: ti
    function sel(ed, from, to) { ed.select(from, to) }
    function back(ed) { ed.undo() }
    function forth(ed) { ed.redo() }
    function put(ed, at, text) { ed.insert(at, text) }
    Item {
        Keys.onPressed: event => { root.log += "parent:" + event.key.toString(16) + ";" }
        TextArea { id: ta; width: 300; height: 150; text: "abcdef\nx\nabcdef" }
        TextField { id: tf; y: 210; width: 300; text: "ab" }
        Item { id: plain; y: 240; width: 10; height: 10 }
        TextField { id: tf2; y: 260; width: 300 }
        TextInput { id: ti; y: 300; width: 100; height: 20; text: "q" }
    }
}
"#;

fn caret(w: &mut Win, i: usize) -> (f64, f64, f64) {
    (
        w.num(i, "cursorPosition"),
        w.num(i, "selectionStart"),
        w.num(i, "selectionEnd"),
    )
}

/// Finding 1: Up and Down move a multi-line editor's caret by visual line, keeping its x.
#[test]
fn finding_01_up_down_move_between_lines() {
    let mut w = Win::new(EDITORS);
    let ta = w.focus("ta");
    w.set(ta, "cursorPosition", Value::Num(5.0));
    w.tap(XK_DOWN, 0);
    assert_eq!(w.num(ta, "cursorPosition"), 8.0, "Down from 5");
    w.tap(XK_DOWN, 0);
    assert_eq!(w.num(ta, "cursorPosition"), 14.0, "Down again keeps the x");
    assert_eq!(w.log(), "");
    w.tap(XK_DOWN, 0);
    assert_eq!(w.num(ta, "cursorPosition"), 14.0, "Down on the last line");
    assert_eq!(w.log(), "parent:1000015;");
    w.tap(XK_UP, 0);
    w.tap(XK_UP, 0);
    assert_eq!(w.num(ta, "cursorPosition"), 5.0, "Up Up");
    assert_eq!(w.log(), "");
    w.tap(XK_UP, 0);
    assert_eq!(w.num(ta, "cursorPosition"), 5.0, "Up on the first line");
    assert_eq!(w.log(), "parent:1000013;");
    w.tap(XK_UP, SHIFT);
    assert_eq!(
        caret(&mut w, ta),
        (5.0, 5.0, 5.0),
        "Shift+Up on the first line"
    );
    assert_eq!(w.log(), "parent:1000013;");
    w.set(ta, "cursorPosition", Value::Num(7.0));
    w.tap(XK_LEFT, 0);
    w.tap(XK_DOWN, 0);
    assert_eq!(
        w.num(ta, "cursorPosition"),
        8.0,
        "Down from the end of line 1"
    );
    w.set(ta, "cursorPosition", Value::Num(12.0));
    w.tap(XK_UP, SHIFT);
    w.tap(XK_UP, SHIFT);
    assert_eq!(
        caret(&mut w, ta),
        (3.0, 3.0, 12.0),
        "Shift+Up twice from 12"
    );
    w.tap(XK_DOWN, 0);
    assert_eq!(
        caret(&mut w, ta),
        (8.0, 8.0, 8.0),
        "Down drops the selection"
    );
    // A single-line editor leaves Down to its parents.
    w.focus("ti");
    w.log();
    w.tap(XK_DOWN, 0);
    assert_eq!(w.log(), "parent:1000015;");
}

/// Text, cursorPosition, canUndo, canRedo.
fn state(w: &mut Win, i: usize) -> (String, f64, bool, bool) {
    (
        w.text(i, "text"),
        w.num(i, "cursorPosition"),
        w.on(i, "canUndo"),
        w.on(i, "canRedo"),
    )
}

fn st(text: &str, cursor: f64, undo: bool, redo: bool) -> (String, f64, bool, bool) {
    (text.into(), cursor, undo, redo)
}

/// Finding 2: Ctrl+Z, Ctrl+Shift+Z, Ctrl+Y, undo() and redo() walk the edit history in Qt's
/// steps; canUndo and canRedo follow.
#[test]
fn finding_02_undo_and_redo() {
    for name in ["tf", "ta"] {
        let area = name == "ta";
        let mut w = Win::new(EDITORS);
        let ed = w.focus(name);
        let assign = |w: &mut Win, text: &str, cursor: f64| {
            w.set(ed, "text", Value::Str(text.into()));
            w.set(ed, "cursorPosition", Value::Num(cursor));
        };
        let undo = |w: &mut Win| w.tap('z' as u32, CTRL);
        let redo = |w: &mut Win| w.tap('z' as u32, CTRL | SHIFT);
        assign(&mut w, "ab", 2.0);
        assert_eq!(state(&mut w, ed), st("ab", 2.0, false, false), "{name}");
        w.keys("xyz");
        assert_eq!(state(&mut w, ed), st("abxyz", 5.0, true, false), "{name}");
        undo(&mut w);
        assert_eq!(state(&mut w, ed), st("ab", 2.0, false, true), "{name}");
        w.tap('y' as u32, CTRL);
        assert_eq!(state(&mut w, ed), st("abxyz", 5.0, true, false), "{name}");
        undo(&mut w);
        redo(&mut w);
        assert_eq!(state(&mut w, ed), st("abxyz", 5.0, true, false), "{name}");
        // Moving the caret ends the run of typed characters.
        w.tap(XK_LEFT, 0);
        w.keys("q");
        undo(&mut w);
        assert_eq!(state(&mut w, ed), st("abxyz", 4.0, true, true), "{name}");
        undo(&mut w);
        assert_eq!(state(&mut w, ed), st("ab", 2.0, false, true), "{name}");
        undo(&mut w);
        assert_eq!(state(&mut w, ed), st("ab", 2.0, false, true), "{name}");
        // Assigning the text starts the history over.
        assign(&mut w, "abcdef", 6.0);
        assert_eq!(state(&mut w, ed), st("abcdef", 6.0, false, false), "{name}");
        w.tap(XK_BACKSPACE, 0);
        w.tap(XK_BACKSPACE, 0);
        undo(&mut w);
        assert_eq!(state(&mut w, ed), st("abcdef", 6.0, false, true), "{name}");
        assign(&mut w, "abcdef", 0.0);
        w.tap(XK_DELETE, 0);
        w.tap(XK_DELETE, 0);
        undo(&mut w);
        // A document's undo leaves the caret after what it put back.
        let at = if area { 2.0 } else { 0.0 };
        assert_eq!(state(&mut w, ed), st("abcdef", at, false, true), "{name}");
        // Typing over a selection undoes with the removal, in one step.
        assign(&mut w, "abcdef", 6.0);
        w.call(
            "sel",
            vec![Value::Item(ed), Value::Num(1.0), Value::Num(3.0)],
        );
        w.keys("wv");
        assert_eq!(state(&mut w, ed), st("awvdef", 3.0, true, false), "{name}");
        undo(&mut w);
        assert_eq!(state(&mut w, ed), st("abcdef", 3.0, false, true), "{name}");
        // QQuickTextInput's undo brings the selection back; QTextDocument's does not.
        let selected = if area { "" } else { "bc" };
        assert_eq!(w.text(ed, "selectedText"), selected, "{name}");
        redo(&mut w);
        assert_eq!(state(&mut w, ed), st("awvdef", 3.0, true, false), "{name}");
        assign(&mut w, "abcdef", 6.0);
        w.tap(XK_BACKSPACE, CTRL);
        assert_eq!(state(&mut w, ed), st("", 0.0, true, false), "{name}");
        w.call("back", vec![Value::Item(ed)]);
        assert_eq!(state(&mut w, ed), st("abcdef", 6.0, false, true), "{name}");
        w.call("forth", vec![Value::Item(ed)]);
        assert_eq!(state(&mut w, ed), st("", 0.0, true, false), "{name}");
        w.call(
            "put",
            vec![Value::Item(ed), Value::Num(0.0), Value::Str("ZZ".into())],
        );
        assert_eq!(state(&mut w, ed), st("ZZ", 2.0, true, false), "{name}");
        undo(&mut w);
        assert_eq!(state(&mut w, ed), st("", 0.0, true, true), "{name}");
        undo(&mut w);
        assert_eq!(state(&mut w, ed), st("abcdef", 6.0, false, true), "{name}");
        // A typed character, a Backspace and another character are three steps.
        assign(&mut w, "ab", 2.0);
        w.keys("x");
        w.tap(XK_BACKSPACE, 0);
        w.keys("y");
        undo(&mut w);
        assert_eq!(state(&mut w, ed), st("ab", 2.0, true, true), "{name}");
        undo(&mut w);
        assert_eq!(state(&mut w, ed), st("abx", 3.0, true, true), "{name}");
        undo(&mut w);
        assert_eq!(state(&mut w, ed), st("ab", 2.0, false, true), "{name}");
        // A read-only TextField has nothing to redo; a read-only TextArea keeps canRedo but
        // ignores the key.
        w.set(ed, "readOnly", Value::Bool(true));
        assert_eq!(w.on(ed, "canRedo"), area, "{name}");
        redo(&mut w);
        assert_eq!(w.text(ed, "text"), "ab", "{name}");
    }
    // A line break is its own step between the runs of text around it.
    let mut w = Win::new(EDITORS);
    let ta = w.focus("ta");
    w.set(ta, "text", Value::Str("ab".into()));
    w.set(ta, "cursorPosition", Value::Num(2.0));
    w.keys("x");
    w.tap(XK_RETURN, 0);
    w.keys("y");
    for want in [("abx\n", 4.0), ("abx", 3.0), ("ab", 2.0)] {
        w.tap('z' as u32, CTRL);
        let got = (w.text(ta, "text"), w.num(ta, "cursorPosition"));
        assert_eq!(got, (want.0.to_string(), want.1));
    }
}

/// Finding 3: Tab and Backtab move the focus on the way up from the focus item; parents' Keys
/// handlers see the key only when the focus item is not itself a tab stop.
#[test]
fn finding_03_tab_moves_focus_before_parent_keys() {
    let names = ["ta", "tf", "plain", "tf2", "ti"];
    let mut w = Win::new(EDITORS);
    w.focus("plain");
    w.log();
    w.tap(XK_TAB, 0);
    assert_eq!(
        (w.who(&names), w.log()),
        ("ta".into(), "parent:1000001;".into())
    );
    w.focus("plain");
    w.log();
    w.tap(XK_ISO_LEFT_TAB, SHIFT);
    assert_eq!(
        (w.who(&names), w.log()),
        ("tf2".into(), "parent:1000002;".into())
    );
    // A tab stop takes the key: the parent's handler does not see it.
    w.focus("tf");
    w.log();
    w.tap(XK_TAB, 0);
    assert_eq!((w.who(&names), w.log()), ("tf2".into(), "".into()));
    w.focus("tf");
    w.tap(XK_ISO_LEFT_TAB, SHIFT);
    assert_eq!((w.who(&names), w.log()), ("ta".into(), "".into()));
}

/// Finding 7: a plain TextInput or TextEdit is not a tab stop; the Controls editors are.
#[test]
fn finding_07_plain_editors_are_not_tab_stops() {
    let mut w = Win::new(
        r#"
import QtQuick
import QtQuick.Controls
Item {
    id: root
    width: 300; height: 200
    property var a: a
    property var ti: ti
    property var te: te
    property var b: b
    TextField { id: a }
    TextInput { id: ti; y: 40; width: 100; height: 20 }
    TextEdit { id: te; y: 70; width: 100; height: 20 }
    TextField { id: b; y: 100 }
}
"#,
    );
    let names = ["a", "ti", "te", "b"];
    w.focus("a");
    let mut order = vec![w.who(&names)];
    for _ in 0..3 {
        w.tap(XK_TAB, 0);
        order.push(w.who(&names));
    }
    assert_eq!(order, ["a", "b", "a", "b"]);
    for name in ["ti", "te"] {
        let i = w.item(name);
        assert!(!w.on(i, "activeFocusOnTab"), "{name}");
    }
    // A plain TextInput leaves Tab to the window, which starts from its first tab stop.
    w.focus("ti");
    w.tap(XK_TAB, 0);
    assert_eq!(w.who(&names), "a");
    // A TextEdit types it.
    let te = w.focus("te");
    w.tap(XK_TAB, 0);
    assert_eq!(
        (w.who(&names), w.text(te, "text")),
        ("te".into(), "\t".into())
    );
}

/// A SwipeView is a tab stop (QQuickSwipeView sets activeFocusOnTab): a Tab that nothing in a
/// page took passes the page's Keys handler and moves on from the SwipeView, into its first tab
/// stop, instead of from the window's first one. The item that loses active focus and the one
/// that gets it keep the reason (Control.focusReason: 1 Tab, 2 Backtab, 7 Other). The focus order
/// and the log are Qt 6.12's on the same tree (its Shift+Tab came as Key_Tab with Shift held, from
/// a keymap without ISO_Left_Tab).
#[test]
fn tab_unhandled_in_a_swipe_view_page_moves_on_inside_the_swipe_view() {
    let mut w = Win::new(
        r#"
import QtQuick
import QtQuick.Controls
Item {
    id: root
    width: 300; height: 300
    property string log: ""
    property var outer: outer
    property var view: view
    property var first: first
    property var search: search
    property var field: field
    Button { id: outer; width: 100; height: 20; onFocusReasonChanged: root.log += "outer" + focusReason + ";" }
    SwipeView {
        id: view
        y: 30; width: 300; height: 250
        Item {
            Keys.onPressed: event => { root.log += "page:" + event.key.toString(16) + ";"; field.forceActiveFocus() }
            Button { id: first; width: 100; height: 20; onFocusReasonChanged: root.log += "first" + focusReason + ";" }
            TextInput { id: search; y: 30; width: 100; height: 20 }
            TextArea { id: field; y: 60; width: 100; height: 40; onFocusReasonChanged: root.log += "field" + focusReason + ";" }
        }
    }
}
"#,
    );
    let names = ["outer", "first", "search", "field"];
    let view = w.item("view");
    assert!(w.on(view, "activeFocusOnTab"));
    assert_eq!(w.num(view, "focusPolicy"), 1.0);
    w.focus("search");
    w.log();
    w.tap(XK_TAB, 0);
    assert_eq!(
        (w.who(&names), w.log()),
        ("first".into(), "page:1000001;field1;first1;".into())
    );
    // A typed key goes to the button, which leaves it to the page: all but the first land in
    // the field.
    w.keys("ab");
    let field = w.item("field");
    assert_eq!(
        (w.who(&names), w.text(field, "text"), w.log()),
        ("field".into(), "b".into(), "page:41;first7;field7;".into())
    );
    // Backwards the SwipeView is left for the stop before it, from a tab stop in it too: the
    // SwipeView holding the focus is passed over.
    w.focus("search");
    w.tap(XK_ISO_LEFT_TAB, SHIFT);
    assert_eq!(
        (w.who(&names), w.log()),
        ("outer".into(), "page:1000002;field2;outer2;".into())
    );
    w.focus("first");
    assert_eq!(w.log(), "outer7;");
    w.tap(XK_ISO_LEFT_TAB, SHIFT);
    assert_eq!(
        (w.who(&names), w.log()),
        ("outer".into(), "first2;outer2;".into())
    );
    // Forwards it takes the focus itself; its ListView then takes `focus` by a binding on the
    // SwipeView's and gives it to the page's focus item, for no reason of its own.
    w.tap(XK_TAB, 0);
    assert_eq!(
        (w.who(&names), w.log()),
        ("first".into(), "outer1;first7;".into())
    );
    w.tap(XK_TAB, 0);
    assert_eq!(
        (w.who(&names), w.log()),
        ("field".into(), "first1;field1;".into())
    );
}

const CONTROLS: &str = r#"
import QtQuick
import QtQuick.Controls
Item {
    id: root
    width: 400; height: 400
    property string log: ""
    property var sl: sl
    property var slv: slv
    property var spin: spin
    property var menu: menu
    Slider { id: sl; x: 310; width: 80; from: 0; to: 100; value: 50; onMoved: root.log += "moved;" }
    Slider {
        id: slv; x: 310; y: 40; height: 80; orientation: Qt.Vertical
        from: 0; to: 100; value: 50; stepSize: 10
        onMoved: root.log += "vmoved;"
    }
    SpinBox {
        id: spin; x: 250; y: 160; from: 0; to: 6; value: 5; stepSize: 2
        onValueModified: root.log += "modified;"
    }
    Menu {
        id: menu
        MenuItem { text: "A"; onTriggered: root.log += "A;" }
        MenuItem { text: "B"; enabled: false; onTriggered: root.log += "B;" }
        MenuItem { text: "C"; onTriggered: root.log += "C;" }
    }
}
"#;

/// Finding 4: arrows step a Slider and a SpinBox; arrows pick a Menu's item and Space, Return
/// or Enter triggers it.
#[test]
fn finding_04_slider_spinbox_and_menu_keys() {
    let mut w = Win::new(CONTROLS);
    let sl = w.focus("sl");
    w.tap(XK_RIGHT, 0);
    assert_eq!((w.num(sl, "value"), w.log()), (50.1, "moved;".into()));
    w.tap(XK_UP, 0);
    assert_eq!(
        (w.num(sl, "value"), w.log()),
        (50.1, "".into()),
        "Up on a horizontal slider"
    );
    w.set(sl, "value", Value::Num(100.0));
    w.tap(XK_RIGHT, 0);
    assert_eq!(
        (w.num(sl, "value"), w.log()),
        (100.0, "".into()),
        "Right at the maximum"
    );
    let slv = w.focus("slv");
    w.tap(XK_UP, 0);
    assert_eq!((w.num(slv, "value"), w.log()), (60.0, "vmoved;".into()));
    w.tap(XK_RIGHT, 0);
    assert_eq!(
        (w.num(slv, "value"), w.log()),
        (60.0, "".into()),
        "Right on a vertical slider"
    );
    w.tap(XK_DOWN, 0);
    assert_eq!((w.num(slv, "value"), w.log()), (50.0, "vmoved;".into()));

    let spin = w.focus("spin");
    w.tap(XK_UP, 0);
    assert_eq!((w.num(spin, "value"), w.log()), (6.0, "modified;".into()));
    w.tap(XK_UP, 0);
    assert_eq!(
        (w.num(spin, "value"), w.log()),
        (6.0, "".into()),
        "Up at the maximum"
    );
    w.tap(XK_DOWN, 0);
    assert_eq!((w.num(spin, "value"), w.log()), (4.0, "modified;".into()));

    let menu = w.item("menu");
    let open = |w: &mut Win| {
        open_popup(&mut w.rt, menu);
        w.kb.sync(&mut w.rt);
        w.rt.settle();
    };
    open(&mut w);
    assert_eq!(
        w.num(menu, "currentIndex"),
        -1.0,
        "an opened menu has no current item"
    );
    let mut seen = Vec::new();
    for sym in [XK_UP, XK_DOWN, XK_UP, XK_DOWN, XK_DOWN, XK_UP, XK_DOWN] {
        w.tap(sym, 0);
        seen.push(w.num(menu, "currentIndex"));
    }
    // Up from none stays at none; both skip the disabled item; neither wraps.
    assert_eq!(seen, [-1.0, 0.0, 0.0, 2.0, 2.0, 0.0, 2.0]);
    w.tap(XK_SPACE, 0);
    assert_eq!((w.on(menu, "visible"), w.log()), (false, "C;".into()));
    open(&mut w);
    assert_eq!(w.num(menu, "currentIndex"), -1.0, "reopened");
    w.tap(XK_RETURN, 0);
    assert_eq!(
        (w.on(menu, "visible"), w.log()),
        (true, "".into()),
        "Return with no item"
    );
    w.tap(XK_DOWN, 0);
    w.tap(XK_KP_ENTER, 0);
    assert_eq!((w.on(menu, "visible"), w.log()), (false, "A;".into()));
    open(&mut w);
    w.tap(XK_DOWN, 0);
    w.tap(XK_RETURN, 0);
    assert_eq!((w.on(menu, "visible"), w.log()), (false, "A;".into()));
}

/// Where one wheel event of `angle` leaves a 200x100 Flickable over 2000x1000 content.
fn wheel(angle: (f64, f64)) -> (f64, f64) {
    let mut w = Win::new(
        r#"
import QtQuick
Item {
    id: root
    width: 400; height: 300
    property var fl: fl
    Flickable {
        id: fl; width: 200; height: 100; contentWidth: 2000; contentHeight: 1000
        Rectangle { width: 2000; height: 1000; color: "red" }
    }
}
"#,
    );
    let fl = w.item("fl");
    let mut p = Pointer::default();
    p.time = 1000.0;
    w.rt.tick(p.time);
    p.enter(&mut w.rt, w.root, 100.0, 50.0);
    p.motion(&mut w.rt, w.root, 100.0, 50.0);
    p.time += 50.0;
    w.rt.tick(p.time);
    p.wheel(&mut w.rt, angle, (0.0, 0.0), (p.time, p.time));
    while w.rt.tick(p.time) {
        p.time += 16.0;
    }
    w.rt.settle();
    (w.num(fl, "contentX"), w.num(fl, "contentY"))
}

/// Finding 5: a wheel's horizontal angle scrolls a Flickable sideways, by the same distance
/// per notch as the vertical one.
#[test]
fn finding_05_horizontal_wheel_scrolls_flickable() {
    assert_eq!(wheel((-120.0, 0.0)), (72.0, 0.0));
    assert_eq!(wheel((0.0, -120.0)), (0.0, 72.0));
    assert_eq!(wheel((-120.0, -120.0)), (72.0, 72.0));
    assert_eq!(wheel((-240.0, 0.0)), (144.0, 0.0));
}

/// Finding 6: Ctrl+Left, Ctrl+Right, Ctrl+Backspace and Ctrl+Delete stop at punctuation the
/// way QTextBoundaryFinder's words do.
#[test]
fn finding_06_ctrl_moves_by_qt_words() {
    for name in ["tf", "ta"] {
        let mut w = Win::new(EDITORS);
        let ed = w.focus(name);
        let at = |w: &mut Win, cursor: f64| {
            w.set(ed, "text", Value::Str("foo.bar/baz qux".into()));
            w.set(ed, "cursorPosition", Value::Num(cursor));
        };
        at(&mut w, 11.0);
        w.tap(XK_LEFT, CTRL);
        assert_eq!(w.num(ed, "cursorPosition"), 8.0, "{name} Ctrl+Left");
        at(&mut w, 0.0);
        w.tap(XK_RIGHT, CTRL);
        assert_eq!(w.num(ed, "cursorPosition"), 3.0, "{name} Ctrl+Right");
        at(&mut w, 11.0);
        w.tap(XK_BACKSPACE, CTRL);
        assert_eq!(
            state(&mut w, ed),
            st("foo.bar/ qux", 8.0, true, false),
            "{name}"
        );
        at(&mut w, 11.0);
        w.tap(XK_DELETE, CTRL);
        assert_eq!(
            state(&mut w, ed),
            st("foo.bar/bazqux", 11.0, true, false),
            "{name}"
        );
    }
}

/// Finding 8: keypad navigation, multimedia and system keysyms arrive as the Qt keys of
/// QXkbCommon's table.
#[test]
fn finding_08_keypad_and_media_keysyms() {
    let mut w = Win::new(
        r#"
import QtQuick
Item {
    id: root
    width: 100; height: 100
    property string log: ""
    property var it: it
    Item { id: it; focus: true; Keys.onPressed: event => { root.log = event.key.toString(16) } }
}
"#,
    );
    w.focus("it");
    let table: &[(&str, u32, u32)] = &[
        ("KP_Home", 0xff95, 0x0100_0010),
        ("KP_Left", 0xff96, 0x0100_0012),
        ("KP_Up", 0xff97, 0x0100_0013),
        ("KP_Right", 0xff98, 0x0100_0014),
        ("KP_Down", 0xff99, 0x0100_0015),
        ("KP_Prior", 0xff9a, 0x0100_0016),
        ("KP_Next", 0xff9b, 0x0100_0017),
        ("KP_End", 0xff9c, 0x0100_0011),
        ("KP_Insert", 0xff9e, 0x0100_0006),
        ("KP_Delete", 0xff9f, 0x0100_0007),
        ("KP_Enter", 0xff8d, 0x0100_0005),
        ("KP_5", 0xffb5, 0x35),
        ("ISO_Level3_Shift", 0xfe03, 0x0100_1103),
        ("XF86AudioLowerVolume", 0x1008_ff11, 0x0100_0070),
        ("XF86AudioMute", 0x1008_ff12, 0x0100_0071),
        ("XF86AudioRaiseVolume", 0x1008_ff13, 0x0100_0072),
        ("XF86AudioPlay", 0x1008_ff14, 0x0100_0080),
        ("XF86AudioNext", 0x1008_ff17, 0x0100_0083),
        ("Sys_Req", 0xff15, 0x0100_000a),
        ("Help", 0xff6a, 0x0100_0058),
    ];
    for &(name, sym, key) in table {
        w.tap(sym, 0);
        assert_eq!(w.log(), format!("{key:x}"), "{name}");
    }
}

/// Finding 9: a ListView's arrows wrap with keyNavigationWraps and do nothing without
/// keyNavigationEnabled, which follows `interactive`.
#[test]
fn finding_09_list_view_key_navigation_properties() {
    let mut w = Win::new(
        r#"
import QtQuick
Item {
    id: root
    width: 100; height: 300
    property var wraps: wraps
    property var off: off
    property var still: still
    property var plain: plain
    component Row: Item { width: 100; height: 20 }
    ListView { id: wraps; width: 100; height: 60; model: 3; keyNavigationWraps: true; delegate: Row {} }
    ListView { id: off; y: 70; width: 100; height: 60; model: 3; keyNavigationEnabled: false; delegate: Row {} }
    ListView { id: still; y: 140; width: 100; height: 60; model: 3; interactive: false; delegate: Row {} }
    ListView { id: plain; y: 210; width: 100; height: 60; model: 3; delegate: Row {} }
}
"#,
    );
    let mut step = |name: &str, from: f64, sym: u32| {
        let lv = w.focus(name);
        w.set(lv, "currentIndex", Value::Num(from));
        w.tap(sym, 0);
        w.num(lv, "currentIndex")
    };
    assert_eq!(step("wraps", 2.0, XK_DOWN), 0.0);
    assert_eq!(step("wraps", 0.0, XK_UP), 2.0);
    assert_eq!(step("off", 0.0, XK_DOWN), 0.0);
    assert_eq!(step("still", 0.0, XK_DOWN), 0.0);
    assert_eq!(step("plain", 0.0, XK_DOWN), 1.0);
    assert_eq!(step("plain", 2.0, XK_DOWN), 2.0);
}

/// Finding 10: mouse and wheel events carry the keyboard modifiers held.
#[test]
fn finding_10_pointer_events_carry_modifiers() {
    let mut w = Win::new(
        r#"
import QtQuick
Item {
    id: root
    width: 200; height: 200
    property string log: ""
    MouseArea {
        width: 200; height: 100
        onPressed: mouse => { root.log += "press:" + mouse.modifiers.toString(16) + ";" }
        onWheel: wheel => { root.log += "wheel:" + wheel.modifiers.toString(16) + ";" }
    }
    Item {
        y: 100; width: 200; height: 100
        WheelHandler { onWheel: event => { root.log += "handler:" + event.modifiers.toString(16) + ";" } }
    }
}
"#,
    );
    let mut p = Pointer::default();
    p.time = 1000.0;
    w.rt.tick(p.time);
    p.enter(&mut w.rt, w.root, 50.0, 50.0);
    p.motion(&mut w.rt, w.root, 50.0, 50.0);
    p.mods = CTRL;
    p.press(&mut w.rt, BTN_LEFT);
    p.release(&mut w.rt, BTN_LEFT);
    assert_eq!(w.log(), "press:4000000;");
    p.mods = SHIFT;
    p.wheel(&mut w.rt, (0.0, -120.0), (0.0, 0.0), (p.time, p.time));
    assert_eq!(w.log(), "wheel:2000000;");
    p.motion(&mut w.rt, w.root, 50.0, 150.0);
    p.mods = CTRL | SHIFT;
    p.wheel(&mut w.rt, (0.0, -120.0), (0.0, 0.0), (p.time, p.time));
    assert_eq!(w.log(), "handler:6000000;");
}
