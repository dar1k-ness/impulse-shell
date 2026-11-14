//! The status icon of a tool call in the assistant chat (AssistantMessage.qml): it spins while
//! the call is in progress, and when the call ends it stands upright again, with the item the
//! message sits in left as it was. The animation reset `parent.rotation`, and an animation has
//! no `parent` of its own in Qt: the name is the message's `parent` property there, so the icon
//! kept the angle it stopped at and the message's parent was turned instead. (This runtime gives
//! every object a `parent`, the icon here, so this test also passed before the icon was named.)

use qml::runtime::Runtime;
use qml::value::Value;
use shell::scene;
use std::path::{Path, PathBuf};

const SIZE: [f64; 2] = [344.0, 200.0];

const MAIN: &str = r#"import QtQuick
import ri.modules.rusty.sidebarLeft.assistant
Item {
    id: main
    width: 344
    height: 200
    property string status: "in_progress"
    property Item holder: holder
    Item {
        id: holder
        anchors.fill: parent
        rotation: 5
        AssistantMessage {
            messageData: ({ role: "tool", kind: "execute", title: "ls", status: main.status })
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
    let d = std::env::temp_dir().join(format!("assistant-status-icon-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    for e in std::fs::read_dir(&ri).unwrap() {
        let e = e.unwrap();
        std::os::unix::fs::symlink(e.path(), d.join(e.file_name())).unwrap();
    }
    std::fs::write(d.join("main.qml"), MAIN).unwrap();
    d
}

#[test]
fn status_icon_stands_upright_once_the_call_ends() {
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
    let mut now = 1000.0;
    let mut frames = |rt: &mut Runtime, n: usize| {
        for _ in 0..n {
            now += 16.0;
            rt.tick(now);
            rt.settle();
            scene::layout_window(rt, &mut fonts, main, SIZE);
            rt.settle();
        }
    };
    frames(&mut rt, 30);
    let Value::Item(holder) = rt.get(main, "holder") else {
        panic!("no holder");
    };
    let icon = {
        let mut w = rt.world.borrow_mut();
        let hits: Vec<usize> = (0..w.items.len())
            .filter(|&i| {
                !w.dead.contains(&i)
                    && w.items[i].types.iter().any(|t| t == "MaterialSymbol")
                    && w.read(i, "text")
                        .is_some_and(|v| v.to_js_string() == "progress_activity")
            })
            .collect();
        assert_eq!(hits.len(), 1, "status icons: {hits:?}");
        hits[0]
    };
    // 30 frames of 16 ms into a turn of 1200 ms.
    let turned = rt.get(icon, "rotation").to_number();
    assert!(
        turned > 0.0 && turned < 360.0,
        "fixture: it spins: {turned}"
    );

    rt.world
        .borrow_mut()
        .write(main, "status", Value::str("completed"));
    rt.settle();
    frames(&mut rt, 2);
    assert_eq!(rt.get(icon, "text").to_js_string(), "check_circle");
    assert_eq!(rt.get(icon, "rotation").to_number(), 0.0);
    assert_eq!(rt.get(holder, "rotation").to_number(), 5.0);
    let _ = std::fs::remove_dir_all(&d);
}
