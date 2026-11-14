//! The selection of the four editors, compared with Qt itself (`qmltestrunner`, offscreen,
//! software renderer, Basic style): what is selected, in which colours, and what of it is drawn,
//! with and without active focus.
//!
//! In Qt a selection does not need focus: `selectAll()` and `select()` from
//! `Component.onCompleted` or from a script leave a field that never had focus with its text
//! drawn in `selectedTextColor` over `selectionColor`. Those colours, when nothing sets them, are
//! #ffffff over #000080 in a TextInput and a TextEdit (QQuickTextInputPrivate,
//! QQuickTextEditPrivate) and `palette.highlightedText` over `palette.highlight` in a TextField
//! and a TextArea (TextField.qml and TextArea.qml of the Basic style), so they follow the palette
//! of what the field is in and its group (`enabled: false`).
//!
//! In the Material style the two are `Material.accentColor` and
//! `Material.primaryHighlightedTextColor` (its TextField.qml and TextArea.qml): the second test
//! reads them under the accents, themes and foregrounds below.
//!
//! The text of each field is black squares (U+25A0) and spaces, so every colour drawn covers
//! whole pixels and the selection shows above, below and between the glyphs: per field the test
//! compares the properties and the box of each colour on the picture. The fields set
//! `NativeRendering`: Qt's software renderer draws every text so, while a text that sets no
//! render type is one of distance fields here, as in Qt's OpenGL scene graph.

#[path = "support/qt_gl.rs"]
mod qt_gl;

use qml::runtime::Runtime;
use qml::value::Value;
use qsg::raster::Canvas;
use shell::{
    keys::Keyboard,
    scene::{self, Target},
};

const RUNNER: &str = "/usr/lib/qt6/bin/qmltestrunner";
const KINDS: [&str; 4] = ["TextInput", "TextField", "TextEdit", "TextArea"];
const WIDTH: usize = 200;
const BAND: usize = 36;
const PAPER: &str = "#c0c0c0";
const INK: &str = "#804000";
/// A colour counts as drawn in a field when it covers at least this many pixels.
const SOLID: usize = 12;

/// The fields of each kind: a name and what differs from a field with the text.
const VARIANTS: [(&str, &str); 7] = [
    ("all", "Component.onCompleted: selectAll()"),
    ("part", "Component.onCompleted: select(2, 7)"),
    (
        "part, colours set",
        "selectionColor: \"#00aa00\"; selectedTextColor: \"#ff0000\"; Component.onCompleted: \
         select(2, 7)",
    ),
    (
        "all, colours set, persistentSelection",
        "selectionColor: \"#00aa00\"; selectedTextColor: \"#ff0000\"; persistentSelection: true; \
         Component.onCompleted: selectAll()",
    ),
    (
        "part, disabled",
        "enabled: false; Component.onCompleted: select(2, 7)",
    ),
    ("focused later", "persistentSelection: false"),
    (
        "focused later, persistentSelection",
        "persistentSelection: true",
    ),
];

fn fields() -> Vec<(&'static str, &'static str, &'static str)> {
    KINDS
        .iter()
        .flat_map(|k| {
            VARIANTS
                .iter()
                .map(move |(name, props)| (*k, *name, *props))
        })
        .collect()
}

fn height() -> usize {
    fields().len() * BAND
}

/// The steps after creation: a name and the script. `f<n>` is field n.
fn stages() -> Vec<(String, String)> {
    let per = VARIANTS.len();
    let mut s = vec![("as created".to_string(), String::new())];
    for (k, kind) in KINDS.iter().enumerate() {
        for v in [5, 6] {
            let f = format!("f{}", k * per + v);
            let name = format!("{kind} ({})", VARIANTS[v].0);
            s.push((
                format!("{name}: focus and select(2, 7)"),
                format!("{f}.forceActiveFocus(); {f}.select(2, 7);"),
            ));
            s.push((
                format!("{name}: focus moves to another item"),
                "sink.forceActiveFocus();".into(),
            ));
            s.push((
                format!("{name}: selectAll() without focus"),
                format!("{f}.selectAll();"),
            ));
        }
    }
    let each = |v: usize, js: &dyn Fn(String) -> String| -> String {
        (0..KINDS.len())
            .map(|k| js(format!("f{}", k * per + v)))
            .collect()
    };
    s.push((
        "selectionColor and selectedTextColor written".into(),
        each(1, &|f| {
            format!("{f}.selectionColor = \"#aa00aa\"; {f}.selectedTextColor = \"#00ffff\"; ")
        }),
    ));
    let reset = |f: String| {
        format!(
            "try {{ {f}.selectionColor = undefined; }} catch (e) {{ root.thrown += 1; }} try {{ \
             {f}.selectedTextColor = undefined; }} catch (e) {{ root.thrown += 1; }} "
        )
    };
    s.push((
        "both colours set to undefined".into(),
        each(1, &reset) + &each(2, &reset),
    ));
    s.push((
        "palette.highlight and palette.highlightedText of the parent".into(),
        "page.palette.highlight = \"#ff00ff\"; page.palette.highlightedText = \"#ffff00\";".into(),
    ));
    s.push(("the parent disabled".into(), "page.enabled = false;".into()));
    s
}

