//! The workspaces widget at a length the layout gives it (`width = 300` after a resize in edit
//! mode, `width = 200`) keeps the mark of an occupied workspace on that workspace's dot: the
//! marks start where the dots do and each is as long as a dot. Before, the marks were centered
//! in the widget and the dots counted from its start, so at 300 wide (ten dots are 260) every
//! mark stood 20 to the right of its dot, an empty circle between two dots.

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
id = "own"
type = "workspaces"
zone = "start"

[[monitor.container.widget]]
id = "longer"
type = "workspaces"
zone = "center"
width = 300

[[monitor.container.widget]]
id = "shorter"
type = "workspaces"
zone = "end"
width = 200
"#;

fn is(w: &World, i: usize, ty: &str) -> bool {
    w.items[i]
        .types
        .iter()
        .any(|t| t.rsplit('.').next() == Some(ty))
}

/// A workspaces widget: its length, and where its dots, its occupied marks and what draws the
/// marks start along the panel.
#[derive(Debug)]
struct Row {
    width: f64,
    dots: Vec<f64>,
    marks: Vec<f64>,
    drawn: (f64, f64),
}

fn row(rt: &mut Runtime, root: usize, id: &str) -> Row {
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
    let x = w.scene_pos(host, Some(root)).0;
    let (mut dots, mut marks, mut drawn) = (vec![], vec![], vec![]);
    let mut stack = vec![host];
    while let Some(i) = stack.pop() {
        stack.extend(w.items[i].children.iter().copied());
        let at = w.scene_pos(i, Some(root)).0 - x;
        if is(&w, i, "NumberWorkspaceItem") {
            dots.push(at);
        } else if is(&w, i, "Pill") && w.read(i, "undirectionalOffset").is_some() {
            let cell = w.items[i].parent.unwrap();
            marks.push(w.scene_pos(cell, Some(root)).0 - x);
        } else if is(&w, i, "MaskMultiEffect") {
            drawn.push((at, num(&mut w, i, "width")));
        }
    }
    dots.sort_by(f64::total_cmp);
    marks.sort_by(f64::total_cmp);
    assert_eq!(drawn.len(), 1, "{id}: what draws the marks");
    Row {
        width: num(&mut w, host, "width"),
        dots,
        marks,
        drawn: drawn[0],
    }
}

#[test]
fn the_occupied_marks_stay_on_the_dots_at_a_length_the_layout_gives() {
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

    let own = row(&mut rt, root, "own");
    assert_eq!(own.dots.len(), 10, "{own:?}");
    assert_eq!(own.width, 260.0, "ten dots of 26: {own:?}");
    for (id, width) in [("own", 260.0), ("longer", 300.0), ("shorter", 200.0)] {
        let r = row(&mut rt, root, id);
        assert_eq!(r.width, width, "{id}: {r:?}");
        assert_eq!(r.dots, own.dots, "{id}: the dots from the widget's start");
        assert_eq!(r.marks, r.dots, "{id}: each mark on its dot");
        assert_eq!(r.drawn, (r.dots[0], 260.0), "{id}: drawn over the dots");
    }
}
