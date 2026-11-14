//! Esc in the bar's system-tray menu (the tree's SysTrayMenu.qml, a PopupWindow over the bar that
//! SysTray.qml whitelists in a HyprlandFocusGrab): the keys reach the menu even when Hyprland
//! enters the bar, Esc in a submenu goes back one level, and Esc at the top closes the menu.

use std::path::PathBuf;

use qml::runtime::Runtime;
use qml::value::Value;
use shell::keys::{KbdWin, Keyboard, kbd_target};

const ESCAPE: u32 = 0xff1b;

/// SysTrayItem's menu Loader, without the tray service behind it.
const QML: &str = r#"
import QtQuick
import ri.modules.rusty.bar
Item {
    id: bar
    width: 400; height: 40
    property string log: ""
    property var menu: loader.item
    property bool menuActive: loader.active
    property var page: p
    property var stack: null
    property bool opened: false
    property bool submenu: false
    onOpenedChanged: if (opened) loader.active = true
    onSubmenuChanged: if (submenu) stack.push(page)
    Item { id: p; implicitWidth: 40; implicitHeight: 40 }
    Loader {
        id: loader
        active: false
        sourceComponent: SysTrayMenu {
            Component.onCompleted: this.open();
            trayItemMenuHandle: null
            onMenuClosed: {
                bar.log += "closed;";
                loader.active = false;
            }
        }
    }
}
"#;

fn tree() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../qml")
}

fn item(rt: &mut Runtime, i: usize, n: &str) -> usize {
    match rt.world.borrow_mut().read(i, n) {
        Some(Value::Item(c)) => c,
        v => panic!("{n}: {v:?}"),
    }
}

fn read(rt: &mut Runtime, i: usize, n: &str) -> String {
    let mut w = rt.world.borrow_mut();
    w.read(i, n).map(|v| v.to_js_string()).unwrap_or_default()
}

fn tap(rt: &mut Runtime, kb: &mut Keyboard, sym: u32) {
    kb.key(rt, sym, None, true, false);
    kb.key(rt, sym, None, false, false);
    rt.settle();
}

struct Tray {
    rt: Runtime,
    kb: Keyboard,
    bar: usize,
    menu: usize,
    stack: usize,
}

impl Tray {
    fn new() -> Self {
        let mut rt = Runtime::new(Some(tree()));
        let dirs: Vec<PathBuf> = [
            "modules/common",
            "modules/common/functions",
            "modules/common/utils",
            "services",
            "",
        ]
        .iter()
        .map(|d| tree().join(d))
        .collect();
        rt.load_singletons_lazy(&dirs);
        let dir = std::env::temp_dir().join(format!("ri-tray-esc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f =
            dir.join(format!("bar-{:?}.qml", std::thread::current().id()).replace(['(', ')'], ""));
        std::fs::write(&f, QML).unwrap();
        let bar = rt.load_file(&f).unwrap();
        let _ = std::fs::remove_file(&f);
        assert!(rt.errors().is_empty(), "{:?}", rt.errors());
        rt.settle();
        Tray {
            rt,
            kb: Keyboard::default(),
            bar,
            menu: 0,
            stack: 0,
        }
    }

    /// The StackView holding the open menu's levels.
    fn find_stack(&mut self) {
        let menu = self.menu;
        let mut w = self.rt.world.borrow_mut();
        let stacks = w.of_type("StackView");
        let under = |mut i: usize| loop {
            if i == menu {
                return true;
            }
            match w.items[i].parent {
                Some(p) => i = p,
                None => return false,
            }
        };
        let found = stacks.iter().copied().find(|&s| under(s));
        drop(w);
        self.stack =
            found.unwrap_or_else(|| panic!("no StackView under the menu; {:?}", self.rt.errors()));
    }

    /// Opens the menu the way SysTrayItem does and lets keys follow the compositor's focus:
    /// Hyprland enters whichever whitelisted surface of the grab it picks first, here the bar.
    fn open(&mut self) {
        self.rt.set(self.bar, "opened", Value::Bool(true));
        self.rt.settle();
        self.menu = item(&mut self.rt, self.bar, "menu");
        self.find_stack();
        self.rt.set(self.bar, "stack", Value::Item(self.stack));
        let wins = [
            KbdWin {
                configured: true,
                mode: 0,
                toplevel: false,
                grabbed: false,
            },
            KbdWin {
                configured: true,
                mode: 0,
                toplevel: false,
                grabbed: true,
            },
        ];
        let items = [self.bar, self.menu];
        self.kb.win = kbd_target(Some(0), &wins).map(|k| items[k]);
        self.kb.sync(&mut self.rt);
        self.rt.settle();
    }

    fn esc(&mut self) {
        tap(&mut self.rt, &mut self.kb, ESCAPE);
    }

    fn visible(&mut self) -> bool {
        read(&mut self.rt, self.bar, "menuActive") == "true"
            && read(&mut self.rt, self.menu, "visible") == "true"
    }

    fn depth(&mut self) -> String {
        read(&mut self.rt, self.stack, "depth")
    }

    fn take_log(&mut self) -> String {
        let s = read(&mut self.rt, self.bar, "log");
        self.rt
            .world
            .borrow_mut()
            .write(self.bar, "log", Value::str(""));
        s
    }
}

#[test]
fn esc_closes_the_tray_menu_entered_through_its_bar() {
    let mut t = Tray::new();
    t.open();
    assert_eq!(
        t.kb.win,
        Some(t.menu),
        "keys stay on the bar under the focus grab"
    );
    assert!(t.visible());
    t.esc();
    assert!(!t.visible(), "Esc left the tray menu open");
    assert_eq!(t.take_log(), "closed;");
    assert!(t.rt.errors().is_empty(), "{:?}", t.rt.errors());
}

#[test]
fn esc_in_a_submenu_goes_back_one_level() {
    let mut t = Tray::new();
    t.open();
    t.rt.set(t.bar, "submenu", Value::Bool(true));
    t.rt.settle();
    assert_eq!(t.depth(), "2");
    t.esc();
    assert_eq!(t.depth(), "1", "Esc in a submenu did not go back");
    assert!(t.visible(), "Esc in a submenu closed the whole menu");
    assert_eq!(t.take_log(), "");
    t.esc();
    assert!(!t.visible());
    assert_eq!(t.take_log(), "closed;");
}
