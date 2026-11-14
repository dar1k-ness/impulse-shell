//! The image types of the `Impulse` module (`qmltypes/Impulse/{least-busy-region,image-regions,
//! text-color,directory-thumbnails}.qmltypes`): setting `running` starts the work on the work
//! pool (`imgtools` does it), and the result reaches the item as its `found` / `progress` /
//! `fileGenerated` / `finished` signals. A run is a `native::Proc`, so `running = false` and
//! the item's destruction stop it and a frame-pacing host holds its result for a frame.
use crate::impulse::Element;
use crate::native::ProcEv;
use crate::runtime::World;
use crate::value::Value;

pub static LEAST_BUSY_REGION: Element = element("LeastBusyRegion");
pub static IMAGE_REGIONS: Element = element("ImageRegions");
pub static TEXT_COLOR: Element = element("TextColor");
pub static DIRECTORY_THUMBNAILS: Element = element("DirectoryThumbnails");

/// An image type has no methods: writing `running` starts and stops its run.
const fn element(name: &'static str) -> Element {
    Element {
        running: Some(crate::native::image_running),
        ..Element::new(name)
    }
}

pub enum Job {
    LeastBusy(imgtools::busy::BusyArgs),
    Regions(imgtools::regions::RegionArgs),
    TextColor {
        path: String,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    },
    Thumbnails {
        dir: String,
        size: String,
    },
}

fn is(w: &World, p: usize, ty: &Element) -> bool {
    w.items[p].types.iter().any(|t| t.as_str() == ty.name)
}

fn s(w: &mut World, p: usize, k: &str) -> String {
    w.read(p, k).map(|v| v.to_js_string()).unwrap_or_default()
}

fn n(w: &mut World, p: usize, k: &str) -> f64 {
    w.read(p, k).map(|v| v.to_number()).unwrap_or(0.0)
}

/// What a run found, the argument of the type's `found`.
pub enum Found {
    Region(imgtools::busy::Busy),
    Regions(Vec<imgtools::regions::Region>),
}

/// Where a run leaves what it found for the main thread to take when the run has exited.
pub type FoundSlot = std::sync::Arc<std::sync::Mutex<Option<Found>>>;

/// Whether `p` is one of the image types.
pub fn is_type(w: &World, p: usize) -> bool {
    [
        &LEAST_BUSY_REGION,
        &IMAGE_REGIONS,
        &TEXT_COLOR,
        &DIRECTORY_THUMBNAILS,
    ]
    .iter()
    .any(|t| is(w, p, t))
}

/// The run for image item `p`, in process on the work pool; `None` when it has nothing to do
/// (no directory).
pub fn start(w: &mut World, p: usize) -> Option<Job> {
    let int = |w: &mut World, k: &str| n(w, p, k) as i64;
    if is(w, p, &LEAST_BUSY_REGION) {
        return Some(Job::LeastBusy(imgtools::busy::BusyArgs {
            image: s(w, p, "imagePath"),
            screen_width: int(w, "screenWidth"),
            screen_height: int(w, "screenHeight"),
            width: int(w, "regionWidth"),
            height: int(w, "regionHeight"),
            horizontal_padding: int(w, "horizontalPadding"),
            vertical_padding: int(w, "verticalPadding"),
            busiest: w.read(p, "busiest").is_some_and(|v| v.truthy()),
            ..Default::default()
        }));
    }
    if is(w, p, &IMAGE_REGIONS) {
        return Some(Job::Regions(imgtools::regions::RegionArgs {
            image: s(w, p, "imagePath"),
            max_width: int(w, "maxWidth"),
            max_height: int(w, "maxHeight"),
        }));
    }
    if is(w, p, &TEXT_COLOR) {
        return Some(Job::TextColor {
            path: s(w, p, "imagePath"),
            x: n(w, p, "cropX"),
            y: n(w, p, "cropY"),
            w: n(w, p, "cropWidth"),
            h: n(w, p, "cropHeight"),
        });
    }
    if !is(w, p, &DIRECTORY_THUMBNAILS) {
        return None;
    }
    let dir = s(w, p, "directory");
    let dir = crate::qurl::trim_file_protocol(&dir).to_string();
    if dir.is_empty() {
        return None;
    }
    let size = s(w, p, "size");
    // Made in process whatever is installed: the shell runs no script for work it does itself.
    Some(Job::Thumbnails { dir, size })
}

/// A thumbnail run of item `p` is done with the file `path`, the `done`th of `total`: its
/// `progress` and `fileGenerated`. The path comes as it is, whatever characters it has.
pub fn file_done(w: &mut World, p: usize, done: usize, total: usize, path: &str) {
    crate::native::emit(
        w,
        p,
        "progress",
        &["completed", "total"],
        vec![Value::Num(done as f64), Value::Num(total as f64)],
    );
    crate::native::emit(w, p, "fileGenerated", &["path"], vec![Value::str(path)]);
}

