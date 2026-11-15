//! The code of an assistant chat's code block is coloured as the original on Quickshell colours it. The original's
//! MessageCodeBlock.qml puts KDE's `SyntaxHighlighter` (org.kde.syntaxhighlighting) on the
//! block's text with the definition `Repository.definitionForName(language)` and the theme
//! "Monokai" or "ayu Light"; the native shell colours it with [`qml::syntax::highlight`].
//!
//! Each text of tests/code_colours/texts (named by the language the chat gives) is drawn twice
//! by the real `qs`, in the same font: once under KDE's highlighter, once as rich text made of
//! the native spans. The two pictures must be equal pixel for pixel; a character whose box
//! differs is counted. No tolerance.
//!
//! What the native shell draws with those spans (a background of a word, a strike-out) is
//! compared with `qs` pixel for pixel in `a_coloured_block_is_drawn_as_quickshell_draws_it`.
//! The colours come from another thread after the block is on screen;
//! `a_block_takes_its_colours_when_they_arrive` draws the block as the shell does until they
//! are in, and `a_block_keeps_its_size_when_its_colours_arrive` holds its size over that.

use qsg::raster::Canvas;
use shell::scene::{self, Target};
use std::path::Path;
use std::process::{Command, Stdio};

const THEMES: [&str; 2] = ["Monokai", "ayu Light"];
/// `font.letterSpacing` of the fixture: a character's box starts half of it before the
/// character.
const SPACING: i64 = 12;

