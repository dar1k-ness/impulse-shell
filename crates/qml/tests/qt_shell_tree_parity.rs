//! Four things the original's own QML tree relies on, each with the log Qt 6.12 (PySide6 6.12.0,
//! `QQmlComponent` under the offscreen platform) prints for the same files:
//! `PropertyChanges { id.prop: v }`, an unqualified `visible` under a hidden ancestor, object
//! trees deeper than 64 levels, and `Loader.status` when a source set later fails to load.
use qml::runtime::Runtime;
use qml::value::Value;

/// Loads `main.qml` among `files` and returns its root's `log` after 128 ms of engine time.
fn log_of(test: &str, files: &[(&str, &str)]) -> String {
    let d = std::env::temp_dir().join(format!("qt-shell-tree-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    for (name, text) in files {
        std::fs::write(d.join(name), format!("import QtQuick\n{text}\n")).unwrap();
    }
    let mut rt = Runtime::new(Some(d.clone()));
    let log = match rt.load_file(&d.join("main.qml")) {
        Ok(root) => {
            rt.settle();
            for frame in 1..=8 {
                rt.tick(f64::from(frame) * 16.0);
                rt.run_later();
                rt.settle();
            }
            match rt.world.borrow_mut().read(root, "log") {
                Some(Value::Str(s)) => s.to_string(),
                v => panic!("log: {v:?}"),
            }
        }
        Err(e) => format!("LOAD ERROR {e}"),
    };
    drop(rt);
    let _ = std::fs::remove_dir_all(&d);
    log
}

fn nested(levels: usize, innermost: &str) -> String {
    format!(
        "{}{innermost}{}",
        "Item { ".repeat(levels),
        " }".repeat(levels)
    )
}

/// `Bar.qml` and `VerticalBar.qml` turn their corners this way when the bar is at the bottom.
#[test]
fn property_changes_id_path_applies_and_restores() {
    let log = log_of(
        "pc-apply",
        &[(
            "main.qml",
            r#"Item { id: r; property string log: ""; property bool atBottom: false
  Rectangle { id: leftCorner; property int corner: 0
     onCornerChanged: r.log += "c" + corner + ";"
     states: State { name: "bottom"; when: r.atBottom; PropertyChanges { leftCorner.corner: 2 } } }
  Component.onCompleted: { atBottom = true; log += leftCorner.corner + leftCorner.state + ";"
     atBottom = false; log += leftCorner.corner + "[" + leftCorner.state + "];" } }"#,
        )],
    );
    assert_eq!(log, "c2;2bottom;c0;0[];");
}

#[test]
fn property_changes_id_path_binding_follows_its_sources_while_in_the_state() {
    let log = log_of(
        "pc-binding",
        &[(
            "main.qml",
            r#"Item { id: r; property string log: ""; property bool flip: false; property int n: 1
  Rectangle { id: rect; property int k: 5
     states: State { name: "s"; when: r.flip; PropertyChanges { rect.k: r.n * 10 } } }
  Component.onCompleted: { flip = true; log += rect.k + ";"; n = 4; log += rect.k + ";"
     flip = false; n = 9; log += rect.k + ";" } }"#,
        )],
    );
    assert_eq!(log, "10;40;5;");
}

#[test]
fn property_changes_id_path_reaches_other_objects_and_outer_documents() {
    let log = log_of(
        "pc-objects",
        &[
            (
                "main.qml",
                r#"Item { id: r; property string log: ""; property bool flip: false
  Rectangle { id: a; property int k: 1 }
  Rectangle { id: b; property int k: 2 }
  Rectangle { id: c; property int k: 5 }
  Item { states: State { name: "s"; when: r.flip; PropertyChanges { a.k: 10; b.k: 20 } } }
  Inner { id: inner; on: r.flip }
  function line() { return [a.k, b.k, c.k, inner.k, inner.j].join() + ";" }
  Component.onCompleted: { flip = true; log += line(); flip = false; log += line() } }"#,
            ),
            (
                "Inner.qml",
                r#"Item { id: me; property bool on: false; property int k: 3; property int j: 4
  states: State { name: "s"; when: me.on; PropertyChanges { me.k: 30; c.k: 50; nope.k: 9; me.j: 40 } } }"#,
            ),
        ],
    );
    // An unknown id drops only its own entry.
    assert_eq!(log, "10,20,50,30,40;1,2,5,3,4;");
}

