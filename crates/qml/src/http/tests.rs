//! Proxy selection against real curl: the same env and URL must reach the same server
//! with the same request line.
use super::*;
use crate::jobs_wall_gcloud_trans::testutil::{self, Resp, Server};
use std::net::SocketAddr;
use std::sync::mpsc::Receiver;
use std::time::Duration;

const VARS: [&str; 6] = [
    "http_proxy",
    "HTTP_PROXY",
    "https_proxy",
    "HTTPS_PROXY",
    "all_proxy",
    "ALL_PROXY",
];

/// Which server got the request, and for which host:port. ureq tunnels plain HTTP
/// through an HTTP proxy with CONNECT where curl sends an absolute-form GET.
fn hits(direct: &Server, proxies: &[Server]) -> Vec<(String, String)> {
    let mut v = vec![];
    for (name, s) in std::iter::once(("direct", direct)).chain(VARS.iter().copied().zip(proxies)) {
        for r in s.take() {
            let t = r.target.strip_prefix("http://").unwrap_or(&r.target);
            let auth = t.split('/').next().unwrap_or_default();
            let auth = if auth.contains(':') {
                auth.to_string()
            } else {
                format!("{auth}:80")
            };
            v.push((name.to_string(), auth));
        }
    }
    v
}

fn native(url: &str, env: &[(&str, String)]) {
    let (url, env) = (
        url.to_string(),
        env.iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect::<Vec<_>>(),
    );
    let get = |k: &str| env.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
    let _ = send_env("GET", &url, &[], None, Opts::default(), &get).and_then(|r| {
        r.bytes()?;
        Ok(())
    });
}

fn curl(url: &str, env: &[(&str, String)]) {
    let mut c = std::process::Command::new("curl");
    c.args(["-q", "-s", "-o", "/dev/null", url])
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default());
    for (k, v) in env {
        c.env(k, v);
    }
    c.status().unwrap();
}

