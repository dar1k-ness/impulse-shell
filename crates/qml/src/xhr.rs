//! QML `XMLHttpRequest` as Qt 6.12's QQmlXMLHttpRequest (qqmlxmlhttprequest.cpp) behaves:
//! the handlers run on every state the request enters, and `status`, `statusText`,
//! `responseText` and the response headers read in each state what Qt gives there.
use crate::eval::{
    Callable, Ex, Interp, NativeFn, R, define_accessor, fn_value, new_obj, set_hidden, set_own,
};
use crate::value::Value;
use std::cell::{Cell, RefCell};
use std::io::Read;
use std::rc::Rc;

const UNSENT: u8 = 0;
const OPENED: u8 = 1;
const HEADERS_RECEIVED: u8 = 2;
const LOADING: u8 = 3;
const DONE: u8 = 4;

/// QQmlXMLHttpRequest follows this many redirects less one.
const MAX_REDIRECTS: u32 = 15;

/// A request goes out again this many times to answer a 401, then the 401 is the page's. Qt
/// 6.12.0 has no bound: a server that calls every Digest answer stale keeps it sending
/// until the page aborts the request, and a synchronous one for 30 s, after which it fails.
const MAX_ANSWERS: u32 = 1000;

/// The status line and headers of the reply in flight (what Qt reads off its QNetworkReply).
#[derive(Default)]
struct Reply {
    status: u16,
    reason: String,
    headers: Vec<(String, String)>,
}

/// An XMLHttpRequest's state: QQmlXMLHttpRequest's members.
#[derive(Default)]
pub(crate) struct XhrRequest {
    /// In capitals, one of [`METHODS`].
    method: RefCell<String>,
    /// The address as `open` got it, without its fragment.
    url: RefCell<String>,
    /// Raw headers in `setRequestHeader` order (m_request's). `open` keeps them, as Qt does.
    headers: RefCell<Vec<(String, String)>>,
    /// The names set since `open` (m_addedHeaders): one of them set again (in any case) gets
    /// the value added after a comma, any other header is replaced.
    added: RefCell<Vec<String>>,
    /// What `send` got to send (m_data).
    data: RefCell<Vec<u8>>,
    /// `open(method, url, false)`: `send` returns after the last handler ran.
    sync: Cell<bool>,
    state: Cell<u8>,
    /// `send` was called since `open` (m_sendFlag).
    sent: Cell<bool>,
    /// The request failed or was aborted (m_errorFlag): `status` reads 0, `statusText` "".
    failed: Cell<bool>,
    /// Counts `open` and `abort`: what a request started before the last of them still
    /// reports is dropped.
    serial: Cell<u64>,
    reply: RefCell<Reply>,
    status: Cell<u16>,
    reason: RefCell<String>,
    /// Filled when HEADERS_RECEIVED is entered and kept after that (m_headersList).
    response_headers: RefCell<Vec<(String, String)>>,
    body: RefCell<Vec<u8>>,
    /// The address the reply in flight is asked from (m_url): `open`'s, then each redirect's.
    asked: RefCell<String>,
    /// What `responseType` was set to, as a string.
    response_type: RefCell<String>,
    /// What the last finished reply's `Content-Type` named (m_mime, m_charset), and whether
    /// a reply so far was XML (m_gotXml). As in Qt, `open` clears none of these six.
    mime: RefCell<Vec<u8>>,
    charset: RefCell<Vec<u8>>,
    got_xml: Cell<bool>,
    /// `overrideMimeType`'s type and charset.
    override_mime: RefCell<Vec<u8>>,
    override_charset: RefCell<Vec<u8>>,
    /// The log has the line about this reply's charset that is not written here.
    said_unread: Cell<bool>,
    /// The object or document `response` was first read as (m_parsedDocument).
    parsed: RefCell<Option<Value>>,
    /// The request has no context to run handlers in (m_qmlContext is null): Qt 6.12.0
    /// drops it once the last handlers of a finished reply ran, and `send` sets it. A
    /// request sent from those handlers goes on without it: its states are entered and
    /// read as ever, but no handler runs for them.
    mute: Cell<bool>,
    /// The logins the requests of this engine remember.
    logins: crate::http::login::Shared,
}

/// What the request thread reports, in order; `Finished` or `Failed` is the last.
pub(crate) enum Event {
    /// A reply's status line and headers arrived.
    Reply {
        status: u16,
        reason: String,
        headers: Vec<(String, String)>,
    },
    /// Body bytes arrived (QNetworkReply::readyRead).
    Data(Vec<u8>),
    /// The reply was a redirect that is being followed to this address: its body is
    /// discarded.
    Redirected(String),
    /// The reply's body is not what its `Content-Encoding` says (QNetworkReply reports an
    /// error that QQmlXMLHttpRequest takes as a reply).
    Undecodable,
    /// The last reply ended as its headers announced.
    Finished,
    /// No reply, or one cut short (QNetworkReply::errorOccurred without an HTTP status).
    Failed,
}

/// A request in flight: its JS object, what its thread reports and its stopper.
pub struct Pending {
    pub(crate) obj: Value,
    pub(crate) rx: std::sync::mpsc::Receiver<Event>,
    pub(crate) serial: u64,
    pub(crate) _stop: crate::runtime::StopOnDrop,
}

/// What `send` puts on the wire.
pub(crate) struct Outgoing {
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body: Option<Vec<u8>>,
    /// The headers of the GET a 303 reply turns the request into.
    headers_of_get: Vec<(String, String)>,
    /// `send` waits for the reply.
    sync: bool,
    logins: crate::http::login::Shared,
    /// Where an asynchronous request stands among those that wait for a socket to its host.
    place: Option<crate::http::login::Place>,
}

/// The methods `open` takes.
const METHODS: [&str; 8] = [
    "GET", "PUT", "HEAD", "POST", "DELETE", "OPTIONS", "PROPFIND", "PATCH",
];

/// Request headers `setRequestHeader` silently leaves out.
fn forbidden_header(name: &str) -> bool {
    let n = name.to_uppercase();
    [
        "ACCEPT-CHARSET",
        "ACCEPT-ENCODING",
        "CONNECTION",
        "CONTENT-LENGTH",
        "COOKIE",
        "COOKIE2",
        "CONTENT-TRANSFER-ENCODING",
        "DATE",
        "EXPECT",
        "HOST",
        "KEEP-ALIVE",
        "REFERER",
        "TE",
        "TRAILER",
        "TRANSFER-ENCODING",
        "UPGRADE",
        "VIA",
    ]
    .contains(&n.as_str())
        || n.starts_with("PROXY-")
        || n.starts_with("SEC-")
}

/// A header name as QHttpHeaders writes it in a request: each word between hyphens with a
/// capital first letter and the rest small.
fn capitalised(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut first = true;
    for c in name.chars() {
        out.push(if first {
            c.to_ascii_uppercase()
        } else {
            c.to_ascii_lowercase()
        });
        first = c == '-';
    }
    out
}

/// QQmlXMLHttpRequest::requestFromUrl's `Content-Type` of a POST or PUT: `have` with its
/// charset made UTF-8, or plain text in UTF-8.
fn content_type(have: Option<&str>) -> String {
    let Some(have) = have else {
        return "text/plain;charset=UTF-8".into();
    };
    match have.find("charset=") {
        Some(i) => {
            let from = i + 8;
            let to = have[from..].find(';').map_or(have.len(), |n| from + n);
            format!("{}UTF-8{}", &have[..from], &have[to..])
        }
        None if have.is_empty() => "charset=UTF-8".into(),
        None => format!("{have};charset=UTF-8"),
    }
}

