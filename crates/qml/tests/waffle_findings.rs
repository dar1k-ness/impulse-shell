//! The waffle panel family (`qml/modules/waffle`) on the tree's own files: a notification's
//! action button invokes the action, the Start button's "Search" entry and `osd trigger` reach
//! a handler, every IPC target has one handler, the night light and Bluetooth pages load
//! without a Bluetooth adapter, the Wi-Fi and Bluetooth lists size themselves, a taskbar button
//! of an app without a desktop entry can be clicked and the focus timer stops at 5 minutes.
use qml::runtime::Runtime;
use qml::value::Value;
use std::path::{Path, PathBuf};

/// True in the process that runs `test`'s body: one of its own, with a home of its own and no
/// session or system bus. False in the process that started it and checked that it passed.
fn isolated(test: &str) -> bool {
    if testenv::in_own_process(test) {
        return true;
    }
    let dir = tempfile::Builder::new()
        .prefix("waffle-findings-")
        .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
        .unwrap();
    let no_bus = format!("unix:path={}", dir.path().join("no-bus").display());
    let mut vars = testenv::home_vars(dir.path());
    vars.push(("DBUS_SESSION_BUS_ADDRESS", no_bus.clone().into()));
    vars.push(("DBUS_SYSTEM_BUS_ADDRESS", no_bus.into()));
    vars.push(("LANG", "C".into()));
    vars.push(("LC_ALL", "C".into()));
    testenv::run_in_own_process(test, vars);
    false
}

macro_rules! isolated {
    ($name:literal) => {
        if !isolated(concat!(module_path!(), "::", $name)) {
            return;
        }
    };
}

fn link_all(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        std::os::unix::fs::symlink(e.path(), to.join(e.file_name())).unwrap();
    }
}

/// The tree with `main` as `zzmain.qml` beside its shell.qml, loaded; `service` replaces one
/// file of `services` (a spy).
fn load_any(main: &str, service: Option<(&str, &str)>) -> (Runtime, usize) {
    let qml = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../qml")
        .canonicalize()
        .unwrap();
    static TREES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = TREES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let d = PathBuf::from(std::env::var_os("HOME").unwrap()).join(format!("tree{n}"));
    link_all(&qml, &d);
    if let Some((name, src)) = service {
        std::fs::remove_file(d.join("services")).unwrap();
        link_all(&qml.join("services"), &d.join("services"));
        std::fs::remove_file(d.join("services").join(name)).unwrap();
        std::fs::write(d.join("services").join(name), src).unwrap();
    }
    std::fs::write(d.join("zzmain.qml"), main).unwrap();
    let mut rt = Runtime::new(Some(d.clone()));
    rt.load_singletons(&[
        d.clone(),
        d.join("modules/common"),
        d.join("modules/common/functions"),
        d.join("services"),
        d.join("modules/waffle/looks"),
    ]);
    rt.settle();
    let main = rt.load_file(&d.join("zzmain.qml")).unwrap();
    rt.settle();
    (rt, main)
}

/// [`load_any`], which must report no error.
fn load(main: &str, service: Option<(&str, &str)>) -> (Runtime, usize) {
    let (rt, main) = load_any(main, service);
    assert_eq!(rt.errors(), Vec::<String>::new());
    (rt, main)
}

fn of_type(rt: &Runtime, ty: &str) -> Vec<usize> {
    rt.world.borrow_mut().of_type(ty)
}

fn text(rt: &Runtime, item: usize, prop: &str) -> String {
    rt.get(item, prop).to_js_string()
}

fn click(rt: &mut Runtime, button: usize) {
    rt.emit(button, "clicked", &[], vec![]);
    rt.settle();
}

const NOTIFICATIONS_SPY: &str = r#"pragma Singleton
import QtQuick
import Rustshell

Singleton {
    property var list: []
    property var invoked: []
    function attemptInvokeAction(id, identifier) {
        invoked = invoked.concat([id + ":" + identifier]);
    }
    function discardNotification(id) {}
}
"#;

/// Finding 8: clicking an action button of a waffle notification asks the Notifications
/// service to invoke that action of that notification.
#[test]
fn notification_action_button_invokes_the_action() {
    isolated!("notification_action_button_invokes_the_action");
    let (mut rt, root) = load(
        r#"import QtQuick
import ri.services
import ri.modules.waffle.notificationCenter
Item {
    width: 400
    height: 400
    property string invoked: Notifications.invoked.join(",")
    WSingleNotification {
        width: 400
        notification: ({
            notificationId: 42, summary: "Mail", body: "Hello", appName: "mail", appIcon: "",
            image: "", time: 0, urgency: "normal", popup: false,
            actions: [{ identifier: "reply", text: "Reply" }, { identifier: "later", text: "Later" }]
        })
    }
}
"#,
        Some(("Notifications.qml", NOTIFICATIONS_SPY)),
    );
    let buttons = of_type(&rt, "WBorderedButton");
    let later: Vec<usize> = buttons
        .into_iter()
        .filter(|&b| text(&rt, b, "text") == "Later")
        .collect();
    assert_eq!(later.len(), 1, "one Later button");
    assert!(rt.get(later[0], "visible").truthy());
    click(&mut rt, later[0]);
    assert_eq!(rt.errors(), Vec::<String>::new());
    assert_eq!(text(&rt, root, "invoked"), "42:later");
}

