//! The command line of `impulse-shell`, which is Quickshell's `qs` command line, so that scripts and
//! keybinds written for `qs` work with `impulse-shell` in its place: `impulse-shell [-c name | -p path]`
//! launches the shell, and `impulse-shell ... ipc call|show|prop get|wait|listen`, `impulse-shell msg` and
//! `impulse-shell kill` talk to a running instance over its socket; `impulse-shell list` and `impulse-shell log` read
//! the instance registry and `log.qslog` files. Instance selection, messages and exit codes
//! follow quickshell's launch/command.cpp and io/ipccomm.cpp. Run under the name `qs` (a link),
//! the default config is quickshell's "default" instead of the original.

use std::io::{Read, Write};
use std::os::unix::io::AsRawFd;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

use crate::ipc::{Rd, base_run_dir, clean_absolute, display_connection, md5_hex, put_str};
use crate::rilog::{Level, Out, quote, read_logs};

/// quickshell's `return -1` from main.
const FAIL: i32 = 255;

#[derive(Default, Debug, PartialEq)]
struct Opts {
    path: Option<String>,
    name: Option<String>,
    newest: bool,
    any_display: bool,
    id: Option<String>,
    pid: Option<i32>,
    show: bool,
    help: bool,
    all: bool,
    json: bool,
    show_dead: bool,
    follow: bool,
    tail: Option<String>,
    rules: Option<String>,
    no_color: bool,
    log_times: bool,
    /// `--check-tree`: report on the tree instead of launching (see [`check_tree`]).
    check_tree: bool,
    /// `-V`, `--version`: print [`version`] instead of launching.
    version: bool,
    /// Invoked as `impulse-shell`, not `qs`: the default config is the original rather than quickshell's "default".
    native: bool,
    /// Subcommand words and positional arguments, in order.
    words: Vec<String>,
}

/// Handle the command line: client subcommands run and exit here; otherwise returns the config
/// directory the native shell should load, plus the standalone app file for a `-p` path that is
/// not a `shell.qml` (settings.qml, welcome.qml, killDialog.qml), which runs as its own window.
pub fn dispatch() -> (PathBuf, Option<PathBuf>) {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some(qml::childguard::ARG) {
        std::process::exit(qml::childguard::serve());
    }
    if args.first().map(String::as_str) == Some(qml::pam::ARG) {
        qml::pam::serve();
    }
    if args.first().map(String::as_str) == Some(qml::ports::record::ARG) {
        std::process::exit(qml::ports::record::serve(&args[1..]));
    }
    if args.first().map(String::as_str) == Some(qml::jobs_wall_gcloud_trans::SWITCHWALL_ARG) {
        std::process::exit(qml::jobs_wall_gcloud_trans::switchwall_serve(&args[1..]));
    }
    let arg0 = std::env::args_os().next().unwrap_or_default();
    let native = Path::new(&arg0).file_name() != Some("qs".as_ref());
    let o = match parse(&args) {
        Ok(o) => Opts { native, ..o },
        Err(e) => {
            elog::eprintln!("{e}");
            std::process::exit(FAIL);
        }
    };
    if o.help {
        say(USAGE);
        std::process::exit(0);
    }
    if o.version {
        say(&version());
        std::process::exit(0);
    }
    match o.words.first().map(String::as_str) {
        Some("ipc" | "msg" | "kill") => {
            std::process::exit(client(&o).unwrap_or_else(|e| output_failed(&e)))
        }
        Some("list") => std::process::exit(list(&o)),
        Some("log") => std::process::exit(log(&o)),
        Some("wallpick") if native => std::process::exit(qml::jobs_wallpick::cli(
            &o.words[1..],
            &qml::xdg::shell_tree(),
        )),
        _ => {}
    }
    let (root, app_file) = launch_target(&o);
    if o.check_tree {
        std::process::exit(check_tree(&root, app_file.as_deref()));
    }
    // From here on this process is the shell: its Process children die with it.
    qml::childguard::enable(qml::childguard::self_command);
    qml::pam::enable(qml::pam::self_command);
    (root, app_file)
}

/// The config directory a launch loads and, for a `-p` path that is not a `shell.qml`, the
/// standalone app file.
fn launch_target(o: &Opts) -> (PathBuf, Option<PathBuf>) {
    let (path, name) = config_selection(o, false);
    if path.is_none() && name.is_none() {
        // Native convention: a bare directory argument, else the original.
        let root = o
            .words
            .first()
            .map(PathBuf::from)
            .unwrap_or_else(qml::xdg::shell_tree);
        return (root, None);
    }
    let file = match locate_config(path, name) {
        Ok(f) => PathBuf::from(f),
        Err(e) => {
            elog::eprintln!("{e}");
            std::process::exit(FAIL);
        }
    };
    let file = std::fs::canonicalize(&file).unwrap_or(file);
    let dir = file.parent().map_or_else(PathBuf::new, Path::to_path_buf);
    if file.file_name() == Some("shell.qml".as_ref()) {
        (dir, None)
    } else {
        (dir, Some(file))
    }
}

