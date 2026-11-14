//! The tree's settings window driven by state, not by timers or transient labels: the navigation
//! column keeps its width while the Config file button shows "Path copied" (Russian labels are
//! wider than the tabs), "go to setting" lands on a row revealed under a collapsed parent once the
//! reveal has settled, pages preload one after another without a polling Timer (also past a page
//! that fails to load), a segmented choice slides its highlight on the first change after its
//! first layout, and the Quick page's theme tiles sit beside the wallpaper whenever both fit.

use qml::runtime::{Runtime, World};
use qml::value::Value;
use shell::scene;
use std::path::{Path, PathBuf};

const SIZE: [f64; 2] = [1120.0, 780.0];

fn ri() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../qml")
        .canonicalize()
        .unwrap()
}

/// Whether this is `test`'s own process (see `testenv::run_in_own_process`). Elsewhere runs
/// it there, with a fresh HOME holding a config.json (`config` its JSON) and `vars` set too.
fn own_home(test: &str, config: &str, vars: &[(&'static str, &str)]) -> bool {
    if testenv::in_own_process(test) {
        return true;
    }
    let home = tempfile::tempdir().unwrap();
    let h = home.path();
    std::fs::create_dir_all(h.join(".config/rusty-impulse")).unwrap();
    std::fs::create_dir_all(h.join(".local/state/rustshell/user")).unwrap();
    std::fs::write(h.join(".config/rusty-impulse/config.json"), config).unwrap();
    let mut env = testenv::home_vars(h);
    env.extend(vars.iter().map(|&(k, v)| (k, v.into())));
    testenv::run_in_own_process(test, env);
    false
}

struct Win {
    rt: Runtime,
    root: usize,
    fonts: scene::Fonts,
    now: f64,
    size: [f64; 2],
}

impl Win {
    fn load(dir: &Path, file: &str, size: [f64; 2]) -> Win {
        let mut rt = Runtime::new(Some(dir.to_path_buf()));
        // As the shell: items are laid out after creation, Behaviors arm after the first layout.
        rt.world.borrow_mut().track_fresh = true;
        let dirs: Vec<PathBuf> = [
            "modules/common",
            "modules/common/functions",
            "modules/common/utils",
            "services",
            "",
        ]
        .iter()
        .map(|d| dir.join(d))
        .collect();
        rt.load_singletons_lazy(&dirs);
        let root = rt.load_file(&dir.join(file)).unwrap();
        rt.settle();
        Win {
            rt,
            root,
            fonts: scene::Fonts::default(),
            now: 0.0,
            size,
        }
    }

    /// One frame: native events, the animation clock 16 ms on, bindings, layout.
    fn frame(&mut self) {
        self.rt.poll_native();
        self.now += 16.0;
        self.rt.tick(self.now);
        self.rt.settle();
        scene::layout_window(&mut self.rt, &mut self.fonts, self.root, self.size);
        self.rt.settle();
    }

    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.frame();
        }
    }

    fn num(&self, i: usize, k: &str) -> f64 {
        self.rt.get(i, k).to_number()
    }

    /// The live item for which `f` holds (panics unless exactly one does).
    fn find(&self, what: &str, f: impl Fn(&mut World, usize) -> bool) -> usize {
        let mut w = self.rt.world.borrow_mut();
        let hits: Vec<usize> = (0..w.items.len())
            .filter(|&i| !w.dead.contains(&i) && f(&mut w, i))
            .collect();
        assert_eq!(hits.len(), 1, "{what}: {hits:?}");
        hits[0]
    }

    fn parent(&self, i: usize) -> usize {
        self.rt.world.borrow().items[i].parent.unwrap()
    }
}

fn is(w: &World, i: usize, t: &str) -> bool {
    w.items[i].types.iter().any(|x| x == t)
}

fn has(w: &World, i: usize, k: &str) -> bool {
    w.items[i].props.contains_key(k)
}

