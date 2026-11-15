//! An item with `layer.enabled` and no effect, against Qt itself (support/qt_window.rs), with
//! no tolerance. Qt draws such an item into a texture (QQuickItemLayer makes a
//! QQuickShaderEffectSource of it) and the texture over the item's rect, so what the item
//! holds lands on the texture's texels, not on the window's pixels: seen once the item's
//! place or its size is not whole in device pixels.
//!
//! The scenes are the two ways it is not whole, at scale 1 (a fraction in the QML itself, on
//! both paths) and at scale 1.25 (whole numbers in the QML, GPU: the CPU path draws at 1).

#[path = "support/qt_window.rs"]
mod qt_window;

use qt_window::Scale;
use std::path::Path;

/// A layered item at (`x`, `y`) of `w` x `h` with `more` layer properties: a Rectangle with
/// hard edges off the pixel grid and the bar's battery number over it.
fn scene(x: f64, y: f64, w: f64, h: f64, more: &str) -> String {
    format!(
        "Item {{ x: {x}; y: {y}; width: {w}; height: {h}; layer.enabled: true{more}\n\
             Rectangle {{ x: 3; y: 3; width: 31.1; height: 17.5; color: \"#e6e1e3\" }}\n\
             Text {{ x: 8; y: 4; text: \"75\"; color: \"#202020\"; renderType: Text.NativeRendering\n\
                 font.family: \"Google Sans Flex\"; font.pixelSize: 13; font.weight: Font.DemiBold }} }}"
    )
}

const SIZE: (usize, usize) = (120, 64);

fn gpu(body: &str, dpr: f64) {
    qt_window::like_qt_by(Scale::Output, body, SIZE, &[dpr], 0, 0);
}

/// The reference: the whole place and size, where the texture's texels are the window's pixels.
#[test]
fn whole_gpu() {
    gpu(&scene(20.0, 20.0, 40.0, 24.0, ""), 1.0);
    gpu(&scene(20.0, 20.0, 40.0, 24.0, ""), 1.25);
}

#[test]
fn whole_cpu() {
    qt_window::cpu_like_qt(&scene(20.0, 20.0, 40.0, 24.0, ""), SIZE);
}

#[test]
fn place_between_pixels_gpu() {
    gpu(&scene(20.3, 20.3, 40.0, 24.0, ""), 1.0);
}

#[test]
fn place_between_pixels_cpu() {
    qt_window::cpu_like_qt(&scene(20.3, 20.3, 40.0, 24.0, ""), SIZE);
}

#[test]
fn size_not_whole_gpu() {
    gpu(&scene(20.0, 20.0, 40.4, 24.4, ""), 1.0);
}

#[test]
fn size_not_whole_cpu() {
    qt_window::cpu_like_qt(&scene(20.0, 20.0, 40.4, 24.4, ""), SIZE);
}

/// 21 * 1.25 = 26.25 device pixels.
#[test]
fn place_between_pixels_at_one_and_a_quarter() {
    gpu(&scene(21.0, 21.0, 40.0, 24.0, ""), 1.25);
}

/// 41 * 1.25 = 51.25 and 25 * 1.25 = 31.25 device pixels.
#[test]
fn size_not_whole_at_one_and_a_quarter() {
    gpu(&scene(20.0, 20.0, 41.0, 25.0, ""), 1.25);
}

/// The item's opacity is the texture's: the number over the Rectangle does not show it through.
#[test]
fn opacity_of_the_whole_gpu() {
    gpu(&scene(20.0, 20.0, 40.0, 24.0, "; opacity: 0.5"), 1.0);
    gpu(&scene(20.0, 20.0, 40.0, 24.0, "; opacity: 0.5"), 1.25);
}

#[test]
fn opacity_of_the_whole_cpu() {
    qt_window::cpu_like_qt(&scene(20.0, 20.0, 40.0, 24.0, "; opacity: 0.5"), SIZE);
}

/// With a layer.effect the opacity is the effect's, once.
#[test]
fn opacity_with_an_effect() {
    let effect = "; opacity: 0.5; layer.effect: MultiEffect { brightness: 0.2 }";
    gpu(&scene(20.0, 20.0, 40.0, 24.0, effect), 1.0);
    gpu(&scene(20.0, 20.0, 40.0, 24.0, effect), 1.25);
}

/// A layer that is shown and is an effect's mask as well: both read the one texture.
#[test]
fn layer_that_is_a_mask() {
    let body = format!(
        "{}\nRectangle {{ id: under; width: 40.4; height: 24.4; color: \"#6750a4\"; visible: false }}\n\
         MultiEffect {{ x: 70.4; y: 20.4; width: 40.4; height: 24.4; source: under\n\
             maskEnabled: true; maskSource: layered }}",
        scene(20.4, 20.4, 40.4, 24.4, "; id: layered")
    );
    gpu(&body, 1.0);
    gpu(&body, 1.25);
}

