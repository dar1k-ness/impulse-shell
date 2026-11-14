//! Owned, lifetime-free JS IR lowered from the oxc AST. Input to the binding evaluator / Rust codegen.
use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_parser::Parser;
use oxc_span::SourceType;
use std::rc::Rc;

pub use crate::jsrt::FnKind;

#[derive(Debug, Clone)]
pub enum Key {
    Name(std::rc::Rc<str>),
    Computed(Expr),
}

/// How an object-literal entry defines its property (`k: v`, `get k() {}`, `set k(v) {}`),
/// or `__proto__: v`, which sets the literal's [[Prototype]] (ECMA-262 Annex B §B.3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropKind {
    Init,
    Get,
    Set,
    Proto,
}

#[derive(Debug, Clone)]
pub enum Pat {
    Id(String),
    /// A function-frame slot resolved at lowering: (function frames to skip, slot, name).
    Local(u32, u32, String),
    /// A name as [`Expr::Dynamic`], as an assignment target: the name, the number of `with`
    /// objects to ask, and the target the code shows for it (a name, a slot, either as
    /// [`Pat::Const`], or a generator's scope-object member).
    Dynamic(String, u32, Box<Pat>),
    Object(Vec<(Key, Pat)>, Option<Box<Pat>>),
    Array(Vec<Option<Pat>>, Option<Box<Pat>>),
    Default(Box<Pat>, Box<Expr>),
    Member(Box<Expr>),
    /// An assignment target naming an immutable binding (`const`, a class's own name in its
    /// body, a named function expression's own name in its body), an `Id` or `Local`: it reads
    /// as that binding, and writing it throws `TypeError` when the flag is set (`ReferenceError`
    /// while the binding is uninitialized), else does nothing (ECMA-262 §9.1.1.1.5
    /// SetMutableBinding: a sloppy write to the non-strict immutable binding of a function
    /// expression's name is ignored).
    Const(Box<Pat>, bool),
    /// InitializeReferencedBinding (ECMA-262 §9.1.1.1.4) of a generator scope-object member
    /// (`ir::genlower`): writes it without the TDZ and `const` checks a plain write makes.
    Init(Box<Expr>),
}

#[derive(Debug, Clone)]
pub struct Func {
    pub name: Option<String>,
    /// The name an anonymous function definition gets from where it is defined (ECMA-262
    /// NamedEvaluation): `f` in `var f = function () {}`. Unlike `name` it binds nothing.
    pub inferred: Option<String>,
    pub params: Vec<Pat>,
    pub rest: Option<Pat>,
    pub body: Vec<Stmt>,
    pub kind: FnKind,
    /// A generator or async function body; `None` for ordinary function code.
    pub coroutine: Option<Coroutine>,
    /// Strict mode code (ECMA-262 §11.2.2).
    pub strict: bool,
    /// The body (or a nested function) names `arguments`.
    pub uses_arguments: bool,
    /// Its code, nested functions included, has a call `eval(...)`.
    pub direct_eval: bool,
    /// The body (or a nested arrow) has a `super` property reference: a method defined in an
    /// object literal then gets the literal as its [[HomeObject]] (ECMA-262 §10.2.7).
    pub uses_super: bool,
    /// `var` names of the body, collected on first call.
    pub hoisted: std::cell::OnceCell<Vec<String>>,
    /// Slots of the frame a call makes (see [`resolve`]).
    pub nslots: u32,
    /// A declaration directly in a function body binds its name in this slot of that frame.
    pub slot: Option<u32>,
    /// A plain function whose body is `return e`, `e` reading only `this` and literals
    /// (see [`this_only`]): a getter call can evaluate it without a call frame.
    pub this_only: bool,
    /// A named function expression, declared in the frame [`named_func_expr`] wraps it in,
    /// where its name is an immutable binding (ECMA-262 §15.2.5,
    /// InstantiateOrdinaryFunctionExpression).
    pub expression: bool,
    /// With [`Func::param_exprs`] the body's declarations get their own environment
    /// (ECMA-262 §10.2.11 step 28): each body `var` named like a formal, or `arguments` of a
    /// function with an arguments object, starts in this slot as this value, read where the
    /// parameters are bound.
    pub var_init: Vec<(Expr, u32)>,
}

/// The function kinds of ECMA-262 CreateDynamicFunction besides `normal`: their calls return
/// a generator object or a promise instead of running the body to completion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Coroutine {
    /// `function*` (§15.5).
    Generator,
    /// `async function`, async method, async arrow (§15.8, §15.9).
    Async,
    /// `async function*` (§15.6).
    AsyncGenerator,
}

impl Coroutine {
    fn of(is_async: bool, generator: bool) -> Option<Self> {
        match (is_async, generator) {
            (false, false) => None,
            (false, true) => Some(Coroutine::Generator),
            (true, false) => Some(Coroutine::Async),
            (true, true) => Some(Coroutine::AsyncGenerator),
        }
    }

    /// Whether the engines run it: a generator's body is lowered to a resumable state machine
    /// (`genlower`); async functions are still refused.
    pub fn runs(self) -> bool {
        self == Coroutine::Generator
    }

    /// The intrinsic constructor of such functions (%GeneratorFunction% etc.), the name
    /// [`unsupported`] reports.
    pub fn name(self) -> &'static str {
        match self {
            Coroutine::Generator => "GeneratorFunction",
            Coroutine::Async => "AsyncFunction",
            Coroutine::AsyncGenerator => "AsyncGeneratorFunction",
        }
    }
}

impl Expr {
    /// What the code shows `self` to be: the binding of a [`Expr::Dynamic`] name and the
    /// call of a [`Expr::DirectEval`]. Compiled code, which has neither `with` objects
    /// nor an `eval` that runs code, takes these.
    pub fn shown(&self) -> &Expr {
        match self {
            Expr::Dynamic(_, _, x) | Expr::DirectEval(_, x) => x,
            e => e,
        }
    }
}

impl Pat {
    /// [`Expr::shown`] of an assignment target.
    pub fn shown(&self) -> &Pat {
        match self {
            Pat::Dynamic(_, _, p) => p,
            p => p,
        }
    }
}

impl Func {
    /// Whether this is the step function of a lowered generator ([`genlower`]).
    pub fn is_gen_step(&self) -> bool {
        self.kind == FnKind::Arrow
            && matches!(self.params.first(), Some(Pat::Id(n) | Pat::Local(_, _, n)) if n == genlower::CTX)
    }

    /// The function's `name` property.
    pub fn display_name(&self) -> &str {
        self.name
            .as_deref()
            .or(self.inferred.as_deref())
            .unwrap_or("")
    }

    pub fn is_arrow(&self) -> bool {
        self.kind.is_arrow()
    }

    /// Its calls make a mapped arguments object (ECMA-262 §10.2.11 step 22): sloppy code with a
    /// simple parameter list, none of it named `arguments`.
    pub fn mapped_arguments(&self) -> bool {
        self.uses_arguments
            && !self.is_arrow()
            && !self.strict
            && self.rest.is_none()
            && self
                .params
                .iter()
                .all(|p| matches!(p, Pat::Id(n) | Pat::Local(_, _, n) if n != "arguments"))
    }

    /// ContainsExpression of the formals (ECMA-262 §8.4.2): a default value or a computed
    /// key, whose closures must not see the body's declarations (§10.2.11 step 28).
    pub fn param_exprs(&self) -> bool {
        fn has(p: &Pat) -> bool {
            match p {
                Pat::Id(_) | Pat::Local(..) => false,
                Pat::Object(ps, rest) => {
                    ps.iter()
                        .any(|(k, p)| matches!(k, Key::Computed(_)) || has(p))
                        || rest.as_deref().is_some_and(has)
                }
                Pat::Array(ps, rest) => {
                    ps.iter().flatten().any(has) || rest.as_deref().is_some_and(has)
                }
                Pat::Const(p, _) | Pat::Dynamic(_, _, p) => has(p),
                Pat::Default(..) | Pat::Member(_) | Pat::Init(_) => true,
            }
        }
        self.params.iter().chain(&self.rest).any(has)
    }

    /// IsSimpleParameterList (ECMA-262 §15.1.3): plain names only. Any other list may
    /// evaluate parameter expressions, so its bindings start uninitialized
    /// (FunctionDeclarationInstantiation, §10.2.11 step 28): binding a pattern without
    /// expressions cannot observe that.
    pub fn simple_params(&self) -> bool {
        self.rest.is_none()
            && self
                .params
                .iter()
                .all(|p| matches!(p, Pat::Id(_) | Pat::Local(..)))
    }

    /// ExpectedArgumentCount (ECMA-262 15.1.5): the parameters before the first default one;
    /// the rest parameter never counts. This is the function's `length`.
    pub fn length(&self) -> u32 {
        self.params
            .iter()
            .take_while(|p| !matches!(p, Pat::Default(..)))
            .count() as u32
    }
}

// V4 has no BigInt: a script with a `1n` literal fails to compile, so none of it runs.
thread_local!(static BIGINT_SEEN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) });

fn reject_bigint(body: Vec<Stmt>) -> Vec<Stmt> {
    if BIGINT_SEEN.with(|c| c.replace(false)) {
        vec![Stmt::Expr(Expr::Unsupported("BigInt".into()))]
    } else {
        body
    }
}

thread_local!(static ARGUMENTS_SEEN: std::cell::Cell<u32> = const { std::cell::Cell::new(0) });
// Calls `eval(...)` lowered so far: a function with one may run a direct eval, which reads the
// function's `arguments` and may declare a `var` in it.
thread_local!(static EVAL_SEEN: std::cell::Cell<u32> = const { std::cell::Cell::new(0) });
thread_local!(static SUPER_SEEN: std::cell::Cell<u32> = const { std::cell::Cell::new(0) });

fn supers_seen() -> u32 {
    SUPER_SEEN.with(|c| c.get())
}

/// The early-error context of the function code being lowered (ECMA-262 static semantics).
#[derive(Default)]
struct EarlyCx {
    /// The enclosing labels, each with whether it labels an iteration statement.
    labels: Vec<(String, bool)>,
    loops: u32,
    switches: u32,
    strict: bool,
    /// Inside a ClassStaticBlock (and its arrows), where `arguments` is an early error.
    static_block: bool,
}

/// Early errors found while lowering: the script is a SyntaxError (QV4 rejects it at compile
/// time), which `lower`/`lower_program` report instead of a body.
#[derive(Default)]
struct Early {
    cx: EarlyCx,
    error: Option<String>,
}

impl Early {
    fn fail(&mut self, msg: impl Into<String>) {
        self.error.get_or_insert_with(|| msg.into());
    }
}

thread_local!(static EARLY: std::cell::RefCell<Early> = Default::default());

fn early<T>(f: impl FnOnce(&mut Early) -> T) -> T {
    EARLY.with(|e| f(&mut e.borrow_mut()))
}

/// Start lowering a script whose top level is strict code or not.
fn early_reset(strict: bool) {
    early(|e| {
        *e = Early {
            cx: EarlyCx {
                strict,
                ..Default::default()
            },
            error: None,
        }
    });
}

fn early_result() -> Result<(), String> {
    match early(|e| e.error.take()) {
        Some(m) => Err(format!("SyntaxError: {m}")),
        None => Ok(()),
    }
}

fn use_strict(ds: &[Directive]) -> bool {
    ds.iter().any(|d| d.directive.as_str() == "use strict")
}

/// Run `f` with the early-error context `cx`, restoring the current one after.
fn with_cx<T>(cx: impl FnOnce(&EarlyCx) -> EarlyCx, f: impl FnOnce() -> T) -> T {
    let saved = early(|e| {
        let n = cx(&e.cx);
        std::mem::replace(&mut e.cx, n)
    });
    let r = f();
    early(|e| e.cx = saved);
    r
}

/// Function code: no label or loop of the enclosing code is a target inside it, and it is
/// strict when the enclosing code is or its own directives say so.
fn function_code<T>(own_strict: bool, f: impl FnOnce() -> T) -> T {
    with_cx(
        |c| EarlyCx {
            strict: c.strict || own_strict,
            ..Default::default()
        },
        f,
    )
}

/// [`function_code`] of an arrow or a static block, which keep (or start) a static block's
/// ban on `arguments` (ECMA-262 §15.7.1 ContainsArguments).
fn arrow_code<T>(own_strict: bool, static_block: bool, f: impl FnOnce() -> T) -> T {
    with_cx(
        |c| EarlyCx {
            strict: c.strict || own_strict,
            static_block: c.static_block || static_block,
            ..Default::default()
        },
        f,
    )
}

/// A loop body (`switch: false`) or a switch's case block: a target of an unlabelled break,
/// and for a loop of continue.
fn breakable<T>(switch: bool, f: impl FnOnce() -> T) -> T {
    early(|e| match switch {
        true => e.cx.switches += 1,
        false => e.cx.loops += 1,
    });
    let r = f();
    early(|e| match switch {
        true => e.cx.switches -= 1,
        false => e.cx.loops -= 1,
    });
    r
}

fn bound_names<'a>(p: &'a BindingPattern, out: &mut Vec<&'a str>) {
    match p {
        BindingPattern::BindingIdentifier(id) => out.push(id.name.as_str()),
        BindingPattern::ObjectPattern(o) => {
            o.properties
                .iter()
                .for_each(|bp| bound_names(&bp.value, out));
            o.rest.iter().for_each(|r| bound_names(&r.argument, out));
        }
        BindingPattern::ArrayPattern(a) => {
            a.elements
                .iter()
                .flatten()
                .for_each(|e| bound_names(e, out));
            a.rest.iter().for_each(|r| bound_names(&r.argument, out));
        }
        BindingPattern::AssignmentPattern(a) => bound_names(&a.left, out),
    }
}

/// Early errors of a parameter list, checked inside its function code (ECMA-262 15.2.1,
/// 15.3.1, 15.4.1): a duplicate name in strict code, in a non-simple list, or in an arrow's or a
/// method's (`unique`) list; and a `"use strict"` body with a non-simple list.
fn check_params(ps: &FormalParameters, own_strict: bool, unique: bool) {
    let simple = ps.rest.is_none()
        && ps.items.iter().all(|p| {
            p.initializer.is_none() && matches!(p.pattern, BindingPattern::BindingIdentifier(_))
        });
    let mut names = Vec::new();
    ps.items
        .iter()
        .for_each(|p| bound_names(&p.pattern, &mut names));
    ps.rest
        .iter()
        .for_each(|r| bound_names(&r.rest.argument, &mut names));
    early(|e| {
        if own_strict && !simple {
            e.fail("\"use strict\" not allowed in function with non-simple parameters");
        }
        if e.cx.strict || !simple || unique {
            let dup = names
                .iter()
                .enumerate()
                .find(|(i, n)| names[..*i].contains(n));
            if let Some((_, n)) = dup {
                e.fail(format!("Duplicate parameter name '{n}'"));
            }
        }
    });
}

#[derive(Debug, Clone)]
pub struct Method {
    pub key: Key,
    pub func: Func,
    /// "method" | "get" | "set"
    pub kind: &'static str,
    pub is_static: bool,
}

#[derive(Debug, Clone)]
pub struct Class {
    pub name: Option<String>,
    /// See [`Func::inferred`].
    pub inferred: Option<String>,
    pub extends: Option<Expr>,
    pub ctor: Option<Func>,
    pub methods: Vec<Method>,
    pub fields: Vec<(Key, Option<Expr>, bool)>,
    /// For each field, how many methods precede it in the class body, so that property keys
    /// are evaluated in source order (ClassDefinitionEvaluation).
    pub field_after: Vec<u32>,
    /// The ClassStaticBlocks, each with how many fields precede it: run in order with the
    /// static fields, as `(() => { body })()` on the class (ECMA-262 §15.7.11). A class
    /// element the runtime does not implement is an `Expr::Unsupported` here, so the class
    /// definition throws.
    pub blocks: Vec<(u32, Expr)>,
    pub slot: Option<u32>,
    /// Property count of the last instance built, to size the next one.
    pub size_hint: std::cell::Cell<u32>,
}

impl Class {
    /// The class's `name` property.
    pub fn display_name(&self) -> &str {
        self.name
            .as_deref()
            .or(self.inferred.as_deref())
            .unwrap_or("")
    }
}

/// A tagged template's strings: cooked (`None` for an invalid escape, legal only in a tagged
/// template, ECMA-262 §13.2.8.3) and raw (TRV, §12.9.6).
#[derive(Debug)]
pub struct TemplateSite {
    pub cooked: Vec<Option<String>>,
    pub raw: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum Expr {
    Num(f64),
    Str(Rc<str>),
    /// A string literal with an unpaired surrogate ([`crate::value::Value::Str16`]).
    Str16(Rc<[u16]>),
    Bool(bool),
    Null,
    Ident(String),
    Local(u32, u32, String),
    /// A name whose binding is only known when it runs (ECMA-262 §9.1.2.1
    /// GetIdentifierReference): between the use and the binding the code shows for it (the
    /// third field: a frame slot, a name looked up as [`Expr::Ident`], or a generator's
    /// scope-object member) lie `with` objects (the second field counts them) or functions
    /// whose direct `eval` may declare a `var`.
    Dynamic(String, u32, Box<Expr>),
    /// A call `eval(...)` (the second field is that [`Expr::Call`]), a direct eval when the
    /// callee is %eval% (ECMA-262 §13.3.6.1 step 6), with the scopes the code it evaluates
    /// is resolved in.
    DirectEval(Rc<EvalScope>, Box<Expr>),
    This,
    Regex(String, String),
    Template(Vec<String>, Vec<Expr>),
    /// GetTemplateObject (ECMA-262 §13.2.8.4): a tagged template's first argument, one
    /// object per site and realm.
    TemplateObject(Rc<TemplateSite>),
    /// `yield e` / `yield* e` (`true`), inside a generator function body.
    Yield(Option<Box<Expr>>, bool),
    Member(Box<Expr>, crate::runtime::Sym, bool),
    Index(Box<Expr>, Box<Expr>, bool),
    Call(Box<Expr>, Vec<Expr>, bool),
    New(Box<Expr>, Vec<Expr>),
    /// Optional-chain boundary: a short-circuited `?.` inside yields `undefined` here.
    Chain(Box<Expr>),
    Unary(UnOp, Box<Expr>),
    Update(UpdateOp, bool, Box<Pat>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Logical(LogicalOp, Box<Expr>, Box<Expr>),
    Cond(Box<Expr>, Box<Expr>, Box<Expr>),
    Assign(AssignOp, Box<Pat>, Box<Expr>),
    Array(Vec<Expr>),
    Object(Vec<(Key, Expr, PropKind)>),
    Spread(Box<Expr>),
    Hole,
    Seq(Vec<Expr>),
    Func(Rc<Func>),
    Class(Box<Class>),
    /// DefineField (ECMA-262 §7.3.33) of a class field on `this`: the property key, the
    /// initializer value, and whether that value is an anonymous function definition to name
    /// after a computed key (NamedEvaluation). Built by class lowering, never parsed.
    DefineField(Box<Expr>, Box<Expr>, bool),
    Super,
    /// `new.target` (ECMA-262 §13.3.12): the NewTarget of the function environment `this`
    /// resolves to (lexical in arrows), `undefined` in a [[Call]].
    NewTarget,
    Unsupported(String),
}

pub use crate::eval::BinOp;

/// `! - + ~ typeof void delete`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Not,
    Neg,
    Pos,
    BitNot,
    Typeof,
    Void,
    Delete,
}

/// `&& || ??`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogicalOp {
    And,
    Or,
    Nullish,
}

/// `++ --`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateOp {
    Inc,
    Dec,
}

/// `=`, a compound `op=` (by its binary operator), or `&&= ||= ??=`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignOp {
    Assign,
    Bin(BinOp),
    Logical(LogicalOp),
}

fn bin_op(o: BinaryOperator) -> BinOp {
    use BinaryOperator as O;
    match o {
        O::Equality => BinOp::Eq,
        O::Inequality => BinOp::Ne,
        O::StrictEquality => BinOp::StrictEq,
        O::StrictInequality => BinOp::StrictNe,
        O::LessThan => BinOp::Lt,
        O::LessEqualThan => BinOp::Le,
        O::GreaterThan => BinOp::Gt,
        O::GreaterEqualThan => BinOp::Ge,
        O::Addition => BinOp::Add,
        O::Subtraction => BinOp::Sub,
        O::Multiplication => BinOp::Mul,
        O::Division => BinOp::Div,
        O::Remainder => BinOp::Rem,
        O::Exponential => BinOp::Exp,
        O::ShiftLeft => BinOp::Shl,
        O::ShiftRight => BinOp::Shr,
        O::ShiftRightZeroFill => BinOp::UShr,
        O::BitwiseOR => BinOp::BitOr,
        O::BitwiseXOR => BinOp::BitXor,
        O::BitwiseAnd => BinOp::BitAnd,
        O::In => BinOp::In,
        O::Instanceof => BinOp::InstanceOf,
    }
}

fn logical_op(o: LogicalOperator) -> LogicalOp {
    match o {
        LogicalOperator::And => LogicalOp::And,
        LogicalOperator::Or => LogicalOp::Or,
        LogicalOperator::Coalesce => LogicalOp::Nullish,
    }
}

fn un_op(o: UnaryOperator) -> UnOp {
    use UnaryOperator as O;
    match o {
        O::LogicalNot => UnOp::Not,
        O::UnaryNegation => UnOp::Neg,
        O::UnaryPlus => UnOp::Pos,
        O::BitwiseNot => UnOp::BitNot,
        O::Typeof => UnOp::Typeof,
        O::Void => UnOp::Void,
        O::Delete => UnOp::Delete,
    }
}