/// `url` with `open`'s user and password in it (QUrl::setUserName and setPassword), as
/// `responseURL` shows it.
fn with_login(url: &str, user: Option<String>, password: Option<String>) -> String {
    let Some(at) = url.find("://").map(|i| i + 3) else {
        return url.into();
    };
    if user.is_none() && password.is_none() {
        return url.into();
    }
    let end = url[at..].find(['/', '?']).map_or(url.len(), |n| at + n);
    let (had, host) = match url[at..end].rsplit_once('@') {
        Some((had, host)) => (Some(had), host),
        None => (None, &url[at..end]),
    };
    let (had_user, had_password) = match had.map(|h| h.split_once(':').unwrap_or((h, ""))) {
        Some((u, p)) => (u.to_string(), p.to_string()),
        None => Default::default(),
    };
    // QUrl shows these of a user name or a password as escapes.
    let shown = |part: String, colon: bool| -> String {
        part.chars()
            .map(|c| match c {
                '/' | '?' | '#' | '[' | ']' | '@' | '%' | '"' | '<' | '>' | '\\' | '^' | '`'
                | '{' | '|' | '}' => format!("%{:02X}", c as u32),
                c if c.is_ascii_control() => format!("%{:02X}", c as u32),
                ':' if colon => "%3A".into(),
                c => c.into(),
            })
            .collect()
    };
    let user = user.map_or(had_user, |u| shown(u, true));
    let password = password.map_or(had_password, |p| shown(p, false));
    let colon = if password.is_empty() { "" } else { ":" };
    format!(
        "{}{user}{colon}{password}@{host}{}",
        &url[..at],
        &url[end..]
    )
}

/// QHttpNetworkConnection's `Accept-Language` for the system locale.
fn accept_language() -> String {
    let l = crate::builtins::system_locale_name().replace('_', "-");
    if l == "C" {
        "en,*".into()
    } else if l.starts_with("en-") {
        format!("{l},*")
    } else {
        format!("{l},en,*")
    }
}

pub(crate) fn xhr_request(this: &Value) -> Option<Rc<XhrRequest>> {
    match this {
        Value::Obj(o) => o.borrow().native.clone()?.downcast::<XhrRequest>().ok(),
        _ => None,
    }
}

impl XhrRequest {
    pub(crate) fn serial(&self) -> u64 {
        self.serial.get()
    }

    /// The headers a request with `method` goes out with: the page's and the ones
    /// QNetworkAccessManager adds, in the order it sends them. The page's come first as they
    /// were set (the `Content-Type` of a POST or PUT last of them, with its charset made
    /// UTF-8), then `Connection`, `Accept-Encoding` and, unless the page set them,
    /// `Accept-Language` and `User-Agent`. A name goes out with each word capitalised.
    fn headers_for(&self, method: &str) -> Vec<(String, String)> {
        let mut headers: Vec<(String, String)> = self
            .headers
            .borrow()
            .iter()
            .map(|(k, v)| (capitalised(k), v.clone()))
            .collect();
        let has = |h: &[(String, String)], name: &str| {
            h.iter().position(|(k, _)| k.eq_ignore_ascii_case(name))
        };
        if matches!(method, "POST" | "PUT") {
            let have = has(&headers, "content-type").map(|i| headers.remove(i).1);
            headers.push(("Content-Type".into(), content_type(have.as_deref())));
        }
        headers.push(("Connection".into(), "Keep-Alive".into()));
        headers.push(("Accept-Encoding".into(), "zstd, br, gzip, deflate".into()));
        if has(&headers, "accept-language").is_none() {
            headers.push(("Accept-Language".into(), accept_language()));
        }
        if has(&headers, "user-agent").is_none() {
            // QQmlXMLHttpRequest's QNetworkAccessManager default.
            headers.push(("User-Agent".into(), "Mozilla/5.0".into()));
        }
        headers
    }

    /// The request `send` makes now.
    pub(crate) fn outgoing(&self) -> Outgoing {
        let method = self.method.borrow().clone();
        // QQmlXMLHttpRequest::send: the other methods go out without what `send` got.
        let body = matches!(
            method.as_str(),
            "POST" | "PUT" | "OPTIONS" | "PROPFIND" | "PATCH"
        )
        .then(|| self.data.borrow().clone());
        // With the login `open` got: it answers a 401.
        let url = crate::native::qurl_encoded(&self.asked.borrow());
        let sync = self.sync.get();
        // Qt starts the requests of a host in the order they were sent.
        let place = (!sync).then(|| {
            crate::http::login::Place::take(&self.logins, &crate::http::login::split(&url).0)
        });
        Outgoing {
            url,
            headers: self.headers_for(&method),
            headers_of_get: self.headers_for("GET"),
            method,
            body,
            sync,
            logins: self.logins.clone(),
            place,
        }
    }

    /// `status` and the header list come from the reply in flight.
    fn read_status(&self) {
        let reply = self.reply.borrow();
        self.status.set(reply.status);
        self.reason.borrow_mut().clone_from(&reply.reason);
    }

    /// QQmlXMLHttpRequest::fillHeadersList: names in lower case, a repeated name once with
    /// its values joined, cookies left out.
    fn fill_headers(&self) {
        let mut list: Vec<(String, String)> = vec![];
        for (k, v) in &self.reply.borrow().headers {
            let k = k.to_ascii_lowercase();
            if k == "set-cookie" || k == "set-cookie2" {
                continue;
            }
            match list.iter_mut().find(|e| e.0 == k) {
                Some(e) => {
                    e.1.push_str(", ");
                    e.1.push_str(v);
                }
                None => list.push((k, v.clone())),
            }
        }
        *self.response_headers.borrow_mut() = list;
    }

    fn has_reply(&self) -> bool {
        !matches!(self.state.get(), UNSENT | OPENED)
    }

    /// LOADING or DONE: the states `response` and its kin read something in.
    fn has_body(&self) -> bool {
        matches!(self.state.get(), LOADING | DONE)
    }

    /// The overriding type, else the reply's.
    fn mime(&self) -> Vec<u8> {
        let over = self.override_mime.borrow();
        if over.is_empty() {
            self.mime.borrow().clone()
        } else {
            over.clone()
        }
    }

    /// QQmlXMLHttpRequest::readEncoding: the type and charset of the reply's `Content-Type`.
    /// The charset is what follows `charset=` in small letters, and it runs as many bytes
    /// as there are before the `;` after it, counted from the start of the header.
    fn read_encoding(&self) {
        let headers = self.response_headers.borrow();
        if let Some((_, v)) = headers.iter().find(|e| e.0 == "content-type") {
            let v = v.as_bytes();
            let find = |from: usize, what: &[u8]| {
                v[from..]
                    .windows(what.len())
                    .position(|w| w == what)
                    .map(|i| from + i)
            };
            match find(0, b";") {
                None => *self.mime.borrow_mut() = v.to_vec(),
                Some(end) => {
                    *self.mime.borrow_mut() = v[..end].to_vec();
                    if let Some(at) = find(0, b"charset=") {
                        let from = at + 8;
                        let len = find(from, b";").unwrap_or(v.len());
                        *self.charset.borrow_mut() = v[from..v.len().min(from + len)].to_vec();
                    }
                }
            }
        }
        let mime = self.mime();
        if mime.is_empty()
            || mime == b"text/xml"
            || mime == b"application/xml"
            || mime.ends_with(b"+xml")
        {
            self.got_xml.set(true);
        }
    }

