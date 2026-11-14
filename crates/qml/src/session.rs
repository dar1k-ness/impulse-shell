//! `Impulse/SessionControl`: the actions the original's `Session.qml` and Battery.qml's automatic
//! suspend call. logind over the system bus with the calls `systemctl`/`loginctl` make,
//! Hyprland's `exit` over its request socket, and `kill(2)` for `terminate(pid)`.

use crate::eval::{Interp, R};
use crate::value::Value;
use zbus::blocking::Connection;
use zbus::proxy::{CacheProperties, MethodFlags};

/// logind calls, one at a time in the order made. Each may wait as long as polkit's
/// authentication dialog stays open (interactive authorization), and a logind that never
/// answers holds only this lane, never the shared workers of `workpool::run`.
static LOGIND: workpool::Lane = workpool::Lane::new();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    PowerOff,
    Reboot,
    RebootToFirmware,
    Suspend,
    Hibernate,
    Lock,
}

bits::bit_flags! {
const REBOOT_VIA_KEXEC: u64 = 1 << 1;
const SOFT_REBOOT_IF_NEXTROOT_SET_UP: u64 = 1 << 3;
}

/// systemd's `getenv_bool(name) > 0` (`parse_boolean`).
fn env_true(name: &str) -> bool {
    std::env::var(name).is_ok_and(|v| {
        matches!(
            v.to_ascii_lowercase().as_str(),
            "1" | "yes" | "y" | "true" | "t" | "on"
        )
    })
}

fn nextroot_mounted() -> bool {
    std::fs::read_to_string("/proc/self/mountinfo").is_ok_and(|m| {
        m.lines()
            .any(|l| l.split(' ').nth(4) == Some("/run/nextroot"))
    })
}

/// The `t flags` argument `systemctl` passes to `<Action>WithFlags` (no tty, so inhibitors
/// are neither checked nor skipped).
pub fn flags(a: Action) -> u64 {
    let mut f = 0;
    if matches!(a, Action::Reboot | Action::RebootToFirmware) {
        if !env_true("SYSTEMCTL_SKIP_AUTO_KEXEC") {
            f |= REBOOT_VIA_KEXEC;
        }
        if !env_true("SYSTEMCTL_SKIP_AUTO_SOFT_REBOOT") && nextroot_mounted() {
            f |= SOFT_REBOOT_IF_NEXTROOT_SET_UP;
        }
    }
    f
}

fn proxy<'a>(
    conn: &'a Connection,
    path: &'a str,
    iface: &'a str,
) -> zbus::Result<zbus::blocking::Proxy<'a>> {
    zbus::blocking::proxy::Builder::new(conn)
        .destination("org.freedesktop.login1")?
        .path(path)?
        .interface(iface)?
        .cache_properties(CacheProperties::No)
        .build()
}

macro_rules! call {
    ($p:expr, $m:expr, $b:expr) => {
        call!($p, $m, $b, true)
    };
    ($p:expr, $m:expr, $b:expr, $ask:expr) => {{
        let f = if $ask {
            MethodFlags::AllowInteractiveAuth.into()
        } else {
            Default::default()
        };
        $p.call_with_flags::<_, _, ()>($m, f, $b).map(|_| ())
    }};
}

fn is_error(e: &zbus::Error, name: &str) -> bool {
    matches!(e, zbus::Error::MethodError(n, _, _) if n.as_str() == name)
}

/// What `systemctl poweroff|reboot --firmware-setup|suspend|hibernate` (logind_reboot in
/// systemctl-logind.c), `reboot` (systemctl's halt mode: no interactive polkit) and
/// `loginctl lock-session` send for `a`.
pub fn logind(conn: &Connection, a: Action) -> zbus::Result<()> {
    if a == Action::Lock {
        let p = proxy(
            conn,
            "/org/freedesktop/login1/session/auto",
            "org.freedesktop.login1.Session",
        )?;
        return call!(p, "Lock", &());
    }
    let p = proxy(
        conn,
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
    )?;
    if a == Action::RebootToFirmware {
        call!(p, "SetRebootToFirmwareSetup", &(true,))?;
    }
    let ask = a != Action::Reboot;
    let _ = call!(p, "SetWallMessage", &("", true), ask);
    let m = match a {
        Action::PowerOff => "PowerOff",
        Action::Reboot | Action::RebootToFirmware => "Reboot",
        Action::Suspend => "Suspend",
        Action::Hibernate => "Hibernate",
        Action::Lock => unreachable!(),
    };
    let wf = format!("{m}WithFlags");
    let mut f = flags(a);
    let mut r = call!(p, wf.as_str(), &(f,), ask);
    if let Err(e) = &r
        && f & SOFT_REBOOT_IF_NEXTROOT_SET_UP != 0
        && is_error(e, "org.freedesktop.DBus.Error.InvalidArgs")
    {
        f &= !SOFT_REBOOT_IF_NEXTROOT_SET_UP;
        r = call!(p, wf.as_str(), &(f,), ask);
    }
    match r {
        Err(e) if is_error(&e, "org.freedesktop.DBus.Error.UnknownMethod") => {
            call!(p, m, &(ask,), ask)
        }
        r => r,
    }
}

