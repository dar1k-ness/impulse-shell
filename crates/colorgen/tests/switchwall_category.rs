//! `category.txt` is replaced atomically and only by the latest wallpaper switch.
use colorgen::switchwall::{categorize_later, next_category_generation, wait_background};

#[test]
fn slow_older_categorization_does_not_win() {
    let d = std::env::temp_dir().join(format!("colorgen-category-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    let file = d.join("wallpaper/category.txt");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    let (finish, finished) = std::sync::mpsc::channel::<()>();
    let old = next_category_generation();
    categorize_later(old, file.clone(), move || {
        let _ = finished.recv();
        Some("anime\n".into())
    });
    let new = next_category_generation();
    categorize_later(new, file.clone(), || Some("nature".into()));
    // The newer one has written its result; only then does the older one finish.
    testenv::wait_exists(&file);
    finish.send(()).unwrap();
    wait_background();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "nature\n");
    let leftovers: Vec<_> = std::fs::read_dir(file.parent().unwrap())
        .unwrap()
        .flatten()
        .map(|e| e.file_name())
        .collect();
    assert_eq!(leftovers, vec![std::ffi::OsString::from("category.txt")]);
    let _ = std::fs::remove_dir_all(&d);
}
