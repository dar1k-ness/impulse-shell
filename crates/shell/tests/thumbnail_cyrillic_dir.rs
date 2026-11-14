//! Wallpaper thumbnails in a Cyrillic directory (the owner's `~/Изображения/Wallpapers`): the
//! `Wallpapers` thumbnailer must write the file the original's own ThumbnailImage looks up, a restart
//! must show the cache without regenerating it, and thumbnails the original already made stay in use.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::scene;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const PNG: [u8; 74] = [
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x02, 0x08, 0x02, 0x00, 0x00, 0x00, 0xfd, 0xd4, 0x9a,
    0x73, 0x00, 0x00, 0x00, 0x11, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0x00,
    0x44, 0x0c, 0x60, 0xf2, 0x3f, 0x00, 0x1b, 0xf2, 0x03, 0xfd, 0xf3, 0xe7, 0x4b, 0xb1, 0x00, 0x00,
    0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

fn ri() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../qml")
}

struct Shell {
    rt: Runtime,
    root: usize,
    fonts: scene::Fonts,
    /// The world's service pings: process output and image loads report through them.
    wake: testenv::Wakeups,
    /// The Process service's epoll set (real child processes).
    ep: Option<i32>,
}

impl Shell {
    fn start(qml: &Path) -> Shell {
        let mut rt = Runtime::new(Some(ri()));
        let wake = testenv::Wakeups::new();
        rt.world.borrow().waker.set_ping(wake.ping());
        let dirs = ["modules/common", "modules/common/functions"].map(|d| ri().join(d));
        rt.load_singletons_where(&dirs, &|_| true);
        let root = rt.load_file(qml).unwrap();
        rt.settle();
        {
            let mut w = rt.world.borrow_mut();
            w.frame_paced = true;
            w.polish_wins.insert(root);
            w.frame_wins = Some(std::iter::once(root).collect());
        }
        let ep = rt.process_fd();
        Shell {
            rt,
            root,
            fonts: scene::Fonts::default(),
            wake,
            ep,
        }
    }

    fn read(&mut self, item: &str, p: &str) -> Value {
        let mut w = self.rt.world.borrow_mut();
        let i = match w.read(self.root, item) {
            Some(Value::Item(i)) => i,
            v => panic!("{item}: {v:?}"),
        };
        w.read(i, p).unwrap_or(Value::Undefined)
    }

    /// Draws frames until `done`, between frames sleeping until a running job or image load
    /// reports. `false` once a frame that began and ended with nothing in flight left `done`
    /// false: nothing is left that could change it.
    fn frames_until(&mut self, done: &dyn Fn(&mut Shell) -> bool) -> bool {
        loop {
            // Taken before the loads and jobs are drained: a report after the take pings.
            self.rt.world.borrow().waker.take();
            let busy = self.in_flight();
            scene::take_finished_images(&mut self.rt.world.borrow_mut());
            scene::layout_window(&mut self.rt, &mut self.fonts, self.root, [600.0, 200.0]);
            self.rt.settle();
            self.rt.world.borrow_mut().frame_seq += 1;
            qml::native::poll_processes(&mut self.rt.world.borrow_mut());
            self.rt.settle();
            if done(self) {
                return true;
            }
            if !self.in_flight() {
                if !busy {
                    return false;
                }
                continue;
            }
            self.wake.wait(self.ep, None);
        }
    }

    /// The thumbnailer runs or an image is loading: each reports when it ends.
    fn in_flight(&mut self) -> bool {
        self.read("gen", "running").truthy()
            || ["fresh", "kept"]
                .iter()
                .any(|i| self.read(i, "status").to_number() == 2.0)
    }

    fn ready(&mut self, item: &str) -> bool {
        self.read(item, "status").to_number() == 1.0
    }

    fn generate(&mut self) {
        let g = match self.rt.world.borrow_mut().read(self.root, "gen") {
            Some(Value::Item(i)) => i,
            v => panic!("gen: {v:?}"),
        };
        self.rt
            .world
            .borrow_mut()
            .write(g, "running", Value::Bool(true));
        self.rt.settle();
        assert!(
            self.frames_until(&|s| !s.read("gen", "running").truthy()),
            "thumbnailer never finished"
        );
    }
}

