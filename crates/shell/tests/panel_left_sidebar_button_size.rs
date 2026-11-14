//! The left sidebar button of a panel from the layout (`panel/LeftSidebarButtonWidget.qml`) has
//! the place and size the same button has in the original's own bar: its own 30x30 in the middle of the
//! bar's thickness, not stretched across it. Stretched, its round background is a pill as thick
//! as the bar, which shows once the left sidebar is open.
//!
//! The expected rectangles are Qt's for the button of the original's `BarContent` and
//! `VerticalBarContent` (`qt_puts_the_bar_button_there` asks `qml6` for them): Quickshell
//! never loads the panel, so that button is what the original shows.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::scene;
use std::path::PathBuf;

/// Qt 6.12: x, y, width, height of the button in a horizontal bar 40 high.
const QT_HORIZONTAL: [f64; 4] = [23.0, 5.0, 30.0, 30.0];
/// Qt 6.12: the same in a vertical bar 46 wide.
const QT_VERTICAL: [f64; 4] = [8.0, 13.25, 30.0, 30.0];

/// The button as the original's bars place it: `LeftSidebarButton` (a `Button` as large as its 19.5 px
/// icon plus 5 px of padding each side) in `BarContent`'s left `RowLayout`, after the screen
/// rounding (23), and at the top of `VerticalBarContent`'s `ColumnLayout`, below the gap (5).
const QT_SCENE: &str = r#"import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Window

Window {
    id: root
    width: 600
    height: 300
    visible: true
    component SidebarButton: Button {
        id: button
        property real buttonPadding: 5
        implicitWidth: icon.width + buttonPadding * 2
        implicitHeight: icon.height + buttonPadding * 2
        background: Rectangle { implicitHeight: 30; radius: 9999 }
        contentItem: Item {}
        Item { id: icon; anchors.centerIn: parent; width: 19.5; height: 19.5 }
    }
    Item {
        id: bar
        width: 461
        height: 40
        RowLayout {
            anchors.fill: parent
            spacing: 0
            SidebarButton { id: across; Layout.alignment: Qt.AlignVCenter; Layout.leftMargin: 23 }
            Item { Layout.leftMargin: 10; Layout.fillWidth: true; Layout.fillHeight: true }
        }
    }
    Item {
        id: verticalBar
        y: 50
        width: 46
        height: 200
        ColumnLayout {
            anchors.fill: parent
            spacing: 10
            SidebarButton {
                id: down
                Layout.alignment: Qt.AlignHCenter
                Layout.topMargin: (46 - implicitWidth) / 2 + 5
            }
            Item { Layout.fillHeight: true }
        }
    }
    function rect(item, bar) {
        const at = item.mapToItem(bar, 0, 0);
        return [at.x, at.y, item.width, item.height].join(" ");
    }
    // The layouts have arranged their items by the first frame.
    onFrameSwapped: {
        console.log("RECT", rect(across, bar));
        console.log("RECT", rect(down, verticalBar));
        Qt.quit();
    }
}
"#;

/// A panel with the start zone of the layout the shell makes from the original's bar; `edge` is its
/// `edge` (and `thickness`) lines.
fn layout(edge: &str) -> String {
    format!(
        r#"version = 2

[[monitor]]
connector = "DP-1"
primary = true

[[monitor.container]]
id = "panel"
kind = "panel"
{edge}
length = "fill"
align = "center"
floating = false

[[monitor.container.widget]]
id = "left-sidebar-button"
type = "left-sidebar-button"
zone = "start"
width = "auto"

[[monitor.container.widget]]
id = "active-window"
type = "active-window"
zone = "start"
width = "stretch"
"#
    )
}

/// `i`'s rectangle in the coordinates of `root`.
fn rect_in(rt: &mut Runtime, root: usize, i: usize) -> [f64; 4] {
    let mut w = rt.world.borrow_mut();
    let num = |w: &mut qml::runtime::World, i: usize, k: &str| {
        w.read(i, k).map_or(f64::NAN, |v| v.to_number())
    };
    let mut r = [0.0, 0.0, num(&mut w, i, "width"), num(&mut w, i, "height")];
    let mut at = Some(i);
    while let Some(k) = at.filter(|&k| k != root) {
        r[0] += num(&mut w, k, "x");
        r[1] += num(&mut w, k, "y");
        at = w.items[k].parent;
    }
    r
}

