//! `Crossfade.request(ms)` of module `Impulse`, called by settings.qml (a page change) and
//! MaterialThemeLoader.qml (a new color scheme): one fade request per call and no process.
use qml::runtime::Runtime;
use std::path::PathBuf;

fn ri() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../qml")
}

/// This process's children, from every thread's `/proc/self/task/<tid>/children` (proc(5)).
fn children() -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir("/proc/self/task")
        .unwrap()
        .flatten()
        .filter_map(|t| std::fs::read_to_string(t.path().join("children")).ok())
        .flat_map(|s| {
            s.split_ascii_whitespace()
                .map(String::from)
                .collect::<Vec<_>>()
        })
        .collect();
    v.sort();
    v
}

/// The argument of the `Crossfade.request(...)` call in `file` of the tree.
fn requested(file: &str) -> String {
    let src = std::fs::read_to_string(ri().join(file)).unwrap();
    let (_, rest) = src
        .split_once("Crossfade.request(")
        .unwrap_or_else(|| panic!("{file}: no Crossfade.request"));
    rest.split_once(')').unwrap().0.to_string()
}

/// In a process of its own (`testenv::run_in_own_process`): the request counter and the list of
/// children are the process's.
#[test]
fn a_request_is_one_fade_and_no_process() {
    const TEST: &str = concat!(module_path!(), "::a_request_is_one_fade_and_no_process");
    if !testenv::in_own_process(TEST) {
        testenv::run_in_own_process(TEST, Vec::<(&str, &str)>::new());
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("main.qml");
    std::fs::write(
        &main,
        "import QtQuick\nimport Impulse\nItem {\n function run(ms) { Crossfade.request(ms) }\n}\n",
    )
    .unwrap();
    let mut rt = Runtime::new(Some(ri()));
    let root = rt.load_file(&main).unwrap();
    for (file, ms) in [
        ("settings.qml", 220.0),
        ("services/MaterialThemeLoader.qml", 400.0),
    ] {
        assert_eq!(requested(file).parse::<f64>().ok(), Some(ms), "{file}");
        let before = children();
        let gen0 = qml::native::crossfade_gen();
        rt.call_method(root, "run", vec![qml::value::Value::Num(ms)])
            .unwrap()
            .unwrap();
        assert_eq!(qml::native::crossfade_gen(), gen0 + 1, "{file}");
        assert!(
            qml::native::detaching() == 0 && children() == before,
            "{file}: a process started"
        );
    }
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
}