#[test]
fn proxy_env_matches_curl() {
    let direct = testutil::serve(|_| Resp::new(200, "direct"));
    let proxies: Vec<Server> = VARS
        .iter()
        .map(|_| testutil::serve(|_| Resp::new(200, "proxied")))
        .collect();
    let px = |n: &str| {
        let i = VARS.iter().position(|v| *v == n).unwrap();
        format!("http://127.0.0.1:{}", proxies[i].port)
    };
    let d = direct.port;
    let http = format!("http://localhost:{d}/x");
    let ip = format!("http://127.0.0.1:{d}/x");
    let https = "https://a.example.invalid/x".to_string();
    let s = |v: &str| v.to_string();
    let all: Vec<(&str, String)> = VARS.iter().map(|v| (*v, px(v))).collect();
    let cases: Vec<(&String, Vec<(&str, String)>)> = vec![
        (&http, vec![]),
        (&http, vec![("HTTP_PROXY", px("HTTP_PROXY"))]),
        (&http, vec![("http_proxy", px("http_proxy"))]),
        (&http, all.clone()),
        (&https, all.clone()),
        (&https, vec![("HTTPS_PROXY", px("HTTPS_PROXY"))]),
        (
            &https,
            vec![
                ("HTTPS_PROXY", px("HTTPS_PROXY")),
                ("ALL_PROXY", px("ALL_PROXY")),
            ],
        ),
        (&https, vec![("http_proxy", px("http_proxy"))]),
        (&http, vec![("https_proxy", px("https_proxy"))]),
        (
            &http,
            vec![
                ("ALL_PROXY", px("ALL_PROXY")),
                ("HTTP_PROXY", px("HTTP_PROXY")),
            ],
        ),
        (
            &http,
            vec![
                ("all_proxy", px("all_proxy")),
                ("ALL_PROXY", px("ALL_PROXY")),
            ],
        ),
        (
            &http,
            vec![("http_proxy", s("")), ("ALL_PROXY", px("ALL_PROXY"))],
        ),
        (
            &http,
            vec![("http_proxy", format!("127.0.0.1:{}", proxies[0].port))],
        ),
        (
            &http,
            [all.clone(), vec![("NO_PROXY", s("localhost"))]].concat(),
        ),
        (
            &http,
            [
                all.clone(),
                vec![("no_proxy", s("other")), ("NO_PROXY", s("localhost"))],
            ]
            .concat(),
        ),
        (
            &http,
            [all.clone(), vec![("no_proxy", s("foo, LOCALHOST"))]].concat(),
        ),
        (
            &http,
            [all.clone(), vec![("no_proxy", s("localhost."))]].concat(),
        ),
        (&http, [all.clone(), vec![("no_proxy", s("*"))]].concat()),
        (
            &http,
            [all.clone(), vec![("no_proxy", s("*,foo"))]].concat(),
        ),
        (
            &http,
            [all.clone(), vec![("no_proxy", s("127.0.0.1"))]].concat(),
        ),
        (
            &https,
            [all.clone(), vec![("no_proxy", s(".example.invalid"))]].concat(),
        ),
        (
            &https,
            [all.clone(), vec![("no_proxy", s("example.invalid"))]].concat(),
        ),
        (
            &https,
            [all.clone(), vec![("no_proxy", s("ample.invalid"))]].concat(),
        ),
        (
            &https,
            [all.clone(), vec![("no_proxy", s("*.example.invalid"))]].concat(),
        ),
        (
            &https,
            [all.clone(), vec![("no_proxy", s("a.example.invalid:443"))]].concat(),
        ),
        (
            &ip,
            [all.clone(), vec![("no_proxy", s("127.0.0.1"))]].concat(),
        ),
        (
            &ip,
            [all.clone(), vec![("no_proxy", s("127.0.0.0/8"))]].concat(),
        ),
        (
            &ip,
            [all.clone(), vec![("no_proxy", s("127.1.0.0/16"))]].concat(),
        ),
        (
            &ip,
            [all.clone(), vec![("no_proxy", s("localhost"))]].concat(),
        ),
    ];
    // Where curl 8.11 (what the original's request.sh runs) sends each case: no request, the
    // server itself, or the proxy of that variable. Recorded, so the host's curl version
    // (Ubuntu's 7.81 still proxies `no_proxy=localhost.`) cannot change the expectation.
    let want = [
        Some("direct"),
        Some("direct"),
        Some("http_proxy"),
        Some("http_proxy"),
        Some("https_proxy"),
        Some("HTTPS_PROXY"),
        Some("HTTPS_PROXY"),
        None,
        Some("direct"),
        Some("ALL_PROXY"),
        Some("all_proxy"),
        Some("ALL_PROXY"),
        Some("http_proxy"),
        Some("direct"),
        Some("http_proxy"),
        Some("direct"),
        Some("direct"),
        Some("direct"),
        Some("http_proxy"),
        Some("http_proxy"),
        None,
        None,
        Some("https_proxy"),
        Some("https_proxy"),
        Some("https_proxy"),
        Some("direct"),
        Some("direct"),
        Some("http_proxy"),
        Some("http_proxy"),
    ];
    assert_eq!(want.len(), cases.len());
    for ((url, env), want) in cases.iter().zip(want) {
        let auth = url.split('/').nth(2).unwrap();
        let auth = if auth.contains(':') {
            auth.to_string()
        } else {
            format!("{auth}:443")
        };
        let want: Vec<(String, String)> = want
            .map(|n| match n {
                "direct" => (n.to_string(), ":80".to_string()),
                _ => (n.to_string(), auth.clone()),
            })
            .into_iter()
            .collect();
        native(url, env);
        assert_eq!(want, hits(&direct, &proxies), "{url} {env:?}");
    }
}

