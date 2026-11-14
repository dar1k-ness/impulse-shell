//! The assistant chat's line for an agent that was asked to end and has not exited
//! (AssistantMessage.qml, role `ending`): it names the agent, whole and on as many lines as
//! the name takes, and its button calls the session's `kill`. A prompt in the queue says so in its header while that agent is waited
//! for, and only then.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::scene;
use std::path::{Path, PathBuf};

const SIZE: [f64; 2] = [344.0, 300.0];

const MAIN: &str = r#"import QtQuick
import ri.modules.rusty.sidebarLeft.assistant
Item {
    id: main
    width: 344
    height: 300
    property int kills: 0
    property bool waiting: true
    QtObject {
        id: session
        readonly property bool waitingForExit: main.waiting
        function kill() { main.kills++ }
    }
    Column {
        anchors.fill: parent
        Item {
            width: parent.width
            height: 80
            AssistantMessage {
                session: session
                messageData: ({ role: "ending", content: "old-agent-with-a-rather-long-program-name-acp" })
            }
        }
        Item {
            width: parent.width
            height: 120
            AssistantMessage {
                session: session
                messageData: ({ role: "user", content: "task", done: true, queued: true })
            }
        }
    }
}
"#;

/// The tree, with a main.qml of this test next to its files.
fn tree() -> PathBuf {
    let ri = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../qml")
        .canonicalize()
        .unwrap();
    let d = std::env::temp_dir().join(format!("assistant-ending-line-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    for e in std::fs::read_dir(&ri).unwrap() {
        let e = e.unwrap();
        std::os::unix::fs::symlink(e.path(), d.join(e.file_name())).unwrap();
    }
    std::fs::write(d.join("main.qml"), MAIN).unwrap();
    d
}

/// The items of type `ty` whose `text` satisfies `is`, visible with all they are in.
fn shown(rt: &Runtime, ty: &str, is: &dyn Fn(&str) -> bool) -> Vec<usize> {
    let mut w = rt.world.borrow_mut();
    let mut found = vec![];
    for i in 0..w.items.len() {
        if w.dead.contains(&i)
            || !w.items[i].types.iter().any(|t| t == ty)
            || !w.read(i, "text").is_some_and(|v| is(&v.to_js_string()))
        {
            continue;
        }
        let mut at = Some(i);
        while let Some(a) = at.filter(|&a| w.read(a, "visible").is_none_or(|v| v.truthy())) {
            at = w.items[a].parent;
            if at.is_none() {
                found.push(i);
            }
        }
    }
    found
}

#[test]
fn ending_line_names_the_agent_and_its_button_kills_it() {
    let d = tree();
    let mut rt = Runtime::new(Some(d.clone()));
    rt.load_singletons(&[
        d.join("modules/common"),
        d.join("modules/common/functions"),
        d.join("services"),
    ]);
    rt.settle();
    let main = rt.load_file(&d.join("main.qml")).unwrap();
    rt.settle();
    let mut fonts = scene::Fonts::default();
    let mut frame = |rt: &mut Runtime| {
        rt.tick(1000.0);
        rt.settle();
        scene::layout_window(rt, &mut fonts, main, SIZE);
        rt.settle();
    };
    frame(&mut rt);

    let line = shown(&rt, "StyledText", &|t| {
        t == "Waiting for old-agent-with-a-rather-long-program-name-acp to exit"
    });
    assert_eq!(line.len(), 1, "the line's text");
    // A name this long does not fit the line next to the button: it is wrapped, not cut.
    assert!(!rt.get(line[0], "truncated").truthy(), "the name is cut");
    assert!(rt.get(line[0], "lineCount").to_number() >= 2.0);
    let (left, width) = (
        rt.get(line[0], "x").to_number(),
        rt.get(line[0], "width").to_number(),
    );
    let label = shown(&rt, "StyledText", &|t| t == "End now");
    assert_eq!(label.len(), 1, "the button's text");
    let button = {
        let w = rt.world.borrow();
        let mut i = label[0];
        while !w.items[i].types.iter().any(|t| t == "RippleButton") {
            i = w.items[i].parent.expect("the label is in a RippleButton");
        }
        i
    };
    assert!(
        left + width <= rt.get(button, "x").to_number(),
        "the text runs under the button"
    );
    rt.emit(button, "clicked", &[], vec![]);
    rt.settle();
    assert_eq!(rt.get(main, "kills").to_number(), 1.0);

    let in_queue =
        |rt: &Runtime| shown(rt, "StyledText", &|t| t.ends_with(" · In the queue")).len();
    assert_eq!(in_queue(&rt), 1, "the queued prompt's header");
    rt.world
        .borrow_mut()
        .write(main, "waiting", Value::Bool(false));
    rt.settle();
    frame(&mut rt);
    assert_eq!(in_queue(&rt), 0, "no agent is waited for");
    let _ = std::fs::remove_dir_all(&d);
}
