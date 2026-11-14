//! The agent's name in the left sidebar's assistant chat. An answer is headed by the agent that
//! wrote it (`agent` of its message), not by the one the chat talks to now. A name wider than the
//! sidebar (an agent that gives no name is called by its program) is read whole in the two
//! places that show it before a message is sent: the title of the empty chat's placeholder and
//! the hint in the input field, each on as many lines as it takes.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::scene;
use std::path::{Path, PathBuf};

const SIZE: [f64; 2] = [344.0, 700.0];
const NAME: &str = "old-agent-with-a-rather-long-program-name-acp";

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
        property var models: []
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
    let d = std::env::temp_dir().join(format!(
        "assistant-agent-name-{name}-{}",
        std::process::id()
    ));
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

/// The StyledTexts in a `within` that read `text`, visible with all they are in.
fn shown(rt: &Runtime, within: &str, text: &str) -> Vec<usize> {
    let mut w = rt.world.borrow_mut();
    let mut found = vec![];
    for i in 0..w.items.len() {
        if w.dead.contains(&i)
            || !w.items[i].types.iter().any(|t| t == "StyledText")
            || !w.read(i, "text").is_some_and(|v| v.to_js_string() == text)
        {
            continue;
        }
        let (mut at, mut inside) = (Some(i), false);
        while let Some(a) = at.filter(|&a| w.read(a, "visible").is_none_or(|v| v.truthy())) {
            inside |= w.items[a].types.iter().any(|t| t == within);
            at = w.items[a].parent;
            if at.is_none() && inside {
                found.push(i);
            }
        }
    }
    found
}

/// Left edge, top, right edge and bottom of `i` in the window.
fn rect(rt: &Runtime, i: usize) -> [f64; 4] {
    let (mut x, mut y, mut at) = (0.0, 0.0, Some(i));
    while let Some(a) = at {
        x += rt.get(a, "x").to_number();
        y += rt.get(a, "y").to_number();
        at = rt.world.borrow().items[a].parent;
    }
    let (wd, ht) = (
        rt.get(i, "width").to_number(),
        rt.get(i, "height").to_number(),
    );
    [x, y, x + wd, y + ht]
}

/// The one shown StyledText of a `within` that reads `text`: all of it is on its lines, more
/// than one, inside the sidebar.
fn read_whole(rt: &Runtime, within: &str, text: &str) -> usize {
    let found = shown(rt, within, text);
    assert_eq!(found.len(), 1, "texts that read {text:?}");
    let t = found[0];
    assert!(!rt.get(t, "truncated").truthy(), "{text:?} is cut");
    let lines = rt.get(t, "lineCount").to_number();
    assert!(lines >= 2.0, "{text:?} is on {lines} line");
    let [left, _, right, _] = rect(rt, t);
    assert!(
        left >= 0.0 && right <= SIZE[0],
        "{text:?} spans {left}..{right} of the sidebar's {}",
        SIZE[0]
    );
    assert!(
        rt.get(t, "contentWidth").to_number() <= right - left,
        "{text:?} is wider than its item"
    );
    t
}

fn chat(agent: &str) -> String {
    format!(
        "import QtQuick\nimport ri.modules.rusty.sidebarLeft\nItem {{\n    id: main\n    width: {}\n    \
         height: {}\n    property string agentName: \"{agent}\"\n{SESSION}\n    AssistantChat {{ \
         anchors.fill: parent; session: session }}\n}}\n",
        SIZE[0], SIZE[1]
    )
}

