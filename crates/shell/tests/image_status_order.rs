//! What a handler of an Image's `status` sees. Qt 6.12 sets the status, then the size of the
//! picture, and emits statusChanged last (QQuickImageBase::requestFinished), so the handler
//! and the bindings on the status read the size of the picture. The lines are those of
//! `qml6` of Qt 6.12 for a `source` written after creation, which is what the fixture does.
//!
//! The fixture has no handler of `width`: Qt emits widthChanged before implicitWidthChanged
//! (QQuickItem::setImplicitSize), rusty-impulse the other way round. That is in the same section of
//! the document.
//!
//! When the status changes is not Qt's for such a write: see "The status of an Image whose
//! `source` is written" in docs/qt-differences.md. A picture created with its `source` is
//! `image_status_qt.rs`.

use std::cell::RefCell;
use std::rc::Rc;

use qml::runtime::Runtime;
use shell::scene;

const SHELL: &str = r#"import QtQuick
Item {
    id: root
    width: 100; height: 100
    property string log: ""
    function say(...a) { log += a.join(" ") + "\n" }
    property int twice: pic.implicitWidth * 2
    readonly property bool ready: pic.status == Image.Ready
    onReadyChanged: say("own ready", ready, "implicit", pic.implicitWidth, "twice", twice)
    Image {
        id: pic
        onStatusChanged: root.say("status", status, "implicit", implicitWidth, "w", width,
            "progress", progress, "twice", root.twice, "ready", root.ready)
        onImplicitWidthChanged: root.say("implicitWidth", implicitWidth, "status", status)
        onProgressChanged: root.say("progress", progress, "status", status)
        Component.onCompleted: root.say("completed", status, implicitWidth)
    }
    function set() {
        say("set")
        pic.source = "p20.png"
        say("after", pic.status, pic.implicitWidth)
    }
}
"#;

/// The log at the end of creation, the lines the write of `source` adds and the lines the
/// layout after it adds.
fn run() -> [String; 3] {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().to_path_buf();
    image::RgbImage::from_pixel(20, 20, image::Rgb([0xc0, 0x80, 0x40]))
        .save(dir.join("p20.png"))
        .unwrap();
    std::fs::write(dir.join("shell.qml"), SHELL).unwrap();
    let mut rt = Runtime::new(Some(dir.clone()));
    // The hook the shell gives a world (`main.rs`).
    let fonts = Rc::new(RefCell::new(scene::Fonts::default()));
    scene::image_hook(&mut rt.world.borrow_mut(), &fonts);
    let root = rt.load_file(&dir.join("shell.qml")).unwrap();
    rt.settle();
    scene::layout_window(&mut rt, &mut fonts.borrow_mut(), root, [100.0, 100.0]);
    rt.settle();
    let created = rt.get(root, "log").to_js_string();
    rt.call_method(root, "set", Vec::new()).unwrap().unwrap();
    rt.settle();
    let log = rt.get(root, "log").to_js_string();
    let written = log[created.len()..].to_string();
    scene::layout_window(&mut rt, &mut fonts.borrow_mut(), root, [100.0, 100.0]);
    rt.settle();
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    let laid = rt.get(root, "log").to_js_string()[log.len()..].to_string();
    [created, written, laid]
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn the_size_of_a_picture_is_set_before_its_status_changes() {
    let [_, _, laid] = run();
    let want = "\
progress 1 status 1
implicitWidth 20 status 1
status 1 implicit 20 w 20 progress 1 twice 40 ready false
own ready true implicit 20 twice 40
";
    assert_eq!(laid, want);
}

/// The rusty-impulse column of docs/qt-differences.md: Qt's line here is `after 1 20`, after the four
/// lines of the handlers. Without a `source` the picture is Null at creation in Qt too.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn a_source_written_after_creation_is_loaded_by_the_next_layout_not_by_the_write() {
    let [created, written, _] = run();
    assert_eq!(created, "completed 0 0\n");
    assert_eq!(written, "set\nafter 0 0\n");
}
