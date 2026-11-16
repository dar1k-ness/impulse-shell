use super::*;
use crate::layout_model::Layout;

const MANIFEST: &str = r#"
[[widget]]
type = "clock"
name = "Clock"
icon = "schedule"

[widget.panel]
component = "C.qml"
size = { min = [40, 20], max = [800, 400], default = [200, 100] }

[widget.desktop]
component = "C.qml"
size = { min = [40, 20], max = [800, 400], default = [200, 100] }

[widget.sidebar]
component = "SidebarC.qml"
size = { min = [1, 1], default = [1, 1] }

[[widget]]
type = "bar-only"
name = "Bar only"
icon = "x"
placements = ["panel"]
component = "C.qml"
size = { min = [10, 10], default = [20, 20] }

[[widget]]
type = "toggle"
name = "Toggle"
icon = "x"
placements = ["sidebar"]
component = "C.qml"
size = { min = [10, 10], default = [56, 56] }
"#;

const LAYOUT: &str = r#"version = 5

[[monitor]]
connector = "DP-1"
primary = true

[[monitor.container]]
id = "bar"
kind = "panel"
edge = "top"
thickness = 40

[[monitor.container.widget]]
id = "a"
type = "bar-only"
zone = "start"

[[monitor.container.widget]]
id = "b"
type = "bar-only"
zone = "start"

[[monitor.container.widget]]
id = "c"
type = "clock"
zone = "center"

[[monitor.container]]
id = "desk"
kind = "desktop"

[[monitor.container.widget]]
id = "d1"
type = "clock"
x = 100
y = 100
w = 200
h = 100

[[monitor.container.widget]]
id = "d2"
type = "clock"
x = 1000
y = 600
w = 200
h = 100

[[monitor.container]]
id = "side"
kind = "sidebar"
side = "right"
columns = 2

[[monitor.container.widget]]
id = "t1"
type = "toggle"
size = 1

[[monitor.container.widget]]
id = "t2"
type = "toggle"
size = 1
"#;

fn registry() -> Registry {
    let mut reg = Registry::default();
    let errors = reg.load_manifest(MANIFEST, std::path::Path::new("/nonexistent"));
    assert!(errors.is_empty(), "{errors:?}");
    reg
}

fn layout() -> Layout {
    Layout::parse(LAYOUT).unwrap()
}

fn r(x: f64, y: f64, w: f64, h: f64) -> Rect {
    Rect { x, y, w, h }
}

fn frame(id: &str, ty: &str, container: &str, kind: Kind, rect: Rect) -> Frame {
    Frame {
        id: id.into(),
        ty: ty.into(),
        container: container.into(),
        kind,
        rect,
        resizable: true,
        least: 0.0,
        clear: 0.0,
    }
}

fn scene<'a>(l: &'a Layout, reg: &'a Registry) -> Scene<'a> {
    Scene {
        layout: l,
        monitor: 0,
        registry: reg,
        natural: HashMap::new(),
        placed: HashMap::new(),
        frames: vec![
            frame(
                "a",
                "bar-only",
                "bar",
                Kind::Panel,
                r(10.0, 0.0, 30.0, 40.0),
            ),
            frame(
                "b",
                "bar-only",
                "bar",
                Kind::Panel,
                r(50.0, 0.0, 30.0, 40.0),
            ),
            frame("c", "clock", "bar", Kind::Panel, r(900.0, 0.0, 120.0, 40.0)),
            frame(
                "d1",
                "clock",
                "desk",
                Kind::Desktop,
                r(100.0, 100.0, 200.0, 100.0),
            ),
            frame(
                "d2",
                "clock",
                "desk",
                Kind::Desktop,
                r(1000.0, 600.0, 200.0, 100.0),
            ),
            frame(
                "t1",
                "toggle",
                "side",
                Kind::Sidebar,
                r(1500.0, 100.0, 56.0, 56.0),
            ),
            frame(
                "t2",
                "toggle",
                "side",
                Kind::Sidebar,
                r(1562.0, 100.0, 56.0, 56.0),
            ),
        ],
        regions: vec![
            Region {
                container: "bar".into(),
                kind: Kind::Panel,
                rect: r(0.0, 0.0, 1920.0, 40.0),
                vertical: false,
                grid: 0,
                room: [f64::INFINITY; 3],
                lengths: HashMap::new(),
                sidebar: None,
            },
            Region {
                container: "side".into(),
                kind: Kind::Sidebar,
                rect: r(1480.0, 40.0, 440.0, 1040.0),
                vertical: false,
                grid: 0,
                room: [f64::INFINITY; 3],
                lengths: HashMap::new(),
                sidebar: None,
            },
            Region {
                container: "desk".into(),
                kind: Kind::Desktop,
                rect: r(0.0, 0.0, 1920.0, 1080.0),
                vertical: false,
                grid: 0,
                room: [f64::INFINITY; 3],
                lengths: HashMap::new(),
                sidebar: None,
            },
        ],
        bounds: (1920.0, 1080.0),
    }
}

fn gesture(s: &Scene, id: &str, mode: Mode, x: f64, y: f64) -> Gesture {
    let f = s.frame(id).unwrap();
    Gesture {
        connector: "DP-1".into(),
        id: id.into(),
        mode,
        edges: if mode == Mode::Resize {
            Edges::BOTTOM_RIGHT
        } else {
            Edges::default()
        },
        press: (x, y),
        start: f.rect,
        kind: f.kind,
        dragging: true,
        preview: None,
        shown: None,
        gain: None,
    }
}

fn drag(
    s: &Scene,
    id: &str,
    mode: Mode,
    from: (f64, f64),
    to: (f64, f64),
) -> (Preview, Option<Edit>) {
    let mut g = gesture(s, id, mode, from.0, from.1);
    let p = s.preview(&g, to.0, to.1);
    g.preview = Some(p.clone());
    (p, s.commit(&g).and_then(|e| e.into_iter().next()))
}

#[test]
fn hit_prefers_surfaces_above_the_desktop_and_smaller_frames() {
    let mut frames = vec![
        frame(
            "desk",
            "clock",
            "desk",
            Kind::Desktop,
            r(0.0, 0.0, 100.0, 100.0),
        ),
        frame(
            "panel",
            "clock",
            "bar",
            Kind::Panel,
            r(0.0, 0.0, 200.0, 40.0),
        ),
    ];
    assert_eq!(hit(&frames, 10.0, 10.0).unwrap().id, "panel");
    assert_eq!(hit(&frames, 10.0, 60.0).unwrap().id, "desk");
    frames.push(frame(
        "small",
        "clock",
        "bar",
        Kind::Panel,
        r(5.0, 5.0, 20.0, 20.0),
    ));
    assert_eq!(hit(&frames, 10.0, 10.0).unwrap().id, "small");
    assert!(hit(&frames, 500.0, 500.0).is_none());
}

