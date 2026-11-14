//! `pywal = True`: what kde-material-you-colors handed to python-pywal 3.3.0
//! (`sequences.send`, `export.every`, `reload.env`), done natively. Same files in
//! `~/.cache/wal` (every template pywal ships, then the user's `~/.config/wal/templates`), the
//! same escape sequences on every open terminal and the same reloads. Differences: a file is
//! only rewritten when its text changes; terminals are written without blocking (a terminal
//! that stopped reading hung the original, which needed a timeout); a user template pywal's
//! `str.format` cannot fill is skipped instead of aborting the rest of the export and the
//! reloads; i3/sway reload over their IPC sockets and polybar with a signal instead of
//! `i3-msg`/`swaymsg`/`pkill`, and only when that program runs; `xrdb` only with an X display
//! and a deadline.

use super::apply::{Bus, Dirs, Report};
use super::color::{self, Rgb};
use super::kconfig::{deadline, write_locked};
use super::theme::Theme;
use std::os::fd::{AsRawFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// pywal 3.3.0 `pywal/templates/`, rendered under the same names.
const TEMPLATES: [(&str, &str); 23] = [
    ("colors", include_str!("pywal/colors")),
    ("colors-kitty.conf", include_str!("pywal/colors-kitty.conf")),
    (
        "colors-konsole.colorscheme",
        include_str!("pywal/colors-konsole.colorscheme"),
    ),
    ("colors-oomox", include_str!("pywal/colors-oomox")),
    ("colors-putty.reg", include_str!("pywal/colors-putty.reg")),
    (
        "colors-rofi-dark.rasi",
        include_str!("pywal/colors-rofi-dark.rasi"),
    ),
    (
        "colors-rofi-light.rasi",
        include_str!("pywal/colors-rofi-light.rasi"),
    ),
    (
        "colors-speedcrunch.json",
        include_str!("pywal/colors-speedcrunch.json"),
    ),
    ("colors-sway", include_str!("pywal/colors-sway")),
    ("colors-tty.sh", include_str!("pywal/colors-tty.sh")),
    (
        "colors-wal-dmenu.h",
        include_str!("pywal/colors-wal-dmenu.h"),
    ),
    ("colors-wal-dwm.h", include_str!("pywal/colors-wal-dwm.h")),
    ("colors-wal-st.h", include_str!("pywal/colors-wal-st.h")),
    (
        "colors-wal-tabbed.h",
        include_str!("pywal/colors-wal-tabbed.h"),
    ),
    ("colors-wal.vim", include_str!("pywal/colors-wal.vim")),
    ("colors-waybar.css", include_str!("pywal/colors-waybar.css")),
    ("colors.Xresources", include_str!("pywal/colors.Xresources")),
    ("colors.css", include_str!("pywal/colors.css")),
    ("colors.hs", include_str!("pywal/colors.hs")),
    ("colors.json", include_str!("pywal/colors.json")),
    ("colors.scss", include_str!("pywal/colors.scss")),
    ("colors.sh", include_str!("pywal/colors.sh")),
    ("colors.yml", include_str!("pywal/colors.yml")),
];

const ALPHA: &str = "100";

/// The pywal colour dict (`get_wal_light_scheme` / `get_wal_dark_scheme`), flattened the way
/// `export.flatten_colors` hands it to templates.
pub struct Palette {
    vars: Vec<(String, String)>,
}

impl Palette {
    /// `dark`: the pywal mode (`pywal_light`, else the Plasma mode). `wallpaper`: the colour
    /// source as given (`--color`), which the original stores verbatim.
    pub fn new(theme: &Theme, dark: bool, wallpaper: &str) -> Palette {
        let k = theme.konsole(dark);
        let cursor = theme
            .material
            .tokens(true)
            .get("onSurface")
            .unwrap_or(k.foreground);
        let mut vars: Vec<(String, String)> = vec![
            ("wallpaper".into(), wallpaper.into()),
            ("alpha".into(), ALPHA.into()),
        ];
        let mut c = |n: &str, v: Rgb| vars.push((n.into(), color::hex(v)));
        c("background", k.background);
        c("backgroundIntense", k.background_intense);
        c("backgroundFaint", k.background_faint);
        c("foreground", k.foreground);
        c("foregroundIntense", k.foreground_intense);
        c("foregroundFaint", k.foreground_faint);
        c("cursor", cursor);
        for (i, v) in k
            .normal
            .iter()
            .chain(&k.intense)
            .chain(&k.faint)
            .enumerate()
        {
            c(&format!("color{i}"), *v);
        }
        Palette { vars }
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.vars
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    /// `sequences.create_sequences(colors, vte_fix=False)` on Linux.
    pub fn sequences(&self) -> String {
        let g = |n: &str| self.get(n).unwrap_or_default();
        let mut s = String::with_capacity(1024);
        for i in 0..16 {
            s += &format!("\x1b]4;{i};{}\x1b\\", g(&format!("color{i}")));
        }
        let (fg, bg, cur) = (g("foreground"), g("background"), g("cursor"));
        for (i, c) in [(10, fg), (11, bg), (12, cur), (13, fg), (17, fg), (19, bg)] {
            s += &format!("\x1b]{i};{c}\x1b\\");
        }
        s += &format!("\x1b]4;232;{bg}\x1b\\\x1b]4;256;{fg}\x1b\\");
        s += &format!("\x1b]708;{bg}\x1b\\");
        s
    }

    /// `str.format(**colors)` with pywal's `util.Color` attributes; `None` where Python raised.
    pub fn render(&self, tpl: &str) -> Option<String> {
        let mut out = String::with_capacity(tpl.len() * 2);
        let mut it = tpl.char_indices().peekable();
        while let Some((i, ch)) = it.next() {
            match ch {
                '{' if it.peek().map(|p| p.1) == Some('{') => {
                    it.next();
                    out.push('{');
                }
                '}' if it.peek().map(|p| p.1) == Some('}') => {
                    it.next();
                    out.push('}');
                }
                '}' => return None,
                '{' => {
                    let end = tpl[i..].find('}')? + i;
                    let field = &tpl[i + 1..end];
                    while it.peek().is_some_and(|p| p.0 <= end) {
                        it.next();
                    }
                    let (name, attr) = field.split_once('.').unwrap_or((field, ""));
                    out += &attribute(self.get(name)?, attr)?;
                }
                c => out.push(c),
            }
        }
        Some(out)
    }
}

fn rgb_of(v: &str) -> Option<Rgb> {
    let h = v.trim_matches('#');
    (h.len() == 6)
        .then(|| color::parse(&format!("#{h}")))
        .flatten()
}

/// `util.Color(value).<attr>`.
fn attribute(v: &str, attr: &str) -> Option<String> {
    let octal = || {
        u64::from_str_radix(v.get(1..)?, 16)
            .ok()
            .map(|n| format!("{n:o}"))
    };
    Some(match attr {
        "" | "hex_color" => v.to_string(),
        "strip" => v.get(1..).unwrap_or_default().to_string(),
        "rgb" => color::triple(rgb_of(v)?),
        "rgba" => format!(
            "rgba({},{:?})",
            color::triple(rgb_of(v)?),
            ALPHA.parse::<f64>().ok()? / 100.0
        ),
        "xrgba" => {
            let h = v.to_lowercase();
            let h = h.trim_matches('#');
            if h.chars().count() != 6 {
                return None;
            }
            format!("{}/{}/{}/ff", &h[..2], &h[2..4], &h[4..])
        }
        "alpha" => format!("[{ALPHA}]{v}"),
        "alpha_num" => ALPHA.to_string(),
        "octal" => format!("#{}", octal()?),
        "octal_strip" => octal()?,
        _ => return None,
    })
}

/// pywal's cache dir (`$PYWAL_CACHE_DIR`, else `<cache>/wal`).
pub fn cache_dir(dirs: &Dirs) -> PathBuf {
    match std::env::var_os("PYWAL_CACHE_DIR") {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _ => dirs.cache.join("wal"),
    }
}

/// `pywal_utils.apply_schemes`: sequences to every terminal, every template into the cache,
/// then the reloads.
pub fn apply(
    dirs: &Dirs,
    theme: &Theme,
    dark: bool,
    wallpaper: &str,
    bus: &dyn Bus,
    r: &mut Report,
) {
    let pal = Palette::new(theme, dark, wallpaper);
    let cache = cache_dir(dirs);
    let mut put = |name: &str, data: &str| {
        let p = cache.join(name);
        if write_locked(&p, data.as_bytes()).unwrap_or(false) {
            r.written.push(p);
        }
    };

    let seq = pal.sequences();
    for tty in bus.ttys() {
        bus.write_tty(&tty, seq.as_bytes());
    }
    put("sequences", &seq);

    let mut rendered: Vec<(String, String)> = Vec::with_capacity(TEMPLATES.len());
    for (name, tpl) in TEMPLATES {
        if let Some(t) = pal.render(tpl) {
            rendered.push((name.to_string(), t));
        }
    }
    let user = dirs.config.join("wal/templates");
    let _ = std::fs::create_dir_all(&user);
    let mut names: Vec<(String, PathBuf)> = std::fs::read_dir(&user)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| !t.is_dir()))
        .filter_map(|e| Some((e.file_name().into_string().ok()?, e.path())))
        .filter(|(n, _)| n != ".DS_Store")
        .collect();
    names.sort();
    for (name, path) in names {
        let Some(t) = std::fs::read_to_string(&path)
            .ok()
            .map(|t| t.replace("\r\n", "\n").replace('\r', "\n"))
        else {
            continue;
        };
        match pal.render(&t) {
            Some(out) => match rendered.iter_mut().find(|(n, _)| *n == name) {
                Some(e) => e.1 = out,
                None => rendered.push((name, out)),
            },
            None => elog::eprintln!(
                "pywal: template {} has fields pywal cannot fill, skipped",
                path.display()
            ),
        }
    }
    for (name, text) in &rendered {
        put(name, text);
    }
    bus.pywal_reload(&cache);
}