/// Who runs the page.
enum By<'a> {
    /// We do: the test calls `stage` and `rows`.
    Us,
    /// `qmltestrunner`, which saves the picture of each stage in this directory.
    Runner(&'a str),
    /// tools/oracle/steps_oracle.py: the page emits `shoot()` after each stage.
    Steps,
}

/// The page; with `native` its fields set `NativeRendering`.
fn source(native: bool, by: By) -> String {
    let mut s = String::from("import QtQuick\nimport QtQuick.Controls\n");
    if matches!(by, By::Runner(_)) {
        s += "import QtTest\n";
    }
    let fields = fields();
    s += &format!(
        "Item {{\n    id: root\n    width: {WIDTH}; height: {}\n    property int thrown: 0\n    \
         Rectangle {{\n        id: page\n        width: {WIDTH}; height: {}\n        color: \
         \"{PAPER}\"\n        Item {{ id: sink }}\n",
        height(),
        height()
    );
    for (n, (kind, _, props)) in fields.iter().enumerate() {
        let background = if matches!(*kind, "TextField" | "TextArea") {
            "background: null; "
        } else {
            ""
        };
        let text = ['\u{25a0}'; 5].map(String::from).join(" ");
        let render = match (native, *kind) {
            (false, _) => "",
            (true, "TextEdit" | "TextArea") => "renderType: TextEdit.NativeRendering; ",
            (true, _) => "renderType: TextInput.NativeRendering; ",
        };
        s += &format!(
            "        {kind} {{ id: f{n}; x: 20; y: {}; width: 150; color: \"{INK}\"; \
             {background}{render}font.family: \"DejaVu Sans\"; \
             font.pixelSize: 14; text: \"{text}\"; {props}\n            cursorDelegate: Item {{}} \
             }}\n",
            n * BAND + 4
        );
    }
    s += "    }\n";
    let ids: Vec<String> = (0..fields.len()).map(|n| format!("f{n}")).collect();
    s += &format!(
        "    property var fields: [{}]\n    function rows() {{\n        let s = \"\";\n        \
         for (const f of fields)\n            s += [f.selectionStart, f.selectionEnd, \"\" + \
         f.selectionColor, \"\" + f.selectedTextColor, f.activeFocus].join(\" \") + \"\\n\";\n        \
         return s + \"thrown \" + thrown + \"\\n\";\n    }}\n    function stage(n) {{\n        \
         switch (n) {{\n",
        ids.join(", ")
    );
    for (n, (_, js)) in stages().iter().enumerate() {
        s += &format!("        case {n}: {js} break;\n");
    }
    s += "        }\n    }\n";
    let count = stages().len();
    if let By::Runner(shots) = by {
        s += &format!(
            "    TestCase {{\n        name: \"selection\"; when: windowShown\n        function \
             test_selection() {{\n            for (let n = 0; n < {count}; n++) {{\n                \
             root.stage(n);\n                waitForRendering(root);\n                for (const \
             l of root.rows().split(\"\\n\"))\n                    if (l) console.log(\"ROW \" + \
             n + \" \" + l);\n                grabImage(page).save(\"{shots}/\" + n + \
             \".ppm\");\n                console.log(\"SAVED true\");\n            }}\n        \
             }}\n    }}\n"
        );
    }
    if matches!(by, By::Steps) {
        // Once the window is on screen and active: a stage, its rows, its picture, and so on.
        s += &format!(
            "    signal shoot()\n    signal finished()\n    property int at: -1\n    \
             Timer {{\n        id: next; interval: 100; repeat: true\n        onTriggered: {{\n            \
             if (root.at < 0 && !root.Window.window.active)\n                return;\n            \
             root.at += 1;\n            if (root.at == {count}) {{\n                next.stop();\n                \
             root.finished();\n                return;\n            }}\n            \
             root.stage(root.at);\n            for (const l of root.rows().split(\"\\n\"))\n                \
             if (l) console.log(\"ROW \" + root.at + \" \" + l);\n            root.shoot();\n        \
             }}\n    }}\n    Connections {{\n        target: root.Window.window\n        \
             function onFrameSwapped() {{ next.start(); }}\n    }}\n"
        );
    }
    s + "}\n"
}

