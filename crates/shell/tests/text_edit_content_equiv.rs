//! `contentWidth` and `lineCount` of TextEdit and TextArea compared with Qt itself (`qml6`,
//! offscreen, style Basic) on the fonts of the machine the test runs on: the width of the
//! widest line as the editor wraps it, not rounded, and the number of its lines; an empty
//! editor has one line and no width, whatever its hint. And the width of the hint of a
//! TextArea, which Basic's TextArea.qml adds its paddings to for its `implicitWidth`.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::scene;

const TEXT: &str = "The quick brown fox jumps over the lazy dog and keeps running through \
                    the field until the evening comes.";
const FONT: &str = "font.family: \"Noto Sans\"; font.pixelSize: 15";

/// Each editor as QML, without its id and font.
const CASES: [&str; 23] = [
    "TextEdit {}",
    "TextEdit { text: \"Hi\" }",
    "TextEdit { text: TEXT }",
    "TextEdit { width: 200; wrapMode: TextEdit.Wrap; text: TEXT }",
    "TextEdit { width: 200; wrapMode: TextEdit.WordWrap; leftPadding: 28; rightPadding: 24; \
     text: TEXT }",
    "TextEdit { width: 200; wrapMode: TextEdit.WrapAnywhere; text: TEXT }",
    "TextEdit { width: 200; wrapMode: TextEdit.Wrap; text: \"one\\ntwo three\\n\" }",
    "TextEdit { width: 200; wrapMode: TextEdit.Wrap; text: \"\\n\\n\" }",
    "TextEdit { width: 120; wrapMode: TextEdit.Wrap; text: \"שלום עולם, מה שלומך היום בבוקר\" }",
    "TextEdit { width: 200; wrapMode: TextEdit.Wrap }",
    "TextEdit { width: 300; wrapMode: TextEdit.Wrap; text: TEXT }",
    "TextArea {}",
    "TextArea { placeholderText: \"Write something here\" }",
    "TextArea { text: \"Hi\"; placeholderText: \"Write something here\" }",
    "TextArea { text: TEXT }",
    "TextArea { width: 200; wrapMode: TextEdit.Wrap; text: TEXT }",
    "TextArea { width: 200; wrapMode: TextEdit.Wrap; padding: 14; text: TEXT; \
     placeholderText: \"Write something here\" }",
    "TextArea { width: 200; wrapMode: TextEdit.Wrap; padding: 24; \
     placeholderText: \"Write something here\" }",
    // Tabs: each as wide as the way to the next tab stop; a line breaks after one that fits
    // and before one that does not.
    "TextEdit { text: \"a\\tb\\tc\" }",
    "TextEdit { width: 200; wrapMode: TextEdit.WordWrap; \
     text: \"one two three\\tfour five\\tsix seven eight\\tnine\" }",
    "TextEdit { width: 120; wrapMode: TextEdit.WordWrap; \
     text: \"\\tif x:\\n\\t\\treturn fairly(long)\\t# done\\t\" }",
    "TextArea { width: 200; wrapMode: TextEdit.Wrap; \
     text: \"def f(x):\\n\\tif x:\\n\\t\\treturn 1\\n\\treturn 0\" }",
    "TextArea { text: \"a\\tb\"; placeholderText: \"Write something here\" }",
];

/// `step()` gives every editor that has no text the sentence and makes the one 300 wide 120.
fn source(qt: bool) -> String {
    let mut s = "import QtQuick\nimport QtQuick.Controls\nItem {\n    width: 800; height: 600\n"
        .to_string();
    for (i, case) in CASES.iter().enumerate() {
        let item = case
            .replacen('{', &format!("{{ id: e{i}; {FONT}; "), 1)
            .replace("TEXT", &format!("\"{TEXT}\""));
        s += &format!("    property var item{i}: e{i}\n    {item}\n");
    }
    let n = CASES.len();
    s += &format!(
        "    function step() {{\n        for (let i = 0; i < {n}; i++) {{\n            \
         const e = this[\"item\" + i];\n            if (e.text === \"\") e.text = \"{TEXT}\";\n            \
         if (e.width === 300) e.width = 120;\n        }}\n    }}\n"
    );
    if qt {
        s += &format!(
            "    function report(tag) {{\n        const out = [];\n        \
             for (let i = 0; i < {n}; i++) {{\n            const e = this[\"item\" + i];\n            \
             out.push(e.contentWidth, e.lineCount);\n        }}\n        \
             console.log(tag, out.join(\" \"));\n    }}\n    \
             Component.onCompleted: {{ report(\"LOADED\"); step(); report(\"STEPPED\"); Qt.quit(); }}\n"
        );
    }
    s += "}\n";
    s
}

/// contentWidth and lineCount of every editor, as loaded and then after `step()`.
fn qt_content() -> Vec<f64> {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("content.qml");
    std::fs::write(&file, source(true)).unwrap();
    let out = std::process::Command::new("qml6")
        .arg(&file)
        .env("QT_QPA_PLATFORM", "offscreen")
        .env("QT_QUICK_CONTROLS_STYLE", "Basic")
        .env("QT_FORCE_STDERR_LOGGING", "1")
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&out.stderr);
    let content: Vec<f64> = ["LOADED ", "STEPPED "]
        .iter()
        .filter_map(|tag| log.lines().find_map(|l| l.split_once(tag)))
        .flat_map(|(_, v)| v.split_whitespace().map(|n| n.parse().unwrap()))
        .collect();
    assert_eq!(content.len(), CASES.len() * 4, "qml6 printed:\n{log}");
    content
}

