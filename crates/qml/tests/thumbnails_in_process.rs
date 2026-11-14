//! `DirectoryThumbnails` makes a directory's thumbnails itself, whatever is installed, and
//! tells its QML how far it is: `progress` and `fileGenerated` for every file as it is done,
//! before `finished` (the wallpaper selector's bar and its thumbnails appearing one by one).
use qml::runtime::Runtime;
use std::path::{Path, PathBuf};
use testenv::Wakeups;

fn ri() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../qml")
}

/// What the type's signals reported until `finished`.
struct Run {
    /// `completed/total` of each `progress`, in order.
    progress: Vec<String>,
    /// The path of each `fileGenerated`, sorted.
    files: Vec<String>,
    /// Signals that came after `finished`.
    late: usize,
}

fn run(dir: &Path) -> Run {
    let mut rt = Runtime::new(Some(ri()));
    let wake = Wakeups::new();
    rt.world.borrow().waker.set_ping(wake.ping());
    let ep = rt.process_fd();
    let t = tempfile::tempdir().unwrap();
    let f = t.path().join("Ab.qml");
    std::fs::write(
        &f,
        format!(
            r#"import QtQuick
import Impulse
Item {{
    id: root
    property string progress: ""
    property string files: ""
    property int late: 0
    property bool done: false
    DirectoryThumbnails {{
        directory: {dir}
        size: "normal"
        running: true
        onProgress: (completed, total) => {{
            if (root.done) root.late += 1;
            root.progress += completed + "/" + total + "\n";
        }}
        onFileGenerated: path => {{
            if (root.done) root.late += 1;
            root.files += JSON.stringify(path) + "\n";
        }}
        onFinished: {{ root.done = true }}
    }}
}}
"#,
            dir = serde_json::to_string(dir.to_str().unwrap()).unwrap()
        ),
    )
    .unwrap();
    let root = rt.load_file(&f).unwrap();
    rt.settle();
    let read = |rt: &Runtime, k: &str| rt.world.borrow_mut().read(root, k).unwrap();
    // The job reports with a ping; nextest bounds a hung one.
    loop {
        rt.poll_native();
        rt.settle();
        if read(&rt, "done").truthy() {
            break;
        }
        if wake.wait(ep, None).fd {
            rt.process_fd_ready();
        }
    }
    let list = |rt: &Runtime, k: &str| -> Vec<String> {
        let text = read(rt, k).to_js_string();
        text.lines().map(str::to_string).collect()
    };
    // One JSON string a line: a path may itself have a line break.
    let mut files: Vec<String> = list(&rt, "files")
        .iter()
        .map(|f| serde_json::from_str(f).unwrap())
        .collect();
    files.sort();
    Run {
        progress: list(&rt, "progress"),
        files,
        late: read(&rt, "late").to_number() as usize,
    }
}

/// A directory of four pictures, one with a line break in its name, a GIF (skipped, as the original's
/// scripts skip it) and a text file (not a picture: no thumbnail), under a name outside ASCII.
fn walls(home: &Path) -> PathBuf {
    let d = home.join("Изображения");
    std::fs::create_dir_all(&d).unwrap();
    for (f, px) in [
        ("a.png", [255, 0, 0]),
        ("b.png", [0, 255, 0]),
        ("c.png", [0, 0, 255]),
        (TWO_LINES, [255, 255, 0]),
    ] {
        image::RgbImage::from_pixel(8, 8, image::Rgb(px))
            .save(d.join(f))
            .unwrap();
    }
    std::fs::write(d.join("d.gif"), b"GIF89a").unwrap();
    std::fs::write(d.join("notes.txt"), b"x").unwrap();
    d
}

/// A file name of two lines.
const TWO_LINES: &str = "two\nlines.png";

fn thumbs(home: &Path, dir: &Path) -> Vec<PathBuf> {
    ["a.png", "b.png", "c.png", TWO_LINES]
        .iter()
        .map(|f| imgtools::thumbs::cache_file(&home.join(".cache/thumbnails/normal"), &dir.join(f)))
        .collect()
}

const HOME: &str = "THUMBNAILS_IN_PROCESS_HOME";

/// Runs `test`'s body in a process of its own with a fresh home, where the thumbnails go.
fn in_fresh_home(test: &str, extra: &[(&'static str, std::ffi::OsString)]) {
    let home = tempfile::tempdir().unwrap();
    let home_path = std::fs::canonicalize(home.path()).unwrap();
    let mut vars = testenv::home_vars(&home_path);
    vars.push((HOME, home_path.into()));
    vars.extend(extra.iter().cloned());
    testenv::run_in_own_process(test, vars);
}

/// With GnomeDesktop's typelib installed the type used to start the original's `bash` scripts instead
/// (`thumbgen-venv.sh`, else `generate-thumbnails-magick.sh`, whose cache names
/// `ThumbnailImage.qml` does not find for a path outside ASCII).
#[test]
fn made_in_process_where_gnome_desktop_is_installed() {
    const TEST: &str = concat!(
        module_path!(),
        "::made_in_process_where_gnome_desktop_is_installed"
    );
    if !testenv::in_own_process(TEST) {
        let typelibs = tempfile::tempdir().unwrap();
        std::fs::write(typelibs.path().join("GnomeDesktop-4.0.typelib"), b"").unwrap();
        // No `bash` (nor anything else) to start: the work is the type's own.
        let no_tools = tempfile::tempdir().unwrap();
        in_fresh_home(
            TEST,
            &[
                ("GI_TYPELIB_PATH", typelibs.path().into()),
                ("PATH", no_tools.path().into()),
            ],
        );
        return;
    }
    let home = PathBuf::from(std::env::var_os(HOME).unwrap());
    let dir = walls(&home);
    run(&dir);
    for t in thumbs(&home, &dir) {
        assert!(t.is_file(), "no thumbnail {}", t.display());
    }
}

#[test]
fn reports_each_file_as_it_is_done() {
    const TEST: &str = concat!(module_path!(), "::reports_each_file_as_it_is_done");
    if !testenv::in_own_process(TEST) {
        in_fresh_home(TEST, &[]);
        return;
    }
    let home = PathBuf::from(std::env::var_os(HOME).unwrap());
    let dir = walls(&home);
    let r = run(&dir);
    let counts = ["1/6", "2/6", "3/6", "4/6", "5/6", "6/6"];
    assert_eq!(r.progress, counts);
    // Each path whole, the one of two lines too.
    let all: Vec<String> = ["a.png", "b.png", "c.png", "d.gif", "notes.txt", TWO_LINES]
        .iter()
        .map(|f| dir.join(f).to_str().unwrap().to_string())
        .collect();
    assert_eq!(r.files, all);
    assert_eq!(r.late, 0, "reports after finished");
    // Every thumbnail is current now: each file is still reported.
    let r = run(&dir);
    assert_eq!(r.progress, counts);
    assert_eq!(r.files, all);
}
