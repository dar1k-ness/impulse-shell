//! A `property` declared under the name of a FINAL C++ property (`x`, `y`, `z`, `left`,
//! `right` of an Item) is a load error. Each case holds what Qt 6.12's QQmlComponent reports
//! for the same file: the first error only, at the declaration's first keyword. An alias that
//! does not resolve and a `required` property with an initializer are reported instead.

use qml::runtime::Runtime;

const FINAL: &str = "Cannot override FINAL property";

/// Each file with Qt's `line:column: message`.
const CASES: &[(&str, &str, &str)] = &[
    (
        "import QtQuick\nItem {\n property var m: ({a: 7})\n property var x: {\n m[\"a\"];\n }\n property var y: 1\n}\n",
        "4:2",
        FINAL,
    ),
    (
        "import QtQuick\nItem {\n property int x\n Item { id: kid; property int y: parent.x + 1 }\n}\n",
        "3:2",
        FINAL,
    ),
    (
        "import QtQuick\nItem {\n property int q\n Item { id: kid; property int y: parent.q + 1 }\n}\n",
        "4:18",
        FINAL,
    ),
    (
        "import QtQuick\nItem {\n    property int z: 40\n}\n",
        "3:5",
        FINAL,
    ),
    (
        "import QtQuick\nItem {\n    property var z: null\n}\n",
        "3:5",
        FINAL,
    ),
    (
        "import QtQuick\nItem {\n    readonly property real left: 1\n    readonly property real right: 2\n}\n",
        "3:5",
        FINAL,
    ),
    (
        "import QtQuick\nItem { property bool x: true }",
        "2:8",
        FINAL,
    ),
    (
        "import QtQuick\nItem {\n    property var x: null\n    function f() { state = \"a\" }\n}\n",
        "3:5",
        FINAL,
    ),
    (
        "import QtQuick\nItem {\n    property int left: 3\n}\n",
        "3:5",
        FINAL,
    ),
    (
        "import QtQuick\nItem {\n    property int right: 3\n}\n",
        "3:5",
        FINAL,
    ),
    (
        "import QtQuick\nItem {\n    property real y: 3\n}\n",
        "3:5",
        FINAL,
    ),
    (
        "import QtQuick\nItem {\n    default property int z: 5\n}\n",
        "3:5",
        FINAL,
    ),
    (
        "import QtQuick\nItem {\n    required property int z\n}\n",
        "3:5",
        FINAL,
    ),
    (
        "import QtQuick\nItem {\n    Item {\n        Item {\n            property int x: 1; property int y: 2\n        }\n    }\n}\n",
        "5:13",
        FINAL,
    ),
    (
        "import QtQuick\nItem {\n    property alias x: t.font.nope\n    Text { id: t }\n}\n",
        "3:23",
        "Invalid alias target location: nope",
    ),
    (
        "import QtQuick\nItem {\n    property alias x: missing.width\n}\n",
        "3:23",
        "Invalid alias reference. Unable to find id \"missing\"",
    ),
    (
        "import QtQuick\nItem {\n    required property int z: 5\n}\n",
        "3:5",
        "Required properties with initializer do not make sense.",
    ),
];

#[test]
fn a_property_named_like_a_final_one_is_qts_load_error() {
    let mut wrong = vec![];
    for (src, at, msg) in CASES {
        let got = Runtime::new(None)
            .load_str(src)
            .err()
            .unwrap_or_else(|| "loaded".into());
        if !got.ends_with(&format!(":{at}: {msg}")) {
            wrong.push(format!("{src:?}\n  qt:   {at}: {msg}\n  ours: {got}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

thread_local! {
    static CONSOLE: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Qt 6.12 also logs `qt.qml.propertyCache.append: Member implicitWidth of the object … overrides
/// a member of the base object. …` for some hidden names and `Final member x is overridden in
/// class …` before the FINAL error. The engine logs neither (docs/qt-differences.md).
#[test]
fn a_hidden_or_final_name_logs_nothing_besides_the_error() {
    let _ = qml::builtins::CONSOLE_SINK.set(|_, line| {
        CONSOLE.with(|c| c.borrow_mut().push(line.to_string()));
    });
    let mut rt = Runtime::new(None);
    rt.load_str(
        "import QtQuick\nItem {\n property real implicitWidth: 3\n property real scale: 2\n property var state: 1\n}\n",
    )
    .unwrap();
    rt.settle();
    assert_eq!(rt.errors(), Vec::<String>::new());
    assert!(
        Runtime::new(None)
            .load_str("import QtQuick\nItem {\n property int x: 1\n}\n")
            .is_err()
    );
    assert_eq!(CONSOLE.with(|c| c.borrow().clone()), Vec::<String>::new());
}

/// Loads `main.qml` of a directory holding `files`: the error, or `loaded`.
fn load_dir(tag: &str, files: &[(&str, &str)]) -> String {
    let d = std::env::temp_dir().join(format!("ri-final-{}-{tag}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    for (name, src) in files {
        std::fs::write(d.join(name), src).unwrap();
    }
    let got = Runtime::new(Some(d.clone()))
        .load_file(&d.join("main.qml"))
        .err()
        .unwrap_or_else(|| "loaded".into());
    std::fs::remove_dir_all(&d).ok();
    got
}

/// The FINAL properties are those of the C++ type an object is made of. A QML file named like
/// a C++ type that has one (`Page.qml` and QQuickPage::contentData) declares none; a QML type
/// made of that C++ type keeps them. Qt 6.12 loads the first two and refuses the third.
#[test]
fn final_goes_by_the_cpp_type_not_by_the_name_of_a_qml_file() {
    let item = ("Page.qml", "import QtQuick\nItem {}\n");
    assert_eq!(
        load_dir(
            "use",
            &[
                item,
                (
                    "main.qml",
                    "import QtQuick\nPage {\n    property int contentData: 1\n}\n"
                )
            ]
        ),
        "loaded"
    );
    assert_eq!(
        load_dir(
            "alias",
            &[
                (
                    "Page.qml",
                    "import QtQuick\nItem {\n    default property alias contentData: col.data\n    Column { id: col }\n}\n"
                ),
                ("main.qml", "import QtQuick\nPage { Item {} }\n")
            ]
        ),
        "loaded"
    );
    let got = load_dir(
        "derived",
        &[
            ("MyPage.qml", "import QtQuick.Controls\nPage {}\n"),
            (
                "main.qml",
                "import QtQuick\nMyPage {\n    property int contentData: 1\n}\n",
            ),
        ],
    );
    assert!(
        got.ends_with("main.qml:3:5: Cannot override FINAL property"),
        "{got}"
    );
}

/// A declared `scale` goes the way every declared name goes: the engine keeps no list of the
/// items that declare it.
#[test]
fn a_declared_scale_has_no_case_of_its_own() {
    for (file, src) in [
        ("runtime.rs", include_str!("../src/runtime.rs")),
        ("plan.rs", include_str!("../src/plan.rs")),
        ("scene.rs", include_str!("../../shell/src/scene.rs")),
    ] {
        assert!(!src.contains("shadows_scale"), "{file}");
    }
}