/// Without `target` the values have no scope object: names come from the declaring document.
#[test]
fn property_changes_id_path_values_resolve_in_the_declaring_context() {
    let log = log_of(
        "pc-scope",
        &[(
            "main.qml",
            r#"Item { id: r; property string log: ""; property bool flip: false; width: 300; property int w2: 11
  Item { id: mid; width: 200; property int w2: 22
    Rectangle { id: rect; width: 50; property int k: 5; property int j: 6; property int w2: 33
       states: State { name: "s"; when: r.flip
          PropertyChanges { rect.k: w2; rect.j: parent ? parent.width : -1 } } } }
  Component.onCompleted: { flip = true; log += rect.k + "," + rect.j + ";" } }"#,
        )],
    );
    assert_eq!(log, "11,-1;");
}

/// With an explicit `target` a dotted name stays a path on the target, as before.
#[test]
fn property_changes_with_target_keeps_dotted_names_on_the_target() {
    let log = log_of(
        "pc-target",
        &[(
            "main.qml",
            r#"Item { id: r; property string log: ""; property bool flip: false
  Rectangle { id: a; property int k: 1; property Item b: c
     states: State { name: "s"; when: r.flip; PropertyChanges { target: a; b.k: 30 } } }
  Rectangle { id: b; property int k: 2 }
  Rectangle { id: c; property int k: 4 }
  Component.onCompleted: { flip = true; log += a.k + "," + b.k + "," + c.k + ";" } }"#,
        )],
    );
    assert_eq!(log, "1,2,30;");
}

#[test]
fn property_changes_id_path_is_animated_by_a_transition() {
    let log = log_of(
        "pc-transition",
        &[(
            "main.qml",
            r#"Item { id: r; property string log: ""; property bool flip: false
  Rectangle { id: rect; property real k: 0
     states: State { name: "s"; when: r.flip; PropertyChanges { rect.k: 100 } }
     transitions: Transition { NumberAnimation { properties: "k"; duration: 30 } } }
  Component.onCompleted: { flip = true; log += (rect.k < 100) + ";" }
  Timer { interval: 50; running: true; onTriggered: r.log += rect.k + ";" } }"#,
        )],
    );
    assert_eq!(log, "true;100;");
}

#[test]
fn unqualified_visible_is_false_under_a_hidden_ancestor() {
    let log = log_of(
        "vis-basic",
        &[(
            "main.qml",
            r#"Item { id: r; property string log: b.viaScope + "," + b.viaId + "," + b.viaThis + "," + b.viaFn()
  Item { id: a; visible: false
    Item { id: b; property bool viaScope: visible; property bool viaId: b.visible
      property bool viaThis: this.visible; function viaFn() { return visible } } } }"#,
        )],
    );
    assert_eq!(log, "false,false,false,false");
}

#[test]
fn unqualified_visible_follows_the_ancestor_afterwards() {
    let log = log_of(
        "vis-later",
        &[(
            "main.qml",
            r#"Item { id: r; property string log: ""
  Item { id: a
    Item { id: b; property bool viaScope: visible; property int n: 0
      onVisibleChanged: { n++; r.log += "h" + visible + ";" }
      onViaScopeChanged: r.log += "s" + viaScope + ";" } }
  Component.onCompleted: { a.visible = false; log += b.viaScope + "," + b.n + ";"
     a.visible = true; log += b.viaScope + "," + b.n + ";" } }"#,
        )],
    );
    assert_eq!(log, "strue;hfalse;sfalse;false,1;htrue;strue;true,2;");
}