/// A proxy without scheme or port is `http://host:80` for both.
#[test]
fn proxy_default_port_matches_curl() {
    if !testutil::have("curl") {
        return;
    }
    let Ok(l) = std::net::TcpListener::bind("127.0.0.2:80") else {
        crate::testenv::unavailable(
            "cannot bind 127.0.0.2:80 (needs net.ipv4.ip_unprivileged_port_start=0)",
        );
        return;
    };
    let (tx, accepted) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            let _ = tx.send(s.peer_addr().unwrap());
        }
    });
    // Connections the client made: those accepted before a marker connected after it returned
    // (the listen queue hands them out in order).
    let count = || {
        let marker = std::net::TcpStream::connect("127.0.0.2:80").unwrap();
        let me = marker.local_addr().unwrap();
        accepted.iter().take_while(|a| *a != me).count()
    };
    let env = [("http_proxy", "127.0.0.2".to_string())];
    curl("http://localhost:9/x", &env);
    let want = count();
    native("http://localhost:9/x", &env);
    assert_eq!((want, count()), (1, 1));
}

fn a(s: &str) -> SocketAddr {
    s.parse().unwrap()
}

#[test]
fn interleave_alternates_families() {
    let v = [
        a("[::1]:1"),
        a("[::2]:1"),
        a("1.0.0.1:1"),
        a("1.0.0.2:1"),
        a("1.0.0.3:1"),
    ];
    assert_eq!(
        interleave(&v),
        [
            a("[::1]:1"),
            a("1.0.0.1:1"),
            a("[::2]:1"),
            a("1.0.0.2:1"),
            a("1.0.0.3:1")
        ]
    );
    assert_eq!(interleave(&v[2..]), v[2..]);
    assert!(interleave(&[]).is_empty());
}

/// A dropped IPv6 route must not hold up IPv4 (curl's happy eyeballs, not ureq's
/// address-by-address connect).
#[test]
fn happy_connect_skips_blackholed_family() {
    // The IPv6 attempts never finish: waiting for one is a hang.
    let v = [a("[::1]:1"), a("[::2]:1"), a("1.0.0.1:1")];
    let r = async_io::block_on(happy_connect(
        &v,
        Duration::from_millis(50),
        |x| async move {
            if x.is_ipv6() {
                std::future::pending().await
            } else {
                Ok(x)
            }
        },
    ));
    assert_eq!(r.unwrap(), a("1.0.0.1:1"));
}

#[test]
fn happy_connect_moves_on_at_once_and_reports_last_error() {
    // A stagger longer than any test may run: waiting for it is a hang.
    let v = [a("[::1]:1"), a("1.0.0.1:1"), a("1.0.0.2:1")];
    let delay = Duration::from_secs(24 * 3600);
    let r = async_io::block_on(happy_connect(&v, delay, |x| async move {
        if x == a("1.0.0.2:1") {
            Ok(x)
        } else {
            Err(std::io::ErrorKind::ConnectionRefused.into())
        }
    }));
    assert_eq!(r.unwrap(), a("1.0.0.2:1"));
    let r = async_io::block_on(happy_connect(&v[..2], delay, |_| async {
        Err::<(), _>(std::io::Error::from(std::io::ErrorKind::HostUnreachable))
    }));
    assert_eq!(r.unwrap_err().kind(), std::io::ErrorKind::HostUnreachable);
    assert_eq!(
        async_io::block_on(happy_connect(&[], Duration::ZERO, |_| async { Ok(()) }))
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::ConnectionRefused
    );
}

/// The custom connector and transport carry a real request.
#[test]
fn get_over_loopback() {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        use std::io::Write;
        let (mut s, _) = l.accept().unwrap();
        let mut b = [0; 1024];
        let _ = s.read(&mut b);
        let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
    });
    let r = get(&format!("http://127.0.0.1:{port}/"), &[], Opts::default()).unwrap();
    assert_eq!(r.status, 200);
    assert_eq!(r.text().unwrap(), "ok");
}

