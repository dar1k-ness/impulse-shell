//! `positionViewAtIndex` modes, on rows that are not realized yet too, and the view following
//! `currentIndex` (QQuickItemViewPrivate::positionViewAtIndex, QQuickItemView::
//! trackedPositionChanged). The ListView numbers are what Qt 6.12 printed for the audit's
//! `flick/l6`: 40 rows of 30 with 4 between in a 200x200 view.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::scene;

struct View {
    rt: Runtime,
    fonts: scene::Fonts,
    win: usize,
    view: usize,
    t: f64,
}

impl View {
    fn new(view: &str) -> View {
        let src = format!(
            r#"import QtQuick
import Rustshell
FloatingWindow {{
    width: 200; height: 200
    property var view: f
    property string log: ""
    function at(i, mode) {{ f.positionViewAtIndex(i, mode); log += f.contentY + " " }}
    function end() {{ f.positionViewAtEnd(); log += f.contentY + " " }}
    function beginning() {{ f.positionViewAtBeginning(); log += f.contentY + " " }}
    function current(i) {{ f.currentIndex = i; log += f.contentY + " " }}
    {view}
}}
"#
        );
        let mut rt = Runtime::new(None);
        let win = rt.load_str(&src).unwrap();
        let mut v = View {
            rt,
            fonts: scene::Fonts::default(),
            win,
            view: 0,
            t: 1000.0,
        };
        v.frame();
        let Some(Value::Item(view)) = v.rt.world.borrow_mut().read(win, "view") else {
            panic!("no view");
        };
        v.view = view;
        v.rt.tick(v.t);
        v.frame();
        v
    }

    fn frame(&mut self) {
        for _ in 0..4 {
            self.rt.settle();
            scene::layout_window(&mut self.rt, &mut self.fonts, self.win, [200.0, 200.0]);
        }
    }

    fn call(&mut self, f: &str, args: &[f64]) {
        let args = args.iter().map(|&a| Value::Num(a)).collect();
        let _ = self.rt.call_method(self.win, f, args).unwrap();
    }

    fn y(&mut self) -> f64 {
        self.rt.get(self.view, "contentY").to_number()
    }

    fn log(&mut self) -> String {
        self.rt.get(self.win, "log").to_js_string()
    }

    /// 16 ms frames until nothing moves; the positions the view passed through.
    fn rest(&mut self) -> Vec<f64> {
        let mut seen = Vec::new();
        for _ in 0..1000 {
            self.t += 16.0;
            let busy = self.rt.tick(self.t);
            self.frame();
            seen.push(self.y());
            if !busy {
                assert!(self.rt.errors().is_empty(), "{:?}", self.rt.errors());
                return seen;
            }
        }
        panic!("the view never came to rest");
    }
}

const LIST: &str = "ListView { id: f; width: 200; height: 200; model: 40; spacing: 4
    delegate: Rectangle { width: 200; height: 30 } }";

const BEGINNING: f64 = 0.0;
const CENTER: f64 = 1.0;
const END: f64 = 2.0;
const VISIBLE: f64 = 3.0;
const CONTAIN: f64 = 4.0;

#[test]
fn each_mode_puts_the_view_where_qt_does() {
    let mut v = View::new(LIST);
    v.call("at", &[20.0, CENTER]);
    v.call("at", &[39.0, BEGINNING]);
    v.call("at", &[5.0, END]);
    v.call("at", &[10.0, CONTAIN]);
    v.call("at", &[12.0, VISIBLE]);
    v.call("end", &[]);
    v.call("beginning", &[]);
    assert_eq!(v.log(), "595 1156 0 170 238 1156 0 ");
    // Nothing takes the view back to the end it was sent to before.
    v.rest();
    assert_eq!(v.y(), 0.0);
}

#[test]
fn a_row_that_is_already_in_sight_stays_put() {
    let mut v = View::new(LIST);
    v.call("at", &[3.0, VISIBLE]);
    v.call("at", &[3.0, CONTAIN]);
    v.call("at", &[-7.0, CENTER]);
    v.call("at", &[400.0, END]);
    v.call("at", &[3.0, 9.0]);
    assert_eq!(v.log(), "0 0 0 1156 1156 ");
}

