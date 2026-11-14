//! The `URL` and `URLSearchParams` globals of Qt's V4 (qv4urlobject.cpp), with the semantics
//! of the WHATWG URL Standard (<https://url.spec.whatwg.org/#api>) where Qt 6.12 deviates
//! from it. Parsing, serialization and the setters are the `url` crate's (`url::quirks` is
//! its implementation of the standard's URL API).
use crate::eval::{
    Callable, Interp, NativeFn, R, define_accessor, fn_value, link_builtin_prototype, new_obj,
    set_hidden,
};
use crate::value::{Internal, IterKind, Obj, ParamsIter, Value, WellKnown};
use std::cell::RefCell;
use std::rc::{Rc, Weak};
use url::{Url, form_urlencoded, quirks};

/// A URL object's internal state: its URL record and its query object (`searchParams`).
struct UrlData {
    url: RefCell<Url>,
    params: Obj,
}

/// A URLSearchParams object's list and URL object (spec §6.2).
struct ParamsData {
    list: RefCell<Vec<(String, String)>>,
    url: Weak<UrlData>,
}

fn data<T: 'static>(v: &Value) -> Option<Rc<T>> {
    match v {
        Value::Obj(o) => o.borrow().native.clone()?.downcast::<T>().ok(),
        _ => None,
    }
}

/// The builtinTag `Object.prototype.toString` reports for a URL or URLSearchParams object.
pub(crate) fn tag(o: &crate::value::JsObject) -> Option<&'static str> {
    let n = o.native.as_ref()?;
    if n.is::<UrlData>() {
        Some("URL")
    } else if n.is::<ParamsData>() {
        Some("URLSearchParams")
    } else {
        None
    }
}

/// Whether `o` is a URLSearchParams object (iterable, unlike other native objects).
pub(crate) fn is_params(o: &crate::value::JsObject) -> bool {
    o.native.as_ref().is_some_and(|n| n.is::<ParamsData>())
}

/// The pair at `index` of URLSearchParams `o`'s list as `kind` asks for, which then moves
/// past it; `None` past the list's end (WebIDL §3.7.10.2 steps 7-12, the list read afresh).
pub(crate) fn params_step(o: &Obj, kind: IterKind, index: &mut usize) -> Option<Value> {
    let p = o.borrow().native.clone()?.downcast::<ParamsData>().ok()?;
    let (k, v) = p.list.borrow().get(*index).cloned()?;
    *index += 1;
    Some(match kind {
        IterKind::Keys => Value::str(&k),
        IterKind::Values => Value::str(&v),
        IterKind::Entries => Value::array(vec![Value::str(&k), Value::str(&v)]),
    })
}

/// `URLSearchParams.prototype.{entries, keys, values}`: a new default iterator object over
/// `this` (WebIDL §3.7.8.1, §3.7.10.1).
fn params_iter(it: &mut Interp, this: &Value, kind: IterKind) -> R<Value> {
    this_params(it, this)?;
    let Value::Obj(params) = this else {
        unreachable!()
    };
    let o = new_obj();
    {
        let mut b = o.borrow_mut();
        b.proto = Some(it.sym.params_iter_proto.clone());
        b.internal = Internal::ParamsIter(Box::new(ParamsIter {
            params: params.clone(),
            kind,
            index: 0,
        }));
    }
    Ok(Value::Obj(o))
}

/// `%URLSearchParamsIteratorPrototype%.next` (WebIDL §3.7.10.2).
pub(crate) fn params_iter_next(it: &mut Interp, this: &Value, _: &[Value]) -> R<Value> {
    let state = match this {
        Value::Obj(o) => match &o.borrow().internal {
            Internal::ParamsIter(p) => Some((p.params.clone(), p.kind, p.index)),
            _ => None,
        },
        _ => None,
    };
    let Some((params, kind, mut index)) = state else {
        return it.throw("TypeError", "Not a URLSearchParams Iterator instance");
    };
    let v = params_step(&params, kind, &mut index);
    if let Value::Obj(o) = this
        && let Internal::ParamsIter(p) = &mut o.borrow_mut().internal
    {
        p.index = index;
    }
    let done = v.is_none();
    Ok(crate::symbol::iter_result(v.unwrap_or_default(), done))
}

fn arg(a: &[Value], i: usize) -> Value {
    a.get(i).cloned().unwrap_or_default()
}

