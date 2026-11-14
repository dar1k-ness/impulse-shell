//! A TextInput whose text is wider than the field scrolls it to keep the cursor in view
//! (QQuickTextInputPrivate::updateHorizontalScroll, `hscroll`), compared with Qt itself
//! (`qmltestrunner`, offscreen) on the fonts of the machine the test runs on. After every step
//! of one script (cursor set, Home / End / Left, a drag with the mouse past either edge, the
//! width, `autoScroll`, the alignment and the text changed) both print the cursor, the x of
//! `cursorRectangle`, of the first and of the last position, `positionAt` at the left edge,
//! the middle and the right edge, `contentWidth` and the selection.
//! Ours never scrolled: the text of a search field ran out of its right edge and the cursor
//! at its end with it.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::{input::Pointer, keys::Keyboard, scene};

const RUNNER: &str = "/usr/lib/qt6/bin/qmltestrunner";
const BTN_LEFT: u32 = 0x110;
const LATIN: &str = "The quick brown fox jumps over the lazy dog";
const HEBREW: &str = "שלום עולם, זה טקסט ארוך למדי שנשבר";

/// The fields: a name and the properties that differ from a 120 px wide TextInput.
const FIELDS: [(&str, &str, &str); 7] = [
    ("left", LATIN, ""),
    ("right", LATIN, "horizontalAlignment: TextInput.AlignRight"),
    (
        "center",
        LATIN,
        "horizontalAlignment: TextInput.AlignHCenter",
    ),
    ("hebrew", HEBREW, ""),
    (
        "hebrew-left",
        HEBREW,
        "horizontalAlignment: TextInput.AlignLeft",
    ),
    ("padded", LATIN, "leftPadding: 7; rightPadding: 11"),
    ("still", LATIN, "autoScroll: false"),
];

#[derive(Clone, Copy)]
enum Step {
    Cursor(usize),
    /// X11 keysym and Qt's name of the key.
    Key(u32, &'static str),
    Press(f64),
    Move(f64),
    Release(f64),
    Width(f64),
    AutoScroll(bool),
    /// Qt::AlignLeft 1, AlignRight 2, AlignHCenter 4.
    Align(u32),
    Text(&'static str),
}

const HOME: Step = Step::Key(0xff50, "Home");
const END: Step = Step::Key(0xff57, "End");
const LEFT: Step = Step::Key(0xff51, "Left");

/// Run on every field, which starts with the cursor at the end of its text.
const SCRIPT: [Step; 36] = [
    Step::Cursor(0),
    Step::Cursor(20),
    Step::Cursor(34),
    Step::Cursor(20),
    HOME,
    END,
    LEFT,
    LEFT,
    Step::Cursor(5),
    // A drag out of the right edge, then one out of the left edge.
    Step::Press(20.0),
    Step::Move(60.0),
    Step::Move(150.0),
    Step::Move(300.0),
    Step::Release(300.0),
    Step::Press(100.0),
    Step::Move(40.0),
    Step::Move(-50.0),
    Step::Move(-200.0),
    Step::Release(-200.0),
    Step::Width(200.0),
    Step::Width(80.0),
    Step::Width(600.0),
    Step::Width(120.0),
    END,
    Step::Width(60.0),
    Step::Width(120.0),
    Step::AutoScroll(false),
    Step::AutoScroll(true),
    Step::Align(2),
    HOME,
    Step::Align(4),
    END,
    Step::Align(1),
    Step::Text("short"),
    Step::Text(HEBREW),
    Step::Text(LATIN),
];

fn source(qt: bool) -> String {
    let mut s = String::from("import QtQuick\n");
    if qt {
        s += "import QtTest\n";
    }
    s += "Item {\n    id: root\n    width: 900; height: 300\n";
    s += "    function align(e, a) { e.horizontalAlignment = a }\n";
    for (i, (_, text, props)) in FIELDS.iter().enumerate() {
        s += &format!(
            "    property alias item{i}: e{i}\n    TextInput {{ id: e{i}; x: 250; y: {}; width: \
             120; font.family: \"DejaVu Sans\"; font.pixelSize: 16; text: \"{text}\"; {props} \
             }}\n",
            10 + 30 * i
        );
    }
    if qt {
        s += "    TestCase {\n        name: \"scroll\"; when: windowShown\n        function \
              row(e) {\n            const y = e.height / 2;\n            console.log(\"ROW\", \
              e.cursorPosition, e.cursorRectangle.x, e.positionToRectangle(0).x + \
              e.leftPadding, e.positionToRectangle(e.length).x + e.leftPadding, e.positionAt(0, \
              y), e.positionAt(e.width / 2, y), e.positionAt(e.width - 1, y), e.contentWidth, \
              e.selectionStart, e.selectionEnd);\n        }\n        function test_scroll() {\n";
        for i in 0..FIELDS.len() {
            s += &format!(
                "            {{\n                const e = e{i};\n                \
                 e.forceActiveFocus();\n                row(e);\n"
            );
            for step in SCRIPT {
                let line = match step {
                    Step::Cursor(p) => format!("e.cursorPosition = {p}"),
                    Step::Key(_, name) => format!("keyClick(Qt.Key_{name})"),
                    Step::Press(x) => format!("mousePress(e, {x}, e.height / 2)"),
                    Step::Move(x) => {
                        format!("mouseMove(e, {x}, e.height / 2, -1, Qt.LeftButton)")
                    }
                    Step::Release(x) => format!("mouseRelease(e, {x}, e.height / 2)"),
                    Step::Width(w) => format!("e.width = {w}"),
                    Step::AutoScroll(on) => format!("e.autoScroll = {on}"),
                    Step::Align(a) => format!("e.horizontalAlignment = {a}"),
                    Step::Text(t) => format!("e.text = \"{t}\""),
                };
                s += &format!("                {line}; row(e);\n");
            }
            s += "            }\n";
        }
        s += "        }\n    }\n";
    }
    s += "}\n";
    s
}

/// One line per field and step: cursorPosition, cursorRectangle.x, x of the first and of the
/// last position, positionAt at the left edge, the middle and the right edge, contentWidth,
/// selectionStart, selectionEnd.
type Row = Vec<f64>;

fn qt_rows() -> Vec<Row> {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("tst_scroll.qml"), source(true)).unwrap();
    let out = std::process::Command::new(RUNNER)
        .args(["-input", "tst_scroll.qml"])
        .current_dir(dir.path())
        .env("QT_QPA_PLATFORM", "offscreen")
        .env("QT_FORCE_STDERR_LOGGING", "1")
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&out.stdout) + String::from_utf8_lossy(&out.stderr);
    let rows: Vec<Row> = log
        .lines()
        .filter_map(|l| l.split_once("ROW ")?.1.into())
        .map(|l: &str| l.split_whitespace().map(|n| n.parse().unwrap()).collect())
        .collect();
    assert_eq!(
        rows.len(),
        FIELDS.len() * (SCRIPT.len() + 1),
        "qmltestrunner printed:\n{log}"
    );
    rows
}