fn assign_op(o: AssignmentOperator) -> AssignOp {
    if o == AssignmentOperator::Assign {
        AssignOp::Assign
    } else if let Some(b) = o.to_binary_operator() {
        AssignOp::Bin(bin_op(b))
    } else {
        AssignOp::Logical(logical_op(
            o.to_logical_operator().expect("assignment operator"),
        ))
    }
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Expr(Expr),
    Var(&'static str, Vec<(Pat, Option<Expr>)>),
    Func(Rc<Func>),
    Return(Option<Expr>),
    If(Expr, Box<Stmt>, Option<Box<Stmt>>),
    Block(Vec<Stmt>),
    For(Option<Box<Stmt>>, Option<Expr>, Option<Expr>, Box<Stmt>),
    /// `for (k p of e)`: `k` is the head's declaration kind (`var`/`let`/`const`), `None` for
    /// an assignment target.
    ForOf(Option<&'static str>, Pat, Expr, Box<Stmt>),
    ForIn(Option<&'static str>, Pat, Expr, Box<Stmt>),
    While(Expr, Box<Stmt>),
    DoWhile(Box<Stmt>, Expr),
    Switch(Expr, Vec<(Option<Expr>, Vec<Stmt>)>),
    Break(Option<String>),
    Continue(Option<String>),
    Labeled(String, Box<Stmt>),
    /// `with (object) body` (ECMA-262 §14.11): `body` runs in an object environment.
    With(Expr, Box<Stmt>),
    Class(Box<Class>),
    Throw(Expr),
    Try(
        Vec<Stmt>,
        Option<(Option<Pat>, Vec<Stmt>)>,
        Option<Vec<Stmt>>,
    ),
    Empty,
    Unsupported(String),
}

mod genlower;

/// A string literal's value: oxc's `JSStr` keeps every UTF-16 code unit (WTF-8), so one with
/// an unpaired surrogate becomes [`Expr::Str16`].
macro_rules! str_lit {
    ($s:expr) => {{
        let s = &$s;
        match s.as_str() {
            Some(t) => Expr::Str(t.into()),
            None => Expr::Str16(s.encode_utf16().collect()),
        }
    }};
}

/// The string value of an oxc `JSStr` (WTF-8 code units) as a runtime UTF-8 string: a
/// surrogate pair is its code point, a lone surrogate U+FFFD (as for computed strings).
macro_rules! js_text {
    ($s:expr) => {{
        let s = $s;
        match s.as_str() {
            Some(t) => t.to_owned(),
            None => s
                .chars()
                .map(|c| c.to_char().unwrap_or('\u{FFFD}'))
                .collect(),
        }
    }};
}

fn bx<T>(t: T) -> Box<T> {
    Box::new(t)
}

fn key(k: &PropertyKey, computed: bool) -> Key {
    match k {
        PropertyKey::StaticIdentifier(id) => Key::Name(id.name.as_str().into()),
        _ => match k.as_expression() {
            Some(Expression::StringLiteral(s)) if !computed => Key::Name(js_text!(s.value).into()),
            Some(Expression::NumericLiteral(n)) if !computed => {
                Key::Name(crate::value::num_to_string(number(n)).into())
            }
            Some(e) => Key::Computed(expr(e)),
            None => Key::Name("#private".into()),
        },
    }
}

fn pat(p: &BindingPattern) -> Pat {
    match p {
        BindingPattern::BindingIdentifier(id) => Pat::Id(id.name.to_string()),
        BindingPattern::ObjectPattern(o) => Pat::Object(
            o.properties
                .iter()
                .map(|bp| (key(&bp.key, bp.computed), pat(&bp.value)))
                .collect(),
            o.rest.as_ref().map(|r| bx(pat(&r.argument))),
        ),
        BindingPattern::ArrayPattern(a) => Pat::Array(
            a.elements.iter().map(|e| e.as_ref().map(pat)).collect(),
            a.rest.as_ref().map(|r| bx(pat(&r.argument))),
        ),
        BindingPattern::AssignmentPattern(a) => with_default(pat(&a.left), expr(&a.right)),
    }
}

fn member(m: &MemberExpression) -> Expr {
    match m {
        MemberExpression::StaticMemberExpression(s) => Expr::Member(
            bx(expr(&s.object)),
            crate::runtime::Sym::new(s.property.name.as_str()),
            s.optional,
        ),
        // V4's code generator reads `o["name"]` as `o.name` (a call through it fails with the
        // message of a member call); a key that is an array index stays computed. So does any
        // key of `this`: V4 loads the base of a computed access first, and a derived
        // constructor's unbound `this` is a ReferenceError there, where `this.name` is not
        // (`Codegen::visit(ArrayMemberExpression *)`, qv4codegen.cpp).
        MemberExpression::ComputedMemberExpression(c) => {
            let name = match &c.expression {
                Expression::StringLiteral(k) => k.value.as_str(),
                _ => None,
            };
            let this = matches!(
                c.object.get_inner_expression(),
                Expression::ThisExpression(_)
            );
            match name.filter(|k| !this && k.parse::<u32>().is_err()) {
                Some(k) => {
                    Expr::Member(bx(expr(&c.object)), crate::runtime::Sym::new(k), c.optional)
                }
                None => Expr::Index(bx(expr(&c.object)), bx(expr(&c.expression)), c.optional),
            }
        }
        MemberExpression::PrivateFieldExpression(_) => Expr::Unsupported("PrivateField".into()),
    }
}

/// V4 makes every call on an optional chain's spine optional, not only `f?.()`: a nullish
/// callee anywhere in the chain short-circuits it to `undefined` (`o?.n.nope()`,
/// `o.nope()?.x`; the original's `Weather.data?.temp.substring(..) ?? "--°"` relies on it). Reads of a
/// nullish object without `?.` still throw, and arguments or computed keys are not the spine.
fn chain_calls_optional(e: Expr) -> Expr {
    match e {
        Expr::Call(c, a, _) => Expr::Call(bx(chain_calls_optional(*c)), a, true),
        Expr::Member(o, n, opt) => Expr::Member(bx(chain_calls_optional(*o)), n, opt),
        Expr::Index(o, k, opt) => Expr::Index(bx(chain_calls_optional(*o)), k, opt),
        e => e,
    }
}

fn simple_target(t: &SimpleAssignmentTarget) -> Pat {
    match t {
        SimpleAssignmentTarget::AssignmentTargetIdentifier(id) => Pat::Id(id.name.to_string()),
        _ => match t.as_member_expression() {
            Some(m) => Pat::Member(bx(member(m))),
            None => Pat::Member(bx(Expr::Unsupported("TSAssignTarget".into()))),
        },
    }
}

fn target(t: &AssignmentTarget) -> Pat {
    if let Some(s) = t.as_simple_assignment_target() {
        return simple_target(s);
    }
    let maybe_default = |m: &AssignmentTargetMaybeDefault| match m {
        AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(d) => {
            with_default(target(&d.binding), expr(&d.init))
        }
        _ => target(m.as_assignment_target().unwrap()),
    };
    match t.as_assignment_target_pattern() {
        Some(AssignmentTargetPattern::ArrayAssignmentTarget(a)) => Pat::Array(
            a.elements
                .iter()
                .map(|e| e.as_ref().map(maybe_default))
                .collect(),
            a.rest.as_ref().map(|r| bx(target(&r.target))),
        ),
        Some(AssignmentTargetPattern::ObjectAssignmentTarget(o)) => Pat::Object(
            o.properties
                .iter()
                .map(|p| match p {
                    AssignmentTargetProperty::AssignmentTargetPropertyIdentifier(i) => {
                        let id = Pat::Id(i.binding.name.to_string());
                        let v = match &i.init {
                            Some(d) => Pat::Default(bx(id), bx(expr(d))),
                            None => id,
                        };
                        (Key::Name(i.binding.name.as_str().into()), v)
                    }
                    AssignmentTargetProperty::AssignmentTargetPropertyProperty(pp) => {
                        (key(&pp.name, pp.computed), maybe_default(&pp.binding))
                    }
                })
                .collect(),
            o.rest.as_ref().map(|r| bx(target(&r.target))),
        ),
        None => Pat::Member(bx(Expr::Unsupported("AssignTarget".into()))),
    }
}

fn args(a: &[Argument]) -> Vec<Expr> {
    a.iter()
        .map(|a| match a {
            Argument::SpreadElement(s) => Expr::Spread(bx(expr(&s.argument))),
            _ => expr(a.as_expression().unwrap()),
        })
        .collect()
}

/// A function of `kind`; a method's or class constructor's (`unique`) parameters are
/// UniqueFormalParameters.
fn func(f: &Function, kind: FnKind, unique: bool) -> Func {
    let own_strict = f.body.as_ref().is_some_and(|b| use_strict(&b.directives));
    function_code(own_strict, || func_code(f, kind, own_strict, unique))
}

fn func_code(f: &Function, kind: FnKind, own_strict: bool, unique: bool) -> Func {
    check_params(&f.params, own_strict, unique);
    let seen = ARGUMENTS_SEEN.with(|c| c.get());
    let evals = EVAL_SEEN.with(|c| c.get());
    let supers = supers_seen();
    let params = f.params.items.iter().map(param).collect();
    let rest = f.params.rest.as_ref().map(|r| pat(&r.rest.argument));
    let mut body = f.body.as_ref().map_or(Vec::new(), |b| stmts(&b.statements));
    // An async generator stays a coroutine nothing runs (see `Coroutine::runs`).
    let lowered = f.generator && !f.r#async;
    if lowered {
        let direct_eval = EVAL_SEEN.with(|c| c.get()) != evals;
        body = genlower::lower(&body, early(|e| e.cx.strict), direct_eval);
    }
    Func {
        name: f.id.as_ref().map(|i| i.name.to_string()),
        inferred: None,
        params,
        rest,
        body,
        kind: if lowered { FnKind::Generator } else { kind },
        coroutine: Coroutine::of(f.r#async, f.generator),
        strict: early(|e| e.cx.strict),
        uses_arguments: ARGUMENTS_SEEN.with(|c| c.get()) != seen,
        direct_eval: EVAL_SEEN.with(|c| c.get()) != evals,
        uses_super: supers_seen() != supers,
        var_init: Vec::new(),
        hoisted: Default::default(),
        nslots: 0,
        slot: None,
        this_only: false,
        expression: false,
    }
}

/// Whether the initializer of a field under `k` gets the computed key as its name.
pub fn names_field(k: &Key, v: Option<&Expr>) -> bool {
    matches!(k, Key::Computed(_)) && v.is_some_and(is_anonymous_fn)
}

/// An object-literal method whose code has a `super` property reference: MakeMethod gives it
/// the literal as its [[HomeObject]] (ECMA-262 §10.2.7).
pub fn is_super_method(e: &Expr) -> bool {
    matches!(e, Expr::Func(f) if f.uses_super && !f.is_arrow())
}

/// IsAnonymousFunctionDefinition (ECMA-262 §8.4.3): a definition still without a name.
pub fn is_anonymous_fn(e: &Expr) -> bool {
    match e {
        Expr::Func(f) => f.name.is_none() && f.inferred.is_none(),
        Expr::Class(c) => c.name.is_none() && c.inferred.is_none(),
        _ => false,
    }
}

/// `e` defined under the name `n` (ECMA-262 NamedEvaluation): an anonymous function or class
/// definition takes `n` as its `name`.
fn named(mut e: Expr, n: &str) -> Expr {
    match &mut e {
        Expr::Func(f) if f.name.is_none() && f.inferred.is_none() => {
            Rc::make_mut(f).inferred = Some(n.to_string());
        }
        Expr::Class(c) if c.name.is_none() && c.inferred.is_none() => {
            c.inferred = Some(n.to_string());
        }
        _ => {}
    }
    e
}

/// `p = e` in a binding or assignment pattern; an identifier names an anonymous `e`.
fn with_default(p: Pat, e: Expr) -> Pat {
    let e = match &p {
        Pat::Id(n) => named(e, n),
        _ => e,
    };
    Pat::Default(bx(p), bx(e))
}

/// A function expression; a named one sees its own name, as in JS:
/// `function n() {}` becomes `(() => { function n() {} return n })()`, the wrapper's `n` being
/// immutable ([`Func::expression`]).
fn named_func_expr(mut f: Func) -> Expr {
    let Some(n) = f.name.clone() else {
        return Expr::Func(Rc::new(f));
    };
    f.expression = true;
    let wrap = Func {
        name: None,
        inferred: None,
        params: Vec::new(),
        rest: None,
        body: vec![Stmt::Func(Rc::new(f)), Stmt::Return(Some(Expr::Ident(n)))],
        kind: FnKind::Arrow,
        coroutine: None,
        strict: false,
        uses_arguments: false,
        direct_eval: false,
        uses_super: false,
        var_init: Vec::new(),
        hoisted: Default::default(),
        nslots: 0,
        slot: None,
        this_only: false,
        expression: false,
    };
    Expr::Call(bx(Expr::Func(Rc::new(wrap))), Vec::new(), false)
}

/// A class; all of it is strict code.
fn class(c: &oxc_ast::ast::Class) -> Class {
    with_cx(
        |cx| EarlyCx {
            labels: cx.labels.clone(),
            strict: true,
            ..*cx
        },
        || class_code(c),
    )
}

fn class_code(c: &oxc_ast::ast::Class) -> Class {
    let mut out = Class {
        name: c.id.as_ref().map(|i| i.name.to_string()),
        inferred: None,
        extends: c.heritage.as_ref().map(|h| expr(&h.expression)),
        ctor: None,
        methods: Vec::new(),
        fields: Vec::new(),
        field_after: Vec::new(),
        blocks: Vec::new(),
        slot: None,
        size_hint: Default::default(),
    };
    for el in &c.body.body {
        match el {
            ClassElement::MethodDefinition(m) => {
                let (kind, fk) = match m.kind {
                    MethodDefinitionKind::Constructor => {
                        let kind = match c.heritage {
                            Some(_) => FnKind::DerivedConstructor,
                            None => FnKind::ClassConstructor,
                        };
                        out.ctor = Some(func(&m.value, kind, true));
                        continue;
                    }
                    MethodDefinitionKind::Method => ("method", FnKind::Method),
                    MethodDefinitionKind::Get => ("get", FnKind::Accessor),
                    MethodDefinitionKind::Set => ("set", FnKind::Accessor),
                };
                let key = key(&m.key, m.computed);
                let mut func = func(&m.value, fk, true);
                if let Key::Name(n) = &key {
                    func.inferred = Some(match kind {
                        "method" => n.to_string(),
                        _ => format!("{kind} {n}"),
                    });
                }
                out.methods.push(Method {
                    key,
                    func,
                    kind,
                    is_static: m.r#static,
                });
            }
            ClassElement::PropertyDefinition(p) => {
                let k = key(&p.key, p.computed);
                let v = p.value.as_ref().map(|v| match &k {
                    Key::Name(n) => named(expr(v), n),
                    Key::Computed(_) => expr(v),
                });
                out.field_after.push(out.methods.len() as u32);
                out.fields.push((k, v, p.r#static));
            }
            ClassElement::StaticBlock(b) => {
                let block = arrow_code(false, true, || {
                    let supers = supers_seen();
                    let evals = EVAL_SEEN.with(|c| c.get());
                    let body = stmts(&b.body);
                    Func {
                        name: None,
                        inferred: None,
                        params: Vec::new(),
                        rest: None,
                        body,
                        kind: FnKind::Arrow,
                        coroutine: None,
                        strict: true,
                        uses_arguments: false,
                        direct_eval: EVAL_SEEN.with(|c| c.get()) != evals,
                        uses_super: supers_seen() != supers,
                        var_init: Vec::new(),
                        hoisted: Default::default(),
                        nslots: 0,
                        slot: None,
                        this_only: false,
                        expression: false,
                    }
                });
                let call = Expr::Call(bx(Expr::Func(Rc::new(block))), Vec::new(), false);
                out.blocks.push((out.fields.len() as u32, call));
            }
            _ => out.blocks.push((
                out.fields.len() as u32,
                Expr::Unsupported("ClassElement".into()),
            )),
        }
    }
    out
}

fn param(p: &FormalParameter) -> Pat {
    let base = pat(&p.pattern);
    match &p.initializer {
        Some(i) => with_default(base, expr(i)),
        None => base,
    }
}

/// The value of a numeric literal. QV4's lexer has no legacy octal literals (ECMA-262 B.1.1):
/// digits after a leading zero are decimal as after any other digit, `010` is ten.
fn number(n: &NumericLiteral) -> f64 {
    let legacy_octal = |raw: &str| !raw[1..].starts_with(['o', 'O']);
    match n.raw.as_ref().map(|raw| raw.as_str()) {
        Some(raw) if n.base == NumberBase::Octal && legacy_octal(raw) => {
            raw.parse().unwrap_or(n.value)
        }
        _ => n.value,
    }
}

pub fn expr(e: &Expression) -> Expr {
    use Expression as E;
    match e {
        E::BooleanLiteral(b) => Expr::Bool(b.value),
        E::NullLiteral(_) => Expr::Null,
        E::NumericLiteral(n) => Expr::Num(number(n)),
        E::StringLiteral(s) => str_lit!(s.value),
        E::RegExpLiteral(r) => {
            Expr::Regex(r.regex.pattern.text.to_string(), r.regex.flags.to_string())
        }
        E::TemplateLiteral(t) => {
            // A cooked string with an unpaired surrogate is the substitution of its literal
            // between two empty strings (ToString of a String is itself, §13.15.5.1).
            let (mut q, mut es) = (Vec::new(), Vec::new());
            for (i, part) in t.quasis.iter().enumerate() {
                match part.value.cooked.as_ref().map(|c| str_lit!(*c)) {
                    Some(Expr::Str(c)) => q.push(c.to_string()),
                    Some(lone) => {
                        q.push(String::new());
                        es.push(lone);
                        q.push(String::new());
                    }
                    None => q.push(part.value.raw.to_string()),
                }
                if let Some(e) = t.expressions.get(i) {
                    es.push(expr(e));
                }
            }
            Expr::Template(q, es)
        }
        // `tag`a${x}b`` is the call `tag(templateObject, x)` (ECMA-262 §13.3.11): a member
        // tag is a method call with `this` = its object.
        E::TaggedTemplateExpression(t) => {
            let site = TemplateSite {
                cooked: t
                    .quasi
                    .quasis
                    .iter()
                    .map(|q| q.value.cooked.as_ref().map(|c| js_text!(*c)))
                    .collect(),
                raw: t
                    .quasi
                    .quasis
                    .iter()
                    .map(|q| q.value.raw.replace("\r\n", "\n").replace('\r', "\n"))
                    .collect(),
            };
            let args = std::iter::once(Expr::TemplateObject(Rc::new(site)))
                .chain(t.quasi.expressions.iter().map(expr))
                .collect();
            Expr::Call(bx(expr(&t.tag)), args, false)
        }
        E::Identifier(i) => match i.name.as_str() {
            "undefined" => Expr::Ident("undefined".into()),
            n => {
                if n == "arguments" {
                    ARGUMENTS_SEEN.with(|c| c.set(c.get() + 1));
                    early(|e| {
                        if e.cx.static_block {
                            e.fail("'arguments' is not allowed in class static blocks");
                        }
                    });
                }
                Expr::Ident(n.to_string())
            }
        },
        E::ThisExpression(_) => Expr::This,
        E::ArrayExpression(a) => Expr::Array(
            a.elements
                .iter()
                .map(|el| match el {
                    ArrayExpressionElement::SpreadElement(s) => Expr::Spread(bx(expr(&s.argument))),
                    ArrayExpressionElement::Elision(_) => Expr::Hole,
                    _ => expr(el.as_expression().unwrap()),
                })
                .collect(),
        ),
        E::ObjectExpression(o) => {
            let mut protos = 0;
            let props = o
                .properties
                .iter()
                .map(|p| match p {
                    ObjectPropertyKind::ObjectProperty(p) => {
                        let v = match &p.value {
                            E::FunctionExpression(f) if p.kind != PropertyKind::Init => {
                                named_func_expr(func(f, FnKind::Accessor, true))
                            }
                            E::FunctionExpression(f) if p.method => {
                                named_func_expr(func(f, FnKind::Method, true))
                            }
                            v => expr(v),
                        };
                        let (kind, prefix) = match p.kind {
                            PropertyKind::Init => (PropKind::Init, ""),
                            PropertyKind::Get => (PropKind::Get, "get "),
                            PropertyKind::Set => (PropKind::Set, "set "),
                        };
                        let k = key(&p.key, p.computed);
                        // `__proto__: v` (not computed, shorthand or a method) sets the
                        // prototype rather than defining a property, and names no function.
                        let kind = match &k {
                            Key::Name(n)
                                if &**n == "__proto__"
                                    && kind == PropKind::Init
                                    && !p.shorthand
                                    && !p.method =>
                            {
                                protos += 1;
                                PropKind::Proto
                            }
                            _ => kind,
                        };
                        let v = match &k {
                            Key::Name(n) if kind != PropKind::Proto => {
                                named(v, &format!("{prefix}{n}"))
                            }
                            _ => v,
                        };
                        (k, v, kind)
                    }
                    ObjectPropertyKind::SpreadProperty(s) => (
                        Key::Name("...".into()),
                        Expr::Spread(bx(expr(&s.argument))),
                        PropKind::Init,
                    ),
                })
                .collect();
            if protos > 1 {
                early(|e| e.fail("Duplicate __proto__ fields are not allowed in object literals"));
            }
            Expr::Object(props)
        }
        E::ArrowFunctionExpression(a) => {
            let own_strict =
                matches!(&a.body, ArrowFunctionBody::FunctionBody(b) if use_strict(&b.directives));
            arrow_code(own_strict, false, || {
                check_params(&a.params, own_strict, true);
                let supers = supers_seen();
                let evals = EVAL_SEEN.with(|c| c.get());
                let params = a.params.items.iter().map(param).collect();
                let rest = a.params.rest.as_ref().map(|r| pat(&r.rest.argument));
                let body = match &a.body {
                    ArrowFunctionBody::FunctionBody(b) => stmts(&b.statements),
                    b => vec![Stmt::Return(Some(expr(b.as_expression().unwrap())))],
                };
                Expr::Func(Rc::new(Func {
                    name: None,
                    inferred: None,
                    params,
                    rest,
                    body,
                    kind: FnKind::Arrow,
                    coroutine: Coroutine::of(a.r#async, false),
                    strict: early(|e| e.cx.strict),
                    uses_arguments: false,
                    direct_eval: EVAL_SEEN.with(|c| c.get()) != evals,
                    uses_super: supers_seen() != supers,
                    var_init: Vec::new(),
                    hoisted: Default::default(),
                    nslots: 0,
                    slot: None,
                    this_only: false,
                    expression: false,
                }))
            })
        }
        E::FunctionExpression(f) => named_func_expr(func(f, FnKind::Normal, false)),
        E::AssignmentExpression(a) => {
            let op = assign_op(a.operator);
            let t = target(&a.left);
            let v = match (&t, op) {
                (Pat::Id(n), AssignOp::Assign | AssignOp::Logical(_)) => named(expr(&a.right), n),
                _ => expr(&a.right),
            };
            Expr::Assign(op, bx(t), bx(v))
        }
        E::UpdateExpression(u) => Expr::Update(
            match u.operator {
                UpdateOperator::Increment => UpdateOp::Inc,
                UpdateOperator::Decrement => UpdateOp::Dec,
            },
            u.prefix,
            bx(simple_target(&u.argument)),
        ),
        E::BinaryExpression(b) => {
            Expr::Binary(bin_op(b.operator), bx(expr(&b.left)), bx(expr(&b.right)))
        }
        E::LogicalExpression(l) => Expr::Logical(
            logical_op(l.operator),
            bx(expr(&l.left)),
            bx(expr(&l.right)),
        ),
        E::UnaryExpression(u) => Expr::Unary(un_op(u.operator), bx(expr(&u.argument))),
        E::ConditionalExpression(c) => Expr::Cond(
            bx(expr(&c.test)),
            bx(expr(&c.consequent)),
            bx(expr(&c.alternate)),
        ),
        E::CallExpression(c) => {
            if matches!(c.callee, E::Super(_)) {
                // ClassStaticBlockBody (ECMA-262 §15.7.1): Contains SuperCall is an error.
                early(|e| {
                    if e.cx.static_block {
                        e.fail("'super()' is not allowed in class static blocks");
                    }
                });
            }
            if !c.optional && matches!(&c.callee, E::Identifier(i) if i.name == "eval") {
                EVAL_SEEN.with(|c| c.set(c.get() + 1));
                ARGUMENTS_SEEN.with(|c| c.set(c.get() + 1));
            }
            Expr::Call(bx(expr(&c.callee)), args(&c.arguments), c.optional)
        }
        E::NewExpression(n) => Expr::New(bx(expr(&n.callee)), args(&n.arguments)),
        E::ChainExpression(c) => Expr::Chain(bx(chain_calls_optional(match &c.expression {
            ChainElement::CallExpression(c) => {
                Expr::Call(bx(expr(&c.callee)), args(&c.arguments), c.optional)
            }
            ChainElement::TSNonNullExpression(t) => expr(&t.expression),
            m => member(m.as_member_expression().unwrap()),
        }))),
        E::ParenthesizedExpression(p) => expr(&p.expression),
        E::SequenceExpression(s) => Expr::Seq(s.expressions.iter().map(expr).collect()),
        E::ClassExpression(c) => Expr::Class(bx(class(c))),
        E::Super(_) => {
            SUPER_SEEN.with(|c| c.set(c.get() + 1));
            Expr::Super
        }
        E::NewTarget(_) => Expr::NewTarget,
        E::YieldExpression(y) => Expr::Yield(y.argument.as_ref().map(|a| bx(expr(a))), y.delegate),
        E::TSAsExpression(t) => expr(&t.expression),
        E::TSSatisfiesExpression(t) => expr(&t.expression),
        E::TSNonNullExpression(t) => expr(&t.expression),
        E::TSTypeAssertion(t) => expr(&t.expression),
        _ => match e.as_member_expression() {
            Some(m) => member(m),
            None => Expr::Unsupported(kind_name(e).into()),
        },
    }
}

fn kind_name(e: &Expression) -> &'static str {
    match e {
        Expression::ClassExpression(_) => "ClassExpression",
        Expression::AwaitExpression(_) => "Await",
        Expression::YieldExpression(_) => "Yield",
        Expression::BigIntLiteral(_) => {
            BIGINT_SEEN.with(|c| c.set(true));
            "BigInt"
        }
        Expression::Super(_) => "Super",
        Expression::ImportExpression(_) => "Import",
        _ => "OtherExpression",
    }
}

fn decl(v: &VariableDeclaration) -> Stmt {
    Stmt::Var(
        v.kind.as_str(),
        v.declarations
            .iter()
            .map(|d| {
                let p = pat(&d.id);
                let init = d.init.as_ref().map(|e| match &p {
                    Pat::Id(n) => named(expr(e), n),
                    _ => expr(e),
                });
                (p, init)
            })
            .collect(),
    )
}

fn for_left(l: &ForStatementLeft) -> (Option<&'static str>, Pat) {
    match l {
        ForStatementLeft::VariableDeclaration(v) => {
            (Some(v.kind.as_str()), pat(&v.declarations[0].id))
        }
        _ => (None, target(l.as_assignment_target().unwrap())),
    }
}

pub fn stmt(s: &Statement) -> Stmt {
    use Statement as S;
    match s {
        S::BlockStatement(b) => Stmt::Block(stmts(&b.body)),
        S::BreakStatement(b) => {
            let l = b.label.as_ref().map(|l| l.name.to_string());
            early(|e| match &l {
                Some(l) if !e.cx.labels.iter().any(|(n, _)| n == l) => {
                    e.fail(format!("Undefined label '{l}'"))
                }
                None if e.cx.loops + e.cx.switches == 0 => e.fail("Illegal break statement"),
                _ => {}
            });
            Stmt::Break(l)
        }
        S::ContinueStatement(c) => {
            let l = c.label.as_ref().map(|l| l.name.to_string());
            early(|e| match &l {
                Some(l) => match e.cx.labels.iter().find(|(n, _)| n == l) {
                    None => e.fail(format!("Undefined label '{l}'")),
                    Some((_, false)) => e.fail(format!(
                        "Illegal continue statement: '{l}' does not denote an iteration statement"
                    )),
                    Some(_) => {}
                },
                None if e.cx.loops == 0 => {
                    e.fail("Illegal continue statement: no surrounding iteration statement")
                }
                None => {}
            });
            Stmt::Continue(l)
        }
        S::LabeledStatement(l) => {
            let name = l.label.name.to_string();
            let mut body = &l.body;
            while let S::LabeledStatement(inner) = body {
                body = &inner.body;
            }
            let iteration = matches!(
                body,
                S::ForStatement(_)
                    | S::ForInStatement(_)
                    | S::ForOfStatement(_)
                    | S::WhileStatement(_)
                    | S::DoWhileStatement(_)
            );
            early(|e| {
                if e.cx.labels.iter().any(|(n, _)| *n == name) {
                    e.fail(format!("Label '{name}' has already been declared"));
                }
                e.cx.labels.push((name.clone(), iteration));
            });
            let body = stmt(&l.body);
            early(|e| e.cx.labels.pop());
            Stmt::Labeled(name, bx(body))
        }
        S::DoWhileStatement(d) => {
            Stmt::DoWhile(bx(breakable(false, || stmt(&d.body))), expr(&d.test))
        }
        S::EmptyStatement(_) => Stmt::Empty,
        S::ExpressionStatement(e) => Stmt::Expr(expr(&e.expression)),
        S::ForInStatement(f) => {
            let (k, p) = for_left(&f.left);
            Stmt::ForIn(k, p, expr(&f.right), bx(breakable(false, || stmt(&f.body))))
        }
        S::ForOfStatement(f) => {
            let (k, p) = for_left(&f.left);
            Stmt::ForOf(k, p, expr(&f.right), bx(breakable(false, || stmt(&f.body))))
        }
        S::ForStatement(f) => Stmt::For(
            f.init.as_ref().map(|i| {
                bx(match i {
                    ForStatementInit::VariableDeclaration(v) => decl(v),
                    _ => Stmt::Expr(expr(i.as_expression().unwrap())),
                })
            }),
            f.test.as_ref().map(expr),
            f.update.as_ref().map(expr),
            bx(breakable(false, || stmt(&f.body))),
        ),
        S::IfStatement(i) => Stmt::If(
            expr(&i.test),
            bx(stmt(&i.consequent)),
            i.alternate.as_ref().map(|a| bx(stmt(a))),
        ),
        S::ReturnStatement(r) => Stmt::Return(r.argument.as_ref().map(expr)),
        S::SwitchStatement(sw) => Stmt::Switch(
            expr(&sw.discriminant),
            breakable(true, || {
                sw.cases
                    .iter()
                    .map(|c| (c.test.as_ref().map(expr), stmts(&c.consequent)))
                    .collect()
            }),
        ),
        S::ThrowStatement(t) => Stmt::Throw(expr(&t.argument)),
        S::TryStatement(t) => Stmt::Try(
            stmts(&t.block.body),
            t.handler.as_ref().map(|h| {
                (
                    h.param.as_ref().map(|p| pat(&p.pattern)),
                    stmts(&h.body.body),
                )
            }),
            t.finalizer.as_ref().map(|f| stmts(&f.body)),
        ),
        S::WithStatement(w) => {
            // ECMA-262 §14.11.1: a `with` statement in strict mode code is a Syntax Error.
            early(|e| {
                if e.cx.strict {
                    e.fail("'with' statements are not allowed in strict mode");
                }
            });
            Stmt::With(expr(&w.object), bx(stmt(&w.body)))
        }
        S::WhileStatement(w) => Stmt::While(expr(&w.test), bx(breakable(false, || stmt(&w.body)))),
        S::VariableDeclaration(v) => decl(v),
        S::FunctionDeclaration(f) => Stmt::Func(Rc::new(func(f, FnKind::Normal, false))),
        S::ClassDeclaration(c) => Stmt::Class(bx(class(c))),
        S::ImportDeclaration(_)
        | S::ExportNamedDeclaration(_)
        | S::ExportDefaultDeclaration(_)
        | S::ExportAllDeclaration(_) => Stmt::Unsupported("Module".into()),
        _ => Stmt::Unsupported("OtherStatement".into()),
    }
}

pub fn stmts(s: &[Statement]) -> Vec<Stmt> {
    s.iter().map(stmt).collect()
}

/// Lowered IR by source: `[expression-like, function]`.
type LowerCache = [rustc_hash::FxHashMap<String, Rc<Vec<Stmt>>>; 2];

thread_local! {
    static LOWERED: std::cell::RefCell<LowerCache> = Default::default();
}

/// ri::wstat: lowered sources and their total source bytes.
pub fn lowered_stats() -> (usize, usize) {
    LOWERED.with(|m| {
        let m = m.borrow();
        (
            m.iter().map(|c| c.len()).sum(),
            m.iter().flat_map(|c| c.keys()).map(|k| k.len()).sum(),
        )
    })
}

/// Lowered IR shared by every instance of the same source (delegates, notifications).
pub fn lower_rc(s: &crate::Script) -> Result<Rc<Vec<Stmt>>, String> {
    let f = matches!(s.kind, crate::ScriptKind::Function) as usize;
    let src: &str = &s.src;
    if let Some(b) = LOWERED.with(|m| m.borrow()[f].get(src).cloned()) {
        return Ok(b);
    }
    let b = Rc::new(lower(s)?);
    LOWERED.with(|m| m.borrow_mut()[f].insert(src.to_string(), b.clone()));
    Ok(b)
}

/// Set by tools that map live bindings back to their IR (`aotcheck`): always lower.
pub static KEEP_IR: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// `lower_rc`, or a shared empty body when the script has an AOT entry (`Code::run` never
/// looks at the body then), sparing the oxc parse and the `LOWERED` lookup per instance.
pub fn lower_unless_aot(s: &crate::Script, aot: bool) -> Result<Rc<Vec<Stmt>>, String> {
    if aot && !KEEP_IR.load(std::sync::atomic::Ordering::Relaxed) {
        thread_local!(static EMPTY: Rc<Vec<Stmt>> = Rc::new(Vec::new()));
        return Ok(EMPTY.with(Rc::clone));
    }
    if !aot {
        thread_local!(static MISS: bool = crate::log_on!("ri::aotmiss"));
        if MISS.with(|m| *m) {
            let head: String = s.src.chars().take(100).collect();
            tracing::debug!(target: "ri::aotmiss", "aotmiss line {}: {}", s.line, head.replace('\n', " "));
        }
    }
    lower_rc(s)
}

/// Lower a QML script span. Expressions become `[Return(e)]` bodies; statements/blocks as-is.
pub fn lower(s: &crate::Script) -> Result<Vec<Stmt>, String> {
    let src = crate::qqmljs::for_async_of(&s.src);
    let wraps = match s.kind {
        crate::ScriptKind::Function => vec![src.to_string()],
        // Return-wrap first so a lone string literal is a value, not a directive prologue.
        crate::ScriptKind::Expression => vec![
            format!("function __qml() {{\nreturn ({src}\n);\n}}"),
            format!("function __qml() {{\n{src}\n}}"),
        ],
        _ => vec![
            format!("function __qml() {{\n{src}\n}}"),
            format!("function __qml() {{\nreturn ({src}\n);\n}}"),
        ],
    };
    let alloc = Allocator::default();
    for (wrap, st) in wraps
        .iter()
        .flat_map(|w| [(w, SourceType::mjs()), (w, SourceType::ts())])
    {
        let r = Parser::new(&alloc, wrap, st).parse();
        if !r.diagnostics.is_empty() {
            continue;
        }
        let Some(Statement::FunctionDeclaration(f)) = r.program.body.first() else {
            break;
        };
        // QML binding code is sloppy unless it says "use strict" (the wrapper's directives).
        early_reset(false);
        BIGINT_SEEN.with(|c| c.set(false));
        let f = func(f, FnKind::Normal, false);
        early_result()?;
        if BIGINT_SEEN.with(|c| c.get()) {
            return Ok(reject_bigint(Vec::new()));
        }
        if s.kind == crate::ScriptKind::Function {
            let mut body = vec![Stmt::Func(Rc::new(f))];
            resolve(&mut body);
            return Ok(body);
        }
        // The completion value of the binding's statement is the binding's value
        // (`x: { a; b }` is b).
        let mut body = f.body;
        binding_completion(&mut body);
        resolve(&mut body);
        return Ok(body);
    }
    Err("unparsable".into())
}

/// [`crate::Shape`] of expression binding `s` classified on its oxc AST, the way
/// `QQmlJS::IRBuilder::setBindingValue` reads the parsed statement: an independent check of
/// the token-level [`crate::Shape::of`] the runtime uses. `None` when it does not parse.
pub fn qmlir_shape(s: &crate::Script) -> Option<crate::Shape> {
    use crate::Shape;
    if s.kind != crate::ScriptKind::Expression {
        return Some(Shape::Script);
    }
    let alloc = Allocator::default();
    let r = Parser::new(&alloc, &s.src, SourceType::mjs()).parse();
    if !r.diagnostics.is_empty() {
        return None;
    }
    let p = &r.program;
    // A lone string statement parses as a directive prologue.
    if p.body.is_empty() && p.directives.len() == 1 {
        return Some(Shape::Literal);
    }
    let [Statement::ExpressionStatement(e)] = p.body.as_slice() else {
        return Some(Shape::Script);
    };
    if !p.directives.is_empty() {
        return Some(Shape::Script);
    }
    fn chain<'a>(e: &'a Expression, out: &mut Vec<&'a str>) -> bool {
        match e {
            Expression::Identifier(i) => {
                out.push(i.name.as_str());
                true
            }
            Expression::StaticMemberExpression(m) if !m.optional && chain(&m.object, out) => {
                out.push(m.property.name.as_str());
                true
            }
            _ => false,
        }
    }
    Some(match &e.expression {
        Expression::StringLiteral(_)
        | Expression::NumericLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_) => Shape::Literal,
        Expression::TemplateLiteral(t) if t.expressions.is_empty() => Shape::Literal,
        Expression::UnaryExpression(u)
            if u.operator == UnaryOperator::UnaryNegation
                && matches!(u.argument, Expression::NumericLiteral(_)) =>
        {
            Shape::Literal
        }
        e => {
            let mut parts = Vec::new();
            if chain(e, &mut parts)
                && (2..=3).contains(&parts.len())
                && parts[0].starts_with(|c: char| c.is_ascii_uppercase())
            {
                Shape::Member(parts.join(".").into())
            } else {
                Shape::Script
            }
        }
    })
}

/// Makes a binding's function body return its completion value the way QV4's codegen does
/// for a `ContextType::Binding` (`Codegen::statementList`, `completionStatement`), which is
/// decided from the source text, not at run time:
///
/// - Outside a loop only the list's completion statement gives a value: the last statement
///   that is not empty, a declaration or an empty block, or the one before a `break` or
///   `continue`. Statements before it are run for their effects.
/// - Inside a loop, a `switch` or a labelled statement that is neither (QV4's
///   `ControlFlowLoop`), every expression statement gives the value so far and every
///   statement list first resets it to `undefined`, except a `switch` clause's own list;
///   a `switch` resets it before its clauses run.
/// - `if` and `try`/`catch` pass the value of the branch that ran; a `finally` block never
///   changes it.
///
/// A completion statement the function ends with returns its value directly (`{ a; b }` is
/// `a; return b`); otherwise the value is kept in [`EVAL_COMPLETION`], which QV4 keeps in the
/// return value register.
fn binding_completion(body: &mut Vec<Stmt>) {
    let mut c = Completion { stores: 0 };
    c.list(body, false, false, true);
    if c.stores > 0 {
        body.insert(
            0,
            Stmt::Var("let", vec![(Pat::Id(EVAL_COMPLETION.into()), None)]),
        );
    }
}

/// [`binding_completion`]'s walk: `stores` counts the statements that assign
/// [`EVAL_COMPLETION`].
struct Completion {
    stores: u32,
}

/// QV4's `CompletionState` of a block's statement list.
#[derive(PartialEq)]
enum CompletionState {
    Empty,
    EmptyAbrupt,
    NonEmpty,
}

/// QV4's `completionState`: it looks for `Kind_VariableDeclaration`, which no statement is,
/// so a `var`/`let`/`const` statement makes a block non-empty (`{ 3; { var z = 1 } }` is
/// `undefined` in Qt, while `{ 3; var z = 1 }` is 3).
fn completion_state(list: &[Stmt]) -> CompletionState {
    for s in list {
        match s {
            Stmt::Break(_) | Stmt::Continue(_) => return CompletionState::EmptyAbrupt,
            Stmt::Empty | Stmt::Func(_) => {}
            Stmt::Block(b) => match completion_state(b) {
                CompletionState::Empty => {}
                state => return state,
            },
            _ => return CompletionState::NonEmpty,
        }
    }
    CompletionState::Empty
}

/// QV4's `completionStatement`: the index of the statement of `list` whose value is the
/// list's completion value.
fn completion_statement(list: &[Stmt]) -> Option<usize> {
    let mut found = None;
    for (i, s) in list.iter().enumerate() {
        match s {
            Stmt::Break(_) | Stmt::Continue(_) => return found,
            Stmt::Throw(_) | Stmt::Return(_) => return Some(i),
            Stmt::Empty | Stmt::Var(..) | Stmt::Func(_) => continue,
            Stmt::Block(b) => match completion_state(b) {
                CompletionState::Empty => continue,
                CompletionState::EmptyAbrupt => return Some(i),
                CompletionState::NonEmpty => {}
            },
            _ => {}
        }
        found = Some(i);
    }
    found
}

/// Whether running `s` does nothing: what may follow a completion statement that returns
/// its value directly.
fn inert(s: &Stmt) -> bool {
    match s {
        Stmt::Empty | Stmt::Func(_) => true,
        Stmt::Var(_, d) => d
            .iter()
            .all(|(p, init)| matches!(p, Pat::Id(_)) && init.is_none()),
        Stmt::Block(b) => b.iter().all(inert),
        _ => false,
    }
}

impl Completion {
    fn set(&mut self, e: Expr) -> Stmt {
        self.stores += 1;
        Stmt::Expr(Expr::Assign(
            AssignOp::Assign,
            bx(Pat::Id(EVAL_COMPLETION.into())),
            bx(e),
        ))
    }

    fn reset(&mut self) -> Stmt {
        self.set(Expr::Ident("undefined".into()))
    }

    fn result() -> Stmt {
        Stmt::Return(Some(Expr::Ident(EVAL_COMPLETION.into())))
    }

    /// `Codegen::statementList` with `requiresReturnValue` set. `in_loop`: inside a
    /// `ControlFlowLoop`; `clause`: the list is a `switch` clause's; `tail`: the function
    /// ends where the list does.
    fn list(&mut self, list: &mut Vec<Stmt>, in_loop: bool, clause: bool, tail: bool) {
        // QV4 returns at once from an empty list: `{}` leaves the value alone.
        if list.is_empty() {
            return;
        }
        if in_loop {
            for s in list.iter_mut() {
                self.stmt(s, true, false);
                // QV4 generates no code past these.
                if matches!(
                    s,
                    Stmt::Throw(_) | Stmt::Break(_) | Stmt::Continue(_) | Stmt::Return(_)
                ) {
                    break;
                }
            }
            if !clause {
                let reset = self.reset();
                list.insert(0, reset);
            }
            return;
        }
        let Some(i) = completion_statement(list) else {
            return;
        };
        let direct = tail && list[i + 1..].iter().all(inert);
        let stores = self.stores;
        self.stmt(&mut list[i], false, direct);
        if tail && !direct && self.stores > stores {
            list.push(Self::result());
        }
    }

    /// A statement QV4 generates with `requiresReturnValue` set. With `tail` the function
    /// ends after `s`, which then returns its value.
    fn stmt(&mut self, s: &mut Stmt, in_loop: bool, tail: bool) {
        let stores = self.stores;
        match s {
            Stmt::Expr(e) => {
                let e = std::mem::replace(e, Expr::Null);
                *s = match tail {
                    true => Stmt::Return(Some(e)),
                    false => self.set(e),
                };
                return;
            }
            Stmt::Block(b) => return self.list(b, in_loop, false, tail),
            Stmt::If(_, a, b) => {
                self.stmt(a, in_loop, tail);
                if let Some(b) = b {
                    self.stmt(b, in_loop, tail);
                }
                return;
            }
            Stmt::For(_, _, _, b)
            | Stmt::ForOf(_, _, _, b)
            | Stmt::ForIn(_, _, _, b)
            | Stmt::While(_, b)
            | Stmt::DoWhile(b, _) => self.stmt(b, true, false),
            Stmt::Switch(..) => {
                self.clauses(s);
                let reset = self.reset();
                *s = Stmt::Block(vec![reset, std::mem::replace(s, Stmt::Empty)]);
            }
            // A labelled loop or `switch` is that statement; any other labelled statement
            // is a `ControlFlowLoop` of its own.
            Stmt::Labeled(_, b) if matches!(**b, Stmt::Switch(..)) => {
                self.clauses(b);
                let reset = self.reset();
                *s = Stmt::Block(vec![reset, std::mem::replace(s, Stmt::Empty)]);
            }
            Stmt::Labeled(_, b) => self.stmt(b, true, false),
            // The `finally` block runs with the value saved and restored around it.
            Stmt::Try(b, c, _) => {
                self.list(b, in_loop, false, false);
                if let Some((_, c)) = c {
                    self.list(c, in_loop, false, false);
                }
            }
            _ => return,
        }
        // Nothing stored: the value is `undefined`, which falling off the end gives.
        if tail && self.stores > stores {
            *s = Stmt::Block(vec![std::mem::replace(s, Stmt::Empty), Self::result()]);
        }
    }

    fn clauses(&mut self, s: &mut Stmt) {
        if let Stmt::Switch(_, cases) = s {
            for (_, b) in cases {
                self.list(b, true, true, false);
            }
        }
    }
}

/// Lower a `.js` resource (Qt `.pragma library` style script).
pub fn lower_program(src: &str) -> Result<Vec<Stmt>, String> {
    // Qt's script directives (`.pragma library`, `.import "x.js" as X`), which come before the
    // program, are blanked to keep the line numbers.
    let end = crate::eval::script_directives(src)?.end;
    let blank = |c: char| if c == '\n' { '\n' } else { ' ' };
    let src: String = src[..end]
        .chars()
        .map(blank)
        .chain(src[end..].chars())
        .collect();
    let src = crate::qqmljs::for_async_of(&src);
    let alloc = Allocator::default();
    let r = Parser::new(&alloc, &src, SourceType::cjs()).parse();
    if !r.diagnostics.is_empty() {
        return Err(r.diagnostics[0].to_string());
    }
    BIGINT_SEEN.with(|c| c.set(false));
    early_reset(use_strict(&r.program.directives));
    let body = stmts(&r.program.body);
    early_result()?;
    let mut body = reject_bigint(body);
    resolve(&mut body);
    Ok(body)
}

/// The binding [`lower_eval`] keeps the completion value in: not a JS identifier, so the
/// evaluated code cannot name it.
pub const EVAL_COMPLETION: &str = "%completion";

/// PerformEval (ECMA-262 §19.2.1.1) source text as a Script, with whether it is strict code.
/// Its completion value (ECMA-262 §8.1 UpdateEmpty, as QV4's codegen keeps it in the return
/// value register) is assigned to [`EVAL_COMPLETION`], declared by the first statement and
/// returned by the last.
pub fn lower_eval(src: &str) -> Result<(Vec<Stmt>, bool), String> {
    let (mut code, strict) = eval_script(src, false)?;
    resolve(&mut code);
    Ok((code, strict))
}

/// [`lower_eval`] for a direct eval (ECMA-262 §19.2.1.1 with `direct` true) called in
/// `scope`: the code is strict when the calling code is, and its names are resolved against
/// the caller's.
pub fn lower_direct_eval(src: &str, scope: &EvalScope) -> Result<EvalCode, String> {
    let (mut body, strict) = eval_script(src, scope.strict)?;
    let mut r = Resolver {
        scopes: scope.scopes.clone(),
        base: scope.scopes.len(),
        strict,
        renamed: scope.renamed.clone(),
        renamed_at: scope.renamed_at,
    };
    let mut declared = Vec::new();
    var_names(&body, &mut declared);
    for s in &body {
        if let Stmt::Func(f) = s {
            declared.extend(f.name.clone());
        }
    }
    let mut seen = std::collections::HashSet::new();
    declared.retain(|n| seen.insert(n.clone()));
    // The lexical declarations, and for strict code the `var` and function ones too, are
    // bindings of the evaluated code's own environment.
    let mut own: Vec<(String, bool)> = lexical_decls(&body)
        .into_iter()
        .filter(|(n, _)| strict || !declared.contains(n))
        .collect();
    let mut vars = Vec::new();
    let mut slots = std::collections::HashMap::new();
    if strict {
        own.extend(declared.into_iter().map(|n| (n, false)));
    } else {
        for s in &body {
            if let Stmt::Func(f) = s
                && let Some(n) = &f.name
                && let Some(Some(at)) = r.var_binding(n)
            {
                slots.insert(n.clone(), at);
            }
        }
        vars = declared;
        vars.retain(|n| r.var_binding(n).is_none());
    }
    r.scoped(own, false, |r| r.stmts(&mut body));
    Ok(EvalCode {
        body,
        strict,
        vars,
        slots,
    })
}

/// The source text of an eval as a Script lowered but not resolved, with whether it is
/// strict code (`strict`: the code calling a direct eval is).
/// The parser's message `m` in QV4's words where the source ends inside a bracket: QV4's
/// parser names the token whose insertion lets it go on (``Expected token `)'``). Other
/// syntax errors keep the parser's own words: QV4 picks its token from its LALR table (a `,`
/// after a list element, a `numeric literal` where an operand is missing), which this parser
/// has no counterpart of.
fn unclosed_message(m: String) -> String {
    for c in [")", "]", "}"] {
        if m == format!("Expected `{c}` but found `EOF`") {
            return format!("Expected token `{c}'");
        }
    }
    m
}

fn eval_script(src: &str, strict: bool) -> Result<(Vec<Stmt>, bool), String> {
    let src = &*crate::qqmljs::for_async_of(src);
    let alloc = Allocator::default();
    let r = Parser::new(&alloc, src, SourceType::script()).parse();
    if !r.diagnostics.is_empty() {
        return Err(unclosed_message(r.diagnostics[0].to_string()));
    }
    let strict = strict || use_strict(&r.program.directives);
    BIGINT_SEEN.with(|c| c.set(false));
    early_reset(strict);
    let body = stmts(&r.program.body);
    early_result()?;
    // A directive is an expression statement too: its string is the completion value so far.
    let mut code = vec![Stmt::Var(
        "let",
        vec![(
            Pat::Id(EVAL_COMPLETION.into()),
            r.program
                .directives
                .last()
                .map(|d| Expr::Str(js_text!(d.expression.value).into())),
        )],
    )];
    for s in reject_bigint(body) {
        eval_completion(s, &mut code);
    }
    code.push(Stmt::Return(Some(Expr::Ident(EVAL_COMPLETION.into()))));
    Ok((code, strict))
}

/// `s` with each expression statement's value stored as the completion value, pushed onto
/// `out`. Statements that end with UpdateEmpty(_, undefined) (if, loops, switch, try) reset
/// it first; a block or a declaration leaves it.
fn eval_completion(s: Stmt, out: &mut Vec<Stmt>) {
    let set = |e: Expr| {
        Stmt::Expr(Expr::Assign(
            AssignOp::Assign,
            bx(Pat::Id(EVAL_COMPLETION.into())),
            bx(e),
        ))
    };
    let inner = |s: Box<Stmt>| {
        let mut v = Vec::new();
        eval_completion(*s, &mut v);
        bx(match v.len() {
            1 => v.pop().unwrap(),
            _ => Stmt::Block(v),
        })
    };
    let list = |b: Vec<Stmt>| {
        let mut v = Vec::new();
        b.into_iter().for_each(|s| eval_completion(s, &mut v));
        v
    };
    let resets = |s: &Stmt| match s {
        Stmt::Labeled(_, b) => matches!(
            **b,
            Stmt::If(..)
                | Stmt::For(..)
                | Stmt::ForOf(..)
                | Stmt::ForIn(..)
                | Stmt::While(..)
                | Stmt::DoWhile(..)
                | Stmt::Switch(..)
                | Stmt::Try(..)
                | Stmt::With(..)
        ),
        Stmt::If(..)
        | Stmt::For(..)
        | Stmt::ForOf(..)
        | Stmt::ForIn(..)
        | Stmt::While(..)
        | Stmt::DoWhile(..)
        | Stmt::Switch(..)
        | Stmt::Try(..)
        | Stmt::With(..) => true,
        _ => false,
    };
    if resets(&s) {
        out.push(set(Expr::Ident("undefined".into())));
    }
    out.push(match s {
        Stmt::Expr(e) => set(e),
        Stmt::Block(b) => Stmt::Block(list(b)),
        Stmt::If(c, a, b) => Stmt::If(c, inner(a), b.map(inner)),
        Stmt::For(i, c, u, b) => Stmt::For(i, c, u, inner(b)),
        Stmt::ForOf(k, p, e, b) => Stmt::ForOf(k, p, e, inner(b)),
        Stmt::ForIn(k, p, e, b) => Stmt::ForIn(k, p, e, inner(b)),
        Stmt::While(c, b) => Stmt::While(c, inner(b)),
        Stmt::DoWhile(b, c) => Stmt::DoWhile(inner(b), c),
        Stmt::Switch(e, cs) => Stmt::Switch(e, cs.into_iter().map(|(t, b)| (t, list(b))).collect()),
        Stmt::Labeled(l, b) => Stmt::Labeled(l, inner(b)),
        Stmt::With(e, b) => Stmt::With(e, inner(b)),
        Stmt::Try(b, c, f) => Stmt::Try(list(b), c.map(|(p, b)| (p, list(b))), f),
        s => s,
    });
}

/// CreateDynamicFunction (ECMA-262 §20.2.1.1.1) for `Function(...params, body)`: lowers
/// `return (function anonymous(<params>\n) {\n<body>\n});`. The parameters and the body
/// must each be the function's whole parameter list and body, as when the spec parses them
/// on their own, so neither can close the function early and run code outside it.
pub fn lower_dynamic_function(
    params: &str,
    body: &str,
    generator: bool,
) -> Result<Vec<Stmt>, String> {
    let head = match generator {
        true => "return (function* anonymous(",
        false => "return (function anonymous(",
    };
    let (params, body) = (
        &*crate::qqmljs::for_async_of(params),
        &*crate::qqmljs::for_async_of(body),
    );
    let src = format!("{head}{params}\n) {{\n{body}\n}});");
    let alloc = Allocator::default();
    let r = Parser::new(&alloc, &src, SourceType::cjs()).parse();
    if !r.diagnostics.is_empty() {
        return Err(r.diagnostics[0].to_string());
    }
    let params_span = (head.len() - 1, head.len() + params.len() + 2);
    let body_span = (params_span.1 + 1, src.len() - 2);
    let whole = match r.program.body.as_slice() {
        [Statement::ReturnStatement(ret)] => match &ret.argument {
            Some(Expression::ParenthesizedExpression(p)) => match &p.expression {
                Expression::FunctionExpression(f) => {
                    let span = |s: oxc_span::Span| (s.start as usize, s.end as usize);
                    span(f.params.span) == params_span
                        && f.body.as_ref().is_some_and(|b| span(b.span) == body_span)
                }
                _ => false,
            },
            _ => false,
        },
        _ => false,
    };
    if !whole {
        return Err("Function: invalid parameters or body".into());
    }
    BIGINT_SEEN.with(|c| c.set(false));
    early_reset(false);
    let body = stmts(&r.program.body);
    early_result()?;
    let mut body = reject_bigint(body);
    resolve(&mut body);
    Ok(body)
}

/// Calls `on_expr` for every expression and `on_stmt` for every statement of `body`, outer
/// before inner: nested functions (parameters, defaults and body), binding and assignment
/// patterns (computed keys and defaults at any depth), `for-in/of` heads, `catch` parameters
/// and classes (`extends`, computed keys, methods and field initializers) included.
pub fn visit(body: &[Stmt], on_expr: &mut dyn FnMut(&Expr), on_stmt: &mut dyn FnMut(&Stmt)) {
    struct V<'a, 'b> {
        on_expr: &'a mut dyn FnMut(&Expr),
        on_stmt: &'b mut dyn FnMut(&Stmt),
    }
    impl V<'_, '_> {
        fn e(&mut self, x: &Expr) {
            (self.on_expr)(x);
            match x {
                Expr::Template(_, v) | Expr::Array(v) | Expr::Seq(v) => {
                    v.iter().for_each(|x| self.e(x))
                }
                Expr::Member(a, ..) | Expr::Chain(a) | Expr::Unary(_, a) | Expr::Spread(a) => {
                    self.e(a)
                }
                Expr::Index(a, b, _)
                | Expr::DefineField(a, b, _)
                | Expr::Binary(_, a, b)
                | Expr::Logical(_, a, b) => {
                    self.e(a);
                    self.e(b)
                }
                Expr::Call(a, v, _) | Expr::New(a, v) => {
                    self.e(a);
                    v.iter().for_each(|x| self.e(x))
                }
                Expr::Cond(a, b, c) => [a, b, c].iter().for_each(|x| self.e(x)),
                Expr::Assign(_, p, b) => {
                    self.p(p);
                    self.e(b)
                }
                Expr::Update(_, _, p) => self.p(p),
                Expr::Yield(a, _) => a.iter().for_each(|x| self.e(x)),
                Expr::Object(v) => v.iter().for_each(|(k, x, _)| {
                    self.key(k);
                    self.e(x)
                }),
                Expr::Func(f) => self.func(f),
                Expr::Class(c) => self.class(c),
                Expr::DirectEval(_, c) | Expr::Dynamic(_, _, c) => self.e(c),
                Expr::Num(_)
                | Expr::Str(_)
                | Expr::Str16(_)
                | Expr::Bool(_)
                | Expr::Null
                | Expr::Ident(_)
                | Expr::Local(..)
                | Expr::This
                | Expr::Regex(..)
                | Expr::TemplateObject(_)
                | Expr::Hole
                | Expr::Super
                | Expr::NewTarget
                | Expr::Unsupported(_) => {}
            }
        }
        fn p(&mut self, p: &Pat) {
            match p {
                Pat::Id(_) | Pat::Local(..) => {}
                Pat::Const(p, _) | Pat::Dynamic(_, _, p) => self.p(p),
                Pat::Member(x) | Pat::Init(x) => self.e(x),
                Pat::Default(p, x) => {
                    self.p(p);
                    self.e(x)
                }
                Pat::Object(props, rest) => {
                    for (k, p) in props {
                        self.key(k);
                        self.p(p)
                    }
                    if let Some(r) = rest {
                        self.p(r)
                    }
                }
                Pat::Array(elems, rest) => {
                    elems.iter().flatten().for_each(|p| self.p(p));
                    if let Some(r) = rest {
                        self.p(r)
                    }
                }
            }
        }
        fn key(&mut self, k: &Key) {
            if let Key::Computed(x) = k {
                self.e(x)
            }
        }
        fn func(&mut self, f: &Func) {
            f.params.iter().chain(f.rest.iter()).for_each(|p| self.p(p));
            self.body(&f.body)
        }
        fn class(&mut self, c: &Class) {
            if let Some(x) = &c.extends {
                self.e(x)
            }
            if let Some(f) = &c.ctor {
                self.func(f)
            }
            for m in &c.methods {
                self.key(&m.key);
                self.func(&m.func)
            }
            for (k, v, _) in &c.fields {
                self.key(k);
                if let Some(x) = v {
                    self.e(x)
                }
            }
            c.blocks.iter().for_each(|(_, x)| self.e(x));
        }
        fn body(&mut self, b: &[Stmt]) {
            b.iter().for_each(|s| self.s(s))
        }
        fn s(&mut self, s: &Stmt) {
            (self.on_stmt)(s);
            match s {
                Stmt::Expr(x) | Stmt::Throw(x) | Stmt::Return(Some(x)) => self.e(x),
                Stmt::Var(_, d) => d.iter().for_each(|(p, x)| {
                    self.p(p);
                    if let Some(x) = x {
                        self.e(x)
                    }
                }),
                Stmt::Func(f) => self.func(f),
                Stmt::If(c, a, b) => {
                    self.e(c);
                    self.s(a);
                    if let Some(b) = b {
                        self.s(b)
                    }
                }
                Stmt::Block(b) => self.body(b),
                Stmt::Labeled(_, b) => self.s(b),
                Stmt::Class(c) => self.class(c),
                Stmt::For(i, t, u, b) => {
                    if let Some(i) = i {
                        self.s(i)
                    }
                    t.iter().chain(u.iter()).for_each(|x| self.e(x));
                    self.s(b)
                }
                Stmt::ForOf(_, p, x, b) | Stmt::ForIn(_, p, x, b) => {
                    self.e(x);
                    self.p(p);
                    self.s(b)
                }
                Stmt::While(x, b) | Stmt::DoWhile(b, x) | Stmt::With(x, b) => {
                    self.e(x);
                    self.s(b)
                }
                Stmt::Switch(x, cs) => {
                    self.e(x);
                    cs.iter().for_each(|(t, b)| {
                        if let Some(t) = t {
                            self.e(t)
                        }
                        self.body(b)
                    })
                }
                Stmt::Try(a, h, f) => {
                    self.body(a);
                    if let Some((p, h)) = h {
                        if let Some(p) = p {
                            self.p(p)
                        }
                        self.body(h)
                    }
                    if let Some(f) = f {
                        self.body(f)
                    }
                }
                Stmt::Return(None)
                | Stmt::Break(_)
                | Stmt::Continue(_)
                | Stmt::Empty
                | Stmt::Unsupported(_) => {}
            }
        }
    }
    V { on_expr, on_stmt }.body(body)
}

/// Whether `body` reads `Qt.include` (to run files in its context).
pub fn uses_include(body: &[Stmt]) -> bool {
    let mut found = false;
    visit(
        body,
        &mut |x| {
            found |= matches!(x, Expr::Member(obj, name, false)
                if matches!(&**obj, Expr::Ident(q) if q == "Qt") && &**name == "include");
        },
        &mut |_| {},
    );
    found
}

/// The literal URLs `body` passes to `Qt.include` (the files a build compiles for it).
pub fn include_targets(body: &[Stmt]) -> Vec<String> {
    let mut out = Vec::new();
    visit(
        body,
        &mut |x| {
            if let Expr::Call(callee, args, _) = x
                && let Expr::Member(obj, name, false) = &**callee
                && matches!(&**obj, Expr::Ident(q) if q == "Qt")
                && &**name == "include"
                && let Some(Expr::Str(url)) = args.first()
            {
                out.push(url.to_string());
            }
        },
        &mut |_| {},
    );
    out
}

/// Kinds of the constructs of `body` compiled code does not run: unsupported nodes, generator
/// / async functions ([`Coroutine::name`]) and a `with` statement, whose object environment
/// exists in the interpreter only.
pub fn unsupported(body: &[Stmt], out: &mut Vec<String>) {
    let found = std::cell::RefCell::new(Vec::new());
    let coroutine = |f: &Func| {
        if let Some(c) = f.coroutine.filter(|c| !c.runs()) {
            found.borrow_mut().push(c.name().to_string())
        }
    };
    let class = |c: &Class| {
        c.ctor
            .iter()
            .chain(c.methods.iter().map(|m| &m.func))
            .for_each(coroutine)
    };
    visit(
        body,
        &mut |x| match x {
            Expr::Unsupported(k) => found.borrow_mut().push(k.clone()),
            Expr::Func(f) => coroutine(f),
            Expr::Class(c) => class(c),
            _ => {}
        },
        &mut |s| match s {
            Stmt::Unsupported(k) => found.borrow_mut().push(k.clone()),
            Stmt::With(..) => found.borrow_mut().push("with".into()),
            Stmt::Func(f) => coroutine(f),
            Stmt::Class(c) => class(c),
            _ => {}
        },
    );
    out.extend(found.into_inner());
}

/// Whether `body` reads `Qt.createQmlObject` (a call, `Qt["createQmlObject"]`, or the function
/// taken as a value): QML text compiled at run time.
pub fn reads_create_qml_object(body: &[Stmt]) -> bool {
    const NAME: &str = "createQmlObject";
    let mut hit = false;
    visit(
        body,
        &mut |x| {
            hit |= match x {
                Expr::Member(o, n, _) => {
                    matches!(&**o, Expr::Ident(q) if q == "Qt") && n.as_str() == NAME
                }
                Expr::Index(o, k, _) => {
                    matches!(&**o, Expr::Ident(q) if q == "Qt")
                        && matches!(&**k, Expr::Str(s) if &**s == NAME)
                }
                _ => false,
            }
        },
        &mut |_| {},
    );
    hit
}

/// A handler written as a function (`onX: function (a) { ... }`, `onX: (a) => ...`): QV4 calls
/// it with the signal's arguments, which bind to its own formals; the signal's parameter names
/// are not in its scope.
pub fn is_function_handler(body: &[Stmt]) -> bool {
    matches!(
        body,
        [Stmt::Expr(Expr::Func(_)) | Stmt::Return(Some(Expr::Func(_)))]
    )
}

/// Whether `body` can reach JavaScript text evaluated at run time, as far as its own text
/// shows:
/// - a read of the global `eval` (a call or the function taken as a value);
/// - a read of the global `Function` that can call it: `Function(...)`, `new Function(...)`,
///   the constructor taken as a value (`var F = Function`, an argument), `Function.call`,
///   `.apply`, `.bind` and `.constructor`. `x instanceof Function`, `typeof Function`, a
///   comparison with it and any other `Function.name` (`Function.prototype.call`) call
///   nothing;
/// - `.constructor` of a function written in place (`(function () {}).constructor`, an arrow,
///   a class) or of `Function.prototype`: that is `Function`.
///
/// A local binding named `eval` or `Function` resolves to a slot and is not one. The
/// constructor of a function held in a variable (`f.constructor(...)`) is not visible here:
/// calling it throws `EvalError` in a build without the interpreter (`builtins::function_ctor`).
pub fn reads_eval(body: &[Stmt]) -> bool {
    const CALLS: [&str; 4] = ["constructor", "call", "apply", "bind"];
    let function = |x: &Expr| matches!(x, Expr::Ident(n) if n == "Function");
    let function_valued = |x: &Expr| match x {
        Expr::Func(_) | Expr::Class(_) => true,
        Expr::Member(o, n, _) => function(o) && n.as_str() == "prototype",
        _ => false,
    };
    let mut hit = false;
    // Reads of `Function`, and how many of them sit where nothing can call it.
    let (mut reads, mut inert) = (0usize, 0usize);
    visit(
        body,
        &mut |x| match x {
            Expr::Ident(n) if n == "eval" => hit = true,
            Expr::Ident(n) if n == "Function" => reads += 1,
            Expr::Unary(UnOp::Typeof, a) => inert += usize::from(function(a)),
            Expr::Binary(BinOp::InstanceOf, _, b) => inert += usize::from(function(b)),
            Expr::Binary(BinOp::Eq | BinOp::Ne | BinOp::StrictEq | BinOp::StrictNe, a, b) => {
                inert += usize::from(function(a)) + usize::from(function(b));
            }
            Expr::Member(o, n, _) if function(o) => {
                inert += usize::from(!CALLS.contains(&n.as_str()));
            }
            Expr::Member(o, n, _) => hit |= n.as_str() == "constructor" && function_valued(o),
            Expr::Index(o, k, _) => {
                hit |= matches!(&**k, Expr::Str(s) if &**s == "constructor") && function_valued(o);
            }
            _ => {}
        },
        &mut |_| {},
    );
    hit || reads > inert
}

#[derive(Debug, Clone)]
enum Sc {
    /// A function call frame: names by slot, the slot count so far, the body's `const`
    /// names, the name of the function expression it declares ([`Func::expression`]),
    /// whether its code is strict, whether it is an arrow function, and whether a direct
    /// eval in it can declare a `var` when it runs ([`Func::direct_eval`] of non-strict code).
    Fun {
        slots: Vec<String>,
        n: u32,
        consts: Vec<String>,
        own: Option<String>,
        strict: bool,
        arrow: bool,
        eval: bool,
    },
    /// The object environment of a `with` statement: which names it binds is known only
    /// when the code runs.
    With,
    /// A block frame: each name is a slot of the enclosing function frame, or stays in
    /// `Frame::vars` (`None`), and whether it is immutable.
    Blk(Vec<(String, Option<u32>, bool)>),
}

/// The scopes around a call `eval(...)`: the code a direct eval runs is resolved in them.
#[derive(Debug)]
pub struct EvalScope {
    scopes: Vec<Sc>,
    /// The calling code is strict mode code.
    strict: bool,
    /// The block bindings of a generator that [`genlower`] made members of scope objects:
    /// the name, the number of `with` statements between the call and the block, and the
    /// member. They are bindings of the scopes up to `renamed_at`.
    renamed: Rc<Vec<(String, u32, Expr)>>,
    renamed_at: usize,
}

/// Code lowered by [`lower_direct_eval`].
pub struct EvalCode {
    pub body: Vec<Stmt>,
    pub strict: bool,
    /// The `var` and function names of non-strict code the calling function does not
    /// declare: EvalDeclarationInstantiation (ECMA-262 §19.2.1.3) creates them in it.
    pub vars: Vec<String>,
    /// The function declarations of non-strict code whose name is a slot of the calling
    /// function: the call frames up to it and the slot.
    pub slots: std::collections::HashMap<String, (u32, u32)>,
}

/// Whether a function or class is created anywhere in the statements: a closure could
/// capture a block's bindings, which then need a fresh frame per evaluation.
fn closure_s(s: &Stmt) -> bool {
    match s {
        Stmt::Func(_) | Stmt::Class(_) | Stmt::Unsupported(_) => true,
        Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Return(Some(e)) => closure_e(e),
        Stmt::Var(_, ds) => ds
            .iter()
            .any(|(p, e)| closure_p(p) || e.as_ref().is_some_and(closure_e)),
        Stmt::If(c, a, b) => closure_e(c) || closure_s(a) || b.as_deref().is_some_and(closure_s),
        Stmt::Block(b) => b.iter().any(closure_s),
        Stmt::For(i, t, u, b) => {
            i.as_deref().is_some_and(closure_s)
                || t.as_ref().is_some_and(closure_e)
                || u.as_ref().is_some_and(closure_e)
                || closure_s(b)
        }
        Stmt::ForOf(_, p, e, b) | Stmt::ForIn(_, p, e, b) => {
            closure_p(p) || closure_e(e) || closure_s(b)
        }
        Stmt::While(e, b) | Stmt::DoWhile(b, e) | Stmt::With(e, b) => closure_e(e) || closure_s(b),
        Stmt::Labeled(_, b) => closure_s(b),
        Stmt::Switch(d, cs) => {
            closure_e(d)
                || cs
                    .iter()
                    .any(|(t, b)| t.as_ref().is_some_and(closure_e) || b.iter().any(closure_s))
        }
        Stmt::Try(a, c, f) => {
            a.iter().any(closure_s)
                || c.as_ref().is_some_and(|(p, b)| {
                    p.as_ref().is_some_and(closure_p) || b.iter().any(closure_s)
                })
                || f.iter().flatten().any(closure_s)
        }
        Stmt::Return(None) | Stmt::Break(_) | Stmt::Continue(_) | Stmt::Empty => false,
    }
}

fn closure_e(e: &Expr) -> bool {
    match e {
        Expr::Func(_) | Expr::Class(_) | Expr::Unsupported(_) | Expr::DirectEval(..) => true,
        Expr::Template(_, v) | Expr::Array(v) | Expr::Seq(v) => v.iter().any(closure_e),
        Expr::Member(a, ..) | Expr::Chain(a) | Expr::Unary(_, a) | Expr::Spread(a) => closure_e(a),
        Expr::Index(a, b, _)
        | Expr::DefineField(a, b, _)
        | Expr::Binary(_, a, b)
        | Expr::Logical(_, a, b) => closure_e(a) || closure_e(b),
        // The code a call `eval(...)` runs can make a closure too.
        Expr::Call(a, _, false) if matches!(&**a, Expr::Ident(n) if n == "eval") => true,
        Expr::Call(a, v, _) | Expr::New(a, v) => closure_e(a) || v.iter().any(closure_e),
        Expr::Cond(a, b, c) => closure_e(a) || closure_e(b) || closure_e(c),
        Expr::Yield(a, _) => a.as_deref().is_some_and(closure_e),
        Expr::Assign(_, p, b) => closure_p(p) || closure_e(b),
        Expr::Update(_, _, p) => closure_p(p),
        Expr::Object(v) => v
            .iter()
            .any(|(k, x, _)| matches!(k, Key::Computed(k) if closure_e(k)) || closure_e(x)),
        Expr::Num(_)
        | Expr::Str(_)
        | Expr::Str16(_)
        | Expr::Bool(_)
        | Expr::Null
        | Expr::Ident(_)
        | Expr::Local(..)
        | Expr::Dynamic(..)
        | Expr::This
        | Expr::Regex(..)
        | Expr::TemplateObject(_)
        | Expr::Hole
        | Expr::Super
        | Expr::NewTarget => false,
    }
}

fn closure_p(p: &Pat) -> bool {
    match p {
        Pat::Id(_) | Pat::Local(..) => false,
        Pat::Object(ps, rest) => {
            ps.iter()
                .any(|(k, p)| matches!(k, Key::Computed(k) if closure_e(k)) || closure_p(p))
                || rest.as_deref().is_some_and(closure_p)
        }
        Pat::Array(ps, rest) => {
            ps.iter().flatten().any(closure_p) || rest.as_deref().is_some_and(closure_p)
        }
        Pat::Default(p, e) => closure_p(p) || closure_e(e),
        Pat::Member(e) | Pat::Init(e) => closure_e(e),
        Pat::Const(p, _) | Pat::Dynamic(_, _, p) => closure_p(p),
    }
}

fn pat_ids(p: &Pat, out: &mut Vec<String>) {
    match p {
        Pat::Id(n) | Pat::Local(_, _, n) => out.push(n.clone()),
        Pat::Object(ps, rest) => {
            ps.iter().for_each(|(_, p)| pat_ids(p, out));
            rest.iter().for_each(|p| pat_ids(p, out));
        }
        Pat::Array(ps, rest) => {
            ps.iter().flatten().for_each(|p| pat_ids(p, out));
            rest.iter().for_each(|p| pat_ids(p, out));
        }
        Pat::Default(p, _) => pat_ids(p, out),
        Pat::Const(p, _) | Pat::Dynamic(_, _, p) => pat_ids(p, out),
        Pat::Member(_) | Pat::Init(_) => {}
    }
}

/// `var` names declared anywhere in `body` outside nested functions.
fn var_names(body: &[Stmt], out: &mut Vec<String>) {
    for s in body {
        match s {
            Stmt::Var("var", ds) => ds.iter().for_each(|(p, _)| pat_ids(p, out)),
            Stmt::If(_, a, b) => {
                var_names(std::slice::from_ref(a), out);
                b.iter()
                    .for_each(|b| var_names(std::slice::from_ref(b), out));
            }
            Stmt::Block(b) => var_names(b, out),
            Stmt::For(i, _, _, b) => {
                i.iter()
                    .for_each(|s| var_names(std::slice::from_ref(s), out));
                var_names(std::slice::from_ref(b), out);
            }
            Stmt::ForOf(k, p, _, b) | Stmt::ForIn(k, p, _, b) => {
                if *k == Some("var") {
                    pat_ids(p, out);
                }
                var_names(std::slice::from_ref(b), out);
            }
            Stmt::While(_, b) | Stmt::DoWhile(b, _) | Stmt::Labeled(_, b) | Stmt::With(_, b) => {
                var_names(std::slice::from_ref(b), out)
            }
            Stmt::Switch(_, cs) => cs.iter().for_each(|(_, b)| var_names(b, out)),
            Stmt::Try(a, c, f) => {
                var_names(a, out);
                c.iter().for_each(|(_, b)| var_names(b, out));
                f.iter().for_each(|b| var_names(b, out));
            }
            _ => {}
        }
    }
}

/// Names a block body declares in its own frame (`let`/`const`/function/class).
fn lexical_names(body: &[Stmt], out: &mut Vec<String>) {
    for s in body {
        match s {
            Stmt::Var(k, ds) if *k != "var" => ds.iter().for_each(|(p, _)| pat_ids(p, out)),
            Stmt::Func(f) => out.extend(f.name.clone()),
            Stmt::Class(c) => out.extend(c.name.clone()),
            _ => {}
        }
    }
}

/// [`lexical_names`] of `body`, each with whether it is a `const` binding.
fn lexical_decls(body: &[Stmt]) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    for s in body {
        let mut ns = Vec::new();
        lexical_names(std::slice::from_ref(s), &mut ns);
        let c = matches!(s, Stmt::Var("const", _));
        out.extend(ns.into_iter().map(|n| (n, c)));
    }
    out
}

/// `e` reads nothing but `this`, literals, non-optional members/indexes and binary operators.
pub fn this_only(e: &Expr) -> bool {
    match e {
        Expr::This | Expr::Num(_) | Expr::Str(_) | Expr::Str16(_) => true,
        Expr::Member(o, _, false) => this_only(o),
        Expr::Index(o, k, false) | Expr::Binary(_, o, k) => this_only(o) && this_only(k),
        _ => false,
    }
}

/// Resolve identifiers bound in function frames (parameters, `var`s and the function body's own
/// declarations) to frame slots. It mirrors the frames the evaluator makes: every call makes
/// one slot frame, every other frame keeps its names by string, and a name declared in such a
/// frame between the use and the function shadows it, so that use stays a name lookup.
/// Top-level code (QML bindings, libraries) keeps its names.
pub fn resolve(body: &mut [Stmt]) {
    let mut r = Resolver {
        scopes: Vec::new(),
        base: 0,
        strict: false,
        renamed: Default::default(),
        renamed_at: 0,
    };
    // Top-level names stay names: the scope only records which of them are immutable.
    r.scoped(lexical_decls(body), false, |r| r.stmts(body));
}

struct Resolver {
    scopes: Vec<Sc>,
    /// The scopes below this index are those of the code calling a direct eval: the
    /// evaluated code gets no slots in their frames.
    base: usize,
    /// The top-level code being resolved is strict mode code.
    strict: bool,
    /// [`EvalScope::renamed`] of the code calling a direct eval.
    renamed: Rc<Vec<(String, u32, Expr)>>,
    renamed_at: usize,
}

impl Resolver {
    /// The slot the code shows `n` bound to (`None`: a name), and the scopes between the use
    /// and that binding which can bind `n` when the code runs: the number of `with` objects
    /// (`None` when there is no such scope, nor a function a direct eval can declare a
    /// `var` in). The engine's own names (`%...`) are no identifiers: nothing rebinds them.
    fn find(&self, n: &str) -> (Option<(u32, u32)>, Option<u32>) {
        let mut hops = 0;
        let mut withs = 0;
        let mut evals = false;
        let dynamic = |withs: u32, evals: bool| {
            ((withs > 0 || evals) && !n.starts_with('%')).then_some(withs)
        };
        for s in self.scopes.iter().rev() {
            match s {
                Sc::Blk(ns) => {
                    if let Some((_, slot, _)) = ns.iter().find(|x| x.0 == n) {
                        return (slot.map(|i| (hops, i)), dynamic(withs, evals));
                    }
                }
                Sc::With => withs += 1,
                Sc::Fun {
                    slots, arrow, eval, ..
                } => {
                    if let Some(i) = slots.iter().position(|x| x == n) {
                        return (Some((hops, i as u32)), dynamic(withs, evals));
                    }
                    // A function's own `arguments` stays a name (its arguments object).
                    if n == "arguments" && !arrow {
                        return (None, dynamic(withs, evals));
                    }
                    evals |= *eval;
                    hops += 1;
                }
            }
        }
        (None, dynamic(withs, evals))
    }

    /// The member that stands for `n` when it names a generator's block binding of the code
    /// calling a direct eval (resolved), and the number of `with` objects asked first.
    fn renamed(&self, n: &str) -> Option<(Expr, u32)> {
        let (_, d, x) = self.renamed.iter().rev().find(|r| r.0 == n)?;
        let mut withs = *d;
        for s in &self.scopes[self.renamed_at..] {
            match s {
                Sc::Blk(ns) if ns.iter().any(|x| x.0 == n) => return None,
                Sc::Fun { slots, arrow, .. }
                    if slots.iter().any(|x| x == n) || (n == "arguments" && !arrow) =>
                {
                    return None;
                }
                Sc::With => withs += 1,
                Sc::Blk(_) | Sc::Fun { .. } => {}
            }
        }
        let mut x = x.clone();
        if let Expr::Member(o, ..) = &mut x
            && let Expr::Ident(s) = &**o
            && let (Some((h, i)), _) = self.find(s)
        {
            **o = Expr::Local(h, i, s.clone());
        }
        Some((x, withs))
    }

    /// The binding of `n` in the function a direct eval is called in, as that function's
    /// code shows it: its slot, or `None` for a name. Object environments are not asked
    /// (ECMA-262 §19.2.1.3 declares in the variable environment).
    fn var_binding(&self, n: &str) -> Option<Option<(u32, u32)>> {
        let mut hops = 0;
        for s in self.scopes.iter().rev() {
            match s {
                Sc::Blk(ns) => {
                    if let Some((_, slot, _)) = ns.iter().find(|x| x.0 == n) {
                        return Some(slot.map(|i| (hops, i)));
                    }
                }
                Sc::With => {}
                // A generator's step function runs its generator function's code.
                Sc::Fun { slots, arrow, .. }
                    if *arrow && slots.first().is_some_and(|s| s == genlower::CTX) =>
                {
                    hops += 1;
                }
                Sc::Fun { slots, .. } => {
                    let i = slots.iter().position(|x| x == n)?;
                    return Some(Some((hops, i as u32)));
                }
            }
        }
        None
    }

    /// Whether the nearest declaration of `n` is an immutable binding, and if so whether a
    /// write to it here throws: a `const` or class name always, the own name of a function
    /// expression (a non-strict immutable binding) in strict code only.
    fn immutable(&self, n: &str) -> Option<bool> {
        for s in self.scopes.iter().rev() {
            match s {
                Sc::Blk(ns) => {
                    if let Some(x) = ns.iter().find(|x| x.0 == n) {
                        return x.2.then_some(true);
                    }
                }
                Sc::With => {}
                Sc::Fun {
                    slots, consts, own, ..
                } => {
                    if slots.iter().any(|x| x == n) {
                        if consts.iter().any(|x| x == n) {
                            return Some(true);
                        }
                        return (own.as_deref() == Some(n)).then(|| self.strict());
                    }
                }
            }
        }
        None
    }

    /// The code being resolved is strict mode code.
    fn strict(&self) -> bool {
        self.scopes[self.base..]
            .iter()
            .rev()
            .find_map(|s| match s {
                Sc::Fun { strict, .. } => Some(*strict),
                Sc::Blk(_) | Sc::With => None,
            })
            .unwrap_or(self.strict)
    }

    fn func(&mut self, f: &mut Func) {
        // With parameter expressions the body's declarations get an environment of their own
        // (ECMA-262 §10.2.11 step 28): a block scope over the formals' frame slots.
        let split = f.param_exprs();
        let mut formals = Vec::new();
        f.params.iter().for_each(|p| pat_ids(p, &mut formals));
        f.rest.iter().for_each(|p| pat_ids(p, &mut formals));
        let mut vars = Vec::new();
        var_names(&f.body, &mut vars);
        let mut names = formals.clone();
        let mut body: Vec<String> = Vec::new();
        if split {
            lexical_names(&f.body, &mut body);
            body.splice(0..0, vars.iter().cloned());
            let mut seen = std::collections::HashSet::new();
            body.retain(|n| seen.insert(n.clone()));
        } else {
            names.extend(vars.iter().cloned());
            lexical_names(&f.body, &mut names);
        }
        let mut slots: Vec<String> = Vec::new();
        for n in names {
            if n != "arguments" && !slots.contains(&n) {
                slots.push(n);
            }
        }
        let nformals = slots.len() as u32;
        let pos = |n: &str| match split {
            true => body
                .iter()
                .position(|x| x == n)
                .map(|i| nformals + i as u32),
            false => slots.iter().position(|x| x == n).map(|i| i as u32),
        };
        let mut own = None;
        let mut funcs = Vec::new();
        for s in f.body.iter_mut() {
            match s {
                Stmt::Func(g) => {
                    if g.expression {
                        own.clone_from(&g.name);
                    }
                    if let Some(n) = &g.name {
                        funcs.push(n.clone());
                        let p = pos(n);
                        Rc::make_mut(g).slot = p;
                    }
                }
                Stmt::Class(c) => {
                    if let Some(n) = &c.name {
                        c.slot = pos(n);
                    }
                }
                _ => {}
            }
        }
        // Step 28.f: a `var` named like a formal, or `arguments` where that names the
        // arguments object, starts as that binding's value; any other starts undefined.
        let arguments =
            formals.iter().any(|n| n == "arguments") || (!f.is_arrow() && f.uses_arguments);
        f.var_init = match split {
            true => body
                .iter()
                .enumerate()
                .filter(|(_, n)| vars.contains(n) && !funcs.contains(n))
                .filter_map(|(i, n)| {
                    let init = match slots.iter().position(|x| x == n) {
                        Some(j) => Expr::Local(0, j as u32, n.clone()),
                        None if n == "arguments" && arguments => Expr::Ident(n.clone()),
                        None => return None,
                    };
                    Some((init, nformals + i as u32))
                })
                .collect(),
            false => Vec::new(),
        };
        f.hoisted = std::cell::OnceCell::from(
            vars.into_iter()
                .filter(|n| !split && n == "arguments")
                .take(1)
                .collect::<Vec<_>>(),
        );
        let lexical = lexical_decls(&f.body);
        let consts: Vec<String> = lexical
            .iter()
            .filter(|(_, c)| *c)
            .map(|(n, _)| n.clone())
            .collect();
        // An arrow function's code is strict inside strict code (ECMA-262 §11.2.2).
        let strict = f.strict || (f.is_arrow() && self.strict());
        let n = nformals + body.len() as u32;
        self.scopes.push(Sc::Fun {
            slots,
            n,
            consts: if split { Vec::new() } else { consts.clone() },
            own,
            strict,
            arrow: f.is_arrow(),
            eval: f.direct_eval && !strict,
        });
        f.params.iter_mut().for_each(|p| self.pat(p));
        if let Some(p) = &mut f.rest {
            self.pat(p);
        }
        if split {
            let scope = body
                .iter()
                .enumerate()
                .map(|(i, x)| (x.clone(), Some(nformals + i as u32), consts.contains(x)))
                .collect();
            self.scopes.push(Sc::Blk(scope));
            self.stmts(&mut f.body);
            self.scopes.pop();
        } else {
            self.stmts(&mut f.body);
        }
        if let Some(Sc::Fun { n, .. }) = self.scopes.pop() {
            f.nslots = n;
        }
        f.this_only = !f.is_arrow()
            && f.coroutine.is_none()
            && !f.uses_arguments
            && f.params.is_empty()
            && f.rest.is_none()
            && matches!(&f.body[..], [Stmt::Return(Some(e))] if this_only(e));
    }

    /// A block scope declaring `names`. Inside a function, when nothing in the block can
    /// capture them (`slots`), they become fresh slots of the function frame.
    fn scoped(&mut self, names: Vec<(String, bool)>, slots: bool, f: impl FnOnce(&mut Self)) {
        let fun = self.scopes[self.base..]
            .iter_mut()
            .rev()
            .find_map(|s| match s {
                Sc::Fun { n, .. } if slots => Some(n),
                _ => None,
            });
        let names = match fun {
            Some(n) => names
                .into_iter()
                .map(|(x, c)| {
                    *n += 1;
                    (x, Some(*n - 1), c)
                })
                .collect(),
            None => names.into_iter().map(|(x, c)| (x, None, c)).collect(),
        };
        self.scopes.push(Sc::Blk(names));
        f(self);
        self.scopes.pop();
    }

    fn block(&mut self, body: &mut [Stmt], extra: Vec<String>, slots: bool) {
        let mut names: Vec<(String, bool)> = extra.into_iter().map(|n| (n, false)).collect();
        names.extend(lexical_decls(body));
        let slots = slots && !body.iter().any(closure_s);
        self.scoped(names, slots, |r| r.stmts(body));
    }

    fn stmts(&mut self, body: &mut [Stmt]) {
        body.iter_mut().for_each(|s| self.stmt(s));
    }

    fn stmt(&mut self, s: &mut Stmt) {
        match s {
            Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Return(Some(e)) => self.expr(e),
            Stmt::Var(_, ds) => {
                for (p, init) in ds {
                    if let Some(e) = init {
                        self.expr(e);
                    }
                    self.pat(p);
                }
            }
            Stmt::Func(f) => self.func(Rc::make_mut(f)),
            Stmt::If(c, a, b) => {
                self.expr(c);
                self.stmt(a);
                if let Some(b) = b {
                    self.stmt(b);
                }
            }
            Stmt::Block(b) => self.block(b, Vec::new(), true),
            Stmt::For(init, t, u, b) => {
                let names = init
                    .as_deref()
                    .map(|i| lexical_decls(std::slice::from_ref(i)))
                    .unwrap_or_default();
                let slots = !(init.as_deref().is_some_and(closure_s)
                    || t.as_ref().is_some_and(closure_e)
                    || u.as_ref().is_some_and(closure_e)
                    || closure_s(b));
                self.scoped(names, slots, |r| {
                    if let Some(i) = init {
                        r.stmt(i);
                    }
                    t.iter_mut().chain(u.iter_mut()).for_each(|e| r.expr(e));
                    r.stmt(b);
                });
            }
            Stmt::ForOf(k, p, e, b) | Stmt::ForIn(k, p, e, b) => {
                self.expr(e);
                // `let`/`const` bind in a scope of their own; `var` names are the function's.
                if k.is_some_and(|k| k != "var") {
                    let mut names = Vec::new();
                    pat_ids(p, &mut names);
                    let c = *k == Some("const");
                    let names = names.into_iter().map(|n| (n, c)).collect();
                    let slots = !closure_p(p) && !closure_s(b);
                    self.scoped(names, slots, |r| {
                        r.pat(p);
                        r.stmt(b);
                    });
                } else {
                    self.target(p);
                    self.stmt(b);
                }
            }
            Stmt::While(e, b) | Stmt::DoWhile(b, e) => {
                self.expr(e);
                self.stmt(b);
            }
            Stmt::Labeled(_, b) => self.stmt(b),
            Stmt::With(e, b) => {
                self.expr(e);
                self.scopes.push(Sc::With);
                self.stmt(b);
                self.scopes.pop();
            }
            Stmt::Switch(d, cs) => {
                self.expr(d);
                let names = cs.iter().flat_map(|(_, b)| lexical_decls(b)).collect();
                let slots = !cs
                    .iter()
                    .any(|(t, b)| t.as_ref().is_some_and(closure_e) || b.iter().any(closure_s));
                self.scoped(names, slots, |r| {
                    for (t, b) in cs {
                        if let Some(t) = t {
                            r.expr(t);
                        }
                        r.stmts(b);
                    }
                });
            }
            Stmt::Class(c) => self.class(c),
            Stmt::Try(a, c, f) => {
                self.block(a, Vec::new(), true);
                if let Some((p, b)) = c {
                    let mut ids = Vec::new();
                    p.iter().for_each(|p| pat_ids(p, &mut ids));
                    let mut names: Vec<(String, bool)> =
                        ids.into_iter().map(|n| (n, false)).collect();
                    names.extend(lexical_decls(b));
                    let slots = !p.as_ref().is_some_and(closure_p) && !b.iter().any(closure_s);
                    self.scoped(names, slots, |r| {
                        if let Some(p) = p {
                            r.pat(p);
                        }
                        r.stmts(b);
                    });
                }
                if let Some(f) = f {
                    self.block(f, Vec::new(), true);
                }
            }
            Stmt::Return(None)
            | Stmt::Break(_)
            | Stmt::Continue(_)
            | Stmt::Empty
            | Stmt::Unsupported(_) => {}
        }
    }

    fn class(&mut self, c: &mut Class) {
        if let Some(e) = &mut c.extends {
            self.expr(e);
        }
        // Inside its body a class's own name is an immutable binding (ECMA-262 §15.7.14).
        self.scoped(
            c.name.iter().map(|n| (n.clone(), true)).collect(),
            false,
            |r| {
                if let Some(f) = &mut c.ctor {
                    r.func(f);
                }
                for m in &mut c.methods {
                    r.key(&mut m.key);
                    r.func(&mut m.func);
                }
                for (k, v, _) in &mut c.fields {
                    r.key(k);
                    if let Some(v) = v {
                        r.expr(v);
                    }
                }
                c.blocks.iter_mut().for_each(|(_, x)| r.expr(x));
            },
        );
    }

    fn key(&mut self, k: &mut Key) {
        if let Key::Computed(e) = k {
            self.expr(e);
        }
    }

    fn pat(&mut self, p: &mut Pat) {
        match p {
            Pat::Id(n) if self.renamed(n).is_some() => {
                let (name, (x, withs)) = (n.clone(), self.renamed(n).unwrap());
                *p = Pat::Member(bx(x));
                if withs > 0 {
                    let b = std::mem::replace(p, Pat::Id(String::new()));
                    *p = Pat::Dynamic(name, withs, bx(b));
                }
            }
            Pat::Id(n) => {
                let (slot, dynamic) = self.find(n);
                let name = n.clone();
                if let Some((h, i)) = slot {
                    *p = Pat::Local(h, i, std::mem::take(n));
                }
                if let Some(withs) = dynamic {
                    let b = std::mem::replace(p, Pat::Id(String::new()));
                    *p = Pat::Dynamic(name, withs, bx(b));
                }
            }
            Pat::Local(..) => {}
            // A generator's binding in a scope object, used inside `with` statements.
            Pat::Dynamic(_, _, q) => self.pat(q),
            Pat::Object(ps, rest) => {
                for (k, p) in ps {
                    self.key(k);
                    self.pat(p);
                }
                if let Some(r) = rest {
                    self.pat(r);
                }
            }
            Pat::Array(ps, rest) => {
                ps.iter_mut().flatten().for_each(|p| self.pat(p));
                if let Some(r) = rest {
                    self.pat(r);
                }
            }
            Pat::Default(p, e) => {
                self.pat(p);
                self.expr(e);
            }
            Pat::Member(e) | Pat::Init(e) => self.expr(e),
            Pat::Const(..) => {}
        }
    }

    /// An assignment target: [`Self::pat`], marking each name bound immutably.
    fn target(&mut self, p: &mut Pat) {
        match p {
            Pat::Id(n) => {
                let c = self.immutable(n);
                self.pat(p);
                if let Some(throws) = c {
                    // The immutable binding is what a dynamic target falls back to.
                    let q = match p {
                        Pat::Dynamic(_, _, q) => &mut **q,
                        q => q,
                    };
                    let b = std::mem::replace(q, Pat::Id(String::new()));
                    *q = Pat::Const(bx(b), throws);
                }
            }
            Pat::Object(ps, rest) => {
                for (k, p) in ps {
                    self.key(k);
                    self.target(p);
                }
                if let Some(r) = rest {
                    self.target(r);
                }
            }
            Pat::Array(ps, rest) => {
                ps.iter_mut().flatten().for_each(|p| self.target(p));
                if let Some(r) = rest {
                    self.target(r);
                }
            }
            Pat::Default(p, e) => {
                self.target(p);
                self.expr(e);
            }
            _ => self.pat(p),
        }
    }

    fn expr(&mut self, e: &mut Expr) {
        if let Expr::Call(a, v, false) = e
            && matches!(&**a, Expr::Ident(n) if n == "eval")
        {
            self.expr(a);
            v.iter_mut().for_each(|x| self.expr(x));
            let scope = Rc::new(EvalScope {
                scopes: self.scopes.clone(),
                strict: self.strict(),
                renamed: self.renamed.clone(),
                renamed_at: self.renamed_at,
            });
            let call = std::mem::replace(e, Expr::Null);
            *e = Expr::DirectEval(scope, bx(call));
            return;
        }
        match e {
            // A call of a generator's block binding keeps `this` undefined, as in
            // [`genlower`]: `(0, scope.f)()`.
            Expr::Call(a, v, _) if matches!(&**a, Expr::Ident(n) if self.renamed(n).is_some()) => {
                let Expr::Ident(n) = &**a else { unreachable!() };
                let (name, (x, withs)) = (n.clone(), self.renamed(n).unwrap());
                **a = Expr::Seq(vec![Expr::Num(0.0), x]);
                if withs > 0 {
                    let b = std::mem::replace(&mut **a, Expr::Null);
                    **a = Expr::Dynamic(name, withs, bx(b));
                }
                v.iter_mut().for_each(|x| self.expr(x));
            }
            Expr::Ident(n) if self.renamed(n).is_some() => {
                let (name, (x, withs)) = (n.clone(), self.renamed(n).unwrap());
                *e = match withs {
                    0 => x,
                    _ => Expr::Dynamic(name, withs, bx(x)),
                };
            }
            // A direct eval in a generator, which [`genlower`] gave the block bindings.
            Expr::DirectEval(scope, call) if scope.scopes.is_empty() => {
                if let Expr::Call(a, v, _) = &mut **call {
                    self.expr(a);
                    v.iter_mut().for_each(|x| self.expr(x));
                }
                *scope = Rc::new(EvalScope {
                    scopes: self.scopes.clone(),
                    strict: self.strict(),
                    renamed: scope.renamed.clone(),
                    renamed_at: self.scopes.len(),
                });
            }
            Expr::Ident(n) => {
                let (slot, dynamic) = self.find(n);
                let name = n.clone();
                if let Some((h, i)) = slot {
                    *e = Expr::Local(h, i, std::mem::take(n));
                }
                if let Some(withs) = dynamic {
                    let b = std::mem::replace(e, Expr::Null);
                    *e = Expr::Dynamic(name, withs, bx(b));
                }
            }
            // A generator's binding in a scope object, used inside `with` statements.
            Expr::Dynamic(_, _, x) => self.expr(x),
            Expr::Template(_, v) | Expr::Array(v) | Expr::Seq(v) => {
                v.iter_mut().for_each(|x| self.expr(x))
            }
            Expr::Member(a, ..) | Expr::Chain(a) | Expr::Unary(_, a) | Expr::Spread(a) => {
                self.expr(a)
            }
            Expr::Index(a, b, _)
            | Expr::DefineField(a, b, _)
            | Expr::Binary(_, a, b)
            | Expr::Logical(_, a, b) => {
                self.expr(a);
                self.expr(b);
            }
            Expr::Call(a, v, _) | Expr::New(a, v) => {
                self.expr(a);
                v.iter_mut().for_each(|x| self.expr(x));
            }
            Expr::Cond(a, b, c) => {
                self.expr(a);
                self.expr(b);
                self.expr(c);
            }
            Expr::Assign(_, p, b) => {
                self.target(p);
                self.expr(b);
            }
            Expr::Update(_, _, p) => self.target(p),
            Expr::Yield(a, _) => {
                if let Some(a) = a {
                    self.expr(a)
                }
            }
            Expr::Object(v) => v.iter_mut().for_each(|(k, x, _)| {
                self.key(k);
                self.expr(x);
            }),
            Expr::Func(f) => self.func(Rc::make_mut(f)),
            Expr::Class(c) => self.class(c),
            Expr::Num(_)
            | Expr::Str(_)
            | Expr::Str16(_)
            | Expr::Bool(_)
            | Expr::Null
            | Expr::Local(..)
            | Expr::DirectEval(..)
            | Expr::This
            | Expr::Regex(..)
            | Expr::TemplateObject(_)
            | Expr::Hole
            | Expr::Super
            | Expr::NewTarget
            | Expr::Unsupported(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    //! Lowering checked against ECMA-262 as QV4 applies it. The IR is rendered as an
    //! s-expression (`(op a b)`, `(call f x)`, `name@hops.slot` for a resolved local) so a test
    //! states the tree it expects; where a script can run, `ev` checks the value too.
    use super::*;
    use crate::eval::{Ex, Interp};

    fn sx(head: &str, items: impl IntoIterator<Item = String>) -> String {
        let mut out = format!("({head}");
        for i in items {
            out.push(' ');
            out.push_str(&i);
        }
        out.push(')');
        out
    }

    fn key_s(k: &Key) -> String {
        match k {
            Key::Name(n) => n.escape_debug().to_string(),
            Key::Computed(x) => format!("[{}]", e(x)),
        }
    }

    fn e(x: &Expr) -> String {
        match x {
            Expr::Num(n) => format!("{n}"),
            Expr::Str(s) => format!("{s:?}"),
            Expr::Str16(u) => sx("str16", u.iter().map(|c| format!("{c}"))),
            Expr::Bool(b) => b.to_string(),
            Expr::Null => "null".into(),
            Expr::Ident(n) => n.clone(),
            Expr::Local(h, i, n) => format!("{n}@{h}.{i}"),
            Expr::Dynamic(_, w, x) => format!("{}?{w}", e(x)),
            Expr::DirectEval(_, c) => sx("direct", [e(c)]),
            Expr::This => "this".into(),
            Expr::Regex(p, f) => format!("/{p}/{f}"),
            Expr::Template(q, v) => {
                let mut parts = vec![format!("{:?}", q[0])];
                for (x, s) in v.iter().zip(&q[1..]) {
                    parts.push(e(x));
                    parts.push(format!("{s:?}"));
                }
                sx("tpl", parts)
            }
            Expr::TemplateObject(t) => {
                let q = |s: &Option<String>| {
                    s.as_ref().map_or("undefined".into(), |s| format!("{s:?}"))
                };
                let cooked = t.cooked.iter().map(q);
                let raw = t.raw.iter().map(|s| format!("{s:?}"));
                sx("tplobj", cooked.chain(std::iter::once(sx("raw", raw))))
            }
            Expr::Member(o, n, opt) => sx(if *opt { "?." } else { "." }, [e(o), n.as_str().into()]),
            Expr::Index(o, k, opt) => sx(if *opt { "?[]" } else { "[]" }, [e(o), e(k)]),
            Expr::Call(c, a, opt) => sx(
                if *opt { "?call" } else { "call" },
                std::iter::once(e(c)).chain(a.iter().map(e)),
            ),
            Expr::New(c, a) => sx("new", std::iter::once(e(c)).chain(a.iter().map(e))),
            Expr::Chain(x) => sx("chain", [e(x)]),
            Expr::Unary(op, a) => sx(un_s(*op), [e(a)]),
            Expr::Update(op, pre, t) => sx(
                &format!("{}{}", if *pre { "pre" } else { "post" }, upd_s(*op)),
                [p(t)],
            ),
            Expr::Binary(op, a, b) => sx(op.as_str(), [e(a), e(b)]),
            Expr::Logical(op, a, b) => sx(&format!("L{}", log_s(*op)), [e(a), e(b)]),
            Expr::Cond(c, a, b) => sx("?", [e(c), e(a), e(b)]),
            Expr::Assign(op, t, v) => sx(&asg_s(*op), [p(t), e(v)]),
            Expr::Array(v) => format!("[{}]", v.iter().map(e).collect::<Vec<_>>().join(", ")),
            Expr::Object(v) => format!(
                "{{{}}}",
                v.iter()
                    .map(|(k, x, kind)| match (k, x, kind) {
                        (Key::Name(n), Expr::Spread(_), _) if &**n == "..." => e(x),
                        (_, _, PropKind::Init) => format!("{}: {}", key_s(k), e(x)),
                        (_, _, PropKind::Proto) => format!("__proto__ {}: {}", key_s(k), e(x)),
                        (_, _, PropKind::Get) => format!("get {}: {}", key_s(k), e(x)),
                        (_, _, PropKind::Set) => format!("set {}: {}", key_s(k), e(x)),
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Expr::Spread(x) => format!("...{}", e(x)),
            Expr::Hole => "_".into(),
            Expr::Seq(v) => sx("seq", v.iter().map(e)),
            Expr::Func(f) => fun(f),
            Expr::Class(c) => class_s(c),
            Expr::Super => "super".into(),
            Expr::NewTarget => "new.target".into(),
            Expr::DefineField(k, v, n) => {
                sx(if *n { "field=named" } else { "field=" }, [e(k), e(v)])
            }
            Expr::Yield(a, d) => sx(if *d { "yield*" } else { "yield" }, a.iter().map(|a| e(a))),
            Expr::Unsupported(k) => sx("unsupported", [k.clone()]),
        }
    }

    fn p(x: &Pat) -> String {
        match x {
            Pat::Id(n) => n.clone(),
            Pat::Local(h, i, n) => format!("{n}@{h}.{i}"),
            Pat::Dynamic(_, w, x) => format!("{}?{w}", p(x)),
            Pat::Const(x, true) => format!("(const {})", p(x)),
            Pat::Const(x, false) => format!("(fnname {})", p(x)),
            Pat::Object(ps, rest) => format!(
                "{{{}}}",
                ps.iter()
                    .map(|(k, v)| format!("{}: {}", key_s(k), p(v)))
                    .chain(rest.iter().map(|r| format!("...{}", p(r))))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Pat::Array(ps, rest) => format!(
                "[{}]",
                ps.iter()
                    .map(|v| v.as_ref().map_or("_".into(), p))
                    .chain(rest.iter().map(|r| format!("...{}", p(r))))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Pat::Default(t, v) => format!("{}={}", p(t), e(v)),
            Pat::Member(x) => e(x),
            Pat::Init(x) => format!("(init {})", e(x)),
        }
    }

    fn block(b: &[Stmt]) -> String {
        format!("{{{}}}", b.iter().map(s).collect::<Vec<_>>().join("; "))
    }

    fn fun(f: &Func) -> String {
        let params = f
            .params
            .iter()
            .map(p)
            .chain(f.rest.iter().map(|r| format!("...{}", p(r))));
        let params = format!("({})", params.collect::<Vec<_>>().join(" "));
        match (&f.name, f.is_arrow()) {
            (_, true) => sx("=>", [params, block(&f.body)]),
            (Some(n), false) => sx("function", [n.clone(), params, block(&f.body)]),
            (None, false) => sx("function", [params, block(&f.body)]),
        }
    }

    fn class_s(c: &Class) -> String {
        let mut items: Vec<String> = c.name.iter().cloned().collect();
        items.extend(c.extends.iter().map(|x| sx("extends", [e(x)])));
        items.extend(c.ctor.iter().map(|f| sx("ctor", [fun(f)])));
        for m in &c.methods {
            let head = format!("{}{}", if m.is_static { "static-" } else { "" }, m.kind);
            items.push(sx(&head, [key_s(&m.key), fun(&m.func)]));
        }
        for (k, v, st) in &c.fields {
            let head = if *st { "static-field" } else { "field" };
            items.push(sx(head, std::iter::once(key_s(k)).chain(v.iter().map(e))));
        }
        items.extend(c.blocks.iter().map(|(_, x)| sx("static-block", [e(x)])));
        sx("class", items)
    }

    fn s(x: &Stmt) -> String {
        let opt = |x: &Option<Expr>| x.as_ref().map_or("_".into(), e);
        match x {
            Stmt::Expr(x) => e(x),
            Stmt::Var(k, ds) => sx(
                k,
                ds.iter().map(|(t, v)| match v {
                    Some(v) => format!("{}={}", p(t), e(v)),
                    None => p(t),
                }),
            ),
            Stmt::Func(f) => fun(f),
            Stmt::Return(v) => sx("return", v.iter().map(e)),
            Stmt::If(c, a, b) => sx("if", [e(c), s(a)].into_iter().chain(b.iter().map(|b| s(b)))),
            Stmt::Block(b) => block(b),
            Stmt::For(i, t, u, b) => sx(
                "for",
                [
                    i.as_ref().map_or("_".into(), |i| s(i)),
                    opt(t),
                    opt(u),
                    s(b),
                ],
            ),
            Stmt::ForOf(k, t, x, b) => sx(
                "for-of",
                k.map(str::to_string).into_iter().chain([p(t), e(x), s(b)]),
            ),
            Stmt::ForIn(k, t, x, b) => sx(
                "for-in",
                k.map(str::to_string).into_iter().chain([p(t), e(x), s(b)]),
            ),
            Stmt::While(c, b) => sx("while", [e(c), s(b)]),
            Stmt::DoWhile(b, c) => sx("do", [s(b), e(c)]),
            Stmt::Switch(d, cs) => sx(
                "switch",
                std::iter::once(e(d)).chain(cs.iter().map(|(t, b)| match t {
                    Some(t) => sx("case", std::iter::once(e(t)).chain(b.iter().map(s))),
                    None => sx("default", b.iter().map(s)),
                })),
            ),
            Stmt::Break(l) => sx("break", l.iter().cloned()),
            Stmt::Continue(l) => sx("continue", l.iter().cloned()),
            Stmt::Labeled(l, b) => sx("label", [l.clone(), s(b)]),
            Stmt::With(x, b) => sx("with", [e(x), s(b)]),
            Stmt::Class(c) => class_s(c),
            Stmt::Throw(x) => sx("throw", [e(x)]),
            Stmt::Try(a, c, f) => {
                let mut items = vec![block(a)];
                if let Some((t, b)) = c {
                    items.push(sx("catch", t.iter().map(p).chain([block(b)])));
                }
                if let Some(f) = f {
                    items.push(sx("finally", [block(f)]));
                }
                sx("try", items)
            }
            Stmt::Empty => ";".into(),
            Stmt::Unsupported(k) => sx("unsupported", [k.clone()]),
        }
    }

    /// A `.js`-style program, one rendered statement per element.
    fn prog(src: &str) -> Vec<String> {
        lower_program(src)
            .unwrap_or_else(|m| panic!("{src}: {m}"))
            .iter()
            .map(s)
            .collect()
    }

    /// The single statement `src` lowers to.
    fn one(src: &str) -> String {
        let v = prog(src);
        assert_eq!(v.len(), 1, "{src}: {v:?}");
        v.into_iter().next().unwrap()
    }

    fn script(src: &str, kind: crate::ScriptKind) -> crate::Script {
        crate::Script::new(src, 1, kind)
    }

    fn un_s(op: UnOp) -> &'static str {
        match op {
            UnOp::Not => "!",
            UnOp::Neg => "-",
            UnOp::Pos => "+",
            UnOp::BitNot => "~",
            UnOp::Typeof => "typeof",
            UnOp::Void => "void",
            UnOp::Delete => "delete",
        }
    }

    fn upd_s(op: UpdateOp) -> &'static str {
        match op {
            UpdateOp::Inc => "++",
            UpdateOp::Dec => "--",
        }
    }

    fn log_s(op: LogicalOp) -> &'static str {
        match op {
            LogicalOp::And => "&&",
            LogicalOp::Or => "||",
            LogicalOp::Nullish => "??",
        }
    }

    fn asg_s(op: AssignOp) -> String {
        match op {
            AssignOp::Assign => "=".into(),
            AssignOp::Bin(b) => format!("{}=", b.as_str()),
            AssignOp::Logical(l) => format!("{}=", log_s(l)),
        }
    }

    fn binding(src: &str, kind: crate::ScriptKind) -> Vec<String> {
        lower(&script(src, kind))
            .unwrap_or_else(|m| panic!("{src}: {m}"))
            .iter()
            .map(s)
            .collect()
    }

    fn run(body: &[Stmt]) -> String {
        let mut it = Interp::default();
        let g = it.global.clone();
        match it.run(body, &g) {
            Ok(v) => v.to_js_string(),
            Err(Ex::Throw(x)) => {
                let name = it
                    .get(&x, "name")
                    .map(|v| v.to_js_string())
                    .unwrap_or_default();
                format!("THROW {name}")
            }
            Err(Ex::Short) => "SHORT".into(),
        }
    }

    /// Evaluate a program (`return` gives the value).
    fn ev(src: &str) -> String {
        run(&lower_program(src).unwrap_or_else(|m| panic!("{src}: {m}")))
    }

    fn ev_cases(cases: &[(&str, &str)]) {
        for (src, want) in cases {
            assert_eq!(ev(&format!("return {src}")), *want, "{src}");
        }
    }

    #[test]
    fn precedence_follows_the_grammar() {
        let cases = [
            ("1 + 2 * 3", "(+ 1 (* 2 3))"),
            ("(1 + 2) * 3", "(* (+ 1 2) 3)"),
            ("a + b % c - d / e", "(- (+ a (% b c)) (/ d e))"),
            ("1 << 2 + 1", "(<< 1 (+ 2 1))"),
            ("a < b == c > d", "(== (< a b) (> c d))"),
            ("1 | 2 & 3 ^ 4", "(| 1 (^ (& 2 3) 4))"),
            ("a == b & c != d", "(& (== a b) (!= c d))"),
            (
                "k in o === x instanceof C",
                "(=== (in k o) (instanceof x C))",
            ),
            ("!a && b || c", "(L|| (L&& (! a) b) c)"),
            ("a || b && c", "(L|| a (L&& b c))"),
            ("(a || b) ?? c", "(L?? (L|| a b) c)"),
            ("a || b ? c : d", "(? (L|| a b) c d)"),
            ("typeof a + 'x'", "(+ (typeof a) \"x\")"),
            ("-a * +b", "(* (- a) (+ b))"),
            ("void 0, !!x", "(seq (void 0) (! (! x)))"),
            ("delete o.p", "(delete (. o p))"),
            ("(-2) ** 2", "(** (- 2) 2)"),
            ("2 ** -1", "(** 2 (- 1))"),
            ("new a.b(1)", "(new (. a b) 1)"),
            ("new a().b", "(. (new a) b)"),
            ("new f", "(new f)"),
            ("a.b(c).d[e]", "([] (. (call (. a b) c) d) e)"),
            ("x = y ? 1 : 2", "(= x (? y 1 2))"),
        ];
        for (src, want) in cases {
            assert_eq!(one(src), want, "{src}");
        }
        ev_cases(&[
            ("1 + 2 * 3", "7"),
            ("(1 + 2) * 3", "9"),
            ("1 << 2 + 1", "8"),
            ("1 | 2 & 3 ^ 4", "7"),
            ("'3' - 1 + '1'", "21"),
            ("1 + 2 + '3'", "33"),
            ("'1' + 2 + 3", "123"),
            ("(-2) ** 2", "4"),
            ("2 ** -1", "0.5"),
            ("typeof undefined + 1", "undefined1"),
            ("!0 + !0", "2"),
            ("1 < 2 < 3", "true"),
            ("3 > 2 > 1", "false"),
        ]);
    }

    #[test]
    fn associativity_follows_the_grammar() {
        let cases = [
            ("a - b - c", "(- (- a b) c)"),
            ("a / b / c", "(/ (/ a b) c)"),
            ("2 ** 3 ** 2", "(** 2 (** 3 2))"),
            ("a = b = c", "(= a (= b c))"),
            ("a += b -= 1", "(+= a (-= b 1))"),
            ("a ? b : c ? d : e", "(? a b (? c d e))"),
            ("a ? b ? 1 : 2 : 3", "(? a (? b 1 2) 3)"),
            ("a && b && c", "(L&& (L&& a b) c)"),
            ("a ?? b ?? c", "(L?? (L?? a b) c)"),
            ("a, b, c", "(seq a b c)"),
        ];
        for (src, want) in cases {
            assert_eq!(one(src), want, "{src}");
        }
        ev_cases(&[
            ("10 - 4 - 3", "3"),
            ("64 / 4 / 2", "8"),
            ("2 ** 3 ** 2", "512"),
            ("(function () { var a, b; a = b = 4; return a + b })()", "8"),
            ("1 ? 'b' : 0 ? 'd' : 'e'", "b"),
            ("0 ? 'b' : 0 ? 'd' : 'e'", "e"),
            ("(1, 2, 3)", "3"),
        ]);
    }

    #[test]
    fn logical_operators_short_circuit() {
        assert!(matches!(
            &lower_program("a ?? b").unwrap()[..],
            [Stmt::Expr(Expr::Logical(LogicalOp::Nullish, ..))]
        ));
        assert_eq!(one("a ||= b"), "(||= a b)");
        assert_eq!(one("o.p &&= f()"), "(&&= (. o p) (call f))");
        assert_eq!(one("o[k] ??= 1"), "(??= ([] o k) 1)");
        assert_eq!(
            ev("var n = 0; false && n++; true || n++; null ?? n++; 0 ?? n++; '' ?? n++; return n"),
            "1"
        );
        ev_cases(&[
            ("0 || 'x'", "x"),
            ("0 ?? 'x'", "0"),
            ("'' && f()", ""),
            ("NaN || null || 'z'", "z"),
            ("null ?? undefined ?? 3", "3"),
            ("1 && 2 && 3", "3"),
            ("false ?? 1", "false"),
        ]);
        assert_eq!(
            ev("var c = 0; function f() { c++; return 9 }\n\
                var a = 1, b = 0, n = null; a ||= f(); b &&= f(); n ??= f(); a &&= 2;\n\
                return [a, b, n, c].join()"),
            "2,0,9,1"
        );
    }

    #[test]
    fn optional_chains() {
        let cases = [
            ("a?.b", "(chain (?. a b))"),
            ("a?.b.c", "(chain (. (?. a b) c))"),
            ("a?.[k]", "(chain (?[] a k))"),
            ("f?.()", "(chain (?call f))"),
            ("a.b?.(x)", "(chain (?call (. a b) x))"),
            // V4: every call on the chain's spine is optional, arguments are not the spine.
            ("o?.n.m(g())", "(chain (?call (. (?. o n) m) (call g)))"),
            ("o.f()?.x", "(chain (?. (?call (. o f)) x))"),
            ("o?.[f()]", "(chain (?[] o (call f)))"),
            ("(a?.b).c", "(. (chain (?. a b)) c)"),
            ("a?.b ?? d", "(L?? (chain (?. a b)) d)"),
        ];
        for (src, want) in cases {
            assert_eq!(one(src), want, "{src}");
        }
        ev_cases(&[
            ("String(null?.x)", "undefined"),
            ("String((void 0)?.x.y.z)", "undefined"),
            ("String(null?.[1])", "undefined"),
            ("String(null?.())", "undefined"),
            ("({a: {b: 3}})?.a?.b", "3"),
            ("({f() { return 4 }}).f?.()", "4"),
            ("String(({}).f?.())", "undefined"),
            ("String(({n: {}})?.n.nope())", "undefined"),
            ("(null?.x)?.y ?? 'd'", "d"),
        ]);
        // Without `?.` a nullish base still throws; parentheses end the chain.
        assert_eq!(ev("var o = {}; return o?.a.b"), "THROW TypeError");
        assert_eq!(ev("var o = null; return (o?.b).c"), "THROW TypeError");
        // Arguments of a short-circuited call are not evaluated.
        assert_eq!(
            ev("var n = 0; var o = null; o?.f(n++); o?.[n++]; return n"),
            "0"
        );
    }

    #[test]
    fn assignment_targets() {
        let cases = [
            ("x = 1", "(= x 1)"),
            ("o.p += 2", "(+= (. o p) 2)"),
            ("o[k] **= 2", "(**= ([] o k) 2)"),
            ("x >>>= 1", "(>>>= x 1)"),
            ("a.b.c = d", "(= (. (. a b) c) d)"),
            ("x <<= y |= z", "(<<= x (|= y z))"),
            ("x++", "(post++ x)"),
            ("++o.p", "(pre++ (. o p))"),
            ("--o[k]", "(pre-- ([] o k))"),
            ("o.n--", "(post-- (. o n))"),
            ("(x) = 1", "(= x 1)"),
        ];
        for (src, want) in cases {
            assert_eq!(one(src), want, "{src}");
        }
        assert_eq!(
            ev(
                "var o = {p: 1, q: [5]}; o.p += 2; o.q[0] *= 3; var x = -8; x >>>= 28;\n\
                var y = 2; y **= 3; var z = 7; z %= 4; z <<= 2; z ^= 1; return [o.p, o.q[0], x, y, z].join()"
            ),
            "3,15,15,8,13"
        );
        assert_eq!(
            ev("var i = 1; var a = i++; var b = ++i; return [a, b, i].join()"),
            "1,3,3"
        );
        assert_eq!(
            ev("var o = {n: '5'}; var r = o.n++; return [typeof r, r, o.n].join()"),
            "number,5,6"
        );
        // The target reference is evaluated before the right-hand side.
        assert_eq!(
            ev("var o = {}; var i = 0; o[i++] = i; return JSON.stringify(o)"),
            r#"{"0":1}"#
        );
        assert_eq!(
            ev("var a = [1, 2]; var i = 0; a[i] += (i = 1); return a.join() + i"),
            "2,21"
        );
        for bad in ["1 = 2", "x++ = 1", "f() += 1", "++1", "a + b = c"] {
            assert!(lower_program(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn destructuring_patterns() {
        let cases = [
            (
                "var {a, b: {c}, ...r} = o",
                "(var {a: a, b: {c: c}, ...r}=o)",
            ),
            (
                "let [x, , y = 2, ...z] = arr",
                "(let [x, _, y=2, ...z]=arr)",
            ),
            (
                "const {[k]: v = 1, 'q-r': w, 3: n} = o",
                "(const {[k]: v=1, q-r: w, 3: n}=o)",
            ),
            ("[a, b] = [b, a]", "(= [a, b] [b, a])"),
            ("({a, b: c = 1, ...d} = o)", "(= {a: a, b: c=1, ...d} o)"),
            ("({a = 5} = {})", "(= {a: a=5} {})"),
            (
                "[o.p, o[k] = 1, ...o.r] = v",
                "(= [(. o p), ([] o k)=1, ...(. o r)] v)",
            ),
            ("({[k]: [x]} = o)", "(= {[k]: [x]} o)"),
        ];
        for (src, want) in cases {
            assert_eq!(one(src), want, "{src}");
        }
        assert_eq!(
            one("function f({a = 1}, [b], ...c) { return a + b }"),
            "(function f ({a: a@0.0=1} [b@0.1] ...c@0.2) {(return (+ a@0.0 b@0.1))})"
        );
        ev_cases(&[
            (
                "(function () { var a = 1, b = 2; [a, b] = [b, a]; return [a, b].join() })()",
                "2,1",
            ),
            (
                "(function () { var [p = 1, q = 2] = [undefined, null]; return JSON.stringify([p, q]) })()",
                "[1,null]",
            ),
            (
                "(function () { var {a, ...r} = {a: 1, b: 2, c: 3}; return a + JSON.stringify(r) })()",
                r#"1{"b":2,"c":3}"#,
            ),
            (
                "(function () { var [x, , ...y] = 'abcd'; return x + y.join('') })()",
                "acd",
            ),
            (
                "(function () { var {a: {b: [, c]}} = {a: {b: [1, 2]}}; return c })()",
                "2",
            ),
            (
                "(function () { var k = 'z', {[k]: v} = {z: 7}; return v })()",
                "7",
            ),
            (
                "(function ({a, b = a * 2}, [c] = [3]) { return a + b + c })({a: 1})",
                "6",
            ),
            (
                "(function () { var d = 0, {x = ++d} = {x: 0}; return d })()",
                "0",
            ),
            (
                "(function () { var o = {}; [o.a, o['b']] = [1, 2]; return o.a + o.b })()",
                "3",
            ),
        ]);
        assert_eq!(ev("var {a} = null; return a"), "THROW TypeError");
        assert_eq!(ev("var [a] = {}; return a"), "THROW TypeError");
    }

    /// Lowering records each function's kind: only plain functions and class constructors are
    /// constructors (ECMA-262 §15.4.4 DefineMethod, §15.4.5, §15.7.14).
    #[test]
    fn function_kinds() {
        let body = lower_program(
            "var o = { m() {}, get g() { return 1 }, set s(v) {}, f: function () {}, a: () => 1, \
             n: function n() {} };\n\
             class C { constructor() {} m() {} static sm() {} get g() { return 1 } set g(v) {} }",
        )
        .unwrap();
        let Stmt::Var(_, ds) = &body[0] else { panic!() };
        let Some(Expr::Object(props)) = &ds[0].1 else {
            panic!()
        };
        let kind = |e: &Expr| match e {
            Expr::Func(f) => f.kind,
            Expr::Call(w, _, _) => match &**w {
                Expr::Func(w) => match &w.body[0] {
                    Stmt::Func(f) => f.kind,
                    _ => panic!(),
                },
                _ => panic!(),
            },
            e => panic!("{e:?}"),
        };
        let kinds: Vec<FnKind> = props.iter().map(|(_, e, _)| kind(e)).collect();
        use FnKind::*;
        assert_eq!(kinds, [Method, Accessor, Accessor, Normal, Arrow, Normal]);
        let Stmt::Class(c) = &body[1] else { panic!() };
        assert_eq!(c.ctor.as_ref().map(|f| f.kind), Some(ClassConstructor));
        let kinds: Vec<FnKind> = c.methods.iter().map(|m| m.func.kind).collect();
        assert_eq!(kinds, [Method, Method, Accessor, Accessor]);
        assert!(kinds.iter().all(|k| !k.is_constructor()) && Normal.is_constructor());
        assert!(ClassConstructor.is_constructor());
    }

    /// Generator and async functions keep their ECMA-262 function kind instead of becoming
    /// ordinary functions; `unsupported` names each one, and the interpreter refuses to call
    /// them rather than run the body to completion.
    #[test]
    fn coroutines_are_not_ordinary_functions() {
        let src = "async function a() { return 1 }\n\
                   function* g() {}\n\
                   async function* ag() {}\n\
                   var h = async () => 1, p = () => 1, f = function () {};\n\
                   var o = { async m() {}, *n() {} };\n\
                   class C { async m() {} static *n() {} m2() {} }";
        let body = lower_program(src).unwrap();
        let mut found = Vec::new();
        unsupported(&body, &mut found);
        assert_eq!(
            found,
            [
                "AsyncFunction",
                "AsyncGeneratorFunction",
                "AsyncFunction",
                "AsyncFunction",
                "AsyncFunction",
            ]
        );
        let Stmt::Func(a) = &body[0] else { panic!() };
        assert_eq!(a.coroutine, Some(Coroutine::Async));
        assert!(!a.this_only);
        for call in [
            "async function f() { return 1 } return f()",
            "async function* g() {} return g()",
            "var h = async () => 1; return h()",
            "return ({ async m() { return 1 } }).m()",
        ] {
            assert_eq!(ev(call), "THROW SyntaxError", "{call}");
        }
        let Stmt::Func(g) = &body[1] else { panic!() };
        assert_eq!(
            (g.coroutine, g.kind),
            (Some(Coroutine::Generator), FnKind::Generator)
        );
        for call in [
            "function* g() { return 1 } return String(g())",
            "class C { *m() {} } return String(new C().m())",
        ] {
            assert_eq!(ev(call), "[object Generator]", "{call}");
        }
        assert_eq!(ev("async function f() {} return typeof f"), "function");
    }

    #[test]
    fn qmlir_shape_of_the_ast_agrees_with_the_tokens() {
        use crate::ast::tests::{SCRIPTS, SHAPES, expected};
        let kind = crate::ScriptKind::Expression;
        for &(src, want) in SHAPES {
            let s = script(src, kind);
            assert_eq!(qmlir_shape(&s), Some(expected(want)), "{src}");
        }
        for src in SCRIPTS
            .iter()
            .filter(|s| !s.starts_with('{') && **s != "5px" && **s != "1__0")
        {
            let s = script(src, kind);
            assert_eq!(qmlir_shape(&s), Some(crate::Shape::Script), "{src}");
        }
    }

    #[test]
    fn arrow_and_function_bodies() {
        let cases = [
            ("x => x * 2", "(=> (x@0.0) {(return (* x@0.0 2))})"),
            ("() => ({a: 1})", "(=> () {(return {a: 1})})"),
            ("() => { x }", "(=> () {x})"),
            (
                "(a, b = a) => a",
                "(=> (a@0.0 b@0.1=a@0.0) {(return a@0.0)})",
            ),
            ("(...r) => r", "(=> (...r@0.0) {(return r@0.0)})"),
            (
                "function (a) { return function () { return a } }",
                "(function (a@0.0) {(return (function () {(return a@1.0)}))})",
            ),
        ];
        for (src, want) in cases {
            assert_eq!(one(&format!("({src})")), want, "{src}");
        }
        // A named function expression sees its own name.
        assert_eq!(
            one("(function fact(n) { return fact })"),
            "(call (=> () {(function fact (n@0.0) {(return fact@1.0)}); (return fact@0.0)}))"
        );
        let body = lower_program(
            "function f() { return arguments.length }\n\
             var g = () => 1;\n\
             function h() { return this.x * 2 }\n\
             function k(a) { var b; let c; { let d = 1 } function m() {} class C {} }",
        )
        .unwrap();
        let func_of = |st: &Stmt| -> Rc<Func> {
            match st {
                Stmt::Func(f) => f.clone(),
                Stmt::Var(_, ds) => match &ds[0].1 {
                    Some(Expr::Func(f)) => f.clone(),
                    _ => panic!(),
                },
                _ => panic!(),
            }
        };
        let (f, g, h, k) = (
            func_of(&body[0]),
            func_of(&body[1]),
            func_of(&body[2]),
            func_of(&body[3]),
        );
        assert!(f.uses_arguments && !f.is_arrow() && !f.this_only);
        assert!(g.is_arrow() && !g.uses_arguments && !g.this_only);
        assert!(h.this_only);
        // a, b, c, m, C, then the block's d as a fresh slot.
        assert_eq!(k.nslots, 6);
        assert!(matches!(&k.body[3], Stmt::Func(m) if m.slot == Some(3)));
        assert!(matches!(&k.body[4], Stmt::Class(c) if c.slot == Some(4)));
        assert_eq!(s(&k.body[2]), "{(let d@0.5=1)}");
        ev_cases(&[
            ("(x => x * 2)(4)", "8"),
            ("(() => ({a: 1}))().a", "1"),
            ("String((() => { 1 })())", "undefined"),
            ("({v: 1, f() { return (() => this.v)() }}).f()", "1"),
            ("(function () { return (() => arguments[0])() })(5)", "5"),
            (
                "(function fact(n) { return n ? n * fact(n - 1) : 1 })(5)",
                "120",
            ),
            ("typeof (function q() {}) + typeof q", "functionundefined"),
            ("(function (a, b = a + 1) { return b })(1)", "2"),
            ("(function (a, ...r) { return r.length })(1, 2, 3)", "2"),
        ]);
        assert_eq!(ev("return f(); function f() { return 7 }"), "7");
        assert_eq!(ev("return typeof v; var v = 1"), "undefined");
        assert_eq!(
            ev("function f() { return typeof v; var v } return f()"),
            "undefined"
        );
        assert_eq!(
            ev("function mk() { var n = 0; return () => ++n } var c = mk(); c(); return c()"),
            "2"
        );
    }

    #[test]
    fn template_literals() {
        assert_eq!(one("`a${x}b${y}`"), "(tpl \"a\" x \"b\" y \"\")");
        assert_eq!(one("`plain`"), "(tpl \"plain\")");
        assert_eq!(one(r"`\n\x41\u{1F600}`"), "(tpl \"\\nA😀\")");
        assert_eq!(one("`${a}${b}`"), "(tpl \"\" a \"\" b \"\")");
        assert_eq!(
            one(r"tag`a${x}\u{110000}b`"),
            "(call tag (tplobj \"a\" undefined (raw \"a\" \"\\\\u{110000}b\")) x)"
        );
        assert_eq!(one("o.f`x`"), "(call (. o f) (tplobj \"x\" (raw \"x\")))");
        assert_eq!(one("`a\r\nb`"), "(tpl \"a\\nb\")");
        assert_eq!(
            one("tag`a\r\nb\rc`"),
            "(call tag (tplobj \"a\\nb\\nc\" (raw \"a\\nb\\nc\")))"
        );
        ev_cases(&[
            ("`${1 + 1}-${'x'}`", "2-x"),
            (
                "`${[1, 2]}|${null}|${undefined}|${{}}`",
                "1,2|null|undefined|[object Object]",
            ),
            ("`a${`b${'c'}`}d`", "abcd"),
            ("`line1\nline2`.length", "11"),
            (
                "`${{toString() { return 'T' }, valueOf() { return 'V' }}}`",
                "V",
            ), // Tagged templates: one template object per site, a member tag's `this` is its object,
            // an invalid escape cooks to undefined (ECMA-262 §13.2.8.4, §13.2.8.3).
            (
                "(() => { function id(s) { return s } function f() { return id`x` } \
                 return [f() === f(), id`x` === id`x`].join() })()",
                "true,false",
            ),
            (
                "({v: 3, f(s, x) { return this.v + s[0] + x }}).f`a${4}`",
                "3a4",
            ),
            (
                r"((s, x) => [s.length, s[0], s[1], s.raw.join('/'), x, Object.keys(s)].join('|'))`a${1}\xg`",
                r"2|a||a/\xg|1|0,1",
            ),
        ]);
        assert!(lower_program(r"`\u{zz}`").is_err());
    }

    #[test]
    fn object_keys() {
        assert_eq!(
            one("({[k]: 1, 's': 2, 3: 4, a, 0x10: 5, 1.5: 6, m() {}})"),
            "{[k]: 1, s: 2, 3: 4, a: a, 16: 5, 1.5: 6, m: (function () {})}"
        );
        assert_eq!(one("o['a' + 'b']"), "([] o (+ \"a\" \"b\"))");
        // Accessors keep their key and carry the kind as `PropKind`.
        assert_eq!(
            one("({get x() { return 1 }, set [k](v) {}})"),
            "{get x: (function () {(return 1)}), set [k]: (function (v@0.0) {})}"
        );
        ev_cases(&[
            ("({get x() { return 4 }}).x", "4"),
            (
                "(function () { var k = 'y'; return ({get [k]() { return 5 }}).y })()",
                "5",
            ),
            (
                "(function () { var o = {set v(x) { this.w = x * 2 }}; o.v = 3; return o.w })()",
                "6",
            ),
            (
                "(function () { var k = 'z'; return JSON.stringify({[k + 1]: 1, [2 * 3]: 2}) })()",
                r#"{"6":2,"z1":1}"#,
            ),
            ("({0x10: 'h'})[16]", "h"),
        ]);
        // Computed keys and values are evaluated left to right.
        assert_eq!(
            ev(
                "var log = []; var o = {[(log.push(1), 'a')]: log.push(2), [(log.push(3), 'b')]: log.push(4)};\n\
                return log.join() + '|' + o.a + o.b"
            ),
            "1,2,3,4|24"
        );
    }

    #[test]
    fn numeric_literal_keys_use_number_to_string() {
        assert_eq!(one("({1e21: 1, 1e-7: 2})"), "{1e+21: 1, 1e-7: 2}");
        assert_eq!(
            one(
                "({0.0: 1, .5: 2, 1.5e300: 3, 5e-324: 4, 1e-6: 5, 0x10: 6, 123456789012345680000: 7})"
            ),
            "{0: 1, 0.5: 2, 1.5e+300: 3, 5e-324: 4, 0.000001: 5, 16: 6, 123456789012345680000: 7}"
        );
        assert_eq!(one("({1e21: a} = o)"), "(= {1e+21: a} o)");
        ev_cases(&[
            ("Object.keys({1e21: 1, 1e-7: 2}).join()", "1e+21,1e-7"),
            ("({1e21: 'x'})['1e+21'] + ({0.1: 'y'})[0.1]", "xy"),
        ]);
    }

    #[test]
    fn spread_elements() {
        let cases = [
            ("[...a, , b]", "[...a, _, b]"),
            ("f(...a, b)", "(call f ...a b)"),
            ("new F(...a)", "(new F ...a)"),
            ("({...o, a: 1})", "{...o, a: 1}"),
            ("[,]", "[_]"),
        ];
        for (src, want) in cases {
            assert_eq!(one(src), want, "{src}");
        }
        ev_cases(&[
            ("Math.max(...[1, 5, 3])", "5"),
            ("[...'ab', ...[1]].join()", "a,b,1"),
            (
                "JSON.stringify({a: 1, ...{a: 2, b: 3}, b: 4})",
                r#"{"a":2,"b":4}"#,
            ),
            (
                "JSON.stringify({...null, ...undefined, ...'hi'})",
                r#"{"0":"h","1":"i"}"#,
            ),
            ("[1, , 2].length", "3"),
            (
                "(function (a, b, c) { return a + b + c })(...[1, 2], 3)",
                "6",
            ),
            ("[...new Set([1, 1, 2])].join()", "1,2"),
        ]);
        assert_eq!(ev("return [...null]"), "THROW TypeError");
    }

    #[test]
    fn if_and_loops() {
        let cases = [
            ("if (a) b; else if (c) d; else e", "(if a b (if c d e))"),
            ("if (a) if (b) x; else y", "(if a (if b x y))"),
            (
                "for (let i = 0; i < 3; i++) s += i",
                "(for (let i=0) (< i 3) (post++ i) (+= s i))",
            ),
            ("for (;;) break", "(for _ _ _ (break))"),
            (
                "for (i = 0, j = 1; ; i++, j--) {}",
                "(for (seq (= i 0) (= j 1)) _ (seq (post++ i) (post-- j)) {})",
            ),
            ("for (var k in o) f(k)", "(for-in var k o (call f k))"),
            ("for (o.k in p);", "(for-in (. o k) p ;)"),
            ("for (const [a, b] of m) {}", "(for-of const [a, b] m {})"),
            ("for (let x in o);", "(for-in let x o ;)"),
            ("for ({x} of xs);", "(for-of {x: x} xs ;)"),
            ("while (n--) f()", "(while (post-- n) (call f))"),
            ("do x++; while (x < 3)", "(do (post++ x) (< x 3))"),
        ];
        for (src, want) in cases {
            assert_eq!(one(src), want, "{src}");
        }
        // Inside a function, loop bindings nothing captures become frame slots.
        assert_eq!(
            one("function f(xs) { for (let i = 0; i < 1; i++) for (const x of xs) g(x, i) }"),
            "(function f (xs@0.0) {(for (let i@0.1=0) (< i@0.1 1) (post++ i@0.1) (for-of const x@0.2 xs@0.0 (call g x@0.2 i@0.1)))})"
        );
        // A captured loop binding stays a per-iteration name.
        assert_eq!(
            one("function f() { for (let i = 0; i < 1; i++) g(() => i) }"),
            "(function f () {(for (let i=0) (< i 1) (post++ i) (call g (=> () {(return i)})))})"
        );
        ev_cases(&[
            (
                "(function (a, b) { if (a) if (b) return 1; else return 2; return 3 })(1, 0)",
                "2",
            ),
            (
                "(function () { var r = []; for (var k in [7, 8]) r.push(typeof k + k); return r.join() })()",
                "string0,string1",
            ),
            (
                "(function () { var n = 0; for (const c of 'a😀') n++; return n })()",
                "2",
            ),
            (
                "(function () { var fs = []; for (let i = 0; i < 3; i++) fs.push(() => i); return fs.map(f => f()).join() })()",
                "0,1,2",
            ),
            (
                "(function () { var fs = []; for (var i = 0; i < 3; i++) fs.push(() => i); return fs.map(f => f()).join() })()",
                "3,3,3",
            ),
            (
                "(function () { var n = 0; do n++; while (false); return n })()",
                "1",
            ),
            (
                "(function () { var n = 0; while (n < 5) { n++; if (n == 2) continue; if (n == 4) break } return n })()",
                "4",
            ),
            (
                "(function () { var s = 0; for (var i = 0, j = 10; i < j; i++, j--) s++; return s })()",
                "5",
            ),
            (
                "(function () { var o = {}, r = []; for (o.k of [1, 2]) r.push(o.k); return r.join() })()",
                "1,2",
            ),
            (
                "(function () { var r = []; for (var [k, v] of Object.entries({a: 1})) r.push(k + v); return r.join() })()",
                "a1",
            ),
            (
                "(function () { var r = []; for (var k in {a: 1, b: 2}) r.push(k); return r.join() })()",
                "a,b",
            ),
            (
                "(function () { var n = 0; for (var k in null) n++; for (var k in undefined) n++; return n })()",
                "0",
            ),
        ]);
    }

    #[test]
    fn for_in_of_var_is_function_scoped() {
        ev_cases(&[
            ("(function () { for (var k in {a: 1}) {} return k })()", "a"),
            ("(function () { for (var x of [1, 2]) {} return x })()", "2"),
            (
                "(function () { var x = 0; for (var x of [5]) {} return x })()",
                "5",
            ),
            (
                "(function () { var x = 0; for (x of [1, 2]); return x })()",
                "2",
            ),
            (
                "(function () { var o = {}; for (o.k in {a: 1}); return o.k })()",
                "a",
            ),
            // `var` is one binding for all iterations; `let`/`const` a fresh one per iteration.
            (
                "(function () { var fs = []; for (var x of [1, 2]) fs.push(() => x); return fs.map(f => f()).join() })()",
                "2,2",
            ),
            (
                "(function () { var fs = []; for (let x of [1, 2]) fs.push(() => x); return fs.map(f => f()).join() })()",
                "1,2",
            ),
            (
                "(function () { var fs = []; for (const k in {a: 1, b: 2}) fs.push(() => k); return fs.map(f => f()).join() })()",
                "a,b",
            ),
            (
                "(function () { let x = 'o'; for (let x of [1]) {} return x })()",
                "o",
            ),
        ]);
        let top = "var fs = []; for (let x of [1, 2]) fs.push(function () { return x }); for (var y of [3, 4]); return fs[0]() + ',' + fs[1]() + ',' + y";
        assert_eq!(ev(top), "1,2,4");
    }

    #[test]
    fn switch_statements() {
        assert_eq!(
            one("switch (x) { case 1: a(); case 2: b(); break; default: d(); case 3: c() }"),
            "(switch x (case 1 (call a)) (case 2 (call b) (break)) (default (call d)) (case 3 (call c)))"
        );
        assert_eq!(one("switch (x) {}"), "(switch x)");
        let f = "function f(x) { var r = ''; switch (x) { case 1: r += 'a'; case 2: r += 'b'; break;\n\
                 default: r += 'd'; case 3: r += 'c' } return r }\n";
        for (arg, want) in [
            ("1", "ab"),
            ("2", "b"),
            ("3", "c"),
            ("9", "dc"),
            ("'1'", "dc"),
        ] {
            assert_eq!(ev(&format!("{f}return f({arg})")), want, "f({arg})");
        }
        assert_eq!(
            ev(
                "var n = 0; function t() { n++; return 2 } switch (2) { case 1: case t(): case t(): } return n"
            ),
            "1"
        );
        assert_eq!(
            ev(
                "function f(x) { switch (x) { case 0: let y = 'z'; return y; default: return 'd' } } return f(0) + f(1)"
            ),
            "zd"
        );
    }

    #[test]
    fn try_throw_and_labels() {
        let cases = [
            (
                "try { a() } catch (e) { b(e) } finally { c() }",
                "(try {(call a)} (catch e {(call b e)}) (finally {(call c)}))",
            ),
            ("try {} catch {}", "(try {} (catch {}))"),
            ("try {} finally {}", "(try {} (finally {}))"),
            (
                "try {} catch ({message}) {}",
                "(try {} (catch {message: message} {}))",
            ),
            ("throw new Error('x')", "(throw (new Error \"x\"))"),
            (
                "outer: for (;;) { for (;;) { continue outer } }",
                "(label outer (for _ _ _ {(for _ _ _ {(continue outer)})}))",
            ),
            ("l: { break l }", "(label l {(break l)})"),
            (";", ";"),
        ];
        for (src, want) in cases {
            assert_eq!(one(src), want, "{src}");
        }
        ev_cases(&[
            (
                "(function () { try { return 1 } finally { return 2 } })()",
                "2",
            ),
            (
                "(function () { var r = ''; try { try { throw 1 } finally { r += 'f' } } catch (e) { r += e } return r })()",
                "f1",
            ),
            (
                "(function () { try { null.x } catch (e) { return e instanceof TypeError } })()",
                "true",
            ),
            (
                "(function () { try { throw {message: 'm'} } catch ({message}) { return message } })()",
                "m",
            ),
            (
                "(function () { try { throw 1 } catch { return 'c' } })()",
                "c",
            ),
            (
                "(function () { var e = 'outer'; try { throw 'inner' } catch (e) {} return e })()",
                "outer",
            ),
            (
                "(function () { var n = 0; outer: for (var i = 0; i < 3; i++) for (var j = 0; j < 3; j++) { if (j == 1) continue outer; if (i == 2) break outer; n++ } return n })()",
                "2",
            ),
            (
                "(function () { var x = 0; l: { x = 1; break l; x = 2 } return x })()",
                "1",
            ),
            (
                "(function () { var r = 0; for (var i = 0; i < 3; i++) { try { if (i == 1) continue; r += 10 } finally { r += 1 } } return r })()",
                "23",
            ),
        ]);
        assert_eq!(ev("throw new RangeError('r')"), "THROW RangeError");
    }

    #[test]
    fn classes() {
        assert_eq!(
            one(
                "class A extends B { constructor(x) { super(x) } m() {} static s() {} get g() { return 1 } x = 1; static y }"
            ),
            "(class A (extends B) (ctor (function (x@0.0) {(call super x@0.0)})) (method m (function () {})) \
             (static-method s (function () {})) (get g (function () {(return 1)})) (field x 1) (static-field y))"
        );
        assert_eq!(one("(class {})"), "(class)");
        ev_cases(&[
            (
                "(function () { class A { constructor(v) { this.v = v } get d() { return this.v * 2 } static k() { return 'K' } }\n\
              class B extends A { constructor() { super(3) } } return new B().d + A.k() })()",
                "6K",
            ),
            (
                "(function () { class C { x = 1; y = this.x + 1 } var c = new C(); return c.x + c.y })()",
                "3",
            ),
        ]);
    }

    #[test]
    fn early_errors_are_rejected() {
        for bad in [
            "break;",
            "continue;",
            "l: l: ;",
            "foo: { continue foo }",
            "function f(a, a) { 'use strict' }",
            "x: { break y }",
            "a: while (1) { b: { continue b } }",
            "a: { a: ; }",
            "switch (1) { case 1: continue }",
            "while (1) { function g() { break } }",
            "a: while (1) { (() => { continue a })() }",
            "'use strict'; function f(a, a) {}",
            "((a, a) => 0)",
            "({ m(a, a) {} })",
            "({ set s([a, a]) {} })",
            "class C { m(a, a) {} }",
            "(class { constructor(a, a) {} })",
            "function f(a, [a]) {}",
            "function f(a = 1, a) {}",
            "function f(a, ...a) {}",
            "function f(a = 1) { 'use strict' }",
            "while (1) { class C { static { break } } }",
            "l: { class C { static { break l } } }",
            "class C { static { arguments } }",
            "class C { static { () => arguments } }",
            "class C extends Object { static { super() } }",
            "class C extends Object { static { () => super() } }",
        ] {
            let e = lower_program(bad).expect_err(bad);
            assert!(e.starts_with("SyntaxError: "), "{bad}: {e}");
        }
        // ClassStaticBlockBody (ECMA-262 §15.7.1): the parser rejects `return` and `await`.
        for bad in [
            "class C { static { return } }",
            "class C { static { await } }",
        ] {
            lower_program(bad).expect_err(bad);
        }
        for good in [
            "function f(a, a) {}",
            "(function (a, a) {})",
            "({ m: function (a, a) {} })",
            "a: b: while (1) { continue a }",
            "a: { break a }",
            "a: ; a: ;",
            "a: { b: { break a } }",
            "switch (1) { case 1: break }",
            "for (;;) { switch (1) { default: continue } }",
            "while (1) { if (1) break; else continue }",
            "do { continue } while (0)",
            "for (const k in o) { break }",
            "a: for (const k of o) { (function () { b: while (1) break b })(); continue a }",
            "class C { static { var a = 1; function f() { return arguments } while (1) break } }",
            "class B {} class C { static { class D extends B { constructor() { super() } } } }",
        ] {
            assert!(lower_program(good).is_ok(), "{good}");
        }
        use crate::ScriptKind::{Block, Function};
        for (src, kind) in [
            ("break", Block),
            ("{ l: l: ; }", Block),
            ("function f(a, a) { 'use strict' }", Function),
        ] {
            let e = lower(&script(src, kind)).expect_err(src);
            assert!(e.starts_with("SyntaxError: "), "{src}: {e}");
        }
        assert!(lower(&script("for (const x of xs) { if (x) break }", Block)).is_ok());
    }

    // The next three lower correctly (the IR is asserted elsewhere); the evaluator's result is
    // what deviates from ECMA-262 / V4.
    #[test]
    fn array_holes_are_absent() {
        ev_cases(&[
            (
                "(1 in [1, , 2]) + ',' + Object.keys([1, , 2]).join()",
                "false,0,2",
            ),
            (
                "(function () { var a = [1]; a.length = 3; a[4] = 5; delete a[0]; var n = 0; a.forEach(() => n++); \
             return [n, a.length, 2 in a, a.join('-'), JSON.stringify(a), JSON.stringify(a.map(x => x * 2)), \
              [...a].length, [...a].includes(undefined), [...a.keys()].join(''), a.indexOf(undefined), \
              a.reduce((s, x, i) => s + i, ''), a.filter(() => true).length].join(' ') })()",
                "1 5 false ----5 [null,null,null,null,5] [null,null,null,null,10] 5 true 01234 -1 4 1",
            ),
            (
                "[3, , 1, undefined].sort().length + ',' + (2 in [3, , 1].sort()) + ',' + Array(2).join('x')",
                "4,false,x",
            ),
        ]);
    }

    #[test]
    fn integer_keys_enumerate_first() {
        ev_cases(&[
            ("Object.keys({b: 1, a: 2, 1: 3, 0: 4}).join()", "0,1,b,a"),
            ("JSON.stringify({z: 1, [2 * 3]: 2})", r#"{"6":2,"z":1}"#),
            (
                "(function () { var o = {b: 1}; o[10] = 1; o['01'] = 1; o[2] = 1; o.a = 1; var r = []; for (var k in o) r.push(k); return r.join() })()",
                "2,10,b,01,a",
            ),
            (
                "Object.entries(Object.assign({}, {x: 1, 1: 2})).join(';') + '|' + Object.keys({...{y: 1, 0: 2}}).join()",
                "1,2;x,1|0,y",
            ),
            (
                "(function () { var a = [7]; a.p = 1; return Object.keys(a).join() })()",
                "0,p",
            ),
        ]);
    }

    #[test]
    fn function_length_counts_formal_params() {
        ev_cases(&[
            (
                "[(function (a, b) {}).length, ((a, b = 1, c) => 0).length, (function (...r) {}).length].join()",
                "2,1,0",
            ),
            (
                "(function () { var o = {m(a, b, ...c) {}}; class K { constructor(a = 1) {} }; \
             function f(a, b, c) {} f.length = 9; var b = f.bind(null, 1); \
             return [o.m.length, K.length, f.length, b.length, f.bind(null, 1, 2, 3, 4).length, b(2) === undefined, \
              new (function (x) { this.x = x; }.bind(null, 5))().x, typeof b].join() })()",
                "2,0,3,2,0,true,5,function",
            ),
        ]);
    }

    #[test]
    fn unsupported_and_parse_errors() {
        // V4 has no BigInt: the whole script is replaced by one unsupported construct.
        assert_eq!(prog("var a = 1; var b = 1n"), ["(unsupported BigInt)"]);
        assert_eq!(ev("var b = 1n; return 1"), "THROW SyntaxError");
        assert_eq!(
            binding("x + 1n", crate::ScriptKind::Expression),
            ["(unsupported BigInt)"]
        );
        // A `with` statement runs in the interpreter; compiled code does not run it.
        assert_eq!(one("with (o) {}"), "(with o {})");
        assert_eq!(ev("with ({a: 1}) { return a }"), "1");
        let mut found = Vec::new();
        unsupported(
            &lower_program("f(import(x)); if (a) { with (o) {} }").unwrap(),
            &mut found,
        );
        assert_eq!(found, ["Import", "with"]);
        // The build-time checks walk every position that holds an expression.
        let unsup = |src: &str| {
            let mut found = Vec::new();
            unsupported(&lower_program(src).unwrap(), &mut found);
            found
        };
        for src in [
            "f(import(x))",
            "function f(a = import(x)) {}",
            "function f(...[a = import(x)]) {}",
            "var f = function ({a = import(x)}) {}",
            "var f = (a = import(x)) => a",
            "var {a = import(x)} = {}",
            "var {[import(x)]: a} = {}",
            "var {a: {b = import(x)}} = {}",
            "({...o[import(x)]} = {})",
            "var [[a = import(x)]] = []",
            "var [, ...[a = import(x)]] = []",
            "({a = import(x)} = {})",
            "[{a: o[import(x)]}] = [{}]",
            "for (var [a = import(x)] of [[]]) {}",
            "for (let {a = import(x)} in {}) {}",
            "for ([o[import(x)]] of []) {}",
            "try {} catch ({a = import(x)}) {}",
            "try {} catch ([a = import(x)]) {}",
            "class A extends (import(x)) {}",
            "var A = class extends (import(x)) {}",
            "class A { constructor(a = import(x)) {} }",
            "class A { m(a = import(x)) {} }",
            "class A { [import(x)]() {} }",
            "class A { static get [import(x)]() {} }",
            "class A { [import(x)] = 1 }",
            "class A { static f = import(x) }",
            "var o = { m(a = import(x)) {} }",
            "var o = { [import(x)]: 1 }",
        ] {
            assert_eq!(unsup(src), ["Import"], "{src}");
        }
        for src in [
            "Qt.createQmlObject('x', p)",
            "function f(p, o = Qt.createQmlObject('x', p)) {}",
            "var f = ([o = Qt['createQmlObject']]) => o",
            "var {o = Qt.createQmlObject('x', p)} = {}",
            "var {[Qt.createQmlObject]: o} = {}",
            "for (var {o = Qt.createQmlObject} of []) {}",
            "for (var [o = Qt.createQmlObject] in {}) {}",
            "try {} catch ({o = Qt.createQmlObject}) {}",
            "class A extends Qt.createQmlObject('x', null) {}",
            "class A { constructor(o = Qt.createQmlObject) {} }",
            "class A { [Qt.createQmlObject]() {} }",
            "class A { [Qt.createQmlObject] }",
            "class A { m(o = Qt.createQmlObject) {} }",
        ] {
            assert!(
                reads_create_qml_object(&lower_program(src).unwrap()),
                "{src}"
            );
        }
        assert!(!reads_create_qml_object(
            &lower_program("function f(a = Qt.binding, {b} = {}) {} class A extends B {}").unwrap()
        ));
        for bad in [
            "var = ;",
            "-2 ** 2",
            "a ?? b || c",
            "a || b ?? c",
            "if (",
            "({a: 1,,})",
            "for (let x of) {}",
            "x => { ",
            "`unterminated",
        ] {
            assert!(lower_program(bad).is_err(), "{bad}");
        }
        assert_eq!(
            lower(&script("1 +", crate::ScriptKind::Expression)).unwrap_err(),
            "unparsable"
        );
        assert!(lower(&script("{ a: 1, }}", crate::ScriptKind::Block)).is_err());
        // Qt directives are blanked; other leading `.` lines continue a member chain.
        assert_eq!(
            ev(".pragma library\n.import \"x.js\" as X\nvar s = 'ab'\n  .toUpperCase()\nreturn s"),
            "AB"
        );
    }

    #[test]
    fn binding_scripts() {
        use crate::ScriptKind::{Block, Expression, Function};
        assert_eq!(binding("a + b", Expression), ["(return (+ a b))"]);
        // A lone string literal is a value, not a directive prologue.
        assert_eq!(
            binding("'use strict'", Expression),
            ["(return \"use strict\")"]
        );
        assert_eq!(binding("{a: 1}", Expression), ["(return {a: 1})"]);
        assert_eq!(binding("a // trailing comment", Expression), ["(return a)"]);
        assert_eq!(binding("(x as any).y", Expression), ["(return (. x y))"]);
        // Statements fall back to the block wrap; the completion value is returned.
        assert_eq!(
            binding("var t = 1; t + 1", Expression),
            ["(var t=1)", "(return (+ t 1))"]
        );
        assert_eq!(
            binding("if (c) 1; else { f(); 2 }", Block),
            ["(if c (return 1) {(call f); (return 2)})"]
        );
        assert_eq!(binding("{ a; b }", Block), ["{a; (return b)}"]);
        assert_eq!(binding("return 3", Block), ["(return 3)"]);
        assert_eq!(binding("for (;;) break", Block), ["(for _ _ _ (break))"]);
        assert_eq!(
            binding("function f(a) { return a + g }", Function),
            ["(function f (a@0.0) {(return (+ a@0.0 g))})"]
        );
        let run_binding = |src: &str, kind| run(&lower(&script(src, kind)).unwrap());
        assert_eq!(run_binding("1 + 2 * 3", Expression), "7");
        assert_eq!(run_binding("var t = 2; t * 5", Block), "10");
        assert_eq!(run_binding("if (0) 'a'; else 'b'", Block), "b");
        assert_eq!(
            run_binding("[1, 2].map(x => x * 2).join()", Expression),
            "2,4"
        );
        // Each source is lowered once and shared.
        let sc = script("q + 1", Expression);
        assert!(Rc::ptr_eq(
            &lower_rc(&sc).unwrap(),
            &lower_rc(&script("q + 1", Expression)).unwrap()
        ));
        assert!(!Rc::ptr_eq(
            &lower_rc(&sc).unwrap(),
            &lower_rc(&script("q + 2", Expression)).unwrap()
        ));
    }

    #[test]
    fn literals() {
        let cases = [
            ("1e3", "1000"),
            ("0x1F", "31"),
            ("0b101", "5"),
            ("0o17", "15"),
            (".5", "0.5"),
            ("1_000", "1000"),
            ("('a\\'b')", "\"a'b\""),
            ("(\"\\u0041\\x42\")", "\"AB\""),
            ("/a+b/gi", "/a+b/gi"),
            ("null", "null"),
            ("true", "true"),
            ("undefined", "undefined"),
            ("this", "this"),
        ];
        for (src, want) in cases {
            assert_eq!(one(src), want, "{src}");
        }
        ev_cases(&[
            ("/a+b/gi.test('xAAB')", "true"),
            ("'\\u{1F600}'.length", "2"),
            ("0.1 + 0.2", "0.30000000000000004"),
        ]);
    }
}
