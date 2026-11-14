//! `String.prototype.normalize` of the six code points Qt 6.12 does not compose
//! (docs/qt-differences.md, "Six code points QString::normalized does not compose"), in the
//! interpreter and in compiled code. `scenes/normalize_nfc/scene.qt` is what `qml6` of Qt 6.12
//! gives (`dump.sh` writes it); the `normalized` rows of `crates/qml/tests/qt_unicode/qt.tsv`
//! hold the same strings through `QString::normalized` and through ICU 78 of the same image.

const QT_JS: &str = include_str!("scenes/normalize_nfc/scene.qt");
const QT: &str = include_str!("../../qml/tests/qt_unicode/qt.tsv");

#[test]
fn normalize_composes_as_icu_where_qt_does_not() {
    let (mut rt, root) = testscenes::load("normalize_nfc/scene.qml");
    rt.settle();
    let rows = rt
        .call_method(root, "rows", vec![])
        .unwrap()
        .unwrap()
        .to_js_string();
    let errors = rt.errors();
    assert!(errors.is_empty(), "{errors:?}");

    // (code point, string, form, QString::normalized, ICU)
    let normalized: Vec<Vec<&str>> = (QT.lines().map(|l| l.split('\t').collect()))
        .filter(|f: &Vec<&str>| f[0] == "normalized")
        .collect();
    let (ours, qt_js): (Vec<&str>, Vec<&str>) = (rows.lines().collect(), QT_JS.lines().collect());
    assert_eq!(normalized.len(), 72);
    assert_eq!((ours.len(), qt_js.len()), (72, 72));

    let mut differ = 0;
    for ((row, ours), qt_js) in normalized.iter().zip(ours).zip(qt_js) {
        let case = row[1..4].join("\t");
        // Qt's `normalize` is `QString::normalized`.
        assert_eq!(qt_js, format!("{case}\t{}", row[4]));
        assert_eq!(ours, format!("{case}\t{}", row[5]), "Qt: {}", row[4]);
        differ += usize::from(ours != qt_js);
    }
    assert_eq!(differ, 30);
}
