//! Equivalence of the in-process HTTP code (`qml::http`, `qml::jobs`) with the curl / jq /
//! file command lines it replaces, against a local HTTP server. Tests needing a missing
//! tool fail (`testenv`).
mod event_loop;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::process::Command;

fn have(tool: &str) -> bool {
    let ok = Command::new("sh")
        .args(["-c", &format!("command -v {tool}")])
        .output()
        .is_ok_and(|o| o.status.success());
    if !ok {
        testenv::unavailable(format!("{tool} not installed"));
    }
    ok
}

/// A server answering every route below; returns its base URL.
fn server() -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            std::thread::spawn(move || {
                let mut r = BufReader::new(s.try_clone().unwrap());
                let mut line = String::new();
                r.read_line(&mut line).unwrap();
                let mut parts = line.split_whitespace();
                let (method, path) = (
                    parts.next().unwrap_or("").to_string(),
                    parts.next().unwrap_or("").to_string(),
                );
                let mut headers = vec![];
                let mut len = 0;
                loop {
                    let mut h = String::new();
                    if r.read_line(&mut h).unwrap_or(0) == 0 || h == "\r\n" {
                        break;
                    }
                    let (k, v) = h.trim_end().split_once(": ").unwrap_or((h.trim_end(), ""));
                    if k.eq_ignore_ascii_case("content-length") {
                        len = v.parse().unwrap_or(0);
                    }
                    // Client-specific headers differ by design (curl's vs ureq's defaults).
                    if ![
                        "user-agent",
                        "accept",
                        "accept-encoding",
                        "host",
                        "content-length",
                    ]
                    .contains(&k.to_ascii_lowercase().as_str())
                    {
                        headers.push(format!("{}={v}", k.to_ascii_lowercase()));
                    }
                }
                headers.sort();
                let mut body = vec![0; len];
                r.read_exact(&mut body).unwrap();
                let (code, extra, out): (u16, String, Vec<u8>) = match path.as_str() {
                    "/ok" => (200, String::new(), "héllo\nworld".into()),
                    "/redir" => (302, "Location: /ok\r\n".into(), vec![]),
                    "/redir3" => (301, "Location: /redir\r\n".into(), "moved".into()),
                    "/missing" => (404, String::new(), "<h1>not found</h1>".into()),
                    "/err" => (500, String::new(), vec![]),
                    "/jpg" => (200, String::new(), JPEG.to_vec()),
                    "/png" => (200, String::new(), PNG.to_vec()),
                    "/png404" => (404, String::new(), PNG.to_vec()),
                    "/empty" => (200, String::new(), vec![]),
                    "/bin" => (
                        200,
                        String::new(),
                        (0..=255u8).cycle().take(300_000).collect(),
                    ),
                    "/echo" => (
                        201,
                        String::new(),
                        format!(
                            "{method} {}\n{}",
                            headers.join("&"),
                            String::from_utf8_lossy(&body)
                        )
                        .into(),
                    ),
                    _ => (404, String::new(), vec![]),
                };
                let mut s = s;
                let _ = write!(
                    s,
                    "HTTP/1.1 {code} X\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n",
                    out.len()
                );
                let _ = s.write_all(&out);
            });
        }
    });
    base
}

/// `curl -s -L -X M -w '\n%{http_code}' -H.. [--data-binary @-] URL` as `xhr_start` ran it.
fn curl_xhr(
    method: &str,
    url: &str,
    headers: &[(String, String)],
    body: Option<&str>,
) -> (i32, String) {
    let mut c = Command::new("curl");
    c.args(["-s", "-L", "-X", method, "-w", "\n%{http_code}"]);
    for (k, v) in headers {
        c.args(["-H", &format!("{k}: {v}")]);
    }
    if body.is_some() {
        c.args(["--data-binary", "@-"]);
    }
    c.arg(url)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped());
    let mut ch = c.spawn().unwrap();
    if let Some(b) = body {
        ch.stdin.take().unwrap().write_all(b.as_bytes()).unwrap();
    }
    drop(ch.stdin.take());
    let out = ch.wait_with_output().unwrap();
    let s = String::from_utf8_lossy(&out.stdout).into_owned();
    let (b, code) = s.rsplit_once('\n').unwrap();
    (code.trim().parse().unwrap_or(0), b.to_string())
}

