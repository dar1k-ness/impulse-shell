//! Jobs of the `Impulse` module's process-like types (`Element::job`): Rust code that runs on a
//! pool thread and reports stdout, stderr and an exit code the way a child process does. A job
//! reads its inputs from the element's QML properties.
use crate::impulse::Element;
use crate::native::ProcEv;
use crate::runtime::World;
use crate::svc::{Sender, Stop};
use crate::value::Value;
use std::path::Path;
use std::sync::Arc;

/// The job's stdout/stderr, delivered like a child process's pipes.
pub struct Pipes<'a>(&'a Sender<ProcEv>);

impl Pipes<'_> {
    pub fn out(&self, s: impl Into<String>) {
        let _ = self.0.send(ProcEv::Out(s.into()));
    }
    pub fn err(&self, s: impl Into<String>) {
        let _ = self.0.send(ProcEv::Err(s.into()));
    }
}

/// Runs on a pool thread; returns the exit code. A job that blocks registers its wakeup
/// with [`Stop::on_stop`] instead of polling the flag.
pub type Job = Box<dyn FnOnce(&Pipes, &Arc<Stop>) -> i32 + Send>;

/// Run `job` on a [`workpool::long`] thread, reporting `Exit` when it returns (or panics).
pub fn start(job: Job, tx: Sender<ProcEv>, stop: Arc<Stop>) {
    workpool::long(move || {
        let code = caught(|| {
            crate::child::with_stop(&stop, || {
                crate::http::with_stop(&stop, || job(&Pipes(&tx), &stop))
            })
        });
        let _ = tx.send(ProcEv::Exit(code));
    });
}

/// A job's exit code, or 1 if it panicked: a crashed job must still end its `Process`
/// (`running` false, `exited` emitted) like a crashed child would.
pub fn caught(f: impl FnOnce() -> i32) -> i32 {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(|e| {
        let msg = e
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| e.downcast_ref::<String>().cloned())
            .unwrap_or_default();
        elog::eprintln!("impulse-shell: native job panicked: {msg}");
        1
    })
}

/// Property `name` of job element `p` as a string (unset: empty).
pub(crate) fn text(w: &mut World, p: usize, name: &str) -> String {
    match w.read(p, name) {
        None | Some(Value::Undefined) | Some(Value::Null) => String::new(),
        Some(v) => v.to_js_string(),
    }
}

/// Property `name` of job element `p` as a bool (unset: false).
pub(crate) fn flag(w: &mut World, p: usize, name: &str) -> bool {
    w.read(p, name).is_some_and(|v| v.truthy())
}

fn ua_header(ua: String) -> Vec<(String, String)> {
    vec![("User-Agent".into(), ua)]
}

/// Fetches `url` into `path` for an image cache unless the file is already there; `Err`
/// says why the file is not there. A body that cannot be the image (an HTTP error page, a captive portal's HTML,
/// an empty reply) is not kept: the original cached it under the image's name, and `[ -f ]` then
/// skipped the download for good, so the favicon or cover stayed broken after the network
/// came back. Such a reply is a failure and the next request downloads again. An
/// image sent with an error status (Google's favicon service answers 404 with its globe
/// icon) is kept.
pub fn fetch_unless_present(
    url: &str,
    headers: &[(String, String)],
    path: &str,
    o: crate::http::Opts,
) -> Result<(), String> {
    fetch_unless_present_fail(url, headers, path, false, o)
}

/// `file`'s description of `path` led by `W x H` when it is an image.
pub fn image_description(path: &Path) -> Option<String> {
    let d = crate::magic::describe(path)?;
    Some(match imagesize::size(path) {
        Ok(s) => format!("{} x {}, {d}", s.width, s.height),
        Err(_) => d,
    })
}

/// `fetch_unless_present`; with `fail` an HTTP error status is a failure whatever its body.
fn fetch_unless_present_fail(
    url: &str,
    headers: &[(String, String)],
    path: &str,
    fail: bool,
    o: crate::http::Opts,
) -> Result<(), String> {
    if Path::new(path).is_file() {
        return Ok(());
    }
    if let Some(rest) = strip_prefix_ci(url, "file://") {
        return copy_file_url(rest, Path::new(path));
    }
    match crate::http::download_max(
        url,
        headers,
        Path::new(path),
        fail,
        o,
        crate::http::IMAGE_MAX,
    ) {
        Ok(status) if cacheable(status, Path::new(path)) => Ok(()),
        Ok(status) => {
            let _ = std::fs::remove_file(path);
            Err(format!("HTTP {status} without an image"))
        }
        Err(e) => Err(e.to_string()),
    }
}

