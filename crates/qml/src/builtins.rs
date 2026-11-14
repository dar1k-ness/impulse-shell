//! Builtin library for the evaluator: ECMAScript subset used by the original + the QML `Qt` object.
use crate::color::{Color, Spec};
use crate::eval::{
    BoundFn, Callable, Hint, Interp, ItemIntegrity, NativeFn, PropKey, ProxyFn, R, define_accessor,
    fn_value, has_own, js_round, link_builtin_prototype, new_obj, set_hidden, set_own,
};
use crate::locale::{Fmt, Locale};
use crate::locale_num::Numeric;
use crate::qdatetime::{QDateTime, Spec as TimeSpec};
use crate::value::{
    CollIter, Internal, IterKind, JsStr, Obj, PERMANENT, READONLY, Value, num_to_string,
};
use std::{cell::RefCell, rc::Rc};

/// Where console output goes besides stderr: the shell's instance log, given the Qt message
/// level (0 debug, 1 info, 2 warning, 3 critical) and the joined line.
pub static CONSOLE_SINK: std::sync::OnceLock<fn(u8, &str)> = std::sync::OnceLock::new();

/// The [`CONSOLE_SINK`] level of a `qmlWarning` line: a warning that is no `console.*` call
/// (Qt gives it the message category `default`, not `qml`).
pub const QML_WARNING: u8 = 4;

/// ToString of a `console` argument that cannot throw (V4's `Value::toQStringNoThrow`): what
/// the conversion threw is printed in the argument's place, and nothing when that throws too.
/// A Symbol prints its description.
fn console_string(it: &mut Interp, v: &Value) -> String {
    if let Value::Symbol(s) = v {
        return s.descriptive();
    }
    match it.to_string(v) {
        Ok(s) => s,
        Err(crate::eval::Ex::Throw(Value::Symbol(s))) => s.descriptive(),
        Err(crate::eval::Ex::Throw(e)) => it.to_string(&e).unwrap_or_default(),
        Err(crate::eval::Ex::Short) => String::new(),
    }
}

/// One `console` argument as V4 prints it (`serializeArray` of qv4consoleobject.cpp): an Array
/// as `[a,b]` with nested Arrays in the same form and one already being printed further up as
/// `[Circular]`, without calling its `toString`; anything else through [`console_string`].
/// `path` holds the Arrays being printed.
fn console_arg(it: &mut Interp, v: &Value, path: &mut Vec<Obj>) -> String {
    let Value::Obj(o) = v else {
        return console_string(it, v);
    };
    let len = {
        let b = o.borrow();
        match &b.array {
            Some(a) if !matches!(b.internal, Internal::Arguments(_)) => a.len(),
            _ => return console_string(it, v),
        }
    };
    if path.iter().any(|p| Rc::ptr_eq(p, o)) {
        return "[Circular]".into();
    }
    path.push(o.clone());
    let mut out = String::from("[");
    for i in 0..len {
        if i > 0 {
            out.push(',');
        }
        // An element's conversion may shorten the array.
        let e = o.borrow().array.as_ref().and_then(|a| a.get(i).cloned());
        let e = e.unwrap_or(Value::Undefined);
        out.push_str(&crate::eval::on_js_stack(|| console_arg(it, &e, path)));
    }
    path.pop();
    out.push(']');
    out
}

/// What the `console` object keeps between calls, as QV4's ExecutionEngine does: the
/// `console.count` counters and the running `console.time` timers.
#[derive(Default)]
pub struct Console {
    counts: std::collections::HashMap<String, u32>,
    timers: std::collections::HashMap<String, std::time::Instant>,
}

/// One Qt message (`qDebug` .. `qCritical`): a line on stderr and in the instance log.
/// Nothing keeps the line afterwards.
fn message(level: u8, line: &str) {
    // Quickshell prints console output to its log.
    elog::eprintln!("console: {line}");
    if let Some(sink) = CONSOLE_SINK.get() {
        sink(level, line);
    }
}

/// The arguments of a console call as one line (QV4 `writeToConsole`).
fn console_line(it: &mut Interp, a: &[Value]) -> String {
    let mut path = Vec::new();
    a.iter()
        .map(|v| console_arg(it, v, &mut path))
        .collect::<Vec<_>>()
        .join(" ")
}

fn console_out(it: &mut Interp, a: &[Value], level: u8) -> R<Value> {
    let line = console_line(it, a);
    message(level, &line);
    Ok(Value::Undefined)
}

/// `Function.prototype.toString` of function `f` as V4 prints it: `function NAME() { [native
/// code] }` with the function's `name` read as a property (so an accessor or a proxy trap
/// runs) when that is a String, and no name otherwise.
pub(crate) fn function_to_string(it: &mut Interp, f: &Value) -> R<String> {
    let Value::Obj(o) = f else {
        return Ok(f.to_js_string());
    };
    // Printed without reading `name`: see `Value::to_js_string`.
    if matches!(
        o.borrow().call,
        Some(Callable::Item(..) | Callable::Bound(_)) | None
    ) {
        return Ok(f.to_js_string());
    }
    Ok(match it.get(f, "name")? {
        n @ (Value::Str(_) | Value::Str16(_)) => crate::value::function_string(&n.to_js_string()),
        _ => crate::value::function_string(""),
    })
}

/// The `console` methods of QV4's `ConsoleObject`, each a function named as its property.
/// The engine keeps no call stack, so `trace`, `assert` and `exception` print no frames.
const CONSOLE: [(&str, NativeFn); 13] = [
    ("debug", |it, _, a| console_out(it, a, 0)),
    ("log", |it, _, a| console_out(it, a, 0)),
    ("info", |it, _, a| console_out(it, a, 1)),
    ("warn", |it, _, a| console_out(it, a, 2)),
    ("error", |it, _, a| console_out(it, a, 3)),
    ("assert", |it, _, a| {
        let Some(cond) = a.first() else {
            return it.throw("Error", "console.assert(): Missing argument");
        };
        if !cond.truthy() {
            let line = console_line(it, &a[1..]);
            message(3, &line);
        }
        Ok(Value::Undefined)
    }),
    // Qt counts the calls of one source line; the engine knows no line, so a label counts.
    ("count", |it, _, a| {
        let name = a.first().map(Value::to_js_string).unwrap_or_default();
        let n = it.console.counts.entry(name.clone()).or_default();
        *n += 1;
        message(0, &format!("{name}: {n}"));
        Ok(Value::Undefined)
    }),
    ("profile", |_, _, _| {
        message(
            2,
            "Cannot start profiling because debug service is disabled. Start with -qmljsdebugger=port:XXXXX.",
        );
        Ok(Value::Undefined)
    }),
    ("profileEnd", |_, _, _| {
        message(
            2,
            "Ignoring console.profileEnd(): the debug service is disabled.",
        );
        Ok(Value::Undefined)
    }),
    ("time", |it, _, a| {
        let [name] = a else {
            return it.throw("Error", "console.time(): Invalid arguments");
        };
        it.console
            .timers
            .insert(name.to_js_string(), std::time::Instant::now());
        Ok(Value::Undefined)
    }),
    ("timeEnd", |it, _, a| {
        let [name] = a else {
            return it.throw("Error", "console.timeEnd(): Invalid arguments");
        };
        let name = name.to_js_string();
        if let Some(t) = it.console.timers.remove(&name) {
            message(0, &format!("{name}: {}ms", t.elapsed().as_millis()));
        }
        Ok(Value::Undefined)
    }),
    ("trace", |it, _, a| {
        if !a.is_empty() {
            return it.throw("Error", "console.trace(): Invalid arguments");
        }
        Ok(Value::Undefined)
    }),
    ("exception", |it, _, a| {
        if a.is_empty() {
            return it.throw("Error", "console.exception(): Missing argument");
        }
        console_out(it, a, 3)
    }),
];

/// A new Map (`is_map`) or Set with prototype `proto` (the intrinsic one when `None`), filled
/// from the iterable `init`.
pub(crate) fn new_coll(
    it: &mut Interp,
    is_map: bool,
    init: &Value,
    proto: Option<Obj>,
) -> R<Value> {
    let o = new_obj();
    o.borrow_mut().proto = proto;
    let Value::Obj(store) = Value::array(Vec::new()) else {
        unreachable!()
    };
    o.borrow_mut().internal = if is_map {
        Internal::Map(store)
    } else {
        Internal::Set(store)
    };
    let v = Value::Obj(o.clone());
    if matches!(init, Value::Undefined | Value::Null) {
        return Ok(v);
    }
    // The Map/Set constructor (ECMA-262 §24.1.1.1, §24.2.2.1): the current `set`/`add`, checked
    // once, called per entry as the iterator steps; an abrupt completion closes the iterator
    // (AddEntriesFromIterable §24.1.1.2).
    let name = if is_map { "set" } else { "add" };
    let adder = it.get(&v, name)?;
    if crate::eval::callable(&adder).is_none() {
        return it.throw("TypeError", "Type error");
    }
    let mut src = it.get_iterator(init)?;
    while let Some(x) = it.iter_step(&mut src)? {
        if let Err(e) = coll_add(it, is_map, &v, &adder, x) {
            return Err(it.iter_close_throw(&mut src, e));
        }
    }
    Ok(v)
}

fn coll_add(it: &mut Interp, is_map: bool, coll: &Value, adder: &Value, x: Value) -> R<()> {
    let args = if is_map {
        if !matches!(x, Value::Obj(_)) {
            return it.throw("TypeError", "Type error");
        }
        vec![it.get(&x, "0")?, it.get(&x, "1")?]
    } else {
        vec![x]
    };
    it.call(adder, coll.clone(), &args)?;
    Ok(())
}

/// SameValueZero identity of a Map key / Set value, for the hash index.
#[derive(PartialEq, Eq, Hash)]
enum CollKey {
    Undefined,
    Null,
    Bool(bool),
    Num(u64),
    Str(Rc<str>),
    Str16(Rc<[u16]>),
    Obj(usize),
    /// A QML type, by its attached type: equal as `===` has them (`types::same_type`).
    Type(Option<Rc<str>>),
    Item(usize),
    Native(u32),
    Sym(usize),
}

fn coll_key(v: &Value) -> Option<CollKey> {
    Some(match v {
        Value::Undefined | Value::Empty => CollKey::Undefined,
        Value::Null => CollKey::Null,
        Value::Bool(b) => CollKey::Bool(*b),
        Value::Num(n) if n.is_nan() => CollKey::Num(f64::NAN.to_bits()),
        Value::Num(n) => CollKey::Num((n + 0.0).to_bits()),
        Value::Str(s) => CollKey::Str(s.clone()),
        Value::Str16(u) => CollKey::Str16(u.clone()),
        // Values of value types compare by value: no stable hash, scan.
        Value::Obj(_) if crate::valuetypes::is_value(v) => return None,
        Value::Obj(o) => match crate::types::qml_type(o) {
            Some(t) => CollKey::Type(t.attached.clone()),
            None => CollKey::Obj(Rc::as_ptr(o) as *const () as usize),
        },
        Value::Item(i) => CollKey::Item(*i),
        Value::Native(n) => CollKey::Native(*n),
        Value::Symbol(s) => CollKey::Sym(Rc::as_ptr(s) as usize),
        Value::Color(_) => return None,
    })
}

/// Hash index of a Map/Set store array, kept on the store's `native` slot. Each entry gets
/// an increasing id; `ids` parallels the array (so stays sorted) and a key's position is a
/// binary search, which keeps `delete` from renumbering every later key.
#[derive(Default)]
struct CollIndex {
    ids: Vec<u64>,
    map: rustc_hash::FxHashMap<CollKey, u64>,
    next: u64,
}

impl CollIndex {
    fn pos(&self, k: &CollKey) -> Option<usize> {
        self.map
            .get(k)
            .and_then(|id| self.ids.binary_search(id).ok())
    }
    fn push(&mut self, k: Option<CollKey>) {
        if let Some(k) = k {
            self.map.insert(k, self.next);
        }
        self.ids.push(self.next);
        self.next += 1;
    }
}

/// Part `i` (0 the key, 1 the value) of entry `x` of a Map store (a `[key, value]` array); a
/// Set's entry is its value.
fn entry_part(x: &Value, is_map: bool, i: usize) -> Value {
    if !is_map {
        return x.clone();
    }
    match x {
        Value::Obj(p) => p
            .borrow()
            .array
            .as_ref()
            .and_then(|v| v.get(i).cloned())
            .unwrap_or_default(),
        _ => Value::Undefined,
    }
}

/// The hash index of Map/Set store `store`, made on first use.
fn coll_index(store: &Obj, is_map: bool) -> Rc<RefCell<CollIndex>> {
    let cur = store
        .borrow()
        .native
        .clone()
        .and_then(|n| n.downcast::<RefCell<CollIndex>>().ok());
    cur.unwrap_or_else(|| {
        let mut m = CollIndex::default();
        for x in store.borrow().array.as_ref().unwrap() {
            m.push(coll_key(&entry_part(x, is_map, 0)));
        }
        let ix = Rc::new(RefCell::new(m));
        store.borrow_mut().native = Some(ix.clone());
        ix
    })
}

/// The next item of a Map or Set iteration over `store` from entry id `cursor`, which then
/// moves past it (%MapIteratorPrototype%.next, %SetIteratorPrototype%.next, ECMA-262
/// §24.1.5.2.1, §24.2.5.2.1): entries keep their ids, so one added meanwhile is still ahead
/// and one deleted is gone; `None` at the end.
pub(crate) fn coll_step(
    store: &Obj,
    is_map: bool,
    kind: IterKind,
    cursor: &mut u64,
) -> Option<Value> {
    let index = coll_index(store, is_map);
    let (x, id) = {
        let ix = index.borrow();
        let pos = ix.ids.partition_point(|&id| id < *cursor);
        let x = store.borrow().array.as_ref()?.get(pos).cloned()?;
        (x, *ix.ids.get(pos)?)
    };
    *cursor = id + 1;
    let k = entry_part(&x, is_map, 0);
    Some(match (kind, is_map) {
        (IterKind::Keys, _) | (IterKind::Values, false) => k,
        (IterKind::Values, true) => entry_part(&x, true, 1),
        (IterKind::Entries, true) => Value::array(vec![k, entry_part(&x, true, 1)]),
        (IterKind::Entries, false) => Value::array(vec![k.clone(), k]),
    })
}

/// `Map.prototype.{entries, keys, values}` and `Set.prototype.{entries, values}`
/// (CreateMapIterator / CreateSetIterator, ECMA-262 §24.1.5.1, §24.2.5.1).
pub(crate) fn coll_iter(it: &mut Interp, this: &Value, is_map: bool, kind: IterKind) -> R<Value> {
    let store = match this {
        Value::Obj(o) => match (&o.borrow().internal, is_map) {
            (Internal::Map(s), true) | (Internal::Set(s), false) => Some(s.clone()),
            _ => None,
        },
        _ => None,
    };
    let Some(store) = store else {
        return it.throw("TypeError", "Type error");
    };
    let o = new_obj();
    {
        let mut b = o.borrow_mut();
        b.proto = Some(if is_map {
            it.sym.map_iter_proto.clone()
        } else {
            it.sym.set_iter_proto.clone()
        });
        b.internal = Internal::CollIter(Box::new(CollIter {
            store: Some(store),
            map: is_map,
            kind,
            cursor: 0,
        }));
    }
    Ok(Value::Obj(o))
}

/// %MapIteratorPrototype%.next / %SetIteratorPrototype%.next (ECMA-262 §24.1.5.2.1,
/// §24.2.5.2.1).
pub(crate) fn coll_iter_next<const MAP: bool>(
    it: &mut Interp,
    this: &Value,
    _: &[Value],
) -> R<Value> {
    let state = match this {
        Value::Obj(o) => match &o.borrow().internal {
            Internal::CollIter(c) if c.map == MAP => Some((c.store.clone(), c.kind, c.cursor)),
            _ => None,
        },
        _ => None,
    };
    let Some((store, kind, mut cursor)) = state else {
        return it.throw(
            "TypeError",
            format!(
                "Not a {} Iterator instance",
                if MAP { "Map" } else { "Set" }
            ),
        );
    };
    let v = store.and_then(|s| coll_step(&s, MAP, kind, &mut cursor));
    if let Value::Obj(o) = this
        && let Internal::CollIter(c) = &mut o.borrow_mut().internal
    {
        c.cursor = cursor;
        if v.is_none() {
            c.store = None;
        }
    }
    let done = v.is_none();
    Ok(crate::symbol::iter_result(v.unwrap_or_default(), done))
}

fn coll_method(it: &mut Interp, o: &Obj, name: &str, a: &[Value]) -> R<Option<Value>> {
    let (store, is_map) = {
        let b = o.borrow();
        match &b.internal {
            Internal::Map(s) => (s.clone(), true),
            Internal::Set(s) => (s.clone(), false),
            _ => return Ok(None),
        }
    };
    let key_of = |x: &Value| entry_part(x, is_map, 0);
    let index = coll_index(&store, is_map);
    let mut k = arg(a, 0);
    if let Value::Num(n) = &mut k {
        // `set`/`add` store -0 as +0 (ECMA-262 §24.1.3.9 step 5, §24.2.3.1 step 4); -0 + +0
        // is +0, and the lookups compare by SameValueZero, which both already satisfy.
        *n += 0.0;
    }
    let ck = coll_key(&k);
    let pos = match &ck {
        Some(ck) => index.borrow().pos(ck),
        None => store
            .borrow()
            .array
            .as_ref()
            .unwrap()
            .iter()
            .position(|x| key_of(x).strict_eq(&k)),
    };
    let r = match name {
        "get" if is_map => pos
            .map(|i| it.get(&store.borrow().array.as_ref().unwrap()[i].clone(), "1"))
            .transpose()?
            .or(Some(Value::Undefined)),
        "has" => Some(Value::Bool(pos.is_some())),
        "set" | "add" if (name == "set") == is_map => {
            let entry = if is_map {
                Value::array(vec![k.clone(), arg(a, 1)])
            } else {
                k.clone()
            };
            let mut b = store.borrow_mut();
            let arr = b.array.as_mut().unwrap();
            match pos {
                Some(i) => arr[i] = entry,
                None => {
                    index.borrow_mut().push(ck);
                    arr.push(entry)
                }
            }
            Some(Value::Obj(o.clone()))
        }
        "delete" => {
            if let Some(i) = pos {
                let mut b = store.borrow_mut();
                let arr = b.array.as_mut().unwrap();
                arr.remove(i);
                let mut ix = index.borrow_mut();
                ix.ids.remove(i);
                if let Some(ck) = ck {
                    ix.map.remove(&ck);
                }
            }
            Some(Value::Bool(pos.is_some()))
        }
        "clear" => {
            store.borrow_mut().array.as_mut().unwrap().clear();
            let mut ix = index.borrow_mut();
            ix.ids.clear();
            ix.map.clear();
            Some(Value::Undefined)
        }
        "forEach" => {
            if crate::eval::callable(&k).is_none() {
                return it.type_error();
            }
            // Live like an iterator: entries added during the walk are visited, deleted
            // ones skipped (ECMA-262 §24.1.3.5, §24.2.3.6).
            let mut cursor = 0;
            while let Some(x) = coll_step(&store, is_map, IterKind::Entries, &mut cursor) {
                let (kk, vv) = (entry_part(&x, true, 0), entry_part(&x, true, 1));
                it.call(&k, arg(a, 1), &[vv, kk, Value::Obj(o.clone())])?;
            }
            Some(Value::Undefined)
        }
        _ => None,
    };
    Ok(r)
}

/// `new Map(iterable)`.
fn map_ctor(it: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
    if !std::mem::take(&mut it.constructing) {
        return it.throw("TypeError", "(Weak)Map requires new");
    }
    // OrdinaryCreateFromConstructor(NewTarget, "%Map.prototype%") before the entries are
    // added (ECMA-262 §24.1.1.1 step 2), so a subclass's adder is the one called.
    let proto = it.new_target_proto.take();
    new_coll(it, true, &arg(a, 0), proto)
}

/// `new Set(iterable)`.
fn set_ctor(it: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
    if !std::mem::take(&mut it.constructing) {
        return it.throw("TypeError", "Set requires new");
    }
    // OrdinaryCreateFromConstructor(NewTarget, "%Set.prototype%") before the entries are
    // added (ECMA-262 §24.2.2.1 step 2), so a subclass's adder is the one called.
    let proto = it.new_target_proto.take();
    new_coll(it, false, &arg(a, 0), proto)
}

/// `get Map.prototype.size` / `get Set.prototype.size` (ECMA-262 §24.1.3.10, §24.2.3.9): the
/// entry count of a Map (a Set), a TypeError for any other receiver.
fn coll_size<const MAP: bool>(it: &mut Interp, this: &Value, _: &[Value]) -> R<Value> {
    if let Value::Obj(o) = this {
        let b = o.borrow();
        if let (Internal::Map(s), true) | (Internal::Set(s), false) = (&b.internal, MAP) {
            return Ok(Value::Num(
                s.borrow().array.as_ref().map_or(0, Vec::len) as f64
            ));
        }
    }
    it.throw("TypeError", "Type error")
}

fn arg(a: &[Value], i: usize) -> Value {
    a.get(i).cloned().unwrap_or_default()
}

/// ToNumber of argument `i` (ECMA-262 §7.1.4), through ToPrimitive for objects.
fn num(it: &mut Interp, a: &[Value], i: usize) -> R<f64> {
    it.to_numeric(&arg(a, i))
}

/// ToNumber of every argument, in order, before any is used (as `Math.max` and kin require).
fn nums(it: &mut Interp, a: &[Value]) -> R<Vec<f64>> {
    a.iter().map(|v| it.to_numeric(v)).collect()
}

/// A built-in function object named `name` taking `length` arguments (ECMA-262
/// CreateBuiltinFunction); with no `length` of its own it has 0.
pub fn native(name: &'static str, length: u32, f: NativeFn) -> Value {
    let v = fn_value(Callable::Native(name, f));
    if let (Value::Obj(o), 1..) = (&v, length) {
        set_length(o, length);
    }
    v
}

/// A built-in constructor's `length`: read-only and configurable (ECMA-262 §18).
fn set_length(f: &Obj, length: u32) {
    set_hidden(f, "length", Value::Num(length.into()));
    f.borrow_mut().set_attr("length", READONLY);
}

/// A built-in value property: `{ [[Writable]]: false, [[Enumerable]]: false,
/// [[Configurable]]: false }` (ECMA-262 §21.3.1, §21.1.2).
fn set_constant(o: &Obj, name: &str, v: Value) {
    set_hidden(o, name, v);
    o.borrow_mut().set_attr(name, READONLY | PERMANENT);
}

/// A built-in function property: non-enumerable (ECMA-262 §18, as V4's built-ins).
fn nat(o: &Obj, name: &'static str, length: u32, f: NativeFn) {
    set_hidden(o, name, native(name, length, f));
}

/// The methods of %Object.prototype% (ECMA-262 §20.1.3), which every object inherits through
/// [`Interp::proto_of`] past its own prototype chain and built-in methods.
fn install_object_prototype(p: &Obj) {
    nat(p, "__defineGetter__", 2, |it, this, a| {
        legacy_accessor(it, this, a, true)
    });
    nat(p, "__defineSetter__", 2, |it, this, a| {
        legacy_accessor(it, this, a, false)
    });
    // Annex B §B.2.2.1: `__proto__`, an accessor for [[GetPrototypeOf]]/[[SetPrototypeOf]].
    let get = |it: &mut Interp, this: &Value, _: &[Value]| -> R<Value> {
        if matches!(this, Value::Undefined | Value::Null) {
            return coercible(it, this).map(drop).map(|()| Value::Undefined);
        }
        // The Easing type reads the accessor from %Object.prototype% and gets that.
        if let Value::Obj(o) = this
            && is_easing(&o.borrow())
        {
            return Ok(Value::Obj(it.object_proto.clone()));
        }
        prototype_of(it, this)
    };
    let set = |it: &mut Interp, this: &Value, a: &[Value]| -> R<Value> {
        coercible(it, this)?;
        let p = match arg(a, 0) {
            Value::Obj(p) => Some(p),
            Value::Null => None,
            _ => return Ok(Value::Undefined),
        };
        if let Value::Obj(o) = this
            && !crate::props::set_prototype(it, o, p.as_ref())?
        {
            return it.throw("TypeError", "Could not change prototype.".to_string());
        }
        Ok(Value::Undefined)
    };
    crate::eval::define_accessor(p, "__proto__", true, native("get __proto__", 0, get), false);
    crate::eval::define_accessor(
        p,
        "__proto__",
        false,
        native("set __proto__", 1, set),
        false,
    );
    nat(p, "hasOwnProperty", 1, |it, this, a| {
        let k = it.to_property_key(&arg(a, 0))?;
        to_object(it, this)?;
        Ok(Value::Bool(own_property(it, this, &k)?.is_some()))
    });
    nat(p, "isPrototypeOf", 1, |it, this, a| {
        let v = arg(a, 0);
        if !matches!(v, Value::Obj(_) | Value::Item(_)) {
            return Ok(Value::Bool(false));
        }
        to_object(it, this)?;
        let Value::Obj(o) = this else {
            return Ok(Value::Bool(false));
        };
        let mut cur = crate::props::get_prototype(it, &v)?;
        while let Some(p) = cur {
            if Rc::ptr_eq(&p, o) {
                return Ok(Value::Bool(true));
            }
            cur = crate::props::get_prototype(it, &Value::Obj(p))?;
        }
        Ok(Value::Bool(false))
    });
    nat(p, "propertyIsEnumerable", 1, |it, this, a| {
        let k = it.to_property_key(&arg(a, 0))?;
        to_object(it, this)?;
        Ok(Value::Bool(own_property(it, this, &k)? == Some(true)))
    });
    nat(p, "toLocaleString", 0, |it, this, _| {
        to_object(it, this)?;
        if let Some(r) = method(it, this, "toString", &[])? {
            return Ok(r);
        }
        let f = it.get(this, "toString")?;
        if crate::eval::callable(&f).is_none() {
            return it.throw("TypeError", "Type error");
        }
        it.call(&f, this.clone(), &[])
    });
    nat(p, "toString", 0, |it, this, _| {
        // IsArray of a proxy throws once it is revoked.
        crate::props::is_array(it, this)?;
        if !matches!(this, Value::Undefined | Value::Null)
            && let Value::Str(tag) =
                it.get_sym(this, &crate::value::WellKnown::ToStringTag.sym())?
        {
            return Ok(Value::str(&format!("[object {tag}]")));
        }
        let tag = match typed_list(it, this) {
            Some(true) => "QML List",
            Some(false) => "V4Sequence",
            None => builtin_tag(this),
        };
        Ok(Value::str(&format!("[object {tag}]")))
    });
    // Object.prototype.valueOf (ECMA-262 §20.1.3.7): ToObject(this value).
    nat(p, "valueOf", 0, |it, this, _| {
        to_object(it, this)?;
        Ok(it.to_object(this))
    });
}

/// The reflection functions of the Object constructor (ECMA-262 §20.1.2): prototypes,
/// property descriptors and integrity levels ([`crate::props`]).
fn install_object_reflection(object: &Obj) {
    use crate::props;
    nat(object, "getPrototypeOf", 1, |it, _, a| {
        let v = arg(a, 0);
        coercible(it, &v)?;
        prototype_of(it, &v)
    });
    nat(object, "setPrototypeOf", 2, |it, _, a| {
        let v = arg(a, 0);
        if matches!(v, Value::Undefined | Value::Null) {
            return it.type_error();
        }
        let p = proto_arg(it, &arg(a, 1))?;
        if let Value::Obj(o) = &v
            && !props::set_prototype(it, o, p.as_ref())?
        {
            return it.throw("TypeError", "Could not change prototype.");
        }
        Ok(v)
    });
    nat(object, "defineProperty", 3, |it, _, a| {
        // A color read from a property is a copy the definition is lost with.
        if let c @ Value::Color(_) = arg(a, 0) {
            it.to_property_key(&arg(a, 1))?;
            props::to_desc(it, &arg(a, 2))?;
            return Ok(c);
        }
        let o = target(it, &arg(a, 0))?;
        let k = it.to_property_key(&arg(a, 1))?;
        let d = props::to_desc(it, &arg(a, 2))?;
        // A type has no place for a new property and does not say so.
        if is_easing(&o.borrow()) {
            return Ok(Value::Obj(o));
        }
        define_or_throw(it, &o, &k, &d)?;
        Ok(arg(a, 0))
    });
    nat(object, "defineProperties", 2, |it, _, a| {
        let o = target(it, &arg(a, 0))?;
        define_properties(it, &o, &arg(a, 1))?;
        Ok(arg(a, 0))
    });
    nat(object, "getOwnPropertyNames", 1, |it, _, a| {
        let v = arg(a, 0);
        coercible(it, &v)?;
        let ks = props::own_keys(it, &v)?;
        Ok(Value::array(ks.iter().map(|k| Value::str(k)).collect()))
    });
    nat(object, "getOwnPropertyDescriptor", 2, |it, _, a| {
        let v = arg(a, 0);
        coercible(it, &v)?;
        let k = it.to_property_key(&arg(a, 1))?;
        // QV4::Sequence answers for no element and not for `length`.
        if let (Value::Obj(o), PropKey::Str(k)) = (&v, &k)
            && crate::valuetypes::is_readonly_sequence(&o.borrow())
            && (&**k == "length" || crate::eval::idx(k).is_some())
        {
            return Ok(Value::Undefined);
        }
        let d = props::own_desc_pk(it, &v, &k)?.map(|d| type_member_desc(&v, d));
        Ok(d.map_or(Value::Undefined, |d| props::from_desc(&d)))
    });
    nat(object, "getOwnPropertyDescriptors", 1, |it, _, a| {
        let v = arg(a, 0);
        coercible(it, &v)?;
        let out = new_obj();
        // QV4::Sequence's key iterator hands out each element; the descriptor made for
        // `length` after them still holds the last one.
        let sequence = match &v {
            Value::Obj(o) if crate::valuetypes::is_readonly_sequence(&o.borrow()) => {
                o.borrow().array.clone()
            }
            _ => None,
        };
        if let Some(items) = sequence {
            let last = items.last().cloned().unwrap_or_default();
            for (i, x) in items.into_iter().enumerate() {
                let d = props::Desc::data(x, true, true, true);
                set_own(&out, &i.to_string(), props::from_desc(&d));
            }
            let d = props::Desc::data(last, true, true, true);
            set_own(&out, "length", props::from_desc(&d));
            return Ok(Value::Obj(out));
        }
        for k in props::own_property_keys(it, &v)? {
            if let Some(d) = props::own_desc_pk(it, &v, &k)? {
                let d = type_member_desc(&v, d);
                match &k {
                    PropKey::Str(k) => set_own(&out, k, props::from_desc(&d)),
                    PropKey::Sym(s) => crate::eval::define_sym(&out, s, props::from_desc(&d), true),
                }
            }
        }
        Ok(Value::Obj(out))
    });
    nat(object, "preventExtensions", 1, |it, _, a| {
        match arg(a, 0) {
            Value::Obj(o) => {
                // V4 returns the object whatever a proxy's trap answers.
                props::prevent_extensions(it, &o)?;
            }
            Value::Item(id) => item_integrity(it, id, ItemIntegrity::NonExtensible),
            _ => {}
        }
        Ok(arg(a, 0))
    });
    nat(object, "seal", 1, |it, _, a| {
        match arg(a, 0) {
            Value::Obj(o) => {
                // V4 returns the object whatever a proxy's trap answers.
                props::set_integrity(it, &o, false)?;
            }
            Value::Item(id) => item_integrity(it, id, ItemIntegrity::Sealed),
            _ => {}
        }
        Ok(arg(a, 0))
    });
    nat(object, "freeze", 1, |it, _, a| {
        match arg(a, 0) {
            Value::Obj(o) => {
                // V4 returns the object whatever a proxy's trap answers.
                props::set_integrity(it, &o, true)?;
            }
            Value::Item(id) => item_integrity(it, id, ItemIntegrity::Frozen),
            _ => {}
        }
        Ok(arg(a, 0))
    });
    nat(object, "isExtensible", 1, |it, _, a| {
        Ok(Value::Bool(match arg(a, 0) {
            Value::Obj(o) => props::is_extensible(it, &o)?,
            Value::Item(id) => !it.item_integrity.contains_key(&id),
            Value::Color(_) => true,
            _ => false,
        }))
    });
    nat(object, "isSealed", 1, |it, _, a| {
        Ok(Value::Bool(match arg(a, 0) {
            Value::Obj(o) => props::test_integrity(it, &o, false)?,
            Value::Item(id) => it.item_integrity.get(&id) >= Some(&ItemIntegrity::Sealed),
            Value::Color(_) => false,
            _ => true,
        }))
    });
    nat(object, "isFrozen", 1, |it, _, a| {
        Ok(Value::Bool(match arg(a, 0) {
            Value::Obj(o) => props::test_integrity(it, &o, true)?,
            Value::Item(id) => it.item_integrity.contains_key(&id),
            Value::Color(_) => false,
            _ => true,
        }))
    });
}

/// The Reflect object (ECMA-262 §28.1): the internal methods of objects as functions.
fn reflect_object() -> Obj {
    use crate::props;
    let r = new_obj();
    nat(&r, "apply", 3, |it, _, a| {
        let f = arg(a, 0);
        if crate::eval::callable(&f).is_none() {
            return it.throw("TypeError", "Type error");
        }
        let args = list_from_array_like(it, &arg(a, 2))?;
        it.call(&f, arg(a, 1), &args)
    });
    // Reflect.construct (§28.1.2): IsConstructor of the target and of newTarget.
    nat(&r, "construct", 2, |it, _, a| {
        let f = arg(a, 0);
        if !crate::proxy::is_constructor(&f) {
            return it.throw("TypeError", "Type error");
        }
        let nt = if a.len() > 2 { arg(a, 2) } else { f.clone() };
        if !crate::proxy::is_constructor(&nt) {
            return it.throw("TypeError", "Type error");
        }
        let args = list_from_array_like(it, &arg(a, 1))?;
        it.construct_new(&f, &args, &nt)
    });
    nat(&r, "defineProperty", 3, |it, _, a| {
        let o = reflect_target(it, &arg(a, 0))?;
        let k = it.to_property_key(&arg(a, 1))?;
        let d = props::to_desc(it, &arg(a, 2))?;
        let own = match &o {
            Value::Obj(ob) => Some(ob.clone()),
            Value::Item(i) => item_own(it, *i),
            _ => None,
        };
        Ok(Value::Bool(match own {
            Some(ob) => props::define_pk(it, &ob, &k, &d)?,
            None => false,
        }))
    });
    nat(&r, "deleteProperty", 2, |it, _, a| {
        let o = reflect_target(it, &arg(a, 0))?;
        let k = it.to_property_key(&arg(a, 1))?;
        Ok(Value::Bool(match &o {
            Value::Obj(ob) => props::delete_pk(it, ob, &k)?,
            _ => false,
        }))
    });
    nat(&r, "get", 2, |it, _, a| {
        let t = arg(a, 0);
        reflect_target(it, &t)?;
        let receiver = if a.len() > 2 { arg(a, 2) } else { t.clone() };
        match (it.to_property_key(&arg(a, 1))?, &t) {
            (PropKey::Str(k), Value::Obj(o)) if a.len() > 2 => it.get_from(o, &k, &receiver),
            (PropKey::Str(k), _) => it.get(&t, &k),
            (PropKey::Sym(s), Value::Obj(o)) => it.get_sym_from(Some(o.clone()), &s, &receiver),
            (PropKey::Sym(_), _) => Ok(Value::Undefined),
        }
    });
    nat(&r, "getOwnPropertyDescriptor", 2, |it, _, a| {
        let t = arg(a, 0);
        reflect_target(it, &t)?;
        let k = it.to_property_key(&arg(a, 1))?;
        let d = props::own_desc_pk(it, &t, &k)?.map(|d| type_member_desc(&t, d));
        Ok(d.map_or(Value::Undefined, |d| props::from_desc(&d)))
    });
    nat(&r, "getPrototypeOf", 1, |it, _, a| {
        let t = arg(a, 0);
        reflect_target(it, &t)?;
        prototype_of(it, &t)
    });
    nat(&r, "has", 2, |it, _, a| {
        let t = arg(a, 0);
        reflect_target(it, &t)?;
        let k = it.to_property_key(&arg(a, 1))?;
        Ok(Value::Bool(props::has_pk(it, &t, &k)?))
    });
    nat(&r, "isExtensible", 1, |it, _, a| {
        let t = arg(a, 0);
        reflect_target(it, &t)?;
        Ok(Value::Bool(match t {
            Value::Obj(o) => props::is_extensible(it, &o)?,
            Value::Item(id) => !it.item_integrity.contains_key(&id),
            _ => true,
        }))
    });
    nat(&r, "ownKeys", 1, |it, _, a| {
        let t = arg(a, 0);
        reflect_target(it, &t)?;
        let ks = props::own_property_keys(it, &t)?;
        Ok(Value::array(ks.iter().map(PropKey::to_value).collect()))
    });
    nat(&r, "preventExtensions", 1, |it, _, a| {
        Ok(Value::Bool(match reflect_target(it, &arg(a, 0))? {
            Value::Obj(o) => props::prevent_extensions(it, &o)?,
            Value::Item(id) => {
                item_integrity(it, id, ItemIntegrity::NonExtensible);
                true
            }
            _ => true,
        }))
    });
    nat(&r, "set", 3, |it, _, a| {
        let t = arg(a, 0);
        reflect_target(it, &t)?;
        let k = it.to_property_key(&arg(a, 1))?;
        let receiver = if a.len() < 4 { t.clone() } else { arg(a, 3) };
        props::set_with_receiver_pk(it, &t, &k, arg(a, 2), &receiver).map(Value::Bool)
    });
    nat(&r, "setPrototypeOf", 2, |it, _, a| {
        let t = arg(a, 0);
        reflect_target(it, &t)?;
        let p = proto_arg(it, &arg(a, 1))?;
        Ok(Value::Bool(match &t {
            Value::Obj(o) => props::set_prototype(it, o, p.as_ref())?,
            _ => false,
        }))
    });
    r
}

/// %Proxy% (ECMA-262 §28.2): a constructor without a `prototype`, and `Proxy.revocable`.
fn proxy_ctor() -> Value {
    let f = fn_value(Callable::Proxy(ProxyFn::Constructor));
    let Value::Obj(c) = &f else { unreachable!() };
    set_length(c, 2);
    nat(c, "revocable", 2, |it, _, a| {
        let p = crate::proxy::create(it, &arg(a, 0), &arg(a, 1))?;
        let revoke = fn_value(Callable::Proxy(ProxyFn::Revoke));
        if let Value::Obj(r) = &revoke {
            r.borrow_mut().internal = Internal::RevocableProxy(Some(p.clone()));
        }
        let o = new_obj();
        set_own(&o, "proxy", Value::Obj(p));
        set_own(&o, "revoke", revoke);
        Ok(Value::Obj(o))
    });
    f
}

/// The target of `Reflect.<name>`, which must be an object.
fn reflect_target(it: &Interp, v: &Value) -> R<Value> {
    match v {
        Value::Obj(_) | Value::Item(_) | Value::Color(_) => Ok(v.clone()),
        _ => it.throw("TypeError", "Type error"),
    }
}

/// CreateListFromArrayLike (ECMA-262 §7.3.18).
fn list_from_array_like(it: &mut Interp, v: &Value) -> R<Vec<Value>> {
    if !matches!(v, Value::Obj(_)) {
        return it.throw("TypeError", "Type error");
    }
    let n = it.get(v, "length")?;
    let n = it.to_numeric(&n)?;
    let len = if n > 0.0 {
        crate::eval::array_len(it, n.trunc().min(4294967295.0))?
    } else {
        0
    };
    (0..len).map(|i| it.get(v, &i.to_string())).collect()
}

/// `Object.preventExtensions`, `seal` or `freeze` of a QML object: the level only rises.
fn item_integrity(it: &mut Interp, id: usize, level: ItemIntegrity) {
    let l = it.item_integrity.entry(id).or_insert(level);
    *l = level.max(*l);
    if let Some(own) = it.host.extras(id, false) {
        own.borrow_mut().non_extensible = true;
    }
}

/// The JavaScript properties of the wrapper of QML object `id`, made on first use: what
/// `Object.defineProperty` writes. They take no new name once the object is not extensible.
fn item_own(it: &mut Interp, id: usize) -> Option<Obj> {
    let own = it.host.extras(id, true)?;
    if it.item_integrity.contains_key(&id) {
        own.borrow_mut().non_extensible = true;
    }
    Some(own)
}

/// RequireObjectCoercible (ECMA-262 §7.2.1) of the argument of `Object.<name>`.
fn coercible(it: &Interp, v: &Value) -> R<()> {
    match v {
        Value::Undefined | Value::Null => it.no_object(v),
        _ => Ok(()),
    }
}

/// The object `Object.<name>` defines properties on (a TypeError for anything else).
fn target(it: &mut Interp, v: &Value) -> R<Obj> {
    let own = match v {
        Value::Obj(o) => Some(o.clone()),
        // A QML object: the properties of its wrapper.
        Value::Item(i) => item_own(it, *i),
        _ => None,
    };
    match own {
        Some(o) => Ok(o),
        None => it.throw("TypeError", "Type error"),
    }
}

/// A prototype argument: an object or null (a TypeError for anything else).
fn proto_arg(it: &Interp, p: &Value) -> R<Option<Obj>> {
    match p {
        Value::Obj(p) => Ok(Some(p.clone())),
        Value::Null => Ok(None),
        _ => it.type_error(),
    }
}

/// [[GetPrototypeOf]] of ToObject(`v`) as a value: the prototype or null.
pub(crate) fn prototype_of(it: &mut Interp, v: &Value) -> R<Value> {
    Ok(crate::props::get_prototype(it, v)?.map_or(Value::Null, Value::Obj))
}

/// DefinePropertyOrThrow (ECMA-262 §7.3.8).
fn define_or_throw(it: &mut Interp, o: &Obj, k: &PropKey, d: &crate::props::Desc) -> R<()> {
    if crate::props::define_pk(it, o, k, d)? {
        return Ok(());
    }
    it.throw("TypeError", "Type error")
}

/// The descriptor script is given for own property descriptor `d` of `v`: a member of the
/// `Easing` type (QQmlTypeWrapper::virtualGetOwnProperty) is described without its value.
fn type_member_desc(v: &Value, d: crate::props::Desc) -> crate::props::Desc {
    match v {
        Value::Obj(o) if is_easing(&o.borrow()) => {
            crate::props::Desc::data(Value::Undefined, true, true, true)
        }
        _ => d,
    }
}

/// ObjectDefineProperties (ECMA-262 §20.1.2.3.1): every descriptor is read before any is
/// applied.
fn define_properties(it: &mut Interp, o: &Obj, props: &Value) -> R<()> {
    coercible(it, props)?;
    let mut ds = Vec::new();
    for k in crate::props::own_property_keys(it, props)? {
        if crate::props::own_desc_pk(it, props, &k)?.is_some_and(|d| d.enumerable == Some(true)) {
            let dv = it.get_pk(props, &k)?;
            ds.push((k, crate::props::to_desc(it, &dv)?));
        }
    }
    for (k, d) in ds {
        define_or_throw(it, o, &k, &d)?;
    }
    Ok(())
}

/// The TypeError ToObject (ECMA-262 §7.1.18) throws for `this` of an %Object.prototype% method.
fn to_object(it: &Interp, this: &Value) -> R<()> {
    match this {
        Value::Undefined | Value::Null => it.no_object(this),
        _ => Ok(()),
    }
}

/// Whether `this` has own property `k`, and if so whether it is enumerable.
fn own_property(it: &mut Interp, this: &Value, k: &PropKey) -> R<Option<bool>> {
    let k = match k {
        PropKey::Str(k) => k.as_str(),
        PropKey::Sym(_) => {
            return Ok(crate::props::own_desc_pk(it, this, k)?.map(|d| d.enumerable == Some(true)));
        }
    };
    Ok(match this {
        Value::Obj(o)
            if o.borrow().is_proxy()
                || crate::types::qml_type(o).is_some_and(|t| t.attached.is_none()) =>
        {
            crate::props::own_desc(it, this, k)?.map(|d| d.enumerable == Some(true))
        }
        Value::Obj(o) => {
            if crate::valuetypes::is_own_method(&o.borrow(), k) {
                return Ok(Some(true));
            }
            // An index written to a value type is kept beside it.
            if crate::eval::idx(k).is_some()
                && let Some(m) = crate::valuetypes::members(o)
            {
                return own_property(it, &Value::Obj(m), &PropKey::Str(k.into()));
            }
            // QV4::Sequence answers for no element and not for `length`.
            if (k == "length" || crate::eval::idx(k).is_some())
                && crate::valuetypes::is_readonly_sequence(&o.borrow())
            {
                return Ok(None);
            }
            if !has_own(o, k) {
                return Ok(None);
            }
            crate::valuetypes::show(o);
            let b = o.borrow();
            let index = (b.array.as_ref())
                .is_some_and(|a| crate::eval::idx(k).is_some_and(|i| i < a.len() && !b.is_hole(i)));
            // A member of `Qt` or of a QML type: see `props::own_desc`.
            let wrapper = crate::qtobj::is_qt(&b) || crate::types::is_type_wrapper(&b);
            Some(index || wrapper || b.keys.iter().any(|x| **x == *k))
        }
        Value::Str(_) | Value::Str16(_) => match k {
            "length" => Some(false),
            _ => crate::eval::idx(k)
                .is_some_and(|i| i < crate::utf16::len_of(this))
                .then_some(true),
        },
        // V4's QObjectWrapper reports every member it resolves as an own enumerable property.
        Value::Item(id) => it.host.has_member(*id, k).then_some(true),
        Value::Color(_) => crate::valuetypes::is_color_own(k).then_some(true),
        _ => None,
    })
}

/// The builtinTag of `Object.prototype.toString` (ECMA-262 §20.1.3.6), which a string
/// `@@toStringTag` overrides; the built-in iterators and weak collections give their tag.
/// Whether `v` is the value read from a `list<T>` property, and then whether `T` is an
/// object type (a QQmlListProperty in Qt, else a sequence wrapper). Either is kept as an
/// array here; `Array.isArray` and `Object.prototype.toString` tell them apart as Qt does.
fn typed_list(it: &mut Interp, v: &Value) -> Option<bool> {
    let Value::Obj(o) = v else { return None };
    let (i, p) = {
        let b = o.try_borrow().ok()?;
        b.array.as_ref()?;
        b.owner.clone()?
    };
    let ty = it.host.prop_type(i, &p)?;
    let of = ty.strip_prefix("list<")?;
    Some(of.starts_with(|c: char| c.is_ascii_uppercase()))
}

fn builtin_tag(this: &Value) -> &'static str {
    match this {
        Value::Undefined => "Undefined",
        Value::Null => "Null",
        Value::Bool(_) => "Boolean",
        Value::Num(_) => "Number",
        Value::Str(_) | Value::Str16(_) => "String",
        Value::Symbol(_) => "Symbol",
        // The class name of V4's wrapper of a QObject.
        Value::Item(_) => "V4QObjectWrapper",
        Value::Obj(o) => {
            let b = o.borrow();
            if crate::types::is_type_wrapper(&b) || is_easing(&b) {
                return "QMLTypeWrapper";
            }
            // V4 names the class of a proxy, whatever its target is. The namespace object of
            // a JS file is a plain object there.
            if b.is_proxy() && !crate::eval::is_script_namespace(&b) {
                return "ProxyObject";
            }
            if crate::valuetypes::is_readonly_sequence(&b) {
                return "V4Sequence";
            }
            if crate::valuetypes::is_variant_map(&b) {
                return "V4ReferenceObject";
            }
            if b.array.is_some() {
                return "Array";
            }
            if b.call.is_some() || b.builtin_ctor {
                return "Function";
            }
            match b.internal {
                Internal::ValueType(_)
                | Internal::Locale(..)
                | Internal::AnchorLine(_)
                | Internal::ScriptString(_) => "QMLValueTypeWrapper",
                Internal::Date(_) => "Date",
                Internal::RegExp { .. } => "RegExp",
                Internal::Error => "Error",
                Internal::Map(_) => "Map",
                Internal::Set(_) => "Set",
                Internal::Arguments(_) => "Arguments",
                Internal::Primitive(ref p) => builtin_tag(p),
                Internal::Generator(_) => "Generator",
                Internal::Promise(_) => "Promise",
                Internal::ArrayBuffer(_) | Internal::DetachedArrayBuffer => "ArrayBuffer",
                Internal::SharedArrayBuffer(_) => "SharedArrayBuffer",
                Internal::DataView(_) => "DataView",
                Internal::TypedArray(ref t) => t.kind.name(),
                Internal::Symbol(_) => "Symbol",
                Internal::WeakMap(_) => "WeakMap",
                Internal::WeakSet(_) => "WeakSet",
                Internal::VariantMap => "V4ReferenceObject",
                _ => crate::url::tag(&b).unwrap_or("Object"),
            }
        }
        Value::Color(_) => "QMLValueTypeWrapper",
        _ => "Object",
    }
}

/// A method of the QML `Qt` object, which V4 lists among its enumerable properties.
fn qt_method(o: &Obj, name: &'static str, f: NativeFn) {
    let v = fn_value(Callable::Native(name, f));
    // V4 gives a method of `Qt` no `name`.
    if let Value::Obj(m) = &v {
        m.borrow_mut().set_attr("name", crate::value::ABSENT);
    }
    set_own(o, name, v);
}

fn u16s(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// A string's UTF-16 code units; ASCII strings index their bytes directly instead of copying.
enum Units<'a> {
    Ascii(&'a str),
    Wide(Wide<'a>),
}

/// The code units of a string that is not ASCII.
enum Wide<'a> {
    /// Those of a string held as UTF-16.
    Held(&'a [u16]),
    /// A short string's, encoded for this call.
    Own(Vec<u16>),
    /// A long string's, encoded once for every call on it ([`crate::utf16`]).
    Shared(Rc<[u16]>),
}

impl std::ops::Deref for Wide<'_> {
    type Target = [u16];
    fn deref(&self) -> &[u16] {
        match self {
            Self::Held(u) => u,
            Self::Own(u) => u,
            Self::Shared(u) => u,
        }
    }
}

impl<'a> Units<'a> {
    /// Those of String value `v`.
    fn of(v: &'a Value) -> Self {
        match v {
            Value::Str(s) if crate::utf16::is_ascii(s) => Self::Ascii(s),
            Value::Str(s) => Self::Wide(match crate::utf16::shared(s) {
                Some(u) => Wide::Shared(u),
                None => Wide::Own(u16s(s)),
            }),
            Value::Str16(u) => Self::Wide(Wide::Held(u)),
            _ => Self::Ascii(""),
        }
    }
    fn len(&self) -> usize {
        match self {
            Self::Ascii(s) => s.len(),
            Self::Wide(u) => u.len(),
        }
    }
    fn unit(&self, i: usize) -> u16 {
        match self {
            Self::Ascii(s) => s.as_bytes()[i] as u16,
            Self::Wide(u) => u[i],
        }
    }
    fn slice(&self, b: usize, e: usize) -> Value {
        match self {
            Self::Ascii(s) => Value::str(&s[b..e]),
            Self::Wide(u) => from16(&u[b..e]),
        }
    }
    /// First match of `n` at or after `from` (<= len).
    fn find(&self, n: JsStr, from: usize) -> Option<usize> {
        match (self, n) {
            (Self::Ascii(s), JsStr::Utf8(n)) => s[from..].find(n).map(|i| i + from),
            (Self::Ascii(_), JsStr::Utf16(_)) => None,
            (Self::Wide(u), n) => {
                let n = n.units();
                (from..=u.len().checked_sub(n.len())?).find(|&i| u[i..i + n.len()] == n[..])
            }
        }
    }
    /// Last match of `n` starting at or before `start`.
    fn rfind(&self, n: JsStr, start: usize) -> Option<usize> {
        match (self, n) {
            (Self::Ascii(s), JsStr::Utf8(n)) => {
                s[..start.saturating_add(n.len()).min(s.len())].rfind(n)
            }
            (Self::Ascii(_), JsStr::Utf16(_)) => None,
            (Self::Wide(u), n) => {
                let n = n.units();
                let hi = u.len().checked_sub(n.len())?.min(start);
                (0..=hi).rev().find(|&i| u[i..i + n.len()] == n[..])
            }
        }
    }
    fn starts_at(&self, n: JsStr, pos: usize) -> bool {
        match (self, n) {
            (Self::Ascii(s), JsStr::Utf8(n)) => s[pos..].starts_with(n),
            (Self::Ascii(_), JsStr::Utf16(_)) => false,
            (Self::Wide(u), n) => u[pos..].starts_with(&n.units()),
        }
    }
    fn ends_at(&self, n: JsStr, end: usize) -> bool {
        match (self, n) {
            (Self::Ascii(s), JsStr::Utf8(n)) => s[..end].ends_with(n),
            (Self::Ascii(_), JsStr::Utf16(_)) => false,
            (Self::Wide(u), n) => u[..end].ends_with(&n.units()),
        }
    }
}

/// `f` applied to each well-formed run of a String with an unpaired surrogate, the unpaired
/// surrogates kept: the code point operations (case mapping, normalization) of ECMA-262,
/// which leave a lone surrogate, a code point of its own, as it is.
fn map_runs(u: &[u16], f: impl Fn(&str) -> String) -> Value {
    let mut out = crate::value::JsBuf::new();
    let mut run = String::new();
    for c in char::decode_utf16(u.iter().copied()) {
        match c {
            Ok(c) => run.push(c),
            Err(e) => {
                out.push_str(&f(&run));
                run.clear();
                out.push(JsStr::Utf16(&[e.unpaired_surrogate()]));
            }
        }
    }
    out.push_str(&f(&run));
    out.finish()
}

/// QString::toLower() of text with no unpaired surrogate.
pub(crate) fn qstring_lower(s: &str) -> String {
    s.chars().flat_map(char::to_lowercase).collect()
}

/// QString::toUpper() of text with no unpaired surrogate.
pub(crate) fn qstring_upper(s: &str) -> String {
    s.chars().flat_map(char::to_uppercase).collect()
}

/// QString::toUpper() and toLower(), which are QV4's `toUpperCase` and `toLowerCase`: each
/// code point's full mapping of Unicode with no rule that reads what stands around it (a
/// final sigma is `σ`). QString copies the text up to the first code point the mapping
/// changes and reads the rest a code point at a time, where an unpaired surrogate is U+FFFD;
/// high surrogates at the end are not read at all.
pub(crate) fn qstring_case(js: JsStr<'_>, upper: bool) -> Value {
    let map = |c: char, out: &mut String| {
        if upper {
            out.extend(c.to_uppercase());
        } else {
            out.extend(c.to_lowercase());
        }
    };
    let u = match js {
        JsStr::Utf8(s) if upper => return Value::str(&qstring_upper(s)),
        JsStr::Utf8(s) => return Value::str(&qstring_lower(s)),
        JsStr::Utf16(u) => u,
    };
    let high = |c: &&u16| (0xD800..0xDC00).contains(*c);
    let end = u.len() - u.iter().rev().take_while(high).count();
    let mut out = crate::value::JsBuf::new();
    let (mut run, mut changed) = (String::new(), false);
    for c in char::decode_utf16(u[..end].iter().copied()) {
        match c {
            Ok(c) => {
                let at = run.len();
                map(c, &mut run);
                changed |= run[at..] != *c.encode_utf8(&mut [0; 4]);
            }
            Err(_) if changed => run.push('\u{FFFD}'),
            Err(e) => {
                out.push_str(&run);
                run.clear();
                out.push(JsStr::Utf16(&[e.unpaired_surrogate()]));
            }
        }
    }
    out.push_str(&run);
    out.push(JsStr::Utf16(&u[end..]));
    out.finish()
}

/// The locale `toLocaleUpperCase` and `toLocaleLowerCase` map case in (QV4's
/// getLocaleFromArgs): the system's with no argument; else the one the first argument names,
/// is (a locale of `Qt.locale`), or has as its first element. Any other object is the C
/// locale, and an argument that is no string and no object is a TypeError.
fn case_locale(it: &mut Interp, a: &[Value]) -> R<Locale> {
    let Some(first) = a.first() else {
        return Ok(Locale::SYSTEM);
    };
    let of = |v: &Value| match v {
        Value::Str(_) | Value::Str16(_) => Some(Locale::named(&v.to_js_string())),
        _ => locale_of(v),
    };
    if let Some(locale) = of(first) {
        return Ok(locale);
    }
    if !matches!(first, Value::Obj(_) | Value::Native(_)) {
        return it.throw("TypeError", "Type error");
    }
    let length = it.get(first, "length")?;
    if to_length(it, &length)? == 0 {
        return Ok(Locale::c());
    }
    Ok(of(&it.get(first, "0")?).unwrap_or_else(Locale::c))
}

/// QLocale::toUpper() and toLower() of `locale`: ICU's case mapping (u_strToUpper,
/// u_strToLower) under the rules of the locale's language, which Turkish and Azerbaijani
/// (dotted and dotless i), Lithuanian (the dot kept under an accent), Greek (no accents on
/// capitals) and Armenian (`և`) have of their own. An unpaired surrogate has no case and
/// stays; no rule reads across it.
fn locale_case(locale: Locale, js: JsStr<'_>, upper: bool) -> Value {
    use icu_locale_core::{LanguageIdentifier, langid};
    const MAPPER: icu_casemap::CaseMapperBorrowed<'static> = icu_casemap::CaseMapper::new();
    let language = match locale.name().split('_').next() {
        Some("tr") => langid!("tr"),
        Some("az") => langid!("az"),
        Some("lt") => langid!("lt"),
        Some("el") => langid!("el"),
        Some("hy") => langid!("hy"),
        _ => LanguageIdentifier::UNKNOWN,
    };
    let map = |s: &str| {
        if upper {
            MAPPER.uppercase_to_string(s, &language).into_owned()
        } else {
            MAPPER.lowercase_to_string(s, &language).into_owned()
        }
    };
    match js {
        JsStr::Utf8(s) => Value::str(&map(s)),
        JsStr::Utf16(u) => map_runs(u, map),
    }
}

/// TrimString (ECMA-262 §22.1.3.32.1) of code units `u`: white space is BMP, never a
/// surrogate.
fn trim16(u: &[u16], start: bool, end: bool) -> Value {
    let space = |c: &u16| char::from_u32(u32::from(*c)).is_some_and(crate::value::is_js_space);
    let b = if start {
        u.iter().take_while(|c| space(c)).count()
    } else {
        0
    };
    let e = if end {
        u.len() - u[b..].iter().rev().take_while(|c| space(c)).count()
    } else {
        u.len()
    };
    from16(&u[b..e])
}

/// The code units of String value `v` (one from ToString).
fn needle(v: &Value) -> JsStr<'_> {
    v.js_str().unwrap_or(JsStr::Utf8(""))
}

/// `s.split(sep)` on code units (ECMA-262 §22.1.3.23 steps 12-16): each unit for an empty
/// separator.
fn split16(s: &[u16], sep: &[u16]) -> Vec<Value> {
    if sep.is_empty() {
        return s.chunks(1).map(from16).collect();
    }
    let mut out = Vec::new();
    let (mut p, mut q) = (0, 0);
    while q + sep.len() <= s.len() {
        if s[q..q + sep.len()] == *sep {
            out.push(from16(&s[p..q]));
            q += sep.len();
            p = q;
        } else {
            q += 1;
        }
    }
    out.push(from16(&s[p..]));
    out
}

/// A String.prototype position argument: ToIntegerOrInfinity clamped to `0..=len`.
fn str_pos(it: &mut Interp, v: &Value, len: usize, default: usize) -> R<usize> {
    if matches!(v, Value::Undefined) {
        return Ok(default);
    }
    let n = it.to_numeric(v)?;
    Ok(if n.is_nan() {
        0
    } else {
        n.clamp(0.0, len as f64) as usize
    })
}

fn from16(u: &[u16]) -> Value {
    Value::from_utf16(u)
}

/// The UTF-16 code units of String value `v`.
fn units(v: &Value) -> Rc<[u16]> {
    match v {
        Value::Str(s) => crate::utf16::units(s),
        Value::Str16(u) => u.clone(),
        _ => Rc::new([]),
    }
}

/// Relative index (slice semantics).
fn rel(it: &mut Interp, v: &Value, len: usize, default: usize) -> R<usize> {
    if matches!(v, Value::Undefined) {
        return Ok(default);
    }
    let n = it.to_numeric(v)?;
    let n = if n.is_nan() { 0.0 } else { n.trunc() };
    Ok(if n < 0.0 {
        (len as f64 + n).max(0.0) as usize
    } else {
        n.min(len as f64) as usize
    })
}

// ---------- colors ----------

pub fn color_name(c: &Color) -> String {
    // An invalid colour is black, with the alpha `alpha()` gave it.
    let [r, g, b, a] = if c.spec == Spec::Invalid {
        [0, 0, 0, (c.a >> 8) as u8]
    } else {
        c.rgba8()
    };
    if a == 255 {
        format!("#{r:02x}{g:02x}{b:02x}")
    } else {
        format!("#{a:02x}{r:02x}{g:02x}{b:02x}")
    }
}

const NAMED: &[(&str, u32)] = &[
    ("transparent", 0x00000000),
    ("black", 0xff000000),
    ("white", 0xffffffff),
    ("red", 0xffff0000),
    ("green", 0xff008000),
    ("lime", 0xff00ff00),
    ("blue", 0xff0000ff),
    ("yellow", 0xffffff00),
    ("cyan", 0xff00ffff),
    ("magenta", 0xffff00ff),
    ("gray", 0xff808080),
    ("grey", 0xff808080),
    ("orange", 0xffffa500),
    ("purple", 0xff800080),
    ("pink", 0xffffc0cb),
    ("brown", 0xffa52a2a),
    ("darkgray", 0xffa9a9a9),
    ("lightgray", 0xffd3d3d3),
    ("silver", 0xffc0c0c0),
    ("navy", 0xff000080),
    ("teal", 0xff008080),
    ("maroon", 0xff800000),
    ("olive", 0xff808000),
    ("aliceblue", 0xfff0f8ff),
    ("antiquewhite", 0xfffaebd7),
    ("aqua", 0xff00ffff),
    ("aquamarine", 0xff7fffd4),
    ("azure", 0xfff0ffff),
    ("beige", 0xfff5f5dc),
    ("bisque", 0xffffe4c4),
    ("blanchedalmond", 0xffffebcd),
    ("blueviolet", 0xff8a2be2),
    ("burlywood", 0xffdeb887),
    ("cadetblue", 0xff5f9ea0),
    ("chartreuse", 0xff7fff00),
    ("chocolate", 0xffd2691e),
    ("coral", 0xffff7f50),
    ("cornflowerblue", 0xff6495ed),
    ("cornsilk", 0xfffff8dc),
    ("crimson", 0xffdc143c),
    ("darkblue", 0xff00008b),
    ("darkcyan", 0xff008b8b),
    ("darkgoldenrod", 0xffb8860b),
    ("darkgreen", 0xff006400),
    ("darkgrey", 0xffa9a9a9),
    ("darkkhaki", 0xffbdb76b),
    ("darkmagenta", 0xff8b008b),
    ("darkolivegreen", 0xff556b2f),
    ("darkorange", 0xffff8c00),
    ("darkorchid", 0xff9932cc),
    ("darkred", 0xff8b0000),
    ("darksalmon", 0xffe9967a),
    ("darkseagreen", 0xff8fbc8f),
    ("darkslateblue", 0xff483d8b),
    ("darkslategray", 0xff2f4f4f),
    ("darkslategrey", 0xff2f4f4f),
    ("darkturquoise", 0xff00ced1),
    ("darkviolet", 0xff9400d3),
    ("deeppink", 0xffff1493),
    ("deepskyblue", 0xff00bfff),
    ("dimgray", 0xff696969),
    ("dimgrey", 0xff696969),
    ("dodgerblue", 0xff1e90ff),
    ("firebrick", 0xffb22222),
    ("floralwhite", 0xfffffaf0),
    ("forestgreen", 0xff228b22),
    ("fuchsia", 0xffff00ff),
    ("gainsboro", 0xffdcdcdc),
    ("ghostwhite", 0xfff8f8ff),
    ("gold", 0xffffd700),
    ("goldenrod", 0xffdaa520),
    ("greenyellow", 0xffadff2f),
    ("honeydew", 0xfff0fff0),
    ("hotpink", 0xffff69b4),
    ("indianred", 0xffcd5c5c),
    ("indigo", 0xff4b0082),
    ("ivory", 0xfffffff0),
    ("khaki", 0xfff0e68c),
    ("lavender", 0xffe6e6fa),
    ("lavenderblush", 0xfffff0f5),
    ("lawngreen", 0xff7cfc00),
    ("lemonchiffon", 0xfffffacd),
    ("lightblue", 0xffadd8e6),
    ("lightcoral", 0xfff08080),
    ("lightcyan", 0xffe0ffff),
    ("lightgoldenrodyellow", 0xfffafad2),
    ("lightgreen", 0xff90ee90),
    ("lightgrey", 0xffd3d3d3),
    ("lightpink", 0xffffb6c1),
    ("lightsalmon", 0xffffa07a),
    ("lightseagreen", 0xff20b2aa),
    ("lightskyblue", 0xff87cefa),
    ("lightslategray", 0xff778899),
    ("lightslategrey", 0xff778899),
    ("lightsteelblue", 0xffb0c4de),
    ("lightyellow", 0xffffffe0),
    ("limegreen", 0xff32cd32),
    ("linen", 0xfffaf0e6),
    ("mediumaquamarine", 0xff66cdaa),
    ("mediumblue", 0xff0000cd),
    ("mediumorchid", 0xffba55d3),
    ("mediumpurple", 0xff9370db),
    ("mediumseagreen", 0xff3cb371),
    ("mediumslateblue", 0xff7b68ee),
    ("mediumspringgreen", 0xff00fa9a),
    ("mediumturquoise", 0xff48d1cc),
    ("mediumvioletred", 0xffc71585),
    ("midnightblue", 0xff191970),
    ("mintcream", 0xfff5fffa),
    ("mistyrose", 0xffffe4e1),
    ("moccasin", 0xffffe4b5),
    ("navajowhite", 0xffffdead),
    ("oldlace", 0xfffdf5e6),
    ("olivedrab", 0xff6b8e23),
    ("orangered", 0xffff4500),
    ("orchid", 0xffda70d6),
    ("palegoldenrod", 0xffeee8aa),
    ("palegreen", 0xff98fb98),
    ("paleturquoise", 0xffafeeee),
    ("palevioletred", 0xffdb7093),
    ("papayawhip", 0xffffefd5),
    ("peachpuff", 0xffffdab9),
    ("peru", 0xffcd853f),
    ("plum", 0xffdda0dd),
    ("powderblue", 0xffb0e0e6),
    ("rosybrown", 0xffbc8f8f),
    ("royalblue", 0xff4169e1),
    ("saddlebrown", 0xff8b4513),
    ("salmon", 0xfffa8072),
    ("sandybrown", 0xfff4a460),
    ("seagreen", 0xff2e8b57),
    ("seashell", 0xfffff5ee),
    ("sienna", 0xffa0522d),
    ("skyblue", 0xff87ceeb),
    ("slateblue", 0xff6a5acd),
    ("slategray", 0xff708090),
    ("slategrey", 0xff708090),
    ("snow", 0xfffffafa),
    ("springgreen", 0xff00ff7f),
    ("steelblue", 0xff4682b4),
    ("tan", 0xffd2b48c),
    ("thistle", 0xffd8bfd8),
    ("tomato", 0xffff6347),
    ("turquoise", 0xff40e0d0),
    ("violet", 0xffee82ee),
    ("wheat", 0xfff5deb3),
    ("whitesmoke", 0xfff5f5f5),
    ("yellowgreen", 0xff9acd32),
];

/// QColor::fromString: #rgb #rrggbb #aarrggbb #rrrgggbbb #rrrrggggbbbb, the SVG color names
/// and "transparent".
/// QColor::fromString: `#` and 3, 6, 8, 9 or 12 hex digits, or an SVG colour name written in
/// any case, with spaces and tabs anywhere in it (`light blue`). Nothing else is skipped: a
/// space before `#` or a line break after a name makes the colour invalid.
pub fn parse_color(s: &str) -> Color {
    if let Some(h) = s.strip_prefix('#') {
        // Digits only: `from_str_radix` also takes a leading sign, and slicing a non-ASCII
        // character in half panics.
        if !h.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Color::INVALID;
        }
        let hx = |a: usize, b: usize| u32::from_str_radix(&h[a..b], 16).unwrap();
        return match h.len() {
            3 => Color::from_argb32(
                0xff000000 | (hx(0, 1) * 0x11) << 16 | (hx(1, 2) * 0x11) << 8 | (hx(2, 3) * 0x11),
            ),
            6 => Color::from_argb32(0xff000000 | hx(0, 6)),
            8 => Color::from_argb32(hx(0, 8)),
            9 => Color {
                spec: Spec::Rgb,
                a: 65535,
                c: [hx(0, 3), hx(3, 6), hx(6, 9)].map(|v| (v as u64 * 65535 / 4095) as u16),
            },
            12 => Color {
                spec: Spec::Rgb,
                a: 65535,
                c: [hx(0, 4), hx(4, 8), hx(8, 12)].map(|v| v as u16),
            },
            _ => Color::INVALID,
        };
    }
    let l: String = s
        .chars()
        .filter(|c| !matches!(c, ' ' | '\t'))
        .map(|c| c.to_ascii_lowercase())
        .collect();
    NAMED
        .iter()
        .find(|(n, _)| *n == l)
        .map(|(_, v)| Color::from_argb32(*v))
        .unwrap_or(Color::INVALID)
}

pub fn to_color(v: &Value) -> Color {
    match v {
        Value::Color(c) => *c,
        Value::Str(s) => parse_color(s),
        Value::Obj(_) => crate::valuetypes::color_of(v).unwrap_or(Color::INVALID),
        _ => Color::INVALID,
    }
}

/// A method of a colour value read as a value (`var f = c.lighter`): V4 gives a function
/// without a name that keeps the colour it was read from as its `this`.
fn color_method(c: &Color, f: NativeFn) -> Value {
    let v = fn_value(Callable::Bound(Rc::new(BoundFn {
        target: fn_value(Callable::Native("", f)),
        this: Value::Color(*c),
        args: Vec::new(),
        length: 0.0,
    })));
    if let Value::Obj(o) = &v {
        set_hidden(o, "name", Value::str(""));
    }
    v
}

pub fn color_prop(c: &Color, k: &str) -> Value {
    let f = |x: f32| Value::Num(x as f64);
    fn call(it: &mut Interp, this: &Value, name: &str, a: &[Value]) -> R<Value> {
        Ok(method_impl(it, this, name, a)?.unwrap_or_default())
    }
    match k {
        "toString" => color_method(c, |it, this, a| call(it, this, "toString", a)),
        "lighter" => color_method(c, |it, this, a| call(it, this, "lighter", a)),
        "darker" => color_method(c, |it, this, a| call(it, this, "darker", a)),
        "alpha" => color_method(c, |it, this, a| call(it, this, "alpha", a)),
        "tint" => color_method(c, |it, this, a| call(it, this, "tint", a)),
        "r" => f(c.red_f()),
        "g" => f(c.green_f()),
        "b" => f(c.blue_f()),
        "a" => f(c.alpha_f()),
        "hsvHue" => f(c.hsv_hue_f()),
        "hsvSaturation" => f(c.hsv_saturation_f()),
        "hsvValue" => f(c.value_f()),
        "hslHue" => f(c.hsl_hue_f()),
        "hslSaturation" => f(c.hsl_saturation_f()),
        "hslLightness" => f(c.lightness_f()),
        "valid" => Value::Bool(c.spec != Spec::Invalid),
        // QColor::rgba(), a QRgb (`#AARRGGBB`) read as the `int` the property is declared as.
        "rgba" => {
            let [r, g, b, a] = c.rgba8();
            Value::Num(i32::from_be_bytes([a, r, g, b]) as f64)
        }
        _ => Value::Undefined,
    }
}

// ---------- number formatting ----------

pub fn to_fixed(x: f64, d: usize) -> String {
    if !x.is_finite() || x.abs() >= 1e21 {
        return num_to_string(x);
    }
    let x = if x == 0.0 { 0.0 } else { x };
    let a = x.abs();
    let p = 10f64.powi(d as i32);
    let scaled = a * p;
    // ECMA-262 takes the larger n on an exact tie, `format!` the even one. A tie needs
    // `a * p` exact (no rounding in the product, checked with an fma) and `p` exact (d <= 22).
    if d <= 22 && scaled.fract() == 0.5 && a.mul_add(p, -scaled) == 0.0 {
        let r = (scaled.floor() + 1.0) / p;
        return format!("{}{:.*}", if x < 0.0 { "-" } else { "" }, d, r);
    }
    format!("{x:.d$}")
}

/// QString::number(double, 'g', 6).
pub fn fmt_g6(x: f64) -> String {
    Locale::c().numeric().to_string(x, b'g', 6)
}

/// One argument of `String.prototype.arg` as QString::arg takes it (QtObject's
/// method_string_arg): a boolean as the int 1 or 0, a number that is an int as that int, any
/// other number as a double (`%g` with 6 digits), anything else as its string. QV4 goes by
/// how the number is stored; here every number is a double, so one with an int's value is an
/// int: a deliberate difference, see docs/qt-differences.md.
fn qarg_text(v: &Value, localized: bool) -> String {
    let int = match v {
        Value::Bool(b) => Some(i32::from(*b)),
        Value::Num(n) if n.fract() == 0.0 && n.abs() <= i32::MAX as f64 + 1.0 => {
            Some(*n as i32).filter(|i| f64::from(*i) == *n)
        }
        _ => None,
    };
    match (int, v) {
        (Some(i), _) if localized => Numeric::system().to_string(f64::from(i), b'f', 0),
        (Some(i), _) => i.to_string(),
        (None, Value::Num(n)) if localized => Numeric::system().to_string(*n, b'g', 6),
        (None, Value::Num(n)) => fmt_g6(*n),
        (None, other) => other.to_js_string(),
    }
}

/// The place marker at `b[i]` (a `%`): `%N`, `%NN` or either with an `L` before the digits,
/// as (its number, whether it has the `L`, its length).
fn qarg_marker(b: &[u8], i: usize) -> Option<(u32, bool, usize)> {
    let localized = b.get(i + 1) == Some(&b'L');
    let mut j = i + 1 + usize::from(localized);
    let mut n = u32::from(*b.get(j).filter(|c| c.is_ascii_digit())? - b'0');
    j += 1;
    if let Some(c) = b.get(j).filter(|c| c.is_ascii_digit()) {
        n = n * 10 + u32::from(c - b'0');
        j += 1;
    }
    Some((n, localized, j - i))
}

/// `s` with every place marker `text` gives a replacement for replaced by it.
fn qarg_replace(s: &str, mut text: impl FnMut(u32, bool) -> Option<String>) -> String {
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let (mut i, mut copied) = (0, 0);
    while i < b.len() {
        let marker = (b[i] == b'%').then(|| qarg_marker(b, i)).flatten();
        let Some((n, localized, len)) = marker else {
            i += 1;
            continue;
        };
        if let Some(t) = text(n, localized) {
            out.push_str(&s[copied..i]);
            out.push_str(&t);
            copied = i + len;
        }
        i += len;
    }
    out.push_str(&s[copied..]);
    out
}

/// QString::arg. One argument replaces every occurrence of the lowest-numbered place marker,
/// in the system locale's number format where the marker has an `L`. Several arguments are
/// QString's multi-argument `arg`: they replace the lowest-numbered markers in order, in one
/// pass (a marker inside a replacement stays), each as its plain text.
pub fn qarg(s: &str, args: &[Value]) -> String {
    let b = s.as_bytes();
    let mut numbers: Vec<u32> = (0..b.len())
        .filter(|&i| b[i] == b'%')
        .filter_map(|i| Some(qarg_marker(b, i)?.0))
        .collect();
    numbers.sort_unstable();
    numbers.dedup();
    numbers.truncate(args.len());
    qarg_replace(s, |n, localized| {
        let arg = &args[numbers.iter().position(|m| *m == n)?];
        Some(qarg_text(arg, localized && args.len() == 1))
    })
}

// ---------- regex ----------

/// Compiled regexes most recently used: patterns are partly built at runtime (TaskbarApps,
/// Cheatsheet), so the cache is bounded.
const RE_CACHE_MAX: usize = 512;

thread_local! {
    static RE_CACHE: RefCell<crate::bytelru::ByteLru<(String, String), Rc<JsRegex>>> =
        RefCell::new(crate::bytelru::ByteLru::new(usize::MAX).max_len(RE_CACHE_MAX));
}

pub fn re_cache_len() -> usize {
    RE_CACHE.with(|c| c.borrow().len())
}

/// %RegExp.prototype% (ECMA-262 §22.2.6): the methods, Annex B `compile`, and the flag
/// accessors reading the [[OriginalSource]] / [[OriginalFlags]] of `this`.
fn install_regexp_prototype(p: &Obj) {
    install_regexp_methods(p);
    nat(p, "compile", 2, regexp_compile);
    let getter = |name: &'static str, f: NativeFn| {
        crate::eval::define_accessor(p, &name[4..], true, native(name, 0, f), false);
    };
    getter("get source", |it, this, _| match regexp_this(this) {
        Some(o) => {
            let Internal::RegExp { source, .. } = &o.borrow().internal else {
                unreachable!()
            };
            Ok(Value::str(&escape_pattern(source)))
        }
        None if is_regexp_proto(it, this) => Ok(Value::str("(?:)")),
        None => it.throw("TypeError", "Type error"),
    });
    getter("get flags", |it, this, _| {
        if !matches!(this, Value::Obj(_)) {
            return it.throw("TypeError", "Type error");
        }
        let mut out = String::new();
        for (c, k) in REGEXP_FLAGS {
            if it.get(this, k)?.truthy() {
                out.push(c);
            }
        }
        Ok(Value::str(&out))
    });
    macro_rules! flag {
        ($($c:literal => $name:literal),*) => {
            $( getter($name, |it, this, _| regexp_flag(it, this, $c)); )*
        };
    }
    flag!(
        'g' => "get global",
        'i' => "get ignoreCase",
        'm' => "get multiline",
        's' => "get dotAll",
        'u' => "get unicode",
        'y' => "get sticky"
    );
}

/// The flags `get flags` collects, in its order (ECMA-262 §22.2.6.4).
const REGEXP_FLAGS: [(char, &str); 6] = [
    ('g', "global"),
    ('i', "ignoreCase"),
    ('m', "multiline"),
    ('s', "dotAll"),
    ('u', "unicode"),
    ('y', "sticky"),
];

/// `this` when it is a RegExp object (has [[RegExpMatcher]]).
fn regexp_this(this: &Value) -> Option<&Obj> {
    match this {
        Value::Obj(o) if obj_kind(o).2 => Some(o),
        _ => None,
    }
}

/// `RegExp.prototype.test` as V4 does it: the built-in exec of a RegExp object whatever its
/// `exec` property is, a TypeError for another receiver.
fn regexp_test(it: &mut Interp, this: &Value, a: &[Value]) -> R<Value> {
    let s = it.to_str_named(&arg(a, 0))?;
    let s16 = units(&s);
    match compile(it, this)? {
        Some((re, _)) => Ok(Value::Bool(regex_exec(it, this, &re, &s16)?.is_some())),
        None => it.type_error(),
    }
}

fn is_regexp_proto(it: &Interp, this: &Value) -> bool {
    matches!(this, Value::Obj(o) if Rc::ptr_eq(o, &it.regexp_proto))
}

/// A flag accessor of %RegExp.prototype% (`get global`, ...): undefined on the prototype
/// itself (ECMA-262 RegExpHasFlag).
fn regexp_flag(it: &mut Interp, this: &Value, c: char) -> R<Value> {
    match regexp_this(this) {
        Some(o) => {
            let Internal::RegExp { flags, .. } = &o.borrow().internal else {
                unreachable!()
            };
            Ok(Value::Bool(flags.contains(c)))
        }
        None if is_regexp_proto(it, this) => Ok(Value::Undefined),
        None => it.throw("TypeError", "Type error"),
    }
}

pub fn regex_value(p: &str, f: &str) -> Value {
    let o = new_obj();
    o.borrow_mut().internal = Internal::RegExp {
        source: p.into(),
        flags: f.into(),
    };
    // Writable, non-enumerable and non-configurable (ECMA-262 §22.2.8.1).
    set_hidden(&o, "lastIndex", Value::Num(0.0));
    o.borrow_mut().set_attr("lastIndex", PERMANENT);
    Value::Obj(o)
}

/// A RegExp's [[OriginalSource]] and [[OriginalFlags]].
pub fn regexp_parts(o: &Obj) -> Option<(Rc<str>, Rc<str>)> {
    match &o.borrow().internal {
        Internal::RegExp { source, flags } => Some((source.clone(), flags.clone())),
        _ => None,
    }
}

/// What `source` reads, as V4 writes it: `\\` before every `/` (in a class and after a
/// backslash too), line terminators as they are, `(?:)` when empty.
fn escape_pattern(p: &str) -> String {
    match p {
        "" => "(?:)".into(),
        p => p.replace('/', "\\/"),
    }
}

/// The flags a RegExp accepts, each at most once.
fn valid_flags(f: &str) -> bool {
    f.chars().all(|c| "gimsuy".contains(c)) && f.char_indices().all(|(i, c)| !f[..i].contains(c))
}

/// RegExpCreate / RegExpInitialize (ECMA-262 §22.2.3.1-2) for `new RegExp(p, f)` and
/// `re.compile(p, f)`: a RegExp `p` gives its source and, unless `f` is given, its flags
/// (`compile` rejects `f` then, Annex B §B.2.4.1); else ToString of each (`undefined` is
/// empty). Bad flags or pattern throw SyntaxError.
fn regexp_new(it: &mut Interp, p: &Value, f: &Value, compiling: bool) -> R<Value> {
    let parts = match p {
        Value::Obj(o) => regexp_parts(o),
        _ => None,
    };
    let (src, flags) = match parts {
        Some((s, fl)) => match f {
            Value::Undefined => (s.to_string(), fl.to_string()),
            _ if compiling => {
                return it.throw("TypeError", "Type error");
            }
            f => (s.to_string(), it.to_string(f)?),
        },
        None => (
            match p {
                Value::Undefined => String::new(),
                p => it.to_string(p)?,
            },
            match f {
                Value::Undefined => String::new(),
                f => it.to_string(f)?,
            },
        ),
    };
    if !valid_flags(&flags) {
        return it.throw(
            "SyntaxError",
            "Invalid flags supplied to RegExp constructor",
        );
    }
    let v = regex_value(&src, &flags);
    compile(it, &v)?;
    Ok(v)
}

/// `RegExp(pattern, flags)`.
fn regexp_ctor(it: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
    let called = !std::mem::take(&mut it.constructing);
    regexp_construct(it, &arg(a, 0), &arg(a, 1), called)
}

/// The RegExp constructor (ECMA-262 §22.2.4.1) on `pattern` and `flags`, `called` without
/// `new`: a regexp-like pattern (IsRegExp) gives its `source` and `flags`, and `RegExp(re)`
/// returns `re` itself when `re.constructor` is %RegExp%.
fn regexp_construct(it: &mut Interp, p: &Value, f: &Value, called: bool) -> R<Value> {
    let pattern_is_regexp = is_regexp(it, p)?;
    if called && pattern_is_regexp && matches!(f, Value::Undefined) {
        let c = it.get(p, "constructor")?;
        if is_intrinsic_regexp(&c) {
            return Ok(p.clone());
        }
    }
    if pattern_is_regexp && regexp_this(p).is_none() {
        let src = it.get(p, "source")?;
        let flags = match f {
            Value::Undefined => it.get(p, "flags")?,
            f => f.clone(),
        };
        return regexp_new(it, &src, &flags, false);
    }
    regexp_new(it, p, f, false)
}

/// `RegExp.prototype.toString` (ECMA-262 §22.2.6.17): `/source/flags` of any object.
fn regexp_to_string(it: &mut Interp, this: &Value) -> R<Value> {
    let source = it.get(this, "source")?;
    let flags = it.get(this, "flags")?;
    let s = format!("/{}/{}", it.to_string(&source)?, it.to_string(&flags)?);
    Ok(Value::str(&s))
}

/// `RegExp.prototype.compile(pattern, flags)` (Annex B §B.2.4.1): reinitializes `this` in
/// place. V4 returns `undefined` and leaves `lastIndex` alone.
fn regexp_compile(it: &mut Interp, this: &Value, a: &[Value]) -> R<Value> {
    let Some(o) = regexp_this(this) else {
        return it.throw("TypeError", "Type error");
    };
    // V4 takes the source of a RegExp pattern and lets the flags argument replace its flags.
    let pattern = match (regexp_this(&arg(a, 0)), arg(a, 1)) {
        (Some(_), Value::Undefined) | (None, _) => arg(a, 0),
        (Some(p), _) => match &p.borrow().internal {
            Internal::RegExp { source, .. } => Value::str(source),
            _ => unreachable!("regexp_this"),
        },
    };
    if let Value::Obj(n) = regexp_new(it, &pattern, &arg(a, 1), true)? {
        let slots = std::mem::take(&mut n.borrow_mut().internal);
        o.borrow_mut().internal = slots;
    }
    Ok(Value::Undefined)
}

/// The last successful match: the input it ran on and the hit.
pub(crate) type LegacyMatch = (Rc<[u16]>, Hit);

/// UpdateLegacyRegExpStaticProperties (TC39 proposal-regexp-legacy-features): every
/// successful RegExpBuiltinExec records its match for the statics.
fn note_match(it: &mut Interp, s: &Rc<[u16]>, h: &Hit) {
    it.regexp_last = Some((s.clone(), h.clone()));
}

/// A legacy static of `RegExp`: 0 `lastMatch`, 1-9 `$1`..`$9`, 10 `lastParen`, 11
/// `leftContext`, 12 `rightContext`, 13 `input`; all empty before any match.
fn legacy_static<const K: u8>(it: &mut Interp, _: &Value, _: &[Value]) -> R<Value> {
    let Some((s, (b, e, g, _))) = &it.regexp_last else {
        return Ok(Value::str(""));
    };
    let group = |i: usize| g.get(i).and_then(Clone::clone).map_or(&[][..], |r| &s[r]);
    Ok(from16(match K {
        0 => &s[*b..*e],
        1..=9 => group(K as usize - 1),
        10 => g.len().checked_sub(1).map_or(&[][..], group),
        11 => &s[..*b],
        12 => &s[*e..],
        _ => s,
    }))
}

/// The legacy static accessors of `RegExp` (QV4 has getters only), each with its alias.
const LEGACY_STATICS: [(&str, &str, NativeFn); 19] = [
    ("lastMatch", "get lastMatch", legacy_static::<0>),
    ("$&", "get $&", legacy_static::<0>),
    ("$1", "get $1", legacy_static::<1>),
    ("$2", "get $2", legacy_static::<2>),
    ("$3", "get $3", legacy_static::<3>),
    ("$4", "get $4", legacy_static::<4>),
    ("$5", "get $5", legacy_static::<5>),
    ("$6", "get $6", legacy_static::<6>),
    ("$7", "get $7", legacy_static::<7>),
    ("$8", "get $8", legacy_static::<8>),
    ("$9", "get $9", legacy_static::<9>),
    ("lastParen", "get lastParen", legacy_static::<10>),
    ("$+", "get $+", legacy_static::<10>),
    ("leftContext", "get leftContext", legacy_static::<11>),
    ("$`", "get $`", legacy_static::<11>),
    ("rightContext", "get rightContext", legacy_static::<12>),
    ("$'", "get $'", legacy_static::<12>),
    ("input", "get input", legacy_static::<13>),
    ("$_", "get $_", legacy_static::<13>),
];

/// A compiled RegExp (ECMA-262 pattern syntax and semantics). Subjects are matched as UTF-16
/// code units, so every index is a JS string index; without `u` the pattern and the subject
/// are sequences of code units (`/^.$/` does not match "😀", `/\ud83d/` does), with `u` of
/// code points.
struct JsRegex {
    re: regress::Regex,
    unicode: bool,
}

/// A match: start, end and the capture groups, as UTF-16 ranges of the subject, and the
/// named groups (each name with its range) when the pattern has any.
type Hit = (
    usize,
    usize,
    Vec<Option<std::ops::Range<usize>>>,
    Option<NamedGroups>,
);
type NamedGroups = Rc<[(Rc<str>, Option<std::ops::Range<usize>>)]>;

fn hit(m: regress::Match) -> Hit {
    let named: Vec<_> = m.named_groups().map(|(n, r)| (Rc::from(n), r)).collect();
    let named = (!named.is_empty()).then(|| named.into());
    (m.start(), m.end(), m.captures, named)
}

impl JsRegex {
    fn new(src: &str, flags: &str) -> Result<Self, regress::Error> {
        let f = regress::Flags {
            icase: flags.contains('i'),
            multiline: flags.contains('m'),
            dot_all: flags.contains('s'),
            unicode: flags.contains('u'),
            ..Default::default()
        };
        let re = if f.unicode {
            regress::Regex::from_unicode(src.chars().map(u32::from), f)
        } else {
            regress::Regex::from_unicode(src.encode_utf16().map(u32::from), f)
        }?;
        Ok(Self {
            re,
            unicode: f.unicode,
        })
    }

    /// The first match starting at or after `start`.
    fn find_from(&self, s: &[u16], start: usize) -> Option<Hit> {
        if self.unicode {
            self.re.find_from_utf16(s, start).next().map(hit)
        } else {
            self.re.find_from_ucs2(s, start).next().map(hit)
        }
    }

    /// `AdvanceStringIndex`.
    fn advance(&self, s: &[u16], i: usize) -> usize {
        advance_index(s, i, self.unicode)
    }
}

fn compile(it: &Interp, v: &Value) -> R<Option<(Rc<JsRegex>, bool)>> {
    let Value::Obj(o) = v else { return Ok(None) };
    let (src, flags) = {
        let b = o.borrow();
        let Internal::RegExp { source, flags } = &b.internal else {
            return Ok(None);
        };
        (source.to_string(), flags.to_string())
    };
    let key = (src.clone(), flags.clone());
    if let Some(r) = RE_CACHE.with(|c| c.borrow_mut().get(&key).cloned()) {
        return Ok(Some((r, flags.contains('g'))));
    }
    match JsRegex::new(&src, &flags) {
        Ok(r) => {
            let r = Rc::new(r);
            RE_CACHE.with(|c| c.borrow_mut().insert(key, r.clone(), 0));
            Ok(Some((r, flags.contains('g'))))
        }
        Err(_) => it.throw("SyntaxError", "Invalid regular expression"),
    }
}

fn group(s: &[u16], g: &Option<std::ops::Range<usize>>) -> Value {
    g.as_ref()
        .map(|r| from16(&s[r.clone()]))
        .unwrap_or_default()
}

// ---------- methods on primitives / arrays ----------

/// A built-in prototype's method list, which [`has_method`] admits for receivers of its kind,
/// and the installer putting each method on the prototype object as a native that
/// [`proto_method`] forwards to the dispatcher ([`method_impl`]), so `[].slice` read as a
/// property is the same method `[].slice()` calls.
macro_rules! proto_methods {
    ($list:ident, $install:ident, $kind:expr, [$($n:literal = $len:literal),* $(,)?]) => {
        const $list: &[&str] = &[$($n),*];
        fn $install(p: &Obj) {
            $( nat(p, $n, $len, |it, this, a| proto_method(it, this, $kind, $n, a)); )*
        }
    };
}

proto_methods!(
    STRING_METHODS,
    install_string_prototype,
    Proto::String,
    [
        "trim" = 0,
        "trimStart" = 0,
        "trimEnd" = 0,
        "toLowerCase" = 0,
        "toLocaleLowerCase" = 0,
        "toUpperCase" = 0,
        "toLocaleUpperCase" = 0,
        "normalize" = 0,
        "toString" = 0,
        "valueOf" = 0,
        "startsWith" = 1,
        "endsWith" = 1,
        "includes" = 1,
        "indexOf" = 1,
        "lastIndexOf" = 1,
        "slice" = 2,
        "substring" = 2,
        "substr" = 2,
        "charAt" = 1,
        "at" = 1,
        "charCodeAt" = 1,
        "codePointAt" = 1,
        "padStart" = 1,
        "padEnd" = 1,
        "repeat" = 1,
        "concat" = 1,
        "localeCompare" = 0,
        "arg" = 0,
        "replace" = 2,
        "replaceAll" = 2,
        "match" = 1,
        "matchAll" = 1,
        "search" = 1,
        "split" = 2,
    ]
);
proto_methods!(
    NUMBER_METHODS,
    install_number_prototype,
    Proto::Number,
    [
        "toFixed" = 1,
        "toString" = 1,
        "toPrecision" = 1,
        "toExponential" = 1,
        "toLocaleString" = 0,
        "toLocaleCurrencyString" = 0,
        "valueOf" = 0,
    ]
);
proto_methods!(
    BOOLEAN_METHODS,
    install_boolean_prototype,
    Proto::Boolean,
    ["toString" = 0, "valueOf" = 0]
);
proto_methods!(
    ARRAY_METHODS,
    install_array_prototype,
    Proto::Array,
    [
        "push" = 1,
        "pop" = 0,
        "shift" = 0,
        "unshift" = 1,
        "splice" = 2,
        "reverse" = 0,
        "fill" = 1,
        "sort" = 1,
        "slice" = 2,
        "concat" = 1,
        "join" = 1,
        "toString" = 0,
        "includes" = 1,
        "indexOf" = 1,
        "lastIndexOf" = 1,
        "at" = 1,
        "flat" = 0,
        "map" = 1,
        "filter" = 1,
        "forEach" = 1,
        "find" = 1,
        "findIndex" = 1,
        "some" = 1,
        "every" = 1,
        "flatMap" = 1,
        "reduce" = 1,
        "reduceRight" = 1,
        "copyWithin" = 2,
        "toLocaleString" = 0,
    ]
);
proto_methods!(
    REGEXP_METHODS,
    install_regexp_methods,
    Proto::RegExp,
    ["exec" = 1, "test" = 1, "toString" = 0]
);
proto_methods!(
    FUNCTION_METHODS,
    install_function_prototype,
    Proto::Function,
    ["call" = 1, "apply" = 2, "bind" = 1, "toString" = 0]
);
proto_methods!(
    MAP_METHODS,
    install_map_prototype,
    Proto::Map,
    [
        "clear" = 0,
        "delete" = 1,
        "forEach" = 1,
        "get" = 1,
        "has" = 1,
        "set" = 2,
    ]
);
proto_methods!(
    SET_METHODS,
    install_set_prototype,
    Proto::Set,
    [
        "add" = 1,
        "clear" = 0,
        "delete" = 1,
        "forEach" = 1,
        "has" = 1,
    ]
);

/// The mutating [`ARRAY_METHODS`], whose result [`array_generic`] writes back.
const ARRAY_MUTATORS: [&str; 9] = [
    "push",
    "pop",
    "shift",
    "unshift",
    "splice",
    "reverse",
    "fill",
    "sort",
    "copyWithin",
];

/// The built-in prototype a [`proto_methods`] list belongs to.
#[derive(Clone, Copy)]
enum Proto {
    String,
    Number,
    Boolean,
    Array,
    Function,
    RegExp,
    Map,
    Set,
}

/// `C.prototype.name` called on `this` (`String.prototype.trim.call(s)`, `[].slice.call(a)`):
/// the brand checks of ECMA-262 (thisNumberValue, RequireObjectCoercible, IsCallable, ...),
/// then the method [`method_impl`] has for that kind of receiver.
fn proto_method(
    it: &mut Interp,
    this: &Value,
    kind: Proto,
    name: &'static str,
    a: &[Value],
) -> R<Value> {
    // thisNumberValue, thisBooleanValue and thisStringValue take a wrapper's internal slot.
    let data = match (&kind, this) {
        (Proto::Number | Proto::Boolean | Proto::String, Value::Obj(o)) => crate::value::wrapped(o),
        _ => None,
    };
    let this = match (&kind, &data) {
        (Proto::Number, Some(p @ Value::Num(_)))
        | (Proto::Boolean, Some(p @ Value::Bool(_)))
        | (Proto::String, Some(p @ (Value::Str(_) | Value::Str16(_)))) => p,
        _ => this,
    };
    let ok = match kind {
        Proto::String => match this {
            // `arg` is Qt's, and takes `thisObject->toQString()` of any receiver.
            // V4's `replace` does the same.
            Value::Undefined | Value::Null => matches!(name, "arg" | "replace"),
            Value::Str(_) | Value::Str16(_) => true,
            _ => !matches!(name, "toString" | "valueOf"),
        },
        Proto::Number => {
            matches!(this, Value::Num(_))
                || matches!(name, "toLocaleString" | "toLocaleCurrencyString")
        }
        Proto::Boolean => matches!(this, Value::Bool(_)),
        // ToObject(this), with QV4's message for undefined and null.
        Proto::Array => match this {
            Value::Undefined | Value::Null => return it.no_object(this),
            _ => true,
        },
        Proto::Function => crate::eval::callable(this).is_some(),
        // `toString` and `test` are generic over any object (ECMA-262 §22.2.6.17, 16), `exec`
        // needs a RegExp.
        Proto::RegExp => match this {
            Value::Obj(o) => matches!(name, "toString" | "test") || obj_kind(o).2,
            _ => false,
        },
        Proto::Map => {
            matches!(this, Value::Obj(o) if matches!(o.borrow().internal, Internal::Map(_)))
        }
        Proto::Set => {
            matches!(this, Value::Obj(o) if matches!(o.borrow().internal, Internal::Set(_)))
        }
    };
    if !ok && matches!(kind, Proto::String) {
        // QV4's getThisString: what a method of String says of a receiver that is no value.
        return it.throw("TypeError", "Type error");
    }
    if !ok {
        return it.type_error();
    }
    let r = match (kind, this) {
        (Proto::String, Value::Str(_) | Value::Str16(_)) => method_impl(it, this, name, a)?,
        (Proto::String, _) => {
            let s = it.to_str_value(this)?;
            method_impl(it, &s, name, a)?
        }
        (Proto::Array, Value::Obj(o)) if o.borrow().array.is_some() => {
            method_impl(it, this, name, a)?
        }
        (Proto::Array, _) if name == "toString" => return array_to_string(it, this),
        (Proto::Array, _) => return array_generic(it, this, name, a),
        (Proto::RegExp, _) if name == "toString" => return regexp_to_string(it, this),
        (Proto::RegExp, _) if name == "test" => return regexp_test(it, this, a),
        (Proto::Number, _) if name.starts_with("toLocale") => {
            Some(number_locale(it, this, name, a)?)
        }
        _ => method_impl(it, this, name, a)?,
    };
    Ok(r.unwrap_or_default())
}

/// `Array.prototype.toString` (ECMA-262 §23.1.3.36), generic over any object: `this.join()`,
/// else %Object.prototype.toString% when `join` is not callable.
fn array_to_string(it: &mut Interp, this: &Value) -> R<Value> {
    let join = it.get(this, "join")?;
    let f = if crate::eval::callable(&join).is_some() {
        join
    } else {
        it.get(&Value::Obj(it.object_proto.clone()), "toString")?
    };
    it.call(&f, this.clone(), &[])
}

/// An %Array.prototype% method on array-like `this` (the methods are generic, ECMA-262
/// §23.1.3): it runs on an array of `this`'s elements up to its `length`, holes kept, and a
/// mutator's result is written back to `this` (a callback sees that array as its third
/// argument).
///
/// The length is the one Qt 6.12's method reads (qv4arrayobject.cpp): ToLength, of which
/// most methods keep the low 32 bits (`uint len = getLength()`); `push`, `splice`,
/// `reverse`, `fill`, `copyWithin`, `includes` and `map` keep all of it.
fn array_generic(it: &mut Interp, this: &Value, name: &str, a: &[Value]) -> R<Value> {
    let mutates = ARRAY_MUTATORS.contains(&name);
    if mutates && this.is_str() {
        return it.throw("TypeError", "Type error");
    }
    let n = it.get(this, "length")?;
    let n = it.to_numeric(&n)?;
    if name == "concat" {
        return array_concat(it, this, a);
    }
    let len64 = if n > 0.0 {
        n.trunc().min(9007199254740991.0) as usize
    } else {
        0
    };
    let len32 = len64 & u32::MAX as usize;
    // A mutator works on the properties in place; the rest copy the elements first.
    if mutates && let Value::Obj(o) = this {
        let len = match name {
            "push" | "splice" | "reverse" | "fill" | "copyWithin" => len64,
            _ => len32,
        };
        return array_mutate(it, o, name, a, len);
    }
    // These read only the elements their arguments name, so they run on the properties
    // whatever the length.
    if matches!(name, "includes" | "indexOf" | "lastIndexOf" | "slice") {
        let len = if name == "includes" { len64 } else { len32 };
        return array_range(it, this, name, a, len);
    }
    if name == "map" && len64 > u32::MAX as usize - 1 {
        return it.throw("RangeError", "Array length out of range.");
    }
    let len = crate::eval::array_len(it, len32 as f64)?;
    let mut items = Vec::with_capacity(len);
    let mut holes = Vec::new();
    for i in 0..len {
        let k = i.to_string();
        if has_property(it, this, &k)? {
            items.push(it.get(this, &k)?);
        } else {
            items.push(Value::Undefined);
            holes.push(i);
        }
    }
    let Value::Obj(copy) = Value::array_made(items, holes) else {
        unreachable!("Value::array_made makes an array object")
    };
    let species = ARRAY_SPECIES.contains(&name).then_some(this);
    Ok(array_method(it, &copy, name, a, species)?.unwrap_or_default())
}

/// `includes`, `indexOf`, `lastIndexOf` and `slice` of %Array.prototype% on array-like
/// `this` of length `len`, as their ECMA-262 algorithms: the HasProperty and Get steps of
/// the indices from the start they compute to the end, and no others.
fn array_range(it: &mut Interp, this: &Value, name: &str, a: &[Value], len: usize) -> R<Value> {
    let target = arg(a, 0);
    Ok(match name {
        "includes" => {
            let from = rel(it, &arg(a, 1), len, 0)?;
            let nan = matches!(target, Value::Num(n) if n.is_nan());
            for k in from..len {
                // Qt reads the element at the index's low 32 bits (`instance->get(k)`).
                let x = it.get(this, &(k as u32).to_string())?;
                if x.strict_eq(&target) || (nan && matches!(x, Value::Num(n) if n.is_nan())) {
                    return Ok(Value::Bool(true));
                }
            }
            Value::Bool(false)
        }
        "indexOf" => {
            let from = rel(it, &arg(a, 1), len, 0)?;
            for k in from..len {
                let key = k.to_string();
                if has_property(it, this, &key)? && it.get(this, &key)?.strict_eq(&target) {
                    return Ok(Value::Num(k as f64));
                }
            }
            Value::Num(-1.0)
        }
        "lastIndexOf" => {
            let last = len as f64 - 1.0;
            let n = if a.len() > 1 { num(it, a, 1)? } else { last };
            let n = if n.is_nan() { 0.0 } else { n.trunc() };
            let n = if n < 0.0 { len as f64 + n } else { n.min(last) };
            if n >= 0.0 {
                for k in (0..=n as usize).rev() {
                    let key = k.to_string();
                    if has_property(it, this, &key)? && it.get(this, &key)?.strict_eq(&target) {
                        return Ok(Value::Num(k as f64));
                    }
                }
            }
            Value::Num(-1.0)
        }
        _ => {
            let (start, end) = (rel(it, &target, len, 0)?, rel(it, &arg(a, 1), len, len)?);
            let count = end.saturating_sub(start);
            let species = array_species_create(it, this, count)?;
            let result = match &species {
                Some(t) => t.clone(),
                None => {
                    crate::eval::array_length(it, count as f64)?;
                    Value::array(Vec::new())
                }
            };
            for k in start..end {
                let key = k.to_string();
                if has_property(it, this, &key)? {
                    let v = it.get(this, &key)?;
                    match (&species, &result) {
                        (None, Value::Obj(r)) => r.borrow_mut().store_element(k - start, v),
                        _ => crate::eval::define_field(
                            it,
                            &result,
                            &(k - start).to_string(),
                            v,
                            false,
                        )?,
                    }
                }
            }
            match (&species, &result) {
                (None, Value::Obj(r)) => r.borrow_mut().set_array_length(count),
                _ => it.put_or_throw(&result, "length", Value::Num(count as f64))?,
            }
            result
        }
    })
}

/// Array.prototype.concat (ECMA-262 §23.1.3.1) on `this`: it and each argument are spread
/// by IsConcatSpreadable (§23.1.3.1.1), holes kept, into the ArraySpeciesCreate of `this`.
fn array_concat(it: &mut Interp, this: &Value, a: &[Value]) -> R<Value> {
    const MAX_SAFE: usize = 9007199254740991;
    let this = match this {
        Value::Undefined | Value::Null => {
            return it.no_object(this);
        }
        Value::Obj(_) => this.clone(),
        _ => it.to_object(this),
    };
    let target = array_species_create(it, &this, 0)?;
    let mut v = Vec::new();
    let mut h = Vec::new();
    for x in std::iter::once(&this).chain(a) {
        if !is_concat_spreadable(it, x)? {
            if v.len() >= MAX_SAFE {
                return it.throw("TypeError", "Array.prototype.concat: result too long");
            }
            v.push(x.clone());
            continue;
        }
        if let Value::Obj(xo) = x
            && xo.borrow().array.is_some()
            && it.elements_are_dense(xo)
        {
            let xb = xo.borrow();
            let at = v.len();
            h.extend(xb.holes.iter().flat_map(|xh| xh.iter()).map(|i| i + at));
            v.extend(xb.array.iter().flatten().cloned());
            continue;
        }
        let n = it.get(x, "length")?;
        let n = to_length(it, &n)?;
        if v.len() + n > MAX_SAFE {
            return it.throw("TypeError", "Array.prototype.concat: result too long");
        }
        // The result is an Array, whose `length` cannot pass 2^32 - 1 (§10.4.2.4).
        if target.is_none() && v.len() + n > u32::MAX as usize {
            return it.throw("RangeError", "Invalid array length");
        }
        for i in 0..n {
            let k = i.to_string();
            if has_property(it, x, &k)? {
                v.push(it.get(x, &k)?);
            } else {
                h.push(v.len());
                v.push(Value::Undefined);
            }
        }
    }
    bare(species_result(it, target, v, h, true)?)
}

/// IsConcatSpreadable (ECMA-262 §23.1.3.1.1): `@@isConcatSpreadable` if defined, else IsArray.
fn is_concat_spreadable(it: &mut Interp, v: &Value) -> R<bool> {
    if !matches!(v, Value::Obj(_)) {
        return Ok(false);
    }
    match it.get_sym(v, &crate::value::WellKnown::IsConcatSpreadable.sym())? {
        Value::Undefined => crate::props::is_array(it, v),
        s => Ok(s.truthy()),
    }
}

/// `o` is a plain object that restricts writes (frozen, sealed, non-extensible or with a
/// read-only property) and whose elements below `len` are all data properties: QV4's `push`
/// and `sort` work on the element storage of such an object as on an array's, past the
/// restriction.
fn stored_elements(o: &Obj, len: usize) -> bool {
    let b = o.borrow();
    matches!(b.internal, Internal::Ordinary)
        && b.array.is_none()
        && b.call.is_none()
        && b.restricted()
        && b.props.contains_key("0")
        && !b.accessors.as_ref().is_some_and(|m| {
            m.keys()
                .any(|k| crate::eval::array_index(k).is_some_and(|i| i < len))
        })
}

/// `o` is an array of `len` elements, all in its storage and none with attributes or an
/// accessor.
fn plain_storage(o: &Obj, len: usize) -> bool {
    let b = o.borrow();
    let index = |k: &Rc<str>| crate::eval::array_index(k).is_some();
    !b.is_proxy()
        && !b.has_bytes()
        && !b.sparse()
        && b.array.as_ref().is_some_and(|a| a.len() == len)
        && !b.attrs.as_ref().is_some_and(|m| m.keys().any(index))
        && !b.accessors.as_ref().is_some_and(|m| m.keys().any(index))
}

/// The mutating %Array.prototype% methods on object `o` of length `len`, as their ECMA-262
/// algorithms (§23.1.3): the same HasProperty / Get / `Set(.., true)` / DeletePropertyOrThrow
/// steps in the same order, so a frozen, sealed or non-extensible receiver refuses exactly the
/// writes the spec refuses, at the point the spec refuses them.
fn array_mutate(it: &mut Interp, o: &Obj, name: &str, a: &[Value], len: usize) -> R<Value> {
    const MAX_LENGTH: f64 = 9007199254740991.0;
    const UINT_MAX: usize = u32::MAX as usize;
    let this = Value::Obj(o.clone());
    // QV4 words a write or delete that `fill`, `copyWithin` and `sort` are refused by its
    // property; the other methods throw its plain TypeError.
    let plain = !matches!(name, "fill" | "copyWithin" | "sort");
    let put = |it: &mut Interp, k: &str, v: Value| -> R<()> {
        if !plain {
            return it.put_or_throw(&this, k, v);
        }
        match it.try_put(&this, k, v)? {
            true => Ok(()),
            false => it.throw("TypeError", "Type error"),
        }
    };
    let del = |it: &mut Interp, k: &str| -> R<()> {
        if !plain {
            return delete_or_throw(it, o, k);
        }
        match crate::props::delete(it, o, k)? {
            true => Ok(()),
            false => it.throw("TypeError", "Type error"),
        }
    };
    let set = |it: &mut Interp, k: usize, v: Value| put(it, &k.to_string(), v);
    let set_len = |it: &mut Interp, n: usize| put(it, "length", Value::Num(n as f64));
    // Copies index `from` to `to`, or deletes `to` where `from` is a hole.
    let shift_one = |it: &mut Interp, from: usize, to: usize| -> R<()> {
        let from = from.to_string();
        if has_property(it, &this, &from)? {
            let v = it.get(&this, &from)?;
            put(it, &to.to_string(), v)
        } else {
            del(it, &to.to_string())
        }
    };
    let too_long = |it: &Interp, n: usize| -> R<()> {
        if n as f64 > MAX_LENGTH {
            return it.throw(
                "TypeError",
                format!("Array.prototype.{name}: length exceeds 2^53 - 1"),
            );
        }
        Ok(())
    };
    Ok(match name {
        // QV4 appends to the storage of an array (`SimpleArrayData::putArray`) and sets its
        // length unchecked: a frozen, sealed or non-extensible array, or one whose `length`
        // is read-only, grows all the same. No attribute byte is written: the new elements
        // of an array that has bytes are readable, but no own properties.
        "push"
            if o.borrow().array.is_some()
                && o.borrow().tail == 0
                && !o.borrow().is_proxy()
                && !o.borrow().sparse() =>
        {
            too_long(it, len + a.len())?;
            let b = &mut *o.borrow_mut();
            let at = b.array.as_ref().map_or(0, Vec::len);
            b.array.get_or_insert_default().extend(a.iter().cloned());
            for k in at..at + a.len() {
                b.stored_without_byte(k);
            }
            Value::Num((at + a.len()) as f64)
        }
        // QV4 appends to an object's element storage too, extensible or not, and then
        // writes `length`: a non-extensible object grows, a frozen one throws at `length`
        // (here before anything is stored).
        "push" if stored_elements(o, len) => {
            too_long(it, len + a.len())?;
            if o.borrow().attr("length") & crate::value::READONLY != 0 {
                return it.throw("TypeError", "Type error");
            }
            for (i, v) in a.iter().enumerate() {
                set_own(o, &(len + i).to_string(), v.clone());
            }
            set_len(it, len + a.len())?;
            Value::Num((len + a.len()) as f64)
        }
        "push" => {
            // Past 2^53 - 1 Qt goes on in doubles: the key is the double's text and the new
            // length the double.
            for (i, v) in a.iter().enumerate() {
                let k = Value::Num(len as f64 + i as f64).to_js_string();
                put(it, &k, v.clone())?;
            }
            let n = Value::Num(len as f64 + a.len() as f64);
            put(it, "length", n.clone())?;
            n
        }
        "pop" => {
            if len == 0 {
                set_len(it, 0)?;
                return Ok(Value::Undefined);
            }
            let k = (len - 1).to_string();
            let v = it.get(&this, &k)?;
            del(it, &k)?;
            set_len(it, len - 1)?;
            v
        }
        "shift" => {
            if len == 0 {
                set_len(it, 0)?;
                return Ok(Value::Undefined);
            }
            let first = it.get(&this, "0")?;
            for k in 1..len {
                shift_one(it, k, k - 1)?;
            }
            del(it, &(len - 1).to_string())?;
            set_len(it, len - 1)?;
            first
        }
        // QV4 inserts at the front of an array's storage when no element has attributes and
        // sets its length unchecked: a non-extensible array grows, a read-only `length` too.
        "unshift" if plain_storage(o, len) => {
            too_long(it, len + a.len())?;
            let b = &mut *o.borrow_mut();
            b.array
                .get_or_insert_default()
                .splice(0..0, a.iter().cloned());
            b.remap_holes(|i| Some(i + a.len()));
            b.shift_slots(a.len() as isize);
            Value::Num((len + a.len()) as f64)
        }
        "unshift" => {
            let n = a.len();
            if n > 0 {
                match plain_indices(it, o, len) {
                    // The elements are all there is to move: every index below `len` that
                    // is not one of them is a hole and stays one.
                    Some(indices) => {
                        let mut moved = Vec::with_capacity(indices.len());
                        for k in indices.into_iter().rev() {
                            let key = k.to_string();
                            moved.push((k + n, it.get(&this, &key)?));
                            delete_or_throw(it, o, &key)?;
                        }
                        for (k, v) in moved {
                            set(it, k, v)?;
                        }
                    }
                    None => {
                        for k in (0..len).rev() {
                            shift_one(it, k, k + n)?;
                        }
                    }
                }
                for (j, v) in a.iter().enumerate() {
                    set(it, j, v.clone())?;
                }
            }
            // Qt adds in 32 bits (`uint newLen = len + argc`).
            let new_len = (len + n) & UINT_MAX;
            set_len(it, new_len)?;
            Value::Num(new_len as f64)
        }
        "splice" => {
            let start = rel(it, &arg(a, 0), len, 0)?;
            let dc = match a.len() {
                0 => 0,
                1 => len - start,
                _ => (num(it, a, 1)?.trunc().max(0.0) as usize).min(len - start),
            };
            let items = a.get(2..).unwrap_or_default();
            if len - dc + items.len() > UINT_MAX - 1 {
                return it.throw(
                    "TypeError",
                    "Array.prototype.splice: length exceeds 2^32 - 2",
                );
            }
            if dc > UINT_MAX - 1 {
                return it.throw("RangeError", "Array length out of range.");
            }
            let target = array_species_create(it, &this, dc)?;
            let mut removed = Vec::with_capacity(dc);
            let mut holes = Vec::new();
            for k in 0..dc {
                let from = (start + k).to_string();
                if has_property(it, &this, &from)? {
                    removed.push(it.get(&this, &from)?);
                } else {
                    removed.push(Value::Undefined);
                    holes.push(k);
                }
            }
            if items.len() < dc {
                for k in start..len - dc {
                    shift_one(it, k + dc, k + items.len())?;
                }
                for k in (len - dc + items.len()..len).rev() {
                    del(it, &k.to_string())?;
                }
            } else if items.len() > dc {
                for k in (start..len - dc).rev() {
                    shift_one(it, k + dc, k + items.len())?;
                }
            }
            // QV4 does not look at what the write of an inserted item answers.
            for (i, v) in items.iter().enumerate() {
                it.try_put(&this, &(start + i).to_string(), v.clone())?;
            }
            set_len(it, len - dc + items.len())?;
            species_result(it, target, removed, holes, true)?
        }
        "reverse" => {
            if len >= UINT_MAX {
                return it.throw(
                    "RangeError",
                    "Array.prototype.reverse: Length out of range.",
                );
            }
            for lower in 0..len / 2 {
                let upper = len - lower - 1;
                let (lk, uk) = (lower.to_string(), upper.to_string());
                let lv = match has_property(it, &this, &lk)? {
                    true => Some(it.get(&this, &lk)?),
                    false => None,
                };
                let uv = match has_property(it, &this, &uk)? {
                    true => Some(it.get(&this, &uk)?),
                    false => None,
                };
                match (lv, uv) {
                    (Some(l), Some(u)) => {
                        set(it, lower, u)?;
                        set(it, upper, l)?;
                    }
                    (None, Some(u)) => {
                        set(it, lower, u)?;
                        del(it, &uk)?;
                    }
                    (Some(l), None) => {
                        del(it, &lk)?;
                        set(it, upper, l)?;
                    }
                    (None, None) => {}
                }
            }
            this.clone()
        }
        "fill" => {
            let v = arg(a, 0);
            let (s, e) = (rel(it, &arg(a, 1), len, 0)?, rel(it, &arg(a, 2), len, len)?);
            if e > UINT_MAX {
                return it.throw("RangeError", "Array length out of range.");
            }
            for k in s..e {
                set(it, k, v.clone())?;
            }
            this.clone()
        }
        "sort" => {
            // SortIndexedProperties (§23.1.3.30.1) skipping holes, then the sorted values
            // written back from index 0 and the holes deleted at the end.
            let mut items = Vec::with_capacity(len);
            for k in 0..len {
                let k = k.to_string();
                if has_property(it, &this, &k)? {
                    items.push(it.get(&this, &k)?);
                }
            }
            let sorted = Value::array(items);
            method_impl(it, &sorted, "sort", a)?;
            let Value::Obj(so) = &sorted else {
                unreachable!()
            };
            let items = so.borrow().array.clone().unwrap_or_default();
            // QV4 sorts an array's storage in place, whatever its elements' attributes, and
            // moves the holes to the end; each position keeps its attributes.
            let mut b = o.borrow_mut();
            if !b.is_proxy()
                && !crate::valuetypes::is_readonly_sequence(&b)
                && b.array.as_ref().is_some_and(|to| to.len() == len)
                && (0..len).filter(|&i| !b.is_hole(i)).count() == items.len()
            {
                let n = items.len();
                let mut items = items;
                items.resize(len, Value::Undefined);
                b.array = Some(items);
                let filled: Vec<usize> = (0..n).filter(|&k| b.is_hole(k)).collect();
                if let Some(h) = &mut b.holes {
                    h.clear();
                }
                b.add_holes(n..len);
                for k in filled {
                    b.stored_without_byte(k);
                }
                return Ok(this.clone());
            }
            drop(b);
            // The same for an object's elements: read-only ones are overwritten, and the
            // holes move to the end of a frozen object.
            if stored_elements(o, len) {
                let bits = o.borrow().attr("0");
                for (k, v) in items.iter().enumerate() {
                    let k = k.to_string();
                    set_own(o, &k, v.clone());
                    o.borrow_mut().set_attr(&k, bits);
                }
                let b = &mut *o.borrow_mut();
                for k in items.len()..len {
                    let k = k.to_string();
                    if b.props.remove(&*k).is_some() {
                        b.keys.retain(|x| **x != *k);
                        b.forget_order(&k);
                        b.set_attr(&k, 0);
                    }
                }
                return Ok(this.clone());
            }
            for (k, v) in items.iter().enumerate() {
                set(it, k, v.clone())?;
            }
            for k in items.len()..len {
                del(it, &k.to_string())?;
            }
            this.clone()
        }
        "copyWithin" => {
            let to = rel(it, &arg(a, 0), len, 0)?;
            let (from, end) = (rel(it, &arg(a, 1), len, 0)?, rel(it, &arg(a, 2), len, len)?);
            let count = end.saturating_sub(from).min(len - to);
            if from < to && to < from + count {
                for i in (0..count).rev() {
                    shift_one(it, from + i, to + i)?;
                }
            } else {
                for i in 0..count {
                    shift_one(it, from + i, to + i)?;
                }
            }
            this.clone()
        }
        _ => unreachable!("{name} is not in ARRAY_MUTATORS"),
    })
}

/// DeletePropertyOrThrow (ECMA-262 §7.3.10).
/// The indices below `len` of `o`'s own elements, ascending, when those are all the indexed
/// properties a method that walks `0..len` would find and reading or writing one runs no
/// code: `o` is an ordinary object or array without accessors, and no object of its
/// prototype chain has an indexed property or is exotic.
fn plain_indices(it: &Interp, o: &Obj, len: usize) -> Option<Vec<usize>> {
    let mut indices: Vec<usize> = {
        let b = o.borrow();
        if b.is_proxy()
            || !matches!(b.internal, Internal::Ordinary)
            || b.accessors.as_ref().is_some_and(|m| !m.is_empty())
            || b.restricted()
        {
            return None;
        }
        let dense = b.array.as_ref().map_or(0, |a| a.len());
        (0..dense)
            .filter(|i| b.holes.as_ref().is_none_or(|h| !h.contains(*i)))
            .chain(b.props.keys().filter_map(|k| crate::eval::array_index(k)))
            .filter(|&i| i < len)
            .collect()
    };
    let mut cur = it.proto_of(o);
    while let Some(p) = cur {
        {
            let b = p.borrow();
            if b.is_proxy()
                || !matches!(b.internal, Internal::Ordinary)
                || b.array.as_ref().is_some_and(|a| !a.is_empty())
                || (b.props.keys())
                    .chain(b.accessors.iter().flat_map(|m| m.keys()))
                    .any(|k| crate::eval::array_index(k).is_some())
            {
                return None;
            }
        }
        cur = it.proto_of(&p);
    }
    indices.sort_unstable();
    Some(indices)
}

fn delete_or_throw(it: &mut Interp, o: &Obj, k: &str) -> R<()> {
    if crate::props::delete(it, o, k)? {
        return Ok(());
    }
    it.throw("TypeError", format!("Cannot delete property '{k}'"))
}

/// HasProperty (ECMA-262 §7.3.12) as the array methods ask it: `k` is an own property of
/// `v` or of its prototype chain. QV4's methods read an array's storage (`Object::get` with
/// `hasProperty`), so an element stored there is found also when it is no property.
fn has_property(it: &mut Interp, v: &Value, k: &str) -> R<bool> {
    Ok(match v {
        Value::Obj(o) => {
            let stored = {
                let b = o.borrow();
                b.attrs.is_some()
                    && b.array.as_ref().is_some_and(|a| {
                        crate::eval::idx(k).is_some_and(|i| i < a.len() && !b.is_hole(i))
                    })
            };
            stored || it.has_property(o, k)?
        }
        Value::Str(_) | Value::Str16(_) => {
            crate::eval::idx(k).is_some_and(|i| i < crate::utf16::len_of(v))
        }
        Value::Item(_) => true,
        _ => false,
    })
}

/// (array, Map/Set, RegExp, function) classification [`method`] dispatches on.
fn obj_kind(o: &Obj) -> (bool, bool, bool, bool) {
    let b = o.borrow();
    (
        b.array.is_some(),
        b.internal.collection().is_some(),
        matches!(b.internal, Internal::RegExp { .. }),
        b.call.is_some(),
    )
}

/// Whether [`method`] has `this.name` (V4 finds these on the prototype): an optional call
/// resolves its callee with it before evaluating the arguments (ECMA-262 OptionalChain).
/// [`method`] dispatches only the names this admits, and the `every_listed_builtin_dispatches`
/// test checks each admitted name has its arm, so the two agree both ways. Item methods are
/// the host's single dispatch ([`crate::eval::Host::native`]).
pub fn has_method(it: &mut Interp, this: &Value, name: &str) -> bool {
    match this {
        Value::Item(id) => it.host.has_method(*id, name),
        _ => is_builtin_method(this, name) && resolves_to_builtin(it, this, name),
    }
}

/// The receiver kinds' built-in prototypes and the methods [`method`] dispatches for them.
fn builtin_method_protos(it: &Interp) -> [(&Obj, &'static [&'static str]); 8] {
    [
        (&it.string_proto, STRING_METHODS),
        (&it.number_proto, NUMBER_METHODS),
        (&it.boolean_proto, BOOLEAN_METHODS),
        (&it.array_proto, ARRAY_METHODS),
        (&it.regexp_proto, REGEXP_METHODS),
        (&it.function_proto, FUNCTION_METHODS),
        (&it.map_proto, MAP_METHODS),
        (&it.set_proto, SET_METHODS),
    ]
}

/// The installed built-in methods, for [`crate::symbol::Realm::builtin_methods`].
pub fn builtin_methods(it: &Interp) -> std::collections::HashMap<(usize, &'static str), Value> {
    let mut m = std::collections::HashMap::new();
    for (p, names) in builtin_method_protos(it) {
        let b = p.borrow();
        for &n in names {
            if let Some(v) = b.props.get(n) {
                m.insert((Rc::as_ptr(p) as usize, n), v.clone());
            }
        }
    }
    m
}

/// Whether `this.name` is the built-in method [`method`] runs for it: a [[Get]] along the
/// prototype chain (without running accessors) that finds the method as installed. An own
/// or replaced property, an accessor or a proxy on the way takes the ordinary call path
/// (ECMA-262 §13.3.6.1 EvaluateCall, §7.1.1.1 OrdinaryToPrimitive). Pseudo-methods without
/// a property (a color's, a signal's `connect`) stay with the fast path.
fn resolves_to_builtin(it: &Interp, this: &Value, name: &str) -> bool {
    let mut cur = match this {
        Value::Str(_) | Value::Str16(_) => it.string_proto.clone(),
        Value::Num(_) => it.number_proto.clone(),
        Value::Bool(_) => it.boolean_proto.clone(),
        Value::Obj(o) => o.clone(),
        _ => return true,
    };
    loop {
        let b = cur.borrow();
        if b.is_proxy() || crate::eval::own_accessor(&b, name).is_some() {
            return false;
        }
        if let Some(v) = b.props.get(name) {
            return it
                .sym
                .builtin_methods
                .get(&(Rc::as_ptr(&cur) as usize, name))
                .is_some_and(|m| m.strict_eq(v));
        }
        if crate::eval::intrinsic(&b, name).is_some() {
            return false;
        }
        drop(b);
        match it.proto_of(&cur) {
            Some(p) => cur = p,
            None => {
                return !builtin_method_protos(it)
                    .iter()
                    .any(|(_, names)| names.contains(&name));
            }
        }
    }
}

/// [`has_method`] for a value that is not a QML object: whether its built-in prototype (the
/// one before %Object.prototype% in its chain) has method `name`.
pub fn is_builtin_method(this: &Value, name: &str) -> bool {
    match this {
        Value::Str(_) | Value::Str16(_) => STRING_METHODS.contains(&name),
        Value::Num(_) => NUMBER_METHODS.contains(&name),
        Value::Bool(_) => BOOLEAN_METHODS.contains(&name),
        Value::Color(_) => matches!(name, "toString" | "lighter" | "darker" | "alpha" | "tint"),
        // A proxy's methods are what its [[Get]] finds.
        Value::Obj(o) if o.borrow().is_proxy() => false,
        Value::Obj(o) => match obj_kind(o) {
            (true, ..) => ARRAY_METHODS.contains(&name),
            (_, true, ..) => match o.borrow().internal {
                Internal::Map(_) => MAP_METHODS.contains(&name),
                _ => SET_METHODS.contains(&name),
            },
            (_, _, true, _) => REGEXP_METHODS.contains(&name),
            (.., true) => {
                FUNCTION_METHODS.contains(&name)
                    || matches!(name, "connect" | "disconnect")
                        && matches!(o.borrow().call, Some(Callable::Item(..)))
            }
            _ => false,
        },
        _ => false,
    }
}

pub fn method(it: &mut Interp, this: &Value, name: &str, a: &[Value]) -> R<Option<Value>> {
    if let Value::Item(id) = this {
        return match it.host_call(*id, name, a) {
            Some(r) => r.map(Some),
            // The object's own function (a QML function, a native object's method).
            None if it.host.get(*id, name).is_some() => Ok(None),
            // A QObject wrapper's prototype is Object.prototype (`item.hasOwnProperty(k)`).
            None => {
                let proto = it.object_proto.clone();
                let f = it.get_from(&proto, name, this)?;
                if crate::eval::callable(&f).is_none() {
                    return Ok(None);
                }
                it.call(&f, this.clone(), a).map(Some)
            }
        };
    }
    if !has_method(it, this, name) {
        return Ok(None);
    }
    method_impl(it, this, name, a)
}

pub(crate) fn method_impl(
    it: &mut Interp,
    this: &Value,
    name: &str,
    a: &[Value],
) -> R<Option<Value>> {
    match this {
        Value::Str(_) | Value::Str16(_) => string_method(it, this, name, a),
        Value::Num(n) => Ok(match name {
            "toFixed" => {
                let d = num(it, a, 0)?;
                let d = if d.is_nan() { 0.0 } else { d.trunc() };
                if !(0.0..=100.0).contains(&d) {
                    return it.throw("RangeError", format!("{} out of range", num_to_string(*n)));
                }
                Some(Value::str(&to_fixed(*n, d as usize)))
            }
            "toString" => {
                let radix = match arg(a, 0) {
                    Value::Undefined => 10.0,
                    r => it.to_numeric(&r)?.trunc(),
                };
                if !(2.0..=36.0).contains(&radix) {
                    return it.throw(
                        "Error",
                        format!(
                            "Number.prototype.toString: {} is not a valid radix",
                            num_to_string(radix)
                        ),
                    );
                }
                Some(Value::str(&if radix == 10.0 || !n.is_finite() {
                    num_to_string(*n)
                } else {
                    radix_str(*n, radix as u32)
                }))
            }
            "toPrecision" => match arg(a, 0) {
                Value::Undefined => Some(Value::str(&num_to_string(*n))),
                p => {
                    let p = it.to_numeric(&p)?.trunc();
                    if !n.is_finite() {
                        Some(Value::str(&num_to_string(*n)))
                    } else if !(1.0..=100.0).contains(&p) {
                        return it.throw(
                            "RangeError",
                            "Number.prototype.toPrecision: precision out of range out of range",
                        );
                    } else {
                        Some(Value::str(&to_precision(*n, p as usize)))
                    }
                }
            },
            "toExponential" => {
                let f = arg(a, 0);
                let d = it.to_numeric(&f)?.trunc();
                if !n.is_finite() {
                    Some(Value::str(&num_to_string(*n)))
                } else if !matches!(f, Value::Undefined) && !(0.0..=100.0).contains(&d) {
                    return it.throw(
                        "RangeError",
                        "Number.prototype.toExponential: fractionDigits out of range out of range",
                    );
                } else {
                    let d = (!matches!(f, Value::Undefined)).then_some(d as usize);
                    Some(Value::str(&to_exponential(*n, d)))
                }
            }
            "toLocaleString" | "toLocaleCurrencyString" => Some(number_locale(it, this, name, a)?),
            "valueOf" => Some(Value::Num(*n)),
            _ => None,
        }),
        Value::Bool(b) => Ok(match name {
            "toString" => Some(Value::str(&b.to_string())),
            "valueOf" => Some(Value::Bool(*b)),
            _ => None,
        }),
        Value::Color(c) => Ok(match name {
            "toString" => Some(Value::str(&color_name(c))),
            // QQuickColorValueType: `lighter(real factor = 1.5)`, `darker(real factor = 2.0)`,
            // `alpha(real value)`, `tint(color tintColor)`.
            "lighter" | "darker" => {
                let f = match a.first() {
                    None | Some(Value::Undefined) if name == "lighter" => 1.5,
                    None | Some(Value::Undefined) => 2.0,
                    Some(_) => num(it, a, 0)?,
                };
                // QColor takes `int(qRound(factor * 100))`: a factor that is not a number
                // gives an invalid colour, one past `int` leaves the colour as it is.
                let pct = (f * 100.0).round();
                Some(Value::Color(if f.is_nan() {
                    Color::INVALID
                } else if !(1.0..=i32::MAX as f64).contains(&pct) {
                    *c
                } else if name == "lighter" {
                    c.lighter(pct as i32)
                } else {
                    c.darker(pct as i32)
                }))
            }
            "alpha" => {
                if a.is_empty() {
                    return it.throw("Error", "Insufficient arguments");
                }
                // QColor::setAlphaF bounds the value to 0..1 (a NaN to 0).
                Some(Value::Color(c.with_alpha_f(num(it, a, 0)? as f32)))
            }
            "tint" => match a.first() {
                None => return it.throw("Error", "Insufficient arguments"),
                Some(Value::Undefined) => {
                    return it.throw(
                        "TypeError",
                        "Passing incompatible arguments to C++ functions from JavaScript is not allowed.",
                    );
                }
                // A value that is no colour is an invalid one, which is opaque: the result.
                Some(t) => Some(Value::Color(crate::qtobj::tint_color(*c, to_color(t)))),
            },
            _ => None,
        }),
        Value::Obj(o) => {
            let (is_arr, is_coll, is_re, is_fn) = obj_kind(o);
            if is_arr
                && crate::valuetypes::is_readonly_sequence(&o.borrow())
                && let Some(why @ "Type error") = crate::valuetypes::refused_sequence_write(name)
            {
                return it.throw("TypeError", why);
            }
            if is_arr {
                // A frozen, sealed or non-extensible array writes through [[Set]] and
                // [[Delete]], which may refuse (and then throw, §23.1.3).
                // So does an array whose elements are not all in its dense storage, read
                // through [[HasProperty]] and [[Get]].
                if (ARRAY_MUTATORS.contains(&name) && o.borrow().restricted())
                    || (ARRAY_METHODS.contains(&name) && !it.elements_are_dense(o))
                {
                    return array_generic(it, this, name, a).map(Some);
                }
                return array_method(it, o, name, a, Some(this));
            }
            if is_coll {
                return coll_method(it, o, name, a);
            }
            if is_re {
                return Ok(match name {
                    "exec" => {
                        let s = it.to_str_named(&arg(a, 0))?;
                        let s16 = units(&s);
                        let (re, _) = compile(it, this)?.unwrap();
                        let h = regex_exec(it, this, &re, &s16)?;
                        Some(h.map_or(Value::Null, |h| hit_array(&s, &s16, &h)))
                    }
                    "test" => Some(regexp_test(it, this, a)?),
                    "toString" => Some(regexp_to_string(it, this)?),
                    _ => None,
                });
            }
            if is_fn {
                return Ok(match name {
                    "call" => Some(it.call(this, arg(a, 0), a.get(1..).unwrap_or(&[]))?),
                    "connect" | "disconnect" => {
                        let Some(Callable::Item(item, sig)) = crate::eval::callable(this) else {
                            return Ok(None);
                        };
                        let what = format!("Function.prototype.{name}");
                        let (recv, f) = match a {
                            [] => return it.throw("Error", format!("{what}: no arguments given")),
                            [f] => (Value::Undefined, f.clone()),
                            [r, f, ..] => (r.clone(), f.clone()),
                        };
                        if crate::eval::callable(&f).is_none() {
                            return it.throw("Error", format!("{what}: target is not a function"));
                        }
                        // The receiver of a two-argument form: an object, or undefined
                        // (QV4's `Function.prototype.connect` checks it after the function).
                        if !matches!(
                            recv,
                            Value::Undefined
                                | Value::Obj(_)
                                | Value::Item(_)
                                | Value::Native(_)
                                | Value::Color(_)
                        ) {
                            return it
                                .throw("Error", format!("{what}: target this is not an object"));
                        }
                        if !it.host.connect(item, sig, recv, f, name == "connect") {
                            return it
                                .throw("Error", format!("{what}: this object is not a signal"));
                        }
                        Some(Value::Undefined)
                    }
                    "bind" => {
                        let args = a.get(1..).unwrap_or(&[]).to_vec();
                        // SetFunctionLength(F, max(0, ToIntegerOrInfinity(target.length) - argCount)).
                        let length = match it.get(this, "length")? {
                            Value::Num(n) if n == f64::INFINITY => n,
                            Value::Num(n) if !n.is_nan() => {
                                (n.trunc() - args.len() as f64).max(0.0) + 0.0
                            }
                            _ => 0.0,
                        };
                        let target = this.clone();
                        let f = fn_value(Callable::Bound(Rc::new(BoundFn {
                            target,
                            this: arg(a, 0),
                            args,
                            length,
                        })));
                        // V4 gives a bound function its own `arguments` and `caller`, the
                        // throwing accessors of %Function.prototype%.
                        if let Value::Obj(b) = &f {
                            for k in ["arguments", "caller"] {
                                let halves =
                                    it.function_proto.borrow().accessors.as_ref().and_then(|m| {
                                        m.get(k).map(|h| (h.get.clone(), h.set.clone()))
                                    });
                                let Some((Some(get), Some(set))) = halves else {
                                    continue;
                                };
                                crate::eval::define_accessor(b, k, true, get, false);
                                crate::eval::define_accessor(b, k, false, set, false);
                                b.borrow_mut().set_attr(k, crate::value::PERMANENT);
                            }
                        }
                        Some(f)
                    }
                    "apply" => {
                        let args = match arg(a, 1) {
                            Value::Undefined | Value::Null => Vec::new(),
                            v => list_from_array_like(it, &v)?,
                        };
                        Some(it.call(this, arg(a, 0), &args)?)
                    }
                    "toString" => Some(Value::str(&function_to_string(it, this)?)),
                    _ => None,
                });
            }
            Ok(None)
        }
        Value::Item(id) => it.host_call(*id, name, a).transpose(),
        _ => Ok(None),
    }
}

/// `Object.prototype.__defineGetter__` / `__defineSetter__` (ECMA-262 §B.2.2.2-3): an
/// enumerable, configurable accessor half `getter` on ToObject(this).
fn legacy_accessor(it: &mut Interp, this: &Value, a: &[Value], getter: bool) -> R<Value> {
    to_object(it, this)?;
    let f = arg(a, 1);
    if crate::eval::callable(&f).is_none() {
        return it.throw("TypeError", "Type error");
    }
    let k = it.to_string(&arg(a, 0))?;
    if let Value::Obj(o) = this {
        // Annex B §B.2.2.2-3: DefinePropertyOrThrow with an enumerable, configurable accessor.
        let (get, set) = if getter {
            (Some(f), None)
        } else {
            (None, Some(f))
        };
        let d = crate::props::Desc {
            get,
            set,
            enumerable: Some(true),
            configurable: Some(true),
            ..Default::default()
        };
        if !crate::props::define(it, o, &k, &d)? {
            return it.throw("TypeError", "Type error");
        }
    }
    Ok(Value::Undefined)
}

// Inverse hyperbolic functions as glibc computes them (fdlibm e_acosh.c, s_asinh.c, e_atanh.c),
// which is what QV4's MathObject reaches through std::acosh and friends; Rust's std rounds
// differently (acosh(2) ends ...168 there, ...166 in Qt).
fn high_word(x: f64) -> u32 {
    (x.to_bits() >> 32) as u32 & 0x7fff_ffff
}

fn libm_acosh(x: f64) -> f64 {
    let hx = (x.to_bits() >> 32) as u32;
    if x < 1.0 || x.is_nan() {
        f64::NAN
    } else if hx >= 0x41b0_0000 {
        if x.is_infinite() {
            x
        } else {
            x.ln() + std::f64::consts::LN_2
        }
    } else if x == 1.0 {
        0.0
    } else if hx > 0x4000_0000 {
        (2.0 * x - 1.0 / (x + (x * x - 1.0).sqrt())).ln()
    } else {
        let t = x - 1.0;
        (t + (2.0 * t + t * t).sqrt()).ln_1p()
    }
}

fn libm_asinh(x: f64) -> f64 {
    let ix = high_word(x);
    if !(0x3e30_0000..0x7ff0_0000).contains(&ix) {
        return x;
    }
    let a = x.abs();
    let w = if ix > 0x41b0_0000 {
        a.ln() + std::f64::consts::LN_2
    } else if ix > 0x4000_0000 {
        (2.0 * a + 1.0 / ((x * x + 1.0).sqrt() + a)).ln()
    } else {
        let t = x * x;
        (a + t / (1.0 + (1.0 + t).sqrt())).ln_1p()
    };
    w.copysign(x)
}

fn libm_atanh(x: f64) -> f64 {
    let a = x.abs();
    let t = if a < 0.5 {
        if a < f64::powi(2.0, -28) {
            return x;
        }
        let t = a + a;
        0.5 * (t + t * a / (1.0 - a)).ln_1p()
    } else if a < 1.0 {
        0.5 * ((a + a) / (1.0 - a)).ln_1p()
    } else if a == 1.0 {
        f64::INFINITY
    } else {
        return f64::NAN;
    };
    t.copysign(x)
}

/// `String.fromCharCode`, `String.fromCodePoint` and `String.raw` (ECMA-262 §22.1.2).
fn install_string_statics(string: &Obj) {
    nat(string, "fromCharCode", 1, |it, _, a| {
        let mut u = Vec::with_capacity(a.len());
        for v in a {
            u.push(crate::eval::to_i32(it.to_numeric(v)?) as u16);
        }
        Ok(from16(&u))
    });
    nat(string, "fromCodePoint", 1, |it, _, a| {
        let mut out = Vec::with_capacity(a.len());
        for v in a {
            let n = it.to_numeric(v)?;
            if !(0.0..=1114111.0).contains(&n) || n.fract() != 0.0 {
                return it.throw("RangeError", "String.fromCodePoint: argument out of range.");
            }
            // UTF16EncodeCodePoint (ECMA-262 §11.1.1): a surrogate code point is its unit.
            match char::from_u32(n as u32) {
                Some(c) => out.extend_from_slice(c.encode_utf16(&mut [0; 2])),
                None => out.push(n as u16),
            }
        }
        Ok(from16(&out))
    });
    nat(string, "raw", 1, |it, _, a| {
        let cooked = arg(a, 0);
        if matches!(cooked, Value::Undefined | Value::Null) {
            return it.throw("TypeError", "Type error");
        }
        let raw = it.get(&cooked, "raw")?;
        if matches!(raw, Value::Undefined | Value::Null) {
            return it.no_object(&raw);
        }
        let n = it.get(&raw, "length")?;
        let n = it.to_numeric(&n)?;
        let len = if n > 0.0 {
            n.trunc().min(9007199254740991.0) as usize
        } else {
            0
        };
        let mut out = crate::value::JsBuf::new();
        for i in 0..len {
            let seg = it.get(&raw, &i.to_string())?;
            out.push_value(&it.to_str_value(&seg)?);
            if i + 1 < len
                && let Some(sub) = a.get(i + 1)
            {
                out.push_value(&it.to_str_value(sub)?);
            }
        }
        Ok(out.finish())
    });
}

/// The %Function.prototype% members past [`FUNCTION_METHODS`]: the `arguments`/`caller`
/// accessors whose halves are both %ThrowTypeError% (ECMA-262 §10.2.4 AddRestrictedFunctionProperties)
/// and QML's signal `connect`/`disconnect`, which throw for a function that is not a signal.
fn install_function_prototype_extras(p: &Obj) {
    let thrower = fn_value(Callable::Native("", |it, _, _| {
        it.throw("TypeError", "Type error")
    }));
    for k in ["arguments", "caller"] {
        crate::eval::define_accessor(p, k, true, thrower.clone(), false);
        crate::eval::define_accessor(p, k, false, thrower.clone(), false);
    }
    nat(p, "connect", 0, |it, this, a| {
        signal_link(it, this, "connect", a)
    });
    nat(p, "disconnect", 0, |it, this, a| {
        signal_link(it, this, "disconnect", a)
    });
}

fn signal_link(it: &mut Interp, this: &Value, name: &str, a: &[Value]) -> R<Value> {
    match method_impl(it, this, name, a)? {
        Some(v) => Ok(v),
        None => it.throw(
            "Error",
            format!("Function.prototype.{name}: this object is not a signal"),
        ),
    }
}

/// The W3C DOM Level 2 ExceptionCode names, codes 1 up (Qt's `DOMException` global).
const DOM_EXCEPTION_CODES: [&str; 17] = [
    "INDEX_SIZE_ERR",
    "DOMSTRING_SIZE_ERR",
    "HIERARCHY_REQUEST_ERR",
    "WRONG_DOCUMENT_ERR",
    "INVALID_CHARACTER_ERR",
    "NO_DATA_ALLOWED_ERR",
    "NO_MODIFICATION_ALLOWED_ERR",
    "NOT_FOUND_ERR",
    "NOT_SUPPORTED_ERR",
    "INUSE_ATTRIBUTE_ERR",
    "INVALID_STATE_ERR",
    "SYNTAX_ERR",
    "INVALID_MODIFICATION_ERR",
    "NAMESPACE_ERR",
    "INVALID_ACCESS_ERR",
    "VALIDATION_ERR",
    "TYPE_MISMATCH_ERR",
];

/// The Web SQL Database SQLException code names, codes 1 up as Qt 6.12 numbers them (Qt's
/// `SQLException` global, `UNKNOWN_ERR` = 1).
const SQL_EXCEPTION_CODES: [&str; 8] = [
    "UNKNOWN_ERR",
    "DATABASE_ERR",
    "VERSION_ERR",
    "TOO_LARGE_ERR",
    "QUOTA_ERR",
    "SYNTAX_ERR",
    "CONSTRAINT_ERR",
    "TIMEOUT_ERR",
];

/// `escape(string)` (ECMA-262 §B.2.1.1): each UTF-16 code unit outside
/// `A-Za-z0-9@*_+-./` as `%XX` below 256, else `%uXXXX`.
fn js_escape(s: &[u16]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(s.len());
    for &u in s {
        match u8::try_from(u) {
            Ok(b) if b.is_ascii_alphanumeric() || b"@*_+-./".contains(&b) => out.push(b as char),
            Ok(b) => write!(out, "%{b:02X}").unwrap(),
            Err(_) => write!(out, "%u{u:04X}").unwrap(),
        }
    }
    out
}

/// `unescape(string)` (ECMA-262 §B.2.1.2): `%uXXXX` and `%XX` decoded, anything else kept.
fn js_unescape(u: &[u16]) -> Value {
    let hex = |r: &[u16]| {
        r.iter().try_fold(0u16, |acc, &d| {
            let d = char::from_u32(u32::from(d))?.to_digit(16)?;
            Some(acc * 16 + d as u16)
        })
    };
    let mut out = Vec::with_capacity(u.len());
    let mut k = 0;
    while k < u.len() {
        if u[k] == u16::from(b'%') {
            let (digits, skip) = if u.get(k + 1) == Some(&u16::from(b'u')) {
                (u.get(k + 2..k + 6), 6)
            } else {
                (u.get(k + 1..k + 3), 3)
            };
            if let Some(v) = digits.and_then(hex) {
                out.push(v);
                k += skip;
                continue;
            }
        }
        out.push(u[k]);
        k += 1;
    }
    from16(&out)
}

/// The name of %eval%, by which a call `eval(...)` knows its callee is that function: a
/// direct eval (ECMA-262 §13.3.6.1), which [`Interp::eval`] runs in the caller's scope.
pub const EVAL: &str = "eval";

/// `eval(x)` of a string (PerformEval, ECMA-262 §19.2.1.1) as indirect eval: global code.
#[cfg(feature = "interp")]
fn eval_code(it: &mut Interp, src: &str) -> R<Value> {
    match crate::ir::lower_eval(src) {
        Ok((code, strict)) => {
            let g = it.global.clone();
            let env = if strict { Interp::child(&g) } else { g };
            it.run_eval(&code, &env)
        }
        Err(e) => it.throw("SyntaxError", e),
    }
}

/// Without the interpreter there is nothing to evaluate source text with (as for
/// [`function_ctor`]).
#[cfg(not(feature = "interp"))]
fn eval_code(it: &mut Interp, _: &str) -> R<Value> {
    it.throw(
        "EvalError",
        "eval: code generation from strings needs the interpreter",
    )
}

/// `Function(p1, ..., body)` / `new Function(...)` (CreateDynamicFunction, ECMA-262
/// §20.2.1.1.1): a function of the global scope compiled from the joined parameters and body.
fn function_ctor(it: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
    let (params, body) = dynamic_source(it, a)?;
    compile_function(it, &params, &body, false)
}

/// %GeneratorFunction% (ECMA-262 §27.3.1.1): CreateDynamicFunction of kind `generator`.
pub(crate) fn generator_function_ctor(it: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
    let (params, body) = dynamic_source(it, a)?;
    compile_function(it, &params, &body, true)
}

/// The parameter list and body source of CreateDynamicFunction's arguments (§20.2.1.1.1).
fn dynamic_source(it: &mut Interp, a: &[Value]) -> R<(String, String)> {
    Ok(match a {
        [] => (String::new(), String::new()),
        [ps @ .., body] => {
            let mut names = Vec::with_capacity(ps.len());
            for p in ps {
                names.push(it.to_string(p)?);
            }
            (names.join(","), it.to_string(body)?)
        }
    })
}

#[cfg(feature = "interp")]
fn compile_function(it: &mut Interp, params: &str, body: &str, generator: bool) -> R<Value> {
    match crate::ir::lower_dynamic_function(params, body, generator) {
        Ok(code) => {
            let g = it.realm();
            it.run(&code, &g)
        }
        Err(_) => it.throw("SyntaxError", "Parse error"),
    }
}

/// Without the interpreter there is nothing to compile source text at run time with.
#[cfg(not(feature = "interp"))]
fn compile_function(it: &mut Interp, _: &str, _: &str, _: bool) -> R<Value> {
    it.throw(
        "EvalError",
        "Function: code generation from strings needs the interpreter",
    )
}

const ERROR_KINDS: [&str; 7] = [
    "Error",
    "TypeError",
    "RangeError",
    "SyntaxError",
    "ReferenceError",
    "EvalError",
    "URIError",
];

/// `Error(msg)` / `new TypeError(msg)`, ...: call and construct both build the error.
fn error_ctor<const K: usize>(it: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
    let msg = match arg(a, 0) {
        Value::Undefined => None,
        m => Some(it.to_string(&m)?),
    };
    Ok(it.make_error(ERROR_KINDS[K], msg))
}

/// `Error.prototype.toString` (ECMA-262 §20.5.3.4), generic over any object.
fn error_to_string(it: &mut Interp, this: &Value, _: &[Value]) -> R<Value> {
    if !matches!(this, Value::Obj(_) | Value::Item(_) | Value::Native(_)) {
        return it.type_error();
    }
    let name = match it.get(this, "name")? {
        Value::Undefined => "Error".to_string(),
        v => it.to_string(&v)?,
    };
    let msg = match it.get(this, "message")? {
        Value::Undefined => String::new(),
        v => it.to_string(&v)?,
    };
    Ok(Value::str(&match (name.is_empty(), msg.is_empty()) {
        (_, true) => name,
        (true, false) => msg,
        _ => format!("{name}: {msg}"),
    }))
}

/// `Number.prototype.toString(radix)` for a finite number and radix other than 10: V8's
/// `DoubleToRadixCString` (all of f64, fractions included; digits until the value is
/// unambiguous).
fn radix_str(value: f64, radix: u32) -> String {
    const CHARS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    const HALF: usize = 1100;
    let next_up = |x: f64| f64::from_bits(x.to_bits() + 1);
    // V8's Double::Exponent(): the power of two of the lowest mantissa bit.
    let exponent = |x: f64| {
        let e = ((x.to_bits() >> 52) & 0x7ff) as i32;
        if e == 0 { -1074 } else { e - 1075 }
    };
    let r = radix as f64;
    let mut buf = vec![0u8; 2 * HALF];
    let mut int_cur = HALF;
    let mut frac_cur = HALF;
    let neg = value < 0.0;
    let value = value.abs();
    let mut integer = value.floor();
    let mut fraction = value - integer;
    let mut delta = (0.5 * (next_up(value) - value)).max(next_up(0.0));
    if fraction >= delta {
        buf[frac_cur] = b'.';
        frac_cur += 1;
        loop {
            fraction *= r;
            delta *= r;
            let digit = fraction as usize;
            buf[frac_cur] = CHARS[digit];
            frac_cur += 1;
            fraction -= digit as f64;
            if (fraction > 0.5 || (fraction == 0.5 && digit & 1 == 1)) && fraction + delta > 1.0 {
                // Round up, carrying through the digits already written.
                loop {
                    frac_cur -= 1;
                    if frac_cur == HALF {
                        integer += 1.0;
                        break;
                    }
                    let c = buf[frac_cur];
                    let d = if c > b'9' { c - b'a' + 10 } else { c - b'0' } as u32;
                    if d + 1 < radix {
                        buf[frac_cur] = CHARS[d as usize + 1];
                        frac_cur += 1;
                        break;
                    }
                }
                break;
            }
            if fraction < delta {
                break;
            }
        }
    }
    // Digits below the precision of a double are zeros.
    while exponent(integer / r) > 0 {
        integer /= r;
        int_cur -= 1;
        buf[int_cur] = b'0';
    }
    loop {
        let rem = integer % r;
        int_cur -= 1;
        buf[int_cur] = CHARS[rem as usize];
        integer = (integer - rem) / r;
        if integer <= 0.0 {
            break;
        }
    }
    if neg {
        int_cur -= 1;
        buf[int_cur] = b'-';
    }
    String::from_utf8_lossy(&buf[int_cur..frac_cur]).into_owned()
}

/// The decimal digits of finite `x != 0` rounded half-up (ECMA-262 picks the larger of two
/// equally near candidates) to `n` significant digits, and the exponent of the first one.
fn sig_digits(x: f64, n: usize) -> (Vec<u8>, i32) {
    // 1100 digits hold the exact binary value, so ties are seen as ties.
    let s = format!("{:.1100e}", x.abs());
    let (m, e) = s.split_once('e').unwrap();
    let mut e: i32 = e.parse().unwrap();
    let all: Vec<u8> = m.bytes().filter(u8::is_ascii_digit).collect();
    let mut d = all[..n].to_vec();
    if all[n] >= b'5' {
        let mut i = n;
        loop {
            if i == 0 {
                d.insert(0, b'1');
                d.pop();
                e += 1;
                break;
            }
            i -= 1;
            if d[i] == b'9' {
                d[i] = b'0';
            } else {
                d[i] += 1;
                break;
            }
        }
    }
    (d, e)
}

fn exp_form(neg: bool, d: &[u8], e: i32) -> String {
    let mut out = String::from(if neg { "-" } else { "" });
    out.push(d[0] as char);
    if d.len() > 1 {
        out.push('.');
        out.push_str(std::str::from_utf8(&d[1..]).unwrap());
    }
    out.push_str(&format!("e{}{}", if e < 0 { '-' } else { '+' }, e.abs()));
    out
}

/// `Number.prototype.toPrecision(p)` for finite `x`, `p` in 1..=100.
pub fn to_precision(x: f64, p: usize) -> String {
    if x == 0.0 {
        return if p == 1 {
            "0".into()
        } else {
            format!("0.{}", "0".repeat(p - 1))
        };
    }
    let neg = x < 0.0;
    let (d, e) = sig_digits(x, p);
    if e < -6 || e >= p as i32 {
        return exp_form(neg, &d, e);
    }
    let digits = std::str::from_utf8(&d).unwrap();
    let body = if e < 0 {
        format!("0.{}{digits}", "0".repeat((-e - 1) as usize))
    } else if e as usize + 1 == p {
        digits.to_string()
    } else {
        let (a, b) = digits.split_at(e as usize + 1);
        format!("{a}.{b}")
    };
    format!("{}{body}", if neg { "-" } else { "" })
}

/// `Number.prototype.toExponential(f)` for finite `x`; `None` uses as many digits as needed.
pub fn to_exponential(x: f64, f: Option<usize>) -> String {
    if x == 0.0 {
        let f = f.unwrap_or(0);
        return if f == 0 {
            "0e+0".into()
        } else {
            format!("0.{}e+0", "0".repeat(f))
        };
    }
    let neg = x < 0.0;
    let (d, e) = match f {
        Some(f) => sig_digits(x, f + 1),
        None => {
            // Shortest round-trip digits.
            let s = format!("{:e}", x.abs());
            let (m, e) = s.split_once('e').unwrap();
            (
                m.bytes().filter(u8::is_ascii_digit).collect(),
                e.parse().unwrap(),
            )
        }
    };
    exp_form(neg, &d, e)
}

/// `RegExpBuiltinExec`: `g`/`y` start at `lastIndex` and update it (0 on failure).
fn regex_exec(it: &mut Interp, this: &Value, re: &JsRegex, s: &Rc<[u16]>) -> R<Option<Hit>> {
    let Value::Obj(o) = this else {
        return Ok(None);
    };
    let flags = regexp_parts(o).map(|p| p.1).unwrap_or_default();
    let (global, sticky) = (flags.contains('g'), flags.contains('y'));
    // RegExpBuiltinExec steps 2-3 read and ToLength `lastIndex` whatever the flags.
    let li = it.get(this, "lastIndex")?;
    let li = it.to_numeric(&li)?;
    if !global && !sticky {
        let hit = re.find_from(s, 0);
        if let Some(h) = &hit {
            note_match(it, s, h);
        }
        return Ok(hit);
    }
    let li = if li.is_nan() {
        0.0
    } else {
        li.trunc().max(0.0)
    };
    let hit = if li > s.len() as f64 {
        None
    } else {
        let pos = li as usize;
        re.find_from(s, pos).filter(|h| !sticky || h.0 == pos)
    };
    let next = hit.as_ref().map_or(0, |h| h.1);
    it.put(this, "lastIndex", Value::Num(next as f64))?;
    if let Some(h) = &hit {
        note_match(it, s, h);
    }
    Ok(hit)
}

/// An `exec` result: match, groups, `index`, `input`.
fn hit_array(s: &Value, s16: &[u16], h: &Hit) -> Value {
    let mut v = vec![from16(&s16[h.0..h.1])];
    v.extend(h.2.iter().map(|g| group(s16, g)));
    let arr = Value::array(v);
    if let Value::Obj(o) = &arr {
        set_own(o, "index", Value::Num(h.0 as f64));
        set_own(o, "input", s.clone());
        set_own(o, "groups", hit_groups(s16, h));
    }
    arr
}

/// `@@split` of a RegExp, without a limit.
/// `RegExp.prototype[@@split]` (ECMA-262 §22.2.6.14): splits with a sticky copy of `this`
/// made by its species constructor.
pub fn regexp_split(it: &mut Interp, this: &Value, a: &[Value]) -> R<Value> {
    if !matches!(this, Value::Obj(_)) {
        return it.throw("TypeError", "Type error");
    }
    let s = it.to_str_value(&arg(a, 0))?;
    let c = crate::symbol::species_constructor(it, this)?;
    let flags = it.get(this, "flags")?;
    let flags = it.to_string(&flags)?;
    let unicode = flags.contains('u');
    let sticky = if flags.contains('y') {
        flags
    } else {
        flags + "y"
    };
    let sticky = Value::str(&sticky);
    let splitter = match c {
        Some(c) if !is_intrinsic_regexp(&c) => it.construct(&c, &[this.clone(), sticky])?,
        _ => regexp_construct(it, this, &sticky, false)?,
    };
    let lim = match arg(a, 1) {
        Value::Undefined => u32::MAX as usize,
        _ => {
            let n = num(it, a, 1)?;
            if n.is_finite() {
                n.trunc().rem_euclid(4294967296.0) as usize
            } else {
                0
            }
        }
    };
    if lim == 0 {
        return Ok(Value::array(Vec::new()));
    }
    // With the built-in `exec`, the sticky loop is the regex's own search.
    let exec = it.get(&splitter, "exec")?;
    if exec.strict_eq(&it.sym.regexp_exec)
        && let Some((re, _)) = compile(it, &splitter)?
    {
        let parts = regex_split(it, &re, &s);
        return Ok(Value::array(parts.into_iter().take(lim).collect()));
    }
    let s16 = units(&s);
    let size = s16.len();
    let mut out = Vec::new();
    if size == 0 {
        if matches!(regexp_exec_call(it, &splitter, &s, &s16)?, Value::Null) {
            out.push(s.clone());
        }
        return Ok(Value::array(out));
    }
    let advance = |q: usize| advance_index(&s16, q, unicode);
    let (mut p, mut q) = (0, 0);
    while q < size {
        it.put_or_throw(&splitter, "lastIndex", Value::Num(q as f64))?;
        let z = regexp_exec_call(it, &splitter, &s, &s16)?;
        if matches!(z, Value::Null) {
            q = advance(q);
            continue;
        }
        let e = it.get(&splitter, "lastIndex")?;
        let e = to_length(it, &e)?.min(size);
        if e == p {
            q = advance(q);
            continue;
        }
        out.push(from16(&s16[p..q]));
        if out.len() == lim {
            return Ok(Value::array(out));
        }
        p = e;
        let n = it.get(&z, "length")?;
        for i in 1..to_length(it, &n)? {
            out.push(it.get(&z, &i.to_string())?);
            if out.len() == lim {
                return Ok(Value::array(out));
            }
        }
        q = p;
    }
    out.push(from16(&s16[p..]));
    Ok(Value::array(out))
}

/// ToLength (ECMA-262 §7.1.20) as an index.
fn to_length(it: &mut Interp, v: &Value) -> R<usize> {
    let n = it.to_numeric(v)?;
    Ok(if n > 0.0 {
        n.trunc().min(9007199254740991.0) as usize
    } else {
        0
    })
}

/// A RegExpExec (ECMA-262 §22.2.7.1) result: the built-in exec's match, unboxed, or the
/// object a user `exec` returned.
enum Exec {
    Hit(Hit),
    Obj(Value),
}

/// RegExpExec (ECMA-262 §22.2.7.1) of `r` on `s`: `r.exec(s)` when that is a function other
/// than %RegExp.prototype.exec%, else RegExpBuiltinExec (`None` for no match).
fn regexp_exec_raw(it: &mut Interp, r: &Value, s: &Value, s16: &Rc<[u16]>) -> R<Option<Exec>> {
    let exec = it.get(r, "exec")?;
    let callable = crate::eval::callable(&exec).is_some();
    if (!callable || exec.strict_eq(&it.sym.regexp_exec))
        && regexp_this(r).is_some()
        && let Some((re, _)) = compile(it, r)?
    {
        return Ok(regex_exec(it, r, &re, s16)?.map(Exec::Hit));
    }
    if !callable {
        return it.throw("TypeError", "Type error");
    }
    match it.call(&exec, r.clone(), std::slice::from_ref(s))? {
        Value::Null => Ok(None),
        z @ Value::Obj(_) => Ok(Some(Exec::Obj(z))),
        _ => it.throw("TypeError", "Type error"),
    }
}

/// RegExpExec as V4's `@@search` and `@@replace` do it: a RegExp object runs the built-in
/// exec whatever its `exec` property is, another receiver goes through [`regexp_exec_raw`].
fn regexp_exec_direct(it: &mut Interp, r: &Value, s: &Value, s16: &Rc<[u16]>) -> R<Option<Exec>> {
    match compile(it, r)? {
        Some((re, _)) => Ok(regex_exec(it, r, &re, s16)?.map(Exec::Hit)),
        None => regexp_exec_raw(it, r, s, s16),
    }
}

/// RegExpExec as a value: the match array, or `null`.
fn regexp_exec_call(it: &mut Interp, r: &Value, s: &Value, s16: &Rc<[u16]>) -> R<Value> {
    Ok(match regexp_exec_raw(it, r, s, s16)? {
        None => Value::Null,
        Some(Exec::Hit(h)) => hit_array(s, s16, &h),
        Some(Exec::Obj(o)) => o,
    })
}

/// The matched substring of a RegExpExec result (`ToString(result[0])`) and whether it is
/// empty.
fn exec_matched(it: &mut Interp, e: &Exec, s16: &[u16]) -> R<(Value, bool)> {
    Ok(match e {
        Exec::Hit(h) => (from16(&s16[h.0..h.1]), h.0 == h.1),
        Exec::Obj(o) => {
            let m = it.get(o, "0")?;
            let m = it.to_string(&m)?;
            let empty = m.is_empty();
            (Value::str(&m), empty)
        }
    })
}

/// AdvanceStringIndex (ECMA-262 §22.2.7.3).
fn advance_index(s: &[u16], i: usize, unicode: bool) -> usize {
    let pair = unicode
        && s.get(i).is_some_and(|u| (0xD800..0xDC00).contains(u))
        && s.get(i + 1).is_some_and(|u| (0xDC00..0xE000).contains(u));
    i + if pair { 2 } else { 1 }
}

/// `rx.lastIndex` past an empty match: AdvanceStringIndex(S, ToLength(lastIndex)).
fn skip_empty_match(it: &mut Interp, rx: &Value, s16: &[u16], unicode: bool) -> R<()> {
    let i = it.get(rx, "lastIndex")?;
    let i = to_length(it, &i)?;
    it.put_or_throw(
        rx,
        "lastIndex",
        Value::Num(advance_index(s16, i, unicode) as f64),
    )
}

/// The receiver check of the RegExp symbol methods, and their string argument.
fn regexp_symbol_args(it: &mut Interp, this: &Value, a: &[Value]) -> R<Value> {
    if !matches!(this, Value::Obj(_)) {
        return it.throw("TypeError", "Type error");
    }
    it.to_str_value(&arg(a, 0))
}

/// `ToString(rx.flags)`.
fn regexp_flags_of(it: &mut Interp, rx: &Value) -> R<String> {
    let f = it.get(rx, "flags")?;
    it.to_string(&f)
}

fn full_unicode(flags: &str) -> bool {
    flags.contains('u') || flags.contains('v')
}

/// Whether `c` is %RegExp%.
fn is_intrinsic_regexp(c: &Value) -> bool {
    matches!(crate::eval::callable(c),
        Some(Callable::Native(_, f)) if std::ptr::fn_addr_eq(f, regexp_ctor as NativeFn))
}

/// IsRegExp (ECMA-262 §7.2.8).
fn is_regexp(it: &mut Interp, v: &Value) -> R<bool> {
    if !matches!(v, Value::Obj(_)) {
        return Ok(false);
    }
    match it.get_sym(v, &crate::value::WellKnown::Match.sym())? {
        Value::Undefined => Ok(regexp_this(v).is_some()),
        m => Ok(m.truthy()),
    }
}

/// GetMethod (ECMA-262 §7.3.11) of `v` for a well-known symbol.
fn get_method(it: &mut Interp, v: &Value, w: crate::value::WellKnown) -> R<Option<Value>> {
    match it.get_sym(v, &w.sym())? {
        Value::Undefined | Value::Null => Ok(None),
        f if crate::eval::callable(&f).is_some() => Ok(Some(f)),
        _ => it.throw("TypeError", "Type error"),
    }
}

/// `RegExp.prototype[@@match]` (ECMA-262 §22.2.6.8).
pub fn regexp_match(it: &mut Interp, this: &Value, a: &[Value]) -> R<Value> {
    let s = regexp_symbol_args(it, this, a)?;
    let s16 = units(&s);
    let flags = regexp_flags_of(it, this)?;
    if !flags.contains('g') {
        return regexp_exec_call(it, this, &s, &s16);
    }
    let unicode = full_unicode(&flags);
    it.put_or_throw(this, "lastIndex", Value::Num(0.0))?;
    let mut out = Vec::new();
    while let Some(e) = regexp_exec_raw(it, this, &s, &s16)? {
        let (m, empty) = exec_matched(it, &e, &s16)?;
        if empty {
            skip_empty_match(it, this, &s16, unicode)?;
        }
        out.push(m);
    }
    Ok(if out.is_empty() {
        Value::Null
    } else {
        Value::array(out)
    })
}

/// `RegExp.prototype[@@search]` (ECMA-262 §22.2.6.12): the index of the first match from 0,
/// `lastIndex` left as it was.
pub fn regexp_search(it: &mut Interp, this: &Value, a: &[Value]) -> R<Value> {
    let s = regexp_symbol_args(it, this, a)?;
    let s16 = units(&s);
    let prev = it.get(this, "lastIndex")?;
    if !same_value(&prev, &Value::Num(0.0)) {
        it.put_or_throw(this, "lastIndex", Value::Num(0.0))?;
    }
    let r = regexp_exec_direct(it, this, &s, &s16)?;
    let cur = it.get(this, "lastIndex")?;
    if !same_value(&cur, &prev) {
        it.put_or_throw(this, "lastIndex", prev)?;
    }
    match r {
        None => Ok(Value::Num(-1.0)),
        Some(Exec::Hit(h)) => Ok(Value::Num(h.0 as f64)),
        Some(Exec::Obj(o)) => it.get(&o, "index"),
    }
}

/// The groups object of a match (`undefined` without named groups), null-prototype.
fn hit_groups(s16: &[u16], h: &Hit) -> Value {
    let Some(named) = &h.3 else {
        return Value::Undefined;
    };
    let o = new_obj();
    o.borrow_mut().null_proto = true;
    for (n, r) in named.iter() {
        set_own(&o, n, group(s16, r));
    }
    Value::Obj(o)
}

/// `RegExp.prototype[@@replace]` (ECMA-262 §22.2.6.11).
pub fn regexp_replace(it: &mut Interp, this: &Value, a: &[Value]) -> R<Value> {
    let s = regexp_symbol_args(it, this, a)?;
    let s16 = units(&s);
    let rep = arg(a, 1);
    let functional = crate::eval::callable(&rep).is_some();
    let template = if functional {
        Rc::new([])
    } else {
        units(&it.to_str_value(&rep)?)
    };
    let flags = regexp_flags_of(it, this)?;
    let global = flags.contains('g');
    let unicode = full_unicode(&flags);
    if global {
        it.put_or_throw(this, "lastIndex", Value::Num(0.0))?;
    }
    let mut results = Vec::new();
    while let Some(e) = regexp_exec_direct(it, this, &s, &s16)? {
        if !global {
            results.push(e);
            break;
        }
        if exec_matched(it, &e, &s16)?.1 {
            skip_empty_match(it, this, &s16, unicode)?;
        }
        results.push(e);
    }
    let mut out = Vec::with_capacity(s16.len());
    let mut next = 0;
    for e in results {
        let (matched, pos, caps, named) = match e {
            Exec::Hit(h) => (
                s16[h.0..h.1].to_vec(),
                h.0,
                h.2.iter().map(|g| group(&s16, g)).collect::<Vec<_>>(),
                hit_groups(&s16, &h),
            ),
            Exec::Obj(o) => {
                let n = it.get(&o, "length")?;
                let n = to_length(it, &n)?;
                let m = it.get(&o, "0")?;
                let matched = units(&it.to_str_value(&m)?).to_vec();
                let p = it.get(&o, "index")?;
                let p = it.to_numeric(&p)?;
                let p = if p.is_nan() { 0.0 } else { p.trunc() };
                let pos = p.clamp(0.0, s16.len() as f64) as usize;
                let mut caps = Vec::new();
                for i in 1..n {
                    let c = it.get(&o, &i.to_string())?;
                    caps.push(match c {
                        Value::Undefined => c,
                        c => it.to_str_value(&c)?,
                    });
                }
                let named = it.get(&o, "groups")?;
                (matched, pos, caps, named)
            }
        };
        let replacement = if functional {
            let mut args = Vec::with_capacity(caps.len() + 4);
            args.push(from16(&matched));
            args.extend(caps);
            args.push(Value::Num(pos as f64));
            args.push(s.clone());
            if !matches!(named, Value::Undefined) {
                args.push(named);
            }
            let r = it.call(&rep, Value::Undefined, &args)?;
            units(&it.to_str_value(&r)?).to_vec()
        } else {
            let named = match named {
                Value::Undefined => named,
                Value::Null => return it.no_object(&named),
                n => it.to_object(&n),
            };
            let mut r = Vec::new();
            get_substitution(it, &matched, &s16, pos, &caps, &named, &template, &mut r)?;
            r
        };
        if pos >= next {
            out.extend_from_slice(&s16[next..pos]);
            out.extend(replacement);
            next = pos + matched.len();
        }
    }
    if next < s16.len() {
        out.extend_from_slice(&s16[next..]);
    }
    Ok(from16(&out))
}

/// `RegExp.prototype[@@matchAll]` (ECMA-262 §22.2.6.9): a RegExp String Iterator over a copy
/// of `this` made by its species constructor.
pub fn regexp_match_all(it: &mut Interp, this: &Value, a: &[Value]) -> R<Value> {
    let s = regexp_symbol_args(it, this, a)?;
    let c = crate::symbol::species_constructor(it, this)?;
    let flags = regexp_flags_of(it, this)?;
    let f = Value::str(&flags);
    let matcher = match c {
        Some(c) if !is_intrinsic_regexp(&c) => it.construct(&c, &[this.clone(), f])?,
        _ => regexp_construct(it, this, &f, false)?,
    };
    let li = it.get(this, "lastIndex")?;
    let li = to_length(it, &li)?;
    it.put_or_throw(&matcher, "lastIndex", Value::Num(li as f64))?;
    let o = new_obj();
    {
        let mut b = o.borrow_mut();
        b.proto = Some(it.sym.regexp_string_iter_proto.clone());
        b.internal = Internal::RegExpStringIter(Box::new(crate::value::RegExpStringIter {
            matcher,
            s16: units(&s),
            s,
            global: flags.contains('g'),
            unicode: full_unicode(&flags),
        }));
    }
    Ok(Value::Obj(o))
}

/// %RegExpStringIteratorPrototype%.next (ECMA-262 §22.2.9.2.1).
pub fn regexp_string_iter_next(it: &mut Interp, this: &Value, _: &[Value]) -> R<Value> {
    let state = match this {
        Value::Obj(o) => match &o.borrow().internal {
            Internal::RegExpStringIter(r) => Some((
                r.matcher.clone(),
                r.s.clone(),
                r.s16.clone(),
                r.global,
                r.unicode,
            )),
            _ => None,
        },
        _ => None,
    };
    let Some((rx, s, s16, global, unicode)) = state else {
        return it.throw("TypeError", "Type error");
    };
    let finish = |this: &Value| {
        if let Value::Obj(o) = this
            && let Internal::RegExpStringIter(r) = &mut o.borrow_mut().internal
        {
            r.matcher = Value::Undefined;
        }
    };
    if matches!(rx, Value::Undefined) {
        return Ok(crate::symbol::iter_result(Value::Undefined, true));
    }
    let Some(e) = regexp_exec_raw(it, &rx, &s, &s16)? else {
        finish(this);
        return Ok(crate::symbol::iter_result(Value::Undefined, true));
    };
    if !global {
        finish(this);
    } else if exec_matched(it, &e, &s16)?.1 {
        skip_empty_match(it, &rx, &s16, unicode)?;
    }
    let v = match e {
        Exec::Hit(h) => hit_array(&s, &s16, &h),
        Exec::Obj(o) => o,
    };
    Ok(crate::symbol::iter_result(v, false))
}

/// `String.prototype.match`, `matchAll`, `replace`, `replaceAll` and `search` (ECMA-262
/// §22.1.3.13-14, 19-20, 22): an object argument's own `@@match`/... method if it has one,
/// else the string search (`replace`) or a new RegExp's method.
fn string_regexp_method(it: &mut Interp, s: &Value, name: &str, a: &[Value]) -> R<Value> {
    use crate::value::WellKnown;
    let (w, flags) = match name {
        "match" => (WellKnown::Match, ""),
        "matchAll" => (WellKnown::MatchAll, "g"),
        "search" => (WellKnown::Search, ""),
        _ => (WellKnown::Replace, ""),
    };
    let a0 = arg(a, 0);
    let this = s.clone();
    if let Value::Obj(_) = &a0 {
        if matches!(name, "matchAll" | "replaceAll") && is_regexp(it, &a0)? {
            let f = it.get(&a0, "flags")?;
            if matches!(f, Value::Undefined | Value::Null) {
                return it.throw("TypeError", "Type error");
            }
            if !it.to_string(&f)?.contains('g') {
                return it.throw("TypeError", "Type error");
            }
        }
        if let Some(m) = get_method(it, &a0, w)? {
            let args = [this, arg(a, 1)];
            let n = if w == WellKnown::Replace { 2 } else { 1 };
            return it.call(&m, a0.clone(), &args[..n]);
        }
    }
    if w == WellKnown::Replace {
        return string_replace(it, s, &a0, &arg(a, 1), name == "replaceAll");
    }
    // RegExpCreate(regexp, flags), then Invoke(rx, @@match, « S »).
    let src = match &a0 {
        Value::Undefined => Value::str(""),
        p => Value::str(&it.to_string(p)?),
    };
    let rx = regexp_new(it, &src, &Value::str(flags), false)?;
    let m = it.get_sym(&rx, &w.sym())?;
    it.call(&m, rx, &[this])
}

/// StringIndexOf (ECMA-262 §6.1.4.1) on UTF-16.
fn index_of(s: &[u16], p: &[u16], from: usize) -> Option<usize> {
    if from > s.len() {
        return None;
    }
    if p.is_empty() {
        return Some(from);
    }
    s[from..]
        .windows(p.len())
        .position(|w| w == p)
        .map(|i| i + from)
}

/// `String.prototype.replace` / `replaceAll` with a string pattern (ECMA-262 §22.1.3.19-20).
fn string_replace(it: &mut Interp, s: &Value, pat: &Value, rep: &Value, all: bool) -> R<Value> {
    let s16 = units(s);
    let p16 = units(&it.to_str_value(pat)?);
    let functional = crate::eval::callable(rep).is_some();
    let template = if functional {
        Rc::new([])
    } else {
        units(&it.to_str_value(rep)?)
    };
    let mut positions = Vec::new();
    let mut from = 0;
    while let Some(p) = index_of(&s16, &p16, from) {
        positions.push(p);
        if !all {
            break;
        }
        from = p + p16.len().max(1);
    }
    if positions.is_empty() {
        return Ok(s.clone());
    }
    let mut out = Vec::with_capacity(s16.len());
    let mut end = 0;
    for p in positions {
        out.extend_from_slice(&s16[end..p]);
        if functional {
            let args = [from16(&p16), Value::Num(p as f64), s.clone()];
            let r = it.call(rep, Value::Undefined, &args)?;
            out.extend_from_slice(&units(&it.to_str_value(&r)?));
        } else {
            get_substitution(
                it,
                &p16,
                &s16,
                p,
                &[],
                &Value::Undefined,
                &template,
                &mut out,
            )?;
        }
        end = p + p16.len();
    }
    out.extend_from_slice(&s16[end..]);
    Ok(from16(&out))
}

/// GetSubstitution (ECMA-262 §22.1.3.19.1): `rep` with its `$` patterns expanded, appended
/// to `out`; `caps` are the captures (strings or undefined), `named` the groups object or
/// undefined.
#[allow(clippy::too_many_arguments)]
fn get_substitution(
    it: &mut Interp,
    matched: &[u16],
    s: &[u16],
    pos: usize,
    caps: &[Value],
    named: &Value,
    rep: &[u16],
    out: &mut Vec<u16>,
) -> R<()> {
    let b = rep;
    let pos = pos.min(s.len());
    let digit = |c: u16| {
        (c >= u16::from(b'0') && c <= u16::from(b'9')).then(|| usize::from(c - u16::from(b'0')))
    };
    let mut i = 0;
    while i < b.len() {
        if b[i] == u16::from(b'$') && i + 1 < b.len() {
            match u8::try_from(b[i + 1]).unwrap_or(0) {
                b'$' => {
                    out.push(u16::from(b'$'));
                    i += 2;
                    continue;
                }
                b'&' => {
                    out.extend_from_slice(matched);
                    i += 2;
                    continue;
                }
                b'`' => {
                    out.extend_from_slice(&s[..pos]);
                    i += 2;
                    continue;
                }
                b'\'' => {
                    out.extend_from_slice(&s[(pos + matched.len()).min(s.len())..]);
                    i += 2;
                    continue;
                }
                d if d.is_ascii_digit() => {
                    let m = caps.len();
                    let mut n = (d - b'0') as usize;
                    let mut j = i + 2;
                    if let Some(e) = b.get(j).copied().and_then(digit)
                        && n * 10 + e <= m
                    {
                        n = n * 10 + e;
                        j += 1;
                    }
                    if (1..=m).contains(&n) {
                        if !matches!(caps[n - 1], Value::Undefined) {
                            out.extend_from_slice(&units(&it.to_str_value(&caps[n - 1])?));
                        }
                        i = j;
                        continue;
                    }
                }
                b'<' if !matches!(named, Value::Undefined) => {
                    if let Some(end) = b[i + 2..].iter().position(|&c| c == u16::from(b'>')) {
                        let name = String::from_utf16_lossy(&b[i + 2..i + 2 + end]);
                        let v = it.get(named, &name)?;
                        if !matches!(v, Value::Undefined) {
                            out.extend_from_slice(&units(&it.to_str_value(&v)?));
                        }
                        i += 3 + end;
                        continue;
                    }
                }
                _ => {}
            }
        }
        out.push(b[i]);
        i += 1;
    }
    Ok(())
}

fn regex_split(it: &mut Interp, re: &JsRegex, s: &Value) -> Vec<Value> {
    let s16 = units(s);
    let size = s16.len();
    if size == 0 {
        return match re.find_from(&s16, 0) {
            Some(h) => {
                note_match(it, &s16, &h);
                Vec::new()
            }
            None => vec![s.clone()],
        };
    }
    let mut out = Vec::new();
    let (mut p, mut q) = (0, 0);
    while q < size {
        let Some(h) = re.find_from(&s16, q).filter(|h| h.0 < size) else {
            break;
        };
        note_match(it, &s16, &h);
        let e = h.1.min(size);
        if e == p {
            q = re.advance(&s16, h.0);
            continue;
        }
        out.push(from16(&s16[p..h.0]));
        out.extend(h.2.iter().map(|g| group(&s16, g)));
        p = e;
        q = p;
    }
    out.push(from16(&s16[p..]));
    out
}

fn string_method(it: &mut Interp, this: &Value, name: &str, a: &[Value]) -> R<Option<Value>> {
    let Some(js) = this.js_str() else {
        return Ok(None);
    };
    let a0 = arg(a, 0);
    if matches!(name, "startsWith" | "endsWith" | "includes") && is_regexp(it, &a0)? {
        return it.throw("TypeError", "Type error");
    }
    if matches!(
        name,
        "match" | "matchAll" | "replace" | "replaceAll" | "search"
    ) {
        return string_regexp_method(it, this, name, a).map(Some);
    }
    let st = |x: &str| Some(Value::str(x));
    if let JsStr::Utf16(u) = js {
        let r = match name {
            "trim" => Some(trim16(u, true, true)),
            "trimStart" => Some(trim16(u, true, false)),
            "trimEnd" => Some(trim16(u, false, true)),
            _ => None,
        };
        if r.is_some() {
            return Ok(r);
        }
    }
    // The receiver as text (an unpaired surrogate read as U+FFFD), for the methods that take
    // it so: converting a UTF-16 string is left to them.
    let lossy = std::cell::OnceCell::new();
    let text = || -> &str { lossy.get_or_init(|| js.lossy()) };
    Ok(match name {
        "trim" => st(crate::value::js_trim(text())),
        "trimStart" => st(text().trim_start_matches(crate::value::is_js_space)),
        "trimEnd" => st(text().trim_end_matches(crate::value::is_js_space)),
        "toLowerCase" => Some(qstring_case(js, false)),
        "toUpperCase" => Some(qstring_case(js, true)),
        "toLocaleLowerCase" | "toLocaleUpperCase" => {
            let locale = case_locale(it, a)?;
            Some(locale_case(locale, js, name == "toLocaleUpperCase"))
        }
        "normalize" => {
            use icu_normalizer::{ComposingNormalizerBorrowed, DecomposingNormalizerBorrowed};
            let form = match a0 {
                Value::Undefined => "NFC".to_string(),
                v => it.to_string(&v)?,
            };
            let f: fn(&str) -> String = match form.as_str() {
                "NFC" => |s| {
                    ComposingNormalizerBorrowed::new_nfc()
                        .normalize(s)
                        .into_owned()
                },
                "NFD" => |s| {
                    DecomposingNormalizerBorrowed::new_nfd()
                        .normalize(s)
                        .into_owned()
                },
                "NFKC" => |s| {
                    ComposingNormalizerBorrowed::new_nfkc()
                        .normalize(s)
                        .into_owned()
                },
                "NFKD" => |s| {
                    DecomposingNormalizerBorrowed::new_nfkd()
                        .normalize(s)
                        .into_owned()
                },
                _ => {
                    return it.throw(
                        "RangeError",
                        "String.prototype.normalize: Invalid normalization form.",
                    );
                }
            };
            match js {
                JsStr::Utf8(t) => st(&f(t)),
                JsStr::Utf16(u) => Some(map_runs(u, f)),
            }
        }
        "toString" | "valueOf" => Some(this.clone()),
        "startsWith" => {
            let u = Units::of(this);
            let p = str_pos(it, &arg(a, 1), u.len(), 0)?;
            Some(Value::Bool(u.starts_at(needle(&it.to_str_value(&a0)?), p)))
        }
        "endsWith" => {
            let u = Units::of(this);
            let e = str_pos(it, &arg(a, 1), u.len(), u.len())?;
            Some(Value::Bool(u.ends_at(needle(&it.to_str_value(&a0)?), e)))
        }
        "includes" | "indexOf" => {
            let u = Units::of(this);
            let r = u.find(
                needle(&it.to_str_value(&a0)?),
                str_pos(it, &arg(a, 1), u.len(), 0)?,
            );
            Some(if name == "includes" {
                Value::Bool(r.is_some())
            } else {
                Value::Num(r.map_or(-1.0, |i| i as f64))
            })
        }
        "lastIndexOf" => {
            let u = Units::of(this);
            let n = num(it, a, 1)?;
            let start = if n.is_nan() {
                u.len()
            } else {
                n.clamp(0.0, u.len() as f64) as usize
            };
            Some(Value::Num(
                u.rfind(needle(&it.to_str_value(&a0)?), start)
                    .map_or(-1.0, |i| i as f64),
            ))
        }
        "slice" => {
            let u = Units::of(this);
            let (b, e) = (
                rel(it, &a0, u.len(), 0)?,
                rel(it, &arg(a, 1), u.len(), u.len())?,
            );
            Some(if b < e { u.slice(b, e) } else { Value::str("") })
        }
        "substring" => {
            let u = Units::of(this);
            let (x, y) = (
                str_pos(it, &a0, u.len(), 0)?,
                str_pos(it, &arg(a, 1), u.len(), u.len())?,
            );
            Some(u.slice(x.min(y), x.max(y)))
        }
        "substr" => {
            let u = Units::of(this);
            let b = rel(it, &a0, u.len(), 0)?;
            let rest = u.len() - b;
            let n = if matches!(arg(a, 1), Value::Undefined) {
                rest
            } else {
                num(it, a, 1)?.max(0.0).min(rest as f64) as usize
            };
            Some(u.slice(b, b + n))
        }
        "charAt" | "at" => {
            let u = Units::of(this);
            let i = num(it, a, 0)?;
            let i = if i.is_nan() { 0.0 } else { i.trunc() };
            let i = if name == "at" && i < 0.0 {
                u.len() as f64 + i
            } else {
                i
            };
            if i >= 0.0 && (i as usize) < u.len() {
                Some(u.slice(i as usize, i as usize + 1))
            } else if name == "at" {
                Some(Value::Undefined)
            } else {
                st("")
            }
        }
        "codePointAt" => {
            let u = Units::of(this);
            let i = if matches!(a0, Value::Undefined) {
                0.0
            } else {
                it.to_numeric(&a0)?
            };
            let i = if i.is_nan() { 0.0 } else { i.trunc() };
            if i < 0.0 || i >= u.len() as f64 {
                Some(Value::Undefined)
            } else {
                let i = i as usize;
                let hi = u.unit(i);
                let lo = (i + 1 < u.len()).then(|| u.unit(i + 1));
                Some(Value::Num(match lo {
                    Some(lo)
                        if (0xD800..0xDC00).contains(&hi) && (0xDC00..0xE000).contains(&lo) =>
                    {
                        (0x10000 + ((u32::from(hi) - 0xD800) << 10) + (u32::from(lo) - 0xDC00))
                            as f64
                    }
                    _ => f64::from(hi),
                }))
            }
        }
        "charCodeAt" => {
            let u = Units::of(this);
            let i = if matches!(a0, Value::Undefined) {
                0.0
            } else {
                it.to_numeric(&a0)?
            };
            let i = if i.is_nan() { 0.0 } else { i.trunc() };
            Some(Value::Num(if i >= 0.0 && (i as usize) < u.len() {
                u.unit(i as usize) as f64
            } else {
                f64::NAN
            }))
        }
        "padStart" | "padEnd" => {
            let len = js.len();
            let target = num(it, a, 0)?;
            let fill = match arg(a, 1) {
                Value::Undefined => Value::str(" "),
                v => it.to_str_value(&v)?,
            };
            let fill = needle(&fill);
            if target.is_nan() || target <= len as f64 || fill.is_empty() {
                return Ok(Some(this.clone()));
            }
            if target > crate::eval::MAX_STRING_LEN as f64 {
                return it.throw("RangeError", "Invalid string length");
            }
            let n = target as usize - len;
            let pad = match fill {
                JsStr::Utf8(f) if f.is_ascii() => {
                    Value::str(&f.chars().cycle().take(n).collect::<String>())
                }
                f => from16(
                    &f.units()
                        .iter()
                        .copied()
                        .cycle()
                        .take(n)
                        .collect::<Vec<_>>(),
                ),
            };
            let mut out = crate::value::JsBuf::new();
            if name == "padStart" {
                out.push_value(&pad);
                out.push(js);
            } else {
                out.push(js);
                out.push_value(&pad);
            }
            Some(out.finish())
        }
        "repeat" => {
            let n = num(it, a, 0)?;
            let count = if n.is_nan() { 0.0 } else { n.trunc() };
            if count < 0.0 || count.is_infinite() {
                return it.throw("RangeError", "Invalid count value");
            }
            let len = js.len();
            if len == 0 || count == 0.0 {
                return Ok(Some(Value::str("")));
            }
            if count * len as f64 > crate::eval::MAX_STRING_LEN as f64 {
                return it.throw("RangeError", "Invalid string length");
            }
            match js {
                JsStr::Utf8(s) => st(&s.repeat(count as usize)),
                JsStr::Utf16(u) => Some(from16(&u.repeat(count as usize))),
            }
        }
        "concat" => {
            let mut out = crate::value::JsBuf::new();
            out.push(js);
            for v in a {
                out.push_value(&it.to_str_named(v)?);
            }
            Some(out.finish())
        }
        "localeCompare" => {
            let o = it.to_str_value(&a0)?;
            Some(Value::Num(f64::from(locale_cmp(js, needle(&o)))))
        }
        "arg" if a.is_empty() => return it.throw("Error", "String.arg(): Invalid arguments"),
        "arg" => {
            // An argument that is neither a number nor a boolean goes in as its ToString
            // (QV4's `toQString`): its own `toString` runs, and what that throws is thrown.
            let mut args = Vec::with_capacity(a.len());
            for v in a {
                args.push(match v {
                    Value::Num(_) | Value::Bool(_) => v.clone(),
                    other => it.to_str_value(other)?,
                });
            }
            st(&qarg(text(), &args))
        }
        "split" => {
            // A separator with a `@@split` (a RegExp) splits (ECMA-262 §22.1.3.23).
            if let Value::Obj(_) = &a0 {
                match it.get_sym(&a0, &crate::value::WellKnown::Split.sym())? {
                    Value::Undefined | Value::Null => {}
                    f => {
                        let args = [this.clone(), arg(a, 1)];
                        return Ok(Some(it.call(&f, a0.clone(), &args)?));
                    }
                }
            }
            // ToUint32(limit): -1 wraps to 2^32 - 1 (no limit), NaN and ±Infinity give 0.
            let limit = if matches!(arg(a, 1), Value::Undefined) {
                usize::MAX
            } else {
                let n = num(it, a, 1)?;
                if n.is_finite() {
                    n.trunc().rem_euclid(4294967296.0) as usize
                } else {
                    0
                }
            };
            let parts: Vec<Value> = if matches!(a0, Value::Undefined) {
                vec![this.clone()]
            } else {
                let sep = it.to_str_value(&a0)?;
                match (js, needle(&sep)) {
                    (JsStr::Utf8(s), JsStr::Utf8("")) if s.is_ascii() => {
                        (0..s.len()).map(|i| Value::str(&s[i..i + 1])).collect()
                    }
                    (JsStr::Utf8(s), JsStr::Utf8(sep)) if !sep.is_empty() => {
                        s.split(sep).map(Value::str).collect()
                    }
                    (js, sep) => split16(&js.units(), &sep.units()),
                }
            };
            Some(Value::array(parts.into_iter().take(limit).collect()))
        }
        _ => None,
    })
}

fn cb(it: &mut Interp, f: &Value, this: &Value, v: &Value, i: usize, arr: &Obj) -> R<Value> {
    it.call(
        f,
        this.clone(),
        &[v.clone(), Value::Num(i as f64), Value::Obj(arr.clone())],
    )
}

/// The object `Array.from` / `Array.of` fill: `new C(...args)` for a constructor `this`
/// other than %Array% (ECMA-262 §23.1.2.1, §23.1.2.3), else `None` for a plain Array.
fn array_from_target(it: &mut Interp, this: &Value, args: &[Value]) -> R<Option<Value>> {
    let intrinsic = matches!(crate::eval::callable(this),
        Some(Callable::Native(_, f)) if std::ptr::fn_addr_eq(f, array_ctor as NativeFn));
    if intrinsic || !crate::proxy::is_constructor(this) {
        return Ok(None);
    }
    it.construct(this, args).map(Some)
}

/// Object(value) (ECMA-262 §20.1.1.1): a new object for null and undefined, else ToObject.
fn object_ctor(it: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
    Ok(match arg(a, 0) {
        Value::Undefined | Value::Null => Value::Obj(new_obj()),
        v => it.to_object(&v),
    })
}

/// Whether `f` is %Object%'s function, whose [[Construct]] for another NewTarget ignores
/// its argument (ECMA-262 §20.1.1.1 step 1).
pub fn is_object_ctor(f: NativeFn) -> bool {
    std::ptr::fn_addr_eq(f, object_ctor as NativeFn)
}

/// The IsCallable(callbackfn) step the callback-taking Array.prototype methods run before
/// they visit an element (ECMA-262 §23.1.3.15 step 3 and kin).
fn callback(it: &Interp, f: &Value) -> R<()> {
    if crate::eval::callable(f).is_none() {
        return it.type_error();
    }
    Ok(())
}

/// `Math.hypot` (ECMA-262 §21.3.2.18): the square root of the sum of the squares of the
/// mathematical values. The values are scaled by the largest, so the squares of a huge or
/// tiny value neither overflow nor vanish; an infinity wins over a NaN; no values give +0.
fn hypot(xs: &[f64]) -> f64 {
    if xs.iter().any(|x| x.is_infinite()) {
        return f64::INFINITY;
    }
    if xs.iter().any(|x| x.is_nan()) {
        return f64::NAN;
    }
    let max = xs.iter().fold(0.0_f64, |m, x| m.max(x.abs()));
    if max == 0.0 {
        return 0.0;
    }
    // Neumaier's compensated sum of the scaled squares.
    let (mut sum, mut lost) = (0.0_f64, 0.0_f64);
    for x in xs {
        let r = x / max;
        let sq = r * r;
        let t = sum + sq;
        lost += if sum >= sq {
            (sum - t) + sq
        } else {
            (sq - t) + sum
        };
        sum = t;
    }
    (sum + lost).sqrt() * max
}

/// `Array(...)` and `new Array(...)` (ECMA-262 §23.1.1.1).
fn array_ctor(it: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
    Ok(match a {
        [Value::Num(n)] => {
            let n = crate::eval::array_length(it, *n)?;
            if n <= crate::value::SPARSE_GAP {
                Value::array_made(vec![Value::Undefined; n], 0..n)
            } else {
                // The holes of a longer one take no storage.
                let a = Value::array(Vec::new());
                if let Value::Obj(o) = &a {
                    let b = &mut *o.borrow_mut();
                    b.cells = true;
                    b.set_array_length(n);
                }
                a
            }
        }
        _ => Value::array_made(a.to_vec(), []),
    })
}

/// The array methods whose result [`array_species_create`] makes.
const ARRAY_SPECIES: &[&str] = &[
    "concat", "filter", "flat", "flatMap", "map", "slice", "splice",
];

/// The object an Array method fills: V4's `map`, `filter`, `slice`, `splice` and `concat`
/// always make a plain Array and read neither `constructor` nor `@@species`.
fn array_species_create(_: &mut Interp, _: &Value, _: usize) -> R<Option<Value>> {
    Ok(None)
}

/// `slice` and `concat` give their result no storage in QV4 when it has no elements.
fn bare(v: Value) -> R<Value> {
    if let Value::Obj(o) = &v {
        o.borrow_mut().cells = false;
    }
    Ok(v)
}

/// The result of a species-aware array method: `items` (but its `holes`) defined on the
/// [`array_species_create`] object, which with `set_len` then gets its `length`.
fn species_result(
    it: &mut Interp,
    target: Option<Value>,
    items: Vec<Value>,
    holes: Vec<usize>,
    set_len: bool,
) -> R<Value> {
    let Some(t) = target else {
        return Ok(Value::array_made(items, holes));
    };
    let n = items.len();
    for (i, v) in items.into_iter().enumerate() {
        if !holes.contains(&i) {
            crate::eval::define_field(it, &t, &i.to_string(), v, false)?;
        }
    }
    if set_len {
        it.put_or_throw(&t, "length", Value::Num(n as f64))?;
    }
    Ok(t)
}

/// [`method`] on an array `o`; `species` is the receiver whose `constructor` the
/// species-aware methods create their result with.
fn array_method(
    it: &mut Interp,
    o: &Obj,
    name: &str,
    a: &[Value],
    species: Option<&Value>,
) -> R<Option<Value>> {
    let species_create = |it: &mut Interp, len: usize| match species {
        Some(s) => array_species_create(it, s, len),
        None => Ok(None),
    };
    let snap = || o.borrow().array.clone().unwrap();
    // Arguments convert (possibly running user code) before the array is borrowed.
    let alen = || o.borrow().array.as_ref().unwrap().len();
    // Element `i` as the callback methods read it at each step (ECMA-262 §23.1.3: HasProperty,
    // then Get): `None` for a hole or an index past the current length, so a callback that
    // writes, pops or shrinks the array changes what later steps see.
    let live = |i: usize| {
        let b = o.borrow();
        let arr = b.array.as_ref()?;
        (i < arr.len() && !b.is_hole(i)).then(|| arr[i].clone())
    };
    let f = arg(a, 0);
    Ok(match name {
        "push" => {
            let mut b = o.borrow_mut();
            let arr = b.array.as_mut().unwrap();
            arr.extend(a.iter().cloned());
            Some(Value::Num(arr.len() as f64))
        }
        "pop" => {
            let mut b = o.borrow_mut();
            let v = b.array.as_mut().unwrap().pop().unwrap_or_default();
            b.cells = true;
            b.remap_holes(Some);
            Some(v)
        }
        "shift" => {
            let mut b = o.borrow_mut();
            let arr = b.array.as_mut().unwrap();
            let v = if arr.is_empty() {
                Value::Undefined
            } else {
                arr.remove(0)
            };
            b.cells = true;
            b.remap_holes(|i| i.checked_sub(1));
            b.shift_slots(-1);
            Some(v)
        }
        "unshift" => {
            let mut b = o.borrow_mut();
            let arr = b.array.as_mut().unwrap();
            for (i, v) in a.iter().enumerate() {
                arr.insert(i, v.clone());
            }
            let n = arr.len();
            b.remap_holes(|i| Some(i + a.len()));
            b.shift_slots(a.len() as isize);
            Some(Value::Num(n as f64))
        }
        "splice" => {
            let len = alen();
            let start = rel(it, &f, len, 0)?;
            let n = match a.len() {
                0 => 0,
                1 => len - start,
                _ => (num(it, a, 1)?.max(0.0) as usize).min(len - start),
            };
            let target = species_create(it, n)?;
            let mut b = o.borrow_mut();
            let cur = b.array.as_ref().unwrap().len();
            if cur < len {
                // Shrunk by a conversion: the indices up to the old length are holes.
                b.array.as_mut().unwrap().resize(len, Value::Undefined);
                b.add_holes(cur..len);
            }
            let arr = b.array.as_mut().unwrap();
            let removed: Vec<Value> = arr
                .splice(start..start + n, a.iter().skip(2).cloned())
                .collect();
            let added = a.len().saturating_sub(2);
            let gone: Vec<usize> = (b.holes.iter().flat_map(|h| h.iter()))
                .filter(|i| (start..start + n).contains(i))
                .map(|i| i - start)
                .collect();
            b.remap_holes(|i| match i {
                _ if i < start => Some(i),
                _ if i < start + n => None,
                _ => Some(i - n + added),
            });
            b.cells = true;
            b.shift_slots(added as isize - n as isize);
            drop(b);
            Some(species_result(it, target, removed, gone, true)?)
        }
        "reverse" => {
            let mut b = o.borrow_mut();
            let arr = b.array.as_mut().unwrap();
            arr.reverse();
            let len = arr.len();
            b.remap_holes(|i| Some(len - 1 - i));
            Some(Value::Obj(o.clone()))
        }
        "fill" => {
            let len = alen();
            let (s, e) = (rel(it, &arg(a, 1), len, 0)?, rel(it, &arg(a, 2), len, len)?);
            let mut b = o.borrow_mut();
            let cur = b.array.as_ref().unwrap().len();
            if cur < len {
                // Shrunk by a conversion: the indices up to the old length are holes.
                b.array.as_mut().unwrap().resize(len, Value::Undefined);
                b.add_holes(cur..len);
            }
            let arr = b.array.as_mut().unwrap();
            for x in arr.iter_mut().take(e).skip(s) {
                *x = f.clone();
            }
            b.remap_holes(|i| (!(s..e).contains(&i)).then_some(i));
            Some(Value::Obj(o.clone()))
        }
        "sort" => {
            if !matches!(f, Value::Undefined) && crate::eval::callable(&f).is_none() {
                return it.throw(
                    "TypeError",
                    "The provided comparison function is not callable.",
                );
            }
            let mut v = snap();
            let mut err = None;
            // ArrayElementLessThan: undefined sorts last, a throwing comparator or ToString
            // yields false and the first error is rethrown after the sort.
            crate::v4sort::sort(&mut v, |x, y| match (x, y) {
                (Value::Undefined, _) => false,
                (_, Value::Undefined) => true,
                _ if err.is_some() => false,
                _ if matches!(f, Value::Undefined) => {
                    match it
                        .to_str_value(x)
                        .and_then(|x| Ok((x, it.to_str_value(y)?)))
                    {
                        Ok((x, y)) => x
                            .js_str()
                            .zip(y.js_str())
                            .is_some_and(|(x, y)| x.cmp_units(y).is_lt()),
                        Err(e) => {
                            err = Some(e);
                            false
                        }
                    }
                }
                _ => match it
                    .call(&f, Value::Undefined, &[x.clone(), y.clone()])
                    .and_then(|r| it.to_numeric(&r))
                {
                    Ok(r) => r < 0.0,
                    Err(e) => {
                        err = Some(e);
                        false
                    }
                },
            });
            if let Some(e) = err {
                return Err(e);
            }
            // Holes (`undefined` slots, sorted last) end up after every undefined (§23.1.3.30).
            let mut b = o.borrow_mut();
            let n = b
                .holes
                .as_ref()
                .map_or(0, |h| h.iter().filter(|&i| i < v.len()).count());
            let len = v.len();
            b.array = Some(v);
            if let Some(h) = &mut b.holes {
                h.clear();
            }
            b.add_holes(len - n..len);
            Some(Value::Obj(o.clone()))
        }
        "slice" => {
            let len = alen();
            let (s, e) = (rel(it, &f, len, 0)?, rel(it, &arg(a, 1), len, len)?);
            let target = species_create(it, e.saturating_sub(s))?;
            let (items, h) = {
                let b = o.borrow();
                let v = b.array.as_ref().unwrap();
                let e = e.min(v.len());
                let h: Vec<usize> = (b.holes.iter().flat_map(|h| h.iter()))
                    .filter(|i| (s..e).contains(i))
                    .map(|i| i - s)
                    .collect();
                (if s < e { v[s..e].to_vec() } else { Vec::new() }, h)
            };
            Some(bare(species_result(it, target, items, h, true)?)?)
        }
        "concat" => Some(array_concat(
            it,
            species.unwrap_or(&Value::Obj(o.clone())),
            a,
        )?),
        "join" => {
            let sep = if matches!(f, Value::Undefined) {
                Value::str(",")
            } else {
                it.to_str_value(&f)?
            };
            Some(it.join(o, &sep)?)
        }
        "toString" => Some(array_to_string(it, &Value::Obj(o.clone()))?),
        "includes" => {
            let from = rel(it, &arg(a, 1), alen(), 0)?;
            let nan = matches!(f, Value::Num(n) if n.is_nan());
            Some(Value::Bool(
                o.borrow()
                    .array
                    .as_ref()
                    .unwrap()
                    .iter()
                    .skip(from)
                    .any(|x| x.strict_eq(&f) || (nan && matches!(x, Value::Num(n) if n.is_nan()))),
            ))
        }
        "indexOf" => {
            let from = rel(it, &arg(a, 1), alen(), 0)?;
            let b = o.borrow();
            let v = b.array.as_ref().unwrap();
            Some(Value::Num(
                (from..v.len())
                    .find(|&i| !b.is_hole(i) && v[i].strict_eq(&f))
                    .map_or(-1.0, |i| i as f64),
            ))
        }
        "lastIndexOf" => {
            let len = alen() as f64;
            let n = if a.len() > 1 {
                num(it, a, 1)?
            } else {
                len - 1.0
            };
            let n = if n.is_nan() { 0.0 } else { n.trunc() };
            let n = if n < 0.0 { len + n } else { n.min(len - 1.0) };
            let b = o.borrow();
            let v = b.array.as_ref().unwrap();
            let n = n.min(v.len() as f64 - 1.0);
            Some(Value::Num(if n < 0.0 {
                -1.0
            } else {
                (0..=n as usize)
                    .rev()
                    .find(|&i| !b.is_hole(i) && v[i].strict_eq(&f))
                    .map_or(-1.0, |i| i as f64)
            }))
        }
        "at" => {
            let i = num(it, a, 0)?;
            let b = o.borrow();
            let v = b.array.as_ref().unwrap();
            let i = if i.is_nan() { 0.0 } else { i.trunc() };
            let i = if i < 0.0 { v.len() as f64 + i } else { i };
            Some(if i >= 0.0 {
                v.get(i as usize).cloned().unwrap_or_default()
            } else {
                Value::Undefined
            })
        }
        "flat" => {
            // FlattenIntoArray (ECMA-262 §23.1.3.13.1): `depth` levels of nested arrays,
            // default 1, `Infinity` for all.
            let depth = match &f {
                Value::Undefined => 1.0,
                d => {
                    let d = it.to_numeric(d)?;
                    if d.is_nan() { 0.0 } else { d.trunc().max(0.0) }
                }
            };
            let target = species_create(it, 0)?;
            fn flatten(src: &Obj, depth: f64, out: &mut Vec<Value>) {
                let xs: Vec<Value> = {
                    let b = src.borrow();
                    let arr = b.array.as_ref().unwrap();
                    (0..arr.len())
                        .filter(|i| !b.is_hole(*i))
                        .map(|i| arr[i].clone())
                        .collect()
                };
                for x in xs {
                    match &x {
                        Value::Obj(xo) if depth >= 1.0 && xo.borrow().array.is_some() => {
                            flatten(xo, depth - 1.0, out)
                        }
                        _ => out.push(x),
                    }
                }
            }
            let mut out = Vec::new();
            flatten(o, depth, &mut out);
            Some(species_result(it, target, out, Vec::new(), false)?)
        }
        "map" | "filter" | "forEach" | "find" | "findIndex" | "some" | "every" | "flatMap" => {
            let len = alen();
            // `find`/`findIndex` visit holes as `undefined`; the rest skip them (`HasProperty`),
            // and `map` keeps them in its result.
            let skips = !matches!(name, "find" | "findIndex");
            let this_arg = arg(a, 1);
            callback(it, &f)?;
            let target = match name {
                "map" | "filter" | "flatMap" => {
                    species_create(it, if name == "map" { len } else { 0 })?
                }
                _ => None,
            };
            let mut out = Vec::new();
            let mut out_holes = Vec::new();
            for i in 0..len {
                let x = match live(i) {
                    Some(x) => x,
                    None if !skips => Value::Undefined,
                    None => {
                        if name == "map" {
                            out_holes.push(out.len());
                            out.push(Value::Undefined);
                        }
                        continue;
                    }
                };
                let x = &x;
                let r = cb(it, &f, &this_arg, x, i, o)?;
                match name {
                    "map" => out.push(r),
                    "flatMap" => match &r {
                        Value::Obj(ro) if ro.borrow().array.is_some() => {
                            let rb = ro.borrow();
                            let rs = rb.array.as_ref().unwrap().iter().enumerate();
                            out.extend(rs.filter(|(j, _)| !rb.is_hole(*j)).map(|(_, y)| y.clone()));
                        }
                        _ => out.push(r),
                    },
                    "filter" if r.truthy() => out.push(x.clone()),
                    "find" if r.truthy() => return Ok(Some(x.clone())),
                    "findIndex" if r.truthy() => return Ok(Some(Value::Num(i as f64))),
                    "some" if r.truthy() => return Ok(Some(Value::Bool(true))),
                    "every" if !r.truthy() => return Ok(Some(Value::Bool(false))),
                    _ => {}
                }
            }
            match name {
                "map" => Some(species_result(it, target, out, out_holes, false)?),
                "filter" | "flatMap" => Some(species_result(it, target, out, Vec::new(), false)?),
                "find" | "forEach" => Some(Value::Undefined),
                "findIndex" => Some(Value::Num(-1.0)),
                "some" => Some(Value::Bool(false)),
                _ => Some(Value::Bool(true)),
            }
        }
        "reduce" | "reduceRight" => {
            if crate::eval::callable(&f).is_none() {
                return it.type_error();
            }
            let len = alen();
            callback(it, &f)?;
            let mut order: Box<dyn Iterator<Item = usize>> = if name == "reduceRight" {
                Box::new((0..len).rev())
            } else {
                Box::new(0..len)
            };
            let mut acc = if a.len() > 1 {
                arg(a, 1)
            } else {
                match order.by_ref().find_map(&live) {
                    Some(x) => x,
                    None => {
                        return it.throw("TypeError", "Type error");
                    }
                }
            };
            for i in order {
                let Some(x) = live(i) else { continue };
                acc = it.call(
                    &f,
                    Value::Undefined,
                    &[acc, x, Value::Num(i as f64), Value::Obj(o.clone())],
                )?;
            }
            Some(acc)
        }
        "copyWithin" => {
            let len = alen();
            let to = rel(it, &f, len, 0)?;
            let from = rel(it, &arg(a, 1), len, 0)?;
            let end = rel(it, &arg(a, 2), len, len)?;
            let count = end.saturating_sub(from).min(len - to);
            let mut b = o.borrow_mut();
            let cur = b.array.as_ref().unwrap().len();
            if cur < len {
                b.array.as_mut().unwrap().resize(len, Value::Undefined);
                b.add_holes(cur..len);
            }
            let src: Vec<(Value, bool)> = (from..from + count)
                .map(|i| (b.array.as_ref().unwrap()[i].clone(), b.is_hole(i)))
                .collect();
            b.remap_holes(|i| (!(to..to + count).contains(&i)).then_some(i));
            b.add_holes(
                src.iter()
                    .enumerate()
                    .filter(|(_, (_, h))| *h)
                    .map(|(k, _)| to + k),
            );
            let arr = b.array.as_mut().unwrap();
            for (k, (v, _)) in src.into_iter().enumerate() {
                arr[to + k] = v;
            }
            Some(Value::Obj(o.clone()))
        }
        "toLocaleString" => {
            let len = o.borrow().array.as_ref().map_or(0, Vec::len);
            let joined = crate::value::joining(o, || {
                let mut out = crate::value::JsBuf::new();
                for k in 0..len {
                    if k > 0 {
                        out.push_str(",");
                    }
                    let x = o.borrow().array.as_ref().and_then(|a| a.get(k).cloned());
                    if let Some(x) = x.filter(|x| !matches!(x, Value::Undefined | Value::Null)) {
                        let m = it.get(&x, "toLocaleString")?;
                        let r = it.call(&m, x, &[])?;
                        out.push_value(&it.to_str_value(&r)?);
                    }
                }
                Ok(out.finish())
            });
            match joined {
                Ok(Some(r)) => Some(r?),
                Ok(None) => Some(Value::str("")),
                Err(_) => it.stack_overflow()?,
            }
        }
        _ => None,
    })
}

// ---------- globals ----------

thread_local! {
    /// The state of `Math.random`'s xorshift64 generator, seeded from the system's random
    /// source when the thread first asks (QV4 seeds its generator from QRandomGenerator).
    static RNG: std::cell::Cell<u64> = std::cell::Cell::new(random_seed());
}

/// Starts this thread's `Math.random` from `seed`, for a run that must give the same numbers
/// as another (aotcheck's reference and candidate, like [`set_clock`]). Zero, the state
/// xorshift64 never leaves, is not a seed.
pub fn set_random_seed(seed: u64) {
    assert_ne!(seed, 0, "Math.random cannot start from zero");
    RNG.with(|r| r.set(seed));
}

/// A seed xorshift64 can start from: any 64 bits but zero, the state it never leaves.
fn random_seed() -> u64 {
    let mut b = [0u8; 8];
    getrandom::fill(&mut b).expect("platform RNG failed");
    u64::from_ne_bytes(b) | 1
}

#[cfg(test)]
thread_local! {
    pub static INSTALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Every global [`install`] defines: the single list the code generator (rigen) reads to know
/// which free names a JS global can shadow.
pub const GLOBALS: &[&str] = &[
    "AggregateError",
    "Array",
    "ArrayBuffer",
    "Atomics",
    "Boolean",
    "DOMException",
    "DataView",
    "Date",
    "Easing",
    "Error",
    "EvalError",
    "Float32Array",
    "Float64Array",
    "Function",
    "Int16Array",
    "Int32Array",
    "Int8Array",
    "JSON",
    "Map",
    "Math",
    "Number",
    "Object",
    "Promise",
    "Proxy",
    "Reflect",
    "QT_TRANSLATE_NOOP",
    "QT_TRID_NOOP",
    "QT_TR_NOOP",
    "Qt",
    "RangeError",
    "ReferenceError",
    "RegExp",
    "SQLException",
    "Set",
    "SharedArrayBuffer",
    "Symbol",
    "WeakMap",
    "WeakSet",
    "String",
    "SyntaxError",
    "TypeError",
    "URIError",
    "URL",
    "URLSearchParams",
    "Uint16Array",
    "Uint32Array",
    "Uint8Array",
    "Uint8ClampedArray",
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
    "unescape",
];

/// The global value properties name lookup resolves itself (no `install`ed var).
pub const VALUE_GLOBALS: &[&str] = &["undefined", "NaN", "Infinity"];

/// A property of the global object, which QML freezes (Qt docs, "JavaScript Environment
/// Restrictions"): a write to it throws like a write to an undeclared name.
pub fn is_frozen_global(name: &str) -> bool {
    GLOBALS.contains(&name)
}

/// Gives the built-in objects the own-key order of [`crate::key_order::KEY_ORDER`].
fn order_keys(it: &Interp) {
    let global = |name: &str| match it.global.borrow().vars.get(name) {
        Some(Value::Obj(o)) => Some(o.clone()),
        _ => None,
    };
    let prototype = |o: Obj| match o.borrow().props.get("prototype") {
        Some(Value::Obj(p)) => Some(p.clone()),
        _ => None,
    };
    for (path, keys) in crate::key_order::KEY_ORDER {
        let (name, proto) = match path.strip_suffix(".prototype") {
            Some(name) => (name, true),
            None => (*path, false),
        };
        let o = match name {
            "%TypedArray%" => global("Uint8Array").and_then(|o| o.borrow().proto.clone()),
            name => global(name),
        };
        let o = match proto {
            true => o.and_then(prototype),
            false => o,
        };
        if let Some(o) = o {
            o.borrow_mut().set_order(keys);
        }
    }
}

pub fn install(it: &mut Interp) {
    #[cfg(test)]
    INSTALLS.with(|c| c.set(c.get() + 1));
    let g = it.global.clone();
    let def = |name: &str, v: Value| {
        debug_assert!(GLOBALS.contains(&name), "builtins::GLOBALS lacks {name}");
        g.borrow_mut().vars.insert(name.to_string(), v)
    };

    let math = new_obj();
    macro_rules! m1 {
        ($($n:literal => $f:expr),* $(,)?) => { $( nat(&math, $n, 1, |it, _, a| Ok(Value::Num(($f)(num(it, a, 0)?)))); )* };
    }
    m1!("abs" => f64::abs, "floor" => f64::floor, "ceil" => f64::ceil, "sqrt" => f64::sqrt, "sin" => f64::sin, "cos" => f64::cos,
        "tan" => f64::tan, "asin" => f64::asin, "acos" => f64::acos, "atan" => f64::atan, "exp" => f64::exp, "log" => f64::ln,
        "log2" => f64::log2, "log10" => f64::log10, "trunc" => f64::trunc, "cbrt" => f64::cbrt, "round" => js_round,
        "sign" => |x: f64| if x.is_nan() || x == 0.0 { x } else { x.signum() },
        "log1p" => f64::ln_1p, "expm1" => f64::exp_m1, "cosh" => f64::cosh, "sinh" => f64::sinh,
        "tanh" => f64::tanh, "acosh" => libm_acosh, "asinh" => libm_asinh, "atanh" => libm_atanh,
        "fround" => |x: f64| x as f32 as f64,
        "clz32" => |x: f64| (crate::eval::to_i32(x) as u32).leading_zeros() as f64);
    nat(&math, "imul", 2, |it, _, a| {
        let (x, y) = (num(it, a, 0)?, num(it, a, 1)?);
        Ok(Value::Num(
            crate::eval::to_i32(x).wrapping_mul(crate::eval::to_i32(y)) as f64,
        ))
    });
    nat(&math, "pow", 2, |it, _, a| {
        let (x, y) = (num(it, a, 0)?, num(it, a, 1)?);
        Ok(Value::Num(crate::eval::num_arith(
            crate::eval::BinOp::Exp,
            x,
            y,
        )))
    });
    nat(&math, "atan2", 2, |it, _, a| {
        Ok(Value::Num(num(it, a, 0)?.atan2(num(it, a, 1)?)))
    });
    nat(&math, "hypot", 2, |it, _, a| {
        let xs = nums(it, a)?;
        Ok(Value::Num(hypot(&xs)))
    });
    nat(&math, "max", 2, |it, _, a| {
        let xs = nums(it, a)?;
        Ok(Value::Num(xs.into_iter().fold(
            f64::NEG_INFINITY,
            |m, x| {
                if x.is_nan() || m.is_nan() {
                    f64::NAN
                } else if x == m {
                    if x.is_sign_positive() { x } else { m }
                } else {
                    m.max(x)
                }
            },
        )))
    });
    nat(&math, "min", 2, |it, _, a| {
        let xs = nums(it, a)?;
        Ok(Value::Num(xs.into_iter().fold(f64::INFINITY, |m, x| {
            if x.is_nan() || m.is_nan() {
                f64::NAN
            } else if x == m {
                if x.is_sign_negative() { x } else { m }
            } else {
                m.min(x)
            }
        })))
    });
    nat(&math, "random", 0, |_, _, _| {
        let x = RNG.with(|r| {
            let mut x = r.get();
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            r.set(x);
            x
        });
        Ok(Value::Num((x >> 11) as f64 / (1u64 << 53) as f64))
    });
    for (k, v) in [
        ("E", std::f64::consts::E),
        ("LN10", std::f64::consts::LN_10),
        ("LN2", std::f64::consts::LN_2),
        ("LOG10E", std::f64::consts::LOG10_E),
        ("LOG2E", std::f64::consts::LOG2_E),
        ("PI", std::f64::consts::PI),
        ("SQRT1_2", std::f64::consts::FRAC_1_SQRT_2),
        ("SQRT2", std::f64::consts::SQRT_2),
    ] {
        set_constant(&math, k, Value::Num(v));
    }
    def("Math", Value::Obj(math));

    let json = new_obj();
    nat(&json, "stringify", 3, |it, _, a| {
        // ECMA-262 §25.5.2 steps 5-8: a Number or String object gives its primitive; a
        // number is that many spaces, a string its first 10 code units, both at most 10.
        let space = match arg(a, 2) {
            Value::Obj(o) => match crate::value::wrapped(&o) {
                Some(Value::Num(_)) => Value::Num(it.to_numeric(&Value::Obj(o))?),
                Some(Value::Str(_) | Value::Str16(_)) => it.to_str_value(&Value::Obj(o))?,
                _ => Value::Undefined,
            },
            v => v,
        };
        let indent = match &space {
            Value::Num(n) => " ".repeat(n.clamp(0.0, 10.0) as usize),
            Value::Str(_) | Value::Str16(_) => {
                let u: Vec<u16> = space.to_js_string().encode_utf16().take(10).collect();
                String::from_utf16_lossy(&u)
            }
            _ => String::new(),
        };
        let replacer = Some(arg(a, 1)).filter(|f| crate::eval::callable(f).is_some());
        let keys = match replacer {
            Some(_) => None,
            None => crate::json::property_list(it, &arg(a, 1))?,
        };
        Ok(
            crate::json::stringify_with(it, &arg(a, 0), replacer, keys, &indent)?
                .map(|s| Value::str(&s))
                .unwrap_or_default(),
        )
    });
    nat(&json, "parse", 2, |it, _, a| {
        let text = it.to_string(&arg(a, 0))?;
        let v = match crate::json::parse(&text) {
            Ok(v) => v,
            Err(_) => return it.throw("SyntaxError", "JSON.parse: Parse error"),
        };
        match arg(a, 1) {
            r if crate::eval::callable(&r).is_some() => crate::json::revive(it, v, &r),
            _ => Ok(v),
        }
    });
    def("JSON", Value::Obj(json));

    let object = new_obj();
    nat(&object, "keys", 1, |it, _, a| {
        coercible(it, &arg(a, 0))?;
        Ok(Value::array(
            it.keys(&arg(a, 0))?
                .into_iter()
                .map(|k| Value::str(&k))
                .collect(),
        ))
    });
    nat(&object, "values", 1, |it, _, a| {
        coercible(it, &arg(a, 0))?;
        Ok(Value::array(
            it.own_entries(&arg(a, 0))?
                .into_iter()
                .map(|(_, v)| v)
                .collect(),
        ))
    });
    nat(&object, "entries", 1, |it, _, a| {
        coercible(it, &arg(a, 0))?;
        Ok(Value::array(
            it.own_entries(&arg(a, 0))?
                .into_iter()
                .map(|(k, v)| Value::array(vec![Value::str(&k), v]))
                .collect(),
        ))
    });
    nat(&object, "assign", 2, |it, _, a| {
        let t = arg(a, 0);
        coercible(it, &t)?;
        for src in a.iter().skip(1) {
            // What script defined on a value type by name is not its own to copy.
            let members = match src {
                Value::Obj(o) => crate::valuetypes::members(o),
                _ => None,
            };
            for (k, v) in it.own_entries(src)? {
                if members.is_some() && crate::props::own_desc(it, src, &k)?.is_none() {
                    continue;
                }
                it.put_or_throw(&t, &k, v)?;
            }
            for (s, v) in it.own_symbol_entries(src)? {
                it.set_sym(&t, &s, v, true)?;
            }
        }
        Ok(t)
    });
    install_object_reflection(&object);
    nat(&object, "fromEntries", 1, |it, _, a| {
        let o = new_obj();
        for e in it.iterate(&arg(a, 0))? {
            let k = it.get(&e, "0")?;
            let k = it.to_string(&k)?;
            let v = it.get(&e, "1")?;
            set_own(&o, &k, v);
        }
        Ok(Value::Obj(o))
    });
    nat(&object, "create", 2, |it, _, a| {
        let o = new_obj();
        match arg(a, 0) {
            Value::Obj(p) => o.borrow_mut().proto = Some(p),
            Value::Null => o.borrow_mut().null_proto = true,
            _ => return it.type_error(),
        }
        if !matches!(arg(a, 1), Value::Undefined) {
            define_properties(it, &o, &arg(a, 1))?;
        }
        Ok(Value::Obj(o))
    });
    nat(&object, "is", 2, |_, _, a| {
        Ok(Value::Bool(same_value(&arg(a, 0), &arg(a, 1))))
    });
    object.borrow_mut().call = Some(Callable::Native("Object", object_ctor));
    let proto = it.object_proto.clone();
    install_object_prototype(&proto);
    link_builtin_prototype(&object, &proto);
    set_length(&object, 1);
    def("Object", Value::Obj(object));
    def("Reflect", Value::Obj(reflect_object()));
    def("Proxy", proxy_ctor());

    let array = new_obj();
    nat(&array, "isArray", 1, |it, _, a| {
        // A QML sequence is no array to V4's Array.isArray, though it acts as one, and
        // neither is the value of a `list<T>` property (a sequence wrapper).
        let v = arg(a, 0);
        if let Value::Obj(o) = &v
            && crate::valuetypes::is_readonly_sequence(&o.borrow())
        {
            return Ok(Value::Bool(false));
        }
        Ok(Value::Bool(
            crate::props::is_array(it, &v)? && typed_list(it, &v).is_none(),
        ))
    });
    nat(&array, "from", 1, |it, this, a| {
        let src = arg(a, 0);
        // Array.from (ECMA-262 §23.1.2.1): the map function is checked before the source is
        // read (step 3); an iterable goes through `@@iterator`, anything else is array-like.
        let f = arg(a, 1);
        if !matches!(f, Value::Undefined) && crate::eval::callable(&f).is_none() {
            return it.throw(
                "TypeError",
                format!("{} is not a function", f.to_js_string()),
            );
        }
        let using = match &src {
            Value::Undefined | Value::Null => return it.no_object(&src),
            _ => it.get_sym(&src, &crate::value::WellKnown::Iterator.sym())?,
        };
        let (target, items) = match using {
            Value::Undefined | Value::Null => {
                let n = it.get(&src, "length")?;
                let n = it.to_numeric(&n)?;
                let n = if n > 0.0 {
                    crate::eval::array_len(it, n.trunc().min(4294967295.0))?
                } else {
                    0
                };
                let target = array_from_target(it, this, &[Value::Num(n as f64)])?;
                let mut v = Vec::with_capacity(n);
                for i in 0..n {
                    v.push(it.get(&src, &i.to_string())?);
                }
                (target, v)
            }
            f => {
                let target = array_from_target(it, this, &[])?;
                let iter = it.iterator_from_method(&src, &f)?;
                (target, it.collect_iter(iter)?)
            }
        };
        if matches!(f, Value::Undefined) {
            return species_result(it, target, items, Vec::new(), true);
        }
        let mut out = Vec::with_capacity(items.len());
        for (i, x) in items.into_iter().enumerate() {
            out.push(it.call(&f, arg(a, 2), &[x, Value::Num(i as f64)])?);
        }
        species_result(it, target, out, Vec::new(), true)
    });
    nat(&array, "of", 0, |it, this, a| {
        let target = array_from_target(it, this, &[Value::Num(a.len() as f64)])?;
        species_result(it, target, a.to_vec(), Vec::new(), true)
    });
    array.borrow_mut().call = Some(Callable::Native("Array", array_ctor));
    let array_proto = it.array_proto.clone();
    array_proto.borrow_mut().array = Some(Vec::new());
    install_array_prototype(&array_proto);
    link_builtin_prototype(&array, &array_proto);
    set_length(&array, 1);
    def("Array", Value::Obj(array));

    let number = new_obj();
    // Qt's QML Number extension (QQmlNumberExtension::method_fromLocaleString).
    nat(&number, "fromLocaleString", 0, |it, _, a| {
        let invalid = "Locale: Number.fromLocaleString(): Invalid arguments";
        let (locale, text) = match a {
            [text] => (Numeric::system(), text),
            [l, text] => match locale_of(l) {
                Some(l) => (l.numeric(), text),
                None => return it.throw("Error", invalid),
            },
            _ => return it.throw("Error", invalid),
        };
        let text = it.to_string(text)?;
        if text.is_empty() {
            return Ok(Value::Num(f64::NAN));
        }
        match locale.to_double(&text) {
            Some(d) => Ok(Value::Num(d)),
            None => it.throw("Error", "Locale: Number.fromLocaleString(): Invalid format"),
        }
    });
    nat(&number, "isInteger", 1, |_, _, a| {
        Ok(Value::Bool(
            matches!(arg(a, 0), Value::Num(n) if n.is_finite() && n.fract() == 0.0),
        ))
    });
    nat(&number, "isSafeInteger", 1, |_, _, a| {
        Ok(Value::Bool(matches!(arg(a, 0), Value::Num(n)
            if n.is_finite() && n.fract() == 0.0 && n.abs() <= 9007199254740991.0)))
    });
    nat(&number, "isFinite", 1, |_, _, a| {
        Ok(Value::Bool(
            matches!(arg(a, 0), Value::Num(n) if n.is_finite()),
        ))
    });
    nat(&number, "isNaN", 1, |_, _, a| {
        Ok(Value::Bool(
            matches!(arg(a, 0), Value::Num(n) if n.is_nan()),
        ))
    });
    // Number.parseFloat and Number.parseInt are the global functions (ECMA-262 §21.1.2.12-13).
    let parse_float_fn = native("parseFloat", 1, |it, _, a| {
        Ok(Value::Num(parse_float(&it.to_string(&arg(a, 0))?)))
    });
    let parse_int_fn = native("parseInt", 2, |it, _, a| {
        let s = it.to_string(&arg(a, 0))?;
        Ok(Value::Num(parse_int(&s, num(it, a, 1)?)))
    });
    set_hidden(&number, "parseFloat", parse_float_fn.clone());
    set_hidden(&number, "parseInt", parse_int_fn.clone());
    for (k, v) in [
        ("MAX_SAFE_INTEGER", 9007199254740991.0),
        ("MIN_SAFE_INTEGER", -9007199254740991.0),
        ("EPSILON", f64::EPSILON),
        ("MAX_VALUE", f64::MAX),
        ("MIN_VALUE", 5e-324),
        ("NaN", f64::NAN),
        ("POSITIVE_INFINITY", f64::INFINITY),
        ("NEGATIVE_INFINITY", f64::NEG_INFINITY),
    ] {
        set_constant(&number, k, Value::Num(v));
    }
    number.borrow_mut().call = Some(Callable::Native("Number", |it, _, a| {
        Ok(Value::Num(if a.is_empty() {
            0.0
        } else {
            it.to_numeric(&a[0])?
        }))
    }));
    number.borrow_mut().internal = Internal::WrapperConstructor;
    let number_proto = it.number_proto.clone();
    number_proto.borrow_mut().internal = Internal::Primitive(Value::Num(0.0));
    install_number_prototype(&number_proto);
    link_builtin_prototype(&number, &number_proto);
    set_length(&number, 1);
    def("Number", Value::Obj(number));

    let string = new_obj();
    string.borrow_mut().call = Some(Callable::Native("String", |it, _, a| {
        match a.first() {
            None => Ok(Value::str("")),
            // String(symbol) is its SymbolDescriptiveString (ECMA-262 §22.1.1.1).
            Some(Value::Symbol(s)) if !it.constructing => Ok(Value::str(&s.descriptive())),
            Some(v) => it.to_str_named(v),
        }
    }));
    string.borrow_mut().internal = Internal::WrapperConstructor;
    install_string_statics(&string);
    let string_proto = it.string_proto.clone();
    string_proto.borrow_mut().internal = Internal::Primitive(Value::str(""));
    install_string_prototype(&string_proto);
    // %String.prototype% is itself a String object for "" (ECMA-262 §22.1.3): its own
    // `length` is 0, non-enumerable, read-only and non-configurable.
    set_hidden(&string_proto, "length", Value::Num(0.0));
    string_proto
        .borrow_mut()
        .set_attr("length", READONLY | PERMANENT);
    link_builtin_prototype(&string, &string_proto);
    set_length(&string, 1);
    def("String", Value::Obj(string));
    let boolean = new_obj();
    boolean.borrow_mut().call = Some(Callable::Native("Boolean", |_, _, a| {
        Ok(Value::Bool(arg(a, 0).truthy()))
    }));
    boolean.borrow_mut().internal = Internal::WrapperConstructor;
    let boolean_proto = it.boolean_proto.clone();
    boolean_proto.borrow_mut().internal = Internal::Primitive(Value::Bool(false));
    install_boolean_prototype(&boolean_proto);
    link_builtin_prototype(&boolean, &boolean_proto);
    set_length(&boolean, 1);
    def("Boolean", Value::Obj(boolean));
    // %Function.prototype% is itself a function that accepts anything and returns undefined
    // (ECMA-262 §20.2.3).
    let function = new_obj();
    function.borrow_mut().call = Some(Callable::Native("Function", function_ctor));
    let function_proto = it.function_proto.clone();
    function_proto.borrow_mut().call = Some(Callable::Native("", |_, _, _| Ok(Value::Undefined)));
    install_function_prototype(&function_proto);
    install_function_prototype_extras(&function_proto);
    link_builtin_prototype(&function, &function_proto);
    set_length(&function, 1);
    // %GeneratorFunction%'s [[Prototype]] is %Function% (ECMA-262 §27.3.2).
    it.generator_protos.constructor.borrow_mut().proto = Some(function.clone());
    def("Function", Value::Obj(function));
    let array_to_string = crate::typedarray::array_proto_member(it, "toString");
    crate::typedarray::install(
        &mut |n, v| {
            def(n, v);
        },
        array_to_string,
        it.function_proto.clone(),
    );
    def("parseInt", parse_int_fn);
    def("parseFloat", parse_float_fn);
    def(
        "isNaN",
        native("isNaN", 1, |it, _, a| {
            Ok(Value::Bool(num(it, a, 0)?.is_nan()))
        }),
    );
    def(
        "isFinite",
        native("isFinite", 1, |it, _, a| {
            Ok(Value::Bool(num(it, a, 0)?.is_finite()))
        }),
    );
    for (name, f) in [
        (
            "encodeURIComponent",
            (|it, _, a| encode_uri(it, &arg(a, 0), "")) as NativeFn,
        ),
        ("encodeURI", |it, _, a| {
            encode_uri(it, &arg(a, 0), ";,/?:@&=+$#")
        }),
        ("decodeURIComponent", |it, _, a| {
            decode_uri(it, &arg(a, 0), "")
        }),
        ("decodeURI", |it, _, a| {
            decode_uri(it, &arg(a, 0), ";,/?:@&=+$#")
        }),
    ] {
        def(name, native(name, 1, f));
    }
    for (name, length, f) in [
        (
            "escape",
            1,
            (|it, _, a| {
                Ok(Value::str(&js_escape(&units(
                    &it.to_str_value(&arg(a, 0))?,
                ))))
            }) as NativeFn,
        ),
        ("unescape", 1, |it, _, a| {
            Ok(js_unescape(&units(&it.to_str_value(&arg(a, 0))?)))
        }),
        (EVAL, 1, |it, _, a| match arg(a, 0) {
            Value::Str(src) => eval_code(it, &src),
            v => Ok(v),
        }),
        ("gc", 0, |_, _, _| {
            crate::eval::request_sweep();
            Ok(Value::Undefined)
        }),
    ] {
        def(name, native(name, length, f));
    }
    for (name, codes) in [
        ("DOMException", &DOM_EXCEPTION_CODES[..]),
        ("SQLException", &SQL_EXCEPTION_CODES[..]),
    ] {
        let o = new_obj();
        for (i, k) in codes.iter().enumerate() {
            set_constant(&o, k, Value::Num((i + 1) as f64));
        }
        def(name, Value::Obj(o));
    }
    let mut error_proto: Option<Obj> = None;
    let mut error_ctor_obj: Option<Obj> = None;
    for (i, f) in [
        error_ctor::<0> as NativeFn,
        error_ctor::<1>,
        error_ctor::<2>,
        error_ctor::<3>,
        error_ctor::<4>,
        error_ctor::<5>,
        error_ctor::<6>,
    ]
    .into_iter()
    .enumerate()
    {
        let name = ERROR_KINDS[i];
        let ctor = native(name, 1, f);
        let proto = new_obj();
        set_hidden(&proto, "name", Value::str(name));
        set_hidden(&proto, "message", Value::str(""));
        // QV4 gives each error prototype its own `toString`.
        set_hidden(&proto, "toString", native("toString", 0, error_to_string));
        match &error_proto {
            None => error_proto = Some(proto.clone()),
            Some(base) => proto.borrow_mut().proto = Some(base.clone()),
        }
        if let Value::Obj(c) = &ctor {
            link_builtin_prototype(c, &proto);
            // The [[Prototype]] of a NativeError constructor is %Error% (ECMA-262 §20.5.6.2).
            match &error_ctor_obj {
                None => error_ctor_obj = Some(c.clone()),
                Some(base) => c.borrow_mut().proto = Some(base.clone()),
            }
        }
        def(name, ctor);
    }
    let aggregate = native("AggregateError", 2, crate::promise::aggregate_error_ctor);
    let aggregate_proto = new_obj();
    set_hidden(&aggregate_proto, "name", Value::str("AggregateError"));
    set_hidden(&aggregate_proto, "message", Value::str(""));
    aggregate_proto.borrow_mut().proto = error_proto;
    if let Value::Obj(c) = &aggregate {
        // As a NativeError's (ECMA-262 §20.5.7.2).
        c.borrow_mut().proto = error_ctor_obj;
        link_builtin_prototype(c, &aggregate_proto);
    }
    def("AggregateError", aggregate);
    let regexp = native("RegExp", 2, regexp_ctor);
    let regexp_proto = it.regexp_proto.clone();
    install_regexp_prototype(&regexp_proto);
    if let Value::Obj(c) = &regexp {
        for (k, getter, f) in LEGACY_STATICS {
            define_accessor(c, k, true, native(getter, 0, f), false);
        }
        link_builtin_prototype(c, &regexp_proto);
    }
    def("RegExp", regexp);
    for (name, ctor, proto, install_proto, size) in [
        (
            "Map",
            map_ctor as NativeFn,
            it.map_proto.clone(),
            install_map_prototype as fn(&Obj),
            coll_size::<true> as NativeFn,
        ),
        (
            "Set",
            set_ctor,
            it.set_proto.clone(),
            install_set_prototype,
            coll_size::<false>,
        ),
    ] {
        let c = native(name, 0, ctor);
        install_proto(&proto);
        define_accessor(&proto, "size", true, native("get size", 0, size), false);
        if let Value::Obj(c) = &c {
            link_builtin_prototype(c, &proto);
        }
        def(name, c);
    }
    def("Promise", crate::promise::constructor(it));
    // Before the symbol-keyed built-ins, which give URLSearchParams its `@@iterator`.
    def("URL", crate::url::url_ctor());
    def("URLSearchParams", crate::url::params_ctor());
    for (name, v) in crate::symbol::install(it) {
        def(name, v);
    }
    def("Date", date_ctor());
    def("XMLHttpRequest", crate::xhr::ctor(&it.vt_protos.xhr));
    def("Easing", Value::Obj(easing_object(&it.function_proto)));

    let console = new_obj();
    for (n, f) in CONSOLE {
        set_hidden(&console, n, fn_value(Callable::Native(n, f)));
    }
    def("console", Value::Obj(console));
    // QJSEngine global print() is console.log.
    def(
        "print",
        fn_value(Callable::Native("print", |it, _, a| console_out(it, a, 0))),
    );

    def("Qt", Value::Obj(qt_object()));

    // No translator is installed (the original translates through its own Translation service), so the
    // QJSEngine translation functions return the source text, with `%n` substituted.
    def(
        "qsTr",
        fn_value(Callable::Native("qsTr", |it, _, a| {
            tr(it, "qsTr", &["sourceText", "disambiguation"], a)
        })),
    );
    def(
        "qsTranslate",
        fn_value(Callable::Native("qsTranslate", |it, _, a| {
            tr(
                it,
                "qsTranslate",
                &["context", "sourceText", "disambiguation"],
                a,
            )
        })),
    );
    def(
        "qsTrId",
        fn_value(Callable::Native("qsTrId", |it, _, a| {
            tr(it, "qsTrId", &["id"], a)
        })),
    );
    def(
        "QT_TR_NOOP",
        fn_value(Callable::Native("QT_TR_NOOP", |_, _, a| Ok(arg(a, 0)))),
    );
    def(
        "QT_TRANSLATE_NOOP",
        fn_value(Callable::Native("QT_TRANSLATE_NOOP", |_, _, a| {
            Ok(arg(a, 1))
        })),
    );
    def(
        "QT_TRID_NOOP",
        fn_value(Callable::Native("QT_TRID_NOOP", |_, _, a| Ok(arg(a, 0)))),
    );
    g.borrow_mut().symbol_realm = Some(Box::new(it.sym.clone()));
    order_keys(it);
    lock_builtins(it);
    install_global_object(it);
}

/// What a QML engine does to the built-ins once they are installed (V4's
/// `ExecutionEngine::lockObject` of the global object): every property that holds an object
/// (a method, a prototype, a getter) becomes read-only and non-configurable, in every object
/// reached through such properties. `toString`, `toLocaleString`, `valueOf` and `constructor`
/// stay as they are, so that objects can override them; so do properties holding primitives
/// (`name`, `length`, `Error.prototype.message`) and the objects' extensibility. A locked
/// object keeps its prototype. `Qt` is a C++ object there, which is left alone.
fn lock_builtins(it: &Interp) {
    const WRITABLE: [&str; 4] = ["toString", "toLocaleString", "valueOf", "constructor"];
    let mut stack: Vec<Obj> = it
        .global
        .borrow()
        .vars
        .iter()
        .filter(|(k, _)| *k != "Qt")
        .filter_map(|(_, v)| match v {
            Value::Obj(o) => Some(o.clone()),
            _ => None,
        })
        .collect();
    let mut seen: rustc_hash::FxHashSet<*const RefCell<crate::value::JsObject>> =
        Default::default();
    while let Some(o) = stack.pop() {
        if !seen.insert(Rc::as_ptr(&o)) {
            continue;
        }
        let mut b = o.borrow_mut();
        b.locked = true;
        let mut lock: Vec<(Rc<str>, u8)> = Vec::new();
        for (k, v) in &b.props {
            if let Value::Obj(m) = v {
                stack.push(m.clone());
                if !WRITABLE.contains(&&**k) {
                    lock.push((k.clone(), READONLY | PERMANENT));
                }
            }
        }
        for (k, a) in b.accessors.iter().flat_map(|m| m.iter()) {
            if let Some(Value::Obj(m)) = &a.get {
                stack.push(m.clone());
                lock.push((k.clone(), PERMANENT));
            }
        }
        for (k, bits) in lock {
            let bits = bits | b.attr(&k);
            b.set_attr(&k, bits);
        }
        for p in b.ext.iter_mut().flat_map(|e| e.syms.iter_mut()) {
            match (&p.value, &p.accessor) {
                (_, Some(a)) => {
                    if let Some(Value::Obj(m)) = &a.get {
                        stack.push(m.clone());
                        p.attrs |= PERMANENT;
                    }
                }
                (Value::Obj(m), None) => {
                    stack.push(m.clone());
                    p.attrs |= READONLY | PERMANENT;
                }
                _ => {}
            }
        }
    }
}

/// The global object (ECMA-262 §19): every global [`install`] defined, and `undefined`, `NaN`
/// and `Infinity`, as the read-only, non-enumerable, non-configurable properties they are in
/// QV4 (whose global object is otherwise extensible).
fn install_global_object(it: &Interp) {
    let g = it.realm();
    let o = it.global_object.clone();
    let mut names: Vec<(String, Value)> = g
        .borrow()
        .vars
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    names.sort_by(|a, b| a.0.cmp(&b.0));
    for (k, v) in names {
        set_constant(&o, &k, v);
    }
    set_constant(&o, "undefined", Value::Undefined);
    set_constant(&o, "NaN", Value::Num(f64::NAN));
    set_constant(&o, "Infinity", Value::Num(f64::INFINITY));
}

/// ES SameValue (`Object.is`): like `===` except `NaN` is itself and `+0` is not `-0`.
pub fn same_value(x: &Value, y: &Value) -> bool {
    match (x, y) {
        (Value::Num(a), Value::Num(b)) if a.is_nan() || b.is_nan() => a.is_nan() && b.is_nan(),
        (Value::Num(a), Value::Num(b)) => a == b && a.is_sign_negative() == b.is_sign_negative(),
        _ => x.strict_eq(y),
    }
}

const ORDINAL: [&str; 4] = ["first", "second", "third", "fourth"];

/// QV4 GlobalExtensions qsTr/qsTranslate/qsTrId with no translation loaded: argument checks as
/// in Qt, then QCoreApplication::translate's `%n`/`%Ln` substitution of the optional count.
fn tr(it: &mut Interp, f: &str, strs: &[&str], a: &[Value]) -> R<Value> {
    let need = if f == "qsTranslate" { 2 } else { 1 };
    if a.len() < need {
        let n = if need == 2 {
            "two arguments"
        } else {
            "one argument"
        };
        return it.throw("Error", format!("{f}() requires at least {n}"));
    }
    // Only qsTrId's checks are type errors (THROW_TYPE_ERROR_WITH_MESSAGE in Qt).
    let kind = if f == "qsTrId" { "TypeError" } else { "Error" };
    for (i, v) in a.iter().enumerate().take(strs.len()) {
        if !v.is_str() {
            return it.throw(
                kind,
                format!(
                    "{f}(): {} argument ({}) must be a string",
                    ORDINAL[i], strs[i]
                ),
            );
        }
    }
    let mut at = strs.len();
    let mut n = -1;
    if f == "qsTranslate" {
        // A string after the disambiguation is Qt 4's encoding: skipped with a warning.
        // The count is then whatever ToInt32 makes of the next argument.
        if a.get(at).is_some_and(Value::is_str) {
            message(
                2,
                "qsTranslate(): specifying the encoding as fourth argument is deprecated",
            );
            at += 1;
        }
        if let Some(v) = a.get(at) {
            n = crate::eval::to_i32(it.to_numeric(v)?);
        }
    } else if let Some(v) = a.get(at) {
        let Value::Num(x) = v else {
            return it.throw(
                kind,
                format!("{f}(): {} argument (n) must be a number", ORDINAL[at]),
            );
        };
        n = crate::eval::to_i32(*x);
    }
    let text = arg(a, if f == "qsTranslate" { 1 } else { 0 }).to_js_string();
    Ok(Value::str(&replace_percent_n(&text, n)))
}

/// QCoreApplication's replacePercentN: `%n` → n, `%Ln` → n with the locale's group separator.
fn replace_percent_n(s: &str, n: i32) -> String {
    if n < 0 {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(p) = rest.find('%') {
        out.push_str(&rest[..p]);
        let after = &rest[p + 1..];
        if let Some(r) = after.strip_prefix('n') {
            out.push_str(&n.to_string());
            rest = r;
        } else if let Some(r) = after.strip_prefix("Ln") {
            out.push_str(&group_digits(n, &system_locale_name()));
            rest = r;
        } else {
            out.push('%');
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// QLocale::toString(int) of the locale named `locale`: its digits, minus sign, group
/// separator and group sizes (the C locale omits group separators).
fn group_digits(n: i32, locale: &str) -> String {
    Locale::named(locale)
        .numeric()
        .to_string(f64::from(n), b'f', 0)
}

/// `parseFloat`: the longest prefix that is a StrDecimalLiteral.
pub(crate) fn parse_float(s: &str) -> f64 {
    let s = s.trim_start_matches(crate::value::is_js_space);
    let b = s.as_bytes();
    let mut i = usize::from(matches!(b.first(), Some(b'+' | b'-')));
    if s[i..].starts_with("Infinity") {
        return if b[0] == b'-' {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        };
    }
    let digits = |i: &mut usize| {
        let st = *i;
        while b.get(*i).is_some_and(u8::is_ascii_digit) {
            *i += 1;
        }
        *i > st
    };
    let mut any = digits(&mut i);
    if b.get(i) == Some(&b'.') {
        let mut j = i + 1;
        if digits(&mut j) || any {
            any = true;
            i = j;
        }
    }
    if !any {
        return f64::NAN;
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        let mut j = i + 1 + usize::from(matches!(b.get(i + 1), Some(b'+' | b'-')));
        if digits(&mut j) {
            i = j;
        }
    }
    s[..i].parse().unwrap_or(f64::NAN)
}

/// `parseInt`; radix 10 rounds like V8's strtod, other radices exactly up to 128 bits.
pub(crate) fn parse_int(s: &str, radix: f64) -> f64 {
    let mut s = s.trim_start_matches(crate::value::is_js_space);
    let neg = s.starts_with('-');
    if neg || s.starts_with('+') {
        s = &s[1..];
    }
    // ECMA-262 §19.2.5 step 8: the radix is ToInt32(radix), 0 meaning "not given".
    let r = crate::eval::to_i32(radix);
    let auto = r == 0;
    let mut r = if auto { 10 } else { r };
    if (r == 16 || auto) && (s.starts_with("0x") || s.starts_with("0X")) {
        s = &s[2..];
        r = 16;
    }
    if !(2..=36).contains(&r) {
        return f64::NAN;
    }
    let r = r as u32;
    let end = s.find(|c: char| !c.is_digit(r)).unwrap_or(s.len());
    let digits = &s[..end];
    if digits.is_empty() {
        return f64::NAN;
    }
    let v = if r == 10 {
        digits.parse().unwrap_or(f64::NAN)
    } else {
        crate::value::radix_digits(digits, r)
    };
    if neg { -v } else { v }
}

/// The members of Qt 6.12's `Qt` object in its declaration order (what V4 enumerates).
pub(crate) const QT_MEMBERS: &[&str] = &[
    "objectName",
    "application",
    "platform",
    "inputMethod",
    "styleHints",
    "uiLanguage",
    "objectNameChanged",
    "uiLanguageChanged",
    "include",
    "isQtObject",
    "color",
    "rgba",
    "hsla",
    "hsva",
    "colorEqual",
    "rect",
    "point",
    "size",
    "vector2d",
    "vector3d",
    "vector4d",
    "quaternion",
    "matrix4x4",
    "lighter",
    "darker",
    "alpha",
    "tint",
    "formatDate",
    "formatTime",
    "formatDateTime",
    "locale",
    "url",
    "resolvedUrl",
    "openUrlExternally",
    "font",
    "fontFamilies",
    "md5",
    "btoa",
    "atob",
    "escapeHtml",
    "quit",
    "exit",
    "createQmlObject",
    "createComponent",
    "binding",
    "callLater",
    "enumStringToValue",
    "enumValueToString",
    "enumValueToStrings",
];

/// `Qt.binding(f)` (QtObject::binding): a function object that, assigned to a QML property
/// from JavaScript, makes `f` that property's binding. Calling it throws.
fn qt_binding(it: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
    match a.len() {
        0 => return it.throw("Error", "Insufficient arguments"),
        1 => {}
        _ => return it.throw("Error", "Too many arguments"),
    }
    let f = binding_target(&a[0]).unwrap_or_else(|| a[0].clone());
    if !matches!(
        crate::eval::callable(&f),
        Some(c) if !matches!(c, Callable::Native(..) | Callable::Item(..))
    ) {
        return it.throw(
            "Error",
            "binding(): argument (binding expression) must be a function",
        );
    }
    let name = it.get(&f, "name")?;
    let o = fn_value(Callable::Native("binding", |it, _, _| {
        it.throw("TypeError", "Bindings must not be called directly.")
    }));
    if let Value::Obj(b) = &o {
        set_hidden(b, "name", name);
        set_hidden(b, "length", Value::Num(0.0));
        b.borrow_mut().internal = Internal::Binding(f);
    }
    Ok(o)
}

/// `Qt.platform` (QQmlPlatform): the OS name as Qt spells it, and the QPA plugin, which Qt
/// picks from `QT_QPA_PLATFORM` (the part before the first `:`) and otherwise uses Wayland.
fn qt_platform() -> Obj {
    let os = if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "macos") {
        "osx"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "android") {
        "android"
    } else {
        "unix"
    };
    let plugin = std::env::var("QT_QPA_PLATFORM")
        .ok()
        .and_then(|p| p.split([':', ';']).next().map(str::to_owned))
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| "wayland".to_owned());
    let p = new_obj();
    set_own(&p, "os", Value::str(os));
    set_own(&p, "pluginName", Value::str(&plugin));
    p.borrow_mut().native = Some(Rc::new(crate::qtobj::QtPlatform));
    p
}

/// The function a `Qt.binding` result binds.
pub fn binding_target(v: &Value) -> Option<Value> {
    match v {
        Value::Obj(o) => match &o.borrow().internal {
            Internal::Binding(f) => Some(f.clone()),
            _ => None,
        },
        _ => None,
    }
}

fn qt_object() -> Obj {
    let qt = new_obj();
    /// `Qt.vector2d(x, y)`, `Qt.point(x, y)`, `Qt.rect(x, y, width, height)` and kin: a
    /// value type from exactly its components (QtObject's Q_INVOKABLEs).
    fn components(it: &mut Interp, vt: crate::valuetypes::Vt, a: &[Value]) -> R<Value> {
        let n = vt.data().len();
        crate::qtobj::arity_range(it, a, n, n)?;
        let mut v = Vec::with_capacity(n);
        for x in a {
            v.push(it.to_numeric(x)?);
        }
        Ok(crate::valuetypes::make_num(vt, &v))
    }
    use crate::valuetypes::Vt;
    qt_method(&qt, "vector2d", |it, _, a| components(it, Vt::Vector2D, a));
    qt_method(&qt, "vector3d", |it, _, a| components(it, Vt::Vector3D, a));
    qt_method(&qt, "vector4d", |it, _, a| components(it, Vt::Vector4D, a));
    qt_method(&qt, "point", |it, _, a| components(it, Vt::PointF, a));
    qt_method(&qt, "size", |it, _, a| components(it, Vt::SizeF, a));
    qt_method(&qt, "rect", |it, _, a| components(it, Vt::RectF, a));
    qt_method(&qt, "rgba", crate::qtobj::rgba);
    qt_method(&qt, "hsla", crate::qtobj::hsla);
    qt_method(&qt, "hsva", crate::qtobj::hsva);
    qt_method(&qt, "color", crate::qtobj::color);
    qt_method(&qt, "lighter", crate::qtobj::lighter);
    qt_method(&qt, "darker", crate::qtobj::darker);
    qt_method(&qt, "alpha", crate::qtobj::alpha);
    qt_method(&qt, "resolvedUrl", crate::qtobj::resolved_url);
    qt_method(&qt, "createQmlObject", crate::runtime::create_qml_object);
    qt_method(&qt, "binding", qt_binding);
    set_own(&qt, "platform", Value::Obj(qt_platform()));
    // QQmlDelayedCallQueue::addUniquelyAndExecuteLater.
    qt_method(&qt, "callLater", |it, _, a| {
        let Some((f, args)) = a.split_first() else {
            return it.throw("Error", "Qt.callLater: no arguments given");
        };
        if crate::eval::callable(f).is_none() {
            return it.throw(
                "Error",
                "Qt.callLater: first argument not a function or signal",
            );
        }
        if !it.host.call_later(f.clone(), args.to_vec()) {
            return it.throw("Error", "Qt.callLater: no QML engine");
        }
        Ok(Value::Undefined)
    });
    qt_method(&qt, "quit", |it, _, _| {
        if let Some(w) = crate::runtime::world_of(it) {
            w.borrow_mut().quit.get_or_insert(0);
        }
        Ok(Value::Undefined)
    });
    qt_method(&qt, "exit", |it, _, a| {
        if let Some(w) = crate::runtime::world_of(it) {
            // QCoreApplication::exit(code) each time: the last code asked for is the status,
            // also when `Component.onDestruction` asks while the shell is already ending.
            // `Qt.quit()` keeps the code (EngineGeneration::quit sets none).
            w.borrow_mut().quit = Some(arg(a, 0).to_number() as i32);
        }
        Ok(Value::Undefined)
    });
    // QDesktopServices::openUrl -> QGenericUnixServices: portal first, then `xdg-open URL`,
    // off the main thread.
    qt_method(&qt, "openUrlExternally", crate::qtobj::open_url_externally);
    qt_method(&qt, "locale", |it, _, a| {
        if a.len() > 1 {
            return it.throw("Error", "Too many arguments");
        }
        Ok(locale_object(&arg(a, 0)))
    });
    qt_method(&qt, "md5", crate::qtobj::md5);
    qt_method(&qt, "formatDateTime", |it, _, a| {
        qt_format_call(it, "formatDateTime", Parts::DateTime, a)
    });
    qt_method(&qt, "formatDate", |it, _, a| {
        qt_format_call(it, "formatDate", Parts::Date, a)
    });
    qt_method(&qt, "formatTime", |it, _, a| {
        qt_format_call(it, "formatTime", Parts::Time, a)
    });
    for name in [
        "objectName",
        "application",
        "inputMethod",
        "styleHints",
        "uiLanguage",
        "objectNameChanged",
        "uiLanguageChanged",
    ] {
        crate::qtobj::define_qobject_member(&qt, name);
    }
    qt.borrow_mut().native = Some(Rc::new(crate::qtobj::QtGlobal));
    crate::eval::set_hidden(&qt, "toString", crate::types::own_to_string());
    qt_method(&qt, "include", crate::qtobj::include);
    qt_method(&qt, "isQtObject", crate::qtobj::is_qt_object);
    qt_method(&qt, "colorEqual", crate::qtobj::color_equal);
    qt_method(&qt, "quaternion", crate::qtobj::quaternion);
    qt_method(&qt, "matrix4x4", crate::qtobj::matrix4x4);
    qt_method(&qt, "tint", crate::qtobj::tint);
    qt_method(&qt, "url", crate::qtobj::url);
    qt_method(&qt, "font", crate::qtobj::font);
    qt_method(&qt, "fontFamilies", crate::qtobj::font_families);
    qt_method(&qt, "btoa", crate::qtobj::btoa);
    qt_method(&qt, "atob", crate::qtobj::atob);
    qt_method(&qt, "escapeHtml", crate::qtobj::escape_html);
    qt_method(&qt, "createComponent", crate::runtime::create_component);
    qt_method(&qt, "enumStringToValue", crate::qtobj::enum_string_to_value);
    qt_method(&qt, "enumValueToString", crate::qtobj::enum_value_to_string);
    qt_method(
        &qt,
        "enumValueToStrings",
        crate::qtobj::enum_value_to_strings,
    );
    // V4 lists the QtObject members in its metaobject's declaration order.
    {
        let mut b = qt.borrow_mut();
        let rank = |k: &str| {
            QT_MEMBERS
                .iter()
                .position(|m| *m == k)
                .unwrap_or(usize::MAX)
        };
        b.keys.sort_by_key(|k| rank(k));
    }
    // Enum values are looked up on the namespace, not listed with its properties.
    for (k, v) in crate::qt_enums::QT_ENUMS {
        set_hidden(&qt, k, Value::Num(*v as f64));
    }
    for (ty, keys) in crate::qt_enums::QT_ENUM_TYPES {
        let ns: Vec<_> = keys
            .iter()
            .map(|(k, v)| (Some(ty.to_string()), k.to_string(), *v as f64))
            .collect();
        if let Value::Obj(o) = crate::types::namespace_object(&ns)
            && let Some(e) = o.borrow().props.get(*ty)
        {
            set_hidden(&qt, ty, e.clone());
        }
    }
    // QV4::QObjectMethod::method_destroy on an object the engine owns. Not a member of the
    // meta-object: `Object.keys(Qt)` does not list it.
    let destroy: NativeFn = |it, _, _| {
        it.throw(
            "Error",
            "Invalid attempt to destroy() an indestructible object",
        )
    };
    set_hidden(
        &qt,
        "destroy",
        fn_value(Callable::Native("destroy", destroy)),
    );
    crate::unwritten::fill(&qt, "Qt");
    qt
}

#[cfg(test)]
mod globals_tests {
    use crate::eval::Interp;

    #[test]
    fn install_defines_exactly_globals() {
        let it = Interp::default();
        let mut got: Vec<String> = it.global.borrow().vars.keys().cloned().collect();
        got.sort();
        let mut want: Vec<String> = super::GLOBALS.iter().map(|s| s.to_string()).collect();
        want.sort();
        assert_eq!(got, want);
    }
}

#[cfg(all(test, feature = "interp"))]
mod tests {
    use super::*;
    use crate::eval::{Ex, Interp};

    /// QColor::fromString takes hex digits only: a sign (which `from_str_radix` accepts) or a
    /// non-ASCII character makes the colour invalid.
    #[test]
    fn parse_color_rejects_non_hex_digits() {
        for s in [
            "#+ab",
            "#+abcde",
            "#+abcdef0",
            "#-ab",
            "#+",
            "#é1",
            "#ab é",
            "#",
            "#12345",
        ] {
            assert!(parse_color(s) == Color::INVALID, "{s}");
        }
        assert!(parse_color("#0ab") == Color::from_argb32(0xff00aabb));
        assert!(parse_color("#80FF0000") == Color::from_argb32(0x80ff0000));
    }

    #[test]
    fn input_method_keyboard_rectangle_is_no_cursor_rectangle() {
        // Qt 6.12's text-input-v3 backend (QWaylandTextInputv3::keyboardRect) hands out the
        // cursor rectangle as keyboardRectangle; there is no input panel, so it stays empty.
        let mut rt = crate::runtime::Runtime::new(None);
        let root = rt
            .load_str(
                "import QtQuick\nItem {\n property var k: Qt.inputMethod.keyboardRectangle\n \
                 property string out: [k.x, k.y, k.width, k.height, Qt.inputMethod.visible,\n \
                 Qt.inputMethod.cursorRectangle.x].join()\n}",
            )
            .unwrap();
        rt.settle();
        let g = crate::inputmethod::Geometry {
            cursor: [10.0, 20.0, 1.0, 16.0],
            ..Default::default()
        };
        crate::inputmethod::set_geometry(&mut rt.world.borrow_mut(), g);
        rt.settle();
        assert_eq!(rt.get(root, "out").to_js_string(), "0,0,0,0,false,10");
    }

    #[test]
    fn regexp_overrides_and_legacy_canonicalize() {
        let cases = [
            (
                "var f = [1]; f.toString = function () { return 'F' }; return [f + '', String(f), [f].join()].join()",
                "F,F,F",
            ),
            (
                "var r = /x/; r.toString = function () { return 'R' }; return r.toString() + String(r)",
                "RR",
            ),
            (
                "var r = /a/; Object.defineProperty(r, 'exec', { value: function () { return null } }); return r.test('a')",
                "true",
            ),
            (
                "return RegExp.prototype.test.call({ exec: function () { return {} } }, 'x')",
                "THROW Type error",
            ),
            (
                "var n = 0, r = /a/; r.lastIndex = { valueOf() { n++; return 0 } }; r.exec('a'); return n",
                "1",
            ),
            (
                "return [/\\u017f/i.test('s'), /[a-z]/i.test('\\u212a'), /\\w/i.test('\\u017f'), /\\u017f/iu.test('s')].join()",
                "false,false,false,true",
            ),
            (
                "return [new RegExp('\\\\u{41}').test('A'), new RegExp('\\\\u{41}', 'u').test('A')].join()",
                "false,true",
            ),
        ];
        for (src, want) in cases {
            assert_eq!(ev(src), want, "{src}");
        }
    }

    #[test]
    fn split_limit_is_to_uint32() {
        assert_eq!(
            ev("return JSON.stringify('a,b'.split(',', -1))"),
            r#"["a","b"]"#
        );
        assert_eq!(
            ev("return JSON.stringify('a,b,c'.split(',', 4294967298))"),
            r#"["a","b"]"#
        );
        assert_eq!(
            ev("return JSON.stringify('a,b'.split(',', 1.9))"),
            r#"["a"]"#
        );
        assert_eq!(ev("return JSON.stringify('a,b'.split(',', NaN))"), "[]");
        assert_eq!(
            ev("return JSON.stringify('a,b'.split(',', Infinity))"),
            "[]"
        );
        assert_eq!(
            ev("return JSON.stringify('a1b'.split(/\\d/, -1))"),
            r#"["a","b"]"#
        );
    }

    #[test]
    fn substr_huge_length_is_clamped() {
        assert_eq!(ev("return 'abc'.substr(1, Infinity)"), "bc");
        assert_eq!(ev("return 'abc'.substr(1, 1e20)"), "bc");
        assert_eq!(ev("return 'abc'.substr(-2, 1.9)"), "b");
        assert_eq!(
            ev("return 'abc'.substr(1, NaN) + '|' + 'abc'.substr(1, -Infinity)"),
            "|"
        );
        assert_eq!(
            ev("return 'abc'.substr(5, Infinity) + '|' + 'abc'.substr(-Infinity)"),
            "|abc"
        );
    }

    // Expected values from V8 (node 22).
    #[test]
    fn string_positions_match_v8() {
        let cases = [
            ("'abc'.endsWith('b', 2)", "true"),
            ("'abc'.endsWith('c', 9)", "true"),
            ("'abc'.includes('a', 1)", "false"),
            ("'abc'.includes('c', -5)", "true"),
            ("'abc'.lastIndexOf('c', 1)", "-1"),
            ("'abc'.lastIndexOf('', 1)", "1"),
            ("'abcabc'.lastIndexOf('bc', 3)", "1"),
            ("'abc'.lastIndexOf('a', NaN)", "0"),
            ("'abc'.startsWith('c', -1)", "false"),
            ("'abc'.indexOf('a', -1)", "0"),
            ("'aé😀b'.indexOf('b', 1)", "4"),
            ("'aé😀b'.lastIndexOf('é', 3)", "1"),
            ("'aé😀b'.endsWith('é', 2)", "true"),
            ("'aé😀b'.slice(1, 4)", "é😀"),
            ("'abc'.charCodeAt(1.7)", "98"),
            ("'é'.padStart(3, 'ab')", "abé"),
            ("'x'.padEnd(4, '😀')", "x😀\u{FFFD}"),
            ("[1, 2, 1].lastIndexOf(1, 1)", "0"),
            ("[1, 2, 1].lastIndexOf(1, -2)", "0"),
            ("[1, 2, 1].lastIndexOf(1, -4)", "-1"),
            ("[1, NaN].includes(NaN) + ',' + [1, 2].at(-1)", "true,2"),
            ("['b', 'a', 'B'].sort().join()", "B,a,b"),
            ("('aé' < 'a😀') + ',' + ('\u{FFFF}' < '😀')", "true,false"),
            (
                "'abc'.replace('b', \"$'\") + 'abc'.replace('b', '$`')",
                "accaac",
            ),
            ("'aé'.replaceAll('', '-')", "-a-é-"),
            ("'é😀'.replace(/(?:)/g, '.').length", "7"),
            ("'é😀'.replace(/(?:)/gu, '.')", ".é.😀."),
        ];
        for (src, want) in cases {
            assert_eq!(ev(&format!("return {src}")), want, "{src}");
        }
    }

    /// `new` of a Map/Set subclass (or Reflect.construct with another NewTarget) makes the
    /// object from NewTarget before AddEntriesFromIterable reads its adder, and `set`/`add`
    /// store -0 as +0 (ECMA-262 §24.1.1.1 step 2, §24.2.2.1 step 2, §24.1.3.9 step 5,
    /// §24.2.3.1 step 4). Expected values worked out from ECMA-262; Qt 6.12's V4 agrees on
    /// the first case but reads another NewTarget's `prototype` only after the constructor
    /// runs, so `Reflect.construct(Map, entries, F)` calls %Map.prototype.set% there.
    #[test]
    fn map_set_new_target_and_negative_zero() {
        let case = include_str!("../../rigen/check/cases/coll_new_target_zero.js");
        assert_eq!(
            ev(&format!("{case}\nreturn result")),
            r#"["M1,M3,S5,S7",true,2,"b",true,2,true,true,true,true,true,true,"true,true,true",true,1,true]"#
        );
        let case = include_str!("../../rigen/check/cases/coll_new_target_spec.js");
        assert_eq!(
            ev(&format!("{case}\nreturn result")),
            r#"["F9,G5",true,"c",true,true]"#
        );
    }

    #[test]
    fn map_set_same_value_zero_and_order() {
        let src = "var m = new Map([[1, 'a'], ['1', 'b']]); m.set(-0, 'z'); m.set(NaN, 'n'); \
            var o = {}; m.set(o, 'o'); m.delete(1); m.set(1, 'c'); m.set(0, 'zz'); \
            var s = new Set([NaN, NaN, 0, -0, '0', o, o]); s.delete(0); s.add(0); \
            return [m.get('1'), m.get(+0), m.get(NaN), m.get(o), m.get({}), m.size, \
                JSON.stringify([...m.keys()].map(String)), s.size, s.has(-0), s.has(o), \
                JSON.stringify([...s].map(String))].join('|')";
        assert_eq!(
            ev(src),
            r#"b|zz|n|o||5|["1","0","NaN","[object Object]","1"]|4|true|true|["NaN","0","[object Object]","0"]"#
        );
        assert_eq!(
            ev(
                "var m = new Map(); m.set(1, 1); m.clear(); m.set(2, 2); return m.has(1) + ',' + m.get(2) + ',' + m.size"
            ),
            "false,2,1"
        );
    }

    #[test]
    fn objects_inherit_object_prototype() {
        assert_eq!(ev("return ({}).toString()"), "[object Object]");
        assert_eq!(ev("return 'toString' in {}"), "true");
        assert_eq!(
            ev("return ({ a: 1 }).hasOwnProperty('a') + ',' + ({}).hasOwnProperty('toString')"),
            "true,false"
        );
        assert_eq!(
            ev("return Object.prototype.toString.call([]) + Object.prototype.toString.call(null)"),
            "[object Array][object Null]"
        );
        assert_eq!(
            ev(
                "function C() {} return Object.prototype.isPrototypeOf(new C()) && new C() instanceof Object"
            ),
            "true"
        );
        assert_eq!(
            ev("return ({ toString: function () { return 'own' } }) + ''"),
            "own"
        );
        assert_eq!(
            ev("var b = Object.create(null); return ('toString' in b) + ',' + typeof b.valueOf"),
            "false,undefined"
        );
        assert_eq!(
            ev("return typeof (1).hasOwnProperty + ',' + (1).valueOf() + ',' + true.valueOf()"),
            "function,1,true"
        );
        assert_eq!(ev("return Object.keys(Object.prototype).length"), "0");
        let it = Interp::default();
        let shared = Interp::with_global(Box::new(crate::eval::NoHost), it.global.clone());
        assert!(Rc::ptr_eq(&it.object_proto, &shared.object_proto));
    }

    #[test]
    fn array_length_assignment_converts_to_primitive() {
        // ArraySetLength (ECMA-262 §10.4.2.4) takes ToNumber of the value, so an object
        // converts through valueOf/toString; Qt 6.12: "1:1", 2, RangeError.
        assert_eq!(
            ev(
                "var a = [1, 2, 3]; a.length = { valueOf: function () { return 1 } }; return a.length + ':' + a"
            ),
            "1:1"
        );
        assert_eq!(
            ev(
                "var a = [1, 2, 3]; a.length = { toString: function () { return '2' }, valueOf: undefined }; return a.length"
            ),
            "2"
        );
        assert_eq!(
            ev(
                "var a = [1, 2, 3]; try { a.length = { valueOf: function () { return 1.5 } } } catch (e) { return e.name } return 'none'"
            ),
            "RangeError"
        );
    }

    #[test]
    fn legacy_regexp_statics_follow_every_successful_match() {
        // TC39 proposal-regexp-legacy-features: every successful RegExpBuiltinExec (also those
        // of replace, search and split) updates them, a failed one leaves them (QV4 clears
        // them on a failed exec, and its replace/search/split never update them).
        assert_eq!(
            ev("/(a)/.exec('xa'); /z/.exec('a'); return RegExp.$1 + RegExp.leftContext"),
            "ax"
        );
        assert_eq!(
            ev("'q1w2'.replace(/(\\d)/g, '-'); return RegExp.lastMatch + RegExp.leftContext"),
            "2q1w"
        );
        assert_eq!(
            ev("'abc'.search(/b/); return RegExp.lastMatch + RegExp.rightContext"),
            "bc"
        );
        assert_eq!(
            ev("'a,b;c'.split(/[,;]/); return RegExp.lastMatch + RegExp.input"),
            ";a,b;c"
        );
        assert_eq!(
            ev("var x; 'ab'.replace(/(b)/, function () { x = RegExp.$1; return ''; }); return x"),
            "b"
        );
        assert_eq!(ev("'ab'.replace('b', ''); return RegExp.input"), "");
        assert_eq!(
            ev("/(b)/.exec('\u{1F600}b'); return RegExp.leftContext + RegExp.$1"),
            "\u{1F600}b"
        );
    }

    #[test]
    fn regexp_compile_and_flags_follow_the_spec() {
        // QV4's compile returns undefined, keeps lastIndex and takes a RegExp pattern with
        // flags; Annex B §B.2.4.1 returns `this`, resets lastIndex and throws a TypeError.
        assert_eq!(
            ev("var r = /a/g; r.lastIndex = 3; return [r.compile('b') === r, r.lastIndex].join()"),
            "false,3"
        );
        assert_eq!(
            ev("try { /a/.compile(/b/, 'g'); return 'no' } catch (e) { return e.name; }"),
            "no"
        );
        assert_eq!(
            ev("return [new RegExp('a', 'sg').flags, /a/s.dotAll, RegExp.prototype.dotAll].join()"),
            "gs,true,"
        );
        assert_eq!(
            ev("var r = new RegExp('a\\nb/[/]'); return [r.source, r.test('a\\nb//')].join()"),
            "a\nb\\/[\\/],true"
        );
    }

    #[test]
    fn builtin_prototypes_are_real_objects() {
        // Each line is what Qt 6.12's V4 prints for the same program.
        let cases = [
            (
                "typeof Array.prototype + '/' + typeof String.prototype + '/' + typeof Function.prototype",
                "object/object/function",
            ),
            (
                "typeof Number.prototype + '/' + typeof Boolean.prototype",
                "object/object",
            ),
            (
                "typeof [].slice + '/' + typeof ''.trim + '/' + typeof (function () {}).call",
                "function/function/function",
            ),
            (
                "(function () { return [].slice.call(arguments).length + ',' + Array.prototype.slice.call(arguments, 1).join('+') })(1, 2, 3)",
                "3,2+3",
            ),
            ("Array.prototype.slice === [].slice", "true"),
            ("String.prototype.trim.call('  x  ') + '|'", "x|"),
            (
                "Array.prototype.join.call({ length: 2, 0: 'a', 1: 'b' }, '-')",
                "a-b",
            ),
            (
                "Array.prototype.map.call('ab', function (c) { return c + c }).join()",
                "aa,bb",
            ),
            (
                "[] instanceof Array && Array.prototype.isPrototypeOf([]) && (function () {}) instanceof Function",
                "true",
            ),
            (
                "Function.prototype.call.call(function () { return this.v }, { v: 7 })",
                "7",
            ),
            ("typeof Function", "function"),
            (
                "Function('a', 'b', 'return a + b')(2, 3) + new Function('return 42')()",
                "47",
            ),
            ("'slice' in []", "true"),
            (
                "Object.keys(Array.prototype).length + ',' + Object.keys(String.prototype).length",
                "0,0",
            ),
            (
                "(function () { Array.prototype.myx = function () { return this.length }; var r = [1, 2].myx(); delete Array.prototype.myx; return r + typeof [].myx })()",
                "2undefined",
            ),
            (
                "(function () { String.prototype.twice = function () { return this + this }; var r = 'ab'.twice(); delete String.prototype.twice; return r })()",
                "abab",
            ),
            ("Number.prototype.toFixed.call(1.234, 1)", "1.2"),
            ("Boolean.prototype.toString.call(true)", "true"),
            (
                "Array.prototype.constructor === Array && [].constructor === Array && ''.constructor === String && (function () {}).constructor === Function",
                "true",
            ),
            ("Array.prototype.push.call({ length: 1 }, 'z')", "2"),
            (
                "(function () { var o = { length: 3, 0: 'c', 1: 'a', 2: 'b' }; Array.prototype.sort.call(o); return o[0] + o[1] + o[2] + o.length })()",
                "abc3",
            ),
            (
                "(function () { var o = { length: 2, 0: 1, 1: 2 }; return Array.prototype.pop.call(o) + ',' + o.length + ',' + (1 in o) })()",
                "2,1,false",
            ),
            ("Function.prototype()", "undefined"),
            (
                "Array.prototype.length + ',' + Array.isArray(Array.prototype)",
                "0,true",
            ),
            (
                "Object.prototype.toString.call(Array.prototype)",
                "[object Array]",
            ),
            ("[1, 2, 3].slice.call([4, 5, 6], 1).join()", "5,6"),
        ];
        for (src, want) in cases {
            assert_eq!(ev(&format!("return {src}")), want, "{src}");
        }
        for src in [
            "String.prototype.trim.call(null)",
            "Array.prototype.slice.call(null)",
            "Number.prototype.toFixed.call('1')",
            "Function.prototype.toString.call({})",
        ] {
            assert!(
                ev(&format!("try {{ {src} }} catch (e) {{ return e.name }}")) == "TypeError",
                "{src}"
            );
        }
        // The parameters and the body cannot close the function early (CreateDynamicFunction
        // parses each on its own).
        for src in [
            "Function('a) {}); globalThis.leak = 1; (function (', '')",
            "Function('', '}); globalThis.leak = 1; (function () {')",
        ] {
            assert_eq!(
                ev(&format!(
                    "try {{ {src} }} catch (e) {{ return e.name + typeof leak }}"
                )),
                "SyntaxErrorundefined",
                "{src}"
            );
        }
    }

    #[test]
    fn functions_have_length_and_name_and_regexp_a_prototype() {
        // The case pins what Qt 6.12's V4 computes for it.
        let case = include_str!("../../rigen/check/cases/function_names.js");
        let pin = case
            .lines()
            .find_map(|l| l.strip_prefix("// v4: "))
            .unwrap();
        assert_eq!(ev(&format!("{case}\nreturn result")), pin);
        let cases = [
            (
                "return (function () {}).bind(null).name + '|' + function f() {}.bind(null).name",
                "__bound function__|__bound function__",
            ),
            (
                "var re = /a/g; re.compile('b', 'i'); return re.source + re.flags + re.lastIndex + re.test('B')",
                "bi0true",
            ),
            (
                "try { RegExp.prototype.test.call({}, 'a') } catch (e) { return e.name }",
                "TypeError",
            ),
            ("return RegExp.prototype.isPrototypeOf(/a/)", "true"),
            (
                "return /a/gimy.flags + '|' + /a/.ignoreCase + '|' + RegExp.prototype.sticky",
                "gimy|false|undefined",
            ),
            (
                "class K { static name() { return 'own' } } return K.name()",
                "own",
            ),
            ("var o = {get g() { return 1 }}; return o.g", "1"),
        ];
        for (src, want) in cases {
            assert_eq!(ev(src), want, "{src}");
        }
    }

    /// Array elements and built-ins' string arguments convert with ToString (ECMA-262 §7.1.17),
    /// running user `toString`/`valueOf`/`Symbol.toPrimitive`; cycles join as `""` as in V8.
    #[test]
    fn elements_and_arguments_stringify_through_to_primitive() {
        let j = "var j = { toString: function () { return 'j' } };";
        let cases = [
            ("return [j].join()", "j"),
            ("return [new URLSearchParams('q=1')].join()", "q=1"),
            ("return [j, [j, null], undefined, , 2].join(j)", "jjj,jjj2"),
            (
                "return String([j]) + [j] + `${[j]}` + [j].toString() + ([j] == 'j')",
                "jjjjtrue",
            ),
            (
                "var v = Object.create(null); v.valueOf = function () { return 5 }; return [v].join()",
                "5",
            ),
            (
                "var p = {}; p[Symbol.toPrimitive] = function (h) { return h }; return [p] + [p].join()",
                "stringstring",
            ),
            (
                "try { [{ toString: function () { throw 7 } }].join() } catch (e) { return e }",
                "7",
            ),
            (
                "var m = [1, { toString: function () { m[2] = 'z'; return 'x' } }, 3]; return m.join()",
                "1,x,z",
            ),
            (
                "var a = [1]; Object.defineProperty(a, 'join', { value: function () { return 'J' } }); return String(a)",
                "J",
            ),
            (
                "return Array.prototype.toString.call({ join: function () { return 'k' } })",
                "k",
            ),
            (
                "return Array.prototype.toString.call({ join: 1 })",
                "[object Object]",
            ),
            ("var c = [1]; c.push(c); return c.join() + '|' + c", "1,|1,"),
            (
                "return [{ toString: function () { return 'b' } }, j].sort().join()",
                "b,j",
            ),
            (
                "try { [j, { toString: function () { throw 8 } }].sort() } catch (e) { return e }",
                "8",
            ),
            ("return [j, [j]].toLocaleString()", "j,j"),
            ("var c = [j]; c.push(c); return c.toLocaleString()", "j,"),
            (
                "return 'a'.concat([j], j) + 'ja'.indexOf(j) + 'xjx'.split(j) + 'aj'.replace(j, j)",
                "ajj0x,xaj",
            ),
            (
                "return 'q'.padEnd(3, j) + encodeURIComponent([j, ' ']) + JSON.parse({ toString: function () { return '[1]' } })",
                "qjjj%2C%201",
            ),
            (
                "return '' + Object.fromEntries([[j, 1]]).j + new RegExp(j).test([j]) + 'j'.startsWith(j)",
                "1truetrue",
            ),
            ("return 'aja'.replaceAll(j, [j, j])", "aj,ja"),
        ];
        for (src, want) in cases {
            assert_eq!(ev(&format!("{j} {src}")), want, "{src}");
        }
    }

    fn ev(src: &str) -> String {
        let body = crate::ir::lower_program(src).unwrap();
        let mut it = Interp::default();
        let g = it.global.clone();
        match it.run(&body, &g) {
            Ok(v) => v.to_js_string(),
            Err(Ex::Throw(e)) => format!("THROW {}", it.get(&e, "message").unwrap().to_js_string()),
            Err(Ex::Short) => "SHORT".into(),
        }
    }

    /// Qt documents `Qt.btoa`/`Qt.atob` as taking array-likes, but QV4 only reads the elements
    /// of a real Array: a typed array, an arguments object or `{length, 0: ...}` encodes as
    /// empty. They are read as array-likes here, with the same per-element rules.
    #[test]
    fn qt_base64_reads_every_array_like() {
        let t = |s: &str| {
            ev(&format!(
                "var v = {s}; return v instanceof ArrayBuffer ? Array.from(new Uint8Array(v)).join() : v"
            ))
        };
        assert_eq!(t("Qt.btoa(new Uint8Array([104, 105]))"), "97,71,107,61");
        assert_eq!(
            t("Qt.btoa(new Uint8Array([1, 104, 105, 2]).subarray(1, 3))"),
            "97,71,107,61"
        );
        assert_eq!(t("Qt.btoa(new Float64Array([104.7, 105]))"), "97,71,107,61");
        assert_eq!(
            t("Qt.btoa(new Int16Array([300]))"),
            "THROW Invalid character"
        );
        assert_eq!(
            t("(function () { return Qt.btoa(arguments) })(104, 105)"),
            "97,71,107,61"
        );
        assert_eq!(t("Qt.btoa({ length: 2, 0: 104, 1: 'i' })"), "97,71,107,61");
        assert_eq!(
            t("Qt.btoa({ length: { valueOf() { return 1 } }, 0: 65 })"),
            "81,81,61,61"
        );
        assert_eq!(t("Qt.btoa(new String('hi'))"), "97,71,107,61");
        assert_eq!(t("Qt.btoa({ length: 1e12 })"), "THROW Invalid character");
        assert_eq!(t("Qt.atob(new Uint8Array([97, 71, 107]))"), "104,105");
        assert_eq!(t("Qt.btoa(new DataView(new ArrayBuffer(2)))"), "");
        assert_eq!(t("Qt.btoa({})"), "");
    }

    /// GetTemplateObject (ECMA-262 §13.2.8.4) freezes the strings and `raw` arrays. Qt 6.12
    /// reports them frozen but still accepts element writes and `push`; the spec rejects them.
    #[test]
    fn string_object_indices_and_length_are_fixed() {
        let t = |s: &str| ev(&format!("return (function () {{ {s} }})()"));
        assert_eq!(
            t(
                "var s = new String('ab'), d = Object.getOwnPropertyDescriptor(s, '0'), \
               l = Object.getOwnPropertyDescriptor(s, 'length'); \
               return [d.value, d.writable, d.enumerable, d.configurable, l.value, l.writable, \
               l.enumerable, l.configurable, Object.getOwnPropertyNames(s), \
               Reflect.set(s, '0', 'z'), Reflect.deleteProperty(s, 'length')].join()"
            ),
            "a,false,true,false,2,false,false,false,0,1,length,false,false"
        );
        assert_eq!(
            t("'use strict'; var s = new String('ab'), r = []; \
               for (var g of [() => { s[0] = 'z' }, () => { s.length = 0 }, () => { delete s[1] }]) \
               { try { g(); r.push('ok') } catch (e) { r.push(e.name) } } return r.join() + s"),
            "TypeError,TypeError,TypeErrorab"
        );
    }

    /// Typed array elements are writable, enumerable, configurable data properties that cannot
    /// be redefined or frozen (ECMA-262 §10.4.5); `delete` of one answers true and leaves it, as
    /// in Qt 6.12. Spec over Qt 6.12 in the rest: its elements are non-configurable, a strict
    /// write past the end throws and `freeze` is silent: `{..,"configurable":false} | undefined |
    /// 0,1 | true | true:7 | true | false | TypeError | ok | 7,2 | true:true`.
    #[test]
    fn typed_array_elements_are_exotic_own_properties() {
        let t = |s: &str| ev(&format!("return (function () {{ {s} }})()"));
        assert_eq!(
            t("var a = new Uint8Array([1, 2]), r = []; \
               function c(f) { try { r.push(String(f())) } catch (e) { r.push(e.name) } } \
               c(() => JSON.stringify(Object.getOwnPropertyDescriptor(a, 0))); \
               c(() => Object.getOwnPropertyDescriptor(a, 5)); \
               c(() => Reflect.ownKeys(a).join()); \
               c(() => delete a[0]); \
               c(() => Reflect.defineProperty(a, 0, {value: 7}) + ':' + a[0]); \
               c(() => Reflect.defineProperty(a, 0, {value: 7, configurable: false})); \
               c(() => Reflect.defineProperty(a, 9, {value: 1})); \
               c(() => { 'use strict'; a[9] = 1; return 'ok' }); \
               c(() => { 'use strict'; delete a[0]; return 'ok' }); \
               c(() => Object.freeze(a)); \
               c(() => Object.isFrozen(Object.freeze(new Uint8Array(0))) + ':' + \
                   Object.isSealed(a)); \
               return r.join(' | ')"),
            "{\"value\":1,\"writable\":true,\"enumerable\":true,\"configurable\":true} | \
             undefined | 0,1 | true | true:7 | false | false | ok | ok | TypeError | \
             true:false"
        );
    }

    #[test]
    fn legacy_accessors_and_generators_follow_attributes_and_strictness() {
        let t = |s: &str| ev(&format!("return (function () {{ {s} }})()"));
        assert_eq!(
            t(
                "var o = Object.preventExtensions({}), f = Object.freeze({a: 1}), r = []; \
               for (var g of [() => o.__defineGetter__('x', () => 1), \
               () => f.__defineSetter__('a', () => 1)]) \
               { try { g(); r.push('ok') } catch (e) { r.push(e.name) } } \
               var p = {}; p.__defineGetter__('y', () => 2); \
               var d = Object.getOwnPropertyDescriptor(p, 'y'); \
               return r.join() + d.enumerable + d.configurable + p.y"
            ),
            "TypeError,TypeErrortruetrue2"
        );
        assert_eq!(
            t(
                "Object.defineProperty(Function.prototype, 'name', { enumerable: true }); \
               function f() {} var a = [], b = []; for (var k in f) a.push(k); delete f.name; \
               for (var k in f) b.push(k); \
               Object.defineProperty(Function.prototype, 'name', { enumerable: false }); \
               return a.length + ':' + b.join()"
            ),
            "0:name"
        );
        assert_eq!(
            t(
                "function* g() { 'use strict'; yield 1; Object.freeze({}).x = 1; yield 2 } \
               var i = g(), r = [i.next().value]; \
               try { i.next(); r.push('ok') } catch (e) { r.push(e.name) } \
               return r.join() + String.raw`a${1}\\n`"
            ),
            "1,TypeErrora1\\n"
        );
    }

    #[test]
    fn template_objects_are_frozen() {
        let t = |s: &str| ev(&format!("return (function () {{ {s} }})()"));
        assert_eq!(
            t(
                "function tag(s) { return s } var s = tag`a${1}b`; s[0] = 'z'; s.raw[1] = 'z'; \
               s.push('q'); return s.join() + s.raw.join() + s.length + Object.isFrozen(s) + \
               Object.isFrozen(s.raw)"
            ),
            "z,b,qa,z3truetrue"
        );
        assert_eq!(
            t(
                "'use strict'; function tag(s) { return s } var s = tag`a`, r = []; \
               for (var g of [() => { s[0] = 'z' }, () => { s.raw[0] = 'z' }, () => { s[1] = 1 }]) \
               { try { g(); r.push('ok') } catch (e) { r.push(e.name) } } return r.join()"
            ),
            "ok,ok,TypeError"
        );
    }

    /// Where Qt 6.12's V4 departs from ECMA-262 the runtime follows the spec; the rest of
    /// `freeze` counts the array's cells once and by the words of the hole bits: asked
    /// for every hole and counted hole by hole, 40 000 holes at the end took seconds.
    #[test]
    fn freezing_an_array_that_ends_in_many_holes_is_not_quadratic() {
        let start = std::time::Instant::now();
        assert_eq!(
            ev("var a = [1]; a.length = 40000; Object.freeze(a); \
                return Object.isFrozen(a) + '|' + a.length + '|' + a[0]"),
            "true|40000|1"
        );
        let took = start.elapsed();
        assert!(
            took < std::time::Duration::from_millis(500),
            "freeze of 40000 holes took {took:?}"
        );
    }

    /// Object reflection is pinned against V4 in `rigen/check/cases/object_reflection.js`.
    #[test]
    fn integrity_levels_follow_ecma262_where_v4_does_not() {
        let t = |body: &str| {
            ev(&format!(
                "try {{ return (function () {{ {body} }})() }} catch (e) {{ return 'THROW ' + e.name }}"
            ))
        };
        // An array is V4's: `push` extends a frozen one, `sort` sorts it, `a[0] = 5` stores,
        // and so does a write to a read-only element (`frozen*.qml` of `builtins_b1_qt_equiv`).
        assert_eq!(t("var a = Object.freeze([1]); a.push(2); return a"), "1,2");
        assert_eq!(
            t("var a = Object.freeze([2, 1]); a.sort(); return a"),
            "1,2"
        );
        assert_eq!(
            t("var a = Object.freeze([1]); a[0] = 5; a[1] = 6; return a + '|' + a.length"),
            "5|1"
        );
        assert_eq!(
            t(
                "var a = [1, 2, 3]; Object.defineProperty(a, '1', {value: 9, writable: false}); \
               a[1] = 5; return a + '|' + Object.getOwnPropertyDescriptor(a, '1').writable"
            ),
            "1,5,3|false"
        );
        // §7.3.16 TestIntegrityLevel: a non-extensible object without properties is sealed.
        assert_eq!(
            t("var o = Object.preventExtensions({a: 1}); delete o.a; \
               return Object.isSealed(o) + ',' + Object.isFrozen(o)"),
            "true,true"
        );
        // V4's order of a function's own keys, not §10.2.9 and §10.2.10's.
        assert_eq!(
            t("return Object.getOwnPropertyNames(function f(a) {})"),
            "prototype,name,length"
        );
        // Annex B §B.2.2.1.2: a primitive `__proto__` is ignored, where V4 throws.
        assert_eq!(
            t("var o = {}; o.__proto__ = 5; return Object.getPrototypeOf(o) === Object.prototype"),
            "true"
        );
        assert_eq!(
            t("'use strict'; function f() {} delete f.name; \
               try { f.name = 1 } catch (e) { return e.name + ':' + JSON.stringify(f.name) } return 'none'"),
            "TypeError:\"\""
        );
        assert_eq!(
            t(
                "function g(a) {} var n = delete g.name, l = delete g.length; \
               return [n, l, JSON.stringify(g.name), g.length, g.hasOwnProperty('name'), \
               g.hasOwnProperty('length'), Object.getOwnPropertyNames(g)].join()"
            ),
            "true,true,\"\",0,false,false,prototype"
        );
        assert_eq!(
            t(
                "function c(f) { try { return String(f()) } catch (e) { return e.name } } \
               return c(function () { 'use strict'; var p = {get g() { return 1 }}; p.g = 2; return 1 })"
            ),
            "TypeError"
        );
        assert_eq!(
            t("'use strict'; var r = []; for (var v of ['abc', 1, true]) \
               { try { v.x = 1; r.push('ok') } catch (e) { r.push(e.name) } } \
               try { 'abc'[0] = 'z' } catch (e) { r.push(e.name) } return r.join()"),
            "TypeError,TypeError,TypeError,TypeError"
        );
        assert_eq!(
            t("var s = 'abc'; s.x = 1; s.length = 0; return s.x + ':' + s.length"),
            "undefined:3"
        );
        assert_eq!(
            t(
                "'use strict'; var seen; Object.defineProperty(String.prototype, 'w', \
               { set(v) { seen = typeof this + v }, configurable: true }); \
               'abc'.w = 1; delete String.prototype.w; return seen"
            ),
            "string1"
        );
    }

    #[test]
    fn watchdog_interrupts_one_runaway_evaluation() {
        use std::time::Duration;
        crate::watchdog::set_timeout(Some(Duration::from_millis(200)));
        let body =
            crate::ir::lower_program("var s = 0; for (var i = 0; i < 1000; i++) s += i; return s")
                .unwrap();
        let mut it = Interp::default();
        let g = it.global.clone();
        // Many short evaluations together outlast the timeout (in this thread's CPU time, which
        // it counts); none of them alone does.
        let cpu = crate::watchdog::thread_cpu_time;
        let t0 = cpu();
        while cpu() - t0 < Duration::from_millis(600) {
            let r = it.run(&body, &g);
            assert!(matches!(r, Ok(Value::Num(n)) if n == 499500.0), "{r:?}");
        }
        // V4 keeps throwing while interrupted: a `catch` cannot keep the loop alive.
        let spin =
            crate::ir::lower_program("for (;;) { try { for (;;) {} } catch (e) {} }").unwrap();
        let t0 = cpu();
        let Err(Ex::Throw(e)) = it.run(&spin, &g) else {
            panic!("runaway loop not stopped")
        };
        let took = cpu() - t0;
        assert_eq!(it.get(&e, "message").unwrap().to_js_string(), "Interrupted");
        assert_eq!(it.get(&e, "name").unwrap().to_js_string(), "Error");
        assert!(
            took >= Duration::from_millis(200) && took < Duration::from_secs(2),
            "{took:?}"
        );
        // The next evaluation is not interrupted.
        assert!(matches!(it.run(&body, &g), Ok(Value::Num(_))));
        assert_eq!(crate::eval::js_depth(), 0);
        crate::watchdog::set_timeout(Some(crate::watchdog::DEFAULT_TIMEOUT));
    }

    #[test]
    fn huge_string_lengths_throw_range_error() {
        let catch = |e: &str| {
            format!(
                "try {{ return {e} }} catch (e) {{ return (e instanceof RangeError) + ' ' + e.message }}"
            )
        };
        let len = "true Invalid string length";
        for src in [
            "'a'.repeat(1e300)",
            "'ab'.repeat(2 ** 28)",
            "'a'.padStart(1e300)",
            "'a'.padEnd(2 ** 30, 'xy')",
            "'a'.padStart(Infinity)",
        ] {
            assert_eq!(ev(&catch(src)), len, "{src}");
        }
        assert_eq!(ev(&catch("'a'.repeat(-1)")), "true Invalid count value");
        assert_eq!(
            ev(&catch("'a'.repeat(Infinity)")),
            "true Invalid count value"
        );
        assert_eq!(ev(&catch("'a'.repeat(-1.5)")), "true Invalid count value");
        // V8 results for the in-range edges.
        assert_eq!(
            ev(
                "return ''.repeat(2 ** 40) + '|' + 'ab'.repeat(2.9) + '|' + 'a'.repeat(NaN) + '|' + 'a'.repeat(-0.5)"
            ),
            "|abab||"
        );
        assert_eq!(
            ev(
                "return 'a'.padStart(1e300, '') + '|' + 'a'.padStart(NaN) + '|' + 'a'.padEnd(3.7, 'xy') + '|' + 'a'.padStart(-5)"
            ),
            "a|a|axy|a"
        );
        assert_eq!(
            ev("return 'é'.repeat(3).length + ' ' + '\u{1F600}'.repeat(2).length"),
            "3 4"
        );
    }

    #[test]
    fn huge_array_lengths_throw_range_error() {
        let catch = |e: &str| {
            format!(
                "try {{ {e}; return 'ok' }} catch (e) {{ return (e instanceof RangeError) + ' ' + e.message }}"
            )
        };
        for (src, n) in [
            ("var a = []; a.length = 4294967296", "4294967296"),
            ("var a = []; a.length = -1", "-1"),
            ("var a = []; a.length = 1.5", "1.5"),
            ("new Array(-1)", "-1"),
            ("new Array(4294967296)", "4294967296"),
            ("Array(2.5)", "2.5"),
        ] {
            assert_eq!(ev(&catch(src)), format!("true {n} out of range"), "{src}");
        }
        // Every length up to 2^32 - 1 is one an array has (ECMA-262 §10.4.2): the holes take
        // no storage.
        for src in [
            "var a = []; a.length = 4294967295",
            "var a = []; a[1e9] = 1",
            "new Array(4294967295)",
        ] {
            assert_eq!(ev(&catch(src)), "ok", "{src}");
        }
        assert!(
            ev(&catch("Array.from({ length: 1e10 })")).starts_with("true Invalid array length: ")
        );
        // Keys past the last array index are plain properties, as in V8.
        assert_eq!(
            ev("var a = [1]; a[4294967295] = 2; return a.length + ' ' + a[4294967295]"),
            "1 2"
        );
        assert_eq!(
            ev("var a = [1, 2, 3]; a.length = '1'; a[4] = 5; return a.length + ' ' + a.join()"),
            "5 1,,,,5"
        );
        assert_eq!(
            ev(
                "return new Array(3).length + ' ' + Array.from({ length: 2.7 }).length + ' ' + Array.from({ length: -1 }).length + ' ' + Array('x').join()"
            ),
            "3 2 0 x"
        );
        assert_eq!(
            ev("var a = []; a[16777215] = 1; return a.length"),
            "16777216"
        );
    }

    #[test]
    fn string_length_is_bounded() {
        let catch = |e: &str| {
            ev(&format!(
                "try {{ return {e} }} catch (e) {{ return (e instanceof RangeError) + ' ' + e.message }}"
            ))
        };
        assert_eq!(catch("'a'.repeat(1e300)"), "true Invalid string length");
        assert_eq!(
            catch("'ab'.repeat(268435445)"),
            "true Invalid string length"
        );
        assert_eq!(catch("'a'.repeat(-1)"), "true Invalid count value");
        assert_eq!(catch("'a'.repeat(Infinity)"), "true Invalid count value");
        assert_eq!(
            catch("'x'.padStart(1e300, '0')"),
            "true Invalid string length"
        );
        assert_eq!(catch("'x'.padEnd(2 ** 40)"), "true Invalid string length");
        assert_eq!(
            ev("return ''.repeat(1e300) + '|' + 'ab'.repeat(2.9) + '|' + 'a'.repeat(NaN)"),
            "|abab|"
        );
        assert_eq!(
            ev("return 'x'.padStart(1e300, '') + 'x'.padEnd(NaN, '0') + '5'.padStart(3, '0')"),
            "xx005"
        );
        assert_eq!(ev("return 'a'.repeat(1 << 20).length"), "1048576");
    }

    #[test]
    fn internal_slots_are_not_properties() {
        // ECMA-262 §6.1.7.2: internal slots are not properties, so no key reaches them.
        let probe = |o: &str| {
            ev(&format!(
                "var o = {o}; var n = []; for (var k in o) n.push(k); \
                 return Object.keys(o).length + ',' + n.length + ',' + JSON.stringify(o)"
            ))
        };
        for o in ["new Date(5)", "new Map([[1, 2]])", "new Set([1])", "/a/g"] {
            assert_eq!(
                probe(o).split(',').take(2).collect::<Vec<_>>(),
                ["0", "0"],
                "{o}"
            );
        }
        assert_eq!(
            ev(
                "var d = new Date(5); delete d['\\x01time']; d['\\x01time'] = 7; return d.getTime()"
            ),
            "5"
        );
        assert_eq!(
            ev(
                "var m = new Map(); m['\\x01coll'] = 1; m.set('a', 1); return m.size + ',' + m.get('a')"
            ),
            "1,1"
        );
        assert_eq!(
            ev(
                "return JSON.stringify(new Map([[1, 2]])) + JSON.stringify(new Set([1])) + JSON.stringify(/a/)"
            ),
            "{}{}{}"
        );
        assert_eq!(
            ev("var e = new Error('m'); return Object.keys(e).length + ',' + String(e)"),
            "0,Error: m"
        );
        assert_eq!(
            ev("var o = {}; o['\\x01get x'] = () => 1; return o.x + ',' + Object.keys(o).length"),
            "undefined,1"
        );
    }

    #[test]
    fn accessors_are_structural() {
        let src = "var o = { get a() { return this.c * 2 }, set a(v) { this.c = v }, c: 1 }; o.a = 5; \
                   var r = Object.keys(o).join() + '|' + o.a + '|' + JSON.stringify(o) + '|' + ('a' in o); \
                   delete o.a; return r + '|' + o.a + '|' + ('a' in o) + '|' + Object.keys(o).join()";
        assert_eq!(ev(src), r#"a,c|10|{"a":10,"c":5}|true|undefined|false|c"#);
        // A data property replaces an accessor, keeping its enumeration position.
        assert_eq!(
            ev("var o = { get a() { return 1 }, b: 2, a: 3 }; return Object.keys(o).join() + o.a"),
            "a,b3"
        );
        assert_eq!(
            ev("var o = { a: 3, b: 2, get a() { return 1 } }; return Object.keys(o).join() + o.a"),
            "a,b1"
        );
        assert_eq!(
            ev("var o = { set a(v) { this.b = v } }; o.a = 4; return o.a + ',' + o.b"),
            "undefined,4"
        );
    }

    #[test]
    fn class_this_and_super_are_bindings_not_names() {
        let src = "class A { f() { return 'A' } get g() { return 'g' } static s() { return 'S' } } \
                   class B extends A { f() { return super.f() + 'B' } h() { return [1].map(() => super.f() + this.g)[0] } } \
                   var b = new B(); return b.f() + '|' + b.h() + '|' + B.s() + '|' + Object.keys(b).length + '|' + b.g";
        assert_eq!(ev(src), "AB|Ag|S|0|g");
    }

    #[test]
    fn deeply_nested_values_drop_without_recursion() {
        let arrays = "var a = 1; for (var i = 0; i < 100000; i++) a = [a]; a = 0";
        let objects = "var o = null; for (var i = 0; i < 100000; i++) o = { next: o }; o = 0";
        let closures =
            "var f = x => x; for (var i = 0; i < 100000; i++) { const g = f; f = y => g(y) } f = 0";
        assert_eq!(
            ev(&format!("{arrays}; {objects}; {closures}; return 'ok'")),
            "ok"
        );
    }

    #[test]
    fn deep_recursion_throws_range_error() {
        let catch = |e: &str| {
            format!(
                "try {{ {e} }} catch (e) {{ return (e instanceof RangeError) + ' ' + e.message }}"
            )
        };
        let want = "true Maximum call stack size exceeded";
        assert_eq!(
            ev(&catch("function f(n) { return f(n + 1) + 1 } f(0)")),
            want
        );
        assert_eq!(
            ev(&catch(
                "var o = { toString() { return 'x' + o } }; String(o)"
            )),
            want
        );
        assert_eq!(ev(&catch("function F() { return new F() } new F()")), want);
        assert_eq!(ev(&catch("var g = n => [n].map(g); g(0)")), want);
        assert_eq!(crate::eval::js_depth(), 0);
        // The stack unwinds fully: recursion within the limit still works afterwards.
        assert_eq!(
            ev("function f(n) { return n ? f(n - 1) + 1 : 0 } return f(500)"),
            "500"
        );
        // The limit counts frames (the script body is one), whatever native stack runs them.
        let depth = "var d = 0; function f() { d++; f() } try { f() } catch (e) {} return d";
        let want = (crate::eval::MAX_JS_DEPTH - 1).to_string();
        assert_eq!(ev(depth), want);
        let small = std::thread::Builder::new().stack_size(256 << 10);
        assert_eq!(
            small.spawn(move || ev(depth)).unwrap().join().unwrap(),
            want
        );
        assert_eq!(
            ev("var a = 1; for (var i = 0; i < 100; i++) a = [a]; return String(a)"),
            "1"
        );
    }

    #[test]
    fn one_element_arrays_convert_through_their_string() {
        // ToNumber(ToPrimitive(join)) (ECMA-262 §7.1.4, Qt 6.12 PySide6 oracle).
        assert_eq!(
            ev(
                "return [Math.abs([true]), Math.abs([false]), Math.abs([' ']), +[true], \
                Math.floor([[true]]), Math.abs(['-7']), Math.abs([[-2]]), 1 / Math.min([-0], 0), \
                Math.abs([null]), Math.abs([undefined]), Math.abs([]), Math.abs([1, 2])].join()"
            ),
            "NaN,NaN,0,NaN,NaN,7,2,Infinity,0,0,0,NaN"
        );
    }

    #[test]
    fn numeric_builtins_take_objects_through_to_primitive() {
        // ToNumber(ToPrimitive(x, number)) (ECMA-262 §7.1.4, Qt 6.12 PySide6 oracle).
        assert_eq!(
            ev(
                "function n(v) { return {valueOf: function () { return v; }}; }\
                var s = {valueOf: null, toString: function () { return '4'; }};\
                var log = [];\
                function t(k, v) { return {valueOf: function () { log.push(k); return v; }}; }\
                var r = [Math.abs(n(-3)), Math.floor(s), Math.pow(n(-3), 2), Math.hypot(n(3), 4),\
                    Math.min(n(-3), 0), Math.max(t('a', 1), t('b', NaN), t('c', 2)), log.join(''),\
                    isNaN(n(1)), isFinite(n(1 / 0)), 'abcdef'.slice(n(3)), 'abc'.charAt(n(1)),\
                    'abc'.charCodeAt(n(2)), (255).toString(n(16)), (1.5).toFixed(n(2)),\
                    [1, 2, 3, 4].slice(n(1), n(-1)).join('|'), new Date(2020, n(5), 1).getMonth(),\
                    new Date(n(0)).getTime(), [3, 1, 2].sort(function (x, y) { return n(x - y); })\
                    .join('|'), parseInt('ff', n(16)), 1 / Math.max(-0, 0), 1 / Math.min(0, -0),\
                    Math.hypot(NaN, -1 / 0), Qt.point(n(1), s).y,\
                    parseInt({toString: function () { return '12'; }}, n(8)),\
                    parseFloat({toString: function () { return '1.5'; }})];\
                return r.join();"
            ),
            "3,4,9,5,-3,NaN,abc,false,false,def,b,99,ff,1.50,2|3,5,0,1|2|3,255,Infinity,-Infinity,Infinity,4,10,1.5"
        );
        assert_eq!(
            ev("var r = [];\
                try { Math.abs({valueOf: function () { throw new Error('boom'); }}); }\
                catch (e) { r.push(e.message); }\
                try { Math.floor({valueOf: function () { return {}; }, toString: function () { return {}; }}); }\
                catch (e) { r.push(e.name); }\
                var seen = [];\
                try { Math.max({valueOf: function () { seen.push(1); throw 7; }}, {valueOf: function () { seen.push(2); return 0; }}); }\
                catch (e) { r.push(e, seen.length); }\
                return r.join();"),
            "boom,TypeError,7,1"
        );
        assert_eq!(
            ev("var a = [1, 2, 3, 4];\
                function cut(v) { return {valueOf: function () { a.length = 2; return v; }}; }\
                var r = [a.slice(cut(0), 4).join('|'), a.indexOf(2, cut(0))];\
                a = [1, 2, 3, 4]; r.push(a.lastIndexOf(2, cut(3)));\
                a = [1, 2, 3, 4]; r.push(a.fill(9, cut(1)).join('|'));\
                a = [1, 2, 3, 4]; r.push(a.splice(cut(1), 3).join('|'), a.join('|'));\
                return r.join();"),
            "1|2,1,1,1|9|9|9,2||,1"
        );
    }

    #[test]
    fn too_deep_conversions_throw_range_error() {
        // Qt 6.12 (PySide6 oracle): 9000 levels convert, 20000 throw `RangeError`.
        let nest = "function nest(n, f) { var a = []; for (var i = 0; i < n; i++) a = f(a); return a } \
                    var arr = a => [a], obj = a => ({ a: a });";
        let run = |n: u32, e: &str| {
            ev(&format!(
                "{nest} var d = nest({n}, arr), o = nest({n}, obj); \
                 try {{ return {e} }} catch (e) {{ return e.name + ': ' + e.message }}"
            ))
        };
        let thrown = "RangeError: Maximum call stack size exceeded";
        for e in [
            "String(d).length",
            "d.join('-').length",
            "d.toString().length",
            "('' + d).length",
            "`${d}`.length",
            "Number(d)",
        ] {
            assert_eq!(run(9000, e), "0", "{e}");
            assert_eq!(run(20000, e), thrown, "{e}");
        }
        assert_eq!(run(9000, "JSON.stringify(d).length"), "18002");
        assert_eq!(run(9000, "JSON.stringify(o).length"), "54002");
        assert_eq!(run(20000, "JSON.stringify(d)"), thrown);
        assert_eq!(run(20000, "JSON.stringify(o)"), thrown);
        // Math.abs converts without throwing: a deep or cyclic one-element chain is `""`, no crash.
        assert_eq!(run(9000, "Math.abs(d)"), "0");
        assert_eq!(
            ev("var c = []; c[0] = c; return Math.abs(c) + '|' + Number(new Date(5))"),
            "0|5"
        );
        // Nothing is left half-joined or half-written: the same values convert afterwards.
        assert_eq!(
            run(
                20000,
                "[(() => { try { String(d) } catch (e) {} try { JSON.stringify(o) } catch (e) {} })(), String([[1], 2]), JSON.stringify([[1], {a: 2}])].join('|')"
            ),
            "|1,2|[[1],{\"a\":2}]"
        );
    }

    #[test]
    fn cyclic_arrays_join_like_v8() {
        assert_eq!(
            ev(
                "var a = [1, 2]; a.push(a); var b = [a, 3]; a.push(b); return a.join('-') + '|' + String(a) + '|' + b"
            ),
            "1-2--,3|1,2,,,3|1,2,,,3"
        );
        assert_eq!(
            ev("var a = [null, undefined, [], [[1]]]; return a.join() + '|' + a.join(undefined)"),
            ",,,1|,,,1"
        );
    }

    #[test]
    fn number_formatting_matches_v8() {
        // Expected values from node 22 (V8).
        let cases = [
            ("(255).toString(16)", "ff"),
            ("(-255).toString(2)", "-11111111"),
            ("(255.5).toString(16)", "ff.8"),
            ("(-7.25).toString(8)", "-7.2"),
            ("Math.PI.toString(16)", "3.243f6a8885a3"),
            ("(0.1).toString(3)", "0.0022002200220022002200220022002201"),
            ("(2 ** 64).toString(16)", "10000000000000000"),
            ("(1e21).toString(36)", "5v1j4f4ds7c000"),
            ("(-1e300).toString(16).length", "251"),
            ("(-1e300).toString(16).slice(0, 20)", "-17e43c8800759c00000"),
            ("(5e-324).toString(2).length", "1076"),
            ("(1.7976931348623157e308).toString(2).length", "1024"),
            ("NaN.toString(16) + (-Infinity).toString(2)", "NaN-Infinity"),
            ("(10).toString(36.9)", "a"),
            ("(123.456).toPrecision(4)", "123.5"),
            ("(2.5).toPrecision(1)", "3"),
            ("(1.005).toPrecision(3)", "1.00"),
            ("(1e21).toPrecision(3)", "1.00e+21"),
            ("(0.00000123).toPrecision(2)", "0.0000012"),
            ("(1.23e-7).toPrecision(2)", "1.2e-7"),
            ("(-0).toPrecision(2)", "0.0"),
            ("(123456).toPrecision(2)", "1.2e+5"),
            ("(0.1).toPrecision(21)", "0.100000000000000005551"),
            ("(1.5).toPrecision() + NaN.toPrecision(200)", "1.5NaN"),
            ("(123456).toExponential(2)", "1.23e+5"),
            ("(1.5).toExponential()", "1.5e+0"),
            ("(0).toExponential(2)", "0.00e+0"),
            ("(-0.00015).toExponential(1)", "-1.5e-4"),
        ];
        for (src, want) in cases {
            assert_eq!(ev(&format!("return {src}")), want, "{src}");
        }
        // QV4's texts; a bad radix is a plain Error there.
        for (src, msg) in [
            (
                "(1).toString(1)",
                "false Number.prototype.toString: 1 is not a valid radix",
            ),
            (
                "(1).toString(37)",
                "false Number.prototype.toString: 37 is not a valid radix",
            ),
            (
                "(1).toPrecision(0)",
                "true Number.prototype.toPrecision: precision out of range out of range",
            ),
            (
                "(1).toPrecision(101)",
                "true Number.prototype.toPrecision: precision out of range out of range",
            ),
            (
                "(1).toExponential(-1)",
                "true Number.prototype.toExponential: fractionDigits out of range out of range",
            ),
        ] {
            assert_eq!(
                ev(&format!(
                    "try {{ return {src} }} catch (e) {{ return (e instanceof RangeError) + ' ' + e.message }}"
                )),
                msg,
                "{src}"
            );
        }
    }

    #[test]
    fn json_stringify_calls_a_replacer_function() {
        for (src, want) in [
            (
                "return JSON.stringify({a: 1, b: 'x'}, function (k, v) { \
                 return typeof v === 'number' ? v * 10 : v })",
                r#"{"a":10,"b":"x"}"#,
            ),
            (
                "return JSON.stringify({a: [{b: 1, c: 2}]}, function (k, v) { \
                 return k === 'b' ? undefined : v })",
                r#"{"a":[{"c":2}]}"#,
            ),
            (
                "var o = {a: 1}; return JSON.stringify(o, function (k, v) { \
                 return k === '' ? (this[''] === o) + ',' + Object.keys(this) : v })",
                r#""true,""#,
            ),
            (
                "var keys = []; JSON.stringify({x: {y: [5]}}, function (k, v) { \
                 keys.push(k + ':' + typeof this); return v }); return keys.join()",
                ":object,x:object,y:object,0:object",
            ),
            (
                "return JSON.stringify({d: new Date(0)}, function (k, v) { \
                 return k === 'd' ? typeof v : v })",
                r#"{"d":"string"}"#,
            ),
            (
                "return JSON.stringify({a: 1}, function () { return undefined })",
                "undefined",
            ),
            (
                "try { JSON.stringify({a: 1}, function () { throw new RangeError('r') }) } \
                 catch (e) { return e.name }",
                "RangeError",
            ),
        ] {
            assert_eq!(ev(src), want, "{src}");
        }
    }

    #[test]
    fn json_stringify_rethrows_from_nested_replacer_and_to_json() {
        for (src, want) in [
            (
                "JSON.stringify({a: 1}, function (k, v) { if (k === 'a') throw new RangeError('r'); return v })",
                "RangeError r",
            ),
            (
                "JSON.stringify({a: {toJSON() { throw new TypeError('t') }}})",
                "TypeError t",
            ),
            (
                "JSON.stringify([1, 2], function (k, v) { if (k === '1') throw new SyntaxError('s'); return v })",
                "SyntaxError s",
            ),
        ] {
            assert_eq!(
                ev(&format!(
                    "try {{ {src}; return 'none' }} catch (e) {{ return e.name + ' ' + e.message }}"
                )),
                want,
                "{src}"
            );
        }
    }

    #[test]
    fn json_stringify_to_json_and_cycles() {
        assert_eq!(
            ev(
                "return JSON.stringify({d: new Date(0), n: new Date(NaN), x: {toJSON(k) { return k + '!' }}, arr: [{toJSON(k) { return typeof k + k }}]})"
            ),
            r#"{"d":"1970-01-01T00:00:00.000Z","n":null,"x":"x!","arr":["string0"]}"#
        );
        assert_eq!(
            ev("return JSON.stringify({toJSON(k) { return [k.length, 1] }})"),
            "[0,1]"
        );
        assert_eq!(
            ev("return new Date(0).toJSON() + ' ' + new Date(NaN).toJSON()"),
            "1970-01-01T00:00:00.000Z null"
        );
        assert_eq!(
            ev("var s = {}; return JSON.stringify([s, s, {a: s}])"),
            r#"[{},{},{"a":{}}]"#
        );
        for cyc in [
            "var c = {}; c.self = {d: c}; JSON.stringify(c)",
            "var a = [1]; a.push({a: a}); JSON.stringify(a, null, 2)",
        ] {
            assert_eq!(
                ev(&format!(
                    "try {{ {cyc}; return 'no throw' }} catch (e) {{ return (e instanceof TypeError) + ' ' + e.message }}"
                )),
                "true Cannot convert circular structure to JSON",
                "{cyc}"
            );
        }
        // The serializer recovers: the same objects stringify once the cycle is cut.
        assert_eq!(
            ev(
                "var c = {}; c.c = c; try { JSON.stringify(c) } catch (e) {} c.c = 1; return JSON.stringify(c)"
            ),
            r#"{"c":1}"#
        );
    }

    #[test]
    fn regex_cache_is_bounded_and_matches_fresh_compiles() {
        let n = RE_CACHE_MAX + 100;
        let src = format!(
            "var hits = 0; for (var i = 0; i < {n}; i++) {{ if (new RegExp('^app' + i + '$', 'i').test('APP' + i)) hits++; }} return hits"
        );
        assert_eq!(ev(&src), n.to_string());
        assert!(re_cache_len() <= RE_CACHE_MAX, "{} cached", re_cache_len());
        // The earliest patterns were evicted: compiling them again gives the same results.
        assert_eq!(
            ev("return [/^app0$/i.test('APP0'), new RegExp('^app0$').test('APP0')].join()"),
            "true,false"
        );
        assert!(re_cache_len() <= RE_CACHE_MAX);
    }

    /// Qt 6.12 (QJSEngine) results: without `u` a pattern and its subject are UTF-16 code units,
    /// with `u` code points; every index is a code-unit index.
    #[test]
    fn regex_matches_utf16_code_units_without_u() {
        for (src, want) in [
            (r#"/^.$/.test("😀")"#, "false"),
            (r#"/^..$/.test("😀")"#, "true"),
            (r#"/^.$/u.test("😀")"#, "true"),
            (r#"/^\S\S$/.test("😀")"#, "true"),
            (r#""😀".match(/./)[0].length"#, "1"),
            (r#""😀".match(/./u)[0].length"#, "2"),
            (r#""😀".match(/[^a]/)[0].length"#, "1"),
            (r#""😀😀".match(/./g).length"#, "4"),
            (r#""😀😀".match(/./gu).length"#, "2"),
            (r#""😀".match(/(?:)/g).length"#, "3"),
            (r#""😀".match(/(?:)/gu).length"#, "2"),
            (r#"/\ud83d/.test("😀")"#, "true"),
            (r#"/\ud83d/u.test("😀")"#, "false"),
            (r#""😀".search(/\ude00/)"#, "1"),
            (r#"/^[\ud83d\ude00]$/.test("😀")"#, "false"),
            (r#"/^[\ud83d\ude00]$/u.test("😀")"#, "true"),
            (r#"/^[😀]$/.test("😀")"#, "false"),
            (r#"/^[😀]$/u.test("😀")"#, "true"),
            (r#"/^😀$/.test("😀")"#, "true"),
            (r#""a😀b".match(/b/).index"#, "3"),
            (r#""a😀b".search(/b/)"#, "3"),
            (r#""😀x".replace(/./g, "-")"#, "---"),
            (r#""😀x".replace(/./gu, "-")"#, "--"),
            (r#""😀".replace(/(?:)/g, "-").length"#, "5"),
            (r#""a😀b😀c".replace(/😀/g, "|")"#, "a|b|c"),
            (r#""a😀b".replace(/b/, "[$`]")"#, "a😀[a😀]"),
            (r#""a😀b".replace(/(.)b/, "<$1>").length"#, "5"),
            (
                r#""x😀y".replace(/y/, function (m, off) { return off })"#,
                "x😀3",
            ),
            (
                r#"JSON.stringify("😀".split(/\ud83d/).map(function (c) { return c.length }))"#,
                "[0,1]",
            ),
            (
                "(function () { var r = /./g; r.exec('😀'); return r.lastIndex })()",
                "1",
            ),
            (
                "(function () { var r = /./gu; r.exec('😀'); return r.lastIndex })()",
                "2",
            ),
            (
                "(function () { var r = /x/g; r.lastIndex = 1; var m = r.exec('😀x'); return m.index + ',' + r.lastIndex })()",
                "2,3",
            ),
            (
                "(function () { var r = /x/y; r.lastIndex = 2; var m = r.exec('😀x'); return m.index + ',' + r.lastIndex })()",
                "2,3",
            ),
            (
                "(function () { var r = /./g; r.lastIndex = 1; var m = r.exec('😀x'); return [m[0].length, m.index, r.lastIndex].join() })()",
                "1,1,2",
            ),
        ] {
            assert_eq!(ev(&format!("return {src}")), want, "{src}");
        }
    }

    /// ECMA-262 where Qt 6.12 deviates: Qt's `split` by a regex yields empty strings, its `u`
    /// `replace` advances past an empty match by one code unit, and it lacks lookbehind and
    /// the `s` flag.
    #[test]
    fn regex_follows_ecma262_where_qt_deviates() {
        for (src, want) in [
            (
                r#"JSON.stringify("😀".split(/(?:)/).map(function (c) { return c.length }))"#,
                "[1,1]",
            ),
            (
                r#"JSON.stringify("😀".split(/(?:)/u).map(function (c) { return c.length }))"#,
                "[2]",
            ),
            (r#"JSON.stringify("ab".split(/(?:)/))"#, r#"["a","b"]"#),
            (r#"JSON.stringify("axb".split(/x*/))"#, r#"["a","b"]"#),
            (r#"JSON.stringify("".split(/(?:)/))"#, "[]"),
            (r#"JSON.stringify("".split(/,/))"#, r#"[""]"#),
            (
                r#"JSON.stringify("a,b,,c".split(/(,)/))"#,
                r#"["a",",","b",",","",",","c"]"#,
            ),
            (
                r#"JSON.stringify("a1b2c".split(/(\d)/, 2))"#,
                r#"["a","1"]"#,
            ),
            (r#""😀".replace(/(?:)/gu, "-")"#, "-😀-"),
            (r#""abc".replace(/b*/g, "-")"#, "-a--c-"),
            (r#"/(?<=\$)\d+/.exec("cost $42")[0]"#, "42"),
            (r#"/^.$/s.test("\n")"#, "true"),
            (r#"/^.$/.test("\u2028")"#, "false"),
            (r#"/\u212a/i.test("k")"#, "false"),
            (r#"/\u212a/iu.test("k")"#, "true"),
            (
                r#"/\d/.test("٣") + "," + /\w/.test("é") + "," + /\s/.test("\ufeff")"#,
                "false,false,true",
            ),
            (
                r#"/{(\d+)}/.exec("a{12}")[1] + /a{/.test("a{") + /]/.test("]")"#,
                "12truetrue",
            ),
            (r#"/[^[\]]+/.exec("ab[c")[0]"#, "ab"),
            (r#"/(a)|b/.exec("b")[1] === undefined"#, "true"),
            (r#"/\1(a)/.exec("aa")[0]"#, "a"),
        ] {
            assert_eq!(ev(&format!("return {src}")), want, "{src}");
        }
    }

    #[test]
    fn json_stringify_numbers_and_escapes() {
        assert_eq!(
            ev(
                r#"return JSON.stringify({a: [1, -0, 1.5, 1e21, 1793217195000, -7], s: 'plain', e: 'q"b\\n\n\u0001'})"#
            ),
            r#"{"a":[1,0,1.5,1e+21,1793217195000,-7],"s":"plain","e":"q\"b\\n\n\u0001"}"#
        );
        assert_eq!(
            ev("return JSON.stringify([{x: 1}], null, 2)"),
            "[\n  {\n    \"x\": 1\n  }\n]"
        );
    }

    /// SerializeJSONObject with a PropertyList (ECMA-262 §25.5.2 step 4.b): an array replacer
    /// lists the keys of every object, strings, numbers and their wrappers, first occurrence
    /// only; each key is read with [[Get]]; arrays and a non-callable, non-array replacer
    /// are not affected. Expected values by hand from the spec.
    #[test]
    fn json_stringify_property_list() {
        for (src, want) in [
            ("return JSON.stringify({b: 1, a: 2}, ['a'])", r#"{"a":2}"#),
            (
                "return JSON.stringify({1: 'one', b: 2, a: 1, c: 3}, \
                 ['b', 1, new String('a'), 'b', new Number(1), {}, true, null])",
                r#"{"b":2,"1":"one","a":1}"#,
            ),
            (
                "return JSON.stringify({a: {a: 1, z: 2}, z: 0, y: [{a: 3, z: 4}]}, ['a', 'y'])",
                r#"{"a":{"a":1},"y":[{"a":3}]}"#,
            ),
            (
                "return JSON.stringify([{a: 1, z: 2}, 3], ['a'])",
                r#"[{"a":1},3]"#,
            ),
            (
                "return JSON.stringify(Object.create({a: 1}), ['a', 'x'])",
                r#"{"a":1}"#,
            ),
            (
                "return JSON.stringify(Object.defineProperty({}, 'h', {value: 5}), ['h'])",
                r#"{"h":5}"#,
            ),
            (
                "return JSON.stringify({a: 1, b: 2}, new Proxy(['b'], {}))",
                r#"{"b":2}"#,
            ),
            (
                "return JSON.stringify(new Proxy({a: 1, b: 2}, {}), ['b'])",
                r#"{"b":2}"#,
            ),
            (
                "return JSON.stringify({a: 1, b: 2}, ['a'], 1)",
                "{\n \"a\": 1\n}",
            ),
            ("return JSON.stringify({a: 1, b: 2}, [])", "{}"),
            (
                "return JSON.stringify({a: 1}, {0: 'b', length: 1})",
                r#"{"a":1}"#,
            ),
            (
                "var l = ['a']; var n = 0; Object.defineProperty(l, 1, \
                 {get() { n++; return 'b' }}); \
                 return JSON.stringify({a: {b: 1}, b: 2}, l) + n",
                r#"{"a":{"b":1},"b":2}1"#,
            ),
        ] {
            assert_eq!(ev(src), want, "{src}");
        }
    }

    /// InternalizeJSONProperty (ECMA-262 §25.5.1.1): the reviver runs bottom-up on each
    /// holder, `undefined` deletes, keys are listed before the walk and read with [[Get]].
    #[test]
    fn json_parse_reviver() {
        for (src, want) in [
            (
                "return JSON.parse('[1,2]', function (k, v) { \
                 return typeof v === 'number' ? v * 2 : v }).join()",
                "2,4",
            ),
            (
                "var log = []; JSON.parse('{\"a\":[1,{\"b\":2}],\"c\":3}', \
                 function (k, v) { log.push(k); return v }); return log.join()",
                "0,b,1,a,c,",
            ),
            (
                "var o = JSON.parse('{\"a\":1,\"b\":2}', function (k, v) { \
                 return k === 'a' ? undefined : v }); return Object.keys(o).join() + ('a' in o)",
                "bfalse",
            ),
            (
                "var r = JSON.parse('[1,2,3]', function (k, v) { \
                 return k === '1' ? undefined : v }); return r.length + ',' + (1 in r)",
                "3,false",
            ),
            (
                "return JSON.parse('{\"a\":1}', function (k, v) { \
                 return k === '' ? (this[''] === v) + ',' + Object.keys(this) : v })",
                "true,",
            ),
            (
                "return JSON.parse('{\"a\":1,\"b\":2}', function (k, v) { \
                 if (k === 'a') this.b = 5; return v }).b",
                "5",
            ),
            (
                "return JSON.parse('{\"a\":1,\"b\":2}', function (k, v) { \
                 if (k === 'a') { this.c = 7; delete this.b } return v; }).c",
                "7",
            ),
            (
                "return JSON.parse('{\"a\":1}', function (k, v) { \
                 return k === '' ? 'root' : v })",
                "root",
            ),
            (
                "return JSON.parse('{\"a\":{\"b\":1}}', function (k, v) { \
                 return k === 'b' ? this.constructor === Object : v }).a.b",
                "true",
            ),
            ("return JSON.parse('[1]', 5)[0]", "1"),
            (
                "try { JSON.parse('[1]', function () { throw new RangeError('r') }) } \
                 catch (e) { return e.name }",
                "RangeError",
            ),
            (
                "var n = 0; JSON.parse('1', function () { n = arguments.length }); return n",
                "2",
            ),
        ] {
            assert_eq!(ev(src), want, "{src}");
        }
    }

    /// Math[@@toStringTag] and JSON[@@toStringTag] (ECMA-262 §21.3.1.9, §25.5.3):
    /// { [[Writable]]: false, [[Enumerable]]: false, [[Configurable]]: true }.
    #[test]
    fn math_and_json_to_string_tag() {
        for (name, tag) in [("Math", "Math"), ("JSON", "JSON")] {
            let src = format!(
                "var d = Object.getOwnPropertyDescriptor({name}, Symbol.toStringTag); \
                 return [Object.prototype.toString.call({name}), String({name}), d.value, \
                 d.writable, d.enumerable, d.configurable].join()"
            );
            assert_eq!(
                ev(&src),
                format!("[object {tag}],[object {tag}],{tag},false,false,true"),
                "{name}"
            );
        }
    }

    #[test]
    fn json_parse_strings_and_keys() {
        assert_eq!(
            ev(
                r#"var o = JSON.parse('{"a":"plain","b":"q\\"x\\n\\u00e9","c":"привет 🙂","a":"dup","d":["",{"e":"\\\\"}]}'); return [Object.keys(o).join(), o.a, o.b, o.c, o.d[0].length, o.d[1].e].join('|')"#
            ),
            "a,b,c,d|dup|q\"x\né|привет 🙂|0|\\"
        );
    }

    #[test]
    fn array_sort_ties_follow_v4() {
        // Qt 6.12 `qml`: V4's sortHelper is an unstable quicksort.
        assert_eq!(
            ev(
                "var c = (a, b) => (a.a && !b.a) ? -1 : (!a.a && b.a) ? 1 : b.s - a.s; \
                return [{n: 'C', s: 100}, {n: 'N', s: 100}, {n: 'H', s: 90, a: true}] \
                .sort(c).map(x => x.n).join()"
            ),
            "H,N,C"
        );
        assert_eq!(
            ev(
                "return [{n: 'A', k: 1}, {n: 'B', k: 1}, {n: 'C', k: 1}, {n: 'D', k: 0}] \
                .sort((a, b) => a.k - b.k).map(x => x.n).join()"
            ),
            "D,A,C,B"
        );
    }

    #[test]
    fn array_from_collections() {
        assert_eq!(
            ev("return Array.from(new Set(['b', 'a', 'b'])).sort().join()"),
            "a,b"
        );
        assert_eq!(ev("return Array.from(new Map([[1, 2]])).join()"), "1,2");
    }

    #[test]
    fn symbol_keys_follow_ecma262_where_qv4_deviates() {
        // QV4 (Qt 6.12) skips symbol keys in Object.defineProperties, Object.create and
        // Object.assign, the proxy [[OwnPropertyKeys]] invariants for them, and the target's
        // symbol properties when freezing a proxy; ECMA-262 takes every own property key
        // (§20.1.2.3.1, §20.1.2.1, §10.5.11, §7.3.15).
        let cases = [
            (
                "var s = Symbol(); var p = {}; p[s] = {value: 7, enumerable: true}; var o = Object.defineProperties({}, p); return o[s] + JSON.stringify(Object.getOwnPropertyDescriptor(o, s))",
                "7{\"value\":7,\"writable\":false,\"enumerable\":true,\"configurable\":false}",
            ),
            (
                "var s = Symbol(); var p = {}; p[s] = {value: 8}; var o = Object.create(null, p); return o[s] + ',' + Reflect.ownKeys(o).length",
                "8,1",
            ),
            (
                "var s = Symbol(); var src = {a: 1}; src[s] = 2; return Object.assign({}, src)[s]",
                "2",
            ),
            (
                "var s = Symbol(); var o = {}; Object.defineProperty(o, s, {value: 1}); var src = {}; src[s] = 2; try { Object.assign(o, src) } catch (e) { return e.name } return 'none'",
                "TypeError",
            ),
            (
                "var s = Symbol(); try { Reflect.ownKeys(new Proxy({}, {ownKeys: function() { return [s, s] }})) } catch (e) { return e.name } return 'none'",
                "TypeError",
            ),
            (
                "var s = Symbol(); var t0 = {}; Object.defineProperty(t0, s, {value: 1}); try { Reflect.ownKeys(new Proxy(t0, {ownKeys: function() { return [] }})) } catch (e) { return e.name } return 'none'",
                "TypeError",
            ),
            (
                "var s = Symbol(); var t0 = {}; t0[s] = 1; Object.freeze(new Proxy(t0, {})); return Object.isFrozen(t0) + JSON.stringify(Object.getOwnPropertyDescriptor(t0, s))",
                "true{\"value\":1,\"writable\":false,\"enumerable\":true,\"configurable\":false}",
            ),
        ];
        for (src, want) in cases {
            assert_eq!(ev(src), want, "{src}");
        }
    }

    #[test]
    fn concat_follows_ecma262_where_qv4_deviates() {
        // QV4 (Qt 6.12) spreads every array whatever its `@@isConcatSpreadable`, and copies an
        // array's holes as holes; IsConcatSpreadable (ECMA-262 §23.1.3.1.1) reads the symbol
        // first, and concat copies an element HasProperty finds, inherited ones too.
        assert_eq!(
            ev(
                "var a = [1, 2]; a[Symbol.isConcatSpreadable] = false; var r = [].concat(a); var s = a.concat([3]); return [r.length, r[0] === a, [0].concat(a, [3]).length, s.length, s[0] === a, s[1]].join()"
            ),
            "1,true,3,2,true,3"
        );
        assert_eq!(
            ev(
                "Array.prototype[1] = 'P'; try { var r = [1, , 3].concat([, 5]); return r.join() + '|' + r.hasOwnProperty(1) + '|' + r.hasOwnProperty(3) } finally { delete Array.prototype[1] }"
            ),
            "1,P,3,,5|true|false"
        );
        assert_eq!(
            ev(
                "var o = {length: 2, 0: 'a'}; o[Symbol.isConcatSpreadable] = 1; var p = new Proxy({length: 1, 0: 'q'}, {}); var a = [9]; a[Symbol.isConcatSpreadable] = ''; return JSON.stringify([[].concat(o), [].concat(p).length, [].concat(a).length, 1 in [].concat(o)])"
            ),
            "[[\"a\",null],1,1,false]"
        );
    }

    #[test]
    fn proxy_follows_ecma262_where_qv4_deviates() {
        // Every proxy is tagged `[object ProxyObject]`, as in QV4 (Qt 6.12). `instanceof`
        // ignores a getPrototypeOf trap there; ECMA-262's OrdinaryHasInstance walks
        // [[GetPrototypeOf]], the trap (§7.3.21).
        assert_eq!(
            ev(
                "return [Object.prototype.toString.call(new Proxy([1], {})), String(new Proxy({}, {})), Object.prototype.toString.call(new Proxy(function() {}, {})), Object.prototype.toString.call(new Proxy(new Proxy([], {}), {}))].join()"
            ),
            "[object ProxyObject],[object ProxyObject],[object ProxyObject],[object ProxyObject]"
        );
        assert_eq!(
            ev(
                "var p = new Proxy({}, {getPrototypeOf: function() { return Array.prototype }}); return (p instanceof Array) + ',' + (p instanceof Object)"
            ),
            "true,true"
        );
        assert_eq!(
            ev(
                "var r = Proxy.revocable([], {}); r.revoke(); try { Object.prototype.toString.call(r.proxy) } catch (e) { return e.name + ': ' + e.message }"
            ),
            "TypeError: Type error"
        );
        // QV4 skips these invariants and traps, or mishandles generic built-ins on a proxy.
        let cases = [
            // §10.5.11 steps 9 and 21-22: duplicates and a missing non-configurable key.
            (
                "Object.keys(new Proxy({}, {ownKeys: function() { return ['a', 'a'] }}))",
                "TypeError",
            ),
            (
                "var t0 = {}; Object.defineProperty(t0, 'c', {value: 1}); Object.getOwnPropertyNames(new Proxy(t0, {ownKeys: function() { return [] }}))",
                "TypeError",
            ),
            // EnumerableOwnProperties and for-in ask [[GetOwnProperty]] (§7.3.23, §14.7.5.9).
            (
                "return Object.keys(new Proxy({a: 1, b: 2}, {getOwnPropertyDescriptor: function(t, k) { return k === 'a' ? undefined : Object.getOwnPropertyDescriptor(t, k) }}))",
                "b",
            ),
            (
                "var p = new Proxy({a: 1}, {ownKeys: function() { return ['z', 'a'] }, getOwnPropertyDescriptor: function(t, k) { return {value: 1, enumerable: k === 'z', configurable: true} }}); var ks = []; for (var k in p) ks.push(k); return ks",
                "z",
            ),
            // The defineProperty trap gets FromPropertyDescriptor of the given fields (§10.5.6).
            (
                "var log; var p = new Proxy({}, {defineProperty: function(t, k, d) { log = k + JSON.stringify(d); return true }}); Object.defineProperty(p, 'x', {value: 1, enumerable: true}); return log",
                "x{\"value\":1,\"enumerable\":true}",
            ),
            // QV4's Object.preventExtensions ignores a refusing trap; §20.1.2.18 throws.
            (
                "return Object.preventExtensions(new Proxy({}, {preventExtensions: function() { return false }})) instanceof Object",
                "true",
            ),
            // SetIntegrityLevel goes through the proxy to its target (§7.3.15).
            (
                "var t0 = {a: 1}; var p = new Proxy(t0, {}); Object.freeze(p); var s0 = {a: 1}; Object.seal(new Proxy(s0, {})); return [Object.isFrozen(t0), Object.isFrozen(p), Object.isSealed(s0), Object.isFrozen(s0)]",
                "true,true,true,false",
            ),
            // [[Construct]] without a trap is the target's; `prototype`, `length` and `name`
            // are read through [[Get]].
            (
                "function F(a) { this.a = a } var P = new Proxy(F, {}); var o = new P(3); return [o.a, o instanceof F, new F(1) instanceof P, new Proxy(function f(a, b) {}, {}).length, new Proxy(function f(a, b) {}, {}).name]",
                "3,true,true,2,f",
            ),
            // IsArray of a revoked proxy throws (§7.2.2); revocable's result has enumerable
            // data properties (CreateDataPropertyOrThrow, §28.2.2.1).
            (
                "var r = Proxy.revocable([], {}); var ks = Object.keys(r); r.revoke(); Array.isArray(r.proxy)",
                "TypeError",
            ),
            (
                "return Object.keys(Proxy.revocable({}, {}))",
                "proxy,revoke",
            ),
            // [[Set]] without a trap defines on the receiver (§10.1.9.2), each operation looks
            // up only its own trap, Object.keys asks each key's descriptor.
            (
                "var log = []; var h = new Proxy({}, {get: function(t, k) { log.push(k); return undefined }}); var p = new Proxy({a: 1}, h); p.a; 'a' in p; p.b = 2; delete p.b; Object.keys(p); return log",
                "get,has,set,getOwnPropertyDescriptor,defineProperty,deleteProperty,ownKeys,getOwnPropertyDescriptor",
            ),
            // SerializeJSONArray of an array proxy (§25.5.2.6) and generic array methods.
            (
                "return JSON.stringify(new Proxy([1, 'x', undefined], {}))",
                "[1,\"x\",null]",
            ),
            (
                "var p = new Proxy([3, 1, 2], {}); p.push(4); return p.join('-') + ',' + p.indexOf(2)",
                "3-1-2-4,2",
            ),
            // Object.values reads each value through [[Get]] (§7.3.23).
            (
                "return Object.values(new Proxy({a: 1, b: 2}, {get: function(t, k) { return t[k] * 10 }}))",
                "10,20",
            ),
            // Object spread and rest (no QV4 syntax) read through the traps.
            (
                "var p = new Proxy({a: 1, b: 2}, {get: function(t, k) { return k }}); var {a, ...rest} = new Proxy({a: 1, b: 2}, {}); return JSON.stringify({...p}) + a + JSON.stringify(rest)",
                "{\"a\":\"a\",\"b\":\"b\"}1{\"b\":2}",
            ),
        ];
        for (src, want) in cases {
            let src = format!("try {{ {src} }} catch (e) {{ return e.name }}");
            assert_eq!(ev(&src), want, "{src}");
        }
    }

    #[test]
    fn object_is_same_value() {
        assert_eq!(
            ev(
                "return [Object.is(NaN, NaN), Object.is(0, -0), Object.is(-0, -0), Object.is(0, 0), Object.is('a', 'a'), Object.is({}, {}), Object.is(), Object.is(null, undefined), Object.is(1, '1')].join()"
            ),
            "true,false,true,true,true,false,true,false,false"
        );
        assert_eq!(ev("var o = {}; return Object.is(o, o)"), "true");
    }

    #[test]
    fn search_and_match_take_string_patterns() {
        assert_eq!(
            ev(
                "return ['a.c'.search('.'), 'abc'.search('c'), 'abc'.search(), 'x\u{1F600}a'.search('a'), 'abc'.search('z'), 'abc'.search(/C/i)].join()"
            ),
            "0,2,0,3,-1,2"
        );
        assert_eq!(
            ev(
                "return ['a.c'.match('.')[0], 'abc'.match('b+').index, 'abc'.match()[0] === '', 'abc'.match('z')].join()"
            ),
            "a,1,true,"
        );
        assert_eq!(
            ev("return 'a'.search('(')"),
            "THROW Invalid regular expression"
        );
    }

    #[test]
    fn qstr_without_translator() {
        assert_eq!(ev("return qsTr('Power')"), "Power");
        assert_eq!(ev("return qsTr('%n file(s) %n', '', 3)"), "3 file(s) 3");
        assert_eq!(ev("return qsTr('%n %Lx %', 'd')"), "%n %Lx %");
        assert_eq!(ev("return qsTr('%n', '', -1)"), "%n");
        assert_eq!(ev("return qsTranslate('Ctx', 'Hi %n', '', 2)"), "Hi 2");
        assert_eq!(ev("return qsTrId('id.x')"), "id.x");
        assert_eq!(
            ev("return QT_TR_NOOP('a') + QT_TRANSLATE_NOOP('c', 'b') + QT_TRID_NOOP('i')"),
            "abi"
        );
        assert_eq!(
            ev("return qsTr()"),
            "THROW qsTr() requires at least one argument"
        );
        assert_eq!(
            ev("return qsTr(1)"),
            "THROW qsTr(): first argument (sourceText) must be a string"
        );
        assert_eq!(
            ev("return qsTr('a', 'b', '1')"),
            "THROW qsTr(): third argument (n) must be a number"
        );
        assert_eq!(
            ev("return qsTranslate('a')"),
            "THROW qsTranslate() requires at least two arguments"
        );
        assert_eq!(group_digits(1234567, "en_US"), "1,234,567");
        assert_eq!(group_digits(-1234, "de_DE"), "-1.234");
        assert_eq!(group_digits(999, "ru_RU"), "999");
        assert_eq!(group_digits(1234, "C"), "1234");
        // What Qt 6.12's `qsTr("%Ln", "", n)` gives under each LANG: the locale's group
        // sizes, the least count of digits it groups, its separator and its digits.
        assert_eq!(group_digits(1234567, "en_IN"), "12,34,567");
        assert_eq!(group_digits(1234567, "hi_IN.UTF-8"), "12,34,567");
        assert_eq!(group_digits(1234567, "de_CH"), "1'234'567");
        assert_eq!(group_digits(1234567, "fr_FR"), "1\u{202f}234\u{202f}567");
        assert_eq!(group_digits(1234567, "ru_RU"), "1\u{a0}234\u{a0}567");
        assert_eq!(group_digits(1234567, "ar_EG"), "١٬٢٣٤٬٥٦٧");
        assert_eq!(group_digits(1234567, "fa_IR"), "۱٬۲۳۴٬۵۶۷");
        assert_eq!(group_digits(1234, "es_ES"), "1234");
        assert_eq!(group_digits(12345, "es_ES"), "12.345");
        assert_eq!(group_digits(1234, "pl_PL"), "1234");
        assert_eq!(group_digits(12345, "pt_PT"), "12\u{a0}345");
    }

    #[test]
    fn json_stringify_matches_v8() {
        assert_eq!(
            ev(
                r#"return JSON.stringify({a:1,b:[1,"x\n\"",null,undefined,()=>1],c:undefined,d:{},e:[],f:{g:{h:true}}},null,2)"#
            ),
            "{\n  \"a\": 1,\n  \"b\": [\n    1,\n    \"x\\n\\\"\",\n    null,\n    null,\n    null\n  ],\n  \"d\": {},\n  \"e\": [],\n  \"f\": {\n    \"g\": {\n      \"h\": true\n    }\n  }\n}"
        );
        assert_eq!(
            ev(r#"return JSON.stringify({a:1,b:[1,"x",null,undefined],c:undefined,d:{},e:[]})"#),
            "{\"a\":1,\"b\":[1,\"x\",null,null],\"d\":{},\"e\":[]}"
        );
        assert_eq!(
            ev(r#"return JSON.stringify([undefined,{u:undefined}],null,"\t")"#),
            "[\n\tnull,\n\t{}\n]"
        );
    }

    #[test]
    fn basics() {
        assert_eq!(
            ev("return [3,1,2].sort().map(x => x * 2).join('-')"),
            "2-4-6"
        );
        assert_eq!(
            ev("let o = {a: 1, ...{b: 2}}; return JSON.stringify(o)"),
            r#"{"a":1,"b":2}"#
        );
        assert_eq!(
            ev("return JSON.parse('{\"x\":[1,2,{\"y\":\"z\"}]}').x[2].y"),
            "z"
        );
        assert_eq!(
            ev("return 'a-b_c'.replace(/[-_]/g, ' ').toUpperCase()"),
            "A B C"
        );
        assert_eq!(ev("return '%1 of %2'.arg(3).arg('x')"), "3 of x");
        assert_eq!(
            ev("return (2.5).toFixed(0) + ' ' + (1.005).toFixed(2) + ' ' + Math.round(-2.5)"),
            "3 1.00 -2"
        );
        assert_eq!(
            ev(
                "class A { constructor(x) { this.x = x } get d() { return this.x * 2 } } class B extends A { y = 5; sum() { return this.x + this.y + this.d } } return new B(1).sum()"
            ),
            "8"
        );
        assert_eq!(
            ev(
                "let s = 0; outer: for (let i = 0; i < 5; i++) { for (let j = 0; j < 5; j++) { if (j == 2) continue outer; if (i == 3) break outer; s += 1 } } return s"
            ),
            "6"
        );
        assert_eq!(
            ev("const {a, b: [c, d = 4]} = {a: 1, b: [2]}; return a + c + d"),
            "7"
        );
        assert_eq!(ev("let x = null; return x?.y?.z ?? 'none'"), "none");
        assert_eq!(ev("try { null.x } catch (e) { return 'caught' }"), "caught");
        assert_eq!(ev("return Qt.rgba(1, 0.5, 0, 0.5).toString()"), "#80ff8000");
        assert_eq!(
            ev(
                "let c = Qt.color('#3a7bd5'); return Qt.hsla(c.hslHue, c.hslSaturation, c.hslLightness, 1).toString()"
            ),
            "#3a7bd5"
        );
        assert_eq!(
            ev("return 'abc'.split('').reverse().join('') + typeof undefinedVar"),
            "cbaundefined"
        );
    }
}

// ---- Date / Qt.locale ----------------------------------------------------------------------

thread_local!(static CLOCK: std::cell::Cell<Option<f64>> = const { std::cell::Cell::new(None) });

/// Pin `Date.now()` / `new Date()` of this thread to `ms` since the epoch (`None`: real clock).
pub fn set_clock(ms: Option<f64>) {
    CLOCK.with(|c| c.set(ms));
}

/// The pinned clock, if any (see [`set_clock`]).
pub fn pinned_clock() -> Option<f64> {
    CLOCK.with(|c| c.get())
}

pub(crate) fn now_ms() -> f64 {
    if let Some(t) = CLOCK.with(|c| c.get()) {
        return t;
    }
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0)
}

/// A Date whose prototype is the realm's %Date.prototype% ([`Interp::proto_of`]): what a QML
/// `date` property holds and hands to script.
pub fn plain_date(ms: f64) -> Value {
    let o = new_obj();
    o.borrow_mut().internal = Internal::Date(ms);
    Value::Obj(o)
}

/// Whether Date.prototype method `name` changes the Date's time value.
pub fn is_date_setter(name: &str) -> bool {
    name.strip_prefix("set").is_some_and(|n| {
        matches!(
            n.strip_prefix("UTC").unwrap_or(n),
            "Time"
                | "Milliseconds"
                | "Seconds"
                | "Minutes"
                | "Hours"
                | "Date"
                | "Month"
                | "FullYear"
        ) || n == "Year"
    })
}

/// `QDateTime::fromString(s, Qt::ISODate)` as a time value: `YYYY-MM-DD`, then optionally `T`, `t`
/// or a space, `HH[:mm[:ss]]` with a `.` or `,` fraction of the last field, and `Z` or a
/// `±HH[[:]mm]` offset. Without a zone the time is local; a date alone is its local midnight.
pub fn qt_iso_datetime(s: &str) -> Option<f64> {
    let b = s.as_bytes();
    let int = |d: &[u8]| -> Option<f64> {
        (!d.is_empty() && d.iter().all(u8::is_ascii_digit))
            .then(|| std::str::from_utf8(d).ok()?.parse::<f64>().ok())
            .flatten()
    };
    if b.len() < 10 || !b[4].is_ascii_punctuation() || !b[7].is_ascii_punctuation() {
        return None;
    }
    let (year, month, day) = (int(&b[..4])?, int(&b[5..7])?, int(&b[8..10])?);
    let day_ms = make_day(year, month - 1.0, day) * 86_400_000.0;
    // QDate::isValid: the 30th of February is no date.
    if year == 0.0 || !(1.0..=12.0).contains(&month) || day < 1.0 {
        return None;
    }
    if make_day(year, month, 1.0) * 86_400_000.0 <= day_ms {
        return None;
    }
    if b.len() == 10 {
        return Some(qdatetime_instant(day_ms));
    }
    if !matches!(b[10], b'T' | b't' | b' ') {
        return None;
    }
    let mut t = &b[11..];
    let mut offset = None;
    if t.last().is_some_and(|c| c.eq_ignore_ascii_case(&b'Z')) {
        offset = Some(0.0);
        t = &t[..t.len() - 1];
    } else if let Some(i) = t.iter().rposition(|c| matches!(c, b'+' | b'-')) {
        let o = &t[i..];
        if !(2..=6).contains(&o.len()) {
            return None;
        }
        let (hh, mm) = match o[1..].iter().position(|c| *c == b':') {
            Some(c) => (&o[1..1 + c], &o[2 + c..]),
            None => (&o[1..o.len().min(3)], &o[o.len().min(3)..]),
        };
        let h = int(hh).filter(|h| *h <= 23.0)?;
        let m = if mm.is_empty() {
            0.0
        } else {
            int(mm).filter(|m| *m <= 59.0)?
        };
        let sign = if o[0] == b'-' { -1.0 } else { 1.0 };
        offset = Some(sign * (h * 60.0 + m) * 60_000.0);
        t = &t[..i];
    }
    let mut frac = None;
    if let Some(i) = t.iter().position(|c| matches!(c, b'.' | b',')) {
        let tail = &t[i + 1..];
        frac = Some(int(tail)? * 0.1f64.powi(tail.len() as i32));
        t = &t[..i];
    }
    if !(2..=8).contains(&t.len()) {
        return None;
    }
    let mut fraction = frac.unwrap_or(0.0);
    let hour = int(&t[..2]).filter(|h| *h <= 24.0)?;
    let mut field = |present: bool, at: usize, last: bool| -> Option<f64> {
        if present {
            let ok = t[at - 1] == b':'
                && if last {
                    t.len() == at + 2
                } else {
                    t.len() > at + 1
                };
            return int(t.get(at..at + 2).filter(|_| ok)?).filter(|v| *v < 60.0);
        }
        if frac.is_none() {
            return Some(0.0);
        }
        fraction *= 60.0;
        let whole = fraction.trunc();
        fraction -= whole;
        Some(whole)
    };
    let mut minute = field(t.len() > 2, 3, false)?;
    let mut second = field(t.len() > 5, 6, true)?;
    let mut hour = hour;
    let mut msec = if frac.is_some() {
        (1000.0 * fraction).round()
    } else {
        0.0
    };
    if msec == 1000.0 {
        msec = 0.0;
        second += 1.0;
        if second == 60.0 {
            second = 0.0;
            minute += 1.0;
            if minute == 60.0 {
                minute = 0.0;
                hour += 1.0;
            }
        }
    }
    // 24:00:00 is the next day's midnight; any other time in hour 24 is none.
    if hour == 24.0 && (minute, second, msec) != (0.0, 0.0, 0.0) {
        return None;
    }
    let ms = day_ms + make_time(hour, minute, second, msec);
    Some(match offset {
        Some(o) => ms - o,
        None => qdatetime_instant(ms),
    })
}

pub fn date_ms(v: &Value) -> Option<f64> {
    match v {
        Value::Obj(o) => match o.borrow().internal {
            Internal::Date(n) => Some(n),
            _ => None,
        },
        _ => None,
    }
}

/// A new Date object of the time of the Date `v`: what a `date` property hands to script on
/// every read, and what a `date` or `var` property keeps of a Date stored in it (Qt holds a
/// QDateTime, not the object).
pub fn date_copy(v: &Value) -> Option<Value> {
    let Value::Obj(d) = v else { return None };
    let b = d.borrow();
    let Internal::Date(ms) = b.internal else {
        return None;
    };
    let o = new_obj();
    o.borrow_mut().proto = b.proto.clone();
    o.borrow_mut().internal = Internal::Date(ms);
    Some(Value::Obj(o))
}

const MS_PER_DAY: f64 = 86_400_000.0;
/// Time values whose local clock readings chrono's types hold: its years end at ±262143,
/// and ECMA-262's time values (±8.64e15 ms) reach the years -271821 and 275760.
const CHRONO_MS: f64 = 8.2e15;
/// The proleptic Gregorian calendar repeats every 400 years: 146097 days, a whole number of
/// weeks, so the dates, the days of the week and the days a zone's daylight-saving rule
/// names are the same 400 years on.
const CYCLE_MS: f64 = 146_097.0 * MS_PER_DAY;

/// The whole 400-year cycles, in ms, between time value or local clock reading `t` and the
/// same day and time of a year chrono holds: `t - cycles_past_chrono(t)` is within
/// [`CHRONO_MS`]. 0 for a `t` chrono holds.
fn cycles_past_chrono(t: f64) -> f64 {
    if t.abs() <= CHRONO_MS {
        return 0.0;
    }
    ((t.abs() - CHRONO_MS) / CYCLE_MS).ceil() * CYCLE_MS * t.signum()
}

/// ECMA-262 TimeClip.
fn time_clip(t: f64) -> f64 {
    if !t.is_finite() || t.abs() > 8.64e15 {
        return f64::NAN;
    }
    t.trunc() + 0.0
}

/// Days from 1970-01-01 to proleptic Gregorian `year`-`month`-`day` (month 1..=12).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468
}

/// (year, month 1..=12, day 1..=31) of the day `z` days after 1970-01-01.
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (
        yoe + era * 400 + i64::from(m <= 2),
        m,
        doy - (153 * mp + 2) / 5 + 1,
    )
}

/// ECMA-262 MakeDay. Years far beyond the time value range are NaN, as the spec permits.
fn make_day(year: f64, month: f64, date: f64) -> f64 {
    if !(year.is_finite() && month.is_finite() && date.is_finite()) {
        return f64::NAN;
    }
    let ym = year.trunc() + (month.trunc() / 12.0).floor();
    if ym.abs() > 1e9 {
        return f64::NAN;
    }
    let mn = month.trunc().rem_euclid(12.0);
    days_from_civil(ym as i64, mn as i64 + 1, 1) as f64 + date.trunc() - 1.0
}

/// ECMA-262 MakeTime.
fn make_time(h: f64, m: f64, s: f64, ms: f64) -> f64 {
    if !(h.is_finite() && m.is_finite() && s.is_finite() && ms.is_finite()) {
        return f64::NAN;
    }
    ((h.trunc() * 60.0 + m.trunc()) * 60.0 + s.trunc()) * 1000.0 + ms.trunc()
}

/// ECMA-262 MakeDate.
fn make_date(day: f64, time: f64) -> f64 {
    let tv = day * MS_PER_DAY + time;
    if tv.is_finite() { tv } else { f64::NAN }
}

/// MakeDate(MakeDay(year, month, date), MakeTime(hours, minutes, seconds, ms)) of `f`.
fn make_full(f: [f64; 7]) -> f64 {
    make_date(
        make_day(f[0], f[1], f[2]),
        make_time(f[3], f[4], f[5], f[6]),
    )
}

/// [year, month 0..=11, date, hours, minutes, seconds, ms] of time value `t`, no zone applied.
fn split_time(t: f64) -> Option<[f64; 7]> {
    if !t.is_finite() {
        return None;
    }
    let day = (t / MS_PER_DAY).floor();
    let (y, m, d) = civil_from_days(day as i64);
    let w = t - day * MS_PER_DAY;
    Some([
        y as f64,
        (m - 1) as f64,
        d as f64,
        (w / 3_600_000.0).floor(),
        (w / 60_000.0).floor() % 60.0,
        (w / 1000.0).floor() % 60.0,
        w % 1000.0,
    ])
}

/// ECMA-262 WeekDay: 0 = Sunday.
fn week_day(t: f64) -> f64 {
    ((t / MS_PER_DAY).floor() + 4.0).rem_euclid(7.0)
}

/// The local zone's offset from UTC in ms at UTC time value `t`. A year chrono does not
/// hold has the offset of the same day and time 400-year cycles nearer: the zone's rule
/// for the years after (or before) its recorded changes is the same there.
fn local_offset(t: f64) -> f64 {
    let near = t - cycles_past_chrono(t);
    match chrono::DateTime::from_timestamp_millis(near as i64) {
        Some(d) => f64::from(crate::tz::local_ahead(d.timestamp())) * 1000.0,
        None => 0.0,
    }
}

/// The first instant, in seconds since the epoch, at which local clocks read `wall`; none
/// when they skip it.
fn local_instant(wall: &chrono::NaiveDateTime) -> Option<i64> {
    let read = wall.and_utc().timestamp();
    // Without a zone to read, local time is UTC.
    let Some(zone) = crate::tz::Database::system().local() else {
        return Some(read);
    };
    let instant = zone.instant_of(read);
    (instant + i64::from(zone.ahead_at(instant)) == read).then_some(instant)
}

/// ECMA-262 LocalTime.
fn local_time(t: f64) -> f64 {
    t + local_offset(t)
}

/// The instant QDateTime makes of the local clock reading `t` (see
/// [`crate::tz::Zone::instant_of`]): a reading the clocks show twice is the first of the
/// two, one they skip is read with the offset from before the skip. A reading in a year
/// chrono does not hold is looked up 400-year cycles nearer (see [`local_offset`]).
fn qdatetime_instant(t: f64) -> f64 {
    if !t.is_finite() {
        return f64::NAN;
    }
    let near = t - cycles_past_chrono(t);
    let read = (near / 1000.0).floor();
    match crate::tz::Database::system().local() {
        Some(zone) => t + (zone.instant_of(read as i64) as f64 - read) * 1000.0,
        // Without a zone to read, local time is UTC.
        None => t,
    }
}

/// V4's UTC(t) (qv4dateobject.cpp): the local clock reading `t` is taken back by the offset
/// that holds at `t` less the zone's standard time, so a reading the clocks skip or show
/// twice is told by the offset in force about then, as Qt 6.12 tells it.
fn utc_time(t: f64) -> f64 {
    if !t.is_finite() {
        return f64::NAN;
    }
    let standard = f64::from(crate::tz::local_standard_ahead()) * 1000.0;
    t - local_offset(t - standard)
}

/// Local [year, month, date, hours, minutes, seconds, ms] of time value `ms`.
fn fields(ms: f64) -> Option<[f64; 7]> {
    split_time(local_time(ms))
}

/// The time value of local time fields `f` (ECMA-262 `TimeClip(UTC(MakeDate(..)))`).
fn make_local(f: [f64; 7]) -> f64 {
    time_clip(utc_time(make_full(f)))
}

const DAY_NAMES: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTH_NAMES: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// ECMA-262 DateString (§21.4.4.41.2) of the day of `t`, a time value or a local clock
/// reading: `Sun Mar 10 2024`; a year before 0 as `-0005`.
/// The year as QDate prints it: it has no year 0 (1 BC is -1), four digits with the sign.
fn qt_year(y: f64) -> String {
    let y = y as i64;
    format!("{:04}", if y <= 0 { y - 1 } else { y })
}
/// `Tue Jan 2`: the day and month of V4's date texts (QDate's Qt::TextDate), `t` in the
/// zone it is shown in.
fn day_month(t: f64, f: &[f64; 7]) -> String {
    format!(
        "{} {} {}",
        DAY_NAMES[week_day(t) as usize],
        MONTH_NAMES[f[1] as usize],
        f[2] as i64
    )
}
fn clock(f: &[f64; 7]) -> String {
    format!("{:02}:{:02}:{:02}", f[3] as i64, f[4] as i64, f[5] as i64)
}
/// V4's `toDateString`: `Tue Jan 2 2024` (QDate::toString()).
fn date_part(t: f64) -> Option<String> {
    let f = split_time(t)?;
    Some(format!("{} {}", day_month(t, &f), qt_year(f[0])))
}
/// V4's `toTimeString`: `03:04:05` (QTime::toString()), without the zone.
fn time_part(ms: f64) -> Option<String> {
    Some(clock(&fields(ms)?))
}
/// V4's `toUTCString`: `Tue Jan 2 03:04:05 2024 GMT` (QDateTime::toString()).
fn utc_string(ms: f64) -> String {
    match split_time(ms) {
        Some(f) => format!("{} {} {} GMT", day_month(ms, &f), clock(&f), qt_year(f[0])),
        None => "Invalid Date".into(),
    }
}

pub fn new_date(it: &mut Interp, ms: f64) -> Value {
    let o = new_obj();
    let proto = match it.realm().borrow().vars.get("Date") {
        Some(Value::Obj(d)) => match d.borrow().props.get("prototype") {
            Some(Value::Obj(p)) => Some(p.clone()),
            _ => None,
        },
        _ => None,
    };
    o.borrow_mut().proto = proto;
    o.borrow_mut().internal = Internal::Date(ms);
    Value::Obj(o)
}

/// `Date.parse` in the system's zone. A clock reading in the ECMAScript format is an
/// instant by V4's UTC(t); one QDateTime read is the instant QDateTime makes of it, which
/// differ when the clocks skip the reading or show it twice.
pub(crate) fn parse_date_local(s: &str) -> f64 {
    let by_v4 = |d: chrono::NaiveDateTime| {
        let t = utc_time(d.and_utc().timestamp_millis() as f64);
        t.is_finite().then_some(t as i64)
    };
    let by_qdatetime = |d: chrono::NaiveDateTime| {
        Some(qdatetime_instant(d.and_utc().timestamp_millis() as f64) as i64)
    };
    parse_date_with(s, by_v4, by_qdatetime)
}

/// The ECMAScript date-time string format as V4's ParseString reads it (qv4dateobject.cpp):
/// `[±yy]yyyy[-MM[-dd]][THH[:mm[:ss[.zzz]]]][Z|±HH[:mm]]`, as (UTC milliseconds or a local
/// clock reading in them, whether it is local). Fields are combined by MakeDay and MakeTime,
/// so day 30 of February is in March; a date alone is UTC, a time without a zone is local.
fn ecma_date_text(s: &str) -> Option<(f64, bool)> {
    #[derive(Clone, Copy, PartialEq, PartialOrd)]
    enum At {
        Year,
        Month,
        Day,
        Hour,
        Minute,
        Second,
        Milli,
        ZoneHour,
        ZoneMinute,
        Done,
    }
    const ORDER: [At; 10] = [
        At::Year,
        At::Month,
        At::Day,
        At::Hour,
        At::Minute,
        At::Second,
        At::Milli,
        At::ZoneHour,
        At::ZoneMinute,
        At::Done,
    ];
    let next = |at: At| ORDER[(at as usize + 1).min(9)];
    // V4 walks the text up to and including the 0 that ends it.
    let mut units = s.encode_utf16().chain([0]).peekable();
    let sign = match units.peek() {
        Some(&c) if c == u16::from(b'+') || c == u16::from(b'-') => {
            units.next();
            Some(if c == u16::from(b'-') { -1.0 } else { 1.0 })
        }
        _ => None,
    };
    let (mut at, mut current, mut size) = (At::Year, 0.0, 0);
    let mut f = [0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0];
    let (mut ahead, mut ahead_sign) = (0.0, 1.0);
    let (mut seen_t, mut seen_z) = (false, false);
    for unit in units {
        if at == At::Done {
            break;
        }
        let ch = char::from_u32(u32::from(unit)).unwrap_or('\u{fffd}');
        if let Some(d) = ch.to_digit(10).filter(|_| ch.is_ascii()) {
            current = current * 10.0 + f64::from(d);
            size += 1;
            continue;
        }
        let ok = match at {
            At::Year => size == if sign.is_some() { 6 } else { 4 },
            At::Month => size == 2 && current <= 12.0,
            At::Day => size == 2 && current <= 31.0,
            At::Hour => size == 2 && current <= 24.0,
            At::Minute => size == 2 && current < 60.0,
            At::Second => size == 2 && current <= 60.0,
            At::Milli => size == 3,
            At::ZoneHour => size == 2 && current <= 23.0,
            At::ZoneMinute => size == 2 && current < 60.0,
            At::Done => true,
        };
        if !ok {
            return None;
        }
        match at {
            At::Month => f[1] = current - 1.0,
            At::ZoneHour => (ahead, seen_z) = (current * 60.0, true),
            At::ZoneMinute => ahead += current,
            At::Done => {}
            field => f[field as usize] = current,
        }
        match ch {
            'T' if at >= At::Hour => return None,
            'T' => (at, seen_t) = (At::Hour, true),
            '-' if at < At::Day => at = next(at),
            '-' if at < At::Minute || at >= At::ZoneHour => return None,
            '-' => (ahead_sign, at) = (-1.0, At::ZoneHour),
            ':' if !matches!(at, At::Hour | At::Minute | At::ZoneHour) => return None,
            ':' => at = next(at),
            '.' if at != At::Second => return None,
            '.' => at = next(at),
            '+' | 'Z' if seen_z || at < At::Minute || at >= At::ZoneHour => return None,
            '+' => at = At::ZoneHour,
            'Z' => (at, seen_z) = (At::Done, true),
            '\0' => at = At::Done,
            _ => {}
        }
        (current, size) = (0.0, 0);
    }
    f[0] *= sign.unwrap_or(1.0);
    let t = make_full(f);
    Some(match seen_z {
        true => (t - ahead * ahead_sign * 60_000.0, false),
        false => (t, seen_t),
    })
}

/// The formats V4 tries, in this order, on text that is neither an ECMAScript date-time nor
/// one of Qt's TextDate, ISODate and RFC2822Date forms (qv4dateobject.cpp, ParseString).
const DATE_FORMATS: [&str; 42] = [
    "M/d/yyyy",
    "M/d/yyyy hh:mm",
    "M/d/yyyy hh:mm A",
    "M/d/yyyy, hh:mm",
    "M/d/yyyy, hh:mm A",
    "MMM d yyyy",
    "MMM d yyyy hh:mm",
    "MMM d yyyy hh:mm:ss",
    "MMM d yyyy, hh:mm",
    "MMM d yyyy, hh:mm:ss",
    "MMMM d yyyy",
    "MMMM d yyyy hh:mm",
    "MMMM d yyyy hh:mm:ss",
    "MMMM d yyyy, hh:mm",
    "MMMM d yyyy, hh:mm:ss",
    "MMM d, yyyy",
    "MMM d, yyyy hh:mm",
    "MMM d, yyyy hh:mm:ss",
    "MMMM d, yyyy",
    "MMMM d, yyyy hh:mm",
    "MMMM d, yyyy hh:mm:ss",
    "d MMM yyyy",
    "d MMM yyyy hh:mm",
    "d MMM yyyy hh:mm:ss",
    "d MMM yyyy, hh:mm",
    "d MMM yyyy, hh:mm:ss",
    "d MMMM yyyy",
    "d MMMM yyyy hh:mm",
    "d MMMM yyyy hh:mm:ss",
    "d MMMM yyyy, hh:mm",
    "d MMMM yyyy, hh:mm:ss",
    "d MMM, yyyy",
    "d MMM, yyyy hh:mm",
    "d MMM, yyyy hh:mm:ss",
    "d MMMM, yyyy",
    "d MMMM, yyyy hh:mm",
    "d MMMM, yyyy hh:mm:ss",
    "yyyy-MM-dd hh:mm:ss t",
    "ddd, d MMM yyyy hh:mm:ss t",
    "ddd, d MMM yy hh:mm:ss t",
    "ddd, d MMM yyyy h:mm:ss t",
    "ddd, d MMM yy h:mm:ss t",
];

/// V4 `Date.parse` (ParseString): the ECMAScript date-time string format; failing that, what
/// QDateTime::fromString reads as Qt::TextDate (what `Date.toString` prints), Qt::ISODate or
/// Qt::RFC2822Date; failing that, the first of [`DATE_FORMATS`] the whole text fits, where a
/// format without a time is the start of that day in UTC and one with a time is local unless
/// the text names a zone. Nothing is trimmed; NaN otherwise. A local clock reading gives its
/// UTC milliseconds by `local_v4` when it is in the ECMAScript format and by `local` when
/// QDateTime read it.
pub(crate) fn parse_date_with(
    s: &str,
    local_v4: impl Fn(chrono::NaiveDateTime) -> Option<i64>,
    local: impl Fn(chrono::NaiveDateTime) -> Option<i64>,
) -> f64 {
    let local_ms = |clock: f64| {
        let clock = chrono::DateTime::from_timestamp_millis(time_clip(clock) as i64)?;
        local_v4(clock.naive_utc()).map(|ms| ms as f64)
    };
    if let Some((t, is_local)) = ecma_date_text(s) {
        return match is_local {
            true if t.is_finite() => local_ms(t).map_or(f64::NAN, time_clip),
            _ => time_clip(t),
        };
    }
    let found = QDateTime::from_text(s)
        .or_else(|| QDateTime::from_iso(s))
        .or_else(|| QDateTime::from_rfc2822(s))
        .or_else(|| {
            DATE_FORMATS.iter().find_map(|format| {
                let q = QDateTime::from_format(s, format)?;
                Some(match format.contains("h:mm") {
                    true => q,
                    false => QDateTime {
                        time: chrono::NaiveTime::MIN,
                        spec: TimeSpec::Utc,
                        ..q
                    },
                })
            })
        });
    let Some(q) = found else {
        return f64::NAN;
    };
    let clock = q.date.and_time(q.time);
    let utc = clock.and_utc().timestamp_millis();
    let ms = match q.spec {
        TimeSpec::Local => local(clock),
        TimeSpec::Utc => Some(utc),
        TimeSpec::Offset(ahead) => Some(utc - i64::from(ahead) * 1000),
    };
    ms.map_or(f64::NAN, |ms| time_clip(ms as f64))
}

/// ToDateString (ECMA-262 §21.4.4.41.4) of time value `ms`. V4 prints another order
/// (`Sun Mar 10 13:00:00 2024 GMT+0300`); `Date.parse` reads both.
/// V4's `Date.prototype.toString`: the local time as [`utc_string`] prints UTC, then the
/// offset from UTC unless it is zero (`Tue Jan 2 06:04:05 2024 GMT+0300`).
fn date_string(ms: f64) -> String {
    let t = local_time(ms);
    let Some(f) = split_time(t) else {
        return "Invalid Date".into();
    };
    let off = (local_offset(ms) / 60_000.0) as i64;
    let zone = match off {
        0 => String::new(),
        _ => format!(
            "{}{:02}{:02}",
            if off < 0 { '-' } else { '+' },
            off.abs() / 60,
            off.abs() % 60
        ),
    };
    format!(
        "{} {} {} GMT{zone}",
        day_month(t, &f),
        clock(&f),
        qt_year(f[0])
    )
}

fn date_ctor() -> Value {
    let f = fn_value(Callable::Native("Date", |it, _, a| {
        // Called as a function, Date ignores its arguments (ECMA-262 §21.4.2.1 step 1).
        if !std::mem::take(&mut it.constructing) {
            return Ok(Value::str(&date_string(now_ms())));
        }
        let ms = match a.len() {
            0 => now_ms(),
            1 => match &a[0] {
                v if date_ms(v).is_some() => date_ms(v).unwrap(),
                Value::Str(s) => parse_date_local(s),
                v => match it.to_primitive(v, Hint::Default)? {
                    Value::Str(s) => parse_date_local(&s),
                    p => it.to_numeric(&p)?,
                },
            },
            _ => make_local(full_year_fields(it, a)?),
        };
        Ok(new_date(it, time_clip(ms)))
    }));
    /// ToNumber of the (year, month[, date, hours, minutes, seconds, ms]) arguments in order,
    /// years 0..=99 meaning 1900..=1999 (ECMA-262 21.4.2.1 / 21.4.3.4).
    fn full_year_fields(it: &mut Interp, a: &[Value]) -> R<[f64; 7]> {
        let mut f = [f64::NAN, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0];
        for (i, v) in a.iter().take(7).enumerate() {
            f[i] = it.to_numeric(v)?;
        }
        if !f[0].is_nan() && (0.0..=99.0).contains(&f[0].trunc()) {
            f[0] = 1900.0 + f[0].trunc();
        }
        Ok(f)
    }
    let Value::Obj(fo) = &f else { unreachable!() };
    set_length(fo, 7);
    nat(fo, "now", 0, |_, _, _| Ok(Value::Num(now_ms())));
    nat(fo, "parse", 1, |it, _, a| {
        let s = it.to_string(&arg(a, 0))?;
        Ok(Value::Num(parse_date_local(&s)))
    });
    nat(fo, "UTC", 7, |it, _, a| {
        Ok(Value::Num(time_clip(make_full(full_year_fields(it, a)?))))
    });
    nat(fo, "fromLocaleDateString", 0, |it, _, a| {
        from_locale(it, a, LocaleParse::Date)
    });
    nat(fo, "fromLocaleTimeString", 0, |it, _, a| {
        from_locale(it, a, LocaleParse::Time)
    });
    nat(fo, "fromLocaleString", 0, |it, _, a| {
        from_locale(it, a, LocaleParse::DateTime)
    });
    // Every read of local time checks whether the system's zone has changed
    // (`tz::Database::local`), so there is no zone cache to drop.
    nat(fo, "timeZoneUpdated", 0, |it, _, a| {
        if !a.is_empty() {
            return it.throw("Error", "Locale: Date.timeZoneUpdated(): Invalid arguments");
        }
        Ok(Value::Undefined)
    });
    let p = new_obj();
    fn this_ms(it: &mut Interp, this: &Value) -> R<f64> {
        match date_ms(this) {
            Some(n) => Ok(n),
            None => it.throw("TypeError", "Type error"),
        }
    }
    fn store(this: &Value, ms: f64) -> Value {
        if let Value::Obj(o) = this {
            o.borrow_mut().internal = Internal::Date(ms);
        }
        Value::Num(ms)
    }
    macro_rules! getter {
        ($name:literal, $i:expr, $utc:expr) => {
            nat(&p, $name, 0, |it, this, _| {
                let ms = this_ms(it, this)?;
                let f = if $utc { split_time(ms) } else { fields(ms) };
                Ok(Value::Num(f.map(|f| f[$i]).unwrap_or(f64::NAN)))
            });
        };
    }
    getter!("getFullYear", 0, false);
    getter!("getMonth", 1, false);
    getter!("getDate", 2, false);
    getter!("getHours", 3, false);
    getter!("getMinutes", 4, false);
    getter!("getSeconds", 5, false);
    getter!("getMilliseconds", 6, false);
    getter!("getUTCFullYear", 0, true);
    getter!("getUTCMonth", 1, true);
    getter!("getUTCDate", 2, true);
    getter!("getUTCHours", 3, true);
    getter!("getUTCMinutes", 4, true);
    getter!("getUTCSeconds", 5, true);
    getter!("getUTCMilliseconds", 6, true);
    /// Date.prototype.set[UTC]{FullYear..Milliseconds} (ECMA-262 21.4.4.20-28): ToNumber of up
    /// to `n` arguments in order, then fields `first..` of the (local) time replaced.
    fn set_fields(
        it: &mut Interp,
        this: &Value,
        a: &[Value],
        first: usize,
        n: usize,
        utc: bool,
    ) -> R<Value> {
        let t = this_ms(it, this)?;
        let given = a.len().clamp(1, n);
        let mut v = [f64::NAN; 4];
        for (i, x) in v.iter_mut().enumerate().take(given) {
            *x = num(it, a, i)?;
        }
        let t = match (t.is_nan(), first == 0) {
            // V4 starts a date that is no time at the epoch for setFullYear alone.
            (true, true) if !utc => 0.0,
            _ if utc => t,
            _ => local_time(t),
        };
        let Some(mut f) = split_time(t) else {
            return Ok(Value::Num(f64::NAN));
        };
        f[first..first + given].copy_from_slice(&v[..given]);
        let u = make_full(f);
        Ok(store(this, time_clip(if utc { u } else { utc_time(u) })))
    }
    macro_rules! setter {
        ($name:literal, $i:expr, $n:expr, $utc:expr) => {
            nat(&p, $name, $n, |it, this, a| {
                set_fields(it, this, a, $i, $n, $utc)
            });
        };
    }
    setter!("setFullYear", 0, 3, false);
    setter!("setMonth", 1, 2, false);
    setter!("setDate", 2, 1, false);
    setter!("setHours", 3, 4, false);
    setter!("setMinutes", 4, 3, false);
    setter!("setSeconds", 5, 2, false);
    setter!("setMilliseconds", 6, 1, false);
    setter!("setUTCFullYear", 0, 3, true);
    setter!("setUTCMonth", 1, 2, true);
    setter!("setUTCDate", 2, 1, true);
    setter!("setUTCHours", 3, 4, true);
    setter!("setUTCMinutes", 4, 3, true);
    setter!("setUTCSeconds", 5, 2, true);
    setter!("setUTCMilliseconds", 6, 1, true);
    // Annex B getYear / setYear: years relative to 1900; setYear maps 0..=99 to 1900..=1999.
    nat(&p, "getYear", 0, |it, this, _| {
        let ms = this_ms(it, this)?;
        Ok(Value::Num(
            fields(ms).map(|f| f[0] - 1900.0).unwrap_or(f64::NAN),
        ))
    });
    nat(&p, "setYear", 1, |it, this, a| {
        let t = this_ms(it, this)?;
        let y = num(it, a, 0)?;
        let t = if t.is_nan() { 0.0 } else { local_time(t) };
        let Some(f) = split_time(t).filter(|_| !y.is_nan()) else {
            return Ok(store(this, f64::NAN));
        };
        let year = if (0.0..=99.0).contains(&y.trunc()) {
            1900.0 + y.trunc()
        } else {
            y
        };
        let day = make_day(year, f[1], f[2]);
        Ok(store(
            this,
            time_clip(utc_time(make_date(day, t.rem_euclid(MS_PER_DAY)))),
        ))
    });
    nat(&p, "getDay", 0, |it, this, _| {
        let ms = this_ms(it, this)?;
        Ok(Value::Num(if ms.is_finite() {
            week_day(local_time(ms))
        } else {
            f64::NAN
        }))
    });
    nat(&p, "getUTCDay", 0, |it, this, _| {
        let ms = this_ms(it, this)?;
        Ok(Value::Num(if ms.is_finite() {
            week_day(ms)
        } else {
            f64::NAN
        }))
    });
    nat(&p, "getTime", 0, |it, this, _| {
        Ok(Value::Num(this_ms(it, this)?))
    });
    nat(&p, "valueOf", 0, |it, this, _| {
        Ok(Value::Num(this_ms(it, this)?))
    });
    nat(&p, "setTime", 1, |it, this, a| {
        this_ms(it, this)?;
        let n = time_clip(num(it, a, 0)?);
        Ok(store(this, n))
    });
    // Minutes, with the seconds of an offset as their fraction (a zone's local mean time).
    nat(&p, "getTimezoneOffset", 0, |it, this, _| {
        let ms = this_ms(it, this)?;
        Ok(Value::Num(if ms.is_finite() {
            // No offset is 0, not -0.
            0.0 - local_offset(ms) / 60_000.0
        } else {
            f64::NAN
        }))
    });
    // ECMA-262 Date Time String Format; years outside 0..=9999 as ±YYYYYY. V4's RangeError
    // for a Date that is no time names the Date's text: `Invalid Date out of range`.
    nat(&p, "toISOString", 0, |it, this, _| {
        let ms = this_ms(it, this)?;
        let Some(f) = split_time(ms) else {
            return it.throw("RangeError", "Invalid Date out of range");
        };
        let y = f[0] as i64;
        let year = match y {
            0..=9999 => format!("{y:04}"),
            _ if y < 0 => format!("-{:06}", -y),
            _ => format!("+{y:06}"),
        };
        let [m, d, h, mi, s, ms] = [f[1] + 1.0, f[2], f[3], f[4], f[5], f[6]].map(|x| x as i64);
        Ok(Value::str(&format!(
            "{year}-{m:02}-{d:02}T{h:02}:{mi:02}:{s:02}.{ms:03}Z"
        )))
    });
    // Date.prototype.toJSON (ECMA-262 §21.4.4.37): `null` when ToPrimitive(this, number) is a
    // non-finite Number, else `this.toISOString()`; any receiver.
    nat(&p, "toJSON", 1, |it, this, _| {
        if matches!(this, Value::Undefined | Value::Null) {
            return it.no_object(this);
        }
        let o = it.to_object(this);
        if let Value::Num(n) = it.to_primitive(&o, Hint::Number)?
            && !n.is_finite()
        {
            return Ok(Value::Null);
        }
        let f = it.get(&o, "toISOString")?;
        if crate::eval::callable(&f).is_none() {
            return it.throw("TypeError", "Type error");
        }
        it.call(&f, o, &[])
    });
    // Date.prototype[@@toPrimitive] (§21.4.4.45): "default" prefers the string; non-writable.
    let to_primitive = native("[Symbol.toPrimitive]", 1, |it, this, a| {
        if !matches!(this, Value::Obj(_)) {
            return it.throw("TypeError", "Type error");
        }
        let string_first = match a.first() {
            Some(Value::Str(h)) if matches!(&**h, "string" | "default") => true,
            Some(Value::Str(h)) if &**h == "number" => false,
            _ => return it.throw("TypeError", "Type error"),
        };
        it.ordinary_to_primitive(this, string_first)
    });
    crate::eval::write_sym(
        &p,
        &crate::value::WellKnown::ToPrimitive.sym(),
        to_primitive,
        None,
        false,
        READONLY,
    );
    nat(&p, "toDateString", 0, |it, this, _| {
        let ms = this_ms(it, this)?;
        Ok(Value::str(&date_part(local_time(ms)).unwrap_or_default()))
    });
    // Qt's Date.toLocaleDateString(locale, format | Locale.FormatType).
    nat(&p, "toLocaleDateString", 0, |it, this, args| {
        let ms = this_ms(it, this)?;
        Ok(Value::str(&locale_date_string(ms, Parts::Date, args)))
    });
    // Qt's Date.toLocaleTimeString(locale, format | Locale.FormatType).
    nat(&p, "toLocaleTimeString", 0, |it, this, args| {
        let ms = this_ms(it, this)?;
        Ok(Value::str(&locale_date_string(ms, Parts::Time, args)))
    });
    nat(&p, "toLocaleString", 0, |it, this, args| {
        let ms = this_ms(it, this)?;
        Ok(Value::str(&locale_date_string(ms, Parts::DateTime, args)))
    });
    nat(&p, "toString", 0, |it, this, _| {
        let ms = this_ms(it, this)?;
        Ok(Value::str(&date_string(ms)))
    });
    nat(&p, "toTimeString", 0, |it, this, _| {
        let ms = this_ms(it, this)?;
        Ok(Value::str(&time_part(ms).unwrap_or_default()))
    });
    // toGMTString is the same function object as toUTCString (ECMA-262 B.2.3.1).
    let to_utc_string = native("toUTCString", 0, |it, this, _| {
        let ms = this_ms(it, this)?;
        Ok(Value::str(&utc_string(ms)))
    });
    set_hidden(&p, "toUTCString", to_utc_string.clone());
    set_hidden(&p, "toGMTString", to_utc_string);
    link_builtin_prototype(fo, &p);
    f
}

#[derive(Clone, Copy, PartialEq)]
enum LocaleParse {
    Date,
    Time,
    DateTime,
}

/// Qt's Date.fromLocale{Date,Time,}String(locale, string[, format | Locale.FormatType]), or
/// (string) in the system locale: QLocale::toDate / toTime / toDateTime. A date is midnight
/// UTC, a time is today in local time, a date-time is local unless the format has a zone.
fn from_locale(it: &mut Interp, a: &[Value], kind: LocaleParse) -> R<Value> {
    let (name, default): (&str, fn(Locale, Fmt) -> &'static str) = match kind {
        LocaleParse::Date => ("fromLocaleDateString", Locale::date_format),
        LocaleParse::Time => ("fromLocaleTimeString", Locale::time_format),
        LocaleParse::DateTime => ("fromLocaleString", Locale::date_time_format),
    };
    let (loc, system, s, fmt) = if let [Value::Str(s)] = a {
        let loc = Locale::system_time();
        (
            loc,
            true,
            s.to_string(),
            default(loc, Fmt::Long).to_string(),
        )
    } else {
        let loc = a.first().filter(|_| a.len() <= 3).and_then(locale_of);
        let Some(loc) = loc else {
            return it.throw("Error", format!("Locale: Date.{name}(): Invalid arguments"));
        };
        // QV4 reads the string with toQStringNoThrow: a throwing toString() reads as "".
        let s = it.to_string(&arg(a, 1)).unwrap_or_default();
        let fmt = match a.get(2) {
            None => default(loc, Fmt::Long).to_string(),
            Some(Value::Str(f)) => f.to_string(),
            Some(Value::Num(n)) => default(loc, Fmt::from_num(*n)).to_string(),
            Some(_) => {
                return it.throw(
                    "Error",
                    format!("Locale: Date.{name}(): Invalid datetime format"),
                );
            }
        };
        (loc, a.first().is_some_and(is_system_locale), s, fmt)
    };
    // The system locale's formats are LC_TIME's and its digits LC_NUMERIC's.
    let num = if system {
        Numeric::system()
    } else {
        loc.numeric()
    };
    let ms = match parse_locale_date(&s, &fmt, loc, &num, kind) {
        None => f64::NAN,
        Some(p) => {
            let time = make_time(p[3], p[4], p[5], p[6]);
            match kind {
                LocaleParse::Date => make_date(make_day(p[0], p[1], p[2]), 0.0),
                LocaleParse::Time => match fields(now_ms()) {
                    Some(d) => qdatetime_instant(make_date(make_day(d[0], d[1], d[2]), time)),
                    None => f64::NAN,
                },
                LocaleParse::DateTime => {
                    let t = make_date(make_day(p[0], p[1], p[2]), time);
                    match p[7] {
                        off if off.is_nan() => qdatetime_instant(t),
                        off => t - off,
                    }
                }
            }
        }
    };
    Ok(new_date(it, time_clip(ms)))
}

/// One element of a QDateTime format: a literal character or a field letter and its width.
enum DateTok {
    Lit(char),
    Field(char, usize),
}

/// Splits a QDateTime format the way [`qt_format_in`] reads it.
fn date_format_tokens(fmt: &str) -> Vec<DateTok> {
    let ch: Vec<char> = fmt.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < ch.len() {
        let c = ch[i];
        if c == '\'' {
            i += 1;
            if ch.get(i) == Some(&'\'') {
                out.push(DateTok::Lit('\''));
                i += 1;
                continue;
            }
            while i < ch.len() {
                if ch[i] == '\'' {
                    if ch.get(i + 1) == Some(&'\'') {
                        out.push(DateTok::Lit('\''));
                        i += 2;
                        continue;
                    }
                    break;
                }
                out.push(DateTok::Lit(ch[i]));
                i += 1;
            }
            i += 1;
            continue;
        }
        let mut n = 1;
        while i + n < ch.len() && ch[i + n] == c {
            n += 1;
        }
        let used = match c {
            'd' | 'M' => n.min(4),
            'y' if n >= 4 => 4,
            'y' if n >= 2 => 2,
            'h' | 'H' | 'm' | 's' => n.min(2),
            'z' if n >= 3 => 3,
            'z' => 1,
            'a' | 'A' if matches!(ch.get(i + 1), Some('p' | 'P')) => 2,
            'a' | 'A' => 1,
            't' => n.min(4),
            _ => {
                out.push(DateTok::Lit(c));
                i += 1;
                continue;
            }
        };
        out.push(DateTok::Field(c, used));
        i += used;
    }
    out
}

/// Stores `v` in `slot`; a field given twice must agree (QDateTimeParser).
fn put_field(slot: &mut Option<i64>, v: i64) -> Option<()> {
    match *slot {
        Some(old) if old != v => None,
        _ => {
            *slot = Some(v);
            Some(())
        }
    }
}

/// `min..=max` ASCII digits at `*pos`.
fn take_digits(s: &[char], pos: &mut usize, min: usize, max: usize) -> Option<(i64, usize)> {
    take_locale_digits(s, pos, min, max, &Locale::c().numeric())
}

/// `min..=max` digits at `*pos` as `num`'s locale reads them (QLocaleData::digitSequence:
/// its own digits and ASCII ones): their value and how many there are.
fn take_locale_digits(
    s: &[char],
    pos: &mut usize,
    min: usize,
    max: usize,
    num: &Numeric,
) -> Option<(i64, usize)> {
    let digits = num.digit_sequence(&s[*pos..], max);
    if digits.len() < min {
        return None;
    }
    let v = digits.iter().fold(0, |v, (d, _)| v * 10 + i64::from(*d));
    *pos += digits.last().map_or(0, |(_, end)| *end);
    Some((v, digits.len()))
}

/// The longest of `names` at `*pos`, compared case-insensitively; its index.
fn take_name<'a>(s: &[char], pos: &mut usize, names: impl Iterator<Item = &'a str>) -> Option<i64> {
    let mut best: Option<(i64, usize)> = None;
    for (i, name) in names.enumerate() {
        let k = name.chars().count();
        if k == 0 || *pos + k > s.len() || best.is_some_and(|(_, b)| b >= k) {
            continue;
        }
        let got: String = s[*pos..*pos + k].iter().collect();
        if got.to_lowercase() == name.to_lowercase() {
            best = Some((i as i64, k));
        }
    }
    let (i, k) = best?;
    *pos += k;
    Some(i)
}

/// A zone of format field `t` .. `tttt` at `*pos` as its offset from UTC in ms: `UTC` for
/// any width; `t` also `Z`, `GMT` and [`UTC`|`GMT`]±h[h][[:]mm]; `tt` [`UTC`]±hh[mm];
/// `ttt` [`UTC`]±hh[:mm]; `tttt` `GMT` and `UTC`±h[h][[:]mm]. Zone names are not known.
fn take_zone(s: &[char], pos: &mut usize, n: usize) -> Option<f64> {
    let rest = &s[*pos..];
    let starts =
        |p: &str| rest.len() >= p.len() && rest.iter().zip(p.chars()).all(|(a, b)| *a == b);
    let gmt = (n == 1 || n == 4) && starts("GMT");
    let prefix = if starts("UTC") || gmt { 3 } else { 0 };
    let offset = if gmt && n == 4 || prefix == 0 && n == 4 {
        None
    } else {
        zone_offset(&rest[prefix..], n, prefix > 0)
    };
    let (ms, len) = match offset {
        Some(o) => (o.0, prefix + o.1),
        None if prefix > 0 => (0.0, prefix),
        None if n == 1 && starts("Z") => (0.0, 1),
        None => return None,
    };
    *pos += len;
    Some(ms)
}

fn zone_offset(s: &[char], n: usize, prefixed: bool) -> Option<(f64, usize)> {
    let sign = match s.first()? {
        '+' => 1.0,
        '-' => -1.0,
        _ => return None,
    };
    let mut i = 1;
    let min_hours = if prefixed && (n == 1 || n == 4) { 1 } else { 2 };
    let (h, _) = take_digits(s, &mut i, min_hours, 2)?;
    let two = |i: usize| s.len() >= i + 2 && s[i..i + 2].iter().all(|c| c.is_ascii_digit());
    let mut m = 0;
    if n != 2 && s.get(i) == Some(&':') && two(i + 1) {
        i += 1;
        m = take_digits(s, &mut i, 2, 2)?.0;
    } else if n != 3 && two(i) {
        m = take_digits(s, &mut i, 2, 2)?.0;
    }
    if m > 59 {
        return None;
    }
    Some((sign * ((h * 60 + m) * 60_000) as f64, i))
}

/// QLocale::toDate / toTime / toDateTime of `s` in format `fmt`: [year, month 0..=11, date,
/// hours, minutes, seconds, ms, zone offset in ms (NaN: local)], or `None` when `s` does not
/// match exactly (QDateTimeParser rules: fixed widths, consistent repeated fields, a
/// weekday without a day picks that weekday nearest the 1st of the month, defaults
/// 1900-01-01 00:00).
fn parse_locale_date(
    s: &str,
    fmt: &str,
    loc: Locale,
    num: &Numeric,
    kind: LocaleParse,
) -> Option<[f64; 8]> {
    let toks = date_format_tokens(fmt);
    let field = |set: &str| {
        toks.iter()
            .any(|t| matches!(t, DateTok::Field(c, _) if set.contains(*c)))
    };
    if kind == LocaleParse::Date && field("hHmszaAt") || kind == LocaleParse::Time && field("dMyt")
    {
        return None;
    }
    let ampm = field("aA");
    let s: Vec<char> = s.chars().collect();
    let digits = |pos: &mut usize, min, max| take_locale_digits(&s, pos, min, max, num);
    let mut pos = 0;
    let [mut year, mut yy, mut month, mut day, mut wday] = [None; 5];
    let [mut hour, mut h12, mut minute, mut second, mut ms, mut pm] = [None; 6];
    let mut zone = None;
    for tok in &toks {
        match *tok {
            DateTok::Lit(c) => {
                let got = *s.get(pos)?;
                if !(got == c || c.is_whitespace() && got.is_whitespace()) {
                    return None;
                }
                pos += 1;
            }
            DateTok::Field(c, n) => match (c, n) {
                ('d', 1 | 2) => put_field(&mut day, digits(&mut pos, n, 2)?.0)?,
                ('M', 1 | 2) => put_field(&mut month, digits(&mut pos, n, 2)?.0)?,
                ('d', _) => {
                    let f = if n == 3 { Fmt::Short } else { Fmt::Long };
                    let names = (0..7).map(|d| loc.day_name(d, f, false));
                    put_field(&mut wday, take_name(&s, &mut pos, names)?)?
                }
                ('M', _) => {
                    let f = if n == 3 { Fmt::Short } else { Fmt::Long };
                    let names = (0..12).map(|m| loc.month_name(m, f, false));
                    put_field(&mut month, take_name(&s, &mut pos, names)? + 1)?
                }
                ('y', 2) => put_field(&mut yy, digits(&mut pos, 2, 2)?.0)?,
                ('y', _) => {
                    // A year behind a sign may have more than four digits
                    // (YearSignIso8601 in qtparsetemporal.cpp).
                    let (sign, most) = match num.sign_at(&s[pos..]) {
                        Some((minus, end)) => {
                            pos += end;
                            (if minus { -1 } else { 1 }, 10)
                        }
                        None => (0, 4),
                    };
                    let v = digits(&mut pos, 4, most)?.0;
                    // QDate has no year 0: year -1 is 1 BCE, astronomical year 0.
                    match (sign, v) {
                        (_, 0) => return None,
                        (-1, v) => put_field(&mut year, 1 - v)?,
                        (_, v) => put_field(&mut year, v)?,
                    }
                }
                ('h', _) if ampm => {
                    let v = digits(&mut pos, n, 2)?.0;
                    if !(1..=12).contains(&v) {
                        return None;
                    }
                    put_field(&mut h12, v)?
                }
                ('h' | 'H', _) => put_field(&mut hour, digits(&mut pos, n, 2)?.0)?,
                ('m', _) => put_field(&mut minute, digits(&mut pos, n, 2)?.0)?,
                ('s', _) => put_field(&mut second, digits(&mut pos, n, 2)?.0)?,
                ('z', 3) => put_field(&mut ms, digits(&mut pos, 3, 3)?.0)?,
                ('z', _) => {
                    let (v, k) = digits(&mut pos, 1, 3)?;
                    put_field(&mut ms, v * 10i64.pow(3 - k as u32))?
                }
                ('a' | 'A', _) => {
                    let texts = [loc.am_text(), loc.pm_text()].into_iter();
                    put_field(&mut pm, take_name(&s, &mut pos, texts)?)?
                }
                // A zone given twice: the last one counts.
                (_, _) => zone = Some(take_zone(&s, &mut pos, n)?),
            },
        }
    }
    if pos != s.len() {
        return None;
    }
    let year = match (year, yy) {
        (Some(y), Some(yy)) if y.rem_euclid(100) != yy => return None,
        (Some(y), _) => y,
        (None, Some(yy)) => 1900 + yy,
        (None, None) => 1900,
    };
    let month = month.unwrap_or(1);
    if !(1..=12).contains(&month) {
        return None;
    }
    let dim =
        days_from_civil(year + month / 12, month % 12 + 1, 1) - days_from_civil(year, month, 1);
    let dow = |d: i64| (days_from_civil(year, month, d) + 4).rem_euclid(7);
    let day = match (day, wday) {
        (Some(d), Some(w)) if (1..=dim).contains(&d) && dow(d) != w => return None,
        (Some(d), _) => d,
        // Qt's weekDayWithinMonth: the given weekday nearest the 1st, inside the month.
        (None, Some(w)) => {
            let diff = match w - dow(1) {
                d if d < -3 => d + 7,
                d if d > 3 => d - 7,
                d => d,
            };
            let d = 1 + diff;
            if d <= 0 { d + 7 } else { d }
        }
        (None, None) => 1,
    };
    let hour = match (pm, h12, hour) {
        (Some(pm), Some(h), hh) => {
            let h = h % 12 + 12 * pm;
            if hh.is_some_and(|hh| hh != h) {
                return None;
            }
            h
        }
        (Some(pm), None, Some(h)) if (h >= 12) != (pm == 1) => return None,
        (_, _, h) => h.unwrap_or(0),
    };
    let (minute, second) = (minute.unwrap_or(0), second.unwrap_or(0));
    if !(1..=dim).contains(&day) || hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    Some([
        year as f64,
        (month - 1) as f64,
        day as f64,
        hour as f64,
        minute as f64,
        second as f64,
        ms.unwrap_or(0) as f64,
        zone.unwrap_or(f64::NAN),
    ])
}

/// Date.toLocale{Date,Time,}String(locale?, format | Locale.FormatType) of the local date,
/// time or both at `ms`: the locale's own format and names; no locale means the system locale.
/// QLocale::system() formats by one of its own formats through the locale `LC_TIME` names
/// (QSystemLocale::query, qlocale_unix.cpp), which is not QLocale::system() to [`zone_text`];
/// it is that only with a format string.
fn locale_date_string(ms: f64, parts: Parts, args: &[Value]) -> String {
    let locale = args.first().and_then(locale_of);
    let system = locale.is_none() || args.first().is_some_and(is_system_locale);
    let text = matches!(args.get(1), Some(Value::Str(_)));
    let loc = match locale {
        Some(loc) if !system || text => loc,
        _ => Locale::system_time(),
    };
    let system = system && text;
    let Some(when) = When::at(ms) else {
        return String::new();
    };
    let stamp = match parts {
        Parts::Date => Stamp::Date(when.day),
        Parts::Time => Stamp::Time(when.time),
        Parts::DateTime => Stamp::DateTime(when),
    };
    let default = |kind: Fmt| match parts {
        Parts::Date => loc.date_format(kind),
        Parts::Time => loc.time_format(kind),
        Parts::DateTime => loc.date_time_format(kind),
    };
    let fmt = match args.get(1) {
        Some(Value::Str(s)) => s,
        Some(Value::Num(n)) => default(Fmt::from_num(*n)),
        _ => default(Fmt::Long),
    };
    qt_format_stamp(&stamp, fmt, loc, system)
}

/// QDateTime::toString format (C locale names).
pub fn qt_format(ms: f64, fmt: &str) -> String {
    qt_format_in(ms, fmt, Locale::c())
}

/// QLocale::toString(QDateTime, format) with `loc`'s day/month names and AM/PM texts.
pub fn qt_format_in(ms: f64, fmt: &str, loc: Locale) -> String {
    match When::at(ms) {
        Some(when) => qt_format_stamp(&Stamp::DateTime(when), fmt, loc, false),
        None => String::new(),
    }
}

/// A QDate: a day of the proleptic Gregorian calendar in any year a time value reaches
/// (chrono's dates end at the years ±262143).
#[derive(Clone, Copy)]
struct Day {
    /// Counted as ECMA-262 counts: the year before year 1 is 0.
    year: i64,
    /// 1..=12.
    month: u32,
    /// 1..=31.
    day: u32,
    /// 0 is Sunday.
    weekday: usize,
}

impl Day {
    /// The day of `t`, a time value or a local clock reading.
    fn of_ms(t: f64) -> Day {
        let (year, month, day) = civil_from_days((t / MS_PER_DAY).floor() as i64);
        Day {
            year,
            month: month as u32,
            day: day as u32,
            weekday: week_day(t) as usize,
        }
    }

    /// QDate::year(): QDate has no year 0, 1 BCE is its year -1.
    fn qt_year(&self) -> i64 {
        match self.year {
            y if y <= 0 => y - 1,
            y => y,
        }
    }
}

impl From<chrono::NaiveDate> for Day {
    fn from(d: chrono::NaiveDate) -> Day {
        use chrono::Datelike;
        Day {
            year: i64::from(d.year()),
            month: d.month(),
            day: d.day(),
            weekday: d.weekday().num_days_from_sunday() as usize,
        }
    }
}

/// The QTime of `t`, a time value or a local clock reading.
fn time_of_ms(t: f64) -> chrono::NaiveTime {
    let ms = t.rem_euclid(MS_PER_DAY) as u32;
    chrono::NaiveTime::from_num_seconds_from_midnight_opt(ms / 1000, ms % 1000 * 1_000_000)
        .expect("a time within one day")
}

/// A QDateTime with the instant it names: its seconds since the epoch and how far ahead of
/// UTC its zone is then.
#[derive(Clone, Copy)]
struct When {
    day: Day,
    time: chrono::NaiveTime,
    spec: TimeSpec,
    secs: i64,
    ahead: i32,
}

impl When {
    /// The local date and time `ms` after the epoch.
    fn at(ms: f64) -> Option<When> {
        if !ms.is_finite() {
            return None;
        }
        let ahead = local_offset(ms);
        Some(When {
            day: Day::of_ms(ms + ahead),
            time: time_of_ms(ms + ahead),
            spec: TimeSpec::Local,
            secs: (ms / 1000.0).floor() as i64,
            ahead: (ahead / 1000.0) as i32,
        })
    }

    fn of(q: QDateTime) -> Option<When> {
        let wall = q.date.and_time(q.time);
        let (secs, ahead) = match q.spec {
            TimeSpec::Local => {
                let secs = local_instant(&wall)?;
                (secs, (wall.and_utc().timestamp() - secs) as i32)
            }
            TimeSpec::Utc => (wall.and_utc().timestamp(), 0),
            TimeSpec::Offset(ahead) => (wall.and_utc().timestamp() - i64::from(ahead), ahead),
        };
        Some(When {
            day: q.date.into(),
            time: q.time,
            spec: q.spec,
            secs,
            ahead,
        })
    }

    /// toOffsetString: `±hh`, `sep`, `mm`.
    fn offset(&self, sep: &str) -> String {
        let (sign, ahead) = match self.ahead {
            a if a < 0 => ('-', -a),
            a => ('+', a),
        };
        format!("{sign}{:02}{sep}{:02}", ahead / 3600, ahead / 60 % 60)
    }

    /// The offset as format fields `tt` and `ttt` print it (QTimeZonePrivate::
    /// isoOffsetFormat): [`When::offset`], and the seconds of an offset that has them (a
    /// zone's local mean time) after another `sep`.
    fn offset_field(&self, sep: &str) -> String {
        match self.ahead.abs() % 60 {
            0 => self.offset(sep),
            secs => format!("{}{sep}{secs:02}", self.offset(sep)),
        }
    }
}

/// What Qt's date formatter is given: a QDateTime, a QDate or a QTime.
#[derive(Clone, Copy)]
enum Stamp {
    DateTime(When),
    Date(Day),
    Time(chrono::NaiveTime),
}

/// QTimeZone::systemTimeZone(), whose names and offsets local time is printed with.
enum SystemZone {
    /// The zone of the database that the system's id names, whatever zone the clocks go by.
    Named(String, std::sync::Arc<crate::tz::Zone>),
    /// With no id at all Qt 6.12 builds the system zone from a pointer that converts to 1
    /// (`QTimeZone(global_tz->backend)`, qtimezone.cpp): a zone so many seconds ahead of UTC,
    /// whose names are all its id (QUtcTimeZonePrivate, qtimezoneprivate.cpp).
    Ahead(i32),
    /// An id that is no zone of the database: an invalid QTimeZone, which has no names.
    Invalid,
}

impl SystemZone {
    fn now() -> SystemZone {
        let id = crate::osutil::system_zone_id();
        if id.is_empty() {
            return SystemZone::Ahead(1);
        }
        let database = crate::tz::Database::system();
        match database.zone(&id) {
            Some(zone) => SystemZone::Named(database.utc_id(&id).unwrap_or(id), zone),
            None => SystemZone::Invalid,
        }
    }
}

/// A `t` field of `repeat` letters for `when` (tzAbbr in QCalendarBackend::dateTimeToString):
/// the zone's short name, its offset without and with a colon, its long name. `system` is
/// QLocale::system().
///
/// Local time's fields are [`SystemZone`]'s (QTimeZone::displayName): its abbreviation, its
/// offset and whether it is in daylight-saving time at `when`. Two cases are not. In
/// QLocale::system() the short name is QDateTime::timeZoneAbbreviation, the C library's, and
/// the offset the one the clocks go by, without its seconds (zoneOffsetFormat of a Qt with
/// ICU, qlocale.cpp). And a zone with no names leaves the same two. A zone that is an offset
/// from UTC (`TZ=UTC+3`) is named by its id, `UTC+03:00` (QUtcTimeZonePrivate).
///
/// A short name of six UTF-16 units that begins with `UTC` gets `:00`: ICU's `UTC+11` and
/// `UTC−10` (fr_FR, fi_FI) are written `UTC+11:00` and `UTC−10:00`, `UTC+9` as it is.
fn zone_text(when: &When, repeat: usize, loc: Locale, system: bool) -> String {
    let mut text = zone_field(when, repeat, loc, system);
    if !(2..=4).contains(&repeat) && text.starts_with("UTC") && text.encode_utf16().count() == 6 {
        text += ":00";
    }
    text
}

fn zone_field(when: &When, repeat: usize, loc: Locale, system: bool) -> String {
    let zone = match when.spec {
        TimeSpec::Local if repeat == 4 || !system => SystemZone::now(),
        _ => SystemZone::Invalid,
    };
    let offset = |sep| {
        let ahead = match &zone {
            SystemZone::Named(_, zone) => zone.ahead_at(when.secs),
            SystemZone::Ahead(ahead) => *ahead,
            SystemZone::Invalid => when.ahead,
        };
        let when = When { ahead, ..*when };
        // QLocale::system() writes hours and minutes only (zoneOffsetFormat).
        if system {
            when.offset(sep)
        } else {
            when.offset_field(sep)
        }
    };
    // A zone's long name is ICU's, asked for QLocale::name(), which the system locale takes
    // from its fallback locale.
    let long = if system { Locale::SYSTEM } else { loc };
    let long_name = |id: &str, daylight| long.icu_zone_name(icu_default_id, id, daylight, true);
    match (repeat, when.spec) {
        (2, _) => offset(""),
        (3, _) => offset(":"),
        (_, TimeSpec::Offset(_)) => format!("UTC{}", when.offset(":")),
        (4, TimeSpec::Utc) => long_name("UTC", false).unwrap_or("UTC").into(),
        (_, TimeSpec::Utc) => "UTC".into(),
        (repeat, TimeSpec::Local) => match zone {
            SystemZone::Named(id, zone) => {
                let (abbreviation, daylight) = zone.name_at(when.secs);
                let named = if repeat == 4 {
                    long_name(&id, daylight)
                } else {
                    loc.icu_zone_name(icu_default_id, &id, daylight, false)
                };
                match (repeat, named) {
                    (4, Some(name)) => name.into(),
                    (_, Some(name)) if repeat != 4 && !loc.is_anglic() => name.into(),
                    // A zone that is an offset from UTC has its id for both names
                    // (QUtcTimeZonePrivate).
                    _ if repeat == 4 || abbreviation.is_empty() => id,
                    _ => abbreviation.into(),
                }
            }
            SystemZone::Ahead(ahead) => crate::tz::offset_id(ahead),
            SystemZone::Invalid => crate::osutil::zone_abbreviation(when.secs).0,
        },
    }
}

/// setlocale(LC_MESSAGES, NULL) in a Qt application, which has called setlocale(LC_ALL, "")
/// (QCoreApplicationPrivate::initLocale): the name the environment gives the locale of
/// messages when the C library has every locale the environment names, else "C", as that
/// call then changes nothing.
fn libc_messages_name() -> String {
    // SAFETY: `c""` is NUL-terminated and newlocale(3) allows a null base (a new object).
    let all = unsafe { libc::newlocale(libc::LC_ALL_MASK, c"".as_ptr(), std::ptr::null_mut()) };
    if all.is_null() {
        return "C".into();
    }
    // SAFETY: `all` came from newlocale and nothing uses it.
    unsafe { libc::freelocale(all) };
    let set = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    let name = set("LC_ALL").or_else(|| set("LC_MESSAGES"));
    name.or_else(|| set("LANG")).unwrap_or("C".into())
}

/// ICU's default locale, uprv_getDefaultLocaleID (putil.cpp, the same in ICU 73.2 and 78.3).
/// It is the C library's locale of messages; when that is C or POSIX, LC_ALL, else
/// LC_MESSAGES, else LANG, the first one that is set, empty or not; and with none set, or
/// with C or POSIX there, en_US_POSIX. The name is cut at its charset and its modifier
/// (`C.UTF-8` is en_US_POSIX too), and the modifier is added as a variant: `aa_BB_modifier`,
/// `aa__modifier`.
fn icu_default_id() -> String {
    let posix = |id: &str| id == "C" || id == "POSIX";
    let id = Some(libc_messages_name())
        .filter(|id| !posix(id))
        .or_else(|| {
            ["LC_ALL", "LC_MESSAGES", "LANG"]
                .iter()
                .find_map(std::env::var_os)
                .map(|v| v.to_string_lossy().into_owned())
        });
    let id = id.filter(|id| !posix(id)).unwrap_or("en_US_POSIX".into());
    let mut corrected = id.split(['.', '@']).next().unwrap_or("").to_string();
    if posix(&corrected) {
        corrected = "en_US_POSIX".into();
    }
    if let Some((_, modifier)) = id.rsplit_once('@') {
        corrected += if corrected.contains('_') { "_" } else { "__" };
        corrected += match modifier.split('.').next().unwrap_or("") {
            "nynorsk" => "NY",
            modifier => modifier,
        };
    }
    corrected
}

/// QLocale::toString(QDateTime | QDate | QTime, format) in `loc`
/// (QCalendarBackend::dateTimeToString): a QDate leaves the time fields as written, a QTime
/// the date fields, and a QTime's `t` is the local zone now.
fn qt_format_stamp(stamp: &Stamp, fmt: &str, loc: Locale, system: bool) -> String {
    use chrono::Timelike;
    let (date, time) = match stamp {
        Stamp::DateTime(w) => (Some(w.day), Some(w.time)),
        Stamp::Date(d) => (Some(*d), None),
        Stamp::Time(t) => (None, Some(*t)),
    };
    let ampm = {
        let mut q = false;
        fmt.chars().any(|c| {
            if c == '\'' {
                q = !q;
            }
            !q && (c == 'a' || c == 'A')
        })
    };
    // The numbers are in the locale's digits (QLocaleData::longLongToString). The system
    // locale takes its digits and signs from `LC_NUMERIC` (QSystemLocale::query).
    let num = if system {
        Numeric::system()
    } else {
        loc.numeric()
    };
    let ch: Vec<char> = fmt.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < ch.len() {
        let c = ch[i];
        if c == '\'' {
            i += 1;
            if i < ch.len() && ch[i] == '\'' {
                out.push('\'');
                i += 1;
                continue;
            }
            while i < ch.len() {
                if ch[i] == '\'' {
                    if i + 1 < ch.len() && ch[i + 1] == '\'' {
                        out.push('\'');
                        i += 2;
                        continue;
                    }
                    break;
                }
                out.push(ch[i]);
                i += 1;
            }
            i += 1;
            continue;
        }
        let mut n = 1;
        while i + n < ch.len() && ch[i + n] == c {
            n += 1;
        }
        let padded = |v: u32, width: usize| num.integer(i64::from(v), width);
        let mut used = None;
        if let Some(d) = date {
            let wd = d.weekday;
            let year = d.qt_year();
            match c {
                'y' if n >= 4 => {
                    used = Some(4);
                    // A year of more than four digits has a sign.
                    if year > 9999 {
                        out += num.plus;
                    }
                    out += &num.integer(year, if year < 0 { 5 } else { 4 });
                }
                'y' if n >= 2 => {
                    used = Some(2);
                    out += &num.integer(year % 100, 2);
                }
                'y' => {
                    used = Some(1);
                    out.push('y');
                }
                'M' => {
                    let n = n.min(4);
                    used = Some(n);
                    out += &match n {
                        1 | 2 => padded(d.month, n),
                        3 => loc
                            .month_name(d.month as usize - 1, Fmt::Short, false)
                            .into(),
                        _ => loc
                            .month_name(d.month as usize - 1, Fmt::Long, false)
                            .into(),
                    };
                }
                'd' => {
                    let n = n.min(4);
                    used = Some(n);
                    out += &match n {
                        1 | 2 => padded(d.day, n),
                        3 => loc.day_name(wd, Fmt::Short, false).into(),
                        _ => loc.day_name(wd, Fmt::Long, false).into(),
                    };
                }
                _ => {}
            }
        }
        if let (None, Some(t)) = (used, time) {
            let two = n.min(2);
            match c {
                'h' | 'H' => {
                    let hour = match t.hour() {
                        h if c == 'H' || !ampm => h,
                        0 => 12,
                        h if h > 12 => h - 12,
                        h => h,
                    };
                    used = Some(two);
                    out += &padded(hour, two);
                }
                'm' => {
                    used = Some(two);
                    out += &padded(t.minute(), two);
                }
                's' => {
                    used = Some(two);
                    out += &padded(t.second(), two);
                }
                'z' => {
                    let z = padded(t.nanosecond() / 1_000_000 % 1000, 3);
                    if n >= 3 {
                        used = Some(3);
                        out += &z;
                    } else {
                        // Up to two trailing zeros go.
                        used = Some(n);
                        let mut z = z.as_str();
                        for _ in 0..2 {
                            z = z.strip_suffix(num.zero).unwrap_or(z);
                        }
                        out += z;
                    }
                }
                'a' | 'A' => {
                    let s = if t.hour() < 12 {
                        loc.am_text()
                    } else {
                        loc.pm_text()
                    };
                    let next = ch.get(i + 1).copied();
                    used = Some(if matches!(next, Some('p') | Some('P')) {
                        2
                    } else {
                        1
                    });
                    // "Ap" / "aP" keep the locale's own case.
                    out += &match (c, next) {
                        ('A', Some('p')) | ('a', Some('P')) => s.into(),
                        ('a', _) => s.to_lowercase(),
                        _ => s.to_uppercase(),
                    };
                }
                't' => {
                    let n = n.min(4);
                    used = Some(n);
                    let when = match stamp {
                        Stamp::DateTime(w) => Some(*w),
                        _ => When::at(now_ms()),
                    };
                    if let Some(when) = when {
                        out += &zone_text(&when, n, loc, system);
                    }
                }
                _ => {}
            }
        }
        i += used.unwrap_or_else(|| {
            out.extend(std::iter::repeat_n(c, n));
            n
        });
    }
    out
}

/// QDate, QTime or QDateTime `toString(Qt::DateFormat)`; a number that is no text format of
/// the enum gives nothing (formatDateTimeObjectUsingDateFormat). Qt::TextDate ends in the
/// year as C's `%04d` prints it (`275760`, `-271822`, `0005`, `-006`), where the `yyyy`
/// field has `+275760` and `-0006`.
fn qt_format_enum(stamp: &Stamp, format: i32) -> String {
    let text = |fmt: &str| qt_format_stamp(stamp, fmt, Locale::c(), false);
    let iso_date = |d: &Day| (0..=9999).contains(&d.year);
    match (format, stamp) {
        (0, Stamp::Date(d)) => text("ddd MMM d ") + &format!("{:04}", d.qt_year()),
        (1 | 9, Stamp::Date(d)) if iso_date(d) => text("yyyy-MM-dd"),
        (8, Stamp::Date(_)) => text("dd MMM yyyy"),
        (0 | 1 | 8, Stamp::Time(_)) => text("HH:mm:ss"),
        (9, Stamp::Time(_)) => text("HH:mm:ss.zzz"),
        (0, Stamp::DateTime(w)) => {
            text("ddd MMM d HH:mm:ss ")
                + &format!("{:04}", w.day.qt_year())
                + &match w.spec {
                    TimeSpec::Local => String::new(),
                    TimeSpec::Utc => " GMT".into(),
                    TimeSpec::Offset(_) => format!(" GMT{}", w.offset("")),
                }
        }
        (1 | 9, Stamp::DateTime(w)) if iso_date(&w.day) => {
            text(if format == 9 {
                "yyyy-MM-ddTHH:mm:ss.zzz"
            } else {
                "yyyy-MM-ddTHH:mm:ss"
            }) + &match w.spec {
                TimeSpec::Local => String::new(),
                TimeSpec::Utc => "Z".into(),
                TimeSpec::Offset(_) => w.offset(":"),
            }
        }
        (8, Stamp::DateTime(w)) => text("dd MMM yyyy HH:mm:ss ") + &w.offset(""),
        _ => String::new(),
    }
}

/// Which of QDate, QTime and QDateTime a `Qt.format*` function formats.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Parts {
    Date,
    Time,
    DateTime,
}

/// DateObject::dateTimeToDate: the day of the local date-time at `ms`; one that is the UTC
/// start of the next day (a Date parsed from a text with no time) is that day.
fn date_of(ms: f64) -> Option<Day> {
    if !ms.is_finite() {
        return None;
    }
    let day = |t: f64| (t / MS_PER_DAY).floor();
    let (local, utc, before) = (day(local_time(ms)), day(ms), day(ms - 1000.0));
    Some(Day::of_ms(if utc != local && before == local {
        ms
    } else {
        local_time(ms)
    }))
}

/// The value `Qt.formatDate`, `formatTime` or `formatDateTime` (`name`) formats: a Date, or
/// the text of anything else read as Qt reads it (dateFromString, timeFromString,
/// dateTimeFromString): ISO 8601 first, then as `Date.parse`. None is an invalid Date.
fn stamp_arg(it: &mut Interp, name: &str, parts: Parts, v: &Value) -> R<Option<Stamp>> {
    let of_ms = |ms: f64| match parts {
        Parts::Date => date_of(ms).map(Stamp::Date),
        Parts::Time => When::at(ms).map(|when| Stamp::Time(when.time)),
        Parts::DateTime => When::at(ms).map(Stamp::DateTime),
    };
    if let Some(ms) = date_ms(v) {
        return Ok(of_ms(ms));
    }
    let s = it.to_string(v)?;
    let iso = QDateTime::from_iso(&s);
    let stamp = match parts {
        Parts::Date => crate::qdatetime::date_from_iso(&s)
            .or(iso.map(|q| q.date))
            .map(|date| Stamp::Date(date.into())),
        Parts::Time => crate::qdatetime::time_from_iso(&s)
            .or(iso.map(|q| q.time))
            .map(Stamp::Time),
        Parts::DateTime => iso.and_then(When::of).map(Stamp::DateTime),
    };
    match stamp.or_else(|| of_ms(parse_date_local(&s))) {
        Some(stamp) => Ok(Some(stamp)),
        None => it.throw("Error", format!("Invalid argument passed to {name}(): {s}")),
    }
}

/// `Qt.formatDate`, `Qt.formatTime` and `Qt.formatDateTime` (`name`), by QtObject's overloads:
/// `(value)` in the system locale's short format, `(value, locale [, Locale.FormatType])`,
/// `(value, Qt.DateFormat)`, and `(value, format)` with a format text, read in the C locale.
fn qt_format_call(it: &mut Interp, name: &str, parts: Parts, a: &[Value]) -> R<Value> {
    enum How {
        Locale(Locale, Fmt),
        Enum(i32),
        Text(Rc<str>),
    }
    if a.is_empty() {
        let own = match parts {
            Parts::Date => Some("QDate"),
            Parts::Time => Some("QTime"),
            Parts::DateTime => None,
        };
        let values: Vec<&str> = ["QString", "QDateTime"].into_iter().chain(own).collect();
        let mut text = "Unable to determine callable overload.  Candidates are:".to_string();
        for value in &values {
            for rest in ["", ",QLocale", ",QLocale,QLocale::FormatType"] {
                text += &format!("\n    {name}({value}{rest})");
            }
        }
        for rest in ["Qt::DateFormat", "QString"] {
            for value in &values {
                text += &format!("\n    {name}({value},{rest})");
            }
        }
        return it.throw("Error", &text);
    }
    if a.len() > 3 {
        return it.throw("Error", "Too many arguments");
    }
    let incompatible =
        "Passing incompatible arguments to C++ functions from JavaScript is not allowed.";
    let how = match (arg(a, 1), a.get(2)) {
        (Value::Undefined, None) => How::Locale(Locale::system_time(), Fmt::Short),
        (Value::Str(s), None) => How::Text(s),
        (v @ (Value::Num(_) | Value::Bool(_)), None) => {
            How::Enum(crate::eval::to_i32(v.to_number()))
        }
        (v, kind) if locale_of(&v).is_some() => {
            let kind = match kind {
                None => Fmt::Short,
                Some(k) => match crate::eval::to_i32(it.to_numeric(k)?) {
                    0 => Fmt::Long,
                    2 => Fmt::Narrow,
                    _ => Fmt::Short,
                },
            };
            let loc = match locale_of(&v) {
                Some(loc) if !is_system_locale(&v) => loc,
                _ => Locale::system_time(),
            };
            How::Locale(loc, kind)
        }
        _ => return it.throw("TypeError", incompatible),
    };
    let Some(stamp) = stamp_arg(it, name, parts, &arg(a, 0))? else {
        return Ok(Value::str(""));
    };
    Ok(Value::str(&match how {
        // A locale's own format: see [`locale_date_string`] for the system's.
        How::Locale(loc, kind) => {
            let fmt = match parts {
                Parts::Date => loc.date_format(kind),
                Parts::Time => loc.time_format(kind),
                Parts::DateTime => loc.date_time_format(kind),
            };
            qt_format_stamp(&stamp, fmt, loc, false)
        }
        How::Enum(format) => qt_format_enum(&stamp, format),
        How::Text(fmt) => qt_format_stamp(&stamp, &fmt, Locale::c(), false),
    }))
}

/// Qt's QML Number extension (QQmlNumberExtension): `toLocaleString([locale [, format
/// [, precision]]])` and `toLocaleCurrencyString([locale [, symbol]])` on any `this`.
fn number_locale(it: &mut Interp, this: &Value, name: &str, a: &[Value]) -> R<Value> {
    let currency = name == "toLocaleCurrencyString";
    if a.len() > if currency { 2 } else { 3 } {
        return it.throw(
            "Error",
            format!("Locale: Number.{name}(): Invalid arguments"),
        );
    }
    let n = it.to_numeric(this)?;
    if a.is_empty() {
        return Ok(Value::str(&Numeric::system().to_string(n, b'g', 6)));
    }
    if locale_of(&a[0]).is_none() {
        if currency {
            return it.throw(
                "Error",
                format!("Locale: Number.{name}(): Invalid arguments"),
            );
        }
        // %Number.prototype.toLocaleString% (ECMA-262 §21.1.3.4).
        return match this {
            Value::Num(n) => Ok(Value::str(&num_to_string(*n))),
            _ => it.throw("TypeError", "Type error"),
        };
    }
    let locale = locale_numeric(&a[0]);
    let invalid = "Locale: Number.toLocaleString(): Invalid arguments";
    if currency {
        let symbol = match a.get(1) {
            None => None,
            Some(Value::Str(s)) => Some(s.clone()),
            Some(_) => return it.throw("Error", invalid),
        };
        return Ok(Value::str(&locale.to_currency_string(n, symbol.as_deref())));
    }
    // Qt takes the low byte of the format's first UTF-16 unit as a `char`.
    let format = match a.get(1) {
        None => b'f',
        Some(Value::Str(s)) => s.encode_utf16().next().map_or(b'f', |u| u as u8),
        Some(_) => return it.throw("Error", invalid),
    };
    let precision = match a.get(2) {
        None => 2,
        Some(Value::Num(p)) => crate::eval::to_i32(*p),
        Some(_) => return it.throw("Error", invalid),
    };
    Ok(Value::str(&locale.to_string(n, format, precision)))
}

/// The date/time data behind a `Qt.locale()` object.
pub(crate) fn locale_of(v: &Value) -> Option<Locale> {
    match v {
        Value::Obj(o) => match o.borrow().internal {
            Internal::Locale(n, _) => Some(Locale(n)),
            _ => None,
        },
        _ => None,
    }
}

/// Whether `v` is the system locale's object: `Qt.locale()` without a name.
fn is_system_locale(v: &Value) -> bool {
    matches!(v, Value::Obj(o) if matches!(o.borrow().internal, Internal::Locale(_, true)))
}

/// QLocale::system().name(): the locale QSystemLocale falls back to, which LC_ALL, then
/// LC_MESSAGES, then LANG names (and LANGUAGE's first entry in their place unless they name
/// the C locale); "C" when none is set.
pub(crate) fn system_locale_name() -> String {
    Locale::SYSTEM.name().to_string()
}

/// The number data of a QML Locale object under its `numberOptions`.
fn locale_numeric(this: &Value) -> Numeric {
    let loc = locale_of(this).unwrap_or(Locale::SYSTEM);
    let options = match this {
        Value::Obj(o) => match o.borrow().props.get("numberOptions") {
            Some(Value::Num(n)) => Some(*n as u32),
            _ => None,
        },
        _ => None,
    };
    let numeric = loc.numeric();
    match options {
        Some(o) => numeric.with_options(o),
        None => numeric,
    }
}

/// QLocale::operator== for two QML Locale objects: the same locale and number options.
/// `None` when either is no Locale.
pub(crate) fn locale_equal(a: &Obj, b: &Obj) -> Option<bool> {
    let (a, b) = (Value::Obj(a.clone()), Value::Obj(b.clone()));
    let (la, lb) = (locale_of(&a)?, locale_of(&b)?);
    Some(la == lb && locale_numeric(&a).options() == locale_numeric(&b).options())
}

/// The QML Locale type (QQmlLocaleValueType): `Qt.locale(name)`, or the system locale for
/// no or an undefined name. Anything else that is no string is the C locale, as "" is.
pub(crate) fn locale_object(name: &Value) -> Value {
    let o = new_obj();
    let loc = match name {
        Value::Undefined => Locale::SYSTEM,
        Value::Str(s) => Locale::named(s),
        _ => Locale::c(),
    };
    o.borrow_mut().internal = Internal::Locale(loc.0, loc == Locale::SYSTEM);
    let numeric = loc.numeric();
    let [decimal, group, minus, plus, exponential, zero] = numeric.symbols();
    let text = |s: &str| Value::str(s);
    let list = crate::valuetypes::readonly_sequence;
    // In the order Qt lists them.
    for (k, v) in [
        ("firstDayOfWeek", Value::Num(loc.first_day_of_week() as f64)),
        (
            "measurementSystem",
            Value::Num(loc.measurement_system() as f64),
        ),
        ("textDirection", Value::Num(loc.text_direction() as f64)),
        (
            "weekDays",
            list(loc.week_days().map(|d| Value::Num(d as f64)).collect()),
        ),
        ("uiLanguages", list(loc.ui_languages().map(text).collect())),
        ("name", text(loc.name())),
        ("nativeLanguageName", text(loc.native_language_name())),
        ("nativeCountryName", text(loc.native_territory_name())),
        ("nativeTerritoryName", text(loc.native_territory_name())),
        ("decimalPoint", text(&decimal)),
        ("groupSeparator", text(&group)),
        ("percent", text(loc.percent())),
        ("zeroDigit", text(&zero)),
        ("negativeSign", text(&minus)),
        ("positiveSign", text(&plus)),
        ("exponential", text(&exponential)),
        ("amText", text(loc.am_text())),
        ("pmText", text(loc.pm_text())),
        ("numberOptions", Value::Num(numeric.options() as f64)),
    ] {
        set_own(&o, k, v);
    }
    Value::Obj(o)
}

/// What a write to property `k` of a QML Locale does: only `numberOptions` can be set.
/// The others are read-only (Qt names the property's C++ type in the error), and a name
/// the type does not have is refused.
pub(crate) fn locale_put(it: &mut Interp, o: &Obj, k: &str, v: Value, throw: bool) -> R<bool> {
    let cpp = match k {
        "numberOptions" => {
            return match v {
                Value::Num(n) => {
                    // QVariant's double to int conversion rounds.
                    let n = crate::eval::to_i32(n.round());
                    o.borrow_mut()
                        .props
                        .insert(k.into(), Value::Num(f64::from(n)));
                    Ok(true)
                }
                v => it.throw(
                    "Error",
                    format!(
                        "Cannot assign {} to QFlags<QLocale::NumberOption>",
                        crate::valuetypes::type_name(&v)
                    ),
                ),
            };
        }
        "firstDayOfWeek" => "QQmlLocale::DayOfWeek",
        "weekDays" => "QList<QQmlLocale::DayOfWeek>",
        "textDirection" => "Qt::LayoutDirection",
        "uiLanguages" => "QStringList",
        "measurementSystem" => "QLocale::MeasurementSystem",
        _ if o.borrow().props.contains_key(k) => "QString",
        // V4 takes a method's index for a property's and names that property's type:
        // those past the last property have none.
        "currencySymbol" => "QLocale::MeasurementSystem",
        "dateTimeFormat" => "QList<QQmlLocale::DayOfWeek>",
        "toString" | "formattedDataSize" | "createSeparatedList" => "",
        _ if LOCALE_METHODS.contains(&k) => "QString",
        _ if throw => return it.throw("TypeError", "Type error"),
        _ => return Ok(false),
    };
    // A list converts to the property's own list type before the write is refused.
    let from = match &v {
        Value::Obj(a)
            if a.borrow().array.is_some() && cpp.starts_with('Q') && cpp.contains("List") =>
        {
            cpp.to_string()
        }
        v => crate::valuetypes::type_name(v),
    };
    it.throw("Error", format!("Cannot assign {from} to {cpp}"))
}

const LOCALE_METHODS: [&str; 11] = [
    "toString",
    "dayName",
    "standaloneDayName",
    "monthName",
    "standaloneMonthName",
    "dateFormat",
    "timeFormat",
    "dateTimeFormat",
    "currencySymbol",
    "formattedDataSize",
    "createSeparatedList",
];

pub(crate) fn is_locale_method(k: &str) -> bool {
    LOCALE_METHODS.contains(&k)
}

/// The methods of the QML Locale type by name. A Locale answers with each as its own
/// property ([`crate::valuetypes::Protos::locale_method`]); the prototype its objects
/// share has `toString` alone.
pub(crate) fn locale_methods() -> Obj {
    let o = new_obj();
    fn this_loc(this: &Value) -> Locale {
        locale_of(this).unwrap_or(Locale::SYSTEM)
    }
    /// A C++ `int` argument: absent is 0.
    fn int(it: &mut Interp, a: &[Value], i: usize) -> R<i32> {
        Ok(crate::eval::to_i32(it.to_numeric(&arg(a, i))?))
    }
    /// The QLocale::FormatType of the format getters: anything but 0 is the short one.
    fn short_or_long(it: &mut Interp, a: &[Value], i: usize) -> R<Fmt> {
        Ok(if int(it, a, i)? == 0 {
            Fmt::Long
        } else {
            Fmt::Short
        })
    }
    /// `monthName(index [, format])` and the like: Qt picks a C++ overload by the argument
    /// count and has none for no argument.
    fn name_args(it: &mut Interp, name: &str, a: &[Value]) -> R<(i32, i32)> {
        if a.is_empty() {
            return it.throw(
                "Error",
                format!(
                    "Unable to determine callable overload.  Candidates are:\n    \
                     {name}(int)\n    {name}(int,QLocale::FormatType)"
                ),
            );
        }
        Ok((int(it, a, 0)?, int(it, a, 1)?))
    }
    fn month(it: &mut Interp, this: &Value, name: &str, a: &[Value], standalone: bool) -> R<Value> {
        let (m, f) = name_args(it, name, a)?;
        let f = match f {
            1 => Fmt::Short,
            2 => Fmt::Narrow,
            _ => Fmt::Long,
        };
        Ok(Value::str(match usize::try_from(m) {
            Ok(m) => this_loc(this).month_name(m, f, standalone),
            Err(_) => "",
        }))
    }
    fn day(it: &mut Interp, this: &Value, name: &str, a: &[Value], standalone: bool) -> R<Value> {
        let (d, f) = name_args(it, name, a)?;
        let f = match f {
            0 => Fmt::Long,
            1 => Fmt::Short,
            2 => Fmt::Narrow,
            _ => return Ok(Value::str("")),
        };
        Ok(Value::str(match d {
            0..=7 => this_loc(this).day_name(d as usize % 7, f, standalone),
            _ => "",
        }))
    }
    nat(&o, "toString", 0, |it, this, a| {
        let loc = this_loc(this);
        if a.len() > 3 {
            return it.throw(
                "Error",
                format!(
                    "Locale: toString(): Expected 1-3 arguments, but received {}",
                    a.len()
                ),
            );
        }
        let v = arg(a, 0);
        if let Some(ms) = date_ms(&v) {
            if a.len() > 2 {
                return it.throw(
                    "Error",
                    "Locale: the toString() overload that takes a Date as its first argument \
                     expects 1 or 2 arguments, but received 3",
                );
            }
            if ms.is_nan() {
                return Ok(Value::str(""));
            }
            // The system locale writes a format of its own as the locale of LC_TIME, and a
            // format given as text with the zone's own abbreviation: see [`locale_date_string`].
            let system = is_system_locale(this);
            let own = if system { Locale::system_time() } else { loc };
            return Ok(Value::str(&match a.get(1) {
                Some(Value::Str(f)) => match When::at(ms) {
                    Some(when) => qt_format_stamp(&Stamp::DateTime(when), f, loc, system),
                    None => String::new(),
                },
                None | Some(Value::Undefined) => {
                    qt_format_in(ms, own.date_time_format(Fmt::Long), own)
                }
                Some(Value::Num(_) | Value::Bool(_)) => {
                    let f = short_or_long(it, a, 1)?;
                    qt_format_in(ms, own.date_time_format(f), own)
                }
                Some(_) => {
                    return it.throw(
                        "Error",
                        "Locale: the second argument to the toString overloads whose first \
                         argument is a Date should be a string or FormatType",
                    );
                }
            }));
        }
        let n = match &v {
            Value::Undefined => return Ok(Value::str(&loc.description())),
            Value::Num(n) => *n,
            Value::Bool(b) => f64::from(u8::from(*b)),
            Value::Str(_) | Value::Str16(_) => {
                return it.throw(
                    "TypeError",
                    "Passing incompatible arguments to C++ functions from JavaScript is not \
                     allowed.",
                );
            }
            _ => {
                return it.throw(
                    "Error",
                    "Locale: toString() expects either an int, double, or Date as its first \
                     argument",
                );
            }
        };
        let numeric = locale_numeric(this);
        if a.len() == 1 {
            // QLocale::toString(int) for a number that is an int.
            return Ok(Value::str(&if n == f64::from(n as i32) {
                numeric.integer_to_string(n as i64)
            } else {
                numeric.to_string(n, b'g', 6)
            }));
        }
        // Qt takes the low byte of the format's first UTF-16 unit as a `char`.
        let format = match &a[1] {
            Value::Null | Value::Undefined => None,
            Value::Obj(_) if date_ms(&a[1]).is_none() => None,
            f => Some(it.to_string(f)?),
        };
        let Some(format) = format else {
            return it.throw(
                "Error",
                "Locale: the second argument to the toString overload whose first argument is \
                 a double should be a char",
            );
        };
        let precision = match a.get(2) {
            None => 6,
            Some(Value::Num(_) | Value::Bool(_)) => int(it, a, 2)?,
            Some(_) => {
                return it.throw(
                    "Error",
                    "Locale: the third argument to the toString overload whose first argument \
                     is a double should be an int",
                );
            }
        };
        Ok(Value::str(&match format.encode_utf16().next() {
            Some(f) => numeric.to_string(n, f as u8, precision),
            None => String::new(),
        }))
    });
    nat(&o, "dayName", 0, |it, this, a| {
        day(it, this, "dayName", a, false)
    });
    nat(&o, "standaloneDayName", 0, |it, this, a| {
        day(it, this, "standaloneDayName", a, true)
    });
    nat(&o, "monthName", 0, |it, this, a| {
        month(it, this, "monthName", a, false)
    });
    nat(&o, "standaloneMonthName", 0, |it, this, a| {
        month(it, this, "standaloneMonthName", a, true)
    });
    nat(&o, "dateFormat", 0, |it, this, a| {
        let f = short_or_long(it, a, 0)?;
        Ok(Value::str(this_loc(this).date_format(f)))
    });
    nat(&o, "timeFormat", 0, |it, this, a| {
        let f = short_or_long(it, a, 0)?;
        Ok(Value::str(this_loc(this).time_format(f)))
    });
    nat(&o, "dateTimeFormat", 0, |it, this, a| {
        let f = short_or_long(it, a, 0)?;
        Ok(Value::str(this_loc(this).date_time_format(f)))
    });
    nat(&o, "currencySymbol", 0, |it, this, a| {
        let loc = this_loc(this);
        let (symbol, iso) = loc.numeric().currency();
        Ok(Value::str(match a.first() {
            None | Some(Value::Undefined) => symbol,
            Some(_) => match int(it, a, 0)? {
                0 => iso,
                1 => symbol,
                2 => loc.currency_name(),
                _ => "",
            },
        }))
    });
    nat(&o, "formattedDataSize", 0, |it, this, a| {
        let invalid = |it: &mut Interp, what: &str| {
            it.throw(
                "Error",
                format!("Locale: formattedDataSize(): Invalid argument ({what})"),
            )
        };
        if a.is_empty() || a.len() > 3 {
            return it.throw(
                "Error",
                format!(
                    "Locale: formattedDataSize(): Expected 1-3 arguments, but received {}",
                    a.len()
                ),
            );
        }
        // QJSValue::toVariant().toLongLong(): what is no number is 0.
        let bytes = match &a[0] {
            Value::Num(n) if n.is_nan() => 0,
            Value::Num(n) if n.abs() >= 9.223372036854776e18 => i64::MIN,
            Value::Num(n) => *n as i64,
            Value::Bool(b) => i64::from(*b),
            Value::Str(s) => s.trim().parse().unwrap_or(0),
            _ => 0,
        };
        let precision = match a.get(1) {
            None | Some(Value::Undefined) => 2,
            Some(Value::Num(_) | Value::Bool(_)) => int(it, a, 1)?,
            Some(_) => return invalid(it, "'precision' must be an int"),
        };
        let format = match a.get(2) {
            None | Some(Value::Undefined) => 0,
            Some(Value::Num(_) | Value::Bool(_)) => (int(it, a, 2)? & 3) as u8,
            Some(_) => return invalid(it, "'format' must be DataSizeFormat"),
        };
        let numeric = locale_numeric(this);
        Ok(Value::str(
            &this_loc(this).formatted_data_size(&numeric, bytes, precision, format),
        ))
    });
    nat(&o, "createSeparatedList", 0, |it, this, a| {
        let items = match a.first() {
            None => return it.throw("Error", "Insufficient arguments"),
            Some(Value::Str(s)) => vec![s.to_string()],
            Some(v @ Value::Obj(_)) if crate::props::is_array(it, v)? => {
                let len = it.get_key(v, &Value::str("length"))?;
                let n = to_length(it, &len)?;
                let mut items = Vec::with_capacity(n);
                for i in 0..n {
                    let x = it.get_key(v, &Value::Num(i as f64))?;
                    items.push(it.to_string(&x)?);
                }
                items
            }
            Some(_) => {
                return it.throw(
                    "TypeError",
                    "Passing incompatible arguments to C++ functions from JavaScript is not \
                     allowed.",
                );
            }
        };
        Ok(Value::str(&this_loc(this).separated_list(&items)))
    });
    o
}

/// Marks the `Easing` type (a QQmlTypeWrapper over QQuickEasingValueType).
struct EasingType;

/// Whether `b` is the `Easing` type: it lists only the members of the QObject behind it,
/// refuses a write to anything but `objectName` and answers `delete` with true.
pub fn is_easing(b: &crate::value::JsObject) -> bool {
    b.native.as_ref().is_some_and(|n| n.is::<EasingType>())
}

/// QObject::toString of the object behind the `Easing` type: a new function each read.
pub fn easing_to_string() -> Value {
    native("", 0, |it, this, _| {
        let name = it.get(this, "objectName")?.to_js_string();
        let at = match this {
            Value::Obj(o) => Rc::as_ptr(o) as *const () as usize,
            _ => 0,
        };
        Ok(Value::str(&match name.as_str() {
            "" => format!("QQmlEasing({at:#x})"),
            _ => format!("QQmlEasing({at:#x}, \"{name}\")"),
        }))
    })
}

fn easing_object(function_proto: &Obj) -> Obj {
    let o = new_obj();
    o.borrow_mut().native = Some(Rc::new(EasingType));
    // V4 puts one object with a `toString` between a type and %Function.prototype%.
    let proto = new_obj();
    set_hidden(
        &proto,
        "toString",
        native("toString", 0, |_, this, _| {
            Ok(match this {
                Value::Obj(o) if is_easing(&o.borrow()) => Value::str("QML/Easing"),
                _ => Value::Undefined,
            })
        }),
    );
    proto.borrow_mut().proto = Some(function_proto.clone());
    o.borrow_mut().proto = Some(proto);
    // QEasingCurve's BezierSpline, TCBSpline and Custom follow these.
    let names = &EASING_NAMES[..45];
    // The values are not listed and cannot be written (QQmlTypeWrapper looks an enum up
    // by name); what is listed are the members of the QObject behind the type.
    let fixed = |n: &str, v: f64| {
        set_hidden(&o, n, Value::Num(v));
        let attrs = crate::value::READONLY | crate::value::PERMANENT;
        let mut b = o.borrow_mut();
        b.attrs.get_or_insert_default().insert(n.into(), attrs);
    };
    for (i, n) in names.iter().enumerate() {
        fixed(n, i as f64);
    }
    // QQuickEasingValueType::Type ends with BezierSpline and its alias Bezier (both
    // QEasingCurve::BezierSpline); QEasingCurve's TCBSpline and Custom are not exported.
    for n in ["BezierSpline", "Bezier"] {
        fixed(n, names.len() as f64);
    }
    set_own(&o, "objectName", Value::str(""));
    set_own(
        &o,
        "objectNameChanged",
        native("", 0, |_, _, _| Ok(Value::Undefined)),
    );
    // QQmlEasing::valueForProgress(type, progress): QEasingCurve(type).valueForProgress.
    set_own(
        &o,
        "valueForProgress",
        native("", 0, |it, _, a| {
            let [ty, progress, ..] = a else {
                return it.throw("Error", "Insufficient arguments");
            };
            // A number that is no curve's leaves the QEasingCurve linear, and one that is not
            // a number is 0.
            let ty = ty.to_number();
            let e = crate::easing::Easing {
                ty: if (0.0..47.0).contains(&ty) {
                    ty as i32
                } else {
                    0
                },
                ..Default::default()
            };
            let t = progress.to_number();
            Ok(Value::Num(e.value(if t.is_nan() { 0.0 } else { t })))
        }),
    );
    // Its own `toString` is read anew each time ([`easing_to_string`]).
    set_hidden(&o, "toString", Value::Undefined);
    crate::unwritten::fill(&o, "Easing");
    // Nothing can be added to it, though V4 calls it extensible.
    o.borrow_mut().non_extensible = true;
    o
}

/// The keys of QEasingCurve::Type in the order of their values.
pub(crate) const EASING_NAMES: [&str; 48] = [
    "Linear",
    "InQuad",
    "OutQuad",
    "InOutQuad",
    "OutInQuad",
    "InCubic",
    "OutCubic",
    "InOutCubic",
    "OutInCubic",
    "InQuart",
    "OutQuart",
    "InOutQuart",
    "OutInQuart",
    "InQuint",
    "OutQuint",
    "InOutQuint",
    "OutInQuint",
    "InSine",
    "OutSine",
    "InOutSine",
    "OutInSine",
    "InExpo",
    "OutExpo",
    "InOutExpo",
    "OutInExpo",
    "InCirc",
    "OutCirc",
    "InOutCirc",
    "OutInCirc",
    "InElastic",
    "OutElastic",
    "InOutElastic",
    "OutInElastic",
    "InBack",
    "OutBack",
    "InOutBack",
    "OutInBack",
    "InBounce",
    "OutBounce",
    "InOutBounce",
    "OutInBounce",
    "InCurve",
    "OutCurve",
    "SineCurve",
    "CosineCurve",
    "BezierSpline",
    "TCBSpline",
    "Custom",
];

#[cfg(all(test, feature = "interp"))]
mod date_tests {
    use super::*;

    fn eval(src: &str) -> String {
        let body = crate::ir::lower_program(&format!("return {src}")).unwrap();
        let mut it = Interp::default();
        let g = it.global.clone();
        it.run(&body, &g).ok().unwrap().to_js_string()
    }

    /// The texts and setters of Qt 6.12 where it departs from ECMA-262: QV4 prints
    /// toUTCString as "Tue Jan 2 03:04:05 2024 GMT", toTimeString without the zone, an
    /// invalid toTimeString as "", and keeps setUTCFullYear on an invalid date NaN.
    #[test]
    fn date_strings_and_setters_follow_qt() {
        let src = "[new Date(Date.UTC(2024, 0, 2, 3, 4, 5, 6)).toUTCString(), \
                    new Date(Date.UTC(-5, 0, 1)).toGMTString(), new Date(Date.UTC(12345, 0, 1)).toUTCString(), \
                    new Date(NaN).toTimeString(), new Date(NaN).toDateString(), \
                    /^03:04:05$/.test(new Date(2024, 0, 2, 3, 4, 5).toTimeString()), \
                    new Date(NaN).setUTCFullYear(2020, 1), new Date(8.64e15).toISOString(), \
                    new Date(-8.64e15).toISOString(), new Date(Date.UTC(-1, 0)).toISOString(), \
                    new Date(Date.UTC(10000, 0)).toISOString()].join('|')";
        assert_eq!(
            eval(src),
            "Tue Jan 2 03:04:05 2024 GMT|Sun Jan 1 00:00:00 -006 GMT|\
             Mon Jan 1 00:00:00 12345 GMT|||true|NaN|\
             +275760-09-13T00:00:00.000Z|-271821-04-20T00:00:00.000Z|\
             -000001-01-01T00:00:00.000Z|+010000-01-01T00:00:00.000Z"
        );
    }

    /// Audit B3-1, B3-2: time values past chrono's years are dates, printed as Qt 6.12 prints
    /// them (QDateTime::toString: the year after the time, the day without a leading zero, no
    /// sign on a year past 9999, nothing for a date that is not one). The zone-dependent
    /// values are in tests/audit_b3_env.rs.
    #[test]
    fn far_dates_and_long_years_format() {
        let src = "[/^(Fri|Sat) Sep 1[23] \\d\\d:\\d\\d:\\d\\d 275760 GMT([+-]\\d{4})?$/.test(String(new Date(8.64e15))), \
                    isNaN(new Date(8.64e15).getTimezoneOffset()), \
                    /^(Fri|Sat) Sep 1[23] 275760$/.test(new Date(8.64e15).toDateString()), \
                    Qt.formatDateTime(new Date(8.64e15), 'yyyy-MM'), \
                    /^-27182[12]$/.test(Qt.formatDateTime(new Date(-8.64e15), 'yyyy')), \
                    new Date(8.64e15 + 1).toDateString(), \
                    new Date(10000, 0, 1).toDateString(), new Date(9999, 11, 31).toDateString(), \
                    new Date(123456, 5, 7).toDateString(), new Date(-1, 0, 1).toDateString(), \
                    String(new Date(10000, 0, 1)).slice(0, 24), \
                    Qt.formatDateTime(new Date(10000, 0, 1), 'yyyy yy'), \
                    Qt.formatDateTime(new Date(-1, 5, 1), 'yyyy yy'), \
                    Qt.formatDateTime(new Date(-1199, 5, 1), 'yyyy yy')].join('|')";
        assert_eq!(
            eval(src),
            "true|false|true|+275760-09|true||Sat Jan 1 10000|Fri Dec 31 9999|\
             Sat Jun 7 123456|Fri Jan 1 -002|Sat Jan 1 00:00:00 10000|+10000 00|-0002 -2|\
             -1200 00"
        );
    }

    #[test]
    fn ecma262_day_arithmetic() {
        for z in [-719_468 - 146_097, -1, 0, 1, 11_016, 19_723, 2_932_896] {
            let (y, m, d) = civil_from_days(z);
            assert_eq!(days_from_civil(y, m, d), z);
        }
        assert_eq!(civil_from_days(19_724), (2024, 1, 2));
        assert_eq!(
            make_day(2024.0, 13.0, 1.0),
            days_from_civil(2025, 2, 1) as f64
        );
        assert_eq!(
            make_day(2024.0, -1.0, 0.0),
            days_from_civil(2023, 11, 30) as f64
        );
        assert!(make_day(1e10, 0.0, 1.0).is_nan());
        assert!(make_time(1.0, f64::INFINITY, 0.0, 0.0).is_nan());
        assert_eq!(time_clip(-0.5).to_bits(), 0.0f64.to_bits());
        assert!(time_clip(8.64e15 + 1.0).is_nan());
        assert_eq!(week_day(-1.0), 3.0);
    }

    #[test]
    fn date_and_qt_format() {
        let body = crate::ir::lower_program(&format!("return {}", "(function(){var d = new Date(2024, 0, 31, 13, 5, 9); d.setDate(d.getDate() + 1); return \
                 [d.getFullYear(), d.getMonth(), d.getDate(), d.getDay(), \
                  Qt.locale().toString(d, 'dddd, MMMM dd yyyy HH:mm:ss'), \
                  Qt.formatDateTime(d, 'h:mm AP'), Qt.locale().toString(d, \"ddd d'th'\"), \
                  Easing.BezierSpline].join('|')})()")).unwrap();
        let mut it = Interp::default();
        let g = it.global.clone();
        let v = it.run(&body, &g).ok().unwrap();
        assert_eq!(
            v.to_js_string(),
            "2024|1|1|4|Thursday, February 01 2024 13:05:09|1:05 PM|Thu 1th|45"
        );
    }

    /// Expected output is Qt 6.12's (PySide6) for the same expression.
    #[test]
    fn locale_names_and_formats_match_qt() {
        let src = "(function(){ var d = new Date(2026, 9, 31, 18, 5, 7); var r = [];\nfor (const n of [\"ru_RU\", \"de_DE\", \"en_US\", \"zh_CN\", \"uk_UA\"]) { var L = Qt.locale(n);\n r.push([L.toString(d, \"dddd, MMMM dd, yyyy\"), L.toString(d, \"ddd dd\"), d.toLocaleDateString(L, \"MMMM yyyy\"),\n  L.toString(d, \"MMM d hh:mm AP ap Ap\"), d.toLocaleDateString(L, 1), d.toLocaleDateString(L), d.toLocaleTimeString(L, 1),\n  L.dayName(0), L.dayName(6, 1), L.dayName(1, 2), L.standaloneDayName(6, 1), L.monthName(0), L.monthName(0, 1), L.standaloneMonthName(0),\n  L.amText, L.pmText, L.dateFormat(1), L.timeFormat(), L.dateTimeFormat(1)].join(\"|\")); }\nr.push(Qt.formatDateTime(d, \"dddd MMMM ap\"));\nreturn r.join(\"\\n\"); })()";
        let body = crate::ir::lower_program(&format!("return {src}")).unwrap();
        let mut it = Interp::default();
        let g = it.global.clone();
        let v = it.run(&body, &g).ok().unwrap();
        let expected = "суббота, октября 31, 2026|сб 31|октября 2026|окт. 31 06:05 PM pm PM|31.10.2026|суббота, 31 октября 2026 г.|18:05|воскресенье|сб|П|сб|января|янв.|январь|AM|PM|dd.MM.yyyy|HH:mm:ss tttt|dd.MM.yyyy HH:mm\nSamstag, Oktober 31, 2026|Sa. 31|Oktober 2026|Okt. 31 06:05 PM pm PM|31.10.26|Samstag, 31. Oktober 2026|18:05|Sonntag|Sa.|M|Sa|Januar|Jan.|Januar|AM|PM|dd.MM.yy|HH:mm:ss tttt|dd.MM.yy HH:mm\nSaturday, October 31, 2026|Sat 31|October 2026|Oct 31 06:05 PM pm PM|10/31/26|Saturday, October 31, 2026|6:05 PM|Sunday|Sat|M|Sat|January|Jan|January|AM|PM|M/d/yy|h:mm:ss Ap tttt|M/d/yy h:mm Ap\n星期六, 十月 31, 2026|周六 31|十月 2026|10月 31 06:05 下午 下午 下午|2026/10/31|2026年10月31日星期六|18:05|星期日|周六|一|周六|一月|1月|一月|上午|下午|yyyy/M/d|tttt HH:mm:ss|yyyy/M/d HH:mm\nсубота, жовтня 31, 2026|сб 31|жовтня 2026|жовт. 31 06:05 ПП пп пп|31.10.26|субота, 31 жовтня 2026 р.|18:05|неділя|сб|П|сб|січня|січ.|січень|дп|пп|dd.MM.yy|HH:mm:ss tttt|dd.MM.yy HH:mm\nSaturday October pm";
        assert_eq!(v.to_js_string(), expected);
    }
}

/// RFC 1321 MD5, lowercase hex (Qt.md5 hashes the UTF-8 bytes of the string).
pub fn md5_hex(data: &[u8]) -> String {
    use md5::Digest;
    md5::Md5::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[cfg(test)]
mod md5_tests {
    #[test]
    fn rfc1321_vectors() {
        assert_eq!(super::md5_hex(b""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(super::md5_hex(b"abc"), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(
            super::md5_hex(b"The quick brown fox jumps over the lazy dog"),
            "9e107d9d372bb6826bd81d3542a419d6"
        );
    }
}

/// ECMA-262 Encode: UTF-8 percent-encoding except unreserved marks and `keep`.
/// Encode (ECMA-262 §19.2.6.5) of ToString(`v`): a URIError for an unpaired surrogate, which
/// only a [`Value::Str16`] holds.
fn encode_uri(it: &mut Interp, v: &Value, keep: &str) -> R<Value> {
    match it.to_str_value(v)? {
        Value::Str(s) => Ok(Value::str(&uri_encode(&s, keep))),
        _ => it.throw("URIError", "malformed URI sequence"),
    }
}

pub(crate) fn uri_encode(s: &str, keep: &str) -> String {
    use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
    const COMPONENT: &AsciiSet = &NON_ALPHANUMERIC
        .remove(b'-')
        .remove(b'_')
        .remove(b'.')
        .remove(b'!')
        .remove(b'~')
        .remove(b'*')
        .remove(b'\'')
        .remove(b'(')
        .remove(b')');
    const URI: &AsciiSet = &COMPONENT
        .remove(b';')
        .remove(b',')
        .remove(b'/')
        .remove(b'?')
        .remove(b':')
        .remove(b'@')
        .remove(b'&')
        .remove(b'=')
        .remove(b'+')
        .remove(b'$')
        .remove(b'#');
    utf8_percent_encode(s, if keep.is_empty() { COMPONENT } else { URI }).to_string()
}

/// ECMA-262 Decode: %XX sequences to UTF-8, leaving escapes of `keep` characters intact.
/// `None` where V4 throws URIError (a `%` without two hex digits, or bytes that are not UTF-8).
/// Decode (ECMA-262 §19.2.6.6) of ToString(`v`): an unpaired surrogate is copied like any
/// unit that is not `%`, and an escape never spans one, so the text between them decodes
/// on its own.
fn decode_uri(it: &mut Interp, v: &Value, keep: &str) -> R<Value> {
    let s = it.to_str_value(v)?;
    let mut out = crate::value::JsBuf::new();
    let mut run = String::new();
    let mut ok = true;
    let mut flush = |run: &mut String, out: &mut crate::value::JsBuf| {
        match uri_decode(run, keep) {
            Some(d) => out.push_str(&d),
            None => ok = false,
        }
        run.clear();
    };
    for c in char::decode_utf16(units(&s).iter().copied()) {
        match c {
            Ok(c) => run.push(c),
            Err(e) => {
                flush(&mut run, &mut out);
                out.push(JsStr::Utf16(&[e.unpaired_surrogate()]));
            }
        }
    }
    flush(&mut run, &mut out);
    match ok {
        true => Ok(out.finish()),
        false => it.throw("URIError", "malformed URI sequence"),
    }
}

pub(crate) fn uri_decode(s: &str, keep: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut start = 0;
    let mut i = 0;
    while let Some(off) = b[i..].iter().position(|&c| c == b'%') {
        i += off;
        let v = b
            .get(i + 1..i + 3)
            .filter(|h| h.iter().all(u8::is_ascii_hexdigit))
            .and_then(|h| u8::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok())?;
        if v.is_ascii() && keep.contains(v as char) {
            out.extend(percent_encoding::percent_decode(&b[start..i]));
            out.extend_from_slice(&b[i..i + 3]);
            start = i + 3;
        }
        i += 3;
    }
    out.extend(percent_encoding::percent_decode(&b[start..]));
    String::from_utf8(out).ok()
}

/// `String.prototype.localeCompare` as QV4 does it (`QString::localeAwareCompare`, which with
/// ICU is `QCollator::defaultCompare` in `QLocale().collation()`): the C locale, and an empty
/// string in any locale, give what `QtPrivate::compareStrings` gives; any other locale goes
/// through the ICU collator with its default (tertiary) strength, so e.g. `ru` sorts Cyrillic
/// before Latin.
fn locale_cmp(a: JsStr<'_>, b: JsStr<'_>) -> i32 {
    thread_local! {
        static COLLATOR: Option<icu_collator::CollatorBorrowed<'static>> =
            collator_for(Locale::system_collation().name());
    }
    COLLATOR.with(|c| collate(c.as_ref(), a, b))
}

fn collate(
    collator: Option<&icu_collator::CollatorBorrowed<'static>>,
    a: JsStr<'_>,
    b: JsStr<'_>,
) -> i32 {
    match (collator, a, b) {
        (Some(_), a, b) if a.is_empty() || b.is_empty() => compare_strings(a, b),
        (Some(c), JsStr::Utf8(a), JsStr::Utf8(b)) => c.compare(a, b) as i32,
        (Some(c), a, b) => c.compare_utf16(&a.units(), &b.units()) as i32,
        (None, a, b) => compare_strings(a, b),
    }
}

/// `QtPrivate::compareStrings`, case sensitive (`ucstrcmp`): the difference of the first two
/// UTF-16 code units that differ, and -1, 0 or 1 by length when one string starts the other.
fn compare_strings(a: JsStr<'_>, b: JsStr<'_>) -> i32 {
    let (mut a, mut b) = (a.code_units(), b.code_units());
    loop {
        match (a.next(), b.next()) {
            (Some(x), Some(y)) if x == y => {}
            (Some(x), Some(y)) => return i32::from(x) - i32::from(y),
            (None, None) => return 0,
            (None, Some(_)) => return -1,
            (Some(_), None) => return 1,
        }
    }
}

/// ICU collator of `QLocale(name)` (`ru_RU.UTF-8`); None for the C locale, which is also what
/// QLocale makes of a name it has no data for.
fn collator_for(name: &str) -> Option<icu_collator::CollatorBorrowed<'static>> {
    if crate::locale::Locale::named(name) == crate::locale::Locale::c() {
        return None;
    }
    let name = name
        .split(['.', '@'])
        .next()
        .unwrap_or("")
        .replace('_', "-");
    let loc =
        icu_locale_core::Locale::try_from_str(&name).unwrap_or(icu_locale_core::Locale::UNKNOWN);
    icu_collator::Collator::try_new(loc.into(), Default::default()).ok()
}

#[cfg(test)]
mod collation_tests {
    use super::*;

    #[test]
    fn ru_collation_sorts_cyrillic_first_like_qt() {
        let c = collator_for("ru_RU").unwrap();
        let mut v = [
            "SongRec",
            "Qt V4L2 video capture utility",
            "Регулятор громкости PulseAudio",
            "Haruna",
            "Qt V4L2 test Utility",
            "Проигрыватель мультимедиа mpv",
        ];
        v.sort_by(|a, b| c.compare(a, b));
        assert_eq!(
            v,
            [
                "Проигрыватель мультимедиа mpv",
                "Регулятор громкости PulseAudio",
                "Haruna",
                "Qt V4L2 test Utility",
                "Qt V4L2 video capture utility",
                "SongRec",
            ]
        );
        let en = collator_for("en_US").unwrap();
        assert_eq!(
            en.compare("Haruna", "Проигрыватель"),
            std::cmp::Ordering::Less
        );
        assert_eq!(en.compare("apple", "Banana"), std::cmp::Ordering::Less);
        assert!(collator_for("C").is_none());
        // Qt 6.12 in the C locale: "a".localeCompare() (against "undefined"), and the rest.
        assert_eq!(
            compare_strings(JsStr::Utf8("a"), JsStr::Utf8("undefined")),
            -20
        );
        assert_eq!(compare_strings(JsStr::Utf8("a"), JsStr::Utf8("B")), 31);
        assert_eq!(
            compare_strings(JsStr::Utf8("a"), JsStr::Utf8("\u{e1}")),
            -128
        );
        assert_eq!(
            compare_strings(JsStr::Utf8("r\u{e9}sum\u{e9}"), JsStr::Utf8("resume")),
            132
        );
        assert_eq!(
            [
                compare_strings(JsStr::Utf8("a"), JsStr::Utf8("ab")),
                compare_strings(JsStr::Utf8("ab"), JsStr::Utf8("a")),
                compare_strings(JsStr::Utf8("a"), JsStr::Utf8("a"))
            ],
            [-1, 1, 0]
        );
    }
}

#[cfg(test)]
mod number_conv_tests {
    use super::*;

    // Expected values from V8 (node 22).
    #[test]
    fn to_fixed_rounds_like_v8() {
        for (x, d, want) in [
            (1.45, 1, "1.4"),
            (10.235, 2, "10.23"),
            (4.35, 1, "4.3"),
            (1.005, 2, "1.00"),
            (0.125, 2, "0.13"),
            (-0.125, 2, "-0.13"),
            (2.5, 0, "3"),
            (-2.5, 0, "-3"),
            (0.5, 0, "1"),
            (8.345, 2, "8.35"),
            (1.255, 2, "1.25"),
            (0.1, 20, "0.10000000000000000555"),
            (-0.0000001, 2, "-0.00"),
        ] {
            assert_eq!(to_fixed(x, d), want, "({x}).toFixed({d})");
        }
    }

    #[test]
    fn parse_int_and_float_match_v8() {
        assert_eq!(parse_int("\u{FEFF}12", f64::NAN), 12.0);
        assert_eq!(
            parse_int("123456789012345678901234567890", f64::NAN),
            1.2345678901234568e29
        );
        assert_eq!(parse_int("  -0x1A", f64::NAN), -26.0);
        assert_eq!(parse_int("ff", 16.0), 255.0);
        assert_eq!(parse_int("1e3", f64::NAN), 1.0);
        assert!(parse_int("0x", f64::NAN).is_nan());
        assert!(parse_int("\u{85}1", f64::NAN).is_nan());
        assert_eq!(parse_float("1e"), 1.0);
        assert_eq!(parse_float("1e+"), 1.0);
        assert_eq!(parse_float(".5.5"), 0.5);
        assert_eq!(parse_float("-.5e-3x"), -0.0005);
        assert_eq!(parse_float("Infinityx"), f64::INFINITY);
        assert_eq!(parse_float("\u{a0} 3.5"), 3.5);
        assert_eq!(parse_float("5."), 5.0);
        for s in ["infinity", "--1", ".", "e5", "", "+"] {
            assert!(parse_float(s).is_nan(), "{s:?}");
        }
    }
}

#[cfg(test)]
mod has_method_tests {
    use super::*;

    #[test]
    fn every_listed_builtin_dispatches() {
        let mut it = Interp::new(Box::new(crate::eval::NoHost));
        let map = new_coll(&mut it, true, &Value::Undefined, None)
            .ok()
            .unwrap();
        let set = new_coll(&mut it, false, &Value::Undefined, None)
            .ok()
            .unwrap();
        let re = regex_value("a", "g");
        let samples: [(Value, &[&str]); 8] = [
            (Value::str("abc"), STRING_METHODS),
            (Value::Num(1.5), NUMBER_METHODS),
            (Value::Bool(true), BOOLEAN_METHODS),
            (
                Value::Color(Color::from_argb32(0xff102030)),
                &["toString", "lighter", "darker"],
            ),
            (Value::array(vec![Value::Num(1.0)]), ARRAY_METHODS),
            (map, MAP_METHODS),
            (set.clone(), SET_METHODS),
            (re, REGEXP_METHODS),
        ];
        for (v, names) in samples {
            for n in names {
                assert!(has_method(&mut it, &v, n), "{n} of {v:?}");
                let r = method_impl(&mut it, &v, n, &[]);
                assert!(
                    !matches!(r, Ok(None)),
                    "{n} of {v:?} is admitted but not dispatched"
                );
            }
        }
        assert!(!has_method(&mut it, &set, "get"));
        assert!(!has_method(&mut it, &Value::str("abc"), "nope"));
        assert!(matches!(
            method(&mut it, &Value::str("abc"), "nope", &[]),
            Ok(None)
        ));
    }
}

#[cfg(all(test, feature = "interp"))]
mod value_type_tests {
    use crate::eval::Interp;

    fn eval(src: &str) -> String {
        let body = crate::ir::lower_program(src).unwrap();
        let mut it = Interp::default();
        let g = it.global.clone();
        it.run(&body, &g).ok().unwrap().to_js_string()
    }

    #[test]
    fn array_arguments_make_the_default_value() {
        // Qt 6.12: an array passed for a QVector3D/QVector4D parameter is a null vector, as an
        // array assigned to a value type property stores the default value.
        assert_eq!(
            eval(
                "var q = Qt.quaternion(1, 2, 3, 4), m = Qt.matrix4x4(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16);\
                 return [q.times([1, 2, 3]), m.times([1, 2, 3, 4]),\
                 Qt.vector3d(1, 2, 3).plus([1, 2, 3])].join('|')"
            ),
            "QVector3D(0, 0, 0)|QVector4D(0, 0, 0, 0)|QVector3D(1, 2, 3)"
        );
    }

    #[test]
    fn methods_are_own_and_unlisted() {
        // Qt 6.12 reports a value type's methods as own properties to hasOwnProperty yet not
        // to `in`, and does not list them.
        assert_eq!(
            eval(
                "var q = Qt.quaternion(1, 2, 3, 4);\
                 return [typeof q.times, q.hasOwnProperty('times'), 'times' in q, 'x' in q,\
                 Object.keys(q).join()].join()"
            ),
            "function,true,false,true,scalar,x,y,z"
        );
    }
}
