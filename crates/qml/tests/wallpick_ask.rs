//! "Pick for me" in the settings (settings/QuickConfig.qml, WallpickAsk.qml): the button does
//! not start the picker, it opens a dialog over the window that asks what the style should go
//! by, the desktop as it is now or the user's own words, and the picker is started with that
//! answer.
use qml::runtime::Runtime;
use qml::value::Value;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

fn ri() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../qml")
}

/// The live item of type `ty` whose `prop` is `text`.
fn labelled(rt: &Runtime, ty: &str, prop: &str, text: &str) -> usize {
    let ids = rt.world.borrow_mut().of_type(ty);
    ids.into_iter()
        .find(|&b| !rt.world.borrow().dead.contains(&b) && rt.get(b, prop).to_js_string() == text)
        .unwrap_or_else(|| panic!("no {ty} {text:?}: {:?}", rt.errors()))
}

#[test]
fn pick_for_me_asks_what_to_go_by_before_it_picks() {
    const TEST: &str = concat!(
        module_path!(),
        "::pick_for_me_asks_what_to_go_by_before_it_picks"
    );
    if !testenv::in_own_process(TEST) {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        std::fs::create_dir_all(home.join(".config/impulse-shell")).unwrap();
        std::fs::write(home.join(".config/impulse-shell/config.json"), "{}\n").unwrap();
        testenv::run_in_own_process(TEST, testenv::home_vars(home));
        return;
    }
    // The picker's runs end at once; what they were asked is kept.
    let runs = Rc::new(RefCell::new(Vec::<Vec<String>>::new()));
    let log = runs.clone();
    qml::native::set_spawn_hook(Some(Box::new(move |argv: &[String]| {
        if argv.first().is_some_and(|a| a == "WallpickRun") {
            log.borrow_mut().push(argv[2..].to_vec());
        }
        Some(String::new())
    })));
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
    rt.load_file(&r.join("modules/settings/QuickConfig.qml"))
        .unwrap();
    // settings.qml lays the dialog over the window
    let ask = rt
        .load_file(&r.join("modules/settings/WallpickAsk.qml"))
        .unwrap();
    // Long enough for the dialog to open or close
    let mut now = 1000.0;
    let mut turn = |rt: &mut Runtime| {
        for _ in 0..40 {
            rt.poll_native();
            now += 25.0;
            rt.tick(now);
        }
    };
    turn(&mut rt);
    assert!(
        !rt.get(ask, "visible").truthy(),
        "the question shows unasked"
    );

    let pick = labelled(&rt, "RippleButtonWithIcon", "mainText", "Pick for me");
    rt.emit(pick, "clicked", &[], vec![]);
    turn(&mut rt);
    assert!(rt.get(ask, "visible").truthy(), "the question did not open");
    assert!(runs.borrow().is_empty(), "the picker ran unasked");

    // Cancel: the question goes away and nothing was picked.
    let cancel = labelled(&rt, "DialogButton", "buttonText", "Cancel");
    rt.emit(cancel, "clicked", &[], vec![]);
    turn(&mut rt);
    assert!(!rt.get(ask, "visible").truthy(), "Cancel left the question");
    assert!(runs.borrow().is_empty(), "Cancel picked");

    // By the desktop: the picker runs with that, and the question goes away.
    rt.emit(pick, "clicked", &[], vec![]);
    turn(&mut rt);
    rt.call_method(ask, "byDesktop", vec![]).unwrap().unwrap();
    turn(&mut rt);
    assert_eq!(*runs.borrow(), [["pick", "--desktop"]]);
    assert!(!rt.get(ask, "visible").truthy());

    // By words: nothing to go by while the field is empty.
    rt.emit(pick, "clicked", &[], vec![]);
    turn(&mut rt);
    let words = labelled(&rt, "DialogButton", "buttonText", "By these words");
    assert!(!rt.get(words, "enabled").truthy());
    rt.call_method(ask, "byWords", vec![]).unwrap().unwrap();
    assert_eq!(runs.borrow().len(), 1);
    let input = {
        let found = rt.world.borrow_mut().of_type("StyledTextInput");
        let w = rt.world.borrow();
        let inside = found.into_iter().find(|&i| w.is_ancestor(ask, i));
        inside.expect("no text field in the question")
    };
    rt.set(input, "text", Value::str("  ночной город под дождём "));
    turn(&mut rt);
    assert!(rt.get(words, "enabled").truthy());
    rt.emit(words, "clicked", &[], vec![]);
    turn(&mut rt);
    assert_eq!(
        runs.borrow()[1..],
        [["pick", "--words", "ночной город под дождём"]]
    );
    assert!(!rt.get(ask, "visible").truthy());
    qml::native::set_spawn_hook(None);
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
}