/// (language, text) of every file of tests/code_colours/texts, by name.
fn texts() -> Vec<(String, String)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/code_colours/texts");
    let mut out: Vec<(String, String)> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| {
            let p = e.unwrap().path();
            let lang = p.file_stem().unwrap().to_str().unwrap().to_string();
            let text = std::fs::read_to_string(&p).unwrap();
            (lang, text.trim_end_matches('\n').to_string())
        })
        .collect();
    out.sort();
    out
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// `text` as the rich text that draws what `spans` say: a paragraph for each line, as the
/// plain text of a TextEdit is a block for each line.
fn rich(text: &str, spans: &[qml::syntax::Span]) -> String {
    let mut out = String::new();
    let mut from = 0;
    for line in text.split('\n') {
        let to = from + line.len();
        if line.is_empty() {
            // As QTextDocument::toHtml writes an empty block: an empty <p> is no line at all.
            out += r#"<p style="-qt-paragraph-type:empty;margin:0"><br /></p>"#;
            from = to + 1;
            continue;
        }
        out += r#"<p style="margin:0;white-space:pre-wrap">"#;
        let mut at = from;
        for s in spans.iter().filter(|s| s.start < to && s.end > from) {
            let (start, end) = (s.start.max(from), s.end.min(to));
            out += &escape(&text[at..start]);
            let [r, g, b, _] = s.color;
            out += &format!(r#"<span style="color:#{r:02x}{g:02x}{b:02x}"#);
            if s.bold {
                out += ";font-weight:bold";
            }
            if s.italic {
                out += ";font-style:italic";
            }
            if let Some([r, g, b, _]) = s.background {
                out += &format!(";background-color:#{r:02x}{g:02x}{b:02x}");
            }
            match (s.underline, s.strike) {
                (true, true) => out += ";text-decoration:underline line-through",
                (true, false) => out += ";text-decoration:underline",
                (false, true) => out += ";text-decoration:line-through",
                (false, false) => {}
            }
            out += r#"">"#;
            out += &escape(&text[start..end]);
            out += "</span>";
            at = end;
        }
        out += &escape(&text[at..to]);
        out += "</p>";
        from = to + 1;
    }
    out
}

struct Drawn {
    /// The definition's name, as the original's label shows it.
    name: String,
    /// x, y and height of the caret before each UTF-16 unit and after the last.
    rects: Vec<(i64, i64, i64)>,
    image: image::RgbaImage,
}

/// What `qs` draws for each case.
fn drawn(cases: &serde_json::Value) -> Vec<Drawn> {
    let dir = tempfile::tempdir().unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/code_colours/shell.qml");
    std::fs::copy(fixture, dir.path().join("shell.qml")).unwrap();
    let cases_file = dir.path().join("cases.json");
    std::fs::write(&cases_file, cases.to_string()).unwrap();
    let out = dir.path().join("out");
    std::fs::create_dir(&out).unwrap();
    let mut qs = Command::new("qs")
        .arg("-p")
        .arg(dir.path())
        .env("CASES", &cases_file)
        .env("OUT", &out)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let log = testenv::Log::capture(qs.stdout.take().unwrap());
    let done = log.wait(|l| l.contains("PROBE DONE"));
    let _ = qs.kill();
    let _ = qs.wait();
    if let Err(l) = done {
        panic!("qs exited before it drew the cases:\n{l}");
    }
    let text = log.text();
    let mut all = vec![];
    for line in text.lines() {
        let Some((_, json)) = line.split_once("PROBE CASE ") else {
            continue;
        };
        let v: serde_json::Value = serde_json::from_str(json).unwrap();
        let at = v["at"].as_u64().unwrap();
        assert_eq!(at as usize, all.len());
        let rects = v["rects"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| {
                let n = |i: usize| r[i].as_f64().unwrap().round() as i64;
                (n(0), n(1), n(2))
            })
            .collect();
        all.push(Drawn {
            name: v["name"].as_str().unwrap().to_string(),
            rects,
            image: image::open(out.join(format!("{at}.png")))
                .unwrap()
                .to_rgba8(),
        });
    }
    assert_eq!(all.len(), cases.as_array().unwrap().len());
    all
}

/// The characters of `text` drawn differently in `ours`: the line of each (from 1) and what
/// differs.
fn differences(text: &str, kde: &Drawn, ours: &Drawn) -> Vec<(usize, String)> {
    // A picture is as wide as its widest line, which bold and slanted characters change:
    // past its edge is the black both are drawn on.
    let at = |d: &Drawn, x: i64, y: i64| {
        let inside = x < i64::from(d.image.width()) && y < i64::from(d.image.height());
        if inside {
            *d.image.get_pixel(x as u32, y as u32)
        } else {
            image::Rgba([0, 0, 0, 255])
        }
    };
    let units: Vec<u16> = text.encode_utf16().collect();
    assert_eq!(kde.rects.len(), units.len() + 1);
    let mut all = vec![];
    let mut line = 1;
    for (i, &u) in units.iter().enumerate() {
        if u == u16::from(b'\n') {
            line += 1;
            continue;
        }
        let (x, y, height) = kde.rects[i];
        let (next_x, next_y, _) = kde.rects[i + 1];
        assert_eq!(next_y, y, "character {i} of {text:?} ends on another line");
        let xs = (x - SPACING / 2).max(0)..next_x - SPACING / 2;
        let differs = (y..y + height)
            .flat_map(|py| xs.clone().map(move |px| (px, py)))
            .find(|&(px, py)| at(kde, px, py) != at(ours, px, py));
        if let Some((px, py)) = differs {
            let (k, o) = (at(kde, px, py).0, at(ours, px, py).0);
            let (dx, dy) = (px - x, py - y);
            let c = String::from_utf16_lossy(&[u]);
            all.push((
                line,
                format!("{c:?} in line {line} ({dx}, {dy} of its box: {k:?}, here {o:?})"),
            ));
        }
    }
    all
}

/// How many characters of `text` are drawn differently in `ours`, and the first such one.
fn differing(text: &str, kde: &Drawn, ours: &Drawn) -> (usize, Option<String>) {
    let all = differences(text, kde, ours);
    (all.len(), all.into_iter().next().map(|d| d.1))
}

/// Whether the test can run; says why not.
fn can_draw() -> bool {
    if !testenv::require("qs") {
        return false;
    }
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        testenv::unavailable("a headless sway (WAYLAND_DISPLAY)");
        return false;
    }
    true
}

/// A block of `text` in `lang` as the native shell draws it: a TextEdit of the fixture's font
/// on black, `size` large, with the spans of `theme` as its SyntaxHighlighter leaves them.
/// A block of `size` as MessageCodeBlock.qml sets it up, with `more` beside the text.
fn block(text: &str, size: (usize, usize), more: &str) -> String {
    format!(
        "import QtQuick\nimport org.kde.syntaxhighlighting\nRectangle {{\n    \
         width: {}; height: {}; color: \"#000000\"\n    \
         property Item edit: edit\n    TextEdit {{\n        id: edit\n        text: {}\n        \
         color: \"#12fe56\"\n        font.family: \"Noto Sans Mono\"\n        \
         font.pixelSize: 20\n        renderType: TextEdit.NativeRendering\n    }}\n{more}}}\n",
        size.0,
        size.1,
        serde_json::Value::from(text)
    )
}

