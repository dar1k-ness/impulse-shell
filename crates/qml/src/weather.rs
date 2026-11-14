//! Refetch of failed the original weather fetches once the network is up. services/Weather.qml refetches
//! only on its `fetchInterval` timer (10 min by default) or a GPS fix, so a fetch that failed for
//! lack of a connection (offline at login) left the bar without weather that long. Native follows
//! NetworkManager's `State`, subscribed to before the first fetch runs and then read: a
//! `WeatherFetch` that fails emits `networkRestored` at once if NM reached
//! NM_STATE_CONNECTED_GLOBAL while it was in flight, and when NM reports it if it is not
//! connected; Weather.qml fetches again in its handler. A failure while online throughout is
//! left to the timer. A failure keeps the previous data. The subscription is a system-bus
//! service ([`crate::sysbus::spawn`]): when the bus connection closes it subscribes and reads
//! `State` again on the next one.

use crate::runtime::World;
use crate::svc::{Sender, Stop, Svc};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Receiver;
use zbus::message::Sequence;
use zbus::zvariant::OwnedValue;

/// The fetch job's exit code when the request failed (no data, nothing parsed).
pub const FAILED: i32 = 7;
const NM: &str = "org.freedesktop.NetworkManager";
const NM_PATH: &str = "/org/freedesktop/NetworkManager";
/// NM_STATE_CONNECTED_GLOBAL.
const CONNECTED: u32 = 70;
/// NM_STATE_UNKNOWN: "networking state is unknown".
const UNKNOWN: u32 = 0;

/// Where the Weather fetch stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fetch {
    Idle,
    /// In flight; `rose`: NM turned connected since it started.
    Running {
        rose: bool,
    },
    /// Failed while not connected: refetch when NM reports connectivity.
    Offline,
    /// A refetch is queued for the next poll.
    Due,
}

/// What [`status`] reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Status {
    /// NM's `State`, once known; `NM_STATE_UNKNOWN` (0) once a read of it failed.
    pub state: Option<u32>,
    pub fetch: Fetch,
    /// Refetches started here so far.
    pub refetches: u32,
}

/// NM's state, or a wakeup for a queued refetch.
enum Ev {
    /// `State` at a bus position of the subscription's connection `session`.
    State {
        session: u64,
        state: u32,
        at: Sequence,
    },
    /// The subscription's read of `State` failed, or was not answered.
    Unread,
    Wake,
}

/// The NetworkManager subscription of the `WeatherFetch` item `fetch`; dropping it ends it.
pub struct Net {
    fetch: usize,
    stop: Arc<Stop>,
    tx: Sender<Ev>,
    rx: Receiver<Ev>,
    /// The connection the newest state applied came on, and its bus position there: the
    /// initial read and the signals race, and positions of two connections do not compare.
    at: Option<(u64, Sequence)>,
    status: Status,
}

impl Drop for Net {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

impl Net {
    pub fn fetch(&self) -> usize {
        self.fetch
    }

    fn new(w: &World, fetch: usize) -> Net {
        let (tx, rx) = w.waker.handle(Svc::Weather).channel();
        let stop = Arc::new(Stop::default());
        if let Some(rule) = rule() {
            let (t, s) = (tx.clone(), stop.clone());
            let mut session = 0;
            crate::sysbus::spawn(vec![rule], stop.clone(), move |c, signals| {
                session += 1;
                watch(c, signals, &t, &s, session)
            });
        }
        Net {
            fetch,
            stop,
            tx,
            rx,
            at: None,
            status: Status {
                state: None,
                fetch: Fetch::Idle,
                refetches: 0,
            },
        }
    }

    fn drain(&mut self) {
        while let Ok(ev) = self.rx.try_recv() {
            let (session, s, at) = match ev {
                Ev::State { session, state, at } => (session, state, at),
                // Nothing says NM was connected: the first CONNECTED known after this, a
                // signal's or a later read's, is a transition, as from any other state.
                Ev::Unread => {
                    self.status.state.get_or_insert(UNKNOWN);
                    continue;
                }
                Ev::Wake => continue,
            };
            if self.at.is_some_and(|a| (session, at) <= a) {
                continue;
            }
            self.at = Some((session, at));
            // The first State read is the one NM was already in, not a transition: a fetch that
            // failed before it arrived failed while online when it is connected, and Weather's
            // timer covers that. The State read on a later connection is compared with the last
            // one known: NM connecting while the bus was away is a transition.
            let Some(prev) = self.status.state.replace(s) else {
                if s == CONNECTED && self.status.fetch == Fetch::Offline {
                    self.status.fetch = Fetch::Idle;
                }
                continue;
            };
            if s == CONNECTED && prev != CONNECTED {
                match self.status.fetch {
                    Fetch::Running { .. } => self.status.fetch = Fetch::Running { rose: true },
                    Fetch::Offline => self.queue(),
                    Fetch::Idle | Fetch::Due => {}
                }
            }
        }
    }

