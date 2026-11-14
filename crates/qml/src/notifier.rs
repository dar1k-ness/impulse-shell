//! `Notify` of the `Impulse` module: the notifications the shell itself shows, sent over
//! D-Bus the way libnotify 0.8's `notify-send SUMMARY BODY [-u critical] -a APP
//! [--hint=int:transient:1] [-A LABEL]...` sends them. Unlike `notify-send`, the body is never
//! unescaped, and an invoked action is reported by its index from 0.
//!
//! It also carries the two things the shell's notification list needs from our notification
//! server that Quickshell's `NotificationServer` does not have: `notificationReplaced(id)`
//! and `forgetImage(source)`.
//!
//! ```qml
//! import Impulse
//! const id = Notify.send(summary, body, { critical: true, actions: ["Open", "Dismiss"] })
//! Connections { target: Notify; function onActionInvoked(id, index) { ... } }
//! ```

use crate::eval::{Interp, R};
use crate::libnotify::{IFACE, PATH};
use crate::runtime::World;
use crate::value::Value;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Condvar, Mutex};

/// The `Notify` singleton.
pub static TYPE: crate::impulse::Type = crate::impulse::Type {
    name: "Notify",
    methods: &[("forgetImage", forget_image), ("send", send_method)],
    init: None,
};

/// One notification with the options `notify-send` was given for it.
#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    /// `-a`
    pub app: String,
    pub summary: String,
    pub body: String,
    pub critical: bool,
    pub transient: bool,
    pub actions: Vec<String>,
}

impl Note {
    /// A notification of the shell itself (`-a Shell`).
    pub fn shell(summary: impl Into<String>, body: impl Into<String>) -> Note {
        Note {
            app: "Shell".into(),
            summary: summary.into(),
            body: body.into(),
            critical: false,
            transient: false,
            actions: vec![],
        }
    }

    /// `-u critical`
    pub fn critical(mut self) -> Note {
        self.critical = true;
        self
    }

    /// `--hint=int:transient:1`
    pub fn transient(mut self) -> Note {
        self.transient = true;
        self
    }

    /// `-A label`
    pub fn action(mut self, label: &str) -> Note {
        self.actions.push(label.into());
        self
    }

    /// The `notify-send` command line for this notification (logs, the spawn hook).
    pub fn argv(&self) -> Vec<String> {
        let mut a = vec![
            "notify-send".into(),
            self.summary.clone(),
            self.body.clone(),
        ];
        if self.critical {
            a.extend(["-u".into(), "critical".into()]);
        }
        for l in &self.actions {
            a.extend(["-A".into(), l.clone()]);
        }
        a.extend(["-a".into(), self.app.clone()]);
        if self.transient {
            a.push("--hint=int:transient:1".into());
        }
        a
    }

    /// Puts the notification on the bus without waiting for the server's answer: it is out
    /// when this returns, so a caller that quits right after (`Qt.quit()`) still sent it, and
    /// the shell, whose own server answers from its main loop, does not wait for itself.
    pub fn post(&self, conn: &zbus::blocking::Connection) -> bool {
        let hints = crate::libnotify::hints(self.critical, self.transient);
        let m = zbus::Message::method_call(PATH, "Notify")
            .and_then(|b| b.destination(IFACE))
            .and_then(|b| b.interface(IFACE))
            .and_then(|b| b.with_flags(zbus::message::Flags::NoReplyExpected))
            .and_then(|b| {
                b.build(&(
                    self.app.as_str(),
                    0u32,
                    "",
                    self.summary.as_str(),
                    self.body.as_str(),
                    Vec::<&str>::new(),
                    hints,
                    -1i32,
                ))
            });
        match m.and_then(|m| conn.send(&m)) {
            Ok(()) => true,
            Err(e) => {
                elog::eprintln!("notify: {e}");
                false
            }
        }
    }

    /// The button labels the server at `conn` can show, and `notify-send`'s exit code so far:
    /// a server without the `actions` capability shows the notification without buttons, and
    /// that is exit code 1.
    fn buttons(&self, conn: &zbus::blocking::Connection) -> (&[String], i32) {
        if self.actions.is_empty() {
            return (&[], 0);
        }
        if shows_buttons(conn.call_method(Some(IFACE), PATH, Some(IFACE), "GetCapabilities", &())) {
            return (&self.actions, 0);
        }
        (&[], 1)
    }

