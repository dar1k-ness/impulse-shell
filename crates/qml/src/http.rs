//! In-process HTTP client (ureq + rustls) for what the original's QML does through `curl`.
//! Each request runs on the caller's worker thread; nothing here spawns processes.
use std::io::Read;
use std::sync::OnceLock;

pub use ureq::Error;

/// curl's transfer options the original relies on.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct Opts {
    /// `-L`: follow redirects (curl's default limit is 50).
    pub follow: bool,
    /// `-4`: resolve IPv4 addresses only.
    pub ipv4: bool,
    /// Give up when connecting or a read/write stalls for [`STALL`] (curl waits forever).
    pub stall: bool,
    /// Give up when the response's head, or then its body, takes longer than this to arrive
    /// ([`Error::Timeout`]).
    pub answer_within: Option<std::time::Duration>,
}

/// How long a [`Opts::stall`] request waits for the server before failing.
pub const STALL: std::time::Duration = std::time::Duration::from_secs(30);

fn agent(o: Opts) -> &'static ureq::Agent {
    static AGENTS: [OnceLock<ureq::Agent>; 8] = [const { OnceLock::new() }; 8];
    AGENTS[o.follow as usize | (o.ipv4 as usize) << 1 | (o.stall as usize) << 2].get_or_init(|| {
        let cfg = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .max_redirects(if o.follow { 50 } else { 0 })
            .max_redirects_will_error(false)
            // Chosen per request by `env_proxy`: ureq's own env lookup is not curl's.
            .proxy(None)
            .ip_family(if o.ipv4 {
                ureq::config::IpFamily::Ipv4Only
            } else {
                ureq::config::IpFamily::Any
            })
            .build();
        // curl's connector order: forwarding / CONNECT proxy, TCP (happy eyeballs), then TLS.
        use ureq::unversioned::transport::{ConnectProxyConnector, Connector, RustlsConnector};
        let connector = ()
            .chain(ForwardProxy {
                tcp: HappyTcp {
                    stall: o.stall.then_some(STALL),
                },
            })
            .chain(ConnectProxyConnector::default())
            .chain(HappyTcp {
                stall: o.stall.then_some(STALL),
            })
            .chain(RustlsConnector::default())
            .chain(StatusLine);
        ureq::Agent::with_parts(cfg, connector, CancelResolver)
    })
}

thread_local! {
    /// The status code and reason phrase of the last status line this thread's request read.
    static STATUS_LINE: std::cell::RefCell<Option<(u16, String)>> =
        const { std::cell::RefCell::new(None) };
    /// The header lines of that reply as the server sent them, once all of them were read.
    static HEADER_LINES: std::cell::RefCell<Option<Vec<(String, String)>>> =
        const { std::cell::RefCell::new(None) };
    /// The header names of this thread's next request, in the order and the letter case
    /// they go out in ([`send_as_qt`]).
    static HEADER_ORDER: std::cell::RefCell<Option<Vec<String>>> =
        const { std::cell::RefCell::new(None) };
    /// The server closed the connection of this thread's last request before the reply's
    /// body was whole, or reading from it failed.
    static PEER_CLOSED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// That reply's head gives its body no length (neither `Content-Length` nor
    /// `Transfer-Encoding`): the body is what comes until the server closes.
    static ENDS_AT_CLOSE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// What that reply's head says to the login of its request.
    static HEARD: std::cell::RefCell<Heard> = const {
        std::cell::RefCell::new(Heard {
            old: false,
            asks: Vec::new(),
        })
    };
    /// The bytes this thread's last request received after its reply's head.
    static BODY_RECEIVED: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Last in the chain, so that it sees what the server sent after TLS: notes each reply's
/// status line for [`Response::reason`], which ureq's parser drops.
#[derive(Debug)]
struct StatusLine;

impl<In: ureq::unversioned::transport::Transport> ureq::unversioned::transport::Connector<In>
    for StatusLine
{
    type Out = Noted<In>;

    fn connect(
        &self,
        _: &ureq::unversioned::transport::ConnectionDetails,
        chained: Option<In>,
    ) -> Result<Option<Self::Out>, Error> {
        Ok(chained.map(|t| Noted { t, sent: false }))
    }
}

#[derive(Debug)]
struct Noted<T> {
    t: T,
    /// A request went out and its reply's head has not been read to its end yet.
    sent: bool,
}

/// The header lines of the reply head `input` starts with, once its closing empty line is
/// there, and how long that head is: ureq drops `Content-Encoding` and `Content-Length` of
/// what it decompresses.
fn header_lines(input: &[u8]) -> Option<(Vec<(String, String)>, usize)> {
    let end = (0..input.len())
        .find(|&i| input[i..].starts_with(b"\r\n\r\n") || input[i..].starts_with(b"\n\n"))?;
    let head = end + if input[end] == b'\r' { 4 } else { 2 };
    let lines = input[..end].split(|b| *b == b'\n').skip(1);
    let lines = lines
        .filter_map(|line| {
            let line = String::from_utf8_lossy(line);
            let (name, value) = line.split_once(':')?;
            Some((name.trim().to_string(), value.trim().to_string()))
        })
        .collect();
    Some((lines, head))
}

/// `(404, "Not Here")` of `HTTP/1.1 404 Not Here`, once the whole line is in `input`.
fn status_line(input: &[u8]) -> Option<(u16, String)> {
    let line = &input[..input.iter().position(|b| *b == b'\n')?];
    let line = String::from_utf8_lossy(line.strip_suffix(b"\r").unwrap_or(line));
    let (code, reason) = line
        .strip_prefix("HTTP/")?
        .split_once(' ')?
        .1
        .split_at_checked(3)?;
    Some((
        code.parse().ok()?,
        reason.strip_prefix(' ').unwrap_or(reason).into(),
    ))
}

/// What a reply's head says to the [`login`] of its request, as its bytes came.
#[derive(Default)]
pub(crate) struct Heard {
    /// The reply is HTTP/1.0.
    pub(crate) old: bool,
    /// Its `WWW-Authenticate` lines after the name, a char for each byte.
    pub(crate) asks: Vec<String>,
}

impl Heard {
    fn of(head: &[u8]) -> Heard {
        let mut lines = head
            .split(|b| *b == b'\n')
            .map(|l| l.strip_suffix(b"\r").unwrap_or(l));
        let old = lines.next().is_some_and(|l| l.starts_with(b"HTTP/1.0 "));
        let asks = lines
            .filter_map(|l| {
                let colon = l.iter().position(|b| *b == b':')?;
                let (name, value) = (l[..colon].trim_ascii(), l[colon + 1..].trim_ascii());
                name.eq_ignore_ascii_case(b"www-authenticate")
                    .then(|| value.iter().map(|&b| char::from(b)).collect())
            })
            .collect();
        Heard { old, asks }
    }
}

/// [`Heard`] of this thread's last reply.
pub(crate) fn heard() -> Heard {
    HEARD.take()
}

/// The name a login's `Authorization` is given to ureq under, as long as that one: ureq
/// sends no `Authorization` with a byte that is not ASCII, and QAuthenticator's Latin-1 has
/// them. [`order_head`] writes the right name.
const LOGIN_AS: &str = "x-login-bytes";

/// Puts the header lines of the request head `out` starts with in the order of `names` and
/// in their letter case, `Host` before them and `Content-Length` before `Connection`, as
/// QNetworkAccessManager sends them. ureq writes them in its own order, names in small
/// letters; the lines only change places and case, so the head keeps its length.
fn order_head(out: &mut [u8], names: &[String]) {
    let Some(end) = out.windows(4).position(|w| w == b"\r\n\r\n") else {
        return;
    };
    let Some(first) = out[..end].windows(2).position(|w| w == b"\r\n") else {
        return;
    };
    let name_of = |line: &[u8]| -> Vec<u8> {
        let n = line.iter().position(|b| *b == b':').unwrap_or(line.len());
        line[..n].to_ascii_lowercase()
    };
    let mut lines: Vec<Vec<u8>> = out[first + 2..end]
        .split(|b| *b == b'\n')
        .map(|l| l.strip_suffix(b"\r").unwrap_or(l).to_vec())
        .collect();
    for line in lines
        .iter_mut()
        .filter(|l| name_of(l) == LOGIN_AS.as_bytes())
    {
        line[..LOGIN_AS.len()].copy_from_slice(b"authorization");
    }
    let take = |name: &str, lines: &mut Vec<Vec<u8>>, to: &mut Vec<u8>| {
        let wanted = name.to_ascii_lowercase();
        if let Some(i) = lines.iter().position(|l| name_of(l) == wanted.as_bytes()) {
            let line = lines.remove(i);
            to.extend_from_slice(name.as_bytes());
            to.extend_from_slice(&line[name.len()..]);
            to.extend_from_slice(b"\r\n");
        }
    };
    let mut head = Vec::with_capacity(end - first);
    take("Host", &mut lines, &mut head);
    for name in names {
        if name.eq_ignore_ascii_case("connection") {
            take("Content-Length", &mut lines, &mut head);
        }
        take(name, &mut lines, &mut head);
    }
    for line in lines {
        head.extend_from_slice(&line);
        head.extend_from_slice(b"\r\n");
    }
    if head.len() == end - first {
        out[first + 2..end + 2].copy_from_slice(&head);
    }
}

impl<T: ureq::unversioned::transport::Transport> ureq::unversioned::transport::Transport
    for Noted<T>
{
    fn buffers(&mut self) -> &mut dyn ureq::unversioned::transport::Buffers {
        self.t.buffers()
    }

    fn transmit_output(
        &mut self,
        amount: usize,
        timeout: ureq::unversioned::transport::NextTimeout,
    ) -> Result<(), Error> {
        // A request head, not the `CONNECT` to a proxy before it nor a part of a body.
        if !self.sent
            && let Some(names) = HEADER_ORDER.take()
        {
            let out = &mut self.t.buffers().output()[..amount];
            if out.starts_with(b"CONNECT ") {
                HEADER_ORDER.set(Some(names));
            } else {
                order_head(out, &names);
            }
        }
        self.sent = true;
        self.t.transmit_output(amount, timeout)
    }

    fn await_input(
        &mut self,
        timeout: ureq::unversioned::transport::NextTimeout,
    ) -> Result<bool, Error> {
        let had = self.t.buffers().input().len();
        let more = self
            .t
            .await_input(timeout)
            .inspect_err(|_| PEER_CLOSED.set(true))?;
        if !more && !ENDS_AT_CLOSE.get() {
            PEER_CLOSED.set(true);
        }
        let input = self.t.buffers().input();
        if !self.sent {
            let came = input.len().saturating_sub(had) as u64;
            BODY_RECEIVED.set(BODY_RECEIVED.get() + came);
        } else if let Some(line) = status_line(input) {
            STATUS_LINE.set(Some(line));
            if let Some((lines, head)) = header_lines(input) {
                self.sent = false;
                ENDS_AT_CLOSE.set(!lines.iter().any(|(name, _)| {
                    name.eq_ignore_ascii_case("content-length")
                        || name.eq_ignore_ascii_case("transfer-encoding")
                }));
                HEADER_LINES.set(Some(lines));
                HEARD.set(Heard::of(&input[..head]));
                BODY_RECEIVED.set((input.len() - head) as u64);
            }
        }
        Ok(more)
    }

    fn is_open(&mut self) -> bool {
        self.t.is_open()
    }

    fn is_tls(&self) -> bool {
        self.t.is_tls()
    }
}

/// curl's `--happy-eyeballs-timeout-ms` default.
const HAPPY_EYEBALLS: std::time::Duration = std::time::Duration::from_millis(200);

/// TCP connect as curl does it (RFC 8305): the resolved addresses interleaved by family, a
/// new attempt started every 200 ms or as soon as one fails, the first to connect wins.
/// ureq alone tries them one after another, so a dropped IPv6 route stalls every request
/// for the kernel's SYN timeout (~2 min per address) before it reaches IPv4.
#[derive(Debug)]
struct HappyTcp {
    stall: Option<std::time::Duration>,
}

impl<In: ureq::unversioned::transport::Transport> ureq::unversioned::transport::Connector<In>
    for HappyTcp
{
    type Out = ureq::unversioned::transport::Either<In, Tcp>;

    fn connect(
        &self,
        details: &ureq::unversioned::transport::ConnectionDetails,
        chained: Option<In>,
    ) -> Result<Option<Self::Out>, Error> {
        use ureq::unversioned::transport::{Either, LazyBuffers};
        if let Some(c) = chained {
            return Ok(Some(Either::A(c)));
        }
        let cfg = details.config;
        // The caller's limit (`stall`, else the request's timeout), none when it set neither:
        // then the kernel's SYN retries (`tcp_syn_retries`) end a handshake that never
        // completes, and `Cancel::stop` one the caller gave up on.
        let limit = self.stall.or(details.timeout.not_zero().map(|t| *t));
        let cancel = CANCEL.with(|c| c.borrow().clone());
        let connect = happy_connect(&details.addrs, HAPPY_EYEBALLS, |a| connect_within(a, limit));
        #[cfg(test)]
        if cancel.is_some() {
            CONNECTING.with(|c| c.borrow().as_ref().map(|f| f()));
        }
        let stream = async_io::block_on(async {
            use futures_lite::FutureExt;
            match &cancel {
                Some(c) => {
                    connect
                        .or(async {
                            let _ = c.woken.recv().await;
                            Err(cancelled())
                        })
                        .await
                }
                None => connect.await,
            }
        })?;
        if cfg.no_delay() {
            stream.set_nodelay(true)?;
        }
        stream.set_read_timeout(self.stall)?;
        stream.set_write_timeout(self.stall)?;
        keepalive(&stream)?;
        let buffers = LazyBuffers::new(cfg.input_buffer_size(), cfg.output_buffer_size());
        let mut t = Tcp {
            s: stream,
            buffers,
            armed: Default::default(),
            stall: self.stall,
        };
        t.arm()?;
        Ok(Some(Either::B(t)))
    }
}

/// One TCP handshake, given up after `limit`. A SYN that a dropped route swallows gets no
/// answer at all, so only a clock ends the attempt (the kernel's own gives up after ~2 min).
async fn connect_within(
    a: std::net::SocketAddr,
    limit: Option<std::time::Duration>,
) -> std::io::Result<std::net::TcpStream> {
    use futures_lite::FutureExt;
    let c = async_io::Async::<std::net::TcpStream>::connect(a);
    let s = match limit {
        Some(t) => {
            c.or(async {
                async_io::Timer::after(t).await;
                Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "connection timed out",
                ))
            })
            .await
        }
        None => c.await,
    }?;
    let s = s.into_inner()?;
    s.set_nonblocking(false)?;
    Ok(s)
}

