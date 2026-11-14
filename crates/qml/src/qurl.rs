//! A url as Qt 6.12's QUrl parses, resolves and prints it (qurl.cpp, qurlrecode.cpp and
//! qdir.cpp's qt_normalizePathSegments): what `Qt.resolvedUrl` answers and the file a `url`
//! property names. A host or a port QUrl calls invalid makes the url print as nothing; its
//! other validity errors are not modelled, such a url prints as parsed.
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QUrl {
    scheme: Option<String>,
    /// User info, host and port as `toString()` prints them.
    authority: Option<String>,
    host: String,
    /// The host or the port did not parse (QUrlPrivate::setError).
    invalid: bool,
    /// Path, query and fragment are held as QUrl holds them (PrettyDecoded).
    path: String,
    query: Option<String>,
    fragment: Option<String>,
}

#[derive(Clone, Copy, PartialEq)]
enum Act {
    Decode,
    Leave,
    Encode,
}

/// What recoding does with a character in one part of a url: `defaultActionTable` with the
/// part's changes (`pathInIsolation`, `pathInUrl` and the like).
#[derive(Clone, Copy)]
struct Part {
    decode: &'static str,
    encode: &'static str,
}

const PATH_IN: Part = Part {
    decode: "?#\"<>^\\|{}",
    encode: "",
};
/// qt_encodeFromUser with `pathInIsolation`: a path that is given decoded.
const PATH_FROM_USER: Part = Part {
    decode: "?#\"<>^\\|{}",
    encode: "[]",
};
const QUERY_IN: Part = Part {
    decode: "#\"<>^\\|{}",
    encode: "",
};
const FRAGMENT_IN: Part = Part {
    decode: "\"<>^\\|{}",
    encode: "",
};
const PATH_OUT: Part = Part {
    decode: "",
    encode: "?#",
};
const QUERY_OUT: Part = Part {
    decode: "",
    encode: "#",
};
const FRAGMENT_OUT: Part = Part {
    decode: "",
    encode: "",
};

impl Part {
    /// For `c` in 0x20..0x80.
    fn act(self, c: u8) -> Act {
        let ch = c as char;
        if self.decode.contains(ch) {
            Act::Decode
        } else if self.encode.contains(ch) {
            Act::Encode
        } else {
            match c {
                b' ' | b'-' | b'.' | b'_' | b'~' => Act::Decode,
                c if c.is_ascii_alphanumeric() => Act::Decode,
                b'"' | b'%' | b'<' | b'>' | b'\\' | b'^' | b'`' | b'{' | b'|' | b'}' | 0x7f => {
                    Act::Encode
                }
                _ => Act::Leave,
            }
        }
    }
}

fn push_hex(out: &mut Vec<u8>, c: u8) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    out.extend([b'%', HEX[usize::from(c >> 4)], HEX[usize::from(c & 15)]]);
}

/// The byte `%XX` at `i` stands for; `None` when two hex digits do not follow the `%`.
fn escape_at(b: &[u8], i: usize) -> Option<u8> {
    let hex = |c: u8| (c as char).to_digit(16);
    match (b.get(i), b.get(i + 1), b.get(i + 2)) {
        (Some(b'%'), Some(&h), Some(&l)) => Some((hex(h)? * 16 + hex(l)?) as u8),
        _ => None,
    }
}

/// The character the escapes at `i` spell in UTF-8 and how many bytes it takes
/// (encodedUtf8ToUtf16 with QUrlUtf8Traits: no noncharacters).
fn escaped_char(b: &[u8], i: usize) -> Option<(char, usize)> {
    let len = match escape_at(b, i)? {
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        _ => return None,
    };
    let mut bytes = [0u8; 4];
    for (k, byte) in bytes.iter_mut().enumerate().take(len) {
        *byte = escape_at(b, i + 3 * k)?;
    }
    let c = std::str::from_utf8(&bytes[..len]).ok()?.chars().next()?;
    let u = c as u32;
    let nonchar = (0xfdd0..=0xfdef).contains(&u) || u & 0xfffe == 0xfffe;
    (!nonchar).then_some((c, len))
}

/// qurlrecode.cpp `recode`; `None` when an escape is malformed and `literal` is off.
fn recode_pass(s: &str, part: Part, literal: bool) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c >= 0x80 {
            out.push(c);
        } else if c != b'%' {
            if c < 0x20 || part.act(c) == Act::Encode {
                push_hex(&mut out, c);
            } else {
                out.push(c);
            }
        } else if literal {
            out.extend(b"%25");
        } else {
            let d = escape_at(b, i)?;
            if d >= 0x80
                && let Some((ch, len)) = escaped_char(b, i)
            {
                out.extend(ch.encode_utf8(&mut [0; 4]).bytes());
                i += 3 * len;
                continue;
            }
            if (0x20..0x80).contains(&d) && part.act(d) == Act::Decode {
                out.push(d);
            } else {
                push_hex(&mut out, d);
            }
            i += 2;
        }
        i += 1;
    }
    String::from_utf8(out).ok()
}

/// qt_urlRecode: unreserved characters, spaces and UTF-8 are decoded, delimiters stay as
/// written, the rest is encoded; one malformed escape makes every `%` of `s` a literal one.
fn recode(s: &str, part: Part) -> String {
    recode_pass(s, part, false)
        .or_else(|| recode_pass(s, part, true))
        .unwrap_or_default()
}

/// qt_normalizePathSegments in url mode: `.` and `..` segments resolved (RFC 3986 5.2.4);
/// a local path also has runs of slashes made one and keeps a `..` that climbs past the root.
fn normalize_path(path: &str, remote: bool) -> String {
    let p: Vec<char> = path.chars().collect();
    let n = p.len();
    let prefix = usize::from(p.first() == Some(&'/'));
    let mut start = prefix;
    let mut out: Vec<char> = Vec::with_capacity(n);
    let mut i = 0;
    while i < n {
        if p[i] == '/' {
            out.push('/');
            i += 1;
            while i < n && p[i] == '/' {
                if remote {
                    out.push('/');
                }
                i += 1;
            }
        }
        let ends = |k: usize| k == n || p[k] == '/';
        let dots = match (p.get(i), p.get(i + 1)) {
            (Some('.'), _) if ends(i + 1) => 1,
            (Some('.'), Some('.')) if ends(i + 2) => 2,
            _ => 0,
        };
        if dots == 0 {
            while i < n && p[i] != '/' {
                out.push(p[i]);
                i += 1;
            }
            continue;
        }
        if dots == 2 {
            if out.len() == start && !remote {
                out.extend(['.', '.']);
                if i + 2 != n {
                    out.push('/');
                    i += 1;
                }
                start = out.len();
                i += 2;
                continue;
            }
            if out.len() > start {
                out.pop();
            }
            while out.len() > start && out.last() != Some(&'/') {
                out.pop();
            }
        }
        i += dots;
        if i != n && out.len() > start && out.last() == Some(&'/') {
            out.pop();
        }
        if out.len() == start {
            if i != n && p[i] == '/' {
                i += 1;
            }
            while prefix == 0 && i != n && p[i] == '/' {
                i += 1;
            }
        }
    }
    if !remote && out.is_empty() && n > 0 {
        return ".".into();
    }
    out.into_iter().collect()
}

