//! `baseUrl` of a Text and a TextEdit in a shell directory, as Quickshell 0.3.2 gives it: the
//! `qs:` url of the document the object is written in, also after a reset.
//! `qs_base_url/quickshell.txt` is what `qs -p crates/qml/tests/qs_base_url` logs in the
//! tools/testenv image (`QT_QPA_PLATFORM=offscreen`): the fixture's Timer prints `report()`.

use qml::runtime::Runtime;

#[test]
fn base_url_in_a_shell_directory_is_quickshell_s() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/qs_base_url");
    let mut rt = Runtime::new(Some(dir.clone()));
    let root = rt.load_file(&dir.join("shell.qml")).unwrap();
    rt.settle();
    let ours = rt.call_method(root, "report", vec![]).unwrap().unwrap();
    assert_eq!(rt.errors(), Vec::<String>::new());
    let qs = std::fs::read_to_string(dir.join("quickshell.txt")).unwrap();
    assert_eq!(ours.to_js_string(), qs);
}
