//! The document ([`crate::ast`]) of a parse. The QML rules of qqmljs.g build it as they
//! reduce, with the tokens Qt reports errors at; [`super::ir`] then checks it as Qt's
//! `QQmlJS::IRBuilder` checks its tree, and what is left is the document.

use super::grammar::RHS;
use super::grammar::T_FEED_UI_PROGRAM;
#[cfg(feature = "interp")]
use super::grammar::{T_FEED_JS_EXPRESSION, T_FEED_JS_SCRIPT};
use super::lexer::Loc;
use super::parser::{Diagnostic, Kind, Parser, Span, valid};
use crate::ast::*;

/// The most nested objects Qt takes: its visitors go 4096 calls deep, three for an object.
const MAX_DEPTH: u32 = 1364;

/// `QV4::Compiler::Codegen::isNameGlobal` (qv4codegen.cpp): the names of the global object.
pub(super) const GLOBAL_NAMES: &[&str] = &[
    "Array",
    "ArrayBuffer",
    "Atomics",
    "Boolean",
    "DOMException",
    "DataView",
    "Date",
    "Error",
    "EvalError",
    "Float32Array",
    "Float64Array",
    "Function",
    "Infinity",
    "Int16Array",
    "Int32Array",
    "Int8Array",
    "JSON",
    "Map",
    "Math",
    "NaN",
    "Number",
    "Object",
    "Promise",
    "Proxy",
    "QT_TRANSLATE_NOOP",
    "QT_TRID_NOOP",
    "QT_TR_NOOP",
    "Qt",
    "RangeError",
    "ReferenceError",
    "Reflect",
    "RegExp",
    "SQLException",
    "Set",
    "SharedArrayBuffer",
    "String",
    "Symbol",
    "SyntaxError",
    "TypeError",
    "URIError",
    "URL",
    "URLSearchParams",
    "Uint16Array",
    "Uint32Array",
    "Uint8Array",
    "Uint8ClampedArray",
    "WeakMap",
    "WeakSet",
    "XMLHttpRequest",
    "console",
    "decodeURI",
    "decodeURIComponent",
    "encodeURI",
    "encodeURIComponent",
    "escape",
    "eval",
    "gc",
    "isFinite",
    "isNaN",
    "parseFloat",
    "parseInt",
    "print",
    "qsTr",
    "qsTrId",
    "qsTranslate",
    "undefined",
    "unescape",
];

/// An object, or the group of a grouped property (`anchors { }`), as written.
pub(super) struct RawObject {
    /// The names of its type, or of the property.
    pub(super) name: String,
    /// Its first token and the last of its names.
    pub(super) at: Loc,
    pub(super) last: Loc,
    pub(super) members: Vec<RawMember>,
    /// None of a group.
    decl: NodeId,
}

/// The name of a binding: its first token and the last of its names.
#[derive(Clone, Copy)]
pub(super) struct NameAt {
    pub(super) first: Loc,
    pub(super) last: Loc,
}

pub(super) struct EnumValue {
    pub(super) name: String,
    pub(super) at: Loc,
    /// The text of the value, the value and where it is.
    pub(super) value: Option<(String, f64, Loc)>,
}

pub(super) enum RawValue {
    /// A script, and whether it is the expression `null`.
    Script(Script, bool),
    Object(RawObject),
    List(Vec<RawObject>),
}

pub(super) struct RawProperty {
    pub(super) name: String,
    pub(super) ty: String,
    modifiers: Modifiers,
    pub(super) value: Option<RawValue>,
    /// The tokens of the name, of `default` and of a word before `<` that is not `list`.
    pub(super) name_at: Loc,
    pub(super) default_at: Loc,
    pub(super) modifier_at: Option<Loc>,
}

impl RawProperty {
    pub(super) fn modifiers_default(&self) -> bool {
        self.modifiers.default
    }
}

/// A member of an object as written; `first` is its first token.
pub(super) struct RawMember {
    pub(super) first: Loc,
    pub(super) kind: RawKind,
}

pub(super) enum RawKind {
    /// `id: name`: what the value says and where it is.
    Id(String, Loc),
    Property(RawProperty),
    Required(String),
    /// A signal, the token of its name and its parameters.
    Signal(String, Loc, String),
    /// A function and the token of its name.
    Function(String, Loc, Script),
    /// `var x`.
    Declaration,
    Binding(String, NameAt, RawValue),
    Group(RawObject),
    /// `Type on property { }`.
    On(String, NameAt, RawObject),
    Child(RawObject),
    Component(String, RawObject),
    Enum(String, Vec<EnumValue>),
}