/// Per field: the first and last column and the first and last row of each colour, the paper
/// aside, that covers `SOLID` pixels or more; rows from the top of the field's band.
type Solids = Vec<Vec<(String, [usize; 4])>>;

fn solids(px: impl Fn(usize, usize) -> [u8; 3]) -> Solids {
    (0..fields().len())
        .map(|n| {
            let mut seen: std::collections::BTreeMap<[u8; 3], (usize, [usize; 4])> =
                Default::default();
            for y in 0..BAND {
                for x in 0..WIDTH {
                    let (count, b) = seen.entry(px(x, n * BAND + y)).or_insert((0, [x, x, y, y]));
                    *count += 1;
                    *b = [b[0].min(x), b[1].max(x), b[2].min(y), b[3].max(y)];
                }
            }
            seen.into_iter()
                .map(|(c, (count, b))| (format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]), count, b))
                .filter(|(c, count, _)| c != PAPER && *count >= SOLID)
                .map(|(c, _, b)| (c, b))
                .collect()
        })
        .collect()
}

/// Per stage: the line of each field (selectionStart, selectionEnd, selectionColor,
/// selectedTextColor, activeFocus), then how many writes of `undefined` threw; and the picture.
type Stage = (Vec<String>, Solids);

fn qt() -> Vec<Stage> {
    let dir = tempfile::tempdir().unwrap();
    let shots = dir.path().display().to_string();
    std::fs::write(
        dir.path().join("tst_selection.qml"),
        source(true, By::Runner(&shots)),
    )
    .unwrap();
    let out = std::process::Command::new(RUNNER)
        .args(["-input", "tst_selection.qml"])
        .current_dir(dir.path())
        .env("QT_QPA_PLATFORM", "offscreen")
        .env("QT_QUICK_BACKEND", "software")
        .env("QT_QUICK_CONTROLS_STYLE", "Basic")
        .env("QT_FORCE_STDERR_LOGGING", "1")
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&out.stdout) + String::from_utf8_lossy(&out.stderr);
    let count = stages().len();
    assert_eq!(
        log.matches("SAVED true").count(),
        count,
        "qmltestrunner printed:\n{log}"
    );
    (0..count)
        .map(|n| {
            let rows = rows_of(&log, n);
            // Binary PPM: "P6\n<width> <height>\n255\n", then RGB rows.
            let ppm = std::fs::read(dir.path().join(format!("{n}.ppm"))).unwrap();
            let header = format!("P6\n{WIDTH} {}\n255\n", height());
            assert!(ppm.starts_with(header.as_bytes()), "not a {header:?} image");
            let px = &ppm[header.len()..];
            (
                rows,
                solids(|x, y| px[(y * WIDTH + x) * 3..][..3].try_into().unwrap()),
            )
        })
        .collect()
}

/// The lines `ROW <stage> ...` of what Qt printed, per stage.
fn rows_of(log: &str, n: usize) -> Vec<String> {
    let tag = format!("ROW {n} ");
    log.lines()
        .filter_map(|l| Some(l.split_once(&tag)?.1.to_string()))
        .collect()
}

fn ours() -> Vec<Stage> {
    let solid = |(rows, c): (Vec<String>, Canvas)| {
        let px = |x: usize, y: usize| {
            let p = c.px[y * c.w + x];
            [p[0], p[1], p[2]]
        };
        (rows, solids(px))
    };
    our_frames(true, false).into_iter().map(solid).collect()
}

