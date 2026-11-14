//! The shell's own texts on a screen whose scale is not 1, against Qt itself
//! (support/qt_window.rs), with no tolerance, the scale given both ways Qt can have it: by the
//! compositor's output, as on a scaled monitor, and by QT_SCALE_FACTOR.
//!
//! `StyledText.qml` (every label of the shell) is Text.NativeRendering with the default
//! hinting preference, `StyledTextInput.qml` (its input fields) with Font.PreferFullHinting:
//! both files are drawn as `qml/` has them, with what they read of Appearance in
//! `styled_text_dpr_qt/Appearance.qml`. The bare `Text` is the number of the bar's battery
//! (ClippedProgressBar.qml) in each hinting preference.

#[path = "support/qt_window.rs"]
mod qt_window;

use qt_window::Scale;
use std::path::Path;

const DPRS: [f64; 3] = [1.0, 1.25, 2.0];
const NO_HINTING: &str = "; font.hintingPreference: Font.PreferNoHinting";
const FULL_HINTING: &str = "; font.hintingPreference: Font.PreferFullHinting";

fn like_qt(by: Scale, font: &str) {
    let body = format!(
        "Text {{ x: 20; y: 20; text: \"75\"; color: \"#e6e1e3\"; renderType: Text.NativeRendering\n\
             font.family: \"Google Sans Flex\"; font.pixelSize: 13; font.weight: Font.DemiBold{font} }}"
    );
    qt_window::like_qt_by(by, &body, (120, 64), &DPRS, 0, 0);
}

#[test]
fn default_hinting_output() {
    like_qt(Scale::Output, "");
}

#[test]
fn default_hinting_qt() {
    like_qt(Scale::Qt, "");
}

#[test]
fn no_hinting_output() {
    like_qt(Scale::Output, NO_HINTING);
}

#[test]
fn no_hinting_qt() {
    like_qt(Scale::Qt, NO_HINTING);
}

#[test]
fn full_hinting_output() {
    like_qt(Scale::Output, FULL_HINTING);
}

#[test]
fn full_hinting_qt() {
    like_qt(Scale::Qt, FULL_HINTING);
}

/// `body` of the types `widgets` of qml/modules/common/widgets, which Qt and we read from a
/// copy of the files beside `styled_text_dpr_qt/Appearance.qml`.
fn widgets_like_qt(by: Scale, widgets: &[&str], body: &str, size: (usize, usize)) {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = tempfile::tempdir().unwrap();
    let common = dir.path().join("ri/modules/common");
    std::fs::create_dir_all(common.join("widgets")).unwrap();
    std::fs::copy(
        here.join("tests/styled_text_dpr_qt/Appearance.qml"),
        common.join("Appearance.qml"),
    )
    .unwrap();
    let module = "module ri.modules.common\nsingleton Appearance 1.0 Appearance.qml\n";
    std::fs::write(common.join("qmldir"), module).unwrap();
    let mut module = String::from("module ri.modules.common.widgets\n");
    for name in widgets {
        let file = format!("{name}.qml");
        std::fs::copy(
            here.join("../../qml/modules/common/widgets").join(&file),
            common.join("widgets").join(&file),
        )
        .unwrap();
        module += &format!("{name} 1.0 {file}\n");
    }
    std::fs::write(common.join("widgets/qmldir"), module).unwrap();
    qt_window::like_qt_in(
        by,
        dir.path(),
        &["modules/common"],
        "import ri.modules.common.widgets\n",
        body,
        size,
        &DPRS,
    );
}

/// A number (the numbers' font, no axes) and words (the main font with its axes).
const LABELS: &str = "StyledText { x: 20; y: 8; text: \"75\" }\n\
                      StyledText { x: 20; y: 34; text: \"Wi-Fi 5 GHz\" }";
const FIELD: &str = "StyledTextInput { x: 20; y: 20; width: 120; text: \"Search 75\" }";

#[test]
fn styled_text_output() {
    widgets_like_qt(Scale::Output, &["StyledText"], LABELS, (160, 64));
}

#[test]
fn styled_text_qt() {
    widgets_like_qt(Scale::Qt, &["StyledText"], LABELS, (160, 64));
}

#[test]
fn styled_text_input_output() {
    widgets_like_qt(Scale::Output, &["StyledTextInput"], FIELD, (160, 64));
}

#[test]
fn styled_text_input_qt() {
    widgets_like_qt(Scale::Qt, &["StyledTextInput"], FIELD, (160, 64));
}
