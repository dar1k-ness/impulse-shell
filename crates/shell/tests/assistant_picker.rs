//! The model picker of the left sidebar's assistant chat: its list starts at the first model
//! each time it opens, also after a search that left one row (the search text is cleared as
//! the picker opens, so the list goes from that row to all of them in one turn).

use qml::runtime::Runtime;
use qml::value::Value;
use shell::scene;
use std::path::{Path, PathBuf};

const SIZE: [f64; 2] = [344.0, 700.0];

/// What AssistantChat.qml and AssistantMessage.qml read of an AcpSession.
const SESSION: &str = r#"
    QtObject {
        id: session
        property string agentName: main.agentName
        property string program: main.agentName
        property string startedProgram: main.agentName
        property var command: [main.agentName]
        property var messages: []
        property var authMethods: []
        property var models: Array.from({ length: 16 }, (_, i) => ({ value: "m" + i, name: "Model " + i, description: "" }))
        property var modes: []
        property bool authRequired: false
        property bool busy: false
        property bool running: true
        property bool waitingForExit: false
        property string sessionId: "s1"
        property string currentMode: ""
        property string currentModeName: ""
        property string currentModel: ""
        property string currentModelName: ""
        function start() {}
        function clear() {}
        function cancel() {}
        function sendPrompt(text) {}
        function authenticate(id) {}
        function setMode(id) {}
        function setModel(id) {}
    }
"#;

/// The tree, with `main` as its main.qml.
fn tree(name: &str, main: &str) -> PathBuf {
    let ri = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../qml")
        .canonicalize()
        .unwrap();
    let d = std::env::temp_dir().join(format!("assistant-picker-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    for e in std::fs::read_dir(&ri).unwrap() {
        let e = e.unwrap();
        std::os::unix::fs::symlink(e.path(), d.join(e.file_name())).unwrap();
    }
    std::fs::write(d.join("main.qml"), main).unwrap();
    d
}

/// A few frames of `root` in a sidebar's width.
fn frames(rt: &mut Runtime, root: usize) {
    let mut fonts = scene::Fonts::default();
    let mut now = 1000.0;
    for _ in 0..60 {
        now += 16.0;
        rt.tick(now);
        rt.settle();
        scene::layout_window(rt, &mut fonts, root, SIZE);
        rt.settle();
    }
}

/// `main` loaded in the tree and laid out in a sidebar's width.
fn laid_out(name: &str, main: &str) -> (Runtime, PathBuf) {
    let d = tree(name, main);
    let mut rt = Runtime::new(Some(d.clone()));
    rt.load_singletons(&[
        d.join("modules/common"),
        d.join("modules/common/functions"),
        d.join("services"),
    ]);
    rt.settle();
    let root = rt.load_file(&d.join("main.qml")).unwrap();
    rt.settle();
    frames(&mut rt, root);
    (rt, d)
}

/// The first live item of type `ty` that `pick` takes.
fn of(rt: &Runtime, ty: &str, pick: impl Fn(&Runtime, usize) -> bool) -> usize {
    let n = rt.world.borrow().items.len();
    (0..n)
        .find(|&i| {
            let is = {
                let w = rt.world.borrow();
                !w.dead.contains(&i) && w.items[i].types.iter().any(|t| t == ty)
            };
            is && pick(rt, i)
        })
        .unwrap_or_else(|| panic!("no {ty}"))
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn picker_opened_after_a_search_starts_at_the_first_model() {
    let main = format!(
        "import QtQuick\nimport ri.modules.rusty.sidebarLeft\nItem {{\n    id: main\n    width: {}\n    \
         height: {}\n    property string agentName: \"Agent\"\n{SESSION}\n    AssistantChat {{ \
         anchors.fill: parent; session: session }}\n}}\n",
        SIZE[0], SIZE[1]
    );
    let (mut rt, d) = laid_out("search", &main);
    let chat = of(&rt, "AssistantChat", |_, _| true);
    let list = of(&rt, "StyledListView", |rt, i| {
        rt.get(i, "spacing").to_number() == 2.0
    });
    let search = of(&rt, "StyledTextInput", |_, _| true);
    let mut root = chat;
    while let Some(p) = rt.world.borrow().items[root].parent {
        root = p;
    }
    // Rows, where the list stands, and where its first row is.
    let state = |rt: &Runtime| {
        let first = rt.world.borrow().delegated[&list].1.first().copied();
        (
            rt.get(list, "count").to_number(),
            rt.get(list, "contentY").to_number(),
            first.map(|f| rt.get(f, "y").to_number()),
        )
    };
    let open = |rt: &mut Runtime| {
        rt.call_method(chat, "openPicker", vec![Value::str("model")])
            .unwrap()
            .unwrap();
        rt.settle();
        frames(rt, root);
    };
    open(&mut rt);
    let all = state(&rt);
    assert_eq!(all, (16.0, 0.0, Some(0.0)));
    rt.set(search, "text", Value::str("Model 2"));
    rt.settle();
    frames(&mut rt, root);
    assert_eq!(state(&rt).0, 1.0);
    // Enter or a click on the row closes the picker.
    rt.set(chat, "pickerKind", Value::str(""));
    rt.settle();
    frames(&mut rt, root);
    assert_eq!(state(&rt).0, 0.0);
    open(&mut rt);
    assert_eq!(state(&rt), all, "opened again");
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    let _ = std::fs::remove_dir_all(&d);
}