fn ureq_xhr(
    method: &str,
    url: &str,
    headers: &[(String, String)],
    body: Option<&str>,
) -> (i32, String) {
    let o = qml::http::Opts {
        follow: true,
        ..Default::default()
    };
    qml::http::send(method, url, headers, body.map(str::as_bytes), o)
        .ok()
        .and_then(|r| Some((r.status as i32, r.text().ok()?)))
        .unwrap_or((0, String::new()))
}

#[test]
fn xhr_matches_curl() {
    if !have("curl") {
        return;
    }
    let base = server();
    let h = |v: &[(&str, &str)]| {
        v.iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect::<Vec<_>>()
    };
    // What the server answers, known here: two sides that both got nothing (a server that
    // does not answer) agree with each other and not with this.
    type Case<'a> = (
        &'a str,
        String,
        Vec<(String, String)>,
        Option<&'a str>,
        (i32, &'a str),
    );
    const OK: (i32, &str) = (200, "héllo\nworld");
    let cases: Vec<Case> = vec![
        ("GET", format!("{base}/ok"), vec![], None, OK),
        ("GET", format!("{base}/redir"), vec![], None, OK),
        ("GET", format!("{base}/redir3"), vec![], None, OK),
        (
            "GET",
            format!("{base}/missing"),
            vec![],
            None,
            (404, "<h1>not found</h1>"),
        ),
        ("GET", format!("{base}/err"), vec![], None, (500, "")),
        (
            "GET",
            format!("{base}/echo"),
            h(&[("X-Api-Key", "k"), ("Content-Type", "application/json")]),
            None,
            (201, "GET content-type=application/json&x-api-key=k\n"),
        ),
        (
            "POST",
            format!("{base}/echo"),
            h(&[("Content-Type", "application/json")]),
            Some("{\"a\": \"ü\"}"),
            (201, "POST content-type=application/json\n{\"a\": \"ü\"}"),
        ),
        (
            "PUT",
            format!("{base}/echo"),
            vec![],
            Some(""),
            // curl names the type of the body `-d` sends, empty or not.
            (201, "PUT content-type=application/x-www-form-urlencoded\n"),
        ),
        (
            "DELETE",
            format!("{base}/echo"),
            vec![],
            None,
            (201, "DELETE \n"),
        ),
        // Nothing listens there: no status, no body.
        (
            "GET",
            "http://127.0.0.1:1/".to_string(),
            vec![],
            None,
            (0, ""),
        ),
    ];
    for (m, url, hs, b, want) in cases {
        let got = ureq_xhr(m, &url, &hs, b);
        assert_eq!(got, curl_xhr(m, &url, &hs, b), "{m} {url}");
        assert_eq!(got, (want.0, want.1.to_string()), "{m} {url}: the reply");
    }
}