    /// The arguments of `Notify` with the buttons `labels`, each under its key of `keys`.
    fn args<'a>(&'a self, keys: &'a [String], labels: &'a [String]) -> Args<'a> {
        let actions = keys
            .iter()
            .zip(labels)
            .flat_map(|(k, l)| [k.as_str(), l.trim()])
            .collect();
        let hints = crate::libnotify::hints(self.critical, self.transient);
        (
            &self.app,
            0,
            "",
            &self.summary,
            &self.body,
            actions,
            hints,
            -1,
        )
    }

    /// `Notify` with the buttons `labels`, each under its index from 0 as the action key: the
    /// id the server gave the notification, which its `ActionInvoked` and `NotificationClosed`
    /// name.
    fn call(&self, conn: &zbus::blocking::Connection, labels: &[String]) -> Option<u32> {
        let keys = keys(labels);
        let r = conn.call_method(
            Some(IFACE),
            PATH,
            Some(IFACE),
            "Notify",
            &self.args(&keys, labels),
        );
        shown_as(r)
    }

    /// `notify-send`'s exit code and stdout, on the session bus or `bus`: with actions, waits
    /// like `notify-send -A` and prints the invoked action's index counted from 0.
    pub fn show(
        &self,
        bus: Option<&str>,
        stop: &std::sync::Arc<crate::svc::Stop>,
    ) -> (i32, String) {
        let mut out = String::new();
        let code = self.show_with(bus, stop, |k| out.push_str(&format!("{k}\n")));
        (code, out)
    }

    /// [`Note::show`] that hands each invoked action's index to `invoked` as it arrives. A
    /// summary or body that starts with `-` is shown as written: `notify-send` given them
    /// before its options parses a song title like `-x - y` as options and shows nothing.
    pub fn show_with(
        &self,
        bus: Option<&str>,
        stop: &std::sync::Arc<crate::svc::Stop>,
        mut invoked: impl FnMut(&str),
    ) -> i32 {
        let Some(conn) = connection(bus) else {
            return 1;
        };
        let (labels, code) = self.buttons(&conn);
        let wait = !labels.is_empty();
        // Subscribed before `Notify`, so no answer to it can be missed.
        let mut signals = None;
        if wait {
            let Ok(rule) = zbus::MatchRule::builder()
                .msg_type(zbus::message::Type::Signal)
                .interface(IFACE)
                .map(|b| b.build())
            else {
                return 1;
            };
            match async_io::block_on(zbus::MessageStream::for_match_rule(
                rule,
                conn.inner(),
                None,
            )) {
                Ok(s) => signals = Some(s),
                Err(_) => return 1,
            }
        }
        let Some(id) = self.call(&conn, labels) else {
            return 1;
        };
        if let Some(mut signals) = signals {
            // Waits on the signal stream and the stop together: no thread, no timeout.
            let (stopped_tx, stopped) = async_channel::bounded::<()>(1);
            let _close = stop.on_stop(move || {
                stopped_tx.close();
            });
            let c = conn.inner();
            async_io::block_on(async {
                use futures_lite::StreamExt;
                loop {
                    let next =
                        futures_lite::future::or(async { Some(signals.next().await) }, async {
                            let _ = stopped.recv().await;
                            None
                        });
                    let m = match next.await {
                        Some(Some(Ok(m))) => m,
                        Some(Some(Err(_))) => continue,
                        _ => break,
                    };
                    match m.header().member().map(|m| m.as_str()) {
                        Some("ActionInvoked") => {
                            if let Ok((i, k)) = m.body().deserialize::<(u32, String)>()
                                && i == id
                            {
                                invoked(&k);
                                let _ = c
                                    .call_method(
                                        Some(IFACE),
                                        PATH,
                                        Some(IFACE),
                                        "CloseNotification",
                                        &(id,),
                                    )
                                    .await;
                            }
                        }
                        Some("NotificationClosed")
                            if m.body()
                                .deserialize::<(u32, u32)>()
                                .is_ok_and(|(i, _)| i == id) =>
                        {
                            break;
                        }
                        _ => {}
                    }
                }
            });
        }
        code
    }
}

/// The arguments of `Notify`.
type Args<'a> = (
    &'a str,
    u32,
    &'a str,
    &'a str,
    &'a str,
    Vec<&'a str>,
    HashMap<&'static str, zbus::zvariant::Value<'static>>,
    i32,
);

/// The action keys of the buttons `labels`: their indexes from 0.
fn keys(labels: &[String]) -> Vec<String> {
    (0..labels.len()).map(|i| i.to_string()).collect()
}

