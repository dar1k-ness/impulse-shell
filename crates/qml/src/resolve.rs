//! Quickshell type resolution: `import ri.a.b` → `<config root>/a/b/*.qml`; same-directory
//! components are implicit. Files starting with `pragma Singleton` are singletons.
use rustc_hash::FxHashMap as HashMap;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::SystemTime;

#[derive(Clone, Debug)]
pub struct TypeRef {
    /// Shared: the same file is listed in the visible-types map of every file importing it.
    pub file: Rc<Path>,
    pub singleton: bool,
    /// The name the directory's `qmldir` registers the file's type under, when that is not
    /// the name this entry is found by (`Uno` for `singleton Renamed Uno.qml`).
    pub registered: Option<Rc<str>>,
    /// A `qmldir` lists the file as a singleton and the file has no `pragma Singleton`.
    pub no_pragma: bool,
}

impl TypeRef {
    /// The key of the singleton's object in the world: the name of its file, as the
    /// singletons the host lists by directory.
    pub fn singleton_key(&self) -> String {
        let stem = self.file.file_stem().unwrap_or_default();
        stem.to_string_lossy().into_owned()
    }
}

pub fn module_dir(root: &Path, uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("ri")?;
    let mut p = root.to_path_buf();
    for seg in rest.split('.').filter(|s| !s.is_empty()) {
        p.push(seg);
    }
    p.is_dir().then_some(p)
}

/// Types by the name a document writes them with.
pub type Types = Rc<HashMap<String, TypeRef>>;

/// A directory's types by name: what a file importing it can name, and what a file of the
/// directory itself can, which sees the ones its `qmldir` keeps to it (`internal`) too. One
/// table when the directory keeps none.
#[derive(Clone)]
struct DirTypes {
    imported: Types,
    own: Types,
}

impl DirTypes {
    fn new(list: Vec<(String, TypeRef, bool)>) -> Self {
        let hidden = list.iter().any(|(.., hidden)| *hidden);
        let table = |internal: bool| -> Types {
            let seen = list.iter().filter(|(.., hidden)| internal || !hidden);
            Rc::new(seen.map(|(n, t, _)| (n.clone(), t.clone())).collect())
        };
        let own = table(true);
        DirTypes {
            imported: if hidden { table(false) } else { own.clone() },
            own,
        }
    }
}

thread_local! {
    /// Each directory's QML types, revalidated by the directory's mtime: every new component
    /// file lists the directories it imports, and re-reading every file there for
    /// `pragma Singleton` each time was ~32k file opens for one startup.
    static DIRS: RefCell<HashMap<PathBuf, (SystemTime, DirTypes)>> = RefCell::default();
}

/// The types a file importing `dir` can name.
pub fn scan_dir(dir: &Path, out: &mut HashMap<String, TypeRef>) {
    scan(dir, false, out)
}

/// `internal`: for a file of `dir` itself, which sees the directory's internal types too.
fn scan(dir: &Path, internal: bool, out: &mut HashMap<String, TypeRef>) {
    if let Some(t) = dir_types(dir, internal) {
        out.extend(t.iter().map(|(n, t)| (n.clone(), t.clone())));
    }
}

/// The table of `dir`'s types, the same one for every document that sees the directory.
fn dir_types(dir: &Path, internal: bool) -> Option<Types> {
    let mtime = std::fs::metadata(dir).and_then(|m| m.modified()).ok()?;
    let hit = DIRS.with_borrow(|c| {
        c.get(dir)
            .filter(|(t, _)| *t == mtime)
            .map(|(_, l)| l.clone())
    });
    let types = hit.unwrap_or_else(|| {
        let l = DirTypes::new(read_dir_types(dir));
        DIRS.with_borrow_mut(|c| c.insert(dir.to_path_buf(), (mtime, l.clone())));
        l
    });
    Some(if internal { types.own } else { types.imported })
}

