//! The properties of the Qt types `qml/` uses, read from a script as Qt 6.12 gives them:
//! with nothing written, after an assignment, after `= undefined`, and from a binding that
//! gives a value, `undefined` and a value again.
//!
//! `qt_props/q/<Type>.qml` holds three objects of the type: `a` with nothing written, `b` with
//! the wider properties written (`padding`, `margins`, `radius`, ...), `c` with `undefined`
//! written to every property where it is declared. Its functions `go`, `step(1..13)` and `fin`
//! put one row per object and property into `out`:
//!
//! ```text
//! a.icon.width [num]: 24 | set 27 | reset 0 | bind 3 -> 0 -> 3 | reset 0
//! ```
//!
//! `qt_props/qt/<Type>.txt` is what Qt 6.12 gives (PySide6 6.12.0, offscreen,
//! `QT_QUICK_CONTROLS_STYLE=Basic`), written by `qt_props/qt.py` in the testenv image
//! (`tools/ci/image-tag`) with `q` at `/p9/q3`, the place the rows of `baseUrl` name:
//!
//! ```text
//! docker run --rm -e QT_QUICK_CONTROLS_STYLE=Basic -v $PWD/crates/shell/tests/qt_props/q:/p9/q3 \
//!     -v $PWD/crates/shell/tests/qt_props:/p9/in <image> \
//!     /opt/qv4-venv/bin/python /p9/in/qt.py /p9/q3 /p9/in/qt
//! ```
//!
//! `qt_props/differ/<Type>.txt` lists the rows of the type that are not what Qt gives, one
//! `<object>.<property>` on a line, then a tab and the kind of difference. The test fails for
//! a differing row that is not listed, and for a listed row that is now Qt's: a fix takes its
//! rows out of the list. It prints `rows 3369, as in Qt N, differ M` and leaves what the
//! engine gave in `<target>/tmp/qt_props/<Type>.txt`, to compare with `qt_props/qt`.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::scene;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// The rows of an output: what is before the first `: ` and what is after it.
fn rows(text: &str) -> BTreeMap<String, String> {
    (text.lines())
        .filter(|l| ["a.", "b.", "c."].iter().any(|p| l.starts_with(p)))
        .filter_map(|l| l.split_once(": "))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn ours(file: &Path) -> String {
    // No shell directory, as `qt.py` has none: a document is a `file:` url there, under
    // Quickshell a `qs:` one (`tests/qs_base_url.rs`).
    let mut rt = Runtime::new(None);
    // Text is measured when written or read, as the shell sets it up.
    let fonts = std::rc::Rc::new(std::cell::RefCell::new(scene::Fonts::default()));
    {
        let fonts = fonts.clone();
        rt.world.borrow_mut().text_measure = Some(Box::new(move |w, i| {
            if let Ok(mut f) = fonts.try_borrow_mut() {
                scene::measure_text(w, &mut f, i);
            }
        }));
    }
    let root = match rt.load_file(file) {
        Ok(root) => root,
        Err(e) => return format!("LOADERR {e}\n"),
    };
    rt.settle();
    let frames = |rt: &mut Runtime, n: usize| {
        for _ in 0..n {
            scene::layout_window(rt, &mut fonts.borrow_mut(), root, [200.0, 200.0]);
            rt.settle();
        }
    };
    frames(&mut rt, 4);
    let call = |rt: &mut Runtime, f: &str, a: Vec<Value>| {
        let _ = rt.call_method(root, f, a).unwrap();
        rt.settle();
        frames(rt, 3);
    };
    call(&mut rt, "go", vec![]);
    for k in 1..14 {
        call(&mut rt, "step", vec![Value::Num(k as f64)]);
    }
    call(&mut rt, "fin", vec![]);
    rt.get(root, "out").to_js_string()
}

#[test]
fn properties_read_as_qt_gives_them_but_for_the_listed_rows() {
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/qt_props");
    let q = data.join("q");
    let mut files: Vec<_> = (std::fs::read_dir(&q).unwrap().flatten())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "qml"))
        .collect();
    files.sort();
    let ours_dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("qt_props");
    std::fs::create_dir_all(&ours_dir).unwrap();
    let (mut total, mut differ, mut unlisted) = (0, BTreeSet::new(), Vec::new());
    let mut listed_rows = BTreeSet::new();
    for file in files {
        let name = file.file_stem().unwrap().to_string_lossy().to_string();
        let qt = rows(&std::fs::read_to_string(data.join(format!("qt/{name}.txt"))).unwrap());
        // A thread each: the scene keeps what it measured per thread.
        let f = file.clone();
        let text = (std::thread::spawn(move || ours(&f)).join()).unwrap_or_default();
        let text = text.replace(&*q.to_string_lossy(), "/p9/q3");
        std::fs::write(ours_dir.join(format!("{name}.txt")), &text).unwrap();
        let listed = std::fs::read_to_string(data.join(format!("differ/{name}.txt")));
        for l in listed.unwrap_or_default().lines() {
            listed_rows.insert(format!("{name} {}", l.split('\t').next().unwrap()));
        }
        let ours = rows(&text);
        for (k, qt) in &qt {
            total += 1;
            if ours.get(k) == Some(qt) {
                continue;
            }
            let row = format!("{name} {}", k.split(" [").next().unwrap());
            if !listed_rows.contains(&row) {
                let ours = ours.get(k).map_or("(no row)", |o| o);
                unlisted.push(format!("+ {name} {k}\n  qt:   {qt}\n  ours: {ours}"));
            }
            differ.insert(row);
        }
    }
    let fixed: Vec<String> = (listed_rows.difference(&differ))
        .map(|r| format!("- {r}"))
        .collect();
    println!(
        "rows {total}, as in Qt {}, differ {}",
        total - differ.len(),
        differ.len()
    );
    assert!(
        unlisted.is_empty() && fixed.is_empty(),
        "rows {total}, differ {}\nnot what Qt gives and not in differ/<Type>.txt:\n{}\nin differ/<Type>.txt and now what Qt gives (take them out):\n{}",
        differ.len(),
        unlisted.join("\n"),
        fixed.join("\n")
    );
}
