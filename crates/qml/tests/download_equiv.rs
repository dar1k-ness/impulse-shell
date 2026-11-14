//! Whole-pipeline A/B of the in-process download jobs against the command lines the original ran, on the
//! same local HTTP server: exit code, stdout, the files left and the requests made match.
//! Favicon.qml's `faviconDownloadProcess` and ImageDownloaderProcess.qml are `Impulse` types
//! (`Download`, `ImageDownload`): their reference side is a `Process` with the command line the original
//! had there (real bash/curl/mkdir/file), kept in this file. Weather.qml's `fetcher` runs once
//! as the command line the original had (tests/fixtures/original) and once as `WeatherFetch`.
#[path = "support/closed_port.rs"]
mod closed_port;
mod event_loop;

use closed_port::closed_port;
use qml::runtime::Runtime;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// The runs share one HTTP server and output folder: one run at a time.
static ENV: Mutex<()> = Mutex::new(());

fn ri() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../qml")
}

fn wallpaper(f: &str) -> Vec<u8> {
    std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../colorgen/tests/switchwall/fixture/home/Pictures/Wallpapers")
            .join(f),
    )
    .unwrap()
}

fn tmpdir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("download-equiv-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn tools(list: &[&str]) -> bool {
    list.iter().all(|t| testenv::require(t))
}

const WTTR: &str = r#"{"current_condition":[{"FeelsLikeC":"10","temp_C":"12","precipMM":1.50,"big":100000000000000000001,"weatherDesc":[{"value":"Ciel d\u00e9gag\u00e9 </>"}],"uvIndex":"3"}],"nearest_area":[{"areaName":[{"value":"Paris"}],"latitude":"48.857","longitude":"2.352"}],"request":[{"query":"x"}],"weather":[{"astronomy":[{"moon_phase":"Waxing Crescent","sunrise":"07:41 AM","sunset":"05:24 PM"}],"date":"2026-11-01"}]}"#;

/// The requests the fake server got, `(target, User-Agent)`, each recorded before it is
/// answered: in memory, or appended to a file (a JSON pair per line) that a test running in a
/// process of its own reads.
#[derive(Clone)]
enum Seen {
    Memory(Arc<Mutex<Vec<(String, String)>>>),
    File(PathBuf),
}

impl Seen {
    fn push(&self, r: (String, String)) {
        match self {
            Seen::Memory(m) => m.lock().unwrap().push(r),
            Seen::File(p) => {
                let line = format!("{}\n", serde_json::to_string(&r).unwrap());
                std::fs::OpenOptions::new()
                    .append(true)
                    .open(p)
                    .unwrap()
                    .write_all(line.as_bytes())
                    .unwrap();
            }
        }
    }

    fn clear(&self) {
        match self {
            Seen::Memory(m) => m.lock().unwrap().clear(),
            Seen::File(p) => std::fs::write(p, "").unwrap(),
        }
    }

    fn all(&self) -> Vec<(String, String)> {
        match self {
            Seen::Memory(m) => m.lock().unwrap().clone(),
            Seen::File(p) => std::fs::read_to_string(p)
                .unwrap()
                .lines()
                .map(|l| serde_json::from_str(l).unwrap())
                .collect(),
        }
    }
}

fn server() -> (String, Seen) {
    let seen = Seen::Memory(Default::default());
    (server_to(seen.clone()), seen)
}

