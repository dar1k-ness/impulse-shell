//! The parser of QML documents: Qt 6.12's own ([`crate::qqmljs`]), which builds the
//! document of [`crate::ast`].

use crate::ast::*;

/// The first error Qt reports for a document, and the ones after it.
#[derive(Debug)]
pub struct Error {
    pub line: usize,
    /// In UTF-16 code units, as Qt counts a column.
    pub col: usize,
    pub msg: String,
    pub rest: Vec<Error>,
    /// Qt rejects the document without saying why (`x: function`): the `errorString()` of
    /// its component is empty and its status Null. The error here is ours.
    pub silent: bool,
}

impl Error {
    /// The errors as `QQmlComponent::errorString` lists them, without the new line after
    /// the last one: `url:line:column: message` each.
    pub fn under(&self, url: &str) -> String {
        let all = std::iter::once(self).chain(&self.rest);
        let lines: Vec<String> = all.map(|e| format!("{url}:{e}")).collect();
        lines.join("\n")
    }
}

/// `line:column: message`; like `QQmlError::toString` without a line or a column that is 0,
/// which is Qt's "none" (an automatic `;` at the start of a line is at column 0).
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (self.line, self.col) {
            (0, _) => write!(f, " {}", self.msg),
            (line, 0) => write!(f, "{line}: {}", self.msg),
            (line, col) => write!(f, "{line}:{col}: {}", self.msg),
        }
    }
}

