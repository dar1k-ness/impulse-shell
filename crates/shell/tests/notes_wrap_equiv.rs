//! The text field of Notes (`NotesContent.qml`: a `StyledTextArea` with padding 28 and 24,
//! wrapping, in a ScrollView 460 wide) compared with Qt itself (`qml6`, offscreen) on the
//! fonts of the machine the test runs on: where the caret stands at every position of a
//! sentence that wraps. The real original broke its first line after "lazy dog", ours after
//! "lazy dog and". Two tests: the TextEdit alone at the width Qt gives it in Notes (378), and
//! the field in its ScrollView with the text typed after the load, as in Notes.
//! `StyledTextArea` sets `font.hintingPreference: Font.PreferFullHinting` and
//! `renderType: Text.NativeRendering`: Qt then lays out with advances rounded to whole pixels.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::scene;

const TEXT: &str = "The quick brown fox jumps over the lazy dog and keeps running through \
    the long field שלום עולם مرحبا بالعالم 123 until the line wraps twice or more";

/// What `StyledTextArea` sets, and each of the two it sets apart from the defaults alone. The
/// render type is always named: `qml6` starts with QtRendering and quickshell, which we follow,
/// with NativeRendering, and with full hinting the two lay out a font differently (QtRendering
/// rounds the design advances, NativeRendering takes the hinted ones).
const FONTS: [&str; 4] = [
    "font.hintingPreference: Font.PreferFullHinting; renderType: TextEdit.NativeRendering; \
     font.variableAxes: ({ \"wdth\": 100, \"wght\": 450 })",
    "font.hintingPreference: Font.PreferFullHinting; renderType: TextEdit.QtRendering",
    "renderType: TextEdit.NativeRendering",
    "renderType: TextEdit.QtRendering",
];

const FAMILIES: [&str; 2] = ["Google Sans Flex", "Noto Sans"];

/// The longest line of the hint of Notes: it is what makes the empty field 378 wide in Qt.
const HINT: &str = "Write something here...\\nUse '-' to create copyable bullet points, like this:";

fn font(family: &str, font: &str) -> String {
    match font.is_empty() {
        true => format!("font.family: \"{family}\"; font.pixelSize: 15"),
        false => format!("font.family: \"{family}\"; font.pixelSize: 15; {font}"),
    }
}

/// The TextEdit alone, with the paddings its TextArea has in Notes: each as QML with `ID`
/// where its id goes, and what the failure calls it.
fn alone() -> Vec<(String, String)> {
    let mut cases = Vec::new();
    for family in FAMILIES {
        for f in FONTS {
            let font = font(family, f);
            cases.push((
                format!(
                    "TextEdit {{ id: ID; width: 378; leftPadding: 28; rightPadding: 24; \
                     topPadding: 24; wrapMode: TextEdit.Wrap; {font}; text: \"{TEXT}\" }}"
                ),
                format!("TextEdit, {font}"),
            ));
        }
    }
    cases
}

/// As NotesContent.qml has it: a TextArea with `padding: 24` and a hint, anchored to the
/// sides of a ScrollView 460 wide. Its text is set by `fill()` after the load.
fn in_view() -> Vec<(String, String)> {
    FAMILIES
        .iter()
        .map(|family| {
            let font = font(family, FONTS[0]);
            (
                format!(
                    "ScrollView {{ width: 460; height: 460; clip: true; \
                     ScrollBar.vertical.policy: ScrollBar.AsNeeded; \
                     TextArea {{ id: ID; anchors.left: parent.left; anchors.right: parent.right; \
                     wrapMode: TextEdit.Wrap; background: null; padding: 24; {font}; \
                     placeholderText: \"{HINT}\" }} }}"
                ),
                format!("TextArea in a ScrollView, {font}"),
            )
        })
        .collect()
}

fn source(cases: &[(String, String)], qt: bool) -> String {
    let mut s = "import QtQuick\nimport QtQuick.Controls\nItem {\n    width: 500; height: 500\n"
        .to_string();
    for (i, (item, _)) in cases.iter().enumerate() {
        let item = item.replace("id: ID", &format!("id: e{i}"));
        s += &format!("    property var item{i}: e{i}\n    {item}\n");
    }
    s += &format!(
        "    function fill() {{\n        for (let i = 0; i < {}; i++) {{\n            \
         const e = this[\"item\" + i];\n            \
         if (e.text === \"\") e.text = \"{TEXT}\";\n        }}\n    }}\n",
        cases.len()
    );
    if qt {
        s += &format!(
            "    Component.onCompleted: {{\n        fill();\n        \
             for (let i = 0; i < {}; i++) {{\n            \
             const e = this[\"item\" + i];\n            \
             const rects = [e.width, e.leftPadding, e.rightPadding];\n            \
             for (let p = 0; p <= e.length; p++) {{\n                \
             const r = e.positionToRectangle(p);\n                rects.push(r.x, r.y);\n            \
             }}\n            console.log(\"CASE\", i, rects.join(\" \"));\n        }}\n        \
             Qt.quit();\n    }}\n",
            cases.len()
        );
    }
    s += "}\n";
    s
}