const PANELS: &str = r#"import QtQuick
import Rustshell
import ri
import ri.modules.waffle.bar
import ri.modules.waffle.startMenu
import ri.modules.waffle.taskView
import ri.modules.waffle.onScreenDisplay
Scope {
    property bool searchOpen: GlobalStates.searchOpen
    property bool overviewOpen: GlobalStates.overviewOpen
    property bool osdVolumeOpen: GlobalStates.osdVolumeOpen
    WaffleStartMenu {}
    WaffleTaskView {}
    WaffleOSD {}
    StartButton {}
    function runEntry(model, text) {
        const entry = model.find(e => e.text === text);
        if (!entry)
            return false;
        entry.action();
        return true;
    }
}
"#;

fn handlers(rt: &Runtime, target: &str) -> Vec<usize> {
    of_type(rt, "IpcHandler")
        .into_iter()
        .filter(|&h| text(rt, h, "target") == target)
        .collect()
}

/// `ipc call target function` as the shell answers it: on the one handler of `target`.
fn ipc(rt: &mut Runtime, target: &str, function: &str) -> Result<(), String> {
    let hs = handlers(rt, target);
    if hs.len() != 1 {
        return Err(format!("{} handlers for target {target:?}", hs.len()));
    }
    let r = rt
        .call_method(hs[0], function, vec![])
        .ok_or_else(|| format!("{target:?} has no function {function:?}"))?;
    rt.settle();
    r.map(|_| ())
}

/// Finding 10: the "Search" entry of the Start button's menu calls an IPC target and function
/// that exist, and the Start menu opens.
#[test]
fn start_button_search_entry_opens_the_start_menu() {
    isolated!("start_button_search_entry_opens_the_start_menu");
    let (mut rt, root) = load(PANELS, None);
    let menus = of_type(&rt, "BarMenu");
    assert_eq!(menus.len(), 1);
    let model = rt.get(menus[0], "model");
    let ran = rt
        .call_method(root, "runEntry", vec![model, Value::str("Search")])
        .unwrap()
        .unwrap();
    assert!(ran.truthy(), "no Search entry");
    let calls = std::mem::take(&mut rt.world.borrow_mut().self_ipc);
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert!(!rt.get(root, "searchOpen").truthy());
    for (target, function) in calls {
        assert_eq!(ipc(&mut rt, &target, &function), Ok(()));
    }
    assert!(rt.get(root, "searchOpen").truthy());
}

/// Finding 11: no IPC target has two handlers; `search` is the Start menu's (its clipboard
/// toggle included) and the Task View answers on `taskView`.
#[test]
fn every_ipc_target_has_one_handler() {
    isolated!("every_ipc_target_has_one_handler");
    let (mut rt, root) = load(PANELS, None);
    let mut targets: Vec<String> = of_type(&rt, "IpcHandler")
        .into_iter()
        .map(|h| text(&rt, h, "target"))
        .collect();
    targets.sort();
    let mut unique = targets.clone();
    unique.dedup();
    assert_eq!(targets, unique);

    let open = |rt: &Runtime| {
        (
            rt.get(root, "searchOpen").truthy(),
            rt.get(root, "overviewOpen").truthy(),
        )
    };
    assert_eq!(ipc(&mut rt, "search", "toggle"), Ok(()));
    assert_eq!(open(&rt), (true, false));
    assert_eq!(ipc(&mut rt, "search", "close"), Ok(()));
    assert_eq!(open(&rt), (false, false));
    assert_eq!(ipc(&mut rt, "search", "clipboardToggle"), Ok(()));
    assert_eq!(open(&rt), (true, false));
    assert_eq!(ipc(&mut rt, "search", "close"), Ok(()));
    assert_eq!(ipc(&mut rt, "search", "workspacesToggle"), Ok(()));
    assert_eq!(open(&rt), (false, true));
    assert_eq!(ipc(&mut rt, "taskView", "close"), Ok(()));
    assert_eq!(open(&rt), (false, false));
    assert_eq!(ipc(&mut rt, "taskView", "toggle"), Ok(()));
    assert_eq!(open(&rt), (false, true));
}