/// A resize of `id` by the handle `edges`, pressed at `from` and pulled to `to`.
fn pull(
    s: &Scene,
    id: &str,
    edges: Edges,
    from: (f64, f64),
    to: (f64, f64),
) -> (Gesture, Option<Edit>) {
    let mut g = gesture(s, id, Mode::Resize, from.0, from.1);
    g.edges = edges;
    g.preview = Some(s.preview(&g, to.0, to.1));
    let e = s.commit(&g).and_then(|e| e.into_iter().next());
    (g, e)
}

const LEFT: Edges = Edges {
    left: true,
    right: false,
    top: false,
    bottom: false,
};
const RIGHT: Edges = Edges {
    left: false,
    right: true,
    top: false,
    bottom: false,
};
const TOP_LEFT: Edges = Edges {
    left: true,
    right: false,
    top: true,
    bottom: false,
};

#[test]
fn a_desktop_widget_has_a_handle_on_each_corner_and_side() {
    let f = r(100.0, 100.0, 200.0, 100.0);
    let at = |x, y| handle_at(&f, Kind::Desktop, false, x, y).map(Edges::name);
    assert_eq!(at(295.0, 195.0), Some("bottom-right"));
    assert_eq!(at(302.0, 202.0), Some("bottom-right"));
    assert_eq!(at(103.0, 98.0), Some("top-left"));
    assert_eq!(at(297.0, 104.0), Some("top-right"));
    assert_eq!(at(96.0, 190.0), Some("bottom-left"));
    assert_eq!(at(200.0, 97.0), Some("top"));
    assert_eq!(at(200.0, 204.0), Some("bottom"));
    assert_eq!(at(104.0, 150.0), Some("left"));
    assert_eq!(at(305.0, 150.0), Some("right"));
    // The middle moves the widget, and so does anything further than the grip from it.
    assert_eq!(at(200.0, 150.0), None);
    assert_eq!(at(120.0, 150.0), None);
    assert_eq!(at(320.0, 150.0), None);
}

#[test]
fn a_small_widget_keeps_its_centre_for_moving() {
    // A 28 px panel toggle: a 16 px handle would cover its centre.
    let f = r(100.0, 6.0, 28.0, 28.0);
    let at = |x, y| handle_at(&f, Kind::Desktop, false, x, y).map(Edges::name);
    assert_eq!(at(114.0, 20.0), None);
    assert_eq!(at(125.0, 31.0), Some("bottom-right"));
    assert_eq!(at(101.0, 7.0), Some("top-left"));
}

#[test]
fn panel_and_sidebar_widgets_have_handles_only_along_the_axis_a_resize_changes() {
    let f = r(900.0, 0.0, 120.0, 40.0);
    let name = |kind, vertical, x, y| handle_at(&f, kind, vertical, x, y).map(Edges::name);
    assert_eq!(name(Kind::Panel, false, 1018.0, 20.0), Some("right"));
    assert_eq!(name(Kind::Panel, false, 902.0, 2.0), Some("left"));
    assert_eq!(name(Kind::Panel, false, 960.0, 38.0), None);
    assert_eq!(name(Kind::Panel, true, 960.0, 38.0), Some("bottom"));
    assert_eq!(name(Kind::Panel, true, 1018.0, 20.0), None);
    assert_eq!(name(Kind::Sidebar, false, 902.0, 20.0), Some("left"));
    assert_eq!(name(Kind::Sidebar, false, 960.0, 1.0), None);
    // An end is held from as far inside the widget as a desktop widget's corner is, and from
    // no further outside than any edge.
    assert_eq!(name(Kind::Panel, false, 1006.0, 20.0), Some("right"));
    assert_eq!(name(Kind::Panel, false, 914.0, 20.0), Some("left"));
    assert_eq!(name(Kind::Panel, false, 1000.0, 20.0), None);
    assert_eq!(name(Kind::Panel, false, 1030.0, 20.0), None);
    assert_eq!(name(Kind::Panel, true, 960.0, 28.0), Some("bottom"));
    assert_eq!(name(Kind::Sidebar, false, 1006.0, 20.0), Some("right"));
    // A small widget keeps its middle third for moving it.
    let small = r(900.0, 0.0, 30.0, 40.0);
    assert_eq!(handle_at(&small, Kind::Panel, false, 915.0, 20.0), None);
}

