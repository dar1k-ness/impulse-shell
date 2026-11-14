//! The waffle taskbar's `TaskAppButton.desktopEntry` follows the entry database instead of
//! polling: an app whose `.desktop` file appears after the button exists (database still
//! loading, a package installed later) resolves on the rescan, with no retry Timer.
mod event_loop;
use event_loop::Loop;
use qml::runtime::Runtime;
use std::path::Path;

fn entry(name: &str) -> String {
    format!("[Desktop Entry]\nType=Application\nName={name}\nExec=true\n")
}

/// The `desktopEntry` declaration of the tree's TaskAppButton.qml, verbatim.
fn task_app_binding() -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../qml/modules/waffle/bar/tasks/TaskAppButton.qml");
    let src = std::fs::read_to_string(p).unwrap();
    assert!(!src.contains("Timer {"), "TaskAppButton polls again");
    let start = src
        .find("readonly property var desktopEntry")
        .expect("desktopEntry");
    let end = start + src[start..].find("\n    }\n").expect("binding end") + 6;
    src[start..end].to_string()
}

#[test]
fn late_desktop_entry_resolves_without_polling() {
    const TEST: &str = concat!(
        module_path!(),
        "::late_desktop_entry_resolves_without_polling"
    );
    if !testenv::in_own_process(TEST) {
        let dir = tempfile::Builder::new()
            .prefix("taskapp-entry-")
            .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
            .unwrap();
        let base = dir.path();
        std::fs::create_dir_all(base.join("home/applications")).unwrap();
        std::fs::create_dir_all(base.join("sys/applications")).unwrap();
        std::fs::write(base.join("home/applications/a.desktop"), entry("A")).unwrap();
        testenv::run_in_own_process(
            TEST,
            [
                ("XDG_DATA_HOME", base.join("home").into_os_string()),
                ("XDG_DATA_DIRS", base.join("sys").into_os_string()),
                ("LANG", "C".into()),
                ("LC_ALL", "C".into()),
            ],
        );
        return;
    }
    let base = Path::new(&std::env::var_os("XDG_DATA_HOME").unwrap())
        .parent()
        .unwrap()
        .to_path_buf();
    let apps_dir = base.join("home/applications");
    let cfg = base.join("cfg");
    std::fs::create_dir_all(&cfg).unwrap();
    let shell = format!(
        "import QtQuick\nimport Quickshell\n\nShellRoot {{\n    id: root\n    property var appEntry: ({{ appId: \"late\" }})\n    {}\n    readonly property string found: desktopEntry ? desktopEntry.id : \"-\"\n}}\n",
        task_app_binding()
    );
    std::fs::write(cfg.join("shell.qml"), shell).unwrap();
    let mut rt = Runtime::new(Some(cfg.clone()));
    let root = rt.load_file(&cfg.join("shell.qml")).unwrap();
    rt.settle();
    let found = |rt: &mut Runtime| {
        rt.world
            .borrow_mut()
            .read(root, "found")
            .map(|v| v.to_js_string())
            .unwrap_or_default()
    };
    let mut lp = Loop::new(&mut rt);
    lp.advance(&mut rt, 300.0);
    assert_eq!(found(&mut rt), "-");
    // Well past the old 5 x 1 s retries: a late install still resolves.
    lp.advance(&mut rt, 6000.0);
    assert_eq!(found(&mut rt), "-");
    std::fs::write(apps_dir.join("late.desktop"), entry("Late")).unwrap();
    assert!(
        lp.until(&mut rt, |rt| found(rt) == "late"),
        "late entry never resolved"
    );
}
