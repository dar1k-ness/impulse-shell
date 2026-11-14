//! An Image's relative `source` is loaded from beside the QML document the Image is written
//! in (QQuickImageBase::loadPixmap: `qmlContext(this)->resolvedUrl(source)`), and reads back
//! as written; the file is the url's `toLocalFile()`, its escapes decoded and its query and
//! fragment dropped. The statuses are the ones Quickshell 0.3.2 on Qt 6.12 gives the same
//! files.

use qml::runtime::Runtime;
use shell::scene;

const SHELL: &str = r#"import QtQuick
import "sub"
Item {
    width: 100; height: 100
    property var statuses: [
        beside.status, below.status, dotted.status, missing.status,
        thing.status, thing.inner, named.status, misnamed.status, loaded.item.status,
        made.item.status, absolute.status,
    ]
    property string written: beside.source
    property real beside_width: beside.implicitWidth
    Image { id: beside; source: "big.png" }
    Image { id: below; source: "sub/pic.png" }
    Image { id: dotted; source: "./big.png" }
    Image { id: missing; source: "missing.png" }
    // The root of sub/Thing.qml is created for this document: its `pic.png` is looked for
    // here, where there is none.
    Thing { id: thing }
    Thing { id: named; source: "big.png" }
    Thing { id: misnamed; source: "pic.png" }
    Loader { id: loaded; sourceComponent: Component { Image { source: "big.png" } } }
    // A document a Loader creates itself: its root is written in that document.
    Loader { id: made; source: "sub/Pic.qml" }
    Image { id: absolute; source: "ABSOLUTE" }
}
"#;

const THING: &str = r#"import QtQuick
Image {
    source: "pic.png"
    property alias inner: inner.status
    Image { id: inner; source: "../big.png" }
}
"#;

const PIC: &str = r#"import QtQuick
Image { source: "pic.png" }
"#;

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn relative_source_is_read_from_beside_its_document() {
    let tmp = tempfile::tempdir().unwrap();
    // A directory whose name a url escapes.
    let dir = tmp.path().join("my конфиг");
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    let png = |name: &str, w: u32| {
        image::RgbImage::from_pixel(w, 3, image::Rgb([0xc0, 0x80, 0x40]))
            .save(dir.join(name))
            .unwrap();
    };
    png("big.png", 4);
    png("sub/pic.png", 2);
    let absolute = dir.join("sub/pic.png");
    let shell = SHELL.replace("ABSOLUTE", absolute.to_str().unwrap());
    std::fs::write(dir.join("shell.qml"), shell).unwrap();
    std::fs::write(dir.join("sub/Thing.qml"), THING).unwrap();
    std::fs::write(dir.join("sub/Pic.qml"), PIC).unwrap();

    let mut rt = Runtime::new(Some(dir.clone()));
    let root = rt.load_file(&dir.join("shell.qml")).unwrap();
    rt.settle();
    let mut fonts = scene::Fonts::default();
    scene::layout_window(&mut rt, &mut fonts, root, [100.0, 100.0]);
    rt.settle();
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());

    const READY: f64 = 1.0;
    const ERROR: f64 = 3.0;
    let statuses = rt.get(root, "statuses").to_js_string();
    let want = [
        READY, READY, READY, ERROR, ERROR, READY, READY, ERROR, READY, READY, READY,
    ]
    .map(|s| s.to_string())
    .join(",");
    assert_eq!(statuses, want);
    assert_eq!(rt.get(root, "written").to_js_string(), "big.png");
    assert_eq!(rt.get(root, "beside_width").to_number(), 4.0);
}

/// Each source, and the width of the image Quickshell 0.3.2 loads for it (0: `Image.Error`).
/// `DIR` is the directory of the document.
const ESCAPED: &[(&str, f64)] = &[
    (r#""a%20b.png""#, 4.0),
    (r#""a b.png""#, 4.0),
    (r#"Qt.resolvedUrl("a b.png")"#, 4.0),
    (r#"Qt.resolvedUrl("DIR/50% a.png")"#, 4.0),
    (r#""50% a.png""#, 4.0),
    (r#""50%25%20a.png""#, 4.0),
    (r#""a#b.png""#, 0.0),
    (r#""a%23b.png""#, 4.0),
    (r#""q?x.png""#, 0.0),
    (r#""q%3Fx.png""#, 4.0),
    (r#""file://DIR/a%20b.png""#, 4.0),
    (r#""a%2520b.png""#, 7.0),
    (r#""a%20c.png""#, 0.0),
    (r#""a%2520c.png""#, 4.0),
    (r#"Qt.resolvedUrl("DIR/a%20c.png")"#, 0.0),
    (r#""DIR/a b.png?x#y""#, 4.0),
    (r#""sub/../a b.png""#, 4.0),
    (r#""file:a b.png""#, 0.0),
    (r#""FILE://DIR/a b.png""#, 4.0),
];

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn an_escaped_source_names_the_decoded_file() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("cfg");
    std::fs::create_dir_all(&dir).unwrap();
    for (name, w) in [
        ("a b.png", 4),
        ("50% a.png", 4),
        ("a#b.png", 4),
        ("q?x.png", 4),
        ("a%20b.png", 7),
        ("a%20c.png", 4),
    ] {
        image::RgbImage::from_pixel(w, 3, image::Rgb([0xc0, 0x80, 0x40]))
            .save(dir.join(name))
            .unwrap();
    }
    let mut shell = String::from("import QtQuick\nItem {\n    width: 100; height: 100\n");
    let ids: Vec<String> = (0..ESCAPED.len()).map(|k| format!("i{k}")).collect();
    for (id, (source, _)) in ids.iter().zip(ESCAPED) {
        let source = source.replace("DIR", dir.to_str().unwrap());
        shell += &format!("    Image {{ id: {id}; source: {source} }}\n");
    }
    let list = |prop: &str| {
        let all: Vec<String> = ids.iter().map(|id| format!("{id}.{prop}")).collect();
        all.join(", ")
    };
    shell += &format!("    property var statuses: [{}]\n", list("status"));
    shell += &format!("    property var widths: [{}]\n}}\n", list("implicitWidth"));
    std::fs::write(dir.join("shell.qml"), shell).unwrap();

    let mut rt = Runtime::new(Some(dir.clone()));
    let root = rt.load_file(&dir.join("shell.qml")).unwrap();
    rt.settle();
    let mut fonts = scene::Fonts::default();
    scene::layout_window(&mut rt, &mut fonts, root, [100.0, 100.0]);
    rt.settle();
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());

    let numbers = |prop: &str| -> Vec<f64> {
        let s = rt.get(root, prop).to_js_string();
        s.split(',').map(|n| n.parse().unwrap()).collect()
    };
    let (statuses, widths) = (numbers("statuses"), numbers("widths"));
    for (k, (source, width)) in ESCAPED.iter().enumerate() {
        let status = if *width == 0.0 { 3.0 } else { 1.0 };
        assert_eq!((statuses[k], widths[k]), (status, *width), "{source}");
    }
}