/// The bar's workspace numbers as Workspaces.qml:186 has them: a row with `layer.enabled`
/// that is shown, as high as the bar, and a Colorizer over it that paints the active pill
/// through the row as its mask. At 1.25 a number's cell is 32.5 device pixels wide and the
/// row begins at 8.75.
fn workspaces(row: &str) -> String {
    let number = |n: u32| {
        format!(
            "Item {{ implicitWidth: 26; implicitHeight: 40\n\
                 Text {{ anchors.centerIn: parent; text: \"{n}\"; color: \"#e8def8\"\n\
                     renderType: Text.NativeRendering; font.family: \"Google Sans Flex\"\n\
                     font.pixelSize: 13 }} }}\n"
        )
    };
    format!(
        "Rectangle {{ anchors.fill: parent; color: \"#1c1b1f\" }}\n\
         Item {{ x: 7; y: 12; width: 104; height: 40\n\
             Item {{ id: active; anchors.fill: parent\n\
                 Rectangle {{ x: 28; anchors.verticalCenter: parent.verticalCenter\n\
                     width: 22; height: 22; radius: 11; color: \"#d0bcff\" }} }}\n\
             Row {{ id: numbers; {row}; layer.enabled: true\n{}{}{}{} }}\n\
             MultiEffect {{ anchors.fill: numbers; source: active\n\
                 colorization: 1; colorizationColor: \"#381e72\"; brightness: 1 - 0.89\n\
                 maskEnabled: true; maskSource: numbers\n\
                 maskThresholdMin: 0.5; maskSpreadAtMin: 1 }} }}",
        number(1),
        number(2),
        number(3),
        number(4)
    )
}

#[test]
fn workspace_numbers() {
    let body = workspaces("anchors.top: parent.top; anchors.bottom: parent.bottom");
    gpu(&body, 1.0);
    gpu(&body, 1.25);
}

/// An edge on the middle of a device pixel (the Rectangle's lower one, 12.5 at 1.25): which row
/// has it depends on which way up the texture is drawn.
#[test]
fn edge_on_half_a_pixel() {
    let body = "Item { x: 20; y: 20; width: 40; height: 24; layer.enabled: true\n\
                    Rectangle { x: 4; y: 3; width: 11; height: 7; color: \"#e6e1e3\" } }";
    gpu(body, 1.25);
}

/// `layer.smooth`, as RoundCorner.qml and CircularProgress.qml have it: linear filtering.
#[test]
fn smooth_between_pixels_at_one_and_a_quarter() {
    gpu(&scene(21.0, 21.0, 41.0, 25.0, "; layer.smooth: true"), 1.25);
}

#[test]
fn smooth_between_pixels_cpu() {
    qt_window::cpu_like_qt(&scene(20.3, 20.3, 40.4, 24.4, "; layer.smooth: true"), SIZE);
}

/// The screen's corners as `qml/` has them (RoundCorner.qml: a Shape with `layer.enabled` and
/// `layer.smooth`), one of each kind, at scale 1 and 1.25; at 1.25 the Shape is 28.75 device
/// pixels and the lower corners begin at 51.25.
///
/// The scale is QT_SCALE_FACTOR's: when sway's output changes its scale under the window,
/// Qt 6.12 is left with one of two pictures of a Shape in a layer from run to run.
#[test]
fn round_corners() {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = tempfile::tempdir().unwrap();
    let widgets = dir.path().join("ri/modules/common/widgets");
    std::fs::create_dir_all(&widgets).unwrap();
    std::fs::copy(
        here.join("../../qml/modules/common/widgets/RoundCorner.qml"),
        widgets.join("RoundCorner.qml"),
    )
    .unwrap();
    let module = "module ri.modules.common.widgets\nRoundCorner 1.0 RoundCorner.qml\n";
    std::fs::write(widgets.join("qmldir"), module).unwrap();
    let corner = |x: u32, y: u32, which: &str| {
        format!(
            "RoundCorner {{ x: {x}; y: {y}; implicitSize: 23; color: \"#e6e1e3\"\n\
                 corner: RoundCorner.CornerEnum.{which} }}\n"
        )
    };
    let body = corner(0, 0, "TopLeft")
        + &corner(57, 0, "TopRight")
        + &corner(0, 41, "BottomLeft")
        + &corner(57, 41, "BottomRight");
    qt_window::off_qt_in(
        Scale::Qt,
        dir.path(),
        "import ri.modules.common.widgets\n",
        &body,
        (80, 64),
        &[(1.0, 0, 0, 0), (1.25, 0, 0, 0)],
    );
}