/// `--version`: the build, the checksum of the QML tree it was compiled from, whether it links
/// the JavaScript interpreter (tools/install.sh installs none that does), and whether this
/// machine has libthai, the optional library Thai is broken into words with (tools/install.sh
/// passes the answer on). Nothing here names a directory of the machine that built it: the
/// same commit gives the same binary wherever it is built (tools/build.sh, which sets
/// RI_COMMIT; the shipped binary does not compile without it, see main.rs).
fn version() -> String {
    format!(
        "impulse-shell {} (commit {})\nQML tree: sum {}\nJavaScript interpreter: {}\n\
         Thai line breaking: {}\nDirectories: {}\n",
        env!("CARGO_PKG_VERSION"),
        option_env!("RI_COMMIT").unwrap_or("unknown"),
        ricompiled::ROOT_SUM,
        if qml::INTERP { "linked" } else { "none" },
        if crate::line_break::thai_library() {
            "between words (libthai)"
        } else {
            "at spaces only (libthai.so.0 is not installed)"
        },
        qml::xdg::APP,
    )
}

/// `--check-tree`: says whether the scripts of the tree a launch with the same arguments would
/// load (its .qml and .js files) are the ones this binary was compiled from, without starting
/// the shell (tools/install.sh asks before it replaces the installed binary). Other files are
/// read at run time and not compared. Exit status 0 if they are, 1 with the files otherwise.
fn check_tree(root: &Path, app_file: Option<&Path>) -> i32 {
    let differing = crate::reload::differing_tree(root, app_file);
    if differing.is_empty() {
        say(&format!(
            "impulse-shell: {} has the scripts this binary was compiled from\n",
            app_file.unwrap_or(root).display()
        ));
        return 0;
    }
    elog::eprintln!(
        "impulse-shell: {} differs from the tree this binary was compiled from:",
        root.display()
    );
    const SHOWN: usize = 20;
    for d in differing.iter().take(SHOWN) {
        elog::eprintln!("  {d}");
    }
    if differing.len() > SHOWN {
        elog::eprintln!("  and {} more", differing.len() - SHOWN);
    }
    1
}

const USAGE: &str = "\
Usage: impulse-shell [-c NAME | -p PATH]                 run the shell (default config: impulse-shell)
       impulse-shell [-c NAME | -p PATH] --check-tree    exit 0 if that config has the scripts this
                                                 binary was compiled from, else list them, exit 1
       impulse-shell -V, --version                       the build, its QML tree, whether it has a
                                                 JavaScript interpreter, and how Thai is broken
                                                 (between words needs libthai)
       impulse-shell [SELECT] ipc call TARGET FUNCTION [ARGS...]
       impulse-shell [SELECT] ipc show | prop get TARGET PROP | wait TARGET SIGNAL | listen TARGET SIGNAL
       impulse-shell [SELECT] msg [-s] TARGET FUNCTION [ARGS...]
       impulse-shell [SELECT] kill
       impulse-shell list [-a | -c NAME | -p PATH] [-j] [--show-dead]
       impulse-shell log [FILE | SELECT] [-t N] [-f] [-r RULES]
       impulse-shell wallpick pick [--desktop | --words TEXT] | like | dislike [--next] |
                              revert | keep | style | reset-taste
SELECT: -c,--config NAME | -p,--path PATH | -i,--id ID | --pid PID, -n,--newest, --any-display
";

fn parse(args: &[String]) -> Result<Opts, String> {
    let mut o = Opts::default();
    let mut it = args.iter();
    let mut rest = false;
    while let Some(a) = it.next() {
        let numeric = a
            .strip_prefix('-')
            .is_some_and(|v| v.starts_with(|c: char| c.is_ascii_digit() || c == '.'));
        if rest || !a.starts_with('-') || a == "-" || numeric {
            o.words.push(a.clone());
            continue;
        }
        if a == "--" {
            rest = true;
            continue;
        }
        let (flag, mut inline) = match a.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f, Some(v.to_string())),
            _ if !a.starts_with("--") && a.len() > 2 && b"cpimtr".contains(&a.as_bytes()[1]) => {
                (&a[..2], Some(a[2..].to_string()))
            }
            _ => (a.as_str(), None),
        };
        let mut val = || {
            inline
                .take()
                .or_else(|| it.next().cloned())
                .ok_or_else(|| format!("{flag} requires an argument"))
        };
        match flag {
            "-p" | "--path" => o.path = Some(val()?),
            "-c" | "--config" => o.name = Some(val()?),
            "-i" | "--id" => o.id = Some(val()?),
            "--pid" => {
                let v = val()?;
                o.pid = Some(
                    v.parse()
                        .map_err(|_| format!("--pid: {v} is not a number"))?,
                );
            }
            "-m" | "--manifest" | "--debug" | "--log-rules" => {
                val()?;
            }
            "-t" | "--tail" => o.tail = Some(val()?),
            "-r" | "--rules" => o.rules = Some(val()?),
            "-a" | "--all" => o.all = true,
            "-j" | "--json" => o.json = true,
            "--show-dead" => o.show_dead = true,
            "-f" | "--follow" => o.follow = true,
            "--no-color" => o.no_color = true,
            "--log-times" => o.log_times = true,
            "-n" | "--newest" => o.newest = true,
            "--any-display" => o.any_display = true,
            "-s" | "--show" => o.show = true,
            "-h" | "--help" => o.help = true,
            "--check-tree" => o.check_tree = true,
            "-V" | "--version" => o.version = true,
            // -d/--daemonize, -v, ...: no native equivalent needed.
            _ => {}
        }
    }
    Ok(o)
}

/// `-p` / `-c`, falling back to `QS_CONFIG_PATH` / `QS_CONFIG_NAME` like CLI11's envname.
fn config_selection(o: &Opts, native: bool) -> (Option<String>, Option<String>) {
    let env = |k| std::env::var(k).ok().filter(|v: &String| !v.is_empty());
    match (&o.path, &o.name) {
        (None, None) => match (env("QS_CONFIG_PATH"), env("QS_CONFIG_NAME")) {
            (None, None) if native => (None, Some("impulse-shell".into())),
            sel => sel,
        },
        (p, n) => (p.clone(), n.clone()),
    }
}