/// The left sidebar button under `root`, and whether a panel widget holds it.
fn button(rt: &Runtime, root: usize) -> (usize, bool) {
    let w = rt.world.borrow();
    let is = |i: usize, ty: &str| {
        w.items[i]
            .types
            .iter()
            .any(|t| t.rsplit('.').next() == Some(ty))
    };
    let mut found = vec![];
    let mut stack = vec![root];
    while let Some(i) = stack.pop() {
        if is(i, "LeftSidebarButton") {
            let in_panel = std::iter::successors(w.items[i].parent, |&p| w.items[p].parent)
                .any(|p| is(p, "PanelWidget"));
            found.push((i, in_panel));
        }
        stack.extend(w.items[i].children.iter().copied());
    }
    assert_eq!(found.len(), 1, "one left sidebar button");
    found[0]
}

/// The button's rectangle in a bar `size` large whose content is `content`, a file of the tree
/// the build compiled (so the build without the interpreter runs it too): the original's own content
/// with no layout, the layout's panel with `panel` (its `edge` lines). Once per test: the scene
/// keeps what it laid out by item id, per thread.
fn placed(content: &str, panel: Option<&str>, size: [f64; 2]) -> [f64; 4] {
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
    if let Some(edge) = panel {
        std::fs::write(dir.path().join("layout.toml"), layout(edge)).unwrap();
        rt.world
            .borrow_mut()
            .open_layout(dir.path().join("layout.toml"));
        rt.settle();
        assert_eq!(rt.world.borrow().errors, Vec::<String>::new(), "the layout");
    }
    let root = rt.load_file(&qml.join(content)).unwrap();
    rt.set(root, "width", Value::Num(size[0]));
    rt.set(root, "height", Value::Num(size[1]));
    rt.settle();
    let mut fonts = scene::Fonts::default();
    for _ in 0..4 {
        scene::layout_window(&mut rt, &mut fonts, root, size);
        rt.settle();
    }
    let (button, in_panel) = button(&rt, root);
    assert_eq!(in_panel, panel.is_some(), "the panel holds the button");
    rect_in(&mut rt, root, button)
}

fn horizontal(panel: Option<&str>) -> [f64; 4] {
    placed("modules/rusty/bar/BarContent.qml", panel, [1920.0, 40.0])
}

fn vertical(panel: Option<&str>) -> [f64; 4] {
    placed(
        "modules/rusty/verticalBar/VerticalBarContent.qml",
        panel,
        [46.0, 1080.0],
    )
}

#[test]
fn horizontal_bar_button_is_where_qt_puts_it() {
    assert_eq!(horizontal(None), QT_HORIZONTAL);
}

/// The panel the shell makes from the original's bar (`edge = "rusty"`).
#[test]
fn horizontal_panel_button_is_where_the_bar_button_is() {
    assert_eq!(horizontal(Some("edge = \"rusty\"")), QT_HORIZONTAL);
}

#[test]
fn vertical_bar_button_is_where_qt_puts_it() {
    assert_eq!(vertical(None), QT_VERTICAL);
}

#[test]
fn vertical_panel_button_is_where_the_bar_button_is() {
    assert_eq!(
        vertical(Some("edge = \"left\"\nthickness = 46")),
        QT_VERTICAL
    );
}

#[test]
fn qt_puts_the_bar_button_there() {
    if !testenv::require("qml6") {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("bars.qml");
    std::fs::write(&file, QT_SCENE).unwrap();
    let out = std::process::Command::new("qml6")
        .arg(&file)
        .env("QT_QPA_PLATFORM", "offscreen")
        .env("QT_FORCE_STDERR_LOGGING", "1")
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&out.stderr);
    let rects: Vec<Vec<f64>> = log
        .lines()
        .filter_map(|l| l.split_once("RECT ").map(|(_, r)| r))
        .map(|r| r.split(' ').map(|n| n.parse().unwrap()).collect())
        .collect();
    assert_eq!(
        rects,
        [QT_HORIZONTAL.to_vec(), QT_VERTICAL.to_vec()],
        "qml6 printed:\n{log}"
    );
}
