//! The layout's bar (`modules/rusty/bar/panel`) behaves as the original's own bar content does: the status
//! indicators' margins animate as they appear, `bar.floatStyleShadow` turns the shadow of a
//! floating horizontal bar off, and the separators next to the workspaces show nothing.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::scene;
use std::path::{Path, PathBuf};

/// The tree with `main` as its main.qml.
fn tree(tag: &str, main: &str) -> PathBuf {
    let ri = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../qml")
        .canonicalize()
        .unwrap();
    let d = std::env::temp_dir().join(format!("bar-panel-content-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    for e in std::fs::read_dir(&ri).unwrap() {
        let e = e.unwrap();
        std::os::unix::fs::symlink(e.path(), d.join(e.file_name())).unwrap();
    }
    std::fs::write(d.join("zzmain.qml"), main).unwrap();
    d
}

fn start(d: &Path) -> (Runtime, usize) {
    let mut rt = Runtime::new(Some(d.to_path_buf()));
    rt.load_singletons(&[
        d.to_path_buf(),
        d.join("modules/common"),
        d.join("modules/common/functions"),
        d.join("services"),
    ]);
    rt.settle();
    let main = rt.load_file(&d.join("zzmain.qml")).unwrap();
    rt.settle();
    (rt, main)
}

/// Writes `value` to `path` under singleton `name`, as the original's settings window and services do.
fn write(rt: &mut Runtime, name: &str, path: &[&str], value: Value) {
    let mut o = *rt.world.borrow().singletons.get(name).expect(name);
    {
        let mut wd = rt.world.borrow_mut();
        let (key, objects) = path.split_last().unwrap();
        for k in objects {
            o = match wd.read(o, k) {
                Some(Value::Item(i)) => i,
                v => panic!("{k}: {v:?}"),
            };
        }
        wd.write(o, key, value);
    }
    rt.settle();
}

/// The live items of type `t`, in creation order.
fn live(rt: &Runtime, t: &str) -> Vec<usize> {
    let wd = rt.world.borrow();
    (0..wd.items.len())
        .filter(|&i| !wd.dead.contains(&i) && wd.items[i].types.iter().any(|x| x == t))
        .collect()
}

const INDICATORS: &str = r#"import QtQuick
import ri.modules.rusty.bar.panel
Item {
    width: 400
    height: 40
    property bool vertical: false
    IndicatorsWidget {}
}
"#;

/// One window on a virtual frame clock, 16 ms a frame.
struct Frames {
    fonts: scene::Fonts,
    main: usize,
    now: f64,
}

impl Frames {
    fn frame(&mut self, rt: &mut Runtime) {
        self.now += 16.0;
        rt.tick(self.now);
        rt.settle();
        scene::layout_window(rt, &mut self.fonts, self.main, [400.0, 40.0]);
        rt.settle();
    }
}

/// The margin `prop` of the unread-notifications indicator on each frame after `silent` (do
/// not disturb) changes to `on`, until it rests at `to`.
fn margin_frames(rt: &mut Runtime, f: &mut Frames, prop: &str, on: bool, to: f64) -> Vec<f64> {
    let unread = *live(rt, "Revealer").last().expect("no Revealer");
    write(rt, "Notifications", &["silent"], Value::Bool(on));
    let mut seen = vec![];
    for _ in 0..200 {
        f.frame(rt);
        let m = rt.get(unread, prop).to_number();
        seen.push(m);
        if m == to && seen.len() > 1 {
            return seen;
        }
    }
    panic!("{prop} never rests at {to}: {seen:?}");
}

/// A margin that animates passes values strictly between its two ends, never outside them.
fn assert_animates(seen: &[f64], from: f64, to: f64, what: &str) {
    let (lo, hi) = (from.min(to), from.max(to));
    assert!(
        seen.iter().all(|m| (lo..=hi).contains(m)),
        "{what}: {seen:?}"
    );
    assert!(
        seen.iter().any(|m| lo < *m && *m < hi),
        "{what}: the margin jumps from {from} to {to}: {seen:?}"
    );
}

/// The original's BarContent / VerticalBarContent: `Behavior on Layout.rightMargin` (horizontal bar) and
/// `Behavior on Layout.bottomMargin` (vertical bar) on the indicators that come and go, so the
/// space next to one opens and closes with it instead of jumping.
#[test]
fn indicator_margins_animate_as_they_appear() {
    let d = tree("indicators", INDICATORS);
    let (mut rt, main) = start(&d);
    let mut f = Frames {
        fonts: scene::Fonts::default(),
        main,
        now: 1000.0,
    };
    // Behaviors are armed once the items went through their first layout.
    for _ in 0..4 {
        f.frame(&mut rt);
    }
    let unread = *live(&rt, "Revealer").last().expect("no Revealer");
    assert_eq!(rt.get(unread, "Layout.rightMargin").to_number(), 0.0);

    let seen = margin_frames(&mut rt, &mut f, "Layout.rightMargin", true, 15.0);
    assert_animates(&seen, 0.0, 15.0, "horizontal, appearing");
    let seen = margin_frames(&mut rt, &mut f, "Layout.rightMargin", false, 0.0);
    assert_animates(&seen, 15.0, 0.0, "horizontal, going");

    rt.set(main, "vertical", Value::Bool(true));
    for _ in 0..4 {
        f.frame(&mut rt);
    }
    assert_eq!(rt.get(unread, "Layout.bottomMargin").to_number(), 0.0);
    let seen = margin_frames(&mut rt, &mut f, "Layout.bottomMargin", true, 6.0);
    assert_animates(&seen, 0.0, 6.0, "vertical, appearing");
    let seen = margin_frames(&mut rt, &mut f, "Layout.bottomMargin", false, 0.0);
    assert_animates(&seen, 6.0, 0.0, "vertical, going");
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    drop(rt);
    std::fs::remove_dir_all(&d).unwrap();
}

const PANEL: &str = r#"import QtQuick
import ri.modules.rusty.bar.panel
Item {
    width: 1200
    height: 60
    PanelContent { anchors.fill: parent }
}
"#;

/// The original's BarContent: the shadow of the floating bar (`bar.cornerStyle` 1) is drawn only with
/// `bar.floatStyleShadow` on (the original's default).
#[test]
fn the_floating_bar_shadow_follows_float_style_shadow() {
    let d = tree("shadow", PANEL);
    let (mut rt, _) = start(&d);
    let shadows = |rt: &Runtime| live(rt, "StyledRectangularShadow").len();
    let bar = |rt: &mut Runtime, key: &str, v: Value| {
        write(rt, "Config", &["options", "bar", key], v);
    };
    assert_eq!(
        shadows(&rt),
        0,
        "the original's default corner style is not floating"
    );
    bar(&mut rt, "cornerStyle", Value::Num(1.0));
    assert_eq!(
        shadows(&rt),
        1,
        "floating, shadow on (the original's default)"
    );
    bar(&mut rt, "floatStyleShadow", Value::Bool(false));
    assert_eq!(shadows(&rt), 0, "floating, shadow off");
    bar(&mut rt, "floatStyleShadow", Value::Bool(true));
    assert_eq!(shadows(&rt), 1, "floating, shadow on again");
    bar(&mut rt, "showBackground", Value::Bool(false));
    assert_eq!(shadows(&rt), 0, "no background, no shadow");
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    drop(rt);
    std::fs::remove_dir_all(&d).unwrap();
}

const SEPARATOR: &str = r#"import QtQuick
import ri.modules.rusty.bar.panel
Item {
    width: 400
    height: 40
    property bool vertical: false
    SeparatorWidget {}
}
"#;

/// The original's BarContent / VerticalBarContent: the separators next to the workspaces are Rectangles
/// with no height in a Row (no width in a Column), which the positioner skips. `bar.borderless`
/// makes them visible and still nothing shows: no line, no space taken. A panel run asks its
/// widget's `shown`.
#[test]
fn the_borderless_separator_shows_nothing() {
    let d = tree("separator", SEPARATOR);
    let (mut rt, main) = start(&d);
    let mut fonts = scene::Fonts::default();
    for borderless in [false, true] {
        write(
            &mut rt,
            "Config",
            &["options", "bar", "borderless"],
            Value::Bool(borderless),
        );
        scene::layout_window(&mut rt, &mut fonts, main, [400.0, 40.0]);
        rt.settle();
        let separator = *live(&rt, "SeparatorWidget").last().expect("no separator");
        assert!(
            !rt.get(separator, "shown").truthy(),
            "borderless {borderless}"
        );
        assert!(
            !rt.get(separator, "visible").truthy(),
            "borderless {borderless}"
        );
    }
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    drop(rt);
    std::fs::remove_dir_all(&d).unwrap();
}