/// What a QML symbol on the stack is worth.
#[derive(Default)]
pub(super) enum Ui {
    #[default]
    None,
    Text(String),
    /// `ExpressionStatement`: where its expression ends, and whether that is a string
    /// literal or `null`.
    Expression {
        end: u32,
        string: bool,
        null: bool,
    },
    Pragma(String),
    /// With the token of a file name.
    Import(Import, Option<Loc>),
    Header(Vec<String>, Vec<Import>),
    /// The name of a function and where it is.
    Function(String, Loc),
    /// Members and how deep objects nest in them.
    Members(Vec<RawMember>, u32),
    Member(RawMember, u32),
    /// `UiObjectDefinition`: an object, or the group of a grouped property.
    Definition(RawObject, u32),
    Objects(Vec<RawObject>, u32),
    EnumValues(Vec<EnumValue>),
}

/// What the rules have built so far.
pub(super) struct Build {
    doc: u64,
    nodes: u32,
    header: Option<(Vec<String>, Vec<Import>)>,
    root: Option<RawObject>,
    /// The errors of `IRBuilder` in the pragmas and the imports.
    header_errors: Vec<Diagnostic>,
    /// The names the imports give their modules (`import QtQuick as Q`).
    qualifiers: Vec<String>,
    /// The pragmas so far, by what Qt calls them.
    pragmas: Vec<&'static str>,
    /// The errors of `QQmlPropertyValidator`, which runs on a document without the above.
    late: Vec<Diagnostic>,
    too_deep: bool,
}

impl Build {
    pub(super) fn new(doc: u64) -> Build {
        Build {
            doc,
            nodes: 0,
            header: None,
            root: None,
            header_errors: Vec::new(),
            qualifiers: Vec::new(),
            pragmas: Vec::new(),
            late: Vec::new(),
            too_deep: false,
        }
    }

    fn node(&mut self) -> NodeId {
        self.nodes = self
            .nodes
            .checked_add(1)
            .expect("qml: more than u32::MAX nodes");
        NodeId {
            doc: self.doc,
            index: self.nodes,
        }
    }
}

/// `QChar::isUpper`, `isLower` and `isLetter` of the first UTF-16 unit of `s`, as Qt asks
/// about a name: a letter outside the BMP starts with a surrogate, which is none of these.
/// The Roman numerals are numbers (Nl) that Unicode also calls upper and lower case.
fn first(s: &str, is: fn(char) -> bool) -> bool {
    s.starts_with(|c: char| is(c) && c.len_utf16() == 1 && !('\u{2160}'..='\u{2188}').contains(&c))
}

pub(super) fn upper(s: &str) -> bool {
    first(s, char::is_uppercase)
}

pub(super) fn lower(s: &str) -> bool {
    first(s, char::is_lowercase)
}

pub(super) fn letter(s: &str) -> bool {
    first(s, char::is_alphabetic)
}

fn position(at: Loc) -> (usize, usize) {
    (at.line as usize, at.column as usize)
}

/// The values of the pragmas that take some, and what Qt calls each pragma in an error.
const PRAGMAS: &[(&str, &str, &[&str])] = &[
    (
        "ComponentBehavior",
        "component behavior",
        &["Unbound", "Bound"],
    ),
    (
        "ListPropertyAssignBehavior",
        "list property assign behavior",
        &["Append", "Replace", "ReplaceIfNotDefault"],
    ),
    (
        "FunctionSignatureBehavior",
        "function signature behavior",
        &["Ignored", "Enforced"],
    ),
    (
        "NativeMethodBehavior",
        "native method behavior",
        &["AcceptThisObject", "RejectThisObject"],
    ),
    (
        "ValueTypeBehavior",
        "value type behavior",
        &[
            "Reference",
            "Copy",
            "Inaddressable",
            "Addressable",
            "Inassertable",
            "Assertable",
        ],
    ),
];