#[derive(Default)]
struct Authority {
    printed: Option<String>,
    host: String,
    invalid: bool,
}

impl Authority {
    /// QUrlPrivate::setAuthority (TolerantMode) and what appendAuthority prints of it.
    fn parse(a: &str) -> Authority {
        let with = |decode, encode| move |s: &str| recode(s, Part { decode, encode });
        let (user_info, rest) = match a.find('@') {
            Some(i) => (Some(&a[..i]), &a[i + 1..]),
            None => (None, a),
        };
        let mut printed = String::new();
        if let Some(info) = user_info {
            let (user, password) = match info.find(':') {
                Some(i) => (&info[..i], Some(&info[i + 1..])),
                None => (info, None),
            };
            // userNameInIsolation, then userNameInUrl.
            let user = with(":@][/?#\"<>^\\|{}", "")(user);
            printed.push_str(&with("", ":@][/?#")(&user));
            if let Some(password) = password {
                let password = with("@][/?#\"<>^\\|{}", "")(password);
                printed.push(':');
                printed.push_str(&with("", "@][/?#")(&password));
            }
            printed.push('@');
        }
        let colon = rest
            .rfind(':')
            .filter(|&c| !rest.starts_with('[') || rest.find(']').is_some_and(|b| b < c));
        let (name, port) = match colon {
            Some(c) => (&rest[..c], Some(&rest[c + 1..])),
            None => (rest, None),
        };
        let mut invalid = false;
        let port = port.filter(|p| !p.is_empty()).and_then(|p| {
            let n = p.bytes().try_fold(0u32, |n, c| {
                let n = n * 10 + u32::from(c.checked_sub(b'0').filter(|d| *d < 10)?);
                (n <= 0xffff).then_some(n)
            });
            invalid |= n.is_none();
            n
        });
        let host = crate::qurlhost::host(name, true, &|s, decode| with(decode, "")(s));
        invalid |= host.is_err();
        let host = host.unwrap_or_else(|left| left);
        printed.push_str(&host);
        if let Some(port) = port {
            printed.push_str(&format!(":{port}"));
        }
        Authority {
            printed: Some(printed),
            host,
            invalid,
        }
    }
}

impl QUrl {
    /// `QUrl(url)` (QUrlPrivate::parse, TolerantMode).
    pub fn parse(url: &str) -> QUrl {
        let hash = url.find('#');
        let head = &url[..hash.unwrap_or(url.len())];
        let question = head.find('?');
        let hier = &head[..question.unwrap_or(head.len())];
        let scheme = hier.find(':').map(|c| &hier[..c]).filter(|s| {
            s.starts_with(|c: char| c.is_ascii_alphabetic())
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
        });
        let rest = &hier[scheme.map_or(0, |s| s.len() + 1)..];
        let (authority, path) = match rest.strip_prefix("//") {
            Some(r) => {
                let (a, p) = r.split_at(r.find('/').unwrap_or(r.len()));
                (Some(Authority::parse(a)), p)
            }
            None => (None, rest),
        };
        let a = authority.unwrap_or_default();
        QUrl {
            scheme: scheme.map(str::to_ascii_lowercase),
            authority: a.printed,
            host: a.host,
            invalid: a.invalid,
            path: recode(path, PATH_IN),
            query: question.map(|q| recode(&head[q + 1..], QUERY_IN)),
            fragment: hash.map(|h| recode(&url[h + 1..], FRAGMENT_IN)),
        }
    }

    /// `QUrl::fromLocalFile(path)`.
    pub fn from_local_file(path: &Path) -> QUrl {
        let mut url = QUrl {
            scheme: Some("file".into()),
            ..QUrl::default()
        };
        url.set_path(&path.to_string_lossy());
        url
    }

    pub fn is_empty(&self) -> bool {
        *self == QUrl::default()
    }

    /// Whether the host or the port did not parse (`!QUrl::isValid()`).
    pub fn is_invalid(&self) -> bool {
        self.invalid
    }

    /// QUrl's `operator==` (`comparesEqual`): the parts held and which of them are present,
    /// whether the url is valid or not; a `file` url may leave its empty host out.
    pub fn equals(&self, other: &QUrl) -> bool {
        let authority = |u: &'_ QUrl| match &u.authority {
            None if u.is_local_file() => Some(String::new()),
            a => a.clone(),
        };
        self.scheme == other.scheme
            && authority(self) == authority(other)
            && self.host == other.host
            && self.path == other.path
            && self.query == other.query
            && self.fragment == other.fragment
    }

    pub fn is_relative(&self) -> bool {
        self.scheme.is_none()
    }

    pub fn is_local_file(&self) -> bool {
        self.scheme.as_deref() == Some("file")
    }

    /// QUrlPrivate::mergePaths (RFC 3986 5.2.3).
    fn merge(&self, rel: &str) -> String {
        if !self.host.is_empty() && self.path.is_empty() {
            return format!("/{rel}");
        }
        let dir = self.path.rfind('/').map_or("", |i| &self.path[..=i]);
        format!("{dir}{rel}")
    }

    /// `QUrl::resolved(rel)`: `rel` as a reference from this url (RFC 3986 5.2.2), its path
    /// normalized.
    pub fn resolved(&self, rel: &QUrl) -> QUrl {
        let mut t = if rel.scheme.is_some() {
            rel.clone()
        } else if rel.authority.is_some() {
            QUrl {
                scheme: self.scheme.clone(),
                ..rel.clone()
            }
        } else {
            let (path, query) = if rel.path.is_empty() {
                (
                    self.path.clone(),
                    rel.query.clone().or_else(|| self.query.clone()),
                )
            } else if rel.path.starts_with('/') {
                (rel.path.clone(), rel.query.clone())
            } else {
                (self.merge(&rel.path), rel.query.clone())
            };
            QUrl {
                scheme: self.scheme.clone(),
                authority: self.authority.clone(),
                host: self.host.clone(),
                invalid: false,
                path,
                query,
                fragment: None,
            }
        };
        t.fragment = rel.fragment.clone();
        t.path = normalize_path(&t.path, !t.is_local_file());
        if t.authority.is_none() && !t.is_local_file() {
            // fixupNonAuthorityPath: such a path cannot start with two slashes.
            let slashes = t.path.len() - t.path.trim_start_matches('/').len();
            t.path.drain(..slashes.saturating_sub(1));
        }
        t
    }

    /// `QUrl::path()`: the path with every escape decoded.
    pub fn path(&self) -> String {
        let b = self.path.as_bytes();
        let mut out = Vec::with_capacity(b.len());
        let mut i = 0;
        while i < b.len() {
            match escape_at(b, i) {
                // What is still escaped above ASCII is not UTF-8.
                Some(d) if d >= 0x80 => out.extend("\u{fffd}".bytes()),
                Some(d) => out.push(d),
                None => {
                    out.push(b[i]);
                    i += 1;
                    continue;
                }
            }
            i += 3;
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    /// `QUrl::setPath(path)` (DecodedMode): every `%` stands for itself, `[` and `]` are
    /// encoded.
    fn set_path(&mut self, path: &str) {
        self.path = recode_pass(path, PATH_FROM_USER, true).unwrap_or_default();
    }

    /// `QUrl::toLocalFile()`: the path with every escape decoded, behind `//host` when there
    /// is a host; query and fragment are not part of it.
    pub fn to_local_file(&self) -> String {
        let path = self.path();
        if self.host.is_empty() {
            return path;
        }
        let sep = if path.is_empty() || path.starts_with('/') {
            ""
        } else {
            "/"
        };
        format!("//{}{sep}{path}", self.host)
    }

    /// The file this url is read from: a `file:` url's, and under the shell directory `root`
    /// what Quickshell reads for a `qs:` url (QsInterceptNetworkAccessManager::createRequest):
    /// `qs:@/qs/PATH` is `PATH` in the shell directory, any other `qs:` path is the file.
    pub fn local_file(&self, root: Option<&Path>) -> Option<PathBuf> {
        match (self.scheme.as_deref(), root) {
            (Some("file"), _) => Some(PathBuf::from(self.to_local_file())),
            (Some("qs"), Some(root)) => {
                let path = self.path();
                Some(PathBuf::from(match path.strip_prefix("@/qs/") {
                    Some(rel) => file_path(root, rel),
                    None => path,
                }))
            }
            _ => None,
        }
    }
}

/// `QDir(dir).filePath(name)`: an absolute name (a resource path, `:name`, is one) is itself,
/// an empty one is the directory as it is written.
fn file_path(dir: &Path, name: &str) -> String {
    let dir = dir.to_string_lossy();
    if name.starts_with(['/', ':']) {
        name.into()
    } else if name.is_empty() || dir.is_empty() || dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
}

/// What a url Quickshell's interceptor is asked about names
/// (QQmlAbstractUrlInterceptor::DataType).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A url QML code resolves: `Qt.resolvedUrl`, an Image's or a Loader's `source`.
    UrlString,
    /// A QML or JavaScript file the engine loads.
    QmlFile,
}

