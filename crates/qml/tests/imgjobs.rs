//! The image types of the `Impulse` module: what their properties ask `imgtools` for, and how
//! a run reports under a frame-pacing host.
use qml::imgjobs::{Job, start};
use qml::runtime::Runtime;
use qml::value::Value;
use std::path::PathBuf;

fn ri() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../qml")
}

fn load(ty: &str, props: &[(&str, Value)]) -> (Runtime, usize) {
    let mut rt = Runtime::new(Some(ri()));
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let f = std::env::temp_dir().join(format!("imgjobs-{ty}-{}-{n}.qml", std::process::id()));
    std::fs::write(&f, format!("import QtQuick\nimport Impulse\n{ty} {{}}\n")).unwrap();
    let root = rt.load_file(&f).unwrap();
    std::fs::remove_file(&f).unwrap();
    let p = root;
    for (k, v) in props {
        rt.world.borrow_mut().write(p, k, v.clone());
    }
    rt.settle();
    (rt, p)
}

fn s(x: &str) -> Value {
    Value::Str(x.into())
}

fn job(rt: &Runtime, p: usize) -> Option<Job> {
    start(&mut rt.world.borrow_mut(), p)
}

#[test]
fn least_busy_region_asks_for_its_properties() {
    for busiest in [false, true] {
        let (rt, p) = load(
            "LeastBusyRegion",
            &[
                ("imagePath", s("/w/a b's.png")),
                ("screenWidth", Value::Num(1920.0)),
                ("screenHeight", Value::Num(1080.0)),
                ("regionWidth", Value::Num(300.0)),
                ("regionHeight", Value::Num(200.0)),
                ("horizontalPadding", Value::Num(50.0)),
                ("verticalPadding", Value::Num(40.0)),
                ("busiest", Value::Bool(busiest)),
            ],
        );
        let Some(Job::LeastBusy(a)) = job(&rt, p) else {
            panic!()
        };
        assert_eq!(
            (
                a.image.as_str(),
                [a.screen_width, a.screen_height],
                [a.width, a.height],
                [a.horizontal_padding, a.vertical_padding],
                a.busiest,
            ),
            ("/w/a b's.png", [1920, 1080], [300, 200], [50, 40], busiest)
        );
    }
}

#[test]
fn image_regions_asks_for_its_properties() {
    let (rt, p) = load(
        "ImageRegions",
        &[
            ("imagePath", s("/tmp/q/image-DP-1")),
            ("maxWidth", Value::Num(1728.0)),
            ("maxHeight", Value::Num(972.0)),
        ],
    );
    let Some(Job::Regions(a)) = job(&rt, p) else {
        panic!()
    };
    assert_eq!(
        (a.image.as_str(), a.max_width, a.max_height),
        ("/tmp/q/image-DP-1", 1728, 972)
    );
}

/// A script sets the properties of an image type that the document does not bind, as
/// RegionSelection.qml starts its ImageRegions (`imageDetectionProcess.running = true`).
#[test]
fn a_script_writes_the_properties_of_an_image_type() {
    for (ty, writes, expect) in [
        (
            "ImageRegions",
            "o.imagePath = '/nonexistent/q/a'; o.maxWidth = 1728; o.maxHeight = 972; o.running = true",
            "/nonexistent/q/a,1728,972,true",
        ),
        (
            "LeastBusyRegion",
            "o.imagePath = '/nonexistent/q/a'; o.screenWidth = 1920; o.busiest = true; o.running = true",
            "/nonexistent/q/a,1920,true,true",
        ),
        (
            "TextColor",
            "o.imagePath = '/nonexistent/q/a'; o.cropX = 536; o.cropHeight = 26; o.running = true",
            "/nonexistent/q/a,536,26,true",
        ),
        (
            "DirectoryThumbnails",
            "o.directory = '/nonexistent/q'; o.size = 'large'; o.running = true",
            "/nonexistent/q,large,true",
        ),
    ] {
        let read = (writes.split("; "))
            .map(|w| w.split_once(" = ").unwrap().0)
            .collect::<Vec<_>>()
            .join(", ");
        let src = format!(
            "import QtQuick\nimport Impulse\nQtObject {{\n    property {ty} o: {ty} {{}}\n    \
             function go() {{ {writes}; return [{read}].join(',') }}\n}}\n"
        );
        let mut rt = Runtime::new(Some(ri()));
        let f = std::env::temp_dir().join(format!("imgjobs-w-{ty}-{}.qml", std::process::id()));
        std::fs::write(&f, src).unwrap();
        let root = rt.load_file(&f).unwrap();
        std::fs::remove_file(&f).unwrap();
        let got = rt.call_method(root, "go", vec![]).unwrap();
        assert_eq!(
            got.map(|v| v.to_js_string()),
            Ok(expect.to_string()),
            "{ty}"
        );
        assert_eq!(rt.errors(), Vec::<String>::new(), "{ty}");
    }
}

