//! The bar stays in the left sidebar's focus grab (the tree's GlobalFocusGrab.qml) on every page
//! of the sidebar. The grab is the sidebar's alone until an item in it has active focus, then
//! the persistent windows join, and the bar gets its pointer back (the brightness hint and wheel
//! at its start). The guide page shown while no assistant is configured has no item that takes
//! focus, so the bar stayed out for as long as the sidebar was open; the sidebar's background
//! holds it now.

use std::path::PathBuf;

use qml::runtime::Runtime;
use qml::value::Value;
use shell::keys::Keyboard;

const ESCAPE: u32 = 0xff1b;

const QML: &str = r#"
import QtQuick
import Rustshell
import ri
import ri.services
import ri.modules.common
import ri.modules.rusty.sidebarLeft
Scope {
    id: root
    property var bar: barWin
    property bool open: false
    onOpenChanged: GlobalStates.sidebarLeftOpen = open
    property bool chat: false
    onChatChanged: Config.options.sidebar.assistant.command = ["cat"]
    PanelWindow {
        id: barWin
        Component.onCompleted: GlobalFocusGrab.addPersistent(barWin)
    }
    SidebarLeft {}
}
"#;

fn tree() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../qml")
}

/// The items the tree's one HyprlandFocusGrab whitelists, and whether it is active.
fn grab(rt: &mut Runtime) -> (Vec<usize>, bool) {
    let mut w = rt.world.borrow_mut();
    let grabs = w.of_type("HyprlandFocusGrab");
    assert_eq!(grabs.len(), 1);
    let wins = match w.read(grabs[0], "windows") {
        Some(Value::Obj(o)) => o
            .borrow()
            .array
            .iter()
            .flatten()
            .filter_map(|x| match x {
                Value::Item(c) => Some(*c),
                _ => None,
            })
            .collect(),
        v => panic!("windows: {v:?}"),
    };
    let active = w.read(grabs[0], "active").is_some_and(|v| v.truthy());
    (wins, active)
}

struct Shell {
    rt: Runtime,
    kb: Keyboard,
    root: usize,
    bar: usize,
    sidebar: usize,
}

fn shell(chat: bool) -> Shell {
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
    let dir = std::env::temp_dir().join(format!("ri-focus-grab-bar-{}-{chat}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("shell.qml");
    std::fs::write(&f, QML).unwrap();
    let root = rt.load_file(&f).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    rt.settle();
    if chat {
        rt.set(root, "chat", Value::Bool(true));
        rt.settle();
    }
    let Value::Item(bar) = rt.get(root, "bar") else {
        panic!("no bar");
    };
    let sidebar = {
        let mut w = rt.world.borrow_mut();
        let wins: Vec<usize> = w
            .of_type("PanelWindow")
            .into_iter()
            .filter(|&i| i != bar)
            .collect();
        assert_eq!(wins.len(), 1, "sidebar windows: {wins:?}");
        wins[0]
    };
    let page = if chat {
        "AssistantChat"
    } else {
        "AssistantGuide"
    };
    assert_eq!(
        rt.world.borrow_mut().of_type(page).len(),
        1,
        "fixture: the {page} page"
    );
    Shell {
        rt,
        kb: Keyboard::default(),
        root,
        bar,
        sidebar,
    }
}

impl Shell {
    /// Opens the sidebar and lets Hyprland enter it, as it does a window that has a focus grab
    /// to itself.
    fn open(&mut self) {
        let (bar, sidebar) = (self.bar, self.sidebar);
        self.kb.sync(&mut self.rt);
        assert_eq!(grab(&mut self.rt), (vec![bar], false));
        self.rt.set(self.root, "open", Value::Bool(true));
        self.rt.settle();
        self.kb.sync(&mut self.rt);
        self.rt.settle();
        assert_eq!(grab(&mut self.rt), (vec![sidebar], true));
        self.kb.win = Some(sidebar);
        self.kb.sync(&mut self.rt);
        self.rt.settle();
    }
}

#[test]
fn bar_joins_the_grab_of_the_left_sidebar_on_its_guide_page() {
    let mut sh = shell(false);
    sh.open();
    assert_eq!(grab(&mut sh.rt), (vec![sh.sidebar, sh.bar], true));
    assert_eq!(sh.kb.active_editor(&sh.rt), None);

    // The keys reach the sidebar's own handler: Esc closes it.
    sh.kb.key(&mut sh.rt, ESCAPE, None, true, false);
    sh.kb.key(&mut sh.rt, ESCAPE, None, false, false);
    sh.rt.settle();
    assert!(!sh.rt.get(sh.sidebar, "visible").truthy());
    sh.kb.win = None;
    sh.kb.sync(&mut sh.rt);
    sh.rt.settle();
    assert_eq!(grab(&mut sh.rt), (vec![sh.bar], false));
}

/// The chat's message field takes the keys from the sidebar's background when it asks for them
/// (here through the page's `onFocusChanged`), and the bar stays in the grab.
#[test]
fn chat_page_moves_the_keys_to_its_message_field() {
    let mut sh = shell(true);
    sh.open();
    assert_eq!(grab(&mut sh.rt), (vec![sh.sidebar, sh.bar], true));
    let chat = sh.rt.world.borrow_mut().of_type("AssistantChat")[0];
    sh.rt.set(chat, "focus", Value::Bool(true));
    sh.rt.settle();
    sh.kb.sync(&mut sh.rt);
    sh.rt.settle();
    let field = sh.kb.active_editor(&sh.rt).expect("the message field");
    assert!(matches!(sh.rt.get(chat, "inputField"), Value::Item(i) if i == field));
    assert_eq!(grab(&mut sh.rt), (vec![sh.sidebar, sh.bar], true));
}