/// Whether the server whose answer to `GetCapabilities` is `caps` shows buttons; `notify-send`
/// says so when it does not.
fn shows_buttons(caps: zbus::Result<zbus::Message>) -> bool {
    let caps: Vec<String> = caps
        .ok()
        .and_then(|m| m.body().deserialize().ok())
        .unwrap_or_default();
    let shows = caps.iter().any(|c| c == "actions");
    if !shows {
        elog::eprintln!(
            "Actions are not supported by this notifications server. Displaying non-interactively."
        );
    }
    shows
}

/// The id of the notification in the server's answer to `Notify`.
fn shown_as(answer: zbus::Result<zbus::Message>) -> Option<u32> {
    let (id,) = answer.ok()?.body().deserialize::<(u32,)>().ok()?;
    Some(id)
}

/// The connection notifications go out on: the shell's shared session-bus connection, or one
/// kept per explicit `bus` address. Each `notify-send` process opens its own; one long-lived
/// connection sends the same calls without a connect and `Hello` per notification.
fn connection(bus: Option<&str>) -> Option<zbus::blocking::Connection> {
    let Some(addr) = bus else {
        return crate::dbus::session();
    };
    static AT: std::sync::Mutex<Vec<(String, zbus::blocking::Connection)>> =
        std::sync::Mutex::new(Vec::new());
    let mut g = AT.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, c)) = g.iter().find(|(a, _)| a == addr) {
        return Some(c.clone());
    }
    let c = crate::dbus::builder(addr).and_then(|b| b.build()).ok()?;
    g.push((addr.to_string(), c.clone()));
    Some(c)
}

/// The singleton's Rust side, kept on its item: ends with it.
struct State {
    /// The id `send` returned last.
    last: std::cell::Cell<u32>,
    /// The sends that wait for a button.
    buttons: Arc<Mutex<Buttons>>,
}

impl Drop for State {
    fn drop(&mut self) {
        let on = {
            let mut b = lock(&self.buttons);
            b.ended = true;
            b.shown.clear();
            b.early.clear();
            b.on.take()
        };
        // Outside the lock: the subscription's sink takes it.
        if let Some((_, end)) = on {
            end.cancel();
        }
    }
}

/// The sends with buttons of one `Notify` singleton that wait for a press or the close, all on
/// one subscription to the server's signals: a notification nobody dismisses holds an entry
/// here, not a thread.
#[derive(Default)]
struct Buttons {
    /// The server's id of each notification shown with buttons, and the id `send` returned
    /// for it.
    shown: HashMap<u32, u32>,
    /// The sends whose `Notify` the server has not answered yet. Each leaves with the answer
    /// or at [`REPLY_LIMIT`].
    unanswered: Vec<Unanswered>,
    /// How many `Notify` calls with buttons were written so far.
    written: u64,
    /// What the server said, before it answered a `Notify`, about an id that may be the one
    /// that answer brings. The shell's own server answers `Notify` one turn after it showed
    /// the notification, so a button pressed in that turn is signalled before the answer.
    ///
    /// Until the answer nothing tells the server's id of our notification from another
    /// application's, so this keeps one entry, of a fixed size, for each id the server names
    /// that may still be ours ([`Buttons::keep`]), and drops it when the last `Notify` written
    /// before it has been answered or has reached [`REPLY_LIMIT`]: an entry lives no longer
    /// than that limit, and there are none while every `Notify` is answered.
    early: Vec<Early>,
    /// The connection the subscription is on, and its end. `None` before the first send with
    /// buttons.
    on: Option<(zbus::blocking::Connection, crate::dbus::Cancel)>,
    /// The singleton is gone: nothing more is shown or reported.
    ended: bool,
}

/// A send with buttons whose `Notify` the server has not answered yet.
struct Unanswered {
    /// The id `send` returned for it.
    id: u32,
    /// Its place among the `Notify` calls written: [`Buttons::written`] after it.
    at: u64,
    /// How many buttons it has: their keys are the indexes below this.
    buttons: usize,
}

/// What the server said about an id before the answer that may bring it.
struct Early {
    server: u32,
    /// The first button pressed: `notify-send -A` takes one press and ends.
    pressed: Option<u32>,
    /// `NotificationClosed`: the id names nothing after it.
    closed: bool,
    /// [`Buttons::written`] when the first signal about the id came. The server cannot have
    /// signalled about a notification it was asked for later.
    after: u64,
}

impl Early {
    /// Whether the id can be the one the server gives `u`: the `Notify` was written before
    /// the server spoke of it, and the pressed button is one of its buttons.
    fn may_be(&self, u: &Unanswered) -> bool {
        u.at <= self.after && self.pressed.is_none_or(|k| (k as usize) < u.buttons)
    }
}

