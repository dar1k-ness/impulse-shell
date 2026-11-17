//! The cheatsheet's window is made once, a moment after the shell starts, and kept: it has a
//! few thousand items (162 tiles of the periodic table, a line per keybind), and an opening
//! that made them all waited a second and a half for its first frame on the PC this was
//! reported from. Not yet shown, the window makes its tiles one at a time, so that no frame of
//! anything else waits for a row of them; shown, it has all of its content at once, as the
//! window made by each opening had.
#![cfg(feature = "interp")]

use qml::runtime::{Runtime, World};
use qml::value::Value;
use shell::keys::Keyboard;
use shell::scene;
use std::path::{Path, PathBuf};

const W: f64 = 1920.0;
const H: f64 = 1040.0;
/// The tiles of the periodic table (periodic_table.js), the empty ones included.
const TILES: usize = 162;
const ESCAPE: u32 = 0xff1b;

const MAIN: &str = r#"import QtQuick
import Rustshell
import ri
import ri.services
import ri.modules.rusty.cheatsheet
Item {
    property int grabbed: GlobalFocusGrab.dismissable.length
    function dismiss() { GlobalFocusGrab.dismiss() }
    function popup(on) { GlobalStates.reloadPopupShown = on }
    Cheatsheet {}
}
"#;

fn tree(name: &str) -> PathBuf {
    let ri = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../qml")
        .canonicalize()
        .unwrap();
    let d = std::env::temp_dir().join(format!("cheatsheet-kept-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    for e in std::fs::read_dir(&ri).unwrap() {
        let e = e.unwrap();
        std::os::unix::fs::symlink(e.path(), d.join(e.file_name())).unwrap();
    }
    std::fs::write(d.join("main.qml"), MAIN).unwrap();
    d
}

struct Sheet {
    rt: Runtime,
    root: usize,
    fonts: scene::Fonts,
    now: f64,
}

impl Sheet {
    fn new(name: &str) -> Sheet {
        let dir = tree(name);
        let mut rt = Runtime::new(Some(dir.clone()));
        rt.load_singletons(&[
            dir.join("modules/common"),
            dir.join("modules/common/functions"),
            dir.join("services"),
            dir.clone(),
        ]);
        rt.set_screens(&[("DP-1".into(), 0, 0, W as i32, H as i32)]);
        rt.settle();
        rt.world.borrow_mut().frame_paced = true;
        let root = rt.load_file(&dir.join("main.qml")).unwrap();
        rt.settle();
        Sheet {
            rt,
            root,
            fonts: scene::Fonts::default(),
            now: 1000.0,
        }
    }

    /// The cheatsheet's window, once it is made.
    fn window(&self) -> Option<usize> {
        let mut w = self.rt.world.borrow_mut();
        let windows = w.of_type("PanelWindow");
        let mut i = *w.of_type("CheatsheetPeriodicTable").first()?;
        while !windows.contains(&i) {
            i = w.items[i].parent?;
        }
        Some(i)
    }

    fn shown(&self) -> bool {
        self.window()
            .is_some_and(|win| self.rt.get(win, "visible").truthy())
    }

    /// One frame of the shell: timers, bindings, and the layout of the window if it shows.
    fn frame(&mut self) {
        self.now += 16.0;
        self.rt.tick(self.now);
        self.rt.settle();
        if let Some(win) = self.window().filter(|_| self.shown()) {
            self.rt.set_polish_wins([win]);
            scene::layout_window(&mut self.rt, &mut self.fonts, win, [W, H]);
            self.rt.settle();
        }
        assert!(self.rt.errors().is_empty(), "{:?}", self.rt.errors());
    }

    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.frame();
        }
    }

    /// `qs ipc call cheatsheet FUNCTION`.
    fn ipc(&mut self, function: &str) {
        let handler = {
            let mut w = self.rt.world.borrow_mut();
            let target = |w: &mut World, i| matches!(w.read(i, "target"), Some(Value::Str(s)) if s.as_ref() == "cheatsheet");
            (w.of_type("IpcHandler").into_iter())
                .find(|&i| target(&mut w, i))
                .expect("the cheatsheet's IpcHandler")
        };
        self.rt
            .call_method(handler, function, vec![])
            .unwrap()
            .unwrap();
        self.rt.settle();
    }

    fn tiles(&self) -> Vec<usize> {
        self.rt.world.borrow_mut().of_type("ElementTile")
    }

    /// Esc, pressed with the keys in the cheatsheet's window.
    fn esc(&mut self) {
        let mut kb = Keyboard::default();
        kb.win = self.window();
        kb.sync(&mut self.rt);
        kb.key(&mut self.rt, ESCAPE, None, true, false);
        kb.key(&mut self.rt, ESCAPE, None, false, false);
        self.rt.settle();
    }

    fn grabbed(&self) -> f64 {
        match self.rt.world.borrow_mut().read(self.root, "grabbed") {
            Some(Value::Num(n)) => n,
            v => panic!("grabbed: {v:?}"),
        }
    }

    /// The tiles of the first row: the `x` of each and the texts it shows.
    fn first_row(&self) -> Vec<(f64, Vec<String>)> {
        let tiles = self.tiles();
        let mut w = self.rt.world.borrow_mut();
        let texts = w.of_type("StyledText");
        let under = |w: &World, mut i: usize, t: usize| loop {
            match w.items[i].parent {
                Some(p) if p == t => break true,
                Some(p) => i = p,
                None => break false,
            }
        };
        // The row of hydrogen: the tiles are not listed in the order of the table.
        let hydrogen = (texts.iter().copied())
            .find(|&i| matches!(w.read(i, "text"), Some(Value::Str(s)) if s.as_ref() == "H"))
            .expect("hydrogen");
        let first = (tiles.iter().copied())
            .find(|&t| under(&w, hydrogen, t))
            .expect("hydrogen's tile");
        let row = w.items[first].parent.unwrap();
        let in_row: Vec<usize> = (tiles.iter().copied())
            .filter(|&t| w.items[t].parent == Some(row))
            .collect();
        (in_row.into_iter())
            .map(|t| {
                let x = match w.read(t, "x") {
                    Some(Value::Num(x)) => x,
                    v => panic!("x: {v:?}"),
                };
                let of_tile: Vec<usize> = (texts.iter().copied())
                    .filter(|&i| under(&w, i, t))
                    .collect();
                let shown = (of_tile.into_iter())
                    .filter_map(|i| match w.read(i, "text") {
                        Some(Value::Str(s)) => Some(s.to_string()),
                        _ => None,
                    })
                    .collect();
                (x, shown)
            })
            .collect()
    }
}

