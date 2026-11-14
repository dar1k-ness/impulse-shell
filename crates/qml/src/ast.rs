#[derive(Debug, Clone)]
pub struct Document {
    /// Unique per parse ([`NodeId::doc`] of every node in it).
    pub id: u64,
    pub pragmas: Vec<String>,
    pub imports: Vec<Import>,
    pub root: Object,
}

#[derive(Debug, Clone)]
pub struct Import {
    /// Module URI (`QtQuick.Layouts`) or quoted path (`"foo.js"`).
    pub uri: String,
    pub version: Option<String>,
    pub alias: Option<String>,
}

/// Where the type of an [`Object`] is looked up.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum Scope {
    /// No import qualifier in front of the name: an inline component, a QML file or a C++
    /// type of that name.
    #[default]
    Plain,
    /// `P.Part` behind the qualifier `P`: a QML file of the imports with that qualifier, or
    /// nothing. `type_name` is as written.
    Qualified,
    /// `QQ.Timer`: the C++ type `Timer` of this module, which is what `type_name` holds. No
    /// QML file or inline component of that name stands for it.
    Module(std::rc::Rc<str>),
}

#[derive(Debug, Clone)]
pub struct Object {
    pub type_name: String,
    /// What the import qualifier in front of the written name came to (`crate::qualified`).
    pub scope: Scope,
    pub members: Vec<Member>,
    pub line: usize,
    /// 1-based column of the type name (0: not from a parsed document).
    pub col: usize,
    /// Identity of the declaration, shared by clones (templates copy object trees).
    pub decl: NodeId,
}

#[derive(Debug, Clone)]
pub struct Script {
    pub src: std::rc::Rc<str>,
    pub line: usize,
    /// 1-based column of the first character (0: not from a parsed document).
    pub col: usize,
    pub kind: ScriptKind,
    /// What QmlIR makes of the binding, decided when it is parsed (also without the
    /// interpreter).
    pub shape: Shape,
    /// Identity of the script, shared by clones (templates copy object trees).
    pub id: NodeId,
}

impl Script {
    /// A script that is not part of a parsed document ([`NodeId::NONE`]).
    pub fn new(src: &str, line: usize, kind: ScriptKind) -> Script {
        Script {
            src: src.into(),
            line,
            col: 0,
            kind,
            shape: Shape::of(src, kind),
            id: NodeId::NONE,
        }
    }
}

/// What Qt's `QQmlJS::IRBuilder::setBindingValue` makes of a binding: an expression statement
/// that is a single string (or substitution-free template), number (also under a unary
/// minus), boolean or null literal is a constant; a `Type.Value` / `Type.Enum.Value` member
/// chain is one when QQmlEnumTypeResolver resolves it to an enum; anything else, including
/// a parenthesized literal, is a JS binding. Comments and white space do not count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shape {
    Literal,
    /// Two or three identifiers joined by `.`, the first one capitalized: `Type.Value`.
    Member(std::rc::Rc<str>),
    Script,
}

impl Shape {
    /// The shape of `src`, from its tokens.
    pub fn of(src: &str, kind: ScriptKind) -> Shape {
        if kind != ScriptKind::Expression {
            return Shape::Script;
        }
        let mut lx = Lexer { s: src, i: 0 };
        let mut toks = Vec::with_capacity(6);
        while let Some(t) = lx.next() {
            // `A.B.C;` is the longest constant.
            if toks.len() == 6 {
                return Shape::Script;
            }
            toks.push(t);
        }
        if toks.last() == Some(&Tok::Punct(';')) {
            toks.pop();
        }
        use Tok::*;
        match toks.as_slice() {
            [Num | Str] | [Punct('-'), Num] => Shape::Literal,
            [Ident("true" | "false" | "null")] => Shape::Literal,
            [Ident(a), Punct('.'), Ident(b)] if capitalized(a) => {
                Shape::Member(format!("{a}.{b}").into())
            }
            [Ident(a), Punct('.'), Ident(b), Punct('.'), Ident(c)] if capitalized(a) => {
                Shape::Member(format!("{a}.{b}.{c}").into())
            }
            _ => Shape::Script,
        }
    }
}

fn capitalized(s: &str) -> bool {
    s.starts_with(|c: char| c.is_ascii_uppercase())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tok<'a> {
    Ident(&'a str),
    /// A NumericLiteral.
    Num,
    /// A string literal or a template literal without substitutions.
    Str,
    Punct(char),
    /// Anything that is no token of a constant (a template with substitutions, a malformed
    /// literal).
    Other,
}

/// The ECMAScript tokens of a binding expression, skipping white space, line terminators and
/// comments (ECMA-262 §12).
struct Lexer<'a> {
    s: &'a str,
    i: usize,
}

fn line_terminator(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

fn ident_part(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '$' | '\u{200c}' | '\u{200d}')
}

