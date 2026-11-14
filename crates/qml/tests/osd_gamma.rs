//! The tree's on-screen display (`modules/rusty/onScreenDisplay/OnScreenDisplay.qml`) on a gamma
//! change: `Hyprsunset.setGamma` announces the attempt and the display opens on its gamma
//! indicator, as in the original. The right sidebar's brightness slider calls `setGamma` below 30 %.
use qml::runtime::Runtime;
use std::path::{Path, PathBuf};

/// True in the process that runs `test`'s body: one of its own, with a home of its own and no
/// session or system bus. False in the process that started it and checked that it passed.
fn isolated(test: &str) -> bool {
    if testenv::in_own_process(test) {
        return true;
    }
    let dir = tempfile::Builder::new()
        .prefix("osd-gamma-")
        .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
        .unwrap();
    let no_bus = format!("unix:path={}", dir.path().join("no-bus").display());
    let mut vars = testenv::home_vars(dir.path());
    vars.push(("DBUS_SESSION_BUS_ADDRESS", no_bus.clone().into()));
    vars.push(("DBUS_SYSTEM_BUS_ADDRESS", no_bus.into()));
    testenv::run_in_own_process(test, vars);
    false
}

const MAIN: &str = r#"import QtQuick
import Rustshell
import ri
import ri.services
import ri.modules.rusty.onScreenDisplay
Scope {
    property bool open: GlobalStates.osdVolumeOpen
    property string indicator: osd.currentIndicator
    OnScreenDisplay { id: osd }
    function dim() {
        Hyprsunset.setGamma(70);
    }
}
"#;

#[test]
fn gamma_change_opens_the_display_on_the_gamma_indicator() {
    if !isolated(concat!(
        module_path!(),
        "::gamma_change_opens_the_display_on_the_gamma_indicator"
    )) {
        return;
    }
    let qml = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../qml")
        .canonicalize()
        .unwrap();
    let d = PathBuf::from(std::env::var_os("HOME").unwrap()).join("tree");
    std::fs::create_dir_all(&d).unwrap();
    for e in std::fs::read_dir(&qml).unwrap() {
        let e = e.unwrap();
        std::os::unix::fs::symlink(e.path(), d.join(e.file_name())).unwrap();
    }
    std::fs::write(d.join("zzmain.qml"), MAIN).unwrap();
    let mut rt = Runtime::new(Some(d.clone()));
    rt.load_singletons(&[
        d.clone(),
        d.join("modules/common"),
        d.join("modules/common/functions"),
        d.join("services"),
    ]);
    rt.settle();
    let main = rt.load_file(&d.join("zzmain.qml")).unwrap();
    rt.settle();
    assert_eq!(rt.errors(), Vec::<String>::new());
    assert!(!rt.get(main, "open").truthy());
    assert_eq!(rt.get(main, "indicator").to_js_string(), "volume");

    rt.call_method(main, "dim", vec![]).unwrap().unwrap();
    rt.settle();
    assert_eq!(rt.errors(), Vec::<String>::new());
    assert!(rt.get(main, "open").truthy(), "the display did not open");
    assert_eq!(rt.get(main, "indicator").to_js_string(), "gamma");
}
