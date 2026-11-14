//! `Process` elements that only ask libmagic or the file system: `FileMimeType`
//! (AttachedFileIndicator.qml, DirectoryIcon.qml) and `TranslationLanguages`
//! (Translation.qml).
use crate::jobs::Job;
use crate::runtime::World;
use std::path::Path;

pub static FILE_MIME_TYPE: crate::impulse::Element = crate::impulse::Element {
    job: Some(file_mime_type),
    created: Some(|w, id| {
        w.declare(id, "followSymlinks", "var");
        if let Some(p) = w.items[id].props.get_mut("followSymlinks") {
            p.value = crate::value::Value::Bool(true);
        }
    }),
    ..crate::impulse::Element::new("FileMimeType")
};

/// `Impulse/FileMimeType`: the MIME type of the file `path` on stdout, without a newline;
/// exit code 1 for a path that cannot be read. A symbolic link is read as the file it points
/// to unless `followSymlinks` is false, when it is `inode/symlink`.
fn file_mime_type(w: &mut World, p: usize) -> Option<Job> {
    let path = crate::jobs::text(w, p, "path");
    let follow = crate::jobs::flag(w, p, "followSymlinks");
    Some(Box::new(move |io, _| match mime_type(&path, follow) {
        Some(m) => {
            io.out(m);
            0
        }
        None => 1,
    }))
}

pub static TRANSLATION_LANGUAGES: crate::impulse::Element = crate::impulse::Element {
    job: Some(translation_languages),
    ..crate::impulse::Element::new("TranslationLanguages")
};

/// `Impulse/TranslationLanguages`: the language codes that have a translation file
/// (`<code>.json`) in directory `path`, sorted, one per line; exit code 1 when the
/// directory cannot be read.
fn translation_languages(w: &mut World, p: usize) -> Option<Job> {
    let dir = crate::jobs::text(w, p, "path");
    Some(Box::new(move |io, _| match languages(Path::new(&dir)) {
        Some(l) => {
            if !l.is_empty() {
                io.out(l.join("\n") + "\n");
            }
            0
        }
        None => 1,
    }))
}

/// The MIME type of the file at `path` from libmagic, no newline; `None` for a path that
/// cannot be read.
pub fn mime_type(path: &str, follow: bool) -> Option<String> {
    if path.is_empty() {
        return None;
    }
    crate::magic::mime_type(Path::new(path), follow)
}

/// The language codes a translations directory provides: its `<code>.json` files (the ones
/// TranslationReader can load as `DIR/<code>.json`), sorted; `None` when it cannot be read.
pub fn languages(dir: &Path) -> Option<Vec<String>> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| {
            let e = e.ok()?;
            let name = e.file_name().into_string().ok()?;
            let code = name.strip_suffix(".json")?;
            (!code.is_empty() && e.path().is_file()).then(|| code.to_string())
        })
        .collect();
    v.sort();
    Some(v)
}