/// The type Qt's compiler names when a literal initial value cannot be a `ty` property
/// (QQmlPropertyValidator "Invalid property assignment: … expected").
fn literal_mismatch(ty: &str, src: &str) -> Option<&'static str> {
    enum Lit {
        Num(f64),
        Str,
        Bool,
    }
    let src = src.trim();
    // A trailing comment is not part of the value.
    let end = match src.as_bytes().first() {
        Some(&q @ (b'"' | b'\'')) => src[1..].find(q as char).map_or(src.len(), |e| e + 2),
        _ => src
            .find("//")
            .or_else(|| src.find("/*"))
            .unwrap_or(src.len()),
    };
    let rest = src[end..].trim_start();
    if !(rest.is_empty() || rest.starts_with("//") || rest.starts_with("/*")) {
        return None;
    }
    let src = src[..end].trim_end();
    let lit = match src {
        "true" | "false" => Lit::Bool,
        _ if src.len() >= 2
            && matches!(src.as_bytes()[0], b'"' | b'\'')
            && src.ends_with(&src[..1])
            && !src[1..src.len() - 1].contains(&src[..1]) =>
        {
            Lit::Str
        }
        _ => {
            let (neg, n) = match src.strip_prefix('-') {
                Some(n) => (-1.0, n.trim_start()),
                None => (1.0, src),
            };
            let v = match n.strip_prefix("0x").or_else(|| n.strip_prefix("0X")) {
                Some(h) => u64::from_str_radix(h, 16).ok().map(|h| h as f64),
                None if n.starts_with(|c: char| c.is_ascii_digit() || c == '.') => n.parse().ok(),
                None => None,
            };
            Lit::Num(neg * v?)
        }
    };
    match (ty, lit) {
        ("int", Lit::Num(v))
            if v.fract() == 0.0 && v >= i32::MIN as f64 && v <= i32::MAX as f64 =>
        {
            None
        }
        ("int", _) => Some("int"),
        ("real" | "double", Lit::Str | Lit::Bool) => Some("number"),
        ("string", Lit::Num(_) | Lit::Bool) => Some("string"),
        ("bool", Lit::Num(_) | Lit::Str) => Some("boolean"),
        ("url", Lit::Num(_) | Lit::Bool) => Some("url"),
        ("color", Lit::Num(_) | Lit::Bool) => Some("color"),
        _ => None,
    }
}

impl Parser {
    fn take(&mut self, k: usize) -> Ui {
        std::mem::take(&mut self.ui[self.tos + k - 1])
    }

    fn span(&self, k: usize) -> Span {
        self.spans[self.tos + k - 1].unwrap_or_default()
    }

    /// The text of symbol `k`.
    fn symbol(&self, k: usize) -> String {
        let s = self.span(k);
        self.slice(s.first.offset, s.end)
    }

    /// The names of symbol `k`, a `UiQualifiedId` or a `UiPropertyType`.
    fn names(&mut self, k: usize) -> String {
        match self.take(k) {
            Ui::Text(t) => t,
            _ => self.symbol(k).split_whitespace().collect(),
        }
    }

    /// Where the names of symbol `k`, a `UiQualifiedId`, are.
    fn name_at(&self, k: usize) -> NameAt {
        NameAt {
            first: self.span(k).first,
            last: self.sym(k).name_at,
        }
    }

    fn header_error(&mut self, at: Loc, message: String) {
        self.build.header_errors.push(Diagnostic {
            line: at.line,
            column: at.column,
            message,
        });
    }

    /// Where the white space and the comments that follow unit `i` on its line end.
    fn after_comments(&self, mut i: usize) -> usize {
        let code = self.lexer.code();
        let at = |i: usize| code.get(i).copied().unwrap_or(0);
        loop {
            if [b' ' as u16, b'\t' as u16, b'\r' as u16].contains(&at(i)) {
                i += 1;
            } else if at(i) == b'/' as u16 && at(i + 1) == b'/' as u16 {
                while i < code.len() && code[i] != b'\n' as u16 {
                    i += 1;
                }
            } else if at(i) == b'/' as u16 && at(i + 1) == b'*' as u16 {
                i += 2;
                while i < code.len() && !(code[i] == b'*' as u16 && at(i + 1) == b'/' as u16) {
                    i += 1;
                }
                i = (i + 2).min(code.len());
            } else {
                return i;
            }
        }
    }

    /// The script of statement or function `k`: its text without the `;` that ends it and
    /// with the comments after it on its line.
    fn script(&mut self, k: usize, function: bool) -> Script {
        let s = self.span(k);
        let code = self.lexer.code();
        let end = s.end as usize;
        let empty = end == s.first.offset as usize + 1;
        let end = if !empty && end > 0 && code.get(end - 1) == Some(&(b';' as u16)) {
            end - 1
        } else {
            self.after_comments(end)
        };
        let text = self.slice(s.first.offset, end as u32);
        let src = text.trim_end();
        let kind = if function {
            ScriptKind::Function
        } else if src.starts_with('{') {
            ScriptKind::Block
        } else {
            ScriptKind::Expression
        };
        Script {
            shape: Shape::of(src, kind),
            src: src.into(),
            line: s.first.line as usize,
            col: if function { 0 } else { s.first.column as usize },
            kind,
            id: self.build.node(),
        }
    }