/// The bar's workspaces as `qml/` has them (modules/rusty/bar/Workspaces.qml with the whole tree
/// behind it), numbers shown: `numbersGrid` is a layer that is shown and the Colorizer's mask.
const BAR_ITEM: &str = "Item {\n        width: 300; height: 64\n        \
    Rectangle { anchors.fill: parent; color: \"#1c1b1f\" }\n        \
    Workspaces { x: 7; y: 12; superPressAndHeld: true }\n    }\n";

/// A window with `inside`.
fn bar(inside: &str) -> String {
    format!(
        "import QtQuick\nimport Quickshell\nimport qs.modules.rusty.bar\n\
         FloatingWindow {{\n    id: win\n    implicitWidth: 300; implicitHeight: 64\n    \
         color: \"#1c1b1f\"\n    {inside}}}\n"
    )
}

const BAR_SIZE: (usize, usize) = (300, 64);

/// The options the bar is drawn with: every workspace's number is a string of `numberMap`.
/// Without one, Workspaces.qml writes the number itself to `text`, which is another difference
/// from Qt and not the layer's.
const BAR_CONFIG: &str = r#"{"bar": {"workspaces": {"numberMap": ["1", "2", "3", "4", "5", "6", "7", "8", "9", "10"]}}}"#;

/// `qml/` in `dir` with `scene` as zzscene.qml beside it and plain_layer_qt/WorkspaceModel.qml
/// in the place of its WorkspaceModel.qml, which asks Hyprland.
fn bar_tree(dir: &Path, scene: &str) -> std::path::PathBuf {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    testenv::tree_in_quickshells_words(&here.join("../../qml"), dir);
    let model = dir.join("modules/common/models/WorkspaceModel.qml");
    std::fs::copy(here.join("tests/plain_layer_qt/WorkspaceModel.qml"), model).unwrap();
    std::fs::write(dir.join("zzscene.qml"), scene).unwrap();
    dir.join("zzscene.qml")
}