/// A reply carries the reason phrase of its own status line as the server worded it, also on
/// a connection used again, and "" when the server sent none.
#[test]
fn reason_is_the_servers_wording() {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let accepted = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let n = accepted.clone();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let mut s = s.unwrap();
            n.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            std::thread::spawn(move || {
                use std::io::Write;
                let mut b = [0; 4096];
                for line in ["404 Not  Here", "200 Fine", "200", "500 "] {
                    if !matches!(s.read(&mut b), Ok(n) if n > 0) {
                        return;
                    }
                    let _ = write!(s, "HTTP/1.1 {line}\r\nContent-Length: 2\r\n\r\nok");
                }
            });
        }
    });
    let url = format!("http://127.0.0.1:{port}/");
    let o = Opts {
        ipv4: true,
        ..Default::default()
    };
    for (status, reason) in [(404, "Not  Here"), (200, "Fine"), (200, ""), (500, "")] {
        let r = get(&url, &[], o).unwrap();
        assert_eq!((r.status, r.reason.as_str()), (status, reason));
        assert_eq!(r.text().unwrap(), "ok");
    }
    assert_eq!(accepted.load(std::sync::atomic::Ordering::SeqCst), 1);
}

/// A server that takes the request, reports it on the receiver, sends `reply`, then holds the
/// connection open until the client closes it.
fn stalling(reply: &'static [u8]) -> (u16, Receiver<()>) {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let (tx, got) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { return };
            let tx = tx.clone();
            std::thread::spawn(move || {
                use std::io::Write;
                let _ = s.read(&mut [0; 4096]);
                let _ = tx.send(());
                let _ = s.write_all(reply);
                while matches!(s.read(&mut [0; 4096]), Ok(n) if n > 0) {}
            });
        }
    });
    (port, got)
}

/// Run `f` under a fresh `Stop` on its own thread, stop it once `blocked` reports it is waiting
/// (a request at the server, a connect or lookup under way), and return what it returned.
/// Nothing ends `f`'s wait but the stop, so a stop that does not reach it is a hang.
fn stop_after<R: Send + 'static>(
    blocked: Receiver<()>,
    f: impl FnOnce() -> R + Send + 'static,
) -> R {
    let stop = std::sync::Arc::new(crate::svc::Stop::default());
    let s = stop.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(with_stop(&s, f));
    });
    blocked.recv().unwrap();
    assert!(rx.try_recv().is_err(), "returned before the stop");
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    rx.recv().unwrap()
}

#[test]
fn stop_interrupts_waiting_for_the_response() {
    let (port, got) = stalling(b"");
    let url = format!("http://127.0.0.1:{port}/");
    let r = stop_after(got, move || {
        get(&url, &[], Opts::default()).map(|r| r.status)
    });
    assert!(r.is_err());
}

#[test]
fn stop_interrupts_a_body_download() {
    let (port, got) = stalling(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\npartial");
    let url = format!("http://127.0.0.1:{port}/");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("out");
    let p = path.clone();
    let r = stop_after(got, move || {
        download(&url, &[], &p, false, Opts::default()).map(|_| ())
    });
    assert!(r.is_err());
    assert!(!path.exists());
}

#[test]
fn stop_interrupts_a_pending_connect() {
    // TEST-NET-1: blackholed, so the connect stays pending until the stop.
    let (tx, connecting) = std::sync::mpsc::channel();
    let r = stop_after(connecting, move || {
        CONNECTING.with(|c| *c.borrow_mut() = Some(Box::new(move || tx.send(()).unwrap())));
        get("http://192.0.2.1:9/", &[], Opts::default()).map(|r| r.status)
    });
    assert!(r.is_err());
}

#[test]
fn stopped_before_the_request_fails_at_once() {
    let (port, _got) = stalling(b"HTTP/1.1 200 OK\r\n\r\n");
    let url = format!("http://127.0.0.1:{port}/");
    let stop = crate::svc::Stop::default();
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(with_stop(&stop, || get(&url, &[], Opts::default())).is_err());
}

/// A stop after the scope ends leaves the pooled connection it used alone.
#[test]
fn stop_after_the_scope_spares_pooled_connections() {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let accepted = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let n = accepted.clone();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let mut s = s.unwrap();
            n.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            std::thread::spawn(move || {
                use std::io::Write;
                let mut b = [0; 4096];
                while matches!(s.read(&mut b), Ok(n) if n > 0) {
                    let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
                }
            });
        }
    });
    let url = format!("http://127.0.0.1:{port}/");
    let o = Opts {
        ipv4: true,
        ..Default::default()
    };
    let stop = crate::svc::Stop::default();
    let body = with_stop(&stop, || get(&url, &[], o).unwrap().text().unwrap());
    assert_eq!(body, "ok");
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    assert_eq!(get(&url, &[], o).unwrap().text().unwrap(), "ok");
    assert_eq!(accepted.load(std::sync::atomic::Ordering::SeqCst), 1);
}

