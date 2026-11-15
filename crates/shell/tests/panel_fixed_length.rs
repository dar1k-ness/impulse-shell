//! A panel widget the layout fixes shorter than its content (`width = 24`, what a player
//! dragged onto a panel got before the panel measured it) shows what fits that length: the
//! player its progress ring without the title, the clock a clock icon, or the time without the
//! date where the time fits. Before, the title and the date lay over the widget itself and the
//! widgets beside it. One the layout leaves its own length shows all of it, as before.

use qml::runtime::{Runtime, World};
use qml::value::Value;
use shell::scene;
use std::path::PathBuf;

const LAYOUT: &str = r#"version = 5

[[monitor]]
connector = "DP-1"
primary = true

[[monitor.container]]
id = "panel"
kind = "panel"
edge = "rusty"
length = "fill"
align = "center"
floating = false

[[monitor.container.widget]]
id = "media"
type = "media"
zone = "start"

[[monitor.container.widget]]
id = "media-fixed"
type = "media"
zone = "start"
width = 24

[[monitor.container.widget]]
id = "clock-fixed"
type = "clock"
zone = "center"
width = 24

[[monitor.container.widget]]
id = "clock-time"
type = "clock"
zone = "center"
width = 64

[[monitor.container.widget]]
id = "clock"
type = "clock"
zone = "end"
"#;

fn is(w: &World, i: usize, ty: &str) -> bool {
    w.items[i]
        .types
        .iter()
        .any(|t| t.rsplit('.').next() == Some(ty))
}

/// Whether `i` and every item above it is visible.
fn shown(w: &mut World, mut i: usize) -> bool {
    loop {
        if !w.read(i, "visible").is_none_or(|v| v.truthy()) {
            return false;
        }
        match w.items[i].parent {
            Some(p) => i = p,
            None => return true,
        }
    }
}

/// Where a panel widget is along the panel, and the text it shows.
struct Shows {
    x: f64,
    width: f64,
    /// The words, and whether each fits the widget's length.
    words: Vec<(String, bool)>,
    /// The icons (Material symbol names).
    icons: Vec<String>,
}

fn shows(rt: &mut Runtime, root: usize, id: &str) -> Shows {
    let mut w = rt.world.borrow_mut();
    let hosts: Vec<usize> = w
        .of_type("PanelWidget")
        .into_iter()
        .filter(|&h| {
            w.read(h, "layoutId")
                .is_some_and(|v| v.to_js_string() == id)
        })
        .collect();
    assert_eq!(hosts.len(), 1, "one panel widget {id}");
    let host = hosts[0];
    let num = |w: &mut World, i: usize, k: &str| w.read(i, k).map_or(f64::NAN, |v| v.to_number());
    let (x, width) = (w.scene_pos(host, Some(root)).0, num(&mut w, host, "width"));
    let (mut words, mut icons) = (vec![], vec![]);
    let mut stack = vec![host];
    while let Some(i) = stack.pop() {
        stack.extend(w.items[i].children.iter().copied());
        if !is(&w, i, "StyledText") || !shown(&mut w, i) {
            continue;
        }
        let text = w
            .read(i, "text")
            .unwrap_or_default()
            .to_js_string()
            .to_string();
        if text.is_empty() {
            continue;
        }
        if is(&w, i, "MaterialSymbol") {
            icons.push(text);
        } else {
            let (tx, tw) = (w.scene_pos(i, Some(root)).0, num(&mut w, i, "width"));
            words.push((text, tx >= x - 0.5 && tx + tw <= x + width + 0.5));
        }
    }
    Shows {
        x,
        width,
        words,
        icons,
    }
}

#[test]
fn a_panel_widget_fixed_shorter_than_its_text_shows_no_text() {
    let dir = tempfile::tempdir().unwrap();
    let qml = PathBuf::from(ricompiled::ROOT);
    let mut rt = Runtime::new(Some(qml.clone()));
    rt.eng()
        .set_compiled(Some(std::rc::Rc::new(ricompiled::Compiled::new(&qml))));
    rt.load_singletons(&[
        qml.clone(),
        qml.join("modules/common"),
        qml.join("modules/common/functions"),
        qml.join("services"),
    ]);
    std::fs::write(dir.path().join("layout.toml"), LAYOUT).unwrap();
    rt.world
        .borrow_mut()
        .open_layout(dir.path().join("layout.toml"));
    rt.settle();
    assert_eq!(rt.world.borrow().errors, Vec::<String>::new(), "the layout");
    let size = [1920.0, 40.0];
    let root = rt
        .load_file(&qml.join("modules/rusty/bar/BarContent.qml"))
        .unwrap();
    rt.set(root, "width", Value::Num(size[0]));
    rt.set(root, "height", Value::Num(size[1]));
    rt.settle();
    let mut fonts = scene::Fonts::default();
    for _ in 0..6 {
        scene::layout_window(&mut rt, &mut fonts, root, size);
        rt.settle();
    }

    let media = shows(&mut rt, root, "media");
    assert_eq!(
        media.words.len(),
        1,
        "its own length: the title, {:?}",
        media.words
    );
    assert!(
        media.words[0].1,
        "{:?} in {} wide",
        media.words, media.width
    );

    let fixed = shows(&mut rt, root, "media-fixed");
    assert_eq!(fixed.width, 24.0);
    assert_eq!(fixed.words, [], "24 wide: no title");
    assert!(fixed.x >= media.x + media.width, "after the one before it");

    let clock = shows(&mut rt, root, "clock");
    assert_eq!(
        clock.words.len(),
        3,
        "the time, a dot, the date: {:?}",
        clock.words
    );
    assert_eq!(clock.icons, Vec::<String>::new());

    let time = shows(&mut rt, root, "clock-time");
    assert_eq!(time.width, 64.0);
    assert_eq!(
        time.words.len(),
        1,
        "64 wide: the time alone, {:?}",
        time.words
    );
    assert!(time.words[0].1, "{:?} in {} wide", time.words, time.width);
    assert!(clock.words.contains(&time.words[0]), "{:?}", clock.words);

    let fixed = shows(&mut rt, root, "clock-fixed");
    assert_eq!(fixed.width, 24.0);
    assert_eq!(fixed.words, [], "24 wide: no time");
    assert_eq!(fixed.icons, ["schedule"]);
}
