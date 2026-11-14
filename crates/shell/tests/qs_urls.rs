//! Urls as Quickshell 0.3.2 gives and takes them.

use qml::runtime::Runtime;
use shell::scene;

fn copy(from: &std::path::Path, to: &std::path::Path, dir: &str) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let (src, dst) = (e.path(), to.join(e.file_name()));
        if src.is_dir() {
            copy(&src, &dst, dir);
        } else if src.extension().is_some_and(|x| x == "png") {
            std::fs::copy(&src, &dst).unwrap();
        } else {
            let text = std::fs::read_to_string(&src).unwrap();
            std::fs::write(&dst, text.replace("DIR", dir)).unwrap();
        }
    }
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn urls_are_quickshell_s() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("cfg");
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/qs_urls");
    copy(&fixture, &dir, dir.to_str().unwrap());

    let mut rt = Runtime::new(Some(dir.clone()));
    let root = rt.load_file(&dir.join("Probe.qml")).unwrap();
    rt.settle();
    let mut fonts = scene::Fonts::default();
    scene::layout_window(&mut rt, &mut fonts, root, [100.0, 100.0]);
    // A FileView reads its file in the event loop's turn.
    rt.poll_native();
    rt.settle();
    eprintln!("ERRORS {:?}", rt.errors());
    let text = rt
        .call_method(root, "reportText", vec![])
        .unwrap()
        .unwrap()
        .to_js_string();
    let text = text.replace(dir.to_str().unwrap(), "DIR");
    let ours: Vec<&str> = text.lines().collect();
    let want = std::fs::read_to_string(fixture.join("expected.qs")).unwrap();
    let mut bad = 0;
    for (k, w) in want.lines().enumerate() {
        let o = ours.get(k).copied().unwrap_or("<none>");
        if o != w {
            bad += 1;
            eprintln!("DIFF\n  qs:   {w}\n  ours: {o}");
        }
    }
    assert_eq!(bad, 0, "of {}", want.lines().count());
}

/// `baseUrl` of `Text`, `TextArea` and `TextEdit` without a written value is the url of the
/// document that holds the object: `qs:@/qs/…` in the shell directory, `file:` or `qs:/abs`
/// outside of it, by the way the document was named. `qs_base_url/expected.qs` is what
/// Quickshell 0.3.2 prints for `qs -p cfg` (`shell.qml` there logs `Probe.report()`).
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn base_url_is_quickshell_s() {
    let tmp = tempfile::tempdir().unwrap();
    let top = tmp.path().to_str().unwrap().to_string();
    let dir = tmp.path().join("cfg");
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/qs_base_url");
    copy(&fixture.join("cfg"), &dir, &top);
    // A document outside of the shell directory.
    copy(&fixture.join("out"), &tmp.path().join("out"), &top);

    let mut rt = Runtime::new(Some(dir.clone()));
    let root = rt.load_file(&dir.join("Probe.qml")).unwrap();
    rt.settle();
    let mut fonts = scene::Fonts::default();
    scene::layout_window(&mut rt, &mut fonts, root, [100.0, 100.0]);
    rt.settle();
    eprintln!("ERRORS {:?}", rt.errors());
    let text = rt
        .call_method(root, "reportText", vec![])
        .unwrap()
        .unwrap()
        .to_js_string();
    let text = text.replace(&top, "DIR");
    let ours: Vec<&str> = text.lines().collect();
    let want = std::fs::read_to_string(fixture.join("expected.qs")).unwrap();
    let mut bad = 0;
    for (k, w) in want.lines().enumerate() {
        let o = ours.get(k).copied().unwrap_or("<none>");
        if o != w {
            bad += 1;
            eprintln!("DIFF\n  qs:   {w}\n  ours: {o}");
        }
    }
    assert_eq!(bad, 0, "of {}", want.lines().count());
}