/// TCP keepalive: a peer that vanished (suspend, network change) is noticed after
/// [`KEEPIDLE`] + [`KEEPCNT`] x [`KEEPINTVL`] of silence (30 s; curl's 60 s x 9 takes ten
/// minutes) while a live but slow server, e.g. an AI model thinking, is never cut off.
const KEEPIDLE: libc::c_int = 15;
const KEEPINTVL: libc::c_int = 5;
const KEEPCNT: libc::c_int = 3;

fn keepalive(s: &std::net::TcpStream) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;
    let set = |level, opt, v: libc::c_int| {
        // SAFETY: the fd is the live socket `s`; the option value is `v`, a live c_int, with its
        // size as the length.
        let r = unsafe {
            libc::setsockopt(
                s.as_raw_fd(),
                level,
                opt,
                (&v as *const libc::c_int).cast(),
                std::mem::size_of::<libc::c_int>() as libc::socklen_t,
            )
        };
        if r == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    };
    set(libc::SOL_SOCKET, libc::SO_KEEPALIVE, 1)?;
    set(libc::IPPROTO_TCP, libc::TCP_KEEPIDLE, KEEPIDLE)?;
    set(libc::IPPROTO_TCP, libc::TCP_KEEPINTVL, KEEPINTVL)?;
    set(libc::IPPROTO_TCP, libc::TCP_KEEPCNT, KEEPCNT)
}

/// The [`with_stop`] scope of the thread making requests.
struct Cancel {
    /// Clones of the sockets used in this scope, shut down on stop.
    socks: std::sync::Mutex<Vec<std::net::TcpStream>>,
    stopped: std::sync::atomic::AtomicBool,
    /// Closed on stop: wakes a pending connect.
    woken: async_channel::Receiver<()>,
    _open: async_channel::Sender<()>,
    /// What [`on_stop`] left to run on stop.
    hooks: std::sync::Mutex<Vec<Box<dyn FnOnce() + Send>>>,
}

impl Cancel {
    fn stop(&self) {
        self.stopped
            .store(true, std::sync::atomic::Ordering::Release);
        // Before the sockets: a hook sees the request as it was when it was stopped.
        let hooks = std::mem::take(&mut *self.hooks.lock().unwrap_or_else(|e| e.into_inner()));
        hooks.into_iter().for_each(|f| f());
        self.woken.close();
        for s in self
            .socks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .drain(..)
        {
            let _ = s.shutdown(std::net::Shutdown::Both);
        }
    }
}

/// Runs `f` when the [`with_stop`] scope this thread is in is stopped, on the thread that
/// stops it and before the stop returns; never outside such a scope.
fn on_stop(f: impl FnOnce() + Send + 'static) {
    let Some(c) = CANCEL.with(|c| c.borrow().clone()) else {
        return;
    };
    let mut hooks = c.hooks.lock().unwrap_or_else(|e| e.into_inner());
    match c.stopped.load(std::sync::atomic::Ordering::Acquire) {
        true => {
            drop(hooks);
            f();
        }
        false => hooks.push(Box::new(f)),
    }
}

/// Whether the [`with_stop`] scope this thread is in was stopped.
fn stopped() -> bool {
    CANCEL.with(|c| {
        c.borrow()
            .as_ref()
            .is_some_and(|c| c.stopped.load(std::sync::atomic::Ordering::Acquire))
    })
}

thread_local! {
    static CANCEL: std::cell::RefCell<Option<std::sync::Arc<Cancel>>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
thread_local! {
    /// Runs when a connect inside a [`with_stop`] scope starts waiting (tests).
    static CONNECTING: std::cell::RefCell<Option<Box<dyn Fn()>>> = const { std::cell::RefCell::new(None) };
}

fn cancelled() -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::Interrupted, "cancelled")
}

/// Run `f` with every request it makes on this thread bound to `stop`, like curl to its
/// SIGTERM: a pending connect fails at once and the sockets in use are shut down, so a
/// blocked read or write returns. Pooled connections reused later are not affected.
pub fn with_stop<R>(stop: &crate::svc::Stop, f: impl FnOnce() -> R) -> R {
    let (open, woken) = async_channel::bounded(1);
    let c = std::sync::Arc::new(Cancel {
        socks: Default::default(),
        stopped: Default::default(),
        woken,
        _open: open,
        hooks: Default::default(),
    });
    let weak = std::sync::Arc::downgrade(&c);
    let _wake = stop.on_stop(move || {
        if let Some(c) = weak.upgrade() {
            c.stop();
        }
    });
    struct Reset(Option<std::sync::Arc<Cancel>>);
    impl Drop for Reset {
        fn drop(&mut self) {
            let prev = self.0.take();
            CANCEL.with(|c| *c.borrow_mut() = prev);
        }
    }
    let _reset = Reset(CANCEL.with(|cur| cur.replace(Some(c))));
    f()
}

/// ureq's resolver, with the lookup moved to a helper thread inside a [`with_stop`] scope so a
/// stop does not wait for a hanging `getaddrinfo`.
#[derive(Debug)]
struct CancelResolver;

impl ureq::unversioned::resolver::Resolver for CancelResolver {
    fn resolve(
        &self,
        uri: &ureq::http::Uri,
        config: &ureq::config::Config,
        timeout: ureq::unversioned::transport::NextTimeout,
    ) -> Result<ureq::unversioned::resolver::ResolvedSocketAddrs, Error> {
        use ureq::unversioned::resolver::DefaultResolver;
        let (uri, config) = (uri.clone(), config.clone());
        cancellable(move || DefaultResolver::default().resolve(&uri, &config, timeout))
    }
}

/// `f()`, on a helper thread inside a [`with_stop`] scope so that a stop returns at once.
fn cancellable<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, Error> + Send + 'static,
) -> Result<T, Error> {
    let Some(c) = CANCEL.with(|c| c.borrow().clone()) else {
        return f();
    };
    let (tx, rx) = async_channel::bounded(1);
    workpool::long(move || {
        let _ = tx.try_send(f());
    });
    async_io::block_on(futures_lite::FutureExt::or(
        async { rx.recv().await.unwrap_or_else(|_| Err(cancelled().into())) },
        async {
            let _ = c.woken.recv().await;
            Err(cancelled().into())
        },
    ))
}

/// ureq's `TcpTransport` (not public) minus ureq's timeouts: the agents set none, like the
/// curl calls they replace; [`Opts::stall`] is a socket timeout set at connect.
#[derive(Debug)]
struct Tcp {
    s: std::net::TcpStream,
    buffers: ureq::unversioned::transport::LazyBuffers,
    /// The [`with_stop`] scope this socket is registered with.
    armed: std::sync::Weak<Cancel>,
    /// The socket's read timeout ([`Opts::stall`]).
    stall: Option<std::time::Duration>,
}

impl std::fmt::Debug for Cancel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Cancel")
    }
}

impl Tcp {
    /// Register with the current [`with_stop`] scope, once per scope (a pooled connection
    /// can serve several); fail if it was already stopped.
    fn arm(&mut self) -> std::io::Result<()> {
        let Some(c) = CANCEL.with(|c| c.borrow().clone()) else {
            return Ok(());
        };
        if std::ptr::eq(self.armed.as_ptr(), std::sync::Arc::as_ptr(&c)) {
            return Ok(());
        }
        let mut socks = c.socks.lock().unwrap_or_else(|e| e.into_inner());
        if c.stopped.load(std::sync::atomic::Ordering::Acquire) {
            return Err(cancelled());
        }
        socks.push(self.s.try_clone()?);
        self.armed = std::sync::Arc::downgrade(&c);
        Ok(())
    }
}

impl ureq::unversioned::transport::Transport for Tcp {
    fn buffers(&mut self) -> &mut dyn ureq::unversioned::transport::Buffers {
        &mut self.buffers
    }

    fn transmit_output(
        &mut self,
        amount: usize,
        _: ureq::unversioned::transport::NextTimeout,
    ) -> Result<(), Error> {
        use std::io::Write;
        use ureq::unversioned::transport::Buffers;
        self.arm()?;
        Ok((&self.s).write_all(&self.buffers.output()[..amount])?)
    }

    fn await_input(
        &mut self,
        timeout: ureq::unversioned::transport::NextTimeout,
    ) -> Result<bool, Error> {
        use std::io::ErrorKind::{Interrupted, TimedOut, WouldBlock};
        use ureq::unversioned::transport::Buffers;
        self.arm()?;
        // The request's own limit ([`Opts::answer_within`]), when it is the shorter one.
        let limit = timeout
            .not_zero()
            .map(|t| *t)
            .filter(|t| self.stall.is_none_or(|s| *t < s));
        if limit.is_some() {
            self.s.set_read_timeout(limit)?;
        }
        let read = loop {
            match (&self.s).read(self.buffers.input_append_buf()) {
                Err(e) if e.kind() == Interrupted => continue,
                r => break r,
            }
        };
        if limit.is_some() {
            self.s.set_read_timeout(self.stall)?;
        }
        let n = match read {
            Err(e) if limit.is_some() && matches!(e.kind(), TimedOut | WouldBlock) => {
                return Err(Error::Timeout(timeout.reason));
            }
            r => r?,
        };
        self.buffers.input_appended(n);
        Ok(n > 0)
    }

    fn is_open(&mut self) -> bool {
        let probe = || -> std::io::Result<bool> {
            self.s.set_nonblocking(true)?;
            let open = matches!((&self.s).read(&mut [0]), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock);
            self.s.set_nonblocking(false)?;
            Ok(open)
        };
        probe().unwrap_or(false)
    }
}

/// curl's plain-HTTP proxying for `http://` URLs: one absolute-form request to the proxy
/// (RFC 9112 3.2.2). ureq would tunnel them through CONNECT, which proxies commonly refuse
/// for port 80 (squid's default `http_access deny CONNECT !SSL_ports`); `https://` keeps it.
#[derive(Debug)]
struct ForwardProxy {
    tcp: HappyTcp,
}

impl<In: ureq::unversioned::transport::Transport> ureq::unversioned::transport::Connector<In>
    for ForwardProxy
{
    type Out = ureq::unversioned::transport::Either<In, Forward>;

    fn connect(
        &self,
        details: &ureq::unversioned::transport::ConnectionDetails,
        chained: Option<In>,
    ) -> Result<Option<Self::Out>, Error> {
        use ureq::unversioned::transport::{ConnectionDetails, Connector, Either};
        if let Some(c) = chained {
            return Ok(Some(Either::A(c)));
        }
        let Some(proxy) = details.config.proxy() else {
            return Ok(None);
        };
        if !matches!(proxy.protocol(), ureq::ProxyProtocol::Http)
            || details.uri.scheme_str() != Some("http")
        {
            return Ok(None);
        }
        let proxied = ConnectionDetails {
            uri: proxy.uri(),
            addrs: details
                .resolver
                .resolve(proxy.uri(), details.config, details.timeout)?,
            config: details.config,
            request_level: details.request_level,
            resolver: details.resolver,
            now: details.now,
            timeout: details.timeout,
            current_time: details.current_time.clone(),
            run_connector: details.run_connector.clone(),
            register_shutdown: details.register_shutdown.clone(),
        };
        let Some(Either::B(tcp)) = Connector::<Tcp>::connect(&self.tcp, &proxied, None)? else {
            unreachable!("HappyTcp connects when nothing is chained");
        };
        let host = details.uri.host().unwrap_or_default();
        let origin = match details.uri.port_u16() {
            Some(p) => format!("http://{host}:{p}"),
            None => format!("http://{host}"),
        };
        let auth = (proxy.username().is_some() || proxy.password().is_some()).then(|| {
            use base64::Engine;
            let creds = format!(
                "{}:{}",
                proxy.username().unwrap_or_default(),
                proxy.password().unwrap_or_default()
            );
            base64::engine::general_purpose::STANDARD.encode(creds)
        });
        Ok(Some(Either::B(Forward {
            tcp,
            origin,
            auth,
            at_request: true,
        })))
    }
}