/// What the server said about one of its notifications.
enum Said {
    /// `ActionInvoked` with this action key.
    Pressed(String),
    /// `NotificationClosed`.
    Closed,
}

fn lock(b: &Mutex<Buttons>) -> std::sync::MutexGuard<'_, Buttons> {
    b.lock().unwrap_or_else(|e| e.into_inner())
}

/// The button the action key `key` is, as [`keys`] names buttons.
fn button(key: &str) -> Option<u32> {
    key.parse().ok().filter(|k: &u32| k.to_string() == key)
}

impl Buttons {
    /// The server said `what` about its notification `server`: a pressed button of one of
    /// ours is reported as `actionInvoked` and the notification closed, as `notify-send -A`
    /// closes it; a closed one waits no more.
    fn said(&mut self, server: u32, what: &Said, back: &crate::impulse::Replies) {
        match what {
            Said::Pressed(key) => {
                if let Ok(k) = key.parse::<u32>() {
                    self.pressed(server, k, back);
                }
            }
            Said::Closed => {
                self.shown.remove(&server);
            }
        }
    }

    /// Button `k` of the server's notification `server` was pressed.
    fn pressed(&mut self, server: u32, k: u32, back: &crate::impulse::Replies) {
        let Some(&id) = self.shown.get(&server) else {
            return;
        };
        back.send(move |w| pressed(w, id, k));
        if let Some((conn, _)) = &self.on {
            close(conn, server);
        }
    }

    /// The `Notify` of the send that returned `id`, with `buttons` buttons, is about to be
    /// written.
    fn ask(&mut self, id: u32, buttons: usize) {
        self.written += 1;
        let at = self.written;
        self.unanswered.push(Unanswered { id, at, buttons });
    }

    /// The server said `what` about `server`, which is none of `shown`: kept in `early` only
    /// if it can turn out to be about one of the `unanswered` sends. That is: a `Notify` is
    /// unanswered; a press names a button such a send has, by the key it was sent under; and
    /// the id is not closed already. One press is kept for an id, the first.
    fn keep(&mut self, server: u32, what: &Said) {
        let pressed = match what {
            Said::Pressed(key) => match button(key) {
                Some(k) => Some(k),
                // Not a key of ours: neither is the notification.
                None => return self.early.retain(|e| e.server != server),
            },
            Said::Closed => None,
        };
        let i = match self.early.iter().position(|e| e.server == server) {
            Some(i) => i,
            None => {
                self.early.push(Early {
                    server,
                    pressed: None,
                    closed: false,
                    after: self.written,
                });
                self.early.len() - 1
            }
        };
        let e = &mut self.early[i];
        if !e.closed {
            e.closed = pressed.is_none();
            e.pressed = e.pressed.or(pressed);
        }
        self.forget_early();
    }

    /// The `Notify` of the send that returned `id` is unanswered no more; the server gave the
    /// notification the id `server`, or no answer came. What the server said about that id
    /// before the answer, and nothing is kept about ids only this send could have had.
    fn answer(&mut self, id: u32, server: Option<u32>) -> Option<Early> {
        let sent = self
            .unanswered
            .iter()
            .position(|u| u.id == id)
            .map(|i| self.unanswered.remove(i));
        let early = server
            .and_then(|server| self.early.iter().position(|e| e.server == server))
            .map(|i| self.early.remove(i))
            .filter(|e| sent.as_ref().is_some_and(|u| e.may_be(u)));
        self.forget_early();
        early
    }

    /// Drops from `early` what can be about no unanswered send any more.
    fn forget_early(&mut self) {
        let Buttons {
            early, unanswered, ..
        } = self;
        early.retain(|e| unanswered.iter().any(|u| e.may_be(u)));
    }
}

/// `CloseNotification(id)` without waiting for the answer: the router thread, which hands the
/// answers on, sends it.
fn close(conn: &zbus::blocking::Connection, id: u32) {
    let m = zbus::Message::method_call(PATH, "CloseNotification")
        .and_then(|b| b.destination(IFACE))
        .and_then(|b| b.interface(IFACE))
        .and_then(|b| b.with_flags(zbus::message::Flags::NoReplyExpected))
        .and_then(|b| b.build(&(id,)));
    if let Err(e) = m.and_then(|m| conn.send(&m)) {
        elog::eprintln!("notify: {e}");
    }
}

const DBUS: &str = "org.freedesktop.DBus";

