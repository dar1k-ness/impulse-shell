//! The parser accepts and rejects what Qt 6.12's does, with Qt's `errorString()`.
//!
//! `qt_parse/generated.tsv` holds documents made by changing a few tokens of a seed
//! (`tools/gen/qqmljs/fuzz`), each with the status and the `errorString()` of a `QQmlComponent`
//! of Qt 6.12 for it.

use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        out.push(match c {
            '\\' => match chars.next() {
                Some('n') => '\n',
                Some('r') => '\r',
                Some('t') => '\t',
                _ => '\\',
            },
            c => c,
        });
    }
    out
}

/// What Qt's component says after parsing `src`: its status and its `errorString()`.
fn verdict(src: &str) -> (u8, String) {
    match qml::parse(src) {
        Ok(_) => (1, String::new()),
        Err(e) if e.silent => (0, String::new()),
        Err(e) => (3, e.under("G.qml") + "\n"),
    }
}

#[test]
fn generated_documents_are_judged_as_qt_judges_them() {
    let dir = root().join("tools/gen/qqmljs/fuzz/seeds");
    let list = include_str!("qt_parse/generated.tsv");
    let (mut accepted, mut rejected) = (0, 0);
    let mut wrong = Vec::new();
    for (n, line) in list.lines().enumerate() {
        let mut fields = line.split('\t');
        let status: u8 = fields.next().unwrap().parse().unwrap();
        let seed = std::fs::read_to_string(dir.join(fields.next().unwrap())).unwrap();
        let text = unescape(fields.next().unwrap());
        let (mut doc, mut at) = (String::new(), 0);
        for edit in fields {
            let mut edit = edit.splitn(3, ',');
            let start: usize = edit.next().unwrap().parse().unwrap();
            let end: usize = edit.next().unwrap().parse().unwrap();
            doc += &seed[at..start];
            doc += &unescape(edit.next().unwrap());
            at = end;
        }
        doc += &seed[at..];
        match status {
            1 => accepted += 1,
            _ => rejected += 1,
        }
        let ours = verdict(&doc);
        if ours != (status, text.clone()) {
            wrong.push(format!(
                "line {}: Qt {status} {text:?}, ours {ours:?}",
                n + 1
            ));
        }
    }
    assert!(accepted >= 300 && rejected >= 1000, "{accepted} {rejected}");
    assert!(
        wrong.is_empty(),
        "{} of {}:\n{}",
        wrong.len(),
        accepted + rejected,
        wrong.join("\n")
    );
}

fn documents(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            documents(&path, out);
        } else if path.extension().is_some_and(|e| e == "qml") {
            out.push(path);
        }
    }
}

/// Every document of the shell and of the fixtures of the generated code and of the shell's
/// tests is one Qt's parser accepts, so this one does. The documents of a `parse_errors`
/// directory are the ones Qt rejects (the `parse_error_*` cases say with what), so this one
/// rejects each.
#[test]
fn documents_of_the_tree_parse() {
    for (dir, least) in [
        ("qml", 600),
        ("crates/rigen/check/cases/qml", 20),
        ("crates/shell/tests", 30),
    ] {
        let mut files = Vec::new();
        documents(&root().join(dir), &mut files);
        let (rejected, files): (Vec<PathBuf>, Vec<PathBuf>) = files
            .into_iter()
            .partition(|f| f.parent().is_some_and(|d| d.ends_with("parse_errors")));
        let accepted: Vec<String> = rejected
            .iter()
            .filter(|f| qml::parse(&std::fs::read_to_string(f).unwrap()).is_ok())
            .map(|f| f.display().to_string())
            .collect();
        assert!(
            accepted.is_empty(),
            "{dir}, parses:\n{}",
            accepted.join("\n")
        );
        assert!(files.len() >= least, "{dir}: {} documents", files.len());
        let failed: Vec<String> = files
            .iter()
            .filter_map(|f| {
                let src = std::fs::read_to_string(f).unwrap();
                let e = qml::parse(&src).err()?;
                Some(e.under(&f.display().to_string()))
            })
            .collect();
        assert!(failed.is_empty(), "{dir}:\n{}", failed.join("\n"));
    }
}