fn mtimes(dir: &Path) -> Vec<(PathBuf, SystemTime)> {
    let mut v: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| {
            let p = e.unwrap().path();
            let t = std::fs::metadata(&p).unwrap().modified().unwrap();
            (p, t)
        })
        .collect();
    v.sort();
    v
}

#[test]
fn cyrillic_dir_thumbnails_are_found_and_cached() {
    const TEST: &str = concat!(
        module_path!(),
        "::cyrillic_dir_thumbnails_are_found_and_cached"
    );
    if !testenv::in_own_process(TEST) {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        std::fs::create_dir(&home).unwrap();
        testenv::run_in_own_process(TEST, testenv::home_vars(&home));
        return;
    }
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    let tmp = home.parent().unwrap();
    let walls = home.join("Изображения/Обои");
    std::fs::create_dir_all(&walls).unwrap();
    for f in ["закат 1.png", "old.png"] {
        std::fs::write(walls.join(f), PNG).unwrap();
    }
    let large = home.join(".cache/thumbnails/large");
    std::fs::create_dir_all(&large).unwrap();
    // A thumbnail the original's ThumbnailImage already made (for this wallpaper, at this size).
    let old = imgtools::thumbs::cache_file(&large, &walls.join("old.png"));
    std::fs::write(&old, PNG).unwrap();
    let past = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    std::fs::File::options()
        .write(true)
        .open(&old)
        .unwrap()
        .set_modified(past)
        .unwrap();

    let image = |id: &str, x: u32, file: &str| {
        format!(
            r#"
    ThumbnailImage {{
        id: {id}
        x: {x}; width: 256; height: 141
        sourceSize: Qt.size(256, 141)
        sourcePath: "{}"
        cache: false
    }}"#,
            walls.join(file).display()
        )
    };
    let qml = tmp.join("t.qml");
    std::fs::write(
        &qml,
        format!(
            r#"
import QtQuick
import Impulse
import ri.modules.common.widgets
Item {{
    id: root
    width: 600; height: 200
    property var gen: g
    property var fresh: a
    property var kept: b
    // WallpaperDirectoryItem's thumbnailGenerated handler.
    DirectoryThumbnails {{
        id: g
        directory: "{walls}"
        size: "large"
        onFinished: {{
            for (const im of [a, b]) {{
                if (im.status !== Image.Error) continue;
                im.source = "";
                im.source = im.thumbnailPath;
            }}
        }}
    }}
    {a}
    {b}
}}
"#,
            walls = walls.display(),
            a = image("a", 0, "закат 1.png"),
            b = image("b", 300, "old.png"),
        ),
    )
    .unwrap();

    let mut sh = Shell::start(&qml);
    let path = |sh: &mut Shell, id: &str| {
        let p = sh.read(id, "thumbnailPath").to_js_string();
        PathBuf::from(p.strip_prefix("file://").unwrap_or(&p))
    };
    let fresh = path(&mut sh, "fresh");
    assert_eq!(fresh.parent().unwrap(), large);
    assert!(!fresh.exists());
    assert_eq!(path(&mut sh, "kept"), old);
    sh.generate();
    assert!(
        fresh.is_file(),
        "generator wrote another name than {}",
        fresh.display()
    );
    assert_eq!(
        fresh,
        imgtools::thumbs::cache_file(&large, &walls.join("закат 1.png"))
    );
    assert!(
        sh.frames_until(&|s| s.ready("fresh") && s.ready("kept")),
        "fresh {:?} kept {:?}",
        sh.read("fresh", "status"),
        sh.read("kept", "status")
    );
    assert_eq!(std::fs::metadata(&old).unwrap().modified().unwrap(), past);
    let cached = mtimes(&large);
    assert_eq!(cached.len(), 2, "{cached:?}");
    drop(sh);

    // Restart: the cache shows at once and nothing is regenerated.
    let mut sh = Shell::start(&qml);
    assert!(
        sh.frames_until(&|s| s.ready("fresh") && s.ready("kept")),
        "after restart: fresh {:?} kept {:?}",
        sh.read("fresh", "status"),
        sh.read("kept", "status")
    );
    sh.generate();
    assert_eq!(mtimes(&large), cached);
}