/// [`Tcp`] to a forwarding proxy: each request's origin-form target is sent absolute, with
/// the proxy's credentials (curl's `Proxy-Authorization: Basic`).
#[derive(Debug)]
struct Forward {
    tcp: Tcp,
    origin: String,
    auth: Option<String>,
    /// The next output starts a request (nothing sent since the last response arrived).
    at_request: bool,
}

/// Where the target of `METHOD /path HTTP/1.1\r\n...` starts.
fn origin_form_target(b: &[u8]) -> Option<usize> {
    let sp = b.iter().position(|&c| c == b' ')?;
    (sp > 0 && b[..sp].iter().all(u8::is_ascii_uppercase) && b.get(sp + 1) == Some(&b'/'))
        .then_some(sp + 1)
}

impl ureq::unversioned::transport::Transport for Forward {
    fn buffers(&mut self) -> &mut dyn ureq::unversioned::transport::Buffers {
        self.tcp.buffers()
    }

    fn transmit_output(
        &mut self,
        amount: usize,
        timeout: ureq::unversioned::transport::NextTimeout,
    ) -> Result<(), Error> {
        use std::io::Write;
        use ureq::unversioned::transport::Buffers;
        let out = &self.tcp.buffers.output()[..amount];
        let start = std::mem::take(&mut self.at_request);
        let (Some(t), Some(eol)) = (
            origin_form_target(out).filter(|_| start),
            out.windows(2).position(|w| w == b"\r\n"),
        ) else {
            return self.tcp.transmit_output(amount, timeout);
        };
        let mut v = Vec::with_capacity(amount + self.origin.len() + 64);
        v.extend_from_slice(&out[..t]);
        v.extend_from_slice(self.origin.as_bytes());
        v.extend_from_slice(&out[t..eol + 2]);
        if let Some(a) = &self.auth {
            v.extend_from_slice(format!("Proxy-Authorization: Basic {a}\r\n").as_bytes());
        }
        v.extend_from_slice(&out[eol + 2..]);
        Ok((&self.tcp.s).write_all(&v)?)
    }

    fn await_input(
        &mut self,
        timeout: ureq::unversioned::transport::NextTimeout,
    ) -> Result<bool, Error> {
        self.at_request = true;
        self.tcp.await_input(timeout)
    }

    fn is_open(&mut self) -> bool {
        self.tcp.is_open()
    }
}

/// RFC 8305 order: alternate address families, starting with the resolver's first choice.
fn interleave(addrs: &[std::net::SocketAddr]) -> Vec<std::net::SocketAddr> {
    let Some(first) = addrs.first() else {
        return vec![];
    };
    let (mut a, mut b): (Vec<_>, Vec<_>) =
        addrs.iter().partition(|x| x.is_ipv6() == first.is_ipv6());
    a.reverse();
    b.reverse();
    let mut o = Vec::with_capacity(addrs.len());
    while let Some(x) = a.pop() {
        o.push(x);
        o.extend(b.pop());
    }
    o.extend(b.into_iter().rev());
    o
}

/// Starts an attempt per address in [`interleave`] order, the next one `delay` after the last or
/// at once when an attempt fails, and returns the first success (or the last error). All
/// attempts run on the calling thread as futures; the losers are dropped (their sockets closed).
async fn happy_connect<T, F: std::future::Future<Output = std::io::Result<T>>>(
    addrs: &[std::net::SocketAddr],
    delay: std::time::Duration,
    attempt: impl Fn(std::net::SocketAddr) -> F,
) -> std::io::Result<T> {
    use std::task::Poll;
    let mut next = interleave(addrs).into_iter();
    let mut running: Vec<std::pin::Pin<Box<F>>> = vec![];
    let mut timer: Option<async_io::Timer> = None;
    let mut last = None;
    loop {
        if timer.is_none() {
            if let Some(a) = next.next() {
                running.push(Box::pin(attempt(a)));
                if next.len() > 0 {
                    timer = Some(async_io::Timer::after(delay));
                }
            } else if running.is_empty() {
                return Err(last.unwrap_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "Connection refused")
                }));
            }
        }
        let done = std::future::poll_fn(|cx| {
            for i in 0..running.len() {
                if let Poll::Ready(r) = running[i].as_mut().poll(cx) {
                    drop(running.swap_remove(i));
                    return Poll::Ready(Some(r));
                }
            }
            match timer.as_mut().map(|t| std::pin::Pin::new(t).poll(cx)) {
                Some(Poll::Ready(_)) => Poll::Ready(None),
                _ => Poll::Pending,
            }
        })
        .await;
        match done {
            Some(Ok(s)) => return Ok(s),
            Some(Err(e)) => last = Some(e),
            None => {}
        }
        timer = None;
    }
}

/// Most a body read into memory may hold (curl has no limit): AI replies and uploads.
pub const BODY_MAX: u64 = 64 << 20;
/// Most a JSON API reply (weather, model lists) may hold.
pub const JSON_MAX: u64 = 4 << 20;
/// Most an `XMLHttpRequest` reply may hold.
pub const XHR_MAX: u64 = 32 << 20;

/// A response whose body has not been read yet.
pub struct Response {
    pub status: u16,
    /// The status line's reason phrase as the server worded it ("" when it sent none).
    pub reason: String,
    pub headers: Vec<(String, String)>,
    pub body: Box<dyn Read + Send>,
}

impl Response {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// The whole body, at most [`BODY_MAX`] bytes.
    pub fn bytes(self) -> std::io::Result<Vec<u8>> {
        self.bytes_max(BODY_MAX)
    }

    /// The whole body; an error (nothing kept) once it passes `max` bytes, so a server that
    /// never stops sending (or a gzip bomb) cannot grow the shell without bound.
    pub fn bytes_max(mut self, max: u64) -> std::io::Result<Vec<u8>> {
        let mut v = vec![];
        (&mut self.body)
            .take(max.saturating_add(1))
            .read_to_end(&mut v)?;
        if v.len() as u64 > max {
            return Err(std::io::Error::new(
                std::io::ErrorKind::FileTooLarge,
                format!("body over {max} bytes"),
            ));
        }
        Ok(v)
    }

    /// The body as text, invalid UTF-8 replaced (what QML gets from curl's stdout).
    pub fn text(self) -> std::io::Result<String> {
        self.text_max(BODY_MAX)
    }

    /// [`Response::text`] of at most `max` bytes (see [`Response::bytes_max`]).
    pub fn text_max(self, max: u64) -> std::io::Result<String> {
        Ok(String::from_utf8_lossy(&self.bytes_max(max)?).into_owned())
    }
}

/// Send one request. HTTP error statuses are responses, not errors (curl without `-f`).
/// An empty `User-Agent` header sends none, as `curl -A ''` does; an empty `Content-Type`
/// sends the body without one, as `curl -H 'Content-Type:'` does.
pub fn send(
    method: &str,
    url: &str,
    headers: &[(String, String)],
    body: Option<&[u8]>,
    o: Opts,
) -> Result<Response, Error> {
    send_env(method, url, headers, body, o, &|k| std::env::var(k).ok())
}

/// The server closed the connection of this thread's last request before the reply's body
/// was whole, or reading it failed.
pub(crate) fn peer_closed() -> bool {
    PEER_CLOSED.get()
}

/// How many bytes of this thread's last request's reply came after its head: the body as
/// it travels, compressed and in its chunks.
pub(crate) fn body_received() -> u64 {
    BODY_RECEIVED.get()
}

/// [`send`] as QNetworkAccessManager sends, not curl: no `Accept: */*`, a body without a
/// `Content-Type` goes out without one, the headers go out in the order and the letter case
/// of `headers` (after `Host`, with `Content-Length` before `Connection`), any method is
/// sent, and the reply's headers are the lines the server sent, `Content-Encoding` of a
/// decompressed body among them.
#[cfg(test)]
pub(crate) fn send_as_qt(
    method: &str,
    url: &str,
    headers: &[(String, String)],
    body: Option<&[u8]>,
    o: Opts,
) -> Result<Response, Error> {
    send_with_login(method, url, headers, None, body, o)
}

/// [`send_as_qt`] with `login` as the `Authorization` after the other headers, in its
/// bytes: QAuthenticator writes a login in Latin-1.
pub(crate) fn send_with_login(
    method: &str,
    url: &str,
    headers: &[(String, String)],
    login: Option<&[u8]>,
    body: Option<&[u8]>,
    o: Opts,
) -> Result<Response, Error> {
    send_body(method, url, headers, body, o, Some(login), &|k| {
        std::env::var(k).ok()
    })
}

fn run<S: ureq::AsSendBody>(
    a: &ureq::Agent,
    req: ureq::http::Request<S>,
    proxy: Option<ureq::Proxy>,
    no_ua: bool,
    answer_within: Option<std::time::Duration>,
    qt: bool,
) -> Result<ureq::http::Response<ureq::Body>, Error> {
    let mut c = a.configure_request(req).proxy(proxy);
    if answer_within.is_some() {
        c = c
            .timeout_recv_response(answer_within)
            .timeout_recv_body(answer_within);
    }
    if no_ua {
        c = c.user_agent(ureq::config::AutoHeaderValue::None);
    }
    if qt {
        c = c
            .accept(ureq::config::AutoHeaderValue::None)
            .allow_non_standard_methods(true);
    }
    a.run(c.build())
}

/// The proxy curl takes from its environment for `url` (lib/url.c `detect_proxy`):
/// `no_proxy` (else `NO_PROXY`) wins, then `<scheme>_proxy`, its uppercase form except
/// `HTTP_PROXY`, then `all_proxy`, `ALL_PROXY`. Empty values count as unset.
fn env_proxy(url: &str, env: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    let get = |k: &str| env(k).filter(|v| !v.is_empty());
    let uri: ureq::http::Uri = url.parse().ok()?;
    let scheme = uri.scheme_str()?.to_ascii_lowercase();
    let host = uri.host()?;
    if let Some(np) = get("no_proxy").or_else(|| get("NO_PROXY"))
        && no_proxy_match(host, &np)
    {
        return None;
    }
    let var = format!("{scheme}_proxy");
    get(&var)
        .or_else(|| {
            (scheme != "http")
                .then(|| get(&var.to_ascii_uppercase()))
                .flatten()
        })
        .or_else(|| get("all_proxy"))
        .or_else(|| get("ALL_PROXY"))
}

/// curl's `Curl_check_noproxy`: `*` alone, else a comma/blank separated list of host
/// suffixes on a label boundary (leading/trailing dots ignored), or IPs with optional
/// CIDR bits for an IP host.
fn no_proxy_match(host: &str, list: &str) -> bool {
    use std::net::IpAddr;
    if list == "*" {
        return true;
    }
    let ip = host
        .strip_prefix('[')
        .and_then(|h| h.strip_suffix(']'))
        .unwrap_or(host)
        .parse::<IpAddr>()
        .ok();
    let name = host.strip_suffix('.').unwrap_or(host);
    list.split([',', ' ', '\t'])
        .filter(|t| !t.is_empty())
        .any(|tok| match ip {
            Some(ip) => {
                let (net, bits) = match tok.split_once('/') {
                    Some((n, b)) => match cidr_bits(b) {
                        Some(bits) => (n, bits),
                        None => return false,
                    },
                    None => (tok, 0),
                };
                match (ip, net.parse::<IpAddr>()) {
                    (IpAddr::V4(a), Ok(IpAddr::V4(n))) => {
                        cidr_match(&a.octets(), &n.octets(), bits)
                    }
                    (IpAddr::V6(a), Ok(IpAddr::V6(n))) => {
                        cidr_match(&a.octets(), &n.octets(), bits)
                    }
                    _ => false,
                }
            }
            None => {
                let t = tok.strip_suffix('.').unwrap_or(tok);
                let t = t.strip_prefix('.').unwrap_or(t);
                let (n, t) = (name.as_bytes(), t.as_bytes());
                if t.len() == n.len() {
                    n.eq_ignore_ascii_case(t)
                } else {
                    t.len() < n.len()
                        && n[n.len() - t.len() - 1] == b'.'
                        && n[n.len() - t.len()..].eq_ignore_ascii_case(t)
                }
            }
        })
}

/// The bits after the `/` of a `no_proxy` address, as curl 8.18 and later read them
/// (`curlx_str_number` up to 128, then the end of the token): digits only. No digits, anything
/// after them or a larger number is `None`, and such a token matches no host. (curl up to 8.17
/// used `atoi` there: no digits were 0, the whole address, and what followed digits was
/// ignored.)
fn cidr_bits(s: &str) -> Option<u32> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok().filter(|bits| *bits <= 128)
}