/// `reload.env()`: xrdb, i3, kitty, sway, polybar, then the Linux console.
pub fn reload_env(cache: &Path) {
    let procs = crate::live::processes();
    let running = |name: &str| {
        procs.iter().any(|(_, comm, argv)| {
            comm == name || argv.first().map(|a| a.rsplit('/').next().unwrap_or(a)) == Some(name)
        })
    };
    let pids = |name: &str| -> Vec<i32> {
        procs
            .iter()
            .filter(|(_, comm, argv)| {
                comm == name
                    || argv.first().map(|a| a.rsplit('/').next().unwrap_or(a)) == Some(name)
            })
            .map(|p| p.0)
            .collect()
    };

    if std::env::var_os("DISPLAY").is_some_and(|d| !d.is_empty()) && which("xrdb") {
        xrdb_merge(&cache.join("colors.Xresources"));
    }
    if running("i3") {
        let sock = std::env::var_os("I3SOCK").map(PathBuf::from).or_else(|| {
            let rt = runtime_dir()?;
            pids("i3")
                .into_iter()
                .map(|p| rt.join(format!("i3/ipc-socket.{p}")))
                .find(|s| s.exists())
        });
        if !sock.is_some_and(|s| ipc_command(&s, "reload")) && which("i3-msg") {
            spawn(&["i3-msg", "reload"]);
        }
    }
    if running("kitty") && which("kitty") {
        spawn(&["kitty", "@", "set-colors", "--all"]);
    }
    if running("sway") {
        // SAFETY: getuid(2) has no failure mode.
        let uid = unsafe { libc::getuid() };
        let sock = std::env::var_os("SWAYSOCK").map(PathBuf::from).or_else(|| {
            let rt = runtime_dir()?;
            pids("sway")
                .into_iter()
                .map(|p| rt.join(format!("sway-ipc.{uid}.{p}.sock")))
                .find(|s| s.exists())
        });
        if !sock.is_some_and(|s| ipc_command(&s, "reload")) && which("swaymsg") {
            spawn(&["swaymsg", "reload"]);
        }
    }
    signal_polybar(&procs);
    if std::env::var("TERM").as_deref() == Ok("linux") {
        tty_console(&cache.join("colors-tty.sh"));
    }
}

