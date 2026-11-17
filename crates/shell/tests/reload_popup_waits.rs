//! The "Shell reloaded" popup starts its fade and its countdown once the shell draws its
//! frames on time: a shell that has just started is still making things, and a frame it is
//! late with would be a step in the fade. It waits for twelve frames in a row that each came
//! on time (not well over what the frames before it took), and for no longer than a second
//! and a half.
#![cfg(feature = "interp")]

use qml::runtime::Runtime;
use std::path::{Path, PathBuf};

const MAIN: &str = r#"import QtQuick
import Rustshell
import ri
Item {
    ReloadPopup {}
}
"#;

fn tree(name: &str) -> PathBuf {
    let ri = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../qml")
        .canonicalize()
        .unwrap();
    let d = std::env::temp_dir().join(format!("reload-popup-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    for e in std::fs::read_dir(&ri).unwrap() {
        let e = e.unwrap();
        std::os::unix::fs::symlink(e.path(), d.join(e.file_name())).unwrap();
    }
    std::fs::write(d.join("main.qml"), MAIN).unwrap();
    d
}

struct Popup {
    rt: Runtime,
    now: f64,
}

impl Popup {
    /// The shell has told its tree that a reload started it.
    fn reloaded(name: &str) -> Popup {
        let dir = tree(name);
        let mut rt = Runtime::new(Some(dir.clone()));
        rt.load_singletons(&[
            dir.join("modules/common"),
            dir.join("modules/common/functions"),
            dir.join("services"),
            dir.clone(),
        ]);
        rt.set_screens(&[("DP-1".into(), 0, 0, 1920, 1080)]);
        rt.settle();
        rt.world.borrow_mut().frame_paced = true;
        rt.load_file(&dir.join("main.qml")).unwrap();
        rt.settle();
        let qs = rt.world.borrow().singletons.get("Rustshell").copied();
        rt.emit(qs.expect("Rustshell"), "reloadCompleted", &[], vec![]);
        rt.settle();
        Popup { rt, now: 1000.0 }
    }

    /// A frame `ms` after the one before it.
    fn frame(&mut self, ms: f64) {
        self.now += ms;
        self.rt.tick(self.now);
        self.rt.settle();
        assert!(self.rt.errors().is_empty(), "{:?}", self.rt.errors());
    }

    /// Whether the fade and the countdown have started.
    fn started(&self) -> bool {
        let wins = self.rt.world.borrow_mut().of_type("PanelWindow");
        wins.iter().any(|&win| self.rt.get(win, "started").truthy())
    }

    /// Frames of `ms` until the popup starts; how many it took.
    fn frames_until_started(&mut self, ms: f64) -> usize {
        (1..400)
            .find(|_| {
                self.frame(ms);
                self.started()
            })
            .expect("the popup never started")
    }
}

#[test]
fn it_starts_after_twelve_frames_on_time() {
    let mut p = Popup::reloaded("steady");
    // Ten frames until the smoothed frame time is near the frames', then the twelve.
    let n = p.frames_until_started(16.0);
    assert!((20..=24).contains(&n), "started after {n} frames");
}

#[test]
fn a_late_frame_starts_the_wait_again() {
    let mut p = Popup::reloaded("late");
    for _ in 0..16 {
        p.frame(16.0);
    }
    // The shell made something for two frames' time.
    p.frame(48.0);
    for _ in 0..11 {
        p.frame(16.0);
        assert!(!p.started());
    }
    p.frame(16.0);
    assert!(p.started(), "twelve frames on time after the late one");
}

#[test]
fn it_starts_after_a_second_and_a_half_of_late_frames() {
    let mut p = Popup::reloaded("never");
    // Every tenth frame is late.
    let mut ms = 0.0;
    while !p.started() {
        assert!(ms < 1800.0, "not started after {ms} ms");
        for _ in 0..9 {
            p.frame(16.0);
        }
        p.frame(64.0);
        ms += 208.0;
    }
    assert!(ms >= 1450.0, "started after {ms} ms");
}