/// The subscription's sink: a signal of the notification server, or the server's name
/// changing hands (the ids in `shown` were the old server's; the new one counts from 1 again).
fn heard(buttons: &Mutex<Buttons>, back: &crate::impulse::Replies, m: &zbus::Message) {
    let h = m.header();
    let (Some(iface), Some(member)) = (h.interface(), h.member()) else {
        return;
    };
    let said = match (iface.as_str(), member.as_str()) {
        (DBUS, "NameOwnerChanged") => {
            let mut b = lock(buttons);
            b.shown.clear();
            b.early.clear();
            return;
        }
        (IFACE, "ActionInvoked") => m
            .body()
            .deserialize::<(u32, String)>()
            .map(|(id, key)| (id, Said::Pressed(key))),
        (IFACE, "NotificationClosed") => m
            .body()
            .deserialize::<(u32, u32)>()
            .map(|(id, _)| (id, Said::Closed)),
        _ => return,
    };
    let Ok((server, said)) = said else {
        return;
    };
    let mut b = lock(buttons);
    if b.shown.contains_key(&server) {
        b.said(server, &said, back);
    } else {
        b.keep(server, &said);
    }
}

/// The server answered the `Notify` of the send that returned `id`, or will not (`answer` is
/// the error: the server's, or [`REPLY_LIMIT`] reached): the notification is one of `shown`
/// under the id the server gave it, and what the server said about that id before the answer
/// is handled now. Without an answer it is not shown as far as anyone can tell, and no button
/// of it is ever reported: `notify-send -A` says "Failed to show notification" and reports
/// none either.
fn answered(
    buttons: &Mutex<Buttons>,
    back: &crate::impulse::Replies,
    id: u32,
    answer: zbus::Result<zbus::Message>,
) {
    if let Err(e) = &answer {
        elog::eprintln!("Failed to show notification: {e}");
    }
    let server = shown_as(answer);
    let mut b = lock(buttons);
    let early = b.answer(id, server);
    let Some(server) = server else {
        return;
    };
    if b.ended {
        return;
    }
    b.shown.insert(server, id);
    let Some(early) = early else {
        return;
    };
    if let Some(k) = early.pressed {
        b.pressed(server, k, back);
    }
    if early.closed {
        b.shown.remove(&server);
    }
}

/// The session bus connection the server's signals reach `buttons` on: subscribed at the
/// first send with buttons, and again on the new connection after the bus was lost.
fn subscribed(
    buttons: &Arc<Mutex<Buttons>>,
    back: &crate::impulse::Replies,
) -> Option<zbus::blocking::Connection> {
    use crate::dbus::Bus;
    let now = crate::dbus::session()?;
    {
        let b = lock(buttons);
        if b.ended {
            return None;
        }
        if let Some((conn, _)) = &b.on
            && conn.unique_name() == now.unique_name()
        {
            return Some(conn.clone());
        }
    }
    let signal = || zbus::MatchRule::builder().msg_type(zbus::message::Type::Signal);
    let rules = vec![
        // From the server only, as libnotify's proxy takes them: another program's signal of
        // the same name is about no notification of ours.
        signal().sender(IFACE).ok()?.interface(IFACE).ok()?.build(),
        signal()
            .sender(DBUS)
            .ok()?
            .interface(DBUS)
            .ok()?
            .member("NameOwnerChanged")
            .ok()?
            .arg(0, IFACE)
            .ok()?
            .build(),
    ];
    let (to, reply) = (buttons.clone(), back.clone());
    let sink = Box::new(move |m: &zbus::Message| {
        heard(&to, &reply, m);
        true
    });
    let (conn, end) = crate::dbus::subscribe_to_cancellable(Bus::Session, rules, sink)?;
    let old = {
        let mut b = lock(buttons);
        if b.ended {
            drop(b);
            end.cancel();
            return None;
        }
        // The ids of a connection that closed are not this one's server's to name.
        b.shown.clear();
        b.early.clear();
        b.on.replace((conn.clone(), end))
    };
    if let Some((_, end)) = old {
        end.cancel();
    }
    Some(conn)
}

/// Sends with buttons, begun one after another on a thread of their own: the first opens the
/// session bus and subscribes, which waits for the bus to answer. Nothing here waits for the
/// notification server.
static SHOWS: workpool::Lane = workpool::Lane::new();

/// How long the server has to answer `GetCapabilities` and `Notify` of a send with buttons:
/// the 25 s GDBus gives a call made with timeout `-1`, which is how libnotify makes both, so
/// `notify-send -A` gave up on each at this moment too.
pub const REPLY_LIMIT: std::time::Duration = std::time::Duration::from_secs(25);