/// The keepalive timer `ss` shows on the client socket to `port`, e.g. `keepalive,59sec,0`.
fn keepalive_timer(port: u16) -> String {
    let o = std::process::Command::new("ss")
        .args([
            "-tnoH",
            "state",
            "established",
            &format!("( dport = :{port} )"),
        ])
        .output()
        .unwrap();
    let s = String::from_utf8_lossy(&o.stdout);
    let t = s.split("timer:(").nth(1).unwrap_or_default();
    t.split(')').next().unwrap_or_default().to_string()
}

fn sockopt(s: &std::net::TcpStream, level: libc::c_int, opt: libc::c_int) -> libc::c_int {
    use std::os::fd::AsRawFd;
    let mut v: libc::c_int = 0;
    let mut n = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
    // SAFETY: the fd is the live socket `s`; the kernel writes at most `n` (the size of the live
    // c_int `v`) bytes to `v` and the length to `n`.
    let r = unsafe {
        libc::getsockopt(
            s.as_raw_fd(),
            level,
            opt,
            (&mut v as *mut libc::c_int).cast(),
            &mut n,
        )
    };
    assert_eq!(r, 0);
    v
}

/// A handshake nobody answers ends at the caller's limit; one that is answered ends with the
/// connection, however long the limit.
#[test]
fn a_connect_ends_with_the_handshake_or_at_the_limit() {
    // TEST-NET-1: blackholed, so the handshake is never answered.
    let unanswered = async_io::block_on(connect_within(
        "192.0.2.1:9".parse().unwrap(),
        Some(std::time::Duration::from_millis(200)),
    ));
    assert_eq!(unanswered.unwrap_err().kind(), std::io::ErrorKind::TimedOut);
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let answered = async_io::block_on(connect_within(
        l.local_addr().unwrap(),
        Some(std::time::Duration::from_secs(86_400)),
    ));
    assert!(answered.is_ok(), "{answered:?}");
}

/// Without `stall` a vanished peer is still noticed within 30 s.
#[test]
fn keepalive_gives_up_on_a_silent_peer_in_30s() {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let s = std::net::TcpStream::connect(l.local_addr().unwrap()).unwrap();
    keepalive(&s).unwrap();
    assert_eq!(sockopt(&s, libc::SOL_SOCKET, libc::SO_KEEPALIVE), 1);
    let idle = sockopt(&s, libc::IPPROTO_TCP, libc::TCP_KEEPIDLE);
    let intvl = sockopt(&s, libc::IPPROTO_TCP, libc::TCP_KEEPINTVL);
    let cnt = sockopt(&s, libc::IPPROTO_TCP, libc::TCP_KEEPCNT);
    assert!(
        cnt > 0 && idle + intvl * cnt <= 30,
        "{idle} + {intvl} x {cnt}"
    );
}