impl<'a> Lexer<'a> {
    fn rest(&self) -> &'a str {
        &self.s[self.i..]
    }

    fn skip_trivia(&mut self) -> bool {
        loop {
            let r = self.rest();
            if let Some(c) = r
                .chars()
                .next()
                .filter(|&c| c.is_whitespace() || c == '\u{feff}')
            {
                self.i += c.len_utf8();
            } else if r.starts_with("//") {
                self.i += r.find(line_terminator).unwrap_or(r.len());
            } else if let Some(body) = r.strip_prefix("/*") {
                match body.find("*/") {
                    Some(e) => self.i += e + 4,
                    None => return false,
                }
            } else {
                return true;
            }
        }
    }

    fn next(&mut self) -> Option<Tok<'a>> {
        if !self.skip_trivia() {
            self.i = self.s.len();
            return Some(Tok::Other);
        }
        let r = self.rest();
        let c = r.chars().next()?;
        let st = self.i;
        let take = |l: &mut Self, n: usize| {
            l.i += n;
            &l.s[st..l.i]
        };
        Some(match c {
            '"' | '\'' | '`' => {
                let mut esc = false;
                let mut end = None;
                for (k, d) in r.char_indices().skip(1) {
                    match d {
                        _ if esc => esc = false,
                        '\\' => esc = true,
                        '$' if c == '`' && r[k + 1..].starts_with('{') => break,
                        _ if d == c => {
                            end = Some(k + 1);
                            break;
                        }
                        _ if c != '`' && line_terminator(d) => break,
                        _ => {}
                    }
                }
                match end {
                    Some(n) => {
                        take(self, n);
                        Tok::Str
                    }
                    None => {
                        self.i = self.s.len();
                        Tok::Other
                    }
                }
            }
            _ if c.is_ascii_digit()
                || (c == '.' && r[1..].starts_with(|d: char| d.is_ascii_digit())) =>
            {
                let radix = r.len() > 1
                    && matches!(r.as_bytes()[1], b'x' | b'X' | b'o' | b'O' | b'b' | b'B')
                    && c == '0';
                let mut prev = ' ';
                let n = r
                    .char_indices()
                    .find(|&(_, d)| {
                        let part = d.is_ascii_alphanumeric()
                            || matches!(d, '_' | '.')
                            || (matches!(d, '+' | '-') && matches!(prev, 'e' | 'E') && !radix);
                        prev = d;
                        !part
                    })
                    .map_or(r.len(), |(k, _)| k);
                if numeric_literal(take(self, n)) {
                    Tok::Num
                } else {
                    Tok::Other
                }
            }
            _ if c.is_alphabetic() || matches!(c, '_' | '$') => {
                let n = r.find(|d: char| !ident_part(d)).unwrap_or(r.len());
                Tok::Ident(take(self, n))
            }
            _ => {
                take(self, c.len_utf8());
                Tok::Punct(c)
            }
        })
    }
}

/// An ECMAScript NumericLiteral (§12.9.3): decimal, or `0x`/`0o`/`0b` with digits of that
/// radix, `_` separators between digits.
fn numeric_literal(s: &str) -> bool {
    let radix = |p: &str, r: u32| {
        s.get(..2).is_some_and(|h| h.eq_ignore_ascii_case(p))
            && separated(&s[2..], |c| c.is_digit(r))
    };
    if radix("0x", 16) || radix("0o", 8) || radix("0b", 2) {
        return true;
    }
    let (mant, exp) = match s.find(['e', 'E']) {
        Some(k) => (&s[..k], Some(s[k + 1..].trim_start_matches(['+', '-']))),
        None => (s, None),
    };
    let (int, frac) = mant.split_once('.').unwrap_or((mant, ""));
    let digits = |p: &str| separated(p, |c| c.is_ascii_digit());
    (!int.is_empty() || !frac.is_empty())
        && (int.is_empty() || digits(int))
        && (frac.is_empty() || digits(frac))
        && exp.is_none_or(digits)
}

/// Non-empty digits with single `_` separators between them.
fn separated(s: &str, digit: impl Fn(char) -> bool) -> bool {
    !s.is_empty()
        && !s.starts_with('_')
        && !s.ends_with('_')
        && !s.contains("__")
        && s.chars().all(|c| c == '_' || digit(c))
}

/// Identity of a node (object declaration or script) of a parsed document: the parse it
/// came from and its ordinal there, in source order. Parses never share a `doc`, so an id
/// stays unique for the life of the process, whether or not its document is still alive.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId {
    pub doc: u64,
    pub index: u32,
}

impl NodeId {
    /// Not from a parsed document (objects and scripts the runtime builds itself).
    pub const NONE: NodeId = NodeId { doc: 0, index: 0 };

