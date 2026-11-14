//! What libnotify 0.8's `notify-send` puts on the session bus, for the native ports that
//! replace its fixed `notify-send SUMMARY BODY ...` calls, and `g_strcompress`. Unlike
//! `notify-send`, the body goes out as given: its unescaping is for text typed in a shell, and
//! turned a `\t` in a file name or message into a tab.
//! libnotify's two `GetServerInformation` round trips before `Notify` are left out: their
//! answers never change the call that follows.

use std::collections::HashMap;
use zbus::zvariant::Value;

pub const PATH: &str = "/org/freedesktop/Notifications";
pub const IFACE: &str = "org.freedesktop.Notifications";

/// The hints notify-send always sends: `urgency` (`-u`), `transient` for
/// `--hint=int:transient:1`, and libnotify 0.8's `sender-pid`.
pub fn hints(critical: bool, transient: bool) -> HashMap<&'static str, Value<'static>> {
    let mut h = HashMap::new();
    h.insert("urgency", Value::U8(if critical { 2 } else { 1 }));
    if transient {
        h.insert("transient", Value::I32(1));
    }
    h.insert("sender-pid", Value::I64(std::process::id().into()));
    h
}

/// `notify-send SUMMARY BODY -a APP` on `conn`, the body verbatim: the id the server returned. libnotify's two
/// `GetServerInformation` round trips are left out: only image hints, never sent here, use them.
pub fn notify(
    conn: &zbus::blocking::Connection,
    summary: &str,
    body: &str,
    app: &str,
) -> Option<u32> {
    conn.call_method(
        Some(IFACE),
        PATH,
        Some(IFACE),
        "Notify",
        &(
            app,
            0u32,
            "",
            summary,
            body,
            Vec::<&str>::new(),
            hints(false, false),
            -1i32,
        ),
    )
    .ok()?
    .body()
    .deserialize::<(u32,)>()
    .ok()
    .map(|(id,)| id)
}

/// [`notify`] on the shared session-bus connection (`crate::dbus::session`), for callers
/// without their own; `None` when there is no bus or the server refused.
pub fn notify_session(summary: &str, body: &str, app: &str) -> Option<u32> {
    notify(&crate::dbus::session()?, summary, body, app)
}

/// `g_strcompress`: C escapes (`\n`, `\t`, octal, `\\`, ...) undone, as notify-send does to
/// its body and polkit-agent-helper's peer to its `g_strescape`d lines.
pub fn strcompress(s: &str) -> String {
    let b = s.as_bytes();
    let mut o = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'\\' || i + 1 >= b.len() {
            o.push(b[i]);
            i += 1;
            continue;
        }
        i += 1;
        match b[i] {
            b'0'..=b'7' => {
                let mut v = 0u32;
                let mut n = 0;
                while n < 3 && i < b.len() && (b'0'..=b'7').contains(&b[i]) {
                    v = v * 8 + (b[i] - b'0') as u32;
                    i += 1;
                    n += 1;
                }
                o.push(v as u8);
                continue;
            }
            b'b' => o.push(8),
            b'f' => o.push(12),
            b'n' => o.push(b'\n'),
            b'r' => o.push(b'\r'),
            b't' => o.push(b'\t'),
            b'v' => o.push(11),
            c => o.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&o).into_owned()
}

#[cfg(test)]
mod tests {
    #[test]
    fn strcompress_escapes() {
        assert_eq!(super::strcompress("a\\nb\\t\\101\\\\\\q\\"), "a\nb\tA\\q\\");
    }
}
