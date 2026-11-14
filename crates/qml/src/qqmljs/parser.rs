//! Qt 6.12's QML parser (`QQmlJS::Parser`: the driver, the error recovery and the actions of
//! qqmljs.g). It accepts the documents Qt's parser accepts and reports Qt's errors for the
//! others. Of JavaScript it builds no tree: it keeps what the actions of the grammar ask
//! about an expression, and the document takes a script as its source text (build.rs).

use super::build::Ui;
use super::grammar::*;
use super::lexer::{Lexer, Loc, RegExpPrefix};
use std::collections::VecDeque;

/// An error of the parser: `line` and `column` from 1, in UTF-16 units as Qt counts them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub line: u32,
    pub column: u32,
    pub message: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum Kind {
    #[default]
    Other,
    /// `IdentifierExpression`.
    Ident,
    /// `FieldMemberExpression`.
    Field,
    /// `ArrayMemberExpression`; `at` is its `[`.
    ArrayMember,
    Call,
    /// `NullExpression`.
    Null,
    StringLiteral,
    /// `NestedExpression`.
    Nested,
    /// `ArrayPattern` or `ObjectPattern`.
    Pattern,
    /// A `BinaryExpression` of `&&` or `||`; `at` is the operator.
    AndOr,
    /// A `BinaryExpression` of `=`.
    Assign,
    /// `CommaExpression`.
    Comma,
    /// `TypeAnnotation`; `at` is its `:`.
    Annotation,
}

/// The tokens of the attributes of a property declaration (`UiPropertyAttributes`).
#[derive(Clone, Copy, Default)]
pub(super) struct Attrs {
    pub(super) required: Loc,
    pub(super) default: Loc,
    pub(super) readonly: Loc,
    pub(super) r#final: Loc,
    pub(super) r#virtual: Loc,
    pub(super) r#override: Loc,
}

/// What the actions need of a symbol on the stack.
#[derive(Clone, Copy, Default)]
pub(super) struct Val {
    pub(super) kind: Kind,
    /// `reparseAsQualifiedId` answers: an identifier, or `.name`s on one.
    qualified: bool,
    /// Of an `ArrayMember`: whether its base is `qualified`.
    base_qualified: bool,
    /// `containsOptionalChain`.
    optional: bool,
    /// `reparseAsFormalParameterList` answers; of a `Nested`, for the expression in it.
    formals: bool,
    pub(super) at: Loc,
    /// Of an identifier or `.name`s on one, also under `[ ]`: its last name.
    pub(super) name_at: Loc,
    pub(super) dval: f64,
    pub(super) attrs: Attrs,
}

/// The text of a symbol: where its first token is and where its last one ends.
#[derive(Clone, Copy, Default)]
pub(super) struct Span {
    pub(super) first: Loc,
    pub(super) end: u32,
}

#[derive(Clone, Copy, Default)]
struct Token {
    token: i32,
    dval: f64,
    loc: Loc,
}

#[derive(PartialEq, Eq)]
enum Cover {
    Invalid,
    Parenthesized,
    FormalParameterList,
}

pub(super) struct Parser {
    pub(super) lexer: Lexer,
    states: Vec<i32>,
    pub(super) vals: Vec<Val>,
    locs: Vec<Loc>,
    /// Beside Qt's stacks: the text of each symbol and what the document is built of.
    pub(super) spans: Vec<Option<Span>>,
    pub(super) ui: Vec<Ui>,
    pub(super) tos: usize,
    /// The current token, -1 for none.
    token: i32,
    dval: f64,
    pub(super) loc: Loc,
    prev_loc: Loc,
    buffer: VecDeque<Token>,
    pub(super) diagnostics: Vec<Diagnostic>,
    pub(super) build: super::build::Build,
    function_nesting: i32,
    class_nesting: i32,
    cover: Cover,
    cover_error: Loc,
    /// The last three tokens the lexer gave, and where each `async` of a `for (async of`
    /// starts.
    #[cfg(feature = "interp")]
    last: [(i32, Loc); 3],
    #[cfg(feature = "interp")]
    pub(super) async_of: Vec<u32>,
}

