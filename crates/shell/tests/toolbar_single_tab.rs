//! The tree's `ToolbarTabBar` with one tab (the left sidebar with only "Assistant"): Quickshell
//! draws no pill behind it. The indicator is sized from `contentItem.children[currentIndex]`,
//! a binding that last ran when the only tab was appended behind its Repeater, before the
//! Repeater stacked it in front without a signal, so it reads the Repeater's zero size. With
//! two tabs the first one is in place by then and the pill is its size.

use qml::runtime::{Runtime, World};
use qml::value::Value;
use shell::scene;
use std::path::{Path, PathBuf};

fn tree(tabs: &str) -> PathBuf {
    tree_of(&format!("ToolbarTabBar {{ tabButtonList: [{tabs}] }}"))
}

fn tree_of(body: &str) -> PathBuf {
    let ri = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../qml")
        .canonicalize()
        .unwrap();
    let d = tempfile::tempdir().unwrap().keep();
    for e in std::fs::read_dir(&ri).unwrap() {
        let e = e.unwrap();
        std::os::unix::fs::symlink(e.path(), d.join(e.file_name())).unwrap();
    }
    std::fs::write(
        d.join("main.qml"),
        format!(
            "import QtQuick\nimport QtQuick.Controls\nimport ri.modules.common.widgets\nimport ri.modules.rusty.sidebarLeft\nimport Quickshell\nItem {{\n    id: host\n    width: 400\n    height: 100\n    \
             {body}\n}}\n"
        ),
    )
    .unwrap();
    d
}

/// [width, height] of the bar's active indicator and of its first tab button.
fn indicator_and_tab(tabs: &str) -> ([f64; 2], [f64; 2]) {
    indicator_and_tab_in(tree(tabs))
}

fn indicator_and_tab_in(d: PathBuf) -> ([f64; 2], [f64; 2]) {
    let mut rt = Runtime::new(Some(d.clone()));
    rt.eng()
        .set_compiled(Some(std::rc::Rc::new(ricompiled::Compiled::new(&d))));
    rt.world.borrow_mut().track_fresh = true;
    rt.world.borrow_mut().frame_paced = true;
    let pf = std::rc::Rc::new(std::cell::RefCell::new(scene::Fonts::default()));
    {
        let f = pf.clone();
        rt.world.borrow_mut().pre_complete = Some(Box::new(move |w, n0| {
            f.try_borrow_mut()
                .is_ok_and(|mut f| scene::measure_new(w, &mut f, n0))
        }));
        let f = pf.clone();
        rt.world.borrow_mut().text_measure = Some(Box::new(move |w, i| {
            if let Ok(mut f) = f.try_borrow_mut() {
                scene::measure_text(w, &mut f, i);
            }
        }));
    }
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
        scene::layout_window(&mut rt, &mut fonts, main, [400.0, 100.0]);
        rt.settle();
        // The sidebar's own window, when the scene loads one.
        let panels: Vec<usize> = {
            let w = rt.world.borrow();
            (0..w.items.len())
                .filter(|&i| !w.dead.contains(&i))
                .filter(|&i| w.items[i].types.iter().any(|t| t == "PanelWindow"))
                .collect()
        };
        for p in panels {
            scene::layout_window(&mut rt, &mut fonts, p, [500.0, 800.0]);
            rt.settle();
        }
    }
    let mut w = rt.world.borrow_mut();
    let of = |w: &World, ty: &str| {
        (0..w.items.len())
            .find(|&i| !w.dead.contains(&i) && w.items[i].types.iter().any(|t| t == ty))
            .unwrap_or_else(|| panic!("no {ty}"))
    };
    let bar = of(&w, "ToolbarTabBar");
    let indicator = w.items[bar]
        .children
        .iter()
        .copied()
        .find(|&c| w.items[c].types.iter().any(|t| t == "Rectangle"))
        .expect("no active indicator");
    let tab = of(&w, "ToolbarTabButton");
    let size = |w: &mut World, i: usize| {
        ["width", "height"].map(|k| w.read(i, k).unwrap_or(Value::Num(f64::NAN)).to_number())
    };
    let r = (size(&mut w, indicator), size(&mut w, tab));
    drop(w);
    std::fs::remove_dir_all(&d).unwrap();
    r
}

#[test]
fn a_single_tab_has_no_indicator() {
    let (indicator, tab) = indicator_and_tab(r#"{ "name": "Assistant", "icon": "forum" }"#);
    assert!(tab[0] > 0.0 && tab[1] > 0.0, "the tab is laid out: {tab:?}");
    assert_eq!(indicator, [0.0, 0.0]);
}

#[test]
fn the_first_of_two_tabs_has_an_indicator_of_its_size() {
    let (indicator, tab) = indicator_and_tab(
        r#"{ "name": "Assistant", "icon": "forum" }, { "name": "Translator", "icon": "translate" }"#,
    );
    assert!(tab[0] > 0.0 && tab[1] > 0.0, "the tab is laid out: {tab:?}");
    // The pill spans the tab on whole pixels.
    assert!(
        (indicator[0] - tab[0]).abs() < 1.0 && indicator[1] == tab[1],
        "indicator {indicator:?}, tab {tab:?}"
    );
}

/// The left sidebar as the shell loads it (ImpulseShellFamily.qml's PanelLoader), where the
/// original draws no pill either.
#[test]
fn the_loaded_left_sidebar_with_one_tab_has_no_indicator() {
    let (indicator, tab) = indicator_and_tab_in(tree_of(
        "LazyLoader { active: true; component: SidebarLeft {} }",
    ));
    assert!(tab[0] > 0.0 && tab[1] > 0.0, "the tab is laid out: {tab:?}");
    assert_eq!(indicator, [0.0, 0.0]);
}
