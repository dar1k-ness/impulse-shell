//! The bar's weather popup (`WeatherPopup.qml`): its cards sit in a `GridLayout` with
//! `uniformCellWidths`, so all of them are equally wide, as in Qt: to the pixel, since the
//! layout puts cell edges on whole pixels (Qt gives 112 and 111 for two columns over 228 with
//! a spacing of 5, see `scenes/qt_geometry/uniform_grid.qml`, `o1`). Runs the tree the build
//! compiled, so without the `interp` feature every script is the compiled one.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::scene;
use std::path::PathBuf;

/// The open popup's window, laid out.
fn popup(rt: &mut Runtime, fonts: &mut scene::Fonts) -> (usize, [f64; 2]) {
    let root = PathBuf::from(ricompiled::ROOT);
    rt.eng()
        .set_compiled(Some(std::rc::Rc::new(ricompiled::Compiled::new(&root))));
    rt.load_singletons(&[
        root.join("modules/common"),
        root.join("modules/common/functions"),
        root.join("modules/common/utils"),
        root.join("services"),
        root.clone(),
    ]);
    rt.set_screens(&[("Virtual-1".into(), 0, 0, 1920, 1080)]);
    let loader = rt
        .load_file(&root.join("modules/rusty/bar/weather/WeatherPopup.qml"))
        .expect("load WeatherPopup.qml");
    rt.settle();
    rt.set(loader, "active", Value::Bool(true));
    rt.settle();
    let win = *scene::find_windows(rt, "rustshell:popup")
        .first()
        .expect("no popup window");
    let mut size = [0.0; 2];
    let mut now = 0.0;
    for _ in 0..4 {
        size = [rt.get(win, "implicitWidth"), rt.get(win, "implicitHeight")].map(|v| v.to_number());
        scene::layout_window(rt, fonts, win, size);
        for _ in 0..60 {
            now += 16.0;
            rt.tick(now);
        }
    }
    (win, size)
}

fn cards(rt: &Runtime, i: usize, out: &mut Vec<(String, f64)>) {
    let (card, kids) = {
        let w = rt.world.borrow();
        let it = &w.items[i];
        (
            it.types.iter().any(|t| t == "WeatherCard"),
            it.children.clone(),
        )
    };
    if card {
        out.push((
            rt.get(i, "title").to_js_string(),
            rt.get(i, "width").to_number(),
        ));
    }
    for c in kids {
        cards(rt, c, out);
    }
}

#[test]
fn weather_cards_are_equally_wide() {
    let mut rt = Runtime::new(Some(PathBuf::from(ricompiled::ROOT)));
    let mut fonts = scene::Fonts::default();
    let (win, _) = popup(&mut rt, &mut fonts);
    let mut found = vec![];
    cards(&rt, win, &mut found);
    assert!(found.len() >= 6, "weather cards: {found:?}");
    let w = found[0].1;
    assert!(
        w > 0.0 && found.iter().all(|c| (c.1 - w).abs() <= 1.0),
        "weather cards differ in width: {found:?}"
    );
}