/// The fake server on a thread of this process, recording to `seen`; its address.
fn server_to(seen: Seen) -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let s2 = seen;
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            let seen = s2.clone();
            std::thread::spawn(move || {
                let mut r = BufReader::new(s.try_clone().unwrap());
                let mut line = String::new();
                r.read_line(&mut line).unwrap_or(0);
                let target = line.split_whitespace().nth(1).unwrap_or("").to_string();
                let mut ua = String::new();
                loop {
                    let mut h = String::new();
                    if r.read_line(&mut h).unwrap_or(0) == 0 || h == "\r\n" {
                        break;
                    }
                    if let Some((k, v)) = h.trim_end().split_once(": ")
                        && k.eq_ignore_ascii_case("user-agent")
                    {
                        ua = v.to_string();
                    }
                }
                if line.starts_with("CONNECT ") {
                    // https: refused, so nothing leaves the machine.
                    seen.push((format!("CONNECT {target}"), String::new()));
                    let mut w = s;
                    let _ = write!(w, "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n");
                    return;
                }
                let (status, body): (&str, Vec<u8>) =
                    if let Some(path) = target.strip_prefix("http://wttr.in") {
                        wttr_reply(path)
                    } else {
                        match target.split('?').next().unwrap() {
                            "/ok" => ("200 OK", b"\x00\x00\x01\x00favicon".to_vec()),
                            "/png" => ("200 OK", wallpaper("syn_small.png")),
                            "/jpg" => ("200 OK", wallpaper("syn_grad.jpg")),
                            "/gif" => ("200 OK", wallpaper("syn_anim.gif")),
                            "/redir" => ("302 Found", Vec::new()),
                            "/err" => ("500 Internal Server Error", b"boom".to_vec()),
                            _ => ("404 Not Found", b"<html>not found</html>\n".to_vec()),
                        }
                    };
                seen.push((target.clone(), ua));
                let mut w = s;
                let loc = if status.starts_with("302") {
                    "Location: /png\r\n"
                } else {
                    ""
                };
                let _ = write!(
                    w,
                    "HTTP/1.1 {status}\r\n{loc}Content-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = w.write_all(&body);
            });
        }
    });
    base
}

/// wttr.in's reply to `path` (`/<where>?format=j1`) on the fake server.
fn wttr_reply(path: &str) -> (&'static str, Vec<u8>) {
    if path.starts_with("/Paris?") || path.contains("48.85") {
        ("200 OK", WTTR.into())
    } else if path.contains("Lyon") {
        ("200 OK", b"{}".to_vec())
    } else if path.contains("York") {
        (
            "404 Not Found",
            b"Unknown location; please try ~40.7,-74.0".to_vec(),
        )
    } else if path.contains("Nowhere") {
        ("200 OK", Vec::new())
    } else {
        ("200 OK", b"oops, not json".to_vec())
    }
}

/// The exit code of a native download whose original command line exited `code`: 0, or 1 for
/// any failure (the native elements do not speak curl's exit codes).
fn failed(code: i32) -> i32 {
    i32::from(code != 0)
}

#[derive(Clone, Debug, PartialEq)]
struct Outcome {
    code: i32,
    out: String,
    files: Vec<(String, Vec<u8>)>,
    requests: Vec<(String, String)>,
}

fn tree(d: &Path) -> Vec<(String, Vec<u8>)> {
    let mut v = Vec::new();
    let mut stack = vec![d.to_path_buf()];
    while let Some(p) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&p) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            let rel = p.strip_prefix(d).unwrap().to_string_lossy().into_owned();
            if p.is_dir() {
                v.push((format!("{rel}/"), Vec::new()));
                stack.push(p);
            } else {
                v.push((rel, std::fs::read(&p).unwrap()));
            }
        }
    }
    v.sort();
    v
}