/// Seconds after the start nothing has asked for the cheatsheet: its window is there, not
/// shown, with every tile, and no frame on the way made more than one of them.
#[test]
fn the_window_is_made_after_the_start_a_tile_a_frame() {
    let mut s = Sheet::new("start");
    assert_eq!(s.window(), None, "made with the shell");
    let mut most = 0;
    let mut have = 0;
    for _ in 0..800 {
        s.frame();
        let n = s.tiles().len();
        most = most.max(n - have);
        have = n;
    }
    assert!(s.window().is_some());
    assert!(!s.shown());
    assert_eq!(have, TILES);
    assert_eq!(most, 1, "tiles made in one frame");
    assert_eq!(s.grabbed(), 0.0);
}

/// An opening shows the window that is there: not one of its tiles is made again. Open, it is
/// in the focus grab; closed, it is out of it and kept.
#[test]
fn the_window_is_not_made_while_the_reload_popup_shows() {
    let mut s = Sheet::new("popup");
    let popup = |s: &mut Sheet, on: bool| {
        (s.rt.call_method(s.root, "popup", vec![Value::Bool(on)]))
            .unwrap()
            .unwrap();
        s.rt.settle();
    };
    // The popup comes a second after the start and stays for four.
    s.frames(60);
    popup(&mut s, true);
    s.frames(250);
    assert_eq!(s.window(), None, "made under the popup");
    // Gone, the window is made after the same while as after a start.
    popup(&mut s, false);
    s.frames(150);
    assert_eq!(s.window(), None, "made as the popup left");
    s.frames(50);
    assert!(s.window().is_some(), "not made after the popup");
    assert!(!s.shown());
}

#[test]
fn an_opening_shows_the_window_that_is_there() {
    let mut s = Sheet::new("open");
    s.frames(800);
    let (window, tiles) = (s.window(), s.tiles());
    for _ in 0..2 {
        s.ipc("toggle");
        s.frames(3);
        assert!(s.shown());
        assert_eq!(s.grabbed(), 1.0);
        assert_eq!((s.window(), s.tiles()), (window, tiles.clone()));
        s.ipc("toggle");
        s.frames(3);
        assert!(!s.shown());
        assert_eq!(s.grabbed(), 0.0);
        assert_eq!((s.window(), s.tiles()), (window, tiles.clone()));
    }
    // Esc closes it, at each opening: the keys are with its close button again.
    for _ in 0..2 {
        s.ipc("open");
        s.frames(3);
        s.esc();
        s.frames(3);
        assert!(!s.shown());
        assert_eq!(s.grabbed(), 0.0);
    }
    // A click outside (the grab's dismissal) closes it as it did.
    s.ipc("open");
    s.frames(3);
    s.rt.call_method(s.root, "dismiss", vec![])
        .unwrap()
        .unwrap();
    s.rt.settle();
    s.frames(3);
    assert!(!s.shown());
    assert_eq!(s.tiles(), tiles);
}

/// An opening that does not wait for the window, or for the rest of its tiles, has all of the
/// content in its first frame.
#[test]
fn an_early_opening_has_all_of_the_content() {
    for wait in [0, 250] {
        let mut s = Sheet::new(&format!("early-{wait}"));
        s.frames(wait);
        assert!(s.tiles().len() < TILES, "after {wait} frames");
        s.ipc("open");
        s.frame();
        assert!(s.shown());
        assert_eq!(s.tiles().len(), TILES, "after {wait} frames");
        let row = s.first_row();
        assert_eq!(row.len(), 18);
        assert!(row.windows(2).all(|p| p[0].0 < p[1].0), "{row:?}");
        let shows = |k: usize, symbol: &str| row[k].1.iter().any(|t| t == symbol);
        assert!(shows(0, "H") && shows(17, "He"), "{row:?}");
    }
}