#[test]
fn old_answer_is_headed_by_the_agent_that_wrote_it() {
    let main = format!(
        "import QtQuick\nimport ri.modules.rusty.sidebarLeft.assistant\nItem {{\n    id: main\n    \
         width: {}\n    height: {}\n    property string agentName: \"New Agent\"\n{SESSION}\n    \
         AssistantMessage {{\n        session: session\n        messageData: ({{ role: \
         \"assistant\", content: \"done\", done: true, agent: \"Old Agent\", model: \"m1\" }})\n    \
         }}\n}}\n",
        SIZE[0], SIZE[1]
    );
    let (rt, d) = laid_out("answer", &main);
    assert_eq!(
        shown(&rt, "AssistantMessage", "Old Agent · m1").len(),
        1,
        "the answer's header"
    );
    assert!(shown(&rt, "AssistantMessage", "New Agent · m1").is_empty());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn long_name_wraps_in_the_empty_chat_placeholder() {
    let (rt, d) = laid_out("placeholder", &chat(NAME));
    let title = read_whole(&rt, "PagePlaceholder", NAME);
    // The description under it starts below the title's last line.
    let about = shown(
        &rt,
        "PagePlaceholder",
        &format!("Talking to {NAME} over the Agent Client Protocol"),
    );
    assert_eq!(about.len(), 1, "the placeholder's description");
    assert!(rect(&rt, title)[3] <= rect(&rt, about[0])[1]);
    let _ = std::fs::remove_dir_all(&d);
}

/// The chat's input field and the area it is in.
fn input(rt: &Runtime) -> (usize, usize) {
    let w = rt.world.borrow();
    let is = |i: usize, ty: &str| w.items[i].types.iter().any(|t| t == ty);
    let field = (0..w.items.len())
        .find(|&i| !w.dead.contains(&i) && is(i, "StyledTextArea"))
        .expect("the input field");
    let mut area = field;
    while !is(area, "Rectangle") {
        area = w.items[area]
            .parent
            .expect("the field is in the input area");
    }
    (field, area)
}

fn root_of(rt: &Runtime, mut i: usize) -> usize {
    while let Some(p) = rt.world.borrow().items[i].parent {
        i = p;
    }
    i
}

#[test]
fn long_name_wraps_in_the_input_hint() {
    // The field has room for the hint's lines: one line's height cuts the hint below its first.
    let (mut rt, d) = laid_out("hint", &chat("Agent"));
    let (_, area) = input(&rt);
    let one_line = rt.get(area, "height").to_number();
    let root = root_of(&rt, area);
    rt.world
        .borrow_mut()
        .write(root, "agentName", Value::str(NAME));
    rt.settle();
    frames(&mut rt, root);
    let high = rt.get(area, "height").to_number();
    assert!(
        high > one_line,
        "the input area is {high} high, as for a hint of one line ({one_line})"
    );
    let hint = read_whole(&rt, "AssistantChat", &format!("Message {NAME}..."));
    // The hint's lines end above the row of chips.
    let ([_, top, _, bottom], [_, area_top, _, area_bottom]) = (rect(&rt, hint), rect(&rt, area));
    assert!(
        area_top <= top && bottom + 20.0 <= area_bottom,
        "hint {top}..{bottom} in the input area {area_top}..{area_bottom}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// A hint of one line is where the field's own placeholder was, at the field's paddings, and
/// the input area is as high with it as with a line typed.
#[test]
fn short_hint_takes_the_place_of_a_typed_line() {
    let (mut rt, d) = laid_out("short", &chat("Agent"));
    let found = shown(&rt, "AssistantChat", "Message Agent...");
    assert_eq!(found.len(), 1, "the hint");
    assert_eq!(rt.get(found[0], "lineCount").to_number(), 1.0);
    let (field, area) = input(&rt);
    let ([left, top, ..], [field_left, field_top, ..]) = (rect(&rt, found[0]), rect(&rt, field));
    let root = root_of(&rt, area);
    // Controls' placeholder is at the field's paddings (TextArea.qml), as its text is.
    let of_field = |k: &str| rt.get(field, k).to_number();
    assert_eq!(
        [left - field_left, top - field_top],
        [of_field("leftPadding"), of_field("topPadding")],
        "the hint in the field"
    );
    // It does not make the field higher than its one line.
    assert_eq!(of_field("height"), of_field("implicitHeight"));
    let with_hint = rt.get(area, "height").to_number();

    rt.world
        .borrow_mut()
        .write(field, "text", Value::str("typed"));
    rt.settle();
    frames(&mut rt, root);
    assert!(shown(&rt, "AssistantChat", "Message Agent...").is_empty());
    assert_eq!(with_hint, rt.get(area, "height").to_number());
    let _ = std::fs::remove_dir_all(&d);
}

/// Lines typed past the height the field stops growing at stay whole: the field is as wide as
/// its view, not as its widest line so far, so a wider line after those is not broken.
#[test]
fn a_wider_line_typed_past_the_fields_height_stays_whole() {
    let (mut rt, d) = laid_out("lines", &chat("Agent"));
    let (field, area) = input(&rt);
    let root = root_of(&rt, area);
    let mut text = String::new();
    let mut high = vec![];
    for n in 0..30 {
        text += "i\n";
        rt.world
            .borrow_mut()
            .write(field, "text", Value::str(&text));
        rt.settle();
        frames(&mut rt, root);
        high.push(rt.get(area, "height").to_number());
        assert_eq!(rt.get(field, "lineCount").to_number(), n as f64 + 2.0);
    }
    assert_eq!(high[29], high[25], "the input area has stopped growing");
    text += "more words";
    rt.world
        .borrow_mut()
        .write(field, "text", Value::str(&text));
    rt.settle();
    frames(&mut rt, root);
    assert_eq!(
        rt.get(field, "lineCount").to_number(),
        31.0,
        "\"more words\" is on one line"
    );
    // The field is one line high again when its text is gone.
    rt.world.borrow_mut().write(field, "text", Value::str(""));
    rt.settle();
    frames(&mut rt, root);
    assert_eq!(
        rt.get(area, "height").to_number(),
        high[0] - (high[1] - high[0])
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// The field is as wide as Controls' placeholder made it in ref/ii: as its hint and its
/// paddings, and no wider than the view when the hint is.
#[test]
fn the_field_is_as_wide_as_its_hint_and_no_wider_than_the_view() {
    let (mut rt, d) = laid_out("wide", &chat("Agent"));
    let (field, area) = input(&rt);
    let root = root_of(&rt, area);
    let view = {
        let w = rt.world.borrow();
        let mut i = field;
        while !w.items[i].types.iter().any(|t| t == "ScrollView") {
            i = w.items[i].parent.expect("the field is in a ScrollView");
        }
        i
    };
    let of = |rt: &Runtime, i: usize, k: &str| rt.get(i, k).to_number();
    let pads = of(&rt, field, "leftPadding") + of(&rt, field, "rightPadding");
    let hint = shown(&rt, "AssistantChat", "Message Agent...")[0];
    assert!(of(&rt, hint, "implicitWidth") + pads < of(&rt, view, "availableWidth"));
    assert_eq!(
        of(&rt, field, "width"),
        of(&rt, hint, "implicitWidth") + pads
    );
    rt.world
        .borrow_mut()
        .write(root, "agentName", Value::str(NAME));
    rt.settle();
    frames(&mut rt, root);
    assert_eq!(of(&rt, field, "width"), of(&rt, view, "availableWidth"));
    let _ = std::fs::remove_dir_all(&d);
}
