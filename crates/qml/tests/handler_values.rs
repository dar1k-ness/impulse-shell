//! An original file starts work from an `on…Changed` handler, and the work reads bindings that
//! depend on the property that changed. Qt 6.12 runs the handler before those bindings, so
//! with the original's file the work started with the previous values: the wallpaper search of a
//! background widget with the previous screen height and strategy. In this tree the handler
//! gives the work its values itself (docs/original-differences.md).
//!
//! The case in `handler_values/` is the file of this tree among stand-ins for what it
//! imports. The `// qt: ` lines of its `main.qml` are what Qt 6.12 prints up to `END`; this
//! engine must print the same, and where Qt is installed Qt is asked again.
#![cfg(feature = "interp")]

#[path = "support/qt_log.rs"]
mod qt_log;

use qml::runtime::Runtime;
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
};

/// A file of `qml/` a case runs, and the line that prints what the work it starts reads:
/// (the line it goes after, the line).
type Real = (&'static str, Option<(&'static str, &'static str)>);

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let to = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy(&e.path(), &to);
        } else {
            std::fs::copy(e.path(), to).unwrap();
        }
    }
}

/// The case's stand-ins with the files of `qml/` among them, as module `qs`.
fn tree(case: &str, real: &[Real]) -> PathBuf {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let d = std::env::temp_dir().join(format!("handler-values-{case}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    copy(&here.join("tests/handler_values").join(case), &d);
    for (path, probe) in real {
        let mut text = std::fs::read_to_string(here.join("../../qml").join(path)).unwrap();
        if let Some((after, line)) = probe {
            let probed = text.replacen(after, &format!("{after}{line}"), 1);
            assert_ne!(probed, text, "{path} has no `{after}`");
            text = probed;
        }
        std::fs::write(d.join("ri").join(path), text).unwrap();
    }
    d
}

/// The directories of `d` with a singleton in them.
fn singleton_dirs(d: &Path, into: &mut Vec<PathBuf>) {
    let mut any = false;
    for e in std::fs::read_dir(d).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            singleton_dirs(&p, into);
        } else if p.extension().is_some_and(|x| x == "qml") {
            any |= std::fs::read_to_string(&p)
                .unwrap()
                .starts_with("pragma Singleton");
        }
    }
    if any {
        into.push(d.to_path_buf());
    }
}

thread_local!(static CONSOLE: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) });

/// What `main.qml` of `d` prints in this engine before `END`.
fn our_log(d: &Path) -> Vec<String> {
    qml::builtins::CONSOLE_SINK
        .get_or_init(|| |_, line| CONSOLE.with_borrow_mut(|c| c.push(line.to_owned())));
    CONSOLE.with_borrow_mut(Vec::clear);
    let qs = d.join("ri");
    let mut rt = Runtime::new(Some(qs.clone()));
    let mut dirs = Vec::new();
    singleton_dirs(&qs, &mut dirs);
    rt.load_singletons(&dirs);
    rt.settle();
    rt.load_file(&d.join("main.qml")).unwrap();
    rt.settle();
    let mut now = 0.0;
    while rt.world.borrow().quit.is_none() {
        assert!(
            now < 60_000.0,
            "never called Qt.quit(): {:?} {:?}",
            rt.errors(),
            CONSOLE.take()
        );
        now += 16.0;
        rt.tick(now);
        rt.run_later();
        rt.settle();
    }
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    until_end(CONSOLE.take())
}

fn until_end(mut log: Vec<String>) -> Vec<String> {
    let end = log
        .iter()
        .position(|l| l == "END")
        .unwrap_or_else(|| panic!("never printed END: {log:#?}"));
    log.truncate(end);
    log
}

fn same_as_qt(case: &str, real: &[Real]) {
    let d = tree(case, real);
    let main = d.join("main.qml");
    let pinned: Vec<String> = std::fs::read_to_string(&main)
        .unwrap()
        .lines()
        .filter_map(|l| l.strip_prefix("// qt: "))
        .map(str::to_owned)
        .collect();
    assert!(!pinned.is_empty());
    let ours = our_log(&d);
    if qt_log::qt_available() {
        let qt = until_end(qt_log::qt_log_all_importing(&main, &d));
        assert_eq!(
            qt, pinned,
            "{case}: Qt 6.12 prints something else now; this engine: {ours:#?}"
        );
    }
    assert_eq!(ours, pinned, "{case}");
    std::fs::remove_dir_all(&d).unwrap();
}

/// The search starts with the height the screen has just got and the strategy just chosen.
#[test]
fn wallpaper_search_reads_the_new_screen_size_and_strategy() {
    same_as_qt(
        "placement",
        &[
            (
                "modules/rusty/background/widgets/AbstractBackgroundWidget.qml",
                Some((
                    "id: leastBusyRegionProc\n",
                    "        onRunningChanged: if (running) console.log(\"search starts: screen\", screenWidth, screenHeight, \"busiest\", busiest, \"image\", imagePath)\n",
                )),
            ),
            (
                "modules/common/widgets/widgetCanvas/AbstractWidget.qml",
                None,
            ),
        ],
    );
}