/// Loads `qml` (whose root has `code`, `out`, `done`), runs it to exit; `files` is what it left
/// under `out_dir` (seeded by `seed`), with `out_dir` replaced by `@D@` in stdout.
fn run(
    qml_file: &Path,
    with_ri: bool,
    out_dir: &Path,
    seed: &dyn Fn(&Path),
    seen: &Seen,
) -> Outcome {
    let _ = std::fs::remove_dir_all(out_dir);
    std::fs::create_dir_all(out_dir).unwrap();
    seed(out_dir);
    seen.clear();
    let mut rt = Runtime::new(with_ri.then(ri));
    let mut lp = event_loop::Loop::new(&mut rt);
    if with_ri {
        let dirs: Vec<PathBuf> = [
            "modules/common",
            "modules/common/functions",
            "modules/common/utils",
        ]
        .iter()
        .map(|d| ri().join(d))
        .collect();
        rt.load_singletons_where(&dirs, &|n| {
            ["Directories", "FileUtils", "StringUtils"].contains(&n)
        });
    }
    let root = rt.load_file(qml_file).unwrap();
    rt.settle();
    let read = |rt: &Runtime, k: &str| rt.world.borrow_mut().read(root, k).unwrap();
    lp.block_until(&mut rt, |rt| {
        read(rt, "code").to_number() != -1.0 && read(rt, "done").truthy()
    });
    let code = read(&rt, "code").to_number() as i32;
    assert_ne!(code, -1, "{} did not exit", qml_file.display());
    // Where the original sets no `-A`, each client sends its own default UA (curl's / ureq's).
    let mut requests: Vec<_> = seen
        .all()
        .iter()
        .map(|(t, ua)| {
            let default = ua.starts_with("curl/") || ua.starts_with("ureq/");
            // curl sends a URL's non-ASCII bytes raw; the port percent-encodes them (RFC 3986).
            let t: String = t
                .bytes()
                .map(|b| match b {
                    0x80.. => format!("%{b:02X}"),
                    _ => char::from(b).to_string(),
                })
                .collect();
            (
                t,
                if default {
                    "<default>".into()
                } else {
                    ua.clone()
                },
            )
        })
        .collect();
    requests.sort();
    Outcome {
        code,
        out: read(&rt, "out")
            .to_js_string()
            .replace(out_dir.to_str().unwrap(), "@D@"),
        files: tree(out_dir),
        requests,
    }
}

/// Runs `original_file` with the original pipeline and `native_file` with the native one;
/// native must give
/// `intended(original)`: the result the original means to give where the original has a flaw the port
/// fixes.
#[allow(clippy::too_many_arguments)]
fn ab_with(
    tag: &str,
    original_file: &Path,
    native_file: &Path,
    with_ri: bool,
    out_dir: &Path,
    seed: &dyn Fn(&Path),
    seen: &Seen,
    intended: &dyn Fn(&Outcome) -> Outcome,
) -> Outcome {
    ab_files(
        tag,
        [original_file, native_file],
        with_ri,
        out_dir,
        seed,
        seen,
        intended,
    )
}

/// [`ab_with`] with the original pipeline in `files[0]` and the native one in `files[1]`.
fn ab_files(
    tag: &str,
    files: [&Path; 2],
    with_ri: bool,
    out_dir: &Path,
    seed: &dyn Fn(&Path),
    seen: &Seen,
    intended: &dyn Fn(&Outcome) -> Outcome,
) -> Outcome {
    let original = run(files[0], with_ri, out_dir, seed, seen);
    let want = intended(&original);
    let got = run(files[1], with_ri, out_dir, seed, seen);
    if got != want {
        let sum = |o: &Outcome| {
            let files: Vec<_> = o.files.iter().map(|(n, b)| (n.clone(), b.len())).collect();
            format!(
                "code {} out {:?} requests {:?} files {files:?}",
                o.code, o.out, o.requests
            )
        };
        let first = want
            .files
            .iter()
            .zip(&got.files)
            .find_map(|((n, a), (_, b))| {
                (a != b).then(|| (n.clone(), a.iter().zip(b).position(|(x, y)| x != y)))
            });
        panic!(
            "{tag}: native vs original\n  native: {}\noriginal: {}\nfirst differing file/byte: {first:?}",
            sum(&got),
            sum(&want)
        );
    }
    original
}

/// `qs` / JS string literal.
fn lit(s: &str) -> String {
    serde_json::to_string(s).unwrap()
}

/// The `Process { id: <id> … }` command line in the original's source (or the file at an absolute path),
/// verbatim.
fn ri_command(file: &str, id: &str) -> String {
    ri_lines(file, id, &["command"])
}

/// The lines setting `props` in the object `id: <id>` of the original's source, verbatim.
fn ri_lines(file: &str, id: &str, props: &[&str]) -> String {
    let src = std::fs::read_to_string(ri().join(file)).unwrap();
    let at = src.find(&format!("id: {id}")).unwrap();
    props
        .iter()
        .map(|p| {
            src[at..]
                .lines()
                .find(|l| l.trim_start().starts_with(&format!("{p}:")))
                .unwrap_or_else(|| panic!("{file}#{id}: no {p}"))
                .trim()
        })
        .collect::<Vec<_>>()
        .join("\n        ")
}