    fn queue(&mut self) {
        self.status.fetch = Fetch::Due;
        let _ = self.tx.send(Ev::Wake);
    }
}

/// NM's `StateChanged`, from NM's current owner.
fn rule() -> Option<zbus::MatchRule<'static>> {
    zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender(NM)
        .and_then(|b| b.path(NM_PATH))
        .and_then(|b| b.interface(NM))
        .and_then(|b| b.member("StateChanged"))
        .ok()
        .map(|b| b.build())
}

/// The subscription on one connection (`session`): reads `State`, then follows `StateChanged`
/// from `signals`, subscribed before the read; both go to `tx` with their bus positions. Ends
/// with the connection (true) or when Weather is gone (false).
fn watch(
    c: &zbus::blocking::Connection,
    signals: &Receiver<zbus::Message>,
    tx: &Sender<Ev>,
    stop: &Stop,
    session: u64,
) -> bool {
    let state = |state: u32, at: Sequence| Ev::State { session, state, at };
    {
        // NetworkManager may never answer: the stop ends the wait for its reply, and so does
        // the time a reply is given, after which its signals are followed all the same.
        let (stopped_tx, stopped) = async_channel::bounded::<()>(1);
        let _close = stop.on_stop(move || drop(stopped_tx));
        let get = c.inner().call_method(
            Some(NM),
            NM_PATH,
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &(NM, "State"),
        );
        let get = crate::sysbus::within_async(get);
        let r = zbus::block_on(futures_lite::future::or(async { Some(get.await) }, async {
            let _ = stopped.recv().await;
            None
        }));
        let Some(r) = r else {
            return false;
        };
        let read = r.and_then(|m| {
            let s = u32::try_from(m.body().deserialize::<OwnedValue>()?)?;
            Ok(state(s, m.recv_position()))
        });
        let ev = read.unwrap_or_else(|e| {
            elog::eprintln!("weather: cannot read NetworkManager's state: {e}");
            Ev::Unread
        });
        if tx.send(ev).is_err() {
            return false;
        }
    }
    for m in signals.iter() {
        if let Ok(s) = m.body().deserialize::<u32>()
            && tx.send(state(s, m.recv_position())).is_err()
        {
            return false;
        }
    }
    true
}

fn is_fetcher(w: &World, p: usize) -> bool {
    crate::procs::type_of(w, p) == Some("WeatherFetch")
}

/// The subscription of the `WeatherFetch` `p`, made on its first fetch.
fn net(w: &mut World, p: usize) -> Option<&mut Net> {
    if !is_fetcher(w, p) {
        return None;
    }
    if w.weather_net.as_ref().is_none_or(|n| n.fetch != p) {
        let n = Net::new(w, p);
        w.weather_net = Some(n);
    }
    let n = w.weather_net.as_mut()?;
    n.drain();
    Some(n)
}

/// Before the fetch `Process` `p` starts.
pub fn started(w: &mut World, p: usize) {
    if let Some(n) = net(w, p) {
        n.status.fetch = Fetch::Running { rose: false };
    }
}

/// After the fetch `Process` `p` exited with `code`.
pub fn exited(w: &mut World, p: usize, code: i32) {
    let Some(n) = net(w, p) else {
        return;
    };
    match n.status.fetch {
        _ if code == 0 => n.status.fetch = Fetch::Idle,
        Fetch::Running { rose: true } => n.queue(),
        _ if n.status.state == Some(CONNECTED) => n.status.fetch = Fetch::Idle,
        _ => n.status.fetch = Fetch::Offline,
    }
}

/// The `WeatherFetch` whose fetch failed offline, once the network is back: its
/// `networkRestored` is due.
pub fn due(w: &mut World) -> Option<usize> {
    let n = w.weather_net.as_mut()?;
    if w.dead.contains(&n.fetch) {
        w.weather_net = None;
        return None;
    }
    n.drain();
    if n.status.fetch != Fetch::Due {
        return None;
    }
    n.status.fetch = Fetch::Idle;
    n.status.refetches += 1;
    Some(n.fetch)
}

/// The Weather fetch's state, once its first fetch ran.
pub fn status(w: &World) -> Option<Status> {
    w.weather_net.as_ref().map(|n| n.status)
}
