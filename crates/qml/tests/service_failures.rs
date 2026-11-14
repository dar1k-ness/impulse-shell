//! Failures the services under `qml/services` used to hide, each on the real tree: a formula
//! that did not render, a wallpaper switch that failed, a damaged to-do file, a folder that
//! does not exist, a web search with `&` or `#` in it, and the brightness of a monitor that
//! animates its changes.
use qml::runtime::Runtime;
use qml::value::Value;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

fn ri() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../qml")
}

struct Shell {
    rt: Runtime,
    /// Every command and native job asked for, as the spawn hook got it.
    calls: Rc<RefCell<Vec<Vec<String>>>>,
    /// The native jobs (by type) that fail from now on: exit code 1.
    failing: Rc<RefCell<Vec<&'static str>>>,
}

impl Drop for Shell {
    fn drop(&mut self) {
        qml::native::set_spawn_hook(None);
    }
}

/// The FIFO under HOME that the `xdg-open` of [`shell`] writes its argument to.
const OPENED: &str = "xdg-open.calls";

thread_local! {
    /// The console lines of this thread's runtime.
    static CONSOLE: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Starts keeping the console lines the runtime of this thread prints.
fn keep_console() {
    let _ = qml::builtins::CONSOLE_SINK.set(|_, line| {
        CONSOLE.with_borrow_mut(|c| c.push(line.to_string()));
    });
}

/// In `test`'s own process (see `testenv::run_in_own_process`): a fresh home, nothing started
/// for real (the hook answers every command and job at once, a `PathKind` from the file
/// system as the type does) and the original's singletons on demand. `None` in the process that ran it
/// there.
fn shell(test: &str) -> Option<Shell> {
    if !testenv::in_own_process(test) {
        let home = tempfile::tempdir().unwrap();
        // `Qt.openUrlExternally` opens nothing: no portal, and this `xdg-open`, which writes
        // what it was given to the FIFO a test made to read it ([`OPENED`]), nowhere without
        // one.
        let bin = home.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let opened = format!(
            "#!/bin/sh\nif [ -p \"$HOME/{OPENED}\" ]; then echo \"$1\" > \"$HOME/{OPENED}\"; fi\n"
        );
        testenv::install_script(&bin.join("xdg-open"), &opened);
        let vars = testenv::home_vars(home.path()).into_iter().chain([
            ("PATH", bin.into_os_string()),
            (
                "DBUS_SESSION_BUS_ADDRESS",
                "unix:path=/nonexistent/bus".into(),
            ),
        ]);
        testenv::run_in_own_process(test, vars);
        return None;
    }
    let calls: Rc<RefCell<Vec<Vec<String>>>> = Rc::default();
    let failing: Rc<RefCell<Vec<&'static str>>> = Rc::default();
    let (log, fail) = (calls.clone(), failing.clone());
    qml::native::set_spawn_hook(Some(Box::new(move |argv: &[String]| {
        log.borrow_mut().push(argv.to_vec());
        let ty = argv.first().map_or("", |a| a.as_str());
        if fail.borrow().contains(&ty) {
            return None;
        }
        if ty == "PathKind" {
            let p = Path::new(argv.get(2).map_or("", |a| a.as_str()));
            let kind = if p.is_dir() {
                "dir"
            } else if p.is_file() {
                "file"
            } else {
                "invalid"
            };
            return Some(format!("{kind}\n"));
        }
        Some(String::new())
    })));
    let r = ri();
    keep_console();
    let mut rt = Runtime::new(Some(r.clone()));
    let dirs: Vec<PathBuf> = [
        "modules/common",
        "modules/common/functions",
        "modules/common/utils",
        "services",
        "",
    ]
    .iter()
    .map(|d| r.join(d))
    .collect();
    rt.load_singletons_lazy(&dirs);
    Some(Shell { rt, calls, failing })
}

impl Shell {
    /// One step of the loop, after what the hook answered has exited.
    fn turn(&mut self) {
        self.rt.poll_native();
        while !self.rt.world.borrow().procs.is_empty() {
            self.rt.wait_native();
            self.rt.poll_native();
        }
        self.rt.tick(1000.0);
    }

    fn turns(&mut self) {
        for _ in 0..8 {
            self.turn();
        }
    }

    /// `body` as a QML object importing the original's modules, once startup is over.
    fn fixture(&mut self, body: &str) -> usize {
        let src = format!(
            "import QtQuick\nimport Quickshell\nimport ri\nimport ri.services\nimport ri.modules.common\n{body}"
        );
        let o = self.rt.load_str(&src).unwrap_or_else(|e| panic!("{e}"));
        self.turns();
        o
    }

    fn call(&mut self, o: usize, f: &str, args: &[&str]) -> Value {
        let args = args.iter().map(|a| Value::str(a)).collect();
        self.rt
            .call_method(o, f, args)
            .unwrap_or_else(|| panic!("no {f}"))
            .unwrap_or_else(|e| panic!("{f}: {e} {:?}", self.rt.errors()))
    }

    /// How many times the native job `ty` was started.
    fn started(&self, ty: &str) -> usize {
        let calls = self.calls.borrow();
        calls.iter().filter(|c| c[0] == ty).count()
    }

    /// The notifications sent, as `summary|body`.
    fn notes(&self) -> Vec<String> {
        let calls = self.calls.borrow();
        calls
            .iter()
            .filter(|c| c[0] == "notify-send")
            .map(|c| format!("{}|{}", c[1], c[2]))
            .collect()
    }

    /// The console lines with `part` in them.
    fn logged(&self, part: &str) -> Vec<String> {
        CONSOLE.with_borrow(|c| c.iter().filter(|l| l.contains(part)).cloned().collect())
    }
}

/// A formula MicroTeX failed on leaves a line in the log with the exit code, and the next
/// request for it renders again: it stayed "processed", so it was text until the shell
/// restarted. A formula that rendered is still rendered once.
#[test]
fn failed_latex_render_is_logged_and_tried_again() {
    let Some(mut sh) = shell(concat!(
        module_path!(),
        "::failed_latex_render_is_logged_and_tried_again"
    )) else {
        return;
    };
    let o = sh.fixture(
        "QtObject {
            property int announced: 0
            property var c: Connections {
                target: LatexRenderer
                function onRenderFinished(hash, imagePath) { announced++ }
            }
            function go(e) { return LatexRenderer.requestRender(e)[1] }
            function n() { return announced }
        }",
    );
    const FAILED: &str = "[LatexRenderer] Rendering failed with exit code 1: x^2";
    sh.failing.borrow_mut().push("MicroTexRender");
    assert!(sh.call(o, "go", &["x^2"]).truthy());
    sh.turns();
    assert_eq!(sh.started("MicroTexRender"), 1);
    assert_eq!(sh.logged("[LatexRenderer]"), [FAILED]);
    assert!(
        sh.call(o, "go", &["x^2"]).truthy(),
        "the failed formula is not asked for again"
    );
    sh.turns();
    assert_eq!(sh.started("MicroTexRender"), 2);
    assert_eq!(sh.call(o, "n", &[]).to_number(), 0.0);

    sh.failing.borrow_mut().clear();
    assert!(sh.call(o, "go", &["x^2"]).truthy());
    sh.turns();
    assert_eq!(sh.call(o, "n", &[]).to_number(), 1.0);
    assert!(!sh.call(o, "go", &["x^2"]).truthy());
    sh.turns();
    assert_eq!(sh.started("MicroTexRender"), 3, "rendered twice");
    assert_eq!(sh.logged("[LatexRenderer]"), [FAILED, FAILED]);
    assert!(sh.rt.errors().is_empty(), "{:?}", sh.rt.errors());
}

