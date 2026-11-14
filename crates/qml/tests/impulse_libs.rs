//! The script libraries of the `Impulse` module (`FuzzySort`, `Levenshtein`, `MaterialShapes`,
//! `RoundedPolygon`, `Morph`) called from QML by name: `tests/impulse_libs/Libs.qml` prints what
//! they answer, and the text must be the one the JS libraries they were written from printed
//! for the same calls (recorded as a length and an FNV-1a hash).
use qml::runtime::Runtime;
use std::path::Path;

fn answer(function: &str) -> String {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = std::env::temp_dir().join(format!("impulse-libs-{}-{function}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(
        manifest.join("tests/impulse_libs/Libs.qml"),
        dir.join("Libs.qml"),
    )
    .unwrap();
    // The two value classes of the tree the calls take as arguments.
    let shapes = manifest.join("../../qml/modules/common/widgets/shapes");
    for f in ["shapes/corner-rounding.js", "geometry/offset.js"] {
        let name = Path::new(f).file_name().unwrap();
        std::fs::copy(shapes.join(f), dir.join(name)).unwrap();
    }
    qml::watchdog::set_timeout(None);
    let mut rt = Runtime::new(None);
    let root = rt.load_file(&dir.join("Libs.qml")).unwrap();
    rt.settle();
    let text = rt
        .call_method(root, function, vec![])
        .unwrap()
        .unwrap()
        .to_js_string();
    std::fs::remove_dir_all(&dir).ok();
    text
}

fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf29ce484222325, |h, b| {
        (h ^ b as u64).wrapping_mul(0x100000001b3)
    })
}

#[test]
fn shapes_and_morphs_are_the_ones_the_js_library_made() {
    let s = answer("shapes");
    assert_eq!(
        s.lines().filter(|l| l.starts_with('E')).count(),
        2,
        "morphs of null and undefined"
    );
    assert_eq!((s.len(), fnv(&s)), (77754357, 0xd976fa17e723ad2f));
}

#[test]
fn searches_and_scores_are_the_ones_the_js_libraries_gave() {
    let s = answer("libs");
    assert_eq!((s.len(), fnv(&s)), (170750, 0x67ac334b4acabace));
}