fn tmpdir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("http_equiv_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Files curl (`flags`) and `qml::http::download` leave for each route, and what the server
/// sends there: two sides that both downloaded nothing agree with each other and not with it.
fn compare_download(flags: &[&str], fail: bool, tag: &str) {
    let base = server();
    let d = tmpdir(tag);
    let bin: Vec<u8> = (0..=255u8).cycle().take(300_000).collect();
    let ok = "héllo\nworld".as_bytes();
    // With `-f` an HTTP error fails and leaves no file; without it the error body is the file.
    let error = |body: &'static [u8]| (!fail).then_some(body);
    let routes: [(&str, Option<&[u8]>); 5] = [
        ("ok", Some(ok)),
        ("bin", Some(&bin)),
        ("redir", Some(ok)),
        ("missing", error(b"<h1>not found</h1>")),
        ("err", error(b"")),
    ];
    for (route, want_file) in routes {
        let (a, b) = (
            d.join(format!("curl-{route}")),
            d.join(format!("ureq-{route}")),
        );
        let curl_ok = Command::new("curl")
            .args(flags)
            .arg("-o")
            .arg(&a)
            .arg(format!("{base}/{route}"))
            .status()
            .unwrap()
            .success();
        let o = qml::http::Opts {
            follow: true,
            ..Default::default()
        };
        let ok = qml::http::download(&format!("{base}/{route}"), &[], &b, fail, o).is_ok();
        assert_eq!(ok, curl_ok, "{tag} {route} exit");
        let file = std::fs::read(&b).ok();
        assert_eq!(std::fs::read(&a).ok(), file, "{tag} {route} file");
        assert_eq!(
            ok,
            want_file.is_some(),
            "{tag} {route}: the download's result"
        );
        assert_eq!(
            file.as_deref(),
            want_file,
            "{tag} {route}: the file is what the server sent"
        );
    }
}

#[test]
fn network_image_download_matches_curl_f() {
    // scene.rs `remote_image`: `curl -s -f -L -o F URL`.
    if have("curl") {
        compare_download(&["-s", "-f", "-L"], true, "image");
    }
}

#[test]
fn process_download_matches_curl() {
    // The original's other `curl -sL -o F URL` lines (booru images) keep saving the error body.
    if have("curl") {
        compare_download(&["-s", "-L"], false, "proc");
    }
}

/// Concurrent downloads of one target (two players with the same cover) each finish with the
/// whole file: a shared `F.part` let one truncate or rename away the other's data.
#[test]
fn concurrent_downloads_of_one_target_do_not_collide() {
    let base = server();
    let d = tmpdir("race");
    let want: Vec<u8> = (0..=255u8).cycle().take(300_000).collect();
    for round in 0..10 {
        let f = d.join(format!("art{round}"));
        let hs: Vec<_> = (0..4)
            .map(|_| {
                let (url, f) = (format!("{base}/bin"), f.clone());
                std::thread::spawn(move || {
                    let o = qml::http::Opts {
                        follow: true,
                        ..Default::default()
                    };
                    qml::http::download(&url, &[], &f, true, o).is_ok()
                })
            })
            .collect();
        for h in hs {
            assert!(h.join().unwrap(), "round {round}: a download failed");
        }
        assert_eq!(std::fs::read(&f).unwrap(), want, "round {round}");
    }
    let left: Vec<_> = std::fs::read_dir(&d)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".part"))
        .collect();
    assert!(left.is_empty(), "leftover temp files: {left:?}");
}

/// The original's `[ -f F ] || curl -4 -sSL URL -o F` saved a 404/500 page as the cover and then reused it
/// for that track forever. The job fails on an HTTP error and leaves no file, so the next
/// attempt fetches the real image.
#[test]
fn cover_art_http_error_is_not_cached() {
    let base = server();
    let d = tmpdir("arterr");
    for route in ["missing", "err"] {
        let art = d.join(format!("{route}.bin"));
        assert_ne!(
            run_cover_art_job(&d, &format!("{base}/{route}"), &art),
            0.0,
            "{route}"
        );
        assert!(!art.exists(), "{route}: error page cached as cover");
    }
    let art = d.join("missing.bin");
    assert_eq!(run_cover_art_job(&d, &format!("{base}/png"), &art), 0.0);
    assert_eq!(std::fs::read(&art).unwrap(), PNG);
}

