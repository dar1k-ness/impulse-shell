//! qml's desktop right-click menu (DesktopLayoutContent.qml + DesktopMenu.qml), as Plasma's:
//! a right-click on the bare desktop opens it at the pointer, its "Enter edit mode" starts
//! edit mode in this process (no IPC client, nothing looked up in PATH) and closes it, and
//! while editing a right-click opens nothing.
#![cfg(feature = "interp")]

use qml::runtime::Runtime;
use qml::value::Value;
use shell::{input::Pointer, scene};
use std::path::{Path, PathBuf};

const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;

const LAYOUT: &str = r#"version = 5

[[monitor]]
connector = "DP-1"
primary = true

[[monitor.container]]
id = "desktop"
kind = "desktop"
"#;

fn tree() -> PathBuf {
    let ri = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../qml")
        .canonicalize()
        .unwrap();
    let d = std::env::temp_dir().join(format!("desktop-menu-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    for e in std::fs::read_dir(&ri).unwrap() {
        let e = e.unwrap();
        std::os::unix::fs::symlink(e.path(), d.join(e.file_name())).unwrap();
    }
    std::fs::write(d.join("layout.toml"), LAYOUT).unwrap();
    std::fs::write(
        d.join("main.qml"),
        "import QtQuick\nimport Impulse\nimport ri.modules.rusty.background\nItem {\n    width: 1920\n    height: 1080\n    \
         property var ctl: controller\n    EditController { id: controller }\n    \
         DesktopLayoutContent { anchors.fill: parent; connector: \"DP-1\" }\n}\n",
    )
    .unwrap();
    d
}

fn menus(rt: &Runtime) -> Vec<usize> {
    let mut w = rt.world.borrow_mut();
    let all: Vec<usize> = (0..w.items.len())
        .filter(|&i| !w.dead.contains(&i) && w.items[i].types.iter().any(|t| t == "DesktopMenu"))
        .collect();
    all.into_iter()
        .filter(|&i| w.read(i, "visible").is_some_and(|v| v.truthy()))
        .collect()
}

#[test]
fn right_click_opens_the_menu_and_its_entry_starts_edit_mode() {
    let d = tree();
    let mut rt = Runtime::new(Some(d.clone()));
    rt.load_singletons(&[
        d.join("modules/common"),
        d.join("modules/common/functions"),
        d.join("services"),
    ]);
    rt.world.borrow_mut().open_layout(d.join("layout.toml"));
    rt.settle();
    let main = rt.load_file(&d.join("main.qml")).unwrap();
    rt.settle();
    let mut fonts = scene::Fonts::default();
    scene::layout_window(&mut rt, &mut fonts, main, [1920.0, 1080.0]);
    let ctl = match rt.world.borrow_mut().read(main, "ctl") {
        Some(Value::Item(i)) => i,
        v => panic!("ctl {v:?}"),
    };
    let active = |rt: &Runtime| {
        rt.world
            .borrow_mut()
            .read(ctl, "active")
            .is_some_and(|v| v.truthy())
    };
    assert!(menus(&rt).is_empty());

    let mut ptr = Pointer::default();
    let click = |rt: &mut Runtime, ptr: &mut Pointer, win: usize, x: f64, y: f64, b: u32| {
        ptr.enter(rt, win, x, y);
        ptr.motion(rt, win, x, y);
        ptr.press(rt, b);
        ptr.release(rt, b);
        rt.settle();
    };
    // A left-click on the desktop opens nothing.
    click(&mut rt, &mut ptr, main, 700.0, 400.0, BTN_LEFT);
    assert!(menus(&rt).is_empty());
    click(&mut rt, &mut ptr, main, 700.0, 400.0, BTN_RIGHT);
    let opened = menus(&rt);
    assert_eq!(opened.len(), 1, "one menu");
    let menu = opened[0];
    {
        let mut w = rt.world.borrow_mut();
        assert_eq!(w.read(menu, "anchor.rect.x").unwrap().to_number(), 700.0);
        assert_eq!(w.read(menu, "anchor.rect.y").unwrap().to_number(), 400.0);
    }
    assert!(!active(&rt));

    // Its first entry, "Enter edit mode", clicked as the popup surface gets it. The text is
    // measured on the first layout, which gives the popup its size.
    scene::layout_window(&mut rt, &mut fonts, menu, [400.0, 400.0]);
    let size = {
        let mut w = rt.world.borrow_mut();
        [
            w.read(menu, "implicitWidth").unwrap().to_number(),
            w.read(menu, "implicitHeight").unwrap().to_number(),
        ]
    };
    assert!(size[0] > 50.0 && size[1] > 50.0, "{size:?}");
    scene::layout_window(&mut rt, &mut fonts, menu, size);
    let (x, y, label) = {
        let mut w = rt.world.borrow_mut();
        let entry = (0..w.items.len())
            .find(|&i| {
                !w.dead.contains(&i) && w.items[i].types.iter().any(|t| t == "DesktopMenuEntry")
            })
            .expect("no entry");
        let label = w.read(entry, "label").unwrap().to_js_string();
        let (x, y) = w.scene_pos(entry, Some(menu));
        (x, y, label)
    };
    assert_eq!(label, "Enter edit mode");
    click(&mut rt, &mut ptr, menu, x + 20.0, y + 10.0, BTN_LEFT);
    assert!(active(&rt), "edit mode started");
    assert!(menus(&rt).is_empty(), "the menu closed");

    // While editing, a right-click on the desktop opens no menu.
    click(&mut rt, &mut ptr, main, 300.0, 300.0, BTN_RIGHT);
    assert!(menus(&rt).is_empty());
    std::fs::remove_dir_all(&d).unwrap();
}