/// The command line the original ran as Favicon.qml's `faviconDownloadProcess`.
const FAVICON_COMMAND: &str = r#"command: ["bash", "-c", `[ -f ${faviconFilePath} ] || curl -s '${root.faviconUrl}' -o '${faviconFilePath}' -L -H 'User-Agent: ${downloadUserAgent}'`]"#;

const COLLECT: &str = r#"stdout: StdioCollector { onStreamFinished: { root.out = text; root.done = true } }
        onExited: (exitCode, exitStatus) => { root.code = exitCode }"#;

#[test]
fn favicon_download_matches_the_original_command() {
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    if !tools(&["bash", "curl"]) {
        return;
    }
    let (base, seen) = server();
    let d = tmpdir("favicon");
    let out = d.join("favicons");
    let qml_dir = d.join("rusty-impulse/modules/common/widgets");
    std::fs::create_dir_all(&qml_dir).unwrap();
    let props = ri_lines(
        "modules/common/widgets/Favicon.qml",
        "faviconDownloadProcess",
        &["url", "path", "userAgent"],
    );
    // Kept to the end of the test, so the port stays closed for both sides.
    let closed = closed_port();
    let dead = closed.url();
    let cases: Vec<(&str, String, &str, bool)> = vec![
        (
            "ok",
            format!("{base}/ok?domain=github.com&sz=32"),
            "Mozilla/5.0 (X11; Linux x86_64) ii",
            false,
        ),
        ("redirect", format!("{base}/redir"), "ua", false),
        ("missing", format!("{base}/missing?domain=x"), "", false),
        ("server error", format!("{base}/err"), "ua", false),
        ("cached", format!("{base}/ok"), "ua", true),
        ("refused", format!("{dead}/ok"), "ua", false),
    ];
    for (tag, url, ua, cached) in cases {
        let file = out.join("github.com.ico");
        let src = |ty: &str, body: &str| {
            format!(
                r#"import QtQuick
import Rustshell.Io
import Impulse
Item {{
    id: root
    property int code: -1
    property string out: ""
    property bool done: false
    property string downloadUserAgent: {}
    property string faviconUrl: {}
    property string faviconFilePath: {}
    {ty} {{
        id: faviconDownloadProcess
        running: true
        {body}
        {COLLECT}
    }}
}}
"#,
                lit(ua),
                lit(&url),
                lit(file.to_str().unwrap()),
            )
        };
        let original = qml_dir.join("FaviconCommand.qml");
        std::fs::write(&original, src("Process", FAVICON_COMMAND)).unwrap();
        let f = qml_dir.join("Favicon.qml");
        std::fs::write(&f, src("Download", &props)).unwrap();
        let seed = |o: &Path| {
            if cached {
                std::fs::write(o.join("github.com.ico"), "old").unwrap();
            }
        };
        // An HTTP error page is not kept as the icon (the original cached it, and `[ -f ]` then never
        // retried): a failure, no file. A failure exits 1 whatever curl's code for it was.
        let error_page = matches!(tag, "missing" | "server error");
        ab_with(
            &format!("favicon {tag}"),
            &original,
            &f,
            false,
            &out,
            &seed,
            &seen,
            &|o| {
                if !error_page {
                    return Outcome {
                        code: failed(o.code),
                        ..o.clone()
                    };
                }
                Outcome {
                    code: 1,
                    out: String::new(),
                    files: o
                        .files
                        .iter()
                        .filter(|(n, _)| n != "github.com.ico")
                        .cloned()
                        .collect(),
                    requests: o.requests.clone(),
                }
            },
        );
    }
}

/// ImageDownloaderProcess.qml as the original had it: a `Process` with its command line.
const IMAGE_DOWNLOADER_COMMAND: &str = r#"import QtQuick
import Rustshell
import Rustshell.Io
import ri.modules.common
import ri.modules.common.functions