/// quickshell's `configBaseDirs`, but for a relative `XDG_CONFIG_HOME`: quickshell takes the
/// variable as it is here, and native ignores a relative value here as everywhere else
/// ([`qml::xdg`], the XDG Base Directory Specification). tools/install.sh puts the impulse-shell config
/// under the same directory.
fn config_base_dirs() -> Vec<PathBuf> {
    let home = qml::xdg::config_home();
    let mut dirs = vec![home];
    let xdg = std::env::var("XDG_CONFIG_DIRS").unwrap_or_default();
    let extra: Vec<PathBuf> = xdg
        .split(':')
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .collect();
    if extra.is_empty() {
        dirs.push("/etc/xdg".into());
    } else {
        dirs.extend(extra);
    }
    dirs.into_iter().map(|d| d.join("rustshell")).collect()
}

/// Where the shell's own tree is installed: `impulse-shell` under `$XDG_DATA_HOME`, then under
/// each of `$XDG_DATA_DIRS` (a package's copy). tools/install.sh puts the tree in the first.
fn tree_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![qml::xdg::data_home()];
    let xdg = std::env::var("XDG_DATA_DIRS").unwrap_or_default();
    let extra: Vec<PathBuf> = xdg
        .split(':')
        .filter(|s| s.starts_with('/'))
        .map(PathBuf::from)
        .collect();
    if extra.is_empty() {
        dirs.extend(["/usr/local/share".into(), "/usr/share".into()]);
    } else {
        dirs.extend(extra);
    }
    dirs.into_iter().map(|d| d.join(qml::xdg::APP)).collect()
}

/// quickshell's `locateConfigFile`: the absolute, cleaned path of the shell's root QML file.
/// The config named `impulse-shell` is the shell's own tree where one is installed
/// ([`tree_dirs`]). Any other named config, and that one where no tree is installed, is looked
/// up in each base directory in turn, and not in one that holds a `shell.qml` itself
/// (`locateNamedConfig`): that file is the default config there. The deprecated
/// `manifest.conf` is not read.
fn locate_config(path: Option<String>, name: Option<String>) -> Result<String, String> {
    let mut file = match path {
        Some(p) => PathBuf::from(p),
        None => {
            let name = name.unwrap_or_else(|| "default".into());
            let mut found = None;
            if name == qml::xdg::APP {
                found = tree_dirs()
                    .into_iter()
                    .map(|d| d.join("shell.qml"))
                    .find(|f| f.is_file());
            }
            for base in config_base_dirs() {
                if found.is_some() {
                    break;
                }
                if base.join("shell.qml").is_file() {
                    if name == "default" {
                        found = Some(base.join("shell.qml"));
                        break;
                    }
                    continue;
                }
                let f = base.join(&name).join("shell.qml");
                if f.is_file() {
                    found = Some(f);
                    break;
                }
            }
            found.ok_or_else(|| {
                if name == "default" {
                    "Could not find \"default\" config directory or shell.qml in any valid config path.".to_string()
                } else {
                    format!("Could not find \"{name}\" config directory in any valid config path.")
                }
            })?
        }
    };
    if file.is_dir() {
        file = file.join("shell.qml");
    }
    if !file.is_file() {
        return Err(format!(
            "Could not open config file at \"{}\"",
            file.display()
        ));
    }
    clean_absolute(&file)
        .ok_or_else(|| format!("Could not open config file at \"{}\"", file.display()))
}

#[derive(Debug, PartialEq)]
enum Req {
    Call(String, String, Vec<String>),
    Show(String, String),
    Prop(String, String),
    Listen(String, String, bool),
    Kill,
}

fn request(words: &[String]) -> Result<Req, String> {
    let w = |i: usize| words.get(i).cloned().unwrap_or_default();
    let tail = |i: usize| words.get(i..).map(<[String]>::to_vec).unwrap_or_default();
    Ok(
        match (
            words.first().map(String::as_str),
            words.get(1).map(String::as_str),
        ) {
            (Some("kill"), _) => Req::Kill,
            (Some("msg"), _) => Req::Call(w(1), w(2), tail(3)),
            (Some("ipc"), Some("call")) => Req::Call(w(2), w(3), tail(4)),
            (Some("ipc"), Some("show")) => Req::Show(String::new(), String::new()),
            (Some("ipc"), Some("prop")) if w(2) == "get" => Req::Prop(w(3), w(4)),
            (Some("ipc"), Some("wait")) => Req::Listen(w(2), w(3), true),
            (Some("ipc"), Some("listen")) => Req::Listen(w(2), w(3), false),
            (Some("ipc"), Some("prop")) => return Err("prop: A subcommand is required".into()),
            (Some("ipc"), Some(s)) => {
                return Err(format!("The following argument was not expected: {s}"));
            }
            _ => return Err("ipc: A subcommand is required".into()),
        },
    )
}

fn client(o: &Opts) -> std::io::Result<i32> {
    let mut req = match request(&o.words) {
        Ok(r) => r,
        Err(e) => {
            elog::eprintln!("{e}\nRun with --help for more information.");
            return Ok(FAIL);
        }
    };
    if o.show
        && let Req::Call(t, f, _) = &req
    {
        req = Req::Show(t.clone(), f.clone());
    }
    let Some(inst) = select_instance(o, false, &|s| elog::eprintln!("{s}")) else {
        return Ok(FAIL);
    };
    let sock = base_run_dir().join("by-id").join(&inst.id).join("ipc.sock");
    let Ok(mut s) = UnixStream::connect(&sock) else {
        elog::eprintln!("Socket Error ServerNotFoundError");
        return Ok(FAIL);
    };
    // Like `IpcClient::connect`, a reached instance means exit code 0 whatever it answers
    // (`qs ipc call TEST_ALIVE || fuzzel` relies on it), except for a handler that threw.
    run(&mut s, &req, &inst.id)
}