/// Finding 12: `osd trigger` shows the on-screen display, and its window follows the focused
/// screen without naming an object that does not exist.
#[test]
fn osd_trigger_shows_the_osd() {
    isolated!("osd_trigger_shows_the_osd");
    let (mut rt, root) = load(PANELS, None);
    assert!(!rt.get(root, "osdVolumeOpen").truthy());
    assert_eq!(ipc(&mut rt, "osd", "trigger"), Ok(()));
    assert_eq!(rt.errors(), Vec::<String>::new());
    assert!(rt.get(root, "osdVolumeOpen").truthy());

    // No monitor is focused here, so `focusedScreen` is `undefined`: Quickshell 0.3.2 warns
    // `Cannot assign [undefined] to RustshellScreenInfo*` and the window keeps its screen.
    let osd = of_type(&rt, "WaffleOSD");
    assert_eq!(osd.len(), 1);
    assert!(matches!(rt.get(osd[0], "focusedScreen"), Value::Undefined));
    let window = of_type(&rt, "PanelWindow")
        .into_iter()
        .find(|w| {
            let world = rt.world.borrow();
            std::iter::successors(Some(*w), |i| world.items[*i].parent).any(|i| i == osd[0])
        })
        .unwrap();
    let screen = rt.get(window, "screen").to_js_string();
    rt.emit(osd[0], "focusedScreenChanged", &[], vec![]);
    rt.settle();
    assert_eq!(
        rt.errors(),
        [" Connections.onFocusedScreenChanged: Cannot assign [undefined] to RustshellScreenInfo*"]
    );
    assert_eq!(rt.get(window, "screen").to_js_string(), screen);
}

/// Finding 13: the night light and Bluetooth pages open and close without a Bluetooth adapter.
/// (That the night light page leaves discovery alone with one: sysbus_services.rs.)
#[test]
fn pages_open_without_a_bluetooth_adapter() {
    isolated!("pages_open_without_a_bluetooth_adapter");
    let (mut rt, root) = load(
        r#"import QtQuick
import ri.modules.waffle.actionCenter.nightLight
import ri.modules.waffle.actionCenter.bluetooth
Item {
    id: root
    width: 400
    height: 600
    property bool night: false
    property bool bluetooth: false
    Loader { anchors.fill: parent; active: root.night; sourceComponent: NightLightControl {} }
    Loader { anchors.fill: parent; active: root.bluetooth; sourceComponent: BluetoothControl {} }
}
"#,
        None,
    );
    for (page, ty) in [
        ("night", "NightLightControl"),
        ("bluetooth", "BluetoothControl"),
    ] {
        rt.set(root, page, Value::Bool(true));
        rt.settle();
        assert_eq!(of_type(&rt, ty).len(), 1, "{page} page");
        rt.set(root, page, Value::Bool(false));
        rt.settle();
        assert_eq!(rt.errors(), Vec::<String>::new(), "{page} page");
    }
}

/// Finding 14: the Wi-Fi and Bluetooth lists take their content height from their own rows,
/// not from whatever `contentLayout` the file that shows the page happens to declare (the
/// action center's main page has one).
#[test]
fn page_lists_do_not_bind_to_an_outer_content_layout() {
    isolated!("page_lists_do_not_bind_to_an_outer_content_layout");
    let (rt, _) = load_any(
        r#"import QtQuick
import ri.modules.waffle.actionCenter.wifi
import ri.modules.waffle.actionCenter.bluetooth
Item {
    width: 400
    height: 600
    Item { id: contentLayout; implicitHeight: 12345 }
    WifiControl { width: 400; height: 300 }
    BluetoothControl { y: 300; width: 400; height: 300 }
}
"#,
        None,
    );
    let named: Vec<String> = rt
        .errors()
        .into_iter()
        .filter(|e| e.contains("contentLayout") || e.contains("contentHeight"))
        .collect();
    assert_eq!(named, Vec::<String>::new());
    let lists = of_type(&rt, "StyledListView");
    assert_eq!(lists.len(), 2);
    for l in lists {
        assert_ne!(rt.get(l, "contentHeight").to_number(), 12345.0);
    }
}

/// Finding 15: clicking the taskbar button of an app with no window and no desktop entry does
/// nothing, as its middle click does.
#[test]
fn task_button_without_a_desktop_entry_can_be_clicked() {
    isolated!("task_button_without_a_desktop_entry_can_be_clicked");
    let (mut rt, root) = load(
        r#"import QtQuick
import ri.modules.waffle.bar.tasks
Item {
    property alias button: button
    TaskAppButton {
        id: button
        appEntry: ({ appId: "no-such-app-anywhere", toplevels: [] })
    }
}
"#,
        None,
    );
    let Value::Item(button) = rt.get(root, "button") else {
        panic!("no button")
    };
    assert!(!rt.get(button, "desktopEntry").truthy());
    click(&mut rt, button);
    assert_eq!(rt.errors(), Vec::<String>::new());
}