#[test]
fn a_press_takes_the_widget_the_pointer_is_on_whichever_one_is_selected() {
    let l = layout();
    let reg = registry();
    let mut s = scene(&l, &reg);
    // A bar's row: `n` right after `b` with no gap, `m` 4 px after it. A desktop widget up
    // against the bar, under the ends of `a` and `b`.
    for (id, ty, container, kind, rect) in [
        (
            "n",
            "bar-only",
            "bar",
            Kind::Panel,
            r(80.0, 0.0, 30.0, 40.0),
        ),
        ("m", "clock", "bar", Kind::Panel, r(114.0, 0.0, 300.0, 40.0)),
        (
            "d0",
            "clock",
            "desk",
            Kind::Desktop,
            r(30.0, 40.0, 200.0, 100.0),
        ),
    ] {
        s.frames.push(frame(id, ty, container, kind, rect));
    }
    let ids: Vec<String> = s.frames.iter().map(|f| f.id.clone()).collect();
    let mut handles = 0;
    for selected in std::iter::once(None).chain(ids.iter().map(|id| Some(id.as_str()))) {
        for y in [0.0, 2.0, 20.0, 39.0, 40.0, 44.0, 47.0, 90.0, 139.0, 145.0] {
            for x in 0..1300 {
                let x = x as f64 * 0.5;
                let on: Vec<&str> = s
                    .frames
                    .iter()
                    .filter(|f| f.rect.contains(x, y))
                    .map(|f| f.id.as_str())
                    .collect();
                let picked = s.pick(selected, x, y);
                let at = format!("at ({x}, {y}) with {selected:?} selected, over {on:?}");
                match picked {
                    Some((f, mode, edges)) => {
                        if on.is_empty() {
                            // Beside every widget only the selected one's handle is taken.
                            assert_eq!(
                                (Some(f.id.as_str()), mode),
                                (selected, Mode::Resize),
                                "{at}"
                            );
                        } else {
                            assert!(on.contains(&f.id.as_str()), "{} taken {at}", f.id);
                        }
                        if mode == Mode::Resize {
                            assert_eq!(Some(f.id.as_str()), selected, "{at}");
                            assert_ne!(edges, Edges::default(), "{at}");
                            handles += 1;
                        }
                    }
                    None => assert!(on.is_empty(), "nothing taken {at}"),
                }
            }
        }
    }
    assert!(handles > 0);
    // The selected widget's handle reaches 8 px out of its frame, but not onto the widget
    // there: the next one in the row, or the desktop widget below the bar.
    let taken = |selected, x, y| {
        s.pick(Some(selected), x, y)
            .map(|(f, mode, _)| (f.id.as_str(), mode))
    };
    assert_eq!(taken("b", 82.0, 20.0), Some(("n", Mode::Move)));
    assert_eq!(taken("n", 78.0, 20.0), Some(("b", Mode::Move)));
    assert_eq!(taken("n", 112.0, 20.0), Some(("n", Mode::Resize)));
    assert_eq!(taken("b", 78.0, 44.0), Some(("d0", Mode::Move)));
    assert_eq!(taken("d0", 60.0, 36.0), Some(("b", Mode::Move)));
    assert_eq!(taken("d0", 45.0, 36.0), Some(("d0", Mode::Resize)));
}

#[test]
fn a_resize_moves_the_edges_of_its_handle_and_keeps_the_others() {
    let l = layout();
    let reg = registry();
    let s = scene(&l, &reg);
    // d1 is 200x100 at (100, 100). Its left side pulled 30 px out: only the width grows, the
    // right edge stays at 300.
    let (g, e) = pull(&s, "d1", LEFT, (100.0, 150.0), (70.0, 190.0));
    assert_eq!(
        g.preview.as_ref().unwrap().ghost,
        r(70.0, 100.0, 230.0, 100.0)
    );
    assert!(matches!(
        e,
        Some(Edit::MoveWidget {
            place: Place::Desktop {
                x: 70,
                y: 100,
                size: Some((230, 100)),
            },
            ..
        })
    ));
    // The top-left corner pushed in: the bottom-right corner stays at (300, 200).
    let (g, _) = pull(&s, "d1", TOP_LEFT, (100.0, 100.0), (150.0, 130.0));
    assert_eq!(
        g.preview.as_ref().unwrap().ghost,
        r(150.0, 130.0, 150.0, 70.0)
    );
    // The right side: the height is left alone whatever the pointer does on that axis.
    let (g, _) = pull(&s, "d1", RIGHT, (300.0, 150.0), (350.0, 400.0));
    assert_eq!(
        g.preview.as_ref().unwrap().ghost,
        r(100.0, 100.0, 250.0, 100.0)
    );
}

#[test]
fn a_resize_stops_at_the_types_limits_and_says_so() {
    let l = layout();
    let reg = registry();
    let s = scene(&l, &reg);
    // The clock is 40x20 at the least and 800x400 at the most.
    let (g, _) = pull(&s, "d1", TOP_LEFT, (100.0, 100.0), (900.0, 900.0));
    assert_eq!(
        g.preview.as_ref().unwrap().ghost,
        r(260.0, 180.0, 40.0, 20.0)
    );
    let hint = s.size_hint(&g).unwrap();
    assert_eq!(hint.size, (40.0, 20.0));
    assert!(hint.smallest && !hint.largest);
    let (g, _) = pull(
        &s,
        "d1",
        Edges::BOTTOM_RIGHT,
        (300.0, 200.0),
        (1900.0, 590.0),
    );
    assert_eq!(
        g.preview.as_ref().unwrap().ghost,
        r(100.0, 100.0, 800.0, 400.0)
    );
    let hint = s.size_hint(&g).unwrap();
    assert!(hint.largest && !hint.smallest);
    let (g, _) = pull(
        &s,
        "d1",
        Edges::BOTTOM_RIGHT,
        (300.0, 200.0),
        (350.0, 230.0),
    );
    let hint = s.size_hint(&g).unwrap();
    assert_eq!(hint.size, (250.0, 130.0));
    assert!(!hint.largest && !hint.smallest);
}

#[test]
fn a_panel_widget_grows_no_longer_than_its_type_and_its_zone_allow() {
    let l = layout();
    let reg = registry();
    let mut s = scene(&l, &reg);
    // The panel's clock is 120 long and at most 800: pulled 2000 px, it stops at 800.
    let (g, e) = pull(&s, "c", RIGHT, (1020.0, 20.0), (3020.0, 20.0));
    assert_eq!(g.preview.as_ref().unwrap().ghost.w, 800.0);
    assert!(matches!(
        e,
        Some(Edit::MoveWidget {
            place: Place::Panel {
                width: Width::Px(800),
                ..
            },
            ..
        })
    ));
    assert!(s.size_hint(&g).unwrap().largest);
    // Its zone, the middle one, has room for 50 px more, half at each end, and the widget
    // grows at the pulled end alone: it stops at 145.
    s.regions[0].room = [0.0, 50.0, 0.0];
    let (g, _) = pull(&s, "c", RIGHT, (1020.0, 20.0), (3020.0, 20.0));
    assert_eq!(g.preview.as_ref().unwrap().ghost.w, 145.0);
    assert!(s.size_hint(&g).unwrap().largest);
    // By its left end: the right end stays at 1020 and the length grows the same.
    let (g, _) = pull(&s, "c", LEFT, (900.0, 20.0), (880.0, 20.0));
    assert_eq!(
        g.preview.as_ref().unwrap().ghost,
        r(880.0, 0.0, 140.0, 40.0)
    );
    let hint = s.size_hint(&g).unwrap();
    assert_eq!(hint.size.0, 140.0);
    assert!(!hint.largest && !hint.smallest);
}