type SharedFonts = std::rc::Rc<std::cell::RefCell<scene::Fonts>>;

/// A runtime that measures text as the shell does, with `source` loaded: its root and the
/// TextEdit of the block.
fn loaded(source: &str) -> (qml::runtime::Runtime, SharedFonts, usize, usize) {
    let mut rt = qml::runtime::Runtime::new(None);
    let fonts = SharedFonts::default();
    {
        let fonts = fonts.clone();
        rt.world.borrow_mut().text_measure = Some(Box::new(move |w, i| {
            if let Ok(mut f) = fonts.try_borrow_mut() {
                scene::measure_text(w, &mut f, i);
            }
        }));
    }
    let root = rt.load_str(source).unwrap();
    rt.settle();
    let qml::value::Value::Item(edit) = rt.get(root, "edit") else {
        panic!("the TextEdit of {source}");
    };
    (rt, fonts, root, edit)
}

fn picture(
    rt: &mut qml::runtime::Runtime,
    fonts: &SharedFonts,
    root: usize,
    size: (usize, usize),
) -> Canvas {
    let mut picture = Canvas::new(size.0, size.1, [0, 0, 0, 0]);
    scene::paint_window(
        rt,
        &mut fonts.borrow_mut(),
        root,
        &mut Target::Cpu(&mut picture),
    );
    picture
}

fn painted(lang: &str, theme: &str, text: &str, size: (usize, usize)) -> Canvas {
    let (mut rt, fonts, root, edit) = loaded(&block(text, size, ""));
    rt.world.borrow_mut().syntax.results.insert(
        edit,
        std::sync::Arc::new(qml::syntax::Highlight {
            text: text.into(),
            spans: qml::syntax::highlight(text, lang, theme),
        }),
    );
    let window = [size.0 as f64, size.1 as f64];
    scene::layout_window(&mut rt, &mut fonts.borrow_mut(), root, window);
    rt.settle();
    picture(&mut rt, &fonts, root, size)
}

/// The window of `root`, last drawn as `shown`, driven as the shell drives one until the
/// colours of `edit` are in: laid out again when the layout revision moved or an item was
/// resized, drawn again when an item was touched. The picture it shows then.
fn until_coloured(
    rt: &mut qml::runtime::Runtime,
    fonts: &SharedFonts,
    root: usize,
    edit: usize,
    size: (usize, usize),
    mut shown: Canvas,
) -> Canvas {
    let window = [size.0 as f64, size.1 as f64];
    let mut laid = {
        let mut w = rt.world.borrow_mut();
        w.touched.clear();
        w.resized.clear();
        w.layout_rev
    };
    let mut now = 0.0;
    while !rt.world.borrow().syntax.results.contains_key(&edit) {
        rt.poll_native();
        now += 16.0;
        rt.tick(now);
        rt.settle();
        let (touched, resized, rev) = {
            let mut w = rt.world.borrow_mut();
            (
                std::mem::take(&mut w.touched),
                std::mem::take(&mut w.resized),
                w.layout_rev,
            )
        };
        if !touched.is_empty() || !resized.is_empty() || rev != laid {
            scene::invalidate_layers(&rt.world.borrow(), &touched);
            if rev != laid {
                scene::layout_window(rt, &mut fonts.borrow_mut(), root, window);
            } else {
                let mut w = rt.world.borrow_mut();
                scene::relayout_targets(&mut w, &mut fonts.borrow_mut(), &resized, 0);
            }
            rt.settle();
            shown = picture(rt, fonts, root, size);
            let mut w = rt.world.borrow_mut();
            w.touched.clear();
            w.resized.clear();
            laid = w.layout_rev;
        }
        if !rt.world.borrow().syntax.results.contains_key(&edit) {
            rt.wait_native();
        }
    }
    shown
}