#[test]
fn file_description_matches_file_command() {
    if !have("file") {
        return;
    }
    let d = tmpdir("magic");
    let samples: Vec<(&str, Vec<u8>)> = vec![
        ("png", b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\x03\x20\0\0\x02\x58\x08\x06\0\0\0".to_vec()),
        ("gif", b"GIF89a\x40\x01\xf0\0\x80\0\0".to_vec()),
        ("jpg", b"\xff\xd8\xff\xe0\0\x10JFIF\0\x01\x01\0\0H\0H\0\0\xff\xc0\0\x11\x08\x04\x38\x07\x80\x03\x01\x22\0\x02\x11\x01\x03\x11\x01\xff\xd9".to_vec()),
        ("html", b"<!DOCTYPE html><html><body>404</body></html>".to_vec()),
        ("empty", vec![]),
    ];
    for (n, bytes) in samples {
        let p = d.join(n);
        std::fs::write(&p, bytes).unwrap();
        let out = Command::new("file").arg(&p).output().unwrap();
        let want = String::from_utf8_lossy(&out.stdout).into_owned();
        let got = format!("{}: {}\n", p.display(), qml::magic::describe(&p).unwrap());
        assert_eq!(got, want, "{n}");
    }
}

const WTTR: &str = r#"{"current_condition":[{"temp_C":"7","weatherDesc":[{"value":"Partly cloudy"}],"FeelsLikeC":"4"}],
"nearest_area":[{"areaName":[{"value":"Москва"}],"latitude":"55.752"}],
"request":[{"query":"Moscow"}],
"weather":[{"astronomy":[{"sunrise":"07:44 AM","sunset":"04:41 PM"}],"date":"2026-10-31"},{"astronomy":[]}]}"#;

#[test]
fn weather_filter_matches_jq() {
    if !have("jq") {
        return;
    }
    let filter = "{current: .current_condition[0], location: .nearest_area[0], astronomy: .weather[0].astronomy[0]}";
    let inputs: Vec<&[u8]> = vec![
        WTTR.as_bytes(),
        b"{}",
        b"{\"weather\":[]}",
        b"null",
        b"",
        b"{\"a\":1} {\"current_condition\":[1]}",
    ];
    for input in inputs {
        let mut c = Command::new("jq")
            .arg(filter)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        c.stdin.take().unwrap().write_all(input).unwrap();
        let out = c.wait_with_output().unwrap();
        let want = String::from_utf8_lossy(&out.stdout).into_owned();
        let got = qml::jobs::weather_filter(input);
        assert_eq!(
            got.is_ok(),
            out.status.success(),
            "{}",
            String::from_utf8_lossy(input)
        );
        assert_eq!(
            got.unwrap_or_default().unwrap_or_default(),
            want,
            "{}",
            String::from_utf8_lossy(input)
        );
    }
    // Text that is not JSON (wttr.in's error page) fails like jq.
    assert!(qml::jobs::weather_filter(b"Unknown location; please try ~55.7,37.6").is_err());
}

#[test]
fn weather_city_matches_js() {
    // `cityName.trim().split(/\s+/).join('+')` evaluated in V4 / node.
    for (c, want) in [
        ("Saint Petersburg", "Saint+Petersburg"),
        ("  New   York \t", "New+York"),
        ("", ""),
        ("Москва", "Москва"),
        ("a\u{3000}b\u{a0}c", "a+b+c"),
    ] {
        assert_eq!(qml::jobs::weather_city(c), want, "{c:?}");
    }
}

