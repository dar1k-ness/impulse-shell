//! The tree's `PagePlaceholder` in a narrow page (the left sidebar's assistant chat empty state): a
//! description wider than the page wraps inside it instead of overflowing both edges, as a
//! centered ColumnLayout sized to the wrapping Text's unwrapped implicitWidth would.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::scene;
use std::path::{Path, PathBuf};

const DESCRIPTION: &str = "Talking to claude-code-acp --model claude-sonnet-4 over the Agent Client Protocol\\nModel and mode are picked with the chips below.";
const WIDTH: f64 = 344.0;
const HEIGHT: f64 = 700.0;

fn tree() -> PathBuf {
    let ri = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../qml")
        .canonicalize()
        .unwrap();
    let d = std::env::temp_dir().join(format!("page-placeholder-wrap-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    for e in std::fs::read_dir(&ri).unwrap() {
        let e = e.unwrap();
        std::os::unix::fs::symlink(e.path(), d.join(e.file_name())).unwrap();
    }
    std::fs::write(
        d.join("main.qml"),
        format!(
            "import QtQuick\nimport ri.modules.common.widgets\nItem {{\n    width: {WIDTH}\n    height: {HEIGHT}\n    \
             PagePlaceholder {{ icon: \"forum\"; title: \"Claude Code\"; description: \"{DESCRIPTION}\"; \
             descriptionHorizontalAlignment: Text.AlignHCenter }}\n}}\n"
        ),
    )
    .unwrap();
    d
}

#[test]
fn long_description_wraps_inside_the_page() {
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
    for _ in 0..60 {
        now += 16.0;
        rt.tick(now);
        rt.settle();
        scene::layout_window(&mut rt, &mut fonts, main, [WIDTH, HEIGHT]);
        rt.settle();
    }
    let mut w = rt.world.borrow_mut();
    let want = DESCRIPTION.replace("\\n", "\n");
    let text = (0..w.items.len())
        .find(|&i| {
            !w.dead.contains(&i)
                && w.items[i].types.iter().any(|t| t == "StyledText")
                && w.read(i, "text").is_some_and(|v| v.to_js_string() == want)
        })
        .expect("no description text");
    let num = |w: &mut qml::runtime::World, i: usize, k: &str| {
        w.read(i, k).unwrap_or(Value::Num(f64::NAN)).to_number()
    };
    let column = w.items[text].parent.unwrap();
    let (mut x, mut i) = (0.0, Some(text));
    while let Some(k) = i.filter(|&k| k != main) {
        x += num(&mut w, k, "x");
        i = w.items[k].parent;
    }
    let width = num(&mut w, text, "width");
    let implicit = num(&mut w, text, "implicitWidth");
    let lines = num(&mut w, text, "lineCount");
    let height = num(&mut w, text, "height");
    let column_height = num(&mut w, column, "height");
    drop(w);
    std::fs::remove_dir_all(&d).unwrap();
    assert!(
        implicit > WIDTH,
        "fixture must overflow unwrapped: {implicit}"
    );
    assert_eq!(width, WIDTH - 2.0 * 16.0, "description width");
    assert_eq!(x, 16.0, "description x in the page");
    assert!(lines >= 3.0, "wrapped into {lines} lines");
    assert!(
        column_height >= height,
        "column {column_height} holds the wrapped text {height}"
    );
}
