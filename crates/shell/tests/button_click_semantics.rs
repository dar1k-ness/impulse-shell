//! The original's common buttons under the pointer, as Qt Quick's Button: `downAction` starts every
//! left press and `releaseAction` ends it, inside or outside the button (Button::pressed /
//! Button::released; the on-screen keyboard's keys press and release a key with them), while
//! the click (Button::clicked) needs the release inside: a press dragged off the button and
//! released elsewhere cancels it. The tray menu's entries act on the click.
#![cfg(feature = "interp")]

use qml::runtime::Runtime;
use qml::value::Value;
use shell::{input::Pointer, scene};
use std::path::PathBuf;

const BTN_LEFT: u32 = 0x110;
const W: f64 = 600.0;
const H: f64 = 400.0;

const QML: &str = r#"
import QtQuick
import Rustshell
import ri.modules.common.widgets
import ri.modules.rusty.bar
Item {
    id: root
    width: 600
    height: 400
    property string log: ""
    RippleButton {
        objectName: "ripple"
        x: 10; y: 10; width: 100; height: 40
        downAction: () => root.log += "down;"
        releaseAction: () => root.log += "release;"
        onClicked: root.log += "click;"
    }
    GroupButton {
        objectName: "group"
        x: 10; y: 60; width: 100; height: 40
        downAction: () => root.log += "down;"
        releaseAction: () => root.log += "release;"
        onClicked: root.log += "click;"
    }
    // The tray menu's entry: a QsMenuEntry, which only the engine makes (`T::new`).
    property QsMenuEntry entry: null
    Connections {
        target: root.entry
        function onTriggered() {
            root.log += "triggered;";
        }
    }
    Loader {
        x: 10; y: 110; width: 200; height: 36
        active: root.entry !== null
        sourceComponent: SysTrayMenuEntry {
            objectName: "entry"
            menuEntry: root.entry
            onDismiss: root.log += "dismiss;"
        }
    }
}
"#;

fn ref_ri() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../qml")
}

struct T {
    rt: Runtime,
    root: usize,
    ptr: Pointer,
    fonts: scene::Fonts,
    now: f64,
}

impl T {
    fn new() -> Self {
        let mut rt = Runtime::new(Some(ref_ri()));
        let dirs: Vec<PathBuf> = [
            "modules/common",
            "modules/common/functions",
            "modules/common/utils",
            "services",
            "",
        ]
        .iter()
        .map(|d| ref_ri().join(d))
        .collect();
        rt.load_singletons_lazy(&dirs);
        let dir = std::env::temp_dir().join(format!("ri-button-click-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f =
            dir.join(format!("t-{:?}.qml", std::thread::current().id()).replace(['(', ')'], ""));
        std::fs::write(&f, QML).unwrap();
        let root = rt.load_file(&f).unwrap();
        let _ = std::fs::remove_file(&f);
        rt.settle();
        {
            let mut w = rt.world.borrow_mut();
            let entry = qml::types::native_singleton(&mut w, "QsMenuEntry");
            for (name, ty, v) in [
                ("isSeparator", "bool", Value::Bool(false)),
                ("enabled", "bool", Value::Bool(true)),
                ("text", "string", Value::str("Quit")),
                ("icon", "string", Value::str("")),
                ("buttonType", "int", Value::Num(0.0)),
                ("checkState", "int", Value::Num(0.0)),
                ("hasChildren", "bool", Value::Bool(false)),
            ] {
                w.declare(entry, name, ty);
                w.write(entry, name, v);
            }
            w.write(root, "entry", Value::Item(entry));
        }
        rt.settle();
        assert!(rt.errors().is_empty(), "{:?}", rt.errors());
        let mut t = T {
            rt,
            root,
            ptr: Pointer::default(),
            fonts: scene::Fonts::default(),
            now: 1000.0,
        };
        t.frames(3);
        t
    }

    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.now += 16.0;
            self.rt.tick(self.now);
            self.rt.settle();
            scene::layout_window(&mut self.rt, &mut self.fonts, self.root, [W, H]);
            self.rt.settle();
        }
    }

    fn named(&self, name: &str) -> usize {
        let mut w = self.rt.world.borrow_mut();
        (0..w.items.len())
            .find(|&i| {
                !w.dead.contains(&i)
                    && w.read(i, "objectName")
                        .is_some_and(|v| v.to_js_string() == name)
            })
            .unwrap_or_else(|| panic!("no {name}"))
    }

    fn center(&self, item: usize) -> (f64, f64) {
        let mut w = self.rt.world.borrow_mut();
        let (x, y) = w.scene_pos(item, Some(self.root));
        let wd = w.read(item, "width").unwrap_or_default().to_number();
        let ht = w.read(item, "height").unwrap_or_default().to_number();
        (x + wd / 2.0, y + ht / 2.0)
    }

    /// A left press at `from`, moved in steps to `to` and released there.
    fn drag(&mut self, from: (f64, f64), to: (f64, f64)) {
        let win = self.root;
        self.ptr.enter(&mut self.rt, win, from.0, from.1);
        self.ptr.motion(&mut self.rt, win, from.0, from.1);
        self.ptr.press(&mut self.rt, BTN_LEFT);
        self.frames(1);
        for k in 1..=8 {
            let t = k as f64 / 8.0;
            let (x, y) = (from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t);
            self.ptr.motion(&mut self.rt, win, x, y);
            self.frames(1);
        }
        self.ptr.release(&mut self.rt, BTN_LEFT);
        self.frames(2);
    }

    fn take_log(&mut self) -> String {
        let s = self
            .rt
            .world
            .borrow_mut()
            .read(self.root, "log")
            .unwrap_or(Value::Undefined)
            .to_js_string();
        self.rt
            .world
            .borrow_mut()
            .write(self.root, "log", Value::str(""));
        s
    }

    fn down(&self, item: usize) -> bool {
        self.rt
            .world
            .borrow_mut()
            .read(item, "down")
            .is_some_and(|v| v.truthy())
    }
}

/// RippleButton (and every button built on it: toolbar buttons, the on-screen keyboard's keys)
/// and GroupButton: a tap presses, releases and clicks once; a press dragged 300 px off is
/// released but not clicked.
#[test]
fn a_press_released_outside_a_button_releases_it_without_a_click() {
    let mut t = T::new();
    for name in ["ripple", "group"] {
        let b = t.named(name);
        let at = t.center(b);
        t.drag(at, at);
        assert_eq!(t.take_log(), "down;release;click;", "{name} tapped");
        t.drag(at, (at.0 + 300.0, at.1 + 200.0));
        assert_eq!(t.take_log(), "down;release;", "{name} dragged off");
        assert!(!t.down(b), "{name} stays down");
        t.drag(at, (at.0 + 20.0, at.1 + 5.0));
        assert_eq!(
            t.take_log(),
            "down;release;click;",
            "{name} moved within itself"
        );
    }
    assert!(t.rt.errors().is_empty(), "{:?}", t.rt.errors());
}

/// A tray menu entry triggers and dismisses the menu on a click only.
#[test]
fn a_tray_menu_entry_triggers_only_on_a_click() {
    let mut t = T::new();
    let e = t.named("entry");
    let at = t.center(e);
    t.drag(at, (at.0, at.1 + 300.0));
    assert_eq!(t.take_log(), "", "dragged off");
    assert!(!t.down(e));
    t.drag(at, at);
    assert_eq!(t.take_log(), "triggered;dismiss;", "tapped");
    assert!(t.rt.errors().is_empty(), "{:?}", t.rt.errors());
}
