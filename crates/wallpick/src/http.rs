//! Blocking HTTP with timeouts, size limits and cancellation (ureq 3).

use crate::Cancel;
use crate::error::{Error, Result};
use std::io::Read;
use std::sync::OnceLock;
use std::time::Duration;

const USER_AGENT: &str = concat!(
    "wallpick/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/dar1k-ness/end-4-golang)"
);
pub(crate) const JSON_LIMIT: u64 = 8 << 20;
pub(crate) const THUMB_LIMIT: u64 = 24 << 20;
pub(crate) const IMAGE_LIMIT: u64 = 128 << 20;

/// How long a request waits for someone else's silence to end: the name lookup, the TCP
/// handshake, the response's head and each read of its body. A wallpaper site that hangs sends
/// nothing, and the worker thread of the pick must not wait for it for ever.
pub(crate) struct Limits {
    pub lookup: Duration,
    pub connect: Duration,
    pub answer: Duration,
    pub read: Duration,
}

const LOOKUP: Duration = Duration::from_secs(15);
const CONNECT: Duration = Duration::from_secs(15);
const ANSWER: Duration = Duration::from_secs(30);
const BODY_STALL: Duration = Duration::from_secs(30);

pub(crate) fn agent_with(limits: &Limits) -> ureq::Agent {
    ureq::Agent::config_builder()
        .user_agent(USER_AGENT)
        .timeout_resolve(Some(limits.lookup))
        .timeout_connect(Some(limits.connect))
        .timeout_recv_response(Some(limits.answer))
        .timeout_per_read(Some(limits.read))
        .max_redirects(8)
        .build()
        .into()
}

fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        agent_with(&Limits {
            lookup: LOOKUP,
            connect: CONNECT,
            answer: ANSWER,
            read: BODY_STALL,
        })
    })
}

fn http_err(url: &str) -> impl FnOnce(ureq::Error) -> Error + '_ {
    move |e| Error::Http {
        url: url.to_string(),
        source: Box::new(e),
    }
}

/// Streams the body of `url` into `sink` in chunks, at most `limit` bytes; returns the length.
pub(crate) fn stream(
    url: &str,
    limit: u64,
    cancel: &Cancel,
    sink: impl FnMut(&[u8]) -> std::io::Result<()>,
) -> Result<u64> {
    stream_with(agent(), url, limit, cancel, sink)
}

fn stream_with(
    agent: &ureq::Agent,
    url: &str,
    limit: u64,
    cancel: &Cancel,
    mut sink: impl FnMut(&[u8]) -> std::io::Result<()>,
) -> Result<u64> {
    cancel.check()?;
    let resp = agent.get(url).call().map_err(http_err(url))?;
    if resp.body().content_length().is_some_and(|n| n > limit) {
        return Err(Error::TooLarge {
            url: url.to_string(),
            limit,
        });
    }
    let mut reader = resp.into_body().into_reader();
    let mut buf = vec![0u8; 64 << 10];
    let mut total = 0u64;
    loop {
        cancel.check()?;
        let n = match reader.read(&mut buf) {
            Ok(0) => return Ok(total),
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(http_err(url)(ureq::Error::Io(e))),
        };
        total += n as u64;
        if total > limit {
            return Err(Error::TooLarge {
                url: url.to_string(),
                limit,
            });
        }
        sink(&buf[..n]).map_err(|e| http_err(url)(ureq::Error::Io(e)))?;
    }
}

pub(crate) fn bytes(url: &str, limit: u64, cancel: &Cancel) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    stream(url, limit, cancel, |c| {
        out.extend_from_slice(c);
        Ok(())
    })?;
    Ok(out)
}

pub(crate) fn json(url: &str, cancel: &Cancel) -> Result<serde_json::Value> {
    let b = bytes(url, JSON_LIMIT, cancel)?;
    serde_json::from_slice(&b).map_err(|source| Error::Json {
        what: url.to_string(),
        source,
    })
}