/// The document of `src`, or what Qt 6.12 says is wrong with it.
pub fn parse(src: &str) -> Result<Document, Error> {
    static DOCS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let doc = DOCS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let error = |line: u32, col: u32, msg: String| Error {
        line: line as usize,
        col: col as usize,
        msg,
        rest: Vec::new(),
        silent: false,
    };
    crate::qqmljs::parse(src, doc).map_err(|failure| match failure {
        crate::qqmljs::Failure::Errors(errors) => {
            let mut errors = errors
                .into_iter()
                .map(|d| error(d.line, d.column, d.message));
            let mut first = errors.next().expect("a rejected document has an error");
            first.rest = errors.collect();
            first
        }
        crate::qqmljs::Failure::Silent(line, col) => Error {
            silent: true,
            ..error(line, col, "Syntax error".to_string())
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_ids_are_per_parse_and_survive_clones() {
        let src = "Item {\n    width: 1\n    Rectangle { height: 2 }\n}\n";
        let ids = |d: &Document| {
            let mut v = vec![];
            d.root.walk(&mut |o| {
                v.push(o.decl);
                let mut ss = vec![];
                o.scripts(&mut ss);
                v.extend(ss.iter().map(|s| s.id));
            });
            v
        };
        let a = parse(src).unwrap();
        let b = parse(src).unwrap();
        let (ia, ib) = (ids(&a), ids(&b));
        assert!(ia.iter().all(|i| !i.is_none() && i.doc == a.id), "{ia:?}");
        let mut uniq = ia.clone();
        uniq.sort();
        uniq.dedup();
        assert_eq!(uniq.len(), ia.len(), "{ia:?}");
        // The same text parsed again is another document: no id is shared.
        assert_ne!(a.id, b.id);
        assert!(ia.iter().all(|i| !ib.contains(i)));
        // A clone (a template instance) keeps the identity of what it was cloned from.
        assert_eq!(ids(&a.clone()), ia);
    }

    #[test]
    fn lines_and_columns_match_the_source() {
        let src = "import QtQuick\n// note\nItem {\n    /* a\n       b */\n    width: 1\n    property string s: \"x\\ny\"\n\n    Rectangle {\n        height: 2\n        function f() {\n            return 3\n        }\n    }\n}\n";
        let doc = parse(src).unwrap();
        assert_eq!(doc.root.line, 3);
        let mut lines = vec![];
        for m in &doc.root.members {
            match m {
                Member::Binding {
                    value: Value::Script(s),
                    ..
                } => lines.push(s.line),
                Member::Property {
                    value: Some(Value::Script(s)),
                    ..
                } => lines.push(s.line),
                Member::Child(o) => {
                    lines.push(o.line);
                    for m in &o.members {
                        match m {
                            Member::Binding {
                                value: Value::Script(s),
                                ..
                            } => lines.push(s.line),
                            Member::Function { script, .. } => lines.push(script.line),
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
        assert_eq!(lines, [6, 7, 9, 10, 11]);
        let e = parse("Item {\n  width: 1\n  height: (\n}\n").unwrap_err();
        assert_eq!(e.to_string(), "4:1: Expected token `)'");
        let e = parse("Item {}\nx").unwrap_err();
        assert_eq!((e.line, e.col), (2, 1), "{e}");
    }

    /// The names of `members`, each with the letter of its kind.
    fn names(members: &[Member]) -> Vec<String> {
        members
            .iter()
            .map(|m| match m {
                Member::Id(name) => format!("i {name}"),
                Member::Property { name, .. } => format!("p {name}"),
                Member::Signal { name, .. } => format!("s {name}"),
                Member::Function { name, .. } => format!("f {name}"),
                Member::Binding { name, .. } => format!("b {name}"),
                Member::Child(o) => format!("c {}", o.type_name),
                Member::Group { name, .. } => format!("g {name}"),
                _ => String::from("?"),
            })
            .collect()
    }

    /// Qt 6.12 reads an identifier of letters of any script (`QQmlJS::Lexer`).
    #[test]
    fn names_are_letters_of_any_script() {
        let src = "QtObject {\n    id: корень\n    property int ёж: 1\n    property int 𝒳: ёж + 1\n    property int ⅷ: 8\n    property int e\u{301}_ё$1٣: 2\n    signal сигнал(int число)\n    onЁжChanged: ёж++\n    onСигнал: (число) => число\n    function сумма(a, b) { return a + b }\n    корень.ёж: 2\n    Ёж { }\n    ёж { δ: 1 }\n}\n";
        let doc = parse(src).unwrap();
        assert_eq!(
            names(&doc.root.members),
            [
                "i корень",
                "p ёж",
                "p 𝒳",
                "p ⅷ",
                "p e\u{301}_ё$1٣",
                "s сигнал",
                "b onЁжChanged",
                "b onСигнал",
                "f сумма",
                "b корень.ёж",
                "c Ёж",
                "g ёж",
            ]
        );
        // `²` is a number that is not a digit and `·` is punctuation: neither is part of a name.
        for (src, col) in [
            ("QtObject {\n    property int x²: 1\n}\n", 19),
            ("QtObject {\n    property int a·b: 1\n}\n", 19),
            ("QtObject {\n    property int ²: 1\n}\n", 18),
            ("QtObject {\n    property int \u{301}e: 1\n}\n", 18),
        ] {
            let e = parse(src).unwrap_err();
            assert_eq!((e.line, e.col), (2, col), "{src}: {e}");
        }
    }

    /// Qt counts a column in UTF-16 code units: Qt 6.12 reports the name `жa` below at
    /// 4:60, thirteen columns after its `property`.
    #[test]
    fn columns_count_utf16_units() {
        let src = "QtObject {\n    property string out\n    property int ёж: 1\n    property int жё: 1; property int 𝒳𝒳: 2; property int жa: 2\n}\n";
        let doc = parse(src).unwrap();
        let at: Vec<_> = doc
            .root
            .members
            .iter()
            .filter_map(|m| match m {
                Member::Property { loc, .. } => Some(*loc),
                _ => None,
            })
            .collect();
        assert_eq!(at, [(2, 5), (3, 5), (4, 5), (4, 25), (4, 47)]);
        let e = parse("QtObject {\n    property var s: \"ёж𝒳\"; property int: 1\n}\n").unwrap_err();
        // Qt 6.12: column 41, "Expected token `identifier'".
        assert_eq!((e.line, e.col), (2, 41), "{e}");
    }
}