/// CPU and wall time of N fetches via a curl process vs in-process ureq (`--features live`).
#[test]
#[cfg_attr(
    not(feature = "live"),
    ignore = "live: --features live, docs/benchmarks.md"
)]
fn measure_curl_vs_ureq() {
    let base = server();
    let url = format!("{base}/ok");
    let n = 200;
    let ru = |who| {
        // SAFETY: rusage is plain integers, for which all-zero bytes are a valid value.
        let mut r: libc::rusage = unsafe { std::mem::zeroed() };
        // SAFETY: `r` is a valid, writable rusage for getrusage(2) to fill.
        unsafe { libc::getrusage(who, &mut r) };
        (r.ru_utime.tv_sec as f64 + r.ru_utime.tv_usec as f64 / 1e6)
            + (r.ru_stime.tv_sec as f64 + r.ru_stime.tv_usec as f64 / 1e6)
    };
    let (t0, c0) = (std::time::Instant::now(), ru(libc::RUSAGE_CHILDREN));
    for _ in 0..n {
        curl_xhr("GET", &url, &[], None);
    }
    let (curl_wall, curl_cpu) = (t0.elapsed().as_secs_f64(), ru(libc::RUSAGE_CHILDREN) - c0);
    let (t0, s0) = (std::time::Instant::now(), ru(libc::RUSAGE_SELF));
    for _ in 0..n {
        ureq_xhr("GET", &url, &[], None);
    }
    let (ureq_wall, ureq_cpu) = (t0.elapsed().as_secs_f64(), ru(libc::RUSAGE_SELF) - s0);
    println!(
        "{n} GETs: curl wall {:.2} ms/req, child CPU {:.2} ms/req; ureq wall {:.2} ms/req, process CPU (incl. test server) {:.2} ms/req",
        curl_wall * 1e3 / n as f64,
        curl_cpu * 1e3 / n as f64,
        ureq_wall * 1e3 / n as f64,
        ureq_cpu * 1e3 / n as f64
    );
}

/// The job runs for the original declaration it names (file + id) in place of its original command:
/// the native download replaces `bash`/`curl` and `onExited` sees exit code 0.
#[test]
fn cover_art_job_runs_for_its_declaration() {
    let base = server();
    let d = tmpdir("job");
    let art = d.join("art.bin");
    assert_eq!(run_cover_art_job(&d, &format!("{base}/bin"), &art), 0.0);
    let want: Vec<u8> = (0..=255u8).cycle().take(300_000).collect();
    assert_eq!(std::fs::read(&art).unwrap(), want);
}

/// Local art (`file://` URLs from Chrome, mpv-mpris...): the job leaves the file
/// PlayerControl's `curl -4 -sSL 'URL' -o 'F'` does and fails when it does.
#[test]
fn cover_art_file_urls_match_curl() {
    if !have("curl") {
        return;
    }
    let d = tmpdir("fileurl");
    let src = d.join("cover art é.png");
    let data: Vec<u8> = (0..=255u8).cycle().take(70_000).collect();
    std::fs::write(&src, &data).unwrap();
    const KEEP: &percent_encoding::AsciiSet =
        &percent_encoding::NON_ALPHANUMERIC.remove(b'/').remove(b'.');
    let enc = |p: &std::path::Path| {
        percent_encoding::utf8_percent_encode(p.to_str().unwrap(), KEEP).to_string()
    };
    let urls = [
        format!("file://{}", enc(&src)),
        format!("file://localhost{}", enc(&src)),
        format!("file://127.0.0.1{}", enc(&src)),
        format!("FILE://{}", enc(&src)),
        format!("file://{}", enc(&d.join("missing.png"))),
        format!("file://otherhost{}", enc(&src)),
        format!("file://{}", src.display()),
    ];
    for (n, url) in urls.iter().enumerate() {
        let (want_out, got_out) = (d.join(format!("curl{n}")), d.join(format!("job{n}")));
        let st = Command::new("bash")
            .args([
                "-c",
                &format!(
                    "[ -f {0} ] || curl -4 -sSL '{url}' -o '{0}'",
                    want_out.display()
                ),
            ])
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        let code = run_cover_art_job(&d, url, &got_out);
        assert_eq!(code == 0.0, st.success(), "exit code for {url}");
        assert_eq!(
            std::fs::read(&got_out).ok(),
            std::fs::read(&want_out).ok(),
            "file for {url}"
        );
    }
}