/// `s` after an ASCII `prefix` matched case-insensitively. Never slices inside a character:
/// an MPRIS `artUrl` can start with any text.
fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let head = s.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &s[prefix.len()..])
}

/// Whether a downloaded reply may stay in an image cache.
fn cacheable(status: u16, path: &Path) -> bool {
    if std::fs::metadata(path).map_or(true, |m| m.len() == 0) {
        return false;
    }
    let mime = crate::magic::mime_type(path, true).unwrap_or_default();
    mime.starts_with("image/") || (status < 400 && !mime.starts_with("text/"))
}

/// Copies the file of a `file://` URL (players such as Chrome and mpv publish local art
/// files): host empty or localhost, percent-decoded path. Any session-bus client can publish
/// an `artUrl`: only a regular file is read (a FIFO hung the worker, `/dev/zero` grew the
/// shell until OOM) and at most [`crate::http::IMAGE_MAX`] bytes of it.
fn copy_file_url(rest: &str, dst: &Path) -> Result<(), String> {
    use std::os::unix::ffi::OsStrExt;
    let rest = ["localhost", "127.0.0.1"]
        .iter()
        .find_map(|h| rest.strip_prefix(h).filter(|r| r.starts_with('/')))
        .unwrap_or(rest);
    if !rest.starts_with('/') || rest.bytes().any(|b| b <= b' ') {
        return Err("not a local file URL".into());
    }
    let raw: Vec<u8> = percent_encoding::percent_decode_str(rest).collect();
    let src = Path::new(std::ffi::OsStr::from_bytes(&raw));
    let mut f = crate::osutil::open_regular(src).map_err(|e| e.to_string())?;
    let tmp = crate::http::part_path(dst);
    let res = std::fs::File::create(&tmp)
        .and_then(|mut out| crate::osutil::copy_max(&mut f, &mut out, crate::http::IMAGE_MAX))
        .and_then(|_| std::fs::rename(&tmp, dst));
    res.map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
}

pub static DOWNLOAD: Element = Element {
    job: Some(download),
    ..Element::new("Download")
};

/// `Impulse/Download`: fetches `url` into `path` unless the file is already there, following
/// redirects; `userAgent`, when set, is the `User-Agent` sent (empty: none) and `ipv4`
/// resolves IPv4 addresses only. Exit code 0 when the file is there afterwards, else 1 with the
/// reason on stderr ([`fetch_unless_present`]).
fn download(w: &mut World, p: usize) -> Option<Job> {
    let path = text(w, p, "path");
    let url = text(w, p, "url");
    let headers = match w.read(p, "userAgent") {
        None | Some(Value::Undefined) | Some(Value::Null) => vec![],
        Some(ua) => ua_header(ua.to_js_string()),
    };
    let ipv4 = flag(w, p, "ipv4");
    Some(Box::new(move |io, _| {
        let o = crate::http::Opts {
            follow: true,
            ipv4,
            stall: false,
            answer_within: None,
        };
        match fetch_unless_present(&url, &headers, &path, o) {
            Ok(()) => 0,
            Err(e) => {
                io.err(format!("Download: {url}: {e}\n"));
                1
            }
        }
    }))
}

pub static IMAGE_DOWNLOAD: Element = Element {
    job: Some(image_download),
    ..Element::new("ImageDownload")
};

/// `Impulse/ImageDownload`: fetches the image at `url` into `path` (its directory is made)
/// unless an image is already there, and prints `path: W x H, <description>` (libmagic's).
/// An HTTP error page is not saved, and a file at `path` that is no image is fetched again.
/// `userAgent`, when not empty, is the `User-Agent` sent.
fn image_download(w: &mut World, p: usize) -> Option<Job> {
    let path = crate::jobs_files::trim_file_protocol(&text(w, p, "path")).to_string();
    let url = text(w, p, "url");
    let ua = text(w, p, "userAgent");
    Some(Box::new(move |io, _| download_image(io, &path, &url, &ua)))
}

fn download_image(io: &Pipes, path: &str, url: &str, ua: &str) -> i32 {
    if !image_dir(path) {
        io.err(format!("ImageDownload: no directory for '{path}'\n"));
        return 1;
    }
    if Path::new(path).is_file() && imagesize::size(path).is_err() {
        let _ = std::fs::remove_file(path);
    }
    let headers = if ua.is_empty() {
        vec![]
    } else {
        ua_header(ua.to_string())
    };
    let o = crate::http::Opts {
        follow: true,
        ..Default::default()
    };
    if let Err(e) = fetch_unless_present_fail(url, &headers, path, true, o) {
        io.err(format!("ImageDownload: {url}: {e}\n"));
        return 1;
    }
    image_description(Path::new(path)).map_or(1, |d| {
        io.out(format!("{path}: {d}\n"));
        0
    })
}

