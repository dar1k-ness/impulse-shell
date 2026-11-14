//! Path tests for the `PathKind` type (crate::procs) and the thumbnail writer of
//! `DirectoryThumbnails` (crate::imgjobs).
use std::path::Path;

pub use crate::qurl::trim_file_protocol;

pub fn validate_dir_out(path: &str) -> &'static str {
    let p = Path::new(path);
    if p.is_dir() {
        "dir\n"
    } else if p.is_file() {
        "file\n"
    } else {
        "invalid\n"
    }
}

/// Exit code (0: the thumbnail is current, 1: it was (re)generated, 2: it could not be) and
/// stderr. The original's `[ -f T ] && exit 0 || { magick S -resize NxN T && exit 1; }` broke on `'`
/// in a path, failed when the size directory did not exist yet, could leave a half-written
/// PNG that later passed the `-f` test, never noticed a changed source and exited 1 (reload)
/// on failure too. Here the thumbnail is written as the Freedesktop spec asks: directories
/// 0700, the file 0600 via a temporary file and a rename, with `Thumb::URI`/`Thumb::MTime`,
/// and regenerated when its `Thumb::MTime` no longer matches the source.
pub fn thumbnail_code(source: &str, size: &str, target: &str) -> (i32, String) {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    let src = Path::new(trim_file_protocol(source));
    let t = Path::new(target);
    let mtime = std::fs::metadata(src)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs().to_string());
    if t.is_file() {
        let stored = imgtools::magick::png_text(t, "Thumb::MTime");
        if mtime.is_none() || stored.is_none() || stored == mtime {
            return (0, String::new());
        }
    }
    let fail = |e: &dyn std::fmt::Display| (2, format!("thumbnail {}: {e}\n", src.display()));
    let Ok(n) = size.parse::<u32>() else {
        return fail(&format!("invalid size {size:?}"));
    };
    let (Some(dir), Some(name)) = (t.parent(), t.file_name()) else {
        return fail(&"invalid thumbnail path");
    };
    if let Err(e) = std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
    {
        return fail(&e);
    }
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = dir.join(format!(
        ".{}.{}-{seq}.tmp",
        name.to_string_lossy(),
        std::process::id()
    ));
    let uri = file_uri(src);
    let mut text = vec![("Thumb::URI", uri.as_str())];
    if let Some(m) = &mtime {
        text.push(("Thumb::MTime", m));
    }
    let r = imgtools::magick::resize_to_png_text(src, n, &tmp, &text).and_then(|()| {
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
        Ok(std::fs::rename(&tmp, t)?)
    });
    match r {
        Ok(()) => (1, String::new()),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            fail(&e)
        }
    }
}

/// `file://` + the absolute path as ThumbnailImage hashes it ([`imgtools::thumbs::file_uri`]).
fn file_uri(p: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let abs = std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
    imgtools::thumbs::file_uri(abs.as_os_str().as_bytes())
}