fn this_url(it: &Interp, this: &Value) -> R<Rc<UrlData>> {
    match data::<UrlData>(this) {
        Some(d) => Ok(d),
        None => it.throw("TypeError", "Value of \"this\" must be of type URL"),
    }
}

fn this_params(it: &Interp, this: &Value) -> R<Rc<ParamsData>> {
    match data::<ParamsData>(this) {
        Some(d) => Ok(d),
        None => it.throw(
            "TypeError",
            "Value of \"this\" must be of type URLSearchParams",
        ),
    }
}

/// WebIDL's TypeError for an operation called with fewer than its required arguments.
fn need(it: &Interp, a: &[Value], n: usize, what: &str) -> R<()> {
    if a.len() < n {
        return it.throw(
            "TypeError",
            format!(
                "{what}: {n} argument(s) required, but only {} present",
                a.len()
            ),
        );
    }
    Ok(())
}

/// The `prototype` of global constructor `name` (the globals are not writable in QV4).
fn proto_of(it: &Interp, name: &str) -> Option<Obj> {
    match it.global.borrow().vars.get(name) {
        Some(Value::Obj(c)) => match c.borrow().props.get("prototype") {
            Some(Value::Obj(p)) => Some(p.clone()),
            _ => None,
        },
        _ => None,
    }
}

fn parse_query(q: &str) -> Vec<(String, String)> {
    form_urlencoded::parse(q.as_bytes()).into_owned().collect()
}

fn serialize(list: &[(String, String)]) -> String {
    form_urlencoded::Serializer::new(String::new())
        .extend_pairs(list)
        .finish()
}

fn new_params(it: &Interp, list: Vec<(String, String)>, url: Weak<UrlData>) -> Obj {
    let o = new_obj();
    let mut b = o.borrow_mut();
    b.proto = proto_of(it, "URLSearchParams");
    b.native = Some(Rc::new(ParamsData {
        list: RefCell::new(list),
        url,
    }));
    drop(b);
    o
}

/// The API URL parser (spec §6.1) on arguments `(url, base)` of the URL constructor,
/// `URL.parse` or `URL.canParse` (`what`): the URL record, or why there is none.
fn api_parse(it: &mut Interp, a: &[Value], what: &str) -> R<Result<Url, String>> {
    need(it, a, 1, what)?;
    let input = it.to_string(&a[0])?;
    let base = match a.get(1) {
        None | Some(Value::Undefined) => None,
        Some(b) => {
            let b = it.to_string(b)?;
            match Url::parse(&b) {
                Ok(u) => Some(u),
                Err(_) => return Ok(Err(format!("Invalid base URL: {b}"))),
            }
        }
    };
    Ok(Url::options()
        .base_url(base.as_ref())
        .parse(&input)
        .map_err(|_| format!("Invalid URL: {input}")))
}

/// The URL constructor (spec §6.1): `new URL(url, base)`.
fn construct_url(it: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
    match api_parse(it, a, "URL constructor")? {
        Ok(url) => Ok(new_url(it, url)),
        Err(e) => it.throw("TypeError", e),
    }
}

/// A new URL object for `url`.
fn new_url(it: &Interp, url: Url) -> Value {
    let list = url.query().map(parse_query).unwrap_or_default();
    let d = Rc::new_cyclic(|w| UrlData {
        params: new_params(it, list, w.clone()),
        url: RefCell::new(url),
    });
    let o = new_obj();
    o.borrow_mut().proto = proto_of(it, "URL");
    o.borrow_mut().native = Some(d);
    Value::Obj(o)
}

/// The `protocol` setter (spec §6.1): basic URL parsing with the scheme start state as state
/// override. The `url` crate refuses every switch to `file` of a URL with a host; the scheme
/// state refuses it only when the URL includes credentials or has a port, and otherwise
/// keeps the host and path.
fn set_protocol(u: &mut Url, v: &str) {
    if quirks::set_protocol(u, v).is_ok() {
        return;
    }
    let scheme = v.split(':').next().unwrap_or_default().to_ascii_lowercase();
    let special = |s: &str| matches!(s, "ftp" | "http" | "https" | "ws" | "wss");
    if scheme != "file"
        || !special(u.scheme())
        || !u.username().is_empty()
        || u.password().is_some()
        || u.port().is_some()
    {
        return;
    }
    let Some(host) = u.host_str().map(str::to_owned) else {
        return;
    };
    let rest = &u.as_str()[u.scheme().len() + 1..];
    let Ok(mut file) = Url::parse(&format!("file:{rest}")) else {
        return;
    };
    // File parsing drops a `localhost` host and the host before a drive letter path; the
    // setter keeps both.
    if file.host_str() != Some(&host) && file.set_host(Some(&host)).is_err() {
        return;
    }
    if file.path() == u.path() {
        *u = file;
    }
}

