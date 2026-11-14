//! What is ready in the call that writes an address and what only later, with the signals in
//! their order: `Image`, `BorderImage`, `AnimatedImage`, `Loader` (and `setSource`),
//! `Qt.createComponent`, `FontLoader`, and under a shell directory `FileView` and `IconImage`.
//! `expected.qt` is what Qt 6.12 writes for `Probe.qml` (`qt.py`), `expected.qs` what
//! Quickshell 0.3.2 writes for `QsProbe.qml` (`qs.sh`); `DIR` in them is the fixture's directory.
//! `waits.qt` and `waits.qs` list the lines that still differ and why.

use qml::runtime::Runtime;
use shell::scene;
use std::path::Path;

/// The file's `report`, written once nothing it started is loading any more.
fn report(shell: bool, file: &str) -> String {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/sync_load");
    let mut rt = Runtime::new(shell.then(|| dir.clone()));
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
    // The loop of a host: a turn for every event the runtime is woken by (its waker's ping,
    // its epoll set), and between two of them it sleeps until the next one comes.
    let wake = testenv::Wakeups::new();
    rt.world.borrow().waker.set_ping(wake.ping());
    let ep = rt.process_fd();
    let root = rt.load_file(&dir.join(file)).unwrap();
    let text = loop {
        // What a turn writes counts only for the next one.
        loop {
            let before = rt.revision();
            rt.settle();
            scene::layout_window(&mut rt, &mut fonts.borrow_mut(), root, [100.0, 100.0]);
            // The readiness is taken before the loads are: a load that reports after the take
            // pings again.
            rt.poll_native();
            let (landed, fetched) = scene::take_finished_images(&mut rt.world.borrow_mut());
            rt.run_later();
            rt.settle();
            // An Image whose load landed says so when it is laid out again.
            if rt.revision() == before && landed.is_empty() && !fetched {
                break;
            }
        }
        let text = rt.get(root, "report").to_js_string();
        if !text.is_empty() {
            break text;
        }
        if rt.later_pending() {
            // The next turn of the loop is owed to what was left for it.
            continue;
        }
        workpool::wait_idle();
        let mut woke = wake.wait(ep, Some(std::time::Duration::ZERO));
        if !woke.any() {
            // Nothing is ready: what is still loading (a decode, a request, a read) wakes.
            woke = wake.wait(ep, None);
        }
        if woke.fd {
            rt.process_fd_ready();
        }
    };
    let dir = dir.to_str().unwrap();
    text.replace(&format!("file://{dir}/"), "DIR/")
        .replace(&format!("{dir}/"), "DIR/")
        .replace("qs:@/qs/", "DIR/")
}

/// `waits` lists the lines known to differ, each under the reason it does: such a line is
/// compared and printed and does not fail the test, until it stops differing; then the test
/// fails for its mark to be removed.
fn same(ours: &str, expected: &str, waits: &str, whose: &str) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/sync_load");
    let want = std::fs::read_to_string(dir.join(expected)).unwrap();
    let waits = std::fs::read_to_string(dir.join(waits)).unwrap();
    let waits: Vec<&str> = waits.lines().filter(|l| !l.starts_with('#')).collect();
    // A line is `name | when | state | signals`: ours is looked up by its first two fields.
    let key = |l: &str| l.splitn(3, " | ").take(2).collect::<Vec<_>>().join(" | ");
    let ours: Vec<&str> = ours.lines().collect();
    let (mut bad, mut waiting) = (0, 0);
    for w in want.lines() {
        let o = ours.iter().find(|o| key(o) == key(w));
        match (o == Some(&w), waits.contains(&key(w).as_str())) {
            (true, false) => {}
            (true, true) => {
                bad += 1;
                eprintln!("MARKED, and no longer differs\n  {whose}: {w}");
            }
            (false, marked) => {
                *(if marked { &mut waiting } else { &mut bad }) += 1;
                let mark = if marked { "WAITS" } else { "DIFF" };
                eprintln!("{mark}\n  {whose}: {w}\n  ours: {}", o.unwrap_or(&"<none>"));
            }
        }
    }
    eprintln!(
        "{waiting} of {} lines differ as marked",
        want.lines().count()
    );
    for o in ours
        .iter()
        .filter(|o| !want.lines().any(|w| key(w) == key(o)))
    {
        bad += 1;
        eprintln!("DIFF\n  {whose}: <none>\n  ours: {o}");
    }
    let known = |l: &&str| want.lines().any(|w| key(w) == **l);
    assert_eq!(
        waits.iter().filter(|l| !known(l)).count(),
        0,
        "marks of no line"
    );
    assert_eq!((bad, ours.len()), (0, want.lines().count()));
}

#[test]
#[cfg_attr(
    not(feature = "interp"),
    ignore = "a tree of its own; AOT-only refuses it"
)]
fn what_is_ready_in_the_same_call_is_qt_s() {
    same(&report(false, "Probe.qml"), "expected.qt", "waits.qt", "qt");
}

#[test]
#[cfg_attr(
    not(feature = "interp"),
    ignore = "a tree of its own; AOT-only refuses it"
)]
fn what_is_ready_in_the_same_call_is_quickshell_s() {
    same(
        &report(true, "QsProbe.qml"),
        "expected.qs",
        "waits.qs",
        "qs",
    );
}

/// `BorderImage` and `FontLoader` are not types of this engine: their lines in `waits.qt`
/// and `waits.qs` stay marked, and both stand in the list of the Qt Quick types the engine
/// lacks. Nothing under `qml/` names either; the first file that does fails here, for the
/// type to be made as Qt 6.12's (the marked lines say how) before the shell relies on it.
#[test]
fn the_shell_uses_no_border_image_and_no_font_loader() {
    fn files(dir: &Path, found: &mut Vec<std::path::PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            match p.extension().and_then(|x| x.to_str()) {
                _ if p.is_dir() => files(&p, found),
                Some("qml" | "js" | "mjs") => found.push(p),
                _ => {}
            }
        }
    }
    let qml = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qml");
    let mut found = Vec::new();
    files(&qml, &mut found);
    assert!(found.len() > 100, "{} files under {qml:?}", found.len());
    let word = |c: char| c.is_alphanumeric() || c == '_';
    let mut uses = Vec::new();
    for f in &found {
        let text = std::fs::read_to_string(f).unwrap();
        for (n, line) in text.lines().enumerate() {
            for ty in ["BorderImage", "FontLoader"] {
                let named = line.match_indices(ty).any(|(at, _)| {
                    !line[..at].chars().next_back().is_some_and(word)
                        && !line[at + ty.len()..].chars().next().is_some_and(word)
                });
                if named {
                    uses.push(format!("{}:{}: {}", f.display(), n + 1, line.trim()));
                }
            }
        }
    }
    assert!(
        uses.is_empty(),
        "qml/ names a Qt Quick type the engine lacks (see the list of such types and the \
         lines marked in tests/sync_load/waits.qt):\n{}",
        uses.join("\n")
    );
}