/// An object that is not an item has no `visible` of its own: the name reaches the root of
/// its document. A component's own file sees the ancestor of the place it is used in.
#[test]
fn unqualified_visible_in_children_and_component_files() {
    let log = log_of(
        "vis-children",
        &[
            (
                "main.qml",
                r#"Item { id: r; property string log: ""; property bool on: false
  Item { id: a; visible: r.on
    Item { id: b; property string s: visible ? "shown" : "hidden"
      QtObject { id: q; property bool v: visible }
      Timer { id: t; running: visible; interval: 1000 }
      Item { id: c; property bool v: visible; property bool pv: parent.visible } }
    Inner { id: inner } }
  function line() { return [b.s, q.v, t.running, c.v, c.pv, inner.rootSees, inner.childSees, inner.ownSees].join() + ";" }
  Component.onCompleted: { log += line(); on = true; log += line(); on = false; log += line() } }"#,
            ),
            (
                "Inner.qml",
                r#"Item { id: me; property bool rootSees: visible; property alias childSees: q.v; property alias ownSees: c.v
  QtObject { id: q; property bool v: visible }
  Item { id: c; visible: true; property bool v: visible } }"#,
            ),
        ],
    );
    assert_eq!(
        log,
        "hidden,true,true,false,false,false,false,false;\
         shown,true,true,true,true,true,true,true;\
         hidden,true,true,false,false,false,false,false;"
    );
}

#[test]
fn unqualified_visible_write_sets_the_own_flag() {
    let log = log_of(
        "vis-write",
        &[(
            "main.qml",
            r#"Item { id: r; property string log: ""
  Item { id: a
    Item { id: b; function hide() { visible = false } function show() { visible = true }
      onVisibleChanged: r.log += "h" + visible + ";" } }
  Component.onCompleted: { b.hide(); log += b.visible + ";"; a.visible = false; b.show(); log += b.visible + ";"
     a.visible = true; log += b.visible + ";" } }"#,
        )],
    );
    assert_eq!(log, "hfalse;false;false;htrue;true;");
}

#[test]
fn objects_nest_deeper_than_64_levels_in_one_file() {
    let main = format!(
        "Item {{ id: r; property string log: \"\"\n{}\n}}",
        nested(
            200,
            "Component.onCompleted: { var n = 0; for (var p = this; p; p = p.parent) n++; r.log = \"\" + n }"
        )
    );
    assert_eq!(log_of("deep-one", &[("main.qml", &main)]), "201");
}

/// Forty levels in each of three files, each instantiated at the bottom of the one before.
#[test]
fn objects_nest_deeper_than_64_levels_across_files() {
    let count = "Component.onCompleted: { var n = 0; for (var p = this; p; p = p.parent) n++; deepest.d = n }";
    let log = log_of(
        "deep-files",
        &[
            (
                "main.qml",
                "Item { id: deepest; property string log: \"\" + deepest.d; property int d: 0\n Deep1 { }\n}",
            ),
            ("Deep1.qml", &nested(40, "Deep2 { }")),
            ("Deep2.qml", &nested(40, "Deep3 { }")),
            ("Deep3.qml", &nested(40, count)),
        ],
    );
    assert_eq!(log, "121");
}

/// Qt refuses such documents when it compiles them; here the load must still end in an error.
#[test]
fn components_instantiating_each_other_fail_to_load() {
    let mutual = log_of(
        "rec-files",
        &[
            ("main.qml", "Item { property string log: \"ok\"; A { } }"),
            ("A.qml", "Item { B { } }"),
            ("B.qml", "Item { A { } }"),
        ],
    );
    assert!(
        mutual.starts_with("LOAD ERROR component recursion"),
        "{mutual}"
    );
    let inline = log_of(
        "rec-inline",
        &[(
            "main.qml",
            "Item { property string log: \"ok\"; component X: Item { X { } }\n X { } }",
        )],
    );
    assert!(
        inline.starts_with("LOAD ERROR component recursion"),
        "{inline}"
    );
}