    /// QQmlXMLHttpRequest::responseBody: the bytes so far as text. A charset Qt has a
    /// converter for and this runtime has not is logged, once for a reply, and the reply is
    /// read as one that names no charset (docs/qt-differences.md).
    fn text(&self) -> crate::xhr_text::Text {
        let over = self.override_charset.borrow();
        let charset = self.charset.borrow();
        let name = if over.is_empty() { &*charset } else { &*over };
        let body = self.body.borrow();
        let (found, unread) = crate::xhr_text::find(name, self.got_xml.get(), &self.mime(), &body);
        self.say_unread(unread);
        crate::xhr_text::text(found, &body)
    }

    fn say_unread(&self, name: Option<Vec<u8>>) {
        if let Some(name) = name
            && !self.said_unread.replace(true)
        {
            elog::eprintln!("{}", crate::xhr_text::unread(&name));
        }
    }

    /// `responseText`: a string of JavaScript holds a surrogate without its pair as Qt's
    /// decoders leave it.
    fn text_value(&self) -> Value {
        match self.text() {
            crate::xhr_text::Text::Whole(text) => Value::str(&text),
            crate::xhr_text::Text::Units(units) => Value::from_utf16(&units),
        }
    }

    /// `responseType` is `kind`, in any case.
    fn wants(&self, kind: &str) -> bool {
        self.response_type.borrow().eq_ignore_ascii_case(kind)
    }

    /// `response` as an object: the one read first is kept for good, whether it was JSON
    /// or a document. JSON that does not parse throws and is read again next time.
    fn parsed(&self, it: &mut Interp, json: bool) -> R<Value> {
        if let Some(v) = &*self.parsed.borrow() {
            return Ok(v.clone());
        }
        let v = if json {
            match crate::json::parse(&self.text().lossy()) {
                Ok(v) => v,
                Err(_) => return it.throw("SyntaxError", "JSON.parse: Parse error"),
            }
        } else {
            let body = self.body.borrow();
            let named = crate::xhr_text::xml_encoding(&body);
            self.say_unread(named.filter(|n| {
                crate::xhr_text::charset(n) == Some(crate::xhr_text::Charset::NotWritten)
            }));
            match crate::xhr_dom::parse(&body) {
                Some(doc) => crate::xhr_dom::document(it, doc),
                None => Value::Null,
            }
        };
        *self.parsed.borrow_mut() = Some(v.clone());
        Ok(v)
    }

    /// QNetworkReply reported an error, an HTTP one too: QQmlXMLHttpRequest::error drops
    /// the request's headers with its QNetworkRequest.
    fn errored(&self) {
        self.headers.borrow_mut().clear();
    }
}

/// Qt's `THROW_DOM(code, message)`.
fn dom_error<T>(it: &Interp, code: u8, message: &str) -> R<T> {
    let e = it.make_error("Error", Some(message.into()));
    if let Value::Obj(o) = &e {
        set_own(o, "code", Value::Num(code.into()));
    }
    Err(Ex::Throw(e))
}

/// `DOMEXCEPTION_INVALID_STATE_ERR`.
fn invalid_state<T>(it: &Interp) -> R<T> {
    dom_error(it, 11, "Invalid state")
}

/// `DOMEXCEPTION_SYNTAX_ERR` for a call with the wrong number of arguments.
fn argument_count<T>(it: &Interp) -> R<T> {
    dom_error(it, 12, "Incorrect argument count")
}

/// Runs handler `name` of `this` if it is a function; what it throws is logged, as Qt does.
fn call_handler(it: &mut Interp, this: &Value, name: &str) {
    let Value::Obj(o) = this else { return };
    let f = o.borrow().props.get(name).cloned();
    let Some(f) = f.filter(|f| crate::eval::callable(f).is_some()) else {
        return;
    };
    if let Err(Ex::Throw(e)) = it.call(&f, Value::Undefined, &[]) {
        let m = it
            .get(&e, "message")
            .map(|v| v.to_js_string())
            .unwrap_or_default();
        if let Some(w) = crate::runtime::world_of(it) {
            w.borrow_mut()
                .errors
                .push(format!("XMLHttpRequest.{name}: {m}"));
        }
    }
}

/// QQmlXMLHttpRequest::dispatchCallbackNow: `onreadystatechange`, and in DONE `onerror` or
/// `onload`, then `onloadend`.
fn dispatch(it: &mut Interp, this: &Value, r: &XhrRequest) {
    let (done, failed) = (r.state.get() == DONE, r.failed.get());
    call_handler(it, this, "onreadystatechange");
    if done {
        call_handler(it, this, if failed { "onerror" } else { "onload" });
        call_handler(it, this, "onloadend");
    }
}

/// Applies what request `serial` of `this` reported and runs the handlers for the states it
/// enters (QQmlXMLHttpRequest::readyRead, finished and error). True once the request needs
/// nothing more: it ended, or a handler opened or aborted it.
pub(crate) fn deliver(it: &mut Interp, this: &Value, serial: u64, e: Event) -> bool {
    let Some(r) = xhr_request(this) else {
        return true;
    };
    if r.serial.get() != serial {
        return true;
    }
    // False once a handler opened or aborted the request.
    // QQmlXMLHttpRequest::dispatchCallbackSafely.
    let enter = |it: &mut Interp, state: u8| {
        r.state.set(state);
        if !r.mute.get() {
            dispatch(it, this, &r);
        }
        r.serial.get() == serial
    };
    match e {
        Event::Reply {
            status,
            reason,
            headers,
        } => {
            *r.reply.borrow_mut() = Reply {
                status,
                reason,
                headers,
            };
            false
        }
        Event::Redirected(to) => {
            r.body.borrow_mut().clear();
            *r.asked.borrow_mut() = to;
            false
        }
        Event::Data(d) => {
            r.read_status();
            if r.state.get() < HEADERS_RECEIVED {
                r.fill_headers();
                if !enter(it, HEADERS_RECEIVED) {
                    return true;
                }
            }
            let state = {
                let mut body = r.body.borrow_mut();
                let was_empty = body.is_empty();
                body.extend_from_slice(&d);
                if was_empty && !body.is_empty() {
                    LOADING
                } else {
                    r.state.get()
                }
            };
            !enter(it, state)
        }
        Event::Finished | Event::Undecodable => {
            r.read_status();
            r.data.borrow_mut().clear();
            match r.status.get() {
                // QNetworkReply reports these as errors. Proxy authentication is the one
                // QQmlXMLHttpRequest::error does not pass on as a reply.
                407 => {
                    r.errored();
                    r.failed.set(true);
                    r.body.borrow_mut().clear();
                }
                // The header list is not filled on this path: a reply without a body has none.
                // The reply's type and charset are not read on it either.
                status if status >= 400 || matches!(e, Event::Undecodable) => {
                    r.errored();
                    if !enter(it, LOADING) {
                        return true;
                    }
                }
                _ => {
                    if r.state.get() < HEADERS_RECEIVED {
                        r.fill_headers();
                        if !enter(it, HEADERS_RECEIVED) {
                            return true;
                        }
                    }
                    r.read_encoding();
                    if r.state.get() < LOADING && !enter(it, LOADING) {
                        return true;
                    }
                    // QQmlXMLHttpRequest::finished ends with `m_qmlContext.reset()`, after
                    // the handlers: one of them may have sent the request again.
                    enter(it, DONE);
                    r.mute.set(true);
                    return true;
                }
            }
            enter(it, DONE);
            true
        }
        Event::Failed => {
            r.read_status();
            r.errored();
            r.data.borrow_mut().clear();
            r.failed.set(true);
            r.body.borrow_mut().clear();
            enter(it, DONE);
            true
        }
    }
}