/// Replaces the query object's list after the URL's query changed.
fn reparse_query(d: &UrlData) {
    if let Some(p) = d
        .params
        .borrow()
        .native
        .clone()
        .and_then(|n| n.downcast::<ParamsData>().ok())
    {
        *p.list.borrow_mut() = d.url.borrow().query().map(parse_query).unwrap_or_default();
    }
}

/// The URLSearchParams update steps (spec §6.2): the list's serialization becomes the query
/// of its URL object, if any.
fn update(p: &ParamsData) {
    let Some(d) = p.url.upgrade() else { return };
    let q = serialize(&p.list.borrow());
    // An empty serialization sets the query to null (which also strips an opaque path's
    // trailing spaces).
    quirks::set_search(&mut d.url.borrow_mut(), &q);
}

type Getter = fn(&Url) -> String;
type Setter = fn(&mut Url, &str);

/// The URL attributes (spec §6.1) other than `href` and `searchParams`.
const ATTRS: &[(&str, Getter, Option<Setter>)] = &[
    ("origin", quirks::origin, None),
    (
        "protocol",
        |u| quirks::protocol(u).into(),
        Some(set_protocol),
    ),
    (
        "username",
        |u| quirks::username(u).into(),
        Some(|u, v| {
            let _ = quirks::set_username(u, v);
        }),
    ),
    (
        "password",
        |u| quirks::password(u).into(),
        Some(|u, v| {
            let _ = quirks::set_password(u, v);
        }),
    ),
    (
        "host",
        |u| quirks::host(u).into(),
        Some(|u, v| {
            let _ = quirks::set_host(u, v);
        }),
    ),
    (
        "hostname",
        |u| quirks::hostname(u).into(),
        Some(|u, v| {
            let _ = quirks::set_hostname(u, v);
        }),
    ),
    (
        "port",
        |u| quirks::port(u).into(),
        Some(|u, v| {
            let _ = quirks::set_port(u, v);
        }),
    ),
    (
        "pathname",
        |u| quirks::pathname(u).into(),
        Some(quirks::set_pathname),
    ),
    (
        "search",
        |u| quirks::search(u).into(),
        Some(quirks::set_search),
    ),
    ("hash", |u| quirks::hash(u).into(), Some(quirks::set_hash)),
];

fn attr(name: &str) -> &'static (&'static str, Getter, Option<Setter>) {
    ATTRS.iter().find(|a| a.0 == name).unwrap()
}

macro_rules! accessors {
    ($p:expr, $($name:literal),*) => {$(
        define_accessor($p, $name, true, fn_value(Callable::Native($name, |it, this, _| {
            let d = this_url(it, this)?;
            let s = (attr($name).1)(&d.url.borrow());
            Ok(Value::str(&s))
        })), false);
        if attr($name).2.is_some() {
            define_accessor($p, $name, false, fn_value(Callable::Native($name, |it, this, a| {
                let d = this_url(it, this)?;
                need(it, a, 1, concat!("URL ", $name, " setter"))?;
                let v = it.to_string(&a[0])?;
                (attr($name).2.unwrap())(&mut d.url.borrow_mut(), &v);
                if $name == "search" {
                    reparse_query(&d);
                }
                Ok(Value::Undefined)
            })), false);
        }
    )*};
}

fn href(it: &mut Interp, this: &Value, _: &[Value]) -> R<Value> {
    let d = this_url(it, this)?;
    Ok(Value::str(quirks::href(&d.url.borrow())))
}