/// An empty formula is not rendered and not kept: MicroTeX fails on it, so each message with
/// `$ $` in it started MicroTeX again.
#[test]
fn empty_latex_formula_is_not_rendered() {
    let Some(mut sh) = shell(concat!(
        module_path!(),
        "::empty_latex_formula_is_not_rendered"
    )) else {
        return;
    };
    let o = sh.fixture(
        "QtObject {
            function go(e) { return LatexRenderer.requestRender(e)[1] }
            function kept() { return LatexRenderer.processedHashes.length + Object.keys(LatexRenderer.processedExpressions).length }
        }",
    );
    assert!(!sh.call(o, "go", &[""]).truthy());
    sh.turns();
    assert_eq!(sh.started("MicroTexRender"), 0);
    assert_eq!(sh.call(o, "kept", &[]).to_number(), 0.0);
    assert!(sh.logged("[LatexRenderer]").is_empty());
    assert!(sh.rt.errors().is_empty(), "{:?}", sh.rt.errors());
}

/// A chat message keeps a formula with no image as it was written: a failed render has no
/// expression to replace, and the word "undefined" in the message was replaced instead.
#[test]
fn chat_message_keeps_a_formula_without_an_image() {
    let Some(mut sh) = shell(concat!(
        module_path!(),
        "::chat_message_keeps_a_formula_without_an_image"
    )) else {
        return;
    };
    // A file, not `fixture`: the import of the chat's folder resolves per document. The
    // block is given a text as everywhere in `qml/`: left as declared (`({})`), its handler
    // assigns an object to a string, which Qt 6.12 refuses ("Cannot assign QJSValue to
    // QString").
    let doc = PathBuf::from(std::env::var_os("HOME").unwrap()).join("Case.qml");
    let src = "import QtQuick\nimport ri.services\nimport ri.modules.rusty.sidebarLeft.aiChat
        Item {
            QtObject { id: message; property bool done: true; property bool thinking: false }
            MessageTextBlock { id: block; done: false; messageData: message; segmentContent: \"\" }
            function go(t) { block.segmentContent = t; block.done = true }
            function shown() { return block.shownText }
        }";
    std::fs::write(&doc, src).unwrap();
    let o = sh.rt.load_file(&doc).unwrap_or_else(|e| panic!("{e}"));
    sh.turns();
    sh.failing.borrow_mut().push("MicroTexRender");
    const TEXT: &str = "undefined is $x^2$";
    sh.call(o, "go", &[TEXT]);
    sh.turns();
    // The message's timer, a second after it is done, asks for every formula once more.
    sh.rt.tick(3000.0);
    sh.turns();
    assert_eq!(sh.call(o, "shown", &[]).to_js_string(), TEXT);
    assert!(sh.rt.errors().is_empty(), "{:?}", sh.rt.errors());
}

/// A wallpaper switch that exits with an error is logged and announced; the queue settles
/// as after any run. A switch that worked announces nothing.
#[test]
fn failed_wallpaper_switch_is_logged_and_announced() {
    let Some(mut sh) = shell(concat!(
        module_path!(),
        "::failed_wallpaper_switch_is_logged_and_announced"
    )) else {
        return;
    };
    let o = sh.fixture(
        "QtObject {
            function go() { Switchwall.run({ mode: 'dark', noSwitch: true }) }
            function busy() { return Switchwall.busy }
        }",
    );
    sh.calls.borrow_mut().clear();
    sh.call(o, "go", &[]);
    sh.turns();
    assert_eq!(sh.started("WallpaperSwitch"), 1);
    assert_eq!(sh.notes(), [""; 0]);
    assert_eq!(sh.logged("[Switchwall]"), [""; 0]);

    sh.failing.borrow_mut().push("WallpaperSwitch");
    sh.call(o, "go", &[]);
    sh.turns();
    assert_eq!(sh.started("WallpaperSwitch"), 2);
    assert_eq!(
        sh.logged("[Switchwall]"),
        ["[Switchwall] Wallpaper switch failed with exit code 1"]
    );
    assert_eq!(sh.notes(), ["Wallpaper switch failed|"]);
    assert!(!sh.call(o, "busy", &[]).truthy(), "the queue never settled");
    assert!(sh.rt.errors().is_empty(), "{:?}", sh.rt.errors());
}

