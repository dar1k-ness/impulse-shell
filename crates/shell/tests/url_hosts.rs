//! Hosts outside ASCII and `xn--` hosts in url values, compared with Qt 6.12: the literal of a
//! url property, a string written to one, `Qt.url`, and two urls compared with `==` and `===`
//! (`scenes/url_hosts/scene.qml`). What Qt gives is `scenes/url_hosts/scene.qt`, written by
//! `scenes/url_hosts/dump.sh` in the testenv image (`qml6`), one line per check.
//!
//! The scene is compiled into the test (`testscenes`): a build with the `interp` feature
//! interprets its scripts, a build without it runs the compiled ones.

const QT: &str = include_str!("scenes/url_hosts/scene.qt");

#[test]
fn url_values_hold_hosts_as_qt_does() {
    let (mut rt, root) = testscenes::load("url_hosts/scene.qml");
    rt.settle();
    let rows = rt
        .call_method(root, "rows", vec![])
        .unwrap()
        .unwrap()
        .to_js_string();
    let errors = rt.errors();
    assert!(errors.is_empty(), "{errors:?}");
    let (ours, qt): (Vec<&str>, Vec<&str>) = (rows.lines().collect(), QT.lines().collect());
    assert_eq!(ours.len(), qt.len(), "rows");
    assert!(qt.len() > 120, "{} rows of Qt", qt.len());
    let wrong: Vec<String> = (qt.iter().zip(&ours))
        .filter(|(qt, ours)| qt != ours)
        .map(|(qt, ours)| format!("Qt:   {qt}\nours: {ours}"))
        .collect();
    assert!(
        wrong.is_empty(),
        "{} of {} rows differ\n{}",
        wrong.len(),
        qt.len(),
        wrong.join("\n")
    );
}