#[test]
fn an_unrealized_row_is_reached_once_it_is_laid_out() {
    // 20 rows of 30, then 80 of 50. The view starts with the 18 rows that begin inside its
    // cache buffer (540 px, 30 each), so the call aims row 80 at 540 + 62 * 30 and stays
    // there: the row is made at that place and `originY` moves instead. Qt 6.12's answer to
    // this scene is the first line of scenes/qt_view_content/position_unrealized.qml
    // (`call y 2400 | later y 2400 oy -1600 ch 5000 | rest y 2400 oy -1600 ch 5000`), which
    // `qt_view_content` compares whole.
    let mut v = View::new(
        "ListView { id: f; width: 200; height: 200; model: 100
    delegate: Rectangle { required property int index; width: 200; height: index < 20 ? 30 : 50 } }",
    );
    v.call("at", &[80.0, BEGINNING]);
    assert_eq!(v.log(), "2400 ");
    v.rest();
    assert_eq!(v.y(), 2400.0);
}

#[test]
fn the_view_runs_after_the_current_item() {
    let mut v = View::new(LIST);
    v.call("current", &[30.0]);
    // Nothing moves inside the assignment; the highlight then runs at 400 px/s and the view
    // keeps it in sight, until row 30 (at 1020, 30 tall) is the last one shown.
    assert_eq!(v.log(), "0 ");
    v.frame();
    let seen = v.rest();
    assert_eq!(v.y(), 850.0);
    assert!(seen.windows(2).all(|p| p[0] <= p[1]), "{seen:?}");
    let moving = seen.iter().filter(|&&y| y > 0.0 && y < 850.0).count();
    assert!(moving > 100, "the view took {moving} frames to get there");
    // Back up: the view stops as soon as the row is in sight again.
    v.call("current", &[2.0]);
    v.frame();
    v.rest();
    assert_eq!(v.y(), 68.0);
}

#[test]
fn the_highlight_starts_over_when_the_cache_buffer_fills() {
    // QQuickItemViewPrivate::refill adds the rows in sight first and those of cacheBuffer one
    // tick later; rows added while the current one is not held start the highlight's move over
    // from where it is (QQuickListViewPrivate::visibleItemsChanged). The numbers are frames of
    // Qt 6.12 run with a fixed 16 ms animation step on the audit's `flick/l6`.
    let mut v = View::new(LIST);
    v.call("end", &[]);
    v.call("beginning", &[]);
    v.call("current", &[30.0]);
    v.frame();
    let seen = v.rest();
    let at = |frame: usize| format!("{:.2}", seen[frame - 1]);
    let got: Vec<String> = [46, 47, 48, 60, 100, 140].iter().map(|&f| at(f)).collect();
    assert_eq!(got, ["0.00", "5.30", "12.84", "113.29", "565.24", "838.93"]);
    assert_eq!(v.y(), 850.0);
}

#[test]
fn a_view_that_does_not_follow_stays() {
    let mut v = View::new(
        "ListView { id: f; width: 200; height: 200; model: 40; spacing: 4
    highlightFollowsCurrentItem: false
    delegate: Rectangle { width: 200; height: 30 } }",
    );
    v.call("current", &[30.0]);
    v.frame();
    v.rest();
    assert_eq!(v.y(), 0.0);
}

#[test]
fn a_grid_view_positions_by_row() {
    // 4 columns of 50: index 50 is in row 12 (at 600), index 99 in the last row, 24.
    let mut v = View::new(
        "GridView { id: f; width: 200; height: 200; model: 100; cellWidth: 50; cellHeight: 50
    delegate: Rectangle { width: 50; height: 50 } }",
    );
    v.call("at", &[50.0, BEGINNING]);
    v.call("at", &[50.0, CENTER]);
    v.call("at", &[99.0, END]);
    // Row 20 ends where the view starts: not in sight, so the view goes up to it.
    v.call("at", &[80.0, VISIBLE]);
    v.call("at", &[40.0, CONTAIN]);
    v.call("at", &[42.0, CONTAIN]);
    v.call("at", &[60.0, CONTAIN]);
    v.call("at", &[0.0, VISIBLE]);
    assert_eq!(v.log(), "600 525 1050 1000 500 500 600 0 ");
}