/// The address a redirect reply sends the request to (QNetworkRequest::RedirectionTargetAttribute
/// resolved against the reply's address); Qt does not follow one to a local file.
fn redirect_target(status: u16, headers: &[(String, String)], from: &str) -> Option<String> {
    if !matches!(status, 301 | 302 | 303 | 305 | 307 | 308) {
        return None;
    }
    let to = &headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("location"))?
        .1;
    let url = url::Url::parse(from).ok()?.join(to).ok()?;
    (!matches!(url.scheme(), "file" | "qrc")).then(|| url.into())
}

/// A `Content-Encoding` QNetworkAccessManager decodes.
#[derive(Clone, Copy)]
enum Encoding {
    Gzip,
    Deflate,
    Brotli,
    Zstd,
}

fn content_encoding(headers: &[(String, String)]) -> Option<(Encoding, &str)> {
    let name = &headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-encoding"))?
        .1;
    let encoding = match name.to_ascii_lowercase().as_str() {
        "gzip" => Encoding::Gzip,
        "deflate" => Encoding::Deflate,
        "br" => Encoding::Brotli,
        "zstd" => Encoding::Zstd,
        _ => return None,
    };
    Some((encoding, name))
}

/// A reply's body as the server sends it; notes that reading it failed, which is not the
/// decoder above it failing.
struct Wire {
    body: Box<dyn Read + Send>,
    broke: Rc<Cell<bool>>,
    /// The body was read to its end.
    ended: Rc<Cell<bool>>,
}

impl Read for Wire {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.body.read(buf).inspect_err(|_| self.broke.set(true))?;
        self.ended
            .set(self.ended.get() || n == 0 && !buf.is_empty());
        Ok(n)
    }
}

/// What reads `wire` decoded from `encoding`. An empty body is not decoded; `deflate` is a
/// zlib stream when it starts as one and a bare deflate stream otherwise. `Err` tells
/// whether the body is not in that encoding (true) or did not arrive (false).
fn decoded(mut wire: Wire, encoding: Encoding) -> Result<Box<dyn Read>, bool> {
    if let Encoding::Zstd = encoding {
        return match ZstdFrames::new(wire) {
            Ok(frames) => Ok(Box::new(frames)),
            Err(_) => Err(true),
        };
    }
    let mut first = [0u8; 2];
    let mut have = 0;
    while have < first.len() {
        match wire.read(&mut first[have..]) {
            Ok(0) => break,
            Ok(n) => have += n,
            Err(_) => return Err(false),
        }
    }
    if have == 0 {
        return Ok(Box::new(std::io::empty()));
    }
    let zlib = have == 2 && first[0] & 0x0F == 8 && u16::from_be_bytes(first).is_multiple_of(31);
    let raw = std::io::Cursor::new(first[..have].to_vec()).chain(wire);
    Ok(match encoding {
        Encoding::Gzip => Box::new(flate2::read::MultiGzDecoder::new(raw)),
        Encoding::Deflate if zlib => Box::new(flate2::read::ZlibDecoder::new(raw)),
        Encoding::Deflate => Box::new(flate2::read::DeflateDecoder::new(raw)),
        Encoding::Brotli => Box::new(brotli_decompressor::Decompressor::new(raw, 4096)),
        Encoding::Zstd => unreachable!(),
    })
}

/// A zstd body decoded by libzstd, which decodes for QNetworkReply too: it takes no frame
/// that asks for more than the last 128 MiB to be kept, and a frame that stops short gives
/// what came. The body goes to it in the pieces it came in, as QNetworkReply's does: libzstd
/// knows a frame of an old format only when the piece that starts it has five bytes of it.
/// The libzstd QNetworkReply loads reads the formats from 0.5 on and this one the older ones
/// too, so a frame of those is refused here, wherever in the body it starts.
struct ZstdFrames {
    wire: Wire,
    decoder: zstd::stream::raw::Decoder<'static>,
    piece: Vec<u8>,
    /// How much of `piece` the decoder has taken.
    taken: usize,
    /// No frame is being decoded: the next byte starts one.
    between: bool,
    ended: bool,
}

impl ZstdFrames {
    fn new(wire: Wire) -> std::io::Result<Self> {
        Ok(ZstdFrames {
            wire,
            decoder: zstd::stream::raw::Decoder::new()?,
            piece: vec![],
            taken: 0,
            between: true,
            ended: false,
        })
    }
}

impl Read for ZstdFrames {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        use zstd::stream::raw::{InBuffer, Operation, OutBuffer};
        loop {
            let rest = &self.piece[self.taken..];
            // Between frames with nothing to give it, the decoder has nothing to give back.
            if !(self.between && rest.is_empty()) {
                if self.between {
                    if before_zstd_05(rest) {
                        return Err(std::io::Error::other("a zstd frame of a format before 0.5"));
                    }
                    self.decoder.reinit()?;
                    self.between = false;
                }
                let mut from = InBuffer::around(rest);
                let mut to = OutBuffer::around(&mut *buf);
                // 0 when the frame has ended.
                self.between = self.decoder.run(&mut from, &mut to)? == 0;
                self.taken += from.pos();
                let gave = to.pos();
                if gave > 0 || buf.is_empty() {
                    return Ok(gave);
                }
                if self.taken < self.piece.len() {
                    continue;
                }
            }
            if self.ended {
                return match self.between {
                    true => Ok(0),
                    false => Err(std::io::ErrorKind::UnexpectedEof.into()),
                };
            }
            self.piece.resize(64 << 10, 0);
            let n = self.wire.read(&mut self.piece)?;
            self.piece.truncate(n);
            self.taken = 0;
            self.ended = n == 0;
        }
    }
}

/// Whether `start` begins a zstd frame of the formats 0.1 to 0.4.
fn before_zstd_05(start: &[u8]) -> bool {
    matches!(start, [0x1E | 0x22..=0x24, 0xB5, 0x2F, 0xFD, ..])
}

/// How a reply's body ended when reading it decoded gave `e`: QNetworkReply finishes a
/// compressed body that stops short with what it decoded, reports one that is not in its
/// encoding as an error QQmlXMLHttpRequest takes as a reply, and fails when the bytes did
/// not arrive (`None`). `whole` tells that the decoder had all of the body by then: what
/// it reports after that is the body stopping short, whatever it calls it.
fn ending(e: &std::io::Error, arrived: bool, whole: bool) -> Option<Event> {
    // ureq wraps what its gzip decoder reports.
    let kind = match e.get_ref().and_then(|e| e.downcast_ref::<ureq::Error>()) {
        Some(ureq::Error::Decompress(_, e)) => e.kind(),
        Some(_) => return None,
        None => e.kind(),
    };
    arrived.then_some(if whole || kind == std::io::ErrorKind::UnexpectedEof {
        Event::Finished
    } else {
        Event::Undecodable
    })
}