/// `bits` 0 means the whole address, as in curl.
fn cidr_match(a: &[u8], n: &[u8], bits: u32) -> bool {
    let all = a.len() as u32 * 8;
    if bits > all {
        return false;
    }
    let bits = if bits == 0 { all } else { bits };
    let (whole, rest) = ((bits / 8) as usize, bits % 8);
    a[..whole] == n[..whole] && (rest == 0 || (a[whole] ^ n[whole]) & (0xffu8 << (8 - rest)) == 0)
}

pub(crate) fn send_env(
    method: &str,
    url: &str,
    headers: &[(String, String)],
    body: Option<&[u8]>,
    o: Opts,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<Response, Error> {
    send_body(method, url, headers, body, o, None, env)
}

fn send_body<B: ureq::AsSendBody>(
    method: &str,
    url: &str,
    headers: &[(String, String)],
    body: Option<B>,
    o: Opts,
    // As Qt sends it, with this login ([`send_with_login`]).
    qt: Option<Option<&[u8]>>,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<Response, Error> {
    let (login, qt) = (qt.flatten(), qt.is_some());
    let method = if method.is_empty() { "GET" } else { method };
    let mut b = ureq::http::Request::builder()
        .method(method.to_ascii_uppercase().as_str())
        .uri(url);
    let mut no_ua = false;
    for (k, v) in headers {
        if k.eq_ignore_ascii_case("user-agent") && v.is_empty() {
            no_ua = true;
            continue;
        }
        // `curl -H 'Content-Type:'`: the body goes out unlabelled.
        if k.eq_ignore_ascii_case("content-type") && v.is_empty() {
            continue;
        }
        b = b.header(k.as_str(), v.as_str());
    }
    if let Some(login) = login {
        b = b.header(LOGIN_AS, login);
    }
    // `curl --data-binary` labels a body without a Content-Type as a form.
    if body.is_some()
        && !qt
        && !headers
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("content-type"))
    {
        b = b.header("Content-Type", "application/x-www-form-urlencoded");
    }
    let proxy = env_proxy(url, env)
        .map(|p| ureq::Proxy::new(&p))
        .transpose()?;
    let a = agent(o);
    STATUS_LINE.set(None);
    HEADER_LINES.set(None);
    PEER_CLOSED.set(false);
    ENDS_AT_CLOSE.set(false);
    BODY_RECEIVED.set(0);
    let names = headers.iter().map(|(k, _)| k.clone());
    let last = login.map(|_| "Authorization".to_string());
    HEADER_ORDER.set(qt.then(|| names.chain(last).collect()));
    HEARD.take();
    let res = match body {
        // A body-less request must not announce `Content-Length: 0` on GET.
        None => run(a, b.body(())?, proxy, no_ua, o.answer_within, qt)?,
        Some(body) => run(a, b.body(body)?, proxy, no_ua, o.answer_within, qt)?,
    };
    let status = res.status().as_u16();
    // The line read last is the reply's own when its code is the reply's; the standard phrase
    // stands in otherwise.
    let line = STATUS_LINE.take().filter(|(code, _)| *code == status);
    let sent = HEADER_LINES.take().filter(|_| qt && line.is_some());
    let reason = match line {
        Some((_, reason)) => reason,
        None => res.status().canonical_reason().unwrap_or_default().into(),
    };
    let headers = match sent {
        // QNetworkReply keeps the `Content-Encoding` of what it decompressed and drops the
        // length; ureq decompressed what is exactly `gzip`.
        Some(mut sent) => {
            let encoding = sent
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("content-encoding"));
            if encoding.is_some_and(|(_, v)| v == "gzip") {
                sent.retain(|(k, _)| !k.eq_ignore_ascii_case("content-length"));
            }
            sent
        }
        None => res
            .headers()
            .iter()
            .map(|(k, v)| {
                (
                    k.to_string(),
                    String::from_utf8_lossy(v.as_bytes()).into_owned(),
                )
            })
            .collect(),
    };
    let body = Box::new(res.into_body().into_with_config().limit(u64::MAX).reader());
    Ok(Response {
        status,
        reason,
        headers,
        body,
    })
}

pub fn get(url: &str, headers: &[(String, String)], o: Opts) -> Result<Response, Error> {
    send("GET", url, headers, None, o)
}

/// What an image cache (MPRIS covers, favicons, `download_image`) downloads at most.
pub const IMAGE_MAX: u64 = 32 << 20;

/// `curl [-f] -o path url`: stream the body to `path` (through `path.part`, so readers never
/// see half a file). With `fail` (curl `-f`) an HTTP error status writes nothing. Unlike curl,
/// a body over [`BODY_MAX`] is refused: a server streaming forever filled the disk (or RAM,
/// for a cover in tmpfs `/tmp`).
pub fn download(
    url: &str,
    headers: &[(String, String)],
    path: &std::path::Path,
    fail: bool,
    o: Opts,
) -> std::io::Result<u16> {
    download_max(url, headers, path, fail, o, BODY_MAX)
}

/// `Err(FileTooLarge)` when the response announces a `Content-Length` over `max`.
pub fn check_length(r: &Response, max: u64) -> std::io::Result<()> {
    match r
        .header("content-length")
        .and_then(|v| v.trim().parse::<u64>().ok())
    {
        Some(n) if n > max => Err(std::io::Error::new(
            std::io::ErrorKind::FileTooLarge,
            format!("body of {n} bytes is over {max}"),
        )),
        _ => Ok(()),
    }
}

/// [`download`] that gives up (writing nothing) once the body passes `max` bytes, or at once
/// when its `Content-Length` says it will.
pub fn download_max(
    url: &str,
    headers: &[(String, String)],
    path: &std::path::Path,
    fail: bool,
    o: Opts,
    max: u64,
) -> std::io::Result<u16> {
    let mut r = get(url, headers, o).map_err(std::io::Error::other)?;
    if fail && r.status >= 400 {
        return Err(std::io::Error::other(format!("HTTP {}", r.status)));
    }
    check_length(&r, max)?;
    let tmp = part_path(path);
    let res = std::fs::File::create(&tmp)
        .and_then(|mut f| crate::osutil::copy_max(&mut r.body, &mut f, max));
    if let Err(e) = res {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    std::fs::rename(&tmp, path)?;
    Ok(r.status)
}

/// A temporary name beside `path`, unique per call: two downloads of the same target (two
/// players showing the same cover) must not write into one `.part` file.
pub fn part_path(path: &std::path::Path) -> std::path::PathBuf {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(format!(".{}-{n}.part", std::process::id()));
    tmp.into()
}

/// `$XDG_CACHE_HOME` (or `~/.cache`).
pub fn cache_home() -> std::path::PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .filter(|v| !v.is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".cache")
        })
}

/// Percent-encode what curl would send raw but a URI cannot carry (spaces, controls,
/// non-ASCII bytes); everything else passes through as curl leaves it.
pub fn encode_path(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_graphic()
            && !matches!(
                b,
                b'"' | b'<' | b'>' | b'\\' | b'^' | b'`' | b'{' | b'|' | b'}'
            )
        {
            o.push(b as char);
        } else {
            o.push_str(&format!("%{b:02X}"));
        }
    }
    o
}

