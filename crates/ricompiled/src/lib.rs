//! Rust compiled from the QML tree by `rigen` at build time (see docs/aot.md).
//!
//! The generated modules live in the `ricompiled-pNN` crates (`parts/`, split by
//! `ricompiled-gen` to bound rustc memory); this crate holds the registry over all of them.
//!
//! Lookups take the key of [`qml::walk`]. A lookup only succeeds when the script source
//! hashes to what was compiled, so a runtime tree that differs from the build tree never runs
//! code compiled from other source.

// std's eprintln! panics once stderr's reader is gone; `elog`'s drops the line.
#![warn(clippy::print_stderr)]

pub mod abi;

pub use abi::{BodyKind, CompiledFn, Entry, Layout};
use qml::{Document, Script};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

ricompiled_gen::include_gen!("registry.rs");
ricompiled_gen::include_gen!("sources.rs");

/// QML tree the crate was compiled from (the build fails without one).
pub const ROOT: &str = ricompiled_gen::ROOT;
/// Checksum of the content of that tree (`tools/build.sh --sum`).
pub const ROOT_SUM: &str = ricompiled_gen::ROOT_SUM;
/// Coverage summary of the generation run.
pub const STATS: &str = ricompiled_gen::STATS;

/// Registry key file of a runtime document path: relative to the original `root`, or the
/// `qrc:/...` path of a built-in module file as is.
pub fn file_key(root: &Path, file: &Path) -> Option<String> {
    let s = file.to_str()?;
    if s.starts_with(qml::types::MODULE_PREFIX) {
        return Some(s.to_string());
    }
    // `.` and `..` are resolved lexically, as Qt resolves a relative URL against its base.
    let mut parts: Vec<&str> = Vec::new();
    for c in file.strip_prefix(root).ok()?.components() {
        match c {
            std::path::Component::Normal(p) => parts.push(p.to_str()?),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                parts.pop()?;
            }
            _ => return None,
        }
    }
    Some(parts.join("/"))
}

/// Rows of a file's walk-ordered table by a fingerprint of their key, built on first use: a
/// document whose sites moved (an edited file) is matched by key, not by a scan of the table
/// per site. Candidates are confirmed against the full key.
struct KeyIndex {
    map: std::sync::OnceLock<HashMap<u64, Vec<u32>>>,
}

impl KeyIndex {
    fn new() -> Self {
        KeyIndex {
            map: std::sync::OnceLock::new(),
        }
    }

    fn find<T>(
        &self,
        table: &'static [T],
        key: impl Fn(&T) -> u64,
        want: u64,
        hit: impl Fn(&T) -> bool,
    ) -> Option<&'static T> {
        let map = self.map.get_or_init(|| {
            let mut m: HashMap<u64, Vec<u32>> = HashMap::with_capacity(table.len());
            for (i, t) in table.iter().enumerate() {
                m.entry(key(t)).or_default().push(i as u32);
            }
            m
        });
        map.get(&want)?
            .iter()
            .map(|&i| &table[i as usize])
            .find(|t| hit(t))
    }
}

fn fingerprint(k: impl std::hash::Hash) -> u64 {
    use std::hash::{BuildHasher, BuildHasherDefault};
    BuildHasherDefault::<std::collections::hash_map::DefaultHasher>::default().hash_one(k)
}

/// The row of `table` (a file's walk-ordered table, read in walk order) that `hit` accepts:
/// the common case is the next one (`*next`), anything else is looked up in `index` by the
/// fingerprint `want` of its key (`key` of a row).
fn walk_match<T>(
    table: &'static [T],
    next: &mut usize,
    index: &KeyIndex,
    key: impl Fn(&T) -> u64,
    want: u64,
    hit: impl Fn(&T) -> bool,
) -> Option<&'static T> {
    let i = *next;
    *next += 1;
    match table.get(i) {
        Some(t) if hit(t) => Some(t),
        _ => index.find(table, key, want, hit),
    }
}

/// Per `FILES` row, its entries by `(path, member, hash)`.
static ENTRY_INDEX: std::sync::LazyLock<Vec<KeyIndex>> =
    std::sync::LazyLock::new(|| (0..FILES.len()).map(|_| KeyIndex::new()).collect());

/// Per `LAYOUT_FILES` row, its layouts by object path.
static LAYOUT_INDEX: std::sync::LazyLock<Vec<KeyIndex>> =
    std::sync::LazyLock::new(|| (0..LAYOUT_FILES.len()).map(|_| KeyIndex::new()).collect());

