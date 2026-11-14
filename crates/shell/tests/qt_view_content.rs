//! A Flickable's content item and what stands in it, as Qt 6.12 has them: its geometry, the
//! items parented to it, `childrenRect`, `mapToItem`, `childAt`, clicks on scrolled content and
//! which delegates a ListView keeps. Each case is a scene in `scenes/qt_view_content/NAME.qml`
//! that leaves its findings in the root's `result`; its first line, `// qt: ...`, is what real
//! Qt left there (`scenes/qt_view_content/qt.py`, run in the testenv image).
//!
//! The scene runs for a second of frames, laid out on each. On frame 20 every point of the
//! root's `clicks` gets a left click, as under `qt.py`.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::input::Pointer;
use shell::scene;

const BTN_LEFT: u32 = 0x110;

fn clicks(rt: &mut Runtime, root: usize) -> Vec<[f64; 2]> {
    let list = |v: &Value| match v {
        Value::Obj(o) => o.borrow().array.clone(),
        _ => None,
    };
    let points = list(&rt.get(root, "clicks")).unwrap_or_default();
    points
        .iter()
        .map(|p| match list(p).as_deref() {
            Some([x, y]) => [x.to_number(), y.to_number()],
            _ => panic!("a click is [x, y], not {p:?}"),
        })
        .collect()
}

fn ours(name: &str) -> String {
    let (mut rt, root) = testscenes::load(&format!("qt_view_content/{name}.qml"));
    rt.settle();
    let size = [rt.get(root, "width"), rt.get(root, "height")].map(|v| v.to_number());
    let fonts = std::rc::Rc::new(std::cell::RefCell::new(scene::Fonts::default()));
    let mut ptr = Pointer::default();
    ptr.fonts = fonts.clone();
    let mut now = 0.0;
    rt.tick(now);
    for frame in 0..64 {
        scene::layout_window(&mut rt, &mut fonts.borrow_mut(), root, size);
        if frame == 20 {
            ptr.enter(&mut rt, root, 1.0, 1.0);
            for [x, y] in clicks(&mut rt, root) {
                ptr.motion(&mut rt, root, x, y);
                ptr.press(&mut rt, BTN_LEFT);
                ptr.release(&mut rt, BTN_LEFT);
                rt.settle();
            }
        }
        now += 16.0;
        rt.tick(now);
    }
    let result = rt.get(root, "result").to_js_string();
    let errors = rt.errors();
    assert!(errors.is_empty(), "{name}: {result}\n{errors:?}");
    result
}

fn check(name: &str, src: &str) {
    let qt = src
        .lines()
        .find_map(|l| l.strip_prefix("// qt: "))
        .expect("the scene's `// qt:` line");
    let ours = ours(name);
    assert!(ours == qt, "{name}:\n    Qt {qt}\n  ours {ours}");
}

macro_rules! case {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[test]
        fn $name() {
            check(
                stringify!($name),
                include_str!(concat!("scenes/qt_view_content/", stringify!($name), ".qml")),
            );
        }
    };
}

case!(
    /// A Flickable's declared items stand in its content item, which moves by `contentX` /
    /// `contentY`; with no content size set it has the view's size.
    content_flick
);
case!(
    /// A ListView's delegates, `header` and `footer` stand in the content item, which starts
    /// at `originY` above the first row.
    content_list
);
case!(
    /// The same for a GridView.
    content_grid
);
case!(
    /// `contentY` below 0: the content item stands below the view's top edge.
    content_overscroll
);
case!(
    /// Clicks reach scrolled rows and children, also the part of one drawn outside a view
    /// that does not clip; a row kept only for `cacheBuffer` takes none.
    content_click
);
case!(
    /// A ListView makes the rows in view, then those within `cacheBuffer` (320); it destroys
    /// the ones scrolled out of both, and keeps the current row.
    created
);
case!(
    /// `cacheBuffer: 0`: the rows in view only.
    created_cache0
);
case!(
    /// `cacheBuffer: 80`: two rows more on each side.
    created_cache80
);
case!(
    /// `displayMarginBeginning` and `displayMarginEnd` widen what counts as in view.
    created_margin
);
case!(
    /// `reuseItems`: rows scrolled out go to the pool and come back; none is made or destroyed.
    created_reuse
);
case!(
    /// Scrolled back, the rows before the first come again one by one; a jump of more than a
    /// page lets all the rows go and makes those the average size puts there.
    created_back
);
case!(
    /// A horizontal ListView with `spacing`.
    created_horizontal
);
case!(
    /// `verticalLayoutDirection: ListView.BottomToTop`: the rows go up from 0.
    created_reversed
);
case!(
    /// Rows of unequal height: rows not made count as the average of the made ones, in
    /// `contentHeight`, `originY` and where a jump lands.
    created_sizes
);
case!(
    /// `currentItem` stays alive outside the rows in view and goes when `currentIndex` moves on.
    created_current
);
case!(
    /// `positionViewAtIndex` and `itemAtIndex` on rows that have no delegate.
    created_position
);
case!(
    /// `positionViewAtIndex` on a row that has no delegate, rows of unequal height: the view
    /// stays where the average size put the row and `originY` moves.
    position_unrealized
);
case!(
    /// `originY` and `contentHeight` in the handler that called `positionViewAtIndex` on a row
    /// without a delegate.
    #[ignore = "known difference (docs/qt-differences.md): originY and contentHeight right \
                after positionViewAtIndex on a row without a delegate are those of before \
                the call until the view is laid out"]
    position_unrealized_origin
);