fn our_content() -> Vec<f64> {
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
    let root = rt.load_str(&source(false)).unwrap();
    let mut content = Vec::new();
    for stepped in [false, true] {
        if stepped {
            rt.call_method(root, "step", Vec::new()).unwrap().unwrap();
        }
        rt.settle();
        scene::layout_window(&mut rt, &mut fonts.borrow_mut(), root, [800.0, 600.0]);
        rt.settle();
        let mut w = rt.world.borrow_mut();
        for i in 0..CASES.len() {
            let Some(Value::Item(e)) = w.read(root, &format!("item{i}")) else {
                panic!("item{i} is not an item");
            };
            for k in ["contentWidth", "lineCount"] {
                content.push(w.read(e, k).map_or(f64::NAN, |v| v.to_number()));
            }
        }
    }
    content
}

#[test]
fn content_width_and_line_count_of_text_edit_are_qt_s() {
    if !testenv::require("qml6") {
        return;
    }
    let (qt, ours) = (qt_content(), our_content());
    let wrong: Vec<String> = qt
        .chunks(2)
        .zip(ours.chunks(2))
        .enumerate()
        .filter(|(_, (q, o))| {
            q.iter()
                .zip(*o)
                .any(|(a, b)| a.is_nan() != b.is_nan() || (a - b).abs() >= 1.0 / 64.0)
        })
        .map(|(n, (q, o))| {
            format!(
                "{}{}: contentWidth ours {}, Qt {}; lineCount ours {}, Qt {}",
                CASES[n % CASES.len()],
                if n < CASES.len() {
                    ""
                } else {
                    ", after step()"
                },
                o[0],
                q[0],
                o[1],
                q[1]
            )
        })
        .collect();
    assert!(
        wrong.is_empty(),
        "{} of {} editors differ from Qt:\n{}",
        wrong.len(),
        qt.len() / 2,
        wrong.join("\n")
    );
}

/// Hints, each for an empty TextArea and for one with text.
const HINTS: [&str; 4] = [
    "Write something here",
    "Write something here...\\nUse '-' to create copyable bullet points, like this:",
    "שלום עולם 123",
    "",
];

fn hints(qt: bool) -> String {
    let mut s = "import QtQuick\nimport QtQuick.Controls\nItem {\n".to_string();
    let texts = ["", "Hi"];
    for (i, (hint, text)) in HINTS.iter().flat_map(|h| texts.map(|t| (h, t))).enumerate() {
        s += &format!(
            "    property var item{i}: e{i}\n    TextArea {{ id: e{i}; {FONT}; background: null; \
             text: \"{text}\"; placeholderText: \"{hint}\" }}\n"
        );
    }
    if qt {
        s += &format!(
            "    Component.onCompleted: {{\n        const out = [];\n        \
             for (let i = 0; i < {}; i++) {{\n            const e = this[\"item\" + i];\n            \
             out.push(e.implicitWidth - e.leftPadding - e.rightPadding);\n        }}\n        \
             console.log(\"HINTS\", out.join(\" \"));\n        Qt.quit();\n    }}\n",
            HINTS.len() * 2
        );
    }
    s += "}\n";
    s
}

/// Without a background the implicitWidth of Basic's TextArea is the wider of its text and
/// its hint, with its paddings: the hint here is wider than "Hi".
#[test]
fn placeholder_width_is_what_qt_adds_the_paddings_to() {
    if !testenv::require("qml6") {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("hints.qml");
    std::fs::write(&file, hints(true)).unwrap();
    let out = std::process::Command::new("qml6")
        .arg(&file)
        .env("QT_QPA_PLATFORM", "offscreen")
        .env("QT_QUICK_CONTROLS_STYLE", "Basic")
        .env("QT_FORCE_STDERR_LOGGING", "1")
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&out.stderr);
    let qt: Vec<f64> = log
        .lines()
        .find_map(|l| l.split_once("HINTS "))
        .map(|(_, v)| v.split_whitespace().map(|n| n.parse().unwrap()).collect())
        .unwrap_or_default();
    assert_eq!(qt.len(), HINTS.len() * 2, "qml6 printed:\n{log}");

    let mut rt = Runtime::new(None);
    let mut fonts = scene::Fonts::default();
    let root = rt.load_str(&hints(false)).unwrap();
    rt.settle();
    let mut w = rt.world.borrow_mut();
    let mut wrong = Vec::new();
    for (i, qt) in qt.iter().enumerate() {
        let Some(Value::Item(e)) = w.read(root, &format!("item{i}")) else {
            panic!("item{i} is not an item");
        };
        let ours = scene::placeholder_width(&mut w, &mut fonts, e);
        // An empty hint: Qt's width is that of the text.
        let qt = if HINTS[i / 2].is_empty() { 0.0 } else { *qt };
        if (qt - ours).abs() >= 1.0 / 64.0 {
            wrong.push(format!(
                "placeholderText {:?}, {}: ours {ours}, Qt {qt}",
                HINTS[i / 2],
                if i % 2 == 0 { "empty" } else { "with text" }
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "{} of {} hints differ from Qt:\n{}",
        wrong.len(),
        qt.len(),
        wrong.join("\n")
    );
}