/// RFC 3986 percent-encoding of everything but the unreserved characters.
pub(crate) fn encode(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                o.push(b as char)
            }
            _ => o.push_str(&format!("%{b:02X}")),
        }
    }
    o
}

#[cfg(test)]
pub(crate) mod fake {
    //! A loopback HTTP/1.1 server answering canned routes, for tests without real network.

    use std::collections::HashMap;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    pub(crate) struct Server {
        pub base: String,
        pub hits: Arc<Mutex<Vec<String>>>,
    }

    type Routes = Arc<Mutex<HashMap<String, (u16, Vec<u8>)>>>;

    pub(crate) fn serve(routes: HashMap<String, (u16, Vec<u8>)>) -> Server {
        serve_dynamic(Arc::new(Mutex::new(routes)))
    }

    /// Routes match on the path without the query; the full request target is recorded in `hits`.
    pub(crate) fn serve_dynamic(routes: Routes) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let base = format!("http://{}", listener.local_addr().expect("addr"));
        let hits = Arc::new(Mutex::new(Vec::new()));
        let h = hits.clone();
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                let Ok(mut conn) = conn else { continue };
                let mut r = BufReader::new(conn.try_clone().expect("clone"));
                let mut line = String::new();
                if r.read_line(&mut line).is_err() {
                    continue;
                }
                loop {
                    let mut hdr = String::new();
                    if r.read_line(&mut hdr).map_or(true, |n| n == 0) || hdr == "\r\n" {
                        break;
                    }
                }
                let target = line.split(' ').nth(1).unwrap_or("/").to_string();
                h.lock().unwrap().push(target.clone());
                let path = target.split('?').next().unwrap_or("/");
                let (code, body) = routes
                    .lock()
                    .unwrap()
                    .get(path)
                    .cloned()
                    .unwrap_or((404, b"nope".to_vec()));
                let head = format!(
                    "HTTP/1.1 {code} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = conn.write_all(head.as_bytes());
                let _ = conn.write_all(&body);
            }
        });
        Server { base, hits }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;

    /// A server that reads one request, writes `answer` and then says nothing more, keeping
    /// the connection open until the returned sender is dropped.
    fn silent_after(answer: &'static [u8]) -> (String, mpsc::Sender<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let url = format!("http://{}/x", listener.local_addr().expect("addr"));
        let (held, release) = mpsc::channel::<()>();
        std::thread::spawn(move || {
            let (mut conn, _) = listener.accept().expect("accept");
            let mut r = BufReader::new(conn.try_clone().expect("clone"));
            loop {
                let mut hdr = String::new();
                if r.read_line(&mut hdr).map_or(true, |n| n == 0) || hdr == "\r\n" {
                    break;
                }
            }
            let _ = conn.write_all(answer);
            let _ = conn.flush();
            // Ends when the test drops its sender.
            let _ = release.recv();
        });
        (url, held)
    }

    fn short(answer: u64, read: u64) -> ureq::Agent {
        agent_with(&Limits {
            lookup: LOOKUP,
            connect: CONNECT,
            answer: Duration::from_millis(answer),
            read: Duration::from_millis(read),
        })
    }

    #[test]
    fn a_server_that_never_answers_is_given_up_at_the_limit() {
        let (url, _held) = silent_after(b"");
        let got = stream_with(
            &short(100, 30_000),
            &url,
            JSON_LIMIT,
            &Cancel::default(),
            |_| Ok(()),
        );
        assert!(matches!(got, Err(Error::Http { .. })), "{got:?}");
    }

    #[test]
    fn a_body_that_stalls_fails_the_download() {
        let (url, _held) = silent_after(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nabc");
        let mut seen = Vec::new();
        let got = stream_with(
            &short(30_000, 100),
            &url,
            JSON_LIMIT,
            &Cancel::default(),
            |c| {
                seen.extend_from_slice(c);
                Ok(())
            },
        );
        assert!(matches!(got, Err(Error::Http { .. })), "{got:?}");
        assert_eq!(seen, b"abc");
    }
}