fn settings(test: &str, vars: &[(&'static str, &str)]) -> Option<Win> {
    settings_with(test, "{}\n", vars)
}

/// `settings`, with `config` as config.json.
fn settings_with(test: &str, config: &str, vars: &[(&'static str, &str)]) -> Option<Win> {
    if !own_home(test, config, vars) {
        return None;
    }
    let mut win = Win::load(&ri(), "settings.qml", SIZE);
    win.frames(90);
    Some(win)
}

/// Russian: "Путь скопирован" is wider than every tab label. Clicking "Файл настроек" (copying
/// the path) used to widen the whole column for 1.5 s and slide the content pane right and back.
#[test]
fn nav_column_width_ignores_the_copied_label() {
    let Some(mut win) = settings(
        concat!(
            module_path!(),
            "::nav_column_width_ignores_the_copied_label"
        ),
        &[("LANG", "ru_RU.UTF-8"), ("LC_ALL", "ru_RU.UTF-8")],
    ) else {
        return;
    };
    let fab = win.find("FAB", |w, i| is(w, i, "FloatingActionButton"));
    let tabs = win.find("tab array", |w, i| is(w, i, "NavigationRailTabArray"));
    let column = win.parent(win.parent(fab));
    assert_eq!(
        win.rt.get(fab, "buttonText").to_js_string(),
        "Файл настроек"
    );
    // ri-testenv has no Material Symbols font, so "edit" and "check" would render as words of
    // different widths; with the font both are one 26 px glyph. Only the label switches here.
    win.rt.set(fab, "iconText", Value::str("edit"));
    win.frames(30);
    let width = win.num(column, "width");
    win.rt.set(fab, "justCopied", Value::Bool(true));
    let mut seen = vec![];
    for _ in 0..40 {
        win.frame();
        seen.push(win.num(column, "width"));
    }
    assert_eq!(
        win.rt.get(fab, "buttonText").to_js_string(),
        "Путь скопирован"
    );
    let (fab_w, tabs_w) = (win.num(fab, "width"), win.num(tabs, "implicitWidth"));
    assert!(
        fab_w > tabs_w,
        "fixture: the copied label ({fab_w}) must be wider than the tabs ({tabs_w})"
    );
    assert!(
        seen.iter().all(|&w| w == width),
        "column width {width} moved while the label switched: {seen:?}"
    );
    assert!(
        fab_w <= width,
        "the FAB ({fab_w}) fits the column ({width})"
    );
}

/// Search's "go to setting" on a row under a collapsed SettingsExpandRow on another page: the
/// page ends scrolled to the row's final place. A fixed 140 ms wait scrolled while the rows
/// above it were still sliding open (220 ms) and left the row off its mark.
#[test]
fn go_to_setting_lands_on_a_revealed_row() {
    let Some(mut win) = settings(
        concat!(module_path!(), "::go_to_setting_lands_on_a_revealed_row"),
        &[],
    ) else {
        return;
    };
    let row = win.find("Web search prefix row", |w, i| {
        has(w, i, "parentRow")
            && matches!(w.read(i, "parentRow"), Some(Value::Item(_)))
            && w.read(i, "title")
                .is_some_and(|t| t.to_js_string() == "Web search")
    });
    assert_eq!(
        win.num(row, "reveal"),
        0.0,
        "fixture: the row starts folded"
    );
    let page = {
        let w = win.rt.world.borrow();
        let mut i = row;
        loop {
            i = w.items[i].parent.unwrap();
            if w.items[i].props.contains_key("settingsPage") {
                break i;
            }
        }
    };
    win.rt
        .call_method(win.root, "goToSetting", vec![Value::Item(row)])
        .unwrap()
        .unwrap();
    win.rt.settle();
    win.frames(120);
    assert_eq!(win.num(row, "reveal"), 1.0);
    // The row's place in the page column (the outermost ColumnLayout below the page): its y and
    // its ancestors' below that column.
    let (y, max_y) = {
        let mut w = win.rt.world.borrow_mut();
        let mut chain = vec![row];
        while let Some(p) = w.items[*chain.last().unwrap()].parent
            && p != page
        {
            chain.push(p);
        }
        let column = chain
            .iter()
            .rposition(|&c| is(&w, c, "ColumnLayout"))
            .unwrap();
        let y: f64 = chain[..column]
            .iter()
            .map(|&c| w.read(c, "y").unwrap_or_default().to_number())
            .sum();
        let ch = w.read(page, "contentHeight").unwrap().to_number();
        let h = w.read(page, "height").unwrap().to_number();
        (y, (ch - h).max(0.0))
    };
    let want = (y - 24.0).clamp(0.0, max_y);
    assert!(want > 0.0, "fixture: the row is below the fold");
    assert_eq!(win.num(page, "contentY"), want, "row at {y}");
}

/// Runs event loop turns (native events, `Qt.callLater`, bindings; the clock stays put) until
/// `preloadCount` reaches the page count, at most one turn per page plus two: the count by turn.
fn preload_turns(win: &mut Win, stack: usize, pages: usize) -> Vec<f64> {
    let mut counts = vec![];
    for _ in 0..pages + 2 {
        win.rt.poll_native();
        win.rt.tick(win.now);
        win.rt.settle();
        counts.push(win.num(stack, "preloadCount"));
    }
    counts
}

fn page_count(win: &Win) -> usize {
    match win.rt.get(win.root, "pages") {
        Value::Obj(o) => o.borrow().array.as_ref().map_or(0, |a| a.len()),
        _ => 0,
    }
}

/// The page stack's Loaders in page order: (status, built).
fn page_loaders(win: &Win, stack: usize) -> Vec<(f64, bool)> {
    let mut w = win.rt.world.borrow_mut();
    let loaders: Vec<usize> = w.items[stack]
        .children
        .clone()
        .into_iter()
        .filter(|&c| is(&w, c, "Loader"))
        .collect();
    loaders
        .iter()
        .map(|&l| {
            let status = w.read(l, "status").map_or(f64::NAN, |v| v.to_number());
            (status, matches!(w.read(l, "item"), Some(Value::Item(_))))
        })
        .collect()
}

/// The pages after the first preload one per event loop turn, each once the one before it is
/// built: no Timer polls `preloadCount`, so no animation clock time has to pass.
#[test]
fn pages_preload_without_a_timer() {
    if !own_home(
        concat!(module_path!(), "::pages_preload_without_a_timer"),
        "{}\n",
        &[],
    ) {
        return;
    }
    let mut win = Win::load(&ri(), "settings.qml", SIZE);
    let stack = win.find("page stack", |w, i| has(w, i, "preloadCount"));
    let pages = page_count(&win);
    assert!(pages > 1);
    let counts = preload_turns(&mut win, stack, pages);
    assert_eq!(
        *counts.last().unwrap(),
        pages as f64,
        "preloadCount by turn: {counts:?}"
    );
    let loaded = page_loaders(&win, stack).iter().filter(|l| l.1).count();
    assert_eq!(loaded, pages, "every page built");
}

const LOADER_READY: f64 = 1.0;
const LOADER_ERROR: f64 = 3.0;

/// A page whose file fails to compile ends in Loader.Error and the preload goes on past it, as
/// the 120 ms Timer the frontier replaced did. Qt 6.12 (PySide6, [`FRONTIER`] below): with
/// `status === Loader.Ready` alone the count stops at the failed page for good.
#[test]
fn pages_preload_past_a_page_that_fails_to_load() {
    if !own_home(
        concat!(
            module_path!(),
            "::pages_preload_past_a_page_that_fails_to_load"
        ),
        "{}\n",
        &[],
    ) {
        return;
    }
    let d = tree_with(
        "preload-error",
        &[("modules/settings/BarConfig.qml", "import QtQuick\nItem {\n")],
    );
    let mut win = Win::load(&d, "settings.qml", SIZE);
    let stack = win.find("page stack", |w, i| has(w, i, "preloadCount"));
    let pages = page_count(&win);
    let counts = preload_turns(&mut win, stack, pages);
    assert_eq!(
        *counts.last().unwrap(),
        pages as f64,
        "preloadCount by turn: {counts:?}"
    );
    let loaders = page_loaders(&win, stack);
    let want: Vec<(f64, bool)> = (0..pages)
        .map(|i| {
            if i == 2 {
                (LOADER_ERROR, false)
            } else {
                (LOADER_READY, true)
            }
        })
        .collect();
    assert_eq!(
        loaders, want,
        "(status, built) by page; BarConfig is page 2"
    );
}

/// The frontier rule on its own, pages failing to compile (`Bad.qml`) and missing. Qt 6.12
/// (PySide6, offscreen) prints `count=6 frontier=0:1,1:1,2:3,3:1,4:3,5:1
/// status=1i,1i,3-,1i,3-,1i`; with `readyOnly: true` it prints `count=3 frontier=0:1,1:1
/// status=1i,1i,3-,0-,0-,0-`.
const FRONTIER: &str = r#"import QtQuick
Item {
    id: root
    property bool readyOnly: false
    property var pages: ["Good.qml", "Good.qml", "Bad.qml", "Good.qml", "Missing.qml", "Good.qml"]
    property string out: ""
    Item {
        id: pageStack
        property int preloadCount: 1
        function preloadNext() {
            if (pageStack.preloadCount < root.pages.length)
                pageStack.preloadCount += 1;
        }
        Repeater {
            id: rep
            model: root.pages
            Loader {
                required property int index
                required property var modelData
                active: index < pageStack.preloadCount
                source: modelData
                readonly property bool preloadFrontier: index === pageStack.preloadCount - 1
                    && (root.readyOnly ? status === Loader.Ready : (status === Loader.Ready || status === Loader.Error))
                onPreloadFrontierChanged: if (preloadFrontier) { root.out += (root.out ? "," : "") + index + ":" + status; Qt.callLater(pageStack.preloadNext) }
            }
        }
    }
    function report() {
        var st = [];
        for (var i = 0; i < rep.count; i++)
            st.push(rep.itemAt(i).status + (rep.itemAt(i).item ? "i" : "-"));
        return "count=" + pageStack.preloadCount + " frontier=" + out + " status=" + st.join(",");
    }
}
"#;

fn frontier(ready_only: bool) -> String {
    let tag = if ready_only {
        "frontier-ready"
    } else {
        "frontier"
    };
    let d = std::env::temp_dir().join(format!("ri-settings-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(
        d.join("Good.qml"),
        "import QtQuick\nItem { property int n: 1 }\n",
    )
    .unwrap();
    std::fs::write(d.join("Bad.qml"), "import QtQuick\nItem {\n").unwrap();
    let main = FRONTIER.replace(
        "property bool readyOnly: false",
        &format!("property bool readyOnly: {ready_only}"),
    );
    std::fs::write(d.join("main.qml"), main).unwrap();
    let mut rt = Runtime::new(Some(d.clone()));
    let root = rt.load_file(&d.join("main.qml")).unwrap();
    rt.settle();
    for _ in 0..8 {
        rt.poll_native();
        rt.tick(0.0);
        rt.settle();
    }
    rt.call_method(root, "report", vec![])
        .unwrap()
        .unwrap()
        .to_js_string()
}

#[test]
fn preload_frontier_advances_past_loader_errors_as_qt() {
    assert_eq!(
        frontier(false),
        "count=6 frontier=0:1,1:1,2:3,3:1,4:3,5:1 status=1i,1i,3-,1i,3-,1i"
    );
}

#[test]
fn preload_frontier_on_ready_alone_stops_at_an_error_as_qt() {
    assert_eq!(
        frontier(true),
        "count=3 frontier=0:1,1:1 status=1i,1i,3-,0-,0-,0-"
    );
}

/// A tree under HOME that links the tree and adds `main.qml`.
fn tree(tag: &str, main: &str) -> PathBuf {
    let d = PathBuf::from(std::env::var_os("HOME").unwrap()).join(format!("tree-{tag}"));
    std::fs::create_dir_all(&d).unwrap();
    for e in std::fs::read_dir(ri()).unwrap() {
        let e = e.unwrap();
        std::os::unix::fs::symlink(e.path(), d.join(e.file_name())).unwrap();
    }
    std::fs::write(d.join("main.qml"), main).unwrap();
    d
}

/// A tree under HOME that links the tree file by file except `files` (path under qml, contents), which it
/// writes in place of the originals.
fn tree_with(tag: &str, files: &[(&str, &str)]) -> PathBuf {
    fn link(from: &Path, to: &Path, files: &[(PathBuf, &str)]) {
        std::fs::create_dir_all(to).unwrap();
        for e in std::fs::read_dir(from).unwrap() {
            let e = e.unwrap();
            let dst = to.join(e.file_name());
            if let Some((_, text)) = files.iter().find(|(p, _)| *p == dst) {
                std::fs::write(&dst, text).unwrap();
            } else if files.iter().any(|(p, _)| p.starts_with(&dst)) {
                link(&e.path(), &dst, files);
            } else {
                std::os::unix::fs::symlink(e.path(), &dst).unwrap();
            }
        }
    }
    let d = PathBuf::from(std::env::var_os("HOME").unwrap()).join(format!("tree-{tag}"));
    let files: Vec<(PathBuf, &str)> = files.iter().map(|(p, t)| (d.join(p), *t)).collect();
    link(&ri(), &d, &files);
    d
}

const SEGMENTED: &str = r#"import QtQuick
import ri.modules.common.widgets
Item {
    width: 600
    height: 100
    property alias choice: seg
    SettingsSegmented {
        id: seg
        options: [{ label: "Alpha", value: "a" }, { label: "Beta", value: "b" }, { label: "Gamma", value: "c" }]
        currentValue: "c"
    }
}
"#;

fn segmented(test: &str) -> Option<(Win, usize, usize)> {
    if !own_home(test, "{}\n", &[]) {
        return None;
    }
    let d = tree("segmented", SEGMENTED);
    let win = Win::load(&d, "main.qml", [600.0, 100.0]);
    let Value::Item(seg) = win.rt.get(win.root, "choice") else {
        panic!("no SettingsSegmented");
    };
    let highlight = {
        let w = win.rt.world.borrow();
        w.items[seg]
            .children
            .iter()
            .copied()
            .find(|&c| is(&w, c, "Rectangle"))
            .unwrap()
    };
    Some((win, seg, highlight))
}

/// Its first layout comes late (a page preloaded hidden): the highlight still appears on the
/// selection without sliding in from the left.
#[test]
fn segmented_highlight_does_not_slide_in_on_a_late_first_layout() {
    let Some((mut win, seg, highlight)) = segmented(concat!(
        module_path!(),
        "::segmented_highlight_does_not_slide_in_on_a_late_first_layout"
    )) else {
        return;
    };
    for _ in 0..20 {
        win.now += 16.0;
        win.rt.tick(win.now);
        win.rt.settle();
    }
    win.frame();
    let at = win.num(highlight, "x");
    assert!(
        at > 4.0,
        "fixture: the third option is not at the left ({at})"
    );
    for _ in 0..30 {
        win.frame();
        assert_eq!(win.num(highlight, "x"), at);
    }
    assert_eq!(win.num(seg, "highlightX"), at);
}

/// A selection changed right after the first layout slides: the 50 ms timer kept the Behavior
/// off for its first 50 ms whatever had happened by then.
#[test]
fn segmented_highlight_slides_on_the_first_change() {
    let Some((mut win, seg, highlight)) = segmented(concat!(
        module_path!(),
        "::segmented_highlight_slides_on_the_first_change"
    )) else {
        return;
    };
    win.frame();
    let from = win.num(highlight, "x");
    win.rt.set(seg, "currentValue", Value::str("a"));
    win.frame();
    let mid = win.num(highlight, "x");
    win.frames(40);
    let to = win.num(highlight, "x");
    assert!(from > to, "fixture: {from} -> {to}");
    assert!(mid > to && mid < from, "{from} -> {mid} -> {to}: no slide");
}

/// The part of `i` that shows, in window coordinates: its rect cut to its clipping ancestors'.
/// None when it or an ancestor is hidden, or nothing of it shows.
fn window_rect(w: &mut World, root: usize, i: usize) -> Option<[f64; 4]> {
    let num = |w: &mut World, i: usize, k: &str| w.read(i, k).unwrap_or_default().to_number();
    let mut chain = vec![];
    let mut c = i;
    while c != root {
        if matches!(w.read(c, "visible"), Some(Value::Bool(false))) || num(w, c, "opacity") == 0.0 {
            return None;
        }
        chain.push(c);
        c = w.items[c].parent?;
    }
    let (mut x, mut y) = (0.0, 0.0);
    let mut shown = [
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
        f64::INFINITY,
        f64::INFINITY,
    ];
    let mut rect = [0.0; 4];
    for &c in chain.iter().rev() {
        x += num(w, c, "x");
        y += num(w, c, "y");
        rect = [x, y, x + num(w, c, "width"), y + num(w, c, "height")];
        if c != i && w.read(c, "clip").is_some_and(|v| v.truthy()) {
            shown = [
                shown[0].max(rect[0]),
                shown[1].max(rect[1]),
                shown[2].min(rect[2]),
                shown[3].min(rect[3]),
            ];
        }
    }
    let r = [
        rect[0].max(shown[0]),
        rect[1].max(shown[1]),
        rect[2].min(shown[2]),
        rect[3].min(shown[3]),
    ];
    (r[2] > r[0] && r[3] > r[1]).then_some([r[0], r[1], r[2] - r[0], r[3] - r[1]])
}

/// Every visible item of the navigation rail that is outside `[0, pane left]`, as
/// "type text [x, y, w, h]". Icon glyphs are left out: ri-testenv has no Material Symbols font,
/// so they lay out as their ligature words; the 56×32 indicator holding each 24 px glyph and the
/// FAB and menu buttons are checked instead.
fn rail_intrusions(win: &Win) -> Vec<String> {
    let rail = win.find("navigation rail", |w, i| is(w, i, "NavigationRail"));
    let wrapper = win.parent(rail);
    let mut w = win.rt.world.borrow_mut();
    let row = w.items[wrapper].parent.unwrap();
    let kids = w.items[row].children.clone();
    let pane = kids[kids.iter().position(|&c| c == wrapper).unwrap() + 1];
    let pane_left = window_rect(&mut w, win.root, pane).unwrap()[0];
    let mut out = vec![];
    let mut todo = vec![rail];
    while let Some(i) = todo.pop() {
        if !w.items[i].inherits("Item") || is(&w, i, "MaterialSymbol") || is(&w, i, "ToolTip") {
            continue;
        }
        todo.extend(scene::visual_kids(&mut w, i));
        let Some(r) = window_rect(&mut w, win.root, i) else {
            continue;
        };
        // A transparent indicator (a tab neither current nor hovered) draws nothing: its border
        // has the width of 1 Qt gives it, and no pen until a width or a colour is set
        // (QQuickPen::isValid).
        let clear = is(&w, i, "Rectangle")
            && w.read(i, "color")
                .is_some_and(|c| qml::builtins::to_color(&c).a == 0)
            && (!w.items[i].props.contains_key("\x01pen")
                || w.read(i, "border.width")
                    .is_none_or(|b| b.to_number() == 0.0));
        if !clear && (r[0] < 0.0 || r[0] + r[2] > pane_left) {
            let text = w
                .read(i, "text")
                .map(|t| t.to_js_string())
                .unwrap_or_default();
            out.push(format!(
                "{} {text:?} {r:?} (pane at {pane_left})",
                w.items[i].types[0]
            ));
        }
    }
    out
}

/// The navigation rail at window widths around the collapse point (900 px), and collapsed by its
/// menu button in a wide window: no label, indicator or button reaches past the window's left
/// edge or into the content pane, in any frame. The collapsed column was 56 px, and Russian
/// labels ("Расширенные", "О программе") are wider: they ran under the pane and off the window.
fn rail_stays_beside_the_content(test: &str, vars: &[(&'static str, &str)]) {
    let Some(mut win) = settings(test, vars) else {
        return;
    };
    let rail = win.find("navigation rail", |w, i| is(w, i, "NavigationRail"));
    let mut seen = vec![];
    let mut check = |win: &mut Win, what: String, frames: usize| {
        for f in 0..frames {
            win.frame();
            seen.extend(
                rail_intrusions(win)
                    .into_iter()
                    .map(|s| format!("{what} frame {f}: {s}")),
            );
        }
    };
    for width in [1120.0, 901.0, 900.0, 860.0, 800.0, 700.0, 600.0, 1400.0] {
        win.size = [width, SIZE[1]];
        check(&mut win, format!("{width} px"), 30);
        assert_eq!(
            win.rt.get(rail, "expanded").truthy(),
            width > 900.0,
            "fixture: the rail collapses at 900 px"
        );
    }
    win.size = SIZE;
    check(&mut win, "1120 px".into(), 30);
    win.rt.set(rail, "expanded", Value::Bool(false));
    check(&mut win, "collapsed by the menu button".into(), 30);
    win.rt.set(rail, "expanded", Value::Bool(true));
    check(&mut win, "expanded by the menu button".into(), 30);
    assert!(seen.is_empty(), "{}", seen.join("\n"));
}

#[test]
fn rail_stays_beside_the_content_ru() {
    rail_stays_beside_the_content(
        concat!(module_path!(), "::rail_stays_beside_the_content_ru"),
        &[("LANG", "ru_RU.UTF-8"), ("LC_ALL", "ru_RU.UTF-8")],
    );
}

#[test]
fn rail_stays_beside_the_content_en() {
    rail_stays_beside_the_content(
        concat!(module_path!(), "::rail_stays_beside_the_content_en"),
        &[("LANG", "en_US.UTF-8"), ("LC_ALL", "en_US.UTF-8")],
    );
}

/// Every visible item of the shown Quick page whose horizontal extent leaves the page's viewport
/// or the card or row it sits on, as "type text [x, y, w, h]". Inside an item that clips (a
/// tile's picture) only that item is checked. Icon glyphs are left out as in `rail_intrusions`.
fn quick_page_overflows(win: &Win) -> Vec<String> {
    let loader = win.find("Quick page loader", |w, i| {
        has(w, i, "pageIndex") && w.read(i, "pageIndex").unwrap().to_number() == 0.0
    });
    let mut w = win.rt.world.borrow_mut();
    let page = scene::visual_kids(&mut w, loader)[0];
    let Some(view) = window_rect(&mut w, win.root, page) else {
        return vec!["the Quick page is not shown".into()];
    };
    // Not `window_rect`: an item cut away entirely by the viewport must still be checked.
    let hidden = |w: &mut World, i: usize| {
        let mut c = i;
        while c != win.root {
            if matches!(w.read(c, "visible"), Some(Value::Bool(false)))
                || w.read(c, "opacity").unwrap_or_default().to_number() == 0.0
            {
                return true;
            }
            c = w.items[c].parent.unwrap();
        }
        false
    };
    let raw = |w: &mut World, i: usize| {
        let mut x = 0.0;
        let mut c = i;
        while c != win.root {
            x += w.read(c, "x").unwrap_or_default().to_number();
            c = w.items[c].parent.unwrap();
        }
        [x, x + w.read(i, "width").unwrap_or_default().to_number()]
    };
    let mut out = vec![];
    let mut todo = vec![(page, [view[0], view[0] + view[2]])];
    while let Some((i, bounds)) = todo.pop() {
        if !w.items[i].inherits("Item")
            || is(&w, i, "MaterialSymbol")
            || is(&w, i, "ToolTip")
            || hidden(&mut w, i)
        {
            continue;
        }
        let [l, r] = raw(&mut w, i);
        if i != page && (l < bounds[0] - 0.5 || r > bounds[1] + 0.5) {
            let text = w
                .read(i, "text")
                .map(|t| t.to_js_string())
                .unwrap_or_default();
            out.push(format!(
                "{} {text:?} [{l}, {r}] outside {bounds:?}",
                w.items[i].types[0]
            ));
        }
        if i != page && w.read(i, "clip").is_some_and(|v| v.truthy()) {
            continue;
        }
        let inner = if is(&w, i, "SettingsCard") || is(&w, i, "SettingsRow") {
            [l.max(bounds[0]), r.min(bounds[1])]
        } else {
            bounds
        };
        for k in scene::visual_kids(&mut w, i) {
            todo.push((k, inner));
        }
    }
    out
}

/// The Quick page from the narrowest settings window (800 px) to a wide one, every frame while
/// the rail collapses and expands: no tile, label or control is cut by the page's edge or runs
/// off its card. The theme tiles sat beside the 268 px wallpaper at every width, so below about
/// 900 px the "Auto" tile was cut at the page's right edge.
fn quick_page_fits(test: &str, vars: &[(&'static str, &str)]) {
    let Some(mut win) = settings(test, vars) else {
        return;
    };
    let mut seen = vec![];
    let mut widths: Vec<f64> = (800..=1440).step_by(20).map(f64::from).collect();
    widths.extend([901.0, 899.0, 801.0, 1120.0]);
    for width in widths {
        win.size = [width, SIZE[1]];
        for f in 0..20 {
            win.frame();
            seen.extend(
                quick_page_overflows(&win)
                    .into_iter()
                    .map(|s| format!("{width} px frame {f}: {s}")),
            );
        }
    }
    seen.truncate(40);
    assert!(seen.is_empty(), "{}", seen.join("\n"));
}

#[test]
fn quick_page_fits_ru() {
    quick_page_fits(
        concat!(module_path!(), "::quick_page_fits_ru"),
        &[("LANG", "ru_RU.UTF-8"), ("LC_ALL", "ru_RU.UTF-8")],
    );
}

#[test]
fn quick_page_fits_en() {
    quick_page_fits(
        concat!(module_path!(), "::quick_page_fits_en"),
        &[("LANG", "en_US.UTF-8"), ("LC_ALL", "en_US.UTF-8")],
    );
}

/// `i`'s top-left corner in window coordinates, unclipped.
fn window_pos(w: &mut World, root: usize, i: usize) -> [f64; 2] {
    let (mut x, mut y) = (0.0, 0.0);
    let mut c = i;
    while c != root {
        x += w.read(c, "x").unwrap_or_default().to_number();
        y += w.read(c, "y").unwrap_or_default().to_number();
        c = w.items[c].parent.unwrap();
    }
    [x, y]
}

/// Where the Quick page's theme tiles stand against the wallpaper at window widths 700..1440:
/// beside it (same top, `columnSpacing` to its right, inside the card) exactly when the wallpaper
/// column, the spacing and the tiles fit in the card's content width, else under it (`rowSpacing`
/// below, same left). Returns the widths at which they stood beside it. The grid compared the
/// sum against its own width, which Qt caps at the layout's maximum (one column's width while
/// every child is fixed), so the tiles stayed under the wallpaper at every width.
fn theme_tiles_placement(win: &mut Win) -> Vec<f64> {
    let tile = win.find("Light theme tile", |w, i| {
        is(w, i, "SettingsPreviewTile")
            && w.read(i, "icon")
                .is_some_and(|v| v.to_js_string() == "light_mode")
    });
    let tiles = win.parent(tile);
    let theme = win.parent(tiles);
    let grid = win.parent(theme);
    let inner = win.parent(grid);
    let wallpaper = win.rt.world.borrow().items[grid].children[0];
    let mut widths: Vec<f64> = (700..=1440).step_by(20).map(f64::from).collect();
    widths.extend([860.0, 1120.0]);
    let mut beside_at = vec![];
    let mut wrong = vec![];
    for width in widths {
        win.size = [width, SIZE[1]];
        win.frames(10);
        let mut w = win.rt.world.borrow_mut();
        let num = |w: &mut World, i: usize, k: &str| w.read(i, k).unwrap_or_default().to_number();
        let spacing = num(&mut w, grid, "columnSpacing");
        let need =
            num(&mut w, wallpaper, "implicitWidth") + spacing + num(&mut w, tiles, "implicitWidth");
        let room = num(&mut w, inner, "width");
        let [cx, _] = window_pos(&mut w, win.root, inner);
        let [wx, wy] = window_pos(&mut w, win.root, wallpaper);
        let [tx, ty] = window_pos(&mut w, win.root, theme);
        let (ww, wh) = (
            num(&mut w, wallpaper, "width"),
            num(&mut w, wallpaper, "height"),
        );
        let tw = num(&mut w, theme, "width");
        let fits = need <= room;
        let beside = (tx - (wx + ww + spacing)).abs() < 0.5
            && (ty - wy).abs() < 0.5
            && tx + tw <= cx + room + 0.5;
        let under =
            (tx - wx).abs() < 0.5 && (ty - (wy + wh + num(&mut w, grid, "rowSpacing"))).abs() < 0.5;
        if fits && !beside || !fits && !under {
            wrong.push(format!(
                "{width} px: need {need} of {room}, wallpaper [{wx}, {wy}, {ww}x{wh}], tiles at [{tx}, {ty}] {tw} wide: expected {}",
                if fits { "beside" } else { "under" }
            ));
        }
        if fits {
            beside_at.push(width);
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    beside_at
}

/// Without the random-wallpaper buttons (`policies.weeb` 0) the wallpaper column is the bare
/// thumbnail, so even ri-testenv's fallback fonts (no Material Symbols: icon names lay out as
/// words) leave room for the tiles beside it in a wide window and none in a narrow one: both
/// arrangements are checked.
fn theme_tiles_beside_the_wallpaper_when_they_fit(test: &str, vars: &[(&'static str, &str)]) {
    let Some(mut win) = settings_with(test, r#"{"policies": {"weeb": 0}}"#, vars) else {
        return;
    };
    let beside_at = theme_tiles_placement(&mut win);
    assert!(
        beside_at.contains(&1120.0) && !beside_at.contains(&700.0),
        "fixture: the tiles fit beside the bare wallpaper at 1120 px, not at 700 px (fit at {beside_at:?})"
    );
}

#[test]
fn theme_tiles_beside_the_wallpaper_when_they_fit_ru() {
    theme_tiles_beside_the_wallpaper_when_they_fit(
        concat!(
            module_path!(),
            "::theme_tiles_beside_the_wallpaper_when_they_fit_ru"
        ),
        &[("LANG", "ru_RU.UTF-8"), ("LC_ALL", "ru_RU.UTF-8")],
    );
}

#[test]
fn theme_tiles_beside_the_wallpaper_when_they_fit_en() {
    theme_tiles_beside_the_wallpaper_when_they_fit(
        concat!(
            module_path!(),
            "::theme_tiles_beside_the_wallpaper_when_they_fit_en"
        ),
        &[("LANG", "en_US.UTF-8"), ("LC_ALL", "en_US.UTF-8")],
    );
}

/// The default page, with the Konachan and osu! buttons beside "Change": the tiles go where
/// they fit at every width.
#[test]
fn theme_tiles_follow_the_room_with_the_random_wallpaper_buttons_ru() {
    let test = concat!(
        module_path!(),
        "::theme_tiles_follow_the_room_with_the_random_wallpaper_buttons_ru"
    );
    let Some(mut win) = settings(test, &[("LANG", "ru_RU.UTF-8"), ("LC_ALL", "ru_RU.UTF-8")])
    else {
        return;
    };
    theme_tiles_placement(&mut win);
}
