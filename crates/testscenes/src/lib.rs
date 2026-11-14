//! The QML scenes of the shell's tests (`crates/shell/tests/scenes`), compiled by rigen when
//! this crate is built, as `ricompiled` holds the shell's tree: a test's scene then runs in a
//! build without the interpreter too, on compiled code, as the installed shell runs its tree.
//!
//! Only tests depend on this crate (a dev-dependency), so the shell's binary holds none of it.
//! A scene is a `.qml` file of that directory, named by its path under it
//! (`negative_size/scene.qml`). Its own scripts run from this crate's tables; the files it
//! takes from elsewhere (built-in modules, the shell's tree) run from `ricompiled`.

// The names generated code is written against; the scenes use only some of them.
#[allow(unused_imports)]
#[path = "../../ricompiled/src/abi.rs"]
mod abi;
/// Runtime helpers of generated code (`rigen::PRELUDE`), the copy `ricompiled`'s parts use.
use ricompiled_rt as rt;

use abi::{Entry, Layout};
use qml::runtime::Runtime;
use qml::{Document, NodeId};
use std::path::Path;
use std::rc::Rc;

include!(concat!(env!("OUT_DIR"), "/gen/registry.rs"));

/// Row of scene `file` (as the runtime loads it) in the tables.
fn row(file: &Path) -> Option<usize> {
    let key = file.strip_prefix(ROOT).ok()?.to_str()?;
    FILES.binary_search_by(|(f, _)| (*f).cmp(key)).ok()
}

/// Whether `src` is byte for byte the scene `name` this build compiled.
pub fn compiled(name: &str, src: &str) -> bool {
    SOURCES
        .binary_search_by(|(f, _)| (*f).cmp(name))
        .is_ok_and(|i| SOURCES[i].1 == qml::walk::src_hash(src))
}

/// [`qml::jsrt::Aot`] over the scenes, and over `ricompiled` for every other file.
pub struct Scenes {
    shell: ricompiled::Compiled,
}

impl Scenes {
    pub fn new() -> Self {
        Scenes {
            shell: ricompiled::Compiled::new(Path::new(ricompiled::ROOT)),
        }
    }
}

impl Default for Scenes {
    fn default() -> Self {
        Self::new()
    }
}

impl qml::jsrt::Aot for Scenes {
    fn doc_scripts(&self, file: &Path, doc: &Document) -> Vec<(NodeId, &'static Entry)> {
        let Some(i) = row(file) else {
            return self.shell.doc_scripts(file, doc);
        };
        let mut out = Vec::new();
        qml::walk::sites(doc, &mut |s| {
            let h = s.hash();
            let hit = |e: &&Entry| e.hash == h && e.path == s.path && e.member == s.name;
            if let Some(e) = FILES[i].1.iter().find(hit) {
                out.push((s.script.id, e));
            }
        });
        out
    }

    fn doc_layouts(&self, file: &Path, doc: &Document) -> Vec<(NodeId, &'static [&'static str])> {
        let Some(i) = row(file) else {
            return self.shell.doc_layouts(file, doc);
        };
        let mut out = Vec::new();
        qml::walk::decls(doc, &mut |path, o, names| {
            let hit = |l: &&Layout| l.0 == path && l.1.iter().eq(names.iter());
            if let Some(l) = LAYOUT_FILES[i].1.iter().find(hit) {
                out.push((o.decl, l.1));
            }
        });
        out
    }

    fn library(&self, file: &Path) -> Option<&'static Entry> {
        self.shell.library(file)
    }

    fn script(&self, file: &Path) -> Option<&'static Entry> {
        self.shell.script(file)
    }
}

/// Why this build cannot run scene `name` whose file holds `src`, if it cannot: it has no
/// interpreter (`interp` false) and did not compile that text.
fn refusal(name: &str, src: &str, interp: bool) -> Option<String> {
    (!interp && !compiled(name, src)).then(|| {
        format!(
            "scene {name}: this build has no interpreter and did not compile the scene as it \
             is in {ROOT} (crates/testscenes/build.rs compiles that directory)"
        )
    })
}

/// A runtime with scene `name` loaded, and the scene's root object. A build with the `interp`
/// feature interprets the scene's scripts; a build without it runs the compiled ones, and
/// panics on a scene it did not compile as the file is now: no test passes on a scene that
/// did not run.
pub fn load(name: &str) -> (Runtime, usize) {
    load_with(name, |_| {})
}

/// As `load`, with `setup` run on the runtime before the scene is loaded: what the shell
/// installs before it loads a document (the text measure, say) is there when the scene's
/// `Component.onCompleted` run.
pub fn load_with(name: &str, setup: impl FnOnce(&mut Runtime)) -> (Runtime, usize) {
    let file = Path::new(ROOT).join(name);
    let src = std::fs::read_to_string(&file).unwrap_or_else(|e| panic!("scene {name}: {e}"));
    if let Some(why) = refusal(name, &src, qml::INTERP) {
        panic!("{why}");
    }
    if qml::INTERP {
        println!("scene {name}: interpreted scripts");
        // From its file, as the compiled one: a scene may declare an inline component.
        let mut rt = Runtime::new(Some(ROOT.into()));
        setup(&mut rt);
        let root = rt
            .load_file(&file)
            .unwrap_or_else(|e| panic!("scene {name}: {e}"));
        return (rt, root);
    }
    println!("scene {name}: compiled scripts");
    let mut rt = Runtime::new(Some(ROOT.into()));
    rt.eng().set_compiled(Some(Rc::new(Scenes::new())));
    setup(&mut rt);
    let root = rt
        .load_file(&file)
        .unwrap_or_else(|e| panic!("scene {name}: {e}"));
    (rt, root)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NAME: &str = "negative_size/scene.qml";

    fn text() -> String {
        std::fs::read_to_string(Path::new(ROOT).join(NAME)).unwrap()
    }

    #[test]
    fn a_scene_is_compiled_only_as_the_text_the_build_read() {
        let src = text();
        assert!(compiled(NAME, &src));
        assert!(!compiled(NAME, &format!("{src}\n")));
        assert!(!compiled("negative_size/none.qml", &src));
    }

    #[test]
    fn without_the_interpreter_a_scene_that_is_not_compiled_is_refused() {
        let src = text();
        assert_eq!(refusal(NAME, &src, false), None);
        let changed = format!("{src}\n");
        let why = refusal(NAME, &changed, false).expect("refused");
        assert!(why.contains("no interpreter"), "{why}");
        assert!(refusal("negative_size/none.qml", &src, false).is_some());
        // The interpreter runs any text.
        assert_eq!(refusal(NAME, &changed, true), None);
    }

    #[test]
    fn every_scene_file_is_in_the_tables() {
        let on_disk = qml::walk::tree(Path::new(ROOT)).qml.len();
        assert!(on_disk > 0);
        assert_eq!(FILES.len(), on_disk);
        assert_eq!(SOURCES.len(), on_disk);
        assert_eq!(LAYOUT_FILES.len(), on_disk);
    }
}