fn qt_laid(cases: &[(String, String)]) -> Vec<Vec<f64>> {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("notes.qml");
    std::fs::write(&file, source(cases, true)).unwrap();
    let out = std::process::Command::new("qml6")
        .arg(&file)
        .env("QT_QPA_PLATFORM", "offscreen")
        .env("QT_QUICK_CONTROLS_STYLE", "Basic")
        .env("QT_FORCE_STDERR_LOGGING", "1")
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&out.stderr);
    let laid: Vec<Vec<f64>> = log
        .lines()
        .filter_map(|l| l.split_once("CASE ")?.1.split_once(' '))
        .map(|(_, rects)| {
            rects
                .split_whitespace()
                .map(|n| n.parse().unwrap())
                .collect()
        })
        .collect();
    assert_eq!(laid.len(), cases.len(), "qml6 printed:\n{log}");
    laid
}

fn our_laid(cases: &[(String, String)]) -> Vec<Vec<f64>> {
    let mut rt = Runtime::new(None);
    let fonts = std::rc::Rc::new(std::cell::RefCell::new(scene::Fonts::default()));
    {
        let fonts = fonts.clone();
        rt.world.borrow_mut().text_measure = Some(Box::new(move |w, i| {
            if let Ok(mut f) = fonts.try_borrow_mut() {
                scene::measure_text(w, &mut f, i);
            }
        }));
    }
    let root = rt.load_str(&source(cases, false)).unwrap();
    for _ in 0..2 {
        rt.settle();
        scene::layout_window(&mut rt, &mut fonts.borrow_mut(), root, [500.0, 500.0]);
        rt.settle();
        rt.call_method(root, "fill", Vec::new()).unwrap().unwrap();
    }
    let mut w = rt.world.borrow_mut();
    let mut fonts = fonts.borrow_mut();
    (0..cases.len())
        .map(|i| {
            let Some(Value::Item(e)) = w.read(root, &format!("item{i}")) else {
                panic!("item{i} is not an item");
            };
            let lines = scene::edit_geometry(&mut w, &mut fonts, e);
            let end = lines.last().map_or(0, |l| l.0 + l.3.len() - 1);
            let size = ["width", "leftPadding", "rightPadding"].map(|k| match w.read(e, k) {
                Some(Value::Num(n)) => n,
                _ => f64::NAN,
            });
            let carets = (0..=end).flat_map(|p| {
                let (start, y, _, xs) = lines.iter().rev().find(|l| l.0 <= p).unwrap();
                [xs[(p - start).min(xs.len() - 1)], *y]
            });
            size.into_iter().chain(carets).collect()
        })
        .collect()
}

/// The chars the lines start at: where y changes.
fn starts(rects: &[f64]) -> Vec<usize> {
    let ys: Vec<f64> = rects[3..].chunks(2).map(|r| r[1]).collect();
    (1..ys.len()).filter(|&p| ys[p] != ys[p - 1]).collect()
}

/// The fields that differ from Qt, each with its width, its line starts and its carets.
fn differing(cases: &[(String, String)]) -> Vec<String> {
    let (qt, ours) = (qt_laid(cases), our_laid(cases));
    let mut bad = Vec::new();
    for (i, (_, name)) in cases.iter().enumerate() {
        let (q, o) = (&qt[i], &ours[i]);
        let same = q.len() == o.len() && q.iter().zip(o).all(|(a, b)| (a - b).abs() < 1.0 / 64.0);
        if !same {
            bad.push(format!(
                "{name}\n  width, leftPadding, rightPadding: ours {:?}, Qt {:?}\n  \
                 lines start at: ours {:?}, Qt {:?}\n  x of the first 48 carets:\n    \
                 ours {:?}\n    Qt   {:?}",
                &o[..3],
                &q[..3],
                starts(o),
                starts(q),
                o[3..].chunks(2).take(48).map(|r| r[0]).collect::<Vec<_>>(),
                q[3..].chunks(2).take(48).map(|r| r[0]).collect::<Vec<_>>(),
            ));
        }
    }
    bad
}

#[test]
fn text_edit_with_paddings_wraps_where_qt_wraps_it() {
    if !testenv::require("qml6") {
        return;
    }
    let cases = alone();
    let bad = differing(&cases);
    assert!(
        bad.is_empty(),
        "{} of {} fields differ from Qt:\n{}",
        bad.len(),
        cases.len(),
        bad.join("\n")
    );
}

/// Qt makes the field 378 wide (TextArea.qml of Basic: the hint 326 and the paddings 52),
/// which leaves its lines 326. That takes both the content of a ScrollView being as wide as
/// the implicitWidth of its one child and the implicitWidth of a TextArea being that of
/// Basic: with neither the field is as wide as the view, 460, and its lines 408.
#[test]
fn notes_field_in_its_scroll_view_wraps_where_qt_wraps_it() {
    if !testenv::require("qml6") {
        return;
    }
    let cases = in_view();
    let bad = differing(&cases);
    assert!(
        bad.is_empty(),
        "{} of {} fields differ from Qt (Qt: width 378, lines 326 wide; ours until the content \
         of a ScrollView and the implicitWidth of a TextArea are Qt's: width 460, lines 408 \
         wide):\n{}",
        bad.len(),
        cases.len(),
        bad.join("\n")
    );
}

/// The text is in the document itself.
#[test]
fn notes_field_with_its_text_declared_wraps_where_qt_wraps_it() {
    if !testenv::require("qml6") {
        return;
    }
    let cases: Vec<_> = in_view()
        .into_iter()
        .map(|(item, name)| {
            let text = format!("text: \"{TEXT}\"; placeholderText");
            (
                item.replace("placeholderText", &text),
                format!("{name}, text declared"),
            )
        })
        .collect();
    let bad = differing(&cases);
    assert!(
        bad.is_empty(),
        "{} of {} fields differ from Qt:\n{}",
        bad.len(),
        cases.len(),
        bad.join("\n")
    );
}