#[test]
fn outlines_of_a_panels_widgets_line_up() {
    let l = layout();
    let reg = registry();
    let mut s = scene(&l, &reg);
    s.frames[0].rect = r(10.4, 7.0, 30.2, 26.0);
    s.frames[2].rect = r(900.0, 2.5, 120.0, 35.0);
    let a = s.outline(&s.frames[0], &s.frames[0].rect);
    let c = s.outline(&s.frames[2], &s.frames[2].rect);
    assert_eq!(a, r(10.0, 3.0, 31.0, 34.0));
    assert_eq!((c.y, c.h), (a.y, a.h));
    // A desktop widget's is its own frame, on whole pixels.
    s.frames[3].rect = r(100.4, 100.6, 200.0, 100.0);
    assert_eq!(
        s.outline(&s.frames[3], &s.frames[3].rect),
        r(100.0, 101.0, 200.0, 100.0)
    );
}

#[test]
fn snapping_moves_edges_onto_nearby_lines_with_guides() {
    let (s, g) = snap_move(
        r(105.0, 300.0, 100.0, 50.0),
        &[100.0],
        &[500.0],
        (1920.0, 1080.0),
    );
    assert_eq!(s, r(100.0, 300.0, 100.0, 50.0));
    assert_eq!(
        g,
        vec![Guide {
            x1: 100.0,
            y1: 0.0,
            x2: 100.0,
            y2: 1080.0
        }]
    );
    // Centre lines snap too.
    let (s, _) = snap_move(r(905.0, 0.0, 100.0, 50.0), &[960.0], &[], (1920.0, 1080.0));
    assert_eq!(s.x, 910.0);
    // Too far: nothing moves.
    let (s, g) = snap_move(
        r(120.0, 300.0, 100.0, 50.0),
        &[100.0],
        &[],
        (1920.0, 1080.0),
    );
    assert_eq!((s.x, g.len()), (120.0, 0));
}

#[test]
fn desktop_move_lands_snapped_and_free() {
    let l = layout();
    let reg = registry();
    let s = scene(&l, &reg);
    // Dropped 4 px right of d2's left edge line: snaps onto x = 1000.
    let (p, e) = drag(&s, "d1", Mode::Move, (150.0, 150.0), (1054.0, 400.0));
    assert!(p.valid);
    assert_eq!(p.ghost, r(1000.0, 350.0, 200.0, 100.0));
    assert!(p.guides.iter().any(|g| g.x1 == 1000.0 && g.x2 == 1000.0));
    assert_eq!(
        e,
        Some(Edit::MoveWidget {
            monitor: 0,
            id: "d1".into(),
            container: "desk".into(),
            place: Place::Desktop {
                x: 1000,
                y: 350,
                size: Some((200, 100))
            },
            index: None,
        })
    );
    // Onto d2: lands beside it, never overlapping.
    let (p, _) = drag(&s, "d1", Mode::Move, (150.0, 150.0), (1100.0, 650.0));
    assert!(p.valid);
    let land = p.ghost.snapped();
    assert!(!land.overlaps(&DRect {
        x: 1000,
        y: 600,
        w: 200,
        h: 100
    }));
}

#[test]
fn desktop_resize_stays_in_limits_and_off_neighbours() {
    let l = layout();
    let reg = registry();
    let s = scene(&l, &reg);
    let (p, e) = drag(&s, "d1", Mode::Resize, (300.0, 200.0), (403.0, 260.0));
    assert!(p.valid);
    assert_eq!(p.ghost, r(100.0, 100.0, 303.0, 160.0));
    assert!(matches!(
        e,
        Some(Edit::MoveWidget {
            place: Place::Desktop {
                size: Some((303, 160)),
                ..
            },
            ..
        })
    ));
    // Shrinking below the type's minimum stops at it.
    let (p, _) = drag(&s, "d1", Mode::Resize, (300.0, 200.0), (100.0, 100.0));
    assert_eq!((p.ghost.w, p.ghost.h), (40.0, 20.0));
}

#[test]
fn panel_drop_picks_zone_and_index_from_neighbours() {
    let l = layout();
    let reg = registry();
    let s = scene(&l, &reg);
    let bar = &s.regions[0];
    // Between a and b: start zone, index 1.
    let (z, i, _) = s.panel_slot(bar, "x", None, 45.0, 20.0);
    assert_eq!((z, i), (Zone::Start, 1));
    // Before c's middle: centre, index 0; past the centre widgets: end zone (empty), index 0.
    let (z, i, _) = s.panel_slot(bar, "x", None, 950.0, 20.0);
    assert_eq!((z, i), (Zone::Center, 0));
    let (z, i, m) = s.panel_slot(bar, "x", None, 1500.0, 20.0);
    assert_eq!((z, i), (Zone::End, 0));
    assert_eq!(m.x, 1917.0);
    // Moving b before a.
    let (p, e) = drag(&s, "b", Mode::Move, (60.0, 20.0), (12.0, 20.0));
    assert!(p.valid && p.marker.is_some());
    assert_eq!(
        e,
        Some(Edit::MoveWidget {
            monitor: 0,
            id: "b".into(),
            container: "bar".into(),
            place: Place::Panel {
                zone: Zone::Start,
                width: Width::Auto
            },
            index: Some(0),
        })
    );
}