/// `mkdir -p $(dirname F)`. In the shared `/tmp` (FloatingImage's `/tmp/rustshell/media/images`)
/// the directory is reached through [`crate::privfs`], and whatever but a regular file sits
/// at `F` (a planted symlink) is removed, never followed.
fn image_dir(path: &str) -> bool {
    let p = Path::new(path);
    if !p.starts_with("/tmp") {
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        return true;
    }
    let Ok((dir, name)) = crate::privfs::Dir::parent_of(p, true) else {
        return false;
    };
    if !dir.has_file(name) {
        dir.remove_all(name);
    }
    true
}

/// `WeatherFetch`: wttr.in's j1 report for `place` (`lat,long` or a `+`-joined city), reduced
/// by [`weather_filter`]. Exit code 0 with the JSON on stdout, else [`crate::weather::FAILED`].
pub fn weather(place: String) -> Job {
    Box::new(move |io, _| {
        let url = format!(
            "https://wttr.in/{}?format=j1",
            crate::http::encode_path(&place)
        );
        let Ok(body) = crate::http::get(&url, &[], Default::default())
            .and_then(|r| Ok(r.bytes_max(crate::http::JSON_MAX)?))
        else {
            return crate::weather::FAILED;
        };
        match weather_filter(&body) {
            Ok(Some(s)) => io.out(s),
            Ok(None) => {}
            Err(e) => {
                io.err(format!("WeatherFetch: {e}\n"));
                return crate::weather::FAILED;
            }
        }
        0
    })
}

/// `formatCityName`: `trim().split(/\s+/).join('+')`.
pub fn weather_city(c: &str) -> String {
    let ws = |ch: char| ch.is_whitespace() || ch == '\u{feff}';
    c.trim_matches(ws)
        .split(ws)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("+")
}

/// `{current: current_condition[0], location: nearest_area[0], astronomy:
/// weather[0].astronomy[0]}` of every JSON value in `input`, pretty-printed; a missing member
/// is `null`. `Err` when `input` is not JSON or a member has another type than the report's.
pub fn weather_filter(input: &[u8]) -> Result<Option<String>, String> {
    use serde_json::Value as J;
    let mut out = String::new();
    for v in serde_json::Deserializer::from_slice(input).into_iter::<J>() {
        let v = v.map_err(|e| e.to_string())?;
        let at = |k: &str| -> Result<J, String> {
            match &v {
                J::Object(m) => Ok(m.get(k).cloned().unwrap_or(J::Null)),
                J::Null => Ok(J::Null),
                _ => Err(format!("the report is {}, not an object", json_type(&v))),
            }
        };
        let first = |a: J| -> Result<J, String> {
            match a {
                J::Array(a) => Ok(a.into_iter().next().unwrap_or(J::Null)),
                J::Null => Ok(J::Null),
                o => Err(format!("expected an array, found {}", json_type(&o))),
            }
        };
        let key = |o: J, k: &str| -> Result<J, String> {
            match o {
                J::Object(mut m) => Ok(m.remove(k).unwrap_or(J::Null)),
                J::Null => Ok(J::Null),
                o => Err(format!("\"{k}\" is in {}, not in an object", json_type(&o))),
            }
        };
        let current = first(at("current_condition")?)?;
        let location = first(at("nearest_area")?)?;
        let astronomy = first(key(first(at("weather")?)?, "astronomy")?)?;
        let mut m = serde_json::Map::new();
        m.insert("current".into(), current);
        m.insert("location".into(), location);
        m.insert("astronomy".into(), astronomy);
        out += &serde_json::to_string_pretty(&J::Object(m)).map_err(|e| e.to_string())?;
        out.push('\n');
    }
    Ok((!out.is_empty()).then_some(out))
}

fn json_type(v: &serde_json::Value) -> &'static str {
    use serde_json::Value as J;
    match v {
        J::Null => "null",
        J::Bool(_) => "boolean",
        J::Number(_) => "number",
        J::String(_) => "string",
        J::Array(_) => "array",
        J::Object(_) => "object",
    }
}