struct Ours {
    rt: Runtime,
    fonts: std::rc::Rc<std::cell::RefCell<scene::Fonts>>,
    root: usize,
    ptr: Pointer,
    kb: Keyboard,
}

impl Ours {
    fn laid(&mut self) {
        self.rt.settle();
        scene::layout_window(
            &mut self.rt,
            &mut self.fonts.borrow_mut(),
            self.root,
            [900.0, 300.0],
        );
        self.rt.settle();
    }

    fn num(&mut self, e: usize, name: &str) -> f64 {
        let v = self.rt.world.borrow_mut().read(e, name);
        v.map_or(f64::NAN, |v| v.to_number())
    }

    fn row(&mut self, e: usize) -> Row {
        self.laid();
        let cursor = self.num(e, "cursorPosition");
        let (width, height) = (self.num(e, "width"), self.num(e, "height"));
        let content = self.num(e, "contentWidth");
        let (from, to) = (self.num(e, "selectionStart"), self.num(e, "selectionEnd"));
        let mut w = self.rt.world.borrow_mut();
        let mut fonts = self.fonts.borrow_mut();
        let lines = scene::edit_geometry(&mut w, &mut fonts, e);
        let xs = &lines[0].3;
        let mut at = |x: f64| {
            scene::edit_position_at(&mut w, &mut fonts, e, x, height / 2.0)
                .map_or(f64::NAN, |p| p as f64)
        };
        vec![
            cursor,
            xs[cursor as usize],
            xs[0],
            xs[xs.len() - 1],
            at(0.0),
            at(width / 2.0),
            at(width - 1.0),
            content,
            from,
            to,
        ]
    }

