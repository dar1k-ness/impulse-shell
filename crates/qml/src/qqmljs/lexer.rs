//! Qt 6.12's QML and JavaScript lexer (`QQmlJS::Lexer`, qqmljslexer.cpp), in QML mode and
//! over whole documents. The text is UTF-16 as in Qt: offsets, lengths and columns count
//! UTF-16 code units.

use super::grammar::*;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Parens {
    Ignore,
    Count,
    Balanced,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Quote {
    Single,
    Double,
    TemplateHead,
    TemplateContinuation,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RegExpPrefix {
    None,
    Equal,
}

/// Where a token is: `offset` and `length` in UTF-16 units, `line` and `column` from 1.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Loc {
    pub offset: u32,
    pub length: u32,
    pub line: u32,
    pub column: u32,
}

pub struct Lexer {
    code: Vec<u16>,
    end: usize,
    /// The unit after `cur` (`_codePtr`).
    pos: usize,
    skip_linefeed: bool,
    line: u32,
    col: u32,
    token_start: usize,
    token_length: usize,
    token_line: u32,
    token_column: u32,
    error_message: String,
    static_is_keyword: bool,

    cur: u16,
    token_value: f64,
    parens: Parens,
    parens_count: i32,
    outer_template_braces: Vec<i32>,
    braces_count: i32,
    stack_token: i32,
    token_kind: i32,
    saw_import: bool,
    prohibit_automatic_semicolon: bool,
    restricted_keyword: bool,
    terminator: bool,
    follows_closing_brace: bool,
    delimited: bool,
    generator_level: i32,
}

const NL: u16 = b'\n' as u16;

fn is_binop(tok: i32) -> bool {
    matches!(
        tok,
        T_AND
            | T_AND_AND
            | T_AND_EQ
            | T_DIVIDE_
            | T_DIVIDE_EQ
            | T_EQ
            | T_EQ_EQ
            | T_EQ_EQ_EQ
            | T_GE
            | T_GT
            | T_GT_GT
            | T_GT_GT_EQ
            | T_GT_GT_GT
            | T_GT_GT_GT_EQ
            | T_LE
            | T_LT
            | T_LT_LT
            | T_LT_LT_EQ
            | T_MINUS
            | T_MINUS_EQ
            | T_NOT_EQ
            | T_NOT_EQ_EQ
            | T_OR
            | T_OR_EQ
            | T_OR_OR
            | T_PLUS
            | T_PLUS_EQ
            | T_REMAINDER
            | T_REMAINDER_EQ
            | T_RETURN
            | T_STAR
            | T_STAR_EQ
            | T_XOR
            | T_XOR_EQ
    )
}

fn hex_digit(c: u16) -> Option<u32> {
    char::from_u32(c as u32).and_then(|c| c.to_digit(16))
}

fn is_digit(c: u16) -> bool {
    (b'0' as u16..=b'9' as u16).contains(&c)
}

fn ch(c: u32) -> char {
    char::from_u32(c).unwrap_or('\0')
}

/// `QChar::isSpace`.
fn is_space(c: u16) -> bool {
    ch(c as u32).is_whitespace()
}

/// `isIdentifierStart`: a letter of any script (categories L and Nl), `_` or `$`. Unicode's
/// ID_Start has a few more characters that are not letters.
fn is_identifier_start(c: u32) -> bool {
    let c = ch(c);
    let not_a_letter = matches!(
        c,
        '\u{1885}' | '\u{1886}' | '\u{2118}' | '\u{212e}' | '\u{309b}' | '\u{309c}'
    );
    c == '_' || c == '$' || (unicode_id_start::is_id_start(c) && !not_a_letter)
}

/// `isIdentifierPart`: the above, combining marks (Mn, Mc), decimal digits (Nd), connector
/// punctuation (Pc), ZWNJ and ZWJ. Unicode's ID_Continue has a few more characters that are
/// none of these.
fn is_identifier_part(c: u32) -> bool {
    let c = ch(c);
    let other = matches!(
        c,
        '\u{b7}' | '\u{387}' | '\u{1369}'..='\u{1371}' | '\u{19da}' | '\u{30fb}' | '\u{ff65}'
    );
    matches!(c, '$' | '\u{200c}' | '\u{200d}') || (unicode_id_start::is_id_continue(c) && !other)
}

fn is_high_surrogate(c: u32) -> bool {
    (0xd800..0xdc00).contains(&c)
}

fn is_low_surrogate(c: u32) -> bool {
    (0xdc00..0xe000).contains(&c)
}

fn surrogate_to_ucs4(high: u32, low: u32) -> u32 {
    (high << 10) + low - 0x35f_dc00
}

/// The token of the word `s` (qqmljskeywords_p.h, QML mode).
fn classify(s: &[u16], yield_is_keyword: bool) -> i32 {
    if s.len() < 2 || s.len() > 12 {
        return T_IDENTIFIER;
    }
    let mut buf = [0u8; 12];
    for (b, &u) in buf.iter_mut().zip(s) {
        if u >= 128 {
            return T_IDENTIFIER;
        }
        *b = u as u8;
    }
    match &buf[..s.len()] {
        b"as" => T_AS,
        b"do" => T_DO,
        b"if" => T_IF,
        b"in" => T_IN,
        b"on" => T_ON,
        b"of" => T_OF,
        b"for" => T_FOR,
        b"get" => T_GET,
        b"let" => T_LET,
        b"new" => T_NEW,
        b"set" => T_SET,
        b"try" => T_TRY,
        b"var" => T_VAR,
        b"case" => T_CASE,
        b"else" => T_ELSE,
        b"enum" => T_ENUM,
        b"from" => T_FROM,
        b"null" => T_NULL,
        b"this" => T_THIS,
        b"true" => T_TRUE,
        b"void" => T_VOID,
        b"with" => T_WITH,
        b"break" => T_BREAK,
        b"catch" => T_CATCH,
        b"class" => T_CLASS,
        b"const" => T_CONST,
        b"false" => T_FALSE,
        b"final" => T_FINAL,
        b"super" => T_SUPER,
        b"throw" => T_THROW,
        b"while" => T_WHILE,
        b"yield" if yield_is_keyword => T_YIELD,
        b"delete" => T_DELETE,
        b"export" => T_EXPORT,
        b"import" => T_IMPORT,
        b"native" => T_NATIVE,
        b"public" => T_PUBLIC,
        b"pragma" => T_PRAGMA,
        b"return" => T_RETURN,
        b"signal" => T_SIGNAL,
        b"static" => T_STATIC,
        b"switch" => T_SWITCH,
        b"throws" => T_THROWS,
        b"typeof" => T_TYPEOF,
        b"default" => T_DEFAULT,
        b"extends" => T_EXTENDS,
        b"finally" => T_FINALLY,
        b"package" => T_PACKAGE,
        b"private" => T_PRIVATE,
        b"virtual" => T_VIRTUAL,
        b"abstract" => T_ABSTRACT,
        b"continue" => T_CONTINUE,
        b"debugger" => T_DEBUGGER,
        b"function" => T_FUNCTION,
        b"override" => T_OVERRIDE,
        b"property" => T_PROPERTY,
        b"readonly" => T_READONLY,
        b"required" => T_REQUIRED,
        b"volatile" => T_VOLATILE,
        b"interface" => T_INTERFACE,
        b"protected" => T_PROTECTED,
        b"transient" => T_TRANSIENT,
        b"component" => T_COMPONENT,
        b"implements" => T_IMPLEMENTS,
        b"instanceof" => T_INSTANCEOF,
        b"synchronized" => T_SYNCHRONIZED,
        _ => T_IDENTIFIER,
    }
}

impl Lexer {
    pub fn new(code: Vec<u16>) -> Lexer {
        Lexer {
            end: code.len(),
            code,
            pos: 0,
            skip_linefeed: false,
            line: 1,
            col: 0,
            token_start: 0,
            token_length: 0,
            token_line: 1,
            token_column: 0,
            error_message: String::new(),
            static_is_keyword: false,
            cur: NL,
            token_value: 0.0,
            parens: Parens::Ignore,
            parens_count: 0,
            outer_template_braces: Vec::new(),
            braces_count: -1,
            stack_token: -1,
            token_kind: 0,
            saw_import: false,
            prohibit_automatic_semicolon: false,
            restricted_keyword: false,
            terminator: false,
            follows_closing_brace: false,
            delimited: true,
            generator_level: 0,
        }
    }

    pub fn code(&self) -> &[u16] {
        &self.code
    }

    /// Where the last token is.
    pub fn loc(&self) -> Loc {
        Loc {
            offset: self.token_start as u32,
            length: self.token_length as u32,
            line: self.token_line,
            column: self.token_column,
        }
    }

    pub fn token_value(&self) -> f64 {
        self.token_value
    }

    pub fn token_length(&self) -> usize {
        self.token_length
    }

    pub fn error_message(&self) -> &str {
        &self.error_message
    }

    pub fn set_static_is_keyword(&mut self, b: bool) {
        self.static_is_keyword = b;
    }

    pub fn enter_generator_body(&mut self) {
        self.generator_level += 1;
    }

    pub fn leave_generator_body(&mut self) {
        self.generator_level -= 1;
    }

    pub fn can_insert_automatic_semicolon(&self, token: i32) -> bool {
        token == T_RBRACE || token == EOF_SYMBOL || self.terminator || self.follows_closing_brace
    }

    fn at(&self, i: usize) -> u16 {
        if i < self.end { self.code[i] } else { 0 }
    }

    fn is_line_terminator(&self) -> bool {
        matches!(self.cur, 0x0a | 0x0d | 0x2028 | 0x2029)
    }

    fn scan_char(&mut self) {
        if self.skip_linefeed {
            self.pos += 1;
            self.skip_linefeed = false;
        }
        self.cur = self.at(self.pos);
        self.pos += 1;
        self.col = self.col.wrapping_add(1);
        if self.is_line_terminator() {
            if self.cur == 0x0d {
                if self.pos < self.end && self.code[self.pos] == NL {
                    self.skip_linefeed = true;
                }
                self.cur = NL;
            }
            self.line += 1;
            self.col = 0;
        }
    }

    fn is(&self, c: char) -> bool {
        self.cur == c as u16
    }

    fn error(&mut self, msg: &str) -> i32 {
        self.error_message = msg.to_string();
        T_ERROR
    }

    pub fn lex(&mut self) -> i32 {
        let previous = self.token_kind;
        loop {
            let kind = self.scan_token();
            self.token_length = (self.pos - self.token_start).saturating_sub(1);
            self.token_kind = kind;
            self.delimited = false;
            self.restricted_keyword = false;
            self.follows_closing_brace = previous == T_RBRACE;
            match kind {
                T_LBRACE | T_SEMICOLON | T_QUESTION | T_COLON | T_TILDE => {
                    if kind == T_LBRACE && self.braces_count > 0 {
                        self.braces_count += 1;
                    }
                    if matches!(kind, T_LBRACE | T_SEMICOLON) {
                        self.saw_import = false;
                    }
                    self.delimited = true;
                }
                T_IMPORT => {
                    self.saw_import = true;
                }
                T_IF | T_FOR | T_WHILE | T_WITH => {
                    self.parens = Parens::Count;
                    self.parens_count = 0;
                }
                T_ELSE | T_DO => self.parens = Parens::Balanced,
                T_CONTINUE | T_BREAK | T_RETURN | T_YIELD | T_THROW => {
                    self.restricted_keyword = true;
                }
                T_RBRACE => {
                    if self.braces_count > 0 {
                        self.braces_count -= 1;
                    }
                    if self.braces_count == 0 {
                        continue;
                    }
                }
                _ => {
                    if matches!(kind, T_AUTOMATIC_SEMICOLON | T_AS) {
                        self.saw_import = false;
                    }
                    if is_binop(kind) {
                        self.delimited = true;
                    }
                }
            }
            match self.parens {
                Parens::Ignore => {}
                Parens::Count => {
                    if kind == T_RPAREN {
                        self.parens_count -= 1;
                        if self.parens_count == 0 {
                            self.parens = Parens::Balanced;
                        }
                    } else if kind == T_LPAREN {
                        self.parens_count += 1;
                    }
                }
                Parens::Balanced => {
                    if kind != T_DO && kind != T_ELSE {
                        self.parens = Parens::Ignore;
                    }
                }
            }
            return kind;
        }
    }

    fn decode_unicode_escape(&mut self) -> Option<u32> {
        self.scan_char();
        if self.pos + 3 <= self.end && hex_digit(self.cur).is_some() {
            let mut cp = 0;
            for _ in 0..4 {
                let Some(d) = hex_digit(self.cur) else {
                    self.error("Illegal unicode escape sequence");
                    return None;
                };
                cp = cp * 16 + d;
                self.scan_char();
            }
            return Some(cp);
        } else if self.pos < self.end && self.is('{') {
            self.scan_char();
            let mut cp = 0u32;
            let mut ok = hex_digit(self.cur).is_some();
            while ok && self.pos <= self.end {
                let Some(d) = hex_digit(self.cur) else { break };
                cp = cp * 16 + d;
                if cp > 0x10ffff {
                    ok = false;
                    break;
                }
                self.scan_char();
            }
            if ok && self.is('}') {
                self.scan_char();
                return Some(cp);
            }
        }
        self.error("Illegal unicode escape sequence");
        None
    }

    fn sync_prohibit_automatic_semicolon(&mut self) {
        if self.parens == Parens::Balanced {
            // After `if (foo)` a line break is no empty statement (ECMA-262 7.9.1).
            self.prohibit_automatic_semicolon = true;
            self.parens = Parens::Ignore;
        } else {
            self.prohibit_automatic_semicolon = false;
        }
    }

    fn skip_line(&mut self) {
        while self.pos <= self.end && !self.is_line_terminator() {
            self.scan_char();
        }
    }

    fn scan_token(&mut self) -> i32 {
        if self.stack_token != -1 {
            return std::mem::replace(&mut self.stack_token, -1);
        }
        if self.braces_count == 0 {
            // Inside a template string.
            return self.scan_string(Quote::TemplateContinuation);
        }
        self.terminator = false;
        'again: loop {
            while is_space(self.cur) {
                if self.is_line_terminator() {
                    if self.restricted_keyword {
                        // Automatic semicolon insertion.
                        self.token_line = self.line;
                        self.token_column = self.col;
                        self.token_start = self.pos - 1;
                        return T_SEMICOLON;
                    }
                    self.terminator = true;
                    self.sync_prohibit_automatic_semicolon();
                }
                self.scan_char();
            }
            self.token_start = self.pos - 1;
            self.token_line = self.line;
            self.token_column = self.col;
            if self.pos > self.end {
                return EOF_SYMBOL;
            }
            let c = self.cur;
            self.scan_char();
            let two = |l: &mut Lexer, next: char, yes: i32| {
                if l.is(next) {
                    l.scan_char();
                    Some(yes)
                } else {
                    None
                }
            };
            return match ch(c as u32) {
                '~' => T_TILDE,
                '}' => T_RBRACE,
                '|' => two(self, '|', T_OR_OR)
                    .or_else(|| two(self, '=', T_OR_EQ))
                    .unwrap_or(T_OR),
                '{' => T_LBRACE,
                '^' => two(self, '=', T_XOR_EQ).unwrap_or(T_XOR),
                ']' => T_RBRACKET,
                '[' => T_LBRACKET,
                '?' => {
                    if self.is('?') {
                        self.scan_char();
                        T_QUESTION_QUESTION
                    } else if self.is('.') && !is_digit(self.at(self.pos)) {
                        self.scan_char();
                        T_QUESTION_DOT
                    } else {
                        T_QUESTION
                    }
                }
                '>' => {
                    if self.is('>') {
                        self.scan_char();
                        if self.is('>') {
                            self.scan_char();
                            two(self, '=', T_GT_GT_GT_EQ).unwrap_or(T_GT_GT_GT)
                        } else {
                            two(self, '=', T_GT_GT_EQ).unwrap_or(T_GT_GT)
                        }
                    } else {
                        two(self, '=', T_GE).unwrap_or(T_GT)
                    }
                }
                '=' => {
                    if self.is('=') {
                        self.scan_char();
                        two(self, '=', T_EQ_EQ_EQ).unwrap_or(T_EQ_EQ)
                    } else {
                        two(self, '>', T_ARROW).unwrap_or(T_EQ)
                    }
                }
                '<' => {
                    if self.is('=') {
                        self.scan_char();
                        T_LE
                    } else if self.is('<') {
                        self.scan_char();
                        two(self, '=', T_LT_LT_EQ).unwrap_or(T_LT_LT)
                    } else {
                        T_LT
                    }
                }
                ';' => T_SEMICOLON,
                ':' => T_COLON,
                '/' => {
                    if self.is('*') {
                        self.scan_char();
                        while self.pos <= self.end {
                            if self.is('*') {
                                self.scan_char();
                                if self.is('/') {
                                    self.scan_char();
                                    continue 'again;
                                }
                            } else {
                                self.scan_char();
                            }
                        }
                        continue 'again;
                    } else if self.is('/') {
                        self.skip_line();
                        continue 'again;
                    }
                    two(self, '=', T_DIVIDE_EQ).unwrap_or(T_DIVIDE_)
                }
                '.' => {
                    if self.saw_import {
                        T_DOT
                    } else if is_digit(self.cur) {
                        self.scan_number(c)
                    } else if self.is('.') {
                        self.scan_char();
                        if self.is('.') {
                            self.scan_char();
                            T_ELLIPSIS
                        } else {
                            self.error("Unexpected token '.'")
                        }
                    } else {
                        T_DOT
                    }
                }
                '-' | '+' => {
                    let (eq, twice, one) = if c == b'-' as u16 {
                        (T_MINUS_EQ, T_MINUS_MINUS, T_MINUS)
                    } else {
                        (T_PLUS_EQ, T_PLUS_PLUS, T_PLUS)
                    };
                    if self.is('=') {
                        self.scan_char();
                        eq
                    } else if self.cur == c {
                        self.scan_char();
                        if self.terminator
                            && !self.delimited
                            && !self.prohibit_automatic_semicolon
                            && self.token_kind != T_LPAREN
                        {
                            self.stack_token = twice;
                            T_SEMICOLON
                        } else {
                            twice
                        }
                    } else {
                        one
                    }
                }
                ',' => T_COMMA,
                '*' => {
                    if self.is('=') {
                        self.scan_char();
                        T_STAR_EQ
                    } else if self.is('*') {
                        self.scan_char();
                        two(self, '=', T_STAR_STAR_EQ).unwrap_or(T_STAR_STAR)
                    } else {
                        T_STAR
                    }
                }
                ')' => T_RPAREN,
                '(' => T_LPAREN,
                '@' => T_AT,
                '&' => two(self, '=', T_AND_EQ)
                    .or_else(|| two(self, '&', T_AND_AND))
                    .unwrap_or(T_AND),
                '%' => two(self, '=', T_REMAINDER_EQ).unwrap_or(T_REMAINDER),
                '!' => {
                    if self.is('=') {
                        self.scan_char();
                        two(self, '=', T_NOT_EQ_EQ).unwrap_or(T_NOT_EQ)
                    } else {
                        T_NOT
                    }
                }
                '`' => {
                    self.outer_template_braces.push(self.braces_count);
                    self.scan_string(Quote::TemplateHead)
                }
                '\'' => self.scan_string(Quote::Single),
                '"' => self.scan_string(Quote::Double),
                '0'..='9' => {
                    if self.saw_import {
                        self.scan_version_number(c)
                    } else {
                        self.scan_number(c)
                    }
                }
                '#' if self.line == 1 && self.col == 2 => {
                    // `#!` line.
                    self.skip_line();
                    continue 'again;
                }
                _ => self.scan_identifier(c),
            };
        }
    }

    fn scan_identifier(&mut self, first: u16) -> i32 {
        let mut c = first as u32;
        let mut escapes = false;
        if is_high_surrogate(c) && is_low_surrogate(self.cur as u32) {
            c = surrogate_to_ucs4(c, self.cur as u32);
            self.scan_char();
        } else if c == '\\' as u32 && self.is('u') {
            escapes = true;
            match self.decode_unicode_escape() {
                Some(cp) => c = cp,
                None => return T_ERROR,
            }
        }
        if !is_identifier_start(c) {
            return T_ERROR;
        }
        while self.pos <= self.end {
            c = self.cur as u32;
            if is_high_surrogate(c) && is_low_surrogate(self.at(self.pos) as u32) {
                self.scan_char();
                c = surrogate_to_ucs4(c, self.cur as u32);
            } else if self.is('\\') && self.at(self.pos) == b'u' as u16 {
                escapes = true;
                self.scan_char();
                match self.decode_unicode_escape() {
                    Some(cp) if is_identifier_part(cp) => continue,
                    Some(_) => break,
                    None => return T_ERROR,
                }
            }
            if !is_identifier_part(c) {
                break;
            }
            self.scan_char();
        }
        if escapes {
            return T_IDENTIFIER;
        }
        classify(
            &self.code[self.token_start..self.pos - 1],
            self.generator_level != 0,
        )
    }

    fn scan_string(&mut self, mode: Quote) -> i32 {
        let quote = match mode {
            Quote::Single => b'\'',
            Quote::Double => b'"',
            Quote::TemplateHead | Quote::TemplateContinuation => b'`',
        } as u16;
        let template = quote == b'`' as u16;
        let closed = |l: &mut Lexer| {
            l.scan_char();
            if template {
                l.braces_count = l.outer_template_braces.pop().unwrap_or(-1);
            }
            match mode {
                Quote::TemplateHead => T_NO_SUBSTITUTION_TEMPLATE,
                Quote::TemplateContinuation => T_TEMPLATE_TAIL,
                _ => T_STRING_LITERAL,
            }
        };
        // A `\r` just read is read again below.
        self.skip_linefeed = false;
        let mut first = true;
        while self.pos <= self.end {
            if self.is_line_terminator() {
                if first {
                    self.line -= 1;
                }
                break;
            } else if self.is('\\') || (self.is('$') && template) {
                break;
            } else if self.cur == quote {
                return closed(self);
            }
            self.cur = self.at(self.pos);
            self.pos += 1;
            self.col = self.col.wrapping_add(1);
            first = false;
        }
        // Back by one unit, to scan it as any other.
        self.pos -= 1;
        self.col = self.col.wrapping_sub(1);
        self.scan_char();
        while self.pos <= self.end {
            if self.cur == quote {
                return closed(self);
            } else if template && self.is('$') && self.at(self.pos) == b'{' as u16 {
                self.scan_char();
                self.scan_char();
                self.braces_count = 1;
                return if mode == Quote::TemplateHead {
                    T_TEMPLATE_HEAD
                } else {
                    T_TEMPLATE_MIDDLE
                };
            } else if self.is('\\') {
                self.scan_char();
                if self.pos > self.end {
                    return self.error("End of file reached at escape sequence");
                }
                match ch(self.cur as u32) {
                    'u' => {
                        if self.decode_unicode_escape().is_none() {
                            return T_ERROR;
                        }
                    }
                    'x' => {
                        if hex_digit(self.at(self.pos)).is_some()
                            && hex_digit(self.at(self.pos + 1)).is_some()
                        {
                            self.scan_char();
                            self.scan_char();
                            self.scan_char();
                        } else {
                            return self.error("Illegal hexadecimal escape sequence");
                        }
                    }
                    '0' if !is_digit(self.at(self.pos)) => self.scan_char(),
                    '0'..='9' => return self.error("Octal escape sequences are not allowed"),
                    _ => self.scan_char(),
                }
            } else {
                self.scan_char();
            }
        }
        self.error("Unclosed string at end of line")
    }

    fn numeric_separator(&mut self, next_is_valid: impl Fn(u16) -> bool) -> bool {
        if self.is('_') {
            let next = self.at(self.pos);
            if next == b'_' as u16 {
                self.error("There can be at most one numeric separator between digits");
                return false;
            }
            if !next_is_valid(next) {
                self.error("A trailing numeric separator is not allowed in numeric literals");
                return false;
            }
            self.scan_char();
        }
        true
    }

    fn scan_radix(&mut self, radix: u32, name: &str) -> i32 {
        let digit = move |c: u16| ch(c as u32).to_digit(radix);
        let prefix = ch(self.cur as u32);
        self.scan_char();
        if digit(self.cur).is_none() {
            return self.error(&format!(
                "At least one {name} digit is required after '0{prefix}'"
            ));
        }
        let mut d = 0.0;
        while let Some(n) = digit(self.cur) {
            d = d * radix as f64 + n as f64;
            self.scan_char();
            if !self.numeric_separator(|c| digit(c).is_some()) {
                return T_ERROR;
            }
        }
        self.token_value = d;
        T_NUMERIC_LITERAL
    }

    fn scan_number(&mut self, first: u16) -> i32 {
        if first == b'0' as u16 {
            match ch(self.cur as u32) {
                'x' | 'X' => return self.scan_radix(16, "hexadecimal"),
                'o' | 'O' => return self.scan_radix(8, "octal"),
                'b' | 'B' => return self.scan_radix(2, "binary"),
                _ => {}
            }
        }
        let mut chars = String::new();
        chars.push(ch(first as u32));
        macro_rules! digits {
            () => {
                while is_digit(self.cur) {
                    chars.push(ch(self.cur as u32));
                    self.scan_char();
                    if !self.numeric_separator(is_digit) {
                        return T_ERROR;
                    }
                }
            };
        }
        if first != b'.' as u16 {
            if !self.numeric_separator(is_digit) {
                return T_ERROR;
            }
            digits!();
            if self.is('.') {
                chars.push('.');
                self.scan_char();
            }
        }
        digits!();
        if self.is('e') || self.is('E') {
            let a = self.at(self.pos);
            if is_digit(a)
                || ((a == b'+' as u16 || a == b'-' as u16) && is_digit(self.at(self.pos + 1)))
            {
                chars.push('e');
                self.scan_char();
                if self.is('+') || self.is('-') {
                    chars.push(ch(self.cur as u32));
                    self.scan_char();
                }
                digits!();
            }
        }
        match chars.parse() {
            Ok(v) => {
                self.token_value = v;
                T_NUMERIC_LITERAL
            }
            Err(_) => self.error("Illegal syntax for exponential number"),
        }
    }

    fn scan_version_number(&mut self, first: u16) -> i32 {
        let mut acc = 0f64;
        if first != b'0' as u16 {
            acc = (first - b'0' as u16) as f64;
            while is_digit(self.cur) {
                acc = acc * 10.0 + (self.cur - b'0' as u16) as f64;
                self.scan_char();
            }
        }
        self.token_value = acc;
        T_VERSION_NUMBER
    }

    /// The rest of a regular expression literal, after its `/` or `/=`. False with
    /// [`Lexer::error_message`] set when it does not end.
    pub fn scan_reg_exp(&mut self, _prefix: RegExpPrefix) -> bool {
        let mut flags = 0u8;
        loop {
            match ch(self.cur as u32) {
                '/' => {
                    self.scan_char();
                    loop {
                        let c = ch(self.cur as u32);
                        let letter = c.is_ascii_alphabetic()
                            || c == '$'
                            || c == '_'
                            || (!c.is_ascii() && c.is_alphanumeric());
                        if !letter {
                            break;
                        }
                        let flag = match c {
                            'g' => 1,
                            'i' => 2,
                            'm' => 4,
                            'u' => 8,
                            'y' => 16,
                            _ => 0,
                        };
                        if flag == 0 || flags & flag != 0 {
                            self.error_message = format!("Invalid regular expression flag '{c}'");
                            return false;
                        }
                        flags |= flag;
                        self.scan_char();
                    }
                    self.token_length = self.pos - self.token_start - 1;
                    return true;
                }
                '\\' => {
                    self.scan_char();
                    if self.pos > self.end || self.is_line_terminator() {
                        self.error("Unterminated regular expression backslash sequence");
                        return false;
                    }
                    self.scan_char();
                }
                '[' => {
                    self.scan_char();
                    while self.pos <= self.end && !self.is_line_terminator() {
                        if self.is(']') {
                            break;
                        } else if self.is('\\') {
                            self.scan_char();
                            if self.pos > self.end || self.is_line_terminator() {
                                self.error("Unterminated regular expression backslash sequence");
                                return false;
                            }
                            self.scan_char();
                        } else {
                            self.scan_char();
                        }
                    }
                    if !self.is(']') {
                        self.error("Unterminated regular expression class");
                        return false;
                    }
                    self.scan_char();
                }
                _ => {
                    if self.pos > self.end || self.is_line_terminator() {
                        self.error("Unterminated regular expression literal");
                        return false;
                    }
                    self.scan_char();
                }
            }
        }
    }
}
