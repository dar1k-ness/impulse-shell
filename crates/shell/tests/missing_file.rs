//! An asynchronous `Image` whose file is not there, and what loads once the file is: the same
//! Image left alone, the same Image given the same source again, and a new Image.
//! `missing_file/expected.qt` is what Qt 6.12 writes for `Missing.qml` (`qt.py`): Loading in
//! the call that writes the source, Error a turn later, and of the three only the new Image
//! loads the file. `waits.qt` lists the lines that still differ and why.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::scene;
use std::path::Path;

/// The document's `report`: the file it asks for by `want` is written when it asks.
fn report() -> String {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/missing_file");
    let tmp = tempfile::tempdir().unwrap();
    let at = tmp.path().to_str().unwrap();
    let text = std::fs::read_to_string(dir.join("Missing.qml")).unwrap();
    let main = tmp.path().join("Main.qml");
    let given = format!("property string dir: \"{at}\"");
    std::fs::write(&main, text.replace("property string dir", &given)).unwrap();
    let mut rt = Runtime::new(None);
    // The hooks the shell installs: what Qt measures and loads inside the call that writes.
    let fonts = std::rc::Rc::new(std::cell::RefCell::new(scene::Fonts::default()));
    {
        let (a, b) = (fonts.clone(), fonts.clone());
        let mut w = rt.world.borrow_mut();
        w.pre_complete = Some(Box::new(move |w, n0| {
            a.try_borrow_mut()
                .is_ok_and(|mut f| scene::measure_new(w, &mut f, n0))
        }));
        w.text_measure = Some(Box::new(move |w, i| {
            if let Ok(mut f) = b.try_borrow_mut() {
                scene::measure_text(w, &mut f, i);
            }
        }));
    }
    scene::image_hook(&mut rt.world.borrow_mut(), &fonts);
    let wake = testenv::Wakeups::new();
    rt.world.borrow().waker.set_ping(wake.ping());
    let ep = rt.process_fd();
    let root = rt.load_file(&main).unwrap();
    let mut written = false;
    loop {
        loop {
            let before = rt.revision();
            rt.settle();
            scene::layout_window(&mut rt, &mut fonts.borrow_mut(), root, [100.0, 100.0]);
            rt.poll_native();
            let (landed, fetched) = scene::take_finished_images(&mut rt.world.borrow_mut());
            rt.run_later();
            rt.settle();
            if rt.revision() == before && landed.is_empty() && !fetched {
                break;
            }
        }
        let text = rt.get(root, "report").to_js_string();
        if !text.is_empty() {
            break text;
        }
        if rt.later_pending() {
            continue;
        }
        if let Value::Str(name) = rt.get(root, "want")
            && !name.is_empty()
            && !written
        {
            written = true;
            std::fs::copy(dir.join("picture.png"), tmp.path().join(&*name)).unwrap();
            rt.call_method(root, "fileThere", vec![]).unwrap().unwrap();
            continue;
        }
        workpool::wait_idle();
        let mut woke = wake.wait(ep, Some(std::time::Duration::ZERO));
        if !woke.any() {
            woke = wake.wait(ep, None);
        }
        if woke.fd {
            rt.process_fd_ready();
        }
    }
}

/// `waits.qt` lists the lines known to differ, each under the reason it does: such a line
/// does not fail the test until it stops differing; then the test fails for its mark to be
/// removed.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn a_file_that_appears_loads_where_qt_loads_it() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/missing_file");
    let want = std::fs::read_to_string(dir.join("expected.qt")).unwrap();
    let waits = std::fs::read_to_string(dir.join("waits.qt")).unwrap();
    let waits: Vec<&str> = waits.lines().filter(|l| !l.starts_with('#')).collect();
    let ours = report();
    let ours: Vec<&str> = ours.lines().collect();
    let key = |l: &str| l.split(" | ").next().unwrap_or_default().to_string();
    let mut bad = Vec::new();
    for w in want.lines() {
        let o = ours.iter().find(|o| key(o) == key(w)).copied();
        match (o == Some(w), waits.contains(&key(w).as_str())) {
            (true, false) | (false, true) => {}
            (true, true) => bad.push(format!("MARKED, and no longer differs\n  Qt:   {w}")),
            (false, false) => bad.push(format!("Qt:   {w}\n  ours: {}", o.unwrap_or("<none>"))),
        }
    }
    assert_eq!(ours.len(), want.lines().count(), "the number of lines");
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}