/// The logins an engine's `XMLHttpRequest`s answer an HTTP 401 with, kept and used as Qt
/// 6.12.0's QNetworkAccessManager does: a QAuthenticator for each channel of a host's
/// QHttpNetworkConnection, and QNetworkAccessAuthenticationManager's cache.
///
/// What a server sent is kept here as Qt keeps it, byte for byte: a `String` of a challenge
/// or a realm has one char for each byte (QString::fromLatin1).
pub(crate) mod login {
    use std::collections::{HashMap, VecDeque};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Condvar, Mutex, MutexGuard};
    use std::time::{Duration, Instant};

    /// The logins of one engine.
    pub(crate) type Shared = Arc<Share>;

    /// [`Logins`] and what wakes the requests that wait for a channel.
    #[derive(Default)]
    pub(crate) struct Share {
        logins: Mutex<Logins>,
        freed: Condvar,
    }

    impl Share {
        fn lock(&self) -> MutexGuard<'_, Logins> {
            self.logins.lock().unwrap_or_else(|e| e.into_inner())
        }
    }

    /// What a challenge asks for, the weakest first (QAuthenticatorPrivate::methodStrength).
    /// Qt knows `Negotiate` only when built with GSSAPI or SSPI, which the Qt of the test
    /// image is not: such a challenge is none.
    #[derive(Clone, Copy, PartialEq, PartialOrd)]
    enum Method {
        None,
        Basic,
        Digest,
        Ntlm,
    }

    /// QAuthenticatorPrivate::Phase: `Done` once an answer was made of the login, back to
    /// `Start` when the login or the realm changes. NTLM goes through `Phase1` (asked for)
    /// and `Phase2` (its first message is out, the second is to come).
    #[derive(Clone, Copy, PartialEq)]
    enum Phase {
        Start,
        Phase1,
        Phase2,
        Done,
        Invalid,
    }

    /// QAuthenticator.
    struct Authenticator {
        user: String,
        password: String,
        realm: String,
        method: Method,
        /// The challenge after its method's name.
        challenge: String,
        phase: Phase,
        /// Digest's client nonce: 32 hex digits, made once. Its last eight are NTLM's
        /// client challenge.
        cnonce: String,
        /// How many Digest answers were made (`nc`).
        count: u32,
    }

    /// QString::toLatin1: `?` for what Latin-1 cannot write.
    fn latin1(s: &str) -> Vec<u8> {
        s.chars()
            .map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?'))
            .collect()
    }

    /// QString::fromLatin1: a char for each byte.
    fn chars(b: &[u8]) -> String {
        b.iter().map(|&b| char::from(b)).collect()
    }

    /// The parts of a challenge (QAuthenticatorPrivate::parseDigestAuthenticationChallenge):
    /// `name=value` between commas, a value in quotes or up to the comma, `\` taking the
    /// next byte as it is. A `qop` list without `auth` leaves no part at all.
    fn options(challenge: &str) -> HashMap<String, String> {
        let b = &latin1(challenge)[..];
        let mut out = HashMap::new();
        let mut d = 0;
        while d < b.len() {
            while d < b.len() && matches!(b[d], b' ' | b'\n' | b'\r') {
                d += 1;
            }
            let start = d;
            while d < b.len() && b[d] != b'=' {
                d += 1;
            }
            let key = &b[start..d];
            d += 1;
            let quote = b.get(d) == Some(&b'"');
            d += usize::from(quote);
            if d >= b.len() {
                break;
            }
            let mut value = vec![];
            while d < b.len() {
                let escaped = b[d] == b'\\' && d + 1 < b.len();
                d += usize::from(escaped);
                if !escaped && b[d] == if quote { b'"' } else { b',' } {
                    break;
                }
                value.push(b[d]);
                d += 1;
            }
            while d < b.len() && b[d] != b',' {
                d += 1;
            }
            d += 1;
            out.insert(chars(key), chars(&value));
        }
        if let Some(qop) = out.get("qop").filter(|q| !q.is_empty()) {
            if !qop.split(',').any(|t| t == "auth") {
                return HashMap::new();
            }
            out.insert("qop".into(), "auth".into());
        }
        out
    }

    /// Whether Qt takes this line of `WWW-Authenticate` for a challenge it knows
    /// (QAuthenticatorPrivate::isMethodSupported): its first word is one of these.
    fn supported(line: &str) -> bool {
        let word = line.split(' ').next().unwrap_or_default();
        ["basic", "ntlm", "digest"]
            .iter()
            .any(|m| word.eq_ignore_ascii_case(m))
    }

    /// The strongest challenge of `lines` and what follows its name
    /// (QAuthenticatorPrivate::parseHttpResponse): a line counts by how it begins, and a
    /// Digest line only without an `algorithm` or with one that begins with `MD5`.
    fn strongest<'a>(lines: &[&'a str]) -> (Method, &'a str) {
        let mut found = (Method::None, "");
        for l in lines {
            let starts = |p: &str| l.get(..p.len()).is_some_and(|s| s.eq_ignore_ascii_case(p));
            let rest = |n: usize| l.get(n..).unwrap_or_default();
            if found.0 < Method::Basic && starts("basic") {
                found = (Method::Basic, rest(6));
            } else if found.0 < Method::Ntlm && starts("ntlm") {
                found = (Method::Ntlm, rest(5));
            } else if found.0 < Method::Digest && starts("digest") {
                let md5 = match options(rest(7)).get("algorithm") {
                    Some(a) => a.get(..3).is_some_and(|a| a.eq_ignore_ascii_case("md5")),
                    None => true,
                };
                if md5 {
                    found = (Method::Digest, rest(7));
                }
            }
        }
        found
    }

    /// `s` in UTF-16, the low byte first (qStringAsUcs2Le).
    fn utf16(s: &str) -> Vec<u8> {
        s.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    fn md5_sum(data: &[u8]) -> Vec<u8> {
        use md5::Digest;
        md5::Md5::digest(data).to_vec()
    }

    /// RFC 2104's HMAC with MD5 (qEncodeHmacMd5).
    fn hmac_md5(key: &[u8], message: &[u8]) -> Vec<u8> {
        let mut block = [0u8; 64];
        match key.len() > 64 {
            true => block[..16].copy_from_slice(&md5_sum(key)),
            false => block[..key.len()].copy_from_slice(key),
        }
        let pad = |with: u8| block.iter().map(|b| b ^ with).collect::<Vec<u8>>();
        let inner = md5_sum(&[pad(0x36), message.to_vec()].concat());
        md5_sum(&[pad(0x5c), inner].concat())
    }

    /// NTLM's first message (qNtlmPhase1): no domain, no workstation, and the flags Unicode,
    /// "request target", NTLM and NTLM2.
    const NTLM_TYPE1: [u8; 32] = {
        let mut m = [0u8; 32];
        let head = *b"NTLMSSP\0\x01\0\0\0\x05\x82\x08\0";
        let mut i = 0;
        while i < head.len() {
            m[i] = head[i];
            i += 1;
        }
        m
    };

    /// QByteArray::fromBase64 as Qt calls it: what is not of the alphabet is skipped.
    fn from_base64(s: &str) -> Vec<u8> {
        use base64::Engine;
        use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
        let config = GeneralPurposeConfig::new()
            .with_decode_padding_mode(DecodePaddingMode::Indifferent)
            .with_decode_allow_trailing_bits(true);
        let mut digits: Vec<u8> = s
            .bytes()
            .filter(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/'))
            .collect();
        if digits.len() % 4 == 1 {
            digits.pop();
        }
        GeneralPurpose::new(&base64::alphabet::STANDARD, config)
            .decode(digits)
            .unwrap_or_default()
    }

    /// The hundreds of nanoseconds since 1601 that NTLM counts time in, to the second.
    fn ntlm_now() -> u64 {
        let epoch = std::time::SystemTime::UNIX_EPOCH;
        let secs = std::time::SystemTime::now()
            .duration_since(epoch)
            .map_or(0, |d| d.as_secs());
        (secs + 11_644_473_600) * 10_000_000
    }

    /// NTLM's third message for the second, `type2` (qNtlmPhase3): NTLMv2's answer, and
    /// LMv2's when the server gave no target information. Nothing for a message that is
    /// not a type 2 one. As in Qt the domain is the server's target name, read as UTF-16
    /// whatever its flags say, unless `domain` is given or the user is `name@domain`; the
    /// time is the server's when its target information has one; and for a server without
    /// Unicode the names are counted in Latin-1 and still written in UTF-16.
    fn ntlm_type3(type2: &[u8], domain: &str, user: &str, password: &str, nonce: &[u8]) -> Vec<u8> {
        use md4::Digest;
        let u16_at = |at: usize| u16::from_le_bytes([type2[at], type2[at + 1]]);
        let u32_at = |at: usize| u32::from(u16_at(at)) | u32::from(u16_at(at + 2)) << 16;
        if type2.len() < 48 || &type2[..8] != b"NTLMSSP\0" || u32_at(8) != 2 {
            return vec![];
        }
        let field = |at: usize| {
            let (len, from) = (usize::from(u16_at(at)), u32_at(at + 4) as usize);
            match len {
                0 => Some(&type2[..0]),
                _ => type2.get(from..from.checked_add(len)?),
            }
        };
        let (Some(target), Some(info)) = (field(12), field(40)) else {
            return vec![];
        };
        let (theirs, challenge) = (u32_at(20), &type2[24..32]);
        let target: Vec<u16> = target
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .collect();
        let target = String::from_utf16_lossy(&target);
        let unicode = theirs & 1 != 0;
        // Unicode, NTLM and target information, with the server's NTLM2 and "always sign";
        // OEM beside Unicode for a server without it.
        let flags = 0x0080_0201 | theirs & 0x0008_8000 | if unicode { 0 } else { 2 };
        let domain = match domain.is_empty() && !user.contains('@') {
            true => target.as_str(),
            false => domain,
        };
        let mut at = 64;
        let mut place = |len: usize| {
            let from = (at + 1) & !1;
            at = from + len;
            let len = (len as u16).to_le_bytes();
            [&len[..], &len[..], &(from as u32).to_le_bytes()[..]].concat()
        };
        let named = |s: &str| s.encode_utf16().count() * if unicode { 2 } else { 1 };
        let (domain_at, user_at) = (place(named(domain)), place(named(user)));
        let workstation_at = place(0);

        let hash = md4::Md4::digest(utf16(password));
        let key = hmac_md5(&hash, &utf16(&(user.to_uppercase() + domain)));
        let answer =
            |rest: &[u8]| [hmac_md5(&key, &[challenge, rest].concat()), rest.to_vec()].concat();
        let lm = match info.is_empty() {
            true => answer(nonce),
            false => vec![],
        };
        // The time of the server's target information (its part 7), if it is eight bytes.
        let mut time = ntlm_now().to_le_bytes().to_vec();
        let mut rest = info;
        while let Some((head, more)) = rest.split_at_checked(4) {
            let id = u16::from_le_bytes([head[0], head[1]]);
            let len = usize::from(u16::from_le_bytes([head[2], head[3]]));
            if id == 7 {
                if let Some(t) = more.get(..8).filter(|_| len == 8) {
                    time = t.to_vec();
                }
                break;
            }
            let Some(next) = more.get(len..) else { break };
            rest = next;
        }
        let blob = [
            &[1, 1, 0, 0, 0, 0, 0, 0][..],
            &time,
            nonce,
            &[0; 4],
            info,
            &[0; 4],
        ]
        .concat();
        let nt = answer(&blob);
        let (lm_at, nt_at) = (place(lm.len()), place(nt.len()));

        let mut m = b"NTLMSSP\0\x03\0\0\0".to_vec();
        for part in [&lm_at, &nt_at, &domain_at, &user_at, &workstation_at] {
            m.extend_from_slice(part);
        }
        m.extend([0; 8]);
        m.extend(flags.to_le_bytes());
        m.extend(utf16(domain));
        m.extend(utf16(user));
        m.extend(lm);
        m.extend(nt);
        m
    }

    impl Authenticator {
        fn new() -> Self {
            let mut seed = [0u8; 8];
            // The nonce only has to differ from other clients': without the system's
            // random numbers it is the same every time.
            let _ = getrandom::fill(&mut seed);
            let seed = format!("{:x}", u64::from_ne_bytes(seed));
            Authenticator {
                user: String::new(),
                password: String::new(),
                realm: String::new(),
                method: Method::None,
                challenge: String::new(),
                phase: Phase::Start,
                cnonce: crate::builtins::md5_hex(seed.as_bytes()),
                count: 0,
            }
        }

        /// Takes the challenge in (QAuthenticatorPrivate::parseHttpResponse): another
        /// realm or a stale Digest nonce lets the login answer again, no login at all
        /// leaves nothing to answer with. NTLM has no realm and is asked of any login. A
        /// weaker challenge in the middle of NTLM's exchange is refused: there is no
        /// method then, and nothing more is answered.
        fn parse(&mut self, method: Method, value: &str) {
            if self.phase == Phase::Phase2 && method < self.method {
                self.method = Method::None;
                self.phase = Phase::Done;
                self.challenge.clear();
                return;
            }
            self.method = method;
            if method == Method::None {
                self.realm.clear();
                self.challenge.clear();
                self.phase = Phase::Invalid;
                return;
            }
            self.challenge = value.trim_matches(|c: char| c.is_ascii_whitespace()).into();
            if method == Method::Ntlm {
                self.realm.clear();
                return;
            }
            let options = options(&self.challenge);
            let realm = options.get("realm").cloned().unwrap_or_default();
            if realm != self.realm {
                if self.phase == Phase::Done {
                    self.phase = Phase::Start;
                }
                self.realm = realm;
            }
            let stale = options.get("stale");
            if method == Method::Digest && stale.is_some_and(|s| s.eq_ignore_ascii_case("true")) {
                self.phase = Phase::Start;
                self.count = 0;
            }
            if self.user.is_empty() && self.password.is_empty() {
                self.phase = Phase::Done;
            }
        }

        /// The `Authorization` for a request of `method` for `uri`
        /// (QAuthenticatorPrivate::calculateResponse), in the bytes that go out: Qt writes
        /// the login in Latin-1.
        fn response(&mut self, method: &str, uri: &str) -> Vec<u8> {
            use base64::Engine;
            let base64 = |b: &[u8]| base64::engine::general_purpose::STANDARD.encode(b);
            self.phase = Phase::Done;
            let answer = match self.method {
                Method::Digest => return [&b"Digest "[..], &self.digest(method, uri)].concat(),
                Method::Ntlm => format!("NTLM {}", base64(&self.ntlm())),
                _ => {
                    let login = latin1(&format!("{}:{}", self.user, self.password));
                    format!("Basic {}", base64(&login))
                }
            };
            // QHttpHeaders keeps no space at the end of a value.
            answer.trim_end().as_bytes().to_vec()
        }

        /// NTLM's next message (QAuthenticatorPrivate::calculateResponse): the first one
        /// without a challenge, after which a login with a user waits for the second; the
        /// third one for the challenge, which is used up by it. A user `domain\name` names
        /// its domain (QAuthenticatorPrivate::updateCredentials).
        fn ntlm(&mut self) -> Vec<u8> {
            if self.challenge.is_empty() {
                if !self.user.is_empty() {
                    self.phase = Phase::Phase2;
                }
                return NTLM_TYPE1.to_vec();
            }
            let type2 = from_base64(&std::mem::take(&mut self.challenge));
            let (domain, user) = self.user.split_once('\\').unwrap_or(("", &self.user));
            let nonce = &self.cnonce.as_bytes()[self.cnonce.len().saturating_sub(8)..];
            ntlm_type3(&type2, domain, user, &self.password, nonce)
        }

        /// RFC 2617's answer to the challenge (QAuthenticatorPrivate::digestMd5Response),
        /// with `qop=auth` when the challenge has a `qop`; each answer counts `nc` up. The
        /// user, the realm and the password are hashed and sent in Latin-1, what the
        /// challenge gave goes back in its bytes.
        fn digest(&mut self, method: &str, uri: &str) -> Vec<u8> {
            let options = options(&self.challenge);
            let get = |k: &str| latin1(options.get(k).map_or("", String::as_str));
            let (nonce, opaque, qop, algorithm) =
                (get("nonce"), get("opaque"), get("qop"), get("algorithm"));
            self.count += 1;
            let nc = format!("{:08x}", self.count);
            let md5 = |parts: &[&[u8]]| crate::builtins::md5_hex(&parts.join(&b":"[..]));
            let (user, realm) = (latin1(&self.user), latin1(&self.realm));
            let mut ha1 = md5(&[&user, &realm, &latin1(&self.password)]);
            if algorithm.eq_ignore_ascii_case(b"md5-sess") {
                ha1 = md5(&[ha1.as_bytes(), &nonce, self.cnonce.as_bytes()]);
            }
            let ha2 = md5(&[method.as_bytes(), uri.as_bytes()]);
            let mut said: Vec<&[u8]> = vec![ha1.as_bytes(), &nonce];
            if !qop.is_empty() {
                said.extend([nc.as_bytes(), self.cnonce.as_bytes(), &qop[..]]);
            }
            said.push(ha2.as_bytes());
            let response = md5(&said);
            let mut out = vec![];
            let mut add = |parts: &[&[u8]]| parts.iter().for_each(|p| out.extend_from_slice(p));
            add(&[
                b"username=\"",
                &user,
                b"\", realm=\"",
                &realm,
                b"\", nonce=\"",
            ]);
            add(&[&nonce, b"\", uri=\"", uri.as_bytes(), b"\", "]);
            if !opaque.is_empty() {
                add(&[b"opaque=\"", &opaque, b"\", "]);
            }
            add(&[b"response=\"", response.as_bytes(), b"\""]);
            if !algorithm.is_empty() {
                add(&[b", algorithm=", &algorithm]);
            }
            if !qop.is_empty() {
                add(&[b", qop=", &qop, b", nc=", nc.as_bytes()]);
                add(&[b", cnonce=\"", self.cnonce.as_bytes(), b"\""]);
            }
            out
        }
    }

    /// QAuthenticator::detach: the authenticator, a new one if there was none; one whose
    /// login already answered may answer again.
    fn detach(auth: &mut Option<Authenticator>) -> &mut Authenticator {
        let auth = auth.get_or_insert_with(Authenticator::new);
        if auth.phase == Phase::Done {
            auth.phase = Phase::Start;
        }
        auth
    }

    /// QAuthenticator::setUser and setPassword: what differs is set, and all of it where
    /// there was no authenticator.
    fn set_login(auth: &mut Option<Authenticator>, user: &str, password: &str) {
        set(auth, user, password, (true, true));
    }

    /// QHttpNetworkConnectionPrivate::copyCredentials for one channel: what differs is
    /// set, an empty user or password being what no authenticator has.
    fn copy_login(auth: &mut Option<Authenticator>, user: &str, password: &str) {
        set(
            auth,
            user,
            password,
            (!user.is_empty(), !password.is_empty()),
        );
    }

    /// Sets the user and the password that differ; `absent` says which do where there is
    /// no authenticator.
    fn set(auth: &mut Option<Authenticator>, user: &str, password: &str, absent: (bool, bool)) {
        let differs = |a: &Authenticator| (a.user != user, a.password != password);
        let (set_user, set_password) = auth.as_ref().map_or(absent, differs);
        if set_user {
            detach(auth).user = user.into();
        }
        if set_password {
            detach(auth).password = password.into();
        }
    }

    /// One of the sockets of a host's connection (QHttpNetworkConnectionChannel).
    #[derive(Default)]
    struct Channel {
        /// A request is on it.
        busy: bool,
        /// A request on it was stopped before its reply ended: Qt 6.12.0 gives the channel
        /// no request any more.
        dead: bool,
        /// The status of the reply it read last.
        last: u16,
        /// An `Authorization` went out since it connected (authenticationCredentialsSent).
        sent: bool,
        /// The last reply left the socket connected.
        open: bool,
        auth: Option<Authenticator>,
    }

    /// QNetworkAccessAuthenticationManager's cache: user and password by host, user (or
    /// none) and realm. Nothing leaves it.
    #[derive(Default)]
    struct Cache(HashMap<(String, String, String), (String, String)>);

    impl Cache {
        /// cacheCredentials: the login is found again under its user and under no user.
        fn store(&mut self, host: &str, auth: &Authenticator) {
            let login = (auth.user.clone(), auth.password.clone());
            for user in [auth.user.as_str(), ""] {
                let key = (host.to_string(), user.to_string(), auth.realm.clone());
                self.0.insert(key, login.clone());
            }
        }

        /// fetchCachedCredentials for the address `at`: nothing for one that has a
        /// password or no path.
        fn fetch(&self, at: &Seat, realm: &str) -> Option<(String, String)> {
            if !at.password.is_empty() || !at.rooted {
                return None;
            }
            let key = (at.host.clone(), at.user.clone(), realm.to_string());
            self.0.get(&key).cloned()
        }
    }

    /// The connection to one scheme, host and port (QHttpNetworkConnection).
    #[derive(Default)]
    struct Host {
        channels: Vec<Channel>,
        /// The requests that wait for a channel, the first to come first, and the number
        /// the next one gets.
        waiting: VecDeque<u64>,
        next: u64,
        /// How many requests have or wait for a channel.
        using: usize,
        /// Since when none does.
        unused: Option<Instant>,
    }

    impl Host {
        /// The free channel a request gets: the first one. Where a stopped request left
        /// none that takes requests, Qt 6.12.0 sends nothing to the host any more; here
        /// the connection starts anew, as Qt's does once it was [`UNUSED`].
        fn free(&mut self) -> Option<usize> {
            if self.channels.iter().all(|c| c.dead && !c.busy) {
                self.channels.clear();
                self.channels.resize_with(CHANNELS, Channel::default);
            }
            self.channels.iter().position(|c| !c.busy && !c.dead)
        }

        /// A request no longer has or waits for a channel: the last one leaves the
        /// connection unused from `now` on.
        fn left(&mut self, now: Instant) {
            self.using = self.using.saturating_sub(1);
            if self.using == 0 {
                self.unused = Some(now);
            }
        }
    }

    /// What an engine remembers of logins.
    pub(crate) struct Logins {
        cache: Cache,
        hosts: HashMap<String, Host>,
        /// The time; a test sets its own.
        now: Box<dyn Fn() -> Instant + Send>,
    }

    impl Default for Logins {
        fn default() -> Self {
            Logins {
                cache: Cache::default(),
                hosts: HashMap::new(),
                now: Box::new(Instant::now),
            }
        }
    }

    /// QHttpNetworkConnection's channels for one host.
    const CHANNELS: usize = 6;

    /// How long a connection that no request uses is kept with what its channels know of
    /// logins (QNetworkAccessCache's ExpiryTime); the next request then starts a new one.
    const UNUSED: Duration = Duration::from_secs(120);

    /// `url` without the user and password written in it, and those two, percent-decoded.
    pub(crate) fn split(url: &str) -> (String, String, String) {
        let none = || (url.to_string(), String::new(), String::new());
        let Some(at) = url.find("://").map(|i| i + 3) else {
            return none();
        };
        let end = url[at..]
            .find(['/', '?', '#'])
            .map_or(url.len(), |n| at + n);
        let Some((login, host)) = url[at..end].rsplit_once('@') else {
            return none();
        };
        let decoded = |s: &str| {
            let b = s.as_bytes();
            let mut out = vec![];
            let mut i = 0;
            while i < b.len() {
                let hex = b
                    .get(i + 1..i + 3)
                    .and_then(|h| std::str::from_utf8(h).ok());
                match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(v) if b[i] == b'%' => {
                        out.push(v);
                        i += 3;
                    }
                    _ => {
                        out.push(b[i]);
                        i += 1;
                    }
                }
            }
            String::from_utf8_lossy(&out).into_owned()
        };
        let (user, password) = login.split_once(':').unwrap_or((login, ""));
        (
            format!("{}{host}{}", &url[..at], &url[end..]),
            decoded(user),
            decoded(password),
        )
    }

    /// One request's place on a channel, from its first send to the end of its reply.
    pub(crate) struct Seat {
        logins: Shared,
        /// Scheme, host and port of the address.
        host: String,
        /// The channel of the host's connection; a synchronous request has a connection
        /// of its own, which nothing else uses.
        channel: Option<usize>,
        own: Channel,
        /// The user and password of the address.
        user: String,
        password: String,
        /// The address has a path.
        rooted: bool,
        /// The path and query of the address.
        target: String,
        /// The request did not go out yet.
        first: bool,
        /// A challenge already looked for a login to answer it with.
        asked: bool,
        /// The channel and one, while the request is out and its reply did not end: the
        /// channel a stop of the request leaves dead.
        flying: Arc<AtomicUsize>,
    }

    /// The scheme, host and port of `url`, which name its connection.
    fn host_of(url: &str) -> String {
        url::Url::parse(url).map_or_else(|_| url.into(), |u| u.origin().ascii_serialization())
    }

    /// A request's place in the line for the channels of its host's connection. `send`
    /// takes it, so the requests get their channels in the order they were sent, as Qt's
    /// do; a place no [`Seat`] took is given up when it is dropped.
    pub(crate) struct Place {
        logins: Shared,
        host: String,
        number: u64,
        /// The connection was there before this request.
        known: bool,
        /// A seat has it.
        kept: bool,
    }

    impl Place {
        /// The last place in the line of the host of `url`, which has no login in it. A
        /// connection no request used for [`UNUSED`] is a new one.
        pub(crate) fn take(logins: &Shared, url: &str) -> Place {
            let host = host_of(url);
            let mut all = logins.lock();
            let Logins { hosts, now, .. } = &mut *all;
            let mut known = hosts.contains_key(&host);
            let at = hosts.entry(host.clone()).or_default();
            if at.using == 0 && at.unused.is_some_and(|since| now() - since >= UNUSED) {
                *at = Host::default();
                known = false;
            }
            at.channels.resize_with(CHANNELS, Channel::default);
            at.using += 1;
            at.unused = None;
            let number = at.next;
            at.next += 1;
            at.waiting.push_back(number);
            // Whoever watches the line sees it grow.
            logins.freed.notify_all();
            Place {
                logins: logins.clone(),
                host,
                number,
                known,
                kept: false,
            }
        }
    }

    impl Drop for Place {
        fn drop(&mut self) {
            if self.kept {
                return;
            }
            let mut all = self.logins.lock();
            let Logins { hosts, now, .. } = &mut *all;
            if let Some(at) = hosts.get_mut(&self.host) {
                at.waiting.retain(|n| *n != self.number);
                at.left(now());
            }
            self.logins.freed.notify_all();
        }
    }

    impl Seat {
        /// A place for a request to `url`, which has no login in it, with the login
        /// `user` and `password` of its address. An asynchronous request takes the first
        /// free channel of its host's connection, after the requests that were sent before
        /// it and still wait for one when all six are taken (QHttpNetworkConnectionPrivate::
        /// _q_startNextRequest); none if it is stopped while it waits. `place` is where
        /// `send` put it in that line; a request without one for the host, as after a
        /// redirect to another, goes to the end. When the connection is not new, a login
        /// remembered for the host without a realm goes to every channel
        /// (QHttpThreadDelegate::startRequest).
        pub(crate) fn new(
            logins: &Shared,
            place: Option<Place>,
            synchronous: bool,
            url: &str,
            user: &str,
            password: &str,
        ) -> Option<Seat> {
            let host = host_of(url);
            let after = url.find("://").map_or(0, |i| i + 3);
            let path = url[after..]
                .find(['/', '?', '#'])
                .map_or("", |n| &url[after + n..]);
            let path = path.split('#').next().unwrap_or_default();
            let mut seat = Seat {
                logins: logins.clone(),
                host,
                channel: None,
                own: Channel::default(),
                user: user.into(),
                password: password.into(),
                rooted: path.starts_with('/'),
                target: match path.starts_with('/') {
                    true => path.into(),
                    false => format!("/{path}"),
                },
                first: true,
                asked: false,
                flying: Arc::default(),
            };
            if synchronous {
                return Some(seat);
            }
            // A stop of the request ends its wait for a channel, and kills the channel it
            // is out on.
            let (flying, share, host) = (seat.flying.clone(), logins.clone(), seat.host.clone());
            super::on_stop(move || {
                let mut all = share.lock();
                let channel = flying.swap(0, Ordering::SeqCst).checked_sub(1);
                let channels = all.hosts.get_mut(&host).map(|h| &mut h.channels);
                if let Some(c) = channel.and_then(|i| channels?.get_mut(i)) {
                    c.dead = true;
                }
                share.freed.notify_all();
            });
            let mut place = place
                .filter(|p| p.host == seat.host && Arc::ptr_eq(&p.logins, logins))
                .unwrap_or_else(|| Place::take(logins, url));
            // From here the seat leaves the line itself.
            place.kept = true;
            let (number, known) = (place.number, place.known);
            let mut all = logins.lock();
            let Logins { cache, hosts, .. } = &mut *all;
            if let Some((user, password)) = cache.fetch(&seat, "").filter(|_| known)
                && !user.is_empty()
                && !password.is_empty()
            {
                for c in hosts
                    .entry(seat.host.clone())
                    .or_default()
                    .channels
                    .iter_mut()
                {
                    copy_login(&mut c.auth, &user, &password);
                }
            }
            loop {
                let Logins { hosts, now, .. } = &mut *all;
                // The place made it.
                let at = hosts.entry(seat.host.clone()).or_default();
                let free = at.free().filter(|_| at.waiting.front() == Some(&number));
                if free.is_some() || super::stopped() {
                    at.waiting.retain(|n| *n != number);
                    match free {
                        Some(free) => {
                            at.channels[free].busy = true;
                            seat.channel = Some(free);
                        }
                        None => at.left(now()),
                    }
                    // The next in the line may find another channel free.
                    logins.freed.notify_all();
                    // Before the seat may be dropped, which locks them.
                    drop(all);
                    return free.map(|_| seat);
                }
                all = logins.freed.wait(all).unwrap_or_else(|e| e.into_inner());
            }
        }

        /// Runs `f` on the request's channel and the cache; the login `f` returns goes to
        /// the other channels of the connection (QHttpNetworkConnectionPrivate::
        /// copyCredentials).
        fn on_channel<T>(
            &mut self,
            f: impl FnOnce(&mut Seat, &mut Channel, &mut Cache) -> (T, bool),
        ) -> T {
            let logins = self.logins.clone();
            let mut all = logins.lock();
            let Logins { cache, hosts, .. } = &mut *all;
            let mut own = std::mem::take(&mut self.own);
            let channels: &mut [Channel] = match hosts.get_mut(&self.host) {
                Some(host) if self.channel.is_some() => &mut host.channels,
                _ => &mut [],
            };
            let channel = match self.channel.and_then(|i| channels.get_mut(i)) {
                Some(c) => c,
                None => &mut own,
            };
            let (out, copy) = f(self, channel, cache);
            let login = channel
                .auth
                .as_ref()
                .filter(|_| copy)
                .map(|a| (a.user.clone(), a.password.clone()));
            self.own = own;
            if let (Some((user, password)), Some(i)) = (login, self.channel) {
                for (_, c) in channels.iter_mut().enumerate().filter(|(n, _)| *n != i) {
                    copy_login(&mut c.auth, &user, &password);
                }
            }
            out
        }

        /// Called before the request goes out, each time it does: the `Authorization` Qt
        /// sends with it, which then is not the one among `headers`. A socket that has to
        /// connect again forgets that it sent a login (QHttpNetworkConnectionChannel::
        /// ensureConnection); a login in the address replaces the channel's
        /// (QHttpProtocolHandler::sendRequest); and a channel that knows how to answer
        /// does so, in place of the page's own `Authorization` only after a 401, and with
        /// NTLM only after a 401 that its login did not answer yet
        /// (QHttpNetworkConnectionPrivate::createAuthorization).
        pub(crate) fn headers(
            &mut self,
            method: &str,
            headers: &mut Vec<(String, String)>,
        ) -> Option<Vec<u8>> {
            if let Some(channel) = self.channel {
                self.flying.store(channel + 1, Ordering::SeqCst);
            }
            self.on_channel(|seat, channel, _| {
                if !channel.open {
                    channel.sent = false;
                    detach(&mut channel.auth);
                }
                channel.open = false;
                let mut copy = false;
                let first = std::mem::take(&mut seat.first);
                if first && !(seat.user.is_empty() && seat.password.is_empty()) {
                    let had = channel.auth.as_ref();
                    let (user, password) = had.map_or(("", ""), |a| (&a.user, &a.password));
                    if seat.user != user || (!seat.password.is_empty() && seat.password != password)
                    {
                        set_login(&mut channel.auth, &seat.user, &seat.password);
                        copy = true;
                    }
                }
                let own = |h: &(String, String)| h.0.eq_ignore_ascii_case("authorization");
                let has_own = headers.iter().any(|h| own(h) && !h.1.is_empty());
                let asked = channel.last == 401;
                let mut answer = None;
                if let Some(auth) = &mut channel.auth
                    && match auth.method {
                        Method::None => false,
                        Method::Ntlm => asked && (auth.phase != Phase::Done || !channel.sent),
                        _ => asked || !has_own,
                    }
                {
                    answer = Some(auth.response(method, &seat.target));
                    headers.retain(|h| !own(h));
                    channel.sent = true;
                }
                (answer, copy)
            })
        }

        /// Called with each reply's status and headers and what [`super::heard`] of it:
        /// whether the request goes out again, to answer a 401
        /// (QHttpNetworkConnectionPrivate::handleAuthenticateChallenge). A challenge Qt
        /// does not know goes to the page and changes nothing. One the channel's login did
        /// not answer yet is answered with it, and the login is remembered. One that comes
        /// back after the login went out, or that finds no login, or that asks for NTLM,
        /// looks for a login in the address and among those remembered
        /// (QNetworkAccessManagerPrivate::authenticationRequired; a synchronous request
        /// looks among those remembered, once); with none the 401 goes to the page and
        /// the channel forgets its login. The socket stays connected as the reply says
        /// (QHttpNetworkReplyPrivate::readHeader): not after `close` anywhere in its
        /// `Connection` or `Proxy-Connection`, nor after HTTP/1.0 without a `Connection`
        /// and without `keep-alive` in `Proxy-Connection`.
        pub(crate) fn answers(
            &mut self,
            status: u16,
            headers: &[(String, String)],
            heard: &super::Heard,
        ) -> bool {
            self.on_channel(|seat, channel, cache| {
                let header = |name: &'static str| {
                    let mut values = headers
                        .iter()
                        .filter(move |(k, _)| k.eq_ignore_ascii_case(name))
                        .map(|(_, v)| v.trim().to_ascii_lowercase());
                    values
                        .next()
                        .map(|first| values.fold(first, |all, v| all + ", " + &v))
                };
                let (connection, proxy) = (header("connection"), header("proxy-connection"));
                let has =
                    |h: &Option<String>, word: &str| h.as_ref().is_some_and(|h| h.contains(word));
                let closes = has(&connection, "close")
                    || has(&proxy, "close")
                    || heard.old
                        && connection.is_none_or(|c| c.is_empty())
                        && !has(&proxy, "keep-alive");
                channel.last = status;
                channel.open = !closes;
                let lines: Vec<&str> = heard.asks.iter().map(|l| l.trim()).collect();
                if status != 401 || !lines.iter().any(|l| supported(l)) {
                    return (false, false);
                }
                let (method, value) = strongest(&lines);
                let auth = channel.auth.get_or_insert_with(Authenticator::new);
                auth.parse(method, value);
                if auth.method == Method::None {
                    return (false, false);
                }
                let mut copy = false;
                let begins = auth.phase == Phase::Start && method == Method::Ntlm;
                if auth.phase == Phase::Done || begins {
                    if begins {
                        auth.phase = Phase::Phase1;
                    }
                    if std::mem::take(&mut channel.sent) {
                        auth.phase = Phase::Done;
                    }
                    let differs = |a: &Authenticator, user: &str, password: &str| {
                        a.user != user || a.password != password
                    };
                    let (user, password) = (seat.user.clone(), seat.password.clone());
                    let found = cache.fetch(seat, &auth.realm);
                    let asked = std::mem::replace(&mut seat.asked, true);
                    if seat.channel.is_none() {
                        if let Some((user, password)) = found.filter(|_| !asked) {
                            set_login(&mut channel.auth, &user, &password);
                        }
                    } else if !asked
                        && !user.is_empty()
                        && !password.is_empty()
                        && differs(auth, &user, &password)
                    {
                        set_login(&mut channel.auth, &user, &password);
                        if let Some(auth) = &channel.auth {
                            cache.store(&seat.host, auth);
                        }
                    } else if let Some((user, password)) =
                        found.filter(|(u, p)| !asked && differs(auth, u, p))
                    {
                        set_login(&mut channel.auth, &user, &password);
                    } else {
                        cache.store(&seat.host, auth);
                    }
                    copy = channel
                        .auth
                        .as_ref()
                        .is_some_and(|a| a.phase != Phase::Done);
                    if !copy {
                        // Nothing new to answer with: the page gets the 401 and Qt
                        // closes the socket.
                        channel.auth = None;
                        channel.open = false;
                        return (false, false);
                    }
                } else if auth.phase == Phase::Start {
                    cache.store(&seat.host, auth);
                }
                (true, copy)
            })
        }

        /// The reply was read to its end: its socket stays as the reply left it.
        pub(crate) fn ended(&mut self) {
            self.flying.store(0, Ordering::SeqCst);
        }
    }

    impl Drop for Seat {
        /// Frees the channel for the request that waits first; a reply that did not come
        /// whole closed its socket.
        fn drop(&mut self) {
            let whole = self.flying.swap(0, Ordering::SeqCst) == 0;
            self.on_channel(|_, channel, _| {
                channel.busy = false;
                channel.open &= whole;
                ((), false)
            });
            if self.channel.is_none() {
                return;
            }
            let mut all = self.logins.lock();
            let Logins { hosts, now, .. } = &mut *all;
            if let Some(host) = hosts.get_mut(&self.host) {
                host.left(now());
            }
            self.logins.freed.notify_all();
        }
    }

    #[cfg(test)]
    mod tests {
        use super::super::Heard;
        use super::*;

        fn asks(challenge: &str) -> Heard {
            Heard {
                old: false,
                asks: vec![challenge.into()],
            }
        }

        /// What `seat` sends with a request that has no headers.
        fn sends(seat: &mut Seat) -> String {
            let answer = seat.headers("GET", &mut vec![]);
            answer.map_or(String::new(), |a| chars(&a))
        }

        fn seat(logins: &Shared, synchronous: bool, url: &str, user: &str, password: &str) -> Seat {
            Seat::new(logins, None, synchronous, url, user, password).unwrap()
        }

        #[test]
        fn a_digest_answer_is_the_one_rfc_2617_shows() {
            let mut a = Authenticator::new();
            assert_eq!(a.cnonce.len(), 32);
            a.user = "Mufasa".into();
            a.password = "Circle Of Life".into();
            a.cnonce = "0a4f113b".into();
            a.parse(
                Method::Digest,
                r#"realm="testrealm@host.com", qop="auth,auth-int", nonce="dcd98b7102dd2f0e8b11d0f600bfb0c093", opaque="5ccc069c403ebaf9f0171e9517f40e41""#,
            );
            assert_eq!(
                chars(&a.response("GET", "/dir/index.html")),
                r#"Digest username="Mufasa", realm="testrealm@host.com", nonce="dcd98b7102dd2f0e8b11d0f600bfb0c093", uri="/dir/index.html", opaque="5ccc069c403ebaf9f0171e9517f40e41", response="6629fae49393a05397450978507c4ef1", qop=auth, nc=00000001, cnonce="0a4f113b""#
            );
            assert!(chars(&a.response("GET", "/dir/index.html")).contains("nc=00000002"));
        }

        #[test]
        fn a_digest_answer_writes_the_login_in_latin_1_and_the_realm_as_it_came() {
            let mut a = Authenticator::new();
            a.user = "пé".into();
            a.password = "€".into();
            a.cnonce = "c".into();
            a.parse(Method::Digest, &chars(b"realm=\"r\xc3\xa9\", nonce=\"n\""));
            let said = a.response("GET", "/");
            let hash = |s: &[u8]| crate::builtins::md5_hex(s);
            let ha1 = hash(b"?\xe9:r\xc3\xa9:?");
            let right = hash(format!("{ha1}:n:{}", hash(b"GET:/")).as_bytes());
            let mut want =
                b"Digest username=\"?\xe9\", realm=\"r\xc3\xa9\", nonce=\"n\", ".to_vec();
            want.extend(format!("uri=\"/\", response=\"{right}\"").bytes());
            assert_eq!(said, want);
        }

        #[test]
        fn a_challenge_is_read_as_qt_reads_it() {
            let o = options(r#"realm="a \"b\", c", nonce=n1 , qop="auth-int,auth""#);
            assert_eq!(o["realm"], r#"a "b", c"#);
            assert_eq!(o["nonce"], "n1 ");
            assert_eq!(o["qop"], "auth");
            assert!(options(r#"realm="r", qop="auth-int""#).is_empty());
            assert!(supported("BASIC") && supported("digest x") && !supported("Basicx realm"));
            assert!(supported("NTLM") && !supported("Negotiate"));
            let lines = [
                "Digest algorithm=SHA-256, nonce=n",
                "basic realm=\"r\"",
                "Bearer",
            ];
            assert!(strongest(&lines) == (Method::Basic, "realm=\"r\""));
            let lines = ["Basic realm=\"r\"", "Digest algorithm=md5-sess"];
            assert!(strongest(&lines) == (Method::Digest, "algorithm=md5-sess"));
            let lines = ["Negotiate", "NTLM", "Digest nonce=n"];
            assert!(strongest(&lines) == (Method::Ntlm, ""));
            assert_eq!(latin1("п:é"), b"?:\xe9");
        }

        #[test]
        fn an_address_gives_its_login_up() {
            let (url, user, password) = split("http://a%41%40:p%3Aw@host:8/x@y?z");
            assert_eq!(
                (url.as_str(), user.as_str(), password.as_str()),
                ("http://host:8/x@y?z", "aA@", "p:w")
            );
            assert_eq!(split("http://host/x@y").0, "http://host/x@y");
        }

        #[test]
        fn a_login_is_remembered_until_it_is_refused() {
            let logins = Shared::default();
            let (basic, none) = (asks("Basic realm=\"r\""), Heard::default());
            let mut at = seat(&logins, false, "http://h/a", "u", "p");
            assert_eq!(sends(&mut at), "");
            assert!(at.answers(401, &[], &basic));
            assert_eq!(sends(&mut at), "Basic dTpw");
            assert!(!at.answers(200, &[], &none));
            at.ended();
            drop(at);
            // A challenge Qt does not know changes nothing.
            let mut at = seat(&logins, false, "http://h/b", "", "");
            assert_eq!(sends(&mut at), "Basic dTpw");
            assert!(!at.answers(401, &[], &asks("Bearer realm=\"r\"")));
            at.ended();
            drop(at);
            // After a 401 the login replaces the page's own header.
            let mut at = seat(&logins, false, "http://h/c", "", "");
            let mut headers = vec![("Authorization".to_string(), "Own".to_string())];
            let answer = at.headers("GET", &mut headers);
            assert_eq!((answer, headers), (Some(b"Basic dTpw".to_vec()), vec![]));
            assert!(!at.answers(401, &[], &basic));
            drop(at);
            let mut at = seat(&logins, false, "http://h/d", "", "");
            assert_eq!(sends(&mut at), "");
            // The refused login is still the one a challenge finds.
            assert!(at.answers(401, &[], &basic));
            assert_eq!(sends(&mut at), "Basic dTpw");
            drop(at);
            // Another engine knows nothing of it.
            let mut at = seat(&Shared::default(), false, "http://h/a", "", "");
            assert!(sends(&mut at).is_empty() && !at.answers(401, &[], &basic));
        }

        #[test]
        fn a_synchronous_request_asks_for_the_login_only_when_challenged() {
            let logins = Shared::default();
            let basic = asks("Basic realm=\"r\"");
            let mut at = seat(&logins, false, "http://h/a", "u", "p");
            assert!(sends(&mut at).is_empty() && at.answers(401, &[], &basic));
            drop(at);
            let mut at = seat(&logins, true, "http://h/a", "", "");
            assert_eq!(sends(&mut at), "");
            assert!(at.answers(401, &[], &basic));
            assert_eq!(sends(&mut at), "Basic dTpw");
            assert!(!at.answers(401, &[], &basic));
            drop(at);
            // Its connection was its own: the host's is as it was.
            let mut at = seat(&logins, false, "http://h/a", "", "");
            assert_eq!(sends(&mut at), "Basic dTpw");
            let mut at = seat(&logins, true, "http://other/a", "u", "p");
            assert!(sends(&mut at).is_empty() && at.answers(401, &[], &basic));
            drop(at);
            assert!(!logins.lock().hosts.contains_key("http://other"));
        }

        /// A request that gets the login `u` and `p` on its channel and is answered whole.
        fn logs_in(logins: &Shared) {
            let mut at = seat(logins, false, "http://h/a", "u", "p");
            assert!(sends(&mut at).is_empty() && at.answers(401, &[], &asks("Basic realm=\"r\"")));
            assert_eq!(sends(&mut at), "Basic dTpw");
            assert!(!at.answers(200, &[], &Heard::default()));
            at.ended();
        }

        #[test]
        fn a_connection_unused_for_two_minutes_starts_anew() {
            let logins = Shared::default();
            let start = Instant::now();
            let passed = Arc::new(Mutex::new(Duration::ZERO));
            let clock = passed.clone();
            logins.lock().now = Box::new(move || start + *clock.lock().unwrap());
            let pass = |d: Duration| *passed.lock().unwrap() += d;
            logs_in(&logins);
            // The time counts from the end of the last request, and a request in between
            // starts it again.
            pass(UNUSED - Duration::from_secs(1));
            let mut at = seat(&logins, false, "http://h/b", "", "");
            assert_eq!(sends(&mut at), "Basic dTpw");
            // However long the request takes, the connection is in use.
            pass(UNUSED * 2);
            let mut other = seat(&logins, false, "http://h/c", "", "");
            assert_eq!(sends(&mut other), "");
            assert!(!at.answers(200, &[], &Heard::default()));
            at.ended();
            drop((at, other));
            pass(UNUSED - Duration::from_secs(1));
            let mut at = seat(&logins, false, "http://h/d", "", "");
            assert_eq!(sends(&mut at), "Basic dTpw");
            assert!(!at.answers(200, &[], &Heard::default()));
            at.ended();
            drop(at);
            // After two minutes without a request the channels know nothing; the login
            // is still found for a challenge.
            pass(UNUSED);
            let mut at = seat(&logins, false, "http://h/e", "", "");
            assert_eq!(sends(&mut at), "");
            assert!(at.answers(401, &[], &asks("Basic realm=\"r\"")));
            assert_eq!(sends(&mut at), "Basic dTpw");
        }

        #[test]
        fn a_reply_says_whether_its_socket_stays() {
            let old = Heard {
                old: true,
                asks: vec![],
            };
            let said = |name: &str, value: &str| vec![(name.to_string(), value.to_string())];
            // The page's own header goes out where the socket stayed, the login where it
            // has to connect again.
            type Case<'a> = (&'a Heard, Vec<(String, String)>, bool);
            let cases: [Case; 8] = [
                (&old, vec![], true),
                (&old, said("Connection", "Keep-Alive"), false),
                (&old, said("connection", "x"), false),
                (&old, said("Proxy-Connection", "keep-alive"), false),
                (&Heard::default(), vec![], false),
                (
                    &Heard::default(),
                    said("Connection", "Keep-Alive, Close"),
                    true,
                ),
                (&Heard::default(), said("Connection", "disclosed"), true),
                (&Heard::default(), said("Proxy-Connection", "CLOSE"), true),
            ];
            for (heard, headers, closed) in cases {
                let logins = Shared::default();
                logs_in(&logins);
                let mut at = seat(&logins, false, "http://h/a", "", "");
                assert_eq!(sends(&mut at), "Basic dTpw");
                assert!(!at.answers(200, &headers, heard));
                at.ended();
                drop(at);
                let mut at = seat(&logins, false, "http://h/a", "", "");
                let mut own = vec![("Authorization".to_string(), "Own".to_string())];
                assert_eq!(at.headers("GET", &mut own), None);
                // A challenge now finds a socket that sent its login, or one that did not.
                assert_eq!(
                    at.answers(401, &[], &asks("Basic realm=\"r\"")),
                    closed,
                    "{headers:?}"
                );
                at.ended();
            }
        }

        #[test]
        fn a_seventh_request_waits_and_a_stopped_one_leaves_its_channel_dead() {
            let logins = Shared::default();
            let mut six: Vec<Seat> = (0..6)
                .map(|_| seat(&logins, false, "http://h/a", "", ""))
                .collect();
            let channels = |seats: &[Seat]| seats.iter().map(|s| s.channel).collect::<Vec<_>>();
            assert_eq!(channels(&six), (0..6).map(Some).collect::<Vec<_>>());
            let (tx, rx) = std::sync::mpsc::channel();
            let theirs = logins.clone();
            let seventh = std::thread::spawn(move || {
                let at = seat(&theirs, false, "http://h/a", "", "");
                tx.send(at.channel).unwrap();
            });
            // It is in the line before the third channel is freed, and gets that one.
            let mut all = logins.lock();
            while all.hosts["http://h"].waiting.is_empty() {
                all = logins.freed.wait(all).unwrap();
            }
            drop(all);
            drop(six.remove(2));
            assert_eq!(rx.recv().unwrap(), Some(2));
            seventh.join().unwrap();
            // A stop of a request that is out kills its channel.
            let stop = crate::svc::Stop::default();
            super::super::with_stop(&stop, || {
                let mut at = seat(&logins, false, "http://h/a", "", "");
                assert_eq!((at.channel, sends(&mut at)), (Some(2), String::new()));
                stop.store(true, Ordering::SeqCst);
            });
            assert!(logins.lock().hosts["http://h"].channels[2].dead);
            // One that still waits for a channel only leaves the line.
            let stop = Arc::new(crate::svc::Stop::default());
            let (theirs, stops) = (logins.clone(), stop.clone());
            let stopping = std::thread::spawn(move || {
                let mut all = theirs.lock();
                while all.hosts["http://h"].waiting.is_empty() {
                    all = theirs.freed.wait(all).unwrap();
                }
                drop(all);
                stops.store(true, Ordering::SeqCst);
            });
            super::super::with_stop(&stop, || {
                assert!(Seat::new(&logins, None, false, "http://h/a", "", "").is_none());
            });
            stopping.join().unwrap();
            assert!(logins.lock().hosts["http://h"].waiting.is_empty());
            // A place that no request took is given up.
            drop(Place::take(&logins, "http://h/a"));
            assert!(logins.lock().hosts["http://h"].waiting.is_empty());
            drop(six.remove(0));
            assert_eq!(seat(&logins, false, "http://h/a", "", "").channel, Some(0));
            // Six dead channels: the connection starts anew.
            drop(six);
            for c in &mut logins.lock().hosts.get_mut("http://h").unwrap().channels {
                c.dead = true;
            }
            assert_eq!(seat(&logins, false, "http://h/a", "", "").channel, Some(0));
        }

        /// Qt 6.12.0, an `abort()` of a request the server holds on the socket that
        /// answered a Basic challenge: the two requests after it went out without an
        /// `Authorization`, a challenge then was answered with the login, which went with
        /// the request after that.
        #[test]
        fn a_stopped_request_takes_the_login_of_its_channel_with_it() {
            let logins = Shared::default();
            logs_in(&logins);
            let stop = crate::svc::Stop::default();
            super::super::with_stop(&stop, || {
                let mut at = seat(&logins, false, "http://h/b", "", "");
                assert_eq!((at.channel, sends(&mut at)), (Some(0), "Basic dTpw".into()));
                stop.store(true, Ordering::SeqCst);
            });
            for path in ["c", "d"] {
                let mut at = seat(&logins, false, &format!("http://h/{path}"), "", "");
                assert_eq!((at.channel, sends(&mut at)), (Some(1), String::new()));
                assert!(!at.answers(200, &[], &Heard::default()));
                at.ended();
            }
            let mut at = seat(&logins, false, "http://h/e", "", "");
            assert!(sends(&mut at).is_empty() && at.answers(401, &[], &asks("Basic realm=\"r\"")));
            assert_eq!(sends(&mut at), "Basic dTpw");
            assert!(!at.answers(200, &[], &Heard::default()));
            at.ended();
            drop(at);
            let mut at = seat(&logins, false, "http://h/f", "", "");
            assert_eq!((at.channel, sends(&mut at)), (Some(1), "Basic dTpw".into()));
        }

        /// A difference written down in docs/qt-differences.md. Qt 6.12.0 after an
        /// `abort()` of six requests the server held, one on each socket of the host: the
        /// requests after them never went out. Here the seventh goes out on a connection
        /// that starts anew: its first socket, no login sent before a challenge asks.
        #[test]
        fn six_stopped_requests_start_the_connection_anew() {
            let logins = Shared::default();
            logs_in(&logins);
            for channel in 0..CHANNELS {
                let stop = crate::svc::Stop::default();
                super::super::with_stop(&stop, || {
                    let mut at = seat(&logins, false, "http://h/b", "", "");
                    let login = if channel == 0 { "Basic dTpw" } else { "" };
                    assert_eq!((at.channel, sends(&mut at)), (Some(channel), login.into()));
                    stop.store(true, Ordering::SeqCst);
                });
            }
            let dead = |logins: &Shared| {
                let all = logins.lock();
                all.hosts["http://h"]
                    .channels
                    .iter()
                    .filter(|c| c.dead)
                    .count()
            };
            assert_eq!(dead(&logins), CHANNELS);
            let mut at = seat(&logins, false, "http://h/c", "", "");
            assert_eq!((at.channel, sends(&mut at)), (Some(0), String::new()));
            assert_eq!(dead(&logins), 0);
            assert!(at.answers(401, &[], &asks("Basic realm=\"r\"")));
            assert_eq!(sends(&mut at), "Basic dTpw");
        }

        #[test]
        fn ntlm_is_answered_with_the_messages_qt_sends() {
            use base64::Engine;
            let hex = |b: &[u8]| b.iter().map(|b| format!("{b:02x}")).collect::<String>();
            let base64 = |b: &[u8]| base64::engine::general_purpose::STANDARD.encode(b);
            let mut a = Authenticator::new();
            a.user = "u".into();
            a.password = "p".into();
            a.parse(Method::Ntlm, "");
            a.phase = Phase::Phase1;
            assert_eq!(
                chars(&a.response("GET", "/")),
                "NTLM TlRMTVNTUAABAAAABYIIAAAAAAAAAAAAAAAAAAAAAAA="
            );
            assert!(a.phase == Phase::Phase2);
            // A type 2 message with the flags 0x00888205, the target `DOM` and the
            // challenge 1 to 8, as the server of xhr_equiv sends it; the head of the type 3
            // message is what Qt 6.12.0 answered it with there.
            let mut type2 = b"NTLMSSP\0\x02\0\0\0\x06\0\x06\0\x30\0\0\0\x05\x82\x88\0".to_vec();
            type2.extend([1, 2, 3, 4, 5, 6, 7, 8]);
            type2.extend([0; 16]);
            type2.extend(utf16("DOM"));
            a.parse(Method::Ntlm, &base64(&type2));
            let said = chars(&a.response("GET", "/"));
            assert!(a.phase == Phase::Done && a.challenge.is_empty());
            let m = from_base64(said.strip_prefix("NTLM ").unwrap());
            assert_eq!(
                hex(&m[..72]),
                "4e544c4d53535000030000001800180048000000300030006000000006000600400000000200020046\
                 000000000000004800000000000000000000000182880044004f004d007500"
            );
            // LMv2 and NTLMv2 of RFC-less MS-NLMP 3.3.2: the key is of the user in capitals
            // and the domain, the hashes are of the server's challenge and what follows them.
            let key = hmac_md5(
                &{
                    use md4::Digest;
                    md4::Md4::digest(utf16("p"))
                },
                &utf16("UDOM"),
            );
            let nonce = &a.cnonce.as_bytes()[24..];
            let (lm, nt) = m[72..].split_at(24);
            let right = |rest: &[u8]| hmac_md5(&key, &[&[1, 2, 3, 4, 5, 6, 7, 8], rest].concat());
            assert_eq!(lm, [right(nonce), nonce.to_vec()].concat());
            assert_eq!(
                (&nt[..16], &nt[16..24]),
                (&right(&nt[16..])[..], &[1, 1, 0, 0, 0, 0, 0, 0][..])
            );
            assert_eq!((&nt[32..40], &nt[40..]), (nonce, &[0; 8][..]));
            // RFC 2202's second test of HMAC-MD5, and RFC 1320's MD4 of "abc".
            assert_eq!(
                hex(&hmac_md5(b"Jefe", b"what do ya want for nothing?")),
                "750c783e6ab0b503eaa86e310a5db738"
            );
            use md4::Digest;
            assert_eq!(
                hex(&md4::Md4::digest(b"abc")),
                "a448017aaf21d8525fc10ae87aa6729d"
            );
            // A message that is no type 2 one is answered with nothing.
            a.parse(Method::Ntlm, "AAAA");
            assert_eq!(chars(&a.response("GET", "/")), "NTLM");
        }
    }
}

#[cfg(test)]
mod tests;
