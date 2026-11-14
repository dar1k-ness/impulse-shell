//! An Image's `sourceSize` is the size asked for, and where none is the loaded pixmap's;
//! `sourceSizeChanged` comes from QQuickImageBase::setSourceSize and again from the end of
//! the load it starts, when the size is not the one the last load left. The signals and the
//! sizes are the ones Qt 6.12 gives the same document, the steps run from
//! `Component.onCompleted` (`for (var i = 0; i < 10; i++) step(i); console.log(logs())`).

use qml::runtime::Runtime;
use qml::value::Value;
use shell::scene;

const SHELL: &str = r#"import QtQuick
Item {
    id: root
    width: 100; height: 100
    property string la: ""
    property string lb: ""
    property string lc: ""
    property string ld: ""
    property string le: ""
    property string lf: ""
    function s(z) { return z.width + "," + z.height }
    Image { id: a; source: "p6x4.png"; onSourceSizeChanged: root.la += s(sourceSize) + " " }
    Image { id: b; source: "p6x4.png"; sourceSize.width: 3; onSourceSizeChanged: root.lb += s(sourceSize) + " " }
    Image { id: c; source: "nope.png"; onSourceSizeChanged: root.lc += s(sourceSize) + " " }
    Image { id: d; onSourceSizeChanged: root.ld += s(sourceSize) + " " }
    AnimatedImage { id: e; onSourceSizeChanged: root.le += s(sourceSize) + " " }
    Image { id: f; source: "p6x4.png"; sourceSize: Qt.size(3, 2); onSourceSizeChanged: root.lf += s(sourceSize) + " " }
    function step(n) {
        root.ld += "| "
        switch (n) {
        case 1: d.source = "p6x4.png"; break
        case 2: d.sourceSize.width = 12; break
        case 3: d.sourceSize = undefined; break
        case 4: d.source = ""; break
        case 5: d.sourceSize = Qt.size(2, 2); break
        case 6: d.source = "p6x4.png"; break
        case 7: d.sourceSize.height = 1; break
        case 8: d.sourceSize = undefined; break
        case 9: e.source = "p6x4.png"; break
        }
    }
    function logs() { return [la, lb, lc, ld, le, lf, s(a.sourceSize), s(b.sourceSize), s(c.sourceSize), s(d.sourceSize), s(e.sourceSize), s(f.sourceSize)].join("\n") }
}
"#;

/// Qt 6.12: per image the sizes its `sourceSizeChanged` saw (`|` starts a step of `d`), then
/// the sizes at the end.
const QT: &str = "6,4 \n\
3,0 3,0 \n\
0,0 \n\
| | 6,4 | 12,4 12,4 | 6,4 6,4 | 0,0 | 2,2 2,2 | | 2,1 2,1 | 2,1 6,4 | \n\
0,0 6,4 \n\
3,2 3,2 \n\
6,4\n\
3,0\n\
0,0\n\
6,4\n\
6,4\n\
3,2";

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn source_size_changes_with_the_request_and_with_the_load() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    image::RgbImage::from_pixel(6, 4, image::Rgb([0xc0, 0x80, 0x40]))
        .save(dir.join("p6x4.png"))
        .unwrap();
    std::fs::write(dir.join("shell.qml"), SHELL).unwrap();

    let mut rt = Runtime::new(Some(dir.to_path_buf()));
    let root = rt.load_file(&dir.join("shell.qml")).unwrap();
    rt.settle();
    let mut fonts = scene::Fonts::default();
    for step in 0..10 {
        rt.call_method(root, "step", vec![Value::Num(step as f64)])
            .unwrap()
            .unwrap();
        rt.settle();
        scene::layout_window(&mut rt, &mut fonts, root, [100.0, 100.0]);
        rt.settle();
    }
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    let logs = rt.call_method(root, "logs", Vec::new()).unwrap().unwrap();
    assert_eq!(logs.to_js_string(), QT);
}
