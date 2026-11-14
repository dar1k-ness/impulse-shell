//! The image types of the `Impulse` module against the QML scripts they took over from: each
//! `scripts/images` / `scripts/thumbnails` script runs as the original's QML ran it (venv Python, OpenCV,
//! magick) and the type runs on the same images; the result the QML gets and the written
//! thumbnails must match.
use qml::runtime::Runtime;
use std::path::{Path, PathBuf};
use testenv::Wakeups;

fn ri() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../qml")
}

fn wallpapers() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../colorgen/tests/switchwall/fixture/home/Pictures/Wallpapers")
}

fn images() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = [
        "syn_grad.jpg",
        "syn_small.png",
        "duo-dark.png",
        "syn_gray.png",
    ]
    .iter()
    .map(|f| wallpapers().join(f))
    .collect();
    v.push(ri().join("assets/images/default_wallpaper.png"));
    v
}

fn tmpdir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("imgjobs-equiv-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// The tools the original commands run, as the original installs them.
fn have_tools() -> bool {
    let venv = std::env::var("RUSTY_IMPULSE_VIRTUAL_ENV").unwrap_or_default();
    if venv.is_empty() {
        testenv::unavailable(format_args!("RUSTY_IMPULSE_VIRTUAL_ENV is not set"));
        return false;
    }
    let ok = std::process::Command::new(format!("{venv}/bin/python3"))
        .args(["-c", "import cv2, numpy"])
        .status()
        .is_ok_and(|s| s.success());
    if !ok {
        testenv::unavailable(format_args!("{venv} lacks cv2/numpy"));
        return false;
    }
    testenv::require("magick") && testenv::require("bash")
}

/// What `argv` (one of the original's image scripts, as the shell's QML ran it before the `Impulse`
/// types) prints, as the JSON the QML parsed; `None` when it prints nothing.
fn script(argv: &[String]) -> Option<serde_json::Value> {
    let out = std::process::Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    (!text.trim().is_empty()).then(|| doubles(serde_json::from_str(&text).unwrap()))
}

/// `v` with every number as the double QML holds (the script prints `1.0`, QML `1`).
fn doubles(v: serde_json::Value) -> serde_json::Value {
    use serde_json::Value as V;
    match v {
        V::Number(n) => {
            serde_json::Number::from_f64(n.as_f64().unwrap()).map_or(V::Null, V::Number)
        }
        V::Array(a) => V::Array(a.into_iter().map(doubles).collect()),
        V::Object(o) => V::Object(o.into_iter().map(|(k, v)| (k, doubles(v))).collect()),
        v => v,
    }
}

fn bash(script: String) -> Vec<String> {
    vec!["bash".into(), "-c".into(), script]
}

/// One of the original's image scripts: they are kept with the tests, the tree does not ship them.
fn scripts(rel: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/original-unshipped/scripts")
        .join(rel)
        .to_str()
        .unwrap()
        .to_string()
}

/// What a run of `ty { props }` reported.
#[derive(Debug, PartialEq)]
struct Run {
    /// The argument of `found`, as JSON; `None` when the run ended without it.
    found: Option<serde_json::Value>,
    /// `progress` and `fileGenerated` signals.
    reports: usize,
}

/// Runs the `Impulse` type `ty { props }` until `finished`.
fn run(ty: &str, props: &str) -> Run {
    let mut rt = Runtime::new(Some(ri()));
    let wake = Wakeups::new();
    rt.world.borrow().waker.set_ping(wake.ping());
    let ep = rt.process_fd();
    let d = tmpdir(&format!("qml-{ty}"));
    let f = d.join("Ab.qml");
    let handlers = if ty == "DirectoryThumbnails" {
        "onProgress: (completed, total) => { root.reports += 1 }\n        \
         onFileGenerated: path => { root.reports += 1 }"
    } else {
        "onFound: v => { root.out = JSON.stringify(v) }"
    };
    std::fs::write(
        &f,
        format!(
            r#"import QtQuick
import Impulse
Item {{
    id: root
    property string out: ""
    property int reports: 0
    property bool done: false
    {ty} {{
        {props}
        running: true
        {handlers}
        onFinished: {{ root.done = true }}
    }}
}}
"#
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
    let out = read(&rt, "out").to_js_string();
    Run {
        found: (!out.is_empty()).then(|| doubles(serde_json::from_str(&out).unwrap())),
        reports: read(&rt, "reports").to_number() as usize,
    }
}

/// The type reports what the script printed. `Some` or `None`, as `want_result` says.
fn same(ty: &str, props: &str, argv: &[String], want_result: bool) {
    let want = script(argv);
    let got = run(ty, props).found;
    assert_eq!(got, want, "{ty} {{ {props} }}: type vs script");
    assert_eq!(want.is_some(), want_result, "{ty} {{ {props} }}: {want:?}");
}

fn q(p: &Path) -> String {
    serde_json::to_string(p.to_str().unwrap()).unwrap()
}

#[test]
fn least_busy_region_matches_script() {
    if !have_tools() {
        return;
    }
    let argv = |img: &Path, sw: i32, sh: i32, [rw, rh, hp, vp]: [i32; 4], busiest: bool| {
        let mut a = vec![scripts("images/least-busy-region-venv.sh")];
        for (flag, v) in [
            ("--screen-width", sw),
            ("--screen-height", sh),
            ("--width", rw),
            ("--height", rh),
            ("--horizontal-padding", hp),
            ("--vertical-padding", vp),
        ] {
            a.extend([flag.to_string(), v.to_string()]);
        }
        a.push(img.to_str().unwrap().to_string());
        if busiest {
            a.push("--busiest".into());
        }
        a
    };
    let props = |img: &Path, sw: i32, sh: i32, [rw, rh, hp, vp]: [i32; 4], busiest: bool| {
        format!(
            "imagePath: {}; screenWidth: {sw}; screenHeight: {sh}; regionWidth: {rw}; \
             regionHeight: {rh}; horizontalPadding: {hp}; verticalPadding: {vp}; \
             busiest: {busiest}",
            q(img)
        )
    };
    // What the background widgets ask for when they set nothing else.
    let plain = [300, 200, 200, 200];
    for img in images() {
        for (sw, sh, region, busiest) in [
            (1920, 1080, plain, false),
            (1920, 1080, plain, true),
            (1280, 720, [450, 200, 50, 80], false),
            (2560, 1440, [5000, 200, 200, 200], false),
        ] {
            same(
                "LeastBusyRegion",
                &props(&img, sw, sh, region, busiest),
                &argv(&img, sw, sh, region, busiest),
                true,
            );
        }
    }
    // An unreadable image: the script prints nothing, the type ends without `found`.
    let broken = wallpapers().join("broken.png");
    same(
        "LeastBusyRegion",
        &props(&broken, 1920, 1080, plain, false),
        &argv(&broken, 1920, 1080, plain, false),
        false,
    );
}

#[test]
fn image_regions_match_script() {
    if !have_tools() {
        return;
    }
    let argv = |img: &Path, mw: i32, mh: i32| {
        bash(format!(
            "{} --hyprctl --image '{}' --max-width {mw} --max-height {mh} ",
            scripts("images/find-regions-venv.sh"),
            img.to_str().unwrap().replace('\'', "'\\''")
        ))
    };
    let props = |img: &Path, mw: i32, mh: i32| {
        format!("imagePath: {}; maxWidth: {mw}; maxHeight: {mh}", q(img))
    };
    for img in images() {
        for (mw, mh) in [(1920, 1080), (300, 200), (4000, 4000)] {
            same(
                "ImageRegions",
                &props(&img, mw, mh),
                &argv(&img, mw, mh),
                true,
            );
        }
    }
    let missing = wallpapers().join("missing it's.png");
    same(
        "ImageRegions",
        &props(&missing, 100, 100),
        &argv(&missing, 100, 100),
        false,
    );
}

#[test]
fn text_color_matches_script() {
    if !have_tools() {
        return;
    }
    let argv = |img: &Path, [x, y, w, h]: [f64; 4]| {
        bash(format!(
            "magick {} +repage -crop {w}x{h}+{x}+{y} png:- | {}",
            img.to_str().unwrap(),
            scripts("images/text-color-venv.sh")
        ))
    };
    let props = |img: &Path, [x, y, w, h]: [f64; 4]| {
        format!(
            "imagePath: {}; cropX: {x}; cropY: {y}; cropWidth: {w}; cropHeight: {h}",
            q(img)
        )
    };
    for img in images() {
        for crop in [
            [0.0, 0.0, 50.0, 30.0],
            [10.5, 3.25, 120.0, 41.5],
            [5.0, 5.0, 1.0, 1.0],
        ] {
            same("TextColor", &props(&img, crop), &argv(&img, crop), true);
        }
    }
    // magick gives the script no data: both report its `{ error }`.
    let broken = wallpapers().join("broken.png");
    let crop = [0.0, 0.0, 10.0, 10.0];
    same(
        "TextColor",
        &props(&broken, crop),
        &argv(&broken, crop),
        true,
    );
}

fn tree(d: &Path) -> Vec<(String, Vec<u8>)> {
    let mut v = Vec::new();
    let mut stack = vec![d.to_path_buf()];
    while let Some(p) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&p) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                let rel = p.strip_prefix(d).unwrap().to_string_lossy().into_owned();
                v.push((rel, std::fs::read(&p).unwrap()));
            }
        }
    }
    v.sort();
    v
}