/// QDecompressHelper::isPotentialArchiveBomb: a body that has decoded to `unpacked` bytes
/// from the `received` ones that came is taken for one made to exhaust memory once it is
/// over 10 MiB and over 40 times (`gzip`, `deflate`) or 100 times (`br`, `zstd`) what came.
/// QNetworkReply ends such a reply as one it cannot decode.
fn unpacks_too_far(encoding: Encoding, unpacked: u64, received: u64) -> bool {
    let most = match encoding {
        Encoding::Gzip | Encoding::Deflate => 40.0,
        Encoding::Brotli | Encoding::Zstd => 100.0,
    };
    unpacked > 10 << 20 && received > 0 && unpacked as f64 / received as f64 > most
}

/// Whether a body that ends with `end` keeps what its last part decoded to: one that only
/// stops short does.
fn keeps_last_part(end: Option<&Event>) -> bool {
    matches!(end, Some(Event::Finished))
}

/// Makes the request and reports each reply's head and body as they arrive. The event the
/// request ends with; `None` when a reply did not come or did not end as announced, or
/// `emit` no longer takes events.
fn exchange(mut req: Outgoing, emit: &dyn Fn(Event) -> bool) -> Option<Event> {
    // A server that stops sending fails the request after http::STALL (QNetworkRequest's
    // default transfer timeout is off, which left a hung request and its thread behind forever).
    let o = crate::http::Opts {
        stall: true,
        ..Default::default()
    };
    let mut buf = vec![0u8; 64 << 10];
    let mut total = 0u64;
    for hop in 1.. {
        // The login written in the address is not sent as a part of it.
        let (url, user, password) = crate::http::login::split(&req.url);
        // More requests than the host's connection has channels wait here for one.
        let place = req.place.take();
        let mut seat =
            crate::http::login::Seat::new(&req.logins, place, req.sync, &url, &user, &password)?;
        let mut tries = 0;
        // A 401 the login can answer is not the page's: the request goes out again with
        // an `Authorization`.
        let mut res = loop {
            let mut headers = req.headers.clone();
            let login = seat.headers(&req.method, &mut headers);
            let body = req.body.as_deref();
            let res = crate::http::send_with_login(
                &req.method,
                &url,
                &headers,
                login.as_deref(),
                body,
                o,
            )
            .ok()?;
            tries += 1;
            if !seat.answers(res.status, &res.headers, &crate::http::heard()) || tries > MAX_ANSWERS
            {
                break res;
            }
        };
        let next =
            redirect_target(res.status, &res.headers, &req.url).filter(|_| hop < MAX_REDIRECTS);
        // ureq decompressed what is exactly `gzip`, and http.rs took its length away.
        let encoding = content_encoding(&res.headers).map(|(e, name)| (e, name == "gzip"));
        if encoding.is_some() {
            res.headers
                .retain(|(k, _)| !k.eq_ignore_ascii_case("content-length"));
        }
        emit(Event::Reply {
            status: res.status,
            reason: res.reason,
            headers: res.headers,
        })
        .then_some(())?;
        let broke = Rc::new(Cell::new(false));
        let ended = Rc::new(Cell::new(false));
        let wire = Wire {
            body: res.body,
            broke: broke.clone(),
            ended: ended.clone(),
        };
        let mut body: Box<dyn Read> = match encoding {
            Some((encoding, false)) => match decoded(wire, encoding) {
                Ok(body) => body,
                Err(undecodable) => return undecodable.then_some(Event::Undecodable),
            },
            _ => Box::new(wire),
        };
        // What the part of the body that came last has decoded to. QNetworkReply hands out
        // what a part decoded to once the whole part decoded, and nothing of a part it gave up
        // on, so this goes on when the next part comes or the body ends.
        let mut held: Vec<u8> = vec![];
        let mut came = crate::http::body_received();
        let mut unpacked = 0u64;
        loop {
            let read = body.read(&mut buf).map_err(|e| match encoding {
                Some((_, true)) => ending(&e, !crate::http::peer_closed(), false),
                // brotli-decompressor calls a stream that stops short invalid.
                Some((Encoding::Brotli, _)) => ending(&e, !broke.get(), ended.get()),
                Some(_) => ending(&e, !broke.get(), false),
                None => None,
            });
            let now = crate::http::body_received();
            let hand_out = match &read {
                Ok(n) => *n == 0 || now != came,
                Err(end) => keeps_last_part(end.as_ref()),
            };
            if hand_out && !held.is_empty() {
                emit(Event::Data(std::mem::take(&mut held))).then_some(())?;
            }
            came = now;
            let n = match read {
                Ok(0) => break,
                Ok(n) => n,
                Err(end) => return end,
            };
            total += n as u64;
            if total > crate::http::XHR_MAX {
                return None;
            }
            let Some((encoding, _)) = encoding else {
                emit(Event::Data(buf[..n].to_vec())).then_some(())?;
                continue;
            };
            unpacked += n as u64;
            if unpacks_too_far(encoding, unpacked, now) {
                return Some(Event::Undecodable);
            }
            held.extend_from_slice(&buf[..n]);
        }
        seat.ended();
        drop(seat);
        let Some(next) = next else { break };
        // RFC 9110 15.4.4: what a 303 points to is fetched with GET.
        if res.status == 303 && req.method != "GET" {
            req.method = "GET".into();
            req.body = None;
            req.headers.clone_from(&req.headers_of_get);
        }
        req.url.clone_from(&next);
        emit(Event::Redirected(next)).then_some(())?;
    }
    Some(Event::Finished)
}

/// The whole of `req`: `emit` gets its events and answers whether more are wanted.
fn job(
    req: Outgoing,
    stop: std::sync::Arc<crate::svc::Stop>,
    emit: impl Fn(Event) -> bool + Send + 'static,
) -> impl FnOnce() + Send + 'static {
    move || {
        let end = crate::http::with_stop(&stop, || exchange(req, &emit));
        emit(end.unwrap_or(Event::Failed));
    }
}

/// Starts `req` on a pool thread. When the system gives the pool no thread and none runs
/// that could take the request, it fails as one that reached no server: `emit` gets
/// [`Event::Failed`] here, for the next turn of the loop. It neither waits for a thread nor
/// goes out from this one. Qt 6.12 leaves such a request opened for good
/// (docs/qt-differences.md).
pub(crate) fn run(
    req: Outgoing,
    stop: std::sync::Arc<crate::svc::Stop>,
    emit: impl Fn(Event) -> bool + Send + 'static,
) {
    // The job takes `emit` when it starts; one that never starts leaves it here.
    let emit = std::sync::Arc::new(std::sync::Mutex::new(Some(emit)));
    let taken = emit.clone();
    let started = workpool::try_long(move || {
        let emit = taken.lock().unwrap().take();
        if let Some(emit) = emit {
            job(req, stop, emit)();
        }
    });
    if let Err(e) = started {
        elog::eprintln!("XMLHttpRequest: cannot start the thread for a request: {e}");
        let emit = emit.lock().unwrap().take();
        if let Some(emit) = emit {
            emit(Event::Failed);
        }
    }
}

