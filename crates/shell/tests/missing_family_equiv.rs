//! Text whose `font.family` is not installed, compared with Qt itself (`qml6`, offscreen) on
//! the fonts of the machine the test runs on: `implicitWidth` of every text in every family.
//! Qt finds a font for each script of the text on its own (QFontPrivate::engineForScript):
//! with the family missing, QFontDatabasePrivate::findFont takes the first of the fallback
//! families fontconfig sorts for that family and the script's language, and that font sets
//! everything in the item of the script: the digits and spaces after Arabic or Hebrew letters
//! too.

use qml::runtime::Runtime;
use shell::scene;

/// Not installed anywhere; the family the shell's configuration asks for, installed or not;
/// one the test machine has.
const FAMILIES: [&str; 5] = [
    "No Such Family 7f3a",
    "Google Sans Flex",
    "Noto Sans",
    "Sans Serif",
    "sans-serif",
];

const TEXTS: [&str; 22] = [
    "plain latin 123",
    "123 456",
    "abc مرحبا بالعالم 123 и",
    "abc مرحبا 123",
    "abc 123 مرحبا",
    "مرحبا 123",
    "مرحبا (12.5) بالعالم",
    "שלום 123 abc",
    "שלום 123",
    "שלום, עולם: 45.6!",
    "мир 123",
    "привет, мир",
    "αβγ 123 δ",
    "abc שלום مرحبا 7 мир 8 αβγ 9",
    "ไทย 123 abc",
    "हिन्दी 123 abc",
    "বাংলা 45, abc",
    "հայերեն 123",
    "ქართული 45.6",
    "中文 123 日本語",
    "한국어 123",
    "abc 12 ไทย 34 हिन्दी 56 مرحبا 78",
];

fn source() -> String {
    let mut s = String::from("import QtQuick\nItem {\n    width: 400; height: 300\n");
    let mut n = 0;
    for family in FAMILIES {
        for text in TEXTS {
            s += &format!(
                "    property real w{n}: t{n}.implicitWidth\n    Text {{ id: t{n}; font.family: \
                 \"{family}\"; font.pixelSize: 15; text: \"{text}\" }}\n"
            );
            n += 1;
        }
    }
    s += &format!(
        "    Component.onCompleted: {{\n        const out = [];\n        for (let i = 0; i < \
         {n}; i++)\n            out.push(this[\"w\" + i]);\n        console.log(\"WIDTHS\", \
         out.join(\" \"));\n        Qt.quit();\n    }}\n}}\n"
    );
    s
}

fn qt_widths() -> Vec<f64> {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("family.qml");
    std::fs::write(&file, source()).unwrap();
    let out = std::process::Command::new("qml6")
        .arg(&file)
        .env("QT_QPA_PLATFORM", "offscreen")
        .env("QT_FORCE_STDERR_LOGGING", "1")
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&out.stderr);
    let widths: Vec<f64> = log
        .lines()
        .find_map(|l| l.split_once("WIDTHS "))
        .map(|(_, w)| w.split_whitespace().map(|n| n.parse().unwrap()).collect())
        .unwrap_or_default();
    assert_eq!(
        widths.len(),
        FAMILIES.len() * TEXTS.len(),
        "qml6 printed:\n{log}"
    );
    widths
}

fn our_widths() -> Vec<f64> {
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
    let root = rt.load_str(&source()).unwrap();
    rt.settle();
    scene::layout_window(&mut rt, &mut fonts.borrow_mut(), root, [400.0, 300.0]);
    rt.settle();
    let mut w = rt.world.borrow_mut();
    (0..FAMILIES.len() * TEXTS.len())
        .map(|n| {
            w.read(root, &format!("w{n}"))
                .map_or(f64::NAN, |v| v.to_number())
        })
        .collect()
}

#[test]
fn text_in_a_missing_family_is_as_wide_as_in_qt() {
    if !testenv::require("qml6") {
        return;
    }
    let qt = qt_widths();
    let ours = our_widths();
    let wrong: Vec<String> = qt
        .iter()
        .zip(&ours)
        .enumerate()
        .filter(|(_, (qt, ours))| (*qt - *ours).abs() >= 1.0 / 64.0)
        .map(|(n, (qt, ours))| {
            format!(
                "font.family {:?}, text {:?}: ours {ours}, Qt {qt}",
                FAMILIES[n / TEXTS.len()],
                TEXTS[n % TEXTS.len()]
            )
        })
        .collect();
    assert!(
        wrong.is_empty(),
        "{} of {} texts differ from Qt (implicitWidth):\n{}",
        wrong.len(),
        qt.len(),
        wrong.join("\n")
    );
}