Process {
    id: root

    signal done(string path, int width, int height);
    required property string filePath;
    required property string sourceUrl;
    property string downloadUserAgent: Config.options?.networking.userAgent ?? ""
    
    function processFilePath() {
        return StringUtils.shellSingleQuoteEscape(FileUtils.trimFileProtocol(filePath));
    }

    function processSourceUrl() {
        return StringUtils.shellSingleQuoteEscape(sourceUrl);
    }

    function curlUserAgentArg() {
        if (!downloadUserAgent) {
            return "";
        }
        return ` -H 'User-Agent: ${StringUtils.shellSingleQuoteEscape(downloadUserAgent)}'`;
    }

    running: true
    command: ["bash", "-c", 
        `mkdir -p $(dirname '${processFilePath()}'); [ -f '${processFilePath()}' ] || curl -sSL '${processSourceUrl()}'${curlUserAgentArg()} -o '${processFilePath()}' && file '${processFilePath()}'`
    ]
    stdout: StdioCollector {
        id: imageSizeOutputCollector
        onStreamFinished: {
            const output = imageSizeOutputCollector.text.trim();
            const match = output.match(/(\d+)\s*x\s*(\d+)/);

            if (match) {
                const width = Number(match[1]);
                const height = Number(match[2]);
                root.done(root.filePath, width, height);
            }
        }
    }
}
"#;

#[test]
fn image_downloader_matches_the_original_command() {
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    if !tools(&["bash", "curl", "mkdir", "dirname", "file"]) {
        return;
    }
    let (base, seen) = server();
    let d = tmpdir("imgdl");
    let out = d.join("out");
    // Kept to the end of the test, so the port stays closed for both sides.
    let closed = closed_port();
    let dead = closed.url();
    let o = out.to_str().unwrap();
    // The file each case leaves (the original's curl has no `-f`: an error page is saved like an image),
    // known here: both sides downloading nothing (a server that
    // does not answer) agree with each other and not with this.
    let (png, jpg, gif) = (
        wallpaper("syn_small.png"),
        wallpaper("syn_grad.jpg"),
        wallpaper("syn_anim.gif"),
    );
    // The cached case's file is there before the download and stays.
    let kept = wallpaper("duo-dark.png");
    type Case<'a> = (
        &'a str,
        String,
        String,
        Option<&'a str>,
        bool,
        Option<&'a [u8]>,
    );
    let cases: Vec<Case> = vec![
        (
            "png",
            format!("{base}/png"),
            format!("{o}/a/b/img.png"),
            Some("Mozilla/5.0 ii"),
            false,
            Some(&png),
        ),
        (
            "jpg file://",
            format!("{base}/jpg"),
            format!("file://{o}/c/it's.jpg"),
            Some("it's ua"),
            false,
            Some(&jpg),
        ),
        (
            "gif no ua",
            format!("{base}/gif"),
            format!("{o}/anim.gif"),
            None,
            false,
            Some(&gif),
        ),
        (
            "empty ua",
            format!("{base}/png"),
            format!("{o}/e.png"),
            Some(""),
            false,
            Some(&png),
        ),
        (
            "redirect",
            format!("{base}/redir"),
            format!("{o}/r.png"),
            None,
            false,
            Some(&png),
        ),
        (
            "missing",
            format!("{base}/missing"),
            format!("{o}/m.png"),
            None,
            false,
            Some(b"<html>not found</html>\n"),
        ),
        (
            "server error",
            format!("{base}/err"),
            format!("{o}/s.png"),
            None,
            false,
            Some(b"boom"),
        ),
        (
            "cached",
            format!("{base}/png"),
            format!("{o}/cached.png"),
            None,
            true,
            Some(&kept),
        ),
        (
            "refused",
            format!("{dead}/png"),
            format!("{o}/x/refused.png"),
            None,
            false,
            None,
        ),
    ];
    for (tag, url, path, ua, cached, want) in cases {
        let ua_prop = ua.map_or(String::new(), |u| format!("downloadUserAgent: {}", lit(u)));
        let src = |ty: &str| {
            format!(
                r#"import QtQuick
import Rustshell.Io
import ri.modules.common.utils
Item {{
    id: root
    property int code: -1
    property string out: ""
    property bool done: false
    {ty} {{
        filePath: {}
        sourceUrl: {}
        {ua_prop}
        {COLLECT}
    }}
}}
"#,
                lit(&path),
                lit(&url),
            )
        };
        std::fs::write(
            d.join("ImageDownloaderCommand.qml"),
            IMAGE_DOWNLOADER_COMMAND,
        )
        .unwrap();
        let original = d.join("AbCommand.qml");
        std::fs::write(&original, src("ImageDownloaderCommand")).unwrap();
        let f = d.join("Ab.qml");
        std::fs::write(&f, src("ImageDownloaderProcess")).unwrap();
        let seed = |o: &Path| {
            if cached {
                std::fs::write(o.join("cached.png"), wallpaper("duo-dark.png")).unwrap();
            }
        };
        let ran = ab_with(
            &format!("image downloader {tag}"),
            &original,
            &f,
            true,
            &out,
            &seed,
            &seen,
            &image_intended,
        );
        let name = path.trim_start_matches("file://");
        let name = name.strip_prefix(&format!("{o}/")).unwrap();
        let file = ran.files.iter().find(|(n, _)| n == name);
        assert_eq!(
            (ran.code == 0, file.map(|(_, b)| b.as_slice())),
            (want.is_some(), want),
            "image downloader {tag}: whether it succeeded, and the file it left"
        );
    }
}