/// QQmlXMLHttpRequestWrapper of `this`; anything else is Qt's "Not an XMLHttpRequest object".
fn request(it: &Interp, this: &Value) -> R<Rc<XhrRequest>> {
    match xhr_request(this) {
        Some(r) => Ok(r),
        None => it.throw("ReferenceError", "Not an XMLHttpRequest object"),
    }
}

/// The getters of the prototype.
const GETTERS: [(&str, NativeFn); 8] = [
    ("readyState", |it, this, _| {
        Ok(Value::Num(request(it, this)?.state.get().into()))
    }),
    ("status", |it, this, _| match request(it, this)? {
        r if !r.has_reply() => invalid_state(it),
        r if !r.failed.get() => Ok(Value::Num(r.status.get().into())),
        _ => Ok(Value::Num(0.0)),
    }),
    ("statusText", |it, this, _| match request(it, this)? {
        r if !r.has_reply() => invalid_state(it),
        r if !r.failed.get() => Ok(Value::str(&r.reason.borrow())),
        _ => Ok(Value::str("")),
    }),
    ("responseText", |it, this, _| {
        let r = request(it, this)?;
        Ok(if r.has_body() {
            r.text_value()
        } else {
            Value::str("")
        })
    }),
    ("responseXML", |it, this, _| {
        let r = request(it, this)?;
        if !r.got_xml.get() || !r.has_body() {
            return Ok(Value::Null);
        }
        if r.response_type.borrow().is_empty() {
            *r.response_type.borrow_mut() = "document".into();
        }
        r.parsed(it, false)
    }),
    ("response", |it, this, _| {
        let r = request(it, this)?;
        if !r.has_body() {
            Ok(Value::str(""))
        } else if r.wants("text") || r.wants("") {
            Ok(r.text_value())
        } else if r.wants("arraybuffer") {
            // QQmlXMLHttpRequest::response hands on m_responseEntityBody, which is null
            // until a part of the body arrived: V4 makes a buffer without data of it.
            let bytes = r.body.borrow().clone();
            Ok(Value::Obj(if bytes.is_empty() {
                crate::typedarray::detached_buffer(it)
            } else {
                crate::typedarray::new_buffer(it, bytes).0
            }))
        } else if r.wants("json") {
            r.parsed(it, true)
        } else if r.wants("document") {
            r.parsed(it, false)
        } else {
            Ok(Value::str(""))
        }
    }),
    ("responseURL", |it, this, _| {
        let r = request(it, this)?;
        Ok(Value::str(&if r.has_body() {
            let asked = r.asked.borrow();
            crate::qurl::QUrl::parse(asked.split('#').next().unwrap_or_default()).to_string()
        } else {
            String::new()
        }))
    }),
    ("responseType", |it, this, _| {
        Ok(Value::str(&request(it, this)?.response_type.borrow()))
    }),
];

/// The methods of the prototype.
const METHODS_OF: [(&str, NativeFn); 7] = [
    ("open", |it, this, a| {
        let r = request(it, this)?;
        if !(2..=5).contains(&a.len()) {
            return argument_count(it);
        }
        let method = a[0].to_js_string().to_uppercase();
        if !METHODS.contains(&method.as_str()) {
            return dom_error(it, 12, "Unsupported HTTP method type");
        }
        if r.sent.get() {
            stop(it, this);
        }
        r.serial.set(r.serial.get() + 1);
        r.sent.set(false);
        r.failed.set(false);
        r.body.borrow_mut().clear();
        r.said_unread.set(false);
        *r.method.borrow_mut() = method;
        let url = a[1].to_js_string();
        let url = url.split('#').next().unwrap_or_default();
        *r.url.borrow_mut() = url.into();
        *r.asked.borrow_mut() = with_login(
            url,
            a.get(3).map(Value::to_js_string),
            a.get(4).map(Value::to_js_string),
        );
        r.added.borrow_mut().clear();
        r.sync.set(a.len() > 2 && !a[2].truthy());
        r.state.set(OPENED);
        dispatch(it, this, &r);
        Ok(Value::Undefined)
    }),
    ("setRequestHeader", |it, this, a| {
        let r = request(it, this)?;
        if a.len() != 2 {
            return argument_count(it);
        }
        if r.state.get() != OPENED || r.sent.get() {
            return invalid_state(it);
        }
        let (k, v) = (a[0].to_js_string(), a[1].to_js_string());
        if forbidden_header(&k) {
            return Ok(Value::Undefined);
        }
        let v = v.trim();
        let mut h = r.headers.borrow_mut();
        let mut added = r.added.borrow_mut();
        let again = added.iter().any(|n| n.eq_ignore_ascii_case(&k));
        match h.iter_mut().find(|e| e.0.eq_ignore_ascii_case(&k)) {
            Some(e) if again => {
                e.1.push(',');
                e.1.push_str(v);
            }
            Some(e) => e.1 = v.into(),
            None => h.push((k.clone(), v.into())),
        }
        if !again {
            added.push(k);
        }
        Ok(Value::Undefined)
    }),
    ("send", |it, this, a| {
        let r = request(it, this)?;
        if r.state.get() != OPENED || r.sent.get() {
            return invalid_state(it);
        }
        r.failed.set(false);
        r.sent.set(true);
        r.mute.set(false);
        *r.data.borrow_mut() = match a.first() {
            None => vec![],
            Some(Value::Obj(o)) => match &o.borrow().internal {
                crate::value::Internal::ArrayBuffer(b) => b.borrow().clone(),
                crate::value::Internal::DetachedArrayBuffer => vec![],
                _ => a[0].to_js_string().into_bytes(),
            },
            Some(v) => v.to_js_string().into_bytes(),
        };
        if r.sync.get() {
            // Qt runs the handlers inside `send` and returns in DONE.
            let serial = r.serial.get();
            if crate::native::sandboxed() {
                // No network there: the request fails the same way in every run.
                deliver(it, this, serial, Event::Failed);
                return Ok(Value::Undefined);
            }
            let (tx, rx) = std::sync::mpsc::channel();
            let stop = crate::runtime::StopOnDrop::default();
            // A thread of its own: `send` waits for it here, so the request must
            // neither queue behind the pool's jobs nor, when the system refuses the
            // thread, run on this one.
            let started = std::thread::Builder::new()
                .name("ri-xhr-sync".into())
                .spawn(job(r.outgoing(), stop.0.clone(), move |e| {
                    tx.send(e).is_ok()
                }));
            if let Err(e) = started {
                elog::eprintln!(
                    "XMLHttpRequest: cannot start the thread for a synchronous request: {e}"
                );
                deliver(it, this, serial, Event::Failed);
                return Ok(Value::Undefined);
            }
            while let Ok(e) = rx.recv() {
                if deliver(it, this, serial, e) {
                    break;
                }
            }
        } else if let Some(w) = crate::runtime::world_of(it) {
            w.borrow_mut()
                .effects
                .push(crate::svc::Effect::Xhr(vec![this.clone()]));
        }
        Ok(Value::Undefined)
    }),
    ("abort", |it, this, _| {
        let r = request(it, this)?;
        stop(it, this);
        r.serial.set(r.serial.get() + 1);
        r.body.borrow_mut().clear();
        r.failed.set(true);
        let idle = match r.state.get() {
            UNSENT | DONE => true,
            OPENED => !r.sent.get(),
            _ => false,
        };
        if !idle {
            r.state.set(DONE);
            r.sent.set(false);
            dispatch(it, this, &r);
        }
        r.state.set(UNSENT);
        Ok(Value::Undefined)
    }),
    ("getResponseHeader", |it, this, a| {
        let r = request(it, this)?;
        if a.len() != 1 {
            return argument_count(it);
        }
        if !r.has_reply() {
            return invalid_state(it);
        }
        let name = arg(a, 0).to_js_string().to_lowercase();
        let h = r.response_headers.borrow();
        Ok(Value::str(
            h.iter().find(|e| e.0 == name).map_or("", |e| &e.1),
        ))
    }),
    ("getAllResponseHeaders", |it, this, a| {
        let r = request(it, this)?;
        if !a.is_empty() {
            return argument_count(it);
        }
        if !r.has_reply() {
            return invalid_state(it);
        }
        let h = r.response_headers.borrow();
        let lines: Vec<String> = h.iter().map(|(k, v)| format!("{k}: {v}")).collect();
        Ok(Value::str(&lines.join("\r\n")))
    }),
    ("overrideMimeType", |it, this, a| {
        let r = request(it, this)?;
        if a.len() != 1 {
            return argument_count(it);
        }
        if r.has_body() {
            return invalid_state(it);
        }
        let given = a[0].to_js_string();
        // What is not `type/subtype` in ASCII letters, digits and `-` reads as bytes.
        let kind = given.split(';').next().unwrap_or_default().trim();
        let parsable = kind.matches('/').count() == 1
            && kind
                .chars()
                .all(|c| c == '-' || c == '/' || c.is_ascii_alphanumeric());
        *r.override_mime.borrow_mut() = if parsable {
            kind.into()
        } else {
            "application/octet-stream".into()
        };
        for part in given.split(';') {
            if let Some(charset) = part.trim().strip_prefix("charset=") {
                *r.override_charset.borrow_mut() = charset.trim().into();
            }
        }
        Ok(Value::Undefined)
    }),
];

