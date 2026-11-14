//! The bar's groups (media, workspaces, clock) moved to other parts of the bar and back while
//! the shell runs (`rusty-impulse wallpick arrange`, the settings' "Group layout") end where
//! they started: the same widgets at the same places and lengths as before the first move, and
//! the same layout.
#![cfg(feature = "interp")]

use qml::layout_model::arrange;
use qml::runtime::{Runtime, World};
use qml::value::Value;
use shell::scene;
use std::path::{Path, PathBuf};

const LAYOUT: &str = r#"version = 5
[[monitor]]
connector = "DP-1"
primary = true

[[monitor.container]]
id = "panel"
kind = "panel"
edge = "rusty"
"#;

const WIDGETS: [(&str, &str, &str); 13] = [
    ("left-sidebar-button", "start", "auto"),
    ("active-window", "start", "stretch"),
    ("resources", "center", "auto"),
    ("media", "center", "stretch"),
    ("separator-1", "center", "auto"),
    ("workspaces", "center", "auto"),
    ("separator-2", "center", "auto"),
    ("clock", "center", "stretch"),
    ("util-buttons", "center", "auto"),
    ("battery", "center", "auto"),
    ("spacer", "end", "stretch"),
    ("systray", "end", "auto"),
    ("indicators", "end", "auto"),
];

struct Bar {
    rt: Runtime,
    fonts: scene::Fonts,
    main: usize,
    now: f64,
    dir: PathBuf,
    width: f64,
}

impl Bar {
    fn new(width: f64) -> Self {
        let ri = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../qml")
            .canonicalize()
            .unwrap();
        let dir = std::env::temp_dir().join(format!("panel-groups-{}-{width}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for e in std::fs::read_dir(&ri).unwrap() {
            let e = e.unwrap();
            std::os::unix::fs::symlink(e.path(), dir.join(e.file_name())).unwrap();
        }
        let mut layout = LAYOUT.to_string();
        for (id, zone, w) in WIDGETS {
            let ty = if id.starts_with("separator") {
                "separator"
            } else {
                id
            };
            layout += &format!(
                "\n[[monitor.container.widget]]\nid = \"{id}\"\ntype = \"{ty}\"\nzone = \"{zone}\"\nwidth = \"{w}\"\n"
            );
        }
        std::fs::write(dir.join("layout.toml"), layout).unwrap();
        std::fs::write(
            dir.join("main.qml"),
            format!(
                "import QtQuick\nimport ri.modules.common\nimport ri.modules.rusty.bar.panel\nItem {{\n    width: {width}\n    height: 40\n    \
                 PanelContent {{\n        anchors.fill: parent\n        \
                 screen: ({{ name: \"DP-1\", width: {width}, height: 1080 }})\n    }}\n}}\n"
            ),
        )
        .unwrap();
        let mut rt = Runtime::new(Some(dir.clone()));
        rt.load_singletons(&[
            dir.join("modules/common"),
            dir.join("modules/common/functions"),
            dir.join("services"),
            dir.clone(),
        ]);
        rt.world.borrow_mut().open_layout(dir.join("layout.toml"));
        rt.settle();
        let main = rt.load_file(&dir.join("main.qml")).unwrap();
        rt.settle();
        let mut bar = Bar {
            rt,
            fonts: scene::Fonts::default(),
            main,
            now: 1000.0,
            dir,
            width,
        };
        bar.frames(200);
        bar
    }

    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.now += 16.0;
            self.rt.tick(self.now);
            self.rt.settle();
            scene::layout_window(&mut self.rt, &mut self.fonts, self.main, [self.width, 40.0]);
            self.rt.settle();
        }
    }

    /// Moves the groups in the layout file, as the wallpick job does, and lets the shell read it.
    fn arrange(&mut self, left: &[&str], center: &[&str], right: &[&str]) {
        let own = |p: &[&str]| p.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let parts = [own(left), own(center), own(right)];
        arrange::in_file(&self.dir.join("layout.toml"), |l| {
            arrange::bar_groups(l, [&parts[0], &parts[1], &parts[2]])
        })
        .unwrap();
        let msg = self.rt.world.borrow_mut().layout.as_mut().unwrap().reload();
        assert_eq!(msg, None);
        self.frames(200);
    }

    /// The layout the shell holds, as read from the file.
    fn layout(&self) -> qml::layout_model::Layout {
        let w = self.rt.world.borrow();
        w.layout.as_ref().unwrap().editor().layout().clone()
    }

    /// Every shown widget of the panel: its layout id, where it starts in the window and how
    /// long it is, in order along the bar.
    fn widgets(&self) -> Vec<(String, f64, f64)> {
        let mut w = self.rt.world.borrow_mut();
        let num =
            |w: &mut World, i: usize, k: &str| w.read(i, k).unwrap_or(Value::Num(0.0)).to_number();
        let slots: Vec<usize> = (0..w.items.len())
            .filter(|&i| {
                !w.dead.contains(&i) && w.items[i].types.iter().any(|t| t == "PanelWidget")
            })
            .collect();
        let mut out = vec![];
        for s in slots {
            let shown = {
                let mut i = Some(s);
                let mut ok = true;
                while let Some(x) = i {
                    ok &= w.read(x, "visible").is_none_or(|v| v.truthy());
                    i = w.items[x].parent;
                }
                ok
            };
            let length = num(&mut w, s, "width");
            if !shown || length <= 0.0 {
                continue;
            }
            // PanelContent's off-screen probes have no layout id.
            let id = match w.read(s, "layoutId") {
                Some(v) if !v.to_js_string().is_empty() => v.to_js_string(),
                _ => continue,
            };
            let at = w.scene_pos(s, None).0;
            out.push((id, at, length));
        }
        out.sort_by(|a, b| a.1.total_cmp(&b.1));
        out
    }
}

impl Drop for Bar {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn there_and_back(width: f64, left: &[&str], center: &[&str], right: &[&str]) {
    let mut bar = Bar::new(width);
    let (layout, before) = (bar.layout(), bar.widgets());
    assert!(before.len() >= 8, "{before:#?}");
    bar.arrange(left, center, right);
    let moved = bar.widgets();
    assert_ne!(moved, before, "the groups did not move");
    bar.arrange(&[], &["media", "workspaces", "clock"], &[]);
    assert_eq!(bar.widgets(), before, "moved: {moved:#?}");
    assert_eq!(bar.layout(), layout);
    assert!(bar.rt.errors().is_empty(), "{:?}", bar.rt.errors());
}

#[test]
fn spread_and_back_to_centered_is_the_bar_as_it_was() {
    there_and_back(1920.0, &["media"], &["workspaces"], &["clock"]);
}

#[test]
fn workspaces_left_and_back_to_centered_is_the_bar_as_it_was() {
    there_and_back(1920.0, &["workspaces"], &["media", "clock"], &[]);
}

#[test]
fn spread_and_back_on_a_1280_bar_is_the_bar_as_it_was() {
    there_and_back(1280.0, &["media"], &["workspaces"], &["clock"]);
}
