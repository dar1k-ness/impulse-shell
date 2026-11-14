//! The text field of Notes (`NotesContent.qml`: a `StyledTextArea` with `padding: 24` and a
//! hint, anchored to the sides of a ScrollView 460 wide) when its text is there before the
//! overlay was ever laid out, as when Notes is opened again and `FileView.onLoaded` fills it.
//! Compared with Qt itself (`qml6`, offscreen) on the fonts of the machine the test runs on:
//! the width of the field and the chars its lines start at. In Qt the field is as wide as its
//! hint (378) and wraps: a TextArea is attached to the Flickable of its ScrollView, and the
//! Flickable has taken its implicit size, before any Component.onCompleted runs. Ours did that
//! at the first layout, with the text already there, and the field was as wide as the view.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::scene;

const TEXT: &str = "The quick brown fox jumps over the lazy dog and keeps running through \
    the long field until the line wraps twice or more";

/// The longest line of the hint of Notes: it is what makes the empty field 378 wide in Qt.
const HINT: &str = "Write something here...\\nUse '-' to create copyable bullet points, like this:";

const FAMILIES: [&str; 2] = ["Google Sans Flex", "Noto Sans"];

/// One field for each family, with what `StyledTextArea` sets.
fn source(qt: bool) -> String {
    let mut s = "import QtQuick\nimport QtQuick.Controls\nItem {\n    id: root\n    \
                 width: 500; height: 500\n"
        .to_string();
    for (i, family) in FAMILIES.iter().enumerate() {
        s += &format!(
            "    property var item{i}: e{i}\n    \
             ScrollView {{ width: 460; height: 460; clip: true; \
             ScrollBar.vertical.policy: ScrollBar.AsNeeded; \
             TextArea {{ id: e{i}; anchors.left: parent.left; anchors.right: parent.right; \
             wrapMode: TextEdit.Wrap; background: null; padding: 24; \
             font.family: \"{family}\"; font.pixelSize: 15; \
             font.hintingPreference: Font.PreferFullHinting; \
             renderType: TextEdit.NativeRendering; \
             font.variableAxes: ({{ \"wdth\": 100, \"wght\": 450 }}); \
             placeholderText: \"{HINT}\" }} }}\n"
        );
    }
    s += &format!(
        "    Component.onCompleted: {{\n        \
         for (let i = 0; i < {}; i++) root[\"item\" + i].text = \"{TEXT}\";\n    }}\n",
        FAMILIES.len()
    );
    if qt {
        // Read after the window was polished and drawn, not in the handler that set the text.
        s += &format!(
            "    Timer {{\n        interval: 200; running: true\n        onTriggered: {{\n            \
             for (let i = 0; i < {}; i++) {{\n                \
             const e = root[\"item\" + i];\n                const out = [e.width];\n                \
             let y = e.positionToRectangle(0).y;\n                \
             for (let p = 1; p <= e.length; p++) {{\n                    \
             const r = e.positionToRectangle(p);\n                    \
             if (r.y !== y) out.push(p);\n                    y = r.y;\n                }}\n                \
             console.log(\"CASE\", i, out.join(\" \"));\n            }}\n            \
             Qt.quit();\n        }}\n    }}\n",
            FAMILIES.len()
        );
    }
    s += "}\n";
    s
}

/// For each field: its width, then the chars its lines after the first start at.
fn qt_laid() -> Vec<Vec<f64>> {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("notes.qml");
    std::fs::write(&file, source(true)).unwrap();
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
        .map(|(_, v)| v.split_whitespace().map(|n| n.parse().unwrap()).collect())
        .collect();
    assert_eq!(laid.len(), FAMILIES.len(), "qml6 printed:\n{log}");
    laid
}

fn our_laid() -> Vec<Vec<f64>> {
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
    // As the shell: sizes that Qt has when a component is complete, before its handlers run.
    {
        let fonts = fonts.clone();
        rt.world.borrow_mut().pre_complete = Some(Box::new(move |w, n0| {
            fonts
                .try_borrow_mut()
                .is_ok_and(|mut f| scene::measure_new(w, &mut f, n0))
        }));
    }
    let root = rt.load_str(&source(false)).unwrap();
    for _ in 0..3 {
        rt.settle();
        scene::layout_window(&mut rt, &mut fonts.borrow_mut(), root, [500.0, 500.0]);
    }
    rt.settle();
    let mut w = rt.world.borrow_mut();
    let mut fonts = fonts.borrow_mut();
    (0..FAMILIES.len())
        .map(|i| {
            let Some(Value::Item(e)) = w.read(root, &format!("item{i}")) else {
                panic!("item{i} is not an item");
            };
            let Some(Value::Num(width)) = w.read(e, "width") else {
                panic!("item{i} has no width");
            };
            let lines = scene::edit_geometry(&mut w, &mut fonts, e);
            let starts = lines.iter().skip(1).map(|l| l.0 as f64);
            std::iter::once(width).chain(starts).collect()
        })
        .collect()
}

#[test]
fn notes_field_filled_before_the_first_layout_is_as_wide_and_wraps_as_in_qt() {
    if !testenv::require("qml6") {
        return;
    }
    let (qt, ours) = (qt_laid(), our_laid());
    let bad: Vec<String> = (0..FAMILIES.len())
        .filter(|&i| qt[i] != ours[i])
        .map(|i| {
            format!(
                "{}: width, then the chars its lines start at: ours {:?}, Qt {:?}",
                FAMILIES[i], ours[i], qt[i]
            )
        })
        .collect();
    assert!(
        bad.is_empty(),
        "{} of {} fields differ from Qt:\n{}",
        bad.len(),
        FAMILIES.len(),
        bad.join("\n")
    );
}
