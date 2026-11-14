//! `Qt.resolvedUrl(url, context)` under a shell directory, and what takes a `url` value there,
//! as Quickshell 0.3.2 answers: `expected.qs` is what `quickshell -p` logs for `shell.qml` of
//! the fixture run from another directory (`CWD` in it, `UP` the one above), `DIR` being the fixture's.

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
fn resolved_url_with_a_context_is_quickshell_s() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("cfg");
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/qs_resolved_url");
    copy(&fixture, &dir, dir.to_str().unwrap());

    let mut rt = Runtime::new(Some(dir.clone()));
    // What the shell installs: an Image loads as it completes and as its `source` is written.
    // What `createObject` makes for `root` (a Scope) is no child item, so no layout loads it.
    let fonts = std::rc::Rc::new(std::cell::RefCell::new(scene::Fonts::default()));
    scene::image_hook(&mut rt.world.borrow_mut(), &fonts);
    scene::text_hooks(&mut rt.world.borrow_mut(), &fonts);
    let root = rt.load_file(&dir.join("Probe.qml")).unwrap();
    rt.settle();
    scene::layout_window(&mut rt, &mut fonts.borrow_mut(), root, [100.0, 100.0]);
    // A FileView reads its file in the event loop's turn.
    rt.poll_native();
    rt.settle();
    let text = rt
        .call_method(root, "reportText", vec![])
        .unwrap()
        .unwrap()
        .to_js_string();
    let cwd = std::env::current_dir().unwrap();
    let above = |p: &std::path::Path, name| (format!("{}/", p.display()), format!("{name}/"));
    let (cwd, up) = (above(&cwd, "CWD"), above(cwd.parent().unwrap(), "UP"));
    let text = text
        .replace(dir.to_str().unwrap(), "DIR")
        .replace(&cwd.0, &cwd.1)
        .replace(&up.0, &up.1);
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
    assert_eq!((bad, ours.len()), (0, want.lines().count()));
}
