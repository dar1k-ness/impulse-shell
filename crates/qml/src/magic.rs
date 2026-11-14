//! `file PATH` through libmagic, the library the `file` command is built on.
use std::path::Path;

fn open(flags: magic::cookie::Flags) -> Option<magic::Cookie<magic::cookie::Load>> {
    magic::Cookie::open(flags)
        .ok()
        .and_then(|c| c.load(&Default::default()).ok())
}

thread_local! {
    static COOKIE: Option<magic::Cookie<magic::cookie::Load>> = open(magic::cookie::Flags::default());
    static MIME_TYPE: Option<magic::Cookie<magic::cookie::Load>> =
        open(magic::cookie::Flags::MIME_TYPE);
    static MIME_TYPE_FOLLOW: Option<magic::Cookie<magic::cookie::Load>> =
        open(magic::cookie::Flags::MIME_TYPE | magic::cookie::Flags::SYMLINK);
}

/// What `file PATH` prints after `PATH: `.
pub fn describe(path: &Path) -> Option<String> {
    COOKIE.with(|c| c.as_ref()?.file(path).ok())
}

/// The MIME type of the file at `path` (`file -b --mime-type PATH` without the newline).
/// With `follow` a symbolic link is read as the file it points to (`file -L`), otherwise it
/// is `inode/symlink`. `None` when the path cannot be read.
pub fn mime_type(path: &Path, follow: bool) -> Option<String> {
    if follow {
        std::fs::metadata(path).ok()?;
        MIME_TYPE_FOLLOW.with(|c| c.as_ref()?.file(path).ok())
    } else {
        std::fs::symlink_metadata(path).ok()?;
        MIME_TYPE.with(|c| c.as_ref()?.file(path).ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// The MIME type DirectoryIcon asks for against `file --mime-type -b PATH` itself, over
    /// the kinds of entry a folder holds (images the original shows inline, other MIME types, special
    /// files); a path that cannot be read has no type.
    #[test]
    fn mime_matches_file() {
        if !crate::testenv::require("file") {
            return;
        }
        let src = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../qml/modules/common/widgets/DirectoryIcon.qml"
        ))
        .unwrap();
        assert!(src.contains("FileMimeType {") && src.contains("path: fileModelData.filePath"));
        let d = tempfile::tempdir().unwrap();
        let p = |n: &str| d.path().join(n);
        std::fs::write(
            p("a.png"),
            b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01\x08\x02\0\0\0",
        )
        .unwrap();
        std::fs::write(
            p("a.jpg"),
            b"\xff\xd8\xff\xe0\0\x10JFIF\0\x01\x01\0\0\x01\0\x01\0\0",
        )
        .unwrap();
        std::fs::write(p("a.gif"), b"GIF89a\x01\0\x01\0\0\0\0;").unwrap();
        std::fs::write(
            p("a.webp"),
            b"RIFF\x1a\0\0\0WEBPVP8L\x0d\0\0\0\x2f\0\0\0\x10\x07\x10\x11\x11\x88\x88\xfe\x07\0",
        )
        .unwrap();
        std::fs::write(p("a.tiff"), b"II*\0\x08\0\0\0\0\0").unwrap();
        std::fs::write(
            p("a.svg"),
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1\" height=\"1\"/>\n",
        )
        .unwrap();
        std::fs::write(
            p("x.svg"),
            "<?xml version=\"1.0\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\"/>\n",
        )
        .unwrap();
        std::fs::write(p("t.txt"), "hello\n").unwrap();
        std::fs::write(p("u.txt"), "привет, мир\n").unwrap();
        std::fs::write(p("l.txt"), b"caf\xe9\n").unwrap();
        std::fs::write(p("s.sh"), "#!/bin/sh\necho hi\n").unwrap();
        std::fs::write(p("p.py"), "#!/usr/bin/env python3\nprint(1)\n").unwrap();
        std::fs::write(p("j.json"), "{\"a\": [1, 2]}\n").unwrap();
        std::fs::write(p("h.html"), "<!DOCTYPE html><html><body></body></html>\n").unwrap();
        std::fs::write(p("doc.pdf"), "%PDF-1.4\n%\u{e2}\u{e3}\n").unwrap();
        std::fs::write(
            p("z.gz"),
            b"\x1f\x8b\x08\0\0\0\0\0\0\x03\x03\0\0\0\0\0\0\0\0\0",
        )
        .unwrap();
        std::fs::write(
            p("zip.zip"),
            b"PK\x05\x06\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0",
        )
        .unwrap();
        std::fs::write(p("b.bin"), [0u8, 1, 2, 3, 255, 254]).unwrap();
        std::fs::write(p("empty"), b"").unwrap();
        std::fs::write(p("имя с пробелом.png"), b"\x89PNG\r\n\x1a\n").unwrap();
        std::fs::create_dir(p("dir")).unwrap();
        std::os::unix::fs::symlink(p("a.png"), p("link.png")).unwrap();
        std::os::unix::fs::symlink(p("nowhere"), p("dangling")).unwrap();
        let fifo = std::ffi::CString::new(p("fifo").to_str().unwrap()).unwrap();
        // SAFETY: a NUL-terminated path.
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o644) }, 0);
        let mut paths: Vec<std::path::PathBuf> = std::fs::read_dir(d.path())
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        paths.sort();
        paths.extend([p("missing.png"), "/dev/null".into(), "/".into(), "".into()]);
        for path in paths {
            for follow in [false, true] {
                let readable = if follow {
                    std::fs::metadata(&path).is_ok()
                } else {
                    std::fs::symlink_metadata(&path).is_ok()
                };
                let real = readable.then(|| {
                    let o = Command::new("file")
                        .args(["--mime-type", "-b"])
                        .args(follow.then_some("-L"))
                        .arg(&path)
                        .output()
                        .unwrap();
                    assert_eq!(o.status.code(), Some(0), "{path:?}");
                    String::from_utf8(o.stdout).unwrap().trim_end().to_string()
                });
                assert_eq!(mime_type(&path, follow), real, "{path:?} follow {follow}");
            }
        }
    }
}
