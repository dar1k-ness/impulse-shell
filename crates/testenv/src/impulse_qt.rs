//! The `Impulse` module for the real Qt. The shell implements the module in Rust
//! (`crates/qml/src/impulse.rs`), so a tree that says `import Impulse` does not load in
//! Quickshell: `module "Impulse" is not installed`. A comparison that runs the tree in both
//! gives Quickshell this module instead: every type the `.qmltypes` files declare, with the
//! declared properties, signals, enums and methods, and methods that do nothing.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// `crates/qml/qmltypes/Impulse`: the files that declare the module.
fn declarations() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../qml/qmltypes/Impulse")
}

/// Writes the module as `<dir>/Impulse` (a `qmldir` and one `.qml` file per type). Quickshell
/// finds it with `QML_IMPORT_PATH=<dir>`.
pub fn impulse_qt_module(dir: &Path) {
    let mut sources = vec![];
    for e in std::fs::read_dir(declarations()).unwrap() {
        let p = e.unwrap().path();
        if p.extension().is_some_and(|x| x == "qmltypes") {
            sources.push(std::fs::read_to_string(&p).unwrap());
        }
    }
    sources.sort();
    let out = dir.join("Impulse");
    std::fs::create_dir_all(&out).unwrap();
    let mut qmldir = String::from("module Impulse\n");
    for (name, version, singleton, text) in sources.iter().flat_map(|s| types(s)) {
        let kind = if singleton { "singleton " } else { "" };
        writeln!(qmldir, "{kind}{name} {version} {name}.qml").unwrap();
        std::fs::write(out.join(format!("{name}.qml")), text).unwrap();
    }
    std::fs::write(out.join("qmldir"), qmldir).unwrap();
}

/// Copies the shell's tree `from` into `to` in real Quickshell's words. The tree names the
/// engine's module and the prefix of its own directories as this engine does, and Quickshell
/// knows neither name. A build with the interpreter reads both, so a comparison gives the
/// copy to both shells.
pub fn tree_in_quickshells_words(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let (src, dst) = (e.path(), to.join(e.file_name()));
        if std::fs::metadata(&src).unwrap().is_dir() {
            tree_in_quickshells_words(&src, &dst);
            continue;
        }
        let code = src
            .extension()
            .is_some_and(|x| x == "qml" || x == "js" || x == "mjs");
        match std::fs::read_to_string(&src) {
            Ok(text) if code => std::fs::write(&dst, in_quickshells_words(&text)).unwrap(),
            _ => {
                std::fs::copy(&src, &dst).unwrap();
            }
        }
    }
}

/// A file of the shell's tree in real Quickshell's words: its module, and the prefix of the
/// tree's own directories in an import.
pub fn in_quickshells_words(text: &str) -> String {
    whole_word(
        &whole_word(text, "Rustshell", "Quickshell"),
        "import ri",
        "import qs",
    )
}

/// `text` with `to` where `from` stands as a word of its own.
fn whole_word(text: &str, from: &str, to: &str) -> String {
    let in_a_word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find(from) {
        out.push_str(&rest[..i]);
        let after = &rest[i + from.len()..];
        let whole = !in_a_word(out.chars().next_back()) && !in_a_word(after.chars().next());
        out.push_str(if whole { to } else { from });
        rest = after;
    }
    out.push_str(rest);
    out
}

/// One `Name { key: value ... Child { } }` of a `.qmltypes` file.
#[derive(Default)]
struct Node {
    name: String,
    fields: Vec<(String, Vec<String>)>,
    children: Vec<Node>,
}

impl Node {
    /// The values of `key`: one for a scalar, each element for a list.
    fn list(&self, key: &str) -> &[String] {
        let f = self.fields.iter().find(|f| f.0 == key);
        f.map_or(&[], |f| f.1.as_slice())
    }

    fn get(&self, key: &str) -> &str {
        self.list(key).first().map_or("", |s| s.as_str())
    }

    fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> {
        self.children.iter().filter(move |c| c.name == name)
    }
}

/// The words, strings and punctuation of a `.qmltypes` file, comments left out.
fn tokens(src: &str) -> Vec<String> {
    let mut out = vec![];
    let mut it = src.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '/' if it.peek() == Some(&'/') => while it.next().is_some_and(|c| c != '\n') {},
            '"' => {
                let mut s = String::from('"');
                while let Some(c) = it.next().filter(|c| *c != '"') {
                    s.push(c);
                }
                out.push(s);
            }
            '{' | '}' | '[' | ']' | ':' => out.push(c.to_string()),
            c if c.is_alphanumeric() || c == '_' || c == '.' || c == '-' => {
                let mut s = String::from(c);
                while let Some(c) = it
                    .peek()
                    .copied()
                    .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '.')
                {
                    s.push(c);
                    it.next();
                }
                out.push(s);
            }
            _ => {}
        }
    }
    out
}

/// The node whose `{` is at `t[*i - 1]`, up to its `}`.
fn node(name: &str, t: &[String], i: &mut usize) -> Node {
    let mut n = Node {
        name: name.into(),
        ..Node::default()
    };
    while *i < t.len() && t[*i] != "}" {
        let word = t[*i].clone();
        *i += 2;
        match t[*i - 1].as_str() {
            "{" => n.children.push(node(&word, t, i)),
            ":" => {
                let mut values = vec![];
                if t[*i] == "[" {
                    *i += 1;
                    while t[*i] != "]" {
                        values.push(t[*i].trim_start_matches('"').to_string());
                        *i += 1;
                    }
                } else {
                    values.push(t[*i].trim_start_matches('"').to_string());
                }
                *i += 1;
                n.fields.push((word, values));
            }
            other => panic!("qmltypes: `{other}` after `{word}`"),
        }
    }
    *i += 1;
    n
}

/// What a property or a parameter of the C++ type `ty` is declared as in QML.
fn qml_type(ty: &str) -> &'static str {
    match ty {
        "QString" => "string",
        "bool" => "bool",
        "int" => "int",
        "double" | "qreal" | "float" => "real",
        _ => "var",
    }
}

