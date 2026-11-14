//! A fresh HOME without config.json: the original's Config writes the whole adapter (`onLoadFailed` →
//! `writeAdapter()`), reloads it through the file watch and becomes ready, so the settings
//! pages (`active: Config.ready && ...`) build, as under Quickshell.
mod event_loop;
use event_loop::Loop;
use qml::runtime::Runtime;
use qml::value::Value;
use std::path::PathBuf;

fn ri() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../qml")
}

/// The top-level options Config.qml's JsonAdapter declares.
fn adapter_keys() -> Vec<String> {
    let text = std::fs::read_to_string(ri().join("modules/common/Config.qml")).unwrap();
    let body = &text[text.find("JsonAdapter {").unwrap()..];
    body.lines()
        .filter_map(|l| l.strip_prefix("            property "))
        .filter_map(|l| l.split_whitespace().nth(1))
        .map(|n| n.trim_end_matches(':').to_string())
        .collect()
}

#[test]
fn fresh_home_writes_config_and_builds_settings_pages() {
    const TEST: &str = concat!(
        module_path!(),
        "::fresh_home_writes_config_and_builds_settings_pages"
    );
    if !testenv::in_own_process(TEST) {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        testenv::run_in_own_process(TEST, testenv::home_vars(home));
        return;
    }
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    let cfg = home.join(".config/rusty-impulse/config.json");
    let r = ri();
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
    rt.load_file(&r.join("settings.qml")).unwrap();
    let mut lp = Loop::new(&mut rt);
    let ready = |rt: &Runtime| {
        let mut w = rt.world.borrow_mut();
        let c = w.singletons.get("Config").copied();
        c.and_then(|c| w.read(c, "ready"))
            .is_some_and(|v| v.truthy())
    };
    assert!(
        lp.until(&mut rt, |rt| ready(rt)),
        "Config.ready never became true on a fresh HOME"
    );
    lp.advance(&mut rt, 1600.0);

    let text = std::fs::read_to_string(&cfg).expect("config.json written");
    let json: serde_json::Value = serde_json::from_str(&text).expect("config.json parses");
    let obj = json.as_object().expect("config.json is an object");
    let keys = adapter_keys();
    assert!(keys.len() > 20, "{keys:?}");
    let missing: Vec<_> = keys.iter().filter(|k| !obj.contains_key(*k)).collect();
    assert!(missing.is_empty(), "config.json lacks {missing:?}");
    assert_eq!(obj["panelFamily"], "rusty");
    assert!(obj["bar"]["weather"].is_object());

    let w = rt.world.borrow();
    let shown = (0..w.items.len())
        .filter(|i| !w.dead.contains(i))
        .filter(|&i| {
            matches!(w.items[i].props.get("source").map(|p| &p.value),
                Some(Value::Str(s)) if s.contains("modules/settings/"))
                && matches!(
                    w.items[i].props.get("item").map(|p| &p.value),
                    Some(Value::Item(_))
                )
        })
        .count();
    drop(w);
    assert!(shown > 0, "no settings page built");
}