/// The last `W x H` in `file`'s description (`100 x 60`; JPEG's `640x400` after `density 1x1`).
fn file_size(desc: &str) -> Option<(u32, u32)> {
    let b = desc.as_bytes();
    (0..b.len()).rev().find_map(|i| {
        if i > 0 && b[i - 1].is_ascii_digit() {
            return None;
        }
        let num = |at: usize| {
            let n = b[at..].iter().take_while(|c| c.is_ascii_digit()).count();
            (n > 0).then(|| (desc[at..at + n].parse::<u32>().ok(), at + n))
        };
        let (w, mut at) = num(i)?;
        at += b[at..].iter().take_while(|&&c| c == b' ').count();
        (b.get(at) == Some(&b'x')).then_some(())?;
        at += 1;
        at += b[at..].iter().take_while(|&&c| c == b' ').count();
        let (h, _) = num(at)?;
        Some((w?, h?))
    })
}

/// ImageDownloaderProcess as intended: `file`'s line led by `W x H`, so the QML size regex
/// finds the image size whatever the format (`file` writes JPEG's as `WxH` after
/// `precision 8`, a figure the regex mistakes for it). A body that is no image (an HTTP
/// error page) is not kept under the image's name: exit 1, no file, no output. A failure exits
/// 1 whatever the code of the original's command line was.
fn image_intended(o: &Outcome) -> Outcome {
    let Some((path, desc)) = o.out.trim_end().split_once(": ") else {
        return Outcome {
            code: failed(o.code),
            ..o.clone()
        };
    };
    match file_size(desc) {
        Some((w, h)) => Outcome {
            out: format!("{path}: {w} x {h}, {desc}\n"),
            ..o.clone()
        },
        None => {
            let name = path.trim_start_matches("@D@/").to_string();
            Outcome {
                code: 1,
                out: String::new(),
                files: o
                    .files
                    .iter()
                    .filter(|(n, _)| *n != name)
                    .cloned()
                    .collect(),
                requests: o.requests.clone(),
            }
        }
    }
}

/// A QML function's source, verbatim.
fn ri_function(src: &str, name: &str) -> String {
    let at = src.find(&format!("function {name}(")).unwrap();
    let mut depth = 0;
    for (i, c) in src[at..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return src[at..at + i + 1].to_string();
                }
            }
            _ => {}
        }
    }
    panic!("{name}")
}