/// What the handler reads here instead (docs/qt-differences.md).
#[test]
fn position_unrealized_origin_differs_from_qt_as_recorded() {
    assert_eq!(
        ours("position_unrealized_origin"),
        "call y 2400 oy 0 ch 3000"
    );
}
case!(
    /// `reuseItems`: a row scrolled out is pooled (`ListView.onPooled`) and the next row made
    /// takes the oldest pooled delegate (`ListView.onReused`); none is destroyed.
    created_reuse_rows
);
case!(
    /// `reuseItems`, a number as the model: a delegate taken from the pool for another row
    /// reads that row's `index` and `modelData`, by name and through `model`.
    zz_reuse_ctx
);
case!(
    /// The same for an array of objects.
    zz_reuse_arr
);
case!(
    /// The same for the roles of a ListModel, whose rows are pooled as any other.
    zz_reuse_roles
);
case!(
    /// The same for a delegate that declares `index` and `modelData` as required properties.
    zz_reuse_req
);
case!(
    /// Rows put before the rows of a view or taken from there: each delegate that stays is told
    /// its new row, `index` by name and `model.index`, with no `index` property declared.
    zz_reindex_ctx
);
case!(
    /// The same with the current row among the rows shown.
    zz_reindex_cur
);
case!(
    /// The same for a list of objects: `model` and `modelData` are the new row's object, a
    /// property of the object read by its name alone stays the one of the row before.
    zz_reuse_obj
);
case!(
    /// The same list, a property of the rows' objects written before and after the delegates
    /// are taken for other rows: who hears of it as `model.n`, and in which order.
    zz_reuse_obj_order
);
case!(
    /// Rows inserted into and removed from a ListModel while the view is scrolled: the rows in
    /// view stay where they are, their `index` follows the model.
    created_model
);
case!(
    /// `currentIndex` set to a row that has no delegate: the view scrolls until the row is in
    /// view (QQuickItemViewPrivate::updateCurrent, `highlightFollowsCurrentItem`).
    current_unmade
);
case!(
    /// The same in a list made later with its choice bound from outside (SelectionDialog), with
    /// rows that come after the view is complete, and with a highlight that takes 50 ms.
    current_late
);
case!(
    /// A view that gets another height comes back inside its rows: a list as tall as its rows
    /// whose last row is chosen before it has a height (a ComboBox's popup opened again after
    /// a choice), and a view at its end that grows.
    current_resized
);
case!(
    /// A view given another model starts over at its beginning with its first row current
    /// (QQuickItemView::setModel); an array with the same elements is the same model.
    current_model
);
case!(
    /// Rows as tall as the layout in them (no height of their own), scrolled so that one
    /// step lets the first row go and brings one more at the end: one row is made, once.
    created_laid
);
case!(
    /// The rows of `created_laid` (no height of their own, `implicitHeight` bound to the
    /// layout in them), with the heights the rows have: each is as tall as its layout when
    /// the view counts how many it needs.
    laid_implicit
);
case!(
    /// The same with the row's `height` bound to the layout's implicit height.
    laid_height
);
case!(
    /// The same with a row that takes its height from what is in it (a Column), where that item's
    /// implicit height is bound to the layout's in it: nothing of the row itself is bound.
    laid_nested
);

/// `laid_implicit` with every row walked from its QML instead of copied from a creation
/// plan (`RI_DEBUG=noplan`, read once per process): a walked row asks for a layout as it is
/// made, and is as tall as its layout when the view counts how many rows it needs.
#[test]
fn laid_implicit_walked() {
    const TEST: &str = concat!(module_path!(), "::laid_implicit_walked");
    if !testenv::in_own_process(TEST) {
        return testenv::run_in_own_process(TEST, [("RI_DEBUG", "noplan")]);
    }
    assert!(qml::debug::on(qml::debug::Switch::NOPLAN));
    check(
        "laid_implicit",
        include_str!("scenes/qt_view_content/laid_implicit.qml"),
    );
}