/// Shows `n`, a send with buttons that returned `id`, and leaves it with `buttons` to wait for
/// a press or the close. Each `notify-send -A` the shell ran was a process of its own, so a
/// server that held one's call held no other: here the two calls a send makes are only
/// written, as Qt's `asyncCall` is, and their answers come back through the connection's
/// router, which also hands on the server's signals.
///
/// A server that answers neither call is given [`REPLY_LIMIT`] for each, as `notify-send`
/// gave it. No answer to `GetCapabilities`: the notification goes out without buttons
/// ("Displaying non-interactively"). No answer to `Notify`: "Failed to show notification",
/// and the send is forgotten ([`answered`]). Every later send is untouched meanwhile.
fn show(
    n: Note,
    id: u32,
    buttons: &Arc<Mutex<Buttons>>,
    back: &crate::impulse::Replies,
    unsent: Unsent,
) -> Option<()> {
    use crate::dbus::call_then_within;
    let conn = subscribed(buttons, back)?;
    let (on, buttons, back) = (conn.clone(), buttons.clone(), back.clone());
    let asked = move |caps| {
        // Its `Notify` is written by the time this returns.
        let _unsent = unsent;
        if !shows_buttons(caps) {
            n.post(&on);
            return;
        }
        let keys = keys(&n.actions);
        let args = n.args(&keys, &n.actions);
        let (to, reply) = (buttons.clone(), back.clone());
        let shown = move |answer| answered(&to, &reply, id, answer);
        lock(&buttons).ask(id, keys.len());
        let to = (IFACE, PATH, IFACE, "Notify");
        if let Err(e) = call_then_within(&on, REPLY_LIMIT, to.0, to.1, to.2, to.3, &args, shown) {
            answered(&buttons, &back, id, Err(e));
        }
    };
    let to = (IFACE, PATH, IFACE, "GetCapabilities");
    call_then_within(&conn, REPLY_LIMIT, to.0, to.1, to.2, to.3, &(), asked).ok()
}

/// Sends without buttons made before the session bus is open, put on it in the order sent by
/// a thread that can wait for the bus to answer. A send the system refuses that thread stays
/// queued, as one of [`SHOWS`] does, until the lane is given it (`workpool::run` tells when it
/// is asked for again; [`flush`] asks too): the sender, the shell's main thread, never opens
/// the bus itself.
static POSTS: workpool::Lane = workpool::Lane::new();

/// How many sends [`POSTS`] still holds, and the wakeup of [`flush`] when none are left and
/// none is [`Unsent`].
static QUEUED: (Mutex<usize>, Condvar) = (Mutex::new(0), Condvar::new());

/// How many sends with buttons are not on the bus yet (counted under [`QUEUED`]'s lock): in
/// [`SHOWS`], or waiting for the server to say whether it shows buttons.
static UNSENT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// One send with buttons from `Notify.send` until its `Notify` is written, or given up.
struct Unsent;

impl Unsent {
    fn new() -> Self {
        let _queued = QUEUED.0.lock().unwrap_or_else(|e| e.into_inner());
        UNSENT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Unsent
    }
}

impl Drop for Unsent {
    fn drop(&mut self) {
        let _queued = QUEUED.0.lock().unwrap_or_else(|e| e.into_inner());
        if UNSENT.fetch_sub(1, std::sync::atomic::Ordering::Relaxed) == 1 {
            QUEUED.1.notify_all();
        }
    }
}

/// How long the shell's exit waits for the services that do not answer, all together, before
/// the process ends without them: [`flush_until`] here and `bluez::flush_until` get one end,
/// this far from the start of the exit.
pub const FLUSH: std::time::Duration = std::time::Duration::from_secs(2);

/// Puts `n`, a notification without buttons, on the session bus: at once when the connection
/// is open ([`Note::post`] waits for nothing), else from [`POSTS`], since opening it waits for
/// the bus to answer and a bus that never does would hold the caller, the shell's main thread.
fn post(n: Note) {
    let mut queued = QUEUED.0.lock().unwrap_or_else(|e| e.into_inner());
    // An earlier send still queued goes out first.
    if *queued == 0
        && let Some(conn) = crate::dbus::connected(crate::dbus::Bus::Session)
    {
        drop(queued);
        n.post(&conn);
        return;
    }
    *queued += 1;
    drop(queued);
    POSTS.run(move || {
        crate::jobs::caught(|| crate::dbus::session().map_or(1, |conn| !n.post(&conn) as i32));
        let mut queued = QUEUED.0.lock().unwrap_or_else(|e| e.into_inner());
        *queued -= 1;
        if *queued == 0 {
            QUEUED.1.notify_all();
        }
    });
}