/// `target.function()` of the running shell of config directory `root`, as `ipc call TARGET
/// FUNCTION` on that config: for a call made in an app window's process. The shell's answer is
/// awaited on a pool thread; a window whose stdout is closed goes on without printing it.
pub fn call_running_shell(root: &Path, target: String, function: String) {
    let o = Opts {
        path: Some(root.to_string_lossy().into_owned()),
        native: true,
        words: vec!["ipc".into(), "call".into(), target, function],
        ..Opts::default()
    };
    workpool::run(move || {
        let _ = client(&o);
    });
}

struct Instance {
    id: String,
    config_path: String,
    shell_id: String,
    launch: Launch,
    /// The pid recorded in the lock file.
    info_pid: i32,
    /// The pid holding the lock, -1 for a dead instance.
    pid: i32,
    display: Option<String>,
}

/// `InstanceInfo::launchTime`: the wall-clock time in its own zone, the instant, and the zone
/// suffix of its ISO form.
#[derive(Default)]
struct Launch {
    wall_ms: i64,
    utc_ms: i64,
    zone: String,
}

const JD_EPOCH: i64 = 2_440_588;
const DAY_MS: i64 = 86_400_000;

/// (year, month, day) of days since 1970-01-01, proleptic Gregorian.
pub(crate) fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

impl Launch {
    fn new(jd: i64, ms: i64, spec: u8, offset: i64) -> Self {
        let wall_ms = (jd - JD_EPOCH) * DAY_MS + ms;
        let (utc_ms, zone) = match spec {
            1 => (wall_ms, "Z".to_string()),
            2 if offset == 0 => (wall_ms, "Z".to_string()),
            2 => {
                let a = offset.abs();
                let sign = if offset < 0 { '-' } else { '+' };
                (
                    wall_ms - offset * 1000,
                    format!("{sign}{:02}:{:02}", a / 3600, a / 60 % 60),
                )
            }
            _ => (Self::local_to_utc(wall_ms), String::new()),
        };
        Launch {
            wall_ms,
            utc_ms,
            zone,
        }
    }

    /// The instant at which the clocks of the zone `Date` reads show `wall_ms`.
    fn local_to_utc(wall_ms: i64) -> i64 {
        let wall = wall_ms.div_euclid(1000);
        let zone = qml::tz::Database::system().local();
        let secs = zone.map_or(wall, |zone| zone.instant_of(wall));
        secs * 1000 + wall_ms.rem_euclid(1000)
    }

    /// `toString(fmt)` with `sep` between date and time (`yyyy-MM-dd hh:mm:ss` or ISO).
    fn text(&self, sep: char) -> String {
        let (y, m, d) = civil(self.wall_ms.div_euclid(DAY_MS));
        let s = self.wall_ms.rem_euclid(DAY_MS) / 1000;
        format!(
            "{y:04}-{m:02}-{d:02}{sep}{:02}:{:02}:{:02}",
            s / 3600,
            s / 60 % 60,
            s % 60
        )
    }
}

/// `QsPaths::checkLock` with dead instances allowed (pid -1).
fn check_lock(dir: &Path) -> Option<Instance> {
    let mut f = std::fs::File::open(dir.join("instance.lock")).ok()?;
    let mut fl = libc::flock {
        l_type: libc::F_WRLCK as _,
        l_whence: libc::SEEK_SET as _,
        l_start: 0,
        l_len: 0,
        l_pid: 0,
    };
    // SAFETY: F_GETLK on the open file takes a pointer to an initialized flock it may overwrite.
    unsafe { libc::fcntl(f.as_raw_fd(), libc::F_GETLK, &mut fl) };
    let pid = if fl.l_type == libc::F_UNLCK as libc::c_short {
        -1
    } else {
        fl.l_pid
    };
    let mut b = Vec::new();
    f.read_to_end(&mut b).ok()?;
    let mut r = Rd::new(&b);
    let id = r.string()?;
    let config_path = r.string()?;
    let shell_id = r.string()?;
    r.string()?; // appId
    let jd = i64::from_be_bytes(r.take(8)?.try_into().ok()?);
    let ms = i64::from(r.u32()?);
    let spec = r.u8()?;
    let offset = match spec {
        2 => i64::from(i32::from_be_bytes(r.take(4)?.try_into().ok()?)),
        _ => 0,
    };
    let launch = Launch::new(jd, ms, spec, offset);
    // Qt::TimeZone carries a zone id; skip the rest rather than misparse.
    let (info_pid, display) = if spec <= 2 {
        let p = i32::from_be_bytes(r.take(4)?.try_into().ok()?);
        (p, r.string())
    } else {
        (0, None)
    };
    Some(Instance {
        id,
        config_path,
        shell_id,
        launch,
        info_pid,
        pid,
        display,
    })
}

