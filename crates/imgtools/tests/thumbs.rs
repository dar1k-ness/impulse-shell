use imgtools::thumbs::*;
use std::path::Path;

#[test]
fn cache_name_is_what_thumbnail_image_looks_up() {
    // arch-pc: ThumbnailImage.qml's `normal` thumbnail of this wallpaper is 3d0577f9…
    // (the magick script named its `large` one 504cd081…, which nothing displays).
    let f = cache_file(
        Path::new("/c"),
        Path::new("/home/dar1k/Изображения/Wallpapers/random_wallpaper.png"),
    );
    assert_eq!(f, Path::new("/c/3d0577f9c0563da310b34af31e75ce40.png"));
    assert_eq!(
        file_uri("/a b/!'()*~-_.:@&+,;=$#%?/И".as_bytes()),
        "file:///a%20b/!'()*~-_.%3A%40%26%2B%2C%3B%3D%24%23%25%3F/%D0%98"
    );
}

#[test]
fn generates_like_script() {
    let t = std::env::temp_dir().join(format!("thumbs-test-{}", std::process::id()));
    let d = t.join("wp dir");
    std::fs::create_dir_all(d.join("sub")).unwrap();
    for f in [
        "a.png",
        "B.JPG",
        "c.GIF",
        "d.mp4",
        ".hidden.png",
        "sub/e.png",
    ] {
        std::fs::write(d.join(f), b"x").unwrap();
    }
    std::os::unix::fs::symlink(d.join("a.png"), d.join("link.png")).unwrap();
    let done = std::sync::Mutex::new(vec![]);
    let r = generate_dir(&d, "x-large", &t, &|src, size, out| {
        assert_eq!(size, 512);
        std::fs::write(out, b"t").unwrap();
        done.lock()
            .unwrap()
            .push(src.file_name().unwrap().to_str().unwrap().to_string());
    });
    assert_eq!(r, (String::new(), 0));
    let mut done = done.into_inner().unwrap();
    done.sort();
    // link.png resolves to a.png, whose thumbnail already exists by then (or is the same
    // file being written): the script makes at most one per real path, racing like ours.
    assert!(
        done == ["B.JPG", "a.png"] || done == ["B.JPG", "a.png", "a.png"],
        "{done:?}"
    );
    assert!(t.join(".cache/thumbnails/x-large").is_dir());
    let again = std::sync::Mutex::new(0);
    generate_dir(&d, "x-large", &t, &|_, _, _| *again.lock().unwrap() += 1);
    assert_eq!(*again.lock().unwrap(), 0);
    assert_eq!(
        generate_dir(&t.join("nope"), "normal", &t, &|_, _, _| ()),
        (
            format!("Directory not found: {}\n", t.join("nope").display()),
            2
        )
    );
    std::fs::remove_dir_all(&t).unwrap();
}

/// A stopped thumbnail job (the Process set to `running: false`) starts no further thumbnails.
#[test]
fn generate_dir_until_stops_between_files() {
    let t = &std::env::temp_dir().join(format!("thumbs-stop-{}", std::process::id()));
    let d = t.join("pics");
    std::fs::create_dir_all(&d).unwrap();
    for i in 0..64 {
        std::fs::write(d.join(format!("{i}.png")), b"x").unwrap();
    }
    let stop = std::sync::atomic::AtomicBool::new(false);
    let done = std::sync::atomic::AtomicUsize::new(0);
    let r = imgtools::thumbs::generate_dir_until(
        &d,
        "normal",
        t,
        &|_, _, _| {
            done.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            stop.store(true, std::sync::atomic::Ordering::SeqCst);
        },
        &|| stop.load(std::sync::atomic::Ordering::SeqCst),
        &|_| (),
    );
    assert_eq!(r, (String::new(), 0));
    let n = done.load(std::sync::atomic::Ordering::SeqCst);
    let _ = std::fs::remove_dir_all(t);
    assert!(n < 64 / 2, "{n} thumbnails made after stop");
}

/// Every file of the directory is reported once as it is done, counted in the order they
/// finish, with whether its thumbnail was made by this run.
#[test]
fn generate_dir_until_reports_each_file() {
    let t = &std::env::temp_dir().join(format!("thumbs-progress-{}", std::process::id()));
    let d = t.join("pics");
    std::fs::create_dir_all(&d).unwrap();
    for f in ["a.png", "b.png", "c.gif", "d.png"] {
        std::fs::write(d.join(f), b"x").unwrap();
    }
    let run = || {
        let seen = std::sync::Mutex::new(vec![]);
        let r = imgtools::thumbs::generate_dir_until(
            &d,
            "normal",
            t,
            &|_, _, out| std::fs::write(out, b"t").unwrap(),
            &|| false,
            &|p| {
                assert_eq!(p.total, 4);
                let name = p.file.file_name().unwrap().to_str().unwrap().to_string();
                seen.lock().unwrap().push((p.done, name, p.made));
            },
        );
        assert_eq!(r, (String::new(), 0));
        let mut seen = seen.into_inner().unwrap();
        let order: Vec<usize> = seen.iter().map(|s| s.0).collect();
        seen.sort_by(|a, b| a.1.cmp(&b.1));
        let files: Vec<(String, bool)> = seen.into_iter().map(|s| (s.1, s.2)).collect();
        (order, files)
    };
    let (order, files) = run();
    let mut sorted = order.clone();
    sorted.sort();
    assert_eq!(sorted, [1, 2, 3, 4], "{order:?}");
    let made = |m: bool| {
        [("a.png", m), ("b.png", m), ("c.gif", false), ("d.png", m)]
            .map(|(f, m)| (f.to_string(), m))
            .to_vec()
    };
    assert_eq!(files, made(true));
    // A second run finds every thumbnail current: each file is still reported, none as made.
    assert_eq!(run().1, made(false));
    let _ = std::fs::remove_dir_all(t);
}