/// Per stage: the rows and the picture of the page under our engine, on the CPU or on the GPU
/// (llvmpipe, as Qt's).
fn our_frames(native: bool, gpu: bool) -> Vec<(Vec<String>, Canvas)> {
    let mut rt = Runtime::new(None);
    let root = rt.load_str(&source(native, By::Us)).unwrap();
    let mut fonts = scene::Fonts::default();
    let mut kb = Keyboard::default();
    kb.win = Some(root);
    let size = [WIDTH as f64, height() as f64];
    let mut screen = gpu.then(|| {
        let (g, _) =
            shell::gpu::Gpu::headless("surfaceless").expect("surfaceless EGL/GLES2 context (Mesa)");
        let s = g.pbuffer(WIDTH as i32, height() as i32).expect("pbuffer");
        (g, s)
    });
    (0..stages().len())
        .map(|n| {
            rt.call_method(root, "stage", vec![Value::Num(n as f64)])
                .unwrap()
                .unwrap();
            for _ in 0..3 {
                kb.sync(&mut rt);
                rt.settle();
                scene::layout_window(&mut rt, &mut fonts, root, size);
            }
            let rows = match rt.call_method(root, "rows", Vec::new()).unwrap().unwrap() {
                Value::Str(s) => s.lines().map(str::to_string).collect(),
                v => panic!("rows() gave {v:?}"),
            };
            // The caret is not compared: Qt's is an empty cursorDelegate, ours is caught in the
            // half of its blink in which it is hidden.
            let hidden = std::time::Duration::from_millis(
                qml::colorscheme::cursor_flash_time() as u64 * 3 / 4,
            );
            rt.world.borrow_mut().caret_reset = std::time::Instant::now().checked_sub(hidden);
            let Some((g, s)) = &mut screen else {
                let mut c = Canvas::new(WIDTH, height(), [0, 0, 0, 0]);
                scene::paint_window(&mut rt, &mut fonts, root, &mut Target::Cpu(&mut c));
                return (rows, c);
            };
            // As the shell draws a window: the recorded draws of what was touched are dropped.
            {
                let mut w = rt.world.borrow_mut();
                let (touched, moved) =
                    (std::mem::take(&mut w.touched), std::mem::take(&mut w.moved));
                scene::invalidate_moved(&w, &moved);
                scene::invalidate_layers(&w, &touched);
            }
            let mut fr = shell::gpu::Frame::new(WIDTH, height());
            scene::paint_window(&mut rt, &mut fonts, root, &mut Target::Gpu(&mut fr));
            s.reswap();
            g.render(s, &mut fr, || {});
            (rows, g.read_surface(s))
        })
        .collect()
}

/// Whether two fields are drawn alike. Qt's software renderer fills every pixel a rectangle
/// touches (`QSGSoftwareInternalRectangleNode::setRect` takes `toAlignedRect()`), we fill
/// the pixels whose centres are in it, as a GPU does: a box of Qt may begin a column earlier
/// and end a column later, and a colour that is one column wide, which is what such a column
/// leaves of a glyph, is not compared.
fn same_boxes(qt: &[(String, [usize; 4])], ours: &[(String, [usize; 4])]) -> bool {
    let wide = |v: &[(String, [usize; 4])]| -> Vec<(String, [usize; 4])> {
        v.iter().filter(|(_, b)| b[1] > b[0]).cloned().collect()
    };
    let (qt, ours) = (wide(qt), wide(ours));
    qt.len() == ours.len()
        && qt.iter().zip(&ours).all(|((qc, q), (oc, o))| {
            qc == oc
                && (q[0] == o[0] || q[0] + 1 == o[0])
                && (q[1] == o[1] || q[1] == o[1] + 1)
                && q[2..] == o[2..]
        })
}