const STATES: [(&str, u8); 5] = [
    ("UNSENT", UNSENT),
    ("OPENED", OPENED),
    ("HEADERS_RECEIVED", HEADERS_RECEIVED),
    ("LOADING", LOADING),
    ("DONE", DONE),
];

/// The state constants on `o`: read-only and not listed among its keys.
fn states(o: &crate::value::Obj) {
    for (k, v) in STATES {
        set_hidden(o, k, Value::Num(v.into()));
        o.borrow_mut()
            .set_attr(k, crate::value::READONLY | crate::value::PERMANENT);
    }
}

/// XMLHttpRequest.prototype: the methods, state constants and properties every request
/// shares. None of them is listed among a request's keys.
pub(crate) fn proto() -> crate::value::Obj {
    let proto = new_obj();
    for (k, f) in GETTERS {
        define_accessor(&proto, k, true, fn_value(Callable::Native(k, f)), false);
    }
    let set_type: NativeFn = |it, this, a| {
        let r = request(it, this)?;
        let Some(to) = a.first() else {
            return argument_count(it);
        };
        *r.response_type.borrow_mut() = to.to_js_string();
        Ok(Value::Undefined)
    };
    define_accessor(
        &proto,
        "responseType",
        false,
        fn_value(Callable::Native("responseType", set_type)),
        false,
    );
    for (k, f) in METHODS_OF {
        set_hidden(&proto, k, fn_value(Callable::Native(k, f)));
    }
    states(&proto);
    // What the requests of this engine remember of logins: each request gets it from here.
    proto.borrow_mut().native = Some(Rc::new(crate::http::login::Shared::default()));
    proto
}

/// `XMLHttpRequest`: the constructor of requests with prototype `proto`.
pub(crate) fn ctor(proto: &crate::value::Obj) -> Value {
    let f = fn_value(Callable::Native("XMLHttpRequest", |it, _, _| {
        // Called as a function it makes nothing.
        if !std::mem::take(&mut it.constructing) {
            return Ok(Value::Undefined);
        }
        let o = new_obj();
        {
            let mut b = o.borrow_mut();
            b.proto = Some(it.vt_protos.xhr.clone());
            let logins = it.vt_protos.xhr.borrow().native.clone();
            b.native = Some(Rc::new(XhrRequest {
                logins: logins
                    .and_then(|l| l.downcast::<crate::http::login::Shared>().ok())
                    .map(|l| (*l).clone())
                    .unwrap_or_default(),
                ..Default::default()
            }));
        }
        Ok(Value::Obj(o))
    }));
    let Value::Obj(c) = &f else {
        return f;
    };
    states(c);
    // Qt's prototype has no `constructor`.
    set_hidden(c, "prototype", Value::Obj(proto.clone()));
    let mut b = c.borrow_mut();
    b.builtin_ctor = true;
    b.native = Some(Rc::new(crate::xhr_dom::prototypes()));
    // V4's `XMLHttpRequest` has a `prototype` and no `length` of its own.
    b.set_attr(
        "prototype",
        crate::value::READONLY | crate::value::PERMANENT,
    );
    b.set_attr("length", crate::value::ABSENT);
    drop(b);
    f
}

fn arg(a: &[Value], i: usize) -> Value {
    a.get(i).cloned().unwrap_or(Value::Undefined)
}

/// Stops the request of `this` in flight, if any (QQmlXMLHttpRequest::destroyNetwork).
fn stop(it: &mut Interp, this: &Value) {
    if let Some(w) = crate::runtime::world_of(it) {
        w.borrow_mut()
            .effects
            .push(crate::svc::Effect::XhrAbort(vec![this.clone()]));
    }
}

