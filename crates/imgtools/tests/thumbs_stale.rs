//! A directory run remakes the thumbnail of a file that changed since it was made (Thumbnail
//! Managing Standard, "Detect Modifications": a thumbnail is valid while its `Thumb::MTime`
//! is the source's modification time), and keeps the others.
use imgtools::thumbs::{cache_file, generate_dir};
use std::path::Path;

fn set_mtime(p: &Path, secs: u64) {
    let t = std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs);
    std::fs::File::options()
        .write(true)
        .open(p)
        .unwrap()
        .set_modified(t)
        .unwrap();
}

/// The thumbnail writer of the shell (`jobs_files::thumbnail_code`): a PNG with `Thumb::MTime`.
fn resize(src: &Path, size: u32, out: &Path) {
    let m = std::fs::metadata(src).unwrap().modified().unwrap();
    let m = m
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        .to_string();
    imgtools::magick::resize_to_png_text(src, size, out, &[("Thumb::MTime", &m)]).unwrap();
}

#[test]
fn changed_source_gets_a_new_thumbnail() {
    let t = std::env::temp_dir().join(format!("thumbs-stale-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&t);
    let d = t.join("walls");
    std::fs::create_dir_all(&d).unwrap();
    let (a, b) = (d.join("a.png"), d.join("b.png"));
    for (f, px) in [(&a, [255, 0, 0]), (&b, [0, 0, 255])] {
        image::RgbImage::from_pixel(8, 8, image::Rgb(px))
            .save(f)
            .unwrap();
        set_mtime(f, 1_000_000);
    }
    let made = std::sync::Mutex::new(vec![]);
    let run = || {
        let r = generate_dir(&d, "normal", &t, &|src, size, out| {
            resize(src, size, out);
            let name = src.file_name().unwrap().to_str().unwrap().to_string();
            made.lock().unwrap().push(name);
        });
        assert_eq!(r, (String::new(), 0));
        let mut v = std::mem::take(&mut *made.lock().unwrap());
        v.sort();
        v
    };
    assert_eq!(run(), ["a.png", "b.png"]);
    assert_eq!(run(), [""; 0], "unchanged sources keep their thumbnails");
    // The wallpaper is replaced by another picture under the same name.
    image::RgbImage::from_pixel(8, 8, image::Rgb([0, 255, 0]))
        .save(&a)
        .unwrap();
    set_mtime(&a, 2_000_000);
    assert_eq!(run(), ["a.png"], "only the changed source is remade");
    let thumb = cache_file(
        &t.join(".cache/thumbnails/normal"),
        &std::fs::canonicalize(&a).unwrap(),
    );
    let px = image::open(&thumb).unwrap().to_rgb8();
    assert_eq!(
        px.get_pixel(0, 0).0,
        [0, 255, 0],
        "the thumbnail shows the new picture"
    );
    assert_eq!(
        imgtools::magick::png_text(&thumb, "Thumb::MTime").as_deref(),
        Some("2000000")
    );
    assert_eq!(run(), [""; 0]);
    let _ = std::fs::remove_dir_all(&t);
}