/// Every script member of `doc` (parsed from key file `file`) with its compiled entry, or
/// None where the document's source differs from what was compiled.
pub fn doc_scripts<'a>(file: &str, doc: &'a Document) -> Vec<(&'a Script, Option<&'static Entry>)> {
    let Ok(row) = FILES.binary_search_by(|(f, _)| (*f).cmp(file)) else {
        let mut out = Vec::new();
        qml::walk::sites(doc, &mut |s| out.push((s.script, None)));
        return out;
    };
    let (es, index) = (FILES[row].1, &ENTRY_INDEX[row]);
    let mut out = Vec::new();
    let mut next = 0;
    qml::walk::sites(doc, &mut |s| {
        let h = s.hash();
        let hit = |e: &Entry| e.hash == h && e.path == s.path && e.member == s.name;
        let key = |e: &Entry| fingerprint((e.path, e.member, e.hash));
        let want = fingerprint((s.path.as_str(), s.name.as_str(), h));
        let e = walk_match(es, &mut next, index, key, want, hit);
        out.push((s.script, e));
    });
    out
}

/// Compiled `.js` library of key file `file` if compiled from source `src`.
pub fn library(file: &str, src: &str) -> Option<&'static Entry> {
    let h = qml::walk::src_hash(src);
    LIBS.binary_search_by(|l| l.file.cmp(file))
        .ok()
        .map(|i| LIBS[i])
        .filter(|l| l.hash == h)
}

/// Compiled code of key file `file`, run by `Qt.include` in its caller's context, if compiled
/// from source `src`.
pub fn script(file: &str, src: &str) -> Option<&'static Entry> {
    let h = qml::walk::src_hash(src);
    SCRIPTS
        .binary_search_by(|l| l.file.cmp(file))
        .ok()
        .map(|i| SCRIPTS[i])
        .filter(|l| l.hash == h)
}

/// A file of a runtime QML tree the build cannot run as it is (edited or added since).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stale {
    /// Registry key (path relative to the tree root).
    pub file: String,
    pub why: Why,
}

/// Why a [`Stale`] file cannot run on the compiled code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Why {
    /// `differ` of its `total` scripts have no compiled entry (a `.js` library counts as one
    /// script).
    Scripts { differ: usize, total: usize },
    /// The file cannot be read, so nothing vouches that it is the one compiled.
    Unreadable(String),
    /// The file differs from the build and is not valid QML (`line:col: message`): none of its
    /// scripts can be matched, and loading it fails.
    Syntax(String),
}

impl std::fmt::Display for Stale {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.why {
            Why::Scripts { differ, total } => write!(f, "{}: {differ}/{total} scripts", self.file),
            Why::Unreadable(e) => write!(f, "{}: cannot be read: {e}", self.file),
            Why::Syntax(e) => write!(f, "{}: syntax error at {e}", self.file),
        }
    }
}

/// Every `.qml`/`.js` file under `root` that the compiled tree cannot run as it is. A build
/// without the interpreter checks this before loading, and before `Rustshell.reload()`.
pub fn stale_files(root: &Path) -> Vec<Stale> {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let tree = qml::walk::tree(&root);
    tree.qml
        .iter()
        .chain(&tree.js)
        .filter_map(|(key, p)| stale(key, p))
        .collect()
}

/// [`Stale`] for the file `p` with registry key `key`, unless the build runs it as it is.
pub fn stale(key: &str, p: &Path) -> Option<Stale> {
    let stale = |why| {
        Some(Stale {
            file: key.to_string(),
            why,
        })
    };
    let src = match std::fs::read_to_string(p) {
        Ok(src) => src,
        Err(e) => return stale(Why::Unreadable(e.to_string())),
    };
    if unchanged(key, &src) {
        return None;
    }
    let (differ, total) = if p.extension().is_some_and(|x| x == "js") {
        (library(key, &src).is_none() as usize, 1)
    } else {
        let doc = match qml::parse(&src) {
            Ok(doc) => doc,
            Err(e) => return stale(Why::Syntax(e.to_string())),
        };
        let all = doc_scripts(key, &doc);
        (all.iter().filter(|(_, e)| e.is_none()).count(), all.len())
    };
    if differ == 0 {
        return None;
    }
    stale(Why::Scripts { differ, total })
}

