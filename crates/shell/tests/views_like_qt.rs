//! Views over changing models, line for line as Qt 6.12 and Quickshell 0.3.2 print them. Each
//! `views_like_qt/NAME.qml` is run on 16 ms frames and its `console.log` lines and the
//! warnings of its objects (`QML ListModel: move: out of range`) are compared with `NAME.txt`,
//! the output of the real thing for the same file:
//!
//! - `ord1`, `cur2` (Qt 6.12, no window): the order in which a delegate gets `modelData`, its
//!   roles, `index` and `model`; `currentIndex` 0 when a model fills.
//! - `ord2` (Qt 6.12, no window): a delegate of a row that is an object gets `modelData`,
//!   `index` and `model`, then the object's properties in the order the object declares them.
//! - `sm2` (Quickshell 0.3.2), `cur1`, `sec` (Qt 6.12), all in no window: such a view is never
//!   polished (QQuickItem::polish() queues only an item that has a window), so it applies row
//!   changes only in a call that needs its rows or when it scrolls.
//! - `sm7w`, `sm2w`, `cur1w`, `secw` (Quickshell 0.3.2, in a FloatingWindow): several views
//!   over one ScriptModel take each row change newest view first; a view applies row changes
//!   at polish or in the call that needs its rows; the current row, `currentItem`, the rows
//!   released and `count` are told in QQuickItemView's order; a section header is created
//!   right after its first row.
//! - `mm1w`, `mm2w` (Quickshell 0.3.2, in a FloatingWindow): several changes of a ScriptModel
//!   and a ListModel in one frame (fill, remove the first row, insert at 0, move, clear) at a
//!   `ListView`, a `GridView` and a `Repeater`: `currentIndex`, `currentItem`, `count`,
//!   `ListView.onAdd` / `onRemove`, `itemAdded` / `itemRemoved` and, in `mm2w`, the `add` and
//!   `remove` transitions.
//! - `pos2w`, `gci1w` (Quickshell 0.3.2, in a FloatingWindow): `positionViewAtIndex` in each of
//!   its six modes at a row before, at the start of, inside, at the end of and after what the
//!   view shows, for `ListView` (plain, with margins, header, footer and spacing, horizontal,
//!   snapping, with a highlight range) and `GridView` (plain, with margins, snapping); `gci1w`
//!   is the wallpaper selector's grid; `mar1w`: a view with margins rests at its beginning and
//!   its end.
//! - `pae1w`, `pae2w`, `pae3w` (Quickshell 0.3.2, in a FloatingWindow): the chat's list, sent
//!   to its end on each new row; a row that grows later does not take the view along.
//! - `nw1`, `oc1` (Qt 6.12, no window), `ftb1w` (Quickshell 0.3.2, in a FloatingWindow): the
//!   differences `docs/qt-differences.md` lists.
//!   Their `.txt` is Qt's output and the test names the lines the engine prints differently.

use qml::runtime::Runtime;
use shell::scene;
use std::cell::RefCell;
use std::path::Path;

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/views_like_qt");

