//! A font weight that no face of the family has takes the face Qt takes. QFontDatabase counts the
//! weight distance in whole tens and keeps the first face it listed among the closest ones
//! (`bestStyle`), so 450 on a family with Regular and Medium is whichever fontconfig lists first:
//! Medium for Noto Sans in ri-testenv. Ours took the lighter one, and the original's "Файл настроек" button
//! label (weight 450) came out 2 px narrower than under Quickshell, which narrowed the settings
//! side column. A family that lacks the text's script hands it to a fallback family, which Qt
//! asks for the face named as the one it picked (QFontEngineFT::init puts FT_Face::style_name
//! into the fontDef the fallback request is copied from) and, when the fallback family has none
//! of that name, for the requested weight. For a fixed-pitch family Qt lists the fallback families
//! with monospace ones first (QFontEngineMulti::ensureFallbackFamiliesQueried).
//! Compared with Qt itself (`qml6`, offscreen) on the fonts of the machine the test runs on.

use qml::runtime::Runtime;
use shell::scene;

/// The original's default family (a variable font; not installed in ri-testenv, where fontconfig gives
/// Noto Sans), static families, one without Cyrillic (the label goes to a fallback family),
/// one that exists only as a variable font, and fixed-pitch ones, whose fallback families Qt
/// lists with monospace ones first: the original's icon font (only a variable font, no letters of the
/// label) and two monospace families.
const FAMILIES: [&str; 9] = [
    "Google Sans Flex",
    "Noto Sans",
    "Noto Serif",
    "Noto Sans Mono",
    "Noto Kufi Arabic",
    "Adwaita Sans",
    "Material Symbols Rounded",
    "Adwaita Mono",
    "DejaVu Sans Mono",
];
/// The button's label; its English text; letters (U+052E, U+052F) that Adwaita Sans and Google
/// Sans Flex lack and Noto Sans has: those go to Noto Sans, which in ri-testenv has no
/// ExtraLight, SemiBold or ExtraBold face to match the variable font's instance by name; and
/// symbols (U+2318, U+283F) that Noto Sans Mono lacks and other families have, monospace ones
/// among them.
const TEXTS: [&str; 4] = ["Файл настроек", "Config file", "Ԯайл Ԯԯ", "⌘ ⠿"];
/// Without font.variableAxes, and with the original's `Appearance.font.variableAxes.main`, which the
/// button's label carries.
const AXES: [&str; 2] = [
    "",
    "; font.variableAxes: ({ \"wght\": 450, \"wdth\": 100 })",
];

/// (family, text, weight, font.variableAxes)
type Case = (&'static str, &'static str, i32, &'static str);

fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for family in FAMILIES {
        for text in TEXTS {
            for axes in AXES {
                cases.extend((2..=18).map(|w| (family, text, w * 50, axes)));
            }
        }
    }
    cases
}

/// One Text and one TextMetrics per case, as the original's FloatingActionButton measures its label.
fn source() -> String {
    let mut s = String::from("import QtQuick\nItem {\n    id: root\n");
    for (i, (family, text, weight, axes)) in cases().into_iter().enumerate() {
        s += &format!(
            "    Text {{ id: t{i}; text: \"{text}\"; font.family: \"{family}\"; \
             font.pixelSize: 14; font.weight: {weight}{axes} }}\n    \
             TextMetrics {{ id: m{i}; font: t{i}.font; text: t{i}.text }}\n    \
             property real iw{i}: t{i}.implicitWidth\n    \
             property real adv{i}: m{i}.advanceWidth\n"
        );
    }
    s += &format!(
        "    Component.onCompleted: {{\n        for (let i = 0; i < {}; i++)\n            \
         console.log(\"WIDTH\", i, root[\"iw\" + i], root[\"adv\" + i]);\n        Qt.quit();\n    }}\n}}\n",
        cases().len()
    );
    s
}

/// (implicitWidth, advanceWidth) per case under Qt.
fn qt_widths(source: &str) -> Vec<(f64, f64)> {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("weights.qml");
    std::fs::write(&file, source).unwrap();
    let out = std::process::Command::new("qml6")
        .arg(&file)
        .env("QT_QPA_PLATFORM", "offscreen")
        .env("QT_FORCE_STDERR_LOGGING", "1")
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&out.stderr);
    let widths: Vec<(f64, f64)> = log
        .lines()
        .filter_map(|l| l.split_once("WIDTH ")?.1.split_once(' ')?.1.split_once(' '))
        .map(|(iw, adv)| (iw.parse().unwrap(), adv.parse().unwrap()))
        .collect();
    assert_eq!(widths.len(), cases().len(), "qml6 printed:\n{log}");
    widths
}

/// The same under our engine, with text measured when written or read, as the shell sets it up.
fn our_widths(source: &str) -> Vec<(f64, f64)> {
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
    let root = rt.load_str(source).unwrap();
    rt.settle();
    scene::layout_window(&mut rt, &mut fonts.borrow_mut(), root, [400.0, 300.0]);
    rt.settle();
    let mut w = rt.world.borrow_mut();
    let mut num = |name: String| w.read(root, &name).map_or(f64::NAN, |v| v.to_number());
    (0..cases().len())
        .map(|i| (num(format!("iw{i}")), num(format!("adv{i}"))))
        .collect()
}

#[test]
fn label_widths_match_qt_at_every_weight() {
    if !testenv::require("qml6") {
        return;
    }
    let source = source();
    let (qt, ours) = (qt_widths(&source), our_widths(&source));
    let wrong: Vec<String> = cases()
        .into_iter()
        .zip(qt.iter().zip(&ours))
        .filter(|(_, (qt, ours))| qt != ours)
        .map(|((family, text, weight, axes), (qt, ours))| {
            format!(
                "{family} {weight} {text:?}{axes}: (implicitWidth, advanceWidth) {ours:?}, \
                 Qt has {qt:?}"
            )
        })
        .collect();
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