    pub fn is_none(self) -> bool {
        self.doc == 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptKind {
    Expression,
    Block,
    Function,
}

#[derive(Debug, Clone)]
pub enum Value {
    Script(Script),
    Object(Object),
    List(Vec<Object>),
}

#[derive(Debug, Clone, Default)]
pub struct Modifiers {
    pub default: bool,
    pub readonly: bool,
    pub required: bool,
    pub final_: bool,
}

#[derive(Debug, Clone)]
pub enum Member {
    Id(String),
    Property {
        name: String,
        ty: String,
        modifiers: Modifiers,
        value: Option<Value>,
        /// (line, column) where the declaration starts, its first modifier or `property`.
        loc: (usize, usize),
    },
    /// `required foo` re-declaring an inherited property as required.
    Required {
        name: String,
        /// (line, column) of the object it belongs to, where Qt reports it unset.
        loc: (usize, usize),
    },
    Signal {
        name: String,
        params: String,
    },
    Function {
        name: String,
        script: Script,
    },
    Binding {
        name: String,
        value: Value,
    },
    /// `anchors { fill: parent }`
    Group {
        name: String,
        members: Vec<Member>,
    },
    /// `Behavior on color { ... }`
    On {
        prop: String,
        object: Object,
    },
    Child(Object),
    Component {
        name: String,
        object: Object,
    },
    Enum {
        name: String,
        values: Vec<(String, Option<String>)>,
    },
}

impl Object {
    pub fn walk<'a>(&'a self, f: &mut dyn FnMut(&'a Object)) {
        f(self);
        fn members<'a>(ms: &'a [Member], f: &mut dyn FnMut(&'a Object)) {
            for m in ms {
                match m {
                    Member::Child(o)
                    | Member::On { object: o, .. }
                    | Member::Component { object: o, .. } => o.walk(f),
                    Member::Group { members: ms, .. } => members(ms, f),
                    Member::Property { value: Some(v), .. } | Member::Binding { value: v, .. } => {
                        match v {
                            Value::Object(o) => o.walk(f),
                            Value::List(os) => os.iter().for_each(|o| o.walk(f)),
                            Value::Script(_) => {}
                        }
                    }
                    _ => {}
                }
            }
        }
        members(&self.members, f);
    }

    pub fn scripts<'a>(&'a self, out: &mut Vec<&'a Script>) {
        fn members<'a>(ms: &'a [Member], out: &mut Vec<&'a Script>) {
            for m in ms {
                match m {
                    Member::Function { script, .. } => out.push(script),
                    Member::Group { members: ms, .. } => members(ms, out),
                    Member::Property {
                        value: Some(Value::Script(s)),
                        ..
                    }
                    | Member::Binding {
                        value: Value::Script(s),
                        ..
                    } => out.push(s),
                    _ => {}
                }
            }
        }
        members(&self.members, out);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::{ScriptKind, Shape};

    /// Binding sources and what QmlIR makes of them (Qt 6.12 `IRBuilder::setBindingValue`).
    pub(crate) const SHAPES: &[(&str, Option<&str>)] = &[
        ("5", None),
        ("-1", None),
        ("- /* c */ 1.5e-3", None),
        ("0x1F", None),
        ("1_000", None),
        (".5", None),
        ("5 // c", None),
        ("5; // c", None),
        ("\"a\" /* x */", None),
        ("'it\\'s'", None),
        ("true // c", None),
        ("null", None),
        ("`a`", None),
        ("`a\\${b}`", None),
        ("Text.AlignRight // c", Some("Text.AlignRight")),
        ("Text /* c */ . AlignRight", Some("Text.AlignRight")),
        (
            "Qt.AlignmentFlag.AlignLeft",
            Some("Qt.AlignmentFlag.AlignLeft"),
        ),
    ];

    /// Sources QmlIR compiles as JS bindings.
    pub(crate) const SCRIPTS: &[&str] = &[
        "(5)",
        "+5",
        "!false",
        "- -5",
        "-true",
        "1 + 1",
        "`${a}`",
        "`a${b}c`",
        "5px",
        "1__0",
        "5..toString()",
        "\"a\" + \"b\"",
        "undefined",
        "root.x",
        "Text.AlignRight.x.y",
        "Text?.AlignRight",
        "Text[\"AlignRight\"]",
        "{ 5 }",
        "/* c */ a",
        "[1, 2]",
        "({a: 1})",
        "qsTr(\"x\")",
        "x // \"a\"",
    ];

    pub(crate) fn expected(s: Option<&str>) -> Shape {
        s.map_or(Shape::Literal, |m| Shape::Member(m.into()))
    }

    #[test]
    fn binding_shapes_follow_qmlir_tokens_not_text() {
        for &(src, want) in SHAPES {
            assert_eq!(
                Shape::of(src, ScriptKind::Expression),
                expected(want),
                "{src}"
            );
        }
        for src in SCRIPTS {
            assert_eq!(
                Shape::of(src, ScriptKind::Expression),
                Shape::Script,
                "{src}"
            );
        }
        assert_eq!(Shape::of("5", ScriptKind::Block), Shape::Script);
    }
}
