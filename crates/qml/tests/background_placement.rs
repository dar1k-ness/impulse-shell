//! The original's background widgets (`AbstractBackgroundWidget`) are placed by the wallpaper once, as
//! The original places them: when Config is ready and the screen size is known (a desktop-container
//! widget is made before its screen is measured), again when the wallpaper or the strategy
//! changes, but not on a later resolution or scale change. The widget's own file runs
//! against stand-ins for the singletons it uses.
use qml::runtime::Runtime;
use qml::value::Value;
use std::path::{Path, PathBuf};

const CONFIG: &str = r##"pragma Singleton
import QtQuick
QtObject {
    property bool ready: false
    // `LeastBusyRegion` runs started.
    property int placements: 0
    property var options: ({
        "background": {
            "wallpaperPath": "/wallpapers/a.png",
            "thumbnailPath": "",
            "widgets": { "clock": { "x": 100, "y": 100, "placementStrategy": "leastBusy" } }
        },
        "lock": { "blur": { "enable": false } }
    })
}
"##;

const APPEARANCE: &str = r##"pragma Singleton
import QtQuick
QtObject {
    property QtObject colors: QtObject {
        property color colPrimary: "#ff8080"
        property color colOnLayer0: "#ffffff"
    }
    property QtObject animation: QtObject {
        property QtObject elementMove: QtObject {
            property Component numberAnimation: Component { NumberAnimation { duration: 0 } }
        }
        property QtObject elementMoveFast: QtObject {
            property Component numberAnimation: Component { NumberAnimation { duration: 0 } }
        }
        property QtObject elementResize: QtObject {
            property Component numberAnimation: Component { NumberAnimation { duration: 0 } }
        }
    }
}
"##;

const GLOBAL_STATES: &str = r##"pragma Singleton
import QtQuick
QtObject { property bool screenLocked: false }
"##;

const COLOR_UTILS: &str = r##"pragma Singleton
import QtQuick
QtObject { function colorWithLightness(c, l) { return c; } }
"##;

const SEARCH: &str = "id: leastBusyRegionProc\n";
const COUNT: &str = "        onRunningChanged: if (running) Config.placements++\n";

fn main_qml(width: i32, height: i32) -> String {
    format!(
        "import QtQuick\nimport ri.modules.rusty.background.widgets\nAbstractBackgroundWidget {{\n    configEntryName: \"clock\"\n    screenWidth: {width}\n    screenHeight: {height}\n    scaledScreenWidth: {width}\n    scaledScreenHeight: {height}\n    wallpaperScale: 1\n}}\n"
    )
}

fn tree(tag: &str) -> PathBuf {
    let ri = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qml");
    let d = std::env::temp_dir().join(format!("background-placement-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    for (path, text) in [
        ("GlobalStates.qml", GLOBAL_STATES.to_string()),
        ("modules/common/Config.qml", CONFIG.to_string()),
        ("modules/common/Appearance.qml", APPEARANCE.to_string()),
        (
            "modules/common/functions/ColorUtils.qml",
            COLOR_UTILS.to_string(),
        ),
    ] {
        let f = d.join(path);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, text).unwrap();
    }
    for path in [
        "modules/rusty/background/widgets/AbstractBackgroundWidget.qml",
        "modules/common/widgets/widgetCanvas/AbstractWidget.qml",
    ] {
        let f = d.join(path);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        let text = std::fs::read_to_string(ri.join(path)).unwrap();
        // The widget's own search, counted where it starts.
        let counted = text.replacen(SEARCH, &format!("{SEARCH}{COUNT}"), 1);
        assert_eq!(
            counted != text,
            path.ends_with("AbstractBackgroundWidget.qml"),
            "{path}"
        );
        std::fs::write(f, counted).unwrap();
    }
    d
}

struct Shell {
    rt: Runtime,
    config: usize,
    widget: usize,
}

fn start(d: &Path, ready: bool, width: i32, height: i32) -> Shell {
    let mut rt = Runtime::new(Some(d.to_path_buf()));
    rt.load_singletons(&[
        d.to_path_buf(),
        d.join("modules/common"),
        d.join("modules/common/functions"),
    ]);
    rt.settle();
    let config = *rt.world.borrow().singletons.get("Config").unwrap();
    rt.set(config, "ready", Value::Bool(ready));
    rt.settle();
    std::fs::write(d.join("main.qml"), main_qml(width, height)).unwrap();
    let widget = rt.load_file(&d.join("main.qml")).unwrap();
    rt.settle();
    Shell { rt, config, widget }
}

impl Shell {
    fn placements(&self) -> f64 {
        self.rt
            .world
            .borrow_mut()
            .read(self.config, "placements")
            .unwrap_or_default()
            .to_number()
    }

    fn screen(&mut self, width: f64, height: f64) {
        for k in ["screenWidth", "scaledScreenWidth"] {
            self.rt.set(self.widget, k, Value::Num(width));
        }
        for k in ["screenHeight", "scaledScreenHeight"] {
            self.rt.set(self.widget, k, Value::Num(height));
        }
        self.rt.settle();
    }
}

/// The monitor sizes after a mode or scale change, in logical pixels.
const CHANGES: [(&str, f64, f64); 3] = [
    ("1280x720", 1280.0, 720.0),
    ("1920x1080 at scale 1.5", 1280.0, 720.0),
    ("2560x1440 at scale 1.5", 1707.0, 960.0),
];

#[test]
fn placed_once_the_screen_is_measured_and_kept_through_mode_and_scale_changes() {
    let d = tree("measured");
    // Made before its screen is measured, Config not loaded yet.
    let mut sh = start(&d, false, 0, 0);
    assert_eq!(sh.placements(), 0.0);
    sh.rt.set(sh.config, "ready", Value::Bool(true));
    sh.rt.settle();
    assert_eq!(sh.placements(), 0.0, "no placement on a 0x0 screen");
    sh.screen(1920.0, 1080.0);
    assert_eq!(sh.placements(), 1.0, "placed once the size is known");
    for (what, w, h) in CHANGES {
        sh.screen(w, h);
        assert_eq!(sh.placements(), 1.0, "{what}: the place is kept");
    }
    sh.screen(1920.0, 1080.0);
    assert_eq!(sh.placements(), 1.0);
    // A new wallpaper places it again, as in the original.
    let options = sh.rt.world.borrow_mut().read(sh.config, "options").unwrap();
    let Value::Obj(o) = &options else { panic!() };
    let Value::Obj(bg) = o.borrow().props["background"].clone() else {
        panic!()
    };
    bg.borrow_mut()
        .props
        .insert("wallpaperPath".into(), Value::str("/wallpapers/b.png"));
    sh.rt.set(sh.config, "options", options.clone());
    sh.rt.settle();
    assert_eq!(sh.placements(), 2.0, "a new wallpaper places it again");
    assert!(sh.rt.errors().is_empty(), "{:?}", sh.rt.errors());
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn made_on_a_measured_screen_with_config_loaded_it_is_placed_at_once() {
    let d = tree("loaded");
    let mut sh = start(&d, true, 1920, 1080);
    // Placed as it is made (each trigger restarts the one run; how many fire at creation is
    // the runtime's business), and only then.
    let placed = sh.placements();
    assert!(placed >= 1.0);
    for (what, w, h) in CHANGES {
        sh.screen(w, h);
        assert_eq!(sh.placements(), placed, "{what}: the place is kept");
    }
    assert!(sh.rt.errors().is_empty(), "{:?}", sh.rt.errors());
    std::fs::remove_dir_all(&d).unwrap();
}