/// A 1920x1080 JPEG header whose `file` description starts with `density 72x72`.
const JPEG: &[u8] = b"\xff\xd8\xff\xe0\0\x10JFIF\0\x01\x01\0\0H\0H\0\0\xff\xc0\0\x11\x08\x04\x38\x07\x80\x03\x01\x22\0\x02\x11\x01\x03\x11\x01\xff\xd9";

/// FloatingImage's `ImageDownloaderProcess` without the original's flaws: an HTTP error page is
/// not cached as the image (the next try downloads again), a cached non-image is replaced, and
/// the size the QML regex takes first is the image's, not a JPEG's 72x72 density.
#[test]
fn image_downloader_skips_error_pages_and_reports_real_size() {
    let base = server();
    let d = tmpdir("imgdl");
    let img = d.join("cache/img");
    let (code, _) = run_image_downloader(&d, &format!("{base}/missing"), &img);
    assert_ne!(code, 0.0);
    assert!(!img.exists(), "a 404 page must not be cached");
    std::fs::write(&img, b"<h1>not found</h1>").unwrap();
    let (code, out) = run_image_downloader(&d, &format!("{base}/jpg"), &img);
    assert_eq!(code, 0.0);
    assert_eq!(std::fs::read(&img).unwrap(), JPEG);
    let re = regex::Regex::new(r"(\d+)\s*x\s*(\d+)").unwrap();
    let c = re.captures(&out).unwrap_or_else(|| panic!("{out:?}"));
    assert_eq!((&c[1], &c[2]), ("1920", "1080"), "{out:?}");
}

/// Run `ImageDownloaderProcess`'s type, `ImageDownload`, for `url` -> `file` under `d`: exit
/// code and stdout.
fn run_image_downloader(d: &std::path::Path, url: &str, file: &std::path::Path) -> (f64, String) {
    let qml_dir = d.join("rusty-impulse/modules/common/utils");
    std::fs::create_dir_all(&qml_dir).unwrap();
    let src = format!(
        r#"import QtQuick
import Rustshell.Io
import Impulse
ImageDownload {{
    id: root
    property string out: ""
    property int code: -1
    url: "{url}"
    path: "{}"
    running: true
    stdout: StdioCollector {{ onStreamFinished: root.out = text }}
    onExited: (exitCode, exitStatus) => {{ root.code = exitCode }}
}}
"#,
        file.display()
    );
    let f = qml_dir.join("ImageDownloaderProcess.qml");
    std::fs::write(&f, src).unwrap();
    let mut rt = qml::runtime::Runtime::new(None);
    let mut lp = event_loop::Loop::new(&mut rt);
    let root = rt.load_file(&f).unwrap();
    rt.settle();
    let read = |rt: &qml::runtime::Runtime, k: &str| rt.world.borrow_mut().read(root, k);
    lp.block_until(&mut rt, |rt| {
        read(rt, "code").map(|v| v.to_number()) != Some(-1.0)
    });
    lp.flush(&mut rt);
    let code = read(&rt, "code").map_or(-1.0, |v| v.to_number());
    let out = read(&rt, "out")
        .map(|v| v.to_js_string())
        .unwrap_or_default();
    (code, out)
}

/// Run PlayerControl's `coverArtDownloader`, a `Download` with `ipv4`, for `url` -> `art` under
/// `d`; its exit code.
fn run_cover_art_job(d: &std::path::Path, url: &str, art: &std::path::Path) -> f64 {
    let qml_dir = d.join("rusty-impulse/modules/rusty/mediaControls");
    std::fs::create_dir_all(&qml_dir).unwrap();
    let src = format!(
        r#"import QtQuick
import Rustshell.Io
import Impulse
Item {{
    id: root
    property int code: -1
    Download {{
        id: coverArtDownloader
        url: "{url}"
        path: "{}"
        ipv4: true
        running: true
        onExited: (exitCode, exitStatus) => {{ root.code = exitCode }}
    }}
}}
"#,
        art.display()
    );
    let f = qml_dir.join("PlayerControl.qml");
    std::fs::write(&f, src).unwrap();
    let mut rt = qml::runtime::Runtime::new(None);
    let mut lp = event_loop::Loop::new(&mut rt);
    let root = rt.load_file(&f).unwrap();
    rt.settle();
    lp.block_until(&mut rt, |rt| {
        rt.world
            .borrow_mut()
            .read(root, "code")
            .map(|v| v.to_number())
            != Some(-1.0)
    });
    rt.world
        .borrow_mut()
        .read(root, "code")
        .map_or(-1.0, |v| v.to_number())
}

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x10\0\0\0\x10\x08\x06\0\0\0\x1f\xf3\xffa";