/// A widget alone between two separators, picked up and let go over its own place, stays between
/// them (it went on to right before the next drawn widget, past the second separator). Beside
/// its place, the half of the gap the pointer is in says which group it joins.
#[test]
fn a_panel_drop_by_a_separator_joins_the_group_on_the_pointers_side() {
    let l = Layout::parse(
        &LAYOUT
            .replacen(
                "[[monitor.container.widget]]\nid = \"b\"",
                "[[monitor.container.widget]]\nid = \"s1\"\ntype = \"separator\"\nzone = \"start\"\n\n\
                 [[monitor.container.widget]]\nid = \"b\"",
                1,
            )
            .replacen(
                "[[monitor.container.widget]]\nid = \"c\"",
                "[[monitor.container.widget]]\nid = \"s2\"\ntype = \"separator\"\nzone = \"start\"\n\n\
                 [[monitor.container.widget]]\nid = \"e\"\ntype = \"bar-only\"\nzone = \"start\"\n\n\
                 [[monitor.container.widget]]\nid = \"c\"",
                1,
            ),
    )
    .unwrap();
    let reg = registry();
    let mut s = scene(&l, &reg);
    s.frames.push(frame(
        "e",
        "bar-only",
        "bar",
        Kind::Panel,
        r(100.0, 0.0, 30.0, 40.0),
    ));
    let bar = &s.regions[0];
    // The start zone is a, s1, b, s2, e: without b, its own place is the third.
    let own = Some(r(50.0, 0.0, 30.0, 40.0));
    assert_eq!(s.panel_slot(bar, "b", own, 72.0, 20.0).1, 2, "over itself");
    assert_eq!(s.panel_slot(bar, "b", own, 44.0, 20.0).1, 2, "short of a");
    assert_eq!(s.panel_slot(bar, "b", own, 96.0, 20.0).1, 2, "short of e");
    assert_eq!(s.panel_slot(bar, "b", own, 38.0, 20.0).1, 1, "over a's end");
    assert_eq!(s.panel_slot(bar, "b", own, 20.0, 20.0).1, 0, "before a");
    assert_eq!(
        s.panel_slot(bar, "b", own, 104.0, 20.0).1,
        3,
        "over e's start"
    );
    assert_eq!(s.panel_slot(bar, "b", own, 120.0, 20.0).1, 4, "after e");
    let (p, e) = drag(&s, "b", Mode::Move, (65.0, 20.0), (72.0, 20.0));
    assert!(p.valid);
    assert!(
        !matches!(e, Some(Edit::MoveWidget { index: Some(i), .. }) if i != 2),
        "{e:?}"
    );
    // One from elsewhere, between a and b: a's group left of the middle of the gap, b's right
    // of it. The marker stands at the end of the group it joins.
    let (_, i, m) = s.panel_slot(bar, "x", None, 44.0, 20.0);
    assert_eq!((i, m.x), (1, 40.0 - 1.5));
    let (_, i, m) = s.panel_slot(bar, "x", None, 46.0, 20.0);
    assert_eq!((i, m.x), (2, 50.0 - 1.5));
    let (_, i, m) = s.panel_slot(bar, "x", None, 85.0, 20.0);
    assert_eq!((i, m.x), (3, 80.0 - 1.5));
    let (_, i, m) = s.panel_slot(bar, "x", None, 95.0, 20.0);
    assert_eq!((i, m.x), (4, 100.0 - 1.5));
}

/// A long widget held near its end and carried a little stays in its place, with the pointer
/// already over the separator after it (it joined the next group). It goes by its middle: past
/// the separator once its middle is past the separator's. Near the panel's end the pointer
/// takes it to the last place, which its middle cannot reach.
#[test]
fn a_long_panel_widget_held_near_its_end_goes_by_its_middle() {
    let l = Layout::parse(
        &LAYOUT
            .replacen(
                "[[monitor.container.widget]]\nid = \"c\"",
                "[[monitor.container.widget]]\nid = \"s2\"\ntype = \"separator\"\nzone = \"start\"\n\n\
                 [[monitor.container.widget]]\nid = \"e\"\ntype = \"bar-only\"\nzone = \"start\"\n\n\
                 [[monitor.container.widget]]\nid = \"c\"",
                1,
            )
            .replacen(
                "[[monitor.container]]\nid = \"desk\"",
                "[[monitor.container.widget]]\nid = \"z\"\ntype = \"bar-only\"\nzone = \"end\"\n\n\
                 [[monitor.container]]\nid = \"desk\"",
                1,
            ),
    )
    .unwrap();
    let reg = registry();
    let mut s = scene(&l, &reg);
    s.frames[1].rect = r(50.0, 0.0, 400.0, 40.0);
    for (id, ty, rect) in [
        ("s2", "separator", r(456.0, 0.0, 8.0, 40.0)),
        ("e", "bar-only", r(470.0, 0.0, 30.0, 40.0)),
        ("z", "bar-only", r(1880.0, 0.0, 30.0, 40.0)),
    ] {
        s.frames.push(frame(id, ty, "bar", Kind::Panel, rect));
    }
    let landed = |to: f64| {
        let (p, e) = drag(&s, "b", Mode::Move, (425.0, 20.0), (to, 20.0));
        assert!(p.valid);
        match e {
            Some(Edit::MoveWidget {
                place: Place::Panel { zone, .. },
                index: Some(i),
                ..
            }) => (zone, i),
            None => (Zone::Start, 1),
            e => panic!("{e:?}"),
        }
    };
    // The start zone without b is a, s2, e.
    assert_eq!(landed(465.0), (Zone::Start, 1), "carried 40 px");
    assert_eq!(landed(630.0), (Zone::Start, 1), "its middle short of s2's");
    assert_eq!(landed(640.0), (Zone::Start, 2), "its middle past s2's");
    assert_eq!(landed(670.0), (Zone::Start, 3), "its middle past e's");
    assert_eq!(landed(1915.0), (Zone::End, 1), "at the panel's end");
}

/// A widget that draws nothing (weather that is off) has no frame; a drop past it lands where
/// the marker stands, not one place short.
#[test]
fn a_panel_drop_counts_the_widgets_that_draw_nothing() {
    let l = Layout::parse(&LAYOUT.replacen(
        "[[monitor.container.widget]]\nid = \"b\"",
        "[[monitor.container.widget]]\nid = \"hidden\"\ntype = \"bar-only\"\nzone = \"start\"\n\n\
         [[monitor.container.widget]]\nid = \"b\"",
        1,
    ))
    .unwrap();
    let reg = registry();
    let s = scene(&l, &reg);
    let bar = &s.regions[0];
    // The layout's start zone is a, hidden, b; only a and b are drawn.
    assert_eq!(s.panel_slot(bar, "x", None, 5.0, 20.0).1, 0);
    assert_eq!(s.panel_slot(bar, "x", None, 45.0, 20.0).1, 2);
    assert_eq!(s.panel_slot(bar, "x", None, 85.0, 20.0).1, 3);
    let (_, e) = drag(&s, "c", Mode::Move, (950.0, 20.0), (85.0, 20.0));
    let mut ed = crate::layout_model::Editor::new(l.clone());
    assert!(ed.apply(e.unwrap()).unwrap());
    let ids: Vec<&str> = ed.layout().monitors[0].containers[0]
        .widgets
        .iter()
        .map(|w| w.id.as_str())
        .collect();
    assert_eq!(ids, ["a", "hidden", "b", "c"]);
}