/// What the run of image item `p` found: its `found`, the region as an object of the fields
/// the widgets read, the regions as a list of `{at: [x, y], size: [width, height]}`.
pub fn found(w: &mut World, p: usize, f: Found) {
    let num = |n: i64| Value::Num(n as f64);
    let object = |fields: Vec<(&str, Value)>| {
        let o = crate::jsrt::object(fields.len());
        for (k, v) in fields {
            crate::jsrt::object_set(&o, k, v);
        }
        o
    };
    let (param, v) = match f {
        Found::Region(b) => (
            "region",
            object(vec![
                ("center_x", num(b.center_x)),
                ("center_y", num(b.center_y)),
                ("width", num(b.width)),
                ("height", num(b.height)),
                ("variance", Value::Num(b.variance)),
                ("dominant_color", Value::str(&b.dominant_color)),
            ]),
        ),
        Found::Regions(rs) => (
            "regions",
            Value::array(
                rs.iter()
                    .map(|r| {
                        object(vec![
                            ("at", Value::array(vec![num(r.x), num(r.y)])),
                            ("size", Value::array(vec![num(r.width), num(r.height)])),
                        ])
                    })
                    .collect(),
            ),
        ),
    };
    crate::native::emit(w, p, "found", &[param], vec![v]);
}

/// What image item `p`'s run printed so far (`buf`; `end`: all of it): TextColor's JSON
/// becomes `found`. A thumbnail run reports its files as [`ProcEv::File`], not as text.
pub fn output(w: &mut World, p: usize, buf: &mut String, end: bool) {
    if is(w, p, &DIRECTORY_THUMBNAILS) {
        buf.clear();
        return;
    }
    if !end {
        return;
    }
    let slot = w.image_found.remove(&p);
    if let Some(f) = slot.and_then(|s| s.lock().ok()?.take()) {
        return found(w, p, f);
    }
    let text = std::mem::take(buf);
    let Ok(v) = crate::json::parse(&text) else {
        return;
    };
    crate::native::emit(w, p, "found", &["colors"], vec![v]);
}

/// The run of image item `p` ended (by itself or stopped).
pub fn finished(w: &mut World, p: usize) {
    w.image_found.remove(&p);
    crate::native::emit(w, p, "finished", &[], vec![]);
}

/// What the job found, its stdout, stderr and exit code. A set `stop` ends a thumbnail run
/// between files; a thumbnail run hands each file to `file_done` as the file is done.
pub fn run(
    job: Job,
    stop: &crate::svc::Stop,
    file_done: &(dyn Fn(imgtools::thumbs::Progress) + Sync),
) -> (Option<Found>, String, String, i32) {
    let found = |f: Option<Found>| {
        let code = if f.is_some() { 0 } else { 1 };
        (f, String::new(), String::new(), code)
    };
    match job {
        Job::LeastBusy(a) => found(
            imgtools::pool::scoped(|| imgtools::busy::least_busy_region(&a)).map(Found::Region),
        ),
        Job::Regions(a) => found(
            imgtools::pool::scoped(|| {
                imgtools::regions::find_regions_as(&a, imgtools::regions::ss_cv::filter_fma())
            })
            .map(Found::Regions),
        ),
        Job::TextColor { path, x, y, w, h } => (
            None,
            imgtools::textcolor::region_json(&path, x, y, w, h) + "\n",
            String::new(),
            0,
        ),
        Job::Thumbnails { dir, size } => {
            let home = std::env::var_os("HOME").unwrap_or_default();
            let (o, c) = imgtools::thumbs::generate_dir_until(
                std::path::Path::new(&dir),
                &size,
                std::path::Path::new(&home),
                &|src, n, out| {
                    let _ = crate::jobs_files::thumbnail_code(
                        &src.to_string_lossy(),
                        &n.to_string(),
                        &out.to_string_lossy(),
                    );
                },
                &|| stop.load(std::sync::atomic::Ordering::Acquire),
                file_done,
            );
            (None, o, String::new(), c)
        }
    }
}

/// Runs `job` on the work pool; its `Stop` is the `Proc`'s, so `running = false` and the
/// item's destruction cancel it.
pub fn spawn(
    w: &mut World,
    p: usize,
    job: Job,
    h: &crate::svc::Handle,
) -> (
    std::sync::mpsc::Receiver<ProcEv>,
    std::sync::Arc<crate::svc::Stop>,
) {
    let (tx, rx) = h.channel();
    let stop = std::sync::Arc::new(crate::svc::Stop::default());
    let st = stop.clone();
    let slot = FoundSlot::default();
    w.image_found.insert(p, slot.clone());
    workpool::long(move || {
        let (found, out, err, code) = run(job, &st, &|f| {
            if !st.load(std::sync::atomic::Ordering::Acquire) {
                let _ = tx.send(ProcEv::File {
                    done: f.done,
                    total: f.total,
                    path: f.file.to_string_lossy().into_owned(),
                });
            }
        });
        if st.load(std::sync::atomic::Ordering::Acquire) {
            return;
        }
        *slot.lock().unwrap_or_else(|e| e.into_inner()) = found;
        if !out.is_empty() {
            let _ = tx.send(ProcEv::Out(out));
        }
        if !err.is_empty() {
            let _ = tx.send(ProcEv::Err(err));
        }
        let _ = tx.send(ProcEv::Exit(code));
    });
    (rx, stop)
}