pub(super) fn valid(l: Loc) -> bool {
    l != Loc::default()
}

impl Parser {
    pub(super) fn new(src: &str, doc: u64) -> Parser {
        Parser {
            lexer: Lexer::new(src.encode_utf16().collect()),
            states: Vec::new(),
            vals: Vec::new(),
            locs: Vec::new(),
            spans: Vec::new(),
            ui: Vec::new(),
            tos: 0,
            token: -1,
            dval: 0.0,
            loc: Loc::default(),
            prev_loc: Loc::default(),
            buffer: VecDeque::new(),
            diagnostics: Vec::new(),
            build: super::build::Build::new(doc),
            // `Parser::parse()` counts the document as a function: a binding may `return`.
            function_nesting: 1,
            class_nesting: 0,
            cover: Cover::Invalid,
            cover_error: Loc::default(),
            #[cfg(feature = "interp")]
            last: [(-1, Loc::default()); 3],
            #[cfg(feature = "interp")]
            async_of: Vec::new(),
        }
    }

    pub(super) fn error(&mut self, at: Loc, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic {
            line: at.line,
            column: at.column,
            message: message.into(),
        });
    }

    /// `sym(k)` of a reduction.
    pub(super) fn sym(&self, k: usize) -> Val {
        self.vals[self.tos + k - 1]
    }

    /// `loc(k)` of a reduction.
    pub(super) fn at(&self, k: usize) -> Loc {
        self.locs[self.tos + k - 1]
    }

    fn text(&self, at: Loc) -> String {
        self.slice(at.offset, at.offset + at.length)
    }

    /// The text from unit `a` to unit `b`.
    pub(super) fn slice(&self, a: u32, b: u32) -> String {
        let code = self.lexer.code();
        let b = (b as usize).min(code.len());
        let a = (a as usize).min(b);
        String::from_utf16_lossy(&code[a..b])
    }

    fn lex(&mut self) {
        self.token = self.lexer.lex();
        self.dval = self.lexer.token_value();
        self.loc = self.lexer.loc();
        #[cfg(feature = "interp")]
        {
            let [(a, _), (b, _), (c, name)] = self.last;
            if [a, b, c, self.token] == [T_FOR, T_LPAREN, T_IDENTIFIER, T_OF]
                && self.text(name) == "async"
            {
                self.async_of.push(name.offset);
            }
            self.last = [self.last[1], self.last[2], (self.token, self.loc)];
        }
    }

    fn push_token(&mut self, token: i32) {
        self.buffer.push_back(Token {
            token: self.token,
            dval: self.dval,
            loc: self.loc,
        });
        self.token = token;
    }

    fn push_token_with_empty_location(&mut self, token: i32) {
        self.push_token(token);
        self.loc = self.prev_loc;
        self.loc.offset += self.loc.length;
        self.loc.column += self.loc.length;
        self.loc.length = 0;
    }

    fn lookahead_token(&mut self) -> i32 {
        if self.token < 0 {
            self.lex();
        }
        self.token
    }

    /// `Parser::parse`. Qt keeps what is above the top of its stacks, and an empty rule
    /// without an action reads it; so do the vectors here.
    /// `Parser::parse`, of a QML document (`feed` T_FEED_UI_PROGRAM) or of what another
    /// `feed` names. It ends with an error, with a document, or with
    /// neither (see [`super::build::Failure::Silent`]).
    pub(super) fn parse(&mut self, feed: i32) {
        let mut action = 0;
        self.buffer.push_back(Token {
            token: feed,
            ..Token::default()
        });
        let mut started = false;
        // False where Qt jumps to `_Lcheck_token`, into the loop and past the push.
        let mut push = true;
        loop {
            loop {
                if push {
                    self.tos = if started { self.tos + 1 } else { 0 };
                    started = true;
                    if self.tos + 16 > self.states.len() {
                        let n = (self.states.len() * 2).max(128);
                        self.states.resize(n, 0);
                        self.vals.resize(n, Val::default());
                        self.locs.resize(n, Loc::default());
                        self.spans.resize(n, None);
                        self.ui.resize_with(n, Ui::default);
                    }
                    self.states[self.tos] = action;
                }
                push = true;
                if self.token == -1 && -TERMINAL_COUNT != ACTION_INDEX[action as usize] as i32 {
                    self.prev_loc = self.loc;
                    match self.buffer.pop_front() {
                        Some(t) => {
                            self.token = t.token;
                            self.dval = t.dval;
                            self.loc = t.loc;
                        }
                        None => self.lex(),
                    }
                }
                action = t_action(action, self.token);
                if action > 0 {
                    if action == ACCEPT_STATE {
                        return;
                    }
                    self.token = -1;
                    self.vals[self.tos] = Val {
                        dval: self.dval,
                        ..Val::default()
                    };
                    self.locs[self.tos] = self.loc;
                    self.spans[self.tos] = Some(Span {
                        first: self.loc,
                        end: self.loc.offset + self.loc.length,
                    });
                    self.ui[self.tos] = Ui::None;
                } else if action < 0 {
                    let r = (-action - 1) as usize;
                    self.tos -= RHS[r] as usize;
                    if !self.reduce(r) {
                        return;
                    }
                    action = nt_action(self.states[self.tos], LHS[r] as i32 - TERMINAL_COUNT);
                }
                if action == 0 {
                    break;
                }
            }
            if !self.buffer.is_empty() {
                return;
            }
            let error_state = self.states[self.tos];
            // Automatic insertion of `;`.
            if self.token != -1
                && ((t_action(error_state, T_AUTOMATIC_SEMICOLON) != 0
                    && self.lexer.can_insert_automatic_semicolon(self.token))
                    || t_action(error_state, T_COMPATIBILITY_SEMICOLON) != 0)
            {
                self.push_token_with_empty_location(T_SEMICOLON);
                action = error_state;
                push = false;
                continue;
            }
            let bad = self.token;
            let bad_at = self.loc;
            self.lex();
            if t_action(error_state, self.token) != 0 {
                let message = if (0..TERMINAL_COUNT).contains(&bad) {
                    format!("Unexpected token `{}'", SPELL[bad as usize].unwrap_or(""))
                } else {
                    "Syntax error".to_string()
                };
                self.error(bad_at, message);
                action = error_state;
                push = false;
                continue;
            }
            const FIRST: [i32; 13] = [
                T_PLUS,
                T_EQ,
                T_COMMA,
                T_COLON,
                T_SEMICOLON,
                T_RPAREN,
                T_RBRACKET,
                T_RBRACE,
                T_NUMERIC_LITERAL,
                T_IDENTIFIER,
                T_LPAREN,
                T_LBRACKET,
                T_LBRACE,
            ];
            let rest = (1..TERMINAL_COUNT).filter(|tk| {
                ![
                    T_AUTOMATIC_SEMICOLON,
                    T_FEED_UI_PROGRAM,
                    T_FEED_JS_STATEMENT,
                    T_FEED_JS_EXPRESSION,
                ]
                .contains(tk)
            });
            let next = self.token;
            let missing = FIRST.into_iter().chain(rest).find(|&tk| {
                let a = t_action(error_state, tk);
                a > 0 && t_action(a, next) != 0
            });
            let Some(tk) = missing else {
                self.error(bad_at, "Syntax error");
                return;
            };
            self.error(
                bad_at,
                format!("Expected token `{}'", SPELL[tk as usize].unwrap_or("")),
            );
            self.push_token(tk);
            // As in Qt, `action` is 0 here: the inserted token meets the first state, which
            // does not take it, and the parse ends with this one error.
            push = false;
        }
    }

    /// Reduces by rule `r`; false where Qt's action returns false from `parse`.
    fn reduce(&mut self, r: usize) -> bool {
        let v = if RULE_HAS_ACTION[r] {
            match self.action(r) {
                Some(v) => Some(v),
                None => return false,
            }
        } else {
            None
        };
        self.build(r);
        if let Some(v) = v {
            self.vals[self.tos] = v;
        }
        let n = RHS[r] as usize;
        if n != 1 {
            let mut symbols = self.spans[self.tos..self.tos + n].iter().flatten();
            self.spans[self.tos] = symbols.next().copied().map(|first| Span {
                first: first.first,
                end: symbols.next_back().unwrap_or(&first).end,
            });
        }
        if r == 238 || r == 239 {
            // The token `/` became a regular expression.
            if let Some(s) = &mut self.spans[self.tos] {
                s.end = s.first.offset + self.locs[self.tos].length;
            }
        }
        true
    }

    /// The action of rule `r` in qqmljs.g, and the value it leaves; none where it returns
    /// false.
    fn action(&mut self, r: usize) -> Option<Val> {
        let mut v = Val::default();
        match r {
            35 | 60..=65 | 423..=435 | 599 => v = self.sym(2),
            57..=59 | 422 => v = self.sym(3),
            24 | 25 => {
                let segment = |d: f64| (0.0..255.0).contains(&d);
                if !segment(self.sym(1).dval) || (r == 24 && !segment(self.sym(3).dval)) {
                    let message = if r == 24 {
                        "Invalid version. Version numbers must be >= 0 and < 255."
                    } else {
                        "Invalid major version. Version numbers must be >= 0 and < 255."
                    };
                    self.error(self.at(1), message);
                    return None;
                }
            }
            29 => {
                let id = self.sym(2);
                if id.kind != Kind::StringLiteral && !id.qualified {
                    self.error(
                        self.at(1),
                        "Expected a qualified name id or a string literal",
                    );
                    return None;
                }
            }
            32 => {
                v.kind = Kind::Ident;
                v.qualified = true;
                v.name_at = self.at(1);
            }
            33 => {
                v.kind = Kind::Field;
                v.qualified = true;
                v.name_at = self.at(3);
            }
            67 => {
                self.error(
                    self.at(1),
                    "Incomplete binding, expected token `:` or `{`",
                );
                return None;
            }
            86..=91 => {
                v = self.sym(2);
                let here = self.at(1);
                let a = v.attrs;
                const VIRTUAL_FINAL: &str = "The 'virtual' cannot be combined with 'final', as these attributes are mutually exclusive";
                const OVERRIDE_FINAL: &str =
                    "'override' is redundant when a property is marked as 'final'";
                const VIRTUAL_OVERRIDE: &str = "'virtual' is redundant when overriding a property. The 'override' must only be used when actually overriding an existing property; using it on a new property is an error.";
                match r {
                    86 => {
                        if valid(a.required) {
                            self.error(
                                a.required,
                                "Duplicated 'required' attribute is not allowed.",
                            );
                        }
                        v.attrs.required = here;
                    }
                    87 => {
                        if valid(a.default) {
                            self.error(
                                a.required,
                                "Duplicated 'default' attribute is not allowed.",
                            );
                        }
                        v.attrs.default = here;
                    }
                    88 => {
                        if valid(a.readonly) {
                            self.error(
                                a.required,
                                "Duplicated 'readonly' attribute is not allowed.",
                            );
                        }
                        v.attrs.readonly = here;
                    }
                    89 => {
                        if valid(a.r#final) {
                            self.error(a.r#final, "Duplicated 'final' attribute is not allowed.");
                        }
                        v.attrs.r#final = here;
                        if valid(a.r#virtual) {
                            self.error(a.r#virtual, VIRTUAL_FINAL);
                        }
                        if valid(a.r#override) {
                            self.error(a.r#override, OVERRIDE_FINAL);
                        }
                    }
                    90 => {
                        if valid(a.r#virtual) {
                            self.error(
                                a.r#virtual,
                                "Duplicated 'virtual' attribute is not allowed.",
                            );
                        }
                        if valid(a.r#override) {
                            self.error(here, VIRTUAL_OVERRIDE);
                        }
                        if valid(a.r#final) {
                            self.error(here, VIRTUAL_FINAL);
                        }
                        v.attrs.r#virtual = here;
                    }
                    _ => {
                        if valid(a.r#override) {
                            self.error(
                                a.r#override,
                                "Duplicated 'override' attribute is not allowed.",
                            );
                        }
                        if valid(a.r#virtual) {
                            self.error(a.r#virtual, VIRTUAL_OVERRIDE);
                        }
                        if valid(a.r#final) {
                            self.error(here, OVERRIDE_FINAL);
                        }
                        v.attrs.r#override = here;
                    }
                }
            }
            95 => {
                let a = self.sym(1).attrs;
                if valid(a.readonly) {
                    self.error(a.readonly, "Read-only properties require an initializer.");
                }
            }
            101 | 102 | 104 => {
                let a = self.sym(1).attrs;
                if valid(a.required) {
                    self.error(
                        a.required,
                        "Required properties with initializer do not make sense.",
                    );
                }
            }
            111 => {
                let e = self.sym(1);
                let qualified = if e.kind == Kind::ArrayMember {
                    e.base_qualified
                } else {
                    e.qualified
                };
                if !qualified {
                    self.error(self.at(1), "Expected a qualified name id");
                    return None;
                }
                v.name_at = e.name_at;
            }
            195 | 199 => {
                let at = self.at(1);
                let which = if r == 195 { "Reserved" } else { "ECMAScript" };
                let word = self.text(at);
                self.error(
                    at,
                    format!("{which} keyword \"{word}\" cannot be used as a QML identifier."),
                );
            }
            209 => self.error(
                self.at(1),
                "QML does not have an `enum` type. Use int, or use double if the enum's underlying type does not fit into int.",
            ),
            214 => {
                v.kind = Kind::Annotation;
                v.at = self.at(1);
            }
            218 => {
                v.kind = Kind::Ident;
                v.qualified = true;
                v.formals = true;
                v.name_at = self.at(1);
            }
            227 => {
                if self.cover != Cover::Parenthesized {
                    self.error(self.cover_error, "Expected token ')'.");
                    return None;
                }
                v = self.sym(1);
            }
            228 => {
                let e = self.sym(2);
                v.kind = Kind::Nested;
                v.optional = e.optional;
                v.formals = e.formals;
                self.cover = Cover::Parenthesized;
            }
            229 | 230 => {
                self.cover_error = self.at(2);
                self.cover = Cover::FormalParameterList;
            }
            231 => {
                if !self.sym(2).formals {
                    self.error(self.at(1), "Invalid Arrow parameter list.");
                    return None;
                }
                self.cover_error = self.at(4);
                self.cover = Cover::FormalParameterList;
            }
            232 => v.kind = Kind::Null,
            236 | 237 => v.kind = Kind::StringLiteral,
            238 | 239 => {
                let prefix = if r == 238 {
                    RegExpPrefix::None
                } else {
                    RegExpPrefix::Equal
                };
                if !self.lexer.scan_reg_exp(prefix) {
                    let message = self.lexer.error_message().to_string();
                    self.error(self.lexer.loc(), message);
                    return None;
                }
                self.locs[self.tos].length = self.lexer.token_length() as u32;
                self.loc = self.locs[self.tos];
            }
            55 | 56 | 240..=242 | 253..=255 | 477 | 478 => {
                v.kind = Kind::Pattern;
                v.formals = true;
            }
            287 | 288 | 305 | 306 => {
                let base = self.sym(1);
                let optional = r == 288 || r == 306;
                v.kind = Kind::ArrayMember;
                v.base_qualified = base.qualified;
                v.name_at = base.name_at;
                v.optional = optional || base.optional;
                v.at = self.at(if optional { 3 } else { 2 });
            }
            289 => {
                v.kind = Kind::Field;
                v.qualified = true;
                v.name_at = self.at(3);
            }
            290..=292 | 307 | 308 => {
                let base = self.sym(1);
                v.kind = Kind::Field;
                v.qualified = r != 290 && base.qualified;
                v.name_at = self.at(3);
                v.optional = r == 292 || r == 308 || (r != 290 && base.optional);
            }
            300..=304 => {
                v.kind = Kind::Call;
                v.optional = r == 301 || r == 304 || (r != 302 && self.sym(1).optional);
            }
            378 | 379 | 382 | 383 => {
                v.kind = Kind::AndOr;
                v.at = self.at(2);
            }
            386 | 387 => {
                let (lhs, rhs) = (self.sym(1), self.sym(3));
                if lhs.kind == Kind::AndOr {
                    self.error(lhs.at, "Left-hand side may not contain || or &&");
                    return None;
                }
                if rhs.kind == Kind::AndOr {
                    self.error(rhs.at, "Right-hand side may not contain || or &&");
                    return None;
                }
            }
            398..=401 => {
                let lhs = self.sym(1);
                if lhs.optional {
                    self.error(
                        self.at(1),
                        "Optional chains are not permitted on the left-hand-side in assignments",
                    );
                }
                if r < 400 {
                    v.kind = Kind::Assign;
                    v.formals = matches!(lhs.kind, Kind::Ident | Kind::Pattern);
                }
            }
            416 | 417 => {
                v.kind = Kind::Comma;
                v.formals = self.sym(1).formals && self.sym(3).formals;
            }
            456..=459 => {
                let list = self.sym(2);
                if list.kind == Kind::Annotation {
                    self.error(
                        list.at,
                        "Type annotations are not permitted in variable declarations",
                    );
                    return None;
                }
            }
            461..=464 => v = self.sym(1),
            465..=468 => {
                v = self.sym(1);
                if v.kind != Kind::Annotation {
                    v = self.sym(3);
                }
            }
            469..=472 => {
                let annotation = self.sym(2);
                if annotation.kind == Kind::Annotation {
                    v = annotation;
                }
            }
            500 => {
                let token = self.lookahead_token();
                if token == T_LBRACE {
                    self.push_token(T_FORCE_BLOCK);
                } else if [T_FUNCTION, T_CLASS, T_LET, T_CONST].contains(&token) {
                    self.push_token(T_FORCE_DECLARATION);
                }
            }
            514 | 515 => {
                let annotation = self.sym(3);
                if annotation.kind == Kind::Annotation {
                    self.error(
                        annotation.at,
                        "Type annotations are not permitted in variable declarations",
                    );
                    return None;
                }
            }
            522 => {
                if self.function_nesting == 0 {
                    self.error(
                        self.at(1),
                        "Return statement not allowed outside of Function declaration.",
                    );
                    return None;
                }
            }
            535 => {
                self.error(
                    self.at(3),
                    "FunctionDeclarations are not allowed after a label.",
                );
                return None;
            }
            561 => self.function_nesting += 1,
            562 => self.function_nesting -= 1,
            569 => {
                if self.cover != Cover::FormalParameterList && !self.sym(1).formals {
                    self.error(self.at(1), "Invalid Arrow parameter list.");
                    return None;
                }
            }
            570 => {
                if self.lookahead_token() == T_LBRACE {
                    self.push_token(T_FORCE_BLOCK);
                }
            }
            576 => self.lexer.enter_generator_body(),
            577 => {
                self.function_nesting -= 1;
                self.lexer.leave_generator_body();
            }
            596 => {
                self.class_nesting += 1;
                if self.class_nesting == 1 {
                    self.lexer.set_static_is_keyword(true);
                }
            }
            597 => {
                self.class_nesting -= 1;
                if self.class_nesting == 0 {
                    self.lexer.set_static_is_keyword(false);
                }
            }
            638 => {
                let token = self.lookahead_token();
                if token == T_FUNCTION || token == T_CLASS {
                    self.push_token(T_FORCE_DECLARATION);
                }
            }
            _ => {}
        }
        Some(v)
    }
}
