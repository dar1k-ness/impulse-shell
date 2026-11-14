//! Wallpaper thumbnails of a directory, made in process whatever is installed: the same file
//! set and skips as the original's `generate-thumbnails-magick.sh`, the files in parallel, with what
//! its `thumbgen.py` (GnomeDesktop's thumbnail factory) adds: a thumbnail is remade when its
//! source changed ([`is_current`]) and every file is reported as it is done
//! ([`generate_dir_until`]'s `progress`). The cache names are not the magick script's: its own
//! `urlencode` (`И` as `%418`, `!'` escaped) names files `ThumbnailImage.qml` never looks up,
//! so the wallpaper selector showed no thumbnail for any path outside `[a-zA-Z0-9.~_-/()*]`
//! (e.g. `~/Изображения`). Names here are the md5 of [`file_uri`], the URI ThumbnailImage hashes.
use md5::{Digest, Md5};
use rayon::prelude::*;
use std::path::{Path, PathBuf};

/// `get_thumbnail_size`.
pub fn size_px(name: &str) -> u32 {
    match name {
        "large" => 256,
        "x-large" => 512,
        "xx-large" => 1024,
        _ => 128,
    }
}

/// `file://` + the path with each segment as `encodeURIComponent` leaves it: ThumbnailImage's
/// `Qt.md5("file://" + path.split("/").map(encodeURIComponent).join("/"))` input.
pub fn file_uri(abs: &[u8]) -> String {
    let mut s = String::from("file://");
    for &b in abs {
        if b.is_ascii_alphanumeric() || b"/-_.!~*'()".contains(&b) {
            s.push(b as char);
        } else {
            s += &format!("%{b:02X}");
        }
    }
    s
}

/// `$CACHE_DIR/<md5 of file_uri(abs)>.png`.
pub fn cache_file(cache_dir: &Path, abs: &Path) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    let h = Md5::digest(file_uri(abs.as_os_str().as_bytes()).as_bytes());
    let hex: String = h.iter().map(|b| format!("{b:02x}")).collect();
    cache_dir.join(format!("{hex}.png"))
}

/// `case "${abs_path,,}" in *.gif|*.mp4|*.webm|*.mkv|*.avi|*.mov) return`.
pub fn skipped(abs: &Path) -> bool {
    let l = abs.to_string_lossy().to_lowercase();
    [".gif", ".mp4", ".webm", ".mkv", ".avi", ".mov"]
        .iter()
        .any(|e| l.ends_with(e))
}

/// `for f in "$TARGET"/*; do [ -f "$f" ] || continue`: non-hidden entries that are (links to)
/// regular files.
pub fn dir_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return vec![];
    };
    let mut v: Vec<PathBuf> = rd
        .flatten()
        .filter(|e| !e.file_name().as_encoded_bytes().starts_with(b"."))
        .map(|e| dir.join(e.file_name()))
        .filter(|p| p.is_file())
        .collect();
    v.sort();
    v
}

/// Whether `thumb` is a thumbnail of `src` as it is now (Thumbnail Managing Standard,
/// "Detect Modifications"): its `Thumb::MTime` is the source's modification time. One written
/// without that key (by another program) or whose source cannot be read is kept.
pub fn is_current(src: &Path, thumb: &Path) -> bool {
    if !thumb.is_file() {
        return false;
    }
    let mtime = mtime_secs(src);
    let stored = crate::magick::png_text(thumb, "Thumb::MTime");
    mtime.is_none() || stored.is_none() || stored == mtime
}

/// `src`'s modification time in whole seconds since the epoch, as `Thumb::MTime` holds it.
pub fn mtime_secs(src: &Path) -> Option<String> {
    let m = std::fs::metadata(src).and_then(|m| m.modified()).ok()?;
    let d = m.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(d.as_secs().to_string())
}

/// `generate_thumbnail "$f"` for one file: whether `resize` ran, that is the file had no
/// current thumbnail ([`is_current`]).
/// `resize(src, N, out)` is the `magick src -resize NxN out` step.
pub fn thumbnail<R: Fn(&Path, u32, &Path) + Sync>(
    src: &Path,
    size: u32,
    cache_dir: &Path,
    resize: &R,
) -> bool {
    let Ok(abs) = std::fs::canonicalize(src) else {
        return false;
    };
    if skipped(&abs) {
        return false;
    }
    let out = cache_file(cache_dir, &abs);
    let _ = std::fs::create_dir_all(cache_dir);
    if is_current(&abs, &out) {
        return false;
    }
    resize(&abs, size, &out);
    true
}

/// One file of a directory run is done: `done` of `total` so far (in the order they finish),
/// and whether its thumbnail was made by this run.
pub struct Progress<'a> {
    pub done: usize,
    pub total: usize,
    pub file: &'a Path,
    pub made: bool,
}

/// `generate-thumbnails-magick.sh --size NAME -d DIR`: stdout and exit code.
pub fn generate_dir<R: Fn(&Path, u32, &Path) + Sync>(
    dir: &Path,
    size_name: &str,
    home: &Path,
    resize: &R,
) -> (String, i32) {
    generate_dir_until(dir, size_name, home, resize, &|| false, &|_| ())
}

/// `generate_dir` that stops starting new thumbnails once `stop` returns true and reports
/// each file to `progress` when it is done, from the thread that did it, one at a time.
pub fn generate_dir_until<R: Fn(&Path, u32, &Path) + Sync>(
    dir: &Path,
    size_name: &str,
    home: &Path,
    resize: &R,
    stop: &(dyn Fn() -> bool + Sync),
    progress: &(dyn Fn(Progress) + Sync),
) -> (String, i32) {
    if !dir.is_dir() {
        return (format!("Directory not found: {}\n", dir.display()), 2);
    }
    let cache = home.join(".cache/thumbnails").join(size_name);
    let size = size_px(size_name);
    let files = dir_files(dir);
    // Counted and reported under one lock: `progress` sees 1, 2, ... in that order.
    let done = std::sync::Mutex::new(0);
    crate::pool::scoped(|| {
        files.par_iter().for_each(|f| {
            if stop() {
                return;
            }
            let made = thumbnail(f, size, &cache, resize);
            let mut done = done.lock().unwrap_or_else(|e| e.into_inner());
            *done += 1;
            progress(Progress {
                done: *done,
                total: files.len(),
                file: f,
                made,
            });
        })
    });
    (String::new(), 0)
}
