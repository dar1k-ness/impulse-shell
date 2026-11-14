//! Every script of the build's tree (and the built-in QML modules) has a compiled entry.
use std::path::Path;

fn check_tree(root: &Path, qml: &[(String, std::path::PathBuf)]) -> usize {
    let mut n = 0;
    for (key, p) in qml {
        let src = std::fs::read_to_string(p).unwrap();
        let doc = qml::parse(&src).unwrap();
        // Independent count: qml::Object::scripts over every object, minus alias targets.
        let mut expect = 0;
        doc.root.walk(&mut |o| {
            let mut ss = Vec::new();
            o.scripts(&mut ss);
            let aliases = o
                .members
                .iter()
                .filter(|m| {
                    matches!(m, qml::Member::Property { ty, value: Some(qml::Value::Script(_)), .. } if ty == "alias")
                })
                .count();
            expect += ss.len() - aliases;
        });
        let got = ricompiled::doc_scripts(key, &doc);
        assert_eq!(got.len(), expect, "{key}: sites vs scripts");
        for (s, e) in &got {
            let e = e.unwrap_or_else(|| panic!("{key}:{} not compiled", s.line));
            assert_eq!(e.file, key);
        }
        n += got.len();
    }
    let _ = root;
    n
}

#[test]
fn every_script_compiled() {
    let modules = rigen::walk::module_tree();
    let mut n = check_tree(Path::new(""), &modules);
    let root = Path::new(ricompiled::ROOT);
    let t = rigen::walk::tree(root);
    assert!(!t.qml.is_empty());
    n += check_tree(root, &t.qml);
    for (key, p) in &t.js {
        let src = std::fs::read_to_string(p).unwrap();
        assert!(
            ricompiled::library(key, &src).is_some(),
            "{key} not compiled"
        );
    }
    // Unchanged files are vouched for by their hash; any edit falls back to the parse.
    for (key, p) in t.qml.iter().chain(&t.js) {
        let src = std::fs::read_to_string(p).unwrap();
        assert!(ricompiled::unchanged(key, &src), "{key} not in SOURCES");
        assert!(
            !ricompiled::unchanged(key, &(src + " ")),
            "{key}: edit unnoticed"
        );
    }
    assert!(ricompiled::stale_files(root).is_empty());
    // Through the runtime trait, from the absolute paths the runtime loads.
    use qml::jsrt::Aot;
    let aot = ricompiled::Compiled::new(root);
    let (_, p) = &t.qml[0];
    let doc = qml::parse(&std::fs::read_to_string(p).unwrap()).unwrap();
    assert!(!aot.doc_scripts(p, &doc).is_empty());
    assert!(aot.library(&t.js[0].1).is_some());
    // A different source never matches.
    let (key, p) = &t.js[0];
    let src = std::fs::read_to_string(p).unwrap() + " ";
    assert!(ricompiled::library(key, &src).is_none());
    assert_eq!(
        ricompiled::file_key(root, &root.join("modules/x/A.qml")).as_deref(),
        Some("modules/x/A.qml")
    );
    // A Loader source reached through `..` keys the file it resolves to.
    assert_eq!(
        ricompiled::file_key(root, &root.join("modules/y/../x/./A.qml")).as_deref(),
        Some("modules/x/A.qml")
    );
    eprintln!("{n} scripts compiled ({})", ricompiled::STATS);
}
