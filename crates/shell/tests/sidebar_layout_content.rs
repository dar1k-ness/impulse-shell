//! The tree's `SidebarLayoutContent`: sections keep their natural height packed to the top, and
//! only a block without an implicit height (notifications) takes the height left over; sections
//! taller than the sidebar are hidden from the bottom up, the filling block keeping its minimum
//! height. The original's quick sliders show as the original's settings say, live.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::scene;
use std::path::{Path, PathBuf};

const MANIFEST: &str = r##"
[[widget]]
type = "block"
name = "Block"
icon = "calendar_month"
placements = ["sidebar"]
component = "testwidgets/Block.qml"
size = { min = [1, 1], default = [4, 1] }

[[widget.setting]]
key = "h"
kind = "int"
default = 120

[[widget]]
type = "filler"
name = "Filler"
icon = "notifications"
placements = ["sidebar"]
component = "testwidgets/Filler.qml"
size = { min = [1, 1], default = [4, 1] }

[[widget]]
type = "sliders"
name = "Sliders"
icon = "tune"
placements = ["sidebar"]
component = "modules/rusty/sidebarRight/widgets/SlidersWidget.qml"
size = { min = [1, 1], default = [5, 1] }
"##;

const MAIN: &str = r#"import QtQuick
import ri.modules.rusty.sidebarRight
Item {
    width: 400
    height: 1000
    property alias content: content
    SidebarLayoutContent { id: content; anchors.fill: parent; connector: "DP-1" }
}
"#;

fn layout(widgets: &[(&str, i32)]) -> String {
    let mut s = String::from(
        "version = 4\n[[monitor]]\nconnector = \"DP-1\"\nprimary = true\n\n[[monitor.container]]\nid = \"right\"\nkind = \"sidebar\"\nside = \"right\"\ncolumns = 4\n",
    );
    for (i, (ty, h)) in widgets.iter().enumerate() {
        s += &format!(
            "\n[[monitor.container.widget]]\nid = \"w{i}\"\ntype = \"{ty}\"\nsize = 4\nsettings = {{ h = {h} }}\n"
        );
    }
    s
}

