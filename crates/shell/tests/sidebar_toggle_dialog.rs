//! The tree's SidebarRightContent: a quick toggle's right-click (`openAudioOutputDialog` and the
//! rest) shows its detail dialog: the ToggleDialog Loader has its item when activeChanged runs,
//! and the dialog it shows there slides and fades in (WindowDialog's Behaviors), as in Qt.

use qml::runtime::Runtime;
use qml::value::Value;
use std::path::{Path, PathBuf};

const MAIN: &str = r#"import QtQuick
import ri.modules.rusty.sidebarRight
Item {
    width: 460
    height: 1000
    property alias content: content
    SidebarRightContent { id: content; anchors.fill: parent }
}
"#;

fn tree(tag: &str) -> PathBuf {
    let ri = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../qml")
        .canonicalize()
        .unwrap();
    let d = std::env::temp_dir().join(format!(
        "sidebar-toggle-dialog-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    for e in std::fs::read_dir(&ri).unwrap() {
        let e = e.unwrap();
        std::os::unix::fs::symlink(e.path(), d.join(e.file_name())).unwrap();
    }
    std::fs::write(d.join("zzmain.qml"), MAIN).unwrap();
    d
}

fn dialog_shown(compiled: bool, prop: &str) -> (bool, Vec<String>) {
    let d = tree(&format!("{compiled}-{prop}"));
    let mut rt = Runtime::new(Some(d.clone()));
    if compiled {
        let root = PathBuf::from(ricompiled::ROOT);
        rt.eng()
            .set_compiled(Some(std::rc::Rc::new(ricompiled::Compiled::new(&root))));
    }
    rt.load_singletons(&[
        d.clone(),
        d.join("modules/common"),
        d.join("modules/common/functions"),
        d.join("services"),
    ]);
    rt.settle();
    let main = rt.load_file(&d.join("zzmain.qml")).unwrap();
    rt.settle();
    let Some(Value::Item(c)) = rt.world.borrow_mut().read(main, "content") else {
        panic!("no content")
    };
    let before = rt.errors().len();
    rt.set(c, prop, Value::Bool(true));
    rt.settle();
    let errs = rt.errors()[before..].to_vec();
    let mut w = rt.world.borrow_mut();
    let n = w.items.len();
    let mut shown = false;
    for i in 0..n {
        if w.dead.contains(&i) || !w.items[i].types.iter().any(|t| t == "Loader") {
            continue;
        }
        if w.read(i, "shownPropertyString")
            .unwrap_or_default()
            .to_js_string()
            != prop
        {
            continue;
        }
        if let Some(Value::Item(it)) = w.read(i, "item") {
            shown = w.read(it, "show").unwrap_or_default().truthy();
        }
    }
    let _ = std::fs::remove_dir_all(&d);
    (shown, errs)
}

const DIALOGS: [&str; 5] = [
    "showAudioOutputDialog",
    "showAudioInputDialog",
    "showBluetoothDialog",
    "showNightLightDialog",
    "showWifiDialog",
];

fn check(compiled: bool) {
    for prop in DIALOGS {
        let (shown, errs) = dialog_shown(compiled, prop);
        assert!(
            !errs
                .iter()
                .any(|e| e.contains("ToggleDialog/Loader.onActiveChanged")),
            "{prop}: {errs:?}"
        );
        assert!(shown, "{prop}: dialog not shown ({errs:?})");
    }
}

#[test]
fn toggle_dialogs_show_interpreted() {
    check(false);
}

#[test]
fn toggle_dialogs_show_compiled() {
    check(true);
}

/// The scrim's alpha and the dialog background's implicitHeight at the frame that opens the
/// dialog and 50 and 400 ms after it.
fn opening(compiled: bool, prop: &str) -> [(f64, f64); 3] {
    let d = tree(&format!("open-{compiled}-{prop}"));
    let mut rt = Runtime::new(Some(d.clone()));
    if compiled {
        let root = PathBuf::from(ricompiled::ROOT);
        rt.eng()
            .set_compiled(Some(std::rc::Rc::new(ricompiled::Compiled::new(&root))));
    }
    rt.load_singletons(&[
        d.clone(),
        d.join("modules/common"),
        d.join("modules/common/functions"),
        d.join("services"),
    ]);
    rt.world.borrow_mut().track_fresh = true;
    let fonts = std::rc::Rc::new(std::cell::RefCell::new(shell::scene::Fonts::default()));
    shell::scene::loaded_layout_hook(&mut rt.world.borrow_mut(), &fonts);
    let lay = |rt: &mut Runtime, main: usize| {
        shell::scene::layout_window(rt, &mut fonts.borrow_mut(), main, [460.0, 1000.0]);
        rt.settle();
    };
    rt.settle();
    let main = rt.load_file(&d.join("zzmain.qml")).unwrap();
    rt.settle();
    lay(&mut rt, main);
    for t in [0.0, 1000.0] {
        rt.tick(t);
        lay(&mut rt, main);
    }
    let Some(Value::Item(c)) = rt.world.borrow_mut().read(main, "content") else {
        panic!("no content")
    };
    rt.set(c, prop, Value::Bool(true));
    rt.settle();
    let dialog = {
        let mut w = rt.world.borrow_mut();
        (0..w.items.len())
            .find_map(|i| {
                if w.dead.contains(&i) || !w.items[i].types.iter().any(|t| t == "Loader") {
                    return None;
                }
                let name = w.read(i, "shownPropertyString")?.to_js_string();
                match w.read(i, "item") {
                    Some(Value::Item(it)) if name == prop => Some(it),
                    _ => None,
                }
            })
            .expect("the dialog")
    };
    let out = [1000.0, 1050.0, 1400.0].map(|t| {
        rt.tick(t);
        rt.settle();
        lay(&mut rt, main);
        let mut w = rt.world.borrow_mut();
        let Some(Value::Color(scrim)) = w.read(dialog, "color") else {
            panic!("no scrim colour")
        };
        let kids = w.items[dialog].children.clone();
        let bg = (kids.into_iter())
            .find(|&k| w.items[k].types.iter().any(|t| t == "Rectangle"))
            .expect("the dialog's background");
        let h = w.read(bg, "implicitHeight").unwrap_or_default().to_number();
        (f64::from(scrim.a), h)
    });
    let _ = std::fs::remove_dir_all(&d);
    out
}

fn check_opening(compiled: bool) {
    for prop in DIALOGS {
        // The layout's caches are per thread and keyed by item: one runtime a thread.
        let [start, mid, end] = std::thread::spawn(move || opening(compiled, prop))
            .join()
            .unwrap();
        assert_eq!(start, (0.0, 0.0), "{prop}: opens at its start");
        assert!(
            mid.0 > 0.0 && mid.0 < end.0,
            "{prop}: scrim {mid:?} {end:?}"
        );
        assert!(
            mid.1 > 0.0 && mid.1 < end.1,
            "{prop}: height {mid:?} {end:?}"
        );
    }
}

#[test]
fn toggle_dialogs_open_animated_interpreted() {
    check_opening(false);
}

#[test]
fn toggle_dialogs_open_animated_compiled() {
    check_opening(true);
}