    /// The value of a binding to statement `k`.
    fn script_value(&mut self, k: usize) -> RawValue {
        let null = matches!(self.ui[self.tos + k - 1], Ui::Expression { null: true, .. });
        RawValue::Script(self.script(k, false), null)
    }

    /// What statement `k` says the id is (`IRBuilder::setId`).
    fn id(&mut self, k: usize) -> String {
        let s = self.span(k);
        match self.take(k) {
            Ui::Expression {
                end, string: true, ..
            } => self.slice(s.first.offset + 1, end.saturating_sub(1)),
            Ui::Expression { end, .. } => self.slice(s.first.offset, end),
            _ => self.slice(s.first.offset, s.end),
        }
    }

    /// The object of the type named by symbol `name` with the members of symbol `members`,
    /// and how deep objects nest in it.
    fn object(&mut self, name: usize, members: usize) -> (RawObject, u32) {
        let (members, depth) = self.initializer(members);
        let object = RawObject {
            at: self.span(name).first,
            last: self.sym(name).name_at,
            name: self.names(name),
            members,
            decl: NodeId::NONE,
        };
        self.define(object, depth)
    }

    /// The definition `d` is an object.
    fn define(&mut self, mut d: RawObject, depth: u32) -> (RawObject, u32) {
        let mut depth = depth + 1;
        if depth > MAX_DEPTH {
            self.build.too_deep = true;
            d.members.clear();
            depth = 1;
        }
        d.decl = self.build.node();
        (d, depth)
    }

