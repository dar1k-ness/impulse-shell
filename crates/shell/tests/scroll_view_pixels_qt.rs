//! What a ScrollView draws, against Qt 6.12 itself (support/qt_window.rs, Basic style): the
//! text of a TextArea is clipped by the paddings of the field, and the attached ScrollBars are
//! drawn where Qt draws them.
//!
//! With `SCROLL_VIEW_SHOTS=DIR` every scene is also written to DIR as `NAME-qt.png` and
//! `NAME-ours.png`.

#[path = "support/qt_window.rs"]
mod qt_window;

use qsg::raster::Canvas;

const BACK: [u8; 3] = [0x20, 0x20, 0x20];
const FONT: &str = "font.family: \"DejaVu Sans\"; font.pixelSize: 14; color: \"white\"";

fn both(name: &str, body: &str, size: (usize, usize)) -> Option<(Canvas, Canvas)> {
    let (qt, ours) = qt_window::both(body, size, 1.0)?;
    if let Some(dir) = std::env::var_os("SCROLL_VIEW_SHOTS") {
        let dir = std::path::Path::new(&dir);
        qt.save_png(dir.join(format!("{name}-qt.png")).to_str().unwrap())
            .unwrap();
        ours.save_png(dir.join(format!("{name}-ours.png")).to_str().unwrap())
            .unwrap();
    }
    Some((qt, ours))
}

/// The box of the pixels that are clearly not the background: [left, top, right, bottom], the
/// last two past the end.
fn ink(c: &Canvas) -> [usize; 4] {
    let mut b = [c.w, c.h, 0, 0];
    for (k, p) in c.px.iter().enumerate() {
        if (0..3).any(|n| p[n].abs_diff(BACK[n]) > 64) {
            let (x, y) = (k % c.w, k / c.w);
            b = [b[0].min(x), b[1].min(y), b[2].max(x + 1), b[3].max(y + 1)];
        }
    }
    b
}

/// Pixels that differ from Qt's by more than 3 a channel.
fn differ(qt: &Canvas, ours: &Canvas) -> usize {
    qt_window::diff(ours, qt, 3).0
}

/// Rows of `_|_|_|`: ink that reaches every edge of what is left of the text.
fn rows() -> String {
    vec!["_|".repeat(40); 12].join("\\n")
}

fn text_box(name: &str, body: &str) {
    let Some((qt, ours)) = both(name, body, (240, 140)) else {
        return;
    };
    assert_eq!(
        ink(&ours),
        ink(&qt),
        "{name}: the box of the text, ours and Qt's"
    );
}

fn area() -> String {
    format!(
        "TextArea {{ leftPadding: 16; topPadding: 12; rightPadding: 20; bottomPadding: 8; \
         {FONT}; text: \"{}\"; background: null }}",
        rows()
    )
}

/// QQuickTextArea::updatePaintNode: in a ScrollView the text is clipped to the Flickable less
/// the paddings of the field.
#[test]
fn the_text_of_a_text_area_in_a_scroll_view_is_clipped_by_its_paddings() {
    let body = format!(
        "ScrollView {{ x: 10; y: 10; width: 200; height: 100\n{} }}",
        area()
    );
    text_box("clip-start", &body);
}

/// The same once the view is scrolled: the clip stays where the view is.
#[test]
fn the_clip_of_a_text_area_stays_when_its_scroll_view_scrolls() {
    let body = format!(
        "ScrollView {{ x: 10; y: 10; width: 200; height: 100\n{}\n\
         Component.onCompleted: {{ contentItem.contentX = 30; contentItem.contentY = 40 }} }}",
        area()
    );
    text_box("clip-scrolled", &body);
}

/// The same clip of a TextArea that is in no Flickable: the field less its paddings.
#[test]
fn the_text_of_a_text_area_is_clipped_by_its_paddings() {
    let body = format!(
        "TextArea {{ x: 10; y: 10; width: 200; height: 100; leftPadding: 16; topPadding: 12; \
         rightPadding: 20; bottomPadding: 8; {FONT}; text: \"{}\"; background: null }}",
        rows()
    );
    text_box("clip-alone", &body);
}

fn bars(name: &str, body: &str) {
    let Some((qt, ours)) = both(name, body, (240, 140)) else {
        return;
    };
    let n = differ(&qt, &ours);
    assert!(
        ink(&qt)[2] > 0,
        "{name}: Qt draws no bar, the scene is wrong"
    );
    assert_eq!(n, 0, "{name}: pixels that differ from Qt's");
}

const CONTENT: &str = "Item { implicitWidth: 300; implicitHeight: 150 }\n\
    Component.onCompleted: { contentItem.contentX = 40; contentItem.contentY = 20 }";