#[test]
fn the_selection_of_an_editor_is_drawn_as_in_qt_with_and_without_focus() {
    if !std::path::Path::new(RUNNER).exists() {
        testenv::equiv_missing(format_args!("{RUNNER} (Qt 6 with QtTest)"));
        return;
    }
    let (qt, ours) = (qt(), ours());
    let fields = fields();
    let stages = stages();
    let mut report = String::new();
    let (mut differ, mut total, mut read, mut drawn) = (0, 0, 0, 0);
    for (n, (name, _)) in stages.iter().enumerate() {
        let mut lines = String::new();
        for f in 0..=fields.len() {
            total += 1;
            let what = match fields.get(f) {
                Some((kind, variant, _)) => format!("{kind} ({variant})"),
                None => "writes of undefined that threw".into(),
            };
            let (q, o) = (qt[n].0.get(f), ours[n].0.get(f));
            let none = Vec::new();
            let (qs, os) = (
                qt[n].1.get(f).unwrap_or(&none),
                ours[n].1.get(f).unwrap_or(&none),
            );
            let same = same_boxes(qs, os);
            if q == o && same {
                continue;
            }
            differ += 1;
            lines += &format!("    {what}\n");
            if q != o {
                read += 1;
                lines += &format!("        read: Qt {q:?}, ours {o:?}\n");
            }
            if !same {
                drawn += 1;
                let only = |a: &Vec<(String, [usize; 4])>, b: &Vec<(String, [usize; 4])>| {
                    a.iter()
                        .filter(|e| !b.contains(e))
                        .map(|(c, [l, r, t, d])| format!("{c} x {l}..{r} y {t}..{d}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                lines += &format!(
                    "        drawn only by Qt: {}\n        drawn only by us: {}\n",
                    only(qs, os),
                    only(os, qs)
                );
            }
        }
        if !lines.is_empty() {
            report += &format!("stage {n}, {name}:\n{lines}");
        }
    }
    assert!(
        differ == 0,
        "{differ} of {total} fields and stages differ from Qt: {read} in what is read \
         (selectionStart selectionEnd selectionColor selectedTextColor activeFocus), {drawn} in \
         what is drawn (colour, its first and last column, its first and last row):\n{report}"
    );
}

/// With `RI_DUMP_DIR` set, both pictures of a stage as PPM files there, to look at a difference.
fn dump(stage: usize, path: &str, ours: &Canvas, qt: &Canvas) {
    let Some(dir) = std::env::var_os("RI_DUMP_DIR") else {
        return;
    };
    for (side, c) in [(path, ours), ("Qt", qt)] {
        let mut out = format!("P6 {} {} 255\n", c.w, c.h).into_bytes();
        out.extend(c.px.iter().flat_map(|p| [p[0], p[1], p[2]]));
        let f = std::path::Path::new(&dir).join(format!("selection-{stage}-{side}.ppm"));
        std::fs::write(f, out).unwrap();
    }
}

/// The same fields and stages with no render type, which is what the shell's QML has: such a
/// text is one of distance fields, in Qt's OpenGL scene graph and here. What is read and every
/// pixel, ours on the CPU and on the GPU, against Qt Quick through OpenGL (support/qt_gl.rs).
#[test]
fn the_selection_of_an_editor_of_distance_fields_is_qts_opengl_picture() {
    if !qt_gl::available() {
        return;
    }
    let (log, shots) = qt_gl::steps(&source(false, By::Steps));
    let stages = stages();
    assert_eq!(shots.len(), stages.len(), "Qt printed:\n{log}");
    let fields = fields();
    let mut report = String::new();
    let (mut read, mut drawn) = (0, 0);
    for (path, gpu) in [("CPU", false), ("GPU", true)] {
        // A thread for each path: what a text node was last built from is kept by thread.
        let frames = std::thread::spawn(move || our_frames(false, gpu));
        for (n, (rows, ours)) in frames.join().unwrap().into_iter().enumerate() {
            let qt = &shots[n];
            assert_eq!((qt.w, qt.h), (ours.w, ours.h), "the size of the page");
            dump(n, path, &ours, qt);
            let name = &stages[n].0;
            let qt_rows = rows_of(&log, n);
            if !gpu && qt_rows != rows {
                for f in 0..=fields.len() {
                    let (q, o) = (qt_rows.get(f), rows.get(f));
                    if q != o {
                        read += 1;
                        let what = fields
                            .get(f)
                            .map_or("thrown".into(), |(k, v, _)| format!("{k} ({v})"));
                        report += &format!("stage {n}, {name}: {what} reads {o:?}, in Qt {q:?}\n");
                    }
                }
            }
            for (f, (kind, variant, _)) in fields.iter().enumerate() {
                let (mut count, mut most, mut at) = (0, 0, (0, 0));
                for y in f * BAND..(f + 1) * BAND {
                    for x in 0..WIDTH {
                        let (a, b) = (qt.px[y * WIDTH + x], ours.px[y * WIDTH + x]);
                        let d = (0..4).map(|k| a[k].abs_diff(b[k])).max().unwrap_or(0);
                        count += (d > 0) as usize;
                        if d > most {
                            (most, at) = (d, (x, y - f * BAND));
                        }
                    }
                }
                if count > 0 {
                    drawn += 1;
                    let (a, b) = (
                        qt.px[(f * BAND + at.1) * WIDTH + at.0],
                        ours.px[(f * BAND + at.1) * WIDTH + at.0],
                    );
                    report += &format!(
                        "stage {n}, {name}, {path}: {kind} ({variant}): {count} pixels differ, \
                         by up to {most} at {at:?} (ours {b:?}, Qt {a:?})\n"
                    );
                }
            }
        }
    }
    assert!(
        read + drawn == 0,
        "{read} fields and stages differ from Qt in what is read, {drawn} fields, stages and \
         paths in a pixel:\n{report}"
    );
}

/// Fields of the Material style: what each reads for its two colours, and the first again after
/// its accent is written.
const MATERIAL: &str = r##"import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material
Item {
    id: root
    property string result
    TextField { id: f0; Material.containerStyle: Material.Outlined }
    TextField { id: f1; Material.containerStyle: Material.Filled; Material.accent: "#00aa00" }
    TextField { id: f2; Material.containerStyle: Material.Outlined; Material.accent: Material.Teal }
    TextField { id: f3; Material.containerStyle: Material.Outlined; Material.theme: Material.Dark }
    TextField {
        id: f4
        Material.containerStyle: Material.Outlined
        Material.theme: Material.Dark; Material.accent: Material.Teal
    }
    TextField { id: f5; Material.containerStyle: Material.Outlined; Material.foreground: "#112233" }
    TextField {
        id: f6
        Material.containerStyle: Material.Outlined
        Material.foreground: "#112233"; Material.theme: Material.Dark
    }
    TextField { id: f7; Material.containerStyle: Material.Outlined; enabled: false }
    TextField {
        id: f8
        Material.containerStyle: Material.Outlined
        selectionColor: "#aa00aa"; selectedTextColor: "#00ffff"
    }
    TextArea { id: a0; Material.containerStyle: Material.Outlined }
    TextArea { id: a1; Material.containerStyle: Material.Filled; Material.accent: "#00aa00" }
    TextArea { id: a2; Material.containerStyle: Material.Outlined; Material.theme: Material.Dark }
    TextArea { id: a3; Material.containerStyle: Material.Outlined; Material.foreground: "#112233" }
    Item {
        Material.accent: Material.Indigo; Material.theme: Material.Dark
        TextField { id: p0; Material.containerStyle: Material.Outlined }
    }
    Component.onCompleted: {
        const out = []
        for (const f of [f0, f1, f2, f3, f4, f5, f6, f7, f8, a0, a1, a2, a3, p0])
            out.push(f.selectionColor + " " + f.selectedTextColor)
        f0.Material.accent = "#123456"
        out.push(f0.selectionColor + " " + f0.selectedTextColor)
        result = out.join("\n")
        //QT console.log("RESULT " + result.replace(/\n/g, "|")); Qt.quit()
    }
}
"##;

#[test]
fn a_material_field_selects_in_its_accent_as_in_qt() {
    if !testenv::require("qml6") {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("qt.qml");
    std::fs::write(&file, MATERIAL.replace("//QT ", "")).unwrap();
    let out = std::process::Command::new("qml6")
        .arg(&file)
        .env("QT_QPA_PLATFORM", "offscreen")
        .env("QT_QUICK_CONTROLS_STYLE", "Basic")
        .env("QT_FORCE_STDERR_LOGGING", "1")
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&out.stderr);
    let qt = (log.lines())
        .find_map(|l| Some(l.split_once("RESULT ")?.1.trim().replace('|', "\n")))
        .unwrap_or_else(|| panic!("qml6 printed:\n{log}"));
    // Imported QML module types resolve for a file under a config root (not `load_str`).
    std::fs::write(dir.path().join("shell.qml"), MATERIAL).unwrap();
    let mut rt = Runtime::new(Some(dir.path().to_path_buf()));
    let root = rt.load_file(&dir.path().join("shell.qml")).unwrap();
    rt.settle();
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    assert_eq!(rt.get(root, "result").to_js_string(), qt);
    // Qt's answer is the one the style's files give, not that of a style that has no accent.
    assert!(
        qt.starts_with("#e91e63 #ffffff\n#00aa00 #ffffff\n#009688 #ffffff\n#f48fb1"),
        "{qt}"
    );
}