thread_local! {
    static CONSOLE: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

fn console(_level: u8, line: &str) {
    CONSOLE.with(|c| c.borrow_mut().push(line.to_string()));
}

fn said_done() -> bool {
    CONSOLE.with(|c| c.borrow().iter().any(|l| l == "done"))
}

/// The lines `name.qml` logs up to `done`, or in `ms` when it never says so, under a host that
/// paces frames the way the shell does: one polish per frame for each window. `screen` gives
/// the Quickshell files a screen.
fn run(name: &str, screen: bool, ms: f64) -> Vec<String> {
    let _ = qml::builtins::CONSOLE_SINK.set(console);
    CONSOLE.with(|c| c.borrow_mut().clear());
    let mut rt = Runtime::new(Some(DIR.into()));
    if screen {
        rt.set_screens(&[("HEADLESS-1".to_string(), 0, 0, 1280, 720)]);
    }
    rt.world.borrow_mut().frame_paced = true;
    let root = (rt.load_file(&Path::new(DIR).join(format!("{name}.qml")))).unwrap();
    rt.settle();
    rt.run_later();
    let wins: Vec<usize> = {
        let w = rt.world.borrow();
        (0..w.items.len())
            .filter(|&i| w.items[i].types.iter().any(|t| t == "FloatingWindow"))
            .collect()
    };
    rt.set_polish_wins(wins.iter().copied());
    let size = |rt: &Runtime, p: &str| {
        let v = (rt.world.borrow_mut().read(root, p)).map_or(f64::NAN, |v| v.to_number());
        if v > 0.0 { v } else { 400.0 }
    };
    let mut fonts = scene::Fonts::default();
    let mut t = 0.0;
    while t <= ms && !said_done() {
        rt.tick(t);
        rt.settle();
        rt.run_later();
        for &win in &wins {
            if rt.held_windows().contains(&win) {
                rt.polish(&[win]);
            }
        }
        let at = [size(&rt, "width"), size(&rt, "height")];
        scene::layout_window(&mut rt, &mut fonts, root, at);
        rt.settle();
        t += 16.0;
    }
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    CONSOLE.with(|c| c.borrow_mut().split_off(0))
}

/// Fails with the lines around the first one that differs from `name.txt`.
fn same(name: &str, got: &[String]) {
    let want = std::fs::read_to_string(format!("{DIR}/{name}.txt")).unwrap();
    let want: Vec<&str> = want.lines().collect();
    let at = (0..got.len().max(want.len()))
        .find(|&k| got.get(k).map(|s| s.as_str()) != want.get(k).copied());
    if let Some(at) = at {
        let from = at.saturating_sub(4);
        let got: Vec<&str> = got.iter().map(|s| s.as_str()).collect();
        let show = |l: &[&str]| l[from.min(l.len())..(at + 6).min(l.len())].join("\n");
        panic!(
            "{name}: line {} differs\n--- got\n{}\n--- want\n{}",
            at + 1,
            show(&got),
            show(&want)
        );
    }
}

#[test]
fn a_delegate_gets_model_data_roles_index_and_model_in_qt_order() {
    same("ord1", &run("ord1", false, 700.0));
}

#[test]
fn a_delegate_of_a_row_object_gets_its_properties_last() {
    same("ord2", &run("ord2", false, 700.0));
}

#[test]
fn a_model_that_fills_makes_row_0_current() {
    same("cur2", &run("cur2", false, 700.0));
}

#[test]
fn a_view_tells_row_changes_in_item_view_order() {
    same("sm7w", &run("sm7w", true, 4000.0));
}

#[test]
fn views_of_one_script_model_take_each_change_newest_first() {
    same("sm2w", &run("sm2w", true, 4000.0));
}

#[test]
fn row_changes_wait_for_polish_or_the_call_that_needs_rows() {
    same("cur1w", &run("cur1w", true, 4000.0));
}

#[test]
fn section_headers_follow_their_first_rows() {
    same("secw", &run("secw", true, 4000.0));
}

#[test]
fn views_in_no_window_hold_script_model_changes() {
    same("sm2", &run("sm2", true, 4000.0));
}

#[test]
fn a_view_in_no_window_applies_row_changes_when_asked_for_rows() {
    same("cur1", &run("cur1", false, 3000.0));
}

#[test]
fn section_headers_in_no_window() {
    same("sec", &run("sec", false, 3000.0));
}

#[test]
fn several_model_changes_in_one_frame_are_told_one_by_one() {
    same("mm1w", &run("mm1w", true, 25000.0));
}

#[test]
fn add_and_remove_transitions_of_several_changes_in_one_frame() {
    same("mm2w", &run("mm2w", true, 25000.0));
}

#[test]
fn position_view_at_index_in_every_mode() {
    same("pos2w", &run("pos2w", true, 25000.0));
}

#[test]
fn margins_keep_out_of_sight_at_the_beginning_and_the_end() {
    same("mar1w", &run("mar1w", true, 4000.0));
}

#[test]
fn the_wallpaper_grid_keeps_the_chosen_row_in_sight() {
    same("gci1w", &run("gci1w", true, 2000.0));
}

/// The lines of `name.txt` (what Qt printed) next to the ones the engine prints.
fn qt_and_ours(name: &str, screen: bool) -> (Vec<String>, Vec<String>) {
    let qt = std::fs::read_to_string(format!("{DIR}/{name}.txt")).unwrap();
    let ours = run(name, screen, 1000.0);
    (qt.lines().map(str::to_string).collect(), ours)
}

/// docs/qt-differences.md, "A view in no window": Qt 6.12 makes 6 rows, the engine 22.
#[test]
fn a_view_in_no_window_makes_more_rows_than_qt() {
    let (qt, ours) = qt_and_ours("nw1", false);
    assert_eq!(qt, ["plain rows 6 of 40", "parted rows 6 of 40", "done"]);
    assert_eq!(
        ours,
        ["plain rows 22 of 40", "parted rows 22 of 40", "done"]
    );
}

/// docs/qt-differences.md, "A view read in Component.onCompleted": Qt 6.12 has laid the view
/// out by then, the engine lays it out right after.
#[test]
fn a_view_in_no_window_is_laid_out_after_component_on_completed() {
    let (qt, ours) = qt_and_ours("oc1", false);
    assert_eq!(
        qt,
        [
            "completed -60 1200 40 0 true",
            "later -60 1200 40 0 true",
            "done"
        ]
    );
    assert_eq!(ours[1..], qt[1..]);
    assert_eq!(ours[0], "completed 0 0 40 0 true");
}

/// docs/qt-differences.md, "GridView.flow: GridView.FlowTopToBottom": Qt 6.12 fills columns
/// and scrolls along x, the engine fills rows and scrolls along y.
#[test]
fn a_grid_that_flows_top_to_bottom_is_laid_out_in_rows() {
    let (qt, ours) = qt_and_ours("ftb1w", true);
    assert_eq!(
        qt,
        [
            "cell 1 at 0,40 cell 2 at 50,0",
            "content 1000 -1",
            "cell 30 at the beginning 750 0",
            "done"
        ]
    );
    assert_eq!(
        ours,
        [
            "cell 1 at 50,0 cell 2 at 100,0",
            "content -1 400",
            "cell 30 at the beginning 0 280",
            "done"
        ]
    );
}

/// The chat's list (`onCountChanged: if (atYEnd) Qt.callLater(positionViewAtEnd)`): the view
/// goes to the end of each new row and stays where it is when the last row grows later.
#[test]
fn a_view_sent_to_its_end_does_not_follow_a_row_that_grows() {
    same("pae2w", &run("pae2w", true, 13000.0));
}

/// The chat's handler as shipped (`if (atYEnd || busy) Qt.callLater(positionViewAtEnd)`): a new
/// message takes the view to the end, an answer written in parts grows below it.
#[test]
fn an_answer_written_in_parts_grows_below_the_view() {
    same("pae3w", &run("pae3w", true, 14000.0));
}

/// The same list with a `bottomMargin`: `positionViewAtEnd` keeps the margin out of sight, so
/// the view is not at its end and the handler stops calling it.
#[test]
fn a_view_sent_to_its_end_keeps_its_bottom_margin_out_of_sight() {
    same("pae1w", &run("pae1w", true, 13000.0));
}