/// Finding 17: the focus timer's minus button stops at 5 minutes.
#[test]
fn focus_time_stops_at_five_minutes() {
    isolated!("focus_time_stops_at_five_minutes");
    let (mut rt, root) = load(
        r#"import QtQuick
import ri.modules.common
import ri.modules.waffle.notificationCenter
Item {
    width: 400
    height: 100
    property int focusTime: Config.options.time.pomodoro.focus
    function setFocusTime(v) { Config.options.time.pomodoro.focus = v }
    FocusFooter { width: 400 }
}
"#,
        None,
    );
    let minus: Vec<usize> = of_type(&rt, "SmallBorderedIconButton")
        .into_iter()
        .filter(|&b| text(&rt, b, "icon.name") == "subtract")
        .collect();
    assert_eq!(minus.len(), 1);
    rt.call_method(root, "setFocusTime", vec![Value::Num(900.0)])
        .unwrap()
        .unwrap();
    rt.settle();
    let mut seen = vec![];
    for _ in 0..4 {
        click(&mut rt, minus[0]);
        seen.push(rt.get(root, "focusTime").to_number());
    }
    assert_eq!(seen, [600.0, 300.0, 300.0, 300.0]);
}

/// Finding 19: a query change makes the search page create no result objects of its own (the
/// LauncherSearch service makes one per result; the page used to copy each and add up to
/// three more for the preview's actions, none ever destroyed).
#[test]
fn search_page_creates_no_objects_per_query() {
    isolated!("search_page_creates_no_objects_per_query");
    let made = |with_page: bool| {
        let (mut rt, root) = load(
            &format!(
                r#"import QtQuick
import ri.services
import ri.modules.waffle.startMenu
import ri.modules.waffle.startMenu.searchPage
Item {{
    width: 800
    height: 600
    property int results: LauncherSearch.results.length
    property string names: LauncherSearch.results.map(e => e.name).sort().join("|")
    function query(q) {{ LauncherSearch.query = q }}
    StartMenuContext {{ id: menuContext }}
    Loader {{
        anchors.fill: parent
        active: {with_page}
        sourceComponent: SearchResults {{ context: menuContext }}
    }}
}}
"#
            ),
            None,
        );
        rt.world.borrow_mut().track_fresh = true;
        let (mut results, mut made, mut live_max) = (0, 0, 0);
        for i in 0..40 {
            let q = ["1+", "2*", "fi", "term"][i % 4].to_string() + &i.to_string();
            rt.call_method(root, "query", vec![Value::str(&q)])
                .unwrap()
                .unwrap();
            rt.settle();
            results += rt.get(root, "results").to_number() as usize;
            let mut w = rt.world.borrow_mut();
            let fresh = std::mem::take(&mut w.fresh);
            let is_result = |i: usize| w.items[i].types.iter().any(|t| t == "LauncherSearchResult");
            made += fresh.into_iter().filter(|&i| is_result(i)).count();
            let live = (0..w.items.len())
                .filter(|&i| is_result(i) && !w.dead.contains(&i))
                .count();
            live_max = live_max.max(live);
        }
        assert_eq!(rt.errors(), Vec::<String>::new());
        assert!(results >= 40, "{results} results");
        if with_page {
            // The page still shows the last query: a row per result, and the previewed
            // result's own action first among the preview's actions.
            let mut shown: Vec<String> = of_type(&rt, "SearchResultButton")
                .into_iter()
                .map(|row| match rt.get(row, "entry") {
                    Value::Item(e) => text(&rt, e, "name"),
                    v => panic!("row entry {v:?}"),
                })
                .collect();
            shown.sort();
            assert_eq!(shown.join("|"), text(&rt, root, "names"));
            let actions: Vec<String> = of_type(&rt, "WButton")
                .into_iter()
                .filter_map(|b| match rt.get(b, "modelData") {
                    Value::Item(a) => Some(text(&rt, a, "iconName")),
                    _ => None,
                })
                .collect();
            assert!(
                actions.iter().any(|a| a == "keyboard_return"),
                "{actions:?}"
            );
        }
        eprintln!(
            "search page {with_page}: 40 queries, {results} results; LauncherSearchResult \
             objects created {made}, most alive at once {live_max}"
        );
        made
    };
    let service = made(false);
    let page = made(true);
    assert_eq!(
        page, service,
        "result objects made with the page vs by the service alone"
    );
}
