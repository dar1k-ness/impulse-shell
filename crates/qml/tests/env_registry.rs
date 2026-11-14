//! docs/env.md is the one list of the `II_*` / `RI_*` environment variables, the `RI_DEBUG`
//! switches and the `RI_LOG` targets: it names every one the code has and none it does not.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Trees whose files are scanned for variable names.
const TREES: &[&str] = &["crates", "tools", "bench", ".github", ".config"];

/// `II_*` names in those trees that are not environment variables.
const NOT_VARIABLES: &[&str] = &[
    // Rust statics and constants.
    "RI_BAR_MAIN",
    "RI_COUNTS",
    // A positional argument in textcolor_region_golden.sh's usage line.
    "RI_SCRIPTS_DIR",
];

const DOC: &str = "docs/env.md";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Every file under `p` outside build output, or `p` itself.
fn files(p: &Path, out: &mut Vec<PathBuf>) {
    if !p.is_dir() {
        out.push(p.to_path_buf());
        return;
    }
    if p.file_name().is_some_and(|n| n == "target") {
        return;
    }
    let mut entries: Vec<PathBuf> = std::fs::read_dir(p)
        .unwrap_or_else(|e| panic!("{}: {e}", p.display()))
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();
    for e in entries {
        files(&e, out);
    }
}

/// The text of every UTF-8 file under `dir` (fixtures such as images are skipped).
fn texts(dir: &Path) -> Vec<String> {
    let mut paths = Vec::new();
    files(dir, &mut paths);
    paths
        .iter()
        .filter_map(|p| String::from_utf8(std::fs::read(p).unwrap()).ok())
        .collect()
}

fn word(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// The names in `text` that start with `II_` or `RI_`. A prefix written as a pattern (`II_*`,
/// `II_<NAME>`, `RI_JS_`) and a longer identifier containing one are not names.
fn variables(text: &str, out: &mut BTreeSet<String>) {
    let b = text.as_bytes();
    let upper = |c: u8| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_';
    for prefix in ["RI_"] {
        for (at, _) in text.match_indices(prefix) {
            if at > 0 && word(b[at - 1]) {
                continue;
            }
            let end = at + b[at..].iter().take_while(|&&c| upper(c)).count();
            if end == at + prefix.len()
                || b[end - 1] == b'_'
                || b.get(end).is_some_and(|&c| word(c))
            {
                continue;
            }
            out.insert(text[at..end].to_owned());
        }
    }
}

/// The `"ri::name"` string literals of `text`: the targets of `tracing` events and `log_on!`.
fn targets(text: &str, out: &mut BTreeSet<String>) {
    for (at, open) in text.match_indices("\"ri::") {
        let rest = &text[at + open.len()..];
        let len = rest
            .bytes()
            .take_while(|&c| c.is_ascii_lowercase() || c == b'_')
            .count();
        if len > 0 && rest[len..].starts_with('"') {
            out.insert(format!("ri::{}", &rest[..len]));
        }
    }
}

fn doc() -> String {
    let p = root().join(DOC);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// What the tables under the heading `title` list: the text in the first backquotes of each row.
fn listed(doc: &str, title: &str) -> BTreeSet<String> {
    let mut lines = doc.lines().skip_while(|l| *l != title);
    assert!(lines.next().is_some(), "{DOC} has no section {title:?}");
    lines
        .take_while(|l| !l.starts_with("## "))
        .filter_map(|l| l.strip_prefix("| `"))
        .map(|l| l.split('`').next().unwrap().to_owned())
        .collect()
}

/// Fails with both differences between what the code has and what the document lists.
#[track_caller]
fn assert_same(what: &str, code: &BTreeSet<String>, listed: &BTreeSet<String>) {
    let missing: Vec<&String> = code.difference(listed).collect();
    let gone: Vec<&String> = listed.difference(code).collect();
    assert!(
        missing.is_empty() && gone.is_empty(),
        "{what}: in the code but not in {DOC}: {missing:?}; in {DOC} but not in the code: {gone:?}"
    );
}

#[test]
fn every_variable_is_listed() {
    let legacy: BTreeSet<&str> = qml::debug::LEGACY.iter().map(|l| l.0).collect();
    let keep = |names: BTreeSet<String>| -> BTreeSet<String> {
        names
            .into_iter()
            .filter(|n| !legacy.contains(n.as_str()) && !NOT_VARIABLES.contains(&n.as_str()))
            .collect()
    };
    let mut code = BTreeSet::new();
    for tree in TREES {
        for text in texts(&root().join(tree)) {
            variables(&text, &mut code);
        }
    }
    let mut listed = BTreeSet::new();
    variables(&doc(), &mut listed);
    assert_same("environment variables", &keep(code), &keep(listed));
}

#[test]
fn every_debug_switch_is_listed() {
    let code: BTreeSet<String> = qml::debug::Switch::all()
        .iter_names()
        .map(|(name, _)| name.to_ascii_lowercase())
        .collect();
    let (params, switches): (BTreeSet<String>, BTreeSet<String>) =
        listed(&doc(), "## `RI_DEBUG` switches")
            .into_iter()
            .partition(|n| n.contains('='));
    assert_same("RI_DEBUG switches", &code, &switches);
    assert!(!params.is_empty(), "{DOC} lists no RI_DEBUG parameter");
    for p in &params {
        let name = p.split('=').next().unwrap();
        let o = qml::debug::Opts::parse(&format!("{name}=1"));
        assert!(o.unknown.is_empty(), "{DOC}: RI_DEBUG has no parameter {p}");
    }
}

#[test]
fn every_log_target_is_listed() {
    let mut code = BTreeSet::new();
    let crates = root().join("crates");
    for c in std::fs::read_dir(&crates).unwrap() {
        let src = c.unwrap().path().join("src");
        if src.is_dir() {
            for text in texts(&src) {
                targets(&text, &mut code);
            }
        }
    }
    assert_same(
        "RI_LOG targets",
        &code,
        &listed(&doc(), "## `RI_LOG` targets"),
    );
}

#[test]
fn scanners_read_names_only() {
    let mut v = BTreeSet::new();
    variables(
        "RI_ROOT=x \"RI_LOG\" $RI_AB_LOG/f II_* II_<NAME> RI_JS_ XII_NO RI_Mixed RI_DEBUG=dump",
        &mut v,
    );
    let v: Vec<&str> = v.iter().map(String::as_str).collect();
    assert_eq!(v, ["RI_AB_LOG", "RI_DEBUG", "RI_LOG", "RI_ROOT"]);
    let mut t = BTreeSet::new();
    targets(
        "target: \"ri::stats\", log_on!(\"ri::canvas_debug\") \"RI_LOG=ri::keys\" \"ri::a,ri::b\"",
        &mut t,
    );
    let t: Vec<&str> = t.iter().map(String::as_str).collect();
    assert_eq!(t, ["ri::canvas_debug", "ri::stats"]);
    let d =
        "## A\n| Name | What |\n| --- | --- |\n| `x` | `y` |\n| `p=<n>` | z |\n## B\n| `q` | |\n";
    let a: Vec<String> = listed(d, "## A").into_iter().collect();
    assert_eq!(a, ["p=<n>", "x"]);
}