/// Whether `src` is byte for byte the build's file `key`, all of whose scripts are compiled:
/// checking that needs a hash, not a parse.
pub fn unchanged(key: &str, src: &str) -> bool {
    SOURCES
        .binary_search_by(|(f, _)| (*f).cmp(key))
        .is_ok_and(|i| SOURCES[i].1 == qml::walk::src_hash(src))
}

/// [`qml::jsrt::Aot`] over this crate for a QML tree loaded from `root` at runtime (which need
/// not be the build tree [`ROOT`]: entries apply wherever the source is identical).
pub struct Compiled {
    root: PathBuf,
}

impl Compiled {
    pub fn new(root: &Path) -> Self {
        Compiled {
            root: root.canonicalize().unwrap_or_else(|_| root.to_path_buf()),
        }
    }

    fn key(&self, file: &Path) -> Option<String> {
        file_key(&self.root, file).or_else(|| file_key(&self.root, &file.canonicalize().ok()?))
    }
}

impl qml::jsrt::Aot for Compiled {
    fn doc_scripts(&self, file: &Path, doc: &Document) -> Vec<(qml::NodeId, &'static Entry)> {
        let Some(key) = self.key(file) else {
            if qml::log_on!("ri::aotstat") {
                tracing::debug!(target: "ri::aotstat", "aotmiss nokey {}", file.display());
            }
            return Vec::new();
        };
        let all = doc_scripts(&key, doc);
        let miss = all.iter().filter(|(_, e)| e.is_none()).count();
        if miss > 0 {
            let how = if qml::INTERP {
                "running them in the interpreter"
            } else {
                "this build has no interpreter, so they do not run"
            };
            elog::eprintln!(
                "aot: {key}: {miss}/{} scripts differ from the compiled tree; {how}",
                all.len()
            );
        }
        all.into_iter()
            .filter_map(|(s, e)| Some((s.id, e?)))
            .collect()
    }

    fn doc_layouts(
        &self,
        file: &Path,
        doc: &Document,
    ) -> Vec<(qml::NodeId, &'static [&'static str])> {
        let Some(key) = self.key(file) else {
            return Vec::new();
        };
        let Ok(i) = LAYOUT_FILES.binary_search_by(|(f, _)| (*f).cmp(key.as_str())) else {
            return Vec::new();
        };
        let (ls, index) = (LAYOUT_FILES[i].1, &LAYOUT_INDEX[i]);
        let mut out = Vec::new();
        let mut next = 0;
        qml::walk::decls(doc, &mut |path, o, names| {
            let hit = |l: &Layout| {
                l.0 == path
                    && l.1.len() == names.len()
                    && l.1.iter().zip(&names).all(|(a, b)| *a == b)
            };
            let key = |l: &Layout| fingerprint(l.0);
            if let Some(l) = walk_match(ls, &mut next, index, key, fingerprint(path), hit) {
                out.push((o.decl, l.1));
            }
        });
        if qml::log_on!("ri::tmpldbg") {
            tracing::debug!(target: "ri::tmpldbg", "doclayouts {key} {}/{next}", out.len());
        }
        out
    }

    fn library(&self, file: &Path) -> Option<&'static Entry> {
        let key = self.key(file)?;
        library(&key, &std::fs::read_to_string(file).ok()?)
    }

    fn script(&self, file: &Path) -> Option<&'static Entry> {
        let key = self.key(file)?;
        script(&key, &std::fs::read_to_string(file).ok()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walk_match_takes_the_next_row_then_the_index() {
        static TABLE: [(&str, u64); 4] = [("a", 1), ("b", 2), ("c", 3), ("b", 4)];
        let index = KeyIndex::new();
        let key = |t: &(&str, u64)| fingerprint(t.0);
        let mut next = 0;
        let mut find = |k: &str, v: u64| {
            walk_match(&TABLE, &mut next, &index, key, fingerprint(k), |t| {
                t.0 == k && t.1 == v
            })
            .map(|t| t.1)
        };
        // In order: the fast path, the index is never built.
        assert_eq!(find("a", 1), Some(1));
        assert_eq!(find("b", 2), Some(2));
        assert!(index.map.get().is_none());
        // Moved and duplicate keys are found by key; missing ones are not.
        assert_eq!(find("b", 4), Some(4));
        assert_eq!(find("a", 1), Some(1));
        assert_eq!(find("c", 9), None);
        assert_eq!(find("z", 1), None);
        assert_eq!(index.map.get().map(HashMap::len), Some(3));
    }
}
