//! Cached wallpaper thumbnails after a restart: the `Wallpapers` thumbnailer (an in-process job
//! outside any window) finishes at once, and WallpaperDirectoryItem's `thumbnailGenerated`
//! handler resets an Error ThumbnailImage's `source` to its `thumbnailPath`, which drops the
//! `source` binding. Its output must wait for the frame that lays the delegates out at their
//! final size, so the image loads the cached thumbnail and keeps following its size.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::scene;
use std::path::PathBuf;

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

#[test]
fn cached_thumbnail_survives_the_generated_signal() {
    let tmp = std::env::temp_dir().join(format!("ri-thumb-restart-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let (cache, walls) = (tmp.join("thumbs"), tmp.join("walls"));
    std::fs::create_dir_all(cache.join("large")).unwrap();
    std::fs::create_dir_all(&walls).unwrap();
    std::fs::write(cache.join("large/a.png"), PNG).unwrap();
    let mut rt = Runtime::new(Some(ri()));
    // Process output and image loads report on channels that wake the world's services.
    let wake = testenv::Wakeups::new();
    rt.world.borrow().waker.set_ping(wake.ping());
    let dirs: Vec<PathBuf> = [
        "modules/common",
        "modules/common/functions",
        "modules/common/utils",
    ]
    .iter()
    .map(|d| ri().join(d))
    .collect();
    rt.load_singletons_where(&dirs, &|n| {
        ["Directories", "FileUtils", "StringUtils"].contains(&n)
    });
    let f = tmp.join("t.qml");
    std::fs::write(
        &f,
        format!(
            r#"
import QtQuick
import Impulse
Item {{
    id: root
    width: 300; height: 200
    property real cellW: 266
    property var img: im
    property var gen: g
    DirectoryThumbnails {{
        id: g
        directory: "{walls}"
        size: "large"
        onFinished: {{
            if (im.status !== Image.Error) return;
            im.source = "";
            im.source = im.thumbnailPath;
        }}
    }}
    Item {{
        width: root.cellW; height: 174
        Image {{
            id: im
            anchors.fill: parent
            sourceSize: Qt.size(width, height)
            property string sizeName: (sourceSize.width <= 256 && sourceSize.height <= 256) ? "large" : "x-large"
            property string thumbnailPath: "{cache}/" + sizeName + "/a.png"
            source: thumbnailPath
        }}
    }}
}}
"#,
            walls = walls.display(),
            cache = cache.display()
        ),
    )
    .unwrap();
    let root = rt.load_file(&f).unwrap();
    rt.settle();
    let item = |rt: &mut Runtime, n: &str| match rt.world.borrow_mut().read(root, n) {
        Some(Value::Item(i)) => i,
        v => panic!("{n}: {v:?}"),
    };
    let (img, job) = (item(&mut rt, "img"), item(&mut rt, "gen"));
    let read = |rt: &mut Runtime, i: usize, p: &str| {
        rt.world.borrow_mut().read(i, p).unwrap_or(Value::Undefined)
    };
    let mut fonts = scene::Fonts::default();
    {
        let mut w = rt.world.borrow_mut();
        w.frame_paced = true;
        w.polish_wins.insert(root);
        w.frame_wins = Some(std::iter::once(root).collect());
    }
    // First frame: the delegate at its first width (x-large, not cached: Error).
    scene::layout_window(&mut rt, &mut fonts, root, [300.0, 200.0]);
    rt.settle();
    rt.world.borrow_mut().frame_seq += 1;
    assert_eq!(read(&mut rt, img, "status").to_number(), 3.0);
    rt.world
        .borrow_mut()
        .write(job, "running", Value::Bool(true));
    rt.settle();
    // The job (in process, on the work pool) is done at once; the grid settles its cells
    // narrower before the next frame.
    workpool::wait_idle();
    qml::native::poll_processes(&mut rt.world.borrow_mut());
    rt.settle();
    rt.world
        .borrow_mut()
        .write(root, "cellW", Value::Num(229.0));
    rt.settle();
    scene::layout_window(&mut rt, &mut fonts, root, [300.0, 200.0]);
    rt.settle();
    rt.world.borrow_mut().frame_seq += 1;
    let ep = rt.process_fd();
    // The readiness is taken before the job is polled: a report after the take pings.
    loop {
        rt.world.borrow().waker.take();
        qml::native::poll_processes(&mut rt.world.borrow_mut());
        rt.settle();
        if !read(&mut rt, job, "running").truthy() {
            break;
        }
        wake.wait(ep, None);
    }
    assert!(
        !read(&mut rt, job, "running").truthy(),
        "thumbnailer never reported"
    );
    // The source binding still follows the size: the cached large thumbnail is shown.
    rt.world
        .borrow_mut()
        .write(root, "cellW", Value::Num(230.0));
    rt.settle();
    scene::layout_window(&mut rt, &mut fonts, root, [300.0, 200.0]);
    rt.settle();
    let src = read(&mut rt, img, "source").to_js_string();
    assert!(src.ends_with("/large/a.png"), "source {src}");
    assert_eq!(read(&mut rt, img, "status").to_number(), 1.0, "Image.Ready");
    let _ = std::fs::remove_dir_all(&tmp);
}