/// `QsPaths::collectInstances`: (live, other display, dead), in `QDir` name order.
fn collect(dir: &Path, display: &str) -> (Vec<Instance>, Vec<Instance>, Vec<Instance>) {
    let (mut live, mut other, mut dead) = (Vec::new(), Vec::new(), Vec::new());
    let Ok(rd) = std::fs::read_dir(dir) else {
        return (live, other, dead);
    };
    let mut names: Vec<String> = rd
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| !n.starts_with('.'))
        .collect();
    names.sort_by_key(|n| n.to_lowercase());
    for n in names {
        let p = dir.join(n);
        if !p.is_dir() {
            continue;
        }
        let Some(i) = check_lock(&p) else { continue };
        if i.pid == -1 {
            dead.push(i);
        } else if !display.is_empty() && i.display.as_deref().is_some_and(|d| d != display) {
            other.push(i);
        } else {
            live.push(i);
        }
    }
    (live, other, dead)
}

/// `sortInstances`: oldest first unless `newest`.
fn sort_instances(v: &mut [Instance], newest: bool) {
    v.sort_by_key(|i| i.launch.utc_ms);
    if newest {
        v.reverse();
    }
}

/// `selectInstance`; `say` prints its `logBare` messages, `dead_fallback` lets `qs log` pick a
/// dead instance when no live one matches.
fn select_instance(o: &Opts, dead_fallback: bool, say: &dyn Fn(&str)) -> Option<Instance> {
    let base = base_run_dir();
    let display = if o.any_display {
        String::new()
    } else {
        display_connection()
    };
    if let Some(pid) = o.pid {
        let i = check_lock(&base.join("by-pid").join(pid.to_string()))
            .filter(|i| dead_fallback || i.pid != -1);
        if i.is_none() {
            say(&format!("No instance found for pid {pid}"));
        }
        return i;
    }
    if let Some(prefix) = o.id.as_deref().filter(|s| !s.is_empty()) {
        let (mut live, mut other, mut dead) = collect(&base.join("by-pid"), &display);
        for v in [&mut live, &mut other, &mut dead] {
            v.retain(|i| i.id.starts_with(prefix));
        }
        let mut found = if live.is_empty() && dead_fallback {
            std::mem::take(&mut dead)
        } else {
            live
        };
        let q = quote(prefix);
        if found.is_empty() {
            if !other.is_empty() {
                say(&format!(
                    "No running instances on the current display {} start with {q}",
                    quote(&display_connection())
                ));
                say("Some instances on other displays match:");
                for i in &other {
                    say(&format!(
                        " - {} ({})",
                        i.id,
                        i.display.as_deref().unwrap_or("")
                    ));
                }
            }
            if dead_fallback {
                say(&format!("No instances start with {q}"));
            } else {
                say(&format!("No running instances start with {q}"));
                if !dead.is_empty() {
                    say("Some dead instances match:");
                    for i in &dead {
                        say(&format!(" - {}", i.id));
                    }
                }
            }
            return None;
        }
        if found.len() != 1 {
            say(&format!("More than one instance starts with {q}"));
            for i in &found {
                say(&format!(
                    " - {}{}",
                    i.id,
                    if i.pid == -1 { " (dead)" } else { "" }
                ));
            }
            return None;
        }
        return found.pop();
    }
    let (path, name) = config_selection(o, o.native);
    let config = match locate_config(path, name) {
        Ok(c) => c,
        Err(e) => {
            say(&e);
            return None;
        }
    };
    let dir = base.join("by-path").join(md5_hex(config.as_bytes()));
    let (live, other, mut dead) = collect(&dir, &display);
    let live_empty = live.is_empty();
    let mut found = if live_empty && dead_fallback {
        std::mem::take(&mut dead)
    } else {
        live
    };
    let newest = o.newest || found.first().is_some_and(|i| i.pid == -1);
    sort_instances(&mut found, newest);
    if found.is_empty() {
        let q = quote(&config);
        if live_empty && dead.len() > 1 {
            say(&format!("No running instances for {q}"));
            say("Dead instances:");
            sort_instances(&mut dead, o.newest);
            for i in &dead {
                say(&format!(" - {}", i.id));
            }
        } else if !other.is_empty() {
            say(&format!(
                "No running instances for {q}  present on the current display {}",
                quote(&display_connection())
            ));
            say("Some instances on other displays match:");
            for i in &other {
                say(&format!(
                    " - {} ({})",
                    i.id,
                    i.display.as_deref().unwrap_or("")
                ));
            }
        } else {
            say(&format!("No running instances for {q}"));
        }
        return None;
    }
    Some(found.swap_remove(0))
}

/// Output settings of `qs list` / `qs log` (`LogManager::init` with `--no-color`, `NO_COLOR`,
/// `--log-times`).
fn log_out(o: &Opts, prefix: &'static str) -> Out {
    let no_color_env = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
    Out {
        color: !o.no_color && !no_color_env,
        timestamps: o.log_times,
        prefix,
    }
}

/// CLI11's error output and exit code for an invalid command line.
fn cli_error(msg: &str, code: i32) -> i32 {
    elog::eprintln!("{msg}\nRun with --help for more information.");
    code
}

