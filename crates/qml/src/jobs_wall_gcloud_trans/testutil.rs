//! Shared pieces of the equivalence tests: a local HTTP server standing in for the network,
//! scratch directories, and capture of a job's / a command's stdout, stderr and exit code.
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug)]
pub struct Req {
    pub method: String,
    /// The request-target as sent (absolute when the client talks to a proxy).
    pub target: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Req {
    pub fn header(&self, k: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(k))
            .map(|(_, v)| v.as_str())
    }
}

pub struct Resp {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// One `Transfer-Encoding: chunked` chunk, as Google answers.
    pub chunked: bool,
}

impl Resp {
    pub fn new(status: u16, body: impl Into<Vec<u8>>) -> Resp {
        Resp {
            status,
            headers: vec![],
            body: body.into(),
            chunked: false,
        }
    }
}

pub type Handler = Arc<dyn Fn(&Req) -> Resp + Send + Sync>;

pub struct Server {
    pub port: u16,
    pub reqs: Arc<Mutex<Vec<Req>>>,
}

impl Server {
    pub fn base(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
    pub fn take(&self) -> Vec<Req> {
        std::mem::take(&mut self.reqs.lock().unwrap())
    }
}

pub fn serve(h: impl Fn(&Req) -> Resp + Send + Sync + 'static) -> Server {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let reqs = Arc::new(Mutex::new(vec![]));
    let h: Handler = Arc::new(h);
    let r2 = reqs.clone();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(s) = s else { continue };
            let (h, reqs) = (h.clone(), r2.clone());
            std::thread::spawn(move || {
                let mut rd = BufReader::new(s.try_clone().unwrap());
                let mut line = String::new();
                if rd.read_line(&mut line).unwrap_or(0) == 0 {
                    return;
                }
                let mut it = line.trim_end().splitn(3, ' ');
                let method = it.next().unwrap_or_default().to_string();
                let target = it.next().unwrap_or_default().to_string();
                let mut headers = vec![];
                loop {
                    let mut l = String::new();
                    if rd.read_line(&mut l).unwrap_or(0) == 0 || l.trim_end().is_empty() {
                        break;
                    }
                    if let Some((k, v)) = l.trim_end().split_once(':') {
                        headers.push((k.trim().to_string(), v.trim().to_string()));
                    }
                }
                let n: usize = headers
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                    .and_then(|(_, v)| v.parse().ok())
                    .unwrap_or(0);
                let mut body = vec![0; n];
                let _ = rd.read_exact(&mut body);
                let req = Req {
                    method,
                    target,
                    headers,
                    body,
                };
                let r = h(&req);
                reqs.lock().unwrap().push(req);
                let mut out = format!("HTTP/1.1 {} X\r\nConnection: close\r\n", r.status);
                for (k, v) in &r.headers {
                    out += &format!("{k}: {v}\r\n");
                }
                let mut s = s;
                if r.chunked {
                    out += "Transfer-Encoding: chunked\r\n\r\n";
                    let _ = s.write_all(out.as_bytes());
                    if !r.body.is_empty() {
                        let _ = s.write_all(format!("{:x}\r\n", r.body.len()).as_bytes());
                        let _ = s.write_all(&r.body);
                        let _ = s.write_all(b"\r\n");
                    }
                    let _ = s.write_all(b"0\r\n\r\n");
                } else {
                    out += &format!("Content-Length: {}\r\n\r\n", r.body.len());
                    let _ = s.write_all(out.as_bytes());
                    let _ = s.write_all(&r.body);
                }
            });
        }
    });
    Server { port, reqs }
}

/// A fresh scratch directory.
pub fn tmpdir(tag: &str) -> std::path::PathBuf {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let d = std::env::temp_dir().join(format!(
        "ri-jobs-{tag}-{}-{}",
        std::process::id(),
        N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

pub type Sink<'a> = &'a (dyn Fn(String) + Sync);

/// A job body's stdout, stderr and exit code.
pub fn capture(f: impl FnOnce(Sink, Sink) -> i32) -> (String, String, i32) {
    let o = Mutex::new(String::new());
    let e = Mutex::new(String::new());
    let code = f(&|s| o.lock().unwrap().push_str(&s), &|s| {
        e.lock().unwrap().push_str(&s)
    });
    (o.into_inner().unwrap(), e.into_inner().unwrap(), code)
}

/// A command's stdout, stderr and exit code.
pub fn run(c: &mut std::process::Command) -> (String, String, i32) {
    let o = c.stdin(std::process::Stdio::null()).output().unwrap();
    (
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
        o.status.code().unwrap_or(-1),
    )
}

/// Whether `prog` can be run; fails the test otherwise (`crate::testenv`).
pub fn have(prog: &str) -> bool {
    let ok = crate::osutil::on_path(prog);
    if !ok {
        crate::testenv::unavailable(format!("`{prog}` is not installed"));
    }
    ok
}

/// The repository's QML tree (`qml`).
pub fn tree() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qml")
}

/// The original's scripts, which the shell no longer ships: the reference side of the comparisons.
pub fn unshipped() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/original-unshipped")
}

/// Wall and CPU time (user+sys of the thread, plus children it waited for) of `f` run on its
/// own thread.
pub fn measure_thread(f: impl FnOnce() + Send) -> (f64, f64) {
    std::thread::scope(|s| {
        s.spawn(|| {
            let cpu = || {
                let (u, c) = (rusage(libc::RUSAGE_THREAD), rusage(libc::RUSAGE_CHILDREN));
                tv(u.ru_utime) + tv(u.ru_stime) + tv(c.ru_utime) + tv(c.ru_stime)
            };
            let (t, c) = (std::time::Instant::now(), cpu());
            f();
            (t.elapsed().as_secs_f64(), cpu() - c)
        })
        .join()
        .unwrap()
    })
}

/// Wall and CPU time (user+sys of all waited-for children) of a command.
pub fn measure_cmd(c: &mut std::process::Command) -> (f64, f64) {
    let cpu = || {
        let u = rusage(libc::RUSAGE_CHILDREN);
        tv(u.ru_utime) + tv(u.ru_stime)
    };
    let (t, c0) = (std::time::Instant::now(), cpu());
    let _ = c.stdin(std::process::Stdio::null()).output();
    (t.elapsed().as_secs_f64(), cpu() - c0)
}

/// getrusage(2) for `who`.
fn rusage(who: libc::c_int) -> libc::rusage {
    // SAFETY: rusage is plain integers, for which all zeros is valid.
    let mut r: libc::rusage = unsafe { std::mem::zeroed() };
    // SAFETY: `r` is a live rusage for the kernel to fill.
    unsafe { libc::getrusage(who, &mut r) };
    r
}

fn tv(t: libc::timeval) -> f64 {
    t.tv_sec as f64 + t.tv_usec as f64 / 1e6
}
