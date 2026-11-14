//! A fresh install, with neither `layout.toml` nor the original's config.json: the first outputs the
//! shell learns seed `layout.toml` from the original's defaults, so `LayoutState.containerKinds` lists
//! the panel and the desktop and edit mode (loaded by RustyImpulseFamily.qml once the
//! layout has containers) has its target from the first frame on.
use qml::runtime::Runtime;
use qml::value::Value;
use std::path::PathBuf;

fn ri() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../qml")
}

fn kinds(rt: &Runtime) -> Vec<String> {
    let mut w = rt.world.borrow_mut();
    let s = w
        .singletons
        .get("LayoutState")
        .copied()
        .expect("LayoutState");
    match w.read(s, "containerKinds") {
        Some(Value::Obj(o)) => o
            .borrow()
            .array
            .iter()
            .flatten()
            .map(|v| v.to_js_string())
            .collect(),
        v => panic!("containerKinds {v:?}"),
    }
}

#[test]
fn fresh_home_gets_a_default_layout_with_a_desktop() {
    // The layout's default path follows HOME and the XDG directories: the test runs in a
    // process of its own with them set to a directory of its own.
    const TEST: &str = concat!(
        module_path!(),
        "::fresh_home_gets_a_default_layout_with_a_desktop"
    );
    if !testenv::in_own_process(TEST) {
        let home = std::env::temp_dir().join(format!("ri-layout-fresh-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        testenv::run_in_own_process(
            TEST,
            [
                ("HOME", home.clone()),
                ("XDG_CONFIG_HOME", home.join(".config")),
                ("XDG_STATE_HOME", home.join(".local/state")),
                ("XDG_CACHE_HOME", home.join(".cache")),
            ],
        );
        let _ = std::fs::remove_dir_all(&home);
        return;
    }
    let home = std::path::PathBuf::from(std::env::var_os("HOME").unwrap());
    let path = qml::layout_model::default_path();
    assert!(path.starts_with(&home), "{}", path.display());
    let r = ri();
    let mut rt = Runtime::new(Some(r.clone()));
    rt.load_singletons(&[r.join("modules/common")]);
    rt.world.borrow_mut().open_layout(&path);
    rt.settle();
    assert!(!path.exists());
    assert!(kinds(&rt).is_empty());

    rt.set_screens(&[("Virtual-1".into(), 0, 0, 1920, 1080)]);
    rt.settle();
    let text = std::fs::read_to_string(&path).expect("layout.toml written on the first run");
    assert!(text.contains("connector = \"Virtual-1\""), "{text}");
    let kinds = kinds(&rt);
    assert!(
        kinds.iter().any(|k| k == "panel") && kinds.iter().any(|k| k == "desktop"),
        "{kinds:?}"
    );
    assert!(
        rt.world
            .borrow()
            .errors
            .iter()
            .all(|e| !e.starts_with("layout")),
        "{:?}",
        rt.world.borrow().errors
    );

    // The next start reads the same layout back and seeds nothing over it.
    let mut again = Runtime::new(Some(r.clone()));
    again.load_singletons(&[r.join("modules/common")]);
    again.world.borrow_mut().open_layout(&path);
    again.set_screens(&[("HDMI-A-1".into(), 0, 0, 2560, 1440)]);
    again.settle();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    std::fs::remove_dir_all(&home).unwrap();
}