pub(crate) fn url_ctor() -> Value {
    let f = fn_value(Callable::Native("URL", construct_url));
    let Value::Obj(c) = &f else { unreachable!() };
    // V4's `URL` has no `length` of its own.
    c.borrow_mut().set_attr("length", crate::value::ABSENT);
    let p = new_obj();
    set_hidden(&p, "toString", fn_value(Callable::Native("toString", href)));
    set_hidden(&p, "toJSON", fn_value(Callable::Native("toJSON", href)));
    define_accessor(
        &p,
        "href",
        true,
        fn_value(Callable::Native("href", href)),
        false,
    );
    define_accessor(
        &p,
        "href",
        false,
        fn_value(Callable::Native("href", |it, this, a| {
            let d = this_url(it, this)?;
            need(it, a, 1, "URL href setter")?;
            let v = it.to_string(&a[0])?;
            if quirks::set_href(&mut d.url.borrow_mut(), &v).is_err() {
                return it.throw("TypeError", format!("Invalid URL: {v}"));
            }
            reparse_query(&d);
            Ok(Value::Undefined)
        })),
        false,
    );
    accessors!(
        &p, "origin", "protocol", "username", "password", "host", "hostname", "port", "pathname",
        "search", "hash"
    );
    define_accessor(
        &p,
        "searchParams",
        true,
        fn_value(Callable::Native("searchParams", |it, this, _| {
            Ok(Value::Obj(this_url(it, this)?.params.clone()))
        })),
        false,
    );
    link_builtin_prototype(c, &p);
    // URL.parse(url, base) and URL.canParse(url, base) (spec §6.1).
    set_hidden(
        c,
        "parse",
        crate::builtins::native("parse", 1, |it, _, a| {
            Ok(match api_parse(it, a, "URL.parse")? {
                Ok(url) => new_url(it, url),
                Err(_) => Value::Null,
            })
        }),
    );
    set_hidden(
        c,
        "canParse",
        crate::builtins::native("canParse", 1, |it, _, a| {
            Ok(Value::Bool(api_parse(it, a, "URL.canParse")?.is_ok()))
        }),
    );
    f
}

/// The URLSearchParams constructor (spec §6.2): `init` converted to
/// `(sequence<sequence<USVString>> or record<USVString, USVString> or USVString)` (WebIDL
/// §3.2.24): an object with an `@@iterator` method is a sequence of pairs, any other object a
/// record, anything else a string with an optional leading `?`.
fn construct_params(it: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
    let list = match arg(a, 0) {
        Value::Undefined => Vec::new(),
        v @ Value::Obj(_) => match iterator_method(it, &v)? {
            Some(m) => {
                let mut pairs = Vec::new();
                for pair in sequence(it, &v, &m)? {
                    let Some(m) = iterator_method(it, &pair)? else {
                        return it.throw(
                            "TypeError",
                            "URLSearchParams: each pair must be an iterable object",
                        );
                    };
                    let mut items = Vec::new();
                    for x in sequence(it, &pair, &m)? {
                        items.push(it.to_string(&x)?);
                    }
                    pairs.push(items);
                }
                let mut list = Vec::new();
                for items in pairs {
                    let Ok::<[String; 2], _>([k, v]) = items.try_into() else {
                        return it.throw(
                            "TypeError",
                            "URLSearchParams: each pair must have exactly two items",
                        );
                    };
                    list.push((k, v));
                }
                list
            }
            None => {
                let mut list = Vec::new();
                for k in it.keys(&v)? {
                    let x = it.get(&v, &k)?;
                    list.push((k, it.to_string(&x)?));
                }
                // An enumerable own symbol key, converted to a USVString, throws.
                if let Value::Obj(o) = &v
                    && o.borrow()
                        .ext
                        .as_ref()
                        .is_some_and(|e| e.syms.iter().any(|p| p.enumerable))
                {
                    return it.throw("TypeError", "Cannot convert a symbol to a string.");
                }
                list
            }
        },
        v => {
            let s = it.to_string(&v)?;
            parse_query(s.strip_prefix('?').unwrap_or(&s))
        }
    };
    Ok(Value::Obj(new_params(it, list, Weak::new())))
}

/// GetMethod(v, @@iterator) of object `v` (ECMA-262 §7.3.10); `None` for a non-object or no
/// method.
fn iterator_method(it: &mut Interp, v: &Value) -> R<Option<Value>> {
    if !matches!(v, Value::Obj(_)) {
        return Ok(None);
    }
    match it.get_sym(v, &WellKnown::Iterator.sym())? {
        Value::Undefined | Value::Null => Ok(None),
        m if crate::eval::callable(&m).is_some() => Ok(Some(m)),
        _ => it.throw("TypeError", "URLSearchParams: @@iterator is not a function"),
    }
}

/// Creating a sequence from iterable `v` and its `@@iterator` method `m` (WebIDL §3.2.21.1).
fn sequence(it: &mut Interp, v: &Value, m: &Value) -> R<Vec<Value>> {
    let mut src = it.iterator_from_method(v, m)?;
    it.iter_rest(&mut src)
}