/// `qs list` (`listInstances`).
fn list(o: &Opts) -> i32 {
    if o.all && (o.path.is_some() || o.name.is_some() || o.newest || o.any_display) {
        return cli_error("[Option Group: Config Selection] excludes --all", 108);
    }
    let out = log_out(o, "");
    let base = base_run_dir();
    let mut config = String::new();
    let dir = if o.all {
        base.join("by-pid")
    } else {
        let (path, name) = config_selection(o, o.native);
        match locate_config(path, name) {
            Ok(c) => config = c,
            Err(e) => {
                out.bare(&e);
                out.bare("Use --all to list all instances.");
                return FAIL;
            }
        }
        base.join("by-path").join(md5_hex(config.as_bytes()))
    };
    let display = if o.any_display || o.all {
        String::new()
    } else {
        display_connection()
    };
    let (mut live, _, mut dead) = collect(&dir, &display);
    sort_instances(&mut live, o.newest);
    let instances = if o.show_dead {
        sort_instances(&mut dead, o.newest);
        dead.extend(live);
        dead
    } else {
        live
    };
    if instances.is_empty() {
        if o.all {
            out.bare("No running instances.");
        } else {
            out.bare(&format!("No running instances for {}", quote(&config)));
            out.bare("Use --all to list all instances.");
        }
        return 0;
    }
    if o.json {
        let arr: Vec<serde_json::Value> = instances
            .iter()
            .map(|i| {
                let mut m = serde_json::Map::new();
                m.insert("config_path".into(), i.config_path.clone().into());
                m.insert("id".into(), i.id.clone().into());
                m.insert(
                    "launch_time".into(),
                    format!("{}{}", i.launch.text('T'), i.launch.zone).into(),
                );
                m.insert("pid".into(), i.info_pid.into());
                m.insert("shell_id".into(), i.shell_id.clone().into());
                serde_json::Value::Object(m)
            })
            .collect();
        let mut b = Vec::new();
        let fmt = serde_json::ser::PrettyFormatter::with_indent(b"    ");
        let mut ser = serde_json::Serializer::with_formatter(&mut b, fmt);
        if serde::Serialize::serialize(&arr, &mut ser).is_err() {
            return FAIL;
        }
        b.push(b'\n');
        let mut so = std::io::stdout().lock();
        let _ = so.write_all(&b);
        let _ = so.flush();
        return 0;
    }
    let now = crate::rilog::now_ms();
    for i in &instances {
        let run = (now - i.launch.utc_ms) / 1000;
        let (secs, mins) = (run % 60, (run - run % 60) / 60);
        let hours = (mins - mins % 60) / 60;
        let dead = i.pid == -1;
        let gray = out.color && dead;
        out.bare(&format!(
            "{}Instance {}{}:\n  Process ID: {}\n  Shell ID: {}\n  Config path: {}\n  Display connection: {}\n  Launch time: {}{}\n{}",
            if gray { "\x1b[90m" } else { "" },
            i.id,
            if dead { " (dead)" } else { "" },
            i.info_pid,
            i.shell_id,
            i.config_path,
            i.display.as_deref().unwrap_or(""),
            i.launch.text(' '),
            if dead {
                String::new()
            } else {
                format!(" (running for {hours} hours, {} minutes, {secs} seconds)", mins % 60)
            },
            if gray { "\x1b[0m" } else { "" },
        ));
    }
    0
}

/// `qs log` (`readLogFile`).
fn log(o: &Opts) -> i32 {
    let tail = match o.tail.as_deref() {
        None => 0,
        Some(t) => match t.parse::<i64>() {
            Ok(n) if (1..=i64::from(i32::MAX)).contains(&n) => n as usize,
            _ => {
                return cli_error(
                    &format!("--tail: Value {t} not in range [1 - 2147483647]"),
                    105,
                );
            }
        },
    };
    let file = o.words.len() > 1;
    let instance = o.id.is_some() || o.pid.is_some();
    let config = o.path.is_some() || o.name.is_some() || o.newest || o.any_display;
    let excluded = match (instance, config) {
        (true, false) if file => Some("[Option Group: Instance Selection] excludes file"),
        (true, true) => {
            Some("[Option Group: Instance Selection] excludes [Option Group: Config Selection]")
        }
        (false, true) if file => Some("[Option Group: Config Selection] excludes file"),
        _ => None,
    };
    if let Some(msg) = excluded {
        return cli_error(msg, 108);
    }
    let out = log_out(o, "READER");
    let path = match o.words.get(1).filter(|f| !f.is_empty()) {
        Some(f) => PathBuf::from(f),
        None => {
            let Some(inst) = select_instance(o, true, &|s| out.bare(s)) else {
                return FAIL;
            };
            base_run_dir()
                .join("by-id")
                .join(&inst.id)
                .join("log.qslog")
        }
    };
    let Ok(file) = std::fs::File::open(&path) else {
        out.emit(
            Level::Critical,
            crate::rilog::BARE,
            &format!("Failed to open log file {}", quote(&path.to_string_lossy())),
        );
        return FAIL;
    };
    let rules = o.rules.as_deref().unwrap_or("");
    if read_logs(file, &path, &out, tail, o.follow, rules) {
        0
    } else {
        FAIL
    }
}