/// Quickshell's picture of the scene in a window of the headless sway at the output's scale
/// `dpr`, with the home `vars` name: the PNG it saved.
fn bar_quickshell(
    dir: &Path,
    vars: &[(&'static str, std::ffi::OsString)],
    dpr: f64,
) -> std::path::PathBuf {
    let shot = dir.join(format!("quickshell-{dpr}.png"));
    // The items are made once the window has the output's scale. With them made before it,
    // at 1, Qt's picture had one of two edges of the rounded Rectangles in a layer from run to
    // run (243 pixels by up to 21 in one run of three).
    let grab = format!(
        "Loader {{\n        id: late\n        active: win.devicePixelRatio == {dpr}\n        \
         sourceComponent: {BAR_ITEM}    }}\n    \
         Timer {{\n        interval: 2000; running: late.status == Loader.Ready\n        \
         onTriggered: late.item.grabToImage(r => {{\n            \
         console.log(\"SAVED\", r.saveToFile(Quickshell.env(\"SHOT\")), win.devicePixelRatio);\n        \
         }})\n    }}\n"
    );
    let file = bar_tree(&dir.join(format!("qt-{dpr}")), &bar(&grab));
    let modules = dir.join(format!("modules-{dpr}"));
    std::fs::create_dir_all(&modules).unwrap();
    testenv::impulse_qt_module(&modules);
    let sway = testenv::Sway::start();
    let st = std::process::Command::new("swaymsg")
        .env("SWAYSOCK", sway.ipc_socket())
        .arg(format!("output HEADLESS-1 scale {dpr}"))
        .stdout(std::process::Stdio::null())
        .status()
        .expect("swaymsg");
    assert!(st.success(), "swaymsg output scale {dpr}");
    let (rd, wr) = std::io::pipe().unwrap();
    let mut cmd = std::process::Command::new("quickshell");
    cmd.arg("-p")
        .arg(&file)
        .envs(vars.iter().map(|(k, v)| (k, v)))
        .env("SHOT", &shot)
        .env("QML_IMPORT_PATH", &modules)
        .env("QT_QPA_PLATFORM", "wayland")
        .env("LIBGL_ALWAYS_SOFTWARE", "1")
        .env("QSG_INFO", "1")
        .stdin(std::process::Stdio::null())
        .stdout(wr.try_clone().unwrap())
        .stderr(wr);
    let mut child = sway.env(&mut cmd).spawn().unwrap();
    drop(cmd);
    let log = testenv::Log::capture(rd);
    let saved = log.wait(|l| l.contains("SAVED"));
    let _ = child.kill();
    let _ = child.wait();
    saved.unwrap_or_else(|l| panic!("Quickshell saved no picture:\n{l}"));
    let text = log.text();
    assert!(
        text.contains(&format!("SAVED true {dpr}")),
        "Quickshell printed:\n{text}"
    );
    assert!(text.contains("llvmpipe"), "Quickshell drew with:\n{text}");
    shot
}

/// Our picture of the same scene at `dpr`, two seconds of frames after Config is ready.
fn bar_ours(dir: &Path, dpr: f64) -> qsg::raster::Canvas {
    let file = bar_tree(&dir.join(format!("ours-{dpr}")), &bar(BAR_ITEM));
    let d = file.parent().unwrap();
    let mut rt = qml::runtime::Runtime::new(Some(d.to_path_buf()));
    rt.load_singletons(&[
        d.to_path_buf(),
        d.join("modules/common"),
        d.join("modules/common/functions"),
        d.join("services"),
    ]);
    rt.settle();
    let win = rt.load_file(&file).expect("scene loads");
    // Config reads config.json off the loop: the frames begin once it has it.
    let config = rt.world.borrow().singletons["Config"];
    let mut now = 0.0;
    while !rt.get(config, "ready").truthy() {
        rt.wait_native();
        rt.poll_native();
        rt.settle();
    }
    let mut fonts = shell::scene::Fonts::default();
    let (w, h) = BAR_SIZE;
    for _ in 0..125 {
        now += 16.0;
        rt.poll_native();
        rt.tick(now);
        rt.settle();
        shell::scene::layout_window(&mut rt, &mut fonts, win, [w as f64, h as f64]);
    }
    qt_window::drawn(&mut rt, win, BAR_SIZE, dpr)
}

/// modules/rusty/bar/Workspaces.qml:186: `numbersGrid`, a plain layer, at scale 1 and 1.25.
/// Each scale is drawn by a process of its own with the home Quickshell had (Config reads
/// config.json under it).
#[test]
fn workspaces_of_the_bar() {
    const TEST: &str = concat!(module_path!(), "::workspaces_of_the_bar");
    if testenv::in_own_process(TEST) {
        let dir = std::env::var_os("BAR_DIR").unwrap();
        let dpr: f64 = std::env::var("BAR_DPR").unwrap().parse().unwrap();
        let want = std::env::var("BAR_QUICKSHELL").unwrap();
        let want = qsg::image::load_png(&want).unwrap();
        let got = bar_ours(Path::new(&dir), dpr);
        qt_window::dump(&got, &want, dpr);
        let (n, max, at) = qt_window::diff(&got, &want, 0);
        assert_eq!(
            n, 0,
            "dpr {dpr}: {n} pixels differ from Quickshell's, the largest by {max} at {at:?}"
        );
        return;
    }
    if !testenv::require_all(&["quickshell", "sway"]) {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(home.join(".config/impulse-shell")).unwrap();
    std::fs::write(home.join(".config/impulse-shell/config.json"), BAR_CONFIG).unwrap();
    for dpr in [1.0, 1.25] {
        let mut vars = testenv::home_vars(&home);
        let shot = bar_quickshell(dir.path(), &vars, dpr);
        vars.push(("BAR_DIR", dir.path().into()));
        vars.push(("BAR_DPR", dpr.to_string().into()));
        vars.push(("BAR_QUICKSHELL", shot.into()));
        testenv::run_in_own_process(TEST, vars);
    }
}

/// An edge on the middle of a pixel inside a layer without an effect, at scale 1: the layer's
/// texture is drawn top row first, so the row at the Rectangle's top is taken and the one at
/// its bottom left out.
const EDGE_INSIDE: &str = "Item { x: 4; y: 4; width: 40; height: 28; layer.enabled: true\n\
         Rectangle { x: 6.5; y: 4.5; width: 12; height: 10; color: \"#ffcc00\" } }";

/// A layer without an effect whose own place is on the middle of a pixel, at scale 1.
const LAYER_ON_HALF: &str = "Item { x: 4.5; y: 4.5; width: 40; height: 28; layer.enabled: true\n\
         Rectangle { anchors.fill: parent; color: \"#ff3366\" }\n\
         Rectangle { x: 6; y: 4; width: 12; height: 10; color: \"#ffcc00\" }\n\
         Rectangle { x: 20.5; y: 10.5; width: 14; height: 12; radius: 4; color: \"white\" } }";

#[test]
fn edge_on_a_pixel_centre_inside_gpu() {
    gpu(EDGE_INSIDE, 1.0);
}

#[test]
fn edge_on_a_pixel_centre_inside_cpu() {
    qt_window::cpu_like_qt(EDGE_INSIDE, SIZE);
}

#[test]
fn layer_on_pixel_centres_gpu() {
    gpu(LAYER_ON_HALF, 1.0);
}

#[test]
fn layer_on_pixel_centres_cpu() {
    qt_window::cpu_like_qt(LAYER_ON_HALF, SIZE);
}
