//! HiDPI: `QsWindow.devicePixelRatio` follows the compositor's (fractional) scale for the
//! window, with devicePixelRatioChanged and bindings read before the first scale re-running
//! (StyledImage's `sourceSize: width * QsWindow.window.devicePixelRatio`), and
//! `ShellScreen.devicePixelRatio` is the output's scale.
use qml::runtime::Runtime;
use qml::value::Value;

const QML: &str = r#"import QtQuick
import Rustshell
Item {
    id: root
    property real screenDpr: Rustshell.screens.length ? Rustshell.screens[0].devicePixelRatio : -1
    PanelWindow {
        id: win
        property int changes: 0
        onDevicePixelRatioChanged: changes++
        Item {
            width: 40
            property real dpr: (QsWindow.window as QsWindow)?.devicePixelRatio ?? 1
            property real source: width * ((QsWindow.window as QsWindow)?.devicePixelRatio ?? 1)
        }
    }
}"#;

#[test]
fn window_dpr_follows_compositor_scale() {
    let mut rt = Runtime::new(None);
    let root = rt.load_str(QML).unwrap();
    rt.settle();
    let win = rt.world.borrow_mut().of_type("PanelWindow")[0];
    let probe = rt.world.borrow().items[win].children[0];
    assert_eq!(rt.get(win, "devicePixelRatio").to_number(), 1.0);
    assert_eq!(rt.get(probe, "dpr").to_number(), 1.0);
    assert_eq!(rt.get(probe, "source").to_number(), 40.0);

    rt.set_device_pixel_ratio(win, 1.5);
    assert_eq!(rt.get(win, "devicePixelRatio").to_number(), 1.5);
    assert_eq!(rt.get(probe, "dpr").to_number(), 1.5);
    assert_eq!(rt.get(probe, "source").to_number(), 60.0);
    assert_eq!(rt.get(win, "changes").to_number(), 1.0);

    // The same scale again (a second preferred_scale event) changes nothing.
    rt.set_device_pixel_ratio(win, 1.5);
    assert_eq!(rt.get(win, "changes").to_number(), 1.0);

    rt.set_device_pixel_ratio(win, 2.0);
    assert_eq!(rt.get(probe, "source").to_number(), 80.0);
    assert_eq!(rt.get(win, "changes").to_number(), 2.0);
    let _ = root;
}

#[test]
fn screen_dpr_is_the_output_scale() {
    let mut rt = Runtime::new(None);
    let root = rt.load_str(QML).unwrap();
    rt.set_screens_dpr(&[("DP-1".into(), 0, 0, 1707, 960, 1.5)]);
    assert_eq!(rt.get(root, "screenDpr").to_number(), 1.5);
    // A rescaled output keeps its ShellScreen and updates the ratio.
    rt.set_screens_dpr(&[("DP-1".into(), 0, 0, 1280, 720, 2.0)]);
    assert_eq!(rt.get(root, "screenDpr").to_number(), 2.0);
    rt.set_screens(&[("DP-1".into(), 0, 0, 2560, 1440)]);
    assert_eq!(rt.get(root, "screenDpr").to_number(), 1.0);
    let _ = Value::Null;
}