    fn property(&mut self, name: usize, ty: String, value: Option<RawValue>) -> Ui {
        let attrs = self.sym(1).attrs;
        if let Some(RawValue::Script(v, _)) = &value
            && let Some(want) = literal_mismatch(&ty, &v.src)
        {
            self.build.late.push(Diagnostic {
                line: v.line as u32,
                column: v.col as u32,
                message: format!("Invalid property assignment: {want} expected"),
            });
        }
        let property = RawProperty {
            name: self.symbol(name),
            ty,
            modifiers: Modifiers {
                default: valid(attrs.default),
                readonly: valid(attrs.readonly),
                required: valid(attrs.required),
                final_: valid(attrs.r#final),
            },
            value,
            name_at: self.span(name).first,
            default_at: attrs.default,
            modifier_at: None,
        };
        self.member(RawKind::Property(property), 0)
    }

    /// The property of a type `identifier < UiPropertyType >` at symbols 2 to 5.
    fn list_property(&mut self, name: usize, value: Option<RawValue>, depth: u32) -> Ui {
        let modifier = self.symbol(2);
        let ty = format!(
            "{modifier}{}",
            self.slice(self.span(3).first.offset, self.span(5).end)
        );
        let modifier_at = (modifier != "list").then(|| self.span(2).first);
        match self.property(name, ty, value) {
            Ui::Member(mut m, _) => {
                if let RawKind::Property(p) = &mut m.kind {
                    p.modifier_at = modifier_at;
                }
                Ui::Member(m, depth)
            }
            other => other,
        }
    }

    /// A member that starts at symbol 1.
    fn member(&self, kind: RawKind, depth: u32) -> Ui {
        let first = self.span(1).first;
        Ui::Member(RawMember { first, kind }, depth)
    }

    fn objects(&mut self, k: usize) -> (Vec<RawObject>, u32) {
        match self.take(k) {
            Ui::Objects(os, depth) => (os, depth),
            _ => (Vec::new(), 0),
        }
    }

    fn initializer(&mut self, k: usize) -> (Vec<RawMember>, u32) {
        match self.take(k) {
            Ui::Members(ms, depth) => (ms, depth),
            _ => (Vec::new(), 0),
        }
    }

    fn enum_value(&mut self, k: usize, number: Option<usize>) -> EnumValue {
        EnumValue {
            name: self.symbol(k),
            at: self.span(k).first,
            value: number.map(|n| {
                let from = self.span(k + 2).first;
                let text = self.slice(from.offset, self.span(n).end);
                let v = self.sym(n).dval;
                (text, if n == k + 2 { v } else { -v }, from)
            }),
        }
    }

    /// `IRBuilder::visit(UiPragma)` for `pragma` at symbol 1 with a name at symbol 2 and
    /// values up to symbol `last`.
    fn pragma(&mut self, last: usize) -> Ui {
        let at = self.span(1).first;
        let name = self.symbol(2);
        let text = self.slice(self.span(2).first.offset, self.span(last).end);
        let values = match self.take(4) {
            Ui::Text(values) if last == 4 => values,
            _ => String::new(),
        };
        if let Some(&(_, called, known)) = PRAGMAS.iter().find(|p| p.0 == name) {
            let unknown = values
                .split(',')
                .find(|v| !v.is_empty() && !known.contains(v));
            if self.build.pragmas.contains(&called) {
                self.header_error(at, format!("Multiple {called} pragmas found"));
            } else if let Some(v) = unknown {
                self.header_error(at, format!("Unknown {called} '{v}' in pragma"));
            } else {
                self.build.pragmas.push(called);
            }
        } else if !["Singleton", "Strict", "Translator"].contains(&name.as_str()) {
            self.header_error(at, format!("Unknown pragma '{name}'"));
        }
        Ui::Pragma(text)
    }

    /// `IRBuilder::visit(UiImport)` for the import of symbol 1, with the version and the
    /// name at these symbols.
    fn import(&mut self, version: Option<usize>, alias: Option<usize>) -> Ui {
        let Ui::Import(mut import, file) = self.take(1) else {
            return Ui::None;
        };
        import.version = version.map(|k| self.symbol(k));
        if let Some(k) = alias {
            let name = self.symbol(k);
            let at = self.span(k).first;
            if !upper(&name) {
                let message = format!(
                    "Invalid import qualifier '{name}': must start with an uppercase letter"
                );
                self.header_error(at, message);
            } else if name == "Qt" {
                let message = "Reserved name \"Qt\" cannot be used as an qualifier";
                self.header_error(at, message.to_string());
            } else {
                self.build.qualifiers.push(name.clone());
            }
            import.alias = Some(name);
        } else if let Some(at) = file {
            let path = import.uri.trim_matches(['"', '\'']);
            if path.ends_with(".js") || path.ends_with(".mjs") {
                self.header_error(at, "Script import requires a qualifier".to_string());
            }
        }
        Ui::Import(import, file)
    }

    /// Builds what rule `r` reduces to, before its symbols leave the stack.
    pub(super) fn build(&mut self, r: usize) {
        let ui = match r {
            // UiProgram: UiHeaderItemListOpt UiRootMember
            6 => {
                if let Ui::Header(pragmas, imports) = self.take(1) {
                    self.build.header = Some((pragmas, imports));
                }
                if let Ui::Definition(d, depth) = self.take(2) {
                    self.build.root = Some(self.define(d, depth).0);
                }
                Ui::None
            }
            // UiHeaderItemList: UiPragma | UiImport, and the list with one more
            9..=12 => {
                let (mut pragmas, mut imports) = match (r, self.take(1)) {
                    (11 | 12, Ui::Header(ps, is)) => (ps, is),
                    (_, item) => {
                        self.ui[self.tos] = item;
                        Default::default()
                    }
                };
                match self.take(if r < 11 { 1 } else { 2 }) {
                    Ui::Pragma(p) => pragmas.push(p),
                    Ui::Import(i, _) => imports.push(i),
                    _ => {}
                }
                Ui::Header(pragmas, imports)
            }
            // UiPragmaValueList: PragmaValue | UiPragmaValueList , PragmaValue
            18 => Ui::Text(self.symbol(1).trim_matches(['"', '\'']).to_string()),
            19 => match self.take(1) {
                Ui::Text(values) => {
                    Ui::Text(values + "," + self.symbol(3).trim_matches(['"', '\'']))
                }
                _ => Ui::None,
            },
            // UiPragma: pragma PragmaId Semicolon, pragma PragmaId : UiPragmaValueList Semicolon
            20 => self.pragma(2),
            21 => self.pragma(4),
            // UiImport: UiImportHead [UiVersionSpecifier] [as QmlIdentifier] Semicolon
            23 => self.import(None, None),
            26 => self.import(Some(2), None),
            27 => self.import(Some(2), Some(4)),
            28 => self.import(None, Some(3)),
            // UiImportHead: import ImportId
            29 => {
                let file = (self.sym(2).kind == Kind::StringLiteral).then(|| self.span(2).first);
                let uri = match file {
                    Some(_) => self.symbol(2),
                    None => self.symbol(2).split_whitespace().collect(),
                };
                #[cfg(feature = "interp")]
                let uri = match file {
                    Some(_) => uri,
                    None => crate::types::own_uri(uri),
                };
                let import = Import {
                    uri,
                    version: None,
                    alias: None,
                };
                Ui::Import(import, file)
            }
            // UiAnnotatedObject: UiAnnotationList UiObjectDefinition
            // UiAnnotatedObjectMember: UiAnnotationList UiObjectMember
            38 | 47 => self.take(2),
            // UiObjectMemberList
            40 | 41 => {
                let (mut members, mut depth) = match r {
                    40 => (Vec::new(), 0),
                    _ => self.initializer(1),
                };
                if let Ui::Member(m, d) = self.take(if r == 40 { 1 } else { 2 }) {
                    members.push(m);
                    depth = depth.max(d);
                }
                Ui::Members(members, depth)
            }
            // UiArrayMemberList
            42 | 43 => {
                let (mut objects, mut depth) = match (r, self.take(1)) {
                    (43, Ui::Objects(os, depth)) => (os, depth),
                    (_, first) => {
                        self.ui[self.tos] = first;
                        Default::default()
                    }
                };
                if let Ui::Definition(d, inner) = self.take(if r == 42 { 1 } else { 3 }) {
                    let (o, d) = self.define(d, inner);
                    objects.push(o);
                    depth = depth.max(d);
                }
                Ui::Objects(objects, depth)
            }
            // UiObjectInitializer: { } | { UiObjectMemberList }
            44 => Ui::Members(Vec::new(), 0),
            45 => self.take(2),
            // UiObjectDefinition: UiQualifiedId UiObjectInitializer
            46 => {
                let (members, depth) = self.initializer(2);
                let definition = RawObject {
                    at: self.span(1).first,
                    last: self.sym(1).name_at,
                    name: self.names(1),
                    members,
                    decl: NodeId::NONE,
                };
                Ui::Definition(definition, depth)
            }
            // UiObjectMember: UiObjectDefinition. What the grammar cannot tell apart: an
            // object, or the group of a property, whose name is no type's.
            49 => match self.take(1) {
                Ui::Definition(d, depth)
                    if upper(d.name.rsplit('.').next().unwrap_or_default()) =>
                {
                    let (o, depth) = self.define(d, depth);
                    self.member(RawKind::Child(o), depth)
                }
                Ui::Definition(d, depth) => self.member(RawKind::Group(d), depth + 1),
                _ => Ui::None,
            },
            // UiObjectMember: UiQualifiedId : ExpressionStatementLookahead
            //     [ UiArrayMemberList CommaOpt ]
            52 => {
                let (objects, depth) = self.objects(5);
                let at = self.name_at(1);
                let binding = RawKind::Binding(self.names(1), at, RawValue::List(objects));
                self.member(binding, depth)
            }
            // UiObjectMember: UiQualifiedId : ExpressionStatementLookahead
            //     UiQualifiedId UiObjectInitializer
            53 => {
                let (o, depth) = self.object(4, 5);
                let at = self.name_at(1);
                let binding = RawKind::Binding(self.names(1), at, RawValue::Object(o));
                self.member(binding, depth)
            }
            // UiObjectMember: UiQualifiedId on UiQualifiedId UiObjectInitializer
            54 => {
                let (o, depth) = self.object(1, 4);
                let at = self.name_at(3);
                let prop = self.names(3);
                self.member(RawKind::On(prop, at, o), depth)
            }
            // UiScriptStatement: its ExpressionStatement, after what the lookahead put before
            57 => self.take(3),
            61 => self.take(2),
            // UiObjectMember: UiQualifiedId : UiScriptStatement
            66 => {
                let at = self.name_at(1);
                let name = self.names(1);
                let value = self.script_value(3);
                let kind = if name == "id" {
                    RawKind::Id(self.id(3), self.span(3).first)
                } else {
                    RawKind::Binding(name, at, value)
                };
                self.member(kind, 0)
            }
            // UiObjectMember: signal identifier ( UiParameterListOpt ) Semicolon
            //     | signal identifier Semicolon
            78 | 79 => {
                let params = match r {
                    78 => self.slice(self.span(3).end, self.span(5).first.offset),
                    _ => String::new(),
                };
                let signal = RawKind::Signal(self.symbol(2), self.span(2).first, params);
                self.member(signal, 0)
            }
            // UiObjectMemberListPropertyNoInitialiser: UiPropertyAttributes identifier <
            //     UiPropertyType > QmlIdentifier Semicolon
            93 => self.list_property(6, None, 0),
            // UiObjectMemberPropertyNoInitialiser: UiPropertyAttributes UiPropertyType
            //     QmlIdentifier Semicolon
            95 => {
                let ty = self.names(2);
                self.property(3, ty, None)
            }
            // UiRequired: required QmlIdentifier Semicolon
            99 => self.member(RawKind::Required(self.symbol(2)), 0),
            // UiObjectMemberWithScriptStatement: UiPropertyAttributes UiPropertyType
            //     QmlIdentifier : UiScriptStatement OptionalSemicolon
            101 => {
                let ty = self.names(2);
                let value = self.script_value(5);
                self.property(3, ty, Some(value))
            }
            // The same of `identifier < UiPropertyType >`
            102 => {
                let value = self.script_value(8);
                self.list_property(6, Some(value), 0)
            }
            // UiObjectMemberWithArray: UiPropertyAttributes identifier < UiPropertyType >
            //     QmlIdentifier : ExpressionStatementLookahead [ UiArrayMemberList CommaOpt ]
            //     Semicolon
            104 => {
                let (objects, depth) = self.objects(10);
                self.list_property(6, Some(RawValue::List(objects)), depth)
            }
            // UiObjectMemberExpressionStatementLookahead: UiPropertyAttributes UiPropertyType
            //     QmlIdentifier : ExpressionStatementLookahead UiQualifiedId
            //     UiObjectInitializer Semicolon
            106 => {
                let ty = self.names(2);
                let (o, depth) = self.object(6, 7);
                match self.property(3, ty, Some(RawValue::Object(o))) {
                    Ui::Member(m, _) => Ui::Member(m, depth),
                    other => other,
                }
            }
            // UiObjectMember: GeneratorDeclaration | FunctionDeclarationWithTypes
            108 | 109 => {
                let (name, at) = match self.take(1) {
                    Ui::Function(name, at) => (name, at),
                    _ => Default::default(),
                };
                let script = self.script(1, true);
                self.member(RawKind::Function(name, at, script), 0)
            }
            // UiObjectMember: VariableStatement
            110 => self.member(RawKind::Declaration, 0),
            // UiQualifiedId: MemberExpression
            111 => {
                let e = self.sym(1);
                let s = self.span(1);
                // Qt ignores `[ ]` after the names.
                let end = match e.kind {
                    Kind::ArrayMember => e.at.offset,
                    _ => s.end,
                };
                Ui::Text(self.slice(s.first.offset, end).split_whitespace().collect())
            }
            // UiObjectMember: enum identifier { EnumMemberList }
            112 => {
                let values = match self.take(4) {
                    Ui::EnumValues(vs) => vs,
                    _ => Vec::new(),
                };
                self.member(RawKind::Enum(self.symbol(2), values), 0)
            }
            // UiObjectMember: component identifier : UiObjectDefinition
            113 => match self.take(4) {
                Ui::Definition(d, depth) => {
                    let (o, depth) = self.define(d, depth);
                    self.member(RawKind::Component(self.symbol(2), o), depth)
                }
                _ => Ui::None,
            },
            // EnumMemberList
            114..=119 => {
                let mut values = match (r, self.take(1)) {
                    (117..=119, Ui::EnumValues(vs)) => vs,
                    _ => Vec::new(),
                };
                let k = if r < 117 { 1 } else { 3 };
                let number = match r {
                    114 | 117 => None,
                    115 | 118 => Some(k + 2),
                    _ => Some(k + 3),
                };
                values.push(self.enum_value(k, number));
                Ui::EnumValues(values)
            }
            // ExpressionStatement: Expression_In Semicolon
            501 => Ui::Expression {
                end: self.span(1).end,
                string: self.sym(1).kind == Kind::StringLiteral,
                null: self.sym(1).kind == Kind::Null,
            },
            // FunctionDeclarationWithTypes: Function BindingIdentifier ( FormalParameters )
            //     TypeAnnotationOpt FunctionLBrace FunctionBody FunctionRBrace
            // GeneratorDeclaration: FunctionStar BindingIdentifier GeneratorLParen
            //     FormalParameters ) FunctionLBrace GeneratorBody GeneratorRBrace
            547 | 579 => Ui::Function(self.symbol(2), self.span(2).first),
            _ if RHS[r] == 1 => return,
            _ => Ui::None,
        };
        self.ui[self.tos] = ui;
    }
}

/// The object of the document that `o` is, when Qt has nothing against it.
fn object(o: RawObject) -> Object {
    let at = position(o.at);
    Object {
        type_name: o.name,
        scope: Default::default(),
        members: members(o.members, at),
        line: at.0,
        col: at.1,
        decl: o.decl,
    }
}

/// The members of the object or group at `at`.
fn members(members: Vec<RawMember>, at: (usize, usize)) -> Vec<Member> {
    stacker::maybe_grow(64 << 10, 1 << 20, || {
        let value = |v| match v {
            RawValue::Script(s, _) => Value::Script(s),
            RawValue::Object(o) => Value::Object(object(o)),
            RawValue::List(os) => Value::List(os.into_iter().map(object).collect()),
        };
        let member = |m: RawMember| {
            Some(match m.kind {
                RawKind::Id(name, _) => Member::Id(name),
                RawKind::Property(p) => Member::Property {
                    name: p.name,
                    ty: p.ty,
                    modifiers: p.modifiers,
                    value: p.value.map(value),
                    loc: position(m.first),
                },
                // `required foo` is reported at the object that re-declares it
                // (QQmlPropertyValidator).
                RawKind::Required(name) => Member::Required { name, loc: at },
                RawKind::Signal(name, _, params) => Member::Signal { name, params },
                RawKind::Function(name, _, script) => Member::Function { name, script },
                RawKind::Declaration => return None,
                RawKind::Binding(name, _, v) => Member::Binding {
                    name,
                    value: value(v),
                },
                RawKind::Group(g) => Member::Group {
                    members: self::members(g.members, position(g.at)),
                    name: g.name,
                },
                RawKind::On(prop, _, o) => Member::On {
                    prop,
                    object: object(o),
                },
                RawKind::Child(o) => Member::Child(object(o)),
                RawKind::Component(name, o) => Member::Component {
                    name,
                    object: object(o),
                },
                RawKind::Enum(name, values) => Member::Enum {
                    name,
                    values: values
                        .into_iter()
                        .map(|v| (v.name, v.value.map(|v| v.0)))
                        .collect(),
                },
            })
        };
        members.into_iter().filter_map(member).collect()
    })
}

/// Why Qt 6.12 does not take a document.
pub enum Failure {
    Errors(Vec<Diagnostic>),
    /// Qt's parser gives up without an error when it meets one right after a token it
    /// looked ahead of (`x: function`): here, at this line and column.
    Silent(u32, u32),
}

/// The document of `src`, numbered `doc`.
pub fn parse(src: &str, doc: u64) -> Result<Document, Failure> {
    let mut p = Parser::new(src, doc);
    p.parse(T_FEED_UI_PROGRAM);
    if !p.diagnostics.is_empty() {
        return Err(Failure::Errors(p.diagnostics));
    }
    let build = p.build;
    let Some(root) = build.root else {
        return Err(Failure::Silent(p.loc.line, p.loc.column));
    };
    let errors = if build.too_deep {
        // `IRBuilder::throwRecursionDepthError`.
        vec![Diagnostic {
            line: 0,
            column: 0,
            message: "Maximum statement or expression depth exceeded".to_string(),
        }]
    } else {
        super::ir::check(
            &root,
            build.header_errors,
            &build.qualifiers,
            p.lexer.code(),
        )
    };
    match [errors, build.late].into_iter().find(|e| !e.is_empty()) {
        Some(errors) => Err(Failure::Errors(errors)),
        None => {
            let (pragmas, imports) = build.header.unwrap_or_default();
            let mut doc = Document {
                id: build.doc,
                pragmas,
                imports,
                root: object(root),
            };
            crate::qualified::resolve(&mut doc);
            Ok(doc)
        }
    }
}

/// `src` with the `async` of every `for (async of` spelled `\u0061sync`: QQmlJS takes that
/// name for the loop's target like any other, ECMAScript reserves the three tokens for
/// `for (async of => ...;;)`, and an escaped name is outside that rule. `src` is a script,
/// an expression or the parameters of a function; the tokens are those Qt's parser reads.
#[cfg(feature = "interp")]
pub fn for_async_of(src: &str) -> std::borrow::Cow<'_, str> {
    if !src.contains("async") {
        return src.into();
    }
    let mut found = None;
    for feed in [T_FEED_JS_SCRIPT, T_FEED_JS_EXPRESSION] {
        let mut p = Parser::new(src, 0);
        p.parse(feed);
        // Past a syntax error the tokens are not the script's: an object literal is no
        // script, and reads as an expression.
        let whole = p.diagnostics.is_empty();
        if whole || found.is_none() {
            found = Some(p.async_of);
        }
        if whole {
            break;
        }
    }
    let found = found.unwrap_or_default();
    if found.is_empty() {
        return src.into();
    }
    let mut out = String::with_capacity(src.len() + 5 * found.len());
    let mut units = 0;
    let mut chars = src.chars();
    for at in found {
        while units < at {
            let c = chars.next().unwrap_or_default();
            units += c.len_utf16() as u32;
            out.push(c);
        }
        out += "\\u0061";
        chars.next();
        units += 1;
    }
    out.extend(chars);
    out.into()
}