#[cfg(test)]
mod url_tests {
    /// Any session-bus client can claim to be a player and publish `file:///dev/zero`, a FIFO
    /// or a huge file as its cover: each is refused at once, leaving no file.
    #[test]
    fn file_art_url_reads_only_bounded_regular_files() {
        let t = tempfile::tempdir().unwrap();
        let fifo = t.path().join("fifo");
        let c = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
        // SAFETY: `c` is a live NUL-terminated path.
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        let big = t.path().join("big");
        std::fs::File::create(&big)
            .unwrap()
            .set_len(crate::http::IMAGE_MAX + 1)
            .unwrap();
        let ok = t.path().join("ok");
        std::fs::write(&ok, b"art").unwrap();
        let dst = t.path().join("out/cover");
        std::fs::create_dir_all(dst.parent().unwrap()).unwrap();
        for src in [
            fifo.to_str().unwrap(),
            "/dev/zero",
            t.path().to_str().unwrap(),
            big.to_str().unwrap(),
        ] {
            assert!(super::copy_file_url(src, &dst).is_err(), "{src}");
            assert_eq!(
                std::fs::read_dir(dst.parent().unwrap()).unwrap().count(),
                0,
                "{src}"
            );
        }
        assert_eq!(super::copy_file_url(ok.to_str().unwrap(), &dst), Ok(()));
        assert_eq!(std::fs::read(&dst).unwrap(), b"art");
    }

    #[test]
    fn file_prefix_never_splits_a_character() {
        assert_eq!(
            super::strip_prefix_ci("FILE:///a/b", "file://"),
            Some("/a/b")
        );
        assert_eq!(super::strip_prefix_ci("file:/", "file://"), None);
        // Byte 7 falls inside the fourth Cyrillic letter.
        assert_eq!(super::strip_prefix_ci("абвгд", "file://"), None);
    }

    #[test]
    fn non_ascii_art_url_fails_without_panicking() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cover");
        let res = super::fetch_unless_present_fail(
            "абвгд/cover.png",
            &[],
            path.to_str().unwrap(),
            false,
            Default::default(),
        );
        assert!(res.is_err());
        assert!(!path.exists());
    }
}

#[cfg(test)]
mod start_tests {
    use crate::native::ProcEv;

    #[test]
    fn panicking_job_still_exits() {
        let (tx, rx) = crate::svc::Waker::default()
            .handle(crate::svc::Svc::Process)
            .channel();
        super::start(
            Box::new(|io, _| {
                io.out("before");
                panic!("bad frame")
            }),
            tx,
            Default::default(),
        );
        let mut exit = None;
        while let Ok(ev) = rx.recv() {
            if let ProcEv::Exit(c) = ev {
                exit = Some(c);
                break;
            }
        }
        assert_eq!(exit, Some(1), "a panicking job must still report Exit");
    }
}

#[cfg(test)]
mod image_downloader_tests {
    use std::os::unix::fs::PermissionsExt;

    // FloatingImage's `/tmp/rustshell/media/images` is shared: a symlink planted where the
    // image goes is replaced, never read or written through, and the directory is private.
    #[test]
    fn temp_image_does_not_follow_symlinks() {
        let t = tempfile::Builder::new().tempdir_in("/tmp").unwrap();
        let victim = t.path().join("victim.png");
        let victim_png = include_bytes!("../../imgtools/tests/fixtures/magick/bilevel.png");
        std::fs::write(&victim, victim_png).unwrap();
        let src = t.path().join("src.png");
        let src_png =
            include_bytes!("../../imgtools/tests/fixtures/magick/golden/onepx.png.128.png");
        std::fs::write(&src, src_png).unwrap();
        let dir = t.path().join("media/images");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o777)).unwrap();
        let p = dir.join("0123abcd");
        std::os::unix::fs::symlink(&victim, &p).unwrap();
        let (tx, rx) = crate::svc::Waker::default()
            .handle(crate::svc::Svc::Process)
            .channel();
        let url = format!("file://{}", src.display());
        let code = super::download_image(&super::Pipes(&tx), p.to_str().unwrap(), &url, "");
        let err: Vec<String> = rx
            .try_iter()
            .filter_map(|e| match e {
                crate::native::ProcEv::Err(s) => Some(s),
                _ => None,
            })
            .collect();
        assert_eq!(code, 0, "{err:?}");
        assert!(!std::fs::symlink_metadata(&p).unwrap().is_symlink());
        assert_eq!(std::fs::read(&p).unwrap(), src_png);
        assert_eq!(std::fs::read(&victim).unwrap(), victim_png);
        assert_eq!(
            std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
}