pub(crate) fn params_ctor() -> Value {
    let f = fn_value(Callable::Native("URLSearchParams", construct_params));
    let Value::Obj(c) = &f else { unreachable!() };
    // V4's `URLSearchParams` has no `length` of its own.
    c.borrow_mut().set_attr("length", crate::value::ABSENT);
    let p = new_obj();
    let methods: [(&'static str, NativeFn); 12] = [
        ("toString", |it, this, _| {
            let p = this_params(it, this)?;
            Ok(Value::str(&serialize(&p.list.borrow())))
        }),
        ("sort", |it, this, _| {
            let p = this_params(it, this)?;
            // Stable, by the names' code units.
            p.list
                .borrow_mut()
                .sort_by(|a, b| a.0.encode_utf16().cmp(b.0.encode_utf16()));
            update(&p);
            Ok(Value::Undefined)
        }),
        ("append", |it, this, a| {
            let p = this_params(it, this)?;
            need(it, a, 2, "URLSearchParams.append")?;
            let (k, v) = (it.to_string(&a[0])?, it.to_string(&a[1])?);
            p.list.borrow_mut().push((k, v));
            update(&p);
            Ok(Value::Undefined)
        }),
        ("delete", |it, this, a| {
            let p = this_params(it, this)?;
            need(it, a, 1, "URLSearchParams.delete")?;
            let k = it.to_string(&a[0])?;
            let v = match arg(a, 1) {
                Value::Undefined => None,
                v => Some(it.to_string(&v)?),
            };
            p.list
                .borrow_mut()
                .retain(|e| e.0 != k || v.as_ref().is_some_and(|v| e.1 != *v));
            update(&p);
            Ok(Value::Undefined)
        }),
        ("has", |it, this, a| {
            let p = this_params(it, this)?;
            need(it, a, 1, "URLSearchParams.has")?;
            let k = it.to_string(&a[0])?;
            let v = match arg(a, 1) {
                Value::Undefined => None,
                v => Some(it.to_string(&v)?),
            };
            let has = p
                .list
                .borrow()
                .iter()
                .any(|e| e.0 == k && v.as_ref().is_none_or(|v| e.1 == *v));
            Ok(Value::Bool(has))
        }),
        ("set", |it, this, a| {
            let p = this_params(it, this)?;
            need(it, a, 2, "URLSearchParams.set")?;
            let (k, v) = (it.to_string(&a[0])?, it.to_string(&a[1])?);
            {
                let mut list = p.list.borrow_mut();
                match list.iter().position(|e| e.0 == k) {
                    Some(i) => {
                        list[i].1 = v;
                        let mut n = 0;
                        list.retain(|e| {
                            n += 1;
                            n <= i + 1 || e.0 != k
                        });
                    }
                    None => list.push((k, v)),
                }
            }
            update(&p);
            Ok(Value::Undefined)
        }),
        ("get", |it, this, a| {
            let p = this_params(it, this)?;
            need(it, a, 1, "URLSearchParams.get")?;
            let k = it.to_string(&a[0])?;
            let list = p.list.borrow();
            Ok(list
                .iter()
                .find(|e| e.0 == k)
                .map_or(Value::Null, |e| Value::str(&e.1)))
        }),
        ("getAll", |it, this, a| {
            let p = this_params(it, this)?;
            need(it, a, 1, "URLSearchParams.getAll")?;
            let k = it.to_string(&a[0])?;
            let list = p.list.borrow();
            Ok(Value::array(
                list.iter()
                    .filter(|e| e.0 == k)
                    .map(|e| Value::str(&e.1))
                    .collect(),
            ))
        }),
        ("forEach", |it, this, a| {
            let p = this_params(it, this)?;
            let f = arg(a, 0);
            if crate::eval::callable(&f).is_none() {
                return it.throw("TypeError", "Type error");
            }
            // WebIDL iterable forEach: the list is read afresh at each step.
            let mut i = 0;
            loop {
                let Some((k, v)) = p.list.borrow().get(i).cloned() else {
                    break;
                };
                it.call(
                    &f,
                    arg(a, 1),
                    &[Value::str(&v), Value::str(&k), this.clone()],
                )?;
                i += 1;
            }
            Ok(Value::Undefined)
        }),
        ("entries", |it, this, _| {
            params_iter(it, this, IterKind::Entries)
        }),
        ("keys", |it, this, _| params_iter(it, this, IterKind::Keys)),
        ("values", |it, this, _| {
            params_iter(it, this, IterKind::Values)
        }),
    ];
    for (k, m) in methods {
        set_hidden(&p, k, fn_value(Callable::Native(k, m)));
    }
    define_accessor(
        &p,
        "size",
        true,
        fn_value(Callable::Native("size", |it, this, _| {
            Ok(Value::Num(this_params(it, this)?.list.borrow().len() as f64))
        })),
        false,
    );
    link_builtin_prototype(c, &p);
    f
}

/// Where Qt 6.12's V4 deviates from the URL Standard: the expected values are the standard's.
#[cfg(all(test, feature = "interp"))]
mod tests {
    use crate::eval::{Ex, Interp};

    fn ev(src: &str) -> String {
        let body = crate::ir::lower_program(src).unwrap();
        let mut it = Interp::default();
        let g = it.global.clone();
        match it.run(&body, &g) {
            Ok(v) => v.to_js_string(),
            Err(Ex::Throw(e)) => {
                let n = it.get(&e, "name").unwrap().to_js_string();
                format!("{n}: {}", it.get(&e, "message").unwrap().to_js_string())
            }
            Err(Ex::Short) => "SHORT".into(),
        }
    }

    #[test]
    fn parses_per_the_standard() {
        for (src, want) in [
            (
                "return new URL('b/c', new URL('http://x.org/a/d')).href",
                "http://x.org/a/b/c",
            ),
            (
                "return new URL('../c?q', 'http://x.org/a/d/e#f').href",
                "http://x.org/a/c?q",
            ),
            (
                "return new URL('HTTP://EXAMPLE.com/%7e a/../b?q= r#f g').href",
                "http://example.com/b?q=%20r#f%20g",
            ),
            (
                "return new URL('http://\u{c4}\u{d6}.com/').hostname + ' ' + new URL('http://xn--nxasmq6b.com/').hostname",
                "xn--4ca0b.com xn--nxasmq6b.com",
            ),
            ("return new URL('  http://a/  ').href", "http://a/"),
            ("return new URL('http://[::1]:3/').hostname", "[::1]"),
            (
                "var u = new URL('http://a:80/'); return [u.port, u.host, u.href].join()",
                ",a,http://a/",
            ),
            (
                "var u = new URL('mailto:a@b.c'); return u.pathname + ' ' + u.origin",
                "a@b.c null",
            ),
            (
                "return new URL()",
                "TypeError: URL constructor: 1 argument(s) required, but only 0 present",
            ),
            (
                "return new URL('b', 'bad')",
                "TypeError: Invalid base URL: bad",
            ),
            (
                "return new URL('b', undefined)",
                "TypeError: Invalid URL: b",
            ),
            (
                "return Object.prototype.toString.call(new URLSearchParams())",
                "[object URLSearchParams]",
            ),
        ] {
            assert_eq!(ev(src), want, "{src}");
        }
    }

    #[test]
    fn setters_follow_the_standard() {
        for (src, want) in [
            (
                "var u = new URL('http://a/b'); u.pathname = 'c d'; return u.href",
                "http://a/c%20d",
            ),
            (
                "var u = new URL('http://a/b?x#y'); u.search = ''; u.hash = ''; return u.href",
                "http://a/b",
            ),
            (
                "var u = new URL('http://a/b'); u.port = 'x'; u.protocol = '1'; return u.href",
                "http://a/b",
            ),
            (
                "var u = new URL('http://a/b'); return u.href = 'bad'",
                "TypeError: Invalid URL: bad",
            ),
            (
                "return URL.prototype.href",
                "TypeError: Value of \"this\" must be of type URL",
            ),
        ] {
            assert_eq!(ev(src), want, "{src}");
        }
    }

    #[test]
    fn search_params_stay_linked_to_the_url() {
        for (src, want) in [
            (
                "var u = new URL('http://a/?x=1'); u.searchParams.append('y', '2 3'); return u.href",
                "http://a/?x=1&y=2+3",
            ),
            (
                "var u = new URL('http://a/?x=1'); var sp = u.searchParams; u.search = '?q=9'; return [sp.toString(), sp === u.searchParams]",
                "q=9,true",
            ),
            (
                "var u = new URL('http://a/?x=1'); var sp = u.searchParams; u.href = 'http://b/?z=%20'; return sp.get('z') + '|'",
                " |",
            ),
            (
                "var u = new URL('http://a/b?x=1#h'); u.searchParams.delete('x'); return u.href + ' ' + u.search",
                "http://a/b#h ",
            ),
            (
                "var u = new URL('http://a/b'); u.searchParams.sort(); return u.href",
                "http://a/b",
            ),
        ] {
            assert_eq!(ev(src), want, "{src}");
        }
    }

    #[test]
    fn search_params_follow_the_standard() {
        for (src, want) in [
            (
                "var p = new URLSearchParams('?q=a+b%20c&e=%E2%82%AC'); return p.get('q') + '|' + p.get('e')",
                "a b c|\u{20ac}",
            ),
            (
                "var p = new URLSearchParams(); p.append('z', 'a b'); return p.toString()",
                "z=a+b",
            ),
            (
                "return new URLSearchParams(new URLSearchParams('a=1&b=2')).toString()",
                "a=1&b=2",
            ),
            ("return new URLSearchParams(null).toString()", "null="),
            (
                "return new URLSearchParams([['a', 'b', 'c']])",
                "TypeError: URLSearchParams: each pair must have exactly two items",
            ),
            (
                "var p = new URLSearchParams('a=1&a=2&b=1'); p.delete('a', '1'); return p.toString()",
                "a=2&b=1",
            ),
            (
                "var p = new URLSearchParams('a=1&b=1&a=2'); p.set('a', '3'); return p.toString()",
                "a=3&b=1",
            ),
            (
                "var p = new URLSearchParams('a=1'); return [p.has('a', '1'), p.has('a', '2')].join()",
                "true,false",
            ),
            (
                "var p = new URLSearchParams('\u{fb03}=1&\u{1f308}=2&a=3'); p.sort(); return Array.from(p.keys()).join()",
                "a,\u{1f308},\u{fb03}",
            ),
            (
                "var p = new URLSearchParams('a=1&b=2'); var r = []; p.forEach(function (v, k, o) { r.push(k + v + (o === p) + this.t) }, { t: 'T' }); return r.join()",
                "a1trueT,b2trueT",
            ),
            (
                "var p = new URLSearchParams('a=1&b=2'); return JSON.stringify([[...p.entries()], [...p.keys()], [...p.values()], Array.from(p), [...p]])",
                r#"[[["a","1"],["b","2"]],["a","b"],["1","2"],[["a","1"],["b","2"]],[["a","1"],["b","2"]]]"#,
            ),
            (
                "var r = []; for (var x of new URLSearchParams('a=1')) r.push(x.join('=')); return r.join()",
                "a=1",
            ),
            (
                "return new URLSearchParams('a').append('b')",
                "TypeError: URLSearchParams.append: 2 argument(s) required, but only 1 present",
            ),
            (
                "return URLSearchParams.prototype.get.call({}, 'a')",
                "TypeError: Value of \"this\" must be of type URLSearchParams",
            ),
        ] {
            assert_eq!(ev(src), want, "{src}");
        }
    }
    /// WebIDL §3.7.10: live default iterator objects over the list, with no done flag (an
    /// exhausted iterator yields again once the list grows), and the sequence init (§3.2.24)
    /// taking any object with an `@@iterator`.
    #[test]
    fn search_params_iterators_are_live() {
        for (src, want) in [
            (
                "var p = new URLSearchParams('a=1&b=2'), i = p.keys(); var r = [i.next().value, i.next().value, i.next().done]; p.append('c', '3'); r.push(i.next().value, i.next().done); return r.join()",
                "a,b,true,c,true",
            ),
            (
                "var p = new URLSearchParams('x=1'), r = []; for (var [k] of p) { r.push(k); if (r.length < 3) p.append('y' + r.length, '') } return r.join()",
                "x,y1,y2",
            ),
            (
                "var p = new URLSearchParams('a=1&b=2&c=3'), r = []; for (var v of p.values()) { r.push(v); if (v == '1') p.delete('a') } return r.join()",
                "1,3",
            ),
            (
                "var u = new URL('http://h/?m=1&n=2'), i = u.searchParams.entries(); var r = [i.next().value]; u.search = '?z=9&w=8'; r.push(i.next().value); return JSON.stringify(r)",
                r#"[["m","1"],["w","8"]]"#,
            ),
            (
                "var p = new URLSearchParams('a=1'), i = p.entries(), P = Object.getPrototypeOf(i); return [Object.prototype.toString.call(i), P === Object.getPrototypeOf(p.values()), Object.getPrototypeOf(P) === Object.getPrototypeOf(Object.getPrototypeOf([].keys())), i[Symbol.iterator]() === i, URLSearchParams.prototype[Symbol.iterator] === URLSearchParams.prototype.entries, Object.getOwnPropertyNames(P), P.next.length].join()",
                "[object URLSearchParams Iterator],true,true,true,true,next,0",
            ),
            (
                "var [a, b, c] = new URLSearchParams('a=1&b=2'); return JSON.stringify([a, b, c])",
                r#"[["a","1"],["b","2"],null]"#,
            ),
            (
                "return [new URLSearchParams(new URLSearchParams('a=1&b=2').entries()), new URLSearchParams(new Map([['m', 1]])), new URLSearchParams([new Set(['s', 't'])])].map(String).join('|')",
                "a=1&b=2|m=1|s=t",
            ),
            (
                "var log = []; try { new URLSearchParams([['a', { toString() { log.push('a'); return 'a' } }], ['b']]) } catch (e) { log.push(e.name) } return log.join()",
                "a,TypeError",
            ),
            (
                "return Object.getPrototypeOf(new URLSearchParams().keys()).next.call([].keys())",
                "TypeError: Not a URLSearchParams Iterator instance",
            ),
            (
                "return [].keys().next.call(new URLSearchParams('a=1').keys()).value",
                "TypeError: Not an Array Iterator instance",
            ),
            (
                "return URLSearchParams.prototype.values.call(new Map())",
                "TypeError: Value of \"this\" must be of type URLSearchParams",
            ),
            (
                "return new URLSearchParams(['ab'])",
                "TypeError: URLSearchParams: each pair must be an iterable object",
            ),
            (
                "return new URLSearchParams({ [Symbol.iterator]: 5 })",
                "TypeError: URLSearchParams: @@iterator is not a function",
            ),
            (
                "return new URLSearchParams({ [Symbol.iterator]: undefined, r: 1 }).toString()",
                "TypeError: Cannot convert a symbol to a string.",
            ),
        ] {
            assert_eq!(ev(src), want, "{src}");
        }
    }

    #[test]
    fn protocol_size_parse_and_can_parse_per_the_standard() {
        // Node 22 (WHATWG URL) gives the same results; Qt 6.12's V4 keeps `http://a/` for the
        // `file` switch and has neither `size` nor `URL.parse`/`URL.canParse`.
        let proto =
            |h: &str, p: &str| format!("var u = new URL('{h}'); u.protocol = '{p}'; return u.href");
        for (src, want) in [
            (proto("http://a/", "file"), "file://a/"),
            (proto("http://localhost/x", "file"), "file://localhost/x"),
            (proto("http://a/C:/x", "file"), "file://a/C:/x"),
            (proto("https://a/p?q#f", "FILE"), "file://a/p?q#f"),
            (proto("http://a/", "file:junk"), "file://a/"),
            (proto("http://u@a/", "file"), "http://u@a/"),
            (proto("http://a:81/", "file"), "http://a:81/"),
            (proto("file:///x", "http"), "file:///x"),
            (proto("file://h/x", "http"), "http://h/x"),
            (proto("ws://a:80/", "http"), "http://a/"),
            (proto("http://a/", "x"), "http://a/"),
            (
                "var p = new URLSearchParams('a=1&b=2&a=3'); var n = p.size; p.delete('a'); return [n, p.size].join()".into(),
                "3,1",
            ),
            (
                "var u = new URL('http://h/?x=1&y=2'); var n = u.searchParams.size; u.search = ''; return [n, u.searchParams.size].join()".into(),
                "2,0",
            ),
            (
                "return Object.getOwnPropertyDescriptor(URLSearchParams.prototype, 'size').get.call({})".into(),
                "TypeError: Value of \"this\" must be of type URLSearchParams",
            ),
            (
                "return [URL.canParse('x:'), URL.canParse('/a'), URL.canParse('/a', 'http://h'), URL.canParse('/a', 'bad'), URL.canParse('http://[::1')].join()".into(),
                "true,false,true,false,false",
            ),
            (
                "var u = URL.parse('/a?q#f', 'http://h/b'); return [u, u instanceof URL, URL.parse('bad'), URL.parse('/a', 'bad'), URL.parse.length, URL.canParse.length].join()".into(),
                "http://h/a?q#f,true,,,1,1",
            ),
            (
                "return URL.canParse()".into(),
                "TypeError: URL.canParse: 1 argument(s) required, but only 0 present",
            ),
        ] {
            assert_eq!(ev(&src), want, "{src}");
        }
    }
}