/// `xrdb -merge -quiet FILE`, given up on after [`xrdb_deadline`].
pub(crate) fn xrdb_merge(file: &Path) {
    if let Ok(d) = xrdb_deadline() {
        run_until(
            workpool::userenv::command("xrdb")
                .arg("-merge")
                .arg("-quiet")
                .arg(file),
            &d,
        );
    }
}

/// pywal's `pkill -USR1 polybar`: `SIGUSR1` to every process of the scan `procs` with
/// `polybar` in its name, if the pid is still that process ([`crate::live::signal_scanned`]).
#[doc(hidden)]
pub fn signal_polybar(procs: &[(i32, String, Vec<String>)]) {
    crate::live::signal_scanned(procs, libc::SIGUSR1, &|comm, _| comm.contains("polybar"));
}

fn runtime_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

fn which(cmd: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::env::var_os("PATH").is_some_and(|p| {
        std::env::split_paths(&p).any(|d| {
            std::fs::metadata(d.join(cmd))
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
    })
}

/// Fire and forget, reaped on exit (no zombie per theme switch); the child's pid.
pub(crate) fn spawn(argv: &[&str]) -> Option<u32> {
    spawn_then(argv, |_| {})
}

/// [`spawn`], then `f` with its exit status on the reaper thread once it is reaped.
fn spawn_then(
    argv: &[&str],
    f: impl FnOnce(Option<std::process::ExitStatus>) + Send + 'static,
) -> Option<u32> {
    use std::process::Stdio;
    let ch = workpool::userenv::command(argv[0])
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let pid = ch.id();
    workpool::reap_then(ch, f);
    Some(pid)
}

/// How long `xrdb -merge` may run: it answers at once, or hangs on an X server that does not
/// answer, and the reloads after it must not wait for that.
fn xrdb_deadline() -> std::io::Result<OwnedFd> {
    deadline(Duration::from_secs(2))
}

/// Runs `cmd` until it exits or `deadline` becomes readable, then kills it; its exit status.
fn run_until(
    cmd: &mut std::process::Command,
    deadline: &OwnedFd,
) -> Option<std::process::ExitStatus> {
    use std::process::Stdio;
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    if workpool::pidfd_open(child.id() as i32).is_ok_and(|fd| !exits_before(&fd, deadline)) {
        let _ = child.kill();
    }
    child.wait().ok()
}

/// Sleeps on the pidfd and `deadline` until either is readable; whether the process exited.
fn exits_before(pidfd: &OwnedFd, deadline: &OwnedFd) -> bool {
    loop {
        let mut p = [pidfd, deadline].map(|f| libc::pollfd {
            fd: f.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        });
        // SAFETY: two valid pollfds for the duration of the call.
        let n = unsafe { libc::poll(p.as_mut_ptr(), 2, -1) };
        if n > 0 {
            return p[0].revents != 0;
        }
        if std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
            return false;
        }
    }
}

/// One i3/sway IPC `RUN_COMMAND`; whether the socket took it.
pub fn ipc_command(sock: &Path, command: &str) -> bool {
    use std::io::{Read, Write};
    let Ok(mut s) = std::os::unix::net::UnixStream::connect(sock) else {
        return false;
    };
    let _ = s.set_write_timeout(Some(Duration::from_secs(1)));
    let _ = s.set_read_timeout(Some(Duration::from_secs(1)));
    let mut msg = b"i3-ipc".to_vec();
    msg.extend((command.len() as u32).to_ne_bytes());
    msg.extend(0u32.to_ne_bytes());
    msg.extend(command.as_bytes());
    if s.write_all(&msg).is_err() {
        return false;
    }
    let mut head = [0u8; 14];
    s.read_exact(&mut head).is_ok() && &head[..6] == b"i3-ipc"
}

/// `sh colors-tty.sh` on a Linux console: the stock script's bytes written here, a user's own
/// script still run by `sh`.
pub(crate) fn tty_console(script: &Path) {
    use std::io::Write;
    let text = std::fs::read_to_string(script).unwrap_or_default();
    let body = text
        .split_once("printf '%b' '")
        .and_then(|(_, b)| b.rsplit_once('\''))
        .map(|(b, _)| b);
    match body {
        Some(b) if !b.contains('\'') && !b.contains("\\0") => {
            let bytes = b.replace("\\e", "\x1b");
            let mut out = std::io::stdout();
            let _ = out.write_all(bytes.as_bytes());
            let _ = out.flush();
        }
        _ => {
            let _ = workpool::userenv::command("sh").arg(script).status();
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn reload_children_leave_no_zombie() {
        let (tx, rx) = std::sync::mpsc::channel();
        let pid = spawn_then(&["true"], move |st| tx.send(st).unwrap()).unwrap();
        assert!(rx.recv().unwrap().is_some_and(|st| st.success()));
        assert!(
            !Path::new(&format!("/proc/{pid}")).exists(),
            "pid {pid} still in /proc (zombie)"
        );
    }

    #[test]
    fn run_deadline_kills_a_hung_child_on_time() {
        use std::os::unix::process::ExitStatusExt;
        let d = deadline(Duration::from_millis(200)).unwrap();
        // Returning at all proves the kill: the child never exits on its own.
        let st = run_until(std::process::Command::new("sleep").arg("infinity"), &d).unwrap();
        assert_eq!(st.signal(), Some(libc::SIGKILL), "{st:?}");
        assert!(fired(&d), "killed before the deadline");
    }

    #[test]
    fn xrdb_is_given_two_seconds() {
        let d = xrdb_deadline().unwrap();
        let mut left = libc::itimerspec {
            it_interval: libc::timespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
            it_value: libc::timespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
        };
        // SAFETY: timerfd_gettime(2) on a live timerfd into a local itimerspec.
        let read = unsafe { libc::timerfd_gettime(d.as_raw_fd(), &mut left) };
        assert_eq!(read, 0);
        // Armed for 2 s: what is left of it is read back, nothing waits for it.
        let left = Duration::new(left.it_value.tv_sec as u64, left.it_value.tv_nsec as u32);
        assert!(
            left > Duration::from_secs(1) && left <= Duration::from_secs(2),
            "{left:?}"
        );
        assert!(!fired(&d));
    }

    /// i3/sway answer a command at once; one that is stopped or hung answers nothing, and no
    /// event says so. The reload then gives up after its one-second socket limits instead of
    /// holding the theme switch.
    #[test]
    fn ipc_command_takes_the_answer_or_gives_up_on_a_silent_socket() {
        use std::io::{Read, Write};
        let dir = std::env::temp_dir().join(format!("ri-pywal-ipc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let sock = dir.join("ipc");
        let l = std::os::unix::net::UnixListener::bind(&sock).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            // The first client is answered; the second is read and held open, unanswered,
            // until the test is done with it.
            let (mut answered, _) = l.accept().unwrap();
            let mut head = [0u8; 14];
            answered.read_exact(&mut head).unwrap();
            let len = u32::from_ne_bytes(head[6..10].try_into().unwrap()) as usize;
            let mut body = vec![0; len];
            answered.read_exact(&mut body).unwrap();
            let mut reply = b"i3-ipc".to_vec();
            reply.extend(2u32.to_ne_bytes());
            reply.extend(0u32.to_ne_bytes());
            reply.extend(b"[]");
            answered.write_all(&reply).unwrap();
            let (held, _) = l.accept().unwrap();
            let _ = rx.recv();
            drop(held);
            body
        });
        assert!(ipc_command(&sock, "reload"));
        assert!(!ipc_command(&sock, "reload"), "a silent socket took it");
        tx.send(()).unwrap();
        assert_eq!(server.join().unwrap(), b"reload");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn run_deadline_returns_as_soon_as_the_child_exits() {
        // A deadline that never fires while `_alive` lives: returning means the child exited.
        let (d, _alive) = std::io::pipe().unwrap();
        let st = run_until(&mut std::process::Command::new("true"), &d.into()).unwrap();
        assert!(st.success(), "{st:?}");
    }

    use super::super::kconfig::fired;
    use super::*;

    fn pal() -> Palette {
        Palette {
            vars: vec![
                ("wallpaper".into(), "#5E7A91".into()),
                ("alpha".into(), "100".into()),
                ("color0".into(), "#1a2B3c".into()),
            ],
        }
    }

    #[test]
    fn format_like_python() {
        let p = pal();
        assert_eq!(p.render("{{x}} {color0}").unwrap(), "{x} #1a2B3c");
        assert_eq!(
            p.render("{color0.rgb}|{color0.xrgba}|{color0.strip}")
                .unwrap(),
            "26,43,60|1a/2b/3c/ff|1a2B3c"
        );
        assert_eq!(
            p.render("{color0.rgba} {color0.alpha}").unwrap(),
            "rgba(26,43,60,1.0) [100]#1a2B3c"
        );
        assert_eq!(
            p.render("{color0.octal} {color0.octal_strip}").unwrap(),
            "#6425474 6425474"
        );
        assert_eq!(p.render("{alpha} {wallpaper}").unwrap(), "100 #5E7A91");
        for bad in [
            "{nope}",
            "{color0.nope}",
            "a } b",
            "{color0",
            "{}",
            "{color0:>8}",
            "{color0[0]}",
        ] {
            assert!(p.render(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn stock_templates_all_render() {
        let mut p = pal();
        let mut add = |n: String| p.vars.push((n, "#102030".into()));
        for n in ["background", "foreground", "cursor"] {
            add(n.into());
        }
        for i in 1..24 {
            add(format!("color{i}"));
        }
        for (name, tpl) in TEMPLATES {
            let out = p.render(tpl).unwrap_or_else(|| panic!("{name}"));
            assert!(
                !out.contains("{color") && !out.contains("{background"),
                "{name}"
            );
        }
    }
}