/// Waits until the sends made so far are on the bus, with buttons or without, for a process
/// about to end (`Qt.quit()` right after `Notify.send`): they outlive it as the `notify-send`
/// process they replace did. A bus or a server that does not answer is waited for until
/// `until`, then the process ends without them: `false`.
pub fn flush_until(until: std::time::Instant) -> bool {
    // A send [`POSTS`] or [`SHOWS`] holds because the system refused the lane its thread:
    // asked for again here, the last time anything would.
    workpool::ask_again();
    let unsent = |posts: &mut usize| *posts + UNSENT.load(std::sync::atomic::Ordering::Relaxed) > 0;
    let queued = QUEUED.0.lock().unwrap_or_else(|e| e.into_inner());
    let left = until.saturating_duration_since(std::time::Instant::now());
    let (_queued, limit) = QUEUED
        .1
        .wait_timeout_while(queued, left, unsent)
        .unwrap_or_else(|e| e.into_inner());
    !limit.timed_out()
}

/// [`flush_until`] with [`FLUSH`] from now.
pub fn flush() -> bool {
    flush_until(std::time::Instant::now() + FLUSH)
}

/// The hidden property of the singleton that holds its [`State`].
const STATE: &str = "\x01notify";

fn state_of(w: &mut World, item: usize) -> Option<Rc<State>> {
    let Some(Value::Obj(o)) = w.read(item, STATE) else {
        return None;
    };
    let n = o.borrow().native.clone()?;
    n.downcast::<State>().ok()
}

fn state(w: &mut World, item: usize) -> Rc<State> {
    if let Some(s) = state_of(w, item) {
        return s;
    }
    let s = Rc::new(State {
        last: Default::default(),
        buttons: Default::default(),
    });
    let o = crate::eval::new_obj();
    o.borrow_mut().native = Some(s.clone());
    w.declare(item, STATE, "var");
    w.write(item, STATE, Value::Obj(o));
    s
}

/// The notification `Notify.send(summary, body, options)` describes. `options` is optional:
/// `appName` (default `"Shell"`), `critical`, `transient`, `actions` (button labels).
fn note(args: &[Value]) -> Note {
    let text = |i: usize| args.get(i).map(|v| v.to_js_string()).unwrap_or_default();
    let mut n = Note::shell(text(0), text(1));
    let Some(Value::Obj(o)) = args.get(2) else {
        return n;
    };
    let o = o.borrow();
    let opt = |k: &str| {
        o.props
            .get(k)
            .filter(|v| !matches!(v, Value::Undefined | Value::Null))
    };
    if let Some(a) = opt("appName") {
        n.app = a.to_js_string();
    }
    n.critical = opt("critical").is_some_and(|v| v.truthy());
    n.transient = opt("transient").is_some_and(|v| v.truthy());
    if let Some(Value::Obj(a)) = opt("actions")
        && let Some(a) = &a.borrow().array
    {
        n.actions = a.iter().map(|l| l.to_js_string()).collect();
    }
    n
}

/// `Notify.send(summary, body, options)` on the singleton `item`: the id of this send, which
/// `actionInvoked(id, index)` names when one of its `actions` is pressed. It never waits for
/// the bus: without actions the notification is on the bus when this returns if the session
/// bus is open, and by [`flush`] otherwise ([`post`]); with them [`SHOWS`] shows it, and the
/// singleton's one subscription to the server's signals hears the button or the close.
pub fn send(w: &mut World, item: usize, args: &[Value]) -> Value {
    let n = note(args);
    let st = state(w, item);
    let id = st.last.get().wrapping_add(1);
    st.last.set(id);
    if crate::native::sandboxed() {
        // Commands are sandboxed (aotcheck, equivalence tests): the hook gets the command
        // line and nothing reaches the real bus.
        crate::native::hooked_spawn(&n.argv());
    } else if n.actions.is_empty() {
        post(n);
    } else {
        let (back, buttons) = (crate::impulse::replies(w), st.buttons.clone());
        let unsent = Unsent::new();
        SHOWS.run(move || {
            crate::jobs::caught(|| show(n, id, &buttons, &back, unsent).map_or(1, |()| 0));
        });
    }
    Value::Num(id as f64)
}