/// The Qt way to nest a component in itself, a Loader naming its own file, has no such limit
/// to hit: every level is a separate load.
#[test]
fn loader_of_its_own_file_is_not_recursion() {
    let log = log_of(
        "rec-loader",
        &[
            (
                "main.qml",
                r#"Item { id: r; property string log: ""; Node { id: d; n: 3 }
  Component.onCompleted: log = "" + d.total }"#,
            ),
            (
                "Node.qml",
                r#"Item { property int n: 0; property int total: n + (l.item ? l.item.total : 0)
  Loader { id: l; active: parent.n > 0; source: "Node.qml"; onLoaded: item.n = parent.n - 1 } }"#,
            ),
        ],
    );
    assert_eq!(log, "6");
}

const LOADER: &str = r#"Loader { id: l; onStatusChanged: { r.n++; r.log += "s" + status + ";" }
    onItemChanged: r.log += "i" + (item === null) + ";"; onLoaded: r.log += "loaded;" }"#;

fn loader_log(test: &str, rest: &str) -> String {
    let main = format!(
        "Item {{ id: r; property string log: \"\"; property int n: 0\n  {LOADER}\n  {rest} }}"
    );
    log_of(test, &[("main.qml", &main), ("Good.qml", "Item { }")])
}

#[test]
fn loader_without_initial_source_reaches_error_when_the_source_set_later_is_missing() {
    let log = loader_log(
        "ld-empty",
        r#"Component.onCompleted: { l.source = "Missing.qml"; log += "after" + l.status + ";" }
  Timer { interval: 20; running: true; onTriggered: r.log += "t" + l.status + "," + r.n + ";" }"#,
    );
    assert_eq!(log, "s3;itrue;after3;t3,1;");
}

#[test]
fn loader_status_follows_missing_empty_and_good_sources() {
    let log = loader_log(
        "ld-sources",
        r#"Component.onCompleted: { l.source = "Missing.qml"; log += "a" + l.status + ";"
     l.source = "Missing2.qml"; log += "b" + l.status + ";"; l.source = ""; log += "c" + l.status + ";"
     l.source = "Missing.qml"; log += "d" + l.status + ";"; l.source = "Good.qml"; log += "e" + l.status + ";"
     l.source = "Missing.qml"; log += "f" + l.status + ";"; l.source = "Good.qml"; log += "g" + l.status + ";"
     l.source = ""; log += "h" + l.status + ";" }"#,
    );
    assert_eq!(
        log,
        "s3;itrue;a3;itrue;b3;s0;itrue;c0;s3;itrue;d3;ifalse;s1;loaded;e1;s3;itrue;f3;\
         ifalse;s1;loaded;g1;s0;itrue;h0;"
    );
}

#[test]
fn loader_status_with_a_bound_source_and_while_inactive() {
    let bound = log_of(
        "ld-bound",
        &[
            (
                "main.qml",
                r#"Item { id: r; property string log: ""; property string which: ""
  Loader { id: l; source: r.which; onStatusChanged: r.log += "s" + status + ";" }
  Component.onCompleted: { which = "Missing.qml"; log += "a" + l.status + ";"; which = "Good.qml"; log += "b" + l.status + ";"
     which = "Missing.qml"; log += "c" + l.status + ";" } }"#,
            ),
            ("Good.qml", "Item { }"),
        ],
    );
    assert_eq!(bound, "s3;a3;s1;b1;s3;c3;");
    let inactive = log_of(
        "ld-inactive",
        &[(
            "main.qml",
            r#"Item { id: r; property string log: ""
  Loader { id: l; active: false; onStatusChanged: r.log += "s" + status + ";" }
  Component.onCompleted: { l.source = "Missing.qml"; log += "a" + l.status + ";"; l.active = true; log += "b" + l.status + ";"
     l.active = false; log += "c" + l.status + ";" } }"#,
        )],
    );
    assert_eq!(inactive, "a0;s3;b3;s0;c0;");
}
