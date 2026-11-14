//! A singleton's `Component.onCompleted` sees bindings that read the singleton by its own
//! name, as in Qt: Directories.qml creates `${Directories.cache}/media/coverart` and friends
//! from its onCompleted.
use qml::runtime::Runtime;
use std::path::PathBuf;

fn tmpdir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("singleton-complete-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn read(rt: &Runtime, singleton: &str, prop: &str) -> String {
    let id = rt.world.borrow().singletons[singleton];
    rt.world
        .borrow_mut()
        .read(id, prop)
        .map(|v| v.to_js_string())
        .unwrap_or_default()
}

#[test]
fn on_completed_sees_self_named_bindings() {
    let d = tmpdir("self");
    std::fs::write(
        d.join("Dirs.qml"),
        r#"pragma Singleton
import QtQuick
import Rustshell

Singleton {
    readonly property string cache: "/c"
    property string coverArt: `${Dirs.cache}/media/coverart`
    property string nested: `${Dirs.coverArt}/x`
    property string seen: ""
    Component.onCompleted: seen = coverArt + "|" + nested
}
"#,
    )
    .unwrap();
    let mut rt = Runtime::new(None);
    rt.load_singletons(std::slice::from_ref(&d));
    assert_eq!(
        read(&rt, "Dirs", "seen"),
        "/c/media/coverart|/c/media/coverart/x"
    );
    assert_eq!(read(&rt, "Dirs", "coverArt"), "/c/media/coverart");
}

#[test]
fn directories_on_completed_paths_are_set() {
    use std::{cell::RefCell, rc::Rc};
    let ri = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../qml");
    let spawned: Rc<RefCell<Vec<String>>> = Rc::default();
    let log = spawned.clone();
    qml::native::set_spawn_hook(Some(Box::new(move |argv: &[String]| {
        log.borrow_mut().push(argv.join(" "));
        Some(String::new())
    })));
    let mut rt = Runtime::new(Some(ri.clone()));
    let dirs: Vec<PathBuf> = ["modules/common", "modules/common/functions"]
        .iter()
        .map(|d| ri.join(d))
        .collect();
    rt.load_singletons_where(&dirs, &|n| ["Directories", "FileUtils"].contains(&n));
    rt.poll_native();
    qml::native::set_spawn_hook(None);
    let spawned = spawned.borrow();
    let cover = spawned
        .iter()
        .find(|c| c.ends_with("/media/coverart"))
        .unwrap_or_else(|| panic!("no coverart directory in {spawned:?}"));
    assert!(cover.starts_with("Files renewDir /"), "{cover}");
    // Every path a `Files` call got was set by then.
    assert!(
        !spawned
            .iter()
            .any(|c| c.starts_with("Files ") && c.ends_with(' ')),
        "{spawned:?}"
    );
}