/// The type lines of a directory's `qmldir`: `[singleton | internal] Name [version] File.qml`
/// as (name, file, singleton, internal).
fn qmldir_types(src: &str) -> Vec<(&str, &str, bool, bool)> {
    let mut out = Vec::new();
    for line in src.lines() {
        let w: Vec<&str> = line.split_whitespace().collect();
        let (singleton, internal, w) = match w.first() {
            Some(&"singleton") => (true, false, &w[1..]),
            Some(&"internal") => (false, true, &w[1..]),
            _ => (false, false, &w[..]),
        };
        let (name, file) = match w {
            [name, file] | [name, _, file] => (*name, *file),
            _ => continue,
        };
        if name.starts_with(char::is_uppercase) && file.ends_with(".qml") {
            out.push((name, file, singleton, internal));
        }
    }
    out
}

/// Every `Name.qml` of the directory is the type `Name`. Without a `qmldir` the files with
/// `pragma Singleton` are singletons, as in the `qmldir` Quickshell writes for a directory
/// that has none. With one, Qt's rules: its lines name the types and say which are
/// singletons, and a file keeps its own name beside the one it is listed under.
fn read_dir_types(dir: &Path) -> Vec<(String, TypeRef, bool)> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return out;
    };
    let qmldir = std::fs::read_to_string(dir.join("qmldir")).ok();
    let listed = qmldir.as_deref().map(qmldir_types).unwrap_or_default();
    // The first line listing a file is what Qt registers its type as.
    let line = |file: &str| listed.iter().find(|l| l.1 == file);
    let pragma = |p: &Path| {
        std::fs::read_to_string(p)
            .map(|s| s.lines().any(|l| l.trim() == "pragma Singleton"))
            .unwrap_or(false)
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().is_some_and(|x| x == "qml") {
            let name = p.file_stem().unwrap().to_string_lossy().to_string();
            if !name.starts_with(char::is_uppercase) {
                continue;
            }
            let file = p.file_name().unwrap().to_string_lossy();
            let (singleton, registered) = match line(&file) {
                None if qmldir.is_none() => (pragma(&p), None),
                None => (false, None),
                Some(&(reg, _, singleton, _)) => (singleton, (reg != name).then(|| Rc::from(reg))),
            };
            out.push((
                name,
                TypeRef {
                    no_pragma: qmldir.is_some() && singleton && !pragma(&p),
                    file: p.into(),
                    singleton,
                    registered,
                },
                false,
            ));
        }
    }
    for &(name, file, singleton, internal) in &listed {
        let p = dir.join(file);
        if out.iter().any(|(n, ..)| n == name) || !p.is_file() {
            continue;
        }
        let first = line(file).map(|l| l.0).filter(|f| *f != name);
        out.push((
            name.to_string(),
            TypeRef {
                no_pragma: singleton && !pragma(&p),
                file: p.into(),
                singleton,
                registered: first.map(Rc::from),
            },
            internal,
        ));
    }
    out
}

/// The key, in a file's visible types, of a singleton its imports list that no file can be
/// loaded with ([`TypeRef::no_pragma`]): no type has the empty name.
pub const UNAVAILABLE: &str = "";

/// Qt's errors for a document whose imports hold the singleton `t` named `name`, listed by a
/// `qmldir` and written without `pragma Singleton`.
pub fn unavailable(vis: &Vis) -> Result<(), String> {
    let Some(t) = vis.get(UNAVAILABLE) else {
        return Ok(());
    };
    let name = t.registered.as_deref().unwrap_or_default();
    Err(format!(
        "Type {name} unavailable\nqmldir defines type as singleton, but no pragma Singleton found in type {name}."
    ))
}

/// The types a document can name: those of its own directory, then those of each import in
/// the order written, a later one shadowing an earlier one. The tables are the directories'
/// and modules' own, shared by every document: a table of all of them per document was 9 MB
/// for the 474 sets of imports of the original.
#[derive(Clone, Debug, Default)]
pub struct Vis {
    layers: Vec<Layer>,
    /// [`UNAVAILABLE`].
    unavailable: Option<TypeRef>,
}