#[test]
fn widgets_move_between_panel_desktop_and_sidebar() {
    let l = layout();
    let reg = registry();
    let s = scene(&l, &reg);
    // Panel clock to the desktop: centred on the pointer at its type's default size, and
    // without a size in the layout, so it keeps showing at its natural one.
    let (p, e) = drag(&s, "c", Mode::Move, (960.0, 20.0), (600.0, 500.0));
    assert!(p.valid);
    assert_eq!(p.ghost, r(500.0, 450.0, 200.0, 100.0));
    assert!(
        matches!(&e, Some(Edit::MoveWidget { container, place: Place::Desktop { x: 500, y: 450, size: None }, .. }) if container == "desk")
    );
    let mut l2 = l.clone();
    let mut ed = crate::layout_model::Editor::new(l2.clone());
    assert!(ed.apply(e.unwrap()).unwrap());
    l2 = ed.layout().clone();
    assert_eq!(l2.monitors[0].container("desk").unwrap().widgets.len(), 3);
    // Desktop clock into the sidebar after t1: one cell.
    let (p, e) = drag(&s, "d1", Mode::Move, (150.0, 150.0), (1610.0, 120.0));
    assert!(p.valid);
    assert_eq!(
        e,
        Some(Edit::MoveWidget {
            monitor: 0,
            id: "d1".into(),
            container: "side".into(),
            place: Place::Sidebar { size: 1 },
            index: Some(2),
        })
    );
    // Desktop clock onto the panel's end zone.
    let (_, e) = drag(&s, "d2", Mode::Move, (1050.0, 650.0), (1800.0, 20.0));
    assert!(matches!(
        e,
        Some(Edit::MoveWidget {
            place: Place::Panel {
                zone: Zone::End,
                width: Width::Auto
            },
            ..
        })
    ));
}

#[test]
fn sidebar_markers_lie_across_the_flow() {
    let l = layout();
    let reg = registry();
    let mut s = scene(&l, &reg);
    let side = s.regions[1].clone();
    // t1 and t2 side by side: the pointer's side of t2 decides, the bar stands between them.
    let (i, m) = s.sidebar_slot(&side, "x", 1559.0, 120.0);
    assert_eq!(i, 1);
    assert_eq!(m, r(1557.5, 100.0, 3.0, 56.0));
    // After the last one, still in its row: a bar on its right.
    let (i, m) = s.sidebar_slot(&side, "x", 1610.0, 120.0);
    assert_eq!((i, m), (2, r(1618.0, 100.0, 3.0, 56.0)));
    // Below the row: a line across the whole row under it.
    let (i, m) = s.sidebar_slot(&side, "x", 1600.0, 400.0);
    assert_eq!((i, m), (2, r(1500.0, 157.5, 118.0, 3.0)));

    // A column of full-width widgets: halves top and bottom decide, whatever the x, and the
    // marker is a full-width line in the gap above the widget the drop goes before.
    for f in s.frames.iter_mut() {
        match f.id.as_str() {
            "t1" => f.rect = r(1500.0, 100.0, 400.0, 56.0),
            "t2" => f.rect = r(1500.0, 166.0, 400.0, 56.0),
            _ => {}
        }
    }
    let (i, m) = s.sidebar_slot(&side, "x", 1880.0, 110.0);
    assert_eq!((i, m), (0, r(1500.0, 95.5, 400.0, 3.0)));
    let (i, m) = s.sidebar_slot(&side, "x", 1880.0, 140.0);
    assert_eq!((i, m), (1, r(1500.0, 159.5, 400.0, 3.0)));
    let (i, m) = s.sidebar_slot(&side, "x", 1510.0, 180.0);
    assert_eq!((i, m), (1, r(1500.0, 159.5, 400.0, 3.0)));
    let (i, m) = s.sidebar_slot(&side, "x", 1510.0, 210.0);
    assert_eq!((i, m), (2, r(1500.0, 223.5, 400.0, 3.0)));
    // A drag over the column shows the same marker.
    let (p, e) = drag(&s, "d1", Mode::Move, (150.0, 150.0), (1700.0, 180.0));
    assert!(p.valid);
    assert_eq!(p.marker, Some(r(1500.0, 159.5, 400.0, 3.0)));
    assert!(matches!(e, Some(Edit::MoveWidget { index: Some(1), .. })));
}

#[test]
fn types_only_go_where_they_are_allowed() {
    let l = layout();
    let reg = registry();
    let s = scene(&l, &reg);
    // A panel-only widget dropped on the desktop: invalid, nothing to commit.
    let (p, e) = drag(&s, "a", Mode::Move, (20.0, 20.0), (600.0, 600.0));
    assert!(!p.valid);
    assert!(e.is_none());
    // A sidebar toggle over the panel.
    let (p, e) = drag(&s, "t1", Mode::Move, (1520.0, 120.0), (500.0, 20.0));
    assert!(!p.valid && e.is_none());
}

#[test]
fn sidebar_and_panel_resizes_go_by_cells_and_pixels() {
    let l = layout();
    let reg = registry();
    let s = scene(&l, &reg);
    let (p, e) = drag(&s, "t1", Mode::Resize, (1556.0, 156.0), (1600.0, 156.0));
    assert_eq!(p.ghost.w, 112.0);
    assert!(matches!(
        e,
        Some(Edit::MoveWidget {
            place: Place::Sidebar { size: 2 },
            ..
        })
    ));
    let (_, e) = drag(&s, "c", Mode::Resize, (1020.0, 40.0), (1080.0, 40.0));
    assert!(matches!(
        e,
        Some(Edit::MoveWidget {
            place: Place::Panel {
                zone: Zone::Center,
                width: Width::Px(180)
            },
            index: None,
            ..
        })
    ));
}

#[test]
fn arrow_keys_nudge_desktop_widgets_and_reorder_the_others() {
    let l = layout();
    let reg = registry();
    let s = scene(&l, &reg);
    assert!(matches!(
        s.nudge("d1", 1, 0, 10),
        Some(Edit::MoveWidget {
            place: Place::Desktop { x: 110, y: 100, .. },
            ..
        })
    ));
    assert!(matches!(
        s.nudge("b", -1, 0, 1),
        Some(Edit::MoveWidget { index: Some(0), .. })
    ));
    assert!(s.nudge("a", -1, 0, 1).is_none());
    assert!(matches!(
        s.nudge("t1", 1, 0, 1),
        Some(Edit::MoveWidget { index: Some(1), .. })
    ));
}