/// `QsUrlInterceptor::intercept` of Quickshell 0.3.2 for the shell directory `root`: `root:PATH`
/// is `qs:@/qs/PATH`; a `qs:` url that is neither under `@/qs/` nor absolute is
/// `qrc:/qs-blackhole`; a url QML code resolves keeps the `qs:` scheme when it names a `.qml`
/// file and is the `file:` url of that file otherwise.
pub fn intercept(mut url: QUrl, root: &Path, kind: Kind) -> QUrl {
    if url.scheme.as_deref() == Some("root") {
        url.scheme = Some("qs".into());
        let path = url.path();
        url.set_path(&format!("@/qs/{}", path.strip_prefix('/').unwrap_or(&path)));
    }
    if url.scheme.as_deref() == Some("qs") {
        let path = url.path();
        let path = match path.strip_prefix("@/qs/") {
            Some(rel) => file_path(root, rel),
            None if !path.starts_with('/') => return QUrl::parse("qrc:/qs-blackhole"),
            None => path,
        };
        if kind == Kind::UrlString && !path.ends_with(".qml") {
            url.scheme = Some("file".into());
            url.set_path(&path);
        }
    }
    url
}

/// The url of the document `file`. Quickshell loads the shell as `qs:@/qs/shell.qml`, so a
/// document in the shell directory `root` is `qs:@/qs/PATH`; any other is a `file:` url. A
/// document a type or an import of the document `like` names has a url relative to it: a
/// `file:` url for one loaded by a `file:` url, `qs:/PATH` for one loaded by an absolute path.
pub fn document_url(file: &Path, root: Option<&Path>, like: Option<&QUrl>) -> QUrl {
    let file = std::path::absolute(file).unwrap_or_else(|_| file.to_path_buf());
    let qs = |path: &str| {
        let mut u = QUrl {
            scheme: Some("qs".into()),
            ..QUrl::default()
        };
        u.set_path(path);
        u
    };
    match like.map(|l| (l.scheme.as_deref(), l.path.starts_with('/'))) {
        Some((Some("file"), _)) => return QUrl::from_local_file(&file),
        Some((Some("qs"), true)) => return qs(&file.to_string_lossy()),
        _ => {}
    }
    let root = root.and_then(|r| std::path::absolute(r).ok());
    match root.as_ref().and_then(|r| file.strip_prefix(r).ok()) {
        Some(rel) => qs(&format!("@/qs/{}", rel.to_string_lossy())),
        None => QUrl::from_local_file(&file),
    }
}

/// `QUrl::toString()`.
impl std::fmt::Display for QUrl {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        if self.invalid {
            return Ok(());
        }
        if let Some(s) = &self.scheme {
            write!(f, "{s}:")?;
        }
        match &self.authority {
            Some(a) => write!(f, "//{a}")?,
            // The XDG file URI spec's three slashes.
            None if self.is_local_file() && self.path.starts_with('/') => f.write_str("//")?,
            None => {}
        }
        f.write_str(&recode(&self.path, PATH_OUT))?;
        if let Some(q) = &self.query {
            write!(f, "?{}", recode(q, QUERY_OUT))?;
        }
        if let Some(fr) = &self.fragment {
            write!(f, "#{}", recode(fr, FRAGMENT_OUT))?;
        }
        Ok(())
    }
}

impl QUrl {
    /// `QUrl::toEncoded()` (FullyEncoded): what `toString()` prints with every space and
    /// every character above ASCII escaped, and the host in its ASCII spelling.
    pub fn to_encoded(&self) -> String {
        let shown = self.to_string();
        let Some(a) = self.authority.as_deref().filter(|_| !self.invalid) else {
            return escaped(&shown);
        };
        // The authority is printed once, behind the scheme.
        let at = self.scheme.as_ref().map_or(0, |s| s.len() + 1) + 2;
        let (info, rest) = a.split_at(a.find('@').map_or(0, |i| i + 1));
        let host = match rest.strip_prefix(self.host.as_str()) {
            Some(port) => crate::qurlhost::ace(&self.host) + port,
            None => rest.to_string(),
        };
        let after = &shown[at + a.len()..];
        escaped(&shown[..at]) + &escaped(info) + &host + &escaped(after)
    }
}

/// `s` with its spaces and what is above ASCII escaped (EncodeSpaces | EncodeUnicode).
fn escaped(s: &str) -> String {
    let mut out = Vec::with_capacity(s.len());
    for c in s.bytes() {
        match c {
            b' ' | 0x80.. => push_hex(&mut out, c),
            _ => out.push(c),
        }
    }
    String::from_utf8(out).unwrap_or_default()
}

/// `QQmlEngine::baseUrl()`: the working directory.
fn engine_base() -> QUrl {
    let mut dir = std::env::current_dir().unwrap_or_default();
    dir.push("");
    QUrl::from_local_file(&dir)
}

