//! Layouts, positioners, views and anchors place their items where Qt 6.12 does. Each case is
//! a scene in `scenes/qt_geometry/NAME.qml` with the geometry real Qt gave it in
//! `scenes/qt_geometry/NAME.qt` (`scenes/qt_geometry/dump.py`, run in the testenv image): one
//! line per item with an `objectName`, `name x y width height implicitWidth implicitHeight`,
//! read after the scene's timers ran.
//!
//! The scenes are compiled into the test (`testscenes`): a build with the `interp` feature
//! interprets their scripts, a build without it runs the compiled ones, as the installed
//! shell runs its tree.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::scene;

fn dump(rt: &mut Runtime, i: usize, rows: &mut Vec<String>) {
    let name = match rt.world.borrow_mut().read(i, "objectName") {
        Some(Value::Str(s)) => s.to_string(),
        _ => String::new(),
    };
    if !name.is_empty() {
        let mut row = name;
        for p in [
            "x",
            "y",
            "width",
            "height",
            "implicitWidth",
            "implicitHeight",
        ] {
            let v = rt.world.borrow_mut().read(i, p);
            row += &format!(" {:.2}", v.map_or(f64::NAN, |v| v.to_number()));
        }
        rows.push(row.replace("-0.00", "0.00"));
    }
    let kids = scene::visual_kids(&mut rt.world.borrow_mut(), i);
    for c in kids {
        dump(rt, c, rows);
    }
}

/// The scene's geometry after a second of frames: it is laid out as loaded, then its timers
/// run each on its frame, as under Qt's dump.
fn ours(name: &str) -> Vec<String> {
    let (mut rt, root) = testscenes::load(&format!("qt_geometry/{name}.qml"));
    rt.settle();
    let size = [rt.get(root, "width"), rt.get(root, "height")].map(|v| v.to_number());
    let mut fonts = scene::Fonts::default();
    let mut now = 0.0;
    rt.tick(now);
    for _ in 0..64 {
        scene::layout_window(&mut rt, &mut fonts, root, size);
        now += 16.0;
        rt.tick(now);
    }
    scene::layout_window(&mut rt, &mut fonts, root, size);
    let mut rows = vec![];
    dump(&mut rt, root, &mut rows);
    rows.sort();
    let errors = rt.errors();
    assert!(errors.is_empty(), "{name}: {errors:?}");
    rows
}

fn check(name: &str, qt: &str) {
    let ours = ours(name);
    let qt: Vec<&str> = qt.lines().filter(|l| !l.is_empty()).collect();
    let mut wrong = vec![];
    for q in &qt {
        let n = q.split(' ').next().unwrap();
        match ours.iter().find(|o| o.split(' ').next() == Some(n)) {
            Some(o) if o == q => {}
            Some(o) => wrong.push(format!("  ours {o}\n    Qt {q}")),
            None => wrong.push(format!("  ours has no {n}\n    Qt {q}")),
        }
    }
    for o in &ours {
        let n = o.split(' ').next().unwrap();
        if !qt.iter().any(|q| q.split(' ').next() == Some(n)) {
            wrong.push(format!("  ours {o}\n    Qt has no {n}"));
        }
    }
    assert!(
        wrong.is_empty(),
        "{name}: name x y width height implicitWidth implicitHeight\n{}",
        wrong.join("\n")
    );
}

macro_rules! case {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[test]
        fn $name() {
            check(
                stringify!($name),
                include_str!(concat!("scenes/qt_geometry/", stringify!($name), ".qt")),
            );
        }
    };
}

case!(
    /// `GridLayout.uniformCellWidths` / `uniformCellHeights`: every column (row) gets one size.
    uniform_grid
);
case!(
    /// `RowLayout` / `ColumnLayout` with `uniformCellSizes` and no set size.
    uniform_row
);
case!(
    /// A width or height written from JavaScript is a set size; `undefined` resets it.
    js_size
);
case!(
    /// `GridLayout`: `Layout.columnSpan`, `rowSpan`, `row`, `column`, `rows` and `flow`.
    grid_cells
);
case!(
    /// The `Grid` positioner: `columns`, `rows`, `flow`, item alignment, padding, right-to-left.
    grid_positioner
);
case!(
    /// `Row`, `Column`, `Flow`: padding on each side and `layoutDirection: Qt.RightToLeft`.
    positioner_padding
);
case!(
    /// The ShaderEffectSource and the effect item of a layer stand after their item among its
    /// siblings: positioners, layouts and `childrenRect` leave them out, and the effect item
    /// is where its item was put.
    layer_positioners
);
case!(
    /// The shapes the shell's own QML puts a layered item in: a layered TextField between a
    /// Loader that is off and a button in a RowLayout (the lock screen's password field), and
    /// a layered SwipeView filling a ColumnLayout (the cheatsheet). The layouts are as wide
    /// and as tall as without the layers, and the SwipeView counts its pages only.
    layer_shell_layouts
);
case!(
    /// `ListView`: `header`, `footer`, `highlight` and `verticalLayoutDirection: BottomToTop`.
    listview_decor
);
case!(
    /// An edge anchor with the centre anchor of the same axis gives the item its size.
    anchor_center
);
case!(rowlayout1);
case!(collayout1);
case!(loader_size);
case!(
    /// A Loader with a set size sets its item's size, and that stays set when the Loader's own
    /// is reset: the Loader then takes the item's size as its implicit one.
    loader_reset
);
case!(
    /// Anchors that cross give a negative size, which anchors to that item then use.
    anchor_cross
);
case!(
    /// A Text that elides lays out no line once its set width leaves no room for text (or,
    /// eliding right, its set height leaves none). The `…State` items carry each Text's
    /// `lineCount`, `truncated`, `contentWidth` and `contentHeight` as x, y, width and height.
    elide_no_room
);
case!(
    /// `Grid.rowSpacing` / `columnSpacing`: the gap is `spacing` until one is written, and
    /// again after its reset; -1 is only the value they read before a write.
    grid_spacing
);
case!(
    /// A property declared in QML under the name of a C++ one (`spacing`, `implicitWidth`,
    /// `state`, `scale`, `currentIndex`, `active`, `model`, `count`, `resources`, `enabled`)
    /// leaves the C++ property, and what the item does with it, as it was.
    hidden_names
);
