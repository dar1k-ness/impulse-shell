//! `Loader.setSource(url)` resolves a relative url in the document of the code that calls it
//! (`callingQmlContext()->resolvedUrl` in QQuickLoaderPrivate::resolveSourceUrl), not in the
//! one the Loader is written in; a written `source` is kept as given and loaded beside the
//! Loader. `expected.qt` is what Qt 6.12 says for `Main.qml` (`qt.py`).
use qml::runtime::Runtime;
use std::path::Path;

#[test]
fn set_source_resolves_its_url_where_it_is_called() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/loader_set_source_caller");
    let mut rt = Runtime::new(Some(dir.clone()));
    let root = rt.load_file(&dir.join("Main.qml")).unwrap();
    let said = rt.call_method(root, "all", vec![]).expect("no all");
    let said = said.unwrap_or_else(|e| panic!("{e} {:?}", rt.errors()));
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    // The fixture's directory is the shell's here, so that `Holder` is a type.
    let ours = said
        .to_js_string()
        .replace(&format!("file://{}", dir.display()), "DIR")
        .replace("qs:@/qs", "DIR");
    let want = std::fs::read_to_string(dir.join("expected.qt")).unwrap();
    let (ours, want): (Vec<&str>, Vec<&str>) = (ours.lines().collect(), want.lines().collect());
    assert_eq!(ours.len(), want.len(), "{ours:#?}");
    let unlike: Vec<String> = (ours.iter().zip(&want))
        .filter(|(o, w)| o != w)
        .map(|(o, w)| format!("Qt:   {w}\nours: {o}"))
        .collect();
    assert!(
        unlike.is_empty(),
        "{} of {} differ:\n{}",
        unlike.len(),
        want.len(),
        unlike.join("\n")
    );
}
