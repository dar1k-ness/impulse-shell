//! When an Image loads its picture, against `qml6` (Qt 6.12).
//! QQuickImageBase::componentComplete loads a `source` that is set, and Qt completes the
//! objects of a document the last created first, all of them before the first
//! `Component.onCompleted` runs: there a picture read from a file is loaded (`status` 1,
//! the implicit size and the `sourceSize` are the picture's), a file that is missing has
//! failed (3) and an `asynchronous` Image is loading (2). A Repeater makes its delegates
//! and a Loader its item when they complete, so the Images in them load in their turn.

use std::cell::RefCell;
use std::rc::Rc;

use qml::runtime::Runtime;
use shell::scene;

/// Images that log their name and `status` when the status changes (`s`) and their name,
/// `status`, implicit width and `sourceSize.width` in `Component.onCompleted` (`c`): next
/// to each other, inside items, in the delegates of a Repeater around a Loader's, in a
/// Loader, in a layout, one with no file, one `asynchronous` and one with no `source`.
fn members(file: &str) -> String {
    let with = |n: &str, source: &str, more: &str| {
        format!(
            "Image {{ property string n: {n}; {source}{more}\
             onStatusChanged: if (status != Image.Ready || !asynchronous) \
             win.events += \" s:\" + n + status\n \
             Component.onCompleted: win.events += \" c:\" + n + status + \"/\" + \
             implicitWidth + \"/\" + sourceSize.width }}"
        )
    };
    let i = |n: &str| with(n, &format!("source: \"{file}\"; "), "");
    let (ix, iy) = (i("\"r\" + index + \"x\""), i("\"r\" + index + \"y\""));
    let iq = i("\"r\" + row.index + \"q\"");
    let lines = [
        "id: win".to_string(),
        "width: 400; height: 400".to_string(),
        "property string events: \"\"".to_string(),
        "property int frames: 0".to_string(),
        i("\"a0\""),
        i("\"a1\""),
        format!(
            "Item {{ {}\n Item {{ {}\n {}\n Component.onCompleted: win.events += \" c:B\" }}\n {} }}",
            i("\"b0\""),
            i("\"b1\""),
            i("\"b2\""),
            i("\"b3\"")
        ),
        format!(
            "Repeater {{ model: 2; Item {{ id: row; required property int index; {ix}\n \
             Loader {{ sourceComponent: {iq} }}\n {iy} }} }}"
        ),
        format!(
            "Loader {{ sourceComponent: Item {{ {}\n {} }} }}",
            i("\"l0\""),
            i("\"l1\"")
        ),
        format!("RowLayout {{ {}\n {} }}", i("\"g0\""), i("\"g1\"")),
        with("\"m\"", "source: \"missing.png\"; ", ""),
        with("\"y\"", "source: \"y.png\"; ", "asynchronous: true; "),
        with("\"e\"", "", ""),
        i("\"z0\""),
        i("\"z1\""),
        "Component.onCompleted: events += \" c:W\"".to_string(),
        "property string text: \"\"".to_string(),
        "signal tell()".to_string(),
        "onTell: text = win.events".to_string(),
    ];
    lines.iter().map(|l| format!("    {l}\n")).collect()
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn images_load_as_they_complete_before_any_completed_handler() {
    if !testenv::require("qml6") {
        return;
    }
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        testenv::unavailable("a headless sway (WAYLAND_DISPLAY)");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().to_path_buf();
    image::RgbImage::from_pixel(8, 6, image::Rgb([0xc0, 0x80, 0x40]))
        .save(dir.join("a.png"))
        .unwrap();
    std::fs::copy(dir.join("a.png"), dir.join("y.png")).unwrap();
    let file = dir.join("order.qml");
    let source = format!(
        "import QtQuick\nimport QtQuick.Layouts\nWindow {{\n{}    visible: true\n    \
         onFrameSwapped: {{\n        frames++;\n        if (frames == 3) {{\n            \
         tell();\n            console.log(\"ORDER\", text);\n            Qt.quit();\n        \
         }}\n    }}\n}}\n",
        members("a.png")
    );
    std::fs::write(&file, source).unwrap();
    let out = std::process::Command::new("qml6")
        .arg(&file)
        .env("QT_QPA_PLATFORM", "wayland-egl")
        .env("LIBGL_ALWAYS_SOFTWARE", "1")
        .env("QT_FORCE_STDERR_LOGGING", "1")
        .env("QT_SCALE_FACTOR", "1")
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&out.stderr);
    let qt = log.lines().find_map(|l| l.split_once("ORDER "));
    let qt = qt
        .unwrap_or_else(|| panic!("qml6 printed:\n{log}"))
        .1
        .trim()
        .to_string();

    let source = format!(
        "import QtQuick\nimport QtQuick.Layouts\nimport Quickshell\nFloatingWindow {{\n{}}}\n",
        members("a.png")
    );
    std::fs::write(dir.join("shell.qml"), source).unwrap();
    let mut rt = Runtime::new(Some(dir.clone()));
    // The hooks the shell gives a world (`main.rs`).
    let fonts = Rc::new(RefCell::new(scene::Fonts::default()));
    {
        let fonts = fonts.clone();
        rt.world.borrow_mut().pre_complete = Some(Box::new(move |w, n0| {
            fonts
                .try_borrow_mut()
                .is_ok_and(|mut f| scene::measure_new(w, &mut f, n0))
        }));
    }
    scene::image_hook(&mut rt.world.borrow_mut(), &fonts);
    let win = rt.load_file(&dir.join("shell.qml")).expect("scene loads");
    for _ in 0..3 {
        for _ in 0..3 {
            rt.settle();
            scene::layout_window(&mut rt, &mut fonts.borrow_mut(), win, [400.0, 400.0]);
        }
        rt.settle();
    }
    rt.emit(win, "tell", &[], vec![]);
    rt.settle();
    let ours = rt.world.borrow_mut().read(win, "text");
    let ours = ours.map_or("undefined".to_string(), |v| v.to_js_string());
    let errors = rt.take_errors().0;
    assert!(
        ours.trim() == qt && errors.is_empty(),
        "the Images do not load when Qt loads them:\n  ours {}\n  Qt   {qt}\nour errors: {errors:?}",
        ours.trim()
    );
}