const THUMB_DIR: &str = "IMGJOBS_THUMB_DIR";
const THUMB_SIZE: &str = "IMGJOBS_THUMB_SIZE";
const THUMB_SCRIPT: &str = "IMGJOBS_THUMB_SCRIPT";

/// Each size's script and type run in a process of their own (`testenv::run_in_own_process`)
/// with a fresh home (`testenv::home_vars`), where the thumbnails go.
#[test]
fn thumbnails_match_magick_script() {
    const TEST: &str = concat!(module_path!(), "::thumbnails_match_magick_script");
    if testenv::in_own_process(TEST) {
        let var = |k| std::env::var(k).unwrap();
        let (dir, size) = (var(THUMB_DIR), var(THUMB_SIZE));
        if var(THUMB_SCRIPT) == "1" {
            // The magick script of the original's command (what ran where GnomeDesktop's thumbnailer,
            // tried first, was not installed).
            let status = std::process::Command::new("bash")
                .arg("-c")
                .arg(format!(
                    "{} --size {size} -d {dir}",
                    scripts("thumbnails/generate-thumbnails-magick.sh"),
                ))
                .status()
                .unwrap();
            assert!(status.success(), "{status}");
        } else {
            let r = run(
                "DirectoryThumbnails",
                &format!("directory: {}; size: \"{size}\"", q(Path::new(&dir))),
            );
            // `progress` and `fileGenerated` for every file of the directory.
            let files = std::fs::read_dir(&dir).unwrap().count();
            assert_eq!(r.reports, 2 * files);
        }
        return;
    }
    if !have_tools() {
        return;
    }
    let walls_dir = tempfile::tempdir().unwrap();
    let walls = walls_dir.path();
    for img in images() {
        std::fs::copy(&img, walls.join(img.file_name().unwrap())).unwrap();
    }
    std::fs::copy(
        wallpapers().join("syn_anim.gif"),
        walls.join("anim one.gif"),
    )
    .unwrap();
    std::fs::write(walls.join("notes.txt"), "x").unwrap();
    for size in ["normal", "large", "x-large", "xx-large"] {
        let mut trees = Vec::new();
        for script in [true, false] {
            let home = tempfile::tempdir().unwrap();
            let mut vars = testenv::home_vars(home.path());
            vars.extend([
                (THUMB_DIR, walls.into()),
                (THUMB_SIZE, size.into()),
                (THUMB_SCRIPT, if script { "1" } else { "0" }.into()),
            ]);
            testenv::run_in_own_process(TEST, vars);
            trees.push(tree(&home.path().join(".cache/thumbnails")));
        }
        assert!(
            !trees[0].is_empty(),
            "{size}: the script wrote no thumbnails"
        );
        let names = |t: &[(String, Vec<u8>)]| t.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>();
        assert_eq!(
            names(&trees[1]),
            names(&trees[0]),
            "{size}: thumbnail files"
        );
        for ((n, a), (_, b)) in trees[0].iter().zip(&trees[1]) {
            let (ia, ib) = (
                image::load_from_memory(a).unwrap(),
                image::load_from_memory(b).unwrap(),
            );
            assert_eq!(
                (ia.width(), ia.height(), ia.to_rgba8().into_raw()),
                (ib.width(), ib.height(), ib.to_rgba8().into_raw()),
                "{size}/{n}: pixels"
            );
        }
    }
}