/// A block whose colours arrive after it was drawn (the highlighter works on another thread,
/// and an answer's block is on screen before it ends) is drawn again in them, with nothing
/// else changing: Quickshell shows the colours at once. The window is driven as the shell
/// drives one: laid out again when the layout revision moved or an item was resized, drawn
/// again when an item was touched.
#[test]
#[cfg_attr(
    not(feature = "interp"),
    ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
)]
fn a_block_takes_its_colours_when_they_arrive() {
    let size = (360, 140);
    let text = "/// A shape. TODO: more\nfn area(r: f64) -> f64 {\n    // BEGIN FIXME: later\n    \
                3.14 * r * r\n}";
    let highlighter = "    SyntaxHighlighter {\n        textEdit: edit\n        \
                       repository: Repository\n        \
                       definition: Repository.definitionForName(\"rust\")\n        \
                       theme: \"Monokai\"\n    }\n";
    let (mut rt, fonts, root, edit) = loaded(&block(text, size, highlighter));
    let window = [size.0 as f64, size.1 as f64];
    scene::layout_window(&mut rt, &mut fonts.borrow_mut(), root, window);
    rt.settle();
    let shown = picture(&mut rt, &fonts, root, size);
    let plain = painted("plaintext", "Monokai", text, size);
    let coloured = painted("rust", "Monokai", text, size);
    assert!(plain.px != coloured.px, "the colours change the picture");
    let shown = until_coloured(&mut rt, &fonts, root, edit, size, shown);
    let differ = (0..shown.px.len())
        .filter(|&i| shown.px[i] != coloured.px[i])
        .count();
    assert_eq!(
        differ, 0,
        "pixels of the block on screen that are not its coloured picture"
    );
}

/// The colours change no line of the block: QSyntaxHighlighter sets formats on the lines of
/// the document, and the one after a last line break stays. The text of the original's block ends in a
/// line break; the block, set up as MessageCodeBlock.qml sets it up (a TextArea in a
/// ScrollView as high as the text, in a column), is as large with its colours as it was
/// before them.
#[test]
#[cfg_attr(
    not(feature = "interp"),
    ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
)]
fn a_block_keeps_its_size_when_its_colours_arrive() {
    let size = (420, 260);
    let text = "/// A shape. TODO: more\nfn area(r: f64) -> f64 {\n    // BEGIN FIXME: later\n    \
                3.14 * r * r\n}\n";
    let source = format!(
        "import QtQuick\nimport QtQuick.Controls\nimport QtQuick.Layouts\n\
         import org.kde.syntaxhighlighting\nRectangle {{\n    \
         width: {}; height: {}; color: \"#000000\"\n    \
         property Item edit: codeTextArea\n    property Item scroll: codeScrollView\n    \
         property Item back: back\n    ColumnLayout {{\n        \
         anchors.left: parent.left; anchors.right: parent.right\n        \
         Rectangle {{\n            id: back\n            Layout.fillWidth: true\n            \
         color: \"#111111\"\n            implicitHeight: codeColumnLayout.implicitHeight\n            \
         ColumnLayout {{\n                id: codeColumnLayout\n                \
         anchors.fill: parent\n                spacing: 0\n                \
         ScrollView {{\n                    id: codeScrollView\n                    \
         Layout.fillWidth: true\n                    implicitWidth: parent.width\n                    \
         implicitHeight: codeTextArea.implicitHeight + 1\n                    \
         contentWidth: codeTextArea.width - 1\n                    clip: true\n                    \
         TextArea {{\n                        id: codeTextArea\n                        \
         Layout.fillWidth: true\n                        readOnly: true\n                        \
         renderType: Text.NativeRendering\n                        \
         font.family: \"Noto Sans Mono\"\n                        font.pixelSize: 15\n                        \
         color: \"#12fe56\"\n                        text: {}\n                        \
         SyntaxHighlighter {{\n                            textEdit: codeTextArea\n                            \
         repository: Repository\n                            \
         definition: Repository.definitionForName(\"rust\")\n                            \
         theme: \"Monokai\"\n                        }}\n                    }}\n                }}\n            \
         }}\n        }}\n    }}\n}}\n",
        size.0,
        size.1,
        serde_json::Value::from(text)
    );
    let sizes = |rt: &qml::runtime::Runtime, root: usize| {
        let mut all = vec![];
        for name in ["edit", "scroll", "back"] {
            let qml::value::Value::Item(item) = rt.get(root, name) else {
                panic!("the {name} of the block");
            };
            for p in ["y", "width", "height", "implicitHeight"] {
                all.push(format!("{name}.{p} {}", rt.get(item, p).to_number()));
            }
        }
        all
    };
    let (mut rt, fonts, root, edit) = loaded(&source);
    let window = [size.0 as f64, size.1 as f64];
    scene::layout_window(&mut rt, &mut fonts.borrow_mut(), root, window);
    rt.settle();
    let shown = picture(&mut rt, &fonts, root, size);
    let before = sizes(&rt, root);
    let lines = text.split('\n').count() as f64;
    let qml::value::Value::Item(area) = rt.get(root, "edit") else {
        panic!("the TextArea of the block");
    };
    let high = rt.get(area, "implicitHeight").to_number();
    let pad = rt.get(area, "topPadding").to_number() + rt.get(area, "bottomPadding").to_number();
    assert_eq!((high - pad) % lines, 0.0, "{lines} lines of one height");
    until_coloured(&mut rt, &fonts, root, edit, size, shown);
    assert_eq!(before, sizes(&rt, root));
    // Laid out anew, as when the panel is opened again.
    scene::layout_window(&mut rt, &mut fonts.borrow_mut(), root, window);
    rt.settle();
    assert_eq!(before, sizes(&rt, root));
}

