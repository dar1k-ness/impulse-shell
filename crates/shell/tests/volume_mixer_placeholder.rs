//! The tree's volume mixer with no applications playing: the "No applications" `PagePlaceholder`
//! is declared in the mixer's ListView (`VolumeDialogContent.qml`, `leftMargin: 20`), so it is
//! a child of the view, not of its contentItem, and `anchors.fill: parent` covers the whole
//! view. The placeholder is centred in the list as in the original, not 20 px to its left.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::scene;
use std::path::{Path, PathBuf};

const SIZE: [f64; 2] = [360.0, 400.0];

fn tree() -> PathBuf {
    let ri = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../qml")
        .canonicalize()
        .unwrap();
    let d = std::env::temp_dir().join(format!("volume-mixer-placeholder-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    for e in std::fs::read_dir(&ri).unwrap() {
        let e = e.unwrap();
        std::os::unix::fs::symlink(e.path(), d.join(e.file_name())).unwrap();
    }
    std::fs::write(
        d.join("main.qml"),
        format!(
            "import QtQuick\nimport ri.modules.rusty.sidebarRight.volumeMixer\nItem {{\n    \
             width: {}\n    height: {}\n    \
             VolumeDialogContent {{ anchors.fill: parent; isSink: true }}\n}}\n",
            SIZE[0], SIZE[1]
        ),
    )
    .unwrap();
    d
}

#[test]
fn no_applications_placeholder_is_centred_in_the_list() {
    let d = tree();
    let mut rt = Runtime::new(Some(d.clone()));
    rt.load_singletons(&[
        d.join("modules/common"),
        d.join("modules/common/functions"),
        d.join("services"),
    ]);
    rt.settle();
    let main = rt.load_file(&d.join("main.qml")).unwrap();
    rt.settle();
    let mut fonts = scene::Fonts::default();
    let mut now = 1000.0;
    for _ in 0..60 {
        now += 16.0;
        rt.tick(now);
        rt.settle();
        scene::layout_window(&mut rt, &mut fonts, main, SIZE);
        rt.settle();
    }
    let mut w = rt.world.borrow_mut();
    let holder = (0..w.items.len())
        .find(|&i| !w.dead.contains(&i) && w.items[i].types.iter().any(|t| t == "PagePlaceholder"))
        .expect("no placeholder");
    let num = |w: &mut qml::runtime::World, i: usize, k: &str| {
        w.read(i, k).unwrap_or(Value::Num(f64::NAN)).to_number()
    };
    let list = w.items[holder].parent.unwrap();
    assert!(w.items[list].types.iter().any(|t| t == "ListView"));
    let shown = num(&mut w, holder, "opacity");
    let margin = num(&mut w, list, "leftMargin");
    let rect = [
        num(&mut w, holder, "x"),
        num(&mut w, holder, "width"),
        num(&mut w, list, "width"),
    ];
    drop(w);
    std::fs::remove_dir_all(&d).unwrap();
    assert_eq!(
        (shown, margin),
        (1.0, 20.0),
        "placeholder shown, list margin"
    );
    // x in the list, width, the list's width.
    assert_eq!(rect, [0.0, rect[2], rect[2]]);
    assert!(rect[2] > 2.0 * margin, "list width {}", rect[2]);
}