#[test]
fn weather_fetch_matches_the_original_command() {
    const TEST: &str = concat!(
        module_path!(),
        "::weather_fetch_matches_the_original_command"
    );
    if !tools(&["bash", "curl", "jq"]) {
        return;
    }
    // The fake server is the proxy of both sides, the original's curl and the native fetch, which read it
    // from their environment: the test runs in a process of its own that has it from the start,
    // and the server runs here, logging the requests to a file that process reads.
    if !testenv::in_own_process(TEST) {
        let d = tempfile::tempdir().unwrap();
        let log = d.path().join("requests");
        std::fs::write(&log, "").unwrap();
        let base = server_to(Seen::File(log.clone()));
        testenv::run_in_own_process(
            TEST,
            [
                ("http_proxy", base.clone().into()),
                ("https_proxy", base.into()),
                ("DOWNLOAD_EQUIV_REQUESTS", log.into_os_string()),
            ],
        );
        return;
    }
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let seen = Seen::File(std::env::var_os("DOWNLOAD_EQUIV_REQUESTS").unwrap().into());
    let d = tmpdir("weather");
    let out = d.join("out");
    // The original side is the original's Weather.qml before `WeatherFetch`, the native side ours.
    let original_file =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/original/services/Weather.qml");
    let sides = [original_file.clone(), ri().join("services/Weather.qml")].map(|f| {
        let src = std::fs::read_to_string(f).unwrap();
        (
            ri_function(&src, "getData"),
            ri_function(&src, "formatCityName"),
        )
    });
    let fetcher_cmd = ri_command(&original_file.to_string_lossy(), "fetcher");
    let fetchers = [
        format!("Process {{\n        id: fetcher\n        {fetcher_cmd}"),
        "WeatherFetch {\n        id: fetcher".to_string(),
    ];
    for (tag, city, gps, loc) in [
        (
            "city",
            "Paris",
            false,
            "({ valid: false, lat: 0, long: 0 })",
        ),
        (
            "spaced city",
            "  New   York ",
            false,
            "({ valid: false, lat: 0, long: 0 })",
        ),
        (
            "non-json",
            "São Paulo",
            false,
            "({ valid: false, lat: 0, long: 0 })",
        ),
        (
            "empty object",
            "Lyon",
            false,
            "({ valid: false, lat: 0, long: 0 })",
        ),
        (
            "empty body",
            "Nowhere",
            false,
            "({ valid: false, lat: 0, long: 0 })",
        ),
        (
            "gps",
            "Lyon",
            true,
            "({ valid: true, lat: 48.8566, long: 2.3522 })",
        ),
        (
            "gps invalid",
            "Paris",
            true,
            "({ valid: false, lat: 1, long: 2 })",
        ),
    ] {
        let src = |(get_data, format_city): &(String, String), fetcher: &str| {
            format!(
                r#"import QtQuick
import Rustshell.Io
import Impulse
Item {{
    id: root
    property int code: -1
    property string out: ""
    property bool done: false
    property string city: {}
    property bool gpsActive: {gps}
    property var location: {loc}
    {get_data}
    {format_city}
    Component.onCompleted: getData()
    {fetcher}
        {COLLECT}
    }}
}}
"#,
                lit(city),
            )
        };
        let files = ["o", "n"].map(|side| d.join(side).join("rusty-impulse/services/Weather.qml"));
        for (i, f) in files.iter().enumerate() {
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, src(&sides[i], &fetchers[i])).unwrap();
        }
        // The port fetches https://wttr.in (the original's plain-http curl leaks the location); the
        // proxy refuses the CONNECT, so native's own request is the only difference, and its
        // URL and filter are checked against what the original fetched and printed. A failed
        // fetch exits `weather::FAILED` so Weather retries, where the original takes it as empty data.
        let original = ab_files(
            &format!("weather {tag}"),
            [&files[0], &files[1]],
            false,
            &out,
            &|_| {},
            &seen,
            &|o| Outcome {
                code: qml::weather::FAILED,
                out: String::new(),
                files: o.files.clone(),
                requests: vec![("CONNECT wttr.in:443".into(), String::new())],
            },
        );
        let [(url, _)] = &original.requests[..] else {
            panic!("weather {tag}: {:?}", original.requests)
        };
        let path = url.strip_prefix("http://wttr.in").unwrap();
        let place = if gps && loc.contains("valid: true") {
            "48.8566,2.3522".to_string()
        } else {
            qml::jobs::weather_city(city)
        };
        assert_eq!(
            path,
            format!("/{}?format=j1", qml::http::encode_path(&place)),
            "weather {tag}"
        );
        let (status, body) = wttr_reply(path);
        let filtered = match qml::jobs::weather_filter(&body) {
            Ok(s) => (0, s.unwrap_or_default()),
            Err(_) => (1, String::new()),
        };
        assert_eq!(
            filtered,
            (failed(original.code), original.out.clone()),
            "weather {tag} ({status})"
        );
    }
}