/// A to-do file that is not JSON is kept beside itself as `todo.json.damaged` before the
/// list is written over it, and the list stays as it was: it became empty, and the next
/// added task replaced the file for good.
#[test]
fn damaged_todo_file_is_kept() {
    let Some(mut sh) = shell(concat!(module_path!(), "::damaged_todo_file_is_kept")) else {
        return;
    };
    const DAMAGED: &str = "[{\"content\": \"buy milk\", \"done\": fal";
    let o = sh.fixture("QtObject { function path() { return Directories.todoPath } }");
    let file = PathBuf::from(sh.call(o, "path", &[]).to_js_string());
    let kept = PathBuf::from(format!("{}.damaged", file.display()));
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, DAMAGED).unwrap();
    // The first use of Todo: it reads the file.
    let o = sh.fixture(
        "QtObject {
            function add(d) { Todo.addTask(d) }
            function reread() { Todo.refresh() }
            function n() { return Todo.list.length }
        }",
    );
    assert_eq!(sh.call(o, "n", &[]).to_number(), 0.0);
    // A FileView that preloads reads in the background (FileView::loadAsync).
    sh.turns();
    let read = |f: &Path| std::fs::read_to_string(f).unwrap_or_else(|e| format!("{e}"));
    assert_eq!(read(&kept), DAMAGED, "the damaged file was not kept");
    assert_eq!(sh.logged("is damaged").len(), 1, "{:?}", sh.logged(""));
    sh.call(o, "add", &["tea"]);
    sh.turns();
    assert_eq!(read(&file), r#"[{"content":"tea","done":false}]"#);
    assert_eq!(read(&kept), DAMAGED);

    // Damaged while the shell runs: the tasks it holds stay.
    std::fs::write(&file, "{}").unwrap();
    sh.call(o, "reread", &[]);
    sh.turns();
    assert_eq!(sh.call(o, "n", &[]).to_number(), 1.0);
    assert_eq!(read(&kept), "{}");
    assert!(sh.rt.errors().is_empty(), "{:?}", sh.rt.errors());
}