#[test]
fn a_desktop_widget_without_a_size_keeps_none_on_move_and_gets_one_on_resize() {
    let mut l = layout();
    for c in &mut l.monitors[0].containers {
        for w in c.widgets.iter_mut().filter(|w| w.id == "d1") {
            w.place = Place::Desktop {
                x: 100,
                y: 100,
                size: None,
            };
        }
    }
    let reg = registry();
    let mut s = scene(&l, &reg);
    s.natural.insert("d1".into(), (200, 100));
    let (p, e) = drag(&s, "d1", Mode::Move, (150.0, 150.0), (450.0, 350.0));
    assert!(p.valid);
    assert!(matches!(
        e,
        Some(Edit::MoveWidget {
            place: Place::Desktop { size: None, .. },
            ..
        })
    ));
    let (p, e) = drag(&s, "d1", Mode::Resize, (300.0, 200.0), (403.0, 260.0));
    assert_eq!(p.ghost, r(100.0, 100.0, 303.0, 160.0));
    assert!(matches!(
        e,
        Some(Edit::MoveWidget {
            place: Place::Desktop {
                size: Some((303, 160)),
                ..
            },
            ..
        })
    ));
}

#[test]
fn a_refused_drop_puts_a_fixed_badge_beside_the_pointer_clear_of_it() {
    let l = layout();
    let reg = registry();
    let s = scene(&l, &reg);
    // Whatever the ghost's size (a thin bar slot or a desktop box), the badge is the same
    // size, below and right of the pointer and outside a 32 px cursor around its hotspot.
    let cursor = |x: f64, y: f64| r(x - BADGE, y - BADGE, 2.0 * BADGE, 2.0 * BADGE);
    let apart = |a: Rect, b: Rect| {
        a.right() <= b.x || b.right() <= a.x || a.bottom() <= b.y || b.bottom() <= a.y
    };
    for (id, from, to) in [
        ("t1", (1520.0, 120.0), (500.0, 20.0)),
        ("a", (20.0, 20.0), (600.0, 600.0)),
    ] {
        let (p, e) = drag(&s, id, Mode::Move, from, to);
        assert!(!p.valid && e.is_none());
        let b = p.refusal.unwrap();
        assert_eq!((b.w, b.h), (BADGE, BADGE));
        assert!(b.x > to.0 && b.y > to.1, "{b:?}");
        assert!(apart(b, cursor(to.0, to.1)), "{b:?}");
    }
    // At the screen's right and bottom ends it goes to the other side, still on screen.
    let (p, _) = drag(&s, "a", Mode::Move, (20.0, 20.0), (1910.0, 1075.0));
    let b = p.refusal.unwrap();
    assert!(
        b.right() < 1910.0 && b.bottom() < 1075.0 && b.x >= 0.0 && b.y >= 0.0,
        "{b:?}"
    );
    assert!(apart(b, cursor(1910.0, 1075.0)), "{b:?}");
    // An accepted drag has none.
    let (p, _) = drag(&s, "d1", Mode::Move, (150.0, 150.0), (450.0, 350.0));
    assert!(p.valid && p.refusal.is_none());
}

#[test]
fn a_new_widget_goes_to_the_free_middle_of_the_desktop() {
    let l = layout();
    let reg = registry();
    let s = scene(&l, &reg);
    let (edit, id) = s.add("clock", Kind::Desktop).unwrap();
    assert_eq!(id, "clock-1");
    let Edit::AddWidget {
        container,
        widget,
        index,
        ..
    } = edit
    else {
        panic!("not an add");
    };
    assert_eq!((container.as_str(), index), ("desk", None));
    // Its default 200x100 centred on 1920x1080, at its natural size.
    assert_eq!(
        widget.place,
        Place::Desktop {
            x: 860,
            y: 490,
            size: None
        }
    );
}

#[test]
fn a_new_panel_widget_takes_the_first_zone_with_room() {
    let l = layout();
    let reg = registry();
    let mut s = scene(&l, &reg);
    let zone_of = |s: &Scene| match s.add("bar-only", Kind::Panel) {
        Some((Edit::AddWidget { widget, index, .. }, _)) => match widget.place {
            Place::Panel { zone, width } => Some((zone, width, index)),
            _ => None,
        },
        _ => None,
    };
    assert_eq!(zone_of(&s), Some((Zone::End, Width::Auto, Some(0))));
    // Its default length is 20: no room at the end, room at the start.
    s.regions[0].room = [20.0, 500.0, 19.0];
    assert_eq!(zone_of(&s), Some((Zone::Start, Width::Auto, None)));
    s.regions[0].room = [0.0, 0.0, 0.0];
    assert_eq!(zone_of(&s), None, "a full panel takes no more");
}

#[test]
fn a_new_widget_goes_only_where_its_type_may_and_gets_a_free_id() {
    let l = layout();
    let reg = registry();
    let s = scene(&l, &reg);
    assert!(s.add("bar-only", Kind::Desktop).is_none());
    assert!(s.add("toggle", Kind::Panel).is_none());
    assert!(s.add("no-such-type", Kind::Desktop).is_none());
    let (edit, id) = s.add("toggle", Kind::Sidebar).unwrap();
    assert_eq!(id, "toggle-1");
    let Edit::AddWidget {
        container, widget, ..
    } = edit
    else {
        panic!("not an add");
    };
    assert_eq!(container, "side");
    // The type's default span (cells), no wider than the sidebar's two columns.
    assert_eq!(widget.place, Place::Sidebar { size: 2 });
    // The edit is one the layout accepts, and the next widget of the type gets the next id.
    let mut e = crate::layout_model::Editor::new(l.clone());
    e.apply(Edit::AddWidget {
        monitor: 0,
        container,
        widget,
        index: None,
    })
    .unwrap();
    let l2 = e.layout().clone();
    let s2 = scene(&l2, &reg);
    assert_eq!(s2.add("toggle", Kind::Sidebar).unwrap().1, "toggle-2");
}

#[test]
fn the_settings_dialog_moves_a_panel_widget_to_a_zone_with_room_for_it() {
    let l = layout();
    let reg = registry();
    let mut s = scene(&l, &reg);
    assert_eq!(
        s.placement("c"),
        Some(Sits::Panel {
            zone: Zone::Center,
            width: Width::Auto
        })
    );
    // Where it is already: nothing to do.
    assert_eq!(s.to_zone("c", Zone::Center), None);
    let Some(Edit::MoveWidget { place, index, .. }) = s.to_zone("c", Zone::End) else {
        panic!("the end zone has room");
    };
    let end = Place::Panel {
        zone: Zone::End,
        width: Width::Auto,
    };
    assert_eq!((place, index), (end, Some(0)));
    // The clock shows 120 long: a zone with less room refuses it.
    s.regions[0].room = [119.0, 0.0, 120.0];
    assert_eq!(s.to_zone("c", Zone::Start), None);
    assert!(s.to_zone("c", Zone::End).is_some());
}