/// qml/modules/rusty/sidebarLeft/aiChat/MessageCodeBlock.qml draws the block with
/// `renderType: Text.NativeRendering`. A block with coloured, bold and italic words, words on
/// a background of their own (`TODO`, `FIXME`, `BEGIN`) and a struck out one (`~~strike~~`),
/// and one of a language without a definition, drawn by the native shell and by `qs` under
/// KDE's highlighter: the same pixels in both themes. Qt adds the lines of a text after all of
/// its glyphs, each as a stroke with soft edges (QQuickTextNodeEngine::addToSceneGraph,
/// QSGRhiInternalTextNode::addDecorationNode).
#[test]
#[cfg_attr(
    not(feature = "interp"),
    ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
)]
fn a_coloured_block_is_drawn_as_quickshell_draws_it() {
    if !can_draw() {
        return;
    }
    let size = (360, 140);
    let texts = [
        (
            "rust",
            "/// A shape. TODO: more\nfn area(r: f64) -> f64 {\n    // BEGIN FIXME: later\n    \
             3.14 * r * r\n}",
        ),
        (
            "markdown",
            "# Title\n- [ ] task with ~~strike~~\n**bold** and *italic*\n~~a *b* **c**~~ d",
        ),
        ("hyprlang", "bind = SUPER, Q, killactive"),
    ];
    let mut cases = vec![];
    for (lang, text) in texts {
        for theme in THEMES {
            cases.push(serde_json::json!({
                "lang": lang, "theme": theme, "text": text, "native": true,
                "width": size.0, "height": size.1,
            }));
        }
    }
    let drawn = drawn(&serde_json::Value::Array(cases));
    let mut wrong = vec![];
    for (i, (lang, text)) in texts.iter().enumerate() {
        for (t, theme) in THEMES.iter().enumerate() {
            let qs = &drawn[i * 2 + t].image;
            assert_eq!((qs.width() as usize, qs.height() as usize), size);
            let ours = painted(lang, theme, text, size);
            let (mut count, mut most, mut rows) = (0, 0, vec![]);
            for (x, y, qs) in qs.enumerate_pixels() {
                let ours = ours.px[y as usize * ours.w + x as usize];
                let by = (0..3).map(|k| qs.0[k].abs_diff(ours[k])).max().unwrap();
                if by > 0 {
                    count += 1;
                    most = most.max(by);
                    if rows.last() != Some(&y) {
                        rows.push(y);
                    }
                }
            }
            if count > 0 {
                wrong.push(format!(
                    "{lang}, {theme}: {count} pixels differ, by up to {most} of 255, in rows \
                     {rows:?}"
                ));
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "a block is not drawn as Quickshell draws it:\n{}",
        wrong.join("\n")
    );
}

/// docs/quickshell-differences.md, "Code inside Markdown and LaTeX code blocks": KDE has a
/// definition for Perl and impulse-shell does not carry it. Quickshell colours Perl code, alone and
/// inside a Markdown or LaTeX block; impulse-shell leaves it in one colour, and colours the Python
/// code beside it as Quickshell does.
#[test]
fn code_of_a_language_outside_the_set_is_coloured_only_by_quickshell() {
    if !can_draw() {
        return;
    }
    let perl = "my $n = 1;\nprint \"n is $n\\n\" if $n > 0; # said";
    let python = "def f(x):\n    return x + 1  # one more";
    // (language, text, the lines of the Perl code).
    let texts = [
        ("perl".to_string(), perl.to_string(), 1..=2),
        (
            "markdown".to_string(),
            format!("Text\n\n```python\n{python}\n```\n\n```perl\n{perl}\n```\n\nText"),
            9..=10,
        ),
        (
            "latex".to_string(),
            format!(
                "\\begin{{minted}}{{python}}\n{python}\n\\end{{minted}}\n\\begin{{minted}}{{perl}}\n{perl}\n\\end{{minted}}\nText"
            ),
            6..=7,
        ),
    ];
    let mut cases = vec![];
    for (lang, text, _) in &texts {
        for theme in THEMES {
            // Quickshell's Perl code has bold words, which are wider: both pictures get one
            // size, or the same characters come out a shade apart.
            let size = serde_json::json!({ "width": 1000, "height": 400 });
            let mut kde = serde_json::json!({ "lang": lang, "theme": theme, "text": text });
            let spans = qml::syntax::highlight(text, lang, theme);
            let mut ours = serde_json::json!({ "text": rich(text, &spans) });
            for case in [&mut kde, &mut ours] {
                case.as_object_mut()
                    .unwrap()
                    .extend(size.as_object().unwrap().clone());
            }
            cases.extend([kde, ours]);
        }
    }
    let drawn = drawn(&serde_json::Value::Array(cases));
    for (i, (lang, text, perl_lines)) in texts.iter().enumerate() {
        for (t, theme) in THEMES.iter().enumerate() {
            let all = differences(text, &drawn[i * 4 + t * 2], &drawn[i * 4 + t * 2 + 1]);
            assert!(
                !all.is_empty(),
                "{lang}, {theme}: Quickshell draws the Perl code as impulse-shell does"
            );
            let outside: Vec<_> = all.iter().filter(|d| !perl_lines.contains(&d.0)).collect();
            assert!(
                outside.is_empty(),
                "{lang}, {theme}: differs outside the Perl code: {outside:?}"
            );
        }
    }
}

#[test]
fn code_is_coloured_as_quickshell_colours_it() {
    if !can_draw() {
        return;
    }
    let texts = texts();
    let mut cases = vec![];
    for (lang, text) in &texts {
        for theme in THEMES {
            cases.push(serde_json::json!({ "lang": lang, "theme": theme, "text": text }));
            let spans = qml::syntax::highlight(text, lang, theme);
            cases.push(serde_json::json!({ "text": rich(text, &spans) }));
        }
    }
    let drawn = drawn(&serde_json::Value::Array(cases));
    let mut report = String::new();
    let mut failed = 0;
    for (i, (lang, text)) in texts.iter().enumerate() {
        let total = text.chars().filter(|c| *c != '\n').count();
        let ours_name = qml::syntax::find(lang).unwrap_or("None");
        let kde_name = &drawn[i * 4].name;
        let mut line = format!("{lang}: {total} characters");
        if ours_name != *kde_name {
            failed += 1;
            line += &format!(", named {ours_name:?} for {kde_name:?}");
        }
        for (t, theme) in THEMES.iter().enumerate() {
            let (n, first) = differing(text, &drawn[i * 4 + t * 2], &drawn[i * 4 + t * 2 + 1]);
            if let Some(first) = first {
                failed += 1;
                line += &format!(", {theme}: {n} differ, first {first}");
            }
        }
        report += &line;
        report.push('\n');
    }
    assert_eq!(
        failed, 0,
        "code is not coloured as Quickshell colours it:\n{report}"
    );
}