#[cfg(test)]
mod tests {
    /// A reply that would not end is given up on once it has brought more than a request
    /// may hold, sent as it is or compressed 30 times, which is less than QNetworkReply
    /// takes for an attack: the request fails, and what it handed on stays within the bound.
    #[test]
    fn a_body_over_the_bound_fails_the_request() {
        use std::io::{Read, Write};
        const MOST: usize = crate::http::XHR_MAX as usize;
        let mut seed = 0x9E37_79B9_7F4A_7C15_u64;
        let mut plain: Vec<u8> = (0..(MOST + (4 << 20)) / 30)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                (seed >> 32) as u8
            })
            .collect();
        plain.resize(MOST + (4 << 20), 0);
        let mut gzip = flate2::write::GzEncoder::new(vec![], flate2::Compression::fast());
        gzip.write_all(&plain).unwrap();
        let gzip = gzip.finish().unwrap();
        assert!(plain.len() / gzip.len() < 40, "{}", gzip.len());
        for (encoding, body) in [("identity", plain), ("gzip", gzip)] {
            let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}/", server.local_addr().unwrap());
            let serving = std::thread::spawn(move || {
                let (mut s, _) = server.accept().unwrap();
                let mut got = [0u8; 4096];
                let _ = s.read(&mut got);
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Encoding: {encoding}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                // The reader hangs up before the end.
                let _ = s
                    .write_all(head.as_bytes())
                    .and_then(|()| s.write_all(&body));
            });
            let handed_on = std::cell::Cell::new(0usize);
            let req = super::Outgoing {
                method: "GET".into(),
                url,
                headers: vec![],
                body: None,
                headers_of_get: vec![],
                sync: false,
                logins: Default::default(),
                place: None,
            };
            let end = super::exchange(req, &|e| {
                if let super::Event::Data(d) = e {
                    handed_on.set(handed_on.get() + d.len());
                }
                true
            });
            assert!(end.is_none(), "{encoding}");
            assert!(handed_on.get() <= MOST, "{encoding}: {}", handed_on.get());
            // A compressed body is handed on as its parts come, so the last part is not.
            if encoding == "identity" {
                assert!(handed_on.get() > MOST / 2, "{}", handed_on.get());
            }
            serving.join().unwrap();
        }
    }

    /// A synchronous request whose thread the system refuses fails at once: `send` returns
    /// in DONE with status 0 after the handlers ran, the refusal is in the log, and nothing
    /// was sent from this thread (the server saw no connection). In a process of its own: the
    /// test reads what that process wrote to its stderr.
    #[test]
    fn a_refused_thread_fails_the_synchronous_request() {
        const TEST: &str = concat!(
            module_path!(),
            "::a_refused_thread_fails_the_synchronous_request"
        );
        if !crate::testenv::in_own_process(TEST) {
            crate::testenv::run_in_own_process(TEST, [("LC_ALL", "C")]);
            return;
        }
        let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        server.set_nonblocking(true).unwrap();
        let mut rt = crate::runtime::Runtime::new(None);
        let r = rt
            .load_str(&format!(
                "import QtQuick\nItem {{ id: r\n\
                 property var log: []\n\
                 function go() {{ let x = new XMLHttpRequest()\n\
                   x.onreadystatechange = () => {{ r.log = [...r.log, 'state:' + x.readyState] }}\n\
                   x.onerror = () => {{ r.log = [...r.log, 'error'] }}\n\
                   x.onloadend = () => {{ r.log = [...r.log, 'loadend'] }}\n\
                   x.open('GET', 'http://{}/', false); x.send()\n\
                   r.log = [...r.log, 'returned:' + x.readyState + ':' + x.status + ':' + x.statusText + ':' + x.responseText] }} }}",
                server.local_addr().unwrap()
            ))
            .unwrap();
        rt.settle();
        crate::testenv::refuse_new_threads();
        let text = crate::testenv::stderr_of(|| {
            let called = rt.call_method(r, "go", vec![]);
            assert!(matches!(called, Some(Ok(_))), "{called:?}");
        });
        assert_eq!(
            rt.get(r, "log").to_js_string(),
            "state:1,state:4,error,loadend,returned:4:0::"
        );
        let line = "XMLHttpRequest: cannot start the thread for a synchronous request:";
        assert_eq!(text.matches(line).count(), 1, "stderr: {text:?}");
        let e = server.accept().unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::WouldBlock);
        assert_eq!(rt.errors(), Vec::<String>::new());
    }

    /// zstd frames holding "hello" in the formats 0.5 and 0.7.
    const OLD_FRAMES: [(&str, &[u8]); 2] = [
        ("0.5", b"\x25\xb5\x2f\xfd\x00\x40\x00\x05hello\xc0\x00\x00"),
        (
            "0.7",
            b"\x27\xb5\x2f\xfd\x20\x05\x40\x00\x05hello\xc0\x00\x00",
        ),
    ];

    /// A body that comes in these pieces, one at each read.
    struct Pieces(std::collections::VecDeque<Vec<u8>>);

    impl std::io::Read for Pieces {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let Some(piece) = self.0.pop_front() else {
                return Ok(0);
            };
            buf[..piece.len()].copy_from_slice(&piece);
            Ok(piece.len())
        }
    }

    /// What a zstd body that comes as `first` bytes and then the rest decodes to.
    fn unzstd(body: &[u8], first: usize) -> Option<Vec<u8>> {
        use std::io::Read;
        let (a, b) = body.split_at(first);
        let pieces = [a, b].into_iter().filter(|p| !p.is_empty());
        let wire = super::Wire {
            body: Box::new(Pieces(pieces.map(<[u8]>::to_vec).collect())),
            broke: Default::default(),
            ended: Default::default(),
        };
        let mut out = vec![];
        let mut body = super::decoded(wire, super::Encoding::Zstd).ok()?;
        body.read_to_end(&mut out).ok()?;
        Some(out)
    }

    /// A frame of an old format is read when the piece that starts it has five bytes of it
    /// and is not when it has fewer: the body goes to libzstd in the pieces it came in.
    ///
    /// Qt 6.12.0 does the same with a body cut by the network. Measured on 35c8f11b with
    /// `xhr_equiv::responses_match_qt`, whose server then wrote the first byte of the body,
    /// waited 200 ms and wrote the rest: for the frames 0.5 and 0.7 Qt gave the states
    /// 1, 3, 4, `status` 200, `statusText` "OK", no text, no headers and no `onerror`, which
    /// is what it gives for a body it cannot decode, and so did this (208 cases, none
    /// differed); in one piece both gave "hello". A test waits on no clock, so the cut is
    /// made here, under the decoder.
    #[test]
    fn an_old_zstd_frame_is_read_when_its_first_piece_has_five_bytes() {
        for (format, frame) in OLD_FRAMES {
            assert_eq!(unzstd(frame, 0).as_deref(), Some(&b"hello"[..]), "{format}");
            assert_eq!(unzstd(frame, 5).as_deref(), Some(&b"hello"[..]), "{format}");
            assert_eq!(unzstd(frame, 1), None, "{format}");
            assert_eq!(unzstd(frame, 4), None, "{format}");
        }
        // A frame of the format in use is read however it is cut.
        let now = b"\x28\xb5\x2f\xfd\x00\x58\x29\x00\x00first";
        for first in 0..now.len() {
            assert_eq!(
                unzstd(now, first).as_deref(),
                Some(&b"first"[..]),
                "{first}"
            );
        }
    }

    /// Two chunks of the transfer coding that came together are one read of the body: ureq
    /// hands on what it has taken out of the chunks, not a chunk at a time. So a frame of an
    /// old format whose first chunk is one byte is read here (QNetworkReply decodes a chunk
    /// at a time and gives that reply up).
    #[test]
    fn chunks_that_came_together_are_read_as_one_piece() {
        use std::io::{Read, Write};
        let (_, frame) = OLD_FRAMES[0];
        let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", server.local_addr().unwrap());
        let serving = std::thread::spawn(move || {
            let (mut s, _) = server.accept().unwrap();
            let mut got = [0u8; 4096];
            let _ = s.read(&mut got);
            let mut all =
                b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n1\r\n"
                    .to_vec();
            all.extend_from_slice(&frame[..1]);
            all.extend_from_slice(format!("\r\n{:x}\r\n", frame.len() - 1).as_bytes());
            all.extend_from_slice(&frame[1..]);
            all.extend_from_slice(b"\r\n0\r\n\r\n");
            s.write_all(&all).unwrap();
        });
        let o = crate::http::Opts {
            stall: true,
            ..Default::default()
        };
        let Ok(mut res) = crate::http::send_as_qt("GET", &url, &[], None, o) else {
            panic!("no reply");
        };
        let mut buf = vec![0u8; 64 << 10];
        let mut reads = vec![];
        loop {
            let n = res.body.read(&mut buf).unwrap();
            if n == 0 {
                break;
            }
            reads.push(n);
        }
        println!("reads of the body: {reads:?}");
        assert_eq!(reads, [frame.len()]);
        serving.join().unwrap();
    }
}