/// Basic ScrollBar.qml: a rounded bar 6 wide in `palette.mid`, shown while the content does
/// not fit and the bar is always on,
#[test]
fn the_bars_of_a_scroll_view_are_drawn_when_always_on() {
    let body = format!(
        "ScrollView {{ x: 10; y: 10; width: 200; height: 100\n\
         ScrollBar.vertical.policy: ScrollBar.AlwaysOn\n\
         ScrollBar.horizontal.policy: ScrollBar.AlwaysOn\n{CONTENT} }}"
    );
    bars("bars-always-on", &body);
}

/// or active; Basic ScrollView.qml makes each bar active while the other one is.
#[test]
fn the_bars_of_a_scroll_view_are_drawn_when_active() {
    let body = format!(
        "ScrollView {{ x: 10; y: 10; width: 200; height: 100\n\
         ScrollBar.vertical.active: true\n{CONTENT} }}"
    );
    bars("bars-active", &body);
}

/// The horizontal bar MessageCodeBlock.qml gives its ScrollView, with its own contentItem.
#[test]
fn a_horizontal_bar_given_to_a_scroll_view_is_drawn() {
    bars(
        "bar-of-code-block",
        "ScrollView { id: v; x: 10; y: 10; width: 200; height: 61\n\
         contentWidth: k.width - 1; clip: true\n\
         ScrollBar.vertical.policy: ScrollBar.AlwaysOff\n\
         ScrollBar.horizontal: ScrollBar {\n\
             anchors.bottom: parent.bottom; anchors.left: parent.left; anchors.right: parent.right\n\
             padding: 5; policy: ScrollBar.AsNeeded\n\
             opacity: visualSize == 1 ? 0 : 1; visible: opacity > 0\n\
             contentItem: Rectangle { implicitHeight: 6; radius: 4; color: \"#808080\" }\n\
         }\n\
         Item { id: k; width: 570; height: 60 }\n\
         Component.onCompleted: contentItem.contentX = 120 }",
    );
}

/// The three ScrollViews of the shell, as in the shots: the input of AssistantChat.qml, the
/// editor of NotesContent.qml and the code of MessageCodeBlock.qml. The vertical bar is active,
/// as it is while the view scrolls. Text is not compared pixel by pixel; the box of everything
/// that is drawn is, so the code has a line of `_` that reaches both sides of the clip.
fn place(name: &str, body: &str) {
    let Some((qt, ours)) = both(name, body, (240, 140)) else {
        return;
    };
    println!("{name}: {} pixels differ from Qt's", differ(&qt, &ours));
    assert_eq!(
        ink(&ours),
        ink(&qt),
        "{name}: the box of what is drawn, ours and Qt's"
    );
}

fn field(more: &str) -> String {
    let lines = "one\\ntwo\\nthree\\nfour\\nfive\\nsix\\nseven\\neight\\nnine\\nten";
    format!(
        "TextArea {{ {more}; wrapMode: TextEdit.Wrap; {FONT}; text: \"{lines}\"; background: null }}"
    )
}

const ACTIVE: &str = "ScrollBar.vertical.policy: ScrollBar.AsNeeded\n\
    ScrollBar.vertical.active: true\nScrollBar.horizontal.active: false";

#[test]
fn the_input_of_the_assistant_chat_is_drawn_where_qt_draws_it() {
    let body = format!(
        "ScrollView {{ x: 10; y: 10; width: 220; height: 78; clip: true\n{ACTIVE}\n{}\n\
         Component.onCompleted: contentItem.contentY = 60 }}",
        field("anchors.fill: parent; padding: 10")
    );
    place("place-assistant-chat", &body);
}

#[test]
fn the_editor_of_the_notes_is_drawn_where_qt_draws_it() {
    let body = format!(
        "ScrollView {{ x: 10; y: 10; width: 220; height: 120; clip: true\n{ACTIVE}\n{}\n\
         Component.onCompleted: contentItem.contentY = 60 }}",
        field("anchors { left: parent.left; right: parent.right } padding: 24")
    );
    place("place-notes", &body);
}

#[test]
fn the_code_of_a_message_is_drawn_where_qt_draws_it() {
    place(
        "place-code-block",
        "ScrollView { x: 10; y: 10; width: 220; implicitHeight: t.implicitHeight + 1\n\
         contentWidth: t.width - 1; clip: true\n\
         ScrollBar.vertical.policy: ScrollBar.AlwaysOff\n\
         ScrollBar.horizontal: ScrollBar {\n\
             anchors.bottom: parent.bottom; anchors.left: parent.left; anchors.right: parent.right\n\
             padding: 5; policy: ScrollBar.AsNeeded\n\
             opacity: visualSize == 1 ? 0 : 1; visible: opacity > 0\n\
             contentItem: Rectangle { implicitHeight: 6; radius: 4; color: \"#808080\" }\n\
         }\n\
         TextArea { id: t; readOnly: true; font.family: \"DejaVu Sans Mono\"; font.pixelSize: 13\n\
             color: \"white\"; background: null\n\
             text: \"fn main() { println!(\\\"a rather long line of code that does not fit\\\"); }\\n________________________________________________________________________\\nlet y = 2;\" }\n\
         Component.onCompleted: contentItem.contentX = 60 }",
    );
}