/// `(name, version, singleton, QML text)` of every exported type `src` declares.
fn types(src: &str) -> Vec<(String, String, bool, String)> {
    let t = tokens(src);
    let start = t.iter().position(|w| w == "Module").expect("no Module") + 2;
    let module = node("Module", &t, &mut { start });
    let mut out = vec![];
    for c in module.all("Component") {
        let Some((name, version)) = c
            .get("exports")
            .strip_prefix("Impulse/")
            .and_then(|e| e.split_once(' '))
        else {
            continue;
        };
        let singleton = c.get("isSingleton") == "true";
        let mut q = String::new();
        if singleton {
            q.push_str("pragma Singleton\n");
        }
        // An element that is a Process (`job` in its `impulse::Element`) is one for Qt too:
        // the tree sets `running`, `stdout` and `onExited` on it.
        let (import, base) = match c.get("prototype") {
            "QQuickItem" => ("QtQuick", "Item"),
            "Process" => ("QtQuick\nimport Quickshell.Io", "Process"),
            _ => ("QtQuick", "QtObject"),
        };
        writeln!(q, "import {import}\n\n{base} {{").unwrap();
        for e in c.all("Enum") {
            let values = e.list("values").join(", ");
            writeln!(q, "    enum {} {{ {values} }}", e.get("name")).unwrap();
        }
        let mut notifiers = vec![];
        for p in c.all("Property") {
            let (name, ty) = (p.get("name"), p.get("type"));
            let default = if c.get("defaultProperty") == name {
                "default "
            } else {
                ""
            };
            let ty = match (p.get("isList") == "true", default) {
                (true, "default ") => "list<QtObject>",
                _ => qml_type(ty),
            };
            writeln!(q, "    {default}property {ty} {name}").unwrap();
            notifiers.push(format!("{name}Changed"));
        }
        // A property already has its `<name>Changed`: declaring it again is an error.
        for s in c.all("Signal") {
            if notifiers.iter().any(|n| n == s.get("name")) {
                continue;
            }
            let params: Vec<_> = s
                .all("Parameter")
                .map(|p| format!("{} {}", qml_type(p.get("type")), p.get("name")))
                .collect();
            writeln!(q, "    signal {}({})", s.get("name"), params.join(", ")).unwrap();
        }
        for m in c.all("Method") {
            let params: Vec<_> = m.all("Parameter").map(|p| p.get("name")).collect();
            writeln!(
                q,
                "    function {}({}) {{}}",
                m.get("name"),
                params.join(", ")
            )
            .unwrap();
        }
        q.push_str("}\n");
        out.push((name.to_string(), version.to_string(), singleton, q));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = r#"import QtQuick.tooling 1.2
// A comment with { braces }.
Module {
    Component {
        name: "ImpulseWarp"
        prototype: "QObject"
        exports: ["Impulse/Warp 1.0"]
        isCreatable: false
        isSingleton: true
        exportMetaObjectRevisions: [256]
        Property { name: "connected"; type: "bool"; isReadonly: true; index: 0 }
        Signal { name: "connectedChanged" }
        Signal {
            name: "failed"
            Parameter { name: "why"; type: "QString" }
            Parameter { name: "code"; type: "int" }
        }
        Method { name: "toggle" }
        Method {
            name: "connectTo"
            type: "bool"
            Parameter { name: "server"; type: "QString" }
            Parameter { name: "options"; type: "QVariant" }
        }
    }
    Component {
        name: "ImpulseHost"
        prototype: "QQuickItem"
        exports: ["Impulse/Host 1.0"]
        defaultProperty: "widgets"
        Enum {
            name: "Status"
            values: ["Null", "Ready"]
        }
        Property { name: "widgets"; type: "QObject"; isList: true }
        Property { name: "ratio"; type: "double" }
    }
    Component {
        name: "ImpulseDownload"
        prototype: "Process"
        exports: ["Impulse/Download 1.0"]
        Property { name: "url"; type: "QString" }
    }
    Component { name: "NotExported"; prototype: "QObject" }
}
"#;

    #[test]
    fn a_singleton_and_an_element() {
        let t = types(SRC);
        assert_eq!(t.len(), 3);
        assert_eq!(
            t[2].3,
            "import QtQuick\nimport Quickshell.Io\n\nProcess {\n    property string url\n}\n"
        );
        assert_eq!(
            (t[0].0.as_str(), t[0].1.as_str(), t[0].2),
            ("Warp", "1.0", true)
        );
        assert_eq!(
            t[0].3,
            "pragma Singleton\nimport QtQuick\n\nQtObject {\n    property bool connected\n    \
             signal failed(string why, int code)\n    function toggle() {}\n    \
             function connectTo(server, options) {}\n}\n"
        );
        assert_eq!(
            (t[1].0.as_str(), t[1].1.as_str(), t[1].2),
            ("Host", "1.0", false)
        );
        assert_eq!(
            t[1].3,
            "import QtQuick\n\nItem {\n    enum Status { Null, Ready }\n    \
             default property list<QtObject> widgets\n    property real ratio\n}\n"
        );
    }

    #[test]
    fn the_module_of_the_repository() {
        let dir = tempfile::tempdir().unwrap();
        impulse_qt_module(dir.path());
        let qmldir = std::fs::read_to_string(dir.path().join("Impulse/qmldir")).unwrap();
        let mut lines = qmldir.lines();
        assert_eq!(lines.next(), Some("module Impulse"));
        let mut n = 0;
        for l in lines {
            let file = l.rsplit(' ').next().unwrap();
            assert!(dir.path().join("Impulse").join(file).is_file(), "{l}");
            n += 1;
        }
        assert!(n > 0, "no type in {}", declarations().display());
    }
}