#[derive(Clone, Debug)]
struct Layer {
    /// `import ... as alias`: its types are named `alias.Name`.
    alias: Option<String>,
    types: Types,
}

impl Layer {
    fn get(&self, name: &str) -> Option<&TypeRef> {
        match &self.alias {
            None => self.types.get(name),
            Some(a) => {
                let name = name.strip_prefix(a.as_str())?.strip_prefix('.')?;
                self.types.get(name)
            }
        }
    }
}

impl Vis {
    pub fn get(&self, name: &str) -> Option<&TypeRef> {
        if name == UNAVAILABLE {
            return self.unavailable.as_ref();
        }
        self.layers.iter().rev().find_map(|l| l.get(name))
    }

    pub fn contains_key(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    /// Every name with its type, in one table.
    pub fn merged(&self) -> HashMap<String, TypeRef> {
        let mut t = HashMap::default();
        for l in &self.layers {
            let types = l.types.iter().map(|(k, v)| (k, v.clone()));
            match &l.alias {
                Some(a) => t.extend(types.map(|(k, v)| (format!("{a}.{k}"), v))),
                None => t.extend(types.map(|(k, v)| (k.clone(), v))),
            }
        }
        t.extend(
            self.unavailable
                .clone()
                .map(|u| (UNAVAILABLE.to_string(), u)),
        );
        t
    }

    fn push(&mut self, alias: &Option<String>, types: Types) {
        if !types.is_empty() {
            let alias = alias.clone();
            self.layers.push(Layer { alias, types });
        }
    }

    /// The singleton listed without its pragma: of those a name still reaches, the first by
    /// the name its `qmldir` gives it, whichever of its names was met.
    fn find_unavailable(&self) -> Option<TypeRef> {
        let named = self.layers.iter().enumerate().flat_map(|(at, l)| {
            let bad = l.types.iter().filter(|(_, v)| v.no_pragma);
            let named = bad.map(|(k, v)| match &l.alias {
                Some(a) => (format!("{a}.{k}"), v),
                None => (k.clone(), v),
            });
            named.filter(move |(k, _)| !self.layers[at + 1..].iter().any(|s| s.get(k).is_some()))
        });
        named
            .map(|(k, v)| {
                let name = k.rsplit('.').next().unwrap_or(&k);
                (v.registered.clone().unwrap_or_else(|| name.into()), v)
            })
            .min_by(|a, b| a.0.cmp(&b.0))
            .map(|(name, v)| TypeRef {
                registered: Some(name),
                ..v.clone()
            })
    }
}

/// Visible QML-file types for a document at `file`: later imports shadow earlier ones. As in
/// Qt, explicit imports take precedence over the implicit import of the file's own directory,
/// so `widgets/CalendarWidget.qml` can wrap the `CalendarWidget` of an imported module instead
/// of instantiating itself.
pub fn visible(root: &Path, file: &Path, doc: &crate::Document) -> Vis {
    let mut t = Vis::default();
    if let Some(own) = file.parent().and_then(|d| dir_types(d, true)) {
        t.push(&None, own);
    }
    for imp in &doc.imports {
        t.push(&imp.alias, crate::types::module_types_shared(&imp.uri));
        // `import "panelFamilies"` / `import ".."`: a directory relative to the importing file.
        let rel = imp
            .uri
            .strip_prefix('"')
            .and_then(|u| u.strip_suffix('"'))
            .filter(|u| !u.ends_with(".js"))
            .and_then(|u| file.parent().map(|d| d.join(u)))
            .filter(|d| d.is_dir());
        let dir = rel.or_else(|| module_dir(root, &imp.uri));
        if let Some(types) = dir.and_then(|d| dir_types(&d, false)) {
            t.push(&imp.alias, types);
        }
    }
    t.unavailable = t.find_unavailable();
    t
}

/// [`visible`] as one table.
pub fn visible_types(root: &Path, file: &Path, doc: &crate::Document) -> HashMap<String, TypeRef> {
    visible(root, file, doc).merged()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One table per document of every type it can name, as before [`Vis`].
    fn one_table(root: &Path, file: &Path, doc: &crate::Document) -> HashMap<String, TypeRef> {
        let mut t = HashMap::default();
        if let Some(d) = file.parent() {
            scan(d, true, &mut t);
        }
        for imp in &doc.imports {
            let m = crate::types::module_types(&imp.uri);
            match &imp.alias {
                Some(a) => t.extend(m.into_iter().map(|(k, v)| (format!("{a}.{k}"), v))),
                None => t.extend(m),
            }
            let rel = imp
                .uri
                .strip_prefix('"')
                .and_then(|u| u.strip_suffix('"'))
                .filter(|u| !u.ends_with(".js"))
                .and_then(|u| file.parent().map(|d| d.join(u)))
                .filter(|d| d.is_dir());
            if let Some(d) = rel.or_else(|| module_dir(root, &imp.uri)) {
                let mut m = HashMap::default();
                scan_dir(&d, &mut m);
                match &imp.alias {
                    Some(a) => t.extend(m.into_iter().map(|(k, v)| (format!("{a}.{k}"), v))),
                    None => t.extend(m),
                }
            }
        }
        let bad = t.iter().filter(|(_, v)| v.no_pragma);
        let bad = bad
            .map(|(k, v)| {
                let name = k.rsplit('.').next().unwrap_or(k);
                (v.registered.clone().unwrap_or_else(|| name.into()), v)
            })
            .min_by(|a, b| a.0.cmp(&b.0))
            .map(|(name, v)| TypeRef {
                registered: Some(name),
                ..v.clone()
            });
        if let Some(bad) = bad {
            t.insert(UNAVAILABLE.to_string(), bad);
        }
        t
    }

    fn same(a: Option<&TypeRef>, b: Option<&TypeRef>) -> bool {
        let key = |t: &TypeRef| {
            (
                t.file.clone(),
                t.singleton,
                t.registered.clone(),
                t.no_pragma,
            )
        };
        a.map(key) == b.map(key)
    }

    fn qml_files(dir: &Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                qml_files(&p, out);
            } else if p.extension().is_some_and(|x| x == "qml") {
                out.push(p);
            }
        }
    }

    /// Every document of the original names the types it named with a table of its own.
    #[test]
    fn shared_tables_name_what_one_table_per_document_named() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qml");
        let mut files = Vec::new();
        qml_files(&root, &mut files);
        assert!(files.len() > 400, "{}", files.len());
        let mut names = 0;
        for f in &files {
            let Ok(doc) = crate::parse(&std::fs::read_to_string(f).unwrap()) else {
                continue;
            };
            let was = one_table(&root, f, &doc);
            let vis = visible(&root, f, &doc);
            for (name, t) in &was {
                assert!(same(vis.get(name), Some(t)), "{name} in {}", f.display());
                assert!(vis.contains_key(name));
            }
            let merged = vis.merged();
            assert_eq!(merged.len(), was.len(), "{}", f.display());
            for name in merged.keys() {
                assert!(was.contains_key(name), "{name} in {}", f.display());
            }
            for name in ["Nope", "nope.Item", "W.", ".Item", "QtQuick"] {
                assert!(
                    same(vis.get(name), was.get(name)),
                    "{name} in {}",
                    f.display()
                );
            }
            names += was.len();
        }
        assert!(names > 20_000, "{names}");
    }

    /// Shadowing, aliases, internal types and the singleton without its pragma, as one table
    /// had them; the tables are one per directory, whoever imports it.
    #[test]
    fn layers_shadow_like_one_table_and_are_shared() {
        let root = std::env::temp_dir().join(format!("qml-vis-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let put = |rel: &str, src: &str| {
            let p = root.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, src).unwrap();
        };
        let item = "import QtQuick\nItem {}\n";
        put("a/Own.qml", item);
        put("a/Twice.qml", item);
        put("a/Main.qml", item);
        put("b/Twice.qml", item);
        put("b/Other.qml", item);
        put("b/Hidden.qml", item);
        put("b/Bad.qml", "import QtQuick\nQtObject {}\n");
        put(
            "b/qmldir",
            "internal Inner Hidden.qml\nsingleton Worse Bad.qml\n",
        );
        put("c/Twice.qml", item);
        put("c/Third.qml", item);
        put("d/Main.qml", item);
        let srcs = [
            ("a/Main.qml", "import ri.b\nimport ri.c as C\nItem {}\n"),
            ("a/Main.qml", "import ri.c\nimport ri.b\nItem {}\n"),
            (
                "a/Main.qml",
                "import ri.b as X\nimport ri.c as X\nItem {}\n",
            ),
            ("a/Main.qml", "import ri.b as X\nimport \"../b\"\nItem {}\n"),
            ("b/Other.qml", "import ri.c\nItem {}\n"),
            ("d/Main.qml", "import ri.b\nimport ri.c as C\nItem {}\n"),
        ];
        let names = [
            "Own", "Twice", "Main", "Other", "Hidden", "Bad", "Worse", "Third", "C.Twice",
            "C.Third", "C.Own", "X.Twice", "X.Third", "X.Other", "X.Hidden", "X.Worse", "X",
            "Inner", "X.Inner",
        ];
        let mut all = Vec::new();
        for (file, src) in srcs {
            let (file, doc) = (root.join(file), crate::parse(src).unwrap());
            let was = one_table(&root, &file, &doc);
            let vis = visible(&root, &file, &doc);
            for name in names {
                assert!(same(vis.get(name), was.get(name)), "{name} in {src}");
            }
            // Which of its entries stands for the singleton without its pragma was the
            // table's order; the name Qt's error gives is the same for each.
            let registered = |t: Option<&TypeRef>| t.map(|t| t.registered.clone());
            assert_eq!(registered(vis.get("")), registered(was.get("")), "{src}");
            assert_eq!(vis.merged().len(), was.len(), "{src}");
            all.push(vis);
        }
        assert!(all[0].get("Twice").unwrap().file.ends_with("b/Twice.qml"));
        assert!(all[1].get("Worse").is_some() && all[1].get("").is_some());
        assert!(all[4].get("Inner").is_some() && all[0].get("Inner").is_none());
        // `a/Main.qml` and `d/Main.qml` import the same two directories: the same two tables.
        let tables = |v: &Vis| -> Vec<Types> { v.layers.iter().map(|l| l.types.clone()).collect() };
        let (a, d) = (tables(&all[0]), tables(&all[5]));
        assert_eq!((a.len(), d.len()), (3, 3));
        assert!(Rc::ptr_eq(&a[1], &d[1]) && Rc::ptr_eq(&a[2], &d[2]));
        assert!(!Rc::ptr_eq(&a[0], &d[0]));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn scan_dir_revalidates_by_directory_mtime() {
        let d = std::env::temp_dir().join(format!("qml-scandir-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        // Each change gets its own directory mtime, whatever the filesystem's timestamp
        // granularity.
        let t0 = std::time::SystemTime::now();
        let touch = |n: u64| {
            std::fs::File::open(&d)
                .unwrap()
                .set_modified(t0 + std::time::Duration::from_secs(n))
                .unwrap()
        };
        std::fs::write(d.join("A.qml"), "import QtQuick\nItem {}\n").unwrap();
        let mut m = super::HashMap::default();
        super::scan_dir(&d, &mut m);
        assert_eq!(m.len(), 1);
        std::fs::write(
            d.join("B.qml"),
            "pragma Singleton\nimport QtQuick\nQtObject {}\n",
        )
        .unwrap();
        touch(1);
        let mut m = super::HashMap::default();
        super::scan_dir(&d, &mut m);
        assert!(!m["A"].singleton && m["B"].singleton, "{m:?}");
        std::fs::remove_file(d.join("A.qml")).unwrap();
        touch(2);
        let mut m = super::HashMap::default();
        super::scan_dir(&d, &mut m);
        assert!(!m.contains_key("A") && m.contains_key("B"), "{m:?}");
        let _ = std::fs::remove_dir_all(&d);
    }
}
