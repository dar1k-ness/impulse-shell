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

/// The bar of `layout`, 1920 wide, laid out.
fn bar(layout: &str) -> (tempfile::TempDir, Runtime, usize) {
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
    std::fs::write(dir.path().join("layout.toml"), layout).unwrap();
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
    (dir, rt, root)
}

#[test]
fn a_panel_widget_fixed_shorter_than_its_text_shows_no_text() {
    let (_dir, mut rt, root) = bar(LAYOUT);
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

/// The centre of a bar as its owner left it: the workspaces pulled to 300 px and the clock set
/// to stretch beside them, in the group that keeps a fixed length.
const CROWDED: &str = r#"version = 5

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
id = "resources"
type = "resources"
zone = "center"

[[monitor.container.widget]]
id = "media"
type = "media"
zone = "center"
width = "stretch"

[[monitor.container.widget]]
id = "separator-1"
type = "separator"
zone = "center"

[[monitor.container.widget]]
id = "separator-2"
type = "separator"
zone = "center"

[[monitor.container.widget]]
id = "workspaces"
type = "workspaces"
zone = "center"
width = 300

[[monitor.container.widget]]
id = "clock"
type = "clock"
zone = "center"
width = "stretch"

[[monitor.container.widget]]
id = "util-buttons"
type = "util-buttons"
zone = "center"
"#;

/// A clock set to stretch is a widget of one size: with no room to stretch into it keeps the
/// length its time and date take. It was cut down to the registry's 24 px, a strip with a
/// piece of the date sticking out.
#[test]
fn a_stretching_clock_with_no_room_keeps_its_time_and_date() {
    let (_dir, mut rt, root) = bar(CROWDED);
    let clock = shows(&mut rt, root, "clock");
    assert_eq!(
        clock.words.len(),
        3,
        "the time, a dot, the date: {:?}",
        clock.words
    );
    assert!(
        clock.words.iter().all(|w| w.1),
        "all of it inside its {} px: {:?}",
        clock.width,
        clock.words
    );
    let spaces = shows(&mut rt, root, "workspaces");
    assert_eq!(spaces.width, 300.0);
    assert!(clock.x >= spaces.x + spaces.width, "after the workspaces");
}

/// A centre group as the first run's is after its clock was carried in beside the player: both
/// stretch in one group of a fixed length, most of which another widget has (there the
/// resources, here a widget of a known length).
const BESIDE_THE_PLAYER: &str = r#"version = 5

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
id = "clock-fixed"
type = "clock"
zone = "center"
width = 200

[[monitor.container.widget]]
id = "clock"
type = "clock"
zone = "center"
width = "stretch"

[[monitor.container.widget]]
id = "media"
type = "media"
zone = "center"
width = "stretch"

[[monitor.container.widget]]
id = "separator-2"
type = "separator"
zone = "center"

[[monitor.container.widget]]
id = "workspaces"
type = "workspaces"
zone = "center"

[[monitor.container.widget]]
id = "util-buttons"
type = "util-buttons"
zone = "center"
"#;

/// A stretching clock carried in beside the stretching player keeps its time and date whole;
/// the player, which cuts its title, is the one that gives way. The clock was left a strip of
/// some 60 px with its text cut on both sides.
#[test]
fn a_stretching_clock_beside_a_stretching_player_keeps_its_time_and_date() {
    let (_dir, mut rt, root) = bar(BESIDE_THE_PLAYER);
    let clock = shows(&mut rt, root, "clock");
    assert_eq!(
        clock.words.len(),
        3,
        "the time, a dot, the date: {:?}",
        clock.words
    );
    assert!(
        clock.words.iter().all(|w| w.1),
        "all of it inside its {} px: {:?}",
        clock.width,
        clock.words
    );
    let media = shows(&mut rt, root, "media");
    assert!(media.x >= clock.x + clock.width, "the player after it");
    assert!(
        media.words.iter().all(|w| w.1),
        "the player's title inside its {} px: {:?}",
        media.width,
        media.words
    );
}

/// A widget carried along its panel in edit mode is on a chip with an outline; let go away from
/// its place it slides there on the chip with no outline, so the only outline on the screen is
/// the one edit mode has around that place at once. Before, the chip kept its outline for the
/// 150 ms of the slide: two outlines a few px apart.
#[test]
fn a_panel_widget_let_go_slides_to_its_place_with_no_outline_of_its_own() {
    let (_dir, mut rt, _root) = bar(LAYOUT);
    let (host, chip) = {
        let mut w = rt.world.borrow_mut();
        let host = w
            .of_type("PanelWidget")
            .into_iter()
            .find(|&h| {
                w.read(h, "layoutId")
                    .is_some_and(|v| v.to_js_string() == "clock")
            })
            .unwrap();
        let chip = w.items[host]
            .children
            .clone()
            .into_iter()
            .find(|&c| {
                is(&w, c, "Rectangle") && w.read(c, "z").is_some_and(|z| z.to_number() == -1.0)
            })
            .expect("the chip");
        (host, chip)
    };
    let chip_is = |rt: &mut Runtime| {
        let mut w = rt.world.borrow_mut();
        let visible = w.read(chip, "visible").is_some_and(|v| v.truthy());
        let outline = match w.read(chip, "border.width").map(|v| v.to_number()) {
            Some(n) if visible && n > 0.0 => w
                .read(chip, "border.color")
                .is_some_and(|c| c.to_js_string() != "#00000000"),
            _ => false,
        };
        (
            visible,
            outline,
            w.read(host, "shiftX").map_or(f64::NAN, |v| v.to_number()),
        )
    };
    assert_eq!(chip_is(&mut rt), (false, false, 0.0), "at rest");
    rt.set(host, "carried", Value::Bool(true));
    rt.set(host, "carryX", Value::Num(-40.0));
    rt.settle();
    let mut now = 1000.0;
    rt.tick(now);
    rt.settle();
    assert_eq!(chip_is(&mut rt), (true, true, -40.0), "carried");
    // Let go: the controller takes both back at once.
    rt.set(host, "carried", Value::Bool(false));
    rt.set(host, "carryX", Value::Num(0.0));
    rt.settle();
    let mut slid = 0;
    loop {
        let (visible, outline, shift) = chip_is(&mut rt);
        assert!(
            !outline,
            "no outline once let go, {shift} px from its place"
        );
        if shift == 0.0 {
            assert!(!visible, "no chip at its place");
            break;
        }
        assert!(visible, "on its chip {shift} px from its place");
        slid += 1;
        assert!(slid < 40, "still {shift} px from its place");
        now += 10.0;
        rt.tick(now);
        rt.settle();
    }
    assert!(slid >= 5, "it slides to its place: {slid} frames");
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
}