/// [`send`] as the method QML calls.
fn send_method(it: &mut Interp, this: &Value, args: &[Value]) -> R<Value> {
    let (Some(w), Value::Item(i)) = (crate::runtime::world_of(it), this) else {
        return Ok(Value::Undefined);
    };
    Ok(send(&mut w.borrow_mut(), *i, args))
}

/// `Notify.forgetImage(source)`: deletes the copy our notification server keeps on disk for
/// an `image` source nothing shows any more (a history entry of an earlier run, discarded).
fn forget_image(_: &mut Interp, _: &Value, args: &[Value]) -> R<Value> {
    if let Some(Value::Str(src)) = args.first() {
        crate::notifications::forget_saved(src);
    }
    Ok(Value::Undefined)
}

/// Emits `notificationReplaced(id)`: the sender of notification `id` of our notification
/// server sent it again (`replaces_id`), which Quickshell's `NotificationServer` only shows
/// as property changes of the same `Notification`.
pub(crate) fn replaced(w: &mut World, id: u32) {
    if let Some(&item) = w.singletons.get("Notify") {
        crate::native::emit(
            w,
            item,
            "notificationReplaced",
            &["id"],
            vec![Value::Num(id as f64)],
        );
    }
}

/// Emits `actionInvoked(id, index)`: button `index` of send `id` was pressed.
fn pressed(w: &mut World, id: u32, index: u32) {
    if let Some(&item) = w.singletons.get("Notify") {
        crate::native::emit(
            w,
            item,
            "actionInvoked",
            &["id", "index"],
            vec![Value::Num(id as f64), Value::Num(index as f64)],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kept(b: &Buttons) -> Vec<(u32, Option<u32>, bool)> {
        b.early
            .iter()
            .map(|e| (e.server, e.pressed, e.closed))
            .collect()
    }

    fn press(key: &str) -> Said {
        Said::Pressed(key.into())
    }

    /// `early` holds only what may be about a send whose `Notify` is unanswered, one entry
    /// for an id, and nothing of it outlives the `Notify` calls written before it.
    #[test]
    fn early_keeps_only_what_an_unanswered_notify_may_bring() {
        let mut b = Buttons::default();
        // Every `Notify` is answered: another application's notification, whatever is said.
        b.keep(4, &press("0"));
        b.keep(4, &Said::Closed);
        assert_eq!(kept(&b), []);

        // Send 1, with two buttons, is unanswered.
        b.ask(1, 2);
        // Keys the send was not given: "default", a third button, "1" written another way.
        b.keep(5, &press("default"));
        b.keep(5, &press("2"));
        b.keep(5, &press("01"));
        assert_eq!(kept(&b), []);
        // One entry for an id: its first press, and its close, after which nothing counts.
        b.keep(6, &press("1"));
        b.keep(6, &press("0"));
        b.keep(6, &Said::Closed);
        b.keep(6, &press("0"));
        b.keep(6, &Said::Closed);
        assert_eq!(kept(&b), [(6, Some(1), true)]);
        // A key of somebody else's about an id kept so far: the id is not ours.
        b.keep(7, &Said::Closed);
        b.keep(7, &press("default"));
        assert_eq!(kept(&b), [(6, Some(1), true)]);
        b.keep(8, &Said::Closed);
        assert_eq!(kept(&b), [(6, Some(1), true), (8, None, true)]);

        // Send 2, with one button, is written after all that: the server said nothing of
        // those ids about it. Of what comes now, the second button can only be send 1's.
        b.ask(2, 1);
        b.keep(9, &press("1"));
        b.keep(10, &press("0"));
        assert_eq!(b.early.len(), 4);

        // The answer to send 1 brings 6: what was said of 6 is handed over, and what only
        // send 1 could have had (8, 9) is dropped. 10 may still be send 2's.
        let early = b.answer(1, Some(6)).expect("what was said of 6");
        assert_eq!((early.pressed, early.closed), (Some(1), true));
        assert_eq!(kept(&b), [(10, Some(0), false)]);

        // Send 2 gets no answer (the reply limit): nothing is left.
        assert!(b.answer(2, None).is_none());
        assert_eq!(kept(&b), []);
        assert!(b.unanswered.is_empty());
    }

    /// An id spoken of before a send's `Notify` was written is not that send's, even when
    /// the answer brings the same number.
    #[test]
    fn early_is_not_about_a_notify_written_later() {
        let mut b = Buttons::default();
        b.ask(1, 1);
        b.keep(6, &press("0"));
        b.ask(2, 1);
        assert!(b.answer(2, Some(6)).is_none());
        assert_eq!(kept(&b), []);
    }
}
