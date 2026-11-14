//! A ListView or GridView keeps its current row in sight, as Qt 6.12 does
//! (QQuickItemView::trackedPositionChanged): it is scrolled to the row it is made with, and to
//! the one a write of `currentIndex` chooses. Each case is a scene in
//! `scenes/view_current_row/NAME.qml` that leaves its findings in the root's `result`; its
//! first line, `// qt: ...`, is what real Qt left there (`scenes/view_current_row/qt.py`, run
//! in the testenv image).
//!
//! The scene runs on frames of 16 ms, laid out on each, until it has its `result`.

use shell::scene;

fn ours(name: &str) -> String {
    let (mut rt, root) = testscenes::load(&format!("view_current_row/{name}.qml"));
    rt.settle();
    let size = [rt.get(root, "width"), rt.get(root, "height")].map(|v| v.to_number());
    let mut fonts = scene::Fonts::default();
    let mut now = 0.0;
    rt.tick(now);
    for _ in 0..1000 {
        scene::layout_window(&mut rt, &mut fonts, root, size);
        let result = rt.get(root, "result").to_js_string();
        if !result.is_empty() {
            let errors = rt.errors();
            assert!(errors.is_empty(), "{name}: {result}\n{errors:?}");
            return result;
        }
        now += 16.0;
        rt.tick(now);
    }
    panic!("{name}: no result after 16 s of frames");
}

fn check(name: &str, src: &str) {
    let qt = src
        .lines()
        .find_map(|l| l.strip_prefix("// qt: "))
        .expect("the scene's `// qt:` line");
    let ours = ours(name);
    let steps = |s: &str| s.split(" | ").map(str::to_owned).collect::<Vec<_>>();
    let differ: Vec<String> = steps(qt)
        .into_iter()
        .zip(steps(&ours))
        .filter(|(q, o)| q != o)
        .map(|(q, o)| format!("    Qt {q}\n  ours {o}"))
        .collect();
    assert!(ours == qt, "{name}:\n{}", differ.join("\n"));
}

/// A list opened on a row far down shows that row; the arrow keys' steps past the end of the
/// view and back bring the row in sight, in a ListView of either direction and in a GridView;
/// the first row brings the header and the margin in sight, the last one the footer.
#[test]
fn current_row() {
    check(
        "current_row",
        include_str!("scenes/view_current_row/current_row.qml"),
    );
}