/// Read until `f` parses one whole response (QDataStream transaction semantics).
fn recv<T>(s: &mut UnixStream, buf: &mut Vec<u8>, f: impl Fn(&mut Rd) -> Option<T>) -> Option<T> {
    let mut chunk = [0u8; 4096];
    loop {
        let mut r = Rd::new(buf);
        if let Some(v) = f(&mut r) {
            let used = r.p;
            buf.drain(..used);
            return Some(v);
        }
        match s.read(&mut chunk) {
            Ok(0) | Err(_) => {
                elog::eprintln!("Error occurred while waiting for response.");
                return None;
            }
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
}

fn vec<T>(r: &mut Rd, f: impl Fn(&mut Rd) -> Option<T>) -> Option<Vec<T>> {
    let n = r.u32()?;
    (0..n).map(|_| f(r)).collect()
}

struct FnDef {
    name: String,
    ret: String,
    args: Vec<(String, String)>,
}

impl FnDef {
    fn read(r: &mut Rd) -> Option<Self> {
        Some(FnDef {
            name: r.string()?,
            ret: r.string()?,
            args: vec(r, |r| Some((r.string()?, r.string()?)))?,
        })
    }
    fn text(&self) -> String {
        let params: Vec<String> = self.args.iter().map(|(n, t)| format!("{n}: {t}")).collect();
        format!(
            "function {}({}): {}",
            self.name,
            params.join(", "),
            self.ret
        )
    }
}

fn prop_def(r: &mut Rd) -> Option<String> {
    Some(format!("property {}: {}", r.string()?, r.string()?))
}

fn target_def(r: &mut Rd) -> Option<String> {
    let mut s = format!("target {}", r.string()?);
    let fns = vec(r, FnDef::read)?;
    let props = vec(r, prop_def)?;
    let sigs = vec(r, |r| {
        let (n, rn, rt) = (r.string()?, r.string()?, r.string()?);
        Some(if rt.is_empty() {
            format!("signal {n}()")
        } else {
            format!("signal {n}({rn}: {rt})")
        })
    })?;
    for l in fns.iter().map(FnDef::text).chain(props).chain(sigs) {
        s.push_str("\n  ");
        s.push_str(&l);
    }
    Some(s)
}

fn common_error(tag: u8, entry: &str) {
    match tag {
        1 => elog::eprintln!("Not ready to accept queries yet."),
        2 => elog::eprintln!("Target not found."),
        3 => elog::eprintln!("{entry} not found."),
        _ => elog::eprintln!("Received invalid IPC response"),
    }
}

fn two(tag: u8, a: &str, b: &str) -> Vec<u8> {
    let mut m = vec![tag];
    put_str(&mut m, a);
    put_str(&mut m, b);
    m
}

/// Send `req` and print the answer as quickshell's client does; the exit code, or the error
/// of writing the answer to stdout.
fn run(s: &mut UnixStream, req: &Req, id: &str) -> std::io::Result<i32> {
    let mut buf = Vec::new();
    let mut send = |m: &[u8]| s.write_all(m).is_ok();
    match req {
        Req::Kill => {
            if send(&[1]) {
                let mut sink = [0u8; 64];
                while matches!(s.read(&mut sink), Ok(n) if n > 0) {}
            }
            elog::eprintln!("Killed {id}");
        }
        Req::Call(t, f, args) => {
            if t.is_empty() {
                elog::eprintln!("Target required to send message.");
                return Ok(0);
            }
            if f.is_empty() {
                elog::eprintln!("Function required to send message.");
                return Ok(0);
            }
            let mut m = two(3, t, f);
            m.extend((args.len() as u32).to_be_bytes());
            for a in args {
                put_str(&mut m, a);
            }
            if !send(&m) {
                return Ok(0);
            }
            enum R {
                Fail(u8),
                Parse(FnDef, bool, u8),
                Done(bool, String),
                Threw(String),
            }
            let resp = recv(s, &mut buf, |r| {
                Some(match r.u8()? {
                    4 => R::Parse(FnDef::read(r)?, r.u8()? != 0, r.u8()?),
                    5 => R::Done(r.u8()? != 0, r.string()?),
                    crate::ipc::CALL_THREW => R::Threw(r.string()?),
                    t => R::Fail(t),
                })
            });
            match resp {
                None => {}
                Some(R::Threw(e)) => {
                    elog::eprintln!("Function threw an error: {e}");
                    return Ok(FAIL);
                }
                Some(R::Done(void, v)) => {
                    if !void {
                        put_line(&v)?;
                    }
                }
                Some(R::Parse(def, count, idx)) => {
                    if count {
                        let need = def.args.len();
                        let many = if need < args.len() { "many" } else { "few" };
                        elog::eprintln!(
                            "Too {many} arguments provided ({need} required but {} were provided.)",
                            args.len()
                        );
                    } else {
                        let i = idx as usize;
                        let ty = def.args.get(i).map_or("", |a| a.1.as_str());
                        let given = args.get(i).map_or("", String::as_str);
                        elog::eprintln!(
                            "Unable to parse argument {} as {ty}. Provided argument: \"{given}\"",
                            i + 1
                        );
                    }
                    elog::eprintln!("Function definition: {}", def.text());
                }
                Some(R::Fail(t)) => common_error(t, "Function"),
            }
        }
        Req::Show(t, n) => {
            if !send(&two(2, t, n)) {
                return Ok(0);
            }
            let resp = recv(s, &mut buf, |r| {
                Some(match r.u8()? {
                    4 => Ok(vec(r, target_def)?),
                    5 => Ok(vec![target_def(r)?]),
                    6 => Ok(vec![FnDef::read(r)?.text()]),
                    7 => Ok(vec![prop_def(r)?]),
                    t => Err(t),
                })
            });
            match resp {
                Some(Ok(lines)) => lines.iter().for_each(|l| elog::eprintln!("{l}")),
                Some(Err(t)) => common_error(t, "Function"),
                None => {}
            }
        }
        Req::Prop(t, p) => {
            if t.is_empty() {
                elog::eprintln!("Target required to send message.");
                return Ok(0);
            }
            if p.is_empty() {
                elog::eprintln!("Property required to send message.");
                return Ok(0);
            }
            if !send(&two(5, t, p)) {
                return Ok(0);
            }
            match recv(s, &mut buf, |r| match r.u8()? {
                4 => Some(Ok(r.string()?)),
                t => Some(Err(t)),
            }) {
                Some(Ok(v)) => put_line(&v)?,
                Some(Err(t)) => common_error(t, "Property"),
                None => {}
            }
        }
        Req::Listen(t, sig, once) => {
            if t.is_empty() {
                elog::eprintln!("Target required to listen for signals.");
                return Ok(0);
            }
            if sig.is_empty() {
                elog::eprintln!("Signal required to listen.");
                return Ok(0);
            }
            if !send(&two(4, t, sig)) {
                return Ok(0);
            }
            loop {
                match recv(s, &mut buf, |r| match r.u8()? {
                    4 => Some(Ok(r.string()?)),
                    t => Some(Err(t)),
                }) {
                    Some(Ok(v)) => {
                        put_line(&v)?;
                        if *once {
                            return Ok(0);
                        }
                    }
                    Some(Err(t)) => {
                        common_error(t, "Signal");
                        return Ok(0);
                    }
                    None => return Ok(0),
                }
            }
        }
    }
    Ok(0)
}

/// Writes `text` to stdout. `print!` panics when the write fails; SIGPIPE is ignored in Rust
/// programs, so the write to a pipe whose reader is gone (`impulse-shell -V | true`) fails with EPIPE.
fn put(text: &str) -> std::io::Result<()> {
    let mut so = std::io::stdout().lock();
    so.write_all(text.as_bytes())?;
    so.flush()
}

fn put_line(line: &str) -> std::io::Result<()> {
    put(&format!("{line}\n"))
}

/// [`put`] for a command that only prints: ends it if the text cannot be written.
fn say(text: &str) {
    if let Err(e) = put(text) {
        output_failed(&e);
    }
}

/// Ends a command whose output could not be written to stdout. A closed pipe ends it by
/// SIGPIPE, as it ends `qs` and every program that keeps the signal's default action (the
/// shell reports 141 and prints nothing); any other error (a full disk) is reported on stderr
/// with the failure status.
fn output_failed(e: &std::io::Error) -> ! {
    if e.kind() == std::io::ErrorKind::BrokenPipe {
        crate::shutdown::exit_by(libc::SIGPIPE);
    }
    elog::eprintln!("impulse-shell: cannot write to the standard output: {e}");
    std::process::exit(FAIL);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn check_tree_selects_the_config_as_a_launch_does() {
        let o = parse(&args("--check-tree -p /x/impulse-shell")).unwrap();
        assert!(o.check_tree && o.words.is_empty());
        assert_eq!(o.path.as_deref(), Some("/x/impulse-shell"));
        let o = parse(&args("-c impulse-shell")).unwrap();
        assert!(!o.check_tree);
    }

    #[test]
    fn parses_hyprland_calls() {
        let o = parse(&args("-c impulse-shell ipc call sidebarLeft toggle")).unwrap();
        assert_eq!(o.name.as_deref(), Some("impulse-shell"));
        assert_eq!(
            request(&o.words).unwrap(),
            Req::Call("sidebarLeft".into(), "toggle".into(), vec![])
        );
        let o = parse(&args(
            "-p /x/impulse-shell/ ipc call brightness increment -5",
        ))
        .unwrap();
        assert_eq!(o.path.as_deref(), Some("/x/impulse-shell/"));
        assert_eq!(
            request(&o.words).unwrap(),
            Req::Call("brightness".into(), "increment".into(), vec!["-5".into()])
        );
        let o = parse(&args(
            "ipc --config=impulse-shell --newest prop get bar visible",
        ))
        .unwrap();
        assert!(o.newest && o.name.as_deref() == Some("impulse-shell"));
        assert_eq!(
            request(&o.words).unwrap(),
            Req::Prop("bar".into(), "visible".into())
        );
        let o = parse(&args("-cimpulse-shell ipc call x y -- -n")).unwrap();
        assert!(!o.newest && o.name.as_deref() == Some("impulse-shell"));
        assert_eq!(
            request(&o.words).unwrap(),
            Req::Call("x".into(), "y".into(), vec!["-n".into()])
        );
        let o = parse(&args("ipc call TEST_ALIVE")).unwrap();
        if std::env::var_os("QS_CONFIG_NAME").is_none()
            && std::env::var_os("QS_CONFIG_PATH").is_none()
        {
            assert_eq!(
                config_selection(&o, true),
                (None, Some("impulse-shell".into()))
            );
            assert_eq!(config_selection(&o, false), (None, None));
        }
        assert_eq!(
            request(&o.words).unwrap(),
            Req::Call("TEST_ALIVE".into(), "".into(), vec![])
        );
        assert!(request(&args("ipc")).is_err());
        assert_eq!(request(&args("kill")).unwrap(), Req::Kill);
    }

    #[test]
    fn non_ascii_args_do_not_panic() {
        let o = parse(&args("ipc call t f привет -ё ё -cпривет")).unwrap();
        assert_eq!(o.name.as_deref(), Some("привет"));
        assert_eq!(
            request(&o.words).unwrap(),
            Req::Call("t".into(), "f".into(), vec!["привет".into(), "ё".into()])
        );
        let o = parse(&args("msg t f — -.5")).unwrap();
        assert_eq!(o.words, args("msg t f — -.5"));
    }

    #[test]
    fn locates_named_and_path_configs() {
        let d = std::env::temp_dir().join(format!("ricli-{}", std::process::id()));
        std::fs::create_dir_all(d.join("impulse-shell")).unwrap();
        std::fs::write(d.join("impulse-shell/shell.qml"), "").unwrap();
        let p = d.join("impulse-shell/").to_string_lossy().into_owned();
        let want = clean_absolute(&d.join("impulse-shell/shell.qml")).unwrap();
        assert_eq!(locate_config(Some(p), None).unwrap(), want);
        assert!(locate_config(Some(d.join("nope").to_string_lossy().into()), None).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }
}