/// `kill PID` (util-linux: SIGTERM; a non-number fails). Non-positive ids, which would
/// signal a process group or everything, are refused.
pub fn kill(pid: &str) -> bool {
    match pid.trim().parse::<i32>() {
        // SAFETY: kill(2) takes no pointers.
        Ok(p) if p > 0 => unsafe { libc::kill(p, libc::SIGTERM) == 0 },
        _ => false,
    }
}

/// Hyprland's `exit` dispatcher over `.socket.sock` (Lua config first, then hyprlang).
pub fn exit_hyprland(dir: &std::path::Path) -> bool {
    let ok = |c: &str| crate::hypr::request(dir, c).is_some_and(|r| r.trim() == "ok");
    ok("eval return hl.dispatch(hl.dsp.exit())") || ok("dispatch exit")
}

/// The methods of the `SessionControl` singleton (`Impulse/session_control.qmltypes`).
const METHODS: &[(&str, crate::eval::NativeFn)] = &[
    ("lock", |it, _, _| act(it, "lock", vec![])),
    ("suspend", |it, _, _| act(it, "suspend", vec![])),
    ("hibernate", |it, _, _| act(it, "hibernate", vec![])),
    ("poweroff", |it, _, _| act(it, "poweroff", vec![])),
    ("reboot", |it, _, _| act(it, "reboot", vec![])),
    ("rebootToFirmware", |it, _, _| {
        act(it, "rebootToFirmware", vec![])
    }),
    ("logout", |it, _, _| act(it, "logout", vec![])),
    ("terminate", |it, _, a| {
        act(it, "kill", vec![a.first().cloned().unwrap_or_default()])
    }),
];

pub static TYPE: crate::impulse::Type = crate::impulse::Type {
    name: "SessionControl",
    methods: METHODS,
    init: None,
};

fn act(it: &mut Interp, m: &str, args: Vec<Value>) -> R<Value> {
    if let Some(w) = crate::runtime::world_of(it) {
        w.borrow_mut()
            .effects
            .push(crate::svc::Effect::Session(crate::svc::Call::new(m, args)));
    }
    Ok(Value::Undefined)
}

/// Runs a `session.*` effect: kills inline, bus/socket work on its own short-lived thread
/// (the original's detached process). A [`crate::native::SpawnHook`] gets the call instead.
pub fn run_effect(c: &crate::svc::Call) {
    let (m, a) = (c.method.as_str(), &c.args);
    let (method, args) = match m {
        "kill" => (
            "terminate",
            a.first().map(|v| v.to_js_string()).into_iter().collect(),
        ),
        m => (m, vec![]),
    };
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    if crate::native::hooked_call(TYPE.name, method, &args).is_some() {
        return;
    }
    let action = match m {
        "kill" => {
            kill(args.first().copied().unwrap_or_default());
            return;
        }
        "logout" => None,
        "lock" => Some(Action::Lock),
        "suspend" => Some(Action::Suspend),
        "hibernate" => Some(Action::Hibernate),
        "poweroff" => Some(Action::PowerOff),
        "reboot" => Some(Action::Reboot),
        "rebootToFirmware" => Some(Action::RebootToFirmware),
        _ => return,
    };
    match action {
        None => crate::hypr::submit(|| {
            if !crate::hypr::socket_dir().is_some_and(|d| exit_hyprland(&d)) {
                elog::eprintln!("session: hyprland exit failed");
            }
        }),
        Some(a) => LOGIND.run(move || {
            if let Err(e) = crate::dbus::system()
                .ok_or_else(|| zbus::Error::Failure("no system bus".into()))
                .and_then(|c| logind(&c, a))
            {
                elog::eprintln!("session: logind {a:?}: {e}");
            }
        }),
    }
}
