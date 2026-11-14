//! The QML type FontMetrics has the values Qt 6.12 gives it: its 15 properties and what its
//! methods advanceWidth, boundingRect, tightBoundingRect and elidedText (with and without
//! flags) return, in a bitmap-only font (one 16 px strike, as a BDF, a PCF and an OpenType
//! file: qsg/tests/fixtures/bitmap) and in ordinary ones, at several sizes and styles.
//!
//! Compared with Qt itself (`qml6`, offscreen): every value as JavaScript prints it, the
//! strings by their UTF-16 units.

use shell::scene;

/// The fonts of a family that are measured: the FontMetrics' properties after the family.
const FONTS: [&str; 7] = [
    "font.pixelSize: 16",
    "font.pixelSize: 11",
    "font.pixelSize: 24; font.bold: true",
    "font.pixelSize: 13; font.italic: true",
    "font.pointSize: 10",
    "font.pixelSize: 16; font.letterSpacing: 2",
    "font.pixelSize: 20; font.weight: 300",
];

/// What is asked of every font, as "name=value" joined by `|`: the properties, the methods
/// on every text, elidedText in the four modes at six widths and, for the text with variants
/// (U+009C) and the one with mnemonics, with Qt.TextShowMnemonic and Qt.TextLongestVariant.
const PROBE: &str = r#"
    function hex(s) {
        let o = [];
        for (let i = 0; i < s.length; i++)
            o.push(s.charCodeAt(i).toString(16));
        return o.join(".");
    }
    function rect(r) { return r.x + "," + r.y + "," + r.width + "," + r.height; }
    function probe(m) {
        let o = [];
        const props = ["ascent", "descent", "height", "leading", "lineSpacing",
            "minimumLeftBearing", "minimumRightBearing", "maximumCharacterWidth", "xHeight",
            "averageCharacterWidth", "underlinePosition", "overlinePosition",
            "strikeOutPosition", "lineWidth", "capitalHeight"];
        for (let p = 0; p < props.length; p++)
            o.push(props[p] + "=" + m[props[p]]);
        const texts = ["", "A", "ABWijm", "Hello, World", "fjord (Wy) _f",
            "\u041f\u0440\u0438\u0432\u0435\u0442 \u043c\u0438\u0440", "A\ud83d\ude00B \u2318",
            "ABW\u009cABWijm ABWijm\u009cAB", "&ABW && i&jm", "e\u0301a\u0308o Wij",
            "\u05e9\u05dc\u05d5\u05dd abc", "\u0633\u0644\u0627\u0645 \u0639\u0644\u064a\u0643\u0645",
            "\u0628\u064e\u064a\u0652\u062a\u064c \u0643\u064e\u0628\u0650\u064a\u0631\u064c",
            "\u072b\u0720\u0721\u0710 \u0725\u0720\u0721\u0710",
            "\u0928\u092e\u0938\u094d\u0924\u0947 \u0926\u0941\u0928\u093f\u092f\u093e 12",
            "\u179f\u17bd\u179f\u17d2\u178f\u17b8 \u1796\u17b7\u1797\u1796",
            "\u07d2\u07de\u07cf \u07de\u07ca\u07f2, A", "office affluent fjfi"];
        const widths = [0, 5, 20, 40, 70, 1000];
        for (let t = 0; t < texts.length; t++) {
            const s = texts[t];
            o.push("advanceWidth(" + t + ")=" + m.advanceWidth(s));
            o.push("boundingRect(" + t + ")=" + rect(m.boundingRect(s)));
            o.push("tightBoundingRect(" + t + ")=" + rect(m.tightBoundingRect(s)));
            for (let mode = 0; mode < 4; mode++)
                for (let k = 0; k < widths.length; k++)
                    o.push("elidedText(" + t + "," + mode + "," + widths[k] + ")="
                        + hex(m.elidedText(s, mode, widths[k])));
        }
        const flags = [0x800, 0x80000, 0x80800];
        for (let t = 7; t < 9; t++)
            for (let f = 0; f < flags.length; f++)
                for (let mode = 0; mode < 3; mode++)
                    for (let k = 0; k < widths.length; k++)
                        o.push("elidedText(" + t + "," + mode + "," + widths[k] + "," + flags[f]
                            + ")=" + hex(m.elidedText(texts[t], mode, widths[k], flags[f])));
        return o.join("|");
    }
"#;

/// A FontMetrics for each of `FONTS` in `family`, and what `PROBE` gives for it.
fn source(family: &str) -> String {
    let mut s = format!("import QtQuick\nItem {{\n    id: root\n{PROBE}");
    for (i, font) in FONTS.into_iter().enumerate() {
        s += &format!(
            "    FontMetrics {{ id: m{i}; font.family: \"{family}\"; {font} }}\n    \
             property string r{i}: probe(m{i})\n"
        );
    }
    s
}

type Measured = Vec<Vec<String>>;

fn values(line: &str) -> Vec<String> {
    line.split('|').map(str::to_string).collect()
}