/// The keepalive above is on the sockets requests really use.
#[test]
fn requests_use_the_keepalive() {
    if !crate::testenv::require("ss") {
        return;
    }
    let (port, got) = stalling(b"");
    let url = format!("http://127.0.0.1:{port}/");
    let stop = std::sync::Arc::new(crate::svc::Stop::default());
    let s = stop.clone();
    let h = std::thread::spawn(move || with_stop(&s, || get(&url, &[], Opts::default()).is_err()));
    got.recv().unwrap();
    let t = keepalive_timer(port);
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(h.join().unwrap());
    // `keepalive,14sec,0`
    let mut f = t.split(',');
    assert_eq!(f.next(), Some("keepalive"), "{t:?}");
    let secs: i64 = f
        .next()
        .unwrap_or_default()
        .trim_end_matches("sec")
        .parse()
        .unwrap_or(-1);
    assert!((0..=KEEPIDLE as i64).contains(&secs), "{t:?}");
}

/// A stop does not wait for a hanging name lookup (run through `cancellable`).
#[test]
fn stop_interrupts_a_pending_lookup() {
    let (tx, looking_up) = std::sync::mpsc::channel();
    let (release, hang) = std::sync::mpsc::channel::<()>();
    let r = stop_after(looking_up, move || {
        cancellable(move || {
            tx.send(()).unwrap();
            let _ = hang.recv();
            Ok(())
        })
    });
    assert!(r.is_err());
    drop(release);
}

#[test]
fn lookup_outside_a_stop_scope_runs_inline() {
    let id = std::thread::current().id();
    assert!(cancellable(move || Ok(std::thread::current().id() == id)).unwrap());
}

/// Lookups in a stop scope run on the shared blocking pool: back-to-back requests reuse its
/// idle threads instead of each starting (and leaking, when stopped) a fresh one.
#[test]
fn stoppable_lookups_reuse_pool_threads() {
    let stop = crate::svc::Stop::default();
    let ids: std::collections::HashSet<_> = (0..8)
        .map(|_| {
            with_stop(&stop, || {
                cancellable(|| Ok(std::thread::current().id())).unwrap()
            })
        })
        .collect();
    assert!(ids.len() < 8, "{} threads for 8 lookups", ids.len());
}

/// Request line and proxy credentials of the first request a proxy at `port` receives.
fn proxy_sees(l: &std::net::TcpListener) -> (String, Option<String>) {
    use std::io::{BufRead, Write};
    let (s, _) = l.accept().unwrap();
    let mut r = std::io::BufReader::new(s.try_clone().unwrap());
    let mut line = String::new();
    r.read_line(&mut line).unwrap();
    let mut auth = None;
    loop {
        let mut h = String::new();
        if r.read_line(&mut h).unwrap_or(0) == 0 || h == "\r\n" {
            break;
        }
        if let Some((k, v)) = h.trim_end().split_once(": ")
            && k.eq_ignore_ascii_case("proxy-authorization")
        {
            auth = Some(v.to_string());
        }
    }
    let _ = (&s).write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
    (line.trim_end().to_string(), auth)
}

#[test]
fn http_through_proxy_is_forwarded_like_curl() {
    if !testutil::have("curl") {
        return;
    }
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    for (proxy, url) in [
        (
            format!("http://127.0.0.1:{port}"),
            "http://wttr.in/Paris?format=j1",
        ),
        (
            format!("http://u%40x:p@127.0.0.1:{port}"),
            "http://example.invalid:8080/a/b?c=d",
        ),
    ] {
        let env = [("http_proxy", proxy)];
        let t = std::thread::scope(|sc| {
            let h = sc.spawn(|| proxy_sees(&l));
            curl(url, &env);
            h.join().unwrap()
        });
        let n = std::thread::scope(|sc| {
            let h = sc.spawn(|| proxy_sees(&l));
            native(url, &env);
            h.join().unwrap()
        });
        assert_eq!(n, t, "{url}");
        assert!(t.0.starts_with("GET http://"), "{t:?}");
    }
}

/// A server that answers 200 and then sends body bytes until the client hangs up.
fn endless() -> u16 {
    use std::io::Write;
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            std::thread::spawn(move || {
                let mut s = s;
                let mut req = [0u8; 4096];
                let _ = std::io::Read::read(&mut s, &mut req);
                let _ = s.write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n");
                let chunk = [b'x'; 64 * 1024];
                while s.write_all(&chunk).is_ok() {}
            });
        }
    });
    port
}