    fn step(&mut self, e: usize, step: Step) {
        let set = |rt: &mut Runtime, name: &str, v: Value| rt.set(e, name, v);
        let y = self.num(e, "y") + self.num(e, "height") / 2.0;
        let x0 = self.num(e, "x");
        match step {
            Step::Cursor(p) => set(&mut self.rt, "cursorPosition", Value::Num(p as f64)),
            Step::Key(sym, _) => {
                self.kb.key(&mut self.rt, sym, None, true, false);
                self.kb.key(&mut self.rt, sym, None, false, false);
            }
            Step::Press(x) => {
                self.ptr.motion(&mut self.rt, self.root, x0 + x, y);
                self.ptr.press(&mut self.rt, BTN_LEFT);
            }
            Step::Move(x) => self.ptr.motion(&mut self.rt, self.root, x0 + x, y),
            Step::Release(x) => {
                self.ptr.motion(&mut self.rt, self.root, x0 + x, y);
                self.ptr.release(&mut self.rt, BTN_LEFT);
            }
            Step::Width(w) => set(&mut self.rt, "width", Value::Num(w)),
            Step::AutoScroll(on) => set(&mut self.rt, "autoScroll", Value::Bool(on)),
            // Written by QML, as in Qt's run: a `horizontalAlignment` somebody set no longer
            // follows the text.
            Step::Align(a) => {
                let args = vec![Value::Item(e), Value::Num(a as f64)];
                self.rt
                    .call_method(self.root, "align", args)
                    .unwrap()
                    .unwrap();
            }
            Step::Text(t) => set(&mut self.rt, "text", Value::str(t)),
        }
    }
}

fn our_rows() -> Vec<Row> {
    let mut rt = Runtime::new(None);
    let fonts = std::rc::Rc::new(std::cell::RefCell::new(scene::Fonts::default()));
    {
        let fonts = fonts.clone();
        rt.world.borrow_mut().text_measure = Some(Box::new(move |w, i| {
            if let Ok(mut f) = fonts.try_borrow_mut() {
                scene::measure_text(w, &mut f, i);
            }
        }));
    }
    let root = rt.load_str(&source(false)).unwrap();
    let mut ours = Ours {
        rt,
        fonts,
        root,
        ptr: Pointer::default(),
        kb: Keyboard::default(),
    };
    ours.kb.win = Some(root);
    ours.laid();
    ours.ptr.enter(&mut ours.rt, root, 5.0, 5.0);
    let mut rows = Vec::new();
    for i in 0..FIELDS.len() {
        let Some(Value::Item(e)) = ours.rt.world.borrow_mut().read(root, &format!("item{i}"))
        else {
            panic!("item{i} is not an item");
        };
        ours.rt.world.borrow_mut().force_active_focus(e);
        ours.kb.sync(&mut ours.rt);
        rows.push(ours.row(e));
        for step in SCRIPT {
            ours.step(e, step);
            rows.push(ours.row(e));
        }
    }
    rows
}

fn same(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1.0 / 64.0)
}

fn step_name(k: usize) -> String {
    match k.checked_sub(1).map(|k| SCRIPT[k]) {
        None => "focus".into(),
        Some(Step::Cursor(p)) => format!("cursorPosition = {p}"),
        Some(Step::Key(_, name)) => name.into(),
        Some(Step::Press(x)) => format!("press at {x}"),
        Some(Step::Move(x)) => format!("move to {x}"),
        Some(Step::Release(x)) => format!("release at {x}"),
        Some(Step::Width(w)) => format!("width = {w}"),
        Some(Step::AutoScroll(on)) => format!("autoScroll = {on}"),
        Some(Step::Align(a)) => format!("horizontalAlignment = {a}"),
        Some(Step::Text(t)) => format!("text = {t:?}"),
    }
}

#[test]
fn text_input_scrolls_as_qt() {
    if !std::path::Path::new(RUNNER).exists() {
        testenv::equiv_missing(format_args!("{RUNNER} (Qt 6 with QtTest)"));
        return;
    }
    let qt = qt_rows();
    let ours = our_rows();
    let per = SCRIPT.len() + 1;
    let wrong: Vec<String> = qt
        .iter()
        .zip(&ours)
        .enumerate()
        .filter(|(_, (qt, ours))| !same(qt, ours))
        .map(|(n, (qt, ours))| {
            format!(
                "{} after {}\n    ours {ours:?}\n    Qt   {qt:?}",
                FIELDS[n / per].0,
                step_name(n % per)
            )
        })
        .collect();
    assert!(
        wrong.is_empty(),
        "{} of {} steps differ from Qt (cursorPosition, cursorRectangle.x, x of the first and \
         the last position, positionAt at the left edge, the middle and the right edge, \
         contentWidth, selectionStart, selectionEnd):\n{}",
        wrong.len(),
        qt.len(),
        wrong.join("\n")
    );
}