/// The wallpaper folder changes only to a folder that exists (for a file, the one it is
/// in): it was set to whatever path was asked for before the check answered.
#[test]
fn wallpaper_folder_is_set_only_to_a_checked_path() {
    let Some(mut sh) = shell(concat!(
        module_path!(),
        "::wallpaper_folder_is_set_only_to_a_checked_path"
    )) else {
        return;
    };
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    let (pics, sub) = (home.join("pics"), home.join("pics/sub"));
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(sub.join("a.png"), "").unwrap();
    let o = sh.fixture(
        "QtObject {
            function go(p) { Wallpapers.setDirectory(p) }
            function dir() { return Wallpapers.effectiveDirectory }
        }",
    );
    let set = |sh: &mut Shell, p: &Path| {
        sh.call(o, "go", &[p.to_str().unwrap()]);
        sh.turns();
        PathBuf::from(sh.call(o, "dir", &[]).to_js_string())
    };
    assert_eq!(set(&mut sh, &pics), pics);
    assert_eq!(set(&mut sh, &home.join("no/such/place")), pics);
    assert_eq!(set(&mut sh, &sub.join("a.png")), sub);
    assert_eq!(set(&mut sh, Path::new("/no/such/place/")), sub);
    assert!(sh.rt.errors().is_empty(), "{:?}", sh.rt.errors());
}

/// The launcher's web search puts the query into the address encoded: `&` and `#` in it cut
/// the query short, and `+` became a space.
#[test]
fn web_search_query_is_encoded() {
    let Some(mut sh) = shell(concat!(module_path!(), "::web_search_query_is_encoded")) else {
        return;
    };
    let o = sh.fixture(
        "QtObject {
            function go(q) {
                LauncherSearch.query = q;
                LauncherSearch.results.find(r => r.iconName === 'travel_explore').execute();
            }
        }",
    );
    // Held open for writing too: `xdg-open` never waits for a reader.
    let calls = PathBuf::from(std::env::var_os("HOME").unwrap()).join(OPENED);
    testenv::fifo(&calls);
    let calls = (std::fs::OpenOptions::new().read(true).write(true))
        .open(&calls)
        .unwrap();
    sh.call(o, "go", &["?c++ & rust #1"]);
    let mut opened = String::new();
    std::io::BufRead::read_line(&mut std::io::BufReader::new(calls), &mut opened).unwrap();
    assert_eq!(
        opened.trim_end(),
        "https://www.google.com/search?q=c%2B%2B%20%26%20rust%20%231%20-site%3Aquora.com%20-site%3Afacebook.com"
    );
}

/// Brightness.qml's `name` member, to its closing brace.
fn brightness_block(name: &str) -> String {
    let src = std::fs::read_to_string(ri().join("services/Brightness.qml")).unwrap();
    let i = src.find(name).unwrap_or_else(|| panic!("{name}"));
    let mut depth = 0;
    for (k, c) in src[i..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return src[i..=i + k].to_string();
                }
            }
            _ => {}
        }
    }
    panic!("{name}")
}

/// Brightness.qml's monitor reduced to what decides when a new level is written, verbatim
/// from qml: its `animateChanges` and `onMultipliedBrightnessChanged`. A monitor that animates
/// its changes (a backlight) writes each level at once; a DDC monitor waits for `setTimer`.
/// The handler asked for `animationEnabled`, which no monitor has, so both waited.
#[test]
fn animated_monitor_writes_brightness_at_once() {
    let src = std::fs::read_to_string(ri().join("services/Brightness.qml")).unwrap();
    let animate = src
        .lines()
        .find(|l| l.contains("property bool animateChanges:"))
        .unwrap();
    let handler = brightness_block("onMultipliedBrightnessChanged:");
    for (is_ddc, written, waiting) in [(false, 1.0, false), (true, 0.0, true)] {
        let mut rt = Runtime::new(None);
        let o = rt
            .load_str(&format!(
                "import QtQuick\nQtObject {{\n id: monitor\n property bool isDdc: {is_ddc}\n{animate}\n property real multipliedBrightness: 0\n property int written: 0\n {handler}\n property var setTimer: Timer {{ id: setTimer; interval: 300; onTriggered: syncBrightness() }}\n function syncBrightness() {{ written++ }}\n function set(v) {{ multipliedBrightness = v }}\n function waiting() {{ return setTimer.running }}\n}}\n"
            ))
            .unwrap_or_else(|e| panic!("{e}"));
        rt.call_method(o, "set", vec![Value::Num(0.5)])
            .unwrap()
            .unwrap();
        let waits = rt.call_method(o, "waiting", vec![]).unwrap().unwrap();
        assert_eq!(rt.get(o, "written").to_number(), written, "DDC {is_ddc}");
        assert_eq!(waits.truthy(), waiting, "DDC {is_ddc}");
        assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    }
}
