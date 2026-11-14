//! When an Image that is not `asynchronous` is loaded, as Qt 6.12 does it: in its own
//! `componentComplete` (QQuickImageBase::componentComplete calls `load()`), which for a file
//! and for an image provider of type Pixmap or Image (Quickshell's `image://icon/` and
//! `image://service.sni/`) ends the load at once. So the Image is `Image.Ready`, and has sent
//! `statusChanged`, before the `Component.onCompleted` of the delegate it is in and before
//! the Repeater's `itemAdded`; and the Images of one delegate load one after another, the
//! last created first, each sending `statusChanged` while those before it are still
//! `Image.Null`.
//!
//! Ours loaded pictures in the shell's layout, after the document was loaded: at
//! `Component.onCompleted` and `itemAdded` every `status` was 0, and by the first
//! `onStatusChanged` all of them were 1. Seen in `tray_pixels_qt.rs`, whose QML looks at the
//! window from `onItemAdded` and from each `onStatusChanged`.

use qml::runtime::Runtime;
use shell::scene;
use std::cell::RefCell;
use std::rc::Rc;

/// `say` logs `what` and the `status` of each of `pics`.
const HEAD: &str = r#"import QtQuick
Item {
    id: root
    width: 100; height: 100
    property var log: []
    property string result: log.join(" ")
    function say(what, pics) {
        log.push(what + "=" + pics.map(p => p.status).join(""))
        logChanged()
    }
"#;

const FILE: &str = r#"
    Repeater {
        model: 1
        onItemAdded: (index, item) => root.say("added", [item.pic])
        Item {
            property alias pic: f
            Component.onCompleted: root.say("completed", [f])
            Image { id: f; source: "a.png"; onStatusChanged: root.say("status", [f]) }
        }
    }
}
"#;

const PROVIDER: &str = r#"
    Repeater {
        model: 1
        onItemAdded: (index, item) => root.say("added", [item.pic])
        Item {
            property alias pic: p
            Component.onCompleted: root.say("completed", [p])
            Image { id: p; source: "image://icon/p"; onStatusChanged: root.say("status", [p]) }
        }
    }
}
"#;

const THREE: &str = r#"
    Repeater {
        model: 1
        onItemAdded: (index, item) => root.say("added", item.pics)
        Item {
            property var pics: [a, b, c]
            Component.onCompleted: root.say("completed", pics)
            Image { id: a; source: "a.png"; onStatusChanged: root.say("a", [a, b, c]) }
            Image { id: b; source: "a.png"; sourceSize: Qt.size(4, 3); onStatusChanged: root.say("b", [a, b, c]) }
            Image { id: c; source: "image://icon/p"; onStatusChanged: root.say("c", [a, b, c]) }
        }
    }
}
"#;

/// What Qt 6.12 logs (PySide6 6.12.0, offscreen; `a.png` 8x6, `image://icon/` a
/// QQuickImageProvider of type Pixmap, as Quickshell's IconImageProvider is), the same when
/// the document is created and half a second later.
const QT_ONE: &str = "status=1 completed=1 added=1";
const QT_THREE: &str = "c=001 b=011 a=111 completed=111 added=111";

/// In `test`'s own process: a home whose icon theme `u` has the icon `p`.
fn setup(test: &str) -> bool {
    if testenv::in_own_process(test) {
        return true;
    }
    let dir = tempfile::tempdir().unwrap();
    let (config, share) = (dir.path().join(".config"), dir.path().join("share"));
    let theme = share.join("icons/u");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::create_dir_all(theme.join("48")).unwrap();
    std::fs::write(config.join("kdeglobals"), "[Icons]\nTheme=u\n").unwrap();
    std::fs::write(
        theme.join("index.theme"),
        "[Icon Theme]\nName=u\nComment=u\nDirectories=48\n\n[48]\nSize=48\nType=Fixed\n",
    )
    .unwrap();
    image::RgbImage::from_pixel(48, 48, image::Rgb([0xc0, 0x80, 0x40]))
        .save(theme.join("48/p.png"))
        .unwrap();
    let mut vars = testenv::home_vars(dir.path());
    vars.extend([
        ("QT_QPA_PLATFORMTHEME", "kde".into()),
        ("KDE_SESSION_VERSION", "6".into()),
        ("XDG_DATA_DIRS", share.into()),
    ]);
    testenv::run_in_own_process_env(
        test,
        vars,
        &["XDG_CURRENT_DESKTOP", "KDEHOME", "QS_ICON_THEME"],
    );
    false
}

/// What the document `HEAD` + `body` has logged once the shell has laid it out.
fn logged(body: &str) -> String {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().to_path_buf();
    image::RgbImage::from_pixel(8, 6, image::Rgb([0xc0, 0x80, 0x40]))
        .save(dir.join("a.png"))
        .unwrap();
    std::fs::write(dir.join("shell.qml"), format!("{HEAD}{body}")).unwrap();
    let mut rt = Runtime::new(Some(dir.clone()));
    // The hook the shell gives a world (`main.rs`).
    let fonts = Rc::new(RefCell::new(scene::Fonts::default()));
    scene::image_hook(&mut rt.world.borrow_mut(), &fonts);
    let root = rt.load_file(&dir.join("shell.qml")).unwrap();
    for _ in 0..3 {
        rt.settle();
        scene::layout_window(&mut rt, &mut fonts.borrow_mut(), root, [100.0, 100.0]);
    }
    rt.settle();
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    rt.get(root, "result").to_js_string()
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn an_image_of_a_file_is_ready_at_completion_and_at_item_added() {
    assert_eq!(logged(FILE), QT_ONE);
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn an_image_of_a_provider_is_ready_at_completion_and_at_item_added() {
    if !setup("an_image_of_a_provider_is_ready_at_completion_and_at_item_added") {
        return;
    }
    assert_eq!(logged(PROVIDER), QT_ONE);
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn the_images_of_a_delegate_load_one_after_another_the_last_first() {
    if !setup("the_images_of_a_delegate_load_one_after_another_the_last_first") {
        return;
    }
    assert_eq!(logged(THREE), QT_THREE);
}