#[test]
fn the_settings_dialog_sizes_a_desktop_widget_within_its_limits() {
    let l = layout();
    let reg = registry();
    let s = scene(&l, &reg);
    // The clock: 40x20 at the least, 800x400 at the most.
    assert_eq!(
        s.placement("d1"),
        Some(Sits::Desktop {
            size: (200, 100),
            min: (40, 20),
            max: (800, 400)
        })
    );
    // The size it has: nothing to do.
    assert_eq!(s.to_size("d1", 200, 100), None);
    let size = |e: Option<Edit>| match e {
        Some(Edit::MoveWidget {
            place: Place::Desktop { x, y, size },
            ..
        }) => (x, y, size),
        other => panic!("{other:?}"),
    };
    assert_eq!(
        size(s.to_size("d1", 300, 150)),
        (100, 100, Some((300, 150)))
    );
    // Past the limits: the limit.
    assert_eq!(size(s.to_size("d1", 5000, 1)), (100, 100, Some((800, 20))));
}

#[test]
fn the_settings_dialog_sets_a_sidebar_widget_s_span_within_the_columns() {
    let l = layout();
    let reg = registry();
    let s = scene(&l, &reg);
    assert_eq!(
        s.placement("t1"),
        Some(Sits::Sidebar {
            size: 1,
            columns: 2
        })
    );
    assert_eq!(s.to_span("t1", 1), None);
    assert_eq!(s.to_span("t1", 3), None);
    assert_eq!(s.to_span("t1", 0), None);
    let Some(Edit::MoveWidget { place, index, .. }) = s.to_span("t1", 2) else {
        panic!("two columns hold a span of two");
    };
    assert_eq!((place, index), (Place::Sidebar { size: 2 }, None));
}

#[test]
fn a_sidebar_that_measured_more_than_its_height_overflows() {
    let l = layout();
    let reg = registry();
    let mut s = scene(&l, &reg);
    assert!(!s.overflows("t1"));
    assert!(!s.overflows("d1"));
    let r = s
        .regions
        .iter_mut()
        .find(|r| r.kind == Kind::Sidebar)
        .expect("a sidebar region");
    let room = r.sidebar.get_or_insert_with(Default::default);
    room.available = 100.0;
    for w in &l.monitors[0]
        .containers
        .iter()
        .find(|c| c.id == r.container)
        .expect("the sidebar")
        .widgets
    {
        room.heights.insert(w.id.clone(), 40.0);
    }
    let fits = !s.overflows("t1");
    let r = s
        .regions
        .iter_mut()
        .find(|r| r.kind == Kind::Sidebar)
        .expect("a sidebar region");
    r.sidebar.as_mut().expect("its room").available = 10.0;
    assert!(fits);
    assert!(s.overflows("t1"));
}

#[test]
fn a_widget_that_places_itself_is_resized_where_it_shows() {
    let l = layout();
    let reg = registry();
    let mut s = scene(&l, &reg);
    // d1 is at (100, 100) in the layout and puts itself at (700, 400), by the wallpaper.
    let shown = DRect {
        x: 700,
        y: 400,
        w: 200,
        h: 100,
    };
    s.placed.insert("d1".into(), Shows::At(shown));
    s.frames[3].rect = r(700.0, 400.0, 200.0, 100.0);
    let (g, e) = pull(
        &s,
        "d1",
        Edges::BOTTOM_RIGHT,
        (900.0, 500.0),
        (950.0, 530.0),
    );
    assert_eq!(
        g.preview.as_ref().unwrap().ghost,
        r(700.0, 400.0, 250.0, 130.0)
    );
    assert!(matches!(
        e,
        Some(Edit::MoveWidget {
            place: Place::Desktop {
                x: 700,
                y: 400,
                size: Some((250, 130)),
            },
            ..
        })
    ));
    assert!(matches!(
        s.to_size("d1", 220, 100),
        Some(Edit::MoveWidget {
            place: Place::Desktop {
                x: 700,
                y: 400,
                size: Some((220, 100)),
            },
            ..
        })
    ));
}

/// A surface whose manifest table says `listed = false` is not offered by the catalog; the
/// type still goes there (a quick toggle, by the pencil of the sidebar's quick toggles).
#[test]
fn the_catalog_leaves_out_the_surfaces_a_type_is_not_listed_for() {
    let mut reg = Registry::default();
    let errors = reg.load_manifest(
        r#"
        [[widget]]
        type = "wifi"
        name = "Wi-Fi"
        icon = "wifi"

        [widget.panel]
        component = "C.qml"
        size = { min = [24, 24], default = [24, 24] }

        [widget.sidebar]
        component = "C.qml"
        size = { min = [1, 1], default = [1, 1] }
        listed = false
        "#,
        std::path::Path::new("/nonexistent"),
    );
    assert!(errors.is_empty(), "{errors:?}");
    let entries = catalog(&reg);
    let [Value::Obj(wifi)] = &entries[..] else {
        panic!("{} entries", entries.len());
    };
    let offered = |surface: &str| wifi.borrow().props.get(surface).is_some_and(Value::truthy);
    assert!(offered("panel"));
    assert!(!offered("sidebar"));
    assert!(!offered("desktop"));
    assert!(reg.get("wifi").unwrap().allows(Placement::Sidebar));
}

/// Every shipped widget that shows more or less of itself by its length along a panel can be
/// pulled by its ends there: a press on the end of a selected clock sizes it and never picks it
/// up. A round toggle has one size.
#[test]
fn the_shipped_panel_widgets_of_many_lengths_have_handles() {
    let mut reg = Registry::default();
    let errors = reg.load_manifest(
        include_str!("../../../../qml/widgets.toml"),
        std::path::Path::new("/nonexistent"),
    );
    assert!(errors.is_empty(), "{errors:?}");
    for ty in ["clock", "weather", "media", "workspaces"] {
        for vertical in [false, true] {
            assert!(resizable(&reg, ty, Kind::Panel, vertical), "{ty}");
        }
    }
    assert!(!resizable(&reg, "nightLight", Kind::Panel, false));
}