#[test]
fn endless_body_is_bounded() {
    let port = endless();
    let url = format!("http://127.0.0.1:{port}/");
    let get = || send_env("GET", &url, &[], None, Opts::default(), &|_| None).unwrap();
    let small = get().bytes_max(1 << 20).map(|v| v.len());
    let text = get().text().map(|t| t.len());
    let e = small.expect_err("bytes_max kept an over-limit body");
    assert_eq!(e.kind(), std::io::ErrorKind::FileTooLarge);
    let e = text.expect_err("text() kept a body over BODY_MAX");
    assert_eq!(e.kind(), std::io::ErrorKind::FileTooLarge);
}

#[test]
fn body_at_limit_is_kept() {
    let s = testutil::serve(|_| Resp::new(200, vec![b'y'; 1000]));
    let url = format!("http://127.0.0.1:{}/", s.port);
    let get = || send_env("GET", &url, &[], None, Opts::default(), &|_| None).unwrap();
    assert_eq!(get().bytes_max(1000).unwrap().len(), 1000);
    assert_eq!(
        get().bytes_max(999).unwrap_err().kind(),
        std::io::ErrorKind::FileTooLarge
    );
}

/// The CIDR bits of a `no_proxy` address, case by case what curl 8.23 does (each run against
/// it with a proxy that refuses connections): digits only, 0 and no `/` are the whole address.
#[test]
fn no_proxy_cidr_bits_are_read_as_curl_reads_them() {
    for (list, matches) in [
        ("127.0.0.0/24", true),
        ("127.0.0.1", true),
        ("127.0.0.1/0", true),
        ("127.0.0.2/0", false),
        ("127.0.0.1/", false),
        ("127.0.0.1/abc", false),
        ("127.0.0.0/24abc", false),
        ("127.0.0.1/32abc", false),
        ("127.0.0.1/33", false),
        ("127.0.0.1/99999999999999999999", false),
    ] {
        assert_eq!(no_proxy_match("127.0.0.1", list), matches, "{list}");
    }
    assert!(no_proxy_match("[::1]", "::1/128"));
    assert!(!no_proxy_match("[::1]", "::1/129"));
    assert!(!no_proxy_match("[::1]", "::1/"));
}

/// QNetworkAccessManager's order and letter case: `Host`, the caller's headers as listed,
/// `Content-Length` before `Connection`; the head keeps its length and the body its bytes.
#[test]
fn a_request_head_goes_out_in_the_callers_order() {
    let mut out = b"POST /p HTTP/1.1\r\ncontent-length: 4\r\nhost: h\r\nx-b: 2\r\nuser-agent: u\r\nconnection: Keep-Alive\r\nx-a: 1\r\n\r\nbody".to_vec();
    let names = ["X-A", "X-B", "Connection", "User-Agent"].map(String::from);
    order_head(&mut out, &names);
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "POST /p HTTP/1.1\r\nHost: h\r\nX-A: 1\r\nX-B: 2\r\nContent-Length: 4\r\nConnection: Keep-Alive\r\nUser-Agent: u\r\n\r\nbody"
    );
}

/// The reply's header lines are read once the empty line after them is there, in the
/// order and with the names the server sent; the head ends where the body starts.
#[test]
fn a_replys_header_lines_are_the_servers() {
    let head = b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Encoding: gzip\r\n";
    assert_eq!(header_lines(head), None);
    let mut whole = head.to_vec();
    whole.extend_from_slice(b"X: a: b\r\n\r\nbody\r\n\r\n");
    let pair = |k: &str, v: &str| (k.to_string(), v.to_string());
    assert_eq!(
        header_lines(&whole),
        Some((
            vec![
                pair("Content-Type", "text/plain"),
                pair("Content-Encoding", "gzip"),
                pair("X", "a: b"),
            ],
            whole.len() - "body\r\n\r\n".len()
        ))
    );
}
