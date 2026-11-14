//! Bugs the tree had when it was taken from the original (reported as end-4/dots-hyprland issues), fixed
//! here: the schema/consumer pairs the runtime reads.

use qml::runtime::Runtime;
use qml::value::Value;
use std::path::{Path, PathBuf};

fn ri() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../qml")
        .canonicalize()
        .unwrap()
}

/// #3593: `Config.qml` declared `notifications.monitor` while `NotificationPopup.qml` and the
/// settings page read `notifications.forceMonitor`.
#[test]
fn notifications_force_monitor_is_in_the_schema() {
    const TEST: &str = concat!(
        module_path!(),
        "::notifications_force_monitor_is_in_the_schema"
    );
    if !testenv::in_own_process(TEST) {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        testenv::run_in_own_process(
            TEST,
            [
                ("XDG_CONFIG_HOME", home.join("config")),
                ("XDG_STATE_HOME", home.join("state")),
                ("XDG_CACHE_HOME", home.join("cache")),
            ],
        );
        return;
    }
    qml::native::set_spawn_hook(Some(Box::new(|_| Some(String::new()))));
    let mut rt = Runtime::new(Some(ri()));
    rt.load_singletons(&[
        ri().join("modules/common"),
        ri().join("modules/common/functions"),
    ]);
    rt.settle();
    let r = rt
        .load_str(
            "import QtQuick\nimport ri.modules.common\nItem {\n\
             property bool declared: Config.options.notifications.forceMonitor !== undefined\n\
             property bool enable: Config.options.notifications.forceMonitor.enable\n\
             property string name: Config.options.notifications.forceMonitor.name\n\
             function set() { Config.options.notifications.forceMonitor.enable = true; Config.options.notifications.forceMonitor.name = \"DP-1\" }\n\
             }",
        )
        .unwrap();
    rt.settle();
    rt.poll_native();
    rt.settle();
    assert!(
        matches!(rt.get(r, "declared"), Value::Bool(true)),
        "declared"
    );
    assert!(matches!(rt.get(r, "enable"), Value::Bool(false)), "enable");
    assert_eq!(rt.get(r, "name").to_js_string(), "");
    let _ = rt.call_method(r, "set", vec![]).unwrap();
    rt.settle();
    assert!(matches!(rt.get(r, "enable"), Value::Bool(true)), "enable");
    assert_eq!(rt.get(r, "name").to_js_string(), "DP-1");
    let errs = rt.errors();
    assert!(!errs.iter().any(|e| e.contains("forceMonitor")), "{errs:?}");
    qml::native::set_spawn_hook(None);
}

fn declared(qml: &str) -> Vec<String> {
    let mut v = vec![];
    for l in qml.lines() {
        let w: Vec<&str> = l.split_whitespace().collect();
        let at = |k: &str| w.iter().position(|x| *x == k);
        if let Some(i) = at("property") {
            if let Some(n) = w.get(i + 2) {
                v.push(n.trim_end_matches(':').to_string());
            }
        } else if let Some(i) = at("function").or(at("signal"))
            && let Some(n) = w.get(i + 1)
        {
            v.push(n.split('(').next().unwrap().to_string());
        }
    }
    v
}

/// #3709: `BarContent.qml` bound `padding` to `workspacesWidget.widgetPadding`, which
/// `Workspaces.qml` (nor its `ButtonMouseArea`/`MouseArea` base) never declared. The bar's
/// groups now live in `BarGroups.qml`.
#[test]
fn bar_content_reads_only_declared_workspaces_members() {
    let bar = std::fs::read_to_string(ri().join("modules/rusty/bar/BarGroups.qml")).unwrap();
    let mut known =
        declared(&std::fs::read_to_string(ri().join("modules/rusty/bar/Workspaces.qml")).unwrap());
    known.extend(declared(
        &std::fs::read_to_string(ri().join("modules/common/widgets/ButtonMouseArea.qml")).unwrap(),
    ));
    known.extend(
        [
            "width",
            "height",
            "implicitWidth",
            "implicitHeight",
            "x",
            "y",
            "visible",
            "containsMouse",
            "pressed",
            "hoverEnabled",
            "enabled",
        ]
        .map(String::from),
    );
    let pat = "workspacesWidget.";
    let used: Vec<&str> = bar
        .match_indices(pat)
        .map(|(i, _)| {
            let rest = &bar[i + pat.len()..];
            &rest[..rest
                .find(|c: char| !c.is_alphanumeric() && c != '_')
                .unwrap_or(rest.len())]
        })
        .collect();
    let missing: Vec<&&str> = used
        .iter()
        .filter(|u| !known.iter().any(|k| k == *u))
        .collect();
    assert!(
        missing.is_empty(),
        "undeclared Workspaces members: {missing:?}"
    );
    assert!(bar.contains("id: workspacesWidget"));
}
