//! `Impulse/ShellControl`, what the shell's QML asks of the shell itself: `ipcCall` queues the
//! call for the shell's IPC handler dispatch, `openWindow` starts the app window's process (a
//! spawn hook sees it here). And no QML file of the tree runs its own command line or `wpctl`
//! instead.
use qml::runtime::Runtime;
use qml::value::Value;
use std::path::{Path, PathBuf};

const QML: &str = r#"import QtQuick
import Rustshell
import Impulse
Item {
    function ipc() { ShellControl.ipcCall("sidebarLeft", "toggle") }
    function win(f) { ShellControl.openWindow(f) }
    function run(argv) { Rustshell.execDetached(argv) }
}
"#;

fn call(rt: &mut Runtime, root: usize, f: &str, args: Vec<Value>) {
    rt.call_method(root, f, args).unwrap().unwrap();
}

#[test]
fn ipc_call_queues_the_handler_call() {
    let mut rt = Runtime::new(None);
    let root = rt.load_str(QML).unwrap();
    rt.settle();
    call(&mut rt, root, "ipc", vec![]);
    call(&mut rt, root, "ipc", vec![]);
    assert_eq!(
        std::mem::take(&mut rt.world.borrow_mut().self_ipc),
        vec![("sidebarLeft".to_string(), "toggle".to_string()); 2]
    );
}

/// Under a spawn hook (tests, aotcheck) nothing is queued or started: the hook sees the call.
/// A `qs -p ... ipc call` command line is a command like any other, not a call of this shell.
#[test]
fn hook_sees_the_calls_and_commands_stay_commands() {
    let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::<Vec<String>>::new()));
    let s2 = seen.clone();
    qml::native::set_spawn_hook(Some(Box::new(move |argv: &[String]| {
        s2.borrow_mut().push(argv.to_vec());
        None
    })));
    let mut rt = Runtime::new(None);
    let root = rt.load_str(QML).unwrap();
    rt.settle();
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("shell.qml"), "").unwrap();
    rt.world.borrow_mut().root_dir = Some(d.path().to_path_buf());
    let dir = d.path().to_str().unwrap();
    call(&mut rt, root, "ipc", vec![]);
    call(&mut rt, root, "win", vec![Value::str("/x/settings.qml")]);
    let own = ["qs", "-p", dir, "ipc", "call", "sidebarLeft", "toggle"];
    call(
        &mut rt,
        root,
        "run",
        vec![Value::array(own.map(Value::str).to_vec())],
    );
    let mute = ["wpctl", "set-mute", "@DEFAULT_SOURCE@", "toggle"];
    call(
        &mut rt,
        root,
        "run",
        vec![Value::array(mute.map(Value::str).to_vec())],
    );
    qml::native::set_spawn_hook(None);
    let sv = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(
        *seen.borrow(),
        vec![
            sv(&["ShellControl", "ipcCall", "sidebarLeft", "toggle"]),
            sv(&["ShellControl", "openWindow", "/x/settings.qml"]),
            sv(&own),
            sv(&mute),
        ]
    );
    assert!(rt.world.borrow().self_ipc.is_empty());
}

fn qml_files(d: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(d).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            qml_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "qml" || x == "js") {
            out.push(p);
        }
    }
}

/// The tree asks through `ShellControl` and the `Audio` service: no file starts `qs` or
/// `wpctl` as a command.
#[test]
fn tree_runs_no_qs_or_wpctl_command() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qml");
    let mut files = vec![];
    qml_files(&root, &mut files);
    assert!(files.len() > 100, "{}", files.len());
    let mut found = vec![];
    for f in files {
        let text = std::fs::read_to_string(&f).unwrap();
        for (n, l) in text.lines().enumerate() {
            if l.contains("[\"qs\",") || l.contains("[\"wpctl\",") {
                found.push(format!("{}:{}: {}", f.display(), n + 1, l.trim()));
            }
        }
    }
    assert!(found.is_empty(), "{}", found.join("\n"));
}

/// The right sidebar's "Screen snip" button closes the sidebar and asks for `region screenshot`
/// in the same turn: `ipcCall` is what waits for the sidebar to leave the screen, no timer here.
#[test]
fn screen_snip_toggle_calls_the_region_selector_as_it_closes_the_sidebar() {
    let test = concat!(
        module_path!(),
        "::screen_snip_toggle_calls_the_region_selector_as_it_closes_the_sidebar"
    );
    if !testenv::in_own_process(test) {
        let home = tempfile::Builder::new()
            .prefix("shell-control-")
            .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
            .unwrap();
        let no_bus = format!("unix:path={}", home.path().join("no-bus").display());
        let mut vars = testenv::home_vars(home.path());
        vars.push(("DBUS_SESSION_BUS_ADDRESS", no_bus.clone().into()));
        vars.push(("DBUS_SYSTEM_BUS_ADDRESS", no_bus.into()));
        testenv::run_in_own_process(test, vars);
        return;
    }
    let tree = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../qml")
        .canonicalize()
        .unwrap();
    // A file, not `load_str`: `import ri.modules...` component types resolve per document.
    let doc = PathBuf::from(std::env::var_os("HOME").unwrap()).join("Snip.qml");
    std::fs::write(
        &doc,
        r#"import QtQuick
import ri
import ri.modules.common.models.quickToggles
Item {
    property bool open: GlobalStates.sidebarRightOpen
    ScreenSnipToggle { id: snip }
    function show() { GlobalStates.sidebarRightOpen = true }
    function press() { snip.mainAction() }
}
"#,
    )
    .unwrap();
    let mut rt = Runtime::new(Some(tree.clone()));
    rt.load_singletons_lazy(&[
        tree.clone(),
        tree.join("services"),
        tree.join("modules/common"),
        tree.join("modules/common/functions"),
    ]);
    let root = rt
        .load_file(&doc)
        .unwrap_or_else(|e| panic!("load: {e:?} {:?}", rt.errors()));
    rt.settle();
    call(&mut rt, root, "show", vec![]);
    assert!(rt.get(root, "open").truthy());
    call(&mut rt, root, "press", vec![]);
    assert!(!rt.get(root, "open").truthy());
    assert_eq!(
        std::mem::take(&mut rt.world.borrow_mut().self_ipc),
        vec![("region".to_string(), "screenshot".to_string())]
    );
}