/// The url of a document, `doc`, or the engine's base url without one.
pub fn base(doc: Option<&QUrl>) -> QUrl {
    doc.cloned().unwrap_or_else(engine_base)
}

/// `QQmlContextData::resolvedUrl(url)` in a context whose document has the url `base` (none:
/// a context no document is behind): a url without a scheme is resolved against the
/// document, or against the engine's base url; an empty one stays empty and one with a
/// scheme stays as it is. Under a shell directory `root` the answer then goes through
/// Quickshell's url interceptor, as `QQmlEngine::interceptUrl` does.
pub fn resolved_in(base: Option<&QUrl>, url: &str, root: Option<&Path>) -> QUrl {
    let url = QUrl::parse(url);
    let url = if url.is_relative() && !url.is_empty() {
        self::base(base).resolved(&url)
    } else {
        url
    };
    match root {
        Some(root) if !url.is_empty() => intercept(url, root, Kind::UrlString),
        _ => url,
    }
}

/// `QtObject::resolvedUrl(url)` for code no QML context runs:
/// `engine->baseUrl().resolved(url)`.
pub fn resolved_without_context(url: &str) -> QUrl {
    engine_base().resolved(&QUrl::parse(url))
}

/// The path of a string that is no QUrl to its reader: Quickshell's `FileView::setPath` and
/// the original's `FileUtils.trimFileProtocol` cut a literal leading `file://` and decode nothing.
pub fn trim_file_protocol(s: &str) -> &str {
    s.strip_prefix("file://").unwrap_or(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// QUrl(u).toLocalFile() (Qt 6.12); a plain path is a relative url's path.
    #[test]
    fn url_to_path_matches_qurl() {
        for (u, qt) in [
            ("file:///home/u/a%20b.png", "/home/u/a b.png"),
            ("file:///home/u/a b.png", "/home/u/a b.png"),
            ("/plain/path", "/plain/path"),
            ("file:///a/../b", "/a/../b"),
            ("file:///x/my#1.png", "/x/my"),
            ("file:///x/q?y=1", "/x/q"),
            ("file:///%E2%82%AC", "/€"),
            ("file:///a//b", "/a//b"),
            ("file://host/a", "//host/a"),
            ("file:/a", "/a"),
            ("file:///a%2Fb", "/a/b"),
            ("file:///a%zz", "/a%zz"),
            ("file:///a%20b%", "/a%20b%"),
            ("//HOST/a", "//host/a"),
        ] {
            assert_eq!(crate::native::url_to_path(u), qt, "{u}");
        }
    }

    /// What Qt 6.12.0 answers for `Qt.resolvedUrl(url)`: in the document
    /// `/doc dir/shell.qml`; in a component without a url (when that differs), and in
    /// `QQmlEngine::evaluate` (when that differs again), both with the working directory
    /// `/tmp/probe cwd`. Recorded from PySide6 6.12.0 in the tools/testenv image.
    const QT: &[(&str, &str, &str, &str)] = &[
        ("/abs/a%20b.png", "file:///abs/a b.png", "", ""),
        ("/abs/a b.png", "file:///abs/a b.png", "", ""),
        ("/abs/a%25b", "file:///abs/a%25b", "", ""),
        ("/abs/a%b", "file:///abs/a%25b", "", ""),
        ("/abs/a%", "file:///abs/a%25", "", ""),
        ("/abs/a%zzb", "file:///abs/a%25zzb", "", ""),
        ("/abs/a%23b", "file:///abs/a%23b", "", ""),
        ("/abs/a%3Fb", "file:///abs/a%3Fb", "", ""),
        ("/abs/a%3fb", "file:///abs/a%3Fb", "", ""),
        ("/abs/a%2Fb", "file:///abs/a%2Fb", "", ""),
        ("/abs/a%41b", "file:///abs/aAb", "", ""),
        ("/abs/a%7Eb", "file:///abs/a~b", "", ""),
        ("/abs/a%2Bb", "file:///abs/a%2Bb", "", ""),
        ("/abs/a%26b", "file:///abs/a%26b", "", ""),
        ("/abs/a%3Ab", "file:///abs/a%3Ab", "", ""),
        ("/abs/a%40b", "file:///abs/a%40b", "", ""),
        ("/abs/a%5Bb", "file:///abs/a%5Bb", "", ""),
        ("/abs/a%22b", "file:///abs/a%22b", "", ""),
        ("/abs/a%3Cb", "file:///abs/a%3Cb", "", ""),
        ("/abs/a%7Bb", "file:///abs/a%7Bb", "", ""),
        ("/abs/a%5Cb", "file:///abs/a%5Cb", "", ""),
        ("/abs/a%5Eb", "file:///abs/a%5Eb", "", ""),
        ("/abs/a%60b", "file:///abs/a%60b", "", ""),
        ("/abs/a%7Cb", "file:///abs/a%7Cb", "", ""),
        ("/abs/a%0Ab", "file:///abs/a%0Ab", "", ""),
        ("/abs/a%7Fb", "file:///abs/a%7Fb", "", ""),
        ("/abs/a%FFb", "file:///abs/a%FFb", "", ""),
        ("/abs/%D0%B0.png", "file:///abs/а.png", "", ""),
        ("/abs/%d0%b0.png", "file:///abs/а.png", "", ""),
        ("/abs/а.png", "file:///abs/а.png", "", ""),
        ("/abs/a\"b", "file:///abs/a%22b", "", ""),
        ("/abs/a<b>", "file:///abs/a%3Cb%3E", "", ""),
        ("/abs/a[b]", "file:///abs/a[b]", "", ""),
        ("/abs/a|b", "file:///abs/a%7Cb", "", ""),
        ("/abs/a^b", "file:///abs/a%5Eb", "", ""),
        ("/abs/a\\b", "file:///abs/a%5Cb", "", ""),
        ("/abs/a{b}", "file:///abs/a%7Bb%7D", "", ""),
        ("/abs/a`b", "file:///abs/a%60b", "", ""),
        ("/abs/a'b", "file:///abs/a'b", "", ""),
        ("/abs/a b?x y#f g", "file:///abs/a b?x y#f g", "", ""),
        ("/abs/a?x%20y#f%20g", "file:///abs/a?x y#f g", "", ""),
        ("/abs/a?x%23y", "file:///abs/a?x%23y", "", ""),
        ("/abs/a#f#g", "file:///abs/a#f#g", "", ""),
        ("/abs/./a/../b", "file:///abs/b", "", ""),
        ("/abs//a", "file:///abs/a", "", ""),
        ("/abs/a/", "file:///abs/a/", "", ""),
        ("/", "file:///", "", ""),
        ("//host/a", "file://host/a", "", ""),
        ("/abs/a;b=c", "file:///abs/a;b=c", "", ""),
        ("/abs/a+b&c=d,e!$*()", "file:///abs/a+b&c=d,e!$*()", "", ""),
        ("/abs/a:b@c", "file:///abs/a:b@c", "", ""),
        ("/abs/a%E2%82%ACb", "file:///abs/a€b", "", ""),
        ("/abs/a\tb", "file:///abs/a%09b", "", ""),
        (
            "big.png",
            "file:///doc dir/big.png",
            "file:///tmp/probe cwd/big.png",
            "",
        ),
        (
            "a%20b.png",
            "file:///doc dir/a b.png",
            "file:///tmp/probe cwd/a b.png",
            "",
        ),
        (
            "a b.png",
            "file:///doc dir/a b.png",
            "file:///tmp/probe cwd/a b.png",
            "",
        ),
        (
            "a%25b",
            "file:///doc dir/a%25b",
            "file:///tmp/probe cwd/a%25b",
            "",
        ),
        (
            "a%23b",
            "file:///doc dir/a%23b",
            "file:///tmp/probe cwd/a%23b",
            "",
        ),
        (
            "%D0%B0.png",
            "file:///doc dir/а.png",
            "file:///tmp/probe cwd/а.png",
            "",
        ),
        (
            "а.png",
            "file:///doc dir/а.png",
            "file:///tmp/probe cwd/а.png",
            "",
        ),
        (
            "a\"b",
            "file:///doc dir/a%22b",
            "file:///tmp/probe cwd/a%22b",
            "",
        ),
        (
            "a[b]",
            "file:///doc dir/a[b]",
            "file:///tmp/probe cwd/a[b]",
            "",
        ),
        ("../x.png", "file:///x.png", "file:///tmp/x.png", ""),
        (
            "./a/../big.png",
            "file:///doc dir/big.png",
            "file:///tmp/probe cwd/big.png",
            "",
        ),
        (
            "sub/",
            "file:///doc dir/sub/",
            "file:///tmp/probe cwd/sub/",
            "",
        ),
        (
            "?q=1",
            "file:///doc dir/shell.qml?q=1",
            "file:///tmp/probe cwd/?q=1",
            "",
        ),
        (
            "#f",
            "file:///doc dir/shell.qml#f",
            "file:///tmp/probe cwd/#f",
            "",
        ),
        (
            "a.png?x y#f g",
            "file:///doc dir/a.png?x y#f g",
            "file:///tmp/probe cwd/a.png?x y#f g",
            "",
        ),
        (
            "a%2Fb",
            "file:///doc dir/a%2Fb",
            "file:///tmp/probe cwd/a%2Fb",
            "",
        ),
        ("a:b", "a:b", "", ""),
        (
            "./a:b",
            "file:///doc dir/a:b",
            "file:///tmp/probe cwd/a:b",
            "",
        ),
        (".", "file:///doc dir/", "file:///tmp/probe cwd/", ""),
        ("..", "file:///", "file:///tmp/", ""),
        ("", "", "", "file:///tmp/probe cwd/"),
        ("file:///b", "file:///b", "", ""),
        ("file:/b", "file:///b", "", ""),
        ("file:///a%20b", "file:///a b", "", ""),
        ("file:///a b", "file:///a b", "", ""),
        ("file://host/a", "file://host/a", "", ""),
        ("file:b", "file:b", "", ""),
        ("http://e/x y.png", "http://e/x y.png", "", ""),
        ("http://e/x%20y.png", "http://e/x y.png", "", ""),
        ("HTTP://E/x", "http://e/x", "", ""),
        ("qrc:/a b", "qrc:/a b", "", ""),
        ("qrc:a", "qrc:a", "", ""),
        ("image://p/a b", "image://p/a b", "", ""),
        ("data:text/plain,a b", "data:text/plain,a b", "", ""),
        ("C:/x", "c:/x", "", ""),
        ("x:y", "x:y", "", ""),
        (
            "~/a",
            "file:///doc dir/~/a",
            "file:///tmp/probe cwd/~/a",
            "",
        ),
        (
            " lead",
            "file:///doc dir/ lead",
            "file:///tmp/probe cwd/ lead",
            "",
        ),
        (
            "trail ",
            "file:///doc dir/trail ",
            "file:///tmp/probe cwd/trail ",
            "",
        ),
        ("/abs/%2E%2E/x", "file:///x", "", ""),
        ("/abs/a%2Eb", "file:///abs/a.b", "", ""),
        ("/abs/%C2%80", "file:///abs/\u{80}", "", ""),
        ("/abs/%EF%BF%BF", "file:///abs/%EF%BF%BF", "", ""),
        ("/abs/%ED%A0%80", "file:///abs/%ED%A0%80", "", ""),
        ("/abs/%F0%9F%98%80", "file:///abs/😀", "", ""),
        ("/abs/a%C3", "file:///abs/a%C3", "", ""),
        ("/abs/a%C3%28", "file:///abs/a%C3%28", "", ""),
        (
            "http://e/a/../b//c",
            "http://e/a/../b//c",
            "",
            "http://e/b//c",
        ),
        (
            "http://e/a%2Fb?x%3Fy#f%23g",
            "http://e/a%2Fb?x%3Fy#f%23g",
            "",
            "",
        ),
        (
            "/abs/a?x%3Fy%26z#f%3Fg%23h",
            "file:///abs/a?x%3Fy%26z#f%3Fg%23h",
            "",
            "",
        ),
        ("/abs/a%27b", "file:///abs/a%27b", "", ""),
        ("/abs/a%21b", "file:///abs/a%21b", "", ""),
        ("/abs/a%5Db", "file:///abs/a%5Db", "", ""),
        ("/abs/a%2fb", "file:///abs/a%2Fb", "", ""),
        ("/abs/a%7eb", "file:///abs/a~b", "", ""),
        ("/abs/a%2db", "file:///abs/a-b", "", ""),
        ("a/b/../../../../x", "file:///../x", "file:///x", ""),
        ("/abs/a/..", "file:///abs/", "", ""),
        ("/abs/a/.", "file:///abs/a/", "", ""),
        ("/..", "file:///..", "", ""),
        ("/../x", "file:///../x", "", ""),
        ("//", "file://", "", ""),
        ("///a", "file:///a", "", ""),
        ("file:", "file:", "", ""),
        ("file:///", "file:///", "", ""),
        ("file://", "file://", "", ""),
        ("/abs/a%00b", "file:///abs/a%00b", "", ""),
        ("/abs/a%1Fb", "file:///abs/a%1Fb", "", ""),
        ("/abs/a\u{a0}b", "file:///abs/a\u{a0}b", "", ""),
        ("/abs/a%C2%A0b", "file:///abs/a\u{a0}b", "", ""),
        ("/abs/a%E2%80%8Bb", "file:///abs/a\u{200b}b", "", ""),
        (
            "?",
            "file:///doc dir/shell.qml?",
            "file:///tmp/probe cwd/?",
            "",
        ),
        (
            "#",
            "file:///doc dir/shell.qml#",
            "file:///tmp/probe cwd/#",
            "",
        ),
        ("/abs/a?", "file:///abs/a?", "", ""),
        ("/abs/a#", "file:///abs/a#", "", ""),
        (
            "a?x#y?z",
            "file:///doc dir/a?x#y?z",
            "file:///tmp/probe cwd/a?x#y?z",
            "",
        ),
        ("/abs/a?x<y>\"z", "file:///abs/a?x%3Cy%3E%22z", "", ""),
        (
            "/abs/a#x<y>\"z {}|\\^`",
            "file:///abs/a#x%3Cy%3E%22z %7B%7D%7C%5C%5E%60",
            "",
            "",
        ),
        (
            "/abs/a?x {}|\\^`[]",
            "file:///abs/a?x %7B%7D%7C%5C%5E%60[]",
            "",
            "",
        ),
        ("/abs/%%41", "file:///abs/%25%2541", "", ""),
        ("/abs/%4", "file:///abs/%254", "", ""),
        ("/abs/%4g", "file:///abs/%254g", "", ""),
        ("/abs/a?x%5By%5D", "file:///abs/a?x%5By%5D", "", ""),
        ("/abs/a?x%2Fy/z", "file:///abs/a?x%2Fy/z", "", ""),
        (
            "/abs/a?%41%7E%25%",
            "file:///abs/a?%2541%257E%2525%25",
            "",
            "",
        ),
        (
            "/abs/a#%41%7E%25%",
            "file:///abs/a#%2541%257E%2525%25",
            "",
            "",
        ),
        ("/abs/😀", "file:///abs/😀", "", ""),
        ("/abs/a%C0%80b", "file:///abs/a%C0%80b", "", ""),
        ("/abs/a%E0%80%80b", "file:///abs/a%E0%80%80b", "", ""),
        ("/abs/a%F4%90%80%80b", "file:///abs/a%F4%90%80%80b", "", ""),
        ("/abs/a%EF%B7%90b", "file:///abs/a%EF%B7%90b", "", ""),
        ("http://user:pw@e:80/x", "http://user:pw@e:80/x", "", ""),
        ("http://E%41/x", "http://ea/x", "", ""),
        ("FILE:///A", "file:///A", "", ""),
        // A first segment with a colon: QUrl calls the url invalid and still resolves it.
        (
            ":x.png",
            "file:///doc dir/:x.png",
            "file:///tmp/probe cwd/:x.png",
            "",
        ),
        (
            ":/x.png",
            "file:///doc dir/:/x.png",
            "file:///tmp/probe cwd/:/x.png",
            "",
        ),
        (
            "./:x.png",
            "file:///doc dir/:x.png",
            "file:///tmp/probe cwd/:x.png",
            "",
        ),
        (":", "file:///doc dir/:", "file:///tmp/probe cwd/:", ""),
        (
            "a/:x.png",
            "file:///doc dir/a/:x.png",
            "file:///tmp/probe cwd/a/:x.png",
            "",
        ),
        (
            ":x/../y.png",
            "file:///doc dir/y.png",
            "file:///tmp/probe cwd/y.png",
            "",
        ),
    ];

    #[test]
    fn resolved_urls_are_qt_s() {
        let doc = QUrl::from_local_file(Path::new("/doc dir/shell.qml"));
        let cwd = QUrl::from_local_file(Path::new("/tmp/probe cwd/"));
        let mut wrong = Vec::new();
        for &(url, in_doc, no_doc, bare) in QT {
            let no_doc = if no_doc.is_empty() { in_doc } else { no_doc };
            let bare = if bare.is_empty() { no_doc } else { bare };
            let got = [
                resolved_in(Some(&doc), url, None).to_string(),
                resolved_in(Some(&cwd), url, None).to_string(),
                cwd.resolved(&QUrl::parse(url)).to_string(),
            ];
            for (got, qt, how) in [
                (&got[0], in_doc, "in a document"),
                (&got[1], no_doc, "without a document"),
                (&got[2], bare, "without a context"),
            ] {
                if got != qt {
                    wrong.push(format!("{url:?} {how}: {got:?}, Qt {qt:?}"));
                }
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }

    /// What Qt 6.12.0 answers for the host of a url: `Qt.resolvedUrl(url)` in the document
    /// `/doc dir/shell.qml` (nothing for a url QUrl calls invalid) and `QUrl(url).toLocalFile()`.
    /// Recorded from PySide6 6.12.0 in the tools/testenv image.
    const QT_HOSTS: &[(&str, &str, &str)] = &[
        ("http://пример.рф/a", "http://пример.рф/a", ""),
        ("http://ПРИМЕР.РФ/a", "http://пример.рф/a", ""),
        ("http://bücher.de/x", "http://bücher.de/x", ""),
        ("http://BÜCHER.de/x", "http://bücher.de/x", ""),
        ("http://xn--bcher-kva.de/x", "http://bücher.de/x", ""),
        ("http://XN--BCHER-KVA.DE/x", "http://bücher.de/x", ""),
        ("http://xn--e1afmkfd.xn--p1ai/", "http://пример.рф/", ""),
        ("http://日本語.jp/", "http://日本語.jp/", ""),
        ("http://例え.テスト/", "http://xn--r8jz45g.xn--zckzah/", ""),
        ("http://faß.de/", "http://faß.de/", ""),
        ("http://ﬁsh.com/", "http://fish.com/", ""),
        ("http://a.b\u{3002}c/", "http://a.b.c/", ""),
        ("http://ex ample.com/", "", ""),
        ("http://exa%6Dple.com/", "http://example.com/", ""),
        ("http://ex%20ample.com/", "", ""),
        ("http://%D0%BF.com/", "http://п.com/", ""),
        ("http://é.com/", "http://é.com/", ""),
        ("http://e\u{301}.com/", "http://é.com/", ""),
        ("http://☃.net/", "http://☃.net/", ""),
        ("http://😀.com/", "http://😀.com/", ""),
        ("http://a_b.com/", "http://a_b.com/", ""),
        ("http://-a.com/", "", ""),
        ("http://xn--a.com/", "http://xn--a.com/", ""),
        ("http://xn--.com/", "", ""),
        ("http://a..b/", "", ""),
        ("http://.a/", "", ""),
        ("file://хост/a", "file://xn--n1agdj/a", "//xn--n1agdj/a"),
        (
            "file://HÖST/a b",
            "file://xn--hst-sna/a b",
            "//xn--hst-sna/a b",
        ),
        ("//Хост/x", "file://xn--n1agdj/x", ""),
        ("qs://Хост/x", "qs://xn--n1agdj/x", ""),
        ("http://user@Пример.рф:80/", "http://user@пример.рф:80/", ""),
        ("http://[::1]/", "http://[::1]/", ""),
        ("http://[::FFFF:1.2.3.4]/", "http://[::ffff:1.2.3.4]/", ""),
        ("http://[v1.a]/", "http://[v1.a]/", ""),
        ("http://1.2.3.4/", "http://1.2.3.4/", ""),
        ("http://0x7f.1/", "http://127.0.0.1/", ""),
        ("http://пример.com/", "http://пример.com/", ""),
        ("http://paypаl.com/", "http://paypаl.com/", ""),
        ("http://аbc.org/", "http://аbc.org/", ""),
        ("http://ab\u{200d}c.com/", "", ""),
        ("http://xn--80akhbyknj4f/", "http://xn--80akhbyknj4f/", ""),
        ("http://испытание/", "http://xn--80akhbyknj4f/", ""),
        ("http://ß/", "http://xn--zca/", ""),
        ("http://İ.com/", "http://i\u{307}.com/", ""),
        ("http://ǅ.com/", "http://dž.com/", ""),
        ("http://a_bü.com/", "http://a_bü.com/", ""),
        (
            "http://a_b.xn--bcher-kva.com/",
            "http://a_b.bücher.com/",
            "",
        ),
        ("http://1ü.com/", "http://1ü.com/", ""),
        ("http://ü1.com/", "http://ü1.com/", ""),
        ("http://user:pw@host/", "http://user:pw@host/", ""),
        ("http://us%20er:p%40w@host/", "http://us er:p%40w@host/", ""),
        ("http://us er@host/", "http://us er@host/", ""),
        ("http://@host/", "http://@host/", ""),
        ("http://:@host/", "http://:@host/", ""),
        ("http://a@b@host/", "", ""),
        ("http://u:p:q@host/", "http://u:p:q@host/", ""),
        ("http://us/er@host/x", "http://us/er@host/x", ""),
        (
            "http://us%3Aer:p%3Aw@host/",
            "http://us%3Aer:p%3Aw@host/",
            "",
        ),
        ("http://u%5B%5D@h/", "http://u%5B%5D@h/", ""),
        ("http://host:80/", "http://host:80/", ""),
        ("http://host:080/", "http://host:80/", ""),
        ("http://host:/", "http://host/", ""),
        ("http://host:65536/", "", ""),
        ("http://host:8a/", "", ""),
        ("http://host:-1/", "", ""),
        ("http://[::1]:80/", "http://[::1]:80/", ""),
        ("http://[::1]:/", "http://[::1]/", ""),
        ("http://[::1/", "", ""),
        ("http://[::1]x/", "", ""),
        ("http://[1:2:3:4:5:6:7:8]/", "http://[1:2:3:4:5:6:7:8]/", ""),
        ("http://[1:0:0:2:0:0:0:3]/", "http://[1:0:0:2::3]/", ""),
        ("http://[0:0:0:0:0:0:0:1]/", "http://[::1]/", ""),
        ("http://[::]/", "http://[::]/", ""),
        ("http://[1::]/", "http://[1::]/", ""),
        ("http://[::0.0.0.1]/", "http://[::1]/", ""),
        ("http://[::1.2.3.4]/", "http://[::1.2.3.4]/", ""),
        (
            "http://[1:2:3:4:5:6:1.2.3.4]/",
            "http://[1:2:3:4:5:6:102:304]/",
            "",
        ),
        ("http://[ABCD:0:0:1::]/", "http://[abcd:0:0:1::]/", ""),
        ("http://[1:2:3:4:5:6:7::]/", "http://[1:2:3:4:5:6:7:0]/", ""),
        ("http://[::1%25eth0]/", "http://[::1%25eth0]/", ""),
        ("http://[::1%25]/", "", ""),
        ("http://[::1%eth0]/", "http://[::1%25eth0]/", ""),
        ("http://[%3A:1]/", "http://[::1]/", ""),
        ("http://[0x1::2]/", "http://[1::2]/", ""),
        ("http://[1:2]/", "", ""),
        ("http://[fe80::1:0:0:1]/", "http://[fe80::1:0:0:1]/", ""),
        ("http://[0:0:1:0:0:1:0:0]/", "http://[::1:0:0:1:0:0]/", ""),
        ("http://[::ffff:0:1]/", "http://[::ffff:0.0.0.1]/", ""),
        ("http://[::01.2.3.4]/", "", ""),
        ("http://[vF.a:b]/", "http://[vF.a:b]/", ""),
        ("http://[va.x%41]/", "http://[vA.xA]/", ""),
        ("http://[v1.]/", "", ""),
        ("http://[vG.a]/", "", ""),
        ("http://[v1.a b]/", "", ""),
        ("http://[v1a]/", "", ""),
        ("http://[v.aaa]/", "", ""),
        ("http://1/", "http://0.0.0.1/", ""),
        ("http://1.2/", "http://1.0.0.2/", ""),
        ("http://1.2.3/", "http://1.2.0.3/", ""),
        ("http://1.2.3.4.5/", "http://1.2.3.4.5/", ""),
        ("http://1.2.3.4./", "http://1.2.3.4./", ""),
        ("http://256.1.1.1/", "http://256.1.1.1/", ""),
        ("http://010.1.1.1/", "http://8.1.1.1/", ""),
        ("http://0x10.0x10.0x10.0x10/", "http://16.16.16.16/", ""),
        ("http://0b11.1/", "http://3.0.0.1/", ""),
        ("http://+1.2.3.4/", "http://1.2.3.4/", ""),
        ("http://1.2.3.08/", "http://1.2.3.08/", ""),
        ("http://4294967295/", "http://255.255.255.255/", ""),
        ("http://4294967296/", "http://4294967296/", ""),
        ("http://%31%30.0.0.1/", "http://10.0.0.1/", ""),
        ("http://１.２.３.４/", "http://1.2.3.4/", ""),
        ("http://%5B::1%5D/", "", ""),
        ("http://0x/", "http://0x/", ""),
        ("http://1.0x100/", "http://1.0.1.0/", ""),
        ("http://0.0.0.256/", "http://0.0.0.256/", ""),
        ("http://1.2.65536/", "http://1.2.65536/", ""),
        ("http://EXAMPLE.com./", "http://example.com./", ""),
        ("http://bücher.de./", "http://bücher.de./", ""),
        ("http://./", "", ""),
        ("http://a.-b.com/", "", ""),
        ("http://a-.com/", "", ""),
        ("http://ab--c.com/", "http://ab--c.com/", ""),
        ("http://ab--ü.com/", "", ""),
        (
            "http://xn--bcher-kva.example/",
            "http://xn--bcher-kva.example/",
            "",
        ),
        ("http://xn--Bcher-kva.de/", "http://bücher.de/", ""),
        ("http://xn--bcher.de/", "http://≆≎≈.de/", ""),
        ("http://xn--b%C3%BCcher.de/", "", ""),
        ("http://שלום.com/", "http://שלום.com/", ""),
        ("http://שלום1.com/", "http://שלום1.com/", ""),
        ("http://1שלום.com/", "http://xn--1-bicuf1d.com/", ""),
        ("http://aשלום.com/", "http://xn--a-bicuf1d.com/", ""),
        ("http://مثال.com/", "http://مثال.com/", ""),
        ("http://a.שלום.com/", "http://a.שלום.com/", ""),
        ("http://1a.שלום.com/", "http://1a.xn--9dbne9b.com/", ""),
        ("http://xn--9dbne9b.com/", "http://שלום.com/", ""),
        ("http://xn--1-0hcn6bz.com/", "http://ו1אןפ.com/", ""),
        ("http://a\u{301}b.com/", "http://áb.com/", ""),
        ("http://\u{301}a.com/", "", ""),
        ("http://ab\u{200c}c.com/", "", ""),
        (
            "http://क\u{94d}\u{200d}ष.com/",
            "http://क\u{94d}\u{200d}ष.com/",
            "",
        ),
        ("http://ب\u{200c}ب.com/", "http://ب\u{200c}ب.com/", ""),
        ("http://ẞ.de/", "http://ß.de/", ""),
        ("http://ς.gr/", "http://ς.gr/", ""),
        ("http://Σ.gr/", "http://σ.gr/", ""),
        ("http://a\u{ad}b.com/", "http://ab.com/", ""),
        ("http://a\u{2060}.com/", "http://a.com/", ""),
        ("http://①.com/", "http://1.com/", ""),
        ("http://a\u{ff0e}b.com/", "http://a.b.com/", ""),
        ("http://a%2Eb.com/", "http://a.b.com/", ""),
        ("http://a%2Fb.com/", "", ""),
        ("http://a%3Ab.com/", "", ""),
        ("http://A%41%7a.com/", "http://aaz.com/", ""),
        ("http://ex\"ample.com/", "", ""),
        ("http://a,b.com/", "", ""),
        ("http://a=b.com/", "", ""),
        ("http://a b/", "", ""),
        ("http://a%00b/", "", ""),
        (
            "http://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.com/",
            "http://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.com/",
            "",
        ),
        (
            "http://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.com/",
            "",
            "",
        ),
        (
            "http://üüüüüüüüüüüüüüüüüüüüüüüüüüüüüüüüüüüüüüüüüüüüüüüüüüüüüüüüüüüü.com/",
            "",
            "",
        ),
        (
            "http://üüüüüüüüüüüüüüüüüüüü.com/",
            "http://üüüüüüüüüüüüüüüüüüüü.com/",
            "",
        ),
        ("//bad host/x", "", ""),
        ("//HOST:80/x", "file://host:80/x", ""),
        ("//u@h/x", "file://u@h/x", ""),
        ("http://host", "http://host", ""),
        ("http://host?q", "http://host?q", ""),
        ("http://host#f", "http://host#f", ""),
        ("http://h/p?a@b:c", "http://h/p?a@b:c", ""),
        ("file://host:80/a", "file://host:80/a", "//host/a"),
        ("file://u@host/a", "file://u@host/a", "//host/a"),
        ("qs://[::1]/x", "qs://[::1]/x", ""),
        ("http:///x", "http:///x", ""),
        ("http://:80/", "http://:80/", ""),
        ("http://😀--a.com/", "", ""),
        ("http://a.٠١.com/", "http://a.xn--8hbc.com/", ""),
        // An IPvFuture address that does not parse leaves what was read of it.
        (
            "file://[v1.01.2.3.4:255.255.255.255:0%25]/",
            "",
            "//[v1.01.2.3.4:255.255.255.255:0/",
        ),
        ("file://[vF.::a:%41:g: 1%eth0]:8a/x y", "", "//[vF.::a:/x y"),
    ];

    #[test]
    fn hosts_are_qt_s() {
        let doc = QUrl::from_local_file(Path::new("/doc dir/shell.qml"));
        let mut wrong = Vec::new();
        for &(url, resolved, local) in QT_HOSTS {
            let got = resolved_in(Some(&doc), url, None).to_string();
            if got != resolved {
                wrong.push(format!("{url:?} resolved: {got:?}, Qt {resolved:?}"));
            }
            let parsed = QUrl::parse(url);
            let got = if parsed.is_local_file() {
                parsed.to_local_file()
            } else {
                String::new()
            };
            if got != local {
                wrong.push(format!("{url:?} local file: {got:?}, Qt {local:?}"));
            }
        }
        assert!(
            wrong.is_empty(),
            "{} differ\n{}",
            wrong.len(),
            wrong.join("\n")
        );
    }

    #[test]
    fn the_engine_s_base_url_is_the_working_directory() {
        let cwd = std::env::current_dir().unwrap();
        assert_eq!(
            resolved_in(None, "a%20b.png", None).to_local_file(),
            cwd.join("a b.png").to_string_lossy()
        );
        assert_eq!(
            Path::new(&resolved_without_context("").to_local_file()),
            cwd.join("")
        );
    }

    /// `QUrl::toLocalFile()` of the url resolved in the document `/doc dir/shell.qml`, and
    /// `QUrl::fromLocalFile(path).toString()` (PySide6 6.12.0).
    #[test]
    fn a_local_file_is_the_decoded_path() {
        let doc = QUrl::from_local_file(Path::new("/doc dir/shell.qml"));
        for (url, qt) in [
            ("/abs/a%20b.png", "/abs/a b.png"),
            ("/abs/50% a.png", "/abs/50% a.png"),
            ("file:///abs/50%25%20a.png", "/abs/50% a.png"),
            ("/abs/a%23b%3Fc", "/abs/a#b?c"),
            ("/abs/a#b?c", "/abs/a"),
            ("/abs/a?b", "/abs/a"),
            ("file:///a%2Fb", "/a/b"),
            ("file://host/a", "//host/a"),
            ("file://HOST/a", "//host/a"),
            ("//HOST/a", "//host/a"),
            ("file:/a", "/a"),
            ("file:///%E2%82%AC", "/\u{20ac}"),
            ("file:///a%20b%", "/a%20b%"),
            ("file:///a%20b%2", "/a%20b%2"),
            ("file:///a%zz", "/a%zz"),
            ("file:///a%FFb", "/a\u{fffd}b"),
            ("file:///a%EF%BF%BFb", "/a\u{fffd}\u{fffd}\u{fffd}b"),
            ("file:///a%00b", "/a\0b"),
            ("a%20b.png", "/doc dir/a b.png"),
            ("../x y", "/x y"),
            ("file:///a/../b", "/a/../b"),
            ("file:///a//b", "/a//b"),
            ("file:a b", "a b"),
            ("FILE:///A b", "/A b"),
            ("file:///x/my#1.png", "/x/my"),
            ("file:///x/q?y=1", "/x/q"),
            ("file:///a%7Fb%09c", "/a\u{7f}b\tc"),
            ("/a\u{7f}b\tc", "/a\u{7f}b\tc"),
        ] {
            let resolved = resolved_in(Some(&doc), url, None);
            assert!(resolved.is_local_file(), "{url}");
            assert_eq!(resolved.to_local_file(), qt, "{url}");
        }
        assert!(!QUrl::parse("http://e/a").is_local_file());
        for (path, qt) in [
            (
                "/abs/50% a#b?c \"d\".png",
                "file:///abs/50%25 a%23b%3Fc %22d%22.png",
            ),
            ("/tmp/probe cwd/", "file:///tmp/probe cwd/"),
            (
                "/a%20b/\u{20ac} \u{436}",
                "file:///a%2520b/\u{20ac} \u{436}",
            ),
            ("/a\tb\u{7f}", "file:///a%09b%7F"),
            ("/a//b/../c", "file:///a//b/../c"),
        ] {
            let url = QUrl::from_local_file(Path::new(path));
            assert_eq!(url.to_string(), qt);
            assert_eq!(QUrl::parse(qt).to_local_file(), path);
        }
    }
}