/// Every font's values under Qt.
fn qt(source: &str) -> Measured {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("metrics.qml");
    let source = format!(
        "{source}    Timer {{\n        interval: 50; running: true\n        \
         onTriggered: {{\n            \
         for (let i = 0; i < {fonts}; i++)\n                \
         console.log(\"FONT \" + root[\"r\" + i]);\n            \
         Qt.quit();\n        }}\n    }}\n}}\n",
        fonts = FONTS.len()
    );
    std::fs::write(&file, source).unwrap();
    let out = std::process::Command::new("qml6")
        .arg(&file)
        .env("QT_QPA_PLATFORM", "offscreen")
        .env("QT_FORCE_STDERR_LOGGING", "1")
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&out.stderr);
    let measured: Measured = log
        .lines()
        .filter_map(|l| l.split_once("FONT ").map(|l| values(l.1)))
        .collect();
    assert_eq!(measured.len(), FONTS.len(), "qml6 printed:\n{log}");
    measured
}

/// The same under our engine, with the fonts measured when read, as the shell sets it up.
fn ours(source: &str) -> Measured {
    let mut rt = qml::runtime::Runtime::new(None);
    let fonts = std::rc::Rc::new(std::cell::RefCell::new(scene::Fonts::default()));
    {
        let mut w = rt.world.borrow_mut();
        let f = fonts.clone();
        w.text_measure = Some(Box::new(move |w, i| {
            scene::measure_text(w, &mut f.borrow_mut(), i);
        }));
        let f = fonts.clone();
        w.font_method = Some(Box::new(move |w, i, name, args| {
            scene::font_method(w, &mut f.borrow_mut(), i, name, args)
        }));
    }
    let root = rt.load_str(&format!("{source}}}\n")).unwrap();
    rt.settle();
    let mut w = rt.world.borrow_mut();
    (0..FONTS.len())
        .map(|i| {
            let line = w.read(root, &format!("r{i}"));
            values(&line.map_or(String::new(), |v| v.to_js_string()))
        })
        .collect()
}

/// Every value of every font of `family` is Qt's. `first` is what a bitmap font's first
/// FontMetrics has in Qt: the family asked for is the one compared, not what fontconfig gives
/// in its place.
fn compare(family: &str, first: Option<&[&str]>) {
    let source = source(family);
    let (qt, ours) = (qt(&source), ours(&source));
    if let Some(first) = first {
        assert_eq!(&qt[0][..first.len()], first, "Qt did not take {family}");
    }
    let (mut count, mut wrong) = (0, Vec::new());
    for (i, font) in FONTS.into_iter().enumerate() {
        assert!(qt[i].len() > 300, "Qt gave {font} {} values", qt[i].len());
        count += qt[i].len();
        if qt[i].len() != ours[i].len() {
            wrong.push(format!(
                "{font}: {} values, Qt has {}",
                ours[i].len(),
                qt[i].len()
            ));
        }
        for (qt, ours) in qt[i].iter().zip(&ours[i]) {
            if qt != ours {
                wrong.push(format!("{font}: {ours}, Qt has {qt}"));
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "{family}: {} of {count} values differ from Qt:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}

/// `compare` in a process whose fontconfig has the fixture `file` and no other user font.
fn compare_bitmap(test: &str, file: &str, family: &str) {
    if !testenv::require("qml6") {
        return;
    }
    if !testenv::in_own_process(test) {
        let dir = tempfile::tempdir().unwrap();
        let (data, cache) = (dir.path().join("data"), dir.path().join("cache"));
        std::fs::create_dir_all(data.join("fonts")).unwrap();
        let fixture = format!(
            "{}/../qsg/tests/fixtures/bitmap/{file}",
            env!("CARGO_MANIFEST_DIR")
        );
        std::fs::copy(fixture, data.join("fonts").join(file)).unwrap();
        testenv::run_in_own_process(test, [("XDG_DATA_HOME", data), ("XDG_CACHE_HOME", cache)]);
        return;
    }
    // The strike's own ascent 12 and descent 4.
    compare(family, Some(&["ascent=12", "descent=4", "height=16"]));
}

#[test]
fn bitmap_font_bdf_matches_qt() {
    const TEST: &str = concat!(module_path!(), "::bitmap_font_bdf_matches_qt");
    compare_bitmap(TEST, "impulse-bitmap-test.bdf", "Impulse Bitmap Test");
}

/// FreeType names a PCF file's family with its foundry first.
#[test]
fn bitmap_font_pcf_matches_qt() {
    const TEST: &str = concat!(module_path!(), "::bitmap_font_pcf_matches_qt");
    compare_bitmap(
        TEST,
        "impulse-bitmap-test.pcf",
        "impulse Impulse Bitmap Test",
    );
}

#[test]
fn bitmap_font_otb_matches_qt() {
    const TEST: &str = concat!(module_path!(), "::bitmap_font_otb_matches_qt");
    compare_bitmap(TEST, "impulse-bitmap-test.otb", "Impulse Bitmap Test");
}

fn compare_outline(family: &str) {
    if testenv::require("qml6") {
        compare(family, None);
    }
}

#[test]
fn noto_sans_matches_qt() {
    compare_outline("Noto Sans");
}

#[test]
fn noto_serif_matches_qt() {
    compare_outline("Noto Serif");
}

#[test]
fn dejavu_sans_mono_matches_qt() {
    compare_outline("DejaVu Sans Mono");
}
