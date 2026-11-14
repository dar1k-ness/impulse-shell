//! `sourceSizeChanged` and `statusChanged` of an Image whose picture the shell loads, as Qt
//! 6.12 sends them (QQuickImageBase::requestFinished): `statusChanged` as a load ends,
//! whatever the status was before it, then `sourceSizeChanged` when the `sourceSize` is not
//! the one the load before ended with. An Image nothing gave a `sourceSize` has the
//! picture's, and none (0x0) with no picture; a `sourceSize` that is set stays whatever is
//! loaded, and a write of it loads again. The Images of a document load the last created
//! first. `crates/rigen/check/cases/qml/image_source_size.qml` has the writes of
//! `sourceSize` with no picture.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::scene;

const SHELL: &str = r#"import QtQuick
Item {
    id: root
    width: 100; height: 100
    property string result: ""
    property var log: []
    Image { id: a; source: "a.png"; onSourceSizeChanged: root.log.push("a"); onStatusChanged: root.log.push("as" + status) }
    Image { id: m; source: "missing.png"; onSourceSizeChanged: root.log.push("m"); onStatusChanged: root.log.push("ms" + status) }
    Image { id: w; source: "a.png"; sourceSize: Qt.size(4, 3); onSourceSizeChanged: root.log.push("w") }
    Image { id: l; source: "a.png"; sourceSize.width: 4; sourceSize.height: 3; onSourceSizeChanged: root.log.push("l") }
    Image { id: n; sourceSize: Qt.size(4, 3); onSourceSizeChanged: root.log.push("n") }
    Image { id: e; onSourceSizeChanged: root.log.push("e") }
    property var steps: [
        () => {},
        () => { a.source = "b.png" },
        () => { a.source = "b.png" },
        () => { a.source = "missing.png" },
        () => { a.source = "" },
        () => { a.source = "a.png" },
        () => { a.sourceSize = Qt.size(4, 3) },
        () => { a.source = "b.png" },
        () => { m.source = "a.png" },
        () => { w.source = "b.png" },
        () => { w.sourceSize.width = 5 },
        () => { n.source = "a.png" },
        () => { e.source = "a.png" },
        () => { e.source = "" },
        () => { l.source = "" },
    ]
    property int go: -1
    onGoChanged: steps[go]()
    property int done: -1
    onDoneChanged: { result += log.join(" ") + ";"; log = [] }
}
"#;

/// What Qt 6.12 logs for each step (PySide6 6.12.0, `a.png` 8x6, `b.png` 20x10, no
/// `missing.png`), the first being the load of the document.
const QT: &str = "l n w l w ms3 m as1 a;as1 a;;as3 a;as0;as1 a;a as1 a;as1;ms1 m;;w w;n;e;e;;";

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn source_size_changes_as_a_load_ends() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().to_path_buf();
    let png = |name: &str, w: u32, h: u32| {
        image::RgbImage::from_pixel(w, h, image::Rgb([0xc0, 0x80, 0x40]))
            .save(dir.join(name))
            .unwrap();
    };
    png("a.png", 8, 6);
    png("b.png", 20, 10);
    std::fs::write(dir.join("shell.qml"), SHELL).unwrap();

    let mut rt = Runtime::new(Some(dir.clone()));
    let root = rt.load_file(&dir.join("shell.qml")).unwrap();
    let mut fonts = scene::Fonts::default();
    let steps = QT.split(';').count() - 1;
    for k in 0..steps {
        rt.set(root, "go", Value::Num(k as f64));
        rt.settle();
        scene::layout_window(&mut rt, &mut fonts, root, [100.0, 100.0]);
        rt.settle();
        rt.set(root, "done", Value::Num(k as f64));
    }
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    assert_eq!(rt.get(root, "result").to_js_string(), QT);
}