/// The tree with its own widgets.toml swapped for two test blocks.
fn tree(tag: &str, layout: &str) -> PathBuf {
    let ri = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../qml")
        .canonicalize()
        .unwrap();
    let d = std::env::temp_dir().join(format!(
        "sidebar-layout-content-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("testwidgets")).unwrap();
    for e in std::fs::read_dir(&ri).unwrap() {
        let e = e.unwrap();
        if e.file_name() != "widgets.toml" {
            std::os::unix::fs::symlink(e.path(), d.join(e.file_name())).unwrap();
        }
    }
    std::fs::write(d.join("widgets.toml"), MANIFEST).unwrap();
    std::fs::write(
        d.join("testwidgets/Block.qml"),
        "import QtQuick\nItem { property int h: 0; implicitHeight: h }\n",
    )
    .unwrap();
    std::fs::write(
        d.join("testwidgets/Filler.qml"),
        "import QtQuick\nItem {}\n",
    )
    .unwrap();
    std::fs::write(d.join("layout.toml"), layout).unwrap();
    std::fs::write(d.join("main.qml"), MAIN).unwrap();
    d
}

/// The tree `d` loaded, with its layout open.
fn start(d: &Path) -> (Runtime, usize) {
    let mut rt = Runtime::new(Some(d.to_path_buf()));
    rt.load_singletons(&[
        d.to_path_buf(),
        d.join("modules/common"),
        d.join("modules/common/functions"),
        d.join("services"),
    ]);
    rt.world.borrow_mut().open_layout(d.join("layout.toml"));
    rt.settle();
    let main = rt.load_file(&d.join("main.qml")).unwrap();
    rt.settle();
    (rt, main)
}

/// `(y, height)` of each visible child of the content column, in order, once laid out.
fn laid_out(rt: &mut Runtime, main: usize) -> Vec<(f64, f64)> {
    let mut fonts = scene::Fonts::default();
    for _ in 0..4 {
        scene::layout_window(rt, &mut fonts, main, [400.0, 1000.0]);
        rt.settle();
    }
    let mut w = rt.world.borrow_mut();
    let Some(Value::Item(c)) = w.read(main, "content") else {
        panic!("no content")
    };
    let Some(Value::Item(c)) = w.read(c, "column") else {
        panic!("no column")
    };
    let kids = w.items[c].children.clone();
    let mut out = vec![];
    for k in kids {
        let t = &w.items[k].types;
        let shown = t.iter().any(|t| t == "Rectangle") || t.first().is_some_and(|t| t == "Item");
        if shown && w.read(k, "visible").unwrap_or_default().to_js_string() == "true" {
            let y = w.read(k, "y").unwrap_or_default().to_number();
            out.push((y, w.read(k, "height").unwrap_or_default().to_number()));
        }
    }
    out
}

/// `(y, height)` of each visible child of the content column, in order.
fn rows(widgets: &[(&str, i32)]) -> Vec<(f64, f64)> {
    let d = tree(
        &widgets
            .iter()
            .map(|w| format!("{}{}", w.0, w.1))
            .collect::<Vec<_>>()
            .join("-"),
        &layout(widgets),
    );
    let (mut rt, main) = start(&d);
    laid_out(&mut rt, main)
}

/// The original's setting `sidebar.quickSliders.<key>`, as its settings window writes it.
fn set_quick_sliders(rt: &mut Runtime, key: &str, on: bool) {
    let config = *rt.world.borrow().singletons.get("Config").expect("Config");
    {
        let mut wd = rt.world.borrow_mut();
        let mut o = config;
        for k in ["options", "sidebar", "quickSliders"] {
            o = match wd.read(o, k) {
                Some(Value::Item(i)) => i,
                v => panic!("{k}: {v:?}"),
            };
        }
        wd.write(o, key, Value::Bool(on));
    }
    rt.settle();
}

/// The sliders in the layout follow the original's settings (Settings > Interface > Sliders) as they
/// change: off (the original's default) they take no place, on they show the sliders chosen, and with
/// none chosen they are gone again, as in the original's own sidebar.
#[test]
fn the_sliders_follow_the_original_s_settings_live() {
    let layout = "version = 4\n[[monitor]]\nconnector = \"DP-1\"\nprimary = true\n\n\
        [[monitor.container]]\nid = \"right\"\nkind = \"sidebar\"\nside = \"right\"\ncolumns = 4\n\n\
        [[monitor.container.widget]]\nid = \"sliders\"\ntype = \"sliders\"\nsize = 4\n\n\
        [[monitor.container.widget]]\nid = \"block\"\ntype = \"block\"\nsize = 4\nsettings = { h = 120 }\n";
    let d = tree("sliders", layout);
    let (mut rt, main) = start(&d);
    // The block alone; what is left of the 1000 stays empty below it.
    let without = vec![(0.0, 120.0)];
    assert_eq!(laid_out(&mut rt, main), without, "sliders off");

    // The sliders above the block, 10 between them: `[sliders, block]`, the sliders' height
    // returned.
    let with = |rt: &mut Runtime, what: &str| {
        let rows = laid_out(rt, main);
        assert_eq!(rows.len(), 2, "{what}: {rows:?}");
        let h = rows[0].1;
        assert!(h > 0.0, "{what}: {rows:?}");
        assert_eq!(rows, vec![(0.0, h), (h + 10.0, 120.0)], "{what}");
        h
    };
    set_quick_sliders(&mut rt, "enable", true);
    let two = with(&mut rt, "brightness and volume");
    set_quick_sliders(&mut rt, "showVolume", false);
    let one = with(&mut rt, "brightness");
    assert!(one < two, "one slider {one}, two {two}");
    set_quick_sliders(&mut rt, "showMic", true);
    assert_eq!(with(&mut rt, "brightness and microphone"), two);
    set_quick_sliders(&mut rt, "showVolume", true);
    let three = with(&mut rt, "all three");
    assert!(two < three, "two sliders {two}, three {three}");

    for k in ["showBrightness", "showVolume", "showMic"] {
        set_quick_sliders(&mut rt, k, false);
    }
    assert_eq!(laid_out(&mut rt, main), without, "no slider chosen");
    set_quick_sliders(&mut rt, "showVolume", true);
    assert_eq!(with(&mut rt, "volume"), one);
    set_quick_sliders(&mut rt, "enable", false);
    assert_eq!(laid_out(&mut rt, main), without, "sliders off again");
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    drop(rt);
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn without_a_filling_block_sections_pack_to_the_top() {
    // 1000 tall, 10 between sections: what is left stays empty below them.
    assert_eq!(
        rows(&[("block", 120), ("block", 80)]),
        vec![(0.0, 120.0), (130.0, 80.0)]
    );
}

#[test]
fn a_filling_block_takes_the_height_left_over() {
    assert_eq!(
        rows(&[("block", 120), ("filler", 0), ("block", 80)]),
        vec![(0.0, 120.0), (130.0, 780.0), (920.0, 80.0)]
    );
}

#[test]
fn sections_taller_than_the_sidebar_hide_from_the_bottom_and_the_filling_block_keeps_the_rest() {
    // 600 + 80 (fill minimum) + 400 + 2 * 10 = 1100 > 1000: the last block has no room and is
    // hidden; the filling block takes what is left, nothing reaches past the sidebar.
    assert_eq!(
        rows(&[("block", 600), ("filler", 0), ("block", 400)]),
        vec![(0.0, 600.0), (610.0, 390.0)]
    );
}