/// Favicon / PlayerControl / ImageDownloaderProcess cache `[ -f F ] || curl -o F URL`: the original kept
/// an error page or an empty reply as the image, and `[ -f ]` skipped every later download.
/// Native keeps only what can be an image (an image sent with an error status included, as
/// Google's favicon service answers 404 with its globe icon) and retries the rest.
#[test]
fn image_cache_keeps_no_error_pages() {
    let base = server();
    let d = tmpdir("cache");
    let o = qml::http::Opts {
        follow: true,
        ..Default::default()
    };
    for (route, kept) in [
        ("png", true),
        ("png404", true),
        ("bin", true),
        ("missing", false),
        ("ok", false),
        ("empty", false),
        ("err", false),
    ] {
        let f = d.join(route);
        let f = f.to_str().unwrap();
        let got = qml::jobs::fetch_unless_present(&format!("{base}/{route}"), &[], f, o);
        assert_eq!(got.is_ok(), kept, "{route}: {got:?}");
        assert_eq!(std::path::Path::new(f).is_file(), kept, "{route}");
    }
    // A cached image is not fetched again.
    let f = d.join("png");
    assert_eq!(
        qml::jobs::fetch_unless_present("http://127.0.0.1:9/x", &[], f.to_str().unwrap(), o),
        Ok(())
    );
}

/// A server answering `/big` with a `Content-Length` far over any limit and `/endless` with a
/// body that never ends; returns its base URL.
fn endless_server() -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            std::thread::spawn(move || {
                let mut r = BufReader::new(s.try_clone().unwrap());
                let mut line = String::new();
                r.read_line(&mut line).unwrap();
                let big = line.contains("/big");
                while r.read_line(&mut line).is_ok_and(|n| n > 2) {
                    line.clear();
                }
                let mut s = s;
                let head = if big {
                    "HTTP/1.1 200 OK\r\nContent-Length: 1099511627776\r\n\r\n"
                } else {
                    "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n"
                };
                let _ = s.write_all(head.as_bytes());
                let chunk = vec![0u8; 64 << 10];
                while s.write_all(&chunk).is_ok() {}
            });
        }
    });
    base
}

/// The original's `curl -o` wrote whatever a server sent: an endless cover filled tmpfs `/tmp` (RAM).
/// A body over the limit, or announced over it, fails with nothing left behind.
#[test]
fn download_refuses_oversize_and_endless_bodies() {
    let base = endless_server();
    let d = tmpdir("dl-max");
    for route in ["big", "endless"] {
        let f = d.join(route);
        let o = qml::http::Opts {
            stall: true,
            ..Default::default()
        };
        let e = qml::http::download_max(&format!("{base}/{route}"), &[], &f, true, o, 1 << 20)
            .unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::FileTooLarge, "{route}: {e}");
    }
    let e = qml::http::download(
        &format!("{base}/endless"),
        &[],
        &d.join("x"),
        true,
        Default::default(),
    )
    .unwrap_err();
    assert_eq!(e.kind(), std::io::ErrorKind::FileTooLarge, "{e}");
    let left: Vec<_> = std::fs::read_dir(&d)
        .unwrap()
        .flatten()
        .map(|e| e.file_name())
        .collect();
    assert!(left.is_empty(), "{left:?}");
}