#[test]
fn text_color_asks_for_its_crop() {
    let (rt, p) = load(
        "TextColor",
        &[
            ("imagePath", s("/tmp/shot")),
            ("cropX", Value::Num(536.0)),
            ("cropY", Value::Num(236.5)),
            ("cropWidth", Value::Num(47.25)),
            ("cropHeight", Value::Num(26.0)),
        ],
    );
    let Some(Job::TextColor { path, x, y, w, h }) = job(&rt, p) else {
        panic!()
    };
    assert_eq!(
        (path.as_str(), x, y, w, h),
        ("/tmp/shot", 536.0, 236.5, 47.25, 26.0)
    );
}

#[test]
fn directory_thumbnails_takes_the_directory_literally() {
    for (dir, runs) in [
        ("/home/dar1k/Изображения/Wallpapers", true),
        ("file:///home/u/Pictures/Wallpapers", true),
        ("/home/u/My Wallpapers", true),
        ("/home/u/$x", true),
        ("", false),
    ] {
        let (rt, p) = load(
            "DirectoryThumbnails",
            &[("directory", s(dir)), ("size", s("large"))],
        );
        let plain = dir.strip_prefix("file://").unwrap_or(dir);
        match job(&rt, p) {
            Some(Job::Thumbnails { dir: d, size }) => {
                assert!(runs, "{dir}");
                assert_eq!((d.as_str(), size.as_str()), (plain, "large"));
            }
            None => assert!(!runs, "{dir}"),
            _ => panic!(),
        }
    }
}

/// A thumbnail job under a frame-pacing host (`frame_paced`), its worker's output events
/// observed through the Process service's wakeups instead of wall time.
struct PacedJob {
    rt: Runtime,
    p: usize,
    dir: PathBuf,
    woke: std::sync::mpsc::Receiver<()>,
}

impl PacedJob {
    fn start(name: &str) -> PacedJob {
        let dir = std::env::temp_dir().join(format!("imgjobs-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (mut rt, p) = load(
            "DirectoryThumbnails",
            &[
                ("directory", s(dir.to_str().unwrap())),
                ("size", s("large")),
            ],
        );
        let (tx, woke) = std::sync::mpsc::channel();
        rt.world.borrow().waker.set_ping(move || {
            let _ = tx.send(());
        });
        rt.world.borrow_mut().frame_paced = true;
        rt.world.borrow_mut().write(p, "running", Value::Bool(true));
        rt.settle();
        PacedJob { rt, p, dir, woke }
    }

    fn running(&mut self) -> bool {
        let p = self.p;
        self.rt
            .world
            .borrow_mut()
            .read(p, "running")
            .is_some_and(|v| v.truthy())
    }

    fn poll(&mut self) {
        qml::native::poll_processes(&mut self.rt.world.borrow_mut());
        self.rt.settle();
    }

    /// Blocks until the worker sends output (a hung job is nextest's slow-timeout to end).
    fn wait_output(&mut self) {
        self.woke.recv().expect("the runtime's waker went away");
        self.rt
            .world
            .borrow()
            .waker
            .take_one(qml::svc::Svc::Process);
    }

    /// Polls, and waits for the worker's next output while the job still runs.
    fn until_reported(&mut self) {
        loop {
            self.poll();
            if !self.running() {
                return;
            }
            self.wait_output();
        }
    }

    fn wants_any_frame(&self) -> bool {
        self.rt.world.borrow().frame_wanted.contains(&usize::MAX)
    }
}

impl Drop for PacedJob {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A windowless in-process image job (the `Wallpapers` singleton's thumbnailer) reports once
/// the host reported a window that draws and drew a frame since the job started.
#[test]
fn paced_windowless_job_waits_for_a_frame() {
    let mut j = PacedJob::start("gate");
    j.wait_output();
    j.poll();
    assert!(j.running(), "reported before the host reported its windows");
    assert!(j.rt.set_frame_wins([usize::MAX - 1]));
    j.poll();
    assert!(j.running(), "reported before a frame");
    assert!(j.wants_any_frame(), "no frame requested");
    j.rt.world.borrow_mut().frame_seq += 1;
    j.until_reported();
    assert!(!j.wants_any_frame());
}

/// No window can draw a frame (all hidden or unconfigured): the job reports without one.
#[test]
fn paced_windowless_job_without_drawable_windows_reports_at_once() {
    let mut j = PacedJob::start("nowin");
    j.wait_output();
    j.poll();
    assert!(j.running(), "reported before the host reported its windows");
    assert!(j.rt.set_frame_wins([]));
    j.until_reported();
    assert_eq!(j.rt.world.borrow().frame_seq, 0);
    assert!(!j.wants_any_frame());
}
