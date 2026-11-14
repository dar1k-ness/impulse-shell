//! The warnings an animation refuses `start()`, `stop()`, `pause()` and `resume()` with name
//! its type as Qt 6.12 does (QQmlMetaType::prettyTypeName): without the qualifier of its
//! import, and by the document when the animation is the root of one. Each
//! `anim_warning_name/NAME.qt612.txt` is what `warning_order/qtmsg.py` printed for
//! `anim_warning_name/NAME.qml` in the PySide6 of ri-testenv, the directory taken off.

use qml::runtime::Runtime;
use std::path::Path;

/// What the engine warned of until `name` calls `Qt.quit()`, as `qtmsg.py` prints it.
fn ours(dir: &Path, name: &str) -> Vec<String> {
    let file = dir.join(name);
    let mut rt = Runtime::new(Some(dir.to_path_buf()));
    rt.load_file(&file).unwrap();
    rt.settle();
    let mut now = 0.0;
    while rt.world.borrow().quit.is_none() {
        assert!(
            now < 1000.0,
            "{name} never called Qt.quit(): {:#?}",
            rt.errors()
        );
        now += 16.0;
        rt.tick(now);
        rt.run_later();
        rt.settle();
    }
    let at = format!("file://{}/", dir.display());
    rt.errors()
        .iter()
        .map(|l| format!("warning: {}", l.replace(&at, "")))
        .collect()
}

fn same_as_qt(name: &str) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/anim_warning_name");
    let qt = std::fs::read_to_string(dir.join(name).with_extension("qt612.txt")).unwrap();
    let qt: Vec<&str> = qt.lines().collect();
    assert_eq!(ours(&dir, name), qt);
}

/// `import "." as My`: `My.Idle` is named `Idle`, in each of the three warnings.
#[test]
fn a_qualified_type_is_named_without_its_qualifier() {
    same_as_qt("qualified.qml");
}

/// The root of `Paused.qml` is named `Paused`.
#[test]
fn the_root_of_a_document_is_named_by_the_document() {
    same_as_qt("Paused.qml");
}

/// The root of `Nested.qml` is named `Nested`, the animation inside it by its type.
#[test]
fn an_animation_inside_the_root_is_named_by_its_type() {
    same_as_qt("Nested.qml");
}
